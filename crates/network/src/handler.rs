use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, Sink, SinkExt, StreamExt};
use pgwire::api::auth::{DefaultServerParameterProvider, LoginInfo, StartupHandler};
use pgwire::api::copy::NoopCopyHandler;
use pgwire::api::portal::Portal;
use pgwire::api::query::{ExtendedQueryHandler, SimpleQueryHandler};
use pgwire::api::results::DescribeResponse;
use pgwire::api::results::{
    DataRowEncoder, DescribePortalResponse, DescribeStatementResponse, FieldFormat, FieldInfo,
    QueryResponse, Response, Tag,
};
use pgwire::api::stmt::{QueryParser, StoredStatement};
use pgwire::api::{ClientInfo, ClientPortalStore, PgWireHandlerFactory, Type};
use pgwire::error::{ErrorInfo, PgWireError, PgWireResult};
use pgwire::messages::startup::Authentication;
use pgwire::messages::{PgWireBackendMessage, PgWireFrontendMessage};

use auth::Authenticator;
use catalog::DataType;
use sql::db_manager::DatabaseManager;
use sql::executor::ExecutionResult;
use sql::value::Value;

// ── Query parser ──────────────────────────────────────────────────────────────

/// A simple query parser that stores the SQL string as-is.
pub struct IceDbQueryParser;

#[async_trait]
impl QueryParser for IceDbQueryParser {
    type Statement = String;

    async fn parse_sql(&self, sql: &str, _types: &[Type]) -> PgWireResult<Self::Statement> {
        Ok(sql.to_owned())
    }
}

// ── Main handler ──────────────────────────────────────────────────────────────

pub struct IceDbHandler {
    pub db_manager: Arc<DatabaseManager>,
    pub authenticator: Arc<Authenticator>,
    query_parser: Arc<IceDbQueryParser>,
    /// Unique ID for this connection, used to track multi-statement transactions.
    session_id: String,
}

impl IceDbHandler {
    pub fn new(db_manager: Arc<DatabaseManager>, authenticator: Arc<Authenticator>) -> Self {
        Self {
            db_manager,
            authenticator,
            query_parser: Arc::new(IceDbQueryParser),
            session_id: new_session_id(),
        }
    }
}

fn new_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    format!("sess-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

impl Drop for IceDbHandler {
    fn drop(&mut self) {
        // Best-effort abort: try the default engine. Per-db sessions will be
        // cleaned up when the engine is dropped.
        if let Ok(engine) = self.db_manager.get_or_open("icedb") {
            engine.abort_session(&self.session_id);
        }
    }
}

// ── Startup / Auth ────────────────────────────────────────────────────────────

/// Custom startup handler that performs cleartext password authentication
/// against the IceDb catalog.
pub struct IceDbStartupHandler {
    authenticator: Arc<Authenticator>,
    param_provider: DefaultServerParameterProvider,
}

impl IceDbStartupHandler {
    pub fn new(authenticator: Arc<Authenticator>) -> Self {
        let mut provider = DefaultServerParameterProvider::default();
        provider.server_version = "16.0 (icedb)".to_owned();
        Self {
            authenticator,
            param_provider: provider,
        }
    }
}

#[async_trait]
impl StartupHandler for IceDbStartupHandler {
    async fn on_startup<C>(
        &self,
        client: &mut C,
        message: PgWireFrontendMessage,
    ) -> PgWireResult<()>
    where
        C: ClientInfo + Sink<PgWireBackendMessage> + Unpin + Send,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        match message {
            PgWireFrontendMessage::Startup(ref startup) => {
                pgwire::api::auth::save_startup_parameters_to_metadata(client, startup);
                client.set_state(pgwire::api::PgWireConnectionState::AuthenticationInProgress);
                client
                    .send(PgWireBackendMessage::Authentication(
                        Authentication::CleartextPassword,
                    ))
                    .await?;
            }
            PgWireFrontendMessage::PasswordMessageFamily(pwd) => {
                let pwd = pwd.into_password()?;
                let login_info = LoginInfo::from_client_info(client);
                let username = login_info.user().unwrap_or("icedb");
                let password = std::str::from_utf8(pwd.password.as_bytes()).unwrap_or("");

                match self.authenticator.authenticate(username, password) {
                    Ok(()) => {
                        pgwire::api::auth::finish_authentication(client, &self.param_provider)
                            .await?;
                    }
                    Err(e) => {
                        let error_info =
                            ErrorInfo::new("FATAL".to_owned(), "28P01".to_owned(), e.to_string());
                        client
                            .feed(PgWireBackendMessage::ErrorResponse(error_info.into()))
                            .await?;
                        client.close().await?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

// ── Value helpers ─────────────────────────────────────────────────────────────

fn value_to_pg_type(value: &Value) -> Type {
    match value {
        Value::Bool(_) => Type::BOOL,
        Value::Int4(_) => Type::INT4,
        Value::Int8(_) => Type::INT8,
        Value::Float8(_) => Type::FLOAT8,
        Value::Text(_) => Type::TEXT,
        Value::Bytes(_) => Type::BYTEA,
        Value::Date(_) => Type::DATE,
        Value::Timestamp(_) => Type::TIMESTAMP,
        Value::Numeric(_) => Type::NUMERIC,
        Value::Uuid(_) => Type::UUID,
        Value::Null => Type::TEXT,
    }
}

fn datatype_to_pg_type(dt: &DataType) -> Type {
    match dt {
        DataType::Boolean => Type::BOOL,
        DataType::Int4 => Type::INT4,
        DataType::Int8 => Type::INT8,
        DataType::Float8 => Type::FLOAT8,
        DataType::Text | DataType::VarChar(_) => Type::TEXT,
        DataType::Bytea => Type::BYTEA,
        DataType::Date => Type::DATE,
        DataType::Timestamp | DataType::TimestampTz => Type::TIMESTAMP,
        DataType::Numeric => Type::NUMERIC,
        DataType::Uuid => Type::UUID,
    }
}

fn build_field_infos(result: &ExecutionResult) -> Vec<FieldInfo> {
    if let Some(first_row) = result.rows.first() {
        // Derive types from actual row values
        first_row
            .schema
            .iter()
            .enumerate()
            .map(|(i, (col_name, _dtype))| {
                let pg_type = first_row.values.get(i).map(value_to_pg_type).unwrap_or(Type::TEXT);
                FieldInfo::new(col_name.clone(), None, None, pg_type, FieldFormat::Text)
            })
            .collect()
    } else {
        // No rows — use col_names + col_types from the plan
        result
            .col_names
            .iter()
            .zip(result.col_types.iter().chain(std::iter::repeat(&DataType::Text)))
            .map(|(name, dt)| FieldInfo::new(name.clone(), None, None, datatype_to_pg_type(dt), FieldFormat::Text))
            .collect()
    }
}

fn encode_value(encoder: &mut DataRowEncoder, value: &Value) -> PgWireResult<()> {
    match value {
        Value::Null => encoder.encode_field(&None::<String>),
        Value::Bool(b) => encoder.encode_field(b),
        Value::Int4(v) => encoder.encode_field(v),
        Value::Int8(v) => encoder.encode_field(v),
        Value::Float8(v) => encoder.encode_field(v),
        Value::Text(s) => encoder.encode_field(s),
        Value::Bytes(_)
        | Value::Date(_)
        | Value::Timestamp(_)
        | Value::Numeric(_)
        | Value::Uuid(_) => {
            let s = value.to_string();
            encoder.encode_field(&s)
        }
    }
}

fn execution_result_to_responses(result: ExecutionResult) -> Vec<Response<'static>> {
    if result.rows.is_empty() && result.col_names.is_empty() {
        // DDL / DML with no result set — just send CommandComplete
        let tag = Tag::new(&result.command);
        return vec![Response::Execution(tag)];
    }

    if result.rows.is_empty() {
        // SELECT that returned 0 rows — still send RowDescription + empty data + CommandComplete
        let field_infos = build_field_infos(&result);
        let schema = Arc::new(field_infos);
        let empty_stream = stream::empty();
        return vec![Response::Query(QueryResponse::new(schema, empty_stream))];
    }

    let field_infos = build_field_infos(&result);
    let schema = Arc::new(field_infos);

    let rows = result.rows;
    let schema_ref = schema.clone();
    let data_row_stream = stream::iter(rows).map(move |row| {
        let mut encoder = DataRowEncoder::new(schema_ref.clone());
        for value in &row.values {
            encode_value(&mut encoder, value)?;
        }
        encoder.finish()
    });

    vec![Response::Query(QueryResponse::new(schema, data_row_stream))]
}

// ── Simple query handler ──────────────────────────────────────────────────────

#[async_trait]
impl SimpleQueryHandler for IceDbHandler {
    async fn do_query<'a, C>(
        &self,
        client: &mut C,
        query: &'a str,
    ) -> PgWireResult<Vec<Response<'a>>>
    where
        C: ClientInfo + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        let db_name = client.metadata().get("database").map(|s| s.as_str()).unwrap_or("icedb");
        let engine = self.db_manager.get_or_open(db_name).map_err(|e| {
            PgWireError::UserError(Box::new(ErrorInfo::new(
                "FATAL".to_owned(),
                "3D000".to_owned(),
                e.to_string(),
            )))
        })?;
        let results = engine.execute_session_multi(&self.session_id, query).map_err(|e| {
            let sqlstate = e.sqlstate().to_owned();
            PgWireError::UserError(Box::new(ErrorInfo::new(
                "ERROR".to_owned(),
                sqlstate,
                e.to_string(),
            )))
        })?;

        let responses: Vec<Response<'static>> = results
            .into_iter()
            .flat_map(execution_result_to_responses)
            .collect();
        Ok(responses)
    }
}

// ── Extended query handler ────────────────────────────────────────────────────

/// Substitute $1, $2, … placeholders in SQL with bound parameter values.
fn substitute_params(sql: &str, params: &[Option<bytes::Bytes>]) -> String {
    let mut result = sql.to_string();
    for (i, param) in params.iter().enumerate() {
        let placeholder = format!("${}", i + 1);
        let value = match param {
            None => "NULL".to_string(),
            Some(bytes) => {
                if let Ok(s) = std::str::from_utf8(bytes) {
                    format!("'{}'", s.replace('\'', "''"))
                } else {
                    // Encode as hex literal
                    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
                    format!("E'\\x{}'", hex)
                }
            }
        };
        result = result.replace(&placeholder, &value);
    }
    result
}

#[async_trait]
impl ExtendedQueryHandler for IceDbHandler {
    type Statement = String;
    type QueryParser = IceDbQueryParser;

    fn query_parser(&self) -> Arc<Self::QueryParser> {
        self.query_parser.clone()
    }

    async fn do_describe_statement<C>(
        &self,
        _client: &mut C,
        _stmt: &StoredStatement<Self::Statement>,
    ) -> PgWireResult<DescribeStatementResponse>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        // Return empty describe response (no parameter types inferred, no fields yet)
        Ok(<DescribeStatementResponse as DescribeResponse>::no_data())
    }

    async fn do_describe_portal<C>(
        &self,
        _client: &mut C,
        portal: &Portal<Self::Statement>,
    ) -> PgWireResult<DescribePortalResponse>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        // Execute with substituted params to get schema, then return field info
        let sql = substitute_params(&portal.statement.statement, &portal.parameters);
        let engine = self.db_manager.get_or_open("icedb").map_err(|e| {
            PgWireError::UserError(Box::new(ErrorInfo::new(
                "FATAL".to_owned(), "3D000".to_owned(), e.to_string(),
            )))
        })?;
        let result = engine.execute(&sql).map_err(|e| {
            let sqlstate = e.sqlstate().to_owned();
            PgWireError::UserError(Box::new(ErrorInfo::new(
                "ERROR".to_owned(),
                sqlstate,
                e.to_string(),
            )))
        });

        match result {
            Ok(r) => {
                let fields = build_field_infos(&r);
                Ok(DescribePortalResponse::new(fields))
            }
            Err(_) => Ok(<DescribePortalResponse as DescribeResponse>::no_data()),
        }
    }

    async fn do_query<'a, 'b: 'a, C>(
        &'b self,
        _client: &mut C,
        portal: &'a Portal<Self::Statement>,
        _max_rows: usize,
    ) -> PgWireResult<Response<'a>>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        let sql = substitute_params(&portal.statement.statement, &portal.parameters);

        let engine = self.db_manager.get_or_open("icedb").map_err(|e| {
            PgWireError::UserError(Box::new(ErrorInfo::new(
                "FATAL".to_owned(), "3D000".to_owned(), e.to_string(),
            )))
        })?;
        let result = engine.execute(&sql).map_err(|e| {
            let sqlstate = e.sqlstate().to_owned();
            PgWireError::UserError(Box::new(ErrorInfo::new(
                "ERROR".to_owned(),
                sqlstate,
                e.to_string(),
            )))
        })?;

        if result.rows.is_empty() {
            let tag = Tag::new(&result.command).with_rows(result.rows_affected as usize);
            return Ok(Response::Execution(tag));
        }

        let field_infos = build_field_infos(&result);
        let schema = Arc::new(field_infos);

        let rows = result.rows;
        let schema_ref = schema.clone();
        let data_row_stream = stream::iter(rows).map(move |row| {
            let mut encoder = DataRowEncoder::new(schema_ref.clone());
            for value in &row.values {
                encode_value(&mut encoder, value)?;
            }
            encoder.finish()
        });

        Ok(Response::Query(QueryResponse::new(schema, data_row_stream)))
    }
}

// ── Handler factory ───────────────────────────────────────────────────────────

pub struct IceDbHandlerFactory {
    pub db_manager: Arc<DatabaseManager>,
    pub authenticator: Arc<Authenticator>,
    pub startup_handler: Arc<IceDbStartupHandler>,
}

impl PgWireHandlerFactory for IceDbHandlerFactory {
    type StartupHandler = IceDbStartupHandler;
    type SimpleQueryHandler = IceDbHandler;
    type ExtendedQueryHandler = IceDbHandler;
    type CopyHandler = NoopCopyHandler;

    fn simple_query_handler(&self) -> Arc<Self::SimpleQueryHandler> {
        Arc::new(IceDbHandler::new(self.db_manager.clone(), self.authenticator.clone()))
    }

    fn extended_query_handler(&self) -> Arc<Self::ExtendedQueryHandler> {
        Arc::new(IceDbHandler::new(self.db_manager.clone(), self.authenticator.clone()))
    }

    fn startup_handler(&self) -> Arc<Self::StartupHandler> {
        self.startup_handler.clone()
    }

    fn copy_handler(&self) -> Arc<Self::CopyHandler> {
        Arc::new(NoopCopyHandler)
    }
}

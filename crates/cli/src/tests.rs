use std::sync::Arc;

use rustyline::completion::Completer;
use rustyline::history::DefaultHistory;
use rustyline::Context;

use crate::completer::SqlCompleter;
use crate::config::Config;
use crate::error::CliError;
use crate::formatter::{format_table, format_value};
use crate::meta::parse_meta_command;
use sql::row::Row;
use sql::value::Value;

// ── Config tests ─────────────────────────────────────────────────────────────

#[test]
fn test_config_defaults() {
    // Use args with only the program name
    let args: Vec<String> = vec!["isql".to_string()];
    // Temporarily remove env vars that might interfere
    let _pg_user = std::env::var("PGUSER").ok();
    let _pg_db = std::env::var("PGDATABASE").ok();
    // We can't easily clear env vars in a portable way in tests, so just test the parser
    let config = Config::from_args(&args);
    assert_eq!(config.data_dir, "./data");
}

#[test]
fn test_config_from_args() {
    let args: Vec<String> = vec![
        "isql".to_string(),
        "--data-dir".to_string(),
        "/tmp/mydb".to_string(),
        "--user".to_string(),
        "alice".to_string(),
        "--dbname".to_string(),
        "mydb".to_string(),
    ];
    let config = Config::from_args(&args);
    assert_eq!(config.data_dir, "/tmp/mydb");
    assert_eq!(config.username, "alice");
    assert_eq!(config.dbname, "mydb");
}

// ── Formatter tests ───────────────────────────────────────────────────────────

#[test]
fn test_format_table_empty() {
    let result = format_table(&[]);
    assert!(result.contains("(0 rows)"));
}

#[test]
fn test_format_table_with_rows() {
    let schema = vec![
        ("id".to_string(), catalog::DataType::Int4),
        ("name".to_string(), catalog::DataType::Text),
    ];
    let row = Row::new(
        vec![Value::Int4(1), Value::Text("Alice".to_string())],
        schema,
    );
    let result = format_table(&[row]);
    assert!(result.contains("id"));
    assert!(result.contains("name"));
    assert!(result.contains("1"));
    assert!(result.contains("Alice"));
    assert!(result.contains("(1 row)"));
}

#[test]
#[allow(clippy::approx_constant)]
fn test_format_value() {
    assert_eq!(format_value(&Value::Null), "NULL");
    assert_eq!(format_value(&Value::Bool(true)), "t");
    assert_eq!(format_value(&Value::Bool(false)), "f");
    assert_eq!(format_value(&Value::Int4(42)), "42");
    assert_eq!(format_value(&Value::Int8(1234567890123)), "1234567890123");
    assert_eq!(format_value(&Value::Float8(3.14)), "3.14");
    assert_eq!(format_value(&Value::Text("hello".to_string())), "hello");
    assert_eq!(format_value(&Value::Bytes(vec![0xde, 0xad])), "\\xdead");
}

// ── Meta-command tests ────────────────────────────────────────────────────────

#[test]
fn test_parse_meta_command() {
    use crate::meta::MetaCommand;

    let check = |s: &str| parse_meta_command(s).unwrap();

    assert!(matches!(check("\\q"), MetaCommand::Quit));
    assert!(matches!(check("\\quit"), MetaCommand::Quit));
    assert!(matches!(check("\\dt"), MetaCommand::ListTables));
    assert!(matches!(check("\\d"), MetaCommand::ListTables));
    assert!(matches!(check("\\du"), MetaCommand::ListRoles));
    assert!(matches!(check("\\l"), MetaCommand::ListDatabases));
    assert!(matches!(check("\\?"), MetaCommand::Help));
    assert!(matches!(check("\\help"), MetaCommand::Help));
    assert!(matches!(check("\\timing"), MetaCommand::Timing));
    assert!(matches!(check("\\x"), MetaCommand::ExpandedOutput));

    // \d tablename
    let cmd = check("\\d mytable");
    assert!(matches!(cmd, MetaCommand::Describe(ref name) if name == "mytable"));
}

#[test]
fn test_unknown_meta_command() {
    let result = parse_meta_command("\\xyz");
    assert!(result.is_err());
    match result {
        Err(CliError::UnknownMetaCommand(s)) => assert!(s.contains("\\xyz")),
        _ => panic!("expected UnknownMetaCommand"),
    }
}

// ── Completer tests ───────────────────────────────────────────────────────────

fn make_history() -> DefaultHistory {
    DefaultHistory::new()
}

#[test]
fn test_sql_completion_keywords() {
    let completer = SqlCompleter {
        table_names: vec![],
    };
    let history = make_history();
    let ctx = Context::new(&history);
    let (_, candidates) = completer.complete("SEL", 3, &ctx).unwrap();
    let displays: Vec<&str> = candidates.iter().map(|p| p.display.as_str()).collect();
    assert!(
        displays.contains(&"SELECT"),
        "expected SELECT in {:?}",
        displays
    );
}

#[test]
fn test_sql_completion_tables() {
    let completer = SqlCompleter {
        table_names: vec!["users".to_string()],
    };
    let history = make_history();
    let ctx = Context::new(&history);
    let (_, candidates) = completer.complete("use", 3, &ctx).unwrap();
    let displays: Vec<&str> = candidates.iter().map(|p| p.display.as_str()).collect();
    assert!(
        displays.contains(&"users"),
        "expected users in {:?}",
        displays
    );
}

// ── Integration test: SQL engine ─────────────────────────────────────────────

#[test]
fn test_cli_execute_sql_via_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_path_buf();

    let wal_writer = Arc::new(wal::WalWriter::open(&data_dir).unwrap());
    let txn_manager = Arc::new(txn::TransactionManager::new(Arc::clone(&wal_writer)));
    let catalog = Arc::new(
        catalog::CatalogManager::open(&data_dir, Arc::clone(&wal_writer), Arc::clone(&txn_manager))
            .unwrap(),
    );
    let engine = Arc::new(sql::QueryEngine::new(
        Arc::clone(&txn_manager),
        Arc::clone(&catalog),
        data_dir.clone(),
    ));

    // CREATE TABLE
    let r = engine.execute("CREATE TABLE t (id INT)");
    assert!(r.is_ok(), "CREATE TABLE failed: {:?}", r);

    // INSERT
    let r = engine.execute("INSERT INTO t VALUES (1)");
    assert!(r.is_ok(), "INSERT failed: {:?}", r);

    // SELECT
    let result = engine.execute("SELECT * FROM t").unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].values[0], Value::Int4(1));
}

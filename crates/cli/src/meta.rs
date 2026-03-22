use crate::error::CliError;
use catalog::manager::CatalogManager;
use std::sync::Arc;

pub enum MetaCommand {
    ListTables,       // \d or \dt
    ListRoles,        // \du
    ListDatabases,    // \l
    Connect(String),  // \c dbname
    Quit,             // \q
    Help,             // \?
    Describe(String), // \d tablename
    Timing,           // \timing (toggle)
    ExpandedOutput,   // \x (toggle)
}

pub fn parse_meta_command(input: &str) -> Result<MetaCommand, CliError> {
    let input = input.trim();
    if input == "\\q" || input == "\\quit" {
        return Ok(MetaCommand::Quit);
    }
    if input == "\\dt" || input == "\\d" {
        return Ok(MetaCommand::ListTables);
    }
    if input == "\\du" {
        return Ok(MetaCommand::ListRoles);
    }
    if input == "\\l" {
        return Ok(MetaCommand::ListDatabases);
    }
    if input == "\\?" || input == "\\help" {
        return Ok(MetaCommand::Help);
    }
    if input == "\\timing" {
        return Ok(MetaCommand::Timing);
    }
    if input == "\\x" {
        return Ok(MetaCommand::ExpandedOutput);
    }
    if let Some(table_name) = input.strip_prefix("\\d ") {
        return Ok(MetaCommand::Describe(table_name.trim().to_string()));
    }
    if let Some(db) = input.strip_prefix("\\c ").or_else(|| input.strip_prefix("\\connect ")) {
        return Ok(MetaCommand::Connect(db.trim().to_string()));
    }
    if input == "\\c" || input == "\\connect" {
        return Ok(MetaCommand::Connect("icedb".to_string()));
    }
    Err(CliError::UnknownMetaCommand(input.to_string()))
}

pub fn execute_meta_command(
    cmd: MetaCommand,
    catalog: &Arc<CatalogManager>,
    _engine: &Arc<sql::QueryEngine>,
    timing: &mut bool,
    expanded: &mut bool,
    data_dir: &std::path::Path,
) -> Result<String, CliError> {
    match cmd {
        MetaCommand::Quit => Ok("\\q".to_string()),  // caller handles quit
        MetaCommand::Help => Ok(HELP_TEXT.to_string()),
        MetaCommand::Timing => {
            *timing = !*timing;
            Ok(format!("Timing is {}.\n", if *timing { "on" } else { "off" }))
        }
        MetaCommand::ExpandedOutput => {
            *expanded = !*expanded;
            Ok(format!("Expanded display is {}.\n", if *expanded { "on" } else { "off" }))
        }
        MetaCommand::ListTables => {
            // Fall back to catalog.list_tables directly since system catalog tables
            // aren't yet exposed as queryable SQL tables
            match catalog.list_tables("public") {
                Ok(tables) => {
                    if tables.is_empty() {
                        Ok("Did not find any relations.\n".to_string())
                    } else {
                        let mut out = String::from(" Schema |  Name  | Type  \n--------+--------+-------\n");
                        for t in &tables {
                            out.push_str(&format!(" public | {:<6} | table\n", t));
                        }
                        Ok(out)
                    }
                }
                Err(e) => Err(CliError::Catalog(e)),
            }
        }
        MetaCommand::ListRoles => {
            Ok("                                   List of roles\n Role name |  Attributes  \n-----------+--------------\n postgres  | Superuser\n".to_string())
        }
        MetaCommand::ListDatabases => {
            let registry = sql::db_manager::DatabaseRegistry::new(data_dir);
            let dbs = registry.list();
            let mut out = String::from("                                  List of databases\n   Name   |  Owner   \n----------+----------\n");
            for db in &dbs {
                out.push_str(&format!(" {:<8} | {}\n", db.name, db.owner));
            }
            Ok(out)
        }
        MetaCommand::Connect(db_name) => {
            // Returned as a special sentinel; the REPL handles engine switching.
            Ok(format!("\\c {}", db_name))
        }
        MetaCommand::Describe(table_name) => {
            // Show column definitions
            match catalog.get_table("public", &table_name) {
                Ok(schema) => {
                    let mut out = format!("Table \"public.{}\"\n", table_name);
                    out.push_str("  Column  |   Type   | Nullable\n");
                    out.push_str("---------+----------+---------\n");
                    for col in &schema.columns {
                        out.push_str(&format!(" {:<10} | {:<8} | {}\n",
                            col.name,
                            col.data_type.type_name(),
                            if col.not_null { "not null" } else { "" }
                        ));
                    }
                    Ok(out)
                }
                Err(_) => Ok(format!("Did not find any relation named \"{}\".\n", table_name)),
            }
        }
    }
}

const HELP_TEXT: &str = r#"General
  \q             quit nkv-psql
  \?             show this help

Informational
  \d [NAME]      describe table, or list all tables
  \dt            list tables
  \du            list roles
  \l             list databases
  \c [DBNAME]    connect to new database

Formatting
  \timing        toggle timing of commands
  \x             toggle expanded output

"#;

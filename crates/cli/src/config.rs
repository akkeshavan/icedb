#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: String,
    pub username: String,
    pub dbname: String,
    pub history_file: Option<String>,
}

impl Config {
    pub fn from_args(args: &[String]) -> Self {
        // Parse: --data-dir, --user / -U, --dbname / -d
        // Fall back to env vars: PGUSER, PGDATABASE
        // Defaults: data_dir="./data", username="icedb", dbname="icedb"
        let data_dir = parse_arg(args, "--data-dir").unwrap_or_else(|| "./data".to_string());
        let username = parse_arg(args, "--user")
            .or_else(|| parse_arg(args, "-U"))
            .or_else(|| std::env::var("PGUSER").ok())
            .unwrap_or_else(|| "icedb".to_string());
        let dbname = parse_arg(args, "--dbname")
            .or_else(|| parse_arg(args, "-d"))
            .or_else(|| std::env::var("PGDATABASE").ok())
            .unwrap_or_else(|| username.clone());
        let history_file = dirs_or_home_history_path();
        Config {
            data_dir,
            username,
            dbname,
            history_file,
        }
    }
}

fn parse_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn dirs_or_home_history_path() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|home| format!("{}/.isql_history", home))
}

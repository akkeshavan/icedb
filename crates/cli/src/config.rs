#[derive(Debug, Clone, PartialEq)]
pub enum SslMode {
    Disable,
    /// Encrypt but do not verify the server certificate.
    Require,
    /// Encrypt and verify the certificate against `sslrootcert`.
    VerifyFull,
}

impl Default for SslMode {
    fn default() -> Self {
        SslMode::Disable
    }
}

impl std::str::FromStr for SslMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "disable" => Ok(SslMode::Disable),
            "allow" | "prefer" | "require" => Ok(SslMode::Require),
            "verify-ca" | "verify-full" => Ok(SslMode::VerifyFull),
            other => Err(format!("unknown sslmode: {}", other)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub dbname: String,
    pub password: Option<String>,
    pub history_file: Option<String>,
    pub sslmode: SslMode,
    pub sslrootcert: Option<String>,
    /// True when `--host` was explicitly supplied on the command line or via
    /// `PGHOST`.  Used by `is_network_mode()` to distinguish an explicit
    /// remote connection from the defaulted localhost.
    pub explicit_host: bool,
}

impl Config {
    pub fn from_args(args: &[String]) -> Self {
        // `--host` / `PGHOST` set the host AND flag network mode.
        // `-h` is reserved for `--help` in main.rs and is NOT treated as host
        // here to avoid the collision.
        let explicit_host_val = parse_arg(args, "--host")
            .or_else(|| std::env::var("PGHOST").ok());
        let explicit_host = explicit_host_val.is_some();
        let host = explicit_host_val.unwrap_or_else(|| "localhost".to_string());

        let port = parse_arg(args, "--port")
            .or_else(|| parse_arg(args, "-p"))
            .or_else(|| std::env::var("PGPORT").ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(5432u16);

        let username = parse_arg(args, "--user")
            .or_else(|| parse_arg(args, "-U"))
            .or_else(|| std::env::var("PGUSER").ok())
            .unwrap_or_else(|| "icedb".to_string());

        let dbname = parse_arg(args, "--dbname")
            .or_else(|| parse_arg(args, "-d"))
            .or_else(|| std::env::var("PGDATABASE").ok())
            .unwrap_or_else(|| username.clone());

        let data_dir = parse_arg(args, "--data-dir").unwrap_or_else(|| "./data".to_string());

        // Password: explicit flag > PGPASSWORD env var > .pgpass file lookup
        let password = parse_arg(args, "--password")
            .or_else(|| parse_arg(args, "-W"))
            .or_else(|| std::env::var("PGPASSWORD").ok())
            .or_else(|| read_pgpass(&host, port, &dbname, &username));

        let sslmode = parse_arg(args, "--sslmode")
            .as_deref()
            .and_then(|s| s.parse::<SslMode>().ok())
            .unwrap_or_default();

        let sslrootcert = parse_arg(args, "--sslrootcert");

        let history_file = dirs_or_home_history_path();

        Config {
            data_dir,
            host,
            port,
            username,
            dbname,
            password,
            history_file,
            sslmode,
            sslrootcert,
            explicit_host,
        }
    }

    /// Returns `true` when the CLI should connect to a running server over TCP
    /// rather than opening the embedded engine from `data_dir`.
    pub fn is_network_mode(&self) -> bool {
        self.explicit_host
    }
}

/// Look up a password in `~/.pgpass` for the given connection parameters.
///
/// File format (one entry per line):
///   hostname:port:database:username:password
///
/// Fields may use `*` as a wildcard.  The first matching line wins.
fn read_pgpass(host: &str, port: u16, dbname: &str, username: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let path = std::path::Path::new(&home).join(".pgpass");
    let contents = std::fs::read_to_string(&path).ok()?;
    let port_str = port.to_string();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = split_pgpass_line(line);
        if fields.len() < 5 {
            continue;
        }
        let (f_host, f_port, f_db, f_user, f_pass) =
            (&fields[0], &fields[1], &fields[2], &fields[3], &fields[4]);
        let matches = |field: &str, value: &str| field == "*" || field == value;
        if matches(f_host, host)
            && matches(f_port, &port_str)
            && matches(f_db, dbname)
            && matches(f_user, username)
        {
            return Some(f_pass.replace("\\:", ":").replace("\\\\", "\\"));
        }
    }
    None
}

/// Split a `.pgpass` line on unescaped `:` characters.
fn split_pgpass_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                current.push('\\');
                current.push(next);
                chars.next();
            }
        } else if ch == ':' {
            fields.push(current.clone());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    fields.push(current);
    fields
}

fn parse_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn dirs_or_home_history_path() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|home| format!("{}/.isql_history", home))
}

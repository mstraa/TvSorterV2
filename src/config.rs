use std::env;
use std::path::PathBuf;

/// Runtime configuration resolved from environment variables.
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// Retained for diagnostics and future use (the DB path is derived from it).
    #[allow(dead_code)]
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub host: String,
    pub port: u16,
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

pub fn load_config() -> AppConfig {
    let data_dir = env::var_os("TVSORTER_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local/share/tvsorter"));
    let database_path = env::var_os("TVSORTER_DATABASE")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("tvsorter.db"));
    let host = env::var("TVSORTER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = env::var("TVSORTER_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8080);
    AppConfig {
        data_dir,
        database_path,
        host,
        port,
    }
}

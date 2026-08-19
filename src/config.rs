use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;

pub const SERVICE_NAME: &str = "WakeBridge";
pub const DISPLAY_NAME: &str = "WakeBridge";
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8787";

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub listen: SocketAddr,
    pub data_dir: PathBuf,
    pub service_mode: bool,
}

impl AppConfig {
    pub fn new(listen: &str, data_dir: Option<PathBuf>, service_mode: bool) -> Result<Self> {
        let listen = listen
            .parse::<SocketAddr>()
            .with_context(|| format!("invalid listen address: {listen}"))?;
        Ok(Self {
            listen,
            data_dir: data_dir.unwrap_or_else(default_data_dir),
            service_mode,
        })
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("wakebridge.db")
    }
}

pub fn default_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\ProgramData\WakeBridge")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("data")
    }
}

pub fn install_dir() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\Program Files\WakeBridge")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("target")
    }
}

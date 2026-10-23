//! Persistent agent configuration (bind address + port).

use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default listen address: loopback only.
pub const DEFAULT_BIND: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
/// Default listen port (compatible with historical AtlasGPU agent collectors).
pub const DEFAULT_PORT: u16 = 12_001;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Interface to bind. Defaults to `127.0.0.1`.
    #[serde(default = "default_bind_string")]
    pub bind: String,
    /// TCP port. Defaults to `12001`.
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_bind_string() -> String {
    DEFAULT_BIND.to_string()
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: default_bind_string(),
            port: default_port(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to resolve config directory")]
    NoProjectDirs,
    #[error("invalid bind address '{0}'")]
    InvalidBind(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse error: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("toml serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

impl Config {
    /// Default XDG / AppData path for the config file.
    pub fn default_path() -> Result<PathBuf, ConfigError> {
        let dirs = ProjectDirs::from("io", "ExoGridTech", "ExoGPUAgent")
            .ok_or(ConfigError::NoProjectDirs)?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Load from `path`, creating a default file if missing.
    pub fn load_or_create(path: &Path) -> Result<Self, ConfigError> {
        if path.exists() {
            let raw = fs::read_to_string(path)?;
            let cfg: Config = toml::from_str(&raw)?;
            return Ok(cfg);
        }

        let cfg = Config::default();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        cfg.save(path)?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        fs::write(path, raw)?;
        Ok(())
    }

    pub fn socket_addr(&self) -> Result<SocketAddr, ConfigError> {
        let ip: IpAddr = self
            .bind
            .parse()
            .map_err(|_| ConfigError::InvalidBind(self.bind.clone()))?;
        Ok(SocketAddr::new(ip, self.port))
    }

    pub fn is_loopback(&self) -> bool {
        self.bind
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("exogpuagent-{name}-{nanos}.toml"))
    }

    #[test]
    fn default_is_localhost_12001() {
        let cfg = Config::default();
        assert_eq!(cfg.bind, "127.0.0.1");
        assert_eq!(cfg.port, 12_001);
        assert!(cfg.is_loopback());
        let addr = cfg.socket_addr().unwrap();
        assert_eq!(addr.to_string(), "127.0.0.1:12001");
    }

    #[test]
    fn load_or_create_writes_defaults() {
        let path = temp_config_path("create");
        let _ = fs::remove_file(&path);

        let cfg = Config::load_or_create(&path).unwrap();
        assert_eq!(cfg, Config::default());
        assert!(path.exists());

        let again = Config::load_or_create(&path).unwrap();
        assert_eq!(again, cfg);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn load_custom_bind_and_port() {
        let path = temp_config_path("custom");
        fs::write(&path, "bind = \"127.0.0.1\"\nport = 12002\n").unwrap();
        let cfg = Config::load_or_create(&path).unwrap();
        assert_eq!(cfg.port, 12_002);
        let _ = fs::remove_file(&path);
    }
}

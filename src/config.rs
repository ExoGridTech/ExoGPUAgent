//! Persistent agent configuration (portable + machine/service paths).

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
/// Default local admin GUI port (Phase 2C-C2).
pub const DEFAULT_GUI_PORT: u16 = 12_010;

/// Access mode. Phase 2B only accepts [`AccessMode::Localhost`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    #[default]
    Localhost,
    /// Reserved for a later phase (rejected by net policy in 2B).
    Tailscale,
    /// Reserved for a later phase (rejected by net policy in 2B).
    Lan,
}

impl AccessMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Localhost => "localhost",
            Self::Tailscale => "tailscale",
            Self::Lan => "lan",
        }
    }
}

/// Local Mini GUI settings (disabled by default; loopback only in 2C-C2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuiConfig {
    /// When false, the admin listener is not started.
    #[serde(default)]
    pub enabled: bool,

    /// Admin bind address (must be loopback).
    #[serde(default = "default_bind_string")]
    pub bind: String,

    /// Admin HTTP port (default 12010).
    #[serde(default = "default_gui_port")]
    pub port: u16,

    /// Dashboard poll interval hint for the SPA (seconds).
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_bind_string(),
            port: default_gui_port(),
            poll_interval_secs: default_poll_interval(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Optional friendly host name (not exposed on `/gpu` yet).
    #[serde(default)]
    pub host_name: String,

    /// Interface to bind. Defaults to `127.0.0.1`.
    #[serde(default = "default_bind_string")]
    pub bind: String,

    /// TCP port. Defaults to `12001`.
    #[serde(default = "default_port")]
    pub port: u16,

    /// Network access mode. Phase 2B/2C: must remain `localhost` for live binds.
    #[serde(default)]
    pub access_mode: AccessMode,

    /// Reserved for future remote auth (never logged). Empty in 2C-C2.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub auth_token: String,

    /// Whether the Windows/Linux service should autostart (installer hint).
    #[serde(default = "default_true")]
    pub autostart: bool,

    /// Local admin Mini GUI (optional). Absent in old configs → defaults.
    #[serde(default)]
    pub gui: GuiConfig,
}

fn default_bind_string() -> String {
    DEFAULT_BIND.to_string()
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_gui_port() -> u16 {
    DEFAULT_GUI_PORT
}

fn default_poll_interval() -> u64 {
    5
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host_name: String::new(),
            bind: default_bind_string(),
            port: default_port(),
            access_mode: AccessMode::Localhost,
            auth_token: String::new(),
            autostart: true,
            gui: GuiConfig::default(),
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
    /// Per-user portable/XDG path (console / non-service use).
    pub fn portable_path() -> Result<PathBuf, ConfigError> {
        let dirs = ProjectDirs::from("io", "ExoGridTech", "ExoGPUAgent")
            .ok_or(ConfigError::NoProjectDirs)?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Machine-wide path used by the Windows service (and future Linux unit).
    pub fn machine_path() -> PathBuf {
        #[cfg(windows)]
        {
            let base = std::env::var_os("PROGRAMDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
            base.join("ExoGridTech")
                .join("ExoGPUAgent")
                .join("config.toml")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("/etc/exogpuagent/config.toml")
        }
    }

    /// Directory containing machine config (ProgramData /etc).
    pub fn machine_dir() -> PathBuf {
        Self::machine_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn machine_logs_dir() -> PathBuf {
        Self::machine_dir().join("logs")
    }

    /// Resolve config path: `--config` / env / machine / portable.
    pub fn resolve_path(
        explicit: Option<PathBuf>,
        prefer_machine: bool,
    ) -> Result<PathBuf, ConfigError> {
        if let Some(path) = explicit {
            return Ok(path);
        }
        if let Some(path) = std::env::var_os("EXOGPUAGENT_CONFIG") {
            return Ok(PathBuf::from(path));
        }
        if prefer_machine {
            return Ok(Self::machine_path());
        }
        Self::portable_path()
    }

    /// Admin commands: machine config if it exists, otherwise portable.
    pub fn resolve_admin_path(
        explicit: Option<PathBuf>,
        force_portable: bool,
    ) -> Result<PathBuf, ConfigError> {
        if let Some(path) = explicit {
            return Ok(path);
        }
        if let Some(path) = std::env::var_os("EXOGPUAGENT_CONFIG") {
            return Ok(PathBuf::from(path));
        }
        if force_portable {
            return Self::portable_path();
        }
        let machine = Self::machine_path();
        if machine.exists() {
            return Ok(machine);
        }
        Self::portable_path()
    }

    /// Load from `path`. If missing, create defaults **without** clobbering an existing file.
    pub fn load_or_create(path: &Path) -> Result<Self, ConfigError> {
        if path.exists() {
            return Self::load(path);
        }

        let cfg = Config::default();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        cfg.save(path)?;
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&raw)?;
        Ok(cfg)
    }

    /// Write config only if the destination does not already exist.
    pub fn save_if_absent(path: &Path) -> Result<Self, ConfigError> {
        if path.exists() {
            return Self::load(path);
        }
        Self::load_or_create(path)
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

    pub fn gui_socket_addr(&self) -> Result<SocketAddr, ConfigError> {
        let ip: IpAddr = self
            .gui
            .bind
            .parse()
            .map_err(|_| ConfigError::InvalidBind(self.gui.bind.clone()))?;
        Ok(SocketAddr::new(ip, self.gui.port))
    }

    pub fn is_loopback(&self) -> bool {
        self.bind
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
    }

    /// Clamp GUI poll interval to a safe range for the SPA.
    pub fn gui_poll_interval_secs(&self) -> u64 {
        self.gui.poll_interval_secs.clamp(2, 60)
    }

    /// Redacted view for logs/doctor (never includes auth_token).
    pub fn safe_summary(&self) -> String {
        format!(
            "host_name={:?} bind={} port={} access_mode={} autostart={} gui.enabled={} gui.port={}",
            self.host_name,
            self.bind,
            self.port,
            self.access_mode.as_str(),
            self.autostart,
            self.gui.enabled,
            self.gui.port
        )
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
        assert_eq!(cfg.access_mode, AccessMode::Localhost);
        assert!(cfg.is_loopback());
        let addr = cfg.socket_addr().unwrap();
        assert_eq!(addr.to_string(), "127.0.0.1:12001");
        assert!(!cfg.gui.enabled);
        assert_eq!(cfg.gui.port, 12_010);
    }

    #[test]
    fn old_config_without_gui_section_loads() {
        let path = temp_config_path("oldgui");
        fs::write(
            &path,
            "bind = \"127.0.0.1\"\nport = 12001\nhost_name = \"legacy\"\n",
        )
        .unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.host_name, "legacy");
        assert!(!cfg.gui.enabled);
        assert_eq!(cfg.gui.port, DEFAULT_GUI_PORT);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn load_or_create_writes_defaults() {
        let path = temp_config_path("create");
        let _ = fs::remove_file(&path);

        let cfg = Config::load_or_create(&path).unwrap();
        assert_eq!(cfg.port, Config::default().port);
        assert!(path.exists());

        let again = Config::load_or_create(&path).unwrap();
        assert_eq!(again.port, cfg.port);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn upgrade_preserves_existing_config() {
        let path = temp_config_path("preserve");
        let _ = fs::remove_file(&path);

        let original = Config {
            host_name: "lab-box".into(),
            port: 12_099,
            ..Config::default()
        };
        original.save(&path).unwrap();

        let loaded = Config::save_if_absent(&path).unwrap();
        assert_eq!(loaded.host_name, "lab-box");
        assert_eq!(loaded.port, 12_099);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn load_custom_bind_and_port() {
        let path = temp_config_path("custom");
        fs::write(
            &path,
            "bind = \"127.0.0.1\"\nport = 12002\nhost_name = \"ck2\"\n",
        )
        .unwrap();
        let cfg = Config::load_or_create(&path).unwrap();
        assert_eq!(cfg.port, 12_002);
        assert_eq!(cfg.host_name, "ck2");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn safe_summary_omits_token() {
        let cfg = Config {
            auth_token: "super-secret".into(),
            ..Config::default()
        };
        let s = cfg.safe_summary();
        assert!(!s.contains("super-secret"));
        assert!(!s.contains("auth_token"));
    }

    #[test]
    fn machine_path_is_absolute() {
        let p = Config::machine_path();
        assert!(p.is_absolute());
        assert!(p.ends_with("config.toml"));
    }
}

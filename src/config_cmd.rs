//! `exogpuagent config` — show / set / path (never prints secrets).

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::{AccessMode, Config, ConfigError};
use crate::net_policy;

#[derive(Debug, Error)]
pub enum ConfigCmdError {
    #[error("{0}")]
    Config(#[from] ConfigError),
    #[error("{0}")]
    Net(#[from] crate::net_policy::NetPolicyError),
    #[error("{0}")]
    Msg(String),
}

pub fn path(
    explicit: Option<PathBuf>,
    prefer_machine: bool,
    force_portable: bool,
) -> Result<PathBuf, ConfigCmdError> {
    if prefer_machine && !force_portable {
        return Ok(Config::machine_path());
    }
    if force_portable {
        return Ok(Config::portable_path()?);
    }
    Ok(Config::resolve_admin_path(explicit, force_portable)?)
}

pub fn show(path: &Path) -> Result<String, ConfigCmdError> {
    let cfg = if path.exists() {
        Config::load(path)?
    } else {
        Config::default()
    };
    // Serialize without auth_token (skip_serializing_if empty already; force clear).
    let safe = Config {
        auth_token: String::new(),
        ..cfg
    };
    Ok(toml::to_string_pretty(&safe).map_err(ConfigError::TomlSer)?)
}

/// Apply `key=value` updates. Returns whether a service restart is recommended.
pub fn set(path: &Path, assignments: &[String]) -> Result<SetResult, ConfigCmdError> {
    if assignments.is_empty() {
        return Err(ConfigCmdError::Msg(
            "at least one key=value assignment is required".into(),
        ));
    }

    let mut cfg = if path.exists() {
        Config::load(path)?
    } else {
        Config::default()
    };

    let mut restart_required = false;

    for raw in assignments {
        let (key, value) = raw
            .split_once('=')
            .ok_or_else(|| ConfigCmdError::Msg(format!("expected key=value, got '{raw}'")))?;
        let key = key.trim();
        let value = value.trim();
        match key {
            "host_name" => {
                cfg.host_name = value.to_string();
            }
            "bind" => {
                cfg.bind = value.to_string();
                restart_required = true;
            }
            "port" => {
                cfg.port = value
                    .parse::<u16>()
                    .map_err(|_| ConfigCmdError::Msg(format!("invalid port '{value}'")))?;
                restart_required = true;
            }
            "access_mode" => {
                cfg.access_mode = parse_access_mode(value)?;
                restart_required = true;
            }
            "autostart" => {
                cfg.autostart = parse_bool(value)?;
            }
            "auth_token" => {
                return Err(ConfigCmdError::Msg(
                    "auth_token cannot be set via CLI in this build (reserved)".into(),
                ));
            }
            "gui.enabled" => {
                cfg.gui.enabled = parse_bool(value)?;
                restart_required = true;
            }
            "gui.bind" => {
                cfg.gui.bind = value.to_string();
                restart_required = true;
            }
            "gui.port" => {
                cfg.gui.port = value
                    .parse::<u16>()
                    .map_err(|_| ConfigCmdError::Msg(format!("invalid gui.port '{value}'")))?;
                restart_required = true;
            }
            "gui.poll_interval_secs" => {
                let n = value.parse::<u64>().map_err(|_| {
                    ConfigCmdError::Msg(format!("invalid gui.poll_interval_secs '{value}'"))
                })?;
                if !(2..=60).contains(&n) {
                    return Err(ConfigCmdError::Msg(
                        "gui.poll_interval_secs must be between 2 and 60".into(),
                    ));
                }
                cfg.gui.poll_interval_secs = n;
            }
            other => {
                return Err(ConfigCmdError::Msg(format!(
                    "unknown config key '{other}' (allowed: host_name, bind, port, access_mode, autostart, gui.enabled, gui.bind, gui.port, gui.poll_interval_secs)"
                )));
            }
        }
    }

    net_policy::validate(&cfg)?;
    cfg.save(path)?;

    Ok(SetResult {
        path: path.to_path_buf(),
        restart_required,
        summary: cfg.safe_summary(),
    })
}

#[derive(Debug)]
pub struct SetResult {
    pub path: PathBuf,
    pub restart_required: bool,
    pub summary: String,
}

fn parse_access_mode(value: &str) -> Result<AccessMode, ConfigCmdError> {
    match value.to_ascii_lowercase().as_str() {
        "localhost" => Ok(AccessMode::Localhost),
        "tailscale" => Ok(AccessMode::Tailscale),
        "lan" => Ok(AccessMode::Lan),
        other => Err(ConfigCmdError::Msg(format!(
            "invalid access_mode '{other}' (expected localhost|tailscale|lan)"
        ))),
    }
}

fn parse_bool(value: &str) -> Result<bool, ConfigCmdError> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(ConfigCmdError::Msg(format!("invalid boolean '{other}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("exogpuagent-cfgcmd-{name}-{nanos}.toml"))
    }

    #[test]
    fn set_port_marks_restart() {
        let path = temp_path("port");
        let _ = std::fs::remove_file(&path);
        let result = set(&path, &["port=12099".into()]).unwrap();
        assert!(result.restart_required);
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.port, 12_099);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn set_rejects_wildcard_bind() {
        let path = temp_path("wild");
        let _ = std::fs::remove_file(&path);
        let err = set(&path, &["bind=0.0.0.0".into()]).unwrap_err();
        assert!(err.to_string().contains("not allowed") || err.to_string().contains("wildcard"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn show_omits_token() {
        let path = temp_path("show");
        let cfg = Config {
            auth_token: "secret".into(),
            host_name: "lab".into(),
            ..Config::default()
        };
        cfg.save(&path).unwrap();
        let shown = show(&path).unwrap();
        assert!(!shown.contains("secret"));
        assert!(shown.contains("lab"));
        let _ = std::fs::remove_file(&path);
    }
}

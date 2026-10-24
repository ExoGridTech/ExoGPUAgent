//! Network bind policy (Phase 2C-C2: localhost telemetry + optional localhost GUI).

use std::net::IpAddr;

use thiserror::Error;

use crate::config::{AccessMode, Config};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NetPolicyError {
    #[error("binding to {0} is not allowed (wildcard / unspecified addresses are rejected)")]
    WildcardBind(String),
    #[error("access_mode '{0}' is not enabled in this build (remote telemetry bind is not available yet)")]
    RemoteModeDisabled(String),
    #[error("bind address '{0}' must be loopback when access_mode is localhost")]
    NonLoopbackLocalhost(String),
    #[error("invalid bind address '{0}'")]
    InvalidBind(String),
    #[error("GUI bind address '{0}' must be loopback")]
    GuiNonLoopback(String),
    #[error("GUI port {0} must differ from telemetry port")]
    GuiPortConflict(u16),
}

/// Validate config against Phase 2C-C2 security rules (fail closed).
pub fn validate(config: &Config) -> Result<(), NetPolicyError> {
    match config.access_mode {
        AccessMode::Localhost => {}
        other => {
            return Err(NetPolicyError::RemoteModeDisabled(
                other.as_str().to_string(),
            ));
        }
    }

    validate_loopback_bind(&config.bind)?;

    if config.gui.enabled {
        validate_gui(config)?;
    }

    Ok(())
}

fn validate_loopback_bind(bind: &str) -> Result<(), NetPolicyError> {
    let ip: IpAddr = bind
        .parse()
        .map_err(|_| NetPolicyError::InvalidBind(bind.to_string()))?;

    if ip.is_unspecified() {
        return Err(NetPolicyError::WildcardBind(bind.to_string()));
    }
    if !ip.is_loopback() {
        return Err(NetPolicyError::NonLoopbackLocalhost(bind.to_string()));
    }
    Ok(())
}

fn validate_gui(config: &Config) -> Result<(), NetPolicyError> {
    let ip: IpAddr = config
        .gui
        .bind
        .parse()
        .map_err(|_| NetPolicyError::InvalidBind(config.gui.bind.clone()))?;
    if ip.is_unspecified() {
        return Err(NetPolicyError::WildcardBind(config.gui.bind.clone()));
    }
    if !ip.is_loopback() {
        return Err(NetPolicyError::GuiNonLoopback(config.gui.bind.clone()));
    }
    if config.gui.port == config.port {
        return Err(NetPolicyError::GuiPortConflict(config.gui.port));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GuiConfig;

    #[test]
    fn localhost_ok() {
        let cfg = Config::default();
        assert!(validate(&cfg).is_ok());
    }

    #[test]
    fn rejects_unspecified() {
        let cfg = Config {
            bind: "0.0.0.0".into(),
            ..Config::default()
        };
        assert!(matches!(
            validate(&cfg),
            Err(NetPolicyError::WildcardBind(_))
        ));
    }

    #[test]
    fn rejects_lan_ip_in_localhost_mode() {
        let cfg = Config {
            bind: "192.168.1.10".into(),
            ..Config::default()
        };
        assert!(matches!(
            validate(&cfg),
            Err(NetPolicyError::NonLoopbackLocalhost(_))
        ));
    }

    #[test]
    fn rejects_remote_modes() {
        let cfg = Config {
            access_mode: AccessMode::Tailscale,
            ..Config::default()
        };
        assert!(matches!(
            validate(&cfg),
            Err(NetPolicyError::RemoteModeDisabled(_))
        ));
    }

    #[test]
    fn gui_enabled_requires_loopback_and_distinct_port() {
        let cfg = Config {
            gui: GuiConfig {
                enabled: true,
                bind: "127.0.0.1".into(),
                port: 12_010,
                poll_interval_secs: 5,
            },
            ..Config::default()
        };
        assert!(validate(&cfg).is_ok());

        let cfg_wild = Config {
            gui: GuiConfig {
                enabled: true,
                bind: "0.0.0.0".into(),
                port: 12_010,
                poll_interval_secs: 5,
            },
            ..Config::default()
        };
        assert!(matches!(
            validate(&cfg_wild),
            Err(NetPolicyError::WildcardBind(_))
        ));

        let cfg_conflict = Config {
            port: 12_010,
            gui: GuiConfig {
                enabled: true,
                bind: "127.0.0.1".into(),
                port: 12_010,
                poll_interval_secs: 5,
            },
            ..Config::default()
        };
        assert!(matches!(
            validate(&cfg_conflict),
            Err(NetPolicyError::GuiPortConflict(_))
        ));
    }
}

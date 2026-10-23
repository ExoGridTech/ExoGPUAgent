//! Network bind policy (Phase 2B: localhost only).

use std::net::IpAddr;

use thiserror::Error;

use crate::config::{AccessMode, Config};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NetPolicyError {
    #[error("binding to {0} is not allowed (wildcard / unspecified addresses are rejected)")]
    WildcardBind(String),
    #[error("access_mode '{0}' is not enabled in this build (Phase 2B is localhost-only)")]
    RemoteModeDisabled(String),
    #[error("bind address '{0}' must be loopback when access_mode is localhost")]
    NonLoopbackLocalhost(String),
    #[error("invalid bind address '{0}'")]
    InvalidBind(String),
}

/// Validate config against Phase 2B security rules (fail closed).
pub fn validate(config: &Config) -> Result<(), NetPolicyError> {
    match config.access_mode {
        AccessMode::Localhost => {}
        other => {
            return Err(NetPolicyError::RemoteModeDisabled(
                other.as_str().to_string(),
            ));
        }
    }

    let ip: IpAddr = config
        .bind
        .parse()
        .map_err(|_| NetPolicyError::InvalidBind(config.bind.clone()))?;

    if ip.is_unspecified() {
        return Err(NetPolicyError::WildcardBind(config.bind.clone()));
    }
    if !ip.is_loopback() {
        return Err(NetPolicyError::NonLoopbackLocalhost(config.bind.clone()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

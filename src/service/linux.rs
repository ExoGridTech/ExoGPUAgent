//! systemd helpers for Linux (Phase 2C-B). Unit registration is package-first;
//! these CLIs support portable installs and day-2 operations.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;

pub const SERVICE_NAME: &str = "exogpuagent";
pub const UNIT_PATH: &str = "/etc/systemd/system/exogpuagent.service";
pub const PACKAGE_UNIT_PATH: &str = "/lib/systemd/system/exogpuagent.service";

/// Unit file content (must stay aligned with `packaging/linux/exogpuagent.service`).
pub const UNIT_FILE: &str = include_str!("../../packaging/linux/exogpuagent.service");

/// Default machine config shipped with the package / portable tarball.
pub const DEFAULT_CONFIG: &str = include_str!("../../packaging/linux/config.toml.default");

pub fn maybe_dispatch_as_service() -> bool {
    false
}

pub fn install(_exe: &Path) -> Result<(), String> {
    require_root()?;

    ensure_system_user()?;
    ensure_dirs_and_config()?;

    if Path::new(PACKAGE_UNIT_PATH).exists() {
        // DEB already owns the unit under /lib/systemd/system.
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", SERVICE_NAME])?;
        return Ok(());
    }

    fs::write(UNIT_PATH, UNIT_FILE).map_err(|e| format!("write {UNIT_PATH}: {e}"))?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", SERVICE_NAME])?;
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    require_root()?;
    let _ = stop();
    let _ = systemctl(&["disable", SERVICE_NAME]);

    // Only remove the unit we may have written for portable installs.
    // Never delete the DEB-managed unit under /lib/systemd/system.
    if Path::new(UNIT_PATH).exists() && !Path::new(PACKAGE_UNIT_PATH).exists() {
        fs::remove_file(UNIT_PATH).map_err(|e| format!("remove {UNIT_PATH}: {e}"))?;
        let _ = systemctl(&["daemon-reload"]);
    } else if Path::new(PACKAGE_UNIT_PATH).exists() {
        return Err(
            "unit is managed by the Debian package; use apt remove/purge exogpuagent".into(),
        );
    }
    Ok(())
}

pub fn start() -> Result<(), String> {
    systemctl(&["start", SERVICE_NAME])
}

pub fn stop() -> Result<(), String> {
    systemctl(&["stop", SERVICE_NAME])
}

pub fn restart() -> Result<(), String> {
    systemctl(&["restart", SERVICE_NAME])
}

pub fn query_state() -> String {
    match Command::new("systemctl")
        .args(["is-active", SERVICE_NAME])
        .output()
    {
        Ok(out) => {
            let active = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let enabled = Command::new("systemctl")
                .args(["is-enabled", SERVICE_NAME])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|| "unknown".into());
            if active.is_empty() {
                format!("unknown (enabled={enabled})")
            } else {
                format!("{active} (enabled={enabled})")
            }
        }
        Err(e) => format!("systemctl unavailable ({e})"),
    }
}

fn require_root() -> Result<(), String> {
    match Command::new("id").args(["-u"]).output() {
        Ok(out) if String::from_utf8_lossy(&out.stdout).trim() == "0" => Ok(()),
        Ok(_) => Err("root privileges required for this service operation".into()),
        Err(e) => Err(format!("failed to check uid via id -u: {e}")),
    }
}

fn ensure_system_user() -> Result<(), String> {
    if user_exists("exogpuagent") {
        return Ok(());
    }
    let home = PathBuf::from("/var/lib/exogpuagent");
    let _ = fs::create_dir_all(&home);
    let status = Command::new("useradd")
        .args([
            "--system",
            "--home-dir",
            home.to_str().unwrap_or("/var/lib/exogpuagent"),
            "--shell",
            "/usr/sbin/nologin",
            "--user-group",
            "exogpuagent",
        ])
        .status()
        .map_err(|e| format!("useradd: {e}"))?;
    if status.success() {
        return Ok(());
    }
    Err("failed to create system user exogpuagent (is useradd available?)".into())
}

fn user_exists(name: &str) -> bool {
    Command::new("id")
        .arg(name)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn ensure_dirs_and_config() -> Result<(), String> {
    let etc = PathBuf::from("/etc/exogpuagent");
    let var = PathBuf::from("/var/lib/exogpuagent");
    fs::create_dir_all(&etc).map_err(|e| format!("mkdir {etc:?}: {e}"))?;
    fs::create_dir_all(&var).map_err(|e| format!("mkdir {var:?}: {e}"))?;

    let cfg = Config::machine_path();
    if !cfg.exists() {
        fs::write(&cfg, DEFAULT_CONFIG).map_err(|e| format!("write {}: {e}", cfg.display()))?;
    }

    // Restrict config directory (best-effort; ignore failures on exotic FS).
    let _ = Command::new("chown")
        .args([
            "root:exogpuagent",
            etc.to_str().unwrap_or("/etc/exogpuagent"),
        ])
        .status();
    let _ = Command::new("chmod")
        .args(["0750", etc.to_str().unwrap_or("/etc/exogpuagent")])
        .status();
    if cfg.exists() {
        let _ = Command::new("chown")
            .args(["root:exogpuagent", cfg.to_str().unwrap_or("")])
            .status();
        let _ = Command::new("chmod")
            .args(["0640", cfg.to_str().unwrap_or("")])
            .status();
    }
    let _ = Command::new("chown")
        .args([
            "exogpuagent:exogpuagent",
            var.to_str().unwrap_or("/var/lib/exogpuagent"),
        ])
        .status();

    Ok(())
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .map_err(|e| format!("systemctl {}: {e}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit {}", output.status)
    };
    Err(format!("systemctl {}: {detail}", args.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_file_mentions_machine_flag() {
        assert!(UNIT_FILE.contains("exogpuagent run --machine"));
        assert!(UNIT_FILE.contains("User=exogpuagent"));
        assert!(UNIT_FILE.contains("StandardOutput=journal"));
    }

    #[test]
    fn default_config_is_localhost() {
        assert!(DEFAULT_CONFIG.contains("127.0.0.1"));
        assert!(DEFAULT_CONFIG.contains("12001"));
        assert!(DEFAULT_CONFIG.contains("localhost"));
    }
}

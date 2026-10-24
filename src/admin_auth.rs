//! Administrative credential storage (Argon2id). Never log passwords or hashes in full.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Config;

const AUTH_FILE_NAME: &str = "admin_auth.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminAuthFile {
    /// PHC string (Argon2id). Never log this value.
    pub password_hash: String,
    /// Schema version for future migrations.
    #[serde(default = "default_version")]
    pub version: u32,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Error)]
pub enum AdminAuthError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("toml serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("password hashing failed")]
    Hash,
    #[error("admin credentials are not bootstrapped; run: exogpuagent gui setup")]
    NotBootstrapped,
    #[error("admin credentials already exist (refusing to overwrite)")]
    AlreadyExists,
    #[error("password does not meet policy (min {0} characters)")]
    WeakPassword(usize),
}

const MIN_PASSWORD_LEN: usize = 12;

impl AdminAuthFile {
    pub fn path_beside_config(config_path: &Path) -> PathBuf {
        config_path
            .parent()
            .map(|p| p.join(AUTH_FILE_NAME))
            .unwrap_or_else(|| PathBuf::from(AUTH_FILE_NAME))
    }

    pub fn machine_path() -> PathBuf {
        Config::machine_dir().join(AUTH_FILE_NAME)
    }

    pub fn portable_path() -> Result<PathBuf, AdminAuthError> {
        let cfg = Config::portable_path().map_err(|_| {
            AdminAuthError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no project dirs",
            ))
        })?;
        Ok(Self::path_beside_config(&cfg))
    }

    pub fn load(path: &Path) -> Result<Self, AdminAuthError> {
        if !path.exists() {
            return Err(AdminAuthError::NotBootstrapped);
        }
        let raw = fs::read_to_string(path)?;
        Ok(toml::from_str(&raw)?)
    }

    pub fn exists(path: &Path) -> bool {
        path.exists()
    }

    /// Create credentials once. Refuses overwrite.
    pub fn bootstrap(path: &Path, password: &str) -> Result<(), AdminAuthError> {
        validate_password_policy(password)?;
        if path.exists() {
            return Err(AdminAuthError::AlreadyExists);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let hash = hash_password(password)?;
        let file = AdminAuthFile {
            password_hash: hash,
            version: 1,
        };
        let raw = toml::to_string_pretty(&file)?;
        write_private_file(path, raw.as_bytes())?;
        Ok(())
    }

    pub fn verify(&self, password: &str) -> bool {
        verify_password(password, &self.password_hash)
    }
}

pub fn validate_password_policy(password: &str) -> Result<(), AdminAuthError> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AdminAuthError::WeakPassword(MIN_PASSWORD_LEN));
    }
    Ok(())
}

pub fn hash_password(password: &str) -> Result<String, AdminAuthError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| AdminAuthError::Hash)
}

pub fn verify_password(password: &str, password_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(password_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), AdminAuthError> {
    let mut f = fs::File::create(path)?;
    f.write_all(bytes)?;
    f.flush()?;
    harden_auth_file_permissions(path);
    Ok(())
}

/// Restrict auth file permissions so the service account can read it.
///
/// - Linux machine path (`/etc/exogpuagent/`): `0640` root:exogpuagent (systemd user).
/// - Otherwise: `0600` for the creating user.
#[cfg(unix)]
fn harden_auth_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let machine = path.starts_with("/etc/exogpuagent");
    let mode = if machine { 0o640 } else { 0o600 };
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    if machine {
        // Best-effort; ignore if groupadd/user not present yet (postinst order).
        let _ = std::process::Command::new("chown")
            .args(["root:exogpuagent", path.to_str().unwrap_or("")])
            .status();
    }
}

#[cfg(not(unix))]
fn harden_auth_file_permissions(_path: &Path) {
    // Windows: inherit ProgramData ACLs (LocalService Modify granted by Inno).
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_auth() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ega-auth-{n}.toml"))
    }

    #[test]
    fn bootstrap_and_verify() {
        let path = temp_auth();
        let _ = fs::remove_file(&path);
        AdminAuthFile::bootstrap(&path, "correct-horse-battery").unwrap();
        let auth = AdminAuthFile::load(&path).unwrap();
        assert!(auth.verify("correct-horse-battery"));
        assert!(!auth.verify("wrong-password!!"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn refuse_overwrite() {
        let path = temp_auth();
        let _ = fs::remove_file(&path);
        AdminAuthFile::bootstrap(&path, "correct-horse-battery").unwrap();
        let err = AdminAuthFile::bootstrap(&path, "another-long-password").unwrap_err();
        assert!(matches!(err, AdminAuthError::AlreadyExists));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn weak_password_rejected() {
        let err = validate_password_policy("short").unwrap_err();
        assert!(matches!(err, AdminAuthError::WeakPassword(_)));
    }

    #[test]
    fn path_beside_config_uses_parent() {
        let p = PathBuf::from("/etc/exogpuagent/config.toml");
        assert_eq!(
            AdminAuthFile::path_beside_config(&p),
            PathBuf::from("/etc/exogpuagent/admin_auth.toml")
        );
        assert_eq!(
            AdminAuthFile::machine_path(),
            Config::machine_dir().join("admin_auth.toml")
        );
    }
}

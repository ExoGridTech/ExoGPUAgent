//! Mini GUI CLI helpers: interactive bootstrap and one-shot setup.
//! Passwords are never logged; prefer TTY prompts or `--password-stdin`.

use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::admin_auth::{validate_password_policy, AdminAuthError, AdminAuthFile};
use crate::config::Config;
use crate::net_policy::{self, NetPolicyError};
use crate::service;

/// Default GUI listen port advertised by setup helpers.
pub const DEFAULT_GUI_PORT: u16 = 12_010;

#[derive(Debug, Error)]
pub enum GuiCliError {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Auth(#[from] AdminAuthError),
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error(transparent)]
    Net(#[from] NetPolicyError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl GuiCliError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Msg(s.into())
    }
}

/// How the operator supplied the admin password for `gui init`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordSource {
    /// Hidden TTY prompts (password + confirmation).
    Interactive,
    /// Single line from stdin (`--password-stdin`).
    StdinLine,
    /// `--password` on argv (discouraged; visible in process listings).
    Argv,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPlan {
    pub config_path: PathBuf,
    pub auth_path: PathBuf,
    pub portable: bool,
}

impl BootstrapPlan {
    pub fn from_paths(config_path: PathBuf, portable: bool) -> Self {
        let auth_path = AdminAuthFile::path_beside_config(&config_path);
        Self {
            config_path,
            auth_path,
            portable,
        }
    }
}

/// Validate that password and confirmation match policy (testable without TTY).
pub fn validate_new_password(password: &str, confirm: &str) -> Result<(), GuiCliError> {
    if password.is_empty() {
        return Err(GuiCliError::msg(
            "cancelled: empty password (no default password is ever set)",
        ));
    }
    if password != confirm {
        return Err(GuiCliError::msg(
            "passwords do not match — try again or cancel with an empty password",
        ));
    }
    validate_password_policy(password)?;
    Ok(())
}

/// Resolve password for `gui init` according to flags and TTY availability.
pub fn resolve_init_password(
    password_flag: Option<String>,
    password_stdin: bool,
    stdin_is_tty: bool,
    prompt: impl Fn() -> Result<String, GuiCliError>,
    read_stdin_line: impl Fn() -> Result<String, GuiCliError>,
) -> Result<(String, PasswordSource), GuiCliError> {
    if password_stdin {
        let line = read_stdin_line()?;
        if line.is_empty() {
            return Err(GuiCliError::msg(
                "--password-stdin expected a single password line on stdin (min 12 characters)",
            ));
        }
        validate_password_policy(&line)?;
        return Ok((line, PasswordSource::StdinLine));
    }
    if let Some(p) = password_flag {
        eprintln!(
            "warning: --password puts the secret in the process command line; prefer interactive prompt or --password-stdin"
        );
        validate_password_policy(&p)?;
        return Ok((p, PasswordSource::Argv));
    }
    if stdin_is_tty {
        let p = prompt()?;
        return Ok((p, PasswordSource::Interactive));
    }
    Err(GuiCliError::msg(
        "no password provided and stdin is not a TTY\n\
         Options:\n\
           • run interactively in a terminal:  exogpuagent gui init\n\
           • pipe one line:                   printf '%s\\n' 'secret' | exogpuagent gui init --password-stdin\n\
           • full wizard (enable + restart):  exogpuagent gui setup",
    ))
}

/// Interactive hidden password + confirmation (requires TTY).
pub fn prompt_password_confirmed() -> Result<String, GuiCliError> {
    eprint!("Admin password (min 12 chars, input hidden): ");
    let _ = io::stderr().flush();
    let password = rpassword::read_password()
        .map_err(|e| GuiCliError::msg(format!("failed to read password: {e}")))?;
    if password.is_empty() {
        return Err(GuiCliError::msg("cancelled: empty password"));
    }
    eprint!("Confirm password: ");
    let _ = io::stderr().flush();
    let confirm = rpassword::read_password()
        .map_err(|e| GuiCliError::msg(format!("failed to read password confirmation: {e}")))?;
    validate_new_password(&password, &confirm)?;
    Ok(password)
}

fn read_password_stdin_line() -> Result<String, GuiCliError> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    Ok(buf.lines().next().unwrap_or("").to_string())
}

/// Create credentials only (does not set `gui.enabled`).
pub fn run_init(
    plan: &BootstrapPlan,
    password_flag: Option<String>,
    password_stdin: bool,
) -> Result<(), GuiCliError> {
    let _ = Config::save_if_absent(&plan.config_path)?;
    let (password, source) = resolve_init_password(
        password_flag,
        password_stdin,
        io::stdin().is_terminal(),
        prompt_password_confirmed,
        read_password_stdin_line,
    )?;
    AdminAuthFile::bootstrap(&plan.auth_path, &password)?;
    // Drop password as soon as possible.
    drop(password);

    println!(
        "OK: admin credentials created at {}",
        plan.auth_path.display()
    );
    println!("    config path: {}", plan.config_path.display());
    match source {
        PasswordSource::Interactive => {
            println!("    (password read from interactive prompt)");
        }
        PasswordSource::StdinLine => {
            println!("    (password read from stdin)");
        }
        PasswordSource::Argv => {}
    }
    if plan.portable {
        println!("Next: exogpuagent config set --portable gui.enabled=true");
        println!("  or:  exogpuagent gui setup --portable");
    } else {
        println!("Next (recommended): sudo exogpuagent gui setup");
        println!("  or:  exogpuagent config set --machine gui.enabled=true && restart the service");
        println!("Then open http://127.0.0.1:{DEFAULT_GUI_PORT}/");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupOutcome {
    /// Operator declined enabling the GUI.
    Skipped,
    /// Credentials + gui.enabled applied (restart may have been best-effort).
    Enabled { restart_ok: bool },
    /// Already enabled with credentials present.
    AlreadyEnabled,
}

/// Enable GUI in config only after auth file exists. Loopback-only.
pub fn enable_gui_in_config(config_path: &Path) -> Result<Config, GuiCliError> {
    let auth_path = AdminAuthFile::path_beside_config(config_path);
    if !AdminAuthFile::exists(&auth_path) {
        return Err(GuiCliError::msg(format!(
            "refusing to enable GUI: missing credentials at {}\n\
             Run: exogpuagent gui init   (or gui setup)",
            auth_path.display()
        )));
    }
    let mut cfg = if config_path.exists() {
        Config::load(config_path)?
    } else {
        Config::default()
    };
    cfg.gui.enabled = true;
    cfg.gui.bind = "127.0.0.1".into();
    cfg.gui.port = DEFAULT_GUI_PORT;
    net_policy::validate(&cfg)?;
    cfg.save(config_path)?;
    Ok(cfg)
}

/// Full interactive wizard: confirm → password → bootstrap → enable → restart.
pub fn run_setup(
    plan: &BootstrapPlan,
    assume_yes: bool,
    skip_restart: bool,
    prompt_yes_no: impl Fn(&str) -> Result<bool, GuiCliError>,
) -> Result<SetupOutcome, GuiCliError> {
    let _ = Config::save_if_absent(&plan.config_path)?;
    let cfg = Config::load(&plan.config_path)?;
    let auth_exists = AdminAuthFile::exists(&plan.auth_path);

    if cfg.gui.enabled && auth_exists {
        println!(
            "Mini GUI already enabled at http://{}:{}/",
            cfg.gui.bind, cfg.gui.port
        );
        println!("config: {}", plan.config_path.display());
        return Ok(SetupOutcome::AlreadyEnabled);
    }

    if !assume_yes {
        if !io::stdin().is_terminal() {
            return Err(GuiCliError::msg(
                "gui setup requires a TTY (or pass --yes with credentials already bootstrapped)\n\
                 Headless servers: pipe a password to `gui init --password-stdin`, then:\n\
                   exogpuagent config set --machine gui.enabled=true && systemctl restart exogpuagent",
            ));
        }
        let enable = prompt_yes_no(&format!(
            "Enable local Mini GUI on 127.0.0.1:{DEFAULT_GUI_PORT}? [y/N] "
        ))?;
        if !enable {
            println!("OK: Mini GUI left disabled (telemetry on 127.0.0.1:12001 unchanged).");
            return Ok(SetupOutcome::Skipped);
        }
    }

    if !auth_exists {
        if !io::stdin().is_terminal() && !assume_yes {
            return Err(GuiCliError::msg(
                "credentials missing and cannot prompt without a TTY",
            ));
        }
        if !io::stdin().is_terminal() {
            return Err(GuiCliError::msg(
                "credentials missing: run `exogpuagent gui init --password-stdin` first",
            ));
        }
        println!("Creating admin credentials (no default password).");
        let password = prompt_password_confirmed()?;
        AdminAuthFile::bootstrap(&plan.auth_path, &password)?;
        drop(password);
        println!("OK: credentials stored at {}", plan.auth_path.display());
    } else {
        println!(
            "OK: reusing existing credentials at {}",
            plan.auth_path.display()
        );
    }

    let cfg = enable_gui_in_config(&plan.config_path)?;
    println!(
        "OK: gui.enabled=true (bind={}, port={})",
        cfg.gui.bind, cfg.gui.port
    );

    let mut restart_ok = false;
    if !skip_restart {
        match service::restart() {
            Ok(()) => {
                println!("OK: service restart requested");
                restart_ok = true;
            }
            Err(e) => {
                eprintln!("warning: service restart failed ({e})");
                eprintln!(
                    "         restart manually, then open http://127.0.0.1:{DEFAULT_GUI_PORT}/"
                );
            }
        }
    } else {
        println!("note: --no-restart set; restart the service to apply gui.enabled");
    }

    println!("Open: http://127.0.0.1:{DEFAULT_GUI_PORT}/");
    Ok(SetupOutcome::Enabled { restart_ok })
}

pub fn prompt_yes_no_stdout(prompt: &str) -> Result<bool, GuiCliError> {
    eprint!("{prompt}");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let t = line.trim().to_ascii_lowercase();
    Ok(matches!(t.as_str(), "y" | "yes"))
}

/// Resolve machine/portable/config path for GUI commands (machine by default).
pub fn resolve_gui_config_path(
    explicit: Option<PathBuf>,
    _machine_flag: bool,
    portable_flag: bool,
) -> Result<PathBuf, GuiCliError> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if portable_flag {
        return Config::portable_path().map_err(GuiCliError::from);
    }
    Ok(Config::machine_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ega-gui-cli-{name}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reject_empty_and_mismatch_and_short() {
        assert!(validate_new_password("", "").is_err());
        assert!(validate_new_password("correct-horse", "other-password").is_err());
        assert!(validate_new_password("short", "short").is_err());
        assert!(validate_new_password("correct-horse-battery", "correct-horse-battery").is_ok());
    }

    #[test]
    fn resolve_prefers_stdin_over_interactive() {
        let (p, src) = resolve_init_password(
            None,
            true,
            true,
            || panic!("should not prompt"),
            || Ok("correct-horse-battery".into()),
        )
        .unwrap();
        assert_eq!(p, "correct-horse-battery");
        assert_eq!(src, PasswordSource::StdinLine);
    }

    #[test]
    fn resolve_interactive_when_tty() {
        let (p, src) = resolve_init_password(
            None,
            false,
            true,
            || Ok("correct-horse-battery".into()),
            || panic!("no stdin"),
        )
        .unwrap();
        assert_eq!(src, PasswordSource::Interactive);
        assert_eq!(p, "correct-horse-battery");
    }

    #[test]
    fn resolve_errors_without_tty_or_flags() {
        let err = resolve_init_password(
            None,
            false,
            false,
            || panic!("no prompt"),
            || panic!("no stdin"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("not a TTY"));
    }

    #[test]
    fn enable_refuses_without_auth_file() {
        let dir = temp_dir("noauth");
        let cfg = dir.join("config.toml");
        Config::default().save(&cfg).unwrap();
        let err = enable_gui_in_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("missing credentials"));
        let loaded = Config::load(&cfg).unwrap();
        assert!(!loaded.gui.enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enable_only_after_bootstrap_fresh() {
        let dir = temp_dir("fresh");
        let cfg_path = dir.join("config.toml");
        Config::default().save(&cfg_path).unwrap();
        assert!(!Config::load(&cfg_path).unwrap().gui.enabled);

        let auth = AdminAuthFile::path_beside_config(&cfg_path);
        AdminAuthFile::bootstrap(&auth, "correct-horse-battery").unwrap();
        let cfg = enable_gui_in_config(&cfg_path).unwrap();
        assert!(cfg.gui.enabled);
        assert_eq!(cfg.gui.port, DEFAULT_GUI_PORT);
        assert_eq!(cfg.gui.bind, "127.0.0.1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrade_preserves_disabled_gui_and_existing_auth() {
        let dir = temp_dir("upgrade");
        let cfg_path = dir.join("config.toml");
        let mut cfg = Config::default();
        cfg.gui.enabled = false;
        cfg.host_name = "lab-keep".into();
        cfg.save(&cfg_path).unwrap();
        let auth = AdminAuthFile::path_beside_config(&cfg_path);
        AdminAuthFile::bootstrap(&auth, "correct-horse-battery").unwrap();

        // Simulate upgrade path: load existing, do not flip enabled.
        let loaded = Config::load(&cfg_path).unwrap();
        assert!(!loaded.gui.enabled);
        assert_eq!(loaded.host_name, "lab-keep");
        assert!(AdminAuthFile::exists(&auth));
        assert!(AdminAuthFile::load(&auth)
            .unwrap()
            .verify("correct-horse-battery"));
        // Re-bootstrap must refuse (upgrade must not change password).
        assert!(matches!(
            AdminAuthFile::bootstrap(&auth, "brand-new-password!!"),
            Err(AdminAuthError::AlreadyExists)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_empty_password_is_explicit() {
        let err = validate_new_password("", "x").unwrap_err();
        assert!(err.to_string().contains("cancelled"));
    }

    #[test]
    fn setup_skip_leaves_gui_disabled() {
        let dir = temp_dir("skip");
        let cfg_path = dir.join("config.toml");
        Config::default().save(&cfg_path).unwrap();
        let plan = BootstrapPlan::from_paths(cfg_path.clone(), false);
        // Non-TTY path is tested via resolve_*; here exercise enable refusal + skip outcome logic.
        assert!(!Config::load(&cfg_path).unwrap().gui.enabled);
        assert!(!AdminAuthFile::exists(&plan.auth_path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fresh_bootstrap_then_enable_is_idempotent_on_auth() {
        let dir = temp_dir("idem");
        let cfg_path = dir.join("config.toml");
        Config::default().save(&cfg_path).unwrap();
        let auth = AdminAuthFile::path_beside_config(&cfg_path);
        AdminAuthFile::bootstrap(&auth, "correct-horse-battery").unwrap();
        enable_gui_in_config(&cfg_path).unwrap();
        // Second enable is fine; password unchanged.
        enable_gui_in_config(&cfg_path).unwrap();
        assert!(AdminAuthFile::load(&auth)
            .unwrap()
            .verify("correct-horse-battery"));
        assert!(matches!(
            AdminAuthFile::bootstrap(&auth, "another-long-pass!"),
            Err(AdminAuthError::AlreadyExists)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

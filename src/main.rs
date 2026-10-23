//! ExoGPUAgent — local-first NVIDIA GPU telemetry agent (Phase 2C Linux + 2B Windows).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use exogpuagent::config::Config;
use exogpuagent::config_cmd;
use exogpuagent::doctor;
use exogpuagent::server;
use exogpuagent::service;
use exogpuagent::status;

#[derive(Debug, Parser)]
#[command(
    name = "exogpuagent",
    about = "Lightweight local NVIDIA GPU telemetry agent",
    version,
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run the HTTP agent in the foreground (portable / console).
    Run {
        /// Path to config.toml (default: user portable path).
        #[arg(long)]
        config: Option<PathBuf>,

        /// Use machine-wide config (ProgramData /etc — same as the service).
        #[arg(long)]
        machine: bool,
    },

    /// Show service + local /health status.
    Status {
        #[arg(long)]
        config: Option<PathBuf>,
        /// Force per-user portable config instead of machine path.
        #[arg(long)]
        portable: bool,
    },

    /// Diagnose NVML, config, bind policy, and local HTTP (does not stop the service).
    Doctor {
        #[arg(long)]
        config: Option<PathBuf>,
        /// Force per-user portable config instead of machine path.
        #[arg(long)]
        portable: bool,
    },

    /// Print version information.
    Version,

    /// Show or modify configuration (never prints secrets).
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// OS service management (Windows SCM / Linux systemd; elevation often required).
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    /// Print the resolved config path.
    Path {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        machine: bool,
        #[arg(long)]
        portable: bool,
    },
    /// Print config.toml contents (auth_token omitted).
    Show {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        machine: bool,
        #[arg(long)]
        portable: bool,
    },
    /// Set one or more key=value fields and save.
    Set {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        machine: bool,
        #[arg(long)]
        portable: bool,
        /// Assignments such as port=12001 host_name=lab
        #[arg(required = true)]
        assignments: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ServiceAction {
    /// Register / enable the OS service (Windows SCM or Linux systemd unit).
    Install {
        /// Path to the exogpuagent binary (default: this executable). Windows only uses this.
        #[arg(long)]
        exe: Option<PathBuf>,
    },
    /// Stop and remove / disable the OS service (DEB-managed units must use apt).
    Uninstall,
    Start,
    Stop,
    Restart,
}

fn main() -> ExitCode {
    // When launched by Windows SCM, dispatch into the service entrypoint.
    if service::maybe_dispatch_as_service() {
        return ExitCode::SUCCESS;
    }

    match run_cli() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("exogpuagent: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_cli() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Commands::Run {
        config: None,
        machine: false,
    }) {
        Commands::Run { config, machine } => {
            let path = Config::resolve_path(config, machine)?;
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(server::run_foreground(&path))?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Status { config, portable } => {
            let path = Config::resolve_admin_path(config, portable)?;
            let report = status::run_status(&path);
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(ExitCode::SUCCESS)
        }
        Commands::Doctor { config, portable } => {
            let path = Config::resolve_admin_path(config, portable)?;
            let report = doctor::run_doctor(&path);
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.ok {
                return Ok(ExitCode::from(1));
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Version => {
            println!(
                "exogpuagent {} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS
            );
            Ok(ExitCode::SUCCESS)
        }
        Commands::Config { action } => match action {
            ConfigAction::Path {
                config,
                machine,
                portable,
            } => {
                let path = config_cmd::path(config, machine, portable)?;
                println!("{}", path.display());
                Ok(ExitCode::SUCCESS)
            }
            ConfigAction::Show {
                config,
                machine,
                portable,
            } => {
                let path = resolve_config_write_path(config, machine, portable)?;
                print!("{}", config_cmd::show(&path)?);
                Ok(ExitCode::SUCCESS)
            }
            ConfigAction::Set {
                config,
                machine,
                portable,
                assignments,
            } => {
                let path = resolve_config_write_path(config, machine, portable)?;
                let result = config_cmd::set(&path, &assignments)?;
                println!("wrote {}", result.path.display());
                println!("summary: {}", result.summary);
                if result.restart_required {
                    println!("note: service restart required for bind/port/access_mode changes");
                    println!("  Windows: exogpuagent service restart");
                    println!("  Linux:   sudo systemctl restart exogpuagent");
                }
                Ok(ExitCode::SUCCESS)
            }
        },
        Commands::Service { action } => match action {
            ServiceAction::Install { exe } => {
                let exe = match exe {
                    Some(p) => p,
                    None => std::env::current_exe()?,
                };
                service::install(&exe)?;
                println!(
                    "service {} installed/enabled (auto-start when supported)",
                    service::SERVICE_NAME
                );
                Ok(ExitCode::SUCCESS)
            }
            ServiceAction::Uninstall => {
                service::uninstall()?;
                println!("service {} uninstalled/disabled", service::SERVICE_NAME);
                Ok(ExitCode::SUCCESS)
            }
            ServiceAction::Start => {
                service::start()?;
                println!("service start requested");
                Ok(ExitCode::SUCCESS)
            }
            ServiceAction::Stop => {
                service::stop()?;
                println!("service stop requested");
                Ok(ExitCode::SUCCESS)
            }
            ServiceAction::Restart => {
                service::restart()?;
                println!("service restart requested");
                Ok(ExitCode::SUCCESS)
            }
        },
    }
}

fn resolve_config_write_path(
    explicit: Option<PathBuf>,
    prefer_machine: bool,
    force_portable: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if prefer_machine {
        return Ok(Config::machine_path());
    }
    if force_portable {
        return Ok(Config::portable_path()?);
    }
    Ok(Config::resolve_admin_path(None, false)?)
}

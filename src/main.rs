//! ExoGPUAgent — local-first NVIDIA GPU telemetry agent (Phase 2B Windows).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use exogpuagent::config::Config;
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

        /// Use machine-wide ProgramData config (same as the Windows service).
        #[arg(long)]
        machine: bool,
    },

    /// Show service + local /health status.
    Status {
        #[arg(long)]
        config: Option<PathBuf>,
        /// Force per-user portable config instead of ProgramData.
        #[arg(long)]
        portable: bool,
    },

    /// Diagnose NVML, config, bind policy, and local HTTP (does not stop the service).
    Doctor {
        #[arg(long)]
        config: Option<PathBuf>,
        /// Force per-user portable config instead of ProgramData.
        #[arg(long)]
        portable: bool,
    },

    /// Print version information.
    Version,

    /// Windows service management (requires elevation for install/uninstall).
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(Debug, Subcommand)]
enum ServiceAction {
    /// Register the Windows service (LocalService, auto-start).
    Install {
        /// Path to exogpuagent.exe (default: this executable).
        #[arg(long)]
        exe: Option<PathBuf>,
    },
    /// Stop and remove the Windows service.
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
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("exogpuagent: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
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
        }
        Commands::Status { config, portable } => {
            let path = Config::resolve_admin_path(config, portable)?;
            let report = status::run_status(&path);
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::Doctor { config, portable } => {
            let path = Config::resolve_admin_path(config, portable)?;
            let report = doctor::run_doctor(&path);
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.ok {
                return Err("doctor reported problems (see JSON above)".into());
            }
        }
        Commands::Version => {
            println!(
                "exogpuagent {} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS
            );
        }
        Commands::Service { action } => match action {
            ServiceAction::Install { exe } => {
                let exe = match exe {
                    Some(p) => p,
                    None => std::env::current_exe()?,
                };
                service::install(&exe)?;
                println!("service {0} installed (LocalService, auto-start)", {
                    #[cfg(windows)]
                    {
                        service::SERVICE_NAME
                    }
                    #[cfg(not(windows))]
                    {
                        "ExoGPUAgent"
                    }
                });
            }
            ServiceAction::Uninstall => {
                service::uninstall()?;
                println!("service uninstalled");
            }
            ServiceAction::Start => {
                service::start()?;
                println!("service start requested");
            }
            ServiceAction::Stop => {
                service::stop()?;
                println!("service stop requested");
            }
            ServiceAction::Restart => {
                service::restart()?;
                println!("service restart requested");
            }
        },
    }
    Ok(())
}

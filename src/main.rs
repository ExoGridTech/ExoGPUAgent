//! ExoGPUAgent — local-first NVIDIA GPU telemetry HTTP agent.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use exogpuagent::config::Config;
use exogpuagent::gpu::{CollectorError, GpuCollector, GpuSnapshot, NvmlCollector};
use exogpuagent::http::{app_router, AppState};

#[derive(Debug, Parser)]
#[command(
    name = "exogpuagent",
    about = "Lightweight local NVIDIA GPU telemetry agent",
    version
)]
struct Cli {
    /// Path to config.toml (default: XDG / AppData ExoGPUAgent config).
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("exogpuagent: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let config_path = match cli.config {
        Some(path) => path,
        None => Config::default_path()?,
    };

    let config = Config::load_or_create(&config_path)?;
    let addr: SocketAddr = config.socket_addr()?;

    if !config.is_loopback() {
        eprintln!("warning: binding to non-loopback address {addr} (default is 127.0.0.1)");
    }

    // NVML is initialized once at startup, off the request path.
    let collector = match NvmlCollector::init() {
        Ok(c) => {
            eprintln!(
                "NVML ready ({} GPU(s)); config {}",
                c.gpu_count().unwrap_or(0),
                config_path.display()
            );
            Arc::new(c) as Arc<dyn GpuCollector>
        }
        Err(err) => {
            eprintln!(
                "warning: NVML unavailable at startup ({err}); /health and /gpu will return 503 until the driver is available (restart required after fix)"
            );
            // Keep serving so /health returns a proper 503 JSON contract.
            Arc::new(FailedCollector {
                message: err.to_string(),
            }) as Arc<dyn GpuCollector>
        }
    };

    let state = AppState { collector };
    let app = app_router(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Collector that always reports unavailable (NVML failed at startup).
struct FailedCollector {
    message: String,
}

impl GpuCollector for FailedCollector {
    fn gpu_count(&self) -> Result<usize, CollectorError> {
        Err(CollectorError::Unavailable(self.message.clone()))
    }

    fn collect(&self) -> Result<Vec<GpuSnapshot>, CollectorError> {
        Err(CollectorError::Unavailable(self.message.clone()))
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

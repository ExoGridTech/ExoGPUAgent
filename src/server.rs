//! HTTP agent runtime (shared by console `run` and Windows service).

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::watch;

use crate::config::Config;
use crate::gpu::{CollectorError, GpuCollector, GpuSnapshot, NvmlCollector};
use crate::http::{app_router, AppState};
use crate::net_policy;

#[derive(Debug)]
pub enum RunError {
    Config(crate::config::ConfigError),
    Net(crate::net_policy::NetPolicyError),
    Io(std::io::Error),
    Bind(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(e) => write!(f, "{e}"),
            Self::Net(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Bind(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RunError {}

impl From<crate::config::ConfigError> for RunError {
    fn from(value: crate::config::ConfigError) -> Self {
        Self::Config(value)
    }
}

impl From<crate::net_policy::NetPolicyError> for RunError {
    fn from(value: crate::net_policy::NetPolicyError) -> Self {
        Self::Net(value)
    }
}

impl From<std::io::Error> for RunError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Load config, enforce net policy, serve until `shutdown` is true.
pub async fn run_with_shutdown(
    config_path: &Path,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), RunError> {
    let config = Config::load_or_create(config_path)?;
    net_policy::validate(&config)?;
    let addr: SocketAddr = config.socket_addr()?;

    eprintln!(
        "exogpuagent: config {} ({})",
        config_path.display(),
        config.safe_summary()
    );

    let collector = match NvmlCollector::init() {
        Ok(c) => {
            eprintln!(
                "exogpuagent: NVML ready ({} GPU(s))",
                c.gpu_count().unwrap_or(0)
            );
            Arc::new(c) as Arc<dyn GpuCollector>
        }
        Err(err) => {
            eprintln!(
                "exogpuagent: warning: NVML unavailable ({err}); /health and /gpu will return 503 until the driver is available (service restart required after fix)"
            );
            Arc::new(FailedCollector {
                message: err.to_string(),
            }) as Arc<dyn GpuCollector>
        }
    };

    let state = AppState { collector };
    let app = app_router(state);

    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        RunError::Bind(format!(
            "failed to bind {addr}: {e} (is another process already using this port? stop it or change port in config.toml)"
        ))
    })?;
    eprintln!("exogpuagent: listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            loop {
                if *shutdown.borrow() {
                    break;
                }
                if shutdown.changed().await.is_err() {
                    break;
                }
            }
        })
        .await?;

    Ok(())
}

/// Console / portable run with Ctrl+C (and SIGTERM on Unix).
pub async fn run_foreground(config_path: &Path) -> Result<(), RunError> {
    let (tx, rx) = watch::channel(false);
    let signal_tx = tx.clone();
    tokio::spawn(async move {
        wait_for_ctrl_c().await;
        let _ = signal_tx.send(true);
    });
    run_with_shutdown(config_path, rx).await
}

async fn wait_for_ctrl_c() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

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

//! HTTP agent runtime (shared by console `run` and Windows service).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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
///
/// If `ready` is provided, sends `Ok(addr)` after a successful bind (or `Err` on
/// failure) so callers (e.g. Windows SCM) can delay reporting `Running`.
pub async fn run_with_shutdown(
    config_path: &Path,
    mut shutdown: watch::Receiver<bool>,
    ready: Option<mpsc::Sender<Result<SocketAddr, String>>>,
) -> Result<(), RunError> {
    let notify = |result: Result<SocketAddr, String>| {
        if let Some(tx) = &ready {
            let _ = tx.send(result);
        }
    };

    let config = match Config::load_or_create(config_path) {
        Ok(c) => c,
        Err(e) => {
            notify(Err(format!("config: {e}")));
            return Err(e.into());
        }
    };

    if let Err(e) = net_policy::validate(&config) {
        notify(Err(format!("net policy: {e}")));
        return Err(e.into());
    }

    let addr: SocketAddr = match config.socket_addr() {
        Ok(a) => a,
        Err(e) => {
            notify(Err(format!("socket: {e}")));
            return Err(e.into());
        }
    };

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

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            let msg = format!(
                "failed to bind {addr}: {e} (is another process already using this port? stop it or change port in config.toml)"
            );
            notify(Err(msg.clone()));
            return Err(RunError::Bind(msg));
        }
    };

    notify(Ok(addr));
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
    run_with_shutdown(config_path, rx, None).await
}

/// Run the HTTP agent on a dedicated OS thread that **block_on**s the Tokio
/// current-thread runtime (required for Windows SCM: never `spawn` then block
/// the same thread on a sync `recv` without driving the runtime).
pub struct AgentWorker {
    shutdown: watch::Sender<bool>,
    join: Option<thread::JoinHandle<Result<(), String>>>,
}

impl AgentWorker {
    /// Spawn worker, wait until bind succeeds (or fail).
    pub fn start(
        config_path: PathBuf,
        ready_timeout: Duration,
    ) -> Result<(Self, SocketAddr), String> {
        let (watch_tx, watch_rx) = watch::channel(false);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<SocketAddr, String>>();

        let join = thread::Builder::new()
            .name("exogpuagent-http".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| format!("tokio runtime: {e}"))?;
                rt.block_on(async {
                    run_with_shutdown(&config_path, watch_rx, Some(ready_tx))
                        .await
                        .map_err(|e| e.to_string())
                })
            })
            .map_err(|e| format!("spawn http worker: {e}"))?;

        let addr = match ready_rx.recv_timeout(ready_timeout) {
            Ok(Ok(addr)) => addr,
            Ok(Err(e)) => {
                let _ = watch_tx.send(true);
                let _ = join.join();
                return Err(e);
            }
            Err(_) => {
                let _ = watch_tx.send(true);
                let _ = join.join();
                return Err(format!(
                    "HTTP server did not become ready within {ready_timeout:?}"
                ));
            }
        };

        Ok((
            Self {
                shutdown: watch_tx,
                join: Some(join),
            },
            addr,
        ))
    }

    pub fn stop(mut self) -> Result<(), String> {
        let _ = self.shutdown.send(true);
        if let Some(join) = self.join.take() {
            match join.join() {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("HTTP worker panicked".into()),
            }
        } else {
            Ok(())
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn worker_listens_and_serves_health() {
        let dir = std::env::temp_dir().join(format!(
            "exogpuagent-worker-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.toml");
        // Bind port 0 is not supported by our config (fixed port). Pick a high ephemeral-ish port.
        // Use an OS-assigned port via a probe then write it into config.
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let raw = format!("bind = \"127.0.0.1\"\nport = {port}\naccess_mode = \"localhost\"\n");
        std::fs::write(&cfg_path, raw).unwrap();

        let (worker, addr) =
            AgentWorker::start(cfg_path, Duration::from_secs(5)).expect("worker start");
        assert_eq!(addr.port(), port);

        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .write_all(
                format!(
                    "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        assert!(
            buf.contains("200") || buf.contains("503"),
            "unexpected response: {buf}"
        );

        worker.stop().expect("worker stop");
    }

    #[test]
    fn current_thread_spawn_without_drive_does_not_run() {
        // Documents the Windows service bug: spawn + block elsewhere starves tasks.
        use std::sync::atomic::{AtomicBool, Ordering};
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.spawn(async move {
            flag.store(true, Ordering::SeqCst);
        });
        // Do NOT block_on — task must not have run.
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !ran.load(Ordering::SeqCst),
            "spawned task ran without driving the current-thread runtime"
        );
    }
}

//! Native Windows Service Control Manager integration.

use std::ffi::OsString;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

use crate::config::Config;
use crate::server;

pub const SERVICE_NAME: &str = "ExoGPUAgent";
const SERVICE_DISPLAY: &str = "ExoGPUAgent NVIDIA Telemetry";
const SERVICE_DESC: &str = "Local-first NVIDIA GPU telemetry agent (localhost HTTP)";

/// Returns true if this process was launched by the SCM and the service has exited.
pub fn maybe_dispatch_as_service() -> bool {
    matches!(
        service_dispatcher::start(SERVICE_NAME, ffi_service_main),
        Ok(())
    )
}

windows_service::define_windows_service!(ffi_service_main, service_main);

fn service_main(_args: Vec<OsString>) {
    if let Err(e) = run_service() {
        eprintln!("exogpuagent service error: {e}");
    }
}

fn run_service() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (shutdown_tx, shutdown_rx) = mpsc::channel();

    let event_handler = move |control| -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let config_path = Config::machine_path();
    let _ = Config::save_if_absent(&config_path);
    let _ = std::fs::create_dir_all(Config::machine_logs_dir());

    let (watch_tx, watch_rx) = tokio::sync::watch::channel(false);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let server = rt.spawn(async move {
        if let Err(e) = server::run_with_shutdown(&config_path, watch_rx).await {
            eprintln!("exogpuagent: server stopped: {e}");
        }
    });

    let _ = shutdown_rx.recv();
    let _ = watch_tx.send(true);

    let _ = rt.block_on(async {
        let _ = tokio::time::timeout(Duration::from_secs(10), server).await;
    });

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

fn service_info_for(exe: &Path) -> ServiceInfo {
    ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::Auto,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe.to_path_buf(),
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: Some(OsString::from(r"NT AUTHORITY\LocalService")),
        account_password: None,
    }
}

pub fn install(exe: &Path) -> Result<(), String> {
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| format!("open SCM: {e}"))?;

    let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let _ = Config::save_if_absent(&Config::machine_path());
    let _ = std::fs::create_dir_all(Config::machine_logs_dir());

    let info = service_info_for(&exe);

    match manager.open_service(
        SERVICE_NAME,
        ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::QUERY_STATUS,
    ) {
        Ok(service) => {
            service
                .change_config(&info)
                .map_err(|e| format!("update service config: {e}"))?;
            let _ = service.set_description(SERVICE_DESC);
            Ok(())
        }
        Err(_) => {
            let service = manager
                .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
                .map_err(|e| format!("create service: {e}"))?;
            let _ = service.set_description(SERVICE_DESC);
            Ok(())
        }
    }
}

pub fn uninstall() -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| format!("open SCM: {e}"))?;
    let service = manager
        .open_service(
            SERVICE_NAME,
            ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| format!("open service: {e}"))?;

    if let Ok(status) = service.query_status() {
        if status.current_state != ServiceState::Stopped {
            let _ = service.stop();
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    service
        .delete()
        .map_err(|e| format!("delete service: {e}"))?;
    Ok(())
}

pub fn start() -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| format!("open SCM: {e}"))?;
    let service = manager
        .open_service(
            SERVICE_NAME,
            ServiceAccess::START | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| format!("open service: {e}"))?;
    match service.start::<&str>(&[]) {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            if msg.to_lowercase().contains("already") {
                Ok(())
            } else {
                Err(format!("start: {e}"))
            }
        }
    }
}

pub fn stop() -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| format!("open SCM: {e}"))?;
    let service = manager
        .open_service(
            SERVICE_NAME,
            ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| format!("open service: {e}"))?;
    match service.stop() {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            if msg.to_lowercase().contains("not started") || msg.to_lowercase().contains("already")
            {
                Ok(())
            } else {
                Err(format!("stop: {e}"))
            }
        }
    }
}

pub fn restart() -> Result<(), String> {
    let _ = stop();
    std::thread::sleep(Duration::from_secs(2));
    start()
}

pub fn query_state() -> String {
    let manager = match ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
    {
        Ok(m) => m,
        Err(e) => return format!("scm unavailable ({e})"),
    };
    let service = match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
        Ok(s) => s,
        Err(_) => return "not installed".into(),
    };
    match service.query_status() {
        Ok(st) => format!("{:?}", st.current_state),
        Err(e) => format!("query failed ({e})"),
    }
}

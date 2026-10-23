//! OS service helpers. Phase 2B implements Windows SCM; Linux is Phase 2C.

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{
    install, maybe_dispatch_as_service, restart, start, stop, uninstall, SERVICE_NAME,
};

/// Human-readable service state for `status`.
pub fn service_state_string() -> String {
    #[cfg(windows)]
    {
        windows::query_state()
    }
    #[cfg(not(windows))]
    {
        "n/a (Windows service support only in Phase 2B; Linux is Phase 2C)".into()
    }
}

#[cfg(not(windows))]
pub fn maybe_dispatch_as_service() -> bool {
    false
}

#[cfg(not(windows))]
pub fn install(_exe: &std::path::Path) -> Result<(), String> {
    Err("Windows service install is only available on Windows".into())
}

#[cfg(not(windows))]
pub fn uninstall() -> Result<(), String> {
    Err("Windows service uninstall is only available on Windows".into())
}

#[cfg(not(windows))]
pub fn start() -> Result<(), String> {
    Err("Windows service start is only available on Windows".into())
}

#[cfg(not(windows))]
pub fn stop() -> Result<(), String> {
    Err("Windows service stop is only available on Windows".into())
}

#[cfg(not(windows))]
pub fn restart() -> Result<(), String> {
    Err("Windows service restart is only available on Windows".into())
}

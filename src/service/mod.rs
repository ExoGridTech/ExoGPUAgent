//! OS service helpers. Windows SCM (2B) + Linux systemd (2C-B).

#[cfg(windows)]
mod windows;

#[cfg(all(unix, not(windows)))]
mod linux;

#[cfg(windows)]
pub use windows::{
    install, maybe_dispatch_as_service, restart, start, stop, uninstall, SERVICE_NAME,
};

#[cfg(all(unix, not(windows)))]
pub use linux::{
    install, maybe_dispatch_as_service, restart, start, stop, uninstall, SERVICE_NAME,
};

/// Human-readable service state for `status`.
pub fn service_state_string() -> String {
    #[cfg(windows)]
    {
        windows::query_state()
    }
    #[cfg(all(unix, not(windows)))]
    {
        linux::query_state()
    }
    #[cfg(not(any(windows, unix)))]
    {
        "n/a".into()
    }
}

//! Local admin Mini GUI (Phase 2C-C2): separate loopback listener.

mod rate_limit;
mod session;
mod snapshot;
mod static_files;
mod telemetry_status;

pub mod routes;

pub use rate_limit::LoginRateLimiter;
pub use routes::{admin_router, AdminState};
pub use session::SessionStore;
pub use snapshot::SnapshotCache;
pub use telemetry_status::TelemetryStatus;

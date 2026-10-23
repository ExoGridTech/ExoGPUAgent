//! HTTP surface: `/health` and `/gpu`.

mod routes;

pub use routes::{app_router, AppState};

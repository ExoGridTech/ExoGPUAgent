//! Embedded Mini GUI assets (no CDN).

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "gui/"]
#[prefix = ""]
pub struct GuiAssets;

pub fn serve_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    // Only allow known asset paths (no path traversal via embed API).
    let allowed = matches!(path, "index.html" | "app.js" | "styles.css" | "favicon.svg");
    if !allowed {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    match GuiAssets::get(path) {
        Some(file) => {
            let mime = match path {
                p if p.ends_with(".html") => "text/html; charset=utf-8",
                p if p.ends_with(".js") => "application/javascript; charset=utf-8",
                p if p.ends_with(".css") => "text/css; charset=utf-8",
                p if p.ends_with(".svg") => "image/svg+xml",
                _ => "application/octet-stream",
            };
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "no-store"),
                ],
                file.data,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

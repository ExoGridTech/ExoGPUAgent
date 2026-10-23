//! Minimal localhost HTTP GET helper (no outbound cloud clients).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LocalHttpError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid HTTP response")]
    InvalidResponse,
    #[error("HTTP {0}")]
    Status(u16),
}

/// GET a path on `127.0.0.1:port` (loopback only).
pub fn get_loopback(port: u16, path: &str) -> Result<(u16, String), LocalHttpError> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;

    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes())?;

    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;

    let (header, body) = buf
        .split_once("\r\n\r\n")
        .ok_or(LocalHttpError::InvalidResponse)?;
    let status_line = header
        .lines()
        .next()
        .ok_or(LocalHttpError::InvalidResponse)?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .ok_or(LocalHttpError::InvalidResponse)?
        .parse()
        .map_err(|_| LocalHttpError::InvalidResponse)?;

    Ok((status, body.to_string()))
}

pub fn get_json_loopback(
    port: u16,
    path: &str,
) -> Result<(u16, serde_json::Value), LocalHttpError> {
    let (status, body) = get_loopback(port, path)?;
    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| LocalHttpError::InvalidResponse)?;
    Ok((status, value))
}

//! Persistent startup/error logs for the Windows service (ProgramData).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;

fn log_path() -> PathBuf {
    Config::machine_logs_dir().join("service.log")
}

/// Append a line to the machine service log and mirror to stderr.
pub fn write(message: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("[{ts}] {message}");
    eprintln!("exogpuagent: {message}");

    let dir = Config::machine_logs_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}

pub fn path_display() -> String {
    log_path().display().to_string()
}

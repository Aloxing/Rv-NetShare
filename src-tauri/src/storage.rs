//! Disk persistence helpers for the share-link app.
//!
//! Disk persistence helpers for the share-link app.
//!
//! Stable app dir (dirs::data_local_dir()/rv-netshare/):
//!   * shares.json - active share sessions, restored on launch
//!   * sites.json - static website sessions, restored on launch
//!
//! Configurable save dir (defaults to the same folder):
//!   * history.json - array of ShareAccess records (one per download)

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime};

use crate::state::{ShareSession, SiteSession};

/// Serializes read-modify-write cycles so concurrent downloads cannot
/// corrupt history.json.
static HISTORY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccessRecord {
    pub id: String,
    pub share_id: String,
    pub share_name: String,
    pub path: String,
    pub bytes: u64,
    pub timestamp: i64,
    pub peer: String,
    pub user_agent: Option<String>,
    pub status: String,
}

pub fn ensure_data_dir(state: &crate::state::AppState) -> io::Result<PathBuf> {
    let dir = state.data_dir.lock().unwrap().clone();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn read_history(dir: &Path) -> io::Result<Vec<AccessRecord>> {
    let path = dir.join("history.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write_history(dir: &Path, history: &[AccessRecord]) -> io::Result<()> {
    let path = dir.join("history.json");
    let raw = serde_json::to_string_pretty(history)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(path, raw)
}

/// Append one access record to the on-disk history, newest first, capped at
/// the same 500 entries the UI keeps in memory.
pub fn append_history(dir: &Path, record: &AccessRecord) -> io::Result<()> {
    let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut history = read_history(dir)?;
    history.insert(0, record.clone());
    history.truncate(500);
    write_history(dir, &history)
}

pub fn remove_history_record(dir: &Path, id: &str) -> io::Result<()> {
    let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut history = read_history(dir)?;
    history.retain(|record| record.id != id);
    write_history(dir, &history)
}

pub fn read_shares(dir: &Path) -> io::Result<Vec<ShareSession>> {
    let path = dir.join("shares.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write_shares(dir: &Path, shares: &[ShareSession]) -> io::Result<()> {
    let path = dir.join("shares.json");
    let raw = serde_json::to_string_pretty(shares)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(path, raw)
}

pub fn read_sites(dir: &Path) -> io::Result<Vec<SiteSession>> {
    let path = dir.join("sites.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write_sites(dir: &Path, sites: &[SiteSession]) -> io::Result<()> {
    let path = dir.join("sites.json");
    let raw = serde_json::to_string_pretty(sites)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(path, raw)
}

/// Reveal the given directory in the OS file manager.
pub fn reveal_in_explorer(path: &Path) -> Result<(), String> {
    let target = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    open_in_os(target).map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn open_in_os(path: &Path) -> io::Result<()> {
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "macos")]
fn open_in_os(path: &Path) -> io::Result<()> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_in_os(path: &Path) -> io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
}

pub fn emit_access_event<R: Runtime>(app: &AppHandle<R>, payload: &AccessRecord) {
    let _ = app.emit("share-access", payload);
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Read directory entries sorted: directories first, then files;
/// alphabetical within each group.
pub fn list_dir(path: &Path) -> io::Result<Vec<(String, bool, u64)>> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if file_type.is_dir() {
            dirs.push((name, true, size));
        } else if file_type.is_file() {
            files.push((name, false, size));
        }
    }
    dirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    dirs.append(&mut files);
    Ok(dirs)
}

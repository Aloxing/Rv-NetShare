// Library entry point for the local-area-network Tauri app.
//
// Responsibilities:
//   * expose Tauri commands used by the Vue 3 frontend
//   * run a tiny embedded HTTP server (built only on std::net) that serves
//     locally created file/folder shares over the LAN
//   * persist access history + config on disk under the user-configurable
//     save directory
//
// The implementation deliberately avoids non-essential third-party
// crates so the project keeps building in environments where crates.io
// is unreachable.

mod html;
mod net;
pub mod server;
pub mod state;
mod storage;

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::net::{get_hostname, get_local_ip};
use crate::server::{start_server, stop_server, ServerStatus};
use crate::state::{AppState, ShareKind, ShareSession};
use crate::storage::{
    ensure_data_dir, read_history, read_shares, reveal_in_explorer, write_history, write_shares,
    AccessRecord,
};

#[derive(Default)]
struct ServerHandle(Mutex<Option<ServerStatus>>);

#[derive(Serialize)]
struct InitialState {
    local_ip: String,
    hostname: String,
    port: u16,
    save_dir: String,
    shares: Vec<ShareSession>,
    history: Vec<AccessRecord>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct AppConfig {
    save_dir: Option<String>,
}

const DEFAULT_PORT: u16 = 48721;
const PORT_FALLBACK_RANGE: std::ops::RangeInclusive<u16> = 48721..=48799;

fn default_app_dir() -> PathBuf {
    dirs::data_local_dir()
        .map(|p| p.join("rv-netshare"))
        .unwrap_or_else(|| std::env::temp_dir().join("rv-netshare"))
}

fn random_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let mut h: u64 = 0xcbf29ce484222325_u64 ^ nanos ^ pid.wrapping_mul(0x9E3779B97F4A7C15);
    h = h.wrapping_mul(0x100000001b3);
    h ^= nanos;
    h = h.wrapping_mul(0x100000001b3);
    const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut out = String::with_capacity(10);
    let mut x = h | 1;
    for _ in 0..10 {
        out.push(ALPHA[(x % 36) as usize] as char);
        x /= 36;
        x = x.wrapping_mul(0x100000001b3);
    }
    out
}

/// Windows `canonicalize()` can return `\\?\`-prefixed extended paths; strip
/// that verbosity before the path is stored, shown, or sent back to the UI.
fn strip_extended_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

fn config_path(state: &AppState) -> PathBuf {
    state.base_dir.lock().unwrap().join("config.json")
}

fn read_config(state: &AppState) -> AppConfig {
    let p = config_path(state);
    let raw = match std::fs::read_to_string(&p) {
        Ok(s) => s,
        Err(_) => return AppConfig::default(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn write_config(state: &AppState, cfg: &AppConfig) {
    let p = config_path(state);
    if let Ok(raw) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(p, raw);
    }
}

fn load_shares(state: &AppState) -> Vec<ShareSession> {
    let dir = state.base_dir.lock().unwrap().clone();
    read_shares(&dir).unwrap_or_default()
}

fn persist_shares(state: &AppState) {
    let dir = state.base_dir.lock().unwrap().clone();
    let shares: Vec<ShareSession> = state.shares.lock().unwrap().values().cloned().collect();
    if let Err(e) = write_shares(&dir, &shares) {
        eprintln!("[share] failed to persist shares: {e}");
    }
}

fn resolve_data_dir(state: &AppState) -> PathBuf {
    let cfg = read_config(state);
    if let Some(custom) = cfg.save_dir.as_deref() {
        let p = PathBuf::from(custom);
        if !p.as_os_str().is_empty() && p.exists() {
            return p;
        }
    }
    state.data_dir.lock().unwrap().clone()
}

/// Single-level directory stats: counts direct children and sums the
/// sizes of direct files. Does NOT recurse into subdirectories so that
/// even huge trees can be added to a share instantly. Subdirectory
/// browsing is delegated to `serve_share` -> `list_dir` which is
/// already single-level.
fn share_path_stats(path: &PathBuf) -> std::io::Result<(u64, u64)> {
    let meta = std::fs::metadata(path)?;
    if meta.is_file() {
        return Ok((meta.len(), meta.len()));
    }
    let mut files = 0u64;
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if file_type.is_file() {
            files += 1;
            bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
        } else if file_type.is_dir() {
            files += 1;
        }
    }
    Ok((files, bytes))
}

#[tauri::command]
fn app_initial_state(state: State<'_, AppState>) -> Result<InitialState, String> {
    let local_ip = get_local_ip().unwrap_or_else(|_| "0.0.0.0".to_string());
    let hostname = get_hostname();
    let save_dir = ensure_data_dir(&state).map_err(|e| e.to_string())?;
    let shares = state.shares.lock().unwrap().values().cloned().collect();
    let history = read_history(&save_dir).unwrap_or_default();
    Ok(InitialState {
        local_ip,
        hostname,
        port: *state.port.lock().unwrap(),
        save_dir: save_dir.to_string_lossy().into_owned(),
        shares,
        history,
    })
}

#[tauri::command]
fn start_receiver_server(
    app: tauri::AppHandle,
    handle: State<'_, ServerHandle>,
) -> Result<u16, String> {
    {
        let guard = handle.0.lock().unwrap();
        if let Some(s) = guard.as_ref() {
            return Ok(s.port);
        }
    }
    let status = try_bind(&app).map_err(|e| e.to_string())?;
    let bound_port = status.port;
    *handle.0.lock().unwrap() = Some(status);
    Ok(bound_port)
}

fn try_bind(app: &tauri::AppHandle) -> Result<ServerStatus, String> {
    let mut last_err: Option<String> = None;
    for port in PORT_FALLBACK_RANGE {
        match start_server(app.clone(), port) {
            Ok(s) => {
                if port != DEFAULT_PORT {
                    eprintln!("[share] default port {DEFAULT_PORT} busy, fell back to {port}");
                }
                return Ok(s);
            }
            Err(e) => {
                last_err = Some(e);
                // best-effort: keep trying until we find a free port
            }
        }
    }
    Err(last_err.unwrap_or_else(|| "no available port".to_string()))
}

#[tauri::command]
fn stop_receiver_server(handle: State<'_, ServerHandle>) -> Result<(), String> {
    let guard = handle.0.lock().unwrap();
    if let Some(s) = guard.as_ref() {
        stop_server(s).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn list_shares(state: State<'_, AppState>) -> Vec<ShareSession> {
    state.shares.lock().unwrap().values().cloned().collect()
}

#[tauri::command]
fn create_share(state: State<'_, AppState>, path: String) -> Result<ShareSession, String> {
    let path = PathBuf::from(path.trim());
    if !path.exists() {
        return Err(format!("路径不存在: {}", path.display()));
    }
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    let kind = if meta.is_dir() {
        ShareKind::Folder
    } else {
        ShareKind::File
    };
    let (size, total_bytes) = share_path_stats(&path).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("share")
        .to_string();
    let session = ShareSession {
        id: random_id(),
        name,
        path: path.to_string_lossy().into_owned(),
        kind,
        size,
        total_bytes,
        created_at: storage::now_secs(),
    };
    state
        .shares
        .lock()
        .unwrap()
        .insert(session.id.clone(), session.clone());
    persist_shares(&state);
    Ok(session)
}

#[tauri::command]
fn remove_share(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.shares.lock().unwrap().remove(&id);
    persist_shares(&state);
    Ok(())
}

#[tauri::command]
fn clear_shares(state: State<'_, AppState>) -> Result<(), String> {
    state.shares.lock().unwrap().clear();
    persist_shares(&state);
    Ok(())
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err("\u{8def}\u{5f84}\u{4e0d}\u{5b58}\u{5728}".to_string());
    }
    reveal_in_explorer(&p)
}

#[tauri::command]
fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let save_dir = ensure_data_dir(&state).map_err(|e| e.to_string())?;
    write_history(&save_dir, &[]).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_history_record(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let save_dir = ensure_data_dir(&state).map_err(|e| e.to_string())?;
    storage::remove_history_record(&save_dir, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn refresh_local_ip() -> Result<String, String> {
    get_local_ip().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_save_dir(state: State<'_, AppState>) -> Result<String, String> {
    Ok(ensure_data_dir(&state)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
fn set_save_dir(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let p = PathBuf::from(path.trim());
    if p.as_os_str().is_empty() {
        return Err("路径不能为空".to_string());
    }
    if !p.exists() {
        return Err(format!("路径不存在: {}", p.display()));
    }
    if !p.is_dir() {
        return Err("必须是文件夹".to_string());
    }
    let mut cfg = read_config(&state);
    cfg.save_dir = Some(p.to_string_lossy().into_owned());
    write_config(&state, &cfg);
    // Re-anchor the in-memory dir so subsequent commands see the new path.
    *state.data_dir.lock().unwrap() = p.clone();
    ensure_data_dir(&state).map_err(|e| e.to_string())?;
    Ok(p.to_string_lossy().into_owned())
}

#[derive(Serialize)]
struct PathCheck {
    path: String,
    valid: bool,
    exists: bool,
    is_dir: bool,
    is_file: bool,
    size: u64,
    basename: String,
}

#[tauri::command]
fn check_paths(paths: Vec<String>) -> Vec<PathCheck> {
    paths.into_iter().map(|p| check_one(&p)).collect()
}

fn check_one(raw: &str) -> PathCheck {
    let trimmed = raw.trim();
    let stripped = strip_quotes(trimmed);
    let pb = PathBuf::from(&stripped);
    let meta = std::fs::metadata(&pb).ok();
    let (exists, is_dir, is_file, size) = match meta.as_ref() {
        Some(m) => (true, m.is_dir(), m.is_file(), m.len()),
        None => (false, false, false, 0),
    };
    PathCheck {
        basename: pb.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(),
        path: stripped,
        valid: exists,
        exists,
        is_dir,
        is_file,
        size,
    }
}

fn strip_quotes(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 {
        let bytes = t.as_bytes();
        if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'') {
            return t[1..t.len() - 1].trim().to_string();
        }
    }
    t.to_string()
}

#[tauri::command]
fn resolve_share_paths(state: State<'_, AppState>, paths: Vec<String>) -> Result<Vec<ShareSession>, String> {
    let mut out = Vec::new();
    for raw in paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() { continue; }
        let stripped = strip_quotes(trimmed);
        let path = PathBuf::from(&stripped);
        let meta = match std::fs::metadata(&path) { Ok(m) => m, Err(_) => continue };
        let kind = if meta.is_dir() { ShareKind::Folder } else { ShareKind::File };
        let (size, total_bytes) = share_path_stats(&path).unwrap_or((0, 0));
        let canonical = std::fs::canonicalize(&path).unwrap_or(path.clone());
        let existing = {
            let shares = state.shares.lock().unwrap();
            shares.values().find(|s| {
                let p = PathBuf::from(&s.path);
                std::fs::canonicalize(&p).ok() == Some(canonical.clone())
            }).cloned()
        };
        if let Some(s) = existing { out.push(s); continue; }
        let name = canonical.file_name().and_then(|s| s.to_str()).unwrap_or("share").to_string();
        let session = ShareSession {
            id: random_id(),
            name,
            path: strip_extended_prefix(&canonical.to_string_lossy()),
            kind,
            size,
            total_bytes,
            created_at: storage::now_secs(),
        };
        state.shares.lock().unwrap().insert(session.id.clone(), session.clone());
        out.push(session);
    }
    persist_shares(&state);
    Ok(out)
}

#[tauri::command]
fn read_file_meta(path: String) -> Result<PathCheck, String> {
    Ok(check_one(&path))
}


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Stable app dir holds config.json + shares.json; the user can
            // redirect only download history via the Settings panel.
            let default_dir = default_app_dir();
            std::fs::create_dir_all(&default_dir).ok();

            let app_state = AppState::new(default_dir.clone(), default_dir.clone(), DEFAULT_PORT);

            // If a previous run redirected the save dir, honour it before
            // starting the server or loading anything else.
            let probe = AppState::new(default_dir.clone(), default_dir.clone(), DEFAULT_PORT);
            let resolved = resolve_data_dir(&probe);
            if resolved != default_dir {
                *app_state.data_dir.lock().unwrap() = resolved.clone();
                std::fs::create_dir_all(&resolved).ok();
            }

            app.manage(app_state);
            app.manage(ServerHandle::default());

            let handle = app.state::<ServerHandle>();
            let state = app.state::<AppState>();

            // Restore active shares so old links keep working after a restart.
            for mut session in load_shares(&state) {
                session.path = strip_extended_prefix(&session.path);
                state.shares.lock().unwrap().insert(session.id.clone(), session);
            }

            if let Ok(status) = try_bind(&app.handle()) {
                let bound = status.port;
                *state.port.lock().unwrap() = bound;
                *handle.0.lock().unwrap() = Some(status);
            }

            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_title("RV NetShare");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_initial_state,
            start_receiver_server,
            stop_receiver_server,
            list_shares,
            create_share,
            remove_share,
            clear_shares,
            open_path,
            clear_history,
            remove_history_record,
            get_save_dir,
            set_save_dir,
            refresh_local_ip,
            check_paths,
            resolve_share_paths,
            read_file_meta,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

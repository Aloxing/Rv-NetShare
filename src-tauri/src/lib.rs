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
mod tunnel;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::net::{get_hostname, get_local_ip};
use crate::server::{start_server, start_site_server, stop_server, ServerStatus};
use crate::state::{
    AppState, ReceiveEncryption, ReceiveSession, ShareKind, ShareSession, SiteSession,
};
use crate::storage::{
    ensure_data_dir, read_history, read_receivers, read_shares, read_sites, reveal_in_explorer,
    write_history, write_receivers, write_shares, write_sites, AccessRecord,
};
use crate::tunnel::TunnelManager;

#[derive(Default)]
struct ServerHandle(Mutex<Option<ServerStatus>>);

#[derive(Default)]
struct SiteServerHandle(Mutex<HashMap<String, ServerStatus>>);

#[derive(Serialize)]
struct InitialState {
    local_ip: String,
    hostname: String,
    port: u16,
    save_dir: String,
    shares: Vec<ShareSession>,
    sites: Vec<SiteSession>,
    receivers: Vec<ReceiveSession>,
    receive_dir: String,
    share_port: Option<u16>,
    site_port: Option<u16>,
    ngrok_authtoken: Option<String>,
    receive_common_password: Option<String>,
    history: Vec<AccessRecord>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct AppConfig {
    save_dir: Option<String>,
    share_port: Option<u16>,
    site_port: Option<u16>,
    ngrok_authtoken: Option<String>,
    receive_common_password: Option<String>,
    receive_dir: Option<String>,
}

const DEFAULT_PORT: u16 = 48721;
const DEFAULT_SITE_PORT: u16 = 48800;
const PORT_FALLBACK_LIMIT: u16 = 100;

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

fn configured_share_port(state: &AppState) -> u16 {
    let port = read_config(state).share_port.unwrap_or(DEFAULT_PORT);
    if port == 0 {
        DEFAULT_PORT
    } else {
        port
    }
}

fn configured_site_port(state: &AppState) -> u16 {
    let port = read_config(state).site_port.unwrap_or(DEFAULT_SITE_PORT);
    if port == 0 {
        DEFAULT_SITE_PORT
    } else {
        port
    }
}

pub(crate) fn receive_common_password(state: &AppState) -> Option<String> {
    read_config(state)
        .receive_common_password
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub(crate) fn receive_root_dir(state: &AppState) -> PathBuf {
    let cfg = read_config(state);
    if let Some(custom) = cfg.receive_dir.as_deref() {
        let path = PathBuf::from(custom);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    state.data_dir.lock().unwrap().clone().join("receive")
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

fn load_sites(state: &AppState) -> Vec<SiteSession> {
    let dir = state.base_dir.lock().unwrap().clone();
    read_sites(&dir).unwrap_or_default()
}

fn persist_sites(state: &AppState) {
    let dir = state.base_dir.lock().unwrap().clone();
    let sites: Vec<SiteSession> = state.sites.lock().unwrap().values().cloned().collect();
    if let Err(e) = write_sites(&dir, &sites) {
        eprintln!("[site] failed to persist sites: {e}");
    }
}

fn load_receivers(state: &AppState) -> Vec<ReceiveSession> {
    let dir = state.base_dir.lock().unwrap().clone();
    read_receivers(&dir).unwrap_or_default()
}

fn persist_receivers(state: &AppState) {
    let dir = state.base_dir.lock().unwrap().clone();
    let receivers: Vec<ReceiveSession> =
        state.receivers.lock().unwrap().values().cloned().collect();
    if let Err(e) = write_receivers(&dir, &receivers) {
        eprintln!("[receive] failed to persist receivers: {e}");
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
    let sites = state.sites.lock().unwrap().values().cloned().collect();
    let receivers = state.receivers.lock().unwrap().values().cloned().collect();
    let history = read_history(&save_dir).unwrap_or_default();
    let receive_dir = receive_root_dir(&state);
    let _ = std::fs::create_dir_all(&receive_dir);
    let cfg = read_config(&state);
    Ok(InitialState {
        local_ip,
        hostname,
        port: *state.port.lock().unwrap(),
        save_dir: save_dir.to_string_lossy().into_owned(),
        shares,
        sites,
        receivers,
        receive_dir: receive_dir.to_string_lossy().into_owned(),
        share_port: cfg.share_port,
        site_port: cfg.site_port,
        ngrok_authtoken: cfg.ngrok_authtoken,
        receive_common_password: cfg.receive_common_password,
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
    let base = configured_share_port(&app.state::<AppState>());
    let status = try_bind(&app, base).map_err(|e| e.to_string())?;
    let bound_port = status.port;
    *handle.0.lock().unwrap() = Some(status);
    Ok(bound_port)
}

fn try_bind(app: &tauri::AppHandle, base: u16) -> Result<ServerStatus, String> {
    let mut last_err: Option<String> = None;
    if base != DEFAULT_PORT {
        match start_server(app.clone(), base) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some(e),
        }
    }
    let end = DEFAULT_PORT.saturating_add(PORT_FALLBACK_LIMIT);
    for port in DEFAULT_PORT..=end {
        match start_server(app.clone(), port) {
            Ok(s) => {
                if port != base {
                    eprintln!("[share] configured port {base} busy, fell back to {port}");
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
async fn remove_share(
    state: State<'_, AppState>,
    tunnels: State<'_, TunnelManager>,
    id: String,
) -> Result<(), String> {
    let _ = tunnels.stop(format!("share:{id}"));
    state.shares.lock().unwrap().remove(&id);
    persist_shares(&state);
    Ok(())
}

#[tauri::command]
async fn clear_shares(
    state: State<'_, AppState>,
    tunnels: State<'_, TunnelManager>,
) -> Result<(), String> {
    let ids: Vec<String> = state.shares.lock().unwrap().keys().cloned().collect();
    for id in ids {
        let _ = tunnels.stop(format!("share:{id}"));
    }
    state.shares.lock().unwrap().clear();
    persist_shares(&state);
    Ok(())
}

#[tauri::command]
fn list_sites(state: State<'_, AppState>) -> Vec<SiteSession> {
    state.sites.lock().unwrap().values().cloned().collect()
}

#[tauri::command]
fn start_site_with_fallback(
    app: &tauri::AppHandle,
    state: &AppState,
    site: &mut SiteSession,
) -> Result<ServerStatus, String> {
    if site.port != 0 {
        if let Ok(status) = start_site_server(app.clone(), site.clone(), site.port) {
            site.port = status.port;
            return Ok(status);
        }
    }
    let base = configured_site_port(state);
    let used: HashSet<u16> = state
        .sites
        .lock()
        .unwrap()
        .values()
        .filter(|s| s.id != site.id)
        .map(|s| s.port)
        .filter(|p| *p != 0)
        .collect();
    let mut candidate = base;
    while used.contains(&candidate) && candidate < u16::MAX {
        candidate += 1;
    }
    match start_site_server(app.clone(), site.clone(), candidate) {
        Ok(status) => {
            site.port = status.port;
            Ok(status)
        }
        Err(_) => Err(format!("端口 {candidate} 被占用，无法使用")),
    }
}

#[tauri::command]
fn create_site(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    handles: State<'_, SiteServerHandle>,
    path: String,
) -> Result<SiteSession, String> {
    let path = PathBuf::from(path.trim());
    if !path.exists() {
        return Err(format!("路径不存在: {}", path.display()));
    }
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_dir() {
        return Err("必须是文件夹".to_string());
    }
    let canonical = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
    if !canonical.join("index.html").is_file() && !canonical.join("index.htm").is_file() {
        return Err("文件夹中未找到 index.html".to_string());
    }

    let existing = {
        let sites = state.sites.lock().unwrap();
        sites
            .values()
            .find(|s| {
                let p = PathBuf::from(&s.path);
                std::fs::canonicalize(&p).ok() == Some(canonical.clone())
            })
            .cloned()
    };
    if let Some(s) = existing {
        return Ok(s);
    }

    let (size, total_bytes) = share_path_stats(&canonical).map_err(|e| e.to_string())?;
    let name = canonical
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("site")
        .to_string();
    let mut session = SiteSession {
        id: random_id(),
        name,
        path: strip_extended_prefix(&canonical.to_string_lossy()),
        size,
        total_bytes,
        created_at: storage::now_secs(),
        port: 0,
    };
    let status = start_site_with_fallback(&app, &state, &mut session)?;
    state
        .sites
        .lock()
        .unwrap()
        .insert(session.id.clone(), session.clone());
    handles.0.lock().unwrap().insert(session.id.clone(), status);
    persist_sites(&state);
    Ok(session)
}

#[tauri::command]
async fn remove_site(
    state: State<'_, AppState>,
    handles: State<'_, SiteServerHandle>,
    tunnels: State<'_, TunnelManager>,
    id: String,
) -> Result<(), String> {
    let _ = tunnels.stop(format!("site:{id}"));
    if let Some(status) = handles.0.lock().unwrap().remove(&id) {
        let _ = stop_server(&status);
    }
    state.sites.lock().unwrap().remove(&id);
    persist_sites(&state);
    Ok(())
}

#[tauri::command]
async fn clear_sites(
    state: State<'_, AppState>,
    handles: State<'_, SiteServerHandle>,
    tunnels: State<'_, TunnelManager>,
) -> Result<(), String> {
    let ids: Vec<String> = state.sites.lock().unwrap().keys().cloned().collect();
    for id in ids {
        let _ = tunnels.stop(format!("site:{id}"));
    }
    for (_, status) in handles.0.lock().unwrap().drain() {
        let _ = stop_server(&status);
    }
    state.sites.lock().unwrap().clear();
    persist_sites(&state);
    Ok(())
}

#[tauri::command]
fn list_receivers(state: State<'_, AppState>) -> Vec<ReceiveSession> {
    state.receivers.lock().unwrap().values().cloned().collect()
}

fn normalize_receive_extensions(extensions: Vec<String>) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for raw in extensions {
        let ext = raw.trim().trim_start_matches('.').to_ascii_lowercase();
        if ext == "*" {
            return Ok(vec!["*".to_string()]);
        }
        if ext.is_empty() {
            continue;
        }
        if ext.len() > 16 {
            return Err(format!("扩展名过长: {ext}"));
        }
        if !out.contains(&ext) {
            out.push(ext);
        }
    }
    if out.is_empty() {
        return Err("请至少指定一种文件类型".to_string());
    }
    Ok(out)
}

#[tauri::command]
fn create_receiver(
    state: State<'_, AppState>,
    name: String,
    extensions: Vec<String>,
    encryption: String,
    custom_password: Option<String>,
) -> Result<ReceiveSession, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("名称不能为空".to_string());
    }
    if name.chars().count() > 40 {
        return Err("名称不能超过 40 个字符".to_string());
    }
    let extensions = normalize_receive_extensions(extensions)?;
    let (mode, saved_password) = match encryption.as_str() {
        "none" => (ReceiveEncryption::None, None),
        "common" => {
            if receive_common_password(&state).is_none() {
                return Err("请先在设置中配置通用加密密码".to_string());
            }
            (ReceiveEncryption::Common, None)
        }
        "custom" => {
            let password = custom_password.unwrap_or_default().trim().to_string();
            if password.is_empty() {
                return Err("单独加密必须设置密码".to_string());
            }
            (ReceiveEncryption::Custom, Some(password))
        }
        _ => return Err("加密方式无效".to_string()),
    };
    let session = ReceiveSession {
        id: random_id(),
        name,
        extensions,
        encryption: mode,
        custom_password: saved_password,
        created_at: storage::now_secs(),
        received_count: 0,
        received_bytes: 0,
    };
    state
        .receivers
        .lock()
        .unwrap()
        .insert(session.id.clone(), session.clone());
    persist_receivers(&state);
    Ok(session)
}

#[tauri::command]
fn update_receiver(
    state: State<'_, AppState>,
    id: String,
    name: String,
    extensions: Vec<String>,
    encryption: String,
    custom_password: Option<String>,
) -> Result<ReceiveSession, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("名称不能为空".to_string());
    }
    if name.chars().count() > 40 {
        return Err("名称不能超过 40 个字符".to_string());
    }
    let extensions = normalize_receive_extensions(extensions)?;
    let (mode, saved_password) = match encryption.as_str() {
        "none" => (ReceiveEncryption::None, None),
        "common" => {
            if receive_common_password(&state).is_none() {
                return Err("请先在设置中配置通用加密密码".to_string());
            }
            (ReceiveEncryption::Common, None)
        }
        "custom" => {
            let password = custom_password.unwrap_or_default().trim().to_string();
            let receivers = state.receivers.lock().unwrap();
            let existing = receivers
                .get(&id)
                .ok_or_else(|| "接收卡片不存在".to_string())?;
            let saved = if password.is_empty() {
                existing.custom_password.clone()
            } else {
                Some(password)
            };
            if saved.is_none() {
                return Err("单独加密必须设置密码".to_string());
            }
            (ReceiveEncryption::Custom, saved)
        }
        _ => return Err("加密方式无效".to_string()),
    };

    let mut receivers = state.receivers.lock().unwrap();
    let receiver = receivers
        .get_mut(&id)
        .ok_or_else(|| "接收卡片不存在".to_string())?;
    receiver.name = name;
    receiver.extensions = extensions;
    receiver.encryption = mode;
    receiver.custom_password = saved_password;
    let updated = receiver.clone();
    drop(receivers);
    persist_receivers(&state);
    Ok(updated)
}

#[tauri::command]
async fn remove_receiver(
    state: State<'_, AppState>,
    tunnels: State<'_, TunnelManager>,
    id: String,
) -> Result<(), String> {
    let _ = tunnels.stop(format!("receive:{id}"));
    state.receivers.lock().unwrap().remove(&id);
    persist_receivers(&state);
    Ok(())
}

#[tauri::command]
async fn clear_receivers(
    state: State<'_, AppState>,
    tunnels: State<'_, TunnelManager>,
) -> Result<(), String> {
    let ids: Vec<String> = state.receivers.lock().unwrap().keys().cloned().collect();
    for id in ids {
        let _ = tunnels.stop(format!("receive:{id}"));
    }
    state.receivers.lock().unwrap().clear();
    persist_receivers(&state);
    Ok(())
}

#[tauri::command]
fn set_receive_common_password(state: State<'_, AppState>, password: String) -> Result<(), String> {
    let mut cfg = read_config(&state);
    let trimmed = password.trim();
    cfg.receive_common_password = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    };
    write_config(&state, &cfg);
    Ok(())
}

#[tauri::command]
fn set_receive_dir(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("路径不能为空".to_string());
    }
    let target = PathBuf::from(path);
    if target.exists() && !target.is_dir() {
        return Err("必须是文件夹".to_string());
    }
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let resolved = std::fs::canonicalize(&target).unwrap_or(target.clone());
    let mut cfg = read_config(&state);
    cfg.receive_dir = Some(resolved.to_string_lossy().into_owned());
    write_config(&state, &cfg);
    Ok(resolved.to_string_lossy().into_owned())
}

#[tauri::command]
async fn start_receiver_tunnel(
    state: State<'_, AppState>,
    tunnels: State<'_, TunnelManager>,
    id: String,
) -> Result<String, String> {
    let receiver = state
        .receivers
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "接收卡片不存在".to_string())?;
    let port = *state.port.lock().unwrap();
    let token = read_config(&state).ngrok_authtoken;
    let root = tunnels.start(
        format!("receive:{id}"),
        format!("http://127.0.0.1:{port}"),
        token,
    )?;
    Ok(format!("{root}/r/{}", receiver.id))
}

#[tauri::command]
async fn stop_receiver_tunnel(tunnels: State<'_, TunnelManager>, id: String) -> Result<(), String> {
    tunnels.stop(format!("receive:{id}"))
}

#[tauri::command]
fn open_receive_dir(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let dir = receive_root_dir(&state).join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    reveal_in_explorer(&dir)
}

#[tauri::command]
async fn set_share_port(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    handle: State<'_, ServerHandle>,
    tunnels: State<'_, TunnelManager>,
    port: u16,
) -> Result<u16, String> {
    if port == 0 {
        return Err("端口无效".to_string());
    }
    let current = handle.0.lock().unwrap().as_ref().map(|s| s.port);
    if current == Some(port) {
        let mut cfg = read_config(&state);
        cfg.share_port = Some(port);
        write_config(&state, &cfg);
        return Ok(port);
    }
    let ids: Vec<String> = state.shares.lock().unwrap().keys().cloned().collect();
    for id in ids {
        let _ = tunnels.stop(format!("share:{id}"));
    }
    let status =
        start_server(app.clone(), port).map_err(|_| format!("端口 {port} 被占用，无法使用"))?;
    let old = handle.0.lock().unwrap().replace(status);
    if let Some(s) = old {
        let _ = stop_server(&s);
    }
    *state.port.lock().unwrap() = port;
    let mut cfg = read_config(&state);
    cfg.share_port = Some(port);
    write_config(&state, &cfg);
    Ok(port)
}

#[tauri::command]
async fn set_site_port(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    handles: State<'_, SiteServerHandle>,
    tunnels: State<'_, TunnelManager>,
    port: u16,
) -> Result<u16, String> {
    if port == 0 {
        return Err("端口无效".to_string());
    }
    let mut sites: Vec<SiteSession> = state.sites.lock().unwrap().values().cloned().collect();
    sites.sort_by_key(|s| (s.created_at, s.id.clone()));
    let ids: Vec<String> = state.sites.lock().unwrap().keys().cloned().collect();
    for id in ids {
        let _ = tunnels.stop(format!("site:{id}"));
    }

    let mut desired_ports = Vec::with_capacity(sites.len());
    for idx in 0..sites.len() {
        let p = port
            .checked_add(idx as u16)
            .ok_or_else(|| "端口超出范围".to_string())?;
        desired_ports.push(p);
    }

    let mut guard = handles.0.lock().unwrap();
    let old: Vec<(String, ServerStatus)> = guard.drain().collect();
    for (_, s) in &old {
        let _ = stop_server(s);
    }

    let mut new_statuses: Vec<(String, ServerStatus)> = Vec::new();
    let mut assignments: Vec<(String, u16)> = Vec::new();

    for (idx, site) in sites.into_iter().enumerate() {
        let desired = desired_ports[idx];
        match start_site_server(app.clone(), site.clone(), desired) {
            Ok(status) => {
                new_statuses.push((site.id.clone(), status));
                assignments.push((site.id.clone(), desired));
            }
            Err(_) => {
                for (_, s) in new_statuses {
                    let _ = stop_server(&s);
                }
                for (id, status) in old {
                    if let Some(site) = state.sites.lock().unwrap().get(&id) {
                        if let Ok(s) = start_site_server(app.clone(), site.clone(), status.port) {
                            guard.insert(id, s);
                        }
                    }
                }
                return Err(format!("端口 {desired} 被占用，无法使用"));
            }
        }
    }

    for (id, s) in new_statuses {
        guard.insert(id, s);
    }
    drop(guard);

    for (id, p) in assignments {
        if let Some(site) = state.sites.lock().unwrap().get_mut(&id) {
            site.port = p;
        }
    }
    let mut cfg = read_config(&state);
    cfg.site_port = Some(port);
    write_config(&state, &cfg);
    persist_sites(&state);
    Ok(port)
}

#[tauri::command]
async fn set_ngrok_authtoken(
    state: State<'_, AppState>,
    tunnels: State<'_, TunnelManager>,
    token: String,
) -> Result<(), String> {
    let mut cfg = read_config(&state);
    let trimmed = token.trim();
    let next = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    };
    if cfg.ngrok_authtoken != next {
        let _ = tunnels.reset();
        cfg.ngrok_authtoken = next;
    }
    write_config(&state, &cfg);
    Ok(())
}

#[tauri::command]
async fn start_share_tunnel(
    state: State<'_, AppState>,
    tunnels: State<'_, TunnelManager>,
    id: String,
) -> Result<String, String> {
    let _share = state
        .shares
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "分享不存在".to_string())?;
    let port = *state.port.lock().unwrap();
    let token = read_config(&state).ngrok_authtoken;
    let root = tunnels.start(
        format!("share:{id}"),
        format!("http://127.0.0.1:{port}"),
        token,
    )?;
    Ok(format!("{root}/s/{id}"))
}

#[tauri::command]
async fn stop_share_tunnel(tunnels: State<'_, TunnelManager>, id: String) -> Result<(), String> {
    tunnels.stop(format!("share:{id}"))
}

#[tauri::command]
async fn start_site_tunnel(
    state: State<'_, AppState>,
    tunnels: State<'_, TunnelManager>,
    id: String,
) -> Result<String, String> {
    let site = state
        .sites
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "站点不存在".to_string())?;
    if site.port == 0 {
        return Err("站点未启动".to_string());
    }
    let token = read_config(&state).ngrok_authtoken;
    tunnels.start(
        format!("site:{id}"),
        format!("http://127.0.0.1:{}", site.port),
        token,
    )
}

#[tauri::command]
async fn stop_site_tunnel(tunnels: State<'_, TunnelManager>, id: String) -> Result<(), String> {
    tunnels.stop(format!("site:{id}"))
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
        basename: pb
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string(),
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
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
        {
            return t[1..t.len() - 1].trim().to_string();
        }
    }
    t.to_string()
}

#[tauri::command]
fn resolve_share_paths(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<ShareSession>, String> {
    let mut out = Vec::new();
    for raw in paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let stripped = strip_quotes(trimmed);
        let path = PathBuf::from(&stripped);
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let kind = if meta.is_dir() {
            ShareKind::Folder
        } else {
            ShareKind::File
        };
        let (size, total_bytes) = share_path_stats(&path).unwrap_or((0, 0));
        let canonical = std::fs::canonicalize(&path).unwrap_or(path.clone());
        let existing = {
            let shares = state.shares.lock().unwrap();
            shares
                .values()
                .find(|s| {
                    let p = PathBuf::from(&s.path);
                    std::fs::canonicalize(&p).ok() == Some(canonical.clone())
                })
                .cloned()
        };
        if let Some(s) = existing {
            out.push(s);
            continue;
        }
        let name = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("share")
            .to_string();
        let session = ShareSession {
            id: random_id(),
            name,
            path: strip_extended_prefix(&canonical.to_string_lossy()),
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
            // Stable app dir holds config.json + shares.json + sites.json; the
            // user can redirect only download history via the Settings panel.
            let default_dir = default_app_dir();
            std::fs::create_dir_all(&default_dir).ok();

            let app_state = AppState::new(default_dir.clone(), default_dir.clone(), DEFAULT_PORT);
            let share_base = configured_share_port(&app_state);

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
            app.manage(SiteServerHandle::default());
            app.manage(TunnelManager::new());

            let handle = app.state::<ServerHandle>();
            let site_handles = app.state::<SiteServerHandle>();
            let state = app.state::<AppState>();

            // Restore active shares so old links keep working after a restart.
            for mut session in load_shares(&state) {
                session.path = strip_extended_prefix(&session.path);
                state
                    .shares
                    .lock()
                    .unwrap()
                    .insert(session.id.clone(), session);
            }
            for mut session in load_sites(&state) {
                session.path = strip_extended_prefix(&session.path);
                state
                    .sites
                    .lock()
                    .unwrap()
                    .insert(session.id.clone(), session);
            }
            let restored_sites: Vec<SiteSession> = {
                let sites = state.sites.lock().unwrap();
                sites.values().cloned().collect()
            };
            for mut session in restored_sites {
                if let Ok(status) = start_site_with_fallback(&app.handle(), &state, &mut session) {
                    site_handles
                        .0
                        .lock()
                        .unwrap()
                        .insert(session.id.clone(), status);
                    state
                        .sites
                        .lock()
                        .unwrap()
                        .insert(session.id.clone(), session);
                } else {
                    eprintln!("[site] failed to restore site server: {}", session.name);
                }
            }
            persist_sites(&state);

            for session in load_receivers(&state) {
                state
                    .receivers
                    .lock()
                    .unwrap()
                    .insert(session.id.clone(), session);
            }

            if let Ok(status) = try_bind(&app.handle(), share_base) {
                let bound = status.port;
                *state.port.lock().unwrap() = bound;
                *handle.0.lock().unwrap() = Some(status);
            }

            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_title("RV NetShare");
            }

            #[cfg(windows)]
            {
                use webview2_com::Microsoft::Web::WebView2::Win32::{
                    ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
                };
                use windows_core::Interface;

                if let Some(main) = app.get_webview_window("main") {
                    let event_window = main.clone();
                    main.on_window_event(move |event| {
                        if let tauri::WindowEvent::Focused(focused) = event {
                            let level = if *focused {
                                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                            } else {
                                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                            };
                            let _ = event_window.with_webview(move |webview| unsafe {
                                if let Ok(core) = webview.controller().CoreWebView2() {
                                    if let Ok(core) = core.cast::<ICoreWebView2_19>() {
                                        let _ = core.SetMemoryUsageTargetLevel(level);
                                    }
                                }
                            });
                        }
                    });
                }
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
            list_sites,
            create_site,
            remove_site,
            clear_sites,
            list_receivers,
            create_receiver,
            update_receiver,
            remove_receiver,
            clear_receivers,
            set_receive_common_password,
            set_receive_dir,
            start_receiver_tunnel,
            stop_receiver_tunnel,
            open_receive_dir,
            set_share_port,
            set_site_port,
            set_ngrok_authtoken,
            start_share_tunnel,
            stop_share_tunnel,
            start_site_tunnel,
            stop_site_tunnel,
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

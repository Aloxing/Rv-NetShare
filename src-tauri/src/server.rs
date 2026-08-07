//! Tiny HTTP/1.1 server backing the share-link app, tuned for max throughput.
//!
//! Routing:
//!   GET  /                     -> HTML index of all active shares
//!   GET  /s/<id>               -> serve a file directly (Content-Disposition)
//!   GET  /s/<id>/<subpath>     -> serve a sub-file or folder listing
//!   Site servers               -> static site rooted at "/"
//!
//! Throughput-oriented choices:
//!   * Single pre-allocated response buffer (header + body in one Vec<u8>)
//!   * 1 MiB read chunks fed straight into the buffer
//!   * 4 MiB SO_SNDBUF, plus platform-specific TCP tuning
//!   * On Linux: libc::sendfile() for true zero-copy file -> socket
//!   * On Windows: writev() to combine header + body in one syscall, plus
//!     a generously-sized TCP send buffer
//!   * Canonicalized share root cached per ShareSession to skip the
//!     per-request filesystem stat

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::html;
use crate::state::{ReceiveEncryption, ReceiveSession, ShareKind, ShareSession, SiteSession};
use crate::storage;

// 4 MiB per read. Combined with a matching SO_SNDBUF this means large file
// writes complete with very few syscalls.
const READ_CHUNK: usize = 4 * 1024 * 1024;
// 4 MiB kernel send buffer. Single biggest knob for LAN throughput.
const SEND_BUFFER: libc::c_int = 4 * 1024 * 1024;
// 256 KiB kernel receive buffer.
const RECV_BUFFER: libc::c_int = 256 * 1024;

pub struct ServerStatus {
    pub port: u16,
    pub stop_flag: Arc<AtomicBool>,
    #[allow(dead_code)]
    pub join: Option<thread::JoinHandle<()>>,
}

pub fn start_server(app: AppHandle, port: u16) -> Result<ServerStatus, String> {
    let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| e.to_string())?;
    let actual_port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();
    let app_clone = app.clone();

    let join = thread::Builder::new()
        .name("lan-share-server".into())
        .spawn(move || {
            for stream in listener.incoming() {
                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }
                match stream {
                    Ok(stream) => {
                        let app = app_clone.clone();
                        thread::Builder::new()
                            .name("lan-share-worker".into())
                            .spawn(move || {
                                let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
                                let _ = stream.set_write_timeout(Some(Duration::from_secs(120)));
                                configure_socket(&stream);
                                let _ = handle_connection(stream, &app);
                            })
                            .ok();
                    }
                    Err(e) => {
                        eprintln!("[share] accept error: {e}");
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;

    Ok(ServerStatus {
        port: actual_port,
        stop_flag,
        join: Some(join),
    })
}

pub fn stop_server(handle: &ServerStatus) -> Result<(), String> {
    handle.stop_flag.store(true, Ordering::Relaxed);
    // Wake the accept loop so the listener can actually exit.
    let _ = TcpStream::connect(("127.0.0.1", handle.port));
    std::thread::sleep(Duration::from_millis(80));
    Ok(())
}

pub fn start_site_server(
    app: AppHandle,
    site: SiteSession,
    port: u16,
) -> Result<ServerStatus, String> {
    let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| e.to_string())?;
    let actual_port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();
    let app_clone = app.clone();

    let join = thread::Builder::new()
        .name(format!("lan-site-server-{}", site.id))
        .spawn(move || {
            for stream in listener.incoming() {
                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }
                match stream {
                    Ok(stream) => {
                        let app = app_clone.clone();
                        let site = site.clone();
                        thread::Builder::new()
                            .name("lan-site-worker".into())
                            .spawn(move || {
                                let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
                                let _ = stream.set_write_timeout(Some(Duration::from_secs(120)));
                                configure_socket(&stream);
                                let _ = handle_site_connection(stream, &app, &site);
                            })
                            .ok();
                    }
                    Err(e) => {
                        eprintln!("[site] accept error: {e}");
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;

    Ok(ServerStatus {
        port: actual_port,
        stop_flag,
        join: Some(join),
    })
}

fn handle_site_connection(
    mut stream: TcpStream,
    app: &AppHandle,
    site: &SiteSession,
) -> io::Result<()> {
    let mut reader = BufReader::with_capacity(16 * 1024, stream.try_clone()?);
    let request_line = match read_request_line(&mut reader) {
        Ok(l) => l,
        Err(_) => return Ok(()),
    };
    let headers = read_headers(&mut reader)?;
    let (method, target) = parse_request_line(&request_line);
    let user_agent = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
        .map(|(_, v)| v.clone());
    let peer_ip = stream
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let range = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("range"))
        .map(|(_, v)| v.clone());

    let response = route_site_root(
        &method,
        &target,
        site,
        &peer_ip,
        user_agent,
        range.as_deref(),
    );

    match response.body {
        ResponseBody::Full(bytes) => {
            enable_nodelay(&stream);
            let _ = stream.write_all(&bytes);
        }
        ResponseBody::File {
            header,
            mut file,
            start,
            length,
        } => {
            write_file_response(&stream, &header, &mut file, start, length)?;
        }
    }
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Both);

    if let Some(ev) = response.event {
        storage::emit_access_event(app, &ev);
        let state = app.state::<crate::state::AppState>();
        if let Ok(dir) = storage::ensure_data_dir(&state) {
            let _ = storage::append_history(&dir, &ev);
        }
    }
    Ok(())
}

fn route_site_root(
    method: &str,
    target: &str,
    site: &SiteSession,
    peer: &str,
    user_agent: Option<String>,
    range: Option<&str>,
) -> Outgoing {
    let target = target.split('?').next().unwrap_or(target);
    let path = match percent_decode(target) {
        Ok(p) => p,
        Err(_) => return error_full(400, "Bad Request", "无效的 URL 地址"),
    };
    if method != "GET" && method != "HEAD" {
        return error_full(405, "Method Not Allowed", "仅支持 GET / HEAD 请求");
    }
    let subpath = path.to_string_lossy().trim_start_matches('/').to_string();
    serve_site(site, &subpath, peer, user_agent, range)
}

/// Configure the socket for maximum LAN throughput. All platform-specific
/// socket options go through `libc` directly to avoid the std API overhead.
#[cfg(unix)]
fn configure_socket(stream: &TcpStream) {
    let fd = stream.as_raw_fd();
    unsafe {
        let optlen = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &SEND_BUFFER as *const _ as *const libc::c_char,
            optlen,
        );
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &RECV_BUFFER as *const _ as *const libc::c_char,
            optlen,
        );
        // Disable delayed ACK so small replies aren't held up.
        let quickack: libc::c_int = 1;
        libc::setsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_QUICKACK,
            &quickack as *const _ as *const libc::c_char,
            optlen,
        );
    }
    // Leave Nagle on (TCP_NODELAY off) so the kernel can coalesce our
    // large writes into bigger segments. We flip it back on per-call for
    // tiny HTML responses.
}

#[cfg(windows)]
fn configure_socket(stream: &TcpStream) {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{setsockopt, SOL_SOCKET, SO_RCVBUF, SO_SNDBUF};

    let socket = stream.as_raw_socket() as usize;
    let send: i32 = SEND_BUFFER;
    let recv: i32 = RECV_BUFFER;
    unsafe {
        let optlen = std::mem::size_of::<i32>() as i32;
        setsockopt(
            socket,
            SOL_SOCKET,
            SO_SNDBUF,
            &send as *const _ as *const u8,
            optlen,
        );
        setsockopt(
            socket,
            SOL_SOCKET,
            SO_RCVBUF,
            &recv as *const _ as *const u8,
            optlen,
        );
    }
}

#[cfg(all(not(unix), not(windows)))]
fn configure_socket(_stream: &TcpStream) {
    // Best-effort no-op for non-unix platforms.
}

fn handle_connection(mut stream: TcpStream, app: &AppHandle) -> io::Result<()> {
    // 16 KiB request read buffer is plenty for typical LAN HTTP headers.
    let mut reader = BufReader::with_capacity(16 * 1024, stream.try_clone()?);
    let request_line = match read_request_line(&mut reader) {
        Ok(l) => l,
        Err(_) => return Ok(()),
    };
    let headers = read_headers(&mut reader)?;
    let (method, target) = parse_request_line(&request_line);
    let user_agent = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
        .map(|(_, v)| v.clone());
    let peer_ip = stream
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let range = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("range"))
        .map(|(_, v)| v.clone());

    let state = app.state::<crate::state::AppState>();
    let shares: Vec<ShareSession> = state.shares.lock().unwrap().values().cloned().collect();
    let sites: Vec<SiteSession> = state.sites.lock().unwrap().values().cloned().collect();
    let receivers: Vec<ReceiveSession> =
        state.receivers.lock().unwrap().values().cloned().collect();

    let response = if method == "POST" {
        handle_receive_upload(&mut reader, &target, &headers, &receivers, &peer_ip, app)
    } else {
        route(
            &method,
            &target,
            &shares,
            &sites,
            &receivers,
            &peer_ip,
            user_agent,
            range.as_deref(),
            app,
        )
    };

    match response.body {
        ResponseBody::Full(bytes) => {
            enable_nodelay(&stream);
            let _ = stream.write_all(&bytes);
        }
        ResponseBody::File {
            header,
            mut file,
            start,
            length,
        } => {
            write_file_response(&stream, &header, &mut file, start, length)?;
        }
    }
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Both);

    if let Some(ev) = response.event {
        storage::emit_access_event(app, &ev);
        if let Ok(dir) = storage::ensure_data_dir(&state) {
            let _ = storage::append_history(&dir, &ev);
        }
    }
    Ok(())
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn json_full(code: u16, ok: bool, message: &str) -> Outgoing {
    let payload = serde_json::json!({ "ok": ok, "message": message }).to_string();
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        411 => "Length Required",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {len}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        code = code,
        reason = reason,
        len = payload.len(),
    );
    let mut bytes = header.into_bytes();
    bytes.extend_from_slice(payload.as_bytes());
    Outgoing {
        body: ResponseBody::Full(bytes),
        event: None,
    }
}

fn receiver_password_matches(
    receiver: &ReceiveSession,
    provided: &str,
    state: &crate::state::AppState,
) -> bool {
    match receiver.encryption {
        ReceiveEncryption::None => true,
        ReceiveEncryption::Common => {
            crate::receive_common_password(state).as_deref() == Some(provided)
        }
        ReceiveEncryption::Custom => receiver.custom_password.as_deref() == Some(provided),
    }
}

fn receiver_allows_extension(receiver: &ReceiveSession, filename: &str) -> bool {
    if receiver.extensions.iter().any(|ext| ext == "*") {
        return true;
    }
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    receiver.extensions.iter().any(|allowed| *allowed == ext)
}

fn unique_receive_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(filename);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let mut index = 1;
    loop {
        let name = if ext.is_empty() {
            format!("{stem} ({index})")
        } else {
            format!("{stem} ({index}).{ext}")
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
        index += 1;
    }
}

fn handle_receive_upload<R: BufRead>(
    reader: &mut R,
    target: &str,
    headers: &[(String, String)],
    receivers: &[ReceiveSession],
    peer: &str,
    app: &AppHandle,
) -> Outgoing {
    let target = target.split('?').next().unwrap_or(target);
    let trimmed = target.trim_start_matches('/');
    let mut segments = trimmed.splitn(3, '/');
    if segments.next() != Some("r") {
        return json_full(404, false, "上传地址无效");
    }
    let id = match segments.next() {
        Some(id) if !id.is_empty() => id,
        _ => return json_full(400, false, "缺少接收卡片 ID"),
    };
    if segments.next().unwrap_or("") != "upload" {
        return json_full(404, false, "上传地址无效");
    }

    let receiver = match receivers.iter().find(|r| r.id == id) {
        Some(r) => r.clone(),
        None => return json_full(404, false, "接收卡片不存在或已删除"),
    };

    let state = app.state::<crate::state::AppState>();
    let provided_password = header_value(headers, "x-upload-password").unwrap_or("");
    if !receiver_password_matches(&receiver, provided_password, &state) {
        return json_full(403, false, "密码错误");
    }
    let user_agent = header_value(headers, "user-agent").map(|s| s.to_string());

    let raw_name = header_value(headers, "x-file-name").unwrap_or("upload.bin");
    let decoded = percent_decode(raw_name).unwrap_or_else(|_| PathBuf::from("upload.bin"));
    let filename = decoded
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("upload.bin")
        .to_string();
    if !receiver_allows_extension(&receiver, &filename) {
        return json_full(415, false, &format!("不允许上传此文件类型: {filename}"));
    }

    let content_length: u64 =
        match header_value(headers, "content-length").and_then(|value| value.trim().parse().ok()) {
            Some(n) => n,
            None => return json_full(411, false, "缺少 Content-Length"),
        };
    if content_length == 0 {
        return json_full(400, false, "文件内容为空");
    }

    let dir = crate::receive_root_dir(&state).join(&receiver.id);
    if let Err(e) = fs::create_dir_all(&dir) {
        return json_full(500, false, &format!("接收目录创建失败: {e}"));
    }
    let path = unique_receive_path(&dir, &filename);
    let mut file = match File::create(&path) {
        Ok(f) => f,
        Err(e) => return json_full(500, false, &format!("文件创建失败: {e}")),
    };
    let mut limited = (&mut *reader).take(content_length);
    if let Err(e) = io::copy(&mut limited, &mut file) {
        return json_full(500, false, &format!("写入文件失败: {e}"));
    }
    drop(file);

    let record = storage::AccessRecord {
        id: format!(
            "recv-{}-{}-{}",
            storage::now_secs(),
            content_length,
            receiver.id
        ),
        share_id: receiver.id.clone(),
        share_name: filename.clone(),
        path: path.to_string_lossy().into_owned(),
        bytes: content_length,
        timestamp: storage::now_secs(),
        peer: peer.to_string(),
        user_agent,
        status: "success".to_string(),
        kind: "receive".to_string(),
    };
    if let Ok(dir) = storage::ensure_data_dir(&state) {
        let _ = storage::append_history(&dir, &record);
        storage::emit_access_event(app, &record);
    }

    let updated = {
        let mut receivers = state.receivers.lock().unwrap();
        if let Some(receiver) = receivers.get_mut(&receiver.id) {
            receiver.received_count += 1;
            receiver.received_bytes += content_length;
        }
        let dir = state.base_dir.lock().unwrap().clone();
        let snapshot: Vec<ReceiveSession> = receivers.values().cloned().collect();
        let _ = storage::write_receivers(&dir, &snapshot);
        receivers.get(&receiver.id).cloned()
    };
    if let Some(updated) = updated {
        let _ = app.emit(
            "receiver-updated",
            &serde_json::json!({ "receiver": updated, "filename": filename }),
        );
    }

    json_full(200, true, &format!("{filename} 已保存"))
}

#[cfg(unix)]
fn enable_nodelay(stream: &TcpStream) {
    let fd = stream.as_raw_fd();
    let on: libc::c_int = 1;
    unsafe {
        let optlen = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        libc::setsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_NODELAY,
            &on as *const _ as *const libc::c_char,
            optlen,
        );
    }
}

#[cfg(windows)]
fn enable_nodelay(stream: &TcpStream) {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{setsockopt, IPPROTO_TCP, TCP_NODELAY};

    let socket = stream.as_raw_socket() as usize;
    let on: i32 = 1;
    unsafe {
        let optlen = std::mem::size_of::<i32>() as i32;
        setsockopt(
            socket,
            IPPROTO_TCP,
            TCP_NODELAY,
            &on as *const _ as *const u8,
            optlen,
        );
    }
}

#[cfg(all(not(unix), not(windows)))]
fn enable_nodelay(_stream: &TcpStream) {}

enum ResponseBody {
    Full(Vec<u8>),
    File {
        header: Vec<u8>,
        file: File,
        start: u64,
        length: u64,
    },
}

enum RangeRequest {
    None,
    Bytes(u64, Option<u64>),
    Unsatisfiable,
}

fn parse_range_header(header: Option<&str>, total: u64) -> RangeRequest {
    let Some(h) = header else {
        return RangeRequest::None;
    };
    let h = h.trim();
    if !h.to_ascii_lowercase().starts_with("bytes=") {
        return RangeRequest::None;
    }
    let spec = h["bytes=".len()..].split(',').next().unwrap_or("").trim();
    let Some((start_s, end_s)) = spec.split_once('-') else {
        return RangeRequest::None;
    };
    if start_s.trim().is_empty() {
        // Suffix range: last N bytes.
        let suffix: u64 = match end_s.trim().parse() {
            Ok(n) => n,
            Err(_) => return RangeRequest::None,
        };
        if suffix == 0 || total == 0 {
            return RangeRequest::Unsatisfiable;
        }
        let start = total.saturating_sub(suffix);
        return RangeRequest::Bytes(start, Some(total - 1));
    }
    let start: u64 = match start_s.trim().parse() {
        Ok(n) => n,
        Err(_) => return RangeRequest::None,
    };
    if total == 0 || start >= total {
        return RangeRequest::Unsatisfiable;
    }
    if end_s.trim().is_empty() {
        return RangeRequest::Bytes(start, None);
    }
    let end: u64 = match end_s.trim().parse() {
        Ok(n) => n,
        Err(_) => return RangeRequest::None,
    };
    if end < start {
        return RangeRequest::None;
    }
    RangeRequest::Bytes(start, Some(end.min(total - 1)))
}

struct Outgoing {
    body: ResponseBody,
    event: Option<storage::AccessRecord>,
}

fn route(
    method: &str,
    target: &str,
    shares: &[ShareSession],
    sites: &[SiteSession],
    receivers: &[ReceiveSession],
    peer: &str,
    user_agent: Option<String>,
    range: Option<&str>,
    app: &AppHandle,
) -> Outgoing {
    let target = target.split('?').next().unwrap_or(target);
    let path = match percent_decode(target) {
        Ok(p) => p,
        Err(_) => return error_full(400, "Bad Request", "无效的 URL 地址"),
    };

    if method != "GET" && method != "HEAD" {
        return error_full(405, "Method Not Allowed", "仅支持 GET / HEAD 请求");
    }

    if path.as_os_str() == "/" || path.as_os_str().is_empty() {
        let body = html::render_index(shares, receivers);
        return html_full(200, &body);
    }

    let trimmed = path.to_string_lossy();
    let trimmed = trimmed.trim_start_matches('/');
    let mut rest_segs = trimmed.splitn(2, '/');
    let first = rest_segs.next().unwrap_or("");
    let rest = rest_segs.next().unwrap_or("");
    let mut parts = rest.splitn(2, '/');
    let id = parts.next().unwrap_or("");
    let subpath = parts.next().unwrap_or("");
    match first {
        "s" => {
            if id.is_empty() {
                return error_full(400, "Bad Request", "缺少分享 ID");
            }
            let share = match shares.iter().find(|s| s.id == id) {
                Some(s) => s.clone(),
                None => return error_full(404, "Not Found", "分享不存在或已停止"),
            };
            serve_share(&share, subpath, peer, user_agent, range, app)
        }
        "w" => {
            if id.is_empty() {
                return error_full(400, "Bad Request", "缺少站点 ID");
            }
            let site = match sites.iter().find(|s| s.id == id) {
                Some(s) => s.clone(),
                None => return error_full(404, "Not Found", "站点不存在或已移除"),
            };
            serve_site(&site, subpath, peer, user_agent, range)
        }
        "r" => {
            if id.is_empty() {
                return error_full(400, "Bad Request", "缺少接收卡片 ID");
            }
            let receiver = match receivers.iter().find(|r| r.id == id) {
                Some(r) => r.clone(),
                None => return error_full(404, "Not Found", "接收卡片不存在或已删除"),
            };
            if !subpath.is_empty() {
                return error_full(404, "Not Found", "页面不存在");
            }
            let password_required = receiver.encryption != ReceiveEncryption::None;
            let body = html::render_receive_page(&receiver, password_required);
            html_full(200, &body)
        }
        _ => error_full(404, "Not Found", "页面不存在"),
    }
}

fn serve_share(
    share: &ShareSession,
    subpath: &str,
    peer: &str,
    user_agent: Option<String>,
    range: Option<&str>,
    _app: &AppHandle,
) -> Outgoing {
    let root = match cached_canonical_root(&share.path) {
        Some(p) => p,
        None => return error_full(500, "Internal Server Error", "无法解析分享路径"),
    };
    match share.kind {
        ShareKind::File => {
            if !subpath.is_empty() {
                return error_full(404, "Not Found", "文件分享不支持子路径");
            }
            serve_file_stream(
                &root,
                peer,
                &share.id,
                &share.name,
                user_agent,
                range,
                "share",
            )
        }
        ShareKind::Folder => {
            let target = if subpath.is_empty() {
                root.clone()
            } else {
                match safe_join(&root, subpath) {
                    Some(p) => p,
                    None => return error_full(400, "Bad Request", "路径无效"),
                }
            };
            let meta = match fs::metadata(&target) {
                Ok(m) => m,
                Err(_) => return error_full(404, "Not Found", "文件不存在"),
            };
            if meta.is_dir() {
                let entries = match storage::list_dir(&target) {
                    Ok(e) => e,
                    Err(_) => return error_full(500, "Internal Server Error", "目录读取失败"),
                };
                let body = html::render_folder_listing(share, subpath, &entries);
                html_full(200, &body)
            } else if meta.is_file() {
                let display = target
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("file");
                serve_file_stream(
                    &target, peer, &share.id, display, user_agent, range, "share",
                )
            } else {
                error_full(404, "Not Found", "不支持的资源类型")
            }
        }
    }
}

fn find_index_file(dir: &Path) -> Option<PathBuf> {
    for name in ["index.html", "index.htm"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn serve_site(
    site: &SiteSession,
    subpath: &str,
    peer: &str,
    user_agent: Option<String>,
    range: Option<&str>,
) -> Outgoing {
    let root = match cached_canonical_root(&site.path) {
        Some(p) => p,
        None => return error_full(500, "Internal Server Error", "无法解析分享路径"),
    };
    let target = if subpath.is_empty() {
        root.clone()
    } else {
        match safe_join(&root, subpath) {
            Some(p) => p,
            None => return error_full(400, "Bad Request", "路径无效"),
        }
    };
    let meta = match fs::metadata(&target) {
        Ok(m) => m,
        Err(_) => return error_full(404, "Not Found", "文件不存在"),
    };
    if meta.is_dir() {
        if let Some(index) = find_index_file(&target) {
            return serve_static_file_stream(
                &index,
                peer,
                &site.id,
                "index.html",
                user_agent,
                range,
                true,
                "site",
            );
        }
        return error_full(404, "Not Found", "未找到 index.html");
    }
    if meta.is_file() {
        let display = target
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        return serve_static_file_stream(
            &target, peer, &site.id, display, user_agent, range, false, "site",
        );
    }
    error_full(404, "Not Found", "不支持的资源类型")
}

/// Memoised canonicalize for share roots so we don't re-stat the disk on every
/// request. The cache lives for the lifetime of the running app.
fn cached_canonical_root(raw: &str) -> Option<PathBuf> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    if let Some(p) = guard.get(raw) {
        return Some(p.clone());
    }
    let canon = fs::canonicalize(raw).ok()?;
    guard.insert(raw.to_string(), canon.clone());
    Some(canon)
}

fn serve_file_stream(
    path: &Path,
    peer: &str,
    share_id: &str,
    display: &str,
    user_agent: Option<String>,
    range: Option<&str>,
    kind: &str,
) -> Outgoing {
    serve_file_response(
        path, peer, share_id, display, user_agent, range, true, true, kind,
    )
}

fn serve_static_file_stream(
    path: &Path,
    peer: &str,
    share_id: &str,
    display: &str,
    user_agent: Option<String>,
    range: Option<&str>,
    record: bool,
    kind: &str,
) -> Outgoing {
    serve_file_response(
        path, peer, share_id, display, user_agent, range, false, record, kind,
    )
}

fn serve_file_response(
    path: &Path,
    peer: &str,
    share_id: &str,
    display: &str,
    user_agent: Option<String>,
    range: Option<&str>,
    attachment: bool,
    record: bool,
    kind: &str,
) -> Outgoing {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return error_full(404, "Not Found", "文件不存在"),
    };
    let len = meta.len();
    let mime = guess_mime(path);
    let (header, start, length) = match parse_range_header(range, len) {
        RangeRequest::None => (build_file_header(mime, len, display, attachment), 0, len),
        RangeRequest::Unsatisfiable => {
            let body = html::render_error(416, "Range Not Satisfiable", "请求的范围无效");
            let header = format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n",
                len = len,
                body_len = body.len(),
            );
            let mut bytes = header.into_bytes();
            bytes.extend_from_slice(body.as_bytes());
            return Outgoing {
                body: ResponseBody::Full(bytes),
                event: None,
            };
        }
        RangeRequest::Bytes(range_start, range_end) => {
            let end = range_end.unwrap_or(len - 1);
            let range_length = end - range_start + 1;
            (
                build_partial_header(
                    mime,
                    range_length,
                    display,
                    range_start,
                    end,
                    len,
                    attachment,
                ),
                range_start,
                range_length,
            )
        }
    };
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return error_full(500, "Internal Server Error", "文件打开失败"),
    };
    let event = if record {
        Some(storage::AccessRecord {
            id: format!("{:x}-{:x}-{:x}", storage::now_secs(), start, length),
            share_id: share_id.to_string(),
            share_name: display.to_string(),
            path: path.to_string_lossy().into_owned(),
            bytes: length,
            timestamp: storage::now_secs(),
            peer: peer.to_string(),
            user_agent,
            status: "success".to_string(),
            kind: kind.to_string(),
        })
    } else {
        None
    };
    Outgoing {
        body: ResponseBody::File {
            header,
            file,
            start,
            length,
        },
        event,
    }
}

fn build_file_header(mime: &str, len: u64, display: &str, attachment: bool) -> Vec<u8> {
    // Pre-size: ~256 bytes covers the whole HTTP header for any sane filename.
    let mut out = String::with_capacity(256);
    out.push_str("HTTP/1.1 200 OK\r\n");
    out.push_str("Content-Type: ");
    out.push_str(mime);
    out.push_str("\r\n");
    out.push_str("Content-Length: ");
    out.push_str(&len.to_string());
    out.push_str("\r\n");
    if attachment {
        out.push_str("Content-Disposition: attachment; filename*=UTF-8\'\'");
        out.push_str(&url_encode_filename(display));
        out.push_str("\r\n");
    }
    out.push_str("Accept-Ranges: bytes\r\n");
    // Connection: close keeps the implementation simple (no keep-alive state
    // machine). With 4 MiB SO_SNDBUF this still saturates gigabit.
    out.push_str("Connection: close\r\n");
    out.push_str("\r\n");
    out.into_bytes()
}

fn build_partial_header(
    mime: &str,
    len: u64,
    display: &str,
    start: u64,
    end: u64,
    total: u64,
    attachment: bool,
) -> Vec<u8> {
    let mut out = String::with_capacity(256);
    out.push_str("HTTP/1.1 206 Partial Content\r\n");
    out.push_str("Content-Type: ");
    out.push_str(mime);
    out.push_str("\r\n");
    out.push_str("Content-Length: ");
    out.push_str(&len.to_string());
    out.push_str("\r\n");
    out.push_str("Content-Range: bytes ");
    out.push_str(&start.to_string());
    out.push_str("-");
    out.push_str(&end.to_string());
    out.push_str("/");
    out.push_str(&total.to_string());
    out.push_str("\r\n");
    if attachment {
        out.push_str("Content-Disposition: attachment; filename*=UTF-8\'\'");
        out.push_str(&url_encode_filename(display));
        out.push_str("\r\n");
    }
    out.push_str("Accept-Ranges: bytes\r\n");
    out.push_str("Connection: close\r\n");
    out.push_str("\r\n");
    out.into_bytes()
}

/// Stream the file straight to the socket. Strategy:
///   1. Linux: write the header, then sendfile() the entire file in one
///      zero-copy syscall.
///   2. Other platforms: stream in large chunks (kernel coalesces).
fn write_file_response(
    mut stream: &TcpStream,
    header: &[u8],
    file: &mut File,
    start: u64,
    length: u64,
) -> io::Result<()> {
    // Always start by writing the header.
    stream.write_all(header)?;

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        if length == 0 {
            return Ok(());
        }
        let mut offset = start as libc::off_t;
        let mut remaining = length;
        loop {
            let sent = unsafe {
                libc::sendfile(
                    stream.as_raw_fd(),
                    file.as_raw_fd(),
                    &mut offset,
                    remaining.min(isize::MAX as u64) as usize,
                )
            };
            if sent < 0 {
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                if err.raw_os_error() == Some(libc::ENOSYS)
                    || err.raw_os_error() == Some(libc::EINVAL)
                {
                    // Fallback for filesystems that don't support sendfile.
                    return write_range_chunks(stream, file, start, length);
                }
                return Err(err);
            }
            if sent == 0 {
                break;
            }
            remaining -= sent as u64;
            if remaining == 0 {
                break;
            }
        }
        return Ok(());
    }

    #[cfg(not(target_os = "linux"))]
    {
        write_range_chunks(stream, file, start, length)
    }
}

fn write_range_chunks(
    mut stream: &TcpStream,
    file: &mut File,
    start: u64,
    length: u64,
) -> io::Result<()> {
    use std::io::{Seek, SeekFrom};

    file.seek(SeekFrom::Start(start))?;
    let mut buf = vec![0u8; READ_CHUNK];
    let mut remaining = length;
    while remaining > 0 {
        let want = buf.len().min(remaining as usize);
        let n = file.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        stream.write_all(&buf[..n])?;
        remaining -= n as u64;
    }
    Ok(())
}

fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    for seg in Path::new(rel).components() {
        match seg {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    let canon_out = fs::canonicalize(&out).ok()?;
    if canon_out.starts_with(root) {
        Some(canon_out)
    } else {
        None
    }
}

fn percent_decode(s: &str) -> io::Result<PathBuf> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_val(bytes[i + 1])?;
            let lo = hex_val(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&out).into_owned()))
}

fn hex_val(c: u8) -> io::Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad percent encoding",
        )),
    }
}

fn url_encode_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || *b == b'.' || *b == b'_' || *b == b'-' {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn guess_mime(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" | "webmanifest" => "application/json; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "txt" | "md" | "log" | "csv" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn html_full(code: u16, body: &str) -> Outgoing {
    let header = format!(
        "HTTP/1.1 {code} OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {len}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        code = code,
        len = body.len(),
    );
    let mut bytes = header.into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    Outgoing {
        body: ResponseBody::Full(bytes),
        event: None,
    }
}

fn error_full(code: u16, reason: &str, msg: &str) -> Outgoing {
    let body = html::render_error(code, reason, msg);
    let header = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
        code = code, reason = reason, len = body.len(),
    );
    let mut bytes = header.into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    Outgoing {
        body: ResponseBody::Full(bytes),
        event: None,
    }
}

fn read_request_line<R: BufRead>(reader: &mut R) -> io::Result<String> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line.trim_end().to_string())
}

fn read_headers<R: BufRead>(reader: &mut R) -> io::Result<Vec<(String, String)>> {
    let mut headers = Vec::with_capacity(16);
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok(headers)
}

fn parse_request_line(line: &str) -> (String, String) {
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    (method, target)
}

/// Bench-friendly entry point: starts the server with a fixed share list,
/// skipping the Tauri AppHandle. Used by examples/bench.rs to measure
/// throughput without spinning up a Tauri webview.
pub fn start_server_with_shares<I>(port: u16, shares: I) -> io::Result<u16>
where
    I: IntoIterator<Item = ShareSession>,
{
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    let actual_port = listener.local_addr()?.port();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();
    let shares_vec: Vec<ShareSession> = shares.into_iter().collect();

    let _join = thread::Builder::new()
        .name("lan-bench-server".into())
        .spawn(move || {
            for stream in listener.incoming() {
                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }
                match stream {
                    Ok(mut stream) => {
                        let shares = shares_vec.clone();
                        let stop = stop_flag_clone.clone();
                        thread::spawn(move || {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(120)));
                            configure_socket(&stream);
                            let peer = stream
                                .peer_addr()
                                .map(|a| a.ip().to_string())
                                .unwrap_or_default();
                            let response = match parse_and_route(&stream, &shares, &[], &peer) {
                                Ok(r) => r,
                                Err(_) => return,
                            };
                            let _ = stop;
                            match response.body {
                                ResponseBody::Full(bytes) => {
                                    enable_nodelay(&stream);
                                    let _ = stream.write_all(&bytes);
                                }
                                ResponseBody::File {
                                    header,
                                    mut file,
                                    start,
                                    length,
                                } => {
                                    let _ = write_file_response(
                                        &stream, &header, &mut file, start, length,
                                    );
                                }
                            }
                            let _ = stream.flush();
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                        });
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        })?;

    Ok(actual_port)
}

fn parse_and_route(
    stream: &TcpStream,
    shares: &[ShareSession],
    sites: &[SiteSession],
    peer: &str,
) -> io::Result<Outgoing> {
    let mut reader = BufReader::with_capacity(16 * 1024, stream.try_clone()?);
    let request_line = read_request_line(&mut reader)?;
    let headers = read_headers(&mut reader)?;
    let (method, target) = parse_request_line(&request_line);
    let user_agent = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
        .map(|(_, v)| v.clone());
    let range = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("range"))
        .map(|(_, v)| v.clone());
    Ok(route_no_app(
        &method,
        &target,
        shares,
        sites,
        &[],
        peer,
        user_agent,
        range.as_deref(),
    ))
}

fn route_no_app(
    method: &str,
    target: &str,
    shares: &[ShareSession],
    sites: &[SiteSession],
    receivers: &[ReceiveSession],
    peer: &str,
    user_agent: Option<String>,
    range: Option<&str>,
) -> Outgoing {
    let target = target.split('?').next().unwrap_or(target);
    let path = match percent_decode(target) {
        Ok(p) => p,
        Err(_) => return error_full(400, "Bad Request", "无效的 URL 地址"),
    };

    if method != "GET" && method != "HEAD" {
        return error_full(405, "Method Not Allowed", "仅支持 GET / HEAD 请求");
    }

    if path.as_os_str() == "/" || path.as_os_str().is_empty() {
        let body = html::render_index(shares, receivers);
        return html_full(200, &body);
    }

    let trimmed = path.to_string_lossy();
    let trimmed = trimmed.trim_start_matches('/');
    let mut rest_segs = trimmed.splitn(2, '/');
    let first = rest_segs.next().unwrap_or("");
    let rest = rest_segs.next().unwrap_or("");
    let mut parts = rest.splitn(2, '/');
    let id = parts.next().unwrap_or("");
    let subpath = parts.next().unwrap_or("");
    match first {
        "s" => {
            if id.is_empty() {
                return error_full(400, "Bad Request", "缺少分享 ID");
            }
            let share = match shares.iter().find(|s| s.id == id) {
                Some(s) => s.clone(),
                None => return error_full(404, "Not Found", "分享不存在或已停止"),
            };
            serve_share_no_app(&share, subpath, peer, user_agent, range)
        }
        "w" => {
            if id.is_empty() {
                return error_full(400, "Bad Request", "缺少站点 ID");
            }
            let site = match sites.iter().find(|s| s.id == id) {
                Some(s) => s.clone(),
                None => return error_full(404, "Not Found", "站点不存在或已移除"),
            };
            serve_site(&site, subpath, peer, user_agent, range)
        }
        "r" => {
            if id.is_empty() {
                return error_full(400, "Bad Request", "缺少接收卡片 ID");
            }
            let receiver = match receivers.iter().find(|r| r.id == id) {
                Some(r) => r.clone(),
                None => return error_full(404, "Not Found", "接收卡片不存在或已删除"),
            };
            if !subpath.is_empty() {
                return error_full(404, "Not Found", "页面不存在");
            }
            let password_required = receiver.encryption != ReceiveEncryption::None;
            let body = html::render_receive_page(&receiver, password_required);
            html_full(200, &body)
        }
        _ => error_full(404, "Not Found", "页面不存在"),
    }
}

fn serve_share_no_app(
    share: &ShareSession,
    subpath: &str,
    peer: &str,
    user_agent: Option<String>,
    range: Option<&str>,
) -> Outgoing {
    let root = match cached_canonical_root(&share.path) {
        Some(p) => p,
        None => return error_full(500, "Internal Server Error", "无法解析分享路径"),
    };
    match share.kind {
        ShareKind::File => {
            if !subpath.is_empty() {
                return error_full(404, "Not Found", "文件分享不支持子路径");
            }
            serve_file_stream(
                &root,
                peer,
                &share.id,
                &share.name,
                user_agent,
                range,
                "share",
            )
        }
        ShareKind::Folder => {
            let target = if subpath.is_empty() {
                root.clone()
            } else {
                match safe_join(&root, subpath) {
                    Some(p) => p,
                    None => return error_full(400, "Bad Request", "路径无效"),
                }
            };
            let meta = match fs::metadata(&target) {
                Ok(m) => m,
                Err(_) => return error_full(404, "Not Found", "文件不存在"),
            };
            if meta.is_dir() {
                let entries = match storage::list_dir(&target) {
                    Ok(e) => e,
                    Err(_) => return error_full(500, "Internal Server Error", "目录读取失败"),
                };
                let body = html::render_folder_listing(share, subpath, &entries);
                html_full(200, &body)
            } else if meta.is_file() {
                let display = target
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("file");
                serve_file_stream(
                    &target, peer, &share.id, display, user_agent, range, "share",
                )
            } else {
                error_full(404, "Not Found", "不支持的资源类型")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rv-netshare-test-{tag}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn folder_session(id: &str, path: &Path) -> ShareSession {
        ShareSession {
            id: id.to_string(),
            name: path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("site")
                .to_string(),
            path: path.to_string_lossy().into_owned(),
            kind: ShareKind::Folder,
            size: 1,
            total_bytes: 1,
            created_at: 1,
        }
    }

    fn site_session(id: &str, path: &Path, port: u16) -> SiteSession {
        SiteSession {
            id: id.to_string(),
            name: path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("site")
                .to_string(),
            path: path.to_string_lossy().into_owned(),
            size: 1,
            total_bytes: 1,
            created_at: 1,
            port,
        }
    }

    fn request_share(target: &str, share: &ShareSession) -> Outgoing {
        route_no_app(
            "GET",
            target,
            std::slice::from_ref(share),
            &[],
            &[],
            "127.0.0.1",
            None,
            None,
        )
    }

    fn request_site(target: &str, site: &SiteSession) -> Outgoing {
        route_no_app(
            "GET",
            target,
            &[],
            std::slice::from_ref(site),
            &[],
            "127.0.0.1",
            None,
            None,
        )
    }

    fn receive_session(
        id: &str,
        extensions: &[&str],
        encryption: ReceiveEncryption,
    ) -> ReceiveSession {
        ReceiveSession {
            id: id.to_string(),
            name: "测试接收".to_string(),
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            encryption,
            custom_password: None,
            created_at: 1,
            received_count: 0,
            received_bytes: 0,
        }
    }

    fn request_receive(target: &str, receiver: &ReceiveSession) -> Outgoing {
        route_no_app(
            "GET",
            target,
            &[],
            &[],
            std::slice::from_ref(receiver),
            "127.0.0.1",
            None,
            None,
        )
    }

    fn header_of(out: &Outgoing) -> String {
        match &out.body {
            ResponseBody::Full(bytes) => {
                let text = String::from_utf8_lossy(bytes);
                text.split("\r\n\r\n").next().unwrap_or("").to_string()
            }
            ResponseBody::File { header, .. } => String::from_utf8_lossy(header).into_owned(),
        }
    }

    fn file_len(out: &Outgoing) -> u64 {
        match &out.body {
            ResponseBody::File { length, .. } => *length,
            _ => 0,
        }
    }

    fn read_file_body(out: &mut Outgoing) -> String {
        match &mut out.body {
            ResponseBody::File { file, length, .. } => {
                let mut buf = vec![0u8; *length as usize];
                file.read_exact(&mut buf).unwrap();
                String::from_utf8(buf).unwrap()
            }
            _ => panic!("expected file response"),
        }
    }

    #[test]
    fn share_folder_with_index_still_shows_listing_and_attachment() {
        let root = temp_test_dir("share-index");
        fs::write(root.join("index.html"), "<h1>hello</h1>").unwrap();
        let share = folder_session("share", &root);

        let listing = request_share(&format!("/s/{}/", share.id), &share);
        match listing.body {
            ResponseBody::Full(bytes) => {
                let body = String::from_utf8(bytes).unwrap();
                assert!(body.contains("index.html"));
            }
            _ => panic!("expected folder listing"),
        }

        let index = request_share(&format!("/s/{}/index.html", share.id), &share);
        let header = header_of(&index);
        assert!(header.contains("Content-Disposition: attachment"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn site_serves_index_and_assets_inline() {
        let root = temp_test_dir("site-root");
        fs::write(root.join("index.html"), "<h1>site</h1>").unwrap();
        fs::write(root.join("style.css"), "body{color:red}").unwrap();
        let site = site_session("site", &root, 48800);

        let mut index = request_site(&format!("/w/{}/", site.id), &site);
        let index_header = header_of(&index);
        assert!(index_header.contains("200 OK"));
        assert!(index_header.contains("Content-Type: text/html; charset=utf-8"));
        assert!(!index_header.contains("Content-Disposition"));
        assert_eq!(file_len(&index), 13);
        assert_eq!(read_file_body(&mut index), "<h1>site</h1>");

        let css = request_site(&format!("/w/{}/style.css", site.id), &site);
        let css_header = header_of(&css);
        assert!(css_header.contains("Content-Type: text/css; charset=utf-8"));
        assert!(!css_header.contains("Content-Disposition"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nested_site_index_is_served_inline() {
        let root = temp_test_dir("site-nested");
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("app").join("index.html"), "<p>app</p>").unwrap();
        let site = site_session("nested", &root, 48801);

        let out = request_site(&format!("/w/{}/app/", site.id), &site);
        let header = header_of(&out);
        assert!(header.contains("Content-Type: text/html; charset=utf-8"));
        assert!(!header.contains("Content-Disposition"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn site_root_server_serves_absolute_assets() {
        let root = temp_test_dir("site-root-server");
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("index.html"), "<h1>site</h1>").unwrap();
        fs::write(root.join("assets").join("app.js"), "console.log(1)").unwrap();
        let site = site_session("root-site", &root, 48802);

        let index = route_site_root("GET", "/", &site, "127.0.0.1", None, None);
        let index_header = header_of(&index);
        assert!(index_header.contains("Content-Type: text/html; charset=utf-8"));
        assert!(!index_header.contains("Content-Disposition"));

        let asset = route_site_root("GET", "/assets/app.js", &site, "127.0.0.1", None, None);
        let asset_header = header_of(&asset);
        assert!(asset_header.contains("Content-Type: application/javascript"));
        assert!(!asset_header.contains("Content-Disposition"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_without_index_keeps_listing_and_attachment() {
        let root = temp_test_dir("index-none");
        fs::write(root.join("readme.txt"), "hello").unwrap();
        fs::write(root.join("archive.zip"), "zip").unwrap();
        let share = folder_session("plain", &root);

        let listing = request_share(&format!("/s/{}/", share.id), &share);
        match listing.body {
            ResponseBody::Full(bytes) => {
                let body = String::from_utf8(bytes).unwrap();
                assert!(body.contains("readme.txt"));
                assert!(body.contains("archive.zip"));
            }
            _ => panic!("expected folder listing"),
        }

        let file = request_share(&format!("/s/{}/archive.zip", share.id), &share);
        let header = header_of(&file);
        assert!(header.contains("Content-Disposition: attachment"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn receive_page_renders_upload_form_and_allowed_types() {
        let receiver = receive_session("recv1", &["jpg", "jpeg", "png"], ReceiveEncryption::Common);

        let out = request_receive(&format!("/r/{}/", receiver.id), &receiver);
        match out.body {
            ResponseBody::Full(bytes) => {
                let body = String::from_utf8(bytes).unwrap();
                assert!(body.contains("/r/recv1/upload"));
                assert!(body.contains(".jpg"));
                assert!(body.contains("通用加密"));
                assert!(body.contains("upload-password"));
                assert!(body.contains("selected-list"));
            }
            _ => panic!("expected receive page"),
        }
    }
}

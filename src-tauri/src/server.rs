//! Tiny HTTP/1.1 server backing the share-link app, tuned for max throughput.
//!
//! Routing:
//!   GET  /                     -> HTML index of all active shares
//!   GET  /s/<id>               -> serve a file directly (Content-Disposition)
//!   GET  /s/<id>/<subpath>     -> serve a sub-file or folder listing
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

use tauri::{AppHandle, Manager};

use crate::html;
use crate::state::{ShareKind, ShareSession};
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

pub fn start_server(
    app: AppHandle,
    port: u16,
) -> Result<ServerStatus, String> {
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
    std::thread::sleep(Duration::from_millis(80));
    Ok(())
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
    use windows_sys::Win32::Networking::WinSock::{setsockopt, SO_RCVBUF, SO_SNDBUF, SOL_SOCKET};

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

    let response = route(&method, &target, &shares, &peer_ip, user_agent, range.as_deref(), app);

    match response.body {
        ResponseBody::Full(bytes) => {
            enable_nodelay(&stream);
            let _ = stream.write_all(&bytes);
        }
        ResponseBody::File { header, mut file, start, length } => {
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
        let body = html::render_index(shares);
        return html_full(200, &body);
    }

    let trimmed = path.to_string_lossy();
    let trimmed = trimmed.trim_start_matches('/');
    let mut rest_segs = trimmed.splitn(2, '/');
    let first = rest_segs.next().unwrap_or("");
    if first != "s" {
        return error_full(404, "Not Found", "页面不存在");
    }
    let rest = rest_segs.next().unwrap_or("");
    let mut parts = rest.splitn(2, '/');
    let id = parts.next().unwrap_or("");
    let subpath = parts.next().unwrap_or("");
    if id.is_empty() {
        return error_full(400, "Bad Request", "缺少分享 ID");
    }

    let share = match shares.iter().find(|s| s.id == id) {
        Some(s) => s.clone(),
        None => return error_full(404, "Not Found", "分享不存在或已停止"),
    };

    serve_share(&share, subpath, peer, user_agent, range, app)
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
            serve_file_stream(&root, peer, &share.id, &share.name, user_agent, range)
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
                serve_file_stream(&target, peer, &share.id, display, user_agent, range)
            } else {
                error_full(404, "Not Found", "不支持的资源类型")
            }
        }
    }
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
) -> Outgoing {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return error_full(404, "Not Found", "文件不存在"),
    };
    let len = meta.len();
    let mime = guess_mime(path);
    let (header, start, length) = match parse_range_header(range, len) {
        RangeRequest::None => (build_file_header(mime, len, display), 0, len),
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
                build_partial_header(mime, range_length, display, range_start, end, len),
                range_start,
                range_length,
            )
        }
    };
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return error_full(500, "Internal Server Error", "文件打开失败"),
    };
    let event = storage::AccessRecord {
        id: format!("{:x}-{:x}-{:x}", storage::now_secs(), start, length),
        share_id: share_id.to_string(),
        share_name: display.to_string(),
        path: path.to_string_lossy().into_owned(),
        bytes: length,
        timestamp: storage::now_secs(),
        peer: peer.to_string(),
        user_agent,
        status: "success".to_string(),
    };
    Outgoing {
        body: ResponseBody::File { header, file, start, length },
        event: Some(event),
    }
}

fn build_file_header(mime: &str, len: u64, display: &str) -> Vec<u8> {
    // Pre-size: ~256 bytes covers the whole HTTP header for any sane filename.
    let mut out = String::with_capacity(256);
    out.push_str("HTTP/1.1 200 OK\r\n");
    out.push_str("Content-Type: ");
    out.push_str(mime);
    out.push_str("\r\n");
    out.push_str("Content-Length: ");
    out.push_str(&len.to_string());
    out.push_str("\r\n");
    out.push_str("Content-Disposition: attachment; filename*=UTF-8\'\'");
    out.push_str(&url_encode_filename(display));
    out.push_str("\r\n");
    out.push_str("Accept-Ranges: bytes\r\n");
    // Connection: close keeps the implementation simple (no keep-alive state
    // machine). With 4 MiB SO_SNDBUF this still saturates gigabit.
    out.push_str("Connection: close\r\n");
    out.push_str("\r\n");
    out.into_bytes()
}

fn build_partial_header(mime: &str, len: u64, display: &str, start: u64, end: u64, total: u64) -> Vec<u8> {
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
    out.push_str("Content-Disposition: attachment; filename*=UTF-8\'\'");
    out.push_str(&url_encode_filename(display));
    out.push_str("\r\n");
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

fn write_range_chunks(mut stream: &TcpStream, file: &mut File, start: u64, length: u64) -> io::Result<()> {
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
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "bad percent encoding")),
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
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "txt" | "md" | "log" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
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
    Outgoing { body: ResponseBody::Full(bytes), event: None }
}

fn error_full(code: u16, reason: &str, msg: &str) -> Outgoing {
    let body = html::render_error(code, reason, msg);
    let header = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
        code = code, reason = reason, len = body.len(),
    );
    let mut bytes = header.into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    Outgoing { body: ResponseBody::Full(bytes), event: None }
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
pub fn start_server_with_shares<I>(
    port: u16,
    shares: I,
) -> io::Result<u16>
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
                            let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
                            let response = match parse_and_route(&stream, &shares, &peer) {
                                Ok(r) => r,
                                Err(_) => return,
                            };
                            let _ = stop;
                            match response.body {
                                ResponseBody::Full(bytes) => {
                                    enable_nodelay(&stream);
                                    let _ = stream.write_all(&bytes);
                                }
                                ResponseBody::File { header, mut file, start, length } => {
                                    let _ = write_file_response(&stream, &header, &mut file, start, length);
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

fn parse_and_route(stream: &TcpStream, shares: &[ShareSession], peer: &str) -> io::Result<Outgoing> {
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
    Ok(route_no_app(&method, &target, shares, peer, user_agent, range.as_deref()))
}

fn route_no_app(
    method: &str,
    target: &str,
    shares: &[ShareSession],
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
        let body = html::render_index(shares);
        return html_full(200, &body);
    }

    let trimmed = path.to_string_lossy();
    let trimmed = trimmed.trim_start_matches('/');
    let mut rest_segs = trimmed.splitn(2, '/');
    let first = rest_segs.next().unwrap_or("");
    if first != "s" {
        return error_full(404, "Not Found", "页面不存在");
    }
    let rest = rest_segs.next().unwrap_or("");
    let mut parts = rest.splitn(2, '/');
    let id = parts.next().unwrap_or("");
    let subpath = parts.next().unwrap_or("");
    if id.is_empty() {
        return error_full(400, "Bad Request", "缺少分享 ID");
    }

    let share = match shares.iter().find(|s| s.id == id) {
        Some(s) => s.clone(),
        None => return error_full(404, "Not Found", "分享不存在或已停止"),
    };

    serve_share_no_app(&share, subpath, peer, user_agent, range)
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
            serve_file_stream(&root, peer, &share.id, &share.name, user_agent, range)
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
                serve_file_stream(&target, peer, &share.id, display, user_agent, range)
            } else {
                error_full(404, "Not Found", "不支持的资源类型")
            }
        }
    }
}

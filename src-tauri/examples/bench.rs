//! Throughput benchmark for the embedded HTTP server.
//!
//! Starts the server with a single file share, then downloads the file
//! several times via raw sockets to measure MB/s.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Instant;

use rv_netshare_lib::server::start_server_with_shares;
use rv_netshare_lib::state::{ShareKind, ShareSession};

fn main() {
    let path = std::env::args().nth(1).expect("usage: bench <file-path> [port]");
    let port: u16 = std::env::args().nth(2).map(|s| s.parse().unwrap()).unwrap_or(48721);

    let file = PathBuf::from(&path);
    let meta = std::fs::metadata(&file).expect("file not found");
    let len = meta.len();
    println!("Sharing {} ({} bytes)", file.display(), len);

    let session = ShareSession {
        id: "bench0001".to_string(),
        name: file.file_name().and_then(|s| s.to_str()).unwrap_or("bench").to_string(),
        path: file.to_string_lossy().into_owned(),
        kind: ShareKind::File,
        size: len,
        total_bytes: len,
        created_at: 0,
    };

    let bound = start_server_with_shares(port, vec![session]).expect("bind failed");
    println!("Server listening on 127.0.0.1:{bound}");
    std::thread::sleep(std::time::Duration::from_millis(100));

    let url_path = "/s/bench0001".to_string();
    let iters = 3usize;
    let mut best = 0f64;

    for i in 0..iters {
        let mut stream = TcpStream::connect(("127.0.0.1", bound)).expect("connect");
        let req = format!("GET {url_path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        let start = Instant::now();
        let mut total = 0usize;
        let mut buf = vec![0u8; 1024 * 1024];
        loop {
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 { break; }
            total += n;
        }
        let elapsed = start.elapsed().as_secs_f64();
        let mbps = (total as f64 / 1024.0 / 1024.0) / elapsed;
        best = best.max(mbps);
        println!("run {i}: {} bytes in {elapsed:.3}s -> {mbps:.1} MB/s", total);
    }
    println!("best: {best:.1} MB/s");
}

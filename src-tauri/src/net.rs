//! Networking helpers implemented on top of std::net only.
//!
//! get_local_ip uses the well-known UDP-socket trick: bind to an
//! arbitrary local port, "connect" to a public IP (no packets are
//! actually sent), then read back the local address the kernel picked.

use std::io;
use std::net::UdpSocket;

pub fn get_local_ip() -> io::Result<String> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    // Connecting to a public IP doesn't send any traffic but forces the
    // kernel to associate the socket with the outbound interface.
    socket.connect("8.8.8.8:80")?;
    let addr = socket.local_addr()?;
    Ok(addr.ip().to_string())
}

pub fn get_hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

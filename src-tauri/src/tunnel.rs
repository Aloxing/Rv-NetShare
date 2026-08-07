//! ngrok tunnel worker for exposing shares and sites to the public internet.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use ngrok::config::ForwarderBuilder;
use ngrok::forwarder::Forwarder;
use ngrok::prelude::{EndpointInfo, TunnelCloser};
use ngrok::tunnel::HttpTunnel;
use ngrok::Session;
use tokio::runtime::Runtime;
use url::Url;

enum TunnelCommand {
    Start {
        key: String,
        target: String,
        authtoken: Option<String>,
        reply: Sender<Result<String, String>>,
    },
    Stop {
        key: String,
        reply: Sender<Result<(), String>>,
    },
    Reset {
        reply: Sender<Result<(), String>>,
    },
    Shutdown,
}

pub struct TunnelManager {
    sender: Sender<TunnelCommand>,
    worker: Option<JoinHandle<()>>,
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelManager {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        let worker = std::thread::Builder::new()
            .name("ngrok-tunnel-worker".into())
            .spawn(move || run_worker(receiver))
            .ok();
        Self { sender, worker }
    }

    pub fn start(
        &self,
        key: String,
        target: String,
        authtoken: Option<String>,
    ) -> Result<String, String> {
        self.request(|reply| TunnelCommand::Start {
            key,
            target,
            authtoken,
            reply,
        })?
    }

    pub fn stop(&self, key: String) -> Result<(), String> {
        self.request(|reply| TunnelCommand::Stop { key, reply })?
    }

    pub fn reset(&self) -> Result<(), String> {
        self.request(|reply| TunnelCommand::Reset { reply })?
    }

    fn request<T: Send + 'static>(
        &self,
        make: impl FnOnce(Sender<T>) -> TunnelCommand,
    ) -> Result<T, String> {
        let (reply_tx, reply_rx) = channel();
        self.sender
            .send(make(reply_tx))
            .map_err(|_| "内网穿透服务未启动".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(60))
            .map_err(|_| "ngrok 连接超时".to_string())
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        let _ = self.sender.send(TunnelCommand::Shutdown);
        self.worker.take();
    }
}

fn run_worker(receiver: Receiver<TunnelCommand>) {
    let runtime = match Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[ngrok] failed to start tokio runtime: {e}");
            return;
        }
    };

    let mut tunnels: HashMap<String, Forwarder<HttpTunnel>> = HashMap::new();
    let mut session: Option<(String, Session)> = None;

    while let Ok(command) = receiver.recv() {
        match command {
            TunnelCommand::Start {
                key,
                target,
                authtoken,
                reply,
            } => {
                let result = runtime.block_on(start_tunnel(
                    &mut session,
                    &mut tunnels,
                    key,
                    target,
                    authtoken,
                ));
                let _ = reply.send(result);
            }
            TunnelCommand::Stop { key, reply } => {
                let result = runtime.block_on(stop_tunnel(&mut tunnels, key));
                let _ = reply.send(result);
            }
            TunnelCommand::Reset { reply } => {
                let result = runtime.block_on(async {
                    close_all(&mut tunnels).await;
                    if let Some((_, mut sess)) = session.take() {
                        let _ = sess.close().await;
                    }
                    Ok(())
                });
                let _ = reply.send(result);
            }
            TunnelCommand::Shutdown => {
                runtime.block_on(async {
                    close_all(&mut tunnels).await;
                    if let Some((_, mut sess)) = session.take() {
                        let _ = sess.close().await;
                    }
                });
                break;
            }
        }
    }
}

async fn start_tunnel(
    session_slot: &mut Option<(String, Session)>,
    tunnels: &mut HashMap<String, Forwarder<HttpTunnel>>,
    key: String,
    target: String,
    authtoken: Option<String>,
) -> Result<String, String> {
    if let Some(forwarder) = tunnels.get(&key) {
        return Ok(forwarder.url().to_string());
    }

    let token_key = authtoken
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
        .to_string();

    let session = match session_slot.take() {
        Some((saved, sess)) if saved == token_key => Some(sess),
        Some((_, mut sess)) => {
            close_all(tunnels).await;
            let _ = sess.close().await;
            None
        }
        None => None,
    };

    let session = match session {
        Some(sess) => sess,
        None => {
            let mut builder = Session::builder();
            if token_key.is_empty() {
                builder.authtoken_from_env();
            } else {
                builder.authtoken(token_key.clone());
            }
            let sess = builder
                .connect()
                .await
                .map_err(|e| format!("ngrok 连接失败: {e}"))?;
            *session_slot = Some((token_key, sess.clone()));
            sess
        }
    };

    let target_url = Url::parse(&target).map_err(|e| format!("转发地址无效: {e}"))?;
    let forwarder = session
        .http_endpoint()
        .metadata(format!("rv-netshare:{key}"))
        .listen_and_forward(target_url)
        .await
        .map_err(|e| format!("ngrok 隧道创建失败: {e}"))?;
    let public_url = forwarder.url().to_string();
    tunnels.insert(key, forwarder);
    Ok(public_url)
}

async fn stop_tunnel(
    tunnels: &mut HashMap<String, Forwarder<HttpTunnel>>,
    key: String,
) -> Result<(), String> {
    if let Some(mut forwarder) = tunnels.remove(&key) {
        forwarder
            .close()
            .await
            .map_err(|e| format!("关闭隧道失败: {e}"))?;
    }
    Ok(())
}

async fn close_all(tunnels: &mut HashMap<String, Forwarder<HttpTunnel>>) {
    for (_, mut forwarder) in tunnels.drain() {
        let _ = forwarder.close().await;
    }
}

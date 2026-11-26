use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use calyx_remote_protocol::{DiscoveryInfo, ErrorKind, RequestEnvelope, ResponseEnvelope};

/// Environment variable that enables the remote control server. `0` binds an
/// ephemeral port.
pub const REMOTE_PORT_ENV: &str = "CALYX_REMOTE_PORT";

/// Server configuration resolved from the environment.
pub struct RemoteConfig {
    /// Port to bind on localhost; `0` picks an ephemeral port.
    pub port: u16,
    /// Where to write the discovery file clients use to find the port.
    pub discovery_path: PathBuf,
}

/// Returns the remote server configuration, or `None` when remote control is
/// not enabled for this process.
pub fn remote_config_from_env(project_path: &Path) -> Option<RemoteConfig> {
    let raw = std::env::var(REMOTE_PORT_ENV).ok()?;
    let port = match raw.trim().parse::<u16>() {
        Ok(port) => port,
        Err(_) => {
            log::error!("Ignoring invalid {REMOTE_PORT_ENV}={raw:?}; expected a port number");
            return None;
        }
    };
    Some(RemoteConfig {
        port,
        discovery_path: project_path.join(".calyx").join("remote.json"),
    })
}

/// Identifies one connected remote client.
pub type ClientId = u64;

/// A message surfaced to the main thread by the connection threads.
pub enum Incoming {
    /// A parsed request from a client.
    Request(ClientId, RequestEnvelope),
    /// A request line that could not be parsed.
    ParseError(ClientId, String),
    /// The client disconnected; any pending responses for it can be dropped.
    Disconnected(ClientId),
}

/// Localhost TCP server speaking the newline-delimited JSON remote protocol.
///
/// Connection handling runs on background threads; requests are queued and
/// must be drained and answered from the main thread via [`Self::drain`] and
/// [`Self::respond`].
pub struct RemoteServer {
    incoming: mpsc::Receiver<Incoming>,
    clients: Arc<Mutex<HashMap<ClientId, mpsc::Sender<String>>>>,
    bound_port: u16,
    discovery_path: Option<PathBuf>,
}

impl RemoteServer {
    /// Binds the server, spawns the accept thread, and writes the discovery
    /// file.
    pub fn start(config: RemoteConfig) -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.port))?;
        let bound_port = listener.local_addr()?.port();

        let discovery_path = match write_discovery_file(&config.discovery_path, bound_port) {
            Ok(()) => Some(config.discovery_path),
            Err(error) => {
                log::warn!(
                    "Failed to write remote discovery file {}: {error}",
                    config.discovery_path.display()
                );
                None
            }
        };

        let (incoming_tx, incoming_rx) = mpsc::channel();
        let clients: Arc<Mutex<HashMap<ClientId, mpsc::Sender<String>>>> = Default::default();

        let accept_clients = Arc::clone(&clients);
        std::thread::Builder::new()
            .name("remote-accept".into())
            .spawn(move || accept_loop(listener, incoming_tx, accept_clients))?;

        log::info!("Remote control server listening on 127.0.0.1:{bound_port}");
        Ok(Self {
            incoming: incoming_rx,
            clients,
            bound_port,
            discovery_path,
        })
    }

    /// Returns the port the server is listening on.
    pub fn bound_port(&self) -> u16 {
        self.bound_port
    }

    /// Drains all messages received since the last call. Main thread only.
    pub fn drain(&self) -> Vec<Incoming> {
        self.incoming.try_iter().collect()
    }

    /// Sends a response to a client, dropping it silently if the client is
    /// gone.
    pub fn respond(&self, client: ClientId, response: &ResponseEnvelope) {
        let line = match serde_json::to_string(response) {
            Ok(line) => line,
            Err(error) => {
                log::error!("Failed to serialize remote response: {error}");
                return;
            }
        };
        let sender = self.clients.lock().unwrap().get(&client).cloned();
        if let Some(sender) = sender {
            let _ = sender.send(line);
        }
    }
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        if let Some(path) = &self.discovery_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn write_discovery_file(path: &Path, port: u16) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let info = DiscoveryInfo {
        port,
        pid: std::process::id(),
    };
    std::fs::write(
        path,
        serde_json::to_string(&info).expect("serialize discovery info"),
    )
}

fn accept_loop(
    listener: TcpListener,
    incoming: mpsc::Sender<Incoming>,
    clients: Arc<Mutex<HashMap<ClientId, mpsc::Sender<String>>>>,
) {
    static NEXT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                log::warn!("Remote control accept failed: {error}");
                continue;
            }
        };
        let client = NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed);
        let _ = stream.set_nodelay(true);
        log::info!("Remote control client {client} connected");

        let (outgoing_tx, outgoing_rx) = mpsc::channel::<String>();
        clients.lock().unwrap().insert(client, outgoing_tx);

        let write_stream = match stream.try_clone() {
            Ok(stream) => stream,
            Err(error) => {
                log::warn!("Failed to clone remote client stream: {error}");
                clients.lock().unwrap().remove(&client);
                continue;
            }
        };
        let _ = std::thread::Builder::new()
            .name(format!("remote-write-{client}"))
            .spawn(move || write_loop(write_stream, outgoing_rx));

        let reader_incoming = incoming.clone();
        let reader_clients = Arc::clone(&clients);
        let _ = std::thread::Builder::new()
            .name(format!("remote-read-{client}"))
            .spawn(move || {
                read_loop(stream, client, &reader_incoming);
                reader_clients.lock().unwrap().remove(&client);
                let _ = reader_incoming.send(Incoming::Disconnected(client));
                log::info!("Remote control client {client} disconnected");
            });
    }
}

fn read_loop(stream: TcpStream, client: ClientId, incoming: &mpsc::Sender<Incoming>) {
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => return,
        };
        if line.trim().is_empty() {
            continue;
        }
        let message = match serde_json::from_str::<RequestEnvelope>(&line) {
            Ok(request) => Incoming::Request(client, request),
            Err(error) => Incoming::ParseError(client, error.to_string()),
        };
        if incoming.send(message).is_err() {
            return;
        }
    }
}

fn write_loop(mut stream: TcpStream, outgoing: mpsc::Receiver<String>) {
    while let Ok(mut line) = outgoing.recv() {
        line.push('\n');
        if stream.write_all(line.as_bytes()).is_err() {
            return;
        }
    }
}

/// Builds the standard parse-error response for a malformed request line.
pub fn parse_error_response(message: String) -> ResponseEnvelope {
    ResponseEnvelope::error(0, ErrorKind::Parse, message)
}

#[cfg(test)]
mod tests;

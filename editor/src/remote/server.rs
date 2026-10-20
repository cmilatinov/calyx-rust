use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use remote_protocol::{DiscoveryInfo, ErrorKind, RequestEnvelope, ResponseEnvelope};

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
mod tests {
    use super::*;
    use remote_protocol::{Command, ResponsePayload};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    fn wait_for_messages(server: &RemoteServer, count: usize) -> Vec<Incoming> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut messages = Vec::new();
        while messages.len() < count && Instant::now() < deadline {
            messages.extend(server.drain());
            std::thread::sleep(Duration::from_millis(5));
        }
        messages
    }

    fn start_test_server() -> (RemoteServer, PathBuf) {
        let discovery_path = std::env::temp_dir()
            .join(format!("calyx-remote-test-{}", uuid::Uuid::new_v4()))
            .join("remote.json");
        let server = RemoteServer::start(RemoteConfig {
            port: 0,
            discovery_path: discovery_path.clone(),
        })
        .expect("server should bind an ephemeral port");
        (server, discovery_path)
    }

    #[test]
    fn requests_round_trip_through_the_server() {
        let (server, discovery_path) = start_test_server();

        let discovery: DiscoveryInfo =
            serde_json::from_str(&std::fs::read_to_string(&discovery_path).unwrap()).unwrap();
        assert_eq!(discovery.port, server.bound_port());
        assert_eq!(discovery.pid, std::process::id());

        let mut stream = TcpStream::connect(("127.0.0.1", server.bound_port())).unwrap();
        stream
            .write_all(b"{\"id\":42,\"cmd\":\"ping\"}\nnot json\n")
            .unwrap();

        let messages = wait_for_messages(&server, 2);
        assert_eq!(messages.len(), 2);
        let Incoming::Request(client, request) = &messages[0] else {
            panic!("expected a request first");
        };
        assert_eq!(request.id, 42);
        assert_eq!(request.command, Command::Ping);
        assert!(matches!(messages[1], Incoming::ParseError(..)));

        server.respond(*client, &ResponseEnvelope::ok(42, serde_json::Value::Null));
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let response: ResponseEnvelope = serde_json::from_str(&line).unwrap();
        assert_eq!(response.id, 42);
        assert!(matches!(response.payload, ResponsePayload::Ok { .. }));

        drop(stream);
        drop(reader);
        let messages = wait_for_messages(&server, 1);
        assert!(messages
            .iter()
            .any(|message| matches!(message, Incoming::Disconnected(_))));

        let _ = std::fs::remove_dir_all(discovery_path.parent().unwrap());
    }

    #[test]
    fn multiple_clients_are_answered_independently() {
        let (server, discovery_path) = start_test_server();

        let mut first = TcpStream::connect(("127.0.0.1", server.bound_port())).unwrap();
        let mut second = TcpStream::connect(("127.0.0.1", server.bound_port())).unwrap();
        first.write_all(b"{\"id\":1,\"cmd\":\"ping\"}\n").unwrap();
        second.write_all(b"{\"id\":2,\"cmd\":\"info\"}\n").unwrap();

        let messages = wait_for_messages(&server, 2);
        for message in &messages {
            let Incoming::Request(client, request) = message else {
                panic!("expected requests only");
            };
            server.respond(
                *client,
                &ResponseEnvelope::ok(request.id, serde_json::json!({ "echo": request.id })),
            );
        }

        for (stream, expected_id) in [(&mut first, 1), (&mut second, 2)] {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let response: ResponseEnvelope = serde_json::from_str(&line).unwrap();
            assert_eq!(response.id, expected_id);
        }

        let _ = std::fs::remove_dir_all(discovery_path.parent().unwrap());
    }
}

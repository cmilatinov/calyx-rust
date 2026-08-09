use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

use remote_protocol::{Command, DiscoveryInfo, RequestEnvelope, ResponseEnvelope, ResponsePayload};

/// Client-side failures that are not command errors from the editor.
#[derive(Debug)]
pub enum CtlError {
    /// The discovery file is missing or unreadable.
    Discovery(String),
    /// Connecting, sending, or receiving failed.
    Transport(String),
    /// No matching response arrived within the timeout.
    Timeout(String),
}

impl std::fmt::Display for CtlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CtlError::Discovery(message) => write!(f, "discovery failed: {message}"),
            CtlError::Transport(message) => write!(f, "transport failed: {message}"),
            CtlError::Timeout(message) => write!(f, "timed out: {message}"),
        }
    }
}

/// Reads the editor's discovery file from `<project>/.calyx/remote.json`.
pub fn discover(project_dir: &Path) -> Result<DiscoveryInfo, CtlError> {
    let path = discovery_path(project_dir);
    let contents = std::fs::read_to_string(&path).map_err(|error| {
        CtlError::Discovery(format!(
            "cannot read {} ({error}); is the editor running with CALYX_REMOTE_PORT set?",
            path.display()
        ))
    })?;
    serde_json::from_str(&contents)
        .map_err(|error| CtlError::Discovery(format!("invalid {}: {error}", path.display())))
}

/// Returns the discovery file location for a project directory.
pub fn discovery_path(project_dir: &Path) -> std::path::PathBuf {
    project_dir.join(".calyx").join("remote.json")
}

/// A connected remote-protocol client.
pub struct Client {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    next_id: u64,
}

impl Client {
    /// Connects to the editor on localhost.
    pub fn connect(port: u16, timeout: Duration) -> Result<Self, CtlError> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let stream = TcpStream::connect_timeout(&address, timeout)
            .map_err(|error| CtlError::Transport(format!("connect to {address}: {error}")))?;
        let _ = stream.set_nodelay(true);
        let reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|error| CtlError::Transport(error.to_string()))?,
        );
        Ok(Self {
            stream,
            reader,
            next_id: 1,
        })
    }

    /// Sends `command` and waits for the matching response, ignoring responses
    /// to other requests.
    pub fn call(
        &mut self,
        command: Command,
        timeout: Duration,
    ) -> Result<ResponsePayload, CtlError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = RequestEnvelope { id, command };
        let mut line = serde_json::to_string(&request)
            .map_err(|error| CtlError::Transport(error.to_string()))?;
        line.push('\n');
        self.stream
            .write_all(line.as_bytes())
            .map_err(|error| CtlError::Transport(format!("send: {error}")))?;

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| CtlError::Timeout(format!("no response after {timeout:?}")))?;
            self.stream
                .set_read_timeout(Some(remaining))
                .map_err(|error| CtlError::Transport(error.to_string()))?;

            let mut response_line = String::new();
            match self.reader.read_line(&mut response_line) {
                Ok(0) => return Err(CtlError::Transport("editor closed the connection".into())),
                Ok(_) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut =>
                {
                    return Err(CtlError::Timeout(format!("no response after {timeout:?}")));
                }
                Err(error) => return Err(CtlError::Transport(format!("receive: {error}"))),
            }

            let response: ResponseEnvelope = match serde_json::from_str(response_line.trim()) {
                Ok(response) => response,
                Err(error) => {
                    return Err(CtlError::Transport(format!(
                        "invalid response line {response_line:?}: {error}"
                    )))
                }
            };
            if response.id == id {
                return Ok(response.payload);
            }
            // A response for another request (e.g. from a previous timed-out
            // call); keep waiting for ours.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn call_matches_response_ids_and_ignores_others() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request: RequestEnvelope = serde_json::from_str(line.trim()).unwrap();
            // Send an unrelated response first, then the real one.
            let noise = ResponseEnvelope::ok(9999, serde_json::json!("noise"));
            let real = ResponseEnvelope::ok(request.id, serde_json::json!({ "pong": true }));
            for response in [noise, real] {
                let mut out = serde_json::to_string(&response).unwrap();
                out.push('\n');
                stream.write_all(out.as_bytes()).unwrap();
            }
        });

        let mut client = Client::connect(port, Duration::from_secs(5)).unwrap();
        let payload = client
            .call(Command::Ping, Duration::from_secs(5))
            .expect("call should succeed");
        match payload {
            ResponsePayload::Ok { data } => assert_eq!(data, serde_json::json!({ "pong": true })),
            ResponsePayload::Error { .. } => panic!("expected ok"),
        }
        server.join().unwrap();
    }

    #[test]
    fn discovery_parses_the_editor_file() {
        let dir = std::env::temp_dir().join(format!("calyx-ctl-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".calyx")).unwrap();
        std::fs::write(
            dir.join(".calyx").join("remote.json"),
            r#"{"port":4655,"pid":7}"#,
        )
        .unwrap();
        let info = discover(&dir).unwrap();
        assert_eq!(info.port, 4655);
        assert_eq!(info.pid, 7);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

use super::*;
use calyx_remote_protocol::{Command, ResponsePayload};
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

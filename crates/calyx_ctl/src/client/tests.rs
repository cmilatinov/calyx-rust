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

#![cfg(feature = "client")]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};
use teka::client::{wait_for_commit, DeviceClient, PasswordRequired};

fn request(stream: &mut TcpStream) -> (String, Vec<(String, String)>, Vec<u8>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut input = BufReader::new(stream);
    let mut line = String::new();
    assert!(input.read_line(&mut line).unwrap() > 0);
    let path = line.trim_end().to_owned();
    let mut headers = Vec::new();
    loop {
        line.clear();
        assert!(input.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        let (key, value) = line.trim_end().split_once(':').unwrap();
        headers.push((key.to_ascii_lowercase(), value.trim().to_owned()));
    }
    let length = headers
        .iter()
        .find(|(key, _)| key == "content-length")
        .map(|(_, value)| value.parse().unwrap())
        .unwrap_or(0);
    let mut body = vec![0; length];
    input.read_exact(&mut body).unwrap();
    (path, headers, body)
}

fn reply(stream: &mut TcpStream, body: &str, headers: &str) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len(),
    )
    .unwrap();
}

#[test]
fn explicit_credentials_keep_session_and_upload_returns_data() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = format!("http://{}/proxy/", listener.local_addr().unwrap());
    let path = std::env::temp_dir().join(format!(
        "teka-client-{}-{}",
        std::process::id(),
        listener.local_addr().unwrap().port(),
    ));
    std::fs::write(&path, b"payload\0bytes").unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert_eq!(
            request(&mut stream).0,
            "GET /proxy/api/v1/auth/status HTTP/1.1"
        );
        reply(&mut stream, r#"{"needs_setup":true}"#, "");

        let (mut stream, _) = listener.accept().unwrap();
        let (path, _, body) = request(&mut stream);
        assert_eq!(path, "POST /proxy/api/v1/auth/setup HTTP/1.1");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({"password": "explicit secret"}),
        );
        reply(&mut stream, "{}", "Set-Cookie: session=test; Path=/\r\n");

        let (mut stream, _) = listener.accept().unwrap();
        let (path, headers, _) = request(&mut stream);
        assert_eq!(path, "GET /proxy/api/v1/platform/architecture HTTP/1.1");
        assert!(headers
            .iter()
            .any(|(key, value)| key == "cookie" && value == "session=test"));
        reply(&mut stream, r#"{"architecture":"ARM64"}"#, "");

        let (mut stream, _) = listener.accept().unwrap();
        let (path, headers, body) = request(&mut stream);
        assert_eq!(
            path,
            "POST /proxy/api/v1/manager/install?profile=trusted HTTP/1.1"
        );
        assert_eq!(body, b"payload\0bytes");
        assert!(headers
            .iter()
            .any(|(key, value)| key == "cookie" && value == "session=test"));
        reply(&mut stream, " {\"app_id\":\"demo\"}\n", "");
    });
    let client = DeviceClient::new(&host).unwrap();
    client.authenticate(Some("explicit secret")).unwrap();
    assert_eq!(client.architecture().unwrap(), "aarch64");
    let response = client.install(&path, "trusted").unwrap();
    assert_eq!(response.value, serde_json::json!({"app_id": "demo"}));
    assert_eq!(response.text, " {\"app_id\":\"demo\"}\n");
    server.join().unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn authentication_requires_explicit_password_only_when_needed() {
    for (status, required) in [
        (r#"{"auth_disabled":true}"#, false),
        (r#"{"authenticated":true}"#, false),
        (r#"{"enforcement":true}"#, true),
        (r#"{"needs_setup":true}"#, true),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            request(&mut stream);
            reply(&mut stream, status, "");
            if required {
                let (mut stream, _) = listener.accept().unwrap();
                let (path, _, _) = request(&mut stream);
                let action = if status.contains("needs_setup") {
                    "setup"
                } else {
                    "login"
                };
                assert_eq!(path, format!("POST /api/v1/auth/{action} HTTP/1.1"));
                write!(stream, "HTTP/1.1 401 Unauthorized\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecret").unwrap();
            }
        });
        let client = DeviceClient::new(&host).unwrap();
        let result = client.authenticate(None);
        if required {
            assert!(result.unwrap_err().is::<PasswordRequired>());
            let status = serde_json::from_str(status).unwrap();
            let error = client.authenticate_password(&status, "secret").unwrap_err();
            assert_eq!(
                error.downcast_ref::<reqwest::Error>().unwrap().status(),
                Some(reqwest::StatusCode::UNAUTHORIZED)
            );
            assert!(error.to_string().contains("401"));
            assert!(!format!("{error:?}").contains("secret"));
        } else {
            result.unwrap();
        }
        server.join().unwrap();
    }
}

#[test]
fn update_wait_accepts_explicit_timing_and_detects_commit() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for status in [
            r#"{"version":"new","update":{"commit_state":"PENDING"}}"#,
            r#"{"version":"new","update":{"commit_state":"COMMITTED"}}"#,
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            assert!(request(&mut stream).0.contains("/auth/status"));
            reply(&mut stream, r#"{"auth_disabled":true}"#, "");
            let (mut stream, _) = listener.accept().unwrap();
            assert!(request(&mut stream).0.contains("/app/teka/status"));
            reply(&mut stream, status, "");
        }
    });
    let status = wait_for_commit(
        &host,
        None,
        "old",
        Instant::now() + Duration::from_secs(10),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(status["version"], "new");
    server.join().unwrap();
}

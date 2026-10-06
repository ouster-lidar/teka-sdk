use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

::app_runtime::declare_app_runtime!("test-app");

#[test]
fn macro_propagates_acquisition_failure_and_stops_before_final_release() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    std::env::set_var(
        "MC_RUNTIME_URL",
        format!("http://{}", listener.local_addr().unwrap()),
    );
    let host = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for status in ["409 Conflict", "200 OK", "204 No Content"] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request = line.clone();
            let mut length = 0;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            requests.push((request, body));
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        }
        requests
    });

    assert!(app_runtime::sync_udp_ports(&[7502, 7503])
        .unwrap_err()
        .contains("409"));
    app_runtime::sync_tcp_ports(&[8000]).unwrap();
    app_runtime::release();
    assert!(app_runtime::sync_tcp_ports(&[8001])
        .unwrap_err()
        .contains("stopping"));
    let requests = host.join().unwrap();
    assert!(requests[0]
        .0
        .starts_with("PUT /api/v1/manager/runtime/test-app/ports "));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&requests[1].1).unwrap(),
        serde_json::json!({"ports": [
            {"port": 8000, "proto": "tcp"},
        ]})
    );
    assert!(requests[2]
        .0
        .starts_with("POST /api/v1/manager/runtime/test-app/release "));
}

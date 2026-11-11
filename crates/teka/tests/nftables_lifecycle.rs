use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use ::teka_app::devres::{NftablesLease, NftablesSet};
use ::teka_app::runtime::{self, ResourceAvailability};
use serde_json::{json, Value};

::teka_app::declare_app_runtime!("lease-test");

#[derive(Default)]
struct Host {
    requests: Mutex<Vec<(String, Value)>>,
    leases: Mutex<Value>,
    ports: Mutex<Value>,
    streams: Mutex<Vec<TcpStream>>,
    listen_port: Mutex<Option<u16>>,
    reject_replay: AtomicBool,
    nftables_gate: Mutex<Option<(mpsc::SyncSender<()>, mpsc::Receiver<()>)>>,
    stop: AtomicBool,
}

impl Host {
    fn handle(&self, mut stream: TcpStream) {
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let path = line.split_whitespace().nth(1).unwrap().to_string();
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
        let body = serde_json::from_slice::<Value>(&body).unwrap_or(json!({}));
        self.requests
            .lock()
            .unwrap()
            .push((path.clone(), body.clone()));
        if path.ends_with("/register") {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n\n\r\n"
            )
            .unwrap();
            self.streams.lock().unwrap().push(stream);
            return;
        }
        let rejected = if path.ends_with("/nftables") {
            if let Some((entered, resume)) = self.nftables_gate.lock().unwrap().take() {
                entered.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(10)).unwrap();
            }
            let rejected = matches!(
                body["leases"][0]["id"].as_str(),
                Some("rejected" | "logical-rejected")
            ) || self.reject_replay.load(Ordering::Acquire);
            if !rejected {
                *self.leases.lock().unwrap() = body.clone();
            }
            rejected
        } else if path.ends_with("/ports") {
            let rejected = body["ports"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["port"] == 8001);
            if !rejected {
                *self.ports.lock().unwrap() = body.clone();
            }
            rejected
        } else {
            if path.ends_with("/release") {
                *self.leases.lock().unwrap() = Value::Null;
                *self.ports.lock().unwrap() = Value::Null;
            }
            false
        };
        let status = if rejected && body["leases"][0]["id"] != "logical-rejected" {
            "409 Conflict"
        } else {
            "200 OK"
        };
        let mut response = json!({"ok": !rejected, "error": if rejected { Some("injected lease rejection") } else { None }});
        if path.ends_with("/activate") {
            if let Some(port) = *self.listen_port.lock().unwrap() {
                response["httpListen"] = json!({"host": "127.0.0.1", "port": port});
            }
        }
        let response = response.to_string();
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        )
        .unwrap();
    }

    fn count(&self, suffix: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.ends_with(suffix))
            .count()
    }

    fn disconnect(&self) {
        for stream in self.streams.lock().unwrap().drain(..) {
            stream.shutdown(Shutdown::Both).unwrap();
        }
    }
}

fn lease(id: &str) -> NftablesLease {
    NftablesLease {
        id: id.into(),
        hook: "input".into(),
        priority: -10,
        sets: vec![NftablesSet {
            name: "sources".into(),
            set_type: "ipv4_addr".into(),
            flags: vec!["dynamic".into(), "timeout".into()],
            timeout: Some(60),
            gc_interval: Some(1),
            size: Some(1024),
        }],
        rules: vec![vec![json!({"counter": null}), json!({"return": null})]],
    }
}

fn wait_for(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for lifecycle event"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn accepted_leases_replay_before_ports_and_dependent_work_waits_for_restoration() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    std::env::set_var(
        "MC_RUNTIME_URL",
        format!("http://{}", listener.local_addr().unwrap()),
    );
    let host = Arc::new(Host::default());
    let serving = host.clone();
    let server = std::thread::spawn(move || {
        while !serving.stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let serving = serving.clone();
                    std::thread::spawn(move || serving.handle(stream));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    let availability = app_runtime::resource_availability();

    // Startup rejection reclaims the activated holder before returning to the caller.
    runtime::activate_resources("lease-test").unwrap();
    assert!(app_runtime::sync_nftables(&[lease("rejected")])
        .unwrap_err()
        .contains("409"));
    assert_eq!(host.count("/release"), 1);
    assert_eq!(*host.leases.lock().unwrap(), Value::Null);

    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    *host.listen_port.lock().unwrap() = Some(occupied.local_addr().unwrap().port());
    let allocated = runtime::activate_http_listen("lease-test").unwrap();
    app_runtime::sync_nftables(&[lease("first")]).unwrap();
    let replacement = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    *host.listen_port.lock().unwrap() = Some(replacement.port());
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let repaired = executor
        .block_on(app_runtime::bind_listener(allocated))
        .unwrap();
    assert_eq!(repaired.local_addr().unwrap(), replacement);
    assert_eq!(
        *host.leases.lock().unwrap(),
        json!({"leases": [lease("first")]})
    );
    let releases = host.count("/release");
    assert_eq!(releases, 2);
    app_runtime::sync_tcp_ports(&[8000]).unwrap();
    runtime::start_supervisor().unwrap();
    wait_for(|| *availability.borrow() == ResourceAvailability::Available);
    assert!(app_runtime::sync_nftables(&[lease("rejected")])
        .unwrap_err()
        .contains("injected lease rejection"));
    assert!(app_runtime::sync_nftables(&[lease("logical-rejected")]).is_err());
    assert!(app_runtime::sync_tcp_ports(&[8001]).is_err());
    assert_eq!(*availability.borrow(), ResourceAvailability::Available);

    let ticks = Arc::new(AtomicUsize::new(0));
    let worker_ticks = ticks.clone();
    let mut worker_availability = app_runtime::resource_availability();
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            loop {
                let state = worker_availability.borrow_and_update().clone();
                match state {
                    ResourceAvailability::Available => {
                        tokio::select! {
                            _ = worker_availability.changed() => {},
                            _ = tokio::time::sleep(Duration::from_millis(5)) => {
                                if *worker_availability.borrow() == ResourceAvailability::Available {
                                    worker_ticks.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                    ResourceAvailability::Unavailable(_) => {
                        worker_availability.changed().await.unwrap();
                    }
                    ResourceAvailability::Stopping => return,
                }
            }
        });
    });
    wait_for(|| ticks.load(Ordering::Relaxed) > 2);
    let accepted = json!({"leases": [lease("first")]});
    assert_eq!(*host.leases.lock().unwrap(), accepted);

    for restart in [false, true] {
        let port_requests = host.count("/ports");
        let nft_requests = host.count("/nftables");
        host.reject_replay.store(true, Ordering::Release);
        if restart {
            *host.leases.lock().unwrap() = Value::Null;
            *host.ports.lock().unwrap() = Value::Null;
        }
        host.disconnect();
        wait_for(|| matches!(*availability.borrow(), ResourceAvailability::Unavailable(_)));
        std::thread::sleep(Duration::from_millis(30));
        let paused = ticks.load(Ordering::Relaxed);
        wait_for(|| host.count("/nftables") > nft_requests);
        assert_eq!(ticks.load(Ordering::Relaxed), paused);
        assert!(matches!(
            *availability.borrow(),
            ResourceAvailability::Unavailable(_)
        ));
        assert_eq!(host.count("/ports"), port_requests);
        // A concurrent port acquisition cannot bypass failed packet-policy replay.
        assert!(app_runtime::sync_tcp_ports(&[8002]).is_err());
        assert_eq!(host.count("/ports"), port_requests);
        assert_eq!(host.count("/release"), releases);
        if !restart {
            assert_eq!(*host.leases.lock().unwrap(), accepted);
        }
        host.reject_replay.store(false, Ordering::Release);
        // An update accepted during recovery replaces the next replay's declaration.
        app_runtime::sync_nftables(&[lease("current")]).unwrap();
        wait_for(|| *availability.borrow() == ResourceAvailability::Available);
        wait_for(|| ticks.load(Ordering::Relaxed) > paused);
        assert_eq!(
            *host.leases.lock().unwrap(),
            json!({"leases": [lease("current")]})
        );
        assert_eq!(
            *host.ports.lock().unwrap(),
            json!({"ports": [{"port": 8000, "proto": "tcp"}]})
        );
        let requests = host.requests.lock().unwrap();
        let port_index = requests
            .iter()
            .rposition(|(path, _)| path.ends_with("/ports"))
            .unwrap();
        assert!(requests[port_index - 1].0.ends_with("/nftables"));
    }

    let (entered_tx, entered_rx) = mpsc::sync_channel(0);
    let (resume_tx, resume_rx) = mpsc::sync_channel(0);
    *host.nftables_gate.lock().unwrap() = Some((entered_tx, resume_rx));
    let update = std::thread::spawn(|| app_runtime::sync_nftables(&[lease("inflight")]));
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    // Resource loss must be visible even while a control request is blocked.
    host.disconnect();
    wait_for(|| matches!(*availability.borrow(), ResourceAvailability::Unavailable(_)));
    let release = std::thread::spawn(|| runtime::stop_and_release("lease-test"));
    wait_for(|| *availability.borrow() == ResourceAvailability::Stopping);
    resume_tx.send(()).unwrap();
    update.join().unwrap().unwrap();
    release.join().unwrap().unwrap();
    worker.join().unwrap();
    assert_eq!(*availability.borrow(), ResourceAvailability::Stopping);
    assert!(app_runtime::sync_nftables(&[])
        .unwrap_err()
        .contains("stopping"));
    assert!(app_runtime::sync_tcp_ports(&[])
        .unwrap_err()
        .contains("stopping"));
    assert_eq!(*host.leases.lock().unwrap(), Value::Null);
    assert!(host
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .0
        .ends_with("/release"));
    host.disconnect();
    host.stop.store(true, Ordering::Release);
    server.join().unwrap();
}

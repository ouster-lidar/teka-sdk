//! Host app devres client.

use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tracing::{info, warn};

use crate::app::devres::{self, NftablesBody, NftablesLease, PortBody, SyncBody};

const HOST_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const HOST_WAIT_POLL: Duration = Duration::from_secs(2);

const RUNTIME_URL_PATH: &str = "/var/lib/mission-control/runtime_url";

static STOPPING: AtomicBool = AtomicBool::new(false);
static CONTROL_REQUEST: Mutex<()> = Mutex::new(());

static PENDING_SUPERVISOR: Mutex<Option<(String, Option<SocketAddr>)>> = Mutex::new(None);

static UDP_PORTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());
static TCP_PORTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());
static NFTABLES: Mutex<Option<Vec<NftablesLease>>> = Mutex::new(None);

/// Registration loss makes resource availability uncertain until replay completes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceAvailability {
    Available,
    Unavailable(String),
    Stopping,
}

fn availability_sender() -> &'static tokio::sync::watch::Sender<ResourceAvailability> {
    static AVAILABILITY: OnceLock<tokio::sync::watch::Sender<ResourceAvailability>> =
        OnceLock::new();
    AVAILABILITY.get_or_init(|| {
        tokio::sync::watch::channel(ResourceAvailability::Unavailable(
            "app runtime has not registered".into(),
        ))
        .0
    })
}

/// Observe current availability before readiness; pause dependent work on loss or exit.
/// Like any watch channel, fast transitions may coalesce.
pub fn resource_availability() -> tokio::sync::watch::Receiver<ResourceAvailability> {
    availability_sender().subscribe()
}

fn publish_availability(state: ResourceAvailability) {
    if state == ResourceAvailability::Available {
        let _guard = CONTROL_REQUEST.lock().unwrap_or_else(|e| e.into_inner());
        set_availability(state);
    } else {
        set_availability(state);
    }
}

fn set_availability(state: ResourceAvailability) {
    availability_sender().send_if_modified(|current| {
        let state =
            if *current == ResourceAvailability::Stopping || STOPPING.load(Ordering::Acquire) {
                ResourceAvailability::Stopping
            } else {
                state
            };
        if *current == state {
            false
        } else {
            *current = state;
            true
        }
    });
}

fn wait_for_host_runtime() -> Result<(), String> {
    let deadline = std::time::Instant::now() + HOST_WAIT_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if STOPPING.load(Ordering::Acquire) {
            return Err("app runtime is stopping".into());
        }
        if app_ready_ping() {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(remaining.min(HOST_WAIT_POLL));
    }
    Err(format!(
        "timed out after {}s waiting for host app runtime at {}",
        HOST_WAIT_TIMEOUT.as_secs(),
        app_base_url()
    ))
}

fn wait_for_host_runtime_recovery() {
    while !STOPPING.load(Ordering::Acquire) && !app_ready_ping() {
        std::thread::sleep(HOST_WAIT_POLL);
    }
}

fn app_ready_ping() -> bool {
    let base = app_base_url();
    let url = format!("{}/api/v1/auth/status", base.trim_end_matches('/'));
    let Ok(client) = client() else {
        return false;
    };
    client
        .get(&url)
        .send()
        .ok()
        .is_some_and(|r| r.status().is_success())
}

pub fn app_base_url() -> String {
    if let Ok(v) = std::env::var("MC_RUNTIME_URL") {
        let u = v.trim();
        if !u.is_empty() {
            return u.to_string();
        }
    }
    if let Ok(v) = std::env::var("MC_FIREWALL_URL") {
        let u = v.trim();
        if !u.is_empty() {
            return u.to_string();
        }
    }
    if let Ok(raw) = std::fs::read_to_string(RUNTIME_URL_PATH) {
        let u = raw.trim();
        if !u.is_empty() {
            return u.to_string();
        }
    }
    "http://127.0.0.1:8080".into()
}

pub fn sensors_list_url() -> Option<String> {
    if let Ok(url) = std::env::var("MC_SENSORS_URL") {
        let url = url.trim();
        if !url.is_empty() {
            return Some(url.to_string());
        }
    }
    let peer = std::env::var("MC_SENSORS_APP_ID")
        .or_else(|_| std::env::var("MC_SENSORS_EXTENSION_ID"))
        .ok()?;
    let peer = peer.trim();
    if peer.is_empty() {
        return None;
    }
    Some(format!(
        "{}/api/v1/app/{peer}/devices",
        app_base_url().trim_end_matches('/')
    ))
}

pub fn dhcp_leases_url() -> Option<String> {
    if let Ok(url) = std::env::var("MC_DHCP_LEASES_URL") {
        let url = url.trim();
        if !url.is_empty() {
            return Some(url.to_string());
        }
    }
    if let Ok(peer) = std::env::var("MC_DHCP_LEASES_APP_ID")
        .or_else(|_| std::env::var("MC_DHCP_LEASES_EXTENSION_ID"))
    {
        let peer = peer.trim();
        if !peer.is_empty() {
            return Some(format!(
                "{}/api/v1/app/{peer}/leases",
                app_base_url().trim_end_matches('/')
            ));
        }
    }
    let path = std::env::var("MC_DHCP_LEASES_PATH").ok()?;
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Some(format!("{}{}", app_base_url().trim_end_matches('/'), path))
}

fn resolve_app_id(default_id: &str) -> String {
    std::env::var("APP_ID")
        .or_else(|_| std::env::var("EXTENSION_ID"))
        .unwrap_or_else(|_| default_id.into())
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client build failed: {e}"))
}

fn register_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .build()
        .map_err(|e| format!("HTTP client build failed: {e}"))
}

/// Start registration before dependent work without reporting systemd readiness.
pub fn start_supervisor() -> Result<(), String> {
    let _guard = CONTROL_REQUEST.lock().map_err(|e| e.to_string())?;
    ensure_running()?;
    let pending = PENDING_SUPERVISOR.lock().map_err(|e| e.to_string())?.take();
    if let Some((id, listen)) = pending {
        info!(target: "app-runtime", app_id = %id, ?listen, "control plane supervisor starting");
        spawn_control_plane_supervisor(id, listen)?;
    }
    Ok(())
}

pub fn notify_ready() {
    if let Err(error) = start_supervisor() {
        publish_availability(ResourceAvailability::Unavailable(error.clone()));
        warn!("app-runtime: {error}");
        #[cfg(target_os = "linux")]
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let _ = sd_notify::notify(&[sd_notify::NotifyState::Ready]);
    }
}

fn activate_with_hint(app_id: &str, hint: Option<SocketAddr>) -> Result<SocketAddr, String> {
    let _guard = CONTROL_REQUEST.lock().map_err(|e| e.to_string())?;
    if STOPPING.load(Ordering::Acquire) {
        return Err("app runtime is stopping".into());
    }
    let client = client()?;
    devres::post_activate(&client, &app_base_url(), app_id, hint)
}

pub fn activate_http_listen(default_app_id: &str) -> Result<SocketAddr, String> {
    let base = app_base_url();
    eprintln!("app-runtime: waiting for host app runtime at {base}...");
    wait_for_host_runtime()?;
    eprintln!("app-runtime: host app runtime ready at {base}");
    let id = resolve_app_id(default_app_id);
    let addr = activate_with_hint(&id, None)?;
    if let Ok(mut g) = PENDING_SUPERVISOR.lock() {
        *g = Some((id, Some(addr)));
    }
    Ok(addr)
}

/// Activate an app with no dynamic HTTP listener (none or fixed proxy backend).
pub fn activate_resources(default_app_id: &str) -> Result<(), String> {
    wait_for_host_runtime()?;
    let id = resolve_app_id(default_app_id);
    reactivate(&id, None)?;
    *PENDING_SUPERVISOR.lock().map_err(|e| e.to_string())? = Some((id, None));
    Ok(())
}

fn reactivate(app_id: &str, listen: Option<SocketAddr>) -> Result<(), String> {
    if let Some(listen) = listen {
        return activate_with_hint(app_id, Some(listen)).map(|_| ());
    }
    let _guard = CONTROL_REQUEST.lock().map_err(|e| e.to_string())?;
    if STOPPING.load(Ordering::Acquire) {
        return Err("app runtime is stopping".into());
    }
    devres::post_activate_resources(&client()?, &app_base_url(), app_id)
}

fn spawn_control_plane_supervisor(
    app_id: String,
    listen: Option<SocketAddr>,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name("app-control-plane".into())
        .spawn(move || control_plane_supervisor(&app_id, listen))
        .map(|_| ())
        .map_err(|error| format!("control plane supervisor thread spawn failed: {error}"))
}

fn control_plane_supervisor(app_id: &str, listen: Option<SocketAddr>) {
    while !STOPPING.load(Ordering::Acquire) {
        hold_register_stream(app_id);
        if STOPPING.load(Ordering::Acquire) {
            return;
        }
        std::thread::sleep(HOST_WAIT_POLL);
        if !app_ready_ping() {
            info!(app_id = %app_id, "waiting for host app runtime recovery");
            wait_for_host_runtime_recovery();
        }
        info!(app_id = %app_id, ?listen, "host app runtime ready; re-activating");
        loop {
            match reactivate(app_id, listen) {
                Ok(()) => {
                    info!(app_id = %app_id, ?listen, "re-activate ok");
                    break;
                }
                Err(e) => {
                    if STOPPING.load(Ordering::Acquire) {
                        return;
                    }
                    publish_availability(ResourceAvailability::Unavailable(e.clone()));
                    warn!(%app_id, error = %e, "re-activate failed");
                    std::thread::sleep(HOST_WAIT_POLL);
                }
            }
        }
        resync_resources(app_id, listen);
    }
}

fn hold_register_stream(app_id: &str) {
    let client = match register_client() {
        Ok(c) => c,
        Err(e) => {
            publish_availability(ResourceAvailability::Unavailable(e.to_string()));
            warn!(app_id = %app_id, error = %e, "register client build failed");
            return;
        }
    };
    let url = devres::register_url(&app_base_url(), app_id);
    info!(app_id = %app_id, %url, "register stream connecting");
    let mut resp = match client.get(&url).send() {
        Ok(r) => r,
        Err(e) => {
            publish_availability(ResourceAvailability::Unavailable(e.to_string()));
            warn!(app_id = %app_id, error = %e, "register stream connect failed");
            return;
        }
    };
    if !resp.status().is_success() {
        publish_availability(ResourceAvailability::Unavailable(format!(
            "register stream rejected: {}",
            resp.status()
        )));
        warn!(
            app_id = %app_id,
            status = %resp.status(),
            "register stream rejected"
        );
        return;
    }
    publish_availability(ResourceAvailability::Available);
    info!(app_id = %app_id, "register stream connected");
    let mut sink = io::sink();
    match std::io::copy(&mut resp, &mut sink) {
        Ok(n) => {
            publish_availability(ResourceAvailability::Unavailable(
                "register stream closed".into(),
            ));
            info!(app_id = %app_id, bytes = n, "register stream closed");
        }
        Err(e) => {
            publish_availability(ResourceAvailability::Unavailable(e.to_string()));
            info!(app_id = %app_id, error = %e, "register stream read error");
        }
    }
}

fn port_bodies() -> Vec<PortBody> {
    let udp = UDP_PORTS.lock().map(|g| g.clone()).unwrap_or_default();
    let tcp = TCP_PORTS.lock().map(|g| g.clone()).unwrap_or_default();
    udp.iter()
        .map(|&port| PortBody { port, proto: "udp" })
        .chain(tcp.iter().map(|&port| PortBody { port, proto: "tcp" }))
        .collect()
}

/// Replay current accepted packet policy before ports; never publish partial restoration.
fn resync_resources(default_app_id: &str, listen: Option<SocketAddr>) {
    while !STOPPING.load(Ordering::Acquire) {
        match replay_resources(default_app_id) {
            Ok(()) => break,
            Err(e) => {
                publish_availability(ResourceAvailability::Unavailable(e.clone()));
                warn!("app-runtime: {e}");
                std::thread::sleep(HOST_WAIT_POLL);
                while !STOPPING.load(Ordering::Acquire) {
                    match reactivate(default_app_id, listen) {
                        Ok(()) => break,
                        Err(e) => {
                            publish_availability(ResourceAvailability::Unavailable(e.clone()));
                            warn!("app-runtime: {e}");
                            std::thread::sleep(HOST_WAIT_POLL);
                        }
                    }
                }
            }
        }
    }
}

/// Replace native packet leases; rejected requests never change the accepted replay cache.
pub fn sync_nftables(default_app_id: &str, leases: &[NftablesLease]) -> Result<(), String> {
    let _guard = CONTROL_REQUEST.lock().map_err(|e| e.to_string())?;
    ensure_running()?;
    if let Err(error) = push_nftables(default_app_id, leases) {
        if PENDING_SUPERVISOR
            .lock()
            .map_err(|e| e.to_string())?
            .take()
            .is_some()
        {
            set_availability(ResourceAvailability::Unavailable(error.clone()));
            if let Err(cleanup) = try_release(default_app_id) {
                return Err(format!("{error}; startup release failed: {cleanup}"));
            }
        }
        return Err(error);
    }
    *NFTABLES.lock().map_err(|e| e.to_string())? = Some(leases.to_vec());
    Ok(())
}

fn push_nftables(default_app_id: &str, leases: &[NftablesLease]) -> Result<(), String> {
    let id = resolve_app_id(default_app_id);
    let url = devres::nftables_url(&app_base_url(), &id);
    let response = client()?
        .put(&url)
        .json(&NftablesBody { leases })
        .send()
        .map_err(|error| format!("PUT {url}: {error}"))?;
    let status = response.status();
    let body: devres::NftablesResponse = response
        .json()
        .map_err(|error| format!("decode nftables response from {url} (HTTP {status}): {error}"))?;
    if !status.is_success() || !body.ok {
        return Err(format!(
            "PUT {url} (HTTP {status}): {}",
            body.error
                .unwrap_or_else(|| "nftables acquisition failed".into())
        ));
    }
    Ok(())
}

fn ensure_running() -> Result<(), String> {
    if STOPPING.load(Ordering::Acquire) {
        Err("app runtime is stopping".into())
    } else {
        Ok(())
    }
}

pub(crate) fn replay_resources(default_app_id: &str) -> Result<(), String> {
    let _guard = CONTROL_REQUEST.lock().map_err(|e| e.to_string())?;
    ensure_running()?;
    push_cached_nftables(default_app_id)?;
    ensure_running()?;
    push_ports(default_app_id, &port_bodies())
}

fn push_cached_nftables(default_app_id: &str) -> Result<(), String> {
    let leases = NFTABLES.lock().map_err(|e| e.to_string())?.clone();
    if let Some(leases) = leases {
        push_nftables(default_app_id, &leases)?;
    }
    Ok(())
}

/// Synchronize UDP leases before reporting application readiness.
pub fn sync_udp_ports(default_app_id: &str, ports: &[u16]) -> Result<(), String> {
    sync_protocol_ports(default_app_id, ports, "udp", &UDP_PORTS)
}

/// Synchronize public TCP ports before reporting application readiness.
pub fn sync_tcp_ports(default_app_id: &str, ports: &[u16]) -> Result<(), String> {
    sync_protocol_ports(default_app_id, ports, "tcp", &TCP_PORTS)
}

fn sync_protocol_ports(
    default_app_id: &str,
    ports: &[u16],
    proto: &'static str,
    accepted: &Mutex<Vec<u16>>,
) -> Result<(), String> {
    let _guard = CONTROL_REQUEST.lock().map_err(|e| e.to_string())?;
    ensure_running()?;
    push_cached_nftables(default_app_id)?;
    ensure_running()?;
    let ports: Vec<u16> = ports.iter().copied().filter(|&port| port > 0).collect();
    let mut bodies = port_bodies();
    bodies.retain(|body| body.proto != proto);
    bodies.extend(ports.iter().map(|&port| PortBody { port, proto }));
    push_ports(default_app_id, &bodies)?;
    *accepted.lock().map_err(|e| e.to_string())? = ports;
    Ok(())
}

fn push_ports(default_app_id: &str, ports: &[PortBody]) -> Result<(), String> {
    let client = client()?;
    let id = resolve_app_id(default_app_id);
    let url = devres::ports_url(&app_base_url(), &id);
    client
        .put(&url)
        .json(&SyncBody { ports })
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map(|_| ())
        .map_err(|e| format!("PUT {url}: {e}"))
}

/// Prevent late re-activation or port replay from racing final release.
pub fn stop_and_release(default_app_id: &str) -> Result<(), String> {
    STOPPING.store(true, Ordering::Release);
    set_availability(ResourceAvailability::Stopping);
    let _guard = CONTROL_REQUEST.lock().unwrap_or_else(|e| e.into_inner());
    try_release(default_app_id)
}

pub(crate) fn release(default_app_id: &str) {
    let _guard = CONTROL_REQUEST.lock().unwrap_or_else(|e| e.into_inner());
    PENDING_SUPERVISOR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    set_availability(ResourceAvailability::Unavailable(
        "app runtime released".into(),
    ));
    if let Err(e) = try_release(default_app_id) {
        warn!("app-runtime: {e}");
    }
}

fn try_release(default_app_id: &str) -> Result<(), String> {
    let client = client()?;
    let id = resolve_app_id(default_app_id);
    let url = devres::release_url(&app_base_url(), &id);
    client
        .post(&url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map(|_| ())
        .map_err(|e| format!("POST {url}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    #[test]
    fn reconnect_retry_uses_current_ports() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        std::env::set_var(
            "MC_RUNTIME_URL",
            format!("http://{}", listener.local_addr().unwrap()),
        );
        *UDP_PORTS.lock().unwrap() = vec![7502];
        let host = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for status in ["409 Conflict", "200 OK", "200 OK"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
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
                requests.push(serde_json::from_slice::<serde_json::Value>(&body).unwrap());
                if status.starts_with("409") {
                    *UDP_PORTS.lock().unwrap() = vec![7503];
                }
                let body = "{\"ok\":true}";
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            requests
        });
        resync_resources("test-app", None);
        let requests = host.join().unwrap();
        assert_eq!(
            requests[0],
            serde_json::json!({"ports": [{"port": 7502, "proto": "udp"}]})
        );
        assert_eq!(requests[1], serde_json::json!({}));
        assert_eq!(
            requests[2],
            serde_json::json!({"ports": [{"port": 7503, "proto": "udp"}]})
        );
    }
}

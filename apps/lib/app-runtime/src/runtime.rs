//! Host app devres client.

use std::io;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use tracing::{info, warn};

use crate::devres::{self, PortBody, SyncBody};

const HOST_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const HOST_WAIT_POLL: Duration = Duration::from_secs(2);

const RUNTIME_URL_PATH: &str = "/var/lib/mission-control/runtime_url";

static PENDING_SUPERVISOR: Mutex<Option<(String, SocketAddr)>> = Mutex::new(None);

enum RegisterHold {
    /// Stream ended while the host was up (control plane lost).
    Disconnected,
    /// Could not open the register stream; retry without releasing devres.
    Retry,
}

static UDP_PORTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());
static TCP_PORTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());

fn wait_for_host_runtime() -> Result<(), String> {
    let deadline = std::time::Instant::now() + HOST_WAIT_TIMEOUT;
    while std::time::Instant::now() < deadline {
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
    while !app_ready_ping() {
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

pub fn notify_ready() {
    #[cfg(target_os = "linux")]
    {
        let _ = sd_notify::notify(&[sd_notify::NotifyState::Ready]);
    }
    let pending = PENDING_SUPERVISOR
        .lock()
        .ok()
        .and_then(|mut g| g.take());
    if let Some((id, listen)) = pending {
        info!(target: "app-runtime", app_id = %id, %listen, "control plane supervisor starting");
        spawn_control_plane_supervisor(id, listen);
    }
}

fn activate_with_hint(app_id: &str, hint: Option<SocketAddr>) -> Result<SocketAddr, String> {
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
        *g = Some((id, addr));
    }
    Ok(addr)
}

fn spawn_control_plane_supervisor(app_id: String, listen: SocketAddr) {
    let id_for_err = app_id.clone();
    if std::thread::Builder::new()
        .name("app-control-plane".into())
        .spawn(move || control_plane_supervisor(&app_id, listen))
        .is_err()
    {
        warn!(app_id = %id_for_err, "control plane supervisor thread spawn failed");
    }
}

fn control_plane_supervisor(app_id: &str, listen: SocketAddr) {
    loop {
        match hold_register_stream(app_id) {
            RegisterHold::Retry => {
                warn!(app_id = %app_id, "register stream retry");
                std::thread::sleep(HOST_WAIT_POLL);
                continue;
            }
            RegisterHold::Disconnected => {
                if app_ready_ping() {
                    info!(
                        app_id = %app_id,
                        %listen,
                        "register stream ended; re-activating devres with host"
                    );
                    loop {
                        match activate_with_hint(app_id, Some(listen)) {
                            Ok(_) => break,
                            Err(e) => {
                                warn!(
                                    app_id = %app_id,
                                    error = %e,
                                    "re-activate after register disconnect failed"
                                );
                                std::thread::sleep(HOST_WAIT_POLL);
                            }
                        }
                    }
                    resync_ports_if_any(app_id);
                    continue;
                }
                info!(
                    app_id = %app_id,
                    "host app runtime unreachable; releasing runtime"
                );
                release(app_id);
            }
        }
        info!(app_id = %app_id, "waiting for host app runtime recovery");
        wait_for_host_runtime_recovery();
        info!(app_id = %app_id, %listen, "host app runtime ready; re-activating");
        loop {
            match activate_with_hint(app_id, Some(listen)) {
                Ok(addr) => {
                    info!(app_id = %app_id, %addr, "re-activate ok");
                    break;
                }
                Err(e) => {
                    warn!(%app_id, error = %e, "re-activate failed");
                    std::thread::sleep(HOST_WAIT_POLL);
                }
            }
        }
        resync_ports_if_any(app_id);
    }
}

fn hold_register_stream(app_id: &str) -> RegisterHold {
    let client = match register_client() {
        Ok(c) => c,
        Err(e) => {
            warn!(app_id = %app_id, error = %e, "register client build failed");
            return RegisterHold::Retry;
        }
    };
    let url = devres::register_url(&app_base_url(), app_id);
    info!(app_id = %app_id, %url, "register stream connecting");
    let mut resp = match client.get(&url).send() {
        Ok(r) => r,
        Err(e) => {
            warn!(app_id = %app_id, error = %e, "register stream connect failed");
            return RegisterHold::Retry;
        }
    };
    if !resp.status().is_success() {
        warn!(
            app_id = %app_id,
            status = %resp.status(),
            "register stream rejected"
        );
        return RegisterHold::Retry;
    }
    info!(app_id = %app_id, "register stream connected");
    let mut sink = io::sink();
    match std::io::copy(&mut resp, &mut sink) {
        Ok(n) => {
            info!(app_id = %app_id, bytes = n, "register stream closed");
            RegisterHold::Disconnected
        }
        Err(e) => {
            info!(app_id = %app_id, error = %e, "register stream read error");
            RegisterHold::Disconnected
        }
    }
}

fn port_bodies() -> Vec<PortBody> {
    let udp = UDP_PORTS.lock().map(|g| g.clone()).unwrap_or_default();
    let tcp = TCP_PORTS.lock().map(|g| g.clone()).unwrap_or_default();
    udp.iter()
        .map(|&port| PortBody {
            port,
            proto: "udp",
        })
        .chain(tcp.iter().map(|&port| PortBody {
            port,
            proto: "tcp",
        }))
        .collect()
}

fn push_ports(default_app_id: &str) {
    sync_ports(default_app_id, &port_bodies());
}

/// Re-push holder ports after supervisor re-activate; skip when the app never registered any.
fn resync_ports_if_any(default_app_id: &str) {
    let bodies = port_bodies();
    if !bodies.is_empty() {
        sync_ports(default_app_id, &bodies);
    }
}

pub fn sync_udp_ports(default_app_id: &str, ports: &[u16]) {
    if let Ok(mut g) = UDP_PORTS.lock() {
        *g = ports
            .iter()
            .copied()
            .filter(|&p| p > 0)
            .collect();
    }
    push_ports(default_app_id);
}

pub fn sync_tcp_ports(default_app_id: &str, ports: &[u16]) {
    if let Ok(mut g) = TCP_PORTS.lock() {
        *g = ports
            .iter()
            .copied()
            .filter(|&p| p > 0)
            .collect();
    }
    push_ports(default_app_id);
}

fn sync_ports(default_app_id: &str, ports: &[PortBody]) {
    let client = match client() {
        Ok(c) => c,
        Err(e) => {
            warn!("app-runtime: {e}");
            return;
        }
    };
    let id = resolve_app_id(default_app_id);
    let url = devres::ports_url(&app_base_url(), &id);
    let payload = SyncBody { ports };
    match client.put(&url).json(&payload).send() {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => warn!("app-runtime: PUT {} → {}", url, r.status()),
        Err(e) => warn!("app-runtime: PUT {url}: {e}"),
    }
}

pub fn release(default_app_id: &str) {
    let client = match client() {
        Ok(c) => c,
        Err(_) => return,
    };
    let id = resolve_app_id(default_app_id);
    let url = devres::release_url(&app_base_url(), &id);
    if let Err(e) = client.post(&url).send() {
        warn!("app-runtime: POST {url}: {e}");
    }
}

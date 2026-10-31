//! Backend subscription to Teka's sensor roster.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::display_config;
use crate::source_config::{self, LoadedSourceConfig, PersistedSourceConfig};
use crate::ui_state;
use crate::AppState;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const DISCOVERY_WINDOW: Duration = Duration::from_secs(30);
const COUNTDOWN_TICK: Duration = Duration::from_secs(1);
const RECONNECT_BACKOFF: Duration = Duration::from_secs(2);
const STALE_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SensorCandidate {
    pub id: String,
    pub hostname: String,
    pub target: String,
    pub serial: String,
    pub model: String,
    pub firmware: String,
    pub status: String,
    #[serde(default)]
    pub targets_this_device: bool,
}

impl SensorCandidate {
    fn label(&self) -> &str {
        if self.hostname.is_empty() {
            &self.target
        } else {
            &self.hostname
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverySnapshot {
    pub status: String,
    pub message: String,
    pub candidates: Vec<SensorCandidate>,
    pub picker_open: bool,
}

impl Default for DiscoverySnapshot {
    fn default() -> Self {
        Self {
            status: "idle".into(),
            message: String::new(),
            candidates: Vec::new(),
            picker_open: false,
        }
    }
}

#[derive(Default)]
struct DiscoveryData {
    snapshot: DiscoverySnapshot,
    latest: Vec<SensorCandidate>,
}

#[derive(Clone, Default)]
pub struct SensorDiscovery {
    data: Arc<RwLock<DiscoveryData>>,
    picker_count: Arc<AtomicUsize>,
    selection_resumed: Arc<tokio::sync::Notify>,
}

pub struct PickerLease(SensorDiscovery);

impl Drop for PickerLease {
    fn drop(&mut self) {
        self.0.end_picker();
    }
}

impl SensorDiscovery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> DiscoverySnapshot {
        self.data.read().snapshot.clone()
    }

    pub fn latest_candidates(&self) -> Vec<SensorCandidate> {
        self.data.read().latest.clone()
    }

    fn clear_roster(&self) {
        let mut data = self.data.write();
        data.latest.clear();
        if self.picker_count.load(Ordering::Acquire) == 0 {
            data.snapshot.candidates.clear();
        }
    }

    pub fn picker_active(&self) -> bool {
        self.picker_count.load(Ordering::Acquire) > 0
    }

    pub fn begin_picker(&self) -> PickerLease {
        self.picker_count.fetch_add(1, Ordering::AcqRel);
        let mut data = self.data.write();
        data.snapshot.picker_open = true;
        PickerLease(self.clone())
    }

    fn end_picker(&self) {
        if self.picker_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            let mut data = self.data.write();
            data.snapshot.picker_open = false;
            data.snapshot.candidates = data.latest.clone();
            self.selection_resumed.notify_one();
        }
    }

    fn set_status(&self, status: &str, message: impl Into<String>) {
        let mut data = self.data.write();
        data.snapshot.status = status.to_string();
        data.snapshot.message = message.into();
    }

    fn replace_roster(&self, candidates: Vec<SensorCandidate>) {
        let mut data = self.data.write();
        data.latest = candidates;
        if self.picker_count.load(Ordering::Acquire) == 0 {
            data.snapshot.candidates = data.latest.clone();
        }
    }
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        discovery_loop(state).await;
    });
}

async fn discovery_loop(state: AppState) {
    let mut shutdown = state.shutdown.subscribe();
    loop {
        if *shutdown.borrow() {
            return;
        }
        let Some(list_url) = crate::app_runtime::sensors_list_url() else {
            publish_discovery_status(
                &state,
                "unavailable",
                "Teka sensor roster is not configured",
            )
            .await;
            wait_for_change(&state, &mut shutdown).await;
            continue;
        };
        let Some(url) = roster_stream_url(&list_url) else {
            publish_discovery_status(&state, "unavailable", "Invalid sensor roster URL").await;
            wait_for_change(&state, &mut shutdown).await;
            continue;
        };

        publish_discovery_status(&state, "discovering", "Connecting to sensor discovery").await;
        let connected = tokio::select! {
            _ = shutdown.changed() => return,
            result = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(&url)) => result,
        };
        let (mut socket, _) = match connected {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => {
                warn!(%error, "sensor roster WebSocket connection failed");
                publish_discovery_status(
                    &state,
                    "unavailable",
                    "Sensor discovery is unavailable; retrying",
                )
                .await;
                reconnect_wait(&state, &mut shutdown).await;
                continue;
            }
            Err(_) => {
                publish_discovery_status(
                    &state,
                    "unavailable",
                    "Sensor discovery connection timed out; retrying",
                )
                .await;
                reconnect_wait(&state, &mut shutdown).await;
                continue;
            }
        };
        info!(%url, "sensor roster WebSocket connected");
        let reconnect = roster_stream(&state, &mut socket, &mut shutdown).await;
        if reconnect {
            publish_discovery_status(
                &state,
                "unavailable",
                "Sensor discovery is unavailable; retrying",
            )
            .await;
            reconnect_wait(&state, &mut shutdown).await;
        } else {
            ui_state::publish_ui(&state);
        }
    }
}

/// Returns true when the stream ended and should reconnect.
async fn roster_stream(
    state: &AppState,
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
) -> bool {
    let mut generation = state.source_generation.load(Ordering::Acquire);
    let mut window_deadline = None;
    let mut window_elapsed = false;
    let remaining = |deadline: Option<Instant>| {
        deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()))
    };
    loop {
        let current_generation = state.source_generation.load(Ordering::Acquire);
        if current_generation != generation {
            generation = current_generation;
            window_elapsed = false;
            window_deadline = window_deadline.map(|_| Instant::now() + DISCOVERY_WINDOW);
        }
        tokio::select! {
            _ = state.source_changed.notified() => {
                consider_roster(state, generation, None).await;
            },
            _ = state.sensor_discovery.selection_resumed.notified() => {
                consider_roster(state, generation, remaining(window_deadline)).await;
            }
            _ = tokio::time::sleep(COUNTDOWN_TICK), if !window_elapsed && window_deadline.is_some() => {
                consider_roster(state, generation, remaining(window_deadline)).await;
            }
            _ = shutdown.changed() => return false,
            _ = async {
                if let Some(deadline) = window_deadline {
                    tokio::time::sleep_until(deadline).await;
                } else {
                    std::future::pending::<()>().await;
                }
            }, if !window_elapsed && window_deadline.is_some() => {
                window_elapsed = true;
                consider_roster(state, generation, Some(Duration::ZERO)).await;
            }
            message = tokio::time::timeout(STREAM_IDLE_TIMEOUT, socket.next()) => {
                match message {
                    Ok(Some(Ok(Message::Text(text)))) => match parse_snapshot(text.as_str()) {
                        Ok(Some(candidates)) => {
                            state.sensor_discovery.replace_roster(candidates.clone());
                            if !window_elapsed {
                                window_deadline.get_or_insert_with(|| Instant::now() + DISCOVERY_WINDOW);
                            }
                            consider_roster(state, generation, remaining(window_deadline)).await;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            warn!(%error, "invalid sensor roster snapshot");
                            state.sensor_discovery.clear_roster();
                            publish_discovery_status(state, "unavailable", "Sensor discovery returned invalid data").await;
                            return true;
                        }
                    },
                    Ok(Some(Ok(Message::Ping(payload)))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            return true;
                        }
                    }
                    Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Err(_) => return true,
                    Ok(Some(Err(error))) => {
                        warn!(%error, "sensor roster WebSocket read failed");
                        return true;
                    }
                    Ok(Some(Ok(_))) => {}
                }
            }
        }
    }
}

fn set_roster_status(
    state: &AppState,
    candidates: &[SensorCandidate],
    remaining: Option<Duration>,
    waiting: bool,
) {
    let elapsed = remaining.is_some_and(|left| left.is_zero());
    let countdown = match remaining {
        Some(left) if !left.is_zero() => format!(" ({}s)", left.as_millis().div_ceil(1000)),
        _ => String::new(),
    };
    let (status, message) = match candidates.len() {
        0 if elapsed => ("no_sensors", "No sensors detected".to_string()),
        0 => ("discovering", format!("Scanning for sensors{countdown}")),
        count if !waiting => ("ready", format!("{count} sensors detected")),
        1 => (
            "discovering",
            format!(
                "Found {}; auto-selecting unless more sensors appear{countdown}",
                candidates[0].label()
            ),
        ),
        count => (
            "selection_required",
            format!("{count} sensors detected; none stream to this device"),
        ),
    };
    publish_status(state, status, &message, waiting);
}

/// A sensor already streaming here wins immediately; a lone sensor wins once the window closes.
fn auto_pick(candidates: &[SensorCandidate], elapsed: bool) -> Option<(&SensorCandidate, &str)> {
    if let Some(candidate) = candidates.iter().find(|c| c.targets_this_device) {
        return Some((candidate, "streams to this device"));
    }
    match candidates {
        [only] if elapsed => Some((only, "only sensor found")),
        _ => None,
    }
}

async fn consider_roster(state: &AppState, generation: u64, remaining: Option<Duration>) {
    let _operation = state.source_operation.lock().await;
    if state.source_generation.load(Ordering::Acquire) != generation {
        return;
    }
    let loaded = source_config::load_details(&display_config::state_dir());
    let waiting = eligible_for_auto_selection(&loaded);
    let candidates = state.sensor_discovery.latest_candidates();
    let elapsed = remaining.is_some_and(|left| left.is_zero());
    let pick = auto_pick(&candidates, elapsed)
        .filter(|_| waiting && !state.sensor_discovery.picker_active());
    let Some((candidate, reason)) = pick else {
        set_roster_status(state, &candidates, remaining, waiting);
        return;
    };
    let message = format!("Auto-configuring {} ({reason})", candidate.label());
    if let Err(error) = apply_target(state, loaded.config, &candidate.target, &message) {
        publish_status(
            state,
            "error",
            &format!("Could not select discovered sensor: {error}"),
            true,
        );
    }
}

fn eligible_for_auto_selection(loaded: &LoadedSourceConfig) -> bool {
    loaded.error.is_none()
        && loaded.config.source == source_config::SOURCE_SENSOR
        && loaded.config.source_enabled
        && loaded.config.sensor_hostname.is_empty()
}

fn apply_target(
    state: &AppState,
    mut cfg: PersistedSourceConfig,
    target: &str,
    message: &str,
) -> Result<(), String> {
    cfg.source = source_config::SOURCE_SENSOR.to_string();
    cfg.sensor_hostname = target.to_string();
    cfg.source_enabled = true;
    cfg.record_enabled = false;
    let state_dir = display_config::state_dir();
    source_config::save(&state_dir, &cfg)?;
    state.source_generation.fetch_add(1, Ordering::AcqRel);
    state.source_changed.notify_one();
    publish_status(state, "configuring", message, true);
    Ok(())
}

async fn publish_discovery_status(state: &AppState, status: &str, message: &str) {
    let _operation = state.source_operation.lock().await;
    let waiting =
        eligible_for_auto_selection(&source_config::load_details(&display_config::state_dir()));
    publish_status(state, status, message, waiting);
}

fn publish_status(state: &AppState, status: &str, message: &str, update_runtime: bool) {
    state.sensor_discovery.set_status(status, message);
    if update_runtime {
        if let Err(error) = source_config::set_runtime_status(status, message) {
            warn!(%error, "could not publish source status");
        }
    }
    ui_state::publish_ui(state);
}

async fn wait_for_change(state: &AppState, shutdown: &mut tokio::sync::watch::Receiver<bool>) {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(1)) => {},
        _ = state.source_changed.notified() => {},
        _ = shutdown.changed() => {},
    }
}

async fn reconnect_wait(state: &AppState, shutdown: &mut tokio::sync::watch::Receiver<bool>) {
    tokio::select! {
        _ = tokio::time::sleep(RECONNECT_BACKOFF) => {},
        _ = state.source_changed.notified() => {},
        _ = shutdown.changed() => {},
    }
}

pub fn roster_stream_url(list_url: &str) -> Option<String> {
    let (scheme, rest) = list_url.split_once("://")?;
    let ws_scheme = match scheme {
        "http" => "ws",
        "https" => "wss",
        _ => return None,
    };
    let (path, query) = rest
        .split_once('?')
        .map(|(path, query)| (path, Some(query)))
        .unwrap_or((rest, None));
    let mut url = format!("{ws_scheme}://{}/stream", path.trim_end_matches('/'));
    if let Some(query) = query {
        url.push('?');
        url.push_str(query);
    }
    Some(url)
}

pub fn parse_snapshot(text: &str) -> Result<Option<Vec<SensorCandidate>>, String> {
    let message: Value = serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))?;
    if message.get("type").and_then(Value::as_str) != Some("snapshot") {
        return Ok(None);
    }
    let devices = message
        .get("data")
        .and_then(|data| data.get("devices"))
        .and_then(Value::as_array)
        .ok_or_else(|| "snapshot is missing data.devices".to_string())?;
    let mut unique = BTreeMap::new();
    for device in devices {
        if let Some(candidate) = candidate_from_device(device) {
            unique
                .entry(candidate.id.clone())
                .and_modify(|existing: &mut SensorCandidate| {
                    let merged = existing.targets_this_device || candidate.targets_this_device;
                    if candidate.target < existing.target {
                        *existing = candidate.clone();
                    }
                    existing.targets_this_device = merged;
                })
                .or_insert(candidate);
        }
    }
    Ok(Some(unique.into_values().collect()))
}

fn candidate_from_device(device: &Value) -> Option<SensorCandidate> {
    if !device.is_object() || is_stale(device) {
        return None;
    }
    let hostname = string_field(device, "hostname");
    let serial = string_field(device, "serial");
    let id = if !serial.is_empty() {
        format!("serial:{}", serial.to_ascii_lowercase())
    } else if !hostname.is_empty() {
        format!("host:{}", hostname.to_ascii_lowercase())
    } else {
        return None;
    };
    let addresses = address_values(device.get("addresses").or_else(|| device.get("ips")));
    let targets_this = targets_this_device(device, &addresses);
    let target = addresses.first().cloned().or_else(|| {
        let host = hostname.trim();
        if host.is_empty() || host.to_ascii_lowercase().ends_with(".local") {
            None
        } else {
            Some(host.to_string())
        }
    })?;
    Some(SensorCandidate {
        id,
        hostname,
        target,
        serial,
        model: string_field(device, "model"),
        firmware: string_field(device, "firmware"),
        status: string_field(device, "status"),
        targets_this_device: targets_this,
    })
}

fn targets_this_device(device: &Value, addresses: &[String]) -> bool {
    let Some(dest) = device
        .pointer("/udp_dest/host")
        .and_then(Value::as_str)
        .and_then(|host| IpAddr::from_str(host.trim()).ok())
    else {
        return false;
    };
    addresses
        .iter()
        .filter_map(|address| IpAddr::from_str(address).ok())
        .any(|ip| local_route_ip(ip) == Some(dest))
}

/// Source address the kernel would use to reach `ip`; connecting UDP sends nothing.
fn local_route_ip(ip: IpAddr) -> Option<IpAddr> {
    let unspecified = match ip {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    let socket = UdpSocket::bind((unspecified, 0)).ok()?;
    socket.connect((ip, 7502)).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn address_values(value: Option<&Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let values: Vec<&Value> = match value {
        Value::Array(values) => values.iter().collect(),
        Value::String(_) => vec![value],
        _ => Vec::new(),
    };
    let mut addresses: Vec<String> = values
        .into_iter()
        .filter_map(|value| {
            let address = value
                .as_str()
                .or_else(|| value.get("ip").and_then(Value::as_str))
                .or_else(|| value.get("address").and_then(Value::as_str))?;
            let address = address.trim();
            let (ip, scope) = address
                .split_once('%')
                .map(|(ip, scope)| (ip, Some(scope)))
                .unwrap_or((address, None));
            if scope.is_some_and(str::is_empty) || IpAddr::from_str(ip).is_err() {
                return None;
            }
            Some(address.to_string())
        })
        .collect();
    addresses.sort_by_key(|address| (address.contains(':'), address.clone()));
    addresses.dedup();
    addresses
}

fn is_stale(device: &Value) -> bool {
    let status = string_field(device, "status").to_ascii_lowercase();
    if ["offline", "stale", "lost", "disconnected", "unreachable"]
        .iter()
        .any(|value| status == *value)
    {
        return true;
    }
    let Some(last_seen) = device.get("last_seen").and_then(last_seen_seconds) else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    last_seen < now - STALE_AFTER.as_secs() as i64
}

fn last_seen_seconds(value: &Value) -> Option<i64> {
    if let Some(number) = value
        .as_i64()
        .or_else(|| value.as_str()?.parse::<i64>().ok())
    {
        return Some(if number > 10_000_000_000 {
            number / 1000
        } else {
            number
        });
    }
    if let Some(number) = value.as_u64() {
        let number = i64::try_from(number).ok()?;
        return Some(if number > 10_000_000_000 {
            number / 1000
        } else {
            number
        });
    }
    parse_rfc3339(value.as_str()?)
}

fn parse_rfc3339(value: &str) -> Option<i64> {
    let (date, time) = value.split_once('T').or_else(|| value.split_once(' '))?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<i64>().ok()?;
    let day = date_parts.next()?.parse::<i64>().ok()?;
    let date_days = days_from_civil(year, month, day);
    let time_end = time.find(['Z', '+', '-']).unwrap_or(time.len());
    let clock = time.get(..time_end)?;
    let mut clock_parts = clock.split(':');
    let hour = clock_parts.next()?.parse::<i64>().ok()?;
    let minute = clock_parts.next()?.parse::<i64>().ok()?;
    let seconds = clock_parts.next()?.split('.').next()?.parse::<i64>().ok()?;
    let offset = match time.get(time_end..)?.chars().next()? {
        'Z' => 0,
        sign @ ('+' | '-') => {
            let offset = time.get(time_end + 1..)?;
            let (hours, minutes) = offset.split_once(':')?;
            let seconds =
                hours.parse::<i64>().ok()? * 3600 + minutes.get(..2)?.parse::<i64>().ok()? * 60;
            if sign == '+' {
                seconds
            } else {
                -seconds
            }
        }
        _ => return None,
    };
    Some(date_days * 86_400 + hour * 3600 + minute * 60 + seconds - offset)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::{
        auto_pick, eligible_for_auto_selection, parse_snapshot, roster_stream_url, SensorCandidate,
        SensorDiscovery,
    };
    use crate::source_config::{LoadedSourceConfig, PersistedSourceConfig};

    #[test]
    fn automatic_selection_requires_an_enabled_unconfigured_sensor_source() {
        let mut loaded = LoadedSourceConfig {
            config: PersistedSourceConfig::default(),
            error: None,
        };
        assert!(eligible_for_auto_selection(&loaded));
        loaded.config.sensor_hostname = "os-1.local".into();
        assert!(!eligible_for_auto_selection(&loaded));
        loaded.config.sensor_hostname.clear();
        loaded.config.source_enabled = false;
        assert!(!eligible_for_auto_selection(&loaded));
        loaded.config.source_enabled = true;
        loaded.config.source = "local_file".into();
        assert!(!eligible_for_auto_selection(&loaded));
        loaded.config.source = "sensor".into();
        loaded.error = Some("invalid configuration".into());
        assert!(!eligible_for_auto_selection(&loaded));
    }

    #[test]
    fn roster_url_maps_http_schemes_and_adds_stream() {
        assert_eq!(
            roster_stream_url("http://mc/api/v1/app/sensors/devices").as_deref(),
            Some("ws://mc/api/v1/app/sensors/devices/stream")
        );
        assert_eq!(
            roster_stream_url("https://mc/api/v1/app/sensors/devices").as_deref(),
            Some("wss://mc/api/v1/app/sensors/devices/stream")
        );
    }

    #[test]
    fn snapshot_replaces_and_deduplicates_candidates_by_sensor_identity() {
        let json = r#"{"type":"snapshot","data":{"devices":[
            {"hostname":"os-1.local","addresses":["10.0.0.5"],"serial":"S-1","model":"OS-1","firmware":"3.1","status":"online"},
            {"hostname":"os-1.local","addresses":["10.0.0.4"],"serial":"S-1","model":"OS-1","firmware":"3.1","status":"online"},
            {"hostname":"stale.local","addresses":["10.0.0.8"],"serial":"S-2","status":"offline"},
            {"hostname":"no-address.local","addresses":[],"serial":"S-3","status":"online"}
        ]}}"#;
        let candidates = parse_snapshot(json).unwrap().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "serial:s-1");
        assert_eq!(candidates[0].target, "10.0.0.4");
    }

    #[test]
    fn sensor_target_prefers_ipv4_and_preserves_scoped_ipv6() {
        let json = r#"{"type":"snapshot","data":{"devices":[
            {"hostname":"os-1.local","addresses":["fe80::5%eth0","192.0.2.5"],"serial":"S-1","status":"online"}
        ]}}"#;
        let candidates = parse_snapshot(json).unwrap().unwrap();
        assert_eq!(candidates[0].target, "192.0.2.5");

        let json = r#"{"type":"snapshot","data":{"devices":[
            {"hostname":"os-1.local","addresses":["fe80::5%eth0"],"serial":"S-1","status":"online"}
        ]}}"#;
        let candidates = parse_snapshot(json).unwrap().unwrap();
        assert_eq!(candidates[0].target, "fe80::5%eth0");
    }

    #[test]
    fn roster_timestamps_accept_mission_control_unix_strings() {
        for last_seen in [
            serde_json::json!(0),
            serde_json::json!("0"),
            serde_json::json!("1970-01-01T00:00:00Z"),
        ] {
            assert!(super::is_stale(
                &serde_json::json!({ "last_seen": last_seen })
            ));
        }
        assert_eq!(
            super::last_seen_seconds(&serde_json::json!("1700000000000")),
            Some(1700000000)
        );
    }

    #[test]
    fn udp_dest_host_matching_local_route_marks_targets_this_device() {
        let json = r#"{"type":"snapshot","data":{"devices":[
            {"hostname":"a.local","addresses":["127.0.0.1"],"serial":"S-1","status":"online","udp_dest":{"host":"127.0.0.1"}},
            {"hostname":"b.local","addresses":["127.0.0.1"],"serial":"S-2","status":"online","udp_dest":{"host":"192.0.2.77"}},
            {"hostname":"c.local","addresses":["127.0.0.1"],"serial":"S-3","status":"online","udp_dest":{"host":"@auto"}},
            {"hostname":"d.local","addresses":["127.0.0.1"],"serial":"S-4","status":"online"}
        ]}}"#;
        let candidates = parse_snapshot(json).unwrap().unwrap();
        let by_id = |id: &str| {
            candidates
                .iter()
                .find(|candidate| candidate.id == id)
                .unwrap()
                .targets_this_device
        };
        assert!(by_id("serial:s-1"));
        assert!(!by_id("serial:s-2"));
        assert!(!by_id("serial:s-3"));
        assert!(!by_id("serial:s-4"));
    }

    #[test]
    fn picker_freezes_published_roster_and_releases_on_drop() {
        let discovery = SensorDiscovery::new();
        let candidate = SensorCandidate {
            id: "serial:s-1".into(),
            hostname: "os-1.local".into(),
            target: "192.0.2.1".into(),
            serial: "S-1".into(),
            model: "OS-1".into(),
            firmware: "3.1".into(),
            status: "online".into(),
            targets_this_device: false,
        };
        discovery.replace_roster(vec![candidate.clone()]);
        let lease = discovery.begin_picker();
        discovery.clear_roster();
        assert!(discovery.snapshot().picker_open);
        assert_eq!(discovery.snapshot().candidates, vec![candidate]);
        assert!(discovery.latest_candidates().is_empty());
        drop(lease);
        assert!(!discovery.snapshot().picker_open);
        assert!(discovery.snapshot().candidates.is_empty());
    }

    #[test]
    fn auto_pick_prefers_this_device_then_lone_sensor_after_window() {
        let sensor = |id: &str, targets_this_device| SensorCandidate {
            id: id.into(),
            hostname: format!("{id}.local"),
            target: id.into(),
            serial: String::new(),
            model: String::new(),
            firmware: String::new(),
            status: String::new(),
            targets_this_device,
        };
        let pick = |candidates: &[SensorCandidate], elapsed| {
            auto_pick(candidates, elapsed).map(|(c, _)| c.id.clone())
        };
        let lone = [sensor("a", false)];
        assert_eq!(pick(&lone, false), None);
        assert_eq!(pick(&lone, true), Some("a".into()));
        let many = [sensor("a", false), sensor("b", false)];
        assert_eq!(pick(&many, true), None);
        let targeted = [sensor("a", false), sensor("b", true)];
        assert_eq!(pick(&targeted, false), Some("b".into()));
        assert_eq!(pick(&[], true), None);
    }
}

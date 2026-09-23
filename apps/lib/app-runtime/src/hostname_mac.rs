//! Hostname resolution for PTP clock identities (host sensors API + DHCP leases).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::runtime;

pub trait MacHostnameResolver: Send + Sync {
    fn hostname_for_mac(&self, mac: &[u8]) -> Option<String>;
}

/// Poll MC sensors and DHCP lease APIs and cache MAC → hostname.
pub struct AppHostnameResolver {
    client: Option<reqwest::blocking::Client>,
    sensors_url: String,
    dhcp_leases_url: String,
    cache: Mutex<HashMap<[u8; 6], String>>,
    last_fetch: Mutex<Option<Instant>>,
}

impl AppHostnameResolver {
    pub fn from_env() -> Self {
        let sensors_url = runtime::sensors_list_url().unwrap_or_default();
        let dhcp_leases_url = runtime::dhcp_leases_url().unwrap_or_default();
        Self::new(sensors_url, dhcp_leases_url)
    }

    pub fn new(sensors_url: String, dhcp_leases_url: String) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_millis(500))
                .build()
                .ok(),
            sensors_url,
            dhcp_leases_url,
            cache: Mutex::new(HashMap::new()),
            last_fetch: Mutex::new(None),
        }
    }

    fn refresh_if_stale(&self) {
        let mut last = self.last_fetch.lock().expect("last_fetch lock");
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        *last = Some(Instant::now());
        drop(last);

        let Some(client) = &self.client else {
            return;
        };

        let mut cache = self.cache.lock().expect("cache lock");
        cache.clear();

        if !self.sensors_url.is_empty() {
            if let Ok(resp) = client.get(&self.sensors_url).send() {
                if let Ok(body) = resp.json::<serde_json::Value>() {
                    if let Some(devices) = body.get("devices").and_then(|v| v.as_array()) {
                        for s in devices {
                            let hostname = s
                                .get("hostname")
                                .and_then(|v| v.as_str())
                                .or_else(|| s.get("name").and_then(|v| v.as_str()));
                            let mac = s.get("mac").and_then(|v| v.as_str());
                            if let (Some(host), Some(mac)) = (hostname, mac) {
                                if let Some(key) = parse_mac(mac) {
                                    cache.insert(key, host.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        if !self.dhcp_leases_url.is_empty() {
            if let Ok(resp) = client.get(&self.dhcp_leases_url).send() {
                if let Ok(leases) = resp.json::<Vec<serde_json::Value>>() {
                    for lease in leases {
                        let mac = lease.get("mac").and_then(|v| v.as_str());
                        let hostname = lease.get("hostname").and_then(|v| v.as_str());
                        if let (Some(mac), Some(host)) = (mac, hostname) {
                            if !host.is_empty() {
                                if let Some(key) = parse_mac(mac) {
                                    cache.entry(key).or_insert_with(|| host.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl MacHostnameResolver for AppHostnameResolver {
    fn hostname_for_mac(&self, mac: &[u8]) -> Option<String> {
        if mac.len() < 6 {
            return None;
        }
        self.refresh_if_stale();
        let key: [u8; 6] = mac[..6].try_into().ok()?;
        self.cache.lock().ok()?.get(&key).cloned()
    }
}

fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<u8> = s
        .split([':', '-'])
        .filter_map(|p| u8::from_str_radix(p.trim(), 16).ok())
        .take(6)
        .collect();
    if parts.len() == 6 {
        Some([parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]])
    } else {
        None
    }
}

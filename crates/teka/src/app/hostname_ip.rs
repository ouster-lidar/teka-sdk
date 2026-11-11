//! Hostname resolution for sensor UDP sources (host sensors API).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::app::runtime;

pub trait HostnameResolver: Send + Sync {
    fn hostname_for_ip(&self, ip: &str) -> Option<String>;
}

/// Poll `GET {sensors_url}` and cache hostname lookups.
pub struct AppSensorsResolver {
    client: Option<reqwest::blocking::Client>,
    sensors_url: String,
    cache: Mutex<HashMap<String, String>>,
    last_fetch: Mutex<Option<Instant>>,
}

impl AppSensorsResolver {
    pub fn new(sensors_url: String) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .ok(),
            sensors_url,
            cache: Mutex::new(HashMap::new()),
            last_fetch: Mutex::new(None),
        }
    }

    pub fn from_env() -> Self {
        let url = runtime::sensors_list_url().unwrap_or_default();
        Self::new(url)
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
        let Ok(resp) = client.get(&self.sensors_url).send() else {
            return;
        };
        let Ok(body) = resp.json::<serde_json::Value>() else {
            return;
        };
        let Some(devices) = body.get("devices").and_then(|v| v.as_array()) else {
            return;
        };
        let mut cache = self.cache.lock().expect("cache lock");
        cache.clear();
        for s in devices {
            let hostname = s
                .get("hostname")
                .and_then(|v| v.as_str())
                .or_else(|| s.get("name").and_then(|v| v.as_str()));
            let ips = s.get("ips").and_then(|v| v.as_array());
            if let (Some(host), Some(ips)) = (hostname, ips) {
                for ip in ips {
                    if let Some(ip) = ip.as_str() {
                        cache.insert(ip.to_string(), host.to_string());
                    }
                }
            }
        }
    }
}

impl HostnameResolver for AppSensorsResolver {
    fn hostname_for_ip(&self, ip: &str) -> Option<String> {
        self.refresh_if_stale();
        self.cache.lock().ok()?.get(ip).cloned()
    }
}

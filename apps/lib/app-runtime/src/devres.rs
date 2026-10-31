//! Loopback devres HTTP client.
//!
//! App backends must reach the Teka app runtime through this module (via
//! [`declare_app_runtime`](crate::declare_app_runtime)). Do not reimplement activate,
//! register, resource sync, or release in app crates.

use std::net::SocketAddr;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Deserialize)]
struct ActivateResponse {
    ok: bool,
    #[serde(rename = "httpListen")]
    http_listen: Option<HttpListen>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HttpListen {
    host: String,
    port: u16,
}

/// JSON body for `POST …/runtime/{id}/activate`.
///
/// The host accepts an empty body for compatibility, but every client
/// must send at least `{}` so `Content-Type: application/json` is always set.
#[derive(Debug, Serialize)]
pub(crate) struct ActivateRequest {
    #[serde(rename = "httpListen", skip_serializing_if = "Option::is_none")]
    http_listen: Option<ActivateListen>,
}

#[derive(Debug, Serialize)]
struct ActivateListen {
    host: String,
    port: u16,
}

#[derive(Serialize)]
pub(crate) struct PortBody {
    pub port: u16,
    pub proto: &'static str,
}

#[derive(Serialize)]
pub(crate) struct SyncBody<'a> {
    pub ports: &'a [PortBody],
}

/// A holder-local native nftables lease; set references are local to this lease.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NftablesLease {
    pub id: String,
    pub hook: String,
    pub priority: i32,
    #[serde(default)]
    pub sets: Vec<NftablesSet>,
    pub rules: Vec<Vec<serde_json::Value>>,
}

/// A lease-local set declaration using native nftables JSON units (seconds).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NftablesSet {
    pub name: String,
    #[serde(rename = "type")]
    pub set_type: String,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(rename = "gc-interval", skip_serializing_if = "Option::is_none")]
    pub gc_interval: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
}

#[derive(Deserialize)]
pub(crate) struct NftablesResponse {
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct NftablesBody<'a> {
    pub leases: &'a [NftablesLease],
}

pub(crate) fn nftables_url(base_url: &str, app_id: &str) -> String {
    format!(
        "{}/api/v1/manager/runtime/{app_id}/nftables",
        base_url.trim_end_matches('/')
    )
}

pub(crate) fn activate_url(base_url: &str, app_id: &str) -> String {
    format!(
        "{}/api/v1/manager/runtime/{app_id}/activate",
        base_url.trim_end_matches('/')
    )
}

pub(crate) fn register_url(base_url: &str, app_id: &str) -> String {
    format!(
        "{}/api/v1/manager/runtime/{app_id}/register",
        base_url.trim_end_matches('/')
    )
}

pub(crate) fn ports_url(base_url: &str, app_id: &str) -> String {
    format!(
        "{}/api/v1/manager/runtime/{app_id}/ports",
        base_url.trim_end_matches('/')
    )
}

pub(crate) fn release_url(base_url: &str, app_id: &str) -> String {
    format!(
        "{}/api/v1/manager/runtime/{app_id}/release",
        base_url.trim_end_matches('/')
    )
}

pub(crate) fn post_activate(
    client: &reqwest::blocking::Client,
    base_url: &str,
    app_id: &str,
    hint: Option<SocketAddr>,
) -> Result<SocketAddr, String> {
    post_activate_response(client, base_url, app_id, hint)?
        .ok_or_else(|| "activate missing httpListen".to_string())
}

pub(crate) fn post_activate_resources(
    client: &reqwest::blocking::Client,
    base_url: &str,
    app_id: &str,
) -> Result<(), String> {
    if post_activate_response(client, base_url, app_id, None)?.is_some() {
        return Err("resource-only activation returned httpListen".into());
    }
    Ok(())
}

fn post_activate_response(
    client: &reqwest::blocking::Client,
    base_url: &str,
    app_id: &str,
    hint: Option<SocketAddr>,
) -> Result<Option<SocketAddr>, String> {
    let url = activate_url(base_url, app_id);
    let request = ActivateRequest {
        http_listen: hint.map(|a| ActivateListen {
            host: a.ip().to_string(),
            port: a.port(),
        }),
    };
    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .map_err(|e| format!("POST {url}: {e}"))?;
    let status = resp.status();
    let raw = resp
        .text()
        .map_err(|e| format!("read activate response from {url}: {e}"))?;
    let parsed: ActivateResponse = serde_json::from_str(&raw).map_err(|e| {
        format!(
            "decode activate response from {url} (HTTP {status}): {e}; body: {}",
            truncate_for_error(&raw)
        )
    })?;
    if !status.is_success() || !parsed.ok {
        return Err(parsed.error.unwrap_or_else(|| "activate failed".into()));
    }
    parsed
        .http_listen
        .map(listen_addr_from_response)
        .transpose()
}

fn listen_addr_from_response(listen: HttpListen) -> Result<SocketAddr, String> {
    let host = if listen.host.is_empty() {
        "127.0.0.1"
    } else {
        listen.host.as_str()
    };
    SocketAddr::from_str(&format!("{host}:{}", listen.port))
        .map_err(|e| format!("invalid httpListen address: {e}"))
}

fn truncate_for_error(raw: &str) -> String {
    const MAX: usize = 256;
    let trimmed = raw.trim();
    if trimmed.len() <= MAX {
        return trimmed.to_string();
    }
    format!("{}…", &trimmed[..MAX])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nftables_declarations_accept_host_defaults() {
        let lease: NftablesLease = serde_json::from_value(serde_json::json!({
            "id": "filter", "hook": "input", "priority": -10,
            "rules": [[{"counter": null}]],
        }))
        .unwrap();
        assert!(lease.sets.is_empty());
        let set: NftablesSet = serde_json::from_value(serde_json::json!({
            "name": "sources", "type": "ipv4_addr",
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(set).unwrap(),
            serde_json::json!({
                "name": "sources", "type": "ipv4_addr", "flags": [],
            })
        );
    }

    #[test]
    fn activate_request_serializes_empty_object_without_hint() {
        let request = ActivateRequest { http_listen: None };
        assert_eq!(serde_json::to_string(&request).unwrap(), "{}");
    }

    #[test]
    fn activate_request_serializes_http_listen_hint() {
        let request = ActivateRequest {
            http_listen: Some(ActivateListen {
                host: "127.0.0.1".into(),
                port: 18001,
            }),
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"httpListen":{"host":"127.0.0.1","port":18001}}"#
        );
    }
}

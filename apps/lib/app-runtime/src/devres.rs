//! Loopback devres HTTP client.
//!
//! Portable backends must reach the host app runtime through this module (via
//! [`declare_app_runtime`](crate::declare_app_runtime)). Do not reimplement activate,
//! register, ports sync, or release in app crates.

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
    if !parsed.ok {
        return Err(parsed
            .error
            .unwrap_or_else(|| "activate failed".into()));
    }
    let listen = parsed
        .http_listen
        .ok_or_else(|| "activate missing httpListen".to_string())?;
    listen_addr_from_response(listen)
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

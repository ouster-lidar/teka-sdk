//! Guest view vs editor access (Teka principal, open-mode enforcement, guest-access file).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ::teka::app::api_error::ApiError;
use axum::extract::Extension;
use axum::extract::Request;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::display_config;

/// Teka injects this on proxied upstream requests when the browser has a session.
pub const HEADER_PRINCIPAL_ID: &str = "x-mc-principal-id";

const GUEST_ACCESS_FILE: &str = "guest-access.json";
const ENFORCEMENT_CACHE_TTL: Duration = Duration::from_secs(5);
const ENFORCEMENT_FETCH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Editor,
    Viewer,
    Denied,
}

#[derive(Serialize, Deserialize)]
struct GuestAccessFile {
    enabled: bool,
}

#[derive(Serialize)]
pub struct GuestAccessResponse {
    enabled: bool,
    editable: bool,
    guest: bool,
}

#[derive(Deserialize)]
pub struct GuestAccessBody {
    enabled: bool,
}

static ENFORCEMENT_CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

fn guest_access_path() -> PathBuf {
    display_config::state_dir().join(GUEST_ACCESS_FILE)
}

/// Guest view defaults on; only an explicit `{"enabled": false}` turns it off.
fn read_guest_access(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<GuestAccessFile>(&raw).ok())
        .is_none_or(|f| f.enabled)
}

pub fn guest_access_enabled() -> bool {
    read_guest_access(&guest_access_path())
}

fn write_guest_access(path: &Path, enabled: bool) -> Result<(), ApiError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                format!("mkdir state: {e}"),
            )
        })?;
    }
    let payload = GuestAccessFile { enabled };
    let encoded = serde_json::to_string_pretty(&payload)
        .map_err(|e| ApiError::invalid_request(format!("encode state: {e}")))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &encoded).map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("write state tmp: {e}"),
        )
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("replace state: {e}"),
        )
    })?;
    Ok(())
}

fn principal_present(headers: &HeaderMap) -> bool {
    headers
        .get(HEADER_PRINCIPAL_ID)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| !s.trim().is_empty())
}

async fn mc_enforcement_enabled() -> bool {
    if let Some((at, value)) = *ENFORCEMENT_CACHE.lock() {
        if at.elapsed() < ENFORCEMENT_CACHE_TTL {
            return value;
        }
    }
    let value = fetch_mc_enforcement().await.unwrap_or(true);
    *ENFORCEMENT_CACHE.lock() = Some((Instant::now(), value));
    value
}

/// Without a Teka password nobody has a principal, so open mode must grant control.
async fn fetch_mc_enforcement() -> Option<bool> {
    let base = crate::app_runtime::app_base_url();
    let url = format!("{}/api/v1/auth/status", base.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(ENFORCEMENT_FETCH_TIMEOUT)
        .build()
        .ok()?;
    let resp = client
        .get(&url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let body: serde_json::Value = resp.json().await.ok()?;
    body.get("enforcement")?.as_bool()
}

pub async fn resolve_access(headers: &HeaderMap) -> Access {
    if principal_present(headers) {
        return Access::Editor;
    }
    if !mc_enforcement_enabled().await {
        return Access::Editor;
    }
    if guest_access_enabled() {
        Access::Viewer
    } else {
        Access::Denied
    }
}

fn api_v1_suffix(path: &str) -> &str {
    path.strip_prefix("/api/v1")
        .map(|p| if p.is_empty() { "/" } else { p })
        .unwrap_or(path)
}

fn method_is_get(method: &Method) -> bool {
    *method == Method::GET || *method == Method::HEAD
}

/// Teka probes `/status` with an 800 ms budget before every proxied request, so
/// it must not wait on the Teka enforcement lookup. It exposes liveness only.
fn is_public(method: &Method, path: &str) -> bool {
    method_is_get(method) && path == "/status"
}

pub fn allowed(method: &Method, path: &str, access: Access) -> bool {
    let get = method_is_get(method);
    is_public(method, path)
        || match access {
            Access::Editor => true,
            Access::Viewer => {
                get && matches!(
                    path,
                    "/guest-access" | "/display/stream" | "/display/status"
                )
            }
            Access::Denied => get && path == "/guest-access",
        }
}

pub async fn gate(mut req: Request, next: Next) -> Response {
    let suffix = api_v1_suffix(req.uri().path());
    if is_public(req.method(), suffix) {
        return next.run(req).await;
    }
    let access = resolve_access(req.headers()).await;
    if !allowed(req.method(), suffix, access) {
        return ApiError::unauthorized("sign in required").into_response();
    }
    req.extensions_mut().insert(access);
    next.run(req).await
}

pub async fn get_guest_access(Extension(access): Extension<Access>) -> Json<GuestAccessResponse> {
    let enabled = read_guest_access(&guest_access_path());
    Json(GuestAccessResponse {
        enabled,
        editable: access == Access::Editor,
        guest: access != Access::Editor,
    })
}

pub async fn put_guest_access(
    Extension(access): Extension<Access>,
    Json(body): Json<GuestAccessBody>,
) -> Result<Json<GuestAccessResponse>, ApiError> {
    if access != Access::Editor {
        return Err(ApiError::unauthorized("sign in required"));
    }
    write_guest_access(&guest_access_path(), body.enabled)?;
    Ok(Json(GuestAccessResponse {
        enabled: body.enabled,
        editable: true,
        guest: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_access_defaults_on_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("how-guest-access-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(GUEST_ACCESS_FILE);
        assert!(read_guest_access(&path));
        write_guest_access(&path, false).unwrap();
        assert!(!read_guest_access(&path));
        write_guest_access(&path, true).unwrap();
        assert!(read_guest_access(&path));
        std::fs::write(&path, "garbage").unwrap();
        assert!(read_guest_access(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every `.route("…")` literal in the API sources, so new routes can't dodge the policy.
    fn declared_routes() -> Vec<String> {
        [
            include_str!("api.rs"),
            include_str!("display.rs"),
            include_str!("osf.rs"),
        ]
        .iter()
        .flat_map(|src| src.split(".route(").skip(1))
        .filter_map(|rest| {
            let rest = rest.trim_start().strip_prefix('"')?;
            Some(rest[..rest.find('"')?].to_string())
        })
        .collect()
    }

    const METHODS: [Method; 5] = [
        Method::GET,
        Method::HEAD,
        Method::POST,
        Method::PUT,
        Method::DELETE,
    ];

    #[test]
    fn every_route_is_declared() {
        let routes = declared_routes();
        assert!(routes.len() >= 15, "route scan found {routes:?}");
        for expected in ["/status", "/pty", "/osfs/{name}", "/guest-access"] {
            assert!(routes.iter().any(|r| r == expected), "missing {expected}");
        }
    }

    #[test]
    fn only_status_is_unauthenticated() {
        for path in declared_routes() {
            for method in &METHODS {
                let open = allowed(method, &path, Access::Denied);
                let expected =
                    method_is_get(method) && matches!(path.as_str(), "/status" | "/guest-access");
                assert_eq!(open, expected, "{method} {path} for signed-out, guest off");
            }
        }
    }

    #[test]
    fn guests_only_read_essentials() {
        for path in declared_routes() {
            for method in &METHODS {
                let open = allowed(method, &path, Access::Viewer);
                let expected = method_is_get(method)
                    && matches!(
                        path.as_str(),
                        "/status" | "/guest-access" | "/display/stream" | "/display/status"
                    );
                assert_eq!(open, expected, "{method} {path} for guest");
            }
        }
    }

    #[test]
    fn editors_reach_every_route() {
        for path in declared_routes() {
            for method in &METHODS {
                assert!(allowed(method, &path, Access::Editor), "{method} {path}");
            }
        }
    }

    #[test]
    fn unknown_paths_need_editor() {
        for access in [Access::Viewer, Access::Denied] {
            assert!(!allowed(&Method::GET, "/not-a-route", access));
            assert!(!allowed(&Method::GET, "/status/stream", access));
        }
    }
}

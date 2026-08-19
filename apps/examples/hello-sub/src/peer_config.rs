//! Persisted peer publisher URLs — `{state_dir}/peer-config.json`.
//!
//! State directory: `STATE_DIRECTORY` when set (systemd `StateDirectory=hello-sub`),
//! else `./tmp` for local dev.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::peer::normalize_publisher_url;

const FILE_NAME: &str = "peer-config.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedPeerConfig {
    #[serde(default)]
    pub publisher_urls: Option<Vec<String>>,
    /// Legacy single-URL field; migrated to `publisher_urls` on load/save.
    #[serde(default)]
    pub publisher_url: Option<String>,
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("STATE_DIRECTORY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tmp"))
}

pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join(FILE_NAME)
}

/// Normalize, dedupe (first wins), drop empties.
pub fn normalize_url_list(urls: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    for raw in urls {
        let u = normalize_publisher_url(&raw);
        if u.is_empty() {
            continue;
        }
        if !out.iter().any(|x| x == &u) {
            out.push(u);
        }
    }
    out
}

pub fn load(state_dir: &Path) -> Option<Vec<String>> {
    let p = path(state_dir);
    let raw = match fs::read_to_string(&p) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warn!("failed to read {}: {e}", p.display());
            return None;
        }
    };
    let cfg: PersistedPeerConfig = match serde_json::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            warn!("failed to parse {}: {e}", p.display());
            return None;
        }
    };
    let urls = if let Some(list) = cfg.publisher_urls {
        normalize_url_list(list)
    } else if let Some(one) = cfg.publisher_url {
        normalize_url_list(std::iter::once(one))
    } else {
        Vec::new()
    };
    if urls.is_empty() {
        None
    } else {
        Some(urls)
    }
}

pub fn save(state_dir: &Path, publisher_urls: &[String]) {
    let p = path(state_dir);
    if let Some(parent) = p.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            warn!("failed to create {}: {e}", parent.display());
            return;
        }
    }
    let cfg = PersistedPeerConfig {
        publisher_urls: Some(publisher_urls.to_vec()),
        publisher_url: None,
    };
    match serde_json::to_string_pretty(&cfg) {
        Ok(body) => {
            if let Err(e) = fs::write(&p, body) {
                warn!("failed to write {}: {e}", p.display());
            } else {
                info!("saved peer config to {}", p.display());
            }
        }
        Err(e) => warn!("failed to serialize peer config: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_migrates_legacy_publisher_url() {
        let dir = std::env::temp_dir().join(format!(
            "hello-sub-peer-cfg-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let body = r#"{ "publisher_url": "other-box" }"#;
        fs::write(path(&dir), body).unwrap();
        let urls = load(&dir).expect("urls");
        assert_eq!(urls, vec!["http://other-box/api/v1/app/hello-pub"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_url_list_dedupes() {
        let urls = normalize_url_list([
            "other-box".into(),
            "http://other-box/api/v1/app/hello-pub".into(),
            "second-box".into(),
        ]);
        assert_eq!(
            urls,
            vec![
                "http://other-box/api/v1/app/hello-pub",
                "http://second-box/api/v1/app/hello-pub",
            ]
        );
    }
}

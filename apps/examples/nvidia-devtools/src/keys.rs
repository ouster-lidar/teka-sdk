//! Persisted authorized_keys for root and js SSH users.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const ROOT_FILE: &str = "authorized_keys.root";
const JS_FILE: &str = "authorized_keys.js";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeysBody {
    pub root: String,
    pub js: String,
}

pub struct KeyStore {
    ssh_dir: PathBuf,
}

impl KeyStore {
    pub fn new(state_root: impl AsRef<Path>) -> Self {
        Self {
            ssh_dir: state_root.as_ref().join("ssh"),
        }
    }

    pub fn read(&self) -> io::Result<KeysBody> {
        Ok(KeysBody {
            root: read_file_or_empty(&self.ssh_dir.join(ROOT_FILE))?,
            js: read_file_or_empty(&self.ssh_dir.join(JS_FILE))?,
        })
    }

    pub fn write(&self, body: &KeysBody) -> Result<(), String> {
        let root = normalize_keys(&body.root)?;
        let js = normalize_keys(&body.js)?;
        write_atomic(&self.ssh_dir.join(ROOT_FILE), &root)?;
        write_atomic(&self.ssh_dir.join(JS_FILE), &js)?;
        Ok(())
    }
}

fn read_file_or_empty(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// Normalize and validate authorized_keys text (one key per non-empty, non-comment line).
pub fn normalize_keys(raw: &str) -> Result<String, String> {
    let mut out = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !looks_like_authorized_key(trimmed) {
            return Err(format!(
                "line {}: not a valid authorized_keys entry",
                idx + 1
            ));
        }
        out.push(trimmed.to_string());
    }
    Ok(out.join("\n"))
}

fn looks_like_authorized_key(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    let Some(kind) = parts.next() else {
        return false;
    };
    matches!(
        kind,
        "ssh-rsa"
            | "ssh-dss"
            | "ssh-ed25519"
            | "ecdsa-sha2-nistp256"
            | "ecdsa-sha2-nistp384"
            | "ecdsa-sha2-nistp521"
            | "sk-ecdsa-sha2-nistp256@openssh.com"
            | "sk-ssh-ed25519@openssh.com"
    ) && parts.next().is_some()
}

fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, format!("{content}\n")).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ed25519_line() {
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI nvidia-devtools";
        assert_eq!(normalize_keys(key).unwrap(), key);
    }

    #[test]
    fn skips_comments_and_blanks() {
        let raw = "# comment\n\nssh-ed25519 AAAA user\n";
        assert_eq!(
            normalize_keys(raw).unwrap(),
            "ssh-ed25519 AAAA user"
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(normalize_keys("not-a-key").is_err());
    }
}

#![forbid(unsafe_code)]

use crate::{
    build_check, checked_output,
    host::{sudo_shell, CommandExt},
};
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, process::Command};

pub(crate) const SETTINGS: &str = include_str!("../build_caches.json");

const INSTALL: &str = r#"set -eu
nix_dir=$1
settings=$2
main="$nix_dir/nix.conf"
custom="$nix_dir/nix.custom.conf"
fragment="$nix_dir/teka-caches.conf"
[ ! -L "$main" ] && [ ! -L "$custom" ] && [ ! -L "$fragment" ] || {
  echo 'Nix configuration is managed declaratively; add these URLs and keys through nix.settings.substituters and nix.settings.trusted-public-keys:' >&2
  printf '%s\n' "$settings" >&2
  exit 1
}
mkdir -p "$nix_dir"
temporary=$(mktemp "$nix_dir/.teka-caches.XXXXXX")
trap 'rm -f "$temporary"' EXIT
printf '%s\n' "$settings" > "$temporary"
chmod 644 "$temporary"
mv -f "$temporary" "$fragment"
target="$main"
if [ -f "$main" ] && grep -Eq '^[!]?include[[:space:]]+([^[:space:]]*/)?nix[.]custom[.]conf[[:space:]]*$' "$main"; then
  target="$custom"
fi
touch "$target"
if ! grep -Eq '^[!]?include[[:space:]]+([^[:space:]]*/)?teka-caches[.]conf[[:space:]]*$' "$target"; then
  printf '\n!include teka-caches.conf\n' >> "$target"
fi
if [ "$nix_dir" = /etc/nix ]; then
  if [ -x /bin/launchctl ]; then
    if /bin/launchctl print system/systems.determinate.nix-daemon >/dev/null 2>&1; then
      /bin/launchctl kickstart -k system/systems.determinate.nix-daemon
    else
      /bin/launchctl kickstart -k system/org.nixos.nix-daemon
    fi
  elif /bin/systemctl is-active --quiet nix-daemon.service; then
    /bin/systemctl restart nix-daemon.service
  elif ! /bin/systemctl is-active --quiet nix-daemon.socket; then
    echo 'Cache settings saved; restart the Nix daemon and rerun teka build init.' >&2
    exit 1
  fi
fi
"#;

fn system_configuration(nix: &Path) -> Result<Value> {
    let output = checked_output(
        Command::new(nix)
            .args(["config", "show", "--json"])
            .env("NIX_USER_CONF_FILES", "/dev/null")
            .env("NIX_CONFIG", ""),
    )?;
    serde_json::from_str(&output).context("read system Nix cache configuration")
}

fn configured(configuration: &Value, settings: &BTreeMap<String, Vec<String>>) -> bool {
    settings.iter().all(|(name, required)| {
        configuration[name]["value"]
            .as_array()
            .is_some_and(|values| {
                required.iter().all(|value| {
                    values
                        .iter()
                        .any(|existing| existing.as_str() == Some(value))
                })
            })
    })
}

pub(super) fn configure(nix: &Path) -> Result<()> {
    let settings: BTreeMap<String, Vec<String>> = serde_json::from_str(SETTINGS)?;
    if !configured(&system_configuration(nix)?, &settings) {
        let text = settings
            .iter()
            .map(|(name, values)| format!("extra-{name} = {}", values.join(" ")))
            .collect::<Vec<_>>()
            .join("\n");
        let directory = std::env::var_os("NIX_CONF_DIR").unwrap_or_else(|| "/etc/nix".into());
        sudo_shell(INSTALL)
            .arg("teka-caches")
            .arg(directory)
            .arg(text)
            .check_status("Nix cache configuration failed or was cancelled")?;
        ensure!(configured(&system_configuration(nix)?, &settings),
            "Nix did not load the cache URLs and signing keys; check system configuration overrides");
    }
    build_check::note("Nix caches: teka, nix-community");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn both_cache_urls_and_signing_keys_are_required() {
        let settings: BTreeMap<String, Vec<String>> = serde_json::from_str(SETTINGS).unwrap();
        let mut configuration = serde_json::json!({});
        for (name, values) in &settings {
            configuration[name] = serde_json::json!({"value": values});
        }
        assert!(configured(&configuration, &settings));
        for name in settings.keys() {
            let mut incomplete = configuration.clone();
            incomplete[name]["value"].as_array_mut().unwrap().pop();
            assert!(!configured(&incomplete, &settings));
        }
    }

    #[test]
    fn cache_configuration_preserves_settings_and_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let main = "substituters = https://cache.nixos.org\n!include nix.custom.conf\n";
        let custom = "max-jobs = 4\nextra-trusted-public-keys = other:abc\n";
        fs::write(directory.path().join("nix.conf"), main).unwrap();
        fs::write(directory.path().join("nix.custom.conf"), custom).unwrap();
        for _ in 0..2 {
            assert!(Command::new("/bin/sh")
                .args(["-c", INSTALL, "teka-caches"])
                .arg(directory.path())
                .arg("extra-substituters = https://teka.cachix.org")
                .status()
                .unwrap()
                .success());
        }
        assert_eq!(
            fs::read_to_string(directory.path().join("nix.conf")).unwrap(),
            main
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("nix.custom.conf")).unwrap(),
            format!("{custom}\n!include teka-caches.conf\n")
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("teka-caches.conf")).unwrap(),
            "extra-substituters = https://teka.cachix.org\n"
        );
    }

    #[test]
    fn cache_configuration_refuses_managed_files() {
        let directory = tempfile::tempdir().unwrap();
        let managed = directory.path().join("managed.conf");
        fs::write(&managed, "managed\n").unwrap();
        std::os::unix::fs::symlink(&managed, directory.path().join("nix.conf")).unwrap();
        let output = Command::new("/bin/sh")
            .args(["-c", INSTALL, "teka-caches"])
            .arg(directory.path())
            .arg("extra-substituters = test")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(fs::read_to_string(managed).unwrap(), "managed\n");
        assert!(!directory.path().join("teka-caches.conf").exists());
    }
}

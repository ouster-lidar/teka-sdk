use super::{Vm, SSH_PORT};
use crate::{
    checked_output,
    host::{sudo_shell, CommandExt},
    plugins::on_path,
};
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    process::Command,
};

const SYSTEM_DIR: &str = "/etc/nix";
const IDENTITY: &str = "/etc/nix/teka-build-vm/builder_ed25519";
const FRAGMENT: &str = "teka-build-vm.conf";

const INSTALL: &str = r#"set -eu
nix_dir=$1
key=$2
configuration=$3
fragment="$nix_dir/teka-build-vm.conf"
credentials="$nix_dir/teka-build-vm"
main="$nix_dir/nix.conf"
custom="$nix_dir/nix.custom.conf"
[ ! -L "$main" ] && [ ! -L "$custom" ] || {
  echo 'Nix configuration is managed declaratively; configure nix.buildMachines for teka-build-vm in your system configuration.' >&2
  exit 1
}
[ ! -L "$credentials" ] && [ ! -L "$fragment" ]
/bin/mkdir -p "$credentials"
/bin/chmod 755 "$credentials"
/usr/bin/install -m 600 "$key" "$credentials/builder_ed25519"
/usr/bin/install -m 644 "$key.pub" "$credentials/builder_ed25519.pub"
temporary=$(/usr/bin/mktemp "$nix_dir/.teka-build-vm.XXXXXX")
trap '/bin/rm -f "$temporary"' EXIT
/usr/bin/install -m 644 "$configuration" "$temporary"
/bin/mv -f "$temporary" "$fragment"
last_setting() {
  /usr/bin/awk '{sub(/[[:space:]]*#.*/, ""); if (NF) line=$0} END {print line}' "$1"
}
target="$main"
# Determinate regenerates nix.conf; its custom file keeps registration persistent.
if [ -f "$main" ] && /usr/bin/grep -Eq '^[!]?include[[:space:]]+([^[:space:]]*/)?nix[.]custom[.]conf[[:space:]]*$' "$main"; then
  target="$custom"
fi
/usr/bin/touch "$target"
if ! last_setting "$target" | /usr/bin/grep -Eq '^[!]?include[[:space:]]+([^[:space:]]*/)?teka-build-vm[.]conf[[:space:]]*$'; then
  printf '\n!include teka-build-vm.conf\n' >> "$target"
fi
if [ "$nix_dir" = /etc/nix ]; then
  if /bin/launchctl print system/systems.determinate.nix-daemon >/dev/null 2>&1; then
    /bin/launchctl kickstart -k system/systems.determinate.nix-daemon
  elif /bin/launchctl print system/org.nixos.nix-daemon >/dev/null 2>&1; then
    /bin/launchctl kickstart -k system/org.nixos.nix-daemon
  else
    echo 'Builder registered; restart the Nix daemon before using it.' >&2
    exit 1
  fi
fi
"#;

pub(super) fn nix() -> PathBuf {
    on_path("nix").unwrap_or_else(|| "/nix/var/nix/profiles/default/bin/nix".into())
}

fn system_configuration() -> Result<Value> {
    let output = checked_output(
        Command::new(nix())
            .args(["config", "show", "--json"])
            .env("NIX_CONF_DIR", SYSTEM_DIR)
            .env("NIX_USER_CONF_FILES", "/dev/null")
            .env("NIX_CONFIG", ""),
    )?;
    serde_json::from_str(&output).context("read system Nix configuration")
}

pub(super) fn managed(row: &str) -> bool {
    row.split_whitespace()
        .next()
        .is_some_and(|uri| uri == format!("ssh-ng://builder@127.0.0.1:{SSH_PORT}"))
}

pub(super) fn rows(builders: &str) -> Result<Vec<String>> {
    let mut rows = Vec::new();
    for entry in builders.split([';', '\n']) {
        let entry = entry.split('#').next().unwrap_or_default().trim();
        if let Some(path) = entry.strip_prefix('@') {
            let contents = match fs::read_to_string(path) {
                Ok(contents) => contents,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| format!("read Nix builders file {path}"))
                }
            };
            rows.extend(contents.split([';', '\n']).filter_map(|row| {
                let row = row.split('#').next().unwrap_or_default().trim();
                (!row.is_empty()).then(|| row.to_owned())
            }));
        } else if !entry.is_empty() {
            rows.push(entry.to_owned());
        }
    }
    Ok(rows)
}

fn configuration_text(existing: &str, row: &str) -> String {
    let mut entries: Vec<_> = existing
        .split([';', '\n'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty() && !managed(entry))
        .collect();
    entries.push(row);
    format!(
        "builders = {}\nbuilders-use-substitutes = true\nbuild-poll-interval = 1\n",
        entries.join("; ")
    )
}

impl Vm {
    fn builder_row(&self) -> Result<String> {
        let host_key = fs::read_to_string(self.root.join("keys/ssh_host_ed25519_key.pub"))?;
        Ok(format!("ssh-ng://builder@127.0.0.1:{SSH_PORT} aarch64-linux,x86_64-linux {IDENTITY} 4 1 big-parallel,benchmark - {}", openssl::base64::encode_block(host_key.trim().as_bytes())))
    }

    fn check_settings(&self, configuration: &Value) -> Result<()> {
        let expected = self.builder_row()?;
        let builders = configuration["builders"]["value"]
            .as_str()
            .unwrap_or_default();
        ensure!(
            rows(builders)?
                .iter()
                .any(|row| row.split_whitespace().eq(expected.split_whitespace())),
            "system Nix daemon has no matching teka-build-vm builder; run `teka build vm init`"
        );
        ensure!(
            configuration["builders-use-substitutes"]["value"].as_bool() == Some(true),
            "enable builders-use-substitutes; run `teka build vm init`"
        );
        ensure!(
            configuration["build-poll-interval"]["value"].as_u64() == Some(1),
            "set build-poll-interval to 1 to avoid delayed remote build retries; run `teka build vm init`"
        );
        let platforms = configuration["extra-platforms"]["value"].as_array();
        ensure!(!platforms.is_some_and(|platforms| platforms.iter().any(|platform| platform.as_str().is_some_and(|platform| platform.ends_with("-linux")))),
            "host extra-platforms claims Linux support; remove Linux platforms so Nix uses remote builders");
        Ok(())
    }

    fn check_registration(&self, configuration: &Value) -> Result<()> {
        self.check_settings(configuration)?;
        let public = fs::read_to_string(format!("{IDENTITY}.pub"))
            .context("read system builder public key; run `teka build vm init`")?;
        ensure!(
            public.trim() == fs::read_to_string(self.root.join("keys/builder_ed25519.pub"))?.trim(),
            "system builder key does not match the VM; run `teka build vm init`"
        );
        let metadata = fs::metadata(IDENTITY).context("system builder SSH identity is missing")?;
        ensure!(metadata.is_file() && metadata.uid() == 0 && metadata.permissions().mode() & 0o777 == 0o600,
            "system builder SSH identity must be root-owned with mode 0600; run `teka build vm init`");
        Ok(())
    }

    pub(super) fn registration(&self) -> Result<String> {
        self.check_registration(&system_configuration()?)?;
        Ok(format!(
            "aarch64-linux, x86_64-linux via localhost:{SSH_PORT}"
        ))
    }

    pub(super) fn register(&self) -> Result<()> {
        let configuration = system_configuration()?;
        if self.check_registration(&configuration).is_ok() {
            eprintln!("✓ System remote builder: already configured");
            return Ok(());
        }
        let existing = configuration["builders"]["value"]
            .as_str()
            .unwrap_or_default();
        let mut fragment_text = configuration_text(existing, &self.builder_row()?);
        if let Some(platforms) = configuration["extra-platforms"]["value"].as_array() {
            let mut platforms: Vec<_> = platforms
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect();
            platforms.retain(|platform| !platform.ends_with("-linux"));
            fragment_text.push_str(&format!("extra-platforms = {}\n", platforms.join(" ")));
        }
        let temporary = tempfile::tempdir()?;
        let fragment = temporary.path().join(FRAGMENT);
        fs::write(&fragment, fragment_text)?;
        crate::build_check::timed(
            "System remote builder",
            "Registering system remote builder...",
            "configured",
            || {
                sudo_shell(INSTALL)
                    .arg("teka-build-vm-install")
                    .arg(SYSTEM_DIR)
                    .arg(self.root.join("keys/builder_ed25519"))
                    .arg(fragment)
                    .check_status("system builder registration failed or was cancelled")
                    .context("configure system remote builder")?;
                self.check_settings(&system_configuration()?)
            .context("verify system builder registration after daemon restart; Nix did not load the registered builder configuration")?;
                Ok(())
            },
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_existing_builders_and_replaces_only_its_own_row() {
        let old = "@/etc/nix/machines; ssh-ng://builder@127.0.0.1:31023 aarch64-linux /old-key 4";
        let row = "ssh-ng://builder@127.0.0.1:31023 aarch64-linux,x86_64-linux /new-key 4";
        let text = configuration_text(old, row);
        assert!(text.starts_with("builders = @/etc/nix/machines; "));
        assert_eq!(text.matches("127.0.0.1:31023").count(), 1);
        assert!(!text.contains("old-key"));
        assert!(text.contains("builders-use-substitutes = true"));
        assert!(text.contains("build-poll-interval = 1\n"));
        assert!(super::super::needs_vm(row).unwrap());
        assert!(super::super::needs_vm(&format!("{row}; ssh://other aarch64-linux")).unwrap());
    }

    #[test]
    fn registration_requires_fast_remote_build_retries() {
        let directory = tempfile::tempdir().unwrap();
        let vm = Vm {
            root: directory.path().to_owned(),
            agent: directory.path().join("agent"),
            domain: "test".into(),
        };
        fs::create_dir_all(vm.root.join("keys")).unwrap();
        fs::write(vm.root.join("keys/ssh_host_ed25519_key.pub"), "host key").unwrap();
        let mut configuration = serde_json::json!({
            "builders": {"value": vm.builder_row().unwrap()},
            "builders-use-substitutes": {"value": true},
            "extra-platforms": {"value": []},
        });
        for interval in [None, Some(5), Some(1)] {
            configuration["build-poll-interval"] = serde_json::json!({"value": interval});
            let result = vm.check_settings(&configuration);
            if interval == Some(1) {
                result.unwrap();
            } else {
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("build-poll-interval"));
            }
        }
    }

    #[test]
    fn host_key_rotation_invalidates_the_daemon_pin_and_refreshes_cli_trust() {
        let directory = tempfile::tempdir().unwrap();
        let vm = Vm {
            root: directory.path().to_owned(),
            agent: directory.path().join("agent"),
            domain: "test".into(),
        };
        fs::create_dir(vm.root.join("keys")).unwrap();
        let key = vm.root.join("keys/ssh_host_ed25519_key.pub");
        fs::write(&key, "ssh-ed25519 OLD").unwrap();
        let mut configuration = serde_json::json!({
            "builders": {"value": vm.builder_row().unwrap()},
            "builders-use-substitutes": {"value": true},
            "build-poll-interval": {"value": 1},
        });
        vm.pin_host_key().unwrap();
        vm.check_settings(&configuration).unwrap();
        fs::write(key, "ssh-ed25519 NEW\n").unwrap();
        assert!(vm.check_settings(&configuration).is_err());
        configuration["builders"]["value"] = vm.builder_row().unwrap().into();
        vm.pin_host_key().unwrap();
        vm.check_settings(&configuration).unwrap();
        assert_eq!(
            fs::read_to_string(vm.root.join("known_hosts")).unwrap(),
            "[127.0.0.1]:31023 ssh-ed25519 NEW\n"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn installer_is_idempotent_and_preserves_system_configuration() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("nix config");
        fs::create_dir(&directory).unwrap();
        let main = "experimental-features = nix-command flakes\n!include nix.custom.conf\n";
        let custom = "builders = @/etc/nix/machines\nmax-jobs = 4\n";
        fs::write(directory.join("nix.custom.conf"), custom).unwrap();
        let key = temporary.path().join("key with spaces");
        fs::write(&key, "private key").unwrap();
        fs::write(key.with_file_name("key with spaces.pub"), "public key").unwrap();
        let fragment = temporary.path().join("fragment");
        let text = configuration_text(
            "@/etc/nix/machines",
            "ssh-ng://builder@127.0.0.1:31023 aarch64-linux - 4",
        );
        fs::write(&fragment, &text).unwrap();
        // Determinate regenerates nix.conf, so registration must stay in its custom file.
        let generated = format!("{main}ssl-cert-file = /etc/ssl/cert.pem\n");
        for configuration in [main, main, generated.as_str()] {
            fs::write(directory.join("nix.conf"), configuration).unwrap();
            let status = Command::new("/bin/sh")
                .args(["-c", INSTALL, "install-test"])
                .arg(&directory)
                .arg(&key)
                .arg(&fragment)
                .status()
                .unwrap();
            assert!(status.success());
            assert_eq!(
                fs::read_to_string(directory.join("nix.conf")).unwrap(),
                configuration
            );
            assert_eq!(
                fs::read_to_string(directory.join("nix.custom.conf")).unwrap(),
                format!("{custom}\n!include teka-build-vm.conf\n")
            );
            assert_eq!(fs::read_to_string(directory.join(FRAGMENT)).unwrap(), text);
        }
        let installed = directory.join("teka-build-vm/builder_ed25519");
        assert_eq!(fs::read_to_string(&installed).unwrap(), "private key");
        assert_eq!(
            fs::metadata(installed).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn installer_refuses_to_modify_declarative_nix_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let managed = directory.path().join("managed");
        fs::write(&managed, "managed configuration").unwrap();
        std::os::unix::fs::symlink(&managed, directory.path().join("nix.conf")).unwrap();
        let output = Command::new("/bin/sh")
            .args(["-c", INSTALL, "install-test"])
            .arg(directory.path())
            .arg("unused-key")
            .arg("unused-fragment")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("managed declaratively"));
        assert_eq!(
            fs::read_to_string(managed).unwrap(),
            "managed configuration"
        );
        assert!(!directory.path().join("teka-build-vm").exists());
    }
}

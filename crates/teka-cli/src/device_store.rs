#![forbid(unsafe_code)]

use anyhow::{anyhow, Context, Result};
#[cfg(target_os = "linux")]
use anyhow::{bail, ensure};
#[cfg(target_os = "linux")]
use secret_service::{
    blocking::{Collection, Item, SecretService},
    EncryptionType,
};
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::collections::HashMap;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::PathBuf,
};

use crate::device::canonical_url;

#[cfg(target_os = "macos")]
use security_framework::{
    item::{update_item, ItemClass, ItemSearchOptions, ItemUpdateOptions},
    os::macos::keychain::{SecKeychain, SecPreferencesDomain::User},
};

const SERVICE: &str = "teka-cli";
#[cfg(target_os = "macos")]
const ITEM_NOT_FOUND: i32 = -25300; // errSecItemNotFound

#[derive(Clone, Default, Deserialize, Serialize)]
pub(crate) struct Metadata {
    pub pn: Option<String>,
    pub sn: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub default_device: Option<String>,
    #[serde(default)]
    pub devices: BTreeSet<String>,
    #[serde(default)]
    pub selected_devices: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Metadata>,
}

pub(crate) fn connections() -> Result<Config> {
    let path = config_path()?;
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let mut config: Config = serde_json::from_slice(&contents)
        .with_context(|| format!("invalid configuration in {}", path.display()))?;
    config.default_device = config
        .default_device
        .as_deref()
        .map(canonical_url)
        .transpose()
        .context("invalid default_device in configuration")?;
    config.devices = config
        .devices
        .iter()
        .map(|device| canonical_url(device))
        .collect::<Result<_>>()
        .context("invalid saved device in configuration")?;
    config.selected_devices = config
        .selected_devices
        .iter()
        .map(|device| canonical_url(device))
        .collect::<Result<_>>()
        .context("invalid selected device in configuration")?;
    if config.selected_devices.is_empty() {
        config
            .selected_devices
            .extend(config.default_device.iter().cloned());
    }
    config
        .devices
        .extend(config.selected_devices.iter().cloned());
    config.metadata = config
        .metadata
        .into_iter()
        .map(|(url, metadata)| Ok((canonical_url(&url)?, metadata)))
        .collect::<Result<_>>()
        .context("invalid device metadata in configuration")?;
    config.default_device = config
        .selected_devices
        .first()
        .filter(|_| config.selected_devices.len() == 1)
        .cloned();
    Ok(config)
}

fn save_config(config: &Config) -> Result<()> {
    let path = config_path()?;
    let parent = path.parent().context("configuration path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(file, "{}", serde_json::to_string(config)?)?;
    file.as_file().sync_all()?;
    file.persist(&path)
        .with_context(|| format!("save {}", path.display()))?;
    Ok(())
}

fn config_path() -> Result<PathBuf> {
    let directory = match std::env::var_os("XDG_CONFIG_HOME").filter(|path| !path.is_empty()) {
        Some(directory) => PathBuf::from(directory),
        None => PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".config"),
    };
    Ok(directory.join("teka/config.json"))
}

fn decode_password(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| anyhow!("credential store password is not valid UTF-8"))
}

fn credential_label(device: &str) -> Result<String> {
    let url = reqwest::Url::parse(device)?;
    Ok(format!("Teka {}", crate::device::short_hostname(&url)?))
}

#[cfg(target_os = "linux")]
pub(crate) fn get(device: &str) -> Result<Option<String>> {
    linux_store(device, |_, item, _| {
        item.map(|item| decode_password(item.get_secret()?))
            .transpose()
    })
}

#[cfg(target_os = "linux")]
pub(crate) fn set(device: &str, password: Option<&str>, metadata: &Metadata) -> Result<()> {
    let label = credential_label(device)?;
    linux_store(device, |collection, item, attrs| {
        let attrs = attrs
            .into_iter()
            .chain(metadata.sn.as_deref().map(|sn| ("sn", sn)))
            .collect();
        match (item, password.map(str::as_bytes)) {
            (Some(item), Some(secret)) => {
                item.set_secret(secret, "text/plain")?;
                item.set_label(&label)?;
                item.set_attributes(attrs)?;
            }
            (Some(item), None) => item.delete()?,
            (None, Some(secret)) => {
                collection.create_item(&label, attrs, secret, true, "text/plain")?;
            }
            (None, None) => {}
        }
        Ok(())
    })
}

#[cfg(target_os = "linux")]
fn linux_store<T>(
    device: &str,
    operation: impl FnOnce(&Collection<'_>, Option<Item<'_>>, HashMap<&str, &str>) -> Result<T>,
) -> Result<T> {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none()
        && !std::env::var_os("XDG_RUNTIME_DIR")
            .is_some_and(|dir| PathBuf::from(dir).join("bus").exists())
    {
        bail!("Secret Service session bus is unavailable; set TEKA_PASSWORD for automation");
    }
    // The user session bus provides local IPC; credentials stay in the OS keyring.
    let service = SecretService::connect(EncryptionType::Plain)
        .context("Secret Service provider is unavailable")?;
    let collection = service
        .get_default_collection()
        .context("Secret Service default collection is unavailable")?;
    let attrs = HashMap::from([("service", SERVICE), ("url", device)]);
    let items = service.search_items(attrs.clone())?;
    if collection.is_locked()? || !items.locked.is_empty() {
        bail!("Secret Service collection is locked; unlock it before using teka");
    }
    ensure!(items.unlocked.len() <= 1, "ambiguous Secret Service match");
    let item = items.unlocked.into_iter().next();
    operation(&collection, item, attrs)
}

#[cfg(target_os = "macos")]
pub(crate) fn get(device: &str) -> Result<Option<String>> {
    let _interaction = SecKeychain::user_interaction_allowed()?
        .then(SecKeychain::disable_user_interaction)
        .transpose()?;
    match SecKeychain::default_for_domain(User)?.find_generic_password(SERVICE, device) {
        Ok((bytes, _)) => Ok(Some(decode_password(bytes.to_vec())?)),
        Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
        Err(error) => Err(error).context("credential store is locked or inaccessible"),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn set(device: &str, password: Option<&str>, metadata: &Metadata) -> Result<()> {
    let keychain = SecKeychain::default_for_domain(User)?;
    let mut query = ItemSearchOptions::new();
    query
        .keychains(std::slice::from_ref(&keychain))
        .class(ItemClass::generic_password())
        .service(SERVICE)
        .account(device);
    if let Some(password) = password {
        keychain
            .set_generic_password(SERVICE, device, password.as_bytes())
            .context("save device credential")?;
        return update_item(
            &query,
            ItemUpdateOptions::new()
                .set_label(credential_label(device)?)
                .set_comment(
                    metadata
                        .sn
                        .as_deref()
                        .map(|sn| format!("SN: {sn}"))
                        .unwrap_or_default(),
                ),
        )
        .context("update device credential metadata");
    }
    match query.delete() {
        Ok(()) => Ok(()),
        Err(error) if error.code() == ITEM_NOT_FOUND => Ok(()),
        Err(error) => Err(error).context("delete device credential"),
    }
}

pub(crate) fn save_connection(
    device: &str,
    password: Option<&str>,
    metadata: &Metadata,
) -> Result<()> {
    let device = canonical_url(device)?;
    let mut config = connections()?;
    config.devices.insert(device.clone());
    let previous_metadata = config
        .metadata
        .insert(device.clone(), metadata.clone())
        .unwrap_or_default();
    let previous = password.map(|_| get(&device)).transpose()?.flatten();
    if password.is_some() {
        set(&device, password, metadata)?;
    }
    let result = save_config(&config);
    if result.is_err() && password.is_some() {
        if let Err(rollback) = set(&device, previous.as_deref(), &previous_metadata) {
            return result.context(format!("restore previous credential failed: {rollback}"));
        }
    }
    result
}

pub(crate) fn forget_connection(device: &str) -> Result<()> {
    let mut config = connections()?;
    let previous = get(device)?;
    let metadata = config.metadata.remove(device).unwrap_or_default();
    set(device, None, &metadata)?;
    config.devices.remove(device);
    config.selected_devices.remove(device);
    config.default_device = config
        .selected_devices
        .first()
        .filter(|_| config.selected_devices.len() == 1)
        .cloned();
    let result = save_config(&config);
    if result.is_err() {
        if let Err(rollback) = set(device, previous.as_deref(), &metadata) {
            return result.context(format!("restore previous credential failed: {rollback}"));
        }
    }
    result
}

pub(crate) fn set_selection(devices: &[String]) -> Result<()> {
    let mut config = connections()?;
    config.selected_devices = devices.iter().cloned().collect();
    config.default_device = config
        .selected_devices
        .first()
        .filter(|_| config.selected_devices.len() == 1)
        .cloned();
    save_config(&config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_labels_shorten_domains_and_preserve_ip_addresses() {
        for (url, expected) in [
            ("http://teka-123.local/", "Teka teka-123"),
            ("https://teka-123.sfo.example:8443/base/", "Teka teka-123"),
            ("http://teka-123/", "Teka teka-123"),
            ("http://192.0.2.1/", "Teka 192.0.2.1"),
            ("http://[fd00::1]/", "Teka [fd00::1]"),
        ] {
            assert_eq!(credential_label(url).unwrap(), expected);
        }
    }
}

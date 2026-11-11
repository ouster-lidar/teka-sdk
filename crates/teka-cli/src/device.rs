use crate::{device_store, plugins::usage};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{fs::File, ops::Deref, path::Path};
use teka::client::{DeviceClient as Client, UploadResponse};

pub use teka::client::{canonical_url, normalize_arch};

pub struct DeviceClient(Client);

impl Deref for DeviceClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DeviceClient {
    pub fn new(host: &str) -> Result<Self> {
        Ok(Self(Client::new(host)?))
    }

    pub fn authenticate(&self) -> Result<Option<String>> {
        let status = self.get_json("api/v1/auth/status")?;
        if status["auth_disabled"] == true || status["authenticated"] == true {
            return Ok(None);
        }
        if status["needs_setup"] != true && status["enforcement"] != true {
            bail!("device auth status does not allow login or setup");
        }
        let (password, _) = credentials(&self.endpoint("")?)?;
        let password = password
            .context("device requires a session; set TEKA_PASSWORD or run teka device select")?;
        self.authenticate_password(&status, &password)?;
        Ok(Some(password))
    }

    pub fn install(&self, path: &Path, profile: &str) -> Result<Value> {
        upload_started(path, "installing")?;
        Ok(print_response(self.0.install(path, profile)?))
    }

    pub fn sysupdate_apply(&self, path: &Path, reboot: bool) -> Result<Value> {
        upload_started(path, "applying system update")?;
        Ok(print_response(self.0.sysupdate_apply(path, reboot)?))
    }
}

pub(crate) fn authentication<T>(result: Result<T>) -> Result<T> {
    result.map_err(|error| {
        if error.is::<teka::client::PasswordRequired>() {
            error.context("device requires a session; set TEKA_PASSWORD or run teka device select")
        } else {
            error
        }
    })
}

fn upload_started(path: &Path, action: &str) -> Result<()> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let size = file.metadata()?.len();
    eprintln!("teka: {action} {} ({size} bytes)", path.display());
    Ok(())
}

fn print_response(response: UploadResponse) -> Value {
    if !response.text.is_empty() {
        println!("{}", response.text);
    }
    response.value
}

pub fn credentials(device: &str) -> Result<(Option<String>, &'static str)> {
    if let Some(password) = std::env::var("TEKA_PASSWORD")
        .ok()
        .filter(|password| !password.is_empty())
    {
        return Ok((Some(password), "env"));
    }
    let password = device_store::get(&canonical_url(device)?)
        .context("read device credentials (set TEKA_PASSWORD for automation)")?;
    let source = password.as_ref().map_or("none", |_| "keychain");
    Ok((password, source))
}

pub fn required_device(flag: Option<&str>) -> Result<String> {
    select_device(flag)?.ok_or_else(|| usage("need --device, TEKA_DEVICE or teka device select"))
}

pub fn parse_device(device: &str) -> Result<String> {
    canonical_url(device).map_err(|error| usage(error.to_string()))
}

pub(crate) fn short_hostname(url: &reqwest::Url) -> Result<&str> {
    let host = url.host_str().context("device URL has no hostname")?;
    Ok(url
        .domain()
        .and_then(|domain| domain.split_once('.'))
        .map_or(host, |(hostname, _)| hostname))
}

pub fn select_device(flag: Option<&str>) -> Result<Option<String>> {
    if let Some(device) = flag {
        device_store::connections()?;
        return Ok(Some(parse_device(device)?));
    }
    let devices = selected_devices()?;
    match devices.len() {
        0 => Ok(None),
        1 => Ok(devices.into_iter().next()),
        _ => Err(usage("multiple devices selected; specify --device")),
    }
}

pub(crate) fn selected_devices() -> Result<Vec<String>> {
    let config = device_store::connections()?;
    match std::env::var("TEKA_DEVICE") {
        Ok(device) => Ok(vec![parse_device(&device)?]),
        _ => Ok(config.selected_devices.into_iter().collect()),
    }
}

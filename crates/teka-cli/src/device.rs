use anyhow::{Context, Result};
use serde_json::Value;
use std::{fs::File, ops::Deref, path::Path};
use crate::client::{DeviceClient as Client, UploadResponse};

pub use crate::client::normalize_arch;

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

    pub fn authenticate(&self) -> Result<()> {
        authentication(self.0.authenticate(password().as_deref()))
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

pub(crate) fn password() -> Option<String> {
    std::env::var("TEKA_PASSWORD").ok()
}

pub(crate) fn authentication<T>(result: Result<T>) -> Result<T> {
    result.map_err(|error| {
        if error.is::<crate::client::PasswordRequired>() {
            error.context("device requires a session; set TEKA_PASSWORD")
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

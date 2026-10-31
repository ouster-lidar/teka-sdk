use anyhow::{bail, Context, Result};
use reqwest::{
    blocking::{Body, Client, Response},
    redirect::Policy,
    Url,
};
use serde_json::{json, Value};
use std::{fs::File, io::Read, path::Path, time::Duration};

pub struct DeviceClient {
    client: Client,
    base: Url,
}

impl DeviceClient {
    pub fn new(host: &str) -> Result<Self> {
        let has_scheme = host.contains("://")
            || host
                .get(..5)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http:"))
            || host
                .get(..6)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https:"));
        // Shorthand hosts work, but explicit schemes are recommended for clarity.
        let address = if has_scheme {
            host.to_owned()
        } else {
            format!("http://{host}")
        };
        let mut base = Url::parse(&address).context("host must be an HTTP(S) URL")?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            bail!("host must be an HTTP(S) URL without credentials, query, or fragment");
        }
        let path = base.path().trim_end_matches('/').to_owned();
        base.set_path(&format!("{path}/"));
        Ok(Self {
            client: Client::builder()
                .cookie_store(true)
                .redirect(Policy::none())
                .connect_timeout(Duration::from_secs(15))
                .build()?,
            base,
        })
    }

    pub fn endpoint(&self, path: &str) -> Result<Url> {
        Ok(self.base.join(path)?)
    }

    /// Session-carrying client for device endpoints without a wrapper here.
    pub fn http(&self) -> &Client {
        &self.client
    }

    /// Rejects unsuccessful responses, including the body in the error.
    pub fn response(response: Response) -> Result<Response> {
        if !response.status().is_success() {
            let status = response.status();
            let mut body = String::new();
            response.take(8192).read_to_string(&mut body).ok();
            bail!("device HTTP {status}: {body}");
        }
        Ok(response)
    }

    pub fn get_json(&self, path: &str) -> Result<Value> {
        Ok(Self::response(
            self.client
                .get(self.endpoint(path)?)
                .timeout(Duration::from_secs(30))
                .send()?,
        )?
        .json()?)
    }

    pub fn authenticate(&self) -> Result<()> {
        let status = self.get_json("api/v1/auth/status")?;
        if status["auth_disabled"] == true || status["authenticated"] == true {
            return Ok(());
        }
        if status["needs_setup"] != true && status["enforcement"] != true {
            bail!("device auth status does not allow login or setup");
        }
        let password = std::env::var("TEKA_PASSWORD")
            .context("device requires a session; set TEKA_PASSWORD")?;
        let path = if status["needs_setup"] == true {
            "api/v1/auth/setup"
        } else {
            "api/v1/auth/login"
        };
        Self::response(
            self.client
                .post(self.endpoint(path)?)
                .json(&json!({"password": password}))
                .timeout(Duration::from_secs(30))
                .send()?,
        )?;
        Ok(())
    }

    pub fn architecture(&self) -> Result<String> {
        let status = self.get_json("api/v1/platform/architecture")?;
        normalize_arch(
            status["architecture"]
                .as_str()
                .context("device returned no architecture")?,
        )
    }

    pub fn remove_app(&self, name: &str) -> Result<()> {
        let url = self.app_endpoint(name, "remove")?;
        Self::response(
            self.client
                .post(url)
                .timeout(Duration::from_secs(30))
                .send()?,
        )?;
        Ok(())
    }

    fn app_endpoint(&self, name: &str, action: &str) -> Result<Url> {
        let mut url = self.endpoint("api/v1/manager/app/")?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid host URL"))?
            .pop_if_empty()
            .push(name)
            .push(action);
        Ok(url)
    }

    pub fn logs(&self, name: &str, seconds: u64) -> Result<String> {
        let mut url = self.app_endpoint(name, "logs")?;
        url.query_pairs_mut()
            .append_pair("since_unit_start", "true");
        let response = Self::response(
            self.client
                .get(url)
                .timeout(Duration::from_secs(seconds.max(1)))
                .send()?,
        )?;
        let mut bytes = Vec::new();
        // A live log stream normally ends by timeout; retain everything received so far.
        let _ = response.take(8 * 1024 * 1024).read_to_end(&mut bytes);
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn teka_status(&self) -> Result<Value> {
        self.get_json("api/v1/app/teka/status")
    }

    pub fn sysupdate_apply(&self, path: &Path, reboot: bool) -> Result<Value> {
        let mut url = self.endpoint("api/v1/app/teka/sysupdate/apply")?;
        if !reboot {
            url.query_pairs_mut().append_pair("reboot", "false");
        }
        self.upload(url, path, "applying system update")
    }

    /// Streams `path` to `url` and prints the response body on stdout.
    fn upload(&self, url: Url, path: &Path, action: &str) -> Result<Value> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let size = file.metadata()?.len();
        eprintln!("teka: {action} {} ({size} bytes)", path.display());
        let upload_timeout = Duration::from_secs(size / (1024 * 1024) + 600);
        let response = Self::response(
            self.client
                .post(url)
                .header("Content-Type", "application/octet-stream")
                .body(Body::sized(file, size))
                .timeout(upload_timeout)
                .send()?,
        )?;
        let text = response.text()?;
        let response: Value =
            serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.clone()));
        if !text.is_empty() {
            println!("{text}");
        }
        Ok(response)
    }

    pub fn install(&self, path: &Path, profile: &str) -> Result<Value> {
        let mut url = self.endpoint("api/v1/manager/install")?;
        if !profile.is_empty() && profile != "default" {
            url.query_pairs_mut().append_pair("profile", profile);
        }
        self.upload(url, path, "installing")
    }
}

pub fn normalize_arch(arch: &str) -> Result<String> {
    match arch.to_ascii_lowercase().as_str() {
        "x86_64" | "amd64" => Ok("x86_64".into()),
        "aarch64" | "arm64" => Ok("aarch64".into()),
        _ => bail!("unsupported device architecture '{arch}'"),
    }
}

//! Shared box selection for CLI commands and third-party tools.
use crate::{
    device::{parse_device, short_hostname},
    device_store,
    plugins::usage,
};
use anyhow::Result;
use clap::Args;

/// Flatten into commands that accept box overrides.
#[derive(Clone, Debug, Default, Args)]
pub struct BoxOptions {
    /// Known box hostname or URL; may be repeated
    #[arg(long = "box", value_name = "URL")]
    pub boxes: Vec<String>,
    #[arg(long = "device", value_name = "URL", hide = true)]
    pub devices: Vec<String>,
}

impl BoxOptions {
    /// Resolve canonical URLs without discovery, authentication, or config writes.
    pub fn resolve(&self) -> Result<Vec<String>> {
        resolve_boxes(&self.boxes, &self.devices)
    }

    /// Resolve at least one box.
    pub fn required(&self) -> Result<Vec<String>> {
        self.required_with_source().map(|(boxes, _)| boxes)
    }

    /// Resolve at least one box and identify the winning selection input.
    pub fn required_with_source(&self) -> Result<(Vec<String>, &'static str)> {
        let (boxes, source) = resolve_boxes_with_source(&self.boxes, &self.devices)?;
        if boxes.is_empty() {
            return Err(usage("need --box, TEKA_BOX or teka box select"));
        }
        Ok((boxes, source))
    }

    /// Resolve exactly one box, rejecting an ambiguous saved selection.
    pub fn single(&self) -> Result<String> {
        let mut boxes = self.required()?;
        if boxes.len() != 1 {
            return Err(usage("multiple boxes selected; specify --box"));
        }
        Ok(boxes.remove(0))
    }
}

/// Apply explicit boxes, legacy flags, environment, then saved selection, in order.
/// An empty selection is returned to callers that allow operating without a target.
pub fn resolve_boxes(boxes: &[String], devices: &[String]) -> Result<Vec<String>> {
    resolve_boxes_with_source(boxes, devices).map(|(boxes, _)| boxes)
}

fn resolve_boxes_with_source(
    boxes: &[String],
    devices: &[String],
) -> Result<(Vec<String>, &'static str)> {
    let config = device_store::connections()?;
    let (explicit, source) = if boxes.is_empty() {
        (devices, "--device")
    } else {
        (boxes, "--box")
    };
    if !explicit.is_empty() {
        return Ok((
            explicit
                .iter()
                .map(|value| resolve_box(value, &config))
                .collect::<Result<_>>()?,
            source,
        ));
    }
    for name in ["TEKA_BOX", "TEKA_DEVICE"] {
        if let Some(value) = std::env::var_os(name) {
            let value = value
                .into_string()
                .map_err(|_| usage(format!("invalid {name}")))?;
            return Ok((vec![resolve_box(&value, &config)?], name));
        }
    }
    Ok((
        config.selected_devices.into_iter().collect(),
        "saved selection (teka box select)",
    ))
}

fn resolve_box(value: &str, config: &device_store::Config) -> Result<String> {
    let ipv6 = value.parse::<std::net::Ipv6Addr>().ok();
    let direct = match ipv6 {
        Some(address) => parse_device(&format!("[{address}]"))?,
        None => parse_device(value)?,
    };
    if value.contains("://")
        || value.contains('/')
        || (value.contains(':')
            && ipv6.is_none()
            && !(value.starts_with('[') && value.ends_with(']')))
    {
        return Ok(direct);
    }
    let requested = reqwest::Url::parse(&direct)?;
    let hostname = requested.host_str().unwrap_or_default();
    let mut matches = config.devices.iter().filter(|known| {
        let Ok(url) = reqwest::Url::parse(known) else {
            return false;
        };
        url.host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case(hostname))
            || short_hostname(&url).is_ok_and(|host| host.eq_ignore_ascii_case(hostname))
    });
    match (matches.next(), matches.next()) {
        (Some(url), None) => Ok(url.clone()),
        (None, _) => Ok(direct),
        _ => Err(usage(
            "multiple known boxes match hostname; specify a full URL with --box",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_overrides_reuse_saved_identity_and_preserve_direct_addresses() {
        let config = device_store::Config {
            devices: [
                "https://teka-example.local:8443/proxy/",
                "http://10.20.30.1/",
                "https://[2001:db8::1]:8443/proxy/",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            ..Default::default()
        };
        for (input, expected) in [
            ("teka-example", "https://teka-example.local:8443/proxy/"),
            ("TEKA-EXAMPLE", "https://teka-example.local:8443/proxy/"),
            (
                "teka-example.local",
                "https://teka-example.local:8443/proxy/",
            ),
            ("http://teka-example", "http://teka-example/"),
            ("teka-example:8080", "http://teka-example:8080/"),
            ("teka-example/path", "http://teka-example/path/"),
            ("untracked", "http://untracked/"),
            ("10.20.30.1", "http://10.20.30.1/"),
            ("10.20.30.2", "http://10.20.30.2/"),
            ("[2001:db8::1]", "https://[2001:db8::1]:8443/proxy/"),
            ("2001:db8::1", "https://[2001:db8::1]:8443/proxy/"),
        ] {
            assert_eq!(resolve_box(input, &config).unwrap(), expected, "{input}");
        }
    }

    #[test]
    fn ambiguous_hostnames_require_an_explicit_url() {
        let config = device_store::Config {
            devices: [
                "http://box.local/",
                "https://box.example.com/",
                "http://box.local:8080/",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            ..Default::default()
        };
        for input in ["box", "box.local"] {
            assert!(resolve_box(input, &config)
                .unwrap_err()
                .to_string()
                .contains("multiple known boxes"));
        }
        assert_eq!(
            resolve_box("http://box.local", &config).unwrap(),
            "http://box.local/"
        );
        assert_eq!(
            resolve_box("box.example.com", &config).unwrap(),
            "https://box.example.com/"
        );
    }
}

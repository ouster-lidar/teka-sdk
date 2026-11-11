use crate::{
    build_check,
    device::{credentials, parse_device, selected_devices, short_hostname, DeviceClient},
    device_store::{self, save_connection, Metadata},
};
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::IsTerminal,
    net::{SocketAddr, ToSocketAddrs},
    sync::mpsc,
    time::{Duration, Instant},
};
use teka::client::discovery::{discover, DiscoveredDevice};

#[derive(Debug, Subcommand)]
pub enum DeviceCommand {
    /// List known devices, or discover devices on the local network
    List {
        /// Match hostname, part number, or serial (case-insensitive substring)
        patterns: Vec<String>,
        /// Perform fresh network discovery instead of listing known devices
        #[arg(long)]
        discover: bool,
        /// Seconds to browse during discovery
        #[arg(long, default_value = "2", requires = "discover", value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        #[arg(long)]
        json: bool,
    },
    /// Authenticate and select default deployment targets
    Select {
        /// Match all devices by hostname, part number, or serial; HTTP(S) URLs are accepted directly
        patterns: Vec<String>,
    },
    /// Forget known devices and their saved credentials
    Forget {
        /// Match known devices by hostname, part number, or serial
        patterns: Vec<String>,
    },
    /// Report device selection, credentials, and connectivity
    Status {
        /// Match known devices by hostname, part number, or serial
        patterns: Vec<String>,
        #[arg(long, value_name = "URL", conflicts_with = "patterns")]
        device: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

pub fn run(command: DeviceCommand) -> Result<()> {
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    match command {
        DeviceCommand::List {
            patterns,
            discover: network,
            timeout,
            json,
        } => {
            if network {
                let devices = discover_devices(timeout, json, &patterns)?;
                let mut seen = BTreeSet::new();
                let devices = discovery_status(
                    devices
                        .into_iter()
                        .filter(|device| matches_discovered(device, &patterns))
                        .filter(|device| {
                            seen.insert(device_identity(&device.url, device.sn.as_deref()))
                        })
                        .collect(),
                );
                if json {
                    println!("{}", serde_json::to_string(&devices)?);
                } else {
                    print_discovery(&devices, color);
                    build_check::note(if devices.is_empty() {
                        "Check that your device is on the same network."
                    } else {
                        "Select: teka device select <PATTERN>"
                    });
                }
            } else {
                let config = device_store::connections()?;
                let devices = known_devices(&config);
                let devices: Vec<_> = devices
                    .iter()
                    .filter(|(url, metadata)| matches_patterns(url, metadata, &patterns))
                    .collect();
                if json {
                    println!("{}", json!(devices.iter().map(|(url, metadata)| json!({
                        "url": url, "selected": config.selected_devices.contains(*url), "pn": metadata.pn, "sn": metadata.sn
                    })).collect::<Vec<_>>()));
                } else {
                    for (url, _) in devices {
                        let marker = if config.selected_devices.contains(url) {
                            '*'
                        } else {
                            ' '
                        };
                        println!("{marker} {url}");
                    }
                }
            }
        }
        DeviceCommand::Select { patterns } => {
            let config = device_store::connections()?;
            if patterns.is_empty() {
                require_terminal()?;
            }
            let mut candidates = if patterns.is_empty() {
                BTreeMap::new()
            } else {
                known_devices(&config)
            };
            for pattern in patterns.iter().filter(|pattern| pattern.contains("://")) {
                candidates.entry(parse_device(pattern)?).or_default();
            }
            let mut discovered = BTreeSet::new();
            if patterns.is_empty() || patterns.iter().any(|pattern| !pattern.contains("://")) {
                for device in discover_devices(2, false, &patterns)? {
                    discovered.insert(device.url.clone());
                    let metadata = candidates.entry(device.url).or_insert_with_key(|url| {
                        config.metadata.get(url).cloned().unwrap_or_default()
                    });
                    metadata.pn = device.pn.or(metadata.pn.take());
                    metadata.sn = device.sn.or(metadata.sn.take());
                }
            }
            let mut selected = if patterns.is_empty() {
                choose_devices(
                    &candidates,
                    &config.selected_devices,
                    "Select devices",
                    true,
                )?
            } else {
                resolve_patterns(&candidates, &patterns)?
            };
            // Prefer current advertisements over saved aliases that may be stale.
            selected.sort_by_key(|url| !discovered.contains(url));
            let selected = unique_urls(&candidates, selected);
            let shared = std::env::var("TEKA_PASSWORD")
                .ok()
                .filter(|password| !password.is_empty());
            let mut passwords = Passwords {
                from_env: shared.is_some(),
                prompted: shared.is_some() || selected.len() < 2,
                shared,
                confirmed: false,
                known: config.devices,
            };
            let mut attempts = passwords
                .shared
                .as_deref()
                .filter(|_| passwords.from_env)
                .map(|password| authenticate_parallel(&selected, password))
                .unwrap_or_default()
                .into_iter();
            for url in &selected {
                let label = device_label(url)?;
                let result = if passwords.from_env {
                    match attempts.next().unwrap() {
                        Err(error)
                            if password_rejected(&error) && std::io::stdin().is_terminal() =>
                        {
                            eprintln!("teka: password rejected for {label}");
                            passwords.shared = None;
                            authenticate(url, &mut passwords, selected.len())
                        }
                        result => result,
                    }
                } else {
                    authenticate(url, &mut passwords, selected.len())
                };
                let password = result.with_context(|| format!("authenticate {label}"))?;
                save_connection(url, password.as_deref(), &candidates[url])?;
            }
            device_store::set_selection(&selected)?;
            for url in selected {
                println!("Selected {}", device_label(&url)?);
            }
        }
        DeviceCommand::Forget { patterns } => {
            let config = device_store::connections()?;
            for pattern in patterns.iter().filter(|pattern| pattern.contains("://")) {
                parse_device(pattern)?;
            }
            let candidates = known_devices(&config);
            let selected = if patterns.is_empty() {
                choose_devices(&candidates, &BTreeSet::new(), "Forget devices", false)?
            } else {
                resolve_patterns(&candidates, &patterns)?
            };
            for url in selected {
                device_store::forget_connection(&url)?;
                println!("Forgot {}", device_label(&url)?);
            }
        }
        DeviceCommand::Status {
            patterns,
            device,
            json,
        } => {
            let config = device_store::connections()?;
            let mut candidates = known_devices(&config);
            let targets = if let Some(device) = device {
                vec![parse_device(&device)?]
            } else if patterns.is_empty() {
                selected_devices()?
            } else {
                for pattern in patterns.iter().filter(|pattern| pattern.contains("://")) {
                    candidates.entry(parse_device(pattern)?).or_default();
                }
                resolve_patterns(&candidates, &patterns)?
            };
            for url in &targets {
                candidates.entry(url.clone()).or_default();
            }
            let reports = unique_urls(&candidates, targets)
                .into_iter()
                .map(|device| {
                    let report = status(&device).with_context(|| format!("status for {device}"))?;
                    Ok((device, report))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            if json {
                println!("{}", json!(reports));
            } else {
                for (index, (device, report)) in reports.iter().enumerate() {
                    if index != 0 {
                        println!();
                    }
                    println!("Device: {device}");
                    for (label, key) in [
                        ("Credentials", "credential_source"),
                        ("System resolver", "system_resolution"),
                        ("Reachability", "reachability"),
                        ("Authentication", "auth"),
                    ] {
                        println!("{label}: {}", report[key].as_str().unwrap_or("none"));
                    }
                    print_addresses(report, color, "");
                    let resolution = report["system_resolution"].as_str().unwrap();
                    if report["reachability"] == "reachable"
                        && matches!(resolution, "failed" | "timeout")
                    {
                        eprintln!("⚠ {device}: system resolver {resolution}; device is reachable. Check host DNS/mDNS configuration.");
                    }
                }
            }
        }
    }
    Ok(())
}

fn known_devices(config: &device_store::Config) -> BTreeMap<String, Metadata> {
    config
        .devices
        .iter()
        .map(|url| {
            (
                url.clone(),
                config.metadata.get(url).cloned().unwrap_or_default(),
            )
        })
        .collect()
}

fn matches_patterns(url: &str, metadata: &Metadata, patterns: &[String]) -> bool {
    patterns.is_empty()
        || patterns
            .iter()
            .any(|pattern| matches_pattern(url, metadata, pattern))
}

fn matches_pattern(url: &str, metadata: &Metadata, pattern: &str) -> bool {
    if pattern.contains("://") {
        return crate::device::canonical_url(pattern).ok().as_deref() == Some(url);
    }
    let pattern = pattern.to_lowercase();
    let host = reqwest::Url::parse(url).ok();
    let matched = [
        host.as_ref().and_then(reqwest::Url::host_str),
        metadata.pn.as_deref(),
        metadata.sn.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase().contains(&pattern));
    matched
}

fn resolve_patterns(
    devices: &BTreeMap<String, Metadata>,
    patterns: &[String],
) -> Result<Vec<String>> {
    let mut selected = BTreeSet::new();
    for pattern in patterns {
        let matches: Vec<_> = devices
            .iter()
            .filter(|(url, metadata)| matches_pattern(url, metadata, pattern))
            .map(|(url, _)| url.clone())
            .collect();
        if matches.is_empty() {
            bail!("no devices match pattern {pattern:?}");
        }
        selected.extend(matches);
    }
    Ok(selected.into_iter().collect())
}

fn device_identity(url: &str, serial: Option<&str>) -> String {
    match serial.map(str::trim).filter(|serial| !serial.is_empty()) {
        Some(serial) => format!("sn:{serial}"),
        None => format!("url:{url}"),
    }
}

fn unique_urls(devices: &BTreeMap<String, Metadata>, urls: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    urls.into_iter()
        .filter(|url| seen.insert(device_identity(url, devices[url].sn.as_deref())))
        .collect()
}

fn matches_discovered(device: &DiscoveredDevice, patterns: &[String]) -> bool {
    matches_patterns(
        &device.url,
        &Metadata {
            pn: device.pn.clone(),
            sn: device.sn.clone(),
        },
        patterns,
    )
}

fn discovered_count(devices: &[DiscoveredDevice], patterns: &[String]) -> usize {
    devices
        .iter()
        .filter(|device| matches_discovered(device, patterns))
        .map(|device| device_identity(&device.url, device.sn.as_deref()))
        .collect::<BTreeSet<_>>()
        .len()
}

fn discover_devices(
    timeout: u64,
    quiet: bool,
    patterns: &[String],
) -> Result<Vec<DiscoveredDevice>> {
    let started = Instant::now();
    if !quiet {
        build_check::progress("Discovering Teka devices...");
    }
    let devices = discover(Duration::from_secs(timeout));
    if quiet {
        devices
    } else {
        build_check::finish(
            "Discovery",
            devices,
            |devices| {
                format!(
                    "{} found, {} matched",
                    discovered_count(devices, &[]),
                    discovered_count(devices, patterns)
                )
            },
            Some(started),
        )
    }
}

fn require_terminal() -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        bail!("device selection requires a terminal; supply PATTERN arguments or an explicit HTTP(S) URL");
    }
    Ok(())
}

fn device_label(device: &str) -> Result<String> {
    let url = reqwest::Url::parse(device)?;
    let hostname = short_hostname(&url)?;
    Ok(match url.port() {
        Some(port) => format!("{hostname}:{port}"),
        None => hostname.to_owned(),
    })
}

fn choose_devices(
    devices: &BTreeMap<String, Metadata>,
    selected: &BTreeSet<String>,
    prompt: &str,
    deduplicate: bool,
) -> Result<Vec<String>> {
    require_terminal()?;
    if devices.is_empty() {
        bail!("no devices available; use teka device list --discover to check the network");
    }
    let mut urls = devices.keys().cloned().collect();
    if deduplicate {
        urls = unique_urls(devices, urls);
    }
    let mut choices = urls
        .iter()
        .map(|url| Ok((device_label(url)?, url)))
        .collect::<Result<Vec<_>>>()?;
    choices.sort();
    let selected_ids: BTreeSet<_> = selected
        .iter()
        .filter_map(|url| {
            devices
                .get(url)
                .map(|metadata| device_identity(url, metadata.sn.as_deref()))
        })
        .collect();
    let defaults: Vec<_> = choices
        .iter()
        .map(|(_, url)| selected_ids.contains(&device_identity(url, devices[*url].sn.as_deref())))
        .collect();
    let indices = dialoguer::MultiSelect::new()
        .with_prompt(format!(
            "{prompt} (↑/↓ to move, Space to toggle, Enter to confirm, Esc to cancel)"
        ))
        .items(choices.iter().map(|(label, _)| label).collect::<Vec<_>>())
        .defaults(&defaults)
        .interact_opt()?
        .context("device selection cancelled")?;
    Ok(indices
        .into_iter()
        .map(|index| choices[index].1.clone())
        .collect())
}

fn discovery_status(devices: Vec<DiscoveredDevice>) -> Vec<Value> {
    // Start all lookups before waiting so the deadline is shared across devices.
    let lookups: Vec<_> = devices
        .iter()
        .map(|device| system_resolution(&device.url).ok())
        .collect();
    devices
        .into_iter()
        .zip(lookups)
        .map(|(device, lookup)| {
            let mut report = serde_json::Map::new();
            report.append(json!(device).as_object_mut().unwrap());
            let mut resolution = lookup.map_or_else(
                || json!({"system_resolution": "failed", "ipv4": [], "ipv6": []}),
                |finish| finish(),
            );
            report.append(resolution.as_object_mut().unwrap());
            Value::Object(report)
        })
        .collect()
}

fn print_discovery(devices: &[Value], color: bool) {
    let (bold, cyan, reset) = if color {
        ("\x1b[1m", "\x1b[36m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    for device in devices {
        let name = device["name"].as_str().unwrap().escape_debug();
        let url = device["url"].as_str().unwrap().escape_debug();
        println!("🖥︎ {bold}{name}{reset}\n  {cyan}{url}{reset}");
        for (label, key) in [
            ("Hardware", "hw"),
            ("Serial", "sn"),
            ("Part", "pn"),
            ("Version", "version"),
        ] {
            if let Some(value) = device[key].as_str().filter(|v| !v.is_empty()) {
                println!("  {label}: {}", value.escape_debug());
            }
        }
        print_addresses(device, color, "  ");
        println!();
    }
}

struct Passwords {
    shared: Option<String>,
    from_env: bool,
    prompted: bool,
    confirmed: bool,
    known: BTreeSet<String>,
}

fn prompt_password(prompt: &str) -> Result<String> {
    if !std::io::stdin().is_terminal() {
        bail!("password prompt requires TTY stdin; set TEKA_PASSWORD for automation");
    }
    Ok(rpassword::prompt_password(prompt)?)
}

fn confirm_password(device: &str, password: &str) -> Result<()> {
    let label = device_label(device)?;
    if password != prompt_password(&format!("? Confirm password for {label}: "))? {
        bail!("passwords do not match for {label}");
    }
    Ok(())
}

fn password_rejected(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<reqwest::Error>()
        .and_then(reqwest::Error::status)
        .is_some_and(|status| matches!(status.as_u16(), 401 | 403))
}

fn authenticate_parallel(devices: &[String], password: &str) -> Vec<Result<Option<String>>> {
    std::thread::scope(|scope| {
        let workers: Vec<_> = devices
            .iter()
            .map(|device| {
                scope.spawn(move || {
                    let client = DeviceClient::new(device)?;
                    let status = client.get_json("api/v1/auth/status")?;
                    if status["auth_disabled"] == true {
                        return Ok(None);
                    }
                    client.authenticate_password(&status, password)?;
                    Ok(Some(password.to_owned()))
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker.join().unwrap_or_else(|_| {
                    Err(anyhow::anyhow!("device authentication worker panicked"))
                })
            })
            .collect()
    })
}

fn authenticate(device: &str, passwords: &mut Passwords, count: usize) -> Result<Option<String>> {
    let label = device_label(device)?;
    let client = DeviceClient::new(device)?;
    let status = client.get_json("api/v1/auth/status")?;
    if status["auth_disabled"] == true {
        return Ok(None);
    }
    let setup = status["needs_setup"] == true;
    if !setup && status["enforcement"] != true {
        bail!("device auth status does not allow login or setup");
    }
    if !setup && !passwords.from_env && passwords.known.contains(device) {
        if let Some(password) =
            device_store::get(device).with_context(|| format!("read saved password for {label}"))?
        {
            match client.authenticate_password(&status, &password) {
                Ok(()) => return Ok(None),
                Err(error) if password_rejected(&error) => {
                    eprintln!("teka: saved password rejected for {label}");
                    return authenticate_prompt(&client, &status, device).map(Some);
                }
                Err(error) => return Err(error),
            }
        }
    }
    if !passwords.prompted {
        let password = prompt_password(&format!(
            "? Password for all {count} selected devices (Enter for per-device passwords): "
        ))?;
        passwords.shared = (!password.is_empty()).then_some(password);
        passwords.prompted = true;
    }
    if let Some(password) = passwords.shared.as_deref() {
        if setup && !passwords.from_env && !passwords.confirmed {
            confirm_password(device, password)?;
            passwords.confirmed = true;
        }
        match client.authenticate_password(&status, password) {
            Ok(()) => return Ok(Some(password.to_owned())),
            Err(error) if !passwords.from_env && password_rejected(&error) => {
                eprintln!("teka: password rejected for {label}");
            }
            Err(error) => return Err(error),
        }
    }
    authenticate_prompt(&client, &status, device).map(Some)
}

fn authenticate_prompt(client: &DeviceClient, status: &Value, device: &str) -> Result<String> {
    let label = device_label(device)?;
    let password = prompt_password(&format!("? Password for {label}: "))?;
    if status["needs_setup"] == true {
        confirm_password(device, &password)?;
    }
    client.authenticate_password(status, &password)?;
    Ok(password)
}

fn system_resolution(device: &str) -> Result<impl FnOnce() -> Value> {
    let url = reqwest::Url::parse(device)?;
    let host = url
        .host_str()
        .context("device URL has no hostname")?
        .trim_matches(['[', ']'])
        .to_owned();
    let deadline = Instant::now() + Duration::from_secs(1);
    let (sender, receiver) = mpsc::channel();
    // System resolver calls cannot be cancelled; share one deadline for all lookups.
    std::thread::spawn(move || {
        if let Ok(addresses) = (host.as_str(), 0).to_socket_addrs() {
            for address in addresses.collect::<BTreeSet<_>>() {
                if sender.send((address, None)).is_err() {
                    break;
                }
                let sender = sender.clone();
                std::thread::spawn(move || {
                    let reverse =
                        dns_lookup::getnameinfo(&address, libc::NI_NAMEREQD | libc::NI_NUMERICSERV)
                            .ok()
                            .map(|(host, _)| host);
                    let _ = sender.send((address, reverse));
                });
            }
        }
    });
    let lookup = std::thread::spawn(move || {
        let mut addresses = BTreeMap::new();
        let state = loop {
            match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok((address, reverse)) => {
                    addresses.insert(address, reverse);
                }
                Err(_) if !addresses.is_empty() => break "ok",
                Err(mpsc::RecvTimeoutError::Timeout) => break "timeout",
                Err(_) => break "failed",
            }
        };
        let family = |ipv4| {
            addresses
                .iter()
                .filter(|(address, _)| address.is_ipv4() == ipv4)
                .map(|(address, reverse)| {
                    json!({"address": address_host(*address), "reverse": reverse})
                })
                .collect::<Vec<_>>()
        };
        json!({"system_resolution": state, "ipv4": family(true), "ipv6": family(false)})
    });
    Ok(move || lookup.join().unwrap())
}

fn address_host(address: SocketAddr) -> String {
    match address {
        SocketAddr::V6(address) if address.scope_id() != 0 => {
            format!("{}%{}", address.ip(), address.scope_id())
        }
        _ => address.ip().to_string(),
    }
}

fn print_addresses(report: &Value, color: bool, indent: &str) {
    let (cyan, green, reset) = if color {
        ("\x1b[36m", "\x1b[32m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    for (label, key) in [("IPv4", "ipv4"), ("IPv6", "ipv6")] {
        for address in report[key].as_array().unwrap() {
            let ip = address["address"].as_str().unwrap_or_default();
            let reverse = address["reverse"]
                .as_str()
                .map(|name| format!(" ({green}{}{reset})", name.escape_debug()))
                .unwrap_or_default();
            println!("{indent}{label}: {cyan}{ip}{reset}{reverse}");
        }
    }
}

fn status(device: &str) -> Result<Value> {
    let mut report = json!({"credential_source": "none",
        "reachability": "unknown", "auth": "unverified",
        "system_resolution": "not_checked", "ipv4": [], "ipv6": []});
    let (password, credential_source) = credentials(device)?;
    report["credential_source"] = json!(credential_source);
    let client = DeviceClient::new(device)?;
    let lookup = system_resolution(device)?;
    let response = client
        .http()
        .get(client.endpoint("api/v1/auth/status")?)
        .timeout(Duration::from_secs(15))
        .send();
    report["reachability"] = json!(if response.is_ok() {
        "reachable"
    } else {
        "unreachable"
    });
    if let Some(auth) = response
        .ok()
        .filter(|r| r.status().is_success())
        .and_then(|r| r.json::<Value>().ok())
    {
        if auth["auth_disabled"] == true {
            report["auth"] = json!("disabled");
        } else if let Some(password) = password {
            // Status never changes an unconfigured device's password.
            if auth["needs_setup"] != true && auth["enforcement"] == true {
                if client.authenticate_password(&auth, &password).is_ok() {
                    report["auth"] = json!("ok");
                    if client.logout().is_err() {
                        eprintln!("teka: could not log out status session for {device}");
                    }
                } else {
                    report["auth"] = json!("failed");
                }
            }
        }
    }
    report
        .as_object_mut()
        .unwrap()
        .append(lookup().as_object_mut().unwrap());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_labels_omit_domains_and_keep_nondefault_ports() {
        for (url, expected) in [
            ("http://teka-123.local/", "teka-123"),
            ("https://teka-123.sfo.example:8443/base/", "teka-123:8443"),
            ("http://teka-123:80/", "teka-123"),
            ("http://teka-123/", "teka-123"),
            ("http://192.0.2.1:8080/", "192.0.2.1:8080"),
            ("http://[fd00::1]/", "[fd00::1]"),
        ] {
            assert_eq!(device_label(url).unwrap(), expected);
        }
    }

    #[test]
    fn serial_identity_deduplicates_aliases_after_matching() {
        let devices: Vec<_> = [
            ("http://gtw-onx.local/", Some("SERIAL-A")),
            ("http://gtw-onx.sfo.example/", Some("SERIAL-A")),
            ("http://[fd00::1]/", Some("SERIAL-A")),
            ("http://gtw-ona.local/", Some("SERIAL-B")),
            ("http://unknown.local/", None),
            ("http://empty.local/", Some(" ")),
        ]
        .into_iter()
        .map(|(url, sn)| DiscoveredDevice {
            name: url.into(),
            url: url.into(),
            hw: None,
            pn: None,
            sn: sn.map(str::to_owned),
            version: None,
        })
        .collect();
        assert_eq!(discovered_count(&devices, &[]), 4);
        assert_eq!(discovered_count(&devices, &["GTW-ON".into()]), 2);
        let mut candidates: BTreeMap<_, _> = devices
            .into_iter()
            .map(|device| {
                (
                    device.url,
                    Metadata {
                        pn: device.pn,
                        sn: device.sn,
                    },
                )
            })
            .collect();
        let matched =
            resolve_patterns(&candidates, &["gtw-onx.local".into(), "gtw-onx.sfo".into()]).unwrap();
        assert_eq!(matched.len(), 2);
        assert_eq!(unique_urls(&candidates, matched).len(), 1);
        assert_eq!(
            unique_urls(
                &candidates,
                vec!["http://unknown.local/".into(), "http://empty.local/".into()]
            )
            .len(),
            2
        );
        candidates.insert(
            "http://192.0.2.1:80/".into(),
            Metadata {
                pn: None,
                sn: Some("ONE".into()),
            },
        );
        candidates.insert(
            "http://192.0.2.1:81/".into(),
            Metadata {
                pn: None,
                sn: Some("TWO".into()),
            },
        );
        assert_eq!(
            unique_urls(
                &candidates,
                vec!["http://192.0.2.1:80/".into(), "http://192.0.2.1:81/".into()]
            )
            .len(),
            2
        );
    }

    #[test]
    fn patterns_match_hostname_part_or_serial_and_deduplicate() {
        let devices: BTreeMap<String, Metadata> = BTreeMap::from([
            (
                "http://box.local/".into(),
                Metadata {
                    pn: Some("OS1-64".into()),
                    sn: Some("AB1234CD".into()),
                },
            ),
            ("http://other.local/".into(), Metadata::default()),
        ]);
        for (pattern, count) in [
            ("BOX.LOCAL", 1),
            ("os1-64", 1),
            ("1234", 1),
            ("", 2),
            ("missing", 0),
            ("http", 0),
            ("HTTP://BOX.local:80", 1),
        ] {
            assert_eq!(
                devices
                    .iter()
                    .filter(|(url, metadata)| matches_pattern(url, metadata, pattern))
                    .count(),
                count,
                "{pattern:?}"
            );
        }
        assert_eq!(
            resolve_patterns(&devices, &["box".into(), "1234".into(), "other".into()])
                .unwrap()
                .len(),
            2
        );
        assert!(resolve_patterns(&devices, &["box".into(), "missing".into()]).is_err());
    }

    #[test]
    fn scoped_ipv6_address_keeps_its_interface() {
        assert_eq!(address_host("[fe80::1%3]:0".parse().unwrap()), "fe80::1%3");
    }

    #[test]
    fn system_resolution_accepts_ip_literals() {
        for url in ["http://127.0.0.1/", "http://[::1]/"] {
            let report = system_resolution(url).unwrap()();
            assert_eq!(report["system_resolution"], "ok");
        }
    }
}

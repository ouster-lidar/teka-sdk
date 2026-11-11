use anyhow::{bail, Context, Result};
use mdns_sd::{HostnameResolutionEvent, ResolvedService, ScopedIp, ServiceDaemon, ServiceEvent};
use reqwest::{dns, Url};
use scopeguard::guard;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

const SERVICE: &str = "_teka._tcp.local.";
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub name: String,
    pub url: String,
    pub hw: Option<String>,
    pub sn: Option<String>,
    pub pn: Option<String>,
    pub version: Option<String>,
}

pub fn discover(timeout: Duration) -> Result<Vec<DiscoveredDevice>> {
    // Dropping a daemon handle alone leaves its worker thread running.
    let daemon = guard(ServiceDaemon::new()?, |daemon| drop(daemon.shutdown()));
    let receiver = daemon.browse(SERVICE).context("browse Teka devices")?;
    let deadline = Instant::now()
        .checked_add(timeout)
        .context("discovery timeout is too large")?;
    let mut devices = BTreeMap::new();
    while Instant::now() < deadline {
        match receiver.recv_deadline(deadline) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                if let Ok(device) = discovered_device(&info) {
                    devices.insert(info.fullname.to_ascii_lowercase(), device);
                }
            }
            Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                devices.remove(&fullname.to_ascii_lowercase());
            }
            Ok(ServiceEvent::SearchStopped(_)) => bail!("mDNS discovery stopped unexpectedly"),
            Ok(_) => {}
            Err(mdns_sd::RecvTimeoutError::Timeout) => break,
            Err(error) => return Err(error).context("receive mDNS discovery result"),
        }
    }
    let mut devices: Vec<_> = devices.into_values().collect();
    devices.sort_by(|a, b| a.name.cmp(&b.name).then(a.url.cmp(&b.url)));
    Ok(devices)
}

fn discovered_device(info: &ResolvedService) -> Result<DiscoveredDevice> {
    let fullname = &info.fullname;
    let suffix = format!(".{SERVICE}");
    let name = if fullname.to_ascii_lowercase().ends_with(&suffix) {
        &fullname[..fullname.len() - suffix.len()]
    } else {
        fullname
    };
    let mut url = Url::parse("http://localhost/")?;
    url.set_host(Some(info.host.trim_end_matches('.')))
        .context("invalid mDNS service hostname")?;
    url.set_port(Some(info.port)).unwrap();
    let property = |key| info.get_property_val_str(key).map(str::to_owned);
    Ok(DiscoveredDevice {
        name: name.to_owned(),
        url: url.into(),
        hw: property("hw"),
        sn: property("sn"),
        pn: property("pn"),
        version: property("v"),
    })
}

/// Uses mDNS for .local names and the operating system resolver for other names.
pub struct MdnsResolver;

impl dns::Resolve for MdnsResolver {
    fn resolve(&self, name: dns::Name) -> dns::Resolving {
        let name = name.as_str().to_ascii_lowercase();
        Box::pin(async move {
            let addresses = if name.trim_end_matches('.').ends_with(".local") {
                resolve_local(&name).await?
            } else {
                tokio::net::lookup_host((name.as_str(), 0)).await?.collect()
            };
            Ok(Box::new(addresses.into_iter()) as dns::Addrs)
        })
    }
}

async fn resolve_local(host: &str) -> Result<Vec<SocketAddr>> {
    let daemon = guard(ServiceDaemon::new()?, |daemon| drop(daemon.shutdown()));
    let hostname = format!("{}.", host.trim_end_matches('.'));
    let receiver = daemon
        .resolve_hostname(&hostname, Some(RESOLVE_TIMEOUT.as_millis() as u64))
        .context("resolve mDNS hostname")?;
    let deadline = tokio::time::Instant::now() + RESOLVE_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, receiver.recv_async()).await?? {
            HostnameResolutionEvent::AddressesFound(_, found) => {
                // SocketAddr's natural order gives the preference for these accepted ranges.
                let addresses: BTreeSet<_> = found
                    .into_iter()
                    .filter_map(|address| {
                        let scope = match &address {
                            ScopedIp::V6(ipv6) => ipv6.scope_id().index,
                            _ => 0,
                        };
                        socket_address(address.to_ip_addr(), scope)
                    })
                    .collect();
                if !addresses.is_empty() {
                    return Ok(addresses.into_iter().collect());
                }
            }
            HostnameResolutionEvent::SearchTimeout(_)
            | HostnameResolutionEvent::SearchStopped(_) => break,
            _ => {}
        }
        tokio::task::yield_now().await;
    }
    bail!("mDNS returned no usable addresses for {host}")
}

fn socket_address(address: IpAddr, scope: u32) -> Option<SocketAddr> {
    let mut socket = SocketAddr::new(address, 0);
    if let SocketAddr::V6(v6) = &mut socket {
        let ip = v6.ip();
        if ip.is_unicast_link_local() && scope != 0 {
            v6.set_scope_id(scope);
        } else if ip.segments()[0] & 0xe000 != 0x2000 && !ip.is_unique_local() {
            return None;
        }
    }
    Some(socket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdns_sd::ServiceInfo;

    #[test]
    fn discovery_rejects_malformed_srv_hosts() {
        let mut info = ServiceInfo::new(SERVICE, "Box", "box.local.", "192.0.2.1", 80, None)
            .unwrap()
            .as_resolved_service();
        for host in ["user@box.local.", "box.local/path", "box.local?query"] {
            info.host = host.into();
            assert!(discovered_device(&info).is_err());
        }
    }

    #[test]
    fn address_ordering_preserves_only_link_local_scope() {
        let addresses: BTreeSet<_> = ["fe80::1", "fd00::1", "2001:db8::1", "192.0.2.1"]
            .map(|ip| socket_address(ip.parse().unwrap(), 7).unwrap())
            .into();
        let expected = [
            "192.0.2.1:0",
            "[2001:db8::1]:0",
            "[fd00::1]:0",
            "[fe80::1%7]:0",
        ]
        .map(|address| address.parse::<SocketAddr>().unwrap());
        assert!(addresses.into_iter().eq(expected));
        for ip in ["fe80::1", "::", "::1", "ff02::1"] {
            assert_eq!(socket_address(ip.parse().unwrap(), 0), None);
        }
    }
}

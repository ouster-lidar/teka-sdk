use crate::build_check;
use anyhow::Result;
use clap::Subcommand;
use std::{
    fmt::Write,
    io::{IsTerminal, Write as _},
    time::{Duration, Instant},
};
use teka::client::discovery::{discover, DiscoveredDevice};

#[derive(Debug, Subcommand)]
pub enum DeviceCommand {
    /// Find Teka devices on the local network
    Discover {
        /// Seconds to browse before printing results
        #[arg(long, default_value = "5", value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        #[arg(long)]
        json: bool,
    },
}

pub fn run(command: DeviceCommand) -> Result<()> {
    match command {
        DeviceCommand::Discover { timeout, json } => {
            if !json {
                build_check::progress("Discovering Teka devices...");
            }
            let started = Instant::now();
            let devices = discover(Duration::from_secs(timeout));
            if json {
                println!("{}", serde_json::to_string(&devices?)?);
            } else {
                if let Ok(devices) = &devices {
                    let color =
                        std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
                    print!("{}", discovery_report(devices, color));
                    std::io::stdout().flush()?;
                }
                let devices = build_check::finish(
                    "Discovery",
                    devices,
                    |devices| {
                        format!(
                            "{} device{} found",
                            devices.len(),
                            if devices.len() == 1 { "" } else { "s" }
                        )
                    },
                    Some(started),
                )?;
                build_check::note(if devices.is_empty() {
                    "Check that your device is on the same network."
                } else {
                    "Connect: teka device connect <URL>"
                });
            }
            Ok(())
        }
    }
}

fn discovery_report(devices: &[DiscoveredDevice], color: bool) -> String {
    let (bold, cyan, reset) = if color {
        ("\x1b[1m", "\x1b[36m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    let mut output = String::new();
    for device in devices {
        writeln!(
            output,
            "🖥︎ {bold}{}{reset}\n  {cyan}{}{reset}",
            device.name.escape_debug(),
            device.url.escape_debug()
        )
        .unwrap();
        for (label, value) in [
            ("Hardware", &device.hw),
            ("Serial", &device.sn),
            ("Part", &device.pn),
            ("Version", &device.version),
        ] {
            if let Some(value) = value.as_deref().filter(|v| !v.is_empty()) {
                writeln!(output, "  {label}: {}", value.escape_debug()).unwrap();
            }
        }
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_output_labels_devices_and_escapes_network_text() {
        let devices = [DiscoveredDevice {
            name: "Teka box\x1b[2J".into(),
            url: "http://box.local/".into(),
            hw: Some("ONX2".into()),
            sn: None,
            pn: None,
            version: Some("1.2".into()),
        }];
        let plain = discovery_report(&devices, false);
        assert_eq!(
            plain,
            "🖥︎ Teka box\\u{1b}[2J\n  http://box.local/\n  Hardware: ONX2\n  Version: 1.2\n\n"
        );
        assert!(discovery_report(&devices, true).contains("\x1b[36mhttp://box.local/\x1b[0m"));
        assert!(discovery_report(&[], false).is_empty());
    }
}

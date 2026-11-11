//! Supervise a Teka app and its TCP/UDP leases.

use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::time::Duration;

use rustix::process::{kill_process_group, Pid, Signal};
use teka::app::runtime::{self, ResourceAvailability};
use tokio::process::{Child, Command};
use tokio::signal::unix::{signal, SignalKind};

fn main() {
    match std::env::args_os()
        .nth(1)
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
    {
        Some("--help" | "-h") => {
            println!("Run an app with Teka registration, resource leases, and lifecycle management.\n\nUsage: teka-runner APP_ID tcp|udp PORT[-PORT] COMMAND [ARG...]");
            return;
        }
        Some("--version" | "-V") => {
            println!("teka-runner {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {}
    }
    match main_result() {
        Ok(status) => exit_with_status(status),
        Err(error) => {
            eprintln!("teka-runner: {error}");
            std::process::exit(1);
        }
    }
}

fn exit_with_status(status: ExitStatus) -> ! {
    if let Some(code) = status.code() {
        std::process::exit(code);
    }
    let signal = status.signal().unwrap_or(libc::SIGTERM);
    let _ = signal_hook::low_level::emulate_default_handler(signal);
    std::process::exit(128 + signal);
}

fn main_result() -> io::Result<ExitStatus> {
    let mut args = std::env::args_os().skip(1);
    let id = args
        .next()
        .and_then(|v| v.into_string().ok())
        .ok_or_else(usage)?;
    let proto = args
        .next()
        .and_then(|v| v.into_string().ok())
        .filter(|v| v == "tcp" || v == "udp")
        .ok_or_else(usage)?;
    let range = args
        .next()
        .and_then(|v| v.into_string().ok())
        .ok_or_else(usage)?;
    let ports = ports(&range)?;
    let program = args.next().ok_or_else(usage)?;
    std::env::set_var("APP_ID", &id);
    let executor = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = executor.block_on(async {
        let mut terminate = signal(SignalKind::terminate())?;
        let mut interrupt = signal(SignalKind::interrupt())?;
        let acquire_id = id.clone();
        let acquire = tokio::task::spawn_blocking(move || {
            runtime::activate_resources(&acquire_id)?;
            match proto.as_str() {
                "tcp" => runtime::sync_tcp_ports(&acquire_id, &ports),
                _ => runtime::sync_udp_ports(&acquire_id, &ports),
            }
        });
        tokio::select! {
            biased;
            _ = terminate.recv() => return Ok(ExitStatus::from_raw(libc::SIGTERM)),
            _ = interrupt.recv() => return Ok(ExitStatus::from_raw(libc::SIGINT)),
            result = acquire => result.map_err(io::Error::other)?.map_err(io::Error::other)?,
        }
        let mut availability = runtime::resource_availability();
        runtime::start_supervisor().map_err(io::Error::other)?;
        let registered = async {
            loop {
                if matches!(
                    *availability.borrow_and_update(),
                    ResourceAvailability::Available
                ) {
                    return Ok::<_, io::Error>(());
                }
                availability.changed().await.map_err(io::Error::other)?;
            }
        };
        tokio::select! {
            biased;
            _ = terminate.recv() => return Ok(ExitStatus::from_raw(libc::SIGTERM)),
            _ = interrupt.recv() => return Ok(ExitStatus::from_raw(libc::SIGINT)),
            result = registered => result?,
        }
        let mut child = Command::new(program)
            .args(args)
            .process_group(0)
            .kill_on_drop(true)
            .spawn()?;
        let group = ProcessGroup(
            Pid::from_raw(
                child
                    .id()
                    .ok_or_else(|| io::Error::other("child has no PID"))? as i32,
            )
            .ok_or_else(|| io::Error::other("invalid child PID"))?,
        );
        runtime::notify_ready();
        let resource_loss = async {
            loop {
                availability.changed().await.map_err(io::Error::other)?;
                let state = availability.borrow_and_update().clone();
                if state != ResourceAvailability::Available {
                    return Err(io::Error::other(format!(
                        "runtime resources unavailable: {state:?}"
                    )));
                }
            }
        };
        let (observed, shutdown_signal) = tokio::select! {
            status = child.wait() => (Some(status), Signal::TERM),
            _ = terminate.recv() => (None, Signal::TERM),
            _ = interrupt.recv() => (None, Signal::INT),
            loss = resource_loss => (Some(loss), Signal::TERM),
        };
        stop_child(&mut child, &group, shutdown_signal).await;
        match observed {
            Some(status) => status,
            None => child.wait().await,
        }
    });
    if let Err(error) = runtime::stop_and_release(&id) {
        eprintln!("teka-runner: {error}");
    }
    result
}

fn usage() -> io::Error {
    io::Error::other("usage: teka-runner APP_ID tcp|udp PORT[-PORT] COMMAND [ARG...]")
}

fn ports(range: &str) -> io::Result<Vec<u16>> {
    let (first, last) = range.split_once('-').unwrap_or((range, range));
    let first: u16 = first.parse().map_err(|_| usage())?;
    let last: u16 = last.parse().map_err(|_| usage())?;
    if first == 0 || first > last {
        return Err(usage());
    }
    Ok((first..=last).collect())
}

struct ProcessGroup(Pid);

impl ProcessGroup {
    fn signal(&self, signal: Signal) {
        let _ = kill_process_group(self.0, signal);
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        // Descendants may outlive the group leader or ignore its shutdown signal.
        self.signal(Signal::KILL);
    }
}

async fn stop_child(child: &mut Child, group: &ProcessGroup, signal: Signal) {
    group.signal(signal);
    if tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .is_err()
    {
        group.signal(Signal::KILL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_status_preserves_codes_and_signals() {
        const RAW_STATUS: &str = "TEKA_RUNNER_TEST_RAW_STATUS";
        if let Ok(raw) = std::env::var(RAW_STATUS) {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let _enter = runtime.enter();
            let _terminate = signal(SignalKind::terminate()).unwrap();
            let _interrupt = signal(SignalKind::interrupt()).unwrap();
            exit_with_status(ExitStatus::from_raw(raw.parse().unwrap()));
        }
        for raw in [0, 42 << 8, libc::SIGTERM, libc::SIGINT, libc::SIGKILL] {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "tests::exit_status_preserves_codes_and_signals"])
                .env(RAW_STATUS, raw.to_string())
                .status()
                .unwrap();
            let expected = ExitStatus::from_raw(raw);
            assert_eq!(status.code(), expected.code());
            assert_eq!(status.signal(), expected.signal());
        }
    }

    #[test]
    fn process_group_forwards_signals_and_kills_on_drop() {
        use std::os::unix::process::CommandExt;

        for signal in [Some(Signal::TERM), Some(Signal::INT), None] {
            let mut child = std::process::Command::new("sleep")
                .arg("60")
                .process_group(0)
                .spawn()
                .unwrap();
            let mut peer = std::process::Command::new("sleep")
                .arg("60")
                .process_group(child.id() as i32)
                .spawn()
                .unwrap();
            let group = ProcessGroup(Pid::from_raw(child.id() as i32).unwrap());
            if let Some(signal) = signal {
                group.signal(signal);
                assert_eq!(child.wait().unwrap().signal(), Some(signal.as_raw()));
                assert_eq!(peer.wait().unwrap().signal(), Some(signal.as_raw()));
            } else {
                drop(group);
                assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
                assert_eq!(peer.wait().unwrap().signal(), Some(libc::SIGKILL));
            }
        }
    }

    #[test]
    fn range_is_inclusive_and_configurable() {
        let range = ports("7000-8000").unwrap();
        assert_eq!(range.len(), 1001);
        assert_eq!((range[0], range[1000]), (7000, 8000));
        assert_eq!(ports("9000-9002").unwrap(), [9000, 9001, 9002]);
        assert_eq!(ports("65535").unwrap(), [65535]);
    }

    #[test]
    fn invalid_ranges_are_rejected() {
        for range in ["0", "8000-7000", "7000-65536", "udp", "7000-8000-9000"] {
            assert!(ports(range).is_err(), "{range}");
        }
    }
}

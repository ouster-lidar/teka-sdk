use crate::{
    build::{self, BuildSource},
    device::DeviceClient,
};
use anyhow::{bail, Context, Result};
use clap::Args;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    num::NonZeroUsize,
    path::PathBuf,
    thread,
    time::Duration,
};

/// Pause after remove before install; the device needs a moment to tear down the unit.
const POST_REMOVE_SLEEP: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Args)]
pub struct DeployOptions {
    /// URL of a Teka device; may be repeated
    #[arg(
        short = 'H',
        long = "host",
        env = "TEKA_HOST",
        value_name = "URL",
        required = true
    )]
    pub hosts: Vec<String>,
    /// App IDs to deploy from ./result, or build when --build is set
    #[arg(value_name = "APP", required = true, num_args = 1..)]
    pub apps: Vec<String>,
    /// Build apps for the device before deploying
    #[arg(long)]
    pub build: bool,
    /// App installation profile
    #[arg(long, default_value = "trusted")]
    pub profile: String,
    /// Remove existing apps before install instead of replacing in place
    #[arg(long)]
    pub remove: bool,
    /// Maximum number of devices to deploy to concurrently
    #[arg(long, default_value = "4")]
    pub jobs: NonZeroUsize,
    #[command(flatten)]
    pub source: BuildSource,
}

fn env_seconds(name: &str, default: u64) -> Result<u64> {
    match std::env::var(name) {
        Ok(value) => value.parse().with_context(|| format!("invalid {name}")),
        Err(_) => Ok(default),
    }
}

fn verify_logs_for_host(client: &DeviceClient, app: &str, host: &str) -> Result<()> {
    std::thread::sleep(Duration::from_secs(env_seconds("TEKA_POST_DEPLOY_SEC", 3)?));
    let logs = client.logs(app, env_seconds("TEKA_LOG_SEC", 10)?)?;
    for line in logs.lines().take(200) {
        eprintln!("teka: {host}: {line}");
    }
    check_logs(&logs)
}

pub fn check_logs(logs: &str) -> Result<()> {
    const BAD_LINES: &[&str] = &[
        "Invalid portable image reference",
        "Failed to initialize the GLX module",
        "eglGetDisplay failed",
        "glXQueryVersion failed",
        "Failed at step NAMESPACE",
        "Failed to set up mount namespacing",
        "status=226/NAMESPACE",
        "status=203/EXEC",
    ];
    if logs.lines().any(|line| {
        BAD_LINES.iter().any(|pattern| line.contains(pattern))
            || (line.contains("libnvidia-glcore.so.")
                && line.contains("cannot open shared object file"))
    }) {
        bail!("app logs contain a known image, graphics, namespace, or executable failure");
    }
    Ok(())
}

struct DeployTarget {
    host: String,
    target: String,
    client: DeviceClient,
}

fn prepare_targets(hosts: &[String]) -> Result<Vec<DeployTarget>> {
    let mut seen = HashSet::new();
    let mut devices = Vec::with_capacity(hosts.len());
    for host in hosts {
        let client = DeviceClient::new(host).with_context(|| format!("invalid host {host}"))?;
        let normalized = client.endpoint("")?.to_string();
        if !seen.insert(normalized) {
            return Err(crate::plugins::usage(format!(
                "duplicate host URL '{host}'"
            )));
        }
        devices.push((host, client));
    }

    let mut targets = Vec::with_capacity(devices.len());
    for (host, client) in devices {
        client
            .authenticate()
            .with_context(|| format!("authenticate {host}"))?;
        let target = format!(
            "{}-linux",
            client
                .architecture()
                .with_context(|| format!("read architecture from {host}"))?
        );
        targets.push(DeployTarget {
            host: host.clone(),
            target,
            client,
        });
    }
    Ok(targets)
}

fn result_farm() -> Option<String> {
    let path = std::fs::read_link(build::RESULT).ok()?;
    let name = path.file_name()?.to_string_lossy();
    name.split_once("-teka-").map(|(_, built)| built.to_owned())
}

fn result_paths(
    apps: &[String],
    targets: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<PathBuf>>> {
    let farm = result_farm();
    if targets.len() > 1 && farm.as_deref() != Some("multi") {
        bail!("mixed-architecture deploy needs a multi-target result; rerun with --build");
    }

    targets
        .iter()
        .map(|target| {
            let root = if farm.as_deref() == Some("multi") {
                let root = PathBuf::from(build::RESULT).join(target);
                if !root.is_dir() {
                    bail!("result has no images for {target}; rebuild with --build");
                }
                root
            } else {
                if let Some(built) = farm.as_deref() {
                    if built != target {
                        bail!("result is for {built}; device is {target}. Rebuild with --build");
                    }
                }
                PathBuf::from(build::RESULT)
            };
            build::raw_paths_at(apps, &root).map(|paths| (target.clone(), paths))
        })
        .collect()
}

fn deploy_target(
    target: &DeployTarget,
    apps: &[String],
    files: &[PathBuf],
    profile: &str,
    remove: bool,
) -> Result<()> {
    for (app, raw) in apps.iter().zip(files) {
        eprintln!("teka: {}: deploying {app}", target.host);
        if remove {
            target.client.remove_app(app)?;
            thread::sleep(POST_REMOVE_SLEEP);
        }
        let response = target.client.install(raw, profile)?;
        let app_id = response["app_id"]
            .as_str()
            .context("device install response missing app_id")?;
        verify_logs_for_host(&target.client, app_id, &target.host)
            .with_context(|| format!("verify {app} logs"))?;
    }
    Ok(())
}

fn deploy_targets(
    targets: &[DeployTarget],
    apps: &[String],
    files: &BTreeMap<String, Vec<PathBuf>>,
    profile: &str,
    remove: bool,
    jobs: usize,
) -> Result<()> {
    let mut failures = Vec::new();
    for batch in targets.chunks(jobs) {
        let results = thread::scope(|scope| {
            let handles = batch
                .iter()
                .map(|target| {
                    let target_files = &files[&target.target];
                    scope.spawn(move || deploy_target(target, apps, target_files, profile, remove))
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("deployment worker panicked")))
                })
                .collect::<Vec<_>>()
        });

        for (target, result) in batch.iter().zip(results) {
            match result {
                Ok(()) => eprintln!("teka: {}: deploy succeeded", target.host),
                Err(err) => {
                    let message = format!("{}: {err:#}", target.host);
                    eprintln!("teka: {message}");
                    failures.push(message);
                }
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "deployment failed on {} host(s): {}",
            failures.len(),
            failures.join("; ")
        )
    }
}

pub fn run(args: DeployOptions) -> Result<()> {
    let apps = &args.apps;
    build::validate_apps(apps)?;
    let targets = prepare_targets(&args.hosts)?;
    let target_arches = targets
        .iter()
        .map(|target| target.target.clone())
        .collect::<BTreeSet<_>>();
    let files = if args.build {
        build::build_apps_for_targets(
            &args.source,
            apps,
            &target_arches.iter().cloned().collect::<Vec<_>>(),
        )?
    } else {
        result_paths(apps, &target_arches)?
    };
    deploy_targets(
        &targets,
        apps,
        &files,
        &args.profile,
        args.remove,
        args.jobs.get(),
    )
}

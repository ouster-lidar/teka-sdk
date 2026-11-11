use crate::{
    build::{self, BuildSource},
    device::DeviceClient,
};
use anyhow::{bail, ensure, Context, Result};
use clap::Args;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

/// Pause after remove before install; the device needs a moment to tear down the unit.
const POST_REMOVE_SLEEP: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Args)]
pub struct DeployOptions {
    /// URL of a Teka device; may be repeated
    #[arg(
        long = "device",
        env = "TEKA_DEVICE",
        value_name = "URL",
        required = true
    )]
    pub hosts: Vec<String>,
    /// App IDs from ./result, or prebuilt .tpk files (default: the project's only app)
    #[arg(value_name = "INPUT")]
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
            build::package_paths_at(apps, &root).map(|paths| (target.clone(), paths))
        })
        .collect()
}

fn deploy_target(
    target: &DeployTarget,
    deployments: &[Deployment],
    profile: &str,
    remove: bool,
) -> Result<()> {
    for deployment in deployments {
        eprintln!("teka: {}: deploying {}", target.host, deployment.input);
        if remove {
            target.client.remove_app(&deployment.app)?;
            thread::sleep(POST_REMOVE_SLEEP);
        }
        let response = target.client.install(&deployment.path, profile)?;
        let app_id = response["app_id"]
            .as_str()
            .context("device install response missing app_id")?;
        verify_logs_for_host(&target.client, app_id, &target.host)
            .with_context(|| format!("verify {} logs", deployment.input))?;
    }
    Ok(())
}

struct Deployment {
    input: String,
    app: String,
    path: PathBuf,
}

fn is_file_input(input: &str) -> bool {
    input.contains('/') || Path::new(input).extension().is_some()
}

fn prepare_deployments(
    inputs: &[String],
    named_paths: &[PathBuf],
    target: &str,
    file_manifests: &BTreeMap<String, teka::tpk::Manifest>,
) -> Result<Vec<Deployment>> {
    let arch = target
        .strip_suffix("-linux")
        .context("invalid device target")?;
    let mut named_paths = named_paths.iter();
    inputs
        .iter()
        .map(|input| {
            let file_input = is_file_input(input);
            let path = if file_input {
                PathBuf::from(input)
            } else {
                named_paths
                    .next()
                    .context("missing package for named app")?
                    .clone()
            };
            let named_manifest;
            let manifest = if file_input {
                file_manifests
                    .get(input)
                    .context("missing file preflight")?
            } else {
                named_manifest = teka::tpk::read_single(&path, teka::tpk::ComponentType::App)?;
                &named_manifest
            };
            ensure!(
                manifest.arch.as_str() == arch,
                "{} is for {}; device is {arch}",
                path.display(),
                manifest.arch.as_str()
            );
            let app = &manifest.components[0].id;
            ensure!(
                file_input || app == input,
                "{} has app ID {app}, expected {input}",
                path.display()
            );
            Ok(Deployment {
                input: input.clone(),
                app: app.clone(),
                path,
            })
        })
        .collect()
}

fn deploy_targets(
    targets: &[DeployTarget],
    deployments: &BTreeMap<String, Vec<Deployment>>,
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
                    let target_deployments = &deployments[&target.target];
                    scope.spawn(move || deploy_target(target, target_deployments, profile, remove))
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

pub fn run(mut args: DeployOptions) -> Result<()> {
    let has_files = args.apps.iter().any(|input| is_file_input(input));
    if has_files && (args.build || args.remove) {
        return Err(crate::plugins::usage(
            "file inputs cannot be used with --build or --remove",
        ));
    }
    let flake = if args.apps.is_empty() {
        let (flake, apps) = build::resolve_apps(&args.source, &args.apps)?;
        args.apps = apps;
        Some(flake)
    } else {
        None
    };
    let apps = args
        .apps
        .iter()
        .filter(|input| !is_file_input(input))
        .cloned()
        .collect::<Vec<_>>();
    build::validate_apps(&apps)?;
    let mut file_manifests = BTreeMap::new();
    for input in args.apps.iter().filter(|input| is_file_input(input)) {
        let manifest = teka::tpk::read_single(Path::new(input), teka::tpk::ComponentType::App)?;
        file_manifests.insert(input.clone(), manifest);
    }
    let targets = prepare_targets(&args.hosts)?;
    let target_arches = targets
        .iter()
        .map(|target| target.target.clone())
        .collect::<BTreeSet<_>>();
    let files = if args.build {
        let architectures = target_arches.iter().cloned().collect::<Vec<_>>();
        let flake = match flake {
            Some(flake) => flake,
            None => build::resolve_apps(&args.source, &apps)?.0,
        };
        build::build_targets(&flake, &apps, &architectures)?
    } else if apps.is_empty() {
        target_arches
            .iter()
            .map(|target| (target.clone(), Vec::new()))
            .collect()
    } else {
        result_paths(&apps, &target_arches)?
    };
    let deployments = target_arches
        .iter()
        .map(|target| {
            prepare_deployments(&args.apps, &files[target], target, &file_manifests)
                .map(|deployments| (target.clone(), deployments))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    deploy_targets(
        &targets,
        &deployments,
        &args.profile,
        args.remove,
        args.jobs.get(),
    )
}

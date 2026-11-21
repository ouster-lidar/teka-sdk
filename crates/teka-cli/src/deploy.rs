use crate::{
    build::{self, BuildSource},
    device::DeviceClient,
    device_commands::device_label,
    target::BoxOptions,
};
use anyhow::{bail, ensure, Context, Result};
use clap::Args;
use std::{
    collections::{BTreeMap, HashSet},
    num::NonZeroUsize,
    path::PathBuf,
    thread,
    time::Duration,
};
use teka::tpk::{self, Architecture, ComponentType};

/// Pause after remove before install; the device needs a moment to tear down the unit.
const POST_REMOVE_SLEEP: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Args)]
pub struct DeployOptions {
    #[command(flatten)]
    pub target: BoxOptions,
    /// App IDs to build, or paths to prebuilt .tpk files (default: the app in this directory or the project's only app)
    #[arg(value_name = "INPUT", value_hint = clap::ValueHint::AnyPath)]
    pub apps: Vec<String>,
    /// App installation profile
    #[arg(long, default_value = "trusted")]
    pub profile: String,
    /// Remove existing apps before install (app IDs only)
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
            host: device_label(host)?,
            target,
            client,
        });
    }
    Ok(targets)
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
    arch: Architecture,
}

impl Deployment {
    fn read(path: PathBuf, expected_app: Option<&str>) -> Result<Self> {
        let manifest = tpk::read_single(&path, ComponentType::App)?;
        let app = &manifest.components[0].id;
        let input = if let Some(expected) = expected_app {
            ensure!(
                app == expected,
                "{} has app ID {app}, expected {expected}",
                path.display()
            );
            expected.to_owned()
        } else {
            path.display().to_string()
        };
        Ok(Self {
            input,
            app: app.clone(),
            path,
            arch: manifest.arch,
        })
    }
}

fn is_file_input(input: &str) -> bool {
    input.ends_with(".tpk") || input.contains('/')
}

fn deploy_targets<'a>(
    targets: &[DeployTarget],
    deployments: impl Fn(&str) -> &'a [Deployment],
    profile: &str,
    remove: bool,
    jobs: usize,
) -> Result<()> {
    for target in targets {
        let arch = target.target.trim_end_matches("-linux");
        for deployment in deployments(&target.target) {
            ensure!(
                deployment.arch.as_str() == arch,
                "{} is for {}; device is {arch}",
                deployment.path.display(),
                deployment.arch.as_str()
            );
        }
    }

    let mut failures = Vec::new();
    for batch in targets.chunks(jobs) {
        let results = thread::scope(|scope| {
            let handles = batch
                .iter()
                .map(|target| {
                    let target_deployments = deployments(&target.target);
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

pub fn run(args: DeployOptions) -> Result<()> {
    let file_count = args
        .apps
        .iter()
        .filter(|input| is_file_input(input))
        .count();
    if file_count > 0 && file_count != args.apps.len() {
        return Err(crate::plugins::usage(
            "deploy app names and package paths in separate invocations",
        ));
    }
    if file_count > 0 && args.remove {
        return Err(crate::plugins::usage(
            "file inputs cannot be used with --remove",
        ));
    }

    let hosts = args.target.required()?;
    if file_count > 0 {
        let deployments = args
            .apps
            .into_iter()
            .map(|input| Deployment::read(input.into(), None))
            .collect::<Result<Vec<_>>>()?;
        let targets = prepare_targets(&hosts)?;
        return deploy_targets(
            &targets,
            |_| &deployments,
            &args.profile,
            false,
            args.jobs.get(),
        );
    }

    let (flake, apps) = build::resolve_apps(&args.source, &args.apps)?;
    let targets = prepare_targets(&hosts)?;
    let architectures = targets.iter().map(|target| target.target.clone()).collect();
    // Keep the Nix GC root alive until the last upload completes.
    let output = tempfile::tempdir().context("create deployment build directory")?;
    let packages = build::build_targets(
        &flake,
        &apps,
        architectures,
        &output.path().join("packages"),
    )?;
    let deployments = packages
        .into_iter()
        .map(|(target, paths)| {
            paths
                .into_iter()
                .zip(&apps)
                .map(|(path, app)| {
                    let path = path
                        .canonicalize()
                        .with_context(|| format!("resolve package {}", path.display()))?;
                    Deployment::read(path, Some(app))
                })
                .collect::<Result<Vec<_>>>()
                .map(|deployments| (target, deployments))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    deploy_targets(
        &targets,
        |target| &deployments[target],
        &args.profile,
        args.remove,
        args.jobs.get(),
    )
}

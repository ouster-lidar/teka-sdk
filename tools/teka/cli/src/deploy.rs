use crate::{
    build::{self, BuildSource},
    device::DeviceClient,
};
use anyhow::{bail, Context, Result};
use clap::Args;
use std::time::Duration;

/// Pause after remove before install; the device needs a moment to tear down the unit.
const POST_REMOVE_SLEEP: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Args)]
#[command(group(
    clap::ArgGroup::new("app")
        .args(["apps", "build"])
        .required(true)
        .multiple(false)
))]
pub struct DeployOptions {
    /// URL of the Teka device
    #[arg(short = 'H', long, env = "TEKA_HOST", value_name = "URL")]
    pub host: String,
    #[arg(value_name = "APP")]
    pub apps: Vec<String>,
    /// Build the named apps for the device before deploying
    #[arg(long, value_name = "APP", num_args = 1..)]
    pub build: Vec<String>,
    #[arg(long, default_value = "trusted")]
    pub profile: String,
    /// Remove existing apps before install instead of replacing in place
    #[arg(long)]
    pub remove: bool,
    #[command(flatten)]
    pub source: BuildSource,
}

fn env_seconds(name: &str, default: u64) -> Result<u64> {
    match std::env::var(name) {
        Ok(value) => value.parse().with_context(|| format!("invalid {name}")),
        Err(_) => Ok(default),
    }
}

pub fn verify_logs(client: &DeviceClient, app: &str) -> Result<()> {
    std::thread::sleep(Duration::from_secs(env_seconds("TEKA_POST_DEPLOY_SEC", 3)?));
    let logs = client.logs(app, env_seconds("TEKA_LOG_SEC", 10)?)?;
    for line in logs.lines().take(200) {
        eprintln!("{line}");
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

/// `result` is a symlink to `…-teka-<target>`. A plain directory has no target to check.
fn ensure_result_target(target: &str) -> Result<()> {
    let Ok(path) = std::fs::read_link(build::RESULT) else {
        return Ok(());
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let Some(built) = name.split_once("-teka-").map(|(_, built)| built) else {
        return Ok(());
    };
    if built != target {
        bail!("result is for {built}; device is {target}. Rebuild with --build");
    }
    Ok(())
}

pub fn run(args: DeployOptions) -> Result<()> {
    let do_build = !args.build.is_empty();
    let apps: &[String] = if do_build { &args.build } else { &args.apps };
    build::validate_apps(apps)?;
    let client = DeviceClient::new(&args.host)?;
    client.authenticate()?;
    let target = format!("{}-linux", client.architecture()?);
    let files = if do_build {
        build::build_apps(&args.source, apps, &target)?
    } else {
        ensure_result_target(&target)?;
        build::raw_paths(apps)?
    };
    for (app, raw) in apps.iter().zip(files) {
        if args.remove {
            client.remove_app(app)?;
            std::thread::sleep(POST_REMOVE_SLEEP);
        }
        let response = client.install(&raw, &args.profile)?;
        let app_id = response["app_id"]
            .as_str()
            .context("device install response missing app_id")?;
        verify_logs(&client, app_id)?;
    }
    Ok(())
}

use crate::{checked_output, device::normalize_arch};
use anyhow::{bail, Context, Result};
use clap::Args;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    io::{IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Debug, Args)]
pub struct BuildSource {
    /// Apps flake directory (default: apps/ at checkout root)
    #[arg(long, env = "TEKA_APPS_FLAKE_ROOT", value_hint = clap::ValueHint::DirPath)]
    pub flake: Option<PathBuf>,
}

/// Out-link for the linkFarm of requested apps.
pub const RESULT: &str = "result";

#[derive(Clone, Debug, Args)]
pub struct BuildOptions {
    /// App IDs to build
    #[arg(value_name = "APP", required = true)]
    pub apps: Vec<String>,
    /// Architecture to build for
    #[arg(long, default_value = "aarch64", value_parser = ["x86_64", "aarch64", "amd64", "arm64"])]
    pub arch: String,
    #[command(flatten)]
    pub source: BuildSource,
}

fn builder_system() -> Result<String> {
    checked_output(Command::new("nix").args([
        "eval",
        "--raw",
        "--impure",
        "--expr",
        "builtins.currentSystem",
    ]))
    .map(|s| s.trim().to_owned())
}

pub fn validate_apps(apps: &[String]) -> Result<()> {
    let mut seen = HashSet::new();
    for app in apps {
        if !teka_pkg::valid_id(app) {
            bail!("invalid app id '{app}'");
        }
        if !seen.insert(app) {
            bail!("duplicate app id '{app}'");
        }
    }
    Ok(())
}

pub fn build_apps(source: &BuildSource, apps: &[String], target: &str) -> Result<Vec<PathBuf>> {
    build_apps_for_targets(source, apps, &[target.to_owned()])?
        .remove(target)
        .context("build did not produce requested target")
}

pub fn build_apps_for_targets(
    source: &BuildSource,
    apps: &[String],
    targets: &[String],
) -> Result<BTreeMap<String, Vec<PathBuf>>> {
    validate_apps(apps)?;
    let targets = targets.iter().cloned().collect::<BTreeSet<_>>();
    if targets.is_empty() {
        bail!("no device targets to build");
    }
    let multi_target = targets.len() > 1;
    let flake = find_apps_flake(source.flake.as_deref())?;
    let known = app_names(&flake)?;
    if let Some(app) = apps.iter().find(|app| !known.contains(app)) {
        bail!("unknown app '{app}'; see `teka app list`");
    }
    let builder = builder_system()?;
    let use_nom = std::io::stderr().is_terminal() && nom_available();
    let mut command = Command::new(if use_nom { "nom" } else { "nix" });
    command
        .args(["build", "--file", "-", "-L", "--out-link", RESULT])
        .args(["--no-write-lock-file", "--argstr", "flake"])
        .arg(flake_reference(&flake)?);
    let expr = if !multi_target {
        result_expr(&builder, targets.first().unwrap(), apps)
    } else {
        multi_result_expr(&builder, &targets, apps)
    };
    run_with_stdin(&mut command, &expr).context("nix build failed")?;

    targets
        .into_iter()
        .map(|target| {
            let root = if multi_target {
                PathBuf::from(RESULT).join(&target)
            } else {
                PathBuf::from(RESULT)
            };
            package_paths_at(apps, &root).map(|paths| (target, paths))
        })
        .collect()
}

pub fn package_paths_at(apps: &[String], root: &Path) -> Result<Vec<PathBuf>> {
    apps.iter()
        .map(|app| {
            let path = root.join(app);
            single_package(&path).with_context(|| format!("no artifact in {}", path.display()))
        })
        .collect()
}

fn app_names(flake: &Path) -> Result<Vec<String>> {
    let output = checked_output(Command::new("nix").args([
        "eval",
        &format!("{}#.teka.apps", flake.display()),
        "--json",
        "--apply",
        "builtins.attrNames",
    ]))
    .with_context(|| format!("read teka.apps from {}", flake.display()))?;
    Ok(serde_json::from_str(&output)?)
}

fn flake_reference(flake: &Path) -> Result<String> {
    let Some(root) = flake.ancestors().find(|dir| dir.join(".git").exists()) else {
        return Ok(flake.to_string_lossy().into_owned());
    };
    let url = reqwest::Url::from_file_path(root)
        .map_err(|_| anyhow::anyhow!("invalid repository path: {}", root.display()))?;
    let mut url = reqwest::Url::parse(&format!("git+{url}"))?;
    url.query_pairs_mut()
        .append_pair("dir", &flake.strip_prefix(root)?.to_string_lossy());
    Ok(url.into())
}

pub fn list(args: BuildSource) -> Result<()> {
    let flake = find_apps_flake(args.flake.as_deref())?;
    let mut names = app_names(&flake)?;
    names.sort();
    for name in names {
        println!("{name}");
    }
    Ok(())
}

pub fn run(args: BuildOptions) -> Result<()> {
    let arch = normalize_arch(&args.arch)?;
    build_apps(&args.source, &args.apps, &format!("{arch}-linux"))?;
    for app in &args.apps {
        println!("{RESULT}/{app}");
    }
    Ok(())
}

pub fn find_apps_flake(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if !path.join("flake.nix").is_file() {
            bail!("no flake.nix in {}", path.display());
        }
        return Ok(path.canonicalize()?);
    }
    let cwd = std::env::current_dir()?;
    // `.git` is a directory in clones and a file in worktrees and submodules.
    let Some(root) = cwd.ancestors().find(|dir| dir.join(".git").exists()) else {
        bail!("not in a git checkout; use --flake or TEKA_APPS_FLAKE_ROOT")
    };
    let apps = root.join("apps");
    if !apps.join("flake.nix").is_file() {
        bail!(
            "no apps/flake.nix in {}; use --flake or TEKA_APPS_FLAKE_ROOT",
            root.display()
        );
    }
    Ok(apps.canonicalize()?)
}

pub fn target_package(builder: &str, app: &str, target: &str) -> (String, String) {
    // Images are Linux. `packages.<darwin>.*` is a cross build that runs on
    // Darwin; skip it and select the Linux target so a Linux builder produces it.
    if !builder.ends_with("-linux") {
        return (target.to_owned(), app.to_owned());
    }
    if builder == target {
        (builder.to_owned(), app.to_owned())
    } else {
        (builder.to_owned(), format!("{app}-cross-{target}"))
    }
}

/// One native derivation depending on the requested packages. Passed to
/// `nix build --file -` so Nix does not treat a temp directory as a flake.
fn result_expr(builder: &str, target: &str, apps: &[String]) -> String {
    let entries = package_entries(builder, target, apps);
    format!(
        r#"{{ flake }}:
let
  apps = builtins.getFlake flake;
  pkgs = apps.inputs.nixpkgs.legacyPackages."{builder}";
in
pkgs.linkFarm "teka-{target}" [
{entries}
]
"#
    )
}

fn multi_result_expr(builder: &str, targets: &BTreeSet<String>, apps: &[String]) -> String {
    let mut farms = Vec::new();
    let mut links = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        farms.push(format!(
            "  target{index} = pkgs.linkFarm \"teka-{target}\" [\n{}\n  ];",
            package_entries(builder, target, apps)
        ));
        links.push(format!(
            "    {{ name = \"{target}\"; path = target{index}; }}"
        ));
    }
    format!(
        r#"{{ flake }}:
let
  apps = builtins.getFlake flake;
  pkgs = apps.inputs.nixpkgs.legacyPackages."{builder}";
{farms}
in
pkgs.linkFarm "teka-multi" [
{links}
]
"#,
        farms = farms.join("\n"),
        links = links.join("\n")
    )
}

fn package_entries(builder: &str, target: &str, apps: &[String]) -> String {
    apps.iter()
        .map(|app| {
            let (system, name) = target_package(builder, app, target);
            format!(
                "    {{\n      name = \"{app}\";\n      path = apps.packages.\"{system}\".\"{name}\";\n    }}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn nom_available() -> bool {
    Command::new("nom")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Stream stderr to the terminal and keep it so the error includes Nix's output.
fn run_with_stdin(command: &mut Command, expr: &str) -> Result<()> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context("start command")?;
    {
        let mut stdin = child.stdin.take().context("command stdin")?;
        stdin
            .write_all(expr.as_bytes())
            .context("write nix expression")?;
    }
    let mut stderr = child.stderr.take().context("command stderr")?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stderr.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = std::io::stderr().write_all(&chunk[..n]);
                    let _ = std::io::stderr().flush();
                    buf.extend_from_slice(&chunk[..n]);
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        buf
    });
    let status = child.wait().context("wait command")?;
    let stderr = reader.join().unwrap_or_default();
    if status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&stderr);
    let text = text.trim();
    if text.is_empty() {
        bail!("command failed with {status} and no output");
    }
    bail!("command failed: {text}")
}

pub fn single_package(output: &Path) -> Result<PathBuf> {
    let mut files = Vec::new();
    for entry in output.read_dir()? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "tpk") && path.is_file() {
            files.push(path);
        }
    }
    if files.len() != 1 {
        bail!(
            "expected exactly one .tpk in {} (got {})",
            output.display(),
            files.len()
        );
    }
    Ok(files.remove(0).canonicalize()?)
}

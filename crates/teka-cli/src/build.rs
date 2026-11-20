use crate::{checked_output, device::normalize_arch};
use anyhow::{bail, Context, Result};
use clap::Args;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    io::{IsTerminal, Read, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Debug, Args)]
pub struct BuildSource {
    /// Flake directory (default: discover from the current directory)
    #[arg(long, env = "TEKA_APPS_FLAKE_ROOT", value_hint = clap::ValueHint::DirPath)]
    pub flake: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ListOptions {
    /// List available examples
    #[arg(long)]
    pub examples: bool,
    #[command(flatten)]
    pub source: BuildSource,
}

/// Out-link for the linkFarm of requested apps.
pub const RESULT: &str = "result";

#[derive(Clone, Debug, Args)]
pub struct BuildOptions {
    /// App IDs to build (default: the project's only app)
    #[arg(value_name = "APP")]
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
        if !teka::tpk::valid_id(app) {
            bail!("invalid app id '{app}'");
        }
        if !seen.insert(app) {
            bail!("duplicate app id '{app}'");
        }
    }
    Ok(())
}

pub(crate) fn resolve_apps(source: &BuildSource, apps: &[String]) -> Result<(String, Vec<String>)> {
    validate_apps(apps)?;
    let (flake, known) = find_apps(source.flake.as_deref())?;
    let apps = if apps.is_empty() {
        validate_apps(&known)?;
        match known.len() {
            0 => {
                bail!("no apps declared in teka.apps; declare an app before building or deploying")
            }
            1 => (),
            _ => bail!(
                "multiple apps declared in teka.apps; specify an app explicitly; see `teka app list`"
            ),
        }
        known
    } else {
        if let Some(app) = apps.iter().find(|app| !known.contains(app)) {
            bail!("unknown app '{app}'; see `teka app list`");
        }
        apps.to_vec()
    };
    Ok((flake, apps))
}

pub(crate) fn build_targets(
    flake: &str,
    apps: &[String],
    targets: BTreeSet<String>,
    out_link: &Path,
) -> Result<BTreeMap<String, Vec<PathBuf>>> {
    if targets.is_empty() {
        bail!("no device targets to build");
    }
    let multi_target = targets.len() > 1;
    let builder = builder_system()?;
    let use_nom = std::io::stderr().is_terminal() && nom_available();
    let mut command = Command::new(if use_nom { "nom" } else { "nix" });
    command
        .args(["build", "--file", "-", "-L", "--out-link"])
        .arg(out_link)
        .args(["--no-write-lock-file", "--argstr", "flake"])
        .arg(flake);
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
                out_link.join(&target)
            } else {
                out_link.to_owned()
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

fn app_names(reference: &str) -> Result<Option<Vec<String>>> {
    let output = checked_output(Command::new("nix").args([
        "eval",
        &format!("{reference}#."),
        "--apply",
        "flake: if flake ? teka.apps then builtins.attrNames flake.teka.apps else null",
        "--json",
        "--no-write-lock-file",
    ]))
    .with_context(|| format!("read teka.apps from {reference}"))?;
    serde_json::from_str(&output)
        .with_context(|| format!("invalid teka.apps names from {reference}"))
}

struct ExampleCatalog {
    reference: String,
    names: Vec<String>,
    sdk_pin: Option<String>,
}

#[derive(serde::Deserialize)]
struct TemplateExports {
    names: Vec<String>,
    is_sdk: bool,
}

fn template_exports(reference: &str) -> Result<Option<TemplateExports>> {
    let output = checked_output(Command::new("nix").args([
        "eval",
        &format!("{reference}#."),
        "--apply",
        "flake: if flake ? templates then { names = builtins.attrNames flake.templates; is_sdk = flake ? lib.mkAppOutputs; } else null",
        "--json",
        "--no-write-lock-file",
    ]))
    .with_context(|| format!("read templates from {reference}"))?;
    serde_json::from_str(&output)
        .with_context(|| format!("invalid template names from {reference}"))
}

pub(crate) fn template_reference(
    source: &BuildSource,
    example: Option<&str>,
) -> Result<(String, Option<String>)> {
    let ExampleCatalog {
        reference,
        names,
        sdk_pin,
    } = example_catalog(source.flake.as_deref())?;
    let name = example.unwrap_or("default");
    if !names.iter().any(|known| known == name) {
        bail!("unknown example '{name}'; see `teka app list --examples`");
    }
    let template = match example {
        Some(name) => format!("{reference}#{name}"),
        None => reference,
    };
    Ok((template, sdk_pin))
}

fn example_catalog(explicit: Option<&Path>) -> Result<ExampleCatalog> {
    if let Some(catalog) = find_catalog(explicit, "templates", |directory| {
        let reference = flake_reference(directory)?;
        let pinned = pin_local_templates(&reference);
        // A project without a commit can still fall through to the bundled catalog.
        let Some(TemplateExports { names, is_sdk }) =
            template_exports(pinned.as_deref().unwrap_or(&reference))?
        else {
            return Ok(None);
        };
        let reference = pinned?;
        Ok(Some(ExampleCatalog {
            sdk_pin: is_sdk.then(|| reference.clone()),
            reference,
            names,
        }))
    })? {
        return Ok(catalog);
    }
    let reference = default_sdk_reference()?;
    let TemplateExports { names, .. } = template_exports(reference)?
        .context("default template provider has no templates output")?;
    Ok(ExampleCatalog {
        reference: reference.to_owned(),
        names,
        sdk_pin: Some(reference.to_owned()),
    })
}

fn pin_local_templates(reference: &str) -> Result<String> {
    if !reference.starts_with("git+file:") {
        return Ok(reference.to_owned());
    }
    let mut url = reqwest::Url::parse(reference)?;
    if url.query_pairs().any(|(key, _)| key == "rev") {
        return Ok(reference.to_owned());
    }
    url.query_pairs_mut().append_pair("ref", "HEAD");
    let metadata: serde_json::Value =
        serde_json::from_str(&checked_output(Command::new("nix").args([
            "flake",
            "metadata",
            "--json",
            "--no-write-lock-file",
            url.as_str(),
        ]))?)?;
    metadata["revision"]
        .as_str()
        .context("template source has no committed revision")?;
    Ok(metadata["url"]
        .as_str()
        .context("template source metadata has no URL")?
        .to_owned())
}

fn default_sdk_reference() -> Result<&'static str> {
    option_env!("TEKA_SDK_REF")
        .filter(|reference| !reference.is_empty())
        .context("this build has no pinned SDK; use --flake or TEKA_APPS_FLAKE_ROOT with a tested SDK checkout")
}

fn flake_reference(flake: &Path) -> Result<String> {
    let root = flake.ancestors().find(|dir| dir.join(".git").exists());
    let url = reqwest::Url::from_file_path(root.unwrap_or(flake))
        .map_err(|_| anyhow::anyhow!("invalid flake path: {}", flake.display()))?;
    let Some(root) = root else {
        return Ok(format!("path:{}", url.path()));
    };
    let mut url = reqwest::Url::parse(&format!("git+{url}"))?;
    url.query_pairs_mut()
        .append_pair("dir", &flake.strip_prefix(root)?.to_string_lossy());
    // Nix decodes URL queries, not form data where '+' means a space.
    let query = url.query().unwrap().replace('+', "%20");
    url.set_query(Some(&query));
    Ok(url.into())
}

pub fn list(args: ListOptions) -> Result<()> {
    let explicit = args.source.flake.as_deref();
    let names = if args.examples {
        example_catalog(explicit)?.names
    } else {
        find_apps(explicit)?.1
    };
    for name in names {
        if args.examples && name == "default" {
            continue;
        }
        println!("{name}");
    }
    Ok(())
}

pub fn run(args: BuildOptions) -> Result<()> {
    let arch = normalize_arch(&args.arch)?;
    let (flake, apps) = resolve_apps(&args.source, &args.apps)?;
    let target = format!("{arch}-linux");
    let packages = build_targets(&flake, &apps, BTreeSet::from([target]), Path::new(RESULT))?;
    for path in packages.into_values().flatten() {
        println!("{}", path.display());
    }
    Ok(())
}

fn find_apps(explicit: Option<&Path>) -> Result<(String, Vec<String>)> {
    if let Some(catalog) = find_catalog(explicit, "teka.apps", |directory| {
        let reference = flake_reference(directory)?;
        Ok(app_names(&reference)?.map(|names| (reference, names)))
    })? {
        Ok(catalog)
    } else {
        bail!(
            "no flake exporting teka.apps found from {}; use --flake or TEKA_APPS_FLAKE_ROOT",
            std::env::current_dir()?.display()
        )
    }
}

fn find_catalog<T>(
    explicit: Option<&Path>,
    registry: &str,
    load: impl Fn(&Path) -> Result<Option<T>>,
) -> Result<Option<T>> {
    if let Some(path) = explicit {
        if !path.join("flake.nix").is_file() {
            bail!("no flake.nix in {}", path.display());
        }
        let path = path.canonicalize()?;
        let catalog =
            load(&path)?.with_context(|| format!("no {registry} output in {}", path.display()))?;
        return Ok(Some(catalog));
    }
    let cwd = std::env::current_dir()?;
    let device = cwd.metadata()?.dev();
    for directory in cwd.ancestors() {
        if directory.metadata()?.dev() != device {
            break;
        }
        if directory.join("flake.nix").is_file() {
            if let Some(catalog) = load(directory)? {
                return Ok(Some(catalog));
            }
        }
        if directory.join(".git").exists() {
            break;
        }
    }
    Ok(None)
}

/// One native derivation depending on the requested packages. Passed to
/// `nix build --file -` so Nix does not treat a temp directory as a flake.
fn result_expr(builder: &str, target: &str, apps: &[String]) -> String {
    let entries = package_entries(builder, target, apps);
    format!(
        r#"{{ flake }}:
let
  apps = builtins.getFlake flake;
  pkgs = (apps.inputs.nixpkgs or apps.inputs.teka-sdk.inputs.nixpkgs).legacyPackages."{builder}";
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
  pkgs = (apps.inputs.nixpkgs or apps.inputs.teka-sdk.inputs.nixpkgs).legacyPackages."{builder}";
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
    // App images require Linux; other hosts delegate to a target-native builder.
    let build_system = if builder.ends_with("-linux") {
        builder
    } else {
        target
    };
    apps.iter()
        .map(|app| {
            format!(
                "    {{\n      name = \"{app}\";\n      path = apps.teka.apps.\"{app}\".packages.\"{build_system}\".\"{target}\";\n    }}"
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
    Ok(files.remove(0))
}

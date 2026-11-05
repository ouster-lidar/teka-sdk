#![forbid(unsafe_code)]

use crate::{
    checked_output,
    plugins::{is_executable, on_path},
};
use anyhow::{ensure, Context, Result};
use clap::Subcommand;
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    time::Instant,
};

#[derive(Debug, Subcommand)]
pub enum BuildCommand {
    /// Initialize Nix and install build tools
    Init,
    /// Check Nix and host build prerequisites
    Check {
        /// Also compile a minimal hello-world for aarch64-linux (may download tools)
        #[arg(long)]
        full: bool,
        /// Print Nix build logs
        #[arg(short, long, requires = "full")]
        verbose: bool,
    },
}

#[derive(Debug)]
pub(crate) struct BuildFailed(pub &'static str);

impl std::fmt::Display for BuildFailed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for BuildFailed {}

pub fn check_teka() -> Result<PathBuf> {
    on_path("teka").context("teka is not executable on PATH; add its install directory to PATH")
}

pub fn check_nix(nix: &Path) -> Result<String> {
    let version = checked_output(Command::new(nix).arg("--version"))
        .context("Nix is unavailable; install Nix and add its bin directory to PATH")?;
    ensure!(!version.trim().is_empty(), "Nix returned an empty version");
    Ok(version.trim().to_owned())
}

#[derive(Debug)]
struct MissingFeature(&'static str);

impl std::fmt::Display for MissingFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} unavailable", self.0)
    }
}

impl std::error::Error for MissingFeature {}

pub fn check_configuration(nix: &Path) -> Result<()> {
    let supported =
        checked_output(Command::new(nix).args(["eval", "--json", "--expr", "builtins ? getFlake"]))
            .map_err(|error| {
                let diagnostic = format!("{error:#}");
                for feature in ["nix-command", "flakes"] {
                    if diagnostic.contains(&format!("'{feature}' is disabled")) {
                        return anyhow::Error::from(MissingFeature(feature));
                    }
                }
                error.context("Nix evaluation failed")
            })?;
    let supported: bool =
        serde_json::from_str(&supported).context("invalid Nix feature probe JSON")?;
    ensure!(supported, MissingFeature("flakes"));
    Ok(())
}

pub fn check_store(nix: &Path) -> Result<()> {
    checked_output(Command::new(nix).args(["store", "ping", "--json"])).context(
        "cannot access the Nix store; check your store setting, daemon and user permissions",
    )?;
    Ok(())
}

pub(crate) fn nixpkgs_revision() -> Result<String> {
    let lock: serde_json::Value = serde_json::from_str(include_str!("../../flake.lock"))?;
    option_env!("TEKA_VM_NIXPKGS_REV")
        .or_else(|| lock["nodes"]["nixpkgs"]["locked"]["rev"].as_str())
        .map(str::to_owned)
        .context("Nixpkgs revision missing from embedded flake.lock")
}

// Pinned independently of channels and registries so the installed binary can run anywhere.
const BUILD_EXPRESSION: &str = r#"{ nonce, nixpkgsRev }:
let
  nixpkgs = builtins.getFlake "github:NixOS/nixpkgs/${nixpkgsRev}";
  host = builtins.currentSystem;
  target = "aarch64-linux";
  pkgs = import nixpkgs.outPath (
    if nixpkgs.lib.hasSuffix "-linux" host then
      {
        localSystem = host;
        crossSystem = target;
      }
    else
      { system = target; }
  );
in
(pkgs.writeCBin "hello" ''
  #include <stdio.h>
  int main(void) { puts("Hello, world!"); }
'').overrideAttrs
  (_: {
    inherit nonce;
    preferLocalBuild = false;
  })
"#;

pub fn check_build(nix: &Path, verbose: bool) -> Result<()> {
    let temporary = tempfile::tempdir().context("create build-check directory")?;
    let expression = temporary.path().join("check.nix");
    fs::write(&expression, BUILD_EXPRESSION).context("write build-check expression")?;
    let result = temporary.path().join("result");
    // A unique derivation exercises a builder even when dependencies are cached.
    let mut command = Command::new(nix);
    command
        .args(["build", "--impure", "--file"])
        .arg(&expression)
        .args(["--argstr", "nonce"])
        .arg(temporary.path())
        .args(["--argstr", "nixpkgsRev"])
        .arg(nixpkgs_revision()?)
        .arg("--out-link")
        .arg(&result);
    command.stdout(Stdio::null());
    if verbose {
        command.arg("--print-build-logs");
    }
    run_command(&mut command, "Nix", verbose)?;
    verify_hello(&result.join("bin/hello"))
}

fn verify_hello(path: &Path) -> Result<()> {
    ensure!(
        is_executable(path),
        "test build did not produce executable bin/hello"
    );
    let mut header = [0u8; 20];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .context("read hello-world ELF header")?;
    ensure!(
        &header[..7] == b"\x7fELF\x02\x01\x01"
            && matches!(u16::from_le_bytes([header[16], header[17]]), 2 | 3)
            && u16::from_le_bytes([header[18], header[19]]) == 183,
        "test build did not produce an AArch64 Linux ELF executable"
    );
    Ok(())
}

#[derive(Debug)]
struct CommandFailed {
    name: &'static str,
    status: ExitStatus,
    logs: Vec<u8>,
}

impl std::fmt::Display for CommandFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status.code() {
            Some(code) => write!(f, "{} exited with status {code}", self.name),
            None => write!(f, "{} exited with {}", self.name, self.status),
        }
    }
}

impl std::error::Error for CommandFailed {}

pub(crate) fn run_command(command: &mut Command, name: &'static str, verbose: bool) -> Result<()> {
    let output = command
        .stderr(if verbose {
            Stdio::inherit()
        } else {
            Stdio::piped()
        })
        .output()
        .with_context(|| format!("start {name}"))?;
    if !output.status.success() {
        return Err(CommandFailed {
            name,
            status: output.status,
            logs: output.stderr,
        }
        .into());
    }
    Ok(())
}

pub(crate) fn display_path(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) {
        if let Ok(relative) = path.strip_prefix(home) {
            return if relative.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", relative.display())
            };
        }
    }
    path.display().to_string()
}

pub(crate) fn progress(message: &str) {
    eprintln!("⏳\u{fe0e} {message}");
}

pub(crate) fn note(message: impl std::fmt::Display) {
    eprintln!("→ {message}");
}

pub(crate) fn failure(name: &str, error: &anyhow::Error) {
    failure_elapsed(name, error, None);
}

fn failure_elapsed(name: &str, error: &anyhow::Error, started: Option<Instant>) {
    let reason = error
        .chain()
        .map(|cause| {
            let message = cause.to_string();
            message
                .strip_prefix("command failed: ")
                .unwrap_or(&message)
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join(": ");
    eprintln!("✗ {name}: {reason}{}", timing(started));
    if let Some(command) = error.downcast_ref::<CommandFailed>() {
        if !command.logs.is_empty() {
            eprintln!();
            let _ = io::stderr().write_all(&command.logs);
            if !command.logs.ends_with(b"\n") {
                eprintln!();
            }
        }
    }
}

fn timing(started: Option<Instant>) -> String {
    started
        .map(|started| format!(" ({:.1}s)", started.elapsed().as_secs_f64()))
        .unwrap_or_default()
}

pub(crate) fn finish<T>(
    name: &str,
    result: Result<T>,
    detail: impl FnOnce(&T) -> String,
    started: Option<Instant>,
) -> Result<T> {
    match result {
        Ok(value) => {
            let detail = detail(&value);
            let separator = if name == "teka" { " " } else { ": " };
            eprintln!("✓ {name}{separator}{detail}{}", timing(started));
            Ok(value)
        }
        Err(error) => {
            if !error.is::<BuildFailed>() {
                failure_elapsed(name, &error, started);
            }
            Err(BuildFailed("build step failed").into())
        }
    }
}

pub(crate) fn report<T>(name: &str, result: Result<T>, detail: impl FnOnce(&T) -> String) -> bool {
    finish(name, result, detail, None).is_ok()
}

pub(crate) fn timed(
    name: &str,
    message: &str,
    detail: &str,
    run: impl FnOnce() -> Result<()>,
) -> Result<()> {
    progress(message);
    let started = Instant::now();
    finish(name, run(), |()| detail.into(), Some(started))
}

pub fn run(command: BuildCommand) -> Result<()> {
    match command {
        BuildCommand::Init => crate::build_init::run(),
        BuildCommand::Check { full, verbose } => checks(Path::new("nix"), full, verbose),
    }
}

pub(crate) fn nix_checks(nix: &Path, initialize: bool, installed: Option<Instant>) -> Result<()> {
    let available = finish("Nix", check_nix(nix), |version| version.clone(), installed).is_ok();
    if !available {
        for name in ["Nix config", "Nix store"] {
            note(format!("{name}: skipped (Nix unavailable)"));
        }
        return Err(BuildFailed("Nix unavailable").into());
    }
    let configuration = check_configuration(nix);
    let missing = configuration
        .as_ref()
        .err()
        .is_some_and(|error| error.is::<MissingFeature>());
    let valid = if initialize && missing {
        timed(
            "Nix config",
            "Enabling Nix features...",
            "valid (flakes)",
            || crate::build_init::enable_features(nix),
        )
        .is_ok()
    } else {
        report("Nix config", configuration, |()| "valid (flakes)".into())
    };
    let store = report("Nix store", check_store(nix), |()| "reachable".into());
    if valid && store {
        Ok(())
    } else if missing && !initialize {
        Err(MissingFeature("flakes").into())
    } else {
        Err(BuildFailed(if !valid {
            "Nix config invalid"
        } else {
            "Nix store unavailable"
        })
        .into())
    }
}

fn checks(nix_path: &Path, full: bool, verbose: bool) -> Result<()> {
    let teka = report("teka", check_teka(), |path| {
        format!("({})", display_path(path))
    });
    let configuration = nix_checks(nix_path, false, None);
    let build = if full && configuration.is_ok() {
        timed(
            "AArch64 Linux test build",
            "Compiling hello-world for AArch64 Linux...",
            "aarch64-linux executable verified",
            || check_build(nix_path, verbose),
        )
        .is_ok()
    } else {
        if full {
            let error = configuration.as_ref().unwrap_err();
            let reason = if error.is::<MissingFeature>() {
                "Nix config invalid".into()
            } else {
                error.to_string()
            };
            note(format!("AArch64 Linux test build: skipped ({reason})"));
        }
        true
    };
    if configuration
        .as_ref()
        .err()
        .is_some_and(|error| error.is::<MissingFeature>())
    {
        eprintln!();
        note("Run `teka build init` to reinitialize setup and enable flakes");
    }
    ensure!(
        teka && configuration.is_ok() && build,
        BuildFailed("build checks failed")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_paths_are_abbreviated_only_at_a_directory_boundary() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(display_path(&home), "~");
        assert_eq!(
            display_path(&home.join(".local/bin/teka")),
            "~/.local/bin/teka"
        );
        let sibling = PathBuf::from(format!("{}-other/log", home.display()));
        assert_eq!(display_path(&sibling), sibling.display().to_string());
    }
}

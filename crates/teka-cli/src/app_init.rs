use crate::{build, build_check};
use anyhow::{ensure, Context, Result};
use clap::Args;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::{ErrorKind, Write},
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
    process::Command,
};

const APP_GUIDANCE: &str = r#"# Agent guidelines

Read [README.md](README.md) for app behavior and verification. Follow the template's `flake.nix`, locked inputs, and existing build definitions.

- Keep changes small. Reuse existing builders for images, packaging, and signing.
- Keep the app ID consistent across the flake, image metadata, and service. Services are wanted by `teka-apps.target`.
- Keep input declarations in `flake.nix` and exact revisions in `flake.lock`. Update pins only when requested.
- Format Nix with `nixfmt` and keep README consistent with behavior.
- Stage source files before Nix evaluation: Git flakes exclude untracked files. Preserve unrelated work.
- Validate with `teka app list` and `teka app build` from the app directory; builds default to AArch64 Linux.
- Run image checks when changing packaging. Report build and runtime verification separately. Deploy only when requested.
- Before finishing, inspect `git diff`, `git diff --cached`, and `git status`.
"#;

#[derive(Debug, Args)]
pub struct InitOptions {
    /// Example name (default: the catalog's default)
    #[arg(long, value_name = "NAME")]
    pub example: Option<String>,
    #[command(flatten)]
    pub source: build::BuildSource,
    /// Directory to initialize
    #[arg(default_value = ".", value_name = "DIR", value_hint = clap::ValueHint::DirPath)]
    pub directory: PathBuf,
}

fn git_files(directory: &Path) -> Result<BTreeSet<OsString>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["ls-files", "--cached", "--others", "-z"])
        .output()
        .context("start command")?;
    ensure!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| OsString::from_vec(path.to_vec()))
        .collect())
}

pub fn run(options: InitOptions) -> Result<()> {
    build_check::timed(
        "App",
        "Initializing example",
        &format!(
            "{} ({})",
            options.example.as_deref().unwrap_or("default"),
            build_check::display_path(&options.directory)
        ),
        || {
            let (template, provider) =
                build::template_reference(&options.source, options.example.as_deref())?;
            let directory = &options.directory;
            ensure!(
                !directory.join("flake.lock").try_exists()?,
                "flake.lock already exists; update dependencies with `nix flake update`"
            );
            fs::create_dir_all(directory).context("create app directory")?;
            let probe = Command::new("git")
                .arg("-C")
                .arg(directory)
                .args(["rev-parse", "--is-inside-work-tree"])
                .output()
                .context("run Git; install Git and add it to PATH")?;
            if probe.status.success() {
                ensure!(
                    probe.stdout == b"true\n",
                    "destination is not in a Git worktree"
                );
            } else {
                build_check::run_command(
                    Command::new("git").arg("-C").arg(directory).arg("init"),
                    "git init",
                    false,
                )?;
            }
            let existing = git_files(directory)?;
            let initialized = build_check::run_command(
                Command::new("nix")
                    .args(["flake", "new", "--template"])
                    .arg(&template)
                    .arg("--")
                    .arg(directory),
                "nix flake new",
                true,
            )
            .and_then(
                |()| match fs::File::create_new(directory.join("AGENTS.md")) {
                    Ok(mut file) => file
                        .write_all(APP_GUIDANCE.as_bytes())
                        .context("write app guidance"),
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
                    Err(error) => Err(error).context("create app guidance"),
                },
            );
            build_check::run_command(
                Command::new("git")
                    .arg("-C")
                    .arg(directory)
                    .args(["add", "--"])
                    .args(git_files(directory)?.difference(&existing)),
                "git add",
                false,
            )?;
            initialized?;
            build_check::run_command(
                Command::new("nix")
                    .current_dir(directory)
                    .args(["flake", "lock", "path:."])
                    .args(["--override-input", "teka-sdk", &provider])
                    // Overrides normally disable writing the lock file.
                    .args(["--output-lock-file", "flake.lock"]),
                "nix flake lock",
                false,
            )?;
            if directory.join("flake.lock").try_exists()? {
                build_check::run_command(
                    Command::new("git")
                        .arg("-C")
                        .arg(directory)
                        .args(["add", "--", "flake.lock"]),
                    "git add",
                    false,
                )?;
            }
            Ok(())
        },
    )?;
    let directory = options
        .directory
        .canonicalize()
        .context("resolve app directory")?;
    let directory = build_check::display_path(&directory);
    let (prefix, path) = directory
        .strip_prefix("~/")
        .map_or(("", directory.as_str()), |path| ("~/", path));
    let path = if path
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/._-~".contains(&byte))
    {
        path.to_owned()
    } else {
        format!(
            "'{}'",
            path.replace('\'', "'\"'\"'").replace('\\', "'\\\\'")
        )
    };
    let directory = format!("{prefix}{path}");
    eprintln!();
    eprintln!("⚒\u{fe0e} Build:  cd {directory} && teka app build");
    eprintln!("✈\u{fe0e} Deploy: cd {directory} && teka app deploy --build");
    eprintln!();
    Ok(())
}

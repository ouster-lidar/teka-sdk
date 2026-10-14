use anyhow::{Context, Result};
use std::{
    collections::BTreeSet,
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
};

/// Usage failure (exit 2).
#[derive(Debug)]
pub struct UsageError(String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

pub fn is_usage(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|source| source.downcast_ref::<UsageError>().is_some())
}

pub(crate) fn usage(msg: impl std::fmt::Display) -> anyhow::Error {
    UsageError(msg.to_string()).into()
}

fn path_entries() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|dir| {
                    if dir.as_os_str().is_empty() {
                        PathBuf::from(".")
                    } else {
                        dir
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn plugin_name_from_file(file_name: &str) -> Option<&str> {
    file_name.strip_prefix("teka-")
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// First `teka-<name>` on PATH wins per name when scanning PATH order.
pub fn discover_plugins() -> Vec<String> {
    discover_plugin_paths()
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

pub(crate) fn discover_plugin_paths() -> Vec<(String, PathBuf)> {
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    for dir in path_entries() {
        let entries = fs::read_dir(&dir).ok();
        for entry in entries.into_iter().flatten().flatten() {
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            let Some(name) = plugin_name_from_file(&file_name) else {
                continue;
            };
            if name.is_empty() || is_builtin(name) {
                continue;
            }
            if !is_executable(&entry.path()) {
                continue;
            }
            if seen.insert(name.to_string()) {
                names.push((name.to_string(), entry.path()));
            }
        }
    }
    names.sort();
    names
}

fn is_builtin(name: &str) -> bool {
    use clap::CommandFactory;
    // clap only adds the `help` subcommand when the command is built.
    name == "help" || crate::Cli::command().find_subcommand(name).is_some()
}

pub fn find_plugin(name: &str) -> Option<PathBuf> {
    if is_builtin(name) {
        return None;
    }
    let file_name = format!("teka-{name}");
    for dir in path_entries() {
        let path = dir.join(&file_name);
        if is_executable(&path) {
            return Some(path);
        }
    }
    None
}

pub fn augment_help(cmd: &mut clap::Command) {
    let plugins = discover_plugins();
    if plugins.is_empty() {
        return;
    }
    let list = plugins.join(", ");
    *cmd = cmd
        .clone()
        .after_help(format!("\nPlugins (teka-<name> on PATH):\n  {list}\n"));
}

pub fn run_external(args: Vec<std::ffi::OsString>) -> Result<()> {
    let (name, rest) = args
        .split_first()
        .context("external command missing name")?;
    let name = name.to_string_lossy();
    let path = find_plugin(&name)
        .ok_or_else(|| usage(format!("unknown command '{name}'; no teka-{name} on PATH")))?;
    let err = Command::new(&path).args(rest).exec();
    Err(anyhow::Error::from(err).context(format!("run {}", path.display())))
}

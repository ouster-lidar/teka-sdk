use anyhow::{Context, Result};
use clap::Command;
use std::{
    collections::BTreeSet,
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
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

pub(crate) fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub(crate) fn on_path(name: &str) -> Option<PathBuf> {
    path_entries()
        .into_iter()
        .map(|directory| directory.join(name))
        .find(|path| is_executable(path))
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
    on_path(&format!("teka-{name}"))
}

pub(crate) fn augment_plugins(cmd: &mut Command, add: impl Fn(&mut Command, Vec<Command>)) {
    fn visit(
        cmd: &mut Command,
        prefix: &str,
        paths: &mut Vec<(String, PathBuf)>,
        add: &impl Fn(&mut Command, Vec<Command>),
    ) {
        // Consume nested matches first so parents cannot list them again.
        for child in cmd.get_subcommands_mut() {
            visit(child, &format!("{prefix}{}-", child.get_name()), paths, add);
        }
        // Building a copy exposes derived settings without finalizing completion.
        let mut scope = cmd.clone();
        scope.build();
        if !scope.is_allow_external_subcommands_set() {
            return;
        }
        let mut plugins = Vec::new();
        paths.retain(|(full_name, path)| {
            let Some(name) = full_name.strip_prefix(prefix) else {
                return true;
            };
            if !name.is_empty() && name != "help" && scope.find_subcommand(name).is_none() {
                let plugin = crate::completion::metadata_command(full_name, path)
                    .unwrap_or_else(|| Command::new(full_name.clone()));
                plugins.push(plugin.name(name.to_owned()));
            }
            false
        });
        if !plugins.is_empty() {
            add(cmd, plugins);
        }
    }
    visit(cmd, "", &mut discover_plugin_paths(), &add);
}

pub fn augment_help(cmd: &mut Command) {
    augment_plugins(cmd, |cmd, plugins| {
        let mut help = Command::new("teka")
            .styles(cmd.get_styles().clone())
            .disable_help_flag(true)
            .disable_help_subcommand(true)
            .subcommand_help_heading("Plugins")
            .help_template("{all-args}");
        for plugin in plugins {
            let about = plugin
                .get_about()
                .cloned()
                .unwrap_or_else(|| "External plugin".into());
            help = help.subcommand(plugin.about(about));
        }
        *cmd = std::mem::take(cmd).after_help(help.render_help());
    });
}

pub fn run_external(args: Vec<std::ffi::OsString>) -> Result<()> {
    run_external_in(&[], args)
}

/// Dispatch an external child relative to its built-in parent command path.
pub fn run_external_in(parents: &[&str], args: Vec<std::ffi::OsString>) -> Result<()> {
    let (name, rest) = args
        .split_first()
        .context("external command missing name")?;
    let name = name.to_string_lossy();
    let plugin = [parents, &[name.as_ref()]].concat().join("-");
    let path = find_plugin(&plugin).ok_or_else(|| usage(format!("unknown command '{name}'")))?;
    let err = ProcessCommand::new(&path).args(rest).exec();
    Err(anyhow::Error::from(err).context(format!("run {}", path.display())))
}

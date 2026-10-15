use crate::{plugins, Cli};
use anyhow::Result;
use clap::{Arg, ArgAction, Command, CommandFactory, ValueEnum, ValueHint};
use clap_complete::Generator;
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Shell {
    Bash,
    Fish,
    Zsh,
}

impl Shell {
    fn from_path(path: &Path) -> Option<Self> {
        match path.file_name()?.to_str()?.trim_start_matches('-') {
            "bash" | ".bash-wrapped" => Some(Self::Bash),
            "fish" | ".fish-wrapped" => Some(Self::Fish),
            "zsh" | ".zsh-wrapped" => Some(Self::Zsh),
            _ => None,
        }
    }

    fn detect() -> Option<Self> {
        parent_shell().or_else(|| {
            std::env::var_os("SHELL").and_then(|shell| Self::from_path(Path::new(&shell)))
        })
    }
}

fn parent_shell() -> Option<Shell> {
    let pid = unsafe { libc::getppid() };
    #[cfg(target_os = "linux")]
    let path = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    #[cfg(target_os = "macos")]
    let path = {
        use std::os::unix::ffi::OsStrExt;

        // /bin/ps is unavailable in sandboxed macOS builds.
        let mut buffer = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let len =
            unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        if len <= 0 {
            return None;
        }
        let path = std::ffi::CStr::from_bytes_until_nul(&buffer).ok()?;
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes()))
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let path = {
        let output = std::process::Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        std::path::PathBuf::from(String::from_utf8(output.stdout).ok()?.trim())
    };
    Shell::from_path(&path)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    version: u32,
    about: Option<String>,
    #[serde(default)]
    options: Vec<OptionMetadata>,
    #[serde(default)]
    subcommands: Vec<SubcommandMetadata>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubcommandMetadata {
    name: String,
    about: Option<String>,
    #[serde(default)]
    options: Vec<OptionMetadata>,
    #[serde(default)]
    subcommands: Vec<SubcommandMetadata>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionMetadata {
    long: Option<String>,
    short: Option<char>,
    help: Option<String>,
    value: Option<ValueMetadata>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValueMetadata {
    choices: Option<Vec<String>>,
    hint: Option<Hint>,
    #[serde(default)]
    optional: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Hint {
    File,
    Directory,
}

// Static generators interpolate command names and choices into shell code.
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.starts_with(|c: char| c.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

fn metadata_command(name: &str, path: &Path) -> Option<Command> {
    let path = path.with_file_name(format!("teka-{name}.completion.json"));
    if !path.metadata().ok()?.is_file() {
        return None;
    }
    let file = File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(65_537).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 65_536 {
        return None;
    }
    let metadata: Metadata = serde_json::from_slice(&bytes).ok()?;
    if metadata.version != 1 {
        return None;
    }
    command_from_metadata(name, metadata.about, metadata.options, metadata.subcommands)
}

fn command_from_metadata(
    name: &str,
    about: Option<String>,
    options: Vec<OptionMetadata>,
    subcommands: Vec<SubcommandMetadata>,
) -> Option<Command> {
    let mut longs = HashSet::from(["help".to_owned()]);
    let mut shorts = HashSet::from(['h']);
    let mut command = Command::new(name.to_owned());
    if let Some(about) = about {
        command = command.about(about);
    }
    for (index, option) in options.into_iter().enumerate() {
        if option.long.is_none() && option.short.is_none() {
            return None;
        }
        let mut arg = Arg::new(format!("option-{index}"));
        if let Some(long) = option.long {
            if !token(&long) || !longs.insert(long.clone()) {
                return None;
            }
            arg = arg.long(long);
        }
        if let Some(short) = option.short {
            if !short.is_ascii_alphanumeric() || !shorts.insert(short) {
                return None;
            }
            arg = arg.short(short);
        }
        if let Some(help) = option.help {
            arg = arg.help(help);
        }
        arg = if let Some(value) = option.value {
            if value.choices.is_some() && value.hint.is_some() {
                return None;
            }
            arg = arg.action(ArgAction::Set).value_hint(match value.hint {
                Some(Hint::File) => ValueHint::FilePath,
                Some(Hint::Directory) => ValueHint::DirPath,
                None if value.choices.is_some() => ValueHint::Unknown,
                None => ValueHint::Other,
            });
            if value.optional {
                arg = arg.num_args(0..=1);
            }
            if let Some(choices) = value.choices {
                if choices.is_empty() || !choices.iter().all(|choice| token(choice)) {
                    return None;
                }
                arg = arg.value_parser(clap::builder::PossibleValuesParser::new(choices));
            }
            arg
        } else {
            arg.action(ArgAction::SetTrue)
        };
        command = command.arg(arg);
    }
    let mut names = HashSet::from(["help".to_owned()]);
    for subcommand in subcommands {
        if !token(&subcommand.name)
            || subcommand.name.contains("__subcmd__")
            || !names.insert(subcommand.name.clone())
        {
            return None;
        }
        command = command.subcommand(command_from_metadata(
            &subcommand.name,
            subcommand.about,
            subcommand.options,
            subcommand.subcommands,
        )?);
    }
    Some(command)
}

fn fish_nested(command: &Command, path: &mut Vec<String>, output: &mut dyn Write) -> Result<()> {
    // Clap's Fish generator omits commands below two subcommand levels.
    if path.len() > 2 {
        let mut condition = format!("__fish_teka_using_subcommand {}", path[0]);
        for name in &path[1..] {
            condition.push_str(&format!("; and __fish_seen_subcommand_from {name}"));
        }
        if command.has_subcommands() {
            condition.push_str("; and not __fish_seen_subcommand_from");
            for subcommand in command.get_subcommands() {
                condition.push_str(&format!(" {}", subcommand.get_name()));
            }
        }
        let mut shallow = Command::new("teka")
            .bin_name("teka")
            .disable_help_flag(true)
            .args(command.get_arguments().cloned());
        shallow.build();
        let mut generated = Vec::new();
        clap_complete::shells::Fish.try_generate(&shallow, &mut generated)?;
        for line in std::str::from_utf8(&generated)?.lines() {
            if let Some(rule) = line.strip_prefix("complete -c teka") {
                writeln!(output, "complete -c teka -n \"{condition}\"{rule}")?;
            } else {
                writeln!(output, "{line}")?;
            }
        }
        for subcommand in command.get_subcommands() {
            writeln!(
                output,
                "complete -c teka -n \"{condition}\" -f -a {}",
                subcommand.get_name()
            )?;
        }
    }
    for subcommand in command.get_subcommands() {
        path.push(subcommand.get_name().to_owned());
        fish_nested(subcommand, path, output)?;
        path.pop();
    }
    Ok(())
}

pub fn run(shell: Option<Shell>) -> Result<()> {
    let shell = shell.or_else(Shell::detect).ok_or_else(|| {
        plugins::usage("cannot detect shell; use `teka completion bash`, `fish`, or `zsh`")
    })?;
    let mut command = Cli::command().bin_name("teka");
    for (name, path) in plugins::discover_plugin_paths() {
        // Bash uses this marker to split command paths during generation.
        if token(&name) && !name.contains("__subcmd__") {
            let plugin = metadata_command(&name, &path).unwrap_or_else(|| Command::new(name));
            command = command.subcommand(plugin);
        }
    }
    command.build();
    let mut output = std::io::stdout().lock();
    match shell {
        Shell::Bash => clap_complete::shells::Bash.try_generate(&command, &mut output)?,
        Shell::Fish => {
            writeln!(output, "complete -c teka -e")?;
            clap_complete::shells::Fish.try_generate(&command, &mut output)?;
            fish_nested(&command, &mut Vec::new(), &mut output)?;
        }
        Shell::Zsh => {
            writeln!(output, "#compdef teka")?;
            // Zsh's generator keeps existing command-list helpers on reload.
            writeln!(
                output,
                "unfunction -m '_teka_commands' '_teka__*_commands' 2>/dev/null || true"
            )?;
            clap_complete::shells::Zsh.try_generate(&command, &mut output)?;
        }
    }
    Ok(())
}

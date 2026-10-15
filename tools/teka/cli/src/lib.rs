pub mod build;
pub mod completion;
pub mod deploy;
pub mod device;
pub mod plugins;
pub mod system;

pub use reqwest;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use std::ffi::OsString;

#[derive(Debug, Parser)]
#[command(name = "teka", version, about = "Tools for Teka systems and apps")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Generate shell completions
    #[command(after_help = r#"Load once per shell:
  Bash: eval "$(teka completion bash)"
  Fish: teka completion fish | source
  Zsh: eval "$(teka completion zsh)"

Reload after updating Teka or plugins."#)]
    Completion {
        /// Shell to generate completions for (auto-detected if omitted)
        #[arg(value_enum)]
        shell: Option<completion::Shell>,
    },
    /// Build, list, and deploy apps
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    /// Inspect and update Teka systems
    System {
        #[command(subcommand)]
        command: system::SystemCommand,
    },
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

#[derive(Debug, Subcommand)]
pub enum AppCommand {
    /// Build app images
    Build(build::BuildOptions),
    /// List available apps
    List(build::BuildSource),
    /// Deploy apps to a device
    #[command(
        after_help = "Examples:\n  teka app deploy --host http://device.local my-app\n  teka app deploy --host http://device.local --build my-app"
    )]
    Deploy(deploy::DeployOptions),
}

pub fn run(command: Command) -> Result<()> {
    match command {
        Command::Completion { shell } => completion::run(shell),
        Command::App { command } => match command {
            AppCommand::Deploy(args) => deploy::run(args),
            AppCommand::Build(args) => build::run(args),
            AppCommand::List(args) => build::list(args),
        },
        Command::System { command } => match command {
            system::SystemCommand::Status(args) => system::run_status(args),
            system::SystemCommand::Update(args) => system::run_update(args),
        },
        Command::External(args) => plugins::run_external(args),
    }
}

fn parse_cli() -> Result<Cli, clap::Error> {
    let mut cmd = Cli::command();
    plugins::augment_help(&mut cmd);
    let matches = cmd.try_get_matches()?;
    Cli::from_arg_matches(&matches)
}

pub fn main_entry() -> i32 {
    let cli = match parse_cli() {
        Ok(cli) => cli,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = e.print();
            return code;
        }
    };
    match cli.command.map(|c| run(c).map(|_| 0)).unwrap_or_else(|| {
        let mut cmd = Cli::command();
        plugins::augment_help(&mut cmd);
        cmd.print_help()?;
        println!();
        Ok(0)
    }) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("teka: error: {e:#}");
            if plugins::is_usage(&e) {
                2
            } else {
                1
            }
        }
    }
}

pub fn checked_output(command: &mut std::process::Command) -> Result<String> {
    let output = command.output().context("start command")?;
    if !output.status.success() {
        bail!(
            "command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout).context("command output is not UTF-8")
}

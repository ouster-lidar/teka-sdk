pub mod build;
pub mod deploy;
pub mod device;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "teka",
    version,
    about = "Build artifacts and deploy to Teka devices"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build and deploy app images
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AppCommand {
    /// Build app artifacts locally
    Build(build::BuildOptions),
    /// List available apps
    List(build::BuildSource),
    /// Deploy apps to a device
    Deploy(deploy::DeployOptions),
}

pub fn run(command: Command) -> Result<()> {
    match command {
        Command::App { command } => match command {
            AppCommand::Deploy(args) => deploy::run(args),
            AppCommand::Build(args) => build::run(args),
            AppCommand::List(args) => build::list(args),
        },
    }
}

pub fn main_entry() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = e.print();
            return code;
        }
    };
    match cli.command.map(|c| run(c).map(|_| 0)).unwrap_or_else(|| {
        Cli::command().print_help()?;
        println!();
        Ok(0)
    }) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("teka: error: {e:#}");
            1
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

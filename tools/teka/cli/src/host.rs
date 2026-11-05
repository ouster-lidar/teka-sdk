use anyhow::{ensure, Result};
use std::process::Command;

pub(crate) trait CommandExt {
    fn check_status(&mut self, message: &str) -> Result<()>;
}

impl CommandExt for Command {
    fn check_status(&mut self, message: &str) -> Result<()> {
        ensure!(self.status()?.success(), "{message}");
        Ok(())
    }
}

pub(crate) fn sudo_shell(script: &str) -> Command {
    let prompt = if cfg!(target_os = "macos") {
        "? macOS password for %p (sudo): "
    } else {
        "? Password for %p (sudo): "
    };
    let mut command = Command::new("sudo");
    command.args(["--prompt", prompt, "/bin/sh", "-c", script]);
    command
}

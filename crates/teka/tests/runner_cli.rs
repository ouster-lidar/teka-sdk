#![cfg(feature = "app")]

use std::{os::unix::process::CommandExt, process::Command};

#[test]
fn runner_accepts_direct_and_plugin_help() {
    for name in ["teka-runner", "teka runner"] {
        let output = Command::new(env!("CARGO_BIN_EXE_teka-runner"))
            .arg0(name)
            .arg("--help")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("Run an app with Teka registration"));
        assert!(help.contains("teka-runner APP_ID tcp|udp"));
    }
}

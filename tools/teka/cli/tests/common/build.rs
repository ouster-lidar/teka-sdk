use std::{
    ffi::OsStr,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};
use tempfile::TempDir;

pub struct Host(pub TempDir);

impl Host {
    pub fn path(&self, name: &str) -> PathBuf {
        self.0.path().join(name)
    }

    pub fn read(&self, name: &str) -> String {
        fs::read_to_string(self.path(name)).unwrap()
    }

    pub fn write(&self, name: &str, contents: impl AsRef<[u8]>) {
        write(self.path(name), contents);
    }

    pub fn remove(&self, name: &str) {
        remove(self.path(name));
    }

    pub fn missing(&self, names: &[&str]) {
        for name in names {
            assert!(!self.path(name).exists(), "unexpected file: {name}");
        }
    }

    pub fn files(&self, expected: &[(&str, &str)]) {
        for (name, contents) in expected {
            assert_eq!(self.read(name), *contents, "{name}");
        }
    }

    pub fn process(&self, binary: impl AsRef<OsStr>, home: &Path, bin: &Path) -> Command {
        let mut command = Command::new(binary);
        command
            .current_dir(self.0.path())
            .env("HOME", home)
            .env("PATH", bin)
            .env_remove("NIX_USER_CONF_FILES")
            .env_remove("NIX_CONFIG")
            .env_remove("XDG_CONFIG_HOME");
        command
    }
}

pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    fs::write(path, contents).unwrap();
}

pub fn remove(path: impl AsRef<Path>) {
    fs::remove_file(path).unwrap();
}

pub fn executable(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn script(path: &Path, body: &str) {
    executable(path, format!("#!/bin/sh\n{body}\n"));
}

#[derive(Debug)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn check(self, code: i32) -> Self {
        assert_eq!(self.status.code(), Some(code), "{self:?}");
        self
    }

    pub fn stderr_contains(&self, patterns: &str) {
        contains(&self.stderr, patterns);
    }

    pub fn stdout_contains(&self, patterns: &str) {
        contains(&self.stdout, patterns);
    }
}

pub fn contains(text: &str, patterns: &str) {
    for pattern in patterns.lines().filter(|line| !line.is_empty()) {
        assert!(text.contains(pattern), "missing {pattern:?}: {text}");
    }
}

pub fn excludes(text: &str, patterns: &str) {
    for pattern in patterns.lines().filter(|line| !line.is_empty()) {
        assert!(!text.contains(pattern), "unexpected {pattern:?}: {text}");
    }
}

impl From<std::process::Output> for Output {
    fn from(output: std::process::Output) -> Self {
        Self {
            status: output.status,
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }
}

pub trait CommandExt {
    fn run(&mut self) -> Output;
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn failed(&mut self) -> Output {
        let output = self.run();
        assert!(!output.status.success(), "{output:?}");
        output
    }
    fn checked(&mut self, code: i32) -> Output {
        self.run().check(code)
    }
}

impl CommandExt for Command {
    fn run(&mut self) -> Output {
        self.output().unwrap().into()
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn ssh_timing(report: &str) {
    let timing = report
        .lines()
        .find_map(|line| line.strip_prefix("✓ SSH: reachable ("))
        .unwrap()
        .strip_suffix("s)")
        .unwrap();
    assert!(timing.parse::<f64>().unwrap() >= 0.0);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn runtime() -> String {
    teka_cli::build_vm::runtime_expression(1800).unwrap()
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn vm_fixture(root: &Path, private: &str, public: &str) {
    for (name, contents) in [
        ("keys/builder_ed25519", private),
        ("keys/builder_ed25519.pub", public),
        ("keys/ssh_host_ed25519_key", "host private key"),
        ("keys/ssh_host_ed25519_key.pub", "ssh-ed25519 AAAA fixture"),
        (
            "known_hosts",
            "[127.0.0.1]:31023 ssh-ed25519 AAAA fixture\n",
        ),
        (
            "runtime/share/teka-build-vm/btop",
            "/nix/store/abc-btop/bin/btop\n",
        ),
        ("runtime.nix", &runtime()),
    ] {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    script(&root.join("runtime/bin/teka-build-vm-runner"), "exit 0");
}

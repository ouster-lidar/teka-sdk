#![forbid(unsafe_code)]

use crate::{
    build_check, checked_output,
    host::{sudo_shell, CommandExt},
    plugins::on_path,
};
use anyhow::{ensure, Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

const NIX_PROFILE: &str = "/nix/var/nix/profiles/default/bin/nix";
const INSTALLER: &str = "https://install.determinate.systems/nix";

fn find_nix() -> Option<PathBuf> {
    on_path("nix").or_else(|| Path::new(NIX_PROFILE).exists().then(|| NIX_PROFILE.into()))
}

fn install_nix(url: &str) -> Result<()> {
    let mut script = tempfile::NamedTempFile::new().context("create Nix installer file")?;
    reqwest::blocking::get(url)
        .context("download Nix installer")?
        .error_for_status()
        .context("download Nix installer")?
        .copy_to(script.as_file_mut())
        .context("save Nix installer")?;
    // Keep stdin attached so the installer owns confirmation and sudo prompts.
    Command::new("/bin/sh")
        .arg(script.path())
        .arg("install")
        .check_status("Nix installation failed or was cancelled")
        .context("start Nix installer")?;
    Ok(())
}

fn destination() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .context("HOME is not set")?;
    let local_bin = PathBuf::from(home).join(".local/bin");
    let path = std::env::var_os("PATH").unwrap_or_default();
    let directory = if std::env::split_paths(&path).any(|directory| directory == local_bin) {
        local_bin
    } else {
        PathBuf::from("/usr/local/bin")
    };
    Ok(directory.join("teka"))
}

fn copy_binary(source: &Path, destination: &Path) -> Result<()> {
    let directory = destination.parent().context("missing install directory")?;
    fs::create_dir_all(directory)?;
    let temporary = tempfile::NamedTempFile::new_in(directory)?;
    fs::copy(source, temporary.path())?;
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o755))?;
    temporary
        .persist(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

fn sudo_copy(source: &Path, destination: &Path) -> Result<()> {
    eprint!(
        "? Install Teka to {}? [y/N] ",
        build_check::display_path(destination)
    );
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    ensure!(
        matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
        "declined"
    );
    sudo_shell(
        r#"set -eu
mkdir -p "$1"
[ ! -d "$3" ]
temporary=$(mktemp "$1/.teka.XXXXXX")
trap 'rm -f "$temporary"' EXIT
install -m 0755 "$2" "$temporary"
mv -f "$temporary" "$3"
"#,
    )
    .arg("teka-install")
    .arg(destination.parent().context("missing install directory")?)
    .arg(source)
    .arg(destination)
    .check_status("sudo installation failed or was cancelled")
    .context("start sudo installation")?;
    Ok(())
}

fn install_teka(source: &Path, destination: &Path) -> Result<()> {
    if destination.canonicalize().ok().as_deref() == Some(source) {
        return Ok(());
    }
    ensure!(
        !destination.is_dir(),
        "destination is a directory: {}",
        build_check::display_path(destination)
    );
    if let Err(error) = copy_binary(source, destination) {
        if destination.parent() == Some(Path::new("/usr/local/bin"))
            && error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::PermissionDenied)
        {
            sudo_copy(source, destination)?;
        } else {
            return Err(error)
                .with_context(|| format!("install {}", build_check::display_path(destination)));
        }
    }
    Ok(())
}

pub(crate) fn enable_features(nix: &Path) -> Result<()> {
    let configuration = if let Some(files) = std::env::var_os("NIX_USER_CONF_FILES") {
        // Nix loads these in reverse order, so the first file has highest priority.
        std::env::split_paths(&files)
            .find(|path| !path.as_os_str().is_empty())
            .context("NIX_USER_CONF_FILES disables user configuration")?
    } else {
        let directory = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .context("HOME is not set")?;
        directory.join("nix/nix.conf")
    };
    if let Ok(metadata) = fs::symlink_metadata(&configuration) {
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Nix configuration is managed or is not a regular file: {}",
            build_check::display_path(&configuration)
        );
    }
    let setting = "extra-experimental-features = nix-command flakes";
    let existing = match fs::read_to_string(&configuration) {
        Ok(existing) => existing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).context("read Nix user configuration"),
    };
    if !existing.lines().any(|line| line.trim() == setting) {
        let directory = configuration
            .parent()
            .context("Nix configuration has no parent directory")?;
        let append = || -> io::Result<()> {
            fs::create_dir_all(directory)?;
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&configuration)?;
            writeln!(file, "\n{setting}")
        };
        if let Err(error) = append() {
            if error.kind() != io::ErrorKind::PermissionDenied {
                return Err(error.into());
            }
            sudo_shell(r#"set -eu; mkdir -p "$1"; [ ! -L "$2" ]; printf '\n%s\n' "$3" >> "$2""#)
                .arg("teka-nix-features")
                .arg(directory)
                .arg(&configuration)
                .arg(setting)
                .check_status("Nix feature configuration failed or was cancelled")?;
        }
    }
    build_check::check_configuration(nix).context(
        "verify required Nix features; check configuration overrides if features remain disabled",
    )
}

fn shell_setup() {
    if on_path("nix").is_none() {
        let script = match crate::completion::Shell::detect() {
            Some(crate::completion::Shell::Fish) => {
                Some("source /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.fish")
            }
            Some(crate::completion::Shell::Bash | crate::completion::Shell::Zsh) => {
                Some(". /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh")
            }
            None => None,
        };
        if let Some(script) = script {
            build_check::note("Open a new shell to load the Nix environment, or run:");
            eprintln!("\n{script}\n");
        } else {
            build_check::note("Open a new shell to load the Nix environment");
        }
    }
    build_check::note("Shell completions: `teka completion --help`");
}

fn initialize() -> Result<()> {
    ensure!(
        matches!(
            (std::env::consts::OS, std::env::consts::ARCH),
            ("linux", "x86_64" | "aarch64") | ("macos", "aarch64")
        ),
        "unsupported host; supported: x86_64-linux, aarch64-linux, aarch64-darwin"
    );
    let installed = if find_nix().is_none() {
        build_check::progress("Installing Nix (interactive setup)...");
        let started = Instant::now();
        if let Err(error) = install_nix(INSTALLER) {
            return build_check::finish("Nix", Err(error), |()| String::new(), Some(started));
        }
        Some(started)
    } else {
        None
    };
    let nix = match find_nix() {
        Some(nix) => nix,
        None => {
            return build_check::finish(
                "Nix",
                Err(anyhow::anyhow!(
                    "Nix unavailable after installation; check the installer output"
                )),
                |()| String::new(),
                installed,
            )
        }
    };
    ensure!(
        build_check::nix_checks(&nix, true, installed).is_ok(),
        build_check::BuildFailed("build initialization failed")
    );

    let source = std::env::current_exe()
        .context("locate running teka")?
        .canonicalize()?;
    let native = option_env!("TEKA_NIX_PACKAGE").is_some();
    let destination = if native {
        source
    } else {
        let destination = destination()?;
        install_teka(&source, &destination)?;
        destination
    };

    if !native {
        let resolved = build_check::check_teka()?.canonicalize()?;
        ensure!(
            resolved == destination.canonicalize()?,
            "{} shadows {}; put {} first on PATH and rerun init",
            build_check::display_path(&resolved),
            build_check::display_path(&destination),
            build_check::display_path(destination.parent().unwrap())
        );
    }
    let version = checked_output(Command::new(&destination).arg("--version"))
        .context("verify installed teka --version")?;
    ensure!(
        version.trim() == concat!("teka ", env!("CARGO_PKG_VERSION")),
        "installed teka returned an unexpected version: {}",
        version.trim()
    );
    eprintln!(
        "✓ {} ({}{})",
        version.trim(),
        build_check::display_path(&destination),
        if native { "; managed by Nix" } else { "" }
    );
    eprintln!();
    if !native {
        shell_setup();
    }
    build_check::note("Run `teka build check --full` to test AArch64 Linux compilation");
    Ok(())
}

pub fn run() -> Result<()> {
    let result = initialize();
    if let Err(error) = &result {
        if !error.is::<build_check::BuildFailed>() {
            let name = if error.to_string() == "declined" {
                "Teka installation"
            } else {
                "Build initialization"
            };
            build_check::failure(name, error);
        }
    }
    result.map_err(|_| build_check::BuildFailed("build initialization failed").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::BufRead, net::TcpListener};

    #[test]
    fn installer_preserves_interactive_arguments_and_reports_failure() {
        for exit in [0, 1, 130] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                for line in io::BufReader::new(&stream).lines() {
                    if line.unwrap().is_empty() {
                        break;
                    }
                }
                let script =
                    format!("[ \"$#\" = 1 ] && [ \"$1\" = install ] || exit 99\nexit {exit}\n");
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{script}",
                    script.len()
                )
                .unwrap();
            });
            assert_eq!(install_nix(&url).is_ok(), exit == 0);
            server.join().unwrap();
        }
    }

    #[test]
    fn atomic_copy_preserves_existing_install_on_failure_and_cleans_up() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let destination = directory.path().join("teka");
        fs::write(&source, b"new version").unwrap();
        fs::write(&destination, b"old version").unwrap();
        assert!(copy_binary(&directory.path().join("missing"), &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"old version");
        copy_binary(&source, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new version");
        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
        assert!(copy_binary(&source, directory.path()).is_err());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}

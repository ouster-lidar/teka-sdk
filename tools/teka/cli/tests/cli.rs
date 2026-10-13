use ouster_teka::{
    build::{single_raw, target_package, validate_apps},
    deploy::check_logs,
    device::{normalize_arch, DeviceClient},
};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn apps_flake(dir: &Path) {
    fs::create_dir(dir.join("bin")).unwrap();
    fs::create_dir(dir.join(".git")).unwrap();
    fs::create_dir(dir.join("apps")).unwrap();
    fs::write(dir.join("apps/flake.nix"), "{}").unwrap();
    script(&dir.join("bin/nix"), NIX_MOCK);
}

fn script(path: &Path, text: &str) {
    fs::write(path, format!("#!/bin/sh\n{text}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

const NIX_MOCK: &str = r#"if [ "$1" = build ]; then
    expr=$(cat)
    out=result
    prev=
    for arg in "$@"; do
        if [ "$prev" = --out-link ]; then
            out=$arg
        fi
        prev=$arg
    done
    names=$(printf '%s\n' "$expr" | sed -n 's/^[[:space:]]*name = "\([^"]*\)".*/\1/p')
    if [ -z "$names" ]; then
        echo "mock: no linkFarm names in nix expression" >&2
        printf '%s\n' "$expr" >&2
        exit 1
    fi
    rm -rf "$out"
    mkdir -p "$out"
    for name in $names; do
        mkdir -p "$out/$name"
        printf '%s' "${TEKA_MOCK_PAYLOAD:-app}" > "$out/$name/app.raw"
    done
    printf '%s\n' "$@" "$expr" >> calls
elif [ "$2" = --raw ]; then
    printf x86_64-linux
else
    printf '["app","hello-world"]'
fi"#;

fn cli(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_teka"));
    c.env_remove("TEKA_HOST")
        .env_remove("TEKA_PASSWORD")
        .env_remove("TEKA_BOOTSTRAP_TOKEN")
        .env_remove("TEKA_APPS_FLAKE_ROOT")
        .env("TEKA_POST_DEPLOY_SEC", "0")
        .env("TEKA_LOG_SEC", "2")
        .env(
            "PATH",
            std::env::join_paths([dir.join("bin")].into_iter().chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )))
            .unwrap(),
        )
        .current_dir(dir);
    c
}

#[test]
fn cross_targets_and_artifacts() {
    assert_eq!(normalize_arch("ARM64").unwrap(), "aarch64");
    assert!(normalize_arch("riscv64").is_err());
    assert_eq!(
        target_package("aarch64-darwin", "app", "x86_64-linux"),
        ("x86_64-linux".into(), "app".into())
    );
    let d = TempDir::new().unwrap();
    assert!(single_raw(d.path()).is_err());
    fs::write(d.path().join("app.raw"), b"data").unwrap();
    assert_eq!(
        single_raw(d.path()).unwrap().file_name().unwrap(),
        "app.raw"
    );
    fs::write(d.path().join("other.raw"), b"data").unwrap();
    assert!(single_raw(d.path()).is_err());
    assert!(check_logs("status=203/EXEC").is_err());
    assert!(check_logs("libnvidia-glcore.so.550 cannot open shared object file").is_err());
    assert!(check_logs("hello world").is_ok());
    assert!(validate_apps(&["app".into(), "app".into()]).is_err());
}

struct Request {
    line: String,
    headers: String,
    body: Vec<u8>,
}
fn serve(
    replies: Vec<(&'static str, &'static str)>,
) -> (String, std::thread::JoinHandle<Vec<Request>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in replies {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut data = Vec::new();
            let header_end = loop {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                data.push(byte[0]);
                if data.ends_with(b"\r\n\r\n") {
                    break data.len();
                }
            };
            let headers = String::from_utf8(data[..header_end].to_vec()).unwrap();
            let size = headers
                .lines()
                .find_map(|s| {
                    s.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(|s| s.parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let mut payload = vec![0; size];
            socket.read_exact(&mut payload).unwrap();
            requests.push(Request {
                line: headers.lines().next().unwrap().into(),
                headers,
                body: payload,
            });
            write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nSet-Cookie: session=test; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    (address, handle)
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn device_urls_preserve_scheme_port_and_base_path() {
    for scheme in ["http", "https"] {
        for suffix in ["", "/"] {
            let client =
                DeviceClient::new(&format!("{scheme}://device.local:8443/proxy{suffix}")).unwrap();
            assert_eq!(
                client.endpoint("api/v1/auth/status").unwrap().as_str(),
                format!("{scheme}://device.local:8443/proxy/api/v1/auth/status")
            );
        }
    }
    for url in [
        "ftp://device.local",
        "device.local",
        "https://user:secret@device.local",
        "https://device.local?token=secret",
        "https://device.local#fragment",
    ] {
        assert!(DeviceClient::new(url).is_err(), "{url}");
    }
}

#[test]
fn authenticated_install_streams_with_cookie_and_profile() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let (url, server) = serve(vec![
        ("200 OK", r#"{"enforcement":true}"#),
        ("200 OK", "{}"),
        ("200 OK", r#"{"architecture":"amd64"}"#),
        ("200 OK", "{}"),
        ("200 OK", r#"{"success":true,"app_id":"runtime-app"}"#),
        ("200 OK", "hello world"),
    ]);
    let output = cli(d.path())
        .env("TEKA_PASSWORD", "secret")
        .env("TEKA_HOST", &url)
        .env("TEKA_MOCK_PAYLOAD", "payload")
        .args([
            "app",
            "deploy",
            "--build",
            "app",
            "--profile",
            "a b",
            "--remove",
        ])
        .output()
        .unwrap();
    success(&output);
    let requests = server.join().unwrap();
    assert_eq!(requests[1].line, "POST /api/v1/auth/login HTTP/1.1");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&requests[1].body).unwrap()["password"],
        "secret"
    );
    assert_eq!(
        requests[2].line,
        "GET /api/v1/platform/architecture HTTP/1.1"
    );
    assert_eq!(
        requests[3].line,
        "POST /api/v1/manager/app/app/remove HTTP/1.1"
    );
    assert_eq!(
        requests[4].line,
        "POST /api/v1/manager/install?profile=a+b HTTP/1.1"
    );
    assert!(requests[4]
        .headers
        .to_ascii_lowercase()
        .contains("cookie: session=test"));
    assert_eq!(requests[4].body, b"payload");
    assert_eq!(
        requests[5].line,
        "GET /api/v1/manager/app/runtime-app/logs?since_unit_start=true HTTP/1.1"
    );
    assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok());
}

#[test]
fn setup_sends_bootstrap_token() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let (url, server) = serve(vec![
        (
            "200 OK",
            r#"{"needs_setup":true,"bootstrap_required":true}"#,
        ),
        ("200 OK", "{}"),
        ("200 OK", r#"{"architecture":"amd64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "hello world"),
    ]);
    success(
        &cli(d.path())
            .env("TEKA_PASSWORD", "secret")
            .env("TEKA_BOOTSTRAP_TOKEN", "token")
            .args(["app", "deploy", "--build", "app", "--host", &url])
            .output()
            .unwrap(),
    );
    let requests = server.join().unwrap();
    assert_eq!(requests[1].line, "POST /api/v1/auth/setup HTTP/1.1");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&requests[1].body).unwrap()["bootstrap_token"],
        "token"
    );
    assert_eq!(
        requests[3].line,
        "POST /api/v1/manager/install?profile=trusted HTTP/1.1"
    );
    assert_eq!(requests[3].body, b"app");
}

#[test]
fn errors_without_password_and_rejects_login_redirect() {
    let d = TempDir::new().unwrap();
    let (url, server) = serve(vec![("200 OK", r#"{"enforcement":true}"#)]);
    let out = cli(d.path())
        .args(["app", "deploy", "--build", "app", "--host", &url])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("TEKA_PASSWORD"));
    server.join().unwrap();
    let (url, server) = serve(vec![("200 OK", "{}")]);
    let out = cli(d.path())
        .env("TEKA_PASSWORD", "secret")
        .args(["app", "deploy", "--build", "app", "--host", &url])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("auth status"));
    server.join().unwrap();
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("302 Found", "login"),
    ]);
    let out = cli(d.path())
        .args(["app", "deploy", "--build", "app", "--host", &url])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("302"));
    server.join().unwrap();
}

#[test]
fn apps_flake_environment_and_flag_precedence() {
    let d = TempDir::new().unwrap();
    let bin = d.path().join("bin");
    fs::create_dir(&bin).unwrap();
    for name in ["environment", "explicit"] {
        let dir = d.path().join(name);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("flake.nix"), "{}").unwrap();
    }
    script(
        &bin.join("nix"),
        "if [ \"$2\" = --raw ]; then printf x86_64-linux; else printf '%s' \"$2\" > \"$TEKA_TEST_NIX_ARGS\"; printf '[\"hello-world\"]'; fi",
    );
    let captured = d.path().join("nix-args");
    for explicit in [false, true] {
        let mut command = cli(d.path());
        command
            .env("TEKA_APPS_FLAKE_ROOT", d.path().join("environment"))
            .env("TEKA_TEST_NIX_ARGS", &captured)
            .args(["app", "list"]);
        if explicit {
            command.arg("--flake").arg(d.path().join("explicit"));
        }
        let output = command.output().unwrap();
        success(&output);
        assert_eq!(output.stdout, b"hello-world\n");
        let selected = if explicit { "explicit" } else { "environment" };
        assert_eq!(
            fs::read_to_string(&captured).unwrap(),
            format!(
                "{}#.teka.apps",
                d.path().join(selected).canonicalize().unwrap().display()
            )
        );
    }
}

#[test]
fn apps_flake_is_found_at_git_root() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let sub = d.path().join("nested/dir");
    fs::create_dir_all(&sub).unwrap();
    let out = cli(d.path())
        .current_dir(&sub)
        .args(["app", "list"])
        .output()
        .unwrap();
    success(&out);
    assert_eq!(out.stdout, b"app\nhello-world\n");

    fs::remove_dir(d.path().join(".git")).unwrap();
    let out = cli(d.path())
        .current_dir(&sub)
        .args(["app", "list"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not in a git checkout"));
}

#[test]
fn build_prints_paths_and_lists_apps_without_a_device() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    for target in [&["--arch", "aarch64"][..], &[], &["--arch", "x86_64"]] {
        let out = cli(d.path())
            .env("TEKA_HOST", "not-a-host-url")
            .args(["app", "build", "hello-world"])
            .args(target)
            .output()
            .unwrap();
        success(&out);
        assert_eq!(
            String::from_utf8(out.stdout).unwrap().trim(),
            "result/hello-world"
        );
    }
    assert_eq!(
        fs::read(d.path().join("result/hello-world/app.raw")).unwrap(),
        b"app"
    );
    let calls = fs::read_to_string(d.path().join("calls")).unwrap();
    assert!(calls.contains("apps.packages.\"x86_64-linux\".\"hello-world-cross-aarch64-linux\""));
    assert!(calls.contains("apps.packages.\"x86_64-linux\".\"hello-world\";"));
    assert!(calls.contains("builtins.getFlake"));
    assert!(calls.contains("--file"));
    assert!(calls.contains("--out-link"));
    assert!(!calls.contains("#packages"));
    let out = cli(d.path()).args(["app", "list"]).output().unwrap();
    success(&out);
    assert_eq!(out.stdout, b"app\nhello-world\n");
}

#[test]
fn build_rejects_apps_missing_from_teka_apps() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let out = cli(d.path())
        .args(["app", "build", "nope"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("unknown app 'nope'"), "{stderr}");
    assert!(!d.path().join("calls").exists());
}

#[test]
fn build_reports_nix_stderr() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    script(
        &d.path().join("bin/nix"),
        r#"if [ "$2" = --raw ]; then
    printf x86_64-linux
elif [ "$1" = build ]; then
    cat >/dev/null
    echo "boom-from-nix" >&2
    exit 1
else
    printf '["hello-world"]'
fi"#,
    );
    let out = cli(d.path())
        .args(["app", "build", "hello-world"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("command failed: boom-from-nix"), "{stderr}");
}

#[test]
fn cli_rejects_invalid_usage() {
    let d = TempDir::new().unwrap();
    for args in [
        vec!["app", "deploy"],
        vec!["app", "deploy", "--build"],
        vec!["app", "deploy", "hello-world", "--build", "hello-world"],
        vec!["app", "build"],
        vec!["app", "build", "hello-world", "--arch", "unknown"],
    ] {
        let out = cli(d.path()).args(&args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn deploy_requires_host() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let out = cli(d.path())
        .args(["app", "deploy", "--build", "hello-world"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--host"));
    assert!(!d.path().join("calls").exists());
}

#[test]
fn arch_accepts_aliases() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    for arch in ["arm64", "amd64"] {
        success(
            &cli(d.path())
                .args(["app", "build", "hello-world", "--arch", arch])
                .output()
                .unwrap(),
        );
    }
    let calls = fs::read_to_string(d.path().join("calls")).unwrap();
    assert!(calls.contains("teka-aarch64-linux"));
    assert!(calls.contains("teka-x86_64-linux"));
}

#[test]
fn host_flag_overrides_environment() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"amd64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"hello-world"}"#),
        ("200 OK", "hello world"),
    ]);
    success(
        &cli(d.path())
            .env("TEKA_HOST", "http://127.0.0.1:1")
            .args(["app", "deploy", "--build", "hello-world", "--host", &url])
            .output()
            .unwrap(),
    );
    server.join().unwrap();
}

fn plugin_bin(dir: &Path, name: &str, body: &str) {
    script(&dir.join("bin").join(format!("teka-{name}")), body);
}

#[test]
fn plugin_dispatches_and_lists_in_help() {
    let d = TempDir::new().unwrap();
    fs::create_dir(d.path().join("bin")).unwrap();
    plugin_bin(
        d.path(),
        "foo",
        r#"printf '%s\n' "$@" >> "$TEKA_PLUGIN_LOG"
exit 7"#,
    );
    plugin_bin(
        d.path(),
        "system",
        r#"echo ran-system >> "$TEKA_PLUGIN_LOG"
exit 0"#,
    );
    let log = d.path().join("plugin-log");
    let out = cli(d.path())
        .env("TEKA_PLUGIN_LOG", &log)
        .args(["foo", "a", "b"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(fs::read_to_string(&log).unwrap(), "a\nb\n");
    let out = cli(d.path())
        .env("TEKA_PLUGIN_LOG", &log)
        .args(["system"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(!fs::read_to_string(&log).unwrap().contains("ran-system"));
    let out = cli(d.path()).args(["--help"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("on PATH):\n  foo\n"));
    let out = cli(d.path()).args(["nope"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn plugins_in_empty_path_entries_are_listed_and_dispatched() {
    let d = TempDir::new().unwrap();
    script(&d.path().join("teka-local"), "exit 7");
    for path in ["", ":/usr/bin", "/usr/bin:"] {
        let out = cli(d.path())
            .env("PATH", path)
            .arg("--help")
            .output()
            .unwrap();
        success(&out);
        assert!(String::from_utf8_lossy(&out.stdout).contains("on PATH):\n  local\n"));
        let out = cli(d.path())
            .env("PATH", path)
            .arg("local")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(7));
    }
}

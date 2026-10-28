mod common;
use ouster_teka::{
    build::{single_raw, target_package, validate_apps},
    deploy::check_logs,
    device::{normalize_arch, DeviceClient},
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
    sync::mpsc,
    time::Duration,
};
use teka_pkg::{Architecture, Component, ComponentType, Manifest};
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

fn os_package(dir: &Path, name: &str, arch: Architecture) -> std::path::PathBuf {
    fs::create_dir_all(dir).unwrap();
    let source = tempfile::tempdir_in(dir).unwrap();
    let image = source.path().join("os.raw");
    let uki = source.path().join("os.efi");
    fs::write(&image, b"OS image".repeat(512)).unwrap();
    fs::write(&uki, b"UKI".repeat(1024)).unwrap();
    let manifest = Manifest {
        format: 1,
        bao_root: None,
        arch,
        components: vec![Component {
            kind: ComponentType::Os,
            id: "device".into(),
            version: "test".into(),
            root_hash: "a".repeat(64),
            image: teka_pkg::file_descriptor(&image, "os.raw").unwrap(),
            uki: Some(teka_pkg::file_descriptor(&uki, "os.efi").unwrap()),
        }],
    };
    let path = dir.join(name);
    teka_pkg::pack(
        &mut fs::File::create(&path).unwrap(),
        &manifest,
        &BTreeMap::from([("os.raw".into(), image), ("os.efi".into(), uki)]),
        common::signing(true).0,
        common::signing(true).1,
    )
    .unwrap();
    path
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
    if printf '%s\n' "$expr" | grep -q 'pkgs.linkFarm "teka-multi"'; then
        store="${out}-store-teka-multi"
        rm -rf "$out" "$store"
        mkdir -p "$store"
        farm=
        while IFS= read -r line; do
            case "$line" in
                *'pkgs.linkFarm "teka-'*)
                    farm=$(printf '%s\n' "$line" | sed -n 's/.*pkgs.linkFarm "teka-\([^"]*\)".*/\1/p')
                    ;;
                *'name = "'* )
                    name=$(printf '%s\n' "$line" | sed -n 's/.*name = "\([^"]*\)".*/\1/p')
                    if [ -n "$farm" ] && [ "$farm" != multi ]; then
                        mkdir -p "$store/$farm/$name"
                        printf '%s:%s' "$farm" "${TEKA_MOCK_PAYLOAD:-app}" > "$store/$farm/$name/app.raw"
                    fi
                    ;;
            esac
        done <<EOF
$expr
EOF
        ln -s "$store" "$out"
    else
        rm -rf "$out"
        mkdir -p "$out"
        for name in $names; do
            mkdir -p "$out/$name"
            printf '%s' "${TEKA_MOCK_PAYLOAD:-app}" > "$out/$name/app.raw"
        done
    fi
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
    serve_with_hook(replies, |_| {})
}

fn serve_with_hook(
    replies: Vec<(&'static str, &'static str)>,
    mut before_reply: impl FnMut(usize) + Send + 'static,
) -> (String, std::thread::JoinHandle<Vec<Request>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (index, (status, body)) in replies.into_iter().enumerate() {
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
            before_reply(index);
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
fn device_urls_default_scheme_and_preserve_port_and_base_path() {
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
    for (host, expected) in [
        ("device.local", "http://device.local/"),
        ("device.local/proxy/", "http://device.local/proxy/"),
        ("device.local:8443/proxy", "http://device.local:8443/proxy/"),
    ] {
        let client = DeviceClient::new(host).unwrap();
        assert_eq!(client.endpoint("").unwrap().as_str(), expected);
    }
    for url in [
        "http://",
        "ftp://device.local",
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
fn deploy_stops_when_removal_fails() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"amd64"}"#),
        ("500 Internal Server Error", "removal failed"),
    ]);
    let out = cli(d.path())
        .args([
            "app", "deploy", "--build", "app", "--host", &url, "--remove",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("removal failed"));
    assert_eq!(server.join().unwrap().len(), 3);
}

#[test]
fn setup_sends_password() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let (url, server) = serve(vec![
        ("200 OK", r#"{"needs_setup":true}"#),
        ("200 OK", "{}"),
        ("200 OK", r#"{"architecture":"amd64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "hello world"),
    ]);
    success(
        &cli(d.path())
            .env("TEKA_PASSWORD", "secret")
            .args(["app", "deploy", "--build", "app", "--host", &url])
            .output()
            .unwrap(),
    );
    let requests = server.join().unwrap();
    assert_eq!(requests[1].line, "POST /api/v1/auth/setup HTTP/1.1");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&requests[1].body).unwrap(),
        serde_json::json!({"password": "secret"})
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
    fs::create_dir(d.path().join(".git")).unwrap();
    let root = d.path().join("checkout");
    fs::create_dir(&root).unwrap();
    apps_flake(&root);
    let sub = root.join("nested/dir");
    fs::create_dir_all(&sub).unwrap();
    let out = cli(&root)
        .current_dir(&sub)
        .args(["app", "list"])
        .output()
        .unwrap();
    success(&out);
    assert_eq!(out.stdout, b"app\nhello-world\n");

    fs::remove_dir(root.join(".git")).unwrap();
    let out = cli(&root)
        .current_dir(&sub)
        .args(["app", "list"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr)
        .contains(&format!("no apps/flake.nix in {}", d.path().display())));
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
fn package_inspect_lists_manifest_without_verifying() {
    let d = TempDir::new().unwrap();
    let path = os_package(d.path(), "os.tpk", Architecture::X86_64);
    let original = fs::read(&path).unwrap();
    let package = teka_pkg::read(&mut std::io::Cursor::new(&original)).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&package.manifest_bytes).unwrap();
    let out = cli(d.path())
        .args(["package", "inspect"])
        .arg(&path)
        .output()
        .unwrap();
    success(&out);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("Manifest\n  device test (OS, x86_64)\n  root ")
            && text.contains("- os.efi")
            && text.contains("zstd")
            && !text.contains("Verification")
            && !text.contains('✓'),
        "{text}"
    );
    let inspect_json = || {
        let out = cli(d.path())
            .args(["package", "inspect", "--json"])
            .arg(&path)
            .output()
            .unwrap();
        success(&out);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
            serde_json::json!({"manifest": manifest})
        );
    };
    inspect_json();
    let mut changed_signature = original.clone();
    changed_signature[16 + package.signature.len() - 1] ^= 1;
    let mut changed_payload = original;
    *changed_payload.last_mut().unwrap() ^= 1;
    for (bytes, error) in [
        (changed_signature, "authenticate envelope"),
        (changed_payload, "HashMismatch"),
    ] {
        fs::write(&path, bytes).unwrap();
        inspect_json();
        let rejected = cli(d.path())
            .args(["package", "inspect", "--json", "--verify"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(rejected.status.code(), Some(1));
        assert!(rejected.stdout.is_empty());
        assert!(String::from_utf8_lossy(&rejected.stderr).contains(error));
    }
}

#[test]
fn package_inspect_preserves_raw_json_and_bounds_manifest() {
    let d = TempDir::new().unwrap();
    let path = d.path().join("diagnostic.tpk");
    let manifest = serde_json::json!({
        "format": 99, "arch": null, "bao_root": null,
        "components": [{"type": "future", "version": null, "custom": {"nullable": null}}],
        "diagnostic": [1, null, "unknown"]
    });
    let json = serde_json::to_vec(&manifest).unwrap();
    let cms = openssl::pkcs7::Pkcs7::sign(
        common::signing(false).1,
        common::signing(false).0,
        &openssl::stack::Stack::new().unwrap(),
        &json,
        openssl::pkcs7::Pkcs7Flags::NOATTR | openssl::pkcs7::Pkcs7Flags::BINARY,
    )
    .unwrap()
    .to_der()
    .unwrap();
    let mut bytes = teka_pkg::MAGIC.to_vec();
    bytes.extend((cms.len() as u32).to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(&cms);
    fs::write(&path, &bytes).unwrap();
    let inspect = |json: bool| {
        let mut command = cli(d.path());
        command.args(["package", "inspect"]).arg(&path);
        if json {
            command.arg("--json");
        }
        command.output().unwrap()
    };
    let out = inspect(true);
    success(&out);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
        serde_json::json!({"manifest": manifest})
    );
    let text = inspect(false);
    assert_eq!(text.status.code(), Some(1));
    assert!(text.stdout.is_empty());
    let mut invalid = vec![bytes[..15].to_vec(), bytes[..bytes.len() - 1].to_vec()];
    let mut bad_magic = bytes.clone();
    bad_magic[0] ^= 1;
    invalid.push(bad_magic);
    for (offset, size) in [(8, 0), (8, teka_pkg::MAX_SIGNATURE as u32 + 1), (12, 1)] {
        let mut changed = bytes.clone();
        changed[offset..offset + 4].copy_from_slice(&size.to_le_bytes());
        invalid.push(changed);
    }
    for bytes in invalid {
        fs::write(&path, bytes).unwrap();
        let out = inspect(true);
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn cli_rejects_invalid_usage() {
    let d = TempDir::new().unwrap();
    for args in [
        vec!["app", "deploy"],
        vec!["app", "deploy", "--build"],
        vec!["app", "deploy", "hello-world", "--build", "hello-world"],
        vec![
            "app",
            "deploy",
            "--host",
            "http://device",
            "--jobs",
            "0",
            "app",
        ],
        vec!["app", "build"],
        vec!["app", "build", "hello-world", "--arch", "unknown"],
        vec!["package", "inspect", "x.tpk", "--trust", "dir"],
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

#[test]
fn deploy_builds_unique_architectures_once_and_reuses_multi_target_result() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let arm_replies = vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"arm64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "arm log"),
        ("200 OK", r#"{"success":true,"app_id":"hello-world"}"#),
        ("200 OK", "arm log"),
    ];
    let x86_replies = vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"amd64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "x86 log"),
        ("200 OK", r#"{"success":true,"app_id":"hello-world"}"#),
        ("200 OK", "x86 log"),
    ];
    let (arm_tx, arm_rx) = mpsc::channel();
    let (x86_tx, x86_rx) = mpsc::channel();
    let (arm_url, arm_server) = serve_with_hook(arm_replies, move |index| {
        if index == 2 {
            arm_tx.send(()).unwrap();
            x86_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("x86 install must start before arm install finishes");
        }
    });
    let (x86_url, x86_server) = serve_with_hook(x86_replies, move |index| {
        if index == 2 {
            x86_tx.send(()).unwrap();
            arm_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("arm install must start before x86 install finishes");
        }
    });

    let out = cli(d.path())
        .env("TEKA_HOST", "http://127.0.0.1:1")
        .env("TEKA_MOCK_PAYLOAD", "payload")
        .args([
            "app",
            "deploy",
            "--build",
            "--jobs",
            "2",
            "--host",
            &arm_url,
            "--host",
            &x86_url,
            "app",
            "hello-world",
        ])
        .output()
        .unwrap();
    success(&out);

    let arm_requests = arm_server.join().unwrap();
    let x86_requests = x86_server.join().unwrap();
    let install_bodies = |requests: &[Request]| {
        requests
            .iter()
            .filter(|request| request.line.starts_with("POST /api/v1/manager/install"))
            .map(|request| request.body.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        install_bodies(&arm_requests),
        vec![b"aarch64-linux:payload".to_vec(); 2]
    );
    assert_eq!(
        install_bodies(&x86_requests),
        vec![b"x86_64-linux:payload".to_vec(); 2]
    );
    assert_eq!(
        fs::read(d.path().join("result/aarch64-linux/app/app.raw")).unwrap(),
        b"aarch64-linux:payload"
    );
    assert_eq!(
        fs::read(d.path().join("result/x86_64-linux/app/app.raw")).unwrap(),
        b"x86_64-linux:payload"
    );

    let calls = fs::read_to_string(d.path().join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|line| *line == "build").count(), 1);
    assert!(calls.contains("apps.packages.\"x86_64-linux\".\"app-cross-aarch64-linux\""));
    assert!(calls.contains("apps.packages.\"x86_64-linux\".\"hello-world-cross-aarch64-linux\""));
    assert!(calls.contains("apps.packages.\"x86_64-linux\".\"app\""));
    assert!(calls.contains("apps.packages.\"x86_64-linux\".\"hello-world\""));

    let (arm_url, arm_server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"aarch64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "arm log"),
    ]);
    let (x86_url, x86_server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"x86_64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "x86 log"),
    ]);
    success(
        &cli(d.path())
            .args([
                "app", "deploy", "--host", &arm_url, "--host", &x86_url, "app",
            ])
            .output()
            .unwrap(),
    );
    let arm_requests = arm_server.join().unwrap();
    let x86_requests = x86_server.join().unwrap();
    assert_eq!(
        install_bodies(&arm_requests),
        vec![b"aarch64-linux:payload".to_vec()]
    );
    assert_eq!(
        install_bodies(&x86_requests),
        vec![b"x86_64-linux:payload".to_vec()]
    );
}

#[test]
fn deploy_reuses_one_build_for_same_architecture_hosts() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let (first_url, first_server) = serve(vec![
        ("200 OK", r#"{"enforcement":true}"#),
        ("200 OK", "{}"),
        ("200 OK", r#"{"architecture":"arm64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "healthy"),
    ]);
    let (second_url, second_server) = serve(vec![
        ("200 OK", r#"{"enforcement":true}"#),
        ("200 OK", "{}"),
        ("200 OK", r#"{"architecture":"aarch64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "healthy"),
    ]);
    let out = cli(d.path())
        .env("TEKA_PASSWORD", "secret")
        .args([
            "app",
            "deploy",
            "--build",
            "--host",
            &first_url,
            "--host",
            &second_url,
            "app",
        ])
        .output()
        .unwrap();
    success(&out);
    let first_requests = first_server.join().unwrap();
    let second_requests = second_server.join().unwrap();
    for requests in [&first_requests, &second_requests] {
        assert_eq!(requests[1].line, "POST /api/v1/auth/login HTTP/1.1");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&requests[1].body).unwrap()["password"],
            "secret"
        );
        assert_eq!(
            requests
                .iter()
                .find(|request| request.line.starts_with("POST /api/v1/manager/install"))
                .unwrap()
                .body,
            b"app"
        );
    }
    let calls = fs::read_to_string(d.path().join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|line| *line == "build").count(), 1);
    assert!(calls.contains("pkgs.linkFarm \"teka-aarch64-linux\""));
    assert!(!calls.contains("teka-multi"));
    assert!(d.path().join("result/app/app.raw").is_file());
}

#[test]
fn deploy_rejects_mixed_architectures_without_matching_build() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let (arm_url, arm_server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"aarch64"}"#),
    ]);
    let (x86_url, x86_server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"x86_64"}"#),
    ]);
    let out = cli(d.path())
        .args([
            "app", "deploy", "--host", &arm_url, "--host", &x86_url, "app",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("mixed-architecture deploy"));
    assert!(!d.path().join("calls").exists());
    assert_eq!(arm_server.join().unwrap().len(), 2);
    assert_eq!(x86_server.join().unwrap().len(), 2);
}

#[test]
fn deploy_collects_failures_after_other_hosts_finish() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    let (good_url, good_server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"aarch64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "healthy"),
    ]);
    let (bad_url, bad_server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"aarch64"}"#),
        ("500 Internal Server Error", "install failed"),
    ]);
    let out = cli(d.path())
        .args([
            "app", "deploy", "--build", "--host", &good_url, "--host", &bad_url, "app",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&format!("{good_url}: deploy succeeded")),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("{bad_url}: device HTTP 500")),
        "{stderr}"
    );
    assert_eq!(good_server.join().unwrap().len(), 4);
    assert_eq!(bad_server.join().unwrap().len(), 3);
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
    let out = cli(d.path())
        .env("PATH", d.path().join("bin"))
        .args(["--help"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("Plugins:\n  foo  External plugin\n"));
    let out = cli(d.path()).args(["nope"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn plugin_help_uses_metadata_and_command_styles_without_running_plugins() {
    let d = TempDir::new().unwrap();
    let bin = d.path().join("bin");
    fs::create_dir(&bin).unwrap();
    plugin_bin(d.path(), "osdev", "echo ran > plugin-ran");
    plugin_bin(d.path(), "plain", "echo ran > plugin-ran");
    fs::write(
        bin.join("teka-osdev.completion.json"),
        r#"{"version":1,"about":"Build and test OS images"}"#,
    )
    .unwrap();
    fs::write(bin.join("teka-plain.completion.json"), "invalid").unwrap();
    for color in [false, true] {
        let mut command = cli(d.path());
        command
            .env("PATH", &bin)
            .env_remove("NO_COLOR")
            .env_remove("CLICOLOR_FORCE")
            .env("TERM", "xterm")
            .arg("--help");
        if color {
            command.env("CLICOLOR_FORCE", "1");
        } else {
            command.env("NO_COLOR", "1");
        }
        let out = command.output().unwrap();
        success(&out);
        let help = String::from_utf8(out.stdout).unwrap();
        if color {
            assert!(help.contains("\x1b[1m\x1b[4mPlugins:\x1b[0m"));
            assert!(help.contains("\x1b[1mosdev\x1b[0m"));
        } else {
            assert!(
                help.contains(
                    "Plugins:\n  osdev  Build and test OS images\n  plain  External plugin\n"
                ),
                "{help:?}"
            );
            assert!(!help.contains('\x1b'));
        }
    }
    assert!(!d.path().join("plugin-ran").exists());
}

fn system_cli(dir: &Path) -> Command {
    let mut command = cli(dir);
    command
        .env("TEKA_UPDATE_POLL_MS", "10")
        .env("TEKA_UPDATE_WAIT_SEC", "5");
    command
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
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("Plugins:\n  local  External plugin\n")
        );
        let out = cli(d.path())
            .env("PATH", path)
            .arg("local")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(7));
    }
}

#[test]
fn system_status_prints_json_and_update_streams() {
    let d = TempDir::new().unwrap();
    let image = d.path().join("update.bin");
    fs::write(&image, b"sysimage").unwrap();
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"version":"1.0","ok":true}"#),
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"result":"INSTALLED_NO_REBOOT"}"#),
    ]);
    let out = system_cli(d.path())
        .args(["system", "status", "--host", &url])
        .output()
        .unwrap();
    success(&out);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["ok"],
        true
    );
    let out = system_cli(d.path())
        .args([
            "system",
            "update",
            "--no-reboot",
            "--host",
            &url,
            image.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    success(&out);
    let requests = server.join().unwrap();
    assert_eq!(
        requests[1].line,
        "GET /api/v1/app/jetstream/status HTTP/1.1"
    );
    assert_eq!(
        requests[3].line,
        "POST /api/v1/app/jetstream/sysupdate/apply?reboot=false HTTP/1.1"
    );
    assert_eq!(requests[3].body, b"sysimage");
}

#[test]
fn system_update_login_and_http_errors() {
    let d = TempDir::new().unwrap();
    let image = d.path().join("update.bin");
    fs::write(&image, b"x").unwrap();
    let (url, server) = serve(vec![
        ("200 OK", r#"{"enforcement":true}"#),
        ("200 OK", "{}"),
        ("200 OK", r#"{"result":"INSTALLED_NO_REBOOT"}"#),
    ]);
    success(
        &system_cli(d.path())
            .env("TEKA_PASSWORD", "secret")
            .args(["system", "update", "--host", &url, image.to_str().unwrap()])
            .output()
            .unwrap(),
    );
    let requests = server.join().unwrap();
    assert_eq!(requests[1].line, "POST /api/v1/auth/login HTTP/1.1");
    let (url, server) = serve(vec![("401 Unauthorized", "nope")]);
    let out = system_cli(d.path())
        .args(["system", "status", "--host", &url])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("401"));
    server.join().unwrap();
}

#[test]
fn system_update_wait_commits_after_reboot() {
    let d = TempDir::new().unwrap();
    let image = d.path().join("update.bin");
    fs::write(&image, b"payload").unwrap();
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"version":"old"}"#),
        ("200 OK", r#"{"result":"REBOOTING"}"#),
        ("503 Service Unavailable", "restarting"),
        ("200 OK", r#"{"auth_disabled":true}"#),
        (
            "200 OK",
            r#"{"version":"old","update":{"commit_state":"NONE"}}"#,
        ),
        ("200 OK", r#"{"auth_disabled":true}"#),
        (
            "200 OK",
            r#"{"version":"old","update":{"commit_state":"COMMITTED"}}"#,
        ),
        ("200 OK", r#"{"auth_disabled":true}"#),
        (
            "200 OK",
            r#"{"version":"old","update":{"commit_state":"PENDING"}}"#,
        ),
        ("200 OK", r#"{"auth_disabled":true}"#),
        (
            "200 OK",
            r#"{"version":"new","update":{"commit_state":"PENDING"}}"#,
        ),
        ("200 OK", r#"{"auth_disabled":true}"#),
        (
            "200 OK",
            r#"{"version":"new","update":{"commit_state":"PENDING"}}"#,
        ),
        ("200 OK", r#"{"auth_disabled":true}"#),
        (
            "200 OK",
            r#"{"version":"new","update":{"commit_state":"COMMITTED"}}"#,
        ),
    ]);
    let out = system_cli(d.path())
        .args([
            "system",
            "update",
            "--wait",
            "--host",
            &url,
            image.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    success(&out);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("REBOOTING"));
    assert!(stdout.contains("COMMITTED"));
    server.join().unwrap();
}

#[test]
fn system_update_wait_aborts_on_unhealthy() {
    let d = TempDir::new().unwrap();
    let image = d.path().join("update.bin");
    fs::write(&image, b"x").unwrap();
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"version":"old"}"#),
        ("200 OK", r#"{"result":"REBOOTING"}"#),
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"version":"new"}"#),
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"version":"new"}"#),
        ("200 OK", r#"{"auth_disabled":true}"#),
        (
            "200 OK",
            r#"{"version":"new","update":{"commit_state":"ABORTED_UNHEALTHY","failed_units":["a.service","b.service"]}}"#,
        ),
    ]);
    let out = system_cli(d.path())
        .args([
            "system",
            "update",
            "--wait",
            "--host",
            &url,
            image.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("a.service"));
    assert!(stderr.contains("b.service"));
    server.join().unwrap();
}

#[test]
fn system_update_wait_detects_rollback() {
    let d = TempDir::new().unwrap();
    let image = d.path().join("update.bin");
    fs::write(&image, b"x").unwrap();
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"version":"same"}"#),
        ("200 OK", r#"{"result":"REBOOTING"}"#),
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"version":"same"}"#),
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"version":"new"}"#),
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"version":"new"}"#),
        ("200 OK", r#"{"auth_disabled":true}"#),
        (
            "200 OK",
            r#"{"version":"same","update":{"commit_state":"COMMITTED"}}"#,
        ),
    ]);
    let out = system_cli(d.path())
        .args([
            "system",
            "update",
            "--wait",
            "--host",
            &url,
            image.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("rolled back"));
    server.join().unwrap();
}

#[test]
fn system_requires_host() {
    let d = TempDir::new().unwrap();
    let out = system_cli(d.path())
        .args(["system", "status"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

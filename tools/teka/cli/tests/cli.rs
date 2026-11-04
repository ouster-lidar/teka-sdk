mod common;
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
use teka_cli::{
    build::{single_package, target_package, validate_apps},
    deploy::check_logs,
    device::{normalize_arch, DeviceClient},
};
use teka_tpk::{Architecture, Component, ComponentType, Manifest};
use tempfile::TempDir;

fn apps_flake(dir: &Path) {
    fs::create_dir(dir.join("bin")).unwrap();
    fs::create_dir(dir.join(".git")).unwrap();
    fs::create_dir(dir.join("apps")).unwrap();
    fs::write(dir.join("apps/flake.nix"), "{}").unwrap();
    script(&dir.join("bin/nix"), NIX_MOCK);
    for (target, arch) in [
        ("x86_64-linux", Architecture::X86_64),
        ("aarch64-linux", Architecture::Aarch64),
    ] {
        for app in ["app", "hello-world"] {
            app_package(
                &dir.join("packages").join(target),
                &format!("{app}.tpk"),
                arch,
                &[app],
            );
        }
    }
}

fn script(path: &Path, text: &str) {
    fs::write(path, format!("#!/bin/sh\n{text}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn app_package(dir: &Path, name: &str, arch: Architecture, apps: &[&str]) -> std::path::PathBuf {
    fs::create_dir_all(dir).unwrap();
    let source = tempfile::tempdir_in(dir).unwrap();
    let mut components = Vec::new();
    let mut files = BTreeMap::new();
    for app in apps {
        let raw = source.path().join(format!("{app}.raw"));
        fs::write(&raw, b"test app image".repeat(256)).unwrap();
        let image = teka_tpk::file_descriptor(&raw, &format!("{app}.raw")).unwrap();
        files.insert(image.path.clone(), raw);
        components.push(Component {
            kind: ComponentType::App,
            id: (*app).into(),
            version: "test".into(),
            root_hash: "a".repeat(64),
            image,
            uki: None,
        });
    }
    let manifest = Manifest {
        format: 1,
        bao_root: None,
        arch,
        components,
    };
    let path = dir.join(name);
    teka_tpk::pack(
        &mut fs::File::create(&path).unwrap(),
        &manifest,
        &files,
        common::signing(false).0,
        common::signing(false).1,
    )
    .unwrap();
    path
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
            image: teka_tpk::file_descriptor(&image, "os.raw").unwrap(),
            uki: Some(teka_tpk::file_descriptor(&uki, "os.efi").unwrap()),
        }],
    };
    let path = dir.join(name);
    teka_tpk::pack(
        &mut fs::File::create(&path).unwrap(),
        &manifest,
        &BTreeMap::from([("os.raw".into(), image), ("os.efi".into(), uki)]),
        common::signing(true).0,
        common::signing(true).1,
    )
    .unwrap();
    path
}

fn prebuilt_app(dir: &Path, app: &str, arch: Architecture) -> std::path::PathBuf {
    let output = dir.join("result").join(app);
    app_package(&output, "app.tpk", arch, &[app])
}

fn corrupt_package(path: &Path, member: usize) {
    let mut file = fs::File::open(path).unwrap();
    let package = teka_tpk::read(&mut file).unwrap();
    let mut bytes = fs::read(path).unwrap();
    let member = &package.members[member];
    let envelope = 16 + u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let offset = envelope + (member.offset + member.size - 1) as usize;
    bytes[offset] ^= 1;
    fs::write(path, bytes).unwrap();
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
                        cp "$TEKA_MOCK_PACKAGES/$farm/$name.tpk" "$store/$farm/$name/app.tpk"
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
        farm=$(printf '%s\n' "$expr" | sed -n 's/.*pkgs.linkFarm "teka-\([^"]*\)".*/\1/p')
        for name in $names; do
            mkdir -p "$out/$name"
            cp "$TEKA_MOCK_PACKAGES/$farm/$name.tpk" "$out/$name/app.tpk"
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
        .env("TEKA_MOCK_PACKAGES", dir.join("packages"))
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
    assert!(single_package(d.path()).is_err());
    fs::write(d.path().join("app.tpk"), b"data").unwrap();
    assert_eq!(
        single_package(d.path()).unwrap().file_name().unwrap(),
        "app.tpk"
    );
    fs::write(d.path().join("other.tpk"), b"data").unwrap();
    assert!(single_package(d.path()).is_err());
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
    let package = prebuilt_app(d.path(), "app", Architecture::X86_64);
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
        .args(["app", "deploy", "app", "--profile", "a b", "--remove"])
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
    assert_eq!(requests[4].body, fs::read(package).unwrap());
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
    let package = prebuilt_app(d.path(), "app", Architecture::X86_64);
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
            .args(["app", "deploy", "app", "--host", &url])
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
    assert_eq!(requests[3].body, fs::read(package).unwrap());
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
    let mut file = fs::File::open(d.path().join("result/hello-world/app.tpk")).unwrap();
    let package = teka_tpk::read(&mut file).unwrap();
    teka_tpk::verify(&mut file, &package).unwrap();
    assert_eq!(package.manifest.components[0].id, "hello-world");
    let calls = fs::read_to_string(d.path().join("calls")).unwrap();
    assert!(calls.contains("apps.packages.\"x86_64-linux\".\"hello-world-cross-aarch64-linux\""));
    assert!(calls.contains("apps.packages.\"x86_64-linux\".\"hello-world\";"));
    assert!(calls.contains("builtins.getFlake") && calls.contains("?dir=apps"));
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
    let package = teka_tpk::read(&mut std::io::Cursor::new(&original)).unwrap();
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
    let mut bytes = teka_tpk::MAGIC.to_vec();
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
    for (offset, size) in [(8, 0), (8, teka_tpk::MAX_SIGNATURE as u32 + 1), (12, 1)] {
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
fn package_stream_rejects_corruption_in_selected_archive_group_before_output() {
    let directory = TempDir::new().unwrap();
    let package = app_package(
        directory.path(),
        "apps.tpk",
        Architecture::X86_64,
        &["app", "other"],
    );
    fs::write(
        directory.path().join("apps.pem"),
        common::signing(false).1.to_pem().unwrap(),
    )
    .unwrap();
    let stream = || {
        cli(directory.path())
            .args(["package", "stream"])
            .arg(&package)
            .arg("--trust")
            .arg(directory.path())
            .args(["--member", "app.raw", "--offset", "23", "--size", "101"])
            .output()
            .unwrap()
    };
    let valid = stream();
    success(&valid);
    assert_eq!(valid.stdout, b"test app image".repeat(256)[23..124]);
    corrupt_package(&package, 1);
    let rejected = stream();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("HashMismatch"));
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
    prebuilt_app(d.path(), "hello-world", Architecture::X86_64);
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"amd64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"hello-world"}"#),
        ("200 OK", "hello world"),
    ]);
    success(
        &cli(d.path())
            .env("TEKA_HOST", "http://127.0.0.1:1")
            .args(["app", "deploy", "hello-world", "--host", &url])
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
        ("200 OK", "healthy"),
        ("200 OK", r#"{"success":true,"app_id":"hello-world"}"#),
        ("200 OK", "healthy"),
    ];
    let x86_replies = vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"amd64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "healthy"),
        ("200 OK", r#"{"success":true,"app_id":"hello-world"}"#),
        ("200 OK", "healthy"),
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
    for (target, requests) in [
        ("aarch64-linux", &arm_requests),
        ("x86_64-linux", &x86_requests),
    ] {
        let expected = ["app", "hello-world"].map(|app| {
            fs::read(
                d.path()
                    .join("packages")
                    .join(target)
                    .join(format!("{app}.tpk")),
            )
            .unwrap()
        });
        assert_eq!(install_bodies(requests), expected);
        assert_eq!(
            fs::read(d.path().join("result").join(target).join("app/app.tpk")).unwrap(),
            expected[0]
        );
    }
    let calls = fs::read_to_string(d.path().join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|line| *line == "build").count(), 1);
    for name in ["app", "hello-world"] {
        assert!(calls.contains(&format!(
            "apps.packages.\"x86_64-linux\".\"{name}-cross-aarch64-linux\""
        )));
        assert!(calls.contains(&format!("apps.packages.\"x86_64-linux\".\"{name}\"")));
    }
    let (arm_url, arm_server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"aarch64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "healthy"),
    ]);
    let (x86_url, x86_server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"x86_64"}"#),
        ("200 OK", r#"{"success":true,"app_id":"app"}"#),
        ("200 OK", "healthy"),
    ]);
    success(
        &cli(d.path())
            .args([
                "app", "deploy", "--host", &arm_url, "--host", &x86_url, "app",
            ])
            .output()
            .unwrap(),
    );
    for (target, requests) in [
        ("aarch64-linux", arm_server.join().unwrap()),
        ("x86_64-linux", x86_server.join().unwrap()),
    ] {
        assert_eq!(
            install_bodies(&requests),
            vec![fs::read(d.path().join("packages").join(target).join("app.tpk")).unwrap()]
        );
    }
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
    for requests in [first_server.join().unwrap(), second_server.join().unwrap()] {
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[1].line, "POST /api/v1/auth/login HTTP/1.1");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&requests[1].body).unwrap()["password"],
            "secret",
        );
    }
    let calls = fs::read_to_string(d.path().join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|line| *line == "build").count(), 1);
    assert!(calls.contains("pkgs.linkFarm \"teka-aarch64-linux\""));
    assert!(!calls.contains("teka-multi"));
    assert!(d.path().join("result/app/app.tpk").is_file());
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
fn deploy_rejects_invalid_files_before_contacting_device() {
    let d = TempDir::new().unwrap();
    let valid = app_package(d.path(), "valid.tpk", Architecture::Aarch64, &["app"]);
    let raw = d.path().join("app.raw");
    fs::write(&raw, fs::read(&valid).unwrap()).unwrap();
    let malformed = d.path().join("malformed.tpk");
    fs::write(&malformed, b"not an archive").unwrap();
    let corrupt = app_package(d.path(), "corrupt.tpk", Architecture::Aarch64, &["app"]);
    corrupt_package(&corrupt, 0);
    let multiple = app_package(
        d.path(),
        "multiple.tpk",
        Architecture::Aarch64,
        &["app", "other"],
    );
    let os = os_package(d.path(), "os.tpk", Architecture::Aarch64);
    for (path, expected) in [
        (raw, "requires a .tpk file"),
        (malformed, "read TPK envelope"),
        (corrupt, "HashMismatch"),
        (multiple, "exactly one component"),
        (os, "requires an app component"),
    ] {
        let out = cli(d.path())
            .args(["app", "deploy", "--host", "http://127.0.0.1:1"])
            .arg(&valid)
            .arg(&path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains(expected) && stderr.contains(path.to_str().unwrap()),
            "{stderr}"
        );
        assert!(!stderr.contains("authenticate"), "{stderr}");
    }
    for option in ["--build", "--remove"] {
        let out = cli(d.path())
            .args(["app", "deploy", "--host", "http://127.0.0.1:1", option])
            .arg(&valid)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&out.stderr).contains("file inputs cannot be used"));
    }
}

#[test]
fn deploy_accepts_bare_package_filename_and_preserves_named_app_lookup() {
    let d = TempDir::new().unwrap();
    let package = app_package(d.path(), "external.tpk", Architecture::X86_64, &["app"]);
    let named_package = prebuilt_app(d.path(), "app", Architecture::X86_64);
    fs::write(d.path().join("app"), b"not a package").unwrap();
    for input in ["external.tpk", "app"] {
        let (url, server) = serve(vec![
            ("200 OK", r#"{"auth_disabled":true}"#),
            ("200 OK", r#"{"architecture":"amd64"}"#),
            ("200 OK", r#"{"success":true,"app_id":"runtime-app"}"#),
            ("200 OK", "healthy"),
        ]);
        success(
            &cli(d.path())
                .args(["app", "deploy", "--host", &url, input])
                .output()
                .unwrap(),
        );
        let requests = server.join().unwrap();
        let expected = if input == "app" {
            &named_package
        } else {
            &package
        };
        assert_eq!(requests[2].body, fs::read(expected).unwrap());
        assert!(requests[3].line.contains("/runtime-app/logs"));
    }
}

#[test]
fn deploy_preflight_blocks_device_mutation() {
    let d = TempDir::new().unwrap();
    prebuilt_app(d.path(), "app", Architecture::Aarch64);
    let invalid = prebuilt_app(d.path(), "hello-world", Architecture::Aarch64);
    corrupt_package(&invalid, 0);
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"aarch64"}"#),
        ("200 OK", "{}"),
    ]);
    let out = cli(d.path())
        .args([
            "app",
            "deploy",
            "--remove",
            "--host",
            &url,
            "app",
            "hello-world",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("HashMismatch") && stderr.contains("hello-world/app.tpk"),
        "{stderr}"
    );
    assert!(DeviceClient::new(&url).unwrap().get_json("done").is_ok());
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|r| r.line.starts_with("GET ")));

    for (id, arch, expected) in [
        (
            "app",
            Architecture::X86_64,
            "is for x86_64; device is aarch64",
        ),
        (
            "other",
            Architecture::Aarch64,
            "has app ID other, expected app",
        ),
    ] {
        app_package(&d.path().join("result/app"), "app.tpk", arch, &[id]);
        let (url, server) = serve(vec![
            ("200 OK", r#"{"auth_disabled":true}"#),
            ("200 OK", r#"{"architecture":"aarch64"}"#),
            ("200 OK", "{}"),
        ]);
        let out = cli(d.path())
            .args(["app", "deploy", "--remove", "--host", &url, "app"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(expected) && stderr.contains("result/app/app.tpk"),
            "{stderr}"
        );
        assert!(DeviceClient::new(&url).unwrap().get_json("done").is_ok());
        assert!(server
            .join()
            .unwrap()
            .iter()
            .all(|r| r.line.starts_with("GET ")));
    }

    apps_flake(d.path());
    corrupt_package(&d.path().join("packages/x86_64-linux/app.tpk"), 0);
    let (arm_url, arm_server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"aarch64"}"#),
        ("200 OK", "{}"),
    ]);
    let (x86_url, x86_server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"x86_64"}"#),
        ("200 OK", "{}"),
    ]);
    let out = cli(d.path())
        .args([
            "app", "deploy", "--build", "--remove", "--host", &arm_url, "--host", &x86_url, "app",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("HashMismatch") && stderr.contains("x86_64-linux/app/app.tpk"),
        "{stderr}"
    );
    for (url, server) in [(&arm_url, arm_server), (&x86_url, x86_server)] {
        assert!(DeviceClient::new(url).unwrap().get_json("done").is_ok());
        assert!(server
            .join()
            .unwrap()
            .iter()
            .all(|r| r.line.starts_with("GET ")));
    }
}

#[test]
fn deploy_collects_failures_after_other_hosts_finish() {
    let d = TempDir::new().unwrap();
    apps_flake(d.path());
    prebuilt_app(d.path(), "app", Architecture::Aarch64);
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
            "app", "deploy", "--host", &good_url, "--host", &bad_url, "app",
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
fn plugins_dispatch_with_builtin_precedence() {
    let d = TempDir::new().unwrap();
    let bin = d.path().join("bin");
    let shadow = d.path().join("shadow");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&shadow).unwrap();
    for (parents, name, builtins) in [
        (&[][..], "foo", &["system", "app", "help"][..]),
        (
            &["app"][..],
            "test",
            &["build", "list", "deploy", "help"][..],
        ),
        (&["app"][..], "another-name", &[][..]),
    ] {
        let plugin = [parents, &[name]].concat().join("-");
        plugin_bin(d.path(), &plugin, "printf '%s\\n' \"$@\"; exit 7");
        script(&shadow.join(format!("teka-{plugin}")), "exit 99");
        for args in [
            vec!["init"],
            vec!["check"],
            vec!["cmd", "--", "two words", "--flag", ""],
            vec!["--help"],
        ] {
            let out = cli(d.path())
                .env("PATH", std::env::join_paths([&bin, &shadow]).unwrap())
                .args(parents)
                .arg(name)
                .args(&args)
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(7), "{out:?}");
            assert_eq!(out.stdout, format!("{}\n", args.join("\n")).as_bytes());
        }
        for builtin in builtins {
            plugin_bin(
                d.path(),
                &[parents, &[*builtin]].concat().join("-"),
                "exit 99",
            );
            let mut command = cli(d.path());
            command.env("PATH", &bin).args(parents).arg(builtin);
            if *builtin != "help" {
                command.arg("--help");
            }
            success(&command.output().unwrap());
        }
    }
    plugin_bin(d.path(), "missing", "exit 99");
    for args in [
        &["app", "missing", "init"][..],
        &["app", "build", "--unknown"],
        &["system"],
        &["nope"],
    ] {
        let out = cli(d.path()).env("PATH", &bin).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        if args.contains(&"missing") {
            assert!(String::from_utf8_lossy(&out.stderr).contains("no teka-app-missing on PATH"));
        }
    }
}

#[test]
fn nested_plugins_appear_only_in_their_parent_help() {
    let d = TempDir::new().unwrap();
    let bin = d.path().join("bin");
    let shadow = d.path().join("shadow");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&shadow).unwrap();
    for name in [
        "app-test",
        "app-another-name",
        "app-build",
        "app-help",
        "osdev",
        "system-tool",
    ] {
        plugin_bin(d.path(), name, "echo ran > plugin-ran");
    }
    script(&shadow.join("teka-app-test"), "exit 99");
    fs::write(
        bin.join("teka-app-test.completion.json"),
        r#"{"version":1,"about":"Nested test plugin"}"#,
    )
    .unwrap();
    fs::write(
        shadow.join("teka-app-test.completion.json"),
        r#"{"version":1,"about":"Shadow plugin"}"#,
    )
    .unwrap();
    for args in [
        vec![],
        vec!["--help"],
        vec!["app"],
        vec!["app", "--help"],
        vec!["help", "app"],
    ] {
        let out = cli(d.path())
            .env("PATH", std::env::join_paths([&bin, &shadow]).unwrap())
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(if args == ["app"] { 2 } else { 0 }));
        let help = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!help.contains("app-test"), "{help}");
        assert!(!help.contains("app-build"), "{help}");
        assert!(!help.contains("app-help"), "{help}");
        assert!(!help.contains("Shadow plugin"), "{help}");
        if args.contains(&"app") {
            assert!(help.contains("Nested test plugin"), "{help}");
            assert!(help.contains("another-name"), "{help}");
            assert!(!help.contains("osdev"), "{help}");
        } else {
            assert!(help.contains("osdev"), "{help}");
            assert!(help.contains("system-tool"), "{help}");
            assert!(!help.contains("Nested test plugin"), "{help}");
        }
    }
    assert!(!d.path().join("plugin-ran").exists());
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
    let image = os_package(d.path(), "update.tpk", Architecture::X86_64);
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"version":"1.0","ok":true}"#),
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"amd64"}"#),
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
    assert_eq!(requests[1].line, "GET /api/v1/app/teka/status HTTP/1.1");
    assert_eq!(
        requests[3].line,
        "GET /api/v1/platform/architecture HTTP/1.1"
    );
    assert_eq!(
        requests[4].line,
        "POST /api/v1/app/teka/sysupdate/apply?reboot=false HTTP/1.1"
    );
    assert_eq!(requests[4].body, fs::read(&image).unwrap());
}

#[test]
fn system_update_login_and_http_errors() {
    let d = TempDir::new().unwrap();
    let image = os_package(d.path(), "update.tpk", Architecture::X86_64);
    let (url, server) = serve(vec![
        ("200 OK", r#"{"enforcement":true}"#),
        ("200 OK", "{}"),
        ("200 OK", r#"{"architecture":"x86_64"}"#),
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
    assert_eq!(
        requests[2].line,
        "GET /api/v1/platform/architecture HTTP/1.1"
    );
    assert_eq!(requests[3].body, fs::read(&image).unwrap());
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
    let image = os_package(d.path(), "update.tpk", Architecture::X86_64);
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"x86_64"}"#),
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
    let image = os_package(d.path(), "update.tpk", Architecture::X86_64);
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"x86_64"}"#),
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
    let image = os_package(d.path(), "update.tpk", Architecture::X86_64);
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"x86_64"}"#),
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
fn system_update_rejects_invalid_packages_before_contacting_device() {
    let d = TempDir::new().unwrap();
    let valid = os_package(d.path(), "valid.tpk", Architecture::X86_64);
    let raw = d.path().join("os.raw");
    std::os::unix::fs::symlink(&valid, &raw).unwrap();
    let invalid_extension = d.path().join("update.invalid");
    fs::copy(&valid, &invalid_extension).unwrap();
    let malformed = d.path().join("malformed.tpk");
    fs::write(&malformed, b"not an archive").unwrap();
    let directory = d.path().join("directory.tpk");
    fs::create_dir(&directory).unwrap();
    let app = app_package(d.path(), "app.tpk", Architecture::X86_64, &["app"]);
    let multiple = app_package(
        d.path(),
        "multiple.tpk",
        Architecture::X86_64,
        &["app", "other"],
    );
    let image_digest = os_package(d.path(), "image-digest.tpk", Architecture::X86_64);
    corrupt_package(&image_digest, 0);
    let uki_digest = os_package(d.path(), "uki-digest.tpk", Architecture::X86_64);
    corrupt_package(&uki_digest, 1);
    for (path, expected) in [
        (raw, "requires a .tpk file"),
        (invalid_extension, "requires a .tpk file"),
        (malformed, "read TPK envelope"),
        (directory, "input must be a regular file"),
        (app, "requires an OS component"),
        (multiple, "exactly one component"),
        (image_digest, "HashMismatch"),
        (uki_digest, "HashMismatch"),
    ] {
        let out = system_cli(d.path())
            .args(["system", "update", "--host", "http://127.0.0.1:1"])
            .arg(&path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains(expected)
                && stderr.contains(path.file_name().unwrap().to_str().unwrap()),
            "{stderr}"
        );
    }
}

#[test]
fn system_update_rejects_architecture_before_upload() {
    let d = TempDir::new().unwrap();
    let image = os_package(d.path(), "update.tpk", Architecture::X86_64);
    let (url, server) = serve(vec![
        ("200 OK", r#"{"auth_disabled":true}"#),
        ("200 OK", r#"{"architecture":"arm64"}"#),
        ("200 OK", "{}"),
    ]);
    let out = system_cli(d.path())
        .args(["system", "update", "--host", &url])
        .arg(&image)
        .output()
        .unwrap();
    assert!(DeviceClient::new(&url).unwrap().get_json("done").is_ok());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("is for x86_64; device is aarch64"));
    assert!(server
        .join()
        .unwrap()
        .iter()
        .all(|r| r.line.starts_with("GET ")));
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

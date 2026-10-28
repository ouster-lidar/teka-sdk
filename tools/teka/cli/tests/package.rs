#![forbid(unsafe_code)]

mod common;

use openssl::{
    cms::{CMSOptions, CmsContentInfo},
    stack::Stack,
    x509::X509,
};
use serde_json::json;
use std::{fs, process::Command};

#[test]
fn pack_zstd_encodes_and_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("demo.raw");
    fs::write(&raw, [0u8; 4096]).unwrap();
    let image = teka_pkg::file_descriptor(&raw, "demo.raw").unwrap();
    let manifest = json!({
        "format": 1, "arch": "x86_64",
        "components": [{"type": "app", "id": "demo", "version": "1", "root_hash": "a".repeat(64), "image": image}],
    });
    let input = dir.path().join("manifest.json");
    fs::write(&input, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    let key = dir.path().join("apps.key");
    let certificate = dir.path().join("apps.pem");
    fs::write(
        &key,
        common::signing(false).0.private_key_to_pem_pkcs8().unwrap(),
    )
    .unwrap();
    fs::write(&certificate, common::signing(false).1.to_pem().unwrap()).unwrap();
    let tool = env!("CARGO_BIN_EXE_teka");
    let pack = |output: &std::path::Path, level: &str| {
        Command::new(tool)
            .args(["package", "pack", "--manifest"])
            .arg(&input)
            .arg("--private-key")
            .arg(&key)
            .arg("--certificate")
            .arg(&certificate)
            .arg("--member")
            .arg(format!("demo.raw={}", raw.display()))
            .arg("--output")
            .arg(output)
            .env("TEKA_ZSTD_LEVEL", level)
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(
        !pack(&dir.path().join("bad.tpk"), "20"),
        "ultra levels exceed the decoder window"
    );
    assert!(pack(&dir.path().join("low.tpk"), "1"));
    let output = dir.path().join("app.tpk");
    assert!(pack(&output, "19"));
    let mut file = fs::File::open(&output).unwrap();
    let package = teka_pkg::read(&mut file).unwrap();
    teka_pkg::verify(&mut file, &package).unwrap();
    let mut finalized = manifest.clone();
    finalized["bao_root"] = serde_json::to_value(&package.manifest.bao_root).unwrap();
    finalized["components"][0]["image"]["size"] = json!(package.members[0].size);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&package.manifest_bytes).unwrap(),
        finalized
    );
    assert_ne!(package.manifest_bytes, fs::read(&input).unwrap());
    let mut signers = Stack::new().unwrap();
    signers
        .push(X509::from_pem(&fs::read(&certificate).unwrap()).unwrap())
        .unwrap();
    let mut recovered = Vec::new();
    CmsContentInfo::from_der(&package.signature)
        .unwrap()
        .verify(
            Some(&signers),
            None,
            None,
            Some(&mut recovered),
            CMSOptions::NOINTERN | CMSOptions::NO_SIGNER_CERT_VERIFY | CMSOptions::BINARY,
        )
        .unwrap();
    assert_eq!(recovered, package.manifest_bytes);
    let cms_file = dir.path().join("manifest.cms");
    let finalized_file = dir.path().join("finalized.json");
    fs::write(&finalized_file, &package.manifest_bytes).unwrap();
    let signed = Command::new("openssl")
        .args(["cms", "-sign", "-binary", "-noattr", "-nodetach"])
        .args(["-md", "sha256", "-outform", "DER", "-in"])
        .arg(&finalized_file)
        .arg("-signer")
        .arg(&certificate)
        .arg("-inkey")
        .arg(&key)
        .arg("-out")
        .arg(&cms_file)
        .output()
        .unwrap();
    assert!(
        signed.status.success(),
        "{}",
        String::from_utf8_lossy(&signed.stderr)
    );
    let cms = fs::read(&cms_file).unwrap();
    let packed = fs::read(&output).unwrap();
    let mut interoperable = teka_pkg::MAGIC.to_vec();
    interoperable.extend((cms.len() as u32).to_le_bytes());
    interoperable.extend(0u32.to_le_bytes());
    interoperable.extend(cms);
    interoperable.extend(&packed[16 + package.signature.len()..]);
    let mut source = std::io::Cursor::new(interoperable);
    let external = teka_pkg::read(&mut source).unwrap();
    teka_pkg::verify(&mut source, &external).unwrap();
    assert_eq!(external.manifest_bytes, package.manifest_bytes);
    assert!(!package.signature.is_empty());
    let stream = |arguments: &[&str]| {
        Command::new(tool)
            .args(["package", "stream"])
            .arg(&output)
            .arg("--trust")
            .arg(dir.path())
            .args(["--member", "demo.raw"])
            .args(arguments)
            .output()
            .unwrap()
    };
    let range = stream(&["--offset", "23", "--size", "101"]);
    assert!(range.status.success());
    assert!(range.stderr.is_empty());
    assert_eq!(range.stdout, vec![0; 101]);
    let invalid = stream(&["--offset", "4097"]);
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    let before = fs::read(&output).unwrap();
    assert!(
        !pack(&output, "9"),
        "existing output must not be overwritten"
    );
    assert_eq!(fs::read(&output).unwrap(), before);
    assert!(fs::read_dir(dir.path()).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".tmp")));
    let noise = (0..4096u64)
        .scan(0x9e37_79b9_7f4a_7c15u64, |x, _| {
            *x ^= *x << 13;
            *x ^= *x >> 7;
            *x ^= *x << 17;
            Some(*x as u8)
        })
        .collect::<Vec<_>>();
    fs::write(&raw, &noise).unwrap();
    let noise_output = dir.path().join("noise.tpk");
    assert!(pack(&noise_output, "9"));
    let mut bytes = fs::read(&noise_output).unwrap();
    let package = teka_pkg::read(&mut std::io::Cursor::new(&bytes)).unwrap();
    assert!(package.members[0].size > noise.len() as u64);
    let envelope = 16 + u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    bytes[envelope + package.members[0].offset as usize + 2048] ^= 1;
    fs::write(&noise_output, bytes).unwrap();
    let rejected = Command::new(tool)
        .args(["package", "stream"])
        .arg(&noise_output)
        .arg("--trust")
        .arg(dir.path())
        .args(["--member", "demo.raw", "--offset", "0", "--size", "16"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("HashMismatch"));
    fs::write(&output, &before[..before.len() - 1024]).unwrap();
    let truncated = stream(&[]);
    assert!(!truncated.status.success());
    assert!(truncated.stdout.is_empty());
}

fn serve_package(
    bytes: Vec<u8>,
    status: &str,
    chunked: bool,
) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/release.tpk", listener.local_addr().unwrap());
    let status = status.to_owned();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            socket.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        assert!(String::from_utf8_lossy(&request)
            .to_ascii_lowercase()
            .contains("accept-encoding: identity"));
        let header = if chunked {
            format!("HTTP/1.1 {status}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
        } else {
            format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
        };
        if socket.write_all(header.as_bytes()).is_err() {
            return;
        }
        for fragment in bytes.chunks(997) {
            if chunked && write!(socket, "{:x}\r\n", fragment.len()).is_err() {
                return;
            }
            if socket.write_all(fragment).is_err() {
                return;
            }
            if chunked && socket.write_all(b"\r\n").is_err() {
                return;
            }
        }
        if chunked {
            let _ = socket.write_all(b"0\r\n\r\n");
        }
    });
    (url, server)
}

#[test]
fn trusted_http_streams_third_member_and_rejects_incomplete_transport() {
    use std::collections::BTreeMap;
    let directory = tempfile::tempdir().unwrap();
    let trust = directory.path();
    fs::write(
        trust.join("apps.pem"),
        common::signing(false).1.to_pem().unwrap(),
    )
    .unwrap();
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut files = BTreeMap::new();
    let mut components = Vec::new();
    let mut payloads = Vec::new();
    for id in ["first", "second", "third"] {
        let mut bytes = (0..180_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect::<Vec<_>>();
        if id == "second" {
            bytes.fill(0xab);
        }
        let path = trust.join(format!("{id}.raw"));
        fs::write(&path, &bytes).unwrap();
        let image = teka_pkg::file_descriptor(&path, &format!("{id}.raw")).unwrap();
        files.insert(image.path.clone(), path);
        components.push(teka_pkg::Component {
            kind: teka_pkg::ComponentType::App,
            id: id.into(),
            version: "test".into(),
            root_hash: "a".repeat(64),
            image,
            uki: None,
        });
        payloads.push(bytes);
    }
    let manifest = teka_pkg::Manifest {
        format: 1,
        bao_root: None,
        arch: teka_pkg::Architecture::X86_64,
        components,
    };
    let mut bytes = Vec::new();
    teka_pkg::pack(
        &mut bytes,
        &manifest,
        &files,
        common::signing(false).0,
        common::signing(false).1,
    )
    .unwrap();
    let package = teka_pkg::read(&mut std::io::Cursor::new(&bytes)).unwrap();
    assert!(package.members[0].size > payloads[0].len() as u64);
    assert!(package.members[1].size < payloads[1].len() as u64);
    assert!(package.members[2].size > payloads[2].len() as u64);
    let run = |source: &str, member: &str| {
        Command::new(env!("CARGO_BIN_EXE_teka"))
            .args([
                "package", "stream", source, "--member", member, "--offset", "23", "--size", "101",
                "--trust",
            ])
            .arg(trust)
            .output()
            .unwrap()
    };
    for chunked in [false, true] {
        let (url, server) = serve_package(bytes.clone(), "200 OK", chunked);
        let output = run(&url, "third.raw");
        server.join().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, payloads[2][23..124]);
    }
    let envelope = 16 + u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut corrupted_prefix = bytes.clone();
    corrupted_prefix[envelope + 4096] ^= 1;
    let disk = trust.join("partial.tpk");
    fs::write(&disk, &corrupted_prefix).unwrap();
    let output = run(disk.to_str().unwrap(), "third.raw");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, payloads[2][23..124]);
    let mut reader = fs::File::open(&disk).unwrap();
    let package = teka_pkg::read(&mut reader).unwrap();
    assert!(teka_pkg::verify(&mut reader, &package).is_err());
    let mut changed_manifest = bytes.clone();
    let manifest_offset = 16
        + package
            .signature
            .windows(package.manifest_bytes.len())
            .position(|window| window == package.manifest_bytes)
            .unwrap();
    changed_manifest[manifest_offset] ^= 1;
    let mut changed_signature = bytes.clone();
    changed_signature[envelope - 1] ^= 1;
    for invalid in [corrupted_prefix, changed_manifest, changed_signature] {
        let (url, server) = serve_package(invalid, "200 OK", true);
        let output = run(&url, "third.raw");
        server.join().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let mut changed_tail = bytes.clone();
    *changed_tail.last_mut().unwrap() ^= 1;
    let mut appended = bytes.clone();
    appended.push(0);
    let truncated = bytes[..bytes.len() - 1].to_vec();
    for chunked in [false, true] {
        for invalid in [&changed_tail, &appended, &truncated] {
            let (url, server) = serve_package(invalid.clone(), "200 OK", chunked);
            let output = run(&url, "first.raw");
            server.join().unwrap();
            assert!(!output.status.success());
            assert_eq!(output.stdout, payloads[0][23..124]);
        }
    }
    for status in ["404 Not Found", "302 Found"] {
        let (url, server) = serve_package(Vec::new(), status, false);
        let output = run(&url, "third.raw");
        server.join().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(&status[..3]));
    }
}

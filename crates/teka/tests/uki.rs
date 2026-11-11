#![cfg(feature = "tpk")]
#![forbid(unsafe_code)]

use base64::{engine::general_purpose::STANDARD, Engine};
use openssl::x509::X509;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use teka::tpk::{
    uki::{verify, verify_component},
    Architecture, Component, ComponentType, FileDescriptor,
};

const ROOT_HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PE_TEMPLATE: &str = "TVoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABQRQAAZIYDAAAAAAAAAAAAAAAAAPAAIgALAgAAAAIAAAAAAAAAAAAAABAAAAAQAAAAAEAAAAAAAAAQAAAAAgAAAAAAAAAAAAAAAAAAAAAAAABAAAAABAAAAAAAAAoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAudGV4dAAAAAEAAAAAEAAAAAIAAAAEAAAAAAAAAAAAAAAAAAAgAABgLmNtZGxpbmUAAAAAACAAAAACAAAABgAAAAAAAAAAAAAAAAAAQAAAQC5vc3JlbAAAAAAAAAAwAAAAAgAAAAgAAAAAAAAAAAAAAAAAAEAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAMMAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn unsigned_uki(cmdline: &str, osrel: &str) -> Vec<u8> {
    let mut bytes = STANDARD.decode(PE_TEMPLATE).unwrap();
    for (i, content) in [(1, cmdline.as_bytes()), (2, osrel.as_bytes())] {
        let h = 392 + i * 40;
        let off = 1024 + i * 512;
        put32(&mut bytes, h + 8, content.len() as u32);
        bytes[off..off + content.len()].copy_from_slice(content);
    }
    bytes
}

fn certificate(directory: &Path, bits: u32) -> X509 {
    let key = directory.join("key.pem");
    let cert = directory.join("cert.pem");
    assert!(Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-nodes",
            "-newkey",
            &format!("rsa:{bits}"),
            "-keyout",
            &key.to_string_lossy(),
            "-out",
            &cert.to_string_lossy(),
            "-days",
            "1",
            "-subj",
            "/CN=TPK UKI test",
        ])
        .status()
        .unwrap()
        .success());
    X509::from_pem(&fs::read(cert).unwrap()).unwrap()
}

fn sign_osrel(directory: &Path, cmdline: &str, osrel: &str) -> PathBuf {
    let unsigned = directory.join("unsigned.efi");
    let signed = directory.join("signed.efi");
    fs::write(&unsigned, unsigned_uki(cmdline, osrel)).unwrap();
    #[cfg(not(target_os = "macos"))]
    let output = Command::new("sbsign")
        .arg("--key")
        .arg(directory.join("key.pem"))
        .arg("--cert")
        .arg(directory.join("cert.pem"))
        .arg("--output")
        .arg(&signed)
        .arg(&unsigned)
        .output()
        .unwrap();
    #[cfg(target_os = "macos")]
    let output = Command::new("osslsigncode")
        .args(["sign", "-key"])
        .arg(directory.join("key.pem"))
        .arg("-certs")
        .arg(directory.join("cert.pem"))
        .arg("-in")
        .arg(&unsigned)
        .arg("-out")
        .arg(&signed)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    signed
}

#[test]
fn uki_verification() {
    let directory = tempfile::tempdir().unwrap();
    let trusted = certificate(directory.path(), 3072);
    let trust = std::slice::from_ref(&trusted);
    let cmdline = format!("quiet roothash={ROOT_HASH}");
    let path = sign_osrel(
        directory.path(),
        &cmdline,
        "IMAGE_ID=demo\nIMAGE_VERSION=1\nBUILD_ID=build\n",
    );
    let verified = verify(&path, trust, ROOT_HASH).unwrap();
    assert_eq!(verified.root_hash, ROOT_HASH);
    assert_eq!(verified.size, std::fs::metadata(&path).unwrap().len());
    let mut component = Component {
        kind: ComponentType::Os,
        id: "demo".into(),
        version: "1+build".into(),
        root_hash: ROOT_HASH.into(),
        image: FileDescriptor {
            path: "os.raw".into(),
            size: 1,
        },
        uki: Some(FileDescriptor {
            path: "os.efi".into(),
            size: 1,
        }),
    };
    verify_component(&path, trust, &component, Architecture::X86_64).unwrap();
    assert!(verify_component(&path, trust, &component, Architecture::Aarch64).is_err());
    component.id = "other".into();
    assert!(verify_component(&path, trust, &component, Architecture::X86_64).is_err());
    component.id = "demo".into();
    component.version = "2+build".into();
    assert!(verify_component(&path, trust, &component, Architecture::X86_64).is_err());
    let osslsigncode = || {
        let fingerprint: String = trusted
            .digest(openssl::hash::MessageDigest::sha256())
            .unwrap()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Command::new("osslsigncode")
            .args(["verify", "-in"])
            .arg(&path)
            .arg("-CAfile")
            .arg(directory.path().join("cert.pem"))
            .arg("-require-leaf-hash")
            .arg(format!("sha256:{fingerprint}"))
            .args(["-ignore-timestamp", "-ignore-crl"])
            .output()
            .unwrap()
    };
    let checked = osslsigncode();
    assert!(
        checked.status.success(),
        "{} {}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(verify(&path, &[], ROOT_HASH).is_err());
    assert!(verify(&path, trust, &"f".repeat(64)).is_err());
    assert!(verify(
        &path,
        &[certificate(tempfile::tempdir().unwrap().path(), 3072)],
        ROOT_HASH
    )
    .is_err());
    let mut bytes = fs::read(&path).unwrap();
    bytes[1024 + 2 * 512] ^= 1;
    fs::write(&path, bytes).unwrap();
    assert!(verify(&path, trust, ROOT_HASH).is_err());
    assert!(!osslsigncode().status.success());
}

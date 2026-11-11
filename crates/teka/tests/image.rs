#![cfg(feature = "tpk")]
#![forbid(unsafe_code)]

mod common;

use base64::{engine::general_purpose::STANDARD, Engine};
use openssl::{
    ec::{EcGroup, EcKey},
    hash::MessageDigest,
    nid::Nid,
    pkcs7::{Pkcs7, Pkcs7Flags},
    pkey::{PKey, Private},
    stack::Stack,
    x509::X509,
};
use std::io::Cursor;
#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use teka::tpk::image::verify_verity_sized;
use teka::tpk::{
    image::{Image, VeritySignature},
    Architecture, Component, ComponentType,
};

const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn ec_cert() -> (X509, PKey<Private>) {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    common::certificate(PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap())
}

fn fingerprint(certificate: &X509) -> String {
    certificate
        .digest(MessageDigest::sha256())
        .unwrap()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn signature(certificate: &X509, key: &PKey<Private>) -> VeritySignature {
    let signature = Pkcs7::sign(
        certificate,
        key,
        &Stack::new().unwrap(),
        HASH.as_bytes(),
        Pkcs7Flags::DETACHED | Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY,
    )
    .unwrap();
    VeritySignature {
        root_hash: HASH.into(),
        certificate_fingerprint: fingerprint(certificate),
        signature: STANDARD.encode(signature.to_der().unwrap()),
    }
}

fn guid(value: &str) -> [u8; 16] {
    let value = value.replace('-', "");
    let mut bytes = [0; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
    }
    bytes[..4].reverse();
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    bytes
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn disk(signature: &VeritySignature, arch: Architecture) -> Vec<u8> {
    let mut bytes = vec![0; 128 * 512];
    bytes[512..520].copy_from_slice(b"EFI PART");
    put_u32(&mut bytes, 520, 0x10000);
    put_u32(&mut bytes, 524, 92);
    for (offset, value) in [(536, 1), (544, 127), (552, 34), (560, 94), (584, 2)] {
        put_u64(&mut bytes, offset, value);
    }
    put_u32(&mut bytes, 592, 128);
    put_u32(&mut bytes, 596, 128);
    let types = match arch {
        Architecture::X86_64 => [
            "4f68bce3-e8cd-4db1-96e7-fbcaf984b709",
            "2c7357ed-ebd2-46d9-aec1-23d437ec2bf5",
            "41092b05-9fc8-4523-994f-2def0408b176",
        ],
        Architecture::Aarch64 => [
            "b921b045-1df0-41c3-af44-4c6f280d3fae",
            "df3300ce-d69f-4c92-978c-9bfb0f38d820",
            "6db69de6-29f4-4758-a7a5-962190f00ce3",
        ],
    };
    for (index, ((start, end), kind)) in [(40, 47), (48, 55), (56, 87)]
        .into_iter()
        .zip(types)
        .enumerate()
    {
        let entry = 1024 + index * 128;
        bytes[entry..entry + 16].copy_from_slice(&guid(kind));
        bytes[entry + 16..entry + 32].fill(index as u8 + 1);
        put_u64(&mut bytes, entry + 32, start);
        put_u64(&mut bytes, entry + 40, end);
    }
    let metadata = serde_json::to_vec(&serde_json::json!({
        "rootHash": signature.root_hash,
        "certificateFingerprint": signature.certificate_fingerprint,
        "signature": signature.signature,
    }))
    .unwrap();
    bytes[56 * 512..56 * 512 + metadata.len()].copy_from_slice(&metadata);
    let table_crc = crc32fast::hash(&bytes[1024..34 * 512]);
    put_u32(&mut bytes, 600, table_crc);
    put_u32(&mut bytes, 528, 0);
    let header_crc = crc32fast::hash(&bytes[512..604]);
    put_u32(&mut bytes, 528, header_crc);
    bytes
}

#[test]
fn reads_gpt_and_verifies_signatures() {
    let (certificate, key) = common::signer();
    let signed = signature(&certificate, &key);
    for arch in [Architecture::X86_64, Architecture::Aarch64] {
        let disk = disk(&signed, arch);
        let image = Image::read(&mut Cursor::new(&disk), 0, disk.len() as u64, arch).unwrap();
        assert_eq!(image.size, disk.len() as u64);
        image
            .verify_signature(std::slice::from_ref(&certificate), ComponentType::App)
            .unwrap();
    }
    let (wrong, _) = common::signer();
    let mut image = Image::read(
        &mut Cursor::new(&disk(&signed, Architecture::X86_64)),
        0,
        disk(&signed, Architecture::X86_64).len() as u64,
        Architecture::X86_64,
    )
    .unwrap();
    assert!(image
        .verify_signature(&[wrong], ComponentType::App)
        .is_err());
    let (app_certificate, app_key) = ec_cert();
    let app_image = Image::read(
        &mut Cursor::new(&disk(
            &signature(&app_certificate, &app_key),
            Architecture::X86_64,
        )),
        0,
        disk(&signature(&app_certificate, &app_key), Architecture::X86_64).len() as u64,
        Architecture::X86_64,
    )
    .unwrap();
    for kind in [ComponentType::App, ComponentType::Os] {
        assert!(app_image
            .verify_signature(std::slice::from_ref(&app_certificate), kind)
            .is_err());
    }
    let (os_certificate, os_key) = common::signer();
    let os_image = Image::read(
        &mut Cursor::new(&disk(
            &signature(&os_certificate, &os_key),
            Architecture::X86_64,
        )),
        0,
        disk(&signature(&os_certificate, &os_key), Architecture::X86_64).len() as u64,
        Architecture::X86_64,
    )
    .unwrap();
    os_image
        .verify_signature(std::slice::from_ref(&os_certificate), ComponentType::Os)
        .unwrap();
    let component: Component = serde_json::from_value(serde_json::json!({
        "type":"app", "version":"1", "id":"demo", "root_hash":HASH,
        "image":{"path":"demo.raw", "size":1},
    }))
    .unwrap();
    image.verify_binding(&component).unwrap();
    image.signature.root_hash = "b".repeat(64);
    assert!(image.verify_binding(&component).is_err());
}

#[test]
#[cfg(target_os = "linux")]
fn verifies_veritysetup_hash_tree() {
    let dir = tempfile::tempdir().unwrap();
    for blocks in [1, 2, 127, 128, 129, 16384, 16385] {
        let data = (0..blocks)
            .flat_map(|i| [i as u8; 4096])
            .collect::<Vec<_>>();
        std::fs::write(dir.path().join("data"), &data).unwrap();
        let _ = std::fs::remove_file(dir.path().join("hash"));
        let output = std::process::Command::new("veritysetup")
            .current_dir(dir.path())
            .args(["format", "data", "hash"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        let root = stdout
            .lines()
            .find_map(|line| line.strip_prefix("Root hash:"))
            .unwrap()
            .trim();
        let hash = std::fs::read(dir.path().join("hash")).unwrap();
        verify_verity_sized(&hash, data.as_slice(), None, root).unwrap();
        assert!(verify_verity_sized(&hash, data.as_slice(), None, HASH).is_err());
        assert!(verify_verity_sized(&hash, data.as_slice().chain(&[0][..]), None, root).is_err());
        let mut bad_data = data.clone();
        bad_data[data.len() - 1] ^= 1;
        assert!(verify_verity_sized(&hash, bad_data.as_slice(), None, root).is_err());
        if blocks > 1 {
            let mut bad_tree = hash.clone();
            bad_tree[4096] ^= 1;
            assert!(verify_verity_sized(&bad_tree, data.as_slice(), None, root).is_err());
        }
        assert!(
            verify_verity_sized(&hash, data.as_slice(), Some(data.len() as u64 + 4096), root)
                .is_err()
        );
        let mut bad_geometry = hash.clone();
        put_u64(&mut bad_geometry, 72, u64::MAX);
        assert!(verify_verity_sized(&bad_geometry, std::io::empty(), None, root).is_err());
        put_u64(&mut bad_geometry, 72, 1_000_000);
        assert!(verify_verity_sized(&bad_geometry, std::io::empty(), None, root).is_err());
    }
}

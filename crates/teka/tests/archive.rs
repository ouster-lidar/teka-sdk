#![forbid(unsafe_code)]
mod common;
#[path = "common/archive.rs"]
mod fixture;

use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, io::Cursor};
use teka_tpk::{pack, read, read_trusted, verify, MAX_MANIFEST, MAX_SIGNATURE};

const PAYLOAD: &[u8] = b"app image bytes";

fn envelope(signature: &[u8], encoded: &[u8]) -> Vec<u8> {
    [
        teka_tpk::MAGIC.as_slice(),
        &(signature.len() as u32).to_le_bytes(),
        &0u32.to_le_bytes(),
        signature,
        encoded,
    ]
    .concat()
}

#[test]
fn inspect_manifest_stops_before_bao() {
    let manifest = b"not even valid JSON";
    let (cert, key) = common::signer();
    let signature = common::sign_manifest(manifest, &cert, &key);
    let bytes = envelope(&signature, b"unread Bao payload");
    let mut source = Cursor::new(&bytes);
    assert_eq!(teka_tpk::inspect_manifest(&mut source).unwrap(), manifest);
    let payload_offset = 16 + signature.len();
    assert_eq!(source.position(), payload_offset as u64);
    assert_eq!(&bytes[source.position() as usize..], b"unread Bao payload");
    for cut in [0, 15, payload_offset - 1] {
        assert!(teka_tpk::inspect_manifest(&mut Cursor::new(&bytes[..cut])).is_err());
    }
    for (offset, size) in [(8, 0), (8, MAX_SIGNATURE as u32 + 1), (12, 1)] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 4].copy_from_slice(&size.to_le_bytes());
        assert!(teka_tpk::inspect_manifest(&mut Cursor::new(invalid)).is_err());
    }
}

#[test]
fn package_round_trip_and_rejects_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("input.raw");
    fs::write(&raw, PAYLOAD).unwrap();
    let manifest: teka_tpk::Manifest =
        serde_json::from_value(common::manifest(PAYLOAD.len())).unwrap();
    let mut bytes = Vec::new();
    let (certificate, key) = common::signer();
    pack(
        &mut bytes,
        &manifest,
        &BTreeMap::from([("demo.raw".into(), raw)]),
        &key,
        &certificate,
    )
    .unwrap();
    assert_eq!(&bytes[..8], b"TPK 1\n\0\0");
    let mut reader = Cursor::new(&bytes);
    let package = read(&mut reader).unwrap();
    verify(&mut reader, &package).unwrap();
    let mut output = Vec::new();
    package.members[0]
        .copy_range(&mut reader, 0, None, &mut output)
        .unwrap();
    assert_eq!(output, PAYLOAD);
    let demo = [("demo.raw", PAYLOAD)];
    let mut invalid = common::manifest(PAYLOAD.len());
    invalid["format"] = json!(2);
    assert!(read(&mut Cursor::new(fixture::archive(
        &invalid,
        &demo,
        &certificate,
        &key
    )))
    .is_err());
    let mut big = common::manifest(PAYLOAD.len());
    big["components"][0]["version"] = json!("x".repeat(MAX_MANIFEST as usize));
    assert!(read(&mut Cursor::new(fixture::archive(
        &big,
        &demo,
        &certificate,
        &key
    )))
    .is_err());
    for cut in [0, 7, 15, bytes.len() - 1] {
        assert!(read(&mut Cursor::new(&bytes[..cut])).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(read(&mut Cursor::new(trailing)).is_err());
}

#[test]
fn requires_exact_bounded_signature_and_unambiguous_manifest() {
    let (certificate, key) = common::signer();
    let valid = fixture::archive(
        &common::manifest(PAYLOAD.len()),
        &[("demo.raw", PAYLOAD)],
        &certificate,
        &key,
    );
    let package = read(&mut Cursor::new(&valid)).unwrap();
    let offset = 16 + package.signature.len();
    let encoded = &valid[offset..];
    let json = serde_json::to_vec_pretty(&package.manifest).unwrap();
    let signature = common::sign_manifest(&json, &certificate, &key);
    let pretty = envelope(&signature, encoded);
    let parsed = read(&mut Cursor::new(&pretty)).unwrap();
    assert_eq!(parsed.manifest_bytes, json);
    assert_eq!(parsed.signature, signature);
    for signature in [
        vec![],
        vec![0; MAX_SIGNATURE as usize + 1],
        signature[..signature.len() - 1].to_vec(),
        [signature.clone(), vec![0]].concat(),
    ] {
        assert!(read(&mut Cursor::new(envelope(&signature, encoded))).is_err());
    }
    for (field, value) in [(8, 0), (8, MAX_SIGNATURE as u32 + 1), (12, 1)] {
        let mut invalid = valid.clone();
        invalid[field..field + 4].copy_from_slice(&value.to_le_bytes());
        assert!(read(&mut Cursor::new(invalid)).is_err());
    }
    let original = String::from_utf8(package.manifest_bytes.clone()).unwrap();
    for invalid in [
        original.replacen('{', "{\"format\":1,", 1),
        original.replacen("\"id\":\"demo\"", "\"id\":\"demo\",\"id\":\"demo\"", 1),
        original.replacen("\"size\":", "\"size\":1,\"size\":", 1),
        original.replacen('{', "{\"unknown\":true,", 1),
        original.replacen("\"path\":", "\"unknown\":true,\"path\":", 1),
    ] {
        assert!(
            teka_tpk::Manifest::parse(invalid.as_bytes()).is_err(),
            "accepted {invalid}"
        );
    }
    let certs = teka_tpk::verify::Certificates {
        app: vec![certificate],
        os: vec![],
    };
    let mut changed = package.clone();
    changed.manifest_bytes.push(b' ');
    assert!(
        teka_tpk::verify::signatures(&mut Cursor::new(&valid), &changed, Some(&certs))
            .unwrap_err()
            .to_string()
            .contains("verify package manifest signature")
    );
    let mut changed = package.clone();
    changed.manifest.components[0].root_hash = "b".repeat(64);
    assert!(
        teka_tpk::verify::signatures(&mut Cursor::new(&valid), &changed, Some(&certs))
            .unwrap_err()
            .to_string()
            .contains("differs from signed bytes")
    );
}

#[test]
fn authenticates_before_parsing_json_and_enforces_signed_geometry() {
    let (certificate, key) = common::signer();
    let trust = teka_tpk::verify::Certificates {
        app: vec![certificate.clone()],
        os: vec![],
    };
    let bytes = fixture::archive(
        &common::manifest(PAYLOAD.len()),
        &[("demo.raw", PAYLOAD)],
        &certificate,
        &key,
    );
    let package = read_trusted(&mut Cursor::new(&bytes), &trust).unwrap();
    let offset = 16 + package.signature.len();
    let mut bad_signature = common::sign_manifest(b"{", &certificate, &key);
    *bad_signature.last_mut().unwrap() ^= 1;
    let malformed = envelope(&bad_signature, &bytes[offset..]);
    let error = read_trusted(&mut Cursor::new(malformed), &trust).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("authenticate envelope before JSON"),
        "{error:#}"
    );
    let (wrong, _) = common::signer();
    let wrong_trust = teka_tpk::verify::Certificates {
        app: vec![wrong],
        os: vec![],
    };
    assert!(read_trusted(&mut Cursor::new(&bytes), &wrong_trust)
        .unwrap_err()
        .to_string()
        .contains("authenticate envelope before JSON"));
    for (field, value) in [
        ("size", json!(0)),
        ("size", json!(u64::MAX)),
        ("size", json!(PAYLOAD.len() + 1)),
        ("size", Value::Null),
        ("stored_size", json!(1)),
        ("encoding", json!("raw")),
        ("encoding", json!("zstd")),
        ("encoding", json!("gzip")),
        ("encoding", Value::Null),
        ("offset", json!(0)),
    ] {
        let mut manifest = serde_json::to_value(&package.manifest).unwrap();
        manifest["components"][0]["image"][field] = value;
        let json = serde_json::to_vec(&manifest).unwrap();
        let signature = common::sign_manifest(&json, &certificate, &key);
        assert!(
            read_trusted(
                &mut Cursor::new(envelope(&signature, &bytes[offset..])),
                &trust
            )
            .is_err(),
            "accepted invalid {field}"
        );
    }
    for (field, value) in [
        ("bao_root", json!("x".repeat(64))),
        ("bao_root", Value::Null),
        ("archive", json!({})),
        ("block_size", json!(1024)),
        ("version", json!("1")),
    ] {
        let mut manifest = serde_json::to_value(&package.manifest).unwrap();
        manifest[field] = value;
        assert!(teka_tpk::Manifest::parse(&serde_json::to_vec(&manifest).unwrap()).is_err());
    }
    for version in [None, Some(json!("")), Some(json!("  ")), Some(Value::Null)] {
        let mut invalid = serde_json::to_value(&package.manifest).unwrap();
        let component = invalid["components"][0].as_object_mut().unwrap();
        component.remove("version");
        if let Some(version) = version {
            component.insert("version".into(), version);
        }
        assert!(teka_tpk::Manifest::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    let mut duplicate = serde_json::to_value(&package.manifest).unwrap();
    let component = duplicate["components"][0].clone();
    duplicate["components"]
        .as_array_mut()
        .unwrap()
        .push(component);
    assert!(teka_tpk::Manifest::parse(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    let mut excessive = serde_json::to_value(&package.manifest).unwrap();
    excessive["components"] = json!((0..9)
        .map(|index| json!({
            "type":"app", "version":"1", "id":format!("app{index}"), "root_hash":"a".repeat(64),
            "image":{"path":format!("app{index}.raw"), "size":teka_tpk::MAX_MEMBER}
        }))
        .collect::<Vec<_>>());
    assert!(teka_tpk::Manifest::parse(&serde_json::to_vec(&excessive).unwrap()).is_err());
}

#[test]
fn os_manifests_require_os_role_trust() {
    let mut manifest = common::manifest(PAYLOAD.len());
    manifest["components"].as_array_mut().unwrap().push(json!({
        "type":"os","id":"teka","version":"1.0","root_hash":"b".repeat(64),
        "image":{"path":"test-machine.raw","size":PAYLOAD.len()},
        "uki":{"path":"test-machine.efi","size":PAYLOAD.len()}}));
    let (app_certificate, app_key) = common::signer();
    let (os_certificate, _) = common::signer();
    let archive = fixture::archive(
        &manifest,
        &[
            ("demo.raw", PAYLOAD),
            ("test-machine.raw", PAYLOAD),
            ("test-machine.efi", PAYLOAD),
        ],
        &app_certificate,
        &app_key,
    );
    let trust = teka_tpk::verify::Certificates {
        app: vec![app_certificate],
        os: vec![os_certificate],
    };
    let error = read_trusted(&mut Cursor::new(&archive), &trust).unwrap_err();
    assert!(
        error.to_string().contains("enforce package signing role"),
        "{error:#}"
    );
    let package = read(&mut Cursor::new(&archive)).unwrap();
    assert_eq!(package.manifest.signing_role(), teka_tpk::ComponentType::Os);
    let mut missing_uki = serde_json::to_value(&package.manifest).unwrap();
    missing_uki["components"][1]
        .as_object_mut()
        .unwrap()
        .remove("uki");
    assert!(teka_tpk::Manifest::parse(&serde_json::to_vec(&missing_uki).unwrap()).is_err());
    assert!(
        teka_tpk::verify::signatures(&mut Cursor::new(&archive), &package, Some(&trust))
            .unwrap_err()
            .to_string()
            .contains("verify package manifest signature")
    );
}

#[test]
fn concatenated_payload_has_no_headers_padding_or_termination() {
    use std::io::Write;
    let first = b"first image bytes";
    let second = vec![0xab; 1234];
    let compress = |data: &[u8]| {
        let mut encoder = zstd::Encoder::new(Vec::new(), 3).unwrap();
        encoder.include_checksum(false).unwrap();
        encoder.include_contentsize(true).unwrap();
        encoder.window_log(23).unwrap();
        encoder
            .set_pledged_src_size(Some(data.len() as u64))
            .unwrap();
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    };
    let first_compressed = compress(first);
    let compressed = compress(&second);
    assert_eq!(first_compressed[4] & 0x04, 0);
    assert_eq!(compressed[4] & 0x04, 0);
    let manifest = json!({"format":1,"arch":"x86_64",
    "components":[
        {"type":"app","id":"first","version":"1","root_hash":"a".repeat(64),
            "image":{"path":"first.raw","size":first.len()}},
        {"type":"app","id":"second","version":"2","root_hash":"b".repeat(64),
            "image":{"path":"second.raw","size":second.len()}}
    ]});
    let (certificate, key) = common::signer();
    let directory = tempfile::tempdir().unwrap();
    let files = [
        ("first.raw", first.as_slice()),
        ("second.raw", second.as_slice()),
    ]
    .into_iter()
    .map(|(name, data)| {
        let path = directory.path().join(name);
        fs::write(&path, data).unwrap();
        (name.into(), path)
    })
    .collect();
    let mut bytes = Vec::new();
    teka_tpk::pack_manifest(
        &mut bytes,
        &serde_json::to_vec(&manifest).unwrap(),
        &files,
        3,
        &key,
        &certificate,
    )
    .unwrap();
    let mut source = Cursor::new(&bytes);
    let package = read(&mut source).unwrap();
    verify(&mut source, &package).unwrap();
    for (member, expected) in package.members.iter().zip([first.as_slice(), &second]) {
        let mut decoded = Vec::new();
        member.check(&mut source, &mut decoded).unwrap();
        assert_eq!(decoded, expected);
    }
    assert_eq!(package.manifest.components[0].version, "1");
    assert_eq!(package.manifest.components[1].version, "2");
    let expected = [first_compressed.as_slice(), compressed.as_slice()].concat();
    assert!(expected.len() < teka_tpk::transport::BLOCK_SIZE as usize);
    let encoded_offset = 16 + package.signature.len();
    assert_eq!(&bytes[encoded_offset..], expected);
    assert_eq!(package.archive_size(), expected.len() as u64);
    assert_eq!(package.members[0].offset, 0);
    assert_eq!(package.members[1].offset, first_compressed.len() as u64);
    let signed = serde_json::to_value(&package.manifest).unwrap();
    assert!(signed.get("archive").is_none());
    assert!(signed["components"][0]["image"].get("encoding").is_none());
    assert_eq!(signed["components"][1]["image"]["size"], compressed.len());
    for offset in [
        0,
        first_compressed.len() - 1,
        first_compressed.len(),
        expected.len() - 1,
    ] {
        let mut corrupt = bytes.clone();
        corrupt[encoded_offset + offset] ^= 1;
        let mut source = Cursor::new(corrupt);
        let package = read(&mut source).unwrap();
        let error = verify(&mut source, &package).unwrap_err();
        assert!(
            error
                .chain()
                .any(|cause| cause.to_string().contains("LeafHashMismatch")),
            "{error:#}"
        );
    }
    let mut root = signed.clone();
    root["bao_root"] = json!("0".repeat(64));
    let json = serde_json::to_vec(&root).unwrap();
    let signature = common::sign_manifest(&json, &certificate, &key);
    let mut source = Cursor::new(envelope(&signature, &bytes[encoded_offset..]));
    let package = read(&mut source).unwrap();
    assert!(verify(&mut source, &package).is_err());
    for size in [compressed.len() - 1, compressed.len() + 1] {
        let mut wrong_length = signed.clone();
        wrong_length["components"][1]["image"]["size"] = json!(size);
        let json = serde_json::to_vec(&wrong_length).unwrap();
        let signature = common::sign_manifest(&json, &certificate, &key);
        assert!(read(&mut Cursor::new(envelope(
            &signature,
            &bytes[encoded_offset..]
        )))
        .is_err());
    }
}

#[test]
fn mixed_bundle_offsets_cross_groups_without_gaps() {
    use std::io::{Read, Write};
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut noise = |size| {
        (0..size)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect::<Vec<_>>()
    };
    let raw = noise(65530);
    let app = noise(140_003);
    let decoded = vec![0xab; 1234];
    let compress = |data: &[u8]| {
        let mut encoder = zstd::Encoder::new(Vec::new(), 3).unwrap();
        encoder.include_checksum(false).unwrap();
        encoder
            .set_pledged_src_size(Some(data.len() as u64))
            .unwrap();
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    };
    let compressed = compress(&decoded);
    let stored_raw = compress(&raw);
    let stored_app = compress(&app);
    let manifest = json!({"format":1,"arch":"x86_64",
    "components":[
        {"type":"os","id":"teka","version":"1","root_hash":"a".repeat(64),
            "image":{"path":"os.raw","size":raw.len()},
            "uki":{"path":"os.efi","size":decoded.len()}},
        {"type":"app","id":"first","version":"1","root_hash":"b".repeat(64),
            "image":{"path":"first.raw","size":app.len()}},
        {"type":"app","id":"second","version":"1","root_hash":"c".repeat(64),
            "image":{"path":"second.raw","size":decoded.len()}},
        {"type":"app","id":"third","version":"1","root_hash":"d".repeat(64),
            "image":{"path":"third.raw","size":raw.len()}}
    ]});
    let members = [
        ("os.raw.zst", stored_raw.as_slice()),
        ("os.efi.zst", compressed.as_slice()),
        ("first.raw.zst", stored_app.as_slice()),
        ("second.raw.zst", compressed.as_slice()),
        ("third.raw.zst", stored_raw.as_slice()),
    ];
    let (certificate, key) = common::signer();
    let bytes = fixture::archive(&manifest, &members, &certificate, &key);
    let mut source = Cursor::new(&bytes);
    let package = read(&mut source).unwrap();
    verify(&mut source, &package).unwrap();
    let expected = members
        .iter()
        .flat_map(|(_, data)| data.iter().copied())
        .collect::<Vec<_>>();
    let mut next = 0;
    for (member, (_, stored)) in package.members.iter().zip(members) {
        assert_eq!(member.offset, next);
        assert_eq!(member.size, stored.len() as u64);
        next += member.size;
    }
    assert_eq!(next, expected.len() as u64);
    assert_eq!(package.archive_size(), next);
    assert!(!package
        .archive_size()
        .div_ceil(teka_tpk::transport::BLOCK_SIZE)
        .is_power_of_two());
    let encoded_offset = 16 + package.signature.len();
    let mut transport = teka_tpk::transport::StreamReader::new(
        &bytes[encoded_offset..],
        package.root().unwrap(),
        next,
    )
    .unwrap();
    let mut stored = Vec::new();
    transport.read_to_end(&mut stored).unwrap();
    transport.finish().unwrap();
    assert_eq!(stored, expected);
    let mut output = Vec::new();
    package.members[4]
        .copy_range(&mut source, 23, Some(101), &mut output)
        .unwrap();
    assert_eq!(output, raw[23..124]);
}

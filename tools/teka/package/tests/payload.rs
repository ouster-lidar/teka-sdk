#![forbid(unsafe_code)]
mod common;
#[path = "common/archive.rs"]
mod fixture;
use fixture::archive;

use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
};

struct Unreadable;

impl Read for Unreadable {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("payload access before manifest authentication")
    }
}

impl Seek for Unreadable {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        panic!("payload access before manifest authentication")
    }
}

#[test]
fn authenticate_before_any_payload_access() {
    let value = common::manifest(b"payload".len());
    let (certificate, key) = common::signer();
    let bytes = archive(&value, &[("demo.raw", b"payload")], &certificate, &key);
    let package = teka_pkg::read(&mut Cursor::new(&bytes)).unwrap();
    let (wrong, _) = common::signer();
    let wrong_trust = teka_pkg::verify::Certificates {
        app: vec![wrong],
        os: vec![],
    };
    let error =
        teka_pkg::verify::signatures(&mut Unreadable, &package, Some(&wrong_trust)).unwrap_err();
    assert!(error
        .to_string()
        .contains("verify package manifest signature"));
}

struct ChangeSourceOnWrite<'a> {
    source: &'a std::path::Path,
    changed: bool,
    bytes: Vec<u8>,
}

impl Write for ChangeSourceOnWrite<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.changed {
            fs::write(
                self.source,
                vec![0; fs::metadata(self.source)?.len() as usize],
            )?;
            self.changed = true;
        }
        self.bytes.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn compressed_payload_is_a_snapshot() {
    let data = (0..16384u64)
        .scan(0x9e37_79b9_7f4a_7c15u64, |x, _| {
            *x ^= *x << 13;
            *x ^= *x >> 7;
            *x ^= *x << 17;
            Some(*x as u8)
        })
        .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("demo.raw");
    fs::write(&source, &data).unwrap();
    let mut draft = common::manifest(data.len());
    draft["bao_root"] = json!("0".repeat(64));
    draft["components"][0]["image"]["size"] = json!(1);
    let (certificate, key) = common::signer();
    let mut output = ChangeSourceOnWrite {
        source: &source,
        changed: false,
        bytes: Vec::new(),
    };
    teka_pkg::pack_manifest(
        &mut output,
        &serde_json::to_vec(&draft).unwrap(),
        &BTreeMap::from([("demo.raw".into(), source.clone())]),
        9,
        &key,
        &certificate,
    )
    .unwrap();
    assert!(output.changed);
    assert_ne!(fs::read(&source).unwrap(), data);
    let mut reader = Cursor::new(&output.bytes);
    let package = teka_pkg::read(&mut reader).unwrap();
    assert!(package.members[0].size > data.len() as u64);
    assert_ne!(package.manifest.bao_root, Some("0".repeat(64)));
    teka_pkg::verify(&mut reader, &package).unwrap();
    let mut restored = Vec::new();
    package.members[0]
        .copy_range(&mut reader, 0, None, &mut restored)
        .unwrap();
    assert_eq!(restored, data);
}

#[test]
fn finalized_manifest_must_still_fit_the_metadata_limit() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("demo.raw");
    fs::write(&source, b"payload").unwrap();
    let mut draft = common::manifest(b"payload".len());
    draft["components"][0]["version"] = json!("");
    let base_size = serde_json::to_vec(&draft).unwrap().len();
    draft["components"][0]["version"] =
        json!("x".repeat(teka_pkg::MAX_MANIFEST as usize - base_size - 1));
    let bytes = serde_json::to_vec(&draft).unwrap();
    assert_eq!(bytes.len(), teka_pkg::MAX_MANIFEST as usize - 1);
    let (certificate, key) = common::signer();
    let mut output = Vec::new();
    let error = teka_pkg::pack_manifest(
        &mut output,
        &bytes,
        &BTreeMap::from([("demo.raw".into(), source)]),
        9,
        &key,
        &certificate,
    )
    .unwrap_err();
    assert!(error.to_string().contains("manifest exceeds limit"));
    assert!(output.is_empty());
}

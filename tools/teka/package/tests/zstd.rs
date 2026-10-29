mod common;
#[path = "common/archive.rs"]
mod fixture;

use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
};

#[test]
fn zstd_transport() {
    fn fixture(value: &Value, compressed: &[u8]) -> Vec<u8> {
        let (certificate, key) = common::signer();
        fixture::archive(value, &[("demo.raw.zst", compressed)], &certificate, &key)
    }

    fn encode(data: &[u8], checksum: bool) -> Vec<u8> {
        let mut encoder = zstd::Encoder::new(Vec::new(), 3).unwrap();
        encoder.include_checksum(checksum).unwrap();
        encoder
            .set_pledged_src_size(Some(data.len() as u64))
            .unwrap();
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }
    fn verify(bytes: &[u8]) -> anyhow::Result<()> {
        let mut input = Cursor::new(bytes);
        let p = teka_tpk::read(&mut input)?;
        teka_tpk::verify(&mut input, &p)
    }
    let data = (0..262144).map(|i| (i % 251) as u8).collect::<Vec<_>>();
    let zst = encode(&data, false);
    let mut input = Cursor::new(fixture(&common::manifest(data.len()), &zst));
    let p = teka_tpk::read(&mut input).unwrap();
    teka_tpk::verify(&mut input, &p).unwrap();
    assert_eq!(p.members[0].size, zst.len() as u64);
    assert_eq!(
        p.members[0].open(&mut input).unwrap().size(),
        data.len() as u64
    );
    let mut out = Cursor::new(vec![0xff; data.len()]);
    p.members[0]
        .copy_range(&mut input, 0, None, &mut out)
        .unwrap();
    assert_eq!(out.into_inner(), data);
    let data = vec![0xab; 131072];
    let ok = encode(&data, false);
    assert_eq!(ok[4] & 0x04, 0);
    verify(&fixture(
        &common::manifest(data.len()),
        &encode(&data, true),
    ))
    .unwrap();
    let mut encoder = zstd::Encoder::new(Vec::new(), 3).unwrap();
    encoder.window_log(10).unwrap();
    encoder
        .set_pledged_src_size(Some(data.len() as u64))
        .unwrap();
    encoder.write_all(&data).unwrap();
    let mut oversized_window = encoder.finish().unwrap();
    assert_eq!(oversized_window[4] & 0x20, 0);
    oversized_window[5] = 0x70;
    let mut oversized_content = vec![0x28, 0xb5, 0x2f, 0xfd, 0xe0];
    oversized_content.extend((teka_tpk::MAX_MEMBER + 1).to_le_bytes());
    for invalid_header in [
        data.clone(),
        oversized_window,
        oversized_content,
        encode(&[], false),
        zstd::encode_all(data.as_slice(), 3).unwrap(),
        vec![0x50, 0x2a, 0x4d, 0x18, 0, 0, 0, 0],
    ] {
        let bytes = fixture(&common::manifest(data.len()), &invalid_header);
        let mut input = Cursor::new(&bytes);
        let package = teka_tpk::read(&mut input).unwrap();
        let mut output = Vec::new();
        assert!(package.members[0]
            .copy_range(&mut input, 0, None, &mut output)
            .is_err());
        assert!(output.is_empty());
        assert!(verify(&bytes).is_err());
    }
    assert_eq!(ok[4] >> 6, 2);
    let size_offset = if ok[4] & 0x20 != 0 { 5 } else { 6 };
    let mut too_short = ok.clone();
    too_short[size_offset..size_offset + 4]
        .copy_from_slice(&((data.len() - 1) as u32).to_le_bytes());
    let mut too_long = ok.clone();
    too_long[size_offset..size_offset + 4]
        .copy_from_slice(&((data.len() + 1) as u32).to_le_bytes());
    for zst in [
        too_short,
        too_long,
        ok[..ok.len() - 1].to_vec(),
        [ok.clone(), ok.clone()].concat(),
        [ok.clone(), vec![0]].concat(),
    ] {
        assert!(verify(&fixture(&common::manifest(data.len()), &zst)).is_err());
    }
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("demo.raw");
    let (certificate, key) = common::signer();
    let pack = |data: &[u8]| {
        fs::write(&raw, data).unwrap();
        let mut enc = Vec::new();
        let mut draft = common::manifest(data.len());
        draft["components"][0]["image"]
            .as_object_mut()
            .unwrap()
            .remove("size");
        teka_tpk::pack_manifest(
            &mut enc,
            &serde_json::to_vec(&draft).unwrap(),
            &BTreeMap::from([("demo.raw".into(), raw.clone())]),
            teka_tpk::DEFAULT_ZSTD_LEVEL,
            &key,
            &certificate,
        )
        .unwrap();
        verify(&enc).unwrap();
        enc
    };
    let noise = (0..16384u64)
        .scan(0x9e37_79b9_7f4a_7c15u64, |x, _| {
            *x ^= *x << 13;
            *x ^= *x >> 7;
            *x ^= *x << 17;
            Some(*x as u8)
        })
        .collect::<Vec<_>>();
    let enc = pack(&noise);
    let mut input = Cursor::new(&enc);
    let package = teka_tpk::read(&mut input).unwrap();
    assert!(package.members[0].size > noise.len() as u64);
    let mut output = Vec::new();
    package.members[0]
        .copy_range(&mut input, 0, None, &mut output)
        .unwrap();
    assert_eq!(output, noise);
    let enc = pack(&data);
    let mut input = Cursor::new(&enc);
    let package = teka_tpk::read(&mut input).unwrap();
    let mut output = Vec::new();
    package.members[0]
        .copy_range(&mut input, 4090, Some(33), &mut output)
        .unwrap();
    assert_eq!(output, data[4090..4123]);
    output.clear();
    package.members[0]
        .copy_range(&mut input, data.len() as u64, None, &mut output)
        .unwrap();
    assert!(output.is_empty());
    assert!(package.members[0]
        .copy_range(&mut input, data.len() as u64 + 1, None, &mut output)
        .is_err());
    assert!(package.members[0]
        .copy_range(&mut input, 1, Some(u64::MAX), &mut output)
        .is_err());
    assert!(output.is_empty());
}

#[test]
fn authenticated_zstd_framing_error_remains_terminal() {
    let data = vec![0xab; 65536];
    let mut encoder = zstd::Encoder::new(Vec::new(), 3).unwrap();
    encoder
        .set_pledged_src_size(Some(data.len() as u64))
        .unwrap();
    encoder.write_all(&data).unwrap();
    let mut stored = encoder.finish().unwrap();
    stored.push(0);
    let manifest = json!({"format":1,"arch":"x86_64",
        "components":[{"type":"app","id":"demo","version":"1","root_hash":"a".repeat(64),
        "image":{"path":"demo.raw","size":data.len()}}]});
    let (certificate, key) = common::signer();
    let bytes = fixture::archive(&manifest, &[("demo.raw.zst", &stored)], &certificate, &key);
    let mut source = Cursor::new(bytes);
    let package = teka_tpk::read(&mut source).unwrap();
    let mut reader = package.members[0].open(&mut source).unwrap();
    let mut output = Vec::new();
    let error = reader.read_to_end(&mut output).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("trailing bytes after zstd frame"),
        "{error}"
    );
    assert_eq!(output, data);
    assert!(reader.read(&mut [0]).is_err());
    assert!(reader.read(&mut [0]).is_err());
}

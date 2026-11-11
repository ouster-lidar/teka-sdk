use openssl::{
    pkey::{PKey, Private},
    x509::X509,
};
use serde_json::{json, Value};
use std::io::Write;
use teka::tpk::transport::Encoder;

pub fn archive(
    manifest: &Value,
    members: &[(&str, &[u8])],
    certificate: &X509,
    key: &PKey<Private>,
) -> Vec<u8> {
    let mut payload = Vec::new();
    let mut manifest = manifest.clone();
    let mut members = members.iter();
    for component in manifest["components"].as_array_mut().unwrap() {
        for field in ["image", "uki"] {
            if let Some(file) = component.get_mut(field) {
                let &(name, data) = members.next().unwrap();
                let path = name.strip_suffix(".zst").unwrap_or(name);
                assert_eq!(file["path"], path);
                let compressed;
                let stored = if name.ends_with(".zst") {
                    data
                } else {
                    let mut encoder = zstd::Encoder::new(Vec::new(), 3).unwrap();
                    encoder
                        .set_pledged_src_size(Some(data.len() as u64))
                        .unwrap();
                    encoder.write_all(data).unwrap();
                    compressed = encoder.finish().unwrap();
                    &compressed
                };
                file["size"] = json!(stored.len());
                payload.extend_from_slice(stored);
            }
        }
    }
    assert!(members.next().is_none());
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&payload).unwrap();
    let encoder = Encoder::new(&file).unwrap();
    manifest["bao_root"] = json!(encoder.root());
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let signature = crate::common::sign_manifest(&bytes, certificate, key);
    let mut output = teka::tpk::MAGIC.to_vec();
    output.extend((signature.len() as u32).to_le_bytes());
    output.extend(0u32.to_le_bytes());
    output.extend(signature);
    encoder.write(&file, &mut output).unwrap();
    output
}

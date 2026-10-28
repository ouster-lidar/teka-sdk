use openssl::{
    asn1::{Asn1Integer, Asn1Time},
    bn::BigNum,
    hash::MessageDigest,
    pkcs7::{Pkcs7, Pkcs7Flags},
    pkey::{PKey, Private},
    rsa::Rsa,
    stack::Stack,
    x509::{X509NameBuilder, X509},
};

pub fn signer() -> (X509, PKey<Private>) {
    let key = PKey::from_rsa(Rsa::generate(3072).unwrap()).unwrap();
    certificate(key)
}

pub fn certificate(key: PKey<Private>) -> (X509, PKey<Private>) {
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "publisher").unwrap();
    let name = name.build();
    let mut cert = X509::builder().unwrap();
    cert.set_version(2).unwrap();
    cert.set_serial_number(&Asn1Integer::from_bn(&BigNum::from_u32(1).unwrap()).unwrap())
        .unwrap();
    cert.set_subject_name(&name).unwrap();
    cert.set_issuer_name(&name).unwrap();
    cert.set_pubkey(&key).unwrap();
    cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    cert.set_not_after(&Asn1Time::days_from_now(1).unwrap())
        .unwrap();
    cert.sign(&key, MessageDigest::sha256()).unwrap();
    (cert.build(), key)
}

#[allow(dead_code)]
pub fn manifest(size: usize) -> serde_json::Value {
    serde_json::json!({"format":1,"arch":"x86_64",
        "components":[{"type":"app","id":"demo","version":"1","root_hash":"a".repeat(64),
        "image":{"path":"demo.raw","size":size}}]})
}

#[allow(dead_code)]
pub fn sign(bytes: &[u8], certificate: &X509, key: &PKey<Private>) -> Vec<u8> {
    Pkcs7::sign(
        certificate,
        key,
        &Stack::new().unwrap(),
        bytes,
        Pkcs7Flags::DETACHED | Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY,
    )
    .unwrap()
    .to_der()
    .unwrap()
}

#[allow(dead_code)]
pub fn sign_manifest(bytes: &[u8], certificate: &X509, key: &PKey<Private>) -> Vec<u8> {
    Pkcs7::sign(
        certificate,
        key,
        &Stack::new().unwrap(),
        bytes,
        Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY,
    )
    .unwrap()
    .to_der()
    .unwrap()
}

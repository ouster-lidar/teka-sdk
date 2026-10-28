use openssl::{
    asn1::Asn1Time,
    hash::MessageDigest,
    pkey::{PKey, Private},
    rsa::Rsa,
    x509::{X509NameBuilder, X509},
};
use std::sync::OnceLock;

pub fn signing(os: bool) -> (&'static PKey<Private>, &'static X509) {
    static APP: OnceLock<(PKey<Private>, X509)> = OnceLock::new();
    static OS: OnceLock<(PKey<Private>, X509)> = OnceLock::new();
    let (key, certificate) = if os { &OS } else { &APP }.get_or_init(|| {
        let key = PKey::from_rsa(Rsa::generate(3072).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "CLI fixture").unwrap();
        let name = name.build();
        let mut certificate = X509::builder().unwrap();
        certificate.set_version(2).unwrap();
        certificate.set_subject_name(&name).unwrap();
        certificate.set_issuer_name(&name).unwrap();
        certificate.set_pubkey(&key).unwrap();
        certificate
            .set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        certificate
            .set_not_after(&Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        certificate.sign(&key, MessageDigest::sha256()).unwrap();
        (key, certificate.build())
    });
    (key, certificate)
}

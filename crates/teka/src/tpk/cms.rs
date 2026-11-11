use crate::tpk::ComponentType;
use anyhow::{ensure, Context, Result};
use openssl::{
    cms::{CMSOptions, CmsContentInfo},
    hash::MessageDigest,
    pkcs7::{Pkcs7, Pkcs7Flags},
    pkey::{PKeyRef, Private},
    stack::Stack,
    x509::{X509Ref, X509},
};

const SIGNED_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 7, 2];
const DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 7, 1];
const SHA256: &[u8] = &[0x60, 0x86, 0x48, 1, 0x65, 3, 4, 2, 1];

// Inspect the fields OpenSSL's safe CMS API does not expose. Only definite,
// minimally encoded DER lengths are accepted, including at the outer boundary.
fn field<'a>(input: &mut &'a [u8], tag: u8) -> Result<&'a [u8]> {
    ensure!(input.len() >= 2 && input[0] == tag, "unexpected CMS field");
    let mut length = usize::from(input[1]);
    let mut start = 2;
    if length & 0x80 != 0 {
        let count = length & 0x7f;
        ensure!(
            count > 0
                && count <= std::mem::size_of::<usize>()
                && input.len() >= 2 + count
                && input[2] != 0,
            "invalid CMS DER length"
        );
        length = 0;
        for byte in &input[2..2 + count] {
            length = length
                .checked_mul(256)
                .and_then(|n| n.checked_add(usize::from(*byte)))
                .context("CMS length overflow")?;
        }
        ensure!(length >= 128, "nonminimal CMS DER length");
        start += count;
    }
    let end = start.checked_add(length).context("CMS length overflow")?;
    ensure!(end <= input.len(), "truncated CMS field");
    let value = &input[start..end];
    *input = &input[end..];
    Ok(value)
}

fn algorithm<'a>(input: &mut &'a [u8]) -> Result<&'a [u8]> {
    let mut fields = field(input, 0x30)?;
    let oid = field(&mut fields, 6)?;
    if !fields.is_empty() {
        ensure!(
            field(&mut fields, 5)?.is_empty(),
            "unsupported CMS algorithm parameters"
        );
    }
    ensure!(fields.is_empty(), "invalid CMS algorithm");
    Ok(oid)
}

fn signed_data(der: &[u8]) -> Result<&[u8]> {
    let mut bytes = der;
    let mut content = field(&mut bytes, 0x30)?;
    ensure!(bytes.is_empty(), "trailing CMS data");
    ensure!(
        field(&mut content, 6)? == SIGNED_DATA,
        "CMS is not SignedData"
    );
    let mut explicit = field(&mut content, 0xa0)?;
    ensure!(content.is_empty(), "unexpected CMS content");
    let signed = field(&mut explicit, 0x30)?;
    ensure!(explicit.is_empty(), "unexpected CMS SignedData");
    Ok(signed)
}

fn encapsulated<'a>(input: &mut &'a [u8]) -> Result<Option<&'a [u8]>> {
    let mut content = field(input, 0x30)?;
    ensure!(
        field(&mut content, 6)? == DATA,
        "CMS content is not binary data"
    );
    if content.is_empty() {
        return Ok(None);
    }
    let mut explicit = field(&mut content, 0xa0)?;
    let bytes = field(&mut explicit, 4)?;
    ensure!(
        content.is_empty() && explicit.is_empty(),
        "invalid CMS content"
    );
    Ok(Some(bytes))
}

fn parse(der: &[u8]) -> Result<Pkcs7> {
    let cms = Pkcs7::from_der(der).context("parse CMS signature")?;
    ensure!(cms.to_der()? == der, "CMS is not canonical DER");
    Ok(cms)
}

fn manifest_size(content: &[u8]) -> Result<()> {
    ensure!(
        !content.is_empty() && content.len() as u64 <= crate::tpk::MAX_MANIFEST,
        "CMS manifest exceeds limit or is empty"
    );
    Ok(())
}

pub(crate) fn manifest(der: &[u8]) -> Result<Vec<u8>> {
    let mut signed = signed_data(der)?;
    field(&mut signed, 2)?;
    field(&mut signed, 0x31)?;
    let content = encapsulated(&mut signed)?.context("CMS manifest content must be attached")?;
    manifest_size(content)?;
    parse(der)?;
    Ok(content.to_vec())
}

#[derive(Clone, Copy)]
enum Mode {
    Attached,
    Detached,
}

pub(crate) fn validate(der: &[u8]) -> Result<()> {
    validate_profile(der, Mode::Detached)
}

fn validate_profile(der: &[u8], mode: Mode) -> Result<()> {
    let mut signed = signed_data(der)?;
    ensure!(
        field(&mut signed, 2)? == [1],
        "unsupported CMS SignedData version"
    );
    let mut algorithms = field(&mut signed, 0x31)?;
    ensure!(
        algorithm(&mut algorithms)? == SHA256 && algorithms.is_empty(),
        "CMS requires SHA-256"
    );
    let content = encapsulated(&mut signed)?;
    ensure!(
        matches!(
            (mode, &content),
            (Mode::Attached, Some(_)) | (Mode::Detached, None)
        ),
        "unexpected CMS attached or detached content"
    );
    let mut certificates = field(&mut signed, 0xa0)?;
    ensure!(
        !certificates.is_empty(),
        "CMS signer certificate is not embedded"
    );
    if matches!(mode, Mode::Attached) {
        manifest_size(content.context("CMS manifest content must be attached")?)?;
        field(&mut certificates, 0x30)?;
        ensure!(
            certificates.is_empty(),
            "CMS manifest requires one certificate"
        );
    }
    let mut signers = field(&mut signed, 0x31)?;
    ensure!(signed.is_empty(), "unsupported CMS fields");
    let mut signer = field(&mut signers, 0x30)?;
    ensure!(signers.is_empty(), "CMS requires one signer");
    ensure!(
        field(&mut signer, 2)? == [1],
        "unsupported CMS signer version"
    );
    field(&mut signer, 0x30)?;
    ensure!(
        algorithm(&mut signer)? == SHA256,
        "CMS signer requires SHA-256"
    );
    let signature_algorithm = algorithm(&mut signer)?;
    ensure!(
        signature_algorithm == [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 1, 1],
        "unsupported CMS signing profile"
    );
    ensure!(
        field(&mut signer, 4)?.len() == 384 && signer.is_empty(),
        "unsupported CMS signature length or attributes"
    );
    let pkcs7 = parse(der)?;
    if matches!(mode, Mode::Attached) {
        let certificates = pkcs7
            .signed()
            .and_then(|signed| signed.certificates())
            .context("CMS signer certificate is not embedded")?;
        profile(&certificates[0])?;
    }
    Ok(())
}

pub(crate) fn embedded(der: &[u8]) -> Result<Vec<X509>> {
    let mut signed = signed_data(der)?;
    field(&mut signed, 2)?;
    field(&mut signed, 0x31)?;
    let mode = if encapsulated(&mut signed)?.is_some() {
        Mode::Attached
    } else {
        Mode::Detached
    };
    validate_profile(der, mode)?;
    let pkcs7 = parse(der)?;
    let certificates = pkcs7
        .signed()
        .and_then(|signed| signed.certificates())
        .context("CMS has no embedded certificate")?;
    Ok(certificates
        .iter()
        .map(|certificate| certificate.to_owned())
        .collect())
}

pub(crate) fn fingerprint(certificate: &X509Ref) -> Result<String> {
    Ok(hex(&certificate.digest(MessageDigest::sha256())?))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn profile(certificate: &X509Ref) -> Result<()> {
    let key = certificate.public_key()?;
    ensure!(
        key.bits() == 3072 && key.rsa().is_ok(),
        "signatures require an RSA-3072 certificate"
    );
    Ok(())
}

#[cfg(test)]
pub(crate) fn sign(
    content: &[u8],
    key: &PKeyRef<Private>,
    certificate: &X509Ref,
) -> Result<Vec<u8>> {
    sign_content(content, key, certificate, Mode::Detached)
}

pub(crate) fn sign_manifest(
    content: &[u8],
    key: &PKeyRef<Private>,
    certificate: &X509Ref,
) -> Result<Vec<u8>> {
    manifest_size(content)?;
    sign_content(content, key, certificate, Mode::Attached)
}

fn sign_content(
    content: &[u8],
    key: &PKeyRef<Private>,
    certificate: &X509Ref,
    mode: Mode,
) -> Result<Vec<u8>> {
    profile(certificate)?;
    ensure!(
        certificate.public_key()?.public_eq(key),
        "signing key does not match certificate"
    );
    let certificates = Stack::new()?;
    let mut flags = Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY;
    if matches!(mode, Mode::Detached) {
        flags |= Pkcs7Flags::DETACHED;
    }
    let signature = Pkcs7::sign(certificate, key, &certificates, content, flags)?.to_der()?;
    validate_profile(&signature, mode)?;
    Ok(signature)
}

pub(crate) fn verify(
    content: &[u8],
    der: &[u8],
    trusted: &[X509],
    kind: ComponentType,
) -> Result<String> {
    validate(der)?;
    verify_content(Some(content), der, trusted, kind)
}

pub(crate) fn verify_manifest(
    content: &[u8],
    der: &[u8],
    trusted: &[X509],
    kind: ComponentType,
) -> Result<String> {
    ensure!(manifest(der)? == content, "CMS manifest content mismatch");
    validate_profile(der, Mode::Attached)?;
    verify_content(None, der, trusted, kind)
}

fn verify_content(
    content: Option<&[u8]>,
    der: &[u8],
    trusted: &[X509],
    kind: ComponentType,
) -> Result<String> {
    let embedded = embedded(der)?;
    for certificate in trusted {
        if profile(certificate).is_err() {
            continue;
        }
        let fingerprint = fingerprint(certificate)?;
        if !embedded
            .iter()
            .any(|embedded| self::fingerprint(embedded).is_ok_and(|value| value == fingerprint))
        {
            continue;
        }
        let mut certificates = Stack::new()?;
        certificates.push(certificate.clone())?;
        let mut cms = CmsContentInfo::from_der(der)?;
        if cms
            .verify(
                Some(&certificates),
                None,
                content,
                None,
                CMSOptions::NOINTERN | CMSOptions::NO_SIGNER_CERT_VERIFY | CMSOptions::BINARY,
            )
            .is_ok()
        {
            return Ok(fingerprint);
        }
    }
    anyhow::bail!("CMS signature has no valid trusted {:?} signer", kind)
}

#[cfg(test)]
#[path = "../../tests/common/mod.rs"]
pub(crate) mod fixtures;

#[cfg(test)]
mod tests {
    use super::*;
    use openssl::{
        ec::{EcGroup, EcKey},
        nid::Nid,
        pkey::PKey,
        rsa::Rsa,
    };

    #[test]
    fn verifies_exact_content_pinned_signer_and_role() {
        let (certificate, key) = fixtures::signer();
        let content = b"{ \"version\": 1 }\n";
        let der = sign(content, &key, &certificate).unwrap();
        let trusted = [certificate.clone()];
        assert_eq!(
            verify(content, &der, &trusted, ComponentType::App).unwrap(),
            fingerprint(&certificate).unwrap()
        );
        assert!(verify(b"{\"version\":1}", &der, &trusted, ComponentType::App).is_err());
        assert!(verify(content, &der, &[], ComponentType::App).is_err());
        let (wrong, wrong_key) = fixtures::signer();
        assert!(verify(content, &der, &[wrong], ComponentType::App).is_err());
        assert!(sign(content, &wrong_key, &certificate).is_err());
        let (os_certificate, os_key) =
            fixtures::certificate(PKey::from_rsa(Rsa::generate(3072).unwrap()).unwrap());
        assert!(verify(
            content,
            &der,
            std::slice::from_ref(&os_certificate),
            ComponentType::Os
        )
        .is_err());
        let der = sign(content, &os_key, &os_certificate).unwrap();
        verify(content, &der, &[os_certificate], ComponentType::Os).unwrap();
    }

    #[test]
    fn rejects_unsupported_cms_profiles() {
        let (certificate, key) = fixtures::signer();
        let certs = Stack::new().unwrap();
        for flags in [
            Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY,
            Pkcs7Flags::DETACHED | Pkcs7Flags::BINARY,
            Pkcs7Flags::DETACHED | Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY | Pkcs7Flags::NOCERTS,
        ] {
            let der = Pkcs7::sign(&certificate, &key, &certs, b"manifest", flags)
                .unwrap()
                .to_der()
                .unwrap();
            assert!(validate(&der).is_err());
        }
        let der = fixtures::sign(b"manifest", &certificate, &key);
        assert!(validate(&[der.clone(), vec![0]].concat()).is_err());
        assert!(validate(&der[..der.len() - 1]).is_err());
        let mut wrong_digest = der.clone();
        let offset = wrong_digest
            .windows(SHA256.len())
            .position(|window| window == SHA256)
            .unwrap();
        wrong_digest[offset + SHA256.len() - 1] = 2;
        assert!(validate(&wrong_digest).is_err());
        let mut wrong_version = der.clone();
        let offset = wrong_version
            .windows(3)
            .position(|window| window == [2, 1, 1])
            .unwrap();
        wrong_version[offset + 2] = 2;
        assert!(validate(&wrong_version).is_err());
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
        let (certificate, key) =
            fixtures::certificate(PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap());
        assert!(sign(b"manifest", &key, &certificate).is_err());
        assert!(validate(&fixtures::sign(b"manifest", &certificate, &key)).is_err());
        let (certificate, key) =
            fixtures::certificate(PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap());
        assert!(sign(b"manifest", &key, &certificate).is_err());
    }
    #[test]
    fn attached_manifest_verifies_original_content_and_pinned_certificate() {
        let (certificate, key) = fixtures::signer();
        let content = b"{ \"version\": 1 }\r\n";
        let der = sign_manifest(content, &key, &certificate).unwrap();
        let trusted = std::slice::from_ref(&certificate);
        assert_eq!(manifest(&der).unwrap(), content);
        assert_eq!(
            verify_manifest(content, &der, trusted, ComponentType::App).unwrap(),
            fingerprint(&certificate).unwrap()
        );
        assert!(verify_manifest(b"{\"version\":1}", &der, trusted, ComponentType::App).is_err());
        assert!(verify_manifest(content, &der, &[], ComponentType::App).is_err());
        let (wrong_certificate, wrong_key) = fixtures::signer();
        assert!(verify_manifest(content, &der, &[wrong_certificate], ComponentType::App).is_err());
        assert!(sign_manifest(content, &wrong_key, &certificate).is_err());
        let mut output = Vec::new();
        CmsContentInfo::from_der(&der)
            .unwrap()
            .verify(
                None,
                None,
                None,
                Some(&mut output),
                CMSOptions::NO_SIGNER_CERT_VERIFY | CMSOptions::BINARY,
            )
            .unwrap();
        assert_eq!(output, content);
        let mut corrupted = der;
        *corrupted.last_mut().unwrap() ^= 1;
        assert_eq!(manifest(&corrupted).unwrap(), content);
        assert!(verify_manifest(content, &corrupted, trusted, ComponentType::App).is_err());
    }

    #[test]
    fn attached_and_detached_profiles_cannot_be_confused() {
        let (certificate, key) = fixtures::signer();
        let content = b"manifest";
        let attached = sign_manifest(content, &key, &certificate).unwrap();
        let detached = sign(content, &key, &certificate).unwrap();
        let trusted = std::slice::from_ref(&certificate);
        assert!(validate(&attached).is_err());
        assert!(verify(content, &attached, trusted, ComponentType::App).is_err());
        assert!(manifest(&detached).is_err());
        assert!(verify_manifest(content, &detached, trusted, ComponentType::App).is_err());
        assert_eq!(embedded(&attached).unwrap().len(), 1);
        assert_eq!(embedded(&detached).unwrap().len(), 1);
        let (extra, _) = fixtures::signer();
        let mut certificates = Stack::new().unwrap();
        certificates.push(extra).unwrap();
        for (flags, attached) in [
            (Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY, true),
            (
                Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY | Pkcs7Flags::DETACHED,
                false,
            ),
        ] {
            let der = Pkcs7::sign(&certificate, &key, &certificates, content, flags)
                .unwrap()
                .to_der()
                .unwrap();
            if attached {
                assert_eq!(manifest(&der).unwrap(), content);
                assert!(verify_manifest(content, &der, trusted, ComponentType::App).is_err());
            } else {
                verify(content, &der, trusted, ComponentType::App).unwrap();
            }
        }
    }

    #[test]
    fn attached_verification_rejects_attributes_and_unsupported_profiles() {
        let (certificate, key) = fixtures::signer();
        let content = b"manifest";
        let trusted = std::slice::from_ref(&certificate);
        for flags in [
            Pkcs7Flags::BINARY,
            Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY | Pkcs7Flags::NOCERTS,
        ] {
            let der = Pkcs7::sign(&certificate, &key, &Stack::new().unwrap(), content, flags)
                .unwrap()
                .to_der()
                .unwrap();
            assert_eq!(manifest(&der).unwrap(), content);
            assert!(verify_manifest(content, &der, trusted, ComponentType::App).is_err());
        }
        let der = sign_manifest(content, &key, &certificate).unwrap();
        let mut wrong_digest = der.clone();
        let offset = wrong_digest
            .windows(SHA256.len())
            .position(|bytes| bytes == SHA256)
            .unwrap();
        wrong_digest[offset + SHA256.len() - 1] = 2;
        assert_eq!(manifest(&wrong_digest).unwrap(), content);
        assert!(verify_manifest(content, &wrong_digest, trusted, ComponentType::App).is_err());
        let mut wrong_version = der;
        let offset = wrong_version
            .windows(3)
            .position(|bytes| bytes == [2, 1, 1])
            .unwrap();
        wrong_version[offset + 2] = 2;
        assert!(verify_manifest(content, &wrong_version, trusted, ComponentType::App).is_err());
        for key in [
            PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap(),
            PKey::from_ec_key(
                EcKey::generate(&EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap()).unwrap(),
            )
            .unwrap(),
        ] {
            let (certificate, key) = fixtures::certificate(key);
            assert!(sign_manifest(content, &key, &certificate).is_err());
            let der = Pkcs7::sign(
                &certificate,
                &key,
                &Stack::new().unwrap(),
                content,
                Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY,
            )
            .unwrap()
            .to_der()
            .unwrap();
            assert_eq!(manifest(&der).unwrap(), content);
            assert!(verify_manifest(content, &der, &[certificate], ComponentType::App).is_err());
        }
    }

    fn der_field(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut bytes = vec![tag];
        if content.len() < 128 {
            bytes.push(content.len() as u8);
        } else {
            let length = content.len().to_be_bytes();
            let length = &length[length.iter().position(|byte| *byte != 0).unwrap()..];
            bytes.push(0x80 | length.len() as u8);
            bytes.extend_from_slice(length);
        }
        bytes.extend_from_slice(content);
        bytes
    }

    #[test]
    fn rejects_malformed_der_and_multiple_signers() {
        let (certificate, key) = fixtures::signer();
        let content = b"manifest";
        let der = sign_manifest(content, &key, &certificate).unwrap();
        assert!(manifest(&[der.as_slice(), &[0]].concat()).is_err());
        assert!(manifest(&der[..der.len() - 1]).is_err());
        let mut body = der.as_slice();
        let body = field(&mut body, 0x30).unwrap();
        assert!(manifest(&[&[0x30, 0x80], body, &[0, 0]].concat()).is_err());
        let count = usize::from(der[1] & 0x7f);
        assert!(manifest(&[&[0x30, 0x80 | (count + 1) as u8, 0], &der[2..]].concat()).is_err());
        let mut signed = signed_data(&der).unwrap();
        let original = signed;
        field(&mut signed, 2).unwrap();
        field(&mut signed, 0x31).unwrap();
        field(&mut signed, 0x30).unwrap();
        field(&mut signed, 0xa0).unwrap();
        let prefix = &original[..original.len() - signed.len()];
        let signers = field(&mut signed, 0x31).unwrap();
        let signed = der_field(
            0x30,
            &[prefix, &der_field(0x31, &[signers, signers].concat())].concat(),
        );
        let multiple = der_field(
            0x30,
            &[der_field(6, SIGNED_DATA), der_field(0xa0, &signed)].concat(),
        );
        let trusted = std::slice::from_ref(&certificate);
        assert!(verify_manifest(content, &multiple, trusted, ComponentType::App).is_err());

        let with_crls = der_field(
            0x30,
            &[prefix, &[0xa1, 0], &der_field(0x31, signers)].concat(),
        );
        let with_crls = der_field(
            0x30,
            &[der_field(6, SIGNED_DATA), der_field(0xa0, &with_crls)].concat(),
        );
        assert!(verify_manifest(content, &with_crls, trusted, ComponentType::App).is_err());

        let mut signer = signers;
        let fields = field(&mut signer, 0x30).unwrap();
        let with_attributes = der_field(0x31, &der_field(0x30, &[fields, &[0xa1, 0]].concat()));
        let with_attributes = der_field(0x30, &[prefix, &with_attributes].concat());
        let with_attributes = der_field(
            0x30,
            &[der_field(6, SIGNED_DATA), der_field(0xa0, &with_attributes)].concat(),
        );
        assert!(verify_manifest(content, &with_attributes, trusted, ComponentType::App).is_err());
    }
    #[test]
    fn manifest_content_must_be_nonempty_and_bounded() {
        let (certificate, key) = fixtures::signer();
        let oversized = vec![0; crate::tpk::MAX_MANIFEST as usize + 1];
        for content in [b"".as_slice(), oversized.as_slice()] {
            assert!(sign_manifest(content, &key, &certificate).is_err());
            let der = Pkcs7::sign(
                &certificate,
                &key,
                &Stack::new().unwrap(),
                content,
                Pkcs7Flags::NOATTR | Pkcs7Flags::BINARY,
            )
            .unwrap()
            .to_der()
            .unwrap();
            assert!(manifest(&der).is_err());
            assert!(verify_manifest(
                content,
                &der,
                std::slice::from_ref(&certificate),
                ComponentType::App
            )
            .is_err());
        }
    }
}

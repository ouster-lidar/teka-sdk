use crate::tpk::{image::Image, uki::VerifiedUki, ComponentType, Package};
use anyhow::{ensure, Context, Result};
use openssl::x509::X509;
use serde::Serialize;
use std::{
    fs,
    io::{Read, Seek, Write},
    path::Path,
};

#[derive(Debug)]
pub struct Operational;

impl std::fmt::Display for Operational {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("verification operation failed")
    }
}

impl std::error::Error for Operational {}

fn stage_uki(input: &mut impl Read, output: &mut impl Write) -> Result<()> {
    let mut buffer = [0; 8192];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        output.write_all(&buffer[..count]).context(Operational)?;
    }
}

#[derive(Default)]
pub struct Certificates {
    pub app: Vec<X509>,
    pub os: Vec<X509>,
}

impl Certificates {
    pub fn load(directory: &Path) -> Result<Self> {
        ensure!(directory.is_dir(), "trust directory does not exist");
        let load = |name: &str| -> Result<Vec<X509>> {
            let path = directory.join(name);
            match fs::read(&path) {
                Ok(bytes) => Ok(X509::stack_from_pem(&bytes)?),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(error) => Err(error)
                    .with_context(|| format!("read certificate bundle {}", path.display())),
            }
        };
        let certificates = Self {
            app: load("apps.pem")?,
            os: load("os.pem")?,
        };
        ensure!(
            !certificates.app.is_empty() || !certificates.os.is_empty(),
            "no configured role certificates"
        );
        Ok(certificates)
    }
}

#[derive(Debug, Serialize)]
pub struct ComponentVerification {
    pub id: String,
    pub image: Image,
    pub uki: Option<VerifiedUki>,
}

#[derive(Debug, Serialize)]
pub struct PackageSignature {
    pub valid: bool,
    pub certificate_fingerprint: String,
    pub role: ComponentType,
}

#[derive(Debug, Serialize)]
pub struct Verification {
    pub manifest: serde_json::Value,
    pub verification: VerificationDetails,
}

#[derive(Debug, Serialize)]
pub struct VerificationDetails {
    pub signatures: Signatures,
    pub integrity: Integrity,
    pub publisher_trust: PublisherTrust,
}

#[derive(Debug, Serialize)]
pub struct Signatures {
    pub manifest: PackageSignature,
    pub native: NativeSignatures,
}

#[derive(Debug, Serialize)]
pub struct NativeSignatures {
    pub valid: bool,
    pub components: Vec<ComponentVerification>,
}

#[derive(Debug, Serialize)]
pub struct Integrity {
    pub blake3: bool,
    pub root_hash_binding: bool,
    pub dm_verity_tree: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PublisherTrust {
    Trusted,
    NotChecked { reason: &'static str },
}

pub fn signatures(
    reader: &mut (impl Read + Seek),
    package: &Package,
    certificates: Option<&Certificates>,
) -> Result<Verification> {
    let role = package.manifest.signing_role();
    let trusted = match (certificates, role) {
        (Some(certificates), ComponentType::App) => certificates.app.clone(),
        (Some(certificates), ComponentType::Os) => certificates.os.clone(),
        (None, _) => crate::tpk::cms::embedded(&package.signature)?,
    };
    let fingerprint = crate::tpk::cms::verify_manifest(
        &package.manifest_bytes,
        &package.signature,
        &trusted,
        role,
    )
    .context("verify package manifest signature")?;

    crate::tpk::validate_metadata(package)?;
    let package_signature = PackageSignature {
        valid: true,
        certificate_fingerprint: fingerprint,
        role,
    };
    let mut components = Vec::new();
    for component in &package.manifest.components {
        let member = package
            .members
            .iter()
            .find(|member| member.path == component.image.path)
            .context("missing image member")?;
        let mut input = member.open(reader)?;
        let size = input.size();
        let image = Image::read(&mut input, 0, size, package.manifest.arch)?;
        std::io::copy(&mut input, &mut std::io::sink())?;
        let trusted = match (certificates, component.kind) {
            (Some(certificates), ComponentType::App) => certificates.app.clone(),
            (Some(certificates), ComponentType::Os) => certificates.os.clone(),
            (None, _) => image.embedded_certificates()?,
        };
        ensure!(
            !trusted.is_empty(),
            "no trusted {:?} image certificates configured",
            component.kind
        );
        image.verify_signature(&trusted, component.kind)?;
        image.verify_binding(component)?;
        ensure!(
            image.verity.size <= 64 * 1024 * 1024,
            "verity partition exceeds staging limit"
        );
        let mut input = member.open(reader)?;
        crate::tpk::image::skip(&mut input, image.verity.offset)?;
        let mut hash = vec![0; image.verity.size as usize];
        input
            .read_exact(&mut hash)
            .context("read verity partition")?;
        drop(input);
        let mut input = member.open(reader)?;
        crate::tpk::image::skip(&mut input, image.root.offset)?;
        crate::tpk::image::verify_verity_sized(
            &hash,
            input.take(image.root.size),
            Some(image.root.size),
            &component.root_hash,
        )?;
        let uki = if let Some(uki) = &component.uki {
            let member = package
                .members
                .iter()
                .find(|member| member.path == uki.path)
                .context("missing UKI member")?;
            let mut input = member.open(reader)?;
            ensure!(
                input.size() <= 256 * 1024 * 1024,
                "UKI exceeds staging limit"
            );
            let mut staging = tempfile::NamedTempFile::new().context(Operational)?;
            stage_uki(&mut input, staging.as_file_mut())?;
            let trusted = match certificates {
                Some(certificates) => certificates.os.clone(),
                None => crate::tpk::uki::embedded_certificates(staging.path())?,
            };
            ensure!(
                !trusted.is_empty(),
                "no trusted UKI certificates configured"
            );
            Some(crate::tpk::uki::verify_component(
                staging.path(),
                &trusted,
                component,
                package.manifest.arch,
            )?)
        } else {
            None
        };
        components.push(ComponentVerification {
            id: component.id.clone(),
            image,
            uki,
        });
    }
    Ok(Verification {
        manifest: serde_json::from_slice(&package.manifest_bytes)?,
        verification: VerificationDetails {
            signatures: Signatures {
                manifest: package_signature,
                native: NativeSignatures {
                    valid: true,
                    components,
                },
            },
            integrity: Integrity {
                blake3: true,
                root_hash_binding: true,
                dm_verity_tree: true,
            },
            publisher_trust: if certificates.is_some() {
                PublisherTrust::Trusted
            } else {
                PublisherTrust::NotChecked {
                    reason: "no_trust_directory",
                }
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn staging_distinguishes_input_rejection_from_output_failure() {
        struct BrokenInput;
        impl Read for BrokenInput {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("invalid compressed payload"))
            }
        }
        struct FullOutput;
        impl Write for FullOutput {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Ok(0)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let error = stage_uki(&mut BrokenInput, &mut Vec::new()).unwrap_err();
        assert!(!error.is::<Operational>());
        let error = stage_uki(&mut &b"UKI"[..], &mut FullOutput).unwrap_err();
        assert!(error.is::<Operational>());
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::WriteZero
        );
    }
}

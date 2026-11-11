use crate::tpk::verify::Operational;
use anyhow::{bail, ensure, Context, Result};
use object::{
    endian::LittleEndian,
    pe::IMAGE_DIRECTORY_ENTRY_SECURITY,
    read::{pe::PeFile64, ReadCache},
    Architecture as PeArchitecture, Object, ObjectSection,
};
use openssl::{
    pkcs7::{Pkcs7, Pkcs7Flags},
    stack::Stack,
    x509::X509,
};
use serde::Serialize;
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    process::{Command, Output},
};

const MAX_METADATA: u64 = 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct VerifiedUki {
    pub size: u64,
    pub root_hash: String,
}

// object::ReadCache discards source I/O errors when parsing fails.
struct PeInput<R> {
    inner: R,
    error: Option<io::Error>,
}

impl<R> PeInput<R> {
    fn observe<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        match result {
            Err(error) => {
                let kind = error.kind();
                self.error = Some(error);
                Err(kind.into())
            }
            otherwise => otherwise,
        }
    }
}

impl<R: Read> Read for PeInput<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.inner.read(bytes) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => return self.observe(result),
            }
        }
    }
}

impl<R: Seek> Seek for PeInput<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let result = self.inner.seek(position);
        self.observe(result)
    }
}

fn with_pe<R: Read + Seek, T>(
    input: R,
    parse: impl FnOnce(&ReadCache<PeInput<R>>) -> Result<T>,
) -> Result<T> {
    let cache = ReadCache::new(PeInput {
        inner: input,
        error: None,
    });
    let result = parse(&cache);
    if let Some(error) = cache.into_inner().error {
        return Err(error).context(Operational);
    }
    result
}

fn run_verifier(command: &mut Command) -> Result<Output> {
    let output = command
        .output()
        .with_context(|| format!("run {}", command.get_program().to_string_lossy()))
        .context(Operational)?;
    if output.status.code().is_none() {
        return Err(anyhow::anyhow!(
            "{} terminated with {}",
            command.get_program().to_string_lossy(),
            output.status
        ))
        .context(Operational);
    }
    Ok(output)
}

fn text_section(path: &Path, name: &str) -> Result<Option<String>> {
    let file = File::open(path).context(Operational)?;
    let length = file.metadata().context(Operational)?.len();
    with_pe(file, |cache| {
        let pe = PeFile64::parse(cache).context("invalid UKI PE executable")?;
        let mut sections = pe
            .sections()
            .filter(|section| section.name().ok() == Some(name));
        let Some(section) = sections.next() else {
            return Ok(None);
        };
        ensure!(sections.next().is_none(), "duplicate UKI {name} section");
        let (offset, size) = section
            .file_range()
            .context("UKI section has no file data")?;
        ensure!(
            size > 0
                && size <= MAX_METADATA
                && section.size() <= MAX_METADATA
                && offset.checked_add(size).is_some_and(|end| end <= length),
            "empty, oversized or truncated UKI {name}"
        );
        let bytes = section.data().context("invalid UKI section data")?;
        let text = std::str::from_utf8(bytes)
            .context("invalid UKI section encoding")?
            .trim_end_matches('\0');
        ensure!(!text.contains('\0'), "embedded NUL in UKI {name}");
        Ok(Some(text.to_owned()))
    })
}

fn signatures(path: &Path) -> Result<Vec<Pkcs7>> {
    let mut file = File::open(path).context(Operational)?;
    let length = file.metadata().context(Operational)?.len();
    let (mut offset, end) = with_pe(&mut file, |cache| {
        let pe = PeFile64::parse(cache).context("invalid UKI PE executable")?;
        let directory = pe
            .data_directory(IMAGE_DIRECTORY_ENTRY_SECURITY)
            .context("UKI has no Authenticode signature")?;
        let offset = u64::from(directory.virtual_address.get(LittleEndian));
        let size = u64::from(directory.size.get(LittleEndian));
        let end = offset
            .checked_add(size)
            .context("UKI certificate table overflow")?;
        ensure!(
            offset % 8 == 0 && size > 0 && size <= MAX_METADATA && end <= length,
            "invalid UKI certificate table"
        );
        Ok((offset, end))
    })?;
    let mut signatures = Vec::new();
    while offset < end {
        ensure!(end - offset >= 8, "truncated UKI certificate header");
        file.seek(SeekFrom::Start(offset)).context(Operational)?;
        let mut header = [0; 8];
        file.read_exact(&mut header).context(Operational)?;
        let size = u64::from(u32::from_le_bytes(header[..4].try_into()?));
        let revision = u16::from_le_bytes(header[4..6].try_into()?);
        let kind = u16::from_le_bytes(header[6..8].try_into()?);
        ensure!(
            (8..=MAX_METADATA).contains(&size) && size <= end - offset,
            "invalid UKI certificate size"
        );
        ensure!(
            revision == 0x0200 && kind == 0x0002,
            "unsupported UKI certificate type"
        );
        let mut der = vec![0; (size - 8) as usize];
        file.read_exact(&mut der).context(Operational)?;
        signatures.push(Pkcs7::from_der(&der).context("invalid UKI PKCS#7 signature")?);
        let aligned = size
            .checked_add(7)
            .context("UKI certificate alignment overflow")?
            & !7;
        offset = offset
            .checked_add(aligned)
            .context("UKI certificate offset overflow")?;
        ensure!(offset <= end, "truncated UKI certificate padding");
    }
    ensure!(!signatures.is_empty(), "UKI has no Authenticode signatures");
    Ok(signatures)
}

pub fn embedded_certificates(path: &Path) -> Result<Vec<X509>> {
    let empty = Stack::new()?;
    let mut certificates = Vec::new();
    for signature in signatures(path)? {
        certificates.extend(signature.signers(&empty, Pkcs7Flags::empty())?);
    }
    Ok(certificates)
}

pub fn verify(path: &Path, trusted: &[X509], expected_root_hash: &str) -> Result<VerifiedUki> {
    ensure!(
        !trusted.is_empty(),
        "UKI verification requires explicit trusted certificates"
    );
    let cmdline = text_section(path, ".cmdline")?.context("UKI missing .cmdline")?;
    let mut hashes = cmdline
        .split_whitespace()
        .filter_map(|word| word.strip_prefix("roothash="));
    let root_hash = hashes.next().context("UKI missing roothash=")?;
    ensure!(hashes.next().is_none(), "duplicate UKI roothash=");
    crate::tpk::validate_digest(root_hash).context("invalid UKI root hash")?;
    ensure!(
        root_hash == expected_root_hash,
        "UKI root hash does not match component"
    );

    let empty = Stack::new()?;
    for signature in signatures(path)? {
        let der = signature.to_der()?;
        let mut detached = tempfile::NamedTempFile::new().context(Operational)?;
        detached.write_all(&der).context(Operational)?;
        for signer in signature.signers(&empty, Pkcs7Flags::empty())? {
            let signer_der = signer.to_der()?;
            if !trusted
                .iter()
                .any(|certificate| certificate.to_der().is_ok_and(|der| der == signer_der))
            {
                continue;
            }
            let mut certificate = tempfile::NamedTempFile::new().context(Operational)?;
            certificate
                .write_all(&signer.to_pem()?)
                .context(Operational)?;
            #[cfg(not(target_os = "macos"))]
            let output = run_verifier(
                Command::new(option_env!("TEKA_SBVERIFY").unwrap_or("sbverify"))
                    .arg("--cert")
                    .arg(certificate.path())
                    .arg("--detached")
                    .arg(detached.path())
                    .arg("--")
                    .arg(path),
            )?;
            #[cfg(target_os = "macos")]
            let output = osslsigncode(path, certificate.path(), &signer)?;
            if !output.status.success() {
                continue;
            }
            return Ok(VerifiedUki {
                size: std::fs::metadata(path).context(Operational)?.len(),
                root_hash: root_hash.to_owned(),
            });
        }
    }
    bail!("UKI has no valid signature from a trusted certificate")
}

#[cfg(target_os = "macos")]
fn osslsigncode(path: &Path, certificate: &Path, signer: &X509) -> Result<std::process::Output> {
    run_verifier(
        Command::new(option_env!("TEKA_OSSLSIGNCODE").unwrap_or("osslsigncode"))
            .args(["verify", "-in"])
            .arg(path)
            .arg("-CAfile")
            .arg(certificate)
            .arg("-require-leaf-hash")
            .arg(format!("sha256:{}", crate::tpk::cms::fingerprint(signer)?))
            .args(["-ignore-timestamp", "-ignore-crl"]),
    )
}

pub fn verify_component(
    path: &Path,
    trusted: &[X509],
    component: &crate::tpk::Component,
    arch: crate::tpk::Architecture,
) -> Result<VerifiedUki> {
    let verified = verify(path, trusted, &component.root_hash)?;
    let expected = match arch {
        crate::tpk::Architecture::Aarch64 => PeArchitecture::Aarch64,
        crate::tpk::Architecture::X86_64 => PeArchitecture::X86_64,
    };
    with_pe(File::open(path).context(Operational)?, |cache| {
        let pe = PeFile64::parse(cache).context("invalid UKI PE executable")?;
        ensure!(
            pe.architecture() == expected,
            "UKI architecture does not match manifest"
        );
        Ok(())
    })?;
    let osrel = text_section(path, ".osrel")?.context("UKI missing .osrel")?;
    let field = |name: &str| -> Result<&str> {
        let mut values = osrel.lines().filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key == name).then_some(value.trim())
        });
        let value = values
            .next()
            .with_context(|| format!("UKI missing {name}"))?;
        ensure!(values.next().is_none(), "duplicate UKI {name}");
        let value = if value.starts_with(['\"', '\'']) {
            ensure!(
                value.len() >= 2 && value.ends_with(&value[..1]),
                "invalid quoted UKI {name}"
            );
            &value[1..value.len() - 1]
        } else {
            value
        };
        ensure!(!value.is_empty(), "empty UKI {name}");
        Ok(value)
    };
    ensure!(
        field("IMAGE_ID")? == component.id,
        "UKI IMAGE_ID does not match component"
    );
    let version = format!("{}+{}", field("IMAGE_VERSION")?, field("BUILD_ID")?);
    ensure!(
        component.version == version,
        "UKI version does not match component"
    );
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pe_parser_preserves_file_read_and_seek_failures() {
        struct BrokenInput {
            seek: bool,
        }
        impl Read for BrokenInput {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("read failed"))
            }
        }
        impl Seek for BrokenInput {
            fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
                if self.seek {
                    Err(io::Error::other("seek failed"))
                } else {
                    Ok(4096)
                }
            }
        }
        for seek in [false, true] {
            let error = with_pe(BrokenInput { seek }, |cache| {
                PeFile64::parse(cache).context("parse UKI")?;
                Ok(())
            })
            .unwrap_err();
            assert!(error.is::<Operational>());
            assert_eq!(
                error.downcast_ref::<io::Error>().unwrap().to_string(),
                if seek { "seek failed" } else { "read failed" }
            );
        }
        let error = with_pe(io::Cursor::new([0; 128]), |cache| {
            PeFile64::parse(cache).context("parse UKI")?;
            Ok(())
        })
        .unwrap_err();
        assert!(!error.is::<Operational>());
    }

    #[test]
    fn verifier_launch_and_signal_failures_are_operational() {
        let directory = tempfile::tempdir().unwrap();
        let error = run_verifier(&mut Command::new(directory.path().join("missing"))).unwrap_err();
        assert!(error.is::<Operational>());
        let error = run_verifier(Command::new("sh").args(["-c", "kill -TERM $$"])).unwrap_err();
        assert!(error.is::<Operational>());
        let rejected = run_verifier(Command::new("sh").args(["-c", "exit 1"])).unwrap();
        assert_eq!(rejected.status.code(), Some(1));
    }
}

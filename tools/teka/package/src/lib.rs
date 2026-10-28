#![forbid(unsafe_code)]
mod cms;
pub mod image;
mod stream;
pub mod transport;
pub mod uki;
pub mod verify;
mod zstd_member;
pub use stream::stream;

use anyhow::{ensure, Context, Result};
use openssl::{
    pkey::{PKeyRef, Private},
    x509::X509Ref,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub const MAX_MANIFEST: u64 = 64 * 1024;
pub const MAX_SIGNATURE: u64 = 64 * 1024;
pub const MAX_ARCHIVE: u64 = 64 * 1024 * 1024 * 1024;
pub const MAX_MEMBER: u64 = 8 * 1024 * 1024 * 1024 - 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Architecture {
    #[serde(rename = "aarch64")]
    Aarch64,
    #[serde(rename = "x86_64")]
    X86_64,
}

impl Architecture {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64",
            Self::X86_64 => "x86_64",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentType {
    App,
    Os,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileDescriptor {
    pub path: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    #[serde(rename = "type")]
    pub kind: ComponentType,
    pub id: String,
    pub version: String,
    pub root_hash: String,
    pub image: FileDescriptor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uki: Option<FileDescriptor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub arch: Architecture,
    pub components: Vec<Component>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bao_root: Option<String>,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let manifest = Self::parse_draft(bytes)?;
        manifest.validate_storage()?;
        Ok(manifest)
    }

    pub(crate) fn parse_draft(bytes: &[u8]) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() as u64 <= MAX_MANIFEST,
            "manifest exceeds limit or is empty"
        );
        let manifest: Self = serde_json::from_slice(bytes).context("parse TPK manifest")?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn signing_role(&self) -> ComponentType {
        if self
            .components
            .iter()
            .any(|component| component.kind == ComponentType::Os)
        {
            ComponentType::Os
        } else {
            ComponentType::App
        }
    }

    pub(crate) fn files(&self) -> impl Iterator<Item = &FileDescriptor> + '_ {
        self.components
            .iter()
            .flat_map(|c| std::iter::once(&c.image).chain(c.uki.iter()))
    }

    fn validate_storage(&self) -> Result<u64> {
        validate_digest(self.bao_root.as_ref().context("missing Bao root")?)?;
        let mut size = 0u64;
        for file in self.files() {
            ensure!(
                file.size > 0 && file.size <= MAX_MEMBER,
                "invalid member size"
            );
            size = size
                .checked_add(file.size)
                .context("payload size overflow")?;
        }
        ensure!(size <= MAX_ARCHIVE, "payloads exceed size limit");
        transport::encoded_size(size)?;
        Ok(size)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.format == 1, "unsupported TPK format");
        ensure!(!self.components.is_empty(), "empty component list");
        for c in &self.components {
            ensure!(!c.id.trim().is_empty(), "empty component ID");
            validate_digest(&c.root_hash)?;
            ensure!(!c.version.trim().is_empty(), "empty component version");
            match c.kind {
                ComponentType::App => {
                    ensure!(valid_id(&c.id), "invalid app ID");
                    ensure!(c.uki.is_none(), "app component has a UKI");
                    ensure!(
                        c.image.path == format!("{}.raw", c.id),
                        "app image path does not match ID"
                    );
                }
                ComponentType::Os => {
                    ensure!(c.uki.is_some(), "OS UKI is required");
                }
            }
        }
        let mut paths = BTreeSet::new();
        for file in self.files() {
            validate_path(&file.path)?;
            ensure!(paths.insert(&file.path), "duplicate component member path");
            ensure!(file.size <= MAX_MEMBER, "invalid member size");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Member {
    pub path: String,
    pub size: u64,
    pub offset: u64,
    archive_root: [u8; 32],
    archive_size: u64,
    archive_offset: u64,
}

#[derive(Clone, Debug)]
pub struct Package {
    pub manifest_bytes: Vec<u8>,
    pub signature: Vec<u8>,
    pub manifest: Manifest,
    pub members: Vec<Member>,
    archive_offset: u64,
    archive_size: u64,
}

pub fn valid_id(id: &str) -> bool {
    id.len() <= 64
        && id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn validate_digest(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid lowercase 64-hex digest"
    );
    Ok(())
}

fn validate_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty()
            && path.len() <= 100
            && path != "."
            && path != ".."
            && !path.contains(['/', '\\', '\0']),
        "invalid flat member path {path:?}"
    );
    Ok(())
}

pub const MAGIC: &[u8; 8] = b"TPK 1\n\0\0";

fn root_bytes(root: &str) -> Result<[u8; 32]> {
    validate_digest(root)?;
    Ok(*bao_tree::blake3::Hash::from_hex(root)?.as_bytes())
}

impl Package {
    pub fn root(&self) -> Result<[u8; 32]> {
        root_bytes(
            self.manifest
                .bao_root
                .as_ref()
                .context("missing Bao root")?,
        )
    }

    pub fn archive_size(&self) -> u64 {
        self.archive_size
    }
}

fn read_cms(reader: &mut impl Read) -> Result<Vec<u8>> {
    let mut prefix = [0; 16];
    reader
        .read_exact(&mut prefix)
        .context("read TPK envelope")?;
    ensure!(&prefix[..8] == MAGIC, "unsupported TPK envelope");
    let size = u32::from_le_bytes(prefix[8..12].try_into()?) as u64;
    ensure!(
        size > 0 && size <= MAX_SIGNATURE,
        "CMS object exceeds limit or is empty"
    );
    ensure!(prefix[12..16] == [0; 4], "nonzero TPK reserved field");
    let mut signature = vec![0; size as usize];
    reader
        .read_exact(&mut signature)
        .context("read CMS object")?;
    Ok(signature)
}

/// Reads unverified manifest bytes for offline inspection only.
/// Installation callers must use read_trusted with configured role certificates.
pub fn inspect_manifest(reader: &mut impl Read) -> Result<Vec<u8>> {
    cms::manifest(&read_cms(reader)?)
}

pub(crate) fn read_envelope(
    reader: &mut impl Read,
    certificates: Option<&verify::Certificates>,
) -> Result<Package> {
    let signature = read_cms(reader)?;
    let manifest_bytes = cms::manifest(&signature)?;
    let trusted = match certificates {
        Some(certificates) => certificates
            .app
            .iter()
            .chain(&certificates.os)
            .cloned()
            .collect(),
        None => cms::embedded(&signature)?,
    };
    let signer = cms::verify_manifest(&manifest_bytes, &signature, &trusted, ComponentType::App)
        .context("authenticate envelope before JSON")?;
    let manifest = Manifest::parse(&manifest_bytes)?;
    if let Some(certificates) = certificates {
        let trusted = match manifest.signing_role() {
            ComponentType::App => &certificates.app,
            ComponentType::Os => &certificates.os,
        };
        ensure!(
            trusted
                .iter()
                .any(|certificate| cms::fingerprint(certificate).is_ok_and(|value| value == signer)),
            "enforce package signing role"
        );
    }
    let archive_root = root_bytes(manifest.bao_root.as_ref().context("missing Bao root")?)?;
    let archive_size = manifest.validate_storage()?;
    let archive_offset = 16 + signature.len() as u64;
    let mut offset = 0u64;
    let members = manifest
        .files()
        .map(|file| {
            let member = Member {
                path: file.path.clone(),
                size: file.size,
                offset,
                archive_root,
                archive_size,
                archive_offset,
            };
            offset = offset
                .checked_add(file.size)
                .context("member offset overflow")?;
            Ok(member)
        })
        .collect::<Result<_>>()?;
    Ok(Package {
        manifest_bytes,
        signature,
        manifest,
        members,
        archive_offset,
        archive_size,
    })
}

/// Reads cryptographically valid metadata without asserting publisher trust.
/// Installation callers must use read_trusted with configured role certificates.
pub fn read(reader: &mut (impl Read + Seek)) -> Result<Package> {
    read_seekable(reader, None)
}

pub fn read_trusted(
    reader: &mut (impl Read + Seek),
    certificates: &verify::Certificates,
) -> Result<Package> {
    read_seekable(reader, Some(certificates))
}

fn read_seekable(
    reader: &mut (impl Read + Seek),
    certificates: Option<&verify::Certificates>,
) -> Result<Package> {
    reader.seek(SeekFrom::Start(0))?;
    let package = read_envelope(reader, certificates)?;
    let end = reader.seek(SeekFrom::End(0))?;
    ensure!(
        end == package
            .archive_offset
            .checked_add(transport::encoded_size(package.archive_size())?)
            .context("encoded archive length overflow")?,
        "truncated or trailing encoded archive"
    );
    reader.seek(SeekFrom::Start(package.archive_offset))?;
    Ok(package)
}

impl Member {
    fn stored<'a, R: Read + Seek>(
        &self,
        reader: &'a mut R,
    ) -> Result<transport::SeekReader<&'a mut R>> {
        let mut archive = transport::SeekReader::new(
            reader,
            self.archive_root,
            self.archive_size,
            self.archive_offset,
        )?;
        archive.seek(SeekFrom::Start(self.offset))?;
        Ok(archive)
    }
}

pub(crate) fn validate_metadata(package: &Package) -> Result<()> {
    let manifest = Manifest::parse(&package.manifest_bytes)?;
    ensure!(
        manifest == package.manifest,
        "package manifest differs from signed bytes"
    );
    ensure!(
        manifest.files().count() == package.members.len(),
        "package member count differs from manifest"
    );
    let root = package.root()?;
    let size = manifest.validate_storage()?;
    ensure!(
        size == package.archive_size,
        "package payload size differs from manifest"
    );
    let mut offset = 0u64;
    for (file, member) in manifest.files().zip(&package.members) {
        ensure!(
            member.path == file.path
                && member.size == file.size
                && member.offset == offset
                && member.archive_root == root
                && member.archive_size == size
                && member.archive_offset == package.archive_offset,
            "package member differs from manifest"
        );
        offset = offset
            .checked_add(file.size)
            .context("member offset overflow")?;
    }
    Ok(())
}

/// Checks complete authenticated transport and decodes every member.
pub fn verify(reader: &mut (impl Read + Seek), package: &Package) -> Result<()> {
    validate_metadata(package)?;
    for member in &package.members {
        member.check(reader, io::sink())?;
    }
    Ok(())
}

pub fn file_descriptor(path: &Path, member_path: &str) -> Result<FileDescriptor> {
    validate_path(member_path)?;
    let info = std::fs::metadata(path).with_context(|| format!("open {}", path.display()))?;
    ensure!(info.is_file(), "input must be a regular file");
    ensure!(
        info.len() > 0 && info.len() <= MAX_MEMBER,
        "invalid member size"
    );
    Ok(FileDescriptor {
        path: member_path.to_owned(),
        size: 0,
    })
}

/// Reads a package holding exactly one component of `kind` and decodes every member.
pub fn read_single(path: &Path, kind: ComponentType) -> Result<Manifest> {
    let what = match kind {
        ComponentType::App => "app",
        ComponentType::Os => "OS",
    };
    ensure!(
        path.extension().is_some_and(|ext| ext == "tpk"),
        "{}: {what} package requires a .tpk file",
        path.display()
    );
    (|| {
        let mut file = std::fs::File::open(path)?;
        ensure!(file.metadata()?.is_file(), "input must be a regular file");
        let package = read(&mut file)?;
        let [component] = &package.manifest.components[..] else {
            anyhow::bail!("{what} package requires exactly one component");
        };
        ensure!(
            component.kind == kind,
            "{what} package requires an {what} component"
        );
        verify(&mut file, &package)?;
        Ok(package.manifest)
    })()
    .with_context(|| format!("validate {} as a single-{what} TPK package", path.display()))
}

/// Level 9 saves 3-5% vs level 3 for ~13% slower decode; 10+ costs 1.4-4x compress time for <1% more.
pub const DEFAULT_ZSTD_LEVEL: i32 = 9;
/// Levels above 19 need windows larger than the decoder's 8 MiB cap.
pub const MAX_ZSTD_LEVEL: i32 = 19;

pub fn pack(
    writer: &mut impl Write,
    manifest: &Manifest,
    files: &BTreeMap<String, PathBuf>,
    private_key: &PKeyRef<Private>,
    certificate: &X509Ref,
) -> Result<()> {
    let bytes = serde_json::to_vec(manifest)?;
    zstd_member::pack(
        writer,
        &bytes,
        files,
        DEFAULT_ZSTD_LEVEL,
        private_key,
        certificate,
    )
}

pub fn pack_manifest(
    writer: &mut impl Write,
    bytes: &[u8],
    files: &BTreeMap<String, PathBuf>,
    level: i32,
    private_key: &PKeyRef<Private>,
    certificate: &X509Ref,
) -> Result<()> {
    zstd_member::pack(writer, bytes, files, level, private_key, certificate)
}

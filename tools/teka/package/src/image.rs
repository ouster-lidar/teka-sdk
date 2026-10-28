use crate::{cms::hex, validate_digest, Architecture, Component, ComponentType};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use openssl::x509::X509;
use serde::{Deserialize, Serialize};
use std::io::{self, Read};

const SECTOR: u64 = 512;
const SIGNATURE_SIZE: u64 = 16 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct Partition {
    pub number: u32,
    pub offset: u64,
    pub size: u64,
    pub uuid: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct VeritySignature {
    pub root_hash: String,
    pub certificate_fingerprint: String,
    pub signature: String,
}

impl VeritySignature {
    fn der(&self) -> Result<Vec<u8>> {
        validate_digest(&self.root_hash)?;
        validate_digest(&self.certificate_fingerprint)?;
        let bytes = STANDARD
            .decode(&self.signature)
            .context("decode image signature")?;
        ensure!(
            !bytes.is_empty() && bytes.len() as u64 <= SIGNATURE_SIZE,
            "invalid image signature size"
        );
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Image {
    pub size: u64,
    pub root: Partition,
    pub verity: Partition,
    pub signature_partition: Partition,
    pub signature: VeritySignature,
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed GPT field"),
    )
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed GPT field"),
    )
}

fn uuid(bytes: &[u8]) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        u32_at(bytes, 0),
        u16::from_le_bytes(bytes[4..6].try_into().expect("fixed UUID field")),
        u16::from_le_bytes(bytes[6..8].try_into().expect("fixed UUID field")),
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

fn partition_types(arch: Architecture) -> [&'static str; 3] {
    match arch {
        Architecture::X86_64 => [
            "4f68bce3-e8cd-4db1-96e7-fbcaf984b709",
            "2c7357ed-ebd2-46d9-aec1-23d437ec2bf5",
            "41092b05-9fc8-4523-994f-2def0408b176",
        ],
        Architecture::Aarch64 => [
            "b921b045-1df0-41c3-af44-4c6f280d3fae",
            "df3300ce-d69f-4c92-978c-9bfb0f38d820",
            "6db69de6-29f4-4758-a7a5-962190f00ce3",
        ],
    }
}

fn type_guid(value: &str) -> [u8; 16] {
    let value = value.replace('-', "");
    let mut bytes = [0u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
    }
    bytes[..4].reverse();
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    bytes
}

impl Image {
    pub fn read(
        reader: &mut impl Read,
        offset: u64,
        size: u64,
        arch: Architecture,
    ) -> Result<Self> {
        let end = offset.checked_add(size).context("image range overflow")?;
        ensure!(
            size >= 3 * SECTOR && size.is_multiple_of(SECTOR),
            "invalid GPT image size"
        );
        skip(reader, offset + SECTOR)?;
        let mut header = [0; 512];
        reader.read_exact(&mut header).context("read GPT header")?;
        ensure!(&header[..8] == b"EFI PART", "missing GPT header");
        ensure!(u32_at(&header, 8) == 0x10000, "unsupported GPT revision");
        let header_size = u32_at(&header, 12) as usize;
        ensure!((92..=512).contains(&header_size), "invalid GPT header size");
        let header_crc = u32_at(&header, 16);
        header[16..20].fill(0);
        ensure!(
            crc32fast::hash(&header[..header_size]) == header_crc,
            "invalid GPT header checksum"
        );
        ensure!(
            u32_at(&header, 20) == 0 && u64_at(&header, 24) == 1,
            "invalid primary GPT header"
        );
        ensure!(
            u64_at(&header, 32) == size / SECTOR - 1,
            "invalid backup GPT location"
        );
        let first = u64_at(&header, 40);
        let last = u64_at(&header, 48);
        ensure!(
            first >= 2 && first <= last && last < size / SECTOR - 1,
            "invalid GPT usable range"
        );
        let table_lba = u64_at(&header, 72);
        let count = u32_at(&header, 80);
        let entry_size = u32_at(&header, 84) as u64;
        ensure!(
            table_lba >= 2 && count > 0 && entry_size >= 128 && entry_size.is_multiple_of(128),
            "invalid GPT entry dimensions"
        );
        let table_offset = table_lba
            .checked_mul(SECTOR)
            .context("GPT table offset overflow")?;
        let table_size = u64::from(count)
            .checked_mul(entry_size)
            .context("GPT table size overflow")?;
        ensure!(
            table_offset
                .checked_add(table_size)
                .is_some_and(|end| end <= first * SECTOR),
            "GPT table overlaps usable range"
        );
        skip(reader, table_offset - 2 * SECTOR)?;
        let mut crc = crc32fast::Hasher::new();
        let expected = partition_types(arch);
        let mut partitions: [Option<Partition>; 3] = [None, None, None];
        for number in 0..count {
            let mut entry = [0; 128];
            reader.read_exact(&mut entry).context("read GPT entry")?;
            crc.update(&entry);
            let mut remaining = entry_size - 128;
            let mut buffer = [0; 4096];
            while remaining > 0 {
                let length = remaining.min(buffer.len() as u64) as usize;
                reader
                    .read_exact(&mut buffer[..length])
                    .context("read GPT entry extension")?;
                crc.update(&buffer[..length]);
                remaining -= length as u64;
            }
            if entry[..16].iter().all(|byte| *byte == 0) {
                continue;
            }
            let role = expected
                .iter()
                .position(|guid| type_guid(guid) == entry[..16])
                .context("unexpected GPT partition type or architecture")?;
            ensure!(partitions[role].is_none(), "duplicate GPT partition type");
            let start = u64_at(&entry, 32);
            let finish = u64_at(&entry, 40);
            ensure!(
                start >= first && start <= finish && finish <= last,
                "GPT partition outside usable range"
            );
            let partition = Partition {
                number: number + 1,
                offset: offset + start * SECTOR,
                size: (finish - start + 1) * SECTOR,
                uuid: uuid(&entry[16..32]),
            };
            for other in partitions.iter().flatten() {
                ensure!(
                    partition.offset + partition.size <= other.offset
                        || other.offset + other.size <= partition.offset,
                    "overlapping GPT partitions"
                );
            }
            partitions[role] = Some(partition);
        }
        ensure!(
            crc.finalize() == u32_at(&header, 88),
            "invalid GPT table checksum"
        );
        let [root, verity, signature_partition] = partitions;
        let root = root.context("missing root partition")?;
        let verity = verity.context("missing verity partition")?;
        let signature_partition =
            signature_partition.context("missing verity signature partition")?;
        ensure!(
            signature_partition.size == SIGNATURE_SIZE,
            "verity signature partition must be 16 KiB"
        );
        let position = offset + table_offset + table_size;
        ensure!(
            signature_partition.offset >= position,
            "signature partition overlaps GPT table"
        );
        skip(reader, signature_partition.offset - position)?;
        let mut bytes = [0; SIGNATURE_SIZE as usize];
        reader
            .read_exact(&mut bytes)
            .context("read image signature metadata")?;
        skip(reader, end - (signature_partition.offset + SIGNATURE_SIZE))?;
        let length = bytes
            .iter()
            .rposition(|byte| *byte != 0)
            .map_or(0, |index| index + 1);
        let signature: VeritySignature =
            serde_json::from_slice(&bytes[..length]).context("parse image signature metadata")?;
        signature.der()?;
        Ok(Self {
            size,
            root,
            verity,
            signature_partition,
            signature,
        })
    }

    pub fn verify_binding(&self, component: &Component) -> Result<()> {
        ensure!(
            self.signature.root_hash == component.root_hash,
            "manifest and image root hashes differ"
        );
        Ok(())
    }

    pub fn embedded_certificates(&self) -> Result<Vec<X509>> {
        crate::cms::embedded(&self.signature.der()?)
    }

    pub fn verify_signature(&self, trusted: &[X509], kind: ComponentType) -> Result<()> {
        let trusted = trusted
            .iter()
            .filter(|certificate| {
                crate::cms::fingerprint(certificate)
                    .is_ok_and(|value| value == self.signature.certificate_fingerprint)
            })
            .cloned()
            .collect::<Vec<_>>();
        crate::cms::verify(
            self.signature.root_hash.as_bytes(),
            &self.signature.der()?,
            &trusted,
            kind,
        )?;
        Ok(())
    }
}

/// Recomputes the dm-verity tree from `data` and checks it against the `hash` partition and `root_hash`.
pub fn verify_verity_sized(
    hash: &[u8],
    mut data: impl Read,
    data_size: Option<u64>,
    root_hash: &str,
) -> Result<()> {
    ensure!(
        hash.len() >= 512 && hash[..8] == *b"verity\0\0",
        "missing verity superblock"
    );
    ensure!(
        u32_at(hash, 8) == 1
            && u32_at(hash, 12) == 1
            && hash[32..64] == *b"sha256\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        "unsupported verity format"
    );
    let (data_block, block) = (u32_at(hash, 64) as usize, u32_at(hash, 68) as usize);
    let salt_size = usize::from(u16::from_le_bytes([hash[80], hash[81]]));
    ensure!(
        [data_block, block]
            .iter()
            .all(|size| size.is_power_of_two() && (512..=65536).contains(size))
            && salt_size <= 256
            && u64_at(hash, 72) > 0,
        "invalid verity superblock"
    );
    let blocks = u64_at(hash, 72);
    let expected_data = blocks
        .checked_mul(data_block as u64)
        .context("verity data size overflow")?;
    ensure!(
        expected_data <= crate::MAX_MEMBER && data_size.is_none_or(|size| size == expected_data),
        "verity data geometry does not match root partition"
    );
    let mut count = blocks;
    let mut tree_size = 0u64;
    while count > 1 {
        let bytes = count.checked_mul(32).context("verity tree size overflow")?;
        count = bytes.div_ceil(block as u64);
        tree_size = tree_size
            .checked_add(
                count
                    .checked_mul(block as u64)
                    .context("verity tree size overflow")?,
            )
            .context("verity tree size overflow")?;
    }
    ensure!(
        (block as u64)
            .checked_add(tree_size)
            .is_some_and(|size| size <= hash.len() as u64),
        "verity tree exceeds hash partition"
    );
    let salt = &hash[88..88 + salt_size];
    let digest = |bytes: &[u8]| {
        let mut sha = openssl::sha::Sha256::new();
        sha.update(salt);
        sha.update(bytes);
        sha.finish()
    };
    let mut buffer = vec![0; data_block];
    let mut level = Vec::new();
    for _ in 0..blocks {
        data.read_exact(&mut buffer).context("read verity data")?;
        level.extend(digest(&buffer));
    }
    ensure!(
        data.read(&mut [0])? == 0,
        "root partition exceeds verity data"
    );
    if blocks == 1 {
        ensure!(hex(&level) == root_hash, "verity root hash mismatch");
        return Ok(());
    }
    let mut levels = Vec::new();
    loop {
        level.resize(level.len().next_multiple_of(block), 0);
        let next = (level.len() > block).then(|| level.chunks(block).flat_map(digest).collect());
        levels.push(level);
        let Some(next) = next else { break };
        level = next;
    }
    let tree = levels.iter().rev().flatten().copied().collect::<Vec<_>>();
    ensure!(
        hash.len() >= block + tree.len() && hash[block..block + tree.len()] == tree,
        "verity hash tree mismatch"
    );
    ensure!(
        hex(&digest(&levels[levels.len() - 1])) == root_hash,
        "verity root hash mismatch"
    );
    Ok(())
}

pub(crate) fn skip(reader: &mut impl Read, length: u64) -> Result<()> {
    ensure!(
        io::copy(&mut reader.take(length), &mut io::sink())? == length,
        "truncated image member"
    );
    Ok(())
}

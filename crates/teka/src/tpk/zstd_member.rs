use crate::tpk::{FileDescriptor, Manifest, Member, MAX_MANIFEST, MAX_MEMBER};
use anyhow::{ensure, Context, Result};
use openssl::{
    pkey::{PKeyRef, Private},
    x509::X509Ref,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, BufReader, Chain, Cursor, Read, Seek, Take, Write},
    path::PathBuf,
};

type Decoder<R> =
    zstd::stream::read::Decoder<'static, BufReader<Chain<Cursor<Vec<u8>>, Stored<R>>>>;

struct Stored<R: Read> {
    input: Take<R>,
    failed: bool,
}

impl<R: Read> Stored<R> {
    fn remaining(&self) -> u64 {
        self.input.limit()
    }
}

impl<R: Read> Read for Stored<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stored payload verification failed",
            ));
        }
        if self.input.limit() == 0 {
            return Ok(0);
        }
        let count = match self.input.read(bytes) {
            Ok(count) => count,
            Err(error) => {
                self.failed = true;
                return Err(error);
            }
        };
        if count == 0 {
            self.failed = true;
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated stored payload",
            ));
        }
        Ok(count)
    }
}

pub struct LogicalReader<R: Read> {
    decoder: Option<Decoder<R>>,
    size: u64,
    remaining: u64,
    failed: bool,
}

impl Member {
    pub fn open<'a, R: Read + Seek>(
        &self,
        reader: &'a mut R,
    ) -> Result<LogicalReader<crate::tpk::transport::SeekReader<&'a mut R>>> {
        self.decode(self.stored(reader)?)
    }

    pub(crate) fn decode<R: Read>(&self, reader: R) -> Result<LogicalReader<R>> {
        let mut reader = Stored {
            input: reader.take(self.size),
            failed: false,
        };
        ensure!(self.size >= 5, "truncated zstd frame");
        let mut header = vec![0; self.size.min(18) as usize];
        reader.read_exact(&mut header)?;
        ensure!(
            header[..4] == [0x28, 0xb5, 0x2f, 0xfd],
            "not an ordinary zstd frame"
        );
        let size = zstd::zstd_safe::get_frame_content_size(&header)
            .map_err(|_| anyhow::anyhow!("invalid zstd frame header"))?
            .context("zstd frame has no content size")?;
        ensure!(
            size > 0 && size <= MAX_MEMBER,
            "invalid decoded member size"
        );
        let input = BufReader::new(Cursor::new(header).chain(reader));
        let mut decoder = zstd::stream::read::Decoder::with_buffer(input)?.single_frame();
        decoder.window_log_max(23)?;
        Ok(LogicalReader {
            decoder: Some(decoder),
            size,
            remaining: size,
            failed: false,
        })
    }

    pub fn copy_range(
        &self,
        reader: &mut (impl Read + Seek),
        offset: u64,
        size: Option<u64>,
        output: &mut impl Write,
    ) -> Result<()> {
        self.open(reader)?.copy_range(offset, size, output)
    }

    /// Decodes the whole member into `output`, checking its frame and decoded size.
    pub fn check(&self, reader: &mut (impl Read + Seek), mut output: impl Write) -> Result<()> {
        io::copy(&mut self.open(reader)?, &mut output)
            .with_context(|| format!("decode member {}", self.path))?;
        Ok(())
    }
}

impl<R: Read> LogicalReader<R> {
    pub fn size(&self) -> u64 {
        self.size
    }

    pub(crate) fn copy_range(
        mut self,
        offset: u64,
        size: Option<u64>,
        output: &mut impl Write,
    ) -> Result<()> {
        let size = size.unwrap_or(
            self.size
                .checked_sub(offset)
                .context("member offset out of bounds")?,
        );
        ensure!(
            offset.checked_add(size).is_some_and(|end| end <= self.size),
            "member range out of bounds"
        );
        ensure!(
            io::copy(&mut (&mut self).take(offset), &mut io::sink())? == offset,
            "truncated logical member prefix"
        );
        ensure!(
            io::copy(&mut (&mut self).take(size), output)? == size,
            "truncated logical member range"
        );
        io::copy(&mut self, &mut io::sink())?;
        Ok(())
    }

    fn read_inner(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let Some(input) = self.decoder.as_mut() else {
            return Ok(0);
        };
        if self.remaining > 0 {
            let limit = self.remaining.min(bytes.len() as u64) as usize;
            let count = input.read(&mut bytes[..limit])?;
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated decoded zstd member",
                ));
            }
            self.remaining -= count as u64;
            return Ok(count);
        }
        let mut extra = [0];
        if input.read(&mut extra)? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "zstd member exceeds logical size",
            ));
        }
        let input = self.decoder.take().expect("active decoder").finish();
        if !input.buffer().is_empty() || input.get_ref().get_ref().1.remaining() != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "trailing bytes after zstd frame",
            ));
        }
        Ok(0)
    }
}

impl<R: Read> Read for LogicalReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "payload decoder failed",
            ));
        }
        let result = self.read_inner(bytes);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}

fn encode(
    source: &mut File,
    target: &mut File,
    file: &FileDescriptor,
    size: u64,
    level: i32,
) -> Result<u64> {
    let start = target.stream_position()?;
    let mut encoder = zstd::stream::write::Encoder::new(target, level)?;
    encoder.window_log(23)?;
    encoder.include_contentsize(true)?;
    encoder.include_checksum(false)?;
    encoder.set_pledged_src_size(Some(size))?;
    ensure!(
        io::copy(&mut source.take(size + 1), &mut encoder)? == size,
        "input size mismatch for {}",
        file.path
    );
    let size = encoder.finish()?.stream_position()? - start;
    ensure!(size > 0 && size <= MAX_MEMBER, "invalid zstd stored size");
    Ok(size)
}

pub(crate) fn pack(
    writer: &mut impl Write,
    bytes: &[u8],
    files: &BTreeMap<String, PathBuf>,
    level: i32,
    private_key: &PKeyRef<Private>,
    certificate: &X509Ref,
) -> Result<()> {
    ensure!(
        (1..=crate::tpk::MAX_ZSTD_LEVEL).contains(&level),
        "zstd level must be 1-{}",
        crate::tpk::MAX_ZSTD_LEVEL
    );
    ensure!(bytes.len() as u64 <= MAX_MANIFEST, "manifest exceeds limit");
    let mut manifest = Manifest::parse_draft(bytes)?;
    ensure!(
        files.keys().collect::<BTreeSet<_>>() == manifest.files().map(|file| &file.path).collect(),
        "input members do not match manifest"
    );
    let mut stored = tempfile::NamedTempFile::new()?;
    for component in &mut manifest.components {
        for descriptor in std::iter::once(&mut component.image).chain(component.uki.iter_mut()) {
            let mut source = File::open(&files[&descriptor.path])?;
            let metadata = source.metadata()?;
            ensure!(metadata.is_file(), "input must be a regular file");
            let size = metadata.len();
            ensure!(size > 0 && size <= MAX_MEMBER, "invalid input member size");
            descriptor.size = encode(&mut source, stored.as_file_mut(), descriptor, size, level)?;
        }
    }

    let mut total_size = 0u64;
    for descriptor in manifest.files() {
        total_size = total_size
            .checked_add(descriptor.size)
            .context("payload size overflow")?;
        ensure!(
            total_size <= crate::tpk::MAX_ARCHIVE,
            "payloads exceed size limit"
        );
    }
    let encoding = crate::tpk::transport::Encoder::new(stored.as_file())?;
    manifest.bao_root = Some(encoding.root());
    let bytes = serde_json::to_vec(&manifest)?;
    Manifest::parse(&bytes)?;
    let signature = crate::tpk::cms::sign_manifest(&bytes, private_key, certificate)?;
    ensure!(
        signature.len() as u64 <= crate::tpk::MAX_SIGNATURE,
        "signature exceeds limit"
    );
    writer.write_all(crate::tpk::MAGIC)?;
    writer.write_all(&(signature.len() as u32).to_le_bytes())?;
    writer.write_all(&0u32.to_le_bytes())?;
    writer.write_all(&signature)?;
    encoding.write(stored.as_file(), writer)?;
    Ok(())
}

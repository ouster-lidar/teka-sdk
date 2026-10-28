use anyhow::{ensure, Result};
use bao_tree::{
    io::{
        outboard::PreOrderOutboard,
        sync::{encode_ranges_validated, CreateOutboard, DecodeResponseIter},
        BaoContentItem, Leaf,
    },
    iter::{BaoChunk, ResponseIter},
    BaoTree, BlockSize, ChunkNum, ChunkRanges,
};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    sync::LazyLock,
};

pub const BLOCK_SIZE: u64 = 64 * 1024;
static ALL: LazyLock<ChunkRanges> = LazyLock::new(ChunkRanges::all);

fn tree(size: u64) -> BaoTree {
    BaoTree::new(size, BlockSize::from_chunk_log(6))
}

pub fn encoded_size(size: u64) -> Result<u64> {
    size.checked_add(tree(size).outboard_size())
        .ok_or_else(|| anyhow::anyhow!("Bao encoded size overflow"))
}

pub struct Encoder {
    outboard: PreOrderOutboard<Vec<u8>>,
}

impl Encoder {
    pub fn new(input: &File) -> Result<Self> {
        encoded_size(input.metadata()?.len())?;
        Ok(Self {
            outboard: PreOrderOutboard::create(input, tree(0).block_size())?,
        })
    }

    pub fn root(&self) -> String {
        self.outboard.root.to_hex().to_string()
    }

    pub fn size(&self) -> u64 {
        self.outboard.tree.size()
    }

    pub fn write(&self, input: &File, output: &mut impl Write) -> Result<()> {
        ensure!(
            input.metadata()?.len() == self.size(),
            "archive size changed"
        );
        encode_ranges_validated(input, &self.outboard, &ALL, output)?;
        Ok(())
    }
}

fn invalid(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

struct Exact<R> {
    input: R,
    remaining: u64,
}

impl<R: Read> Read for Exact<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let limit = output
            .len()
            .min(self.remaining.min(usize::MAX as u64) as usize);
        let count = self.input.read(&mut output[..limit])?;
        if count == 0 && self.remaining != 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.remaining -= count as u64;
        if self.remaining == 0 && self.input.read(&mut [0])? != 0 {
            return Err(invalid("data after Bao archive"));
        }
        Ok(count)
    }
}

pub struct StreamReader<R> {
    decoder: DecodeResponseIter<'static, Exact<R>>,
    leaf: Option<Leaf>,
    offset: usize,
    failed: bool,
}

impl<R: Read> StreamReader<R> {
    pub fn new(mut input: R, root: [u8; 32], size: u64) -> io::Result<Self> {
        let remaining = encoded_size(size).map_err(invalid)?;
        if remaining == 0 && input.read(&mut [0])? != 0 {
            return Err(invalid("data after Bao archive"));
        }
        Ok(Self {
            decoder: DecodeResponseIter::new(
                root.into(),
                tree(size),
                Exact { input, remaining },
                &ALL,
            ),
            leaf: None,
            offset: 0,
            failed: false,
        })
    }

    pub fn finish(&mut self) -> io::Result<()> {
        io::copy(self, &mut io::sink())?;
        Ok(())
    }
}

impl<R: Read> Read for StreamReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.failed {
            return Err(invalid("Bao reader failed"));
        }
        if output.is_empty() {
            return Ok(0);
        }
        loop {
            if let Some(leaf) = &self.leaf {
                let count = output.len().min(leaf.data.len() - self.offset);
                if count != 0 {
                    output[..count].copy_from_slice(&leaf.data[self.offset..self.offset + count]);
                    self.offset += count;
                    return Ok(count);
                }
            }
            match self.decoder.next() {
                Some(Ok(BaoContentItem::Leaf(leaf))) => {
                    self.leaf = Some(leaf);
                    self.offset = 0;
                }
                Some(Ok(BaoContentItem::Parent(_))) => {}
                Some(Err(error)) => {
                    self.failed = true;
                    return Err(invalid(error));
                }
                None => return Ok(0),
            }
        }
    }
}

pub struct SeekReader<R> {
    input: R,
    root: [u8; 32],
    size: u64,
    encoded_offset: u64,
    position: u64,
    leaf: Option<Leaf>,
    failed: bool,
}

impl<R: Read + Seek> SeekReader<R> {
    pub fn new(mut input: R, root: [u8; 32], size: u64, encoded_offset: u64) -> io::Result<Self> {
        let end = encoded_offset
            .checked_add(encoded_size(size).map_err(invalid)?)
            .ok_or_else(|| invalid("Bao archive offset overflow"))?;
        if input.seek(SeekFrom::End(0))? != end {
            return Err(invalid("Bao archive encoded length mismatch"));
        }
        Ok(Self {
            input,
            root,
            size,
            encoded_offset,
            position: 0,
            leaf: None,
            failed: false,
        })
    }

    fn load(&mut self) -> io::Result<()> {
        let bao = tree(self.size);
        let chunk = ChunkNum(self.position / BLOCK_SIZE * (BLOCK_SIZE / 1024));
        let end = ChunkNum(chunk.0 + BLOCK_SIZE / 1024);
        // A final unbounded range prevents traversal into omitted interior proofs.
        let ranges = if end.to_bytes() >= self.size {
            ChunkRanges::from(chunk..)
        } else {
            ChunkRanges::from(chunk..end)
        };
        let mut encoded = Vec::with_capacity(BLOCK_SIZE as usize + 4096);
        let mut leaf_offset = 0;
        for item in ResponseIter::new(bao, ranges.clone()) {
            let (offset, size) = match item {
                BaoChunk::Parent { node, .. } => {
                    let start = node.chunk_range().start.to_bytes();
                    // Combined preorder places preceding payload bytes before this proof.
                    let offset = start + bao.pre_order_offset(node).unwrap() * 64;
                    leaf_offset = offset + 64;
                    if self.position >= node.mid().to_bytes() {
                        let left_size = node.mid().to_bytes() - start;
                        leaf_offset += left_size + tree(left_size).outboard_size();
                    }
                    (offset, 64)
                }
                BaoChunk::Leaf { size, .. } => (leaf_offset, size),
            };
            self.input
                .seek(SeekFrom::Start(self.encoded_offset + offset))?;
            let start = encoded.len();
            encoded.resize(start + size, 0);
            self.input.read_exact(&mut encoded[start..])?;
        }
        for item in DecodeResponseIter::new(self.root.into(), bao, encoded.as_slice(), &ranges) {
            if let BaoContentItem::Leaf(leaf) = item.map_err(invalid)? {
                self.leaf = Some(leaf);
            }
        }
        Ok(())
    }
}

impl<R: Read + Seek> Read for SeekReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.failed {
            return Err(invalid("Bao reader failed"));
        }
        if output.is_empty() || self.position == self.size {
            return Ok(0);
        }
        if !self.leaf.as_ref().is_some_and(|leaf| {
            self.position >= leaf.offset && self.position - leaf.offset < leaf.data.len() as u64
        }) {
            if let Err(error) = self.load() {
                self.failed = true;
                return Err(error);
            }
        }
        let leaf = self.leaf.as_ref().unwrap();
        let offset = (self.position - leaf.offset) as usize;
        let count = output.len().min(leaf.data.len() - offset);
        output[..count].copy_from_slice(&leaf.data[offset..offset + count]);
        self.position += count as u64;
        Ok(count)
    }
}

impl<R: Read + Seek> Seek for SeekReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if self.failed {
            return Err(invalid("Bao reader failed"));
        }
        let position = match position {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => self.size.checked_add_signed(offset),
        }
        .filter(|&position| position <= self.size)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek outside Bao archive"))?;
        self.position = position;
        Ok(position)
    }
}

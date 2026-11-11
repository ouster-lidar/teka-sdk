#![cfg(feature = "tpk")]

use std::{
    cell::RefCell,
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    rc::Rc,
};
use teka::tpk::transport::{encoded_size, Encoder, SeekReader, StreamReader, BLOCK_SIZE};

fn encoded(size: usize) -> (Vec<u8>, Vec<u8>, [u8; 32]) {
    let data: Vec<u8> = (0..size)
        .map(|i| ((i * 31 + i / 7919) % 251) as u8)
        .collect();
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&data).unwrap();
    let encoder = Encoder::new(&file).unwrap();
    assert_eq!(encoder.size(), size as u64);
    let root = *bao_tree::blake3::Hash::from_hex(encoder.root())
        .unwrap()
        .as_bytes();
    let mut encoded = Vec::new();
    encoder.write(&file, &mut encoded).unwrap();
    assert_eq!(encoded.len() as u64, encoded_size(size as u64).unwrap());
    (data, encoded, root)
}

#[test]
fn forward_roundtrip_and_exact_framing() {
    for size in [
        0,
        1,
        1024,
        BLOCK_SIZE as usize,
        BLOCK_SIZE as usize + 1,
        5 * BLOCK_SIZE as usize + 13,
    ] {
        let (data, encoded, root) = encoded(size);
        let mut stream = StreamReader::new(encoded.as_slice(), root, size as u64).unwrap();
        let mut output = Vec::new();
        stream.read_to_end(&mut output).unwrap();
        stream.finish().unwrap();
        assert_eq!(output, data);
        if size == 0 {
            continue;
        }
        for cut in [0, encoded.len() - 1] {
            let mut stream = StreamReader::new(&encoded[..cut], root, size as u64).unwrap();
            assert!(stream.finish().is_err());
            assert!(stream.read(&mut [0]).is_err());
        }
        let mut appended = encoded.clone();
        appended.push(0);
        let mut stream = StreamReader::new(appended.as_slice(), root, size as u64).unwrap();
        assert!(stream.finish().is_err());
        assert!(SeekReader::new(Cursor::new(appended), root, size as u64, 0).is_err());
    }
}

#[test]
fn decoder_stops_after_parent_mismatch() {
    let size = 3 * BLOCK_SIZE + 17;
    let (_, mut encoded, root) = encoded(size as usize);
    encoded[0] ^= 1;
    let tree = bao_tree::BaoTree::new(size, bao_tree::BlockSize::from_chunk_log(6));
    let ranges = bao_tree::ChunkRanges::all();
    let mut decoder =
        bao_tree::io::sync::DecodeResponseIter::new(root.into(), tree, encoded.as_slice(), &ranges);
    assert!(decoder.next().unwrap().is_err());
    assert!(decoder.next().is_none());
    assert!(decoder.next().is_none());
}

#[test]
fn rejects_corruption_before_releasing_bad_block() {
    let size = 3 * BLOCK_SIZE as usize + 17;
    let (data, encoded, root) = encoded(size);
    for position in [0, 64, encoded.len() - 1] {
        let mut corrupt = encoded.clone();
        corrupt[position] ^= 1;
        let mut stream = StreamReader::new(corrupt.as_slice(), root, size as u64).unwrap();
        let mut output = Vec::new();
        assert!(stream.read_to_end(&mut output).is_err());
        assert_eq!(output, data[..output.len()]);
        assert!(output.len() < data.len());
        assert!(stream.read(&mut [0]).is_err());
    }
}

struct Tracked {
    inner: Cursor<Vec<u8>>,
    reads: Rc<RefCell<Vec<(u64, usize)>>>,
}
impl Read for Tracked {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let start = self.inner.position();
        let count = self.inner.read(output)?;
        self.reads.borrow_mut().push((start, count));
        Ok(count)
    }
}
impl Seek for Tracked {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

#[test]
fn seek_reads_only_requested_block_and_proofs() {
    for size in [
        1,
        BLOCK_SIZE as usize,
        BLOCK_SIZE as usize + 1,
        5 * BLOCK_SIZE as usize + 13,
        8 * BLOCK_SIZE as usize,
    ] {
        let (data, mut encoded, root) = encoded(size);
        let reads = Rc::new(RefCell::new(Vec::new()));
        encoded.splice(..0, [0; 29]);
        let source = Tracked {
            inner: Cursor::new(encoded),
            reads: reads.clone(),
        };
        let mut reader = SeekReader::new(source, root, size as u64, 29).unwrap();
        for position in [size - 1, size / 2, 0] {
            reads.borrow_mut().clear();
            reader.seek(SeekFrom::Start(position as u64)).unwrap();
            let mut byte = [0];
            reader.read_exact(&mut byte).unwrap();
            assert_eq!(byte[0], data[position]);
            let block = position as u64 / BLOCK_SIZE * BLOCK_SIZE;
            for &(offset, count) in reads.borrow().iter() {
                if count > 64 {
                    assert!(offset >= 29 + block, "decoded an earlier payload block");
                    assert!(count as u64 <= BLOCK_SIZE);
                }
            }
        }
        reader.seek(SeekFrom::Start(0)).unwrap();
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        assert_eq!(output, data);
        assert!(reader.seek(SeekFrom::End(1)).is_err());
        assert!(reader.seek(SeekFrom::Start(u64::MAX)).is_err());
    }
}

#[test]
fn seek_corruption_poison_is_terminal() {
    let size = 5 * BLOCK_SIZE as usize + 13;
    let (_, mut encoded, root) = encoded(size);
    *encoded.last_mut().unwrap() ^= 1;
    let mut reader = SeekReader::new(Cursor::new(encoded), root, size as u64, 0).unwrap();
    reader.seek(SeekFrom::End(-1)).unwrap();
    assert!(reader.read(&mut [0]).is_err());
    assert!(reader.seek(SeekFrom::Start(0)).is_err());
    assert!(reader.read(&mut [0]).is_err());
}

#[test]
fn later_block_seek_does_not_authenticate_prefix_payload() {
    let size = 5 * BLOCK_SIZE as usize + 13;
    let (data, mut encoded, root) = encoded(size);
    let first_payload = encoded
        .windows(128)
        .position(|bytes| bytes == &data[..128])
        .unwrap();
    encoded[first_payload] ^= 1;
    let mut reader = SeekReader::new(Cursor::new(encoded), root, size as u64, 0).unwrap();
    reader.seek(SeekFrom::Start(4 * BLOCK_SIZE)).unwrap();
    let mut output = [0; 128];
    reader.read_exact(&mut output).unwrap();
    assert_eq!(
        output,
        data[4 * BLOCK_SIZE as usize..4 * BLOCK_SIZE as usize + 128]
    );
    reader.seek(SeekFrom::Start(0)).unwrap();
    assert!(reader.read(&mut [0]).is_err());
}

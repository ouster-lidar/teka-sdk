use crate::tpk::{read_envelope, transport, verify::Certificates};
use anyhow::{ensure, Context, Result};
use std::io::{self, Read, Write};

/// Authenticates a sequential package and emits a selected logical range.
/// Output can precede an error in a later Bao group or transport trailer.
pub fn stream<R: Read>(
    mut reader: R,
    certificates: &Certificates,
    member: &str,
    offset: u64,
    size: Option<u64>,
    output: &mut impl Write,
) -> Result<()> {
    let package = read_envelope(&mut reader, Some(certificates))?;
    let selected = package
        .members
        .iter()
        .position(|item| item.path == member)
        .context("missing member")?;
    let mut archive =
        transport::StreamReader::new(reader, package.root()?, package.archive_size())?;
    for (index, member) in package.members.iter().enumerate() {
        if index == selected {
            member
                .decode(&mut archive)?
                .copy_range(offset, size, output)?;
        } else {
            ensure!(
                io::copy(&mut (&mut archive).take(member.size), &mut io::sink())? == member.size,
                "truncated stored payload"
            );
        }
    }
    let mut extra = [0];
    ensure!(archive.read(&mut extra)? == 0, "unexpected archive bytes");
    Ok(())
}

#![forbid(unsafe_code)]

use anyhow::{ensure, Context, Result};
use clap::Subcommand;
use openssl::{pkey::PKey, x509::X509};
use std::{collections::BTreeMap, fs::File, io::Read, path::PathBuf};

#[derive(Debug, Subcommand)]
pub enum PackageCommand {
    /// Inspect a TPK package offline
    Inspect(crate::inspect::InspectOptions),
    /// Authenticate and emit a logical member range from disk or HTTP(S)
    Stream {
        #[arg(value_name = "FILE_OR_HTTP_URL")]
        file: String,
        #[arg(long, value_name = "DIR", value_hint = clap::ValueHint::DirPath)]
        trust: PathBuf,
        #[arg(long)]
        member: String,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long)]
        size: Option<u64>,
    },
    /// Hash payloads, sign the manifest, and atomically create a TPK package
    Pack {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, value_name = "NAME=FILE", required = true)]
        member: Vec<String>,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        private_key: PathBuf,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        certificate: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, env = "TEKA_ZSTD_LEVEL", default_value_t = teka_pkg::DEFAULT_ZSTD_LEVEL)]
        zstd_level: i32,
    },
}

pub fn run(command: PackageCommand) -> Result<()> {
    match command {
        PackageCommand::Inspect(args) => crate::inspect::run(args),
        PackageCommand::Stream {
            file,
            trust,
            member,
            offset,
            size,
        } => {
            let certificates = teka_pkg::verify::Certificates::load(&trust)?;
            if file.starts_with("http://") || file.starts_with("https://") {
                let response = reqwest::blocking::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .no_gzip()
                    .no_brotli()
                    .no_deflate()
                    .no_zstd()
                    .connect_timeout(std::time::Duration::from_secs(15))
                    .build()?
                    .get(&file)
                    .header(reqwest::header::ACCEPT_ENCODING, "identity")
                    .send()?;
                ensure!(
                    response.status().is_success(),
                    "package HTTP {}",
                    response.status()
                );
                return teka_pkg::stream(
                    response,
                    &certificates,
                    &member,
                    offset,
                    size,
                    &mut std::io::stdout().lock(),
                );
            }
            let mut file = File::open(file)?;
            ensure!(file.metadata()?.is_file(), "input must be a regular file");
            let package = teka_pkg::read_trusted(&mut file, &certificates)?;
            let member = package
                .members
                .iter()
                .find(|item| item.path == member)
                .context("missing member")?;
            member.copy_range(&mut file, offset, size, &mut std::io::stdout().lock())
        }
        PackageCommand::Pack {
            manifest,
            member,
            output,
            private_key,
            certificate,
            zstd_level,
        } => {
            let mut bytes = Vec::new();
            File::open(manifest)?
                .take(teka_pkg::MAX_MANIFEST + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= teka_pkg::MAX_MANIFEST,
                "manifest exceeds limit"
            );
            let mut sources = BTreeMap::new();
            for member in member {
                let (name, file) = member.split_once('=').context("member must be NAME=FILE")?;
                ensure!(!file.is_empty(), "empty member source path");
                ensure!(
                    sources
                        .insert(name.to_owned(), PathBuf::from(file))
                        .is_none(),
                    "duplicate member input"
                );
            }
            let private_key = PKey::private_key_from_pem(&std::fs::read(private_key)?)?;
            let certificate = X509::from_pem(&std::fs::read(certificate)?)?;
            let parent = output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            let mut staging = tempfile::NamedTempFile::new_in(parent)?;
            teka_pkg::pack_manifest(
                staging.as_file_mut(),
                &bytes,
                &sources,
                zstd_level,
                &private_key,
                &certificate,
            )?;
            staging.persist_noclobber(output)?;
            Ok(())
        }
    }
}

use anyhow::{ensure, Context, Result};
use clap::Args;
use std::{fs::File, path::PathBuf};
use teka::tpk::{
    verify::{signatures, Certificates, PublisherTrust},
    ComponentType,
};

#[derive(Clone, Debug, Args)]
pub struct InspectOptions {
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    pub file: PathBuf,
    /// Verify package authentication and native payloads
    #[arg(long)]
    pub verify: bool,
    /// Trust directory containing apps.pem and os.pem certificate bundles; without it,
    /// signatures are checked against their embedded certificates
    #[arg(long, requires = "verify", value_name = "DIR", value_hint = clap::ValueHint::DirPath)]
    pub trust: Option<PathBuf>,
    /// Print the package report as JSON
    #[arg(long)]
    pub json: bool,
    /// Show full hashes
    #[arg(short, long)]
    pub verbose: bool,
}

pub fn run(args: InspectOptions) -> Result<()> {
    let mut file =
        File::open(&args.file).with_context(|| format!("open {}", args.file.display()))?;
    ensure!(file.metadata()?.is_file(), "input must be a regular file");
    let (manifest_bytes, verified) = if args.verify {
        let certificates = args
            .trust
            .as_ref()
            .map(|trust| Certificates::load(trust))
            .transpose()?;
        let package = match &certificates {
            Some(certificates) => teka::tpk::read_trusted(&mut file, certificates)?,
            None => teka::tpk::read(&mut file)?,
        };
        let verified = signatures(&mut file, &package, certificates.as_ref())?;
        (package.manifest_bytes, Some(verified))
    } else {
        (teka::tpk::inspect_manifest(&mut file)?, None)
    };
    if args.json {
        let report = match &verified {
            Some(verified) => serde_json::to_value(verified)?,
            None => serde_json::json!({
                "manifest": serde_json::from_slice::<serde_json::Value>(&manifest_bytes)?
            }),
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let manifest = teka::tpk::Manifest::parse(&manifest_bytes)?;
    println!("Manifest");
    for (index, component) in manifest.components.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!(
            "  {} {} ({}, {})",
            component.id.escape_debug(),
            component.version.escape_debug(),
            kind(component.kind),
            manifest.arch.as_str()
        );
        println!("  root {}", hash(&component.root_hash, args.verbose));
        let rows = std::iter::once(&component.image)
            .chain(component.uki.iter())
            .map(|file| (file.path.clone(), size(file.size)))
            .collect::<Vec<_>>();
        let name_width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0);
        for (name, stored) in rows {
            println!("  - {:<name_width$}  {stored:>9} zstd", name.escape_debug());
        }
    }
    let Some(verified) = verified else {
        return Ok(());
    };
    let details = &verified.verification;
    let manifest_signature = &details.signatures.manifest;
    let components = &details.signatures.native.components;
    println!("\nVerification");
    println!("  Signatures");
    println!(
        "    ✓ manifest ({} signer {})",
        kind(manifest_signature.role),
        hash(&manifest_signature.certificate_fingerprint, args.verbose)
    );
    for component in components {
        let id = component.id.escape_debug();
        println!(
            "    ✓ {id} image (signer {})",
            hash(
                &component.image.signature.certificate_fingerprint,
                args.verbose
            )
        );
        if component.uki.is_some() {
            println!("    ✓ {id} UKI Authenticode");
        }
    }
    println!("  Integrity");
    println!("    ✓ BLAKE3 transport");
    for component in components {
        let id = component.id.escape_debug();
        println!("    ✓ {id} root hash binding");
        println!("    ✓ {id} dm-verity tree");
    }
    println!("  Publisher trust");
    match details.publisher_trust {
        PublisherTrust::Trusted => println!("    ✓ signing certificates match configured trust"),
        PublisherTrust::NotChecked { .. } => {
            println!("    · not checked (add --trust DIR to verify the publisher)");
        }
    }
    Ok(())
}

fn kind(kind: ComponentType) -> &'static str {
    match kind {
        ComponentType::App => "app",
        ComponentType::Os => "OS",
    }
}

fn hash(value: &str, verbose: bool) -> &str {
    if verbose {
        value
    } else {
        &value[..12.min(value.len())]
    }
}

fn size(bytes: u64) -> String {
    let (divisor, unit) = match bytes {
        0..1024 => return format!("{bytes} B"),
        1024..1_048_576 => (1024.0, "KiB"),
        1_048_576..1_073_741_824 => (1_048_576.0, "MiB"),
        _ => (1_073_741_824.0, "GiB"),
    };
    format!("{:.1} {unit}", bytes as f64 / divisor)
}

use anyhow::{ensure, Context, Result};
use clap::Args;
use std::{fs::File, path::PathBuf};
use teka_pkg::ComponentType;

#[derive(Clone, Debug, Args)]
pub struct InspectOptions {
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    pub file: PathBuf,
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
    let manifest_bytes = teka_pkg::inspect_manifest(&mut file)?;
    if args.json {
        let report = serde_json::json!({
            "manifest": serde_json::from_slice::<serde_json::Value>(&manifest_bytes)?
        });
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let manifest = teka_pkg::Manifest::parse(&manifest_bytes)?;
    for (index, component) in manifest.components.iter().enumerate() {
        if index > 0 {
            println!();
        }
        let kind = match component.kind {
            ComponentType::App => "app",
            ComponentType::Os => "OS",
        };
        println!(
            "{} {} ({kind}, {})",
            component.id.escape_debug(),
            component.version.escape_debug(),
            manifest.arch.as_str()
        );
        println!("root {}", hash(&component.root_hash, args.verbose));
        let rows = std::iter::once(&component.image)
            .chain(component.uki.iter())
            .map(|file| (file.path.clone(), size(file.size)))
            .collect::<Vec<_>>();
        let name_width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0);
        for (name, stored) in rows {
            println!("- {:<name_width$}  {stored:>9} zstd", name.escape_debug());
        }
    }
    Ok(())
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

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use isarmg_upgrade::{BackupManifest, Product};

#[derive(Debug, Parser)]
#[command(name = "isarmg-upgrade", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the persistent-resource contract owned by the offline tool.
    Catalog {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Parse and strictly validate a backup manifest without touching data.
    InspectManifest {
        #[arg(value_name = "MANIFEST")]
        manifest: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Catalog { json } => print_catalog(json),
        Command::InspectManifest { manifest } => {
            let manifest = BackupManifest::read(&manifest)
                .with_context(|| format!("inspect {}", manifest.display()))?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            Ok(())
        }
    }
}

fn print_catalog(json: bool) -> anyhow::Result<()> {
    if json {
        let catalog: Vec<_> = Product::ALL
            .into_iter()
            .map(|product| {
                let contract = product.contract();
                serde_json::json!({
                    "product": product,
                    "runtime_state": contract.has_runtime_state,
                    "resources": contract.resources,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&catalog)?);
    } else {
        for product in Product::ALL {
            let contract = product.contract();
            let resources = contract
                .resources
                .iter()
                .map(|resource| format!("{resource:?}").to_lowercase())
                .collect::<Vec<_>>()
                .join(",");
            println!("{product}\t{resources}");
        }
    }
    Ok(())
}

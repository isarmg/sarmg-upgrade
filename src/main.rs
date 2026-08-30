use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use isarmg_upgrade::{
    BackupManifest, Product, RecoveryAction, RestoreExisting, create_sqlite_backup,
    recover_sqlite_restore, restore_sqlite_backup, verify_sqlite_backup,
};

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
    /// Create a current-format backup for a SQLite-only product.
    BackupSqlite {
        #[arg(long)]
        product: Product,
        #[arg(long, value_name = "DATABASE")]
        database: PathBuf,
        #[arg(long, value_name = "NEW_DIRECTORY")]
        output: PathBuf,
    },
    /// Verify an immutable SQLite-only backup set.
    VerifySqlite {
        #[arg(value_name = "BACKUP_DIRECTORY")]
        input: PathBuf,
    },
    /// Restore one SQLite-only backup under an exclusive maintenance lock.
    RestoreSqlite {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        expect_version: String,
        #[arg(long, value_name = "BACKUP_DIRECTORY")]
        input: PathBuf,
        #[arg(long, value_name = "DATABASE")]
        database: PathBuf,
        #[arg(long)]
        replace_existing: bool,
    },
    /// Finish or roll back an interrupted SQLite restore journal.
    RecoverSqlite {
        #[arg(long, value_name = "RECOVERY_DIRECTORY")]
        recovery: PathBuf,
        #[arg(long)]
        action: RecoveryAction,
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
        Command::BackupSqlite {
            product,
            database,
            output,
        } => {
            let backup = create_sqlite_backup(product, &database, &output)?;
            println!("{}", serde_json::to_string_pretty(&backup.manifest)?);
            Ok(())
        }
        Command::VerifySqlite { input } => {
            let backup = verify_sqlite_backup(&input)?;
            println!("{}", serde_json::to_string_pretty(&backup.manifest)?);
            Ok(())
        }
        Command::RestoreSqlite {
            product,
            expect_version,
            input,
            database,
            replace_existing,
        } => {
            let existing = if replace_existing {
                RestoreExisting::Replace
            } else {
                RestoreExisting::Refuse
            };
            let result =
                restore_sqlite_backup(product, &expect_version, &input, &database, existing)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Command::RecoverSqlite { recovery, action } => {
            let result = recover_sqlite_restore(&recovery, action)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
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

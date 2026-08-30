use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use isarmg_upgrade::{
    BackupManifest, DufsRecoveryOptions, DufsTreeBudget, DufsUpgradeOptions, Product,
    RecoveryAction, RestoreExisting, SentinelRecoveryOptions, SentinelUpgradeOptions,
    create_sqlite_backup, recover_dufs_upgrade, recover_sentinel_upgrade, recover_sqlite_restore,
    restore_sqlite_backup, sentinel_credentials_key_from_file, upgrade_dufs, upgrade_sentinel,
    upgrade_sqlite, verify_dufs_source_backup, verify_sentinel_source_backup, verify_source_backup,
    verify_sqlite_backup,
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
        #[arg(long)]
        product: Product,
        #[arg(long, value_name = "BACKUP_DIRECTORY")]
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
        #[arg(long)]
        product: Product,
        #[arg(long)]
        expect_version: String,
        #[arg(long, value_name = "RECOVERY_DIRECTORY")]
        recovery: PathBuf,
        #[arg(long)]
        action: RecoveryAction,
    },
    /// Run one explicitly selected, exact SQLite version adapter offline.
    UpgradeSqlite {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        from_version: String,
        #[arg(long)]
        to_version: String,
        #[arg(long, value_name = "DATABASE")]
        database: PathBuf,
        #[arg(long, value_name = "NEW_SOURCE_BACKUP_DIRECTORY")]
        backup_output: PathBuf,
    },
    /// Verify the old-generation backup for one exact SQLite adapter.
    VerifySourceBackup {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        from_version: String,
        #[arg(long)]
        to_version: String,
        #[arg(long, value_name = "SOURCE_BACKUP_DIRECTORY")]
        input: PathBuf,
    },
    /// Upgrade the exact Sentinel SQLite, MediaMTX, and recording generation offline.
    UpgradeSentinel {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        from_version: String,
        #[arg(long)]
        to_version: String,
        #[arg(long, value_name = "DATABASE")]
        database: PathBuf,
        #[arg(long, value_name = "NEW_COMPOSITE_BACKUP_DIRECTORY")]
        backup_output: PathBuf,
        #[arg(long, value_name = "RUNTIME_DIRECTORY")]
        runtime_directory: PathBuf,
        #[arg(long, value_name = "MEDIAMTX_CONFIG")]
        mediamtx_config: PathBuf,
        #[arg(long, value_name = "MEDIAMTX_CONTRACT")]
        mediamtx_contract: PathBuf,
        #[arg(long, value_name = "RECORDINGS_DIRECTORY")]
        recordings_directory: PathBuf,
        #[arg(long, value_name = "BASE64_KEY_FILE")]
        credentials_key_file: PathBuf,
    },
    /// Verify an immutable Sentinel old-generation composite backup.
    VerifySentinelSourceBackup {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        from_version: String,
        #[arg(long)]
        to_version: String,
        #[arg(long, value_name = "SOURCE_BACKUP_DIRECTORY")]
        input: PathBuf,
        #[arg(long, value_name = "BASE64_KEY_FILE")]
        credentials_key_file: PathBuf,
    },
    /// Finish or roll back an interrupted exact Sentinel upgrade.
    RecoverSentinelUpgrade {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        from_version: String,
        #[arg(long)]
        to_version: String,
        #[arg(long, value_name = "DATABASE")]
        database: PathBuf,
        #[arg(long, value_name = "RUNTIME_DIRECTORY")]
        runtime_directory: PathBuf,
        #[arg(long, value_name = "RECOVERY_DIRECTORY")]
        recovery: PathBuf,
        #[arg(
            long,
            value_name = "BASE64_KEY_FILE",
            required_if_eq("action", "commit")
        )]
        credentials_key_file: Option<PathBuf>,
        #[arg(long)]
        action: RecoveryAction,
    },
    /// Upgrade the exact Dufs SQLite/config/shared-tree generation offline.
    UpgradeDufs {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        from_version: String,
        #[arg(long)]
        to_version: String,
        #[arg(long, value_name = "STATE_SQLITE3")]
        database: PathBuf,
        #[arg(long, value_name = "NEW_COMPOSITE_BACKUP_DIRECTORY")]
        backup_output: PathBuf,
        #[arg(long, value_name = "PROTECTED_YAML")]
        config: PathBuf,
        #[arg(long, value_name = "SHARED_ROOT")]
        shared_root: PathBuf,
        #[arg(long, value_name = "STATE_DIRECTORY")]
        state_dir: PathBuf,
        #[arg(long)]
        service_uid: u32,
        #[arg(long)]
        service_gid: u32,
        #[arg(long)]
        max_tree_entries: u64,
        #[arg(long)]
        max_tree_logical_bytes: u64,
        #[arg(long)]
        max_tree_backup_bytes: u64,
        #[arg(long)]
        max_entries_per_directory: u64,
    },
    /// Verify an immutable Dufs v0.49.7 composite source backup.
    VerifyDufsSourceBackup {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        from_version: String,
        #[arg(long)]
        to_version: String,
        #[arg(long, value_name = "SOURCE_BACKUP_DIRECTORY")]
        input: PathBuf,
        #[arg(long, value_name = "PROTECTED_YAML")]
        config: PathBuf,
        #[arg(long, value_name = "SHARED_ROOT")]
        shared_root: PathBuf,
        #[arg(long)]
        service_uid: u32,
        #[arg(long)]
        service_gid: u32,
    },
    /// Finish or roll back an interrupted exact Dufs composite upgrade.
    RecoverDufsUpgrade {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        from_version: String,
        #[arg(long)]
        to_version: String,
        #[arg(long, value_name = "STATE_SQLITE3")]
        database: PathBuf,
        #[arg(long, value_name = "PROTECTED_YAML")]
        config: PathBuf,
        #[arg(long, value_name = "SHARED_ROOT")]
        shared_root: PathBuf,
        #[arg(long, value_name = "STATE_DIRECTORY")]
        state_dir: PathBuf,
        #[arg(long)]
        service_uid: u32,
        #[arg(long)]
        service_gid: u32,
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
        Command::VerifySqlite { product, input } => {
            let backup = verify_sqlite_backup(product, &input)?;
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
        Command::RecoverSqlite {
            product,
            expect_version,
            recovery,
            action,
        } => {
            let result = recover_sqlite_restore(product, &expect_version, &recovery, action)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Command::UpgradeSqlite {
            product,
            from_version,
            to_version,
            database,
            backup_output,
        } => {
            let result = upgrade_sqlite(
                product,
                &from_version,
                &to_version,
                &database,
                &backup_output,
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Command::VerifySourceBackup {
            product,
            from_version,
            to_version,
            input,
        } => {
            let backup = verify_source_backup(product, &from_version, &to_version, &input)?;
            println!("{}", serde_json::to_string_pretty(&backup.manifest)?);
            Ok(())
        }
        Command::UpgradeSentinel {
            product,
            from_version,
            to_version,
            database,
            backup_output,
            runtime_directory,
            mediamtx_config,
            mediamtx_contract,
            recordings_directory,
            credentials_key_file,
        } => {
            let credentials_key = sentinel_credentials_key_from_file(&credentials_key_file)?;
            let result = upgrade_sentinel(&SentinelUpgradeOptions {
                product,
                from_version,
                to_version,
                database,
                backup_output,
                runtime_directory,
                mediamtx_config,
                mediamtx_contract,
                recordings_directory,
                credentials_key,
            })?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Command::VerifySentinelSourceBackup {
            product,
            from_version,
            to_version,
            input,
            credentials_key_file,
        } => {
            let credentials_key = sentinel_credentials_key_from_file(&credentials_key_file)?;
            let backup = verify_sentinel_source_backup(
                product,
                &from_version,
                &to_version,
                &input,
                &credentials_key,
            )?;
            println!("{}", serde_json::to_string_pretty(&backup.manifest)?);
            Ok(())
        }
        Command::RecoverSentinelUpgrade {
            product,
            from_version,
            to_version,
            database,
            runtime_directory,
            recovery,
            credentials_key_file,
            action,
        } => {
            let credentials_key = credentials_key_file
                .as_deref()
                .map(sentinel_credentials_key_from_file)
                .transpose()?;
            let result = recover_sentinel_upgrade(&SentinelRecoveryOptions {
                product,
                from_version,
                to_version,
                database,
                runtime_directory,
                recovery_directory: recovery,
                action,
                credentials_key,
            })?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Command::UpgradeDufs {
            product,
            from_version,
            to_version,
            database,
            backup_output,
            config,
            shared_root,
            state_dir,
            service_uid,
            service_gid,
            max_tree_entries,
            max_tree_logical_bytes,
            max_tree_backup_bytes,
            max_entries_per_directory,
        } => {
            let result = upgrade_dufs(&DufsUpgradeOptions {
                product,
                from_version,
                to_version,
                database,
                backup_output,
                config,
                shared_root,
                state_dir,
                service_uid,
                service_gid,
                tree_budget: DufsTreeBudget {
                    max_entries: max_tree_entries,
                    max_logical_bytes: max_tree_logical_bytes,
                    max_backup_bytes: max_tree_backup_bytes,
                    max_entries_per_directory,
                },
            })?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Command::VerifyDufsSourceBackup {
            product,
            from_version,
            to_version,
            input,
            config,
            shared_root,
            service_uid,
            service_gid,
        } => {
            let backup = verify_dufs_source_backup(
                product,
                &from_version,
                &to_version,
                &input,
                &config,
                &shared_root,
                service_uid,
                service_gid,
            )?;
            println!("{}", serde_json::to_string_pretty(&backup.manifest)?);
            Ok(())
        }
        Command::RecoverDufsUpgrade {
            product,
            from_version,
            to_version,
            database,
            config,
            shared_root,
            state_dir,
            service_uid,
            service_gid,
            recovery,
            action,
        } => {
            let result = recover_dufs_upgrade(&DufsRecoveryOptions {
                product,
                from_version,
                to_version,
                database,
                config,
                shared_root,
                state_dir,
                service_uid,
                service_gid,
                recovery_directory: recovery,
                action,
            })?;
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
                    "requires_external_credentials_key": contract.requires_external_credentials_key,
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
            let credentials = if contract.requires_external_credentials_key {
                "\texternal-credentials-key=required"
            } else {
                ""
            };
            println!("{product}\t{resources}{credentials}");
        }
    }
    Ok(())
}

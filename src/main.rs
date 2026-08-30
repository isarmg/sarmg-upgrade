use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use isarmg_upgrade::{
    BackupManifest, CompositeCurrentOptions, CurrentRecoveryAction, CurrentRecoveryOptions,
    CurrentRestoreOptions, DufsCurrentOptions, DufsCurrentRestoreOptions, DufsRecoveryOptions,
    DufsTreeBudget, DufsUpgradeOptions, NamedFile, Product, RecoveryAction, RestoreExisting,
    SentinelRecoveryOptions, SentinelUpgradeOptions, backup_current, backup_dufs_current,
    create_sqlite_backup, create_sqlite_backup_with_credentials, recover_current,
    recover_dufs_upgrade, recover_sentinel_upgrade, recover_sqlite_restore, restore_current,
    restore_dufs_current, restore_sqlite_backup, restore_sqlite_backup_with_credentials,
    sentinel_credentials_key_from_file, support_matrix, upgrade_dufs, upgrade_sentinel,
    upgrade_sqlite, verify_current_backup, verify_dufs_current_backup, verify_dufs_source_backup,
    verify_sentinel_source_backup, verify_source_backup, verify_sqlite_backup,
    verify_sqlite_backup_with_credentials,
};

#[derive(Debug, Parser)]
#[command(name = "isarmg-upgrade", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print only capabilities implemented by this exact binary.
    Support {
        /// Emit the stable machine-readable representation.
        #[arg(long)]
        json: bool,
    },
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
    /// Back up the exact current Photo SQLite and DATA_DIR generation.
    BackupPhoto {
        #[arg(long, value_name = "DATABASE")]
        database: PathBuf,
        #[arg(long, value_name = "DATA_DIR")]
        data_dir: PathBuf,
        #[arg(long, value_name = "NEW_DIRECTORY")]
        output: PathBuf,
    },
    /// Verify an exact current Photo composite backup.
    VerifyPhotoBackup {
        #[arg(long, value_name = "BACKUP_DIRECTORY")]
        input: PathBuf,
    },
    /// Restore the exact current Photo SQLite and DATA_DIR generation.
    RestorePhoto {
        #[arg(long, value_name = "BACKUP_DIRECTORY")]
        input: PathBuf,
        #[arg(long, value_name = "DATABASE")]
        database: PathBuf,
        #[arg(long, value_name = "DATA_DIR")]
        data_dir: PathBuf,
        #[arg(long)]
        replace_existing: bool,
    },
    /// Commit or roll back an interrupted Photo current restore.
    RecoverPhotoRestore {
        #[arg(long, value_name = "RECOVERY_DIRECTORY")]
        recovery: PathBuf,
        #[arg(long)]
        action: CurrentRecoveryAction,
    },
    /// Back up the exact current Sentinel composite generation.
    BackupSentinelCurrent {
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        runtime_directory: PathBuf,
        #[arg(long)]
        mediamtx_config: PathBuf,
        #[arg(long)]
        mediamtx_contract: PathBuf,
        #[arg(long)]
        recordings_directory: PathBuf,
        #[arg(long)]
        credentials_key_id: String,
        #[arg(long)]
        credentials_key_file: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Verify the exact current Sentinel composite backup.
    VerifySentinelCurrent {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        credentials_key_id: String,
        #[arg(long)]
        credentials_key_file: PathBuf,
    },
    /// Restore the exact current Sentinel composite generation.
    RestoreSentinelCurrent {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        runtime_directory: PathBuf,
        #[arg(long)]
        mediamtx_config: PathBuf,
        #[arg(long)]
        mediamtx_contract: PathBuf,
        #[arg(long)]
        recordings_directory: PathBuf,
        #[arg(long)]
        credentials_key_id: String,
        #[arg(long)]
        credentials_key_file: PathBuf,
        #[arg(long)]
        replace_existing: bool,
    },
    /// Commit or roll back an interrupted Sentinel current restore.
    RecoverSentinelCurrent {
        #[arg(long)]
        recovery: PathBuf,
        #[arg(long)]
        credentials_key_id: String,
        #[arg(long)]
        credentials_key_file: PathBuf,
        #[arg(long)]
        action: CurrentRecoveryAction,
    },
    /// Back up the exact current Dufs SQLite, protected config, and shared root.
    BackupDufsCurrent {
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        shared_root: PathBuf,
        #[arg(long)]
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
    /// Verify the exact current Dufs composite backup.
    VerifyDufsCurrent {
        #[arg(long)]
        input: PathBuf,
    },
    /// Restore an exact current Dufs backup into a missing DB and empty shared root.
    RestoreDufsCurrent {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        shared_root: PathBuf,
        #[arg(long)]
        state_dir: PathBuf,
        #[arg(long)]
        service_uid: u32,
        #[arg(long)]
        service_gid: u32,
        #[arg(long)]
        replace_config: bool,
    },
    /// Create a current-format backup for a SQLite-only product.
    BackupSqlite {
        #[arg(long)]
        product: Product,
        #[arg(long, value_name = "DATABASE")]
        database: PathBuf,
        #[arg(long, value_name = "NEW_DIRECTORY")]
        output: PathBuf,
        #[arg(long, value_name = "KEY_ID")]
        credentials_key_id: Option<String>,
        #[arg(long, value_name = "BASE64_KEY_FILE")]
        credentials_key_file: Option<PathBuf>,
    },
    /// Verify an immutable SQLite-only backup set.
    VerifySqlite {
        #[arg(long)]
        product: Product,
        #[arg(long, value_name = "BACKUP_DIRECTORY")]
        input: PathBuf,
        #[arg(long, value_name = "KEY_ID")]
        credentials_key_id: Option<String>,
        #[arg(long, value_name = "BASE64_KEY_FILE")]
        credentials_key_file: Option<PathBuf>,
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
        #[arg(long, value_name = "KEY_ID")]
        credentials_key_id: Option<String>,
        #[arg(long, value_name = "BASE64_KEY_FILE")]
        credentials_key_file: Option<PathBuf>,
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
        Command::Support { json } => print_support(json),
        Command::Catalog { json } => print_catalog(json),
        Command::InspectManifest { manifest } => {
            let manifest = BackupManifest::read(&manifest)
                .with_context(|| format!("inspect {}", manifest.display()))?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            Ok(())
        }
        Command::BackupPhoto {
            database,
            data_dir,
            output,
        } => print_current(backup_current(&photo_current(database, data_dir, output))?),
        Command::VerifyPhotoBackup { input } => print_current(verify_current_backup(
            &photo_current(input.join("database.sqlite3"), input.join("tree"), input),
        )?),
        Command::RestorePhoto {
            input,
            database,
            data_dir,
            replace_existing,
        } => print_current(restore_current(&CurrentRestoreOptions {
            product: Product::PhotoBackup,
            input,
            database,
            tree: data_dir,
            runtime_directory: None,
            configuration: Vec::new(),
            replace_existing,
            credentials_key_id: None,
            credentials_key: None,
        })?),
        Command::RecoverPhotoRestore { recovery, action } => {
            print_current(recover_current(&CurrentRecoveryOptions {
                recovery_directory: recovery,
                action,
                credentials_key_id: None,
                credentials_key: None,
            })?)
        }
        Command::BackupSentinelCurrent {
            database,
            runtime_directory,
            mediamtx_config,
            mediamtx_contract,
            recordings_directory,
            credentials_key_id,
            credentials_key_file,
            output,
        } => {
            let key = sentinel_credentials_key_from_file(&credentials_key_file)?;
            print_current(backup_current(&sentinel_current(
                database,
                runtime_directory,
                mediamtx_config,
                mediamtx_contract,
                recordings_directory,
                credentials_key_id,
                key,
                output,
            ))?)
        }
        Command::VerifySentinelCurrent {
            input,
            credentials_key_id,
            credentials_key_file,
        } => {
            let key = sentinel_credentials_key_from_file(&credentials_key_file)?;
            print_current(verify_current_backup(&sentinel_current(
                input.join("database.sqlite3"),
                input.join("runtime-unused"),
                input.join("mediamtx.yml"),
                input.join("mediamtx.lock"),
                input.join("tree"),
                credentials_key_id,
                key,
                input,
            ))?)
        }
        Command::RestoreSentinelCurrent {
            input,
            database,
            runtime_directory,
            mediamtx_config,
            mediamtx_contract,
            recordings_directory,
            credentials_key_id,
            credentials_key_file,
            replace_existing,
        } => {
            let key = sentinel_credentials_key_from_file(&credentials_key_file)?;
            print_current(restore_current(&CurrentRestoreOptions {
                product: Product::SentinelMonitor,
                input,
                database,
                tree: recordings_directory,
                runtime_directory: Some(runtime_directory),
                configuration: vec![
                    NamedFile {
                        name: "mediamtx.yml".to_owned(),
                        path: mediamtx_config,
                    },
                    NamedFile {
                        name: "mediamtx.lock".to_owned(),
                        path: mediamtx_contract,
                    },
                ],
                replace_existing,
                credentials_key_id: Some(credentials_key_id),
                credentials_key: Some(key),
            })?)
        }
        Command::RecoverSentinelCurrent {
            recovery,
            credentials_key_id,
            credentials_key_file,
            action,
        } => {
            let key = sentinel_credentials_key_from_file(&credentials_key_file)?;
            print_current(recover_current(&CurrentRecoveryOptions {
                recovery_directory: recovery,
                action,
                credentials_key_id: Some(credentials_key_id),
                credentials_key: Some(key),
            })?)
        }
        Command::BackupDufsCurrent {
            database,
            output,
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
            let result = backup_dufs_current(&DufsCurrentOptions {
                database,
                output,
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
            println!("{}", serde_json::to_string_pretty(&result.manifest)?);
            Ok(())
        }
        Command::VerifyDufsCurrent { input } => {
            let result = verify_dufs_current_backup(&input)?;
            println!("{}", serde_json::to_string_pretty(&result.manifest)?);
            Ok(())
        }
        Command::RestoreDufsCurrent {
            input,
            database,
            config,
            shared_root,
            state_dir,
            service_uid,
            service_gid,
            replace_config,
        } => {
            let result = restore_dufs_current(&DufsCurrentRestoreOptions {
                input,
                database,
                config,
                shared_root,
                state_dir,
                service_uid,
                service_gid,
                replace_config,
            })?;
            println!("{}", serde_json::to_string_pretty(&result.manifest)?);
            Ok(())
        }
        Command::BackupSqlite {
            product,
            database,
            output,
            credentials_key_id,
            credentials_key_file,
        } => {
            let credentials =
                sqlite_credentials(product, credentials_key_id, credentials_key_file)?;
            let backup = match credentials.as_ref() {
                Some((key_id, key)) => {
                    create_sqlite_backup_with_credentials(product, &database, &output, key_id, key)?
                }
                None => create_sqlite_backup(product, &database, &output)?,
            };
            println!("{}", serde_json::to_string_pretty(&backup.manifest)?);
            Ok(())
        }
        Command::VerifySqlite {
            product,
            input,
            credentials_key_id,
            credentials_key_file,
        } => {
            let credentials =
                sqlite_credentials(product, credentials_key_id, credentials_key_file)?;
            let backup = match credentials.as_ref() {
                Some((key_id, key)) => {
                    verify_sqlite_backup_with_credentials(product, &input, key_id, key)?
                }
                None => verify_sqlite_backup(product, &input)?,
            };
            println!("{}", serde_json::to_string_pretty(&backup.manifest)?);
            Ok(())
        }
        Command::RestoreSqlite {
            product,
            expect_version,
            input,
            database,
            replace_existing,
            credentials_key_id,
            credentials_key_file,
        } => {
            let existing = if replace_existing {
                RestoreExisting::Replace
            } else {
                RestoreExisting::Refuse
            };
            let credentials =
                sqlite_credentials(product, credentials_key_id, credentials_key_file)?;
            let result = match credentials.as_ref() {
                Some((key_id, key)) => restore_sqlite_backup_with_credentials(
                    product,
                    &expect_version,
                    &input,
                    &database,
                    existing,
                    key_id,
                    key,
                )?,
                None => {
                    restore_sqlite_backup(product, &expect_version, &input, &database, existing)?
                }
            };
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

fn sqlite_credentials(
    product: Product,
    key_id: Option<String>,
    key_file: Option<PathBuf>,
) -> anyhow::Result<Option<(String, [u8; 32])>> {
    match (product, key_id, key_file) {
        (Product::SunshineManager, Some(key_id), Some(key_file)) => Ok(Some((
            key_id,
            sentinel_credentials_key_from_file(&key_file)?,
        ))),
        (Product::SunshineManager, _, _) => anyhow::bail!(
            "sunshine-manager requires both --credentials-key-id and --credentials-key-file"
        ),
        (_, None, None) => Ok(None),
        _ => anyhow::bail!("credentials key options are only valid for sunshine-manager"),
    }
}

fn print_support(json: bool) -> anyhow::Result<()> {
    let matrix = support_matrix();
    if json {
        println!("{}", serde_json::to_string_pretty(&matrix)?);
    } else {
        for product in matrix.products {
            let current = product.current_state.backup.join(",");
            let edges = product
                .upgrade_edges
                .iter()
                .map(|edge| format!("{}->{}", edge.from, edge.to))
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "{}\tcurrent={}\tupgrade={}",
                product.product, current, edges
            );
        }
    }
    Ok(())
}

fn photo_current(database: PathBuf, data_dir: PathBuf, output: PathBuf) -> CompositeCurrentOptions {
    CompositeCurrentOptions {
        product: Product::PhotoBackup,
        database,
        tree: data_dir,
        output,
        runtime_directory: None,
        configuration: Vec::new(),
        credentials_key_id: None,
        credentials_key: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn sentinel_current(
    database: PathBuf,
    runtime_directory: PathBuf,
    mediamtx_config: PathBuf,
    mediamtx_contract: PathBuf,
    recordings_directory: PathBuf,
    credentials_key_id: String,
    credentials_key: [u8; 32],
    output: PathBuf,
) -> CompositeCurrentOptions {
    CompositeCurrentOptions {
        product: Product::SentinelMonitor,
        database,
        tree: recordings_directory,
        output,
        runtime_directory: Some(runtime_directory),
        configuration: vec![
            NamedFile {
                name: "mediamtx.yml".to_owned(),
                path: mediamtx_config,
            },
            NamedFile {
                name: "mediamtx.lock".to_owned(),
                path: mediamtx_contract,
            },
        ],
        credentials_key_id: Some(credentials_key_id),
        credentials_key: Some(credentials_key),
    }
}

fn print_current(result: isarmg_upgrade::CurrentStateResult) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
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

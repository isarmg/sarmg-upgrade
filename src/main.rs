use std::path::PathBuf;

use anyhow::{Context, ensure};
use clap::{Parser, Subcommand};
use sarmg_upgrade::{
    BackupManifest, CompositeCurrentOptions, CurrentRecoveryAction, CurrentRecoveryOptions,
    CurrentRestoreOptions, Product, RecoveryAction, RestoreExisting, backup_current,
    create_sqlite_backup, create_sqlite_backup_with_credentials, credentials_key_from_file,
    recover_current, recover_sqlite_restore, restore_current, restore_sqlite_backup,
    restore_sqlite_backup_with_credentials, support_matrix, verify_current_backup,
    verify_sqlite_backup, verify_sqlite_backup_with_credentials,
};

#[derive(Debug, Parser)]
#[command(name = "sarmg-upgrade", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// 开发阶段只暴露当前版本的备份、校验与恢复命令。
/// 历史升级边会在稳定格式形成后以独立、可审计的适配器重新加入。
#[derive(Debug, Subcommand)]
enum Command {
    Support {
        #[arg(long)]
        json: bool,
    },
    Catalog {
        #[arg(long)]
        json: bool,
    },
    InspectManifest {
        manifest: PathBuf,
    },
    BackupMedia {
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    VerifyMediaBackup {
        #[arg(long)]
        input: PathBuf,
    },
    RestoreMedia {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        replace_existing: bool,
    },
    RecoverMediaRestore {
        #[arg(long)]
        recovery: PathBuf,
        #[arg(long)]
        action: CurrentRecoveryAction,
    },
    BackupSqlite {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        credentials_key_id: Option<String>,
        #[arg(long)]
        credentials_key_file: Option<PathBuf>,
    },
    VerifySqlite {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        credentials_key_id: Option<String>,
        #[arg(long)]
        credentials_key_file: Option<PathBuf>,
    },
    RestoreSqlite {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        expect_version: String,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        replace_existing: bool,
        #[arg(long)]
        credentials_key_id: Option<String>,
        #[arg(long)]
        credentials_key_file: Option<PathBuf>,
    },
    RecoverSqlite {
        #[arg(long)]
        product: Product,
        #[arg(long)]
        expect_version: String,
        #[arg(long)]
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
            let parsed = BackupManifest::read(&manifest)
                .with_context(|| format!("inspect {}", manifest.display()))?;
            println!("{}", serde_json::to_string_pretty(&parsed)?);
            Ok(())
        }
        Command::BackupMedia {
            database,
            data_dir,
            output,
        } => print_current(backup_current(&media_options(database, data_dir, output))?),
        Command::VerifyMediaBackup { input } => print_current(verify_current_backup(
            &media_options(input.join("database.sqlite3"), input.join("tree"), input),
        )?),
        Command::RestoreMedia {
            input,
            database,
            data_dir,
            replace_existing,
        } => print_current(restore_current(&CurrentRestoreOptions {
            product: Product::MediaBackup,
            input,
            database,
            tree: data_dir,
            runtime_directory: None,
            configuration: Vec::new(),
            replace_existing,
            credentials_key_id: None,
            credentials_key: None,
        })?),
        Command::RecoverMediaRestore { recovery, action } => {
            print_current(recover_current(&CurrentRecoveryOptions {
                recovery_directory: recovery,
                action,
                credentials_key_id: None,
                credentials_key: None,
            })?)
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
            ensure!(
                product == Product::HostMonitoring,
                "recover-sqlite is currently supported only for host-monitoring"
            );
            let result = recover_sqlite_restore(product, &expect_version, &recovery, action)?;
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
        (Product::SunshineManager, Some(key_id), Some(key_file)) => {
            Ok(Some((key_id, credentials_key_from_file(&key_file)?)))
        }
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
            println!(
                "{}\tbackup={}\tverify={}\trestore={}\trecover={}",
                product.product,
                product.current_state.backup.join(","),
                product.current_state.verify.join(","),
                product.current_state.restore.join(","),
                product.current_state.recover.join(",")
            );
        }
    }
    Ok(())
}

fn media_options(database: PathBuf, data_dir: PathBuf, output: PathBuf) -> CompositeCurrentOptions {
    CompositeCurrentOptions {
        product: Product::MediaBackup,
        database,
        tree: data_dir,
        output,
        runtime_directory: None,
        configuration: Vec::new(),
        credentials_key_id: None,
        credentials_key: None,
    }
}

fn print_current(result: sarmg_upgrade::CurrentStateResult) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn print_catalog(json: bool) -> anyhow::Result<()> {
    let entries = Product::ALL
        .into_iter()
        .map(Product::contract)
        .collect::<Vec<_>>();
    if json {
        let output = entries
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "product": entry.product,
                    "resources": entry.resources,
                    "has_runtime_state": entry.has_runtime_state,
                    "requires_external_credentials_key": entry.requires_external_credentials_key,
                })
            })
            .collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for entry in entries {
            println!(
                "{}\tresources={}\truntime={}\texternal-key={}",
                entry.product,
                entry
                    .resources
                    .iter()
                    .map(|resource| format!("{resource:?}").to_lowercase())
                    .collect::<Vec<_>>()
                    .join(","),
                entry.has_runtime_state,
                entry.requires_external_credentials_key
            );
        }
    }
    Ok(())
}

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use xssc::{Product, support_matrix};

#[derive(Debug, Parser)]
#[command(name = "xssc", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Upgrade current managed releases and recover the complete protected group.
#[derive(Debug, Subcommand)]
enum Command {
    /// Back up xscs 0.15.0 and invalidate only its v3 observation cache offline.
    PrepareXscsProtocol {
        #[arg(long)]
        plan: PathBuf,
    },
    /// Upgrade a signed release after the operator has stopped every state writer.
    ApplyUpgrade {
        #[arg(long)]
        plan: PathBuf,
    },
    /// Inspect durable phases and verify completed backups.
    InspectUpgrade {
        #[arg(long)]
        work_directory: PathBuf,
    },
    /// Recover the original release, configuration and persistent data together.
    RecoverUpgrade {
        #[arg(long)]
        work_directory: PathBuf,
        /// Authorize discarding writes after a program received run ownership.
        #[arg(long)]
        allow_data_loss: bool,
    },
    Support {
        #[arg(long)]
        json: bool,
    },
    Catalog {
        #[arg(long)]
        json: bool,
    },
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let value = match error.downcast_ref::<xssc::upgrade::UpgradeFailure>() {
                Some(error) => serde_json::json!({"error": error}),
                None => {
                    serde_json::json!({"error": {"code": "MAINTENANCE_FAILED", "message": "离线维护失败；请检查当前操作契约、文件权限和支持范围。"}})
                }
            };
            println!("{value}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::PrepareXscsProtocol { plan } => {
            let result = xssc::upgrade::xscs_preparation::prepare_from_file(&plan)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Command::ApplyUpgrade { plan } => {
            let plan = xssc::upgrade::read_plan(&plan)?;
            let result = xssc::upgrade::apply(&plan, &mut xssc::upgrade::SystemdControl)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Command::InspectUpgrade { work_directory } => {
            let result = xssc::upgrade::inspect_status(&work_directory)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Command::RecoverUpgrade {
            work_directory,
            allow_data_loss,
        } => {
            let result = xssc::upgrade::recover(
                &work_directory,
                allow_data_loss,
                &mut xssc::upgrade::SystemdControl,
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Command::Support { json } => print_support(json),
        Command::Catalog { json } => print_catalog(json),
    }
}

fn print_support(json: bool) -> anyhow::Result<()> {
    let matrix = support_matrix();
    if json {
        println!("{}", serde_json::to_string_pretty(&matrix)?);
    } else {
        for product in matrix.products {
            println!(
                "{}\tupgrade={}\trecovery={}",
                product.product, product.signed_release_upgrade, product.durable_recovery
            );
        }
    }
    Ok(())
}

fn print_catalog(json: bool) -> anyhow::Result<()> {
    let entries = Product::ALL
        .into_iter()
        .map(Product::contract)
        .collect::<Vec<_>>();
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        for entry in entries {
            println!("{}\truntime={}", entry.product, entry.has_runtime_state);
        }
    }
    Ok(())
}

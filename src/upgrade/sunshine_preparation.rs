//! One explicit offline transition for a rebuildable Sunshine observation cache.
//!
//! This is not a runtime compatibility reader or a general SQL migration hook.
//! Credentials, snapshots, tasks, operation fingerprints and audit rows stay intact.

use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, ensure};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use uuid::Uuid;
use xcss_contracts::{ReleaseIdentity, SchemaIdentity};
use xcss_state_file::PrivateStateDirectory;

use super::{
    MAX_JSON, UpgradeFailure, failure, parse_validated_state, process, snapshot,
    stopped_service_report, validate_digest, validate_service,
};

const PRODUCT: &str = "xscs";
const SOURCE_VERSION: &str = "0.15.0";
const TARGET_VERSION: &str = "0.16.0";
const REPORT: &str = "sunshine-preparation.json";
const SCHEMA_SHA: &str = "0466872562dde0c06ef73e42e683801c21cc1d7be3488ca332a5e9a3d9c0518b";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationPlan {
    pub format: u32,
    pub service: String,
    pub source_binary: PathBuf,
    /// Obtain this checksum independently of the plan and downloaded binary.
    pub source_binary_sha256: String,
    pub source_identity: ReleaseIdentity,
    pub config: PathBuf,
    pub data_dir: PathBuf,
    pub database: PathBuf,
    pub work_directory: PathBuf,
    pub max_backup_bytes: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationReport {
    pub format: u32,
    pub operation_id: Uuid,
    pub source_identity: ReleaseIdentity,
    pub target_version: String,
    pub phase: String,
    pub invalidated_devices: u64,
    plan: PreparationPlan,
    data_entries: Vec<snapshot::Entry>,
    data_ownership: Vec<snapshot::Ownership>,
    config_entries: Vec<snapshot::Entry>,
    config_ownership: Vec<snapshot::Ownership>,
}

#[derive(Clone, Copy, Debug)]
enum PreparationStage {
    Plan,
    Owner,
    Source,
    Writers,
    Backup,
    CacheWrite,
    PostCommit,
}

impl PreparationStage {
    fn public_failure(self, work: Option<&Path>) -> UpgradeFailure {
        let (code, message) = match self {
            Self::Plan => (
                "SUNSHINE_PLAN_INVALID",
                "准备计划不符合唯一受支持的源版本、路径、权限或备份预算；尚未开始缓存写入。",
            ),
            Self::Owner => (
                "SUNSHINE_OWNER_INVALID",
                "须以实际数据属主和服务组运行，且私有状态目录完整有效；尚未开始缓存写入。",
            ),
            Self::Source => (
                "SUNSHINE_SOURCE_INVALID",
                "源程序摘要、实际发行身份、当前数据身份或持久资源范围不符；尚未开始缓存写入。",
            ),
            Self::Writers => (
                "SUNSHINE_WRITER_NOT_EXCLUDED",
                "服务未确认停止、存在待恢复状态或维护锁已被占用；尚未开始缓存写入。",
            ),
            Self::Backup => (
                "SUNSHINE_BACKUP_FAILED",
                "完整备份、原组一致性或写入前准备记录未确认成功；尚未开始缓存写入，保留工作目录检查。",
            ),
            Self::CacheWrite => (
                "SUNSHINE_CACHE_STATE_UNCONFIRMED",
                "缓存事务或其完成记录未确认；缓存可能已失效。保持停服，按准备记录核验五个观察字段和备份，不能盲目重跑或恢复。",
            ),
            Self::PostCommit => (
                "SUNSHINE_POSTCOMMIT_FAILED",
                "缓存事务已提交，但文件身份或完成记录核验失败。保持停服，保留现状及备份，按恢复文档核验，不启动新服务。",
            ),
        };
        failure(code, message, work)
    }
}

pub fn prepare_from_file(path: &Path) -> Result<PreparationReport, UpgradeFailure> {
    let mut stage = PreparationStage::Plan;
    let mut work = None;
    let result = (|| -> anyhow::Result<PreparationReport> {
        ensure!(
            cfg!(target_os = "linux"),
            "offline preparation requires Linux"
        );
        let file = snapshot::open_regular(path)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.uid() == rustix::process::geteuid().as_raw()
                && metadata.mode() & 0o777 == 0o600,
            "preparation plan must be owned by the caller with mode 0600"
        );
        let plan: PreparationPlan =
            serde_json::from_slice(&snapshot::read_bounded(path, MAX_JSON)?)?;
        work = Some(plan.work_directory.clone());
        prepare_with_stop_check(&plan, require_stopped, &mut stage)
    })();
    // Product output and configuration can contain secrets: expose the stable
    // phase code, never the nested OS, JSON, SQLite or child-stderr error chain.
    result.map_err(|_| stage.public_failure(work.as_deref()))
}

fn schema_identity() -> anyhow::Result<SchemaIdentity> {
    Ok(SchemaIdentity::new(PRODUCT, "0.10.1", 7, SCHEMA_SHA)?)
}

fn validate_plan(plan: &PreparationPlan) -> anyhow::Result<()> {
    ensure!(
        plan.format == 1 && plan.max_backup_bytes > 0,
        "invalid preparation format/budget"
    );
    validate_service(&plan.service)?;
    validate_digest(&plan.source_binary_sha256)?;
    plan.source_identity.validate()?;
    ensure!(
        plan.source_identity.product == PRODUCT
            && plan.source_identity.version == SOURCE_VERSION
            && plan.source_identity.target == "x86_64-unknown-linux-gnu",
        "this transition accepts only the bound Sunshine 0.15.0 release"
    );
    for path in [
        &plan.source_binary,
        &plan.config,
        &plan.data_dir,
        &plan.database,
        &plan.work_directory,
    ] {
        snapshot::require_path(path)?;
    }
    ensure!(
        plan.database.parent() == Some(plan.data_dir.as_path()),
        "database must be a direct data child"
    );
    ensure!(
        !plan.config.starts_with(&plan.data_dir)
            && !plan.source_binary.starts_with(&plan.data_dir)
            && !plan.work_directory.starts_with(&plan.data_dir)
            && !plan.data_dir.starts_with(&plan.work_directory)
            && !plan.config.starts_with(&plan.work_directory)
            && !plan.source_binary.starts_with(&plan.work_directory),
        "work, program, configuration and state must have separate boundaries"
    );
    let config: serde_json::Value =
        serde_json::from_slice(&snapshot::read_bounded(&plan.config, MAX_JSON)?)?;
    let configured_database = if let Some(url) = config.get("database_url") {
        let url = url.as_str().context("database URL must be a string")?;
        let path = url
            .strip_prefix("sqlite://")
            .or_else(|| url.strip_prefix("sqlite:"))
            .context("plain SQLite URL required")?;
        ensure!(
            !path.contains(['?', '#', '%', '\0']),
            "plain SQLite URL required"
        );
        PathBuf::from(path)
    } else {
        plan.data_dir.join("sunshine.sqlite3")
    };
    ensure!(
        configured_database == plan.database,
        "plan database differs from effective configuration"
    );
    Ok(())
}

fn require_stopped(plan: &PreparationPlan) -> anyhow::Result<()> {
    let report = process::run_bounded(
        "/usr/bin/systemctl",
        &[
            "show".into(),
            plan.service.clone(),
            "--property=LoadState".into(),
            "--property=MainPID".into(),
            "--property=ActiveState".into(),
        ],
        30,
    )?;
    ensure!(
        stopped_service_report(&report)?,
        "the writer service must already be stopped"
    );
    Ok(())
}

fn verify_source(plan: &PreparationPlan, uid: u32, gid: u32) -> anyhow::Result<()> {
    ensure!(
        snapshot::digest_file(&plan.source_binary)? == plan.source_binary_sha256,
        "source binary checksum differs"
    );
    let identity = process::run_bounded_delegated(
        &plan.source_binary,
        &["release-identity".into(), "--json".into()],
        30,
        Some((uid, gid)),
        None,
    )?;
    let identity: ReleaseIdentity = serde_json::from_slice(&identity)?;
    ensure!(
        identity == plan.source_identity,
        "actual source release differs from the independently bound plan"
    );
    let report = process::run_bounded_delegated(
        &plan.source_binary,
        &[
            "config".into(),
            "validate".into(),
            "--config".into(),
            plan.config.to_string_lossy().into_owned(),
            "--data-dir".into(),
            plan.data_dir.to_string_lossy().into_owned(),
            "--json".into(),
        ],
        30,
        Some((uid, gid)),
        None,
    )?;
    let state = parse_validated_state(&report)?;
    ensure!(
        state.schema_identity == schema_identity()?,
        "source data identity differs"
    );
    ensure!(
        state.state_paths.contains(&plan.data_dir)
            && state.state_paths.contains(&plan.config)
            && state
                .state_paths
                .iter()
                .all(|path| path == &plan.data_dir || path == &plan.config),
        "the complete source state must be covered by configuration and data backups"
    );
    Ok(())
}

// The production entry always passes the fixed systemd check above. A fixture
// can supply only the stopped-state assertion while exercising real file,
// source-identity, backup, lock and SQLite boundaries.
fn prepare_with_stop_check(
    plan: &PreparationPlan,
    stopped: impl Fn(&PreparationPlan) -> anyhow::Result<()>,
    stage: &mut PreparationStage,
) -> anyhow::Result<PreparationReport> {
    *stage = PreparationStage::Plan;
    validate_plan(plan)?;
    // The SQLite worker must retain the service owner for any journal/WAL it
    // creates. This explicit command runs as that owner, including under sudo;
    // it never opens the original database through an administrative UID bridge.
    *stage = PreparationStage::Owner;
    let state = PrivateStateDirectory::open(&plan.data_dir)?;
    let owner = fs::symlink_metadata(&plan.data_dir)?;
    ensure!(
        rustix::process::getegid().as_raw() == owner.gid(),
        "caller must use the data directory's service group"
    );
    *stage = PreparationStage::Source;
    verify_source(plan, owner.uid(), owner.gid())?;
    *stage = PreparationStage::Writers;
    state.verify_no_pending_maintenance()?;
    stopped(plan)?;
    let _maintenance = state.try_maintenance_lock()?;
    state.verify_no_pending_maintenance()?;
    let name = plan
        .database
        .file_name()
        .context("database file name missing")?
        .to_string_lossy();
    let instance = state.create_file(format!(".{name}.xscs.instance.lock"))?;
    let maintenance = state.create_file(format!(".{name}.xscs.maintenance.lock"))?;
    for file in [&instance, &maintenance] {
        rustix::fs::flock(
            file.file(),
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        )?;
        file.verify_identity()?;
    }
    stopped(plan)?;
    state.verify_identity()?;
    *stage = PreparationStage::Source;
    verify_source(plan, owner.uid(), owner.gid())?;
    *stage = PreparationStage::Backup;
    let database = snapshot::open_regular(&plan.database)?;
    ensure!(
        database.metadata()?.uid() == owner.uid() && database.metadata()?.mode() & 0o777 == 0o600,
        "database requires its actual service owner and mode 0600"
    );

    let data_entries = snapshot::inventory(&plan.data_dir, true, plan.max_backup_bytes, true)?;
    let data_ownership = snapshot::ownership(&plan.data_dir, &data_entries)?;
    let config_owner = snapshot::open_regular(&plan.config)?.metadata()?;
    let config_entries = snapshot::inventory_owned(
        &plan.config,
        false,
        plan.max_backup_bytes,
        false,
        config_owner.uid(),
    )?;
    let config_ownership = snapshot::ownership(&plan.config, &config_entries)?;
    let total = data_entries
        .iter()
        .chain(&config_entries)
        .try_fold(0u64, |sum, entry| {
            sum.checked_add(entry.bytes).context("backup size overflow")
        })?;
    ensure!(
        total <= plan.max_backup_bytes,
        "complete backup exceeds budget"
    );
    snapshot::private_directory(&plan.work_directory)?;
    snapshot::snapshot(
        &plan.data_dir,
        &plan.work_directory.join("data"),
        &data_entries,
    )?;
    snapshot::snapshot(
        &plan.config,
        &plan.work_directory.join("config"),
        &config_entries,
    )?;
    ensure!(
        snapshot::inventory(
            &plan.work_directory.join("data"),
            true,
            plan.max_backup_bytes,
            true
        )? == data_entries,
        "data backup differs"
    );
    ensure!(
        snapshot::inventory(
            &plan.work_directory.join("config"),
            false,
            plan.max_backup_bytes,
            false
        )? == config_entries,
        "configuration backup differs"
    );
    ensure!(
        snapshot::inventory(&plan.data_dir, true, plan.max_backup_bytes, true)? == data_entries
            && snapshot::ownership(&plan.data_dir, &data_entries)? == data_ownership,
        "source state changed during backup"
    );
    ensure!(
        snapshot::digest_file(&plan.config)? == config_entries[0].sha256,
        "configuration changed during backup"
    );
    *stage = PreparationStage::Writers;
    stopped(plan)?;
    *stage = PreparationStage::Backup;
    let mut report = PreparationReport {
        format: 1,
        operation_id: Uuid::new_v4(),
        source_identity: plan.source_identity.clone(),
        target_version: TARGET_VERSION.into(),
        phase: "backup-complete".into(),
        invalidated_devices: 0,
        plan: plan.clone(),
        data_entries,
        data_ownership,
        config_entries,
        config_ownership,
    };
    snapshot::atomic_json(&plan.work_directory.join(REPORT), &report)?;
    // Persist intent before SQLite can commit. After this boundary an error
    // must never imply that the original observations are still present.
    report.phase = "cache-write-intent".into();
    snapshot::atomic_json(&plan.work_directory.join(REPORT), &report)?;
    *stage = PreparationStage::CacheWrite;
    report.invalidated_devices =
        xcss_sqlite::block_on_sqlite_connection(invalidate_cache(&plan.database))?;
    *stage = PreparationStage::PostCommit;
    report.phase = "cache-invalidated".into();
    snapshot::atomic_json(&plan.work_directory.join(REPORT), &report)?;
    state.verify_identity()?;
    let after = fs::symlink_metadata(&plan.database)?;
    let before = database.metadata()?;
    ensure!(
        after.dev() == before.dev()
            && after.ino() == before.ino()
            && after.uid() == owner.uid()
            && after.gid() == before.gid(),
        "database identity/owner changed"
    );
    ensure!(
        snapshot::digest_file(&plan.config)? == report.config_entries[0].sha256,
        "configuration changed during preparation"
    );
    report.phase = "prepared".into();
    snapshot::atomic_json(&plan.work_directory.join(REPORT), &report)?;
    Ok(report)
}

async fn invalidate_cache(path: &Path) -> anyhow::Result<u64> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let result = async {
        xcss_sqlite::apply_connection_limits(&mut connection, xcss_sqlite::ConnectionLimits::new(1024 * 1024)).await?;
        xcss_sqlite::enable_defensive(&mut connection).await?;
        let deadline = Instant::now() + Duration::from_secs(30);
        connection.lock_handle().await?.set_progress_handler(1000, move || Instant::now() < deadline);
        xcss_sqlite::require_current_schema(&mut connection, &schema_identity()?).await?;
        let mut transaction = connection.begin().await?;
        let mut rows = sqlx::query("SELECT capabilities_json FROM devices WHERE capabilities_json IS NOT NULL LIMIT 10001")
            .fetch(&mut *transaction);
        let mut count = 0;
        while let Some(row) = rows.try_next().await? {
            count += 1;
            ensure!(count <= 10000, "observation count exceeds preparation limit");
            let json: String = row.try_get("capabilities_json")?;
            validate_old_capabilities(&json)?;
        }
        drop(rows);
        let updated = sqlx::query("UPDATE devices SET capabilities_json=NULL,session_id=NULL,last_seen_at_micros=NULL,health_at_micros=NULL,sunshine_reachable=NULL WHERE capabilities_json IS NOT NULL OR session_id IS NOT NULL OR last_seen_at_micros IS NOT NULL OR health_at_micros IS NOT NULL OR sunshine_reachable IS NOT NULL")
            .execute(&mut *transaction).await?.rows_affected();
        transaction.commit().await?;
        Ok::<_, anyhow::Error>(updated)
    }.await;
    // Close the native SQLite worker before comparing files or releasing locks.
    let closed = connection.close().await;
    result.and_then(|count| {
        closed?;
        Ok(count)
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OldCapabilities {
    protocol: String,
    client_version: String,
    os: String,
    sunshine_version: String,
    managed_fields: Vec<String>,
    #[serde(rename = "restart_allowed")]
    _restart_allowed: bool,
    #[serde(rename = "application_management")]
    _application_management: bool,
    #[serde(rename = "application_host_commands_allowed")]
    _application_host_commands_allowed: bool,
    #[serde(rename = "moonlight_pairing_management")]
    _moonlight_pairing_management: bool,
    #[serde(rename = "diagnostics")]
    _diagnostics: bool,
    #[serde(rename = "maintenance")]
    _maintenance: bool,
    #[serde(rename = "service_control")]
    _service_control: bool,
}

fn validate_old_capabilities(json: &str) -> anyhow::Result<()> {
    ensure!(
        json.len() <= 64 * 1024,
        "capability observation exceeds source message limit"
    );
    let capabilities: OldCapabilities = serde_json::from_str(json)?;
    ensure!(
        capabilities.protocol == "sunshine-management/3"
            && !capabilities.client_version.is_empty()
            && capabilities.client_version.len() <= 128
            && matches!(
                capabilities.os.as_str(),
                "linux_x86_64" | "windows_x86_64" | "macos_x86_64" | "macos_aarch64"
            )
            && capabilities.sunshine_version == "2026.914.233613"
            && capabilities.managed_fields.len() <= 1024
            && capabilities
                .managed_fields
                .iter()
                .all(|field| !field.is_empty() && field.len() <= 128),
        "observation differs from the exact complete Sunshine v3 source contract"
    );
    Ok(())
}

#[cfg(test)]
mod tests;

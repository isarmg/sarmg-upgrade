use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use rusqlite::{Connection, OpenFlags, config::DbConfig, params};
use rustix::{
    fs::{AtFlags, FileType, Mode, OFlags, fchown, fstat, openat2, statat},
    process::{Gid, Uid},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    DATABASE_FILE, MaintenanceLock, PendingDirectory, Product, SchemaIdentity, SecureDirectory,
    SourceClone, TargetStaging, adjacent_marker, copy_database_online, create_private_empty_file,
    hash_regular_file, open_read_only, schema_fingerprint_connection, secure_resolve_flags,
    sqlite_read_only_uri, sync_directory,
};
use crate::RecoveryAction;
use crate::sqlite::{absolute_path, require_regular_file, verify_metadata_table_shape};

mod config;
mod journal;
mod tree;

use config::ConfigAnchor;
use journal::{DufsJournal, DufsJournalPhase};
use tree::{PurgeExpectation, RootAnchor, StageFileExpectation, StageMove, TreeInventory};

const FROM_VERSION: &str = "0.49.7";
const TO_VERSION: &str = "0.50.0";
const SOURCE_TAG_COMMIT: &str = "5b098e2a8f05557b72efdf7929f4ccef3a3af837";
const TARGET_SOURCE_COMMIT: &str = "2369bd990abf4c1492ca16178f2f66765104be25";
const SOURCE_APPLICATION_ID: i64 = 0x4455_4653;
const SOURCE_USER_VERSION: i64 = 5;
const TARGET_REVISION: u64 = 1;
const SCHEMA_SHA256: &str = "3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423";
const CURRENT_SCHEMA_SQL: &str = include_str!("../../upgrades/dufs_0_49_7_to_0_50_0/current.sql");
#[cfg(test)]
const SOURCE_SCHEMA_SQL: &str = include_str!("../../upgrades/dufs_0_49_7_to_0_50_0/source.sql");
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_DATABASE_GENERATION_ENTRY_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_OPERATIONS: u64 = 4096;
const MAX_OPERATIONS_PER_OWNER: u64 = 1024;
const MAX_UPLOADS: u64 = 16_384;
const MAX_UPLOADS_PER_OWNER: u64 = 4096;
const MAX_PURGES: u64 = 4096;
const MAX_PURGES_PER_OWNER: u64 = 1024;
const MAX_DB_PATH_BYTES: usize = 65_536;
const BACKUP_MANIFEST_VERSION: u32 = 1;
const BACKUP_MANIFEST_FILE: &str = "manifest.json";
const BACKUP_DATABASE_FILE: &str = "database.sqlite3";
const BACKUP_CONFIG_FILE: &str = "dufs-config.yml";
const BACKUP_TREE_DIRECTORY: &str = "shared-root";
const BACKUP_RAW_GENERATION_DIRECTORY: &str = "raw-generation";
const OLD_STAGE_DIRECTORY: &[u8] = b".dufs-quarantine-00000000-0000-0000-0000-000000000000.hold";
const NEW_STAGE_DIRECTORY: &[u8] = b".dufs-upload-stages";
const UPLOAD_PREFIX: &[u8] = b".dufs-upload-";
const UPLOAD_SUFFIX: &[u8] = b".part";
const READINESS_PREFIX: &[u8] = b".dufs-readiness-";
const CURRENT_BACKUP_MANIFEST_VERSION: u32 = 2;

#[derive(Clone, Debug)]
pub struct DufsCurrentOptions {
    pub database: PathBuf,
    pub output: PathBuf,
    pub config: PathBuf,
    pub shared_root: PathBuf,
    pub state_dir: PathBuf,
    pub service_uid: u32,
    pub service_gid: u32,
    pub tree_budget: DufsTreeBudget,
}

#[derive(Clone, Debug)]
pub struct DufsCurrentRestoreOptions {
    pub input: PathBuf,
    pub database: PathBuf,
    pub config: PathBuf,
    pub shared_root: PathBuf,
    pub state_dir: PathBuf,
    pub service_uid: u32,
    pub service_gid: u32,
    pub replace_config: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DufsCurrentBackupManifest {
    pub manifest_version: u32,
    pub adapter_id: String,
    pub tool_version: String,
    pub product: Product,
    pub application_version: String,
    pub schema_identity: SchemaIdentity,
    pub created_at_epoch_seconds: u64,
    pub database: DufsStoredResource,
    pub config: DufsConfigMetadata,
    pub config_file: DufsStoredResource,
    pub root_device: u64,
    pub root_inode: u64,
    pub tree: TreeInventory,
    pub database_records: BTreeMap<String, u64>,
    pub tree_budget: DufsTreeBudget,
}

#[derive(Clone, Debug)]
pub struct VerifiedDufsCurrentBackup {
    pub directory: PathBuf,
    pub manifest: DufsCurrentBackupManifest,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DufsTreeBudget {
    pub max_entries: u64,
    pub max_logical_bytes: u64,
    pub max_backup_bytes: u64,
    pub max_entries_per_directory: u64,
}

impl DufsTreeBudget {
    fn validate(self) -> anyhow::Result<()> {
        ensure!(
            self.max_entries > 0
                && self.max_logical_bytes > 0
                && self.max_backup_bytes > 0
                && self.max_entries_per_directory > 0,
            "every Dufs tree resource budget must be non-zero"
        );
        Ok(())
    }
}

pub struct DufsUpgradeOptions {
    pub product: Product,
    pub from_version: String,
    pub to_version: String,
    pub database: PathBuf,
    pub backup_output: PathBuf,
    pub config: PathBuf,
    pub shared_root: PathBuf,
    pub state_dir: PathBuf,
    pub service_uid: u32,
    pub service_gid: u32,
    pub tree_budget: DufsTreeBudget,
}

pub fn backup_dufs_current(
    options: &DufsCurrentOptions,
) -> anyhow::Result<VerifiedDufsCurrentBackup> {
    options.tree_budget.validate()?;
    let mut config = ConfigAnchor::open(&options.config, options.service_uid, options.service_gid)?;
    ensure!(
        config.parsed.shared_root == absolute_path(&options.shared_root)?
            && config.parsed.state_dir == absolute_path(&options.state_dir)?,
        "Dufs current paths do not match the protected config"
    );
    ensure!(
        absolute_path(&options.database)?
            == absolute_path(&options.state_dir)?.join("state.sqlite3"),
        "Dufs current database must be state-dir/state.sqlite3"
    );
    tree::validate_disjoint_paths(
        &options.shared_root,
        &options.state_dir,
        config.configured_path(),
        &options.output,
    )?;
    let maintenance = MaintenanceLock::exclusive(Product::DufsRam, &options.database)?;
    let root = RootAnchor::lock(
        &options.shared_root,
        options.service_uid,
        options.service_gid,
    )?;
    let synthetic = DufsUpgradeOptions {
        product: Product::DufsRam,
        from_version: FROM_VERSION.to_owned(),
        to_version: TO_VERSION.to_owned(),
        database: options.database.clone(),
        backup_output: options.output.clone(),
        config: options.config.clone(),
        shared_root: options.shared_root.clone(),
        state_dir: options.state_dir.clone(),
        service_uid: options.service_uid,
        service_gid: options.service_gid,
        tree_budget: options.tree_budget,
    };
    validate_state_and_database(&synthetic, &maintenance, &root)?;
    let source = SourceClone::create(&maintenance, Product::DufsRam)?;
    let identity = verify_target_database(&source.database(), root.identity())?;
    let records = inspect_record_counts(&source.database())?;
    let mut pending = PendingDirectory::create(&options.output)?;
    let output = pending.path();
    create_private_empty_file(&output.join(BACKUP_DATABASE_FILE))?;
    copy_database_online(&source.database(), &output.join(BACKUP_DATABASE_FILE))?;
    ensure!(
        verify_target_database(&output.join(BACKUP_DATABASE_FILE), root.identity())? == identity,
        "Dufs current database identity changed while it was backed up"
    );
    copy_config(&config, &output.join(BACKUP_CONFIG_FILE))?;
    let tree = root.backup_tree(&output.join(BACKUP_TREE_DIRECTORY), options.tree_budget)?;
    source.ensure_source_unchanged()?;
    root.ensure_unchanged()?;
    config.ensure_unchanged()?;
    let (database_bytes, database_sha256) = hash_regular_file(&output.join(BACKUP_DATABASE_FILE))?;
    let (config_bytes, config_sha256) = hash_regular_file(&output.join(BACKUP_CONFIG_FILE))?;
    let manifest = DufsCurrentBackupManifest {
        manifest_version: CURRENT_BACKUP_MANIFEST_VERSION,
        adapter_id: "dufs-ram-current-0.50.0-r1".to_owned(),
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        product: Product::DufsRam,
        application_version: TO_VERSION.to_owned(),
        schema_identity: identity,
        created_at_epoch_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        database: DufsStoredResource {
            path: BACKUP_DATABASE_FILE.to_owned(),
            bytes: database_bytes,
            sha256: database_sha256,
        },
        config: config.metadata(),
        config_file: DufsStoredResource {
            path: BACKUP_CONFIG_FILE.to_owned(),
            bytes: config_bytes,
            sha256: config_sha256,
        },
        root_device: root.identity().device,
        root_inode: root.identity().inode,
        tree,
        database_records: records,
        tree_budget: options.tree_budget,
    };
    validate_current_manifest(&manifest)?;
    write_json_create_new(&output.join(BACKUP_MANIFEST_FILE), &manifest)?;
    sync_directory(&output)?;
    pending.commit()?;
    verify_dufs_current_backup(&options.output)
}

pub fn verify_dufs_current_backup(input: &Path) -> anyhow::Result<VerifiedDufsCurrentBackup> {
    let directory = SecureDirectory::open(input, "Dufs current backup directory")?;
    ensure!(
        directory.entry_names()?
            == vec![
                BACKUP_DATABASE_FILE.to_owned(),
                BACKUP_CONFIG_FILE.to_owned(),
                BACKUP_MANIFEST_FILE.to_owned(),
                BACKUP_TREE_DIRECTORY.to_owned(),
            ],
        "Dufs current backup entry set is not exact"
    );
    let manifest: DufsCurrentBackupManifest =
        serde_json::from_slice(&directory.read_bounded(BACKUP_MANIFEST_FILE, 64 * 1024 * 1024)?)?;
    validate_current_manifest(&manifest)?;
    verify_stored_file(&directory, &manifest.database)?;
    verify_stored_file(&directory, &manifest.config_file)?;
    ensure!(
        verify_target_database(
            &directory.child_path(BACKUP_DATABASE_FILE),
            tree::RootIdentity {
                device: manifest.root_device,
                inode: manifest.root_inode
            },
        )? == manifest.schema_identity,
        "Dufs current backup database identity mismatch"
    );
    ensure!(
        inspect_record_counts(&directory.child_path(BACKUP_DATABASE_FILE))?
            == manifest.database_records,
        "Dufs current backup database records mismatch"
    );
    ensure!(
        tree::inventory_path(
            &directory.child_path(BACKUP_TREE_DIRECTORY),
            manifest.tree_budget
        )? == manifest.tree,
        "Dufs current backup tree inventory mismatch"
    );
    Ok(VerifiedDufsCurrentBackup {
        directory: absolute_path(input)?,
        manifest,
    })
}

pub fn restore_dufs_current(
    options: &DufsCurrentRestoreOptions,
) -> anyhow::Result<VerifiedDufsCurrentBackup> {
    let backup = verify_dufs_current_backup(&options.input)?;
    ensure!(
        absolute_path(&options.database)?
            == absolute_path(&options.state_dir)?.join("state.sqlite3"),
        "Dufs restore database must be state-dir/state.sqlite3"
    );
    tree::validate_core_path_relationships(
        &options.shared_root,
        &options.state_dir,
        &options.config,
    )?;
    let parsed = config::parse_bytes(&fs::read(backup.directory.join(BACKUP_CONFIG_FILE))?)?;
    ensure!(
        parsed.shared_root == absolute_path(&options.shared_root)?
            && parsed.state_dir == absolute_path(&options.state_dir)?,
        "backup Dufs config does not bind the explicit restore paths"
    );
    tree::validate_state_directory(&options.state_dir, options.service_uid, options.service_gid)?;
    let maintenance = MaintenanceLock::exclusive(Product::DufsRam, &options.database)?;
    ensure!(
        !options.database.exists(),
        "Dufs current restore only installs into a missing database target"
    );
    let root = RootAnchor::lock(
        &options.shared_root,
        options.service_uid,
        options.service_gid,
    )?;
    ensure!(
        fs::read_dir(&options.shared_root)?.next().is_none(),
        "Dufs current restore only installs into an empty shared root"
    );
    ensure!(
        options.replace_config || !options.config.exists(),
        "Dufs config already exists; pass --replace-config"
    );

    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let database_stage = options
        .state_dir
        .join(format!(".state.sqlite3.restore-{nonce}"));
    let config_stage = options
        .config
        .parent()
        .context("Dufs config has no parent")?
        .join(format!(".dufs-config.restore-{nonce}"));
    let config_original = options
        .config
        .parent()
        .context("Dufs config has no parent")?
        .join(format!(".dufs-config.original-{nonce}"));

    copy_regular_exact(
        &backup.directory.join(BACKUP_DATABASE_FILE),
        &database_stage,
    )?;
    let connection = Connection::open_with_flags(
        &database_stage,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure_verifier(&connection)?;
    connection.execute(
        "UPDATE store_meta SET value=?1 WHERE key='root-device-be'",
        [root.identity().device.to_be_bytes().to_vec()],
    )?;
    connection.execute(
        "UPDATE store_meta SET value=?1 WHERE key='root-inode-be'",
        [root.identity().inode.to_be_bytes().to_vec()],
    )?;
    drop(connection);
    let database_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&database_stage)?;
    fchown(
        &database_file,
        Some(Uid::from_raw(options.service_uid)),
        Some(Gid::from_raw(options.service_gid)),
    )?;
    fs::set_permissions(
        &database_stage,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )?;
    database_file.sync_all()?;
    ensure!(
        verify_target_database(&database_stage, root.identity())?
            == backup.manifest.schema_identity,
        "rebound Dufs restore database is invalid"
    );

    copy_regular_exact(&backup.directory.join(BACKUP_CONFIG_FILE), &config_stage)?;
    let config_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&config_stage)?;
    fchown(
        &config_file,
        Some(Uid::from_raw(backup.manifest.config.uid)),
        Some(Gid::from_raw(backup.manifest.config.gid)),
    )?;
    fs::set_permissions(
        &config_stage,
        std::os::unix::fs::PermissionsExt::from_mode(backup.manifest.config.mode),
    )?;
    config_file.sync_all()?;
    ConfigAnchor::open(&config_stage, options.service_uid, options.service_gid)?;

    let restored_tree = tree::restore_tree_into_empty(
        &backup.directory.join(BACKUP_TREE_DIRECTORY),
        &options.shared_root,
        backup.manifest.tree_budget,
    )?;
    ensure!(
        restored_tree == backup.manifest.tree,
        "Dufs restored tree differs from backup"
    );
    // Tree installation intentionally restores root metadata, so the empty
    // target snapshot is no longer the right postcondition. Keep the original
    // locked FD alive, then independently re-open the configured path and
    // require it to resolve to the same inode with valid service ownership.
    let restored_root = RootAnchor::open_unlocked(
        &options.shared_root,
        options.service_uid,
        options.service_gid,
    )?;
    ensure!(
        restored_root.identity() == root.identity(),
        "Dufs shared-root identity changed while restoring the tree"
    );

    if options.config.exists() {
        fs::rename(&options.config, &config_original)?;
    }
    fs::rename(&config_stage, &options.config)?;
    sync_directory(
        options
            .config
            .parent()
            .context("Dufs config has no parent")?,
    )?;
    fs::rename(&database_stage, &options.database)?;
    sync_directory(&options.state_dir)?;
    verify_target_database(&options.database, restored_root.identity())?;
    ConfigAnchor::open(&options.config, options.service_uid, options.service_gid)?;
    if config_original.exists() {
        fs::remove_file(config_original)?;
    }
    drop(maintenance);
    Ok(backup)
}

fn validate_current_manifest(manifest: &DufsCurrentBackupManifest) -> anyhow::Result<()> {
    ensure!(
        manifest.manifest_version == CURRENT_BACKUP_MANIFEST_VERSION
            && manifest.adapter_id == "dufs-ram-current-0.50.0-r1"
            && manifest.tool_version == env!("CARGO_PKG_VERSION")
            && manifest.product == Product::DufsRam
            && manifest.application_version == TO_VERSION,
        "Dufs current backup adapter identity is not exact"
    );
    ensure!(
        manifest.schema_identity
            == SchemaIdentity {
                application: Product::DufsRam.slug().to_owned(),
                application_version: TO_VERSION.to_owned(),
                schema_revision: TARGET_REVISION,
                schema_sha256: SCHEMA_SHA256.to_owned(),
            },
        "Dufs current backup schema identity is not exact"
    );
    ensure!(
        manifest.database.path == BACKUP_DATABASE_FILE
            && manifest.config_file.path == BACKUP_CONFIG_FILE
            && manifest.config_file.bytes == manifest.config.bytes
            && manifest.config_file.sha256 == manifest.config.sha256
            && manifest.config.sensitive,
        "Dufs current backup resource identity is invalid"
    );
    manifest.tree_budget.validate()?;
    Ok(())
}

impl std::fmt::Debug for DufsUpgradeOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DufsUpgradeOptions")
            .field("product", &self.product)
            .field("from_version", &self.from_version)
            .field("to_version", &self.to_version)
            .field("database", &self.database)
            .field("backup_output", &self.backup_output)
            .field("config", &self.config)
            .field("shared_root", &self.shared_root)
            .field("state_dir", &self.state_dir)
            .field("service_uid", &self.service_uid)
            .field("service_gid", &self.service_gid)
            .field("tree_budget", &self.tree_budget)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct DufsRecoveryOptions {
    pub product: Product,
    pub from_version: String,
    pub to_version: String,
    pub database: PathBuf,
    pub config: PathBuf,
    pub shared_root: PathBuf,
    pub state_dir: PathBuf,
    pub service_uid: u32,
    pub service_gid: u32,
    pub recovery_directory: PathBuf,
    pub action: RecoveryAction,
}

#[derive(Clone, Debug, Serialize)]
pub struct DufsUpgradeResult {
    pub product: Product,
    pub from_version: String,
    pub to_version: String,
    pub source_backup: PathBuf,
    pub database: PathBuf,
    pub schema_identity: SchemaIdentity,
    pub stage_directories_moved: u64,
}

#[derive(Clone, Debug)]
pub struct VerifiedDufsSourceBackup {
    pub directory: PathBuf,
    pub manifest: DufsCompositeBackupManifest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DufsStoredResource {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DufsConfigMetadata {
    pub bytes: u64,
    pub sha256: String,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub sensitive: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DufsCompositeBackupManifest {
    pub manifest_version: u32,
    pub tool_version: String,
    pub product: Product,
    pub from_version: String,
    pub to_version: String,
    pub source_tag_commit: String,
    pub target_source_commit: String,
    pub source_schema_identity: SchemaIdentity,
    pub created_at_epoch_seconds: u64,
    pub database: DufsStoredResource,
    pub raw_database_generation: Vec<DufsStoredResource>,
    pub config: DufsConfigMetadata,
    pub config_file: DufsStoredResource,
    pub root_binding_sha256: String,
    pub tree: TreeInventory,
    pub database_records: BTreeMap<String, u64>,
    pub tree_budget: DufsTreeBudget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DufsDatabaseSummary {
    records: BTreeMap<String, u64>,
    stage_moves: Vec<StageMove>,
    purges: Vec<PurgeExpectation>,
}

struct RawGenerationSnapshot {
    pending: PendingDirectory,
    resources: Vec<DufsStoredResource>,
}

impl RawGenerationSnapshot {
    fn create(source: &SourceClone, maintenance: &MaintenanceLock) -> anyhow::Result<Self> {
        let marker = adjacent_marker(
            &maintenance.location,
            Product::DufsRam,
            "upgrade-source-raw",
        )?;
        let pending = PendingDirectory::create(&marker)?;
        let mut resources = Vec::new();
        for suffix in std::iter::once("").chain(super::SQLITE_SIDECARS) {
            let mut name = OsString::from(DATABASE_FILE);
            name.push(suffix);
            let source_path = source.pending.path().join(&name);
            if !source_path.exists() {
                continue;
            }
            let destination = pending.path().join(&name);
            copy_regular_exact(&source_path, &destination)?;
            let (bytes, sha256) = hash_regular_file(&destination)?;
            ensure!(
                bytes <= MAX_DATABASE_GENERATION_ENTRY_BYTES,
                "Dufs raw source generation entry exceeds the 1 GiB adapter limit"
            );
            resources.push(DufsStoredResource {
                path: name
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("internal Dufs raw snapshot name is not UTF-8"))?,
                bytes,
                sha256,
            });
        }
        ensure!(
            resources.len() == source.digests.len(),
            "Dufs raw source snapshot generation set changed"
        );
        for (resource, digest) in resources.iter().zip(&source.digests) {
            ensure!(
                resource.bytes == digest.bytes && resource.sha256 == digest.sha256,
                "Dufs raw source snapshot differs from the locked source generation"
            );
        }
        sync_directory(&pending.path())?;
        source.ensure_source_unchanged()?;
        Ok(Self { pending, resources })
    }
}

/// Upgrade exactly Dufs v0.49.7/schema-v5 to v0.50.0/current revision 1.
pub fn upgrade_dufs(options: &DufsUpgradeOptions) -> anyhow::Result<DufsUpgradeResult> {
    upgrade_dufs_with_hook(options, |_| Ok(()))
}

fn upgrade_dufs_with_hook(
    options: &DufsUpgradeOptions,
    hook: impl FnMut(DufsJournalPhase) -> anyhow::Result<()>,
) -> anyhow::Result<DufsUpgradeResult> {
    validate_selection(options.product, &options.from_version, &options.to_version)?;
    options.tree_budget.validate()?;
    let mut config = ConfigAnchor::open(&options.config, options.service_uid, options.service_gid)?;
    let output_parent_identity = validate_explicit_paths(options, &config)?;
    let maintenance = MaintenanceLock::exclusive(Product::DufsRam, &options.database)?;
    let root = RootAnchor::lock(
        &options.shared_root,
        options.service_uid,
        options.service_gid,
    )?;
    let (state_identity, database_identity) =
        validate_state_and_database(options, &maintenance, &root)?;
    let protected = [
        ("shared root", root.identity()),
        ("state directory", state_identity),
        ("database", database_identity),
        ("config", config.identity()),
        ("backup output parent", output_parent_identity),
    ];
    validate_distinct_protected_objects(&protected)?;
    config.ensure_unchanged()?;

    let source_clone = SourceClone::create(&maintenance, Product::DufsRam)?;
    validate_generation_limits(&source_clone)?;
    let raw_source = RawGenerationSnapshot::create(&source_clone, &maintenance)?;
    recover_private_source_clone(&source_clone.database())?;
    source_clone.ensure_source_unchanged()?;
    let source_identity = verify_source_database(&source_clone.database(), root.identity())?;
    let owner_map = owner_mapping(&config.parsed.usernames)?;
    let mut database_summary = inspect_source_rows(&source_clone.database(), &owner_map)?;
    root.validate_stage_plan(
        &mut database_summary.stage_moves,
        options.service_uid,
        options.tree_budget,
        &protected[1..],
    )?;
    root.validate_purges(&database_summary.purges)?;

    let source_backup = create_source_backup(
        options,
        &mut config,
        &root,
        &source_clone,
        &raw_source,
        &source_identity,
        &database_summary,
    )?;
    let verified_backup = verify_dufs_source_backup(
        options.product,
        &options.from_version,
        &options.to_version,
        &source_backup,
        &options.config,
        &options.shared_root,
        options.service_uid,
        options.service_gid,
    )?;

    let staging = TargetStaging::create(&maintenance, Product::DufsRam)?;
    let target_identity = create_target_database(
        &verified_backup.directory.join(BACKUP_DATABASE_FILE),
        &staging.database(),
        &owner_map,
        &database_summary,
        root.identity(),
        options.service_uid,
        options.service_gid,
    )?;
    source_clone.ensure_source_unchanged()?;
    root.ensure_unchanged()?;
    config.ensure_unchanged()?;

    let mut journal = DufsJournal::prepare(
        &maintenance,
        &config,
        &root,
        &staging.database(),
        &verified_backup,
        &database_summary.stage_moves,
        &target_identity,
    )?;
    journal.install(&maintenance, &root, hook)?;

    Ok(DufsUpgradeResult {
        product: Product::DufsRam,
        from_version: FROM_VERSION.to_owned(),
        to_version: TO_VERSION.to_owned(),
        source_backup: verified_backup.directory,
        database: options.database.clone(),
        schema_identity: target_identity,
        stage_directories_moved: database_summary.stage_moves.len() as u64,
    })
}

pub fn recover_dufs_upgrade(options: &DufsRecoveryOptions) -> anyhow::Result<DufsUpgradeResult> {
    validate_selection(options.product, &options.from_version, &options.to_version)?;
    let mut config = ConfigAnchor::open(&options.config, options.service_uid, options.service_gid)?;
    ensure!(
        config.parsed.shared_root == absolute_path(&options.shared_root)?
            && config.parsed.state_dir == absolute_path(&options.state_dir)?,
        "explicit Dufs recovery paths do not match protected config"
    );
    ensure!(
        absolute_path(&options.database)?
            == absolute_path(&options.state_dir)?.join("state.sqlite3"),
        "Dufs recovery database must be exactly state-dir/state.sqlite3"
    );
    tree::validate_core_path_relationships(
        &options.shared_root,
        &options.state_dir,
        config.configured_path(),
    )?;
    let state_identity = tree::validate_state_directory(
        &options.state_dir,
        options.service_uid,
        options.service_gid,
    )?;
    let maintenance = MaintenanceLock::exclusive(Product::DufsRam, &options.database)?;
    let root = RootAnchor::lock(
        &options.shared_root,
        options.service_uid,
        options.service_gid,
    )?;
    let database_stat = statat(
        &maintenance.location.parent,
        &maintenance.location.database_name,
        AtFlags::SYMLINK_NOFOLLOW,
    )?;
    ensure!(
        FileType::from_raw_mode(database_stat.st_mode) == FileType::RegularFile
            && database_stat.st_nlink == 1,
        "Dufs recovery database path must be one regular file"
    );
    validate_distinct_protected_objects(&[
        ("shared root", root.identity()),
        ("state directory", state_identity),
        (
            "database",
            tree::RootIdentity {
                device: database_stat.st_dev,
                inode: database_stat.st_ino,
            },
        ),
        ("config", config.identity()),
    ])?;
    config.ensure_unchanged()?;
    let journal = DufsJournal::open(&options.recovery_directory, &maintenance, &config, &root)?;
    let (backup, identity, moves) = match options.action {
        RecoveryAction::Commit => journal.recover_commit(&maintenance, &root)?,
        RecoveryAction::Rollback => journal.recover_rollback(&maintenance, &root)?,
    };
    Ok(DufsUpgradeResult {
        product: Product::DufsRam,
        from_version: FROM_VERSION.to_owned(),
        to_version: TO_VERSION.to_owned(),
        source_backup: backup,
        database: options.database.clone(),
        schema_identity: identity,
        stage_directories_moved: moves,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn verify_dufs_source_backup(
    product: Product,
    from_version: &str,
    to_version: &str,
    input: &Path,
    config_path: &Path,
    shared_root: &Path,
    service_uid: u32,
    service_gid: u32,
) -> anyhow::Result<VerifiedDufsSourceBackup> {
    validate_selection(product, from_version, to_version)?;
    let mut config = ConfigAnchor::open(config_path, service_uid, service_gid)?;
    let root = RootAnchor::open_unlocked(shared_root, service_uid, service_gid)?;
    ensure!(
        config.parsed.shared_root == absolute_path(shared_root)?,
        "backup verifier shared root does not match protected config"
    );
    let verified = verify_dufs_source_backup_anchored(input, &config, &root)?;
    config.ensure_unchanged()?;
    Ok(verified)
}

fn verify_dufs_source_backup_anchored(
    input: &Path,
    config: &ConfigAnchor,
    root: &RootAnchor,
) -> anyhow::Result<VerifiedDufsSourceBackup> {
    let directory = SecureDirectory::open(input, "Dufs source backup directory")?;
    let expected = vec![
        BACKUP_DATABASE_FILE.to_owned(),
        BACKUP_CONFIG_FILE.to_owned(),
        BACKUP_MANIFEST_FILE.to_owned(),
        BACKUP_RAW_GENERATION_DIRECTORY.to_owned(),
        BACKUP_TREE_DIRECTORY.to_owned(),
    ];
    ensure!(
        directory.entry_names()? == expected,
        "Dufs backup entry set is not exact"
    );
    let manifest: DufsCompositeBackupManifest =
        serde_json::from_slice(&directory.read_bounded(BACKUP_MANIFEST_FILE, 64 * 1024 * 1024)?)
            .context("parse exact Dufs backup manifest")?;
    validate_backup_manifest(&manifest, config, root)?;

    verify_stored_file(&directory, &manifest.database)?;
    verify_raw_generation(&directory, &manifest.raw_database_generation)?;
    verify_stored_file(&directory, &manifest.config_file)?;
    ensure!(
        directory.read_bounded(BACKUP_CONFIG_FILE, MAX_CONFIG_BYTES)? == config.bytes(),
        "Dufs backup config does not match the anchored protected config"
    );
    let identity =
        verify_source_database(&directory.child_path(BACKUP_DATABASE_FILE), root.identity())?;
    ensure!(
        identity == manifest.source_schema_identity,
        "Dufs backup database identity mismatch"
    );
    let records = inspect_record_counts(&directory.child_path(BACKUP_DATABASE_FILE))?;
    ensure!(
        records == manifest.database_records,
        "Dufs backup row counts mismatch"
    );
    let inventory = tree::inventory_path(
        &directory.child_path(BACKUP_TREE_DIRECTORY),
        manifest.tree_budget,
    )?;
    ensure!(
        inventory == manifest.tree,
        "Dufs backup tree inventory mismatch"
    );
    Ok(VerifiedDufsSourceBackup {
        directory: absolute_path(input)?,
        manifest,
    })
}

fn validate_selection(product: Product, from: &str, to: &str) -> anyhow::Result<()> {
    ensure!(
        product == Product::DufsRam,
        "Dufs adapter requires --product dufs-ram"
    );
    ensure!(
        from == FROM_VERSION && to == TO_VERSION,
        "only the explicit Dufs {FROM_VERSION} -> {TO_VERSION} adapter exists"
    );
    Ok(())
}

fn validate_explicit_paths(
    options: &DufsUpgradeOptions,
    config: &ConfigAnchor,
) -> anyhow::Result<tree::RootIdentity> {
    let root = absolute_path(&options.shared_root)?;
    let state = absolute_path(&options.state_dir)?;
    let database = absolute_path(&options.database)?;
    ensure!(
        config.parsed.shared_root == root,
        "--shared-root does not match config serve-path"
    );
    ensure!(
        config.parsed.state_dir == state,
        "--state-dir does not match config state-dir"
    );
    ensure!(
        database == state.join("state.sqlite3"),
        "Dufs database must be exactly state-dir/state.sqlite3"
    );
    tree::validate_disjoint_paths(
        &root,
        &state,
        config.configured_path(),
        &options.backup_output,
    )
}

fn validate_state_and_database(
    options: &DufsUpgradeOptions,
    maintenance: &MaintenanceLock,
    root: &RootAnchor,
) -> anyhow::Result<(tree::RootIdentity, tree::RootIdentity)> {
    let state_identity = tree::validate_state_directory(
        &options.state_dir,
        options.service_uid,
        options.service_gid,
    )?;
    let stat = statat(
        &maintenance.location.parent,
        &maintenance.location.database_name,
        AtFlags::SYMLINK_NOFOLLOW,
    )?;
    ensure!(
        FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
            && stat.st_nlink == 1
            && stat.st_uid == options.service_uid
            && stat.st_gid == options.service_gid
            && stat.st_mode & 0o7777 == 0o600,
        "Dufs source database must be a 0600 service-owned regular file with one link"
    );
    root.ensure_unchanged()?;
    Ok((
        state_identity,
        tree::RootIdentity {
            device: stat.st_dev,
            inode: stat.st_ino,
        },
    ))
}

fn validate_generation_limits(source: &SourceClone) -> anyhow::Result<()> {
    for digest in &source.digests {
        ensure!(
            digest.bytes <= MAX_DATABASE_GENERATION_ENTRY_BYTES,
            "Dufs database generation entry exceeds the 1 GiB adapter limit"
        );
    }
    Ok(())
}

fn validate_distinct_protected_objects(
    objects: &[(&str, tree::RootIdentity)],
) -> anyhow::Result<()> {
    for (index, (left_label, left)) in objects.iter().enumerate() {
        for (right_label, right) in &objects[index + 1..] {
            ensure!(
                left != right,
                "Dufs protected {left_label} aliases protected {right_label}"
            );
        }
    }
    Ok(())
}

fn owner_mapping(usernames: &[String]) -> anyhow::Result<BTreeMap<Vec<u8>, Vec<u8>>> {
    owner_mapping_with(usernames, |username| {
        let source = Sha256::digest(username.as_bytes()).to_vec();
        let mut target_hasher = Sha256::new();
        target_hasher.update(b"dufs-durable-owner-v1\0");
        target_hasher.update(username.as_bytes());
        (source, target_hasher.finalize().to_vec())
    })
}

fn owner_mapping_with(
    usernames: &[String],
    mut derive: impl FnMut(&str) -> (Vec<u8>, Vec<u8>),
) -> anyhow::Result<BTreeMap<Vec<u8>, Vec<u8>>> {
    let mut mapping = BTreeMap::new();
    let mut targets = BTreeSet::new();
    for username in usernames {
        let (source, target) = derive(username);
        ensure!(
            source.len() == 32 && target.len() == 32,
            "internal Dufs owner digest width is invalid"
        );
        ensure!(
            mapping.insert(source, target.clone()).is_none(),
            "Dufs auth usernames collide under the old owner digest"
        );
        ensure!(
            targets.insert(target),
            "Dufs auth usernames collide under the new owner digest"
        );
    }
    Ok(mapping)
}

fn verify_source_database(
    database: &Path,
    root: tree::RootIdentity,
) -> anyhow::Result<SchemaIdentity> {
    require_regular_file(database, "Dufs source SQLite database")?;
    let connection = open_read_only(database)?;
    configure_verifier(&connection)?;
    verify_integrity(&connection)?;
    ensure!(
        pragma_i64(&connection, "application_id")? == SOURCE_APPLICATION_ID,
        "Dufs source application_id is not exact"
    );
    ensure!(
        pragma_i64(&connection, "user_version")? == SOURCE_USER_VERSION,
        "Dufs source user_version is not schema v5"
    );
    let metadata: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='product_metadata'",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        metadata == 0,
        "Dufs v0.49.7 source must not contain product_metadata"
    );
    let fingerprint = schema_fingerprint_connection(&connection)?;
    ensure!(
        fingerprint == SCHEMA_SHA256,
        "Dufs source is not the exact official schema-v5 contract"
    );
    verify_exact_schema_entries(&connection, false)?;
    verify_root_binding(&connection, root)?;
    Ok(SchemaIdentity {
        application: Product::DufsRam.slug().to_owned(),
        application_version: FROM_VERSION.to_owned(),
        schema_revision: SOURCE_USER_VERSION as u64,
        schema_sha256: fingerprint,
    })
}

fn verify_target_database(
    database: &Path,
    root: tree::RootIdentity,
) -> anyhow::Result<SchemaIdentity> {
    require_regular_file(database, "Dufs target SQLite database")?;
    let connection = open_read_only(database)?;
    configure_verifier(&connection)?;
    verify_integrity(&connection)?;
    ensure!(
        pragma_i64(&connection, "application_id")? == 0
            && pragma_i64(&connection, "user_version")? == 0,
        "Dufs current SQLite header identity is not exact"
    );
    verify_exact_schema_entries(&connection, true)?;
    let fingerprint = schema_fingerprint_connection(&connection)?;
    ensure!(
        fingerprint == SCHEMA_SHA256,
        "Dufs target schema fingerprint is not official"
    );
    verify_metadata_table_shape(&connection)?;
    let row: (i64, String, String, i64, String) = connection.query_row(
        "SELECT singleton, application, application_version, schema_revision, schema_sha256 FROM product_metadata",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )?;
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM product_metadata", [], |row| {
        row.get(0)
    })?;
    ensure!(
        count == 1
            && row
                == (
                    1,
                    Product::DufsRam.slug().to_owned(),
                    TO_VERSION.to_owned(),
                    TARGET_REVISION as i64,
                    SCHEMA_SHA256.to_owned()
                ),
        "Dufs target product_metadata is not exact"
    );
    verify_root_binding(&connection, root)?;
    Ok(SchemaIdentity {
        application: Product::DufsRam.slug().to_owned(),
        application_version: TO_VERSION.to_owned(),
        schema_revision: TARGET_REVISION,
        schema_sha256: fingerprint,
    })
}

fn recover_private_source_clone(database: &Path) -> anyhow::Result<()> {
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("open private Dufs source clone for rollback-journal recovery")?;
    configure_verifier(&connection)?;
    let _: i64 = connection.query_row("PRAGMA schema_version", [], |row| row.get(0))?;
    drop(connection);
    File::open(database)?.sync_all()?;
    sync_directory(
        database
            .parent()
            .context("private Dufs source clone has no parent")?,
    )?;
    Ok(())
}

fn configure_verifier(connection: &Connection) -> anyhow::Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "mmap_size", 0)?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.pragma_update(None, "synchronous", "EXTRA")?;
    let journal_mode: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    ensure!(
        journal_mode.eq_ignore_ascii_case("delete"),
        "Dufs SQLite journal mode is not the exact DELETE contract"
    );
    Ok(())
}

fn verify_integrity(connection: &Connection) -> anyhow::Result<()> {
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    ensure!(
        integrity.eq_ignore_ascii_case("ok"),
        "Dufs SQLite integrity check failed"
    );
    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    ensure!(
        foreign_keys.query([])?.next()?.is_none(),
        "Dufs SQLite foreign-key check failed"
    );
    Ok(())
}

fn pragma_i64(connection: &Connection, name: &str) -> anyhow::Result<i64> {
    ensure!(
        matches!(name, "application_id" | "user_version"),
        "unsupported pragma"
    );
    connection
        .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
        .map_err(Into::into)
}

fn verify_exact_schema_entries(connection: &Connection, current: bool) -> anyhow::Result<()> {
    let mut names = connection
        .prepare("SELECT type || ':' || name FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY type, name")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut expected = vec![
        "index:operations_expiry".to_owned(),
        "index:purge_jobs_due".to_owned(),
        "index:purge_jobs_prepared".to_owned(),
        "index:upload_sessions_expiry".to_owned(),
        "table:operations".to_owned(),
        "table:purge_jobs".to_owned(),
        "table:store_meta".to_owned(),
        "table:upload_sessions".to_owned(),
    ];
    if current {
        expected.push("table:product_metadata".to_owned());
    }
    names.sort();
    expected.sort();
    ensure!(names == expected, "Dufs SQLite object set is not exact");
    Ok(())
}

fn verify_root_binding(connection: &Connection, root: tree::RootIdentity) -> anyhow::Result<()> {
    let rows = connection
        .prepare("SELECT key, value FROM store_meta ORDER BY key")?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    ensure!(
        rows == vec![
            (
                "root-device-be".to_owned(),
                root.device.to_be_bytes().to_vec()
            ),
            (
                "root-inode-be".to_owned(),
                root.inode.to_be_bytes().to_vec()
            ),
        ],
        "Dufs store_meta does not bind exactly to the locked shared root"
    );
    Ok(())
}

fn inspect_source_rows(
    source: &Path,
    owners: &BTreeMap<Vec<u8>, Vec<u8>>,
) -> anyhow::Result<DufsDatabaseSummary> {
    let connection = open_read_only(source)?;
    configure_verifier(&connection)?;
    let records = inspect_record_counts_connection(&connection)?;
    enforce_row_budgets(&connection, &records)?;
    validate_all_owners(&connection, owners)?;
    validate_target_primary_keys(&connection, owners)?;
    let purges = inspect_purges(&connection)?;
    let stage_moves = inspect_upload_stage_moves(&connection)?;
    Ok(DufsDatabaseSummary {
        records,
        stage_moves,
        purges,
    })
}

fn inspect_record_counts(source: &Path) -> anyhow::Result<BTreeMap<String, u64>> {
    let connection = open_read_only(source)?;
    inspect_record_counts_connection(&connection)
}

fn inspect_record_counts_connection(
    connection: &Connection,
) -> anyhow::Result<BTreeMap<String, u64>> {
    let mut counts = BTreeMap::new();
    for table in ["store_meta", "operations", "upload_sessions", "purge_jobs"] {
        let value: i64 =
            connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })?;
        counts.insert(table.to_owned(), u64::try_from(value)?);
    }
    Ok(counts)
}

fn enforce_row_budgets(
    connection: &Connection,
    records: &BTreeMap<String, u64>,
) -> anyhow::Result<()> {
    for (table, total, max_total, max_per_owner) in [
        (
            "operations",
            records["operations"],
            MAX_OPERATIONS,
            MAX_OPERATIONS_PER_OWNER,
        ),
        (
            "upload_sessions",
            records["upload_sessions"],
            MAX_UPLOADS,
            MAX_UPLOADS_PER_OWNER,
        ),
        (
            "purge_jobs",
            records["purge_jobs"],
            MAX_PURGES,
            MAX_PURGES_PER_OWNER,
        ),
    ] {
        ensure!(total <= max_total, "Dufs {table} row budget exceeded");
        let maximum: i64 = connection.query_row(
            &format!("SELECT COALESCE(MAX(n),0) FROM (SELECT COUNT(*) n FROM {table} GROUP BY owner_digest)"),
            [],
            |row| row.get(0),
        )?;
        ensure!(
            u64::try_from(maximum)? <= max_per_owner,
            "Dufs {table} per-owner row budget exceeded"
        );
    }
    Ok(())
}

fn validate_all_owners(
    connection: &Connection,
    owners: &BTreeMap<Vec<u8>, Vec<u8>>,
) -> anyhow::Result<()> {
    let mut statement = connection.prepare(
        "SELECT owner_digest FROM operations UNION SELECT owner_digest FROM upload_sessions UNION SELECT owner_digest FROM purge_jobs",
    )?;
    for owner in statement.query_map([], |row| row.get::<_, Vec<u8>>(0))? {
        let owner = owner?;
        ensure!(
            owner.len() == 32 && owners.contains_key(&owner),
            "persisted Dufs owner cannot be uniquely resolved from protected auth config"
        );
    }
    Ok(())
}

fn validate_target_primary_keys(
    connection: &Connection,
    owners: &BTreeMap<Vec<u8>, Vec<u8>>,
) -> anyhow::Result<()> {
    for (table, id) in [
        ("operations", "operation_id"),
        ("upload_sessions", "upload_id"),
        ("purge_jobs", "job_id"),
    ] {
        let mut seen = BTreeSet::new();
        let mut statement =
            connection.prepare(&format!("SELECT owner_digest, {id} FROM {table}"))?;
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })? {
            let (owner, identifier) = row?;
            let mapped = owners
                .get(&owner)
                .context("unresolved owner during target-key preflight")?;
            ensure!(
                seen.insert((mapped.clone(), identifier)),
                "Dufs target primary-key collision"
            );
        }
    }
    Ok(())
}

fn inspect_upload_stage_moves(connection: &Connection) -> anyhow::Result<Vec<StageMove>> {
    let mut moves = BTreeMap::<Vec<u8>, StageMove>::new();
    let mut physical = BTreeMap::<(u64, u64), Vec<u8>>::new();
    let mut statement = connection.prepare(
        "SELECT owner_digest, upload_id, target_path, stage_path, state, stage_device_be, stage_inode_be FROM upload_sessions ORDER BY owner_digest, upload_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, Vec<u8>>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<Vec<u8>>>(5)?,
            row.get::<_, Option<Vec<u8>>>(6)?,
        ))
    })?;
    for row in rows {
        let (owner, upload_id, target, stage, state, stage_device, stage_inode) = row?;
        let target = validate_relative_db_path(&target, "upload target_path")?;
        let stage = validate_relative_db_path(&stage, "upload stage_path")?;
        ensure!(upload_id.len() == 16, "Dufs upload_id length is invalid");
        let expected = expected_old_stage_path(&target, &upload_id)?;
        ensure!(
            stage == expected,
            "Dufs upload stage_path is not the exact reconciled v0.49.7 path"
        );
        let stage_parent = parent_before_private_dir(&stage, OLD_STAGE_DIRECTORY)?;
        let old_dir = append_component(&stage_parent, OLD_STAGE_DIRECTORY);
        let new_dir = append_component(&stage_parent, NEW_STAGE_DIRECTORY);
        if !matches!(state, 0 | 1 | 5) {
            continue;
        }
        let identity = match (stage_device, stage_inode) {
            (Some(device), Some(inode)) => {
                ensure!(
                    device.len() == 8 && inode.len() == 8,
                    "Dufs stage identity width is invalid"
                );
                let key = (
                    u64::from_be_bytes(device.try_into().expect("length checked")),
                    u64::from_be_bytes(inode.try_into().expect("length checked")),
                );
                if let Some(existing) = physical.insert(key, owner.clone()) {
                    ensure!(
                        existing == owner,
                        "one physical Dufs stage is active for multiple owners"
                    );
                }
                Some(key)
            }
            (None, None) => anyhow::bail!("active Dufs upload has no persisted stage identity"),
            _ => anyhow::bail!("Dufs stage identity columns are not paired"),
        };
        moves
            .entry(old_dir.clone())
            .or_insert_with(|| StageMove::new(old_dir, new_dir))
            .add_file(StageFileExpectation::new(stage, identity));
    }
    Ok(moves.into_values().collect())
}

fn inspect_purges(connection: &Connection) -> anyhow::Result<Vec<PurgeExpectation>> {
    let mut purges = Vec::new();
    let mut statement = connection.prepare(
        "SELECT job_id,target_path,trash_path,source_device_be,source_inode_be,trash_revision,is_directory,state,attempts FROM purge_jobs ORDER BY owner_digest,job_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, Vec<u8>>(3)?,
            row.get::<_, Vec<u8>>(4)?,
            row.get::<_, Option<Vec<u8>>>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
        ))
    })?;
    for row in rows {
        let (job_id, target, trash, device, inode, revision, is_directory, state, attempts) = row?;
        let target = validate_relative_db_path(&target, "purge target_path")?;
        let trash = validate_relative_db_path(&trash, "purge trash_path")?;
        ensure!(
            job_id.len() == 16 && device.len() == 8 && inode.len() == 8,
            "Dufs purge identifier width is invalid"
        );
        let job_id = uuid::Uuid::from_slice(&job_id).context("Dufs purge job_id is not a UUID")?;
        let parent = target
            .iter()
            .rposition(|byte| *byte == b'/')
            .map_or_else(Vec::new, |index| target[..index].to_vec());
        let expected_trash = append_component(
            &parent,
            format!(".dufs-upload-delete-{job_id}.trash").as_bytes(),
        );
        ensure!(
            trash == expected_trash,
            "Dufs purge trash_path is not derived from its target and job_id"
        );
        ensure!(
            matches!(state, 0..=2) && matches!(is_directory, 0 | 1),
            "Dufs purge state/type is invalid"
        );
        if state == 0 {
            ensure!(
                revision.is_none() && attempts == 0,
                "prepared Dufs purge has impossible revision or attempts"
            );
        } else {
            ensure!(
                revision.as_ref().is_some_and(|value| value.len() == 32),
                "ready/claimed Dufs purge lacks its exact trash revision"
            );
        }
        purges.push(PurgeExpectation {
            target,
            trash,
            source_device: u64::from_be_bytes(device.try_into().expect("length checked")),
            source_inode: u64::from_be_bytes(inode.try_into().expect("length checked")),
            is_directory: is_directory == 1,
            state,
        });
    }
    Ok(purges)
}

fn validate_relative_db_path(bytes: &[u8], label: &str) -> anyhow::Result<Vec<u8>> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_DB_PATH_BYTES && !bytes.contains(&0),
        "{label} length or NUL contract is invalid"
    );
    ensure!(
        bytes[0] != b'/' && bytes.last() != Some(&b'/'),
        "{label} must be relative and canonical"
    );
    for component in bytes.split(|byte| *byte == b'/') {
        ensure!(
            !component.is_empty() && component != b"." && component != b"..",
            "{label} contains a non-normal component"
        );
    }
    Ok(bytes.to_vec())
}

fn expected_old_stage_path(target: &[u8], upload_id: &[u8]) -> anyhow::Result<Vec<u8>> {
    let slash = target.iter().rposition(|byte| *byte == b'/');
    let (parent, filename) = slash.map_or((&b""[..], target), |index| {
        (&target[..index], &target[index + 1..])
    });
    ensure!(!filename.is_empty(), "Dufs target has no filename");
    let lossy = String::from_utf8_lossy(filename);
    let tag = lower_hex(&Sha256::digest(lossy.as_bytes()));
    let uuid = uuid::Uuid::from_slice(upload_id).context("Dufs upload_id is not a UUID")?;
    let name = format!(".dufs-upload-{tag}-{uuid}.part");
    let mut result = Vec::new();
    if !parent.is_empty() {
        result.extend_from_slice(parent);
        result.push(b'/');
    }
    result.extend_from_slice(OLD_STAGE_DIRECTORY);
    result.push(b'/');
    result.extend_from_slice(name.as_bytes());
    Ok(result)
}

fn parent_before_private_dir(stage: &[u8], private: &[u8]) -> anyhow::Result<Vec<u8>> {
    let components = stage.split(|byte| *byte == b'/').collect::<Vec<_>>();
    ensure!(
        components.len() >= 2 && components[components.len() - 2] == private,
        "Dufs stage is not inside the exact private directory"
    );
    Ok(components[..components.len() - 2].join(&b'/'))
}

fn append_component(parent: &[u8], component: &[u8]) -> Vec<u8> {
    let mut result = parent.to_vec();
    if !result.is_empty() {
        result.push(b'/');
    }
    result.extend_from_slice(component);
    result
}

fn rewrite_stage_path(path: &[u8]) -> anyhow::Result<Vec<u8>> {
    let parent = parent_before_private_dir(path, OLD_STAGE_DIRECTORY)?;
    let name = path
        .rsplit(|byte| *byte == b'/')
        .next()
        .context("Dufs stage path has no name")?;
    let mut result = append_component(&parent, NEW_STAGE_DIRECTORY);
    result.push(b'/');
    result.extend_from_slice(name);
    Ok(result)
}

fn create_target_database(
    source: &Path,
    target: &Path,
    owners: &BTreeMap<Vec<u8>, Vec<u8>>,
    expected: &DufsDatabaseSummary,
    root: tree::RootIdentity,
    service_uid: u32,
    service_gid: u32,
) -> anyhow::Result<SchemaIdentity> {
    create_private_empty_file(target)?;
    let mut connection = Connection::open_with_flags(
        target,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
    connection.pragma_update(None, "synchronous", "EXTRA")?;
    connection.execute_batch(CURRENT_SCHEMA_SQL)?;
    ensure!(
        schema_fingerprint_connection(&connection)? == SCHEMA_SHA256,
        "embedded Dufs target schema does not match the official fingerprint"
    );
    connection.execute(
        "INSERT INTO product_metadata(singleton,application,application_version,schema_revision,schema_sha256) VALUES (1,?1,?2,?3,?4)",
        params![Product::DufsRam.slug(), TO_VERSION, TARGET_REVISION as i64, SCHEMA_SHA256],
    )?;
    let source_uri = sqlite_read_only_uri(source)?;
    connection.execute("ATTACH DATABASE ?1 AS legacy", [source_uri])?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO store_meta(key,value) SELECT key,value FROM legacy.store_meta",
        [],
    )?;
    copy_owner_table(&transaction, "operations", owners, |tx, old, new| {
        tx.execute(
            "INSERT INTO operations SELECT ?1, operation_id, fingerprint, lease_token, state, terminal_state, http_status, error_code, created_at_ms, updated_at_ms, expires_at_ms FROM legacy.operations WHERE owner_digest=?2",
            params![new, old],
        ).map(|_| ()).map_err(Into::into)
    })?;
    copy_owner_table(&transaction, "upload_sessions", owners, |tx, old, new| {
        let mut select = tx.prepare("SELECT upload_id,target_path,stage_path,upload_length,durable_offset,state,stage_device_be,stage_inode_be,target_revision,updated_at_ms,expires_at_ms FROM legacy.upload_sessions WHERE owner_digest=?1")?;
        let rows = select.query_map([old], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<Vec<u8>>>(6)?,
                row.get::<_, Option<Vec<u8>>>(7)?,
                row.get::<_, Option<Vec<u8>>>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
            ))
        })?;
        for row in rows {
            let (
                id,
                target,
                stage,
                length,
                offset,
                state,
                device,
                inode,
                revision,
                updated,
                expires,
            ) = row?;
            tx.execute(
                "INSERT INTO upload_sessions VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    new,
                    id,
                    target,
                    rewrite_stage_path(&stage)?,
                    length,
                    offset,
                    state,
                    device,
                    inode,
                    revision,
                    updated,
                    expires
                ],
            )?;
        }
        Ok(())
    })?;
    copy_owner_table(&transaction, "purge_jobs", owners, |tx, old, new| {
        tx.execute(
            "INSERT INTO purge_jobs SELECT ?1, job_id, target_path, trash_path, source_device_be, source_inode_be, trash_revision, is_directory, state, attempts, next_attempt_at_ms, created_at_ms, updated_at_ms FROM legacy.purge_jobs WHERE owner_digest=?2",
            params![new, old],
        ).map(|_| ()).map_err(Into::into)
    })?;
    transaction.commit()?;
    connection.execute_batch("DETACH DATABASE legacy; PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(connection);
    let target_file = OpenOptions::new().read(true).write(true).open(target)?;
    let target_stat = fstat(&target_file)?;
    if target_stat.st_uid != service_uid || target_stat.st_gid != service_gid {
        fchown(
            &target_file,
            Some(Uid::from_raw(service_uid)),
            Some(Gid::from_raw(service_gid)),
        )?;
    }
    fs::set_permissions(target, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    target_file.sync_all()?;
    sync_directory(target.parent().context("Dufs target must have a parent")?)?;
    let identity = verify_target_database(target, root)?;
    let actual = inspect_record_counts(target)?;
    ensure!(
        actual == expected.records,
        "Dufs target row counts do not match source"
    );
    verify_mapped_rows(source, target, owners)?;
    Ok(identity)
}

fn copy_owner_table(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    owners: &BTreeMap<Vec<u8>, Vec<u8>>,
    mut copy: impl FnMut(&rusqlite::Transaction<'_>, &[u8], &[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut statement = transaction.prepare(&format!(
        "SELECT DISTINCT owner_digest FROM legacy.{table} ORDER BY owner_digest"
    ))?;
    let old = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for owner in old {
        copy(
            transaction,
            &owner,
            owners.get(&owner).context("unresolved owner during copy")?,
        )?;
    }
    Ok(())
}

fn verify_mapped_rows(
    source: &Path,
    target: &Path,
    owners: &BTreeMap<Vec<u8>, Vec<u8>>,
) -> anyhow::Result<()> {
    let source_uri = sqlite_read_only_uri(source)?;
    let target_uri = sqlite_read_only_uri(target)?;
    let connection = Connection::open_in_memory()?;
    connection.execute("ATTACH DATABASE ?1 AS old", [source_uri])?;
    connection.execute("ATTACH DATABASE ?1 AS new", [target_uri])?;
    for (old, new) in owners {
        for (table, columns) in [
            (
                "operations",
                "operation_id,fingerprint,lease_token,state,terminal_state,http_status,error_code,created_at_ms,updated_at_ms,expires_at_ms",
            ),
            (
                "purge_jobs",
                "job_id,target_path,trash_path,source_device_be,source_inode_be,trash_revision,is_directory,state,attempts,next_attempt_at_ms,created_at_ms,updated_at_ms",
            ),
        ] {
            let missing: i64 = connection.query_row(&format!("SELECT COUNT(*) FROM (SELECT {columns} FROM old.{table} WHERE owner_digest=?1 EXCEPT SELECT {columns} FROM new.{table} WHERE owner_digest=?2)"), params![old,new], |row| row.get(0))?;
            ensure!(
                missing == 0,
                "Dufs {table} payload changed during owner remap"
            );
        }
        let mut source_rows = connection.prepare("SELECT upload_id,target_path,stage_path,upload_length,durable_offset,state,stage_device_be,stage_inode_be,target_revision,updated_at_ms,expires_at_ms FROM old.upload_sessions WHERE owner_digest=?1 ORDER BY upload_id")?;
        let old_rows = source_rows
            .query_map([old], row_as_values)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut target_rows = connection.prepare("SELECT upload_id,target_path,stage_path,upload_length,durable_offset,state,stage_device_be,stage_inode_be,target_revision,updated_at_ms,expires_at_ms FROM new.upload_sessions WHERE owner_digest=?1 ORDER BY upload_id")?;
        let new_rows = target_rows
            .query_map([new], row_as_values)?
            .collect::<Result<Vec<_>, _>>()?;
        ensure!(
            old_rows.len() == new_rows.len(),
            "Dufs upload row count changed during remap"
        );
        for (mut old_row, new_row) in old_rows.into_iter().zip(new_rows) {
            let old_stage = match &old_row[2] {
                rusqlite::types::Value::Blob(value) => value,
                _ => anyhow::bail!("Dufs upload stage_path is not a BLOB"),
            };
            old_row[2] = rusqlite::types::Value::Blob(rewrite_stage_path(old_stage)?);
            ensure!(
                old_row == new_row,
                "Dufs upload payload changed beyond the exact stage path rewrite"
            );
        }
    }
    Ok(())
}

fn row_as_values(row: &rusqlite::Row<'_>) -> rusqlite::Result<Vec<rusqlite::types::Value>> {
    (0..11).map(|index| row.get(index)).collect()
}

fn create_source_backup(
    options: &DufsUpgradeOptions,
    config: &mut ConfigAnchor,
    root: &RootAnchor,
    source_clone: &SourceClone,
    raw_source: &RawGenerationSnapshot,
    identity: &SchemaIdentity,
    summary: &DufsDatabaseSummary,
) -> anyhow::Result<PathBuf> {
    let mut pending = PendingDirectory::create(&options.backup_output)?;
    let output = pending.path();
    create_private_empty_file(&output.join(BACKUP_DATABASE_FILE))?;
    copy_database_online(&source_clone.database(), &output.join(BACKUP_DATABASE_FILE))?;
    ensure!(
        verify_source_database(&output.join(BACKUP_DATABASE_FILE), root.identity())? == *identity,
        "canonical Dufs backup identity changed"
    );
    copy_config(config, &output.join(BACKUP_CONFIG_FILE))?;
    let raw_database_generation = copy_raw_generation(raw_source, &output)?;
    let tree_inventory =
        root.backup_tree(&output.join(BACKUP_TREE_DIRECTORY), options.tree_budget)?;
    source_clone.ensure_source_unchanged()?;
    root.ensure_unchanged()?;
    config.ensure_unchanged()?;
    let (database_bytes, database_sha256) = hash_regular_file(&output.join(BACKUP_DATABASE_FILE))?;
    let (config_bytes, config_sha256) = hash_regular_file(&output.join(BACKUP_CONFIG_FILE))?;
    let manifest = DufsCompositeBackupManifest {
        manifest_version: BACKUP_MANIFEST_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        product: Product::DufsRam,
        from_version: FROM_VERSION.to_owned(),
        to_version: TO_VERSION.to_owned(),
        source_tag_commit: SOURCE_TAG_COMMIT.to_owned(),
        target_source_commit: TARGET_SOURCE_COMMIT.to_owned(),
        source_schema_identity: identity.clone(),
        created_at_epoch_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("clock predates Unix epoch")?
            .as_secs(),
        database: DufsStoredResource {
            path: BACKUP_DATABASE_FILE.to_owned(),
            bytes: database_bytes,
            sha256: database_sha256,
        },
        raw_database_generation,
        config: config.metadata(),
        config_file: DufsStoredResource {
            path: BACKUP_CONFIG_FILE.to_owned(),
            bytes: config_bytes,
            sha256: config_sha256,
        },
        root_binding_sha256: root.identity().sha256(),
        tree: tree_inventory,
        database_records: summary.records.clone(),
        tree_budget: options.tree_budget,
    };
    validate_backup_manifest(&manifest, config, root)?;
    write_json_create_new(&output.join(BACKUP_MANIFEST_FILE), &manifest)?;
    sync_directory(&output)?;
    pending.commit()?;
    absolute_path(&options.backup_output)
}

fn copy_config(config: &ConfigAnchor, destination: &Path) -> anyhow::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    file.write_all(config.bytes())?;
    file.sync_all()?;
    Ok(())
}

fn validate_backup_manifest(
    manifest: &DufsCompositeBackupManifest,
    config: &ConfigAnchor,
    root: &RootAnchor,
) -> anyhow::Result<()> {
    ensure!(
        manifest.manifest_version == BACKUP_MANIFEST_VERSION
            && manifest.product == Product::DufsRam
            && manifest.from_version == FROM_VERSION
            && manifest.to_version == TO_VERSION
            && manifest.source_tag_commit == SOURCE_TAG_COMMIT
            && manifest.target_source_commit == TARGET_SOURCE_COMMIT,
        "Dufs backup manifest adapter identity is not exact"
    );
    ensure!(
        manifest.source_schema_identity
            == SchemaIdentity {
                application: Product::DufsRam.slug().to_owned(),
                application_version: FROM_VERSION.to_owned(),
                schema_revision: SOURCE_USER_VERSION as u64,
                schema_sha256: SCHEMA_SHA256.to_owned()
            },
        "Dufs backup source identity is not exact"
    );
    ensure!(
        manifest.config == config.metadata() && manifest.config_file.sha256 == config.sha256(),
        "Dufs backup config identity mismatch"
    );
    ensure!(
        manifest.database.path == BACKUP_DATABASE_FILE
            && manifest.config_file.path == BACKUP_CONFIG_FILE
            && manifest.root_binding_sha256 == root.identity().sha256(),
        "Dufs backup resource or root binding mismatch"
    );
    validate_raw_generation_contract(&manifest.raw_database_generation)?;
    manifest.tree_budget.validate()?;
    Ok(())
}

fn copy_raw_generation(
    source: &RawGenerationSnapshot,
    output: &Path,
) -> anyhow::Result<Vec<DufsStoredResource>> {
    let raw_directory = output.join(BACKUP_RAW_GENERATION_DIRECTORY);
    fs::create_dir(&raw_directory)?;
    fs::set_permissions(
        &raw_directory,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;
    let mut resources = Vec::new();
    for resource in &source.resources {
        let source_path = source.pending.path().join(&resource.path);
        let name = resource.path.clone();
        let destination = raw_directory.join(&name);
        copy_regular_exact(&source_path, &destination)?;
        let (bytes, sha256) = hash_regular_file(&destination)?;
        ensure!(
            bytes == resource.bytes && sha256 == resource.sha256,
            "Dufs raw backup differs from its pre-recovery source snapshot"
        );
        resources.push(DufsStoredResource {
            path: format!("{BACKUP_RAW_GENERATION_DIRECTORY}/{name}"),
            bytes,
            sha256,
        });
    }
    sync_directory(&raw_directory)?;
    validate_raw_generation_contract(&resources)?;
    Ok(resources)
}

fn copy_regular_exact(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let mut source_file = File::open(source)?;
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    std::io::copy(&mut source_file, &mut destination_file)?;
    destination_file.sync_all()?;
    ensure!(
        hash_regular_file(source)? == hash_regular_file(destination)?,
        "Dufs raw generation copy differs from its source"
    );
    Ok(())
}

fn validate_raw_generation_contract(resources: &[DufsStoredResource]) -> anyhow::Result<()> {
    ensure!(
        !resources.is_empty() && resources.len() <= 4,
        "Dufs raw generation resource count is invalid"
    );
    let expected = std::iter::once("")
        .chain(super::SQLITE_SIDECARS)
        .map(|suffix| format!("{BACKUP_RAW_GENERATION_DIRECTORY}/{BACKUP_DATABASE_FILE}{suffix}"))
        .collect::<BTreeSet<_>>();
    let actual = resources
        .iter()
        .map(|resource| resource.path.clone())
        .collect::<BTreeSet<_>>();
    ensure!(
        actual.len() == resources.len() && actual.is_subset(&expected),
        "Dufs raw generation paths are invalid or duplicate"
    );
    ensure!(
        actual.contains(&format!(
            "{BACKUP_RAW_GENERATION_DIRECTORY}/{BACKUP_DATABASE_FILE}"
        )),
        "Dufs raw generation lacks its main database"
    );
    Ok(())
}

fn verify_raw_generation(
    directory: &SecureDirectory,
    resources: &[DufsStoredResource],
) -> anyhow::Result<()> {
    validate_raw_generation_contract(resources)?;
    let raw_fd = openat2(
        &directory.file,
        BACKUP_RAW_GENERATION_DIRECTORY,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        secure_resolve_flags(),
    )?;
    let raw = SecureDirectory {
        file: File::from(raw_fd),
    };
    let expected_names = resources
        .iter()
        .map(|resource| {
            Path::new(&resource.path)
                .file_name()
                .expect("validated path")
                .to_string_lossy()
                .into_owned()
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        raw.entry_names()?.into_iter().collect::<BTreeSet<_>>() == expected_names,
        "Dufs raw generation entry set mismatch"
    );
    for resource in resources {
        verify_stored_file(directory, resource)?;
    }
    Ok(())
}

fn verify_stored_file(
    directory: &SecureDirectory,
    resource: &DufsStoredResource,
) -> anyhow::Result<()> {
    ensure!(
        Path::new(&resource.path)
            .components()
            .all(|component| matches!(component, Component::Normal(_))),
        "Dufs stored resource path is invalid"
    );
    let (bytes, sha256) = hash_regular_file(&directory.child_path(&resource.path))?;
    ensure!(
        bytes == resource.bytes && sha256 == resource.sha256,
        "Dufs stored resource checksum mismatch"
    );
    Ok(())
}

fn write_json_create_new(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("String writes cannot fail");
    }
    output
}

fn encode_path(path: &[u8]) -> String {
    STANDARD_NO_PAD.encode(path)
}
fn decode_path(path: &str) -> anyhow::Result<Vec<u8>> {
    STANDARD_NO_PAD.decode(path).context("decode raw Dufs path")
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        ffi::{OsStr, OsString},
        fs,
        os::unix::{
            ffi::{OsStrExt, OsStringExt},
            fs::{FileExt, MetadataExt, PermissionsExt},
        },
        process::Command,
    };

    use super::*;
    use rustix::{
        fs::{FlockOperation, flock},
        process::{getegid, geteuid},
    };

    const TEST_ACCOUNT: &str = "alice:$argon2id$v=19$m=19456,t=2,p=1$HdPI2G8k0h+yEgnqIt2rSw$P+MRyz7wH+b/iPY+He/9DApcy6yB9TAoo7j2JG1Smzs";
    const ALICE_OLD: &str = "2bd806c97f0e00af1a1fc3328fa763a9269723c8db8fac4f93af71db186d6e90";
    const ALICE_NEW: &str = "35ad2994ff78c7cbd371449a5f087bd5bd23f766c5cd46825ca6be1a2addb5e4";

    struct Fixture {
        _temporary: tempfile::TempDir,
        database: PathBuf,
        backup: PathBuf,
        config: PathBuf,
        shared_root: PathBuf,
        state_dir: PathBuf,
        old_stage_directory: PathBuf,
        new_stage_directory: PathBuf,
        stage_name: String,
        service_uid: u32,
        service_gid: u32,
    }

    impl Fixture {
        fn new() -> Self {
            let temporary = tempfile::tempdir().unwrap();
            let shared_root = temporary.path().join("shared");
            let state_dir = temporary.path().join("state");
            fs::create_dir(&shared_root).unwrap();
            fs::create_dir(&state_dir).unwrap();
            fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
            let service_uid = geteuid().as_raw();
            let service_gid = getegid().as_raw();
            let database = state_dir.join("state.sqlite3");
            let config = temporary.path().join("dufs.yml");
            let backup = temporary.path().join("source-backup");
            fs::write(
                &config,
                format!(
                    "serve-path: '{}'\nstate-dir: '{}'\nauth:\n  - '{}'\n",
                    shared_root.display(),
                    state_dir.display(),
                    TEST_ACCOUNT
                ),
            )
            .unwrap();
            fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();

            let docs = shared_root.join("docs");
            fs::create_dir(&docs).unwrap();
            let old_stage_directory = docs.join(OsStr::from_bytes(OLD_STAGE_DIRECTORY));
            let new_stage_directory = docs.join(OsStr::from_bytes(NEW_STAGE_DIRECTORY));
            fs::create_dir(&old_stage_directory).unwrap();
            fs::set_permissions(&old_stage_directory, fs::Permissions::from_mode(0o700)).unwrap();
            let upload_id = [4_u8; 16];
            let target = b"docs/file.txt";
            let old_stage = expected_old_stage_path(target, &upload_id).unwrap();
            let stage_name = Path::new(OsStr::from_bytes(&old_stage))
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            let stage_path = old_stage_directory.join(&stage_name);
            fs::write(&stage_path, b"partial").unwrap();
            fs::set_permissions(&stage_path, fs::Permissions::from_mode(0o600)).unwrap();
            let stage_metadata = fs::symlink_metadata(&stage_path).unwrap();
            fs::write(shared_root.join("ordinary.txt"), b"ordinary-data").unwrap();
            let purge_id = [5_u8; 16];
            let purge_uuid = uuid::Uuid::from_bytes(purge_id);
            let trash_name = format!(".dufs-upload-delete-{purge_uuid}.trash");
            let trash = shared_root.join(&trash_name);
            fs::write(&trash, b"pending-delete").unwrap();
            let trash_metadata = fs::symlink_metadata(&trash).unwrap();

            let connection = Connection::open(&database).unwrap();
            connection.execute_batch(SOURCE_SCHEMA_SQL).unwrap();
            connection
                .pragma_update(None, "application_id", SOURCE_APPLICATION_ID)
                .unwrap();
            connection
                .pragma_update(None, "user_version", SOURCE_USER_VERSION)
                .unwrap();
            let root_metadata = fs::symlink_metadata(&shared_root).unwrap();
            connection.execute(
                "INSERT INTO store_meta(key,value) VALUES ('root-device-be',?1),('root-inode-be',?2)",
                params![root_metadata.dev().to_be_bytes().as_slice(), root_metadata.ino().to_be_bytes().as_slice()],
            ).unwrap();
            let owner = Sha256::digest(b"alice").to_vec();
            connection.execute(
                "INSERT INTO operations(owner_digest,operation_id,fingerprint,lease_token,state,terminal_state,http_status,error_code,created_at_ms,updated_at_ms,expires_at_ms) VALUES (?1,?2,?3,?4,0,NULL,NULL,NULL,1,2,NULL)",
                params![owner, [1_u8;16].as_slice(), [2_u8;32].as_slice(), [3_u8;16].as_slice()],
            ).unwrap();
            connection.execute(
                "INSERT INTO upload_sessions(owner_digest,upload_id,target_path,stage_path,upload_length,durable_offset,state,stage_device_be,stage_inode_be,target_revision,updated_at_ms,expires_at_ms) VALUES (?1,?2,?3,?4,10,7,0,?5,?6,?7,3,4)",
                params![owner, upload_id.as_slice(), target.as_slice(), old_stage, stage_metadata.dev().to_be_bytes().as_slice(), stage_metadata.ino().to_be_bytes().as_slice(), [7_u8;32].as_slice()],
            ).unwrap();
            connection.execute(
                "INSERT INTO purge_jobs(owner_digest,job_id,target_path,trash_path,source_device_be,source_inode_be,trash_revision,is_directory,state,attempts,next_attempt_at_ms,created_at_ms,updated_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,0,1,0,5,6,7)",
                params![owner, purge_id.as_slice(), b"victim.bin".as_slice(), trash_name.as_bytes(), trash_metadata.dev().to_be_bytes().as_slice(), trash_metadata.ino().to_be_bytes().as_slice(), [8_u8;32].as_slice()],
            ).unwrap();
            assert_eq!(
                schema_fingerprint_connection(&connection).unwrap(),
                SCHEMA_SHA256
            );
            drop(connection);
            fs::set_permissions(&database, fs::Permissions::from_mode(0o600)).unwrap();

            Self {
                _temporary: temporary,
                database,
                backup,
                config,
                shared_root,
                state_dir,
                old_stage_directory,
                new_stage_directory,
                stage_name,
                service_uid,
                service_gid,
            }
        }

        fn options(&self) -> DufsUpgradeOptions {
            DufsUpgradeOptions {
                product: Product::DufsRam,
                from_version: FROM_VERSION.to_owned(),
                to_version: TO_VERSION.to_owned(),
                database: self.database.clone(),
                backup_output: self.backup.clone(),
                config: self.config.clone(),
                shared_root: self.shared_root.clone(),
                state_dir: self.state_dir.clone(),
                service_uid: self.service_uid,
                service_gid: self.service_gid,
                tree_budget: DufsTreeBudget {
                    max_entries: 100,
                    max_logical_bytes: 1024 * 1024,
                    max_backup_bytes: 1024 * 1024,
                    max_entries_per_directory: 100,
                },
            }
        }

        fn recovery_options(&self, action: RecoveryAction) -> DufsRecoveryOptions {
            DufsRecoveryOptions {
                product: Product::DufsRam,
                from_version: FROM_VERSION.to_owned(),
                to_version: TO_VERSION.to_owned(),
                database: self.database.clone(),
                config: self.config.clone(),
                shared_root: self.shared_root.clone(),
                state_dir: self.state_dir.clone(),
                service_uid: self.service_uid,
                service_gid: self.service_gid,
                recovery_directory: self
                    .state_dir
                    .join(".state.sqlite3.dufs-ram.upgrade-recovery"),
                action,
            }
        }
    }

    fn generation_bytes(path: &Path) -> BTreeMap<String, Vec<u8>> {
        std::iter::once("")
            .chain(super::super::SQLITE_SIDECARS)
            .filter_map(|suffix| {
                let path = PathBuf::from(format!("{}{suffix}", path.display()));
                path.exists()
                    .then(|| (suffix.to_owned(), fs::read(path).unwrap()))
            })
            .collect()
    }

    fn hex(value: &[u8]) -> String {
        lower_hex(value)
    }

    fn decode_hex(value: &str) -> Vec<u8> {
        let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
        assert!(remainder.is_empty());
        pairs
            .iter()
            .map(|pair| {
                let text = std::str::from_utf8(pair).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }

    #[test]
    fn pinned_contract_and_owner_golden_values_are_exact() {
        let fixture = Fixture::new();
        let root = RootAnchor::open_unlocked(
            &fixture.shared_root,
            fixture.service_uid,
            fixture.service_gid,
        )
        .unwrap();
        let source = verify_source_database(&fixture.database, root.identity()).unwrap();
        assert_eq!(source.schema_sha256, SCHEMA_SHA256);
        let mapping = owner_mapping(&["alice".to_owned()]).unwrap();
        let old = Sha256::digest(b"alice").to_vec();
        assert_eq!(hex(&old), ALICE_OLD);
        assert_eq!(hex(&mapping[&old]), ALICE_NEW);
        assert_eq!(
            SOURCE_TAG_COMMIT,
            "5b098e2a8f05557b72efdf7929f4ccef3a3af837"
        );
        assert_eq!(
            TARGET_SOURCE_COMMIT,
            "2369bd990abf4c1492ca16178f2f66765104be25"
        );
    }

    #[test]
    fn digest_and_target_primary_key_collisions_are_rejected() {
        let users = ["alice".to_owned(), "bob".to_owned()];
        assert!(owner_mapping_with(&users, |_| (vec![1; 32], vec![2; 32])).is_err());
        assert!(
            owner_mapping_with(&users, |username| {
                let source = if username == "alice" {
                    vec![1; 32]
                } else {
                    vec![3; 32]
                };
                (source, vec![2; 32])
            })
            .is_err()
        );

        let fixture = Fixture::new();
        let connection = Connection::open(&fixture.database).unwrap();
        connection.execute(
            "INSERT INTO operations(owner_digest,operation_id,fingerprint,lease_token,state,terminal_state,http_status,error_code,created_at_ms,updated_at_ms,expires_at_ms) VALUES (?1,?2,?3,?4,0,NULL,NULL,NULL,1,2,NULL)",
            params![[9_u8;32].as_slice(), [1_u8;16].as_slice(), [2_u8;32].as_slice(), [3_u8;16].as_slice()],
        ).unwrap();
        let old_alice = Sha256::digest(b"alice").to_vec();
        let owners = BTreeMap::from([
            (old_alice, vec![8_u8; 32]),
            (vec![9_u8; 32], vec![8_u8; 32]),
        ]);
        assert!(validate_target_primary_keys(&connection, &owners).is_err());
    }

    #[test]
    fn upgrades_exact_dufs_fixture_and_preserves_authorization_payloads() {
        let fixture = Fixture::new();
        let source_bytes = generation_bytes(&fixture.database);
        let old_stage_metadata = fs::symlink_metadata(&fixture.old_stage_directory).unwrap();
        let result = upgrade_dufs(&fixture.options()).unwrap();
        assert_eq!(result.schema_identity.application_version, TO_VERSION);
        assert!(!fixture.old_stage_directory.exists());
        assert_eq!(
            fs::read(fixture.new_stage_directory.join(&fixture.stage_name)).unwrap(),
            b"partial"
        );
        let new_stage_metadata = fs::symlink_metadata(&fixture.new_stage_directory).unwrap();
        assert_eq!(
            (new_stage_metadata.dev(), new_stage_metadata.ino()),
            (old_stage_metadata.dev(), old_stage_metadata.ino())
        );

        let connection = Connection::open(&fixture.database).unwrap();
        let expected_owner = decode_hex(ALICE_NEW);
        for table in ["operations", "upload_sessions", "purge_jobs"] {
            let owner: Vec<u8> = connection
                .query_row(&format!("SELECT owner_digest FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(owner, expected_owner, "{table}");
        }
        let revision: Vec<u8> = connection
            .query_row("SELECT target_revision FROM upload_sessions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(revision, vec![7_u8; 32]);
        let stage: Vec<u8> = connection
            .query_row("SELECT stage_path FROM upload_sessions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(
            stage
                .windows(NEW_STAGE_DIRECTORY.len())
                .any(|window| window == NEW_STAGE_DIRECTORY)
        );
        drop(connection);

        let backup = verify_dufs_source_backup(
            Product::DufsRam,
            FROM_VERSION,
            TO_VERSION,
            &fixture.backup,
            &fixture.config,
            &fixture.shared_root,
            fixture.service_uid,
            fixture.service_gid,
        )
        .unwrap();
        assert_eq!(
            fs::read(fixture.backup.join("raw-generation/database.sqlite3")).unwrap(),
            source_bytes[""]
        );
        assert!(backup.manifest.config.sensitive);
        assert!(
            !serde_json::to_string(&backup.manifest)
                .unwrap()
                .contains("alice")
        );
    }

    #[test]
    fn rejects_wrong_identity_unknown_owner_and_path_escape_before_backup_or_mutation() {
        for mutation in 0..5 {
            let fixture = Fixture::new();
            let connection = Connection::open(&fixture.database).unwrap();
            match mutation {
                0 => connection.pragma_update(None, "user_version", 4).unwrap(),
                1 => connection.pragma_update(None, "application_id", 0).unwrap(),
                2 => {
                    connection
                        .execute(
                            "UPDATE operations SET owner_digest=?1",
                            [[99_u8; 32].as_slice()],
                        )
                        .unwrap();
                }
                3 => {
                    connection
                        .execute(
                            "UPDATE upload_sessions SET stage_path=?1",
                            [b"../escape".as_slice()],
                        )
                        .unwrap();
                }
                4 => connection
                    .execute_batch("CREATE TABLE nearby(id INTEGER PRIMARY KEY);")
                    .unwrap(),
                _ => unreachable!(),
            }
            drop(connection);
            let before = generation_bytes(&fixture.database);
            assert!(upgrade_dufs(&fixture.options()).is_err());
            assert_eq!(generation_bytes(&fixture.database), before);
            assert!(fixture.old_stage_directory.exists());
            assert!(!fixture.backup.exists());
        }
    }

    #[test]
    fn missing_active_stage_and_wrong_purge_identity_fail_before_backup() {
        let missing = Fixture::new();
        fs::remove_file(missing.old_stage_directory.join(&missing.stage_name)).unwrap();
        let before = generation_bytes(&missing.database);
        assert!(upgrade_dufs(&missing.options()).is_err());
        assert_eq!(generation_bytes(&missing.database), before);
        assert!(!missing.backup.exists());

        let wrong_purge = Fixture::new();
        Connection::open(&wrong_purge.database)
            .unwrap()
            .execute(
                "UPDATE purge_jobs SET source_inode_be=?1",
                [u64::MAX.to_be_bytes().as_slice()],
            )
            .unwrap();
        let before = generation_bytes(&wrong_purge.database);
        assert!(upgrade_dufs(&wrong_purge.options()).is_err());
        assert_eq!(generation_bytes(&wrong_purge.database), before);
        assert!(!wrong_purge.backup.exists());
    }

    #[test]
    fn protected_config_errors_do_not_disclose_credentials() {
        let insecure = Fixture::new();
        fs::set_permissions(&insecure.config, fs::Permissions::from_mode(0o644)).unwrap();
        let error = upgrade_dufs(&insecure.options()).unwrap_err();
        let report = format!("{error:#}");
        assert!(!report.contains("alice") && !report.contains("argon2"));
        assert!(!insecure.backup.exists());

        let duplicate = Fixture::new();
        fs::write(
            &duplicate.config,
            format!(
                "serve-path: '{}'\nstate-dir: '{}'\nauth:\n  - '{}'\n  - '{}'\n",
                duplicate.shared_root.display(),
                duplicate.state_dir.display(),
                TEST_ACCOUNT,
                TEST_ACCOUNT,
            ),
        )
        .unwrap();
        fs::set_permissions(&duplicate.config, fs::Permissions::from_mode(0o600)).unwrap();
        let report = format!("{:#}", upgrade_dufs(&duplicate.options()).unwrap_err());
        assert!(!report.contains("alice") && !report.contains("argon2"));
    }

    #[test]
    fn composite_tree_backup_preserves_sparse_hardlink_non_utf8_and_symlink_entries() {
        let fixture = Fixture::new();
        let first = fixture.shared_root.join("hard-a");
        let second = fixture.shared_root.join("hard-b");
        fs::write(&first, b"one-inode").unwrap();
        fs::hard_link(&first, &second).unwrap();
        let sparse = fixture.shared_root.join("sparse.bin");
        let sparse_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&sparse)
            .unwrap();
        sparse_file.set_len(2 * 1024 * 1024).unwrap();
        sparse_file.write_all_at(b"z", 2 * 1024 * 1024 - 1).unwrap();
        sparse_file.sync_all().unwrap();
        let raw_name = OsString::from_vec(vec![b'n', b'o', b'n', 0xff]);
        fs::write(fixture.shared_root.join(&raw_name), b"raw-name").unwrap();
        std::os::unix::fs::symlink("ordinary.txt", fixture.shared_root.join("relative-link"))
            .unwrap();

        let mut options = fixture.options();
        options.tree_budget.max_logical_bytes = 4 * 1024 * 1024;
        options.tree_budget.max_backup_bytes = 4 * 1024 * 1024;
        upgrade_dufs(&options).unwrap();
        let archived = fixture.backup.join(BACKUP_TREE_DIRECTORY);
        let hard_a = fs::symlink_metadata(archived.join("hard-a")).unwrap();
        let hard_b = fs::symlink_metadata(archived.join("hard-b")).unwrap();
        assert_eq!((hard_a.dev(), hard_a.ino()), (hard_b.dev(), hard_b.ino()));
        assert_eq!(fs::read(archived.join(raw_name)).unwrap(), b"raw-name");
        assert_eq!(
            fs::read_link(archived.join("relative-link")).unwrap(),
            Path::new("ordinary.txt")
        );
        assert_eq!(
            fs::symlink_metadata(archived.join("sparse.bin"))
                .unwrap()
                .len(),
            2 * 1024 * 1024
        );
    }

    #[test]
    fn namespace_collision_and_running_root_lock_fail_without_product_writes() {
        let fixture = Fixture::new();
        fs::write(
            fixture
                .shared_root
                .join(".dufs-readiness-11111111-1111-1111-1111-111111111111.probe"),
            b"old-user-data",
        )
        .unwrap();
        let before = generation_bytes(&fixture.database);
        assert!(upgrade_dufs(&fixture.options()).is_err());
        assert_eq!(generation_bytes(&fixture.database), before);
        assert!(!fixture.backup.exists());

        let locked = Fixture::new();
        let root_file = File::open(&locked.shared_root).unwrap();
        flock(&root_file, FlockOperation::NonBlockingLockExclusive).unwrap();
        assert!(upgrade_dufs(&locked.options()).is_err());
        assert!(!locked.backup.exists());

        let bounded = Fixture::new();
        let before = generation_bytes(&bounded.database);
        let mut options = bounded.options();
        options.tree_budget.max_entries_per_directory = 1;
        assert!(upgrade_dufs(&options).is_err());
        assert_eq!(generation_bytes(&bounded.database), before);
        assert!(!bounded.backup.exists());
    }

    #[test]
    fn interrupted_barrier_rolls_back_exact_bytes_and_installed_target_commits() {
        for (phase, action) in [
            (DufsJournalPhase::Prepared, RecoveryAction::Rollback),
            (DufsJournalPhase::Barrier, RecoveryAction::Rollback),
            (
                DufsJournalPhase::StageDirectoryMoved,
                RecoveryAction::Rollback,
            ),
            (DufsJournalPhase::TreeMoved, RecoveryAction::Rollback),
            (DufsJournalPhase::Installed, RecoveryAction::Commit),
            (DufsJournalPhase::Verified, RecoveryAction::Commit),
            (DufsJournalPhase::Committed, RecoveryAction::Commit),
        ] {
            let fixture = Fixture::new();
            let before = generation_bytes(&fixture.database);
            let result = upgrade_dufs_with_hook(&fixture.options(), |point| {
                if point == phase {
                    anyhow::bail!("simulated crash")
                }
                Ok(())
            });
            assert!(result.is_err());
            let recovered = recover_dufs_upgrade(&fixture.recovery_options(action)).unwrap();
            match action {
                RecoveryAction::Rollback => {
                    assert_eq!(generation_bytes(&fixture.database), before);
                    assert!(fixture.old_stage_directory.exists());
                    assert!(!fixture.new_stage_directory.exists());
                    assert_eq!(recovered.schema_identity.application_version, FROM_VERSION);
                }
                RecoveryAction::Commit => {
                    assert!(!fixture.old_stage_directory.exists());
                    assert!(fixture.new_stage_directory.exists());
                    assert_eq!(recovered.schema_identity.application_version, TO_VERSION);
                }
            }
            assert!(!fixture.recovery_options(action).recovery_directory.exists());
        }
    }

    #[test]
    fn partial_sidecar_barrier_recovers_from_an_unpersisted_phase() {
        for action in [RecoveryAction::Rollback, RecoveryAction::Commit] {
            let fixture = Fixture::new();
            let mut sidecar_name = fixture.database.as_os_str().to_os_string();
            sidecar_name.push("-journal");
            let sidecar = PathBuf::from(sidecar_name);
            fs::write(&sidecar, []).unwrap();
            fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o600)).unwrap();
            let before = generation_bytes(&fixture.database);
            upgrade_dufs_with_hook(&fixture.options(), |phase| {
                if phase == DufsJournalPhase::Barrier {
                    anyhow::bail!("simulated crash")
                }
                Ok(())
            })
            .unwrap_err();

            let recovery = fixture.recovery_options(action);
            let stored_sidecar = recovery.recovery_directory.join("original.sqlite3-journal");
            assert!(stored_sidecar.exists());
            fs::rename(&stored_sidecar, &sidecar).unwrap();
            let journal_path = recovery.recovery_directory.join("journal.json");
            let mut journal: serde_json::Value =
                serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
            journal["phase"] = serde_json::json!("prepared");
            fs::write(&journal_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();

            let result = recover_dufs_upgrade(&recovery).unwrap();
            match action {
                RecoveryAction::Rollback => {
                    assert_eq!(generation_bytes(&fixture.database), before);
                    assert_eq!(result.schema_identity.application_version, FROM_VERSION);
                }
                RecoveryAction::Commit => {
                    assert!(!sidecar.exists());
                    assert_eq!(result.schema_identity.application_version, TO_VERSION);
                    assert!(
                        fixture
                            .backup
                            .join("raw-generation/database.sqlite3-journal")
                            .exists()
                    );
                }
            }
        }
    }

    #[test]
    fn hot_delete_journal_is_backed_up_without_opening_the_original() {
        const CHILD_DATABASE: &str = "ISARMG_DUFS_HOT_JOURNAL_CHILD_DATABASE";
        if let Some(database) = std::env::var_os(CHILD_DATABASE) {
            let database = PathBuf::from(database);
            let connection = Connection::open(&database).unwrap();
            connection
                .execute_batch(
                    "PRAGMA journal_mode=DELETE; PRAGMA synchronous=EXTRA; \
                     PRAGMA cache_size=1; PRAGMA cache_spill=ON; BEGIN IMMEDIATE;",
                )
                .unwrap();
            let owner = Sha256::digest(b"alice").to_vec();
            for value in 100_u128..700 {
                let identifier = value.to_be_bytes();
                let fingerprint = Sha256::digest(identifier);
                connection.execute(
                    "INSERT INTO operations(owner_digest,operation_id,fingerprint,lease_token,state,terminal_state,http_status,error_code,created_at_ms,updated_at_ms,expires_at_ms) VALUES (?1,?2,?3,?4,0,NULL,NULL,NULL,1,2,NULL)",
                    params![owner, identifier.as_slice(), fingerprint.as_slice(), identifier.as_slice()],
                ).unwrap();
            }
            let mut journal_name = database.as_os_str().to_os_string();
            journal_name.push("-journal");
            let journal = PathBuf::from(journal_name);
            let bytes = fs::read(&journal).unwrap();
            assert!(
                bytes.len() > 512 && bytes[..8] == [0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7]
            );
            File::open(&journal).unwrap().sync_all().unwrap();
            File::open(&database).unwrap().sync_all().unwrap();
            std::process::exit(86);
        }

        let fixture = Fixture::new();
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("sqlite::upgrade::dufs_0_49_7_to_0_50_0::tests::hot_delete_journal_is_backed_up_without_opening_the_original")
            .arg("--nocapture")
            .env(CHILD_DATABASE, &fixture.database)
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(86),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let before = generation_bytes(&fixture.database);
        assert!(before["-journal"].len() > 512);

        upgrade_dufs_with_hook(&fixture.options(), |phase| {
            if phase == DufsJournalPhase::Prepared {
                anyhow::bail!("simulated crash")
            }
            Ok(())
        })
        .unwrap_err();
        assert_eq!(generation_bytes(&fixture.database), before);
        for (suffix, bytes) in &before {
            let archived = fixture
                .backup
                .join(format!("raw-generation/database.sqlite3{suffix}"));
            assert_eq!(&fs::read(archived).unwrap(), bytes);
        }
        let result =
            recover_dufs_upgrade(&fixture.recovery_options(RecoveryAction::Rollback)).unwrap();
        assert_eq!(result.schema_identity.application_version, FROM_VERSION);
        assert_eq!(generation_bytes(&fixture.database), before);
    }

    #[test]
    fn tampered_backup_and_journal_are_rejected_fail_closed() {
        let fixture = Fixture::new();
        upgrade_dufs_with_hook(&fixture.options(), |phase| {
            if phase == DufsJournalPhase::Barrier {
                anyhow::bail!("simulated crash")
            }
            Ok(())
        })
        .unwrap_err();
        let recovery = fixture.recovery_options(RecoveryAction::Rollback);
        let journal_path = recovery.recovery_directory.join("journal.json");
        let mut journal: serde_json::Value =
            serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
        journal["target_identity"]["schema_sha256"] = serde_json::json!("00".repeat(32));
        fs::write(&journal_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();
        assert!(recover_dufs_upgrade(&recovery).is_err());
        assert!(
            !fs::read(&fixture.database)
                .unwrap()
                .starts_with(b"SQLite format 3\0")
        );

        let stage_plan = Fixture::new();
        upgrade_dufs_with_hook(&stage_plan.options(), |phase| {
            if phase == DufsJournalPhase::Barrier {
                anyhow::bail!("simulated crash")
            }
            Ok(())
        })
        .unwrap_err();
        let recovery = stage_plan.recovery_options(RecoveryAction::Rollback);
        let journal_path = recovery.recovery_directory.join("journal.json");
        let mut journal: serde_json::Value =
            serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
        journal["stage_moves"] = serde_json::json!([]);
        fs::write(&journal_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();
        assert!(recover_dufs_upgrade(&recovery).is_err());
        assert!(
            !fs::read(&stage_plan.database)
                .unwrap()
                .starts_with(b"SQLite format 3\0")
        );

        let clean = Fixture::new();
        upgrade_dufs(&clean.options()).unwrap();
        OpenOptions::new()
            .append(true)
            .open(clean.backup.join(BACKUP_CONFIG_FILE))
            .unwrap()
            .write_all(b"tamper")
            .unwrap();
        assert!(
            verify_dufs_source_backup(
                Product::DufsRam,
                FROM_VERSION,
                TO_VERSION,
                &clean.backup,
                &clean.config,
                &clean.shared_root,
                clean.service_uid,
                clean.service_gid
            )
            .is_err()
        );

        let manifest = Fixture::new();
        upgrade_dufs(&manifest.options()).unwrap();
        let manifest_path = manifest.backup.join(BACKUP_MANIFEST_FILE);
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        document["source_tag_commit"] = serde_json::json!("0".repeat(40));
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&document).unwrap(),
        )
        .unwrap();
        assert!(
            verify_dufs_source_backup(
                Product::DufsRam,
                FROM_VERSION,
                TO_VERSION,
                &manifest.backup,
                &manifest.config,
                &manifest.shared_root,
                manifest.service_uid,
                manifest.service_gid
            )
            .is_err()
        );
    }
}

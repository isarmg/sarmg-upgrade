use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf, absolute},
    time::{SystemTime, UNIX_EPOCH},
};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use anyhow::{Context, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hkdf::Hkdf;
use rusqlite::{Connection, OpenFlags, backup::Backup};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{ExternalRequirement, Product, SchemaIdentity};

const MANIFEST_FILE: &str = "manifest.json";
const DATABASE_FILE: &str = "database.sqlite3";
const TREE_DIRECTORY: &str = "tree";
const CURRENT_MANIFEST_VERSION: u32 = 3;
const CURRENT_RESTORE_JOURNAL_VERSION: u32 = 3;
const MAX_MANIFEST_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CURRENT_JOURNAL_BYTES: u64 = 1024 * 1024;
const MAX_TREE_ENTRIES: u64 = 2_000_000;
const MAX_TREE_DEPTH: usize = 128;

pub(crate) const MEDIA_CURRENT_APPLICATION_VERSION: &str = "0.2.0";
const MEDIA_SCHEMA_REVISION: u64 = 1;
const MEDIA_SCHEMA_SHA256: &str =
    "2563e6afc3fff272d02b7a5615272cc773862243bfd15aec51655abf1d9c6b1c";
pub(crate) const SENTINEL_CURRENT_APPLICATION_VERSION: &str = "0.2.0";
const SENTINEL_SCHEMA_REVISION: u64 = 1;
const SENTINEL_SCHEMA_SHA256: &str =
    "f547ddc817d830d23b5305bb1f88b29898d6531568edd6eb194c2b629eb560c0";
const SENTINEL_KEY_ID: &str = "sentinel-credentials-0.2.0-key-1";
const SENTINEL_KEY_DERIVATION_SALT: &[u8] = b"sentinel-monitor/0.2.0/credential-envelope/key/v1";
const SENTINEL_KEY_DERIVATION_INFO: &[u8] = b"sentinel-credential-envelope/aes-256-gcm";
const SENTINEL_AAD_DOMAIN: &str = "sentinel-monitor/0.2.0/credential-envelope/aad/v1";
const SENTINEL_MAX_ENVELOPE_BYTES: usize = 64 * 1024;
const SENTINEL_MAX_PLAINTEXT_BYTES: usize = 16 * 1024;
pub(crate) const DUFS_CURRENT_APPLICATION_VERSION: &str = "0.50.1";
const DUFS_SCHEMA_REVISION: u64 = 1;
const DUFS_SCHEMA_SHA256: &str = "3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423";

#[derive(Clone)]
pub struct CompositeCurrentOptions {
    pub product: Product,
    pub database: PathBuf,
    pub tree: PathBuf,
    pub output: PathBuf,
    pub runtime_directory: Option<PathBuf>,
    pub configuration: Vec<NamedFile>,
    pub credentials_key_id: Option<String>,
    pub credentials_key: Option<[u8; 32]>,
}

impl std::fmt::Debug for CompositeCurrentOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompositeCurrentOptions")
            .field("product", &self.product)
            .field("database", &self.database)
            .field("tree", &self.tree)
            .field("output", &self.output)
            .field("runtime_directory", &self.runtime_directory)
            .field("configuration", &self.configuration)
            .field("credentials_key_id", &self.credentials_key_id)
            .field(
                "credentials_key",
                &self.credentials_key.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct NamedFile {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct CurrentRestoreOptions {
    pub product: Product,
    pub input: PathBuf,
    pub database: PathBuf,
    pub tree: PathBuf,
    pub runtime_directory: Option<PathBuf>,
    pub configuration: Vec<NamedFile>,
    pub replace_existing: bool,
    pub credentials_key_id: Option<String>,
    pub credentials_key: Option<[u8; 32]>,
}

#[derive(Clone, Debug)]
pub struct CurrentRecoveryOptions {
    pub product: Product,
    pub expected_application_version: String,
    pub input: PathBuf,
    pub database: PathBuf,
    pub tree: PathBuf,
    pub runtime_directory: Option<PathBuf>,
    pub recovery_directory: PathBuf,
    pub action: CurrentRecoveryAction,
    pub credentials_key_id: Option<String>,
    pub credentials_key: Option<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CurrentRecoveryAction {
    Commit,
    Rollback,
}

impl std::str::FromStr for CurrentRecoveryAction {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "commit" => Ok(Self::Commit),
            "rollback" => Ok(Self::Rollback),
            _ => anyhow::bail!("recovery action must be commit or rollback"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentBackupManifest {
    pub manifest_version: u32,
    pub adapter_id: String,
    pub tool_version: String,
    pub product: Product,
    pub application_version: String,
    pub schema_identity: SchemaIdentity,
    pub created_at_epoch_seconds: u64,
    pub source_tree_identity_sha256: String,
    pub database: CurrentFile,
    pub configuration: Vec<CurrentFile>,
    pub tree: TreeArchive,
    pub external_requirements: Vec<ExternalRequirement>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub mode: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TreeArchive {
    pub directory: String,
    pub mode: u32,
    pub directories: Vec<TreeDirectory>,
    pub files: Vec<TreeFile>,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TreeDirectory {
    pub path: String,
    pub mode: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TreeFile {
    pub path: String,
    pub mode: u32,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CurrentStateResult {
    pub product: Product,
    pub application_version: String,
    pub directory: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RestorePhase {
    Prepared,
    OriginalsPreserved,
    Installed,
    Verified,
    RollbackStarted,
    RollbackVerified,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RestoreJournal {
    journal_version: u32,
    tool_version: String,
    product: Product,
    application_version: String,
    adapter_id: String,
    schema_identity: SchemaIdentity,
    created_at_epoch_seconds: u64,
    source_backup: PathBuf,
    source_backup_identity_sha256: String,
    source_manifest_version: u32,
    source_manifest_created_at_epoch_seconds: u64,
    source_manifest_bytes: u64,
    source_manifest_sha256: String,
    source_tree_identity_sha256: String,
    database: PathBuf,
    tree: PathBuf,
    database_path_identity_sha256: String,
    tree_path_identity_sha256: String,
    database_stage: PathBuf,
    tree_stage: PathBuf,
    database_original: PathBuf,
    tree_original: PathBuf,
    incoming_database: CurrentFile,
    incoming_tree: TreeArchive,
    original_database: Option<CurrentFile>,
    original_tree: Option<TreeArchive>,
    configuration: Vec<CurrentFile>,
    configuration_targets: Vec<RestoreConfiguration>,
    external_requirements: Vec<ExternalRequirement>,
    phase: RestorePhase,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RestoreConfiguration {
    name: String,
    target: PathBuf,
    target_path_identity_sha256: String,
    stage: PathBuf,
    original: PathBuf,
    incoming: CurrentFile,
    original_file: Option<CurrentFile>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SentinelCredentialEnvelope {
    product: String,
    application_version: String,
    envelope_revision: u32,
    key_id: String,
    nonce: String,
    ciphertext: String,
}

pub fn backup_current(options: &CompositeCurrentOptions) -> anyhow::Result<CurrentStateResult> {
    validate_options(options)?;
    let _locks = ProductLocks::acquire(
        options.product,
        &options.database,
        &options.tree,
        options.runtime_directory.as_deref(),
    )?;
    let identity = verify_database(options.product, &options.database)?;
    verify_external_key(options.product, &options.database, options.credentials())?;
    verify_product_state(options.product, &options.database, &options.tree)?;
    verify_configuration(options.product, &options.configuration, &options.tree)?;

    // Both the SQLite-only and composite backup paths publish through the same
    // directory-FD based renameat2(RENAME_NOREPLACE) primitive.  In particular,
    // a competing creator can never turn this final publication into a clobber.
    let mut pending = crate::sqlite::PendingDirectory::create(&options.output)?;
    let pending_path = pending.path();
    let database_output = pending_path.join(DATABASE_FILE);
    copy_sqlite_snapshot(&options.database, &database_output)?;
    ensure!(
        verify_database(options.product, &database_output)? == identity,
        "database identity changed while the current backup was created"
    );
    verify_external_key(options.product, &database_output, options.credentials())?;

    let tree_output = pending_path.join(TREE_DIRECTORY);
    create_private_directory(&tree_output)?;
    copy_strict_tree(&options.tree, &tree_output, 0)?;
    let tree = inventory_tree(&tree_output)?;
    ensure!(
        tree == inventory_tree(&options.tree)?,
        "data tree changed while the current backup was created"
    );

    let mut configuration = Vec::new();
    for named in &options.configuration {
        validate_name(&named.name)?;
        let destination = pending_path.join(&named.name);
        copy_regular(&named.path, &destination)?;
        configuration.push(current_file(&named.name, &destination)?);
    }
    configuration.sort_by(|left, right| left.path.cmp(&right.path));
    ensure!(
        configuration
            .iter()
            .map(|entry| &entry.path)
            .collect::<BTreeSet<_>>()
            .len()
            == configuration.len(),
        "configuration backup names are duplicated"
    );

    verify_product_state(options.product, &database_output, &tree_output)?;
    let manifest = CurrentBackupManifest {
        manifest_version: CURRENT_MANIFEST_VERSION,
        adapter_id: format!(
            "{}-current-{}-r1",
            options.product, identity.application_version
        ),
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        product: options.product,
        application_version: identity.application_version.clone(),
        schema_identity: identity,
        created_at_epoch_seconds: now_seconds()?,
        source_tree_identity_sha256: path_identity_sha256(&options.tree)?,
        database: current_file(DATABASE_FILE, &database_output)?,
        configuration,
        tree,
        external_requirements: external_requirements(options)?,
    };
    validate_manifest(&manifest)?;
    write_json_create_new(&pending_path.join(MANIFEST_FILE), &manifest)?;
    File::open(&pending_path)?.sync_all()?;
    pending.commit()?;
    verify_current_backup(options)
}

pub fn verify_current_backup(
    options: &CompositeCurrentOptions,
) -> anyhow::Result<CurrentStateResult> {
    validate_options(options)?;
    verify_current_backup_root(&options.output)?;
    let manifest = read_manifest(&options.output)?;
    validate_manifest(&manifest)?;
    ensure!(
        manifest.product == options.product,
        "backup product mismatch"
    );
    ensure!(
        manifest.schema_identity
            == verify_database(options.product, &options.output.join(DATABASE_FILE))?,
        "backup database identity mismatch"
    );
    ensure!(
        manifest.database == current_file(DATABASE_FILE, &options.output.join(DATABASE_FILE))?,
        "backup database digest mismatch"
    );
    verify_tree_archive(&manifest.tree, &options.output.join(TREE_DIRECTORY))?;
    for file in &manifest.configuration {
        ensure!(
            *file == current_file(&file.path, &options.output.join(&file.path))?,
            "backup configuration digest mismatch"
        );
    }
    ensure!(
        options.configuration.len() == manifest.configuration.len(),
        "current adapter configuration resource count differs"
    );
    for configured in &options.configuration {
        let expected = manifest
            .configuration
            .iter()
            .find(|entry| entry.path == configured.name)
            .context("current adapter configuration name differs")?;
        ensure!(
            current_file(&configured.name, &configured.path)? == *expected,
            "current adapter configuration content differs for {}",
            configured.name
        );
    }
    ensure!(
        manifest.external_requirements == external_requirements(options)?,
        "backup external requirements do not match the supplied key"
    );
    verify_external_key(
        options.product,
        &options.output.join(DATABASE_FILE),
        options.credentials(),
    )?;
    verify_product_state(
        options.product,
        &options.output.join(DATABASE_FILE),
        &options.output.join(TREE_DIRECTORY),
    )?;
    Ok(CurrentStateResult {
        product: options.product,
        application_version: manifest.application_version,
        directory: absolute(&options.output)?,
    })
}

pub fn restore_current(options: &CurrentRestoreOptions) -> anyhow::Result<CurrentStateResult> {
    let source_configuration = options
        .configuration
        .iter()
        .map(|file| NamedFile {
            name: file.name.clone(),
            path: options.input.join(&file.name),
        })
        .collect();
    let verify = CompositeCurrentOptions {
        product: options.product,
        database: options.input.join(DATABASE_FILE),
        tree: options.input.join(TREE_DIRECTORY),
        output: options.input.clone(),
        runtime_directory: options.runtime_directory.clone(),
        configuration: source_configuration,
        credentials_key_id: options.credentials_key_id.clone(),
        credentials_key: options.credentials_key,
    };
    verify_current_backup(&verify)?;
    let source_backup = canonical_existing_directory(&options.input, "backup source")?;
    let database = canonical_target_path(&options.database)?;
    let tree = canonical_target_path(&options.tree)?;
    ensure_source_and_targets_are_disjoint(&source_backup, &database, &tree)?;
    let configuration_targets = options
        .configuration
        .iter()
        .map(|file| {
            Ok(NamedFile {
                name: file.name.clone(),
                path: canonical_target_path(&file.path)?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    ensure_configuration_targets_are_disjoint(
        &source_backup,
        &database,
        &tree,
        &configuration_targets,
    )?;
    let _locks = ProductLocks::acquire(
        options.product,
        &database,
        &tree,
        options.runtime_directory.as_deref(),
    )?;
    let database_exists = path_exists(&database)?;
    let tree_exists = path_exists(&tree)?;
    let configuration_exists = configuration_targets
        .iter()
        .map(|file| path_exists(&file.path))
        .collect::<anyhow::Result<Vec<_>>>()?;
    ensure!(
        options.replace_existing
            || (!database_exists
                && !tree_exists
                && configuration_exists.iter().all(|exists| !exists)),
        "restore targets already exist; pass --replace-existing"
    );
    ensure!(
        database_exists == tree_exists
            && configuration_exists
                .iter()
                .all(|exists| *exists == database_exists),
        "restore target contains a mixed composite-state generation"
    );
    let (original_database, original_tree) = if database_exists {
        verify_database(options.product, &database)
            .context("existing restore database is not the exact current generation")?;
        verify_product_state(options.product, &database, &tree)
            .context("existing restore tree is not the exact current generation")?;
        (
            Some(current_file(DATABASE_FILE, &database)?),
            Some(inventory_tree(&tree)?),
        )
    } else {
        (None, None)
    };

    let nonce = Uuid::new_v4().simple().to_string();
    let database_stage = sibling(&database, &format!("incoming-{nonce}"))?;
    let tree_stage = sibling(&tree, &format!("incoming-{nonce}"))?;
    let database_original = sibling(&database, &format!("original-{nonce}"))?;
    let tree_original = sibling(&tree, &format!("original-{nonce}"))?;
    let recovery = sibling(&database, &format!("recovery-{nonce}"))?;
    copy_regular(&source_backup.join(DATABASE_FILE), &database_stage)?;
    create_private_directory(&tree_stage)?;
    copy_strict_tree(&source_backup.join(TREE_DIRECTORY), &tree_stage, 0)?;
    let manifest = read_manifest(&source_backup)?;
    verify_incoming_generation(
        &manifest.database,
        &manifest.tree,
        &database_stage,
        &tree_stage,
    )?;
    verify_database(options.product, &database_stage)?;
    verify_product_state(options.product, &database_stage, &tree_stage)?;
    let mut restore_configuration = Vec::new();
    for (target, exists) in configuration_targets.iter().zip(configuration_exists) {
        let incoming = manifest
            .configuration
            .iter()
            .find(|file| file.path == target.name)
            .context("restore configuration name differs from the manifest")?
            .clone();
        let stage = sibling(&target.path, &format!("incoming-{nonce}"))?;
        let original = sibling(&target.path, &format!("original-{nonce}"))?;
        copy_regular(&source_backup.join(&target.name), &stage)?;
        ensure!(
            current_file(&target.name, &stage)? == incoming,
            "staged restore configuration differs from the manifest"
        );
        restore_configuration.push(RestoreConfiguration {
            name: target.name.clone(),
            target: target.path.clone(),
            target_path_identity_sha256: target_path_identity_sha256(&target.path)?,
            stage,
            original,
            incoming,
            original_file: exists
                .then(|| current_file(&target.name, &target.path))
                .transpose()?,
        });
    }
    restore_configuration.sort_by(|left, right| left.name.cmp(&right.name));
    create_private_directory(&recovery)?;
    sync_parent(&recovery)?;
    let recovery = canonical_existing_directory(&recovery, "restore recovery directory")?;
    let (source_manifest_bytes, source_manifest_sha256) =
        hash_file(&source_backup.join(MANIFEST_FILE))?;
    let mut journal = RestoreJournal {
        journal_version: CURRENT_RESTORE_JOURNAL_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        product: options.product,
        application_version: manifest.application_version.clone(),
        adapter_id: manifest.adapter_id.clone(),
        schema_identity: manifest.schema_identity.clone(),
        created_at_epoch_seconds: now_seconds()?,
        source_backup: source_backup.clone(),
        source_backup_identity_sha256: existing_path_identity_sha256(
            b"sarmg-current-source-backup-v1\0",
            &source_backup,
        )?,
        source_manifest_version: manifest.manifest_version,
        source_manifest_created_at_epoch_seconds: manifest.created_at_epoch_seconds,
        source_manifest_bytes,
        source_manifest_sha256,
        source_tree_identity_sha256: manifest.source_tree_identity_sha256.clone(),
        database: database.clone(),
        tree: tree.clone(),
        database_path_identity_sha256: target_path_identity_sha256(&database)?,
        tree_path_identity_sha256: target_path_identity_sha256(&tree)?,
        database_stage,
        tree_stage,
        database_original,
        tree_original,
        incoming_database: manifest.database.clone(),
        incoming_tree: manifest.tree.clone(),
        original_database,
        original_tree,
        configuration: manifest.configuration.clone(),
        configuration_targets: restore_configuration,
        external_requirements: manifest.external_requirements.clone(),
        phase: RestorePhase::Prepared,
    };
    validate_restore_journal(&journal, &recovery)?;
    write_journal(&recovery, &journal)?;
    resume_current_commit(&recovery, &mut journal)?;
    verify_external_key(
        options.product,
        &journal.database,
        options
            .credentials_key_id
            .as_deref()
            .zip(options.credentials_key.as_ref()),
    )?;
    cleanup_recovery_directory(&recovery, &journal)?;
    Ok(CurrentStateResult {
        product: options.product,
        application_version: manifest.application_version,
        directory: database,
    })
}

pub fn recover_current(options: &CurrentRecoveryOptions) -> anyhow::Result<CurrentStateResult> {
    let (version, _, _) = product_contract(options.product)?;
    ensure!(
        options.expected_application_version == version,
        "--expect-version is not the official current version for {}",
        options.product
    );
    let source_backup = canonical_existing_directory(&options.input, "backup source")?;
    let database = canonical_target_path(&options.database)?;
    let tree = canonical_target_path(&options.tree)?;
    ensure_source_and_targets_are_disjoint(&source_backup, &database, &tree)?;
    let recovery =
        canonical_existing_directory(&options.recovery_directory, "restore recovery directory")?;
    let mut journal = read_restore_journal(&recovery)?;
    validate_restore_journal(&journal, &recovery)?;
    ensure!(
        journal.product == options.product,
        "recovery journal product mismatch"
    );
    ensure!(
        journal.application_version == options.expected_application_version,
        "recovery journal does not match --expect-version"
    );
    ensure!(
        journal.source_backup == source_backup,
        "recovery source path mismatch"
    );
    ensure!(
        journal.database == database,
        "recovery database target mismatch"
    );
    ensure!(journal.tree == tree, "recovery data-tree target mismatch");
    ensure!(
        journal.external_requirements
            == external_requirements_for(
                options
                    .credentials_key_id
                    .as_deref()
                    .zip(options.credentials_key.as_ref()),
            ),
        "recovery external credentials key does not match the journal"
    );
    let _locks = ProductLocks::acquire(
        options.product,
        &database,
        &tree,
        options.runtime_directory.as_deref(),
    )?;
    discard_uncommitted_journal_update(&recovery)?;
    verify_recovery_source(&journal)?;
    verify_recovery_evidence(&journal, options.action)?;
    match options.action {
        CurrentRecoveryAction::Commit => {
            ensure!(
                !matches!(
                    journal.phase,
                    RestorePhase::RollbackStarted | RestorePhase::RollbackVerified
                ),
                "a rollback journal cannot be committed"
            );
            resume_current_commit(&recovery, &mut journal)?;
            verify_external_key(
                journal.product,
                &journal.database,
                options
                    .credentials_key_id
                    .as_deref()
                    .zip(options.credentials_key.as_ref()),
            )?;
            cleanup_recovery_directory(&recovery, &journal)?;
        }
        CurrentRecoveryAction::Rollback => {
            resume_current_rollback(&recovery, &mut journal)?;
            cleanup_recovery_directory(&recovery, &journal)?;
        }
    }
    Ok(CurrentStateResult {
        product: journal.product,
        application_version: journal.application_version,
        directory: database,
    })
}

fn read_restore_journal(recovery: &Path) -> anyhow::Result<RestoreJournal> {
    validate_recovery_directory_entries(recovery)?;
    let path = recovery.join("restore-journal.json");
    let metadata = fs::symlink_metadata(&path)?;
    ensure!(
        metadata.is_file() && metadata.nlink() == 1 && metadata.len() <= MAX_CURRENT_JOURNAL_BYTES,
        "current restore journal is not a bounded single-link regular file"
    );
    let bytes = fs::read(&path)?;
    ensure!(
        bytes.len() as u64 <= MAX_CURRENT_JOURNAL_BYTES,
        "current restore journal is too large"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

fn validate_recovery_directory_entries(recovery: &Path) -> anyhow::Result<()> {
    let mut journal_seen = false;
    for entry in fs::read_dir(recovery)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("current recovery entry name is not UTF-8"))?;
        ensure!(
            name == "restore-journal.json" || name == "restore-journal.pending",
            "current recovery directory contains an unexpected entry: {name}"
        );
        let metadata = fs::symlink_metadata(entry.path())?;
        ensure!(
            metadata.is_file()
                && metadata.nlink() == 1
                && metadata.len() <= MAX_CURRENT_JOURNAL_BYTES,
            "current recovery entry is not a bounded single-link regular file"
        );
        journal_seen |= name == "restore-journal.json";
    }
    ensure!(
        journal_seen,
        "current recovery directory has no restore journal"
    );
    Ok(())
}

fn discard_uncommitted_journal_update(recovery: &Path) -> anyhow::Result<()> {
    let pending = recovery.join("restore-journal.pending");
    if path_exists(&pending)? {
        let metadata = fs::symlink_metadata(&pending)?;
        ensure!(
            metadata.is_file()
                && metadata.nlink() == 1
                && metadata.len() <= MAX_CURRENT_JOURNAL_BYTES,
            "pending current restore journal is invalid"
        );
        fs::remove_file(pending)?;
        File::open(recovery)?.sync_all()?;
    }
    Ok(())
}

fn validate_restore_journal(journal: &RestoreJournal, recovery: &Path) -> anyhow::Result<()> {
    ensure!(
        journal.journal_version == CURRENT_RESTORE_JOURNAL_VERSION,
        "unsupported current restore journal version"
    );
    ensure!(
        journal.tool_version == env!("CARGO_PKG_VERSION"),
        "current restore journal tool version is not exact"
    );
    let (version, revision, schema_sha256) = product_contract(journal.product)?;
    ensure!(
        journal.application_version == version,
        "current restore journal application version is not current"
    );
    ensure!(
        journal.adapter_id == format!("{}-current-{version}-r1", journal.product),
        "current restore journal adapter id is not exact"
    );
    ensure!(
        journal.schema_identity
            == SchemaIdentity {
                application: journal.product.slug().to_owned(),
                application_version: version.to_owned(),
                schema_revision: revision,
                schema_sha256: schema_sha256.to_owned(),
            },
        "current restore journal schema identity is not exact"
    );
    ensure!(
        journal.created_at_epoch_seconds > 0
            && journal.source_manifest_created_at_epoch_seconds > 0,
        "current restore journal timestamp is invalid"
    );
    ensure!(
        journal.source_manifest_version == CURRENT_MANIFEST_VERSION,
        "current restore journal manifest version is not current"
    );
    ensure!(
        journal.source_manifest_bytes > 0 && journal.source_manifest_bytes <= MAX_MANIFEST_BYTES,
        "current restore journal manifest size is invalid"
    );
    for digest in [
        &journal.source_backup_identity_sha256,
        &journal.source_manifest_sha256,
        &journal.source_tree_identity_sha256,
        &journal.database_path_identity_sha256,
        &journal.tree_path_identity_sha256,
    ] {
        validate_sha256(digest)?;
    }
    validate_product_resources(
        journal.product,
        &journal.configuration,
        &journal.external_requirements,
    )?;
    ensure!(
        journal.configuration_targets.len() == journal.configuration.len(),
        "current restore journal configuration target count is not exact"
    );
    for (resource, target) in journal
        .configuration
        .iter()
        .zip(&journal.configuration_targets)
    {
        ensure!(
            target.name == resource.path && target.incoming == *resource,
            "current restore journal configuration mapping is not exact"
        );
        validate_name(&target.name)?;
        validate_sha256(&target.target_path_identity_sha256)?;
        if let Some(original) = &target.original_file {
            ensure!(
                original.path == target.name,
                "current restore journal original configuration name is not exact"
            );
            validate_sha256(&original.sha256)?;
        }
    }
    validate_current_file(&journal.incoming_database, DATABASE_FILE)?;
    validate_tree_archive_contract(&journal.incoming_tree)?;
    match (&journal.original_database, &journal.original_tree) {
        (Some(database), Some(tree)) => {
            validate_current_file(database, DATABASE_FILE)?;
            validate_tree_archive_contract(tree)?;
        }
        (None, None) => {}
        _ => anyhow::bail!("current restore journal original generation is incomplete"),
    }
    ensure!(
        journal.phase != RestorePhase::OriginalsPreserved || journal.original_database.is_some(),
        "originals-preserved phase requires an original generation"
    );

    let source_backup = canonical_existing_directory(&journal.source_backup, "backup source")?;
    ensure!(
        source_backup == journal.source_backup,
        "current restore journal source path is not canonical"
    );
    ensure!(
        existing_path_identity_sha256(b"sarmg-current-source-backup-v1\0", &source_backup,)?
            == journal.source_backup_identity_sha256,
        "current restore journal source path identity mismatch"
    );
    let database = canonical_target_path(&journal.database)?;
    let tree = canonical_target_path(&journal.tree)?;
    ensure!(
        database == journal.database && tree == journal.tree,
        "current restore journal target path is not canonical"
    );
    ensure_source_and_targets_are_disjoint(&source_backup, &database, &tree)?;
    ensure!(
        target_path_identity_sha256(&database)? == journal.database_path_identity_sha256,
        "current restore journal database path identity mismatch"
    );
    ensure!(
        target_path_identity_sha256(&tree)? == journal.tree_path_identity_sha256,
        "current restore journal data-tree path identity mismatch"
    );
    ensure!(
        recovery == canonical_existing_directory(recovery, "restore recovery directory")?,
        "current restore recovery path is not canonical"
    );
    let database_name = database
        .file_name()
        .and_then(|name| name.to_str())
        .context("current restore database name must be UTF-8")?;
    let recovery_name = recovery
        .file_name()
        .and_then(|name| name.to_str())
        .context("current restore recovery directory name must be UTF-8")?;
    let prefix = format!(".{database_name}.recovery-");
    let nonce = recovery_name
        .strip_prefix(&prefix)
        .context("current restore recovery directory does not match its database")?;
    let identifier = Uuid::parse_str(nonce)
        .context("current restore recovery directory identifier is invalid")?;
    ensure!(
        identifier.simple().to_string() == nonce,
        "current restore recovery identifier is not canonical"
    );
    ensure!(
        sibling(&database, &format!("recovery-{nonce}"))? == recovery,
        "current restore recovery directory is not the exact database sibling"
    );
    ensure!(
        journal.database_stage == sibling(&database, &format!("incoming-{nonce}"))?
            && journal.tree_stage == sibling(&tree, &format!("incoming-{nonce}"))?
            && journal.database_original == sibling(&database, &format!("original-{nonce}"))?
            && journal.tree_original == sibling(&tree, &format!("original-{nonce}"))?,
        "current restore journal stage/original paths are not exact target siblings"
    );
    let mut configuration_paths = BTreeSet::new();
    for target in &journal.configuration_targets {
        let canonical = canonical_target_path(&target.target)?;
        ensure!(
            canonical == target.target
                && target_path_identity_sha256(&target.target)?
                    == target.target_path_identity_sha256,
            "current restore journal configuration target identity mismatch"
        );
        ensure!(
            target.stage == sibling(&target.target, &format!("incoming-{nonce}"))?
                && target.original == sibling(&target.target, &format!("original-{nonce}"))?,
            "current restore journal configuration stage/original paths are not exact"
        );
        ensure!(
            configuration_paths.insert(target.target.clone()),
            "current restore journal configuration targets are duplicated"
        );
    }
    let configuration = journal
        .configuration_targets
        .iter()
        .map(|target| NamedFile {
            name: target.name.clone(),
            path: target.target.clone(),
        })
        .collect::<Vec<_>>();
    ensure_configuration_targets_are_disjoint(&source_backup, &database, &tree, &configuration)?;
    Ok(())
}

fn verify_recovery_source(journal: &RestoreJournal) -> anyhow::Result<()> {
    let (manifest_bytes, manifest_sha256) = hash_file(&journal.source_backup.join(MANIFEST_FILE))?;
    ensure!(
        manifest_bytes == journal.source_manifest_bytes
            && manifest_sha256 == journal.source_manifest_sha256,
        "current restore source manifest content hash mismatch"
    );
    verify_current_backup_root(&journal.source_backup)?;
    let manifest = read_manifest(&journal.source_backup)?;
    validate_manifest(&manifest)?;
    ensure!(
        manifest.manifest_version == journal.source_manifest_version
            && manifest.created_at_epoch_seconds
                == journal.source_manifest_created_at_epoch_seconds
            && manifest.product == journal.product
            && manifest.application_version == journal.application_version
            && manifest.adapter_id == journal.adapter_id
            && manifest.schema_identity == journal.schema_identity
            && manifest.source_tree_identity_sha256 == journal.source_tree_identity_sha256
            && manifest.database == journal.incoming_database
            && manifest.tree == journal.incoming_tree
            && manifest.configuration == journal.configuration
            && manifest.external_requirements == journal.external_requirements,
        "current restore journal does not match its exact source manifest"
    );
    verify_incoming_generation(
        &journal.incoming_database,
        &journal.incoming_tree,
        &journal.source_backup.join(DATABASE_FILE),
        &journal.source_backup.join(TREE_DIRECTORY),
    )?;
    ensure!(
        verify_database(journal.product, &journal.source_backup.join(DATABASE_FILE))?
            == journal.schema_identity,
        "current restore source database identity mismatch"
    );
    verify_product_state(
        journal.product,
        &journal.source_backup.join(DATABASE_FILE),
        &journal.source_backup.join(TREE_DIRECTORY),
    )?;
    for resource in &journal.configuration {
        ensure!(
            current_file(&resource.path, &journal.source_backup.join(&resource.path))? == *resource,
            "current restore source configuration content mismatch"
        );
    }
    Ok(())
}

fn verify_recovery_evidence(
    journal: &RestoreJournal,
    action: CurrentRecoveryAction,
) -> anyhow::Result<()> {
    if action == CurrentRecoveryAction::Commit {
        ensure!(
            !matches!(
                journal.phase,
                RestorePhase::RollbackStarted | RestorePhase::RollbackVerified
            ),
            "current restore journal has already entered rollback"
        );
    }
    let database_stage = observed_current_file(&journal.database_stage, DATABASE_FILE)?;
    let tree_stage = observed_tree_archive(&journal.tree_stage)?;
    let database_target = observed_current_file(&journal.database, DATABASE_FILE)?;
    let tree_target = observed_tree_archive(&journal.tree)?;
    let database_original = observed_current_file(&journal.database_original, DATABASE_FILE)?;
    let tree_original = observed_tree_archive(&journal.tree_original)?;

    ensure_optional_exact(
        database_stage.as_ref(),
        &journal.incoming_database,
        "staged current database",
    )?;
    ensure_optional_exact(
        tree_stage.as_ref(),
        &journal.incoming_tree,
        "staged current data tree",
    )?;
    if let Some(observed) = database_target.as_ref() {
        ensure!(
            observed == &journal.incoming_database
                || journal.original_database.as_ref() == Some(observed),
            "current database target content does not match the journal"
        );
    }
    if let Some(observed) = tree_target.as_ref() {
        ensure!(
            observed == &journal.incoming_tree || journal.original_tree.as_ref() == Some(observed),
            "current data-tree target content does not match the journal"
        );
    }
    match (&journal.original_database, &database_original) {
        (Some(expected), Some(observed)) => ensure!(
            observed == expected,
            "preserved current database content does not match the journal"
        ),
        (None, Some(_)) => anyhow::bail!("unexpected preserved current database"),
        _ => {}
    }
    match (&journal.original_tree, &tree_original) {
        (Some(expected), Some(observed)) => ensure!(
            observed == expected,
            "preserved current data-tree content does not match the journal"
        ),
        (None, Some(_)) => anyhow::bail!("unexpected preserved current data tree"),
        _ => {}
    }
    let mut configuration_targets_are_incoming = true;
    let mut configuration_stages_are_absent = true;
    let mut configuration_originals_are_available = true;
    let mut configuration_originals_are_preserved = true;
    for configuration in &journal.configuration_targets {
        let stage = observed_current_file(&configuration.stage, &configuration.name)?;
        let target = observed_current_file(&configuration.target, &configuration.name)?;
        let original = observed_current_file(&configuration.original, &configuration.name)?;
        ensure_optional_exact(
            stage.as_ref(),
            &configuration.incoming,
            "staged current configuration",
        )?;
        if let Some(observed) = target.as_ref() {
            ensure!(
                observed == &configuration.incoming
                    || configuration.original_file.as_ref() == Some(observed),
                "current configuration target content does not match the journal"
            );
        }
        match (&configuration.original_file, &original) {
            (Some(expected), Some(observed)) => ensure!(
                observed == expected,
                "preserved current configuration content does not match the journal"
            ),
            (None, Some(_)) => anyhow::bail!("unexpected preserved current configuration"),
            _ => {}
        }
        configuration_targets_are_incoming &= target.as_ref() == Some(&configuration.incoming);
        configuration_stages_are_absent &= stage.is_none();
        configuration_originals_are_available &=
            configuration.original_file.as_ref().is_none_or(|expected| {
                target.as_ref() == Some(expected) || original.as_ref() == Some(expected)
            });
        configuration_originals_are_preserved &= configuration
            .original_file
            .as_ref()
            .is_none_or(|expected| original.as_ref() == Some(expected));
    }

    let targets_are_incoming = database_target.as_ref() == Some(&journal.incoming_database)
        && tree_target.as_ref() == Some(&journal.incoming_tree)
        && configuration_targets_are_incoming;
    let stages_are_absent =
        database_stage.is_none() && tree_stage.is_none() && configuration_stages_are_absent;
    let has_original = journal.original_database.is_some();
    let original_database_available = journal.original_database.as_ref().is_some_and(|expected| {
        database_target.as_ref() == Some(expected) || database_original.as_ref() == Some(expected)
    });
    let original_tree_available = journal.original_tree.as_ref().is_some_and(|expected| {
        tree_target.as_ref() == Some(expected) || tree_original.as_ref() == Some(expected)
    });
    if action == CurrentRecoveryAction::Rollback && has_original {
        ensure!(
            original_database_available
                && original_tree_available
                && configuration_originals_are_available,
            "current restore journal has no exact original generation to roll back"
        );
    }
    match journal.phase {
        RestorePhase::Prepared => {
            if has_original {
                ensure!(
                    database_stage.as_ref() == Some(&journal.incoming_database)
                        && tree_stage.as_ref() == Some(&journal.incoming_tree)
                        && journal.configuration_targets.iter().all(|configuration| {
                            observed_current_file(&configuration.stage, &configuration.name)
                                .is_ok_and(|observed| {
                                    observed.as_ref() == Some(&configuration.incoming)
                                })
                        }),
                    "prepared replacement journal lost staged incoming content"
                );
                ensure!(
                    original_database_available
                        && original_tree_available
                        && configuration_originals_are_available,
                    "prepared replacement journal lost original content"
                );
            } else {
                ensure!(
                    database_stage.as_ref() == Some(&journal.incoming_database)
                        || database_target.as_ref() == Some(&journal.incoming_database),
                    "prepared journal has no exact incoming database"
                );
                ensure!(
                    tree_stage.as_ref() == Some(&journal.incoming_tree)
                        || tree_target.as_ref() == Some(&journal.incoming_tree),
                    "prepared journal has no exact incoming data tree"
                );
                ensure!(
                    journal.configuration_targets.iter().all(|configuration| {
                        observed_current_file(&configuration.stage, &configuration.name)
                            .is_ok_and(|stage| stage.as_ref() == Some(&configuration.incoming))
                            || observed_current_file(&configuration.target, &configuration.name)
                                .is_ok_and(|target| {
                                    target.as_ref() == Some(&configuration.incoming)
                                })
                    }),
                    "prepared journal has no exact incoming configuration"
                );
            }
        }
        RestorePhase::OriginalsPreserved => {
            ensure!(
                has_original,
                "originals-preserved journal has no original generation"
            );
            ensure!(
                database_original.is_some()
                    && tree_original.is_some()
                    && configuration_originals_are_preserved,
                "originals-preserved journal lost original content"
            );
            ensure!(
                database_stage.as_ref() == Some(&journal.incoming_database)
                    || database_target.as_ref() == Some(&journal.incoming_database),
                "originals-preserved journal has no exact incoming database"
            );
            ensure!(
                tree_stage.as_ref() == Some(&journal.incoming_tree)
                    || tree_target.as_ref() == Some(&journal.incoming_tree),
                "originals-preserved journal has no exact incoming data tree"
            );
            ensure!(
                journal.configuration_targets.iter().all(|configuration| {
                    observed_current_file(&configuration.stage, &configuration.name)
                        .is_ok_and(|stage| stage.as_ref() == Some(&configuration.incoming))
                        || observed_current_file(&configuration.target, &configuration.name)
                            .is_ok_and(|target| target.as_ref() == Some(&configuration.incoming))
                }),
                "originals-preserved journal has no exact incoming configuration"
            );
        }
        RestorePhase::Installed => ensure!(
            targets_are_incoming
                && stages_are_absent
                && (!has_original
                    || (database_original.is_some()
                        && tree_original.is_some()
                        && configuration_originals_are_preserved)),
            "installed journal does not contain the exact installed generation"
        ),
        RestorePhase::Verified => ensure!(
            targets_are_incoming && stages_are_absent,
            "verified journal does not contain the exact installed generation"
        ),
        RestorePhase::RollbackStarted => {
            if has_original {
                ensure!(
                    database_target.as_ref() == journal.original_database.as_ref()
                        || database_original.as_ref() == journal.original_database.as_ref(),
                    "rollback journal has no exact original database"
                );
                ensure!(
                    tree_target.as_ref() == journal.original_tree.as_ref()
                        || tree_original.as_ref() == journal.original_tree.as_ref(),
                    "rollback journal has no exact original data tree"
                );
                ensure!(
                    configuration_originals_are_available,
                    "rollback journal has no exact original configuration"
                );
            }
        }
        RestorePhase::RollbackVerified => {
            if has_original {
                ensure!(
                    database_target.as_ref() == journal.original_database.as_ref()
                        && tree_target.as_ref() == journal.original_tree.as_ref()
                        && journal.configuration_targets.iter().all(|configuration| {
                            observed_current_file(&configuration.target, &configuration.name)
                                .is_ok_and(|target| {
                                    target.as_ref() == configuration.original_file.as_ref()
                                })
                        }),
                    "rollback-verified journal does not contain the exact original generation"
                );
            } else {
                ensure!(
                    database_target.is_none()
                        && tree_target.is_none()
                        && journal.configuration_targets.iter().all(|configuration| {
                            observed_current_file(&configuration.target, &configuration.name)
                                .is_ok_and(|target| target.is_none())
                        }),
                    "rollback-verified journal unexpectedly retains installed targets"
                );
            }
        }
    }
    Ok(())
}

fn resume_current_commit(recovery: &Path, journal: &mut RestoreJournal) -> anyhow::Result<()> {
    if journal.phase == RestorePhase::Verified {
        return verify_installed_generation(journal);
    }
    if journal.phase != RestorePhase::Installed {
        preserve_original_file_for_commit(
            DATABASE_FILE,
            &journal.database,
            &journal.database_stage,
            &journal.database_original,
            journal.original_database.as_ref(),
            &journal.incoming_database,
        )?;
        preserve_original_tree_for_commit(
            &journal.tree,
            &journal.tree_stage,
            &journal.tree_original,
            journal.original_tree.as_ref(),
            &journal.incoming_tree,
        )?;
        for configuration in &journal.configuration_targets {
            preserve_original_file_for_commit(
                &configuration.name,
                &configuration.target,
                &configuration.stage,
                &configuration.original,
                configuration.original_file.as_ref(),
                &configuration.incoming,
            )?;
        }
        if journal.original_database.is_some() {
            journal.phase = RestorePhase::OriginalsPreserved;
            replace_journal(recovery, journal)?;
        }
        install_incoming_tree(journal)?;
        install_incoming_file(journal)?;
        for configuration in &journal.configuration_targets {
            install_incoming_configuration(configuration)?;
        }
        sync_parent(&journal.database)?;
        sync_parent(&journal.tree)?;
        for configuration in &journal.configuration_targets {
            sync_parent(&configuration.target)?;
        }
        journal.phase = RestorePhase::Installed;
        replace_journal(recovery, journal)?;
    }
    verify_installed_generation(journal)?;
    journal.phase = RestorePhase::Verified;
    replace_journal(recovery, journal)
}

fn resume_current_rollback(recovery: &Path, journal: &mut RestoreJournal) -> anyhow::Result<()> {
    if journal.phase != RestorePhase::RollbackVerified {
        if journal.phase != RestorePhase::RollbackStarted {
            journal.phase = RestorePhase::RollbackStarted;
            replace_journal(recovery, journal)?;
        }
        restore_original_file(journal)?;
        restore_original_tree(journal)?;
        for configuration in &journal.configuration_targets {
            restore_original_configuration(configuration)?;
        }
        match (&journal.original_database, &journal.original_tree) {
            (Some(database), Some(tree)) => {
                ensure!(
                    observed_current_file(&journal.database, DATABASE_FILE)?.as_ref()
                        == Some(database),
                    "rolled-back database does not match its journal"
                );
                ensure!(
                    observed_tree_archive(&journal.tree)?.as_ref() == Some(tree),
                    "rolled-back data tree does not match its journal"
                );
                verify_database(journal.product, &journal.database)?;
                verify_product_state(journal.product, &journal.database, &journal.tree)?;
                verify_installed_configuration(journal, true)?;
            }
            (None, None) => ensure!(
                !path_exists(&journal.database)?
                    && !path_exists(&journal.tree)?
                    && journal
                        .configuration_targets
                        .iter()
                        .all(|configuration| !path_exists(&configuration.target).unwrap_or(false)),
                "rollback of an initially empty target left installed content"
            ),
            _ => unreachable!(),
        }
        journal.phase = RestorePhase::RollbackVerified;
        replace_journal(recovery, journal)?;
    }
    Ok(())
}

fn preserve_original_file_for_commit(
    name: &str,
    target: &Path,
    stage: &Path,
    original: &Path,
    expected_original: Option<&CurrentFile>,
    incoming: &CurrentFile,
) -> anyhow::Result<()> {
    let Some(expected_original) = expected_original else {
        return Ok(());
    };
    if path_exists(original)? {
        ensure!(
            observed_current_file(original, name)?.as_ref() == Some(expected_original),
            "preserved current file content mismatch"
        );
        return Ok(());
    }
    if let Some(observed) = observed_current_file(target, name)? {
        if &observed == incoming && !path_exists(stage)? {
            return Ok(());
        }
        ensure!(
            &observed == expected_original,
            "current file target is neither the incoming nor original generation"
        );
        fs::rename(target, original)?;
        sync_parent(target)?;
    }
    Ok(())
}

fn preserve_original_tree_for_commit(
    target: &Path,
    stage: &Path,
    original: &Path,
    expected_original: Option<&TreeArchive>,
    incoming: &TreeArchive,
) -> anyhow::Result<()> {
    let Some(expected_original) = expected_original else {
        return Ok(());
    };
    if path_exists(original)? {
        ensure!(
            observed_tree_archive(original)?.as_ref() == Some(expected_original),
            "preserved current data-tree content mismatch"
        );
        return Ok(());
    }
    if let Some(observed) = observed_tree_archive(target)? {
        if &observed == incoming && !path_exists(stage)? {
            return Ok(());
        }
        ensure!(
            &observed == expected_original,
            "current data-tree target is neither the incoming nor original generation"
        );
        fs::rename(target, original)?;
        sync_parent(target)?;
    }
    Ok(())
}

fn install_incoming_file(journal: &RestoreJournal) -> anyhow::Result<()> {
    if let Some(target) = observed_current_file(&journal.database, DATABASE_FILE)? {
        ensure!(
            target == journal.incoming_database,
            "current database target blocks incoming installation"
        );
        ensure!(
            !path_exists(&journal.database_stage)?,
            "incoming database exists at both target and stage"
        );
        return Ok(());
    }
    ensure!(
        observed_current_file(&journal.database_stage, DATABASE_FILE)?.as_ref()
            == Some(&journal.incoming_database),
        "exact staged incoming database is unavailable"
    );
    fs::rename(&journal.database_stage, &journal.database)?;
    Ok(())
}

fn install_incoming_tree(journal: &RestoreJournal) -> anyhow::Result<()> {
    if let Some(target) = observed_tree_archive(&journal.tree)? {
        ensure!(
            target == journal.incoming_tree,
            "current data-tree target blocks incoming installation"
        );
        ensure!(
            !path_exists(&journal.tree_stage)?,
            "incoming data tree exists at both target and stage"
        );
        return Ok(());
    }
    ensure!(
        observed_tree_archive(&journal.tree_stage)?.as_ref() == Some(&journal.incoming_tree),
        "exact staged incoming data tree is unavailable"
    );
    fs::rename(&journal.tree_stage, &journal.tree)?;
    Ok(())
}

fn install_incoming_configuration(configuration: &RestoreConfiguration) -> anyhow::Result<()> {
    if let Some(target) = observed_current_file(&configuration.target, &configuration.name)? {
        ensure!(
            target == configuration.incoming,
            "current configuration target blocks incoming installation"
        );
        ensure!(
            !path_exists(&configuration.stage)?,
            "incoming configuration exists at both target and stage"
        );
        return Ok(());
    }
    ensure!(
        observed_current_file(&configuration.stage, &configuration.name)?.as_ref()
            == Some(&configuration.incoming),
        "exact staged incoming configuration is unavailable"
    );
    fs::rename(&configuration.stage, &configuration.target)?;
    Ok(())
}

fn restore_original_file(journal: &RestoreJournal) -> anyhow::Result<()> {
    match journal.original_database.as_ref() {
        Some(original) => {
            if !path_exists(&journal.database_original)? {
                ensure!(
                    observed_current_file(&journal.database, DATABASE_FILE)?.as_ref()
                        == Some(original),
                    "exact original database is unavailable for rollback"
                );
                return Ok(());
            }
            if path_exists(&journal.database)? {
                ensure!(
                    observed_current_file(&journal.database, DATABASE_FILE)?.as_ref()
                        == Some(&journal.incoming_database),
                    "rollback database target is not the exact incoming generation"
                );
                ensure!(
                    !path_exists(&journal.database_stage)?,
                    "rollback database stage is already occupied"
                );
                fs::rename(&journal.database, &journal.database_stage)?;
            }
            ensure!(
                observed_current_file(&journal.database_original, DATABASE_FILE)?.as_ref()
                    == Some(original),
                "exact preserved database is unavailable for rollback"
            );
            fs::rename(&journal.database_original, &journal.database)?;
            sync_parent(&journal.database)?;
        }
        None => {
            if path_exists(&journal.database)? {
                ensure!(
                    observed_current_file(&journal.database, DATABASE_FILE)?.as_ref()
                        == Some(&journal.incoming_database),
                    "rollback refuses an unknown database target"
                );
                fs::remove_file(&journal.database)?;
                sync_parent(&journal.database)?;
            }
        }
    }
    Ok(())
}

fn restore_original_tree(journal: &RestoreJournal) -> anyhow::Result<()> {
    match journal.original_tree.as_ref() {
        Some(original) => {
            if !path_exists(&journal.tree_original)? {
                ensure!(
                    observed_tree_archive(&journal.tree)?.as_ref() == Some(original),
                    "exact original data tree is unavailable for rollback"
                );
                return Ok(());
            }
            if path_exists(&journal.tree)? {
                ensure!(
                    observed_tree_archive(&journal.tree)?.as_ref() == Some(&journal.incoming_tree),
                    "rollback data-tree target is not the exact incoming generation"
                );
                ensure!(
                    !path_exists(&journal.tree_stage)?,
                    "rollback data-tree stage is already occupied"
                );
                fs::rename(&journal.tree, &journal.tree_stage)?;
            }
            ensure!(
                observed_tree_archive(&journal.tree_original)?.as_ref() == Some(original),
                "exact preserved data tree is unavailable for rollback"
            );
            fs::rename(&journal.tree_original, &journal.tree)?;
            sync_parent(&journal.tree)?;
        }
        None => {
            if path_exists(&journal.tree)? {
                ensure!(
                    observed_tree_archive(&journal.tree)?.as_ref() == Some(&journal.incoming_tree),
                    "rollback refuses an unknown data-tree target"
                );
                fs::remove_dir_all(&journal.tree)?;
                sync_parent(&journal.tree)?;
            }
        }
    }
    Ok(())
}

fn restore_original_configuration(configuration: &RestoreConfiguration) -> anyhow::Result<()> {
    match configuration.original_file.as_ref() {
        Some(original) => {
            if !path_exists(&configuration.original)? {
                ensure!(
                    observed_current_file(&configuration.target, &configuration.name)?.as_ref()
                        == Some(original),
                    "exact original configuration is unavailable for rollback"
                );
                return Ok(());
            }
            if path_exists(&configuration.target)? {
                ensure!(
                    observed_current_file(&configuration.target, &configuration.name)?.as_ref()
                        == Some(&configuration.incoming),
                    "rollback configuration target is not the exact incoming generation"
                );
                ensure!(
                    !path_exists(&configuration.stage)?,
                    "rollback configuration stage is already occupied"
                );
                fs::rename(&configuration.target, &configuration.stage)?;
            }
            ensure!(
                observed_current_file(&configuration.original, &configuration.name)?.as_ref()
                    == Some(original),
                "exact preserved configuration is unavailable for rollback"
            );
            fs::rename(&configuration.original, &configuration.target)?;
            sync_parent(&configuration.target)?;
        }
        None => {
            if path_exists(&configuration.target)? {
                ensure!(
                    observed_current_file(&configuration.target, &configuration.name)?.as_ref()
                        == Some(&configuration.incoming),
                    "rollback refuses an unknown configuration target"
                );
                fs::remove_file(&configuration.target)?;
                sync_parent(&configuration.target)?;
            }
        }
    }
    Ok(())
}

fn verify_installed_generation(journal: &RestoreJournal) -> anyhow::Result<()> {
    verify_incoming_generation(
        &journal.incoming_database,
        &journal.incoming_tree,
        &journal.database,
        &journal.tree,
    )?;
    ensure!(
        verify_database(journal.product, &journal.database)? == journal.schema_identity,
        "installed current database identity mismatch"
    );
    verify_product_state(journal.product, &journal.database, &journal.tree)?;
    verify_installed_configuration(journal, false)
}

fn verify_installed_configuration(journal: &RestoreJournal, original: bool) -> anyhow::Result<()> {
    for configuration in &journal.configuration_targets {
        let expected = if original {
            configuration.original_file.as_ref()
        } else {
            Some(&configuration.incoming)
        };
        ensure!(
            observed_current_file(&configuration.target, &configuration.name)?.as_ref() == expected,
            "installed current configuration does not match its journal"
        );
    }
    Ok(())
}

fn verify_incoming_generation(
    database_expected: &CurrentFile,
    tree_expected: &TreeArchive,
    database: &Path,
    tree: &Path,
) -> anyhow::Result<()> {
    ensure!(
        current_file(DATABASE_FILE, database)? == *database_expected,
        "current database content hash mismatch"
    );
    ensure!(
        inventory_tree(tree)? == *tree_expected,
        "current data-tree content hash mismatch"
    );
    Ok(())
}

fn observed_current_file(path: &Path, name: &str) -> anyhow::Result<Option<CurrentFile>> {
    path_exists(path)?
        .then(|| current_file(name, path))
        .transpose()
}

fn observed_tree_archive(path: &Path) -> anyhow::Result<Option<TreeArchive>> {
    path_exists(path)?.then(|| inventory_tree(path)).transpose()
}

fn ensure_optional_exact<T: PartialEq>(
    observed: Option<&T>,
    expected: &T,
    label: &str,
) -> anyhow::Result<()> {
    ensure!(
        observed.is_none_or(|observed| observed == expected),
        "{label} content does not match the journal"
    );
    Ok(())
}

fn validate_current_file(file: &CurrentFile, expected_path: &str) -> anyhow::Result<()> {
    ensure!(file.path == expected_path, "current file path is not exact");
    ensure!(file.bytes > 0, "current database file is empty");
    ensure!(file.mode & !0o7777 == 0, "current file mode is invalid");
    validate_sha256(&file.sha256)
}

fn validate_tree_archive_contract(tree: &TreeArchive) -> anyhow::Result<()> {
    ensure!(
        tree.directory == TREE_DIRECTORY,
        "current tree path is not exact"
    );
    ensure!(tree.mode & !0o7777 == 0, "current tree mode is invalid");
    ensure!(
        tree.files
            .iter()
            .try_fold(0_u64, |sum, file| sum.checked_add(file.bytes))
            == Some(tree.bytes),
        "current tree byte count is invalid"
    );
    validate_sha256(&tree.sha256)?;
    for directory in &tree.directories {
        validate_relative(&directory.path)?;
        ensure!(
            directory.mode & !0o7777 == 0,
            "current tree directory mode is invalid"
        );
    }
    for file in &tree.files {
        validate_relative(&file.path)?;
        ensure!(
            file.mode & !0o7777 == 0,
            "current tree file mode is invalid"
        );
        validate_sha256(&file.sha256)?;
    }
    ensure!(
        tree.directories
            .windows(2)
            .all(|pair| pair[0].path < pair[1].path)
            && tree
                .files
                .windows(2)
                .all(|pair| pair[0].path < pair[1].path),
        "current tree entries are duplicated or unsorted"
    );
    Ok(())
}

impl CompositeCurrentOptions {
    fn credentials(&self) -> Option<(&str, &[u8; 32])> {
        self.credentials_key_id
            .as_deref()
            .zip(self.credentials_key.as_ref())
    }
}

fn configuration_names(files: &[NamedFile]) -> Vec<&str> {
    let mut names = files
        .iter()
        .map(|file| file.name.as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

fn current_configuration_names(files: &[CurrentFile]) -> Vec<&str> {
    let mut names = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

fn validate_product_resources(
    product: Product,
    configuration: &[CurrentFile],
    external: &[ExternalRequirement],
) -> anyhow::Result<()> {
    match product {
        Product::MediaBackup => ensure!(
            configuration.is_empty() && external.is_empty(),
            "Media current resources are not exact"
        ),
        Product::DufsRam => ensure!(
            current_configuration_names(configuration) == ["dufs.yaml"] && external.is_empty(),
            "Dufs current resources are not exact"
        ),
        Product::SentinelMonitor => ensure!(
            current_configuration_names(configuration)
                == ["mediamtx.lock", "mediamtx.yml", "sentinel.env"]
                && external.len() == 1
                && external[0].kind == "credentials-key"
                && external[0].kid == "sentinel-credentials-0.2.0-key-1"
                && external[0].algorithm == "aes-256-gcm-hkdf-sha256"
                && external[0].envelope_version == 1,
            "Sentinel current resources are not exact"
        ),
        _ => anyhow::bail!("unsupported composite current product {product}"),
    }
    Ok(())
}

fn validate_options(options: &CompositeCurrentOptions) -> anyhow::Result<()> {
    product_contract(options.product)?;
    for path in [&options.database, &options.tree, &options.output] {
        ensure!(path.is_absolute(), "current adapter paths must be absolute");
    }
    let verifying_published_backup = options.database == options.output.join(DATABASE_FILE)
        && options.tree == options.output.join(TREE_DIRECTORY);
    ensure!(
        verifying_published_backup
            || (!options.output.starts_with(&options.tree)
                && !options.tree.starts_with(&options.output)),
        "backup output and data tree must be disjoint"
    );
    match options.product {
        Product::MediaBackup => ensure!(
            options.credentials().is_none() && options.configuration.is_empty(),
            "Media current adapter does not accept external key or configuration resources"
        ),
        Product::SentinelMonitor => ensure!(
            options.credentials().is_some()
                && configuration_names(&options.configuration)
                    == ["mediamtx.lock", "mediamtx.yml", "sentinel.env"],
            "Sentinel current adapter requires its external key and exact three configuration resources"
        ),
        Product::DufsRam => ensure!(
            options.credentials().is_none()
                && configuration_names(&options.configuration) == ["dufs.yaml"],
            "Dufs current adapter requires exactly dufs.yaml and no external key"
        ),
        _ => anyhow::bail!(
            "no strict composite current adapter for {}",
            options.product
        ),
    }
    Ok(())
}

fn product_contract(product: Product) -> anyhow::Result<(&'static str, u64, &'static str)> {
    match product {
        Product::MediaBackup => Ok((
            MEDIA_CURRENT_APPLICATION_VERSION,
            MEDIA_SCHEMA_REVISION,
            MEDIA_SCHEMA_SHA256,
        )),
        Product::SentinelMonitor => Ok((
            SENTINEL_CURRENT_APPLICATION_VERSION,
            SENTINEL_SCHEMA_REVISION,
            SENTINEL_SCHEMA_SHA256,
        )),
        Product::DufsRam => Ok((
            DUFS_CURRENT_APPLICATION_VERSION,
            DUFS_SCHEMA_REVISION,
            DUFS_SCHEMA_SHA256,
        )),
        _ => anyhow::bail!("unsupported composite current product {product}"),
    }
}

fn verify_database(product: Product, path: &Path) -> anyhow::Result<SchemaIdentity> {
    let (version, revision, expected_sha) = product_contract(product)?;
    let expected = SchemaIdentity::new(product.slug(), version, revision, expected_sha)
        .context("compiled Media schema identity is invalid")?;
    let actual = crate::sqlite::verify_schema_identity_database(path)?;
    actual.require_exact(&expected).with_context(|| {
        format!("database is not the exact official current {product} contract")
    })?;
    Ok(actual)
}

fn verify_external_key(
    product: Product,
    database: &Path,
    credentials: Option<(&str, &[u8; 32])>,
) -> anyhow::Result<()> {
    match (product, credentials) {
        (Product::MediaBackup, None) => Ok(()),
        (Product::DufsRam, None) => Ok(()),
        (Product::SentinelMonitor, Some((kid, key))) => {
            ensure!(
                kid == SENTINEL_KEY_ID,
                "Sentinel credentials key ID is not current"
            );
            verify_sentinel_credentials(database, key)
        }
        _ => anyhow::bail!("external credentials are not valid for {product}"),
    }
}

fn verify_sentinel_credentials(database: &Path, master_key: &[u8; 32]) -> anyhow::Result<()> {
    let mut derived_key = [0_u8; 32];
    Hkdf::<Sha256>::new(Some(SENTINEL_KEY_DERIVATION_SALT), master_key)
        .expand(SENTINEL_KEY_DERIVATION_INFO, &mut derived_key)
        .map_err(|_| anyhow::anyhow!("Sentinel credential key derivation failed"))?;
    let cipher = Aes256Gcm::new_from_slice(&derived_key)
        .map_err(|_| anyhow::anyhow!("Sentinel credential key is invalid"))?;
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare(
        "SELECT id, main_stream_url_enc, sub_stream_url_enc, username_enc, password_enc \
         FROM cameras ORDER BY id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, Option<Vec<u8>>>(2)?,
            row.get::<_, Option<Vec<u8>>>(3)?,
            row.get::<_, Option<Vec<u8>>>(4)?,
        ))
    })?;
    for row in rows {
        let (camera_id, main, sub, username, password) = row?;
        let camera_id =
            Uuid::parse_str(&camera_id).context("Sentinel camera ID is not a canonical UUID")?;
        for (field, envelope) in [
            ("main_stream_url_enc", Some(main)),
            ("sub_stream_url_enc", sub),
            ("username_enc", username),
            ("password_enc", password),
        ] {
            if let Some(envelope) = envelope {
                decrypt_sentinel_credential(&cipher, camera_id, field, &envelope)?;
            }
        }
    }
    Ok(())
}

fn decrypt_sentinel_credential(
    cipher: &Aes256Gcm,
    camera_id: Uuid,
    field: &str,
    encoded: &[u8],
) -> anyhow::Result<()> {
    ensure!(
        !encoded.is_empty() && encoded.len() <= SENTINEL_MAX_ENVELOPE_BYTES,
        "Sentinel credential envelope is not exactly current or authenticated"
    );
    let envelope: SentinelCredentialEnvelope = serde_json::from_slice(encoded)
        .context("Sentinel credential envelope is not exactly current or authenticated")?;
    ensure!(
        serde_json::to_vec(&envelope)? == encoded
            && envelope.product == Product::SentinelMonitor.slug()
            && envelope.application_version == SENTINEL_CURRENT_APPLICATION_VERSION
            && envelope.envelope_revision == 1
            && envelope.key_id == SENTINEL_KEY_ID,
        "Sentinel credential envelope is not exactly current or authenticated"
    );
    let nonce = decode_sentinel_base64(&envelope.nonce)?;
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| {
        anyhow::anyhow!("Sentinel credential envelope is not exactly current or authenticated")
    })?;
    let ciphertext = decode_sentinel_base64(&envelope.ciphertext)?;
    ensure!(
        (16..=SENTINEL_MAX_PLAINTEXT_BYTES + 16).contains(&ciphertext.len()),
        "Sentinel credential envelope is not exactly current or authenticated"
    );
    let aad = sentinel_credential_aad(camera_id, field);
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| {
            anyhow::anyhow!("Sentinel credential envelope is not exactly current or authenticated")
        })?;
    String::from_utf8(plaintext)
        .map(|_| ())
        .map_err(|_| anyhow::anyhow!("Sentinel credential plaintext is not UTF-8"))
}

fn decode_sentinel_base64(encoded: &str) -> anyhow::Result<Vec<u8>> {
    let decoded = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| {
        anyhow::anyhow!("Sentinel credential envelope is not exactly current or authenticated")
    })?;
    ensure!(
        URL_SAFE_NO_PAD.encode(&decoded) == encoded,
        "Sentinel credential envelope is not exactly current or authenticated"
    );
    Ok(decoded)
}

fn sentinel_credential_aad(camera_id: Uuid, field: &str) -> Vec<u8> {
    let camera_id = camera_id.hyphenated().to_string();
    let revision = "1";
    let mut aad = Vec::new();
    for value in [
        SENTINEL_AAD_DOMAIN,
        Product::SentinelMonitor.slug(),
        SENTINEL_CURRENT_APPLICATION_VERSION,
        revision,
        SENTINEL_KEY_ID,
        camera_id.as_str(),
        field,
    ] {
        aad.extend_from_slice(&(value.len() as u64).to_be_bytes());
        aad.extend_from_slice(value.as_bytes());
    }
    aad
}

fn external_requirements(
    options: &CompositeCurrentOptions,
) -> anyhow::Result<Vec<ExternalRequirement>> {
    Ok(external_requirements_for(options.credentials()))
}

fn external_requirements_for(credentials: Option<(&str, &[u8; 32])>) -> Vec<ExternalRequirement> {
    match credentials {
        Some((kid, key)) => vec![ExternalRequirement {
            kind: "credentials-key".to_owned(),
            kid: kid.to_owned(),
            sha256: lower_hex(&Sha256::digest(key)),
            algorithm: "aes-256-gcm-hkdf-sha256".to_owned(),
            envelope_version: 1,
        }],
        None => Vec::new(),
    }
}

fn verify_product_state(product: Product, database: &Path, tree: &Path) -> anyhow::Result<()> {
    match product {
        Product::MediaBackup => verify_media_state(database, tree),
        Product::SentinelMonitor | Product::DufsRam => {
            let connection =
                Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let foreign_keys: i64 = connection.query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_check",
                [],
                |row| row.get(0),
            )?;
            ensure!(
                foreign_keys == 0,
                "current product foreign key check failed"
            );
            inventory_tree(tree)?;
            Ok(())
        }
        _ => anyhow::bail!("unsupported current product {product}"),
    }
}

fn verify_configuration(product: Product, files: &[NamedFile], _tree: &Path) -> anyhow::Result<()> {
    match product {
        Product::MediaBackup => {
            ensure!(
                files.is_empty(),
                "Media Backup has no configuration resources"
            );
            Ok(())
        }
        Product::SentinelMonitor => {
            ensure!(
                configuration_names(files) == ["mediamtx.lock", "mediamtx.yml", "sentinel.env"],
                "Sentinel configuration resources are not exact"
            );
            Ok(())
        }
        Product::DufsRam => {
            ensure!(
                configuration_names(files) == ["dufs.yaml"],
                "Dufs configuration resource is not exact"
            );
            Ok(())
        }
        _ => anyhow::bail!("unsupported current product {product}"),
    }
}

fn verify_media_state(database: &Path, tree: &Path) -> anyhow::Result<()> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare(
        "SELECT a.storage_path,b.storage_path,b.stored_size,b.content_blake3 \
         FROM blobs b JOIN accounts a ON a.id=b.account_id ORDER BY b.id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (account, blob, bytes, expected_blake3) = row?;
        validate_relative(&account)?;
        validate_relative(&blob)?;
        ensure!(bytes >= 0, "Media Backup blob has a negative stored size");
        let path = tree.join(account).join(blob);
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file() && metadata.nlink() == 1,
            "Media Backup blob is not a single-link regular file"
        );
        ensure!(
            metadata.len() == bytes as u64,
            "Media Backup blob size differs from SQLite"
        );
        let mut hasher = blake3::Hasher::new();
        let mut file = File::open(&path)?;
        let mut buffer = [0_u8; 128 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        ensure!(
            hasher.finalize().to_hex().as_str() == expected_blake3,
            "Media Backup blob BLAKE3 differs from SQLite"
        );
    }
    Ok(())
}

fn copy_sqlite_snapshot(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let source = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut destination_connection = Connection::open(destination)?;
    let backup = Backup::new(&source, &mut destination_connection)?;
    backup.run_to_completion(128, std::time::Duration::from_millis(5), None)?;
    drop(backup);
    destination_connection.close().map_err(|(_, error)| error)?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o600))?;
    File::open(destination)?.sync_all()?;
    Ok(())
}

fn copy_strict_tree(source: &Path, destination: &Path, depth: usize) -> anyhow::Result<()> {
    ensure!(depth <= MAX_TREE_DEPTH, "data tree exceeds maximum depth");
    let source_metadata = fs::symlink_metadata(source)?;
    ensure!(
        source_metadata.is_dir(),
        "data tree contains a non-directory root"
    );
    fs::set_permissions(
        destination,
        fs::Permissions::from_mode(source_metadata.mode() & 0o7777),
    )?;
    let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let metadata = fs::symlink_metadata(entry.path())?;
        let target = destination.join(entry.file_name());
        if metadata.is_dir() {
            create_private_directory(&target)?;
            copy_strict_tree(&entry.path(), &target, depth + 1)?;
        } else if metadata.is_file() {
            ensure!(
                metadata.nlink() == 1,
                "data tree contains a hard-linked file"
            );
            copy_regular(&entry.path(), &target)?;
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(metadata.mode() & 0o7777),
            )?;
        } else {
            anyhow::bail!("data tree contains a symbolic link or special file")
        }
    }
    File::open(destination)?.sync_all()?;
    Ok(())
}

fn inventory_tree(root: &Path) -> anyhow::Result<TreeArchive> {
    let root_metadata = fs::symlink_metadata(root)?;
    ensure!(root_metadata.is_dir(), "data tree root is not a directory");
    let mode = root_metadata.mode() & 0o7777;
    let mut directories = Vec::new();
    let mut files = Vec::new();
    inventory_recursive(root, root, 0, &mut directories, &mut files)?;
    directories.sort_by(|left, right| left.path.cmp(&right.path));
    files.sort_by(|left, right| left.path.cmp(&right.path));
    ensure!(
        directories
            .len()
            .checked_add(files.len())
            .is_some_and(|count| count as u64 <= MAX_TREE_ENTRIES),
        "data tree exceeds maximum entry count"
    );
    let bytes = files
        .iter()
        .try_fold(0_u64, |sum, file| sum.checked_add(file.bytes))
        .context("tree byte count overflow")?;
    let canonical = serde_json::to_vec(&(mode, directories.as_slice(), files.as_slice(), bytes))?;
    Ok(TreeArchive {
        directory: TREE_DIRECTORY.to_owned(),
        mode,
        directories,
        files,
        bytes,
        sha256: lower_hex(&Sha256::digest(canonical)),
    })
}

fn verify_tree_archive(expected: &TreeArchive, root: &Path) -> anyhow::Result<()> {
    ensure!(
        *expected == inventory_tree(root)?,
        "backup tree inventory mismatch"
    );
    Ok(())
}

fn inventory_recursive(
    root: &Path,
    current: &Path,
    depth: usize,
    directories: &mut Vec<TreeDirectory>,
    files: &mut Vec<TreeFile>,
) -> anyhow::Result<()> {
    ensure!(depth <= MAX_TREE_DEPTH, "data tree exceeds maximum depth");
    let metadata = fs::symlink_metadata(current)?;
    ensure!(metadata.is_dir(), "data tree contains an invalid directory");
    if current != root {
        directories.push(TreeDirectory {
            path: relative_utf8(root, current)?,
            mode: metadata.mode() & 0o7777,
        });
    }
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_dir() {
            inventory_recursive(root, &entry.path(), depth + 1, directories, files)?;
        } else if metadata.is_file() {
            ensure!(
                metadata.nlink() == 1,
                "data tree contains a hard-linked file"
            );
            let path = relative_utf8(root, &entry.path())?;
            let (bytes, sha256) = hash_file(&entry.path())?;
            files.push(TreeFile {
                path,
                mode: metadata.mode() & 0o7777,
                bytes,
                sha256,
            });
        } else {
            anyhow::bail!("data tree contains a symbolic link or special file")
        }
    }
    Ok(())
}

fn validate_manifest(manifest: &CurrentBackupManifest) -> anyhow::Result<()> {
    let (version, revision, schema) = product_contract(manifest.product)?;
    ensure!(
        manifest.manifest_version == CURRENT_MANIFEST_VERSION,
        "unsupported current backup manifest version"
    );
    ensure!(
        manifest.tool_version == env!("CARGO_PKG_VERSION"),
        "backup was not produced by this exact tool version"
    );
    ensure!(
        manifest.application_version == version,
        "backup application version is not current"
    );
    ensure!(
        manifest.adapter_id == format!("{}-current-{version}-r1", manifest.product),
        "backup adapter id is not exact"
    );
    ensure!(
        manifest.schema_identity
            == SchemaIdentity {
                application: manifest.product.slug().to_owned(),
                application_version: version.to_owned(),
                schema_revision: revision,
                schema_sha256: schema.to_owned(),
            },
        "backup schema identity is not exact"
    );
    ensure!(
        manifest.database.path == DATABASE_FILE && manifest.tree.directory == TREE_DIRECTORY,
        "backup resource paths are not exact"
    );
    ensure!(
        manifest.tree.mode & !0o7777 == 0,
        "backup tree root mode is invalid"
    );
    ensure!(
        manifest
            .tree
            .files
            .iter()
            .try_fold(0_u64, |sum, file| sum.checked_add(file.bytes))
            == Some(manifest.tree.bytes),
        "backup tree byte count is invalid"
    );
    for file in std::iter::once(&manifest.database).chain(manifest.configuration.iter()) {
        validate_relative(&file.path)?;
        validate_sha256(&file.sha256)?;
    }
    for directory in &manifest.tree.directories {
        validate_relative(&directory.path)?;
    }
    for file in &manifest.tree.files {
        validate_relative(&file.path)?;
        validate_sha256(&file.sha256)?;
    }
    validate_sha256(&manifest.tree.sha256)?;
    validate_sha256(&manifest.source_tree_identity_sha256)?;
    match manifest.product {
        Product::MediaBackup => {
            ensure!(
                manifest.configuration.is_empty(),
                "Media backup configuration must be exactly empty"
            );
            ensure!(
                manifest.external_requirements.is_empty(),
                "Media backup unexpectedly requires an external key"
            );
        }
        Product::SentinelMonitor | Product::DufsRam => validate_product_resources(
            manifest.product,
            &manifest.configuration,
            &manifest.external_requirements,
        )?,
        _ => unreachable!(),
    }
    Ok(())
}

struct ProductLocks {
    _files: Vec<File>,
}

impl ProductLocks {
    fn acquire(
        product: Product,
        database: &Path,
        tree: &Path,
        _runtime: Option<&Path>,
    ) -> anyhow::Result<Self> {
        let mut files = Vec::new();
        let locks = match product {
            Product::MediaBackup => vec![
                sibling(database, "media-backup.lock")?,
                sibling(tree, "media-backup.lock")?,
            ],
            Product::SentinelMonitor | Product::DufsRam => vec![
                sibling(database, &format!("{}.lock", product.slug()))?,
                sibling(tree, &format!("{}.lock", product.slug()))?,
            ],
            _ => anyhow::bail!("unsupported product lock contract"),
        };
        let mut seen = BTreeSet::new();
        for path in locks {
            if !seen.insert(path.clone()) {
                continue;
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(&path)?;
            let metadata = file.metadata()?;
            ensure!(
                metadata.is_file() && metadata.nlink() == 1,
                "runtime lock is not a single-link regular file"
            );
            rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
                .map_err(|error| {
                    anyhow::anyhow!("product is running or lock acquisition failed: {error}")
                })?;
            files.push(file);
        }
        Ok(Self { _files: files })
    }
}

fn read_manifest(directory: &Path) -> anyhow::Result<CurrentBackupManifest> {
    let metadata = fs::symlink_metadata(directory.join(MANIFEST_FILE))?;
    ensure!(
        metadata.is_file() && metadata.nlink() == 1 && metadata.len() <= MAX_MANIFEST_BYTES,
        "backup manifest is not a bounded regular file"
    );
    Ok(serde_json::from_slice(&fs::read(
        directory.join(MANIFEST_FILE),
    )?)?)
}

fn verify_current_backup_root(directory: &Path) -> anyhow::Result<()> {
    let root_metadata = fs::symlink_metadata(directory)?;
    ensure!(
        root_metadata.is_dir(),
        "backup root must be a directory, not a symbolic link or special file"
    );

    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("backup root entry name is not UTF-8"))?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if name == TREE_DIRECTORY {
            ensure!(metadata.is_dir(), "backup tree is not a directory");
        } else {
            ensure!(
                metadata.is_file() && metadata.nlink() == 1,
                "backup root file {name} is not a single-link regular file"
            );
        }
        ensure!(actual.insert(name), "backup root entry is duplicated");
    }
    ensure!(
        actual.contains(MANIFEST_FILE),
        "backup root has no manifest"
    );
    let manifest = read_manifest(directory)?;
    let mut expected = [DATABASE_FILE, MANIFEST_FILE, TREE_DIRECTORY]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    for configuration in &manifest.configuration {
        validate_name(&configuration.path)?;
        expected.insert(configuration.path.clone());
    }
    ensure!(
        actual == expected,
        "backup root does not contain the exact current resource set"
    );
    Ok(())
}

fn current_file(name: &str, path: &Path) -> anyhow::Result<CurrentFile> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && metadata.nlink() == 1,
        "backup resource is not a single-link regular file"
    );
    let (bytes, sha256) = hash_file(path)?;
    Ok(CurrentFile {
        path: name.to_owned(),
        bytes,
        sha256,
        mode: metadata.mode() & 0o7777,
    })
}

fn copy_regular(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    ensure!(
        metadata.is_file() && metadata.nlink() == 1,
        "source is not a single-link regular file"
    );
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(metadata.mode() & 0o7777)
        .open(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()?;
    ensure!(
        hash_file(source)? == hash_file(destination)?,
        "file changed while it was copied"
    );
    Ok(())
}

fn hash_file(path: &Path) -> anyhow::Result<(u64, String)> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    let mut bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(read as u64)
            .context("file byte count overflow")?;
        hasher.update(&buffer[..read]);
    }
    ensure!(metadata.len() == bytes, "file changed while it was hashed");
    Ok((bytes, lower_hex(&hasher.finalize())))
}

fn write_json_create_new(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn write_journal(recovery: &Path, journal: &RestoreJournal) -> anyhow::Result<()> {
    write_json_create_new(&recovery.join("restore-journal.json"), journal)?;
    File::open(recovery)?.sync_all()?;
    Ok(())
}

fn replace_journal(recovery: &Path, journal: &RestoreJournal) -> anyhow::Result<()> {
    let pending = recovery.join("restore-journal.pending");
    write_json_create_new(&pending, journal)?;
    fs::rename(&pending, recovery.join("restore-journal.json"))?;
    File::open(recovery)?.sync_all()?;
    Ok(())
}

fn cleanup_recovery_directory(recovery: &Path, journal: &RestoreJournal) -> anyhow::Result<()> {
    if path_exists(&journal.database_original)? {
        fs::remove_file(&journal.database_original)?;
    }
    if path_exists(&journal.tree_original)? {
        fs::remove_dir_all(&journal.tree_original)?;
    }
    if path_exists(&journal.database_stage)? {
        fs::remove_file(&journal.database_stage)?;
    }
    if path_exists(&journal.tree_stage)? {
        fs::remove_dir_all(&journal.tree_stage)?;
    }
    for configuration in &journal.configuration_targets {
        if path_exists(&configuration.original)? {
            fs::remove_file(&configuration.original)?;
        }
        if path_exists(&configuration.stage)? {
            fs::remove_file(&configuration.stage)?;
        }
    }
    discard_uncommitted_journal_update(recovery)?;
    fs::remove_file(recovery.join("restore-journal.json"))?;
    fs::remove_dir(recovery)?;
    sync_parent(&journal.database)?;
    sync_parent(&journal.tree)?;
    for configuration in &journal.configuration_targets {
        sync_parent(&configuration.target)?;
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> anyhow::Result<()> {
    fs::create_dir(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn sibling(path: &Path, suffix: &str) -> anyhow::Result<PathBuf> {
    let parent = path.parent().context("path has no parent")?;
    let name = path.file_name().context("path has no file name")?;
    let mut sibling = std::ffi::OsString::from(".");
    sibling.push(name);
    sibling.push(".");
    sibling.push(suffix);
    Ok(parent.join(sibling))
}

fn path_exists(path: &Path) -> anyhow::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn canonical_existing_directory(path: &Path, label: &str) -> anyhow::Result<PathBuf> {
    ensure!(path.is_absolute(), "{label} path must be absolute");
    let metadata = fs::symlink_metadata(path)?;
    ensure!(metadata.is_dir(), "{label} must be a directory");
    let canonical = fs::canonicalize(path)?;
    ensure!(canonical == path, "{label} path must already be canonical");
    Ok(canonical)
}

fn canonical_target_path(path: &Path) -> anyhow::Result<PathBuf> {
    ensure!(path.is_absolute(), "current target path must be absolute");
    let parent = path.parent().context("current target path has no parent")?;
    let name = path
        .file_name()
        .context("current target path has no file name")?;
    ensure!(
        matches!(path.components().next_back(), Some(Component::Normal(_))),
        "current target name is not one safe path component"
    );
    let canonical_parent = fs::canonicalize(parent)?;
    ensure!(
        canonical_parent == parent,
        "current target parent path must already be canonical"
    );
    let canonical = canonical_parent.join(name);
    ensure!(
        canonical == path,
        "current target path must already be canonical"
    );
    if path_exists(path)? {
        ensure!(
            fs::canonicalize(path)? == canonical,
            "current target must not be a symbolic link"
        );
    }
    Ok(canonical)
}

fn existing_path_identity_sha256(domain: &[u8], path: &Path) -> anyhow::Result<String> {
    let canonical = fs::canonicalize(path)?;
    ensure!(canonical == path, "identity path is not canonical");
    let metadata = fs::symlink_metadata(path)?;
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hash_length_framed(&mut hasher, canonical.as_os_str().as_bytes());
    hasher.update(metadata.dev().to_be_bytes());
    hasher.update(metadata.ino().to_be_bytes());
    Ok(lower_hex(&hasher.finalize()))
}

fn target_path_identity_sha256(path: &Path) -> anyhow::Result<String> {
    let canonical = canonical_target_path(path)?;
    let parent = canonical
        .parent()
        .context("current target path has no parent")?;
    let metadata = fs::symlink_metadata(parent)?;
    let mut hasher = Sha256::new();
    hasher.update(b"sarmg-current-target-path-v1\0");
    hash_length_framed(&mut hasher, canonical.as_os_str().as_bytes());
    hasher.update(metadata.dev().to_be_bytes());
    hasher.update(metadata.ino().to_be_bytes());
    Ok(lower_hex(&hasher.finalize()))
}

fn ensure_source_and_targets_are_disjoint(
    source: &Path,
    database: &Path,
    tree: &Path,
) -> anyhow::Result<()> {
    ensure!(database != tree, "current restore targets are not distinct");
    ensure!(
        !database.starts_with(source)
            && !tree.starts_with(source)
            && !source.starts_with(tree)
            && !database.starts_with(tree)
            && !tree.starts_with(database),
        "current restore source and targets must be disjoint"
    );
    Ok(())
}

fn ensure_configuration_targets_are_disjoint(
    source: &Path,
    database: &Path,
    tree: &Path,
    configuration: &[NamedFile],
) -> anyhow::Result<()> {
    let mut targets = BTreeSet::new();
    targets.insert(database.to_path_buf());
    for file in configuration {
        ensure!(
            !file.path.starts_with(source)
                && !source.starts_with(&file.path)
                && !file.path.starts_with(tree)
                && !tree.starts_with(&file.path),
            "current restore configuration targets must be disjoint from backup and data tree"
        );
        ensure!(
            targets.insert(file.path.clone()),
            "current restore configuration targets are not distinct"
        );
    }
    Ok(())
}

fn hash_length_framed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn relative_utf8(root: &Path, path: &Path) -> anyhow::Result<String> {
    let relative = path.strip_prefix(root)?;
    validate_relative(relative)?;
    relative
        .to_str()
        .map(str::to_owned)
        .context("data tree path is not UTF-8")
}

fn validate_name(name: &str) -> anyhow::Result<()> {
    validate_relative(Path::new(name))?;
    ensure!(
        Path::new(name).components().count() == 1,
        "configuration name must be one component"
    );
    Ok(())
}

fn validate_relative(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();
    ensure!(
        !path.as_os_str().is_empty()
            && !path.is_absolute()
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "path is not a safe relative path"
    );
    Ok(())
}

fn validate_sha256(value: &str) -> anyhow::Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "SHA-256 is not canonical lowercase hexadecimal"
    );
    Ok(())
}

fn path_identity_sha256(path: &Path) -> anyhow::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    let mut hasher = Sha256::new();
    hasher.update(b"sarmg-current-tree-identity-v1\0");
    hasher.update(metadata.dev().to_be_bytes());
    hasher.update(metadata.ino().to_be_bytes());
    Ok(lower_hex(&hasher.finalize()))
}

fn sync_parent(path: &Path) -> anyhow::Result<()> {
    File::open(path.parent().context("path has no parent")?)?.sync_all()?;
    Ok(())
}

fn now_seconds() -> anyhow::Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn create_exact_backup_root(path: &Path) {
        fs::create_dir(path).unwrap();
        fs::write(path.join(DATABASE_FILE), b"database").unwrap();
        fs::write(
            path.join(MANIFEST_FILE),
            serde_json::to_vec_pretty(&test_manifest()).unwrap(),
        )
        .unwrap();
        fs::create_dir(path.join(TREE_DIRECTORY)).unwrap();
    }

    fn test_current_file(path: &str) -> CurrentFile {
        CurrentFile {
            path: path.to_owned(),
            bytes: 1,
            sha256: "0".repeat(64),
            mode: 0o600,
        }
    }

    fn test_tree_archive() -> TreeArchive {
        TreeArchive {
            directory: TREE_DIRECTORY.to_owned(),
            mode: 0o700,
            directories: Vec::new(),
            files: Vec::new(),
            bytes: 0,
            sha256: "0".repeat(64),
        }
    }

    fn test_manifest() -> CurrentBackupManifest {
        CurrentBackupManifest {
            manifest_version: CURRENT_MANIFEST_VERSION,
            adapter_id: format!(
                "{}-current-{}-r1",
                Product::MediaBackup,
                MEDIA_CURRENT_APPLICATION_VERSION
            ),
            tool_version: env!("CARGO_PKG_VERSION").to_owned(),
            product: Product::MediaBackup,
            application_version: MEDIA_CURRENT_APPLICATION_VERSION.to_owned(),
            schema_identity: SchemaIdentity {
                application: Product::MediaBackup.slug().to_owned(),
                application_version: MEDIA_CURRENT_APPLICATION_VERSION.to_owned(),
                schema_revision: MEDIA_SCHEMA_REVISION,
                schema_sha256: MEDIA_SCHEMA_SHA256.to_owned(),
            },
            created_at_epoch_seconds: 1,
            source_tree_identity_sha256: "0".repeat(64),
            database: test_current_file(DATABASE_FILE),
            configuration: Vec::new(),
            tree: test_tree_archive(),
            external_requirements: Vec::new(),
        }
    }

    fn sentinel_test_envelope(key: &[u8; 32], camera_id: Uuid, plaintext: &str) -> Vec<u8> {
        let mut derived_key = [0_u8; 32];
        Hkdf::<Sha256>::new(Some(SENTINEL_KEY_DERIVATION_SALT), key)
            .expand(SENTINEL_KEY_DERIVATION_INFO, &mut derived_key)
            .unwrap();
        let cipher = Aes256Gcm::new_from_slice(&derived_key).unwrap();
        let nonce = [3_u8; 12];
        let aad = sentinel_credential_aad(camera_id, "main_stream_url_enc");
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &aad,
                },
            )
            .unwrap();
        serde_json::to_vec(&SentinelCredentialEnvelope {
            product: Product::SentinelMonitor.slug().to_owned(),
            application_version: SENTINEL_CURRENT_APPLICATION_VERSION.to_owned(),
            envelope_revision: 1,
            key_id: SENTINEL_KEY_ID.to_owned(),
            nonce: URL_SAFE_NO_PAD.encode(nonce),
            ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
        })
        .unwrap()
    }

    fn test_restore_journal(root: &Path) -> (PathBuf, RestoreJournal) {
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        let database = root.join("media.sqlite3");
        let tree = root.join("media");
        let nonce = Uuid::new_v4().simple().to_string();
        let recovery = sibling(&database, &format!("recovery-{nonce}")).unwrap();
        fs::create_dir(&recovery).unwrap();
        let manifest = test_manifest();
        let journal = RestoreJournal {
            journal_version: CURRENT_RESTORE_JOURNAL_VERSION,
            tool_version: env!("CARGO_PKG_VERSION").to_owned(),
            product: Product::MediaBackup,
            application_version: manifest.application_version.clone(),
            adapter_id: manifest.adapter_id.clone(),
            schema_identity: manifest.schema_identity.clone(),
            created_at_epoch_seconds: 1,
            source_backup: source.clone(),
            source_backup_identity_sha256: existing_path_identity_sha256(
                b"sarmg-current-source-backup-v1\0",
                &source,
            )
            .unwrap(),
            source_manifest_version: CURRENT_MANIFEST_VERSION,
            source_manifest_created_at_epoch_seconds: 1,
            source_manifest_bytes: 1,
            source_manifest_sha256: "0".repeat(64),
            source_tree_identity_sha256: manifest.source_tree_identity_sha256.clone(),
            database: database.clone(),
            tree: tree.clone(),
            database_path_identity_sha256: target_path_identity_sha256(&database).unwrap(),
            tree_path_identity_sha256: target_path_identity_sha256(&tree).unwrap(),
            database_stage: sibling(&database, &format!("incoming-{nonce}")).unwrap(),
            tree_stage: sibling(&tree, &format!("incoming-{nonce}")).unwrap(),
            database_original: sibling(&database, &format!("original-{nonce}")).unwrap(),
            tree_original: sibling(&tree, &format!("original-{nonce}")).unwrap(),
            incoming_database: manifest.database,
            incoming_tree: manifest.tree,
            original_database: None,
            original_tree: None,
            configuration: Vec::new(),
            configuration_targets: Vec::new(),
            external_requirements: Vec::new(),
            phase: RestorePhase::Prepared,
        };
        (recovery, journal)
    }

    #[test]
    fn backup_root_requires_exact_regular_resources() {
        let root = tempfile::tempdir().unwrap();
        let backup = root.path().join("backup");
        create_exact_backup_root(&backup);
        verify_current_backup_root(&backup).unwrap();

        fs::write(backup.join("extra"), b"not allowed").unwrap();
        assert!(verify_current_backup_root(&backup).is_err());
        fs::remove_file(backup.join("extra")).unwrap();

        fs::remove_file(backup.join(MANIFEST_FILE)).unwrap();
        symlink(root.path().join("outside"), backup.join(MANIFEST_FILE)).unwrap();
        assert!(verify_current_backup_root(&backup).is_err());
        fs::remove_file(backup.join(MANIFEST_FILE)).unwrap();
        fs::write(backup.join(MANIFEST_FILE), b"manifest").unwrap();

        fs::remove_file(backup.join(DATABASE_FILE)).unwrap();
        fs::create_dir(backup.join(DATABASE_FILE)).unwrap();
        assert!(verify_current_backup_root(&backup).is_err());
    }

    #[test]
    fn sentinel_external_key_must_authenticate_every_credential_envelope() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("sentinel.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(include_str!(
                "../tests/fixtures/sources/sentinel-monitor/0.2.0/database.sql"
            ))
            .unwrap();
        let camera_id = Uuid::new_v4();
        let key = [7_u8; 32];
        let envelope = sentinel_test_envelope(&key, camera_id, "rtsp://camera.invalid/main");
        connection
            .execute(
                "INSERT INTO cameras(\
                    id,name,main_stream_url_enc,created_at,updated_at\
                 ) VALUES(?1,'Camera',?2,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
                (camera_id.hyphenated().to_string(), envelope),
            )
            .unwrap();
        drop(connection);

        verify_sentinel_credentials(&database, &key).unwrap();
        assert!(verify_sentinel_credentials(&database, &[8_u8; 32]).is_err());
    }

    #[test]
    fn tree_inventory_binds_root_mode_into_identity() {
        let root = tempfile::tempdir().unwrap();
        let tree = root.path().join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("blob"), b"content").unwrap();
        fs::set_permissions(&tree, fs::Permissions::from_mode(0o750)).unwrap();
        let first = inventory_tree(&tree).unwrap();
        assert_eq!(first.mode, 0o750);

        fs::set_permissions(&tree, fs::Permissions::from_mode(0o700)).unwrap();
        let second = inventory_tree(&tree).unwrap();
        assert_eq!(second.mode, 0o700);
        assert_ne!(first, second);
        assert_ne!(first.sha256, second.sha256);
        assert!(verify_tree_archive(&first, &tree).is_err());
        verify_tree_archive(&second, &tree).unwrap();
    }

    #[test]
    fn media_verify_rejects_handwritten_manifest_configuration() {
        let root = tempfile::tempdir().unwrap();
        let backup = root.path().join("backup");
        create_exact_backup_root(&backup);
        let mut manifest = test_manifest();
        manifest
            .configuration
            .push(test_current_file("handwritten.conf"));
        fs::write(backup.join("handwritten.conf"), b"x").unwrap();
        fs::write(
            backup.join(MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let error = verify_current_backup(&CompositeCurrentOptions {
            product: Product::MediaBackup,
            database: root.path().join("unused.sqlite3"),
            tree: root.path().join("unused-tree"),
            output: backup,
            runtime_directory: None,
            configuration: Vec::new(),
            credentials_key_id: None,
            credentials_key: None,
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("configuration must be exactly empty")
        );
    }

    #[test]
    fn current_journal_rejects_tampered_phase_path_hash_and_configuration() {
        let root = tempfile::tempdir().unwrap();
        let (recovery, journal) = test_restore_journal(root.path());
        validate_restore_journal(&journal, &recovery).unwrap();

        let mut old = journal.clone();
        old.journal_version = 1;
        assert!(validate_restore_journal(&old, &recovery).is_err());

        let mut phase = journal.clone();
        phase.phase = RestorePhase::OriginalsPreserved;
        assert!(validate_restore_journal(&phase, &recovery).is_err());
        let mut unknown_phase = serde_json::to_value(&journal).unwrap();
        unknown_phase["phase"] = serde_json::Value::String("unknown-phase".to_owned());
        assert!(serde_json::from_value::<RestoreJournal>(unknown_phase).is_err());

        let mut path = journal.clone();
        path.tree = root.path().join("other-tree");
        assert!(validate_restore_journal(&path, &recovery).is_err());

        let mut hash = journal.clone();
        hash.database_path_identity_sha256 = "0".repeat(64);
        assert!(validate_restore_journal(&hash, &recovery).is_err());

        fs::write(journal.source_backup.join(DATABASE_FILE), b"database").unwrap();
        fs::create_dir(journal.source_backup.join(TREE_DIRECTORY)).unwrap();
        fs::write(journal.source_backup.join(MANIFEST_FILE), b"{}").unwrap();
        let mut content_hash = journal.clone();
        content_hash.source_manifest_bytes = 2;
        content_hash.source_manifest_sha256 = "f".repeat(64);
        let error = verify_recovery_source(&content_hash).unwrap_err();
        assert!(error.to_string().contains("manifest content hash mismatch"));

        let mut configuration = journal;
        configuration
            .configuration
            .push(test_current_file("handwritten.conf"));
        assert!(validate_restore_journal(&configuration, &recovery).is_err());
    }

    #[test]
    fn current_recovery_fails_non_blocking_when_either_product_lock_is_held() {
        for hold_database_lock in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let (recovery, journal) = test_restore_journal(root.path());
            write_journal(&recovery, &journal).unwrap();
            let held_path = if hold_database_lock {
                sibling(&journal.database, "media-backup.lock").unwrap()
            } else {
                sibling(&journal.tree, "media-backup.lock").unwrap()
            };
            let held = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(held_path)
                .unwrap();
            rustix::fs::flock(&held, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
            let error = recover_current(&CurrentRecoveryOptions {
                product: Product::MediaBackup,
                expected_application_version: MEDIA_CURRENT_APPLICATION_VERSION.to_owned(),
                input: journal.source_backup.clone(),
                database: journal.database.clone(),
                tree: journal.tree.clone(),
                runtime_directory: None,
                recovery_directory: recovery,
                action: CurrentRecoveryAction::Commit,
                credentials_key_id: None,
                credentials_key: None,
            })
            .unwrap_err();
            assert!(error.to_string().contains("lock acquisition failed"));
        }
    }
}

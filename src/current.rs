use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, ensure};
use rusqlite::{Connection, OpenFlags, backup::Backup};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{ExternalRequirement, Product, SchemaIdentity};

const MANIFEST_FILE: &str = "manifest.json";
const DATABASE_FILE: &str = "database.sqlite3";
const TREE_DIRECTORY: &str = "tree";
const CURRENT_MANIFEST_VERSION: u32 = 2;
const MAX_MANIFEST_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TREE_ENTRIES: u64 = 2_000_000;
const MAX_TREE_DEPTH: usize = 128;

const MEDIA_VERSION: &str = "0.2.0";
const MEDIA_SCHEMA_REVISION: u64 = 1;
// Updated by the final contract-generation pass whenever the current schema changes.
const MEDIA_SCHEMA_SHA256: &str =
    "a464584cf7a55f9e50cb85bb539b1f42a9285f707440bb0bcfcd31a6b3a083c0";

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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RestoreJournal {
    journal_version: u32,
    product: Product,
    database: PathBuf,
    tree: PathBuf,
    database_stage: PathBuf,
    tree_stage: PathBuf,
    database_original: PathBuf,
    tree_original: PathBuf,
    configuration: Vec<RestoreFileJournal>,
    phase: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RestoreFileJournal {
    name: String,
    target: PathBuf,
    stage: PathBuf,
    original: PathBuf,
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

    let pending = pending_path(&options.output)?;
    create_private_directory(&pending)?;
    let result = (|| {
        let database_output = pending.join(DATABASE_FILE);
        copy_sqlite_snapshot(&options.database, &database_output)?;
        ensure!(
            verify_database(options.product, &database_output)? == identity,
            "database identity changed while the current backup was created"
        );
        verify_external_key(options.product, &database_output, options.credentials())?;

        let tree_output = pending.join(TREE_DIRECTORY);
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
            let destination = pending.join(&named.name);
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
        write_json_create_new(&pending.join(MANIFEST_FILE), &manifest)?;
        File::open(&pending)?.sync_all()?;
        fs::rename(&pending, &options.output)?;
        sync_parent(&options.output)?;
        verify_current_backup(options)
    })();
    if result.is_err() && pending.exists() {
        let _ = fs::remove_dir_all(&pending);
    }
    result
}

pub fn verify_current_backup(
    options: &CompositeCurrentOptions,
) -> anyhow::Result<CurrentStateResult> {
    validate_options(options)?;
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
    ensure!(
        manifest.tree == inventory_tree(&options.output.join(TREE_DIRECTORY))?,
        "backup tree inventory mismatch"
    );
    for file in &manifest.configuration {
        ensure!(
            *file == current_file(&file.path, &options.output.join(&file.path))?,
            "backup configuration digest mismatch"
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
    let verify = CompositeCurrentOptions {
        product: options.product,
        database: options.database.clone(),
        tree: options.tree.clone(),
        output: options.input.clone(),
        runtime_directory: options.runtime_directory.clone(),
        configuration: options.configuration.clone(),
        credentials_key_id: options.credentials_key_id.clone(),
        credentials_key: options.credentials_key,
    };
    verify_current_backup(&verify)?;
    let _locks = ProductLocks::acquire(
        options.product,
        &options.database,
        &options.tree,
        options.runtime_directory.as_deref(),
    )?;
    let database_exists = options.database.exists();
    let tree_exists = options.tree.exists();
    let configuration_exists = options
        .configuration
        .iter()
        .map(|file| file.path.exists())
        .collect::<Vec<_>>();
    ensure!(
        options.replace_existing || (!database_exists && !tree_exists),
        "restore targets already exist; pass --replace-existing"
    );
    ensure!(
        database_exists == tree_exists,
        "restore target contains a mixed database/data-tree generation"
    );
    ensure!(
        configuration_exists
            .iter()
            .all(|exists| *exists == database_exists),
        "restore target contains a mixed configuration generation"
    );
    if database_exists {
        verify_database(options.product, &options.database)
            .context("existing restore database is not the exact current generation")?;
        verify_product_state(options.product, &options.database, &options.tree)
            .context("existing restore tree is not the exact current generation")?;
        verify_configuration(options.product, &options.configuration, &options.tree)
            .context("existing restore configuration is not the exact current generation")?;
    }

    let nonce = Uuid::new_v4().simple().to_string();
    let database_stage = sibling(&options.database, &format!("incoming-{nonce}"))?;
    let tree_stage = sibling(&options.tree, &format!("incoming-{nonce}"))?;
    let database_original = sibling(&options.database, &format!("original-{nonce}"))?;
    let tree_original = sibling(&options.tree, &format!("original-{nonce}"))?;
    let recovery = sibling(&options.database, &format!("recovery-{nonce}"))?;
    copy_regular(&options.input.join(DATABASE_FILE), &database_stage)?;
    create_private_directory(&tree_stage)?;
    copy_strict_tree(&options.input.join(TREE_DIRECTORY), &tree_stage, 0)?;
    let manifest = read_manifest(&options.input)?;
    let manifest_configuration = manifest
        .configuration
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    let mut configuration = Vec::new();
    for target in &options.configuration {
        let stored = manifest_configuration
            .get(target.name.as_str())
            .with_context(|| format!("backup lacks configuration {}", target.name))?;
        let stage = sibling(&target.path, &format!("incoming-{nonce}"))?;
        let original = sibling(&target.path, &format!("original-{nonce}"))?;
        copy_regular(&options.input.join(&stored.path), &stage)?;
        configuration.push(RestoreFileJournal {
            name: target.name.clone(),
            target: target.path.clone(),
            stage,
            original,
        });
    }
    ensure!(
        configuration.len() == manifest.configuration.len(),
        "restore configuration target set is not exact"
    );
    verify_database(options.product, &database_stage)?;
    verify_product_state(options.product, &database_stage, &tree_stage)?;
    create_private_directory(&recovery)?;
    let mut journal = RestoreJournal {
        journal_version: 1,
        product: options.product,
        database: options.database.clone(),
        tree: options.tree.clone(),
        database_stage,
        tree_stage,
        database_original,
        tree_original,
        configuration,
        phase: "prepared".to_owned(),
    };
    write_journal(&recovery, &journal)?;
    if database_exists {
        fs::rename(&journal.database, &journal.database_original)?;
        fs::rename(&journal.tree, &journal.tree_original)?;
        for file in &journal.configuration {
            fs::rename(&file.target, &file.original)?;
        }
        journal.phase = "originals-preserved".to_owned();
        replace_journal(&recovery, &journal)?;
    }
    fs::rename(&journal.tree_stage, &journal.tree)?;
    fs::rename(&journal.database_stage, &journal.database)?;
    for file in &journal.configuration {
        fs::rename(&file.stage, &file.target)?;
    }
    journal.phase = "installed".to_owned();
    replace_journal(&recovery, &journal)?;
    verify_database(options.product, &journal.database)?;
    verify_product_state(options.product, &journal.database, &journal.tree)?;
    verify_external_key(
        options.product,
        &journal.database,
        options
            .credentials_key_id
            .as_deref()
            .zip(options.credentials_key.as_ref()),
    )?;
    verify_configuration_journal(options.product, &journal.configuration, &journal.tree)?;
    journal.phase = "verified".to_owned();
    replace_journal(&recovery, &journal)?;
    cleanup_committed(&recovery, &journal)?;
    let manifest = read_manifest(&options.input)?;
    Ok(CurrentStateResult {
        product: options.product,
        application_version: manifest.application_version,
        directory: absolute(&options.database)?,
    })
}

pub fn recover_current(options: &CurrentRecoveryOptions) -> anyhow::Result<CurrentStateResult> {
    let recovery = absolute(&options.recovery_directory)?;
    let journal: RestoreJournal =
        serde_json::from_slice(&fs::read(recovery.join("restore-journal.json"))?)?;
    ensure!(
        journal.journal_version == 1,
        "unsupported current restore journal"
    );
    match options.action {
        CurrentRecoveryAction::Commit => {
            if !journal.tree.exists() && journal.tree_stage.exists() {
                fs::rename(&journal.tree_stage, &journal.tree)?;
            }
            if !journal.database.exists() && journal.database_stage.exists() {
                fs::rename(&journal.database_stage, &journal.database)?;
            }
            for file in &journal.configuration {
                if !file.target.exists() && file.stage.exists() {
                    fs::rename(&file.stage, &file.target)?;
                }
            }
            verify_database(journal.product, &journal.database)?;
            verify_product_state(journal.product, &journal.database, &journal.tree)?;
            verify_external_key(
                journal.product,
                &journal.database,
                options
                    .credentials_key_id
                    .as_deref()
                    .zip(options.credentials_key.as_ref()),
            )?;
            verify_configuration_journal(journal.product, &journal.configuration, &journal.tree)?;
            cleanup_committed(&recovery, &journal)?;
        }
        CurrentRecoveryAction::Rollback => {
            let had_original_generation = journal.database_original.exists()
                || journal.tree_original.exists()
                || journal
                    .configuration
                    .iter()
                    .any(|file| file.original.exists());
            if journal.database_original.exists() {
                if journal.database.exists() {
                    fs::rename(&journal.database, &journal.database_stage)?;
                }
                fs::rename(&journal.database_original, &journal.database)?;
            }
            if journal.tree_original.exists() {
                if journal.tree.exists() {
                    fs::rename(&journal.tree, &journal.tree_stage)?;
                }
                fs::rename(&journal.tree_original, &journal.tree)?;
            }
            for file in &journal.configuration {
                if file.original.exists() {
                    if file.target.exists() {
                        fs::rename(&file.target, &file.stage)?;
                    }
                    fs::rename(&file.original, &file.target)?;
                }
            }
            if !had_original_generation && journal.phase != "prepared" {
                if journal.database.exists() {
                    fs::remove_file(&journal.database)?;
                }
                if journal.tree.exists() {
                    fs::remove_dir_all(&journal.tree)?;
                }
                for file in &journal.configuration {
                    if file.target.exists() {
                        fs::remove_file(&file.target)?;
                    }
                }
            }
            if had_original_generation {
                verify_database(journal.product, &journal.database)?;
                verify_product_state(journal.product, &journal.database, &journal.tree)?;
                verify_external_key(
                    journal.product,
                    &journal.database,
                    options
                        .credentials_key_id
                        .as_deref()
                        .zip(options.credentials_key.as_ref()),
                )?;
                verify_configuration_journal(
                    journal.product,
                    &journal.configuration,
                    &journal.tree,
                )?;
            }
            if journal.database_stage.exists() {
                fs::remove_file(&journal.database_stage)?;
            }
            if journal.tree_stage.exists() {
                fs::remove_dir_all(&journal.tree_stage)?;
            }
            for file in &journal.configuration {
                if file.stage.exists() {
                    fs::remove_file(&file.stage)?;
                }
            }
            fs::remove_dir(&recovery)?;
        }
    }
    Ok(CurrentStateResult {
        product: journal.product,
        application_version: product_contract(journal.product)?.0.to_owned(),
        directory: journal.database,
    })
}

impl CompositeCurrentOptions {
    fn credentials(&self) -> Option<(&str, &[u8; 32])> {
        self.credentials_key_id
            .as_deref()
            .zip(self.credentials_key.as_ref())
    }
}

fn validate_options(options: &CompositeCurrentOptions) -> anyhow::Result<()> {
    product_contract(options.product)?;
    for path in [&options.database, &options.tree, &options.output] {
        ensure!(path.is_absolute(), "current adapter paths must be absolute");
    }
    ensure!(
        !options.output.starts_with(&options.tree) && !options.tree.starts_with(&options.output),
        "backup output and data tree must be disjoint"
    );
    match options.product {
        Product::MediaBackup => ensure!(
            options.credentials().is_none() && options.configuration.is_empty(),
            "Media current adapter does not accept external key or configuration resources"
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
        Product::MediaBackup => Ok((MEDIA_VERSION, MEDIA_SCHEMA_REVISION, MEDIA_SCHEMA_SHA256)),
        _ => anyhow::bail!("unsupported composite current product {product}"),
    }
}

fn verify_database(product: Product, path: &Path) -> anyhow::Result<SchemaIdentity> {
    let (version, revision, expected_sha) = product_contract(product)?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    ensure!(
        integrity.eq_ignore_ascii_case("ok"),
        "SQLite integrity check failed"
    );
    ensure!(
        connection
            .prepare("PRAGMA foreign_key_check")?
            .query([])?
            .next()?
            .is_none(),
        "SQLite foreign-key check failed"
    );
    let row: (i64, String, String, i64, String) = connection.query_row(
        "SELECT singleton,application,application_version,schema_revision,schema_sha256 FROM product_metadata",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )?;
    let fingerprint = crate::sqlite::schema_fingerprint(path)?;
    ensure!(
        row == (
            1,
            product.slug().to_owned(),
            version.to_owned(),
            revision as i64,
            expected_sha.to_owned()
        ) && fingerprint == expected_sha,
        "database is not the exact official current {product} contract"
    );
    Ok(SchemaIdentity {
        application: product.slug().to_owned(),
        application_version: version.to_owned(),
        schema_revision: revision,
        schema_sha256: fingerprint,
    })
}

fn verify_external_key(
    product: Product,
    _database: &Path,
    credentials: Option<(&str, &[u8; 32])>,
) -> anyhow::Result<()> {
    match (product, credentials) {
        (Product::MediaBackup, None) => Ok(()),
        _ => anyhow::bail!("external credentials are not valid for {product}"),
    }
}

fn external_requirements(
    options: &CompositeCurrentOptions,
) -> anyhow::Result<Vec<ExternalRequirement>> {
    Ok(match options.credentials() {
        Some((kid, key)) => vec![ExternalRequirement {
            kind: "credentials-key".to_owned(),
            kid: kid.to_owned(),
            sha256: lower_hex(&Sha256::digest(key)),
            algorithm: "aes-256-gcm-hkdf-sha256".to_owned(),
            envelope_version: 1,
        }],
        None => Vec::new(),
    })
}

fn verify_product_state(product: Product, database: &Path, tree: &Path) -> anyhow::Result<()> {
    match product {
        Product::MediaBackup => verify_media_state(database, tree),
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
        _ => anyhow::bail!("unsupported current product {product}"),
    }
}

fn verify_configuration_journal(
    product: Product,
    files: &[RestoreFileJournal],
    tree: &Path,
) -> anyhow::Result<()> {
    let named = files
        .iter()
        .map(|file| NamedFile {
            name: file.name.clone(),
            path: file.target.clone(),
        })
        .collect::<Vec<_>>();
    verify_configuration(product, &named, tree)
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
    let canonical = serde_json::to_vec(&(directories.as_slice(), files.as_slice(), bytes))?;
    Ok(TreeArchive {
        directory: TREE_DIRECTORY.to_owned(),
        directories,
        files,
        bytes,
        sha256: lower_hex(&Sha256::digest(canonical)),
    })
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
        Product::MediaBackup => ensure!(
            manifest.external_requirements.is_empty(),
            "Media backup unexpectedly requires an external key"
        ),
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
    write_json_create_new(&recovery.join("restore-journal.json"), journal)
}

fn replace_journal(recovery: &Path, journal: &RestoreJournal) -> anyhow::Result<()> {
    let pending = recovery.join("restore-journal.pending");
    write_json_create_new(&pending, journal)?;
    fs::rename(&pending, recovery.join("restore-journal.json"))?;
    File::open(recovery)?.sync_all()?;
    Ok(())
}

fn cleanup_committed(recovery: &Path, journal: &RestoreJournal) -> anyhow::Result<()> {
    if journal.database_original.exists() {
        fs::remove_file(&journal.database_original)?;
    }
    if journal.tree_original.exists() {
        fs::remove_dir_all(&journal.tree_original)?;
    }
    if journal.database_stage.exists() {
        fs::remove_file(&journal.database_stage)?;
    }
    if journal.tree_stage.exists() {
        fs::remove_dir_all(&journal.tree_stage)?;
    }
    for file in &journal.configuration {
        if file.original.exists() {
            fs::remove_file(&file.original)?;
        }
        if file.stage.exists() {
            fs::remove_file(&file.stage)?;
        }
        sync_parent(&file.target)?;
    }
    fs::remove_file(recovery.join("restore-journal.json"))?;
    fs::remove_dir(recovery)?;
    sync_parent(&journal.database)?;
    sync_parent(&journal.tree)?;
    Ok(())
}

fn create_private_directory(path: &Path) -> anyhow::Result<()> {
    fs::create_dir(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn pending_path(output: &Path) -> anyhow::Result<PathBuf> {
    ensure!(!output.exists(), "backup output already exists");
    sibling(output, &format!("pending-{}", Uuid::new_v4().simple()))
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

fn absolute(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
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

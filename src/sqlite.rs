use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::fd::AsRawFd,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Context, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rusqlite::{Connection, OpenFlags, backup::Backup as SqliteBackup};
use rustix::{
    fs::{
        AtFlags, FileType, FlockOperation, Mode, OFlags, RenameFlags, ResolveFlags, flock, fstat,
        mkdirat, open, openat2, renameat_with, statat,
    },
    io::Errno,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    BackupManifest, ExternalRequirement, Product, ResourceEntry, ResourceKind, SchemaIdentity,
    manifest::MANIFEST_VERSION,
};

mod restore;
pub use restore::{
    RecoveryAction, RecoveryResult, RestoreExisting, RestoreResult, recover_sqlite_restore,
    restore_sqlite_backup, restore_sqlite_backup_with_credentials,
};

const DATABASE_FILE: &str = "database.sqlite3";
const MANIFEST_FILE: &str = "manifest.json";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_CREDENTIAL_KEY_BYTES: u64 = 4096;
#[cfg(test)]
pub(super) const PRODUCT_METADATA_DDL: &str = "CREATE TABLE product_metadata (\n\
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),\n\
    application TEXT NOT NULL,\n\
    application_version TEXT NOT NULL,\n\
    schema_revision INTEGER NOT NULL,\n\
    schema_sha256 TEXT NOT NULL\n\
)";
pub(super) const HOST_CURRENT_APPLICATION_VERSION: &str = "0.7.0";
pub(super) const HOST_CURRENT_SCHEMA_REVISION: u64 = 1;
pub(super) const HOST_CURRENT_SCHEMA_SHA256: &str =
    "2f63778e94b345d100c10f8b45b98f06e39590547f6b1d65f9b5b0e7f6989328";
pub(super) const SUNSHINE_CURRENT_APPLICATION_VERSION: &str = "0.7.0";
pub(super) const SUNSHINE_CURRENT_SCHEMA_REVISION: u64 = 1;
pub(super) const SUNSHINE_CURRENT_SCHEMA_SHA256: &str =
    "a8e2fe3c3a9a59a9a36979bcef3628299832d02078e421953ab78e0c0900d5a7";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OfficialSqliteIdentity {
    pub product: Product,
    pub application_version: &'static str,
    pub schema_revision: u64,
    pub schema_sha256: &'static str,
}

pub(super) fn official_sqlite_identity(product: Product) -> anyhow::Result<OfficialSqliteIdentity> {
    require_sqlite_only_product(product)?;
    let identity = match product {
        Product::HostMonitoring => OfficialSqliteIdentity {
            product,
            application_version: HOST_CURRENT_APPLICATION_VERSION,
            schema_revision: HOST_CURRENT_SCHEMA_REVISION,
            schema_sha256: HOST_CURRENT_SCHEMA_SHA256,
        },
        Product::SunshineManager => OfficialSqliteIdentity {
            product,
            application_version: SUNSHINE_CURRENT_APPLICATION_VERSION,
            schema_revision: SUNSHINE_CURRENT_SCHEMA_REVISION,
            schema_sha256: SUNSHINE_CURRENT_SCHEMA_SHA256,
        },
        _ => anyhow::bail!("no official generic current SQLite identity for {product}"),
    };
    Ok(identity)
}

pub(super) fn verify_official_sqlite_identity(
    product: Product,
    identity: &SchemaIdentity,
) -> anyhow::Result<()> {
    let official = official_sqlite_identity(product)?;
    ensure!(
        identity.application == official.product.slug()
            && identity.application_version == official.application_version
            && identity.schema_revision == official.schema_revision
            && identity.schema_sha256 == official.schema_sha256,
        "SQLite identity is not the exact official current contract for {product}"
    );
    Ok(())
}

#[derive(Clone, Debug)]
pub struct VerifiedSqliteBackup {
    pub directory: PathBuf,
    pub manifest: BackupManifest,
}

/// Create one immutable, checksummed SQLite backup set.
///
/// This generic path intentionally accepts only the exact current metadata
/// contract. Databases from older versions require an explicit product adapter.
pub fn create_sqlite_backup(
    product: Product,
    database: &Path,
    output: &Path,
) -> anyhow::Result<VerifiedSqliteBackup> {
    ensure!(
        !product.contract().requires_external_credentials_key,
        "{product} backup requires an explicit external credentials key"
    );
    create_sqlite_backup_internal(product, database, output, None)
}

pub fn create_sqlite_backup_with_credentials(
    product: Product,
    database: &Path,
    output: &Path,
    key_id: &str,
    credentials_key: &[u8; 32],
) -> anyhow::Result<VerifiedSqliteBackup> {
    ensure!(
        product == Product::SunshineManager,
        "keyed SQLite backup is only defined for sunshine-manager"
    );
    validate_key_id(key_id)?;
    create_sqlite_backup_internal(product, database, output, Some((key_id, credentials_key)))
}

fn create_sqlite_backup_internal(
    product: Product,
    database: &Path,
    output: &Path,
    credentials: Option<(&str, &[u8; 32])>,
) -> anyhow::Result<VerifiedSqliteBackup> {
    require_sqlite_only_product(product)?;
    let maintenance = MaintenanceLock::shared(product, database)?;
    let source_path = maintenance.database_path();
    let source_identity = verify_official_sqlite_database(&source_path, product)
        .context("verify current source database before backup")?;
    if let Some((key_id, key)) = credentials {
        verify_sunshine_encrypted_values(&source_path, key_id, key)?;
    }

    let mut pending = PendingDirectory::create(output)?;
    let pending_path = pending.path();
    let database_output = pending_path.join(DATABASE_FILE);
    copy_database_online(&source_path, &database_output)?;
    let snapshot_identity = verify_official_sqlite_database(&database_output, product)
        .context("verify the SQLite snapshot")?;
    ensure!(
        snapshot_identity == source_identity,
        "database identity changed while the backup was created"
    );
    if let Some((key_id, key)) = credentials {
        verify_sunshine_encrypted_values(&database_output, key_id, key)?;
    }

    let (bytes, sha256) = hash_regular_file(&database_output)?;
    let manifest = BackupManifest {
        manifest_version: MANIFEST_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        product,
        application_version: snapshot_identity.application_version.clone(),
        schema_identity: Some(snapshot_identity),
        created_at_epoch_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_secs(),
        external_requirements: credentials
            .map(|(key_id, key)| sunshine_external_requirement(key_id, key))
            .into_iter()
            .collect(),
        resources: vec![ResourceEntry {
            name: "database".to_owned(),
            kind: ResourceKind::Sqlite,
            path: DATABASE_FILE.into(),
            bytes,
            files: 1,
            sha256,
        }],
    };
    manifest.validate()?;
    write_manifest(&pending_path.join(MANIFEST_FILE), &manifest)?;
    sync_directory(&pending_path)?;
    pending.commit()?;
    drop(maintenance);

    match credentials {
        Some((key_id, key)) => verify_sqlite_backup_with_credentials(product, output, key_id, key),
        None => verify_sqlite_backup(product, output),
    }
}

/// Verify the full backup set against the explicitly selected official product
/// identity without changing either it or a product database.
pub fn verify_sqlite_backup(
    product: Product,
    input: &Path,
) -> anyhow::Result<VerifiedSqliteBackup> {
    ensure!(
        !product.contract().requires_external_credentials_key,
        "{product} verification requires an explicit external credentials key"
    );
    verify_sqlite_backup_internal(product, input, None)
}

pub fn verify_sqlite_backup_with_credentials(
    product: Product,
    input: &Path,
    key_id: &str,
    credentials_key: &[u8; 32],
) -> anyhow::Result<VerifiedSqliteBackup> {
    ensure!(
        product == Product::SunshineManager,
        "keyed SQLite verification is only defined for sunshine-manager"
    );
    validate_key_id(key_id)?;
    verify_sqlite_backup_internal(product, input, Some((key_id, credentials_key)))
}

fn verify_sqlite_backup_internal(
    product: Product,
    input: &Path,
    credentials: Option<(&str, &[u8; 32])>,
) -> anyhow::Result<VerifiedSqliteBackup> {
    let official = official_sqlite_identity(product)?;
    let directory = SecureDirectory::open(input, "backup directory")?;
    let entries = directory.entry_names()?;
    ensure!(
        entries == vec![DATABASE_FILE.to_owned(), MANIFEST_FILE.to_owned()],
        "SQLite backup directory must contain exactly database.sqlite3 and manifest.json"
    );

    let manifest_bytes = directory.read_bounded(MANIFEST_FILE, MAX_MANIFEST_BYTES)?;
    let manifest = BackupManifest::from_slice(&manifest_bytes)?;
    ensure!(
        manifest.product == product,
        "SQLite backup product does not match the explicit product"
    );
    match credentials {
        Some((key_id, key)) => ensure!(
            manifest.external_requirements == [sunshine_external_requirement(key_id, key)],
            "external credentials key does not match the backup manifest"
        ),
        None => ensure!(
            manifest.external_requirements.is_empty(),
            "backup unexpectedly requires an external key"
        ),
    }
    ensure!(
        manifest.application_version == official.application_version,
        "SQLite backup version is not the official current version for {product}"
    );
    ensure!(
        manifest.resources.len() == 1,
        "SQLite-only backup must declare exactly one resource"
    );
    let resource = &manifest.resources[0];
    ensure!(
        resource.name == "database"
            && resource.kind == ResourceKind::Sqlite
            && resource.path == Path::new(DATABASE_FILE)
            && resource.files == 1,
        "SQLite backup database resource contract is invalid"
    );

    let database_path = directory.child_path(DATABASE_FILE);
    let (bytes, sha256) = hash_regular_file(&database_path)?;
    ensure!(
        bytes == resource.bytes,
        "SQLite backup size does not match its manifest"
    );
    ensure!(
        sha256 == resource.sha256,
        "SQLite backup checksum does not match its manifest"
    );
    let identity = verify_official_sqlite_database(&database_path, product)?;
    if let Some((key_id, key)) = credentials {
        verify_sunshine_encrypted_values(&database_path, key_id, key)?;
    }
    ensure!(
        manifest.schema_identity.as_ref() == Some(&identity),
        "SQLite backup schema identity does not match its manifest"
    );
    ensure!(
        manifest.application_version == identity.application_version,
        "SQLite backup application version does not match its manifest"
    );

    Ok(VerifiedSqliteBackup {
        directory: absolute_path(input)?,
        manifest,
    })
}

fn sunshine_external_requirement(key_id: &str, key: &[u8; 32]) -> ExternalRequirement {
    ExternalRequirement {
        kind: "credentials-key".to_owned(),
        kid: key_id.to_owned(),
        sha256: lower_hex(&Sha256::digest(key)),
        algorithm: "aes-256-gcm".to_owned(),
        envelope_version: 1,
    }
}

fn validate_key_id(value: &str) -> anyhow::Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "credentials key id is invalid"
    );
    Ok(())
}

/// Read a private base64-encoded 32-byte key without following a final symlink.
///
/// This helper is product-neutral and remains part of the reusable backup layer;
/// it does not recognize any historical database generation.
pub fn credentials_key_from_file(path: &Path) -> anyhow::Result<[u8; 32]> {
    let named_before = fs::symlink_metadata(path).context("inspect credentials key file")?;
    ensure!(
        named_before.is_file()
            && named_before.nlink() == 1
            && named_before.mode() & 0o077 == 0
            && named_before.len() <= MAX_CREDENTIAL_KEY_BYTES,
        "credentials key must be a private, single-link, bounded regular file"
    );
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .context("open credentials key file")?;
    let mut file = File::from(descriptor);
    let opened_before = file.metadata()?;
    ensure!(
        opened_before.dev() == named_before.dev()
            && opened_before.ino() == named_before.ino()
            && opened_before.nlink() == 1
            && opened_before.mode() & 0o077 == 0,
        "credentials key path changed while it was opened"
    );
    let mut bytes = Vec::with_capacity(opened_before.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_CREDENTIAL_KEY_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let opened_after = file.metadata()?;
    let named_after = fs::symlink_metadata(path)?;
    ensure!(
        bytes.len() as u64 == opened_before.len()
            && opened_after.dev() == opened_before.dev()
            && opened_after.ino() == opened_before.ino()
            && opened_after.len() == opened_before.len()
            && opened_after.mtime() == opened_before.mtime()
            && opened_after.mtime_nsec() == opened_before.mtime_nsec()
            && named_after.dev() == opened_after.dev()
            && named_after.ino() == opened_after.ino()
            && named_after.mode() & 0o077 == 0,
        "credentials key file changed while it was read"
    );
    let encoded = String::from_utf8(bytes).context("credentials key file is not UTF-8")?;
    STANDARD
        .decode(encoded.trim())
        .context("credentials key file must contain base64")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("credentials key must decode to exactly 32 bytes"))
}

pub(super) fn verify_sunshine_encrypted_values(
    database: &Path,
    key_id: &str,
    key: &[u8; 32],
) -> anyhow::Result<()> {
    let connection = open_read_only(database)?;
    let cipher = Aes256Gcm::new_from_slice(key).expect("32-byte Sunshine key");
    for (table, column, require_json) in [
        ("hosts", "secret", false),
        ("operations", "request_ciphertext", true),
    ] {
        let sql = format!("SELECT {column} FROM {table} WHERE {column} IS NOT NULL ORDER BY rowid");
        let mut statement = connection.prepare(&sql)?;
        let values = statement.query_map([], |row| row.get::<_, String>(0))?;
        for value in values {
            let value = value?;
            let prefix = format!("sunshine:v1:{key_id}:");
            let encoded = value
                .strip_prefix(&prefix)
                .context("Sunshine ciphertext key id or envelope version mismatch")?;
            let payload = STANDARD
                .decode(encoded)
                .context("Sunshine ciphertext is not canonical base64")?;
            ensure!(payload.len() > 12, "Sunshine ciphertext is truncated");
            let plaintext = cipher
                .decrypt(Nonce::from_slice(&payload[..12]), &payload[12..])
                .map_err(|_| {
                    anyhow::anyhow!("credentials key cannot authenticate Sunshine encrypted state")
                })?;
            if require_json {
                serde_json::from_slice::<serde_json::Value>(&plaintext)
                    .context("Sunshine operation ciphertext does not contain JSON")?;
            } else {
                String::from_utf8(plaintext).context("Sunshine host credential is not UTF-8")?;
            }
        }
    }
    Ok(())
}

fn verify_official_sqlite_database(
    database: &Path,
    product: Product,
) -> anyhow::Result<SchemaIdentity> {
    let identity = verify_current_database(database, product)?;
    verify_official_sqlite_identity(product, &identity)?;
    Ok(identity)
}

/// Calculate the canonical current-schema fingerprint used by every product.
pub fn schema_fingerprint(database: &Path) -> anyhow::Result<String> {
    let location = DatabaseLocation::resolve(database)?;
    let connection = open_read_only(&location.database_path())?;
    schema_fingerprint_connection(&connection)
}

fn verify_current_database(database: &Path, product: Product) -> anyhow::Result<SchemaIdentity> {
    require_regular_file(database, "SQLite database")?;
    let connection = open_read_only(database)?;
    connection
        .pragma_update(None, "trusted_schema", "OFF")
        .context("disable trusted SQLite schema")?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .context("configure SQLite verification timeout")?;

    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .context("run SQLite integrity check")?;
    ensure!(
        integrity.eq_ignore_ascii_case("ok"),
        "SQLite integrity check failed"
    );
    let mut foreign_keys = connection
        .prepare("PRAGMA foreign_key_check")
        .context("prepare SQLite foreign-key check")?;
    ensure!(
        foreign_keys.query([])?.next()?.is_none(),
        "SQLite foreign-key check failed"
    );

    verify_metadata_table_shape(&connection)?;
    let row_count: i64 =
        connection.query_row("SELECT COUNT(*) FROM product_metadata", [], |row| {
            row.get(0)
        })?;
    ensure!(
        row_count == 1,
        "product_metadata must contain exactly one row"
    );
    let (singleton, application, application_version, schema_revision, schema_sha256): (
        i64,
        String,
        String,
        i64,
        String,
    ) = connection.query_row(
        "SELECT singleton, application, application_version, schema_revision, schema_sha256 \
         FROM product_metadata",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    ensure!(singleton == 1, "product_metadata singleton must equal 1");
    let schema_revision = u64::try_from(schema_revision)
        .context("product_metadata schema_revision must not be negative")?;
    let identity = SchemaIdentity {
        application,
        application_version,
        schema_revision,
        schema_sha256,
    };
    identity.validate(product)?;

    let migration_table_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='_sqlx_migrations'",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        migration_table_count == 0,
        "current-only databases must not contain the SQLx migration ledger"
    );
    let actual = schema_fingerprint_connection(&connection)?;
    ensure!(
        actual == identity.schema_sha256,
        "actual SQLite schema fingerprint does not match product_metadata"
    );
    Ok(identity)
}

fn verify_metadata_table_shape(connection: &Connection) -> anyhow::Result<()> {
    let mut statement = connection.prepare(
        "SELECT cid, name, type, \"notnull\", COALESCE(dflt_value, ''), pk \
         FROM pragma_table_info('product_metadata') ORDER BY cid",
    )?;
    let columns = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let expected = [
        (0, "singleton", "INTEGER", 1, "", 1),
        (1, "application", "TEXT", 1, "", 0),
        (2, "application_version", "TEXT", 1, "", 0),
        (3, "schema_revision", "INTEGER", 1, "", 0),
        (4, "schema_sha256", "TEXT", 1, "", 0),
    ];
    ensure!(
        columns.len() == expected.len(),
        "product_metadata has an unexpected shape"
    );
    for (actual, expected) in columns.iter().zip(expected) {
        ensure!(
            actual.0 == expected.0
                && actual.1 == expected.1
                && actual.2.eq_ignore_ascii_case(expected.2)
                && actual.3 == expected.3
                && actual.4 == expected.4
                && actual.5 == expected.5,
            "product_metadata has an unexpected column definition"
        );
    }
    Ok(())
}

fn schema_fingerprint_connection(connection: &Connection) -> anyhow::Result<String> {
    let mut statement = connection.prepare(
        "SELECT type, name, tbl_name, COALESCE(sql, '') FROM sqlite_schema \
         WHERE name NOT GLOB 'sqlite_*' AND name <> 'product_metadata' \
         ORDER BY type, name, tbl_name",
    )?;
    let rows = statement.query_map([], |row| {
        Ok([
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ])
    })?;
    let mut digest = Sha256::new();
    for row in rows {
        for field in row? {
            let bytes = field.as_bytes();
            digest.update((bytes.len() as u64).to_be_bytes());
            digest.update(bytes);
        }
    }
    Ok(lower_hex(&digest.finalize()))
}

fn copy_database_online(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let source = open_read_only(source)?;
    let mut destination_connection = Connection::open_with_flags(
        destination,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("create SQLite snapshot")?;
    destination_connection.busy_timeout(Duration::from_secs(3))?;
    {
        let backup = SqliteBackup::new(&source, &mut destination_connection)?;
        backup.run_to_completion(128, Duration::from_millis(10), None)?;
    }
    destination_connection.execute_batch("PRAGMA journal_mode=DELETE;")?;
    drop(destination_connection);
    File::open(destination)?.sync_all()?;
    Ok(())
}

fn open_read_only(path: &Path) -> anyhow::Result<Connection> {
    Connection::open_with_flags(
        path,
        // The path is rooted through a previously validated, open directory
        // descriptor. SQLite's NOFOLLOW flag rejects that intentional
        // /proc/self/fd magic-link component, so final-entry safety instead
        // comes from openat2 validation and a non-writable parent directory.
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open SQLite database {} read-only", path.display()))
}

fn require_sqlite_only_product(product: Product) -> anyhow::Result<()> {
    let contract = product.contract();
    ensure!(
        contract.resources == [ResourceKind::Sqlite],
        "{product} requires a composite product adapter, not the SQLite-only command"
    );
    Ok(())
}

fn write_manifest(path: &Path, manifest: &BackupManifest) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create backup manifest {}", path.display()))?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn hash_regular_file(path: &Path) -> anyhow::Result<(u64, String)> {
    let fd = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .with_context(|| format!("open backup resource {}", path.display()))?;
    let metadata = fstat(&fd)?;
    ensure!(
        FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
            && metadata.st_nlink == 1,
        "backup resource must be one regular file"
    );
    let mut file = File::from(fd);
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(read as u64)
            .context("backup resource size overflow")?;
        digest.update(&buffer[..read]);
    }
    ensure!(
        bytes == metadata.st_size as u64,
        "backup resource changed while it was hashed"
    );
    Ok((bytes, lower_hex(&digest.finalize())))
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

fn require_regular_file(path: &Path, label: &str) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("{label} does not exist: {}", path.display()))?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "{label} must be a regular file"
    );
    use std::os::unix::fs::MetadataExt;
    ensure!(
        metadata.nlink() == 1,
        "{label} must have exactly one hard link"
    );
    Ok(())
}

fn absolute_path(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::from("/");
    for component in absolute.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir => anyhow::bail!("paths must not contain parent traversal"),
            Component::Prefix(_) => anyhow::bail!("unsupported path prefix"),
        }
    }
    Ok(normalized)
}

struct DatabaseLocation {
    parent: File,
    database_name: OsString,
    configured_database: PathBuf,
}

impl DatabaseLocation {
    fn resolve(database: &Path) -> anyhow::Result<Self> {
        Self::resolve_with_requirement(database, true)
    }

    fn resolve_target(database: &Path) -> anyhow::Result<Self> {
        Self::resolve_with_requirement(database, false)
    }

    fn resolve_with_requirement(database: &Path, must_exist: bool) -> anyhow::Result<Self> {
        let database = absolute_path(database)?;
        let parent = SecureDirectory::open(
            database.parent().context("database must have a parent")?,
            "SQLite database parent",
        )?;
        let database_name = database
            .file_name()
            .context("database must name a file")?
            .to_os_string();
        match statat(&parent.file, &database_name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(metadata) => ensure!(
                FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                    && metadata.st_nlink == 1,
                "SQLite database must be one regular file"
            ),
            Err(Errno::NOENT) if !must_exist => {}
            Err(Errno::NOENT) => anyhow::bail!("SQLite database does not exist"),
            Err(error) => {
                return Err(std::io::Error::from(error)).context("inspect SQLite database");
            }
        }
        Ok(Self {
            parent: parent.file,
            database_name,
            configured_database: database,
        })
    }

    fn child_path(&self, name: &OsStr) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.parent.as_raw_fd())).join(name)
    }

    fn database_path(&self) -> PathBuf {
        self.child_path(&self.database_name)
    }

    fn configured_database_path(&self) -> &Path {
        &self.configured_database
    }

    fn acquire_lock(&self, product: Product, operation: FlockOperation) -> anyhow::Result<File> {
        let mut lock_name = OsString::from(".");
        lock_name.push(&self.database_name);
        lock_name.push(".");
        lock_name.push(product.slug());
        lock_name.push(".maintenance.lock");
        let fd = openat2(
            &self.parent,
            &lock_name,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
            secure_resolve_flags(),
        )
        .context("open product maintenance lock")?;
        let metadata = fstat(&fd)?;
        ensure!(
            FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                && metadata.st_nlink == 1
                && metadata.st_mode & 0o077 == 0,
            "maintenance lock must be one private regular file"
        );
        let named = statat(&self.parent, &lock_name, AtFlags::SYMLINK_NOFOLLOW)?;
        ensure!(
            FileType::from_raw_mode(named.st_mode) == FileType::RegularFile
                && named.st_nlink == 1
                && named.st_mode & 0o077 == 0
                && named.st_dev == metadata.st_dev
                && named.st_ino == metadata.st_ino,
            "maintenance lock path changed while it was opened"
        );
        match flock(&fd, operation) {
            Ok(()) => Ok(File::from(fd)),
            Err(Errno::WOULDBLOCK) => {
                anyhow::bail!("product service or exclusive maintenance is active")
            }
            Err(error) => Err(std::io::Error::from(error)).context("acquire maintenance lock"),
        }
    }
}

struct MaintenanceLock {
    location: DatabaseLocation,
    _file: File,
}

impl MaintenanceLock {
    fn shared(product: Product, database: &Path) -> anyhow::Result<Self> {
        let location = DatabaseLocation::resolve(database)?;
        let file = location.acquire_lock(product, FlockOperation::NonBlockingLockShared)?;
        Ok(Self {
            location,
            _file: file,
        })
    }

    fn exclusive(product: Product, database: &Path) -> anyhow::Result<Self> {
        let location = DatabaseLocation::resolve_target(database)?;
        let file = location.acquire_lock(product, FlockOperation::NonBlockingLockExclusive)?;
        Ok(Self {
            location,
            _file: file,
        })
    }

    fn database_path(&self) -> PathBuf {
        self.location.database_path()
    }
}

struct SecureDirectory {
    file: File,
}

impl SecureDirectory {
    fn open(path: &Path, label: &str) -> anyhow::Result<Self> {
        let path = absolute_path(path)?;
        let filesystem_root = open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let relative = path.strip_prefix("/")?;
        let relative = if relative.as_os_str().is_empty() {
            Path::new(".")
        } else {
            relative
        };
        let fd = openat2(
            &filesystem_root,
            relative,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            secure_resolve_flags(),
        )
        .with_context(|| format!("open {label} without following symbolic links"))?;
        let metadata = fstat(&fd)?;
        ensure!(
            FileType::from_raw_mode(metadata.st_mode) == FileType::Directory,
            "{label} must be a directory"
        );
        ensure!(
            metadata.st_mode & 0o022 == 0,
            "{label} must not be writable by group or other users"
        );
        Ok(Self {
            file: File::from(fd),
        })
    }

    fn child_path(&self, child: impl AsRef<Path>) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd())).join(child)
    }

    fn require_regular_child(&self, name: &OsStr, label: &str) -> anyhow::Result<()> {
        let metadata = statat(&self.file, name, AtFlags::SYMLINK_NOFOLLOW)
            .with_context(|| format!("inspect {label}"))?;
        ensure!(
            FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                && metadata.st_nlink == 1,
            "{label} must be one regular file"
        );
        Ok(())
    }

    fn read_bounded(&self, name: &str, limit: u64) -> anyhow::Result<Vec<u8>> {
        self.require_regular_child(OsStr::new(name), name)?;
        let fd = openat2(
            &self.file,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
            secure_resolve_flags(),
        )?;
        let metadata = fstat(&fd)?;
        ensure!(
            metadata.st_size >= 0 && metadata.st_size as u64 <= limit,
            "{name} is too large"
        );
        let mut bytes = Vec::with_capacity(metadata.st_size as usize);
        File::from(fd).take(limit + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= limit, "{name} is too large");
        Ok(bytes)
    }

    fn entry_names(&self) -> anyhow::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(self.child_path("."))? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("backup entry name is not UTF-8"))?;
            names.push(name);
        }
        names.sort();
        Ok(names)
    }
}

struct PendingDirectory {
    parent: SecureDirectory,
    pending_name: OsString,
    output_name: OsString,
    committed: bool,
}

impl PendingDirectory {
    fn create(output: &Path) -> anyhow::Result<Self> {
        let output = absolute_path(output)?;
        let parent_path = output
            .parent()
            .context("backup output must have a parent")?;
        let parent = SecureDirectory::open(parent_path, "backup output parent")?;
        let output_name = output
            .file_name()
            .context("backup output must name a directory")?
            .to_os_string();
        match statat(&parent.file, &output_name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => {}
            Ok(_) => anyhow::bail!("backup output already exists"),
            Err(error) => return Err(std::io::Error::from(error)).context("inspect backup output"),
        }

        let mut pending_name = OsString::from(".");
        pending_name.push(&output_name);
        pending_name.push(format!(".pending-{}", Uuid::new_v4()));
        mkdirat(&parent.file, &pending_name, Mode::from_raw_mode(0o700))
            .context("create pending backup directory")?;
        parent.file.sync_all()?;
        Ok(Self {
            parent,
            pending_name,
            output_name,
            committed: false,
        })
    }

    fn path(&self) -> PathBuf {
        self.parent.child_path(&self.pending_name)
    }

    fn commit(&mut self) -> anyhow::Result<()> {
        renameat_with(
            &self.parent.file,
            &self.pending_name,
            &self.parent.file,
            &self.output_name,
            RenameFlags::NOREPLACE,
        )
        .context("publish completed backup without overwriting")?;
        self.parent.file.sync_all()?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for PendingDirectory {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(self.path());
            let _ = self.parent.file.sync_all();
        }
    }
}

fn sync_directory(path: &Path) -> anyhow::Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn secure_resolve_flags() -> ResolveFlags {
    ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS | ResolveFlags::NO_SYMLINKS
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST_CURRENT_SCHEMA_SQL: &str =
        include_str!("../tests/fixtures/current/host-monitoring.sql");
    const SUNSHINE_CURRENT_SCHEMA_SQL: &str =
        include_str!("../tests/fixtures/current/sunshine-manager.sql");

    pub(super) fn create_current_database(path: &Path, product: Product, version: &str) {
        let official = official_sqlite_identity(product).unwrap();
        let schema = match product {
            Product::HostMonitoring => HOST_CURRENT_SCHEMA_SQL,
            Product::SunshineManager => SUNSHINE_CURRENT_SCHEMA_SQL,
            _ => panic!("test fixture supports only generic SQLite products"),
        };
        let connection = Connection::open(path).unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        connection.execute_batch(schema).unwrap();
        connection.execute_batch(PRODUCT_METADATA_DDL).unwrap();
        let fingerprint = schema_fingerprint_connection(&connection).unwrap();
        assert_eq!(fingerprint, official.schema_sha256);
        connection
            .execute(
                "INSERT INTO product_metadata (
                   singleton, application, application_version, schema_revision, schema_sha256
                 ) VALUES (1, ?1, ?2, ?3, ?4)",
                (
                    product.slug(),
                    version,
                    i64::try_from(official.schema_revision).unwrap(),
                    fingerprint,
                ),
            )
            .unwrap();
    }

    pub(super) fn insert_test_record(path: &Path, product: Product, value: &str) {
        let connection = Connection::open(path).unwrap();
        match product {
            Product::HostMonitoring => {
                connection
                    .execute(
                        "INSERT INTO audit_events (action,target,actor,created_at)
                         VALUES (?1,'fixture','test','2026-01-01T00:00:00Z')",
                        [value],
                    )
                    .unwrap();
            }
            Product::SunshineManager => {
                connection
                    .execute(
                        "INSERT INTO audit_logs (action,target,actor,created_at_micros)
                         VALUES (?1,'fixture','test',1)",
                        [value],
                    )
                    .unwrap();
            }
            _ => panic!("test fixture supports only generic SQLite products"),
        }
    }

    pub(super) fn test_record_count(path: &Path, product: Product) -> i64 {
        let table = match product {
            Product::HostMonitoring => "audit_events",
            Product::SunshineManager => "audit_logs",
            _ => panic!("test fixture supports only generic SQLite products"),
        };
        Connection::open(path)
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn assert_backup_rejected_without_database_change(
        database: &Path,
        product: Product,
        output: &Path,
    ) {
        let before = fs::read(database).unwrap();
        assert!(create_sqlite_backup(product, database, output).is_err());
        assert_eq!(fs::read(database).unwrap(), before);
        assert!(!output.exists());
    }

    #[test]
    fn creates_and_verifies_immutable_sqlite_backup() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("source.sqlite3");
        let output = root.path().join("backup");
        create_current_database(&database, Product::HostMonitoring, "0.7.0");

        let backup = create_sqlite_backup(Product::HostMonitoring, &database, &output).unwrap();
        assert_eq!(backup.manifest.application_version, "0.7.0");
        verify_sqlite_backup(Product::HostMonitoring, &output).unwrap();

        insert_test_record(&database, Product::HostMonitoring, "later");
        assert_eq!(
            test_record_count(&output.join(DATABASE_FILE), Product::HostMonitoring),
            0
        );
    }

    #[test]
    fn official_allowlist_accepts_exact_host_identity() {
        let product = Product::HostMonitoring;
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("current.sqlite3");
        let output = root.path().join("backup");
        let official = official_sqlite_identity(product).unwrap();
        create_current_database(&database, product, official.application_version);

        let created = create_sqlite_backup(product, &database, &output).unwrap();
        let identity = created.manifest.schema_identity.as_ref().unwrap();
        assert_eq!(identity.application, product.slug());
        assert_eq!(identity.application_version, official.application_version);
        assert_eq!(identity.schema_revision, official.schema_revision);
        assert_eq!(identity.schema_sha256, official.schema_sha256);
        verify_sqlite_backup(product, &output).unwrap();
    }

    #[test]
    fn rejects_wrong_version_revision_and_self_consistent_unknown_schema() {
        let root = tempfile::tempdir().unwrap();

        let wrong_version = root.path().join("wrong-version.sqlite3");
        create_current_database(&wrong_version, Product::HostMonitoring, "0.7.1");
        assert_backup_rejected_without_database_change(
            &wrong_version,
            Product::HostMonitoring,
            &root.path().join("wrong-version-backup"),
        );

        let wrong_revision = root.path().join("wrong-revision.sqlite3");
        create_current_database(&wrong_revision, Product::HostMonitoring, "0.7.0");
        Connection::open(&wrong_revision)
            .unwrap()
            .execute("UPDATE product_metadata SET schema_revision=2", [])
            .unwrap();
        assert_backup_rejected_without_database_change(
            &wrong_revision,
            Product::HostMonitoring,
            &root.path().join("wrong-revision-backup"),
        );

        let unknown_schema = root.path().join("unknown-schema.sqlite3");
        create_current_database(&unknown_schema, Product::HostMonitoring, "0.7.0");
        let connection = Connection::open(&unknown_schema).unwrap();
        connection
            .execute_batch("CREATE TABLE internally_consistent_unknown (id INTEGER PRIMARY KEY);")
            .unwrap();
        let unknown_sha = schema_fingerprint_connection(&connection).unwrap();
        assert_ne!(unknown_sha, HOST_CURRENT_SCHEMA_SHA256);
        connection
            .execute(
                "UPDATE product_metadata SET schema_sha256=?1",
                [&unknown_sha],
            )
            .unwrap();
        drop(connection);
        assert_backup_rejected_without_database_change(
            &unknown_schema,
            Product::HostMonitoring,
            &root.path().join("unknown-schema-backup"),
        );
    }

    #[test]
    fn verify_rejects_tampered_manifest_and_cross_product_selection() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("host.sqlite3");
        let backup = root.path().join("backup");
        create_current_database(&database, Product::HostMonitoring, "0.7.0");
        create_sqlite_backup(Product::HostMonitoring, &database, &backup).unwrap();

        assert!(verify_sqlite_backup(Product::SunshineManager, &backup).is_err());
        let manifest_path = backup.join(MANIFEST_FILE);
        let original = fs::read(&manifest_path).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
        manifest["schema_identity"]["schema_revision"] = serde_json::json!(2);
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert!(verify_sqlite_backup(Product::HostMonitoring, &backup).is_err());

        let mut cross_product: serde_json::Value = serde_json::from_slice(&original).unwrap();
        cross_product["product"] = serde_json::json!("sunshine-manager");
        cross_product["application_version"] = serde_json::json!("0.7.0");
        cross_product["schema_identity"]["application"] = serde_json::json!("sunshine-manager");
        cross_product["schema_identity"]["schema_sha256"] =
            serde_json::json!(SUNSHINE_CURRENT_SCHEMA_SHA256);
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&cross_product).unwrap(),
        )
        .unwrap();
        assert!(verify_sqlite_backup(Product::HostMonitoring, &backup).is_err());
        assert!(verify_sqlite_backup(Product::SunshineManager, &backup).is_err());
    }

    #[test]
    fn refuses_legacy_database_without_modifying_output() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("legacy.sqlite3");
        Connection::open(&database)
            .unwrap()
            .execute_batch("CREATE TABLE legacy (id INTEGER PRIMARY KEY);")
            .unwrap();
        let output = root.path().join("backup");
        assert!(create_sqlite_backup(Product::HostMonitoring, &database, &output).is_err());
        assert!(!output.exists());
    }

    #[test]
    fn never_overwrites_an_existing_output() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("source.sqlite3");
        let output = root.path().join("backup");
        create_current_database(&database, Product::SunshineManager, "0.7.0");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("keep"), b"unchanged").unwrap();
        assert!(create_sqlite_backup(Product::SunshineManager, &database, &output).is_err());
        assert_eq!(fs::read(output.join("keep")).unwrap(), b"unchanged");
    }

    #[test]
    fn detects_resource_corruption_and_extra_entries() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("source.sqlite3");
        let first = root.path().join("first");
        let second = root.path().join("second");
        create_current_database(&database, Product::HostMonitoring, "0.7.0");
        create_sqlite_backup(Product::HostMonitoring, &database, &first).unwrap();
        fs::write(first.join("unexpected"), b"no").unwrap();
        assert!(verify_sqlite_backup(Product::HostMonitoring, &first).is_err());

        create_sqlite_backup(Product::HostMonitoring, &database, &second).unwrap();
        OpenOptions::new()
            .append(true)
            .open(second.join(DATABASE_FILE))
            .unwrap()
            .write_all(b"corruption")
            .unwrap();
        assert!(verify_sqlite_backup(Product::HostMonitoring, &second).is_err());
    }

    #[test]
    fn rejects_symlinked_parent_and_hard_linked_database() {
        use std::fs::hard_link;
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        fs::create_dir(&real).unwrap();
        let database = real.join("source.sqlite3");
        create_current_database(&database, Product::HostMonitoring, "0.7.0");
        hard_link(&database, real.join("alias.sqlite3")).unwrap();
        assert!(
            create_sqlite_backup(
                Product::HostMonitoring,
                &database,
                &root.path().join("backup")
            )
            .is_err()
        );

        fs::remove_file(real.join("alias.sqlite3")).unwrap();
        symlink(&real, root.path().join("linked")).unwrap();
        assert!(
            create_sqlite_backup(
                Product::HostMonitoring,
                &root.path().join("linked/source.sqlite3"),
                &root.path().join("backup")
            )
            .is_err()
        );
    }

    #[test]
    fn refuses_a_database_with_schema_drift_without_creating_output() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("source.sqlite3");
        let output = root.path().join("backup");
        create_current_database(&database, Product::HostMonitoring, "0.7.0");
        Connection::open(&database)
            .unwrap()
            .execute_batch("CREATE TABLE schema_drift (id INTEGER PRIMARY KEY);")
            .unwrap();

        assert!(create_sqlite_backup(Product::HostMonitoring, &database, &output).is_err());
        assert!(!output.exists());
    }

    #[test]
    fn product_exclusive_maintenance_lock_blocks_online_backup() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("source.sqlite3");
        let output = root.path().join("backup");
        create_current_database(&database, Product::HostMonitoring, "0.7.0");
        let location = DatabaseLocation::resolve(&database).unwrap();
        let exclusive = location
            .acquire_lock(
                Product::HostMonitoring,
                FlockOperation::NonBlockingLockExclusive,
            )
            .unwrap();

        assert!(create_sqlite_backup(Product::HostMonitoring, &database, &output).is_err());
        assert!(!output.exists());
        drop(exclusive);
        create_sqlite_backup(Product::HostMonitoring, &database, &output).unwrap();
    }
}

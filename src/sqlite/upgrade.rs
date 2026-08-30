use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::fd::AsRawFd,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, ensure};
use rusqlite::{Connection, OpenFlags};
use rustix::{
    fs::{AtFlags, FileType, Mode, OFlags, fstat, openat2, statat},
    io::Errno,
};
use serde::Serialize;
use sha2::{Digest, Sha384};

use super::{
    DATABASE_FILE, MANIFEST_FILE, MAX_MANIFEST_BYTES, MaintenanceLock, PRODUCT_METADATA_DDL,
    PendingDirectory, SecureDirectory, copy_database_online, hash_regular_file, open_read_only,
    restore::{RestorePoint, recover_sqlite_restore_under_lock, replace_with_staged_database},
    schema_fingerprint_connection, secure_resolve_flags, sync_directory, verify_current_database,
    write_manifest,
};
use crate::{
    BackupManifest, Product, ResourceEntry, ResourceKind, SchemaIdentity,
    manifest::MANIFEST_VERSION,
};

mod host_0_6_to_0_7;
mod sentinel_0_1_to_0_2;
mod sunshine_0_6_to_0_7;

pub use sentinel_0_1_to_0_2::{
    SentinelCompanionContract, SentinelRecordingArchive, SentinelRecoveryOptions,
    SentinelSourceBackupManifest, SentinelStoredFile, SentinelUpgradeOptions,
    SentinelUpgradeResult, VerifiedSentinelSourceBackup, recover_sentinel_upgrade,
    sentinel_credentials_key_from_file, upgrade_sentinel, verify_sentinel_source_backup,
};

const SQLITE_SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];

#[derive(Clone, Debug)]
pub struct VerifiedSourceBackup {
    pub directory: PathBuf,
    pub manifest: BackupManifest,
}

#[derive(Clone, Debug, Serialize)]
pub struct SqliteUpgradeResult {
    pub product: Product,
    pub from_version: String,
    pub to_version: String,
    pub source_backup: PathBuf,
    pub database: PathBuf,
    pub schema_identity: SchemaIdentity,
}

#[derive(Clone, Copy)]
struct Adapter {
    product: Product,
    from_version: &'static str,
    to_version: &'static str,
    source_revision: u64,
    source_schema_sha256: &'static str,
    target_revision: u64,
    target_schema_sha256: &'static str,
    target_schema_sql: &'static str,
    verify_ledger: fn(&Connection) -> anyhow::Result<()>,
    copy_rows: fn(&Connection) -> anyhow::Result<()>,
}

impl Adapter {
    fn resolve(product: Product, from_version: &str, to_version: &str) -> anyhow::Result<Self> {
        let adapter = match (product, from_version, to_version) {
            (Product::HostMonitoring, "0.6.0", "0.7.0") => host_0_6_to_0_7::ADAPTER,
            (Product::SunshineManager, "0.6.0", "0.7.0") => sunshine_0_6_to_0_7::ADAPTER,
            _ => anyhow::bail!(
                "no exact SQLite adapter for {product} {from_version} -> {to_version}"
            ),
        };
        ensure!(
            adapter.product == product,
            "internal adapter product mismatch"
        );
        Ok(adapter)
    }
}

/// Upgrade one exact, explicitly selected SQLite generation.
///
/// There is deliberately no version discovery and no migration-chain search.
/// The source is first cloned without opening SQLite, then backed up and
/// verified before a new target database is built beside it. Only the final
/// journaled switch mutates the product generation.
pub fn upgrade_sqlite(
    product: Product,
    from_version: &str,
    to_version: &str,
    database: &Path,
    backup_output: &Path,
) -> anyhow::Result<SqliteUpgradeResult> {
    upgrade_sqlite_with_hook(
        product,
        from_version,
        to_version,
        database,
        backup_output,
        |_| Ok(()),
    )
}

fn upgrade_sqlite_with_hook(
    product: Product,
    from_version: &str,
    to_version: &str,
    database: &Path,
    backup_output: &Path,
    hook: impl FnMut(RestorePoint) -> anyhow::Result<()>,
) -> anyhow::Result<SqliteUpgradeResult> {
    let adapter = Adapter::resolve(product, from_version, to_version)?;
    let maintenance = MaintenanceLock::exclusive(product, database)?;
    let source_clone = SourceClone::create(&maintenance, product)?;
    let source_identity = verify_source_database(&source_clone.database(), adapter)
        .context("verify the exact cloned source contract")?;

    let source_backup = create_source_backup(
        adapter,
        &source_clone.database(),
        &source_identity,
        backup_output,
    )?;
    let source_backup =
        verify_source_backup(product, from_version, to_version, &source_backup.directory)?;

    let staging = TargetStaging::create(&maintenance, product)?;
    let target_identity = create_target_database(
        adapter,
        &source_backup.directory.join(DATABASE_FILE),
        &staging.database(),
    )?;
    source_clone
        .ensure_source_unchanged()
        .context("source generation changed before the journaled switch")?;

    replace_with_staged_database(
        &maintenance,
        &staging.directory(),
        product,
        &target_identity,
        hook,
    )?;

    Ok(SqliteUpgradeResult {
        product,
        from_version: from_version.to_owned(),
        to_version: to_version.to_owned(),
        source_backup: source_backup.directory,
        database: maintenance
            .location
            .configured_database_path()
            .to_path_buf(),
        schema_identity: target_identity,
    })
}

/// Verify a backup against one explicit source side of one exact adapter.
pub fn verify_source_backup(
    product: Product,
    from_version: &str,
    to_version: &str,
    input: &Path,
) -> anyhow::Result<VerifiedSourceBackup> {
    let adapter = Adapter::resolve(product, from_version, to_version)?;
    let directory = SecureDirectory::open(input, "source backup directory")?;
    let entries = directory.entry_names()?;
    ensure!(
        entries == vec![DATABASE_FILE.to_owned(), MANIFEST_FILE.to_owned()],
        "source backup directory must contain exactly database.sqlite3 and manifest.json"
    );
    let manifest =
        BackupManifest::from_slice(&directory.read_bounded(MANIFEST_FILE, MAX_MANIFEST_BYTES)?)?;
    ensure!(
        manifest.product == product,
        "source backup product mismatch"
    );
    ensure!(
        manifest.application_version == from_version,
        "source backup application version mismatch"
    );
    ensure!(
        manifest.resources.len() == 1,
        "source backup must declare exactly one resource"
    );
    let resource = &manifest.resources[0];
    ensure!(
        resource.name == "database"
            && resource.kind == ResourceKind::Sqlite
            && resource.path == Path::new(DATABASE_FILE)
            && resource.files == 1,
        "source backup database resource contract is invalid"
    );
    let database = directory.child_path(DATABASE_FILE);
    let (bytes, sha256) = hash_regular_file(&database)?;
    ensure!(bytes == resource.bytes, "source backup size mismatch");
    ensure!(sha256 == resource.sha256, "source backup checksum mismatch");
    let identity = verify_source_database(&database, adapter)?;
    ensure!(
        manifest.schema_identity.as_ref() == Some(&identity),
        "source backup schema identity mismatch"
    );
    Ok(VerifiedSourceBackup {
        directory: super::absolute_path(input)?,
        manifest,
    })
}

fn create_source_backup(
    adapter: Adapter,
    cloned_database: &Path,
    source_identity: &SchemaIdentity,
    output: &Path,
) -> anyhow::Result<VerifiedSourceBackup> {
    let mut pending = PendingDirectory::create(output)?;
    let database_output = pending.path().join(DATABASE_FILE);
    create_private_empty_file(&database_output)?;
    copy_database_online(cloned_database, &database_output)?;
    let snapshot_identity = verify_source_database(&database_output, adapter)
        .context("verify canonical old-generation backup")?;
    ensure!(
        &snapshot_identity == source_identity,
        "source schema identity changed while its backup was created"
    );
    let (bytes, sha256) = hash_regular_file(&database_output)?;
    let manifest = BackupManifest {
        manifest_version: MANIFEST_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        product: adapter.product,
        application_version: adapter.from_version.to_owned(),
        schema_identity: Some(snapshot_identity),
        created_at_epoch_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_secs(),
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
    write_manifest(&pending.path().join(MANIFEST_FILE), &manifest)?;
    sync_directory(&pending.path())?;
    pending.commit()?;
    Ok(VerifiedSourceBackup {
        directory: super::absolute_path(output)?,
        manifest,
    })
}

fn verify_source_database(database: &Path, adapter: Adapter) -> anyhow::Result<SchemaIdentity> {
    super::require_regular_file(database, "source SQLite database")?;
    let connection = open_read_only(database)?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    connection.busy_timeout(Duration::from_secs(3))?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    ensure!(
        integrity.eq_ignore_ascii_case("ok"),
        "source SQLite integrity check failed"
    );
    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    ensure!(
        foreign_keys.query([])?.next()?.is_none(),
        "source SQLite foreign-key check failed"
    );
    let metadata_tables: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='product_metadata'",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        metadata_tables == 0,
        "old source unexpectedly contains product_metadata"
    );
    let user_version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let application_id: i64 =
        connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    ensure!(
        user_version == 0 && application_id == 0,
        "old source has unexpected SQLite header identity"
    );
    (adapter.verify_ledger)(&connection)?;
    let actual = schema_fingerprint_connection(&connection)?;
    ensure!(
        actual == adapter.source_schema_sha256,
        "source SQLite schema is not the exact registered old contract: expected {}, got {actual}",
        adapter.source_schema_sha256
    );
    Ok(SchemaIdentity {
        application: adapter.product.slug().to_owned(),
        application_version: adapter.from_version.to_owned(),
        schema_revision: adapter.source_revision,
        schema_sha256: actual,
    })
}

fn create_target_database(
    adapter: Adapter,
    source_backup: &Path,
    target: &Path,
) -> anyhow::Result<SchemaIdentity> {
    create_private_empty_file(target)?;
    let connection = Connection::open_with_flags(
        target,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(3))?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.execute_batch(adapter.target_schema_sql)?;
    connection.execute_batch(PRODUCT_METADATA_DDL)?;
    let before_metadata = schema_fingerprint_connection(&connection)?;
    ensure!(
        before_metadata == adapter.target_schema_sha256,
        "embedded target schema fingerprint mismatch: expected {}, got {before_metadata}",
        adapter.target_schema_sha256
    );
    connection.execute(
        "INSERT INTO product_metadata (
           singleton, application, application_version, schema_revision, schema_sha256
         ) VALUES (1, ?1, ?2, ?3, ?4)",
        (
            adapter.product.slug(),
            adapter.to_version,
            i64::try_from(adapter.target_revision)?,
            adapter.target_schema_sha256,
        ),
    )?;

    let source_uri = sqlite_read_only_uri(source_backup)?;
    connection.execute("ATTACH DATABASE ?1 AS legacy", [source_uri])?;
    let copy_result = (adapter.copy_rows)(&connection);
    let detach_result = connection.execute_batch("DETACH DATABASE legacy;");
    copy_result?;
    detach_result?;

    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    ensure!(
        integrity.eq_ignore_ascii_case("ok"),
        "target SQLite integrity check failed"
    );
    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    ensure!(
        foreign_keys.query([])?.next()?.is_none(),
        "target SQLite foreign-key check failed"
    );
    drop(foreign_keys);
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(connection);
    File::open(target)?.sync_all()?;
    sync_directory(
        target
            .parent()
            .context("target database must have a parent")?,
    )?;

    let identity = verify_current_database(target, adapter.product)?;
    ensure!(
        identity.application_version == adapter.to_version
            && identity.schema_revision == adapter.target_revision
            && identity.schema_sha256 == adapter.target_schema_sha256,
        "created target identity does not match the exact adapter"
    );
    Ok(identity)
}

fn sqlite_read_only_uri(path: &Path) -> anyhow::Result<String> {
    let value = path.to_str().context("SQLite backup path must be UTF-8")?;
    ensure!(
        !value
            .bytes()
            .any(|byte| matches!(byte, b'?' | b'#' | b'\0')),
        "SQLite backup path contains URI control characters"
    );
    Ok(format!("file:{value}?mode=ro&immutable=1"))
}

fn create_private_empty_file(path: &Path) -> anyhow::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create private SQLite file {}", path.display()))?;
    file.sync_all()?;
    Ok(())
}

struct TargetStaging {
    _pending: PendingDirectory,
    configured_directory: PathBuf,
}

impl TargetStaging {
    fn create(maintenance: &MaintenanceLock, product: Product) -> anyhow::Result<Self> {
        let marker = adjacent_marker(&maintenance.location, product, "upgrade-target")?;
        let pending = PendingDirectory::create(&marker)?;
        let configured_directory = marker
            .parent()
            .context("target staging marker must have a parent")?
            .join(&pending.pending_name);
        Ok(Self {
            _pending: pending,
            configured_directory,
        })
    }

    fn directory(&self) -> PathBuf {
        self.configured_directory.clone()
    }

    fn database(&self) -> PathBuf {
        self.directory().join(DATABASE_FILE)
    }
}

#[derive(Clone)]
struct GenerationDigest {
    source_name: OsString,
    bytes: u64,
    sha256: String,
}

struct SourceClone {
    pending: PendingDirectory,
    source_parent: File,
    digests: Vec<GenerationDigest>,
}

impl SourceClone {
    fn create(maintenance: &MaintenanceLock, product: Product) -> anyhow::Result<Self> {
        let marker = adjacent_marker(&maintenance.location, product, "upgrade-source")?;
        let pending = PendingDirectory::create(&marker)?;
        let destination = open_pending_directory(&pending)?;
        let mut digests = Vec::new();
        for suffix in std::iter::once("").chain(SQLITE_SIDECARS) {
            let mut source_name = maintenance.location.database_name.clone();
            source_name.push(suffix);
            let mut destination_name = OsString::from(DATABASE_FILE);
            destination_name.push(suffix);
            match statat(
                &maintenance.location.parent,
                &source_name,
                AtFlags::SYMLINK_NOFOLLOW,
            ) {
                Err(Errno::NOENT) if suffix.is_empty() => {
                    anyhow::bail!("upgrade source database does not exist")
                }
                Err(Errno::NOENT) => continue,
                Err(error) => {
                    return Err(std::io::Error::from(error))
                        .context("inspect source SQLite generation");
                }
                Ok(metadata) => ensure!(
                    FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                        && metadata.st_nlink == 1,
                    "source SQLite generation entries must be regular files with one hard link"
                ),
            }
            let source_path = maintenance.location.child_path(&source_name);
            let before = hash_regular_file(&source_path)?;
            copy_regular_file_at(
                &maintenance.location.parent,
                &source_name,
                &destination.file,
                &destination_name,
            )?;
            let copied = hash_regular_file(&destination.child_path(&destination_name))?;
            ensure!(copied == before, "source clone differs from source bytes");
            digests.push(GenerationDigest {
                source_name,
                bytes: before.0,
                sha256: before.1,
            });
        }
        destination.file.sync_all()?;
        let clone = Self {
            pending,
            source_parent: maintenance.location.parent.try_clone()?,
            digests,
        };
        clone.ensure_source_unchanged()?;
        Ok(clone)
    }

    fn database(&self) -> PathBuf {
        self.pending.path().join(DATABASE_FILE)
    }

    fn ensure_source_unchanged(&self) -> anyhow::Result<()> {
        for digest in &self.digests {
            let path = PathBuf::from(format!("/proc/self/fd/{}", self.source_parent.as_raw_fd()))
                .join(&digest.source_name);
            let current = hash_regular_file(&path)?;
            ensure!(
                current.0 == digest.bytes && current.1 == digest.sha256,
                "source SQLite generation bytes changed during upgrade preparation"
            );
        }
        for suffix in std::iter::once("").chain(SQLITE_SIDECARS) {
            let main = &self.digests[0].source_name;
            let mut name = if suffix.is_empty() {
                main.clone()
            } else {
                let mut name = main.clone();
                name.push(suffix);
                name
            };
            // The main digest already includes no suffix; avoid appending a
            // sidecar suffix twice when checking the captured name set.
            if suffix.is_empty() {
                name = main.clone();
            }
            let captured = self.digests.iter().any(|digest| digest.source_name == name);
            let present = match statat(&self.source_parent, &name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(_) => true,
                Err(Errno::NOENT) => false,
                Err(error) => return Err(std::io::Error::from(error).into()),
            };
            ensure!(
                present == captured,
                "source SQLite sidecar set changed during upgrade preparation"
            );
        }
        Ok(())
    }
}

fn open_pending_directory(pending: &PendingDirectory) -> anyhow::Result<SecureDirectory> {
    let directory = openat2(
        &pending.parent.file,
        &pending.pending_name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        secure_resolve_flags(),
    )?;
    Ok(SecureDirectory {
        file: File::from(directory),
    })
}

fn adjacent_marker(
    location: &super::DatabaseLocation,
    product: Product,
    purpose: &str,
) -> anyhow::Result<PathBuf> {
    let database_name = location
        .database_name
        .to_str()
        .context("upgrade database name must be UTF-8")?;
    Ok(location
        .configured_database_path()
        .parent()
        .context("upgrade database must have a parent")?
        .join(format!(".{database_name}.{}.{purpose}", product.slug())))
}

fn copy_regular_file_at(
    source_parent: &File,
    source_name: &OsStr,
    destination_parent: &File,
    destination_name: &OsStr,
) -> anyhow::Result<()> {
    let source = openat2(
        source_parent,
        source_name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        secure_resolve_flags(),
    )?;
    let source_metadata = fstat(&source)?;
    ensure!(
        FileType::from_raw_mode(source_metadata.st_mode) == FileType::RegularFile
            && source_metadata.st_nlink == 1,
        "source generation entry must be one regular file"
    );
    let destination = openat2(
        destination_parent,
        destination_name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
        secure_resolve_flags(),
    )?;
    let mut source = File::from(source);
    let mut destination = File::from(destination);
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        destination.write_all(&buffer[..read])?;
        copied = copied
            .checked_add(read as u64)
            .context("source clone size overflow")?;
    }
    ensure!(
        copied == source_metadata.st_size as u64,
        "source generation entry changed while it was cloned"
    );
    destination.sync_all()?;
    Ok(())
}

fn expected_migration_checksum(sql: &str) -> Vec<u8> {
    Sha384::digest(sql.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use super::*;
    use crate::{RecoveryAction, recover_sqlite_restore};

    fn create_host_fixture(path: &Path) {
        host_0_6_to_0_7::create_fixture(path).unwrap();
    }

    fn create_sunshine_fixture(path: &Path) {
        sunshine_0_6_to_0_7::create_fixture(path).unwrap();
    }

    fn generation_bytes(path: &Path) -> BTreeMap<String, Vec<u8>> {
        std::iter::once("")
            .chain(SQLITE_SIDECARS)
            .filter_map(|suffix| {
                let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
                candidate
                    .exists()
                    .then(|| (suffix.to_owned(), fs::read(candidate).unwrap()))
            })
            .collect()
    }

    #[test]
    fn generic_current_allowlist_matches_registered_adapter_targets() {
        for adapter in [host_0_6_to_0_7::ADAPTER, sunshine_0_6_to_0_7::ADAPTER] {
            let official = super::super::official_sqlite_identity(adapter.product).unwrap();
            assert_eq!(adapter.to_version, official.application_version);
            assert_eq!(adapter.target_revision, official.schema_revision);
            assert_eq!(adapter.target_schema_sha256, official.schema_sha256);
        }
    }

    #[test]
    fn upgrades_real_host_fixture_and_preserves_every_table() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("host.sqlite3");
        let backup = root.path().join("host-0.6-backup");
        create_host_fixture(&database);

        let result = upgrade_sqlite(
            Product::HostMonitoring,
            "0.6.0",
            "0.7.0",
            &database,
            &backup,
        )
        .unwrap();
        assert_eq!(result.schema_identity.schema_revision, 1);
        assert_eq!(
            result.schema_identity.schema_sha256,
            host_0_6_to_0_7::TARGET_SCHEMA_SHA256
        );
        verify_source_backup(Product::HostMonitoring, "0.6.0", "0.7.0", &backup).unwrap();

        let current = Connection::open(&database).unwrap();
        for table in host_0_6_to_0_7::DATA_TABLES {
            let rows: i64 = current
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(rows, 1, "{table}");
        }
        let ledgers: i64 = current
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name='_sqlx_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledgers, 0);
        current
            .execute(
                "INSERT INTO audit_events (action,target,actor,created_at) VALUES ('next','x','test','2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        let next_id: i64 = current
            .query_row("SELECT MAX(event_id) FROM audit_events", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(next_id, 8);
    }

    #[test]
    fn rejects_wrong_adapter_and_tampered_ledger_without_touching_source() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("host.sqlite3");
        create_host_fixture(&database);
        let connection = Connection::open(&database).unwrap();
        connection
            .execute(
                "UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=3",
                [],
            )
            .unwrap();
        drop(connection);
        fs::write(
            PathBuf::from(format!("{}-journal", database.display())),
            b"ignored-non-hot-journal",
        )
        .unwrap();
        let before = generation_bytes(&database);

        assert!(
            upgrade_sqlite(
                Product::HostMonitoring,
                "0.6.0",
                "0.7.0",
                &database,
                &root.path().join("backup")
            )
            .is_err()
        );
        assert_eq!(generation_bytes(&database), before);
        assert!(!root.path().join("backup").exists());
        assert!(
            upgrade_sqlite(
                Product::HostMonitoring,
                "0.5.0",
                "0.7.0",
                &database,
                &root.path().join("other")
            )
            .is_err()
        );
    }

    #[test]
    fn exclusive_product_lock_and_hard_linked_sidecars_fail_closed() {
        use std::fs::hard_link;

        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("host.sqlite3");
        create_host_fixture(&database);
        let location = super::super::DatabaseLocation::resolve(&database).unwrap();
        let held = location
            .acquire_lock(
                Product::HostMonitoring,
                rustix::fs::FlockOperation::NonBlockingLockExclusive,
            )
            .unwrap();
        assert!(
            upgrade_sqlite(
                Product::HostMonitoring,
                "0.6.0",
                "0.7.0",
                &database,
                &root.path().join("locked-backup")
            )
            .is_err()
        );
        drop(held);

        let journal = PathBuf::from(format!("{}-journal", database.display()));
        fs::write(&journal, b"not-hot").unwrap();
        hard_link(&journal, root.path().join("journal-alias")).unwrap();
        let before = generation_bytes(&database);
        assert!(
            upgrade_sqlite(
                Product::HostMonitoring,
                "0.6.0",
                "0.7.0",
                &database,
                &root.path().join("hardlink-backup")
            )
            .is_err()
        );
        assert_eq!(generation_bytes(&database), before);
    }

    #[test]
    fn refuses_nearby_schema_and_never_overwrites_backup() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("host.sqlite3");
        create_host_fixture(&database);
        Connection::open(&database)
            .unwrap()
            .execute_batch("CREATE TABLE almost_old (id INTEGER PRIMARY KEY);")
            .unwrap();
        let before = generation_bytes(&database);
        let output = root.path().join("backup");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("keep"), b"unchanged").unwrap();
        assert!(
            upgrade_sqlite(
                Product::HostMonitoring,
                "0.6.0",
                "0.7.0",
                &database,
                &output
            )
            .is_err()
        );
        assert_eq!(generation_bytes(&database), before);
        assert_eq!(fs::read(output.join("keep")).unwrap(), b"unchanged");
    }

    #[test]
    fn source_backup_detects_database_and_manifest_corruption() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("host.sqlite3");
        let backup = root.path().join("backup");
        create_host_fixture(&database);
        let adapter = host_0_6_to_0_7::ADAPTER;
        let identity = verify_source_database(&database, adapter).unwrap();
        create_source_backup(adapter, &database, &identity, &backup).unwrap();
        OpenOptions::new()
            .append(true)
            .open(backup.join(DATABASE_FILE))
            .unwrap()
            .write_all(b"corrupt")
            .unwrap();
        assert!(verify_source_backup(Product::HostMonitoring, "0.6.0", "0.7.0", &backup).is_err());

        let second = root.path().join("second-backup");
        create_source_backup(adapter, &database, &identity, &second).unwrap();
        let manifest_path = second.join(MANIFEST_FILE);
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["unexpected"] = serde_json::json!(true);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(verify_source_backup(Product::HostMonitoring, "0.6.0", "0.7.0", &second).is_err());
    }

    #[test]
    fn interrupted_host_upgrade_can_commit_or_restore_exact_old_bytes() {
        for action in [RecoveryAction::Rollback, RecoveryAction::Commit] {
            let root = tempfile::tempdir().unwrap();
            let database = root.path().join("host.sqlite3");
            let backup = root.path().join("backup");
            create_host_fixture(&database);
            let before = generation_bytes(&database);
            let result = upgrade_sqlite_with_hook(
                Product::HostMonitoring,
                "0.6.0",
                "0.7.0",
                &database,
                &backup,
                |point| {
                    if matches!(point, RestorePoint::Installed) {
                        anyhow::bail!("injected interruption")
                    }
                    Ok(())
                },
            );
            assert!(result.is_err());
            let recovery = fs::read_dir(root.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .contains(".restore-")
                })
                .unwrap();
            recover_sqlite_restore(Product::HostMonitoring, "0.7.0", &recovery, action).unwrap();
            if action == RecoveryAction::Rollback {
                assert_eq!(generation_bytes(&database), before);
                verify_source_database(&database, host_0_6_to_0_7::ADAPTER).unwrap();
            } else {
                let identity = verify_current_database(&database, Product::HostMonitoring).unwrap();
                assert_eq!(identity.application_version, "0.7.0");
            }
        }
    }

    #[test]
    fn upgrades_real_sunshine_fixture_and_preserves_every_table() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("sunshine.sqlite3");
        let backup = root.path().join("sunshine-0.6-backup");
        create_sunshine_fixture(&database);

        let result = upgrade_sqlite(
            Product::SunshineManager,
            "0.6.0",
            "0.7.0",
            &database,
            &backup,
        )
        .unwrap();
        assert_eq!(result.schema_identity.schema_revision, 1);
        assert_eq!(
            result.schema_identity.schema_sha256,
            sunshine_0_6_to_0_7::TARGET_SCHEMA_SHA256
        );
        let source =
            verify_source_backup(Product::SunshineManager, "0.6.0", "0.7.0", &backup).unwrap();
        assert_eq!(source.manifest.schema_identity.unwrap().schema_revision, 3);

        let current = Connection::open(&database).unwrap();
        for table in sunshine_0_6_to_0_7::DATA_TABLES {
            let rows: i64 = current
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(rows, 1, "{table}");
        }
        current
            .execute(
                "INSERT INTO audit_logs (action,target,actor,created_at_micros) VALUES ('next','x','test',12)",
                [],
            )
            .unwrap();
        let next_id: i64 = current
            .query_row("SELECT MAX(audit_id) FROM audit_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(next_id, 8);
    }

    #[test]
    fn rejects_tampered_sunshine_ledger_without_touching_source_or_sidecar() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("sunshine.sqlite3");
        create_sunshine_fixture(&database);
        Connection::open(&database)
            .unwrap()
            .execute(
                "UPDATE _sqlx_migrations SET installed_on='not-a-timestamp' \
                 WHERE version=202608290002",
                [],
            )
            .unwrap();
        fs::write(
            PathBuf::from(format!("{}-journal", database.display())),
            b"ignored-non-hot-journal",
        )
        .unwrap();
        let before = generation_bytes(&database);
        assert!(
            upgrade_sqlite(
                Product::SunshineManager,
                "0.6.0",
                "0.7.0",
                &database,
                &root.path().join("backup")
            )
            .is_err()
        );
        assert_eq!(generation_bytes(&database), before);
        assert!(!root.path().join("backup").exists());
    }

    #[test]
    fn interrupted_sunshine_upgrade_can_commit_or_restore_exact_old_bytes() {
        for action in [RecoveryAction::Rollback, RecoveryAction::Commit] {
            let root = tempfile::tempdir().unwrap();
            let database = root.path().join("sunshine.sqlite3");
            let backup = root.path().join("backup");
            create_sunshine_fixture(&database);
            let before = generation_bytes(&database);
            let result = upgrade_sqlite_with_hook(
                Product::SunshineManager,
                "0.6.0",
                "0.7.0",
                &database,
                &backup,
                |point| {
                    if matches!(point, RestorePoint::Installed) {
                        anyhow::bail!("injected interruption")
                    }
                    Ok(())
                },
            );
            assert!(result.is_err());
            let recovery = fs::read_dir(root.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .contains(".restore-")
                })
                .unwrap();
            recover_sqlite_restore(Product::SunshineManager, "0.7.0", &recovery, action).unwrap();
            if action == RecoveryAction::Rollback {
                assert_eq!(generation_bytes(&database), before);
                verify_source_database(&database, sunshine_0_6_to_0_7::ADAPTER).unwrap();
            } else {
                let identity =
                    verify_current_database(&database, Product::SunshineManager).unwrap();
                assert_eq!(identity.application_version, "0.7.0");
            }
        }
    }
}

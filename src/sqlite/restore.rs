use std::{
    fmt,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::fd::AsRawFd,
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, ensure};
use rustix::{
    fs::{
        AtFlags, FileType, Mode, OFlags, RenameFlags, fstat, mkdirat, openat2, renameat_with,
        statat, unlinkat,
    },
    io::Errno,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    DATABASE_FILE, DatabaseLocation, MaintenanceLock, SecureDirectory, absolute_path,
    hash_regular_file, secure_resolve_flags, verify_current_database, verify_sqlite_backup,
};
use crate::{Product, SchemaIdentity};

const JOURNAL_VERSION: u32 = 1;
const JOURNAL_FILE: &str = "restore-journal.json";
const INCOMING_FILE: &str = "incoming.sqlite3";
const PHASE_PREPARED: &str = "phase-prepared";
const PHASE_ORIGINALS_PRESERVED: &str = "phase-originals-preserved";
const PHASE_INSTALLED: &str = "phase-installed";
const PHASE_VERIFIED: &str = "phase-verified";
const MAX_JOURNAL_BYTES: u64 = 1024 * 1024;
const SQLITE_SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreExisting {
    Refuse,
    Replace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryAction {
    Commit,
    Rollback,
}

impl fmt::Display for RecoveryAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Commit => "commit",
            Self::Rollback => "rollback",
        })
    }
}

impl FromStr for RecoveryAction {
    type Err = ParseRecoveryActionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "commit" => Ok(Self::Commit),
            "rollback" => Ok(Self::Rollback),
            _ => Err(ParseRecoveryActionError(value.to_owned())),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unsupported recovery action {0:?}; expected commit or rollback")]
pub struct ParseRecoveryActionError(String);

#[derive(Clone, Debug, Serialize)]
pub struct RestoreResult {
    pub product: Product,
    pub application_version: String,
    pub schema_identity: SchemaIdentity,
    pub database: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecoveryResult {
    pub action: RecoveryAction,
    pub product: Product,
    pub database: PathBuf,
}

/// Restore a verified SQLite-only backup under an exclusive product lock.
///
/// Replacing an existing generation requires an explicit policy. The old main
/// database and every SQLite sidecar are durably moved into a recovery journal
/// before the new database is installed.
pub fn restore_sqlite_backup(
    product: Product,
    expected_application_version: &str,
    input: &Path,
    database: &Path,
    existing: RestoreExisting,
) -> anyhow::Result<RestoreResult> {
    let backup = verify_sqlite_backup(input)?;
    ensure!(
        backup.manifest.product == product,
        "backup product does not match --product"
    );
    ensure!(
        backup.manifest.application_version == expected_application_version,
        "backup application version does not match --expect-version"
    );
    let expected_identity = backup
        .manifest
        .schema_identity
        .clone()
        .context("verified SQLite backup has no schema identity")?;

    let maintenance = MaintenanceLock::exclusive(product, database)?;
    let location = &maintenance.location;
    let originals = inspect_original_generation(location)?;
    if !originals.is_empty() && existing == RestoreExisting::Refuse {
        anyhow::bail!(
            "restore destination already has a SQLite generation; pass --replace-existing"
        );
    }
    ensure_backup_and_destination_are_distinct(input, location)?;

    let recovery = RecoveryDirectory::create(location, product)?;
    let mut mutation_started = false;
    let attempt = restore_prepared(
        &backup.directory,
        location,
        &recovery,
        originals,
        product,
        &expected_identity,
        &mut mutation_started,
        |_| Ok(()),
    );
    match attempt {
        Ok(()) => {
            recovery.cleanup().with_context(|| {
                format!(
                    "restore is installed and verified, but recovery cleanup is incomplete at {}",
                    recovery.configured_path.display()
                )
            })?;
            Ok(RestoreResult {
                product,
                application_version: expected_identity.application_version.clone(),
                schema_identity: expected_identity,
                database: location.configured_database_path().to_path_buf(),
            })
        }
        Err(error) if !mutation_started => {
            let _ = recovery.cleanup();
            Err(error.context("restore failed before the destination was changed"))
        }
        Err(error) => Err(error.context(format!(
            "restore was interrupted after destination mutation; recovery evidence is preserved at {}",
            recovery.configured_path.display()
        ))),
    }
}

/// Resolve an interrupted restore journal in the direction chosen by an
/// operator. No automatic direction is inferred from an old version.
pub fn recover_sqlite_restore(
    recovery_path: &Path,
    action: RecoveryAction,
) -> anyhow::Result<RecoveryResult> {
    let recovery = RecoveryDirectory::open(recovery_path)?;
    let journal = recovery.read_journal()?;
    journal.validate()?;
    recovery.validate_name(&journal)?;
    let destination = recovery
        .configured_path
        .parent()
        .context("recovery directory must have a parent")?
        .join(&journal.destination_name);
    let maintenance = MaintenanceLock::exclusive(journal.product, &destination)?;
    ensure_same_directory(&recovery.parent.file, &maintenance.location.parent)?;

    match action {
        RecoveryAction::Commit => {
            resume_commit(&maintenance.location, &recovery, &journal)?;
            recovery.cleanup().with_context(|| {
                format!(
                    "restore is committed, but recovery cleanup is incomplete at {}",
                    recovery.configured_path.display()
                )
            })?;
        }
        RecoveryAction::Rollback => {
            resume_rollback(&maintenance.location, &recovery, &journal)?;
            recovery.cleanup().with_context(|| {
                format!(
                    "rollback is complete, but recovery cleanup is incomplete at {}",
                    recovery.configured_path.display()
                )
            })?;
        }
    }

    Ok(RecoveryResult {
        action,
        product: journal.product,
        database: destination,
    })
}

#[derive(Clone, Copy, Debug)]
enum RestorePoint {
    OriginalsPreserved,
    Installed,
    Verified,
}

#[allow(clippy::too_many_arguments)]
fn restore_prepared(
    backup_directory: &Path,
    location: &DatabaseLocation,
    recovery: &RecoveryDirectory,
    originals: Vec<GenerationEntry>,
    product: Product,
    expected_identity: &SchemaIdentity,
    mutation_started: &mut bool,
    mut hook: impl FnMut(RestorePoint) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let backup = SecureDirectory::open(backup_directory, "backup directory")?;
    copy_regular_file(
        &backup.file,
        DATABASE_FILE,
        &recovery.directory.file,
        INCOMING_FILE,
    )?;
    let incoming_path = recovery.child_path(INCOMING_FILE);
    let incoming_identity = verify_current_database(&incoming_path, product)?;
    ensure!(
        &incoming_identity == expected_identity,
        "staged restore identity changed after backup verification"
    );
    let (incoming_bytes, incoming_sha256) = hash_regular_file(&incoming_path)?;
    let journal = RestoreJournal {
        journal_version: JOURNAL_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        product,
        application_version: expected_identity.application_version.clone(),
        schema_identity: expected_identity.clone(),
        destination_name: location
            .database_name
            .to_str()
            .context("restore database name must be UTF-8")?
            .to_owned(),
        incoming_bytes,
        incoming_sha256,
        originals,
        created_at_epoch_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_secs(),
    };
    journal.validate()?;
    recovery.write_journal(&journal)?;
    recovery.write_marker(PHASE_PREPARED)?;

    *mutation_started = true;
    preserve_originals(location, recovery, &journal)?;
    recovery.write_marker(PHASE_ORIGINALS_PRESERVED)?;
    hook(RestorePoint::OriginalsPreserved)?;

    install_incoming(location, recovery, &journal)?;
    recovery.write_marker(PHASE_INSTALLED)?;
    hook(RestorePoint::Installed)?;

    verify_installed(location, &journal)?;
    recovery.write_marker(PHASE_VERIFIED)?;
    hook(RestorePoint::Verified)?;
    Ok(())
}

fn preserve_originals(
    location: &DatabaseLocation,
    recovery: &RecoveryDirectory,
    journal: &RestoreJournal,
) -> anyhow::Result<()> {
    for entry in &journal.originals {
        let source_exists =
            checked_file_state(&location.parent, &entry.destination_name, Some(entry))?;
        let recovery_exists =
            checked_file_state(&recovery.directory.file, &entry.recovery_name, Some(entry))?;
        match (source_exists, recovery_exists) {
            (true, false) => {
                renameat_with(
                    &location.parent,
                    &entry.destination_name,
                    &recovery.directory.file,
                    &entry.recovery_name,
                    RenameFlags::NOREPLACE,
                )
                .context("preserve original SQLite generation entry")?;
                location.parent.sync_all()?;
                recovery.directory.file.sync_all()?;
            }
            (false, true) => {}
            (true, true) => anyhow::bail!(
                "original generation entry exists in both destination and recovery directory"
            ),
            (false, false) => {
                anyhow::bail!("original generation entry disappeared before it could be preserved")
            }
        }
    }
    Ok(())
}

fn install_incoming(
    location: &DatabaseLocation,
    recovery: &RecoveryDirectory,
    journal: &RestoreJournal,
) -> anyhow::Result<()> {
    match statat(
        &location.parent,
        &location.database_name,
        AtFlags::SYMLINK_NOFOLLOW,
    ) {
        Err(Errno::NOENT) => {
            ensure_incoming_matches(recovery, journal)?;
            renameat_with(
                &recovery.directory.file,
                INCOMING_FILE,
                &location.parent,
                &location.database_name,
                RenameFlags::NOREPLACE,
            )
            .context("install staged SQLite restore without overwriting")?;
            recovery.directory.file.sync_all()?;
            location.parent.sync_all()?;
        }
        Ok(metadata) => {
            ensure!(
                FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                    && metadata.st_nlink == 1,
                "restore destination is not one regular file"
            );
            ensure_installed_file_matches(location, journal)?;
        }
        Err(error) => {
            return Err(std::io::Error::from(error)).context("inspect restore destination");
        }
    }
    Ok(())
}

fn verify_installed(location: &DatabaseLocation, journal: &RestoreJournal) -> anyhow::Result<()> {
    let path = location.database_path();
    let identity = verify_current_database(&path, journal.product)?;
    ensure!(
        identity == journal.schema_identity,
        "installed SQLite schema identity is not the staged identity"
    );
    ensure_installed_file_matches(location, journal)
}

fn resume_commit(
    location: &DatabaseLocation,
    recovery: &RecoveryDirectory,
    journal: &RestoreJournal,
) -> anyhow::Result<()> {
    if recovery.marker_exists(PHASE_VERIFIED)? {
        verify_installed(location, journal)?;
        return Ok(());
    }
    preserve_originals_for_resume(location, recovery, journal)?;
    install_incoming(location, recovery, journal)?;
    recovery.write_marker(PHASE_INSTALLED)?;
    verify_installed(location, journal)?;
    recovery.write_marker(PHASE_VERIFIED)?;
    Ok(())
}

fn preserve_originals_for_resume(
    location: &DatabaseLocation,
    recovery: &RecoveryDirectory,
    journal: &RestoreJournal,
) -> anyhow::Result<()> {
    for entry in &journal.originals {
        let destination_state =
            regular_file_hash_if_present(&location.parent, &entry.destination_name)?;
        let recovery_state =
            regular_file_hash_if_present(&recovery.directory.file, &entry.recovery_name)?;

        match (destination_state, recovery_state) {
            (Some((bytes, hash)), None) if bytes == entry.bytes && hash == entry.sha256 => {
                renameat_with(
                    &location.parent,
                    &entry.destination_name,
                    &recovery.directory.file,
                    &entry.recovery_name,
                    RenameFlags::NOREPLACE,
                )?;
                location.parent.sync_all()?;
                recovery.directory.file.sync_all()?;
            }
            (None, Some((bytes, hash))) if bytes == entry.bytes && hash == entry.sha256 => {}
            (Some((bytes, hash)), Some((saved_bytes, saved_hash)))
                if entry.destination_name == journal.destination_name
                    && bytes == journal.incoming_bytes
                    && hash == journal.incoming_sha256
                    && saved_bytes == entry.bytes
                    && saved_hash == entry.sha256 => {}
            _ => anyhow::bail!(
                "cannot prove original generation entry {} before committing recovery",
                entry.destination_name
            ),
        }
    }
    Ok(())
}

fn resume_rollback(
    location: &DatabaseLocation,
    recovery: &RecoveryDirectory,
    journal: &RestoreJournal,
) -> anyhow::Result<()> {
    let original_database = journal
        .originals
        .iter()
        .find(|entry| entry.destination_name == journal.destination_name);
    if let Some((bytes, hash)) =
        regular_file_hash_if_present(&location.parent, &journal.destination_name)?
    {
        let is_original =
            original_database.is_some_and(|entry| bytes == entry.bytes && hash == entry.sha256);
        if !is_original {
            ensure!(
                bytes == journal.incoming_bytes && hash == journal.incoming_sha256,
                "destination is neither the original nor the staged database"
            );
            renameat_with(
                &location.parent,
                &journal.destination_name,
                &recovery.directory.file,
                "abandoned-new.sqlite3",
                RenameFlags::NOREPLACE,
            )
            .context("preserve abandoned installed database during rollback")?;
            location.parent.sync_all()?;
            recovery.directory.file.sync_all()?;
        }
    }

    for entry in &journal.originals {
        let destination_state =
            regular_file_hash_if_present(&location.parent, &entry.destination_name)?;
        let recovery_state =
            regular_file_hash_if_present(&recovery.directory.file, &entry.recovery_name)?;
        match (destination_state, recovery_state) {
            (Some((bytes, hash)), None) if bytes == entry.bytes && hash == entry.sha256 => {}
            (None, Some((bytes, hash))) if bytes == entry.bytes && hash == entry.sha256 => {
                renameat_with(
                    &recovery.directory.file,
                    &entry.recovery_name,
                    &location.parent,
                    &entry.destination_name,
                    RenameFlags::NOREPLACE,
                )
                .context("restore original SQLite generation during rollback")?;
                recovery.directory.file.sync_all()?;
                location.parent.sync_all()?;
            }
            _ => anyhow::bail!(
                "cannot prove original generation entry {} before rollback",
                entry.destination_name
            ),
        }
    }

    if original_database.is_none() {
        ensure!(
            regular_file_hash_if_present(&location.parent, &journal.destination_name)?.is_none(),
            "rollback of an originally absent database did not leave it absent"
        );
    }
    Ok(())
}

fn ensure_incoming_matches(
    recovery: &RecoveryDirectory,
    journal: &RestoreJournal,
) -> anyhow::Result<()> {
    let (bytes, sha256) = hash_regular_file(&recovery.child_path(INCOMING_FILE))
        .context("verify staged incoming database")?;
    ensure!(
        bytes == journal.incoming_bytes && sha256 == journal.incoming_sha256,
        "staged incoming database does not match its restore journal"
    );
    Ok(())
}

fn ensure_installed_file_matches(
    location: &DatabaseLocation,
    journal: &RestoreJournal,
) -> anyhow::Result<()> {
    let (bytes, sha256) = hash_regular_file(&location.database_path())?;
    ensure!(
        bytes == journal.incoming_bytes && sha256 == journal.incoming_sha256,
        "installed database does not match the staged restore"
    );
    Ok(())
}

fn inspect_original_generation(
    location: &DatabaseLocation,
) -> anyhow::Result<Vec<GenerationEntry>> {
    let database_name = location
        .database_name
        .to_str()
        .context("restore database name must be UTF-8")?;
    let mut names = vec![database_name.to_owned()];
    names.extend(
        SQLITE_SIDECARS
            .into_iter()
            .map(|suffix| format!("{database_name}{suffix}")),
    );
    let mut entries = Vec::new();
    for destination_name in names {
        if let Some((bytes, sha256)) =
            regular_file_hash_if_present(&location.parent, &destination_name)?
        {
            let recovery_name = if destination_name == database_name {
                "original.sqlite3".to_owned()
            } else {
                format!(
                    "original.sqlite3{}",
                    destination_name
                        .strip_prefix(database_name)
                        .context("invalid SQLite sidecar name")?
                )
            };
            entries.push(GenerationEntry {
                destination_name,
                recovery_name,
                bytes,
                sha256,
            });
        }
    }
    Ok(entries)
}

fn regular_file_hash_if_present(
    parent: &File,
    name: &str,
) -> anyhow::Result<Option<(u64, String)>> {
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(metadata) => {
            ensure!(
                FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                    && metadata.st_nlink == 1,
                "SQLite generation entry must be one regular file"
            );
            Ok(Some(hash_regular_file(&child_path(parent, name))?))
        }
        Err(Errno::NOENT) => Ok(None),
        Err(error) => Err(std::io::Error::from(error)).context("inspect SQLite generation entry"),
    }
}

fn checked_file_state(
    parent: &File,
    name: &str,
    expected: Option<&GenerationEntry>,
) -> anyhow::Result<bool> {
    let Some((bytes, sha256)) = regular_file_hash_if_present(parent, name)? else {
        return Ok(false);
    };
    if let Some(expected) = expected {
        ensure!(
            bytes == expected.bytes && sha256 == expected.sha256,
            "SQLite generation entry changed after restore preparation"
        );
    }
    Ok(true)
}

fn copy_regular_file(
    source_parent: &File,
    source_name: &str,
    destination_parent: &File,
    destination_name: &str,
) -> anyhow::Result<()> {
    let source = openat2(
        source_parent,
        source_name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        super::secure_resolve_flags(),
    )?;
    let source_metadata = fstat(&source)?;
    ensure!(
        FileType::from_raw_mode(source_metadata.st_mode) == FileType::RegularFile
            && source_metadata.st_nlink == 1,
        "restore source must be one regular file"
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
    let copied = std::io::copy(&mut source, &mut destination)?;
    ensure!(
        copied == source_metadata.st_size as u64,
        "restore source changed while it was staged"
    );
    destination.sync_all()?;
    destination_parent.sync_all()?;
    Ok(())
}

fn ensure_backup_and_destination_are_distinct(
    backup_directory: &Path,
    location: &DatabaseLocation,
) -> anyhow::Result<()> {
    let backup = SecureDirectory::open(backup_directory, "backup directory")?;
    let source = openat2(
        &backup.file,
        DATABASE_FILE,
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        secure_resolve_flags(),
    )?;
    let source = fstat(source)?;
    if let Ok(destination) = statat(
        &location.parent,
        &location.database_name,
        AtFlags::SYMLINK_NOFOLLOW,
    ) {
        ensure!(
            source.st_dev != destination.st_dev || source.st_ino != destination.st_ino,
            "backup database and restore destination are the same file"
        );
    }
    Ok(())
}

fn ensure_same_directory(left: &File, right: &File) -> anyhow::Result<()> {
    let left = fstat(left)?;
    let right = fstat(right)?;
    ensure!(
        left.st_dev == right.st_dev && left.st_ino == right.st_ino,
        "recovery journal and restore destination do not share one parent directory"
    );
    Ok(())
}

fn child_path(parent: &File, name: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd())).join(name)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RestoreJournal {
    journal_version: u32,
    tool_version: String,
    product: Product,
    application_version: String,
    schema_identity: SchemaIdentity,
    destination_name: String,
    incoming_bytes: u64,
    incoming_sha256: String,
    originals: Vec<GenerationEntry>,
    created_at_epoch_seconds: u64,
}

impl RestoreJournal {
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.journal_version == JOURNAL_VERSION,
            "unsupported restore journal version"
        );
        ensure!(
            !self.tool_version.is_empty()
                && self.tool_version.len() <= 128
                && self
                    .tool_version
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')),
            "restore journal tool version is invalid"
        );
        self.schema_identity.validate(self.product)?;
        ensure!(
            self.application_version == self.schema_identity.application_version,
            "restore journal application version is inconsistent"
        );
        validate_single_name(&self.destination_name)?;
        validate_sha256(&self.incoming_sha256)?;
        ensure!(
            self.incoming_bytes > 0,
            "restore journal incoming database is empty"
        );
        let database_name = &self.destination_name;
        let allowed_destinations = [
            database_name.clone(),
            format!("{database_name}-wal"),
            format!("{database_name}-shm"),
            format!("{database_name}-journal"),
        ];
        let allowed_recovery = [
            "original.sqlite3".to_owned(),
            "original.sqlite3-wal".to_owned(),
            "original.sqlite3-shm".to_owned(),
            "original.sqlite3-journal".to_owned(),
        ];
        let mut previous = None;
        for original in &self.originals {
            original.validate()?;
            let position = allowed_destinations
                .iter()
                .position(|name| name == &original.destination_name)
                .context("restore journal contains an unknown destination entry")?;
            ensure!(
                original.recovery_name == allowed_recovery[position],
                "restore journal recovery entry mapping is invalid"
            );
            ensure!(
                previous.is_none_or(|last| last < position),
                "restore journal original entries are duplicated or unsorted"
            );
            previous = Some(position);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GenerationEntry {
    destination_name: String,
    recovery_name: String,
    bytes: u64,
    sha256: String,
}

impl GenerationEntry {
    fn validate(&self) -> anyhow::Result<()> {
        validate_single_name(&self.destination_name)?;
        validate_single_name(&self.recovery_name)?;
        validate_sha256(&self.sha256)?;
        Ok(())
    }
}

fn validate_single_name(name: &str) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= 255,
        "invalid journal entry name"
    );
    let path = Path::new(name);
    ensure!(
        path.components().count() == 1
            && matches!(path.components().next(), Some(Component::Normal(_))),
        "journal entry name is not one safe path component"
    );
    Ok(())
}

fn validate_sha256(value: &str) -> anyhow::Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "journal SHA-256 is not canonical lowercase hexadecimal"
    );
    Ok(())
}

struct RecoveryDirectory {
    parent: SecureDirectory,
    directory: SecureDirectory,
    name: String,
    configured_path: PathBuf,
}

impl RecoveryDirectory {
    fn create(location: &DatabaseLocation, product: Product) -> anyhow::Result<Self> {
        let database_name = location
            .database_name
            .to_str()
            .context("restore database name must be UTF-8")?;
        let name = format!(
            ".{database_name}.{}.restore-{}",
            product.slug(),
            Uuid::new_v4()
        );
        mkdirat(&location.parent, &name, Mode::from_raw_mode(0o700))
            .context("create restore recovery directory")?;
        location.parent.sync_all()?;
        let directory = openat2(
            &location.parent,
            &name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            secure_resolve_flags(),
        )?;
        let configured_path = location
            .configured_database_path()
            .parent()
            .context("restore destination must have a parent")?
            .join(&name);
        Ok(Self {
            parent: SecureDirectory {
                file: location.parent.try_clone()?,
            },
            directory: SecureDirectory {
                file: File::from(directory),
            },
            name,
            configured_path,
        })
    }

    fn open(path: &Path) -> anyhow::Result<Self> {
        let configured_path = absolute_path(path)?;
        let parent_path = configured_path
            .parent()
            .context("recovery directory must have a parent")?;
        let name = configured_path
            .file_name()
            .and_then(|name| name.to_str())
            .context("recovery directory name must be UTF-8")?
            .to_owned();
        validate_single_name(&name)?;
        let parent = SecureDirectory::open(parent_path, "recovery parent")?;
        let directory = SecureDirectory::open(&configured_path, "recovery directory")?;
        Ok(Self {
            parent,
            directory,
            name,
            configured_path,
        })
    }

    fn child_path(&self, name: impl AsRef<Path>) -> PathBuf {
        self.directory.child_path(name)
    }

    fn write_journal(&self, journal: &RestoreJournal) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec_pretty(journal)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.child_path(JOURNAL_FILE))?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        self.directory.file.sync_all()?;
        Ok(())
    }

    fn read_journal(&self) -> anyhow::Result<RestoreJournal> {
        let bytes = read_bounded_at(&self.directory.file, JOURNAL_FILE, MAX_JOURNAL_BYTES)?;
        let journal: RestoreJournal = serde_json::from_slice(&bytes)?;
        Ok(journal)
    }

    fn write_marker(&self, name: &str) -> anyhow::Result<()> {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.child_path(name))
        {
            Ok(file) => file.sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let (bytes, _) = hash_regular_file(&self.child_path(name))?;
                ensure!(bytes == 0, "restore phase marker is invalid");
            }
            Err(error) => return Err(error).context("write restore phase marker"),
        }
        self.directory.file.sync_all()?;
        Ok(())
    }

    fn marker_exists(&self, name: &str) -> anyhow::Result<bool> {
        match statat(&self.directory.file, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(metadata) => {
                ensure!(
                    FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                        && metadata.st_nlink == 1
                        && metadata.st_size == 0,
                    "restore phase marker is invalid"
                );
                Ok(true)
            }
            Err(Errno::NOENT) => Ok(false),
            Err(error) => Err(std::io::Error::from(error)).context("inspect restore phase marker"),
        }
    }

    fn validate_name(&self, journal: &RestoreJournal) -> anyhow::Result<()> {
        let prefix = format!(
            ".{}.{}.restore-",
            journal.destination_name,
            journal.product.slug()
        );
        let suffix = self
            .name
            .strip_prefix(&prefix)
            .context("recovery directory name does not match its journal")?;
        Uuid::parse_str(suffix).context("recovery directory has an invalid identifier")?;
        Ok(())
    }

    fn cleanup(&self) -> anyhow::Result<()> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(self.directory.child_path("."))? {
            let entry = entry?;
            names.push(entry.file_name());
        }
        // Keep the journal and verified marker until all data evidence has
        // been removed. An interrupted cleanup can still prove the installed
        // generation before an operator retries commit.
        names.sort_by_key(|name| {
            if name == JOURNAL_FILE {
                1
            } else if name == PHASE_VERIFIED {
                2
            } else {
                0
            }
        });
        for name in names {
            let metadata = statat(&self.directory.file, &name, AtFlags::SYMLINK_NOFOLLOW)?;
            ensure!(
                FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                    && metadata.st_nlink == 1,
                "recovery cleanup refuses a non-regular or hard-linked entry"
            );
            unlinkat(&self.directory.file, &name, AtFlags::empty())
                .context("remove recovery entry")?;
            self.directory.file.sync_all()?;
        }
        unlinkat(&self.parent.file, &self.name, AtFlags::REMOVEDIR)
            .context("remove empty restore recovery directory")?;
        self.parent.file.sync_all()?;
        Ok(())
    }
}

fn read_bounded_at(parent: &File, name: &str, limit: u64) -> anyhow::Result<Vec<u8>> {
    let fd = openat2(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        secure_resolve_flags(),
    )?;
    let metadata = fstat(&fd)?;
    ensure!(
        FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
            && metadata.st_nlink == 1,
        "restore journal must be one regular file"
    );
    ensure!(
        metadata.st_size >= 0 && metadata.st_size as u64 <= limit,
        "restore journal is too large"
    );
    let mut bytes = Vec::with_capacity(metadata.st_size as usize);
    File::from(fd).take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "restore journal is too large");
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::Connection;

    use super::*;
    use crate::sqlite::{create_sqlite_backup, tests::create_current_database};

    fn widget_count(path: &Path) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM widgets", [], |row| row.get(0))
            .unwrap()
    }

    fn insert_widget(path: &Path, name: &str) {
        Connection::open(path)
            .unwrap()
            .execute("INSERT INTO widgets (name) VALUES (?1)", [name])
            .unwrap();
    }

    #[test]
    fn restores_missing_database_without_replace_flag() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.sqlite3");
        let backup = root.path().join("backup");
        let destination = root.path().join("restored.sqlite3");
        create_current_database(&source, Product::HostMonitoring, "0.7.0");
        insert_widget(&source, "saved");
        create_sqlite_backup(Product::HostMonitoring, &source, &backup).unwrap();

        restore_sqlite_backup(
            Product::HostMonitoring,
            "0.7.0",
            &backup,
            &destination,
            RestoreExisting::Refuse,
        )
        .unwrap();
        assert_eq!(widget_count(&destination), 1);
        assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".restore-")
        }));
    }

    #[test]
    fn refuses_existing_destination_without_explicit_replace() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.sqlite3");
        let destination = root.path().join("destination.sqlite3");
        let backup = root.path().join("backup");
        create_current_database(&source, Product::SunshineManager, "0.7.0");
        create_current_database(&destination, Product::SunshineManager, "0.7.0");
        let before = fs::read(&destination).unwrap();
        create_sqlite_backup(Product::SunshineManager, &source, &backup).unwrap();

        assert!(
            restore_sqlite_backup(
                Product::SunshineManager,
                "0.7.0",
                &backup,
                &destination,
                RestoreExisting::Refuse,
            )
            .is_err()
        );
        assert_eq!(fs::read(&destination).unwrap(), before);
    }

    #[test]
    fn replaces_existing_generation_only_after_verification() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.sqlite3");
        let destination = root.path().join("destination.sqlite3");
        let backup = root.path().join("backup");
        create_current_database(&source, Product::HostMonitoring, "0.7.0");
        create_current_database(&destination, Product::HostMonitoring, "0.7.0");
        insert_widget(&source, "new");
        insert_widget(&destination, "old-one");
        insert_widget(&destination, "old-two");
        create_sqlite_backup(Product::HostMonitoring, &source, &backup).unwrap();

        restore_sqlite_backup(
            Product::HostMonitoring,
            "0.7.0",
            &backup,
            &destination,
            RestoreExisting::Replace,
        )
        .unwrap();
        assert_eq!(widget_count(&destination), 1);
    }

    #[test]
    fn interrupted_install_can_be_rolled_back_or_committed() {
        for action in [RecoveryAction::Rollback, RecoveryAction::Commit] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("source.sqlite3");
            let destination = root.path().join("destination.sqlite3");
            let backup = root.path().join("backup");
            create_current_database(&source, Product::HostMonitoring, "0.7.0");
            create_current_database(&destination, Product::HostMonitoring, "0.7.0");
            insert_widget(&source, "new");
            insert_widget(&destination, "old-one");
            insert_widget(&destination, "old-two");
            create_sqlite_backup(Product::HostMonitoring, &source, &backup).unwrap();
            let verified = verify_sqlite_backup(&backup).unwrap();
            let identity = verified.manifest.schema_identity.unwrap();
            let maintenance =
                MaintenanceLock::exclusive(Product::HostMonitoring, &destination).unwrap();
            let originals = inspect_original_generation(&maintenance.location).unwrap();
            let recovery =
                RecoveryDirectory::create(&maintenance.location, Product::HostMonitoring).unwrap();
            let mut mutation_started = false;
            let result = restore_prepared(
                &verified.directory,
                &maintenance.location,
                &recovery,
                originals,
                Product::HostMonitoring,
                &identity,
                &mut mutation_started,
                |point| {
                    if matches!(point, RestorePoint::Installed) {
                        anyhow::bail!("injected crash")
                    }
                    Ok(())
                },
            );
            assert!(result.is_err());
            assert!(mutation_started);
            let recovery_path = recovery.configured_path.clone();
            drop(recovery);
            drop(maintenance);

            recover_sqlite_restore(&recovery_path, action).unwrap();
            assert_eq!(
                widget_count(&destination),
                if action == RecoveryAction::Commit {
                    1
                } else {
                    2
                }
            );
            assert!(!recovery_path.exists());
        }
    }

    #[test]
    fn wrong_expected_version_and_corrupt_backup_do_not_touch_destination() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.sqlite3");
        let destination = root.path().join("destination.sqlite3");
        let backup = root.path().join("backup");
        create_current_database(&source, Product::HostMonitoring, "0.7.0");
        create_current_database(&destination, Product::HostMonitoring, "0.7.0");
        let before = fs::read(&destination).unwrap();
        create_sqlite_backup(Product::HostMonitoring, &source, &backup).unwrap();

        assert!(
            restore_sqlite_backup(
                Product::HostMonitoring,
                "0.8.0",
                &backup,
                &destination,
                RestoreExisting::Replace,
            )
            .is_err()
        );
        assert_eq!(fs::read(&destination).unwrap(), before);
        OpenOptions::new()
            .append(true)
            .open(backup.join(DATABASE_FILE))
            .unwrap()
            .write_all(b"corrupt")
            .unwrap();
        assert!(
            restore_sqlite_backup(
                Product::HostMonitoring,
                "0.7.0",
                &backup,
                &destination,
                RestoreExisting::Replace,
            )
            .is_err()
        );
        assert_eq!(fs::read(&destination).unwrap(), before);
    }
}

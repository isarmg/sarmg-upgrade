use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use rustix::{
    fs::{AtFlags, FileType, RenameFlags, fchown, fstat, renameat_with, statat},
    io::Errno,
    process::{Gid, Uid},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    BACKUP_DATABASE_FILE, BACKUP_MANIFEST_FILE, BACKUP_RAW_GENERATION_DIRECTORY,
    DufsStoredResource, FROM_VERSION, MAX_DATABASE_GENERATION_ENTRY_BYTES, Product, SCHEMA_SHA256,
    SOURCE_USER_VERSION, SecureDirectory, StageMove, TARGET_REVISION, TO_VERSION,
    VerifiedDufsSourceBackup,
    config::ConfigAnchor,
    create_private_empty_file, encode_path, hash_regular_file, inspect_source_rows, lower_hex,
    owner_mapping, sync_directory,
    tree::{self, RootAnchor, RootIdentity, StagePosition},
    validate_distinct_protected_objects, verify_dufs_source_backup_anchored,
    verify_target_database,
};
use crate::{SchemaIdentity, sqlite::MaintenanceLock};

const JOURNAL_VERSION: u32 = 1;
const JOURNAL_FILE: &str = "journal.json";
const JOURNAL_TEMP_FILE: &str = ".journal.json.tmp";
const ORIGINAL_DATABASE_FILE: &str = "original.sqlite3";
const TARGET_DATABASE_FILE: &str = "target.sqlite3";
const BLOCKER_MAGIC: &[u8] =
    b"ISARMG-DUFS-UPGRADE-BLOCKER-V1\nThis is deliberately not a SQLite database.\n";
const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
const SIDECARS: [&str; 3] = ["-journal", "-wal", "-shm"];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum DufsJournalPhase {
    Prepared,
    Barrier,
    StageDirectoryMoved,
    TreeMoved,
    Installed,
    Verified,
    Committed,
    RolledBack,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct GenerationFile {
    suffix: String,
    bytes: u64,
    sha256: String,
    mode: u32,
    uid: u32,
    gid: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct JournalDocument {
    journal_version: u32,
    product: Product,
    from_version: String,
    to_version: String,
    phase: DufsJournalPhase,
    database_path_base64: String,
    root_device: u64,
    root_inode: u64,
    backup_path_base64: String,
    backup_manifest: DufsStoredResource,
    source_generation: Vec<GenerationFile>,
    target_database: DufsStoredResource,
    blocker_sha256: String,
    target_identity: SchemaIdentity,
    stage_moves: Vec<StageMove>,
}

pub(super) struct DufsJournal {
    configured_path: PathBuf,
    directory: super::SecureDirectory,
    document: JournalDocument,
}

impl DufsJournal {
    pub(super) fn prepare(
        maintenance: &MaintenanceLock,
        config: &ConfigAnchor,
        root: &RootAnchor,
        staged_target: &Path,
        backup: &VerifiedDufsSourceBackup,
        stage_moves: &[StageMove],
        target_identity: &SchemaIdentity,
    ) -> anyhow::Result<Self> {
        let configured_path = recovery_path(maintenance)?;
        ensure!(
            !configured_path.exists(),
            "an unfinished Dufs upgrade journal already exists; recover it explicitly"
        );
        fs::create_dir(&configured_path)?;
        fs::set_permissions(
            &configured_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )?;
        sync_directory(
            configured_path
                .parent()
                .context("Dufs recovery directory must have a parent")?,
        )?;
        let directory = super::SecureDirectory::open(&configured_path, "Dufs recovery directory")?;

        let target_path = directory.child_path(TARGET_DATABASE_FILE);
        create_private_empty_file(&target_path)?;
        copy_exact_regular(staged_target, &target_path)?;
        copy_ownership_and_mode(staged_target, &target_path)?;
        ensure!(
            verify_target_database(&target_path, root.identity())? == *target_identity,
            "Dufs recovery target identity changed while copied"
        );
        let (target_bytes, target_sha256) = hash_regular_file(&target_path)?;

        let original_placeholder = directory.child_path(ORIGINAL_DATABASE_FILE);
        write_blocker(&original_placeholder)?;
        let (blocker_bytes, blocker_sha256) = hash_regular_file(&original_placeholder)?;
        ensure!(
            blocker_bytes == BLOCKER_MAGIC.len() as u64,
            "internal Dufs blocker length mismatch"
        );

        let source_generation = capture_generation(maintenance)?;
        let backup_manifest_path = backup.directory.join(BACKUP_MANIFEST_FILE);
        let (manifest_bytes, manifest_sha256) = hash_regular_file(&backup_manifest_path)?;
        let document = JournalDocument {
            journal_version: JOURNAL_VERSION,
            product: Product::DufsRam,
            from_version: FROM_VERSION.to_owned(),
            to_version: TO_VERSION.to_owned(),
            phase: DufsJournalPhase::Prepared,
            database_path_base64: encode_path(
                maintenance
                    .location
                    .configured_database_path()
                    .as_os_str()
                    .as_bytes(),
            ),
            root_device: root.identity().device,
            root_inode: root.identity().inode,
            backup_path_base64: encode_path(backup.directory.as_os_str().as_bytes()),
            backup_manifest: DufsStoredResource {
                path: BACKUP_MANIFEST_FILE.to_owned(),
                bytes: manifest_bytes,
                sha256: manifest_sha256,
            },
            source_generation,
            target_database: DufsStoredResource {
                path: TARGET_DATABASE_FILE.to_owned(),
                bytes: target_bytes,
                sha256: target_sha256,
            },
            blocker_sha256,
            target_identity: target_identity.clone(),
            stage_moves: stage_moves.to_vec(),
        };
        let journal = Self {
            configured_path,
            directory,
            document,
        };
        if let Err(error) = journal
            .validate(maintenance, config, root)
            .and_then(|()| journal.persist())
        {
            let _ = fs::remove_dir_all(&journal.configured_path);
            let _ = sync_directory(
                journal
                    .configured_path
                    .parent()
                    .expect("recovery path has a parent"),
            );
            return Err(error);
        }
        Ok(journal)
    }

    pub(super) fn open(
        recovery: &Path,
        maintenance: &MaintenanceLock,
        config: &ConfigAnchor,
        root: &RootAnchor,
    ) -> anyhow::Result<Self> {
        let expected = recovery_path(maintenance)?;
        ensure!(
            crate::sqlite::absolute_path(recovery)? == expected,
            "Dufs recovery directory is not the exact adjacent journal path"
        );
        let directory = super::SecureDirectory::open(&expected, "Dufs recovery directory")?;
        let bytes = directory.read_bounded(JOURNAL_FILE, MAX_JOURNAL_BYTES)?;
        let document: JournalDocument =
            serde_json::from_slice(&bytes).context("parse exact Dufs recovery journal")?;
        let journal = Self {
            configured_path: expected,
            directory,
            document,
        };
        journal.validate(maintenance, config, root)?;
        Ok(journal)
    }

    pub(super) fn install(
        &mut self,
        maintenance: &MaintenanceLock,
        root: &RootAnchor,
        mut hook: impl FnMut(DufsJournalPhase) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        hook(DufsJournalPhase::Prepared)?;
        if let Err(error) = self.enter_barrier(maintenance) {
            return self.rollback_after_error(
                maintenance,
                root,
                error.context("enter Dufs non-SQLite upgrade barrier"),
            );
        }
        if let Err(error) = self.set_phase(DufsJournalPhase::Barrier) {
            return self.rollback_after_error(maintenance, root, error);
        }
        hook(DufsJournalPhase::Barrier)?;

        for movement in &self.document.stage_moves {
            if let Err(error) = root.move_stage(movement, true) {
                return self.rollback_after_error(maintenance, root, error);
            }
            hook(DufsJournalPhase::StageDirectoryMoved)?;
        }
        if let Err(error) = self.set_phase(DufsJournalPhase::TreeMoved) {
            return self.rollback_after_error(maintenance, root, error);
        }
        hook(DufsJournalPhase::TreeMoved)?;

        if let Err(error) = self.install_target(maintenance) {
            return self.rollback_after_error(maintenance, root, error);
        }
        if let Err(error) = self.set_phase(DufsJournalPhase::Installed) {
            return self.rollback_after_error(maintenance, root, error);
        }
        hook(DufsJournalPhase::Installed)?;

        if let Err(error) = self.verify_installed(maintenance, root) {
            return self.rollback_after_error(maintenance, root, error);
        }
        if let Err(error) = self.set_phase(DufsJournalPhase::Verified) {
            return self.rollback_after_error(maintenance, root, error);
        }
        hook(DufsJournalPhase::Verified)?;
        if let Err(error) = self.set_phase(DufsJournalPhase::Committed) {
            return self.rollback_after_error(maintenance, root, error);
        }
        hook(DufsJournalPhase::Committed)?;
        self.cleanup_committed()?;
        Ok(())
    }

    pub(super) fn recover_commit(
        mut self,
        maintenance: &MaintenanceLock,
        root: &RootAnchor,
    ) -> anyhow::Result<(PathBuf, SchemaIdentity, u64)> {
        let result = self.recover_commit_inner(maintenance, root);
        if result.is_err()
            && self.document.phase != DufsJournalPhase::Committed
            && let Err(barrier) = self.retain_recovery_barrier(maintenance, root)
        {
            let original = result.expect_err("checked recovery result");
            return Err(original.context(format!(
                "Dufs commit recovery failed and its barrier could not be re-established: {barrier:#}"
            )));
        }
        result
    }

    fn recover_commit_inner(
        &mut self,
        maintenance: &MaintenanceLock,
        root: &RootAnchor,
    ) -> anyhow::Result<(PathBuf, SchemaIdentity, u64)> {
        let database = maintenance.database_path();
        match self.classify_database(maintenance, root)? {
            DatabasePosition::Source => self.enter_barrier(maintenance)?,
            DatabasePosition::Blocker => self.finish_sidecar_barrier(maintenance)?,
            DatabasePosition::ExactTarget | DatabasePosition::CurrentTarget => {
                ensure!(
                    self.all_stages(root, StagePosition::New)?,
                    "current Dufs database has a mixed stage tree; refusing to overwrite it"
                );
                self.verify_installed(maintenance, root)?;
                self.set_phase(DufsJournalPhase::Committed)?;
                let backup = self.backup_path()?;
                let identity = verify_target_database(&database, root.identity())?;
                let moves = self.document.stage_moves.len() as u64;
                self.cleanup_committed()?;
                return Ok((backup, identity, moves));
            }
            DatabasePosition::Ambiguous => {
                anyhow::bail!("Dufs recovery cannot prove the canonical database generation")
            }
        }
        for movement in &self.document.stage_moves {
            match root.classify_stage(movement)? {
                StagePosition::Old => root.move_stage(movement, true)?,
                StagePosition::New => {}
                StagePosition::Ambiguous => {
                    anyhow::bail!("Dufs recovery found an ambiguous stage-directory generation")
                }
            }
        }
        self.set_phase(DufsJournalPhase::TreeMoved)?;
        self.install_target(maintenance)?;
        self.verify_installed(maintenance, root)?;
        self.set_phase(DufsJournalPhase::Committed)?;
        let backup = self.backup_path()?;
        let identity = verify_target_database(&database, root.identity())?;
        let moves = self.document.stage_moves.len() as u64;
        self.cleanup_committed()?;
        Ok((backup, identity, moves))
    }

    pub(super) fn recover_rollback(
        mut self,
        maintenance: &MaintenanceLock,
        root: &RootAnchor,
    ) -> anyhow::Result<(PathBuf, SchemaIdentity, u64)> {
        self.rollback(maintenance, root)?;
        self.set_phase(DufsJournalPhase::RolledBack)?;
        let identity = official_source_identity();
        let backup = self.backup_path()?;
        let moves = self.document.stage_moves.len() as u64;
        self.cleanup_rolled_back()?;
        Ok((backup, identity, moves))
    }

    fn validate(
        &self,
        maintenance: &MaintenanceLock,
        config: &ConfigAnchor,
        root: &RootAnchor,
    ) -> anyhow::Result<()> {
        ensure!(
            self.document.journal_version == JOURNAL_VERSION
                && self.document.product == Product::DufsRam
                && self.document.from_version == FROM_VERSION
                && self.document.to_version == TO_VERSION,
            "Dufs recovery journal adapter identity is not exact"
        );
        ensure!(
            self.document.database_path_base64
                == encode_path(
                    maintenance
                        .location
                        .configured_database_path()
                        .as_os_str()
                        .as_bytes()
                ),
            "Dufs recovery journal database path mismatch"
        );
        ensure!(
            RootIdentity {
                device: self.document.root_device,
                inode: self.document.root_inode
            } == root.identity(),
            "Dufs recovery journal root binding mismatch"
        );
        ensure!(
            self.document.target_identity
                == SchemaIdentity {
                    application: Product::DufsRam.slug().to_owned(),
                    application_version: TO_VERSION.to_owned(),
                    schema_revision: TARGET_REVISION,
                    schema_sha256: SCHEMA_SHA256.to_owned(),
                },
            "Dufs recovery target identity is not official"
        );
        ensure!(
            self.document
                .source_generation
                .first()
                .is_some_and(|entry| entry.suffix.is_empty())
                && self.document.source_generation.len() <= 4,
            "Dufs recovery source generation set is invalid"
        );
        let mut previous_sidecar = None;
        let (service_uid, service_gid) = root.owner();
        for (index, entry) in self.document.source_generation.iter().enumerate() {
            ensure!(
                entry.bytes <= MAX_DATABASE_GENERATION_ENTRY_BYTES
                    && entry.sha256.len() == 64
                    && entry
                        .sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    && entry.mode == 0o600
                    && entry.uid == service_uid
                    && entry.gid == service_gid,
                "Dufs recovery source generation metadata is invalid"
            );
            if index == 0 {
                continue;
            }
            let sidecar = SIDECARS
                .iter()
                .position(|suffix| *suffix == entry.suffix)
                .context("Dufs recovery source sidecar suffix is invalid")?;
            ensure!(
                previous_sidecar.is_none_or(|previous| sidecar > previous),
                "Dufs recovery source sidecar set is duplicated or out of order"
            );
            previous_sidecar = Some(sidecar);
        }
        self.find_blocker_path(maintenance)?;
        self.verify_backup(maintenance, config, root)?;
        Ok(())
    }

    fn verify_backup(
        &self,
        maintenance: &MaintenanceLock,
        config: &ConfigAnchor,
        root: &RootAnchor,
    ) -> anyhow::Result<()> {
        let backup = self.backup_path()?;
        let (bytes, sha256) = hash_regular_file(&backup.join(BACKUP_MANIFEST_FILE))?;
        ensure!(
            bytes == self.document.backup_manifest.bytes
                && sha256 == self.document.backup_manifest.sha256,
            "Dufs source backup manifest changed after journal preparation"
        );
        let verified = verify_dufs_source_backup_anchored(&backup, config, root)?;
        let state = maintenance
            .location
            .configured_database_path()
            .parent()
            .context("Dufs recovery database has no state directory")?;
        tree::validate_core_path_relationships(
            root.configured_path(),
            state,
            config.configured_path(),
        )?;
        for protected in [root.configured_path(), state, config.configured_path()] {
            ensure!(
                !verified.directory.starts_with(protected)
                    && !protected.starts_with(&verified.directory),
                "Dufs recovery backup overlaps a protected product path"
            );
        }
        let state_stat = fstat(&maintenance.location.parent)?;
        let database_stat = statat(
            &maintenance.location.parent,
            &maintenance.location.database_name,
            AtFlags::SYMLINK_NOFOLLOW,
        )?;
        let backup_anchor =
            SecureDirectory::open(&verified.directory, "Dufs recovery source backup")?;
        let backup_stat = fstat(&backup_anchor.file)?;
        let protected = [
            (
                "state directory",
                RootIdentity {
                    device: state_stat.st_dev,
                    inode: state_stat.st_ino,
                },
            ),
            (
                "database",
                RootIdentity {
                    device: database_stat.st_dev,
                    inode: database_stat.st_ino,
                },
            ),
            ("config", config.identity()),
            (
                "source backup",
                RootIdentity {
                    device: backup_stat.st_dev,
                    inode: backup_stat.st_ino,
                },
            ),
        ];
        let mut all_protected = vec![("shared root", root.identity())];
        all_protected.extend_from_slice(&protected);
        validate_distinct_protected_objects(&all_protected)?;
        root.ensure_no_protected_aliases(verified.manifest.tree_budget, &protected)?;
        ensure!(
            verified.manifest.raw_database_generation.len()
                == self.document.source_generation.len(),
            "Dufs journal and backup raw generation sets differ"
        );
        for entry in &self.document.source_generation {
            let path = format!(
                "{BACKUP_RAW_GENERATION_DIRECTORY}/database.sqlite3{}",
                entry.suffix
            );
            let stored = verified
                .manifest
                .raw_database_generation
                .iter()
                .find(|stored| stored.path == path)
                .context("Dufs journal sidecar is absent from the source backup")?;
            ensure!(
                stored.bytes == entry.bytes && stored.sha256 == entry.sha256,
                "Dufs journal source generation differs from its source backup"
            );
        }
        let owners = owner_mapping(&config.parsed.usernames)?;
        let expected =
            inspect_source_rows(&verified.directory.join(BACKUP_DATABASE_FILE), &owners)?;
        ensure!(
            expected.stage_moves.len() == self.document.stage_moves.len(),
            "Dufs recovery stage plan count differs from its source database"
        );
        for (actual, expected) in self.document.stage_moves.iter().zip(expected.stage_moves) {
            let mut logical = actual.clone();
            ensure!(
                logical.old_device.is_some() && logical.old_inode.is_some(),
                "Dufs recovery stage plan lacks its anchored source identity"
            );
            logical.old_device = None;
            logical.old_inode = None;
            ensure!(
                logical == expected,
                "Dufs recovery stage plan differs from its source database"
            );
            ensure!(
                root.classify_stage(actual)? != StagePosition::Ambiguous,
                "Dufs recovery stage plan does not match either exact tree generation"
            );
        }
        Ok(())
    }

    fn enter_barrier(&self, maintenance: &MaintenanceLock) -> anyhow::Result<()> {
        ensure!(
            matches!(
                self.classify_database_without_sqlite(maintenance)?,
                DatabasePosition::Source
            ),
            "Dufs canonical database is not the exact source before barrier"
        );
        ensure!(
            self.original_is_blocker()?,
            "Dufs original slot does not contain the exact blocker before barrier"
        );
        renameat_with(
            &maintenance.location.parent,
            &maintenance.location.database_name,
            &self.directory.file,
            ORIGINAL_DATABASE_FILE,
            RenameFlags::EXCHANGE,
        )?;
        maintenance.location.parent.sync_all()?;
        self.directory.file.sync_all()?;
        ensure!(
            self.canonical_is_blocker(maintenance)?,
            "Dufs canonical path is not fail-closed after barrier exchange"
        );
        self.finish_sidecar_barrier(maintenance)
    }

    fn finish_sidecar_barrier(&self, maintenance: &MaintenanceLock) -> anyhow::Result<()> {
        for sidecar in self.document.source_generation.iter().skip(1) {
            let mut source_name = maintenance.location.database_name.clone();
            source_name.push(&sidecar.suffix);
            let mut destination_name = OsString::from(ORIGINAL_DATABASE_FILE);
            destination_name.push(&sidecar.suffix);
            let source_exists = match statat(
                &maintenance.location.parent,
                &source_name,
                AtFlags::SYMLINK_NOFOLLOW,
            ) {
                Ok(_) => true,
                Err(Errno::NOENT) => false,
                Err(error) => {
                    return Err(std::io::Error::from(error))
                        .context("inspect canonical Dufs source sidecar");
                }
            };
            let destination_exists = match statat(
                &self.directory.file,
                &destination_name,
                AtFlags::SYMLINK_NOFOLLOW,
            ) {
                Ok(_) => true,
                Err(Errno::NOENT) => false,
                Err(error) => {
                    return Err(std::io::Error::from(error))
                        .context("inspect stored Dufs source sidecar");
                }
            };
            match (source_exists, destination_exists) {
                (true, false) => {
                    ensure!(
                        matches_stored(&maintenance.location.child_path(&source_name), sidecar)?,
                        "Dufs source sidecar changed before barrier"
                    );
                    renameat_with(
                        &maintenance.location.parent,
                        &source_name,
                        &self.directory.file,
                        &destination_name,
                        RenameFlags::NOREPLACE,
                    )?;
                    maintenance.location.parent.sync_all()?;
                    self.directory.file.sync_all()?;
                }
                (false, true) => ensure!(
                    matches_stored(&self.directory.child_path(&destination_name), sidecar)?,
                    "stored Dufs source sidecar changed during barrier"
                ),
                (true, true) => anyhow::bail!(
                    "Dufs source sidecar exists in both canonical and recovery generations"
                ),
                (false, false) => anyhow::bail!("Dufs source sidecar disappeared during barrier"),
            }
        }
        for suffix in SIDECARS {
            if self
                .document
                .source_generation
                .iter()
                .any(|entry| entry.suffix == suffix)
            {
                continue;
            }
            let mut canonical = maintenance.location.database_name.clone();
            canonical.push(suffix);
            let mut stored = OsString::from(ORIGINAL_DATABASE_FILE);
            stored.push(suffix);
            let canonical_absent = match statat(
                &maintenance.location.parent,
                &canonical,
                AtFlags::SYMLINK_NOFOLLOW,
            ) {
                Err(Errno::NOENT) => true,
                Ok(_) => false,
                Err(error) => {
                    return Err(std::io::Error::from(error))
                        .context("inspect unrecorded canonical Dufs sidecar");
                }
            };
            let stored_absent =
                match statat(&self.directory.file, &stored, AtFlags::SYMLINK_NOFOLLOW) {
                    Err(Errno::NOENT) => true,
                    Ok(_) => false,
                    Err(error) => {
                        return Err(std::io::Error::from(error))
                            .context("inspect unrecorded stored Dufs sidecar");
                    }
                };
            ensure!(
                canonical_absent && stored_absent,
                "an unrecorded Dufs sidecar appeared during barrier"
            );
        }
        Ok(())
    }

    fn install_target(&self, maintenance: &MaintenanceLock) -> anyhow::Result<()> {
        ensure!(
            self.canonical_is_blocker(maintenance)?,
            "Dufs canonical database lost its blocker before target install"
        );
        ensure!(
            self.canonical_sidecars_absent(maintenance)?,
            "Dufs canonical sidecars remain before target install"
        );
        let target = self.directory.child_path(TARGET_DATABASE_FILE);
        let (bytes, sha256) = hash_regular_file(&target)?;
        ensure!(
            bytes == self.document.target_database.bytes
                && sha256 == self.document.target_database.sha256,
            "Dufs staged target was tampered"
        );
        renameat_with(
            &maintenance.location.parent,
            &maintenance.location.database_name,
            &self.directory.file,
            TARGET_DATABASE_FILE,
            RenameFlags::EXCHANGE,
        )?;
        maintenance.location.parent.sync_all()?;
        self.directory.file.sync_all()?;
        Ok(())
    }

    fn verify_installed(
        &self,
        maintenance: &MaintenanceLock,
        root: &RootAnchor,
    ) -> anyhow::Result<()> {
        ensure!(
            self.all_stages(root, StagePosition::New)?,
            "not every Dufs stage directory is in the current namespace"
        );
        ensure!(
            self.canonical_sidecars_absent(maintenance)?,
            "unexpected Dufs sidecar after target install"
        );
        let identity = verify_target_database(&maintenance.database_path(), root.identity())?;
        ensure!(
            identity == self.document.target_identity,
            "installed Dufs target identity mismatch"
        );
        let metadata = fs::symlink_metadata(maintenance.database_path())?;
        let source = &self.document.source_generation[0];
        ensure!(
            metadata.mode() & 0o7777 == source.mode
                && metadata.uid() == source.uid
                && metadata.gid() == source.gid,
            "installed Dufs target ownership or mode differs from the source generation"
        );
        Ok(())
    }

    fn rollback_after_error(
        &mut self,
        maintenance: &MaintenanceLock,
        root: &RootAnchor,
        original: anyhow::Error,
    ) -> anyhow::Result<()> {
        match self.rollback(maintenance, root) {
            Ok(()) => {
                Err(original.context("Dufs upgrade failed; exact old generation was restored"))
            }
            Err(rollback) => Err(original.context(format!(
                "Dufs upgrade failed and rollback remains fail-closed: {rollback:#}"
            ))),
        }
    }

    fn retain_recovery_barrier(
        &self,
        maintenance: &MaintenanceLock,
        root: &RootAnchor,
    ) -> anyhow::Result<()> {
        match self.classify_database(maintenance, root)? {
            DatabasePosition::Blocker => Ok(()),
            DatabasePosition::Source => {
                if self.all_stages(root, StagePosition::Old)? {
                    return Ok(());
                }
                ensure!(
                    self.original_is_blocker()?,
                    "Dufs mixed source/tree generation has no exact blocker"
                );
                renameat_with(
                    &maintenance.location.parent,
                    &maintenance.location.database_name,
                    &self.directory.file,
                    ORIGINAL_DATABASE_FILE,
                    RenameFlags::EXCHANGE,
                )?;
                maintenance.location.parent.sync_all()?;
                self.directory.file.sync_all()?;
                ensure!(
                    self.canonical_is_blocker(maintenance)?,
                    "Dufs mixed source/tree generation could not be blocked"
                );
                Ok(())
            }
            DatabasePosition::CurrentTarget => {
                ensure!(
                    self.all_stages(root, StagePosition::New)?,
                    "a changed current Dufs database has a mixed stage tree"
                );
                Ok(())
            }
            DatabasePosition::ExactTarget | DatabasePosition::Ambiguous => {
                ensure!(
                    self.target_slot_is_blocker()?,
                    "Dufs recovery has no exact blocker available for fail-closed exchange"
                );
                renameat_with(
                    &maintenance.location.parent,
                    &maintenance.location.database_name,
                    &self.directory.file,
                    TARGET_DATABASE_FILE,
                    RenameFlags::EXCHANGE,
                )?;
                maintenance.location.parent.sync_all()?;
                self.directory.file.sync_all()?;
                ensure!(
                    self.canonical_is_blocker(maintenance)?,
                    "Dufs recovery failed to restore its canonical blocker"
                );
                Ok(())
            }
        }
    }

    fn rollback(&self, maintenance: &MaintenanceLock, root: &RootAnchor) -> anyhow::Result<()> {
        match self.classify_database(maintenance, root)? {
            DatabasePosition::ExactTarget => {
                ensure!(
                    self.target_slot_is_blocker()?,
                    "Dufs rollback has no exact blocker beside the installed target"
                );
                renameat_with(
                    &maintenance.location.parent,
                    &maintenance.location.database_name,
                    &self.directory.file,
                    TARGET_DATABASE_FILE,
                    RenameFlags::EXCHANGE,
                )?;
                maintenance.location.parent.sync_all()?;
                self.directory.file.sync_all()?;
            }
            DatabasePosition::CurrentTarget => anyhow::bail!(
                "Dufs current database changed after installation; rollback would destroy current writes, so only commit recovery is allowed"
            ),
            DatabasePosition::Blocker => {}
            DatabasePosition::Source => {
                if self.all_stages(root, StagePosition::Old)? {
                    return self.verify_source_generation(maintenance, root);
                }
                ensure!(
                    self.original_is_blocker()?,
                    "mixed Dufs source/tree generation has no exact blocker"
                );
                renameat_with(
                    &maintenance.location.parent,
                    &maintenance.location.database_name,
                    &self.directory.file,
                    ORIGINAL_DATABASE_FILE,
                    RenameFlags::EXCHANGE,
                )?;
                maintenance.location.parent.sync_all()?;
                self.directory.file.sync_all()?;
                ensure!(
                    self.canonical_is_blocker(maintenance)?,
                    "mixed Dufs source/tree generation could not be blocked"
                );
            }
            DatabasePosition::Ambiguous => anyhow::bail!(
                "Dufs canonical database generation is ambiguous; blocker is retained"
            ),
        }
        for movement in self.document.stage_moves.iter().rev() {
            match root.classify_stage(movement)? {
                StagePosition::New => root.move_stage(movement, false)?,
                StagePosition::Old => {}
                StagePosition::Ambiguous => {
                    anyhow::bail!("Dufs stage generation is ambiguous; blocker is retained")
                }
            }
        }
        self.restore_sidecars(maintenance)?;
        ensure!(
            self.original_is_source()?,
            "Dufs original main database does not match the recorded source"
        );
        ensure!(
            self.canonical_is_blocker(maintenance)?,
            "Dufs canonical path is not blocked before source restore"
        );
        renameat_with(
            &maintenance.location.parent,
            &maintenance.location.database_name,
            &self.directory.file,
            ORIGINAL_DATABASE_FILE,
            RenameFlags::EXCHANGE,
        )?;
        maintenance.location.parent.sync_all()?;
        self.directory.file.sync_all()?;
        self.verify_source_generation(maintenance, root)
    }

    fn restore_sidecars(&self, maintenance: &MaintenanceLock) -> anyhow::Result<()> {
        for sidecar in self.document.source_generation.iter().skip(1) {
            let mut canonical = maintenance.location.database_name.clone();
            canonical.push(&sidecar.suffix);
            let mut stored = OsString::from(ORIGINAL_DATABASE_FILE);
            stored.push(&sidecar.suffix);
            match statat(&self.directory.file, &stored, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(_) => {
                    match statat(
                        &maintenance.location.parent,
                        &canonical,
                        AtFlags::SYMLINK_NOFOLLOW,
                    ) {
                        Err(Errno::NOENT) => {}
                        Ok(_) => anyhow::bail!("unexpected canonical Dufs sidecar blocks rollback"),
                        Err(error) => {
                            return Err(std::io::Error::from(error))
                                .context("inspect canonical Dufs sidecar");
                        }
                    }
                    renameat_with(
                        &self.directory.file,
                        &stored,
                        &maintenance.location.parent,
                        &canonical,
                        RenameFlags::NOREPLACE,
                    )?;
                    self.directory.file.sync_all()?;
                    maintenance.location.parent.sync_all()?;
                }
                Err(Errno::NOENT) => {
                    let canonical_path = maintenance.location.child_path(&canonical);
                    ensure!(
                        matches_stored(&canonical_path, sidecar)?,
                        "recorded Dufs sidecar is missing during rollback"
                    );
                }
                Err(error) => {
                    return Err(std::io::Error::from(error)).context("inspect stored Dufs sidecar");
                }
            }
        }
        Ok(())
    }

    fn verify_source_generation(
        &self,
        maintenance: &MaintenanceLock,
        root: &RootAnchor,
    ) -> anyhow::Result<()> {
        for entry in &self.document.source_generation {
            let mut name = maintenance.location.database_name.clone();
            name.push(&entry.suffix);
            ensure!(
                matches_stored(&maintenance.location.child_path(&name), entry)?,
                "restored Dufs generation bytes differ from the recorded source"
            );
        }
        for suffix in SIDECARS {
            if !self
                .document
                .source_generation
                .iter()
                .any(|entry| entry.suffix == suffix)
            {
                let mut name = maintenance.location.database_name.clone();
                name.push(suffix);
                ensure!(
                    !maintenance.location.child_path(&name).exists(),
                    "unrecorded Dufs sidecar appeared during rollback"
                );
            }
        }
        ensure!(
            self.all_stages(root, StagePosition::Old)?,
            "restored Dufs source has a non-old stage namespace"
        );
        Ok(())
    }

    fn classify_database(
        &self,
        maintenance: &MaintenanceLock,
        root: &RootAnchor,
    ) -> anyhow::Result<DatabasePosition> {
        let simple = self.classify_database_without_sqlite(maintenance)?;
        if simple != DatabasePosition::Ambiguous {
            return Ok(simple);
        }
        if let Ok(identity) = verify_target_database(&maintenance.database_path(), root.identity())
            && identity == self.document.target_identity
        {
            return Ok(DatabasePosition::CurrentTarget);
        }
        Ok(DatabasePosition::Ambiguous)
    }

    fn classify_database_without_sqlite(
        &self,
        maintenance: &MaintenanceLock,
    ) -> anyhow::Result<DatabasePosition> {
        let path = maintenance.database_path();
        if matches_stored(&path, &self.document.source_generation[0])? {
            return Ok(DatabasePosition::Source);
        }
        let (bytes, sha256) = match hash_regular_file(&path) {
            Ok(value) => value,
            Err(_) => return Ok(DatabasePosition::Ambiguous),
        };
        if bytes == BLOCKER_MAGIC.len() as u64 && sha256 == self.document.blocker_sha256 {
            return Ok(DatabasePosition::Blocker);
        }
        if bytes == self.document.target_database.bytes
            && sha256 == self.document.target_database.sha256
        {
            return Ok(DatabasePosition::ExactTarget);
        }
        Ok(DatabasePosition::Ambiguous)
    }

    fn all_stages(&self, root: &RootAnchor, position: StagePosition) -> anyhow::Result<bool> {
        for movement in &self.document.stage_moves {
            if root.classify_stage(movement)? != position {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn canonical_is_blocker(&self, maintenance: &MaintenanceLock) -> anyhow::Result<bool> {
        Ok(matches!(
            self.classify_database_without_sqlite(maintenance)?,
            DatabasePosition::Blocker
        ))
    }

    fn original_is_blocker(&self) -> anyhow::Result<bool> {
        let (bytes, sha256) =
            hash_regular_file(&self.directory.child_path(ORIGINAL_DATABASE_FILE))?;
        Ok(bytes == BLOCKER_MAGIC.len() as u64 && sha256 == self.document.blocker_sha256)
    }

    fn target_slot_is_blocker(&self) -> anyhow::Result<bool> {
        let (bytes, sha256) = hash_regular_file(&self.directory.child_path(TARGET_DATABASE_FILE))?;
        Ok(bytes == BLOCKER_MAGIC.len() as u64 && sha256 == self.document.blocker_sha256)
    }

    fn original_is_source(&self) -> anyhow::Result<bool> {
        matches_stored(
            &self.directory.child_path(ORIGINAL_DATABASE_FILE),
            &self.document.source_generation[0],
        )
    }

    fn canonical_sidecars_absent(&self, maintenance: &MaintenanceLock) -> anyhow::Result<bool> {
        for suffix in SIDECARS {
            let mut name = maintenance.location.database_name.clone();
            name.push(suffix);
            match statat(
                &maintenance.location.parent,
                &name,
                AtFlags::SYMLINK_NOFOLLOW,
            ) {
                Ok(_) => return Ok(false),
                Err(Errno::NOENT) => {}
                Err(error) => {
                    return Err(std::io::Error::from(error))
                        .context("inspect canonical Dufs sidecar set");
                }
            }
        }
        Ok(true)
    }

    fn find_blocker_path(&self, maintenance: &MaintenanceLock) -> anyhow::Result<PathBuf> {
        let mut found = None;
        for path in [
            maintenance.database_path(),
            self.directory.child_path(ORIGINAL_DATABASE_FILE),
            self.directory.child_path(TARGET_DATABASE_FILE),
        ] {
            if let Ok((bytes, sha256)) = hash_regular_file(&path)
                && bytes == BLOCKER_MAGIC.len() as u64
                && sha256 == self.document.blocker_sha256
            {
                ensure!(
                    found.is_none(),
                    "Dufs recovery contains more than one exact blocker"
                );
                found = Some(path);
            }
        }
        found.context("Dufs recovery cannot locate its exact non-SQLite blocker")
    }

    fn set_phase(&mut self, phase: DufsJournalPhase) -> anyhow::Result<()> {
        self.document.phase = phase;
        self.persist()
    }

    fn persist(&self) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.document)?;
        ensure!(
            bytes.len() as u64 <= MAX_JOURNAL_BYTES,
            "Dufs recovery journal exceeds its size limit"
        );
        let temp = self.directory.child_path(JOURNAL_TEMP_FILE);
        match fs::remove_file(&temp) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("remove stale Dufs journal temp file"),
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        renameat_with(
            &self.directory.file,
            JOURNAL_TEMP_FILE,
            &self.directory.file,
            JOURNAL_FILE,
            RenameFlags::empty(),
        )?;
        self.directory.file.sync_all()?;
        Ok(())
    }

    fn backup_path(&self) -> anyhow::Result<PathBuf> {
        let raw = super::decode_path(&self.document.backup_path_base64)?;
        Ok(PathBuf::from(OsString::from_vec(raw)))
    }

    fn cleanup_committed(&self) -> anyhow::Result<()> {
        ensure!(
            matches!(self.document.phase, DufsJournalPhase::Committed),
            "Dufs journal is not committed"
        );
        fs::remove_dir_all(&self.configured_path)?;
        sync_directory(
            self.configured_path
                .parent()
                .context("Dufs recovery path has no parent")?,
        )
    }

    fn cleanup_rolled_back(&self) -> anyhow::Result<()> {
        ensure!(
            matches!(self.document.phase, DufsJournalPhase::RolledBack),
            "Dufs journal is not rolled back"
        );
        fs::remove_dir_all(&self.configured_path)?;
        sync_directory(
            self.configured_path
                .parent()
                .context("Dufs recovery path has no parent")?,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DatabasePosition {
    Source,
    Blocker,
    ExactTarget,
    CurrentTarget,
    Ambiguous,
}

fn official_source_identity() -> SchemaIdentity {
    SchemaIdentity {
        application: Product::DufsRam.slug().to_owned(),
        application_version: FROM_VERSION.to_owned(),
        schema_revision: SOURCE_USER_VERSION as u64,
        schema_sha256: SCHEMA_SHA256.to_owned(),
    }
}

fn recovery_path(maintenance: &MaintenanceLock) -> anyhow::Result<PathBuf> {
    let name = maintenance
        .location
        .database_name
        .to_str()
        .context("Dufs database filename must be UTF-8")?;
    Ok(maintenance
        .location
        .configured_database_path()
        .parent()
        .context("Dufs database must have a parent")?
        .join(format!(".{name}.dufs-ram.upgrade-recovery")))
}

fn capture_generation(maintenance: &MaintenanceLock) -> anyhow::Result<Vec<GenerationFile>> {
    let mut generation = Vec::new();
    for suffix in std::iter::once("").chain(SIDECARS) {
        let mut name = maintenance.location.database_name.clone();
        name.push(suffix);
        let path = maintenance.location.child_path(&name);
        match statat(
            &maintenance.location.parent,
            &name,
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Err(Errno::NOENT) if suffix.is_empty() => {
                anyhow::bail!("Dufs source database disappeared")
            }
            Err(Errno::NOENT) => continue,
            Err(error) => {
                return Err(std::io::Error::from(error)).context("inspect Dufs source generation");
            }
            Ok(stat) => ensure!(
                FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
                    && stat.st_nlink == 1,
                "Dufs source generation entry is not one regular file"
            ),
        }
        let (bytes, sha256) = hash_regular_file(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        generation.push(GenerationFile {
            suffix: suffix.to_owned(),
            bytes,
            sha256,
            mode: metadata.mode() & 0o7777,
            uid: metadata.uid(),
            gid: metadata.gid(),
        });
    }
    Ok(generation)
}

fn matches_stored(path: &Path, expected: &GenerationFile) -> anyhow::Result<bool> {
    match (hash_regular_file(path), fs::symlink_metadata(path)) {
        (Ok((bytes, sha256)), Ok(metadata)) => Ok(bytes == expected.bytes
            && sha256 == expected.sha256
            && metadata.mode() & 0o7777 == expected.mode
            && metadata.uid() == expected.uid
            && metadata.gid() == expected.gid),
        _ => Ok(false),
    }
}

fn write_blocker(path: &Path) -> anyhow::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(BLOCKER_MAGIC)?;
    file.sync_all()?;
    Ok(())
}

fn copy_exact_regular(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let mut source = File::open(source)?;
    let mut destination_file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(destination)?;
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        destination_file.write_all(&buffer[..read])?;
        digest.update(&buffer[..read]);
        bytes += read as u64;
    }
    destination_file.sync_all()?;
    drop(destination_file);
    let (stored_bytes, stored_sha256) = hash_regular_file(destination)?;
    ensure!(
        stored_bytes == bytes && stored_sha256 == lower_hex(&digest.finalize()),
        "Dufs exact file copy verification failed"
    );
    Ok(())
}

fn copy_ownership_and_mode(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let source = fs::symlink_metadata(source)?;
    let destination_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(destination)?;
    let destination_stat = fstat(&destination_file)?;
    if destination_stat.st_uid != source.uid() || destination_stat.st_gid != source.gid() {
        fchown(
            &destination_file,
            Some(Uid::from_raw(source.uid())),
            Some(Gid::from_raw(source.gid())),
        )?;
    }
    fs::set_permissions(
        destination,
        fs::Permissions::from_mode(source.mode() & 0o7777),
    )?;
    destination_file.sync_all()?;
    Ok(())
}

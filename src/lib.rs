//! Offline lifecycle tooling shared by operators, never by product runtimes.

pub mod catalog;
pub mod manifest;
pub mod sqlite;

pub use catalog::{Product, ProductContract, ResourceKind};
pub use manifest::{BackupManifest, ManifestError, ResourceEntry, SchemaIdentity};
pub use sqlite::{
    DufsCompositeBackupManifest, DufsRecoveryOptions, DufsStoredResource, DufsTreeBudget,
    DufsUpgradeOptions, DufsUpgradeResult, RecoveryAction, RecoveryResult, RestoreExisting,
    RestoreResult, SentinelCompanionContract, SentinelRecordingArchive, SentinelRecoveryOptions,
    SentinelSourceBackupManifest, SentinelStoredFile, SentinelUpgradeOptions,
    SentinelUpgradeResult, SqliteUpgradeResult, VerifiedDufsSourceBackup,
    VerifiedSentinelSourceBackup, VerifiedSourceBackup, VerifiedSqliteBackup, create_sqlite_backup,
    recover_dufs_upgrade, recover_sentinel_upgrade, recover_sqlite_restore, restore_sqlite_backup,
    schema_fingerprint, sentinel_credentials_key_from_file, upgrade_dufs, upgrade_sentinel,
    upgrade_sqlite, verify_dufs_source_backup, verify_sentinel_source_backup, verify_source_backup,
    verify_sqlite_backup,
};

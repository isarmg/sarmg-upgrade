//! Offline lifecycle tooling shared by operators, never by product runtimes.

pub mod catalog;
pub mod current;
pub mod manifest;
pub mod sqlite;
pub mod support;

pub use catalog::{Product, ProductContract, ResourceKind};
pub use current::{
    CompositeCurrentOptions, CurrentBackupManifest, CurrentRecoveryAction, CurrentRecoveryOptions,
    CurrentRestoreOptions, CurrentStateResult, NamedFile, backup_current, recover_current,
    restore_current, verify_current_backup,
};
pub use manifest::{
    BackupManifest, ExternalRequirement, ManifestError, ResourceEntry, SchemaIdentity,
};
pub use sqlite::{
    DufsCompositeBackupManifest, DufsCurrentBackupManifest, DufsCurrentOptions,
    DufsCurrentRestoreOptions, DufsRecoveryOptions, DufsStoredResource, DufsTreeBudget,
    DufsUpgradeOptions, DufsUpgradeResult, RecoveryAction, RecoveryResult, RestoreExisting,
    RestoreResult, SentinelCompanionContract, SentinelRecordingArchive, SentinelRecoveryOptions,
    SentinelSourceBackupManifest, SentinelStoredFile, SentinelUpgradeOptions,
    SentinelUpgradeResult, SqliteUpgradeResult, VerifiedDufsCurrentBackup,
    VerifiedDufsSourceBackup, VerifiedSentinelSourceBackup, VerifiedSourceBackup,
    VerifiedSqliteBackup, backup_dufs_current, create_sqlite_backup,
    create_sqlite_backup_with_credentials, recover_dufs_upgrade, recover_sentinel_upgrade,
    recover_sqlite_restore, restore_dufs_current, restore_sqlite_backup,
    restore_sqlite_backup_with_credentials, schema_fingerprint, sentinel_credentials_key_from_file,
    upgrade_dufs, upgrade_sentinel, upgrade_sqlite, verify_dufs_current_backup,
    verify_dufs_source_backup, verify_sentinel_source_backup, verify_source_backup,
    verify_sqlite_backup, verify_sqlite_backup_with_credentials,
};
pub use support::{SupportMatrix, support_matrix};

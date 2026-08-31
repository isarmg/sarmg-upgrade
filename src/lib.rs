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
    RecoveryAction, RecoveryResult, RestoreExisting, RestoreResult, VerifiedSqliteBackup,
    create_sqlite_backup, create_sqlite_backup_with_credentials, credentials_key_from_file,
    recover_sqlite_restore, restore_sqlite_backup, restore_sqlite_backup_with_credentials,
    schema_fingerprint, verify_sqlite_backup, verify_sqlite_backup_with_credentials,
};
pub use support::{SupportMatrix, support_matrix};

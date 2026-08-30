//! Offline lifecycle tooling shared by operators, never by product runtimes.

pub mod catalog;
pub mod manifest;
pub mod sqlite;

pub use catalog::{Product, ProductContract, ResourceKind};
pub use manifest::{BackupManifest, ManifestError, ResourceEntry, SchemaIdentity};
pub use sqlite::{
    RecoveryAction, RecoveryResult, RestoreExisting, RestoreResult, VerifiedSqliteBackup,
    create_sqlite_backup, recover_sqlite_restore, restore_sqlite_backup, schema_fingerprint,
    verify_sqlite_backup,
};

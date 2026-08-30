//! Offline lifecycle tooling shared by operators, never by product runtimes.

pub mod catalog;
pub mod manifest;

pub use catalog::{Product, ProductContract, ResourceKind};
pub use manifest::{BackupManifest, ManifestError, ResourceEntry};

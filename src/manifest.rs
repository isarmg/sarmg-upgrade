use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{Product, ResourceKind};

pub const MANIFEST_VERSION: u32 = 1;

/// The manifest is written last. Its presence declares a complete backup set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub manifest_version: u32,
    pub tool_version: String,
    pub product: Product,
    pub application_version: String,
    pub schema_identity: Option<SchemaIdentity>,
    pub created_at_epoch_seconds: u64,
    pub resources: Vec<ResourceEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaIdentity {
    pub application: String,
    pub application_version: String,
    pub schema_revision: u64,
    pub schema_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceEntry {
    pub name: String,
    pub kind: ResourceKind,
    pub path: PathBuf,
    pub bytes: u64,
    pub files: u64,
    pub sha256: String,
}

impl BackupManifest {
    pub fn read(path: &Path) -> Result<Self, ManifestError> {
        let bytes = fs::read(path).map_err(|source| ManifestError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let manifest = Self::from_slice(&bytes)?;
        Ok(manifest)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, ManifestError> {
        let manifest: Self = serde_json::from_slice(bytes)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.manifest_version != MANIFEST_VERSION {
            return Err(ManifestError::UnsupportedVersion(self.manifest_version));
        }
        require_identifier("tool_version", &self.tool_version)?;
        require_identifier("application_version", &self.application_version)?;
        if self.product.contract().has_runtime_state && self.resources.is_empty() {
            return Err(ManifestError::MissingResources(self.product));
        }
        if self
            .resources
            .iter()
            .any(|resource| resource.kind == ResourceKind::Sqlite)
        {
            let identity = self
                .schema_identity
                .as_ref()
                .ok_or(ManifestError::MissingSchemaIdentity)?;
            identity.validate(self.product)?;
        }

        let mut names = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut previous_name: Option<&str> = None;
        for resource in &self.resources {
            require_identifier("resource name", &resource.name)?;
            validate_relative_path(&resource.path)?;
            validate_sha256(&resource.sha256)?;
            if resource.files == 0 {
                return Err(ManifestError::EmptyResource(resource.name.clone()));
            }
            if !names.insert(resource.name.as_str()) {
                return Err(ManifestError::DuplicateResourceName(resource.name.clone()));
            }
            if !paths.insert(resource.path.as_path()) {
                return Err(ManifestError::DuplicateResourcePath(resource.path.clone()));
            }
            if previous_name.is_some_and(|previous| previous >= resource.name.as_str()) {
                return Err(ManifestError::ResourcesNotSorted);
            }
            previous_name = Some(&resource.name);
        }
        Ok(())
    }
}

impl SchemaIdentity {
    pub fn validate(&self, product: Product) -> Result<(), ManifestError> {
        require_identifier("schema application", &self.application)?;
        require_identifier("schema application_version", &self.application_version)?;
        validate_sha256(&self.schema_sha256)?;
        if self.application != product.slug() {
            return Err(ManifestError::ProductIdentityMismatch {
                product,
                application: self.application.clone(),
            });
        }
        Ok(())
    }
}

fn require_identifier(field: &'static str, value: &str) -> Result<(), ManifestError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
    {
        return Err(ManifestError::InvalidIdentifier {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<(), ManifestError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ManifestError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), ManifestError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ManifestError::InvalidSha256(value.to_owned()));
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("failed to read manifest {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("manifest JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("manifest version {0} is unsupported")]
    UnsupportedVersion(u32),
    #[error("{field} is not a bounded ASCII identifier: {value:?}")]
    InvalidIdentifier { field: &'static str, value: String },
    #[error("backup manifest for {0} contains no resources")]
    MissingResources(Product),
    #[error("a SQLite backup manifest must include schema_identity")]
    MissingSchemaIdentity,
    #[error("manifest product {product} does not match schema application {application:?}")]
    ProductIdentityMismatch {
        product: Product,
        application: String,
    },
    #[error("resource path is not a safe relative path: {0}")]
    UnsafePath(PathBuf),
    #[error("resource SHA-256 is not canonical lowercase hexadecimal: {0:?}")]
    InvalidSha256(String),
    #[error("resource {0:?} contains no files")]
    EmptyResource(String),
    #[error("resource name is duplicated: {0:?}")]
    DuplicateResourceName(String),
    #[error("resource path is duplicated: {0}")]
    DuplicateResourcePath(PathBuf),
    #[error("resources must be strictly sorted by name")]
    ResourcesNotSorted,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest() -> BackupManifest {
        BackupManifest {
            manifest_version: MANIFEST_VERSION,
            tool_version: "0.1.0".into(),
            product: Product::HostMonitoring,
            application_version: "0.7.0".into(),
            schema_identity: Some(SchemaIdentity {
                application: "host-monitoring".into(),
                application_version: "0.7.0".into(),
                schema_revision: 1,
                schema_sha256: "b".repeat(64),
            }),
            created_at_epoch_seconds: 1,
            resources: vec![ResourceEntry {
                name: "database".into(),
                kind: ResourceKind::Sqlite,
                path: "database.sqlite3".into(),
                bytes: 4096,
                files: 1,
                sha256: "a".repeat(64),
            }],
        }
    }

    #[test]
    fn accepts_canonical_manifest() {
        valid_manifest().validate().unwrap();
    }

    #[test]
    fn rejects_parent_path() {
        let mut manifest = valid_manifest();
        manifest.resources[0].path = "../database.sqlite3".into();
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::UnsafePath(_))
        ));
    }

    #[test]
    fn rejects_unknown_json_fields() {
        let mut value = serde_json::to_value(valid_manifest()).unwrap();
        value["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<BackupManifest>(value).is_err());
    }

    #[test]
    fn rejects_unsorted_or_duplicate_resources() {
        let mut manifest = valid_manifest();
        let mut second = manifest.resources[0].clone();
        second.name = "archive".into();
        second.path = "archive.bin".into();
        manifest.resources.push(second);
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::ResourcesNotSorted)
        ));
    }
}

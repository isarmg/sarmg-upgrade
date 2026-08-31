use std::{
    collections::BTreeSet,
    fs,
    ops::{Deref, DerefMut},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serialize};

use crate::{Product, ResourceKind};

use sarmg_contracts::{
    BACKUP_MANIFEST_VERSION, BackupManifest as ContractBackupManifest, ValidationError,
};
pub use sarmg_contracts::{
    BackupExternalRequirement as ExternalRequirement, BackupResource as ResourceEntry,
    SchemaIdentity,
};

pub const MANIFEST_VERSION: u8 = BACKUP_MANIFEST_VERSION;

/// Foundation 定义线上的通用备份清单；本包装只叠加本工具拥有的产品策略。
///
/// 这样 JSON 字段、数值边界和基础类型只有一个实现，同时仍由本仓库拒绝
/// 错产品、危险路径、重复资源和不符合当前产品要求的外部密钥声明。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct BackupManifest(ContractBackupManifest);

impl<'de> Deserialize<'de> for BackupManifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let manifest = Self(ContractBackupManifest::deserialize(deserializer)?);
        manifest.validate().map_err(serde::de::Error::custom)?;
        Ok(manifest)
    }
}

impl Deref for BackupManifest {
    type Target = ContractBackupManifest;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for BackupManifest {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl BackupManifest {
    pub fn new(contract: ContractBackupManifest) -> Result<Self, ManifestError> {
        let manifest = Self(contract);
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn into_contract(self) -> ContractBackupManifest {
        self.0
    }

    pub fn product(&self) -> Result<Product, ManifestError> {
        self.0
            .product
            .parse()
            .map_err(|_| ManifestError::UnsupportedProduct(self.0.product.clone()))
    }

    pub fn read(path: &Path) -> Result<Self, ManifestError> {
        let bytes = fs::read(path).map_err(|source| ManifestError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_slice(&bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, ManifestError> {
        // 先走 Foundation 的严格 parser；本类型的 Deserialize 随后叠加产品校验。
        Ok(serde_json::from_slice(bytes)?)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        self.0.validate()?;
        let product = self.product()?;

        if product.contract().has_runtime_state && self.resources.is_empty() {
            return Err(ManifestError::MissingResources(product));
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
            validate_schema_identity_for_product(identity, product)?;
            if identity.application_version != self.application_version {
                return Err(ManifestError::SchemaVersionMismatch {
                    manifest: self.application_version.clone(),
                    schema: identity.application_version.clone(),
                });
            }
        }
        if product.contract().requires_external_credentials_key {
            if self.external_requirements.len() != 1
                || self.external_requirements[0].kind != "credentials-key"
            {
                return Err(ManifestError::MissingExternalCredentialsKey(product));
            }
        } else if !self.external_requirements.is_empty() {
            return Err(ManifestError::UnexpectedExternalRequirements(product));
        }

        let mut names = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut previous_name: Option<&str> = None;
        for resource in &self.resources {
            let path = Path::new(&resource.path);
            validate_relative_path(path)?;
            if !names.insert(resource.name.as_str()) {
                return Err(ManifestError::DuplicateResourceName(resource.name.clone()));
            }
            if !paths.insert(resource.path.as_str()) {
                return Err(ManifestError::DuplicateResourcePath(path.to_path_buf()));
            }
            if previous_name.is_some_and(|previous| previous >= resource.name.as_str()) {
                return Err(ManifestError::ResourcesNotSorted);
            }
            previous_name = Some(&resource.name);
        }
        Ok(())
    }
}

pub(crate) fn validate_schema_identity_for_product(
    identity: &SchemaIdentity,
    product: Product,
) -> Result<(), ManifestError> {
    identity.validate()?;
    if identity.application != product.slug() {
        return Err(ManifestError::ProductIdentityMismatch {
            product,
            application: identity.application.clone(),
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

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("failed to read manifest {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("manifest JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("shared backup-manifest contract is invalid: {0}")]
    Contract(#[from] ValidationError),
    #[error("schema identity is invalid: {0}")]
    SchemaIdentity(#[from] sarmg_contracts::SchemaIdentityError),
    #[error("manifest names an unsupported product: {0:?}")]
    UnsupportedProduct(String),
    #[error("backup manifest for {0} contains no resources")]
    MissingResources(Product),
    #[error("a SQLite backup manifest must include schema_identity")]
    MissingSchemaIdentity,
    #[error("backup manifest for {0} lacks its exact external credentials-key requirement")]
    MissingExternalCredentialsKey(Product),
    #[error("backup manifest for {0} unexpectedly declares external requirements")]
    UnexpectedExternalRequirements(Product),
    #[error("manifest product {product} does not match schema application {application:?}")]
    ProductIdentityMismatch {
        product: Product,
        application: String,
    },
    #[error("manifest application version {manifest:?} does not match schema version {schema:?}")]
    SchemaVersionMismatch { manifest: String, schema: String },
    #[error("resource path is not a safe relative path: {0}")]
    UnsafePath(PathBuf),
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
        BackupManifest::new(ContractBackupManifest {
            manifest_version: MANIFEST_VERSION,
            tool_version: "0.2.0".into(),
            product: Product::HostMonitoring.slug().into(),
            application_version: "0.7.0".into(),
            schema_identity: Some(
                SchemaIdentity::new("host-monitoring", "0.7.0", 1, "b".repeat(64)).unwrap(),
            ),
            created_at_epoch_seconds: 1,
            external_requirements: Vec::new(),
            resources: vec![ResourceEntry {
                name: "database".into(),
                kind: ResourceKind::Sqlite,
                path: "database.sqlite3".into(),
                bytes: 4096,
                files: 1,
                sha256: "a".repeat(64),
            }],
        })
        .unwrap()
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

    #[test]
    fn rejects_schema_and_manifest_version_disagreement() {
        let mut manifest = valid_manifest();
        manifest.application_version = "0.7.1".into();
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::SchemaVersionMismatch { .. })
        ));
    }

    #[test]
    fn rejects_an_unknown_product_during_deserialization() {
        let mut value = serde_json::to_value(valid_manifest()).unwrap();
        value["product"] = serde_json::json!("unknown-product");
        assert!(serde_json::from_value::<BackupManifest>(value).is_err());
    }

    #[test]
    fn retains_shared_numeric_boundaries_after_product_wrapping() {
        let mut manifest = valid_manifest();
        manifest.resources[0].files = 0;
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::Contract(
                ValidationError::NonPositiveInteger { .. }
            ))
        ));

        let mut manifest = valid_manifest();
        manifest.created_at_epoch_seconds = sarmg_contracts::MAX_SAFE_JSON_INTEGER + 1;
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::Contract(
                ValidationError::UnsafeInteger { .. }
            ))
        ));
    }
}

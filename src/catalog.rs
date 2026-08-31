use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// A product whose persistent state is managed by this offline repository.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Product {
    MediaBackup,
    HostMonitoring,
    SunshineManager,
    SentinelMonitor,
    DufsRam,
    SarmgFoundation,
}

impl Product {
    pub const ALL: [Self; 6] = [
        Self::MediaBackup,
        Self::HostMonitoring,
        Self::SunshineManager,
        Self::SentinelMonitor,
        Self::DufsRam,
        Self::SarmgFoundation,
    ];

    pub const fn slug(self) -> &'static str {
        match self {
            Self::MediaBackup => "media-backup",
            Self::HostMonitoring => "host-monitoring",
            Self::SunshineManager => "sunshine-manager",
            Self::SentinelMonitor => "sentinel-monitor",
            Self::DufsRam => "dufs-ram",
            Self::SarmgFoundation => "sarmg-foundation",
        }
    }

    pub const fn contract(self) -> ProductContract {
        match self {
            Self::MediaBackup => ProductContract {
                product: self,
                resources: &[ResourceKind::Sqlite, ResourceKind::DataTree],
                has_runtime_state: true,
                requires_external_credentials_key: false,
            },
            Self::HostMonitoring => ProductContract {
                product: self,
                resources: &[ResourceKind::Sqlite],
                has_runtime_state: true,
                requires_external_credentials_key: false,
            },
            Self::SunshineManager => ProductContract {
                product: self,
                resources: &[ResourceKind::Sqlite],
                has_runtime_state: true,
                requires_external_credentials_key: true,
            },
            Self::SentinelMonitor => ProductContract {
                product: self,
                resources: &[
                    ResourceKind::Sqlite,
                    ResourceKind::Configuration,
                    ResourceKind::CompanionContract,
                    ResourceKind::Recordings,
                ],
                has_runtime_state: true,
                requires_external_credentials_key: true,
            },
            Self::DufsRam => ProductContract {
                product: self,
                resources: &[
                    ResourceKind::Sqlite,
                    ResourceKind::DataTree,
                    ResourceKind::Configuration,
                ],
                has_runtime_state: true,
                requires_external_credentials_key: false,
            },
            // Foundation is a library repository. It participates in source/API
            // upgrades, but has no service state to back up or restore.
            Self::SarmgFoundation => ProductContract {
                product: self,
                resources: &[],
                has_runtime_state: false,
                requires_external_credentials_key: false,
            },
        }
    }
}

impl fmt::Display for Product {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.slug())
    }
}

impl FromStr for Product {
    type Err = ParseProductError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|product| product.slug() == value)
            .ok_or_else(|| ParseProductError(value.to_owned()))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unsupported product {0:?}")]
pub struct ParseProductError(String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceKind {
    Sqlite,
    DataTree,
    Configuration,
    CompanionContract,
    Recordings,
}

#[derive(Clone, Copy, Debug)]
pub struct ProductContract {
    pub product: Product,
    pub resources: &'static [ResourceKind],
    pub has_runtime_state: bool,
    pub requires_external_credentials_key: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_names_round_trip() {
        for product in Product::ALL {
            assert_eq!(product.slug().parse::<Product>().unwrap(), product);
        }
    }

    #[test]
    fn foundation_has_no_runtime_backup_contract() {
        let contract = Product::SarmgFoundation.contract();
        assert!(!contract.has_runtime_state);
        assert!(contract.resources.is_empty());
        assert!(!contract.requires_external_credentials_key);
    }

    #[test]
    fn sentinel_declares_its_external_credentials_key_requirement() {
        assert!(
            Product::SentinelMonitor
                .contract()
                .requires_external_credentials_key
        );
    }

    #[test]
    fn sunshine_declares_its_external_credentials_key_requirement() {
        assert!(
            Product::SunshineManager
                .contract()
                .requires_external_credentials_key
        );
    }
}

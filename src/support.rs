use serde::Serialize;

use crate::{
    Product,
    current::{
        DUFS_CURRENT_APPLICATION_VERSION, MEDIA_CURRENT_APPLICATION_VERSION,
        SENTINEL_CURRENT_APPLICATION_VERSION,
    },
    sqlite::{HOST_CURRENT_APPLICATION_VERSION, SUNSHINE_CURRENT_APPLICATION_VERSION},
};

/// 唯一发布产物平台。该工具不是常驻 Server，但正式二进制与各 Server
/// 使用同一 Linux AMD64 交付边界。
pub const FORMAL_RELEASE_TARGET: &str = "x86_64-unknown-linux-gnu";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SupportMatrix {
    pub tool_version: &'static str,
    pub formal_release_target: &'static str,
    pub supported_capabilities: Vec<String>,
    pub products: Vec<ProductSupport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProductSupport {
    pub product: Product,
    pub current_state: CurrentStateSupport,
    pub upgrade_edges: Vec<UpgradeEdge>,
    pub external_requirements: Vec<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CurrentStateSupport {
    pub backup: Vec<&'static str>,
    pub verify: Vec<&'static str>,
    pub restore: Vec<&'static str>,
    pub recover: Vec<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UpgradeEdge {
    pub from: &'static str,
    pub to: &'static str,
}

impl CurrentStateSupport {
    fn exact(version: &'static str, recover: bool) -> Self {
        Self {
            backup: vec![version],
            verify: vec![version],
            restore: vec![version],
            recover: recover.then_some(version).into_iter().collect(),
        }
    }

    fn none() -> Self {
        Self {
            backup: Vec::new(),
            verify: Vec::new(),
            restore: Vec::new(),
            recover: Vec::new(),
        }
    }
}

/// Single authority for CLI help, release metadata, and machine-readable support output.
/// Every entry names only code that is present in this binary; no planned capability is listed.
pub fn support_matrix() -> SupportMatrix {
    let products = vec![
        ProductSupport {
            product: Product::MediaBackup,
            current_state: CurrentStateSupport::exact(MEDIA_CURRENT_APPLICATION_VERSION, true),
            upgrade_edges: Vec::new(),
            external_requirements: Vec::new(),
        },
        ProductSupport {
            product: Product::HostMonitoring,
            current_state: CurrentStateSupport::exact(HOST_CURRENT_APPLICATION_VERSION, true),
            upgrade_edges: Vec::new(),
            external_requirements: Vec::new(),
        },
        ProductSupport {
            product: Product::SunshineManager,
            current_state: CurrentStateSupport::exact(SUNSHINE_CURRENT_APPLICATION_VERSION, false),
            upgrade_edges: Vec::new(),
            external_requirements: vec!["credentials-key"],
        },
        ProductSupport {
            product: Product::SentinelMonitor,
            current_state: CurrentStateSupport::exact(SENTINEL_CURRENT_APPLICATION_VERSION, true),
            upgrade_edges: Vec::new(),
            external_requirements: vec!["credentials-key"],
        },
        ProductSupport {
            product: Product::DufsRam,
            current_state: CurrentStateSupport::exact(DUFS_CURRENT_APPLICATION_VERSION, true),
            upgrade_edges: Vec::new(),
            external_requirements: Vec::new(),
        },
        ProductSupport {
            product: Product::SarmgFoundation,
            current_state: CurrentStateSupport::none(),
            upgrade_edges: Vec::new(),
            external_requirements: Vec::new(),
        },
    ];
    let mut supported_capabilities = Vec::new();
    for product in &products {
        for (operation, versions) in [
            ("current-backup", &product.current_state.backup),
            ("current-verify", &product.current_state.verify),
            ("current-restore", &product.current_state.restore),
            ("current-recover", &product.current_state.recover),
        ] {
            supported_capabilities.extend(
                versions
                    .iter()
                    .map(|version| format!("{}-{operation}-{version}", product.product)),
            );
        }
        supported_capabilities.extend(
            product
                .upgrade_edges
                .iter()
                .map(|edge| format!("{}-upgrade-{}-to-{}", product.product, edge.from, edge.to)),
        );
    }
    supported_capabilities.sort();
    SupportMatrix {
        tool_version: env!("CARGO_PKG_VERSION"),
        formal_release_target: FORMAL_RELEASE_TARGET,
        supported_capabilities,
        products,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_product_appears_once_and_no_development_upgrade_edge_is_advertised() {
        let matrix = support_matrix();
        assert_eq!(matrix.formal_release_target, FORMAL_RELEASE_TARGET);
        assert_eq!(matrix.products.len(), Product::ALL.len());
        for product in Product::ALL {
            assert_eq!(
                matrix
                    .products
                    .iter()
                    .filter(|entry| entry.product == product)
                    .count(),
                1
            );
        }
        let sunshine = matrix
            .products
            .iter()
            .find(|entry| entry.product == Product::SunshineManager)
            .unwrap();
        assert_eq!(sunshine.external_requirements, ["credentials-key"]);
        assert!(
            matrix
                .products
                .iter()
                .all(|entry| entry.upgrade_edges.is_empty())
        );
        assert!(matrix.products.iter().all(|entry| {
            !entry.current_state.backup.is_empty()
                || (!entry.current_state.verify.is_empty()
                    || !entry.current_state.restore.is_empty()
                    || !entry.current_state.recover.is_empty())
                || entry.external_requirements.is_empty()
        }));
        assert!(
            matrix
                .supported_capabilities
                .contains(&"media-backup-current-restore-0.2.0".to_owned())
        );
    }
}

use serde::Serialize;

use crate::Product;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SupportMatrix {
    pub tool_version: &'static str,
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
            current_state: CurrentStateSupport::exact("0.2.0", true),
            upgrade_edges: Vec::new(),
            external_requirements: Vec::new(),
        },
        ProductSupport {
            product: Product::HostMonitoring,
            current_state: CurrentStateSupport::exact("0.7.0", true),
            upgrade_edges: Vec::new(),
            external_requirements: Vec::new(),
        },
        ProductSupport {
            product: Product::SunshineManager,
            current_state: CurrentStateSupport::exact("0.7.0", false),
            upgrade_edges: Vec::new(),
            external_requirements: vec!["credentials-key"],
        },
        ProductSupport {
            product: Product::SentinelMonitor,
            current_state: CurrentStateSupport::none(),
            upgrade_edges: Vec::new(),
            external_requirements: vec!["credentials-key"],
        },
        ProductSupport {
            product: Product::DufsRam,
            current_state: CurrentStateSupport::none(),
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
        assert!(
            matrix
                .supported_capabilities
                .contains(&"media-backup-current-restore-0.2.0".to_owned())
        );
    }
}

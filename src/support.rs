use crate::Product;
use serde::Serialize;

pub const FORMAL_RELEASE_TARGET: &str = "x86_64-unknown-linux-gnu";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SupportMatrix {
    pub tool_version: &'static str,
    pub source_revision: &'static str,
    pub compiled_target: &'static str,
    pub formal_release_target: &'static str,
    pub release_upgrade_protocol: &'static str,
    pub state_transition: &'static str,
    pub supported_capabilities: Vec<&'static str>,
    pub products: Vec<ProductSupport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProductSupport {
    pub product: Product,
    pub signed_release_upgrade: bool,
    pub durable_recovery: bool,
}

/// Products provide ordinary current-state diagnostics; signed deployment
/// authority belongs to this tool's controlled definitions.
/// No software version is inferred from a persistent format label.
pub fn support_matrix() -> SupportMatrix {
    SupportMatrix {
        tool_version: env!("CARGO_PKG_VERSION"),
        source_revision: env!("XSSC_COMPILED_SOURCE_REVISION"),
        compiled_target: env!("XSSC_COMPILED_TARGET"),
        formal_release_target: FORMAL_RELEASE_TARGET,
        release_upgrade_protocol: "signed-product-state-systemd-v1",
        state_transition: "current-to-future-same-contract",
        supported_capabilities: vec![
            "signed-same-schema-release-upgrade",
            "complete-immutable-release-root",
            "complete-program-configuration-state-backup",
            "durable-release-upgrade-recovery",
            "explicit-post-start-data-loss-authorization",
            "root-operator-service-uid-separation",
        ],
        products: Product::ALL
            .into_iter()
            .map(|product| {
                let runtime = product.contract().has_runtime_state;
                ProductSupport {
                    product,
                    signed_release_upgrade: runtime,
                    durable_recovery: runtime,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_names_six_servers_and_no_library_runtime() {
        let matrix = support_matrix();
        assert_eq!(matrix.products.len(), Product::ALL.len());
        for product in Product::ALL {
            let matching = matrix
                .products
                .iter()
                .filter(|entry| entry.product == product)
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1);
            let expected = product != Product::Xcss;
            assert_eq!(matching[0].signed_release_upgrade, expected);
            assert_eq!(matching[0].durable_recovery, expected);
        }
        assert_eq!(matrix.state_transition, "current-to-future-same-contract");
        assert!(
            matrix
                .supported_capabilities
                .iter()
                .all(|entry| !entry.contains("migration"))
        );
    }
}

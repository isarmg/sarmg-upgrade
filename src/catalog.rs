use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// A product whose persistent state is managed by this offline repository.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Product {
    Xszs,
    Xsos,
    Xscs,
    Xcos,
    Xczs,
    Xocs,
    #[serde(rename = "xcss")]
    Xcss,
}

impl Product {
    pub const ALL: [Self; 7] = [
        Self::Xszs,
        Self::Xsos,
        Self::Xscs,
        Self::Xcos,
        Self::Xczs,
        Self::Xocs,
        Self::Xcss,
    ];

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Xszs => "xszs",
            Self::Xsos => "xsos",
            Self::Xscs => "xscs",
            Self::Xcos => "xcos",
            Self::Xczs => "xczs",
            Self::Xocs => "xocs",
            Self::Xcss => "xcss",
        }
    }

    pub const fn contract(self) -> ProductContract {
        ProductContract {
            product: self,
            has_runtime_state: !matches!(self, Self::Xcss),
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

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ProductContract {
    pub product: Product,
    pub has_runtime_state: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_names_round_trip() {
        for product in Product::ALL {
            assert_eq!(product.slug().parse::<Product>().unwrap(), product);
            assert_eq!(serde_json::to_value(product).unwrap(), product.slug());
        }
    }

    #[test]
    fn foundation_is_a_library_and_all_six_servers_have_runtime_state() {
        for product in Product::ALL {
            assert_eq!(
                product.contract().has_runtime_state,
                product != Product::Xcss
            );
        }
    }
}

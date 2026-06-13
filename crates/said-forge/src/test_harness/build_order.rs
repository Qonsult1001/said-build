//! Entity build order. The Dev Spec README declares the dependency
//! order in its "Build Order (Top-Down Approach)" section. We hard-
//! code the canonical sequence; if the README is in a different
//! location or a custom order is required, the operator can override
//! by writing `dtcard/.forge/test-build-order.toml`.

use std::path::Path;

/// Default TXN build order, per
/// `dt/Spec/Dev Planning/OVERVIEW.md` "Build Order".
/// Identity + Health are excluded — they don't follow the standard
/// CRUD lifecycle (Identity is OAuth flows; Health is a single GET).
pub const DEFAULT_TXN_ORDER: &[&str] = &[
    "BinSponsor",
    "Bin",
    "ProgramManager",  // must come before BinRange — BinRange body
                       // references {{programManagerId}}
    "BinRange",
    "Product",
    "Business",
    "Cardholder",
    "Account",
    "Card",
    "Pin",
    "SpendControl",
    "MerchantControl",
    "MerchantControlGroup",
    "SpendOverride",
    "Transaction",
    "DigitalWalletToken",
    "Alert",
    "Fee",
    "Webhook",
    "DelegatedApprovalSource",
    "DelegatedApprovalStandIn",
];

/// Try `<workspace>/.forge/test-build-order.toml` first, fall back to
/// the default order. Toml shape:
///
/// ```toml
/// order = ["BinSponsor", "Bin", ...]
/// ```
pub fn load_or_default(workspace_root: &Path) -> Vec<String> {
    let custom = workspace_root.join(".forge").join("test-build-order.toml");
    if custom.exists() {
        if let Ok(text) = std::fs::read_to_string(&custom) {
            if let Ok(t) = toml::from_str::<toml::Table>(&text) {
                if let Some(arr) = t.get("order").and_then(|v| v.as_array()) {
                    let order: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    if !order.is_empty() {
                        return order;
                    }
                }
            }
        }
    }
    DEFAULT_TXN_ORDER.iter().map(|s| s.to_string()).collect()
}

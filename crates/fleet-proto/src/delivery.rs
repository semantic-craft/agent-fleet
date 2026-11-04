use serde::{Deserialize, Serialize};

/// An observed outcome, not an ordered ladder: completion does not prove UI visibility.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeliveryTier {
    Accepted,
    ThreadVisible,
    Running,
    Completed,
    Failed,
    Unknown,
}

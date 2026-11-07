use serde::{Deserialize, Serialize};
use std::fmt;

/// An observed outcome, not an ordered ladder: completion does not prove UI visibility.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeliveryTier {
    Accepted,
    ThreadVisible,
    Running,
    Completed,
    Failed,
    Unknown,
}

impl DeliveryTier {
    pub const ALL: [Self; 6] = [
        Self::Accepted,
        Self::ThreadVisible,
        Self::Running,
        Self::Completed,
        Self::Failed,
        Self::Unknown,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::ThreadVisible => "thread-visible",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "accepted" => Some(Self::Accepted),
            "thread-visible" | "thread_visible" => Some(Self::ThreadVisible),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    pub const fn proves_sidebar_visibility(self) -> bool {
        matches!(self, Self::ThreadVisible)
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }

    pub const fn lattice_satisfies(self, claimed: Self) -> bool {
        match (self, claimed) {
            (a, b) if a as u8 == b as u8 => true,
            (Self::ThreadVisible | Self::Running | Self::Completed, Self::Accepted) => true,
            _ => false,
        }
    }
}

impl fmt::Display for DeliveryTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

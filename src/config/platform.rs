//! Supported streaming platforms.

/// Identifiers for platforms that DYTL can query and record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Douyin,
    Kuaishou,
    Twitter,
}

impl Platform {
    /// Parse a platform name from config or CLI text.
    ///
    /// Accepts short aliases: `dy`, `ks`, `x`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "douyin" | "dy" => Some(Self::Douyin),
            "kuaishou" | "ks" => Some(Self::Kuaishou),
            "twitter" | "x" => Some(Self::Twitter),
            _ => None,
        }
    }

    /// Stable machine-readable name used in paths and process args.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Douyin => "douyin",
            Self::Kuaishou => "kuaishou",
            Self::Twitter => "twitter",
        }
    }

    /// Human-readable label for logs and UI text.
    pub fn label(self) -> &'static str {
        match self {
            Self::Douyin => "抖音",
            Self::Kuaishou => "快手",
            Self::Twitter => "Twitter/X",
        }
    }
}

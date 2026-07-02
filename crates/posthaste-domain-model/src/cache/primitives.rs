use super::*;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheScoringWeights {
    pub recency: f64,
    pub thread_activity: f64,
    pub sender_affinity: f64,
    pub explicit_importance: f64,
    pub search_context: f64,
    pub local_behavior: f64,
    pub size_alpha: f64,
}

impl Default for CacheScoringWeights {
    fn default() -> Self {
        Self {
            recency: 0.35,
            thread_activity: 0.20,
            sender_affinity: 0.15,
            explicit_importance: 0.10,
            search_context: 0.10,
            local_behavior: 0.10,
            size_alpha: DEFAULT_SIZE_ALPHA,
        }
    }
}

/// Optional cache layer scored independently for the same message.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheLayer {
    Body,
    RawMessage,
    AttachmentBlob,
}

impl CacheLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::RawMessage => "raw_message",
            Self::AttachmentBlob => "attachment_blob",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "body" => Some(Self::Body),
            "raw_message" => Some(Self::RawMessage),
            "attachment_blob" => Some(Self::AttachmentBlob),
            _ => None,
        }
    }
}

/// Download/storage unit needed to satisfy a cache candidate.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheFetchUnit {
    BodyOnly,
    RawMessage,
    AttachmentBlob,
}

impl CacheFetchUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BodyOnly => "body_only",
            Self::RawMessage => "raw_message",
            Self::AttachmentBlob => "attachment_blob",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "body_only" => Some(Self::BodyOnly),
            "raw_message" => Some(Self::RawMessage),
            "attachment_blob" => Some(Self::AttachmentBlob),
            _ => None,
        }
    }
}

/// Persisted state for a cache candidate/fetch object.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheObjectState {
    Wanted,
    Fetching,
    Cached,
    Failed,
    Evicted,
}

impl CacheObjectState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wanted => "wanted",
            Self::Fetching => "fetching",
            Self::Cached => "cached",
            Self::Failed => "failed",
            Self::Evicted => "evicted",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "wanted" => Some(Self::Wanted),
            "fetching" => Some(Self::Fetching),
            "cached" => Some(Self::Cached),
            "failed" => Some(Self::Failed),
            "evicted" => Some(Self::Evicted),
            _ => None,
        }
    }
}

/// Global cache budget and layer eligibility.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CachePolicy {
    pub soft_cap_bytes: u64,
    pub hard_cap_bytes: u64,
    pub cache_bodies: bool,
    pub cache_raw_messages: bool,
    pub cache_attachments: bool,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            soft_cap_bytes: 1024 * 1024 * 1024,
            hard_cap_bytes: 2 * 1024 * 1024 * 1024,
            cache_bodies: true,
            cache_raw_messages: false,
            cache_attachments: false,
        }
    }
}

impl CachePolicy {
    pub fn budget(self, used_bytes: u64, interactive_pressure: f64) -> CacheBudget {
        CacheBudget {
            used_bytes,
            soft_cap_bytes: self.soft_cap_bytes,
            hard_cap_bytes: self.hard_cap_bytes.max(self.soft_cap_bytes),
            interactive_pressure,
        }
    }
}

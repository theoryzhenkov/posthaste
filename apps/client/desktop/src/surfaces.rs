//! Surface descriptors and their window plumbing: the wire shapes the
//! frontend posts to `open_surface_window` (mirroring
//! `apps/client/frontend/src/surfaces/types.ts`), the route each surface
//! loads (mirroring `surfaces/serialize.ts`), and the per-surface window
//! label, title, and size.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum SurfaceDescriptor {
    #[serde(rename = "attachment")]
    Attachment {
        disposition: SurfaceDisposition,
        params: AttachmentSurfaceParams,
    },
    #[serde(rename = "message")]
    Message {
        disposition: SurfaceDisposition,
        params: MessageSurfaceParams,
    },
    #[serde(rename = "settings")]
    Settings {
        disposition: SurfaceDisposition,
        params: SettingsSurfaceParams,
    },
    #[serde(rename = "compose")]
    Compose {
        disposition: SurfaceDisposition,
        params: ComposeSurfaceParams,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SurfaceDisposition {
    #[serde(rename = "focused")]
    Focused,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AttachmentSurfaceParams {
    pub(crate) source_id: String,
    pub(crate) message_id: String,
    pub(crate) attachment_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MessageSurfaceParams {
    pub(crate) conversation_id: String,
    pub(crate) source_id: String,
    pub(crate) message_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SettingsSurfaceParams {
    pub(crate) category: Option<SettingsSurfaceCategory>,
    pub(crate) target: Option<SettingsSurfaceTarget>,
}

/// Mirrors `SETTINGS_SURFACE_CATEGORIES` in the frontend's surface types.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SettingsSurfaceCategory {
    General,
    Appearance,
    Accounts,
    Outbox,
    Mailboxes,
    Automations,
    Tags,
    Storage,
    Notifications,
    Troubleshooting,
}

impl SettingsSurfaceCategory {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Accounts => "accounts",
            Self::Outbox => "outbox",
            Self::Mailboxes => "mailboxes",
            Self::Automations => "automations",
            Self::Tags => "tags",
            Self::Storage => "storage",
            Self::Notifications => "notifications",
            Self::Troubleshooting => "troubleshooting",
        }
    }
}

/// Mirrors `ComposeIntent` in `apps/client/frontend/src/composeIntent.ts`.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum ComposeSurfaceParams {
    #[serde(rename = "new")]
    New {
        #[serde(rename = "sourceId")]
        source_id: String,
    },
    #[serde(rename = "reply")]
    Reply {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
    },
    #[serde(rename = "replyAll")]
    ReplyAll {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
    },
    #[serde(rename = "forward")]
    Forward {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
    },
    #[serde(rename = "draft")]
    Draft {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
    },
    #[serde(rename = "mailto")]
    Mailto {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "mailtoUri")]
        mailto_uri: String,
    },
}

impl ComposeSurfaceParams {
    fn kind_str(&self) -> &'static str {
        match self {
            Self::New { .. } => "new",
            Self::Reply { .. } => "reply",
            Self::ReplyAll { .. } => "replyAll",
            Self::Forward { .. } => "forward",
            Self::Draft { .. } => "draft",
            Self::Mailto { .. } => "mailto",
        }
    }

    fn source_id(&self) -> &str {
        match self {
            Self::New { source_id }
            | Self::Reply { source_id, .. }
            | Self::ReplyAll { source_id, .. }
            | Self::Forward { source_id, .. }
            | Self::Draft { source_id, .. }
            | Self::Mailto { source_id, .. } => source_id,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum SettingsSurfaceTarget {
    #[serde(rename = "account")]
    Account {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    #[serde(rename = "newAccount")]
    NewAccount,
    #[serde(rename = "smartMailbox")]
    SmartMailbox {
        #[serde(rename = "smartMailboxId")]
        smart_mailbox_id: String,
    },
    #[serde(rename = "newSmartMailbox")]
    NewSmartMailbox,
    #[serde(rename = "sourceMailbox")]
    SourceMailbox {
        #[serde(rename = "sourceAccountId")]
        source_account_id: String,
        #[serde(rename = "sourceMailboxId")]
        source_mailbox_id: String,
    },
}

/// The hash route a surface loads, mirroring `surfaceRoute` in the
/// frontend's `surfaces/serialize.ts` (same paths, same query keys, same
/// pair order) so a Rust-built route parses identically on the other side.
pub(crate) fn surface_route(surface: &SurfaceDescriptor) -> String {
    match surface {
        SurfaceDescriptor::Attachment {
            disposition,
            params,
        } => {
            let _ = disposition;
            format!(
                "/surface/attachment?sourceId={}&messageId={}&attachmentId={}",
                encode_component(&params.source_id),
                encode_component(&params.message_id),
                encode_component(&params.attachment_id)
            )
        }
        SurfaceDescriptor::Message {
            disposition,
            params,
        } => {
            let _ = disposition;
            format!(
                "/surface/message?conversationId={}&sourceId={}&messageId={}",
                encode_component(&params.conversation_id),
                encode_component(&params.source_id),
                encode_component(&params.message_id)
            )
        }
        SurfaceDescriptor::Settings {
            disposition,
            params,
        } => {
            let _ = disposition;
            let mut pairs = Vec::new();
            push_query_pair(
                &mut pairs,
                "category",
                params
                    .category
                    .as_ref()
                    .map(SettingsSurfaceCategory::as_str),
            );
            push_settings_target_query_pairs(&mut pairs, params.target.as_ref());
            if pairs.is_empty() {
                "/surface/settings".to_string()
            } else {
                format!("/surface/settings?{}", pairs.join("&"))
            }
        }
        SurfaceDescriptor::Compose {
            disposition,
            params,
        } => {
            let _ = disposition;
            let mut pairs = Vec::new();
            push_compose_query_pairs(&mut pairs, params);
            format!("/surface/compose?{}", pairs.join("&"))
        }
    }
}

/// The URL a standalone surface window loads. Always the one real bundled
/// document (`index.html`) with the surface route — query params included —
/// carried in the URL FRAGMENT, so every window on every platform loads the
/// same file and never depends on SPA path-fallback behavior in the asset
/// protocol. The client reads the route (and its query) from `location.hash`.
pub(crate) fn surface_window_url(surface: &SurfaceDescriptor) -> String {
    format!("index.html#{}", surface_route(surface))
}

pub(crate) fn surface_window_navigation_script(route: &str) -> String {
    let route_json = serde_json::to_string(route).expect("surface route should serialize to JSON");
    format!(
        "(() => {{ const route = {route_json}; window.history.replaceState(window.history.state, '', '#' + route); window.dispatchEvent(new HashChangeEvent('hashchange')); }})();"
    )
}

pub(crate) fn validate_surface_descriptor(surface: &SurfaceDescriptor) -> Result<(), String> {
    if let SurfaceDescriptor::Settings { params, .. } = surface {
        if let (Some(category), Some(target)) = (&params.category, &params.target) {
            let target_category = settings_target_category(target);
            if *category != target_category {
                return Err("settings surface category does not match target kind".to_string());
            }
        }
    }
    Ok(())
}

pub(crate) fn settings_target_category(target: &SettingsSurfaceTarget) -> SettingsSurfaceCategory {
    match target {
        SettingsSurfaceTarget::Account { .. } | SettingsSurfaceTarget::NewAccount => {
            SettingsSurfaceCategory::Accounts
        }
        SettingsSurfaceTarget::SmartMailbox { .. }
        | SettingsSurfaceTarget::NewSmartMailbox
        | SettingsSurfaceTarget::SourceMailbox { .. } => SettingsSurfaceCategory::Mailboxes,
    }
}

/// A stable per-surface window label: reopening the same message, attachment,
/// or compose intent focuses the existing window instead of stacking a new
/// one; settings is a singleton.
pub(crate) fn surface_window_label(surface: &SurfaceDescriptor) -> String {
    match surface {
        SurfaceDescriptor::Attachment { params, .. } => {
            let key = format!(
                "{}:{}:{}",
                params.source_id, params.message_id, params.attachment_id
            );
            format!("attachment-{:016x}", stable_hash(key.as_bytes()))
        }
        SurfaceDescriptor::Settings { .. } => "settings".to_string(),
        SurfaceDescriptor::Compose { .. } => {
            format!(
                "compose-{:016x}",
                stable_hash(surface_route(surface).as_bytes())
            )
        }
        SurfaceDescriptor::Message { params, .. } => {
            let key = format!("{}:{}", params.source_id, params.message_id);
            format!("message-{:016x}", stable_hash(key.as_bytes()))
        }
    }
}

pub(crate) fn surface_title(surface: &SurfaceDescriptor) -> &'static str {
    match surface {
        SurfaceDescriptor::Attachment { .. } => "Posthaste Attachment",
        SurfaceDescriptor::Settings { .. } => "Posthaste Settings",
        SurfaceDescriptor::Message { .. } => "Posthaste Message",
        SurfaceDescriptor::Compose { .. } => "Posthaste Compose",
    }
}

/// Window sizes, kept in sync with `surfaceWindowPolicy.ts` popup sizes so
/// the desktop and browser variants of a surface open at the same geometry.
pub(crate) fn surface_window_size(surface: &SurfaceDescriptor) -> (f64, f64) {
    match surface {
        SurfaceDescriptor::Attachment { .. } => (1100.0, 820.0),
        SurfaceDescriptor::Settings { .. } => (980.0, 720.0),
        SurfaceDescriptor::Message { .. } => (900.0, 760.0),
        SurfaceDescriptor::Compose { .. } => (780.0, 640.0),
    }
}

fn push_query_pair(pairs: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        pairs.push(format!("{key}={}", encode_component(value)));
    }
}

fn push_compose_query_pairs(pairs: &mut Vec<String>, params: &ComposeSurfaceParams) {
    push_query_pair(pairs, "composeKind", Some(params.kind_str()));
    push_query_pair(pairs, "sourceId", Some(params.source_id()));
    match params {
        ComposeSurfaceParams::New { .. } => {}
        ComposeSurfaceParams::Reply { message_id, .. }
        | ComposeSurfaceParams::ReplyAll { message_id, .. }
        | ComposeSurfaceParams::Forward { message_id, .. }
        | ComposeSurfaceParams::Draft { message_id, .. } => {
            push_query_pair(pairs, "messageId", Some(message_id));
        }
        ComposeSurfaceParams::Mailto { mailto_uri, .. } => {
            push_query_pair(pairs, "mailtoUri", Some(mailto_uri));
        }
    }
}

fn push_settings_target_query_pairs(
    pairs: &mut Vec<String>,
    target: Option<&SettingsSurfaceTarget>,
) {
    let Some(target) = target else {
        return;
    };

    match target {
        SettingsSurfaceTarget::Account { account_id } => {
            push_query_pair(pairs, "targetKind", Some("account"));
            push_query_pair(pairs, "accountId", Some(account_id));
        }
        SettingsSurfaceTarget::NewAccount => {
            push_query_pair(pairs, "targetKind", Some("newAccount"));
        }
        SettingsSurfaceTarget::SmartMailbox { smart_mailbox_id } => {
            push_query_pair(pairs, "targetKind", Some("smartMailbox"));
            push_query_pair(pairs, "smartMailboxId", Some(smart_mailbox_id));
        }
        SettingsSurfaceTarget::NewSmartMailbox => {
            push_query_pair(pairs, "targetKind", Some("newSmartMailbox"));
        }
        SettingsSurfaceTarget::SourceMailbox {
            source_account_id,
            source_mailbox_id,
        } => {
            push_query_pair(pairs, "targetKind", Some("sourceMailbox"));
            push_query_pair(pairs, "sourceAccountId", Some(source_account_id));
            push_query_pair(pairs, "sourceMailboxId", Some(source_mailbox_id));
        }
    }
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

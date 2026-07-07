use super::*;

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
/// protocol. The client reads the route (and its query) from `location.hash`
/// (see `apps/web/src/surfaces/location.ts`).
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

pub(crate) fn surface_window_size(surface: &SurfaceDescriptor) -> (f64, f64) {
    match surface {
        SurfaceDescriptor::Attachment { .. } => (1100.0, 820.0),
        SurfaceDescriptor::Settings { .. } => (980.0, 720.0),
        SurfaceDescriptor::Message { .. } => (900.0, 760.0),
        SurfaceDescriptor::Compose { .. } => (780.0, 640.0),
    }
}

pub(crate) fn push_query_pair(pairs: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        pairs.push(format!("{key}={}", encode_component(value)));
    }
}

pub(crate) fn push_compose_query_pairs(pairs: &mut Vec<String>, params: &ComposeSurfaceParams) {
    match params {
        ComposeSurfaceParams::New { source_id } => {
            push_query_pair(pairs, "composeKind", Some("new"));
            push_query_pair(pairs, "sourceId", Some(source_id));
        }
        ComposeSurfaceParams::Reply {
            source_id,
            message_id,
        } => {
            push_query_pair(pairs, "composeKind", Some("reply"));
            push_query_pair(pairs, "sourceId", Some(source_id));
            push_query_pair(pairs, "messageId", Some(message_id));
        }
        ComposeSurfaceParams::Forward {
            source_id,
            message_id,
        } => {
            push_query_pair(pairs, "composeKind", Some("forward"));
            push_query_pair(pairs, "sourceId", Some(source_id));
            push_query_pair(pairs, "messageId", Some(message_id));
        }
    }
}

pub(crate) fn push_settings_target_query_pairs(
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

pub(crate) fn encode_component(value: &str) -> String {
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

pub(crate) fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

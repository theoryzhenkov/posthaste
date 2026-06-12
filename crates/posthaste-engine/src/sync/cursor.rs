use serde_json::json;

const EMAIL_CURSOR_KIND: &str = "jmap-email";
const EMAIL_METADATA_VERSION: u64 = 2;

pub(super) fn non_empty_state(state: &str) -> Option<&str> {
    (!state.is_empty()).then_some(state)
}

pub(crate) fn encode_email_cursor_state(server_state: &str) -> String {
    json!({
        "kind": EMAIL_CURSOR_KIND,
        "metadataVersion": EMAIL_METADATA_VERSION,
        "state": server_state,
    })
    .to_string()
}

pub(crate) fn decode_email_cursor_state(state: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(state).ok()?;
    let kind = value.get("kind")?.as_str()?;
    let metadata_version = value.get("metadataVersion")?.as_u64()?;
    if kind != EMAIL_CURSOR_KIND || metadata_version != EMAIL_METADATA_VERSION {
        return None;
    }
    value
        .get("state")?
        .as_str()
        .and_then(non_empty_state)
        .map(String::from)
}

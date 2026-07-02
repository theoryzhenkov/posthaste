use axum::extract::Request;
use time::OffsetDateTime;

use crate::authz::{CaveatContext, ResourceShape, RouteAuthz, ScopeMode};

/// Build the [`CaveatContext`] for a matched, authorized-pending request. For
/// `Gate` routes the resource axes come from path params (matched against the
/// route template); for `Filter` routes they come from the query string. An
/// axis the route does not populate stays `None`, so a caveat restricting it is
/// unsatisfiable and the request is denied — the fail-closed rule.
pub(crate) fn build_context(req: &Request, template: &str, authz: &RouteAuthz) -> CaveatContext {
    // `template` is nest-stripped (no `/v1`); strip the same prefix from the
    // concrete path so segment matching lines up.
    let raw_path = req.uri().path();
    let path = raw_path.strip_prefix("/v1").unwrap_or(raw_path);
    let (account, mailbox, message) = match authz.mode {
        ScopeMode::Gate => extract_path_axes(template, path, &authz.resource),
        ScopeMode::Filter => extract_query_axes(req.uri().query(), &authz.resource),
    };
    CaveatContext {
        action: authz.action,
        account,
        mailbox,
        message,
        now: OffsetDateTime::now_utc(),
    }
}

/// Resolve the `(account, mailbox, message)` axes from path params by matching
/// the request path against the route template segment-by-segment.
fn extract_path_axes(
    template: &str,
    path: &str,
    shape: &ResourceShape,
) -> (Option<String>, Option<String>, Option<String>) {
    let params = path_params(template, path);
    let pick = |name: Option<&str>| name.and_then(|n| params.get(n).cloned());
    (
        pick(shape.account),
        pick(shape.mailbox),
        pick(shape.message),
    )
}

/// Match a route template (`/sources/{source_id}/messages/{message_id}`) against
/// a concrete path, returning the captured `{param}` values. Returns an empty
/// map on any segment-count mismatch (the templates always match here, since the
/// router selected this template).
fn path_params(template: &str, path: &str) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();
    let template_segments: Vec<&str> = template.trim_matches('/').split('/').collect();
    let path_segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    if template_segments.len() != path_segments.len() {
        return params;
    }
    for (tpl, actual) in template_segments.iter().zip(path_segments.iter()) {
        if let Some(name) = tpl.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            // Path segments are percent-encoded on the wire; decode so caveat
            // comparison is against the logical id.
            let decoded = percent_decode(actual);
            params.insert(name.to_string(), decoded);
        }
    }
    params
}

/// Resolve the `(account, mailbox, message)` axes from query params for a
/// `Filter` route. A missing filter leaves the axis `None`, so a resource caveat
/// restricting it is unsatisfiable → the request is denied (the matching-filter
/// rule). `message` is never a query filter in this API.
fn extract_query_axes(
    query: Option<&str>,
    shape: &ResourceShape,
) -> (Option<String>, Option<String>, Option<String>) {
    let pick = |name: Option<&str>| name.and_then(|n| query_param(query, n));
    (
        pick(shape.account),
        pick(shape.mailbox),
        pick(shape.message),
    )
}

/// Read a single query parameter's (percent-decoded) value. Fails CLOSED on a
/// DUPLICATE key: if the query string carries `name` more than once, returns
/// `None` (so the resource caveat is unsatisfiable → deny) rather than taking
/// the first occurrence. This prevents any middleware-vs-handler disagreement on
/// `?sourceId=a&sourceId=b` (the middleware must never authorize a value the
/// handler might not use).
fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    let query = query?;
    let mut found: Option<String> = None;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == name {
            if found.is_some() {
                // Duplicate key — ambiguous; fail closed.
                return None;
            }
            found = Some(percent_decode(value));
        }
    }
    found
}

/// Minimal percent-decoder for path/query segment values. Decodes `%XX` escapes
/// and `+` (in query position both `+` and `%20` mean space). On any malformed
/// escape the original byte is preserved.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

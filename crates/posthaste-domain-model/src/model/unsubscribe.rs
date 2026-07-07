use serde::{Deserialize, Serialize};

/// Parsed `List-Unsubscribe` targets for a message (RFC 2369), plus the
/// RFC 8058 one-click marker derived from the companion
/// `List-Unsubscribe-Post: List-Unsubscribe=One-Click` header.
///
/// Stored as JSON in the message row and surfaced on the message detail DTO so
/// the client can offer an Unsubscribe affordance. Targets are validated at
/// parse time (conservatively — see [`parse_list_unsubscribe`]); the server
/// re-validates `https` before performing the one-click POST.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ListUnsubscribe {
    /// First valid `https:` target from the header, if any. Guaranteed by the
    /// parser to be an ASCII https URL with no userinfo and a non-IP-literal
    /// host. `http:` targets are dropped, never downgraded-to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub https: Option<String>,
    /// First valid `mailto:` target, kept as the full URI (query params such as
    /// `subject=` are needed to prefill the composer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mailto: Option<String>,
    /// True when the message carries `List-Unsubscribe-Post:
    /// List-Unsubscribe=One-Click` (RFC 8058) *and* an https target exists —
    /// the https URL may then be POSTed server-side without user navigation.
    #[serde(default)]
    pub one_click: bool,
}

/// Parses the `List-Unsubscribe` header (RFC 2369) with its optional
/// `List-Unsubscribe-Post` companion (RFC 8058).
///
/// The header is a comma-separated list of `<URI>` entries, possibly folded
/// across lines and interleaved with comments; folding whitespace *inside* the
/// angle brackets is removed per RFC 2369 ("the URL line is continued").
/// The first valid https and the first valid mailto target are kept.
///
/// Returns `None` when no valid target survives validation (malformed header,
/// http-only, rejected URLs, ...) — absence of the affordance, not an error.
pub fn parse_list_unsubscribe(header: &str, post_header: Option<&str>) -> Option<ListUnsubscribe> {
    let mut https = None;
    let mut mailto = None;

    let mut rest = header;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else { break };
        // RFC 2369: a URI may be folded across lines inside the brackets —
        // strip all whitespace (incl. CR/LF from unfolding) within the entry.
        let uri: String = after[..end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if https.is_none() && validate_one_click_https(&uri).is_ok() {
            https = Some(uri);
        } else if mailto.is_none() && is_valid_mailto(&uri) {
            mailto = Some(uri);
        }
        if https.is_some() && mailto.is_some() {
            break;
        }
        rest = &after[end + 1..];
    }

    if https.is_none() && mailto.is_none() {
        return None;
    }

    // RFC 8058 §3.1: the value is exactly `List-Unsubscribe=One-Click`.
    // One-click is only meaningful with an https target (§3.2).
    let one_click = https.is_some()
        && post_header.is_some_and(|v| {
            let unfolded: String = v.chars().filter(|c| *c != '\r' && *c != '\n').collect();
            unfolded
                .trim()
                .eq_ignore_ascii_case("List-Unsubscribe=One-Click")
        });

    Some(ListUnsubscribe {
        https,
        mailto,
        one_click,
    })
}

/// Why an https unsubscribe target was rejected. Diagnostic only — callers
/// treat any rejection as "no target".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsubscribeUrlError {
    /// Not an `https:` URL (includes `http:` — never downgraded-to).
    NotHttps,
    /// Contains bytes outside printable ASCII (IDN senders must use punycode).
    NonAscii,
    /// The authority contains userinfo (`user@host` games).
    HasUserinfo,
    /// The host is an IP literal (IPv4 or `[...]`) — rejected conservatively.
    IpLiteralHost,
    /// Empty/otherwise malformed authority (bad port, invalid host chars).
    MalformedAuthority,
}

/// Conservative validation of an https one-click unsubscribe target.
///
/// Dependency-free by design (this crate is the wasm-facing leaf): scheme must
/// be `https`, the URL must be printable ASCII, the authority must carry no
/// userinfo, the host must be a plausible DNS name (letters/digits/`-`/`.`
/// labels, not an IP literal, not digits-and-dots), and any port must be
/// numeric. The server re-validates with a full URL parser before POSTing —
/// this check only has to be *at least* as strict as "safe to store".
pub fn validate_one_click_https(uri: &str) -> Result<(), UnsubscribeUrlError> {
    if uri.len() < 9 || !uri[..8].eq_ignore_ascii_case("https://") {
        return Err(UnsubscribeUrlError::NotHttps);
    }
    if uri.bytes().any(|b| !(0x21..=0x7e).contains(&b)) {
        return Err(UnsubscribeUrlError::NonAscii);
    }
    let rest = &uri[8..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.contains('@') {
        return Err(UnsubscribeUrlError::HasUserinfo);
    }
    if authority.contains('[') || authority.contains(']') {
        return Err(UnsubscribeUrlError::IpLiteralHost);
    }
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    if let Some(port) = port {
        if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) || port.len() > 5 {
            return Err(UnsubscribeUrlError::MalformedAuthority);
        }
    }
    if host.is_empty()
        || host.len() > 253
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        || host.starts_with(['-', '.'])
        || host.ends_with('-')
        || host.contains("..")
    {
        return Err(UnsubscribeUrlError::MalformedAuthority);
    }
    // Digits-and-dots hosts are IPv4(-ish) literals — reject conservatively.
    if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(UnsubscribeUrlError::IpLiteralHost);
    }
    Ok(())
}

/// Minimal validation of a `mailto:` unsubscribe target (RFC 6068): at least
/// one address with a non-empty local part and domain before any `?` query.
fn is_valid_mailto(uri: &str) -> bool {
    if uri.len() <= 7 || !uri[..7].eq_ignore_ascii_case("mailto:") {
        return false;
    }
    if uri.bytes().any(|b| !(0x21..=0x7e).contains(&b)) {
        return false;
    }
    let to = &uri[7..uri.find('?').unwrap_or(uri.len())];
    // Every comma-separated address must look like local@domain.
    !to.is_empty()
        && to.split(',').all(|addr| {
            matches!(addr.split_once('@'), Some((local, domain))
                if !local.is_empty() && !domain.is_empty() && !domain.contains('@'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- RFC 2369 §3.2 examples ---

    #[test]
    fn rfc2369_single_mailto() {
        let parsed =
            parse_list_unsubscribe("<mailto:list@host.com?subject=unsubscribe>", None).unwrap();
        assert_eq!(
            parsed.mailto.as_deref(),
            Some("mailto:list@host.com?subject=unsubscribe")
        );
        assert_eq!(parsed.https, None);
        assert!(!parsed.one_click);
    }

    #[test]
    fn rfc2369_comment_and_two_mailtos_takes_first() {
        let parsed = parse_list_unsubscribe(
            "(Use this command to get off the list)\n <mailto:list-manager@host.com?body=unsubscribe%20list>,\n <mailto:list-off@host.com>",
            None,
        )
        .unwrap();
        assert_eq!(
            parsed.mailto.as_deref(),
            Some("mailto:list-manager@host.com?body=unsubscribe%20list")
        );
    }

    #[test]
    fn rfc2369_http_url_is_dropped_mailto_kept() {
        // The RFC example uses http:// — plain-http targets are dropped, never
        // stored or downgraded-to; the mailto fallback survives.
        let parsed = parse_list_unsubscribe(
            "<http://www.host.com/list.cgi?cmd=unsub&lst=list>,\n <mailto:list-request@host.com?subject=unsubscribe>",
            None,
        )
        .unwrap();
        assert_eq!(parsed.https, None);
        assert_eq!(
            parsed.mailto.as_deref(),
            Some("mailto:list-request@host.com?subject=unsubscribe")
        );
        assert!(!parsed.one_click);
    }

    // --- RFC 8058 ---

    #[test]
    fn rfc8058_one_click() {
        let parsed = parse_list_unsubscribe(
            "<https://example.com/unsubscribe/opaquepart>",
            Some("List-Unsubscribe=One-Click"),
        )
        .unwrap();
        assert_eq!(
            parsed.https.as_deref(),
            Some("https://example.com/unsubscribe/opaquepart")
        );
        assert!(parsed.one_click);
    }

    #[test]
    fn rfc8058_one_click_case_insensitive_and_folded() {
        let parsed = parse_list_unsubscribe(
            "<https://example.com/u/1>",
            Some("  list-unsubscribe=one-click \r\n"),
        )
        .unwrap();
        assert!(parsed.one_click);
    }

    #[test]
    fn post_header_with_other_value_is_not_one_click() {
        let parsed = parse_list_unsubscribe(
            "<https://example.com/u/1>",
            Some("List-Unsubscribe=Two-Click"),
        )
        .unwrap();
        assert!(!parsed.one_click);
    }

    #[test]
    fn one_click_requires_https_target() {
        // Post header present but only a mailto target: not one-click.
        let parsed = parse_list_unsubscribe(
            "<mailto:unsub@example.com>",
            Some("List-Unsubscribe=One-Click"),
        )
        .unwrap();
        assert!(!parsed.one_click);
    }

    #[test]
    fn both_https_and_mailto_kept() {
        let parsed = parse_list_unsubscribe(
            "<https://example.com/unsub/123>, <mailto:unsub@example.com?subject=stop>",
            Some("List-Unsubscribe=One-Click"),
        )
        .unwrap();
        assert_eq!(
            parsed.https.as_deref(),
            Some("https://example.com/unsub/123")
        );
        assert_eq!(
            parsed.mailto.as_deref(),
            Some("mailto:unsub@example.com?subject=stop")
        );
        assert!(parsed.one_click);
    }

    #[test]
    fn folded_url_inside_brackets_is_unfolded() {
        let parsed =
            parse_list_unsubscribe("<https://example.com/unsub\r\n /long/token>", None).unwrap();
        assert_eq!(
            parsed.https.as_deref(),
            Some("https://example.com/unsub/long/token")
        );
    }

    // --- malformed → None ---

    #[test]
    fn malformed_headers_yield_none() {
        for header in [
            "",
            "unsubscribe here",
            "<>",
            "<https://example.com/unsub", // unterminated
            "<ftp://example.com/unsub>",
            "<http://example.com/unsub>", // http-only: dropped, nothing left
            "<mailto:>",
            "<mailto:no-at-sign>",
            "<mailto:trailing@>",
            "<mailto:@nodomain.com>",
        ] {
            assert_eq!(
                parse_list_unsubscribe(header, None),
                None,
                "header: {header:?}"
            );
        }
    }

    // --- https validation (security posture) ---

    #[test]
    fn https_validation_rejects_games() {
        use UnsubscribeUrlError::*;
        for (url, err) in [
            ("http://example.com/u", NotHttps),
            ("https://user:pass@example.com/u", HasUserinfo),
            ("https://example.com@evil.com/u", HasUserinfo),
            ("https://127.0.0.1/u", IpLiteralHost),
            ("https://[::1]/u", IpLiteralHost),
            ("https://8.8.8.8:8443/u", IpLiteralHost),
            ("https:///u", MalformedAuthority),
            ("https://exa mple.com/u", NonAscii),
            ("https://exämple.com/u", NonAscii),
            ("https://example.com:notaport/u", MalformedAuthority),
            ("https://.example.com/u", MalformedAuthority),
            ("https://ex..ample.com/u", MalformedAuthority),
        ] {
            assert_eq!(validate_one_click_https(url), Err(err), "url: {url}");
        }
    }

    #[test]
    fn https_validation_accepts_normal_urls() {
        for url in [
            "https://example.com/unsub?u=1&t=abc",
            "https://mail.lists.example.co.uk:8443/unsubscribe/opaque",
            "https://example.com",
        ] {
            assert_eq!(validate_one_click_https(url), Ok(()), "url: {url}");
        }
    }

    #[test]
    fn serde_shape_is_camel_case() {
        let parsed = ListUnsubscribe {
            https: Some("https://example.com/u".into()),
            mailto: None,
            one_click: true,
        };
        let json = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, r#"{"https":"https://example.com/u","oneClick":true}"#);
        let round: ListUnsubscribe = serde_json::from_str(&json).unwrap();
        assert_eq!(round, parsed);
    }
}

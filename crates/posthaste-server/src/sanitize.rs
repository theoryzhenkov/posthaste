use std::collections::{HashMap, HashSet};

use ammonia::Builder;

/// Sanitize raw email HTML for safe rendering in an iframe.
///
/// Allows email-safe tags/attributes, forces safe link behavior,
/// and strips tracking pixels.
///
/// @spec docs/L1-api#message-body-sanitization
pub fn sanitize_email_html(raw_html: &str) -> String {
    let tags: HashSet<&str> = [
        "a",
        "b",
        "blockquote",
        "br",
        "caption",
        "center",
        "code",
        "col",
        "colgroup",
        "dd",
        "del",
        "div",
        "dl",
        "dt",
        "em",
        "font",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "i",
        "img",
        "ins",
        "li",
        "ol",
        "p",
        "pre",
        "s",
        "small",
        "span",
        "strong",
        "sub",
        "sup",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "tr",
        "u",
        "ul",
    ]
    .into_iter()
    .collect();

    let mut tag_attributes: HashMap<&str, HashSet<&str>> = HashMap::new();

    // Global attributes applied to all tags
    let global_attrs: HashSet<&str> = ["class", "dir", "style"].into_iter().collect();
    for &tag in &tags {
        tag_attributes.insert(tag, global_attrs.clone());
    }

    // Per-tag attributes (merged with globals)
    let per_tag: &[(&str, &[&str])] = &[
        ("a", &["href", "title", "target"]),
        ("img", &["src", "alt", "width", "height"]),
        (
            "td",
            &[
                "colspan", "rowspan", "align", "valign", "width", "height", "bgcolor",
            ],
        ),
        (
            "th",
            &[
                "colspan", "rowspan", "align", "valign", "width", "height", "bgcolor",
            ],
        ),
        (
            "table",
            &[
                "border",
                "cellpadding",
                "cellspacing",
                "width",
                "bgcolor",
                "align",
            ],
        ),
        ("font", &["color", "face", "size"]),
        ("div", &["align"]),
        ("p", &["align"]),
        ("span", &["align"]),
        ("col", &["span", "width"]),
        ("colgroup", &["span", "width"]),
    ];

    for &(tag, attrs) in per_tag {
        let entry = tag_attributes
            .entry(tag)
            .or_insert_with(|| global_attrs.clone());
        for &attr in attrs {
            entry.insert(attr);
        }
    }

    let url_schemes: HashSet<&str> = ["https", "http", "mailto", "cid"].into_iter().collect();

    let sanitized = Builder::default()
        .tags(tags)
        .tag_attributes(tag_attributes)
        .url_schemes(url_schemes)
        .link_rel(Some("noopener noreferrer"))
        .add_tag_attributes("a", ["target"])
        .clean(raw_html)
        .to_string();

    strip_tracking_pixels(&sanitized)
}

/// Remove 1x1 tracking pixel images and images with disallowed src schemes.
fn strip_tracking_pixels(html: &str) -> String {
    // Simple approach: parse <img ...> tags and filter them.
    // We use a basic state machine rather than regex to avoid the regex dep.
    let mut result = String::with_capacity(html.len());
    let mut remaining = html;

    while let Some(start) = find_img_tag_start(remaining) {
        result.push_str(&remaining[..start]);

        // Find the end of this tag
        let tag_start = &remaining[start..];
        let end = tag_start
            .find('>')
            .map(|i| i + 1)
            .unwrap_or(tag_start.len());
        let tag = &tag_start[..end];

        if should_keep_img(tag) {
            result.push_str(tag);
        }

        remaining = &remaining[start + end..];
    }

    result.push_str(remaining);
    result
}

/// Find the byte offset of the next `<img` tag start (case-insensitive).
fn find_img_tag_start(html: &str) -> Option<usize> {
    let bytes = html.as_bytes();
    let last_start = bytes.len().saturating_sub(4);
    for index in 0..last_start {
        let candidate = &bytes[index..index + 4];
        if candidate.eq_ignore_ascii_case(b"<img") {
            let next = bytes.get(index + 4).copied();
            if matches!(
                next,
                Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
            ) {
                return Some(index);
            }
        }
    }
    None
}

/// Determine if an <img> tag should be kept.
///
/// Strip if:
/// - no src attribute (ammonia already removed a disallowed scheme)
/// - src is not `cid:` or `https://`
/// - width=1 AND height=1 (tracking pixel)
fn should_keep_img(tag: &str) -> bool {
    let tag_lower = tag.to_ascii_lowercase();

    // Check src scheme — missing src means ammonia stripped a disallowed scheme
    match extract_attr(&tag_lower, "src") {
        None => return false,
        Some(src) if !src.starts_with("cid:") && !src.starts_with("https://") => return false,
        _ => {}
    }

    // Check for 1x1 tracking pixel
    let width = extract_attr(&tag_lower, "width");
    let height = extract_attr(&tag_lower, "height");
    if let (Some(w), Some(h)) = (width, height) {
        let w = w.trim().trim_matches('"').trim_matches('\'');
        let h = h.trim().trim_matches('"').trim_matches('\'');
        if w == "1" && h == "1" {
            return false;
        }
    }

    true
}

/// Extract an attribute value from an HTML tag string (lowercase).
fn extract_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let search = format!("{attr}=\"");
    if let Some(pos) = tag.find(&search) {
        let start = pos + search.len();
        let rest = &tag[start..];
        let end = rest.find('"').unwrap_or(rest.len());
        return Some(&rest[..end]);
    }

    // Try single quotes
    let search = format!("{attr}='");
    if let Some(pos) = tag.find(&search) {
        let start = pos + search.len();
        let rest = &tag[start..];
        let end = rest.find('\'').unwrap_or(rest.len());
        return Some(&rest[..end]);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_tags() {
        let input = "<p>Hello</p><script>alert('xss')</script>";
        let result = sanitize_email_html(input);
        assert!(!result.contains("<script>"));
        assert!(result.contains("<p>Hello</p>"));
    }

    #[test]
    fn allows_basic_formatting() {
        let input = "<b>bold</b> <i>italic</i> <a href=\"https://example.com\">link</a>";
        let result = sanitize_email_html(input);
        assert!(result.contains("<b>bold</b>"));
        assert!(result.contains("<i>italic</i>"));
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn strips_tracking_pixel() {
        let input =
            r#"<p>Hello</p><img src="https://track.example.com/pixel.gif" width="1" height="1">"#;
        let result = sanitize_email_html(input);
        assert!(!result.contains("<img"));
    }

    #[test]
    fn strips_data_uri_img() {
        let input = r#"<img src="data:image/png;base64,AAAA">"#;
        let result = sanitize_email_html(input);
        assert!(!result.contains("<img"));
    }

    #[test]
    fn keeps_cid_img() {
        let input = r#"<img src="cid:image001@example.com" alt="photo">"#;
        let result = sanitize_email_html(input);
        assert!(result.contains("cid:image001@example.com"));
    }

    #[test]
    fn keeps_https_img_non_pixel() {
        let input =
            r#"<img src="https://example.com/photo.jpg" alt="photo" width="200" height="150">"#;
        let result = sanitize_email_html(input);
        assert!(result.contains("https://example.com/photo.jpg"));
    }

    #[test]
    fn strips_uppercase_tracking_pixel() {
        let input =
            r#"<p>Hello</p><IMG SRC="https://track.example.com/pixel.gif" WIDTH="1" HEIGHT="1">"#;
        let result = sanitize_email_html(input);
        assert!(!result.to_ascii_lowercase().contains("<img"));
    }

    #[test]
    fn keeps_mixed_case_https_img() {
        let input =
            r#"<Img Src="https://example.com/photo.jpg" Alt="photo" Width="200" Height="150">"#;
        let result = sanitize_email_html(input);
        assert!(result.to_ascii_lowercase().contains("<img"));
        assert!(result.contains("https://example.com/photo.jpg"));
    }

    #[test]
    fn sets_link_rel() {
        let input = r#"<a href="https://example.com">click</a>"#;
        let result = sanitize_email_html(input);
        assert!(result.contains("noopener noreferrer"));
    }
}

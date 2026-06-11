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
    let input = r#"<img src="https://example.com/photo.jpg" alt="photo" width="200" height="150">"#;
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
    let input = r#"<Img Src="https://example.com/photo.jpg" Alt="photo" Width="200" Height="150">"#;
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

#[test]
fn strips_link_target_so_renderer_controls_navigation() {
    let input = r#"<a href="https://example.com" target="_blank">click</a>"#;
    let result = sanitize_email_html(input);

    assert!(!result.to_ascii_lowercase().contains("target="));
    assert!(result.contains("https://example.com"));
}

#[test]
fn strips_event_handler_attributes() {
    let result = sanitize_email_html(r#"<p onclick="alert(1)">hi</p>"#);
    assert!(!result.contains("onclick"));
    assert!(!result.contains("alert"));
    assert!(result.contains("<p>hi</p>"));
}

#[test]
fn drops_javascript_and_data_uri_hrefs() {
    let js = sanitize_email_html(r#"<a href="javascript:alert(1)">x</a>"#);
    assert!(!js.contains("javascript:"));
    let data = sanitize_email_html(r#"<a href="data:text/html,<script>alert(1)</script>">x</a>"#);
    assert!(!data.contains("data:"));
    assert!(!data.contains("<script"));
}

#[test]
fn strips_dangerous_elements() {
    // Active-content and navigation-hijacking elements have no place in rendered email
    // bodies; ammonia's allowlist must drop them entirely (no src/action leakage).
    let vectors = [
        r#"<iframe src="https://evil.example"></iframe>"#,
        r#"<object data="https://evil.example"></object>"#,
        r#"<embed src="https://evil.example">"#,
        r#"<form action="https://evil.example"><input></form>"#,
        r#"<svg onload="alert(1)"></svg>"#,
        r#"<meta http-equiv="refresh" content="0;url=https://evil.example">"#,
        r#"<base href="https://evil.example/">"#,
        r#"<style>@import url(https://evil.example/x.css);</style>"#,
    ];
    for input in vectors {
        let result = sanitize_email_html(input).to_ascii_lowercase();
        assert!(
            !result.contains("evil.example"),
            "dangerous element leaked content: input={input:?} result={result:?}"
        );
        assert!(
            !result.contains("onload"),
            "event handler survived: {input:?}"
        );
    }
}

#[test]
fn strips_remote_and_expression_css_from_style_attribute() {
    let url = sanitize_email_html(
        r#"<div style="background-image:url(https://track.example/p.png)">x</div>"#,
    );
    assert!(!url.contains("url("), "css url() survived: {url:?}");
    assert!(
        !url.contains("track.example"),
        "remote css ref leaked: {url:?}"
    );

    let expr = sanitize_email_html(r#"<div style="width:expression(alert(1))">x</div>"#);
    assert!(
        !expr.contains("expression("),
        "css expression() survived: {expr:?}"
    );
}

#[test]
fn keeps_safe_inline_style_declarations() {
    let result = sanitize_email_html(r#"<p style="color:red; font-weight:bold">hi</p>"#);
    assert!(
        result.contains("color:red"),
        "safe style dropped: {result:?}"
    );
    assert!(
        result.contains("font-weight:bold"),
        "safe style dropped: {result:?}"
    );
}

use super::*;

#[test]
fn render_markdown_excludes_raw_html_and_markdown_images() {
    let rendered =
        render_markdown("<script>alert(1)</script>\n\n![pixel](https://example.test/pixel.png)");

    assert!(rendered.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!rendered.contains("<script>"));
    assert!(!rendered.contains("<img"));
    assert!(!rendered.contains("https://example.test/pixel.png"));
    assert!(rendered.contains("pixel"));
}

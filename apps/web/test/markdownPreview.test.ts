import { describe, expect, it } from 'bun:test'

import { renderMarkdownPreview } from '../src/markdownPreview'

describe('markdown composer preview', () => {
  it('renders Markdown emphasis to HTML', () => {
    expect(renderMarkdownPreview('Hello **world**')).toContain(
      '<strong>world</strong>',
    )
  })

  it('renders GFM tables, strikethrough, task lists, and autolinks', () => {
    const html = renderMarkdownPreview(
      [
        '| Item | Done |',
        '| --- | --- |',
        '| ~~old~~ | https://example.com |',
        '',
        '- [x] Done',
      ].join('\n'),
    )

    expect(html).toContain('<table>')
    expect(html).toContain('<del>old</del>')
    expect(html).toContain('type="checkbox"')
    expect(html).toContain('<a href="https://example.com">')
  })

  it('keeps raw HTML and Markdown images out of the preview HTML', () => {
    const html = renderMarkdownPreview(
      '<script>alert(1)</script>\n\n![pixel](https://example.com/pixel.png)',
    )

    expect(html).toContain('&lt;script&gt;alert(1)&lt;/script&gt;')
    expect(html).not.toContain('<script>')
    expect(html).not.toContain('<img')
    expect(html).not.toContain('https://example.com/pixel.png')
    expect(html).toContain('pixel')
  })
})

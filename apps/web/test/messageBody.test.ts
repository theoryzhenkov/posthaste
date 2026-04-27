import { describe, expect, it } from 'bun:test'

import { resolveMessageBodyRender } from '../src/messageBody'

describe('message body rendering preference', () => {
  it('prefers sanitized HTML over plaintext alternatives', () => {
    expect(
      resolveMessageBodyRender({
        bodyHtml: '<h1><strong>Posts</strong></h1>',
        bodyText: '# **Posts**',
        preview: null,
      }),
    ).toEqual({
      kind: 'html',
      html: '<h1><strong>Posts</strong></h1>',
    })
  })

  it('falls back to plaintext when no HTML body exists', () => {
    expect(
      resolveMessageBodyRender({
        bodyHtml: null,
        bodyText: 'Hello\n\nWorld',
        preview: null,
      }),
    ).toEqual({
      kind: 'text',
      paragraphs: ['Hello', 'World'],
    })
  })
})

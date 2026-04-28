import { describe, expect, it } from 'bun:test'

import { toggleMarkdownMarker } from '../src/markdownEditing'

describe('Markdown editing commands', () => {
  it('wraps selected text with a Markdown marker', () => {
    expect(
      toggleMarkdownMarker(
        { text: 'Hello world', selectionStart: 6, selectionEnd: 11 },
        '*',
      ),
    ).toEqual({
      text: 'Hello *world*',
      selectionStart: 7,
      selectionEnd: 12,
    })
  })

  it('removes surrounding markers around the selected text', () => {
    expect(
      toggleMarkdownMarker(
        { text: 'Hello *world*', selectionStart: 7, selectionEnd: 12 },
        '*',
      ),
    ).toEqual({
      text: 'Hello world',
      selectionStart: 6,
      selectionEnd: 11,
    })
  })

  it('inserts paired markers at the cursor for the next typed text', () => {
    expect(
      toggleMarkdownMarker(
        { text: 'Hello ', selectionStart: 6, selectionEnd: 6 },
        '**',
      ),
    ).toEqual({
      text: 'Hello ****',
      selectionStart: 8,
      selectionEnd: 8,
    })
  })
})

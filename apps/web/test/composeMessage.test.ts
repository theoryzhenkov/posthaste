import { describe, expect, it } from 'bun:test'

import {
  buildSendInput,
  formatRecipients,
  parseRecipients,
} from '../src/composeMessage'

describe('compose message helpers', () => {
  it('parses free-form and display-name recipients', () => {
    expect(
      parseRecipients(
        'Ada <ada@example.test>; bob@example.test, "Cy" <cy@example.test>',
      ),
    ).toEqual([
      { name: 'Ada', email: 'ada@example.test' },
      { name: null, email: 'bob@example.test' },
      { name: 'Cy', email: 'cy@example.test' },
    ])
  })

  it('formats recipient display names only when present', () => {
    expect(
      formatRecipients([
        { name: 'Ada', email: 'ada@example.test' },
        { name: null, email: 'bob@example.test' },
      ]),
    ).toBe('Ada <ada@example.test>, bob@example.test')
  })

  it('builds the send input without constraining free-form senders', () => {
    expect(
      buildSendInput({
        from: 'anything@example.test',
        to: 'Ada <ada@example.test>',
        cc: '',
        bcc: 'blind@example.test',
        subject: '  Hello  ',
        body: 'Markdown body',
      }),
    ).toEqual({
      from: { name: null, email: 'anything@example.test' },
      to: [{ name: 'Ada', email: 'ada@example.test' }],
      cc: [],
      bcc: [{ name: null, email: 'blind@example.test' }],
      subject: 'Hello',
      body: 'Markdown body',
      inReplyTo: null,
      references: null,
    })
  })
})

import { describe, expect, it } from 'bun:test'

import {
  buildSendInput,
  formatRecipients,
  parseRecipients,
  readAttachmentForSend,
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

  it('reads attachments as base64 send payloads', async () => {
    const file = new File(['hello attachment'], 'notes.txt', {
      type: 'text/plain',
    })

    const payload = await readAttachmentForSend({
      id: 'a1',
      file,
      filename: 'notes.txt',
      mimeType: 'text/plain',
      size: file.size,
    })

    expect(payload).toEqual({
      filename: 'notes.txt',
      mimeType: 'text/plain',
      contentBase64: 'aGVsbG8gYXR0YWNobWVudA==',
    })
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
        attachments: [],
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
      attachments: [],
    })
  })
})

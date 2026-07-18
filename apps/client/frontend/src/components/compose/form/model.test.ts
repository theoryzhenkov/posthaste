import { describe, expect, test } from 'bun:test'

import type { MessageDetailResult, MessageSummary } from '@/gen'
import { parseEmailPattern, type EmailPattern } from '@/domain/address'
import {
  accountFromOptions,
  replyContextFromDetail,
  toSendMessageRequest,
  type ComposeAccount,
} from './model'

function summary(overrides: Partial<MessageSummary> = {}): MessageSummary {
  return {
    id: 'm1',
    sourceId: 'acct-1',
    sourceName: 'Work',
    sourceThreadId: 't1',
    conversationId: 'c1',
    subject: 'Quarterly report',
    fromName: 'Ada Lovelace',
    fromEmail: 'ada@example.com',
    to: [{ name: 'Theo', email: 'theo@example.com' }],
    preview: null,
    receivedAt: '2026-07-01T10:00:00Z',
    hasAttachment: false,
    isRead: true,
    isFlagged: false,
    mailboxIds: ['inbox'],
    keywords: ['$seen'],
    rfcMessageId: '<orig@example.com>',
    inReplyTo: '<parent@example.com>',
    ...overrides,
  }
}

function detail(
  overrides: Partial<MessageDetailResult> = {},
): MessageDetailResult {
  return {
    summary: summary(),
    bodyHtml: null,
    bodyText: 'first line\nsecond line',
    attachments: [],
    ...overrides,
  }
}

describe('replyContextFromDetail', () => {
  test('derives reply recipients, subjects, and threading headers', () => {
    const ctx = replyContextFromDetail(detail())
    expect(ctx.to).toEqual([{ name: 'Ada Lovelace', email: 'ada@example.com' }])
    expect(ctx.originalTo).toEqual([{ name: 'Theo', email: 'theo@example.com' }])
    expect(ctx.replySubject).toBe('Re: Quarterly report')
    expect(ctx.forwardSubject).toBe('Fwd: Quarterly report')
    expect(ctx.inReplyTo).toBe('<orig@example.com>')
    expect(ctx.references).toBe('<parent@example.com> <orig@example.com>')
    expect(ctx.originalDate).toBe('2026-07-01T10:00:00Z')
  })

  test('quotes every body line and builds the forwarded block', () => {
    const ctx = replyContextFromDetail(detail())
    expect(ctx.quotedBody).toBe('> first line\n> second line')
    expect(ctx.forwardedBody).toContain(
      '---------- Forwarded message ----------',
    )
    expect(ctx.forwardedBody).toContain(
      'From: Ada Lovelace <ada@example.com>',
    )
    expect(ctx.forwardedBody).toContain('Subject: Quarterly report')
    expect(ctx.forwardedBody).toContain('first line\nsecond line')
  })

  test('does not double-prefix an already-prefixed subject', () => {
    const ctx = replyContextFromDetail(
      detail({ summary: summary({ subject: 're: hello' }) }),
    )
    expect(ctx.replySubject).toBe('re: hello')
  })

  test('degrades without a body or threading headers', () => {
    const ctx = replyContextFromDetail(
      detail({
        bodyText: null,
        summary: summary({
          rfcMessageId: undefined,
          inReplyTo: undefined,
        }),
      }),
    )
    expect(ctx.quotedBody).toBeNull()
    expect(ctx.forwardedBody).toBeNull()
    expect(ctx.inReplyTo).toBeNull()
    expect(ctx.references).toBeNull()
  })
})

describe('toSendMessageRequest', () => {
  test('pins the draft identity onto the assembled input', () => {
    const request = toSendMessageRequest(
      {
        from: { name: null, email: 'me@example.com' },
        to: [{ name: null, email: 'you@example.com' }],
        cc: [],
        bcc: [],
        subject: 'hi',
        body: 'text',
        inReplyTo: '<orig@example.com>',
        references: '<a> <b>',
        attachments: [],
      },
      'draft-key-1',
    )
    expect(request.draftId).toBe('draft-key-1')
    expect(request.inReplyTo).toBe('<orig@example.com>')
    expect(request.sendAt).toBeUndefined()
    expect(request.undoWindowSeconds).toBeUndefined()
  })
})

describe('accountFromOptions', () => {
  const accounts: ComposeAccount[] = [
    {
      id: 'acct-1',
      name: 'Work',
      fullName: 'Theo R',
      emailPatterns: ['theo@example.com', '*@corp.example.com'].map(
        (raw): EmailPattern => {
          const pattern = parseEmailPattern(raw)
          if (!pattern) throw new Error(`fixture pattern rejected: ${raw}`)
          return pattern
        },
      ),
    },
  ]

  test('offers configured concrete addresses and the identity first', () => {
    const options = accountFromOptions(
      accounts,
      { name: 'Theo R', email: 'theo@example.com' },
      'acct-1',
      [],
    )
    expect(options[0]).toMatchObject({
      origin: 'identity',
      email: 'theo@example.com',
    })
    // The identical configured address is de-duplicated behind the identity.
    expect(
      options.filter((option) => option.email === 'theo@example.com'),
    ).toHaveLength(1)
  })

  test('keeps cached senders only when they match the account patterns', () => {
    const options = accountFromOptions(accounts, null, 'acct-1', [
      { sourceId: 'acct-1', name: null, email: 'alias@corp.example.com' },
      { sourceId: 'acct-1', name: null, email: 'stranger@elsewhere.com' },
    ])
    const emails = options.map((option) => option.email)
    expect(emails).toContain('alias@corp.example.com')
    expect(emails).not.toContain('stranger@elsewhere.com')
  })
})

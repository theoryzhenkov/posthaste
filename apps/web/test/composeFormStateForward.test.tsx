import { describe, expect, it } from 'bun:test'
import { act, renderHook } from '@testing-library/react'

import type { ReplyContext } from '../src/api/types'
import {
  composeAttachmentFromFile,
  formatReplyAttribution,
} from '../src/components/composeFormHelpers'
import { useComposeFormState } from '../src/components/compose-overlay/useComposeFormState'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const replyContext: ReplyContext = {
  to: [{ name: 'Ada Sender', email: 'ada@example.com' }],
  cc: [{ name: 'Cc Person', email: 'cc@example.com' }],
  originalTo: [{ name: 'You', email: 'me@example.com' }],
  replySubject: 'Re: Subject',
  forwardSubject: 'Fwd: Subject',
  quotedBody: '> original line',
  forwardedBody: '---------- Forwarded message ----------\nFrom: Ada\n\nbody',
  inReplyTo: '<msg-1@example.com>',
  references: '<msg-1@example.com>',
  originalFrom: [{ name: 'Ada Sender', email: 'ada@example.com' }],
  originalDate: '2026-07-06T10:34:00Z',
}

/** The attribution the hook inserts (environment locale — computed, not pinned;
 *  the pinned-locale formatting cases live in composeFormHelpers.test.ts). */
const attribution = formatReplyAttribution(
  replyContext.originalFrom[0],
  replyContext.originalDate,
)

function renderForm(intentKind: 'reply' | 'forward') {
  return renderHook(() =>
    useComposeFormState({
      composeKey: `primary:em-1:${intentKind}`,
      draftSeed: undefined,
      forwardAttachments: [],
      identity: { email: 'me@example.com', name: 'You' },
      intentKind,
      isMessageBasedCompose: true,
      replyContext,
    }),
  )
}

describe('useComposeFormState — forward', () => {
  it('forward starts with empty recipients (not the original sender) + Fwd subject + forwarded body', () => {
    const { result } = renderForm('forward')
    const { form } = result.current

    // Regression: forward must NOT pre-fill `to` with replyContext.to (the
    // original sender) — a forward goes to brand-new recipients.
    expect(form.to).toBe('')
    expect(form.cc).toBe('')
    expect(form.subject).toBe('Fwd: Subject')
    expect(form.body).toContain('Forwarded message')
  })

  it('reply still addresses the original sender + Re subject + quoted body', () => {
    const { result } = renderForm('reply')
    const { form } = result.current

    expect(form.to).toContain('ada@example.com')
    expect(form.subject).toBe('Re: Subject')
    expect(form.body).toContain('> original line')
  })
})

describe('useComposeFormState — FIX2 reply streaming', () => {
  const baseProps = {
    composeKey: 'primary:em-1:reply',
    draftSeed: undefined,
    forwardAttachments: [],
    identity: { email: 'me@example.com', name: 'You' },
    intentKind: 'reply' as const,
    isMessageBasedCompose: true,
    signature: null,
  }

  it('is usable before replyContext arrives, then streams the quote in WITHOUT clobbering early edits', () => {
    const { result, rerender } = renderHook(
      (props: Parameters<typeof useComposeFormState>[0]) =>
        useComposeFormState(props),
      { initialProps: { ...baseProps, replyContext: undefined } },
    )

    // The editor is interactive immediately: no quote gate, an empty body.
    expect(result.current.form.body).toBe('')

    // The user starts typing their reply before the quote has loaded.
    act(() => {
      result.current.setField('body', 'My reply')
    })
    expect(result.current.form.body).toBe('My reply')

    // replyContext settles → the quote streams in BELOW the early text (which is
    // preserved, not reset), and the recipient/subject fill in.
    rerender({ ...baseProps, replyContext })

    expect(result.current.form.body).toBe(
      `My reply\n\n${attribution}\n> original line`,
    )
    expect(result.current.form.to).toContain('ada@example.com')
    expect(result.current.form.subject).toBe('Re: Subject')
  })

  it('does not overwrite recipient/subject fields the user already edited', () => {
    const { result, rerender } = renderHook(
      (props: Parameters<typeof useComposeFormState>[0]) =>
        useComposeFormState(props),
      { initialProps: { ...baseProps, replyContext: undefined } },
    )

    act(() => {
      result.current.setField('to', 'someone@else.com')
      result.current.setField('subject', 'My own subject')
    })

    rerender({ ...baseProps, replyContext })

    // The user's edits win; only the untouched body gets the streamed quote.
    expect(result.current.form.to).toBe('someone@else.com')
    expect(result.current.form.subject).toBe('My own subject')
    expect(result.current.form.body).toContain('> original line')
  })
})

describe('useComposeFormState — signature/attribution assembly order', () => {
  const baseProps = {
    composeKey: 'primary:em-1:reply',
    draftSeed: undefined,
    forwardAttachments: [],
    identity: { email: 'me@example.com', name: 'You' },
    intentKind: 'reply' as const,
    isMessageBasedCompose: true,
  }
  // The exact reply body pinned: cursor space at the TOP, then the signature
  // (with its RFC 3676 `-- ` delimiter), then the attribution line, then the
  // `> `-quote. This is the assembly-order contract.
  const assembled = `\n\n-- \nSig\n\n${attribution}\n> original line`

  it('reply: cursor space, signature, attribution, quote — in that order (context first)', () => {
    const { result } = renderHook(() =>
      useComposeFormState({ ...baseProps, replyContext, signature: 'Sig' }),
    )
    expect(result.current.form.body).toBe(assembled)
  })

  it('assembles the SAME body when the signature settles before the quote', () => {
    const { result, rerender } = renderHook(
      (props: Parameters<typeof useComposeFormState>[0]) =>
        useComposeFormState(props),
      {
        initialProps: {
          ...baseProps,
          replyContext: undefined,
          signature: 'Sig',
        },
      },
    )
    // Signature seeded alone (appended — nothing else in the body yet).
    expect(result.current.form.body).toBe('\n\n-- \nSig')
    // The quote streams in below it.
    rerender({ ...baseProps, replyContext, signature: 'Sig' })
    expect(result.current.form.body).toBe(assembled)
  })

  it('assembles the SAME body when the quote settles before the signature', () => {
    const { result, rerender } = renderHook(
      (props: Parameters<typeof useComposeFormState>[0]) =>
        useComposeFormState(props),
      { initialProps: { ...baseProps, replyContext, signature: null } },
    )
    expect(result.current.form.body).toBe(`\n\n${attribution}\n> original line`)
    // The signature arrives late → inserted ABOVE the quote, not appended.
    rerender({ ...baseProps, replyContext, signature: 'Sig' })
    expect(result.current.form.body).toBe(assembled)
  })

  it('keeps early-typed text at the top, above the signature', () => {
    const { result, rerender } = renderHook(
      (props: Parameters<typeof useComposeFormState>[0]) =>
        useComposeFormState(props),
      {
        initialProps: {
          ...baseProps,
          replyContext: undefined,
          signature: null,
        },
      },
    )
    act(() => {
      result.current.setField('body', 'My reply')
    })
    rerender({ ...baseProps, replyContext, signature: 'Sig' })
    expect(result.current.form.body).toBe(
      `My reply\n\n-- \nSig\n\n${attribution}\n> original line`,
    )
  })

  it('forward: signature above the forwarded-message block (which carries its own header)', () => {
    const { result } = renderHook(() =>
      useComposeFormState({
        ...baseProps,
        composeKey: 'primary:em-1:forward',
        intentKind: 'forward' as const,
        replyContext,
        signature: 'Sig',
      }),
    )
    expect(result.current.form.body).toBe(
      `\n\n-- \nSig\n\n${replyContext.forwardedBody}`,
    )
  })

  it('new message keeps the existing end-of-body signature behavior', () => {
    const { result } = renderHook(() =>
      useComposeFormState({
        ...baseProps,
        composeKey: 'primary:new',
        intentKind: 'new' as const,
        isMessageBasedCompose: false,
        replyContext: undefined,
        signature: 'Sig',
      }),
    )
    expect(result.current.form.body).toBe('\n\n-- \nSig')
  })

  it('a resumed draft still skips signature seeding entirely', () => {
    const { result } = renderHook(() =>
      useComposeFormState({
        ...baseProps,
        composeKey: 'primary:draft-1:draft',
        intentKind: 'draft' as const,
        isMessageBasedCompose: true,
        replyContext: undefined,
        draftSeed: {
          from: '',
          to: '',
          cc: '',
          bcc: '',
          subject: 'Resumed',
          body: 'draft body',
        },
        signature: 'Sig',
      }),
    )
    expect(result.current.form.body).toBe('draft body')
  })

  it('reply without a display name attributes the bare email', () => {
    const context: ReplyContext = {
      ...replyContext,
      originalFrom: [{ name: null, email: 'ada@example.com' }],
    }
    const { result } = renderHook(() =>
      useComposeFormState({
        ...baseProps,
        replyContext: context,
        signature: null,
      }),
    )
    expect(result.current.form.body).toBe(
      `\n\n${formatReplyAttribution({ name: null, email: 'ada@example.com' }, context.originalDate)}\n> original line`,
    )
    expect(result.current.form.body).toContain('ada@example.com wrote:')
    expect(result.current.form.body).not.toContain('<ada@example.com> wrote:')
  })
})

describe('useComposeFormState — attachment ingestion (paste/drop) and draft reopen', () => {
  const baseProps = {
    composeKey: 'primary:new',
    draftSeed: undefined,
    forwardAttachments: [],
    identity: { email: 'me@example.com', name: 'You' },
    intentKind: 'new' as const,
    isMessageBasedCompose: false,
    replyContext: undefined,
    signature: null,
  }

  it('ingests pasted/dropped files into the same attachment state as the picker', () => {
    const { result } = renderHook(() => useComposeFormState(baseProps))
    act(() => {
      result.current.ingestFiles([
        new File(['%PDF-1.4'], 'doc.pdf', { type: 'application/pdf' }),
      ])
    })
    expect(result.current.form.attachments).toHaveLength(1)
    expect(result.current.form.attachments[0].filename).toBe('doc.pdf')
    expect(result.current.form.attachments[0].mimeType).toBe('application/pdf')
    expect(result.current.errorMessage).toBeNull()
  })

  it('names unnamed pasted images pasted-image-<n>.<ext>, preserving the MIME type', () => {
    const { result } = renderHook(() => useComposeFormState(baseProps))
    act(() => {
      result.current.ingestFiles([
        new File([new Uint8Array([137, 80])], '', { type: 'image/png' }),
      ])
    })
    act(() => {
      result.current.ingestFiles([
        new File([new Uint8Array([255, 216])], '', { type: 'image/jpeg' }),
      ])
    })
    const [first, second] = result.current.form.attachments
    expect(first.filename).toBe('pasted-image-1.png')
    expect(first.mimeType).toBe('image/png')
    expect(second.filename).toBe('pasted-image-2.jpeg')
    expect(second.mimeType).toBe('image/jpeg')
  })

  it('rejects an over-limit pasted file with the size-cap message instead of silently failing', () => {
    const { result } = renderHook(() => useComposeFormState(baseProps))
    const oversized = new File(
      [new Uint8Array(10 * 1024 * 1024 + 1)],
      'huge.bin',
      { type: 'application/octet-stream' },
    )
    act(() => {
      result.current.ingestFiles([oversized])
    })
    expect(result.current.form.attachments).toHaveLength(0)
    expect(result.current.errorMessage).toBe('huge.bin is larger than 10.0 MB.')
  })

  it('reopening a draft seeds its saved attachments back into the form (round-trip)', () => {
    const savedAttachment = composeAttachmentFromFile(
      new File(['pasted bytes'], 'pasted-image-1.png', { type: 'image/png' }),
    )
    const { result } = renderHook(() =>
      useComposeFormState({
        ...baseProps,
        composeKey: 'primary:draft-1:draft',
        intentKind: 'draft' as const,
        isMessageBasedCompose: true,
        draftSeed: {
          from: '',
          to: '',
          cc: '',
          bcc: '',
          subject: 'Resumed',
          body: 'draft body',
        },
        forwardAttachments: [savedAttachment],
      }),
    )
    expect(result.current.form.attachments).toEqual([savedAttachment])
  })
})

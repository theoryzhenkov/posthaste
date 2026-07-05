import { describe, expect, it } from 'bun:test'
import { act, renderHook } from '@testing-library/react'

import type { ReplyContext } from '../src/api/types'
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
}

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

    expect(result.current.form.body).toBe('My reply\n\n> original line')
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

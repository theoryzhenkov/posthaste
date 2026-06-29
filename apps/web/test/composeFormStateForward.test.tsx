import { describe, expect, it } from 'bun:test'
import { renderHook } from '@testing-library/react'

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

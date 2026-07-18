import { describe, expect, test } from 'bun:test'

import {
  composeView,
  initialComposeMachineState,
  reduceCompose,
  type ComposeEvent,
  type ComposeMachineState,
  type ComposeSession,
} from './machine'
import {
  MAX_COMPOSE_ATTACHMENTS,
  EMPTY_FORM,
  type ComposeAttachment,
  type ComposeForm,
} from './model'

function session(
  resetKey = 'k1',
  initialForm: ComposeForm = EMPTY_FORM,
): ComposeSession {
  return { resetKey, initialForm }
}

function run(
  s: ComposeSession,
  events: ComposeEvent[],
  from: ComposeMachineState = initialComposeMachineState(s),
): ComposeMachineState {
  return events.reduce((state, event) => reduceCompose(state, s, event), from)
}

function attachment(id: string, size = 100): ComposeAttachment {
  return {
    id,
    file: new File(['x'], `${id}.txt`),
    filename: `${id}.txt`,
    mimeType: 'text/plain',
    size,
  }
}

describe('fieldChanged', () => {
  test('writes the field and marks the session edited', () => {
    const s = session()
    const state = run(s, [{ type: 'fieldChanged', field: 'subject', value: 'Hi' }])
    expect(state.form.subject).toBe('Hi')
    expect(state.hasUserEdited).toBe(true)
    expect(state.errorMessage).toBeNull()
  })

  test('keeps an existing error line (edits do not clear it)', () => {
    const s = session()
    const state = run(s, [
      { type: 'errorReported', message: 'boom' },
      { type: 'fieldChanged', field: 'body', value: 'text' },
    ])
    expect(state.errorMessage).toBe('boom')
  })
})

describe('session normalization', () => {
  test('an event after the reset key changed starts from the new initial form', () => {
    const first = session('k1')
    const edited = run(first, [
      { type: 'fieldChanged', field: 'body', value: 'old session text' },
      { type: 'errorReported', message: 'stale error' },
    ])
    const next = session('k2', { ...EMPTY_FORM, subject: 'Resumed' })
    const state = reduceCompose(edited, next, {
      type: 'fieldChanged',
      field: 'to',
      value: 'a@b.c',
    })
    expect(state.resetKey).toBe('k2')
    expect(state.form.subject).toBe('Resumed')
    expect(state.form.body).toBe('')
    expect(state.form.to).toBe('a@b.c')
    expect(state.errorMessage).toBeNull()
  })

  test('composeView reads a stale state as the fresh session', () => {
    const first = session('k1')
    const edited = run(first, [
      { type: 'fieldChanged', field: 'body', value: 'typed' },
    ])
    const next = session('k2')
    const view = composeView(edited, next)
    expect(view.form.body).toBe('')
    expect(view.hasUserEdited).toBe(false)
  })
})

describe('attachments', () => {
  test('adds within limits, marks edited, clears the error line', () => {
    const s = session()
    const state = run(s, [
      { type: 'errorReported', message: 'old error' },
      { type: 'attachmentsAdded', attachments: [attachment('a')] },
    ])
    expect(state.form.attachments.map((a) => a.id)).toEqual(['a'])
    expect(state.errorMessage).toBeNull()
    expect(state.hasUserEdited).toBe(true)
  })

  test('rejects an over-limit batch whole and surfaces the reason', () => {
    const s = session()
    const tooMany = Array.from({ length: MAX_COMPOSE_ATTACHMENTS + 1 }, (_, i) =>
      attachment(`a${i}`),
    )
    const state = run(s, [{ type: 'attachmentsAdded', attachments: tooMany }])
    expect(state.form.attachments).toEqual([])
    expect(state.errorMessage).toContain(`${MAX_COMPOSE_ATTACHMENTS}`)
    expect(state.hasUserEdited).toBe(false)
  })

  test('removes by id and marks edited', () => {
    const s = session()
    const state = run(s, [
      { type: 'attachmentsAdded', attachments: [attachment('a'), attachment('b')] },
      { type: 'attachmentRemoved', attachmentId: 'a' },
    ])
    expect(state.form.attachments.map((a) => a.id)).toEqual(['b'])
  })
})

describe('identityDefaulted', () => {
  test('fills an empty From without marking the session edited', () => {
    const s = session()
    const state = run(s, [
      { type: 'identityDefaulted', from: 'Theo <theo@example.com>' },
    ])
    expect(state.form.from).toBe('Theo <theo@example.com>')
    expect(state.hasUserEdited).toBe(false)
  })

  test('never clobbers a chosen From', () => {
    const s = session()
    const state = run(s, [
      { type: 'fieldChanged', field: 'from', value: 'me@else.com' },
      { type: 'identityDefaulted', from: 'Theo <theo@example.com>' },
    ])
    expect(state.form.from).toBe('me@else.com')
  })
})

describe('forwardAttachmentsSeeded', () => {
  test('seeds once and only into an empty attachment list', () => {
    const s = session()
    const state = run(s, [
      { type: 'forwardAttachmentsSeeded', attachments: [attachment('orig')] },
      { type: 'forwardAttachmentsSeeded', attachments: [attachment('again')] },
    ])
    expect(state.form.attachments.map((a) => a.id)).toEqual(['orig'])
    expect(state.hasUserEdited).toBe(false)
  })

  test('yields to attachments the user already added', () => {
    const s = session()
    const state = run(s, [
      { type: 'attachmentsAdded', attachments: [attachment('mine')] },
      { type: 'forwardAttachmentsSeeded', attachments: [attachment('orig')] },
    ])
    expect(state.form.attachments.map((a) => a.id)).toEqual(['mine'])
  })
})

describe('replyContextSeeded', () => {
  const seed: ComposeEvent = {
    type: 'replyContextSeeded',
    to: 'Ada <ada@example.com>',
    cc: 'Grace <grace@example.com>',
    subject: 'Re: Hello',
    quoteBlock: 'On Mon Ada wrote:\n> hello',
  }

  test('fills untouched fields and appends the quote below early typing', () => {
    const s = session()
    const state = run(s, [
      { type: 'fieldChanged', field: 'body', value: 'typed early' },
      seed,
    ])
    expect(state.form.to).toBe('Ada <ada@example.com>')
    expect(state.form.cc).toBe('Grace <grace@example.com>')
    expect(state.form.subject).toBe('Re: Hello')
    expect(state.form.body).toBe('typed early\n\nOn Mon Ada wrote:\n> hello')
  })

  test('never overwrites fields the user already typed, and seeds once', () => {
    const s = session()
    const state = run(s, [
      { type: 'fieldChanged', field: 'to', value: 'other@example.com' },
      seed,
      seed,
    ])
    expect(state.form.to).toBe('other@example.com')
    // The quote appended exactly once.
    expect(state.form.body).toBe('\n\nOn Mon Ada wrote:\n> hello')
  })
})

describe('signatureSeeded', () => {
  test('appends for a fresh message, once', () => {
    const s = session()
    const state = run(s, [
      { type: 'fieldChanged', field: 'body', value: 'hello' },
      { type: 'signatureSeeded', signature: 'Theo', placement: 'append' },
      { type: 'signatureSeeded', signature: 'Theo', placement: 'append' },
    ])
    expect(state.form.body).toBe('hello\n\n-- \nTheo')
  })

  test('lands above the quote when the quote seeded first', () => {
    const s = session()
    const state = run(s, [
      {
        type: 'replyContextSeeded',
        to: 'a@b.c',
        cc: '',
        subject: 'Re: x',
        quoteBlock: '> quoted',
      },
      { type: 'signatureSeeded', signature: 'Theo', placement: 'aboveQuote' },
    ])
    expect(state.form.body).toBe('\n\n-- \nTheo\n\n> quoted')
  })

  test('signature first, quote later: the quote lands below it', () => {
    const s = session()
    const state = run(s, [
      { type: 'signatureSeeded', signature: 'Theo', placement: 'aboveQuote' },
      {
        type: 'replyContextSeeded',
        to: 'a@b.c',
        cc: '',
        subject: 'Re: x',
        quoteBlock: '> quoted',
      },
    ])
    expect(state.form.body).toBe('\n\n-- \nTheo\n\n> quoted')
  })
})

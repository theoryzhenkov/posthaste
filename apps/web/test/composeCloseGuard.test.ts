import { describe, expect, it } from 'bun:test'

import {
  EMPTY_FORM,
  type ComposeForm,
} from '../src/components/composeFormHelpers'
import {
  composeCloseCopy,
  composeFormHasContent,
  shouldPromptBeforeClose,
} from '../src/components/compose-overlay/composeCloseGuard'

function form(overrides: Partial<ComposeForm> = {}): ComposeForm {
  return { ...EMPTY_FORM, ...overrides }
}

describe('composeCloseGuard', () => {
  it('prompts when closing a dirty NEW compose (edited + has content)', () => {
    expect(
      shouldPromptBeforeClose({
        form: form({ body: 'draft in progress' }),
        hasUserEdited: true,
        isSending: false,
      }),
    ).toBe(true)
  })

  it('does NOT prompt for an empty/unchanged compose', () => {
    // Untouched.
    expect(
      shouldPromptBeforeClose({
        form: form(),
        hasUserEdited: false,
        isSending: false,
      }),
    ).toBe(false)
    // Focused/blurred but no content typed.
    expect(
      shouldPromptBeforeClose({
        form: form(),
        hasUserEdited: true,
        isSending: false,
      }),
    ).toBe(false)
  })

  it('does NOT prompt when a send is in flight (the sent-message close)', () => {
    expect(
      shouldPromptBeforeClose({
        form: form({ body: 'already on its way' }),
        hasUserEdited: true,
        isSending: true,
      }),
    ).toBe(false)
  })

  it('detects content across any addressable field or an attachment', () => {
    expect(composeFormHasContent(form())).toBe(false)
    expect(composeFormHasContent(form({ to: 'a@b.com' }))).toBe(true)
    expect(composeFormHasContent(form({ subject: 'hi' }))).toBe(true)
    expect(
      composeFormHasContent(
        form({
          attachments: [{ id: 'a1' } as ComposeForm['attachments'][number]],
        }),
      ),
    ).toBe(true)
  })

  it('words the prompt for a fresh compose vs. a resumed draft', () => {
    expect(composeCloseCopy('new').title).toBe('Save this message as a draft?')
    expect(composeCloseCopy('reply').saveLabel).toBe('Save as draft')
    expect(composeCloseCopy('draft').title).toBe('Save changes to this draft?')
    expect(composeCloseCopy('draft').saveLabel).toBe('Save changes')
  })
})

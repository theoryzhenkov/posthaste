import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import { ComposeCloseConfirmDialog } from './ComposeCloseConfirmDialog'
import { shouldPromptBeforeClose } from './composeCloseGuard'
import { EMPTY_COMPOSE_FORM } from '../form/composeMessage'
import type { ComposeForm } from '../form/model'

function renderDialog(open: boolean): string {
  return renderToStaticMarkup(
    <ComposeCloseConfirmDialog
      open={open}
      intentKind="new"
      onKeepEditing={() => {}}
      onDiscard={() => {}}
      onSaveAsDraft={() => {}}
    />,
  )
}

describe('ComposeCloseConfirmDialog', () => {
  test('renders nothing while closed', () => {
    expect(renderDialog(false)).toBe('')
  })

  test('renders in place, scoped to the compose surface', () => {
    const markup = renderDialog(true)
    // The prompt is an alertdialog rendered where it is mounted (inside the
    // compose surface's positioned container) — no portal, and positioned
    // absolutely within that container, never fixed over the whole window.
    expect(markup).toContain('role="alertdialog"')
    expect(markup).toContain('absolute inset-0')
    expect(markup).not.toContain('fixed')
    expect(markup).toContain('Save this message as a draft?')
    expect(markup).toContain('Keep editing')
    expect(markup).toContain('Discard')
    expect(markup).toContain('Save as draft')
  })
})

describe('shouldPromptBeforeClose', () => {
  const form = (body: string): ComposeForm => ({
    ...EMPTY_COMPOSE_FORM,
    body,
  })

  test('prompts only for edited content that is worth keeping', () => {
    expect(
      shouldPromptBeforeClose({
        form: form('hello'),
        hasUserEdited: true,
        isSending: false,
      }),
    ).toBe(true)
    expect(
      shouldPromptBeforeClose({
        form: form(''),
        hasUserEdited: true,
        isSending: false,
      }),
    ).toBe(false)
    expect(
      shouldPromptBeforeClose({
        form: form('hello'),
        hasUserEdited: false,
        isSending: false,
      }),
    ).toBe(false)
    expect(
      shouldPromptBeforeClose({
        form: form('hello'),
        hasUserEdited: true,
        isSending: true,
      }),
    ).toBe(false)
  })
})

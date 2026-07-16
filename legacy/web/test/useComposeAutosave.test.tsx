import { afterEach, describe, expect, it, spyOn } from 'bun:test'
import { act, renderHook } from '@testing-library/react'

import {
  EMPTY_FORM,
  composeAttachmentFromFile,
  type ComposeForm,
} from '../src/components/composeFormHelpers'
import { useComposeAutosave } from '../src/components/compose-overlay/useComposeAutosave'
import { runtimeMutations } from '../src/runtime/mutations'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const resolveSubmissionSourceId = (): string => 'acct-1'

function form(overrides: Partial<ComposeForm> = {}): ComposeForm {
  return { ...EMPTY_FORM, ...overrides }
}

function renderAutosave(args: {
  form?: ComposeForm
  fixedDraftKey?: string
  existingDraftSourceId?: string
  intentKind?: 'new' | 'draft'
}) {
  return renderHook(
    (props: { form: ComposeForm }) =>
      useComposeAutosave({
        form: props.form,
        resetKey: 'primary:new',
        fixedDraftKey: args.fixedDraftKey,
        existingDraftSourceId: args.existingDraftSourceId,
        intentKind: args.intentKind ?? 'new',
        replyContext: undefined,
        resolveSubmissionSourceId,
      }),
    { initialProps: { form: args.form ?? form() } },
  )
}

describe('useComposeAutosave — traditional (no continuous autosave)', () => {
  afterEach(() => {
    // spies are restored per-test below via mockRestore
  })

  it('does NOT push save_draft while the user types — only on explicit save', async () => {
    const saveSpy = spyOn(
      runtimeMutations.messages,
      'saveDraft',
    ).mockResolvedValue({} as never)
    const { result, rerender } = renderAutosave({})

    // Simulate a stream of edits (typing) via re-renders with changing content.
    for (const body of ['h', 'he', 'hel', 'hello']) {
      rerender({ form: form({ body }) })
    }
    // Let any (non-existent) debounced effect settle.
    await act(async () => {
      await Promise.resolve()
    })
    expect(saveSpy).not.toHaveBeenCalled()

    // The explicit close-prompt save is the ONLY thing that persists.
    await act(async () => {
      await result.current.saveDraft()
    })
    expect(saveSpy).toHaveBeenCalledTimes(1)
    saveSpy.mockRestore()
  })

  it('editing an existing draft + Save updates the SAME draft (no twin)', async () => {
    const saveSpy = spyOn(
      runtimeMutations.messages,
      'saveDraft',
    ).mockResolvedValue({} as never)
    const { result } = renderAutosave({
      form: form({ subject: 'Resumed', body: 'edited' }),
      fixedDraftKey: 'draft-123',
      existingDraftSourceId: 'acct-1',
      intentKind: 'draft',
    })

    await act(async () => {
      await result.current.saveDraft()
      await result.current.saveDraft()
    })

    // Both saves key by the resumed draft's id — one draft, not a twin.
    expect(saveSpy).toHaveBeenCalledTimes(2)
    for (const call of saveSpy.mock.calls) {
      expect(call[0].input.draftId).toBe('draft-123')
      expect(call[0].sourceId).toBe('acct-1')
    }
    saveSpy.mockRestore()
  })

  it('a new compose saves under one stable minted key across repeated saves', async () => {
    const saveSpy = spyOn(
      runtimeMutations.messages,
      'saveDraft',
    ).mockResolvedValue({} as never)
    const { result } = renderAutosave({ form: form({ body: 'hi' }) })

    await act(async () => {
      await result.current.saveDraft()
      await result.current.saveDraft()
    })
    const keys = saveSpy.mock.calls.map((c) => c[0].input.draftId)
    expect(keys).toHaveLength(2)
    expect(keys[0]).toBe(keys[1])
    expect(keys[0]).toMatch(/^draft-local-/)
    saveSpy.mockRestore()
  })

  it('send-consumes-draft: discard deletes a resumed draft, no-ops for an unsaved new compose', async () => {
    const deleteSpy = spyOn(
      runtimeMutations.messages,
      'deleteDraft',
    ).mockResolvedValue({} as never)

    // Resumed draft → the server copy is deleted on send.
    const resumed = renderAutosave({
      form: form({ body: 'edited' }),
      fixedDraftKey: 'draft-123',
      existingDraftSourceId: 'acct-1',
      intentKind: 'draft',
    })
    await act(async () => {
      await resumed.result.current.discardDraft()
    })
    expect(deleteSpy).toHaveBeenCalledTimes(1)
    expect(deleteSpy.mock.calls[0][0]).toEqual({
      sourceId: 'acct-1',
      draftId: 'draft-123',
    })
    deleteSpy.mockClear()

    // Brand-new compose that was never saved → nothing to delete.
    const fresh = renderAutosave({ form: form({ body: 'hi' }) })
    await act(async () => {
      await fresh.result.current.discardDraft()
    })
    expect(deleteSpy).not.toHaveBeenCalled()
    deleteSpy.mockRestore()
  })

  it('serializes a pasted attachment into the saved draft (autosave round-trip, save half)', async () => {
    const saveSpy = spyOn(
      runtimeMutations.messages,
      'saveDraft',
    ).mockResolvedValue({} as never)
    const pasted = composeAttachmentFromFile(
      new File(['pasted bytes'], 'pasted-image-1.png', { type: 'image/png' }),
    )
    const { result } = renderAutosave({
      form: form({ body: 'hi', attachments: [pasted] }),
    })

    await act(async () => {
      await result.current.saveDraft()
    })

    // The draft carries the pasted file's name, MIME type and content — the
    // reopen half (attachments seeded back into the form) is covered in
    // composeFormStateForward.test.tsx.
    const saved = saveSpy.mock.calls[0][0].input.message.attachments
    expect(saved).toEqual([
      {
        filename: 'pasted-image-1.png',
        mimeType: 'image/png',
        contentBase64: btoa('pasted bytes'),
      },
    ])
    saveSpy.mockRestore()
  })
})

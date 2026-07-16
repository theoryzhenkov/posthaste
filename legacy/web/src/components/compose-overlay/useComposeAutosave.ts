import { useEffect, useRef, useState } from 'react'

import type { Recipient, ReplyContext } from '@/api/types'
import type { ComposeIntent } from '@/composeIntent'
import { runtimeMutations } from '@/runtime/mutations'

import { buildSendInput, readAttachmentForSend } from '../composeFormHelpers'
import type { ComposeForm } from '../composeFormHelpers'

function mintDraftKey(): string {
  return `draft-local-${crypto.randomUUID()}`
}

/**
 * Compose draft persistence — traditional email-client model.
 *
 * There is NO continuous background autosave. The in-progress message is
 * persisted as a provider draft ONLY on an explicit user action: the
 * close-without-send prompt's "Save as draft" calls {@link saveDraft} once. A
 * single stable `draftKey` per compose session is sent on that save; when
 * resuming an existing draft its id is reused as the key so the save UPDATES
 * that draft (via the runtime's durable alias) rather than spawning a twin
 * (M69). {@link discardDraft} deletes the draft after the message is sent
 * (send-consumes-draft) — for a resumed draft the server copy is deleted, and
 * for a compose saved via the close-prompt the created draft is deleted.
 *
 * @spec docs/L1-outbox#temp-id-reconciliation
 */
export function useComposeAutosave({
  resetKey,
  fixedDraftKey,
  existingDraftSourceId,
  intentKind,
  replyContext,
  form,
  resolveSubmissionSourceId,
}: {
  form: ComposeForm
  resetKey: string
  // When resuming an existing draft, its id is reused as the key so a save
  // updates that draft instead of creating a new one.
  fixedDraftKey: string | undefined
  // The account a resumed draft already lives in, so a send can delete it.
  existingDraftSourceId: string | undefined
  intentKind: ComposeIntent['kind']
  replyContext: ReplyContext | undefined
  resolveSubmissionSourceId: (from: Recipient | null) => string
}) {
  // One stable draft key per compose session: the existing draft's id when
  // resuming, otherwise minted and regenerated when the compose identity
  // (resetKey) changes.
  const [draftKey, setDraftKey] = useState(
    () => fixedDraftKey ?? mintDraftKey(),
  )
  const seenResetKeyRef = useRef(resetKey)
  // The account the draft lives in (for the send-time delete). Seeded from the
  // resumed draft's account; overwritten by the source of an explicit save.
  const savedSourceIdRef = useRef<string | null>(existingDraftSourceId ?? null)
  // Whether a server draft exists for this compose that a send should consume:
  // true when resuming an existing draft, or once a close-prompt save succeeds.
  const serverDraftExistsRef = useRef(Boolean(fixedDraftKey))
  // Whether an explicit save has already run this session — gates the late
  // adoption of the resumed draft's stable id (below).
  const savedOnceRef = useRef(false)
  const savingRef = useRef(false)
  // Set once the draft is finalized (sent/discarded) so a later discard no-ops.
  const finalizedRef = useRef(false)

  // Latest form/context read by the (otherwise stable) save closure. Updated in
  // an effect (never during render) so the React Compiler can optimize freely.
  const stateRef = useRef({ form, intentKind, replyContext })
  useEffect(() => {
    stateRef.current = { form, intentKind, replyContext }
  })

  useEffect(() => {
    if (seenResetKeyRef.current === resetKey) {
      return
    }
    seenResetKeyRef.current = resetKey
    savedSourceIdRef.current = existingDraftSourceId ?? null
    serverDraftExistsRef.current = Boolean(fixedDraftKey)
    savedOnceRef.current = false
    finalizedRef.current = false
    setDraftKey(fixedDraftKey ?? mintDraftKey())
  }, [resetKey, fixedDraftKey, existingDraftSourceId])

  // When resuming a draft, its stable id only arrives once the draft content
  // loads (after the initial fallback to the provider id). Adopt it before the
  // first save — the form cannot be edited until it is seeded, so no save has
  // keyed by the fallback id yet — so a later save coalesces onto one draft.
  useEffect(() => {
    if (fixedDraftKey && fixedDraftKey !== draftKey && !savedOnceRef.current) {
      setDraftKey(fixedDraftKey)
    }
  }, [fixedDraftKey, draftKey])

  // Plain function (not useCallback): the React Compiler memoizes it, and it
  // reads mutable refs the compiler must not see manually memoized. Persists the
  // current form as a provider draft under the stable key — one save, invoked
  // explicitly by the close-without-send prompt.
  const saveDraft = async (): Promise<void> => {
    if (savingRef.current || finalizedRef.current) {
      return
    }
    savingRef.current = true
    try {
      const current = stateRef.current
      const input = buildSendInput(current.form)
      if (current.intentKind === 'reply' && current.replyContext) {
        input.inReplyTo = current.replyContext.inReplyTo
        input.references = current.replyContext.references
      }
      input.attachments = await Promise.all(
        current.form.attachments.map(readAttachmentForSend),
      )
      const sourceId = resolveSubmissionSourceId(input.from)
      savedSourceIdRef.current = sourceId
      await runtimeMutations.messages.saveDraft({
        sourceId,
        input: { draftId: draftKey, message: input },
      })
      savedOnceRef.current = true
      serverDraftExistsRef.current = true
    } catch {
      // Best-effort: leave the content in the form. The compose is closing, so
      // there is no retry; the unsaved content is simply not persisted.
    } finally {
      savingRef.current = false
    }
  }

  /**
   * Delete the draft (after the message is sent, or on explicit discard). A
   * no-op when no server draft exists for this compose (e.g. a brand-new
   * compose that was never saved).
   */
  const discardDraft = async (): Promise<void> => {
    finalizedRef.current = true
    const sourceId = savedSourceIdRef.current
    if (!serverDraftExistsRef.current || !sourceId) {
      return
    }
    serverDraftExistsRef.current = false
    try {
      await runtimeMutations.messages.deleteDraft({
        sourceId,
        draftId: draftKey,
      })
    } catch {
      // Best-effort: a lingering draft is preferable to blocking the send.
    }
  }

  return { draftKey, saveDraft, discardDraft }
}

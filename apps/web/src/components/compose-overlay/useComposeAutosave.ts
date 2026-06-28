import { useEffect, useRef, useState } from 'react'

import type { Recipient, ReplyContext } from '@/api/types'
import type { ComposeIntent } from '@/composeIntent'
import { runtimeMutations } from '@/runtime/mutations'

import {
  buildSendInput,
  readAttachmentForSend,
  type ComposeForm,
} from '../composeFormHelpers'

const AUTOSAVE_DEBOUNCE_MS = 800

/** Whether the form holds anything worth persisting as a draft. */
function formHasContent(form: ComposeForm): boolean {
  return Boolean(
    form.to.trim() ||
    form.cc.trim() ||
    form.bcc.trim() ||
    form.subject.trim() ||
    form.body.trim() ||
    form.attachments.length > 0,
  )
}

/** A change key that excludes attachment bytes (compared by id, not content). */
function formSignature(form: ComposeForm): string {
  return JSON.stringify({
    from: form.from,
    to: form.to,
    cc: form.cc,
    bcc: form.bcc,
    subject: form.subject,
    body: form.body,
    attachments: form.attachments.map((attachment) => attachment.id),
  })
}

function mintDraftKey(): string {
  return `draft-local-${crypto.randomUUID()}`
}

/**
 * Local-first compose autosave.
 *
 * Persists the in-progress message as a provider draft (debounced) so it is not
 * lost on close/crash and is editable offline. A single stable `draftKey` per
 * compose session is sent on every save; the runtime's durable alias maps it to
 * the live provider draft, so repeated edits update one draft rather than
 * creating duplicates.
 *
 * @spec docs/L1-outbox#temp-id-reconciliation
 */
export function useComposeAutosave({
  form,
  ready,
  hasUserEdited,
  resetKey,
  fixedDraftKey,
  intentKind,
  replyContext,
  resolveSubmissionSourceId,
}: {
  form: ComposeForm
  ready: boolean
  hasUserEdited: boolean
  resetKey: string
  // When resuming an existing draft, its id is reused as the key so edits update
  // that draft instead of creating a new one.
  fixedDraftKey: string | undefined
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
  const savedSourceIdRef = useRef<string | null>(null)
  const savedSignatureRef = useRef<string | null>(null)
  const savingRef = useRef(false)
  const pendingRef = useRef(false)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  // The latest signature that has been queued (debounced) but not yet persisted.
  // Drives the flush-on-close so an edit made within the debounce window is not
  // lost when the compose window is closed before the timer fires.
  const pendingSignatureRef = useRef<string | null>(null)
  // Set once the draft is finalized (sent/discarded) so the close flush no-ops.
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
    savedSignatureRef.current = null
    savedSourceIdRef.current = null
    setDraftKey(fixedDraftKey ?? mintDraftKey())
  }, [resetKey, fixedDraftKey])

  // When resuming a draft, its stable id only arrives once the draft content
  // loads (after the initial fallback to the provider id). Adopt it before the
  // first save — the form cannot be edited until it is seeded, so no save has
  // keyed by the fallback id yet — so edits coalesce onto one draft.
  useEffect(() => {
    if (
      fixedDraftKey &&
      fixedDraftKey !== draftKey &&
      savedSignatureRef.current === null
    ) {
      setDraftKey(fixedDraftKey)
    }
  }, [fixedDraftKey, draftKey])

  // Plain functions (not useCallback): the React Compiler memoizes them, and
  // they read mutable refs the compiler must not see manually memoized.
  const saveNow = async (): Promise<void> => {
    if (savingRef.current) {
      pendingRef.current = true
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
    } catch {
      // Best-effort: leave the content in the form and retry on the next edit.
      savedSignatureRef.current = null
    } finally {
      savingRef.current = false
      if (pendingRef.current) {
        pendingRef.current = false
        void saveNow()
      }
    }
  }

  // Computed during render (pure) so the autosave effect depends on a primitive
  // signature rather than the whole form object.
  const signature =
    ready && hasUserEdited && formHasContent(form) ? formSignature(form) : null

  useEffect(() => {
    if (signature === null || signature === savedSignatureRef.current) {
      return
    }
    if (timerRef.current) {
      clearTimeout(timerRef.current)
    }
    pendingSignatureRef.current = signature
    timerRef.current = setTimeout(() => {
      savedSignatureRef.current = signature
      pendingSignatureRef.current = null
      void saveNow()
    }, AUTOSAVE_DEBOUNCE_MS)
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current)
      }
    }
    // Re-run only when the form signature changes; `saveNow` reads the latest
    // state through refs and is stabilized by the React Compiler.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature])

  // Flush a pending edit when the compose session ends (unmount / close). The
  // debounce timer is cleared on unmount, so without this an edit made within
  // the last `AUTOSAVE_DEBOUNCE_MS` would be lost on close. Skipped once the
  // draft has been finalized (sent/discarded). Fire-and-forget: the saveDraft
  // mutation outlives the component.
  useEffect(() => {
    return () => {
      if (!finalizedRef.current && pendingSignatureRef.current !== null) {
        savedSignatureRef.current = pendingSignatureRef.current
        pendingSignatureRef.current = null
        void saveNow()
      }
    }
    // Unmount-only: `saveNow` reads the latest state through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  /**
   * Delete the autosaved draft (after the message is sent, or on explicit
   * discard). No-op if nothing was ever saved.
   */
  const discardDraft = async (): Promise<void> => {
    finalizedRef.current = true
    pendingSignatureRef.current = null
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
    const sourceId = savedSourceIdRef.current
    if (savedSignatureRef.current === null || !sourceId) {
      return
    }
    savedSignatureRef.current = null
    try {
      await runtimeMutations.messages.deleteDraft({
        sourceId,
        draftId: draftKey,
      })
    } catch {
      // Best-effort: a lingering draft is preferable to blocking the send.
    }
  }

  return { discardDraft }
}

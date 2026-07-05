import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type SetStateAction,
} from 'react'

import type { Identity, Recipient, ReplyContext } from '@/api/types'
import type { ComposeIntent } from '@/composeIntent'

import type { MarkdownComposerEditorHandle } from '../MarkdownComposerEditor'
import {
  EMPTY_FORM,
  appendSignature,
  composeAttachmentFromFile,
  formatRecipient,
  formatRecipients,
  type ComposeAttachment,
  type ComposeForm,
} from '../composeFormHelpers'
import { validateAttachmentLimits } from './attachments'

/**
 * Derive the reply-all recipient set: original From + To (minus self) go to
 * `to`, original Cc (minus self) goes to `cc`. Recipients are de-duplicated by
 * email (case-insensitive). Only the primary identity address is excluded;
 * alias exclusion is a follow-up.
 */
function replyAllRecipients(
  replyTo: Recipient[],
  originalTo: Recipient[],
  cc: Recipient[],
  selfEmail: string | undefined,
): { to: Recipient[]; cc: Recipient[] } {
  const self = selfEmail?.toLowerCase()
  const dedupedExcludingSelf = (recipients: Recipient[]): Recipient[] => {
    const seen = new Set<string>()
    const out: Recipient[] = []
    for (const r of recipients) {
      const key = r.email.toLowerCase()
      if (seen.has(key) || (self && key === self)) continue
      seen.add(key)
      out.push(r)
    }
    return out
  }
  return {
    to: dedupedExcludingSelf([...replyTo, ...originalTo]),
    cc: dedupedExcludingSelf(cc),
  }
}

export function useComposeFormState({
  composeKey,
  draftSeed,
  forwardAttachments,
  identity,
  intentKind,
  isMessageBasedCompose,
  replyContext,
  signature,
}: {
  composeKey: string
  draftSeed:
    | {
        from: string
        to: string
        cc: string
        bcc: string
        subject: string
        body: string
      }
    | undefined
  forwardAttachments: ComposeAttachment[]
  identity: Identity | undefined
  intentKind: ComposeIntent['kind']
  isMessageBasedCompose: boolean
  replyContext: ReplyContext | undefined
  signature: string | null
}) {
  const bodyRef = useRef<MarkdownComposerEditorHandle>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const initialForm = useMemo<ComposeForm>(() => {
    if (intentKind === 'draft') {
      return draftSeed
        ? {
            from: draftSeed.from,
            to: draftSeed.to,
            cc: draftSeed.cc,
            bcc: draftSeed.bcc,
            subject: draftSeed.subject,
            body: draftSeed.body,
            attachments: [],
          }
        : EMPTY_FORM
    }
    // FIX2 — a reply/forward starts EMPTY and streams its quoted body +
    // recipients + subject in via the seed effect below (keyed to a STABLE
    // reset key), so the editor is usable the instant it opens and a late
    // `replyContext` never resets the form out from under early typing.
    return EMPTY_FORM
  }, [draftSeed, intentKind])
  // Only a DRAFT resume flips loading→ready (its loaded content REPLACES the
  // empty form). A reply/forward keeps a stable reset key: its quote streams in
  // via an effect, so the form must not reset (which would clobber early edits).
  const formResetKey =
    intentKind === 'draft'
      ? `${composeKey}:${draftSeed ? 'ready' : 'loading'}`
      : composeKey
  const [composeState, setComposeState] = useState(() => ({
    errorMessage: null as string | null,
    form: initialForm,
    resetKey: formResetKey,
  }))
  const [fromMenuOpen, setFromMenuOpen] = useState(false)
  const [fromInputFocused, setFromInputFocused] = useState(false)
  const [editedResetKey, setEditedResetKey] = useState<string | null>(null)
  const [isReadingAttachments, setIsReadingAttachments] = useState(false)
  const editedResetKeyRef = useRef<string | null>(null)
  const seededAttachmentsKeyRef = useRef<string | null>(null)
  const seededSignatureKeyRef = useRef<string | null>(null)

  const needsFormReset = composeState.resetKey !== formResetKey
  const form = needsFormReset ? initialForm : composeState.form
  const errorMessage = needsFormReset ? null : composeState.errorMessage
  const setForm = useCallback(
    (nextForm: SetStateAction<ComposeForm>) => {
      setComposeState((current) => {
        const isCurrentForm = current.resetKey === formResetKey
        const baseForm = isCurrentForm ? current.form : initialForm
        return {
          errorMessage: isCurrentForm ? current.errorMessage : null,
          form: typeof nextForm === 'function' ? nextForm(baseForm) : nextForm,
          resetKey: formResetKey,
        }
      })
    },
    [formResetKey, initialForm],
  )
  const setErrorMessage = useCallback(
    (message: string | null) => {
      setComposeState((current) => {
        const isCurrentForm = current.resetKey === formResetKey
        return {
          errorMessage: message,
          form: isCurrentForm ? current.form : initialForm,
          resetKey: formResetKey,
        }
      })
    },
    [formResetKey, initialForm],
  )
  const setField = useCallback(
    <K extends keyof ComposeForm>(field: K, value: ComposeForm[K]) => {
      editedResetKeyRef.current = formResetKey
      setEditedResetKey(formResetKey)
      setForm((current) => ({ ...current, [field]: value }))
    },
    [formResetKey, setForm],
  )
  const handleBodyChange = useCallback(
    (value: string) => setField('body', value),
    [setField],
  )
  const removeAttachment = useCallback(
    (attachmentId: string) => {
      setField(
        'attachments',
        form.attachments.filter((attachment) => attachment.id !== attachmentId),
      )
    },
    [form.attachments, setField],
  )
  const handleAttachFiles = useCallback(
    (files: FileList | null) => {
      if (!files || files.length === 0) {
        return
      }
      const nextAttachments = [
        ...form.attachments,
        ...Array.from(files).map(composeAttachmentFromFile),
      ]
      const error = validateAttachmentLimits(nextAttachments)
      if (error) {
        setErrorMessage(error)
      } else {
        setErrorMessage(null)
        setField('attachments', nextAttachments)
      }
      if (fileInputRef.current) {
        fileInputRef.current.value = ''
      }
    },
    [form.attachments, setErrorMessage, setField],
  )

  useEffect(() => {
    if (isMessageBasedCompose && replyContext) {
      requestAnimationFrame(() => bodyRef.current?.focus())
    }
  }, [composeKey, isMessageBasedCompose, replyContext])

  useEffect(() => {
    if (!identity || form.from.trim().length > 0) {
      return
    }
    const frame = requestAnimationFrame(() => {
      setForm((current) =>
        current.from.trim().length > 0
          ? current
          : { ...current, from: formatRecipient(identity) },
      )
    })

    return () => cancelAnimationFrame(frame)
  }, [form.from, identity, setForm])

  useEffect(() => {
    if (intentKind !== 'forward' || forwardAttachments.length === 0) {
      return
    }
    if (seededAttachmentsKeyRef.current === formResetKey) {
      return
    }
    seededAttachmentsKeyRef.current = formResetKey
    setForm((current) =>
      current.attachments.length > 0
        ? current
        : { ...current, attachments: forwardAttachments },
    )
  }, [intentKind, forwardAttachments, formResetKey, setForm])

  const seededReplyContextKeyRef = useRef<string | null>(null)
  useEffect(() => {
    // FIX2 — stream the reply/forward quote + recipients + subject into the form
    // once `replyContext` is available (from the cache placeholder or the served
    // fetch), WITHOUT resetting the form: the editor was interactive from the
    // start, so this only FILLS fields the user hasn't touched and APPENDS the
    // quote below any early-typed text. Ref-guarded to seed once per session.
    if (intentKind === 'new' || intentKind === 'draft' || !replyContext) {
      return
    }
    // Reply-all excludes self from the recipient set — wait for the identity so
    // the seeded recipients are correct.
    if (intentKind === 'replyAll' && !identity) {
      return
    }
    if (seededReplyContextKeyRef.current === formResetKey) {
      return
    }
    seededReplyContextKeyRef.current = formResetKey
    const seed =
      intentKind === 'forward'
        ? replyContext.forwardedBody
        : replyContext.quotedBody
    // Reply-all derives the full recipient set (original From + To, plus the
    // original Cc) with the user's own address excluded. A plain reply uses the
    // original From only; forward starts empty.
    const { to, cc } =
      intentKind === 'forward'
        ? { to: [], cc: [] }
        : intentKind === 'replyAll'
          ? replyAllRecipients(
              replyContext.to,
              replyContext.originalTo,
              replyContext.cc,
              identity?.email,
            )
          : { to: replyContext.to, cc: [] }
    const subject =
      intentKind === 'forward'
        ? replyContext.forwardSubject
        : replyContext.replySubject
    setForm((current) => ({
      ...current,
      to: current.to.trim() ? current.to : formatRecipients(to),
      cc: current.cc.trim() ? current.cc : formatRecipients(cc),
      subject: current.subject.trim() ? current.subject : subject,
      body: seed ? `${current.body}\n\n${seed}` : current.body,
    }))
  }, [intentKind, replyContext, identity, formResetKey, setForm])

  useEffect(() => {
    // Seed the account's signature into the body once per fresh composition
    // (new/reply/forward) so it is visible and editable. Skipped for a resumed
    // draft, which carries its own body (and may already include a signature);
    // the ref guard prevents re-inserting it across re-renders or account-list
    // reloads, so a user edit is never clobbered.
    //
    // @spec docs/L1-compose#sender-selection
    if (!signature || intentKind === 'draft') {
      return
    }
    if (seededSignatureKeyRef.current === formResetKey) {
      return
    }
    seededSignatureKeyRef.current = formResetKey
    setForm((current) => ({
      ...current,
      body: appendSignature(current.body, signature),
    }))
  }, [signature, intentKind, formResetKey, setForm])

  return {
    bodyRef,
    editedResetKeyRef,
    errorMessage,
    fileInputRef,
    form,
    formResetKey,
    fromInputFocused,
    fromMenuOpen,
    handleAttachFiles,
    handleBodyChange,
    hasUserEdited: editedResetKey === formResetKey,
    isReadingAttachments,
    removeAttachment,
    setErrorMessage,
    setField,
    setFromInputFocused,
    setFromMenuOpen,
    setIsReadingAttachments,
  }
}

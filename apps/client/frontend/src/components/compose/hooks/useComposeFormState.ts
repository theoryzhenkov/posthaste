/**
 * Wiring between the compose form machine (`form/machine.ts` — the reducer
 * that owns form truth) and the composer's queries/effects. This hook derives
 * named events and dispatches them; the only state it keeps itself is UI
 * ephemera (From menu/focus, the in-flight attachment read) per the charter.
 */
import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from 'react'

import type { Identity, ReplyContext } from '@/data/transport/api'
import type { ComposeIntent, MailtoSeed } from '@/domain/composeIntent'

import type { MarkdownComposerEditorHandle } from '../editor/MarkdownComposerEditor'
import {
  composeAttachmentFromFile,
  deriveReplySeed,
  formatRecipient,
  formatRecipients,
  initialComposeForm,
  type ComposeForm,
  type ComposeAttachment,
  type DraftSeed,
} from '../form/model'
import {
  composeView,
  initialComposeMachineState,
  reduceCompose,
  type ComposeEvent,
  type ComposeSession,
} from '../form/machine'
import { withPastedFileName } from '../attachments/attachments'

export function useComposeFormState({
  composeKey,
  draftSeed,
  forwardAttachments,
  identity,
  intentKind,
  isMessageBasedCompose,
  mailtoSeed,
  replyContext,
  signature,
}: {
  composeKey: string
  draftSeed: DraftSeed | undefined
  forwardAttachments: ComposeAttachment[]
  identity: Identity | undefined
  intentKind: ComposeIntent['kind']
  isMessageBasedCompose: boolean
  /** Parsed `mailto:` prefill (the `mailto` intent); undefined otherwise. */
  mailtoSeed?: MailtoSeed
  replyContext: ReplyContext | undefined
  signature: string | null
}) {
  const bodyRef = useRef<MarkdownComposerEditorHandle>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const initialForm = useMemo<ComposeForm>(
    () => initialComposeForm({ draftSeed, intentKind, mailtoSeed }),
    [draftSeed, intentKind, mailtoSeed],
  )
  // Only a DRAFT resume flips loading→ready (its loaded content REPLACES the
  // empty form). A reply/forward keeps a stable reset key: its quote streams in
  // via an effect, so the form must not reset (which would clobber early edits).
  const formResetKey =
    intentKind === 'draft'
      ? `${composeKey}:${draftSeed ? 'ready' : 'loading'}`
      : composeKey
  const session = useMemo<ComposeSession>(
    () => ({ resetKey: formResetKey, initialForm }),
    [formResetKey, initialForm],
  )
  const [machineState, dispatch] = useReducer(
    (
      state: ReturnType<typeof initialComposeMachineState>,
      action: { session: ComposeSession; event: ComposeEvent },
    ) => reduceCompose(state, action.session, action.event),
    session,
    initialComposeMachineState,
  )
  const send = useCallback(
    (event: ComposeEvent) => dispatch({ session, event }),
    [session],
  )
  // UI ephemera — deliberately outside the machine.
  const [fromMenuOpen, setFromMenuOpen] = useState(false)
  const [fromInputFocused, setFromInputFocused] = useState(false)
  const [isReadingAttachments, setIsReadingAttachments] = useState(false)

  const view = composeView(machineState, session)
  const { form, errorMessage, hasUserEdited } = view

  // The last reset key that saw a user edit, read outside React's render
  // cycle (caret pinning below, the window-elevation close decision).
  const editedResetKeyRef = useRef<string | null>(null)
  useEffect(() => {
    if (hasUserEdited) {
      editedResetKeyRef.current = formResetKey
    }
  }, [hasUserEdited, formResetKey])

  const setErrorMessage = useCallback(
    (message: string | null) => send({ type: 'errorReported', message }),
    [send],
  )
  const setField = useCallback(
    <K extends keyof ComposeForm>(field: K, value: ComposeForm[K]) => {
      editedResetKeyRef.current = formResetKey
      send({ type: 'fieldChanged', field, value } as ComposeEvent)
    },
    [formResetKey, send],
  )
  const handleBodyChange = useCallback(
    (value: string) => setField('body', value),
    [setField],
  )
  const removeAttachment = useCallback(
    (attachmentId: string) => {
      editedResetKeyRef.current = formResetKey
      send({ type: 'attachmentRemoved', attachmentId })
    },
    [formResetKey, send],
  )
  // Monotonic per-session ordinal for naming unnamed pasted files
  // (`pasted-image-<n>.png`), so two screenshots never collide.
  const pastedFileOrdinalRef = useRef(0)
  /**
   * Shared attachment ingestion for every entry path — the picker, paste
   * (Cmd+V) into the body editor or the fields, and drag-and-drop onto the
   * composer. Unnamed clipboard images get a generated name; the machine
   * enforces the send-path size caps, surfacing the over-limit message in the
   * footer instead of failing silently at send.
   */
  const ingestFiles = useCallback(
    (files: File[]) => {
      if (files.length === 0) {
        return
      }
      editedResetKeyRef.current = formResetKey
      send({
        type: 'attachmentsAdded',
        attachments: files.map((file) =>
          composeAttachmentFromFile(
            withPastedFileName(file, ++pastedFileOrdinalRef.current),
          ),
        ),
      })
    },
    [formResetKey, send],
  )
  const handleAttachFiles = useCallback(
    (files: FileList | null) => {
      ingestFiles(files ? Array.from(files) : [])
      if (fileInputRef.current) {
        fileInputRef.current.value = ''
      }
    },
    [ingestFiles],
  )

  useEffect(() => {
    if (isMessageBasedCompose && replyContext) {
      requestAnimationFrame(() => {
        // Top-posting: the caret starts on the empty line ABOVE the signature
        // and quote. Only pin it while the body is still untouched — once the
        // user has typed, refocusing (e.g. the placeholder→served context
        // transition) must not yank the caret away.
        if (editedResetKeyRef.current === formResetKey) {
          bodyRef.current?.focus()
        } else {
          bodyRef.current?.focusAtStart()
        }
      })
    }
  }, [composeKey, isMessageBasedCompose, replyContext, formResetKey])

  useEffect(() => {
    if (!identity || form.from.trim().length > 0) {
      return
    }
    const frame = requestAnimationFrame(() => {
      send({ type: 'identityDefaulted', from: formatRecipient(identity) })
    })

    return () => cancelAnimationFrame(frame)
  }, [form.from, identity, send])

  useEffect(() => {
    // A forward re-sends the original files; a resumed draft restores the files
    // it was saved with (without this, re-saving the draft would drop them —
    // the attachment round-trip for pasted/picked files depends on it).
    if (
      (intentKind !== 'forward' && intentKind !== 'draft') ||
      forwardAttachments.length === 0
    ) {
      return
    }
    send({ type: 'forwardAttachmentsSeeded', attachments: forwardAttachments })
  }, [intentKind, forwardAttachments, send])

  useEffect(() => {
    // FIX2 — stream the reply/forward quote + recipients + subject into the form
    // once `replyContext` is available (from the cache placeholder or the served
    // fetch), WITHOUT resetting the form: the editor was interactive from the
    // start, so this only FILLS fields the user hasn't touched and APPENDS the
    // quote below any early-typed text. The machine seeds once per session.
    if (
      intentKind === 'new' ||
      intentKind === 'draft' ||
      intentKind === 'mailto' ||
      !replyContext
    ) {
      return
    }
    // Reply-all excludes self from the recipient set — wait for the identity so
    // the seeded recipients are correct.
    if (intentKind === 'replyAll' && !identity) {
      return
    }
    const seed = deriveReplySeed(intentKind, replyContext, identity?.email)
    send({
      type: 'replyContextSeeded',
      to: formatRecipients(seed.to),
      cc: formatRecipients(seed.cc),
      subject: seed.subject,
      quoteBlock: seed.quoteBlock,
    })
  }, [intentKind, replyContext, identity, send])

  useEffect(() => {
    // Seed the account's signature into the body once per fresh composition
    // (new/reply/forward) so it is visible and editable. Skipped for a resumed
    // draft, which carries its own body (and may already include a signature);
    // the machine's seed-once guard prevents re-inserting it across re-renders
    // or account-list reloads, so a user edit is never clobbered.
    //
    // A new message appends at the end (which IS the top — there is no quote).
    // A reply/reply-all/forward top-posts: the signature goes ABOVE the seeded
    // quote/forward block. When the quote has not streamed in yet the signature
    // is appended at the end and the later quote lands below it — the same
    // final order either way.
    if (!signature || intentKind === 'draft') {
      return
    }
    send({
      type: 'signatureSeeded',
      signature,
      // A mailto compose has no quote — the signature appends like `new`.
      placement:
        intentKind === 'new' || intentKind === 'mailto' ? 'append' : 'aboveQuote',
    })
  }, [signature, intentKind, send])

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
    hasUserEdited,
    ingestFiles,
    isReadingAttachments,
    removeAttachment,
    setErrorMessage,
    setField,
    setFromInputFocused,
    setFromMenuOpen,
    setIsReadingAttachments,
  }
}

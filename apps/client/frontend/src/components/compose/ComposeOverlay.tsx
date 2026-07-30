/**
 * Compose and reply overlay backed by the Rust JMAP send API.
 *
 */
import { useCallback, useMemo, useState } from 'react'

import { parseMailtoUri, type ComposeIntent } from '@/domain/composeIntent'
import { useCommandScope, type CommandScope } from '@/lib/command'
import { useBeforeUnloadGuard } from '@/lib/dom'

import { FloatingPanel } from '../floating/FloatingPanel'
import { ComposeAttachmentList } from './attachments/ComposeAttachmentList'
import { ComposeBodyEditor } from './editor/ComposeBodyEditor'
import { ComposeCloseConfirmDialog } from './shell/ComposeCloseConfirmDialog'
import { ComposeFields } from './form/ComposeFields'
import { ComposeFooter } from './shell/ComposeFooter'
import { ComposeHeader } from './shell/ComposeHeader'
import { shouldPromptBeforeClose } from './shell/composeCloseGuard'
import { useComposeAutosave } from './hooks/useComposeAutosave'
import { useComposeFileDrop } from './attachments/useComposeFileDrop'
import { useComposeFormState } from './hooks/useComposeFormState'
import { useComposeQueries } from './hooks/useComposeQueries'
import { useDisplayedFromOptions } from './form/useDisplayedFromOptions'
import { useComposeSubmission } from './hooks/useComposeSubmission'
import { useComposeWindowElevation } from './hooks/useComposeWindowElevation'
import { useForwardAttachments } from './attachments/useForwardAttachments'

interface ComposeOverlayProps {
  intent: ComposeIntent
  shell?: 'floating' | 'document'
  onClose: () => void
  /**
   * Reopen the composer on a kept draft after a held send is undone
   * (the undo-restores-compose path). Optional: shells without a compose
   * router (e.g. a focused window) fall back to the draft simply remaining
   * in Drafts.
   */
  onRestoreDraft?: (sourceId: string, draftKey: string) => void
}

export function ComposeOverlay({
  intent,
  shell = 'floating',
  onClose,
  onRestoreDraft,
}: ComposeOverlayProps) {
  const queries = useComposeQueries({ intent })
  const forwardAttachments = useForwardAttachments({ intent })
  const signature = queries.signature
  const mailtoSeed = useMemo(
    () =>
      intent.kind === 'mailto' ? parseMailtoUri(intent.mailtoUri) : undefined,
    [intent],
  )
  const formState = useComposeFormState({
    composeKey: queries.composeKey,
    draftSeed: queries.draftSeed,
    forwardAttachments: forwardAttachments.attachments,
    identity: queries.identity,
    intentKind: intent.kind,
    isMessageBasedCompose: queries.isMessageBasedCompose,
    mailtoSeed,
    replyContext: queries.replyContextQuery.data,
    signature,
  })
  const displayedFromOptions = useDisplayedFromOptions({
    formFrom: formState.form.from,
    fromMenuOpen: formState.fromMenuOpen,
    fromOptions: queries.fromOptions,
  })

  // A DRAFT resume still gates the composer (its loaded content replaces the
  // form). A reply/forward no longer does (FIX2): the editor + fields are usable
  // immediately and the quoted body streams in when `replyContext` settles.
  const isWaitingForDraftSeed = queries.isDraftEdit && !queries.draftSeed
  // The reply/forward quote is still being prepared: the anchored message's
  // detail answer has not arrived yet. A message the reader pane already
  // holds shares the same query entry, so the quote seeds instantly.
  const isQuotePending =
    queries.requiresMessageContext &&
    !queries.replyContextQuery.data &&
    !queries.replyContextQuery.isError
  // Only a draft resume (or a forward-attachment read) blocks the editor/fields.
  const isEditorPreparing =
    (isWaitingForDraftSeed && !queries.draftSeedQuery.isError) ||
    forwardAttachments.isLoading
  // The gated content itself failed to load (draft seed / forward attachments) —
  // a full error, not the subtle quote-notice a reply shows.
  const isEditorPreparingError =
    (queries.isDraftEdit && queries.draftSeedQuery.isError) ||
    forwardAttachments.isError
  const fieldsDisabled = isEditorPreparing || isEditorPreparingError
  // SEND / autosave readiness: the anchored message's context (threading
  // headers + quote) must have arrived before a reply/forward submits. This
  // gates submission + autosave, not the editor. A quote-fetch error clears
  // it (send degrades to no-quote rather than blocking forever).
  const isPreparingMessage =
    (isWaitingForDraftSeed || isQuotePending || forwardAttachments.isLoading) &&
    !queries.replyContextQuery.isError &&
    !queries.draftSeedQuery.isError
  // A subtle, non-blocking banner above the (already-usable) reply/forward
  // editor while the served quote streams in — or if it failed to load.
  const quoteNoticeLabel = isQuotePending
    ? intent.kind === 'forward'
      ? 'Adding forwarded text...'
      : 'Adding quoted text...'
    : queries.replyContextQuery.isError && queries.requiresMessageContext
      ? 'Could not load the quoted text; you can still send your reply.'
      : null
  const preparingLabel =
    intent.kind === 'forward'
      ? 'Preparing forward...'
      : intent.kind === 'draft'
        ? 'Loading draft...'
        : 'Preparing reply...'
  const fromLabel = formState.form.from.trim()
    ? formState.form.from
    : queries.identityQuery.isError
      ? 'Sender unavailable'
      : queries.identity?.name
        ? `${queries.identity.name} <${queries.identity.email}>`
        : (queries.identity?.email ?? 'Loading sender...')

  const autosave = useComposeAutosave({
    form: formState.form,
    resetKey: formState.formResetKey,
    // Resume keys by the draft's stable identity once loaded, falling back to
    // its provider id (legacy drafts without the header, or before load). The
    // backend edits an existing provider draft in place either way.
    fixedDraftKey:
      intent.kind === 'draft'
        ? (queries.draftSeedDraftId ?? intent.messageId)
        : undefined,
    // The account a resumed draft already lives in, so a send can delete it.
    existingDraftSourceId:
      intent.kind === 'draft' ? intent.sourceId : undefined,
    intentKind: intent.kind,
    replyContext: queries.replyContextQuery.data,
    resolveSubmissionSourceId: queries.resolveSubmissionSourceId,
  })

  const { handleSubmit, isSending } = useComposeSubmission({
    draftKey: autosave.draftKey,
    form: formState.form,
    intentKind: intent.kind,
    isPreparingMessage,
    onClose,
    // Held (undo-send) sends persist the compose as a draft first, so Undo
    // can restore it with full fidelity.
    onRestoreDraft,
    onSent: autosave.discardDraft,
    replyContext: queries.replyContextQuery.data,
    resolveSubmissionSourceId: queries.resolveSubmissionSourceId,
    setErrorMessage: formState.setErrorMessage,
    setIsReadingAttachments: formState.setIsReadingAttachments,
  })
  // Traditional close flow: closing a dirty compose without sending prompts to
  // save it as a draft. Empty/unchanged composes (and the post-send close, which
  // the send path invokes directly) close with no prompt.
  const [showCloseConfirm, setShowCloseConfirm] = useState(false)
  const requestClose = useCallback(() => {
    if (
      shouldPromptBeforeClose({
        form: formState.form,
        hasUserEdited: formState.hasUserEdited,
        isSending,
      })
    ) {
      setShowCloseConfirm(true)
      return
    }
    onClose()
  }, [formState.form, formState.hasUserEdited, isSending, onClose])
  const handleKeepEditing = useCallback(() => setShowCloseConfirm(false), [])
  // While the confirm dialog is up the panel's own dismissal is disabled:
  // Escape and click-away belong to the prompt (the dialog consumes Escape
  // itself), not to the compose surface behind it.
  const ignoreClose = useCallback(() => {}, [])
  const handleDiscardOnClose = useCallback(() => {
    // Discard the unsaved edits — for a resumed draft the existing draft is left
    // untouched (this is not the trash/discard-draft action).
    setShowCloseConfirm(false)
    onClose()
  }, [onClose])
  const handleSaveAsDraft = useCallback(() => {
    setShowCloseConfirm(false)
    void (async () => {
      await autosave.saveDraft()
      onClose()
    })()
  }, [autosave, onClose])

  // Safety net: warn on a full tab/app close while a dirty compose is open. The
  // traditional model has no continuous autosave, so this is the only guard
  // against losing content to a hard navigation.
  useBeforeUnloadGuard(
    shouldPromptBeforeClose({
      form: formState.form,
      hasUserEdited: formState.hasUserEdited,
      isSending: false,
    }),
  )

  const { isOpeningWindow, openInitialComposeInWindow } =
    useComposeWindowElevation({
      editedResetKeyRef: formState.editedResetKeyRef,
      formResetKey: formState.formResetKey,
      intent,
      onClose,
    })

  // ⌘Enter sends via the command dispatcher (`compose.send`): the scope binds
  // the active submission for exactly as long as this composer is mounted.
  const sendScope = useMemo<CommandScope>(
    () => ({
      owner: 'overlay',
      services: { compose: { send: () => handleSubmit() } },
    }),
    [handleSubmit],
  )
  useCommandScope(sendScope)

  // Paste (Cmd+V) and drag-and-drop attach files through the same ingestion
  // path as the picker; disabled while the composer is not ready or sending.
  const ingestFiles =
    !fieldsDisabled && !isSending ? formState.ingestFiles : undefined
  const { isDragActive, dropZoneProps } = useComposeFileDrop(ingestFiles)

  const panelLabel =
    intent.kind === 'reply'
      ? 'reply composer'
      : intent.kind === 'forward'
        ? 'forward composer'
        : intent.kind === 'draft'
          ? 'draft composer'
          : 'message composer'
  const header = (
    <ComposeHeader fromLabel={fromLabel} intentKind={intent.kind} />
  )
  const content = (
    <div className="relative flex min-h-0 flex-1 flex-col" {...dropZoneProps}>
      <ComposeFields
        displayedFromOptions={displayedFromOptions}
        fieldsDisabled={fieldsDisabled}
        form={formState.form}
        fromInputFocused={formState.fromInputFocused}
        fromMenuOpen={formState.fromMenuOpen}
        intentKind={intent.kind}
        recipientSuggestions={queries.recipientSuggestions}
        setFromInputFocused={formState.setFromInputFocused}
        setFromMenuOpen={formState.setFromMenuOpen}
        onFieldChange={formState.setField}
        onPasteFiles={ingestFiles}
      />
      <ComposeBodyEditor
        bodyRef={formState.bodyRef}
        isPreparingMessage={isEditorPreparing}
        isPreparingError={isEditorPreparingError}
        noticeLabel={quoteNoticeLabel}
        preparingLabel={preparingLabel}
        value={formState.form.body}
        onChange={formState.handleBodyChange}
        onFiles={ingestFiles}
      />
      <ComposeAttachmentList
        attachments={formState.form.attachments}
        fieldsDisabled={fieldsDisabled}
        isReadingAttachments={formState.isReadingAttachments}
        isSending={isSending}
        onRemoveAttachment={formState.removeAttachment}
      />
      <ComposeFooter
        errorMessage={formState.errorMessage}
        fieldsDisabled={fieldsDisabled}
        fileInputRef={formState.fileInputRef}
        isReadingAttachments={formState.isReadingAttachments}
        isSending={isSending}
        statusLabel={
          isEditorPreparing
            ? preparingLabel
            : isQuotePending
              ? (quoteNoticeLabel ?? 'Ready')
              : 'Ready'
        }
        onAttachFiles={formState.handleAttachFiles}
        onClose={requestClose}
        onSubmit={handleSubmit}
      />
      {isDragActive ? (
        <div className="pointer-events-none absolute inset-1 z-20 flex items-center justify-center rounded-lg border-2 border-dashed border-ring bg-background/75 text-sm font-medium text-foreground">
          Drop files to attach
        </div>
      ) : null}
      {/* Scoped to the compose surface: the prompt's scrim covers only this
          composer (a confirmation appears over the window it affects); the
          rest of the app stays interactive and the panel header keeps
          working (move/pin). */}
      <ComposeCloseConfirmDialog
        open={showCloseConfirm}
        intentKind={intent.kind}
        onKeepEditing={handleKeepEditing}
        onDiscard={handleDiscardOnClose}
        onSaveAsDraft={handleSaveAsDraft}
      />
    </div>
  )

  if (shell === 'document') {
    return (
      <div className="flex h-full min-h-0 flex-col bg-background text-foreground">
        <div className="surface-chrome shrink-0 border-b text-chrome-foreground">
          {header}
        </div>
        {content}
      </div>
    )
  }

  return (
    <FloatingPanel
      panelLabel={panelLabel}
      storageKey="posthaste.compose.panelOffset"
      sizePreset="compose"
      className="flex flex-col"
      header={header}
      // While the confirm prompt is up it owns Escape/click-away — the panel's
      // own dismissal must not close the compose underneath it.
      onClose={showCloseConfirm ? ignoreClose : requestClose}
      onOpenInWindow={
        !formState.hasUserEdited && !isOpeningWindow
          ? openInitialComposeInWindow
          : undefined
      }
    >
      {content}
    </FloatingPanel>
  )
}

/**
 * Compose and reply overlay backed by the Rust JMAP send API.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L1-compose#mime-structure
 */
import { useCallback, useEffect, useState } from 'react'

import type { ComposeIntent } from '@/composeIntent'

import { FloatingPanel } from './FloatingPanel'
import { ComposeAttachmentList } from './compose-overlay/ComposeAttachmentList'
import { ComposeBodyEditor } from './compose-overlay/ComposeBodyEditor'
import { ComposeCloseConfirmDialog } from './compose-overlay/ComposeCloseConfirmDialog'
import { ComposeFields } from './compose-overlay/ComposeFields'
import { ComposeFooter } from './compose-overlay/ComposeFooter'
import { ComposeHeader } from './compose-overlay/ComposeHeader'
import { shouldPromptBeforeClose } from './compose-overlay/composeCloseGuard'
import { useComposeAutosave } from './compose-overlay/useComposeAutosave'
import { useComposeFormState } from './compose-overlay/useComposeFormState'
import { useComposeQueries } from './compose-overlay/useComposeQueries'
import { useDisplayedFromOptions } from './compose-overlay/useDisplayedFromOptions'
import { useComposeSubmission } from './compose-overlay/useComposeSubmission'
import { useComposeWindowElevation } from './compose-overlay/useComposeWindowElevation'
import { useForwardAttachments } from './compose-overlay/useForwardAttachments'

interface ComposeOverlayProps {
  intent: ComposeIntent
  shell?: 'floating' | 'document'
  onClose: () => void
}

export function ComposeOverlay({
  intent,
  shell = 'floating',
  onClose,
}: ComposeOverlayProps) {
  const queries = useComposeQueries({ intent })
  const forwardAttachments = useForwardAttachments({ intent })
  const signature =
    queries.accountsQuery.data?.find(
      (account) => account.id === intent.sourceId,
    )?.signature ?? null
  const formState = useComposeFormState({
    composeKey: queries.composeKey,
    draftSeed: queries.draftSeed,
    forwardAttachments: forwardAttachments.attachments,
    identity: queries.identityQuery.data,
    intentKind: intent.kind,
    isMessageBasedCompose: queries.isMessageBasedCompose,
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
  // The reply/forward quote is still being prepared: no served context yet, or
  // only a cache PLACEHOLDER so far (the authoritative fetch is still in flight).
  const isQuotePending =
    queries.requiresMessageContext &&
    (!queries.replyContextQuery.data ||
      queries.replyContextQuery.isPlaceholderData) &&
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
  // SEND / autosave readiness: the message must be AUTHORITATIVELY prepared
  // (real In-Reply-To/References + Cc), not a cache placeholder — so a send
  // never uses provisional threading. This gates submission + autosave, not the
  // editor. A quote-fetch error clears it (send degrades to no-quote rather than
  // blocking forever).
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
      : queries.identityQuery.data?.name
        ? `${queries.identityQuery.data.name} <${queries.identityQuery.data.email}>`
        : (queries.identityQuery.data?.email ?? 'Loading sender...')

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
    form: formState.form,
    intentKind: intent.kind,
    isPreparingMessage,
    onClose,
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
  // While the confirm dialog is up the panel's own dismissal is disabled so it
  // does not race Radix's overlay/Escape handling (a panel pointerdown fires
  // before a dialog button's click and would tear the dialog down first).
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
  useEffect(() => {
    const isDirty = shouldPromptBeforeClose({
      form: formState.form,
      hasUserEdited: formState.hasUserEdited,
      isSending: false,
    })
    if (!isDirty) {
      return
    }
    function handleBeforeUnload(event: BeforeUnloadEvent) {
      event.preventDefault()
      event.returnValue = ''
    }
    window.addEventListener('beforeunload', handleBeforeUnload)
    return () => window.removeEventListener('beforeunload', handleBeforeUnload)
  }, [formState.form, formState.hasUserEdited])

  const { isOpeningWindow, openInitialComposeInWindow } =
    useComposeWindowElevation({
      editedResetKeyRef: formState.editedResetKeyRef,
      formResetKey: formState.formResetKey,
      intent,
      onClose,
    })

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
        event.preventDefault()
        handleSubmit()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [handleSubmit])

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
    <>
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
      />
      <ComposeBodyEditor
        bodyRef={formState.bodyRef}
        isPreparingMessage={isEditorPreparing}
        isPreparingError={isEditorPreparingError}
        noticeLabel={quoteNoticeLabel}
        preparingLabel={preparingLabel}
        value={formState.form.body}
        onChange={formState.handleBodyChange}
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
    </>
  )

  const closeConfirmDialog = (
    <ComposeCloseConfirmDialog
      open={showCloseConfirm}
      intentKind={intent.kind}
      onKeepEditing={handleKeepEditing}
      onDiscard={handleDiscardOnClose}
      onSaveAsDraft={handleSaveAsDraft}
    />
  )

  if (shell === 'document') {
    return (
      <div className="flex h-full min-h-0 flex-col bg-background text-foreground">
        <div className="shrink-0 border-b border-border/70 bg-chrome text-chrome-foreground">
          {header}
        </div>
        {content}
        {closeConfirmDialog}
      </div>
    )
  }

  return (
    <>
      <FloatingPanel
        panelLabel={panelLabel}
        storageKey="posthaste.compose.panelOffset"
        sizePreset="compose"
        className="flex flex-col"
        header={header}
        // While the confirm dialog is up, Radix owns dismissal (Escape / overlay)
        // — disable the panel's own Escape/click-away so the two don't fight.
        onClose={showCloseConfirm ? ignoreClose : requestClose}
        onOpenInWindow={
          !formState.hasUserEdited && !isOpeningWindow
            ? openInitialComposeInWindow
            : undefined
        }
      >
        {content}
      </FloatingPanel>
      {closeConfirmDialog}
    </>
  )
}

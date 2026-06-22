/**
 * Compose and reply overlay backed by the Rust JMAP send API.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L1-compose#mime-structure
 */
import { useEffect } from 'react'

import type { ComposeIntent } from '@/composeIntent'

import { FloatingPanel } from './FloatingPanel'
import { ComposeAttachmentList } from './compose-overlay/ComposeAttachmentList'
import { ComposeBodyEditor } from './compose-overlay/ComposeBodyEditor'
import { ComposeFields } from './compose-overlay/ComposeFields'
import { ComposeFooter } from './compose-overlay/ComposeFooter'
import { ComposeHeader } from './compose-overlay/ComposeHeader'
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
  const formState = useComposeFormState({
    composeKey: queries.composeKey,
    draftSeed: queries.draftSeed,
    forwardAttachments: forwardAttachments.attachments,
    identity: queries.identityQuery.data,
    intentKind: intent.kind,
    isMessageBasedCompose: queries.isMessageBasedCompose,
    replyContext: queries.replyContextQuery.data,
  })
  const displayedFromOptions = useDisplayedFromOptions({
    formFrom: formState.form.from,
    fromMenuOpen: formState.fromMenuOpen,
    fromOptions: queries.fromOptions,
  })

  const isWaitingForMessageContext =
    (queries.requiresMessageContext && !queries.replyContextQuery.data) ||
    (queries.isDraftEdit && !queries.draftSeed)
  const isPreparingMessage =
    (isWaitingForMessageContext &&
      !queries.replyContextQuery.isError &&
      !queries.draftSeedQuery.isError) ||
    forwardAttachments.isLoading
  const fieldsDisabled =
    isWaitingForMessageContext || forwardAttachments.isLoading
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
    ready: !isPreparingMessage,
    hasUserEdited: formState.hasUserEdited,
    resetKey: formState.formResetKey,
    fixedDraftKey: intent.kind === 'draft' ? intent.messageId : undefined,
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
        isPreparingMessage={isPreparingMessage}
        isReplyContextError={
          queries.replyContextQuery.isError || forwardAttachments.isError
        }
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
        statusLabel={isPreparingMessage ? preparingLabel : 'Ready'}
        onAttachFiles={formState.handleAttachFiles}
        onClose={onClose}
        onSubmit={handleSubmit}
      />
    </>
  )

  if (shell === 'document') {
    return (
      <div className="flex h-full min-h-0 flex-col bg-background text-foreground">
        <div className="shrink-0 border-b border-border/70 bg-chrome text-chrome-foreground">
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
      zIndexClassName="z-[80]"
      sizePreset="compose"
      className="flex flex-col"
      header={header}
      onClose={onClose}
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

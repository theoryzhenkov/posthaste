import type { ComposeIntent } from '@/composeIntent'

import type { ComposeForm } from '../composeFormHelpers'

/** Whether the form holds anything worth persisting as a draft. */
export function composeFormHasContent(form: ComposeForm): boolean {
  return Boolean(
    form.to.trim() ||
    form.cc.trim() ||
    form.bcc.trim() ||
    form.subject.trim() ||
    form.body.trim() ||
    form.attachments.length > 0,
  )
}

/**
 * Whether closing the compose should first prompt to save a draft. A send in
 * flight never prompts (the message is on its way / the send path closes
 * directly); otherwise prompt only when the user has made edits AND there is
 * content worth keeping.
 */
export function shouldPromptBeforeClose({
  form,
  hasUserEdited,
  isSending,
}: {
  form: ComposeForm
  hasUserEdited: boolean
  isSending: boolean
}): boolean {
  if (isSending) {
    return false
  }
  return hasUserEdited && composeFormHasContent(form)
}

export interface ComposeCloseCopy {
  title: string
  description: string
  saveLabel: string
}

/**
 * Dialog copy for the close-without-send prompt. Resuming an existing draft is
 * worded as saving CHANGES; a fresh compose is worded as saving the message as
 * a new draft.
 */
export function composeCloseCopy(
  intentKind: ComposeIntent['kind'],
): ComposeCloseCopy {
  if (intentKind === 'draft') {
    return {
      title: 'Save changes to this draft?',
      description: "Your changes will be lost if you don't save them.",
      saveLabel: 'Save changes',
    }
  }
  return {
    title: 'Save this message as a draft?',
    description: 'You can finish it later from your Drafts.',
    saveLabel: 'Save as draft',
  }
}

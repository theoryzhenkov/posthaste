/**
 * The compose form's state machine: one pure reducer over named events owns
 * form truth (field values, attachments, seeding, the error line, whether the
 * user has edited). The hook layer (`useComposeFormState`) is wiring: it
 * derives events from queries/effects and dispatches; UI ephemera (menu open,
 * focus, in-flight attachment reads) stay component state per the charter.
 *
 * SESSIONS: a compose session is identified by its reset key. Every dispatch
 * carries the current session; the reducer normalizes first — an event
 * arriving after the key changed starts from that session's initial form, so
 * a stale update can never resurrect the previous composition. Reading state
 * for render goes through {@link composeView} with the same rule.
 *
 * SEED-ONCE: the reply/forward quote, the forwarded/resumed attachments, and
 * the signature each stream in exactly once per session, guarded here (not by
 * effect-order), and only ever FILL fields the user has not touched.
 */
import { validateAttachmentLimits } from '../attachments/attachments'
import {
  appendSignature,
  insertSignatureAboveQuote,
  type ComposeAttachment,
  type ComposeForm,
} from './model'

/** The compose session identity a dispatch happens under. */
export interface ComposeSession {
  resetKey: string
  initialForm: ComposeForm
}

export interface ComposeMachineState {
  resetKey: string
  form: ComposeForm
  errorMessage: string | null
  /** Whether THIS session's form carries user edits (drives close-prompt and
   * seed-fill guards). */
  hasUserEdited: boolean
  seededForwardAttachments: boolean
  seededReplyContext: boolean
  seededSignature: boolean
  /** The exact quote block this session seeded; the signature inserts ABOVE
   * it when the quote arrived first, keeping signature-above-quote in either
   * seeding order. */
  seededQuoteBlock: string | null
}

/** One field edit, typed per field so `value` matches `field`. */
type FieldChanged = {
  [K in keyof ComposeForm]: { type: 'fieldChanged'; field: K; value: ComposeForm[K] }
}[keyof ComposeForm]

export type ComposeEvent =
  | FieldChanged
  | { type: 'attachmentsAdded'; attachments: ComposeAttachment[] }
  | { type: 'attachmentRemoved'; attachmentId: string }
  /** Default the From line to the account identity — a fill, not an edit. */
  | { type: 'identityDefaulted'; from: string }
  /** Restore a forward's / resumed draft's files, once, if none were added. */
  | { type: 'forwardAttachmentsSeeded'; attachments: ComposeAttachment[] }
  /** Stream the reply/forward context in: fill untouched recipient/subject
   * fields, append the quote below any early-typed text. */
  | {
      type: 'replyContextSeeded'
      to: string
      cc: string
      subject: string
      quoteBlock: string | null
    }
  /** Insert the account signature: at the end for a fresh message, above the
   * seeded quote for a reply/forward. */
  | { type: 'signatureSeeded'; signature: string; placement: 'append' | 'aboveQuote' }
  | { type: 'errorReported'; message: string | null }

export function initialComposeMachineState(
  session: ComposeSession,
): ComposeMachineState {
  return {
    resetKey: session.resetKey,
    form: session.initialForm,
    errorMessage: null,
    hasUserEdited: false,
    seededForwardAttachments: false,
    seededReplyContext: false,
    seededSignature: false,
    seededQuoteBlock: null,
  }
}

/** The state as the current session sees it: a stale-session state reads as
 * that session's fresh start (the reducer applies the same normalization on
 * the next dispatch). */
export function composeView(
  state: ComposeMachineState,
  session: ComposeSession,
): ComposeMachineState {
  return state.resetKey === session.resetKey
    ? state
    : initialComposeMachineState(session)
}

export function reduceCompose(
  state: ComposeMachineState,
  session: ComposeSession,
  event: ComposeEvent,
): ComposeMachineState {
  const current = composeView(state, session)
  switch (event.type) {
    case 'fieldChanged':
      return {
        ...current,
        hasUserEdited: true,
        form: { ...current.form, [event.field]: event.value },
      }
    case 'attachmentsAdded': {
      const attachments = [...current.form.attachments, ...event.attachments]
      const error = validateAttachmentLimits(attachments)
      if (error) {
        // Over a limit: reject the batch whole, surface why, keep the form.
        return { ...current, errorMessage: error }
      }
      return {
        ...current,
        errorMessage: null,
        hasUserEdited: true,
        form: { ...current.form, attachments },
      }
    }
    case 'attachmentRemoved':
      return {
        ...current,
        hasUserEdited: true,
        form: {
          ...current.form,
          attachments: current.form.attachments.filter(
            (attachment) => attachment.id !== event.attachmentId,
          ),
        },
      }
    case 'identityDefaulted':
      return current.form.from.trim().length > 0
        ? current
        : { ...current, form: { ...current.form, from: event.from } }
    case 'forwardAttachmentsSeeded':
      if (current.seededForwardAttachments) {
        return current
      }
      return {
        ...current,
        seededForwardAttachments: true,
        form:
          current.form.attachments.length > 0
            ? current.form
            : { ...current.form, attachments: event.attachments },
      }
    case 'replyContextSeeded': {
      if (current.seededReplyContext) {
        return current
      }
      const form = current.form
      return {
        ...current,
        seededReplyContext: true,
        seededQuoteBlock: event.quoteBlock,
        form: {
          ...form,
          to: form.to.trim() ? form.to : event.to,
          cc: form.cc.trim() ? form.cc : event.cc,
          subject: form.subject.trim() ? form.subject : event.subject,
          body: event.quoteBlock
            ? `${form.body}\n\n${event.quoteBlock}`
            : form.body,
        },
      }
    }
    case 'signatureSeeded':
      if (current.seededSignature) {
        return current
      }
      return {
        ...current,
        seededSignature: true,
        form: {
          ...current.form,
          body:
            event.placement === 'append'
              ? appendSignature(current.form.body, event.signature)
              : insertSignatureAboveQuote(
                  current.form.body,
                  event.signature,
                  current.seededQuoteBlock,
                ),
        },
      }
    case 'errorReported':
      return { ...current, errorMessage: event.message }
  }
}

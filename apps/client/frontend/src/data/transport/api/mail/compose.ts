// Compose wire types. The recipient/attachment shapes the backend accepts are
// the generated twins in `@/gen`; this module re-exports them under their
// historical names so the whole tree shares one type identity. The remaining
// shapes are client-side compositions (reply context, the composer's partial
// send input before its stable draft identity is injected).

/** Recipient of a message (generated `Recipient`). */
export type { Recipient } from '@/gen'

/** One outgoing attachment (generated `SendMessageAttachment`). */
export type { SendMessageAttachment as SendMessageAttachmentInput } from '@/gen'

import type { Recipient, SendMessageAttachment } from '@/gen'

export interface Identity {
  id: string
  name: string
  email: string
}

export interface ReplyContext {
  to: Recipient[]
  cc: Recipient[]
  /** Original `To` recipients; lets a client build a reply-all set (From + To + Cc minus self) without a second fetch. */
  originalTo: Recipient[]
  replySubject: string
  forwardSubject: string
  quotedBody: string | null
  forwardedBody: string | null
  inReplyTo: string | null
  references: string | null
  /**
   * The original message's `From` recipients, verbatim. `to` holds the derived
   * reply recipient; the attribution line ("On <date> <sender> wrote:") is
   * built from this so it always names the actual sender.
   */
  originalFrom: Recipient[]
  /** The original message's date (RFC 3339), localized into the attribution line. */
  originalDate: string | null
}

/**
 * The composer's assembled content before its stable draft identity and any
 * hold options are attached. `toSendMessageRequest` pins the `draftId` and the
 * submission layer adds `sendAt`/`undoWindowSeconds` to produce the wire
 * `SendMessageRequest`.
 */
export interface SendMessageInput {
  from: Recipient | null
  to: Recipient[]
  cc: Recipient[]
  bcc: Recipient[]
  subject: string
  body: string
  inReplyTo: string | null
  references: string | null
  attachments: SendMessageAttachment[]
}

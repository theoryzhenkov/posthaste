/** @spec docs/L1-api#compose */
export interface Identity {
  id: string
  name: string
  email: string
}

/** @spec docs/L1-api#compose */
export interface Recipient {
  name: string | null
  email: string
}

/** @spec docs/L1-api#compose */
export interface ReplyContext {
  to: Recipient[]
  cc: Recipient[]
  replySubject: string
  forwardSubject: string
  quotedBody: string | null
  inReplyTo: string | null
  references: string | null
}

/** @spec docs/L1-api#compose */
export interface SendMessageAttachmentInput {
  filename: string
  mimeType: string
  contentBase64: string
}

/** @spec docs/L1-api#compose */
export interface SendMessageInput {
  from: Recipient | null
  to: Recipient[]
  cc: Recipient[]
  bcc: Recipient[]
  subject: string
  body: string
  inReplyTo: string | null
  references: string | null
  attachments: SendMessageAttachmentInput[]
}

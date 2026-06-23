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
  forwardedBody: string | null
  inReplyTo: string | null
  references: string | null
}

/** Compose-ready content parsed from an existing provider draft. @spec docs/L1-outbox#operation-model */
export interface DraftContent {
  from: Recipient | null
  to: Recipient[]
  cc: Recipient[]
  bcc: Recipient[]
  subject: string
  body: string
  /**
   * Stable `X-Posthaste-Draft-Id` for this draft, when present. Autosave keys by
   * this so a resumed edit updates the draft in place instead of duplicating it
   * as the provider id rotates.
   */
  draftId: string | null
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
  /**
   * Server-injected stable draft identity for a draft save; clients never set
   * it (a sent message is a fresh message). Present for wire conformance.
   */
  draftId?: string | null
}

/** @spec docs/L1-outbox#operation-model */
export type OperationKind =
  | 'setKeywords'
  | 'replaceMailboxes'
  | 'destroy'
  | 'draftCreate'
  | 'draftUpdate'
  | 'draftDelete'
  | 'send'

/** @spec docs/L1-outbox#state-machine */
export type OperationState = 'pending' | 'inflight' | 'applied' | 'failed'

/** @spec docs/L1-outbox#operation-model */
export type OperationEntityKind = 'message' | 'draft'

/** @spec docs/L1-outbox#operation-model */
export interface OperationEntity {
  kind: OperationEntityKind
  id: string
}

/** A local-first command in the outbox. @spec docs/L1-outbox#operation-model */
export interface Operation {
  id: string
  accountId: string
  entity: OperationEntity
  kind: OperationKind
  payload: unknown
  state: OperationState
  attempts: number
  lastError: string | null
  dependsOn: string | null
  createdAt: string
  updatedAt: string
}

/** Request body for saving a draft local-first. @spec docs/L1-outbox#operation-model */
export interface SaveDraftInput {
  draftId: string | null
  message: SendMessageInput
}

/** Settlement payload carried by the `operation.settled` event. @spec docs/L1-outbox#settlement */
export interface OperationSettlement {
  id: string
  outcome: 'applied' | 'failed'
  assignedEntityId: string | null
  error: string | null
}

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
export type OperationState =
  | 'pending'
  | 'inflight'
  | 'applied'
  | 'conflicted'
  | 'failed'

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
  baseCursor: string | null
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
  outcome: 'applied' | 'conflicted' | 'failed'
  assignedEntityId: string | null
  error: string | null
}

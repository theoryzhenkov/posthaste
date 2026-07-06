/**
 * This file was auto-generated from query-schema.json by
 * scripts/gen-query-schema.ts. Do not make direct changes to the file.
 *
 * Regenerate: `bun run query-schema:generate`. The committed copy is
 * drift-checked verbatim by `bun run query-schema:check`. The artifact itself is
 * emitted from the canonical Rust schema
 * (`posthaste_domain_model::query_schema_document`), so the field set, per-field
 * value type, and per-field operators here can never diverge from the store SQL
 * compiler.
 */
import type { SmartMailboxField, SmartMailboxOperator } from './types'

/** The coarse value-shape family of a query field (the Rust `QueryValueType`). */
export type QueryValueType = 'text' | 'bool' | 'date' | 'number'

/** A field's canonical spec: its value type and the operators it accepts. */
export interface QueryFieldSchema {
  valueType: QueryValueType
  operators: readonly SmartMailboxOperator[]
}

/**
 * The canonical field -> { valueType, operators } table, generated from the
 * Rust schema. Presentation (widget + label) lives in `fieldRegistry.ts`; this
 * is only the drift-prone DATA the store compiler shares.
 */
export const QUERY_FIELD_SCHEMA: Record<SmartMailboxField, QueryFieldSchema> = {
  sourceId: { valueType: 'text', operators: ['equals', 'in'] },
  sourceName: { valueType: 'text', operators: ['equals', 'contains', 'in', 'beginsWith', 'endsWith', 'regex'] },
  messageId: { valueType: 'text', operators: ['equals', 'in'] },
  threadId: { valueType: 'text', operators: ['equals', 'in'] },
  conversationId: { valueType: 'text', operators: ['equals', 'in'] },
  mailboxId: { valueType: 'text', operators: ['equals', 'in'] },
  mailboxName: { valueType: 'text', operators: ['equals', 'contains', 'in', 'beginsWith', 'endsWith', 'regex'] },
  mailboxRole: { valueType: 'text', operators: ['equals', 'in'] },
  isRead: { valueType: 'bool', operators: ['equals'] },
  isFlagged: { valueType: 'bool', operators: ['equals'] },
  hasAttachment: { valueType: 'bool', operators: ['equals'] },
  keyword: { valueType: 'text', operators: ['equals', 'in'] },
  fromName: { valueType: 'text', operators: ['equals', 'contains', 'in', 'beginsWith', 'endsWith', 'regex'] },
  fromEmail: { valueType: 'text', operators: ['equals', 'contains', 'in', 'beginsWith', 'endsWith', 'regex'] },
  to: { valueType: 'text', operators: ['equals', 'contains', 'in', 'beginsWith', 'endsWith', 'regex'] },
  subject: { valueType: 'text', operators: ['equals', 'contains', 'in', 'beginsWith', 'endsWith', 'regex'] },
  preview: { valueType: 'text', operators: ['equals', 'contains', 'in', 'beginsWith', 'endsWith', 'regex'] },
  receivedAt: { valueType: 'date', operators: ['lt', 'gt', 'le', 'ge'] },
  size: { valueType: 'number', operators: ['lt', 'gt', 'le', 'ge'] },
}

/** Every query field, in the schema's canonical declaration order. */
export const ALL_QUERY_FIELDS: readonly SmartMailboxField[] = [
  'sourceId',
  'sourceName',
  'messageId',
  'threadId',
  'conversationId',
  'mailboxId',
  'mailboxName',
  'mailboxRole',
  'isRead',
  'isFlagged',
  'hasAttachment',
  'keyword',
  'fromName',
  'fromEmail',
  'to',
  'subject',
  'preview',
  'receivedAt',
  'size',
]

/** The mail-query AST: one query system, several front-ends.
 *
 *  The front-end-agnostic query language shared by every surface that filters
 *  mail — smart mailboxes (saved queries) and automation-rule WHEN-clauses. The
 *  `SmartMailbox` container itself lives in `./smartMailboxes`.
 *  @spec docs/L1-search#smart-mailbox-data-model */

/** @spec docs/L1-search#smart-mailbox-data-model */
export type MailQueryGroupOperator = 'all' | 'any'

/** @spec docs/L1-search#smart-mailbox-data-model */
export type MailQueryField =
  | 'sourceId'
  | 'sourceName'
  | 'messageId'
  | 'threadId'
  | 'conversationId'
  | 'mailboxId'
  | 'mailboxName'
  | 'mailboxRole'
  | 'isRead'
  | 'isFlagged'
  | 'hasAttachment'
  | 'keyword'
  | 'fromName'
  | 'fromEmail'
  | 'to'
  | 'subject'
  | 'preview'
  | 'receivedAt'
  | 'size'

/** Neutral comparison operators (D6): the four ordered comparisons are
 *  `lt`/`gt`/`le`/`ge` (`< > <= >=`), labelled per field type in the editor
 *  ("before/after" for dates, "smaller/larger than" for size). The model no
 *  longer speaks "date". Stored rules using the old names still deserialize
 *  server-side via serde aliases.
 *  @spec docs/L1-search#smart-mailbox-data-model */
export type MailQueryOperator =
  | 'equals'
  | 'in'
  | 'contains'
  | 'beginsWith'
  | 'endsWith'
  | 'regex'
  | 'lt'
  | 'gt'
  | 'le'
  | 'ge'

/** Time unit for a relative date offset.
 *  @spec docs/L1-search#smart-mailbox-data-model */
export type DateUnit = 'minutes' | 'hours' | 'days' | 'weeks' | 'months'

/** A typed date condition value. `absolute` compares against a stored RFC3339
 *  instant; `relative` is a rolling "N units ago" offset resolved at query
 *  time (so it never freezes to a fixed date at edit time). Distinguished from
 *  the scalar `MailQueryValue` shapes by being an object with a `kind` tag.
 *  @spec docs/L1-search#smart-mailbox-data-model */
export type DateValue =
  | { kind: 'absolute'; value: string }
  | { kind: 'relative'; amount: number; unit: DateUnit }

/** @spec docs/L1-search#smart-mailbox-data-model */
export type MailQueryValue = string | string[] | boolean | DateValue

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface MailQueryGroup {
  operator: MailQueryGroupOperator
  negated: boolean
  nodes: MailQueryRuleNode[]
}

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface MailQueryCondition {
  type: 'condition'
  field: MailQueryField
  operator: MailQueryOperator
  negated: boolean
  value: MailQueryValue
}

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface MailQueryRuleGroup {
  type: 'group'
  operator: MailQueryGroupOperator
  negated: boolean
  nodes: MailQueryRuleNode[]
}

/** @spec docs/L1-search#smart-mailbox-data-model */
export type MailQueryRuleNode = MailQueryRuleGroup | MailQueryCondition

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface MailQueryRule {
  root: MailQueryGroup
}

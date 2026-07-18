/** The mail-query AST: one query system, several front-ends.
 *
 *  The front-end-agnostic query language shared by every surface that filters
 *  mail — smart mailboxes (saved queries) and automation-rule WHEN-clauses. The
 *  `SmartMailbox` container itself lives in `./smartMailboxes`. */

export type MailQueryGroupOperator = 'all' | 'any'

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
  | 'body'
  | 'receivedAt'
  | 'size'

/** The four ordered comparisons are `lt`/`gt`/`le`/`ge` (`< > <= >=`), labelled
 *  per field type in the editor ("before/after" for dates, "smaller/larger than"
 *  for size). Stored rules using old operator names still deserialize server-side
 *  via serde aliases. */
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

/** Time unit for a relative date offset. */
export type DateUnit = 'minutes' | 'hours' | 'days' | 'weeks' | 'months'

/** A typed date condition value. `absolute` compares against a stored RFC3339
 *  instant; `relative` is a rolling "N units ago" offset resolved at query
 *  time (so it never freezes to a fixed date at edit time). Distinguished from
 *  the scalar `MailQueryValue` shapes by being an object with a `kind` tag. */
export type DateValue =
  | { kind: 'absolute'; value: string }
  | { kind: 'relative'; amount: number; unit: DateUnit }

export type MailQueryValue = string | string[] | boolean | DateValue

export interface MailQueryGroup {
  operator: MailQueryGroupOperator
  negated: boolean
  nodes: MailQueryRuleNode[]
}

export interface MailQueryCondition {
  type: 'condition'
  field: MailQueryField
  operator: MailQueryOperator
  negated: boolean
  value: MailQueryValue
}

export interface MailQueryRuleGroup {
  type: 'group'
  operator: MailQueryGroupOperator
  negated: boolean
  nodes: MailQueryRuleNode[]
}

export type MailQueryRuleNode = MailQueryRuleGroup | MailQueryCondition

export interface MailQueryRule {
  root: MailQueryGroup
}

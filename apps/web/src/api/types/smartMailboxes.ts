export type SmartMailboxKind = 'default' | 'user'

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxGroupOperator = 'all' | 'any'

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxField =
  | 'sourceId'
  | 'sourceName'
  | 'messageId'
  | 'threadId'
  | 'mailboxId'
  | 'mailboxName'
  | 'mailboxRole'
  | 'isRead'
  | 'isFlagged'
  | 'hasAttachment'
  | 'keyword'
  | 'fromName'
  | 'fromEmail'
  | 'subject'
  | 'preview'
  | 'receivedAt'

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxOperator =
  | 'equals'
  | 'in'
  | 'contains'
  | 'before'
  | 'after'
  | 'onOrBefore'
  | 'onOrAfter'

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxValue = string | string[] | boolean

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface SmartMailboxGroup {
  operator: SmartMailboxGroupOperator
  negated: boolean
  nodes: SmartMailboxRuleNode[]
}

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface SmartMailboxCondition {
  type: 'condition'
  field: SmartMailboxField
  operator: SmartMailboxOperator
  negated: boolean
  value: SmartMailboxValue
}

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface SmartMailboxRuleGroup {
  type: 'group'
  operator: SmartMailboxGroupOperator
  negated: boolean
  nodes: SmartMailboxRuleNode[]
}

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxRuleNode = SmartMailboxRuleGroup | SmartMailboxCondition

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface SmartMailboxRule {
  root: SmartMailboxGroup
}

/** @spec docs/L1-api#smart-mailbox-crud */
export interface SmartMailbox {
  id: string
  name: string
  position: number
  kind: SmartMailboxKind
  defaultKey: string | null
  parentId: string | null
  rule: SmartMailboxRule
  createdAt: string
  updatedAt: string
}

/** @spec docs/L1-api#smart-mailbox-crud */
export interface SmartMailboxSummary {
  id: string
  name: string
  position: number
  kind: SmartMailboxKind
  defaultKey: string | null
  parentId: string | null
  unreadMessages: number
  totalMessages: number
  createdAt: string
  updatedAt: string
}

export interface CreateSmartMailboxInput {
  name: string
  position?: number
  rule: SmartMailboxRule
}

/** @spec docs/L1-api#smart-mailbox-crud */
export interface UpdateSmartMailboxInput {
  name?: string
  position?: number
  rule?: SmartMailboxRule
}

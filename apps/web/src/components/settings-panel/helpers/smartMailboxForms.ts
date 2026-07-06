import type {
  SmartMailbox,
  SmartMailboxCondition,
  SmartMailboxField,
  SmartMailboxGroupOperator,
  SmartMailboxOperator,
  SmartMailboxRule,
  SmartMailboxRuleNode,
  SmartMailboxSummary,
} from '../../../api/types'
import type { SmartMailboxFormState } from '../types'
import { operatorOptionsForField, valueTypeForField } from './fieldRegistry'

export { operatorOptionsForField, valueTypeForField } from './fieldRegistry'
export type { ConditionValueType, FieldDescriptor } from './fieldRegistry'

/** Default empty form state for creating a new smart mailbox. */
export const EMPTY_SMART_MAILBOX_FORM: SmartMailboxFormState = {
  name: '',
  role: null,
  rule: defaultEmptyRule(),
}

export function defaultEmptyRule(): SmartMailboxRule {
  return {
    root: {
      operator: 'all',
      negated: false,
      nodes: [],
    },
  }
}

/** Convert an existing smart mailbox into editable form state. */
export function formFromSmartMailbox(
  smartMailbox: SmartMailbox | SmartMailboxSummary,
): SmartMailboxFormState {
  return {
    name: smartMailbox.name,
    role: smartMailbox.role,
    rule:
      'rule' in smartMailbox
        ? smartMailbox.rule
        : EMPTY_SMART_MAILBOX_FORM.rule,
  }
}

/**
 * Available smart mailbox filter fields for the rule builder UI.
 * @spec docs/L1-search#smart-mailbox-data-model
 */
export const FIELD_OPTIONS: Array<{ value: SmartMailboxField; label: string }> =
  [
    { value: 'sourceId', label: 'Source ID' },
    { value: 'sourceName', label: 'Source Name' },
    { value: 'messageId', label: 'Message ID' },
    { value: 'threadId', label: 'Thread ID' },
    { value: 'mailboxId', label: 'Mailbox ID' },
    { value: 'mailboxName', label: 'Mailbox Name' },
    { value: 'mailboxRole', label: 'Mailbox Role' },
    { value: 'isRead', label: 'Read state' },
    { value: 'isFlagged', label: 'Flagged' },
    { value: 'hasAttachment', label: 'Has attachment' },
    { value: 'keyword', label: 'Keyword' },
    { value: 'fromName', label: 'From name' },
    { value: 'fromEmail', label: 'From email' },
    { value: 'subject', label: 'Subject' },
    { value: 'preview', label: 'Preview' },
    { value: 'receivedAt', label: 'Received at' },
  ]

/** @spec docs/L1-search#smart-mailbox-data-model */
export const GROUP_OPERATOR_OPTIONS: Array<{
  value: SmartMailboxGroupOperator
  label: string
}> = [
  { value: 'all', label: 'All' },
  { value: 'any', label: 'Any' },
]

export function parseGroupOperator(
  value: string,
  fallback: SmartMailboxGroupOperator,
): SmartMailboxGroupOperator {
  return (
    GROUP_OPERATOR_OPTIONS.find((option) => option.value === value)?.value ??
    fallback
  )
}

export function parseField(
  value: string,
  fallback: SmartMailboxField,
): SmartMailboxField {
  return (
    FIELD_OPTIONS.find((option) => option.value === value)?.value ?? fallback
  )
}

export function parseOperator(
  value: string,
  field: SmartMailboxField,
  fallback: SmartMailboxOperator,
): SmartMailboxOperator {
  return (
    operatorOptionsForField(field).find((operator) => operator === value) ??
    fallback
  )
}

export function defaultCondition(
  field: SmartMailboxField = 'mailboxRole',
): SmartMailboxCondition {
  const operator = operatorOptionsForField(field)[0]
  const isBooleanField = valueTypeForField(field) === 'boolean'
  return {
    type: 'condition',
    field,
    operator,
    negated: false,
    value: isBooleanField ? false : '',
  }
}

/** Create an empty rule group node. */
export function defaultGroup(): SmartMailboxRuleNode {
  return {
    type: 'group',
    operator: 'all',
    negated: false,
    nodes: [],
  }
}

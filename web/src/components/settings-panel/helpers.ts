/**
 * Pure helper functions and constants for the settings panel editors.
 *
 * Handles form state conversion, smart mailbox rule builder helpers,
 * and visual status mapping.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import type {
  AccountDriver,
  AccountOverview,
  CreateAccountInput,
  SmartMailbox,
  SmartMailboxCondition,
  SmartMailboxField,
  SmartMailboxGroupOperator,
  SmartMailboxOperator,
  SmartMailboxRuleNode,
  SmartMailboxSummary,
  UpdateAccountInput,
} from '../../api/types'
import type { AccountFormState, SmartMailboxFormState } from './types'

/** @spec docs/L1-api#account-crud-lifecycle */
export const ACCOUNT_DRIVER_VALUES = ['jmap', 'mock'] as const

/** Default empty form state for creating a new account. */
export const EMPTY_FORM: AccountFormState = {
  id: '',
  name: '',
  driver: 'jmap',
  enabled: true,
  baseUrl: '',
  username: '',
  password: '',
  secretMode: 'replace',
}

/** Default empty form state for creating a new smart mailbox. */
export const EMPTY_SMART_MAILBOX_FORM: SmartMailboxFormState = {
  name: '',
  position: 0,
  rule: {
    root: {
      operator: 'all',
      negated: false,
      nodes: [],
    },
  },
}

/** Convert an existing account overview into editable form state. */
export function formFromAccount(account: AccountOverview): AccountFormState {
  return {
    id: account.id,
    name: account.name,
    driver: account.driver,
    enabled: account.enabled,
    baseUrl: account.transport.baseUrl ?? '',
    username: account.transport.username ?? '',
    password: '',
    secretMode: account.transport.secret.configured ? 'keep' : 'replace',
  }
}

/** Convert an existing smart mailbox into editable form state. */
export function formFromSmartMailbox(
  smartMailbox: SmartMailbox | SmartMailboxSummary,
): SmartMailboxFormState {
  return {
    name: smartMailbox.name,
    position: smartMailbox.position,
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
    { value: 'mailboxId', label: 'Mailbox ID' },
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

/** Parse a string into a valid account driver, returning the fallback on mismatch. */
export function parseAccountDriver(
  value: string,
  fallback: AccountDriver,
): AccountDriver {
  return (
    ACCOUNT_DRIVER_VALUES.find((candidate) => candidate === value) ?? fallback
  )
}

/** Parse a string into a valid group operator, returning the fallback on mismatch. */
export function parseGroupOperator(
  value: string,
  fallback: SmartMailboxGroupOperator,
): SmartMailboxGroupOperator {
  return (
    GROUP_OPERATOR_OPTIONS.find((option) => option.value === value)?.value ??
    fallback
  )
}

/** Parse a string into a valid smart mailbox field, returning the fallback on mismatch. */
export function parseField(
  value: string,
  fallback: SmartMailboxField,
): SmartMailboxField {
  return (
    FIELD_OPTIONS.find((option) => option.value === value)?.value ?? fallback
  )
}

/** Parse a string into a valid operator for the given field. */
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

/**
 * Return the valid operators for a given smart mailbox field.
 * @spec docs/L1-search#smart-mailbox-data-model
 */
export function operatorOptionsForField(
  field: SmartMailboxField,
): SmartMailboxOperator[] {
  switch (field) {
    case 'sourceId':
    case 'sourceName':
    case 'mailboxId':
    case 'mailboxRole':
    case 'keyword':
      return ['equals', 'in']
    case 'isRead':
    case 'isFlagged':
    case 'hasAttachment':
      return ['equals']
    case 'fromName':
    case 'fromEmail':
    case 'subject':
    case 'preview':
      return ['equals', 'contains', 'in']
    case 'receivedAt':
      return ['before', 'after', 'onOrBefore', 'onOrAfter']
  }
}

/** Create a default condition node for the given field. */
export function defaultCondition(
  field: SmartMailboxField = 'mailboxRole',
): SmartMailboxCondition {
  const operator = operatorOptionsForField(field)[0]
  const isBooleanField =
    field === 'isRead' || field === 'isFlagged' || field === 'hasAttachment'
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

/**
 * Build a secret instruction payload from the current form state.
 * @spec docs/L1-api#secret-management
 */
export function buildSecretInput(form: AccountFormState) {
  if (form.secretMode === 'replace') {
    return { mode: 'replace' as const, password: form.password }
  }
  return { mode: form.secretMode as 'keep' | 'clear' }
}

/** Build a create-account API payload from form state. */
export function buildCreateAccountPayload(
  form: AccountFormState,
): CreateAccountInput {
  return {
    id: form.id.trim(),
    name: form.name.trim(),
    driver: form.driver,
    enabled: form.enabled,
    transport: {
      baseUrl: form.baseUrl,
      username: form.username,
    },
    secret: buildSecretInput(form),
  }
}

/**
 * Build an update-account API payload from form state.
 * @spec docs/L1-api#account-crud-lifecycle
 */
export function buildUpdateAccountPayload(
  form: AccountFormState,
): UpdateAccountInput {
  return {
    name: form.name.trim(),
    driver: form.driver,
    enabled: form.enabled,
    transport: {
      baseUrl: form.baseUrl,
      username: form.username,
    },
    secret: buildSecretInput(form),
  }
}

/** Map account status to Tailwind color classes for the status badge. */
export function statusTone(status: AccountOverview['status']): string {
  switch (status) {
    case 'ready':
      return 'text-emerald-700 border-emerald-500/30 bg-emerald-500/10'
    case 'syncing':
      return 'text-blue-700 border-blue-500/30 bg-blue-500/10'
    case 'degraded':
      return 'text-amber-700 border-amber-500/30 bg-amber-500/10'
    case 'authError':
      return 'text-rose-700 border-rose-500/30 bg-rose-500/10'
    case 'offline':
      return 'text-orange-700 border-orange-500/30 bg-orange-500/10'
    case 'disabled':
      return 'text-zinc-600 border-zinc-500/30 bg-zinc-500/10'
  }
}

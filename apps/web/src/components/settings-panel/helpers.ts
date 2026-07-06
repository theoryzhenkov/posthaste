/**
 * Pure helper functions and constants for the settings panel editors.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-search#smart-mailbox-data-model
 */
export {
  EMPTY_FORM,
  applyImapDefaults,
  buildAccountAppearanceInput,
  buildCreateAccountPayload,
  buildSecretInput,
  buildTransportInput,
  buildUpdateAccountPayload,
  emptyAccountForm,
  formFromAccount,
  imapDefaultsForEmail,
  normalizeAccountInitials,
  parseEmailPatterns,
} from './helpers/accountForms'
export type { ImapProviderDefaults } from './helpers/accountForms'
export {
  statusTone,
  syncProgressLabel,
  syncProgressPercent,
} from './helpers/accountStatus'
export {
  EMPTY_SMART_MAILBOX_FORM,
  FIELD_OPTIONS,
  GROUP_OPERATOR_OPTIONS,
  defaultCondition,
  defaultEmptyRule,
  defaultGroup,
  formFromSmartMailbox,
  operatorOptionsForField,
  parseField,
  parseGroupOperator,
  parseOperator,
  valueTypeForField,
} from './helpers/smartMailboxForms'
export type {
  ConditionValueType,
  FieldDescriptor,
} from './helpers/smartMailboxForms'

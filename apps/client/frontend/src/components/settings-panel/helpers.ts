/**
 * Pure helper functions and constants for the settings panel editors.
 *
 */
export {
  EMPTY_FORM,
  applyImapDefaults,
  buildAccountAppearanceInput,
  buildCreateAccountIntent,
  buildIdentityPatch,
  buildSecretChange,
  buildTransportIntent,
  emptyAccountForm,
  formFromAccount,
  imapDefaultsForEmail,
  normalizeAccountInitials,
  parseEmailPatterns,
  shouldWriteTransport,
} from './helpers/accountForms'
export type {
  EndpointSettings,
  ImapProviderDefaults,
} from './helpers/accountForms'
export { statusTone } from './helpers/accountStatus'
export {
  EMPTY_SMART_MAILBOX_FORM,
  FIELD_OPTIONS,
  GROUP_OPERATOR_OPTIONS,
  defaultCondition,
  defaultEmptyRule,
  defaultGroup,
  formFromSmartMailbox,
  operatorLabel,
  operatorLabelForField,
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

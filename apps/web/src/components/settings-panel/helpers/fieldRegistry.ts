import type {
  SmartMailboxField,
  SmartMailboxOperator,
} from '../../../api/types'

/**
 * The value-entry "type" for a condition field. This is the web-side mirror of
 * the Rust compiler's per-field operator/value matrix (`field_compilers.rs`):
 * it drives which VALUE widget the condition editor renders, while the emitted
 * `SmartMailboxValue` wire shape (string | string[] | boolean) is unchanged.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
export type ConditionValueType =
  | 'text'
  | 'boolean'
  | 'date'
  | 'mailboxRef'
  | 'accountRef'
  | 'roleEnum'
  | 'keyword'
  | 'address'
  | 'size'

/** Descriptor for a single condition field: its value type + allowed operators. */
export interface FieldDescriptor {
  /** Drives the type-directed value widget in the condition editor. */
  valueType: ConditionValueType
  /**
   * Operators offered for this field, mirroring the Rust compiler's type-gated
   * matrix (`field_compilers.rs`). Kept here so the operator subset and the
   * value widget stay in lock-step and cannot drift apart.
   */
  operators: SmartMailboxOperator[]
}

const ID_OPERATORS: SmartMailboxOperator[] = ['equals', 'in']
const TEXT_OPERATORS: SmartMailboxOperator[] = ['equals', 'contains', 'in']
const BOOL_OPERATORS: SmartMailboxOperator[] = ['equals']
const DATE_OPERATORS: SmartMailboxOperator[] = [
  'before',
  'after',
  'onOrBefore',
  'onOrAfter',
]
// Numeric size comparison reuses the four inequality operators as `< > <= >=`
// (the Rust `compile_numeric_field` matrix), so the wire enum stays unchanged.
const SIZE_OPERATORS: SmartMailboxOperator[] = [
  'before',
  'after',
  'onOrBefore',
  'onOrAfter',
]

/**
 * The single field → { valueType, operators } table that drives the whole
 * condition row. Adding type-directed value widgets is a matter of setting the
 * right `valueType` here; the emitted value never changes shape.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
export const FIELD_REGISTRY: Record<SmartMailboxField, FieldDescriptor> = {
  sourceId: { valueType: 'accountRef', operators: ID_OPERATORS },
  sourceName: { valueType: 'text', operators: TEXT_OPERATORS },
  messageId: { valueType: 'text', operators: ID_OPERATORS },
  threadId: { valueType: 'text', operators: ID_OPERATORS },
  mailboxId: { valueType: 'mailboxRef', operators: ID_OPERATORS },
  mailboxName: { valueType: 'text', operators: TEXT_OPERATORS },
  mailboxRole: { valueType: 'roleEnum', operators: ID_OPERATORS },
  isRead: { valueType: 'boolean', operators: BOOL_OPERATORS },
  isFlagged: { valueType: 'boolean', operators: BOOL_OPERATORS },
  hasAttachment: { valueType: 'boolean', operators: BOOL_OPERATORS },
  // Tag/keyword autocomplete is a follow-on (no known-tag source wired yet);
  // interim value type is `keyword`, rendered as the generic text box.
  keyword: { valueType: 'keyword', operators: ID_OPERATORS },
  // Address autocomplete is a follow-on (no shared address picker to reuse in
  // the condition editor yet); interim value type is `address` → text box.
  fromName: { valueType: 'address', operators: TEXT_OPERATORS },
  fromEmail: { valueType: 'address', operators: TEXT_OPERATORS },
  // Recipient (To) address field — matched against `to_json` (Cc/Bcc are not
  // stored as separate columns, so only To is queryable). Same operators and
  // wire shape as fromEmail; interim widget is the address text box.
  to: { valueType: 'address', operators: TEXT_OPERATORS },
  subject: { valueType: 'text', operators: TEXT_OPERATORS },
  preview: { valueType: 'text', operators: TEXT_OPERATORS },
  receivedAt: { valueType: 'date', operators: DATE_OPERATORS },
  // Byte size + unit widget; emits a byte-count string the numeric compiler
  // parses (`compile_numeric_field` on `message.size`).
  size: { valueType: 'size', operators: SIZE_OPERATORS },
}

/** The value type that drives the type-directed widget for a field. */
export function valueTypeForField(
  field: SmartMailboxField,
): ConditionValueType {
  return FIELD_REGISTRY[field].valueType
}

/**
 * Operator subset for a field — the single source of truth, consumed by both
 * the operator dropdown and `defaultCondition`.
 */
export function operatorOptionsForField(
  field: SmartMailboxField,
): SmartMailboxOperator[] {
  return FIELD_REGISTRY[field].operators
}

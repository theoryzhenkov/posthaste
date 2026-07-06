import type {
  SmartMailboxField,
  SmartMailboxOperator,
} from '../../../api/types'
import {
  QUERY_FIELD_SCHEMA,
  type QueryValueType,
} from '../../../api/querySchema.gen'

/**
 * The value-entry "type" for a condition field: which VALUE widget the condition
 * editor renders. This is PRESENTATION and is *finer* than the compiler's value
 * type — e.g. an id, a mailbox ref, a role, and an address all share the coarse
 * `text` value type in the Rust schema but want different pickers here.
 *
 * The field -> valueType/operators DATA is generated from the Rust schema
 * (`querySchema.gen.ts`); this module only layers the widget choice on top.
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

/** Descriptor for a single condition field: its widget value type + operators. */
export interface FieldDescriptor {
  /** Drives the type-directed value widget in the condition editor. */
  valueType: ConditionValueType
  /**
   * Operators offered for this field. Sourced from the generated Rust schema, so
   * the operator subset offered by the editor can never drift from the store SQL
   * compiler's accepted set.
   */
  operators: readonly SmartMailboxOperator[]
}

/**
 * The default widget for each coarse (Rust) value type. A field with no explicit
 * override in {@link WIDGET_OVERRIDE} renders with this widget.
 */
const DEFAULT_WIDGET: Record<QueryValueType, ConditionValueType> = {
  text: 'text',
  bool: 'boolean',
  date: 'date',
  number: 'size',
}

/**
 * Presentation-only refinement: fields whose coarse value type is `text` (or
 * `number`) but that deserve a dedicated picker instead of the generic text box.
 * Purely which widget to show — it never changes the emitted wire shape or the
 * allowed operators (those come from the generated schema).
 *
 * Tag/keyword and address autocomplete are follow-ons (no shared picker wired
 * yet), so `keyword`/`address` render as text boxes today; the distinct type is
 * kept so wiring the picker later is a one-line change here.
 */
const WIDGET_OVERRIDE: Partial<Record<SmartMailboxField, ConditionValueType>> =
  {
    sourceId: 'accountRef',
    mailboxId: 'mailboxRef',
    mailboxRole: 'roleEnum',
    keyword: 'keyword',
    fromName: 'address',
    fromEmail: 'address',
    // Recipient (To) address field — matched against `to_json`. Cc/Bcc are not
    // stored as separate columns, so only To is queryable.
    to: 'address',
    // Byte size + unit widget over the numeric compiler (`compile_numeric_field`).
    size: 'size',
  }

/**
 * The field -> { valueType (widget), operators } table. The DATA (field set +
 * operators + coarse value type) is generated from the Rust schema; only the
 * widget refinement above is hand-maintained here.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
export const FIELD_REGISTRY: Record<SmartMailboxField, FieldDescriptor> =
  Object.fromEntries(
    (Object.keys(QUERY_FIELD_SCHEMA) as SmartMailboxField[]).map((field) => {
      const spec = QUERY_FIELD_SCHEMA[field]
      return [
        field,
        {
          valueType: WIDGET_OVERRIDE[field] ?? DEFAULT_WIDGET[spec.valueType],
          operators: spec.operators,
        },
      ]
    }),
  ) as Record<SmartMailboxField, FieldDescriptor>

/** The value type that drives the type-directed widget for a field. */
export function valueTypeForField(
  field: SmartMailboxField,
): ConditionValueType {
  return FIELD_REGISTRY[field].valueType
}

/**
 * D6 — the human LABEL for a neutral operator, keyed off the field's value type.
 * The MODEL operators are neutral (`lt`/`gt`/`le`/`ge` = `< > <= >=`); the editor
 * labels them per type: a numeric/size field reads "smaller than / larger than /
 * at most / at least", while a date field reads "before / after / on or before /
 * on or after". `equals`/`contains`/`in` are type-agnostic.
 *
 * @spec docs/eph/RFC-L2-query-schema.md#d6--neutral-operator-names
 */
export function operatorLabel(
  operator: SmartMailboxOperator,
  valueType: ConditionValueType,
): string {
  const isSize = valueType === 'size'
  switch (operator) {
    case 'equals':
      return 'equals'
    case 'contains':
      return 'contains'
    case 'in':
      return 'is one of'
    case 'lt':
      return isSize ? 'smaller than' : 'before'
    case 'gt':
      return isSize ? 'larger than' : 'after'
    case 'le':
      return isSize ? 'at most' : 'on or before'
    case 'ge':
      return isSize ? 'at least' : 'on or after'
    default:
      return operator
  }
}

/** The operator label for a field, resolving its value type from the registry. */
export function operatorLabelForField(
  field: SmartMailboxField,
  operator: SmartMailboxOperator,
): string {
  return operatorLabel(operator, valueTypeForField(field))
}

/**
 * Operator subset for a field — generated from the Rust schema, consumed by both
 * the operator dropdown and `defaultCondition`.
 */
export function operatorOptionsForField(
  field: SmartMailboxField,
): readonly SmartMailboxOperator[] {
  return FIELD_REGISTRY[field].operators
}

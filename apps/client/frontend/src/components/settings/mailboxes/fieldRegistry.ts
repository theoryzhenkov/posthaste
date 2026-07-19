import type { MailQueryField, MailQueryOperator } from '../../../data/transport/api/index'

/** The coarse value-shape family of a query field. */
type QueryValueType = 'text' | 'bool' | 'date' | 'number'

/** A field's canonical spec: its value type and the operators it accepts. */
interface QueryFieldSchema {
  valueType: QueryValueType
  operators: readonly MailQueryOperator[]
}

const TEXT_OPERATORS: readonly MailQueryOperator[] = [
  'equals',
  'contains',
  'in',
  'beginsWith',
  'endsWith',
  'regex',
]
const ID_OPERATORS: readonly MailQueryOperator[] = ['equals', 'in']
const ORDERED_OPERATORS: readonly MailQueryOperator[] = ['lt', 'gt', 'le', 'ge']

/**
 * The field -> { valueType, operators } table, mirroring the backend query
 * compiler's accepted set (`MailQueryField`/`MailQueryOperator` are the
 * generated wire enums; this table is the per-field pairing).
 */
const QUERY_FIELD_SCHEMA: Record<MailQueryField, QueryFieldSchema> = {
  sourceId: { valueType: 'text', operators: ID_OPERATORS },
  sourceName: { valueType: 'text', operators: TEXT_OPERATORS },
  messageId: { valueType: 'text', operators: ID_OPERATORS },
  threadId: { valueType: 'text', operators: ID_OPERATORS },
  conversationId: { valueType: 'text', operators: ID_OPERATORS },
  mailboxId: { valueType: 'text', operators: ID_OPERATORS },
  mailboxName: { valueType: 'text', operators: TEXT_OPERATORS },
  mailboxRole: { valueType: 'text', operators: ID_OPERATORS },
  isRead: { valueType: 'bool', operators: ['equals'] },
  isFlagged: { valueType: 'bool', operators: ['equals'] },
  hasAttachment: { valueType: 'bool', operators: ['equals'] },
  keyword: { valueType: 'text', operators: ID_OPERATORS },
  fromName: { valueType: 'text', operators: TEXT_OPERATORS },
  fromEmail: { valueType: 'text', operators: TEXT_OPERATORS },
  to: { valueType: 'text', operators: TEXT_OPERATORS },
  subject: { valueType: 'text', operators: TEXT_OPERATORS },
  preview: { valueType: 'text', operators: TEXT_OPERATORS },
  body: { valueType: 'text', operators: ['contains'] },
  receivedAt: { valueType: 'date', operators: ORDERED_OPERATORS },
  size: { valueType: 'number', operators: ORDERED_OPERATORS },
}

/**
 * The value-entry "type" for a condition field: which VALUE widget the condition
 * editor renders. This is PRESENTATION and is *finer* than the coarse value
 * type — e.g. an id, a mailbox ref, a role, and an address all share the coarse
 * `text` value type but want different pickers here.
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
interface FieldDescriptor {
  /** Drives the type-directed value widget in the condition editor. */
  valueType: ConditionValueType
  /**
   * Operators offered for this field. Sourced from the generated Rust schema, so
   * the operator subset offered by the editor can never drift from the store SQL
   * compiler's accepted set.
   */
  operators: readonly MailQueryOperator[]
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
 * Each widget value type maps to a registry row in `conditionValueWidgets.tsx`
 * carrying BOTH arities (scalar + `in`-list entry) and any suggestion source
 * (`address` → the compose address book, `keyword` → the live tag list), so a
 * capability declared here composes with every operator the schema admits.
 */
const WIDGET_OVERRIDE: Partial<Record<MailQueryField, ConditionValueType>> = {
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
 */
const FIELD_REGISTRY: Record<MailQueryField, FieldDescriptor> =
  Object.fromEntries(
    (Object.keys(QUERY_FIELD_SCHEMA) as MailQueryField[]).map((field) => {
      const spec = QUERY_FIELD_SCHEMA[field]
      return [
        field,
        {
          valueType: WIDGET_OVERRIDE[field] ?? DEFAULT_WIDGET[spec.valueType],
          operators: spec.operators,
        },
      ]
    }),
  ) as Record<MailQueryField, FieldDescriptor>

/** The value type that drives the type-directed widget for a field. */
export function valueTypeForField(field: MailQueryField): ConditionValueType {
  return FIELD_REGISTRY[field].valueType
}

/**
 * The human LABEL for a neutral operator, keyed off the field's value type.
 * The MODEL operators are neutral (`lt`/`gt`/`le`/`ge` = `< > <= >=`); the editor
 * labels them per type: a numeric/size field reads "smaller than / larger than /
 * at most / at least", while a date field reads "before / after / on or before /
 * on or after". `equals`/`contains`/`in` are type-agnostic.
 */
function operatorLabel(
  operator: MailQueryOperator,
  valueType: ConditionValueType,
): string {
  const isSize = valueType === 'size'
  switch (operator) {
    case 'equals':
      return 'equals'
    case 'contains':
      return 'contains'
    case 'beginsWith':
      return 'begins with'
    case 'endsWith':
      return 'ends with'
    case 'regex':
      return 'matches regex'
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
  field: MailQueryField,
  operator: MailQueryOperator,
): string {
  return operatorLabel(operator, valueTypeForField(field))
}

/**
 * Operator subset for a field — generated from the Rust schema, consumed by both
 * the operator dropdown and `defaultCondition`.
 */
export function operatorOptionsForField(
  field: MailQueryField,
): readonly MailQueryOperator[] {
  return FIELD_REGISTRY[field].operators
}

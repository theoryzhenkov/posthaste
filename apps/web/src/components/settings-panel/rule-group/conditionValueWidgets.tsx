/**
 * Type-directed VALUE widgets for a single condition row. Which widget renders
 * is inferred from the field's `valueType` (see `fieldRegistry.ts`) plus the
 * chosen operator — the direct answer to "auto-fill the correct value format".
 *
 * WIRE-SHAPE PARITY (load-bearing): every widget emits the exact same
 * `SmartMailboxValue` the old text box did — a `string` for single-value ops, a
 * `string[]` for the `in` operator, a `boolean` for boolean fields. The pickers
 * only change how the user enters that value, never its serialized shape, so the
 * compiler/evaluator and stored JSON are unchanged.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import type {
  SmartMailboxCondition,
  SmartMailboxOperator,
  SmartMailboxValue,
} from '../../../api/types'
import { buildAddressBookSuggestionOptions } from '@/composeAddressSuggestions'
import { RecipientSuggestionInput } from '@/components/compose-overlay/RecipientSuggestionInput'
import { queryKeys } from '@/queryKeys'
import { runtimeViews } from '@/runtime/views'
import { ASSIGNABLE_MAILBOX_ROLES } from '../../../domainVocabulary'
import { Input } from '../../ui/input'
import { SelectItem } from '../../ui/select'
import { LabeledSelect, MailboxSelect } from '../MailboxSelect'
import { valueTypeForField } from '../helpers'
import { useConditionEditorData } from './conditionEditorContext'
import {
  absoluteDateValue,
  bytesFromSize,
  dateInputValue,
  dateValueMode,
  pickedRefValue,
  relativeDateValue,
  relativeParts,
  RELATIVE_UNIT_OPTIONS,
  SIZE_UNIT_OPTIONS,
  sizeInputParts,
  splitListValue,
  UNSET_REF,
  type RelativeUnit,
  type SizeUnit,
} from './conditionValueFormat'

// ---------------------------------------------------------------------------
// Widget dispatch
// ---------------------------------------------------------------------------

const INPUT_CLASS =
  'h-8 rounded-md border-border bg-background text-[13px] shadow-none'

function emitValue(
  condition: SmartMailboxCondition,
  onChange: (condition: SmartMailboxCondition) => void,
  value: SmartMailboxValue,
) {
  onChange({ ...condition, value })
}

/**
 * The type-directed value editor. Booleans and the multi-value `in` operator
 * are handled first (they cut across value types); otherwise the field's
 * `valueType` selects the widget.
 */
export function ConditionValueEditor({
  condition,
  onChange,
}: {
  condition: SmartMailboxCondition
  onChange: (condition: SmartMailboxCondition) => void
}) {
  const valueType = valueTypeForField(condition.field)

  if (valueType === 'boolean') {
    return <BooleanValueWidget condition={condition} onChange={onChange} />
  }
  if (condition.operator === 'in') {
    return <ListValueWidget condition={condition} onChange={onChange} />
  }
  switch (valueType) {
    case 'date':
      return <DateValueWidget condition={condition} onChange={onChange} />
    case 'mailboxRef':
      return <MailboxValueWidget condition={condition} onChange={onChange} />
    case 'accountRef':
      return <AccountValueWidget condition={condition} onChange={onChange} />
    case 'roleEnum':
      return <RoleValueWidget condition={condition} onChange={onChange} />
    case 'size':
      return <SizeValueWidget condition={condition} onChange={onChange} />
    case 'address':
      return <AddressValueWidget condition={condition} onChange={onChange} />
    default:
      // text | keyword — the generic text box (honest fallback; keyword
      // autocomplete is a follow-on slice).
      return <TextValueWidget condition={condition} onChange={onChange} />
  }
}

type WidgetProps = {
  condition: SmartMailboxCondition
  onChange: (condition: SmartMailboxCondition) => void
}

function BooleanValueWidget({ condition, onChange }: WidgetProps) {
  return (
    <div data-testid="value-widget-boolean" className="grid gap-1 text-[13px]">
      <LabeledSelect
        ariaLabel="Value"
        value={String(Boolean(condition.value))}
        onValueChange={(value) =>
          emitValue(condition, onChange, value === 'true')
        }
      >
        <SelectItem value="true">true</SelectItem>
        <SelectItem value="false">false</SelectItem>
      </LabeledSelect>
    </div>
  )
}

function ListValueWidget({ condition, onChange }: WidgetProps) {
  return (
    <div data-testid="value-widget-list" className="grid gap-1 text-[13px]">
      <Input
        type="text"
        aria-label="Value"
        className={INPUT_CLASS}
        value={
          Array.isArray(condition.value)
            ? condition.value.join(', ')
            : String(condition.value)
        }
        placeholder="comma, separated, values"
        onChange={(event) =>
          emitValue(condition, onChange, splitListValue(event.target.value))
        }
      />
    </div>
  )
}

/**
 * Address fields (`fromEmail`, `fromName`, `to`) share the SAME autocomplete
 * the compose recipient inputs use: the persistent server-side address book
 * (`senderAddresses`) fed through the reused `RecipientSuggestionInput`. A
 * condition holds a single address, so the input runs in `replace` mode (a pick
 * sets the bare email) and still emits the identical `string` wire shape the
 * old text box did.
 */
function AddressValueWidget({ condition, onChange }: WidgetProps) {
  const addressBook = useQuery({
    queryKey: queryKeys.senderAddresses,
    queryFn: runtimeViews.compose.senderAddresses,
  })
  const suggestions = useMemo(
    () => buildAddressBookSuggestionOptions(addressBook.data ?? []),
    [addressBook.data],
  )
  const value = Array.isArray(condition.value) ? '' : String(condition.value)
  return (
    <div data-testid="value-widget-address" className="grid gap-1 text-[13px]">
      <RecipientSuggestionInput
        ariaLabel="Value"
        selectionMode="replace"
        placeholder="name or email"
        suggestions={suggestions}
        value={value}
        onChange={(next) => emitValue(condition, onChange, next)}
      />
    </div>
  )
}

function TextValueWidget({ condition, onChange }: WidgetProps) {
  return (
    <div data-testid="value-widget-text" className="grid gap-1 text-[13px]">
      <Input
        type="text"
        aria-label="Value"
        className={INPUT_CLASS}
        value={Array.isArray(condition.value) ? '' : String(condition.value)}
        placeholder="value"
        onChange={(event) => emitValue(condition, onChange, event.target.value)}
      />
    </div>
  )
}

/**
 * A natural-language "reading" for a date condition. Each reading bundles the
 * sub-editor (absolute date vs. rolling relative) with the operator it implies,
 * so the user picks e.g. "in the last" instead of the nonsensical
 * "before" + "within". The reading OWNS the operator for date fields — the
 * ConditionEditor hides the generic operator dropdown for dates (see
 * `ConditionEditor.tsx`).
 */
type DateReading = {
  key: string
  label: string
  /** Optional trailing word after the amount/unit, e.g. "ago". */
  suffix?: string
  mode: 'absolute' | 'relative'
  operator: SmartMailboxOperator
}

// The MODEL operators are neutral (`lt`/`gt`/`le`/`ge`); for date fields the
// reading LABELS them "before/after/on or before/on or after" (D6's per-type
// labelling — dates read as dates even though the operator is neutral).
const DATE_READINGS: DateReading[] = [
  {
    key: 'onOrAfter',
    label: 'on or after',
    mode: 'absolute',
    operator: 'ge',
  },
  { key: 'after', label: 'after', mode: 'absolute', operator: 'gt' },
  { key: 'before', label: 'before', mode: 'absolute', operator: 'lt' },
  {
    key: 'onOrBefore',
    label: 'on or before',
    mode: 'absolute',
    operator: 'le',
  },
  // received_at > now-N: the message arrived inside the rolling window.
  {
    key: 'inTheLast',
    label: 'in the last',
    mode: 'relative',
    operator: 'gt',
  },
  // received_at < now-N: the message is older than the rolling window.
  {
    key: 'moreThanAgo',
    label: 'more than',
    suffix: 'ago',
    mode: 'relative',
    operator: 'lt',
  },
]

/** Derive the active reading from the stored condition (operator + value). */
function readingForCondition(condition: SmartMailboxCondition): DateReading {
  if (dateValueMode(condition.value) === 'relative') {
    // A relative value is either "in the last" (gt) or "more than … ago"
    // (lt), keyed off the neutral operator.
    return condition.operator === 'lt'
      ? DATE_READINGS.find((r) => r.key === 'moreThanAgo')!
      : DATE_READINGS.find((r) => r.key === 'inTheLast')!
  }
  return (
    DATE_READINGS.find(
      (r) => r.mode === 'absolute' && r.operator === condition.operator,
    ) ?? DATE_READINGS[0]
  )
}

function DateValueWidget({ condition, onChange }: WidgetProps) {
  const reading = readingForCondition(condition)
  const parts = relativeParts(condition.value)
  const [amount, setAmount] = useState(parts.amount)
  const [unit, setUnit] = useState<RelativeUnit>(parts.unit)

  const applyReading = (next: DateReading) => {
    onChange({
      ...condition,
      operator: next.operator,
      value:
        next.mode === 'relative'
          ? relativeDateValue(Number(amount), unit)
          : absoluteDateValue(dateInputValue(condition.value)),
    })
  }

  return (
    <div data-testid="value-widget-date" className="grid gap-1 text-[13px]">
      <div className="flex items-center gap-1.5">
        <LabeledSelect
          ariaLabel="Date mode"
          value={reading.key}
          onValueChange={(value) => {
            const next =
              DATE_READINGS.find((r) => r.key === value) ?? DATE_READINGS[0]
            applyReading(next)
          }}
        >
          {DATE_READINGS.map((option) => (
            <SelectItem key={option.key} value={option.key}>
              {option.label}
            </SelectItem>
          ))}
        </LabeledSelect>
        {reading.mode === 'absolute' ? (
          <Input
            type="date"
            aria-label="Value"
            className={INPUT_CLASS}
            value={dateInputValue(condition.value)}
            onChange={(event) =>
              emitValue(
                condition,
                onChange,
                absoluteDateValue(event.target.value),
              )
            }
          />
        ) : (
          <>
            <Input
              type="number"
              min={0}
              aria-label="Amount"
              className={INPUT_CLASS}
              value={amount}
              onChange={(event) => {
                const next = event.target.value
                setAmount(next)
                emitValue(
                  condition,
                  onChange,
                  relativeDateValue(Number(next), unit),
                )
              }}
            />
            <LabeledSelect
              ariaLabel="Unit"
              value={unit}
              onValueChange={(value) => {
                const nextUnit = (value as RelativeUnit) ?? 'days'
                setUnit(nextUnit)
                emitValue(
                  condition,
                  onChange,
                  relativeDateValue(Number(amount), nextUnit),
                )
              }}
            >
              {RELATIVE_UNIT_OPTIONS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </LabeledSelect>
            {reading.suffix ? (
              <span className="text-[13px] text-muted-foreground">
                {reading.suffix}
              </span>
            ) : null}
          </>
        )}
      </div>
    </div>
  )
}

function SizeValueWidget({ condition, onChange }: WidgetProps) {
  const initial = sizeInputParts(condition.value)
  const [amount, setAmount] = useState(initial.amount)
  const [unit, setUnit] = useState<SizeUnit>(initial.unit)

  return (
    <div data-testid="value-widget-size" className="grid gap-1 text-[13px]">
      <div className="flex items-center gap-1.5">
        <Input
          type="number"
          min={0}
          aria-label="Value"
          className={INPUT_CLASS}
          value={amount}
          placeholder="size"
          onChange={(event) => {
            const next = event.target.value
            setAmount(next)
            emitValue(condition, onChange, bytesFromSize(Number(next), unit))
          }}
        />
        <LabeledSelect
          ariaLabel="Unit"
          value={unit}
          onValueChange={(value) => {
            const nextUnit = (value as SizeUnit) ?? 'kb'
            setUnit(nextUnit)
            emitValue(
              condition,
              onChange,
              bytesFromSize(Number(amount), nextUnit),
            )
          }}
        >
          {SIZE_UNIT_OPTIONS.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </LabeledSelect>
      </div>
    </div>
  )
}

function MailboxValueWidget({ condition, onChange }: WidgetProps) {
  const { accountId, mailboxes } = useConditionEditorData()
  return (
    <div data-testid="value-widget-mailbox" className="grid gap-1 text-[13px]">
      <MailboxSelect
        accountId={accountId}
        ariaLabel="Value"
        mailboxId={
          Array.isArray(condition.value) ? '' : String(condition.value)
        }
        staticMailboxes={mailboxes}
        onChange={(mailboxId) => emitValue(condition, onChange, mailboxId)}
      />
    </div>
  )
}

function AccountValueWidget({ condition, onChange }: WidgetProps) {
  const { accounts } = useConditionEditorData()
  const current = Array.isArray(condition.value) ? '' : String(condition.value)
  const value = current.trim().length > 0 ? current : UNSET_REF
  const known = accounts.some((account) => account.id === current)
  return (
    <div data-testid="value-widget-account" className="grid gap-1 text-[13px]">
      <LabeledSelect
        ariaLabel="Value"
        value={value}
        onValueChange={(next) =>
          emitValue(condition, onChange, pickedRefValue(next))
        }
      >
        <SelectItem value={UNSET_REF}>Choose account</SelectItem>
        {accounts.map((account) => (
          <SelectItem key={account.id} value={account.id}>
            {account.name}
          </SelectItem>
        ))}
        {current.trim().length > 0 && !known && (
          <SelectItem value={current}>{current}</SelectItem>
        )}
      </LabeledSelect>
    </div>
  )
}

function RoleValueWidget({ condition, onChange }: WidgetProps) {
  const current = Array.isArray(condition.value) ? '' : String(condition.value)
  const value = current.trim().length > 0 ? current : UNSET_REF
  const known = (ASSIGNABLE_MAILBOX_ROLES as readonly string[]).includes(
    current,
  )
  return (
    <div data-testid="value-widget-role" className="grid gap-1 text-[13px]">
      <LabeledSelect
        ariaLabel="Value"
        value={value}
        onValueChange={(next) =>
          emitValue(condition, onChange, pickedRefValue(next))
        }
      >
        <SelectItem value={UNSET_REF}>Choose role</SelectItem>
        {ASSIGNABLE_MAILBOX_ROLES.map((role) => (
          <SelectItem key={role} value={role}>
            {role.charAt(0).toUpperCase() + role.slice(1)}
          </SelectItem>
        ))}
        {current.trim().length > 0 && !known && (
          <SelectItem value={current}>{current}</SelectItem>
        )}
      </LabeledSelect>
    </div>
  )
}

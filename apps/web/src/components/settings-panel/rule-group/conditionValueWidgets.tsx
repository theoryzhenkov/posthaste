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
import { useState } from 'react'
import type {
  SmartMailboxCondition,
  SmartMailboxValue,
} from '../../../api/types'
import { ASSIGNABLE_MAILBOX_ROLES } from '../../../domainVocabulary'
import { Input } from '../../ui/input'
import { SelectItem } from '../../ui/select'
import { LabeledSelect, MailboxSelect } from '../MailboxSelect'
import { valueTypeForField } from '../helpers'
import { useConditionEditorData } from './conditionEditorContext'
import {
  dateInputValue,
  pickedRefValue,
  relativeDateValue,
  RELATIVE_UNIT_OPTIONS,
  splitListValue,
  toRfc3339FromDateInput,
  UNSET_REF,
  type RelativeUnit,
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
    default:
      // text | keyword | address — the generic text box (honest fallback;
      // keyword/address autocomplete are follow-on slices).
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

function DateValueWidget({ condition, onChange }: WidgetProps) {
  const [mode, setMode] = useState<'absolute' | 'relative'>('absolute')
  const [amount, setAmount] = useState('7')
  const [unit, setUnit] = useState<RelativeUnit>('days')

  return (
    <div data-testid="value-widget-date" className="grid gap-1 text-[13px]">
      <div className="flex items-center gap-1.5">
        <LabeledSelect
          ariaLabel="Date mode"
          value={mode}
          onValueChange={(value) =>
            setMode(value === 'relative' ? 'relative' : 'absolute')
          }
        >
          <SelectItem value="absolute">On</SelectItem>
          <SelectItem value="relative">Within</SelectItem>
        </LabeledSelect>
        {mode === 'absolute' ? (
          <Input
            type="date"
            aria-label="Value"
            className={INPUT_CLASS}
            value={dateInputValue(condition.value)}
            onChange={(event) =>
              emitValue(
                condition,
                onChange,
                toRfc3339FromDateInput(event.target.value),
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
                  relativeDateValue(Number(next), unit, new Date()),
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
                  relativeDateValue(Number(amount), nextUnit, new Date()),
                )
              }}
            >
              {RELATIVE_UNIT_OPTIONS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </LabeledSelect>
          </>
        )}
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

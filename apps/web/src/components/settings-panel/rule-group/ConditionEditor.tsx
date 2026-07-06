import type { SmartMailboxCondition } from '../../../api/types'
import { Button } from '../../ui/button'
import { Checkbox } from '../../ui/checkbox'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../ui/select'
import {
  defaultCondition,
  FIELD_OPTIONS,
  operatorOptionsForField,
  parseField,
  parseOperator,
  valueTypeForField,
} from '../helpers'
import { ConditionValueEditor } from './conditionValueWidgets'

/**
 * Single condition row editor: field, operator, value, and negate toggle.
 *
 * The VALUE input is type-directed — its widget is inferred from the field's
 * `valueType` (see `conditionValueWidgets.tsx`), while the emitted value keeps
 * the same wire shape (string | string[] | boolean) as the old text box.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
export function ConditionEditor({
  condition,
  onChange,
  onRemove,
}: {
  condition: SmartMailboxCondition
  onChange: (condition: SmartMailboxCondition) => void
  onRemove: () => void
}) {
  const operators = operatorOptionsForField(condition.field)
  const valueType = valueTypeForField(condition.field)
  const isBooleanField = valueType === 'boolean'
  // Date fields fold the operator into the value widget's natural reading
  // ("in the last N days" / "before <date>"), so the generic operator dropdown
  // is hidden — the user never sees "before" next to "within".
  const isDateField = valueType === 'date'

  return (
    <div className="grid gap-2 sm:grid-cols-[72px_minmax(0,1fr)_auto] sm:items-center">
      <span className="text-[12px] font-medium text-muted-foreground">
        Where
      </span>

      <div className="grid gap-2 lg:grid-cols-[minmax(0,1.05fr)_auto_minmax(0,0.85fr)_minmax(0,1.1fr)] lg:items-center">
        <FieldSelect condition={condition} onChange={onChange} />
        <label className="flex h-8 items-center justify-center gap-1.5 px-1 text-[12px] text-muted-foreground">
          <Checkbox
            checked={condition.negated}
            onCheckedChange={(checked) =>
              onChange({ ...condition, negated: checked === true })
            }
          />
          not
        </label>
        {isDateField ? (
          <span className="h-8" aria-hidden="true" />
        ) : (
          <OperatorSelect
            condition={condition}
            isBooleanField={isBooleanField}
            operators={operators}
            onChange={onChange}
          />
        )}
        <ConditionValueEditor condition={condition} onChange={onChange} />
      </div>

      <div className="flex items-center justify-end">
        <Button
          size="sm"
          variant="outline"
          type="button"
          className="h-8 rounded-md border-border bg-background px-2 font-mono text-[12px] text-muted-foreground hover:text-destructive"
          aria-label="Remove expression"
          onClick={onRemove}
        >
          -
        </Button>
      </div>
    </div>
  )
}

function FieldSelect({
  condition,
  onChange,
}: {
  condition: SmartMailboxCondition
  onChange: (condition: SmartMailboxCondition) => void
}) {
  return (
    <div className="grid gap-1 text-[13px]">
      <Select
        value={condition.field}
        onValueChange={(value) => {
          const field = parseField(value, condition.field)
          const nextOperator = operatorOptionsForField(field)[0]
          onChange({ ...defaultCondition(field), operator: nextOperator })
        }}
      >
        <SelectTrigger
          aria-label="Field"
          className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {FIELD_OPTIONS.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

function OperatorSelect({
  condition,
  isBooleanField,
  operators,
  onChange,
}: {
  condition: SmartMailboxCondition
  isBooleanField: boolean
  operators: string[]
  onChange: (condition: SmartMailboxCondition) => void
}) {
  return (
    <div className="grid gap-1 text-[13px]">
      <Select
        value={condition.operator}
        onValueChange={(value) => {
          const operator = parseOperator(
            value,
            condition.field,
            condition.operator,
          )
          onChange({
            ...condition,
            operator,
            value: operator === 'in' ? [] : isBooleanField ? false : '',
          })
        }}
      >
        <SelectTrigger
          aria-label="Operator"
          className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {operators.map((operator) => (
            <SelectItem key={operator} value={operator}>
              {operator}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

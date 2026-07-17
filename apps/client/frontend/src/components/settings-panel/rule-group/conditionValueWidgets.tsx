/**
 * Type-directed VALUE widgets for a single condition row — ONE registry
 * ({@link VALUE_WIDGETS}) mapping the field's `valueType` (see
 * `fieldRegistry.ts`) to its widgets, COMPOSED with the operator's arity:
 *
 * * scalar operators (`equals`/`contains`/…) render the type's `Scalar` widget;
 * * the list operator (`in`) renders the generic {@link ListValueEditor}
 *   around the type's `ListEntry` widget.
 *
 * Because capabilities (address-book autocomplete, tag suggestions, the
 * mailbox/account/role pickers) hang off the VALUE TYPE — not off one
 * (type × operator) cell — every (field type × operator) combination the grammar
 * admits keeps its capabilities: `fromEmail × in` gets the
 * same autocomplete `fromEmail × equals` does. (The old dispatch special-cased
 * `in` into a bare comma-separated text box BEFORE the type switch, which is
 * exactly how "switch to 'is one of' and autocomplete stops working" happened.)
 *
 * WIRE-SHAPE PARITY (load-bearing): every widget emits the exact same
 * `MailQueryValue` the old text box did — a `string` for single-value ops, a
 * `string[]` for the `in` operator, a `boolean` for boolean fields. The pickers
 * only change how the user enters that value, never its serialized shape, so
 * the compiler/evaluator and stored JSON are unchanged.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import { useState, type ReactNode } from 'react'
import { X } from 'lucide-react'

import type {
  MailQueryCondition,
  MailQueryOperator,
  MailQueryValue,
} from '../../../api/types'
import { RecipientSuggestionInput } from '@/components/compose-overlay/RecipientSuggestionInput'
import { ASSIGNABLE_MAILBOX_ROLES } from '../../../domainVocabulary'
import { Button } from '../../ui/button'
import { Input } from '../../ui/input'
import { SelectItem } from '../../ui/select'
import { LabeledSelect, MailboxSelect } from '../MailboxSelect'
import { valueTypeForField, type ConditionValueType } from '../helpers'
import { useConditionEditorData } from './conditionEditorContext'
import {
  absoluteDateValue,
  appendListEntries,
  bytesFromSize,
  dateInputValue,
  dateValueMode,
  listValueEntries,
  pickedRefValue,
  relativeDateValue,
  relativeParts,
  removeListEntry,
  RELATIVE_UNIT_OPTIONS,
  SIZE_UNIT_OPTIONS,
  sizeInputParts,
  UNSET_REF,
  type RelativeUnit,
  type SizeUnit,
} from './conditionValueFormat'
import {
  useAddressBookSuggestions,
  useKeywordSuggestions,
} from './suggestionSources'
import { TextSuggestionInput } from './TextSuggestionInput'

// ---------------------------------------------------------------------------
// The registry + dispatch
// ---------------------------------------------------------------------------

const INPUT_CLASS =
  'h-8 rounded-md border-border bg-background text-[13px] shadow-none'

type WidgetProps = {
  condition: MailQueryCondition
  onChange: (condition: MailQueryCondition) => void
}

/** Props for a type's `in`-list ENTRY widget: edit a draft, commit entries. */
type ListEntryProps = {
  /** The free-text draft (unused by pure pickers, which commit on selection). */
  draft: string
  onDraftChange: (draft: string) => void
  /** Commit one entry (or a comma-separated batch) to the list. */
  onCommit: (entry: string) => void
  /** Placeholder for text-shaped entries. */
  placeholder?: string
}

/**
 * One value type's widgets. `Scalar` renders for single-value operators;
 * `ListEntry` is the adder the generic {@link ListValueEditor} wraps for `in`.
 * Types whose grammar never offers `in` (`boolean`/`date`/`size`) omit
 * `ListEntry`; the plain-text entry is the safety net.
 */
interface ValueWidgetSpec {
  Scalar: (props: WidgetProps) => ReactNode
  ListEntry?: (props: ListEntryProps) => ReactNode
  /** Placeholder for the plain-text list entry fallback. */
  listPlaceholder?: string
}

const VALUE_WIDGETS: Record<ConditionValueType, ValueWidgetSpec> = {
  text: {
    Scalar: TextValueWidget,
    listPlaceholder: 'value — Enter to add',
  },
  boolean: { Scalar: BooleanValueWidget },
  date: { Scalar: DateValueWidget },
  size: { Scalar: SizeValueWidget },
  mailboxRef: { Scalar: MailboxValueWidget, ListEntry: MailboxListEntry },
  accountRef: { Scalar: AccountValueWidget, ListEntry: AccountListEntry },
  roleEnum: { Scalar: RoleValueWidget, ListEntry: RoleListEntry },
  keyword: { Scalar: KeywordValueWidget, ListEntry: KeywordListEntry },
  address: { Scalar: AddressValueWidget, ListEntry: AddressListEntry },
}

/**
 * The type-directed value editor: value type selects the widget row from the
 * registry, the operator's arity selects scalar vs. list within it.
 */
export function ConditionValueEditor({
  condition,
  onChange,
}: {
  condition: MailQueryCondition
  onChange: (condition: MailQueryCondition) => void
}) {
  const valueType = valueTypeForField(condition.field)
  const spec = VALUE_WIDGETS[valueType]
  if (condition.operator === 'in' && valueType !== 'boolean') {
    return (
      <ListValueEditor condition={condition} onChange={onChange} spec={spec} />
    )
  }
  return <spec.Scalar condition={condition} onChange={onChange} />
}

function emitValue(
  condition: MailQueryCondition,
  onChange: (condition: MailQueryCondition) => void,
  value: MailQueryValue,
) {
  onChange({ ...condition, value })
}

// ---------------------------------------------------------------------------
// The generic `in` list editor (chips + a type-directed entry widget)
// ---------------------------------------------------------------------------

/**
 * The generic multi-value editor for the `in` operator: the current entries as
 * removable chips plus the value type's OWN entry widget (autocomplete,
 * pickers) as the adder. Emits the same `string[]` wire shape the old
 * comma-separated box did; a typed entry may still be a comma-separated batch
 * (paste convenience — split exactly like before).
 */
function ListValueEditor({
  condition,
  onChange,
  spec,
}: WidgetProps & { spec: ValueWidgetSpec }) {
  const [draft, setDraft] = useState('')
  const entries = listValueEntries(condition.value)

  const commit = (entry: string) => {
    const next = appendListEntries(entries, entry)
    if (next.length !== entries.length) {
      emitValue(condition, onChange, next)
    }
    setDraft('')
  }

  const Entry = spec.ListEntry ?? TextListEntry
  return (
    <div data-testid="value-widget-list" className="grid gap-1 text-[13px]">
      {entries.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {entries.map((entry, index) => (
            <span
              key={`${entry}-${index}`}
              className="inline-flex max-w-full items-center gap-1 rounded-md border border-border bg-bg-elev/60 py-0.5 pl-2 pr-1 text-[12px]"
            >
              <span className="min-w-0 truncate">{entry}</span>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className="h-4 w-4 rounded-sm p-0 text-muted-foreground hover:text-destructive"
                aria-label={`Remove ${entry}`}
                onClick={() =>
                  emitValue(
                    condition,
                    onChange,
                    removeListEntry(entries, index),
                  )
                }
              >
                <X size={11} strokeWidth={2} />
              </Button>
            </span>
          ))}
        </div>
      )}
      <Entry
        draft={draft}
        onDraftChange={setDraft}
        onCommit={commit}
        placeholder={spec.listPlaceholder}
      />
    </div>
  )
}

/** Fallback list entry: a plain text box, Enter (or blur) commits the draft. */
function TextListEntry({
  draft,
  onDraftChange,
  onCommit,
  placeholder,
}: ListEntryProps) {
  return (
    <Input
      type="text"
      aria-label="Add value"
      className={INPUT_CLASS}
      value={draft}
      placeholder={placeholder ?? 'value — Enter to add'}
      onChange={(event) => onDraftChange(event.target.value)}
      onBlur={() => {
        if (draft.trim().length > 0) onCommit(draft)
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && draft.trim().length > 0) {
          event.preventDefault()
          onCommit(draft)
        }
      }}
    />
  )
}

// ---------------------------------------------------------------------------
// address — the compose-shared address-book autocomplete, both arities
// ---------------------------------------------------------------------------

/**
 * Address fields (`fromEmail`, `fromName`, `to`) share the SAME autocomplete
 * the compose recipient inputs use: the persistent server-side address book
 * (`senderAddresses`) fed through the reused `RecipientSuggestionInput`. A
 * scalar condition holds a single address, so the input runs in `replace` mode
 * (a pick sets the bare email) and still emits the identical `string` wire
 * shape the old text box did.
 */
function AddressValueWidget({ condition, onChange }: WidgetProps) {
  const suggestions = useAddressBookSuggestions()
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

/** The same autocomplete as the adder of an `in` list: a pick or Enter commits
 *  the address as one entry. This is what "is one of" + autocomplete means. */
function AddressListEntry({ draft, onDraftChange, onCommit }: ListEntryProps) {
  const suggestions = useAddressBookSuggestions()
  return (
    <RecipientSuggestionInput
      ariaLabel="Add value"
      selectionMode="replace"
      placeholder="name or email — Enter to add"
      suggestions={suggestions}
      value={draft}
      onChange={onDraftChange}
      onEnter={onCommit}
      onPick={onCommit}
    />
  )
}

// ---------------------------------------------------------------------------
// keyword — live tag suggestions, both arities
// ---------------------------------------------------------------------------

function KeywordValueWidget({ condition, onChange }: WidgetProps) {
  const suggestions = useKeywordSuggestions()
  return (
    <div data-testid="value-widget-keyword" className="grid gap-1 text-[13px]">
      <TextSuggestionInput
        ariaLabel="Value"
        className={INPUT_CLASS}
        placeholder="tag"
        suggestions={suggestions}
        value={Array.isArray(condition.value) ? '' : String(condition.value)}
        onChange={(next) => emitValue(condition, onChange, next)}
      />
    </div>
  )
}

function KeywordListEntry({ draft, onDraftChange, onCommit }: ListEntryProps) {
  const suggestions = useKeywordSuggestions()
  return (
    <TextSuggestionInput
      ariaLabel="Add value"
      className={INPUT_CLASS}
      placeholder="tag — Enter to add"
      suggestions={suggestions}
      value={draft}
      onChange={onDraftChange}
      onEnter={onCommit}
      onPick={onCommit}
    />
  )
}

// ---------------------------------------------------------------------------
// text / boolean
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// date
// ---------------------------------------------------------------------------

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
  operator: MailQueryOperator
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
function readingForCondition(condition: MailQueryCondition): DateReading {
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

// ---------------------------------------------------------------------------
// size
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// mailboxRef / accountRef / roleEnum — pickers, both arities
// ---------------------------------------------------------------------------

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

/** The same mailbox picker as an `in` adder: each pick appends an entry. */
function MailboxListEntry({ onCommit }: ListEntryProps) {
  const { accountId, mailboxes } = useConditionEditorData()
  return (
    <MailboxSelect
      accountId={accountId}
      ariaLabel="Add value"
      mailboxId=""
      staticMailboxes={mailboxes}
      onChange={(mailboxId) => {
        if (mailboxId.trim().length > 0) onCommit(mailboxId)
      }}
    />
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

function AccountListEntry({ onCommit }: ListEntryProps) {
  const { accounts } = useConditionEditorData()
  return (
    <LabeledSelect
      ariaLabel="Add value"
      value={UNSET_REF}
      onValueChange={(next) => {
        const picked = pickedRefValue(next)
        if (picked.length > 0) onCommit(picked)
      }}
    >
      <SelectItem value={UNSET_REF}>Add account</SelectItem>
      {accounts.map((account) => (
        <SelectItem key={account.id} value={account.id}>
          {account.name}
        </SelectItem>
      ))}
    </LabeledSelect>
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

function RoleListEntry({ onCommit }: ListEntryProps) {
  return (
    <LabeledSelect
      ariaLabel="Add value"
      value={UNSET_REF}
      onValueChange={(next) => {
        const picked = pickedRefValue(next)
        if (picked.length > 0) onCommit(picked)
      }}
    >
      <SelectItem value={UNSET_REF}>Add role</SelectItem>
      {ASSIGNABLE_MAILBOX_ROLES.map((role) => (
        <SelectItem key={role} value={role}>
          {role.charAt(0).toUpperCase() + role.slice(1)}
        </SelectItem>
      ))}
    </LabeledSelect>
  )
}

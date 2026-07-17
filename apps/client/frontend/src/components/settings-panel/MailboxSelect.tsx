/**
 * Shared mailbox picker used by the automation move-action editor AND the
 * type-directed condition editor's `mailboxId` value widget. Single source so
 * the condition builder reuses the exact picker the move action uses rather
 * than forking a second one.
 *
 */
import type React from 'react'
import type { Mailbox } from '../../api/types'
import { useMailboxCounts } from '@/data'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select'

/**
 * A labelled `<Select>`. Pass `label` to render the field label above the
 * control; omit it (e.g. inside a compact condition row) and pass `ariaLabel`
 * so the control stays accessible without the visible label.
 */
export function LabeledSelect({
  label,
  ariaLabel,
  value,
  onValueChange,
  children,
}: {
  label?: string
  ariaLabel?: string
  value: string
  onValueChange: (value: string) => void
  children: React.ReactNode
}) {
  const control = (
    <Select value={value} onValueChange={onValueChange}>
      <SelectTrigger
        aria-label={ariaLabel ?? label}
        className="h-8 w-full rounded-md border-border bg-background text-[13px] shadow-none"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>{children}</SelectContent>
    </Select>
  )

  if (!label) {
    return <div className="grid gap-1 text-[13px]">{control}</div>
  }

  return (
    <label className="grid gap-1.5 text-[13px]">
      <span className="text-[12px] font-medium text-muted-foreground">
        {label}
      </span>
      {control}
    </label>
  )
}

export function MailboxSelect({
  accountId,
  label,
  ariaLabel,
  mailboxId,
  staticMailboxes,
  onChange,
}: {
  accountId: string
  label?: string
  ariaLabel?: string
  mailboxId: string
  staticMailboxes: Mailbox[] | null
  onChange: (mailboxId: string) => void
}) {
  const mailboxesQuery = useMailboxCounts(accountId, {
    enabled: staticMailboxes === null && accountId.trim().length > 0,
  })
  const mailboxes =
    staticMailboxes ?? mailboxesQuery.data?.rows.map((row) => row.mailbox) ?? []
  const value = mailboxId.trim().length > 0 ? mailboxId : '__unset__'

  return (
    <LabeledSelect
      label={label}
      ariaLabel={ariaLabel}
      value={value}
      onValueChange={(value) =>
        onChange(value.startsWith('__unset__') ? '' : value)
      }
    >
      <SelectItem value="__unset__">Choose mailbox</SelectItem>
      {mailboxes.map((mailbox) => (
        <SelectItem key={mailbox.id} value={mailbox.id}>
          {mailbox.name}
        </SelectItem>
      ))}
      {mailboxId.trim().length > 0 &&
        !mailboxes.some((mailbox) => mailbox.id === mailboxId) && (
          <SelectItem value={mailboxId}>{mailboxId}</SelectItem>
        )}
    </LabeledSelect>
  )
}

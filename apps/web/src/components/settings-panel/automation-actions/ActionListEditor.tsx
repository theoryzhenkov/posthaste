import { useQuery } from '@tanstack/react-query'
import type React from 'react'
import type { AutomationAction, Mailbox } from '../../../api/types'
import { queryKeys } from '../../../queryKeys'
import { runtimeViews } from '../../../runtime/views'
import { Button } from '../../ui/button'
import { Input } from '../../ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../ui/select'
import {
  ACTION_KIND_OPTIONS,
  actionForKind,
  defaultAction,
  parseActionKind,
} from '../automationRuleHelpers'
import { Field } from '../shared'

export function LabeledSelect({
  label,
  value,
  onValueChange,
  children,
}: {
  label: string
  value: string
  onValueChange: (value: string) => void
  children: React.ReactNode
}) {
  return (
    <label className="grid gap-1.5 text-[13px]">
      <span className="text-[12px] font-medium text-muted-foreground">
        {label}
      </span>
      <Select value={value} onValueChange={onValueChange}>
        <SelectTrigger className="h-8 w-full rounded-md border-border bg-background text-[13px] shadow-none">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>{children}</SelectContent>
      </Select>
    </label>
  )
}

export function ActionListEditor({
  accountId,
  actions,
  staticMailboxes,
  onChange,
}: {
  accountId: string
  actions: AutomationAction[]
  staticMailboxes: Mailbox[] | null
  onChange: (actions: AutomationAction[]) => void
}) {
  return (
    <div className="space-y-3">
      <div className="flex justify-end">
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 rounded-md border-border bg-background px-2 text-[12px]"
          onClick={() => onChange([...actions, defaultAction()])}
        >
          Add action
        </Button>
      </div>
      <div className="space-y-2">
        {actions.map((action, index) => (
          <ActionRow
            key={index}
            accountId={accountId}
            action={action}
            staticMailboxes={staticMailboxes}
            onChange={(nextAction) =>
              onChange(
                actions.map((candidate, candidateIndex) =>
                  candidateIndex === index ? nextAction : candidate,
                ),
              )
            }
            onRemove={() =>
              onChange(
                actions.filter((_, candidateIndex) => candidateIndex !== index),
              )
            }
          />
        ))}
      </div>
    </div>
  )
}

function ActionRow({
  accountId,
  action,
  staticMailboxes,
  onChange,
  onRemove,
}: {
  accountId: string
  action: AutomationAction
  staticMailboxes: Mailbox[] | null
  onChange: (action: AutomationAction) => void
  onRemove: () => void
}) {
  return (
    <div className="grid gap-2 sm:grid-cols-[minmax(150px,0.9fr)_minmax(160px,1fr)_auto] sm:items-end">
      <LabeledSelect
        label="Action"
        value={action.kind}
        onValueChange={(value) =>
          onChange(actionForKind(parseActionKind(value, action.kind)))
        }
      >
        {ACTION_KIND_OPTIONS.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {option.label}
          </SelectItem>
        ))}
      </LabeledSelect>
      <ActionValueEditor
        accountId={accountId}
        action={action}
        staticMailboxes={staticMailboxes}
        onChange={onChange}
      />
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className="h-8 justify-self-start px-2 text-muted-foreground hover:text-destructive sm:justify-self-end"
        onClick={onRemove}
      >
        Remove
      </Button>
    </div>
  )
}

function ActionValueEditor({
  accountId,
  action,
  staticMailboxes,
  onChange,
}: {
  accountId: string
  action: AutomationAction
  staticMailboxes: Mailbox[] | null
  onChange: (action: AutomationAction) => void
}) {
  if (action.kind === 'applyTag' || action.kind === 'removeTag') {
    return (
      <Field
        label="Tag"
        value={action.tag}
        placeholder="newsletter"
        onChange={(tag) => onChange({ ...action, tag })}
      />
    )
  }
  if (action.kind === 'moveToMailbox') {
    return (
      <MailboxSelect
        accountId={accountId}
        label="Target mailbox"
        mailboxId={action.mailboxId}
        staticMailboxes={staticMailboxes}
        onChange={(mailboxId) => onChange({ ...action, mailboxId })}
      />
    )
  }
  return (
    <label className="grid gap-1.5 text-[13px]">
      <span className="text-[12px] font-medium text-muted-foreground">
        Value
      </span>
      <Input
        className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
        value="No value"
        disabled
      />
    </label>
  )
}

function MailboxSelect({
  accountId,
  label,
  mailboxId,
  staticMailboxes,
  onChange,
}: {
  accountId: string
  label: string
  mailboxId: string
  staticMailboxes: Mailbox[] | null
  onChange: (mailboxId: string) => void
}) {
  const mailboxesQuery = useQuery({
    queryKey: queryKeys.mailboxes(accountId),
    queryFn: () => runtimeViews.mail.mailboxes(accountId),
    enabled: staticMailboxes === null && accountId.trim().length > 0,
  })
  const mailboxes = staticMailboxes ?? mailboxesQuery.data ?? []
  const value = mailboxId.trim().length > 0 ? mailboxId : '__unset__'

  return (
    <LabeledSelect
      label={label}
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

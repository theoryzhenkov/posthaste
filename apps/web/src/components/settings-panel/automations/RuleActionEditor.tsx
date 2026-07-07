/**
 * Action picker for a GUI-created automation rule (RFC-L2-scripting ruling 23).
 *
 * Edits a {@link WritableRuleAction} — the SAFE action set only, driven by the
 * registry in `ruleActionHelpers.ts` (tag / move / moveToRole / markRead /
 * flag / notify / destroy / emit / webhook). There is deliberately **no
 * `exec`**: it is not a variant of `WritableRuleAction`, so neither the
 * registry nor this editor can express it (exec stays config-file-only — a
 * GUI-settable exec would be remote code execution).
 *
 * `destroy` is mail-destructive and rendered unmistakably so: destructive
 * styling in the picker + hint, and an explanatory irreversibility callout in
 * the form (plus the save-guard in `AutomationsPane` mirroring the server's
 * non-empty-WHEN validation).
 *
 * The webhook branch surfaces the security guidance INLINE (a product
 * requirement, not decoration — docs/scripting-security.md threat 2): grants
 * default to least privilege (`read`), and granting anything beyond read, or
 * pointing at a non-local host, shows the prompt-injection warning and nudges
 * sender-scoping the WHEN-clause.
 */
import { AlertTriangle, TriangleAlert } from 'lucide-react'

import {
  RULE_MOVE_ROLES,
  type RuleGrant,
  type RuleMoveRole,
  type WritableRuleAction,
} from '../../../api/types'
import { Checkbox } from '../../ui/checkbox'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../ui/select'
import { Field } from '../shared'
import { LabeledSelect } from '../MailboxSelect'
import {
  ACTION_KIND_OPTIONS,
  ACTION_REGISTRY,
  type ActionKind,
  defaultActionForKind,
} from './ruleActionHelpers'

const MOVE_ROLE_OPTIONS: { value: RuleMoveRole; label: string }[] = [
  { value: 'archive', label: 'Archive' },
  { value: 'junk', label: 'Junk' },
  { value: 'trash', label: 'Trash (recoverable)' },
  { value: 'inbox', label: 'Inbox' },
]

const GRANT_OPTIONS: { value: RuleGrant; label: string }[] = [
  { value: 'read', label: 'read' },
  { value: 'tag', label: 'tag' },
  { value: 'move', label: 'move' },
  { value: 'send', label: 'send' },
  { value: 'delete', label: 'delete' },
]

function isLocalHost(url: string): boolean {
  try {
    const { hostname } = new URL(url)
    return (
      hostname === 'localhost' ||
      hostname === '127.0.0.1' ||
      hostname === '::1' ||
      hostname === '[::1]'
    )
  } catch {
    // An unparseable/empty URL isn't yet a known host — don't warn about it.
    return true
  }
}

export function RuleActionEditor({
  action,
  onChange,
}: {
  action: WritableRuleAction
  onChange: (action: WritableRuleAction) => void
}) {
  const descriptor = ACTION_REGISTRY[action.kind]
  return (
    <div className="space-y-4">
      <label className="grid gap-1.5 text-[13px]">
        <span className="text-[12px] font-medium text-muted-foreground">
          Do this
        </span>
        <Select
          value={action.kind}
          onValueChange={(value) =>
            onChange(defaultActionForKind(value as ActionKind))
          }
        >
          <SelectTrigger className="h-8 min-w-48 rounded-md border-border bg-background text-[13px] shadow-none">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {ACTION_KIND_OPTIONS.map((option) => (
              <SelectItem key={option.kind} value={option.kind}>
                <span
                  className={
                    option.destructive ? 'font-medium text-destructive' : ''
                  }
                >
                  {option.label}
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span
          className={
            descriptor.destructive
              ? 'text-[12px] font-medium text-destructive'
              : 'text-[12px] text-muted-foreground'
          }
        >
          {descriptor.hint}
        </span>
      </label>

      {action.kind === 'tag' && (
        <Field
          label="Tag"
          value={action.tag}
          placeholder="e.g. reviewed"
          onChange={(tag) => onChange({ kind: 'tag', tag })}
        />
      )}

      {action.kind === 'move' && (
        <Field
          label="Mailbox id"
          value={action.mailboxId}
          placeholder="e.g. Archive"
          onChange={(mailboxId) => onChange({ kind: 'move', mailboxId })}
        />
      )}

      {action.kind === 'moveToRole' && (
        <div className="grid max-w-56 gap-1.5">
          <LabeledSelect
            label="Move to"
            value={action.role}
            onValueChange={(role) =>
              onChange({
                kind: 'moveToRole',
                role: (RULE_MOVE_ROLES as readonly string[]).includes(role)
                  ? (role as RuleMoveRole)
                  : 'archive',
              })
            }
          >
            {MOVE_ROLE_OPTIONS.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </LabeledSelect>
          <span className="text-[12px] text-muted-foreground">
            Files the message into the mailbox carrying this role on its own
            account — one rule covers every account.
          </span>
        </div>
      )}

      {action.kind === 'markRead' && (
        <div className="grid max-w-56 gap-1.5">
          <LabeledSelect
            label="Mark as"
            value={action.read ? 'read' : 'unread'}
            onValueChange={(value) =>
              onChange({ kind: 'markRead', read: value === 'read' })
            }
          >
            <SelectItem value="read">read</SelectItem>
            <SelectItem value="unread">unread</SelectItem>
          </LabeledSelect>
        </div>
      )}

      {action.kind === 'flag' && (
        <div className="grid max-w-56 gap-1.5">
          <LabeledSelect
            label="Flag state"
            value={action.flagged ? 'flagged' : 'unflagged'}
            onValueChange={(value) =>
              onChange({ kind: 'flag', flagged: value === 'flagged' })
            }
          >
            <SelectItem value="flagged">flagged</SelectItem>
            <SelectItem value="unflagged">not flagged</SelectItem>
          </LabeledSelect>
        </div>
      )}

      {action.kind === 'notify' && (
        <div className="space-y-3">
          <Field
            label="Title"
            value={action.title}
            placeholder="e.g. Invoice arrived"
            onChange={(title) => onChange({ ...action, kind: 'notify', title })}
          />
          <Field
            label="Body (optional)"
            value={action.body ?? ''}
            onChange={(body) =>
              onChange({ ...action, kind: 'notify', body: body || null })
            }
          />
        </div>
      )}

      {action.kind === 'destroy' && <DestroyCallout />}

      {action.kind === 'emit' && (
        <p className="rounded-md border border-dashed border-border-soft px-3 py-3 text-[12px] text-muted-foreground">
          Emits only the <code>rule.fired</code> fact. Pair it with a
          client-side <code>posthastectl watch --rule …</code> that runs your
          handler where your code lives — the server decides <em>whether</em>,
          the edge decides <em>how</em>.
        </p>
      )}

      {action.kind === 'webhook' && (
        <WebhookFields action={action} onChange={onChange} />
      )}
    </div>
  )
}

/** The unmistakable destroy warning (a destructive, irreversible action). */
function DestroyCallout() {
  return (
    <div
      data-testid="destroy-action-callout"
      className="flex gap-2.5 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2.5 text-[12px] leading-5 text-destructive"
    >
      <TriangleAlert
        size={15}
        strokeWidth={1.8}
        className="mt-[1px] shrink-0"
      />
      <div className="space-y-1">
        <p className="font-medium">
          Permanently deletes matching messages — this cannot be undone.
        </p>
        <p>
          Every message this rule matches is destroyed outright: it does NOT go
          to Trash and cannot be recovered, on this device or the server. The
          rule runs unattended on every matching message, so make the conditions
          above as narrow as possible (a specific sender, an exact subject). A
          rule with no conditions is refused.
        </p>
      </div>
    </div>
  )
}

function WebhookFields({
  action,
  onChange,
}: {
  action: Extract<WritableRuleAction, { kind: 'webhook' }>
  onChange: (action: WritableRuleAction) => void
}) {
  const grants = action.grants ?? []
  const hasWriteGrant = grants.some((g) => g !== 'read')
  const remoteHost = action.url.length > 0 && !isLocalHost(action.url)

  const toggleGrant = (grant: RuleGrant, checked: boolean) => {
    const next = checked
      ? Array.from(new Set([...grants, grant]))
      : grants.filter((g) => g !== grant)
    onChange({ ...action, grants: next })
  }

  return (
    <div className="space-y-4">
      <Field
        label="Webhook URL"
        value={action.url}
        placeholder="http://127.0.0.1:8787/hook"
        onChange={(url) => onChange({ ...action, url })}
      />

      <div className="grid gap-1.5 text-[13px]">
        <span className="text-[12px] font-medium text-muted-foreground">
          Token grants
        </span>
        <div className="flex flex-wrap gap-3">
          {GRANT_OPTIONS.map((option) => (
            <label
              key={option.value}
              className="flex items-center gap-1.5 text-[12px] text-muted-foreground"
            >
              <Checkbox
                checked={grants.includes(option.value)}
                onCheckedChange={(checked) =>
                  toggleGrant(option.value, checked === true)
                }
              />
              {option.label}
            </label>
          ))}
        </div>
        <span className="text-[12px] text-muted-foreground">
          The rule mints a per-invocation token carrying exactly these grants,
          scoped to the matched message. Grant the least the handler needs — the
          default is <code>read</code>.
        </span>
      </div>

      <Field
        label="Token expiry (seconds)"
        type="number"
        value={action.expirySeconds ?? 3600}
        onChange={(value) =>
          onChange({
            ...action,
            expirySeconds: Number.parseInt(value, 10) || 3600,
          })
        }
      />

      {(hasWriteGrant || remoteHost) && (
        <SecurityCallout
          hasWriteGrant={hasWriteGrant}
          remoteHost={remoteHost}
        />
      )}
    </div>
  )
}

/** Inline prompt-injection warning (threat 2). Shown when a webhook grants more
 *  than read, or targets a non-local host — the two ways the blast radius of a
 *  hijacked handler grows. */
function SecurityCallout({
  hasWriteGrant,
  remoteHost,
}: {
  hasWriteGrant: boolean
  remoteHost: boolean
}) {
  return (
    <div className="flex gap-2.5 rounded-md border border-amber-500/25 bg-amber-500/5 px-3 py-2.5 text-[12px] leading-5 text-amber-800 dark:text-amber-300">
      <AlertTriangle
        size={15}
        strokeWidth={1.8}
        className="mt-[1px] shrink-0"
      />
      <div className="space-y-1">
        <p className="font-medium">Prompt-injection surface</p>
        <p>
          Anyone who can make a message match this rule can drive whatever your
          handler does.{' '}
          {hasWriteGrant && 'This token can write, not just read. '}
          {remoteHost && 'It also POSTs message content to a non-local host. '}
          Scope the WHEN-clause to trusted senders (e.g.{' '}
          <code>from:you@yourdomain.com</code>) and grant the least the handler
          needs — never hand write/send scope to a rule that feeds untrusted
          content to an autonomous agent.
        </p>
      </div>
    </div>
  )
}

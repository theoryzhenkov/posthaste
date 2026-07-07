/**
 * The Automations settings pane (RFC-L2-scripting ruling 23): in-app creation
 * of SAFE automation rules — tag / move / notify / emit / webhook. exec is
 * config-file-only and never creatable here (it is not a variant of the write
 * body type), so authored exec rules from rules.toml are shown read-only.
 *
 * Reuses the shared WHEN-clause grammar builder ({@link RuleGroupEditor}) for
 * the rule's `when`, and a purpose-built {@link RuleActionEditor} for the safe
 * action set. Security guidance (least-grant default + prompt-injection /
 * sender-scope nudges) is surfaced inline by the action editor.
 */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Pencil, Plus, Trash2, Workflow } from 'lucide-react'
import { useMemo, useState } from 'react'

import type {
  Rule,
  SmartMailboxGroup,
  WritableRuleAction,
  WritableRuleInput,
} from '../../api/types'
import { queryKeys } from '../../queryKeys'
import { runtimeMutations } from '../../runtime/mutations'
import { runtimeViews } from '../../runtime/views'
import { Badge } from '../ui/badge'
import { Button } from '../ui/button'
import { Checkbox } from '../ui/checkbox'
import { RuleActionEditor } from './automations/RuleActionEditor'
import {
  actionSummary,
  defaultActionForKind,
  isDestructiveActionKind,
} from './automations/ruleActionHelpers'
import { defaultEmptyRule } from './helpers'
import { ConditionEditorContext } from './rule-group/conditionEditorContext'
import { RuleGroupEditor } from './RuleGroupEditor'
import {
  Field,
  FeedbackBanner,
  SettingsBackButton,
  SettingsEmptyState,
  SettingsList,
  SettingsPage,
  SettingsPageHeader,
} from './shared'

interface RuleForm {
  name: string
  when: Rule['when']
  action: WritableRuleAction
  enabled: boolean
  stopProcessing: boolean
}

/** Does the WHEN tree contain at least one leaf condition? Mirrors the server's
 *  destroy guard (`validate_rule_action`), so the editor can refuse an
 *  unconditional destroy BEFORE the request instead of surfacing a 400. */
function groupHasCondition(group: SmartMailboxGroup): boolean {
  return group.nodes.some((node) =>
    node.type === 'condition' ? true : groupHasCondition(node),
  )
}

type EditorState =
  | { mode: 'list' }
  | { mode: 'new' }
  | { mode: 'edit'; id: string }

function isExec(rule: Rule): boolean {
  return rule.action.kind === 'exec'
}

/** A safe action, or a fresh tag action if the source is exec (unreachable in
 *  practice — exec rules are not editable). */
function writableActionOf(rule: Rule): WritableRuleAction {
  return rule.action.kind === 'exec'
    ? defaultActionForKind('tag')
    : (rule.action as WritableRuleAction)
}

function formFromRule(rule: Rule): RuleForm {
  return {
    name: rule.name,
    when: rule.when,
    action: writableActionOf(rule),
    enabled: rule.enabled,
    stopProcessing: rule.stopProcessing === true,
  }
}

function emptyForm(): RuleForm {
  return {
    name: '',
    when: defaultEmptyRule(),
    action: defaultActionForKind('tag'),
    enabled: true,
    stopProcessing: false,
  }
}

export function AutomationsPane() {
  const queryClient = useQueryClient()
  const [editor, setEditor] = useState<EditorState>({ mode: 'list' })
  const [form, setForm] = useState<RuleForm>(emptyForm())
  const [error, setError] = useState<string | null>(null)

  const rulesQuery = useQuery({
    queryKey: queryKeys.rules,
    queryFn: runtimeViews.rules.list,
  })
  const rules = rulesQuery.data ?? []

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: queryKeys.rules })

  const saveMutation = useMutation({
    mutationFn: (input: { id?: string; body: WritableRuleInput }) =>
      input.id
        ? runtimeMutations.rules.update(input.id, input.body)
        : runtimeMutations.rules.create(input.body),
    onSuccess: async () => {
      await invalidate()
      setEditor({ mode: 'list' })
      setError(null)
    },
    onError: (err: Error) => setError(err.message),
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => runtimeMutations.rules.delete(id),
    onSuccess: invalidate,
    onError: (err: Error) => setError(err.message),
  })

  const openNew = () => {
    setForm(emptyForm())
    setError(null)
    setEditor({ mode: 'new' })
  }

  const openEdit = (rule: Rule) => {
    setForm(formFromRule(rule))
    setError(null)
    setEditor({ mode: 'edit', id: rule.id })
  }

  const save = () => {
    const body: WritableRuleInput = {
      name: form.name.trim(),
      when: form.when,
      action: form.action,
      enabled: form.enabled,
      stopProcessing: form.stopProcessing,
    }
    saveMutation.mutate(
      editor.mode === 'edit' ? { id: editor.id, body } : { body },
    )
  }

  if (editor.mode !== 'list') {
    return (
      <RuleEditor
        heading={editor.mode === 'new' ? 'New automation' : 'Edit automation'}
        form={form}
        error={error}
        isPending={saveMutation.isPending}
        onChange={setForm}
        onCancel={() => {
          setEditor({ mode: 'list' })
          setError(null)
        }}
        onSave={save}
      />
    )
  }

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Automations"
        description="Rules that react to your mail at the server — tag, move, notify, or call a webhook. Rules run whenever a matching message arrives, even with every app closed."
        actions={
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-8 rounded-md"
            onClick={openNew}
          >
            <Plus size={15} strokeWidth={1.7} />
            New rule
          </Button>
        }
      />

      {error && <FeedbackBanner tone="error">{error}</FeedbackBanner>}

      {rules.length === 0 ? (
        <SettingsEmptyState
          icon={<Workflow size={34} strokeWidth={1.4} />}
          title="No automations yet"
          description="Create a rule to tag, move, notify, or hand a message to a webhook when it arrives."
          action={
            <Button type="button" size="sm" variant="outline" onClick={openNew}>
              <Plus size={15} strokeWidth={1.7} />
              New rule
            </Button>
          }
        />
      ) : (
        <SettingsList
          title={`${rules.length} rule${rules.length === 1 ? '' : 's'}`}
        >
          <ul className="divide-y divide-border-soft">
            {rules.map((rule) => (
              <RuleRow
                key={rule.id}
                rule={rule}
                onEdit={() => openEdit(rule)}
                onDelete={() => {
                  setError(null)
                  deleteMutation.mutate(rule.id)
                }}
                deleting={
                  deleteMutation.isPending &&
                  deleteMutation.variables === rule.id
                }
              />
            ))}
          </ul>
        </SettingsList>
      )}
    </SettingsPage>
  )
}

function RuleRow({
  rule,
  onEdit,
  onDelete,
  deleting,
}: {
  rule: Rule
  onEdit: () => void
  onDelete: () => void
  deleting: boolean
}) {
  const locked = isExec(rule)
  return (
    <li className="flex items-center gap-3 px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[13px] font-medium text-foreground">
            {rule.name}
          </span>
          {!rule.enabled && (
            <Badge variant="outline" className="h-5 px-1.5 text-[11px]">
              off
            </Badge>
          )}
          {locked && (
            <Badge
              variant="outline"
              className="h-5 px-1.5 text-[11px] text-muted-foreground"
              title="This rule runs an exec action and is defined in rules.toml — editable only on the server host."
            >
              config file
            </Badge>
          )}
        </div>
        <p
          className={
            isDestructiveActionKind(rule.action.kind)
              ? 'mt-0.5 truncate font-mono text-[11px] font-medium text-destructive'
              : 'mt-0.5 truncate font-mono text-[11px] text-muted-foreground'
          }
        >
          {actionSummary(rule.action)}
        </p>
      </div>
      {locked ? (
        <span className="text-[11px] text-muted-foreground">read-only</span>
      ) : (
        <div className="flex shrink-0 items-center gap-1">
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-7 w-7 rounded-md p-0"
            aria-label="Edit rule"
            onClick={onEdit}
          >
            <Pencil size={14} strokeWidth={1.6} />
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-7 w-7 rounded-md p-0 text-muted-foreground hover:text-destructive"
            aria-label="Delete rule"
            disabled={deleting}
            onClick={onDelete}
          >
            <Trash2 size={14} strokeWidth={1.6} />
          </Button>
        </div>
      )}
    </li>
  )
}

function RuleEditor({
  heading,
  form,
  error,
  isPending,
  onChange,
  onCancel,
  onSave,
}: {
  heading: string
  form: RuleForm
  error: string | null
  isPending: boolean
  onChange: (form: RuleForm) => void
  onCancel: () => void
  onSave: () => void
}) {
  // Account context for the WHEN builder's pickers (the `sourceId` account
  // picker especially) — the automations pane is account-agnostic, so mailboxes
  // stay unscoped (`accountId: ''` disables the mailbox query; a stored id
  // still round-trips as a raw entry).
  const accountsQuery = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: runtimeViews.accounts.list,
  })
  const conditionData = useMemo(
    () => ({
      accountId: '',
      mailboxes: null,
      accounts: (accountsQuery.data ?? []).map((account) => ({
        id: account.id,
        name: account.name,
      })),
    }),
    [accountsQuery.data],
  )

  // Mirror of the server's destroy guard (`validate_rule_action`): a destroy
  // rule must carry at least one condition — refuse the save locally with an
  // explanation instead of round-tripping to a 400.
  const unconditionalDestroy =
    form.action.kind === 'destroy' && !groupHasCondition(form.when.root)
  const canSave =
    form.name.trim().length > 0 && !isPending && !unconditionalDestroy

  return (
    <SettingsPage>
      <SettingsBackButton ariaLabel="Back to automations" onClick={onCancel}>
        Automations
      </SettingsBackButton>
      <SettingsPageHeader title={heading} />

      {error && <FeedbackBanner tone="error">{error}</FeedbackBanner>}

      <div className="space-y-6">
        <Field
          label="Name"
          value={form.name}
          placeholder="e.g. Tag receipts"
          onChange={(name) => onChange({ ...form, name })}
        />

        <section className="space-y-3">
          <h2 className="text-[12px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
            When a message matches
          </h2>
          <div className="rounded-lg border border-border-soft bg-bg-elev/45 p-4">
            <ConditionEditorContext.Provider value={conditionData}>
              <RuleGroupEditor
                group={form.when.root}
                onChange={(root) => onChange({ ...form, when: { root } })}
              />
            </ConditionEditorContext.Provider>
          </div>
          <p className="text-[12px] text-muted-foreground">
            Tip: scope to senders you trust (e.g.{' '}
            <code>from:you@yourdomain.com</code>) — an unscoped content rule
            that feeds an agent is an open injection surface.
          </p>
        </section>

        <section className="space-y-3">
          <h2 className="text-[12px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
            Then
          </h2>
          <RuleActionEditor
            action={form.action}
            onChange={(action) => onChange({ ...form, action })}
          />
        </section>

        {unconditionalDestroy && (
          <FeedbackBanner tone="error">
            A destroy rule needs at least one condition — an unconditional rule
            would permanently delete every incoming message. Add a condition
            above.
          </FeedbackBanner>
        )}

        <label className="flex items-center gap-2 text-[13px] text-muted-foreground">
          <Checkbox
            checked={form.enabled}
            onCheckedChange={(checked) =>
              onChange({ ...form, enabled: checked === true })
            }
          />
          Enabled
        </label>

        <label className="flex items-start gap-2 text-[13px] text-muted-foreground">
          <Checkbox
            checked={form.stopProcessing}
            onCheckedChange={(checked) =>
              onChange({ ...form, stopProcessing: checked === true })
            }
          />
          <span>
            Stop processing more rules
            <span className="block text-[12px] text-muted-foreground/80">
              When this rule matches a message, skip every later rule for it.
              Rules run in order: config-file rules first, then these, sorted by
              name.
            </span>
          </span>
        </label>

        <div className="flex items-center gap-2 pt-2">
          <Button
            type="button"
            size="sm"
            className="h-8 rounded-md"
            disabled={!canSave}
            onClick={onSave}
          >
            {isPending ? 'Saving…' : 'Save rule'}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-8 rounded-md"
            onClick={onCancel}
          >
            Cancel
          </Button>
        </div>
      </div>
    </SettingsPage>
  )
}

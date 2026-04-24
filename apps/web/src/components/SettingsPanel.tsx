/**
 * Settings panel for account and smart mailbox administration.
 *
 * Opens to a quiet category index; detail panes drill into focused settings.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#smart-mailbox-crud
 */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  ArrowLeft,
  FolderSearch,
  Mailbox,
  Palette,
  Settings as SettingsIcon,
} from 'lucide-react'
import { useEffect, useState } from 'react'
import {
  deleteAccount,
  deleteSmartMailbox,
  disableAccount,
  enableAccount,
  fetchAccount,
  fetchSettings,
  fetchSmartMailbox,
  fetchSmartMailboxes,
  patchSettings,
  resetDefaultSmartMailboxes,
  triggerSync,
  updateSmartMailbox,
} from '../api/client'
import type { AccountOverview, SmartMailboxSummary } from '../api/types'
import { AccountsPane } from './settings-panel/AccountsPane'
import { AppearancePane } from './settings-panel/AppearancePane'
import { GeneralPane } from './settings-panel/GeneralPane'
import { SmartMailboxesPane } from './settings-panel/SmartMailboxesPane'
import { brandAccents } from '../design/tokens'
import {
  applyAccountMutationResult,
  invalidateAccountReadModels,
  removeAccountOverview,
} from '../domainCache'
import { cn } from '../lib/utils'
import { queryKeys } from '../queryKeys'
import { Button } from './ui/button'
import type {
  EditorTarget,
  SmartMailboxEditorTarget,
} from './settings-panel/types'

type SettingsCategory = 'general' | 'appearance' | 'accounts' | 'mailboxes'

const SETTINGS_CATEGORIES = [
  {
    id: 'general',
    label: 'General',
    description: 'Default account and workspace-wide preferences.',
    icon: SettingsIcon,
    accent: brandAccents.blue,
  },
  {
    id: 'appearance',
    label: 'Appearance',
    description: 'Built-in themes, color mode, and density.',
    icon: Palette,
    accent: brandAccents.sage,
  },
  {
    id: 'accounts',
    label: 'Accounts',
    description: 'Connected mail sources, sync state, and credentials.',
    icon: Mailbox,
    accent: brandAccents.coral,
  },
  {
    id: 'mailboxes',
    label: 'Mailboxes & Rules',
    description: 'Smart mailboxes and rules that shape your views.',
    icon: FolderSearch,
    accent: brandAccents.violet,
  },
] as const

/** @spec docs/L1-api#account-crud-lifecycle */
interface SettingsPanelProps {
  accounts: AccountOverview[]
  activeAccountId: string | null
  initialAccountId?: string | null
  initialCategory?: SettingsCategory
  initialSmartMailboxId?: string | null
  onActiveAccountChange: (accountId: string | null) => void
  onClose?: () => void
  shell?: 'page' | 'overlay'
}

/**
 * Settings panel shell: category home plus drill-in detail views.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#smart-mailbox-crud
 */
export function SettingsPanel({
  accounts,
  activeAccountId,
  initialAccountId,
  initialCategory,
  initialSmartMailboxId,
  onActiveAccountChange,
  onClose,
  shell = 'page',
}: SettingsPanelProps) {
  const queryClient = useQueryClient()

  const [activeCategory, setActiveCategory] = useState<SettingsCategory>(
    initialCategory ?? 'general',
  )
  const [editorTarget, setEditorTarget] = useState<EditorTarget | null>(null)
  const [smartMailboxEditorTarget, setSmartMailboxEditorTarget] =
    useState<SmartMailboxEditorTarget | null>(null)
  const [smartMailboxActionPendingKey, setSmartMailboxActionPendingKey] =
    useState<string | null>(null)
  const [smartMailboxActionError, setSmartMailboxActionError] = useState<
    string | null
  >(null)
  const [accountCommandError, setAccountCommandError] = useState<string | null>(
    null,
  )

  useEffect(() => {
    if (initialCategory !== undefined) {
      setActiveCategory(initialCategory)
    }
    if (
      initialCategory === 'accounts' &&
      initialAccountId &&
      accounts.some((account) => account.id === initialAccountId)
    ) {
      setEditorTarget(initialAccountId)
      setSmartMailboxEditorTarget(null)
    }
    if (initialCategory === 'mailboxes' && initialSmartMailboxId) {
      setSmartMailboxEditorTarget(initialSmartMailboxId)
      setEditorTarget(null)
    }
  }, [accounts, initialAccountId, initialCategory, initialSmartMailboxId])

  const settingsQuery = useQuery({
    queryKey: queryKeys.settings,
    queryFn: fetchSettings,
  })
  const smartMailboxListQuery = useQuery({
    queryKey: queryKeys.smartMailboxes,
    queryFn: fetchSmartMailboxes,
  })

  const effectiveEditorTarget =
    editorTarget !== null &&
    editorTarget !== 'new' &&
    !accounts.some((account) => account.id === editorTarget)
      ? null
      : editorTarget
  const editorAccountId =
    effectiveEditorTarget === null || effectiveEditorTarget === 'new'
      ? null
      : effectiveEditorTarget
  const accountQuery = useQuery({
    queryKey: queryKeys.account(editorAccountId),
    queryFn: () => fetchAccount(editorAccountId!),
    enabled: editorAccountId !== null,
  })
  const editingAccount =
    accountQuery.data ??
    accounts.find((account) => account.id === editorAccountId) ??
    null

  const smartMailboxSummaries = smartMailboxListQuery.data ?? []
  const effectiveSmartMailboxTarget =
    smartMailboxEditorTarget !== null &&
    smartMailboxEditorTarget !== 'new' &&
    !smartMailboxSummaries.some(
      (mailbox) => mailbox.id === smartMailboxEditorTarget,
    )
      ? null
      : smartMailboxEditorTarget
  const editingSmartMailboxId =
    effectiveSmartMailboxTarget === null ||
    effectiveSmartMailboxTarget === 'new'
      ? null
      : effectiveSmartMailboxTarget
  const smartMailboxQuery = useQuery({
    queryKey: queryKeys.smartMailbox(editingSmartMailboxId),
    queryFn: () => fetchSmartMailbox(editingSmartMailboxId!),
    enabled: editingSmartMailboxId !== null,
  })
  const editingSmartMailbox =
    smartMailboxQuery.data ??
    smartMailboxSummaries.find(
      (mailbox) => mailbox.id === editingSmartMailboxId,
    ) ??
    null

  const invalidateSmartMailboxQueries = async (smartMailboxId?: string) => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.sidebar }),
      queryClient.invalidateQueries({ queryKey: queryKeys.messagesRoot }),
      queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes }),
      smartMailboxId
        ? queryClient.invalidateQueries({
            queryKey: queryKeys.smartMailbox(smartMailboxId),
          })
        : Promise.resolve(),
    ])
  }

  const runSmartMailboxAction = async (
    pendingKey: string,
    action: () => Promise<void>,
  ) => {
    if (smartMailboxActionPendingKey !== null) {
      return
    }
    setSmartMailboxActionError(null)
    setSmartMailboxActionPendingKey(pendingKey)
    try {
      await action()
    } catch (error) {
      setSmartMailboxActionError(
        error instanceof Error ? error.message : 'Smart mailbox action failed.',
      )
    } finally {
      setSmartMailboxActionPendingKey(null)
    }
  }

  const handleResetSmartMailboxes = () => {
    void runSmartMailboxAction('reset-defaults', async () => {
      await resetDefaultSmartMailboxes()
      await invalidateSmartMailboxQueries()
      setSmartMailboxEditorTarget(null)
    })
  }

  const handleReorderSmartMailbox = (
    mailbox: SmartMailboxSummary,
    position: number,
  ) => {
    void runSmartMailboxAction(`reorder:${mailbox.id}`, async () => {
      await updateSmartMailbox(mailbox.id, { position })
      await invalidateSmartMailboxQueries(mailbox.id)
    })
  }

  const defaultMutation = useMutation({
    mutationFn: (accountId: string | null) =>
      patchSettings({ defaultAccountId: accountId }),
    onSuccess: async () => {
      invalidateAccountReadModels(queryClient)
    },
  })

  const commandMutation = useMutation({
    mutationFn: async ({
      action,
      account,
    }: {
      action: 'enable' | 'disable' | 'delete' | 'sync'
      account: AccountOverview
    }) => {
      switch (action) {
        case 'enable':
          return enableAccount(account.id)
        case 'disable':
          return disableAccount(account.id)
        case 'delete':
          return deleteAccount(account.id)
        case 'sync':
          return triggerSync(account.id)
      }
    },
    onMutate: () => {
      setAccountCommandError(null)
    },
    onSuccess: async (_result, variables) => {
      if (variables.action === 'delete') {
        removeAccountOverview(queryClient, variables.account.id)
        invalidateAccountReadModels(queryClient)
      } else {
        invalidateAccountReadModels(queryClient, variables.account.id)
      }
      if (variables.action === 'delete') {
        const fallbackAccountId =
          accounts.find(
            (account) =>
              account.id !== variables.account.id &&
              account.enabled &&
              account.isDefault,
          )?.id ??
          accounts.find(
            (account) => account.id !== variables.account.id && account.enabled,
          )?.id ??
          null
        if (activeAccountId === variables.account.id) {
          onActiveAccountChange(fallbackAccountId)
        }
        if (effectiveEditorTarget === variables.account.id) {
          setEditorTarget(null)
        }
      }
    },
    onError: (error: Error) => {
      setAccountCommandError(error.message)
    },
  })

  const editorKey =
    effectiveEditorTarget === null
      ? 'account:none'
      : effectiveEditorTarget === 'new'
        ? 'account:new'
        : `account:${effectiveEditorTarget}:${editingAccount?.updatedAt ?? 'pending'}`
  const smartMailboxEditorKey =
    effectiveSmartMailboxTarget === null
      ? 'mailbox:none'
      : effectiveSmartMailboxTarget === 'new'
        ? 'mailbox:new'
        : `mailbox:${effectiveSmartMailboxTarget}:${editingSmartMailbox?.updatedAt ?? 'pending'}`
  function handleSelectCategory(category: SettingsCategory) {
    setActiveCategory(category)
    setEditorTarget(null)
    setSmartMailboxEditorTarget(null)
  }

  return (
    <section
      className={cn(
        'flex h-full min-h-0 w-full flex-col overflow-hidden text-card-foreground md:flex-row',
        shell === 'overlay' ? 'bg-background' : 'bg-card',
      )}
    >
      <SettingsRail
        activeCategory={activeCategory}
        accountCount={accounts.length}
        smartMailboxCount={smartMailboxSummaries.length}
        onClose={onClose}
        onSelect={handleSelectCategory}
      />

      <main className="min-w-0 flex-1 bg-background">
        <div className="h-full min-h-0 overflow-hidden bg-transparent">
          {activeCategory === 'general' && (
            <div className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
              <GeneralPane
                accounts={accounts}
                defaultAccountId={settingsQuery.data?.defaultAccountId}
                onDefaultAccountChange={(accountId) =>
                  defaultMutation.mutate(accountId)
                }
                isPending={defaultMutation.isPending}
              />
            </div>
          )}

          {activeCategory === 'appearance' && (
            <div className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
              <AppearancePane />
            </div>
          )}

          {activeCategory === 'accounts' && (
            <AccountsPane
              accounts={accounts}
              selectedAccountId={effectiveEditorTarget}
              editingAccount={editingAccount}
              editorKey={editorKey}
              onSelectAccount={(accountId) => setEditorTarget(accountId)}
              onBackToAccounts={() => setEditorTarget(null)}
              onCreateAccount={() => setEditorTarget('new')}
              onCommand={(action, account) =>
                commandMutation.mutate({ action, account })
              }
              onSaved={async (account) => {
                applyAccountMutationResult(queryClient, account)
                setEditorTarget(account.id)
              }}
              onVerified={async () => {
                invalidateAccountReadModels(
                  queryClient,
                  editorAccountId ?? undefined,
                )
              }}
              commandMutation={commandMutation}
              commandError={accountCommandError}
            />
          )}

          {activeCategory === 'mailboxes' && (
            <SmartMailboxesPane
              smartMailboxes={smartMailboxSummaries}
              selectedMailboxId={effectiveSmartMailboxTarget}
              editingSmartMailbox={editingSmartMailbox}
              editorKey={smartMailboxEditorKey}
              actionPendingKey={smartMailboxActionPendingKey}
              actionError={smartMailboxActionError}
              onSelectMailbox={(mailboxId) =>
                setSmartMailboxEditorTarget(mailboxId)
              }
              onBackToMailboxes={() => setSmartMailboxEditorTarget(null)}
              onCreateMailbox={() => setSmartMailboxEditorTarget('new')}
              onResetDefaults={handleResetSmartMailboxes}
              onReorderMailbox={handleReorderSmartMailbox}
              onSaved={async (mailbox) => {
                await invalidateSmartMailboxQueries(mailbox.id)
                setSmartMailboxEditorTarget(mailbox.id)
              }}
              onDeleted={async (mailboxId) => {
                await deleteSmartMailbox(mailboxId)
                await invalidateSmartMailboxQueries()
                setSmartMailboxEditorTarget(null)
              }}
            />
          )}
        </div>
      </main>
    </section>
  )
}

function SettingsRail({
  activeCategory,
  accountCount,
  smartMailboxCount,
  onClose,
  onSelect,
}: {
  activeCategory: SettingsCategory
  accountCount: number
  smartMailboxCount: number
  onClose?: () => void
  onSelect: (category: SettingsCategory) => void
}) {
  return (
    <aside className="flex max-h-[190px] min-h-0 w-full shrink-0 flex-col border-b border-sidebar-border bg-sidebar text-sidebar-foreground md:h-full md:max-h-none md:w-[210px] md:border-b-0 md:border-r">
      <div className="flex h-12 shrink-0 items-center px-4 md:h-14">
        {onClose && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={onClose}
            className="h-7 rounded-[5px] px-2 text-[13px] font-medium text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
          >
            <ArrowLeft size={15} strokeWidth={1.6} />
            Back to app
          </Button>
        )}
      </div>

      <nav className="ph-scroll min-h-0 flex-1 overflow-y-auto px-3 py-2">
        <p className="px-3 pb-2 font-mono text-[11px] font-semibold uppercase tracking-[0.7px] text-[var(--sidebar-section-label)]">
          Preferences
        </p>
        <div className="space-y-1">
          {SETTINGS_CATEGORIES.map((category) => {
            const Icon = category.icon
            const isActive = category.id === activeCategory
            const count =
              category.id === 'accounts'
                ? accountCount
                : category.id === 'mailboxes'
                  ? smartMailboxCount
                  : null

            return (
              <button
                key={category.id}
                type="button"
                onClick={() => onSelect(category.id)}
                style={{
                  backgroundColor: isActive
                    ? `color-mix(in oklab, ${category.accent} 16%, transparent)`
                    : undefined,
                }}
                className={cn(
                  'group flex h-[28px] w-full items-center gap-2 rounded-[5px] px-2 text-left text-[13px] font-medium transition-colors',
                  isActive
                    ? 'text-sidebar-accent-foreground'
                    : 'text-sidebar-foreground/68 hover:bg-sidebar-accent/70 hover:text-sidebar-accent-foreground',
                )}
              >
                <Icon
                  size={17}
                  strokeWidth={1.6}
                  className="shrink-0"
                  style={{ color: category.accent }}
                />
                <span className="min-w-0 flex-1 truncate font-medium">
                  {category.label}
                </span>
                {count !== null && (
                  <span className="font-mono text-[11px] text-sidebar-foreground/50">
                    {count}
                  </span>
                )}
              </button>
            )
          })}
        </div>
      </nav>

      <div className="hidden shrink-0 px-6 py-5 font-mono text-[11px] text-sidebar-foreground/45 md:block">
        v1.0.0 · JMAP 0.3
      </div>
    </aside>
  )
}

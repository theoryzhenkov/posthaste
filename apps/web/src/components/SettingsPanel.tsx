/**
 * Settings panel for account and smart mailbox administration.
 *
 * Opens to a quiet category index; detail panes drill into focused settings.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#smart-mailbox-crud
 */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  deleteSmartMailbox,
  fetchAccount,
  fetchSettings,
  fetchSmartMailbox,
  resetDefaultSmartMailboxes,
  updateSmartMailbox,
  patchSettings,
} from '../api/client'
import type {
  AccountOverview,
  AppSettings,
  SmartMailboxSummary,
} from '../api/types'
import {
  applyAccountMutationResult,
  invalidateAccountReadModels,
  invalidateSmartMailboxMutationReadModels,
} from '../domainCache'
import { cn } from '../lib/utils'
import { queryKeys } from '../queryKeys'
import { fetchRuntimeSmartMailboxes } from '../runtime/adapter'
import {
  accountSettingsSurface,
  newAccountSettingsSurface,
  newSmartMailboxSettingsSurface,
  settingsCategorySurface,
  smartMailboxSettingsSurface,
  sourceMailboxSettingsSurface,
  type SettingsSurfaceDescriptor,
} from '../surfaces'
import { SettingsPanelContent } from './settings-panel/SettingsPanelContent'
import {
  SettingsRail,
  type SettingsCategory,
} from './settings-panel/SettingsRail'
import {
  accountEditorKey,
  smartMailboxEditorKey,
} from './settings-panel/editorKeys'
import { settingsPanelReadiness } from './settings-panel/readiness'
import {
  removeLinkedSmartMailboxAutomation,
  rewriteLinkedSmartMailboxAutomation,
} from './settings-panel/linkedAutomation'
import {
  accountEditorTargetFromSettingsTarget,
  mailboxEditorTargetFromSettingsTarget,
} from './settings-panel/targetRouting'
import { useAccountCommandMutation } from './settings-panel/useAccountCommandMutation'

/** @spec docs/L1-api#account-crud-lifecycle */
interface SettingsPanelProps {
  accounts: AccountOverview[]
  activeAccountId: string | null
  surface: SettingsSurfaceDescriptor
  onActiveAccountChange: (accountId: string | null) => void
  onNavigate: (surface: SettingsSurfaceDescriptor) => void
  onClose?: () => void
  showBackToApp?: boolean
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
  surface,
  onActiveAccountChange,
  onNavigate,
  onClose,
  showBackToApp = true,
  shell = 'page',
}: SettingsPanelProps) {
  const queryClient = useQueryClient()
  const activeCategory = surface.params.category ?? 'general'
  const settingsTarget = surface.params.target ?? null
  const editorTarget =
    activeCategory === 'accounts'
      ? accountEditorTargetFromSettingsTarget(settingsTarget)
      : null
  const mailboxEditorTarget =
    activeCategory === 'mailboxes'
      ? mailboxEditorTargetFromSettingsTarget(settingsTarget)
      : null
  const [smartMailboxActionPendingKey, setSmartMailboxActionPendingKey] =
    useState<string | null>(null)
  const [smartMailboxActionError, setSmartMailboxActionError] = useState<
    string | null
  >(null)
  const [accountCommandError, setAccountCommandError] = useState<string | null>(
    null,
  )

  const settingsQuery = useQuery({
    queryKey: queryKeys.settings,
    queryFn: fetchSettings,
  })
  const smartMailboxListQuery = useQuery({
    queryKey: queryKeys.smartMailboxes,
    queryFn: fetchRuntimeSmartMailboxes,
  })

  const effectiveEditorTarget = editorTarget
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
  const smartMailboxEditorTarget =
    mailboxEditorTarget?.kind === 'smart' ? mailboxEditorTarget.id : null
  const effectiveSmartMailboxTarget = smartMailboxEditorTarget
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

  const currentSettings = () =>
    queryClient.getQueryData<AppSettings>(queryKeys.settings) ??
    settingsQuery.data ??
    null
  const invalidateSmartMailboxQueries = (smartMailboxId?: string) =>
    invalidateSmartMailboxMutationReadModels(queryClient, smartMailboxId)

  const runSmartMailboxAction = async (
    pendingKey: string,
    action: () => Promise<void>,
  ) => {
    if (smartMailboxActionPendingKey !== null) return
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

  const defaultMutation = useMutation({
    mutationFn: (accountId: string | null) =>
      patchSettings({ defaultAccountId: accountId }),
    onSuccess: async () => {
      invalidateAccountReadModels(queryClient)
    },
  })
  const commandMutation = useAccountCommandMutation({
    accounts,
    activeAccountId,
    effectiveEditorTarget,
    onActiveAccountChange,
    onNavigate,
    queryClient,
    setAccountCommandError,
  })

  const editorKey = accountEditorKey(effectiveEditorTarget)
  const mailboxEditorKey = smartMailboxEditorKey({
    target: effectiveSmartMailboxTarget,
    editingSmartMailbox,
  })
  const settingsReadinessState = settingsPanelReadiness({
    accountQuery,
    editingSmartMailboxId,
    editorAccountId,
    settingsQuery,
    smartMailboxListQuery,
    smartMailboxQuery,
  })

  function handleSelectCategory(category: SettingsCategory) {
    onNavigate(settingsCategorySurface(category))
  }

  return (
    <section
      data-posthaste-state={settingsReadinessState}
      className={cn(
        'flex h-full min-h-0 w-full flex-col overflow-hidden text-card-foreground md:flex-row',
        shell === 'overlay' ? 'bg-background' : 'bg-card',
      )}
    >
      <SettingsRail
        activeCategory={activeCategory}
        accountCount={accounts.length}
        smartMailboxCount={smartMailboxSummaries.length}
        onClose={showBackToApp ? onClose : undefined}
        onSelect={handleSelectCategory}
      />
      <SettingsPanelContent
        accounts={accounts}
        accountCommandError={accountCommandError}
        activeCategory={activeCategory}
        commandMutation={commandMutation}
        defaultAccountId={settingsQuery.data?.defaultAccountId}
        defaultMutation={defaultMutation}
        editingAccount={editingAccount}
        editingSmartMailbox={editingSmartMailbox}
        editorKey={editorKey}
        effectiveEditorTarget={effectiveEditorTarget}
        effectiveSmartMailboxTarget={effectiveSmartMailboxTarget}
        mailboxEditorTarget={mailboxEditorTarget}
        settings={settingsQuery.data ?? null}
        smartMailboxActionError={smartMailboxActionError}
        smartMailboxActionPendingKey={smartMailboxActionPendingKey}
        smartMailboxEditorKey={mailboxEditorKey}
        smartMailboxSummaries={smartMailboxSummaries}
        onAutomationSettingsSaved={async (settings) => {
          queryClient.setQueryData(queryKeys.settings, settings)
          invalidateAccountReadModels(queryClient)
        }}
        onBackToAccounts={() => onNavigate(settingsCategorySurface('accounts'))}
        onBackToMailboxes={() =>
          onNavigate(settingsCategorySurface('mailboxes'))
        }
        onCreateAccount={() => onNavigate(newAccountSettingsSurface())}
        onCreateMailbox={() => onNavigate(newSmartMailboxSettingsSurface())}
        onDefaultAccountChange={(accountId) =>
          defaultMutation.mutate(accountId)
        }
        onDeletedSmartMailbox={async (mailboxId) => {
          await removeLinkedSmartMailboxAutomation({
            queryClient,
            settings: currentSettings(),
            smartMailboxId: mailboxId,
          })
          await deleteSmartMailbox(mailboxId)
          await invalidateSmartMailboxQueries()
          onNavigate(settingsCategorySurface('mailboxes'))
        }}
        onReorderSmartMailbox={(mailbox: SmartMailboxSummary, position) => {
          void runSmartMailboxAction(`reorder:${mailbox.id}`, async () => {
            await updateSmartMailbox(mailbox.id, { position })
            await invalidateSmartMailboxQueries(mailbox.id)
          })
        }}
        onResetSmartMailboxes={() => {
          void runSmartMailboxAction('reset-defaults', async () => {
            await resetDefaultSmartMailboxes()
            await invalidateSmartMailboxQueries()
            onNavigate(settingsCategorySurface('mailboxes'))
          })
        }}
        onSavedAccount={async (account) => {
          applyAccountMutationResult(queryClient, account)
          onNavigate(accountSettingsSurface(account.id))
        }}
        onSavedSmartMailbox={async (mailbox) => {
          await invalidateSmartMailboxQueries(mailbox.id)
          await rewriteLinkedSmartMailboxAutomation({
            queryClient,
            settings: currentSettings(),
            smartMailbox: mailbox,
          })
          onNavigate(smartMailboxSettingsSurface(mailbox.id))
        }}
        onSelectAccount={(accountId) =>
          onNavigate(accountSettingsSurface(accountId))
        }
        onSelectSmartMailbox={(mailboxId) =>
          onNavigate(smartMailboxSettingsSurface(mailboxId))
        }
        onSelectSourceMailbox={(accountId, mailboxId) =>
          onNavigate(sourceMailboxSettingsSurface(accountId, mailboxId))
        }
        onVerifiedAccount={async () => {
          invalidateAccountReadModels(queryClient, editorAccountId ?? undefined)
        }}
      />
    </section>
  )
}

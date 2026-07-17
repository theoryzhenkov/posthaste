/**
 * Settings panel for account and smart mailbox administration.
 *
 * Opens to a quiet category index; detail panes drill into focused settings.
 * Every read is a query family (`appSettings`, `smartMailboxes`,
 * `accountSettings`); every write is a command whose acceptance rides the
 * global invalidation cycle.
 */
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'
import type { AppSettings, CachePolicy, Notifications } from '@/gen'
import { cn } from '../lib/utils'
import { useMailClient } from '@/data/context'
import { useAccountSettings, useAppSettings, useSmartMailboxes, ensureAppSettings } from '@/data/queries'
import { runCommand, useCommands } from '@/data/commands'
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
import type { AccountRow } from '@/gen'
import {
  markSurfaceBootstrap,
  markSurfaceBootstrapOnce,
} from '../surfaceBootstrapLog'

interface SettingsPanelProps {
  accounts: AccountRow[]
  activeAccountId: string | null
  surface: SettingsSurfaceDescriptor
  onActiveAccountChange: (accountId: string | null) => void
  onNavigate: (surface: SettingsSurfaceDescriptor) => void
  onClose?: () => void
  showBackToApp?: boolean
  shell?: 'page' | 'overlay'
}

/** Settings panel shell: category home plus drill-in detail views. */
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
  markSurfaceBootstrapOnce('settings_panel_render')
  useEffect(() => {
    markSurfaceBootstrap('settings_panel_mounted')
  }, [])
  const client = useMailClient()
  const queryClient = useQueryClient()
  const commands = useCommands()
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

  const settingsQuery = useAppSettings()
  const settings = settingsQuery.data?.settings ?? null
  const smartMailboxListQuery = useSmartMailboxes()

  const effectiveEditorTarget = editorTarget
  const editorAccountId =
    effectiveEditorTarget === null || effectiveEditorTarget === 'new'
      ? null
      : effectiveEditorTarget
  const accountQuery = useAccountSettings(editorAccountId ?? '', {
    enabled: editorAccountId !== null,
  })
  const editingAccount = accountQuery.data ?? null

  const smartMailboxSummaries = smartMailboxListQuery.data?.rows ?? []
  const smartMailboxEditorTarget =
    mailboxEditorTarget?.kind === 'smart' ? mailboxEditorTarget.id : null
  const effectiveSmartMailboxTarget = smartMailboxEditorTarget
  const editingSmartMailboxId =
    effectiveSmartMailboxTarget === null ||
    effectiveSmartMailboxTarget === 'new'
      ? null
      : effectiveSmartMailboxTarget
  // The `smartMailboxes` rows carry the full configuration (rule included),
  // so the editor needs no second read.
  const editingSmartMailbox =
    smartMailboxSummaries.find(
      (mailbox) => mailbox.id === editingSmartMailboxId,
    ) ?? null

  const currentSettings = () => settingsQuery.data?.settings ?? null

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

  // Settings writes are read-modify-write: fold the change into the freshest
  // document and post one `updateSettings` command.
  const patchSettings = async (
    transform: (settings: AppSettings) => AppSettings,
  ) => {
    const current = await ensureAppSettings(client, queryClient)
    return runCommand(client, queryClient, {
      updateSettings: { settings: transform(current), forceBackfill: false },
    })
  }

  const defaultMutation = useMutation({
    mutationFn: (accountId: string | null) =>
      patchSettings((current) => ({ ...current, defaultAccountId: accountId })),
  })
  const cacheMutation = useMutation({
    mutationFn: (cachePolicy: CachePolicy) =>
      patchSettings((current) => ({ ...current, cachePolicy })),
  })
  const notificationsMutation = useMutation({
    mutationFn: (notifications: Notifications) =>
      patchSettings((current) => ({ ...current, notifications })),
  })
  const composeSettingsMutation = useMutation({
    mutationFn: (undoSendDelaySeconds: number) =>
      patchSettings((current) => ({
        ...current,
        compose: { ...current.compose, undoSendDelaySeconds },
      })),
  })
  const commandMutation = useAccountCommandMutation({
    accounts,
    activeAccountId,
    effectiveEditorTarget,
    onActiveAccountChange,
    onNavigate,
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
    // The list rows carry the editing mailbox; no separate detail query runs.
    smartMailboxQuery: smartMailboxListQuery,
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
        cacheMutation={cacheMutation}
        notificationsMutation={notificationsMutation}
        commandMutation={commandMutation}
        defaultAccountId={settings?.defaultAccountId}
        defaultMutation={defaultMutation}
        editingAccount={editingAccount}
        editingSmartMailbox={editingSmartMailbox}
        editorKey={editorKey}
        effectiveEditorTarget={effectiveEditorTarget}
        effectiveSmartMailboxTarget={effectiveSmartMailboxTarget}
        mailboxEditorTarget={mailboxEditorTarget}
        settings={settings}
        smartMailboxActionError={smartMailboxActionError}
        smartMailboxActionPendingKey={smartMailboxActionPendingKey}
        smartMailboxEditorKey={mailboxEditorKey}
        smartMailboxSummaries={smartMailboxSummaries}
        onAutomationSettingsSaved={async () => {}}
        onBackToAccounts={() => onNavigate(settingsCategorySurface('accounts'))}
        onBackToMailboxes={() =>
          onNavigate(settingsCategorySurface('mailboxes'))
        }
        onCreateAccount={() => onNavigate(newAccountSettingsSurface())}
        onCreateMailbox={() => onNavigate(newSmartMailboxSettingsSurface())}
        onDefaultAccountChange={(accountId) =>
          defaultMutation.mutate(accountId)
        }
        onUndoSendDelayChange={(seconds) =>
          composeSettingsMutation.mutate(seconds)
        }
        isComposePending={composeSettingsMutation.isPending}
        onDeletedSmartMailbox={async (mailboxId) => {
          await removeLinkedSmartMailboxAutomation({
            client,
            queryClient,
            settings: currentSettings(),
            smartMailboxId: mailboxId,
          })
          await commands.run({
            deleteSmartMailbox: { smartMailboxId: mailboxId },
          })
          onNavigate(settingsCategorySurface('mailboxes'))
        }}
        onResetSmartMailboxes={() => {
          void runSmartMailboxAction('reset-defaults', async () => {
            await commands.run({ resetSmartMailboxes: {} })
            onNavigate(settingsCategorySurface('mailboxes'))
          })
        }}
        onSavedAccount={async (accountId) => {
          onNavigate(accountSettingsSurface(accountId))
        }}
        onSavedSmartMailbox={async (mailbox) => {
          await rewriteLinkedSmartMailboxAutomation({
            client,
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
          await queryClient.invalidateQueries()
        }}
      />
    </section>
  )
}

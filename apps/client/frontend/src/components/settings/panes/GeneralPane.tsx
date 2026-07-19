/**
 * General preferences: default account selector and at-a-glance overview.
 *
 */
import type { AccountRow } from '@/gen'
import { isTauriRuntime } from '@/lib/platform/runtime'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../ui/form/select'
import { SettingsPage, SettingsPageHeader, SettingsSection } from '../panel/shared'
import { UpdatesSection } from './UpdatesSection'

/** Undo-send delay choices (seconds); 0 disables the hold. */
const UNDO_SEND_DELAY_OPTIONS = [0, 5, 10, 20, 30] as const

export function GeneralPane({
  accounts,
  defaultAccountId,
  onDefaultAccountChange,
  isPending,
  undoSendDelaySeconds,
  onUndoSendDelayChange,
  isComposePending,
}: {
  accounts: AccountRow[]
  defaultAccountId: string | null | undefined
  onDefaultAccountChange: (accountId: string | null) => void
  isPending: boolean
  undoSendDelaySeconds: number
  onUndoSendDelayChange: (seconds: number) => void
  isComposePending: boolean
}) {
  return (
    <SettingsPage>
      <SettingsPageHeader
        title="General"
        description="Choose the default account Posthaste should use when no source is selected."
      />

      <SettingsSection title="Defaults">
        <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
          <div className="min-w-0">
            <p className="text-[13px] font-medium text-foreground">
              Default account
            </p>
            <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
              Used when a compose flow does not already have account context.
            </p>
          </div>
          <Select
            value={defaultAccountId ?? '__none__'}
            onValueChange={(value) =>
              onDefaultAccountChange(value === '__none__' ? null : value)
            }
            disabled={isPending}
          >
            <SelectTrigger className="h-8 w-full rounded-md border-border bg-background text-[13px] shadow-none">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__none__">No default</SelectItem>
              {accounts.map((account) => (
                <SelectItem key={account.id} value={account.id}>
                  {account.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </SettingsSection>

      <SettingsSection title="Sending">
        <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
          <div className="min-w-0">
            <p className="text-[13px] font-medium text-foreground">Undo send</p>
            <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
              Hold outgoing mail briefly after you hit Send so you can cancel
              it. The message leaves only after the delay.
            </p>
          </div>
          <Select
            value={String(undoSendDelaySeconds)}
            onValueChange={(value) => {
              const seconds = Number(value)
              if (Number.isFinite(seconds)) {
                onUndoSendDelayChange(seconds)
              }
            }}
            disabled={isComposePending}
          >
            <SelectTrigger className="h-8 w-full rounded-md border-border bg-background text-[13px] shadow-none">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {UNDO_SEND_DELAY_OPTIONS.map((seconds) => (
                <SelectItem key={seconds} value={String(seconds)}>
                  {seconds === 0
                    ? 'Off (send immediately)'
                    : `${seconds} seconds`}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </SettingsSection>

      {isTauriRuntime() && <UpdatesSection />}
    </SettingsPage>
  )
}

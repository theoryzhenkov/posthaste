import { X } from 'lucide-react'

import { ActionBar } from '@/components/ActionBar'
import {
  isAggregateMessageView,
  viewModeKey,
} from '@/components/message-list/model'
import type { EmailActions } from '@/hooks/useEmailActions'

import { MailOverlays } from './MailOverlays'
import { MailPanels } from './MailPanels'
import type { MailClientViewProps } from './MailClientView.types'

export function MailClientView(props: MailClientViewProps) {
  return (
    <div
      className="flex h-full flex-col overflow-hidden"
      data-posthaste-state={props.appReadinessState}
    >
      <ActionBar
        isDarkMode={props.isDarkMode}
        isSettingsOpen={props.isSettingsSurfaceOpen}
        searchQuery={props.searchQuery}
        viewModeKey={
          props.isSettingsSurfaceOpen ||
          (props.effectiveView === null && !props.searchQuery.trim())
            ? null
            : viewModeKey(props.effectiveView, props.searchQuery)
        }
        showSourceMailboxDefault={isAggregateMessageView(
          props.effectiveView,
          props.searchQuery,
        )}
        onClearSearch={props.onClearSearch}
        onCompose={props.onCompose}
        onOpenCommandPalette={props.onOpenCommandPalette}
        onShowShortcuts={props.onShowShortcuts}
        onToggleSettings={props.onToggleSettings}
        onToggleTheme={props.onToggleTheme}
      />
      {props.actions.errorMessage && (
        <ActionErrorBanner actions={props.actions} />
      )}
      <MailPanels {...props} />
      <MailOverlays {...props} />
    </div>
  )
}

function ActionErrorBanner({ actions }: { actions: EmailActions }) {
  return (
    <div className="flex items-center gap-3 border-b border-destructive/20 bg-destructive/5 px-3 py-2 text-sm text-destructive">
      <span className="min-w-0 flex-1">{actions.errorMessage}</span>
      <button
        type="button"
        aria-label="Dismiss error"
        className="ph-focus-ring flex size-6 shrink-0 items-center justify-center rounded-md text-destructive/70 transition-colors hover:bg-destructive/10 hover:text-destructive"
        onClick={actions.clearError}
      >
        <X size={14} strokeWidth={1.8} />
      </button>
    </div>
  )
}

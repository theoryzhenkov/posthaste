import type { ComponentProps } from 'react'

import type { TagSummary } from '@/data/transport/api'
import type { MessageSummary } from '@/gen'
import type { ResizablePanelGroup } from '@/components/ui/display/resizable'
import type { ComposeIntent } from '@/domain/composeIntent'
import type { EmailActions } from '@/data/hooks/useEmailActions'
import type { useMailClientHandlers } from '@/app/mail/useMailClientHandlers'
import type { MailSelection } from '@/data/models/selection'
import type { PreparedServerSearchQuery } from '@/domain/searchQuery'
import type { SettingsSurfaceCategory, SurfaceDescriptor } from '@/surfaces'
import type { SidebarSelection } from '@/components/sidebar/Sidebar'

type PanelGroupProps = ComponentProps<typeof ResizablePanelGroup>

export type LayoutValue = PanelGroupProps['defaultLayout']
export type LayoutHandler = NonNullable<PanelGroupProps['onLayoutChanged']>

export interface MailClientViewProps {
  actions: EmailActions
  /** The app/handler bundle, bound as the palette's `ActionServices.app`. */
  handlers: ReturnType<typeof useMailClientHandlers>
  appReadinessState: string
  closeCompose: () => void
  /** Non-null when the palette should open straight into a parameterized
   *  action's pick-step (keyboard chord → picker). */
  commandPaletteSeedActionId: string | null
  composeIntent: ComposeIntent | null
  effectiveSurface: SurfaceDescriptor | null
  effectiveView: SidebarSelection | null
  invalidSurfaceRoute: string | null
  isCommandPaletteOpen: boolean
  isDarkMode: boolean
  isMessageDetailOpen: boolean
  isSettingsSurfaceOpen: boolean
  isTagEditorOpen: boolean
  messageDefaultLayout: LayoutValue
  preparedSearchQuery: PreparedServerSearchQuery
  searchQuery: string
  selectedMessage: MailSelection | null
  selectedMessageData: MessageSummary | undefined
  shellDefaultLayout: LayoutValue
  showShortcuts: boolean
  tags: TagSummary[]
  viewRole: string | null
  onAddTag: (tag: string) => void
  onApplySearch: (query: string) => void
  onClearSearch: () => void
  onClearSelectedMessage: () => void
  onCloseCommandPalette: () => void
  onCompose: () => void
  onEditDraft: () => void
  onForward: () => void
  onReplyAll: () => void
  onMessageLayoutChanged: LayoutHandler
  onOpenCommandPalette: () => void
  onOpenFocusedMessage: () => void
  onOpenSettings: (
    category?: SettingsSurfaceCategory,
    options?: { accountId?: string | null; smartMailboxId?: string | null },
  ) => void
  onOpenTagEditor: () => void
  onPlaceholderAction: (label: string) => void
  onRejectSearchPreview: () => void
  onRemoveTag: (tag: string) => void
  onReply: () => void
  /** Open the composer prefilled from a `mailto:` unsubscribe URI (List-
   *  Unsubscribe mailto path); the source is the selected message's. */
  onUnsubscribeMailto: (mailtoUri: string) => void
  onSearch: (query: string, append?: boolean) => void
  onSelectMessage: (message: MessageSummary) => void
  onSelectMessageRef: (selection: MailSelection) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onSetTagEditorOpen: (open: boolean) => void
  onShellLayoutChanged: LayoutHandler
  onShowShortcuts: () => void
  onSyncSource: (sourceId: string) => void
  onToggleShortcuts: () => void
  onToggleSettings: () => void
  onToggleTheme: () => void
}

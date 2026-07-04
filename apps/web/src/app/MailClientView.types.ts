import type { ComponentProps } from 'react'

import type { MessageDetail, MessageSummary, TagSummary } from '@/api/types'
import type { ResizablePanelGroup } from '@/components/ui/resizable'
import type { ComposeIntent } from '@/composeIntent'
import type { EmailActions } from '@/hooks/useEmailActions'
import type { MailSelection } from '@/mailState'
import type { PreparedServerSearchQuery } from '@/searchQuery'
import type { SettingsSurfaceCategory, SurfaceDescriptor } from '@/surfaces'
import type { SidebarSelection } from '@/components/Sidebar'

type PanelGroupProps = ComponentProps<typeof ResizablePanelGroup>

export type LayoutValue = PanelGroupProps['defaultLayout']
export type LayoutHandler = NonNullable<PanelGroupProps['onLayoutChanged']>

export interface MailClientViewProps {
  actions: EmailActions
  appReadinessState: string
  closeCompose: () => void
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
  selectedMessageData: MessageDetail | undefined
  shellDefaultLayout: LayoutValue
  showShortcuts: boolean
  tags: TagSummary[]
  viewRole: string | null
  onAddTag: (tag: string) => void
  onApplySearch: (query: string) => void
  onArchive: () => void
  onSnooze: (until: number) => void
  onClearSearch: () => void
  onClearSelectedMessage: () => void
  onCloseCommandPalette: () => void
  onCompose: () => void
  onDiscardDraft: () => void
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
  onToggleFlag: () => void
  onToggleShortcuts: () => void
  onToggleSettings: () => void
  onToggleTheme: () => void
  onTrash: () => void
}

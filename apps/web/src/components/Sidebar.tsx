/**
 * Left-pane sidebar with smart mailbox and source mailbox navigation.
 *
 * Renders normalized account, mailbox, smart mailbox, and tag read models.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
import { useMemo, useState } from 'react'

import { useMailboxNavigationReadModels } from '../mailboxNavigationReadModels'
import { sortSmartMailboxes } from './sidebar/model'
import {
  AccountsSection,
  SidebarError,
  SidebarLoading,
  SmartMailboxSection,
  TagsSection,
} from './sidebar/SidebarContent'

/**
 * Discriminated union representing the current sidebar selection.
 * @spec docs/L0-ui#navigation-model
 */
export type SidebarSelection =
  | { kind: 'smart-mailbox'; id: string; name: string }
  | {
      kind: 'source-mailbox'
      sourceId: string
      mailboxId: string
      name: string
    }

/** @spec docs/L1-ui#component-hierarchy */
interface SidebarProps {
  selectedView: SidebarSelection | null
  onOpenAccountSettings: (sourceId: string) => void
  onOpenSmartMailboxSettings: (smartMailboxId: string) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onSelectTag: (tag: string) => void
  onSyncSource: (sourceId: string) => void
}

/**
 * Sidebar navigation with smart mailbox and source mailbox sections.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
export function Sidebar({
  selectedView,
  onOpenAccountSettings,
  onOpenSmartMailboxSettings,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
  onSelectTag,
  onSyncSource,
}: SidebarProps) {
  const { error, isLoading, refetchBootstrap, smartMailboxes, sources, tags } =
    useMailboxNavigationReadModels()

  const [mailboxesCollapsed, setMailboxesCollapsed] = useState(false)
  const [sourcesCollapsed, setSourcesCollapsed] = useState(false)
  const sortedSmartMailboxes = useMemo(
    () => sortSmartMailboxes(smartMailboxes),
    [smartMailboxes],
  )

  return (
    <aside className="flex h-full min-h-0 min-w-0 flex-col bg-sidebar text-sidebar-foreground">
      <nav className="ph-scroll min-h-0 flex-1 overflow-y-auto px-2 pb-4 pt-3">
        {isLoading && <SidebarLoading />}
        {error && <SidebarError onRetry={() => void refetchBootstrap()} />}
        {!isLoading && !error && (
          <>
            <SmartMailboxSection
              collapsed={mailboxesCollapsed}
              mailboxes={sortedSmartMailboxes}
              selectedView={selectedView}
              onOpenSmartMailboxSettings={onOpenSmartMailboxSettings}
              onSelectSmartMailbox={onSelectSmartMailbox}
              onToggle={() => setMailboxesCollapsed((prev) => !prev)}
            />
            <TagsSection tags={tags} onSelectTag={onSelectTag} />
            <AccountsSection
              collapsed={sourcesCollapsed}
              selectedView={selectedView}
              sources={sources}
              onOpenAccountSettings={onOpenAccountSettings}
              onSelectSourceMailbox={onSelectSourceMailbox}
              onSyncSource={onSyncSource}
              onToggle={() => setSourcesCollapsed((prev) => !prev)}
            />
          </>
        )}
      </nav>
    </aside>
  )
}

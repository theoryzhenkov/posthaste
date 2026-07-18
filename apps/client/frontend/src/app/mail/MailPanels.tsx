import type { ReactNode } from 'react'

import { useActivePane } from '@/components/keyboard/usePane'
import type { PaneId } from '@/domain/vocabulary'
import { MessageDetail as MessageDetailPane } from '@/components/mail/detail/MessageDetail'
import { MessageList } from '@/components/mail/list/MessageList'
import { Sidebar } from '@/components/sidebar/Sidebar'
import { conversationViewQuery } from '@/domain/searchQuery'
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from '@/components/ui/display/resizable'

import type { MailClientViewProps } from './MailClientView.types'

/**
 * Marks its region as the keyboard-focused pane on pointer-down and draws a
 * subtle inset ring while active, so `h`/`l` movement and `j`/`k` routing have
 * a visible anchor.
 */
function PaneFocusRegion({
  pane,
  children,
}: {
  pane: PaneId
  children: ReactNode
}) {
  const { activePane, focusPane } = useActivePane()
  return (
    <div
      data-pane={pane}
      data-pane-active={activePane === pane}
      onMouseDownCapture={() => focusPane(pane)}
      className="h-full min-h-0 ring-ring/45 data-[pane-active=true]:ring-1 data-[pane-active=true]:ring-inset"
    >
      {children}
    </div>
  )
}

export function MailPanels(props: MailClientViewProps) {
  return (
    <ResizablePanelGroup
      orientation="horizontal"
      defaultLayout={props.shellDefaultLayout}
      onLayoutChanged={props.onShellLayoutChanged}
      className="min-h-0 flex-1"
    >
      <ResizablePanel
        id="sidebar"
        defaultSize="210px"
        minSize="190px"
        maxSize="420px"
        groupResizeBehavior="preserve-pixel-size"
      >
        <PaneFocusRegion pane="sidebar">
          <Sidebar
            selectedView={props.effectiveView}
            onOpenAccountSettings={(sourceId) =>
              props.onOpenSettings('accounts', { accountId: sourceId })
            }
            onOpenSmartMailboxSettings={(smartMailboxId) =>
              props.onOpenSettings('mailboxes', { smartMailboxId })
            }
            onSelectSmartMailbox={props.onSelectSmartMailbox}
            onSelectSourceMailbox={props.onSelectSourceMailbox}
            onSyncSource={props.onSyncSource}
          />
        </PaneFocusRegion>
      </ResizablePanel>
      <ResizableHandle />
      <ResizablePanel
        id="mail-content"
        minSize="360px"
        groupResizeBehavior="preserve-relative-size"
      >
        <MessagePanels {...props} />
      </ResizablePanel>
    </ResizablePanelGroup>
  )
}

function MessagePanels(props: MailClientViewProps) {
  return (
    <ResizablePanelGroup
      orientation="horizontal"
      defaultLayout={props.messageDefaultLayout}
      onLayoutChanged={props.onMessageLayoutChanged}
      className="h-full min-h-0"
    >
      <ResizablePanel
        id="message-list"
        defaultSize="420px"
        minSize="360px"
        maxSize={props.isMessageDetailOpen ? '960px' : undefined}
      >
        <PaneFocusRegion pane="list">
          <MessageList
            selectedView={props.effectiveView}
            selection={props.selectedMessage}
            onSelectMessage={props.onSelectMessageRef}
            onClearSelection={props.onClearSelectedMessage}
            onClearSearchQuery={props.onRejectSearchPreview}
            actions={props.actions}
            viewRole={props.viewRole}
            onViewConversation={(message) =>
              props.onSearch(conversationViewQuery(message.conversationId))
            }
            searchQuery={props.searchQuery}
            preparedSearchQuery={props.preparedSearchQuery}
          />
        </PaneFocusRegion>
      </ResizablePanel>
      {props.isMessageDetailOpen && (
        <>
          <ResizableHandle />
          <ResizablePanel id="message-detail" minSize="300px">
            {/* The detail pane is not a keyboard focus region — it displays the
                list's selected message; `j`/`k` in the list drive it. */}
            <div className="h-full min-h-0">
              <MessageDetailPane
                selection={props.selectedMessage}
                actions={props.actions}
                viewRole={props.viewRole}
                onEditDraft={props.onEditDraft}
                onForward={props.onForward}
                onOpenFocusedMessage={props.onOpenFocusedMessage}
                onReply={props.onReply}
                onReplyAll={props.onReplyAll}
                onSelectMessage={props.onSelectMessage}
                onSearch={props.onSearch}
                onTag={() => props.onSetTagEditorOpen(true)}
                onUnsubscribeMailto={props.onUnsubscribeMailto}
              />
            </div>
          </ResizablePanel>
        </>
      )}
    </ResizablePanelGroup>
  )
}

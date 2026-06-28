import { MessageDetail as MessageDetailPane } from '@/components/MessageDetail'
import { MessageList } from '@/components/MessageList'
import { Sidebar } from '@/components/Sidebar'
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from '@/components/ui/resizable'

import type { MailClientViewProps } from './MailClientView.types'

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
          onSelectTag={props.onSelectTag}
          onSyncSource={props.onSyncSource}
        />
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
        <MessageList
          selectedView={props.effectiveView}
          selection={props.selectedMessage}
          onSelectMessage={props.onSelectMessageRef}
          onClearSelection={props.onClearSelectedMessage}
          onClearSearchQuery={props.onRejectSearchPreview}
          actions={props.actions}
          viewRole={props.viewRole}
          searchQuery={props.searchQuery}
          preparedSearchQuery={props.preparedSearchQuery}
        />
      </ResizablePanel>
      {props.isMessageDetailOpen && (
        <>
          <ResizableHandle />
          <ResizablePanel id="message-detail" minSize="300px">
            <MessageDetailPane
              selection={props.selectedMessage}
              onArchive={props.onArchive}
              onEditDraft={props.onEditDraft}
              onForward={props.onForward}
              onReply={props.onReply}
              onReplyAll={props.onReplyAll}
              onSelectMessage={props.onSelectMessage}
              onSearch={props.onSearch}
            />
          </ResizablePanel>
        </>
      )}
    </ResizablePanelGroup>
  )
}

/**
 * Message detail header with registry-driven action row.
 *
 * The action row renders the host-resolved `detail-header` actions
 * (`headerActionsFor`, built by the app hosts via `commands/bind` — the same
 * resolver the context menu / palette / keyboard use). This makes the row
 * role-aware and the draft-vs-message branch availability-driven (a draft
 * resolves to edit/discard only) — while this component stays pure UI over
 * `lib/command`'s resolved view (R11: components never import `commands/`).
 *
 * Presentation: icon buttons in a fixed header order, a popover for
 * parameterized actions (Snooze presets), a confirm dialog for destructive
 * `confirm`-bearing actions (delete-permanently), the flag tint, and the tag
 * editor's outside-click anchor attribute.
 *
 * The message's own properties are NOT laid out here — none of them.
 * `MessageFieldRows` renders every one as a labelled row off the shared
 * message-field registry, so the detail pane and the message list describe a
 * field the same way.
 *
 * That now includes the subject, the sender and the tag chips, which this
 * component used to draw itself as a heading, a byline and a trailing row.
 * They were the properties a reader could not turn off, reorder or find in the
 * picker, purely because they were hard-coded here. The avatar circle went
 * with them: its initials stood in for a portrait that mail does not carry,
 * and what is left is a document header rather than a chat bubble.
 *
 * What remains is the header's own furniture — actions, which are verbs rather
 * than properties of the message.
 */
import { useState } from 'react'

import {
  runActionWithConfirm,
  type ActionConfirmCopy as ActionConfirm,
  type ResolvedActionView,
} from '@/lib/command'

import type { MessageDetail, MessageSummary } from '@/data/transport/api'

import { Button } from '../../ui/form/button'
import { KeyboardConfirmDialog } from '../../keyboard/KeyboardConfirmDialog'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '../../ui/overlay/popover'
import { MessageFieldRows } from './MessageFieldRows'

export function MessageHeader({
  conversationSubject,
  message,
  headerActionsFor,
  onSearch,
  threadMessages,
}: {
  conversationSubject: string | null | undefined
  message: MessageDetail
  /** Host-resolved `detail-header` actions for a loaded message
   *  (`commands/bind.buildDetailHeaderActions`) — callbacks pre-bound. */
  headerActionsFor: (message: MessageDetail) => ResolvedActionView[]
  onSearch?: (query: string, append?: boolean) => void
  threadMessages: MessageSummary[]
}) {
  const headerActions = orderForHeader(headerActionsFor(message))

  return (
    <div className="shrink-0 border-b border-border px-5 py-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <MessageFieldRows
            conversationSubject={conversationSubject}
            message={message}
            onSearch={onSearch}
            threadMessageCount={threadMessages.length}
          />
        </div>
        <HeaderActions
          headerActions={headerActions}
          isFlagged={message.isFlagged}
        />
      </div>
    </div>
  )
}

/** The header's fixed visual order (matching the pre-registry layout). Resolver
 *  output is section-ordered for menus; the header keeps its familiar icon-row
 *  arrangement instead. Unknown ids append in resolver order. */
const HEADER_ACTION_ORDER = [
  'message.reply',
  'message.reply-all',
  'message.forward',
  'message.edit-draft',
  'message.archive',
  'message.move-to-inbox',
  'message.move-to-trash',
  'message.delete-permanently',
  'message.discard-draft',
  'message.snooze',
  'message.unsubscribe',
  'message.toggle-flag',
  'message.tag',
  'message.open-focused',
]

function orderForHeader(resolved: ResolvedActionView[]): ResolvedActionView[] {
  const rank = (action: ResolvedActionView) => {
    const index = HEADER_ACTION_ORDER.indexOf(action.id)
    return index === -1 ? HEADER_ACTION_ORDER.length : index
  }
  return [...resolved].sort((a, b) => rank(a) - rank(b))
}

/** "Snooze…" → "Snooze": the icon button needs the bare label (and the snooze
 *  e2e flow anchors on `aria-label="Snooze"`). */
function headerLabel(action: ResolvedActionView): string {
  return action.title.replace(/…$/, '')
}

function HeaderActions({
  headerActions,
  isFlagged,
}: {
  headerActions: ResolvedActionView[]
  isFlagged: boolean
}) {
  // A destructive `confirm`-bearing action (delete-permanently) parks its
  // runner here — same gate the keyboard tier uses, same dialog host.
  const [pendingConfirm, setPendingConfirm] = useState<{
    confirm: ActionConfirm
    onConfirm: () => void
  } | null>(null)

  return (
    <div className="flex shrink-0 items-center gap-1">
      {headerActions.map((action) =>
        action.params ? (
          <HeaderParamAction key={action.id} action={action} />
        ) : (
          <Button
            key={action.id}
            aria-label={headerLabel(action)}
            data-tag-editor-trigger={
              action.id === 'message.tag' ? 'true' : undefined
            }
            onClick={() =>
              runActionWithConfirm(action, (confirm, onConfirm) =>
                setPendingConfirm({ confirm, onConfirm }),
              )
            }
            size="icon-sm"
            title={headerLabel(action)}
            type="button"
            variant="ghost"
            className={
              action.id === 'message.toggle-flag' && isFlagged
                ? 'text-signal-flag'
                : undefined
            }
          >
            <action.icon size={14} strokeWidth={1.6} />
          </Button>
        ),
      )}
      <KeyboardConfirmDialog
        confirm={pendingConfirm?.confirm ?? null}
        onConfirm={() => {
          pendingConfirm?.onConfirm()
          setPendingConfirm(null)
        }}
        onCancel={() => setPendingConfirm(null)}
      />
    </div>
  )
}

/** A PARAMETERIZED action in the header renders as an icon button + popover of
 *  its options (the Snooze presets popover, generically). */
function HeaderParamAction({ action }: { action: ResolvedActionView }) {
  const [open, setOpen] = useState(false)
  const label = headerLabel(action)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          aria-label={label}
          size="icon-sm"
          title={label}
          type="button"
          variant="ghost"
        >
          <action.icon size={14} strokeWidth={1.6} />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-44">
        {(action.params ?? []).map((option) => (
          <Button
            key={option.id}
            className="w-full justify-start"
            onClick={() => {
              void action.executeWith?.(option)
              setOpen(false)
            }}
            size="sm"
            type="button"
            variant="ghost"
          >
            {option.label}
          </Button>
        ))}
      </PopoverContent>
    </Popover>
  )
}

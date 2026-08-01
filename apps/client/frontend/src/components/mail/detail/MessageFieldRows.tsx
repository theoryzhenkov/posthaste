/**
 * The detail pane's labelled field rows — `Arrived at:`, `To:`, and whichever
 * others the reader has turned on — plus the right-click picker that chooses
 * them.
 *
 * Two rules govern what appears. A field must be SELECTED (the picker, stored
 * beside the list's column choice) and it must be PRESENT on this message. The
 * second is why an empty field is a genuine non-render rather than a row with
 * nothing after the colon: `BCC` is stripped in transit on every received
 * message, so a reader who turns it on would otherwise see a permanently
 * broken-looking row on all their mail.
 *
 * The picker mirrors the column picker exactly — same context menu, same
 * checkbox items, same "Revert to Default" — because it is the same idea
 * applied to the other surface, and the two are driven by one registry.
 */
import {
  ContextMenu,
  ContextMenuCheckboxItem,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '../../ui/overlay/context-menu'

import type { MessageSummary } from '@/data/transport/api'

import {
  fieldsForSurface,
  getMessageField,
  messageFieldText,
  visibleDetailFields,
} from '../fields'
import { useDetailFieldConfig } from '../thread/useFieldConfig'
import { formatAbsoluteDate } from './model'

/** Rows render in the registry's declaration order, not selection order, so a
 *  field keeps its place however it was toggled on. */
const DETAIL_FIELDS = fieldsForSurface('detail')

export function MessageFieldRows({
  message,
  threadMessageCount,
}: {
  message: MessageSummary
  /** Shown alongside the date when the conversation has more than one
   *  message; not a message field, so it is not in the registry. */
  threadMessageCount: number
}) {
  const { detailFields, toggleDetailField, resetDetailFields } =
    useDetailFieldConfig()
  const selected = new Set(detailFields)
  const visible = visibleDetailFields(detailFields, message)

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 gap-y-0.5 text-[12px] text-muted-foreground">
          <FieldRow label="Arrived at">
            <span className="font-mono text-[11px]">
              {formatAbsoluteDate(message.receivedAt)}
            </span>
            {threadMessageCount > 1 && (
              <span className="ml-2 font-mono text-[11px] text-muted-foreground/80">
                {threadMessageCount} messages
              </span>
            )}
          </FieldRow>
          {visible.map((id) => (
            <FieldRow key={id} label={getMessageField(id).label}>
              {messageFieldText(id, message)}
            </FieldRow>
          ))}
        </dl>
      </ContextMenuTrigger>
      <ContextMenuContent>
        {DETAIL_FIELDS.map((id) => (
          <ContextMenuCheckboxItem
            key={id}
            checked={selected.has(id)}
            onCheckedChange={() => toggleDetailField(id)}
          >
            {getMessageField(id).label}
          </ContextMenuCheckboxItem>
        ))}
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={resetDetailFields}>
          Revert to Default
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}

function FieldRow({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <>
      <dt className="whitespace-nowrap text-muted-foreground/60">{label}:</dt>
      <dd className="min-w-0 break-words text-foreground/85">{children}</dd>
    </>
  )
}

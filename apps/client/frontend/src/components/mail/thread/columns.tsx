import { Circle, Paperclip, Star } from 'lucide-react'
import type { CSSProperties, ReactNode } from 'react'
import type { MessageSummary } from '../../../data/transport/api/index'
import type { SortDirection } from '../../../domain/vocabulary'
import { cn } from '../../../lib/design/cn'
import { userTags } from '../detail/model'
import {
  fieldsForSurface,
  getMessageField,
  messageFieldText,
  type MessageFieldId,
} from '../fields'
import { MailboxChip } from '../list/MailboxChip'
import type { MailboxDirectory } from '../list/model/useMailboxDirectory'
import { TagChip } from '../tags/TagChip'

/** A column is a message field the registry marks as list-showable. Naming it
 *  as a subtype rather than a second hand-written union is what keeps the two
 *  surfaces from drifting: a field the registry stops offering to the list
 *  stops type-checking here. */
export type ColumnId = Extract<
  MessageFieldId,
  | 'unread'
  | 'flagged'
  | 'attachment'
  | 'from'
  | 'subject'
  | 'preview'
  | 'date'
  | 'source'
  | 'sourceMailbox'
  | 'tags'
>

/**
 * Row-scoped data a cell renderer may need beyond the message itself. Most
 * columns ignore it; the `sourceMailbox` column uses the (cache-only) mailbox
 * directory to resolve which mailbox a row lives in, excluding the mailbox
 * already being viewed.
 */
export interface ColumnRenderContext {
  mailboxDirectory: MailboxDirectory
  /** The mailbox already being viewed (single source-mailbox views), excluded
   *  from the resolved candidate memberships when possible. */
  excludeMailboxId: string | null
}

/** Layout and rendering — everything about a column that is NOT its identity
 *  or its label. Those two come from the message-field registry, so the
 *  column header and the detail row that shows the same field cannot disagree
 *  about what it is called. */
interface BaseColumnDef {
  id: ColumnId
  basis: number
  minWidth?: number
  align?: 'left' | 'right' | 'center'
  header?: ReactNode
  resizable?: boolean
  render: (message: MessageSummary, context: ColumnRenderContext) => ReactNode
}

interface FixedColumnDef extends BaseColumnDef {
  kind: 'fixed'
}

interface StretchColumnDef extends BaseColumnDef {
  kind: 'stretch'
  grow: number
}

export type ColumnDef = FixedColumnDef | StretchColumnDef

export interface ThreadListLayout {
  gridTemplateColumns: string
  minWidth: number
  tableStyle: CSSProperties
  gridStyle: CSSProperties
}

/** Layout + render per column. `label` is deliberately absent — `getColumnDef`
 *  merges it in from the registry. */
const COLUMN_LAYOUTS: Record<ColumnId, ColumnDef> = {
  unread: {
    id: 'unread',
    kind: 'fixed',
    basis: 28,
    align: 'center',
    header: <Circle aria-hidden size={11} className="text-muted-foreground" />,
    render: (message) =>
      !message.isRead ? (
        <span aria-hidden className="size-2 rounded-full bg-signal-unread" />
      ) : null,
  },
  flagged: {
    id: 'flagged',
    kind: 'fixed',
    basis: 28,
    align: 'center',
    header: <Star size={11} className="text-muted-foreground" />,
    render: (message) =>
      message.isFlagged ? (
        <Star size={12} className="fill-signal-flag text-signal-flag" />
      ) : null,
  },
  attachment: {
    id: 'attachment',
    kind: 'fixed',
    basis: 28,
    align: 'center',
    header: <Paperclip size={11} className="text-muted-foreground" />,
    render: (message) =>
      message.hasAttachment ? (
        <Paperclip size={12} className="text-muted-foreground" />
      ) : null,
  },
  from: {
    id: 'from',
    kind: 'fixed',
    basis: 180,
    minWidth: 80,
    resizable: true,
    render: (message) => {
      const sender = messageFieldText('from', message) || 'Unknown'
      return (
        <div className="min-w-0 overflow-hidden">
          <span
            className={cn(
              'block truncate',
              !message.isRead
                ? 'font-medium text-foreground'
                : 'text-muted-foreground/85',
            )}
          >
            {sender}
          </span>
        </div>
      )
    },
  },
  subject: {
    id: 'subject',
    kind: 'stretch',
    basis: 320,
    minWidth: 120,
    grow: 1,
    resizable: true,
    render: (message) => {
      return (
        <div className="flex min-w-0 items-center gap-2 overflow-hidden">
          <span
            className={cn(
              'block min-w-0 truncate leading-none',
              !message.isRead
                ? 'font-semibold text-foreground'
                : 'text-foreground/92',
            )}
          >
            {messageFieldText('subject', message) || '(no subject)'}
          </span>
        </div>
      )
    },
  },
  preview: {
    id: 'preview',
    kind: 'stretch',
    basis: 220,
    minWidth: 160,
    grow: 1,
    resizable: true,
    render: (message) => (
      <span className="min-w-0 truncate text-xs text-muted-foreground">
        {messageFieldText('preview', message)}
      </span>
    ),
  },
  date: {
    id: 'date',
    kind: 'fixed',
    basis: 128,
    minWidth: 80,
    resizable: true,
    render: (message) => (
      <span className="min-w-0 truncate whitespace-nowrap font-mono text-[11px] tabular-nums text-muted-foreground">
        {messageFieldText('date', message)}
      </span>
    ),
  },
  source: {
    id: 'source',
    kind: 'fixed',
    basis: 72,
    minWidth: 54,
    resizable: true,
    render: (message) => {
      return (
        <span
          className={cn(
            'min-w-0 truncate',
            !message.isRead
              ? 'font-medium text-foreground'
              : 'text-muted-foreground/85',
          )}
        >
          {messageFieldText('source', message)}
        </span>
      )
    },
  },
  sourceMailbox: {
    id: 'sourceMailbox',
    kind: 'fixed',
    basis: 120,
    minWidth: 72,
    resizable: true,
    render: (message, { mailboxDirectory, excludeMailboxId }) => {
      const resolved = mailboxDirectory.resolve(message, excludeMailboxId)
      if (!resolved) {
        return null
      }
      return (
        <MailboxChip
          name={resolved.mailbox.name}
          role={resolved.mailbox.role}
          accountName={resolved.isMultiAccount ? resolved.accountName : null}
          className="max-w-full"
        />
      )
    },
  },
  tags: {
    id: 'tags',
    kind: 'stretch',
    basis: 140,
    minWidth: 60,
    grow: 0.5,
    resizable: true,
    render: (message) => {
      const tags = userTags(message.keywords)
      if (tags.length === 0) {
        return null
      }
      return (
        <span className="flex min-w-0 items-center gap-1 overflow-hidden">
          {tags.map((tag) => (
            <TagChip key={tag} name={tag} className="h-5 shrink-0" />
          ))}
        </span>
      )
    },
  },
}

/** All available columns, in the registry's declaration order — derived, so a
 *  field the registry newly offers to the list becomes a pickable column
 *  without a second edit here. */
export const ALL_COLUMNS: ColumnId[] = fieldsForSurface('list') as ColumnId[]

export const DEFAULT_COLUMNS: ColumnId[] = [
  'unread',
  'flagged',
  'attachment',
  'subject',
  'from',
  'date',
  'source',
  'tags',
]

/** A column's full definition: its layout and renderer, with the label taken
 *  from the registry so the list header and the detail row agree. */
export function getColumnDef(id: ColumnId): ColumnDef & { label: string } {
  return { ...COLUMN_LAYOUTS[id], label: getMessageField(id).label }
}

export type ColumnWidths = Partial<Record<ColumnId, number>>

export function getColumnBasis(id: ColumnId, widths?: ColumnWidths): number {
  const def = COLUMN_LAYOUTS[id]
  return Math.max(def.minWidth ?? def.basis, widths?.[id] ?? def.basis)
}

function buildGridTemplate(
  columns: ColumnId[],
  widths?: ColumnWidths,
): string {
  return columns
    .map((id) => {
      const def = COLUMN_LAYOUTS[id]
      const basis = getColumnBasis(id, widths)
      return def.kind === 'stretch'
        ? `minmax(${basis}px, ${def.grow}fr)`
        : `${basis}px`
    })
    .join(' ')
}

export function buildThreadListLayout(
  columns: ColumnId[],
  widths?: ColumnWidths,
): ThreadListLayout {
  const minWidth = columns.reduce(
    (sum, id) => sum + getColumnBasis(id, widths),
    0,
  )
  const gridTemplateColumns = buildGridTemplate(columns, widths)

  return {
    gridTemplateColumns,
    minWidth,
    tableStyle: {
      minWidth,
      width: '100%',
    },
    gridStyle: {
      gridTemplateColumns,
    },
  }
}

// ---------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------

export interface SortConfig {
  columnId: ColumnId
  direction: SortDirection
}

export const DEFAULT_SORT: SortConfig = {
  columnId: 'date',
  direction: 'desc',
}

/** Columns that the backend supports for server-side sorting. */
export const SORTABLE_COLUMNS: ReadonlySet<ColumnId> = new Set<ColumnId>([
  'date',
  'from',
  'subject',
  'source',
  'flagged',
  'attachment',
])

import { Circle, Paperclip, Star } from 'lucide-react'
import type { CSSProperties, ReactNode } from 'react'
import type { MessageSummary } from '../../api/types'
import { cn } from '../../lib/utils'
import { formatRelativeTime } from '../../utils/relativeTime'
import { userTags } from '../message-detail/model'
import { MailboxChip } from '../message-list/MailboxChip'
import type { MailboxDirectory } from '../message-list/useMailboxDirectory'
import { TagChip } from '../tags/TagChip'

export type ColumnId =
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

interface BaseColumnDef {
  id: ColumnId
  label: string
  basis: number
  minWidth?: number
  align?: 'left' | 'right' | 'center'
  header?: ReactNode
  resizable?: boolean
  render: (message: MessageSummary, context: ColumnRenderContext) => ReactNode
}

export interface FixedColumnDef extends BaseColumnDef {
  kind: 'fixed'
}

export interface StretchColumnDef extends BaseColumnDef {
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

const COLUMN_DEFS: Record<ColumnId, ColumnDef> = {
  unread: {
    id: 'unread',
    kind: 'fixed',
    label: 'Unread',
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
    label: 'Flag',
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
    label: 'Attachment',
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
    label: 'From',
    basis: 180,
    minWidth: 80,
    resizable: true,
    render: (message) => {
      const sender = message.fromName ?? message.fromEmail ?? 'Unknown'
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
    label: 'Subject',
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
            {message.subject ?? '(no subject)'}
          </span>
        </div>
      )
    },
  },
  preview: {
    id: 'preview',
    kind: 'stretch',
    label: 'Preview',
    basis: 220,
    minWidth: 160,
    grow: 1,
    resizable: true,
    render: (message) => (
      <span className="min-w-0 truncate text-xs text-muted-foreground">
        {message.preview ?? ''}
      </span>
    ),
  },
  date: {
    id: 'date',
    kind: 'fixed',
    label: 'Date Received',
    basis: 128,
    minWidth: 80,
    resizable: true,
    render: (message) => (
      <span className="min-w-0 truncate whitespace-nowrap font-mono text-[11px] tabular-nums text-muted-foreground">
        {formatRelativeTime(message.receivedAt)}
      </span>
    ),
  },
  source: {
    id: 'source',
    kind: 'fixed',
    label: 'Account',
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
          {message.sourceName}
        </span>
      )
    },
  },
  sourceMailbox: {
    id: 'sourceMailbox',
    kind: 'fixed',
    label: 'Mailbox',
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
    label: 'Tags',
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

/** All available columns in picker display order */
export const ALL_COLUMNS: ColumnId[] = [
  'unread',
  'flagged',
  'attachment',
  'subject',
  'from',
  'date',
  'source',
  'sourceMailbox',
  'tags',
  'preview',
]

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

export function getColumnDef(id: ColumnId): ColumnDef {
  return COLUMN_DEFS[id]
}

export type ColumnWidths = Partial<Record<ColumnId, number>>

export function getColumnBasis(id: ColumnId, widths?: ColumnWidths): number {
  const def = COLUMN_DEFS[id]
  return Math.max(def.minWidth ?? def.basis, widths?.[id] ?? def.basis)
}

export function buildGridTemplate(
  columns: ColumnId[],
  widths?: ColumnWidths,
): string {
  return columns
    .map((id) => {
      const def = COLUMN_DEFS[id]
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

export type SortDirection = 'asc' | 'desc'

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

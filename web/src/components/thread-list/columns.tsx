import { Paperclip, Star } from "lucide-react";
import type { ReactNode } from "react";
import type { ConversationSummary } from "../../api/types";
import { cn } from "../../lib/utils";
import { formatRelativeTime } from "../../utils/relativeTime";

export type ColumnId =
  | "unread"
  | "flagged"
  | "attachment"
  | "from"
  | "subject"
  | "preview"
  | "date"
  | "source"
  | "tags"
  | "threadSize";

export interface ColumnDef {
  id: ColumnId;
  label: string;
  gridWidth: string;
  align?: "left" | "right" | "center";
  header?: ReactNode;
  render: (conversation: ConversationSummary) => ReactNode;
}

const COLUMN_DEFS: Record<ColumnId, ColumnDef> = {
  unread: {
    id: "unread",
    label: "Unread",
    gridWidth: "28px",
    align: "center",
    header: <span aria-hidden className="size-1.5 rounded-full bg-muted-foreground/60" />,
    render: (c) =>
      c.unreadCount > 0 ? (
        <span aria-hidden className="size-2 rounded-full bg-signal-unread" />
      ) : null,
  },
  flagged: {
    id: "flagged",
    label: "Flag",
    gridWidth: "28px",
    align: "center",
    header: <Star size={11} className="text-muted-foreground" />,
    render: (c) =>
      c.isFlagged ? (
        <Star size={12} className="fill-signal-flag text-signal-flag" />
      ) : null,
  },
  attachment: {
    id: "attachment",
    label: "Attachment",
    gridWidth: "28px",
    align: "center",
    header: <Paperclip size={11} className="text-muted-foreground" />,
    render: (c) =>
      c.hasAttachment ? (
        <Paperclip size={12} className="text-muted-foreground" />
      ) : null,
  },
  from: {
    id: "from",
    label: "From",
    gridWidth: "180px",
    render: (c) => {
      const hasUnread = c.unreadCount > 0;
      const sender = c.fromName ?? c.fromEmail ?? "Unknown";
      return (
        <div className="min-w-0">
          <span
            className={cn(
              "block truncate",
              hasUnread
                ? "font-medium text-foreground"
                : "text-muted-foreground/85",
            )}
          >
            {sender}
          </span>
        </div>
      );
    },
  },
  subject: {
    id: "subject",
    label: "Subject",
    gridWidth: "320px",
    render: (c) => {
      const hasUnread = c.unreadCount > 0;
      return (
        <div className="flex min-w-0 items-center gap-2 overflow-hidden">
          {c.messageCount > 1 && (
            <span className="rounded-[3px] border border-[var(--border-strong)] bg-background/45 px-1 font-mono text-[10px] font-medium leading-4 tabular-nums text-muted-foreground">
              {c.messageCount}
            </span>
          )}
          <span
            className={cn(
              "block truncate leading-none",
              hasUnread ? "font-semibold text-foreground" : "text-foreground/92",
            )}
          >
            {c.subject ?? "(no subject)"}
          </span>
        </div>
      );
    },
  },
  preview: {
    id: "preview",
    label: "Preview",
    gridWidth: "minmax(0, 1fr)",
    render: (c) => (
      <span className="truncate text-xs text-muted-foreground">
        {c.preview ?? ""}
      </span>
    ),
  },
  date: {
    id: "date",
    label: "Date Received",
    gridWidth: "128px",
    render: (c) => (
      <span className="whitespace-nowrap font-mono text-[11px] tabular-nums text-muted-foreground">
        {formatRelativeTime(c.latestReceivedAt)}
      </span>
    ),
  },
  source: {
    id: "source",
    label: "Account",
    gridWidth: "72px",
    render: (c) => (
      <span className="truncate font-mono text-[10px] text-muted-foreground/75">
        {c.latestSourceName}
      </span>
    ),
  },
  tags: {
    id: "tags",
    label: "Tags",
    gridWidth: "140px",
    render: () => (
      <span className="truncate font-mono text-[10px] uppercase text-muted-foreground/40" />
    ),
  },
  threadSize: {
    id: "threadSize",
    label: "Size",
    gridWidth: "50px",
    align: "right",
    render: (c) => (
      <span className="font-mono text-xs tabular-nums text-muted-foreground">
        {c.messageCount > 1 ? String(c.messageCount) : ""}
      </span>
    ),
  },
};

/** All available columns in picker display order */
export const ALL_COLUMNS: ColumnId[] = [
  "unread",
  "flagged",
  "attachment",
  "subject",
  "from",
  "date",
  "source",
  "tags",
  "preview",
  "threadSize",
];

export const DEFAULT_COLUMNS: ColumnId[] = [
  "unread",
  "flagged",
  "attachment",
  "subject",
  "from",
  "date",
  "source",
  "tags",
];

export function getColumnDef(id: ColumnId): ColumnDef {
  return COLUMN_DEFS[id];
}

export type ColumnWidths = Partial<Record<ColumnId, number>>;

export function buildGridTemplate(
  columns: ColumnId[],
  widths?: ColumnWidths,
): string {
  return columns
    .map((id) => {
      const override = widths?.[id];
      return override !== undefined ? `${override}px` : COLUMN_DEFS[id].gridWidth;
    })
    .join(" ");
}

// ---------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------

export type SortDirection = "asc" | "desc";

export interface SortConfig {
  columnId: ColumnId;
  direction: SortDirection;
}

export const DEFAULT_SORT: SortConfig = {
  columnId: "date",
  direction: "desc",
};

/** Columns that the backend supports for server-side sorting. */
export const SORTABLE_COLUMNS: ReadonlySet<ColumnId> = new Set<ColumnId>([
  "date",
  "from",
  "subject",
  "source",
  "threadSize",
  "flagged",
  "attachment",
]);

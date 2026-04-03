import { Paperclip, Star } from "lucide-react";
import type { ReactNode } from "react";
import type { ConversationSummary } from "../../api/types";
import { cn } from "../../lib/utils";
import { formatRelativeTime } from "../../utils/relativeTime";

export type ColumnId =
  | "from"
  | "subject"
  | "preview"
  | "date"
  | "source"
  | "threadSize"
  | "flagged"
  | "attachment";

export interface ColumnDef {
  id: ColumnId;
  label: string;
  gridWidth: string;
  align?: "left" | "right" | "center";
  render: (conversation: ConversationSummary) => ReactNode;
}

const COLUMN_DEFS: Record<ColumnId, ColumnDef> = {
  from: {
    id: "from",
    label: "From",
    gridWidth: "minmax(120px, 1fr)",
    render: (c) => {
      const hasUnread = c.unreadCount > 0;
      const sender = c.fromName ?? c.fromEmail ?? "Unknown";
      return (
        <div className="flex min-w-0 items-center gap-1.5">
          {hasUnread && (
            <span className="size-1.5 shrink-0 rounded-full bg-primary" />
          )}
          {c.isFlagged && (
            <Star
              size={12}
              className="shrink-0 fill-amber-400 text-amber-400"
            />
          )}
          <span
            className={cn(
              "truncate",
              hasUnread
                ? "font-semibold text-foreground"
                : "text-muted-foreground",
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
    gridWidth: "minmax(0, 2fr)",
    render: (c) => {
      const hasUnread = c.unreadCount > 0;
      const threadLabel =
        c.messageCount > 1 ? `${c.messageCount} in thread` : null;
      return (
        <div className="min-w-0 overflow-hidden">
          <div className="flex items-center gap-2">
            <span
              className={cn(
                "shrink-0 truncate",
                hasUnread ? "font-semibold" : "text-muted-foreground",
              )}
            >
              {c.subject ?? "(no subject)"}
            </span>
            {c.preview && (
              <span className="truncate text-xs text-muted-foreground">
                {c.preview}
              </span>
            )}
          </div>
          {threadLabel && (
            <div className="mt-0.5 text-[10px] font-mono uppercase tracking-wider text-muted-foreground/70">
              {threadLabel}
            </div>
          )}
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
    label: "Date",
    gridWidth: "100px",
    align: "right",
    render: (c) => (
      <span className="whitespace-nowrap font-mono text-xs tabular-nums text-muted-foreground">
        {formatRelativeTime(c.latestReceivedAt)}
      </span>
    ),
  },
  source: {
    id: "source",
    label: "Source",
    gridWidth: "80px",
    align: "right",
    render: (c) => (
      <span className="font-mono text-[9px] uppercase tracking-wider text-muted-foreground/60">
        {c.latestSourceName}
      </span>
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
  flagged: {
    id: "flagged",
    label: "Flag",
    gridWidth: "32px",
    align: "center",
    render: (c) =>
      c.isFlagged ? (
        <Star size={12} className="fill-amber-400 text-amber-400" />
      ) : null,
  },
  attachment: {
    id: "attachment",
    label: "Attach",
    gridWidth: "32px",
    align: "center",
    render: (c) =>
      c.hasAttachment ? (
        <Paperclip size={12} className="text-muted-foreground" />
      ) : null,
  },
};

/** All available columns in picker display order */
export const ALL_COLUMNS: ColumnId[] = [
  "flagged",
  "attachment",
  "from",
  "subject",
  "preview",
  "date",
  "source",
  "threadSize",
];

export const DEFAULT_COLUMNS: ColumnId[] = [
  "from",
  "subject",
  "date",
  "source",
];

export function getColumnDef(id: ColumnId): ColumnDef {
  return COLUMN_DEFS[id];
}

export function buildGridTemplate(columns: ColumnId[]): string {
  return columns.map((id) => COLUMN_DEFS[id].gridWidth).join(" ");
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

function getSortValue(
  columnId: ColumnId,
  c: ConversationSummary,
): string | number {
  switch (columnId) {
    case "from":
      return (c.fromName ?? c.fromEmail ?? "").toLowerCase();
    case "subject":
      return (c.subject ?? "").toLowerCase();
    case "preview":
      return (c.preview ?? "").toLowerCase();
    case "date":
      return c.latestReceivedAt;
    case "source":
      return c.latestSourceName.toLowerCase();
    case "threadSize":
      return c.messageCount;
    case "flagged":
      return c.isFlagged ? 1 : 0;
    case "attachment":
      return c.hasAttachment ? 1 : 0;
  }
}

export function compareConversations(
  config: SortConfig,
  a: ConversationSummary,
  b: ConversationSummary,
): number {
  const va = getSortValue(config.columnId, a);
  const vb = getSortValue(config.columnId, b);
  let cmp: number;
  if (typeof va === "number" && typeof vb === "number") {
    cmp = va - vb;
  } else {
    cmp = String(va).localeCompare(String(vb));
  }
  return config.direction === "asc" ? cmp : -cmp;
}

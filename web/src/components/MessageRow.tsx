/**
 * Single conversation row in the message list.
 *
 * Renders sender, subject, preview, relative timestamp, unread dot,
 * flag star, message count badge, and source tag.
 *
 * @spec docs/L1-ui#messagelist
 */
import type { ConversationSummary } from "../api/types";
import { cn } from "../lib/utils";
import {
  type ColumnId,
  type ColumnWidths,
  buildGridTemplate,
  getColumnDef,
} from "./thread-list/columns";

/** @spec docs/L1-ui#messagelist */
interface MessageRowProps {
  message: ConversationSummary;
  isSelected: boolean;
  onSelect: () => void;
  columns: ColumnId[];
  widths?: ColumnWidths;
}

/**
 * Fixed-height conversation row displaying sender, subject,
 * preview, date, unread state, flag, and thread count.
 *
 * @spec docs/L1-ui#messagelist
 */
export function MessageRow({
  message,
  isSelected,
  onSelect,
  columns,
  widths,
}: MessageRowProps) {
  return (
    <button
      className={cn(
        "grid h-full w-full items-center gap-3",
        "border-b border-border px-3 py-2 text-left text-sm transition-colors",
        "ph-focus-ring",
        isSelected && "border-l-2 border-l-brand-coral bg-sidebar-accent text-sidebar-accent-foreground",
        !isSelected && "border-l-2 border-l-transparent bg-panel hover:bg-panel-muted",
      )}
      style={{ gridTemplateColumns: buildGridTemplate(columns, widths) }}
      onClick={onSelect}
      type="button"
    >
      {columns.map((columnId) => {
        const def = getColumnDef(columnId);
        return (
          <div
            key={columnId}
            className={cn(
              "min-w-0",
              def.align === "right" && "text-right",
              def.align === "center" && "flex justify-center",
            )}
          >
            {def.render(message)}
          </div>
        );
      })}
    </button>
  );
}

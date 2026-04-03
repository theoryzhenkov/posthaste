import type { ConversationSummary } from "../api/types";
import { cn } from "../lib/utils";
import {
  type ColumnId,
  buildGridTemplate,
  getColumnDef,
} from "./thread-list/columns";

interface MessageRowProps {
  message: ConversationSummary;
  isSelected: boolean;
  onSelect: () => void;
  columns: ColumnId[];
}

export function MessageRow({
  message,
  isSelected,
  onSelect,
  columns,
}: MessageRowProps) {
  return (
    <button
      className={cn(
        "grid h-full w-full items-center gap-3",
        "border-b border-border px-3 py-2 text-left text-sm transition-colors",
        "hover:bg-accent/50",
        isSelected && "border-l-2 border-l-primary bg-accent",
        !isSelected && "border-l-2 border-l-transparent",
      )}
      style={{ gridTemplateColumns: buildGridTemplate(columns) }}
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

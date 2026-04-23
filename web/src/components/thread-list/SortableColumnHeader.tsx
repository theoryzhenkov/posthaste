import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { ArrowDown, ArrowUp } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "../../lib/utils";
import { ColumnResizeHandle } from "./ColumnResizeHandle";
import type { SortDirection } from "./columns";

interface SortableColumnHeaderProps {
  id: string;
  label: string;
  align?: "left" | "right" | "center";
  icon?: ReactNode;
  resizeBasis?: number;
  resizeMinWidth?: number;
  sortDirection?: SortDirection;
  isSortable?: boolean;
  showResizeDivider?: boolean;
  resizePlacement?: "between-columns" | "table-end";
  onSort: () => void;
  onResize?: (width: number) => void;
}

export function SortableColumnHeader({
  id,
  label,
  align,
  icon,
  resizeBasis,
  resizeMinWidth,
  sortDirection,
  isSortable = true,
  showResizeDivider = true,
  resizePlacement = "between-columns",
  onSort,
  onResize,
}: SortableColumnHeaderProps) {
  const hasResizeHandle = onResize !== undefined && resizeBasis !== undefined;
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <button
      ref={setNodeRef}
      type="button"
      className={cn(
        "relative flex h-full min-w-0 select-none items-center overflow-visible px-0 text-[11px]",
        isSortable ? "cursor-pointer" : "cursor-grab",
        isDragging && "z-10 opacity-60",
      )}
      style={style}
      onClick={() => {
        if (isSortable) {
          onSort();
        }
      }}
      {...attributes}
      {...listeners}
    >
      <span
        className={cn(
          "flex h-full min-w-0 flex-1 items-center gap-1 overflow-hidden px-2.5",
          hasResizeHandle && "pr-4",
          resizePlacement === "table-end" && "pr-5",
          align === "right" && "justify-end",
          align === "center" && "justify-center px-0",
        )}
      >
        {icon ? (
          <span className="min-w-0 shrink-0 overflow-hidden">{icon}</span>
        ) : (
          <span className="min-w-0 truncate">{label.toUpperCase()}</span>
        )}
        {sortDirection === "asc" && (
          <ArrowUp size={10} className="shrink-0 text-foreground" />
        )}
        {sortDirection === "desc" && (
          <ArrowDown size={10} className="shrink-0 text-foreground" />
        )}
      </span>
      {hasResizeHandle && (
        <ColumnResizeHandle
          basis={resizeBasis}
          minWidth={resizeMinWidth}
          showDivider={showResizeDivider}
          placement={resizePlacement}
          onResize={onResize}
        />
      )}
    </button>
  );
}

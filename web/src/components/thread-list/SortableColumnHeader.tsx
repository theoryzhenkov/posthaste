import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { ArrowDown, ArrowUp } from "lucide-react";
import { useRef } from "react";
import { cn } from "../../lib/utils";
import { ColumnResizeHandle } from "./ColumnResizeHandle";
import type { SortDirection } from "./columns";

interface SortableColumnHeaderProps {
  id: string;
  label: string;
  align?: "left" | "right" | "center";
  sortDirection?: SortDirection;
  onSort: () => void;
  onResize?: (width: number) => void;
}

export function SortableColumnHeader({
  id,
  label,
  align,
  sortDirection,
  onSort,
  onResize,
}: SortableColumnHeaderProps) {
  const columnRef = useRef<HTMLButtonElement>(null);
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
    cursor: "pointer",
  };

  return (
    <button
      ref={(node) => {
        setNodeRef(node);
        (columnRef as React.MutableRefObject<HTMLButtonElement | null>).current = node;
      }}
      type="button"
      className={cn(
        "relative flex items-center gap-0.5 select-none",
        align === "right" && "justify-end",
        align === "center" && "justify-center",
        isDragging && "z-10 opacity-60",
      )}
      style={style}
      onClick={onSort}
      {...attributes}
      {...listeners}
    >
      <span>{label}</span>
      {sortDirection === "asc" && (
        <ArrowUp size={10} className="shrink-0 text-foreground" />
      )}
      {sortDirection === "desc" && (
        <ArrowDown size={10} className="shrink-0 text-foreground" />
      )}
      {onResize && (
        <ColumnResizeHandle onResize={onResize} columnRef={columnRef} />
      )}
    </button>
  );
}

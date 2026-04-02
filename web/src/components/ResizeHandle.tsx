import { cn } from "../lib/utils";

interface ResizeHandleProps {
  onMouseDown: (e: React.MouseEvent) => void;
}

export function ResizeHandle({ onMouseDown }: ResizeHandleProps) {
  return (
    <div
      className={cn(
        "group relative z-10 w-0 cursor-col-resize",
        // Hit area extends beyond the visual line
        "before:absolute before:inset-y-0 before:-left-1.5 before:w-3",
        // Visual indicator — thin line that highlights on hover/active
        "after:absolute after:inset-y-0 after:-left-px after:w-0.5",
        "after:bg-border after:transition-colors",
        "after:hover:bg-primary/40 after:active:bg-primary/60",
      )}
      onMouseDown={onMouseDown}
    />
  );
}

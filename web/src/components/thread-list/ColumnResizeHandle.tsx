import { useCallback, useRef } from "react";

interface ColumnResizeHandleProps {
  onResize: (width: number) => void;
  /** Ref to the parent column header element to measure its current width. */
  columnRef: React.RefObject<HTMLElement | null>;
  minWidth?: number;
}

export function ColumnResizeHandle({
  onResize,
  columnRef,
  minWidth = 32,
}: ColumnResizeHandleProps) {
  const draggingRef = useRef(false);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const startX = e.clientX;
      const startWidth = columnRef.current?.getBoundingClientRect().width ?? 100;
      draggingRef.current = true;

      const target = e.currentTarget as HTMLElement;
      target.setPointerCapture(e.pointerId);

      function onPointerMove(ev: PointerEvent) {
        if (!draggingRef.current) return;
        const delta = ev.clientX - startX;
        const newWidth = Math.max(minWidth, startWidth + delta);
        onResize(newWidth);
      }

      function onPointerUp() {
        draggingRef.current = false;
        target.removeEventListener("pointermove", onPointerMove);
        target.removeEventListener("pointerup", onPointerUp);
      }

      target.addEventListener("pointermove", onPointerMove);
      target.addEventListener("pointerup", onPointerUp);
    },
    [columnRef, minWidth, onResize],
  );

  return (
    <div
      className="group absolute -right-1.5 top-0 z-20 flex h-full w-3 cursor-col-resize items-center justify-center"
      onPointerDown={handlePointerDown}
      onClick={(e) => e.stopPropagation()}
    >
      <div className="h-3 w-px bg-border transition-all group-hover:h-full group-hover:w-0.5 group-hover:bg-brand-coral" />
    </div>
  );
}

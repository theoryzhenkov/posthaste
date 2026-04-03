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
      className="absolute top-0 right-0 z-20 h-full w-1.5 cursor-col-resize opacity-0 transition-opacity hover:opacity-100 active:opacity-100"
      onPointerDown={handlePointerDown}
      onClick={(e) => e.stopPropagation()}
    >
      <div className="mx-auto h-full w-px bg-border" />
    </div>
  );
}

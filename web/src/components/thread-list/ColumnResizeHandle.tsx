import { useCallback, useRef } from "react";

interface ColumnResizeHandleProps {
  onResize: (width: number) => void;
  basis: number;
  minWidth?: number;
  showDivider?: boolean;
}

export function ColumnResizeHandle({
  onResize,
  basis,
  minWidth = 32,
  showDivider = true,
}: ColumnResizeHandleProps) {
  const draggingRef = useRef(false);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const startX = e.clientX;
      const startWidth = basis;
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
    [basis, minWidth, onResize],
  );

  return (
    <div
      className="group absolute right-0 top-0 z-20 flex h-full w-2 translate-x-1/2 cursor-col-resize items-center justify-center"
      onPointerDown={handlePointerDown}
      onClick={(e) => e.stopPropagation()}
    >
      <div
        className={
          showDivider
            ? "h-full w-px bg-border/80 transition-colors group-hover:bg-brand-coral/70 group-active:bg-brand-coral"
            : "h-full w-px bg-transparent transition-colors group-hover:bg-brand-coral/70 group-active:bg-brand-coral"
        }
      />
    </div>
  );
}

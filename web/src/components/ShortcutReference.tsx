/**
 * Keyboard shortcut reference overlay, toggled with `?`.
 *
 * @spec docs/L1-ui#keyboard-shortcuts
 */
import { useEffect, useRef } from "react";

interface ShortcutReferenceProps {
  onClose: () => void;
}

const SHORTCUTS: { keys: string[]; action: string }[] = [
  { keys: ["j", "\u2193"], action: "Next conversation" },
  { keys: ["k", "\u2191"], action: "Previous conversation" },
  { keys: ["e"], action: "Archive" },
  { keys: ["#", "Backspace"], action: "Trash" },
  { keys: ["/"], action: "Focus search" },
  { keys: ["?"], action: "Toggle this reference" },
];

export function ShortcutReference({ onClose }: ShortcutReferenceProps) {
  const cardRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  function handleOverlayClick(event: React.MouseEvent) {
    if (cardRef.current && !cardRef.current.contains(event.target as Node)) {
      onClose();
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-background/95 backdrop-blur-sm"
      onClick={handleOverlayClick}
    >
      <div
        ref={cardRef}
        className="w-full max-w-sm rounded-xl border border-border bg-card p-5 shadow-lg"
      >
        <h2 className="text-sm font-semibold tracking-tight">Keyboard shortcuts</h2>
        <div className="mt-4 space-y-2.5">
          {SHORTCUTS.map((shortcut) => (
            <div
              key={shortcut.action}
              className="flex items-center justify-between gap-4 text-sm"
            >
              <span className="text-muted-foreground">{shortcut.action}</span>
              <div className="flex items-center gap-1.5">
                {shortcut.keys.map((key, index) => (
                  <span key={key}>
                    {index > 0 && (
                      <span className="mr-1.5 text-xs text-muted-foreground/50">/</span>
                    )}
                    <kbd className="rounded border border-border bg-muted px-1.5 py-0.5 font-mono text-xs">
                      {key}
                    </kbd>
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

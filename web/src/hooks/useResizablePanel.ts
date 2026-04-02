import { useCallback, useEffect, useRef, useState } from "react";

const STORAGE_KEY = "posthaste-panel-widths";

interface PanelWidths {
  sidebar: number;
  messageList: number;
}

const DEFAULTS: PanelWidths = { sidebar: 220, messageList: 420 };
const MIN_SIDEBAR = 160;
const MAX_SIDEBAR = 400;
const MIN_MESSAGE_LIST = 280;
const MAX_MESSAGE_LIST = 800;

function loadWidths(): PanelWidths {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      const parsed = JSON.parse(stored) as Partial<PanelWidths>;
      return {
        sidebar: clamp(parsed.sidebar ?? DEFAULTS.sidebar, MIN_SIDEBAR, MAX_SIDEBAR),
        messageList: clamp(
          parsed.messageList ?? DEFAULTS.messageList,
          MIN_MESSAGE_LIST,
          MAX_MESSAGE_LIST,
        ),
      };
    }
  } catch {
    // ignore corrupt storage
  }
  return DEFAULTS;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

type Panel = "sidebar" | "messageList";

export function useResizablePanel() {
  const [widths, setWidths] = useState<PanelWidths>(loadWidths);
  const dragging = useRef<{ panel: Panel; startX: number; startWidth: number } | null>(null);

  const persistWidths = useCallback((next: PanelWidths) => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  }, []);

  const startResize = useCallback((panel: Panel, e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = {
      panel,
      startX: e.clientX,
      startWidth: panel === "sidebar" ? widths.sidebar : widths.messageList,
    };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, [widths]);

  useEffect(() => {
    function onMouseMove(e: MouseEvent) {
      const d = dragging.current;
      if (!d) return;

      const delta = e.clientX - d.startX;
      const raw = d.startWidth + delta;

      const [min, max] =
        d.panel === "sidebar"
          ? [MIN_SIDEBAR, MAX_SIDEBAR]
          : [MIN_MESSAGE_LIST, MAX_MESSAGE_LIST];

      const clamped = clamp(raw, min, max);

      setWidths((prev) => {
        const next = { ...prev, [d.panel]: clamped };
        persistWidths(next);
        return next;
      });
    }

    function onMouseUp() {
      if (!dragging.current) return;
      dragging.current = null;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    }

    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [persistWidths]);

  return { widths, startResize };
}

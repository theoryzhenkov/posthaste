import type { ResizeHandle } from '@/floatingPanelGeometry'

// Edge and corner resize handles. Corners drive both axes; corners sit above the
// edge handles so the overlap at the borders resolves to a 2-D drag.
export const RESIZE_HANDLES: {
  handle: ResizeHandle
  className: string
  corner: boolean
}[] = [
  {
    handle: 'top',
    className: 'inset-x-0 top-0 h-1.5 cursor-ns-resize',
    corner: false,
  },
  {
    handle: 'bottom',
    className: 'inset-x-0 bottom-0 h-1.5 cursor-ns-resize',
    corner: false,
  },
  {
    handle: 'left',
    className: 'inset-y-0 left-0 w-1.5 cursor-ew-resize',
    corner: false,
  },
  {
    handle: 'right',
    className: 'inset-y-0 right-0 w-1.5 cursor-ew-resize',
    corner: false,
  },
  {
    handle: 'top-left',
    className: 'left-0 top-0 size-3 cursor-nwse-resize',
    corner: true,
  },
  {
    handle: 'top-right',
    className: 'right-0 top-0 size-3 cursor-nesw-resize',
    corner: true,
  },
  {
    handle: 'bottom-left',
    className: 'bottom-0 left-0 size-3 cursor-nesw-resize',
    corner: true,
  },
  {
    handle: 'bottom-right',
    className: 'bottom-0 right-0 size-3 cursor-nwse-resize',
    corner: true,
  },
]

export const GUIDE_LINE_ACTIVE_CLASS =
  'bg-[color-mix(in_oklab,var(--brand-coral)_46%,transparent)]'
export const GUIDE_LINE_IDLE_CLASS =
  'bg-[color-mix(in_oklab,var(--foreground)_14%,transparent)]'
// The resize grid shows many more lines than the movement rails, so its idle
// lines are fainter to stay calm.
export const GRID_LINE_IDLE_CLASS =
  'bg-[color-mix(in_oklab,var(--foreground)_8%,transparent)]'

// Don't start a header drag from interactive controls or the grip (the grip has
// its own drag handlers); empty header space drags the panel.
export const HEADER_NO_DRAG_SELECTOR =
  'button, a, input, textarea, select, label, [contenteditable], [data-no-drag]'

// Manual double-click detection on a resize handle (preventDefault suppresses
// the native dblclick): a second press on the same handle within this window
// and pointer slop grows the panel to the grid instead of dragging.
export const RESIZE_DOUBLE_CLICK_MS = 350
export const RESIZE_DOUBLE_CLICK_SLOP = 6

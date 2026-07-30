import { resizeGridCell } from '@/components/floating/geometry'

import { viewportSize } from './geometry'
import type { PanelSize } from './types'

export function ResizeCellBadge({
  isResizing,
  panelSize,
}: {
  isResizing: boolean
  panelSize: PanelSize | null
}) {
  const resizeCells =
    isResizing && panelSize
      ? (() => {
          const cell = resizeGridCell(viewportSize())
          return {
            columns:
              cell.width > 0 ? Math.round(panelSize.width / cell.width) : 0,
            rows:
              cell.height > 0 ? Math.round(panelSize.height / cell.height) : 0,
          }
        })()
      : null

  if (!resizeCells) {
    return null
  }

  return (
    <div className="pointer-events-none absolute left-1/2 top-1/2 z-40 -translate-x-1/2 -translate-y-1/2 rounded-md border [border-color:color-mix(in_oklab,var(--brand-coral)_30%,var(--border))] bg-[color-mix(in_oklab,var(--panel)_85%,transparent)] px-2 py-1 font-mono text-[11px] font-medium text-foreground/80 shadow-sm">
      {resizeCells.columns} × {resizeCells.rows}
    </div>
  )
}

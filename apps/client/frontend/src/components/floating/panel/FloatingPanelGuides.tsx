import type {
  ActiveRails,
  ActiveResizeLines,
  GuideLayout,
  ResizeHandle,
  ResizeSnapLines,
} from '@/components/floating/geometry'
import {
  GRID_LINE_IDLE_CLASS,
  GUIDE_LINE_ACTIVE_CLASS,
  GUIDE_LINE_IDLE_CLASS,
} from './constants'

export function FloatingPanelGuides({
  activeRails,
  activeResizeLines,
  guideLayout,
  isDragging,
  isResizing,
  resizeHandle,
  resizeLines,
}: {
  activeRails: ActiveRails
  activeResizeLines: ActiveResizeLines
  guideLayout: GuideLayout | null
  isDragging: boolean
  isResizing: boolean
  resizeHandle: ResizeHandle | null
  resizeLines: ResizeSnapLines | null
}) {
  const resizeShowsX =
    resizeHandle !== null &&
    (resizeHandle.includes('left') || resizeHandle.includes('right'))
  const resizeShowsY =
    resizeHandle !== null &&
    (resizeHandle.includes('top') || resizeHandle.includes('bottom'))
  return (
    <>
      {isDragging && guideLayout && (
        <div className="pointer-events-none fixed inset-0">
          {guideLayout.columns.map((column) => {
            const lineClass =
              activeRails.x === column.rail
                ? GUIDE_LINE_ACTIVE_CLASS
                : GUIDE_LINE_IDLE_CLASS
            return (
              <div
                key={`column:${column.rail}`}
                className="absolute top-0 h-full"
                style={{ left: column.left, width: column.width }}
              >
                <div
                  className={`absolute left-0 top-0 h-full w-px ${lineClass}`}
                />
                <div
                  className={`absolute right-0 top-0 h-full w-px ${lineClass}`}
                />
              </div>
            )
          })}
          {guideLayout.rows.map((row) => {
            const lineClass =
              activeRails.y === row.rail
                ? GUIDE_LINE_ACTIVE_CLASS
                : GUIDE_LINE_IDLE_CLASS
            return (
              <div
                key={`row:${row.rail}`}
                className="absolute left-0 w-full"
                style={{ height: row.height, top: row.top }}
              >
                <div
                  className={`absolute left-0 top-0 h-px w-full ${lineClass}`}
                />
                <div
                  className={`absolute bottom-0 left-0 h-px w-full ${lineClass}`}
                />
              </div>
            )
          })}
        </div>
      )}
      {isResizing && resizeLines && (
        <div className="pointer-events-none fixed inset-0">
          {resizeShowsX &&
            resizeLines.x.map((x) => (
              <div
                key={`resize-x:${x}`}
                className={`absolute top-0 h-full w-px ${
                  activeResizeLines.x.includes(x)
                    ? GUIDE_LINE_ACTIVE_CLASS
                    : GRID_LINE_IDLE_CLASS
                }`}
                style={{ left: x }}
              />
            ))}
          {resizeShowsY &&
            resizeLines.y.map((y) => (
              <div
                key={`resize-y:${y}`}
                className={`absolute left-0 h-px w-full ${
                  activeResizeLines.y.includes(y)
                    ? GUIDE_LINE_ACTIVE_CLASS
                    : GRID_LINE_IDLE_CLASS
                }`}
                style={{ top: y }}
              />
            ))}
        </div>
      )}
    </>
  )
}

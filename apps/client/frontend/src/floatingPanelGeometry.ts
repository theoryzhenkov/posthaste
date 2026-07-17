export {
  FLOATING_PANEL_RAIL_RESISTANCE_DISTANCE,
  type ActiveRails,
  type ActiveResizeLines,
  type GuideColumn,
  type GuideLayout,
  type GuideRow,
  type PanelGeometry,
  type PanelOffset,
  type PanelRails,
  type PanelRect,
  type RailDragState,
  type ResizeConstraints,
  type ResizeHandle,
  type ResizeLocks,
  type ResizeResult,
  type ResizeSnapLines,
} from './floating-panel-geometry/types'
export {
  clampPanelOffset,
  isFiniteOffset,
} from './floating-panel-geometry/offset'
export {
  guideColumns,
  guideRows,
  panelRailOffsets,
  resistPanelOffset,
} from './floating-panel-geometry/rails'
export {
  expandPanelToGrid,
  resizeGridCell,
  resizeGridLines,
  resizePanelRect,
} from './floating-panel-geometry/resize'

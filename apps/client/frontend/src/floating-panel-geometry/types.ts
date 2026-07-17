export const FLOATING_PANEL_RAIL_RESISTANCE_DISTANCE = 12

export interface PanelOffset {
  x: number
  y: number
}

export interface PanelGeometry {
  width: number
  height: number
}

export interface ActiveRails {
  x: number | null
  y: number | null
}

export interface PanelRails {
  x: number[]
  y: number[]
}

export interface GuideColumn {
  left: number
  rail: number
  width: number
}

export interface GuideRow {
  height: number
  rail: number
  top: number
}

export interface GuideLayout {
  columns: GuideColumn[]
  rows: GuideRow[]
}

export interface RailDragState {
  lockedX?: number | null
  lockedY?: number | null
}

export type ResizeHandle =
  | 'top'
  | 'right'
  | 'bottom'
  | 'left'
  | 'top-left'
  | 'top-right'
  | 'bottom-left'
  | 'bottom-right'

export interface PanelRect {
  left: number
  top: number
  width: number
  height: number
}

export interface ResizeConstraints {
  minWidth: number
  maxWidth: number
  minHeight: number
  maxHeight: number
}

export interface ResizeSnapLines {
  x: number[]
  y: number[]
}

export interface ActiveResizeLines {
  x: number[]
  y: number[]
}

export interface ResizeLocks {
  x: number | null
  y: number | null
}

export interface ResizeResult {
  size: PanelGeometry
  offset: PanelOffset
  activeLines: ActiveResizeLines
  locks: ResizeLocks
}

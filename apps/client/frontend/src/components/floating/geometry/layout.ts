export const FLOATING_PANEL_GRID = {
  columns: 12,
  rows: 8,
  screenMargin: 16,
  topOffset: 54,
} as const

export type FloatingPanelSizePreset = 'command' | 'compose'

interface FloatingPanelSizePolicy {
  widthColumns: number
  heightRows?: number
  minWidth: number
  maxWidth: number
  minHeight: number
  maxHeight?: number
}

export interface ViewportSize {
  width: number
  height: number
}

const FLOATING_PANEL_SIZE_POLICIES = {
  command: {
    widthColumns: 6,
    minWidth: 360,
    maxWidth: 640,
    minHeight: 192,
    maxHeight: 560,
  },
  compose: {
    widthColumns: 8,
    heightRows: 6,
    minWidth: 560,
    maxWidth: 780,
    minHeight: 460,
    maxHeight: 640,
  },
} as const satisfies Record<FloatingPanelSizePreset, FloatingPanelSizePolicy>

function floatingPanelSizePolicy(
  preset: FloatingPanelSizePreset,
): FloatingPanelSizePolicy {
  return FLOATING_PANEL_SIZE_POLICIES[preset]
}

// Floor sizes when a panel has no preset; mirror the `min-w-72`/`min-h-48`
// safety floor on the panel sheet.
const FLOATING_PANEL_FALLBACK_MIN_WIDTH = 288
const FLOATING_PANEL_FALLBACK_MIN_HEIGHT = 192

export interface FloatingPanelResizeConstraints {
  minWidth: number
  maxWidth: number
  minHeight: number
  maxHeight: number
}

// Bounds for user-driven edge/corner resizing: the minimum comes from the
// preset (or the shared floor), while the maximum is the usable viewport so a
// panel can grow to fill the screen rather than being capped at its preset.
export function floatingPanelResizeConstraints(
  preset: FloatingPanelSizePreset | undefined,
  viewport: ViewportSize,
): FloatingPanelResizeConstraints {
  const policy = preset ? floatingPanelSizePolicy(preset) : undefined
  const usableWidth = Math.max(
    0,
    viewport.width - FLOATING_PANEL_GRID.screenMargin * 2,
  )
  const usableHeight = Math.max(
    0,
    viewport.height -
      FLOATING_PANEL_GRID.topOffset -
      FLOATING_PANEL_GRID.screenMargin,
  )
  return {
    minWidth: Math.min(
      policy?.minWidth ?? FLOATING_PANEL_FALLBACK_MIN_WIDTH,
      usableWidth,
    ),
    maxWidth: usableWidth,
    minHeight: Math.min(
      policy?.minHeight ?? FLOATING_PANEL_FALLBACK_MIN_HEIGHT,
      usableHeight,
    ),
    maxHeight: usableHeight,
  }
}

export function floatingPanelSizeStyle(
  preset: FloatingPanelSizePreset,
): Record<string, string> {
  const policy = floatingPanelSizePolicy(preset)
  const usableWidth = `calc(100vw - ${FLOATING_PANEL_GRID.screenMargin * 2}px)`
  const widthRatio = ratio(policy.widthColumns, FLOATING_PANEL_GRID.columns)
  const minWidth = `min(${policy.minWidth}px, ${usableWidth})`
  const maxWidth = `min(${policy.maxWidth}px, ${usableWidth})`
  const style: Record<string, string> = {
    width: `clamp(${minWidth}, calc(${usableWidth} * ${widthRatio}), ${maxWidth})`,
    minWidth,
    maxWidth: usableWidth,
    minHeight: `min(${policy.minHeight}px, calc(100vh - ${FLOATING_PANEL_GRID.topOffset + FLOATING_PANEL_GRID.screenMargin}px))`,
    maxHeight: `calc(100vh - ${FLOATING_PANEL_GRID.topOffset + FLOATING_PANEL_GRID.screenMargin}px)`,
  }

  if (policy.heightRows !== undefined) {
    const usableHeight = `calc(100vh - ${FLOATING_PANEL_GRID.topOffset + FLOATING_PANEL_GRID.screenMargin}px)`
    const heightRatio = ratio(policy.heightRows, FLOATING_PANEL_GRID.rows)
    const minHeight = `min(${policy.minHeight}px, ${usableHeight})`
    const maxHeight = `min(${policy.maxHeight ?? policy.minHeight}px, ${usableHeight})`
    style.height = `clamp(${minHeight}, calc(${usableHeight} * ${heightRatio}), ${maxHeight})`
  }

  return style
}

function ratio(parts: number, total: number): string {
  return `${(parts / total).toFixed(4).replace(/0+$/, '').replace(/\.$/, '')}`
}

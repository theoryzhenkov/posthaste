export const uiDensities = ['compact', 'cozy', 'comfortable'] as const

export type UiDensity = (typeof uiDensities)[number]

export const defaultUiDensity = 'compact' as const satisfies UiDensity

export const uiDensitySettings = {
  compact: {
    rowHeight: 78,
    controlHeight: 28,
    gap: 6,
  },
  cozy: {
    rowHeight: 88,
    controlHeight: 32,
    gap: 8,
  },
  comfortable: {
    rowHeight: 100,
    controlHeight: 36,
    gap: 10,
  },
} as const satisfies Record<
  UiDensity,
  {
    rowHeight: number
    controlHeight: number
    gap: number
  }
>

export function isUiDensity(value: string): value is UiDensity {
  return uiDensities.includes(value as UiDensity)
}

import { normalizeAccentHue } from './accent'
import type { ResolvedThemeMode } from './theme'

export type GlassBloomId = string

export interface GlassBloom {
  readonly id: GlassBloomId
  readonly hue: number
  readonly x: number
  readonly y: number
  readonly opacity: number
  readonly radius: number
}

export interface GlassThemeParameters {
  readonly blooms: readonly GlassBloom[]
}

export type GlassBloomPatch = Partial<
  Pick<GlassBloom, 'hue' | 'x' | 'y' | 'opacity' | 'radius'>
>

export const minGlassBloomCount = 1
export const maxGlassBloomCount = 8

export const defaultGlassThemeParameters = {
  blooms: [
    {
      id: 'bloom-1',
      hue: 285,
      x: 20,
      y: 10,
      opacity: 0.35,
      radius: 45,
    },
    {
      id: 'bloom-2',
      hue: 345,
      x: 85,
      y: 25,
      opacity: 0.25,
      radius: 45,
    },
    {
      id: 'bloom-3',
      hue: 220,
      x: 50,
      y: 90,
      opacity: 0.3,
      radius: 50,
    },
    {
      id: 'bloom-4',
      hue: 155,
      x: 10,
      y: 85,
      opacity: 0.2,
      radius: 40,
    },
  ],
} as const satisfies GlassThemeParameters

const legacyBloomIds = new Set(['primary', 'warm', 'cool', 'fresh'])

export function clampNumber(
  value: unknown,
  min: number,
  max: number,
  fallback: number,
): number {
  const number = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(number)) {
    return fallback
  }
  return Math.min(max, Math.max(min, number))
}

function defaultBloom(index: number): GlassBloom {
  return defaultGlassThemeParameters.blooms[
    index % defaultGlassThemeParameters.blooms.length
  ]
}

function normalizeBloomId(
  value: unknown,
  index: number,
  existing: Set<string>,
): string {
  const raw =
    typeof value === 'string' && value.trim()
      ? value.trim()
      : `bloom-${index + 1}`
  const base = legacyBloomIds.has(raw) ? `bloom-${index + 1}` : raw
  let candidate = base
  let suffix = 2
  while (existing.has(candidate)) {
    candidate = `${base}-${suffix}`
    suffix += 1
  }
  existing.add(candidate)
  return candidate
}

function readBloom(
  input: unknown,
  index: number,
  existing: Set<string>,
): GlassBloom {
  const fallback = defaultBloom(index)
  const record =
    input && typeof input === 'object' ? (input as Record<string, unknown>) : {}
  return {
    id: normalizeBloomId(record.id, index, existing),
    hue: normalizeAccentHue(Number(record.hue ?? fallback.hue)),
    x: clampNumber(record.x, 0, 100, fallback.x),
    y: clampNumber(record.y, 0, 100, fallback.y),
    opacity: clampNumber(record.opacity, 0, 0.5, fallback.opacity),
    radius: clampNumber(record.radius, 25, 70, fallback.radius),
  }
}

export function normalizeGlassThemeParameters(
  input: unknown,
): GlassThemeParameters {
  const existing = new Set<string>()
  const records = (
    input &&
    typeof input === 'object' &&
    Array.isArray((input as Record<string, unknown>).blooms)
      ? ((input as Record<string, unknown>).blooms as unknown[])
      : defaultGlassThemeParameters.blooms
  ).slice(0, maxGlassBloomCount)
  const blooms = records.map((record, index) =>
    readBloom(record, index, existing),
  )
  if (blooms.length >= minGlassBloomCount) {
    return { blooms }
  }
  return {
    blooms: [readBloom(defaultGlassThemeParameters.blooms[0], 0, existing)],
  }
}

export function updateGlassBloom(
  parameters: GlassThemeParameters,
  bloomId: GlassBloomId,
  patch: GlassBloomPatch,
): GlassThemeParameters {
  return normalizeGlassThemeParameters({
    blooms: parameters.blooms.map((bloom) =>
      bloom.id === bloomId ? { ...bloom, ...patch } : bloom,
    ),
  })
}

export function createGlassBloom(
  parameters: GlassThemeParameters,
  patch: GlassBloomPatch = {},
): GlassBloom {
  const index = parameters.blooms.length
  const existing = new Set(parameters.blooms.map((bloom) => bloom.id))
  const fallback = defaultBloom(index)
  return readBloom(
    {
      id: `bloom-${index + 1}`,
      hue: patch.hue ?? fallback.hue,
      x: patch.x ?? 50,
      y: patch.y ?? 50,
      opacity: patch.opacity ?? fallback.opacity,
      radius: patch.radius ?? fallback.radius,
    },
    index,
    existing,
  )
}

export function appendGlassBloom(
  parameters: GlassThemeParameters,
  patch?: GlassBloomPatch,
): { parameters: GlassThemeParameters; bloom: GlassBloom } {
  if (parameters.blooms.length >= maxGlassBloomCount) {
    const normalized = normalizeGlassThemeParameters(parameters)
    return {
      bloom: normalized.blooms[normalized.blooms.length - 1],
      parameters: normalized,
    }
  }
  const bloom = createGlassBloom(parameters, patch)
  return {
    bloom,
    parameters: normalizeGlassThemeParameters({
      blooms: [...parameters.blooms, bloom],
    }),
  }
}

export function removeGlassBloom(
  parameters: GlassThemeParameters,
  bloomId: GlassBloomId,
): GlassThemeParameters {
  if (parameters.blooms.length <= minGlassBloomCount) {
    return normalizeGlassThemeParameters(parameters)
  }
  return normalizeGlassThemeParameters({
    blooms: parameters.blooms.filter((bloom) => bloom.id !== bloomId),
  })
}

export function glassBloomColor(
  bloom: GlassBloom,
  mode: ResolvedThemeMode,
): string {
  const lightness = mode === 'dark' ? 0.58 : 0.72
  const chroma = mode === 'dark' ? 0.18 : 0.15
  return `oklch(${lightness} ${chroma} ${normalizeAccentHue(bloom.hue)} / ${bloom.opacity})`
}

export function glassBloomDisplayColor(bloom: GlassBloom): string {
  return `oklch(0.68 0.17 ${normalizeAccentHue(bloom.hue)})`
}

export function glassMeshBackground(
  parameters: GlassThemeParameters,
  mode: ResolvedThemeMode,
): string {
  const base =
    mode === 'dark'
      ? 'linear-gradient(180deg, #0a0812 0%, #050410 100%)'
      : 'linear-gradient(180deg, #f0e8ff 0%, #e8f0ff 100%)'
  const gradients = parameters.blooms.map((bloom) => {
    return `radial-gradient(circle at ${bloom.x}% ${bloom.y}%, ${glassBloomColor(
      bloom,
      mode,
    )} 0%, transparent ${bloom.radius}%)`
  })
  return [...gradients, base].join(', ')
}

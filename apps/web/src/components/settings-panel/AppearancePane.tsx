import {
  Check,
  Monitor,
  Moon,
  Paintbrush,
  Plus,
  Sun,
  Trash2,
} from 'lucide-react'
import { useRef, useState, type PointerEvent } from 'react'

import {
  accentColor,
  glassBloomDisplayColor,
  glassMeshBackground,
  maxGlassBloomCount,
  minGlassBloomCount,
  palettePresetIds,
  palettePresets,
  themeModes,
  uiDensities,
  type GlassBloom,
  type GlassBloomId,
  type PalettePresetId,
  type ThemeMode,
  type UiDensity,
} from '@/design'
import { useDesignTheme } from '@/hooks/useDesignTheme'
import { cn } from '@/lib/utils'
import { SettingsPage, SettingsPageHeader, SettingsSection } from './shared'

const themeModeLabels = {
  dark: 'Dark',
  light: 'Light',
  system: 'System',
} as const satisfies Record<ThemeMode, string>

const themeModeIcons = {
  dark: Moon,
  light: Sun,
  system: Monitor,
} as const satisfies Record<ThemeMode, typeof Moon>

const densityLabels = {
  compact: 'Compact',
  cozy: 'Cozy',
  comfortable: 'Comfortable',
} as const satisfies Record<UiDensity, string>

const paletteSwatches = {
  neutral: [
    'oklch(0.22 0.008 60)',
    'oklch(0.34 0.06 250)',
    'oklch(0.68 0.17 45)',
  ],
  paperInk: [
    'oklch(0.985 0.005 80)',
    'oklch(0.20 0.01 60)',
    'oklch(0.62 0.14 25)',
  ],
  brutalist: ['oklch(0.98 0 0)', 'oklch(0.12 0 0)', 'oklch(0.68 0.17 45)'],
  glass: [
    'oklch(0.27 0.10 286)',
    'oklch(0.67 0.14 318)',
    'oklch(0.72 0.13 205)',
  ],
  acid: ['oklch(0.10 0 0)', 'oklch(0.82 0.25 135)', 'oklch(0.96 0.01 125)'],
  marzipan: [
    'oklch(0.96 0.035 75)',
    'oklch(0.84 0.08 35)',
    'oklch(0.72 0.11 320)',
  ],
  botanical: [
    'oklch(0.25 0.055 150)',
    'oklch(0.92 0.035 115)',
    'oklch(0.68 0.08 145)',
  ],
} as const satisfies Record<PalettePresetId, readonly [string, string, string]>

const hueGradient =
  'linear-gradient(90deg, oklch(0.68 0.17 0), oklch(0.68 0.17 45), oklch(0.68 0.17 90), oklch(0.68 0.17 145), oklch(0.68 0.17 205), oklch(0.68 0.17 260), oklch(0.68 0.17 315), oklch(0.68 0.17 360))'

function bloomColor(bloom: GlassBloom): string {
  return glassBloomDisplayColor(bloom)
}

function SliderRow({
  label,
  max,
  min,
  step = 1,
  value,
  onChange,
  suffix = '',
}: {
  label: string
  max: number
  min: number
  step?: number
  value: number
  onChange: (value: number) => void
  suffix?: string
}) {
  return (
    <label className="grid gap-1.5">
      <span className="flex items-center justify-between text-[12px] text-muted-foreground">
        <span>{label}</span>
        <span className="font-mono">
          {Number.isInteger(value) ? value : value.toFixed(2)}
          {suffix}
        </span>
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
        className="ph-hue-range h-4 w-full cursor-pointer appearance-none rounded-full border border-border-soft bg-bg-elev accent-primary"
      />
    </label>
  )
}

function GlassMeshEditor() {
  const theme = useDesignTheme()
  const meshRef = useRef<HTMLDivElement>(null)
  const [selectedBloomId, setSelectedBloomId] = useState<GlassBloomId>(
    theme.glassTheme.blooms[0]?.id ?? 'bloom-1',
  )
  const selectedBloomIndex = Math.max(
    0,
    theme.glassTheme.blooms.findIndex((bloom) => bloom.id === selectedBloomId),
  )
  const selectedBloom =
    theme.glassTheme.blooms.find((bloom) => bloom.id === selectedBloomId) ??
    theme.glassTheme.blooms[0]
  if (!selectedBloom) {
    return null
  }
  const selectedBloomColor = bloomColor(selectedBloom)
  const canAddBloom = theme.glassTheme.blooms.length < maxGlassBloomCount
  const canRemoveBloom = theme.glassTheme.blooms.length > minGlassBloomCount

  function updateSelectedBloom(
    patch: Parameters<typeof theme.setGlassBloom>[1],
  ) {
    theme.setGlassBloom(selectedBloom.id, patch)
  }

  function updateBloomPosition(
    event: PointerEvent<HTMLElement>,
    bloomId = selectedBloom.id,
  ) {
    const rect = meshRef.current?.getBoundingClientRect()
    if (!rect) {
      return
    }
    const x = ((event.clientX - rect.left) / rect.width) * 100
    const y = ((event.clientY - rect.top) / rect.height) * 100
    theme.setGlassBloom(bloomId, { x, y })
  }

  function handleAddBloom() {
    if (!canAddBloom) {
      return
    }
    const bloomId = theme.addGlassBloom({
      hue: selectedBloom.hue,
      x: 50,
      y: 50,
      opacity: selectedBloom.opacity,
      radius: selectedBloom.radius,
    })
    setSelectedBloomId(bloomId)
  }

  function handleRemoveBloom() {
    if (!canRemoveBloom) {
      return
    }
    const remaining = theme.glassTheme.blooms.filter(
      (bloom) => bloom.id !== selectedBloom.id,
    )
    const nextBloom =
      remaining[Math.min(selectedBloomIndex, remaining.length - 1)]
    if (nextBloom) {
      setSelectedBloomId(nextBloom.id)
    }
    theme.removeGlassBloom(selectedBloom.id)
  }

  return (
    <SettingsSection title="Glass mesh">
      <div className="grid gap-4 lg:grid-cols-[minmax(240px,320px)_1fr]">
        <div
          ref={meshRef}
          role="application"
          aria-label="Glass bloom positions"
          className="relative aspect-[4/3] min-h-[190px] overflow-hidden rounded-lg border border-border-soft shadow-[var(--shadow-pane)]"
          style={{
            background: glassMeshBackground(
              theme.glassTheme,
              theme.resolvedMode,
            ),
          }}
          onPointerDown={(event) => {
            event.currentTarget.setPointerCapture(event.pointerId)
            updateBloomPosition(event)
          }}
          onPointerMove={(event) => {
            if (event.buttons === 1) {
              updateBloomPosition(event)
            }
          }}
        >
          <div className="absolute inset-0 backdrop-blur-[2px]" />
          {theme.glassTheme.blooms.map((bloom, index) => {
            const isSelected = bloom.id === selectedBloom.id
            return (
              <button
                key={bloom.id}
                type="button"
                title={`Bloom ${index + 1}`}
                onPointerDown={(event) => {
                  event.stopPropagation()
                  event.currentTarget.setPointerCapture(event.pointerId)
                  setSelectedBloomId(bloom.id)
                  updateBloomPosition(event, bloom.id)
                }}
                onPointerMove={(event) => {
                  if (event.buttons === 1) {
                    updateBloomPosition(event, bloom.id)
                  }
                }}
                onClick={(event) => {
                  event.stopPropagation()
                  setSelectedBloomId(bloom.id)
                }}
                className={cn(
                  'ph-focus-ring absolute size-7 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 shadow-[0_4px_12px_rgb(0_0_0/0.24)] transition-transform',
                  isSelected
                    ? 'scale-110 border-white'
                    : 'border-white/55 hover:scale-105',
                )}
                style={{
                  backgroundColor: bloomColor(bloom),
                  left: `${bloom.x}%`,
                  top: `${bloom.y}%`,
                }}
              />
            )
          })}
        </div>

        <div className="min-w-0 space-y-4">
          <div className="flex items-start gap-3">
            <div className="min-w-0 flex-1">
              <p className="text-[13px] font-medium text-foreground">
                Bloom {selectedBloomIndex + 1}
              </p>
              <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
                Drag the handle to position it, then tune color, intensity, and
                spread.
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              <button
                type="button"
                title="Add bloom"
                disabled={!canAddBloom}
                onClick={handleAddBloom}
                className="ph-focus-ring flex size-8 items-center justify-center rounded-md border text-primary-foreground transition-colors disabled:opacity-35"
                style={{
                  backgroundColor: selectedBloomColor,
                  borderColor: selectedBloomColor,
                }}
              >
                <Plus size={15} strokeWidth={1.8} />
              </button>
              <button
                type="button"
                title="Delete bloom"
                disabled={!canRemoveBloom}
                onClick={handleRemoveBloom}
                className="ph-focus-ring flex size-8 items-center justify-center rounded-md border bg-background/55 transition-colors disabled:opacity-35"
                style={{
                  borderColor: selectedBloomColor,
                  color: selectedBloomColor,
                }}
              >
                <Trash2 size={15} strokeWidth={1.8} />
              </button>
            </div>
          </div>

          <div className="grid grid-cols-4 gap-2 sm:grid-cols-8">
            {theme.glassTheme.blooms.map((bloom, index) => {
              const isSelected = bloom.id === selectedBloom.id
              const color = bloomColor(bloom)
              return (
                <button
                  key={bloom.id}
                  type="button"
                  title={`Bloom ${index + 1}`}
                  onClick={() => setSelectedBloomId(bloom.id)}
                  className={cn(
                    'ph-focus-ring h-8 rounded-md border text-[11px] font-semibold transition-colors',
                    isSelected ? 'text-primary-foreground' : 'bg-background/45',
                  )}
                  style={{
                    backgroundColor: isSelected ? color : undefined,
                    borderColor: color,
                    color: isSelected ? 'var(--primary-foreground)' : color,
                  }}
                >
                  {index + 1}
                </button>
              )
            })}
          </div>

          <div className="grid gap-3 sm:grid-cols-2">
            <SliderRow
              label="Hue"
              min={0}
              max={359}
              value={selectedBloom.hue}
              onChange={(hue) => updateSelectedBloom({ hue })}
              suffix="°"
            />
            <SliderRow
              label="Intensity"
              min={0}
              max={0.5}
              step={0.01}
              value={selectedBloom.opacity}
              onChange={(opacity) => updateSelectedBloom({ opacity })}
            />
            <SliderRow
              label="Radius"
              min={25}
              max={70}
              value={selectedBloom.radius}
              onChange={(radius) => updateSelectedBloom({ radius })}
              suffix="%"
            />
          </div>
        </div>
      </div>
    </SettingsSection>
  )
}

export function AppearancePane() {
  const theme = useDesignTheme()
  const activeAccent = accentColor(theme.accentHue)

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Appearance"
        description="Choose the built-in theme, color mode, and interface density."
      />

      <div>
        <SettingsSection title="Theme">
          <div className="grid gap-2 sm:grid-cols-2">
            {palettePresetIds.map((presetId) => {
              const preset = palettePresets[presetId]
              const isActive = theme.palettePreset === presetId
              return (
                <button
                  key={presetId}
                  type="button"
                  onClick={() => theme.setPalettePreset(presetId)}
                  className={cn(
                    'ph-focus-ring flex min-h-[74px] items-center gap-3 rounded-lg border bg-bg-elev/45 px-3 py-3 text-left transition-colors',
                    isActive
                      ? 'border-ring bg-bg-elev shadow-[0_0_0_2px_var(--focus-soft)]'
                      : 'border-border-soft hover:border-border',
                  )}
                >
                  <span className="flex h-10 w-14 shrink-0 overflow-hidden rounded-md border border-border-soft bg-background">
                    {paletteSwatches[presetId].map((color) => (
                      <span
                        key={color}
                        className="min-w-0 flex-1"
                        style={{ backgroundColor: color }}
                      />
                    ))}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block text-[13px] font-medium text-foreground">
                      {preset.label}
                    </span>
                    <span className="mt-0.5 block text-[12px] leading-5 text-muted-foreground">
                      {preset.description}
                    </span>
                  </span>
                  {isActive && (
                    <Check
                      size={16}
                      strokeWidth={1.8}
                      className="shrink-0 text-ring"
                    />
                  )}
                </button>
              )
            })}
          </div>
        </SettingsSection>

        <SettingsSection title="Accent">
          <div className="flex min-h-[72px] flex-col gap-4">
            <div className="flex items-center gap-3">
              <span
                className="flex size-11 shrink-0 items-center justify-center rounded-lg border border-border-soft shadow-[var(--shadow-pane)]"
                style={{ backgroundColor: activeAccent }}
              >
                <Paintbrush
                  size={17}
                  strokeWidth={1.7}
                  className="text-primary-foreground"
                />
              </span>
              <div className="min-w-0 flex-1">
                <p className="text-[13px] font-medium text-foreground">
                  App color
                </p>
                <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
                  Hue is adjustable; contrast and saturation stay within the app
                  range.
                </p>
              </div>
              <span className="font-mono text-[11px] text-muted-foreground">
                {theme.accentHue}°
              </span>
            </div>

            <label className="block">
              <span className="sr-only">Accent hue</span>
              <input
                type="range"
                min={0}
                max={359}
                step={1}
                value={theme.accentHue}
                onChange={(event) =>
                  theme.setAccentHue(Number(event.target.value))
                }
                className="ph-hue-range h-4 w-full cursor-pointer appearance-none rounded-full border border-border-soft bg-transparent accent-primary"
                style={{ background: hueGradient }}
              />
            </label>
          </div>
        </SettingsSection>

        {theme.palettePreset === 'glass' && <GlassMeshEditor />}

        <SettingsSection title="Mode">
          <div className="inline-flex rounded-lg border border-border-soft bg-bg-elev/45 p-1">
            {themeModes.map((mode) => {
              const Icon = themeModeIcons[mode]
              const isActive = theme.mode === mode
              return (
                <button
                  key={mode}
                  type="button"
                  onClick={() => theme.setMode(mode)}
                  className={cn(
                    'ph-focus-ring inline-flex h-8 items-center gap-1.5 rounded-md px-3 text-[12px] font-medium transition-colors',
                    isActive
                      ? 'bg-background text-foreground shadow-sm'
                      : 'text-muted-foreground hover:bg-background/60 hover:text-foreground',
                  )}
                >
                  <Icon size={14} strokeWidth={1.6} />
                  {themeModeLabels[mode]}
                </button>
              )
            })}
          </div>
        </SettingsSection>

        <SettingsSection title="Density">
          <div className="inline-flex rounded-lg border border-border-soft bg-bg-elev/45 p-1">
            {uiDensities.map((density) => {
              const isActive = theme.density === density
              return (
                <button
                  key={density}
                  type="button"
                  onClick={() => theme.setDensity(density)}
                  className={cn(
                    'ph-focus-ring h-8 rounded-md px-3 text-[12px] font-medium transition-colors',
                    isActive
                      ? 'bg-background text-foreground shadow-sm'
                      : 'text-muted-foreground hover:bg-background/60 hover:text-foreground',
                  )}
                >
                  {densityLabels[density]}
                </button>
              )
            })}
          </div>
        </SettingsSection>
      </div>
    </SettingsPage>
  )
}

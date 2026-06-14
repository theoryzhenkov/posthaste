import { Check, Paintbrush } from 'lucide-react'

import {
  accentColor,
  palettePresetIds,
  palettePresets,
  themeModes,
  uiDensities,
} from '@/design'
import { cn } from '@/lib/utils'
import type { useDesignTheme } from '@/hooks/useDesignTheme'

import { SettingsSection } from '../shared'
import {
  densityLabels,
  hueGradient,
  paletteSwatches,
  themeModeIcons,
  themeModeLabels,
} from './constants'

type DesignTheme = ReturnType<typeof useDesignTheme>

export function ThemePresetSection({ theme }: { theme: DesignTheme }) {
  return (
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
  )
}

export function AccentSection({ theme }: { theme: DesignTheme }) {
  const activeAccent = accentColor(theme.accentHue)
  return (
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
            <p className="text-[13px] font-medium text-foreground">App color</p>
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
            onChange={(event) => theme.setAccentHue(Number(event.target.value))}
            className="ph-hue-range h-4 w-full cursor-pointer appearance-none rounded-full border border-border-soft bg-transparent accent-primary"
            style={{ background: hueGradient }}
          />
        </label>
      </div>
    </SettingsSection>
  )
}

export function ModeSection({ theme }: { theme: DesignTheme }) {
  return (
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
  )
}

export function DensitySection({ theme }: { theme: DesignTheme }) {
  return (
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
  )
}

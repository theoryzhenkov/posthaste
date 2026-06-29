import { Check } from 'lucide-react'

import {
  accentColor,
  builtInThemeIds,
  builtInThemes,
  resolvedThemeModes,
  themeModes,
  uiDensities,
  type ResolvedThemeMode,
} from '@/design'
import { cn } from '@/lib/utils'
import type { useDesignTheme } from '@/hooks/useDesignTheme'

import { SettingsSection } from '../shared'
import {
  densityLabels,
  hueGradient,
  themeModeIcons,
  themeModeLabels,
  themeSwatches,
} from './constants'

type DesignTheme = ReturnType<typeof useDesignTheme>

const modeLabels: Record<ResolvedThemeMode, string> = {
  light: 'Light',
  dark: 'Dark',
}

export function ThemeSection({ theme }: { theme: DesignTheme }) {
  return (
    <SettingsSection title="Theme">
      <div className="grid gap-2 sm:grid-cols-2">
        {builtInThemeIds.map((themeId) => {
          const definition = builtInThemes[themeId]
          const isActive = theme.theme === themeId
          return (
            <button
              key={themeId}
              type="button"
              onClick={() => theme.setTheme(themeId)}
              className={cn(
                'ph-focus-ring flex min-h-[74px] items-center gap-3 rounded-lg border bg-bg-elev/45 px-3 py-3 text-left transition-colors',
                isActive
                  ? 'border-ring bg-bg-elev shadow-[0_0_0_2px_var(--focus-soft)]'
                  : 'border-border-soft hover:border-border',
              )}
            >
              <span className="flex h-10 w-14 shrink-0 overflow-hidden rounded-md border border-border-soft bg-background">
                {themeSwatches[themeId].map((color) => (
                  <span
                    key={color}
                    className="min-w-0 flex-1"
                    style={{ backgroundColor: color }}
                  />
                ))}
              </span>
              <span className="min-w-0 flex-1">
                <span className="block text-[13px] font-medium text-foreground">
                  {definition.label}
                </span>
                <span className="mt-0.5 block text-[12px] leading-5 text-muted-foreground">
                  {definition.description}
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

function AccentModeRow({
  mode,
  hue,
  onChange,
}: {
  mode: ResolvedThemeMode
  hue: number
  onChange: (hue: number) => void
}) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-3">
        <span
          className="size-7 shrink-0 rounded-md border border-border-soft shadow-[var(--shadow-pane)]"
          style={{ backgroundColor: accentColor(hue) }}
        />
        <span className="flex-1 text-[12px] font-medium text-foreground">
          {modeLabels[mode]}
        </span>
        <span className="font-mono text-[11px] text-muted-foreground">
          {hue}°
        </span>
      </div>
      <label className="block">
        <span className="sr-only">{modeLabels[mode]} accent hue</span>
        <input
          type="range"
          min={0}
          max={359}
          step={1}
          value={hue}
          onChange={(event) => onChange(Number(event.target.value))}
          className="ph-hue-range h-4 w-full cursor-pointer appearance-none rounded-full border border-border-soft bg-transparent accent-primary"
          style={{ background: hueGradient }}
        />
      </label>
    </div>
  )
}

export function AccentSection({ theme }: { theme: DesignTheme }) {
  return (
    <SettingsSection title="Accent color">
      <div className="flex flex-col gap-4">
        <p className="text-[12px] leading-5 text-muted-foreground">
          The interactive/brand hue, set independently for light and dark mode.
          Contrast and saturation stay within the app range.
        </p>
        {resolvedThemeModes.map((mode) => (
          <AccentModeRow
            key={mode}
            mode={mode}
            hue={theme[mode].accentHue}
            onChange={(hue) => theme.setAccentHue(mode, hue)}
          />
        ))}
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

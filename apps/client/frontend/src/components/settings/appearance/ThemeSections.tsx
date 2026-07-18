import { useRef, useState } from 'react'
import { Check, RotateCcw } from 'lucide-react'

import {
  accentColor,
  builtInThemeIds,
  builtInThemes,
  defaultAccentHue,
  defaultSurfaceHue,
  resolvedThemeModes,
  themeModes,
  uiDensities,
  type ResolvedThemeMode,
} from '@/lib/design'
import { cn } from '@/lib/cn'
import type { useDesignTheme } from '@/lib/design/useDesignTheme'

import { SettingsSection } from '../panel/shared'
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

function HueControl({
  label,
  hue,
  defaultHue,
  onChange,
}: {
  label: string
  hue: number
  defaultHue: number
  onChange: (hue: number) => void
}) {
  // The numeric field keeps a local draft while focused so multi-digit typing
  // isn't fought by the controlled value, and commits once (on blur/Enter)
  // rather than firing a write-through PATCH per keystroke. Commit reads the
  // input's live DOM value via a ref (robust regardless of which input event
  // fired). The slider commits live (its drag is discrete + bounded).
  const inputRef = useRef<HTMLInputElement>(null)
  const [draft, setDraft] = useState<string | null>(null)
  const commit = () => {
    const digits = (inputRef.current?.value ?? '').replace(/[^0-9]/g, '')
    const next = Number(digits)
    if (digits !== '' && Number.isFinite(next)) {
      onChange(next)
    }
    setDraft(null)
  }
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <span
          className="size-6 shrink-0 rounded-md border border-border-soft shadow-[var(--shadow-pane)]"
          style={{ backgroundColor: accentColor(hue) }}
        />
        <span className="flex-1 text-[12px] font-medium text-foreground">
          {label}
        </span>
        <input
          ref={inputRef}
          type="text"
          inputMode="numeric"
          value={draft ?? String(hue)}
          onChange={(event) =>
            setDraft(event.target.value.replace(/[^0-9]/g, '').slice(0, 3))
          }
          onFocus={(event) => event.currentTarget.select()}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              commit()
              event.currentTarget.blur()
            }
          }}
          aria-label={`${label} hue`}
          className="ph-focus-ring h-7 w-14 rounded-md border border-border-soft bg-bg-elev/45 px-2 text-right font-mono text-[11px] text-foreground"
        />
        <span className="text-[11px] text-muted-foreground">°</span>
        <button
          type="button"
          disabled={hue === defaultHue}
          onClick={() => onChange(defaultHue)}
          title="Reset to default"
          aria-label={`Reset ${label} to default`}
          className="ph-focus-ring flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-background/60 hover:text-foreground disabled:opacity-35"
        >
          <RotateCcw size={13} strokeWidth={1.7} />
        </button>
      </div>
      <label className="block">
        <span className="sr-only">{label} hue slider</span>
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

function ModeColorGroup({
  theme,
  mode,
}: {
  theme: DesignTheme
  mode: ResolvedThemeMode
}) {
  // Glass derives its surface from the mesh editor, not the base surface
  // tokens, so a surface-hue control would be a no-op there — show it only for
  // the solid (Classic-style) themes.
  const showSurface = theme.theme !== 'glass'
  return (
    <div className="flex flex-col gap-3 rounded-lg border border-border-soft bg-bg-elev/35 p-3">
      <span className="text-[12px] font-semibold text-foreground">
        {modeLabels[mode]} mode
      </span>
      <HueControl
        label="Accent"
        hue={theme[mode].accentHue}
        defaultHue={defaultAccentHue}
        onChange={(hue) => theme.setAccentHue(mode, hue)}
      />
      {showSurface && (
        <HueControl
          label="Surface"
          hue={theme[mode].surfaceHue}
          defaultHue={defaultSurfaceHue}
          onChange={(hue) => theme.setSurfaceHue(mode, hue)}
        />
      )}
    </div>
  )
}

export function ColorsSection({ theme }: { theme: DesignTheme }) {
  return (
    <SettingsSection title="Colors">
      <div className="flex flex-col gap-3">
        <p className="text-[12px] leading-5 text-muted-foreground">
          Set the accent (brand) and surface (main background) hues
          independently for light and dark mode. Enter a precise hue or reset to
          the default.
        </p>
        <div className="grid gap-2 sm:grid-cols-2">
          {resolvedThemeModes.map((mode) => (
            <ModeColorGroup key={mode} theme={theme} mode={mode} />
          ))}
        </div>
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

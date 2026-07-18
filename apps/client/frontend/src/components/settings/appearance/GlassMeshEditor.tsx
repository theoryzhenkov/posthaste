import { Plus, Trash2 } from 'lucide-react'
import { useRef, useState, type PointerEvent } from 'react'

import {
  glassBloomDisplayColor,
  glassMeshBackground,
  maxGlassBloomCount,
  minGlassBloomCount,
  type GlassBloom,
  type GlassBloomId,
} from '@/lib/design'
import { useDesignTheme } from '@/lib/design/useDesignTheme'
import { cn } from '@/lib/cn'

import { SettingsSection } from '../panel/shared'
import { SliderRow } from './SliderRow'

function bloomColor(bloom: GlassBloom): string {
  return glassBloomDisplayColor(bloom)
}

export function GlassMeshEditor() {
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
            <BloomButtons
              color={selectedBloomColor}
              canAdd={canAddBloom}
              canRemove={canRemoveBloom}
              onAdd={handleAddBloom}
              onRemove={handleRemoveBloom}
            />
          </div>

          <div className="grid grid-cols-4 gap-2 sm:grid-cols-8">
            {theme.glassTheme.blooms.map((bloom, index) => (
              <BloomTab
                key={bloom.id}
                bloom={bloom}
                index={index}
                isSelected={bloom.id === selectedBloom.id}
                onSelect={() => setSelectedBloomId(bloom.id)}
              />
            ))}
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

function BloomButtons({
  color,
  canAdd,
  canRemove,
  onAdd,
  onRemove,
}: {
  color: string
  canAdd: boolean
  canRemove: boolean
  onAdd: () => void
  onRemove: () => void
}) {
  return (
    <div className="flex shrink-0 items-center gap-1.5">
      <button
        type="button"
        title="Add bloom"
        disabled={!canAdd}
        onClick={onAdd}
        className="ph-focus-ring flex size-8 items-center justify-center rounded-md border text-primary-foreground transition-colors disabled:opacity-35"
        style={{ backgroundColor: color, borderColor: color }}
      >
        <Plus size={15} strokeWidth={1.8} />
      </button>
      <button
        type="button"
        title="Delete bloom"
        disabled={!canRemove}
        onClick={onRemove}
        className="ph-focus-ring flex size-8 items-center justify-center rounded-md border bg-background/55 transition-colors disabled:opacity-35"
        style={{ borderColor: color, color }}
      >
        <Trash2 size={15} strokeWidth={1.8} />
      </button>
    </div>
  )
}

function BloomTab({
  bloom,
  index,
  isSelected,
  onSelect,
}: {
  bloom: GlassBloom
  index: number
  isSelected: boolean
  onSelect: () => void
}) {
  const color = bloomColor(bloom)
  return (
    <button
      type="button"
      title={`Bloom ${index + 1}`}
      onClick={onSelect}
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
}

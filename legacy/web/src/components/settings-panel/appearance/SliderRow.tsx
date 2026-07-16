export function SliderRow({
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

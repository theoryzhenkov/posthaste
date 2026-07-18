export function ComposeLine({
  children,
  label,
}: {
  children: React.ReactNode
  label: string
}) {
  return (
    <label className="grid grid-cols-[4rem_minmax(0,1fr)] items-center gap-2">
      <span className="text-right text-[12px] font-medium text-muted-foreground">
        {label}
      </span>
      {children}
    </label>
  )
}

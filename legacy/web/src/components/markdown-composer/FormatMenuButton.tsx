import type { FormatCommand } from './formattingCommands'

export function FormatMenuButton({
  command,
  onClick,
}: {
  command: FormatCommand
  onClick: () => void
}) {
  const Icon = command.icon
  return (
    <button
      type="button"
      className="flex size-8 items-center justify-center rounded text-muted-foreground hover:bg-[var(--hover-bg)] hover:text-foreground"
      title={command.label}
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
    >
      <Icon size={15} />
    </button>
  )
}

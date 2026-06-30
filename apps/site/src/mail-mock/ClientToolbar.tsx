import { Archive, Flag } from 'lucide-react'

export function ClientToolbar({
  canArchive,
  isFlagged,
  isMessageSelected,
  onArchive,
  onToggleFlag,
}: {
  canArchive: boolean
  isFlagged: boolean
  isMessageSelected: boolean
  onArchive: () => void
  onToggleFlag: () => void
}) {
  return (
    <nav className="client-toolbar" aria-label="Primary">
      <div className="traffic-lights" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
      <button
        type="button"
        className="toolbar-action"
        disabled={!canArchive}
        onClick={onArchive}
        title="Archive"
      >
        <Archive aria-hidden="true" />
      </button>
      <button
        type="button"
        className={`toolbar-action ${isFlagged ? 'active' : ''}`}
        disabled={!isMessageSelected}
        onClick={onToggleFlag}
        title="Flag"
      >
        <Flag aria-hidden="true" />
      </button>
    </nav>
  )
}

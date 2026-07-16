import { Folder, Plus } from 'lucide-react'

import { Button } from '../../ui/button'
import { SettingsEmptyState } from '../shared'

export function SmartMailboxesEmptyState({
  onCreateMailbox,
}: {
  onCreateMailbox: () => void
}) {
  return (
    <SettingsEmptyState
      icon={<Folder size={36} strokeWidth={1.5} />}
      title="No mailbox selected"
      description="Pick a mailbox from the list, or create a smart mailbox."
      action={
        <Button
          size="sm"
          variant="outline"
          type="button"
          onClick={onCreateMailbox}
          className="rounded-md border-border bg-bg-elev"
        >
          <Plus size={13} strokeWidth={2} />
          New smart mailbox
        </Button>
      }
    />
  )
}

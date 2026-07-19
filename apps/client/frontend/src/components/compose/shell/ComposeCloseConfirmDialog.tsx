import type { ComposeIntent } from '@/domain/composeIntent'
import { SurfaceConfirm } from '@/components/ui/overlay/surface-confirm'
import { Button } from '@/components/ui/form/button'

import { composeCloseCopy } from './composeCloseGuard'

/**
 * Close-without-send confirmation. Shown when the user closes a dirty compose
 * (X / Escape / click-away / footer Close). Rendered INSIDE the compose
 * surface — the scrim covers only the composer, the rest of the app stays
 * interactive, and the panel's header (move/pin) keeps working. Three
 * actions: keep editing (also Escape / scrim click), discard the unsaved
 * content, or save it as a draft.
 */
export function ComposeCloseConfirmDialog({
  open,
  intentKind,
  onKeepEditing,
  onDiscard,
  onSaveAsDraft,
}: {
  open: boolean
  intentKind: ComposeIntent['kind']
  onKeepEditing: () => void
  onDiscard: () => void
  onSaveAsDraft: () => void
}) {
  const copy = composeCloseCopy(intentKind)
  return (
    <SurfaceConfirm
      open={open}
      title={copy.title}
      description={copy.description}
      onDismiss={onKeepEditing}
    >
      <Button variant="outline" onClick={onKeepEditing}>
        Keep editing
      </Button>
      <Button variant="ghost" onClick={onDiscard}>
        Discard
      </Button>
      <Button className="col-span-2" onClick={onSaveAsDraft}>
        {copy.saveLabel}
      </Button>
    </SurfaceConfirm>
  )
}

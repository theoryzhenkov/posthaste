import { useCallback, useState, type RefObject } from 'react'
import { toast } from 'sonner'

import type { ComposeIntent } from '@/domain/composeIntent'
import { shouldCloseOriginalComposeAfterWindowOpen } from '@/components/compose/hooks/composeWindowElevation'
import { openSurfaceInSeparateWindow } from '@/desktop/runtime'
import { composeSurface } from '@/surfaces'

export function useComposeWindowElevation({
  editedResetKeyRef,
  formResetKey,
  intent,
  onClose,
}: {
  editedResetKeyRef: RefObject<string | null>
  formResetKey: string
  intent: ComposeIntent
  onClose: () => void
}) {
  const [isOpeningWindow, setIsOpeningWindow] = useState(false)
  const openInitialComposeInWindow = useCallback(() => {
    const openingResetKey = formResetKey
    setIsOpeningWindow(true)
    void openSurfaceInSeparateWindow(composeSurface(intent))
      .then(() => {
        if (
          shouldCloseOriginalComposeAfterWindowOpen({
            openingResetKey,
            lastEditedResetKey: editedResetKeyRef.current,
          })
        ) {
          onClose()
        }
      })
      .catch((error: unknown) => {
        toast.error(
          error instanceof Error ? error.message : 'Failed to open window',
        )
      })
      .finally(() => setIsOpeningWindow(false))
  }, [editedResetKeyRef, formResetKey, intent, onClose])

  return { isOpeningWindow, openInitialComposeInWindow }
}

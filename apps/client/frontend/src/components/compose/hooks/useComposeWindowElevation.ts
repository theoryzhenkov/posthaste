import { useCallback, useState, type RefObject } from 'react'
import { toast } from 'sonner'

import type { ComposeIntent } from '@/domain/composeIntent'
import { shouldCloseOriginalComposeAfterWindowOpen } from '@/components/compose/hooks/composeWindowElevation'
import { usePlatformServices } from '@/lib/platform/services'
import { composeSurface } from '@/domain/surface'

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
  const { openSurfaceInSeparateWindow } = usePlatformServices()
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
  }, [editedResetKeyRef, formResetKey, intent, onClose, openSurfaceInSeparateWindow])

  return { isOpeningWindow, openInitialComposeInWindow }
}

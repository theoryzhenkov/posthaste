import { useEffect, type RefObject } from 'react'

export function usePanelDismissal({
  closeIgnoreSelector,
  isPinned,
  onClose,
  panelRef,
}: {
  closeIgnoreSelector: string | undefined
  isPinned: boolean
  onClose: () => void
  panelRef: RefObject<HTMLDivElement | null>
}) {
  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented) {
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        onClose()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [onClose])

  useEffect(() => {
    function handlePointerDown(event: PointerEvent) {
      if (isPinned) {
        return
      }
      const target = event.target
      if (!(target instanceof Node)) {
        return
      }
      if (panelRef.current?.contains(target)) {
        return
      }
      if (
        closeIgnoreSelector &&
        target instanceof Element &&
        target.closest(closeIgnoreSelector)
      ) {
        return
      }
      onClose()
    }

    window.addEventListener('pointerdown', handlePointerDown, true)
    return () =>
      window.removeEventListener('pointerdown', handlePointerDown, true)
  }, [closeIgnoreSelector, isPinned, onClose, panelRef])
}

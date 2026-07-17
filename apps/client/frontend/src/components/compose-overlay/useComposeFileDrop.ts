import { useRef, useState, type DragEvent as ReactDragEvent } from 'react'

import { filesFromDataTransfer } from './attachments'

/**
 * Drag-and-drop file ingestion for the composer: dropping files anywhere on
 * the compose surface attaches them through the same ingestion path as the
 * picker and paste. While a file drag hovers the composer, `isDragActive`
 * drives the drop affordance overlay; the depth counter keeps it up while the
 * drag crosses child elements (each fires its own enter/leave pair).
 *
 * Pass `ingest: undefined` to disable (e.g. while sending) — drags are then
 * ignored entirely.
 */
export function useComposeFileDrop(
  ingest: ((files: File[]) => void) | undefined,
) {
  const [isDragActive, setIsDragActive] = useState(false)
  const dragDepthRef = useRef(0)
  const isFileDrag = (event: ReactDragEvent) =>
    Boolean(event.dataTransfer?.types.includes('Files'))

  const onDragEnter = (event: ReactDragEvent) => {
    if (!isFileDrag(event) || !ingest) {
      return
    }
    event.preventDefault()
    dragDepthRef.current += 1
    setIsDragActive(true)
  }
  const onDragLeave = (event: ReactDragEvent) => {
    if (!isFileDrag(event)) {
      return
    }
    dragDepthRef.current = Math.max(0, dragDepthRef.current - 1)
    if (dragDepthRef.current === 0) {
      setIsDragActive(false)
    }
  }
  const onDragOver = (event: ReactDragEvent) => {
    if (!isFileDrag(event) || !ingest) {
      return
    }
    // Signal a valid drop target so the browser doesn't navigate to the file.
    event.preventDefault()
  }
  const onDrop = (event: ReactDragEvent) => {
    dragDepthRef.current = 0
    setIsDragActive(false)
    // The body editor's own drop hook may have consumed the event already.
    if (event.defaultPrevented || !ingest) {
      return
    }
    const files = filesFromDataTransfer(event.dataTransfer)
    if (files.length === 0) {
      return
    }
    event.preventDefault()
    ingest(files)
  }

  return {
    isDragActive,
    dropZoneProps: { onDragEnter, onDragLeave, onDragOver, onDrop },
  }
}

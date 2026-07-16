/**
 * Paste (Cmd+V) and drag-and-drop file ingestion for the composer:
 * `filesFromDataTransfer` (the shared clipboard/drag extraction),
 * `withPastedFileName` (naming unnamed pasted images), the ComposeFields
 * paste handler (files → attachments, text pastes untouched), and the
 * `useComposeFileDrop` drop-zone hook (affordance + ingestion).
 */
import { describe, expect, it } from 'bun:test'
import type { DragEvent as ReactDragEvent } from 'react'
import { act, fireEvent, render, renderHook } from '@testing-library/react'

import {
  filesFromDataTransfer,
  withPastedFileName,
} from '../src/components/compose-overlay/attachments'
import { ComposeFields } from '../src/components/compose-overlay/ComposeFields'
import { useComposeFileDrop } from '../src/components/compose-overlay/useComposeFileDrop'
import { EMPTY_FORM } from '../src/components/composeFormHelpers'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function pngFile(name = 'shot.png'): File {
  return new File([new Uint8Array([137, 80, 78, 71])], name, {
    type: 'image/png',
  })
}

describe('filesFromDataTransfer', () => {
  it('returns the files of a file-carrying transfer', () => {
    const file = pngFile()
    const data = { files: [file], items: [] } as unknown as DataTransfer
    expect(filesFromDataTransfer(data)).toEqual([file])
  })

  it('falls back to file-kind items when `files` is empty (some clipboard sources)', () => {
    const file = pngFile()
    const data = {
      files: [],
      items: [
        { kind: 'string', getAsFile: () => null },
        { kind: 'file', getAsFile: () => file },
      ],
    } as unknown as DataTransfer
    expect(filesFromDataTransfer(data)).toEqual([file])
  })

  it('returns nothing for a text-only transfer (plain paste stays untouched)', () => {
    const data = {
      files: [],
      items: [{ kind: 'string', getAsFile: () => null }],
    } as unknown as DataTransfer
    expect(filesFromDataTransfer(data)).toEqual([])
    expect(filesFromDataTransfer(null)).toEqual([])
  })
})

describe('withPastedFileName', () => {
  it('keeps a named file untouched', () => {
    const file = pngFile('named.png')
    expect(withPastedFileName(file, 3)).toBe(file)
  })

  it('names an unnamed image from its MIME subtype', () => {
    const named = withPastedFileName(
      new File([new Uint8Array([1])], '', { type: 'image/png' }),
      2,
    )
    expect(named.name).toBe('pasted-image-2.png')
    expect(named.type).toBe('image/png')
  })

  it('names unnamed non-images pasted-file-<n>', () => {
    const named = withPastedFileName(
      new File([new Uint8Array([1])], '', { type: '' }),
      1,
    )
    expect(named.name).toBe('pasted-file-1')
    expect(named.type).toBe('application/octet-stream')
  })
})

describe('ComposeFields paste', () => {
  function renderFields(onPasteFiles: (files: File[]) => void) {
    return render(
      <ComposeFields
        displayedFromOptions={[]}
        fieldsDisabled={false}
        form={EMPTY_FORM}
        fromInputFocused={false}
        fromMenuOpen={false}
        intentKind="new"
        recipientSuggestions={[]}
        setFromInputFocused={() => {}}
        setFromMenuOpen={() => {}}
        onFieldChange={() => {}}
        onPasteFiles={onPasteFiles}
      />,
    )
  }

  it('a ClipboardEvent carrying a file becomes an attachment (default prevented)', () => {
    const pasted: File[][] = []
    const { container } = renderFields((files) => pasted.push(files))
    const file = pngFile()
    const notPrevented = fireEvent.paste(container.firstElementChild!, {
      clipboardData: { files: [file], items: [] },
    })
    expect(pasted).toEqual([[file]])
    // preventDefault was called — the paste is consumed as an attachment.
    expect(notPrevented).toBe(false)
  })

  it('a text-only paste is left entirely to the inputs', () => {
    const pasted: File[][] = []
    const { container } = renderFields((files) => pasted.push(files))
    const notPrevented = fireEvent.paste(container.firstElementChild!, {
      clipboardData: {
        files: [],
        items: [{ kind: 'string', getAsFile: () => null }],
      },
    })
    expect(pasted).toEqual([])
    expect(notPrevented).toBe(true)
  })
})

describe('useComposeFileDrop', () => {
  function dragEvent(
    types: string[],
    files: File[] = [],
    { defaultPrevented = false } = {},
  ) {
    const event = {
      dataTransfer: { types, files, items: [] },
      defaultPrevented,
      preventDefault: () => {
        event.defaultPrevented = true
      },
    }
    return event as unknown as ReactDragEvent & { defaultPrevented: boolean }
  }

  it('shows the affordance across nested enter/leave pairs, ingests on drop', () => {
    const dropped: File[][] = []
    const { result } = renderHook(() =>
      useComposeFileDrop((files) => dropped.push(files)),
    )
    act(() => {
      result.current.dropZoneProps.onDragEnter(dragEvent(['Files']))
      result.current.dropZoneProps.onDragEnter(dragEvent(['Files']))
      result.current.dropZoneProps.onDragLeave(dragEvent(['Files']))
    })
    // Still over a child element — the overlay stays up.
    expect(result.current.isDragActive).toBe(true)

    const file = pngFile()
    const drop = dragEvent(['Files'], [file])
    act(() => {
      result.current.dropZoneProps.onDrop(drop)
    })
    expect(dropped).toEqual([[file]])
    expect(drop.defaultPrevented).toBe(true)
    expect(result.current.isDragActive).toBe(false)
  })

  it('ignores a drop the body editor already consumed (no double ingestion)', () => {
    const dropped: File[][] = []
    const { result } = renderHook(() =>
      useComposeFileDrop((files) => dropped.push(files)),
    )
    act(() => {
      result.current.dropZoneProps.onDrop(
        dragEvent(['Files'], [pngFile()], { defaultPrevented: true }),
      )
    })
    expect(dropped).toEqual([])
  })

  it('ignores text drags and everything while ingestion is disabled', () => {
    const dropped: File[][] = []
    const { result: enabled } = renderHook(() =>
      useComposeFileDrop((files) => dropped.push(files)),
    )
    const textDrag = dragEvent(['text/plain'])
    act(() => {
      enabled.current.dropZoneProps.onDragEnter(textDrag)
      enabled.current.dropZoneProps.onDrop(textDrag)
    })
    expect(enabled.current.isDragActive).toBe(false)
    expect(textDrag.defaultPrevented).toBe(false)
    expect(dropped).toEqual([])

    const { result: disabled } = renderHook(() => useComposeFileDrop(undefined))
    const fileDrag = dragEvent(['Files'], [pngFile()])
    act(() => {
      disabled.current.dropZoneProps.onDragEnter(fileDrag)
      disabled.current.dropZoneProps.onDrop(fileDrag)
    })
    expect(disabled.current.isDragActive).toBe(false)
    expect(fileDrag.defaultPrevented).toBe(false)
  })
})

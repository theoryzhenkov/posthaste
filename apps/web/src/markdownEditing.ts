export interface MarkdownEditState {
  selectionEnd: number
  selectionStart: number
  text: string
}

function normalizedSelection(selectionStart: number, selectionEnd: number) {
  return {
    end: Math.max(selectionStart, selectionEnd),
    start: Math.min(selectionStart, selectionEnd),
  }
}

export function toggleMarkdownMarker(
  state: MarkdownEditState,
  marker: string,
): MarkdownEditState {
  const { start, end } = normalizedSelection(
    state.selectionStart,
    state.selectionEnd,
  )
  const markerLength = marker.length
  const selectedText = state.text.slice(start, end)
  const hasSurroundingMarkers =
    start >= markerLength &&
    state.text.slice(start - markerLength, start) === marker &&
    state.text.slice(end, end + markerLength) === marker

  if (hasSurroundingMarkers) {
    return {
      text:
        state.text.slice(0, start - markerLength) +
        selectedText +
        state.text.slice(end + markerLength),
      selectionStart: start - markerLength,
      selectionEnd: end - markerLength,
    }
  }

  const selectionIncludesMarkers =
    selectedText.length >= markerLength * 2 &&
    selectedText.startsWith(marker) &&
    selectedText.endsWith(marker)

  if (selectionIncludesMarkers) {
    const unwrappedText = selectedText.slice(
      markerLength,
      selectedText.length - markerLength,
    )
    return {
      text: state.text.slice(0, start) + unwrappedText + state.text.slice(end),
      selectionStart: start,
      selectionEnd: start + unwrappedText.length,
    }
  }

  return {
    text:
      state.text.slice(0, start) +
      marker +
      selectedText +
      marker +
      state.text.slice(end),
    selectionStart: start + markerLength,
    selectionEnd: end + markerLength,
  }
}

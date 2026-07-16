import { EditorSelection } from '@codemirror/state'
import type { Command } from '@codemirror/view'
import {
  Bold,
  Code2,
  Italic,
  Strikethrough,
  type LucideIcon,
} from 'lucide-react'

import { toggleMarkdownMarker } from '@/markdownEditing'

export interface FormatCommand {
  icon: LucideIcon
  key: string
  label: string
  marker: string
  shortcut: string
}

export const FORMAT_COMMANDS: FormatCommand[] = [
  { icon: Bold, key: 'bold', label: 'Bold', marker: '**', shortcut: 'Mod-b' },
  {
    icon: Italic,
    key: 'italic',
    label: 'Italic',
    marker: '*',
    shortcut: 'Mod-i',
  },
  { icon: Code2, key: 'code', label: 'Code', marker: '`', shortcut: 'Mod-e' },
  {
    icon: Strikethrough,
    key: 'strike',
    label: 'Strikethrough',
    marker: '~~',
    shortcut: 'Mod-Shift-x',
  },
]

export function formatSelection(marker: string): Command {
  return (view) => {
    const source = view.state.doc.toString()
    const range = view.state.selection.main
    const next = toggleMarkdownMarker(
      {
        text: source,
        selectionStart: range.from,
        selectionEnd: range.to,
      },
      marker,
    )

    view.dispatch({
      changes: { from: 0, to: source.length, insert: next.text },
      scrollIntoView: true,
      selection:
        next.selectionStart === next.selectionEnd
          ? EditorSelection.cursor(next.selectionStart)
          : EditorSelection.range(next.selectionStart, next.selectionEnd),
      userEvent: 'input.format',
    })
    return true
  }
}

import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import { EditorSelection, EditorState, type Extension } from '@codemirror/state'
import { tags } from '@lezer/highlight'
import {
  Bold,
  Code2,
  Italic,
  Strikethrough,
  type LucideIcon,
} from 'lucide-react'
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from 'react'
import {
  EditorView,
  keymap,
  placeholder as editorPlaceholder,
  type Command,
  type ViewUpdate,
} from '@codemirror/view'

import { toggleMarkdownMarker } from '@/markdownEditing'
import { cn } from '@/lib/utils'

export interface MarkdownComposerEditorHandle {
  focus: () => void
}

interface MarkdownComposerEditorProps {
  className?: string
  onChange: (value: string) => void
  placeholder?: string
  value: string
}

interface ContextMenuPosition {
  x: number
  y: number
}

interface FormatCommand {
  icon: LucideIcon
  key: string
  label: string
  marker: string
  shortcut: string
}

const FORMAT_COMMANDS: FormatCommand[] = [
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

const composerTheme = EditorView.theme({
  '&': {
    background: 'transparent',
    color: 'var(--foreground)',
    height: '100%',
    minHeight: '220px',
  },
  '&.cm-focused': {
    outline: 'none',
  },
  '.cm-content': {
    caretColor: 'var(--foreground)',
    minHeight: '220px',
    padding: '16px 20px',
  },
  '.cm-line': {
    padding: '0',
  },
  '.cm-placeholder': {
    color: 'color-mix(in oklab, var(--muted-foreground) 70%, transparent)',
  },
  '.cm-scroller': {
    fontFamily: '"Geist", system-ui, sans-serif',
    fontSize: '13px',
    lineHeight: '1.6',
    overflow: 'auto',
  },
  '.cm-selectionBackground': {
    background:
      'color-mix(in oklab, var(--brand-coral) 24%, transparent) !important',
  },
})

const markdownHighlightStyle = HighlightStyle.define([
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strong, fontWeight: '700' },
  { tag: tags.strikethrough, textDecoration: 'line-through' },
  {
    tag: tags.monospace,
    background: 'color-mix(in oklab, var(--muted) 52%, transparent)',
    borderRadius: '4px',
    fontFamily: '"Geist Mono", ui-monospace, SFMono-Regular, monospace',
    padding: '0 2px',
  },
  { tag: tags.link, color: 'var(--brand-blue)', textDecoration: 'underline' },
  { tag: tags.url, color: 'var(--brand-blue)' },
  { tag: tags.heading, fontWeight: '700' },
  { tag: tags.quote, color: 'var(--muted-foreground)', fontStyle: 'italic' },
  { tag: tags.punctuation, color: 'var(--muted-foreground)' },
  { tag: tags.meta, color: 'var(--muted-foreground)' },
])

function formatSelection(marker: string): Command {
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

export const MarkdownComposerEditor = forwardRef<
  MarkdownComposerEditorHandle,
  MarkdownComposerEditorProps
>(function MarkdownComposerEditor(
  { className, onChange, placeholder, value },
  ref,
) {
  const containerRef = useRef<HTMLDivElement>(null)
  const editorRef = useRef<EditorView | null>(null)
  const initialValueRef = useRef(value)
  const [contextMenu, setContextMenu] = useState<ContextMenuPosition | null>(
    null,
  )

  const handleEditorUpdate = useCallback(
    (update: ViewUpdate) => {
      if (update.docChanged) {
        onChange(update.state.doc.toString())
      }
    },
    [onChange],
  )

  const extensions = useMemo<Extension[]>(
    () => [
      history(),
      markdown({
        addKeymap: true,
        base: markdownLanguage,
        completeHTMLTags: false,
      }),
      syntaxHighlighting(markdownHighlightStyle),
      composerTheme,
      EditorView.lineWrapping,
      EditorView.updateListener.of(handleEditorUpdate),
      keymap.of([
        ...FORMAT_COMMANDS.map((command) => ({
          key: command.shortcut,
          preventDefault: true,
          run: formatSelection(command.marker),
        })),
        ...historyKeymap,
        ...defaultKeymap,
      ]),
      placeholder ? editorPlaceholder(placeholder) : [],
    ],
    [handleEditorUpdate, placeholder],
  )

  useEffect(() => {
    if (!containerRef.current) {
      return
    }

    const view = new EditorView({
      parent: containerRef.current,
      state: EditorState.create({ doc: initialValueRef.current, extensions }),
    })
    editorRef.current = view

    return () => {
      view.destroy()
      editorRef.current = null
    }
  }, [extensions])

  useEffect(() => {
    const view = editorRef.current
    if (!view) {
      return
    }
    const currentValue = view.state.doc.toString()
    if (currentValue === value) {
      return
    }
    view.dispatch({
      changes: { from: 0, to: currentValue.length, insert: value },
    })
  }, [value])

  useImperativeHandle(
    ref,
    () => ({
      focus: () => editorRef.current?.focus(),
    }),
    [],
  )

  useEffect(() => {
    if (!contextMenu) {
      return
    }

    function closeMenu() {
      setContextMenu(null)
    }

    window.addEventListener('mousedown', closeMenu)
    window.addEventListener('keydown', closeMenu)
    window.addEventListener('scroll', closeMenu, true)
    return () => {
      window.removeEventListener('mousedown', closeMenu)
      window.removeEventListener('keydown', closeMenu)
      window.removeEventListener('scroll', closeMenu, true)
    }
  }, [contextMenu])

  function runFormat(marker: string) {
    const view = editorRef.current
    if (!view) {
      return
    }
    formatSelection(marker)(view)
    view.focus()
    setContextMenu(null)
  }

  return (
    <div
      className={cn('relative h-full min-h-[220px]', className)}
      onContextMenu={(event) => {
        event.preventDefault()
        editorRef.current?.focus()
        setContextMenu({ x: event.clientX, y: event.clientY })
      }}
    >
      <div ref={containerRef} className="h-full min-h-[220px]" />
      {contextMenu && (
        <div
          className="fixed z-[120] flex overflow-hidden rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          {FORMAT_COMMANDS.map((command) => (
            <FormatMenuButton
              key={command.key}
              command={command}
              onClick={() => runFormat(command.marker)}
            />
          ))}
        </div>
      )}
    </div>
  )
})

function FormatMenuButton({
  command,
  onClick,
}: {
  command: FormatCommand
  onClick: () => void
}) {
  const Icon = command.icon
  return (
    <button
      type="button"
      className="flex size-8 items-center justify-center rounded text-muted-foreground hover:bg-[var(--hover-bg)] hover:text-foreground"
      title={command.label}
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
    >
      <Icon size={15} />
    </button>
  )
}

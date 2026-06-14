import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import { EditorState, type Extension } from '@codemirror/state'
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
  type ViewUpdate,
} from '@codemirror/view'

import { cn } from '@/lib/utils'

import {
  FORMAT_COMMANDS,
  FormatMenuButton,
  formatSelection,
} from './markdown-composer/formatting'
import {
  composerTheme,
  markdownSyntaxHighlighting,
} from './markdown-composer/theme'

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
      markdownSyntaxHighlighting,
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

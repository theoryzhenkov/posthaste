import { HighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { tags } from '@lezer/highlight'
import { EditorView } from '@codemirror/view'

export const composerTheme = EditorView.theme({
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

export const markdownSyntaxHighlighting = syntaxHighlighting(
  markdownHighlightStyle,
)

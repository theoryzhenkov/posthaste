import { Loader2 } from 'lucide-react'
import { lazy, Suspense, type RefObject } from 'react'

import type { MarkdownComposerEditorHandle } from './MarkdownComposerEditor'

const MarkdownComposerEditor = lazy(() =>
  import('./MarkdownComposerEditor').then((module) => ({
    default: module.MarkdownComposerEditor,
  })),
)

export function ComposeBodyEditor({
  bodyRef,
  isPreparingMessage,
  isPreparingError,
  noticeLabel,
  preparingLabel,
  onChange,
  onFiles,
  value,
}: {
  bodyRef: RefObject<MarkdownComposerEditorHandle | null>
  /** Full-screen spinner: the gated content (a resumed draft / forward
   *  attachments) is still loading. A reply/forward quote does NOT gate this —
   *  the editor renders immediately and the quote streams in (FIX2). */
  isPreparingMessage: boolean
  /** Full-screen error: the gated content failed to load. */
  isPreparingError: boolean
  /** A subtle, non-blocking banner shown ABOVE the usable editor while the
   *  reply/forward quote streams in (or if it failed) — never replaces it. */
  noticeLabel: string | null
  preparingLabel: string
  onChange: (value: string) => void
  /** Files pasted (Cmd+V) into or dropped onto the body editor → attachments. */
  onFiles?: (files: File[]) => void
  value: string
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col bg-[color-mix(in_oklab,var(--background)_62%,transparent)]">
      {isPreparingMessage ? (
        <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
          <Loader2 size={16} className="animate-spin" />
          {preparingLabel}
        </div>
      ) : isPreparingError ? (
        <div className="flex h-full items-center justify-center px-6 text-center text-sm text-destructive">
          Could not prepare this message. Close the composer and try again.
        </div>
      ) : (
        <>
          {noticeLabel ? (
            <div className="flex shrink-0 items-center gap-2 px-4 py-1.5 text-xs text-muted-foreground">
              <Loader2 size={12} className="animate-spin" />
              {noticeLabel}
            </div>
          ) : null}
          <div className="min-h-0 flex-1">
            <Suspense
              fallback={
                <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
                  <Loader2 size={16} className="animate-spin" />
                  Loading editor...
                </div>
              }
            >
              <MarkdownComposerEditor
                ref={bodyRef}
                value={value}
                onChange={onChange}
                onFiles={onFiles}
                placeholder="Write Markdown"
              />
            </Suspense>
          </div>
        </>
      )}
    </div>
  )
}

import { useEffect } from 'react'
import { AlertTriangle, X } from 'lucide-react'

import {
  closeCurrentSurfaceWindow,
  listenForDesktopCloseRequest,
} from '@/desktop'

import { Button } from './ui/button'

export function InvalidSurface({
  route,
  onClose,
}: {
  route: string
  onClose: () => void
}) {
  return (
    <div
      className="flex h-full min-h-0 items-center justify-center bg-background p-6 text-foreground"
      data-posthaste-state="state.surface.invalid.ready.test"
      data-posthaste-surface-kind="invalid"
    >
      <section className="grid max-w-md gap-4 rounded-lg border border-border bg-card p-5 text-card-foreground shadow-lg">
        <div className="flex items-start gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-full bg-destructive/10 text-destructive">
            <AlertTriangle size={18} />
          </div>
          <div className="min-w-0 flex-1">
            <h1 className="text-base font-semibold">
              Surface route unavailable
            </h1>
            <p className="mt-1 text-sm text-muted-foreground">
              PostHaste could not open this focused surface route.
            </p>
          </div>
        </div>
        <code className="block max-h-28 overflow-auto rounded border border-border bg-muted/40 px-2 py-1.5 text-xs text-muted-foreground">
          {route}
        </code>
        <div className="flex justify-end">
          <Button type="button" variant="secondary" onClick={onClose}>
            <X size={14} />
            Close
          </Button>
        </div>
      </section>
    </div>
  )
}

export function InvalidSurfaceDocument({ route }: { route: string }) {
  useEffect(() => {
    let unlisten: (() => void) | null = null
    let disposed = false
    void listenForDesktopCloseRequest(() => {
      void closeCurrentSurfaceWindow()
    }).then((nextUnlisten) => {
      if (disposed) {
        nextUnlisten()
        return
      }
      unlisten = nextUnlisten
    })

    function handleKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'w') {
        event.preventDefault()
        void closeCurrentSurfaceWindow()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => {
      disposed = true
      unlisten?.()
      window.removeEventListener('keydown', handleKeyDown)
    }
  }, [])

  return (
    <main className="h-full min-h-0 bg-background text-foreground">
      <InvalidSurface
        route={route}
        onClose={() => void closeCurrentSurfaceWindow()}
      />
    </main>
  )
}

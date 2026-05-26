/**
 * Sandboxed iframe for rendering email HTML.
 *
 * Persisted message HTML is sanitized in Rust via ammonia before reaching the
 * frontend.
 * The iframe uses `sandbox="allow-same-origin"` with no script execution.
 * Long messages scroll inside the iframe rather than expanding the detail pane.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 * @spec docs/L0-ui#html-email-rendering
 */
import { useEffect, useMemo, useRef } from 'react'

import { openExternalUrl } from '../desktop'
import { externalEmailLinkUrl } from '../emailLinks'
import { cn } from '../lib/utils'

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface EmailFrameProps {
  html: string
  className?: string
  title?: string
}

/**
 * Renders email HTML inside a sandboxed `srcdoc` iframe.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 * @spec docs/L0-branding#color-palette-light-mode-primary
 */
export function EmailFrame({
  html,
  className,
  title = 'Email content',
}: EmailFrameProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null)
  const wrappedHtml = useMemo(
    () => `<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        :root { color-scheme: light; }
        html {
            background: transparent;
        }
        body {
            font-family: "Geist", system-ui, sans-serif;
            font-size: 14px;
            line-height: 1.65;
            color: #141618;
            margin: 0;
            padding: 16px;
            background: transparent;
            word-wrap: break-word;
            overflow-wrap: break-word;
        }
        h1, h2, h3, h4 {
            color: #0D1117;
            line-height: 1.2;
        }
        img { max-width: 100%; height: auto; }
        a { color: #2B7EC2; }
        blockquote {
            border-left: 2px solid #D4DAE0;
            margin: 16px 0;
            padding: 4px 0 4px 16px;
            color: #5A6370;
        }
        pre {
            overflow-x: auto;
            background: #EEF1F4;
            padding: 12px 14px;
            border: 1px solid #D4DAE0;
        }
        code {
            font-family: "Geist Mono", monospace;
        }
    </style>
</head>
<body>${html}</body>
</html>`,
    [html],
  )

  useEffect(() => {
    const frame = iframeRef.current
    if (!frame) {
      return
    }

    let unbindClickHandler: (() => void) | null = null
    const bindClickHandler = () => {
      unbindClickHandler?.()
      const document = frame.contentDocument
      if (!document) {
        return
      }

      const handleClick = (event: MouseEvent) => {
        const target = event.target as {
          closest?: (selector: string) => Element | null
          parentElement?: Element | null
        } | null
        const element = target?.closest ? target : target?.parentElement
        const anchor = element?.closest?.('a[href]')
        const href = externalEmailLinkUrl(anchor?.getAttribute('href') ?? null)
        if (!href) {
          return
        }

        event.preventDefault()
        event.stopPropagation()
        void openExternalUrl(href).catch((error: unknown) => {
          console.error('Failed to open email link externally', error)
        })
      }

      document.addEventListener('click', handleClick)
      unbindClickHandler = () =>
        document.removeEventListener('click', handleClick)
    }

    frame.addEventListener('load', bindClickHandler)
    bindClickHandler()

    return () => {
      frame.removeEventListener('load', bindClickHandler)
      unbindClickHandler?.()
    }
  }, [wrappedHtml])

  return (
    <iframe
      ref={iframeRef}
      className={cn('block h-full w-full border-0 bg-transparent', className)}
      sandbox="allow-same-origin"
      srcDoc={wrappedHtml}
      title={title}
    />
  )
}

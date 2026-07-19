/**
 * Sandboxed iframe for rendering email HTML.
 *
 * Persisted message HTML is sanitized in Rust via ammonia before reaching the
 * frontend, and the iframe runs with no `allow-scripts`, so the email's own
 * markup can never execute scripts.
 *
 * Links always open in the system browser, never inside the app. A parent-added
 * click handler cannot run in a no-scripts sandboxed frame (WKWebView blocks
 * it), so instead the link navigates and we intercept the navigation:
 * - Desktop: `<base target="_top">` navigates the top frame; the Tauri webview's
 *   navigation handler opens external URLs externally and blocks the in-app nav.
 * - Browser: `<base target="_blank">` opens a new tab natively.
 *
 * Long messages scroll inside the iframe rather than expanding the detail pane.
 *
 */
import { useMemo } from 'react'

import { isTauriRuntime } from '@/lib/platform/runtime'
import { cn } from '../../../lib/design/cn'

interface EmailFrameProps {
  html: string
  className?: string
  title?: string
}

type EmailLinkTarget = '_blank' | '_top'

function normalizeEmailLinkTargets(
  html: string,
  target: EmailLinkTarget,
): string {
  if (typeof document === 'undefined') {
    return html
  }

  const template = document.createElement('template')
  template.innerHTML = html
  for (const anchor of template.content.querySelectorAll('a[href]')) {
    anchor.setAttribute('target', target)
    anchor.setAttribute('rel', 'noopener noreferrer')
  }
  return template.innerHTML
}

export function EmailFrame({
  html,
  className,
  title = 'Email content',
}: EmailFrameProps) {
  // Desktop intercepts top-frame navigations (open external + block); the
  // browser opens links in a new tab. Both keep the no-scripts sandbox. Force
  // per-anchor targets too because many emails ship target="_blank", which
  // overrides <base target="_top"> and is blocked by the desktop sandbox.
  const isDesktop = isTauriRuntime()
  const linkTarget = isDesktop ? '_top' : '_blank'
  const bodyHtml = useMemo(
    () => normalizeEmailLinkTargets(html, linkTarget),
    [html, linkTarget],
  )
  const wrappedHtml = useMemo(
    () => `<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <base target="${linkTarget}">
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
<body>${bodyHtml}</body>
</html>`,
    [bodyHtml, linkTarget],
  )

  return (
    <iframe
      className={cn('block h-full w-full border-0 bg-transparent', className)}
      sandbox={
        isDesktop
          ? 'allow-same-origin allow-top-navigation-by-user-activation'
          : 'allow-same-origin allow-popups allow-popups-to-escape-sandbox'
      }
      srcDoc={wrappedHtml}
      title={title}
    />
  )
}

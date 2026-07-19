/**
 * Structural contract of the email body frame: the iframe auto-sizes to its
 * content and never scrolls itself, so the surrounding detail pane is the one
 * scroll container. (Regression: a viewport-locked `h-full` frame moved all
 * scrolling inside the sandboxed document, which wheel events cannot reliably
 * reach in the desktop webviews and app keyboard chords can never reach.)
 */
import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import { EmailFrame } from './EmailFrame'

function renderFrame(): string {
  return renderToStaticMarkup(<EmailFrame html="<p>Hello</p>" />)
}

describe('EmailFrame', () => {
  test('the frame is not viewport-locked — the outer pane owns scrolling', () => {
    const markup = renderFrame()
    // No h-full: the frame's height tracks its content (measured after load),
    // so long bodies overflow the OUTER scroll container instead of scrolling
    // invisibly inside the sandbox.
    expect(markup).not.toContain('h-full')
    expect(markup).toContain('w-full')
  })

  test('the frame document itself never scrolls', () => {
    // srcDoc CSS pins html overflow hidden; with the parent sizing the frame
    // to fit, any residual scrolling would trap the wheel inside the sandbox.
    expect(renderFrame()).toContain('overflow: hidden')
  })

  test('the sandbox never grants script execution', () => {
    expect(renderFrame()).not.toContain('allow-scripts')
  })
})

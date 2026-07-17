/** Application entry point: constructs the MailClient facade and mounts the
 * React root in StrictMode. Served behind the dev proxy the facade needs no
 * token of its own (the proxy injects the Authorization header); the packaged
 * app passes the connection-info credentials instead. */
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App'
import { applyBrandFavicon } from './brandFavicon'
import { MailClient } from './client'
import { MailClientProvider } from './data'
import { ackDesktopWebviewBoot } from './desktop'
import { markSurfaceBootstrap } from './surfaceBootstrapLog'

applyBrandFavicon()

if ('__TAURI_INTERNALS__' in window) {
  import('./consoleCapture').then(({ installConsoleCapture }) =>
    installConsoleCapture(),
  )
}

markSurfaceBootstrap('main_entry', {
  hash: window.location.hash,
  tauri: '__TAURI_INTERNALS__' in window,
})

// Boot ACK: tells the desktop backend this window's JS is alive, keeping the
// guarded (JS-aware) close flow; a window that never ACKs is force-destroyed
// on close so a failed frontend load can never yield an unclosable window.
void ackDesktopWebviewBoot()

// Dev-proxy construction: same-origin requests, Authorization injected by
// the proxy, so the facade needs no baseUrl or token of its own.
const mailClient = new MailClient({ baseUrl: '', token: '' })

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MailClientProvider client={mailClient}>
      <App />
    </MailClientProvider>
  </StrictMode>,
)

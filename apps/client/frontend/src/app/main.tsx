/** Application entry point: constructs the MailClient facade and mounts the
 * React root in StrictMode. Served behind the dev proxy the facade needs no
 * token of its own (the proxy injects the Authorization header); the packaged
 * app passes the connection-info credentials instead. */
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App'
import { applyBrandFavicon } from './shell/brandFavicon'
import { bootstrapClientOptions, MailClient } from '../data/transport/client'
import { MailClientProvider } from '../data/index'
import { ackDesktopWebviewBoot } from '../desktop/runtime'
import { markSurfaceBootstrap } from '../surfaces/bootstrapLog'

applyBrandFavicon()

if ('__TAURI_INTERNALS__' in window) {
  import('../desktop/diagnostics/consoleCapture').then(({ installConsoleCapture }) =>
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

// Desktop shell: the injected window globals carry the embedded backend's
// port + token. Dev proxy: same-origin requests, Authorization injected by
// the proxy, so the facade needs no baseUrl or token of its own.
const mailClient = new MailClient(bootstrapClientOptions())

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MailClientProvider client={mailClient}>
      <App />
    </MailClientProvider>
  </StrictMode>,
)

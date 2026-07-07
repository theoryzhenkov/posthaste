/** Application entry point: mounts the React root in StrictMode. */
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App'
import { applyBrandFavicon } from './brandFavicon'
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

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)

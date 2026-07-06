/** Application entry point: mounts the React root in StrictMode. */
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App'
import { applyBrandFavicon } from './brandFavicon'
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

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)

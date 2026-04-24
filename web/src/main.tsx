/** Application entry point: mounts the React root in StrictMode. */
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App'

if ('__TAURI_INTERNALS__' in window) {
  import('./consoleCapture').then(({ installConsoleCapture }) =>
    installConsoleCapture(),
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)

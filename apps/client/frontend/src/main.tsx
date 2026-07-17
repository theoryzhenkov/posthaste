import { createRoot } from 'react-dom/client'
import { App } from './app/App'
import { MailClient } from './client'
import { MailClientProvider } from './hooks'
import './app.css'

// The backend's connection info (port + session secret from the app data
// directory), injected onto the page by the packaged shell before this
// bundle runs. In dev neither field is set: the Vite proxy forwards requests
// to the backend and injects the Authorization header from
// POSTHASTE_DEV_TOKEN, so the in-page token stays empty.
interface InjectedConnectionInfo {
  baseUrl?: string
  port?: number
  token?: string
}

declare global {
  interface Window {
    __POSTHASTE_CONNECTION__?: InjectedConnectionInfo
  }
}

const info = window.__POSTHASTE_CONNECTION__
const baseUrl =
  info?.baseUrl ?? (info?.port !== undefined ? `http://127.0.0.1:${info.port}` : '')
const client = new MailClient({ baseUrl, token: info?.token ?? '' })

createRoot(document.getElementById('root')!).render(
  <MailClientProvider client={client}>
    <App />
  </MailClientProvider>,
)

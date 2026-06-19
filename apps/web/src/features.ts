export function runtimeMailListViewsEnabled(): boolean {
  return import.meta.env.VITE_RUNTIME_MAIL_LIST_VIEWS === '1'
}

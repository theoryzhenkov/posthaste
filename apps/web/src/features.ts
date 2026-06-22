export function runtimeMailListViewsEnabled(): boolean {
  return import.meta.env.VITE_RUNTIME_MAIL_LIST_VIEWS === '1'
}

/// Whether message-detail and conversation surfaces render from runtime view
/// frames (5b-1). Layered on top of their HTTP queries, unlike the mail-list
/// path which replaces its query.
export function runtimeObjectViewsEnabled(): boolean {
  return import.meta.env.VITE_RUNTIME_OBJECT_VIEWS === '1'
}

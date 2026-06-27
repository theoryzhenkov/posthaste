/// Whether message-detail and conversation surfaces render from runtime view
/// frames (5b-1). Always on as of 5d: layered on top of their HTTP queries so
/// flag/read/move optimism pushes instantly. (The mail list itself renders
/// solely from its runtime `mailList` view; the legacy HTTP query + event-patch
/// fork was retired.)
export function runtimeObjectViewsEnabled(): boolean {
  return true
}

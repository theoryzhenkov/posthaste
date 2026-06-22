/// The mail list renders from the runtime `mailList` view (with in-place
/// window-extend for infinite scroll) rather than the legacy HTTP query +
/// event-patch path. Default on as of the windowed-pagination work.
export function runtimeMailListViewsEnabled(): boolean {
  return true
}

/// Whether message-detail and conversation surfaces render from runtime view
/// frames (5b-1). Always on as of 5d: layered on top of their HTTP queries so
/// flag/read/move optimism pushes instantly. The mail-list view stays behind
/// {@link runtimeMailListViewsEnabled} until it supports windowed pagination;
/// the list reflects optimism through the domain-event cache path meanwhile.
export function runtimeObjectViewsEnabled(): boolean {
  return true
}

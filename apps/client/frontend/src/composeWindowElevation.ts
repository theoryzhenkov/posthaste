export function shouldCloseOriginalComposeAfterWindowOpen({
  openingResetKey,
  lastEditedResetKey,
}: {
  openingResetKey: string
  lastEditedResetKey: string | null
}): boolean {
  // Surface-window compose routes carry only the initial intent. If the user
  // edits the floating draft while the native/window open is in flight, keep
  // the original compose popup open so that draft text is not discarded.
  return lastEditedResetKey !== openingResetKey
}

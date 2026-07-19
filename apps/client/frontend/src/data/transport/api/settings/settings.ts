// The settings document and its parts are the generated wire twins,
// re-exported under the historical names so the settings panes share one type
// identity with the `appSettings` query and the `updateSettings` command.
export type { AppSettings, CachePolicy, MailboxGroup, TagAppearance } from '@/gen'

/** The app-default undo-send hold when the setting is unset. */
export const DEFAULT_UNDO_SEND_DELAY_SECONDS = 10

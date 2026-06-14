/**
 * Type-level conformance assertions between the curated frontend view-model
 * (`./types`) and the generated wire schema (`./schema.gen`).
 *
 * This module is type-only: it emits no runtime code. It exists purely so that
 * `tsc` fails if the curated types silently drift from the wire contract.
 *
 * @spec docs/L1-api#endpoint-table
 */
export type * from './conformance/accounts'
export type * from './conformance/automation'
export type * from './conformance/compose'
export type * from './conformance/mail'
export type * from './conformance/scalars'
export type * from './conformance/settings'
export type * from './conformance/smartMailboxes'

// Types intentionally without their own assertion:
// - MessageCommand: frontend-only union dispatched to separate backend commands.
// - KnownMailboxRole: curated narrowing of the wire's free-form mailbox role.
// - AccountConnectionOverview variants: covered by _AccountConnectionOverview.

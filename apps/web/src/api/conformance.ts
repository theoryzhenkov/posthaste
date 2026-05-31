/**
 * Type-level conformance assertions between the curated frontend view-model
 * (`./types`) and the generated wire schema (`./schema.gen`).
 *
 * This module is type-only: it emits no runtime code. It exists purely so that
 * `tsc` fails if the curated types silently drift from the wire contract.
 *
 * The curated layer intentionally differs from the wire in three tolerated ways
 * (see the P2 design note):
 *   1. Renamed types (e.g. `Mailbox` <-> wire `MailboxSummary`).
 *   2. Tighter optionality: serde `Option<T>` emits `field?: T | null`, but the
 *      frontend often declares `field: T | null` because the backend always
 *      sends it. The bidirectional `DeepPartial` check below tolerates this.
 *   3. Frontend-only abstractions with no wire counterpart (e.g.
 *      `MessageCommand`); these get a comment instead of an assertion.
 *
 * What it CATCHES: renamed fields, removed fields, and incompatible value types.
 *
 * @spec docs/L1-api#endpoint-table
 */
import type { components } from './schema.gen'
import type {
  AccountAppearance,
  AccountConnectionOverview,
  AccountDriver,
  AccountOverview,
  AccountTransportInput,
  AppSettings,
  AutomationAction,
  AutomationRule,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
  AutomationTrigger,
  CachePolicy,
  CachedSenderAddress,
  ConversationPage,
  ConversationSummary,
  ConversationView,
  CreateAccountInput,
  CreateSmartMailboxInput,
  DomainEvent,
  Identity,
  Mailbox,
  MailEndpointSettings,
  MessageAttachment,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  MessageSortField,
  MessageSummary,
  OkResponse,
  PatchMailboxInput,
  ProviderAuthKind,
  ProviderHint,
  ProviderKind,
  RawMessageRef,
  Recipient,
  ReplyContext,
  SecretInstructionInput,
  SecretStatus,
  SendMessageInput,
  SidebarResponse,
  SidebarSmartMailbox,
  SidebarSource,
  SmartMailbox,
  SmartMailboxCondition,
  SmartMailboxField,
  SmartMailboxGroup,
  SmartMailboxGroupOperator,
  SmartMailboxKind,
  SmartMailboxOperator,
  SmartMailboxRule,
  SmartMailboxRuleGroup,
  SmartMailboxRuleNode,
  SmartMailboxSummary,
  SmartMailboxValue,
  SourceMessageRef,
  StartOAuthResponse,
  StartProviderOAuthInput,
  SyncMode,
  SyncProgress,
  TagSummary,
  TransportSecurity,
  UpdateAccountInput,
  UpdateSmartMailboxInput,
  VerificationResponse,
} from './types'

type Wire = components['schemas']

type AssertTrue<T extends true> = T

/** Strips `null`/`undefined` so the nullability axis can be compared separately. */
type Defined<T> = T extends null | undefined ? never : T

/**
 * Two leaf (non-object) types are compatible when either is assignable to the
 * other, ignoring `null`/`undefined`. This tolerates two intentional curated
 * tightenings while still catching genuinely unrelated retypes:
 *   - value-domain narrowing, e.g. wire `string` vs curated `KnownMailboxRole`,
 *     or wire `ProviderAuthKind` vs a per-variant `'oauth2'`.
 *   - free-form objects: the wire renders `serde_json::Value` as the opaque
 *     `Record<string, never>`, while the curated side uses `Record<string,
 *     unknown>`; these are mutually-narrowing and so count as compatible.
 * A real retype (e.g. `string` vs `number`) is assignable in neither direction
 * and is still rejected.
 */
type LeafCompatible<A, B> = [Defined<A>] extends [Defined<B>]
  ? true
  : [Defined<B>] extends [Defined<A>]
    ? true
    : false

/**
 * An object is "structural" (recurse into its named keys) only when it has a
 * fixed set of properties. Arrays and free-form index-signature bags (e.g. the
 * wire's opaque `Record<string, never>` for `serde_json::Value`, or a curated
 * `Record<string, unknown>`) are NOT structural: they are compared as leaves via
 * `LeafCompatible`, so the free-form payload conforms without spurious recursion.
 */
type IsObject<T> =
  Defined<T> extends object
    ? Defined<T> extends readonly unknown[]
      ? false
      : string extends keyof Defined<T>
        ? false
        : number extends keyof Defined<T>
          ? false
          : true
    : false

/**
 * Recursion fuel. Each nesting level pops one element; when empty, deeply
 * nested fields (e.g. the self-referential smart-mailbox rule tree) fall back to
 * plain bidirectional assignability instead of recursing further, which both
 * terminates and keeps the check sound for the bounded depth above it.
 */
type Depth = [unknown, unknown, unknown, unknown, unknown, unknown]
type Pop<D extends unknown[]> = D extends [unknown, ...infer Rest] ? Rest : []

/**
 * `true` iff curated `V` structurally conforms to wire `W`, recursively.
 *
 * Rules, applied per (possibly nested) field:
 *   - Optionality is ignored: a field optional on one side and required on the
 *     other still conforms (the tolerated `Option<T>` -> required tightening).
 *   - Field NAMES must match in both directions: a key present on one side and
 *     absent on the other is a renamed/removed field and fails (`keyof` set
 *     equality below).
 *   - Arrays recurse on their element type.
 *   - Nested objects recurse.
 *   - Leaves must be `LeafCompatible` (mutually assignable modulo null), which
 *     tolerates value-domain narrowing but rejects unrelated retypes.
 *
 * Unions on either side are matched member-to-member: every curated member must
 * conform to SOME wire member and vice versa (so discriminated unions line up
 * variant-by-variant).
 */
type StructConforms<V, W, D extends unknown[]> = [Defined<V>] extends [never]
  ? true
  : [Defined<W>] extends [never]
    ? true
    : D extends []
      ? LeafCompatible<V, W>
      : IsObject<V> extends true
        ? IsObject<W> extends true
          ? // both objects: key sets must match, and shared keys recurse.
            [Exclude<keyof Defined<V>, keyof Defined<W>>] extends [never]
            ? [Exclude<keyof Defined<W>, keyof Defined<V>>] extends [never]
              ? AllKeysConform<Defined<V>, Defined<W>, D>
              : false
            : false
          : false
        : IsObject<W> extends true
          ? false
          : Defined<V> extends readonly (infer Ve)[]
            ? Defined<W> extends readonly (infer We)[]
              ? Conforms<Ve, We, D>
              : false
            : Defined<W> extends readonly unknown[]
              ? false
              : LeafCompatible<V, W>

type AllKeysConform<V, W, D extends unknown[]> = {
  [K in keyof V & keyof W]: Conforms<V[K], W[K], Pop<D>> extends true
    ? true
    : false
}[keyof V & keyof W] extends true
  ? true
  : false

/** Each member of `A` must conform to at least one member of `B`. */
type Covered<A, B, D extends unknown[]> = [false] extends [
  A extends unknown ? (MatchesAny<A, B, D> extends true ? true : false) : never,
]
  ? false
  : true

type MatchesAny<A, B, D extends unknown[]> = [true] extends [
  B extends unknown
    ? StructConforms<A, B, D> extends true
      ? true
      : false
    : never,
]
  ? true
  : false

/** `true` when no member of union `T` is a structural object or an array. */
type IsLeafUnion<T> = [true] extends [
  T extends unknown
    ? IsObject<T> extends true
      ? false
      : ArrayOf<T> extends true
        ? false
        : true
    : never,
]
  ? [false] extends [
      T extends unknown
        ? IsObject<T> extends true
          ? false
          : ArrayOf<T> extends true
            ? false
            : true
        : never,
    ]
    ? false
    : true
  : false

type ArrayOf<T> = Defined<T> extends readonly unknown[] ? true : false

/**
 * Bidirectional conformance between a curated type and its wire counterpart.
 *
 * Leaf unions (scalars and string-literal enums) are compared with
 * `LeafCompatible`, which tolerates value-domain NARROWING in either direction
 * (e.g. a per-variant `'oauth2'` against wire `ProviderAuthKind`, or
 * `KnownMailboxRole` against wire `string`) but rejects unrelated retypes.
 *
 * Object / discriminated-union types are matched by mutual `Covered`: every
 * curated member must structurally conform to SOME wire member and vice versa,
 * so renamed/removed fields and mis-shaped variants are caught.
 *
 * Each top-level assertion below wraps this in `AssertTrue<...>`, so a `false`
 * result becomes a `tsc` error at that line.
 */
type Conforms<View, W, D extends unknown[] = Depth> =
  IsLeafUnion<View> extends true
    ? LeafCompatible<View, W>
    : IsLeafUnion<W> extends true
      ? LeafCompatible<View, W>
      : Covered<View, W, D> extends true
        ? Covered<W, View, D> extends true
          ? true
          : false
        : false

/* --- Scalar / string-enum view-models --------------------------------- */
export type _AccountDriver = AssertTrue<
  Conforms<AccountDriver, Wire['AccountDriver']>
>
export type _ProviderKind = AssertTrue<
  Conforms<ProviderKind, Wire['ProviderKind']>
>
export type _ProviderHint = AssertTrue<
  Conforms<ProviderHint, Wire['ProviderHint']>
>
export type _ProviderAuthKind = AssertTrue<
  Conforms<ProviderAuthKind, Wire['ProviderAuthKind']>
>
export type _TransportSecurity = AssertTrue<
  Conforms<TransportSecurity, Wire['TransportSecurity']>
>
export type _AutomationTrigger = AssertTrue<
  Conforms<AutomationTrigger, Wire['AutomationTrigger']>
>
export type _MessageSortField = AssertTrue<
  Conforms<MessageSortField, Wire['MessageSortField']>
>
export type _SyncMode = AssertTrue<Conforms<SyncMode, Wire['SyncMode']>>
export type _SmartMailboxKind = AssertTrue<
  Conforms<SmartMailboxKind, Wire['SmartMailboxKind']>
>
export type _SmartMailboxGroupOperator = AssertTrue<
  Conforms<SmartMailboxGroupOperator, Wire['SmartMailboxGroupOperator']>
>
export type _SmartMailboxField = AssertTrue<
  Conforms<SmartMailboxField, Wire['SmartMailboxField']>
>
export type _SmartMailboxOperator = AssertTrue<
  Conforms<SmartMailboxOperator, Wire['SmartMailboxOperator']>
>
export type _SmartMailboxValue = AssertTrue<
  Conforms<SmartMailboxValue, Wire['SmartMailboxValue']>
>

/* --- Appearance / settings -------------------------------------------- */
export type _MailEndpointSettings = AssertTrue<
  Conforms<MailEndpointSettings, Wire['ImapTransportSettings']>
>
// NOTE: appearance (ThemeMode / PalettePresetId / UiDensity /
// AppAppearanceSettings / AppGlassThemeSettings / AppGlassBloomSettings) is
// client-local presentation state and is intentionally NOT in the wire schema,
// so it has no conformance assertion here.
export type _AppSettings = AssertTrue<
  Conforms<AppSettings, Wire['AppSettings']>
>
export type _CachePolicy = AssertTrue<
  Conforms<CachePolicy, Wire['CachePolicy']>
>
export type _SecretStatus = AssertTrue<
  Conforms<SecretStatus, Wire['SecretStatus']>
>
export type _AccountAppearance = AssertTrue<
  Conforms<AccountAppearance, Wire['AccountAppearance']>
>

/* --- Automation -------------------------------------------------------- */
export type _AutomationAction = AssertTrue<
  Conforms<AutomationAction, Wire['AutomationAction']>
>
export type _AutomationRule = AssertTrue<
  Conforms<AutomationRule, Wire['AutomationRule']>
>
export type _AutomationRulePreviewInput = AssertTrue<
  Conforms<AutomationRulePreviewInput, Wire['PreviewAutomationRuleRequest']>
>
export type _AutomationRulePreviewResponse = AssertTrue<
  Conforms<AutomationRulePreviewResponse, Wire['AutomationRulePreviewResponse']>
>

/* --- Accounts ---------------------------------------------------------- */
export type _AccountConnectionOverview = AssertTrue<
  Conforms<AccountConnectionOverview, Wire['AccountConnectionOverview']>
>
export type _AccountOverview = AssertTrue<
  Conforms<AccountOverview, Wire['AccountOverview']>
>
export type _AccountTransportInput = AssertTrue<
  Conforms<AccountTransportInput, Wire['AccountTransportRequest']>
>
export type _SecretInstructionInput = AssertTrue<
  Conforms<SecretInstructionInput, Wire['SecretWriteRequest']>
>
export type _CreateAccountInput = AssertTrue<
  Conforms<CreateAccountInput, Wire['CreateAccountRequest']>
>
export type _UpdateAccountInput = AssertTrue<
  Conforms<UpdateAccountInput, Wire['PatchAccountRequest']>
>
export type _VerificationResponse = AssertTrue<
  Conforms<VerificationResponse, Wire['VerificationResponse']>
>
export type _StartProviderOAuthInput = AssertTrue<
  Conforms<StartProviderOAuthInput, Wire['StartProviderOAuthRequest']>
>
export type _StartOAuthResponse = AssertTrue<
  Conforms<StartOAuthResponse, Wire['StartOAuthResponse']>
>
export type _CachedSenderAddress = AssertTrue<
  Conforms<CachedSenderAddress, Wire['CachedSenderAddress']>
>
export type _SyncProgress = AssertTrue<
  Conforms<SyncProgress, Wire['SyncProgress']>
>

/* --- Compose ----------------------------------------------------------- */
export type _Identity = AssertTrue<Conforms<Identity, Wire['Identity']>>
export type _Recipient = AssertTrue<Conforms<Recipient, Wire['Recipient']>>
export type _ReplyContext = AssertTrue<
  Conforms<ReplyContext, Wire['ReplyContext']>
>
export type _SendMessageInput = AssertTrue<
  Conforms<SendMessageInput, Wire['SendMessageRequest']>
>
export type _OkResponse = AssertTrue<Conforms<OkResponse, Wire['OkResponse']>>

/* --- Mailboxes / messages --------------------------------------------- */
export type _Mailbox = AssertTrue<Conforms<Mailbox, Wire['MailboxSummary']>>
export type _PatchMailboxInput = AssertTrue<
  Conforms<PatchMailboxInput, Wire['PatchMailboxRequest']>
>
export type _MessageSummary = AssertTrue<
  Conforms<MessageSummary, Wire['MessageSummary']>
>
export type _MessagePage = AssertTrue<
  Conforms<MessagePage, Wire['MessagePageResponse']>
>
export type _RawMessageRef = AssertTrue<
  Conforms<RawMessageRef, Wire['RawMessageRef']>
>
export type _MessageAttachment = AssertTrue<
  Conforms<MessageAttachment, Wire['MessageAttachment']>
>
export type _MessageDetail = AssertTrue<
  Conforms<MessageDetail, Wire['MessageDetail']>
>
export type _SourceMessageRef = AssertTrue<
  Conforms<SourceMessageRef, Wire['SourceMessageRef']>
>
export type _MessageCommandResult = AssertTrue<
  Conforms<MessageCommandResult, Wire['CommandResult']>
>

/* --- Conversations ----------------------------------------------------- */
export type _ConversationSummary = AssertTrue<
  Conforms<ConversationSummary, Wire['ConversationSummary']>
>
export type _ConversationPage = AssertTrue<
  Conforms<ConversationPage, Wire['ConversationPageResponse']>
>
export type _ConversationView = AssertTrue<
  Conforms<ConversationView, Wire['ConversationView']>
>

/* --- Sidebar / tags ---------------------------------------------------- */
export type _SidebarSmartMailbox = AssertTrue<
  Conforms<SidebarSmartMailbox, Wire['SidebarSmartMailbox']>
>
export type _TagSummary = AssertTrue<Conforms<TagSummary, Wire['TagSummary']>>
export type _SidebarSource = AssertTrue<
  Conforms<SidebarSource, Wire['SidebarSource']>
>
export type _SidebarResponse = AssertTrue<
  Conforms<SidebarResponse, Wire['SidebarResponse']>
>

/* --- Events ------------------------------------------------------------ */
export type _DomainEvent = AssertTrue<
  Conforms<DomainEvent, Wire['DomainEvent']>
>

/* --- Smart mailboxes --------------------------------------------------- */
// The wire emits the rule-tree discriminant (`type`) only on the union
// `SmartMailboxRuleNode` (= group-member | condition-member), whereas the
// curated layer folds the discriminant into each node variant. So the curated
// `SmartMailboxRuleGroup`/`SmartMailboxCondition` map to the corresponding
// MEMBER of the wire node union, while the bare (discriminant-less) curated
// `SmartMailboxGroup` maps to the bare wire `SmartMailboxGroup`.
export type _SmartMailboxGroup = AssertTrue<
  Conforms<SmartMailboxGroup, Wire['SmartMailboxGroup']>
>
export type _SmartMailboxCondition = AssertTrue<
  Conforms<
    SmartMailboxCondition,
    Extract<Wire['SmartMailboxRuleNode'], { type: 'condition' }>
  >
>
export type _SmartMailboxRuleGroup = AssertTrue<
  Conforms<
    SmartMailboxRuleGroup,
    Extract<Wire['SmartMailboxRuleNode'], { type: 'group' }>
  >
>
export type _SmartMailboxRuleNode = AssertTrue<
  Conforms<SmartMailboxRuleNode, Wire['SmartMailboxRuleNode']>
>
export type _SmartMailboxRule = AssertTrue<
  Conforms<SmartMailboxRule, Wire['SmartMailboxRule']>
>
export type _SmartMailbox = AssertTrue<
  Conforms<SmartMailbox, Wire['SmartMailbox']>
>
export type _SmartMailboxSummary = AssertTrue<
  Conforms<SmartMailboxSummary, Wire['SmartMailboxSummary']>
>
export type _CreateSmartMailboxInput = AssertTrue<
  Conforms<CreateSmartMailboxInput, Wire['CreateSmartMailboxRequest']>
>
export type _UpdateSmartMailboxInput = AssertTrue<
  Conforms<UpdateSmartMailboxInput, Wire['PatchSmartMailboxRequest']>
>

/* --- Types intentionally without their own assertion ------------------- */
// frontend-only: no wire schema -- MessageCommand (union dispatched to 4 endpoints;
//   the backend models its arms as separate SetKeywords/AddToMailbox/... commands).
// frontend-only: no wire schema -- KnownMailboxRole (a curated narrowing of the
//   wire's free-form mailbox `role: string`; tolerated by LeafCompatible above).
// covered transitively -- ManualCredentialsAccountConnectionOverview and
//   ManagedOAuthAccountConnectionOverview are the two variants of
//   AccountConnectionOverview (asserted as _AccountConnectionOverview); the wire
//   has no standalone schema for either variant.

// Force this module to be treated as a module with no runtime emit.
export type {}

/**
 * The command-surface contract (R4/R11): the domain-free half of the action
 * table that UI components are allowed to see.
 *
 * `commands/` owns the registry, resolver, and definitions; components never
 * import it (R11: commands bind UI to verbs, never the reverse). What a
 * component receives instead is:
 *
 * - {@link ResolvedActionView}: one resolved, context-bound action — title,
 *   icon, enablement, confirm copy, param options, and bound runners. Hosts in
 *   `app/` resolve from the registry and hand these down as props.
 * - {@link runActionWithConfirm}: the ONE execution gate every surface uses,
 *   so a destructive action can never run without its confirm dialog and a
 *   parameterized action never silently no-ops.
 * - {@link useCommandScope}: registers a scoped service binding (compose send,
 *   surface close, …) with the app-mounted command dispatcher, replacing
 *   per-component `window.addEventListener` input handling.
 */
import { createContext, useContext, useEffect } from 'react'
import type { LucideIcon } from 'lucide-react'

/** Section ordering within menus / palette groups. */
export type ActionSection =
  | 'open'
  | 'compose-reply'
  | 'state'
  | 'organize'
  | 'move'
  | 'navigate'
  | 'app'

/**
 * One choosable target of a PARAMETERIZED action (e.g. a mailbox for
 * `message.move-to-mailbox`, a snooze preset for `message.snooze`). Pure data —
 * `id` is the value the runner receives, `label` is what every surface renders
 * (context submenu row, palette pick-step row, header popover row).
 */
export interface ActionParamOption {
  id: string
  label: string
  icon?: LucideIcon
  /** Extra search terms for the palette's pick-step filter. */
  keywords?: string
}

/** Confirmation copy shown by the shared dialog host before a gated run. */
export interface ActionConfirmCopy {
  title: string
  description: string
  confirmLabel: string
}

/**
 * A resolved, context-bound action as a surface renders it. The registry's
 * `ResolvedAction` extends this with the full definition; components only ever
 * see this flattened view.
 */
export interface ResolvedActionView {
  /** Stable namespaced id, e.g. `message.archive`. */
  id: string
  section: ActionSection
  destructive: boolean
  /** Context-applied. */
  title: string
  icon: LucideIcon
  enabled: boolean
  disabledReason?: string
  /** Context-resolved confirmation copy; `undefined` = run without a dialog. */
  confirm?: ActionConfirmCopy
  /** Context-resolved options of a PARAMETERIZED action; `undefined` for plain
   *  actions. Surfaces render these as their picker. */
  params?: ActionParamOption[]
  /** Bound runner. Route through {@link runActionWithConfirm} instead of
   *  calling this directly wherever a confirm/param gate can apply. */
  execute: () => void | Promise<void>
  /** Parameterized runner — present iff {@link params} is. */
  executeWith?: (param: ActionParamOption) => void | Promise<void>
}

/**
 * Run a resolved action, honoring its gates.
 *
 * A PARAMETERIZED action (params present) cannot run bare — it routes through
 * `requestParam` (e.g. the palette pick-step) or is skipped. A
 * `confirm`-bearing action is NEVER executed directly: it routes through
 * `requestConfirm` and only runs if the user accepts. Everything else runs
 * instantly.
 */
export function runActionWithConfirm(
  action: ResolvedActionView,
  requestConfirm: (confirm: ActionConfirmCopy, onConfirm: () => void) => void,
  requestParam?: (action: ResolvedActionView) => void,
): void {
  if (action.params !== undefined) {
    requestParam?.(action)
    return
  }
  if (action.confirm) {
    requestConfirm(action.confirm, () => void action.execute())
    return
  }
  void action.execute()
}

/** Who owns input where a command scope is mounted. */
export type CommandInputOwner = 'mail' | 'overlay' | 'surface'

/**
 * The domain-free service capabilities a scope can bind. Definitions in
 * `commands/` gate their availability on these bindings, so a chord only
 * resolves where its capability exists (e.g. ⌘Enter sends only while a
 * composer scope is mounted).
 */
export interface CommandScopeServices {
  /** Desktop devtools — bound once by the app root in the Tauri runtime. */
  desktop?: {
    isDeveloperToolsEnabled: () => boolean
    toggleDevtools: () => void | Promise<void>
  }
  /** The focused-surface host: bound while a surface owns the screen and may
   *  be closed (Escape). */
  surfaceHost?: {
    close: () => void
  }
  /** The active composer: bound while a compose form is mounted (⌘Enter). */
  compose?: {
    send: () => void
  }
}

/** One scoped binding of input-owner + services, registered with the
 *  dispatcher for as long as its host is mounted. */
export interface CommandScope {
  owner: CommandInputOwner
  services: CommandScopeServices
}

/** Registration surface the dispatcher provides. Scopes are consulted
 *  last-registered-first, mirroring the visual stacking of their hosts. */
export interface CommandScopeRegistry {
  register: (scope: CommandScope) => () => void
}

/** Provided by the command dispatcher at the app root (`commands/dispatcher`).
 *  Components consume it via {@link useCommandScope}. */
export const CommandScopeContext = createContext<CommandScopeRegistry | null>(
  null,
)

/**
 * Register a command scope for the lifetime of the calling component. The
 * scope's chords route through the app dispatcher — the component itself never
 * listens for input. A missing provider (unit tests, isolated harnesses) is a
 * no-op.
 */
export function useCommandScope(scope: CommandScope | null): void {
  const registry = useContext(CommandScopeContext)
  useEffect(() => {
    if (!registry || !scope) return
    return registry.register(scope)
  }, [registry, scope])
}

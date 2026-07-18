/**
 * Charter ratchet lint (docs/client/L2-charter.md): three structural bans —
 * R4 (global listeners outside the dispatcher/primitives), R3 (exported
 * boolean validators over raw strings), R5 (hand-rolled store plumbing).
 * Style and correctness stay with prettier/tsc; no style rules here.
 *
 * Baseline mechanism: ESLint has no native baseline, so current offender
 * files are listed in the override blocks below. Never add per-file disable
 * comments. Every burn-down block has burned to zero (R3 in the primitives
 * slice, R4 in the commands slice, R5 in the store slice); what remains are
 * permanent allowlists for the named infrastructure. New files/sites fail
 * immediately.
 *
 * Selector approximations (documented per the charter's spirit over letter):
 * - R3 matches exported `function is*` / `has*` whose FIRST param is annotated
 *   exactly `string`. Union params (`string | null`), arrow-function
 *   exports, and unannotated params escape it; the domain/ parse kit is the
 *   real enforcement, this selector just catches regressions.
 * - R5 matches `new Set(...)` bound to a name ending in `listeners`
 *   (variable or class field). A hand-rolled store that names its set
 *   differently escapes; the paired subscribe/getSnapshot shape is not
 *   expressible in an AST selector.
 */
import tseslint from 'typescript-eslint'

const R4_MESSAGE =
  'R4: all input is a command — route through the command registry. Global listeners live only in the dispatcher and named low-level primitives (see the allowlist in eslint.config.mjs).'

const R3_SELECTOR =
  "ExportNamedDeclaration > FunctionDeclaration[id.name=/^(is|has)[A-Z]/][params.0.typeAnnotation.typeAnnotation.type='TSStringKeyword']"
const R3_MESSAGE =
  'R3: parse, don’t validate — no exported is*/has*(raw: string) validators. Return a branded type from parseX(raw): X | null instead.'

const R5_SELECTORS = [
  "VariableDeclarator[init.type='NewExpression'][init.callee.name='Set'][id.name=/[lL]isteners$/]",
  "PropertyDefinition[value.type='NewExpression'][value.callee.name='Set'][key.name=/[lL]isteners$/]",
]
const R5_MESSAGE =
  'R5: one store implementation — use createStore from lib/store (or React state); do not hand-roll listener sets.'

const restrictedSyntax = (selectors) => [
  'error',
  ...selectors.map(({ selector, message }) => ({ selector, message })),
]

const R3_RULE = { selector: R3_SELECTOR, message: R3_MESSAGE }
const R5_RULES = R5_SELECTORS.map((selector) => ({
  selector,
  message: R5_MESSAGE,
}))

export default [
  { ignores: ['dist/**', 'node_modules/**', 'src/gen/**'] },
  {
    files: ['src/**/*.{ts,tsx}'],
    languageOptions: {
      parser: tseslint.parser,
      ecmaVersion: 2022,
      sourceType: 'module',
    },
    rules: {
      'no-restricted-properties': [
        'error',
        { object: 'window', property: 'addEventListener', message: R4_MESSAGE },
        {
          object: 'document',
          property: 'addEventListener',
          message: R4_MESSAGE,
        },
      ],
      'no-restricted-syntax': restrictedSyntax([R3_RULE, ...R5_RULES]),
    },
  },

  // R4 permanent allowlist: the registry's two dispatchers (the mail shell's
  // KeyboardController and the scope-based CommandDispatcher every other
  // window keydown routes through) and named low-level primitives — the DOM
  // primitive module (dismissal/measure/unload guards, lib/dom.ts),
  // panel dismissal/placement, routing, storage sync (the preferences store's
  // multi-key sync and lib/store's stored-store sync), crash capture. These
  // are the infrastructure the rule routes everything into; the slice-4
  // burn-down block (App/SurfaceHost/FocusedSurface/OnboardingTour/
  // ComposeOverlay/MarkdownComposerEditor) burned to zero.
  {
    files: [
      'src/app/input/keyboard/KeyboardController.tsx',
      'src/commands/dispatcher.tsx',
      'src/lib/dom.ts',
      'src/components/floating/hooks/usePanelDismissal.ts',
      'src/components/floating/hooks/usePanelPlacement.ts',
      'src/surfaces/useSurfaceRouting.ts',
      'src/data/preferences/store.ts',
      'src/lib/store.ts',
      'src/desktop/diagnostics/consoleCapture.ts',
    ],
    rules: { 'no-restricted-properties': 'off' },
  },

  // R5 exemption: lib/store.ts is the one blessed implementation (the slice-4
  // burn-down list emptied when the seven hand-rolled copies migrated onto
  // it). R3 stays live here.
  {
    files: ['src/lib/store.ts'],
    rules: { 'no-restricted-syntax': restrictedSyntax([R3_RULE]) },
  },
]

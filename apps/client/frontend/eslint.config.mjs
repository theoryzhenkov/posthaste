/**
 * Charter ratchet lint (docs/client/L2-charter.md): three structural bans —
 * R4 (global listeners outside the dispatcher/primitives), R3 (exported
 * boolean validators over raw strings), R5 (hand-rolled store plumbing).
 * Style and correctness stay with prettier/tsc; no style rules here.
 *
 * Baseline mechanism: ESLint has no native baseline, so current offender
 * files are listed in the override blocks below. Never add per-file disable
 * comments. The "burn-down" blocks are slices 3-5 debt: entries only leave
 * this file (as the sites migrate); new files/sites fail immediately.
 *
 * Selector approximations (documented per the charter's spirit over letter):
 * - R3 matches exported `function is*` / `has*` whose FIRST param is annotated
 *   exactly `string`. Union params (`string | null`), arrow-function
 *   exports, and unannotated params escape it; tightening happens in the
 *   primitives slice, not by selector golf.
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

  // R4 permanent allowlist: the command dispatcher and named low-level
  // primitives (focus/dismissal/placement, routing, storage sync, crash
  // capture). These are the infrastructure the rule routes everything into.
  {
    files: [
      'src/components/keyboard/KeyboardController.tsx',
      'src/components/floating/hooks/usePanelDismissal.ts',
      'src/components/floating/hooks/usePanelPlacement.ts',
      'src/surfaces/useSurfaceRouting.ts',
      'src/data/preferences/store.ts',
      'src/desktop/devtools.ts',
      'src/desktop/diagnostics/consoleCapture.ts',
    ],
    rules: { 'no-restricted-properties': 'off' },
  },

  // R4 baseline (slice 4 burn-down): component/app listener sites that must
  // migrate onto the command registry. Remove each entry as it lands.
  {
    files: [
      'src/app/App.tsx',
      'src/app/host/SurfaceHost.tsx',
      'src/app/host/FocusedSurface.tsx',
      'src/app/shell/onboarding/OnboardingTour.tsx',
      'src/components/compose/ComposeOverlay.tsx',
      'src/components/compose/editor/MarkdownComposerEditor.tsx',
    ],
    rules: { 'no-restricted-properties': 'off' },
  },

  // R3 baseline (slice 3 burn-down): the exported raw-string validators that
  // collapse into domain/ parsers. R5 stays live in these files.
  {
    files: [
      'src/lib/design/tokens/density.ts',
      'src/lib/design/theme/theme.ts',
      'src/components/compose/form/model.ts',
      'src/domain/addressSuggestions.ts',
      'src/domain/search/scan.ts',
    ],
    rules: { 'no-restricted-syntax': restrictedSyntax(R5_RULES) },
  },

  // R5 exemption: lib/store* is where the one blessed implementation lives.
  // R5 baseline (slice 4 burn-down): the hand-rolled copies that migrate to
  // it. R3 stays live in all of these files.
  {
    files: [
      'src/lib/store.ts',
      'src/lib/store/**',
      'src/components/mail/list/model/useViewMode.ts',
      'src/components/mail/thread/useColumnConfig.ts',
      'src/app/shell/onboarding/store.ts',
      'src/data/preferences/store.ts',
      'src/data/notifications/store.ts',
      'src/data/transport/client.ts',
      'src/desktop/devtools.ts',
    ],
    rules: { 'no-restricted-syntax': restrictedSyntax([R3_RULE]) },
  },
]

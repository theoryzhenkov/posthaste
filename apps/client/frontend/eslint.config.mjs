/**
 * Charter ratchet lint (docs/client/L2-charter.md): four structural bans —
 * R4 (global listeners outside the dispatcher/primitives), R3 (exported
 * boolean validators over raw strings), R5 (hand-rolled store plumbing),
 * R8 (ambient clock/randomness/storage outside their seams).
 * Style and correctness stay with prettier/tsc; no style rules here.
 *
 * Baseline mechanism: ESLint has no native baseline, so current offender
 * files are listed in the override blocks below. Never add per-file disable
 * comments. Every burn-down block has burned to zero (R3 in the primitives
 * slice, R4 in the commands slice, R5 in the store slice, R8 in the sweep
 * slice); what remains are permanent allowlists for the named
 * infrastructure. New files/sites fail immediately.
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
 * - R8 bans the direct spellings (`Date.now()`, zero-arg `new Date()`,
 *   `Math.random()`, `crypto.randomUUID()`, `localStorage` bare or via
 *   `window.`). Derived-value construction (`new Date(value)`) stays legal —
 *   only the wall-clock READ is ambient. An aliased global escapes the
 *   selectors; the seam modules are the real API.
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

// R8: ambient dependencies cross ONE seam each (lib/ambient/{time,random,
// storage}; multi-key preference persistence additionally lives in
// data/preferences). Components never read the wall clock, the RNG, or web
// storage directly.
const R8_TIME_MESSAGE =
  'R8: the clock is a seam — import now()/nowMs() from lib/ambient/time instead of reading the ambient Date.'
const R8_RANDOM_MESSAGE =
  'R8: randomness is a seam — import newId()/randomInt() from lib/ambient/random instead of the ambient RNG.'
const R8_STORAGE_MESSAGE =
  'R8: web storage is a seam — use createStoredStore (lib/store), lib/ambient/storage, or the preferences store instead of touching localStorage.'

const R4_PROPERTIES = [
  { object: 'window', property: 'addEventListener', message: R4_MESSAGE },
  { object: 'document', property: 'addEventListener', message: R4_MESSAGE },
]
const R8_TIME_PROPERTIES = [
  { object: 'Date', property: 'now', message: R8_TIME_MESSAGE },
]
const R8_RANDOM_PROPERTIES = [
  { object: 'Math', property: 'random', message: R8_RANDOM_MESSAGE },
  { object: 'crypto', property: 'randomUUID', message: R8_RANDOM_MESSAGE },
]
const R8_STORAGE_PROPERTIES = [
  { object: 'window', property: 'localStorage', message: R8_STORAGE_MESSAGE },
  { object: 'window', property: 'sessionStorage', message: R8_STORAGE_MESSAGE },
]
const R8_STORAGE_GLOBALS = [
  { name: 'localStorage', message: R8_STORAGE_MESSAGE },
  { name: 'sessionStorage', message: R8_STORAGE_MESSAGE },
]
// Zero-arg `new Date()` is a wall-clock read; `new Date(value)` is parsing.
const R8_BARE_DATE_RULE = {
  selector: "NewExpression[callee.name='Date'][arguments.length=0]",
  message: R8_TIME_MESSAGE,
}

const restrictedSyntax = (selectors) => [
  'error',
  ...selectors.map(({ selector, message }) => ({ selector, message })),
]

const R3_RULE = { selector: R3_SELECTOR, message: R3_MESSAGE }
const R5_RULES = R5_SELECTORS.map((selector) => ({
  selector,
  message: R5_MESSAGE,
}))
const ALL_SYNTAX_RULES = [R3_RULE, ...R5_RULES, R8_BARE_DATE_RULE]

const restrictedProperties = (groups) => ['error', ...groups.flat()]

export default [
  { ignores: ['dist/**', 'node_modules/**'] },
  {
    files: ['src/**/*.{ts,tsx}'],
    languageOptions: {
      parser: tseslint.parser,
      ecmaVersion: 2022,
      sourceType: 'module',
    },
    rules: {
      'no-restricted-properties': restrictedProperties([
        R4_PROPERTIES,
        R8_TIME_PROPERTIES,
        R8_RANDOM_PROPERTIES,
        R8_STORAGE_PROPERTIES,
      ]),
      'no-restricted-globals': ['error', ...R8_STORAGE_GLOBALS],
      'no-restricted-syntax': restrictedSyntax(ALL_SYNTAX_RULES),
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
  // ComposeOverlay/MarkdownComposerEditor) burned to zero. The R8 property
  // bans stay live here.
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
    rules: {
      'no-restricted-properties': restrictedProperties([
        R8_TIME_PROPERTIES,
        R8_RANDOM_PROPERTIES,
        R8_STORAGE_PROPERTIES,
      ]),
    },
  },

  // R5 exemption: lib/store.ts is the one blessed implementation (the slice-4
  // burn-down list emptied when the seven hand-rolled copies migrated onto
  // it). R3 and the R8 bare-Date ban stay live here.
  {
    files: ['src/lib/store.ts'],
    rules: {
      'no-restricted-syntax': restrictedSyntax([R3_RULE, R8_BARE_DATE_RULE]),
    },
  },

  // R8 permanent allowlists — the seams themselves. Each seam file keeps
  // every ban that is not its own: the clock file may not touch storage, the
  // storage files may not read the clock, and so on.
  {
    // The clock seam: ambient Date reads live here alone.
    files: ['src/lib/ambient/time.ts'],
    rules: {
      'no-restricted-properties': restrictedProperties([
        R4_PROPERTIES,
        R8_RANDOM_PROPERTIES,
        R8_STORAGE_PROPERTIES,
      ]),
      'no-restricted-syntax': restrictedSyntax([R3_RULE, ...R5_RULES]),
    },
  },
  {
    // The randomness seam: the ambient RNG lives here alone (the clock half
    // of newId comes through lib/ambient/time).
    files: ['src/lib/ambient/random.ts'],
    rules: {
      'no-restricted-properties': restrictedProperties([
        R4_PROPERTIES,
        R8_TIME_PROPERTIES,
        R8_STORAGE_PROPERTIES,
      ]),
    },
  },
  {
    // The storage seams: lib/ambient/storage (single-key, best-effort) and
    // the preferences store's multi-key legacy persistence. store.ts's R4
    // exemption above already covers its window 'storage'-event listener.
    files: ['src/lib/ambient/storage.ts', 'src/data/preferences/storage.ts'],
    rules: {
      'no-restricted-properties': restrictedProperties([
        R4_PROPERTIES,
        R8_TIME_PROPERTIES,
        R8_RANDOM_PROPERTIES,
      ]),
    },
  },
  {
    // data/preferences/store.ts sits in the R4 allowlist AND compares
    // event.storageArea against window.localStorage (the storage-sync
    // filter): R4 off, storage-property ban off, everything else live.
    files: ['src/data/preferences/store.ts'],
    rules: {
      'no-restricted-properties': restrictedProperties([
        R8_TIME_PROPERTIES,
        R8_RANDOM_PROPERTIES,
      ]),
    },
  },
]

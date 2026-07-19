/**
 * R11 import boundaries + the app-is-composition-root rule
 * (docs/client/L2-charter.md). Fail-on-NEW-only: current violations live in
 * .dependency-cruiser-known-violations.json (regenerate with
 * `bun run check:boundaries:baseline`) and are the slice 3-5 burn-down list.
 * `bun run check:boundaries` passes them via --ignore-known.
 */

// Every home may import its own subtree plus the shared homes. Generated wire
// types live outside src/ in the @posthaste/protocol workspace package
// (imported as '@/gen'), so they sit beyond these src-scoped rules.
const SHARED = ['lib', 'domain', 'data']
const HOMES = ['commands', 'components', 'data', 'desktop', 'domain', 'lib', 'surfaces']

const homeRules = HOMES.map((home) => ({
  name: `r11-${home}-boundary`,
  comment: `R11: src/${home} imports only its own subtree, lib/, domain/, data/`,
  severity: 'error',
  from: { path: `^src/${home}/` },
  to: {
    path: '^src/',
    pathNot: `^src/(${[...new Set([home, ...SHARED])].join('|')})/`,
  },
}))

module.exports = {
  forbidden: [
    ...homeRules,
    {
      name: 'r11-nobody-imports-app',
      comment:
        'R11: app/ is the sole composition root; nothing outside app/ imports it',
      severity: 'error',
      from: { pathNot: '^src/app/' },
      to: { path: '^src/app/' },
    },
    {
      name: 'r11-components-no-commands',
      comment: 'R11: commands bind UI to verbs, never the reverse',
      severity: 'error',
      from: { path: '^src/components/' },
      to: { path: '^src/commands/' },
    },
    {
      name: 'no-circular',
      comment: 'Import cycles defeat locality of reasoning (tenet I)',
      severity: 'error',
      from: {},
      to: { circular: true },
    },
  ],
  options: {
    // ../protocol/src/gen is generated wire types and never hand-touched; the
    // generator emits mutually recursive TYPE-ONLY imports (MailQueryGroup <->
    // MailQueryRuleNode) for recursive protocol shapes — erased at runtime and
    // correct by construction, so the package is not followed rather than
    // baselined under no-circular.
    doNotFollow: { path: 'node_modules|^\\.\\./protocol/' },
    tsConfig: { fileName: 'tsconfig.json' },
    tsPreCompilationDeps: true,
    exclude: { path: '\\.test\\.[^.]+$' },
    enhancedResolveOptions: {
      exportsFields: ['exports'],
      conditionNames: ['import', 'require', 'node', 'default', 'types'],
      mainFields: ['module', 'main', 'types', 'typings'],
    },
    reporterOptions: {
      text: { highlightFocused: true },
    },
  },
}

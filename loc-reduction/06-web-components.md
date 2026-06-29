# LOC-reduction audit — web components / app / design / surfaces / command-search / floating-panel-geometry / automation-rules / notifications / actions

Scope (TS/TSX, in-scope dirs under `apps/web/src/`):

| dir | LOC |
|---|---|
| components/ | 16,889 |
| command-search/ | 1,833 |
| app/ | 823 |
| design/ | 721 |
| surfaces/ | 566 |
| floating-panel-geometry/ | 499 |
| automation-rules/ | 385 |
| notifications/ | 182 |
| actions/ | 139 |
| **total** | **~22,040** |

## Headline (read this first)

**This part of the tree is already well-factored. The brief's three biggest hypotheses do not hold:**

1. **"useRuntimeMutation factory could collapse ~300 LOC"** — there are only **11**
   `useMutation` call sites in all of `web/src`, and each has a *genuinely distinct*
   `onSuccess` (different `setQueryData` keys, form-state resets, signature bumps,
   account-fallback logic). The only shared boilerplate is the
   `onError: (e) => setError(e.message)` line (7 sites) and `invalidate…ReadModels`.
   A factory realistically recovers **~25 LOC**, not 300, and at MED risk. (B1)
2. **"oversized files >400 LOC"** — **none** in scope. Largest is
   `useRuntimeMailListView.ts` at 351, then `SettingsPanel.tsx` 308. The tree is
   already split into small files (median well under 150 LOC). Good for context-fit;
   nothing to carve here.
3. **"dead components / unreferenced files"** — a full export-reference scan (520
   exports) and a never-imported-file scan found **2** truly-dead functions (~13 LOC)
   and **0** dead files. The `design/index.ts` "never imported" hit is a false
   positive (it is the `@/design` barrel).

So the realistic safe LOC ceiling for this scope is **~50 LOC** (low-risk) rising to
**~110 LOC** if behaviour-changing/abstraction-adding items (B4, B5) are accepted.
Reported honestly below rather than inflated.

---

## Findings

### DEAD

**D1 | DEAD | `command-search`/`command-palette/model.ts:26-33` | ~8 LOC | low / n**
`currentSearchableServerQuery(query)` — exported, **1 total reference in the repo
(its own definition)**. Delete the function. Verified: `grep -rn currentSearchableServerQuery` → only the def line.

**D2 | DEAD | `notifications/store.ts:55-58` | ~5 LOC | low / n**
`getNotificationsSnapshot()` — exported "for non-React readers and tests", **1 total
reference (the def)**. Note `getSnapshot()` (the `useSyncExternalStore` snapshot) is
identical body and *is* used, so D2 is a pure duplicate-and-unused. Delete the
function + its 1-line jsdoc.

**D3 | DEAD(export-only) | multiple | ~0 net LOC, housekeeping | low / n**
~23 symbols are exported but referenced only inside their own file (used internally,
so the *code* stays — only the `export` keyword is removable, ≈0 LOC but shrinks the
public surface AI has to read). Notable clusters:
`ranker.ts:rankCandidates`, `automationRuleHelpers.ts:{createRuleId,isActionComplete,actionSummary}`,
`thread-list/columns.tsx:{FixedColumnDef,StretchColumnDef,ColumnDef,buildGridTemplate}`,
`command-search/types.ts:{ProviderResultPage,SearchFeatureMap,DecayedCounter,LocalRankingModelSnapshot}`,
`command-search/match.ts:{QueryMatchKind,TextMatchResult,normalizeSearchText}`,
`accountEditorModel.ts:{7 model types}`, `actions/contextualActions.ts:{ActionGroup,ContextualAction}`,
`notifications/store.ts:{NotificationAction,NotificationInput}`,
`message-list/useRuntimeMailListView.ts:RuntimeMailListView`,
`compose-overlay/useForwardAttachments.ts:ForwardAttachmentsResult`,
`app/MailClientView.types.ts:{LayoutValue,LayoutHandler}`,
`WindowChrome.tsx:WINDOW_TRAFFIC_LIGHT_INSET`.
**Not a LOC win — list for surface reduction only.**

**D4 | DEAD(feature stub) | `thread-list/columns.tsx:178-185` | ~6 LOC | low / y**
The `tags` column's `render` returns a permanently-empty `<span/>` (renders nothing —
no tag data is read). It is in `ALL_COLUMNS`/`DEFAULT_COLUMNS`, so it occupies a grid
slot that always shows blank. Either wire it or drop the column def + its two array
entries (~6 LOC, behaviour-change: a blank column disappears).

### DUP / BOILERPLATE

**B1 | DUP | 11 `useMutation` sites (settings-panel + compose-overlay + MailClient) | ~25 LOC | med / n**
A `useRuntimeMutation` factory absorbing the repeated
`onError: (e: Error) => setError(e.message)` (7 sites) and the
`onSuccess → invalidate…ReadModels` tail. Sites:
`SourceMailboxEditor.tsx:55`, `SmartMailboxEditor.tsx:86`, `AccountEditor.tsx:81,108`,
`useAccountCommandMutation.ts:23`, `AccountAppearanceFields.tsx:38`,
`automation-actions/{AutomationRuleEditor.tsx:50,linkedAutomationRules.tsx:52}`,
`accounts-pane/AccountSetupChoice.tsx:31`, `SettingsPanel.tsx:174`,
`compose-overlay/useComposeSubmission.tsx:40`. **Caveat:** each `mutationFn` and most
`onSuccess` bodies are bespoke (verified by reading them), so the factory only
abstracts error + invalidate — modest, and it adds an indirection layer the next
reader must learn. Recommend a thin `onMutationError(setError)` helper rather than a
full factory; lower risk, similar saving.

**B2 | DUP | reinlined `Field` (shared.tsx already exports it) | ~10 LOC | low / n**
`shared.tsx:Field` (label + muted span + `Input`) is reinlined verbatim at
`SourceMailboxEditor.tsx`, `AccountEditor.tsx`, `account-editor/AccountAppearanceFields.tsx`
(the `grid gap-1.5 text-[13px]` + `text-[12px] font-medium text-muted-foreground` span +
`Input`). 3 of the ~6 reinlined `<label>` blocks are plain text inputs that can call
`<Field/>` directly (the rest wrap a `Select`/`Textarea` and can't). ~3-4 LOC each.

**B3 | BOILERPLATE | repeated className literals | ~0 net | low / n**
Top repeats: `text-[13px] font-medium text-foreground` (9×),
`text-[12px] font-medium text-muted-foreground` (8×),
`h-8 rounded-md border-border bg-background text-[13px] shadow-none` (7×, the Field
input), `ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8` (6×). Hoisting to named
`cn` constants is net-neutral on LOC (a const def + reference ≈ the inline string).
**Skip for LOC**; only worth it for consistency, not size.

**B4 | DUP(feature stub) | `onPlaceholderAction` threaded through 6 files | ~14 LOC | med / y**
`onPlaceholderAction(label)` is a no-op-ish stub for unimplemented Reply-all / Forward /
Snooze. It is declared/forwarded in `ActionBar.tsx`, `CommandPalette.tsx`,
`command-palette/usePaletteActions.ts`, `app/MailClientView.tsx`, `app/MailOverlays.tsx`,
`app/MailClientView.types.ts`, `app/MailClient.tsx` (8 prop hops). If product drops the
3 placeholder buttons, the whole prop chain + 3 `ToolbarChip`s + 1 palette action
collapse (~14 LOC across the drilling). **Behaviour-change: those buttons disappear** —
needs a product call, not a mechanical edit.

**B5 | DUP | rule-editor add/remove-row scaffolding | ~25 LOC | med / n**
`ActionListEditor.tsx`, `rule-group/ConditionEditor.tsx`, `RuleGroupEditor.tsx`,
`automation-actions/AutomationRuleList.tsx` each reimplement the same
`onChange([...items, defaultX()])` / `items.filter((_, i) => i !== index)` +
remove-button-row pattern (`onRemove`/`onAdd` props, the trailing `<Button>` with
`Trash2`). A generic `<EditableRowList>` (add button + per-row remove) could fold ~25
LOC. **Trade-off:** adds a generic abstraction over 4 visually-distinct editors; raises
read cost for an unfamiliar reader. Marginal — list, don't rush.

### VERBOSE / OVERSIZED

**V1 | none qualifying.** No in-scope file exceeds 400 LOC. The largest
(`useRuntimeMailListView.ts` 351, `SettingsPanel.tsx` 308, `thread-list/columns.tsx`
294 — a pure data table, not reducible; `AccountEditor.tsx` 294, `app/MailClient.tsx`
294) are all legitimately sized for their responsibility. The glass-theme cluster
(`design/glassTheme.ts` 237 + `appearance/GlassMeshEditor.tsx` 286) is a real feature,
not bloat — reducing it means removing the glass-mesh editor (out of scope for a
LOC audit). **Nothing to split.**

---

## Total estimated LOC saved

| tier | items | est LOC |
|---|---|---|
| Safe, no behaviour change | D1, D2, B2, B1(thin helper) | **~48** |
| Requires product / abstraction call | D4, B4, B5 | **~45** |
| Housekeeping (export-surface, ~0 LOC) | D3 | 0 |
| **Practical ceiling** | | **~90** |

Honest read: there is **no large LOC win hiding in these directories** — they are
already small-file, low-duplication, near-zero-dead-code. The best thing for
"fits AI context better" here is not deletion but the export-surface trim (D3) and
*not* adding more abstraction (B1/B5 cut both ways).

## Top 5 ranked by LOC/risk

1. **D1** `currentSearchableServerQuery` — 8 LOC, low/n. Provably dead (1 ref). Delete.
2. **D2** `getNotificationsSnapshot` — 5 LOC, low/n. Dead + duplicate of `getSnapshot`. Delete.
3. **B2** reinline→`Field` — ~10 LOC, low/n. Use the component that already exists.
4. **B1** thin `onMutationError` helper (not a full factory) — ~25 LOC, med/n. Highest
   raw LOC but adds indirection; prefer the minimal version.
5. **D4** drop the empty `tags` column — ~6 LOC, low/**y** (blank column vanishes).

(B4 and B5 deliberately ranked below the cut: both are real LOC but gated on a product
decision or add an abstraction whose read-cost offsets the saving.)

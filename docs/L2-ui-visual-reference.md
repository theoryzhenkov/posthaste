---
scope: L2
summary: "Precise visual contract for the handoff-matched PostHaste interface"
modified: 2026-04-23
reviewed: 2026-04-23
depends:
  - path: docs/L0-branding
  - path: docs/L0-ui
  - path: docs/L1-ui
  - path: docs/L1-search
  - path: docs/L1-compose
dependents: []
---

# UI Visual Reference -- L2

## Reference Source

This document translates the standalone handoff into implementation rules. The reference is `.claude-design/Posthaste.standalone.bundled.html`, with readable source under `.claude-design/handoff/posthaste/project/src/`.

The handoff default is `theme = dark`, `preset = neutral`, `density = standard`, `layout = 3`. All dimensions below target that state unless noted.

## Global Tokens

Use the token values from [L0-branding](L0-branding.md). No component should introduce unregistered type sizes, icon sizes, radii, or signal colors.

Default typography:

- Body and UI text: Geist.
- Metadata, counters, dates, keyboard hints, code-like values: Geist Mono.
- Display font: Fraunces only for rare display moments, not for the normal app shell.

Default icons:

- Toolbar chip icons: `14px`.
- Sidebar row icons: `14px`.
- Sidebar disclosure chevrons: `12px`.
- Table header flag/attachment icons: `12px`.
- Reader action icons: `14px`.
- Modal close icons: `16px`.

All lucide-style icons use the stroke value attached to their token size.

## Root Shell

The root shell fills the viewport. Embedded mode has no border radius, no outer card shadow, and no decorative background. The root background is `bg`; text defaults to `fg`.

The shell is a vertical flex layout:

- `ActionBar` at the top.
- Main horizontal pane group below.
- Overlays render absolutely above the shell.

The main pane group is horizontal:

- Sidebar: `210px` standard, `180px` compact.
- Splitter: `1px` visible rule, `8px` hit area.
- Message list: `420px` standard, `360px` compact, `520px` in two-pane list-only mode.
- Splitter: same as above.
- Reader: flexes, minimum `280px`.

Pane splitters:

- Background: `border`.
- Cursor: `col-resize`.
- Hover/active overlay: `3px` width, `accent.coral`, centered over the `1px` rule.
- Hit area: `4px` on each side of the visible rule.

## Action Bar

The action bar height is:

- Compact: `38px`
- Standard: `42px`
- Roomy: `46px`

Structure, left to right:

1. Traffic lights.
2. `8px` spacer.
3. Compose chip.
4. Separator.
5. Reply, reply-all, forward chips.
6. Separator.
7. Archive, trash, flag, snooze, tag chips.
8. Flexible spacer.
9. Query search.
10. Shortcut button.
11. Settings button.
12. Theme button.

The action bar background is `bgTitlebar`. It has a `1px` bottom border in `borderSoft`, horizontal padding `12px`, and `4px` gap between adjacent toolbar items.

Traffic lights:

- Three circles, each `12px`.
- Gap: `8px`.
- Colors: `#ff5f57`, `#febc2e`, `#28c940`.
- Each circle has `inset 0 0 0 0.5px rgba(0,0,0,0.2)`.

Toolbar chips:

- Height: `28px`.
- Border: none.
- Radius: `6px`.
- Font: Geist `12px`, weight `500`.
- Icon size: `14px`.
- Gap between icon and label: `5px`.
- Icon-only padding: `0 6px`.
- Label chip padding: `0 9px 0 8px`.
- Inactive foreground: `fgMuted`.
- Hover background: `hoverBg`.
- Active background: `accent.coralSoft`.
- Active foreground: `accent.coralDeep`.
- Transition: background over about `0.1s`.

Keyboard hints inside labeled toolbar chips:

- Font: Geist Mono `11px`.
- Opacity: `0.6`.
- Padding: `1px 4px`.
- Radius: `3px`.
- Background: `rgba(128,128,128,0.12)`.
- Left margin: `2px`.

Separators:

- Width: `1px`.
- Height: `18px`.
- Background: `borderSoft`.
- Margin: `0 6px`.

Icon-only utility buttons for shortcuts, settings, and theme:

- Size: `28px` square.
- Radius: `5px`.
- Background: transparent.
- Foreground: `fgMuted`.
- Shortcut button uses Geist Mono `13px`, weight `700`.
- Settings and theme icons use `14px`.

## Query Search

Resting search:

- Width: `220px`.
- Height: `26px`.
- Radius: `6px`.
- Padding: `0 8px`.
- Gap: `6px`.
- Background: `bgElev`.
- Border: `1px solid borderSoft`.
- Cursor: text if it opens command/search.
- Icon: search, `12px`, `fgFaint`.
- Label: `Search mail`, Geist `12px`, `fgFaint`.
- Right hint: `Cmd+K`, Geist Mono `11px`, `fgFaint`, padding `1px 5px`, radius `3px`, background `bg`, border `1px solid borderSoft`.

Focused or non-empty search:

- Width: `340px`.
- Border: `1px solid focusRing`.
- Box shadow: `0 0 0 2px color-mix(in oklab, focusRing 30%, transparent)`.
- Input font: Geist Mono `12px`.
- Input text color: `fg`.
- Placeholder: `from:maya tag:work date:>2026-04-01`.

## Sidebar

The sidebar fills its pane and scrolls vertically with class `ph-scroll`.

Base:

- Background: `bgSidebar`.
- Right border: `1px solid border`.
- Width: `100%`.
- Bottom padding: `12px`.

Section order:

1. Quick filters: `All Inboxes`, `Flagged`.
2. `Smart`.
3. `Tags`.
4. `Accounts`.

Top quick filter block:

- Padding: `10px 0 2px`.
- `All Inboxes` uses `Icons.All`, count, and `accent.coral`.
- `Flagged` uses `Icons.Flag`, count, and `signal.flag`.

Section headers:

- Padding: `14px 14px 6px`.
- Font: Geist Mono `11px`, weight `600`.
- Color: `fgFaint`.
- Letter spacing: `0.6px`.
- Text transform: uppercase.
- Display: flex, align center, justify between.
- `Accounts` header adds `margin-top: 8px`.

Smart section header add button:

- Size: `16px` square.
- Icon: plus, `12px`.
- Background: transparent.
- Default color: `fgFaint`.
- Hover color: `accent.coral`.
- Hover background: `hoverBg`.
- Radius: `3px`.

Sidebar rows:

- Role: treeitem.
- Focusable.
- Height: `24px` compact, `28px` standard, `32px` roomy.
- Margin: `0 6px`.
- Padding: `0 8px 0 calc(8px + depth * 14px)`.
- Gap: `8px`.
- Radius: `5px`.
- Font: Geist `13px`.
- Weight: `500`, or `600` when selected.
- Default background: transparent.
- Hover background: `hoverBg`.
- Selected background: `selBg`.
- Default foreground: `fg`.
- Selected foreground: `selFg`.
- Focus ring: `0 0 0 2px focusRing`.
- User select: none.

Sidebar row left structure:

- A `14px` spacer precedes the icon. This aligns rows with account children and disclosure affordances.
- Icon size is `14px`.
- Unselected icon color is row accent if supplied, otherwise `fgMuted`.
- Selected icon color is `selFg`.
- Label truncates with ellipsis.

Sidebar row counters:

- Display only when count is greater than `0`.
- Font: Geist Mono `11px`, weight `600`.
- Unselected color: `fgFaint`.
- Selected color: `selFg`.
- Quick row counters are plain text, not filled pills.

Smart rows:

| Mailbox | Icon | Accent |
|---|---|---|
| Relevant | `Sparkle` | `accent.coral` |
| Read Later | `Snooze` | `accent.amber` |
| Bills | `Tag` | `accent.violet` |
| Newsletters | `Layers` | `accent.sage` |
| Today | `Bolt` | `accent.blue` |

When a smart row has an edit handler and is hovered, the count is replaced by an edit rules button:

- Size: `18px`.
- Icon: sliders, `11px`.
- Radius: `3px`.
- Color: `fgFaint`, or `selFg` if selected.
- Background: transparent.

Tags:

- Use tag icon at `14px`.
- Icon color is the tag color.
- Tag rows do not show counts unless backend provides them.

Account headers:

- Height: `30px` standard, `26px` compact.
- Margin: `6px 6px 2px`.
- Padding: `0 10px`.
- Gap: `8px`.
- Radius: `5px`.
- Cursor: pointer.
- Disclosure button: `14px`, transparent, `fgMuted`, opacity `0.7`, rotated `90deg` when expanded.
- Stamp: `18px` square, radius `4px`, account color background, white text, Geist Mono `11px`, weight `700`.
- Account label: Geist `12px`, weight `700`, color `fg`.
- Unread pill: background `signal.unread`, color `#fff`, Geist Mono `11px`, weight `700`, padding `1px 6px`, radius `6px`, min width `18px`, centered.

Account mailbox children:

- Use `depth = 1`.
- Use mailbox role icons.
- Use normal row counter treatment, not account-header filled pills.

## Message List

The message list is a tabular conversation list in standard and compact densities. It is not a card list.

Pane:

- Background: `bgList`.
- Right border: `1px solid border`.
- Overflow hidden.
- Role: `listbox`.

Default columns:

| ID | Width | Resizable | Label | Alignment |
|---|---:|---|---|---|
| `unread` | `28px` | No | empty | center |
| `flag` | `28px` | No | flag icon | center |
| `attach` | `28px` | No | attachment icon | center |
| `subject` | `320px` | Yes | `Subject` | left |
| `from` | `180px` | Yes | `From` | left |
| `date` | `128px` | Yes | `Date Received` | left |
| `account` | `72px` | Yes | `Account` | right |
| `tags` | `140px` | Yes | `Tags` | left |

Minimum widths:

- `subject`: `120px`
- `from`: `80px`
- `date`: `80px`
- `account`: `54px`
- `tags`: `60px`

The last column stretches to absorb extra pane width when the pane is wider than the raw column total plus dividers.

Header:

- Height: `26px`.
- Display: flex.
- Background: `bgTitlebar`.
- Bottom border: `1px solid borderStrong`.
- Sticky at top.
- Z-index: `3`.
- Width equals total effective column width.

Header cells:

- Width: column width.
- Padding: `0 10px`.
- Font: Geist Mono `11px`, weight `600`.
- Text transform: uppercase.
- Letter spacing: `0.5px`.
- Default color: `fgFaint`.
- Sorted color: `fg`.
- Sortable hover color: `fgMuted`.
- Sortable hover background: `hoverBg`.
- Display: flex, align center, gap `4px`.
- Justification follows alignment.
- Sorted indicator is `↑` or `↓` with opacity `0.8`.
- Flag and attachment headers render icons at `12px`.

Column dividers:

- Width: `1px`.
- Height: `60%` of header.
- Background: `borderSoft`.
- Cursor: `col-resize`.
- Hit area: `4px` on each side and `50%` above/below.
- Hover/active highlight: `3px` coral line, extending `20%` beyond divider height.

Rows:

- Height: `24px` compact, `30px` standard, `48px` roomy.
- Width: total effective column width.
- Display: flex.
- Background: selected `selBg`; hover `hoverBg`; zebra odd `bgListAlt`; otherwise `bgList`.
- Text color: selected `selFg`; unread `fg`; read `fgMuted`.
- Font: Geist `13px` standard, `12px` compact.
- Weight: `600` unread, `400` read.
- Focus state: inset `0 0 0 2px focusRing`.
- Cursor: pointer.

Row cells:

- Width equals header column width.
- Fixed columns `unread`, `flag`, `attach` have no horizontal padding.
- Other cells use `0 10px`.
- Display: flex, align center.
- Gap: `6px`.
- Overflow hidden; text truncates.

Unread dot:

- Size: `7px`.
- Radius: circular.
- Unselected background: `signal.unread`.
- Selected background: `selFg`.

Flag icon:

- Size: `12px`.
- Unselected color: `signal.flag`.
- Selected color: `selFg`.

Attachment icon:

- Size: `12px`.
- Unselected color: `fgFaint`.
- Selected color: `selFg`.

Thread badge:

- Font: Geist Mono `11px`, weight `600`.
- Text color: selected `selFg`, otherwise `fgMuted`.
- Background: selected `rgba(255,255,255,0.15)`, otherwise `bgElev`.
- Border: selected transparent, otherwise `1px solid borderSoft`.
- Padding: `0 5px`.
- Radius: `3px`.
- Line height: `14px`.
- Minimum width: `18px`.
- Centered text.
- Appears before the subject when `threadCount > 1`.

Subject column:

- Shows thread badge first when present.
- Subject text truncates.
- Subject uses row font and row weight.
- The standard tabular handoff does not show preview text in the same row.

From column:

- Shows sender display name.
- Truncates.
- Uses row font and row color.

Date column:

- Font: Geist Mono `11px`.
- Color: selected `selFg`, otherwise `fgSubtle`.
- Shows full date label from the data, for example `Yesterday, 18:04`.

Account column:

- Font: Geist Mono `11px`.
- Color: selected `selFg`, otherwise `fgFaint`.
- Text maps accounts to compact labels such as `Gmail`, `Work`, `Univ.`.

Tags column:

- Up to three tags.
- Gap: `4px`.
- Tag pill font: Geist Mono `11px`, weight `600`.
- Text color: selected `selFg`, otherwise tag color.
- Background: selected `rgba(255,255,255,0.12)`, otherwise `color-mix(in oklab, tagColor 14%, transparent)`.
- Padding: `0 5px`.
- Radius: `3px`.
- Line height: `14px`.
- Label truncates to the first four characters in tabular mode.

Roomy density:

- Uses a two-line card-like row instead of tabular columns.
- Padding: `10px 12px`.
- Gap: `10px`.
- Border bottom: `1px solid borderSoft`.
- Contains unread dot rail, sender/date line, subject line, preview line, and full tag chips.

## Reader Pane

The reader pane is the right-hand article surface.

Empty state:

- Full width and height.
- Background: `bgReader`.
- Centered flex column.
- Gap: `14px`.
- Color: `fgFaint`.
- Stamp: `72px`, color `fgFaint`, text `NO MSG`, date `SELECTED`.
- Text: `Select a message to read`, Geist `13px`, weight `500`.
- Navigation hint: Geist Mono `11px`, includes `J` and `K` kbd pills.
- Kbd pills: padding `1px 5px`, radius `3px`, border `1px solid borderSoft`, background `bgElev`.

Loaded state:

- Role: `article`.
- Background: `bgReader`.
- Color: `fg`.
- Overflow: auto.
- Uses `ph-scroll`.

Reader header:

- Padding: `16px 20px 12px`.
- Bottom border: `1px solid borderSoft`.
- Position: relative.
- Header inner row: flex, align start, gap `12px`.

Subject:

- Font: Geist `17px`.
- Weight: `600`.
- Color: `fg`.
- Letter spacing: `-0.2px`.
- Margin bottom: `8px`.
- Line height: `1.25`.

Sender block:

- Avatar size: `28px` circular.
- Avatar background: `color-mix(in oklab, accent.coral 40%, bg)`.
- Avatar text: white, Geist `12px`, weight `700`.
- Sender row gap: `10px`, wraps when needed.
- Sender name: Geist `13px`, weight `600`, color `fg`.
- Sender email: Geist Mono `11px`, color `fgMuted`, wrapped in angle brackets.
- Recipient/date line: Geist Mono `11px`, color `fgMuted`, margin top `2px`.
- Recipient/date format: `to <recipient> · <date>`.

Reader action buttons:

- Size: `28px` square.
- Radius: `5px`.
- Background: transparent.
- Hover background: `hoverBg`.
- Color: `fgMuted`.
- Icon size: `14px`.
- Focus ring: `0 0 0 2px focusRing`.
- Actions: reply, forward, archive, more.

Reader tag strip:

- Display only when the message has tags.
- Margin top: `10px`.
- Gap: `6px`.
- Tag font: Geist Mono `11px`, weight `600`.
- Color: tag color.
- Background: `color-mix(in oklab, tagColor 14%, transparent)`.
- Padding: `2px 7px`.
- Radius: `4px`.
- Each tag contains a `5px` circular dot in tag color plus label.

Attachment strip:

- Display only when attachments exist.
- Padding: `10px 20px`.
- Bottom border: `1px solid borderSoft`.
- Background: `bgElev`.

Attachment rows:

- Display: flex, align center.
- Gap: `10px`.
- Padding: `8px 10px`.
- Background: `bg`.
- Border: `1px solid border`.
- Radius: `6px`.
- Margin bottom: `6px` except last row.

Attachment type tile:

- Size: `32px` square.
- Radius: `4px`.
- Text: uppercase Geist Mono `11px`, weight `700`, white.
- `pdf`: `accent.rose`.
- `image`: `accent.violet`, label `img`.
- `ai`: label `ai`.
- Other files: `accent.blue`.

Attachment text:

- Filename: Geist `12px`, weight `500`, color `fg`, single line ellipsis.
- Size: Geist Mono `11px`, color `fgFaint`.
- Actions: download and more icon buttons, using the reader action button treatment.

Reader body:

- Padding: `18px 22px 28px`.
- Font: Geist `13px`.
- Line height: `1.6`.
- Color: `fg`.
- White space: pre-wrap for plain text.
- Max width: `720px`.
- Body does not center itself in the handoff; it begins at the left padding. If HTML iframe constraints require centering for compatibility, the visual result must still preserve the `720px` readable width and avoid full-pane white slabs.
- Links use `accent.coralDeep`, no underline, with `1px dotted accent.coral` bottom border.

## Command Palette

The command palette opens from `Cmd/Ctrl+K` and the search control.

Overlay:

- Absolute inset `0`.
- Z-index: `2500`.
- Align top center.
- Padding top: `9%`.
- Background: `rgba(6,4,12,0.4)`.
- Backdrop filter: `blur(22px) saturate(150%)`.
- Animation: modal fade in around `0.16s`.

Palette sheet:

- Width: `640px`.
- Max width: `92vw`.
- Background: `rgba(22,20,28,0.88)`.
- Border: `1px solid rgba(255,255,255,0.08)`.
- Radius: `14px`.
- Shadow: `0 28px 80px rgba(0,0,0,0.6)`.
- Overflow: hidden.
- Text color: `fg`.
- Font: Geist.

Input row:

- Display: flex, align center.
- Padding: `0 16px`.
- Bottom border: `1px solid borderSoft`.
- Search icon: `18px`, `fgMuted`.
- Input height: `48px`.
- Input padding: `0 12px`.
- Font size: `16px`.
- Placeholder: `Search messages, contacts, commands...`.
- Right kbd: `Esc`.

Results:

- Max height: `440px`.
- Padding: `6px 0`.
- Group label: padding `4px 16px`, Geist Mono `11px`, uppercase, letter spacing `0.7px`, weight `600`, color `fgFaint`.
- Row: full width, display flex, align center, gap `10px`, padding `8px 16px`, border none.
- Active row background: `rgba(255,255,255,0.08)`.
- Row font: Geist `13px`.
- Icon: `15px`, `fgMuted`.
- Subtext: Geist `12px`, `fgMuted`, max width `240px`.

Footer:

- Padding: `8px 16px`.
- Top border: `1px solid borderSoft`.
- Font: Geist Mono `11px`.
- Color: `fgFaint`.
- Shows `Up/Down navigate`, `Enter select`, `Esc close`, and `posthaste`.

## Settings Sheet

Settings opens as a centered sheet, not a route replacement.

Backdrop:

- Absolute inset `0`.
- Z-index: `2100`.
- Background: dark `rgba(6,4,12,0.55)`, light `rgba(40,30,60,0.35)`.
- Backdrop filter: `blur(18px) saturate(140%)`.
- Display: flex, center.
- Padding: `32px`.
- Animation: modal fade in around `0.18s`.

Sheet:

- Width: `100%`.
- Max width: `1080px`.
- Height: `100%`.
- Max height: `760px`.
- Background: dark `rgba(22,20,28,0.88)`, light `rgba(253,252,250,0.94)`.
- Border: `1px solid border`.
- Radius: `16px`.
- Dark shadow: `0 32px 80px rgba(0,0,0,0.55), 0 0 0 1px rgba(255,255,255,0.04) inset`.
- Display: flex.
- Overflow: hidden.
- Font: Geist.

Rail:

- Width: `220px`.
- Flex shrink: `0`.
- Background: `rgba(0,0,0,0.12)`.
- Right border: `1px solid borderSoft`.
- Padding: `16px 0`.
- Header padding: `0 16px 14px`.
- Header uses `PostmarkStamp` at `20px` in `accent.coral`.
- Header title: Geist `17px`, weight `700`, letter spacing `-0.3px`.
- Footer: pinned bottom, padding `12px 16px`, Geist Mono `11px`, color `fgFaint`, text `v1.0.0 · JMAP 0.3`.

Rail items:

- Sections: Accounts, Mailboxes & Rules, Appearance, Signatures, Privacy & Tracking, Keyboard, Automation, About.
- Display: flex, align center, gap `10px`.
- Padding: `8px 14px`.
- Margin: `1px 8px`.
- Radius: `7px`.
- Border: none.
- Font: Geist `13px`.
- Weight: `600` active, `500` inactive.
- Inactive background: transparent.
- Inactive text: `fg`.
- Inactive icon: `fgMuted`.
- Active background: `accent.coralSoft`.
- Active text/icon: `accent.coralDeep`.
- Icon size: `15px`.

Content:

- Flex column.
- Header row: display flex, align center, padding `14px 22px`, bottom border `1px solid borderSoft`.
- Title: Geist `17px`, weight `700`, letter spacing `-0.3px`.
- Close button: `30px` square, radius `8px`, transparent, `fgMuted`, icon `16px`.
- Scroll body: flex `1`, overflow auto, padding `22px`.

Settings account rows:

- Section label: Geist Mono `11px`, uppercase, weight `600`, color `fgFaint`, letter spacing `0.7px`.
- Account list gap: `10px`, margin bottom `20px`.
- Row display: flex, align center, gap `12px`.
- Row padding: `14px`.
- Background: `bgElev`.
- Radius: `10px`.
- Border: `1px solid borderSoft`.
- Stamp: `38px` square, radius `8px`, account color background, white text, Geist Mono `13px`, weight `700`.
- Account label: Geist `13px`, weight `600`.
- Meta: Geist Mono `11px`, `fgMuted`, text like `JMAP · sync ok · 12 mailboxes`.
- Actions: ghost modal buttons `Edit`, `Remove`.
- Add account: primary modal button with plus icon.

Smart mailbox rows:

- Header row has section label left and primary `New smart mailbox` button right.
- Row display: flex, align center, gap `12px`.
- Padding: `12px`.
- Background: `bgElev`.
- Radius: `10px`.
- Border: `1px solid borderSoft`.
- Cursor: pointer.
- Icon tile: `30px` square, radius `8px`, background `color-mix(in srgb, accent 20%, transparent)`, foreground matching the smart mailbox accent, icon `14px`.
- Name: Geist `13px`, weight `600`.
- Summary: Geist Mono `11px`, `fgMuted`, text like `5 unread · 3 match conditions · 2 actions`.
- Chevron: `14px`, `fgFaint`.

Appearance rows and other settings form rows:

- Display: flex row unless stacked.
- Gap: `16px`.
- Padding: `12px 0`.
- Bottom border: `1px solid borderSoft`.
- Label column width: `180px`.
- Label: Geist `13px`, weight `500`, color `fg`.
- Hint: Geist `11px`, color `fgMuted`, margin top `3px`, line height `1.4`.
- Control area flexes.

Modal buttons:

- Height: `32px`.
- Padding: `0 14px`.
- Radius: `8px`.
- Gap: `6px`.
- Font: Geist `12px`, weight `600`.
- Primary background: `accent.coral`, text `#fff`.
- Primary border: `color-mix(in srgb, black 12%, transparent)`.
- Primary shadow: `0 1px 0 rgba(255,255,255,0.2) inset, 0 2px 6px rgba(0,0,0,0.12)`.
- Secondary background: `bgElev`, text `fg`, border `border`.
- Ghost background: transparent, text `fg`.
- Danger background: transparent, text and border `accent.rose`.

Inputs and selects:

- Height: `32px`.
- Padding: inputs `0 10px`; selects `0 28px 0 10px`.
- Border: `1px solid border`.
- Radius: `8px`.
- Background: `bg`.
- Text: `fg`.
- Font: Geist `13px`, or Geist Mono when marked mono.
- Focus border: `focusRing`.
- Focus shadow: `0 0 0 3px color-mix(in srgb, focusRing 20%, transparent)`.

Switches:

- Track: `34px` by `20px`, radius `999px`.
- Checked background: `accent.coral`.
- Unchecked background: `border`.
- Thumb: `16px`, white, top `2px`, left `16px` checked or `2px` unchecked.
- Thumb shadow: `0 1px 3px rgba(0,0,0,0.2)`.

## Mailbox Editor

Mailbox editor uses the shared modal primitive.

Modal:

- Width: `860px`.
- Height: `640px`.
- Backdrop and sheet treatment from the shared modal primitive.

Header:

- Padding: `16px 22px 12px`.
- Bottom border: `1px solid borderSoft`.
- Display: flex, align center, gap `12px`.
- Includes icon picker, inline name input, type label, and close button.
- Name input: Geist `17px`, weight `700`, letter spacing `-0.3px`, transparent background.
- Type label: Geist Mono `11px`, `fgMuted`, `SMART MAILBOX` or `MAILBOX RULES`.
- Close button: `30px` square, radius `8px`, icon `16px`.

Tabs:

- Present for smart mailboxes.
- Padding: `0 22px`.
- Bottom border: `1px solid borderSoft`.
- Tab padding: `10px 14px`.
- Active text: `fg`, weight `600`.
- Inactive text: `fgMuted`, weight `500`.
- Active underline: `2px solid accent.coral`.

Match mode segmented control:

- Aligns right in the tab row.
- Background: `bg`.
- Border: `1px solid borderSoft`.
- Radius: `7px`.
- Padding: `3px`.
- Segment padding: `3px 10px`.
- Font: Geist Mono `11px`, uppercase, letter spacing `0.5px`, weight `600`.
- Active background: `bgElev`.

Editor content:

- Padding: `20px 22px`.
- Scrolls with `ph-scroll`.
- Condition/action rows use `bgElev`, `borderSoft`, radius `10px`.
- Footer padding: `14px 22px`, top border `1px solid borderSoft`.
- Footer hint: Geist Mono `11px`, `fgMuted`.

## Compose Modal

Compose is a centered modal with a mail-window header.

Backdrop:

- Absolute inset `0`.
- Background: `rgba(10,8,6,0.5)`.
- Backdrop filter: `blur(4px) saturate(140%)`.
- Z-index: `100`.

Sheet:

- Width: `700px`.
- Max width: `92%`.
- Max height: `92%`.
- Background: `bgReader`.
- Radius: `12px`.
- Border: `1px solid border`.
- Shadow: `0 28px 80px rgba(0,0,0,0.55), 0 0 0 1px rgba(255,255,255,0.03) inset`.
- Flex column.

Header:

- Height: `40px`.
- Padding: `0 14px`.
- Gap: `10px`.
- Background: `bgTitlebar`.
- Bottom border: `1px solid borderSoft`.
- Top radius: `12px`.
- Traffic lights: `12px`, colors match action bar.
- Center title: `New Message`, Geist `12px`, weight `600`, color `fgMuted`, letter spacing `0.2px`.

Fields:

- Field rows use compact label-plus-control layout.
- From stamp is `14px` square, radius `3px`, account color, white Geist Mono `9px`, weight `700`.
- Recipient inputs use Geist `13px`, transparent background.
- Subject input uses Geist `13.5px`, weight `500`.

Body:

- Textarea minimum height: `220px`.
- Padding: `14px 16px 8px`.
- Font: Geist `13.5px`.
- Line height: `1.55`.
- Background: `bgReader`.

Footer:

- Padding: `10px 12px`.
- Gap: `6px`.
- Top border: `1px solid borderSoft`.
- Background: `bgElev`.
- Bottom radius: `12px`.
- Send button is coral, with `Cmd+Enter` kbd hint.
- Tool buttons include attach, AI assist, template, follow-up, tracking, encrypt.

Backend gaps may disable schedule, tracking, encryption, templates, and AI controls, but the visual placement should remain reserved.

## Shared Modal Primitive

Backdrop:

- Absolute inset `0`.
- Z-index: `2000`.
- Display center.
- Dark background: `rgba(6,4,12,0.55)`.
- Light background: `rgba(40,30,60,0.35)`.
- Backdrop filter: `blur(18px) saturate(140%)`.

Sheet:

- Max width: `calc(100% - 48px)`.
- Max height: `calc(100% - 48px)`.
- Dark background: `rgba(22,20,28,0.82)`.
- Light background: `rgba(255,255,254,0.85)`.
- Backdrop filter: `blur(24px) saturate(180%)`.
- Border: dark `1px solid rgba(255,255,255,0.09)`, light `1px solid rgba(20,18,28,0.08)`.
- Radius: `16px`.
- Dark shadow: `0 32px 80px rgba(0,0,0,0.55), 0 0 0 1px rgba(255,255,255,0.04) inset`.
- Flex column, overflow hidden.

Modal header:

- Padding: `18px 22px 16px`.
- Gap: `12px`.
- Bottom border: `1px solid borderSoft`.
- Optional icon tile: `36px` square, radius `10px`, background `accent.coralSoft`, color `accent.coralDeep`.
- Title: Geist `17px`, weight `700`, letter spacing `-0.3px`.
- Subtitle: Geist `12px`, `fgMuted`, margin top `2px`.
- Close: `30px` square, radius `8px`, icon `16px`.

Modal footer:

- Padding: `14px 22px`.
- Gap: `8px`.
- Top border: `1px solid borderSoft`.
- Background: `color-mix(in srgb, currentColor 2%, transparent)`.

## Status Bar

The handoff includes a status bar component even though the current shell may omit it.

Status bar:

- Height: `24px`.
- Padding: `0 12px`.
- Gap: `16px`.
- Top border: `1px solid borderSoft`.
- Background: `bgSidebar`.
- Font: Geist Mono `11px`.
- Color: `fgMuted`.
- Left text: `<count> messages · <unread> unread`.
- Sync indicator: `6px` sage dot plus `JMAP · Fastmail · sync ok`.
- Right account label.

## Scrollbars

Elements with `ph-scroll` use thin custom scrollbars:

- Scrollbar width/height: `8px`.
- Track: transparent.
- Thumb: `rgba(128,128,128,0.25)`.
- Thumb radius: `4px`.
- Hover thumb: `rgba(128,128,128,0.4)`.

## Assertions

| ID | Sev. | Assertion |
|---|---|---|
| default-dark-neutral | MUST | The default shell uses dark neutral tokens, not the old light-first theme or the glass preset |
| locked-type-ramp | MUST | UI components use only the documented type ramp unless this spec is updated |
| signal-separation | MUST | Coral, blue, and slate-blue remain separate brand/flag, unread, and selection signals |
| actionbar-order | MUST | The action bar uses the reference control order from traffic lights through theme toggle |
| sidebar-order | MUST | The sidebar section order is quick filters, Smart, Tags, Accounts |
| account-counter-blue | MUST | Account-header unread counters use `signal.unread` as a filled blue pill with white text |
| message-list-tabular | MUST | Standard density message rows are tabular rows, not card rows |
| row-height-standard | MUST | Standard density message rows are `30px` high |
| header-row-alignment | MUST | Message list header cells and row cells share the same effective column widths |
| reader-body-width | MUST | Reader body content uses a `720px` maximum readable width |
| settings-centered-sheet | MUST | Settings opens as a centered modal sheet over the live app shell |
| overlays-glass-only | SHOULD | Glass blur is reserved for overlays and modals, not the default app background |

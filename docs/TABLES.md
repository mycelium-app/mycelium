# Tables: archaeology and a single design

Written 2026-10-26 from `git log`, PLAN.md, HANDOVER.md and the code. Nothing was built or run; "unverified" marks inference from reading code.

## 1. History

| Date | Commit | Symptom | Root cause | Fix | Came back? |
|---|---|---|---|---|---|
| 2026-10-22 | 294588a, 270180a | Markdown tables in agent messages added; cells did not line up | each table laid out cells by hand | `TableBlock` with measured natural widths, per-column alignment | yes, next day |
| 2026-10-23 | 9b64db6 | A table whose text wraps left spare width unused; a short table's border spanned the thread | columns capped at `Tokens.Size.TableCell` with no redistribution of the spare width; border on a full-width box | capped columns grow into the spare width; `IntrinsicSize.Max` so the border hugs columns | yes, same day |
| 2026-10-23 | 2cc10ed | Settings tables (Accounts, Agents, Forges, Instructions, Issues, Models, Projects, Schedules, Sessions) wrapped, truncated or sat in different layouts | every screen hand-rolled `Row` + `weight` | introduced `ContentTable`/`ContentRow`/`ColumnSpec(min,max,flexible)`: columns sized to content, main column shrinks first | yes |
| 2026-10-23 | 0f4a80e | Chat models table not in a card, no space under its title | one-off layout | wrapped in card | no |
| 2026-10-23 | d41b9d5 | Screens differed from design boards (card tables) | per-screen styling | card tables | no |
| 2026-10-23 | 712d29b | History table (Airtable-like) added with its own `HistoryTable`, `FixedWidthColumns`, `LocalResolvedWidths`, `rememberTextMeasurer` width measuring | `ContentTable` measures by intrinsics, which cannot contain a `LazyColumn`; History needs lazy rows | second layout path: widths measured with a text measurer and passed in | yes |
| 2026-10-24 | b7c434e | History rows drifted out of line when a new row widened a column | natural widths recomputed per item as lazy rows composed | widths measured over all rows up front (`HistoryTableTest.rowsAlreadyShownStayInLine...`) | partly |
| 2026-10-23 | PLAN.md "Not done and not recorded" (reported 13:33, after 9b64db6) | "the row lines do not reach the border, and the table does not fit its content" | never diagnosed; the entry says check on v0.3.9+ before changing anything. Unverified candidates: in `TableBlock` the `Hairline`s and the border follow `IntrinsicSize.Max` while cell widths come from a separate `BoxWithConstraints` computation, so the two disagree; in `ContentTable` rows are padded by `gap` on both sides but the available width is computed on the whole, so natural-width columns plus the flexible remainder can leave a strip | none | open |
| 2026-10-25 | 30feba4 / 3a46dfd, HANDOVER.md | Crash: a model list inside `ContentTable` used `LazyColumn`; `ContentTable` calls `maxIntrinsicWidth` on every child and lazy layouts throw on intrinsics | intrinsic measurement is the design's core and it cannot see lazy content | the browser was moved outside `ContentTable` (`ModelProvidersSettings.kt` ~189, with a fixed `height(minOf(280, n*32))`) | design flaw remains |
| 2026-10-26 | operator report | "more line wrap issues" | see section 3 | none yet | open |

Tests that exist: `ContentTableTest` (2: short column narrower and one-line; flexible column shrinks first), `MarkdownTableTest` (3), `HistoryTableTest` (alignment, sort, filter, group). None render at a narrow width and assert single-line cells, none asserts row lines span the border, and `DataTable`, `Assistants`, `Usage`/`Activity` have no table test.

## 2. Current state

Three independent layout engines plus hand-rolled rows.

| Component | Where | Column sizing | Lazy? | Scroll | Wrap/truncate |
|---|---|---|---|---|---|
| `ContentTable` | `ContentTable.kt` | intrinsic natural width clamped to `[min 56dp, max 240dp]`; one `flexible` column absorbs spare and shrinks first; if the sum still overflows, all columns are scaled proportionally below their minimum | no (intrinsics) | none | per cell, caller's `maxLines`; `Label` default is unlimited lines with `Clip` |
| `FixedWidthColumns` | same file | same resolver, widths supplied by caller from a text measurer | yes | none | per cell |
| `HistoryTable` | `HistoryTable.kt` | measured widths, user-resizable (56-600dp), Title flexible; touch mode is 2 columns | `LazyColumn` | none | per cell |
| `DataTable` | `Metrics.kt` | wraps `ContentTable`; first column flexible, `maxLines = 1`, optional 72dp trailing | no | none | single line, ellipsis |
| `TableBlock` | `ThreadView.kt` | own algorithm: `TableCell` cap, spare redistributed, `IntrinsicSize.Max` | no | `horizontalScroll` (only table with it) | wraps (intended for prose) |
| `TableCard`/`tableHeader()` | `Components.kt` | none, a bordered Column | n/a | none | n/a |
| `GeneratedUi` table node | `GeneratedUi.kt` | reuses `TableBlock` | no | yes | wraps |

`ContentTable` users (each declares a `ColumnSpec` list inline): Projects (4 cols), Accounts (3), Memory banks (4), Chat models (5), Assistants (7), Chat tools (5), Import (5), Assistant settings (3 tables: 3, 5, 5 cols), Issues (5), Sessions (2 tables, 4 cols), Groups (4), Forges (4), Schedules (2 tables: 6, 3), Instructions (4).
`DataTable` users: Node page (6 tables), Activity (6 tables).
`HistoryTable`: Archive screen.
Own row layouts not on the shared table: Workflows list (`MyButton` per row, flagged in UI-AUDIT.md), Nodes (`NodeGroup`), Tasks (board, intended), Artefacts (card grid, intended), Models browser (a `LazyColumn` of labels), Git branch pickers, Usage card.

## 3. Root causes

1. Widths come from intrinsic measurement (`maxIntrinsicWidth(Infinity)` over every child). Consequences: lazy content crashes it (the HANDOVER crash), so `HistoryTable` built a second engine; the measurement is a side effect written into a mutable `IntArray` from the intrinsic pass, which is order-dependent; double measurement cost.
2. Minimums are advisory. `resolveColumnWidths` ends with a proportional scale of every column below its `min` when the sum of minimums exceeds the width, so narrow windows crush columns (1-2 characters wide) and text wraps character by character. There is no horizontal scroll to fall back on.
3. `ColumnSpec.min` is a flat 56dp for every column, regardless of the column's content (a date, a status, an ID, a number all have a real minimum).
4. Truncate-or-wrap is the caller's choice per `Label`, and the default is unlimited lines with `Clip` (no ellipsis). Any cell that forgets `maxLines = 1`, or contains a `Row` of chips/buttons rather than one `Label`, wraps. Cells that are not text (buttons, badges) have no policy at all.
5. The max cap (240dp default) and "one flexible column" are global guesses: a table with two long text columns (Import, Chat tools, Assistants) has one flexible and the other clipped or capped, and with two `flexible` columns only the first absorbs spare (`flexibleIndex = indexOf(true)`).
6. Row lines and borders are separate children of a Column-like layout, and the border, card background, gap padding and row widths are computed in different places (and differently in `TableBlock`, `ContentTable`, `DataTable`, `TableCard`), so lines and column edges disagree (the open PLAN.md item).
7. Four implementations (`ContentTable`, `FixedWidthColumns`, `TableBlock`, `HistoryTable`) mean each fix lands in one and the other bugs persist: 9b64db6 fixed Markdown tables only, 2cc10ed fixed settings tables only.
8. Numbers use proportional figures and no right alignment, so numeric columns jitter (Usage, Activity, History tokens/cost).
9. Test coverage is two cases at one width.

## 4. Proposal: one `Table`

One component, `mycelium/ui/Table.kt`, replacing `ContentTable`, `FixedWidthColumns`, `DataTable`, `TableBlock` layout and `HistoryTable`'s layout (HistoryTable keeps sorting/resizing/grouping as features on top).

### Contract

```kotlin
data class Column(
    val title: String,
    val min: Dp,                      // hard floor; never crushed below
    val preferred: Dp? = null,        // fixed content width; null = measure
    val weight: Float = 0f,           // >0: shares spare width and is shrunk first
    val align: ColumnAlign = Start,   // Start | Center | End
    val overflow: Overflow = Truncate,// Truncate (1 line, ellipsis) | Wrap (grows row) 
    val numeric: Boolean = false,     // End aligned, tabular figures (fontFeatureSettings "tnum"), Truncate
)
```

Rules:
- Widths are resolved once by a single pure function `resolve(columns, measuredNatural, available) -> widths`: start from `preferred ?: natural`, clamp to `[min, max]`; spare goes to weighted columns by weight; shortfall is taken from weighted columns down to their min, then from other columns down to their min, and never below. If the sum of minimums still exceeds the available width, the table width becomes that sum and the table scrolls horizontally (`horizontalScroll` inside the table; header and body share the scroll state so they move together).
- Natural width is measured with `TextMeasurer` over the data (as History does), not with intrinsics. Cells are declared as data (`text`) or as a slot with `Column.preferred` required, so a slot never needs intrinsic measurement. This makes lazy rows work: header and every row, lazy or not, take the same `widths` list.
- Truncate columns draw one line with ellipsis and expose the full text as a tooltip/`contentDescription`; Wrap columns wrap and set row height; numeric columns use tabular figures and right alignment. No `Label` in a table cell may be unbounded by default.
- One row container draws: the card border and clip once on the table, row hairlines as full-table-width drawn lines (width = scroll content width, not per-cell), header background across the full width, gap/padding in exactly one place.
- Rows are `LazyColumn` items when the row count exceeds a threshold (or `lazy = true`) and a plain Column otherwise; same code path.
- Resizable columns, sort and grouping (History) are optional parameters on the same component, with stored width overriding `preferred`.
- Touch/narrow mode is a declared alternative: `Column.priority` hides low-priority columns below a breakpoint instead of crushing them.

### Migration list (in order)

1. Build `Table`, `resolve` and the tests below; no screens yet.
2. `ContentTable` call sites (one commit each group): Accounts, Forges, Instructions, Sessions, Groups, Schedules, Issues, Projects, Memory banks, Chat tools, Import, Chat models, Assistants, Assistant settings (3).
3. `DataTable`: Node page (6), Activity (6) with numeric columns.
4. `TableBlock` (Markdown and generated UI): `Wrap` columns, numeric detection from `aligns`.
5. `HistoryTable`: layout onto `Table`, keep sort/resize/grouping.
6. Workflows list, then the Chat models browser and Usage card rows if they are tabular.
7. Delete `ContentTable`, `ContentRow`, `FixedWidthColumns`, `TableCard`, `tableHeader()`, `resolveColumnWidths`.

### Tests (desktop, `createComposeRule`, widths 320, 480, 900, 1600dp)

Each table fixture has long titles, a long path, a number column and an empty cell.

| Test | Catches |
|---|---|
| every Truncate cell is one line (cell height equals the row's single-line height) at every width | the current "line wrap" reports, 2cc10ed regressions |
| no column is narrower than its `min` at any width (narrow case triggers scrolling) | proportional crush in `resolveColumnWidths` |
| when the sum of minimums exceeds the width, the table scrolls and the header moves with the body | missing scroll |
| header cell left edge equals the body cell left edge in every column, for the first and last of 200 lazy rows | History drift (b7c434e) |
| row hairline left and right equal the card border inner edges; header background spans the same | open "row lines do not reach the border" |
| the table's right edge equals content right edge when there is spare (weighted column absorbs), and equals the sum of widths when no column has weight and `fill = false` | "does not fit its content", 9b64db6 |
| a table with a `LazyColumn` of 500 rows composes without throwing | HANDOVER lazy intrinsic crash |
| two weighted columns share spare in proportion | single-`flexible` limit |
| numeric cells right edges align and digits are tabular | jittery Usage/Activity figures |
| Wrap column grows the row, neighbours stay top-aligned and aligned to the same edges | Markdown table regressions |
| pure `resolve` property tests: sum equals available or minimums, none below min, deterministic | the resolver |
| one render test per migrated screen at 320 and 900 | each screen fell back to its own layout |

Resize and verification: run the new tests locally only (project rule: no tests in CI).

## Not verified

The cause of the open "row lines do not reach the border" report was not reproduced; the candidates in section 1 are from reading code. No build, test or render was run.

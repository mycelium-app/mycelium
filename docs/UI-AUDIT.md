# Mycelium clients: UI consistency audit

Scope: `clients/shared/src/commonMain/kotlin/mycelium/ui` (all paths below are relative to it). Read-only, nothing built or run. Severity: **B** blocks a task, **I** inconsistent, **P** polish.

Conventions taken from PLAN.md (line 40, 76, 147) and HANDOVER.md (line 32): vertical three-dot menus for row actions everywhere, focus rings keyboard-only, hover on everything, tooltips at once under the button, no restating the screen. The assistants row menu is specified at PLAN.md:147 as "Pin, Clear, Settings, Archive".

Reference implementations:
- `Shell.kt:710-850` `ConversationRow`: right-click (`isSecondaryPressed`, 731), long-press (`detectTapGestures`, 725), menu Pin / Rename / Close session / Archive (739-748). Hand-built inline; no shared modifier.
- `Components.kt:543` `RowMenu(title)`: visible three-dot button (`MyIcon.MoreVertical`, description "More for $title", tooltip) + `MenuPopup` (bottom sheet on touch, popup on desktop). Used by ~25 list rows.
- `ConversationScreen.kt:607-637`: header "More" menu Pin / Show walkthrough / Close session / Archive-Restore / Delete; `Retry` button for Failed (599).
- `Components.kt:784` `ErrorBanner`, `ArtifactsScreen.kt:123`, `MemoriesSettings.kt:83-89` (empty text plus Retry), `NodesScreen.kt:90` (empty text with next step).

## 1. Row actions

| Sev | Where | What differs | Convention | Smallest fix |
|---|---|---|---|---|
| **B** | Sidebar assistant rows, `AssistantsScreen.kt:64-92` (`sidebarAssistantItems`) | No menu at all: no three-dot, no right-click (`Shell.kt:731` pattern absent), no long-press. Pin, Close session, Clear, Settings, Archive unreachable from the sidebar. Conversation rows right above have all of it. | `Shell.kt:710-748` | Wrap row in the shared `rowContextMenu` modifier (see Shared components) and share one `assistantMenu(store, assistant)` item list with the page row. |
| **B** | Assistant thread header, `AssistantScreen.kt:64-90` | `ConversationScreen` is given `header = ...`, and `ConversationHeader` returns early with `custom()` (`ConversationScreen.kt:529-530`), so the whole conversation header is replaced. Lost: Close session (`:627`), Retry on Failed (`:599`), Pin, Show walkthrough. The menu offers only Settings (`:83-85`); Clear is a loose Outline button. A stuck assistant session cannot be closed and reopened. This is the operator's reported bug. | `ConversationScreen.kt:607-637` | In `AssistantHeader` add Close session (`store.closeSession` on `assistant.conversationId`), Pin/Unpin, Archive to the `RowMenu`; render Retry when the conversation is Failed. Better: `ConversationHeader` takes the assistant actions as a slot instead of replacing itself. |
| **B** | Assistants page row, `AssistantsScreen.kt:187-191` | Has three-dot but only Pin / Settings / Archive. Missing Close session and Clear (PLAN.md:147 lists Clear). No right-click or long-press. | `Shell.kt:739-748` | Add Close session (only when `conversationId != null`) and Clear; add the context modifier. |
| I | Sidebar conversation rows `Shell.kt:710-850` | Only right-click / long-press, no visible three-dot. Every other row type shows one, so touch users and mouse users who have not found right-click have no cue. Actions also differ from the header menu: no Show walkthrough, no Restore/Delete. | RowMenu | Render `RowMenu` on hover/focus at the row end, plus keep the context gesture. |
| I | `SchedulesScreen.kt:136-146` | Row menu ok but no right-click/long-press; Delete has no confirmation (artefacts, groups, banks, conversations all confirm); Delete uses `MyIcon.Close`. | `ArtifactsScreen.kt:240,270` | Add the context modifier; confirm Delete with `DeleteConversationDialog`-style dialog generalised. |
| I | Tasks board cards `TasksScreen.kt:304-415` | Inline buttons only (Stop, Answer, Approve, Decline, Start, Delete, Mark done); no three-dot, no context menu. Delete is an inline ghost button without confirmation. | RowMenu for secondary actions, one primary inline button | Keep primary action inline; move Delete / Decline into `RowMenu`. |
| I | Workflows list `WorkflowsScreen.kt:256-262` | A workflow row is a plain `MyButton` with text "name · status · Round n". No menu; Pause / Resume / Stop only appear after opening it (`:269-282`). Not a table, no `ContentRow`. | `SchedulesScreen.kt:120-150` | Render as `ContentRow` with `RowMenu` (Open, Pause/Resume, Stop). |
| I | Artefacts grid `ArtifactsScreen.kt:227` | Menu good; no right-click/long-press. | `Shell.kt:731` | Context modifier. |
| I | Nodes `NodesScreen.kt:238-268` | Raw `MyButton(MoreVertical)` plus `MenuPopup` instead of `RowMenu`; Revoke asks inline (`:283`) while other deletes use dialogs. Menu is mixed: icon and colour pickers inside the same popup. | `RowMenu` | Use `RowMenu`; confirm Revoke via the shared dialog. |
| I | Projects / repositories `ProjectsScreen.kt:303-345` | No three-dot: a `ProjectGroupMenu` (own popup, `ProjectGroupMenu.kt:44`) plus an inline "New conversation" button. No Rename / Remove from this node action. | `RowMenu` | Fold `ProjectGroupMenu` items into a `RowMenu` with New conversation as the first item. |
| I | Issues `IssuesScreen.kt:191-214` | Inline buttons plus a bespoke `MenuPopup` for "Create on". | `RowMenu` | Use `RowMenu` for secondary actions. |
| I | Past conversations `ArchiveScreen.kt:233-250` | Menu offers Restore and Delete (archived only) but no Open, no right-click. History table rows (`HistoryTable.kt:390`) have no context gesture. | `Shell.kt:731` | Context modifier on `HistoryTable` rows. |
| I | Subagent rows `ConversationScreen.kt:760-770` | Row has title, status, since; no menu, so a subagent cannot be stopped or archived from the list. | Conversation row menu | `RowMenu` with Stop / Open. |
| P | Settings rows (Memories, Groups, Providers, Integrations, Forges, Accounts, Sessions, Instructions, Imports) | All use `RowMenu`; none have right-click or long-press. Delete wording varies: "Delete", "Delete group", "Remove provider", "Remove integration", "Disconnect", "Revoke". Icons: `MyIcon.Close` (`SchedulesScreen.kt:144`, `ArtifactsScreen.kt:240`, `AssistantSettingsScreen.kt:235,289,373`) or none (`ArchiveScreen.kt:248`, `MemoriesSettings.kt:129`, `GroupsSettings.kt:83,172`). | One destructive item style | Add a `MyIcon.Trash` and a `DestructiveMenuItem`; keep distinct verbs only where meaning differs (Revoke, Disconnect). |

## 2. Navigation

| Sev | Where | What differs | Convention | Fix |
|---|---|---|---|---|
| I | Back controls | Four styles: icon `MyIcon.Back` (`SettingsScreen.kt:112`, `GitPanel.kt:167`), icon `MyIcon.ChevronLeft` (`ArtefactViewer.kt:57,180`, `AssistantSettingsScreen.kt:82`, `WorkflowsScreen.kt:99`), text only "Nodes" (`NodePage.kt:75`), icon plus text (`GroupsSettings.kt:76`, `ConversationScreen.kt:733`). Descriptions: "Back to settings", "Back to artefacts", "All workflows", "Back to Nodes". | Single `BackButton(label)` | Extract `BackButton(to: String)`: ChevronLeft + text where it fits, description always "Back to $to". |
| I | `AssistantSettingsScreen.kt:82` | Back exists now; "Save" / "Done" lives elsewhere in the form, not in the bar. Other editors (Schedules, Tasks) use dialogs. | Dialog for short forms, screen for long | Leave; note the split. |
| I | `AssistantScreen.kt:64-90` | No back to Assistants on the thread header; the sidebar menu button is the only way. Node page, artefact and settings all have one. | NodePage | Add `BackButton("Assistants")` when the sidebar is not pinned (non-null `LocalOpenList` shows the menu icon instead). |
| P | `NewConversationScreen.kt:121,130`, `NewChatScreen.kt:73` | New screens with no cancel/back; rely on sidebar. | Back or Cancel | Add Cancel or back. |
| P | Dialog actions | `Cancel` muted ghost + `Save` Outline (`Shell.kt:762-763`) is the convention; Revoke uses `Danger` + Cancel; `Primary` appears only in sign-in and Login panel. Order and alignment mostly match (right-aligned); inline rename in `ConversationScreen.kt:545-546` uses Sm. | Shell rename dialog | Extract `DialogActions(cancel, confirm, destructive)`. |

## 3. Headers and bars

| Sev | Where | What differs | Convention | Fix |
|---|---|---|---|---|
| I | Titles | `BarTitle` for list screens (Activity, Archive, Assistants, Artefacts, Issues, Nodes, Projects, Schedules, Tasks, Workflows, Git, Terminal). Detail screens use `Label(..., Tokens.Type.Title)` directly: assistant (`AssistantScreen.kt:75`), node (`NodePage.kt:82`), artefact (`ArtefactViewer.kt:215`), conversation (`ConversationScreen.kt:559`), Settings detail (`SettingsScreen.kt:113`), Groups (`GroupsSettings.kt:72`), Instructions (`InstructionsSettings.kt:139`). `BarTitle` adds the heading role; some `Label` titles omit `heading()`: `ArtefactViewer.kt:181,215`, `ConversationScreen.kt:559`, `ProjectBreadcrumb.kt:38`. | `BarTitle` | Make `BarTitle` accept `maxLines` and use it for every bar; screen titles then always carry the heading semantic. |
| I | Primary action position | Always right of the title via `Box(weight 1f)` on list screens, but Tasks puts "Add a task" in the filter row (`TasksScreen.kt:158`) with a start padding; Workflows toggles "New workflow" (`:104`); Nodes hides the label on touch (`:80`). Assistants "New assistant" Outline (`AssistantsScreen.kt:114`), Schedules / Artefacts similar. | Outline, right-aligned | Keep; move Tasks "Add a task" to the right edge. |
| I | Overflow in headers | Conversation header: "More" three-dot (`ConversationScreen.kt:608`). Assistant header: RowMenu with description "More for <name>". Node group row: raw MyButton. Descriptions differ: "More" vs "More for X". | `RowMenu` | Use `RowMenu(title)` in every bar. |
| P | Assistant header Clear | `MyButton("Clear", icon = Close, Outline)` (`AssistantScreen.kt:79`) uses the Close icon (Close means dismiss elsewhere); a destructive action sits as a prominent button. | Menu item | Move into the RowMenu. |

## 4. Empty, loading and error states

| Sev | Where | What differs | Convention | Fix |
|---|---|---|---|---|
| I | Missing empty state | Tasks board (`TasksScreen.kt`), Workflows list, Groups / Sessions / Forges settings: no text when the list is empty (grep for "No ..." finds none). | `NodesScreen.kt:90` (say what is missing and the next step) | Add one sentence plus the create button. |
| I | Loading wording | "Loading the assistant" (no full stop, `AssistantScreen.kt:44`), "Loading activity." (with full stop, `ActivityScreen.kt:95`), "Loading the conversation" (`ConversationScreen.kt:150`), "Loading the changes". | One style | Pick no full stop for loading, full stop for sentences; one `LoadingLabel`. |
| I | Retry | Offered only in Memories (`MemoriesSettings.kt:83`), thread failure (`ThreadView.kt:285`), usage limit. Load failures elsewhere say "Close the tab and open it again" (`ChangesView.kt:266,339`) instead of a Retry button. `ErrorBanner` (`Components.kt:784`) is dismiss only. | `MemoriesSettings.kt:83` | Add optional `onRetry` to `ErrorBanner`; use it for the changes, git and refresh failures. |
| I | Assistant not found / load fail | `AssistantScreen.kt:42-46` shows "Loading the assistant" forever if the assistant was archived or deleted. | `ArtefactViewer.kt:181` "Artefact not found" | Distinguish loading from not found; offer Back to Assistants. |
| P | Empty tone | Mix of "No X yet", "There are no X", "No X match." and bare statements. Archive search (`ArchiveScreen.kt:197`) and Projects (`:359`) fine; Schedules (`:109`) good. | GOV.UK: what is wrong and how to fix | Standardise on "No X. <next step>." |

## 5. Buttons and menus

| Sev | Where | What differs | Convention | Fix |
|---|---|---|---|---|
| I | Menu primitives | `OptionMenu` (`Composer.kt:735`, a select), `RowMenu`, bare `MenuPopup` (about 30 call sites) with ad hoc `Box { MyButton + if (open) MenuPopup }`. About half of the bare uses are row/bar action menus that should be `RowMenu`: `NodesScreen.kt:241`, `ProjectGroupMenu.kt:44`, `IssuesScreen.kt:208`, `ConversationScreen.kt:611`. `OptionMenu` lives in `Composer.kt`, not `Components.kt`. | Three roles: select (`OptionMenu`), actions (`RowMenu`), custom popover | Move `OptionMenu` to Components; migrate action menus to `RowMenu`. |
| I | Workflows editor | Uses raw `MyButton` toggles as radio options (`WorkflowsScreen.kt:119-194`), hard-coded `8.dp` (`:283`) and `310.dp`, `Label` headings without semantics. The segmented control and `OptionMenu` exist. | `Components.kt:480` segmented control | Replace with segmented controls / `OptionMenu`; use tokens. |
| I | Variants | Ghost (muted) for secondary, Outline for the affirmative action, `Danger` for confirm of destructive (Sessions, Groups, Nodes, NodePage), `Primary` only on sign-in. Task cards use Outline for several co-equal buttons (Approve, Answer, Mark done, Stop). | Shell rename dialog | Document in `Components.kt`: one Outline per context, others Ghost. |
| P | Icon-only buttons | `MyButton` defaults `tooltip = description` when text is null (`Components.kt:224`), so tooltips are consistent. Pressable text rows (`pressable(... description=...)`, e.g. assistant rows, node names `NodesScreen.kt:203`) have none; fine for rows, noted for the "tooltips everywhere" rule. | | None. |

## 6. Tables, lists, search, sort

| Sev | Where | What differs | Convention | Fix |
|---|---|---|---|---|
| I | Tables | `ContentTable` for Assistants, Schedules, Projects, Issues, settings lists; `HistoryTable` (sortable, resizable columns) for Past conversations; card grid for Artefacts; a kanban for Tasks; free `Column` for Workflows. Nodes use their own `NodeGroup` layout. | `AssistantsScreen.kt:141-164`, `ContentTable.kt` | Workflows to `ContentTable`. Leave Tasks and Artefacts as boards. |
| I | Search | Present: Artefacts (`ArtifactsScreen.kt:113`), Archive, Assistants (`:119`), Projects (`:139`), Memories (`:73`). Absent: Schedules, Tasks, Issues, Nodes, Workflows, Integrations (PLAN.md:76 says searchable), Groups. | `AssistantsScreen.kt:119` | Extract `ListSearch(label, query)` (leading search icon, label hidden) and add where lists exceed a screen. Artefacts and Projects already use `leadingIcon`; Assistants, Memories, Archive differ. |
| I | Sorting | Only `HistoryTable` sorts. Assistants have group-by but no sort; Schedules fixed order. | `HistoryTable.kt:213` | Add header sort to `ContentTable` columns when needed (Assistants: last active, name). |
| P | Table alignment | PLAN says left-aligned; Assistants pin column (`ColumnSpec(min = 0.dp, max = 28.dp)`) and trailing menu are right. Fine. | | None. |

## 7. Session controls

| Control | Conversation | Assistant thread | Subagent | Workflow step |
|---|---|---|---|---|
| Stop | Composer `SendButtons` (`Composer.kt:723`) | Same composer | Composer when opened; none in subagent list (`ConversationScreen.kt:760`) | "Stop workflow" button at the workflow level (`ThreadView.kt:1134`, `WorkflowsScreen.kt:269`); no per-step stop |
| Retry | Header (`:599`) and thread failure (`ThreadView.kt:285`) | **Missing**: header replaced (`ConversationScreen.kt:529`); only the in-thread Retry on the last failed row | None | None (failed step shows output only) |
| Close session | Header menu (`:627`) and sidebar menu (`Shell.kt:745`), non-chat only | **Missing everywhere** | Sidebar row menu (it is a conversation) | n/a |
| Continue on | Composer `OptionMenu` (`Composer.kt:403,427`) | Same composer | Same | n/a |
| Pin / Archive | Header and sidebar | Pin / Archive only on page rows; none on thread | | |

Severity **B**: Close session and Retry absent for assistant threads. **I**: no per-subagent or per-step stop.

## 8. Accessibility

| Sev | Where | What | Fix |
|---|---|---|---|
| I | `Shell.kt:802,845` | Status badge and unread dot are colour-only; the row description carries the status text for screen readers, but sighted colour-blind users see only hue (and for chats, only a dot). Sidebar assistants use distinct icons (`AssistantsScreen.kt:84-86`), which is the right pattern. | Use `StatusGlyph` shapes as the assistants do, or an icon for the badge. |
| I | `AssistantsScreen.kt:155-160` | Pin column is an `Icon` with no description when pinned; state is announced only visually. | Add "Pinned" to the row description. |
| I | Gesture-only menus | Sidebar conversation rows rely on right-click / long-press (`Shell.kt:725-736`). Keyboard users cannot reach Pin / Close session / Archive from the sidebar (no key binding, no focusable menu button). | Visible `RowMenu` (focusable) per row. |
| P | `contentDescription = null` | Decorative images only (`ArtifactsScreen.kt:179`, `Attachments.kt:141`, `ProjectIcons.kt:116`); acceptable, but artefact thumbnails sit inside a described row. | None. |
| P | Titles without `heading()` | `ArtefactViewer.kt:181,215`, `ConversationScreen.kt:559`, `ProjectBreadcrumb.kt:38`, `AssistantScreen.kt:75` has it. | Use `BarTitle`. |
| P | Touch targets | `touchMin` and `rowHeight()` used by buttons and rows. `ButtonSize.Sm` maps through `touchMin`, so OK. Sidebar `Box(Modifier.size(S2))` dots are decorative. | None. |
| P | `RowMenu` label | Description "More for $title" can be long when `title` is a schedule prompt or memory text (`SchedulesScreen.kt:136`, `MemoriesSettings.kt:115`). | Truncate in `RowMenu`. |

## Shared components to extract or extend

1. **`Modifier.rowContextMenu(onOpen)`** (from `Shell.kt:725-736`): right-click and long-press open the same menu. Apply to every row type that has a `RowMenu`.
2. **`RowMenu` with a context mode**: one `RowActions(title) { close -> items }` that shows the three-dot button and also answers the context gesture, anchored to the row. Replace the hand-built popups in `Shell.kt:739`, `NodesScreen.kt:241`, `ProjectGroupMenu.kt:44`, `IssuesScreen.kt:208`, `ConversationScreen.kt:611`.
3. **`assistantMenuItems(store, assistant, close)`**: Pin/Unpin, Close session, Clear, Settings, Archive, shared by the sidebar, the Assistants page and the assistant header.
4. **`conversationMenuItems(...)`**: Pin, Rename, Close session, Archive/Restore, Delete, shared by the sidebar, header and history table.
5. **`ConversationHeader` actions slot** instead of `custom` replacement, so assistant threads keep Retry and Close session.
6. **`BackButton(to)`** and **`BarTitle`** everywhere (heading role).
7. **`DestructiveMenuItem` + `MyIcon.Trash` + `ConfirmDialog`** (generalise `DeleteConversationDialog`).
8. **`ErrorBanner(onRetry)`**, **`EmptyState(text, action)`**, **`LoadingLabel`**.
9. **`ListSearch`** (search input with icon) and optional sortable `ContentTable` headers.
10. Move `OptionMenu` from `Composer.kt` to `Components.kt`.

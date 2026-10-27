# Coherence audit

Written 2026-10-27 from the code at `f74c55f`, PLAN.md ("Coherence refactoring push" and the v0.11.0 follow-ups) and the canonical design "Mycelium UI (canonical)". Nothing was built or run for this audit. Principle (operator, https://maevyn.codes/decoherence): one way to do each thing; extend the existing mechanism rather than add a per-feature special case; a transition is finished only when every use has moved and the old way is deleted.

Paths: `ui/` and `state/` and `api/` are `clients/shared/src/commonMain/kotlin/mycelium/…`; `control/` is `crates/control/src/`; `node/` is `crates/node/src/`.

docs/UI-AUDIT.md (2026-10-26) and docs/TABLES.md predate several fixes. Back buttons are already one `BackButton` (`ui/RowMenus.kt:74`) and every table already lays out on `Table` (`ui/Table.kt`, `bb6052f` removed `ContentTable`), so both documents describe some problems that no longer exist. TABLES.md section 2 still lists `ContentTable` and `FixedWidthColumns`.

## Handled by the v0.12.0 feature work

These jobs are done more than one way today, and the queued v0.12.0 items replace the whole flow. The refactor leaves their files alone; the new code must use the dominant patterns below (one action runner, `ConfirmDialog`, `RenameDialog`, `BackButton`, `Table`).

| Job | Variants today | Replaced by |
|---|---|---|
| Approvals and reviews | `ReviewCard` (Zulip review, Share ignores unticked items), per-kind proposal and access cards in `ui/MyceliumCards.kt` each with its own decide state (`Deciding`, hand-rolled `running`/`failure`), `InstructionProposals`, `ArtifactAccess`, `ProposedSchedule`; permission rules as `Inherit/Yes/No` segmented rows in `ui/IntegrationPolicyEditor.kt`; backend: one table, state machine and `publish` per kind (`message_proposals.rs`, `start_proposals.rs`, `instruction_proposals.rs`, `api_access.rs`, `ssh_access.rs`, `artifact_access.rs`, `integration_review.rs`, `integration_policy.rs`) | "One approval flow with a response editor for every integration tool", "Simplify the approvals editor", "Split buttons for alternative actions", "Instruction proposals open the wrong place" |
| Pomodoro and breaks | Pomodoro timer beside break reminders (`ui/WellnessScreen.kt`, `ui/BreakToast.kt`, `ui/TimeAndFocusSettings.kt`, `state/Wellness.kt`, `control/wellness.rs`) | "Remove Pomodoro" |
| Reflection setup | Reflections tab has its own node, harness and folder pickers and its own schedule (`ui/ReflectionsTab.kt`, `state/Learning.kt`, `control/reflections.rs` calling the schedule handlers) beside assistants, which already have preferences, storage and wakes | "Weekly reflection reinvents assistants" (system Wellness assistant) |
| Integration account pages | Account picked from a table, token expiry edited inline under the list (`ui/IntegrationPage.kt`, `ui/IntegrationUsage.kt`, `ui/ChatToolsSettings.kt`, `ui/IntegrationsHome.kt`, `ui/IntegrationsCatalogue.kt`, `ui/IntegrationModel.kt`) | "Integration accounts" sub-pages, "No inline editing", the Integrations conformance pass |
| Sidebar row menus | Two `RowMenu` overloads (`ui/Components.kt:537` stateless, `ui/RowMenus.kt:57` with `RowMenuState` and `revealOnHover`), `rowContextMenu` on sidebar rows with a visible three-dot, hand-built rename dialog in `ui/Shell.kt` `ConversationRow` | "Sidebar rows lost their plain right-click menu"; the sidebar rewrite should also fold the two `RowMenu` overloads into one and use `RenameDialog` |
| Assistant screens | `ui/AssistantScreen.kt`, `ui/AssistantsScreen.kt`, `ui/AssistantSettingsScreen.kt` (inline file editor, its own ordered-rows editor) | "Assistant settings and creation redesign", "Assistant actions" |
| Usage cards | `ui/UsageCard.kt` (`runCatching` around a suspend call), `ui/UsageLimitCard.kt` | "Usage card", "Usage-limit card on the first limit reply" |
| Workflow cards and list | `ui/WorkflowsScreen.kt` reads untyped `JsonObject` (26 `jsonObject[...]` reads); `control/workflows.rs` returns `Json<Value>` while every other resource has a type in `crates/proto/src/api.rs` | "Broken workflow cards in chats", "Selectors" (step model list reusing `ModelPicker`). Typing the workflow API belongs after that fix. |

## Jobs done more than one way

### 1. Running an action and reporting its failure (clients)

Dominant: `rememberAttempt` (`ui/Components.kt:517`) for composables and `attempt {}` (`ui/PlacePicker.kt:48`) for a suspend block; both map `ApiException` to its message and anything else to "Could not reach the control plane. Check your connection and try again.", and rethrow cancellation. `Store.act`/`guarded` (`state/Store.kt:963`) is the same for store-level actions.

Variants:
- `GitPanel.run` (`ui/GitPanel.kt:112`) is a line-for-line copy of `rememberAttempt`.
- Hand-written `try`/`catch (cancelled: CancellationException)`/`catch (_: Exception)` blocks in 18 UI files (ChangesView has 7), several of them swallowing the failure with an empty catch (20 files contain `catch (_: Exception)`).
- The failure sentence is a literal in 16 files instead of one constant.
- `attempt` lives in `PlacePicker.kt`, a picker, though 13 files use it.
- Hand-rolled `var running`/`var failure` pairs around `attempt` in TasksScreen, ProposedSchedule, SchedulesScreen, ExistingFolderDialog, NewProjectDialog, ProjectIcons, MyceliumCards.

Engineering problem: `runCatching { store.api.… }` catches `CancellationException`, so a composable that leaves composition mid-request keeps running as if the call had failed. Network uses: `ui/NodePage.kt:63`, `ui/SettingsScreen.kt:295`, `ui/WalkthroughCheck.kt:33`, `ui/WalkthroughView.kt:92,101`, `ui/ConversationUsageBar.kt:32`, `ui/Composer.kt:218`, `state/Store.kt:853,875`, plus `ui/UsageCard.kt:202` and ten in `state/Learning.kt`/`state/Wellness.kt` (v0.12.0 files). `Store.orNull` is the cancellation-safe form already.

### 2. Confirming a destructive action

Dominant: `ConfirmDialog(title, detail, confirm)` (`ui/RowMenus.kt:79`), used by Schedules, Tasks, ChatTools, IntegrationPage.

Variants: `DeleteConversationDialog` is the same markup written out again; hand-built `Dialog` + Cancel + Danger rows in `ArtifactsScreen.kt:269`, `MemoriesSettings.kt:164`, `MemoryBanksBlock.kt:130`, `GroupsSettings.kt:135`, `SessionsSettings.kt:151,173`, `ForgesSettings.kt:163`. Destructive menu items: "Delete" with `MyIcon.Close` (4), "Delete" with no icon (5), "Remove" with and without `Close`; `Close` means dismiss everywhere else.

### 3. Renaming

Dominant: `RenameDialog` (`ui/Components.kt:682`), used by ModelProviders and IntegrationPage.

Variants: hand-built rename dialogs in `ui/Shell.kt:736` (sidebar, v0.12.0) and `ui/ArtifactsScreen.kt:257`; a rename row with its own Save button in `ui/GroupsSettings.kt:116`; an inline title field in the conversation header (`ui/ConversationScreen.kt:529-546`), although canonical artboard 1.4 shows a dialog. Moving the header to the dialog is a visible change, so it is not in the refactor below.

### 4. Inline edit versus a dialog or sub-page

Operator rule: no inline editing. Inline today: memory text in its row (`ui/MemoriesSettings.kt:94-128`), the conversation title (above), reflection section fields committed on blur (`ui/ReflectionsTab.kt:172`, v0.12.0), token expiry (v0.12.0). Dialogs elsewhere: schedules, tasks, rename, assistant files. The memory and title changes are visible changes and want their own item, not the behaviour-preserving refactor.

### 5. Row overflow menus

Dominant: `RowMenu(title)` (three-dot, "More for X", `MenuPopup`).

Variants: raw `MyButton(MoreVertical)` + `MenuPopup` in `ui/NodesScreen.kt:238` and the conversation header (`ui/ConversationScreen.kt:593`); `ProjectGroupMenu` (`ui/ProjectGroupMenu.kt:40`, "Options for X"); bespoke `MenuPopup` in Issues for "Create on"; `ToolbarMenu`/`FilterMenu` private to `HistoryTable.kt`. Sidebar rows are v0.12.0.

### 6. Settings sub-pages and back navigation

Dominant: top-level destinations are a `Route` (`ui/Shell.kt:121`); `BackButton(to)` is the only back control.

Variants: sub-pages held in screen-local `remember` state: `SettingsScreen` `section` and `openGroup`, `IntegrationsView` (sealed, private to ChatToolsSettings), Workflows `selected`, GitPanel `openCommit`/`viewing`. Only the Settings section has a `BackHandler`, so Android back leaves a Workflows detail, a group or a commit by closing the whole screen. `BackButton` text casing varies with the caller ("settings", "groups", "Integrations", "Nodes"). `SettingsSection.ChatModels` is a dead alias redirected to `Accounts` in two places.

### 7. Schedules

Backend is already one mechanism: the reflection schedule and assistant wakes are rows in `schedules`. Client variants: the Schedules dialog (`ui/SchedulesScreen.kt:192`, cadence kinds), the proposed-schedule editor (`ui/ProposedSchedule.kt:75`, time-only or raw cron, v0.12.0), the Reflections setup (v0.12.0). Cron is described in words twice: `describeCron`/`describeInWords` (`state/Schedules.kt`) and `schedules::describe_cron` (`control/schedules.rs:136`, sent as `Assistant.wakes`).

### 8. Pickers

Dominant: `PlacePicker` (node, agent, folder, project) and `ModelPicker`.

Variants: `NewConversationScreen` rebuilds the node and agent choice with the same two empty-state sentences as `PlacePicker.kt:102,133` ("No node is online…", "No agent is installed on…"); the Reflections tab and assistant settings have their own node pickers (v0.12.0); the workflow step model list (v0.12.0, "Selectors").

### 9. Archive and restore

Conversations: `onArchive` from the header menu, sidebar menu and Past conversations, label toggles Archive/Restore. Assistants: `patchAssistant(archived = …)` (v0.12.0). No other resource archives. One way already; nothing to do beyond the sidebar item.

### 10. Tables

One engine (`Table`) since `bb6052f`. Remaining: `HistoryTable` keeps its own toolbar menus; right-aligned headers lack end padding (v0.11.0 follow-up "Right-aligned table headers"); TABLES.md is stale.

### 11. Empty, loading and failed states

No shared component. Empty states are free `Label`s with three tones ("No X yet", "No X match.", "No X. Do Y."); loading labels mix full stops ("Loading activity." vs "Loading the conversation"); `ErrorBanner` is dismiss-only and Retry exists only in Memories, the thread and the usage limit. Changing the wording is a visible change and is left out of the refactor; an `EmptyState`/`LoadingLabel` primitive is a later transition once the canonical artboards 5.14, 6.6 and 7.6 settle the wording.

### 12. Dialog action rows

`Row(spacedBy(S2, Alignment.End)) { Cancel muted; confirm }` is written out 25 times; "Cancel" muted appears 32 times in 25 files. A `DialogActions` primitive is a later transition (it touches files in every workstream below).

### 13. Client state

Dominant: `Store` holds `AppState`, kept live by the stream, with `refreshX`/`createX`/`patchX`/`deleteX` per resource.

Variants: `LearningStore` and `WellnessStore` with their own flows and `runCatching` (v0.12.0); 177 direct `store.api.…` calls from 40 UI files, most of them loading into screen-local `remember` state that the stream never updates (GitPanel 27, AssistantSettings 14, Instructions 12, ChangesView 10). Moving those into `Store` is the largest remaining transition and is not in this round: it changes when data refreshes.

### 14. Clock ticks

Five ways to tick a clock for relative times: `produceState` + `delay(30.seconds)` (ThreadView), `produceState` + `ClockTickMillis` (GoalBar), `LaunchedEffect` + `var now` + `TickMillis` (TasksScreen), `MinuteMillis` (WellnessScreen, v0.12.0), `nextUsageTick` (UsageCard, v0.12.0).

### 15. Instant parsing

`runCatching { Instant.parse(x) }.getOrNull()` is written 22 times across `state/Format.kt`, `state/Accounts.kt`, `state/Goals.kt`, `state/UsageLimit.kt`, `state/Chats.kt`, `state/AppState.kt`, `state/Thread.kt`, `ui/ProjectsScreen.kt`, and the integration files. Later transition, after the integration pages land.

### 16. Control plane: node request and answer

`control/state.rs:121-130` keeps six maps (`file_requests`, `git_requests`, `steer_requests`, `stop_requests`, `folder_requests`, `reset_requests`), each with a `request_x`/`answer_x`/`forget_x_request` triplet that differs only in the message and answer type (about 200 lines). Callers each wrap the receiver in their own `tokio::time::timeout(...).await.ok().and_then(Result::ok)`.

### 17. Control plane: background loops

Five `*_regularly` loops spawned in `control/lib.rs:113-117`: `schedules`, `usage_limits` and `activity_sweep` sleep on `config.scheduler_tick` (activity capped at 10 s); `workflows` uses a hard-coded one-second `tokio::time::interval`; `versions` sleeps on `version_check_interval`. Each repeats the readiness check and the warn-and-continue error handling.

### 18. Control plane: handlers calling handlers

24 calls build axum extractors to call another module's handler (`prompts::prompt(State(state.clone()), user, Path(id), Json(...))`), including the reflection schedule calling `schedules::create`/`patch`. Six of them discard the result with `let _ =` (`api_access.rs:160`, `instruction_proposals.rs:230`, `message_proposals.rs:172`, `schedules.rs:332`, `start_proposals.rs:275`, and `prompts::cancel` at `workflows.rs:352`), so a failed prompt or cancel is silent. The fix is a plain `prompts::send(state, owner, id, request)` the handler and every caller share; most callers are approval modules, so it follows the v0.12.0 approval flow.

### 19. Control plane: owner-scoped loaders

Dominant: `find_owned(state, id, owner)` (groups, projects, schedules, forges, model_providers, store, the proposal modules). Variants: `artifacts::owned(state, id, owner)`, `lastfm::owned(state, owner, id)` (argument order reversed), `artifact_access::owned(state, owner, artifact) -> ()`, `assistants::load(state, owner, id)`.

### 20. Naming

- "Artefact" versus "artifact" inside the client: `Route.Artifacts` and `Route.Artefact`, `ArtifactsScreen.kt` and `ArtefactViewer.kt`. Wire names stay "artifact"; user-facing text is already "artefact".
- "Chat tool server" (`ChatToolServer`, `chat_tool_servers.rs`, `ChatToolsSettings.kt`, `SettingsSection.ChatTools`) versus "Integration" in the interface and `integrations.rs`. `SettingsSection.Accounts` is labelled "Providers".
- Renames cross dozens of files and every concurrent branch; they are listed here, not scheduled.

## Engineering problems

- Dead code: `IntegrationLogo` (`ui/BrandLogos.kt:33`), `TextStyle.scaledForCode` (`ui/Display.kt:63`), `MyceliumApi.connectConversations` (`api/MyceliumApi.kt:490`) have no caller. `MyceliumApi.exerciseSolution` has none yet but backs canonical artboard 8.8, so it stays.
- God files: `control/mcp.rs` (1505 lines, `handle` is one 830-line match over every tool, `tool_definitions` is 440 lines of JSON); `node/git.rs` (1018 production lines: dispatch, worktrees, git-town stack, moves, file reads and edits in one module); `crates/proto/src/api.rs` (2104 lines); `ui/ThreadView.kt` (1166), `ui/Composer.kt` (999), `ui/Shell.kt` (898). `mcp.rs` is where the approval flow lands, so it is split after that.
- Duplicated git invocation in the node: `node/files.rs:25` and `node/changes.rs:128` call `Command::new("git")` directly instead of `git::git`.
- Production `unwrap`: `control/forge_cache.rs` unwraps `Mutex::lock()` six times where `control/state.rs` uses `if let Ok(...)`; `control/workflows.rs` (4) and `node/changes.rs` (1) unwrap outside tests.
- Inconsistent row mapping in the control plane: 107 tuple `query_as::<_, (…)>` against 43 `FromRow` structs; tuples of six or more fields (`reflections.rs:154`) are hard to read and to change.
- Error types in the node mix `anyhow::Result` and `Result<_, String>` (git.rs 31 `String` errors).
- Tests that cannot fail: the render tests (`TablesRenderTest`, `BoardsRenderTest`, `ScreensRenderTest`, `WalkthroughRenderTest`, `ScrollbarRenderTest`, `GitGraphTest.rendersTheGraphForReview`, `GitStackTest.rendersTheStackForReview`, and the new `AllScreensRenderTest`) write PNGs and fail only on a crash. They are review aids, not tests; either assert something (no clipped text, single-line cells at 320 px) or move them to a render task outside `:desktop:test`. `BoardsRenderTest.settingsSections` and `AllScreensRenderTest.settingsSections` render the same screens.
- Duplicated test harness: 97 desktop test files each build their own `MockEngine` fake control plane, and six define their own `render`/`everywhere`. A shared fake would cut most of each file's setup.
- Comments: none depend on the implementation; the only doc comments are clap help text in `crates/cli`.

## Refactor workstreams

Behaviour unchanged: no visible text, layout or request changes. Each workstream owns its files outright; no file is in two workstreams, and none touches a v0.12.0 file listed above. The existing tests for each area are the bar.

1. **client-foundation**: one action runner and one failure sentence; the starting-work screens share one node and agent chooser.
2. **client-settings**: every Settings screen runs actions through the runner and confirms through `ConfirmDialog`; the dead `ChatModels` alias goes.
3. **client-work-screens**: list and detail screens (History, Artefacts, Schedules, Projects, Issues, Nodes, Activity) use `RowMenu`, `ConfirmDialog`, `RenameDialog` and the runner.
4. **client-conversation**: the conversation, thread, composer, git, changes, terminal, walkthrough and goal views use the runner, cancellation-safe loads and one clock.
5. **control-plumbing**: one pending-request registry for node answers, one background-loop helper, one owner-loader shape.
6. **node-git**: `git.rs` split by job, and every git call goes through `git::git`.

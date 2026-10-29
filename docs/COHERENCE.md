# Coherence audit

Rewritten 2026-10-27 from the code at `8e412b0` (v0.12.1 plus plan commits). The v0.12.0 audit (`00913e5`) is superseded: v0.12.0 and v0.12.1 finished several of its transitions and started new variants. Nothing was built or run for this audit; every count below is a `grep` over the tree. Principle (operator, https://maevyn.codes/decoherence): one way to do each thing, the dominant way wins, a transition is finished only when every use has moved and the old way is deleted, and no assumption crosses a boundary unstated.

Paths: `ui/`, `state/` and `api/` are `clients/shared/src/commonMain/kotlin/mycelium/…`; `control/` is `crates/control/src/`; `node/` is `crates/node/src/`; `desktop-test/` is `clients/desktop/src/test/kotlin/mycelium/desktop/`.

## Finished since v0.12.0

These were variants in the last audit and are now one way. Nothing to do; new code must keep to them.

- Node git: `node/git.rs` is split by job into `node/git/{mod,repository,commits,history,moves,stack,folder}.rs`, and every git process starts in `git::git`/`git::git_with_input` (`node/git/mod.rs:80,88`). No `Command::new("git")` outside the module.
- Background loops: every `*_regularly` loop and the wellness boundary loop run through `boot::run_while_ready` (`control/schedules.rs:472`, `workflows.rs:431`, `usage_limits.rs:297`, `activity_sweep.rs:12`, `versions.rs:92`, `wellness.rs:480`).
- Node request maps: one generic `PendingRequests<A>` (`control/state.rs:113`) replaces six hand-written maps. The per-kind wrappers around it remain (job 6 below).
- Client action runner in the UI: `attempt`/`rememberAttempt` (`ui/Components.kt:636,649`) is used by 36 UI files; `GitPanel.run` and the hand-written `try`/`catch` blocks in screens are gone; the unreachable sentence is one constant (`ControlPlaneUnreachable`).
- Clock ticks: `rememberNow` (`ui/Clock.kt:11`) is the way, used by GoalBar, HoursScreens, TasksScreen and ThreadView.
- `ConfirmDialog` and `RenameDialog` are used by 13 and 6 files; `DeleteConversationDialog` is a two-caller wrapper over `ConfirmDialog`.
- Comments: there are no implementation comments anywhere (Rust, Kotlin, Svelte, TypeScript). The only doc comments are clap help text in `crates/cli/src/main.rs`.

## Jobs done more than one way

### 1. Sending a prompt, cancelling, creating, closing from inside the control plane

Dominant: a plain function the handler wraps, as `prompts::config` wraps `prompts::set_config` (`control/prompts.rs:471,477`) and `prompts::continue_unless_queued`, `prompts::retry_failed` (`prompts.rs:203,216`).

Variants: 18 calls build axum extractors to call another module's handler:
- `prompts::prompt(State(..), user, Path(..), Json(..))` from `api_access.rs:173`, `assistants.rs:430,467`, `instruction_proposals.rs:264`, `message_proposals.rs:150,172`, `start_proposals.rs:275`, `tasks.rs:257,297`, `schedules.rs:362,571`.
- `prompts::cancel` from `workflows.rs:352`; `assistants::create` from `assistant_changes.rs:243`; `conversations::close` from `assistants.rs:488`; `assistants::prompt` from `schedules.rs:546`; `workflows::get`, `goals::get`, `projects::upload_avatar` from `mcp.rs:867,1286,1328`.

Engineering problems in the same calls:
- Six discard the result with `let _ =` (`api_access.rs:173`, `instruction_proposals.rs:264`, `message_proposals.rs:172`, `start_proposals.rs:275`, `schedules.rs:362`, `workflows.rs:352`), so a failed prompt or cancel is silent.
- To call a handler the caller forges an `AuthUser` with empty email and name: `api_access.rs:172`, `goals.rs:189`, `instructions.rs:295`, `message_proposals.rs:113`, `workflows.rs:312,344,392,477`, `schedules.rs:517,571`, `mcp.rs:866,1284,1327,1457`. That is a hidden assumption across a boundary: the handler must never read `email`, `name`, `session` or `scopes`.
- Callers that need "queued because the node is offline" read it back out of an HTTP `Response` (`message_proposals.rs:150`, `tasks.rs:297`, `schedules.rs:571`).

Target: `prompts::send(state, owner, id, PromptRequest) -> ApiResult<Sent>` and `prompts::stop(state, owner, id)`, likewise `assistants::create_for`, `conversations::close_for`, `assistants::wake`, `workflows::load`, `goals::load`, `projects::set_avatar`; handlers become thin wrappers; every caller handles the result; no `AuthUser` is built outside `auth.rs`.

### 2. Loading a row the caller owns (control plane)

Dominant: `find_owned(state, id, owner) -> ApiResult<Row>` (15 modules: groups, projects, schedules, forges, model_providers, artifacts, lastfm, assistant_changes and the proposal and access modules).

Variants: `assistants::load(state, owner, id)` (`assistants.rs:109`, argument order reversed); `artifact_access::owned(state, owner, artifact) -> ApiResult<()>` (`artifact_access.rs:66`); `walkthroughs::find_owned_walkthrough` (`walkthroughs.rs:232`, returns an index); `store::find_owned(pool, …)` takes a pool, not the state (`store.rs:148`), which is fine because it also runs inside transactions.

### 3. Deciding an approval (control plane)

Eight modules each carry their own table, `find_owned`, `list`, `approve`, `decline` and "already decided" conflict: `message_proposals.rs`, `start_proposals.rs`, `instruction_proposals.rs`, `api_access.rs`, `ssh_access.rs`, `artifact_access.rs`, `assistant_changes.rs`, `assistant_actions.rs`. Merging the tables is a schema change and out of scope. Within scope: `api_access.rs` and `ssh_access.rs` are the same flow (token to the conversation's node, the same `no_node` and `node_offline` conflicts with identical sentences at `api_access.rs:236-295` and `ssh_access.rs:168-209`, the same `token_saved`); they share one token-delivery path.

### 4. Built-in integration tool calls

Dominant: `pub fn tools() -> Vec<StoredTool>` and `pub async fn call(…) -> CallOutput`, dispatched by `Integration` in `control/integrations.rs:100-130` (zulip, email, stoat, caldav).

Variant: `lastfm::call` returns its own `Outcome` (`control/lastfm.rs:193`).

### 5. Defining and dispatching MCP tools

`control/mcp.rs` (1712 lines) does three jobs: the tool catalogue (`tool_definitions`, lines 66-618, one JSON literal), the dispatcher (`handle`, lines 763-1678, one match over every tool) and the transport (`reply`, `failure`, `stream`, `call_in_process`). The integration modules declare their tools beside their handlers (job 4), which is the shape the built-in tools should take: each area's tool definitions and handlers in one module, `mcp.rs` keeping the transport and the routing table.

### 6. Asking a node and waiting for its answer (control plane)

Dominant: `PendingRequests<A>` (`control/state.rs:113`).

Variants: 18 per-kind wrappers `request_x`/`answer_x`/`forget_x_request` (`state.rs:315-399`) that differ only in the message; every caller writes its own `tokio::time::timeout(T, answered).await.ok().and_then(Result::ok)` and its own forget (`account_usage.rs:155`, `conversations.rs:661`, `git.rs:117`, `nodes.rs:114`, `assistants.rs:800`; `prompts.rs:321` keeps the receiver for a late answer and is the one legitimate exception). Target: one `state.ask_node(node, timeout, |request_id| message) -> Option<A>` that registers, sends, waits and forgets, and one `state.answer(node, request_id, answer)` the link dispatcher calls.

### 7. Running an action and reporting its failure (clients)

Dominant: `attempt`/`rememberAttempt` (`ui/Components.kt:636,649`); `Store.act` (`state/Store.kt:1031`) for store actions that report into the store's banner; `Store.orNull` (`Store.kt:1083`) for a cancellation-safe load.

Variants:
- `Deciding` (`ui/MyceliumCards.kt:657-680`), a runner with its own `running`/`failure`.
- Hand-rolled `var running`/`var failure` pairs around `attempt` in `ui/ChangesView.kt`, `ui/ExistingFolderDialog.kt`, `ui/NewProjectDialog.kt`, `ui/ProjectIcons.kt`, `ui/TasksScreen.kt`, `ui/SchedulesScreen.kt`, `ui/ProposedSchedule.kt:82-83`, `ui/MyceliumCards.kt:410-411`.
- `runCatching { api.… }` in `LearningStore` (`state/Learning.kt:125-237`, 11 calls) and `WellnessStore` (`state/Wellness.kt:145-263`, 10 calls), and `ui/UsageCard.kt:210`. `runCatching` catches `CancellationException`, so a cancelled call is treated as a failure and the coroutine keeps going.
- `catch (_: Exception)` without the cancellation rethrow in `ui/WorkflowsScreen.kt` (3) and `ui/Shell.kt` (1).

Target: one runner with its `running` and `failure` state (`rememberAction`, extending `rememberAttempt`); the two stores use `orNull`-style cancellation-safe helpers; `Deciding` and every hand-rolled pair are deleted.

### 8. Row overflow menus (clients)

Dominant: `RowMenu(title, state, content)` (`ui/RowMenus.kt:71`) with `rowContextMenu`.

Variants: a second, stateless `RowMenu(title, content)` overload (`ui/Components.kt:660`); `ProjectGroupMenu` builds its own three-dot button labelled "Options for X" (`ui/ProjectGroupMenu.kt:40`); the conversation header builds its own (`ui/ConversationScreen.kt:652`, labelled "More" when there are no slots); `HistoryTable` keeps private `ToolbarMenu`/`FilterMenu` (`ui/HistoryTable.kt:68,92`), which are toolbar menus, not row menus, and stay.

Target: one `RowMenu` with a defaulted state parameter, so both call shapes compile against one function. The two hand-built buttons have different accessible names; moving them is a visible change and is listed under "Not scheduled".

### 9. Confirming a destructive action (clients)

Dominant: `ConfirmDialog` (`ui/RowMenus.kt:84`).

Variants: hand-built `Dialog` with a Danger button in `ui/MemoryBanksBlock.kt:130-145` and `ui/NodePage.kt:245-262`; inline confirmation rows in `ui/GroupsSettings.kt:110-127` and `ui/NodesScreen.kt:292-301`. The inline rows are a different layout and the two dialogs order their buttons differently from `ConfirmDialog`; moving any of them is visible.

### 10. Renaming (clients)

Dominant: `RenameDialog` (`ui/Components.kt:814`), six callers.

Variants: hand-built rename dialog for sidebar conversations (`ui/Shell.kt:844`); hand-built name-a-view dialog (`ui/HistoryTable.kt:208`); inline rename rows in `ui/NodesScreen.kt:213-223` and the conversation header (`ui/ConversationScreen.kt:575-580`). The inline ones are visible changes.

### 11. Sub-pages and back navigation (clients)

Dominant: a `Route` (`ui/Shell.kt`) for destinations; `BackButton` (`ui/RowMenus.kt:79`) for the control.

Variants: sub-pages in screen-local `remember` state: `SettingsScreen` `section`/`openGroup` (`ui/SettingsScreen.kt:75-76`), `IntegrationsView` (`ui/ChatToolsSettings.kt:45`), `AssistantSettingsScreen` `section` (`:59`), Workflows `selected` (`ui/WorkflowsScreen.kt:75`), GitPanel `openCommit`/`viewing` (`ui/GitPanel.kt:81,86`). Only Settings has a `BackHandler` (`SettingsScreen.kt:92`). `BackButton` labels mix case ("Integrations", "Nodes", "Assistants" against "settings", "groups", "workflows", "artefacts"). `SettingsSection.ChatModels` is a dead alias redirected to `Accounts` (`SettingsScreen.kt:54,75,189`). Adding back handling and recasing labels are visible; deleting the alias is not.

### 12. Parsing timestamps (clients)

34 `Instant.parse` calls, most wrapped in `runCatching { … }.getOrNull()`, across `state/{Chats,Puzzles,Accounts,UsageLimit,HarnessModels,Goals,Learning,AppState,Format,UsageHistory,Thread}.kt` and `ui/{ProjectsScreen,IntegrationUsage,UsageHistoryScreen,IntegrationModel,WellnessScreen}.kt`. Target: one `instantOrNull(text)` in `state/Format.kt`; every site calls it.

### 13. Ticking a clock (clients)

Dominant: `rememberNow`. Variants: `WellnessScreen` runs its own `LaunchedEffect` + `delay(MinuteMillis)` (`ui/WellnessScreen.kt:47,70`); `UsageCard` delays by `nextUsageTick` (`ui/UsageCard.kt:141`), which is a different cadence and stays. `MinuteMillis` is defined twice (`WellnessScreen.kt:47`, `HoursScreens.kt:42`).

### 14. Workflow API types

Every resource has a type in `crates/proto/src/api.rs` and `api/Models.kt` except workflows: `control/workflows.rs:173,205,214,231` return `Json<Value>`, `MyceliumApi.workflows/createWorkflow/controlWorkflow` return `JsonObject` (`api/MyceliumApi.kt:105-109`), and `ui/WorkflowsScreen.kt` reads fields by string. The git endpoints (`control/git.rs`) also return `Json<Value>`, but they relay the node's answer untouched, which is one job and stays.

### 15. Where wire types live

Rust: `crates/proto/src/api.rs` (2485 lines, 229 types, one job) is dominant; `crates/proto/src/usage_history.rs` is the one area split out. Moving it touches four importers in other crates for no gain; left. Kotlin: `api/Models.kt` (1804 lines, 407 declarations) beside per-area files `WellnessModels.kt`, `LearningModels.kt`, `DecisionModels.kt`, `CalendarModels.kt`, `IntegrationSetupModels.kt`, `UsageHistory.kt`, `GitGraph.kt`. The per-area files are the newer direction and Kotlin moves within a package change no imports, so `Models.kt` splits by area.

### 16. Errors in the node

`Result<_, String>` in 14 files (51 signatures, `node/git/*`, `changes.rs`, `transcripts.rs`, `codex_app_server.rs`, `assistant_folder.rs`, `update.rs`, `resources.rs`, `test_run.rs`) beside `anyhow` in 10. The answers sent to the control plane carry a message string, so the string form is the boundary, not the internal type. Target: `anyhow::Result` inside the node, converted to the answer's string once where the daemon replies (`node/daemon.rs`).

### 17. Naming

- "Artefact" against "artifact" in the client: `Route.Artifacts` and `Route.Artefact` (`ui/Shell.kt:161,163`), `ArtifactsScreen.kt` against `ArtefactViewer.kt`. Wire names stay "artifact"; user text is already "artefact".
- "Chat tool server" (`ChatToolServer`, `control/chat_tool_servers.rs`, `ui/ChatToolsSettings.kt`, `SettingsSection.ChatTools`) against "Integration" in the interface and `control/integrations.rs`.
- These renames cross every workstream and every concurrent branch; listed, not scheduled.

## Engineering problems

- Dead code: `IntegrationLogo` (`ui/BrandLogos.kt:33`, only a render test calls it, `desktop-test/IntegrationLogosRenderTest.kt`), `countdownText` (`ui/WellnessScreen.kt:56`), `inputKindLabel` and `pinnedActions` (`state/Assistants.kt:62,68`), `SettingsSection.ChatModels`. `MyceliumApi.exerciseSolution` has one caller now (`state/Learning.kt:157`).
- God files over about 1500 lines that mix jobs: `control/mcp.rs` (1712, job 5). Large but single-job: `crates/proto/src/api.rs` (2485), `api/Models.kt` (1804, split in job 15), `control/assistants.rs` (1252), `ui/ThreadView.kt` (1181), `control/imports.rs` (1146), `node/acp.rs` (1153), `state/Store.kt` (1096), `ui/Composer.kt` (1031), `ui/Shell.kt` (1013).
- Production `expect`: `control/provider_usage.rs:105` (`expect("finite spend")`). No production `unwrap` remains in control or node.
- Row mapping: 117 tuple `query_as::<_, (…)>` against 47 `FromRow` structs. Tuples are dominant for short rows; a tuple of six or more fields becomes a `FromRow` struct.
- Editor (`clients/editor/src/lib/store.svelte.ts`): a parenthesised `await` feeding `??` (`:399`) and a chain of inline `if`/`else if` statements without braces (`:330-334`).
- Test harness: 109 desktop test files each build their own `MockEngine` fake control plane, and seven define their own `render`/`everywhere`. A shared fake is a later transition; it touches every test file and changes no product code.
- Client data loading: 208 `store.api.…` calls in UI files, most loading into screen-local state the stream never updates (ConversationScreen 45, GitPanel 35, AssistantSettings 25, AssistantActions 21). Moving them into `Store` changes when data refreshes; not in this round.

## Not scheduled (visible changes)

Each changes what a person sees or hears, so it needs its own item rather than this behaviour-preserving refactor: inline confirm rows and the two hand-built confirm dialogs into `ConfirmDialog` (job 9); inline rename rows into `RenameDialog` (job 10); `ProjectGroupMenu` and the conversation header onto `RowMenu` (different accessible names, job 8); back handling for screen-local sub-pages and `BackButton` casing (job 11); the 36 empty-state `Label("No …")` calls onto `EmptyState` (`ui/ListControls.kt:35`, nine callers, different spacing); the 34 hand-written `Cancel`-then-confirm dialog rows onto a `DialogActions` primitive (button order differs between callers); merging the eight approval tables; the naming renames (job 17).

## Refactor workstreams

Behaviour unchanged: no visible text, layout, accessible name or request changes. Each workstream owns its files outright and no file is in two. A shared component belongs to exactly one workstream, which owns its API; the others only call it. Where one workstream calls an API another adds, the dependency is named; the owner lands first. The existing tests are the bar.

1. **control-core**: jobs 1, 2, 3 (token delivery), 6; the `provider_usage` `expect`. Owns `prompts::send`/`prompts::stop` and the other plain functions in job 1 except those in workstreams 2 and 4, and `state.ask_node`.
2. **control-tools**: jobs 4 and 5: `mcp.rs` split by area with the routing table kept in `mcp.rs`; `lastfm::call` returns `CallOutput`; `goals::load` and `projects::set_avatar` as plain functions. Calls `workflows::load` (workstream 4).
3. **node-and-hosts**: job 16 in the node; the editor's two style problems; the keeper and artefact host crates are in scope for any of the same problems.
4. **wire**: jobs 14 and 15: typed workflow API end to end (proto types, `control/workflows.rs` returning them and exposing `workflows::load`, `MyceliumApi`, `WorkflowsScreen`); `Models.kt` split by area. Calls `prompts::stop` (workstream 1).
5. **client-foundation**: job 7's runner, job 8's single `RowMenu`, job 12's `instantOrNull`, the stores' cancellation-safe loads, dead code in `state/` and `ui/BrandLogos.kt`; owns every UI primitive, all of `state/`, the platform modules and the tests.
6. **client-conversation**: the conversation, thread, composer, git, changes, walkthrough, cards and usage views onto the runner (`Deciding` deleted) and `instantOrNull`.
7. **client-settings**: Settings, integrations, accounts, assistants and setup screens onto the runner and `instantOrNull`; the `ChatModels` alias deleted.
8. **client-workspace**: the shell, sidebar, lists, nodes, projects, tasks, schedules, history, wellness and learning screens onto the runner, `rememberNow` and `instantOrNull`; `countdownText` deleted.

# Changelog

## v0.15.2

Every screen change in this release was designed in the canonical design artefact first.

- The composer bar shows the same controls in the same order everywhere, each once: a failed conversation keeps the account control so you can switch to an account that works, and a chat on a chat model has one model control that also lists your agent accounts.
- The account control always names the current account or model in words, never an internal id.
- One account picker everywhere you choose an agent account or chat model: the composer, New conversation, New chat and open chats. It has search across accounts, nodes and models, favourites and recents, clear section headings, and each account shows its node (or This device) beside its usage.
- Continuing a conversation on an account on another node is refused when that node does not have the conversation's folder, with Move to that node offered instead, so a conversation can no longer end up on a node where it cannot run.
- A new chat started on an account on a given node starts on that node.
- The mode and model you chose are applied every time a session starts or resumes, and the composer shows what the agent reports, so a conversation that says Auto runs in Auto.
- A failed conversation can be moved, and the move button says Move unless a turn is running.
- The conversation header moves the project, node and Open in IDE into its menu when space runs out instead of dropping them, and a failed conversation shows one Retry, inside the error.
- Queued messages show their images, and editing one opens it in the composer with its images; it is held back until you save. Queued messages can be reordered from the keyboard.
- An alert's actions sit inside the alert, such as signing in to GitLab's MCP server.
- Images in the composer and its hover preview are shown whole instead of cropped to a square.
- A break that ran over records how long you were actually away, the break note box starts empty each time, and restoring an archived conversation keeps it open.
- The setup progress bar in Projects sits on its row's centre line, and the New conversation button there is no longer cut off.

## v0.15.1

- Every chat can have an environment, including chats on a chat model: the Environment tab is in every chat, and starting one uses a node that can run environments, with Run on to choose when there are several.
- Agents start the environment themselves with environment_start, and are told how to work in one: start it when needed, hand web goals to browser_task, click by describing the target, and take a screenshot after you hand back.
- Break notes are under Recent breaks in Wellness & Learning, Reflections, beside the weekly reflection, instead of in Settings.
- A project's new logo shows in the sidebar and every list straight away, including one an agent sets, instead of only after opening Projects.
- The project, node and Open in IDE controls in a conversation's header share one style.
- The sidebar can be hidden at any width, with the button at the top of the sidebar or Ctrl or Cmd+B, and it stays hidden after a restart. There is one Show the sidebar button, and a panel that takes over the chat has one header with Back to conversation.
- Moving a conversation no longer misses a file edited in the same second as the last commit.
- A failed action shows as a toast in the corner, with Retry where it can be retried, instead of a bar across the top of the app. It stays until you dismiss it, and the break reminder uses the same toast.
- A chat's account button names its model when it is not the provider's default, instead of showing an internal id.

## v0.15.0

Cowork follow-ups. Every screen in this release was designed in the canonical design artefact first.

- Chats can run Claude Code and Codex: the Chat section of the account menu lists your agent accounts, and Mycelium picks a node with that account online and gives the chat a scratch folder there, deleted with the chat. Continue on moves it to another node or account. A chat on an agent can start an environment without choosing a folder.
- Environments run any app the node offers, not only Chromium: choose what it starts with, see and switch its windows, and open more. Agents open apps with environment_launch. The node page lists the apps each node offers.
- Fast actions with a decisions model: a decisions provider can take the new Browser use. Agents hand a goal to browser_task and the model picks each step from the page's controls, shown live in the thread with Stop task. environment_click can find a described target on a screenshot of any app. Box questions need Jev 1.14 (typesafe/jev-1.14 on OpenRouter).
- The thread says when you started an environment, took it over and handed it back, and the agent is told with its next message.
- An environment stops when its conversation is closed or deleted, or when its node shuts down; starting it again reopens an archived conversation.
- While the agent has control the live view is view-only on the server too, and the node page names who is in control.
- Environment tools never ask for permission, in any mode.
- On the phone the environment opens full screen from the conversation bar.
- Before a break ends, early or on time, the break screen asks what you did in it. The notes show in Time and focus and the weekly reflection draws on them.

## v0.14.0

Every screen in this release was designed in the canonical design artefact first.

- Mycelium manages goals: after each turn it decides whether to nudge the agent and says why, the agent reports a goal done or blocked with report_goal, repeated outcomes back off and then stop, an open question holds the goal, and Stop never nudges. Nudge now and Continue are on the goal bar.
- Workflows have their own icon and a proper page: a table with status, where each workflow is, where it started and its project; a page with steps as lanes, every round kept, gates you approve or send back with a reason, a confirmed Stop, Re-run, and failures that name the step and the error. Claude Code workflow runs are listed too.
- New artefact kinds: Excalidraw drawings, Typst documents with a live preview and PDF, Docs, Slides with a presenter view, Grafana-compatible dashboards, Motion, and Design systems that designs can use. Artefacts can be created from the app.
- Apps: add Coolify and an Apps tab appears above Artefacts, with each app's status, deployments, build logs, environment variables and the conversation that built it. Agents create and deploy apps after you approve.
- Tandoor integration: recipes, meal plans and the shopping list.
- Cowork: a conversation can have its own environment, a headless Wayland desktop with a browser on a node, shown in the right sidebar. Agents see and act in it, and you can take over and hand back. It never shows your own screen.
- Voice: a built-in Voice assistant with its own instructions. With voice on, a microphone beside the usage gauge listens only when you press it or use Ctrl or Cmd+Shift+Space, and can read replies aloud through your provider. Settings has a Privacy page. The wake phrase is not available yet on any device.
- Wellness & Learning has an Activity tab: what was on screen through the day as a timeline, and the last week by hour.
- GitHub's MCP server uses your GitHub forge's token, from the forge or the catalogue, instead of a sign-in GitHub does not support.
- A pasted image keeps its place in a draft when you switch away and back, and the arrow keys no longer stick on an image in the composer.
- The built-in browser follows its container when the sidebar or a side panel is resized.
- Large files read from a node arrive in pieces, so reading one no longer drops the node's connection.

## v0.13.1

- During a break the keyboard stays on the break screen: Tab no longer reaches the app behind it, screen readers skip it, and End break early has focus.

## v0.13.0

Every screen in this release was designed in the canonical design artefact first. The code had a whole-codebase refactor for cohesion before the features landed, and the tests now check what the app does rather than its wording or layout.

- On a narrow window the left sidebar hides behind a menu button and opens over the chat; narrower still, an open side panel takes over the chat with Back to conversation. Your saved widths never change.
- Starting a break covers the app with a break screen and the time left. Running work carries on, notifications wait, and ending a break early asks first.
- When Mycelium itself is closed the sidebar shows no conversations or chats at all, and Close now takes a group out of every list at once.
- Workflow card actions sit at the right end of the header, and subagents are listed newest first.
- Settings has an Agents section: the default permission mode for Claude Code and Codex and for each account, the usage-limit choice, and your Any model list.
- Account menus offer Claude Code, any account; Codex, any account; and Any model. Mycelium starts on the one with the most usage left and moves the conversation when it hits a limit, with a line in the thread saying why.
- Each account can carry the price you actually pay, and Usage history uses it.
- A goal set on a conversation always reaches the agent, now or when its turn ends, and the goal bar says when it was sent. A goal stopped by a usage limit is held, not ended, and carries on when the limit resets without counting the held time.
- Wordle and Connections can be played in Mycelium itself. enclose.horse joins the puzzles, and every puzzle records its result and score with a streak and a calendar of past days.
- New integrations: Discord, FreeAgent, Actual Budget and Paperless-ngx.
- Local builds are lighter: one test binary per crate, the mold linker, pruning after test runs and Gradle's configuration cache.

## v0.12.1

v0.12.0 was tagged but never published: its Android and web builds failed. v0.12.1 is that release with both building.

## v0.12.0

The canonical UI design now lives in the design artefact "Mycelium UI (canonical)", and every screen in this release was designed there first. The code had a refactor for cohesion before the features landed; what still has more than one way is listed in docs/COHERENCE.md.

- History hides its Node and Agent columns when the window is too narrow for every column, so the row menu stays in reach.
- A usage-limit reply gets the paused card and the automatic resume even when the agent wrote a long answer before it hit the limit.
- Every tool call that passes through Mycelium has one approval flow: Approve, Approve and edit response, Always approve and Deny. Edit response opens a table, tree or text editor with a jq filter the agent can set, and Send to agent sends only what you kept. Zulip uses the same flow, with Edit response before the agent sees it as its default.
- An account's approvals are a preset (Ask before writes, Ask before everything, Ask before everything and edit responses, Don't ask) with a Customise table of Ask first and Edit response per tool.
- Buttons with alternatives are split buttons: Approve, Send and Decline have a chevron menu, and Decline without telling the agent skips the message to the conversation.
- Wellness & Learning: the leaf is green whatever accent you pick.
- Pomodoro is removed. Break reminders are the only break timer.
- Skills has a Show control with All and Hidden. Hidden lists the skills you hid and brings one back with Unhide skill.
- Show a solution on an exercise opens what the agent wrote in the conversation it came from.
- Recently learned on the Overview lists what Teach me, a walkthrough check or an exercise review noted you learned.
- Assistant notifications and sidebar badges are held during quiet hours when Hold assistant pings is on, and the Overview says how many were held this week.
- Puzzles is a tab on Wellness & Learning. The desktop app reads the day's result of Wordle, Connections, Daily Orbs and Catfishing from the game page in the built-in browser, only on those four sites; the phone and web keep the Played tick.
- Reflections says when the Wellness assistant drafted each week.
- Adding an MCP server, Outline, LiteLLM or Last.fm is one dialog with a numbered step strip: address, credential, test, name. Back keeps what you typed, the test runs by itself, and the token expiry is chosen beside the token.
- Each integration account has its own page with Overview, Approvals, Availability and Usage tabs. The integration page lists its accounts as links with the token expiry beside each.
- Changing an account's expiry, token, approvals or availability opens a dialog with Cancel and Save instead of editing in place. So do creating or renaming a group, adding a folder to a group, and editing a memory.
- Email, Zulip, Stoat and Calendar use the same step dialog as other integrations when you add them.
- An agent can ask for API access to an integration with request_api_access. After you approve, it calls the integration's API through Mycelium with a token that expires, without ever holding the integration's key, and you can revoke it on the account page.
- Any string argument of an integration tool can be a file in the conversation folder, written as {"file": "path"}. The node reads it, up to 25 MB, and Mycelium passes the exact text, or base64 for attachment tools, so a long document never passes through the chat.
- Providers has a Decisions section for an OpenJev or Jev-compatible server, and agents get a decide tool that answers yes or no, choice and score questions with probabilities.
- Sidebar rows open their menu on right-click, long-press, the context-menu key or Shift+F10. The three-dot button is gone from the sidebar and stays in tables.
- Buttons never truncate their label, and rows of buttons wrap on a narrow screen.
- Right-aligned table columns keep 12dp of padding after their header and cells.
- Artefact cards and the viewer show the project and conversation an artefact came from, and the list filters by project.
- A failed refresh offers Retry, a list that fails its first load says so with Retry, and every list has a Search box as its first control.
- A workflow card that was stopped, failed or has made no progress for 15 minutes says how it ended instead of spinning.
- A proposed instructions edit for a group opens that group's page, where its Instructions tab shows the proposal, and Decline can skip telling the agent.
- Assistant settings are split into Identity, Instructions, Models and nodes, Storage and secrets, and Schedules and actions, each saving on its own. A new assistant asks only for a name, group and instructions.
- Assistants have Actions: saved prompts with inputs, run from the Actions button or a pinned chip above the composer, and used by schedules. An assistant can propose a new action for you to approve.
- Calendar is a new integration for any CalDAV server such as Nextcloud, Radicale, Fastmail or iCloud. Give the server address, username and an app password, and Mycelium finds your calendars. Agents can list calendars, list and search events in a date range, and create, change or delete events, which ask first.
- Agents can list, create and update assistants over MCP. Creating and changing instructions wait for approval, other changes apply at once and can be undone, and the assistants scope of request_api_access covers the API.
- Wellness is a built-in assistant that drafts reflections, the end-of-day wrap-up and exercise reviews. It replaces the weekly reflection setup, which no longer has its own node, agent, folder or schedule.
- Usage history, opened from the arrow on the Usage card, charts the 5-hour and weekly windows over time with tokens per percentage point, the API-equivalent value and the subscription price, and is backfilled from orchestrator readings and imported conversations.
- Activity has an All time period that groups the chart by day.
- A conversation stopped by a usage limit shows a card on the first limit reply saying it is paused until the reset and resumes automatically by default; a second limit reply updates the same card.
- A workflow whose agents hit a usage limit says it is paused until the reset and resumes with its conversation.
- Quiet hours are now Mycelium hours: Time and focus sets when the app is open, with a day-by-day Opens and Closes table and Closed all day, and 24:00 as a closing time.
- Every group has hours of its own on a new Hours tab of the group page, and the Groups settings and table show each group's hours. New groups are always open.
- A group that is closed leaves the sidebar, the group selector and every list, its notifications and assistant pings wait until it opens, and agents, workflows and schedules keep running.
- The last 15 minutes before a group closes show a countdown above the main bar, and the composer works as usual.
- Selecting a closed group, or opening the app while everything is closed, shows a calm screen with the time it opens again and the groups that are open now.
- Open a closed group for 15 minutes asks why, shows a countdown with Close now while it is open, and the Overview lists what was opened out of hours this week.
- New conversation has a Model menu that lists the models the node's harness last reported, with the default model first, even before any conversation is open. The assistant form and the composer use the same list.
- The new branch field of New worktree says which folder the worktree is created in and which branch it starts from.
- An agent can show an Outline document beside the conversation with open_outline. The desktop app opens it in a side panel with Outline's own page; the phone and web open the system browser.
- Open in IDE: a split button in the conversation header, and a button on file cards, the Changes view and exercises, opens the folder in Zed, VS Code or JetBrains. Settings, IDEs chooses which are offered and their commands. A folder on another node opens through the IDE's SSH remote by way of Mycelium.
- A session that is idle when the instructions change closes and resumes under the same session id before its next message, so it follows the new instructions.


## v0.11.0

Experiments are now just Voice input: everything else that was an experiment is on for everyone.

- **Wellness & Learning**, a new sidebar page:
  - Time with agents and time looking at sessions.
  - Break reminders that never take focus, quiet hours per weekday, an optional 25/5 Pomodoro, and daily puzzles for breaks.
  - Teach me mode, where the agent gives hints instead of code.
  - A short check of understanding at the end of a walkthrough.
  - Weekly reflections drafted for you to edit.
  - Skills drawn from your work, and practice exercises on their own branch that an agent reviews without editing.
- **Integrations, redesigned:**
  - Cards by status, with search and filters.
  - A catalogue to add integrations from.
  - A page per integration with its accounts.
  - Per-account approval and result-review settings at three levels: integration, read or write, and tool.
  - Token expiry, with a warning before a token lapses.
  - Availability by group.
  - A usage tab and an activity log of every call.
- **New integrations:**
  - **Last.fm:** reads with an API key; love and scrobble after signing in.
  - **GitLab and GitHub MCP servers,** added with their forge.
- **Zulip asks before every call,** and you review results before the agent sees them: share, remove items or withhold.
- **New and re-enabled integrations work in open chats** without a reload.
- **Subagents and workflows need your approval:** Create, Create and connect, or Don't create. Agents can list nodes, accounts and folders to choose from.
- **Assistant secrets reload as soon as they change.**
- **Archived assistants** are listed under Archived and can be restored.
- **Idle assistants** stay in the sidebar.
- **Assistants and workflows get row menus,** with Close session, Retry and Stop where they apply.
- **A failed conversation can be dismissed** so it settles.
- **Continue on** lists every signed-in account of every harness, and the account picker no longer offers signed-out accounts.
- **Fixed: an assistant stuck after Continue on,** and messages that were held silently when a node was away.
- **Tables:** one table component everywhere. Columns no longer wrap or get squeezed; tables scroll sideways when they need to.
- **Walkthroughs:**
  - Regenerate is now **Iterate**, with an optional comment, and it sends your notes.
  - An empty scope no longer replaces the current walkthrough.
  - Notes no longer appear twice.
- **Layout:**
  - The side panel widens until the chat reaches its minimum.
  - The group selector fills the sidebar width.
  - Sidebar branch names stay on one line.
  - Assistant settings have a back button.
  - The worktree row under the composer is gone, and lives in the Git tab.
  - "Scroll to the newest message" stays clear of pinned cards.
- **Stoat** defaults to https://stoat.chat/api.

Folder browsing, account switching, banked resets, steering, live secrets and running exercise tests need nodes on v0.11.0.

## v0.10.1

- Experiments can be switched on again. Ticking a box no longer fails with a server error.
- Each experiment has a short description.
- Send now adds your message to the turn that is running, without stopping it. Agents that cannot take a message mid-turn, and nodes older than v0.10.1, still stop the turn first.
- Uploaded project logos show in the sidebar, the project menus and Projects.
- The project menu in a conversation shows each project's icon, and names the node only when projects are on several nodes.
- The group selector fills the width of the sidebar.
- The composer's picker row fades out where it is cut off.
- The sidebar no longer has a project filter.

## v0.10.0

Several features are new experiments, off until you turn them on in Settings, Experiments.

- Claude sign-in saves the login to the account it was started for, so several Claude accounts work on one node. The sign-in link is no longer cut short, and a pasted code completes the sign-in.
- Experiments in Settings turn Tasks, Workflows, Instructions editing, the Details and Subagents sidebars, walkthroughs and moving chats on or off for you. Turning one off keeps its data.
- Groups (experiment): Settings, Groups creates, renames and deletes groups and edits each group's instructions and folders. The group filter sits at the top of the sidebar, says when a hidden group needs you, and also scopes History, Artefacts, Tasks, Workflows and Schedules.
- Assistants (experiment): persistent assistants with their own instructions, group, storage and secrets, one thread that moves between chat and a node, ordered model and node fallback, optional Git backup, and schedules that wake them.
- Continue a conversation on another account, harness or chat model from "Continue on…" in the composer, and optionally switch automatically when a usage window is full.
- Projects: browse folders on a node instead of typing a path, put a conversation in a project from its breadcrumb, filter the sidebar by project, see branch and pull request on each row, and fold finished conversations into Settled. The Projects tab shows its last listing straight away.
- The model menu searches, groups OpenRouter models by vendor and lists favourite and recent models.
- Memory records which model, agent, node, account, project and conversation wrote each memory, and recall shows the source. Memory banks (experiment) limit recall by model class and group.
- Conversations in connected groups can message each other and start conversations in each other's projects (experiment).
- Voice input (experiment): dictate into the composer. Android and the web use the device's speech recognition, which may send audio to Google; the desktop uses your speech-to-text provider.
- Integrations: Outline by address and API key, MCP sign-in with OAuth, and LiteLLM as a gateway and as a chat-model provider.
- Codex banked resets show on the usage card, and Use reset spends one after asking.
- Email can delete and move messages, including between accounts. Deleting, moving and sending always ask you first, from agents as well as chats.
- The composer shows context use in a compact control, and the desktop browser shows its context menu inside the app.
- A running workflow stays pinned beside the chat after the turn that started it ends.

Folder browsing, account switching and banked resets need nodes on v0.10.0.

## v0.9.1

- Email, Stoat and Zulip connect directly from Integrations. Agents can read and search them, and sending from a chat always asks for approval. Email requires TLS or STARTTLS.
- The open conversation shows its reported context use and recorded spend above the composer.
- Anthropic chats and supported models on OpenRouter use prompt caching, with cache reads and writes included in usage.
- Projects find an icon in their repository without replacing one you chose. Existing conversations in project folders and worktrees regain their project association.
- The desktop browser renders at native resolution, relays page cursors and refreshes reliably when an artefact first opens.
- Artefact previews render at 2× pixel density, and usage bars show an even-pace marker and refresh when a window resets.
- PDF artefacts open in PDF.js with scripting disabled. Save to Mycelium stores annotations as a new version for the owner.
- The design editor keeps toolbar controls apart, makes small objects easier to select, shows resize cursors and can zoom to an artboard.
- Swiping inside an open artefact no longer opens the phone sidebar, and Codex sign-in is only offered on nodes where Codex is installed.

## v0.8.1

- The Usage card puts "last updated" at the right edge, keeps use and time left whole, gives the forecast as a relative time, and draws bars at 90% or more in red.
- A full weekly window hides the 5-hour window, which cannot be used until the week resets.
- Integrations can be renamed from their row menu, with the same naming rules as adding one.
- The desktop's built-in browser fills its panel again at UI scales above 100%.

## v0.8.0

- Chat models can propose tasks and ask questions without leaving unreadable tasks. Tool failures are reported as failures rather than declined approvals, and existing affected tasks are repaired.
- Codex MCP calls show tool cards with arguments and results, including batched tool execution, instead of shell command blocks.
- Settings has grouped navigation and a Providers page combining harness accounts and chat models. Unused generated signed-out accounts stay hidden, and large model lists are searchable and compact.
- Import separates progress from row actions, Instructions shows global and group layers, and Memories scrolls correctly with retryable load errors and less padding.
- Integrations replaces Chat tools, with compact MCP rows and direct or gateway connections. Agent connections has a clearer name, and Sessions sits above Account.
- Row actions use vertical three-dot menus consistently across Settings and the existing conversation, node and project menus.
- The Usage card is compact: each window shows its forecast, use and time left on one line above its bar, a window whose reset has passed shows as reset, and a full window no longer predicts when it runs out.
- OpenRouter usage shows spend against the key's active limit with a bar and the total below, sorts first when the open chat uses it, drops the repeated "OpenRouter: OpenRouter" name, and providers can be renamed in Settings.
- Ctrl+Backspace in the composer deletes only the word before the cursor, and a pasted badge counts as one word.

## v0.7.1

- Workspace rows in the sidebar show their project name in bold before the title again.

## v0.7.0

- Artefacts belong to projects and chats, with view/edit links, access requests and agent discovery.
- Edit files in Changes, stage, unstage and revert individual hunks, and open commits from History. Git commands find git-town through mise and use approved forge credentials for fetch and push.
- Run native workflows and subagents across nodes, agents and chat models, with dependencies, approval gates, nested workflows and saved progress.
- Approve a connection between chats to allow future messages in both directions, and revoke it in Settings.
- Goals show the running turn and reported token usage before it finishes. Agents can read their goal and account usage.
- Usage shows OpenRouter API-key spend in USD. Last successful usage readings and their fetch times survive restarts on the control plane.
- Project selection is searchable, the desktop composer keeps Stop and Send visible, and the first message appears immediately in a new conversation.
- Codex Guardian Review instructions render as cards, design saves finish correctly, and agents can set their project's icon from an image file.
- Startup recovery finishes before new chat replies are accepted, preventing an interrupted startup from leaving a new reply unfinished.

## v0.6.12

- The desktop's built-in browser is drawn by the app itself: the panel beside it resizes again, typing and clicks reach it directly, it follows the app's UI scale, and the scroll wheel and Ctrl+scroll zoom go the right way.

## v0.6.11

- Ctrl+Z and Ctrl+Shift+Z in the composer undo and redo pasted text and image badges whole, a word at a time for typing.

## v0.6.10

- A project without a chosen icon shows its first letter, so workspace rows show the project by its icon and no longer put its name before the title.

## v0.6.9

- Close session ends the agent of any conversation that still has one running, including ones waiting for review.
- Codex accounts show how much of their 5-hour and weekly usage is left.
- A sign-in that fails or is cancelled no longer leaves a signed-out account behind.
- Each Sign in button names the node it signs in on.
- The sign-in window shows Codex's output without colour codes, opens the right link, and copies the one-time code for you.
- Workspaces in the sidebar read "[project] title".

## v0.6.8

- The desktop app draws artefacts inside its own window, so they no longer open a separate blank window or show black.
- Hover over, or tap, an image in the composer to see a preview of it.
- The import page counts conversations, not every subagent inside them.

## v0.6.7

- Everything you see says artefact rather than artifact.
- Markdown artefacts are drawn by the app itself, so they read like chat messages and work everywhere.
- Pasted text and images sit in the composer as rounded inline badges, removed whole with Backspace.
- On a phone the sidebar only opens on a clearly sideways swipe, so scrolling an artefact no longer pulls it open.
- Claude Code and Codex sessions get the tools of your connected tool servers, and Mycelium tells running sessions when those change.
- Chats name each connected tool server and its tools, so they look them up instead of saying they have none.

## v0.6.6

- The desktop app starts its built-in browser when it opens and waits until it is ready, so opening an artefact no longer shows an error dialog on JetBrains Runtime 25.

## v0.6.5

- The desktop app's built-in browser starts on JetBrains Runtime 25, which the Nix package uses, so artefacts show inside the app there too.
- Each conversation remembers its own right-hand panel.

## v0.6.2

- Queued messages go to the agent together when its turn ends, so several points arrive in one turn without interrupting it. Send all now interrupts with all of them.
- Send now no longer stops the message it just sent.
- Pasted text sits inline in the composer as a chip, and Backspace removes it whole.
- Settings has an Import page for transcripts and Claude chats.
- Instructions shows only proposals still waiting, with decided ones behind Show decided.
- The right-hand panel stays open as you switch conversations.
- The Design viewer in the app follows your light or dark mode and accent.
- The desktop app says in its terminal output why its built-in browser could not start.

## v0.6.1

- Artefacts open inside the app. A conversation's Artefacts panel shows one beside the chat, and the full view keeps its chat beside it. Both reload when an agent saves a new version. The desktop app now runs on JetBrains Runtime for its built-in browser.
- Artefact cards show a preview: a snapshot of pages and designs, the opening lines of Markdown, and images themselves.
- Nodes update themselves when the control plane is newer, and the node page has an Update button.
- Working statuses are reliable: a chat shows Working while its agent replies, even unprompted, and returns to Review when nothing is running. The composer only shows Stop while the agent itself is answering.
- The workflow card lists stages before they start, shows each running agent's tokens, turns and time as they change, opens an agent's conversation when you click it, and marks agents that stop responding.
- The Subagents panel names workflow agents by their label, keeps the parent's list inside a subagent, and shows how long ago each was active.
- On a phone, swipe right anywhere to open the sidebar, and long conversations load far fewer messages.
- History hides empty conversations, shows how much of a conversation's tokens ran in subagents, and keeps its columns aligned.
- The git Stack tab is one column with aligned Switch buttons.
- Retry tells the agent to continue, and resuming after a usage limit says so.
- The Design editor uses the app's colours, and keeps text you type when you click away.

## v0.5.4

- Agents can ask for SSH access to one of your nodes. You approve it in the chat for a set time, and can shorten it or revoke it. The connection goes through the control plane, so the node opens no port.
- A long paste becomes a Pasted text badge in the composer and in the sent message, and expands in place.
- History leaves subagents out, counts their tokens and cost in their conversation's total, and keeps every row in line with the headers.
- Chat rows drop the agent icon and the indent, and show a dot on the right: white for a reply you have not read, blue while the agent answers.
- Much less writing to the database while agents stream, and faster lookups of a conversation's messages and turns.

## v0.5.2

- Subagents leave the sidebar. A conversation's subagents are listed in a Subagents panel on its right, with their status.

## v0.5.1

- A message you send after the agent's turn has ended goes straight to it, even while background agents keep the chat showing as working.
- Stop leaves the chat stopped, and ends the turn after a few seconds even when the agent keeps talking.
- The composer shows Stop only while the agent is answering you, not while background work runs.

## v0.5.0

- A public Design opens for visitors, instead of failing to load its viewer.
- Text typed into a text box or sticky note in the Design editor is kept when you click elsewhere or pick another tool.
- A public artefact's menu also copies its private link.
- A chat that finishes with messages still queued does not notify or chime until the queue has run.
- Linux desktop notifications stay up for five seconds.
- A diff of a long file shows only the changed lines.

## v0.4.0

- Artifacts: agents create, update and read web pages, Markdown, files such as PDF, and Design boards, each kept with every version, comments and a small data store pages can save to.
- A Design editor of your own: artboards an agent fills with HTML, shapes, text, sticky notes, a pen, comments, present mode and version history, saving as you work and updating live when the agent changes it.
- The Artifacts screen sits below History in the sidebar, with filters, search, public or private, rename and delete.
- Artifacts are served from their own host on their own domain, behind their own sign-in, private by default and public when you choose.
- History is a table with sortable and resizable columns, filters, grouping by project or node, and saved views.
- Agents can ask for temporary API access, scoped and expiring; you choose the scopes and approve it in the chat, and the token goes to a file on the node, never into the chat.
- Subagents appear as read-only side chats under their conversation, live and when imported.
- Imports wait for a transcript to finish uploading, and an account signed in on several nodes shows once.
- The workflow card shows each agent's tokens, turns and time.
- A chat stays Working while its workflow agents are still running, and moves to Ready to review when the last one reports.
- Slash commands and modes are kept on long chats even though their history loads lazily.
- A goal no longer prompts the agent in a loop while it waits on its own background work, and shows the weekly usage for chats without a recorded account.
- A chat waiting for a usage limit to reset sits under Working as "Waiting for reset", with times in your own time zone.
- The Usage toggle is an icon beside Activity and Settings, and the status dot sits in line when a project has no icon.
- What's new shows the changes since the version you last ran, and Settings opens it at any time.
- The release flake has an immutable versioned URL beside the moving one.

## v0.3.13

- Linux desktop notifications work through notify-send, and the web client asks for notification permission on the first click.
- Cloning an existing folder over the forge's own SSH or HTTPS address registers it as that repository, whatever host name the forge uses for SSH.
- Close session in the sidebar and header menus ends the agent process and leaves the chat in place.
- Agents can find and message other conversations, with an approval card before each message.
- Long chats load the newest messages first and older ones as you scroll.
- A restarted node restores an idle session as idle, Stop clears a turn the agent is not running, and a session whose keeper died ends instead of staying live.

## v0.3.12

- Pinned cards sit beside the chat.
- Notifications come with a chime.
- Walkthrough notes added, removed or sent show live on your other devices.
- Claude has a solid mark.
- A conversation started in a folder inside a project belongs to that project.
- A tool search in the thread is one quiet line naming the tools it loaded.

## v0.3.11

- Projects have a named icon or an uploaded logo of up to 256 KiB.
- Sidebar rows show status badges, and there is a usage card and an image viewer.
- Composer drafts are kept.
- A schedule's prompt, cadence, each-run choice and time zone can be changed after it is made.
- Row menus offer actions for each conversation.
- Signing in from the app asks the node for the sign-in methods it accepts.
- Progress bars and bar-chart tracks in generated UI are visible when empty.
- An import runs as a background job that commits each conversation on its own, lists files that fail with a reason, skips files still being written and shows its progress.

## v0.3.10

- Chats call tools from Mycelium and remote MCP servers, ask before each call and find tools with tool_search.
- Tool calls and results are cards, and an approval shows the tool, server and arguments with Allow once, Always allow and Deny.
- Settings, Chat tools adds, checks, switches, trusts and removes the MCP servers chats may call.
- Changes is one scrolling view of every changed file with sticky headers, stage, collapse and keyboard support.
- A Stack tab shows the current branch's Git Town stack with counts, sync and push tags and each branch's pull or merge request, and syncs or proposes when git-town is installed.
- A usage limit appears as its own card with its reset time, and a failed conversation can resume automatically after the reset.
- The walkthrough is a tab again, closed from the tab itself.
- When the terminal's shell exits, the side panel closes.
- Tool results longer than 50000 characters are cut before they are stored or sent.
- A NUL character in an event, a search text or a remembered text no longer makes the save fail.

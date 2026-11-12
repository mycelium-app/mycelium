# Changelog

## v0.34.24

- A Claude conversation keeps its Mycelium tools when its session restarts, for example on a move to another account at a usage limit or a model change. Each run of an agent now has its own credentials file, so the end of an earlier run no longer removes the file a newer run is using.

## v0.34.23

- A message you send arrives once, even after a double tap, a retry or a reconnect, and a double tap on Start makes one conversation or chat. Voice sends are covered too.

## v0.34.22

- elect.horse, the daily gerrymandering puzzle, is in the Wellness puzzles. Mycelium reads today's result from the game when you open it, with the seats won as the score, or you can enter it by hand.
- The desktop app zooms out to 50%: Ctrl+- steps through 75, 67, 60 and 50%.

## v0.34.21

- Switching to another tab and back keeps the Outline panel as it was: the open document, collection and account, the expanded folders, the tree's scroll position and whether the panel is open. On the desktop the document itself keeps its page, scroll and history without reloading.

## v0.34.20

- You can record a Claude limit reset by hand, with its kind and expiry, from the Claude account's menu on Settings, Providers. Recorded resets show in the Usage card like Codex banked resets, soonest expiry first, and Use reset marks one used in Mycelium without contacting Anthropic. Expired and used ones drop off.
- One malformed tool can no longer hide every Mycelium tool from an agent. A session that starts without its Mycelium tools says so in the conversation, and a client that reconnects after the control plane restarts is told to fetch its tools again.

## v0.34.19

- The Usage card marks the account an assistant's chat runs on as This chat, as it does in any other conversation.

## v0.34.18

- Agents can show images in a reply. A new Web search integration offers search_images, and images whose exact address came from that search load in place, fetched by the control plane and never by your device. Several in a row form a gallery, and the enlarged view names the site and opens the page the image came from.
- Any other image in a reply waits behind Load image from its host, and nothing is requested until you press it. An image on a private network, over 10 MB or not an image says why it is not shown and keeps a link to it.
- Fetches made for agents refuse private addresses on every redirect, not only the first address.

## v0.34.17

- The desktop app stays responsive while agents stream into long conversations. Pinned cards no longer re-read every earlier reply, which pushed recent messages out of the formatting cache, and Needs you no longer rebuilds a whole conversation for each of its waiting approvals on every streamed word.

## v0.34.16

- A side chat or fork of an agent chat stays in Chats under its parent instead of also showing in Workspaces, and shares its parent's folder, which is kept until no conversation uses it.
- A side chat of an assistant's conversation stays with its assistant.
- An agent chat continued on a model and back again returns to Chats with a fresh folder, and a model chat continued on an agent with no folder becomes an agent chat in Chats.
- The subagents card starts collapsed unless a subagent is waiting on you.
- An integration tool whose input schema is a reference is listed with the object it names, so agents can call it.

## v0.34.15

- An environment started from an assistant's chat shows there, in the same side panel as in any other conversation. Assistant chats now have the full panel rail.
- Using an existing folder as a project recognises a remote that names the forge through an SSH config host alias or a git insteadOf rewrite, by resolving it the way git does. The node needs Mycelium v0.34.15 or newer for this.
- A folder on a node in the New project dialog no longer shows a ~/projects path under Set up on.
- A message proposal in Needs you is headed by its assistant's name and shows the reason the agent gave for it, not the conversation's latest message. An agent can withdraw its own pending message with withdraw_message, and a newer message to the same conversation replaces the older one.

## v0.34.14

- The Usage card is half as tall: each window is one row with its bar, percentage and reset, and one line says when usage was last read. Anthropic API spend shows beside the subscription accounts, today's and this month's, once an Admin API key is added to the Anthropic provider in Settings, Providers. OpenRouter spend uses the same row.
- Agent chats in the Chats list sit flush left like every other chat, with the same status dot and agent mark, and an Inbox item without a project no longer leaves an empty icon space.
- Feed badges keep their full label at every text size.
- The project tools name the node by its name instead of its id.
- A Claude conversation's Mycelium credentials no longer appear in the node's process list: the node keeps them in a private file that the agent reads, and removes it when the session ends. You can revoke a conversation's Mycelium credentials, or one credential by its SHA-256, with POST /api/v1/mcp-session-tokens/revoke. The node needs this release.

## v0.34.13

- A new chat can open in an existing worktree of its project, including worktrees made with plain git. The node needs Mycelium v0.34.13 or newer.
- Cloning a project no longer reports failure when the clone worked, and a clone still becomes a project when the app stops waiting. Cloning again onto a folder that already holds the same repository registers that folder instead of refusing it, so a clone left behind by an earlier failure can be recovered. A new project is filed under the group it was started from.
- Three changes announced in Mycelium v0.34.11 that were missing from v0.34.11 and v0.34.12 are back: a model, mode or setting the agent refuses leaves a notice instead of failing the conversation, Needs you lists every waiting approval from any conversation that is not archived, and Stop shows only while a turn is running.

## v0.34.12

- The Wellness & Learning page has a Start a break now button that starts a break straight away, the same break the reminder offers.
- A conversation that arrives just after the app connects stays in the list instead of being dropped by the first refresh.

## v0.34.11

- A conversation that reaches a usage limit is caught even when the agent sends the limit message before a usage update or after its turn has ended, so it moves to another account instead of sitting still. A side chat at a limit with every account spent moves on when another account resets.
- A model, mode or setting the agent refuses leaves a notice in the conversation instead of failing it, and the model button shows the model the agent is using.
- Stop shows only while a turn is running and turns into Stopping when pressed.
- Needs you lists every approval that is waiting, from any conversation that is not archived.
- Release and pipeline watches report back even when a webhook was registered, and a workflow step stays running while its agent waits on background work.
- Feed posts can carry inline badges for a project, type, outcome or anything custom.
- You can open a to-do item to its details and edit the list, saving with or without telling the agent, and an assistant's list survives Clear.

## v0.34.10

- Voice mode in any conversation: one control in the composer listens, then reads the reply aloud. The agent is told when voice mode starts and stops, and can add a short spoken version of its answer, which is the only part read aloud and is never shown in the thread.
- The subagents card also lists the subagents a conversation started itself, with the reply field when one is waiting, and a subagent that fails tells the conversation that started it.
- A group connection can work one way or both ways. Agent connections shows each connection's direction and can change it, and the approval card connects only the direction asked for.
- A conversation whose finished turn asked you a question moves straight away instead of waiting for a turn end that never comes.

## v0.34.9

- Action items: an agent can add things for you to do, such as an update only you can run. Each stays in Needs you, even after you write in the conversation, until you mark it done or the agent sees it has happened and resolves it. A conversation can hold several, adding one raises a notification, and a conversation with an open item is not archived when idle. Agents use the add_action_item and resolve_action_item tools.
- A chat turn's token usage is saved together with the end of the turn, so the Activity page never shows a finished turn with no tokens.

## v0.34.8

- Opening a chat that runs an agent in a scratch folder keeps the sidebar on Chats instead of switching to Workspaces.
- A workflow step or subagent that names a model starts on that model, checked against the models its node reported, instead of failing with "none reported yet".
- Each release publishes the local speech runner image as ghcr.io/mycelium-app/mycelium-local-speech.

## v0.34.7

- Settings has a Voice section holding every voice setting: how you talk, how speech becomes text, how replies are spoken and what leaves the machine, with Try it for speech to text and reading aloud. It replaces Privacy and takes the Speech table from Providers, the voice and speed from the Voice assistant and dictation from Experiments.
- Speech to text can run on the server with a Hugging Face model. The control plane downloads it once, shows its progress and size, reuses it and can remove it, so audio stays on the server.
- An OpenRouter provider lists its transcription models, such as Voxtral, and transcribes with them.

## v0.34.6

- The subagents card lists waiting and running subagents and those finished in the last hour, in a list that stops at about five rows and scrolls. It is not drawn when nothing qualifies; older subagents stay in the side panel's history.

## v0.34.5

- Tandoor: edit and delete recipes with their steps and ingredients, set recipe images, import a recipe from a page, find foods, units and keywords, set food nutrition, change and delete meal plans, and uncheck or remove shopping list entries. search_recipes takes filters, and get_recipe returns everything update_recipe takes. check_off_item no longer takes checked; use uncheck_item.
- A conversation that hits a limit with no free account resumes when another account in its routing resets.
- An approval review refused for exhausted credits counts as the account's limit, so the conversation moves to another account.

## v0.34.4

- A message sent while an assistant's session is mid-turn queues into that session instead of starting a new one.
- An API token granted to a conversation reaches conversations in its group and connected ones, with the same limits as reading another conversation.
- Re-authenticating an OAuth integration such as Outline signs the same account in again, keeping its name and tool names.
- The start proposal test for a call with no conversation runs again.

## v0.34.3

- The Outline side panel shows the collection's document tree above the document, with an account and collection picker, on desktop, Android and the web.
- Clicking a Linux notification's body opens its conversation, break notifications open nothing, and a failing notify-send falls back to gdbus.
- A Codex conversation on a signed-in account resumes after moving to another node. Only the control plane needs this version.
- The unread dot sits in the row's trailing status slot, where the time or status icon goes.

## v0.34.2

- The subagent proposal card lets you change the model, using the same model picker as the message box. A Claude agent that has offered no models in a conversation falls back to the models its node reported.
- The start proposal integration tests run again.

## v0.34.1

- The built-in Voice assistant is no longer listed under Assistants in the sidebar; it stays at the bottom left, in the Inbox dock and on the Assistants page.
- Voice conversations never wait in Needs you or add to the Inbox count.
- A Claude conversation on a signed-in account resumes after moving to another node. Only the control plane needs this version.

## v0.34.0

- A conversation's watches are one Watching control in the header.
- A conversation with an agent reply you have not opened is unread: its sidebar row shows a white dot and a semibold title.
- A favourite that names only a harness, with no model, can be chosen.
- Inbox items and sent messages render Markdown; compact previews keep their line limits.
- Managed conversations idle for 3 days archive themselves, and managers can archive theirs. A manager's message reopens an archived conversation it manages. The 3 days is fixed, with no setting.
- Settings, Providers has a Speech section: speech to text and reading replies aloud each use a stored provider and model, chosen in a model dialog, with "Choose a model" when no model is recognised.

## v0.33.0

- Tool calls no longer fold into a "Show N tool calls" row; they show as they did before v0.32.0.
- Subagents show in the thread as a card, each row with its status, harness, model, status and run time. A subagent waiting for an answer gets its own card with its question and a reply field that sends straight into it.
- A pending artefact access request appears in the Inbox's Needs you.
- Renaming an integration account changes its title and keeps the tool names agents use.
- A backup asked for while one is running goes round again instead of being dropped, so a deletion always reaches the repository.
- read_artifact takes save_to, writing what it reads to a file in the conversation folder.

## v0.32.0

- Workflows leave the sidebar for the conversation's side panel: Subagents and workflows is one history, newest first, and a workflow opens there to its steps, with a waiting gate's Decline and Approve in the gate's row. The Workflows page and manual workflow form are gone.
- show_conversation brings another chat into an assistant's thread as a live card: its state, latest result and everything it waits on, with the same approval cards as the chat itself, and a reply box that sends straight into it as you. Only you can approve.
- Three or more tool calls in a row fold into one row, and anything waiting on you inside them is shown above the composer instead.
- Chats and workflow steps that agents start use Any model when they name no model or account, and take an optional routing.
- The Inbox's Voice dock always sits below Needs you and takes typed requests; the microphone setting only controls listening.
- An assistant's notifications carry the assistant's name rather than the thread's title.
- A conversation restarted while busy, for new secrets, a reload or another account, no longer gets stuck before it starts. Nodes need this version.
- design_set_artboard takes the artboard's HTML from a file in the conversation folder.

## v0.31.0

- Tasks is retired: the board, its sidebar link, the /api/v1/tasks routes and the propose_task, ask_question, list_tasks, approve_task, decline_task and close_tasks tools are gone, and the tasks table is dropped. An agent asks its question as an ordinary message and ends its turn; a turn whose last agent message ends with a question leaves the conversation in Needs input, and the thread shows "Waiting for your answer" after the last message.
- The Inbox feed is native: agents post one sentence per release or finished feature with post_to_feed, shown newest first with a link to the chat. Murmur is no longer a feed source.
- An assistant's conversation moves to another node with its storage, transcript and secrets. Nodes need this version.
- Chats started by another conversation before v0.29.0 count as managed even when their starter has been renamed since.
- A failed workflow marks its unstarted steps not run at the same moment, and a goal sent again shows as pending before it is delivered.

## v0.30.0

- Inbox sits under New conversation and is the screen the app opens on. Stats show tokens per second, today's spend by hour, account usage and what is running; Needs you lists pending permissions, questions, proposals, workflow gates and flagged chats from every conversation, each resolved in place; the feed shows Murmur posts when a Murmur integration is set up. Narrow windows collapse the stats, and phones get Needs you, Feed and Stats tabs.
- With Voice open on the Inbox, saying approve, decline or an answer acts on the outlined question or proposal. Permission requests and workflow gates still need a tap.
- Paperless-ngx has an api tool for any /api/ path and query, with all_pages; GET runs at once and writes follow the account's approvals. download_document saves a document's original unchanged with save_to, and get_document saves the whole OCR text when save_to is used.
- all_pages keeps the caller's filters on every page. FreeAgent's next-page links drop them, so a filtered read used to return every row after the first page.
- send_to_conversation delivers at once to a conversation in the same group, as read_conversation already allowed.
- A connection between two conversations expires a week after it was last used, and each delivered message renews it. A managing link is kept while the managed chat is busy. Settings, Agent connections shows when each expires. Group connections do not expire.

## v0.29.0

- A conversation started by another conversation is managed by it. Managed conversations sit in a Managed by assistant section below Working, collapsed by default, and never notify the person: their events go to the manager. Conversations started that way before this version count as managed.
- flag_for_person brings a managed conversation to the person with a reason: it returns to its usual section showing the reason, and notifies. The flag clears when the person writes in it, and Take over in the conversation menu ends management. Archiving the manager returns its conversations to their usual sections.
- An approved or declined assistant change no longer stays above the composer. Applied changes are listed under Changes agents made in the assistant's settings, each with Undo.
- Pinned sidebar assistants show a pin, and the sidebar lists up to 12 assistants.
- An assistant's instructions approved while it works in a folder survive the copy back and reach the folder.
- send_to_conversation says a message was queued, not sent, when the target is in the middle of a turn or its node is offline.
- Integration tests fail, saying how to start the test database, when MYCELIUM_TEST_DATABASE_URL is unset.

## v0.28.1

- Republishes v0.28.0, whose desktop and web build failed.

## v0.28.0

- A conversation started with start_conversation or start_subagent tells the one that started it when its turn ends and it has finished, failed or is waiting on the person. watch_conversation does the same for other conversations it can read, and unwatch and list_watches manage watches.
- watch_pipeline and watch_release tell a conversation when a GitHub Actions run or GitLab pipeline finishes, or when every run for a release tag has finished. Connected forges deliver through webhooks Mycelium registers; otherwise, and for public GitHub repositories, Mycelium checks every minute.
- wake_at wakes a conversation once at a time with a note to itself, and survives restarts.
- Notifications arrive as messages that wake an idle conversation. Several that arrive while it works come as one turn, each state change notifies once, and a turn started by a notification notifies nobody.
- A turn that ends with an open question leaves the conversation in Needs input instead of looking finished.
- A conversation shows its pending wake-ups and what it watches above the thread, each with Cancel or Stop.

## v0.27.0

- Add account on FreeAgent reuses the existing account's OAuth app, so it goes straight to naming and signing in. Use another app enters new credentials.
- FreeAgent accounts are titled with their company's name from FreeAgent, and an account set up without a name takes the company's name.
- FreeAgent has an api tool for any v2 endpoint. GET runs at once, writes follow the account's approvals, all_pages returns every row, and requests wait out rate limits.
- A file argument can ask for base64 with "encoding": "base64", for attachments such as PDFs.
- Any integration tool takes save_to, which writes the whole answer into the conversation folder. Nodes need this version.
- Opening the project picker puts the cursor in its search.

## v0.26.2

- Ctrl+` shows or hides the terminal in the desktop app whatever has keyboard focus in the window.

## v0.26.1

- The environment live view reconnects with a fresh ticket after its connection drops, instead of staying black.
- Publishes the v0.26.0 changes, whose build did not finish.

## v0.26.0

- Assistants can have an icon, an uploaded image or their first letter, on a colour, chosen from Change icon in their settings.
- With All groups chosen, sidebar assistant rows start with their group's name, as conversation rows start with their project's.
- A subagent's model can be named by its id or display name. A model the agent does not offer is refused before the approval card, with the models it does offer, and list_places names each agent's models.
- Workflow cards under a conversation can be unpinned, and a finished, failed or stopped workflow leaves two minutes after it ends.

## v0.25.1

- Cmd+V, Ctrl+V and Shift+Insert paste into the terminal on desktop and the web.
- Agents and terminals start with the folder's direnv environment when direnv is installed and the folder's .envrc is allowed.

## v0.25.0

- Agents can list the task board with list_tasks, showing each task's column, age and source, and approve, decline or close tasks with approve_task, decline_task and close_tasks. Each change asks on an Approve or Deny card in the conversation, and closing several tasks asks once for the whole batch.
- Tasks have a Closed state. A closed task leaves the board.
- Agents can register a project with create_project and change its group or test command with update_project, each after an Approve or Deny card.
- API tokens with conversation read access can read the task board.
- Swiping the phone sidebar open closes the keyboard.

## v0.24.4

- The pull request panel renders markdown in its summary and comments.

## v0.24.3

- Any account, and each favourite in the Any model list, now picks the account whose weekly window resets soonest while it still has usage, so weekly usage is not lost at the reset.

## v0.24.2

- The break reminder can no longer be dismissed. It ends when you start a break or switch break reminders off, and it can be snoozed once: when it comes back it offers only Start break.

## v0.24.1

- View staged in the git panel opens a Staged changes tab with only the staged files and their staged hunks, and says so when nothing is staged. View diff still shows every change.

## v0.24.0

- The default mode in Settings, Agents offers the modes each agent last reported, under its own names. Auto for Claude Code is its real auto mode and Accept edits is a choice of its own, and Codex defaults are applied with the ids Codex reports. Saved Codex defaults move to the new ids.
- An assistant working in a workspace has the Mode control of any workspace conversation.
- Each walkthrough step has a reviewed toggle, and Approve in a step marks it reviewed and opens the next one. Sections show as reviewed once all their steps are, the header counts reviewed steps, and reviewed state is kept with the walkthrough on every device. Steps are no longer marked viewed on their own.
- Check your understanding shows each question as a card with radio options, its feedback under the options, a Back to step link and a primary Check. The step list stacks above the step in a narrow panel.
- Agents can read another conversation in the same group, a connected group or a connected conversation with read_conversation.

## v0.23.1

- Tab copies a prepared squash or merge message into an empty commit field so it can be edited.
- Two-line conversation icons are smaller, with the same title alignment.
- Conversation rows hide a standalone main branch subtitle and retain feature branches and pull request details.

## v0.23.0

- The New conversation project menu lists each project once. The node follows the project you choose; when no online node has it, Run on asks you to choose a node and offers to clone it there. Tasks and Schedules pick projects the same way, and a project you just cloned stays chosen.
- Terminals keep running under tmux when the node restarts, and come back with their conversation and scrollback.
- Environments open no network ports: the desktop is shared over a private socket and the browser is driven over a pipe.
- Kubernetes outposts join with a single-use token from Add an outpost, and adopt session pods from before outposts.
- New conversation applies the chosen Mode and reported Effort before its first prompt, underlines the project in the headline and shows nodes in their colours everywhere.
- Provider accounts are named by the part after the @ everywhere, two-line sidebar rows show a larger project icon, and the phone Projects screen fits its rows.

## v0.22.0

- Stored secrets are encrypted at rest. Set MYCELIUM_SECRET_KEY, or the control plane creates a key in its data folder on first boot; back that file up. `mycelium control rotate-secrets` rotates it.
- Kubernetes outposts: several Kubernetes nodes can share one control plane, each in its own namespace with its own names and labelled pods. Add one from Nodes, Join, Add an outpost.
- Terminals survive closing and reopening the app, including when the first load fails or the node is slow to send the scrollback.
- Up in an empty composer edits the last queued message.
- Zulip tools fetch a message by id or permalink, page through history, filter by channel, topic, sender and date, accept Zulip search operators such as from:me, and say whether more messages exist.
- The place strip under New conversation is two controls: Run on for the node and a control for the checkout, worktree or folder.
- Show chat beside an app or artefact with no conversation lets you choose one, and Change chat picks another.
- A session approval for a Coolify api write covers only that method and resource, and deletes always ask.
- Permission cards close when the agent abandons the request, and an older node is told at once that it cannot create folders.
- Security hardening: the node link refuses another node's conversations and closes a flooding node, node folders stay inside the projects folder and are private, and terminals no longer inherit the node's secrets.

## v0.21.0

- Apps open inside Mycelium, beside the conversation that built them, and an open app can show its conversation next to it. Open in browser stays available.
- A new project's repository can be created on any connected forge, under its account or a GitLab group or organisation.
- Any model stays on the model a conversation is using until that model reaches a limit, then moves to the first favourite with usage left. It no longer switches back when usage returns, which would lose context.
- The usage card says the weekday a window resets when it is a day or more away.
- Ctrl and Alt with the arrow keys move and select by word across a pasted block, citation or image in the composer.

## v0.20.0

- Coolify agents can use the whole Coolify API, including databases, services, volumes and domains, and can create a database and get its connection URL. Reading never asks; anything that changes something asks first.
- The folder browser can create a folder. A new conversation with no project starts a new project with a generated name, and can create its repository on your forge at the same time.
- Repository lists have a Refresh button that fetches from the forge straight away.
- Repositories can be set up from the Projects screen on a phone.

## v0.19.0

- Terminals live on the node: they keep running through dropped connections, control plane restarts and app restarts, and come back with their scrollback. Tabs and splits are saved per conversation, so every device opens the same layout.
- The terminal on a phone is a full-screen sheet you type into with the keyboard, with a key row for Esc, Tab, Ctrl, the arrows and Paste, and one terminal at a time.
- Coolify can be given the whole instance. Agents choose the project, server and environment for each app, and an integration limited to one project can be widened from its account page.
- Environments no longer need Chromium on the node. Browser tools say when no Chromium-based browser is open.

## v0.18.0

- Agents keep a to-do list for the conversation, pinned above the composer and expandable to its checklist. Claude Code and Codex fill it from their plans, other agents and chat models set it with a tool, and whoever continues the conversation on another model or account is given it.
- New conversation chooses the project in the headline, with the place in a strip under the composer. The no-node notice is one line with Nodes inside it.
- The phone connects as soon as the server answers instead of after loading everything, and coming back to the app reconnects a stale connection at once.
- Move is a split button: move after this turn, interrupt and move now, or copy the conversation to another node and keep this one running.
- The model menu shows names only, its account submenu has icons, and the Fast mode and Agent harness settings are gone from the composer bar.
- Shorter switch and Any node lines, markdown tables with aligned cells, letter avatars for every assistant, and assistant actions that run on the assistant's own settings.
- Fixed the model menu crashing when a favourite was saved twice, and favourites that differ only by account are kept.

## v0.17.0

- One model selector combines agent accounts and chat models, with search, favourites and an account submenu. Favourites are the ordered Any model list. Selecting a particular account keeps it pinned when it reaches a usage limit.
- New conversation is the default screen. Schedules is in Settings; History, Tasks and Workflows are experiments, off by default. Groups have their own icons.
- Terminals open below the conversation with Ctrl+`, with tabs, splits and draggable dividers. They keep running when you leave the conversation. Agents can read or type through the existing approval flow, and session permissions are available from the mode menu.
- Any node chooses among the nodes holding the project, using free resources and the node preferences in Settings, Agents. The place menu can clone a project onto another node; sending while cloning waits for the checkout to finish. Conversations can be snoozed, and chat width can be Comfortable, Wide or Full.
- GitHub, GitLab and Forgejo share the issues and pull requests list, with authors and assignees. Pull request previews show Summary, Timeline and Code, refresh when the forge changes, and can filter unresolved comments. Pull request state appears on conversation rows, and typing @ in the composer can refer to an issue or pull request.
- Citations are inline composer chips, with surrounding text written in the composer. Path badges reveal local files in the system file browser and retain remote node context. Queued messages have a Send now split button, and assistant files and secrets are edited in dialogs.
- Chat models pick up MCP tool changes between tool rounds. An OpenAI-compatible chat provider can also supply Decisions answers from token probabilities.
- Fixed composer keyboard handling, overlapping daily activity, narrow sign-in screens, stale terminal scrollback and approval requests whose identifiers are reused. Codex credit exhaustion shows the explanation and recovery actions instead of Internal error, without inventing an automatic reset time.

## v0.16.1

v0.16.0 was tagged but not published. Every screen change in this release was designed in the canonical design artefact first.

- New conversation starts from the composer: project and place sit beside the account and model controls, the project picker searches and lists recent projects first, the place menu chooses the node and an existing checkout or a new worktree with its branch, and a folder is only asked for when there is no project. The composer starts on your most recent project and stays usable with no node online.
- The folder browser has the path first and a single primary action, Choose this folder.
- Cowork environments have a searchable app launcher with open windows, recent apps and apps grouped by kind, and the thread shows when an environment started, when you took over or handed back, and when it stopped.
- The live environment view renders again while the agent is in control.
- While listening, voice offers Send and Cancel, and the microphone says it sends what was heard.
- Wake phrase: say "Hey Mycelium" or "OK Mycelium" to start voice in the Linux desktop app (Settings, Privacy). Detection runs on the device, and audio is discarded unless the phrase is heard.
- Experiment: Kubernetes nodes. A node in a cluster runs each conversation in its own pod from the `ghcr.io/mycelium-app/mycelium-agent` image, with Claude Code and Codex preinstalled and agent sign-ins from a Kubernetes Secret. Turn it on in Settings, Experiments, then join one from Nodes. The node page shows its cluster settings and each conversation's pod. See `docs/KUBERNETES.md`.
- Every node page has the More menu with Rename, Drain and Revoke.
- Security, from independent reviews by Claude Opus 5.5 and Claude Mythos 5.1:
  - Each agent session gets its own short-lived Mycelium token instead of the node's credential, and Mycelium's tools act for the conversation's owner. Nodes on older versions lose Mycelium's tools until they update.
  - A scoped API token can no longer approve its own access requests, cookie requests are checked for their origin, and sign-in redirects stay on the control plane.
  - The global instruction layer and dashboards are limited to operators (`MYCELIUM_OPERATORS`, by default the first allowed email). Memories, imports, SSH access and device sign-ins are limited to their owner.
  - With `MYCELIUM_ALLOWED_EMAILS` set, sign-in needs the identity provider to mark the email as verified.
  - Outbound requests no longer follow redirects or reach link-local and cloud metadata addresses, and oversized replies are refused. Integrations on your own network keep working.
  - Forge credentials no longer reach repository hooks or a rewritten `origin`; git stays inside the projects folder.
  - Private artefacts are only shown to their owner, page artefacts run in a sandboxed frame and keep their data store, and only the owner can write to a page's data store.
  - Actual's encryption password is no longer returned with its settings.

## v0.15.6

The first release published on GitHub. v0.15.3 to v0.15.5 were tagged but never published.

Mycelium now lives at [github.com/mycelium-app/mycelium](https://github.com/mycelium-app/mycelium). The repository history was rewritten for the move, so clone it again rather than pulling into an old clone.

- Releases are published on GitHub: the binary, desktop, web and Android downloads are on the GitHub release, and the control plane image is `ghcr.io/mycelium-app/mycelium`. Nodes on this version update themselves from GitHub.
- The Android app is signed with a new key. Uninstall the old app once, add `https://github.com/mycelium-app/mycelium` in Obtainium and sign in again; later releases install over it as usual.
- The installer reads the latest version from `releases/latest/download/latest`. Its `MYCELIUM_S3` override is replaced by `MYCELIUM_RELEASES` and `MYCELIUM_LATEST`.
- On macOS the node's launch agent is now `app.mycelium.node`. Unload and remove the old Mycelium launch agent in `~/Library/LaunchAgents` before reinstalling.
- The Nix flake is `github:mycelium-app/mycelium`.
- The README and the landing page are up to date, and the landing page sends you to the README for installing.
- The control plane image is published without build attestations, and only once every download and the signed Android app are ready.
- Usage history no longer imports readings from the previous orchestrator. Readings it imported before stay in the history.
- The design editor's source moved to `clients/editor` beside the other clients.

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

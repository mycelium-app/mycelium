# Ideas

Features asked for but not yet designed into a stage. Each links what inspired it. `DESIGN.md` remains
the plan; an idea moves there when it is designed.

## Settings and accounts

- **Structured settings.** Sections for appearance, accounts, agents, sessions, forges and about, in place
  of one long page.
- **Sessions.** Every signed-in client and node session, with where and when it was last used, and a way to
  end one.
- **Past conversation import.** Per agent (Claude Code, Codex at least): find the conversations already on a
  node and bring them into Mycelium's history.
- **One account, many nodes.** The same agent account signed in on several nodes is one account, not one row
  per node. An account switcher picks which account a conversation runs as.
  Inspiration: [zerosub](https://paseo.sh/plugins/kapybara-org/zerosub).

## Conversations

- **Queued messages attached to the composer**, each with send now, edit and remove.
- **Chat titles.** Until the user sets one, the title keeps updating from a predicted title; clearing a title
  returns to the predicted one. Inspiration: [chat-titles](https://paseo.sh/plugins/iarmankhan/chat-titles).
- **Side chat.** A second, throwaway thread beside a conversation for a quick question that should not enter
  the main context. Inspiration: [sidechat](https://paseo.sh/plugins/moyf/sidechat).
- **Annotations.** Select text in a chat, file, diff or terminal output and attach it, with a comment, to the
  next message without sending anything yet. Inspiration: [quote-notes](https://paseo.sh/plugins/xzthemeofsss/quote-notes).
- **Suggested replies.** Up to four likely next replies as buttons under a finished turn; tapping one sends it.
  Inspiration: [tech-bought-my-soul](https://paseo.sh/plugins/5carl/tech-bought-my-soul).
- **Chats and Workspaces.** Two modes: Workspaces for coding agents on nodes, Chats for ordinary AI chats,
  including local models such as Ollama, which the control plane might run itself. Likely tied to groups.
- **Generated UI.** Agents return Compose UI, with a web view for anything richer.
  Inspiration: [genui](https://paseo.sh/plugins/anh-chu/genui).

## Forges and projects

- **Forges.** Connect GitHub, GitLab (including self-hosted) and others; one view of repositories across
  forges and organisations.
- **Projects.** Clone and set up a repository on a node as a project; New conversation then starts from a
  project instead of a folder path. "Projects" in the sidebar.
- **Issues as workspaces.** Start a workspace from a forge issue.
  Inspiration: [issues-workspaces](https://paseo.sh/plugins/ajma/issues-workspaces).
- **Git view.** History graph, status and diffs; Zed's git panel is the reference.
  Inspiration: [git-graph](https://paseo.sh/plugins/siyxo511/git-graph).

## Nodes

- **Activity.** Usage over time across nodes and agents: a calendar heatmap, a 30-day histogram, an hour-by-week
  timeline, the tools and models used most, and the conversations needing attention. Derived from what orchestrator
  records.
  Inspiration: [activity](https://paseo.cafe/plugins/activity/).
- **Resources.** CPU, memory, swap, disk, the busiest processes and Docker per node, with recent history and
  confirmed clean-up actions; a load pill beside a conversation's composer.
  Inspiration: [machine-status](https://paseo.sh/plugins/dutchakdev/machine-status),
  [resources](https://paseo.sh/plugins/zedobaia/resources).
- **Ports.** The ports each conversation opened on its node, with links by reach (node, tailnet, public), and
  sharing through a tunnel in the style of frp.
  Inspiration: [ports](https://paseo.sh/plugins/zedobaia/ports).
- **Update from the Nodes tab** for nodes installed with `install.sh`.

## From better-paseo

[better-paseo](https://paseo.sh/plugins/roundreddot/better-paseo) collects changes worth taking:

- **Compact timeline.** Typed rows for tool calls, reasoning and replies; tool calls start collapsed and remember
  it; hover previews for file paths; model, time and duration on finished replies; syntax highlighting.
- **Tasks board.** Lanes for needs input, running, to review and done; cards with model, project, branch, timer
  and subtasks; stop an agent or answer its permission request from the card.

## Decided in review

- **Navigation.** Tasks and Schedules sit under Nodes at the top of the sidebar. The bottom bar is
  `[Projects] [Issues]`, then the theme, Activity and Settings icon buttons.
- **Memory.** Every agent reads and writes Mycelium's memory through its MCP server, and only that:
  Claude Code's and Codex's own memory features are turned off on every node.
- **Search.** Every transcript, imported ones included, is indexed on the server with ctx; its status
  is a Settings section.
- **Generated UI** can act: send a message, start a side chat, open a file.
- **Git** follows Zed's git panel: Changes and History tabs, staging, Fetch, commit, undo.
- **Walkthroughs** are a tab beside the conversation, written by the agent through an MCP tool.
- **Terminal** is a panel on the conversation's right-hand rail.
- **Account switcher** shows only when an agent has two or more accounts, with usage left per account.
- **Queued messages** follow Zed's queue: a collapsible header with Clear all, hover actions per row.

## Order of work

Each release ships, tagged and verified from its artefacts, before the next starts.

1. Memory and history: Mycelium's memory for every agent, importing past conversations, indexing every
   transcript on the server, and searching it.
2. Terminal and git: a terminal panel on a tmux session in the worktree, and Zed's git panel.
3. Projects: forges, projects, worktrees and new projects, then issues as workspaces.
4. Tasks and schedules.
5. Accounts and sessions.
6. Instructions and groups.
7. Conversation polish: Zed's queue, predicted titles, suggested replies, side chats and annotations.
8. Nodes: node pages with resources and ports, and Activity.
9. Moves.
10. Hands off: goals and diff walkthroughs.
11. Chats beside workspaces, local models and generated UI.

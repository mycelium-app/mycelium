# Mycelium design

Mycelium is the ground-up successor to an earlier single-machine orchestrator. That orchestrator runs coding
agents on one host; Mycelium runs them on every machine you own and gives you one place to see and
steer all of them, from web, iOS and Android.

This document records the decisions made so far and the questions still open. Nothing is built yet.

## Shape

```
              clients: web · iOS · Android · desktop · Zed (over ACP)
                                   │  OIDC
                                   ▼
                    ┌──────────────────────────────┐
                    │ control plane  (homelab)     │
                    │ nodes · projects · tasks     │
                    │ memory · usage · assistant   │
                    │ transcript copy · ctx index  │
                    │ git hub mirrors · config     │
                    └──────────────────────────────┘
                       ▲            ▲            ▲
                       │ nodes dial out, one long-lived connection each
                 ┌─────┴───┐   ┌────┴────┐   ┌───┴─────┐
                 │ laptop  │   │ server  │   │ this VM │
                 │ daemon  │   │ daemon  │   │ daemon  │
                 │ keepers │   │ keepers │   │ keepers │
                 └─────────┘   └─────────┘   └─────────┘
```

- **Control plane.** One instance, on the homelab. Owns everything shared: the node registry,
  projects, groups, the task queue, memory, usage, the assistant, the copy of every transcript,
  the ctx index, a bare git mirror per project, and the managed agent config. Clients only ever
  talk to it.
- **Nodes.** A daemon on each machine. It dials out to the control plane and holds the connection,
  so a laptop behind NAT needs no port forwarding. A node owns the sessions running on it and
  their transcripts.
- **Offline nodes.** Shown as offline, with their conversations and worktrees marked offline on
  that machine. Their history stays readable from the control plane's copy; nothing on them can be
  continued or moved until they return.

## Sessions

Two kinds, side by side:

- **ACP sessions**: the agent's [Agent Client Protocol](https://agentclientprotocol.com) adapter
  (`claude-agent-acp`, `codex-acp`). Structured prompts, cancellation, permission requests and tool
  calls, instead of typing into a terminal and scraping it.
- **tmux sessions**: what orchestrator does today. Still the only way to reach a full TUI, and the
  fallback while ACP adapters lag behind the CLIs.

**Keeper.** An ACP agent is a child of whoever spawned it over stdio, so a daemon restart would kill
every running turn. Each ACP session therefore runs under a small keeper process that:

1. starts the agent and owns its stdio;
2. journals every JSON-RPC frame to disk with a sequence number;
3. listens on a Unix socket; a reconnecting daemon asks for everything since the last sequence it
   saw.

The keeper is deliberately tiny and rarely changed, so the daemon updates freely without touching
running sessions. Upgrading an adapter happens at idle: `session/close`, then `session/resume` on
the new version. tmux already plays the keeper role for tmux sessions.

## Logins

ACP reports which login methods an agent supports (`authMethods`), fails a request that needs one
with `auth_required`, and has `logout`. It does not perform the login.

- Subscription logins on a headless node use the **terminal** method: the daemon runs the agent's
  own interactive login in a PTY and streams it to whichever client you are holding. Open the URL
  on the phone, paste the code back. `codex-acp` documents that its ChatGPT login does not work in
  remote projects any other way. Codex has a device-code flow, but only behind
  `codex login --device-auth` (0.163.0; the flag has no help text and plain `codex login` never
  falls back to it): the code is entered at `<issuer>/codex/device` within 15 minutes.
- **One login per node and provider.** Tokens are never copied between nodes: both CLIs treat
  refresh tokens as single use, and two machines sharing one would log each other out. Codex names
  a server error for it, `refresh_token_reused`, and treats it as permanent. Claude Code 2.1.308
  saves the refresh token each refresh returns with a compare-and-swap under a lock file, and on
  `invalid_grant` blanks the stored login. Both guards only work between processes sharing one
  credential file. For Claude this is client code only: the server rejecting an old token has not
  been observed.
- **API keys** are the exception: the control plane stores them once and injects them as
  environment variables.
- A **logins grid**, nodes × providers, shows logged in, logged out or expired, and which account.
  It is updated from `auth_required`, so an expired login shows there rather than as a failed task.
- **Usage is per account, not per node.** Two nodes on one subscription share one limit, and the
  usage stop adds them up.

## Authentication

- **People** sign in to the control plane with OIDC (authorisation code with PKCE) against the
  homelab identity provider.
- **Nodes** join with the OIDC device flow: `mycelium node join https://<control plane>` prints a
  code, you approve it in a browser, and the control plane issues the node its own credential.
  Revoking a node is one button.
- **From Nix**, without the install script: `nix run github:mycelium-app/mycelium -- node
  join https://<control plane>` joins a machine ad hoc. The flake also exports NixOS, nix-darwin and
  home-manager modules (`services.mycelium-node = { enable = true; controlPlane = "…"; }`) that run
  the daemon; `mycelium node join` then approves it once. The node credential lives in the state
  directory, never in the Nix store. An unattended machine takes a single-use join token minted in
  the UI and delivered as a file (`joinTokenFile`, from sops or agenix) instead of a device code.

## Per-node settings

Logins; installed agents and their versions; where projects and worktrees live; how many sessions
it may run at once; labels for routing (`gpu`, `laptop`); enable, drain and revoke. Drain lets
running sessions finish and starts no new ones.

## Projects, worktrees and conversations

| Thing | Lives | Moves by |
|---|---|---|
| Project | On the control plane, cloned on any number of nodes | Cloning it on another node |
| Branch | Anywhere; `main` may be checked out on many nodes at once | Ordinary git |
| Worktree | One directory on exactly one node | Handing over its ownership |
| Conversation | Bound to one worktree | Following its worktree |

**Matching a project across nodes**

1. Match on the normalised remote URL: SSH and HTTPS are the same, a trailing `.git` or `/` is
   ignored. A host alias table covers hosts with two names: `gitlab.example.com:20014` over
   SSH and `gitlab.example.com` over HTTPS are the same GitLab.
2. Use the remote the branch pushes to, normally `origin`.
3. The root commit (`git rev-list --max-parents=0 HEAD`) is a hint only: a fork shares it with its
   upstream. A root-commit match with different remotes is offered as a merge for you to confirm,
   never applied on its own. A repo with no remote is matched the same way, by root commit and
   folder name.

**The hub.** Each node pushes every branch, automatically, into its own namespace on the control
plane's mirror:

```
refs/nodes/<node>/heads/*      every branch, as that node sees it
refs/nodes/<node>/wip/<tree>   the uncommitted-state snapshot taken for a move
```

Nodes never overwrite each other's refs, and the control plane can show each node's view of a
branch:

| State | Meaning | What happens |
|---|---|---|
| In sync | Same commit as the hub | Nothing |
| Behind | Another node pushed on top | Fast-forward on its own, only while the worktree is idle and clean |
| Ahead | Unpushed local commits | Pushed to the hub on its own, never to GitLab |
| Diverged | Both sides have new commits | Shown; you or an agent rebase or merge |

GitLab stays the real remote and only real pushes reach it. Local git config (hooks, identity,
credential helpers) stays per node. Submodules and Git LFS are unsupported for moves at first.

**Moving a conversation or worktree**

1. Wait for idle. A running turn cannot move, and neither can the processes it started (dev
   servers, background shells, containers). The UI offers "move after this turn" and names what
   will be left behind.
2. Snapshot the worktree (index, unstaged and untracked changes) into a commit on its `wip` ref.
3. On the destination: fetch, check out the branch, apply the snapshot, rebuild the index.
4. Copy the agent's files: Claude's transcript and its per-project folder (memory, file
   checkpoints); Codex's session file.
5. Resume with ACP `session/resume`, or `--resume` under tmux.
6. Hand over ownership. The old copy is archived, not deleted, until the move is confirmed.

Several conversations on one worktree move together. "Fork here" is the alternative: a new branch
from the snapshot on the destination, leaving the original where it was. If the destination
already has the branch checked out with its own uncommitted changes, the moved conversation gets a
new worktree there; the two sets of changes meet through ordinary git.

**Paths.** Every node lays projects out the same way under its own root,
`<root>/<namespace>/<project>/worktrees/<name>`. Home directories differ between machines, so a
move rewrites the root prefix inside transcripts, which embed absolute paths throughout.

**Gitignored files do not move.** Build output and dependencies are rebuilt; projects pinned with
mise or Nix rebuild the same anywhere. Secrets such as `.env` come from the control plane's secret
store instead.

## Conversations, history and memory

**Sync.** Claude and Codex write append-only JSONL. The node daemon follows each file and ships new
lines from the last byte offset it sent; when a file shrinks or its start no longer matches, it
ships the whole file again. The control plane keeps the raw files as a per-node copy of
`~/.claude/projects` and `~/.codex/sessions`, and derives one common event format the clients
read. Keeping the raw files means a parser fix can be applied to old history again.

**Indexing with [ctx](https://ctx.rs).** ctx (Apache-2.0, Rust) indexes agent transcripts and
answers search, show and blame. Its own sync, the hosted history beta, does not fit: no OIDC,
manual per-device invitations, normalised records rather than resumable transcripts, and no blame,
graph or semantic search remotely. Instead the control plane runs ctx locally over the copy, one
named source per node and provider:

```sh
export CTX_DATA_ROOT=/var/lib/mycelium/ctx CTX_UPGRADE_AUTO=off CTX_ANALYTICS_ENABLED=false
ctx sources add laptop-claude --provider claude --root /var/lib/mycelium/mirror/laptop/.claude --source-group laptop
ctx sources add laptop-codex  --provider codex  --root /var/lib/mycelium/mirror/laptop/.codex  --source-group laptop
```

The source name, not the path, decides citation identity, so relocating a mirror breaks nothing.
ctx updates itself and sends usage data by default; both are switched off and the release binary
is pinned. It builds with Bazel across roughly a hundred crates, so the release binary is used
rather than a Nix source build.

**Blame needs the node.** `ctx blame` (2.2.8) resolves the repository from the checkout it recorded
at index time, not from the working directory, and requires `git rev-parse --show-toplevel` to
return that path. A bare mirror always fails that check, and moving the checkout makes blame report
it unavailable. So the control plane answers search and show; blame runs on the node that holds
the working tree, with the data root that indexed it.

**Two layers.**

| Layer | Answers | Source |
|---|---|---|
| Record (ctx) | What happened: exact transcripts and tool calls | The transcript copy |
| Memory (`recall`, `remember`) | What we learned: short, confirmed findings | Written by agents, curated by the operator |

Both reach every session on every node through one MCP server on the control plane, proxied by the
node daemon with the node's credential. Memory suggestions work over the whole record rather than
one machine's.

## Managed agent config

Today each machine needs `install.sh` and a `git pull` of agents-md. Instead:

1. The agents-md repo stays the source of truth. Editing it in the UI commits and pushes.
2. The control plane computes what each node should have (global AGENTS.md, skills, plugin list,
   MCP config, settings) and sends it. The node writes `~/.codex/AGENTS.md`, and
   `~/.claude/CLAUDE.md` importing AGENTS.md.
3. A node that was offline catches up on reconnect; its settings page shows the version it holds.
4. Per-node additions live in a separate managed section, so the shared file stays identical.
5. A running Claude session read CLAUDE.md when it started, so a change reaches new sessions only.
   The UI shows which sessions are on an older version.

## Groups

A group (`slop`, `work`, `stoat`, `homelab`) is a namespace. Each project belongs to exactly one,
and the group carries its own agent instructions and its own memories, layered between the global
ones and the project's:

| Layer | Instructions | Memory |
|---|---|---|
| Global | agents-md's AGENTS.md | Findings true everywhere |
| Group | The group's AGENTS.md | Findings true for that group's projects |
| Project | The repo's own AGENTS.md or CLAUDE.md | |

For example, `work` carries "when working with immich dependencies, keep the `@immich/ui` version in
step with the one the `@acme-org/backups-*` packages use". A session in a `work` project sees it;
a session in a `slop` project never does. The org guidelines (`slop/guidelines`) become the `slop`
group's instructions rather than a per-project import.

**Instructions** are delivered through the directory layout, so no repo is touched: projects live
at `<projects root>/<group>/<project>` on each node, and the node writes the group's file beside
them. Claude Code reads `CLAUDE.md` from every parent of its working directory, so
`<projects root>/<group>/CLAUDE.md` is enough. Codex stops at the project root, marked by `.git`
unless `project_root_markers` says otherwise (codex 0.163.0), so it never sees a file above the
repo; for Codex the daemon has to add the group layer when it starts the session. Which Codex
option does that without replacing the base instructions (`model_instructions_file` does) is
unchecked.

**Memory.** Every memory has a scope: global, or one or more groups. Global memories stay the
common case: anything true of the tools, the host or the agents themselves. A scope decides what a
session sees by default, not what it may read:

- `recall` searches global memories and the session's own group, and names how many matches it
  left out in other groups, so the agent knows when to look further.
- `recall(query, groups: [...])` or `groups: all` reaches into other groups. A `slop` session
  bumping an immich dependency can ask `work` what it learned about `@immich/ui`.
- `remember` defaults to the session's group, so a finding about a work dependency stays in `work`.
  Widening a memory to more groups, or to global, is a deliberate step, the operator's from the UI.

If question 3 settles on groups gating permissions, reaching into another group follows the same
rule.

## Cross-project tools

orchestrator's MCP surface, made aware of nodes:

| Tool | Notes |
|---|---|
| `list_projects`, `get_project` | Which nodes hold a clone; worktrees and their owners |
| `create_project` | On a named node, or the default; creates the clone and hub mirror |
| `list_conversations` | Across nodes, with state (working, ready to review, done) and online or offline |
| `add_task`, `claim_task`, `complete_task`, `release_task` | Target a node, a worktree or a conversation |
| `send_to_conversation` | Queued until idle, delivered over ACP or tmux; held while the node is offline |
| `move_conversation`, `move_worktree` | The idle-only move above |
| `ask_question`, `check_answer` | Answerable from any client |
| `recall`, `remember` | Curated memory: global plus the session's group by default, other groups on request |
| `search_history`, `show_history` | ctx over every node's transcripts |

A skill file says when to use which, shipped to every node through the managed config.

## Clients

GPUI, Zed's UI framework, is desktop-first. At zed `03efd49` (2026-10-22) the web backend
(`gpui_web`, wasm32 over wgpu) is in the tree but CI only type-checks it, and threads need nightly
Rust and a cross-origin-isolated page. iOS (`gpui_ios`) merged on 2026-10-14 as experimental, with
no CI. Android has no code: the port PR closed unmerged. Neither backend is on crates.io, whose
`gpui` 0.2.2 is a year old, so using them means a git dependency on a Zed commit. Zed is also heavy
to build under Nix. So Zed is a client rather than the client: a small `mycelium acp` stdio bridge
presents the control plane to Zed as an ACP agent, and Zed lists and resumes Mycelium sessions in
its own agent panel.

The main app is one codebase for web, iOS and Android. Candidates:

| | Expo / React Native | Compose Multiplatform |
|---|---|---|
| Web | DOM, so it can be made accessible | Wasm on a canvas, beta, weak accessibility |
| iOS / Android | Native | Native (iOS stable) |
| Desktop | Electron | JVM |
| Nix | Painful | Already solved in slop/sedge |
| Prior art | [Paseo](https://github.com/getpaseo/paseo)'s chat and status board | slop/sedge, slop/orchestrator-android |

Leaning Compose Multiplatform for every target, for build times: one Kotlin codebase builds web,
iOS, Android and desktop, and its Nix build is already solved in slop/sedge, where Expo's is
painful. The cost lands on the web client, the one used most: Compose renders it to a canvas, so
WCAG 2.2 AA there means fighting the framework rather than getting the DOM's semantics for free.
**Simple first.** The default screen is the conversation list and the conversation. Goals,
schedules, tickets, walkthroughs, moves and logins are each one click from where they apply and
invisible until then: the node in the conversation header, the goal in the composer, the
walkthrough on a diff. The sidebar is ordered by status (needs input, failed, ready to review,
working, done); group and node are metadata on a row and a filter.

Paseo's sidebar (ready to review, working, done) and its chat view are the reference for
the interaction design.

Daemon and control plane: Rust on axum and tokio, with Postgres for the control plane. ctx is Rust;
Nix builds of Rust are cheap.

## Prior art

- **[Paseo](https://github.com/getpaseo/paseo)** (Apache-2.0): a daemon per machine with Electron
  and Expo clients and an end-to-end encrypted relay. No central control plane.
- **[ctx](https://github.com/ctxrs/ctx)**: transcript indexing, search and blame.
- **[OpenChamber](https://openchamber.dev/)**: the features to borrow. A large diff read as an
  ordered walkthrough with the key change marked; a conversation started from a GitHub issue, pull
  request or Linear ticket with the whole thread attached; a goal the agent works towards turn
  after turn until a separate check says it is done or stuck, with a token budget and Pause; and
  scheduled tasks that start a conversation on their own. A schedule either starts a new conversation
  each run or reuses one, so a recurring job can see what its earlier runs did.
- **Zed Delta and DeltaDB**: operation-level history between commits, each edit anchored to the
  conversation that made it; terminal Claude Code sessions shown as threads over ACP; work handed
  to a cloud runner. Private beta with no licence or self-hosting found, so ideas only. `ctx blame`
  covers most of the edit-to-conversation link without a CRDT, on the node that has the checkout.

## Open questions

1. **Fork Paseo or start fresh?** A fork brings the daemon, clients and status board on day one, in
   TypeScript; the control plane and keeper go on top.
2. **Compose for every target, or Compose plus a DOM web client.** Compose is the lean; whether the
   web client's accessibility cost is acceptable is open.
3. **What else a group scopes.** It scopes instructions and memory (see Groups). Whether tasks and
   permissions stop at the group boundary too is open.
4. **Which identity provider** runs in the homelab, and whether its groups drive permissions or the
   control plane keeps its own users.

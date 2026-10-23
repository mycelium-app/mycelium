# Stage 1: run and steer

The build contract for the first release. `DESIGN.md` says why; this file says exactly what, so the
parts can be built in parallel and still fit. Change this file before changing an interface.

## Scope

In: the control plane, the node daemon and its keeper, Claude Code and Codex over ACP, the
conversation list by status, live threads with queued messages, stop, permissions, mode and model,
per-node agent logins, the transcript copy, live workflow progress for Claude Code, rename and
archive, a web client and a desktop client, install by script, Nix or NixOS.

Out until later stages: the git hub, tmux sessions, memory, search, iOS and iPad, schedules, tickets.

## Repository

```
Cargo.toml            workspace
crates/proto          wire types shared by every Rust part (serde)
crates/control        control plane: axum router, Postgres, auth, node hub, client stream
crates/node           node daemon: control-plane link, ACP client, transcripts, logins
crates/keeper         keeper: owns an agent's stdio, journals frames, replays over a socket
crates/cli            the `mycelium` binary
clients/              Compose Multiplatform: shared UI, web (wasmJs), desktop (JVM), androidApp
design/               tokens.css, mycelium.css, tokens.json: the design system's source
site/                 the landing page
nix/                  NixOS, nix-darwin and home-manager modules (flake.nix at the root)
install.sh            curl | sh installer for nodes
Dockerfile            control plane image, web client included
mise.toml             toolchains: rust, java, gradle, node
```

One binary, `mycelium`:

| Command | Does |
|---|---|
| `mycelium control serve` | Runs the control plane |
| `mycelium node join <url>` | Device-code join; writes the node credential |
| `mycelium node run` | Runs the daemon (systemd, launchd or the Nix modules call this) |
| `mycelium node status` | Prints the node's id, control plane, connection and sessions |
| `mycelium keeper --dir <d> -- <cmd> <args…>` | Internal: one per conversation |
| `mycelium version` | Prints the version, set from the release tag at build |

Stack: Rust 2024 edition, tokio, axum, sqlx (Postgres, compile-time unchecked queries are fine),
tokio-tungstenite, clap with `env` fallbacks, tracing. No OpenSSL: rustls everywhere, so the
release binaries are static musl.

## Control plane

### Config (environment)

| Variable | Default | Meaning |
|---|---|---|
| `MYCELIUM_DATABASE_URL` | required | Postgres URL |
| `MYCELIUM_LISTEN` | `0.0.0.0:8080` | HTTP listen address |
| `MYCELIUM_PUBLIC_URL` | required | e.g. `https://mycelium.example.com`, used in links and OIDC redirects |
| `MYCELIUM_DATA_DIR` | `/var/lib/mycelium` | Transcript copy and node journals |
| `MYCELIUM_OIDC_ISSUER` | required unless dev login | OIDC issuer URL |
| `MYCELIUM_OIDC_CLIENT_ID` | required unless dev login | |
| `MYCELIUM_OIDC_CLIENT_SECRET` | empty | Public clients use PKCE only |
| `MYCELIUM_DEV_LOGIN` | empty | An email; signs every browser in as that user. Refused unless `MYCELIUM_PUBLIC_URL` is `http://localhost` or `http://127.0.0.1` |
| `MYCELIUM_ALLOWED_EMAILS` | empty | Comma-separated email addresses allowed to sign in through OIDC (case-insensitive, the provider must not report the address as unverified). Empty allows every account at the identity provider, and the control plane logs a warning at startup. The dev login ignores it |
| `MYCELIUM_WEB_DIR` | `/usr/share/mycelium/web` | The built web client, served at `/` |
| `MYCELIUM_SKIP_MIGRATIONS` | `false` | For gated deploys only |
| `MYCELIUM_RELEASE_URL` | `https://github.com/mycelium-app/mycelium/releases/latest/download/latest` | Plain-text tag of the newest Mycelium release, checked hourly |
| `MYCELIUM_NPM_REGISTRY_URL` | `https://registry.npmjs.org` | Registry the newest `@agentclientprotocol/claude-agent-acp` and `@agentclientprotocol/codex-acp` are read from, checked hourly |
| `MYCELIUM_TRUSTED_PROXIES` | empty | Comma-separated addresses or CIDR ranges of reverse proxies. `X-Forwarded-For` is read only on connections from these, taking the nearest address that is not itself a proxy. Empty trusts no header |

### Boot, health and migrations

- Migrations are embedded (`sqlx::migrate!`) and run at boot inside `pg_advisory_lock(7461001)`.
  Replicas block on the lock, then find nothing to do.
- A failed migration does not exit. The process stays up, `GET /readyz` returns 503 with the
  error, and the error is logged once at error level at startup.
- `GET /healthz` is 200 whenever the process serves HTTP. `GET /readyz` is 200 only when
  migrations are applied and the database answers.
- Migrations are append-only. A shipped migration is never edited.

### Database

```
users            id uuid pk, subject text unique, email text, name text, created_at
web_sessions     id uuid pk, user_id, token_hash bytea unique, expires_at, created_at, user_agent, address, last_used_at null
device_codes     id uuid pk, kind text ('node'|'client'), device_code_hash bytea unique, user_code text unique,
                 label text, hostname text, os text, arch text, ip text, expires_at,
                 approved_by uuid null, approved_at null, result_id uuid null, consumed_at null
nodes            id uuid pk, name text unique, hostname, os, arch, version text, token_hash bytea unique,
                 created_by uuid, created_at, last_seen_at, revoked_at null, draining bool default false
client_tokens    id uuid pk, user_id, label text, hostname, address, token_hash bytea unique, created_at, last_used_at, revoked_at null
agents           node_id, agent text ('claude'|'codex'), version text, available bool, pk(node_id, agent)
logins           id uuid pk, node_id, agent, account text null, method text, state text, usage_left, usage_windows jsonb, usage_read_at
login_usage_history  node_id, agent, account, read_at, windows jsonb: one row per reading, the last 8 days kept
                 ('signed_in'|'signed_out'|'expired'|'unknown'), updated_at, unique(node_id, agent, coalesce(account, ''))
conversations    id uuid pk, owner uuid, node_id, agent, cwd text, title text, status text, acp_session_id text null,
                 mode text null, model text null, account text null, created_at, updated_at, archived_at null
events           id bigserial pk, conversation_id, seq bigint, kind text, payload jsonb, created_at,
                 unique(conversation_id, seq)
queued_prompts   id uuid pk, conversation_id, position int, text text, created_at
transcripts      node_id, path text, bytes bigint, updated_at, pk(node_id, path)
projects         id uuid pk, owner uuid, node_id uuid, name text, path text, forge_id uuid null, repository text null, group_id uuid null, icon text null, avatar_sha256 text null,
                 created_at, unique(node_id, path)
worktrees        id uuid pk, project_id uuid, branch text, path text, issue_number bigint null, created_at, unique(project_id, branch)
tasks            id uuid pk, owner uuid, node_id uuid, agent text, cwd text, project_id uuid null, branch text null, title text, prompt text null,
                 started_at null, conversation_id uuid null, created_at
schedules        id uuid pk, owner uuid, node_id uuid, agent text, cwd text, project_id uuid null, prompt text, cron text,
                 each_run text ('new'|'same'), paused bool, time_zone text ('UTC'), proposed bool, conversation_id null, next_run_at, last_run_at null, last_conversation_id null, created_at
users.time_zone  text null: the person's own zone, set by `PUT /me/time-zone` or by the first schedule created with one
schedule_runs    id uuid pk, schedule_id uuid, ran_at, outcome text ('started'|'skipped'), reason text null, conversation_id uuid null
groups           id uuid pk, owner uuid, name text, created_at, unique(owner, name)
group_folders    id uuid pk, group_id uuid, node_id uuid null (any node), prefix text (absolute, no trailing slash), created_at, unique(group_id, node_id, prefix)
instruction_versions  id text pk (commit SHA with a source, else 40 hex characters), seq bigserial, layer text ('global' or a group id), group_id uuid null,
                 text text, author uuid null, created_at
instruction_source    singleton row: forge_id uuid, repository text, path text
usage_turns      id bigserial pk, conversation_id uuid, owner uuid, node_id uuid, agent text, project_id uuid null, model text null, ended_at, day date (UTC),
                 working_ms bigint null, reported bool, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens bigint, cost_usd double null, cost_total double null, cost_estimated bool (cost_usd came from list prices)
goals            conversation_id uuid pk, text text, token_budget bigint null, time_limit_seconds bigint null, five_hour_limit smallint null, weekly_limit smallint null, state text ('active'|'paused'|'met'|'stuck'|'over_budget'|'limit_reached'), reason text null, check_id uuid null, started_at
walkthroughs     id uuid pk, conversation_id uuid, steps jsonb, created_at (every version kept, the newest is shown)
forges           id uuid pk, owner uuid, kind text ('github'|'gitlab'), base_url text, account text, token text,
                 organisations text[], created_at, unique(owner, base_url, account)
attachments      owner uuid, sha256 text, media_type text, size bigint, created_at, pk(owner, sha256); the bytes live once in `$MYCELIUM_DATA_DIR/attachments/<sha256>`
model_providers  id uuid pk, owner uuid, kind text ('openai_compatible'|'anthropic'), name text, base_url text null, api_key text null,
                 models text[], default_model text null, created_at
```

`conversations.provider_id` (null) marks a chat. A chat is a conversation with `agent` `chat` whose `node_id` is a per-owner hidden node (hostname `control-plane`, always revoked, never listed or reachable), so events, search, titles, stream and usage records need no second code path. A chat's title is also a `search_entries` row at `seq` 0.

Stage 1 is single-tenant: conversations belong to the person who created them, but nodes are shared by
everyone who may sign in. Sign-in output, input and stopping belong to the person who started the sign-in.
Set `MYCELIUM_ALLOWED_EMAILS` unless the identity provider only holds people who may run agents on every node.

Tokens are 32 random bytes, base64url; only their SHA-256 is stored. A node token is sent as
`Authorization: Bearer myc_node_<token>`, a client token as `Bearer myc_client_<token>`, a browser
session as the `myc_session` cookie (HttpOnly, Secure unless localhost, SameSite=Lax).

### Conversation status

One of `working`, `input` (a permission request or question is waiting), `review` (the turn ended
and the person has not opened it since), `failed`, `done` (opened after the turn ended). Archived
is `archived_at` set, independent of status. The sidebar orders: input, failed, review, working,
done.

### HTTP API

JSON over `/api/v1`, errors as `{"error": "<code>", "message": "<GOV.UK-style sentence>"}`.

People:
- `GET /auth/login?next=<path>`: OIDC authorisation code with PKCE, or the dev login.
- `GET /auth/callback`: finishes OIDC, sets the cookie, redirects to `next`. A provider refusal (`?error=`) or a missing code answers the JSON error (`sign_in_refused`, `sign_in_expired`).
- `POST /auth/logout`.
- `GET /api/v1/me` → `{id, email, name}`.

Device codes (one flow for nodes and for desktop clients):
- `POST /api/v1/device` `{kind, label, hostname, os, arch}` → `{device_code, user_code, verification_uri,
  verification_uri_complete, interval, expires_in}`. User codes look like `WDJB-MJHT`, expire in
  10 minutes, single use.
- Both device endpoints are limited per client address (a token bucket in memory: 10 starts a minute, 30 polls a minute, refilling steadily); beyond that they answer 429 `rate_limited` with `Retry-After`.
- `POST /api/v1/device/token` `{device_code}` → `{"error":"authorization_pending"}` (400) until approved,
  then `{kind, id, token}` once. Expired or denied codes return `expired_token` or `access_denied`.
- `GET /api/v1/device/{user_code}` (signed in) → the pending request, for the approval page.
- `POST /api/v1/device/{user_code}/approve` and `/deny` (signed in).
- The web client serves the approval page at `/approve?code=WDJB-MJHT`.

Nodes:
- `GET /api/v1/nodes` → nodes with `online` (linked and past its `hello`), `draining`, agents and logins. A node lists the conversations whose agent session is running on it as `sessions`, republished when one starts or closes. A node and each agent carry `update_available` (the newer version) only when the control plane has seen a newer release; a failed check keeps the last known version.
- `POST /api/v1/nodes/{id}/drain` `{draining}`, `DELETE /api/v1/nodes/{id}` revokes.
- `POST /api/v1/nodes/{id}/watch` → `{latest, history}`: asks the node to sample for the next 30 seconds and returns its newest sample (`null` until one arrives) and the last 200 CPU points `[{at_ms, cpu_percent}]`. A client calls it when a node's page opens and every 15 seconds while the page stays open; the samples follow as `node_resources`. 404 for an unknown node, 409 `node_offline`. A sample is `{at_ms, available, cpu_percent, cores, load: [1, 5, 15 minutes], memory_total_bytes, memory_available_bytes, swap_total_bytes, swap_free_bytes, disks: [{mount, total_bytes, free_bytes}], processes: [{pid, name, command, cpu_percent, memory_bytes, agent, stoppable, conversation_id?, cwd?}], ports: [{port, address, pid?, process?, conversation_id?, cwd?}], docker?: {running, containers: [{name, state}]}}`. `stoppable` is true for a process that belongs to a Mycelium conversation or that the node user owns. `docker` comes from `docker ps --all` run without a shell (so `DOCKER_HOST` and the default socket apply) and is absent when the CLI is missing, fails or takes over 3 seconds, which never fails the sample. `available` is false on a node that cannot read `/proc` (only Linux is read); `cpu_percent` of a process counts one core as 100. `command` is the program name only, never its arguments.
- `POST /api/v1/nodes/{id}/processes/{pid}/stop` → 204: asks the node to send SIGTERM to a process from its latest sample. 404 when the sample does not hold the pid, 409 `process_not_stoppable` when `stoppable` is false, 409 `node_offline`, 409 `stop_failed` with the node's reason (the process has exited, is no longer the named one, or belongs to another user), 504 `stop_timeout` after 10 seconds. The node re-checks the name and the ownership against `/proc` before signalling, so a reused pid is never stopped.
- `POST /api/v1/nodes/{id}/logins` `{agent, method}` → `{login_id}`; starts the agent's own sign-in on
  the node. Output streams as `login_output`; `POST /api/v1/logins/{login_id}/input` `{text}` writes to
  it; `DELETE /api/v1/logins/{login_id}` stops it.

Terminals:
- `POST /api/v1/conversations/{id}/terminal` `{cols, rows}` → `{terminal_id}`; 404 for a conversation that is not
  the caller's, 409 `node_offline`, 400 `size_required` for a zero size. Output streams as `terminal_output`.
  `POST /api/v1/terminals/{terminal_id}/input` `{data}` (base64 bytes), `POST /api/v1/terminals/{terminal_id}/resize`
  `{cols, rows}` and `DELETE /api/v1/terminals/{terminal_id}` (detaches; the tmux session keeps running).

Sessions:
- `GET /api/v1/sessions` → the caller's browser sessions and client tokens `[{id, kind: "browser"|"client", label, hostname, address, last_used_at, current}]`, the current one first. `DELETE /api/v1/sessions/{id}` ends one at once (404 for another person's); ending the current one is 409 `current_session` unless `?confirm_current=true`. Nodes carry `address` in `GET /api/v1/nodes`.

Conversations:
- `GET /api/v1/conversations?archived=false`.
- `POST /api/v1/conversations` `{node_id, agent, account?, cwd, title?, prompt?}` → the conversation (409 `account_not_signed_in` unless the node reports that account signed in for the agent); the node
  starts the ACP session, and `prompt`, if given, is the first message.
- `GET /api/v1/conversations/{id}`, `PATCH` `{title?, archived?, account?, use_default_account?}` (409 `account_locked` once the conversation has a session), `POST /{id}/opened` marks review as done, `DELETE` (204) removes an archived conversation for good (404 for another person's, 409 `not_archived` unless archived). Its events, queued messages, search entries, goal, walkthroughs and usage records go with it, so Activity totals no longer count its turns; its side chats are deleted too; attachments no message of the owner still carries are removed, and shared ones stay. The node is told to close its session, and `conversation_removed` is streamed for the conversation and each side chat.
- `GET /api/v1/conversations/{id}/events?after=<seq>&limit=500`.
- `POST /{id}/retry` (204, owner only) starts a failed conversation's session again: status returns to `done`, the node is asked to `session/load` the conversation's ACP session (a new session in the same folder when it never had one) and anything queued is delivered. 409 `node_offline` while the node is offline, 409 `node_removed` when it was revoked. A conversation that is not failed is left alone. `POST /{id}/prompt` on a failed conversation retries first and then sends.
- A failure whose message is a usage limit (Claude Code's "You've hit your session limit · resets 4:30pm (Europe/London)", the weekly and Opus variants, the older `usage limit reached|<epoch>`, Codex's "hit your usage limit") is recorded on its `error` event as `usage_limit: {window: five_hour|seven_day|seven_day_opus|usage, resets_at?}`. The time is read in the zone the message names (the person's zone when it names none), as its next occurrence; a message without one takes the earliest future `resets_at` among the login's usage windows.
- `POST /{id}/resume` `{at?}` (204, owner only) schedules a retry of a failed conversation: `at` defaults to the limit's `resets_at` plus a minute and is never earlier than now. 409 `not_failed`, 409 `no_reset_time` when there is neither. `DELETE /{id}/resume` (204) cancels it. The conversation carries `resume_at` while one is pending; any retry clears it. Due resumes are claimed with `update ... skip locked`, so two control planes never both fire one: the session is retried and what was queued is delivered, else "Continue from where you stopped." is sent as the person's message. A resume that cannot start (node offline) is put back a minute later. `limit_resumes` counts automatic resumes since the last finished turn.
- `GET`/`PUT /api/v1/usage-limit` `{on_usage_limit: "ask"|"resume"}` (default `ask`). With `resume`, a usage limit with a known reset schedules its own resume at once, up to 3 times in a row without a finished turn, and a goal on that conversation stays active instead of stopping. Side chats are never resumed automatically.
- `POST /{id}/prompt` `{text, attachments?}`: sent now if idle, otherwise appended to the queue. `attachments` are the objects `POST /{id}/attachments` returned (`{sha256, name, media_type, size}`); `text` may be empty when there are attachments (400 `attachment_missing` for one never uploaded to this owner, 400 `too_many_attachments` over 10, 413 `attachments_too_large` over 40 MB together). The `user_message` event payload carries `attachments`, and so does each queued prompt.
- `POST /{id}/attachments?name=<file name>` with the raw bytes as the body and the media type as `Content-Type` → `{sha256, name, media_type, size}` (201). Stored once per SHA-256 however often it is uploaded; 413 `attachment_too_large` over 20 MB, 400 `attachment_empty`.
- `GET /api/v1/attachments/{sha256}`: the bytes, to the person who uploaded them only (404 otherwise). Images are served inline with their media type, everything else as `application/octet-stream` with `Content-Disposition: attachment`; `Cache-Control: private, max-age=31536000, immutable`, an `ETag` of the hash, `nosniff` and `Content-Security-Policy: sandbox`.
- `GET /{id}/queue`, `PATCH /{id}/queue/{qid}` `{text}`, `DELETE /{id}/queue/{qid}`, `POST /{id}/queue/{qid}/send-now`
  (cancels the current turn, then sends this one).
- `POST /{id}/cancel`.
- `POST /{id}/permission` `{request_id, option_id}`.
- `POST /{id}/config` `{mode?, model?, config?: {id: value}}` → ACP `session/set_mode`, `session/set_model`
  or `session/set_config_option`, whichever the agent advertises.

Activity:
- `GET /api/v1/activity?range=today|7d|30d&node_id=` → `{range, totals, days, hours, conversations, nodes, agents, projects, models, unreported_agents}` for the caller's own conversations, `range` defaulting to `7d` (400 `range_invalid` otherwise) and `node_id` narrowing to one node. Every row is `{key, name, turns, reported_turns, working_ms, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost_usd}`; `days` has one row per UTC day of the range and `hours` one row per UTC hour (`key` is `YYYY-MM-DDTHH`) from the first day of the range to the current hour, empty ones included, `cost_usd` is null until an agent reports a cost, and the breakdowns hold the top 50 by tokens. A turn is recorded when its `turn_end` arrives: tokens from the turn's `usage`, cost as the change in the agent's running total from its latest `usage_update` (`cost.amount`), the model from that update's `_meta["_claude/model"]` else the conversation's, and time working from the last user message to the turn end. A turn with neither tokens nor cost is counted as a turn only (`reported_turns` does not include it), and `unreported_agents` names the agents in the range with no reported turn. Every row also carries `cost_estimated`, true when any turn in it was priced from list prices rather than reported by an agent. Imported conversations record one row per turn from the transcript (see History import and search).
- `GET /api/v1/activity/conversations` → `[{key: conversation id, turns, reported_turns, input_tokens, ..., cost_usd, cost_estimated}]`, all time, one row per conversation of the caller that has a turn; History reads its Turns, Tokens and Cost columns from it.

History:
- `GET /api/v1/imports` → `[{node_id, agent, found, imported}]` per node and agent; `POST /api/v1/imports`
  `{node_id, agent}` → 202 `{id, node_id, agent, state, total, done, imported, skipped, waiting, failed: [{path, reason}]}`
  at once, the import running as a background job (one running job per node and agent: a second start returns it).
  `GET /api/v1/imports/jobs` → the latest job per node and agent. `GET /api/v1/indexing` → `{conversations, entries, nodes: [{node_id, transcripts, imported, conversations}]}`.
- `GET /api/v1/search?q=&limit=20` → `[{conversation_id, title, node_id, updated_at, snippet}]`; an empty `q` is 400.

Live:
- `GET /api/v1/stream` (WebSocket, cookie or client token). The server sends one JSON object per
  message: `conversation` (the row), `event` (`{conversation_id, seq, kind, payload}`), `queue`,
  `conversation_removed` (`{conversation_id}`), `node`, `node_removed` (`{node_id}`, sent when a node is revoked), `node_resources` (`{node_id, sample}`), `login_output` (`{login_id, data}`), `terminal_output` (`{terminal_id, data}`, base64 bytes), `terminal_exited` (`{terminal_id}`, also sent when the terminal is stopped or its node disconnects or is revoked), `login_finished` (`{login_id, ok}`; `ok` is false when the sign-in is stopped, or its node disconnects or is revoked), `device` (`{user_code, kind: "node"|"client", label, hostname, state: "pending"|"approved"|"denied"|"expired"|"consumed", result_id: null|uuid}`, sent to everyone signed in whenever a device code is created, approved, denied, expires or is exchanged; `result_id` is the node or client token id once consumed). A node is published with `node` (`online` false) as soon as its credential is issued at token exchange, before the daemon first connects. Clients drop the node on `node_removed`, and reconnect and refetch. A request that carries an `Origin` header must match `MYCELIUM_PUBLIC_URL`'s origin, otherwise 403 `origin_not_allowed`; clients that send none (desktop, tests) are accepted.

Memory:
- `GET /api/v1/memories?query=` (newest first, or full-text matches when `query` is given), `PATCH /memories/{id}` `{text}`, `DELETE /memories/{id}`. Any signed-in user manages them.
- `POST /api/v1/mcp` (node token): MCP Streamable HTTP with plain JSON replies. Tools `recall {query, limit?}`, `remember {text}`, `propose_schedule {prompt, preset? ("weekdays at 20:00", "daily at 09:00", "every 15 minutes", "every 2 hours"), cron?, each_run? (`same` by default, in the calling conversation, or `new`), time_zone?}`, which stores a proposed schedule on the calling conversation's node, agent and folder and answers an error result saying what to fix for an unknown preset, a cron that never runs or a bad zone, and `walkthrough {steps: [{title, key?, section?, icon? (`logic|tests|docs|config|ui|data|other`, otherwise dropped), explanation?, files?: [{path, diff | hunks}]}]}`, which stores a diff walkthrough for the calling conversation (`x-mycelium-conversation`), replacing the shown one and keeping the older ones, and publishes it as `walkthrough` on the stream. A file's `hunks` are joined with newlines into its `diff`. A call without steps, a step without a title, or a file without a path or diff stores nothing and answers an error result saying what to fix. `get_instructions {layer?}` answers `Layer: <name>`, `Revision: <version id, or none>` and the layer's text; `propose_instructions_edit {text, base_revision, reason, layer?}` stores a proposal with the complete new text. `layer` is `global` or a group name and defaults to the calling conversation's group, else global. A `base_revision` that is not the layer's current revision, an empty text or reason, a text equal to the current one or an unknown layer stores nothing and answers an error result saying what to fix. A call is one waiting proposal per conversation and layer: a newer one supersedes the older.
- `GET /api/v1/conversations/{id}/walkthrough` → the newest walkthrough `{id, conversation_id, steps: [{title, key, section?, icon?, explanation, files: [{path, diff}]}], created_at}` or `null`.
- `GET /api/v1/walkthroughs/{id}/reads` → the step numbers (from 0) the person has read; `PUT /api/v1/walkthroughs/{id}/reads/{step}` (204) marks one, repeating it changes nothing (400 `no_such_step` past the last, 404 for another person's walkthrough). Stored per person in `walkthrough_reads`.

Forges:
- `GET /api/v1/forges` → `[{id, kind: "github"|"gitlab", base_url, account, token_hint, organisations, created_at}]`; `POST /api/v1/forges` `{kind, base_url?, token}` → the forge (201). `base_url` defaults to `https://github.com` or `https://gitlab.com`. The token is checked against the forge's user endpoint first: 400 `forge_token_rejected`, `forge_unreachable`, `forge_failed`, `base_url_invalid` or `token_required`. Connecting an account again replaces its token. `DELETE /api/v1/forges/{id}` disconnects. Forges belong to the person who connected them.
- `GET /api/v1/repositories?forge=&organisation=&q=` → `{repositories: [{forge_id, owner, name, full_name, description, private, default_branch}], organisations: [{forge_id, name}], failures: [{forge_id, message}]}`: the repositories of every connected forge (up to 500 each), fetched live; a forge that fails is reported in `failures` and does not hide the others.

Projects:
- `GET /api/v1/projects` → `[{id, node_id, name, path, forge_id, repository, created_at}]`, the caller's own. `POST /api/v1/projects` `{node_id, forge_id, repository: "owner/name"}` clones the repository on the node into `<projects_root>/projects/<owner>/<name>` with the forge's token; `{node_id, path}` registers an existing folder inside the node's projects root. 201 with the project; 409 `node_offline`, `git_failed` (git's own message) or `project_exists`; 400 `repository_invalid` or `project_source_required`.
- `POST /api/v1/conversations` also takes `project_id` (instead of `cwd`): the conversation runs in the project's folder; the node must be the project's (400 `project_not_on_node`).
- `POST /api/v1/conversations` with `project_id` and `branch` first has the node create a git worktree on that new branch (`git worktree add -b`), in `<project>-worktrees/<branch with / as ->` beside the project, and the conversation runs there (cwd is the worktree). 400 `branch_required` or `project_required`; 409 `git_failed` with git's message (branch exists, not a valid name) and no conversation is created.
- `GET|PUT|PATCH|DELETE /api/v1/conversations/{id}/goal`: a conversation's goal, `{conversation_id, text, token_budget?, time_limit_seconds?, five_hour_limit?, weekly_limit?, tokens_used, turns, started_at, five_hour_used?, weekly_used?, state, reason?}` or `null`. `PUT {text, token_budget?, time_limit_seconds?, five_hour_limit?, weekly_limit?}` sets it; only the text is required, each limit is optional (400 `text_required`, `budget_invalid`, `time_limit_invalid`, `five_hour_limit_invalid` and `weekly_limit_invalid` for a percent outside 1 to 100, `goal_not_allowed` on a side chat). `five_hour_used` and `weekly_used` are the percent used of the conversation's account windows (`five_hour`, `seven_day`), absent while unknown and starts it over; on an idle conversation it sends `Work towards this goal: <text>`. `PATCH {paused}` pauses or resumes (409 `goal_ended` once `met`, `stuck` or `over_budget`); resuming an idle conversation runs a check at once. `DELETE` clears it (204). `state` is `active`, `paused`, `met`, `stuck` or `over_budget`; `reason` is the check's reason, or why it ended or paused. `tokens_used` sums input, output and cache tokens from the usage records of the conversation and its checks since the goal was set, and `turns` counts the conversation's turns. Streamed as `goal` and `goal_cleared`. While a goal is `active`, each turn that ends starts a check: a side chat (`side_kind` `check`, never listed, archived once it answers) on the same node, folder, agent and account, asked to answer only `{"verdict": "met"|"continue"|"stuck", "reason": "..."}`; the first JSON object in its answer is read, and anything else counts as `stuck`. `met` and `stuck` end the goal. `continue` first checks every limit set: it ends the goal as `over_budget` when the token budget is used up, or as `limit_reached` when the time limit since `started_at` has passed or the account's 5-hour or weekly window has reached its percent (an unknown window never stops it); otherwise it sends `Continue towards the goal: <text>` and the reason. A message from the person is delivered as usual and does not pause the goal; a check that answers while the person's turn is running is dropped and the check after that turn decides. Stopping a turn pauses an active goal; a failed agent or check session ends it as `stuck`.
- `GET /api/v1/conversations/{id}/move-plan?node_id=` → `{movable, reason?, branch?, changed_files, project_on_target, clones_project, agent_session, stays_ports: [port], stays_processes: [name]}`: what a move to that node would carry and what stays. Not movable (with `reason`) when the folder's project has no forge repository or the source node cannot name a branch (detached HEAD, no commits). `project_on_target` is whether the caller has a project with the same forge and repository on the target; `clones_project` says a move would clone it first. Ports and processes are the source node's latest resource sample whose `conversation_id` or `cwd` is this conversation's folder (the agent's own processes excluded); the call asks the node to sample and waits up to 3 seconds when it holds none. 409 `same_node`, `node_draining`, `node_offline`, `agent_unavailable` or `account_not_signed_in`; 404 for an unknown node.
- `POST /api/v1/conversations/{id}/move` `{node_id, fork?: false}` → 202 `{conversation_id, node_id, fork, state, message?, result_id?}`, then `GET` the same address for the latest progress (`null` before any move; held in the control plane's memory) and `move` on the stream. A busy conversation (`working` or `input`) answers `waiting` and moves as soon as its turn ends; an idle one answers `moving`. `state` is `waiting`, `moving`, `done` (`result_id` is the conversation that now runs on the target: the same one for a move, the new one for a fork), `failed` (`message` says what failed, that nothing changed and how to retry) or `cancelled`. `DELETE` the same address cancels a `waiting` move (204; 409 `move_not_waiting` otherwise). 409 `move_in_progress` while one is waiting or moving. While moving, queued messages are held and sent afterwards. A move: the source node snapshots (`git` `snapshot`: branch, head commit, a binary patch of the working tree including untracked files taken through a scratch index so the folder and its staging are untouched, and a bundle of commits no remote has), the control plane clones the project on the target if the caller has none there (`projects/<owner>/<name>`, registered as a project), the target node `apply`s it (refuses when its folder has uncommitted changes or its branch holds commits the snapshot lacks; checks out the branch at the head commit, applies the patch, and puts its previous branch back if the patch fails), the control plane copies the agent's transcript from its mirror to the target (`restore_transcript` chunks of 192 KB, into `.claude/projects/<target folder with every non-alphanumeric as ->/<session>.jsonl` for Claude Code, the same path for Codex), tells the target where the journal numbering continues (`continue_sequence`), updates the conversation's node, folder and project, and sends `start_session` with `resume`. When the target reports the session started the source's session is closed; when it reports failure the conversation row and events are put back as they were and the target's session is closed. A fork does the same but inserts a new conversation (hidden until its session starts, titled `<title> (fork)`) and starts it with `fork` set for Claude Code. A snapshot over 8 MB is refused. A worktree folder moves into the target project's own folder; a folder inside the project keeps its relative path.
- `GET /api/v1/tasks` → the caller's backlog tasks that are not started yet, `[{id, node_id, agent, cwd, project_id, branch, title, prompt, created_at}]`. `POST /api/v1/tasks` `{node_id, agent, title, prompt?, project_id?, branch?, cwd?}` adds one (201; 400 `title_required`, `cwd_required`, `project_required` or `project_not_on_node`). `POST /api/v1/tasks/{id}/start` starts a conversation the way `POST /conversations` does, titled with the task, whose prompt is the task's prompt or else its title (201 with the conversation; the task leaves the backlog, and stays there when the start is refused, for example `node_offline`). `DELETE /api/v1/tasks/{id}` removes an unstarted task.
- `GET /api/v1/schedules` → `[{id, node_id, agent, cwd, project_id, prompt, cron, each_run, paused, time_zone, proposed, conversation_id, next_run_at, last_run_at, last_conversation_id, created_at}]`. `POST /api/v1/schedules` `{node_id, agent, prompt, cron, each_run: "new"|"same", project_id?, cwd?, time_zone?}` (201; 400 `prompt_required`, `cron_invalid`, `cron_never_runs`, `cwd_required`, `project_not_on_node`, `time_zone_invalid`). `cron` is five fields, evaluated in `time_zone` (an IANA name; without one, the person's own, else UTC). `PUT /api/v1/me/time-zone` `{time_zone}` (204; 400 `time_zone_invalid`) stores the person's zone. A proposed schedule (`proposed` true, from the `propose_schedule` tool) never runs: `POST /api/v1/schedules/{id}/confirm` `{cron?, each_run?, time_zone?}` accepts it with optional edits and returns it (409 `schedule_not_proposed` when already accepted); `POST /api/v1/schedules/{id}/decline` (204) deletes it and queues a message on its conversation saying it was declined; run and pause answer 409 `schedule_proposed`. Both publish: `schedule` (the schedule) and `schedule_removed` (`{schedule_id}`). `PATCH /api/v1/schedules/{id}` `{paused}` pauses it, or resumes it with the next run counted from now. `POST /api/v1/schedules/{id}/run` runs it now without moving `next_run_at` (201 with the run). `DELETE /api/v1/schedules/{id}`. `GET /api/v1/schedule-runs` → the caller's 50 latest runs, `[{id, schedule_id, ran_at, outcome, reason, conversation_id}]`.
- `GET /api/v1/worktrees` → `[{id, project_id, branch, path, issue_number, created_at}]` for the caller's projects.

Groups and instructions:
- `GET /api/v1/groups` → the caller's groups `[{id, name, folders: [{id, node_id?, prefix}], created_at}]`. `POST /api/v1/groups` `{name}` (201; 400 `name_required` or `name_invalid`, which allows up to 40 letters, numbers, dots, dashes and underscores; 409 `group_exists`). `PATCH /api/v1/groups/{id}` `{name}` renames. `DELETE /api/v1/groups/{id}` leaves its projects ungrouped; memories stored in its scope stay in the list. `POST /api/v1/groups/{id}/folders` `{prefix, node_id?}` makes the group claim a folder, on one node or on any (201 `{id, node_id, prefix}`; 400 `folder_invalid` unless it is a full path below the root with no `.` or `..`; 404 for an unknown node; 409 `folder_claimed` when a group already claims it on that node). `DELETE /api/v1/groups/{id}/folders/{folder_id}` removes the claim (204).
- `PATCH /api/v1/projects/{id}` `{group_id}` puts a project in one of the caller's groups, or takes it out when `group_id` is null (404 for another person's project or group). Projects carry `group_id`. The same route sets the project's icon: `{icon}` names one from the client's set (lowercase letters, digits and dashes, up to 40; 400 `icon_invalid`) and `{clear_icon: true}` removes it; a request with either leaves the group alone, and either one removes the uploaded logo. `POST /api/v1/projects/{id}/avatar` takes the image as the body with its `Content-Type` (PNG, JPEG, WebP or SVG, at most 256 KB; 400 `avatar_not_image`, `avatar_empty`; 413 `avatar_too_large`), stores it as an attachment, sets `avatar_sha256` (and clears `icon`) and answers the project; `GET /api/v1/attachments/{sha256}` serves it to its owner.
- `GET /api/v1/instructions` → `{source: {forge_id, repository, path} | null, revision, layers: [{layer, name, path, text, version: {id, created_at} | null}]}`. `layer` is `global` or a group id; `revision` is the id of the newest version of any layer. `path` is the layer's file in the source repository, null without a source.
- `PUT /api/v1/instructions/{layer}` `{text}` saves the layer as a new version and pushes to every node; saving the text the layer already holds changes nothing. Without a source the version id is a 40-character hash held in Postgres only. With a source the save is a commit through the forge's contents API (GitHub `PUT /repos/{r}/contents/{path}`, GitLab `/projects/{id}/repository/files/{path}`) and the version id is the commit SHA; a file that changed on the forge since it was read answers 409 `forge_file_changed` and nothing is saved.
- `POST /api/v1/instructions/pull` reads every layer's file from the source and records a version, with the commit SHA as its id, for each that differs from the latest (409 `no_source`). A file the repository does not have leaves the layer as it was.
- `GET /api/v1/instruction-proposals` → the caller's proposed edits, waiting ones first then the newest decided, at most 100: `[{id, conversation_id, conversation_title, layer, layer_name, base_revision, base_text, text, reason, state: "proposed"|"approved"|"declined"|"superseded", note, out_of_date, created_at, decided_at}]`. `base_revision` is the layer's version id when the agent read it, empty for a layer with no version; `out_of_date` is true while the proposal waits and the layer's newest version is not its base (any save or pull of the layer changes it, and republishes the waiting proposals of that layer).
- `POST /api/v1/instruction-proposals/{id}/approve` saves `text` to the layer exactly as `PUT /instructions/{layer}` does (same version, same push to every node, same commit through the source when one is connected) and returns the proposal (409 `proposal_not_proposed` once decided, 409 `proposal_out_of_date`, and the save's own errors such as `forge_file_changed` leave it waiting). `POST /api/v1/instruction-proposals/{id}/decline` `{note?, redo?}` marks it declined and queues a message on its conversation: that the edit was declined with the note, or with `redo` that the instructions changed and the agent should read them again and propose again. Both publish `instruction_proposal` (the proposal); a new proposal also publishes `superseded` for the earlier waiting ones from the same conversation and layer.
- `PUT /api/v1/instruction-source` `{forge_id, repository: "owner/name", path?}` sets the source (path defaults to `AGENTS.md`; 400 `repository_invalid` or `path_invalid`); `DELETE /api/v1/instruction-source` clears it and keeps the versions. The source is one for the whole control plane and uses the forge of the person who set it.
- The global layer is the file at `path`. A group's layer is its sibling with the group name before the extension: `AGENTS.md` and group `slop` give `AGENTS.slop.md` (`docs/AGENTS.md` gives `docs/AGENTS.slop.md`). Renaming a group does not move the file already committed.

Issues:
- `GET /api/v1/issues?q=` → `{issues: [{forge_id, repository, number, title, url, body}], failures: [{forge_id, message}]}`: the open issues of every forge repository the caller has set up as a project (a repository set up on several nodes is fetched once). `q` is passed to the forge in its own syntax: GitHub's issue search (added to `repo:<name> is:issue is:open`), GitLab's `search` parameter. A forge that fails is reported in `failures`.
- `POST /api/v1/projects/{id}/issues/{number}/workspace` `{agent}` → the conversation (201). The control plane fetches the issue, has the node create a worktree on `issue/<number>-<title slug>` (reused when the issue already has one) and starts a conversation there, titled `#<number> <title>`, whose first prompt is `Work on issue #<number>: <title>`, the link and the body. 404 for an issue the forge does not have, 409 `project_without_forge` for a project that was only a folder.

Node link:
- `GET /api/v1/node/connect` (WebSocket, node token). Frames below.

### Activity estimates

Energy, water and forest on the Activity page are estimates the client derives from the tokens of the range, never measured, and the page says so in a "How this is estimated" note. The factors and where they come from:

- Energy: 0.6 Wh per 1,000 output tokens and 0.2 Wh per 1,000 input tokens, counting cache reads and writes as input. Epoch AI estimates about 0.3 Wh for a typical ChatGPT query of a few hundred output tokens and about 2.5 Wh for a 10,000 token prompt (https://epoch.ai/gradient-updates/how-much-energy-does-chatgpt-use, 2025). It is a third-party estimate for one provider, applied to every agent, and cached tokens are probably cheaper than fresh input, so it errs high.
- Water: 1.1 mL per Wh, the ratio of the 0.26 mL and 0.24 Wh Google reports for a median Gemini text prompt, which includes the data centre's cooling water (https://cloud.google.com/blog/products/infrastructure/measuring-the-environmental-impact-of-ai-inference, 2025). It does not count water used to generate the electricity.
- CO2: 445 g per kWh, the IEA's global average for electricity generation in 2024 (https://iea.org/reports/electricity-2025/emissions). Google's own market-based figure is far lower; a global grid average is the one a reader can check.
- Forest: a tree-day is 60 g of CO2, from the widely quoted 48 pounds (about 22 kg) a mature tree absorbs a year, attributed to the Arbor Day Foundation. The figure is repeated more often than it is sourced.

### Memory

Agents share one memory through two MCP tools served by the control plane at `/api/v1/mcp`, so a
finding stored in one conversation is recalled in any other, on any node. `remember` stores a global
memory attributed to `<node>/<conversation title>`; `recall` runs a Postgres full-text search over
global memories. The node passes the server to the agent at `session/new`, `session/load` and
`session/resume` when the agent advertises `mcpCapabilities.http`, with the node token and the
conversation id as headers, and starts Claude with `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` so its own
memory is off. Settings has a Memories page to search, edit and delete.

A conversation's group is its project's group if the project has one, else the group whose claimed folder is the
longest prefix of the conversation's `cwd` (a claim on a specific node beats one on any node; `/projects` matches
`/projects/app` but not `/projects2`). It is resolved when read, by the SQL function `conversation_group`, so
imported conversations are grouped without a re-import and a changed claim applies at once. Conversations carry
`group_id`. Memory scopes are `global` and `group:<group id>`. The node's `x-mycelium-conversation` header names the
conversation.

- `recall` searches `global` plus the conversation's group and, when other groups hold matches, appends how many it
  left out and in which groups. `groups` takes group names or `"all"` to search those too; an unknown name is an error
  result.
- `remember` stores into the conversation's group, or globally when it has none. `scope: "global"` stores globally;
  `scope: "group"` is refused, with a plain message, when the conversation has no group. The reply names where the
  memory went.

### Instructions

One global instructions file plus one layer per group, each saved as versions. The control plane
sends `instructions {revision, global?}` to a node whenever any layer is saved or pulled and after
each `hello`. `revision` is the newest version id; `global` is the global layer's text. The node
writes the global text, behind the line `<!-- Managed by Mycelium. Changes made here are overwritten. -->`,
to the file each installed agent reads for user-level instructions, in the agent's default home and
in the home of every account on the node: `<home>/.claude/CLAUDE.md` (or `$CLAUDE_CONFIG_DIR/CLAUDE.md`)
for Claude Code, `<home>/.codex/AGENTS.md` (or `$CODEX_HOME/AGENTS.md`) for Codex. A file that exists
without that first line is never touched: the node lists it in `instructions_unmanaged` and delivers nothing there.
The node answers `instructions_held {revision, unmanaged: [label]}`; `GET /nodes` carries `instructions_revision`,
`instructions_unmanaged` and `sessions_on_older_instructions`, the running sessions whose
conversation started on a revision other than the newest.

A group's layer reaches a conversation in the group (by project or by folder) without touching the project's git
tree: `start_session` carries `instructions` (the layer's text) and the node appends it to the system
prompt through `_meta.systemPrompt.append` on `session/new`, `session/load` and `session/resume`, which
`claude-agent-acp` reads. For Codex the node starts one agent process per conversation and sets `CODEX_CONFIG`
in its environment to JSON with `developer_instructions` (codex-acp merges it into the session config; it adds a
developer message and keeps the base instructions). A `CODEX_CONFIG` the node already has is parsed and kept, and an
existing `developer_instructions` is followed by ours. The control plane records the revision a
conversation started with when it sends `start_session`.

`instructions` in `start_session` is the group layer followed by the enabled Mycelium extensions, joined by blank
lines. Extensions are built-in additions with a per-user switch, `GET`/`PUT /api/v1/extensions`
`{generated_ui, rich_markdown, mycelium_tools}`, all on by default (table `user_settings`). `generated_ui` adds the `mycelium-ui`
component schema, the text chats get plus the workspace-only `open_file {path}` action. `rich_markdown` adds a note
naming the Markdown Mycelium renders and that raw HTML is not rendered; chats get that note too. `mycelium_tools` adds a note that the agent runs inside Mycelium with its MCP server, one line per tool generated from the server's own tool list (`mcp__mycelium__<name>`), and that the agent's own memory is off on purpose; chats do not get it, as they find their tools with `tool_search`. A switch applies to
sessions started after it changes. Settings, Instructions has a "Mycelium extensions" section below the layers.

### Forges

Settings has a Forges section to connect GitHub and GitLab, including self-hosted ones, with a personal access token. Tokens live
in `forges.token` in Postgres, in plain text, and no API response carries one: a forge is returned with `token_hint` (the last
four characters) and the `account` the forge's user endpoint named when the token was saved. The control plane makes every forge
call itself (GitHub `/user`, `/user/orgs`, `/user/repos`; GitLab `/user`, `/groups`, `/projects`; GitHub Enterprise is
`<base_url>/api/v3`, GitLab `<base_url>/api/v4`). Organisations always start with the account itself.

### Chats and model providers

- `GET /api/v1/model-providers` → `[{id, kind: "openai_compatible"|"anthropic", name, base_url, key_hint, models, default_model, created_at}]`; `POST` `{kind, name, base_url?, api_key?}` → the provider (201). OpenAI-compatible needs `base_url` (Ollama serves one at `http://<host>:11434/v1`), the key is optional; Anthropic needs `api_key`, and `base_url` defaults to `https://api.anthropic.com`. The models are fetched first (`GET {base_url}/models`, Anthropic `GET /v1/models`), so a bad address or key is refused and nothing is stored: 400 `provider_key_rejected`, `provider_unreachable`, `provider_failed`, `base_url_invalid`, `base_url_required`, `api_key_required`, `name_required`. `default_model` is the first model for OpenAI-compatible, and for Anthropic the first listed of `claude-sonnet-5-5`, `claude-opus-5-5`, `claude-haiku-5-5`. Keys are stored server-side and never returned, only `key_hint`. `POST /api/v1/model-providers/{id}/refresh` fetches the models again; `DELETE` removes it and its chats keep their history but cannot continue (409 `provider_removed`).
- `GET /api/v1/chat-tool-servers` → `[{id, name, url, token_hint, enabled, always_allow, tools: [{name, description}], created_at}]`; `POST` `{name, url, bearer_token?}` → the server (201): remote MCP servers a chat may use. The server is asked for its tools first (`tools/list`), so a bad address or token is refused with nothing stored: 400 `tool_server_failed` (its message says what to fix), `url_invalid`, `name_required`, `name_taken` (also 409 for a name another server of yours has, and `mycelium`, which is the name of Mycelium's own tools). `PATCH /{id}` `{enabled?, always_allow?}` switches it off or trusts all its tools without asking; `POST /{id}/refresh` lists its tools again; `DELETE` removes it. The token is stored server-side, never returned, only `token_hint`. The control plane speaks MCP over Streamable HTTP, revision 2026-07-28 first (every request its own POST with `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` and the per-request `_meta`), and falls back to the `initialize` handshake of 2025-11-25 (with `Mcp-Session-Id`) when the server answers a `400`, `404` or `405` that is not a modern JSON-RPC error. Responses may be JSON or an event stream. stdio servers on nodes are not supported.
- `POST /api/v1/chats` `{provider_id, model?, prompt?, parent_id?}` → the chat as a conversation (201), `agent` `chat`, `provider_id` set, `model` defaulting to the provider's `default_model` (400 `model_required`). A `prompt` is sent at once. `parent_id` makes it a side chat of another chat.
- Chats use the conversation endpoints: `prompt` sends a message (409 `chat_busy` while it is replying), `cancel` stops the reply and ends the turn with `stop_reason` `cancelled`, `events`, `PATCH` for title and archive, and `side` starts a linked chat on the same provider and model. A reply streams as `acp_update` events with `agent_message_chunk` content, one per token group the provider sends, then `turn_end` (`end_turn`, `cancelled` or `error`); a provider failure adds an `error` event first. Replies run in the control plane; the whole conversation so far is sent each time, with a system prompt that describes the `mycelium-ui` block. The title is predicted from the first message like a workspace's. Images in a chat go to the provider as OpenAI-compatible `image_url` data URLs or Anthropic `image` blocks, text files are inlined in the message, and earlier turns keep theirs. Chats are searchable by title and contents and are never indexed with ctx. Each turn writes a usage record, with tokens where the provider reports them (OpenAI-compatible: the final `usage` chunk; Anthropic: `message_start` and `message_delta`). A chat that was replying when the control plane stopped is `review` again after the restart.
- Chats call tools. The model is sent `tool_search` and Mycelium's own tools (`mcp__mycelium__<name>`, run in the control plane as the chat's owner; `walkthrough` says it needs a workspace), not every tool: `tool_search {query}` ranks the tools of every enabled source by keyword (a match in the server or tool name counts three, in the description one) and returns the best five with their descriptions and input schemas, and a tool it returns is loaded for the rest of the chat. Tools are named `mcp__<server>__<tool>`. OpenAI-compatible providers get `tools` and return `tool_calls` deltas, Anthropic gets `tools` and returns `tool_use` blocks with `input_json_delta`; results go back as `tool` messages or `tool_result` blocks, and the turn loops until the model answers without a call, at most 20 rounds (then a `note` says so). A provider answering a request that carries tools with an error that mentions tools gets the request again without them, with a `note` "This model does not take tools, so it answered without them.". Each call is an `acp_update` `tool_call` (`toolCallId`, `title` the tool name, `rawInput`, `status`) followed by a `tool_call_update` (`completed` or `failed`, text `content`; `tool_search` adds `rawOutput.loaded`). Every call except `tool_search` first raises a `permission_request` (`request_id` the call id, `toolCall {title, rawInput, server}`, options `allow_once`, `allow_always` and `reject_once`) and the chat is `input`; `POST /conversations/{id}/permission` answers it (409 `already_resolved` when it was not waiting), and the turn records `permission_resolved`, goes back to `working` and runs the tool or tells the model "The person declined this tool call." `allow_always` is remembered for that tool in that chat, and a server's `always_allow` skips the question for its tools. A new message while a call waits, or `cancel`, resolves it as `cancelled`, fails the call and carries on from the new message. A `note` is an `acp_update` whose `sessionUpdate` is `note`. History is replayed to the model from the events, so a reload and the next turn both see the calls and their results; a call with no result is replayed as not approved.

### Projects

A project is a folder on a node, set up by cloning a forge repository or by registering a folder that already exists. The control
plane relays `clone` and `register` to the node as `git` requests (clone times out after 10 minutes). The node refuses any
folder outside its `projects_root` (or containing `..`) and any clone address that is not http or https. For a clone the control
plane sends the forge's token in the request; the node passes it to git as an `http.<origin>/.extraheader` setting through the
environment, so it appears neither in arguments nor in `.git/config`, and it is not kept. Not done: later `git fetch` in a
project's folder uses whatever git credentials the node itself has, not the forge's token.

### Schedules

A loop on every control plane wakes each `scheduler_tick` (15 seconds), once migrations are done, and selects the due, unpaused schedules
with `for update skip locked`, so replicas never run the same schedule twice. For each it moves `next_run_at` to the next cron time after now (a
run missed while the control plane was down happens once), then starts the run: with `each_run` `new` a new conversation in the project or
folder on the node, with `same` the prompt is queued on the previous run's conversation, or a new conversation when there is none or
that one failed or is gone. A run that cannot start (node offline or draining, agent not installed) is recorded with `outcome` `skipped` and the
reason, and the schedule moves on. A schedule keeps the time zone it was made in: the next run is the cron evaluated as local time there and
converted back, so 20:00 stays 20:00 across daylight saving changes. The person's zone is sent by the clients after sign-in. Agents never
schedule on their own: `propose_schedule` stores the schedule as `proposed`, the tick ignores it, and the person confirms (with edits) or declines it in the
conversation or on the Schedules page.

### Live updates

The rule: every client-visible change publishes. A change a client shows is never learned by refetching.
`crates/control/tests/live.rs` performs each change below over HTTP or the node link and asserts the
message arrives on an open stream, and fails when a route that changes state is neither tested there
nor listed as silent, so a new route cannot skip publishing unnoticed. Owner-scoped messages reach
only the person who owns the conversation or started the sign-in; the rest reach everyone signed in.

| Change | Message | To |
|---|---|---|
| Device code created, approved, denied, expired, exchanged | `device` | everyone |
| Node credential issued (exchange) | `node` (offline), then `device` consumed | everyone |
| Node link opened, `hello` (agents), `logins`, link closed or silent for 90s | `node` | everyone |
| Drain on or off | `node` | everyone |
| Revoke | `node_removed`, plus `login_finished` for its sign-ins | everyone |
| Node `resources` sample | `node_resources` (`{node_id, sample}`) | everyone |
| Move requested, waiting, moving, done, failed, cancelled | `move` (`{conversation_id, node_id, fork, state, message?, result_id?}`), plus `conversation` as the node and folder change | owner |
| Sign-in output | `login_output` | starter |
| Sign-in ends: node reports it, `DELETE /logins/{id}`, node disconnects or is revoked | `login_finished` | starter |
| Terminal output | `terminal_output` | opener |
| Terminal ends: node reports it, `DELETE /terminals/{id}`, node disconnects or is revoked | `terminal_exited` | opener |
| Conversation created (also by starting an issue workspace), renamed, archived or restored | `conversation` (and `queue` on create) | owner |
| Archived conversation deleted | `conversation_removed` for it and each side chat | owner |
| Schedule proposed or confirmed | `schedule` | owner |
| Schedule proposal declined | `schedule_removed` | owner |
| Memory stored by the `remember` tool, or edited (`PATCH /memories/{id}`) | `memory` (the memory) | everyone |
| Memory deleted (`DELETE /memories/{id}`) | `memory_removed` (`{memory_id}`) | everyone |
| An instruction edit proposed by an agent, approved, declined or superseded, or its layer saved while it waits | `instruction_proposal` (the proposal) | the owner |
| History imported (`POST /imports`) | `conversation` per new conversation | owner |
| Import job progress, at most once a second, and when it finishes | `import_job` (the job) | owner |
| `session_started` (session id, mode, model) | `conversation` | owner |
| Mode or model changed | `conversation` | owner |
| Prompt sent from the queue | `event` user_message, `conversation` working, `queue` | owner |
| `acp_update`, `workflow` | `event` | owner |
| `permission_request` | `event`, `conversation` input | owner |
| Permission answered | `event` permission_resolved, `conversation` working | owner |
| `turn_end` | `event`, `conversation` review | owner |
| `session_failed`, or `hello` finding a running conversation without a session | `event` error, `conversation` failed | owner |
| Retry of a failed conversation | `conversation` (no longer failed) | owner |
| Resume scheduled or cancelled | `conversation` with or without `resume_at` | owner |
| Opened | `conversation` done | owner |
| Message queued, edited, removed, sent now | `queue` | owner |

The Activity page is not pushed: it asks again whenever a `conversation` message changes any conversation, since a turn ending always changes one.

Silent on purpose, because no client shows them: user, web session and client token rows (sign-in,
`last_used_at`), `nodes.last_seen_at` refreshed by `pong` (clients show `online`, which is published),
transcript chunks (no client endpoint or message), the node's `ack`s and prompt acknowledgements,
send-now and unacknowledged-prompt bookkeeping, and the routes `POST /auth/logout`,
`POST /nodes/{id}/logins` (the output follows as `login_output`), `POST /nodes/{id}/watch` (the samples follow as `node_resources`), `POST /nodes/{id}/processes/{pid}/stop` (the process leaves the next `node_resources` sample), `POST /logins/{id}/input` and
`POST /conversations/{id}/terminal` (the output follows as `terminal_output`), `POST /conversations/{id}/attachments` (the answer names the stored file, and the message that carries it publishes), the git routes `POST /conversations/{id}/git/{stage,unstage,stage-all,commit,fetch,undo,switch}` (the answer is the response, and the git panel reloads its status after each), `PUT /walkthroughs/{id}/reads/{step}` (the page marks the step checked itself), the walkthrough notes routes `POST /walkthroughs/{id}/notes`, `DELETE /walkthroughs/{id}/notes/{note_id}` and `POST /walkthroughs/{id}/notes/sent` (the page that made the change shows it; other devices see notes after a reload), `POST /terminals/{id}/input` and
`POST /terminals/{id}/resize` (the terminal answers with output), the chat tool server routes (settings rows no stream message shows, as with model providers), `POST /conversations/{id}/cancel` (the turn ends with `turn_end` from the node), and the forge routes
`PUT /me/time-zone`, and `POST /forges` and `DELETE /forges/{id}` (the Forges settings page replaces its list from the answer), and `POST /projects` (the Projects page adds the answer), `POST /projects/{id}/avatar` (the answer is the updated project, as with `PATCH /projects/{id}`), and the task and schedule routes that only change those lists (`POST` and `DELETE /tasks`, `POST`, `PATCH` and `DELETE /schedules`: the Tasks and Schedules pages replace their lists from the answer). `POST /tasks/{id}/start` and `POST /schedules/{id}/run` publish the new conversation as any start does, and a scheduled run does too.

### Events

`events.kind` is one of: `user_message`, `acp_update` (payload: the ACP `session/update` params as
sent), `permission_request` (payload: the ACP request params plus `request_id`), `permission_resolved`,
`turn_end` (`{stop_reason}`), `error` (`{message}`), `workflow` (`{run_id, line}`: `line` is one journal line as a JSON object, or a string when the line is not JSON; workflow events carry no node `seq` and are never deduplicated).
Clients render the thread from events alone.

## Node link protocol

JSON text frames, one object each, tagged by `type`. Every node-to-control message about a
conversation carries the keeper's journal `seq` so the control plane can deduplicate after a
reconnect; the control plane answers `ack` with the highest `seq` stored.

Node to control plane:

| type | fields |
|---|---|
| `hello` | `version, hostname, os, arch, agents: [{agent, version, available}], sessions: [{conversation_id, last_seq, live}]` (`live` is false for a session whose agent has exited) |
| `prompt_received` | `conversation_id, prompt_id` (sent once the prompt is written to the keeper) |
| `session_started` | `conversation_id, acp_session_id, modes?, models?, config_options?` |
| `acp_update` | `conversation_id, seq, update` |
| `permission_request` | `conversation_id, seq, request_id, request` |
| `turn_end` | `conversation_id, seq, stop_reason, usage?` (`usage` is the ACP prompt response's `usage` object when the agent sends one: `inputTokens`, `outputTokens`, `cachedReadTokens`, `cachedWriteTokens`) |
| `session_closed` | `conversation_id` (the node closed an idle session to make room, so the next message starts it again with `session/load`) |
| `session_failed` | `conversation_id, seq, message` |
| `prompt_received` | `conversation_id, prompt_id` (sent once the node has written `session/prompt` to the keeper) |
| `logins` | `logins: [{agent, account?, method, state, usage_left?, windows?: [{name, used_percent, resets_at?}], usage_read_at?}]` |
| `instructions_held` | `revision, unmanaged: [label]` |
| `login_output` | `login_id, data` (UTF-8 text from the PTY) |
| `login_finished` | `login_id, ok` |
| `terminal_output` | `terminal_id, data` (base64; raw bytes from the PTY) |
| `terminal_exited` | `terminal_id` |
| `transcript` | `path, offset, data` (base64; the bytes at `offset`) |
| `workflow` | `conversation_id, run_id, line` (the journal line as JSON; no `seq`) |
| `resources` | `sample` (see `POST /nodes/{id}/watch`; sent every 3 seconds while a watch lease lasts) |
| `process_stopped` | `request_id, ok, error?` (the answer to `stop_process`) |
| `pong` | |

Control plane to node:

| type | fields |
|---|---|
| `ack` | `conversation_id, seq` |
| `stop_process` | `request_id, pid, name` (send SIGTERM to the process if it is still called `name` and is stoppable) |
| `start_session` | `conversation_id, agent, cwd, resume?: acp_session_id, account?, instructions?` |
| `instructions` | `revision, global?` |
| `prompt` | `conversation_id, prompt_id, text, attachments?` (`[{name, media_type, data}]`, `data` base64). The node sends PNG, JPEG, GIF and WebP as ACP `image` blocks when the agent's `promptCapabilities.image` is true, other valid UTF-8 files as embedded `resource` blocks when `embeddedContext` is true, and writes the rest under `<cwd>/.mycelium/attachments/<prompt_id>/` (a `.gitignore` of `*` sits in `.mycelium/`) and names the path in the prompt. The node link accepts messages up to 64 MiB |
| `cancel` | `conversation_id` |
| `permission_response` | `conversation_id, request_id, outcome` (ACP `RequestPermissionOutcome`) |
| `set_config` | `conversation_id, mode?, model?, config?` |
| `close_session` | `conversation_id` |
| `start_login` | `login_id, agent, method, account?` |
| `login_input` | `login_id, text` |
| `stop_login` | `login_id` |
| `start_terminal` | `terminal_id, conversation_id, cwd, cols, rows` |
| `terminal_input` | `terminal_id, data` (base64 bytes) |
| `resize_terminal` | `terminal_id, cols, rows` |
| `stop_terminal` | `terminal_id` |
| `transcript_ack` | `path, bytes` |
| `restore_transcript` | `path, offset, data` (base64 chunk of an agent transcript, written under the node's home inside `.claude/projects` or `.codex/sessions`; offset 0 starts the file again) |
| `continue_sequence` | `conversation_id, after` (the session's journal numbering continues after `after`) |
| `watch` | `seconds` (a lease: the node samples every 3 seconds until it runs out, and does nothing when nobody holds one; asking again extends it) |
| `ping` | |
| `revoked` | the node deletes its credential and exits 0 |

The control plane keeps every sent `prompt` as unacknowledged until `prompt_received` arrives with its `prompt_id` (or the turn ends or the session fails). After a `hello`, it sends again the unacknowledged prompts of sessions the node reports live, and forgets those of sessions it does not. A `session_started` for the `acp_session_id` the conversation already holds sets mode and model only where they are empty. The node ignores a `prompt_id` it has already received (the ids it has written are persisted in the session directory) and answers a repeat with `prompt_received` again. Failures the node emits itself (a refused start, an unreachable keeper) take the next free `seq` for the conversation, tracked in the session directory, never a fixed value.

An answer to a permission request is refused with 409 when the conversation has failed (`session_failed`, retry it first) or is no longer waiting for input (`already_resolved`). A pending send-now is forgotten when its node disconnects.

A node reports one `logins` entry per account: the agent's default home plus one per folder under `<state_dir>/accounts/<agent>/<name>`, each named by the email found in that folder's own config (`.claude.json` for Claude Code, the `auth.json` identity token for Codex), else by the folder name. The node runs an agent as an account by setting `CLAUDE_CONFIG_DIR` or `CODEX_HOME` to that folder, for both `start_session` and `start_login`; `account` on `start_login` signs in to (and creates, if new) that account's folder. Each `logins` report replaces the node's previous list. A Claude account's usage is read every 5 minutes from `GET https://api.anthropic.com/api/oauth/usage` with the bearer token in that account's own `.credentials.json` (best effort): every window the response carries (`five_hour`, `seven_day`, `seven_day_opus`, ...) is reported with its percent used and `resets_at`, and `usage_left` is 100 minus the busiest. The control plane keeps the latest per account and a reading history, and `GET /nodes` adds to each window `rate_per_hour` and `runs_out_at` (the rate since the window last reset, shown only when it would run out before it resets). Codex accounts report no windows: codex-acp 2.2.2 exposes rate limits only inside its own session text, and its `_meta.quota` carries token counts.

`start_login` carries the method the node reported in `logins`: `claude` supports `oauth_token`, `codex` supports `device_auth`. Any other method is answered with `login_output` explaining it and `login_finished` with `ok` false.

On `hello` the control plane marks every `working` or `input` conversation on that node that has no live session as `failed`, with an `error` event. A prompt for a conversation whose session is not live is preceded by `start_session` with `resume` set to its `acp_session_id`. Journal `seq` values keep increasing across a resume: the node continues from the end of the previous journal.

The node reconnects with exponential backoff (1s to 60s, jitter) and sends `hello` with the last
acknowledged `seq` per live conversation; the control plane replies with `ack`s and the node
replays everything after them from the keepers.

## Node daemon

### Config

`~/.config/mycelium/node.toml` (or `/var/lib/mycelium-node/node.toml` under the system service):
`control_plane`, `node_id`, `token`, `state_dir`, `projects_root`, `max_sessions` (default 8). At the limit the node closes its least recently used idle session (not working, not waiting for input) to make room and tells the control plane; the conversation resumes through `session/load` on its next message. It refuses the new session only when every session is busy.
`mycelium node join` writes it with mode 0600. `MYCELIUM_NODE_CONFIG` overrides the path.
`MYCELIUM_JOIN_TOKEN_FILE`, when set, joins without a device code: a single-use token minted in the
UI. Stage 1 implements the device code; the token file is accepted by the CLI flag parser and
reports "Join tokens arrive in a later release" until then.

### Agents

| agent | ACP command (default, overridable) | Login command |
|---|---|---|
| `claude` | `claude-agent-acp` (`MYCELIUM_CLAUDE_ACP`) | method `oauth_token`: `claude setup-token`, verify against `claude --help` |
| `codex` | `codex-acp` (`MYCELIUM_CODEX_ACP`) | method `device_auth`: `codex login --device-auth` |

An agent is available when its ACP command resolves on `PATH`. The daemon reports versions from
`<cmd> --version` where it answers.

### ACP client

The daemon is the ACP client. Per conversation it starts a keeper, then speaks JSON-RPC 2.0 over
the keeper socket: `initialize` (protocol version 1, `clientCapabilities: {fs: {readTextFile: false,
writeTextFile: false}, terminal: false}`), then `session/new {cwd, mcpServers: []}` or
`session/load`/`session/resume` when resuming. It forwards every `session/update` as `acp_update`,
turns `session/request_permission` into `permission_request` and answers it when the control plane
sends `permission_response`, and answers any other agent-to-client request with JSON-RPC error
-32601. It keeps reading updates after a prompt response, because Claude Code sends turns it
starts itself. `cancel` is the ACP `session/cancel` notification.

### Keeper

`mycelium keeper --dir <state>/sessions/<conversation_id> -- <acp command>`:

1. Spawns the command with piped stdin and stdout; stderr goes to `<dir>/stderr.log`.
2. Appends every frame in both directions to `<dir>/journal.jsonl` as
   `{"seq": n, "dir": "in"|"out", "frame": <json>}`, `seq` starting at 1, fsync'd per frame.
3. Listens on `<dir>/keeper.sock`. A client sends `{"attach": {"after": n}}` and receives every
   journal entry after `n`, then live entries; it sends `{"send": <frame>}` to write to the agent.
   One client at a time; a new attach replaces the old. After the replay of existing entries
   the keeper sends one `{"live": true}` line, then live entries. The node waits for this marker
   to know the replay is complete and that everything after it is new. `{"close": true}` kills
   the agent, after which step 4 applies. The keeper streams the replay from the journal file and
   holds none of it in memory.
4. When the agent exits it writes `{"exit": code}` to `<dir>/exit.json` and quits.

The daemon restarts without touching keepers: it reattaches to every `<dir>/keeper.sock` that
answers and treats the rest as ended. It saves the session's state to `<dir>/state.json` whenever
an acknowledgement catches up with the end of the journal, and reattaches after that position
instead of 0; without a saved state it replays from 0.

### Transcripts and workflows

The daemon follows `~/.claude/projects/**/*.jsonl` and `~/.codex/sessions/**/*.jsonl`, sending
new bytes from the last `transcript_ack` (inotify or a 2-second poll). Workflow journals
(`~/.claude/projects/*/*/subagents/workflows/*/journal.jsonl`) are followed the same way and also
sent as `workflow` messages for the conversation whose `acp_session_id` matches the session
directory, so clients can draw the workflow card live.

### History import and search

Mirrored transcripts (`.claude/projects/**` and `.codex/sessions/**`, subagent journals excluded) are imported
on request from Settings, Agents: each becomes a `done` conversation owned by the requester, with the transcript's
session id as `acp_session_id` so the node can resume it, and events in the shape live conversations use. A
transcript whose session id already belongs to a conversation on that node is counted as imported and skipped.
Every user message and every assistant turn, live or imported, is stored in `search_entries` and searched with
Postgres full-text search (`websearch_to_tsquery`, English) for now, not ctx. Search is scoped to the caller's own
conversations and returns the best matching message per conversation with `**` around the matched words.

An import is a background job that handles one transcript at a time: the file is read line by line, each conversation
is committed in its own transaction with its events inserted in batches, and a file that fails is recorded in the job
with a reason and skipped. `done`, `imported`, `skipped` (session already imported), `waiting` and `failed` are counted
as it goes and kept in `import_jobs`; a running job found at boot is ended. A mirrored file written to in the last five
minutes is not imported (`waiting`: still being sent by the node, or in use) and stays pending for a later import. A
NUL in any stored string is stored as U+FFFD. The node sends at most sixteen 256 KiB transcript chunks per two-second
poll, and the control plane rewrites a file's `transcripts` row on a chunk shorter than 256 KiB or at most every three
seconds.

An import records one `usage_turns` row per turn from the transcript: the turn's day is the day of its last assistant or
tool entry, time working runs from the user message that starts the turn to that entry (capped at two hours so an idle
gap is not work), and tokens are summed over the turn's assistant messages. Claude Code usage is read from
`message.usage` once per `message.id` (a message is written several times while streaming, the last copy wins; `<synthetic>`
messages are skipped), with cache writes split into five-minute and one-hour from `cache_creation`. Codex usage is the growth
of `token_count` `info.total_token_usage` between events, with `cached_input_tokens` counted as cache reads and removed from
input, and the model comes from the latest `turn_context`. Cost is estimated from list prices by the longest model id
prefix and stored with `cost_estimated`; a turn with a model that has no price keeps its tokens and has no cost. Boot
repair `import_usage` re-parses every import without live events from the mirrored transcript and replaces its rows.

List prices, USD per million tokens (input, cache write 5 minutes, cache write 1 hour, cache read, output), read from
https://platform.claude.com/docs/en/about-claude/pricing and https://developers.openai.com/api/docs/pricing on 2026-10-23.
The table is `PRICES` in `crates/control/src/pricing.rs`; Claude Haiku 5.5 is left out because its price depends on
prompt size, so its turns show tokens only.

| Model | Input | Write 5m | Write 1h | Read | Output |
|---|---|---|---|---|---|
| claude-fable-5-1, claude-mythos-5-1 | 10 | 12.5 | 20 | 0.25 | 50 |
| claude-fable-5, claude-mythos-5 | 10 | 12.5 | 20 | 1 | 50 |
| claude-opus-5-5 | 4 | 5 | 8 | 0.2 | 20 |
| claude-opus-5, 4-8, 4-7, 4-6, 4-5 | 5 | 6.25 | 10 | 0.5 | 25 |
| claude-opus-4-1, claude-opus-4 | 15 | 18.75 | 30 | 1.5 | 75 |
| claude-sonnet-5-5 | 2 | 2.5 | 4 | 0.1 | 10 |
| claude-sonnet-5 | 2 | 2.5 | 4 | 0.2 | 10 |
| claude-sonnet-4 (4, 4-5, 4-6) | 3 | 3.75 | 6 | 0.3 | 15 |
| claude-haiku-4-5 | 1 | 1.25 | 2 | 0.1 | 5 |
| claude-3-5-haiku | 0.8 | 1 | 1.6 | 0.08 | 4 |

OpenAI (input, cached input, output): gpt-5.6-sol 4 / 0.4 / 20, gpt-5.6-terra 2 / 0.2 / 12, gpt-5.6-luna 0.2 / 0.02 / 1.2,
gpt-5.5 5 / 0.5 / 30, gpt-5.4 2.5 / 0.25 / 15, gpt-5.4-mini 0.75 / 0.075 / 4.5, gpt-5.4-nano 0.2 / 0.02 / 1.25,
gpt-5.3-codex and gpt-5.2 1.75 / 0.175 / 14, gpt-5.1 and gpt-5 1.25 / 0.125 / 10, gpt-5-mini 0.25 / 0.025 / 2,
gpt-5-nano 0.05 / 0.005 / 0.4. Short-context rates; long-context rates are higher and not applied.

### Terminal

Each conversation has a terminal panel on its right-hand rail. Opening it posts `POST /conversations/{id}/terminal`; the
node runs `tmux new-session -A -s mycelium-<first 8 characters of the conversation id> -c <cwd>` in a PTY of the
panel's size, so reopening reattaches to the same session, and falls back to `$SHELL` (or `/bin/sh`) in the folder when
tmux is not installed. Input is written raw, output is read in chunks and relayed as base64, and a resize resizes the
PTY. Closing the panel stops only the PTY client: the tmux session keeps running on the node. The control plane tracks
each terminal's owner and node like a sign-in, so only the opener sees its output and a node can only send output for
terminals it was asked to start. The clients carry their own terminal emulator (cell grid, cursor, SGR colours,
scroll regions, alternate screen) and send keys as bytes. Not done: scrollback in the fallback shell, mouse reporting,
text selection and answering the terminal's own queries (device attributes, colour requests).

### Git panel

Each conversation has a Git panel on its right-hand rail, beside Details and Terminal (one panel open at a time; a bottom
sheet on phones). The control plane relays a `git` request to the node over the link, like the file listing, and the node
runs `git -C <folder>` with arguments, never through a shell: `GET /conversations/{id}/git/status`, `git/log?limit=`,
`git/diff?path=&staged=` (untracked files diff against `/dev/null`, output cut at 200 KB with a `truncated` flag) and
`POST git/stage`, `git/unstage`, `git/stage-all`, `git/commit` `{message, amend?, signoff?, no_verify?}` (an amend may keep the last message), `git/fetch`, `git/pull` `{rebase?}`, `git/push` `{force?}` (`--force-with-lease`; a branch with no upstream is pushed to `origin`, or the only remote, with `--set-upstream`) and `git/undo` (`reset --soft HEAD~1`, answering with the undone commit's message). Calls
time out after 10 seconds (fetch, pull and push 60), an offline node is a 409 `node_offline`, an empty commit message is a 400, and git's
own error text comes back as a 409 `git_failed`. `GET git/graph?skip=&limit=` (default and most 200) returns commits of every
branch in topological order with parents and structured refs, `GET git/show?hash=` a commit's patch, `GET git/branches` local and
remote branches by recency with author, date and subject, and `POST git/switch` `{branch, create?, track?}` runs `git switch`
(never a stash: git refuses when changes would be overwritten and says so). `GET git/stack` answers the current branch's stack from main down through its children, read from Git Town's `git-town-branch.<branch>.parent` config with `git-town.main-branch` (or `main-branch` in `.git-town.toml`, or the older `main-branch-name`), or inferred from which local branches contain which with `inferred: true`; each branch has its counts against its parent, `pushed`, `needs_sync`, last subject and, from the project's forge by head branch, its pull or merge request; `git_town_available` says whether `git-town` is on the node's PATH, and `POST git/town/sync` and `git/town/propose` run `git-town sync --stack` and `git-town propose` (300 seconds, its own output as the 409 text on failure). The panel has Changes (files under Tracked and Untracked, by folder, with
line counts and a checkbox that stages or unstages; selecting a file shows its diff in place of the list) History, Graph and Stack tabs (lanes
drawn on the client from parent hashes; the footer's branch name opens a searchable branch picker),
with the branch, ahead and behind counts, Fetch, the commit message, Commit and Undo at the bottom. It refreshes when it
opens, after every action and when the conversation's status changes. Not done: a diff of every file at once (View diff
opens the first changed file), unstaging in a repository with no commits, discarding changes, and branch switching.

## Clients

Compose Multiplatform, Kotlin, one `shared` module and two targets: `web` (wasmJs, served by the
control plane) and `desktop` (JVM). They implement the screens on the design canvas for stage 1,
with the design system's tokens and sizes, in dark and light:

- Sign in (control plane address, then OIDC in the browser; desktop uses a client device code).
- Approve a device code (`/approve?code=…`).
- The shell: sidebar by status with group-free rows (stage 1 has no groups), conversation header
  with rename, thread rendered from events (messages, tool calls, diffs, plans, permission cards,
  workflow card), composer (mode, model, effort where offered, queue, stop, send), the rail with
  Details, Terminal and Git.
- New conversation: node, agent, folder.
- Nodes: list, join commands (shell, Nix, NixOS), logins with sign-in output, drain, revoke.
- Past conversations: archived and done.
- Tasks, under Nodes in the sidebar: a board of lanes Needs input, To do, Running, To review and Done. A task is a
  conversation placed by its status (Failed waits in To review), plus the backlog tasks in To do, which Start turns into a
  conversation. Each lane collapses to a strip, Done starts collapsed, and the collapsed lanes are remembered per client.
  One filter (text, project, node, agent) applies to every lane. A card shows the agent, model, project and worktree branch,
  a timer while running, Stop on a running agent, and the pending permission request's options on a conversation that needs input.
- Schedules, under Tasks in the sidebar: the schedules with when (Every, Daily, Weekly or a cron expression) and its time zone, what each
  run does (new conversation or the same one), the next run, Run now, Pause or Resume and Delete, and the last runs with the
  conversation each produced or why it was skipped. A proposed schedule shows as Waiting for you with Review and Decline. The same
  card appears above the composer of the conversation that proposed it, with Confirm schedule, Change (a dialog to edit the time,
  the zone and each run) and Decline.

## Packaging and release

- `install.sh`: installs to `/opt/mycelium/mycelium` (or `~/.local/bin` without root), downloads
  `${S3}/$(curl ${S3}/latest)/mycelium-<os>-<arch>`, verifies `.sha256`, writes a systemd user or
  system unit for `mycelium node run`, then runs `mycelium node join` if no credential exists.
  `MYCELIUM_BINARY=/path` installs a local build.
- `flake.nix`: `packages.default` (the binary), `nixosModules.mycelium-node`,
  `darwinModules.mycelium-node`, `homeManagerModules.mycelium-node`
  (`services.mycelium-node = { enable; controlPlane; joinTokenFile ? null; }`).
- `Dockerfile`: control plane image with the web client at `/usr/share/mycelium/web`, non-root uid
  1000, `dumb-init`, `ghcr.io/mycelium-app/mycelium`.
- Android: `clients/androidApp`, application id `slop.mycelium`. `versionCode` is
  `major*10000 + minor*100 + patch` and `versionName` is `major.minor.patch`, both read from `CI_COMMIT_TAG` (or
  `-PappVersion=vX.Y.Z`); off a tag they are `1` and `0.0.0-dev`. The phone layout applies below 600dp wide, on any
  platform: the conversation is the main screen, the list is a left drawer, menus are bottom sheets, controls are at
  least 44dp. Sign-in is the client device code flow in the system browser.
- CI builds and publishes, never tests. On `main`: the image (`:sha` and `:latest`) and Pages. On a `vX.Y.Z` tag, each
  of these uploads its files and `.sha256` to `s3://artefacts/mycelium/<tag>/` from its own job:
  `binary:x86_64` (static binary, via `publish:s3`, which also writes the `latest` marker and `install.sh`),
  `build:clients`, one Gradle run for the desktop `.deb` and `.tar.gz`, `mycelium-web.tar.gz` and `mycelium-android.apk`.
  `build:clients` pins its own Android SDK, signs with `apksigner` from `ANDROID_KEYSTORE_BASE64` and
  `ANDROID_KEYSTORE_PASSWORD` (alias `mycelium`), and refuses to upload unless the APK's application id, version code
  and version name match the tag and the code exceeds the previous tag's. `publish:release` links all of them.

## Tests (local only)

- Unit tests next to the code for parsing, ordering and state transitions.
- Control plane integration tests against a real Postgres (`docker run postgres:17-alpine`), driven
  through HTTP.
- Tests that need Postgres skip with a printed `SKIPPED` line when `MYCELIUM_TEST_DATABASE_URL` is unset.
- An end-to-end test: control plane, a node, and a fake ACP agent (`crates/node/tests/fake-acp`, a
  small script speaking `initialize`, `session/new`, `session/prompt`, `session/update`,
  `session/request_permission` and `session/cancel`), driven through the HTTP API: join, start a
  conversation, stream updates, answer a permission, queue, cancel, restart the daemon mid-turn and
  see the replay.
- One manual check against the real `claude-agent-acp` before the release.

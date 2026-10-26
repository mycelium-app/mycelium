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
| `MYCELIUM_DATABASE_POOL_SIZE` | `10` | Maximum database connections per control-plane process |
| `MYCELIUM_DATABASE_ACQUIRE_TIMEOUT_SECONDS` | `3` | Time to wait for a database connection |
| `MYCELIUM_LISTEN` | `0.0.0.0:8080` | HTTP listen address |
| `MYCELIUM_PUBLIC_URL` | required | e.g. `https://mycelium.example.com`, used in links and OIDC redirects |
| `MYCELIUM_DATA_DIR` | `/var/lib/mycelium` | Transcript copy and node journals |
| `MYCELIUM_OIDC_ISSUER` | required unless dev login | OIDC issuer URL |
| `MYCELIUM_OIDC_CLIENT_ID` | required unless dev login | |
| `MYCELIUM_OIDC_CLIENT_SECRET` | empty | Public clients use PKCE only |
| `MYCELIUM_DEV_LOGIN` | empty | An email; signs every browser in as that user. Refused unless `MYCELIUM_PUBLIC_URL` is `http://localhost` or `http://127.0.0.1` |
| `MYCELIUM_ALLOWED_EMAILS` | empty | Comma-separated email addresses allowed to sign in through OIDC (case-insensitive, the provider must not report the address as unverified). Empty allows every account at the identity provider, and the control plane logs a warning at startup. The dev login ignores it |
| `MYCELIUM_WEB_DIR` | `/usr/share/mycelium/web` | The built web client, served at `/` |
| `MYCELIUM_EDITOR_DIR` | `/usr/share/mycelium/editor` | The built Design editor, served at `/design/` to signed-in users (anyone else is sent to `/auth/login`) |
| `MYCELIUM_ARTIFACT_DOMAIN` | empty | Domain the artifact host serves `https://{slug}.<domain>` on; an artifact's `url` is null without it |
| `MYCELIUM_ARTIFACT_HOST_TOKEN` | empty | Bearer token of the artifact host. Empty disables `/internal/artifacts/*` (404) |
| `MYCELIUM_BROWSER_URL` | empty | DevTools endpoint of a headless browser, `http://host:9222` or `ws://`. Empty disables artifact snapshots |
| `MYCELIUM_SKIP_MIGRATIONS` | `false` | For gated deploys only |
| `MYCELIUM_RELEASE_URL` | `https://github.com/mycelium-app/mycelium/releases/latest/download/latest` | Plain-text tag of the newest Mycelium release, checked hourly |
| `MYCELIUM_NPM_REGISTRY_URL` | `https://registry.npmjs.org` | Registry the newest `@agentclientprotocol/claude-agent-acp` and `@agentclientprotocol/codex-acp` are read from, checked hourly |
| `MYCELIUM_RELEASES_URL` | `https://github.com/mycelium-app/mycelium/releases/download` | Folder of `<version>/mycelium-<os>-<arch>` builds and their `.sha256` that a node downloads when it updates. A node older than the control plane updates itself when it connects; the Update button on a node page does the same on request |
| `MYCELIUM_TRUSTED_PROXIES` | empty | Comma-separated addresses or CIDR ranges of reverse proxies. `X-Forwarded-For` is read only on connections from these, taking the nearest address that is not itself a proxy. Empty trusts no header |

### Boot, health and migrations

- Migrations are embedded (`sqlx::migrate!`) and run at boot inside `pg_advisory_lock(7461001)`.
  Replicas block on the lock, then find nothing to do.
- A failed migration does not exit. The process stays up, `GET /readyz` returns 503 with the
  error, and the error is logged once at error level at startup.
- `GET /healthz` is 200 whenever the process serves HTTP. `GET /readyz` is 200 only when
  migrations and startup recovery have finished and the database answers. Chat sends return
  503 `starting` until recovery finishes, so it cannot reset a newly started reply.
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
client_tokens    id uuid pk, user_id, label text, hostname, address, token_hash bytea unique, created_at, last_used_at, revoked_at null, scopes text[] null (null is full access), expires_at null, conversation_id null (the conversation that asked)
api_access_requests  id uuid pk, owner, conversation_id, scopes text[], hours, reason, token_id null, state requested|approved|declined, created_at, decided_at null
ssh_access_requests  id uuid pk, owner, conversation_id, node_id (the node to reach), minutes, reason, state requested|approved|declined|revoked, token_hash null unique (SHA-256 of the secret), created_at, decided_at null, expires_at null, revoked_at null
ssh_sessions  id uuid pk, request_id, owner, conversation_id, node_id, started_at, ended_at null
api_token_calls  id bigint pk, token_id, method, route (the matched route, with `{id}` placeholders), status, called_at; rows older than 30 days are deleted whenever a scoped token makes a call
agents           node_id, agent text ('claude'|'codex'), version text, available bool, pk(node_id, agent)
logins           id uuid pk, node_id, agent, account text null, method text, state text, usage_left, usage_windows jsonb, usage_read_at
login_usage_history  node_id, agent, account, read_at, windows jsonb: one row per reading, the last 8 days kept
                 ('signed_in'|'signed_out'|'expired'|'unknown'), updated_at, unique(node_id, agent, coalesce(account, ''))
conversations    id uuid pk, owner uuid, node_id, agent, cwd text, title text, status text, agent_active bool, last_activity_at null, acp_session_id text null,
                 mode text null, model text null, account text null, created_at, updated_at, archived_at null
events           id bigserial pk, conversation_id, seq bigint, kind text, payload jsonb, created_at,
                 unique(conversation_id, seq)
queued_prompts   id uuid pk, conversation_id, position int, text text, created_at
transcripts      node_id, path text, bytes bigint, updated_at, pk(node_id, path)
projects         id uuid pk, owner uuid, node_id uuid, name text, path text, forge_id uuid null, repository text null, group_id uuid null, icon text null, avatar_sha256 text null, avatar_automatic bool,
                 created_at, unique(node_id, path)
worktrees        id uuid pk, project_id uuid, branch text, path text, issue_number bigint null, created_at, unique(project_id, branch)
tasks            id uuid pk, owner uuid, node_id uuid, agent text, cwd text, project_id uuid null, branch text null, title text, prompt text null,
                 started_at null, conversation_id uuid null, created_at
schedules        id uuid pk, owner uuid, node_id uuid, agent text, cwd text, project_id uuid null, prompt text, cron text,
                 each_run text ('new'|'same'), paused bool, time_zone text ('UTC'), proposed bool, conversation_id null, next_run_at, last_run_at null, last_conversation_id null, created_at
users.time_zone  text null: the person's own zone, set by `PUT /me/time-zone` or by the first schedule created with one
schedule_runs    id uuid pk, schedule_id uuid, ran_at, outcome text ('started'|'skipped'), reason text null, conversation_id uuid null
groups           id uuid pk, owner uuid, name text, created_at, unique(owner, name)
assistants       id uuid pk, owner uuid, name text, description text null, group_id uuid null, preferences jsonb, limited jsonb, conversation_id uuid null (the current session), pinned_at, archived_at, created_at, updated_at, unique(owner, name)
assistant_files  assistant_id uuid, path text, content bytea, sha256 text, updated_at, pk(assistant_id, path); `AGENTS.md` holds the assistant's instructions
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
artifacts        id uuid pk, owner uuid, conversation_id uuid null, kind text ('design'|'page'|'markdown'|'file'), title text, slug text unique, public bool, latest_version int, created_at, updated_at
artifact_versions  artifact_id, version int, author text ('agent'|'person'), media_type text, sha256 text (the bytes are stored as an attachment of the owner), note text null, thumbnail_sha256 text null (the snapshot, stored the same way), created_at; pk(artifact_id, version)
artifact_comments  id uuid pk, artifact_id, author_name text, body text, anchor jsonb null, resolved bool, created_at
artifact_data    artifact_id, key text, value jsonb, updated_at; pk(artifact_id, key)
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

`agent_active` (boolean on the conversation, default false) is true while the main agent itself is busy:
from the moment a prompt is sent to the node until its turn ends, and while the agent streams output it
was not prompted for (`agent_message_chunk`, `agent_thought_chunk`, `tool_call`, `tool_call_update`,
`plan`), as Claude Code does when its background tasks finish. It is cleared by a turn end of any stop
reason, a failed or closed session, closing and archiving. `last_activity_at` (not on the wire) is
written with it, at most every 5 seconds per conversation. Unprompted output on a `review` or `done`
conversation sets it `working` again. Status is `working` while `agent_active` or a workflow agent of the
conversation is alive. A workflow agent (journal `agentId`) is alive when it has a `started` line and no
`result` or `failed` line, and its mirrored transcript (`agent-<agentId>.jsonl` under the conversation's
`acp_session_id` folder, same node) changed in the last 15 minutes, or, with no transcript yet, it
started in the last 15 minutes; lines without an `agentId` count only while younger than 15 minutes. A
sweep every 10 seconds (`scheduler_tick` when shorter) clears `agent_active` once the turn has ended and
nothing was streamed for 30 seconds, and returns a `working` conversation with no active agent and no
live workflow agent to `review`.

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
- `GET /api/v1/sessions` → the caller's browser sessions and client tokens `[{id, kind: "browser"|"client"|"agent", label, hostname, address, last_used_at, current, scopes?, expires_at?, conversation_id?, conversation_title?}]`, the current one first. `DELETE /api/v1/sessions/{id}` ends one at once (404 for another person's); ending the current one is 409 `current_session` unless `?confirm_current=true`. Nodes carry `address` in `GET /api/v1/nodes`. An `agent` session is a token granted by `request_api_access`: it lists its scopes, its expiry and the conversation that asked, and is gone from the list once it expires. Revoking it with `DELETE` stops it at once.

Conversations:
- `GET /api/v1/conversations?archived=false`.
- `POST /api/v1/conversations` `{node_id, agent, account?, cwd, title?, prompt?}` → the conversation (409 `account_not_signed_in` unless the node reports that account signed in for the agent); the node
  starts the ACP session, and `prompt`, if given, is the first message.
- `GET /api/v1/conversations/{id}`, `PATCH` `{title?, archived?, account?, use_default_account?, teach_me?}` (409 `account_locked` once the conversation has a session; `teach_me` turns Teach me mode on or off for this conversation: while on, the session instructions ask for hints and explanations instead of code and no file edits, a change is recorded as a `teach_mode` event `{on}` for the thread divider, and the session restarts with the new instructions before the next message is sent), `POST /{id}/opened` marks review as done, `DELETE` (204) removes an archived conversation for good (404 for another person's, 409 `not_archived` unless archived). Its events, queued messages, search entries, goal, walkthroughs and usage records go with it, so Activity totals no longer count its turns; its side chats are deleted too; attachments no message of the owner still carries are removed, and shared ones stay. The node is told to close its session, and `conversation_removed` is streamed for the conversation and each side chat.
- Subagent side chats: a Claude Code subagent transcript (`agent-<id>.jsonl` under the session's `subagents` folder) becomes a read-only side chat (`side_kind` `subagent`, `status` `done`, `imported_from` the transcript path) under the conversation whose `acp_session_id` is the session, titled from the description of the parent's Task call with the same prompt, else the first line of its prompt. It is created or rebuilt as the node's upload of the file catches up with the `size` each `transcript` message carries, and by an import (a subagent whose parent is not known yet waits). Prompting one is 400 `read_only`, and `POST /conversations` and the side chat route refuse `subagent` (400 `parent_invalid`). An import records a file whose stored bytes are below its reported size as failed with `Still uploading.`.
- `GET /api/v1/conversations/{id}/events?after=<seq>&limit=500`. With `before=<seq>` it returns the newest page of events below that seq, in ascending order, starting at the nearest user message so a page never begins mid-turn.
- `GET /api/v1/conversations/{id}/session` returns the latest `available_commands_update`, `config_option_update` and `current_mode_update` of the conversation as a JSON array of the update objects, so the slash commands, settings and mode survive a client that has not loaded the early events. The control plane keeps them as events arrive from the node.
- `POST /{id}/retry` (204, owner only) starts a failed conversation's session again: status returns to `done`, the node is asked to `session/load` the conversation's ACP session (a new session in the same folder when it never had one) and anything queued is delivered. 409 `node_offline` while the node is offline, 409 `node_removed` when it was revoked. A conversation that is not failed is left alone. `POST /{id}/prompt` on a failed conversation retries first and then sends.
- A failure whose message is a usage limit (Claude Code's "You've hit your session limit · resets 4:30pm (Europe/London)", the weekly and Opus variants, the older `usage limit reached|<epoch>`, Codex's "hit your usage limit") is recorded on its `error` event as `usage_limit: {window: five_hour|seven_day|seven_day_opus|usage, resets_at?}`. The time is read in the zone the message names (the person's zone when it names none), as its next occurrence; a message without one takes the earliest future `resets_at` among the login's usage windows.
- `POST /{id}/dismiss` (204, owner only) settles a failed conversation as `done` without restarting its session, clears any pending resume and keeps its history. A conversation that is not failed is left alone.
- `POST /{id}/resume` `{at?}` (204, owner only) schedules a retry of a failed conversation: `at` defaults to the limit's `resets_at` plus a minute and is never earlier than now. 409 `not_failed`, 409 `no_reset_time` when there is neither. `DELETE /{id}/resume` (204) cancels it. The conversation carries `resume_at` while one is pending; any retry clears it. Due resumes are claimed with `update ... skip locked`, so two control planes never both fire one: the session is retried and what was queued is delivered, else "Continue from where you stopped." is sent as the person's message. A resume that cannot start (node offline) is put back a minute later. `limit_resumes` counts automatic resumes since the last finished turn.
- `GET`/`PUT /api/v1/usage-limit` `{on_usage_limit: "ask"|"resume"|"switch"}` (default `ask`). With `switch`, a harness conversation (not a side chat) that reaches a limit continues on the other signed-in account of the same agent on its node with the most usage left (above zero; the `continued` event carries `reason: "usage_limit"`) and is sent "The usage limit was reached on {account}. Continue from where you stopped."; with no such account, or when the switch is refused, it behaves as `resume`. With `resume`, a usage limit with a known reset schedules its own resume at once, up to 3 times in a row without a finished turn, and a goal on that conversation stays active instead of stopping. Side chats are never resumed automatically.
- `GET`/`PUT /api/v1/wellness/settings`: break reminders (`break_reminders`, `break_after_minutes` 5 to 480, default 50; `snooze_minutes` 1 to 120, default 10; `break_minutes` 1 to 120, default 5), `pomodoro`, `quiet_hours` (off by default) with three hold switches (`quiet_hold_notifications`, `quiet_hold_schedules`, `quiet_hold_pings`, default on), `time_zone` (IANA, falls back to the account's) and seven `quiet_days` (`weekday` 0 Monday to 6 Sunday, `enabled`, `all_day`, `starts`/`ends` as `HH:MM`; when `ends` is not after `starts` the window runs into the next morning). Defaults when never saved: 22:00 to 08:00 on weekdays, all day at weekends. A save broadcasts `wellness_changed` to the person's clients.
- `POST /api/v1/wellness/heartbeat {seconds}` adds viewing time to the person's local day, capped at 90 seconds per call. `POST /api/v1/wellness/breaks {minutes?}` starts a break (201, default `break_minutes`); `DELETE /api/v1/wellness/breaks/current` ends it early (204). `PUT`/`DELETE /api/v1/wellness/puzzles/{puzzle}/today` marks or unmarks `wordle`, `connections`, `daily-orbs` or `catfishing` as played today (204; other names 404).
- `GET /api/v1/wellness/overview?days=7` (1 to 31): per local day `viewing_minutes`, `agent_minutes` (the union of the person's turn working spans, so concurrent agents count once), `breaks` and `puzzles`; plus `last_break_at`, `current_break`, `quiet_now` and `held` (schedules due but waiting). A schedule that falls due in quiet hours with `quiet_hold_schedules` on keeps its `next_run_at`, records one run with outcome `held`, and starts on the first tick after the window ends. Assistant schedule wake-ups follow the same path.
- `POST /{id}/prompt` `{text, attachments?}`: sent now if idle, otherwise appended to the queue. `attachments` are the objects `POST /{id}/attachments` returned (`{sha256, name, media_type, size}`); `text` may be empty when there are attachments (400 `attachment_missing` for one never uploaded to this owner, 400 `too_many_attachments` over 10, 413 `attachments_too_large` over 40 MB together). The `user_message` event payload carries `attachments`, and so does each queued prompt. While the node is not connected the answer is 202 `{queued: true, message}` and the message waits in the queue until the node says hello.
- `POST /{id}/attachments?name=<file name>` with the raw bytes as the body and the media type as `Content-Type` → `{sha256, name, media_type, size}` (201). Stored once per SHA-256 however often it is uploaded; 413 `attachment_too_large` over 20 MB, 400 `attachment_empty`.
- `GET /api/v1/attachments/{sha256}`: the bytes, to the person who uploaded them only (404 otherwise). Images are served inline with their media type, everything else as `application/octet-stream` with `Content-Disposition: attachment`; `Cache-Control: private, max-age=31536000, immutable`, an `ETag` of the hash, `nosniff` and `Content-Security-Policy: sandbox`.
- Artifacts (`docs/ARTIFACTS.md`), the caller's own only (404 for another person's). An artifact is `{id, owner, conversation_id?, kind: design|page|markdown|file, title, slug, public, latest_version, thumbnail_version, url, created_at, updated_at, versions?}`; `thumbnail_version` is the newest version with a snapshot, or null (always null for `markdown` and `file`, and while `MYCELIUM_BROWSER_URL` is unset); `url` is `https://{slug}.{MYCELIUM_ARTIFACT_DOMAIN}` or null, and `versions` (`[{version, author: agent|person, media_type, sha256, note?, created_at}]`, newest first) is carried by the single-artifact answers, not the list. `GET /api/v1/artifacts?kind=&q=` lists newest updated first (`q` matches the title or slug). `POST /api/v1/artifacts` `{kind, title, content | content_base64, media_type?, conversation_id?}` (201): a design without content starts with one empty page, other kinds need content; the slug comes from the title with a short random suffix when taken; a `file` takes `media_type` (default `application/octet-stream`), other kinds have a fixed one (`application/vnd.mycelium.design+json`, `text/html`, `text/markdown`). 400 `kind_invalid`, `title_required`, `title_too_long`, `content_required`, `content_ambiguous`, `content_invalid`, `content_not_text`, `artifact_empty`, `design_invalid`, `media_type_invalid`; 413 `artifact_too_large` over 20 MB. `GET /api/v1/artifacts/{id}`, `PATCH` `{title?, public?}` (the slug never changes), `DELETE` (204; versions, comments and data go with it, the stored bytes stay as attachments). `POST /api/v1/artifacts/{id}/versions` `{content | content_base64, note?}` adds a version by the person and keeps the media type. `GET /api/v1/artifacts/{id}/thumbnail` serves the newest snapshot as `image/png` with an `ETag` and `Cache-Control: private, no-cache` (404 `That snapshot` while there is none); the artifact is published again on the stream when a snapshot finishes. `GET /api/v1/artifacts/{id}/versions/{version}/content` serves the bytes with `X-Content-Type-Options: nosniff`; the `page` kind is `text/html` under `Content-Security-Policy: sandbox allow-scripts`, every other kind uses `sandbox`.
- Artifact comments and data: `GET|POST /api/v1/artifacts/{id}/comments` (`{body, anchor?, author_name?}`, author defaults to the person's name; 400 `comment_required`, `comment_too_long`), `PATCH /api/v1/artifacts/{id}/comments/{comment_id}` `{resolved}`. `GET /api/v1/artifacts/{id}/data` lists `[{key, value, updated_at}]` and `PUT /api/v1/artifacts/{id}/data/{key}` stores any JSON value up to 1 MB (400 `data_key_invalid`, 413 `data_too_large`).
- Internal artifact host API, with `Authorization: Bearer <MYCELIUM_ARTIFACT_HOST_TOKEN>` (404 while no token is configured, 401 without or with a wrong one), seeing every owner's artifacts: `GET /internal/artifacts/by-slug/{slug}` (the artifact with `versions` plus the latest `media_type` and `sha256`), `.../content` (the latest bytes, headers as above), `.../data`, `PUT .../data/{key}` and `POST .../versions?subject=<sub>` (the raw bytes as a new version of a file artefact, saved from the artifact host's PDF viewer; `subject` is the OIDC `sub` the host signed the visitor in as, matched to the user `oidc:<sub>`, and only that artefact's owner may save: 403 `edit_forbidden` otherwise, 400 `artifact_not_file` for other kinds, 413 above 20 MiB; the new version is streamed to the owner like any other).
- `GET /design/` and `/design/{artifact_id}` serve the Design editor single-page app from `MYCELIUM_EDITOR_DIR` to a signed-in browser (the same session cookie and API); artboard HTML renders in `<iframe sandbox="allow-scripts" srcdoc>`, never `allow-same-origin`.
- MCP tools on `/api/v1/mcp`, tied to the calling conversation: `create_artifact {kind, title, content?, path?, media_type?}` (`path` is read from the conversation's folder on its node, 20 MB at most), `update_artifact {artifact, content? | path?, note?}`, `design_set_artboard {artifact, page?, artboard?, name?, x?, y?, w?, h?, html}` (loads the latest design, replaces the named artboard or adds one at the right of the others, 1440 by 900 unless sized, and saves a new version by the agent), `design_remove_item {artifact, item}`, `read_artifact {artifact, artboard?}` and `read_artifact_feedback {artifact}` (unresolved comments with anchors, and the data store). `artifact` is an id or a slug.
- `GET /{id}/queue`, `PATCH /{id}/queue/{qid}` `{text}`, `DELETE /{id}/queue/{qid}`, `POST /{id}/queue/{qid}/send-now`
  (on a node of 0.10.1 or later, injects this one into the running turn with `steer` and does not stop it; when the agent cannot be steered or the node is older, it cancels the current turn and sends this one when the turn ends; when the node does not answer within five seconds, the message stays queued and the turn is left running, and a late `injected` answer still removes it from the queue and records it). When the agent answers `prompt_required` because no turn was running, the message is sent as a normal prompt.
- `POST /{id}/queue/send-all-now` (204, owner only) combines every queued message into one (texts joined by a blank line, attachments concatenated, in queue order) and sends it now, steering or cancelling the current turn like send-now; 404 when the queue is empty. When a turn ends, the whole queue is likewise sent as one prompt.
- `POST /{id}/cancel`.
- `POST /{id}/close` (204, owner only) ends the agent process of a conversation that is `working` or waiting for input, even if the agent is not answering: the node is sent `close_session`, unacknowledged prompts for it are dropped, the conversation becomes `done` (not archived) and is streamed. A conversation that is idle, failed or a chat is left alone and the answer is still 204. 409 `node_offline` while the node is offline. A later message resumes the session from its transcript.
- `POST /{id}/permission` `{request_id, option_id}`.
- `GET /{id}/reviews/{request_id}` `{items}`: the held items of an integration result, 404 once answered. `POST /{id}/review` `{request_id, decision: share|remove|withhold, removed: [index]}`: 204, 409 `already_resolved`, 400 `nothing_to_remove`. A result held for review raises `review_request` (`request_id, server, tool, count`, never the content) with the chat `input`, then `review_resolved` (`request_id, decision, removed_count`). Held content lives only in memory: share returns it, remove returns the kept items with a note, withhold or a cancel tells the agent it was withheld.
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
- Every memory carries `source` (`model`, `agent`, `node_id`, `node_name`, `account`, `project_id`, `project_name`, `message_seq`, `tool_call_id`, and `conversation_title` on the list), stamped when it is written from the conversation's facts and names as they were then. `tool_call_id` comes from the MCP call's `_meta["claudecode/toolUseId"]`, or from the chat's own tool call id; it is null when the harness sends neither.
- `recall` answers with one line saying the memories are data, then each memory as `<memory id= written= scope= model= agent= node= account= project= conversation= message= tool_call=>text</memory>` (null attributes left out, `"` and `<` replaced in attribute values, `</memory` in the text written `<\/memory`).
- Memory banks: `GET|POST /api/v1/memory-banks` and `PATCH|DELETE /api/v1/memory-banks/{id}` `{name, model_class: "private"|"zdr"|"cloud", group_ids}` (DELETE is 409 `bank_not_empty` while memories remain, 409 `bank_name_taken` on a duplicate name), `PUT /api/v1/memories/{id}/bank` `{bank_id|null}`. `model_class` is the loosest model allowed: a chat uses its provider's `model_class` (`PATCH /model-providers/{id}` `{model_class}`), every harness conversation and a call outside a conversation count as `cloud`. Empty `group_ids` means every group. A banked memory is recalled only by a conversation whose model class and group the bank allows. `remember` takes `bank` (a name) and `recall` lists the banks open to the conversation. The stream carries `memory_bank` and `memory_bank_removed {bank_id}`.
- `GET /api/v1/mcp` (node token): the Streamable HTTP event stream. It sends the JSON-RPC notification `notifications/tools/list_changed` to every open stream of the node's owner when one of their chat tool servers is added, removed, switched on or off, or refreshed. `initialize` advertises `capabilities.tools.listChanged` and answers an `Mcp-Session-Id` header, which the stream does not need. `tools/list` also lists the tools of the owner's enabled chat tool servers as `<server slug>__<tool>`, with the description prefixed by `<server name>: `, leaving out a server that does not answer; `tools/call` of such a name forwards to that server with its stored bearer token and returns its result, or an error result saying what went wrong (a switched-off server says so).
- `POST /api/v1/mcp` (node token): MCP Streamable HTTP with plain JSON replies. Tools `recall {query, limit?}`, `remember {text}`, `propose_schedule {prompt, preset? ("weekdays at 20:00", "daily at 09:00", "every 15 minutes", "every 2 hours"), cron?, each_run? (`same` by default, in the calling conversation, or `new`), time_zone?}`, which stores a proposed schedule on the calling conversation's node, agent and folder and answers an error result saying what to fix for an unknown preset, a cron that never runs or a bad zone, and `walkthrough {steps: [{title, key?, section?, icon? (`logic|tests|docs|config|ui|data|other`, otherwise dropped), explanation?, files?: [{path, diff | hunks}]}]}`, which stores a diff walkthrough for the calling conversation (`x-mycelium-conversation`), replacing the shown one and keeping the older ones, and publishes it as `walkthrough` on the stream. A file's `hunks` are joined with newlines into its `diff`. A call without steps, a step without a title, or a file without a path or diff stores nothing and answers an error result saying what to fix. `get_instructions {layer?}` answers `Layer: <name>`, `Revision: <version id, or none>` and the layer's text; `propose_instructions_edit {text, base_revision, reason, layer?}` stores a proposal with the complete new text. `layer` is `global` or a group name and defaults to the calling conversation's group, else global. A `base_revision` that is not the layer's current revision, an empty text or reason, a text equal to the current one or an unknown layer stores nothing and answers an error result saying what to fix. A call is one waiting proposal per conversation and layer: a newer one supersedes the older. `find_conversations {query?, limit?}` lists the calling conversation's owner's unarchived conversations, most recent first, as `<id>: <title> (project <name or none>, node <name>, agent <agent>, status <status>, last activity <time>)`, at most `limit` (default 10, up to 50); `query` words must each appear in the title, project name or folder. It only reads. `send_to_conversation {conversation (id or exact title), message, reason}` never sends: it stores a waiting message proposal from the calling conversation and answers `Waiting for the person to approve sending this to <title> (proposal <id>).` An empty field, a target that is the calling conversation, an archived one, one that is not the owner's (answered as if it did not exist) or a title shared by several answers an error result and stores nothing.
- `GET /api/v1/conversations/{id}/walkthrough` → the newest walkthrough `{id, conversation_id, steps: [{title, key, section?, icon?, explanation, files: [{path, diff}]}], created_at}` or `null`.
- A walkthrough can carry `checks: [{step, question, choices}]` (the answers stay on the server), written by the MCP `walkthrough` tool's optional `checks` `[{step, question, choices, answer, explanation}]` with `step` numbered from 1; the call stores nothing and says what to fix when a step does not exist, a step has two checks, a check has fewer than 2 or more than 4 choices, or `answer` is not a choice. `GET /api/v1/walkthroughs/{id}/answers` → `[{step, choice, correct}]` of the person's answers; `PUT /api/v1/walkthroughs/{id}/answers/{step}` `{choice}` → `{step, choice, correct, explanation}` and records it, answering again replaces it (400 `no_such_check` for a step without a check, 400 `no_such_choice`, 404 for another person's walkthrough). Skipping records nothing.
- Weekly reflections. `GET /api/v1/reflections` → `[{id, week, done, learned, waiting, next_week, edited, drafted_at?, conversation_id?}]`, newest week first; `week` is the Monday (in the person's time zone) as a date, the four sections are Markdown, and `edited` names the sections the person has changed. `PATCH /api/v1/reflections/{id}` `{done?, learned?, waiting?, next_week?}` saves the text and adds each section whose text changed to `edited` (404 for another person's). `POST /api/v1/reflections/{id}/redraft` → 202 `{conversation_id}` starts a drafting conversation for that week on the reflection schedule's node, agent and folder (409 `reflection_not_set_up` before the schedule exists). `GET /api/v1/reflections/schedule` → `{schedule_id, node_id, agent, cwd, enabled, cron}` or `null`; `POST /api/v1/reflections/schedule` `{node_id, agent, cwd, enabled}` creates the schedule once (preset "fridays at 16:00", a new conversation each run, held by quiet hours like any schedule), then only pauses or resumes it, or replaces it when the node, agent or folder changes. The MCP tool `save_reflection {week?, done?, learned?, waiting?, next_week?}` upserts the week and never overwrites a section listed in `edited`; only a conversation started by the reflection schedule or by a redraft may call it. It streams `wellness_changed`.
- Skills. `GET /api/v1/skills` → `[{kind, name, own_lines, agent_lines, own_share?, weekly_own_lines[8], last_practised?}]` over the last 90 days, `kind` being `language`, `area` (the first two folders of a path) or `other`, hidden skills left out. `agent_lines` counts lines added by agent `diff` tool calls, `own_lines` lines added through `POST /api/v1/conversations/{id}/git/file`; `own_share` is own / (own + agent), a share of changed lines, and `weekly_own_lines` runs oldest week first. The first call per person counts the last 90 days of stored agent diffs once. Commits made outside Mycelium are not counted. `PUT|DELETE /api/v1/skills/hidden` `{kind, name}` → 204 hides or shows a skill (400 `skill_invalid`).
- Exercises. `GET /api/v1/exercises?skill=` → `[{id, kind, name, title, brief, minutes?, state, branch?, conversation_id?, reviewing, not_now_until?, tests?, review?, created_at, started_at?}]`, newest first; `state` is `suggested`, `started`, `reviewed` or `done`, and `review` is `{id, summary, comments[{path, line, body, praise}], tests?, created_at}`. `GET /api/v1/exercises/suggested` → up to three suggested exercises not snoozed and not for a hidden skill. `POST /api/v1/exercises/{id}/start` → 201 adds a worktree on `learn/<slug>` in the exercise's project on its node and a conversation in it (409 `exercise_needs_project` when the source conversation had no project); starting again returns the same exercise. `POST /api/v1/exercises/{id}/not-now` hides it until midnight in the person's zone. `POST /api/v1/exercises/{id}/tests` runs the project's `test_command` (or the node's detected one) in the exercise's worktree and returns `{exit, passed?, failed, failing[], timed_out, output_tail}`, also kept as `tests`. `POST /api/v1/exercises/{id}/review` → 202 asks the exercise's conversation for a review; while `reviewing`, the control plane refuses that conversation's edit, delete and move permission requests. `POST /api/v1/exercises/{id}/done` marks it done. `GET /api/v1/exercises/{id}/solution` → `[{path, diff}]` the source conversation's branch diff. `POST /api/v1/exercises/propose` `{node_id, agent, cwd?, project_id?}` → 202 `{conversation_id}` starts a conversation that proposes exercises. The MCP tools `propose_exercise {title, brief, skill_name, skill_kind?, source_conversation_id?, minutes?}` and `save_exercise_review {summary, comments?}` (only while the exercise is being reviewed) stream `wellness_changed`.
- `GET /api/v1/walkthroughs/{id}/reads` → the step numbers (from 0) the person has read; `PUT /api/v1/walkthroughs/{id}/reads/{step}` (204) marks one, repeating it changes nothing (400 `no_such_step` past the last, 404 for another person's walkthrough). Stored per person in `walkthrough_reads`.

Forges:
- `GET /api/v1/forges` → `[{id, kind: "github"|"gitlab", base_url, account, token_hint, organisations, created_at}]`; `POST /api/v1/forges` `{kind, base_url?, token}` → the forge (201). `base_url` defaults to `https://github.com` or `https://gitlab.com`. The token is checked against the forge's user endpoint first: 400 `forge_token_rejected`, `forge_unreachable`, `forge_failed`, `base_url_invalid` or `token_required`. Connecting an account again replaces its token. `POST` also takes `add_mcp?` (default true): a GitLab forge gets an `mcp` integration named "GitLab" (or "GitLab <host>" if taken) at `<base_url>/api/v4/mcp`, and github.com gets "GitHub" at `https://api.githubcopilot.com/mcp/`, both using the forge token and linked by `forge_id` (GitHub Enterprise Server has none). The forge returns `mcp_server_id?`; when the MCP server refuses the token the forge still connects and `mcp_error` says to use a fine-grained token with the MCP permission or sign in (`POST /chat-tool-servers/oauth` takes `forge_id` to link the signed-in server). Connecting again updates the linked server's token. `DELETE /api/v1/forges/{id}` disconnects and removes the linked integration. Forges belong to the person who connected them.
- `GET /api/v1/repositories?forge=&organisation=&q=` → `{repositories: [{forge_id, owner, name, full_name, description, private, default_branch}], organisations: [{forge_id, name}], failures: [{forge_id, message}]}`: the repositories of every connected forge (up to 500 each), fetched live; a forge that fails is reported in `failures` and does not hide the others.

Projects:
- `GET /api/v1/projects` → `[{id, node_id, name, path, forge_id, repository, created_at}]`, the caller's own. `POST /api/v1/projects` `{node_id, forge_id, repository: "owner/name"}` clones the repository on the node into `<projects_root>/projects/<owner>/<name>` with the forge's token; `{node_id, path}` registers an existing folder inside the node's projects root. 201 with the project; 409 `node_offline`, `git_failed` (git's own message) or `project_exists`; 400 `repository_invalid` or `project_source_required`.
- `POST /api/v1/conversations` also takes `project_id` (instead of `cwd`): the conversation runs in the project's folder; the node must be the project's (400 `project_not_on_node`).
- `POST /api/v1/conversations` with `project_id` and `branch` first has the node create a git worktree on that new branch (`git worktree add -b`), in `<project>-worktrees/<branch with / as ->` beside the project, and the conversation runs there (cwd is the worktree). 400 `branch_required` or `project_required`; 409 `git_failed` with git's message (branch exists, not a valid name) and no conversation is created.
- `GET|PUT|PATCH|DELETE /api/v1/conversations/{id}/goal`: a conversation's goal, `{conversation_id, text, token_budget?, time_limit_seconds?, five_hour_limit?, weekly_limit?, tokens_used, tokens_known, turns, turn_active, started_at, five_hour_used?, weekly_used?, state, reason?}` or `null`. `PUT {text, token_budget?, time_limit_seconds?, five_hour_limit?, weekly_limit?}` sets it; only the text is required, each limit is optional (400 `text_required`, `budget_invalid`, `time_limit_invalid`, `five_hour_limit_invalid` and `weekly_limit_invalid` for a percent outside 1 to 100, `goal_not_allowed` on a side chat). `five_hour_used` and `weekly_used` are the percent used of the conversation's account windows (`five_hour`, `seven_day`), absent while unknown and starts it over; on an idle conversation it sends `Work towards this goal: <text>`. `PATCH {paused}` pauses or resumes (409 `goal_ended` once `met`, `stuck` or `over_budget`); resuming an idle conversation runs a check at once. `DELETE` clears it (204). `state` is `active`, `paused`, `met`, `stuck` or `over_budget`; `reason` is the check's reason, or why it ended or paused. `tokens_used` sums input, output and cache tokens from the usage records of the conversation and its checks since the goal was set, and `turns` counts the conversation's turns. Streamed as `goal` and `goal_cleared`. While a goal is `active`, each turn that ends starts a check: a side chat (`side_kind` `check`, never listed, archived once it answers) on the same node, folder, agent and account, asked to answer only `{"verdict": "met"|"continue"|"waiting"|"stuck", "reason": "..."}`; the first JSON object in its answer is read, and anything else counts as `stuck`. `met` and `stuck` end the goal. `continue` first checks every limit set: it ends the goal as `over_budget` when the token budget is used up, or as `limit_reached` when the time limit since `started_at` has passed or the account's 5-hour or weekly window has reached its percent (an unknown window never stops it); otherwise it sends `Continue towards the goal: <text>` and the reason. `waiting` (the agent has started work that will resume it on its own, such as background commands, subagents or a pipeline) records the reason and sends nothing; the goal stays active and the next turn end, usually the background work reporting back, starts another check. A message from the person is delivered as usual and does not pause the goal; a check that answers while the person's turn is running is dropped and the check after that turn decides. Stopping a turn pauses an active goal; a failed agent or check session ends it as `stuck`.
- `GET /api/v1/conversations/{id}/move-plan?node_id=` → `{movable, reason?, branch?, changed_files, project_on_target, clones_project, agent_session, stays_ports: [port], stays_processes: [name]}`: what a move to that node would carry and what stays. Not movable (with `reason`) when the folder's project has no forge repository or the source node cannot name a branch (detached HEAD, no commits). `project_on_target` is whether the caller has a project with the same forge and repository on the target; `clones_project` says a move would clone it first. Ports and processes are the source node's latest resource sample whose `conversation_id` or `cwd` is this conversation's folder (the agent's own processes excluded); the call asks the node to sample and waits up to 3 seconds when it holds none. 409 `same_node`, `node_draining`, `node_offline`, `agent_unavailable` or `account_not_signed_in`; 404 for an unknown node.
- MCP `read_earlier_turns {query?, before?, limit?}` (behind Continue elsewhere): this conversation's earlier user and agent messages, oldest first as `#{seq} User|Agent: text` (each cut at 4,000 characters), 10 by default and at most 50. `query` searches them; `before` is a `#` number from the last answer and pages back, and the answer ends with `Call again with before=N for older turns.` while more exist. It reads only the calling conversation, including the turns from before it moved to this model or account.
- `POST /api/v1/conversations/{id}/continue` `{agent, account?, use_default_account?, provider_id?, model?, node_id?, cwd?}` → 200 `Conversation`. The conversation keeps its id and thread; only the session behind it changes, marked by a `continued` event. The same harness on another account (`kind: "account"`): the node's session is closed, the next message starts it again with `resume` under the new account (the node copies the transcript into that account first). Another harness or a chat model (`kind: "handoff"`): the old session id moves to `previous_session_ids` (so it is not offered for import), and the first message to the new session is prefixed with a handoff built from the stored thread (what it continues from, title, folder, goal, the original request, files touched, the last 8 messages verbatim and a pointer to `read_earlier_turns`); the stored message and the thread show only what the person typed. A chat target replays only the events after the marker, preceded by the same handoff. A chat continued on a harness without `node_id`/`cwd` goes back to the node and folder of its latest harness. `continued` payload: `{kind, agent, account, provider_id, provider_name, model, node_id, node_name, cwd, from: {agent, account, provider_id, provider_name, model, node_id, node_name, cwd, project_id}, reason}` (`node_id`/`node_name` only when the node changed). 400 `model_required`, `folder_required`, `already_a_chat`, `agent_unknown`; 409 `conversation_busy` (`working` or `input`), `same_target`, `account_not_signed_in`, `agent_unavailable`, or `node_outdated` (an account change on a node older than v0.11.0).
- `POST /api/v1/conversations/{id}/move` `{node_id, fork?: false}` → 202 `{conversation_id, node_id, fork, state, message?, result_id?}`, then `GET` the same address for the latest progress (`null` before any move; held in the control plane's memory) and `move` on the stream. A busy conversation (`working` or `input`) answers `waiting` and moves as soon as its turn ends; an idle one answers `moving`. `state` is `waiting`, `moving`, `done` (`result_id` is the conversation that now runs on the target: the same one for a move, the new one for a fork), `failed` (`message` says what failed, that nothing changed and how to retry) or `cancelled`. `DELETE` the same address cancels a `waiting` move (204; 409 `move_not_waiting` otherwise). 409 `move_in_progress` while one is waiting or moving. While moving, queued messages are held and sent afterwards. A move: the source node snapshots (`git` `snapshot`: branch, head commit, a binary patch of the working tree including untracked files taken through a scratch index so the folder and its staging are untouched, and a bundle of commits no remote has), the control plane clones the project on the target if the caller has none there (`projects/<owner>/<name>`, registered as a project), the target node `apply`s it (refuses when its folder has uncommitted changes or its branch holds commits the snapshot lacks; checks out the branch at the head commit, applies the patch, and puts its previous branch back if the patch fails), the control plane copies the agent's transcript from its mirror to the target (`restore_transcript` chunks of 192 KB, into `.claude/projects/<target folder with every non-alphanumeric as ->/<session>.jsonl` for Claude Code, the same path for Codex), tells the target where the journal numbering continues (`continue_sequence`), updates the conversation's node, folder and project, and sends `start_session` with `resume`. When the target reports the session started the source's session is closed; when it reports failure the conversation row and events are put back as they were and the target's session is closed. A fork does the same but inserts a new conversation (hidden until its session starts, titled `<title> (fork)`) and starts it with `fork` set for Claude Code. A snapshot over 8 MB is refused. A worktree folder moves into the target project's own folder; a folder inside the project keeps its relative path.
- `GET /api/v1/tasks` → the caller's backlog tasks that are not started yet, `[{id, node_id, agent, cwd, project_id, branch, title, prompt, created_at}]`. `POST /api/v1/tasks` `{node_id, agent, title, prompt?, project_id?, branch?, cwd?}` adds one (201; 400 `title_required`, `cwd_required`, `project_required` or `project_not_on_node`). `POST /api/v1/tasks/{id}/start` starts a conversation the way `POST /conversations` does, titled with the task, whose prompt is the task's prompt or else its title (201 with the conversation; the task leaves the backlog, and stays there when the start is refused, for example `node_offline`). `DELETE /api/v1/tasks/{id}` removes an unstarted task.
- `GET /api/v1/schedules` → `[{id, node_id, agent, cwd, project_id, prompt, cron, each_run, paused, time_zone, proposed, conversation_id, next_run_at, last_run_at, last_conversation_id, created_at}]`. `POST /api/v1/schedules` `{node_id, agent, prompt, cron, each_run: "new"|"same", project_id?, cwd?, time_zone?}` (201; 400 `prompt_required`, `cron_invalid`, `cron_never_runs`, `cwd_required`, `project_not_on_node`, `time_zone_invalid`). `cron` is five fields, evaluated in `time_zone` (an IANA name; without one, the person's own, else UTC). `PUT /api/v1/me/time-zone` `{time_zone}` (204; 400 `time_zone_invalid`) stores the person's zone. A proposed schedule (`proposed` true, from the `propose_schedule` tool) never runs: `POST /api/v1/schedules/{id}/confirm` `{cron?, each_run?, time_zone?}` accepts it with optional edits and returns it (409 `schedule_not_proposed` when already accepted); `POST /api/v1/schedules/{id}/decline` (204) deletes it and queues a message on its conversation saying it was declined; run and pause answer 409 `schedule_proposed`. Both publish: `schedule` (the schedule) and `schedule_removed` (`{schedule_id}`). `PATCH /api/v1/schedules/{id}` `{paused}` pauses it, or resumes it with the next run counted from now. `POST /api/v1/schedules/{id}/run` runs it now without moving `next_run_at` (201 with the run). `DELETE /api/v1/schedules/{id}`. `GET /api/v1/schedule-runs` → the caller's 50 latest runs, `[{id, schedule_id, ran_at, outcome, reason, conversation_id}]`.
- `GET /api/v1/worktrees` → `[{id, project_id, branch, path, issue_number, created_at}]` for the caller's projects.

Groups and instructions:
- `GET /api/v1/groups` → the caller's groups `[{id, name, folders: [{id, node_id?, prefix}], created_at}]`. `POST /api/v1/groups` `{name}` (201; 400 `name_required` or `name_invalid`, which allows up to 40 letters, numbers, dots, dashes and underscores; 409 `group_exists`). `PATCH /api/v1/groups/{id}` `{name}` renames. `DELETE /api/v1/groups/{id}` leaves its projects ungrouped; memories stored in its scope stay in the list. `POST /api/v1/groups/{id}/folders` `{prefix, node_id?}` makes the group claim a folder, on one node or on any (201 `{id, node_id, prefix}`; 400 `folder_invalid` unless it is a full path below the root with no `.` or `..`; 404 for an unknown node; 409 `folder_claimed` when a group already claims it on that node). `DELETE /api/v1/groups/{id}/folders/{folder_id}` removes the claim (204).
- `GET /api/v1/assistants?archived=` lists the caller's assistants `[{id, name, description?, group_id?, preferences: {models: [{agent?, provider_id?, model?, account?}], nodes, chat_first, fall_back_on}, conversation_id?, state: "working"|"input"|"idle", last_active_at?, session_count, pinned_at?, archived_at?, created_at}]`; `description` falls back to the first line of `AGENTS.md`. `POST /api/v1/assistants` `{name, description?, group_id?, instructions?, preferences?}` (201) creates one and writes `AGENTS.md` from `instructions`; 400 `name_required`, `name_invalid` (over 60 characters), `provider_unknown`, `model_required`, `node_unknown`, `fall_back_invalid`; 404 for another person's group; 409 `name_taken`. `GET /api/v1/assistants/{id}` adds `instructions`. `PATCH /api/v1/assistants/{id}` `{name?, description?, group_id?, clear_group?, instructions?, pinned?, archived?, preferences?}` changes it (an empty `description` goes back to the `AGENTS.md` line). Streamed as `assistant`.
- `POST /api/v1/assistants/{id}/prompt` `{text, attachments?}` → the assistant's session `Conversation`. With a current session (`assistants.conversation_id`) the message is sent to it, or queued while a reply is running; without one, a chat starts on the first chat model in `preferences.models` (400 `model_required` "Choose a model for this assistant in its settings, then send your message again." when there is none; 400 `text_required`) and becomes the current session. A session is a `chat` conversation carrying `assistant_id`; later sessions keep the model they started on, so a changed preference applies from the next session. `POST /api/v1/assistants/{id}/clear` stops a running reply, empties `conversation_id` and answers the assistant; the earlier sessions stay as conversations with `assistant_id`. A session's `group_id` is its assistant's group when it has one, and its instructions are the global layer, the group layer and the assistant's `AGENTS.md`, in that order, after the chat instructions.
- `GET /api/v1/assistants/{id}/files` → the assistant's storage `[{path, size, updated_at}]`, `AGENTS.md` included. `GET|PUT|DELETE /api/v1/assistants/{id}/files/{path}`: `GET` answers the bytes (`text/plain` when they are UTF-8), `PUT` takes the bytes as the body and answers the file, `DELETE` answers 204 (404 for a file that does not exist). A path has no leading or trailing slash, no empty segment, no `..` and at most 200 characters (400 `path_invalid`); a file is at most 1 MiB (413 `file_too_large`) and storage at most 25 MiB (413 `storage_full`). Writing `AGENTS.md` changes the assistant's instructions and, when it has no description of its own, its description. MCP tools `assistant_list_files {prefix?}`, `assistant_read_file {path}`, `assistant_write_file {path, content}` and `assistant_delete_file {path}` do the same from the assistant's own chat; elsewhere they answer "This conversation does not belong to an assistant.", and in a harness session "Use the files in your working folder; they are copied back after this turn."
- An assistant's first prompt picks its session from `preferences`: with `chat_first` the first chat model, otherwise the first model choice that is not limited, a harness choice needing an online node of the assistant's `nodes` (any online node when empty) that has the agent and, when named, the account signed in; none workable answers 409 `no_choice_available`. A harness session runs in `.mycelium-assistants/{id}` under the node's projects root: before it starts the control plane sends `write_folder` (storage, replacing the folder) and after every turn `read_folder`, and the node's `folder_files` reply updates storage (changed files stored, missing files deleted unless the node reported `truncated`). The model of a harness choice is set when its session starts. MCP tool `assistant_use_workspace {reason?}`, from an assistant's chat only, moves the same conversation to the first workable node when the reply ends (a `continued` event with `node_id` and `node_name`, shown as "Moved to {node}"), with storage copied into the folder first.
- Fallback: when a harness session hits a usage limit, or a chat model fails, the thread moves to the next workable choice through `continue_on` with `reason` `usage_limit` or `model_error` (the failing choice is recorded in `assistants.limited` until the limit resets, 10 minutes for a model error), and a prompt to a session whose node is offline or account signed out moves first (`offline`, `signed_out`). Only the reasons in `preferences.fall_back_on` apply. `GET /api/v1/assistants/{id}` answers `next_choice`, the label of the choice the next session would start on. A chat moving to another chat model records a `continued` event of kind `model` and keeps its whole history.
- `GET /api/v1/assistants/{id}/secrets` → `[{name, description?, state: set|waiting, set_by?}]`; `PUT /api/v1/assistants/{id}/secrets/{name}` `{value?, description?}` sets the value (a request without `value` creates or describes a waiting secret) and answers the secret; `DELETE` answers 204. Names are environment variable names (400 `secret_name_invalid`). Values are never returned: harness sessions receive them as `env` on `start_session`, and the node also keeps them in a mode-600 file per session (`KEY='value'` lines, single-quoted so the file can be sourced) whose path is the session's `MYCELIUM_SECRETS_FILE`. Setting or deleting a value sends `secrets_changed {conversation_id, env}` to the node of every running working-folder session of the assistant, and the node rewrites the file at once; the session's own environment stays as it started, so the next queued message restarts the session first (a `start_session` with the current `env` and `resume`; a running node session whose `env` differs is replaced), never mid-turn. The restart marker is in the control plane's memory. Every value of four characters or more is replaced by `[redacted]` in events as they are stored and in the events already stored when a value is set. MCP tools `assistant_set_secret {name, value, description?}` and `assistant_request_secret {name, description?}` do the same from the assistant's own conversation. Secrets are plain columns, as provider keys are.
- `PUT /api/v1/assistants/{id}/backup` `{forge_id, repository, branch?}` backs the assistant's storage up to a repository on one of the person's forges (the repository must be readable, else the forge's error; the default branch when `branch` is empty) and answers the assistant; `DELETE` stops it (204); `POST /api/v1/assistants/{id}/backup/pull` replaces storage with the repository tree (files matched by the stored `.gitignore` are left alone) and answers the assistant. After every workspace turn, chat file change and file edit the changed files are committed one per commit and deleted files removed; `.gitignore` matches stay out. The assistant carries `backup` (`last_at`, `last_commit`, `error`; a failure is stored there, never thrown) and each listed file `backed_up`. Only one backup runs per assistant at a time; a change made while one runs is picked up by the next.
- A schedule proposed from an assistant's conversation carries `assistant_id` (its `agent`, `node_id` and `cwd` are placeholders and ignored); when it runs it prompts the assistant's current session, or starts one as a first prompt does, whatever `each_run` says. The assistant shows `wakes`, a description of the cron of its next confirmed, unpaused schedule (for example "Weekdays at 08:00").
- `GET /api/v1/groups/{id}/instructions` answers that group's instruction layer `{layer, name, text, version, path}` and `PUT` with `{text}` saves it exactly as `PUT /api/v1/instructions/{layer}` does (404 for another person's group).
- `PATCH /api/v1/projects/{id}` `{group_id}` puts a project in one of the caller's groups, or takes it out when `group_id` is null (404 for another person's project or group). Projects carry `group_id`. The same route sets the project's test command: `{test_command}` is the command exercises and Run tests use in a worktree of the project, and an empty string clears it so the node's detected command is used. The same route sets the project's icon: `{icon}` names one from the client's set (lowercase letters, digits and dashes, up to 40; 400 `icon_invalid`) and `{clear_icon: true}` removes it; a request with either leaves the group alone, and either one removes the uploaded logo. `POST /api/v1/projects/{id}/avatar` takes the image as the body with its `Content-Type` (PNG, JPEG or WebP, at most 256 KB; 400 `avatar_not_image`, `avatar_empty`; 413 `avatar_too_large`), stores it as an attachment, sets `avatar_sha256` (and clears `icon`) and answers the project; `GET /api/v1/attachments/{sha256}` serves it to its owner. A project with no icon and no logo gets one found in its repository: on `POST /projects` in the background, and on `POST /api/v1/projects/{id}/find-icon` (answers the project; 404 for another person's), the control plane asks the node (`GitCommand::FindLogo`) for the first image among `logo`, `icon`, `favicon`, `logo-icon` and `app-icon` (png, webp, jpg, jpeg or svg) in the root, `assets`, `public`, `static`, `site`, `docs`, `.github` and `images`, then Android launcher icons, then the first local image in the first 40 lines of `README.md`; files over 256 KB or outside the folder are skipped, and an SVG is rendered to a 256 px PNG with resvg 0.49 (Apache-2.0 or MIT). It is stored as an attachment with `avatar_automatic` true, so a later find replaces it but never a person's chosen icon or uploaded logo; nothing is done while the node is offline.
- `GET /api/v1/instructions` → `{source: {forge_id, repository, path} | null, revision, layers: [{layer, name, path, text, version: {id, created_at} | null}]}`. `layer` is `global` or a group id; `revision` is the id of the newest version of any layer. `path` is the layer's file in the source repository, null without a source.
- `PUT /api/v1/instructions/{layer}` `{text}` saves the layer as a new version and pushes to every node; saving the text the layer already holds changes nothing. Without a source the version id is a 40-character hash held in Postgres only. With a source the save is a commit through the forge's contents API (GitHub `PUT /repos/{r}/contents/{path}`, GitLab `/projects/{id}/repository/files/{path}`) and the version id is the commit SHA; a file that changed on the forge since it was read answers 409 `forge_file_changed` and nothing is saved.
- `POST /api/v1/instructions/pull` reads every layer's file from the source and records a version, with the commit SHA as its id, for each that differs from the latest (409 `no_source`). A file the repository does not have leaves the layer as it was.
- `GET /api/v1/instruction-proposals` → the caller's proposed edits, waiting ones first then the newest decided, at most 100: `[{id, conversation_id, conversation_title, layer, layer_name, base_revision, base_text, text, reason, state: "proposed"|"approved"|"declined"|"superseded", note, out_of_date, created_at, decided_at}]`. `base_revision` is the layer's version id when the agent read it, empty for a layer with no version; `out_of_date` is true while the proposal waits and the layer's newest version is not its base (any save or pull of the layer changes it, and republishes the waiting proposals of that layer).
- `POST /api/v1/instruction-proposals/{id}/approve` saves `text` to the layer exactly as `PUT /instructions/{layer}` does (same version, same push to every node, same commit through the source when one is connected) and returns the proposal (409 `proposal_not_proposed` once decided, 409 `proposal_out_of_date`, and the save's own errors such as `forge_file_changed` leave it waiting). `POST /api/v1/instruction-proposals/{id}/decline` `{note?, redo?}` marks it declined and queues a message on its conversation: that the edit was declined with the note, or with `redo` that the instructions changed and the agent should read them again and propose again. Both publish `instruction_proposal` (the proposal); a new proposal also publishes `superseded` for the earlier waiting ones from the same conversation and layer.
- `GET /api/v1/message-proposals` → the caller's message proposals, waiting ones first then the newest decided, at most 100: `[{id, conversation_id, target_id, target_title, target_node, target_project, message, reason, state: "proposed"|"sent"|"declined", created_at, decided_at}]`. The calling conversation shows each as a card.
- `POST /api/v1/message-proposals/{id}/approve` queues `From <source title>:`, a blank line and the message on the target conversation exactly as `POST /conversations/{id}/prompt` would, and returns the proposal as `sent` (409 `proposal_not_proposed` once decided, 409 `target_archived` when the target was archived meanwhile). `POST /api/v1/message-proposals/{id}/decline` marks it declined, queues nothing on the target and queues a message on the calling conversation saying it was declined. Both publish `message_proposal` (the proposal); a new proposal publishes it too.
- `start_subagent` and `start_workflow` (MCP) create nothing: they record a start proposal on the calling conversation and reply that they are waiting for the person (an error when the call comes from no conversation, or when a node or account name matches nothing). Steps may name `node` and `account` by name or id; several matches, or several signed-in accounts, become choices on the card. `list_places` (MCP, read only) lists nodes with `online`, agents, signed-in accounts, projects and chat models.
- `GET /api/v1/start-proposals` → `[{id, conversation_id, kind: "subagent"|"workflow", title, steps: [{key, title, depends_on, gate, node: {id, name, online}, folder, agent, account, model, first_message, node_choices, account_choices}], state: "proposed"|"started"|"declined", connect, workflow_id, created_at, decided_at}]`. `POST /api/v1/start-proposals/{id}/approve` (body `{choices?: {step_key: {node_id?, account?}}}`) creates the workflow and returns the proposal as `started` (409 `proposal_not_proposed` once decided, 409 `choice_needed` while a choice is open). `POST /api/v1/start-proposals/{id}/connect` needs full access, approves and connects the calling conversation to every step conversation. `POST /api/v1/start-proposals/{id}/decline` starts nothing and queues "I declined starting <title>" on the calling conversation. All publish `start_proposal`.
- `GET /api/v1/api-access-requests` → the caller's requests for temporary API access, waiting ones first then the newest decided, at most 100: `[{id, conversation_id, scopes, hours, reason, state: "requested"|"approved"|"declined", created_at, decided_at}]`. The MCP tool `request_api_access {scopes, hours?, reason}` (scopes from `conversations:read`, `conversations:write`, `nodes:read`, `settings:read`; hours 1 to 24, default 4) records one for the calling conversation and replies `Waiting for the person to approve API access (request <id>).`; it publishes `api_access_request` (the request).
- `POST /api/v1/api-access-requests/{id}/approve` `{scopes?}` mints a client token limited to the requested scopes, or to the ticked subset when `scopes` is given (400 `scopes_required` when none is left), expiring after the requested hours, sends it to the conversation's node as `api_token` and publishes the request as `approved`. 409 `request_not_requested` once decided, 409 `no_node` for a conversation without a node, 409 `node_offline` (the token is revoked and the request stays open). When the node answers `api_token_saved`, the control plane queues a message on the conversation with the file path, the scopes, the expiry and the API address, never the token; a node that could not save it gets the token revoked and the agent told. `POST /api/v1/api-access-requests/{id}/decline` marks it declined, mints nothing and queues a message saying so. Both publish `api_access_request`.
- `GET /api/v1/ssh-access-requests` → the caller's requests for temporary SSH access to a node, waiting ones first then the newest, at most 100: `[{id, conversation_id, node_id, node_name, minutes, reason, state: "requested"|"approved"|"declined"|"revoked", created_at, decided_at, expires_at}]`. The MCP tool `request_ssh_access {node, minutes?, reason}` (node is a name or id of one of the person's nodes; minutes 5 to 240, default 30) records one for the calling conversation and replies `Waiting for the person to approve SSH access to <node> (request <id>).`; it publishes `ssh_access_request` (the request).
- `POST /api/v1/ssh-access-requests/{id}/approve` `{minutes?}` mints a token (`myc_ssh_…`), expiring after `minutes` (at most what was requested, at least 5; 400 `minutes_out_of_range`), sends it to the conversation's node as `ssh_token` and publishes the request as `approved`. 409 `request_not_requested` once decided, 409 `no_node` for a conversation without a node, 409 `node_offline` (the request stays open). When the node answers `ssh_token_saved`, the control plane queues a message with the file path and the `ssh` command, never the token; a node that could not save it gets the request revoked and the agent told. `POST …/{id}/decline` marks it declined and queues a message saying so. `POST …/{id}/revoke` ends approved access at once, closes its open sessions and queues a message saying so (409 `request_not_approved` otherwise). All publish `ssh_access_request`.
- `GET /api/v1/nodes/{id}/ssh` (WebSocket, `Authorization: Bearer myc_ssh_…`; `{id}` is the node's id or name) relays raw SSH bytes between the agent and the node's in-process SSH server: binary frames carry the bytes both ways, and either side closing ends the session. It also ends when the grant expires, when it is revoked, or when the node goes offline. Refused before the upgrade with 401 (missing, unknown, expired or revoked token), 403 `ssh_wrong_node` (the token covers another node) or 409 `node_offline`. Each session is recorded in `ssh_sessions`.
- A scoped token passes one check per route group, before the handler: `GET` under `/api/v1/conversations`, `/api/v1/attachments`, `/api/v1/search` and `/api/v1/activity/conversations` needs `conversations:read`; any other method under `/api/v1/conversations` needs `conversations:write`, except the terminal and permission routes; `GET /api/v1/nodes` and `/api/v1/worktrees` need `nodes:read`; `GET` of instructions, extensions, usage-limit, projects, groups and memories needs `settings:read`; `/api/v1/me`, `/api/v1/version` and the health routes need no scope. Every other route, including every route added later, needs a full-access token and answers a scoped one with 403 `scope_not_granted`. An expired token answers 401 like a revoked one. Tokens from the device sign-in have no scopes and are not limited or logged.
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

### Experiments

`GET`/`PUT /api/v1/experiments` `{voice}` is per person (migration 0068, columns on `user_settings`, `experiment_voice` from migration 0084). Voice is the only
experiment: it is off by default, Settings, Experiments has one checkbox for it, and while it is off `/api/v1/transcriptions` answers 403
`experiment_disabled` ("Enable the Voice experiment in Settings, Experiments"). Tasks, Workflows, Instructions, Groups, the Details and Subagents
sidebars, Walkthroughs, Move chats, Continue elsewhere, Assistants, Memory banks and Chat across groups graduated in v0.11 and are on for everyone: their
routes and MCP tools are never refused, and `GET` still answers their old fields, all `true`, so older clients keep showing them. `PUT` stores only
`voice` and ignores the rest. The old columns stay in `user_settings` unread. Schedules were never gated.

### Forges

Settings has a Forges section to connect GitHub and GitLab, including self-hosted ones, with a personal access token. Tokens live
in `forges.token` in Postgres, in plain text, and no API response carries one: a forge is returned with `token_hint` (the last
four characters) and the `account` the forge's user endpoint named when the token was saved. The control plane makes every forge
call itself (GitHub `/user`, `/user/orgs`, `/user/repos`; GitLab `/user`, `/groups`, `/projects`; GitHub Enterprise is
`<base_url>/api/v3`, GitLab `<base_url>/api/v4`). Organisations always start with the account itself.

### Chats and model providers

- `GET /api/v1/model-provider-usage` lists the signed-in person's configured OpenRouter keys as `{provider_id, name, read_at?, spend?}`. Spend is USD `{total, daily?, weekly?, monthly?, limit_remaining?, limit?, limit_reset?}`, fetched from OpenRouter `/api/v1/key` at most once per minute after a successful reading. PostgreSQL retains the last successful reading and original timestamp across failures and restarts; keys never reach clients. Usage refreshes while its panel is open. These are per-key totals, not a whole OpenRouter account balance.
- `POST /api/v1/usage/resets` `{agent: "codex", account?, idempotency_key}` spends one banked Codex usage reset through the `codex app-server` on a node that holds the account, and answers `{outcome}` (`reset`, `alreadyRedeemed`, `nothingToReset` or `noCredit`). Reuse the key when retrying so a retry cannot spend two. `400 reset_unsupported` for a non-Codex agent, `409 no_reset` when no signed-in node reports a banked reset, `409 node_offline`, `409 reset_failed` with Codex's message, `504 reset_timeout` after 20 seconds. Agents get no MCP tool for it: it spends something scarce.
- `GET /api/v1/model-providers` → `[{id, kind: "openai_compatible"|"anthropic", name, base_url, key_hint, models, default_model, created_at}]`; `POST` `{kind, name, base_url?, api_key?}` → the provider (201). OpenAI-compatible needs `base_url` (Ollama serves one at `http://<host>:11434/v1`), the key is optional; Anthropic needs `api_key`, and `base_url` defaults to `https://api.anthropic.com`. The models are fetched first (`GET {base_url}/models`, Anthropic `GET /v1/models`), so a bad address or key is refused and nothing is stored: 400 `provider_key_rejected`, `provider_unreachable`, `provider_failed`, `base_url_invalid`, `base_url_required`, `api_key_required`, `name_required`. `default_model` is the first model for OpenAI-compatible, and for Anthropic the first listed of `claude-sonnet-5-5`, `claude-opus-5-5`, `claude-haiku-5-5`. Keys are stored server-side and never returned, only `key_hint`. `PATCH /api/v1/model-providers/{id}` `{name}` renames it (trimmed; 400 `name_required`, 404 for someone else's) and returns the provider. `PUT /api/v1/model-providers/{id}/favourites` `{model, favourite}` marks or unmarks a model as a favourite (idempotent) and returns the provider, whose `favourites` lists them; 400 `model_unknown` when the provider does not list the model, 404 for someone else's. `POST /api/v1/model-providers/{id}/refresh` fetches the models again and keeps the favourites; `DELETE` removes it and its chats keep their history but cannot continue (409 `provider_removed`).
- Voice input (Voice experiment): `POST /api/v1/transcriptions` takes the raw recording as the body (`Content-Type: audio/wav`, up to 25 MB) and returns `{text}` from the first model provider that lists a speech-to-text model (`speech_model` on `GET /model-providers`, an OpenAI-compatible provider with a `whisper` or `transcribe` model). 400 `audio_empty`, 409 `no_speech_provider`, 502 `transcription_failed`.
- `GET /api/v1/chat-tool-servers` → `[{id, name, kind: "mcp"|"litellm"|"zulip"|"email"|"stoat", url, account?, settings?, token_hint, oauth?, enabled, always_allow, tools: [{name, description}], created_at}]`; `POST` `{name, kind?, url, account?, settings?, bearer_token?}` → the server (201): remote MCP servers a chat may use. The server is asked for its tools first (`tools/list`), so a bad address or token is refused with nothing stored: 400 `tool_server_failed` (its message says what to fix), `url_invalid`, `name_required`, `name_taken` (also 409 for a name another server of yours has, and `mycelium`, which is the name of Mycelium's own tools). `PATCH /{id}` `{enabled?, always_allow?, name?}` switches it off, trusts all its tools without asking or renames it (the same `name_required` and `name_taken` rules as `POST`, a name another server of yours has being 409); `POST /{id}/refresh` lists its tools again; `DELETE` removes it. The token is stored server-side, never returned, only `token_hint`. The control plane speaks MCP over Streamable HTTP, revision 2026-07-28 first (every request its own POST with `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` and the per-request `_meta`), and falls back to the `initialize` handshake of 2025-11-25 (with `Mcp-Session-Id`) when the server answers a `400`, `404` or `405` that is not a modern JSON-RPC error. Responses may be JSON or an event stream. stdio servers on nodes are not supported. Built-in integrations are served by the control plane and listed, namespaced, approved and announced (`tools/list_changed`) exactly like a remote server's tools. `kind` `zulip` takes `url` (the Zulip server, for example `https://example.zulipchat.com`), `account` (the account's email address) and `bearer_token` (its Zulip API key, which is the credential Zulip's HTTP basic authentication takes); they are checked with `GET /api/v1/users/me` first (400 `tool_server_failed`, `account_required`, `api_key_required`, `kind_invalid` for any other kind). The email address is returned as `account`, the key never is. `settings` carries the non-secret fields of an integration (a `settings jsonb` column), is returned as stored, and is checked per kind (400 `settings_invalid`). The tools are `list_channels`, `get_messages {channel?, topic?, direct_with?, limit?}`, `search_messages {query, channel?, limit?}` and `send_message {channel, topic | direct_to, content}`; `send_message` asks for approval in chats on every call, even when `always_allow` is on or the person chose Always allow for it earlier. Workspace agents reach the same tools through the MCP endpoint, where the tools that always ask raise the same approval card in the agent's conversation and wait for the person (Allow once or Deny; a decline returns a tool error saying it was declined, and a call that does not come from a conversation is refused). `refresh` on a built-in integration changes nothing.
  Each server is one account of an integration and also carries `status` (`connected`, `disabled`, `expires_soon` within 14 days of `token_expires_at`, `expired`), `token_expires_at?`, `last_used_at?`, `last_error?`, `locked` (Zulip: every tool asks and results are reviewed, whatever the policy says), `groups` (empty means every group; otherwise only conversations in those groups, and never a conversation with no group, see the integration) and `policy`: `{ask: L, review: L}` where `L` is `{all?, read?, write?, tools?: {name: bool}}`, a missing key meaning inherited and the most specific of tool, read or write, then all winning. Every tool lists its `kind` (`read` or `write`, from the MCP `readOnlyHint`, a tool without one counting as write) and whether it `asks`. Built-in write tools always ask. `POST` and `PATCH` take `token_expires_at` (400 `expiry_in_past`: "Choose an expiry date in the future."), `PATCH` also `clear_token_expiry` and `policy`, `POST` also `policy` and `groups`. Result review is stored but not yet enforced.
- Last.fm (`kind` `lastfm`): `url` may be left empty (it defaults to `https://ws.audioscrobbler.com/2.0/`), `account` is the Last.fm username, `bearer_token` is the shared secret and `settings` `{api_key}` is required (400 `settings_invalid`: "Enter your Last.fm API key."); `user.getInfo` verifies them first. The reads `get_recent_tracks`, `get_top_artists`, `get_top_tracks`, `get_now_playing` and `get_track_info` need nothing more. The writes `love_track`, `unlove_track`, `scrobble` and `update_now_playing` always ask and need a session: `POST /api/v1/chat-tool-servers/{id}/lastfm-sign-in` → `{authorize_url}` (`https://www.last.fm/api/auth/` with `api_key` and `cb` = `<public url>/integrations/lastfm/callback?sign_in=<capability>`, valid 10 minutes, held in memory); `GET /integrations/lastfm/callback?sign_in&token` (no session: `sign_in` is the capability) calls `auth.getSession` signed with the shared secret, stores the session key, sends `integrations_changed` and announces the tools; `DELETE /api/v1/chat-tool-servers/{id}/lastfm-sign-in` signs out (204). The list shows `signed_in: true` while a session is stored; the session key is never returned. A write without a session, or one Last.fm refuses with error 9 (revoked, which also clears the stored session), fails with "Sign in to Last.fm in Settings, Integrations to love tracks and scrobble."; reads keep working.
- `PUT /api/v1/chat-tool-servers/{id}/groups` `{groups: [id]}` replaces the groups an integration is available in (400 `group_invalid`: "Choose a group you own."); `[]` means every group. `GET /api/v1/integration-calls?server=&provider=&decision=approved|declined|auto&review=&surface=chat|workspace|assistant&before=` → the newest 100 `[{id, server_id?, server_name, tool, kind, conversation_id?, surface, provider?, decision, review?, is_error, at}]`, one per call to an integration from a chat or a workspace agent, kept 90 days. Result text is never stored.

  `kind` `email` takes `account` (the IMAP and SMTP username), `bearer_token` (the password or an app password, never returned) and `settings` `{imap_host, imap_port?, imap_tls?, smtp_host, smtp_port?, smtp_tls?, from?}` where `*_tls` is `tls` (implicit TLS, the default, ports 993 and 465), `starttls` (143 and 587) `none` is refused (400 `settings_invalid`, "Choose TLS or STARTTLS for IMAP. Mycelium does not send your password unencrypted.", and the same for SMTP); `MYCELIUM_MAIL_EXTRA_CA` names a PEM file of extra trusted certificates, which the tests use for their self-signed fakes; `from` is the sender address when the username is not one; `url` is ignored. It is stored only after IMAP login and SMTP authentication both succeed (400 `tool_server_failed`). IMAP uses `async-imap` 0.12 (MIT or Apache-2.0, over tokio-rustls with the webpki roots), SMTP uses `lettre` 0.11 (MIT, rustls), and messages are parsed with `mail-parser` 0.11 (MIT or Apache-2.0); mail-parser 1.0 needs a newer compiler than the pinned toolchain. Each call opens a connection, does its work and logs out. The tools are `list_folders`, `list_messages {folder?, limit?}` (id, from, subject, date; newest first), `read_message {folder?, id}` (plain text body capped at 20000 characters, recipients, Message-ID, and attachments as name and encoded size from `BODYSTRUCTURE`, never their content), `search_messages {folder?, from?, subject?, text?, since?, limit?}` (IMAP `SEARCH`, `since` as `YYYY-MM-DD`, non-ASCII words sent with `CHARSET UTF-8`) and `send_message {to?, cc?, subject?, body, reply_to_id?, reply_to_folder?}`. A message id is the IMAP UID in its folder. Reading fetches only the preferred plain-text or HTML MIME section, at most 240000 encoded bytes before the character cap. A reply fetches the original's headers, sets `In-Reply-To` and `References`, defaults the subject to `Re:` and the recipient to its `Reply-To` or sender. `send_message` always asks for approval. Sent mail is not copied to a Sent folder.
  Email also exposes `list_accounts` (ids, names and usernames of the caller's enabled Email integrations), `delete_message {folder?, id}` and `move_message {folder?, id, destination_folder, destination_account?}`. Omitting `destination_account` keeps the move in the current integration; otherwise give its id from `list_accounts`. The target must be an enabled Email integration belonging to the caller. Deletes are permanent and require `UIDPLUS` for UID-scoped removal; same-account moves require native `MOVE`. A server without the needed capability is refused after connecting and before any change, rather than using a broad `EXPUNGE`. All three email writes, including `send_message`, always ask for approval in chats despite Always allow, and in agents' conversations too. Cross-account moves transfer the original MIME, including attachments, directly between IMAP connections, with the original internal date and supported flags (`\Recent` and `\Deleted` are excluded); message bytes and credentials are never returned to the model. Messages above 20 MiB, missing sizes or mismatched transfer lengths are refused, with a bounded response budget and a 45-second timeout covering the whole operation, connecting included. The source is removed only after the destination confirms `APPEND`, and a delete counts only once a `UID SEARCH` no longer finds the message; failed source removal reports the retained destination copy and attempts to restore its original deletion flag. A lost connection or timeout can leave a copy, so uncertain outcomes explicitly ask the person to check both folders before retrying. A moved message receives a new destination UID; list that folder again before acting on it.

  `kind` `outline` is a known app: `url` is the Outline address (`https://docs.example.com`), resolved to its MCP endpoint `<origin>/mcp` unless the path already ends in `/mcp`, and `bearer_token` is an Outline API key (400 `api_key_required`: "Enter your Outline API key. Create one in Outline under Settings, API & Apps."; a key Outline refuses is 400 `tool_server_failed`: "Outline refused that API key. Check it was copied whole and has not been revoked."). It is stored as an ordinary `mcp` server with `settings` `{app: "outline"}`, so tools, approvals and refresh work as for any MCP server.

  `kind` `litellm` is a LiteLLM proxy's MCP gateway: `url` is the proxy's address, resolved to `<origin>/mcp/` when it has no path (a path such as `/mcp/dev_group` is kept as typed) and `bearer_token` is the LiteLLM key, required (400 `api_key_required`: "Enter your LiteLLM key."). It is otherwise an ordinary remote MCP server, except that the key is sent as `x-litellm-api-key: Bearer <key>` instead of `Authorization`, because LiteLLM forwards `Authorization` to pass-through servers as the user's own token.

  `POST /api/v1/chat-tool-servers/oauth` `{name, url, kind?: "mcp"|"outline"}` → `{authorize_url}` starts a sign-in to a remote MCP server (or Outline) that supports OAuth, as an alternative to a bearer token. Mycelium discovers the app's authorisation server (the `resource_metadata` of an unauthenticated `initialize`'s 401, else `/.well-known/oauth-protected-resource`, then `/.well-known/oauth-authorization-server` or `openid-configuration`), registers itself there (dynamic client registration with `redirect_uris` `<public url>/integrations/oauth/callback` and no client secret) and returns the authorisation URL (PKCE S256, `state`, `resource`, the app's `scopes_supported`). An app without a `registration_endpoint` is 400 `oauth_unsupported`: "This app does not let Mycelium sign in. Use an API key instead." Pending sign-ins live in memory for 10 minutes. `GET /integrations/oauth/callback?code&state` (no session: `state` is the capability) exchanges the code, lists the tools and stores an ordinary `mcp` server with `oauth: true` in the list, then sends the stream message `integrations_changed` to its owner; an unknown or expired `state` shows "This sign-in has expired. Start it again from Settings, Integrations." and stores nothing. Tokens are never returned. An access token within 60 seconds of expiry is refreshed with the stored refresh token before a chat or agent calls the server; a refused refresh fails that call with "Sign in to <name> again in Settings, Integrations." and disables nothing.

  `kind` `stoat` (a Revolt-compatible chat, API at `https://stoat.chat/api`, the address the client fills in when none is given) takes `url` (the API address), `bearer_token` and `settings` `{token_kind: "user"|"bot"}`, sent as `x-session-token` or `x-bot-token`; they are checked with `GET /users/@me`. The tools are `list_servers` (servers with their channels, and direct and group channels, read from the events connection's `Ready` event because the REST API has no list of servers), `get_messages {channel, limit?}`, `search_messages {channel, query, limit?}` (the API's text search, at most 64 characters) and `send_message {channel, content, reply_to?}`, which always asks for approval. Message times are decoded from the message id.
- `POST /api/v1/chats` `{provider_id, model?, prompt?, parent_id?}` → the chat as a conversation (201), `agent` `chat`, `provider_id` set, `model` defaulting to the provider's `default_model` (400 `model_required`). A `prompt` is sent at once. `parent_id` makes it a side chat of another chat.
- Chats use the conversation endpoints: `prompt` sends a message (409 `chat_busy` while it is replying), `cancel` stops the reply and ends the turn with `stop_reason` `cancelled`, `events`, `PATCH` for title and archive, and `side` starts a linked chat on the same provider and model. A reply streams as `acp_update` events with `agent_message_chunk` content, one per token group the provider sends, then `turn_end` (`end_turn`, `cancelled` or `error`); a provider failure adds an `error` event first. Replies run in the control plane; the whole conversation so far is sent each time, with a system prompt that describes the `mycelium-ui` block. The title is predicted from the first message like a workspace's. Images in a chat go to the provider as OpenAI-compatible `image_url` data URLs or Anthropic `image` blocks, text files are inlined in the message, and earlier turns keep theirs. Chats are searchable by title and contents and are never indexed with ctx. Each turn writes a usage record, with tokens where the provider reports them (OpenAI-compatible: the final `usage` chunk; Anthropic: `message_start` and `message_delta`). Cache reads and writes are recorded separately from uncached input and summed across tool rounds. Current context comes from the latest individual request's prompt size, emitted as `usage_update`; cumulative billing totals are never used as chat context. Anthropic requests mark the final tool definition (or system block without tools) and the final message block for ephemeral caching. OpenRouter requests for `anthropic/` and `google/` models mark the system and final user content parts; other OpenAI-compatible request bodies stay unchanged. A chat that was replying when the control plane stopped is `review` again after the restart.
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
| Artifact created, new version, renamed, made public or private, or deleted (`deleted: true`) | `artifact` (the artifact, with its versions) | owner |
| Artifact comment added or resolved | `artifact_comment` (the comment) | owner |
| API access requested, approved or declined | `api_access_request` (the request, never the token) | owner |
| SSH access requested, approved, declined or revoked | `ssh_access_request` (the request, never the token) | owner |
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
`POST /conversations/{id}/terminal` (the output follows as `terminal_output`), `POST /conversations/{id}/attachments` (the answer names the stored file, and the message that carries it publishes), the git routes `POST /conversations/{id}/git/{stage,unstage,stage-all,commit,fetch,undo,switch}` (the answer is the response, and the git panel reloads its status after each), `PUT /walkthroughs/{id}/reads/{step}` (the page marks the step checked itself), `PUT /artifacts/{id}/data/{key}` and the artifact host's `PUT /internal/artifacts/by-slug/{slug}/data/{key}` (the page that stored the value shows it, and the agent reads it back through `read_artifact_feedback`), the walkthrough notes routes `POST /walkthroughs/{id}/notes`, `DELETE /walkthroughs/{id}/notes/{note_id}` and `POST /walkthroughs/{id}/notes/sent` (the page that made the change shows it; other devices see notes after a reload), `POST /terminals/{id}/input` and
`POST /terminals/{id}/resize` (the terminal answers with output), the chat tool server routes (settings rows no stream message shows, as with model providers), `POST /conversations/{id}/cancel` (the turn ends with `turn_end` from the node), and the forge routes
`PUT /me/time-zone`, and `POST /forges` and `DELETE /forges/{id}` (the Forges settings page replaces its list from the answer), and `POST /projects` (the Projects page adds the answer), `POST /projects/{id}/avatar` and `POST /projects/{id}/find-icon` (the answer is the updated project, as with `PATCH /projects/{id}`), and the task and schedule routes that only change those lists (`POST` and `DELETE /tasks`, `POST`, `PATCH` and `DELETE /schedules`: the Tasks and Schedules pages replace their lists from the answer). `POST /tasks/{id}/start` and `POST /schedules/{id}/run` publish the new conversation as any start does, and a scheduled run does too.

### Events

`events.kind` is one of: `user_message`, `acp_update` (payload: the ACP `session/update` params as
sent), `permission_request` (payload: the ACP request params plus `request_id`), `permission_resolved`,
`turn_end` (`{stop_reason}`), `error` (`{message}`), `workflow` (`{run_id, line}`: `line` is one journal line as a JSON object, or a string when the line is not JSON; workflow events carry no node `seq` and are never deduplicated), `continued` (see `POST /conversations/{id}/continue`).
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
| `steered` | `request_id, outcome, error?` (`injected`: the running turn took the message; `prompt_required`: no turn was running, send it as a prompt; `unsupported`: the agent did not advertise `_meta.steering.supported` at initialize, there is no live session, or it refused) |
| `logins` | `logins: [{agent, account?, method, state, usage_left?, windows?: [{name, used_percent, resets_at?}], usage_read_at?}]` |
| `instructions_held` | `revision, unmanaged: [label]` |
| `api_token_saved` | `conversation_id, path?` (answers `api_token`: the file the token was written to, or no path when it could not be written) |
| `ssh_token_saved` | `request_id, path?` (answers `ssh_token`: the file the token was written to, or no path when it could not be written) |
| `ssh_data` | `session_id, data` (base64; raw SSH bytes for that session) |
| `ssh_closed` | `session_id` (the node ended the session, or did not know it) |
| `login_output` | `login_id, data` (UTF-8 text from the PTY) |
| `login_finished` | `login_id, ok` |
| `terminal_output` | `terminal_id, data` (base64; raw bytes from the PTY) |
| `terminal_exited` | `terminal_id` |
| `transcript` | `path, offset, data` (base64; the bytes at `offset`) |
| `workflow` | `conversation_id, run_id, line` (the journal line as JSON; no `seq`) |
| `resources` | `sample` (see `POST /nodes/{id}/watch`; sent every 3 seconds while a watch lease lasts) |
| `process_stopped` | `request_id, ok, error?` (the answer to `stop_process`) |
| `reset_used` | `request_id, outcome?, error?` (the answer to `use_reset`; the node sends a fresh `logins` first) |
| `pong` | |

Control plane to node:

| type | fields |
|---|---|
| `ack` | `conversation_id, seq` |
| `stop_process` | `request_id, pid, name` (send SIGTERM to the process if it is still called `name` and is stoppable) |
| `use_reset` | `request_id, agent, account?, idempotency_key` (redeem a banked Codex reset for that account through `codex app-server`) |
| `start_session` | `conversation_id, agent, cwd, resume?: acp_session_id, account?, instructions?` |
| `instructions` | `revision, global?` |
| `prompt` | `conversation_id, prompt_id, text, attachments?` (`[{name, media_type, data}]`, `data` base64). The node sends PNG, JPEG, GIF and WebP as ACP `image` blocks when the agent's `promptCapabilities.image` is true, other valid UTF-8 files as embedded `resource` blocks when `embeddedContext` is true, and writes the rest under `<cwd>/.mycelium/attachments/<prompt_id>/` (a `.gitignore` of `*` sits in `.mycelium/`) and names the path in the prompt. The node link accepts messages up to 64 MiB |
| `steer` | `conversation_id, request_id, text, attachments?` (as `prompt`; the node calls the ACP extension `_session/steering` with `idleBehavior` `promptRequired`, which injects the message into the running turn, and answers `steered`. Sent only to nodes of 0.10.1 or later) |
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
| `api_token` | `conversation_id, token` (written mode 600 to `<state dir>/sessions/<conversation>/api-token`, answered with `api_token_saved`) |
| `ssh_token` | `request_id, conversation_id, token` (written mode 600 to `<state dir>/sessions/<conversation>/ssh-token-<request>`, answered with `ssh_token_saved`) |
| `ssh_open` | `session_id` (the node starts an SSH server session, registered before any following `ssh_data`) |
| `ssh_data` | `session_id, data` (base64; raw SSH bytes for that session) |
| `ssh_close` | `session_id` (the node ends the session, no reply) |
| `ping` | |
| `revoked` | the node deletes its credential and exits 0 |

The control plane keeps every sent `prompt` as unacknowledged until `prompt_received` arrives with its `prompt_id` (or the turn ends or the session fails). After a `hello`, it sends again the unacknowledged prompts of sessions the node reports live, and forgets those of sessions it does not. A `session_started` for the `acp_session_id` the conversation already holds sets mode and model only where they are empty. The node ignores a `prompt_id` it has already received (the ids it has written are persisted in the session directory) and answers a repeat with `prompt_received` again. Failures the node emits itself (a refused start, an unreachable keeper) take the next free `seq` for the conversation, tracked in the session directory, never a fixed value.

An answer to a permission request is refused with 409 when the conversation has failed (`session_failed`, retry it first) or is no longer waiting for input (`already_resolved`). A pending send-now is forgotten when its node disconnects.

A node reports one `logins` entry per account: the agent's default home plus one per folder under `<state_dir>/accounts/<agent>/<name>`, each named by the email found in that folder's own config (`.claude.json` for Claude Code, the `auth.json` identity token for Codex), else by the folder name. The node runs an agent as an account by setting `CLAUDE_CONFIG_DIR` or `CODEX_HOME` to that folder, for both `start_session` and `start_login`; `account` on `start_login` signs in to (and creates, if new) that account's folder. Each `logins` report replaces the node's previous list. A Claude account's usage is read every 5 minutes from `GET https://api.anthropic.com/api/oauth/usage` with the bearer token in that account's own `.credentials.json` (best effort): every window the response carries (`five_hour`, `seven_day`, `seven_day_opus`, ...) is reported with its percent used and `resets_at`, and `usage_left` is 100 minus the busiest. The control plane keeps the latest per account and a reading history, and `GET /nodes` adds to each window `rate_per_hour` and `runs_out_at` (the rate since the window last reset, shown only when it would run out before it resets). Codex accounts also report banked resets, `reset_credits: {available_count, credits?: [{id, expires_at?, title?}]}`, read every 5 minutes from `codex app-server` (`account/rateLimits/read`, best effort; `credits` is absent when only the count is known). Codex accounts report no windows: codex-acp 2.2.2 exposes rate limits only inside its own session text, and its `_meta.quota` carries token counts.

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
| `claude` | `claude-agent-acp` (`MYCELIUM_CLAUDE_ACP`) | method `oauth_token`: `claude auth login`, verify against `claude --help` |
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

### SSH

An in-process SSH server (russh) with no listening port, reached only through the link: `ssh_open` starts a session and `ssh_data` carries its bytes both ways. It accepts any session the control plane opens, with no password or key check, because the control plane already checked the grant. Shells and commands run as the node's user from its home directory, with or without a PTY. Port forwarding and subsystems such as sftp are refused.

The host key is one Ed25519 key per node, generated on first use and kept mode 600 at `<state dir>/ssh_host_ed25519_key`, so it is stable across restarts. The agent's `ssh` trusts it on first use into a `known_hosts` file beside the token, which is safe because the bytes only travel over the node's authenticated link.

`mycelium ssh-proxy <node> --token-file <path>` is the `ProxyCommand` the agent uses: it opens the WebSocket above with the token read from the file and copies stdin and stdout to binary frames.

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
`POST git/hunk` applies a selected hunk only while its view fingerprint matches the disk; `POST git/file` reads or saves UTF-8 text with an expected-content guard. These node-local disk operations are silent, like staging.

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
- The release flake is published twice under the artefacts prefix: `mycelium-flake.tar.gz`, which each release overwrites, and `mycelium-flake-<tag>.tar.gz`, which never changes. Pin the versioned URL as the flake input so a lock never points at a file a later release replaced.
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

Workflow creation and control publish workflow snapshots; scheduler transitions publish the same messages. Underlying conversations publish their usual events.

### Approved conversation connections

`POST /api/v1/conversation-connections` accepts `source_id` and `target_id` owned by the signed-in person. The connection grants messaging in both directions. `GET /api/v1/conversation-connections` lists active connections; `DELETE /api/v1/conversation-connections/{id}` revokes one. These changes publish `conversation_connection` snapshots. Only full person access may approve a connection. `POST /api/v1/message-proposals/{id}/connect` sends a proposed message and connects its two conversations. Connected `send_to_conversation` calls queue directly; other calls retain per-message approval. Settings lists connections and offers revocation.

`POST /api/v1/group-connections` accepts `source_group_id` and `target_group_id` (400 `connection_self` for the same group, 404 for a group that is not the person's); `GET` lists active group connections and `DELETE /api/v1/group-connections/{id}` revokes one, publishing `group_connection`. A `send_to_conversation` between two conversations in connected groups queues directly. `POST /api/v1/message-proposals/{id}/connect-groups` sends a proposed message and connects the two conversations' groups (409 `connection_no_group` unless both conversations have different groups). Message proposals carry `source_group` and `target_group`.

`set_project_icon(path)` reads an image inside the calling conversation’s folder on its node and applies it to that conversation’s registered project. It uses the existing avatar validation and storage and republishes affected conversations. PNG, JPEG and WebP are accepted up to 256 KB; SVG must be converted to PNG first.

Goal progress combines completed turn usage with deduplicated token samples from mirrored Claude/Codex transcripts, including managed account folders. Context occupancy is not billed token usage. `turns` counts completed turns; `turn_active` identifies an in-progress turn, and `tokens_known` distinguishes pending reporting from confirmed zero. Agents read this through `get_goal` and account windows through `get_account_usage`. The control plane retains last-successful login usage snapshots with original `usage_read_at`; unknown or older reports do not overwrite them. Sign-in state and account removal stay authoritative. Expired quota windows do not enforce goal limits.

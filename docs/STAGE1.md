# Stage 1: run and steer

The build contract for the first release. `DESIGN.md` says why; this file says exactly what, so the
parts can be built in parallel and still fit. Change this file before changing an interface.

## Scope

In: the control plane, the node daemon and its keeper, Claude Code and Codex over ACP, the
conversation list by status, live threads with queued messages, stop, permissions, mode and model,
per-node agent logins, the transcript copy, live workflow progress for Claude Code, rename and
archive, a web client and a desktop client, install by script, Nix or NixOS.

Out until later stages: moving conversations, the git hub, tmux sessions, managed AGENTS.md,
groups, memory, search, iOS and iPad, goals, schedules, tickets, walkthroughs.

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
web_sessions     id uuid pk, user_id, token_hash bytea unique, expires_at, created_at
device_codes     id uuid pk, kind text ('node'|'client'), device_code_hash bytea unique, user_code text unique,
                 label text, hostname text, os text, arch text, ip text, expires_at,
                 approved_by uuid null, approved_at null, result_id uuid null, consumed_at null
nodes            id uuid pk, name text unique, hostname, os, arch, version text, token_hash bytea unique,
                 created_by uuid, created_at, last_seen_at, revoked_at null, draining bool default false
client_tokens    id uuid pk, user_id, label text, token_hash bytea unique, created_at, last_used_at, revoked_at null
agents           node_id, agent text ('claude'|'codex'), version text, available bool, pk(node_id, agent)
logins           id uuid pk, node_id, agent, account text null, method text, state text
                 ('signed_in'|'signed_out'|'expired'|'unknown'), updated_at
conversations    id uuid pk, owner uuid, node_id, agent, cwd text, title text, status text, acp_session_id text null,
                 mode text null, model text null, created_at, updated_at, archived_at null
events           id bigserial pk, conversation_id, seq bigint, kind text, payload jsonb, created_at,
                 unique(conversation_id, seq)
queued_prompts   id uuid pk, conversation_id, position int, text text, created_at
transcripts      node_id, path text, bytes bigint, updated_at, pk(node_id, path)
```

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
- `GET /api/v1/nodes` → nodes with `online` (linked and past its `hello`), `draining`, agents and logins.
- `POST /api/v1/nodes/{id}/drain` `{draining}`, `DELETE /api/v1/nodes/{id}` revokes.
- `POST /api/v1/nodes/{id}/logins` `{agent, method}` → `{login_id}`; starts the agent's own sign-in on
  the node. Output streams as `login_output`; `POST /api/v1/logins/{login_id}/input` `{text}` writes to
  it; `DELETE /api/v1/logins/{login_id}` stops it.

Conversations:
- `GET /api/v1/conversations?archived=false`.
- `POST /api/v1/conversations` `{node_id, agent, cwd, title?, prompt?}` → the conversation; the node
  starts the ACP session, and `prompt`, if given, is the first message.
- `GET /api/v1/conversations/{id}`, `PATCH` `{title?, archived?}`, `POST /{id}/opened` marks review as done.
- `GET /api/v1/conversations/{id}/events?after=<seq>&limit=500`.
- `POST /{id}/prompt` `{text}`: sent now if idle, otherwise appended to the queue.
- `GET /{id}/queue`, `PATCH /{id}/queue/{qid}` `{text}`, `DELETE /{id}/queue/{qid}`, `POST /{id}/queue/{qid}/send-now`
  (cancels the current turn, then sends this one).
- `POST /{id}/cancel`.
- `POST /{id}/permission` `{request_id, option_id}`.
- `POST /{id}/config` `{mode?, model?, config?: {id: value}}` → ACP `session/set_mode`, `session/set_model`
  or `session/set_config_option`, whichever the agent advertises.

Live:
- `GET /api/v1/stream` (WebSocket, cookie or client token). The server sends one JSON object per
  message: `conversation` (the row), `event` (`{conversation_id, seq, kind, payload}`), `queue`,
  `node`, `node_removed` (`{node_id}`, sent when a node is revoked), `login_output` (`{login_id, data}`), `login_finished` (`{login_id, ok}`; `ok` is false when the sign-in is stopped, or its node disconnects or is revoked), `device` (`{user_code, kind: "node"|"client", label, hostname, state: "pending"|"approved"|"denied"|"expired"|"consumed", result_id: null|uuid}`, sent to everyone signed in whenever a device code is created, approved, denied, expires or is exchanged; `result_id` is the node or client token id once consumed). A node is published with `node` (`online` false) as soon as its credential is issued at token exchange, before the daemon first connects. Clients drop the node on `node_removed`, and reconnect and refetch. A request that carries an `Origin` header must match `MYCELIUM_PUBLIC_URL`'s origin, otherwise 403 `origin_not_allowed`; clients that send none (desktop, tests) are accepted.

Node link:
- `GET /api/v1/node/connect` (WebSocket, node token). Frames below.

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
| Sign-in output | `login_output` | starter |
| Sign-in ends: node reports it, `DELETE /logins/{id}`, node disconnects or is revoked | `login_finished` | starter |
| Conversation created, renamed, archived or restored | `conversation` (and `queue` on create) | owner |
| `session_started` (session id, mode, model) | `conversation` | owner |
| Mode or model changed | `conversation` | owner |
| Prompt sent from the queue | `event` user_message, `conversation` working, `queue` | owner |
| `acp_update`, `workflow` | `event` | owner |
| `permission_request` | `event`, `conversation` input | owner |
| Permission answered | `event` permission_resolved, `conversation` working | owner |
| `turn_end` | `event`, `conversation` review | owner |
| `session_failed`, or `hello` finding a running conversation without a session | `event` error, `conversation` failed | owner |
| Opened | `conversation` done | owner |
| Message queued, edited, removed, sent now | `queue` | owner |

Silent on purpose, because no client shows them: user, web session and client token rows (sign-in,
`last_used_at`), `nodes.last_seen_at` refreshed by `pong` (clients show `online`, which is published),
transcript chunks (no client endpoint or message), the node's `ack`s and prompt acknowledgements,
send-now and unacknowledged-prompt bookkeeping, and the routes `POST /auth/logout`,
`POST /nodes/{id}/logins` (the output follows as `login_output`), `POST /logins/{id}/input` and
`POST /conversations/{id}/cancel` (the turn ends with `turn_end` from the node).

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
| `turn_end` | `conversation_id, seq, stop_reason` |
| `session_failed` | `conversation_id, seq, message` |
| `prompt_received` | `conversation_id, prompt_id` (sent once the node has written `session/prompt` to the keeper) |
| `logins` | `logins: [{agent, account?, method, state}]` |
| `login_output` | `login_id, data` (UTF-8 text from the PTY) |
| `login_finished` | `login_id, ok` |
| `transcript` | `path, offset, data` (base64; the bytes at `offset`) |
| `workflow` | `conversation_id, run_id, line` (the journal line as JSON; no `seq`) |
| `pong` | |

Control plane to node:

| type | fields |
|---|---|
| `ack` | `conversation_id, seq` |
| `start_session` | `conversation_id, agent, cwd, resume?: acp_session_id` |
| `prompt` | `conversation_id, prompt_id, text` |
| `cancel` | `conversation_id` |
| `permission_response` | `conversation_id, request_id, outcome` (ACP `RequestPermissionOutcome`) |
| `set_config` | `conversation_id, mode?, model?, config?` |
| `close_session` | `conversation_id` |
| `start_login` | `login_id, agent, method` |
| `login_input` | `login_id, text` |
| `stop_login` | `login_id` |
| `transcript_ack` | `path, bytes` |
| `ping` | |
| `revoked` | the node deletes its credential and exits 0 |

The control plane keeps every sent `prompt` as unacknowledged until `prompt_received` arrives with its `prompt_id` (or the turn ends or the session fails). After a `hello`, it sends again the unacknowledged prompts of sessions the node reports live, and forgets those of sessions it does not. A `session_started` for the `acp_session_id` the conversation already holds sets mode and model only where they are empty. The node ignores a `prompt_id` it has already received (the ids it has written are persisted in the session directory) and answers a repeat with `prompt_received` again. Failures the node emits itself (a refused start, an unreachable keeper) take the next free `seq` for the conversation, tracked in the session directory, never a fixed value.

An answer to a permission request is refused with 409 when the conversation has failed (`session_failed`) or is no longer waiting for input (`already_resolved`). A pending send-now is forgotten when its node disconnects.

`start_login` carries the method the node reported in `logins`: `claude` supports `oauth_token`, `codex` supports `device_auth`. Any other method is answered with `login_output` explaining it and `login_finished` with `ok` false.

On `hello` the control plane marks every `working` or `input` conversation on that node that has no live session as `failed`, with an `error` event. A prompt for a conversation whose session is not live is preceded by `start_session` with `resume` set to its `acp_session_id`. Journal `seq` values keep increasing across a resume: the node continues from the end of the previous journal.

The node reconnects with exponential backoff (1s to 60s, jitter) and sends `hello` with the last
acknowledged `seq` per live conversation; the control plane replies with `ack`s and the node
replays everything after them from the keepers.

## Node daemon

### Config

`~/.config/mycelium/node.toml` (or `/var/lib/mycelium-node/node.toml` under the system service):
`control_plane`, `node_id`, `token`, `state_dir`, `projects_root`, `max_sessions` (default 4).
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

## Clients

Compose Multiplatform, Kotlin, one `shared` module and two targets: `web` (wasmJs, served by the
control plane) and `desktop` (JVM). They implement the screens on the design canvas for stage 1,
with the design system's tokens and sizes, in dark and light:

- Sign in (control plane address, then OIDC in the browser; desktop uses a client device code).
- Approve a device code (`/approve?code=…`).
- The shell: sidebar by status with group-free rows (stage 1 has no groups), conversation header
  with rename, thread rendered from events (messages, tool calls, diffs, plans, permission cards,
  workflow card), composer (mode, model, effort where offered, queue, stop, send), the rail with
  Details.
- New conversation: node, agent, folder.
- Nodes: list, join commands (shell, Nix, NixOS), logins with sign-in output, drain, revoke.
- Past conversations: archived and done.

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

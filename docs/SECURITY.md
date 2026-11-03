# Security

## Secrets at rest

The control plane encrypts every secret it stores with AES-256-GCM before it reaches Postgres. A database dump, a backup or a read-only SQL account shows ciphertext only.

| Table | Column |
|---|---|
| `forges` | `token` |
| `model_providers` | `api_key` |
| `decision_providers` | `api_key` |
| `chat_tool_servers` | `bearer_token` (integration credentials: Coolify, Outline, email passwords, Tandoor and the rest; MCP OAuth access tokens) |
| `chat_tool_servers` | `oauth_refresh_token` |
| `chat_tool_servers` | `session_key` (Last.fm and FreeAgent sign-ins) |
| `chat_tool_servers` | `settings.encryption_password` (Actual budget password) |
| `assistant_secrets` | `value` |

A stored value reads `enc:v1:<key id>:<nonce and ciphertext>`. The key id is the first four bytes of the SHA-256 of the key, so a value says which key sealed it without revealing the key.

Tokens that are only ever compared are stored as SHA-256 hashes, never encrypted: node tokens, web session tokens, API access tokens, MCP session tokens, SSH access tokens and device codes. `MYCELIUM_OIDC_CLIENT_SECRET` and `MYCELIUM_ARTIFACT_HOST_TOKEN` are read from the environment and never stored.

No API response carries a secret value. Lists show a hint (the last characters of a token) or a flag such as `encryption_password_set`.

### Setting the key

- Generate one: `openssl rand -base64 32`.
- Set `MYCELIUM_SECRET_KEY` to it, in the same place as `MYCELIUM_DATABASE_URL`.
- Unset, the control plane generates a key at `$MYCELIUM_DATA_DIR/secret.key` (mode `0600`) on first boot and logs its path once. The file must be on the persistent volume and in the backups: without the key the stored secrets cannot be read.
- A deploy stays `pull` plus `up -d`. Existing plaintext values are encrypted in place by the boot migration, under the same advisory lock as the schema migrations. Boot is idempotent: values already sealed under the current key are left alone.
- A failed boot migration keeps the control plane up and not ready, as for any other migration.

### Rotating the key

1. Generate a new key.
2. Set `MYCELIUM_SECRET_KEY` to the new key and `MYCELIUM_SECRET_KEY_PREVIOUS` to the old one (comma-separated for several).
3. Restart. The boot migration re-encrypts every value that is not under the current key.
4. Once nothing is sealed under the old key, drop it from `MYCELIUM_SECRET_KEY_PREVIOUS`.

With the generated key file instead of the environment, run `mycelium control rotate-secrets`: it puts a fresh key at the top of `secret.key`, keeps the older keys below it for reading, and re-encrypts every value. It needs the same `MYCELIUM_DATABASE_URL` and `MYCELIUM_DATA_DIR` as the server.

A value sealed under a key that is no longer configured fails to read, and the failure names the key id. Add that key back to `MYCELIUM_SECRET_KEY_PREVIOUS`.

### Limits

- The key sits beside the control plane process, so this protects the database and its backups, not a host where the control plane runs as an attacker.
- A secret inside a message or event a person typed into a chat is not a stored credential and is not encrypted. Assistant secrets are redacted from events by value.

## Node link, environments and node-local findings

Findings from the node link, environments and node-local hardening review that followed the v0.16.0 fixes. Each entry gives severity, location, the fix and the test that fails without it.

### Fixed

| Severity | Location | Finding | Fix | Test |
| --- | --- | --- | --- | --- |
| Medium | control `link.rs`, `SessionFailed` and `Workflow` | A node could name a conversation that runs on another node. The failure handling (goal state, queued prompts, usage-limit scheduling) and the workflow index ran before the node check that `record` makes, so a second node could fail another node's conversation or write workflow rows for it. | Both messages are dropped unless the conversation runs on the sending node. | `a_node_cannot_fail_or_index_workflows_of_a_conversation_on_another_node` in `tests/it/live.rs` |
| Medium | control `link.rs`, `run_link` | No limit on how fast an authenticated node could send events, so a stuck or hostile node could saturate the database. | A link that sends more than 20,000 messages in 10 seconds is closed; the node reconnects with its usual back-off. | `a_node_flooding_the_link_with_messages_is_disconnected` in `tests/it/node_link.rs` |
| Medium | node `files.rs`, `ListFiles` | The folder to search came from the control plane unchecked, so it listed any readable directory on the machine. | The folder must resolve inside the projects root, symlinks followed. | `a_folder_outside_the_projects_root_lists_nothing` |
| Medium | node `assistant_folder.rs`, `WriteFolder` | A symlink placed inside an assistant folder (by an agent working there) was followed when the folder was written back, so a file could be written outside the projects root. | The resolved parent must stay inside the folder and a symlink target is refused. | `writing_does_not_follow_a_symlink_planted_in_the_folder_to_a_place_outside_it` |
| Medium | node `terminal.rs` | Terminal shells inherited the node daemon's environment, including `MYCELIUM_*` variables and anything named like a token, secret, password, credential or API key. | Those variables are removed from every terminal. | `a_shell_does_not_inherit_the_nodes_secrets` |
| Low | node `config.rs`, `daemon.rs`, `secrets_file.rs`, `assistant_folder.rs` | The sessions folder (per-conversation API, SSH and secrets files) and assistant folders were created with the default 0755 mode. The files inside were 0600, but their names and the folders were listable by other local users. | Created and tightened to 0700, including when the sessions folder already existed with a wider mode. | `a_conversations_token_folder_and_the_sessions_folder_are_private`, `the_folder_and_its_subfolders_are_private_to_the_node_user` |
| Low | node `transcripts.rs`, `RestoreTranscript` | The restore offset was only checked for being non-negative, so a huge offset created a very large sparse file. | Offsets above 4 GiB are refused before the file is opened. | `a_restore_at_an_enormous_offset_does_not_create_a_huge_sparse_file` |
| Medium | node `environment.rs`, `browser_control.rs`, wayvnc and Chromium | Both listened on `127.0.0.1` with no credential, so another user on the same machine could connect to the VNC stream or the Chromium debug port of a running environment. | wayvnc listens on a Unix socket inside the 0700 runtime directory, and Chromium-based browsers started by the node use `--remote-debugging-pipe` on file descriptors 3 and 4. An environment opens no TCP listener. | `a_real_environment_is_reachable_only_through_private_sockets` (skipped where sway, wayvnc, grim or a Chromium-based browser is missing), `a_browser_that_has_exited_is_dropped_and_the_next_one_answers` |

### Checked and already sound

- The node link refuses plain `ws://` except to localhost (`connect_url_refuses_plain_http_except_to_this_machine`), and the node token file is written 0600.
- Every answer a node sends back (`PendingRequests`, git relay chunks, terminal buffers, folder reads, environment screenshots and input) is matched on both the request id and the id of the node that was asked, so a node cannot answer for another. Terminal, login and environment-stream messages are likewise matched to the owning node. Both ends cap a WebSocket message at 64 MiB.
- Git commands run without a shell, branch names and hashes are validated, user paths follow `--`, and the folder browser, create-folder and file read and edit commands resolve symlinks against the projects root.
- Transcript restore stays inside the agent transcript folders and refuses `..`, absolute paths and symlink escapes.

### Open

| Severity | Location | Finding | Why it is open |
| --- | --- | --- | --- |
| Low | node `terminal.rs`, `StartTerminal` | The terminal folder is any existing directory. | A terminal is a full shell as the node user, so confining its starting folder adds nothing. |

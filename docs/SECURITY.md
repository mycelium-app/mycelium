# Security findings

Findings from the node link, environments and node-local hardening review that followed the v0.16.0 fixes. Each entry gives severity, location, the fix and the test that fails without it.

## Fixed

| Severity | Location | Finding | Fix | Test |
| --- | --- | --- | --- | --- |
| Medium | control `link.rs`, `SessionFailed` and `Workflow` | A node could name a conversation that runs on another node. The failure handling (goal state, queued prompts, usage-limit scheduling) and the workflow index ran before the node check that `record` makes, so a second node could fail another node's conversation or write workflow rows for it. | Both messages are dropped unless the conversation runs on the sending node. | `a_node_cannot_fail_or_index_workflows_of_a_conversation_on_another_node` in `tests/it/live.rs` |
| Medium | control `link.rs`, `run_link` | No limit on how fast an authenticated node could send events, so a stuck or hostile node could saturate the database. | A link that sends more than 20,000 messages in 10 seconds is closed; the node reconnects with its usual back-off. | `a_node_flooding_the_link_with_messages_is_disconnected` in `tests/it/node_link.rs` |
| Medium | node `files.rs`, `ListFiles` | The folder to search came from the control plane unchecked, so it listed any readable directory on the machine. | The folder must resolve inside the projects root, symlinks followed. | `a_folder_outside_the_projects_root_lists_nothing` |
| Medium | node `assistant_folder.rs`, `WriteFolder` | A symlink placed inside an assistant folder (by an agent working there) was followed when the folder was written back, so a file could be written outside the projects root. | The resolved parent must stay inside the folder and a symlink target is refused. | `writing_does_not_follow_a_symlink_planted_in_the_folder_to_a_place_outside_it` |
| Medium | node `terminal.rs` | Terminal shells inherited the node daemon's environment, including `MYCELIUM_*` variables and anything named like a token, secret, password, credential or API key. | Those variables are removed from every terminal. | `a_shell_does_not_inherit_the_nodes_secrets` |
| Low | node `config.rs`, `daemon.rs`, `secrets_file.rs`, `assistant_folder.rs` | The sessions folder (per-conversation API, SSH and secrets files) and assistant folders were created with the default 0755 mode. The files inside were 0600, but their names and the folders were listable by other local users. | Created and tightened to 0700, including when the sessions folder already existed with a wider mode. | `a_conversations_token_folder_and_the_sessions_folder_are_private`, `the_folder_and_its_subfolders_are_private_to_the_node_user` |
| Low | node `transcripts.rs`, `RestoreTranscript` | The restore offset was only checked for being non-negative, so a huge offset created a very large sparse file. | Offsets above 4 GiB are refused before the file is opened. | `a_restore_at_an_enormous_offset_does_not_create_a_huge_sparse_file` |

## Checked and already sound

- The node link refuses plain `ws://` except to localhost (`connect_url_refuses_plain_http_except_to_this_machine`), and the node token file is written 0600.
- Every answer a node sends back (`PendingRequests`, git relay chunks, terminal buffers, folder reads, environment screenshots and input) is matched on both the request id and the id of the node that was asked, so a node cannot answer for another. Terminal, login and environment-stream messages are likewise matched to the owning node. Both ends cap a WebSocket message at 64 MiB.
- Git commands run without a shell, branch names and hashes are validated, user paths follow `--`, and the folder browser, create-folder and file read and edit commands resolve symlinks against the projects root.
- Transcript restore stays inside the agent transcript folders and refuses `..`, absolute paths and symlink escapes.

## Open

| Severity | Location | Finding | Why it is open |
| --- | --- | --- | --- |
| Medium | node `environment.rs`, wayvnc and Chromium | Both listen on `127.0.0.1` with no credential, so another user on the same machine can connect to the VNC stream or the Chromium debug port of a running environment. The runtime directory and its sockets are 0700 and removed on stop, but TCP on loopback is not subject to file permissions. | The fix is to move wayvnc to a Unix socket inside the 0700 runtime directory and Chromium to `--remote-debugging-pipe`. Neither can be exercised without wayvnc and a Chromium build on the test machine, and the browser control client needs a rewrite for the pipe. Until then, run environments only on nodes without untrusted local users. |
| Low | node `terminal.rs`, `StartTerminal` | The terminal folder is any existing directory. | A terminal is a full shell as the node user, so confining its starting folder adds nothing. |

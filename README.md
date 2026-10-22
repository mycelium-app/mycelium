> **Claude wrote this project end to end**, starting with the design in
> [`docs/DESIGN.md`](docs/DESIGN.md). Nothing is built yet. When it is, be most sceptical of moving
> a conversation between machines: it snapshots uncommitted work, rewrites paths inside agent
> transcripts and hands ownership from one node to another, and a mistake there loses work.

<div align="center">

# Mycelium

**Coding agents on every machine you own, steered from one place.**

</div>

---

Mycelium is the successor to an earlier single-machine orchestrator. A control plane on your
homelab keeps track of every conversation, project and worktree across your machines; a daemon on
each machine runs the agents and dials in. You steer all of it from web, iOS or Android.

| | |
|---|---|
| **Nodes** | Laptop, server, VM: each runs a daemon that connects out to the control plane |
| **Agents** | Claude Code and Codex, over the Agent Client Protocol or in tmux |
| **Moves** | A conversation and its worktree move between machines while idle |
| **History** | Every transcript copied centrally and searchable with ctx, even when its machine is off |
| **Memory** | Shared findings every session can recall, on every machine |
| **Config** | One AGENTS.md, skills and MCP config, pushed to every node, with a layer per group |
| **Logins** | Claude and Codex logins per node, managed from any client |

## Status

Design only. Read [`docs/DESIGN.md`](docs/DESIGN.md) for the architecture and the open questions.

## Licence

[WTFPL](LICENSE).

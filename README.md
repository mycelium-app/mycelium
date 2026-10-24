> **Claude wrote this project end to end**, from the design in [`docs/DESIGN.md`](docs/DESIGN.md) to the
> release. Be most sceptical of what happens when a connection drops: the node replays each agent's
> journal to the control plane after a reconnect or a daemon restart, and a mistake in that replay
> loses or duplicates part of a conversation. The end-to-end tests cover it against a fake agent, and
> a real Claude Code session was checked once by hand.

<div align="center">

<img src="site/logo.svg" alt="" width="72" height="72">

# Mycelium

**Coding agents on every machine you own, steered from one place.**

</div>

---

Mycelium is the successor to an earlier single-machine orchestrator. A control plane on your
homelab keeps track of every conversation, project and worktree across your machines; a daemon on
each machine runs the agents and dials in. You steer all of it from the web, the Linux desktop app or Android.

| | |
|---|---|
| **Nodes** | Laptop, server, VM: each runs a daemon that connects out to the control plane |
| **Agents** | Claude Code and Codex, over the Agent Client Protocol (tmux is planned) |
| **Moves** | Planned: a conversation and its worktree move between machines while idle |
| **History** | Every transcript copied centrally, even when its machine is off; search with ctx is planned |
| **Memory** | Planned: shared findings every session can recall, on every machine |
| **Config** | Planned: one AGENTS.md, skills and MCP config, pushed to every node, with a layer per group |
| **Logins** | Claude and Codex logins per node, managed from any client |

## Status

[v0.1.10](https://github.com/mycelium-app/mycelium/releases) is stage 1: the control plane, the node daemon, Claude Code
and Codex over ACP, and clients for web, desktop and Android. Install a node with:

```sh
curl -fsSL https://github.com/mycelium-app/mycelium/releases/latest/download/install.sh | sh -s -- https://<your control plane>
```

| Client | Where |
|---|---|
| Web | Served by the control plane at `/`. `mycelium-web.tar.gz` in each release is the same build for any static host |
| Desktop | `mycelium-desktop-linux-x86_64.deb` and `.tar.gz` in each release |
| Android | `mycelium-android.apk` in each release, signed, for sideloading or Obtainium (application id `slop.mycelium`) |

Releases are at `https://github.com/mycelium-app/mycelium/releases/download/<tag>/`, each file with a `.sha256`, and are linked
from the GitLab release. Android and desktop sign in with a device code, the same way a node does.
The desktop app runs on JetBrains Runtime for its embedded browser.

[`docs/DESIGN.md`](docs/DESIGN.md) has the architecture and the stages, [`docs/STAGE1.md`](docs/STAGE1.md) the
contract stage 1 was built to, and [example.com/mycelium](https://github.com/mycelium-app/mycelium) the short version.

## Install a node

Script, on Linux or macOS. Run as root for a system service in `/opt/mycelium`, or as yourself for a
user service in `~/.local/bin`. The script prints a code to approve in the web client.

```sh
curl -fsSL https://github.com/mycelium-app/mycelium/releases/latest/download/install.sh | sh -s -- https://mycelium.example.com
```

Nix, with the flake's `nixosModules`, `darwinModules` or `homeManagerModules`:

```nix
{
  imports = [ mycelium.nixosModules.mycelium-node ];
  services.mycelium-node = {
    enable = true;
    controlPlane = "https://mycelium.example.com";
  };
}
```

`MYCELIUM_BINARY=/path/to/mycelium` makes the script install a local build instead of downloading.

## Run the control plane

```sh
docker run -d --name mycelium -p 8080:8080 \
  -e MYCELIUM_DATABASE_URL=postgres://mycelium:secret@db/mycelium \
  -e MYCELIUM_PUBLIC_URL=https://mycelium.example.com \
  -e MYCELIUM_OIDC_ISSUER=https://id.example.com \
  -e MYCELIUM_OIDC_CLIENT_ID=mycelium \
  -v mycelium-data:/var/lib/mycelium \
  ghcr.io/mycelium-app/mycelium:latest
```

Migrations run at boot. `/healthz` stays 200 while the process is up and `/readyz` returns 503 until
the database is migrated. The full list of variables is in [`docs/STAGE1.md`](docs/STAGE1.md#config-environment).

## Licence

[WTFPL](LICENSE).

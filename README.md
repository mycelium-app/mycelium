> **An AI agent wrote this project end to end.** Claude built Mycelium from the design in
> [`docs/DESIGN.md`](docs/DESIGN.md) through to each release, with Codex on a few changes. Be most
> sceptical of reconnects: after a dropped connection or a daemon restart, the node replays each
> agent's journal to the control plane, and a mistake in that replay loses or duplicates part of a
> conversation. End-to-end tests cover the replay against a fake agent. Outside the tests, one manual
> run against a real Claude Code session is the only check.

<div align="center">

<img src="site/logo.svg" alt="" width="72" height="72">

# Mycelium

**Run Claude Code and Codex on your own machines and steer them from one place.**

[Install](#install) · [Releases](https://github.com/mycelium-app/mycelium/releases) · [Design](docs/DESIGN.md) · [Changelog](CHANGELOG.md)

</div>

---

Mycelium is a self-hosted workspace for coding agents. You run a control plane on one server and a
node on each machine you want agents on. Each node runs Claude Code and Codex and dials out to the
control plane, so a laptop behind NAT works as well as a server. You pick up any conversation from the
web client, the Linux desktop app or Android, and you can chat with Anthropic, Ollama and OpenRouter
models in the same workspace.

| | |
|---|---|
| **Conversations** | Start one on your laptop and carry on from your phone. Move an idle conversation and its worktree to another node. |
| **Artefacts** | Designs, pages, documents and slides open beside the conversation, with versions, comments and per-conversation access. |
| **Tasks and workflows** | One board holds the agents' questions and proposed tasks. Workflows run agents in parallel with a conversation per step, and schedules repeat a prompt. |
| **Assistants** | Agents with their own instructions, storage, secrets and actions. |
| **Tools** | MCP integrations with approval for each call, GitHub and GitLab projects, a terminal and Git panel, and diff walkthroughs. |
| **Cowork** | A separate desktop with its own browser. An agent drives it and you can take over at any point. |
| **Memory and instructions** | Agents store findings that any session on any node can recall. AGENTS.md has a layer per group, and agents can only propose changes to it. |
| **Usage** | Each account's 5-hour and weekly windows with reset times and projections. Long-running goals take a token, time or usage limit. |

---

## Install

You need Docker Compose and a Linux x86_64 machine for the control plane. Each node needs Node.js
with npm for the agent adapters.

### 1. Start the control plane

Save this as `compose.yaml` and replace both `change-me` placeholders with the same strong password:

```yaml
services:
  db:
    image: postgres:17-alpine
    environment:
      POSTGRES_USER: mycelium
      POSTGRES_PASSWORD: change-me
      POSTGRES_DB: mycelium
    volumes:
      - db:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD", "pg_isready", "-U", "mycelium"]
      interval: 2s
      retries: 30
    restart: unless-stopped
  mycelium:
    image: ghcr.io/mycelium-app/mycelium:latest
    depends_on:
      db:
        condition: service_healthy
    ports:
      - "127.0.0.1:8080:8080"
    environment:
      MYCELIUM_DATABASE_URL: postgres://mycelium:change-me@db:5432/mycelium
      MYCELIUM_PUBLIC_URL: http://localhost:8080
      MYCELIUM_DEV_LOGIN: you@example.com
    volumes:
      - data:/var/lib/mycelium
    restart: unless-stopped
volumes:
  db:
  data:
```

```sh
docker compose up -d
```

Open `http://localhost:8080`. The control plane runs its database migrations when it starts.
`/healthz` returns 200 while the process is up, and `/readyz` returns 503 until the migrations finish.

The development login works on localhost only. To serve Mycelium to other machines, put it behind
HTTPS, remove `MYCELIUM_DEV_LOGIN` and set:

| Variable | Value |
|---|---|
| `MYCELIUM_PUBLIC_URL` | The HTTPS address, such as `https://mycelium.example.com` |
| `MYCELIUM_OIDC_ISSUER` | Your OpenID Connect provider |
| `MYCELIUM_OIDC_CLIENT_ID` | The client you registered with `<public-url>/auth/callback` as its redirect |
| `MYCELIUM_ALLOWED_EMAILS` | The people allowed to sign in |

[`docs/STAGE1.md`](docs/STAGE1.md#config-environment) lists the other variables.

### 2. Install the agent adapters on each node

```sh
npm install -g @agentclientprotocol/claude-agent-acp
npm install -g @agentclientprotocol/codex-acp
```

You sign in to the agent accounts from Mycelium once the node has joined.

### 3. Join the node

```sh
curl -fsSL https://github.com/mycelium-app/mycelium/releases/latest/download/install.sh | sh -s -- https://mycelium.example.com
```

The installer checks the download's SHA-256, starts a systemd service and prints a link. Open the
link, check that the code matches and approve. Run it as root for a system service in `/opt/mycelium`,
or as yourself for a user service in `~/.local/bin`. Published builds are for Linux x86_64. On macOS,
build from source and set `MYCELIUM_BINARY=/path/to/mycelium` before you run the installer.

On NixOS, nix-darwin or home-manager, add the flake as an input:

```nix
inputs.mycelium.url = "github:mycelium-app/mycelium";
```

Import `nixosModules.mycelium-node`, `darwinModules.mycelium-node` or
`homeManagerModules.mycelium-node` and enable the service:

```nix
services.mycelium-node = {
  enable = true;
  controlPlane = "https://mycelium.example.com";
};
```

Then approve the node once with `mycelium node join https://mycelium.example.com`.

### 4. Open a client

| Client | Where to get it |
|---|---|
| Web | The control plane serves it at `/`. `mycelium-web.tar.gz` in each release is the same build for any static host. |
| Linux desktop | `mycelium-desktop-linux-x86_64.deb` or `.tar.gz` from the [releases](https://github.com/mycelium-app/mycelium/releases) |
| Android | `mycelium-android.apk` from the releases, for sideloading or Obtainium |

The desktop and Android apps sign in with a device code, the same way a node joins.

---

## Development

| Path | Contents |
|---|---|
| `crates/` | The control plane, node daemon, CLI and wire protocol, in Rust on axum and tokio |
| `clients/` | The web, desktop and Android clients, in Kotlin Multiplatform with Compose, and `clients/editor`, the design editor for artefacts, in TypeScript with Vite |
| `site/` | The landing page |
| `docs/` | [`DESIGN.md`](docs/DESIGN.md) for the architecture, [`STAGE1.md`](docs/STAGE1.md) for the API and config contract |

`mise.toml` pins the toolchains. `mise run test-rust` runs the Rust tests against a throwaway Postgres
on port 55499, and `mise run test-clients` runs the client tests.

## Licence

[WTFPL](LICENSE).

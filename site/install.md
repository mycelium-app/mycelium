# Installing Mycelium

Mycelium has two parts: one **control plane**, which you run once (a container and a Postgres
database), and a **node daemon** on every machine whose coding agents you want to steer. The web
client is served by the control plane. Current release: see
`https://github.com/mycelium-app/mycelium/releases/latest/download/latest`.

## 1. Run the control plane

Save as `compose.yaml` and run `docker compose up -d`:

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

Then open `http://localhost:8080`. `GET /readyz` returns 200 once migrations have run; they run
automatically at boot.

`MYCELIUM_DEV_LOGIN` signs every browser in as that address and only works while
`MYCELIUM_PUBLIC_URL` is `http://localhost` or `http://127.0.0.1`. To serve it to other machines,
put it behind HTTPS and replace the dev login with your identity provider:

| Variable | Meaning |
|---|---|
| `MYCELIUM_PUBLIC_URL` | The address people and nodes use, e.g. `https://mycelium.example.com` |
| `MYCELIUM_OIDC_ISSUER` | Your OIDC issuer URL |
| `MYCELIUM_OIDC_CLIENT_ID` | The client registered for Mycelium; redirect URI `<public url>/auth/callback` |
| `MYCELIUM_OIDC_CLIENT_SECRET` | Optional; public clients use PKCE only |
| `MYCELIUM_ALLOWED_EMAILS` | Comma-separated addresses allowed to sign in; empty allows everyone at the provider |
| `MYCELIUM_TRUSTED_PROXIES` | Comma-separated CIDRs of your reverse proxies, so client addresses are read from `X-Forwarded-For` |
| `MYCELIUM_LISTEN` | Listen address, default `0.0.0.0:8080` |
| `MYCELIUM_DATA_DIR` | Transcript copy, default `/var/lib/mycelium` |

`GET /healthz` is the liveness check and `GET /readyz` the readiness check.

## 2. Install the agents on each machine

The node runs agents through their ACP adapters. Install the ones you use, and sign the agents
in later from the Mycelium client:

```sh
npm install -g @agentclientprotocol/claude-agent-acp   # Claude Code
npm install -g @agentclientprotocol/codex-acp           # Codex
```

## 3. Add a machine (node)

Linux x86_64, with the control plane's address:

```sh
curl -fsSL https://github.com/mycelium-app/mycelium/releases/latest/download/install.sh | sh -s -- http://localhost:8080
```

The installer downloads the release, checks its SHA-256, installs `mycelium` (to
`/opt/mycelium` as root, otherwise `~/.local/bin`), and starts `mycelium node join`. That prints
a code and a link: open the link in a signed-in browser, check the code matches, and approve it.
The installer then starts the node as a systemd service (a user service when not root).

To do the steps by hand: `mycelium node join <control plane address>`, then
`mycelium node run`. `mycelium node status` shows the node's id, control plane and sessions.

Other architectures: build from source, or use the Nix package, and run the installer with
`MYCELIUM_BINARY=/path/to/mycelium`.

### NixOS, nix-darwin or home-manager

The flake (`github:mycelium-app/mycelium`, which needs access to the repository)
exports `nixosModules.mycelium-node`, `darwinModules.mycelium-node` and
`homeManagerModules.mycelium-node`. Without access to the repository, use the flake each release
publishes instead. It installs the released Linux x86_64 binaries and exports
`packages.x86_64-linux.mycelium`, `packages.x86_64-linux.mycelium-desktop`,
`nixosModules.mycelium-node` and `homeManagerModules.mycelium-node`:

```nix
inputs.mycelium.url = "https://github.com/mycelium-app/mycelium/releases/latest/download/mycelium-flake.tar.gz";
```

```nix
services.mycelium-node = {
  enable = true;
  controlPlane = "https://mycelium.example.com";
  path = [ ];  # extra packages on the node's PATH, e.g. a packaged claude-agent-acp
};
```

Then run `mycelium node join https://mycelium.example.com` once on that machine to approve it.

## 4. Clients

| Client | Where |
|---|---|
| Web | Served by the control plane at its public URL |
| Desktop (Linux) | `mycelium-desktop-linux-x86_64.deb` or `.tar.gz` under `https://github.com/mycelium-app/mycelium/releases/download/<release>/`, each with a `.sha256` |
| Android | `mycelium-android.apk` under the same release folder, with a `.sha256`. Application id `slop.mycelium` |
| Web, self-hosted | `mycelium-web.tar.gz` under the same release folder: the same client the control plane serves, as static files |

The desktop and Android clients sign in with a device code, like a node.

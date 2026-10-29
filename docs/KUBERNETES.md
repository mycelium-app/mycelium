# Kubernetes nodes (experiment)

A Kubernetes node is an ordinary Mycelium node that runs in a cluster and starts each agent
session in its own pod. It joins the control plane like any other node and keeps the same journal,
replay and link behaviour. Only the way an agent starts differs: instead of running
`claude-agent-acp` or `codex-acp` as a local process under a keeper, the node creates a pod from
an agent image and speaks the Agent Client Protocol to it over the Kubernetes attach API.

The control plane shows the Join option for Kubernetes nodes behind the `kubernetes_nodes`
experiment (Settings, Experiments). The node itself works whether or not the experiment is on.

## How it works

1. The node daemon runs as a one-replica Deployment with a service account that can manage pods
   in its namespace.
2. Starting a conversation creates a pod named `mycelium-session-<conversation id>`, labelled
   with `mycelium.example/node` and `mycelium.example/conversation`. Its main container runs the
   agent with standard input kept open.
3. The node writes ACP frames to the pod through `pods/attach` and reads the agent's output
   through `pods/log`, and records both directions in its own journal under the node state
   directory.
4. When the node restarts it finds each session's pod by label, replays its journal, then reads
   the pod's log from the first frame it has not yet seen. The pod plays the part the keeper plays
   on a normal node.
5. Closing a conversation deletes its pod. A pod whose agent exited is kept until the
   conversation is started again or closed, so its logs stay readable.

The pod runs as a non-root user, has no service account token, and has restart policy `Never`.
The agent's working folder is `/workspace/<folder>`.

## Install

1. Create the namespace, service account and role:

   ```
   kubectl apply -f deploy/kubernetes/namespace.yaml
   kubectl apply -f deploy/kubernetes/rbac.yaml
   ```

2. Build and push the agent image, or use `ghcr.io/mycelium-app/mycelium-agent:latest`:

   ```
   docker build -f deploy/kubernetes/agent.Dockerfile -t registry.example.com/mycelium-agent:1 deploy/kubernetes
   docker push registry.example.com/mycelium-agent:1
   ```

3. Join the node. Run `mycelium node join https://mycelium.example.com` once on any machine,
   then put the node's address, id and token in `node.toml` with the cluster paths:

   ```
   control_plane = "https://mycelium.example.com"
   node_id = "00000000-0000-0000-0000-000000000000"
   token = "myc_node_example"
   state_dir = "/var/lib/mycelium"
   projects_root = "/var/lib/mycelium/projects"
   ```

   ```
   kubectl -n mycelium create secret generic mycelium-node-credential --from-file=node.toml=node.toml
   ```

4. Create the agent sign-ins Secret (see below), then start the node:

   ```
   kubectl apply -f deploy/kubernetes/node.yaml
   ```

The node appears in the node list as soon as it connects. Start a conversation on it and
`kubectl -n mycelium get pods` shows the session's pod.

## Sign-ins

Agent sign-in files come from one Secret in the node's namespace, named by
`credentials_secret` (default `mycelium-agent-credentials`). It is mounted read-only into every
session pod, and each file present is copied to where the agent reads it. Every key is optional.

| Key | Copied to | Used by |
| --- | --- | --- |
| `claude-credentials.json` | `~/.claude/.credentials.json` | Claude Code |
| `claude.json` | `~/.claude.json` | Claude Code |
| `codex-auth.json` | `~/.codex/auth.json` | Codex |
| `codex-config.toml` | `~/.codex/config.toml` | Codex |

```
kubectl -n mycelium create secret generic mycelium-agent-credentials \
  --from-file=claude-credentials.json=$HOME/.claude/.credentials.json \
  --from-file=codex-auth.json=$HOME/.codex/auth.json
```

The files are copied into each pod's home, so a token an agent refreshes is not written back to
the Secret. Re-create the Secret when a sign-in expires. The values are never logged by the node.

## Configuration

Settings go in a `[kubernetes]` section of `node.toml`, and each can be overridden by an
environment variable.

| Setting | Variable | Default |
| --- | --- | --- |
| `enabled` | `MYCELIUM_KUBERNETES_ENABLED` | `false` |
| `namespace` | `MYCELIUM_KUBERNETES_NAMESPACE` | `mycelium` |
| `image` | `MYCELIUM_KUBERNETES_IMAGE` | `ghcr.io/mycelium-app/mycelium-agent:latest` |
| `image_pull_policy` | `MYCELIUM_KUBERNETES_IMAGE_PULL_POLICY` | the cluster default |
| `credentials_secret` | `MYCELIUM_KUBERNETES_CREDENTIALS_SECRET` | `mycelium-agent-credentials` |
| `storage_class` | `MYCELIUM_KUBERNETES_STORAGE_CLASS` | unset: an `emptyDir` workspace |
| `storage_size` | `MYCELIUM_KUBERNETES_STORAGE_SIZE` | `10Gi` |
| `resources.requests`, `resources.limits` | none | none |

```
[kubernetes]
enabled = true
image = "registry.example.com/mycelium-agent:1"

[kubernetes.resources.requests]
cpu = "500m"
memory = "1Gi"

[kubernetes.resources.limits]
memory = "4Gi"
```

With a `storage_class`, each conversation gets a persistent volume claim named like its pod, and
the claim survives the pod so a restarted conversation finds its files. Claims are not deleted
automatically.

## Limitations

- Experiment: the pod protocol, labels and Secret keys can change.
- Pod output is read from the container log, so the agent must write only protocol frames to
  standard output. Lines that are not JSON-RPC are ignored.
- Keep `state_dir` on a persistent volume, as `node.yaml` does. Without it a restarted node has no
  journal and cannot resume a session it did not finish.
- Resuming a conversation in a new pod does not restore the agent's own transcript, because the
  pod's home is not persistent. Only the workspace persists, and only with a `storage_class`.
- Per-conversation secrets and per-account sign-ins are not passed to the pod. All sessions use
  the one sign-in Secret.
- Attachments sent with a prompt are saved on the node, not in the pod.
- Terminals, SSH, environments and browser tools run in the node's own container, not the pod.
- Pods and claims are not garbage collected when the node is removed. Delete them with
  `kubectl -n mycelium delete pods -l app.kubernetes.io/managed-by=mycelium-node`.

use crate::config::KubernetesConfig;
use anyhow::{Context, Result, bail};
use futures_util::{AsyncBufReadExt as _, StreamExt as _};
use k8s_openapi::api::core::v1::{Container, EmptyDirVolumeSource, EnvVar, PersistentVolumeClaim, PersistentVolumeClaimSpec, PersistentVolumeClaimVolumeSource, Pod, PodSecurityContext, PodSpec, ResourceRequirements, SecretVolumeSource, Volume, VolumeMount, VolumeResourceRequirements};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::{Api, AttachParams, DeleteParams, ListParams, LogParams, ObjectMeta, Patch, PatchParams, PostParams};
use kube::{Client, ResourceExt};
use mycelium_proto::api::{KubernetesNode, PodState, SessionPod};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use uuid::Uuid;

pub const MANAGED_BY_LABEL: &str = "app.kubernetes.io/managed-by";
pub const MANAGED_BY: &str = "mycelium-node";
pub const NODE_LABEL: &str = "mycelium.example/node";
pub const OUTPOST_LABEL: &str = "mycelium.app/outpost";
pub const CONVERSATION_LABEL: &str = "mycelium.example/conversation";
pub const AGENT_CONTAINER: &str = "agent";
pub const WORKSPACE_PATH: &str = "/workspace";
pub const CREDENTIALS_PATH: &str = "/var/run/mycelium/credentials";
pub const AGENT_HOME: &str = "/home/agent";
const BEFORE_OUTPOSTS_POD_PREFIX: &str = "mycelium-session-";

const AGENT_USER: i64 = 1000;
const START_POLL: Duration = Duration::from_millis(500);
const START_ATTEMPTS: usize = 600;
const EXIT_POLL: Duration = Duration::from_millis(500);
const EXIT_ATTEMPTS: usize = 20;

pub const SIGN_IN_FILES: [(&str, &str); 4] = [
    ("claude-credentials.json", ".claude/.credentials.json"),
    ("claude.json", ".claude.json"),
    ("codex-auth.json", ".codex/auth.json"),
    ("codex-config.toml", ".codex/config.toml"),
];

#[derive(Debug, Clone)]
pub struct PodLaunch {
    pub agent_command: Vec<String>,
    pub agent_env: Vec<(String, String)>,
    pub cwd: String,
}

#[derive(Clone)]
pub struct PodRuntime {
    pods: Api<Pod>,
    claims: Api<PersistentVolumeClaim>,
    config: KubernetesConfig,
    node_id: Uuid,
}

pub fn pod_name(outpost: &str, conversation_id: Uuid) -> String {
    format!("{outpost}-session-{conversation_id}")
}

fn labels(outpost: &str, node_id: Uuid, conversation_id: Uuid) -> BTreeMap<String, String> {
    BTreeMap::from([
        (MANAGED_BY_LABEL.to_string(), MANAGED_BY.to_string()),
        (OUTPOST_LABEL.to_string(), outpost.to_string()),
        (NODE_LABEL.to_string(), node_id.to_string()),
        (CONVERSATION_LABEL.to_string(), conversation_id.to_string()),
    ])
}

fn quantities(values: &BTreeMap<String, String>) -> Option<BTreeMap<String, Quantity>> {
    (!values.is_empty()).then(|| values.iter().map(|(name, value)| (name.clone(), Quantity(value.clone()))).collect())
}

fn start_script() -> String {
    let mut script = format!("set -e\nmkdir -p \"{AGENT_HOME}/.claude\" \"{AGENT_HOME}/.codex\" \"$MYCELIUM_CWD\"\n");

    for (key, target) in SIGN_IN_FILES {
        script.push_str(&format!("if [ -f \"{CREDENTIALS_PATH}/{key}\" ]; then install -m 600 \"{CREDENTIALS_PATH}/{key}\" \"{AGENT_HOME}/{target}\"; fi\n"));
    }

    script.push_str("cd \"$MYCELIUM_CWD\"\nexec \"$@\"");

    script
}

pub fn pod_spec(config: &KubernetesConfig, node_id: Uuid, conversation_id: Uuid, launch: &PodLaunch) -> Pod {
    let name = pod_name(&config.outpost, conversation_id);
    let mut env: Vec<EnvVar> = launch.agent_env.iter().map(|(name, value)| EnvVar { name: name.clone(), value: Some(value.clone()), ..Default::default() }).collect();
    env.push(EnvVar { name: "MYCELIUM_CWD".to_string(), value: Some(launch.cwd.clone()), ..Default::default() });
    env.push(EnvVar { name: "HOME".to_string(), value: Some(AGENT_HOME.to_string()), ..Default::default() });

    let mut command = vec!["sh".to_string(), "-c".to_string(), start_script(), "mycelium-agent".to_string()];
    command.extend(launch.agent_command.iter().cloned());

    let workspace = match &config.storage_class {
        Some(_) => Volume { name: "workspace".to_string(), persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource { claim_name: name.clone(), read_only: Some(false) }), ..Default::default() },
        None => Volume { name: "workspace".to_string(), empty_dir: Some(EmptyDirVolumeSource::default()), ..Default::default() },
    };
    let credentials = Volume { name: "credentials".to_string(), secret: Some(SecretVolumeSource { secret_name: Some(config.credentials_secret.clone()), optional: Some(true), ..Default::default() }), ..Default::default() };

    let container = Container {
        name: AGENT_CONTAINER.to_string(),
        image: Some(config.image.clone()),
        image_pull_policy: config.image_pull_policy.clone(),
        command: Some(command),
        env: Some(env),
        stdin: Some(true),
        stdin_once: Some(false),
        tty: Some(false),
        resources: Some(ResourceRequirements { requests: quantities(&config.resources.requests), limits: quantities(&config.resources.limits), ..Default::default() }),
        volume_mounts: Some(vec![
            VolumeMount { name: "workspace".to_string(), mount_path: WORKSPACE_PATH.to_string(), ..Default::default() },
            VolumeMount { name: "credentials".to_string(), mount_path: CREDENTIALS_PATH.to_string(), read_only: Some(true), ..Default::default() },
        ]),
        ..Default::default()
    };

    Pod {
        metadata: ObjectMeta { name: Some(name), namespace: Some(config.namespace.clone()), labels: Some(labels(&config.outpost, node_id, conversation_id)), ..Default::default() },
        spec: Some(PodSpec {
            restart_policy: Some("Never".to_string()),
            automount_service_account_token: Some(false),
            security_context: Some(PodSecurityContext { run_as_user: Some(AGENT_USER), run_as_non_root: Some(true), fs_group: Some(AGENT_USER), ..Default::default() }),
            containers: vec![container],
            volumes: Some(vec![workspace, credentials]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn claim_spec(config: &KubernetesConfig, node_id: Uuid, conversation_id: Uuid) -> PersistentVolumeClaim {
    PersistentVolumeClaim {
        metadata: ObjectMeta { name: Some(pod_name(&config.outpost, conversation_id)), namespace: Some(config.namespace.clone()), labels: Some(labels(&config.outpost, node_id, conversation_id)), ..Default::default() },
        spec: Some(PersistentVolumeClaimSpec {
            access_modes: Some(vec!["ReadWriteOnce".to_string()]),
            storage_class_name: config.storage_class.clone(),
            resources: Some(VolumeResourceRequirements { requests: Some(BTreeMap::from([("storage".to_string(), Quantity(config.storage_size.clone()))])), ..Default::default() }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn selector(outpost: &str, node_id: Uuid, conversation_id: Uuid) -> String {
    format!("{MANAGED_BY_LABEL}={MANAGED_BY},{OUTPOST_LABEL}={outpost},{NODE_LABEL}={node_id},{CONVERSATION_LABEL}={conversation_id}")
}

fn exit_code_of(pod: &Pod) -> Option<i64> {
    let statuses = pod.status.as_ref()?.container_statuses.as_ref()?;
    let status = statuses.iter().find(|status| status.name == AGENT_CONTAINER)?;

    status.state.as_ref()?.terminated.as_ref().map(|terminated| i64::from(terminated.exit_code))
}

pub fn state_of(pod: &Pod) -> PodState {
    if exit_code_of(pod).is_some() {
        return PodState::Exited;
    }

    match started(pod) {
        true => PodState::Running,
        false => PodState::Pending,
    }
}

fn started(pod: &Pod) -> bool {
    pod.status.as_ref().and_then(|status| status.phase.as_deref()).is_some_and(|phase| phase != "Pending")
}

fn append(journal: &mut std::fs::File, seq: &mut i64, direction: &str, frame: Value) -> Result<Value> {
    *seq += 1;
    let entry = json!({"seq": *seq, "dir": direction, "frame": frame});
    writeln!(journal, "{entry}")?;
    journal.sync_data()?;

    Ok(entry)
}

fn frame_of(line: &str) -> Option<Value> {
    serde_json::from_str::<Value>(line).ok().filter(|frame| frame.get("jsonrpc").is_some())
}

impl PodRuntime {
    pub async fn connect(config: KubernetesConfig, node_id: Uuid) -> Result<PodRuntime> {
        let client = Client::try_default().await.context("This node could not reach the Kubernetes API. Run it inside the cluster with a service account, or set KUBECONFIG.")?;

        let runtime = PodRuntime::with_client(client, config, node_id);
        let adopted = runtime.adopt_unlabelled().await;

        match adopted {
            Ok(0) => {}
            Ok(count) => tracing::info!("adopted {count} session pods from before outposts into the outpost {}", runtime.config.outpost),
            Err(error) => tracing::warn!("could not adopt session pods from before outposts: {error}"),
        }

        Ok(runtime)
    }

    pub fn with_client(client: Client, config: KubernetesConfig, node_id: Uuid) -> PodRuntime {
        PodRuntime { pods: Api::namespaced(client.clone(), &config.namespace), claims: Api::namespaced(client, &config.namespace), config, node_id }
    }

    pub async fn adopt_unlabelled(&self) -> Result<usize> {
        let namespace = self.pods.list(&ListParams::default()).await.context("list the namespace's pods")?;
        let node = self.node_id.to_string();
        let before_outposts: Vec<&Pod> = namespace
            .items
            .iter()
            .filter(|pod| {
                let labels = pod.labels();

                !labels.contains_key(OUTPOST_LABEL)
                    && labels.get(MANAGED_BY_LABEL).is_some_and(|managed_by| managed_by == MANAGED_BY)
                    && labels.get(NODE_LABEL).is_some_and(|owner| *owner == node)
                    && pod.name_any().starts_with(BEFORE_OUTPOSTS_POD_PREFIX)
            })
            .collect();

        if before_outposts.is_empty() {
            return Ok(0);
        }

        let outposts: BTreeSet<&String> = namespace.items.iter().filter_map(|pod| pod.labels().get(OUTPOST_LABEL)).collect();

        if outposts.iter().any(|outpost| **outpost != self.config.outpost) {
            tracing::warn!("{} session pods from before outposts were left alone: other outposts run in the namespace {}", before_outposts.len(), self.config.namespace);

            return Ok(0);
        }

        let label = Patch::Merge(json!({"metadata": {"labels": {OUTPOST_LABEL: self.config.outpost}}}));

        for pod in &before_outposts {
            self.pods.patch(&pod.name_any(), &PatchParams::default(), &label).await.context("label a session pod from before outposts")?;
        }

        Ok(before_outposts.len())
    }

    pub async fn create(&self, conversation_id: Uuid, launch: &PodLaunch) -> Result<()> {
        self.delete(conversation_id).await?;

        if self.config.storage_class.is_some() {
            let claim = claim_spec(&self.config, self.node_id, conversation_id);
            let created = self.claims.create(&PostParams::default(), &claim).await;
            let already_there = matches!(&created, Err(kube::Error::Api(status)) if status.code == 409);

            if !already_there {
                created.context("create the session's workspace claim")?;
            }
        }

        let pod = pod_spec(&self.config, self.node_id, conversation_id, launch);
        self.pods.create(&PostParams::default(), &pod).await.context("create the session's pod")?;

        Ok(())
    }

    pub async fn report(&self, conversation_ids: &[Uuid]) -> KubernetesNode {
        let mut pods = Vec::new();

        for conversation_id in conversation_ids {
            let Ok(Some(pod)) = self.find(*conversation_id).await else {
                continue;
            };

            pods.push(SessionPod { conversation_id: *conversation_id, name: pod.name_any(), state: state_of(&pod) });
        }

        KubernetesNode { outpost: self.config.outpost.clone(), namespace: self.config.namespace.clone(), image: self.config.image.clone(), credentials_secret: self.config.credentials_secret.clone(), workspace_persistent: self.config.storage_class.is_some(), pods }
    }

    pub async fn find(&self, conversation_id: Uuid) -> Result<Option<Pod>> {
        let found = self.pods.list(&ListParams::default().labels(&selector(&self.config.outpost, self.node_id, conversation_id))).await.context("list the session's pods")?;
        let owned = found.items.into_iter().find(|pod| pod.labels().get(OUTPOST_LABEL).is_some_and(|outpost| *outpost == self.config.outpost));

        Ok(owned)
    }

    pub async fn delete(&self, conversation_id: Uuid) -> Result<()> {
        let Some(pod) = self.find(conversation_id).await? else {
            return Ok(());
        };

        let deleted = self.pods.delete(&pod.name_any(), &DeleteParams::default()).await;
        let already_gone = matches!(&deleted, Err(kube::Error::Api(status)) if status.code == 404);

        if !already_gone {
            deleted.context("delete the session's pod")?;
        }

        Ok(())
    }

    async fn wait_until_started(&self, name: &str) -> Result<()> {
        for _ in 0..START_ATTEMPTS {
            let pod = self.pods.get_opt(name).await?;
            let Some(pod) = pod else {
                bail!("The session's pod {name} was removed before it started.");
            };

            if started(&pod) {
                return Ok(());
            }

            tokio::time::sleep(START_POLL).await;
        }

        bail!("The session's pod {name} did not start. Check its events with kubectl describe pod {name}.");
    }

    async fn stdin(&self, name: &str) -> Result<impl tokio::io::AsyncWrite + Unpin + use<>> {
        let params = AttachParams::default().container(AGENT_CONTAINER).stdin(true).stdout(false).stderr(false).tty(false);
        let mut attached = self.pods.attach(name, &params).await.context("attach to the session's pod")?;

        attached.stdin().context("The session's pod gave no standard input.")
    }

    async fn exit_code(&self, name: &str) -> i64 {
        for _ in 0..EXIT_ATTEMPTS {
            let Ok(Some(pod)) = self.pods.get_opt(name).await else {
                return -1;
            };

            if let Some(code) = exit_code_of(&pod) {
                return code;
            }

            tokio::time::sleep(EXIT_POLL).await;
        }

        -1
    }

    pub async fn bridge(self, conversation_id: Uuid, dir: &Path, launch: Option<PodLaunch>, after: usize, entries: mpsc::UnboundedSender<Value>, mut frames: mpsc::UnboundedReceiver<Value>) -> Result<()> {
        let mut name = pod_name(&self.config.outpost, conversation_id);
        let journal_path = dir.join("journal.jsonl");

        if let Some(launch) = &launch {
            std::fs::write(&journal_path, "")?;
            self.create(conversation_id, launch).await?;
        } else {
            let Some(pod) = self.find(conversation_id).await? else {
                bail!("The session's pod {name} is gone.");
            };

            name = pod.name_any();
        }

        let mut seq = 0;
        let mut seen_from_agent = 0;
        let mut replayed = Vec::new();
        for line in std::fs::read_to_string(&journal_path).unwrap_or_default().lines() {
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };

            seq = entry["seq"].as_i64().unwrap_or(seq);
            if entry["dir"] == "in" {
                seen_from_agent += 1;
            }

            replayed.push(entry);
        }

        let mut journal = std::fs::OpenOptions::new().create(true).append(true).open(&journal_path)?;
        for entry in replayed.into_iter().skip(after) {
            let _ = entries.send(entry);
        }

        self.wait_until_started(&name).await?;

        let params = LogParams { container: Some(AGENT_CONTAINER.to_string()), ..Default::default() };
        let history = self.pods.logs(&name, &params).await.context("read the session's output")?;
        let frames_so_far: Vec<Value> = history.lines().filter_map(frame_of).collect();
        let mut skip = frames_so_far.len();
        for frame in frames_so_far.into_iter().skip(seen_from_agent) {
            let entry = append(&mut journal, &mut seq, "in", frame)?;
            let _ = entries.send(entry);
        }

        let _ = entries.send(json!({"live": true}));

        let followed = self.pods.log_stream(&name, &LogParams { follow: true, ..params }).await.context("follow the session's output")?;
        let mut output = followed.lines();
        let mut stdin = self.stdin(&name).await?;

        loop {
            tokio::select! {
                line = output.next() => {
                    let Some(Ok(line)) = line else {
                        break;
                    };

                    let Some(frame) = frame_of(&line) else {
                        continue;
                    };

                    if skip > 0 {
                        skip -= 1;
                        continue;
                    }

                    let entry = append(&mut journal, &mut seq, "in", frame)?;
                    let _ = entries.send(entry);
                }
                message = frames.recv() => {
                    let Some(message) = message else {
                        return Ok(());
                    };

                    if message.get("close").is_some() {
                        self.delete(conversation_id).await?;

                        continue;
                    }

                    let Some(frame) = message.get("send").cloned() else {
                        continue;
                    };

                    append(&mut journal, &mut seq, "out", frame.clone())?;
                    let line = format!("{frame}\n");
                    let mut written = stdin.write_all(line.as_bytes()).await;
                    if written.is_err() {
                        stdin = self.stdin(&name).await?;
                        written = stdin.write_all(line.as_bytes()).await;
                    }

                    written.context("write to the session's pod")?;
                    stdin.flush().await?;
                }
            }
        }

        let code = self.exit_code(&name).await;
        std::fs::write(dir.join("exit.json"), json!({"exit": code}).to_string())?;
        let _ = entries.send(json!({"exit": code}));

        Ok(())
    }
}

#[cfg(test)]
mod kube_tests {
    use super::*;
    use kube::client::Body;
    use std::sync::{Arc, Mutex};

    fn config() -> KubernetesConfig {
        KubernetesConfig { enabled: true, outpost: "lab".into(), namespace: "agents".into(), image: "registry.example.com/agent:1".into(), image_pull_policy: Some("Always".into()), credentials_secret: "agent-signins".into(), ..KubernetesConfig::default() }
    }

    fn launch() -> PodLaunch {
        PodLaunch { agent_command: vec!["claude-agent-acp".into()], agent_env: vec![("CLAUDE_CODE_DISABLE_AUTO_MEMORY".into(), "1".into())], cwd: "/workspace/site".into() }
    }

    fn conversation() -> Uuid {
        Uuid::from_u128(7)
    }

    fn node() -> Uuid {
        Uuid::from_u128(1)
    }

    fn container(pod: &Pod) -> &Container {
        &pod.spec.as_ref().unwrap().containers[0]
    }

    fn volume(pod: &Pod, name: &str) -> Volume {
        pod.spec.as_ref().unwrap().volumes.as_ref().unwrap().iter().find(|volume| volume.name == name).unwrap().clone()
    }

    fn fake_client(requests: Arc<Mutex<Vec<String>>>, answer: Value) -> Client {
        let service = tower::service_fn(move |request: http::Request<Body>| {
            let requests = requests.clone();
            let answer = answer.clone();

            async move {
                requests.lock().unwrap().push(format!("{} {}", request.method(), request.uri()));

                Ok::<_, std::convert::Infallible>(http::Response::builder().header("content-type", "application/json").body(Body::from(answer.to_string().into_bytes())).unwrap())
            }
        });

        Client::new(service, "agents")
    }

    #[test]
    fn the_pod_runs_the_configured_image_and_agent_with_its_stdin_open() {
        let pod = pod_spec(&config(), node(), conversation(), &launch());
        let agent = container(&pod);

        assert_eq!(pod.metadata.name.as_deref(), Some("lab-session-00000000-0000-0000-0000-000000000007"));
        assert_eq!(pod.metadata.namespace.as_deref(), Some("agents"));
        assert_eq!(agent.image.as_deref(), Some("registry.example.com/agent:1"));
        assert_eq!(agent.image_pull_policy.as_deref(), Some("Always"));
        assert_eq!((agent.stdin, agent.stdin_once, agent.tty), (Some(true), Some(false), Some(false)));
        assert_eq!(agent.command.as_ref().unwrap().last().map(String::as_str), Some("claude-agent-acp"));
        assert_eq!(pod.spec.as_ref().unwrap().restart_policy.as_deref(), Some("Never"));
    }

    #[test]
    fn the_pod_is_labelled_with_the_outpost_node_and_conversation() {
        let pod = pod_spec(&config(), node(), conversation(), &launch());
        let labels = pod.metadata.labels.unwrap();

        assert_eq!(labels.get(CONVERSATION_LABEL).map(String::as_str), Some("00000000-0000-0000-0000-000000000007"));
        assert_eq!(labels.get(NODE_LABEL).map(String::as_str), Some("00000000-0000-0000-0000-000000000001"));
        assert_eq!(labels.get(MANAGED_BY_LABEL).map(String::as_str), Some(MANAGED_BY));
        assert_eq!(labels.get(OUTPOST_LABEL).map(String::as_str), Some("lab"));
    }

    #[test]
    fn the_pod_gets_the_agent_environment_and_working_folder() {
        let pod = pod_spec(&config(), node(), conversation(), &launch());
        let env: Vec<(String, String)> = container(&pod).env.as_ref().unwrap().iter().map(|variable| (variable.name.clone(), variable.value.clone().unwrap())).collect();

        assert!(env.contains(&("CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_string(), "1".to_string())));
        assert!(env.contains(&("MYCELIUM_CWD".to_string(), "/workspace/site".to_string())));
        assert!(env.contains(&("HOME".to_string(), AGENT_HOME.to_string())));
    }

    #[test]
    fn the_sign_in_secret_is_mounted_read_only_and_copied_to_where_each_agent_reads_it() {
        let pod = pod_spec(&config(), node(), conversation(), &launch());
        let secret = volume(&pod, "credentials").secret.unwrap();
        let mount = container(&pod).volume_mounts.as_ref().unwrap().iter().find(|mount| mount.name == "credentials").unwrap();
        let script = &container(&pod).command.as_ref().unwrap()[2];

        assert_eq!(secret.secret_name.as_deref(), Some("agent-signins"));
        assert_eq!((mount.mount_path.as_str(), mount.read_only), (CREDENTIALS_PATH, Some(true)));
        assert!(script.contains("/home/agent/.claude/.credentials.json"));
        assert!(script.contains("/home/agent/.codex/auth.json"));
    }

    #[test]
    fn the_workspace_is_an_empty_directory_unless_a_storage_class_is_set() {
        let pod = pod_spec(&config(), node(), conversation(), &launch());
        let workspace = volume(&pod, "workspace");
        assert!(workspace.empty_dir.is_some() && workspace.persistent_volume_claim.is_none());

        let stored = KubernetesConfig { storage_class: Some("fast".into()), ..config() };
        let pod = pod_spec(&stored, node(), conversation(), &launch());
        let claim = claim_spec(&stored, node(), conversation());

        assert_eq!(volume(&pod, "workspace").persistent_volume_claim.unwrap().claim_name, "lab-session-00000000-0000-0000-0000-000000000007");
        assert_eq!(claim.spec.unwrap().storage_class_name.as_deref(), Some("fast"));
    }

    #[test]
    fn configured_resources_become_requests_and_limits() {
        let mut sized = config();
        sized.resources.requests.insert("cpu".into(), "500m".into());
        sized.resources.limits.insert("memory".into(), "4Gi".into());

        let pod = pod_spec(&sized, node(), conversation(), &launch());
        let resources = container(&pod).resources.clone().unwrap();

        assert_eq!(resources.requests.unwrap().get("cpu"), Some(&Quantity("500m".into())));
        assert_eq!(resources.limits.unwrap().get("memory"), Some(&Quantity("4Gi".into())));
    }

    #[test]
    fn the_agent_pod_has_no_cluster_api_access() {
        let pod = pod_spec(&config(), node(), conversation(), &launch());

        assert_eq!(pod.spec.unwrap().automount_service_account_token, Some(false));
    }

    #[tokio::test]
    async fn a_restarted_node_finds_a_running_session_pod_by_its_labels() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let pod = pod_spec(&config(), node(), conversation(), &launch());
        let list = json!({"apiVersion": "v1", "kind": "PodList", "metadata": {}, "items": [pod]});
        let runtime = PodRuntime::with_client(fake_client(requests.clone(), list), config(), node());

        let found = runtime.find(conversation()).await.unwrap();

        assert_eq!(found.unwrap().name_any(), pod_name("lab", conversation()));
        let sent = requests.lock().unwrap().clone();
        assert!(sent[0].starts_with("GET /api/v1/namespaces/agents/pods?"), "{sent:?}");
        assert!(sent[0].contains("labelSelector=") && sent[0].contains("00000000-0000-0000-0000-000000000007"), "{sent:?}");
    }

    #[tokio::test]
    async fn every_pod_listing_carries_the_outposts_selector() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let list = json!({"apiVersion": "v1", "kind": "PodList", "metadata": {}, "items": []});
        let runtime = PodRuntime::with_client(fake_client(requests.clone(), list), config(), node());

        runtime.find(conversation()).await.unwrap();
        runtime.delete(conversation()).await.unwrap();
        runtime.report(&[conversation()]).await;

        let sent = requests.lock().unwrap().clone();
        assert!(!sent.is_empty());
        for request in sent {
            assert!(request.contains("labelSelector=") && request.contains("mycelium.app%2Foutpost%3Dlab"), "{request}");
        }
    }

    #[tokio::test]
    async fn a_pod_of_another_outpost_is_never_found_or_deleted_even_if_the_cluster_returns_it() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let other = KubernetesConfig { outpost: "edge".into(), ..config() };
        let foreign = pod_spec(&other, node(), conversation(), &launch());
        let list = json!({"apiVersion": "v1", "kind": "PodList", "metadata": {}, "items": [foreign]});
        let runtime = PodRuntime::with_client(fake_client(requests.clone(), list), config(), node());

        assert!(runtime.find(conversation()).await.unwrap().is_none());
        runtime.delete(conversation()).await.unwrap();

        assert!(requests.lock().unwrap().iter().all(|request| request.starts_with("GET")));
    }

    fn answering_by_method(requests: Arc<Mutex<Vec<String>>>, list: Value, patched: Value) -> Client {
        let service = tower::service_fn(move |request: http::Request<Body>| {
            let requests = requests.clone();
            let answer = if request.method() == http::Method::PATCH { patched.clone() } else { list.clone() };

            async move {
                requests.lock().unwrap().push(format!("{} {}", request.method(), request.uri()));

                Ok::<_, std::convert::Infallible>(http::Response::builder().header("content-type", "application/json").body(Body::from(answer.to_string().into_bytes())).unwrap())
            }
        });

        Client::new(service, "agents")
    }

    fn pod_from_before_outposts(conversation_id: Uuid, owner: Uuid) -> Pod {
        let mut pod = pod_spec(&KubernetesConfig { outpost: "mycelium".into(), ..config() }, owner, conversation_id, &launch());
        pod.metadata.labels.as_mut().unwrap().remove(OUTPOST_LABEL);

        pod
    }

    fn node_pod_of(outpost: &str) -> Pod {
        Pod { metadata: ObjectMeta { name: Some(format!("{outpost}-node-5d9")), labels: Some(BTreeMap::from([(OUTPOST_LABEL.to_string(), outpost.to_string())])), ..Default::default() }, ..Default::default() }
    }

    async fn adoption_requests(items: Vec<Pod>) -> (usize, Vec<String>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let list = json!({"apiVersion": "v1", "kind": "PodList", "metadata": {}, "items": items});
        let mut patched = serde_json::to_value(&items[0]).unwrap();
        patched["apiVersion"] = json!("v1");
        patched["kind"] = json!("Pod");
        let runtime = PodRuntime::with_client(answering_by_method(requests.clone(), list, patched), config(), node());

        let adopted = runtime.adopt_unlabelled().await.unwrap();
        let sent = requests.lock().unwrap().clone();

        (adopted, sent)
    }

    #[tokio::test]
    async fn a_pod_from_before_outposts_is_labelled_when_only_this_outpost_runs_in_the_namespace() {
        let old = pod_from_before_outposts(conversation(), node());
        let (adopted, sent) = adoption_requests(vec![old, node_pod_of("lab")]).await;

        assert_eq!(adopted, 1);
        assert!(sent.iter().any(|request| request.starts_with("PATCH /api/v1/namespaces/agents/pods/mycelium-session-00000000-0000-0000-0000-000000000007")), "{sent:?}");
    }

    #[tokio::test]
    async fn pods_from_before_outposts_are_left_alone_when_another_outpost_runs_in_the_namespace() {
        let old = pod_from_before_outposts(conversation(), node());
        let (adopted, sent) = adoption_requests(vec![old, node_pod_of("lab"), node_pod_of("edge")]).await;

        assert_eq!(adopted, 0);
        assert!(sent.iter().all(|request| request.starts_with("GET")), "{sent:?}");
    }

    #[tokio::test]
    async fn a_pod_from_before_outposts_that_belongs_to_another_node_is_left_alone() {
        let old = pod_from_before_outposts(conversation(), Uuid::from_u128(2));
        let (adopted, sent) = adoption_requests(vec![old, node_pod_of("lab")]).await;

        assert_eq!(adopted, 0);
        assert!(sent.iter().all(|request| request.starts_with("GET")), "{sent:?}");
    }

    #[tokio::test]
    async fn a_pod_that_already_has_an_outpost_label_is_not_relabelled() {
        let current = pod_spec(&config(), node(), conversation(), &launch());
        let (adopted, sent) = adoption_requests(vec![current]).await;

        assert_eq!(adopted, 0);
        assert!(sent.iter().all(|request| request.starts_with("GET")), "{sent:?}");
    }

    #[tokio::test]
    async fn a_conversation_without_a_pod_is_not_found_and_deleting_it_does_nothing() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let list = json!({"apiVersion": "v1", "kind": "PodList", "metadata": {}, "items": []});
        let runtime = PodRuntime::with_client(fake_client(requests.clone(), list), config(), node());

        assert!(runtime.find(conversation()).await.unwrap().is_none());
        runtime.delete(conversation()).await.unwrap();

        assert!(requests.lock().unwrap().iter().all(|request| request.starts_with("GET")));
    }

    #[tokio::test]
    async fn the_node_reports_its_cluster_settings_and_each_session_pods_state() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut exited = pod_spec(&config(), node(), conversation(), &launch());
        exited.status = Some(serde_json::from_value(json!({"phase": "Running", "containerStatuses": [{"name": "agent", "image": "i", "imageID": "i", "ready": false, "restartCount": 0, "state": {"terminated": {"exitCode": 1}}}]})).unwrap());
        let list = json!({"apiVersion": "v1", "kind": "PodList", "metadata": {}, "items": [exited]});
        let runtime = PodRuntime::with_client(fake_client(requests, list), config(), node());

        let report = runtime.report(&[conversation()]).await;

        assert_eq!((report.namespace.as_str(), report.image.as_str(), report.credentials_secret.as_str(), report.workspace_persistent), ("agents", "registry.example.com/agent:1", "agent-signins", false));
        assert_eq!(report.pods, vec![SessionPod { conversation_id: conversation(), name: pod_name("lab", conversation()), state: PodState::Exited }]);
    }

    #[test]
    fn only_json_rpc_lines_from_the_pod_are_agent_frames() {
        assert!(frame_of(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#).is_some());
        assert!(frame_of(r#"{"level":"info","msg":"starting"}"#).is_none());
        assert!(frame_of("npm warn deprecated").is_none());
    }
}

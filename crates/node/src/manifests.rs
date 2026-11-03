use crate::config::DEFAULT_AGENT_IMAGE;
use crate::pod::OUTPOST_LABEL;
use anyhow::{Result, bail};

const NODE_IMAGE: &str = "ghcr.io/mycelium-app/mycelium:latest";
const MAX_OUTPOST_NAME: usize = 40;
const MAX_NAMESPACE_NAME: usize = 63;
const MAX_STORAGE_CLASS_NAME: usize = 253;

pub struct Outpost {
    pub name: String,
    pub namespace: String,
    pub storage_class: Option<String>,
}

fn is_dns_label(value: &str, longest: usize) -> bool {
    let edge_ok = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());

    value.len() <= longest && value.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && edge_ok(value.chars().next()) && edge_ok(value.chars().last())
}

impl Outpost {
    pub fn validate(&self) -> Result<()> {
        if !is_dns_label(&self.name, MAX_OUTPOST_NAME) {
            bail!("Enter a name of up to {MAX_OUTPOST_NAME} lowercase letters, numbers and hyphens, starting and ending with a letter or number.");
        }

        if !is_dns_label(&self.namespace, MAX_NAMESPACE_NAME) {
            bail!("Enter a namespace of up to {MAX_NAMESPACE_NAME} lowercase letters, numbers and hyphens, starting and ending with a letter or number.");
        }

        if let Some(class) = &self.storage_class {
            let valid = class.len() <= MAX_STORAGE_CLASS_NAME && class.split('.').all(|part| is_dns_label(part, MAX_STORAGE_CLASS_NAME));

            if !valid {
                bail!("Enter a storage class as it is named in the cluster, or leave it empty.");
            }
        }

        Ok(())
    }

    pub fn manifests(&self) -> Result<String> {
        self.validate()?;

        let name = &self.name;
        let namespace = &self.namespace;
        let storage_class = self.storage_class.as_ref().map(|class| format!("            - name: MYCELIUM_KUBERNETES_STORAGE_CLASS\n              value: {class}\n")).unwrap_or_default();

        Ok(format!(
            r#"apiVersion: v1
kind: Namespace
metadata:
  name: {namespace}
---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {name}-node
  namespace: {namespace}
  labels:
    {OUTPOST_LABEL}: {name}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {name}-node
  namespace: {namespace}
  labels:
    {OUTPOST_LABEL}: {name}
rules:
  - apiGroups: [""]
    resources: ["pods"]
    verbs: ["get", "list", "create", "delete"]
  - apiGroups: [""]
    resources: ["pods/attach"]
    verbs: ["get", "create"]
  - apiGroups: [""]
    resources: ["pods/log"]
    verbs: ["get"]
  - apiGroups: [""]
    resources: ["secrets"]
    resourceNames: ["{name}-agent-credentials"]
    verbs: ["get"]
  - apiGroups: [""]
    resources: ["persistentvolumeclaims"]
    verbs: ["create"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {name}-node
  namespace: {namespace}
  labels:
    {OUTPOST_LABEL}: {name}
subjects:
  - kind: ServiceAccount
    name: {name}-node
    namespace: {namespace}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {name}-node
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: {name}-node-state
  namespace: {namespace}
  labels:
    {OUTPOST_LABEL}: {name}
spec:
  accessModes: ["ReadWriteOnce"]
  resources:
    requests:
      storage: 1Gi
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {name}-node
  namespace: {namespace}
  labels:
    {OUTPOST_LABEL}: {name}
spec:
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels:
      app.kubernetes.io/name: mycelium-node
      {OUTPOST_LABEL}: {name}
  template:
    metadata:
      labels:
        app.kubernetes.io/name: mycelium-node
        {OUTPOST_LABEL}: {name}
    spec:
      serviceAccountName: {name}-node
      securityContext:
        runAsUser: 1000
        runAsNonRoot: true
        fsGroup: 1000
      containers:
        - name: node
          image: {NODE_IMAGE}
          command: ["dumb-init", "mycelium", "node", "run"]
          env:
            - name: MYCELIUM_NODE_CONFIG
              value: /etc/mycelium/node.toml
            - name: MYCELIUM_KUBERNETES_ENABLED
              value: "true"
            - name: MYCELIUM_KUBERNETES_OUTPOST
              value: {name}
            - name: MYCELIUM_KUBERNETES_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
            - name: MYCELIUM_KUBERNETES_IMAGE
              value: {DEFAULT_AGENT_IMAGE}
            - name: MYCELIUM_KUBERNETES_CREDENTIALS_SECRET
              value: {name}-agent-credentials
{storage_class}          volumeMounts:
            - name: credential
              mountPath: /etc/mycelium
              readOnly: true
            - name: state
              mountPath: /var/lib/mycelium
      volumes:
        - name: credential
          secret:
            secretName: {name}-node-credential
            defaultMode: 0400
        - name: state
          persistentVolumeClaim:
            claimName: {name}-node-state
"#
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outpost(name: &str, namespace: &str, storage_class: Option<&str>) -> Outpost {
        Outpost { name: name.into(), namespace: namespace.into(), storage_class: storage_class.map(String::from) }
    }

    fn documents(outpost: &Outpost) -> Vec<String> {
        outpost.manifests().unwrap().split("\n---\n").map(String::from).collect()
    }

    fn kind_of(document: &str) -> &str {
        document.lines().find_map(|line| line.strip_prefix("kind: ")).unwrap()
    }

    #[test]
    fn every_namespaced_object_is_in_the_outposts_namespace_and_carries_its_label() {
        let lab = outpost("lab", "team-a", None);
        let label = format!("{OUTPOST_LABEL}: lab");

        for document in documents(&lab).iter().filter(|document| kind_of(document) != "Namespace") {
            assert!(document.contains("  namespace: team-a\n"), "{document}");
            assert!(document.contains(&label), "{document}");
        }
    }

    #[test]
    fn permissions_are_a_role_in_the_namespace_never_cluster_wide() {
        let kinds: Vec<String> = documents(&outpost("lab", "team-a", None)).iter().map(|document| kind_of(document).to_string()).collect();

        assert!(kinds.contains(&"Role".to_string()) && kinds.contains(&"RoleBinding".to_string()));
        assert!(!kinds.iter().any(|kind| kind.starts_with("Cluster")), "{kinds:?}");
    }

    #[test]
    fn the_role_reads_only_the_outposts_own_sign_in_secret() {
        let role = documents(&outpost("lab", "team-a", None)).into_iter().find(|document| kind_of(document) == "Role").unwrap();

        assert!(role.contains("resourceNames: [\"lab-agent-credentials\"]"));
        assert!(!role.contains("\"*\""));
    }

    #[test]
    fn two_outposts_in_one_namespace_share_no_object_names_and_select_their_own_node_pods() {
        let lab = outpost("lab", "shared", None).manifests().unwrap();
        let edge = outpost("edge", "shared", None).manifests().unwrap();
        let names_of = |manifests: &str| -> Vec<String> { manifests.lines().filter_map(|line| line.strip_prefix("  name: ")).map(String::from).collect() };

        let lab_names = names_of(&lab);
        let edge_names = names_of(&edge);

        assert!(lab_names.iter().filter(|name| *name != "shared").all(|name| !edge_names.contains(name)), "{lab_names:?} {edge_names:?}");
        assert!(lab.contains(&format!("      {OUTPOST_LABEL}: lab\n  template")));
        assert!(edge.contains(&format!("      {OUTPOST_LABEL}: edge\n  template")));
    }

    #[test]
    fn the_node_is_told_its_outpost_and_storage_class_and_sign_in_secret() {
        let manifests = outpost("lab", "team-a", Some("fast")).manifests().unwrap();

        assert!(manifests.contains("name: MYCELIUM_KUBERNETES_OUTPOST\n              value: lab"));
        assert!(manifests.contains("name: MYCELIUM_KUBERNETES_STORAGE_CLASS\n              value: fast"));
        assert!(manifests.contains("name: MYCELIUM_KUBERNETES_CREDENTIALS_SECRET\n              value: lab-agent-credentials"));
        assert!(manifests.contains("secretName: lab-node-credential"));
    }

    #[test]
    fn without_a_storage_class_the_node_is_not_told_one() {
        assert!(!outpost("lab", "team-a", None).manifests().unwrap().contains("STORAGE_CLASS"));
    }

    #[test]
    fn names_that_are_not_dns_labels_are_refused_with_how_to_fix_them() {
        for (name, namespace, class) in [("Lab", "team-a", None), ("", "team-a", None), ("lab-", "team-a", None), ("lab", "team a", None), ("lab", "team-a\nkind: x", None), ("lab", "team-a", Some("fast\n"))] {
            let refused = outpost(name, namespace, class).manifests();

            assert!(refused.is_err(), "{name:?} {namespace:?} {class:?}");
        }

        let message = outpost("Lab", "team-a", None).manifests().unwrap_err().to_string();
        assert!(message.starts_with("Enter a name"), "{message}");
    }

    #[test]
    fn a_name_too_long_to_fit_its_object_names_is_refused() {
        assert!(outpost(&"a".repeat(41), "team-a", None).validate().is_err());
        assert!(outpost(&"a".repeat(40), "team-a", None).validate().is_ok());
    }
}

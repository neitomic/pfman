use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct KubeContext {
    pub name: String,
    pub cluster: String,
    pub namespace: Option<String>,
}

impl KubeContext {
    pub fn display_name(&self) -> String {
        if let Some(ns) = &self.namespace {
            format!("{} ({})", self.name, ns)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct KubeTarget {
    pub kind: String,       // "pod" or "service"
    pub name: String,
    pub namespace: String,
    pub ports: Vec<u16>,
}

impl KubeTarget {
    pub fn display_name(&self) -> String {
        let ports_str = if self.ports.is_empty() {
            "no ports".to_string()
        } else {
            self.ports
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!("{}/{} [{}] - ports: {}", self.kind, self.name, self.namespace, ports_str)
    }

    pub fn target_string(&self) -> String {
        format!("{}/{}", self.kind, self.name)
    }
}

#[derive(Debug, Deserialize)]
struct KubeConfig {
    #[serde(rename = "current-context")]
    current_context: Option<String>,
    contexts: Option<Vec<KubeConfigContext>>,
}

#[derive(Debug, Deserialize)]
struct KubeConfigContext {
    name: String,
    context: KubeConfigContextDetails,
}

#[derive(Debug, Deserialize)]
struct KubeConfigContextDetails {
    cluster: String,
    namespace: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PodList {
    items: Vec<Pod>,
}

#[derive(Debug, Deserialize)]
struct Pod {
    metadata: PodMetadata,
    spec: PodSpec,
}

#[derive(Debug, Deserialize)]
struct PodMetadata {
    name: String,
    namespace: String,
}

#[derive(Debug, Deserialize)]
struct PodSpec {
    containers: Vec<Container>,
}

#[derive(Debug, Deserialize)]
struct Container {
    ports: Option<Vec<ContainerPort>>,
}

#[derive(Debug, Deserialize)]
struct ContainerPort {
    #[serde(rename = "containerPort")]
    container_port: u16,
}

#[derive(Debug, Deserialize)]
struct ServiceList {
    items: Vec<Service>,
}

#[derive(Debug, Deserialize)]
struct Service {
    metadata: ServiceMetadata,
    spec: ServiceSpec,
}

#[derive(Debug, Deserialize)]
struct ServiceMetadata {
    name: String,
    namespace: String,
}

#[derive(Debug, Deserialize)]
struct ServiceSpec {
    ports: Option<Vec<ServicePort>>,
}

#[derive(Debug, Deserialize)]
struct ServicePort {
    port: u16,
}

pub fn get_kubeconfig_path() -> PathBuf {
    if let Ok(kubeconfig) = std::env::var("KUBECONFIG") {
        PathBuf::from(kubeconfig)
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".kube").join("config")
    }
}

pub fn parse_kube_config() -> Option<(String, Vec<KubeContext>)> {
    let config_path = get_kubeconfig_path();
    if !config_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&config_path).ok()?;
    let config: KubeConfig = serde_yaml::from_str(&content).ok()?;

    let current_context = config.current_context?;
    let contexts = config
        .contexts?
        .into_iter()
        .map(|ctx| KubeContext {
            name: ctx.name,
            cluster: ctx.context.cluster,
            namespace: ctx.context.namespace,
        })
        .collect();

    Some((current_context, contexts))
}

pub fn get_current_context() -> Option<String> {
    let output = Command::new("kubectl")
        .args(["config", "current-context"])
        .output()
        .ok()?;

    if output.status.success() {
        let context = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();
        if !context.is_empty() {
            return Some(context);
        }
    }
    None
}

pub fn get_pods_with_ports(context: Option<&str>, namespace: Option<&str>) -> Vec<KubeTarget> {
    let mut cmd = Command::new("kubectl");
    cmd.args(["get", "pods", "--all-namespaces", "-o", "json"]);

    if let Some(ctx) = context {
        cmd.args(["--context", ctx]);
    }

    let output = match cmd.output() {
        Ok(out) if out.status.success() => out,
        _ => return Vec::new(),
    };

    let pod_list: PodList = match serde_json::from_slice(&output.stdout) {
        Ok(list) => list,
        Err(_) => return Vec::new(),
    };

    pod_list
        .items
        .into_iter()
        .filter_map(|pod| {
            let ports: Vec<u16> = pod
                .spec
                .containers
                .iter()
                .filter_map(|c| c.ports.as_ref())
                .flatten()
                .map(|p| p.container_port)
                .collect();

            if ports.is_empty() {
                return None;
            }

            if let Some(ns) = namespace {
                if pod.metadata.namespace != ns {
                    return None;
                }
            }

            Some(KubeTarget {
                kind: "pods".to_string(),
                name: pod.metadata.name,
                namespace: pod.metadata.namespace,
                ports,
            })
        })
        .collect()
}

pub fn get_services_with_ports(context: Option<&str>, namespace: Option<&str>) -> Vec<KubeTarget> {
    let mut cmd = Command::new("kubectl");
    cmd.args(["get", "services", "--all-namespaces", "-o", "json"]);

    if let Some(ctx) = context {
        cmd.args(["--context", ctx]);
    }

    let output = match cmd.output() {
        Ok(out) if out.status.success() => out,
        _ => return Vec::new(),
    };

    let service_list: ServiceList = match serde_json::from_slice(&output.stdout) {
        Ok(list) => list,
        Err(_) => return Vec::new(),
    };

    service_list
        .items
        .into_iter()
        .filter_map(|svc| {
            let ports: Vec<u16> = svc
                .spec
                .ports
                .as_ref()?
                .iter()
                .map(|p| p.port)
                .collect();

            if ports.is_empty() {
                return None;
            }

            if let Some(ns) = namespace {
                if svc.metadata.namespace != ns {
                    return None;
                }
            }

            Some(KubeTarget {
                kind: "services".to_string(),
                name: svc.metadata.name,
                namespace: svc.metadata.namespace,
                ports,
            })
        })
        .collect()
}

pub fn get_targets(context: Option<&str>, namespace: Option<&str>) -> Vec<KubeTarget> {
    let mut targets = Vec::new();
    targets.extend(get_pods_with_ports(context, namespace));
    targets.extend(get_services_with_ports(context, namespace));
    targets
}

pub fn filter_targets(targets: &[KubeTarget], query: &str) -> Vec<KubeTarget> {
    if query.is_empty() {
        return targets.to_vec();
    }

    let query_lower = query.to_lowercase();
    targets
        .iter()
        .filter(|t| {
            t.name.to_lowercase().contains(&query_lower)
                || t.namespace.to_lowercase().contains(&query_lower)
                || t.kind.to_lowercase().contains(&query_lower)
        })
        .cloned()
        .collect()
}

pub fn get_namespaces(context: Option<&str>) -> Vec<String> {
    let mut cmd = Command::new("kubectl");
    cmd.args(["get", "namespaces", "-o", "jsonpath={.items[*].metadata.name}"]);

    if let Some(ctx) = context {
        cmd.args(["--context", ctx]);
    }

    let output = match cmd.output() {
        Ok(out) if out.status.success() => out,
        _ => return Vec::new(),
    };

    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(|s| s.to_string())
        .collect()
}

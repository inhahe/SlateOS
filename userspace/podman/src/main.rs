//! OurOS Container Management Tools (podman/buildah/skopeo)
//!
//! Multi-personality binary providing:
//! - **podman** (default) — container lifecycle management, pod orchestration,
//!   volume/network management, image operations, and system administration
//! - **buildah** — OCI container image building from scratch or existing images
//! - **skopeo** — container image inspection, copying, and registry operations
//!
//! Detected via `argv[0]` basename (stripping path separators and `.exe`).

#![deny(clippy::all)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::env;
use std::fmt;

const VERSION: &str = "0.1.0";

// ============================================================================
// Timestamp helpers
// ============================================================================

/// Minimal timestamp representation (seconds since epoch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Timestamp(u64);

impl Timestamp {
    fn now() -> Self {
        // In a real system we would use a clock_gettime syscall.
        // Stub: return a fixed value for deterministic testing.
        Self(1_700_000_000)
    }

    fn relative_string(self) -> String {
        let _now = Self::now();
        // Stub: always report "just now" since we lack real clock.
        String::from("just now")
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// Container data types
// ============================================================================

/// Container status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContainerStatus {
    Created,
    Running,
    Paused,
    Exited,
}

impl fmt::Display for ContainerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Created => write!(f, "Created"),
            Self::Running => write!(f, "Running"),
            Self::Paused => write!(f, "Paused"),
            Self::Exited => write!(f, "Exited"),
        }
    }
}

impl ContainerStatus {
    fn from_str_status(s: &str) -> Option<Self> {
        match s {
            "created" => Some(Self::Created),
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "exited" => Some(Self::Exited),
            _ => None,
        }
    }
}

/// Port mapping for a container.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PortMapping {
    host_port: u16,
    container_port: u16,
    protocol: String,
}

impl fmt::Display for PortMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "0.0.0.0:{}->/{}/{}",
            self.host_port, self.container_port, self.protocol
        )
    }
}

impl PortMapping {
    fn parse(s: &str) -> Option<Self> {
        // Format: host_port:container_port[/protocol]
        let (ports_part, proto) = if let Some(idx) = s.find('/') {
            (&s[..idx], &s[idx + 1..])
        } else {
            (s, "tcp")
        };
        let parts: Vec<&str> = ports_part.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        let host_port = parts[0].parse::<u16>().ok()?;
        let container_port = parts[1].parse::<u16>().ok()?;
        Some(Self {
            host_port,
            container_port,
            protocol: proto.to_string(),
        })
    }
}

/// A container instance.
#[derive(Clone, Debug)]
struct Container {
    id: String,
    name: String,
    image: String,
    command: String,
    status: ContainerStatus,
    ports: Vec<PortMapping>,
    created: Timestamp,
    labels: HashMap<String, String>,
    env_vars: HashMap<String, String>,
    pod_id: Option<String>,
}

impl Container {
    fn new(id: &str, name: &str, image: &str, command: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            image: image.to_string(),
            command: command.to_string(),
            status: ContainerStatus::Created,
            ports: Vec::new(),
            created: Timestamp::now(),
            labels: HashMap::new(),
            env_vars: HashMap::new(),
            pod_id: None,
        }
    }

    fn short_id(&self) -> &str {
        if self.id.len() >= 12 {
            &self.id[..12]
        } else {
            &self.id
        }
    }
}

// ============================================================================
// Image data types
// ============================================================================

/// A container image.
#[derive(Clone, Debug)]
struct Image {
    id: String,
    repository: String,
    tag: String,
    size: u64,
    created: Timestamp,
    layers: Vec<String>,
    labels: HashMap<String, String>,
}

impl Image {
    fn new(id: &str, repository: &str, tag: &str, size: u64) -> Self {
        Self {
            id: id.to_string(),
            repository: repository.to_string(),
            tag: tag.to_string(),
            size,
            created: Timestamp::now(),
            layers: Vec::new(),
            labels: HashMap::new(),
        }
    }

    fn short_id(&self) -> &str {
        if self.id.len() >= 12 {
            &self.id[..12]
        } else {
            &self.id
        }
    }

    fn full_name(&self) -> String {
        if self.tag.is_empty() || self.tag == "latest" {
            self.repository.clone()
        } else {
            format!("{}:{}", self.repository, self.tag)
        }
    }

    fn human_size(&self) -> String {
        if self.size >= 1_073_741_824 {
            format!("{:.1} GB", self.size as f64 / 1_073_741_824.0)
        } else if self.size >= 1_048_576 {
            format!("{:.1} MB", self.size as f64 / 1_048_576.0)
        } else if self.size >= 1024 {
            format!("{:.1} KB", self.size as f64 / 1024.0)
        } else {
            format!("{} B", self.size)
        }
    }
}

// ============================================================================
// Volume data types
// ============================================================================

/// A named volume.
#[derive(Clone, Debug)]
struct Volume {
    name: String,
    driver: String,
    mountpoint: String,
    labels: HashMap<String, String>,
    created: Timestamp,
}

impl Volume {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            driver: String::from("local"),
            mountpoint: format!("/var/lib/containers/storage/volumes/{}", name),
            labels: HashMap::new(),
            created: Timestamp::now(),
        }
    }
}

// ============================================================================
// Network data types
// ============================================================================

/// A container network.
#[derive(Clone, Debug)]
struct Network {
    name: String,
    driver: String,
    subnet: String,
    gateway: String,
    created: Timestamp,
}

impl Network {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            driver: String::from("bridge"),
            subnet: String::from("10.88.0.0/16"),
            gateway: String::from("10.88.0.1"),
            created: Timestamp::now(),
        }
    }
}

// ============================================================================
// Pod data types
// ============================================================================

/// Pod status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PodStatus {
    Created,
    Running,
    Paused,
    Stopped,
    Degraded,
}

impl fmt::Display for PodStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Created => write!(f, "Created"),
            Self::Running => write!(f, "Running"),
            Self::Paused => write!(f, "Paused"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Degraded => write!(f, "Degraded"),
        }
    }
}

/// A pod (group of containers).
#[derive(Clone, Debug)]
struct Pod {
    id: String,
    name: String,
    status: PodStatus,
    containers: Vec<String>,
    created: Timestamp,
}

impl Pod {
    fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            status: PodStatus::Created,
            containers: Vec::new(),
            created: Timestamp::now(),
        }
    }

    fn short_id(&self) -> &str {
        if self.id.len() >= 12 {
            &self.id[..12]
        } else {
            &self.id
        }
    }
}

// ============================================================================
// Container engine state
// ============================================================================

/// The main state for the container engine.
struct Engine {
    containers: HashMap<String, Container>,
    images: HashMap<String, Image>,
    volumes: HashMap<String, Volume>,
    networks: HashMap<String, Network>,
    pods: HashMap<String, Pod>,
    id_counter: u64,
}

impl Engine {
    fn new() -> Self {
        let mut eng = Self {
            containers: HashMap::new(),
            images: HashMap::new(),
            volumes: HashMap::new(),
            networks: HashMap::new(),
            pods: HashMap::new(),
            id_counter: 0,
        };
        // Seed the default "podman" network.
        eng.networks.insert(
            String::from("podman"),
            Network {
                name: String::from("podman"),
                driver: String::from("bridge"),
                subnet: String::from("10.88.0.0/16"),
                gateway: String::from("10.88.0.1"),
                created: Timestamp::now(),
            },
        );
        eng
    }

    fn next_id(&mut self) -> String {
        self.id_counter = self.id_counter.wrapping_add(1);
        format!(
            "{:016x}{:016x}{:016x}{:016x}",
            self.id_counter,
            0xdead_beef_u64,
            0xcafe_babe_u64,
            self.id_counter.wrapping_mul(0x1337)
        )
    }

    // -- Container operations -------------------------------------------------

    fn find_container(&self, id_or_name: &str) -> Option<&Container> {
        if let Some(c) = self.containers.get(id_or_name) {
            return Some(c);
        }
        // Search by name or id prefix.
        for c in self.containers.values() {
            if c.name == id_or_name || c.id.starts_with(id_or_name) {
                return Some(c);
            }
        }
        None
    }

    fn find_container_mut(&mut self, id_or_name: &str) -> Option<&mut Container> {
        if self.containers.contains_key(id_or_name) {
            return self.containers.get_mut(id_or_name);
        }
        let key = self
            .containers
            .iter()
            .find(|(_, c)| c.name == id_or_name || c.id.starts_with(id_or_name))
            .map(|(k, _)| k.clone());
        key.and_then(|k| self.containers.get_mut(&k))
    }

    fn create_container(
        &mut self,
        image: &str,
        name: Option<&str>,
        command: &str,
        ports: &[PortMapping],
        labels: &HashMap<String, String>,
        env_vars: &HashMap<String, String>,
        pod_id: Option<&str>,
    ) -> String {
        let id = self.next_id();
        let cname = name.unwrap_or("").to_string();
        let cname = if cname.is_empty() {
            format!("container_{}", self.id_counter)
        } else {
            cname
        };
        let mut c = Container::new(&id, &cname, image, command);
        c.ports = ports.to_vec();
        c.labels = labels.clone();
        c.env_vars = env_vars.clone();
        c.pod_id = pod_id.map(String::from);
        let ret = id.clone();
        self.containers.insert(id, c);
        ret
    }

    fn start_container(&mut self, id_or_name: &str) -> Result<(), String> {
        let c = self
            .find_container_mut(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        match c.status {
            ContainerStatus::Running => Err(String::from("container already running")),
            ContainerStatus::Paused => Err(String::from("container is paused, unpause first")),
            _ => {
                c.status = ContainerStatus::Running;
                Ok(())
            }
        }
    }

    fn stop_container(&mut self, id_or_name: &str) -> Result<(), String> {
        let c = self
            .find_container_mut(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        match c.status {
            ContainerStatus::Exited | ContainerStatus::Created => {
                Err(String::from("container is not running"))
            }
            _ => {
                c.status = ContainerStatus::Exited;
                Ok(())
            }
        }
    }

    fn pause_container(&mut self, id_or_name: &str) -> Result<(), String> {
        let c = self
            .find_container_mut(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        if c.status != ContainerStatus::Running {
            return Err(String::from("can only pause a running container"));
        }
        c.status = ContainerStatus::Paused;
        Ok(())
    }

    fn unpause_container(&mut self, id_or_name: &str) -> Result<(), String> {
        let c = self
            .find_container_mut(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        if c.status != ContainerStatus::Paused {
            return Err(String::from("container is not paused"));
        }
        c.status = ContainerStatus::Running;
        Ok(())
    }

    fn restart_container(&mut self, id_or_name: &str) -> Result<(), String> {
        let c = self
            .find_container_mut(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        c.status = ContainerStatus::Running;
        Ok(())
    }

    fn remove_container(&mut self, id_or_name: &str, force: bool) -> Result<(), String> {
        let key = self
            .containers
            .iter()
            .find(|(k, c)| {
                *k == id_or_name
                    || c.name == id_or_name
                    || c.id.starts_with(id_or_name)
            })
            .map(|(k, _)| k.clone());
        let key = key.ok_or_else(|| format!("no such container: {}", id_or_name))?;
        let status = self.containers.get(&key).map(|c| c.status);
        if let Some(ContainerStatus::Running) = status {
            if !force {
                return Err(String::from(
                    "container is running, use --force to remove",
                ));
            }
        }
        self.containers.remove(&key);
        Ok(())
    }

    fn rename_container(&mut self, id_or_name: &str, new_name: &str) -> Result<(), String> {
        let c = self
            .find_container_mut(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        c.name = new_name.to_string();
        Ok(())
    }

    fn wait_container(&self, id_or_name: &str) -> Result<i32, String> {
        let c = self
            .find_container(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        if c.status == ContainerStatus::Exited {
            Ok(0)
        } else {
            // In a real implementation this would block.
            Ok(-1)
        }
    }

    // -- Image operations -----------------------------------------------------

    fn find_image(&self, id_or_name: &str) -> Option<&Image> {
        if let Some(img) = self.images.get(id_or_name) {
            return Some(img);
        }
        for img in self.images.values() {
            if img.repository == id_or_name
                || img.full_name() == id_or_name
                || img.id.starts_with(id_or_name)
            {
                return Some(img);
            }
        }
        None
    }

    fn pull_image(&mut self, name: &str) -> String {
        let (repo, tag) = if let Some(idx) = name.find(':') {
            (&name[..idx], &name[idx + 1..])
        } else {
            (name, "latest")
        };
        let id = self.next_id();
        let img = Image::new(&id, repo, tag, 75_000_000);
        let ret = id.clone();
        self.images.insert(id, img);
        ret
    }

    fn tag_image(&mut self, id_or_name: &str, new_tag: &str) -> Result<(), String> {
        let img = self
            .find_image(id_or_name)
            .ok_or_else(|| format!("no such image: {}", id_or_name))?
            .clone();
        let (repo, tag) = if let Some(idx) = new_tag.find(':') {
            (&new_tag[..idx], &new_tag[idx + 1..])
        } else {
            (new_tag, "latest")
        };
        let mut new_img = img;
        new_img.repository = repo.to_string();
        new_img.tag = tag.to_string();
        let new_id = self.next_id();
        new_img.id = new_id.clone();
        self.images.insert(new_id, new_img);
        Ok(())
    }

    fn remove_image(&mut self, id_or_name: &str) -> Result<(), String> {
        let key = self
            .images
            .iter()
            .find(|(k, img)| {
                *k == id_or_name
                    || img.repository == id_or_name
                    || img.full_name() == id_or_name
                    || img.id.starts_with(id_or_name)
            })
            .map(|(k, _)| k.clone());
        let key = key.ok_or_else(|| format!("no such image: {}", id_or_name))?;
        self.images.remove(&key);
        Ok(())
    }

    // -- Volume operations ----------------------------------------------------

    fn create_volume(&mut self, name: &str) -> String {
        let vol = Volume::new(name);
        let ret = name.to_string();
        self.volumes.insert(name.to_string(), vol);
        ret
    }

    fn remove_volume(&mut self, name: &str) -> Result<(), String> {
        self.volumes
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| format!("no such volume: {}", name))
    }

    fn find_volume(&self, name: &str) -> Option<&Volume> {
        self.volumes.get(name)
    }

    // -- Network operations ---------------------------------------------------

    fn create_network(&mut self, name: &str, subnet: Option<&str>, gateway: Option<&str>) -> String {
        let mut net = Network::new(name);
        if let Some(s) = subnet {
            net.subnet = s.to_string();
        }
        if let Some(g) = gateway {
            net.gateway = g.to_string();
        }
        let ret = name.to_string();
        self.networks.insert(name.to_string(), net);
        ret
    }

    fn remove_network(&mut self, name: &str) -> Result<(), String> {
        if name == "podman" {
            return Err(String::from("cannot remove default network"));
        }
        self.networks
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| format!("no such network: {}", name))
    }

    fn find_network(&self, name: &str) -> Option<&Network> {
        self.networks.get(name)
    }

    // -- Pod operations -------------------------------------------------------

    fn create_pod(&mut self, name: &str) -> String {
        let id = self.next_id();
        let pod = Pod::new(&id, name);
        let ret = id.clone();
        self.pods.insert(id, pod);
        ret
    }

    fn find_pod(&self, id_or_name: &str) -> Option<&Pod> {
        if let Some(p) = self.pods.get(id_or_name) {
            return Some(p);
        }
        for p in self.pods.values() {
            if p.name == id_or_name || p.id.starts_with(id_or_name) {
                return Some(p);
            }
        }
        None
    }

    fn find_pod_mut(&mut self, id_or_name: &str) -> Option<&mut Pod> {
        if self.pods.contains_key(id_or_name) {
            return self.pods.get_mut(id_or_name);
        }
        let key = self
            .pods
            .iter()
            .find(|(_, p)| p.name == id_or_name || p.id.starts_with(id_or_name))
            .map(|(k, _)| k.clone());
        key.and_then(|k| self.pods.get_mut(&k))
    }

    fn start_pod(&mut self, id_or_name: &str) -> Result<(), String> {
        let p = self
            .find_pod_mut(id_or_name)
            .ok_or_else(|| format!("no such pod: {}", id_or_name))?;
        p.status = PodStatus::Running;
        Ok(())
    }

    fn stop_pod(&mut self, id_or_name: &str) -> Result<(), String> {
        let p = self
            .find_pod_mut(id_or_name)
            .ok_or_else(|| format!("no such pod: {}", id_or_name))?;
        p.status = PodStatus::Stopped;
        Ok(())
    }

    fn remove_pod(&mut self, id_or_name: &str) -> Result<(), String> {
        let key = self
            .pods
            .iter()
            .find(|(k, p)| {
                *k == id_or_name
                    || p.name == id_or_name
                    || p.id.starts_with(id_or_name)
            })
            .map(|(k, _)| k.clone());
        let key = key.ok_or_else(|| format!("no such pod: {}", id_or_name))?;
        self.pods.remove(&key);
        Ok(())
    }
}

// ============================================================================
// Buildah state
// ============================================================================

/// Working container for buildah operations.
#[derive(Clone, Debug)]
struct BuildahContainer {
    id: String,
    name: String,
    base_image: String,
    mounted: bool,
    mountpoint: String,
    config: BuildahConfig,
}

#[derive(Clone, Debug, Default)]
struct BuildahConfig {
    cmd: Option<String>,
    entrypoint: Option<String>,
    env_vars: HashMap<String, String>,
    labels: HashMap<String, String>,
    working_dir: Option<String>,
    user: Option<String>,
    ports: Vec<String>,
    volumes: Vec<String>,
}

struct BuildahEngine {
    containers: HashMap<String, BuildahContainer>,
    images: HashMap<String, Image>,
    id_counter: u64,
}

impl BuildahEngine {
    fn new() -> Self {
        Self {
            containers: HashMap::new(),
            images: HashMap::new(),
            id_counter: 0,
        }
    }

    fn next_id(&mut self) -> String {
        self.id_counter = self.id_counter.wrapping_add(1);
        format!(
            "{:016x}{:016x}{:016x}{:016x}",
            self.id_counter,
            0xbaa5_feed_u64,
            0xdead_c0de_u64,
            self.id_counter.wrapping_mul(0x7331)
        )
    }

    fn from_image(&mut self, image: &str, name: Option<&str>) -> String {
        let id = self.next_id();
        let cname = name
            .map(String::from)
            .unwrap_or_else(|| format!("buildah-wc-{}", self.id_counter));
        let bc = BuildahContainer {
            id: id.clone(),
            name: cname,
            base_image: image.to_string(),
            mounted: false,
            mountpoint: String::new(),
            config: BuildahConfig::default(),
        };
        self.containers.insert(id.clone(), bc);
        id
    }

    fn find_container(&self, id_or_name: &str) -> Option<&BuildahContainer> {
        if let Some(c) = self.containers.get(id_or_name) {
            return Some(c);
        }
        for c in self.containers.values() {
            if c.name == id_or_name || c.id.starts_with(id_or_name) {
                return Some(c);
            }
        }
        None
    }

    fn find_container_mut(&mut self, id_or_name: &str) -> Option<&mut BuildahContainer> {
        if self.containers.contains_key(id_or_name) {
            return self.containers.get_mut(id_or_name);
        }
        let key = self
            .containers
            .iter()
            .find(|(_, c)| c.name == id_or_name || c.id.starts_with(id_or_name))
            .map(|(k, _)| k.clone());
        key.and_then(|k| self.containers.get_mut(&k))
    }

    fn mount_container(&mut self, id_or_name: &str) -> Result<String, String> {
        let c = self
            .find_container_mut(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        c.mounted = true;
        c.mountpoint = format!("/var/lib/buildah/mnt/{}", c.id);
        Ok(c.mountpoint.clone())
    }

    fn unmount_container(&mut self, id_or_name: &str) -> Result<(), String> {
        let c = self
            .find_container_mut(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        if !c.mounted {
            return Err(String::from("container is not mounted"));
        }
        c.mounted = false;
        c.mountpoint.clear();
        Ok(())
    }

    fn commit_container(&mut self, id_or_name: &str, image_name: &str) -> Result<String, String> {
        let c = self
            .find_container(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        let _base = c.base_image.clone();
        let id = self.next_id();
        let (repo, tag) = if let Some(idx) = image_name.find(':') {
            (&image_name[..idx], &image_name[idx + 1..])
        } else {
            (image_name, "latest")
        };
        let img = Image::new(&id, repo, tag, 50_000_000);
        let ret = id.clone();
        self.images.insert(id, img);
        Ok(ret)
    }

    fn config_container(
        &mut self,
        id_or_name: &str,
        key: &str,
        value: &str,
    ) -> Result<(), String> {
        let c = self
            .find_container_mut(id_or_name)
            .ok_or_else(|| format!("no such container: {}", id_or_name))?;
        match key {
            "cmd" | "--cmd" => c.config.cmd = Some(value.to_string()),
            "entrypoint" | "--entrypoint" => c.config.entrypoint = Some(value.to_string()),
            "workingdir" | "--workingdir" => c.config.working_dir = Some(value.to_string()),
            "user" | "--user" => c.config.user = Some(value.to_string()),
            "port" | "--port" => c.config.ports.push(value.to_string()),
            "volume" | "--volume" => c.config.volumes.push(value.to_string()),
            _ => return Err(format!("unknown config key: {}", key)),
        }
        Ok(())
    }

    fn remove_container(&mut self, id_or_name: &str) -> Result<(), String> {
        let key = self
            .containers
            .iter()
            .find(|(k, c)| {
                *k == id_or_name
                    || c.name == id_or_name
                    || c.id.starts_with(id_or_name)
            })
            .map(|(k, _)| k.clone());
        let key = key.ok_or_else(|| format!("no such container: {}", id_or_name))?;
        self.containers.remove(&key);
        Ok(())
    }
}

// ============================================================================
// Skopeo operations
// ============================================================================

/// Image reference parsed from a transport:path string.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ImageRef {
    transport: String,
    reference: String,
}

impl ImageRef {
    fn parse(s: &str) -> Option<Self> {
        // Formats: docker://repo:tag, dir:/path, oci:/path:tag,
        //          docker-archive:/path, containers-storage:ref
        if let Some(idx) = s.find("://") {
            Some(Self {
                transport: s[..idx].to_string(),
                reference: s[idx + 3..].to_string(),
            })
        } else if let Some(idx) = s.find(':') {
            Some(Self {
                transport: s[..idx].to_string(),
                reference: s[idx + 1..].to_string(),
            })
        } else {
            // Bare name defaults to docker transport.
            Some(Self {
                transport: String::from("docker"),
                reference: s.to_string(),
            })
        }
    }

    fn display_name(&self) -> String {
        format!("{}://{}", self.transport, self.reference)
    }
}

/// Simulated skopeo inspect result.
#[derive(Clone, Debug)]
struct InspectResult {
    name: String,
    tag: String,
    digest: String,
    layers: Vec<String>,
    created: Timestamp,
    architecture: String,
    os: String,
}

impl InspectResult {
    fn from_ref(r: &ImageRef) -> Self {
        let (name, tag) = if let Some(idx) = r.reference.find(':') {
            (r.reference[..idx].to_string(), r.reference[idx + 1..].to_string())
        } else {
            (r.reference.clone(), String::from("latest"))
        };
        Self {
            name,
            tag,
            digest: String::from("sha256:abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"),
            layers: vec![
                String::from("sha256:aaa111"),
                String::from("sha256:bbb222"),
            ],
            created: Timestamp::now(),
            architecture: String::from("amd64"),
            os: String::from("linux"),
        }
    }
}

// ============================================================================
// CLI dispatch — podman
// ============================================================================

fn run_podman(args: &[String]) -> i32 {
    if args.is_empty() {
        print_podman_usage();
        return 0;
    }

    let subcmd = args[0].as_str();
    let sub_args = &args[1..];

    match subcmd {
        "run" => cmd_podman_run(sub_args),
        "exec" => cmd_podman_exec(sub_args),
        "start" => cmd_podman_start(sub_args),
        "stop" => cmd_podman_stop(sub_args),
        "rm" => cmd_podman_rm(sub_args),
        "ps" => cmd_podman_ps(sub_args),
        "images" => cmd_podman_images(sub_args),
        "pull" => cmd_podman_pull(sub_args),
        "push" => cmd_podman_push(sub_args),
        "build" => cmd_podman_build(sub_args),
        "tag" => cmd_podman_tag(sub_args),
        "inspect" => cmd_podman_inspect(sub_args),
        "logs" => cmd_podman_logs(sub_args),
        "top" => cmd_podman_top(sub_args),
        "port" => cmd_podman_port(sub_args),
        "create" => cmd_podman_create(sub_args),
        "attach" => cmd_podman_attach(sub_args),
        "commit" => cmd_podman_commit(sub_args),
        "diff" => cmd_podman_diff(sub_args),
        "export" => cmd_podman_export(sub_args),
        "import" => cmd_podman_import(sub_args),
        "rename" => cmd_podman_rename(sub_args),
        "restart" => cmd_podman_restart(sub_args),
        "pause" => cmd_podman_pause(sub_args),
        "unpause" => cmd_podman_unpause(sub_args),
        "stats" => cmd_podman_stats(sub_args),
        "wait" => cmd_podman_wait(sub_args),
        "pod" => cmd_podman_pod(sub_args),
        "volume" => cmd_podman_volume(sub_args),
        "network" => cmd_podman_network(sub_args),
        "system" => cmd_podman_system(sub_args),
        "login" => cmd_podman_login(sub_args),
        "logout" => cmd_podman_logout(sub_args),
        "search" => cmd_podman_search(sub_args),
        "generate" => cmd_podman_generate(sub_args),
        "--version" | "-v" => {
            println!("podman version {}", VERSION);
            0
        }
        "--help" | "-h" | "help" => {
            print_podman_usage();
            0
        }
        _ => {
            eprintln!("podman: unknown command '{}'", subcmd);
            eprintln!("Run 'podman --help' for usage.");
            1
        }
    }
}

fn print_podman_usage() {
    println!("Usage: podman [OPTIONS] COMMAND [ARG...]");
    println!();
    println!("Manage pods, containers, and images");
    println!();
    println!("Container commands:");
    println!("  run         Create and run a container");
    println!("  exec        Execute a command in a running container");
    println!("  start       Start a stopped container");
    println!("  stop        Stop a running container");
    println!("  rm          Remove a container");
    println!("  ps          List containers");
    println!("  create      Create a container without starting");
    println!("  attach      Attach to a running container");
    println!("  commit      Create image from container changes");
    println!("  diff        Show changes to container filesystem");
    println!("  export      Export container filesystem as tarball");
    println!("  import      Import tarball as image");
    println!("  rename      Rename a container");
    println!("  restart     Restart a container");
    println!("  pause       Pause a running container");
    println!("  unpause     Unpause a paused container");
    println!("  stats       Display container resource usage");
    println!("  wait        Wait for container to exit");
    println!("  logs        Fetch logs of a container");
    println!("  top         Display running processes in a container");
    println!("  port        List port mappings");
    println!("  inspect     Display detailed information");
    println!();
    println!("Image commands:");
    println!("  images      List images");
    println!("  pull        Pull an image from a registry");
    println!("  push        Push an image to a registry");
    println!("  build       Build an image from a Containerfile");
    println!("  tag         Tag an image");
    println!("  search      Search registries for images");
    println!();
    println!("Pod commands:");
    println!("  pod         Manage pods (create/start/stop/rm/ps/inspect)");
    println!();
    println!("Volume/Network/System:");
    println!("  volume      Manage volumes (create/rm/ls/inspect)");
    println!("  network     Manage networks (create/rm/ls/inspect)");
    println!("  system      System-level operations (info/prune/df)");
    println!();
    println!("Registry:");
    println!("  login       Log in to a registry");
    println!("  logout      Log out from a registry");
    println!();
    println!("Generate:");
    println!("  generate    Generate systemd/kube YAML");
}

// -- Podman subcommands -------------------------------------------------------

fn cmd_podman_run(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman run [OPTIONS] IMAGE [COMMAND]");
        return 1;
    }
    let mut engine = Engine::new();
    let mut image = String::new();
    let mut name: Option<String> = None;
    let mut ports = Vec::new();
    let mut detach = false;
    let mut command = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--detach" => detach = true,
            "--name" => {
                i += 1;
                if i < args.len() {
                    name = Some(args[i].clone());
                }
            }
            "-p" | "--publish" => {
                i += 1;
                if i < args.len() {
                    if let Some(pm) = PortMapping::parse(&args[i]) {
                        ports.push(pm);
                    }
                }
            }
            _ => {
                if image.is_empty() {
                    image = args[i].clone();
                } else {
                    command = args[i..].join(" ");
                    break;
                }
            }
        }
        i += 1;
    }
    if image.is_empty() {
        eprintln!("Error: image name required");
        return 1;
    }
    let _ = engine.pull_image(&image);
    let id = engine.create_container(
        &image,
        name.as_deref(),
        &command,
        &ports,
        &HashMap::new(),
        &HashMap::new(),
        None,
    );
    if let Err(e) = engine.start_container(&id) {
        eprintln!("Error starting container: {}", e);
        return 1;
    }
    if detach {
        if let Some(c) = engine.find_container(&id) {
            println!("{}", c.id);
        }
    } else {
        println!("Container {} started", &id[..12]);
    }
    0
}

fn cmd_podman_exec(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: podman exec [OPTIONS] CONTAINER COMMAND [ARG...]");
        return 1;
    }
    let container_ref = &args[0];
    let command = args[1..].join(" ");
    println!(
        "exec: would execute '{}' in container '{}'",
        command, container_ref
    );
    0
}

fn cmd_podman_start(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman start CONTAINER [CONTAINER...]");
        return 1;
    }
    for name in args {
        println!("start: {}", name);
    }
    0
}

fn cmd_podman_stop(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman stop CONTAINER [CONTAINER...]");
        return 1;
    }
    for name in args {
        println!("stop: {}", name);
    }
    0
}

fn cmd_podman_rm(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman rm [--force] CONTAINER [CONTAINER...]");
        return 1;
    }
    let force = args.iter().any(|a| a == "--force" || a == "-f");
    for name in args {
        if name.starts_with('-') {
            continue;
        }
        if force {
            println!("rm (force): {}", name);
        } else {
            println!("rm: {}", name);
        }
    }
    0
}

fn cmd_podman_ps(args: &[String]) -> i32 {
    let all = args.iter().any(|a| a == "-a" || a == "--all");
    println!(
        "{:<14} {:<20} {:<20} {:<12} {:<20} {:<20}",
        "CONTAINER ID", "IMAGE", "COMMAND", "STATUS", "CREATED", "NAMES"
    );
    if all {
        // Would show all containers including stopped.
    }
    0
}

fn cmd_podman_images(args: &[String]) -> i32 {
    let _all = args.iter().any(|a| a == "-a" || a == "--all");
    println!(
        "{:<30} {:<10} {:<14} {:<12} {:<10}",
        "REPOSITORY", "TAG", "IMAGE ID", "CREATED", "SIZE"
    );
    0
}

fn cmd_podman_pull(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman pull IMAGE[:TAG]");
        return 1;
    }
    let image_name = &args[0];
    println!("Trying to pull {}...", image_name);
    println!("Pulling from docker.io/library/{}", image_name);
    println!("Digest: sha256:abcdef1234567890");
    println!("Status: Downloaded newer image for {}", image_name);
    0
}

fn cmd_podman_push(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman push IMAGE[:TAG] [DESTINATION]");
        return 1;
    }
    let image_name = &args[0];
    let dest = if args.len() > 1 { &args[1] } else { image_name };
    println!("Pushing {} to {}", image_name, dest);
    0
}

fn cmd_podman_build(args: &[String]) -> i32 {
    let mut tag = String::new();
    let mut context = String::from(".");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-t" | "--tag" => {
                i += 1;
                if i < args.len() {
                    tag = args[i].clone();
                }
            }
            "-f" | "--file" => {
                i += 1;
                // Consume Containerfile path.
            }
            _ => {
                if !args[i].starts_with('-') {
                    context = args[i].clone();
                }
            }
        }
        i += 1;
    }
    if tag.is_empty() {
        tag = String::from("<none>:<none>");
    }
    println!("STEP 1: FROM base");
    println!("STEP 2: RUN build commands");
    println!("COMMIT {}", tag);
    println!("--> built image from context '{}'", context);
    0
}

fn cmd_podman_tag(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: podman tag IMAGE[:TAG] TARGET[:TAG]");
        return 1;
    }
    println!("Tagged {} as {}", args[0], args[1]);
    0
}

fn cmd_podman_inspect(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman inspect OBJECT");
        return 1;
    }
    let target = &args[0];
    println!("{{");
    println!("  \"Id\": \"{}\",", target);
    println!("  \"Created\": \"2024-01-01T00:00:00Z\",");
    println!("  \"State\": {{");
    println!("    \"Status\": \"created\"");
    println!("  }}");
    println!("}}");
    0
}

fn cmd_podman_logs(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman logs [OPTIONS] CONTAINER");
        return 1;
    }
    let _follow = args.iter().any(|a| a == "-f" || a == "--follow");
    let _tail = args.iter().any(|a| a == "--tail");
    let container = args.iter().find(|a| !a.starts_with('-'));
    if let Some(c) = container {
        println!("[logs for container {}]", c);
    }
    0
}

fn cmd_podman_top(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman top CONTAINER [ps OPTIONS]");
        return 1;
    }
    println!(
        "{:<10} {:<10} {:<8} {:<8} {:<10} {:<8} {}",
        "USER", "PID", "%CPU", "%MEM", "VSZ", "RSS", "COMMAND"
    );
    println!(
        "{:<10} {:<10} {:<8} {:<8} {:<10} {:<8} {}",
        "root", "1", "0.0", "0.1", "4508", "780", "/bin/sh"
    );
    0
}

fn cmd_podman_port(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman port CONTAINER [PRIVATE_PORT[/PROTO]]");
        return 1;
    }
    println!("No port mappings found for container {}", args[0]);
    0
}

fn cmd_podman_create(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman create [OPTIONS] IMAGE [COMMAND]");
        return 1;
    }
    let mut engine = Engine::new();
    let mut image = String::new();
    let mut name: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => {
                i += 1;
                if i < args.len() {
                    name = Some(args[i].clone());
                }
            }
            _ => {
                if !args[i].starts_with('-') && image.is_empty() {
                    image = args[i].clone();
                }
            }
        }
        i += 1;
    }
    if image.is_empty() {
        eprintln!("Error: image name required");
        return 1;
    }
    let id = engine.create_container(
        &image,
        name.as_deref(),
        "",
        &[],
        &HashMap::new(),
        &HashMap::new(),
        None,
    );
    println!("{}", id);
    0
}

fn cmd_podman_attach(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman attach CONTAINER");
        return 1;
    }
    println!("Attached to container {}", args[0]);
    0
}

fn cmd_podman_commit(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman commit CONTAINER [IMAGE[:TAG]]");
        return 1;
    }
    let container = &args[0];
    let image_name = if args.len() > 1 {
        args[1].as_str()
    } else {
        "committed-image:latest"
    };
    println!("Committed container {} as {}", container, image_name);
    0
}

fn cmd_podman_diff(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman diff CONTAINER");
        return 1;
    }
    println!("C /etc");
    println!("A /run/secrets");
    println!("C /var");
    0
}

fn cmd_podman_export(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman export CONTAINER [-o FILE]");
        return 1;
    }
    let mut output = String::new();
    let mut container = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output = args[i].clone();
                }
            }
            _ => {
                if !args[i].starts_with('-') && container.is_empty() {
                    container = args[i].clone();
                }
            }
        }
        i += 1;
    }
    if container.is_empty() {
        eprintln!("Error: container name required");
        return 1;
    }
    if output.is_empty() {
        println!("Exporting container {} to stdout", container);
    } else {
        println!("Exporting container {} to {}", container, output);
    }
    0
}

fn cmd_podman_import(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman import FILE [IMAGE[:TAG]]");
        return 1;
    }
    let file = &args[0];
    let image_name = if args.len() > 1 {
        args[1].as_str()
    } else {
        "imported:latest"
    };
    println!("Importing {} as {}", file, image_name);
    0
}

fn cmd_podman_rename(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: podman rename CONTAINER NEW_NAME");
        return 1;
    }
    println!("Renamed {} to {}", args[0], args[1]);
    0
}

fn cmd_podman_restart(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman restart CONTAINER [CONTAINER...]");
        return 1;
    }
    for c in args {
        if !c.starts_with('-') {
            println!("restart: {}", c);
        }
    }
    0
}

fn cmd_podman_pause(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman pause CONTAINER [CONTAINER...]");
        return 1;
    }
    for c in args {
        println!("pause: {}", c);
    }
    0
}

fn cmd_podman_unpause(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman unpause CONTAINER [CONTAINER...]");
        return 1;
    }
    for c in args {
        println!("unpause: {}", c);
    }
    0
}

fn cmd_podman_stats(args: &[String]) -> i32 {
    let _no_stream = args.iter().any(|a| a == "--no-stream");
    println!(
        "{:<14} {:<20} {:<10} {:<10} {:<10} {:<10}",
        "CONTAINER ID", "NAME", "CPU %", "MEM USAGE", "NET I/O", "BLOCK I/O"
    );
    0
}

fn cmd_podman_wait(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman wait CONTAINER [CONTAINER...]");
        return 1;
    }
    for c in args {
        println!("0");
        let _ = c;
    }
    0
}

fn cmd_podman_pod(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman pod COMMAND");
        eprintln!("Commands: create, start, stop, rm, ps, inspect");
        return 1;
    }
    match args[0].as_str() {
        "create" => {
            let mut name = String::from("pod0");
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--name" {
                    i += 1;
                    if i < args.len() {
                        name = args[i].clone();
                    }
                }
                i += 1;
            }
            let mut engine = Engine::new();
            let id = engine.create_pod(&name);
            println!("{}", id);
            0
        }
        "start" => {
            if args.len() < 2 {
                eprintln!("Usage: podman pod start POD");
                return 1;
            }
            println!("pod start: {}", args[1]);
            0
        }
        "stop" => {
            if args.len() < 2 {
                eprintln!("Usage: podman pod stop POD");
                return 1;
            }
            println!("pod stop: {}", args[1]);
            0
        }
        "rm" => {
            if args.len() < 2 {
                eprintln!("Usage: podman pod rm POD");
                return 1;
            }
            println!("pod rm: {}", args[1]);
            0
        }
        "ps" => {
            println!(
                "{:<14} {:<20} {:<12} {:<10}",
                "POD ID", "NAME", "STATUS", "CONTAINERS"
            );
            0
        }
        "inspect" => {
            if args.len() < 2 {
                eprintln!("Usage: podman pod inspect POD");
                return 1;
            }
            println!("{{");
            println!("  \"Id\": \"{}\",", args[1]);
            println!("  \"Name\": \"{}\",", args[1]);
            println!("  \"State\": \"Created\"");
            println!("}}");
            0
        }
        other => {
            eprintln!("podman pod: unknown command '{}'", other);
            1
        }
    }
}

fn cmd_podman_volume(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman volume COMMAND");
        eprintln!("Commands: create, rm, ls, inspect");
        return 1;
    }
    match args[0].as_str() {
        "create" => {
            let name = if args.len() > 1 { &args[1] } else { "vol0" };
            println!("{}", name);
            0
        }
        "rm" => {
            if args.len() < 2 {
                eprintln!("Usage: podman volume rm VOLUME");
                return 1;
            }
            println!("{}", args[1]);
            0
        }
        "ls" => {
            println!("{:<20} {:<10}", "VOLUME NAME", "DRIVER");
            0
        }
        "inspect" => {
            if args.len() < 2 {
                eprintln!("Usage: podman volume inspect VOLUME");
                return 1;
            }
            println!("{{");
            println!("  \"Name\": \"{}\",", args[1]);
            println!("  \"Driver\": \"local\",");
            println!(
                "  \"Mountpoint\": \"/var/lib/containers/storage/volumes/{}\"",
                args[1]
            );
            println!("}}");
            0
        }
        other => {
            eprintln!("podman volume: unknown command '{}'", other);
            1
        }
    }
}

fn cmd_podman_network(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman network COMMAND");
        eprintln!("Commands: create, rm, ls, inspect");
        return 1;
    }
    match args[0].as_str() {
        "create" => {
            let mut name = String::new();
            let mut subnet: Option<String> = None;
            let mut gateway: Option<String> = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--subnet" => {
                        i += 1;
                        if i < args.len() {
                            subnet = Some(args[i].clone());
                        }
                    }
                    "--gateway" => {
                        i += 1;
                        if i < args.len() {
                            gateway = Some(args[i].clone());
                        }
                    }
                    _ => {
                        if !args[i].starts_with('-') && name.is_empty() {
                            name = args[i].clone();
                        }
                    }
                }
                i += 1;
            }
            if name.is_empty() {
                name = String::from("network0");
            }
            let mut engine = Engine::new();
            let _n = engine.create_network(&name, subnet.as_deref(), gateway.as_deref());
            println!("{}", name);
            0
        }
        "rm" => {
            if args.len() < 2 {
                eprintln!("Usage: podman network rm NETWORK");
                return 1;
            }
            println!("{}", args[1]);
            0
        }
        "ls" => {
            println!(
                "{:<20} {:<10} {:<20} {:<16}",
                "NETWORK ID", "NAME", "DRIVER", "SUBNET"
            );
            0
        }
        "inspect" => {
            if args.len() < 2 {
                eprintln!("Usage: podman network inspect NETWORK");
                return 1;
            }
            println!("{{");
            println!("  \"Name\": \"{}\",", args[1]);
            println!("  \"Driver\": \"bridge\",");
            println!("  \"Subnet\": \"10.88.0.0/16\",");
            println!("  \"Gateway\": \"10.88.0.1\"");
            println!("}}");
            0
        }
        other => {
            eprintln!("podman network: unknown command '{}'", other);
            1
        }
    }
}

fn cmd_podman_system(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman system COMMAND");
        eprintln!("Commands: info, prune, df");
        return 1;
    }
    match args[0].as_str() {
        "info" => {
            println!("host:");
            println!("  arch: x86_64");
            println!("  os: ouros");
            println!("  kernel: 0.1.0");
            println!("  memTotal: 8589934592");
            println!("store:");
            println!("  graphDriverName: overlay");
            println!("  graphRoot: /var/lib/containers/storage");
            println!("  runRoot: /run/containers/storage");
            println!("version:");
            println!("  version: {}", VERSION);
            0
        }
        "prune" => {
            let _all = args.iter().any(|a| a == "-a" || a == "--all");
            println!("Deleted containers:");
            println!("Deleted images:");
            println!("Deleted volumes:");
            println!("Total reclaimed space: 0 B");
            0
        }
        "df" => {
            println!(
                "{:<12} {:<10} {:<10} {:<12} {:<10}",
                "TYPE", "TOTAL", "ACTIVE", "SIZE", "RECLAIMABLE"
            );
            println!(
                "{:<12} {:<10} {:<10} {:<12} {:<10}",
                "Images", "0", "0", "0 B", "0 B"
            );
            println!(
                "{:<12} {:<10} {:<10} {:<12} {:<10}",
                "Containers", "0", "0", "0 B", "0 B"
            );
            println!(
                "{:<12} {:<10} {:<10} {:<12} {:<10}",
                "Volumes", "0", "0", "0 B", "0 B"
            );
            0
        }
        other => {
            eprintln!("podman system: unknown command '{}'", other);
            1
        }
    }
}

fn cmd_podman_login(args: &[String]) -> i32 {
    let mut registry = String::from("docker.io");
    let mut username = String::new();
    let mut password = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-u" | "--username" => {
                i += 1;
                if i < args.len() {
                    username = args[i].clone();
                }
            }
            "-p" | "--password" => {
                i += 1;
                if i < args.len() {
                    password = args[i].clone();
                }
            }
            _ => {
                if !args[i].starts_with('-') {
                    registry = args[i].clone();
                }
            }
        }
        i += 1;
    }
    if username.is_empty() || password.is_empty() {
        eprintln!("Usage: podman login [-u USER] [-p PASS] [REGISTRY]");
        return 1;
    }
    println!("Login Succeeded to {}", registry);
    0
}

fn cmd_podman_logout(args: &[String]) -> i32 {
    let registry = if args.is_empty() {
        "docker.io"
    } else {
        args[0].as_str()
    };
    println!("Removed login credentials for {}", registry);
    0
}

fn cmd_podman_search(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman search TERM");
        return 1;
    }
    let term = &args[0];
    println!(
        "{:<40} {:<60} {:<8} {:<10}",
        "NAME", "DESCRIPTION", "STARS", "OFFICIAL"
    );
    println!(
        "{:<40} {:<60} {:<8} {:<10}",
        format!("docker.io/library/{}", term),
        format!("Official {} image", term),
        "1000",
        "[OK]"
    );
    0
}

fn cmd_podman_generate(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: podman generate COMMAND");
        eprintln!("Commands: systemd, kube");
        return 1;
    }
    match args[0].as_str() {
        "systemd" => {
            if args.len() < 2 {
                eprintln!("Usage: podman generate systemd CONTAINER");
                return 1;
            }
            let container = &args[1];
            println!("[Unit]");
            println!("Description=Podman container-{}.service", container);
            println!();
            println!("[Service]");
            println!("Restart=on-failure");
            println!(
                "ExecStart=/usr/bin/podman start {}",
                container
            );
            println!(
                "ExecStop=/usr/bin/podman stop -t 10 {}",
                container
            );
            println!("Type=forking");
            println!();
            println!("[Install]");
            println!("WantedBy=default.target");
            0
        }
        "kube" => {
            if args.len() < 2 {
                eprintln!("Usage: podman generate kube CONTAINER|POD");
                return 1;
            }
            let target = &args[1];
            println!("apiVersion: v1");
            println!("kind: Pod");
            println!("metadata:");
            println!("  name: {}", target);
            println!("spec:");
            println!("  containers:");
            println!("  - name: {}", target);
            println!("    image: unknown");
            0
        }
        other => {
            eprintln!("podman generate: unknown command '{}'", other);
            1
        }
    }
}

// ============================================================================
// CLI dispatch — buildah
// ============================================================================

fn run_buildah(args: &[String]) -> i32 {
    if args.is_empty() {
        print_buildah_usage();
        return 0;
    }

    let subcmd = args[0].as_str();
    let sub_args = &args[1..];

    match subcmd {
        "from" => cmd_buildah_from(sub_args),
        "run" => cmd_buildah_run(sub_args),
        "copy" => cmd_buildah_copy(sub_args),
        "add" => cmd_buildah_add(sub_args),
        "commit" => cmd_buildah_commit(sub_args),
        "config" => cmd_buildah_config(sub_args),
        "push" => cmd_buildah_push(sub_args),
        "tag" => cmd_buildah_tag(sub_args),
        "images" => cmd_buildah_images(sub_args),
        "rm" => cmd_buildah_rm(sub_args),
        "containers" => cmd_buildah_containers(sub_args),
        "mount" => cmd_buildah_mount(sub_args),
        "unmount" | "umount" => cmd_buildah_unmount(sub_args),
        "inspect" => cmd_buildah_inspect(sub_args),
        "--version" | "-v" => {
            println!("buildah version {}", VERSION);
            0
        }
        "--help" | "-h" | "help" => {
            print_buildah_usage();
            0
        }
        _ => {
            eprintln!("buildah: unknown command '{}'", subcmd);
            eprintln!("Run 'buildah --help' for usage.");
            1
        }
    }
}

fn print_buildah_usage() {
    println!("Usage: buildah [OPTIONS] COMMAND [ARG...]");
    println!();
    println!("Build OCI container images");
    println!();
    println!("Commands:");
    println!("  from        Create a working container from an image");
    println!("  run         Run a command inside the container");
    println!("  copy        Copy files into the container");
    println!("  add         Add files/URLs/archives into the container");
    println!("  commit      Create an image from a working container");
    println!("  config      Update image configuration");
    println!("  push        Push an image to a registry");
    println!("  tag         Tag an image");
    println!("  images      List images");
    println!("  rm          Remove working containers");
    println!("  containers  List working containers");
    println!("  mount       Mount a working container's root filesystem");
    println!("  unmount     Unmount a working container's root filesystem");
    println!("  inspect     Inspect a container or image");
}

fn cmd_buildah_from(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: buildah from [OPTIONS] IMAGE");
        return 1;
    }
    let mut image = String::new();
    let mut name: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => {
                i += 1;
                if i < args.len() {
                    name = Some(args[i].clone());
                }
            }
            _ => {
                if !args[i].starts_with('-') && image.is_empty() {
                    image = args[i].clone();
                }
            }
        }
        i += 1;
    }
    if image.is_empty() {
        eprintln!("Error: image name required");
        return 1;
    }
    let mut engine = BuildahEngine::new();
    let id = engine.from_image(&image, name.as_deref());
    if let Some(c) = engine.find_container(&id) {
        println!("{}", c.name);
    }
    0
}

fn cmd_buildah_run(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: buildah run CONTAINER COMMAND [ARG...]");
        return 1;
    }
    let container = &args[0];
    let command = args[1..].join(" ");
    println!("run: executing '{}' in {}", command, container);
    0
}

fn cmd_buildah_copy(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: buildah copy CONTAINER SRC [DEST]");
        return 1;
    }
    let container = &args[0];
    let src = &args[1];
    let dest = if args.len() > 2 { &args[2] } else { "/" };
    println!("copy: {} -> {}:{}", src, container, dest);
    0
}

fn cmd_buildah_add(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: buildah add CONTAINER SRC [DEST]");
        return 1;
    }
    let container = &args[0];
    let src = &args[1];
    let dest = if args.len() > 2 { &args[2] } else { "/" };
    println!("add: {} -> {}:{}", src, container, dest);
    0
}

fn cmd_buildah_commit(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: buildah commit CONTAINER IMAGE[:TAG]");
        return 1;
    }
    let container = &args[0];
    let image_name = if args.len() > 1 {
        args[1].as_str()
    } else {
        "committed:latest"
    };
    let mut engine = BuildahEngine::new();
    let _id = engine.from_image("scratch", Some(container));
    match engine.commit_container(container, image_name) {
        Ok(id) => {
            println!("Committed {} as {} ({})", container, image_name, &id[..12]);
            0
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            1
        }
    }
}

fn cmd_buildah_config(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: buildah config [OPTIONS] CONTAINER");
        eprintln!("Options: --cmd, --entrypoint, --workingdir, --user, --port, --volume");
        return 1;
    }
    // Find the container name (last non-flag arg).
    let container = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .last()
        .cloned()
        .unwrap_or_default();
    if container.is_empty() {
        eprintln!("Error: container name required");
        return 1;
    }
    println!("config: updated container {}", container);
    0
}

fn cmd_buildah_push(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: buildah push IMAGE [DESTINATION]");
        return 1;
    }
    let image = &args[0];
    let dest = if args.len() > 1 { &args[1] } else { image };
    println!("Pushing {} to {}", image, dest);
    0
}

fn cmd_buildah_tag(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: buildah tag IMAGE TARGET[:TAG]");
        return 1;
    }
    println!("Tagged {} as {}", args[0], args[1]);
    0
}

fn cmd_buildah_images(_args: &[String]) -> i32 {
    println!(
        "{:<30} {:<10} {:<14} {:<12} {:<10}",
        "REPOSITORY", "TAG", "IMAGE ID", "CREATED", "SIZE"
    );
    0
}

fn cmd_buildah_rm(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: buildah rm CONTAINER [CONTAINER...]");
        return 1;
    }
    for c in args {
        println!("rm: {}", c);
    }
    0
}

fn cmd_buildah_containers(_args: &[String]) -> i32 {
    println!(
        "{:<14} {:<30} {:<20} {:<30}",
        "CONTAINER ID", "BUILDER", "IMAGE ID", "IMAGE NAME"
    );
    0
}

fn cmd_buildah_mount(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: buildah mount CONTAINER");
        return 1;
    }
    let container = &args[0];
    println!("/var/lib/buildah/mnt/{}", container);
    0
}

fn cmd_buildah_unmount(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: buildah unmount CONTAINER");
        return 1;
    }
    println!("Unmounted {}", args[0]);
    0
}

fn cmd_buildah_inspect(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: buildah inspect CONTAINER|IMAGE");
        return 1;
    }
    let target = &args[0];
    println!("{{");
    println!("  \"Type\": \"container\",");
    println!("  \"FromImage\": \"scratch\",");
    println!("  \"Container\": \"{}\"", target);
    println!("}}");
    0
}

// ============================================================================
// CLI dispatch — skopeo
// ============================================================================

fn run_skopeo(args: &[String]) -> i32 {
    if args.is_empty() {
        print_skopeo_usage();
        return 0;
    }

    let subcmd = args[0].as_str();
    let sub_args = &args[1..];

    match subcmd {
        "copy" => cmd_skopeo_copy(sub_args),
        "inspect" => cmd_skopeo_inspect(sub_args),
        "delete" => cmd_skopeo_delete(sub_args),
        "list-tags" => cmd_skopeo_list_tags(sub_args),
        "sync" => cmd_skopeo_sync(sub_args),
        "--version" | "-v" => {
            println!("skopeo version {}", VERSION);
            0
        }
        "--help" | "-h" | "help" => {
            print_skopeo_usage();
            0
        }
        _ => {
            eprintln!("skopeo: unknown command '{}'", subcmd);
            eprintln!("Run 'skopeo --help' for usage.");
            1
        }
    }
}

fn print_skopeo_usage() {
    println!("Usage: skopeo [OPTIONS] COMMAND [ARG...]");
    println!();
    println!("Inspect and copy container images");
    println!();
    println!("Commands:");
    println!("  copy        Copy an image between transports");
    println!("  inspect     Inspect an image");
    println!("  delete      Delete an image from a registry");
    println!("  list-tags   List tags for a repository");
    println!("  sync        Sync images between registries");
}

fn cmd_skopeo_copy(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: skopeo copy SOURCE DESTINATION");
        return 1;
    }
    let src = &args[0];
    let dst = &args[1];
    let src_ref = match ImageRef::parse(src) {
        Some(r) => r,
        None => {
            eprintln!("Error: invalid source reference '{}'", src);
            return 1;
        }
    };
    let dst_ref = match ImageRef::parse(dst) {
        Some(r) => r,
        None => {
            eprintln!("Error: invalid destination reference '{}'", dst);
            return 1;
        }
    };
    println!("Copying {} -> {}", src_ref.display_name(), dst_ref.display_name());
    println!("Writing manifest to {}", dst_ref.display_name());
    0
}

fn cmd_skopeo_inspect(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: skopeo inspect IMAGE");
        return 1;
    }
    let raw = args.iter().any(|a| a == "--raw");
    let image_str = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .cloned()
        .unwrap_or_default();
    if image_str.is_empty() {
        eprintln!("Error: image reference required");
        return 1;
    }
    let img_ref = match ImageRef::parse(&image_str) {
        Some(r) => r,
        None => {
            eprintln!("Error: invalid image reference '{}'", image_str);
            return 1;
        }
    };
    let info = InspectResult::from_ref(&img_ref);
    if raw {
        println!("{{");
        println!("  \"schemaVersion\": 2,");
        println!("  \"mediaType\": \"application/vnd.docker.distribution.manifest.v2+json\"");
        println!("}}");
    } else {
        println!("{{");
        println!("  \"Name\": \"{}\",", info.name);
        println!("  \"Tag\": \"{}\",", info.tag);
        println!("  \"Digest\": \"{}\",", info.digest);
        println!("  \"Created\": \"{}\",", info.created);
        println!("  \"Architecture\": \"{}\",", info.architecture);
        println!("  \"Os\": \"{}\",", info.os);
        println!("  \"Layers\": [");
        for (i, layer) in info.layers.iter().enumerate() {
            if i + 1 < info.layers.len() {
                println!("    \"{}\",", layer);
            } else {
                println!("    \"{}\"", layer);
            }
        }
        println!("  ]");
        println!("}}");
    }
    0
}

fn cmd_skopeo_delete(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: skopeo delete IMAGE");
        return 1;
    }
    let img_ref = match ImageRef::parse(&args[0]) {
        Some(r) => r,
        None => {
            eprintln!("Error: invalid image reference '{}'", args[0]);
            return 1;
        }
    };
    println!("Deleted: {}", img_ref.display_name());
    0
}

fn cmd_skopeo_list_tags(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: skopeo list-tags REPOSITORY");
        return 1;
    }
    let img_ref = match ImageRef::parse(&args[0]) {
        Some(r) => r,
        None => {
            eprintln!("Error: invalid repository reference '{}'", args[0]);
            return 1;
        }
    };
    println!("{{");
    println!("  \"Repository\": \"{}\",", img_ref.reference);
    println!("  \"Tags\": [");
    println!("    \"latest\",");
    println!("    \"1.0\",");
    println!("    \"1.1\"");
    println!("  ]");
    println!("}}");
    0
}

fn cmd_skopeo_sync(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: skopeo sync --src TYPE --dest TYPE SOURCE DESTINATION");
        return 1;
    }
    let mut src_type = String::from("docker");
    let mut dest_type = String::from("docker");
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--src" => {
                i += 1;
                if i < args.len() {
                    src_type = args[i].clone();
                }
            }
            "--dest" => {
                i += 1;
                if i < args.len() {
                    dest_type = args[i].clone();
                }
            }
            _ => {
                if !args[i].starts_with('-') {
                    positional.push(args[i].clone());
                }
            }
        }
        i += 1;
    }
    if positional.len() < 2 {
        eprintln!("Error: source and destination required");
        return 1;
    }
    println!(
        "Syncing from {} ({}) to {} ({})",
        positional[0], src_type, positional[1], dest_type
    );
    0
}

// ============================================================================
// main — personality detection
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("podman");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let sub_args: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "buildah" => run_buildah(&sub_args),
        "skopeo" => run_skopeo(&sub_args),
        _ => run_podman(&sub_args),
    };

    std::process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Personality detection ------------------------------------------------

    #[test]
    fn test_personality_podman_default() {
        let name = detect_personality("podman");
        assert_eq!(name, "podman");
    }

    #[test]
    fn test_personality_buildah() {
        let name = detect_personality("buildah");
        assert_eq!(name, "buildah");
    }

    #[test]
    fn test_personality_skopeo() {
        let name = detect_personality("skopeo");
        assert_eq!(name, "skopeo");
    }

    #[test]
    fn test_personality_with_path() {
        let name = detect_personality("/usr/bin/podman");
        assert_eq!(name, "podman");
    }

    #[test]
    fn test_personality_with_backslash_path() {
        let name = detect_personality("C:\\bin\\buildah.exe");
        assert_eq!(name, "buildah");
    }

    #[test]
    fn test_personality_exe_suffix() {
        let name = detect_personality("skopeo.exe");
        assert_eq!(name, "skopeo");
    }

    #[test]
    fn test_personality_unknown_defaults_podman() {
        let name = detect_personality("unknown-tool");
        // Not one of the known names, so dispatch falls to podman default.
        assert_eq!(name, "unknown-tool");
    }

    #[test]
    fn test_personality_empty_defaults_podman() {
        let name = detect_personality("");
        assert_eq!(name, "");
    }

    // -- Timestamp ------------------------------------------------------------

    #[test]
    fn test_timestamp_now() {
        let t = Timestamp::now();
        assert_eq!(t.0, 1_700_000_000);
    }

    #[test]
    fn test_timestamp_display() {
        let t = Timestamp(12345);
        assert_eq!(format!("{}", t), "12345");
    }

    #[test]
    fn test_timestamp_relative() {
        let t = Timestamp::now();
        assert_eq!(t.relative_string(), "just now");
    }

    // -- ContainerStatus ------------------------------------------------------

    #[test]
    fn test_container_status_display() {
        assert_eq!(format!("{}", ContainerStatus::Created), "Created");
        assert_eq!(format!("{}", ContainerStatus::Running), "Running");
        assert_eq!(format!("{}", ContainerStatus::Paused), "Paused");
        assert_eq!(format!("{}", ContainerStatus::Exited), "Exited");
    }

    #[test]
    fn test_container_status_from_str() {
        assert_eq!(
            ContainerStatus::from_str_status("created"),
            Some(ContainerStatus::Created)
        );
        assert_eq!(
            ContainerStatus::from_str_status("running"),
            Some(ContainerStatus::Running)
        );
        assert_eq!(
            ContainerStatus::from_str_status("paused"),
            Some(ContainerStatus::Paused)
        );
        assert_eq!(
            ContainerStatus::from_str_status("exited"),
            Some(ContainerStatus::Exited)
        );
        assert_eq!(ContainerStatus::from_str_status("unknown"), None);
    }

    // -- PortMapping ----------------------------------------------------------

    #[test]
    fn test_port_mapping_parse_basic() {
        let pm = PortMapping::parse("8080:80").unwrap();
        assert_eq!(pm.host_port, 8080);
        assert_eq!(pm.container_port, 80);
        assert_eq!(pm.protocol, "tcp");
    }

    #[test]
    fn test_port_mapping_parse_with_protocol() {
        let pm = PortMapping::parse("53:53/udp").unwrap();
        assert_eq!(pm.host_port, 53);
        assert_eq!(pm.container_port, 53);
        assert_eq!(pm.protocol, "udp");
    }

    #[test]
    fn test_port_mapping_parse_invalid() {
        assert!(PortMapping::parse("invalid").is_none());
        assert!(PortMapping::parse("abc:def").is_none());
        assert!(PortMapping::parse("").is_none());
    }

    #[test]
    fn test_port_mapping_display() {
        let pm = PortMapping {
            host_port: 8080,
            container_port: 80,
            protocol: String::from("tcp"),
        };
        let s = format!("{}", pm);
        assert!(s.contains("8080"));
        assert!(s.contains("80"));
        assert!(s.contains("tcp"));
    }

    // -- Container ------------------------------------------------------------

    #[test]
    fn test_container_new() {
        let c = Container::new("abc123def456", "web", "nginx:latest", "/bin/sh");
        assert_eq!(c.id, "abc123def456");
        assert_eq!(c.name, "web");
        assert_eq!(c.image, "nginx:latest");
        assert_eq!(c.command, "/bin/sh");
        assert_eq!(c.status, ContainerStatus::Created);
        assert!(c.ports.is_empty());
        assert!(c.labels.is_empty());
    }

    #[test]
    fn test_container_short_id() {
        let c = Container::new("abcdef123456789", "x", "i", "c");
        assert_eq!(c.short_id(), "abcdef123456");
    }

    #[test]
    fn test_container_short_id_short() {
        let c = Container::new("abc", "x", "i", "c");
        assert_eq!(c.short_id(), "abc");
    }

    // -- Image ----------------------------------------------------------------

    #[test]
    fn test_image_new() {
        let img = Image::new("abc123", "nginx", "latest", 100_000_000);
        assert_eq!(img.id, "abc123");
        assert_eq!(img.repository, "nginx");
        assert_eq!(img.tag, "latest");
        assert_eq!(img.size, 100_000_000);
    }

    #[test]
    fn test_image_full_name_with_tag() {
        let img = Image::new("abc", "nginx", "1.25", 100);
        assert_eq!(img.full_name(), "nginx:1.25");
    }

    #[test]
    fn test_image_full_name_latest() {
        let img = Image::new("abc", "nginx", "latest", 100);
        assert_eq!(img.full_name(), "nginx");
    }

    #[test]
    fn test_image_full_name_empty_tag() {
        let img = Image::new("abc", "nginx", "", 100);
        assert_eq!(img.full_name(), "nginx");
    }

    #[test]
    fn test_image_short_id() {
        let img = Image::new("abcdef123456789", "r", "t", 0);
        assert_eq!(img.short_id(), "abcdef123456");
    }

    #[test]
    fn test_image_human_size_bytes() {
        let img = Image::new("a", "r", "t", 512);
        assert_eq!(img.human_size(), "512 B");
    }

    #[test]
    fn test_image_human_size_kb() {
        let img = Image::new("a", "r", "t", 2048);
        assert_eq!(img.human_size(), "2.0 KB");
    }

    #[test]
    fn test_image_human_size_mb() {
        let img = Image::new("a", "r", "t", 75_000_000);
        assert_eq!(img.human_size(), "71.5 MB");
    }

    #[test]
    fn test_image_human_size_gb() {
        let img = Image::new("a", "r", "t", 2_000_000_000);
        assert_eq!(img.human_size(), "1.9 GB");
    }

    // -- Volume ---------------------------------------------------------------

    #[test]
    fn test_volume_new() {
        let v = Volume::new("mydata");
        assert_eq!(v.name, "mydata");
        assert_eq!(v.driver, "local");
        assert!(v.mountpoint.contains("mydata"));
    }

    // -- Network --------------------------------------------------------------

    #[test]
    fn test_network_new() {
        let n = Network::new("mynet");
        assert_eq!(n.name, "mynet");
        assert_eq!(n.driver, "bridge");
        assert_eq!(n.subnet, "10.88.0.0/16");
        assert_eq!(n.gateway, "10.88.0.1");
    }

    // -- PodStatus ------------------------------------------------------------

    #[test]
    fn test_pod_status_display() {
        assert_eq!(format!("{}", PodStatus::Created), "Created");
        assert_eq!(format!("{}", PodStatus::Running), "Running");
        assert_eq!(format!("{}", PodStatus::Paused), "Paused");
        assert_eq!(format!("{}", PodStatus::Stopped), "Stopped");
        assert_eq!(format!("{}", PodStatus::Degraded), "Degraded");
    }

    // -- Pod ------------------------------------------------------------------

    #[test]
    fn test_pod_new() {
        let p = Pod::new("pod123456789abc", "mypod");
        assert_eq!(p.name, "mypod");
        assert_eq!(p.status, PodStatus::Created);
        assert!(p.containers.is_empty());
    }

    #[test]
    fn test_pod_short_id() {
        let p = Pod::new("abcdef123456789", "p");
        assert_eq!(p.short_id(), "abcdef123456");
    }

    // -- Engine ---------------------------------------------------------------

    #[test]
    fn test_engine_new_has_default_network() {
        let eng = Engine::new();
        assert!(eng.find_network("podman").is_some());
    }

    #[test]
    fn test_engine_next_id_unique() {
        let mut eng = Engine::new();
        let a = eng.next_id();
        let b = eng.next_id();
        assert_ne!(a, b);
    }

    #[test]
    fn test_engine_create_container() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("web"), "/bin/sh", &[], &HashMap::new(), &HashMap::new(), None);
        assert!(!id.is_empty());
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.name, "web");
        assert_eq!(c.image, "nginx");
        assert_eq!(c.status, ContainerStatus::Created);
    }

    #[test]
    fn test_engine_create_container_auto_name() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", None, "", &[], &HashMap::new(), &HashMap::new(), None);
        let c = eng.find_container(&id).unwrap();
        assert!(c.name.starts_with("container_"));
    }

    #[test]
    fn test_engine_find_container_by_name() {
        let mut eng = Engine::new();
        let _id = eng.create_container("alpine", Some("finder"), "", &[], &HashMap::new(), &HashMap::new(), None);
        assert!(eng.find_container("finder").is_some());
    }

    #[test]
    fn test_engine_find_container_by_prefix() {
        let mut eng = Engine::new();
        let id = eng.create_container("alpine", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        let prefix = &id[..8];
        assert!(eng.find_container(prefix).is_some());
    }

    #[test]
    fn test_engine_start_container() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        assert!(eng.start_container(&id).is_ok());
        assert_eq!(eng.find_container(&id).unwrap().status, ContainerStatus::Running);
    }

    #[test]
    fn test_engine_start_already_running() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        eng.start_container(&id).unwrap();
        assert!(eng.start_container(&id).is_err());
    }

    #[test]
    fn test_engine_stop_container() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        eng.start_container(&id).unwrap();
        assert!(eng.stop_container(&id).is_ok());
        assert_eq!(eng.find_container(&id).unwrap().status, ContainerStatus::Exited);
    }

    #[test]
    fn test_engine_stop_not_running() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        assert!(eng.stop_container(&id).is_err());
    }

    #[test]
    fn test_engine_pause_container() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        eng.start_container(&id).unwrap();
        assert!(eng.pause_container(&id).is_ok());
        assert_eq!(eng.find_container(&id).unwrap().status, ContainerStatus::Paused);
    }

    #[test]
    fn test_engine_pause_not_running() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        assert!(eng.pause_container(&id).is_err());
    }

    #[test]
    fn test_engine_unpause_container() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        eng.start_container(&id).unwrap();
        eng.pause_container(&id).unwrap();
        assert!(eng.unpause_container(&id).is_ok());
        assert_eq!(eng.find_container(&id).unwrap().status, ContainerStatus::Running);
    }

    #[test]
    fn test_engine_unpause_not_paused() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        assert!(eng.unpause_container(&id).is_err());
    }

    #[test]
    fn test_engine_restart_container() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        assert!(eng.restart_container(&id).is_ok());
        assert_eq!(eng.find_container(&id).unwrap().status, ContainerStatus::Running);
    }

    #[test]
    fn test_engine_remove_container() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        assert!(eng.remove_container(&id, false).is_ok());
        assert!(eng.find_container(&id).is_none());
    }

    #[test]
    fn test_engine_remove_running_without_force() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        eng.start_container(&id).unwrap();
        assert!(eng.remove_container(&id, false).is_err());
    }

    #[test]
    fn test_engine_remove_running_with_force() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        eng.start_container(&id).unwrap();
        assert!(eng.remove_container(&id, true).is_ok());
    }

    #[test]
    fn test_engine_rename_container() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("old"), "", &[], &HashMap::new(), &HashMap::new(), None);
        assert!(eng.rename_container(&id, "new").is_ok());
        assert_eq!(eng.find_container(&id).unwrap().name, "new");
    }

    #[test]
    fn test_engine_rename_not_found() {
        let mut eng = Engine::new();
        assert!(eng.rename_container("nope", "new").is_err());
    }

    #[test]
    fn test_engine_wait_exited() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        eng.start_container(&id).unwrap();
        eng.stop_container(&id).unwrap();
        assert_eq!(eng.wait_container(&id).unwrap(), 0);
    }

    #[test]
    fn test_engine_wait_running() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        eng.start_container(&id).unwrap();
        assert_eq!(eng.wait_container(&id).unwrap(), -1);
    }

    #[test]
    fn test_engine_wait_not_found() {
        let eng = Engine::new();
        assert!(eng.wait_container("nope").is_err());
    }

    // -- Image engine operations ----------------------------------------------

    #[test]
    fn test_engine_pull_image() {
        let mut eng = Engine::new();
        let id = eng.pull_image("nginx:latest");
        assert!(!id.is_empty());
        let img = eng.find_image(&id).unwrap();
        assert_eq!(img.repository, "nginx");
        assert_eq!(img.tag, "latest");
    }

    #[test]
    fn test_engine_pull_image_no_tag() {
        let mut eng = Engine::new();
        let id = eng.pull_image("alpine");
        let img = eng.find_image(&id).unwrap();
        assert_eq!(img.repository, "alpine");
        assert_eq!(img.tag, "latest");
    }

    #[test]
    fn test_engine_tag_image() {
        let mut eng = Engine::new();
        let id = eng.pull_image("nginx:latest");
        assert!(eng.tag_image(&id, "myrepo:v1").is_ok());
        assert!(eng.find_image("myrepo").is_some());
    }

    #[test]
    fn test_engine_tag_image_not_found() {
        let mut eng = Engine::new();
        assert!(eng.tag_image("nope", "tag:v1").is_err());
    }

    #[test]
    fn test_engine_remove_image() {
        let mut eng = Engine::new();
        let id = eng.pull_image("nginx");
        assert!(eng.remove_image(&id).is_ok());
        assert!(eng.find_image(&id).is_none());
    }

    #[test]
    fn test_engine_remove_image_not_found() {
        let mut eng = Engine::new();
        assert!(eng.remove_image("nope").is_err());
    }

    #[test]
    fn test_engine_find_image_by_name() {
        let mut eng = Engine::new();
        let _id = eng.pull_image("alpine:3.18");
        assert!(eng.find_image("alpine").is_some());
    }

    #[test]
    fn test_engine_find_image_by_full_name() {
        let mut eng = Engine::new();
        let _id = eng.pull_image("alpine:3.18");
        assert!(eng.find_image("alpine:3.18").is_some());
    }

    // -- Volume engine operations ---------------------------------------------

    #[test]
    fn test_engine_create_volume() {
        let mut eng = Engine::new();
        let name = eng.create_volume("mydata");
        assert_eq!(name, "mydata");
        assert!(eng.find_volume("mydata").is_some());
    }

    #[test]
    fn test_engine_remove_volume() {
        let mut eng = Engine::new();
        eng.create_volume("mydata");
        assert!(eng.remove_volume("mydata").is_ok());
        assert!(eng.find_volume("mydata").is_none());
    }

    #[test]
    fn test_engine_remove_volume_not_found() {
        let mut eng = Engine::new();
        assert!(eng.remove_volume("nope").is_err());
    }

    // -- Network engine operations --------------------------------------------

    #[test]
    fn test_engine_create_network() {
        let mut eng = Engine::new();
        let name = eng.create_network("mynet", Some("192.168.1.0/24"), Some("192.168.1.1"));
        assert_eq!(name, "mynet");
        let net = eng.find_network("mynet").unwrap();
        assert_eq!(net.subnet, "192.168.1.0/24");
        assert_eq!(net.gateway, "192.168.1.1");
    }

    #[test]
    fn test_engine_create_network_defaults() {
        let mut eng = Engine::new();
        eng.create_network("net2", None, None);
        let net = eng.find_network("net2").unwrap();
        assert_eq!(net.subnet, "10.88.0.0/16");
    }

    #[test]
    fn test_engine_remove_network() {
        let mut eng = Engine::new();
        eng.create_network("mynet", None, None);
        assert!(eng.remove_network("mynet").is_ok());
        assert!(eng.find_network("mynet").is_none());
    }

    #[test]
    fn test_engine_remove_default_network() {
        let mut eng = Engine::new();
        assert!(eng.remove_network("podman").is_err());
    }

    #[test]
    fn test_engine_remove_network_not_found() {
        let mut eng = Engine::new();
        assert!(eng.remove_network("nope").is_err());
    }

    // -- Pod engine operations ------------------------------------------------

    #[test]
    fn test_engine_create_pod() {
        let mut eng = Engine::new();
        let id = eng.create_pod("mypod");
        assert!(!id.is_empty());
        let pod = eng.find_pod(&id).unwrap();
        assert_eq!(pod.name, "mypod");
        assert_eq!(pod.status, PodStatus::Created);
    }

    #[test]
    fn test_engine_start_pod() {
        let mut eng = Engine::new();
        let id = eng.create_pod("mypod");
        assert!(eng.start_pod(&id).is_ok());
        assert_eq!(eng.find_pod(&id).unwrap().status, PodStatus::Running);
    }

    #[test]
    fn test_engine_stop_pod() {
        let mut eng = Engine::new();
        let id = eng.create_pod("mypod");
        eng.start_pod(&id).unwrap();
        assert!(eng.stop_pod(&id).is_ok());
        assert_eq!(eng.find_pod(&id).unwrap().status, PodStatus::Stopped);
    }

    #[test]
    fn test_engine_remove_pod() {
        let mut eng = Engine::new();
        let id = eng.create_pod("mypod");
        assert!(eng.remove_pod(&id).is_ok());
        assert!(eng.find_pod(&id).is_none());
    }

    #[test]
    fn test_engine_remove_pod_not_found() {
        let mut eng = Engine::new();
        assert!(eng.remove_pod("nope").is_err());
    }

    #[test]
    fn test_engine_find_pod_by_name() {
        let mut eng = Engine::new();
        let _id = eng.create_pod("findme");
        assert!(eng.find_pod("findme").is_some());
    }

    #[test]
    fn test_engine_find_pod_by_prefix() {
        let mut eng = Engine::new();
        let id = eng.create_pod("pfx");
        let prefix = &id[..8];
        assert!(eng.find_pod(prefix).is_some());
    }

    #[test]
    fn test_engine_start_pod_not_found() {
        let mut eng = Engine::new();
        assert!(eng.start_pod("nope").is_err());
    }

    #[test]
    fn test_engine_stop_pod_not_found() {
        let mut eng = Engine::new();
        assert!(eng.stop_pod("nope").is_err());
    }

    // -- BuildahEngine --------------------------------------------------------

    #[test]
    fn test_buildah_from_image() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("ubuntu:22.04", None);
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.base_image, "ubuntu:22.04");
        assert!(c.name.starts_with("buildah-wc-"));
    }

    #[test]
    fn test_buildah_from_image_named() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("ubuntu:22.04", Some("mybuilder"));
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.name, "mybuilder");
    }

    #[test]
    fn test_buildah_mount() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        let mp = eng.mount_container(&id).unwrap();
        assert!(mp.contains(&id));
        assert!(eng.find_container(&id).unwrap().mounted);
    }

    #[test]
    fn test_buildah_unmount() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        eng.mount_container(&id).unwrap();
        assert!(eng.unmount_container(&id).is_ok());
        assert!(!eng.find_container(&id).unwrap().mounted);
    }

    #[test]
    fn test_buildah_unmount_not_mounted() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        assert!(eng.unmount_container(&id).is_err());
    }

    #[test]
    fn test_buildah_commit() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", Some("wc1"));
        let img_id = eng.commit_container(&id, "myimage:v1").unwrap();
        assert!(!img_id.is_empty());
        let img = eng.images.get(&img_id).unwrap();
        assert_eq!(img.repository, "myimage");
        assert_eq!(img.tag, "v1");
    }

    #[test]
    fn test_buildah_commit_not_found() {
        let mut eng = BuildahEngine::new();
        assert!(eng.commit_container("nope", "img").is_err());
    }

    #[test]
    fn test_buildah_config_cmd() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        assert!(eng.config_container(&id, "--cmd", "/bin/sh").is_ok());
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.config.cmd.as_deref(), Some("/bin/sh"));
    }

    #[test]
    fn test_buildah_config_entrypoint() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        assert!(eng.config_container(&id, "--entrypoint", "/app").is_ok());
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.config.entrypoint.as_deref(), Some("/app"));
    }

    #[test]
    fn test_buildah_config_workingdir() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        assert!(eng.config_container(&id, "--workingdir", "/app").is_ok());
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.config.working_dir.as_deref(), Some("/app"));
    }

    #[test]
    fn test_buildah_config_user() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        assert!(eng.config_container(&id, "--user", "nobody").is_ok());
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.config.user.as_deref(), Some("nobody"));
    }

    #[test]
    fn test_buildah_config_port() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        assert!(eng.config_container(&id, "--port", "8080").is_ok());
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.config.ports, vec!["8080"]);
    }

    #[test]
    fn test_buildah_config_volume() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        assert!(eng.config_container(&id, "--volume", "/data").is_ok());
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.config.volumes, vec!["/data"]);
    }

    #[test]
    fn test_buildah_config_unknown_key() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        assert!(eng.config_container(&id, "--unknown", "val").is_err());
    }

    #[test]
    fn test_buildah_config_not_found() {
        let mut eng = BuildahEngine::new();
        assert!(eng.config_container("nope", "--cmd", "x").is_err());
    }

    #[test]
    fn test_buildah_remove_container() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        assert!(eng.remove_container(&id).is_ok());
        assert!(eng.find_container(&id).is_none());
    }

    #[test]
    fn test_buildah_remove_not_found() {
        let mut eng = BuildahEngine::new();
        assert!(eng.remove_container("nope").is_err());
    }

    #[test]
    fn test_buildah_find_by_name() {
        let mut eng = BuildahEngine::new();
        let _id = eng.from_image("alpine", Some("byname"));
        assert!(eng.find_container("byname").is_some());
    }

    #[test]
    fn test_buildah_find_by_prefix() {
        let mut eng = BuildahEngine::new();
        let id = eng.from_image("alpine", None);
        let prefix = &id[..6];
        assert!(eng.find_container(prefix).is_some());
    }

    #[test]
    fn test_buildah_next_id_unique() {
        let mut eng = BuildahEngine::new();
        let a = eng.next_id();
        let b = eng.next_id();
        assert_ne!(a, b);
    }

    // -- ImageRef (skopeo) ----------------------------------------------------

    #[test]
    fn test_imageref_parse_docker() {
        let r = ImageRef::parse("docker://nginx:latest").unwrap();
        assert_eq!(r.transport, "docker");
        assert_eq!(r.reference, "nginx:latest");
    }

    #[test]
    fn test_imageref_parse_dir() {
        let r = ImageRef::parse("dir:/tmp/myimage").unwrap();
        assert_eq!(r.transport, "dir");
        assert_eq!(r.reference, "/tmp/myimage");
    }

    #[test]
    fn test_imageref_parse_oci() {
        let r = ImageRef::parse("oci:/tmp/oci:tag").unwrap();
        assert_eq!(r.transport, "oci");
        assert_eq!(r.reference, "/tmp/oci:tag");
    }

    #[test]
    fn test_imageref_parse_bare_name() {
        let r = ImageRef::parse("nginx").unwrap();
        assert_eq!(r.transport, "docker");
        assert_eq!(r.reference, "nginx");
    }

    #[test]
    fn test_imageref_display() {
        let r = ImageRef {
            transport: String::from("docker"),
            reference: String::from("nginx:latest"),
        };
        assert_eq!(r.display_name(), "docker://nginx:latest");
    }

    // -- InspectResult --------------------------------------------------------

    #[test]
    fn test_inspect_result_from_ref() {
        let r = ImageRef::parse("docker://nginx:1.25").unwrap();
        let info = InspectResult::from_ref(&r);
        assert_eq!(info.name, "nginx");
        assert_eq!(info.tag, "1.25");
        assert_eq!(info.architecture, "amd64");
        assert_eq!(info.os, "linux");
        assert!(!info.layers.is_empty());
    }

    #[test]
    fn test_inspect_result_default_tag() {
        let r = ImageRef::parse("alpine").unwrap();
        let info = InspectResult::from_ref(&r);
        assert_eq!(info.tag, "latest");
    }

    // -- CLI dispatch return codes --------------------------------------------

    #[test]
    fn test_podman_version() {
        let rc = run_podman(&[String::from("--version")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_help() {
        let rc = run_podman(&[String::from("--help")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_no_args() {
        let rc = run_podman(&[]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_unknown_command() {
        let rc = run_podman(&[String::from("nonexistent")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_ps() {
        let rc = run_podman(&[String::from("ps")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_ps_all() {
        let rc = run_podman(&[String::from("ps"), String::from("-a")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_images() {
        let rc = run_podman(&[String::from("images")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_pull() {
        let rc = run_podman(&[String::from("pull"), String::from("nginx")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_pull_no_args() {
        let rc = run_podman(&[String::from("pull")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_run_detach() {
        let rc = run_podman(&[
            String::from("run"),
            String::from("-d"),
            String::from("--name"),
            String::from("web"),
            String::from("nginx"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_run_no_args() {
        let rc = run_podman(&[String::from("run")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_create() {
        let rc = run_podman(&[
            String::from("create"),
            String::from("--name"),
            String::from("c1"),
            String::from("alpine"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_create_no_image() {
        let rc = run_podman(&[String::from("create")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_start_no_args() {
        let rc = run_podman(&[String::from("start")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_stop_no_args() {
        let rc = run_podman(&[String::from("stop")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_rm_no_args() {
        let rc = run_podman(&[String::from("rm")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_exec_no_args() {
        let rc = run_podman(&[String::from("exec")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_inspect_no_args() {
        let rc = run_podman(&[String::from("inspect")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_inspect_target() {
        let rc = run_podman(&[String::from("inspect"), String::from("mycontainer")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_logs_no_args() {
        let rc = run_podman(&[String::from("logs")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_top_no_args() {
        let rc = run_podman(&[String::from("top")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_port_no_args() {
        let rc = run_podman(&[String::from("port")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_diff_no_args() {
        let rc = run_podman(&[String::from("diff")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_export_no_args() {
        let rc = run_podman(&[String::from("export")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_import_file() {
        let rc = run_podman(&[String::from("import"), String::from("archive.tar")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_rename_no_args() {
        let rc = run_podman(&[String::from("rename")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_rename_ok() {
        let rc = run_podman(&[
            String::from("rename"),
            String::from("old"),
            String::from("new"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_stats() {
        let rc = run_podman(&[String::from("stats"), String::from("--no-stream")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_wait_no_args() {
        let rc = run_podman(&[String::from("wait")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_tag_no_args() {
        let rc = run_podman(&[String::from("tag")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_tag_ok() {
        let rc = run_podman(&[
            String::from("tag"),
            String::from("nginx"),
            String::from("myrepo:v1"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_commit_no_args() {
        let rc = run_podman(&[String::from("commit")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_push_no_args() {
        let rc = run_podman(&[String::from("push")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_attach_no_args() {
        let rc = run_podman(&[String::from("attach")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_restart_no_args() {
        let rc = run_podman(&[String::from("restart")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_pause_no_args() {
        let rc = run_podman(&[String::from("pause")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_unpause_no_args() {
        let rc = run_podman(&[String::from("unpause")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_login_no_creds() {
        let rc = run_podman(&[String::from("login")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_login_with_creds() {
        let rc = run_podman(&[
            String::from("login"),
            String::from("-u"),
            String::from("user"),
            String::from("-p"),
            String::from("pass"),
            String::from("quay.io"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_logout() {
        let rc = run_podman(&[String::from("logout")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_search_no_args() {
        let rc = run_podman(&[String::from("search")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_search_ok() {
        let rc = run_podman(&[String::from("search"), String::from("nginx")]);
        assert_eq!(rc, 0);
    }

    // -- Pod subcommands ------------------------------------------------------

    #[test]
    fn test_podman_pod_no_args() {
        let rc = run_podman(&[String::from("pod")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_pod_create() {
        let rc = run_podman(&[
            String::from("pod"),
            String::from("create"),
            String::from("--name"),
            String::from("mypod"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_pod_ps() {
        let rc = run_podman(&[String::from("pod"), String::from("ps")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_pod_start_no_args() {
        let rc = run_podman(&[String::from("pod"), String::from("start")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_pod_stop_no_args() {
        let rc = run_podman(&[String::from("pod"), String::from("stop")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_pod_rm_no_args() {
        let rc = run_podman(&[String::from("pod"), String::from("rm")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_pod_inspect_no_args() {
        let rc = run_podman(&[String::from("pod"), String::from("inspect")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_pod_inspect_ok() {
        let rc = run_podman(&[
            String::from("pod"),
            String::from("inspect"),
            String::from("mypod"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_pod_unknown() {
        let rc = run_podman(&[String::from("pod"), String::from("unknown")]);
        assert_eq!(rc, 1);
    }

    // -- Volume subcommands ---------------------------------------------------

    #[test]
    fn test_podman_volume_no_args() {
        let rc = run_podman(&[String::from("volume")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_volume_create() {
        let rc = run_podman(&[
            String::from("volume"),
            String::from("create"),
            String::from("myvol"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_volume_ls() {
        let rc = run_podman(&[String::from("volume"), String::from("ls")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_volume_rm_no_args() {
        let rc = run_podman(&[String::from("volume"), String::from("rm")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_volume_inspect_no_args() {
        let rc = run_podman(&[String::from("volume"), String::from("inspect")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_volume_inspect_ok() {
        let rc = run_podman(&[
            String::from("volume"),
            String::from("inspect"),
            String::from("myvol"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_volume_unknown() {
        let rc = run_podman(&[String::from("volume"), String::from("unknown")]);
        assert_eq!(rc, 1);
    }

    // -- Network subcommands --------------------------------------------------

    #[test]
    fn test_podman_network_no_args() {
        let rc = run_podman(&[String::from("network")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_network_create() {
        let rc = run_podman(&[
            String::from("network"),
            String::from("create"),
            String::from("mynet"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_network_ls() {
        let rc = run_podman(&[String::from("network"), String::from("ls")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_network_rm_no_args() {
        let rc = run_podman(&[String::from("network"), String::from("rm")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_network_inspect_no_args() {
        let rc = run_podman(&[String::from("network"), String::from("inspect")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_network_inspect_ok() {
        let rc = run_podman(&[
            String::from("network"),
            String::from("inspect"),
            String::from("mynet"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_network_unknown() {
        let rc = run_podman(&[String::from("network"), String::from("unknown")]);
        assert_eq!(rc, 1);
    }

    // -- System subcommands ---------------------------------------------------

    #[test]
    fn test_podman_system_no_args() {
        let rc = run_podman(&[String::from("system")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_system_info() {
        let rc = run_podman(&[String::from("system"), String::from("info")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_system_prune() {
        let rc = run_podman(&[String::from("system"), String::from("prune")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_system_df() {
        let rc = run_podman(&[String::from("system"), String::from("df")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_system_unknown() {
        let rc = run_podman(&[String::from("system"), String::from("unknown")]);
        assert_eq!(rc, 1);
    }

    // -- Generate subcommands -------------------------------------------------

    #[test]
    fn test_podman_generate_no_args() {
        let rc = run_podman(&[String::from("generate")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_generate_systemd() {
        let rc = run_podman(&[
            String::from("generate"),
            String::from("systemd"),
            String::from("mycontainer"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_generate_kube() {
        let rc = run_podman(&[
            String::from("generate"),
            String::from("kube"),
            String::from("mypod"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_generate_unknown() {
        let rc = run_podman(&[String::from("generate"), String::from("unknown")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_generate_systemd_no_container() {
        let rc = run_podman(&[String::from("generate"), String::from("systemd")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_podman_generate_kube_no_target() {
        let rc = run_podman(&[String::from("generate"), String::from("kube")]);
        assert_eq!(rc, 1);
    }

    // -- Buildah CLI ----------------------------------------------------------

    #[test]
    fn test_buildah_version() {
        let rc = run_buildah(&[String::from("--version")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_help() {
        let rc = run_buildah(&[String::from("--help")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_no_args() {
        let rc = run_buildah(&[]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_unknown() {
        let rc = run_buildah(&[String::from("unknown")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_from_cli() {
        let rc = run_buildah(&[String::from("from"), String::from("alpine")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_from_no_args() {
        let rc = run_buildah(&[String::from("from")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_run_cli() {
        let rc = run_buildah(&[
            String::from("run"),
            String::from("wc1"),
            String::from("ls"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_run_no_args() {
        let rc = run_buildah(&[String::from("run")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_copy_cli() {
        let rc = run_buildah(&[
            String::from("copy"),
            String::from("wc1"),
            String::from("file.txt"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_copy_no_args() {
        let rc = run_buildah(&[String::from("copy")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_add_cli() {
        let rc = run_buildah(&[
            String::from("add"),
            String::from("wc1"),
            String::from("archive.tar"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_add_no_args() {
        let rc = run_buildah(&[String::from("add")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_commit_cli() {
        let rc = run_buildah(&[
            String::from("commit"),
            String::from("wc1"),
            String::from("myimage:v1"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_commit_no_args() {
        let rc = run_buildah(&[String::from("commit")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_config_cli() {
        let rc = run_buildah(&[
            String::from("config"),
            String::from("--cmd"),
            String::from("/bin/sh"),
            String::from("wc1"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_config_no_args() {
        let rc = run_buildah(&[String::from("config")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_push_cli() {
        let rc = run_buildah(&[String::from("push"), String::from("myimage")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_push_no_args() {
        let rc = run_buildah(&[String::from("push")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_tag_cli() {
        let rc = run_buildah(&[
            String::from("tag"),
            String::from("img"),
            String::from("repo:v1"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_tag_no_args() {
        let rc = run_buildah(&[String::from("tag")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_images_cli() {
        let rc = run_buildah(&[String::from("images")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_rm_cli() {
        let rc = run_buildah(&[String::from("rm"), String::from("wc1")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_rm_no_args() {
        let rc = run_buildah(&[String::from("rm")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_containers_cli() {
        let rc = run_buildah(&[String::from("containers")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_mount_cli() {
        let rc = run_buildah(&[String::from("mount"), String::from("wc1")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_mount_no_args() {
        let rc = run_buildah(&[String::from("mount")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_unmount_cli() {
        let rc = run_buildah(&[String::from("unmount"), String::from("wc1")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_unmount_no_args() {
        let rc = run_buildah(&[String::from("unmount")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_inspect_cli() {
        let rc = run_buildah(&[String::from("inspect"), String::from("wc1")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_buildah_inspect_no_args() {
        let rc = run_buildah(&[String::from("inspect")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_buildah_umount_alias() {
        let rc = run_buildah(&[String::from("umount"), String::from("wc1")]);
        assert_eq!(rc, 0);
    }

    // -- Skopeo CLI -----------------------------------------------------------

    #[test]
    fn test_skopeo_version() {
        let rc = run_skopeo(&[String::from("--version")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_skopeo_help() {
        let rc = run_skopeo(&[String::from("--help")]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_skopeo_no_args() {
        let rc = run_skopeo(&[]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_skopeo_unknown() {
        let rc = run_skopeo(&[String::from("unknown")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_skopeo_copy_cli() {
        let rc = run_skopeo(&[
            String::from("copy"),
            String::from("docker://nginx"),
            String::from("dir:/tmp/nginx"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_skopeo_copy_no_args() {
        let rc = run_skopeo(&[String::from("copy")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_skopeo_inspect_cli() {
        let rc = run_skopeo(&[
            String::from("inspect"),
            String::from("docker://nginx:latest"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_skopeo_inspect_no_args() {
        let rc = run_skopeo(&[String::from("inspect")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_skopeo_inspect_raw() {
        let rc = run_skopeo(&[
            String::from("inspect"),
            String::from("--raw"),
            String::from("docker://nginx"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_skopeo_delete_cli() {
        let rc = run_skopeo(&[
            String::from("delete"),
            String::from("docker://nginx:old"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_skopeo_delete_no_args() {
        let rc = run_skopeo(&[String::from("delete")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_skopeo_list_tags_cli() {
        let rc = run_skopeo(&[
            String::from("list-tags"),
            String::from("docker://nginx"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_skopeo_list_tags_no_args() {
        let rc = run_skopeo(&[String::from("list-tags")]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_skopeo_sync_cli() {
        let rc = run_skopeo(&[
            String::from("sync"),
            String::from("--src"),
            String::from("docker"),
            String::from("--dest"),
            String::from("dir"),
            String::from("nginx"),
            String::from("/tmp/sync"),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_skopeo_sync_no_args() {
        let rc = run_skopeo(&[String::from("sync")]);
        assert_eq!(rc, 1);
    }

    // -- Container with ports -------------------------------------------------

    #[test]
    fn test_engine_container_with_ports() {
        let mut eng = Engine::new();
        let ports = vec![PortMapping {
            host_port: 8080,
            container_port: 80,
            protocol: String::from("tcp"),
        }];
        let id = eng.create_container("nginx", Some("web"), "", &ports, &HashMap::new(), &HashMap::new(), None);
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.ports.len(), 1);
        assert_eq!(c.ports[0].host_port, 8080);
    }

    #[test]
    fn test_engine_container_with_labels() {
        let mut eng = Engine::new();
        let mut labels = HashMap::new();
        labels.insert(String::from("app"), String::from("web"));
        let id = eng.create_container("nginx", Some("labeled"), "", &[], &labels, &HashMap::new(), None);
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.labels.get("app").map(|s| s.as_str()), Some("web"));
    }

    #[test]
    fn test_engine_container_with_env() {
        let mut eng = Engine::new();
        let mut env_vars = HashMap::new();
        env_vars.insert(String::from("PORT"), String::from("3000"));
        let id = eng.create_container("node", Some("app"), "", &[], &HashMap::new(), &env_vars, None);
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.env_vars.get("PORT").map(|s| s.as_str()), Some("3000"));
    }

    #[test]
    fn test_engine_container_with_pod() {
        let mut eng = Engine::new();
        let pod_id = eng.create_pod("mypod");
        let id = eng.create_container("nginx", Some("in-pod"), "", &[], &HashMap::new(), &HashMap::new(), Some(&pod_id));
        let c = eng.find_container(&id).unwrap();
        assert_eq!(c.pod_id.as_deref(), Some(pod_id.as_str()));
    }

    // -- Podman run with port publish -----------------------------------------

    #[test]
    fn test_podman_run_with_port() {
        let rc = run_podman(&[
            String::from("run"),
            String::from("-d"),
            String::from("-p"),
            String::from("8080:80"),
            String::from("nginx"),
        ]);
        assert_eq!(rc, 0);
    }

    // -- Podman build with tag ------------------------------------------------

    #[test]
    fn test_podman_build_with_tag() {
        let rc = run_podman(&[
            String::from("build"),
            String::from("-t"),
            String::from("myapp:v1"),
            String::from("."),
        ]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_podman_build_no_args() {
        let rc = run_podman(&[String::from("build")]);
        assert_eq!(rc, 0);
    }

    // -- Multiple containers --------------------------------------------------

    #[test]
    fn test_engine_multiple_containers() {
        let mut eng = Engine::new();
        let id1 = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        let id2 = eng.create_container("alpine", Some("c2"), "", &[], &HashMap::new(), &HashMap::new(), None);
        assert_ne!(id1, id2);
        assert!(eng.find_container("c1").is_some());
        assert!(eng.find_container("c2").is_some());
    }

    #[test]
    fn test_engine_start_paused_fails() {
        let mut eng = Engine::new();
        let id = eng.create_container("nginx", Some("c1"), "", &[], &HashMap::new(), &HashMap::new(), None);
        eng.start_container(&id).unwrap();
        eng.pause_container(&id).unwrap();
        assert!(eng.start_container(&id).is_err());
    }

    // -- Helper function for personality detection in tests --------------------

    fn detect_personality(argv0: &str) -> String {
        let bytes = argv0.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &argv0[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    }
}

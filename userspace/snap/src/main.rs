//! Multi-personality snap package management utility for SlateOS.
//!
//! This binary detects its personality from `argv[0]`:
//!   - `snap`          — snap package management CLI
//!   - `snapd`         — snap daemon
//!   - `snap-confine`  — snap confinement helper
//!
//! The snap CLI provides subcommands for installing, removing, refreshing,
//! and managing snap packages along with their interfaces, services,
//! channels, and confinement settings.

#![deny(clippy::all)]
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

const CHANNELS: &[&str] = &["stable", "candidate", "beta", "edge"];

// ============================================================================
// Personality detection
// ============================================================================

/// Which personality this invocation runs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Personality {
    Snap,
    Snapd,
    SnapConfine,
}

impl Personality {
    fn name(self) -> &'static str {
        match self {
            Self::Snap => "snap",
            Self::Snapd => "snapd",
            Self::SnapConfine => "snap-confine",
        }
    }
}

/// Extract personality from argv[0] basename.
fn detect_personality(argv0: &str) -> Personality {
    let base = basename(argv0);
    let stem = base.strip_suffix(".exe").unwrap_or(base);
    match stem {
        "snapd" => Personality::Snapd,
        "snap-confine" => Personality::SnapConfine,
        _ => Personality::Snap,
    }
}

/// Return the filename portion of a path.
fn basename(path: &str) -> &str {
    let after_slash = match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    };
    match after_slash.rfind('\\') {
        Some(i) => &after_slash[i + 1..],
        None => after_slash,
    }
}

// ============================================================================
// Snap types
// ============================================================================

/// Type of a snap package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapType {
    App,
    Gadget,
    Kernel,
    Core,
    Base,
}

impl SnapType {
    fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Gadget => "gadget",
            Self::Kernel => "kernel",
            Self::Core => "core",
            Self::Base => "base",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "app" => Some(Self::App),
            "gadget" => Some(Self::Gadget),
            "kernel" => Some(Self::Kernel),
            "core" => Some(Self::Core),
            "base" => Some(Self::Base),
            _ => None,
        }
    }
}

/// Status of an installed snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapStatus {
    Installed,
    Active,
    Disabled,
}

impl SnapStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "installed" => Some(Self::Installed),
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// Confinement model for a snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Confinement {
    Strict,
    Classic,
    Devmode,
}

impl Confinement {
    fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Classic => "classic",
            Self::Devmode => "devmode",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "strict" => Some(Self::Strict),
            "classic" => Some(Self::Classic),
            "devmode" => Some(Self::Devmode),
            _ => None,
        }
    }
}

/// A snap package entry.
#[derive(Clone, Debug)]
struct SnapInfo {
    name: String,
    version: String,
    revision: u64,
    channel: String,
    developer: String,
    snap_type: SnapType,
    status: SnapStatus,
    confinement: Confinement,
    install_date: String,
    size: u64,
    summary: String,
    description: String,
}

impl SnapInfo {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: "0.0.0".to_string(),
            revision: 1,
            channel: "stable".to_string(),
            developer: "unknown".to_string(),
            snap_type: SnapType::App,
            status: SnapStatus::Active,
            confinement: Confinement::Strict,
            install_date: "2025-01-01T00:00:00Z".to_string(),
            size: 0,
            summary: String::new(),
            description: String::new(),
        }
    }
}

// ============================================================================
// Interface types
// ============================================================================

/// A snap interface connection.
#[derive(Clone, Debug)]
struct Interface {
    name: String,
    slot_snap: String,
    slot_name: String,
    plug_snap: String,
    plug_name: String,
    auto_connect: bool,
}

impl Interface {
    fn new(name: &str, slot_snap: &str, slot_name: &str, plug_snap: &str, plug_name: &str) -> Self {
        Self {
            name: name.to_string(),
            slot_snap: slot_snap.to_string(),
            slot_name: slot_name.to_string(),
            plug_snap: plug_snap.to_string(),
            plug_name: plug_name.to_string(),
            auto_connect: false,
        }
    }
}

// ============================================================================
// Service types
// ============================================================================

/// Status of a snap service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServiceStatus {
    Running,
    Stopped,
    Failed,
}

impl ServiceStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

/// A snap service entry.
#[derive(Clone, Debug)]
struct SnapService {
    snap_name: String,
    service_name: String,
    status: ServiceStatus,
    enabled: bool,
}

// ============================================================================
// Change / task tracking
// ============================================================================

/// Status of a change operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChangeStatus {
    Doing,
    Done,
    Error,
    Undone,
    Aborted,
}

impl ChangeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Doing => "Doing",
            Self::Done => "Done",
            Self::Error => "Error",
            Self::Undone => "Undone",
            Self::Aborted => "Aborted",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "Doing" | "doing" => Some(Self::Doing),
            "Done" | "done" => Some(Self::Done),
            "Error" | "error" => Some(Self::Error),
            "Undone" | "undone" => Some(Self::Undone),
            "Aborted" | "aborted" => Some(Self::Aborted),
            _ => None,
        }
    }
}

/// A tracked change (snap operation in progress or completed).
#[derive(Clone, Debug)]
struct Change {
    id: u64,
    status: ChangeStatus,
    kind: String,
    summary: String,
    tasks: Vec<Task>,
    ready_time: Option<String>,
    spawn_time: String,
}

/// A task within a change.
#[derive(Clone, Debug)]
struct Task {
    id: u64,
    kind: String,
    summary: String,
    status: ChangeStatus,
    progress_done: u64,
    progress_total: u64,
}

// ============================================================================
// Alias tracking
// ============================================================================

/// A snap command alias.
#[derive(Clone, Debug)]
struct Alias {
    snap_name: String,
    alias_name: String,
    command: String,
    status: AliasStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AliasStatus {
    Manual,
    Auto,
    Disabled,
}

impl AliasStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
            Self::Disabled => "disabled",
        }
    }
}

// ============================================================================
// Snapshot types
// ============================================================================

/// A snap snapshot (save/restore).
#[derive(Clone, Debug)]
struct Snapshot {
    id: u64,
    snap_name: String,
    version: String,
    revision: u64,
    size: u64,
    timestamp: String,
}

// ============================================================================
// Assertion types
// ============================================================================

/// A snap assertion record.
#[derive(Clone, Debug)]
struct Assertion {
    assertion_type: String,
    headers: BTreeMap<String, String>,
    body: String,
}

// ============================================================================
// Store search result
// ============================================================================

/// A result from a store search.
#[derive(Clone, Debug)]
struct StoreResult {
    name: String,
    version: String,
    publisher: String,
    summary: String,
    snap_type: SnapType,
    channels: Vec<String>,
    confinement: Confinement,
}

// ============================================================================
// Debug info types
// ============================================================================

/// Debug connectivity result.
#[derive(Clone, Debug)]
struct ConnectivityResult {
    reachable: bool,
    latency_ms: u64,
    store_url: String,
}

/// Debug timing entry.
#[derive(Clone, Debug)]
struct TimingEntry {
    label: String,
    duration_ms: u64,
}

// ============================================================================
// Session / auth
// ============================================================================

/// User authentication state.
#[derive(Clone, Debug)]
struct AuthState {
    logged_in: bool,
    email: String,
    macaroon: String,
}

impl AuthState {
    fn empty() -> Self {
        Self {
            logged_in: false,
            email: String::new(),
            macaroon: String::new(),
        }
    }
}

// ============================================================================
// SnapState — the runtime database
// ============================================================================

/// Central state for the snap system.
struct SnapState {
    snaps: BTreeMap<String, SnapInfo>,
    interfaces: Vec<Interface>,
    services: Vec<SnapService>,
    changes: Vec<Change>,
    aliases: Vec<Alias>,
    snapshots: Vec<Snapshot>,
    assertions: Vec<Assertion>,
    store_catalog: Vec<StoreResult>,
    auth: AuthState,
    config: BTreeMap<String, BTreeMap<String, String>>,
    next_change_id: u64,
    next_task_id: u64,
    next_snapshot_id: u64,
}

impl SnapState {
    fn new() -> Self {
        let mut state = Self {
            snaps: BTreeMap::new(),
            interfaces: Vec::new(),
            services: Vec::new(),
            changes: Vec::new(),
            aliases: Vec::new(),
            snapshots: Vec::new(),
            assertions: Vec::new(),
            store_catalog: Vec::new(),
            auth: AuthState::empty(),
            config: BTreeMap::new(),
            next_change_id: 1,
            next_task_id: 1,
            next_snapshot_id: 1,
        };
        state.seed_defaults();
        state
    }

    fn seed_defaults(&mut self) {
        // Pre-installed core snaps
        let mut core = SnapInfo::new("core22");
        core.version = "20240101".to_string();
        core.revision = 1380;
        core.snap_type = SnapType::Base;
        core.developer = "canonical".to_string();
        core.summary = "Runtime environment based on Ubuntu 22.04".to_string();
        core.size = 67_108_864;
        self.snaps.insert("core22".to_string(), core);

        let mut snapd_snap = SnapInfo::new("snapd");
        snapd_snap.version = "2.63".to_string();
        snapd_snap.revision = 21465;
        snapd_snap.snap_type = SnapType::Core;
        snapd_snap.developer = "canonical".to_string();
        snapd_snap.summary = "Snap daemon".to_string();
        snapd_snap.size = 41_943_040;
        self.snaps.insert("snapd".to_string(), snapd_snap);

        // Some store catalog entries
        self.store_catalog.push(StoreResult {
            name: "firefox".to_string(),
            version: "130.0".to_string(),
            publisher: "mozilla".to_string(),
            summary: "Mozilla Firefox web browser".to_string(),
            snap_type: SnapType::App,
            channels: vec!["stable".to_string(), "beta".to_string(), "edge".to_string()],
            confinement: Confinement::Strict,
        });
        self.store_catalog.push(StoreResult {
            name: "vlc".to_string(),
            version: "3.0.20".to_string(),
            publisher: "videolan".to_string(),
            summary: "VLC media player".to_string(),
            snap_type: SnapType::App,
            channels: vec!["stable".to_string(), "edge".to_string()],
            confinement: Confinement::Strict,
        });
        self.store_catalog.push(StoreResult {
            name: "chromium".to_string(),
            version: "128.0".to_string(),
            publisher: "canonical".to_string(),
            summary: "Chromium web browser".to_string(),
            snap_type: SnapType::App,
            channels: vec!["stable".to_string(), "candidate".to_string(), "beta".to_string()],
            confinement: Confinement::Strict,
        });
        self.store_catalog.push(StoreResult {
            name: "gimp".to_string(),
            version: "2.10.36".to_string(),
            publisher: "snapcrafters".to_string(),
            summary: "GNU Image Manipulation Program".to_string(),
            snap_type: SnapType::App,
            channels: vec!["stable".to_string()],
            confinement: Confinement::Strict,
        });
        self.store_catalog.push(StoreResult {
            name: "code".to_string(),
            version: "1.92.0".to_string(),
            publisher: "microsoft".to_string(),
            summary: "Visual Studio Code".to_string(),
            snap_type: SnapType::App,
            channels: vec!["stable".to_string(), "beta".to_string(), "edge".to_string()],
            confinement: Confinement::Classic,
        });

        // Default interfaces
        self.interfaces.push(Interface::new(
            "network", "core", "network", "core22", "network",
        ));
        self.interfaces.push(Interface::new(
            "home", "core", "home", "core22", "home",
        ));

        // Completed change
        self.changes.push(Change {
            id: 1,
            status: ChangeStatus::Done,
            kind: "install-snap".to_string(),
            summary: "Install snap \"core22\"".to_string(),
            tasks: vec![
                Task {
                    id: 1,
                    kind: "download-snap".to_string(),
                    summary: "Download snap core22 from channel stable".to_string(),
                    status: ChangeStatus::Done,
                    progress_done: 100,
                    progress_total: 100,
                },
                Task {
                    id: 2,
                    kind: "mount-snap".to_string(),
                    summary: "Mount snap core22 (1380)".to_string(),
                    status: ChangeStatus::Done,
                    progress_done: 1,
                    progress_total: 1,
                },
            ],
            ready_time: Some("2025-01-01T00:01:00Z".to_string()),
            spawn_time: "2025-01-01T00:00:00Z".to_string(),
        });
        self.next_change_id = 2;
        self.next_task_id = 3;
    }

    // ========================================================================
    // Change/task helpers
    // ========================================================================

    fn add_change(&mut self, kind: &str, summary: &str, task_summaries: &[&str]) -> u64 {
        let cid = self.next_change_id;
        self.next_change_id += 1;
        let mut tasks = Vec::new();
        for ts in task_summaries {
            let tid = self.next_task_id;
            self.next_task_id += 1;
            tasks.push(Task {
                id: tid,
                kind: kind.to_string(),
                summary: ts.to_string(),
                status: ChangeStatus::Done,
                progress_done: 1,
                progress_total: 1,
            });
        }
        self.changes.push(Change {
            id: cid,
            status: ChangeStatus::Done,
            kind: kind.to_string(),
            summary: summary.to_string(),
            tasks,
            ready_time: Some("2025-01-01T00:05:00Z".to_string()),
            spawn_time: "2025-01-01T00:04:00Z".to_string(),
        });
        cid
    }

    // ========================================================================
    // Install
    // ========================================================================

    fn install_snap(&mut self, name: &str, channel: Option<&str>, confinement: Option<Confinement>) -> Result<String, String> {
        if self.snaps.contains_key(name) {
            return Err(format!("snap \"{}\" is already installed", name));
        }
        let ch = channel.unwrap_or("stable");
        if !CHANNELS.contains(&ch) {
            return Err(format!("unknown channel: {}", ch));
        }
        // Look up in store
        let store_entry = self.store_catalog.iter().find(|e| e.name == name);
        let mut snap = SnapInfo::new(name);
        snap.channel = ch.to_string();
        if let Some(entry) = store_entry {
            snap.version = entry.version.clone();
            snap.developer = entry.publisher.clone();
            snap.summary = entry.summary.clone();
            snap.snap_type = entry.snap_type;
            snap.confinement = entry.confinement;
        }
        if let Some(c) = confinement {
            snap.confinement = c;
        }
        snap.revision = 100;
        snap.size = 52_428_800;
        snap.status = SnapStatus::Active;
        let msg = format!("{} {} from '{}' ({}) installed", name, snap.version, snap.developer, ch);
        self.snaps.insert(name.to_string(), snap);

        let summary_str = format!("Install snap \"{}\"", name);
        let task_strs = [
            &format!("Download snap {} from channel {}", name, ch) as &str,
            &format!("Mount snap {} (100)", name),
            &format!("Setup snap {} (100) security profiles", name),
        ];
        // Collect to avoid borrow conflict
        let task_refs: Vec<&str> = task_strs.to_vec();
        self.add_change("install-snap", &summary_str, &task_refs);
        Ok(msg)
    }

    // ========================================================================
    // Remove
    // ========================================================================

    fn remove_snap(&mut self, name: &str) -> Result<String, String> {
        if self.snaps.remove(name).is_none() {
            return Err(format!("snap \"{}\" is not installed", name));
        }
        // Remove associated services
        self.services.retain(|s| s.snap_name != name);
        // Remove associated aliases
        self.aliases.retain(|a| a.snap_name != name);
        let summary_str = format!("Remove snap \"{}\"", name);
        self.add_change("remove-snap", &summary_str, &[
            &format!("Remove data for snap {}", name),
            &format!("Discard snap {}", name),
        ]);
        Ok(format!("{} removed", name))
    }

    // ========================================================================
    // Refresh
    // ========================================================================

    fn refresh_snap(&mut self, name: &str, channel: Option<&str>) -> Result<String, String> {
        let snap = self.snaps.get_mut(name)
            .ok_or_else(|| format!("snap \"{}\" is not installed", name))?;
        if let Some(ch) = channel {
            if !CHANNELS.contains(&ch) {
                return Err(format!("unknown channel: {}", ch));
            }
            snap.channel = ch.to_string();
        }
        snap.revision += 1;
        let rev = snap.revision;
        let ver = snap.version.clone();
        let ch = snap.channel.clone();
        let summary_str = format!("Refresh snap \"{}\"", name);
        self.add_change("refresh-snap", &summary_str, &[
            &format!("Download snap {} from channel {}", name, ch),
            &format!("Mount snap {} ({})", name, rev),
        ]);
        Ok(format!("{} refreshed to {} (rev {})", name, ver, rev))
    }

    fn refresh_all(&mut self) -> Vec<String> {
        let names: Vec<String> = self.snaps.keys().cloned().collect();
        let mut results = Vec::new();
        for name in &names {
            if let Ok(msg) = self.refresh_snap(name, None) {
                results.push(msg);
            }
        }
        if results.is_empty() {
            results.push("All snaps up to date.".to_string());
        }
        results
    }

    // ========================================================================
    // Revert
    // ========================================================================

    fn revert_snap(&mut self, name: &str) -> Result<String, String> {
        let snap = self.snaps.get_mut(name)
            .ok_or_else(|| format!("snap \"{}\" is not installed", name))?;
        if snap.revision <= 1 {
            return Err(format!("snap \"{}\" has no earlier revision to revert to", name));
        }
        snap.revision -= 1;
        let rev = snap.revision;
        let summary_str = format!("Revert snap \"{}\"", name);
        self.add_change("revert-snap", &summary_str, &[
            &format!("Revert snap {} to revision {}", name, rev),
        ]);
        Ok(format!("{} reverted to revision {}", name, rev))
    }

    // ========================================================================
    // List
    // ========================================================================

    fn list_snaps(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!(
            "{:<20} {:<12} {:<8} {:<12} {:<12} {:<10} Notes",
            "Name", "Version", "Rev", "Tracking", "Publisher", "Status"
        ));
        for snap in self.snaps.values() {
            let notes = snap.confinement.as_str();
            lines.push(format!(
                "{:<20} {:<12} {:<8} {:<12} {:<12} {:<10} {}",
                snap.name, snap.version, snap.revision, snap.channel,
                snap.developer, snap.status.as_str(), notes
            ));
        }
        lines
    }

    // ========================================================================
    // Find (store search)
    // ========================================================================

    fn find_snaps(&self, query: &str, section: Option<&str>) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!(
            "{:<20} {:<12} {:<15} {:<10} Summary",
            "Name", "Version", "Publisher", "Confinement"
        ));
        for entry in &self.store_catalog {
            let name_match = entry.name.contains(query) || entry.summary.to_lowercase().contains(&query.to_lowercase());
            let section_match = section.is_none_or(|_s| true);
            if name_match && section_match {
                lines.push(format!(
                    "{:<20} {:<12} {:<15} {:<10} {}",
                    entry.name, entry.version, entry.publisher,
                    entry.confinement.as_str(), entry.summary
                ));
            }
        }
        if lines.len() == 1 {
            lines.push("No matching snaps found.".to_string());
        }
        lines
    }

    // ========================================================================
    // Info
    // ========================================================================

    fn snap_info(&self, name: &str) -> Result<Vec<String>, String> {
        // Check installed first
        if let Some(snap) = self.snaps.get(name) {
            let mut lines = Vec::new();
            lines.push(format!("name:         {}", snap.name));
            lines.push(format!("summary:      {}", snap.summary));
            lines.push(format!("publisher:    {}", snap.developer));
            lines.push(format!("store-url:    https://snapcraft.io/{}", snap.name));
            lines.push("license:      unset".to_string());
            lines.push(format!("description:  {}", if snap.description.is_empty() { &snap.summary } else { &snap.description }));
            lines.push(format!("type:         {}", snap.snap_type.as_str()));
            lines.push(format!("snap-id:      {}", make_snap_id(&snap.name)));
            lines.push(format!("tracking:     {}", snap.channel));
            lines.push(format!("refresh-date: {}", snap.install_date));
            lines.push(format!("installed:    {} (rev {}) {} {}", snap.version, snap.revision,
                format_size(snap.size), snap.confinement.as_str()));
            return Ok(lines);
        }
        // Check store
        if let Some(entry) = self.store_catalog.iter().find(|e| e.name == name) {
            let mut lines = Vec::new();
            lines.push(format!("name:        {}", entry.name));
            lines.push(format!("summary:     {}", entry.summary));
            lines.push(format!("publisher:   {}", entry.publisher));
            lines.push(format!("store-url:   https://snapcraft.io/{}", entry.name));
            lines.push(format!("type:        {}", entry.snap_type.as_str()));
            lines.push(format!("confinement: {}", entry.confinement.as_str()));
            let ch_str = entry.channels.join(", ");
            lines.push(format!("channels:    {}", ch_str));
            return Ok(lines);
        }
        Err(format!("snap \"{}\" not found", name))
    }

    // ========================================================================
    // Run
    // ========================================================================

    fn run_snap(&self, name: &str, args: &[String]) -> Result<String, String> {
        let snap = self.snaps.get(name)
            .ok_or_else(|| format!("snap \"{}\" is not installed", name))?;
        if snap.status == SnapStatus::Disabled {
            return Err(format!("snap \"{}\" is disabled", name));
        }
        let args_str = if args.is_empty() {
            String::new()
        } else {
            format!(" {}", args.join(" "))
        };
        Ok(format!("Running {}{} (confinement: {})", name, args_str, snap.confinement.as_str()))
    }

    // ========================================================================
    // Connect / disconnect interfaces
    // ========================================================================

    fn connect_interface(&mut self, plug_snap: &str, plug_name: &str, slot_snap: &str, slot_name: &str) -> Result<String, String> {
        if !self.snaps.contains_key(plug_snap) && plug_snap != "core" {
            return Err(format!("snap \"{}\" is not installed", plug_snap));
        }
        // Check for duplicate
        for iface in &self.interfaces {
            if iface.plug_snap == plug_snap && iface.plug_name == plug_name
                && iface.slot_snap == slot_snap && iface.slot_name == slot_name
            {
                return Err("interface already connected".to_string());
            }
        }
        self.interfaces.push(Interface {
            name: plug_name.to_string(),
            slot_snap: slot_snap.to_string(),
            slot_name: slot_name.to_string(),
            plug_snap: plug_snap.to_string(),
            plug_name: plug_name.to_string(),
            auto_connect: false,
        });
        Ok(format!("Connected {}:{} to {}:{}", plug_snap, plug_name, slot_snap, slot_name))
    }

    fn disconnect_interface(&mut self, plug_snap: &str, plug_name: &str) -> Result<String, String> {
        let before = self.interfaces.len();
        self.interfaces.retain(|i| !(i.plug_snap == plug_snap && i.plug_name == plug_name));
        if self.interfaces.len() == before {
            return Err(format!("no connection found for {}:{}", plug_snap, plug_name));
        }
        Ok(format!("Disconnected {}:{}", plug_snap, plug_name))
    }

    fn list_interfaces(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("{:<20} {:<25} {:<25} {:<8}", "Interface", "Slot", "Plug", "Auto"));
        for iface in &self.interfaces {
            let slot = format!("{}:{}", iface.slot_snap, iface.slot_name);
            let plug = format!("{}:{}", iface.plug_snap, iface.plug_name);
            let auto_str = if iface.auto_connect { "yes" } else { "no" };
            lines.push(format!("{:<20} {:<25} {:<25} {:<8}", iface.name, slot, plug, auto_str));
        }
        lines
    }

    // ========================================================================
    // Services
    // ========================================================================

    fn service_start(&mut self, snap_name: &str, svc_name: Option<&str>) -> Result<String, String> {
        if !self.snaps.contains_key(snap_name) {
            return Err(format!("snap \"{}\" is not installed", snap_name));
        }
        let sn = svc_name.unwrap_or("default");
        for svc in &mut self.services {
            if svc.snap_name == snap_name && svc.service_name == sn {
                if svc.status == ServiceStatus::Running {
                    return Err(format!("service {}.{} is already running", snap_name, sn));
                }
                svc.status = ServiceStatus::Running;
                return Ok(format!("Started {}.{}", snap_name, sn));
            }
        }
        self.services.push(SnapService {
            snap_name: snap_name.to_string(),
            service_name: sn.to_string(),
            status: ServiceStatus::Running,
            enabled: true,
        });
        Ok(format!("Started {}.{}", snap_name, sn))
    }

    fn service_stop(&mut self, snap_name: &str, svc_name: Option<&str>) -> Result<String, String> {
        if !self.snaps.contains_key(snap_name) {
            return Err(format!("snap \"{}\" is not installed", snap_name));
        }
        let sn = svc_name.unwrap_or("default");
        for svc in &mut self.services {
            if svc.snap_name == snap_name && svc.service_name == sn {
                if svc.status == ServiceStatus::Stopped {
                    return Err(format!("service {}.{} is already stopped", snap_name, sn));
                }
                svc.status = ServiceStatus::Stopped;
                return Ok(format!("Stopped {}.{}", snap_name, sn));
            }
        }
        Err(format!("service {}.{} not found", snap_name, sn))
    }

    fn service_restart(&mut self, snap_name: &str, svc_name: Option<&str>) -> Result<String, String> {
        if !self.snaps.contains_key(snap_name) {
            return Err(format!("snap \"{}\" is not installed", snap_name));
        }
        let sn = svc_name.unwrap_or("default");
        for svc in &mut self.services {
            if svc.snap_name == snap_name && svc.service_name == sn {
                svc.status = ServiceStatus::Running;
                return Ok(format!("Restarted {}.{}", snap_name, sn));
            }
        }
        Err(format!("service {}.{} not found", snap_name, sn))
    }

    fn service_logs(&self, snap_name: &str, svc_name: Option<&str>) -> Result<Vec<String>, String> {
        if !self.snaps.contains_key(snap_name) {
            return Err(format!("snap \"{}\" is not installed", snap_name));
        }
        let sn = svc_name.unwrap_or("default");
        let exists = self.services.iter().any(|s| s.snap_name == snap_name && s.service_name == sn);
        if !exists {
            return Err(format!("service {}.{} not found", snap_name, sn));
        }
        Ok(vec![
            format!("-- Logs for {}.{} --", snap_name, sn),
            "2025-01-01T00:00:00Z: Service started".to_string(),
            "2025-01-01T00:00:01Z: Listening on :8080".to_string(),
        ])
    }

    fn list_services(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("{:<20} {:<20} {:<10} {:<8}", "Snap", "Service", "Status", "Enabled"));
        for svc in &self.services {
            lines.push(format!(
                "{:<20} {:<20} {:<10} {:<8}",
                svc.snap_name, svc.service_name,
                svc.status.as_str(),
                if svc.enabled { "yes" } else { "no" }
            ));
        }
        lines
    }

    // ========================================================================
    // Configuration (get/set)
    // ========================================================================

    fn set_config(&mut self, snap_name: &str, key: &str, value: &str) -> Result<String, String> {
        if !self.snaps.contains_key(snap_name) {
            return Err(format!("snap \"{}\" is not installed", snap_name));
        }
        self.config
            .entry(snap_name.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        Ok(format!("Set {}={} for {}", key, value, snap_name))
    }

    fn get_config(&self, snap_name: &str, key: Option<&str>) -> Result<Vec<String>, String> {
        if !self.snaps.contains_key(snap_name) {
            return Err(format!("snap \"{}\" is not installed", snap_name));
        }
        let conf = self.config.get(snap_name);
        match (conf, key) {
            (Some(map), Some(k)) => {
                match map.get(k) {
                    Some(v) => Ok(vec![format!("{}: {}", k, v)]),
                    None => Err(format!("key \"{}\" not found for snap \"{}\"", k, snap_name)),
                }
            }
            (Some(map), None) => {
                let mut lines = Vec::new();
                for (k, v) in map {
                    lines.push(format!("{}: {}", k, v));
                }
                if lines.is_empty() {
                    lines.push(format!("No configuration for {}", snap_name));
                }
                Ok(lines)
            }
            (None, _) => Ok(vec![format!("No configuration for {}", snap_name)]),
        }
    }

    // ========================================================================
    // Aliases
    // ========================================================================

    fn add_alias(&mut self, snap_name: &str, alias_name: &str, command: &str) -> Result<String, String> {
        if !self.snaps.contains_key(snap_name) {
            return Err(format!("snap \"{}\" is not installed", snap_name));
        }
        for a in &self.aliases {
            if a.alias_name == alias_name {
                return Err(format!("alias \"{}\" already exists", alias_name));
            }
        }
        self.aliases.push(Alias {
            snap_name: snap_name.to_string(),
            alias_name: alias_name.to_string(),
            command: command.to_string(),
            status: AliasStatus::Manual,
        });
        Ok(format!("Added alias {} -> {}.{}", alias_name, snap_name, command))
    }

    fn remove_alias(&mut self, alias_name: &str) -> Result<String, String> {
        let before = self.aliases.len();
        self.aliases.retain(|a| a.alias_name != alias_name);
        if self.aliases.len() == before {
            return Err(format!("alias \"{}\" not found", alias_name));
        }
        Ok(format!("Removed alias {}", alias_name))
    }

    fn prefer(&mut self, snap_name: &str) -> Result<String, String> {
        if !self.snaps.contains_key(snap_name) {
            return Err(format!("snap \"{}\" is not installed", snap_name));
        }
        // Mark all aliases of this snap as preferred (auto)
        let mut count = 0u32;
        for a in &mut self.aliases {
            if a.snap_name == snap_name {
                a.status = AliasStatus::Auto;
                count += 1;
            }
        }
        Ok(format!("Preferred snap {} ({} aliases updated)", snap_name, count))
    }

    fn list_aliases(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("{:<15} {:<15} {:<20} {:<10}", "Snap", "Alias", "Command", "Status"));
        for a in &self.aliases {
            lines.push(format!(
                "{:<15} {:<15} {:<20} {:<10}",
                a.snap_name, a.alias_name, a.command, a.status.as_str()
            ));
        }
        lines
    }

    // ========================================================================
    // Changes / tasks / abort / watch
    // ========================================================================

    fn list_changes(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("{:<5} {:<10} {:<25} {}", "ID", "Status", "Spawn", "Summary"));
        for c in &self.changes {
            lines.push(format!(
                "{:<5} {:<10} {:<25} {}",
                c.id, c.status.as_str(), c.spawn_time, c.summary
            ));
        }
        lines
    }

    fn list_tasks(&self, change_id: u64) -> Result<Vec<String>, String> {
        let change = self.changes.iter().find(|c| c.id == change_id)
            .ok_or_else(|| format!("change {} not found", change_id))?;
        let mut lines = Vec::new();
        lines.push(format!("Change {}: {} ({})", change.id, change.summary, change.status.as_str()));
        lines.push(format!("{:<5} {:<10} {:<15} {}", "ID", "Status", "Progress", "Summary"));
        for t in &change.tasks {
            lines.push(format!(
                "{:<5} {:<10} {:<15} {}",
                t.id, t.status.as_str(),
                format!("{}/{}", t.progress_done, t.progress_total),
                t.summary
            ));
        }
        Ok(lines)
    }

    fn abort_change(&mut self, change_id: u64) -> Result<String, String> {
        let change = self.changes.iter_mut().find(|c| c.id == change_id)
            .ok_or_else(|| format!("change {} not found", change_id))?;
        if change.status == ChangeStatus::Done {
            return Err(format!("change {} is already done", change_id));
        }
        if change.status == ChangeStatus::Aborted {
            return Err(format!("change {} is already aborted", change_id));
        }
        change.status = ChangeStatus::Aborted;
        for t in &mut change.tasks {
            if t.status == ChangeStatus::Doing {
                t.status = ChangeStatus::Aborted;
            }
        }
        Ok(format!("Aborted change {}", change_id))
    }

    fn watch_change(&self, change_id: u64) -> Result<Vec<String>, String> {
        let change = self.changes.iter().find(|c| c.id == change_id)
            .ok_or_else(|| format!("change {} not found", change_id))?;
        let mut lines = Vec::new();
        lines.push(format!("Watching change {}...", change_id));
        lines.push(format!("Status: {}", change.status.as_str()));
        for t in &change.tasks {
            lines.push(format!("  Task {}: {} ({}/{})",
                t.id, t.summary, t.progress_done, t.progress_total));
        }
        if change.status == ChangeStatus::Done || change.status == ChangeStatus::Aborted || change.status == ChangeStatus::Error {
            lines.push(format!("Change {} finished: {}", change_id, change.status.as_str()));
        }
        Ok(lines)
    }

    // ========================================================================
    // Snapshots: save / restore / forget
    // ========================================================================

    fn save_snapshot(&mut self, name: &str) -> Result<String, String> {
        let snap = self.snaps.get(name)
            .ok_or_else(|| format!("snap \"{}\" is not installed", name))?;
        let sid = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        self.snapshots.push(Snapshot {
            id: sid,
            snap_name: name.to_string(),
            version: snap.version.clone(),
            revision: snap.revision,
            size: snap.size,
            timestamp: "2025-01-01T00:10:00Z".to_string(),
        });
        Ok(format!("Saved snapshot {} for {} (rev {})", sid, name, snap.revision))
    }

    fn restore_snapshot(&self, snapshot_id: u64) -> Result<String, String> {
        let snapshot = self.snapshots.iter().find(|s| s.id == snapshot_id)
            .ok_or_else(|| format!("snapshot {} not found", snapshot_id))?;
        Ok(format!("Restored snapshot {} for {} (rev {})", snapshot_id, snapshot.snap_name, snapshot.revision))
    }

    fn forget_snapshot(&mut self, snapshot_id: u64) -> Result<String, String> {
        let before = self.snapshots.len();
        self.snapshots.retain(|s| s.id != snapshot_id);
        if self.snapshots.len() == before {
            return Err(format!("snapshot {} not found", snapshot_id));
        }
        Ok(format!("Forgot snapshot {}", snapshot_id))
    }

    // ========================================================================
    // Assertions: known / ack
    // ========================================================================

    fn known_assertions(&self, assertion_type: Option<&str>) -> Vec<String> {
        let mut lines = Vec::new();
        for a in &self.assertions {
            if let Some(at) = assertion_type
                && a.assertion_type != at {
                    continue;
                }
            lines.push(format!("type: {}", a.assertion_type));
            for (k, v) in &a.headers {
                lines.push(format!("  {}: {}", k, v));
            }
            if !a.body.is_empty() {
                lines.push(format!("  body: {}...", &a.body[..a.body.len().min(40)]));
            }
            lines.push(String::new());
        }
        if lines.is_empty() {
            lines.push("No assertions found.".to_string());
        }
        lines
    }

    fn ack_assertion(&mut self, assertion_type: &str, key: &str, value: &str) -> Result<String, String> {
        let mut headers = BTreeMap::new();
        headers.insert(key.to_string(), value.to_string());
        self.assertions.push(Assertion {
            assertion_type: assertion_type.to_string(),
            headers,
            body: String::new(),
        });
        Ok(format!("Acknowledged assertion of type {}", assertion_type))
    }

    // ========================================================================
    // Download / pack
    // ========================================================================

    fn download_snap(&self, name: &str, channel: Option<&str>) -> Result<String, String> {
        let ch = channel.unwrap_or("stable");
        let entry = self.store_catalog.iter().find(|e| e.name == name);
        match entry {
            Some(e) => Ok(format!("Downloaded {}_{}_{}.snap from channel {}", name, e.version, ch, ch)),
            None => Err(format!("snap \"{}\" not found in store", name)),
        }
    }

    fn pack_snap(&self, directory: &str) -> Result<String, String> {
        if directory.is_empty() {
            return Err("directory path required".to_string());
        }
        Ok(format!("Packed snap from directory: {}", directory))
    }

    // ========================================================================
    // Debug subcommands
    // ========================================================================

    fn debug_connectivity(&self) -> Vec<String> {
        vec![
            "Connectivity check:".to_string(),
            "  Store URL: https://api.snapcraft.io".to_string(),
            "  Reachable: true".to_string(),
            "  Latency: 42ms".to_string(),
        ]
    }

    fn debug_timings(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push("Recent operation timings:".to_string());
        for c in self.changes.iter().rev().take(5) {
            lines.push(format!("  Change {}: {} ({})", c.id, c.summary, c.status.as_str()));
        }
        lines
    }

    fn debug_state_changes(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push("State changes:".to_string());
        for c in &self.changes {
            lines.push(format!("  {} -> {} at {}", c.summary, c.status.as_str(), c.spawn_time));
        }
        if self.changes.is_empty() {
            lines.push("  (none)".to_string());
        }
        lines
    }

    fn debug_ensure_state_soon(&self) -> String {
        "Ensured state will be refreshed on next snapd interaction.".to_string()
    }

    // ========================================================================
    // Auth: login / logout / whoami
    // ========================================================================

    fn login(&mut self, email: &str) -> Result<String, String> {
        if email.is_empty() {
            return Err("email address required".to_string());
        }
        if !email.contains('@') {
            return Err(format!("invalid email: {}", email));
        }
        self.auth = AuthState {
            logged_in: true,
            email: email.to_string(),
            macaroon: format!("macaroon-{}", email.len()),
        };
        Ok(format!("Logged in as {}", email))
    }

    fn logout(&mut self) -> Result<String, String> {
        if !self.auth.logged_in {
            return Err("not logged in".to_string());
        }
        self.auth = AuthState::empty();
        Ok("Logged out.".to_string())
    }

    fn whoami(&self) -> String {
        if self.auth.logged_in {
            format!("email: {}", self.auth.email)
        } else {
            "not logged in".to_string()
        }
    }

    // ========================================================================
    // Version
    // ========================================================================

    fn version_info(&self) -> Vec<String> {
        vec![
            format!("snap    {}", VERSION),
            format!("snapd   {}", VERSION),
            "series  16".to_string(),
            "slateos   0.1.0".to_string(),
        ]
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn make_snap_id(name: &str) -> String {
    let hash: u64 = name.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(u64::from(b)));
    format!("{:016x}", hash)
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1}MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

fn parse_snap_colon(s: &str) -> (&str, &str) {
    match s.find(':') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, s),
    }
}

fn print_lines(lines: &[String]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in lines {
        let _ = writeln!(out, "{}", line);
    }
}

fn print_err(msg: &str) {
    let stderr = io::stderr();
    let mut err = stderr.lock();
    let _ = writeln!(err, "error: {}", msg);
}

// ============================================================================
// Snap CLI dispatch
// ============================================================================

fn run_snap(args: &[String]) -> i32 {
    let mut state = SnapState::new();

    if args.is_empty() {
        print_snap_usage();
        return 0;
    }

    let subcmd = args[0].as_str();
    let sub_args = &args[1..];

    match subcmd {
        "install" => cmd_install(&mut state, sub_args),
        "remove" => cmd_remove(&mut state, sub_args),
        "refresh" => cmd_refresh(&mut state, sub_args),
        "revert" => cmd_revert(&mut state, sub_args),
        "list" => cmd_list(&state),
        "find" => cmd_find(&state, sub_args),
        "info" => cmd_info(&state, sub_args),
        "run" => cmd_run(&state, sub_args),
        "connect" => cmd_connect(&mut state, sub_args),
        "disconnect" => cmd_disconnect(&mut state, sub_args),
        "interfaces" => cmd_interfaces(&state),
        "services" => cmd_services(&mut state, sub_args),
        "set" => cmd_set(&mut state, sub_args),
        "get" => cmd_get(&state, sub_args),
        "alias" => cmd_alias(&mut state, sub_args),
        "unalias" => cmd_unalias(&mut state, sub_args),
        "prefer" => cmd_prefer(&mut state, sub_args),
        "changes" => cmd_changes(&state),
        "tasks" => cmd_tasks(&state, sub_args),
        "abort" => cmd_abort(&mut state, sub_args),
        "watch" => cmd_watch(&state, sub_args),
        "version" | "--version" => cmd_version(&state),
        "whoami" => cmd_whoami(&state),
        "login" => cmd_login(&mut state, sub_args),
        "logout" => cmd_logout(&mut state),
        "save" => cmd_save(&mut state, sub_args),
        "restore" => cmd_restore(&state, sub_args),
        "forget" => cmd_forget(&mut state, sub_args),
        "known" => cmd_known(&state, sub_args),
        "ack" => cmd_ack(&mut state, sub_args),
        "download" => cmd_download(&state, sub_args),
        "pack" => cmd_pack(&state, sub_args),
        "debug" => cmd_debug(&state, sub_args),
        "help" | "--help" | "-h" => { print_snap_usage(); 0 }
        _ => {
            print_err(&format!("unknown command: {}", subcmd));
            1
        }
    }
}

fn print_snap_usage() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "Usage: snap <command> [options]");
    let _ = writeln!(out);
    let _ = writeln!(out, "Package management commands:");
    let _ = writeln!(out, "  install     Install a snap");
    let _ = writeln!(out, "  remove      Remove a snap");
    let _ = writeln!(out, "  refresh     Refresh (update) a snap or all snaps");
    let _ = writeln!(out, "  revert      Revert a snap to its previous revision");
    let _ = writeln!(out, "  list        List installed snaps");
    let _ = writeln!(out, "  find        Search the store for snaps");
    let _ = writeln!(out, "  info        Show detailed info about a snap");
    let _ = writeln!(out, "  run         Run a snap application");
    let _ = writeln!(out, "  download    Download a snap from the store");
    let _ = writeln!(out, "  pack        Pack a snap from a directory");
    let _ = writeln!(out);
    let _ = writeln!(out, "Interface commands:");
    let _ = writeln!(out, "  connect     Connect an interface plug to a slot");
    let _ = writeln!(out, "  disconnect  Disconnect an interface plug");
    let _ = writeln!(out, "  interfaces  List all interfaces");
    let _ = writeln!(out);
    let _ = writeln!(out, "Service commands:");
    let _ = writeln!(out, "  services    Manage snap services (start/stop/restart/logs)");
    let _ = writeln!(out);
    let _ = writeln!(out, "Configuration commands:");
    let _ = writeln!(out, "  set         Set snap configuration");
    let _ = writeln!(out, "  get         Get snap configuration");
    let _ = writeln!(out);
    let _ = writeln!(out, "Alias commands:");
    let _ = writeln!(out, "  alias       Create a command alias for a snap");
    let _ = writeln!(out, "  unalias     Remove a command alias");
    let _ = writeln!(out, "  prefer      Prefer a snap's aliases over others");
    let _ = writeln!(out);
    let _ = writeln!(out, "Change tracking:");
    let _ = writeln!(out, "  changes     List recent changes");
    let _ = writeln!(out, "  tasks       Show tasks for a change");
    let _ = writeln!(out, "  abort       Abort a pending change");
    let _ = writeln!(out, "  watch       Watch a change in progress");
    let _ = writeln!(out);
    let _ = writeln!(out, "Authentication:");
    let _ = writeln!(out, "  whoami      Show current user");
    let _ = writeln!(out, "  login       Authenticate with snap store");
    let _ = writeln!(out, "  logout      Log out");
    let _ = writeln!(out);
    let _ = writeln!(out, "Snapshot commands:");
    let _ = writeln!(out, "  save        Save a snap snapshot");
    let _ = writeln!(out, "  restore     Restore a snap snapshot");
    let _ = writeln!(out, "  forget      Forget (delete) a snapshot");
    let _ = writeln!(out);
    let _ = writeln!(out, "Assertion commands:");
    let _ = writeln!(out, "  known       Show known assertions");
    let _ = writeln!(out, "  ack         Acknowledge an assertion");
    let _ = writeln!(out);
    let _ = writeln!(out, "Debug commands:");
    let _ = writeln!(out, "  debug       Debug subcommands (connectivity/timings/state-changes/ensure-state-soon)");
    let _ = writeln!(out);
    let _ = writeln!(out, "  version     Show version information");
    let _ = writeln!(out, "  help        Show this help");
}

// ============================================================================
// Subcommand implementations
// ============================================================================

fn cmd_install(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snap name required");
        return 1;
    }
    let mut channel = None;
    let mut confinement = None;
    let mut name = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--channel" => {
                i += 1;
                if i < args.len() {
                    channel = Some(args[i].as_str());
                }
            }
            "--classic" => confinement = Some(Confinement::Classic),
            "--devmode" => confinement = Some(Confinement::Devmode),
            s if s.starts_with('-') => {
                print_err(&format!("unknown flag: {}", s));
                return 1;
            }
            _ => name = Some(args[i].as_str()),
        }
        i += 1;
    }
    let snap_name = match name {
        Some(n) => n,
        None => { print_err("snap name required"); return 1; }
    };
    // Rebind channel to an owned string so it lives long enough
    let channel_owned: Option<String> = channel.map(|c| c.to_string());
    let channel_ref = channel_owned.as_deref();
    match state.install_snap(snap_name, channel_ref, confinement) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_remove(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snap name required");
        return 1;
    }
    match state.remove_snap(&args[0]) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_refresh(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        let results = state.refresh_all();
        print_lines(&results);
        return 0;
    }
    let mut channel = None;
    let mut name = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--channel" => {
                i += 1;
                if i < args.len() {
                    channel = Some(args[i].as_str());
                }
            }
            s if s.starts_with('-') => {
                print_err(&format!("unknown flag: {}", s));
                return 1;
            }
            _ => name = Some(args[i].as_str()),
        }
        i += 1;
    }
    if let Some(n) = name {
        let channel_owned: Option<String> = channel.map(|c| c.to_string());
        let channel_ref = channel_owned.as_deref();
        match state.refresh_snap(n, channel_ref) {
            Ok(msg) => { println!("{}", msg); 0 }
            Err(e) => { print_err(&e); 1 }
        }
    } else {
        let results = state.refresh_all();
        print_lines(&results);
        0
    }
}

fn cmd_revert(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snap name required");
        return 1;
    }
    match state.revert_snap(&args[0]) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_list(state: &SnapState) -> i32 {
    print_lines(&state.list_snaps());
    0
}

fn cmd_find(state: &SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("search query required");
        return 1;
    }
    let mut query = "";
    let mut section = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--section" => {
                i += 1;
                if i < args.len() {
                    section = Some(args[i].as_str());
                }
            }
            _ => query = args[i].as_str(),
        }
        i += 1;
    }
    let section_owned: Option<String> = section.map(|s| s.to_string());
    let section_ref = section_owned.as_deref();
    print_lines(&state.find_snaps(query, section_ref));
    0
}

fn cmd_info(state: &SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snap name required");
        return 1;
    }
    match state.snap_info(&args[0]) {
        Ok(lines) => { print_lines(&lines); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_run(state: &SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snap name required");
        return 1;
    }
    let name = &args[0];
    let run_args = if args.len() > 1 { &args[1..] } else { &[] };
    match state.run_snap(name, run_args) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_connect(state: &mut SnapState, args: &[String]) -> i32 {
    if args.len() < 2 {
        print_err("usage: snap connect <plug-snap>:<plug> <slot-snap>:<slot>");
        return 1;
    }
    let (plug_snap, plug_name) = parse_snap_colon(&args[0]);
    let (slot_snap, slot_name) = parse_snap_colon(&args[1]);
    match state.connect_interface(plug_snap, plug_name, slot_snap, slot_name) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_disconnect(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("usage: snap disconnect <plug-snap>:<plug>");
        return 1;
    }
    let (plug_snap, plug_name) = parse_snap_colon(&args[0]);
    match state.disconnect_interface(plug_snap, plug_name) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_interfaces(state: &SnapState) -> i32 {
    print_lines(&state.list_interfaces());
    0
}

fn cmd_services(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_lines(&state.list_services());
        return 0;
    }
    let subcmd = args[0].as_str();
    let svc_args = &args[1..];
    match subcmd {
        "start" => {
            if svc_args.is_empty() {
                print_err("snap name required");
                return 1;
            }
            let (snap_name, svc) = parse_snap_colon(&svc_args[0]);
            let svc_name = if svc == snap_name { None } else { Some(svc) };
            match state.service_start(snap_name, svc_name) {
                Ok(msg) => { println!("{}", msg); 0 }
                Err(e) => { print_err(&e); 1 }
            }
        }
        "stop" => {
            if svc_args.is_empty() {
                print_err("snap name required");
                return 1;
            }
            let (snap_name, svc) = parse_snap_colon(&svc_args[0]);
            let svc_name = if svc == snap_name { None } else { Some(svc) };
            match state.service_stop(snap_name, svc_name) {
                Ok(msg) => { println!("{}", msg); 0 }
                Err(e) => { print_err(&e); 1 }
            }
        }
        "restart" => {
            if svc_args.is_empty() {
                print_err("snap name required");
                return 1;
            }
            let (snap_name, svc) = parse_snap_colon(&svc_args[0]);
            let svc_name = if svc == snap_name { None } else { Some(svc) };
            match state.service_restart(snap_name, svc_name) {
                Ok(msg) => { println!("{}", msg); 0 }
                Err(e) => { print_err(&e); 1 }
            }
        }
        "logs" => {
            if svc_args.is_empty() {
                print_err("snap name required");
                return 1;
            }
            let (snap_name, svc) = parse_snap_colon(&svc_args[0]);
            let svc_name = if svc == snap_name { None } else { Some(svc) };
            match state.service_logs(snap_name, svc_name) {
                Ok(lines) => { print_lines(&lines); 0 }
                Err(e) => { print_err(&e); 1 }
            }
        }
        _ => {
            print_err(&format!("unknown services subcommand: {}", subcmd));
            1
        }
    }
}

fn cmd_set(state: &mut SnapState, args: &[String]) -> i32 {
    if args.len() < 2 {
        print_err("usage: snap set <snap> <key>=<value>");
        return 1;
    }
    let snap_name = &args[0];
    let kv = &args[1];
    match kv.find('=') {
        Some(i) => {
            let key = &kv[..i];
            let value = &kv[i + 1..];
            match state.set_config(snap_name, key, value) {
                Ok(msg) => { println!("{}", msg); 0 }
                Err(e) => { print_err(&e); 1 }
            }
        }
        None => {
            print_err("expected key=value");
            1
        }
    }
}

fn cmd_get(state: &SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snap name required");
        return 1;
    }
    let snap_name = &args[0];
    let key = if args.len() > 1 { Some(args[1].as_str()) } else { None };
    match state.get_config(snap_name, key) {
        Ok(lines) => { print_lines(&lines); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_alias(state: &mut SnapState, args: &[String]) -> i32 {
    if args.len() < 3 {
        // With fewer args, list aliases
        if args.is_empty() {
            print_lines(&state.list_aliases());
            return 0;
        }
        print_err("usage: snap alias <snap> <alias> <command>");
        return 1;
    }
    match state.add_alias(&args[0], &args[1], &args[2]) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_unalias(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("alias name required");
        return 1;
    }
    match state.remove_alias(&args[0]) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_prefer(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snap name required");
        return 1;
    }
    match state.prefer(&args[0]) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_changes(state: &SnapState) -> i32 {
    print_lines(&state.list_changes());
    0
}

fn cmd_tasks(state: &SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("change ID required");
        return 1;
    }
    let id: u64 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => { print_err("invalid change ID"); return 1; }
    };
    match state.list_tasks(id) {
        Ok(lines) => { print_lines(&lines); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_abort(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("change ID required");
        return 1;
    }
    let id: u64 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => { print_err("invalid change ID"); return 1; }
    };
    match state.abort_change(id) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_watch(state: &SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("change ID required");
        return 1;
    }
    let id: u64 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => { print_err("invalid change ID"); return 1; }
    };
    match state.watch_change(id) {
        Ok(lines) => { print_lines(&lines); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_version(state: &SnapState) -> i32 {
    print_lines(&state.version_info());
    0
}

fn cmd_whoami(state: &SnapState) -> i32 {
    println!("{}", state.whoami());
    0
}

fn cmd_login(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("email required");
        return 1;
    }
    match state.login(&args[0]) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_logout(state: &mut SnapState) -> i32 {
    match state.logout() {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_save(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snap name required");
        return 1;
    }
    match state.save_snapshot(&args[0]) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_restore(state: &SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snapshot ID required");
        return 1;
    }
    let id: u64 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => { print_err("invalid snapshot ID"); return 1; }
    };
    match state.restore_snapshot(id) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_forget(state: &mut SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snapshot ID required");
        return 1;
    }
    let id: u64 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => { print_err("invalid snapshot ID"); return 1; }
    };
    match state.forget_snapshot(id) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_known(state: &SnapState, args: &[String]) -> i32 {
    let atype = if args.is_empty() { None } else { Some(args[0].as_str()) };
    print_lines(&state.known_assertions(atype));
    0
}

fn cmd_ack(state: &mut SnapState, args: &[String]) -> i32 {
    if args.len() < 3 {
        print_err("usage: snap ack <type> <key> <value>");
        return 1;
    }
    match state.ack_assertion(&args[0], &args[1], &args[2]) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_download(state: &SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("snap name required");
        return 1;
    }
    let mut channel = None;
    let mut name = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--channel" => {
                i += 1;
                if i < args.len() {
                    channel = Some(args[i].as_str());
                }
            }
            _ => name = Some(args[i].as_str()),
        }
        i += 1;
    }
    let snap_name = match name {
        Some(n) => n,
        None => { print_err("snap name required"); return 1; }
    };
    let channel_owned: Option<String> = channel.map(|c| c.to_string());
    let channel_ref = channel_owned.as_deref();
    match state.download_snap(snap_name, channel_ref) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_pack(state: &SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        print_err("directory required");
        return 1;
    }
    match state.pack_snap(&args[0]) {
        Ok(msg) => { println!("{}", msg); 0 }
        Err(e) => { print_err(&e); 1 }
    }
}

fn cmd_debug(state: &SnapState, args: &[String]) -> i32 {
    if args.is_empty() {
        println!("Usage: snap debug <subcommand>");
        println!("  connectivity        Check store connectivity");
        println!("  timings             Show recent operation timings");
        println!("  state-changes       Show state change history");
        println!("  ensure-state-soon   Trigger state refresh");
        return 0;
    }
    match args[0].as_str() {
        "connectivity" => { print_lines(&state.debug_connectivity()); 0 }
        "timings" => { print_lines(&state.debug_timings()); 0 }
        "state-changes" => { print_lines(&state.debug_state_changes()); 0 }
        "ensure-state-soon" => { println!("{}", state.debug_ensure_state_soon()); 0 }
        _ => {
            print_err(&format!("unknown debug subcommand: {}", args[0]));
            1
        }
    }
}

// ============================================================================
// Snapd daemon personality
// ============================================================================

fn run_snapd(args: &[String]) -> i32 {
    if !args.is_empty() && (args[0] == "--help" || args[0] == "-h") {
        println!("Usage: snapd [options]");
        println!();
        println!("The snap daemon manages snap packages, interfaces, and services.");
        println!();
        println!("Options:");
        println!("  --help       Show this help");
        println!("  --version    Show version");
        println!("  --debug      Enable debug logging");
        return 0;
    }
    if !args.is_empty() && (args[0] == "--version") {
        println!("snapd {}", VERSION);
        return 0;
    }
    let debug = !args.is_empty() && args[0] == "--debug";
    println!("snapd {} starting...", VERSION);
    if debug {
        println!("[debug] Debug logging enabled");
    }
    println!("snapd: managing snap lifecycle and interfaces");
    println!("snapd: listening on /run/snapd.socket");
    println!("snapd: ready");
    0
}

// ============================================================================
// Snap-confine personality
// ============================================================================

fn run_snap_confine(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("Usage: snap-confine <snap-name> <command> [args...]");
        println!();
        println!("Sets up the confinement environment for a snap and executes");
        println!("the given command inside it.");
        println!();
        println!("Options:");
        println!("  --classic    Use classic (unconfined) mode");
        println!("  --devmode    Use development mode (non-enforcing)");
        return 0;
    }
    if args[0] == "--version" {
        println!("snap-confine {}", VERSION);
        return 0;
    }

    let mut snap_name = None;
    let mut command = None;
    let mut mode = Confinement::Strict;
    let mut cmd_args = Vec::new();
    let mut past_flags = false;

    for arg in args {
        if !past_flags {
            match arg.as_str() {
                "--classic" => { mode = Confinement::Classic; continue; }
                "--devmode" => { mode = Confinement::Devmode; continue; }
                _ => {}
            }
        }
        if snap_name.is_none() {
            snap_name = Some(arg.clone());
            past_flags = true;
        } else if command.is_none() {
            command = Some(arg.clone());
        } else {
            cmd_args.push(arg.clone());
        }
    }

    let snap_name = match snap_name {
        Some(n) => n,
        None => { print_err("snap name required"); return 1; }
    };
    let command = match command {
        Some(c) => c,
        None => { print_err("command required"); return 1; }
    };

    println!("snap-confine: setting up confinement for {}", snap_name);
    println!("  confinement: {}", mode.as_str());
    println!("  mounting snap filesystem...");
    println!("  setting up security profiles...");
    println!("  setting up cgroups...");
    println!("  executing: {}{}", command,
        if cmd_args.is_empty() { String::new() } else { format!(" {}", cmd_args.join(" ")) });
    0
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = if args.is_empty() { "snap" } else { &args[0] };
    let personality = detect_personality(argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match personality {
        Personality::Snap => run_snap(&rest),
        Personality::Snapd => run_snapd(&rest),
        Personality::SnapConfine => run_snap_confine(&rest),
    };

    process::exit(code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // === Personality detection ===

    #[test]
    fn test_personality_snap_default() {
        assert_eq!(detect_personality("snap"), Personality::Snap);
    }

    #[test]
    fn test_personality_snap_with_path() {
        assert_eq!(detect_personality("/usr/bin/snap"), Personality::Snap);
    }

    #[test]
    fn test_personality_snap_windows_path() {
        assert_eq!(detect_personality("C:\\bin\\snap.exe"), Personality::Snap);
    }

    #[test]
    fn test_personality_snapd() {
        assert_eq!(detect_personality("snapd"), Personality::Snapd);
    }

    #[test]
    fn test_personality_snapd_with_path() {
        assert_eq!(detect_personality("/usr/lib/snapd/snapd"), Personality::Snapd);
    }

    #[test]
    fn test_personality_snapd_exe() {
        assert_eq!(detect_personality("snapd.exe"), Personality::Snapd);
    }

    #[test]
    fn test_personality_snap_confine() {
        assert_eq!(detect_personality("snap-confine"), Personality::SnapConfine);
    }

    #[test]
    fn test_personality_snap_confine_with_path() {
        assert_eq!(detect_personality("/usr/lib/snapd/snap-confine"), Personality::SnapConfine);
    }

    #[test]
    fn test_personality_snap_confine_exe() {
        assert_eq!(detect_personality("C:\\snap\\snap-confine.exe"), Personality::SnapConfine);
    }

    #[test]
    fn test_personality_unknown_defaults_to_snap() {
        assert_eq!(detect_personality("something-else"), Personality::Snap);
    }

    #[test]
    fn test_personality_names() {
        assert_eq!(Personality::Snap.name(), "snap");
        assert_eq!(Personality::Snapd.name(), "snapd");
        assert_eq!(Personality::SnapConfine.name(), "snap-confine");
    }

    // === Basename ===

    #[test]
    fn test_basename_simple() {
        assert_eq!(basename("snap"), "snap");
    }

    #[test]
    fn test_basename_unix_path() {
        assert_eq!(basename("/usr/bin/snap"), "snap");
    }

    #[test]
    fn test_basename_windows_path() {
        assert_eq!(basename("C:\\bin\\snap.exe"), "snap.exe");
    }

    #[test]
    fn test_basename_mixed_separators() {
        assert_eq!(basename("/usr/local\\bin/snap"), "snap");
    }

    // === SnapType ===

    #[test]
    fn test_snap_type_parse_all() {
        assert_eq!(SnapType::parse("app"), Some(SnapType::App));
        assert_eq!(SnapType::parse("gadget"), Some(SnapType::Gadget));
        assert_eq!(SnapType::parse("kernel"), Some(SnapType::Kernel));
        assert_eq!(SnapType::parse("core"), Some(SnapType::Core));
        assert_eq!(SnapType::parse("base"), Some(SnapType::Base));
        assert_eq!(SnapType::parse("unknown"), None);
    }

    #[test]
    fn test_snap_type_as_str() {
        assert_eq!(SnapType::App.as_str(), "app");
        assert_eq!(SnapType::Gadget.as_str(), "gadget");
        assert_eq!(SnapType::Kernel.as_str(), "kernel");
        assert_eq!(SnapType::Core.as_str(), "core");
        assert_eq!(SnapType::Base.as_str(), "base");
    }

    // === SnapStatus ===

    #[test]
    fn test_snap_status_parse_all() {
        assert_eq!(SnapStatus::parse("installed"), Some(SnapStatus::Installed));
        assert_eq!(SnapStatus::parse("active"), Some(SnapStatus::Active));
        assert_eq!(SnapStatus::parse("disabled"), Some(SnapStatus::Disabled));
        assert_eq!(SnapStatus::parse("unknown"), None);
    }

    #[test]
    fn test_snap_status_as_str() {
        assert_eq!(SnapStatus::Installed.as_str(), "installed");
        assert_eq!(SnapStatus::Active.as_str(), "active");
        assert_eq!(SnapStatus::Disabled.as_str(), "disabled");
    }

    // === Confinement ===

    #[test]
    fn test_confinement_parse_all() {
        assert_eq!(Confinement::parse("strict"), Some(Confinement::Strict));
        assert_eq!(Confinement::parse("classic"), Some(Confinement::Classic));
        assert_eq!(Confinement::parse("devmode"), Some(Confinement::Devmode));
        assert_eq!(Confinement::parse("none"), None);
    }

    #[test]
    fn test_confinement_as_str() {
        assert_eq!(Confinement::Strict.as_str(), "strict");
        assert_eq!(Confinement::Classic.as_str(), "classic");
        assert_eq!(Confinement::Devmode.as_str(), "devmode");
    }

    // === ChangeStatus ===

    #[test]
    fn test_change_status_parse() {
        assert_eq!(ChangeStatus::parse("Doing"), Some(ChangeStatus::Doing));
        assert_eq!(ChangeStatus::parse("doing"), Some(ChangeStatus::Doing));
        assert_eq!(ChangeStatus::parse("Done"), Some(ChangeStatus::Done));
        assert_eq!(ChangeStatus::parse("Error"), Some(ChangeStatus::Error));
        assert_eq!(ChangeStatus::parse("Undone"), Some(ChangeStatus::Undone));
        assert_eq!(ChangeStatus::parse("Aborted"), Some(ChangeStatus::Aborted));
        assert_eq!(ChangeStatus::parse("xxx"), None);
    }

    #[test]
    fn test_change_status_as_str() {
        assert_eq!(ChangeStatus::Doing.as_str(), "Doing");
        assert_eq!(ChangeStatus::Done.as_str(), "Done");
        assert_eq!(ChangeStatus::Error.as_str(), "Error");
        assert_eq!(ChangeStatus::Undone.as_str(), "Undone");
        assert_eq!(ChangeStatus::Aborted.as_str(), "Aborted");
    }

    // === AliasStatus ===

    #[test]
    fn test_alias_status_as_str() {
        assert_eq!(AliasStatus::Manual.as_str(), "manual");
        assert_eq!(AliasStatus::Auto.as_str(), "auto");
        assert_eq!(AliasStatus::Disabled.as_str(), "disabled");
    }

    // === ServiceStatus ===

    #[test]
    fn test_service_status_as_str() {
        assert_eq!(ServiceStatus::Running.as_str(), "running");
        assert_eq!(ServiceStatus::Stopped.as_str(), "stopped");
        assert_eq!(ServiceStatus::Failed.as_str(), "failed");
    }

    // === Helpers ===

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(512), "512B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024), "1.0KB");
        assert_eq!(format_size(2048), "2.0KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1_048_576), "1.0MB");
        assert_eq!(format_size(52_428_800), "50.0MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1_073_741_824), "1.0GB");
    }

    #[test]
    fn test_parse_snap_colon_with_colon() {
        assert_eq!(parse_snap_colon("firefox:network"), ("firefox", "network"));
    }

    #[test]
    fn test_parse_snap_colon_without_colon() {
        let (a, b) = parse_snap_colon("firefox");
        assert_eq!(a, "firefox");
        assert_eq!(b, "firefox");
    }

    #[test]
    fn test_make_snap_id_deterministic() {
        let id1 = make_snap_id("firefox");
        let id2 = make_snap_id("firefox");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_make_snap_id_different_names() {
        let id1 = make_snap_id("firefox");
        let id2 = make_snap_id("chromium");
        assert_ne!(id1, id2);
    }

    // === SnapInfo ===

    #[test]
    fn test_snap_info_new_defaults() {
        let info = SnapInfo::new("test-snap");
        assert_eq!(info.name, "test-snap");
        assert_eq!(info.version, "0.0.0");
        assert_eq!(info.revision, 1);
        assert_eq!(info.channel, "stable");
        assert_eq!(info.snap_type, SnapType::App);
        assert_eq!(info.status, SnapStatus::Active);
        assert_eq!(info.confinement, Confinement::Strict);
    }

    // === SnapState seeding ===

    #[test]
    fn test_state_has_default_snaps() {
        let state = SnapState::new();
        assert!(state.snaps.contains_key("core22"));
        assert!(state.snaps.contains_key("snapd"));
    }

    #[test]
    fn test_state_has_default_interfaces() {
        let state = SnapState::new();
        assert!(state.interfaces.len() >= 2);
    }

    #[test]
    fn test_state_has_store_catalog() {
        let state = SnapState::new();
        assert!(state.store_catalog.len() >= 5);
    }

    #[test]
    fn test_state_has_initial_change() {
        let state = SnapState::new();
        assert!(!state.changes.is_empty());
        assert_eq!(state.changes[0].status, ChangeStatus::Done);
    }

    // === Install ===

    #[test]
    fn test_install_from_store() {
        let mut state = SnapState::new();
        let result = state.install_snap("firefox", None, None);
        assert!(result.is_ok());
        assert!(state.snaps.contains_key("firefox"));
    }

    #[test]
    fn test_install_already_installed() {
        let mut state = SnapState::new();
        let result = state.install_snap("core22", None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_install_with_channel() {
        let mut state = SnapState::new();
        let result = state.install_snap("firefox", Some("beta"), None);
        assert!(result.is_ok());
        assert_eq!(state.snaps.get("firefox").unwrap().channel, "beta");
    }

    #[test]
    fn test_install_with_invalid_channel() {
        let mut state = SnapState::new();
        let result = state.install_snap("firefox", Some("nightly"), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_install_with_classic_confinement() {
        let mut state = SnapState::new();
        let result = state.install_snap("firefox", None, Some(Confinement::Classic));
        assert!(result.is_ok());
        assert_eq!(state.snaps.get("firefox").unwrap().confinement, Confinement::Classic);
    }

    #[test]
    fn test_install_unknown_snap() {
        let mut state = SnapState::new();
        let result = state.install_snap("nonexistent-app", None, None);
        assert!(result.is_ok());
        assert!(state.snaps.contains_key("nonexistent-app"));
    }

    #[test]
    fn test_install_creates_change() {
        let mut state = SnapState::new();
        let changes_before = state.changes.len();
        let _ = state.install_snap("firefox", None, None);
        assert!(state.changes.len() > changes_before);
    }

    // === Remove ===

    #[test]
    fn test_remove_installed() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let result = state.remove_snap("firefox");
        assert!(result.is_ok());
        assert!(!state.snaps.contains_key("firefox"));
    }

    #[test]
    fn test_remove_not_installed() {
        let mut state = SnapState::new();
        let result = state.remove_snap("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_cleans_services() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.service_start("firefox", Some("web"));
        assert!(!state.services.is_empty());
        let _ = state.remove_snap("firefox");
        assert!(state.services.iter().all(|s| s.snap_name != "firefox"));
    }

    #[test]
    fn test_remove_cleans_aliases() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.add_alias("firefox", "ff", "firefox");
        let _ = state.remove_snap("firefox");
        assert!(state.aliases.iter().all(|a| a.snap_name != "firefox"));
    }

    // === Refresh ===

    #[test]
    fn test_refresh_snap() {
        let mut state = SnapState::new();
        let old_rev = state.snaps.get("core22").unwrap().revision;
        let result = state.refresh_snap("core22", None);
        assert!(result.is_ok());
        assert!(state.snaps.get("core22").unwrap().revision > old_rev);
    }

    #[test]
    fn test_refresh_not_installed() {
        let mut state = SnapState::new();
        let result = state.refresh_snap("nonexistent", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_refresh_with_channel_switch() {
        let mut state = SnapState::new();
        let _ = state.refresh_snap("core22", Some("beta"));
        assert_eq!(state.snaps.get("core22").unwrap().channel, "beta");
    }

    #[test]
    fn test_refresh_invalid_channel() {
        let mut state = SnapState::new();
        let result = state.refresh_snap("core22", Some("nightly"));
        assert!(result.is_err());
    }

    #[test]
    fn test_refresh_all() {
        let mut state = SnapState::new();
        let results = state.refresh_all();
        assert!(!results.is_empty());
    }

    // === Revert ===

    #[test]
    fn test_revert_snap() {
        let mut state = SnapState::new();
        let _ = state.refresh_snap("core22", None);
        let rev_after_refresh = state.snaps.get("core22").unwrap().revision;
        let result = state.revert_snap("core22");
        assert!(result.is_ok());
        assert!(state.snaps.get("core22").unwrap().revision < rev_after_refresh);
    }

    #[test]
    fn test_revert_not_installed() {
        let mut state = SnapState::new();
        let result = state.revert_snap("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_revert_at_revision_one() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        // Force revision to 1 for this test
        state.snaps.get_mut("firefox").unwrap().revision = 1;
        let result = state.revert_snap("firefox");
        assert!(result.is_err());
    }

    // === List ===

    #[test]
    fn test_list_snaps_has_header() {
        let state = SnapState::new();
        let lines = state.list_snaps();
        assert!(!lines.is_empty());
        assert!(lines[0].contains("Name"));
    }

    #[test]
    fn test_list_snaps_includes_installed() {
        let state = SnapState::new();
        let lines = state.list_snaps();
        let has_core22 = lines.iter().any(|l| l.contains("core22"));
        assert!(has_core22);
    }

    // === Find ===

    #[test]
    fn test_find_existing() {
        let state = SnapState::new();
        let lines = state.find_snaps("firefox", None);
        assert!(lines.iter().any(|l| l.contains("firefox")));
    }

    #[test]
    fn test_find_no_match() {
        let state = SnapState::new();
        let lines = state.find_snaps("zzz-nonexistent-zzz", None);
        assert!(lines.iter().any(|l| l.contains("No matching")));
    }

    #[test]
    fn test_find_partial_match() {
        let state = SnapState::new();
        let lines = state.find_snaps("media", None);
        // "VLC media player" summary contains "media"
        assert!(lines.iter().any(|l| l.contains("vlc")));
    }

    // === Info ===

    #[test]
    fn test_info_installed_snap() {
        let state = SnapState::new();
        let result = state.snap_info("core22");
        assert!(result.is_ok());
        let lines = result.unwrap();
        assert!(lines.iter().any(|l| l.contains("core22")));
    }

    #[test]
    fn test_info_store_snap() {
        let state = SnapState::new();
        let result = state.snap_info("firefox");
        assert!(result.is_ok());
    }

    #[test]
    fn test_info_not_found() {
        let state = SnapState::new();
        let result = state.snap_info("totally-fake-snap");
        assert!(result.is_err());
    }

    // === Run ===

    #[test]
    fn test_run_installed_snap() {
        let state = SnapState::new();
        let result = state.run_snap("core22", &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_not_installed() {
        let state = SnapState::new();
        let result = state.run_snap("nonexistent", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_run_disabled_snap() {
        let mut state = SnapState::new();
        state.snaps.get_mut("core22").unwrap().status = SnapStatus::Disabled;
        let result = state.run_snap("core22", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_run_with_args() {
        let state = SnapState::new();
        let args = vec!["--headless".to_string()];
        let result = state.run_snap("core22", &args);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("--headless"));
    }

    // === Interfaces ===

    #[test]
    fn test_connect_interface() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let result = state.connect_interface("firefox", "network", "core", "network");
        assert!(result.is_ok());
    }

    #[test]
    fn test_connect_duplicate_interface() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.connect_interface("firefox", "net1", "core", "net1");
        let result = state.connect_interface("firefox", "net1", "core", "net1");
        assert!(result.is_err());
    }

    #[test]
    fn test_connect_snap_not_installed() {
        let mut state = SnapState::new();
        let result = state.connect_interface("nonexist", "net", "core", "net");
        assert!(result.is_err());
    }

    #[test]
    fn test_disconnect_interface() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.connect_interface("firefox", "camera", "core", "camera");
        let result = state.disconnect_interface("firefox", "camera");
        assert!(result.is_ok());
    }

    #[test]
    fn test_disconnect_nonexistent() {
        let mut state = SnapState::new();
        let result = state.disconnect_interface("nonexist", "net");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_interfaces_has_header() {
        let state = SnapState::new();
        let lines = state.list_interfaces();
        assert!(!lines.is_empty());
        assert!(lines[0].contains("Interface"));
    }

    // === Services ===

    #[test]
    fn test_service_start() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let result = state.service_start("firefox", Some("web"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_service_start_not_installed() {
        let mut state = SnapState::new();
        let result = state.service_start("nonexist", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_service_start_already_running() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.service_start("firefox", Some("web"));
        let result = state.service_start("firefox", Some("web"));
        assert!(result.is_err());
    }

    #[test]
    fn test_service_stop() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.service_start("firefox", Some("web"));
        let result = state.service_stop("firefox", Some("web"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_service_stop_already_stopped() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.service_start("firefox", Some("web"));
        let _ = state.service_stop("firefox", Some("web"));
        let result = state.service_stop("firefox", Some("web"));
        assert!(result.is_err());
    }

    #[test]
    fn test_service_stop_not_found() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let result = state.service_stop("firefox", Some("nonexist"));
        assert!(result.is_err());
    }

    #[test]
    fn test_service_restart() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.service_start("firefox", Some("web"));
        let result = state.service_restart("firefox", Some("web"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_service_restart_not_found() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let result = state.service_restart("firefox", Some("nonexist"));
        assert!(result.is_err());
    }

    #[test]
    fn test_service_logs() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.service_start("firefox", Some("web"));
        let result = state.service_logs("firefox", Some("web"));
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn test_service_logs_not_found() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let result = state.service_logs("firefox", Some("nonexist"));
        assert!(result.is_err());
    }

    #[test]
    fn test_list_services_has_header() {
        let state = SnapState::new();
        let lines = state.list_services();
        assert!(!lines.is_empty());
        assert!(lines[0].contains("Snap"));
    }

    // === Configuration ===

    #[test]
    fn test_set_config() {
        let mut state = SnapState::new();
        let result = state.set_config("core22", "key1", "value1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_set_config_not_installed() {
        let mut state = SnapState::new();
        let result = state.set_config("nonexist", "k", "v");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_config() {
        let mut state = SnapState::new();
        let _ = state.set_config("core22", "key1", "value1");
        let result = state.get_config("core22", Some("key1"));
        assert!(result.is_ok());
        assert!(result.unwrap()[0].contains("value1"));
    }

    #[test]
    fn test_get_config_missing_key() {
        let mut state = SnapState::new();
        let _ = state.set_config("core22", "key1", "value1");
        let result = state.get_config("core22", Some("missing"));
        assert!(result.is_err());
    }

    #[test]
    fn test_get_config_all_keys() {
        let mut state = SnapState::new();
        let _ = state.set_config("core22", "k1", "v1");
        let _ = state.set_config("core22", "k2", "v2");
        let result = state.get_config("core22", None);
        assert!(result.is_ok());
        assert!(result.unwrap().len() >= 2);
    }

    #[test]
    fn test_get_config_no_config() {
        let state = SnapState::new();
        let result = state.get_config("core22", None);
        assert!(result.is_ok());
        assert!(result.unwrap()[0].contains("No configuration"));
    }

    // === Aliases ===

    #[test]
    fn test_add_alias() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let result = state.add_alias("firefox", "ff", "firefox");
        assert!(result.is_ok());
    }

    #[test]
    fn test_add_alias_snap_not_installed() {
        let mut state = SnapState::new();
        let result = state.add_alias("nonexist", "ff", "cmd");
        assert!(result.is_err());
    }

    #[test]
    fn test_add_duplicate_alias() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.add_alias("firefox", "ff", "firefox");
        let result = state.add_alias("firefox", "ff", "firefox2");
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_alias() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.add_alias("firefox", "ff", "firefox");
        let result = state.remove_alias("ff");
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_alias_not_found() {
        let mut state = SnapState::new();
        let result = state.remove_alias("nonexist");
        assert!(result.is_err());
    }

    #[test]
    fn test_prefer() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.add_alias("firefox", "ff", "firefox");
        let result = state.prefer("firefox");
        assert!(result.is_ok());
        assert_eq!(state.aliases[0].status, AliasStatus::Auto);
    }

    #[test]
    fn test_prefer_not_installed() {
        let mut state = SnapState::new();
        let result = state.prefer("nonexist");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_aliases_has_header() {
        let state = SnapState::new();
        let lines = state.list_aliases();
        assert!(!lines.is_empty());
        assert!(lines[0].contains("Alias"));
    }

    // === Changes / tasks / abort / watch ===

    #[test]
    fn test_list_changes_has_header() {
        let state = SnapState::new();
        let lines = state.list_changes();
        assert!(!lines.is_empty());
        assert!(lines[0].contains("ID"));
    }

    #[test]
    fn test_list_changes_includes_initial() {
        let state = SnapState::new();
        let lines = state.list_changes();
        assert!(lines.iter().any(|l| l.contains("core22")));
    }

    #[test]
    fn test_list_tasks() {
        let state = SnapState::new();
        let result = state.list_tasks(1);
        assert!(result.is_ok());
        let lines = result.unwrap();
        assert!(lines.len() >= 2);
    }

    #[test]
    fn test_list_tasks_not_found() {
        let state = SnapState::new();
        let result = state.list_tasks(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_abort_doing_change() {
        let mut state = SnapState::new();
        // Add a "doing" change to abort
        state.changes.push(Change {
            id: state.next_change_id,
            status: ChangeStatus::Doing,
            kind: "test".to_string(),
            summary: "Test change".to_string(),
            tasks: vec![Task {
                id: state.next_task_id,
                kind: "test-task".to_string(),
                summary: "Test task".to_string(),
                status: ChangeStatus::Doing,
                progress_done: 0,
                progress_total: 1,
            }],
            ready_time: None,
            spawn_time: "2025-01-01T00:00:00Z".to_string(),
        });
        let cid = state.next_change_id;
        state.next_change_id += 1;
        state.next_task_id += 1;
        let result = state.abort_change(cid);
        assert!(result.is_ok());
    }

    #[test]
    fn test_abort_done_change() {
        let mut state = SnapState::new();
        let result = state.abort_change(1);
        assert!(result.is_err());
    }

    #[test]
    fn test_abort_not_found() {
        let mut state = SnapState::new();
        let result = state.abort_change(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_watch_change() {
        let state = SnapState::new();
        let result = state.watch_change(1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_watch_not_found() {
        let state = SnapState::new();
        let result = state.watch_change(999);
        assert!(result.is_err());
    }

    // === Snapshots ===

    #[test]
    fn test_save_snapshot() {
        let mut state = SnapState::new();
        let result = state.save_snapshot("core22");
        assert!(result.is_ok());
        assert_eq!(state.snapshots.len(), 1);
    }

    #[test]
    fn test_save_snapshot_not_installed() {
        let mut state = SnapState::new();
        let result = state.save_snapshot("nonexist");
        assert!(result.is_err());
    }

    #[test]
    fn test_restore_snapshot() {
        let mut state = SnapState::new();
        let _ = state.save_snapshot("core22");
        let result = state.restore_snapshot(1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_restore_snapshot_not_found() {
        let state = SnapState::new();
        let result = state.restore_snapshot(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_forget_snapshot() {
        let mut state = SnapState::new();
        let _ = state.save_snapshot("core22");
        let result = state.forget_snapshot(1);
        assert!(result.is_ok());
        assert!(state.snapshots.is_empty());
    }

    #[test]
    fn test_forget_snapshot_not_found() {
        let mut state = SnapState::new();
        let result = state.forget_snapshot(999);
        assert!(result.is_err());
    }

    // === Assertions ===

    #[test]
    fn test_known_assertions_empty() {
        let state = SnapState::new();
        let lines = state.known_assertions(None);
        assert!(lines.iter().any(|l| l.contains("No assertions")));
    }

    #[test]
    fn test_ack_assertion() {
        let mut state = SnapState::new();
        let result = state.ack_assertion("account", "account-id", "abc123");
        assert!(result.is_ok());
        assert_eq!(state.assertions.len(), 1);
    }

    #[test]
    fn test_known_after_ack() {
        let mut state = SnapState::new();
        let _ = state.ack_assertion("account", "account-id", "abc123");
        let lines = state.known_assertions(None);
        assert!(lines.iter().any(|l| l.contains("account")));
    }

    #[test]
    fn test_known_filtered_by_type() {
        let mut state = SnapState::new();
        let _ = state.ack_assertion("account", "id", "123");
        let _ = state.ack_assertion("snap-declaration", "snap-id", "456");
        let lines = state.known_assertions(Some("account"));
        assert!(lines.iter().any(|l| l.contains("account")));
        assert!(!lines.iter().any(|l| l.contains("snap-declaration")));
    }

    // === Download / pack ===

    #[test]
    fn test_download_existing() {
        let state = SnapState::new();
        let result = state.download_snap("firefox", None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_download_with_channel() {
        let state = SnapState::new();
        let result = state.download_snap("firefox", Some("beta"));
        assert!(result.is_ok());
        assert!(result.unwrap().contains("beta"));
    }

    #[test]
    fn test_download_not_in_store() {
        let state = SnapState::new();
        let result = state.download_snap("totally-fake", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_pack_snap() {
        let state = SnapState::new();
        let result = state.pack_snap("/path/to/snap");
        assert!(result.is_ok());
    }

    #[test]
    fn test_pack_empty_dir() {
        let state = SnapState::new();
        let result = state.pack_snap("");
        assert!(result.is_err());
    }

    // === Debug ===

    #[test]
    fn test_debug_connectivity() {
        let state = SnapState::new();
        let lines = state.debug_connectivity();
        assert!(lines.iter().any(|l| l.contains("Reachable")));
    }

    #[test]
    fn test_debug_timings() {
        let state = SnapState::new();
        let lines = state.debug_timings();
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_debug_state_changes() {
        let state = SnapState::new();
        let lines = state.debug_state_changes();
        assert!(lines[0].contains("State changes"));
    }

    #[test]
    fn test_debug_ensure_state_soon() {
        let state = SnapState::new();
        let msg = state.debug_ensure_state_soon();
        assert!(msg.contains("Ensured"));
    }

    // === Auth ===

    #[test]
    fn test_login() {
        let mut state = SnapState::new();
        let result = state.login("user@example.com");
        assert!(result.is_ok());
        assert!(state.auth.logged_in);
    }

    #[test]
    fn test_login_empty_email() {
        let mut state = SnapState::new();
        let result = state.login("");
        assert!(result.is_err());
    }

    #[test]
    fn test_login_invalid_email() {
        let mut state = SnapState::new();
        let result = state.login("not-an-email");
        assert!(result.is_err());
    }

    #[test]
    fn test_logout() {
        let mut state = SnapState::new();
        let _ = state.login("user@example.com");
        let result = state.logout();
        assert!(result.is_ok());
        assert!(!state.auth.logged_in);
    }

    #[test]
    fn test_logout_not_logged_in() {
        let mut state = SnapState::new();
        let result = state.logout();
        assert!(result.is_err());
    }

    #[test]
    fn test_whoami_logged_in() {
        let mut state = SnapState::new();
        let _ = state.login("user@example.com");
        let msg = state.whoami();
        assert!(msg.contains("user@example.com"));
    }

    #[test]
    fn test_whoami_not_logged_in() {
        let state = SnapState::new();
        let msg = state.whoami();
        assert!(msg.contains("not logged in"));
    }

    // === Version ===

    #[test]
    fn test_version_info() {
        let state = SnapState::new();
        let lines = state.version_info();
        assert!(lines.iter().any(|l| l.contains("snap")));
        assert!(lines.iter().any(|l| l.contains("snapd")));
    }

    // === Interface new() ===

    #[test]
    fn test_interface_new() {
        let iface = Interface::new("network", "core", "network-slot", "firefox", "network-plug");
        assert_eq!(iface.name, "network");
        assert_eq!(iface.slot_snap, "core");
        assert_eq!(iface.slot_name, "network-slot");
        assert_eq!(iface.plug_snap, "firefox");
        assert_eq!(iface.plug_name, "network-plug");
        assert!(!iface.auto_connect);
    }

    // === AuthState ===

    #[test]
    fn test_auth_state_empty() {
        let auth = AuthState::empty();
        assert!(!auth.logged_in);
        assert!(auth.email.is_empty());
    }

    // === add_change helper ===

    #[test]
    fn test_add_change_increments_ids() {
        let mut state = SnapState::new();
        let cid_before = state.next_change_id;
        let _cid = state.add_change("test", "Test change", &["task1", "task2"]);
        assert_eq!(state.next_change_id, cid_before + 1);
    }

    #[test]
    fn test_add_change_creates_tasks() {
        let mut state = SnapState::new();
        let cid = state.add_change("test", "Test change", &["t1", "t2", "t3"]);
        let change = state.changes.iter().find(|c| c.id == cid).unwrap();
        assert_eq!(change.tasks.len(), 3);
    }

    // === Service default name ===

    #[test]
    fn test_service_start_default_name() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let result = state.service_start("firefox", None);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("default"));
    }

    // === Multiple install + list ===

    #[test]
    fn test_install_multiple_and_list() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let _ = state.install_snap("vlc", None, None);
        let lines = state.list_snaps();
        assert!(lines.iter().any(|l| l.contains("firefox")));
        assert!(lines.iter().any(|l| l.contains("vlc")));
    }

    // === Config overwrite ===

    #[test]
    fn test_config_overwrite() {
        let mut state = SnapState::new();
        let _ = state.set_config("core22", "key", "old");
        let _ = state.set_config("core22", "key", "new");
        let result = state.get_config("core22", Some("key"));
        assert!(result.is_ok());
        assert!(result.unwrap()[0].contains("new"));
    }

    // === Edge: multiple snapshots ===

    #[test]
    fn test_multiple_snapshots() {
        let mut state = SnapState::new();
        let _ = state.save_snapshot("core22");
        let _ = state.save_snapshot("core22");
        assert_eq!(state.snapshots.len(), 2);
        assert_ne!(state.snapshots[0].id, state.snapshots[1].id);
    }

    // === Edge: install devmode ===

    #[test]
    fn test_install_devmode() {
        let mut state = SnapState::new();
        let result = state.install_snap("firefox", None, Some(Confinement::Devmode));
        assert!(result.is_ok());
        assert_eq!(state.snaps.get("firefox").unwrap().confinement, Confinement::Devmode);
    }

    // === Refresh bumps revision ===

    #[test]
    fn test_refresh_bumps_revision() {
        let mut state = SnapState::new();
        let _ = state.install_snap("firefox", None, None);
        let r1 = state.snaps.get("firefox").unwrap().revision;
        let _ = state.refresh_snap("firefox", None);
        let r2 = state.snaps.get("firefox").unwrap().revision;
        assert_eq!(r2, r1 + 1);
    }

    // === Abort sets task statuses ===

    #[test]
    fn test_abort_sets_task_status() {
        let mut state = SnapState::new();
        let cid = state.next_change_id;
        state.changes.push(Change {
            id: cid,
            status: ChangeStatus::Doing,
            kind: "test".to_string(),
            summary: "Test".to_string(),
            tasks: vec![
                Task { id: 100, kind: "t".to_string(), summary: "s".to_string(), status: ChangeStatus::Doing, progress_done: 0, progress_total: 1 },
                Task { id: 101, kind: "t".to_string(), summary: "s".to_string(), status: ChangeStatus::Done, progress_done: 1, progress_total: 1 },
            ],
            ready_time: None,
            spawn_time: "2025-01-01T00:00:00Z".to_string(),
        });
        state.next_change_id += 1;
        let _ = state.abort_change(cid);
        let change = state.changes.iter().find(|c| c.id == cid).unwrap();
        // The "Doing" task should become "Aborted", the "Done" task stays "Done"
        assert_eq!(change.tasks[0].status, ChangeStatus::Aborted);
        assert_eq!(change.tasks[1].status, ChangeStatus::Done);
    }
}

//! Enhanced process explorer features.
//!
//! Provides additional analysis and control capabilities beyond the base
//! process table:
//!
//! - **Window picker** ("crosshair" mode) — click any window to identify its
//!   owning process.
//! - **Blocking analyzer** — show what resources a process is waiting on,
//!   trace dependency chains, and detect deadlocks.
//! - **Affinity control** — view and modify CPU affinity masks.
//! - **Priority control** — change process scheduling priority.
//! - **Environment viewer** — browse and search a process's environment
//!   variables.
//! - **Memory map viewer** — display virtual memory regions with color-coded
//!   region types.
#![allow(dead_code)]

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::{HashMap, HashSet};

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const MOCHA_BASE: Color = Color::rgb(30, 30, 46);
const MOCHA_MANTLE: Color = Color::rgb(24, 24, 37);
const MOCHA_CRUST: Color = Color::rgb(17, 17, 27);
const MOCHA_SURFACE0: Color = Color::rgb(49, 50, 68);
const MOCHA_SURFACE1: Color = Color::rgb(69, 71, 90);
const MOCHA_SURFACE2: Color = Color::rgb(88, 91, 112);
const MOCHA_OVERLAY0: Color = Color::rgb(108, 112, 134);
const MOCHA_TEXT: Color = Color::rgb(205, 214, 244);
const MOCHA_SUBTEXT0: Color = Color::rgb(166, 173, 200);
const MOCHA_SUBTEXT1: Color = Color::rgb(186, 194, 222);
const MOCHA_RED: Color = Color::rgb(243, 139, 168);
const MOCHA_MAROON: Color = Color::rgb(235, 160, 172);
const MOCHA_PEACH: Color = Color::rgb(250, 179, 135);
const MOCHA_YELLOW: Color = Color::rgb(249, 226, 175);
const MOCHA_GREEN: Color = Color::rgb(166, 227, 161);
const MOCHA_TEAL: Color = Color::rgb(148, 226, 213);
const MOCHA_BLUE: Color = Color::rgb(137, 180, 250);
const MOCHA_LAVENDER: Color = Color::rgb(180, 190, 254);
const MOCHA_MAUVE: Color = Color::rgb(203, 166, 247);
const MOCHA_SKY: Color = Color::rgb(137, 220, 235);
const MOCHA_SAPPHIRE: Color = Color::rgb(116, 199, 236);
const MOCHA_FLAMINGO: Color = Color::rgb(242, 205, 205);
const MOCHA_ROSEWATER: Color = Color::rgb(245, 224, 220);

/// Standard row height used across feature panels.
const FEATURE_ROW_HEIGHT: f32 = 22.0;
/// Standard left padding for text in panels.
const FEATURE_TEXT_PAD: f32 = 8.0;
/// Standard font size for body text.
const FEATURE_FONT_SIZE: f32 = 13.0;
/// Standard font size for section headers.
const FEATURE_HEADER_FONT_SIZE: f32 = 14.0;

// ============================================================================
// ProcessAction — unified event enum for all features
// ============================================================================

/// Actions emitted by feature panels that the host should handle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessAction {
    /// Kill the process with the given PID.
    KillProcess(u32),
    /// Set the scheduling priority of a process.
    SetPriority(u32, PriorityLevel),
    /// Set the CPU affinity mask of a process.
    SetAffinity(u32, AffinityMask),
    /// Enter window-identification mode (crosshair).
    IdentifyWindow,
}

// ============================================================================
// 1. Window picker ("crosshair" mode)
// ============================================================================

/// Result of picking a window with the crosshair.
#[derive(Clone, Debug)]
pub struct PickResult {
    /// Window identifier.
    pub window_id: u64,
    /// PID of the process that owns the window.
    pub pid: u32,
    /// Name of the owning process.
    pub process_name: String,
    /// Title of the picked window.
    pub window_title: String,
}

/// State for the window-identification ("crosshair") mode.
///
/// When activated the cursor changes to a crosshair and the next click on any
/// window returns a [`PickResult`] identifying the owning process.  Press
/// Escape to cancel without picking.
#[derive(Clone, Debug)]
pub struct WindowPicker {
    /// Whether crosshair mode is currently active.
    pub active: bool,
    /// Window currently under the cursor (highlighted).
    pub hovered_window: Option<u64>,
    /// Result of the last completed pick, if any.
    pub result: Option<PickResult>,
}

impl WindowPicker {
    /// Create a new inactive picker.
    pub fn new() -> Self {
        Self {
            active: false,
            hovered_window: None,
            result: None,
        }
    }

    /// Enter crosshair mode.
    pub fn activate(&mut self) {
        self.active = true;
        self.hovered_window = None;
        self.result = None;
    }

    /// Cancel crosshair mode without picking.
    pub fn cancel(&mut self) {
        self.active = false;
        self.hovered_window = None;
    }

    /// Record a hover over a window (for highlighting).
    pub fn hover(&mut self, window_id: u64) {
        if self.active {
            self.hovered_window = Some(window_id);
        }
    }

    /// Complete a pick: record the result and deactivate.
    pub fn pick(&mut self, result: PickResult) {
        self.result = Some(result);
        self.active = false;
        self.hovered_window = None;
    }

    /// Return a mock pick result for UI testing.
    pub fn mock_pick() -> PickResult {
        PickResult {
            window_id: 0x1A3F,
            pid: 203,
            process_name: "editor".to_string(),
            window_title: "untitled.rs - SlateOS Editor".to_string(),
        }
    }

    /// Render the crosshair-mode overlay or the pick-result panel.
    ///
    /// When active, shows a semi-transparent overlay with instructions.
    /// When a result is available, shows the identified process info.
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        if self.active {
            // Overlay banner
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: FEATURE_ROW_HEIGHT * 2.0,
                color: Color::rgba(MOCHA_CRUST.r, MOCHA_CRUST.g, MOCHA_CRUST.b, 220),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + FEATURE_TEXT_PAD,
                y: y + 6.0,
                text: "Crosshair mode active".to_string(),
                color: MOCHA_YELLOW,
                font_size: FEATURE_HEADER_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + FEATURE_TEXT_PAD,
                y: y + 26.0,
                text: "Click a window to identify its process. Press Esc to cancel."
                    .to_string(),
                color: MOCHA_SUBTEXT0,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
            });
        } else if let Some(ref res) = self.result {
            // Result panel
            let panel_h = FEATURE_ROW_HEIGHT * 5.0;
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: panel_h,
                color: MOCHA_MANTLE,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::StrokeRect {
                x,
                y,
                width,
                height: panel_h,
                color: MOCHA_SURFACE1,
                line_width: 1.0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + FEATURE_TEXT_PAD,
                y: y + 4.0,
                text: "Identified Window".to_string(),
                color: MOCHA_BLUE,
                font_size: FEATURE_HEADER_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
            });

            let labels = [
                ("Window", format!("{:#06X}", res.window_id)),
                ("PID", res.pid.to_string()),
                ("Process", res.process_name.clone()),
                ("Title", res.window_title.clone()),
            ];
            for (i, (label, value)) in labels.iter().enumerate() {
                let row_y = y + FEATURE_ROW_HEIGHT + (i as f32 * FEATURE_ROW_HEIGHT);
                cmds.push(RenderCommand::Text {
                    x: x + FEATURE_TEXT_PAD,
                    y: row_y + 3.0,
                    text: format!("{label}:"),
                    color: MOCHA_SUBTEXT0,
                    font_size: FEATURE_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(80.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 90.0,
                    y: row_y + 3.0,
                    text: value.clone(),
                    color: MOCHA_TEXT,
                    font_size: FEATURE_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 98.0),
                });
            }
        }

        cmds
    }
}

impl Default for WindowPicker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 2. Process blocking / lock analysis
// ============================================================================

/// Why a process or thread is blocked.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WaitReason {
    /// Waiting on a mutex identified by its kernel object ID.
    Mutex(u64),
    /// Waiting on a semaphore.
    Semaphore(u64),
    /// Waiting on I/O to the given path.
    IO(String),
    /// Sleeping for the given number of milliseconds.
    Sleep(u64),
    /// Waiting on a network operation at the given address.
    Network(String),
    /// Waiting on an IPC channel.
    Channel(u64),
    /// Waiting on a futex at the given virtual address.
    Futex(u64),
    /// Not blocked.
    None,
}

impl WaitReason {
    /// Human-readable short label.
    pub fn label(&self) -> String {
        match self {
            Self::Mutex(id) => format!("Mutex #{id}"),
            Self::Semaphore(id) => format!("Semaphore #{id}"),
            Self::IO(path) => format!("I/O: {path}"),
            Self::Sleep(ms) => format!("Sleep {ms}ms"),
            Self::Network(addr) => format!("Net: {addr}"),
            Self::Channel(id) => format!("Channel #{id}"),
            Self::Futex(addr) => format!("Futex @{addr:#X}"),
            Self::None => "Not blocked".to_string(),
        }
    }

    /// Color for rendering this wait reason.
    pub fn color(&self) -> Color {
        match self {
            Self::Mutex(_) | Self::Futex(_) => MOCHA_RED,
            Self::Semaphore(_) => MOCHA_MAROON,
            Self::IO(_) => MOCHA_PEACH,
            Self::Sleep(_) => MOCHA_LAVENDER,
            Self::Network(_) => MOCHA_BLUE,
            Self::Channel(_) => MOCHA_TEAL,
            Self::None => MOCHA_GREEN,
        }
    }
}

/// A single link in a blocking dependency chain.
#[derive(Clone, Debug)]
pub struct BlockingLink {
    /// PID of the waiting process.
    pub waiter_pid: u32,
    /// Name of the waiting process.
    pub waiter_name: String,
    /// What it is waiting on.
    pub reason: WaitReason,
    /// PID of the process holding the resource (if known).
    pub holder_pid: Option<u32>,
    /// Name of the holder (if known).
    pub holder_name: Option<String>,
}

/// Full blocking analysis for a process.
#[derive(Clone, Debug)]
pub struct BlockingInfo {
    /// PID of the analyzed process.
    pub pid: u32,
    /// Direct wait reason for this process.
    pub direct_reason: WaitReason,
    /// Full dependency chain (may span multiple processes).
    pub chain: Vec<BlockingLink>,
    /// Whether a deadlock (circular wait) was detected.
    pub deadlock_detected: bool,
    /// PIDs involved in the deadlock cycle, if any.
    pub deadlock_cycle: Vec<u32>,
}

/// Analyzes process blocking relationships and detects deadlocks.
#[derive(Clone, Debug)]
pub struct BlockingAnalyzer {
    /// Map from PID to its current wait reason.
    wait_reasons: HashMap<u32, WaitReason>,
    /// Map from resource to the PID holding it.
    /// The key is a string representation of the `WaitReason` resource.
    resource_holders: HashMap<String, (u32, String)>,
    /// Map from PID to process name.
    process_names: HashMap<u32, String>,
}

impl BlockingAnalyzer {
    /// Create a new empty analyzer.
    pub fn new() -> Self {
        Self {
            wait_reasons: HashMap::new(),
            resource_holders: HashMap::new(),
            process_names: HashMap::new(),
        }
    }

    /// Register a process and its current wait state.
    pub fn register_process(&mut self, pid: u32, name: &str, reason: WaitReason) {
        self.process_names.insert(pid, name.to_string());
        self.wait_reasons.insert(pid, reason);
    }

    /// Register which process holds a particular resource.
    pub fn register_holder(&mut self, resource_key: &str, pid: u32, name: &str) {
        self.resource_holders
            .insert(resource_key.to_string(), (pid, name.to_string()));
    }

    /// Build the dependency chain starting from `pid`.
    ///
    /// Follows holder relationships: if process A waits on Mutex #1 and
    /// process B holds Mutex #1, the chain continues to whatever B is
    /// waiting on, and so forth.
    pub fn analyze_blocking(&self, pid: u32) -> BlockingInfo {
        let direct_reason = self
            .wait_reasons
            .get(&pid)
            .cloned()
            .unwrap_or(WaitReason::None);

        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut deadlock_detected = false;
        let mut deadlock_cycle = Vec::new();

        let mut current_pid = pid;
        visited.insert(current_pid);

        loop {
            let reason = match self.wait_reasons.get(&current_pid) {
                Some(r) if *r != WaitReason::None => r.clone(),
                _ => break,
            };

            let resource_key = reason.label();
            let (holder_pid, holder_name) = self
                .resource_holders
                .get(&resource_key)
                .map(|(p, n)| (Some(*p), Some(n.clone())))
                .unwrap_or((None, None));

            let waiter_name = self
                .process_names
                .get(&current_pid)
                .cloned()
                .unwrap_or_else(|| format!("pid:{current_pid}"));

            chain.push(BlockingLink {
                waiter_pid: current_pid,
                waiter_name,
                reason,
                holder_pid,
                holder_name,
            });

            match holder_pid {
                Some(hp) => {
                    if !visited.insert(hp) {
                        // We have seen this PID before: deadlock cycle.
                        deadlock_detected = true;
                        deadlock_cycle = self.extract_cycle(&chain, hp);
                        break;
                    }
                    current_pid = hp;
                }
                None => break,
            }
        }

        BlockingInfo {
            pid,
            direct_reason,
            chain,
            deadlock_detected,
            deadlock_cycle,
        }
    }

    /// Extract the PIDs forming a cycle ending at `cycle_pid`.
    fn extract_cycle(&self, chain: &[BlockingLink], cycle_pid: u32) -> Vec<u32> {
        let mut cycle = Vec::new();
        let mut found_start = false;
        for link in chain {
            if link.waiter_pid == cycle_pid {
                found_start = true;
            }
            if found_start {
                cycle.push(link.waiter_pid);
            }
        }
        // Close the cycle.
        cycle.push(cycle_pid);
        cycle
    }

    /// Scan all registered processes and return every deadlock cycle found.
    pub fn detect_all_deadlocks(&self) -> Vec<Vec<u32>> {
        let mut all_cycles: Vec<Vec<u32>> = Vec::new();
        let mut globally_visited: HashSet<u32> = HashSet::new();

        for &pid in self.wait_reasons.keys() {
            if globally_visited.contains(&pid) {
                continue;
            }
            let info = self.analyze_blocking(pid);
            for &p in &info.chain.iter().map(|l| l.waiter_pid).collect::<Vec<_>>() {
                globally_visited.insert(p);
            }
            if info.deadlock_detected && !info.deadlock_cycle.is_empty() {
                // Deduplicate: only keep the cycle if we haven't already
                // recorded it with a different starting point.
                let cycle_set: HashSet<u32> = info.deadlock_cycle.iter().copied().collect();
                let already_found = all_cycles.iter().any(|c| {
                    let s: HashSet<u32> = c.iter().copied().collect();
                    s == cycle_set
                });
                if !already_found {
                    all_cycles.push(info.deadlock_cycle);
                }
            }
        }

        all_cycles
    }

    /// Create an analyzer pre-loaded with realistic stub data.
    pub fn with_demo_data() -> Self {
        let mut a = Self::new();

        // Simple chain: editor -> Mutex #42 -> held by compositor
        a.register_process(203, "editor", WaitReason::Mutex(42));
        a.register_holder("Mutex #42", 100, "compositor");
        a.register_process(100, "compositor", WaitReason::IO("/dev/gpu0".to_string()));

        // Deadlock: httpd-worker -> Mutex #7 -> held by netd
        //           netd -> Channel #15 -> held by httpd-worker
        a.register_process(301, "httpd-worker", WaitReason::Mutex(7));
        a.register_holder("Mutex #7", 101, "netd");
        a.register_process(101, "netd", WaitReason::Channel(15));
        a.register_holder("Channel #15", 301, "httpd-worker");

        // Sleeping process (no chain).
        a.register_process(202, "terminal", WaitReason::Sleep(5000));

        // Network wait.
        a.register_process(
            302,
            "httpd-worker",
            WaitReason::Network("192.168.1.50:443".to_string()),
        );

        // Process not blocked.
        a.register_process(200, "desktop", WaitReason::None);

        // Futex wait.
        a.register_process(201, "explorer", WaitReason::Futex(0x7FFE_0010_A000));

        a
    }

    /// Render the blocking analysis for a process.
    pub fn render(&self, pid: u32, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let info = self.analyze_blocking(pid);
        let mut cmds = Vec::new();
        let mut cy = y;

        // Section header.
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: FEATURE_ROW_HEIGHT,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + FEATURE_TEXT_PAD,
            y: cy + 4.0,
            text: format!("Blocking Analysis  PID {pid}"),
            color: MOCHA_BLUE,
            font_size: FEATURE_HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
        });
        cy += FEATURE_ROW_HEIGHT;

        // Deadlock warning.
        if info.deadlock_detected {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: FEATURE_ROW_HEIGHT,
                color: Color::rgba(243, 139, 168, 40),
                corner_radii: CornerRadii::ZERO,
            });
            let cycle_str: Vec<String> = info
                .deadlock_cycle
                .iter()
                .map(|p| {
                    self.process_names
                        .get(p)
                        .cloned()
                        .unwrap_or_else(|| p.to_string())
                })
                .collect();
            cmds.push(RenderCommand::Text {
                x: x + FEATURE_TEXT_PAD,
                y: cy + 4.0,
                text: format!("DEADLOCK: {}", cycle_str.join(" -> ")),
                color: MOCHA_RED,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
            });
            cy += FEATURE_ROW_HEIGHT;
        }

        // Dependency chain.
        if info.chain.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + FEATURE_TEXT_PAD,
                y: cy + 4.0,
                text: "Process is not blocked.".to_string(),
                color: MOCHA_GREEN,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
            });
        } else {
            for (i, link) in info.chain.iter().enumerate() {
                let indent = (i as f32) * 20.0;
                let bg = if i % 2 == 0 {
                    MOCHA_BASE
                } else {
                    MOCHA_MANTLE
                };
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: cy,
                    width,
                    height: FEATURE_ROW_HEIGHT,
                    color: bg,
                    corner_radii: CornerRadii::ZERO,
                });

                // Arrow connector.
                if i > 0 {
                    cmds.push(RenderCommand::Text {
                        x: x + FEATURE_TEXT_PAD + indent - 16.0,
                        y: cy + 4.0,
                        text: "->".to_string(),
                        color: MOCHA_OVERLAY0,
                        font_size: FEATURE_FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }

                // Process name and wait reason.
                let text = format!(
                    "{} (PID {}) waiting on {}",
                    link.waiter_name,
                    link.waiter_pid,
                    link.reason.label()
                );
                cmds.push(RenderCommand::Text {
                    x: x + FEATURE_TEXT_PAD + indent,
                    y: cy + 4.0,
                    text,
                    color: link.reason.color(),
                    font_size: FEATURE_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - indent - FEATURE_TEXT_PAD * 2.0),
                });

                // Holder info on same row if known.
                if let Some(ref hname) = link.holder_name {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 180.0,
                        y: cy + 4.0,
                        text: format!("held by {hname}"),
                        color: MOCHA_SUBTEXT0,
                        font_size: FEATURE_FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(170.0),
                    });
                }

                cy += FEATURE_ROW_HEIGHT;
            }
        }

        cmds
    }
}

impl Default for BlockingAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 3. Process affinity control
// ============================================================================

/// CPU affinity mask — a bitset indicating which CPUs a process may run on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AffinityMask {
    /// Bitmask where bit N means CPU N is allowed.
    bits: u64,
    /// Total number of CPUs in the system.
    cpu_count: u32,
}

impl AffinityMask {
    /// Create a mask with all CPUs enabled.
    pub fn all(cpu_count: u32) -> Self {
        let count = cpu_count.min(64);
        let bits = if count >= 64 {
            u64::MAX
        } else {
            (1u64 << count) - 1
        };
        Self {
            bits,
            cpu_count: count,
        }
    }

    /// Create a mask with a single CPU enabled.
    pub fn single(cpu: u32, cpu_count: u32) -> Self {
        let count = cpu_count.min(64);
        let cpu = cpu.min(count.saturating_sub(1));
        Self {
            bits: 1u64 << cpu,
            cpu_count: count,
        }
    }

    /// Create a mask with even-numbered CPUs enabled (0, 2, 4, ...).
    pub fn even_cores(cpu_count: u32) -> Self {
        let count = cpu_count.min(64);
        let mut bits = 0u64;
        let mut i = 0u32;
        while i < count {
            bits |= 1u64 << i;
            i += 2;
        }
        Self {
            bits,
            cpu_count: count,
        }
    }

    /// Create a mask with odd-numbered CPUs enabled (1, 3, 5, ...).
    pub fn odd_cores(cpu_count: u32) -> Self {
        let count = cpu_count.min(64);
        let mut bits = 0u64;
        let mut i = 1u32;
        while i < count {
            bits |= 1u64 << i;
            i += 2;
        }
        Self {
            bits,
            cpu_count: count,
        }
    }

    /// Create a mask from a raw bitmask.
    pub fn from_bits(bits: u64, cpu_count: u32) -> Self {
        let count = cpu_count.min(64);
        let mask = if count >= 64 {
            u64::MAX
        } else {
            (1u64 << count) - 1
        };
        Self {
            bits: bits & mask,
            cpu_count: count,
        }
    }

    /// Whether a specific CPU is enabled in this mask.
    pub fn is_cpu_enabled(&self, cpu: u32) -> bool {
        if cpu >= self.cpu_count {
            return false;
        }
        (self.bits >> cpu) & 1 == 1
    }

    /// Toggle a specific CPU in the mask.
    ///
    /// Returns `false` if the toggle would result in an empty mask (at least
    /// one CPU must remain enabled).
    pub fn toggle_cpu(&mut self, cpu: u32) -> bool {
        if cpu >= self.cpu_count {
            return false;
        }
        let new_bits = self.bits ^ (1u64 << cpu);
        if new_bits == 0 {
            // Refuse to create an empty mask.
            return false;
        }
        self.bits = new_bits;
        true
    }

    /// Number of CPUs enabled in this mask.
    pub fn enabled_count(&self) -> u32 {
        self.bits.count_ones()
    }

    /// Total number of CPUs in the system.
    pub fn cpu_count(&self) -> u32 {
        self.cpu_count
    }

    /// Raw bitmask value.
    pub fn bits(&self) -> u64 {
        self.bits
    }

    /// Emit a `ProcessAction::SetAffinity` for the given PID.
    pub fn make_action(&self, pid: u32) -> ProcessAction {
        ProcessAction::SetAffinity(pid, self.clone())
    }

    /// Render the affinity editor UI.
    ///
    /// Shows a grid of CPU toggles with preset buttons.
    pub fn render(&self, pid: u32, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let mut cy = y;

        // Header.
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: FEATURE_ROW_HEIGHT,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + FEATURE_TEXT_PAD,
            y: cy + 4.0,
            text: format!(
                "CPU Affinity  PID {pid}  ({}/{} cores)",
                self.enabled_count(),
                self.cpu_count
            ),
            color: MOCHA_BLUE,
            font_size: FEATURE_HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
        });
        cy += FEATURE_ROW_HEIGHT + 4.0;

        // Preset buttons row.
        let presets = ["All", "Single", "Even", "Odd"];
        let btn_w = 60.0f32;
        let btn_h = 20.0f32;
        let btn_gap = 8.0f32;
        for (i, label) in presets.iter().enumerate() {
            let bx = x + FEATURE_TEXT_PAD + (i as f32 * (btn_w + btn_gap));
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: cy,
                width: btn_w,
                height: btn_h,
                color: MOCHA_SURFACE0,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: cy + 3.0,
                text: label.to_string(),
                color: MOCHA_TEXT,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(btn_w - 16.0),
            });
        }
        cy += btn_h + 6.0;

        // CPU grid: each CPU is a small square.
        let cell_size = 28.0f32;
        let cell_gap = 4.0f32;
        let cols = ((width - FEATURE_TEXT_PAD * 2.0) / (cell_size + cell_gap)).floor() as u32;
        let cols = cols.max(1);

        for cpu in 0..self.cpu_count {
            let col = cpu % cols;
            let row = cpu / cols;
            let cx = x + FEATURE_TEXT_PAD + (col as f32 * (cell_size + cell_gap));
            let cell_y = cy + (row as f32 * (cell_size + cell_gap));

            let (bg, fg) = if self.is_cpu_enabled(cpu) {
                (MOCHA_GREEN, MOCHA_CRUST)
            } else {
                (MOCHA_SURFACE0, MOCHA_OVERLAY0)
            };

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: cell_y,
                width: cell_size,
                height: cell_size,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 6.0,
                y: cell_y + 7.0,
                text: cpu.to_string(),
                color: fg,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(cell_size - 8.0),
            });
        }

        cmds
    }
}

// ============================================================================
// 4. Process priority control
// ============================================================================

/// Scheduling priority levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PriorityLevel {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

impl PriorityLevel {
    /// All levels in ascending order.
    pub const ALL: [PriorityLevel; 6] = [
        PriorityLevel::Idle,
        PriorityLevel::BelowNormal,
        PriorityLevel::Normal,
        PriorityLevel::AboveNormal,
        PriorityLevel::High,
        PriorityLevel::Realtime,
    ];

    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::BelowNormal => "Below Normal",
            Self::Normal => "Normal",
            Self::AboveNormal => "Above Normal",
            Self::High => "High",
            Self::Realtime => "Realtime",
        }
    }

    /// Numeric value for the scheduler (-20 .. 20 range analogous to nice).
    pub fn nice_value(self) -> i32 {
        match self {
            Self::Idle => 19,
            Self::BelowNormal => 10,
            Self::Normal => 0,
            Self::AboveNormal => -5,
            Self::High => -10,
            Self::Realtime => -20,
        }
    }

    /// Color for rendering.
    pub fn color(self) -> Color {
        match self {
            Self::Idle => MOCHA_OVERLAY0,
            Self::BelowNormal => MOCHA_SUBTEXT0,
            Self::Normal => MOCHA_TEXT,
            Self::AboveNormal => MOCHA_YELLOW,
            Self::High => MOCHA_PEACH,
            Self::Realtime => MOCHA_RED,
        }
    }

    /// Whether changing to this level should show a warning.
    pub fn needs_warning(self) -> bool {
        matches!(self, Self::Realtime)
    }
}

/// Priority selector UI state.
#[derive(Clone, Debug)]
pub struct PrioritySelector {
    /// PID being modified.
    pub pid: u32,
    /// Current priority of the process.
    pub current: PriorityLevel,
    /// Whether the dropdown is open.
    pub dropdown_open: bool,
    /// Currently hovered item in the dropdown.
    pub hover_index: Option<usize>,
    /// Whether the realtime warning dialog is showing.
    pub warning_visible: bool,
    /// Pending priority that triggered the warning.
    pending_level: Option<PriorityLevel>,
}

impl PrioritySelector {
    /// Create a new selector for a process.
    pub fn new(pid: u32, current: PriorityLevel) -> Self {
        Self {
            pid,
            current,
            dropdown_open: false,
            hover_index: None,
            warning_visible: false,
            pending_level: None,
        }
    }

    /// Toggle the dropdown.
    pub fn toggle_dropdown(&mut self) {
        self.dropdown_open = !self.dropdown_open;
        self.hover_index = None;
    }

    /// Select a priority level.
    ///
    /// If the level is `Realtime`, shows a warning dialog instead of applying
    /// immediately. Returns the action to emit, if any.
    pub fn select(&mut self, level: PriorityLevel) -> Option<ProcessAction> {
        self.dropdown_open = false;
        if level.needs_warning() {
            self.warning_visible = true;
            self.pending_level = Some(level);
            None
        } else {
            self.current = level;
            Some(ProcessAction::SetPriority(self.pid, level))
        }
    }

    /// Confirm the pending realtime priority change.
    pub fn confirm_warning(&mut self) -> Option<ProcessAction> {
        self.warning_visible = false;
        if let Some(level) = self.pending_level.take() {
            self.current = level;
            Some(ProcessAction::SetPriority(self.pid, level))
        } else {
            None
        }
    }

    /// Dismiss the warning without applying.
    pub fn dismiss_warning(&mut self) {
        self.warning_visible = false;
        self.pending_level = None;
    }

    /// Render the priority selector.
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let mut cy = y;

        // Header.
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: FEATURE_ROW_HEIGHT,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + FEATURE_TEXT_PAD,
            y: cy + 4.0,
            text: format!("Priority  PID {}", self.pid),
            color: MOCHA_BLUE,
            font_size: FEATURE_HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
        });
        cy += FEATURE_ROW_HEIGHT;

        // Current value button.
        cmds.push(RenderCommand::FillRect {
            x: x + FEATURE_TEXT_PAD,
            y: cy,
            width: width - FEATURE_TEXT_PAD * 2.0,
            height: FEATURE_ROW_HEIGHT,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + FEATURE_TEXT_PAD + 8.0,
            y: cy + 4.0,
            text: self.current.label().to_string(),
            color: self.current.color(),
            font_size: FEATURE_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - FEATURE_TEXT_PAD * 2.0 - 30.0),
        });
        // Dropdown arrow.
        let arrow = if self.dropdown_open { "^" } else { "v" };
        cmds.push(RenderCommand::Text {
            x: x + width - FEATURE_TEXT_PAD - 20.0,
            y: cy + 4.0,
            text: arrow.to_string(),
            color: MOCHA_SUBTEXT0,
            font_size: FEATURE_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cy += FEATURE_ROW_HEIGHT;

        // Dropdown items.
        if self.dropdown_open {
            for (i, level) in PriorityLevel::ALL.iter().enumerate() {
                let hovered = self.hover_index == Some(i);
                let bg = if *level == self.current {
                    MOCHA_SURFACE1
                } else if hovered {
                    MOCHA_SURFACE0
                } else {
                    MOCHA_MANTLE
                };
                cmds.push(RenderCommand::FillRect {
                    x: x + FEATURE_TEXT_PAD,
                    y: cy,
                    width: width - FEATURE_TEXT_PAD * 2.0,
                    height: FEATURE_ROW_HEIGHT,
                    color: bg,
                    corner_radii: CornerRadii::ZERO,
                });
                cmds.push(RenderCommand::Text {
                    x: x + FEATURE_TEXT_PAD + 8.0,
                    y: cy + 4.0,
                    text: level.label().to_string(),
                    color: level.color(),
                    font_size: FEATURE_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - FEATURE_TEXT_PAD * 2.0 - 16.0),
                });
                cy += FEATURE_ROW_HEIGHT;
            }
        }

        // Realtime warning dialog overlay.
        if self.warning_visible {
            let dlg_w = 300.0f32;
            let dlg_h = 100.0f32;
            let dlg_x = x + (width - dlg_w) / 2.0;
            let dlg_y = y + 40.0;

            // Shadow.
            cmds.push(RenderCommand::BoxShadow {
                x: dlg_x,
                y: dlg_y,
                width: dlg_w,
                height: dlg_h,
                offset_x: 0.0,
                offset_y: 4.0,
                blur: 12.0,
                spread: 0.0,
                color: Color::rgba(0, 0, 0, 120),
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::FillRect {
                x: dlg_x,
                y: dlg_y,
                width: dlg_w,
                height: dlg_h,
                color: MOCHA_CRUST,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: dlg_x,
                y: dlg_y,
                width: dlg_w,
                height: dlg_h,
                color: MOCHA_RED,
                line_width: 1.0,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: dlg_x + 12.0,
                y: dlg_y + 12.0,
                text: "Warning".to_string(),
                color: MOCHA_RED,
                font_size: FEATURE_HEADER_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(dlg_w - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: dlg_x + 12.0,
                y: dlg_y + 34.0,
                text: "Realtime priority can make the system".to_string(),
                color: MOCHA_TEXT,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dlg_w - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: dlg_x + 12.0,
                y: dlg_y + 50.0,
                text: "unresponsive. Continue?".to_string(),
                color: MOCHA_TEXT,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dlg_w - 24.0),
            });

            // Confirm button.
            let btn_y = dlg_y + dlg_h - 30.0;
            cmds.push(RenderCommand::FillRect {
                x: dlg_x + dlg_w - 150.0,
                y: btn_y,
                width: 60.0,
                height: 22.0,
                color: MOCHA_RED,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: dlg_x + dlg_w - 142.0,
                y: btn_y + 4.0,
                text: "Yes".to_string(),
                color: MOCHA_CRUST,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            // Cancel button.
            cmds.push(RenderCommand::FillRect {
                x: dlg_x + dlg_w - 80.0,
                y: btn_y,
                width: 60.0,
                height: 22.0,
                color: MOCHA_SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: dlg_x + dlg_w - 72.0,
                y: btn_y + 4.0,
                text: "Cancel".to_string(),
                color: MOCHA_TEXT,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        cmds
    }
}

// ============================================================================
// 5. Process environment viewer
// ============================================================================

/// A single environment variable entry.
#[derive(Clone, Debug)]
pub struct EnvEntry {
    /// Variable name.
    pub name: String,
    /// Variable value.
    pub value: String,
}

/// Sortable columns in the environment viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvSortColumn {
    Name,
    Value,
}

/// Environment variable viewer for a process.
#[derive(Clone, Debug)]
pub struct EnvViewer {
    /// PID of the process being viewed.
    pub pid: u32,
    /// Full list of environment variables.
    entries: Vec<EnvEntry>,
    /// Current search/filter text (case-insensitive).
    filter: String,
    /// Current sort column.
    sort_column: EnvSortColumn,
    /// Sort ascending.
    sort_ascending: bool,
    /// Index of the selected row in the filtered list.
    selected_index: Option<usize>,
    /// Scroll offset (number of rows scrolled past).
    scroll_offset: usize,
}

impl EnvViewer {
    /// Create a new viewer for a process with the given environment.
    pub fn new(pid: u32, env: Vec<(String, String)>) -> Self {
        let entries = env
            .into_iter()
            .map(|(name, value)| EnvEntry { name, value })
            .collect();
        let mut viewer = Self {
            pid,
            entries,
            filter: String::new(),
            sort_column: EnvSortColumn::Name,
            sort_ascending: true,
            selected_index: None,
            scroll_offset: 0,
        };
        viewer.sort();
        viewer
    }

    /// Set the search filter text.
    pub fn set_filter(&mut self, text: &str) {
        self.filter = text.to_lowercase();
        self.selected_index = None;
        self.scroll_offset = 0;
    }

    /// Return entries matching the current filter.
    pub fn filtered_entries(&self) -> Vec<&EnvEntry> {
        if self.filter.is_empty() {
            self.entries.iter().collect()
        } else {
            self.entries
                .iter()
                .filter(|e| {
                    e.name.to_lowercase().contains(&self.filter)
                        || e.value.to_lowercase().contains(&self.filter)
                })
                .collect()
        }
    }

    /// Sort entries by the current column and direction.
    fn sort(&mut self) {
        let asc = self.sort_ascending;
        match self.sort_column {
            EnvSortColumn::Name => {
                self.entries
                    .sort_by(|a, b| {
                        let cmp = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                        if asc { cmp } else { cmp.reverse() }
                    });
            }
            EnvSortColumn::Value => {
                self.entries
                    .sort_by(|a, b| {
                        let cmp = a.value.to_lowercase().cmp(&b.value.to_lowercase());
                        if asc { cmp } else { cmp.reverse() }
                    });
            }
        }
    }

    /// Change the sort column (toggles direction if same column).
    pub fn set_sort(&mut self, col: EnvSortColumn) {
        if self.sort_column == col {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = col;
            self.sort_ascending = true;
        }
        self.sort();
    }

    /// Get the value of the currently selected entry (for clipboard copy).
    pub fn selected_value(&self) -> Option<String> {
        let filtered = self.filtered_entries();
        self.selected_index
            .and_then(|i| filtered.get(i))
            .map(|e| e.value.clone())
    }

    /// Get the name of the currently selected entry.
    pub fn selected_name(&self) -> Option<String> {
        let filtered = self.filtered_entries();
        self.selected_index
            .and_then(|i| filtered.get(i))
            .map(|e| e.name.clone())
    }

    /// Create a viewer pre-loaded with realistic stub data.
    pub fn with_demo_data(pid: u32) -> Self {
        Self::new(
            pid,
            vec![
                ("PATH".to_string(), "/usr/bin:/usr/local/bin:/bin".to_string()),
                ("HOME".to_string(), "/home/user".to_string()),
                ("USER".to_string(), "user".to_string()),
                ("SHELL".to_string(), "/usr/bin/oursh".to_string()),
                ("LANG".to_string(), "en_US.UTF-8".to_string()),
                ("TERM".to_string(), "slateos-256color".to_string()),
                ("DISPLAY".to_string(), ":0".to_string()),
                ("XDG_RUNTIME_DIR".to_string(), "/run/user/1000".to_string()),
                ("XDG_DATA_HOME".to_string(), "/home/user/.local/share".to_string()),
                ("XDG_CONFIG_HOME".to_string(), "/home/user/.config".to_string()),
                ("XDG_CACHE_HOME".to_string(), "/home/user/.cache".to_string()),
                ("EDITOR".to_string(), "oured".to_string()),
                ("PAGER".to_string(), "less".to_string()),
                ("RUST_LOG".to_string(), "info".to_string()),
                ("GPU_DRIVER".to_string(), "virtio-gpu".to_string()),
                ("DBUS_SESSION_BUS_ADDRESS".to_string(), "unix:path=/run/user/1000/bus".to_string()),
                ("SSH_AUTH_SOCK".to_string(), "/run/user/1000/ssh-agent.sock".to_string()),
                ("LD_LIBRARY_PATH".to_string(), "/usr/lib:/usr/local/lib".to_string()),
                ("PKG_STORE".to_string(), "/nix/store".to_string()),
                ("COLORTERM".to_string(), "truecolor".to_string()),
            ],
        )
    }

    /// Render the environment viewer.
    pub fn render(&self, x: f32, y: f32, width: f32, max_rows: usize) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let mut cy = y;
        let name_col_w = width * 0.35;

        // Header.
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: FEATURE_ROW_HEIGHT,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + FEATURE_TEXT_PAD,
            y: cy + 4.0,
            text: format!("Environment  PID {}", self.pid),
            color: MOCHA_BLUE,
            font_size: FEATURE_HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
        });
        cy += FEATURE_ROW_HEIGHT;

        // Filter bar.
        cmds.push(RenderCommand::FillRect {
            x: x + FEATURE_TEXT_PAD,
            y: cy,
            width: width - FEATURE_TEXT_PAD * 2.0,
            height: FEATURE_ROW_HEIGHT,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        let filter_text = if self.filter.is_empty() {
            "Search...".to_string()
        } else {
            self.filter.clone()
        };
        let filter_color = if self.filter.is_empty() {
            MOCHA_OVERLAY0
        } else {
            MOCHA_TEXT
        };
        cmds.push(RenderCommand::Text {
            x: x + FEATURE_TEXT_PAD + 8.0,
            y: cy + 4.0,
            text: filter_text,
            color: filter_color,
            font_size: FEATURE_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - FEATURE_TEXT_PAD * 2.0 - 16.0),
        });
        cy += FEATURE_ROW_HEIGHT + 2.0;

        // Column headers.
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: FEATURE_ROW_HEIGHT,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        let name_arrow = if self.sort_column == EnvSortColumn::Name {
            if self.sort_ascending { " ^" } else { " v" }
        } else {
            ""
        };
        let value_arrow = if self.sort_column == EnvSortColumn::Value {
            if self.sort_ascending { " ^" } else { " v" }
        } else {
            ""
        };
        cmds.push(RenderCommand::Text {
            x: x + FEATURE_TEXT_PAD,
            y: cy + 4.0,
            text: format!("Name{name_arrow}"),
            color: MOCHA_SUBTEXT1,
            font_size: FEATURE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(name_col_w - FEATURE_TEXT_PAD),
        });
        cmds.push(RenderCommand::Text {
            x: x + name_col_w,
            y: cy + 4.0,
            text: format!("Value{value_arrow}"),
            color: MOCHA_SUBTEXT1,
            font_size: FEATURE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - name_col_w - FEATURE_TEXT_PAD),
        });
        cy += FEATURE_ROW_HEIGHT;

        // Rows.
        let filtered = self.filtered_entries();
        let visible_count = max_rows.min(filtered.len().saturating_sub(self.scroll_offset));
        for i in 0..visible_count {
            let entry_idx = self.scroll_offset + i;
            let entry = match filtered.get(entry_idx) {
                Some(e) => e,
                None => break,
            };

            let selected = self.selected_index == Some(entry_idx);
            let bg = if selected {
                MOCHA_SURFACE1
            } else if i % 2 == 0 {
                MOCHA_BASE
            } else {
                MOCHA_MANTLE
            };

            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: FEATURE_ROW_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });
            cmds.push(RenderCommand::Text {
                x: x + FEATURE_TEXT_PAD,
                y: cy + 4.0,
                text: entry.name.clone(),
                color: MOCHA_GREEN,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(name_col_w - FEATURE_TEXT_PAD * 2.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + name_col_w,
                y: cy + 4.0,
                text: entry.value.clone(),
                color: MOCHA_TEXT,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - name_col_w - FEATURE_TEXT_PAD),
            });

            cy += FEATURE_ROW_HEIGHT;
        }

        // Footer with count.
        let total = filtered.len();
        cmds.push(RenderCommand::Text {
            x: x + FEATURE_TEXT_PAD,
            y: cy + 4.0,
            text: format!(
                "{total} variable{}{}",
                if total == 1 { "" } else { "s" },
                if self.filter.is_empty() {
                    String::new()
                } else {
                    format!(" (filtered from {})", self.entries.len())
                }
            ),
            color: MOCHA_OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
        });

        cmds
    }
}

// ============================================================================
// 6. Process memory map
// ============================================================================

/// Type of a virtual memory region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegionType {
    /// Executable code (.text).
    Code,
    /// Initialized and uninitialized data (.data, .bss).
    Data,
    /// Thread stack.
    Stack,
    /// Heap (dynamic allocations).
    Heap,
    /// Memory-mapped file.
    MappedFile,
    /// Shared memory region (IPC).
    Shared,
}

impl RegionType {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Code => "Code",
            Self::Data => "Data",
            Self::Stack => "Stack",
            Self::Heap => "Heap",
            Self::MappedFile => "Mapped File",
            Self::Shared => "Shared",
        }
    }

    /// Color for rendering this region type.
    pub fn color(self) -> Color {
        match self {
            Self::Code => MOCHA_BLUE,
            Self::Data => MOCHA_GREEN,
            Self::Stack => MOCHA_MAUVE,
            Self::Heap => MOCHA_PEACH,
            Self::MappedFile => MOCHA_TEAL,
            Self::Shared => MOCHA_YELLOW,
        }
    }
}

/// Memory protection flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Protection {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl Protection {
    /// Format as a "rwx" string.
    pub fn to_rwx(self) -> String {
        format!(
            "{}{}{}",
            if self.read { "r" } else { "-" },
            if self.write { "w" } else { "-" },
            if self.execute { "x" } else { "-" },
        )
    }
}

/// A single virtual memory region.
#[derive(Clone, Debug)]
pub struct MemoryRegion {
    /// Start virtual address.
    pub start_addr: u64,
    /// End virtual address (exclusive).
    pub end_addr: u64,
    /// Region type.
    pub region_type: RegionType,
    /// Memory protection flags.
    pub protection: Protection,
    /// Backing description (file path, "[heap]", "[stack]", etc.).
    pub backing: String,
    /// Committed (resident) bytes.
    pub committed: u64,
    /// Reserved (mapped) bytes.
    pub reserved: u64,
}

impl MemoryRegion {
    /// Size of the region in bytes.
    pub fn size(&self) -> u64 {
        self.end_addr.saturating_sub(self.start_addr)
    }
}

/// Memory map viewer for a process.
#[derive(Clone, Debug)]
pub struct MemoryMap {
    /// PID of the process.
    pub pid: u32,
    /// All memory regions, sorted by start address.
    pub regions: Vec<MemoryRegion>,
    /// Total committed memory across all regions.
    pub total_committed: u64,
    /// Total reserved memory across all regions.
    pub total_reserved: u64,
}

impl MemoryMap {
    /// Create a memory map from a list of regions.
    pub fn new(pid: u32, mut regions: Vec<MemoryRegion>) -> Self {
        regions.sort_by_key(|r| r.start_addr);
        let total_committed = regions.iter().map(|r| r.committed).sum();
        let total_reserved = regions.iter().map(|r| r.reserved).sum();
        Self {
            pid,
            regions,
            total_committed,
            total_reserved,
        }
    }

    /// Create a memory map pre-loaded with realistic stub data.
    pub fn with_demo_data(pid: u32) -> Self {
        let regions = vec![
            MemoryRegion {
                start_addr: 0x0040_0000,
                end_addr: 0x0048_0000,
                region_type: RegionType::Code,
                protection: Protection { read: true, write: false, execute: true },
                backing: "/usr/bin/editor".to_string(),
                committed: 512 * 1024,
                reserved: 512 * 1024,
            },
            MemoryRegion {
                start_addr: 0x0060_0000,
                end_addr: 0x0062_0000,
                region_type: RegionType::Data,
                protection: Protection { read: true, write: true, execute: false },
                backing: "/usr/bin/editor".to_string(),
                committed: 128 * 1024,
                reserved: 128 * 1024,
            },
            MemoryRegion {
                start_addr: 0x00A0_0000,
                end_addr: 0x01A0_0000,
                region_type: RegionType::Heap,
                protection: Protection { read: true, write: true, execute: false },
                backing: "[heap]".to_string(),
                committed: 8 * 1024 * 1024,
                reserved: 16 * 1024 * 1024,
            },
            MemoryRegion {
                start_addr: 0x7F00_0000_0000,
                end_addr: 0x7F00_0010_0000,
                region_type: RegionType::MappedFile,
                protection: Protection { read: true, write: false, execute: false },
                backing: "/usr/lib/libguitk.so".to_string(),
                committed: 1024 * 1024,
                reserved: 1024 * 1024,
            },
            MemoryRegion {
                start_addr: 0x7F00_0100_0000,
                end_addr: 0x7F00_0108_0000,
                region_type: RegionType::Code,
                protection: Protection { read: true, write: false, execute: true },
                backing: "/usr/lib/libguitk.so".to_string(),
                committed: 512 * 1024,
                reserved: 512 * 1024,
            },
            MemoryRegion {
                start_addr: 0x7F00_1000_0000,
                end_addr: 0x7F00_1004_0000,
                region_type: RegionType::Shared,
                protection: Protection { read: true, write: true, execute: false },
                backing: "shm:compositor-buffer".to_string(),
                committed: 256 * 1024,
                reserved: 256 * 1024,
            },
            MemoryRegion {
                start_addr: 0x7FFE_0000_0000,
                end_addr: 0x7FFE_0020_0000,
                region_type: RegionType::Stack,
                protection: Protection { read: true, write: true, execute: false },
                backing: "[stack]".to_string(),
                committed: 64 * 1024,
                reserved: 2 * 1024 * 1024,
            },
        ];
        Self::new(pid, regions)
    }

    /// Render the memory map viewer.
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let mut cy = y;

        // Header.
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: FEATURE_ROW_HEIGHT,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + FEATURE_TEXT_PAD,
            y: cy + 4.0,
            text: format!("Memory Map  PID {}", self.pid),
            color: MOCHA_BLUE,
            font_size: FEATURE_HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - FEATURE_TEXT_PAD * 2.0),
        });
        cy += FEATURE_ROW_HEIGHT;

        // Summary bar: committed vs reserved.
        cmds.push(RenderCommand::FillRect {
            x: x + FEATURE_TEXT_PAD,
            y: cy,
            width: width - FEATURE_TEXT_PAD * 2.0,
            height: FEATURE_ROW_HEIGHT,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + FEATURE_TEXT_PAD + 8.0,
            y: cy + 4.0,
            text: format!(
                "Committed: {}   Reserved: {}",
                format_size(self.total_committed),
                format_size(self.total_reserved),
            ),
            color: MOCHA_TEXT,
            font_size: FEATURE_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - FEATURE_TEXT_PAD * 2.0 - 16.0),
        });
        cy += FEATURE_ROW_HEIGHT + 4.0;

        // Color-coded bar showing relative region sizes.
        let bar_height = 16.0f32;
        let bar_width = width - FEATURE_TEXT_PAD * 2.0;
        cmds.push(RenderCommand::FillRect {
            x: x + FEATURE_TEXT_PAD,
            y: cy,
            width: bar_width,
            height: bar_height,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });

        if self.total_reserved > 0 {
            let mut bar_x = x + FEATURE_TEXT_PAD;
            for region in &self.regions {
                let frac = region.reserved as f64 / self.total_reserved as f64;
                let seg_w = (frac * bar_width as f64) as f32;
                if seg_w > 0.5 {
                    cmds.push(RenderCommand::FillRect {
                        x: bar_x,
                        y: cy,
                        width: seg_w,
                        height: bar_height,
                        color: region.region_type.color(),
                        corner_radii: CornerRadii::ZERO,
                    });
                }
                bar_x += seg_w;
            }
        }
        cy += bar_height + 4.0;

        // Legend.
        let legend_types = [
            RegionType::Code,
            RegionType::Data,
            RegionType::Stack,
            RegionType::Heap,
            RegionType::MappedFile,
            RegionType::Shared,
        ];
        let legend_item_w = 90.0f32;
        for (i, rt) in legend_types.iter().enumerate() {
            let lx = x + FEATURE_TEXT_PAD + (i as f32 * legend_item_w);
            cmds.push(RenderCommand::FillRect {
                x: lx,
                y: cy + 2.0,
                width: 10.0,
                height: 10.0,
                color: rt.color(),
                corner_radii: CornerRadii::all(2.0),
            });
            cmds.push(RenderCommand::Text {
                x: lx + 14.0,
                y: cy + 1.0,
                text: rt.label().to_string(),
                color: MOCHA_SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(legend_item_w - 18.0),
            });
        }
        cy += FEATURE_ROW_HEIGHT;

        // Column headers.
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: FEATURE_ROW_HEIGHT,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        let col_headers = ["Address Range", "Size", "Prot", "Type", "Backing"];
        let col_widths = [
            width * 0.30,
            width * 0.12,
            width * 0.08,
            width * 0.12,
            width * 0.38,
        ];
        let mut hx = x + FEATURE_TEXT_PAD;
        for (header, &cw) in col_headers.iter().zip(col_widths.iter()) {
            cmds.push(RenderCommand::Text {
                x: hx,
                y: cy + 4.0,
                text: header.to_string(),
                color: MOCHA_SUBTEXT1,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(cw - 4.0),
            });
            hx += cw;
        }
        cy += FEATURE_ROW_HEIGHT;

        // Region rows.
        for (i, region) in self.regions.iter().enumerate() {
            let bg = if i % 2 == 0 { MOCHA_BASE } else { MOCHA_MANTLE };
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: FEATURE_ROW_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });

            // Color indicator bar on the left.
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width: 3.0,
                height: FEATURE_ROW_HEIGHT,
                color: region.region_type.color(),
                corner_radii: CornerRadii::ZERO,
            });

            let mut rx = x + FEATURE_TEXT_PAD;

            // Address range.
            cmds.push(RenderCommand::Text {
                x: rx,
                y: cy + 4.0,
                text: format!("{:#014X}-{:#014X}", region.start_addr, region.end_addr),
                color: MOCHA_TEXT,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[0] - 4.0),
            });
            rx += col_widths[0];

            // Size.
            cmds.push(RenderCommand::Text {
                x: rx,
                y: cy + 4.0,
                text: format_size(region.size()),
                color: MOCHA_TEXT,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[1] - 4.0),
            });
            rx += col_widths[1];

            // Protection.
            cmds.push(RenderCommand::Text {
                x: rx,
                y: cy + 4.0,
                text: region.protection.to_rwx(),
                color: MOCHA_SUBTEXT0,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[2] - 4.0),
            });
            rx += col_widths[2];

            // Type.
            cmds.push(RenderCommand::Text {
                x: rx,
                y: cy + 4.0,
                text: region.region_type.label().to_string(),
                color: region.region_type.color(),
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[3] - 4.0),
            });
            rx += col_widths[3];

            // Backing.
            cmds.push(RenderCommand::Text {
                x: rx,
                y: cy + 4.0,
                text: region.backing.clone(),
                color: MOCHA_SUBTEXT0,
                font_size: FEATURE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_widths[4] - 4.0),
            });

            cy += FEATURE_ROW_HEIGHT;
        }

        cmds
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Format a byte size for display (KiB / MiB / GiB).
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Blocking chain tests ------------------------------------------------

    #[test]
    fn simple_blocking_chain() {
        let mut analyzer = BlockingAnalyzer::new();
        // A waits on Mutex #1, held by B.  B waits on I/O.
        analyzer.register_process(1, "A", WaitReason::Mutex(1));
        analyzer.register_holder("Mutex #1", 2, "B");
        analyzer.register_process(2, "B", WaitReason::IO("/dev/sda".to_string()));

        let info = analyzer.analyze_blocking(1);
        assert!(!info.deadlock_detected);
        assert_eq!(info.chain.len(), 2);
        assert_eq!(info.chain[0].waiter_pid, 1);
        assert_eq!(info.chain[0].holder_pid, Some(2));
        assert_eq!(info.chain[1].waiter_pid, 2);
        assert_eq!(info.chain[1].holder_pid, None); // I/O has no holder
    }

    #[test]
    fn chain_terminates_at_non_blocked_holder() {
        let mut analyzer = BlockingAnalyzer::new();
        analyzer.register_process(10, "waiter", WaitReason::Semaphore(5));
        analyzer.register_holder("Semaphore #5", 20, "holder");
        analyzer.register_process(20, "holder", WaitReason::None);

        let info = analyzer.analyze_blocking(10);
        assert!(!info.deadlock_detected);
        // Chain should only have one link: the waiter. The holder is not
        // blocked so the chain stops.
        assert_eq!(info.chain.len(), 1);
        assert_eq!(info.chain[0].waiter_pid, 10);
        assert_eq!(info.chain[0].holder_pid, Some(20));
    }

    #[test]
    fn not_blocked_produces_empty_chain() {
        let mut analyzer = BlockingAnalyzer::new();
        analyzer.register_process(99, "idle", WaitReason::None);

        let info = analyzer.analyze_blocking(99);
        assert!(!info.deadlock_detected);
        assert!(info.chain.is_empty());
        assert_eq!(info.direct_reason, WaitReason::None);
    }

    // -- Deadlock detection tests --------------------------------------------

    #[test]
    fn two_process_deadlock() {
        let mut analyzer = BlockingAnalyzer::new();
        // A holds Mutex #1, waits on Mutex #2.
        // B holds Mutex #2, waits on Mutex #1.
        analyzer.register_process(1, "A", WaitReason::Mutex(2));
        analyzer.register_holder("Mutex #2", 2, "B");
        analyzer.register_process(2, "B", WaitReason::Mutex(1));
        analyzer.register_holder("Mutex #1", 1, "A");

        let info = analyzer.analyze_blocking(1);
        assert!(info.deadlock_detected);
        // Cycle should include both PIDs.
        assert!(info.deadlock_cycle.contains(&1));
        assert!(info.deadlock_cycle.contains(&2));
    }

    #[test]
    fn three_process_deadlock() {
        let mut analyzer = BlockingAnalyzer::new();
        // A -> Mutex #1 (held by B) -> Mutex #2 (held by C) -> Mutex #3 (held by A)
        analyzer.register_process(1, "A", WaitReason::Mutex(1));
        analyzer.register_holder("Mutex #1", 2, "B");
        analyzer.register_process(2, "B", WaitReason::Mutex(2));
        analyzer.register_holder("Mutex #2", 3, "C");
        analyzer.register_process(3, "C", WaitReason::Mutex(3));
        analyzer.register_holder("Mutex #3", 1, "A");

        let info = analyzer.analyze_blocking(1);
        assert!(info.deadlock_detected);
        assert!(info.deadlock_cycle.contains(&1));
        assert!(info.deadlock_cycle.contains(&2));
        assert!(info.deadlock_cycle.contains(&3));
    }

    #[test]
    fn detect_all_deadlocks_finds_cycles() {
        let analyzer = BlockingAnalyzer::with_demo_data();
        let cycles = analyzer.detect_all_deadlocks();
        // Demo data has one deadlock: httpd-worker <-> netd.
        assert!(!cycles.is_empty());
        let cycle = &cycles[0];
        assert!(cycle.contains(&301)); // httpd-worker
        assert!(cycle.contains(&101)); // netd
    }

    #[test]
    fn no_deadlock_in_linear_chain() {
        let mut analyzer = BlockingAnalyzer::new();
        analyzer.register_process(1, "A", WaitReason::Mutex(10));
        analyzer.register_holder("Mutex #10", 2, "B");
        analyzer.register_process(2, "B", WaitReason::Mutex(20));
        analyzer.register_holder("Mutex #20", 3, "C");
        analyzer.register_process(3, "C", WaitReason::Sleep(1000));

        let info = analyzer.analyze_blocking(1);
        assert!(!info.deadlock_detected);
        assert_eq!(info.chain.len(), 3);
    }

    // -- Affinity mask tests -------------------------------------------------

    #[test]
    fn all_mask_has_all_bits() {
        let mask = AffinityMask::all(8);
        assert_eq!(mask.enabled_count(), 8);
        for i in 0..8 {
            assert!(mask.is_cpu_enabled(i));
        }
        assert!(!mask.is_cpu_enabled(8));
    }

    #[test]
    fn single_mask() {
        let mask = AffinityMask::single(3, 8);
        assert_eq!(mask.enabled_count(), 1);
        assert!(mask.is_cpu_enabled(3));
        assert!(!mask.is_cpu_enabled(0));
        assert!(!mask.is_cpu_enabled(7));
    }

    #[test]
    fn even_cores() {
        let mask = AffinityMask::even_cores(8);
        assert!(mask.is_cpu_enabled(0));
        assert!(!mask.is_cpu_enabled(1));
        assert!(mask.is_cpu_enabled(2));
        assert!(!mask.is_cpu_enabled(3));
        assert!(mask.is_cpu_enabled(4));
        assert_eq!(mask.enabled_count(), 4);
    }

    #[test]
    fn odd_cores() {
        let mask = AffinityMask::odd_cores(8);
        assert!(!mask.is_cpu_enabled(0));
        assert!(mask.is_cpu_enabled(1));
        assert!(!mask.is_cpu_enabled(2));
        assert!(mask.is_cpu_enabled(3));
        assert_eq!(mask.enabled_count(), 4);
    }

    #[test]
    fn toggle_cpu() {
        let mut mask = AffinityMask::all(4);
        assert!(mask.toggle_cpu(2)); // disable CPU 2
        assert!(!mask.is_cpu_enabled(2));
        assert_eq!(mask.enabled_count(), 3);

        assert!(mask.toggle_cpu(2)); // re-enable CPU 2
        assert!(mask.is_cpu_enabled(2));
        assert_eq!(mask.enabled_count(), 4);
    }

    #[test]
    fn toggle_refuses_empty_mask() {
        let mut mask = AffinityMask::single(0, 4);
        // Trying to disable the only enabled CPU should fail.
        assert!(!mask.toggle_cpu(0));
        assert!(mask.is_cpu_enabled(0));
    }

    #[test]
    fn from_bits_masks_to_cpu_count() {
        let mask = AffinityMask::from_bits(0xFF, 4);
        // Only 4 CPUs, so upper bits are masked off.
        assert_eq!(mask.bits(), 0x0F);
        assert_eq!(mask.enabled_count(), 4);
    }

    #[test]
    fn out_of_range_cpu_not_enabled() {
        let mask = AffinityMask::all(4);
        assert!(!mask.is_cpu_enabled(4));
        assert!(!mask.is_cpu_enabled(100));
    }

    // -- Environment search tests --------------------------------------------

    #[test]
    fn filter_matches_name() {
        let viewer = EnvViewer::with_demo_data(100);
        let mut viewer = viewer;
        viewer.set_filter("path");
        let filtered = viewer.filtered_entries();
        // Should match PATH and LD_LIBRARY_PATH at minimum.
        assert!(filtered.len() >= 2);
        assert!(filtered.iter().any(|e| e.name == "PATH"));
        assert!(filtered.iter().any(|e| e.name == "LD_LIBRARY_PATH"));
    }

    #[test]
    fn filter_matches_value() {
        let mut viewer = EnvViewer::with_demo_data(100);
        viewer.set_filter("truecolor");
        let filtered = viewer.filtered_entries();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "COLORTERM");
    }

    #[test]
    fn filter_is_case_insensitive() {
        let mut viewer = EnvViewer::with_demo_data(100);
        viewer.set_filter("DISPLAY");
        let upper_count = viewer.filtered_entries().len();
        assert!(upper_count > 0);

        viewer.set_filter("display");
        let lower_count = viewer.filtered_entries().len();
        assert_eq!(upper_count, lower_count);
    }

    #[test]
    fn empty_filter_returns_all() {
        let viewer = EnvViewer::with_demo_data(100);
        let filtered = viewer.filtered_entries();
        assert_eq!(filtered.len(), 20); // demo data has 20 entries
    }

    #[test]
    fn sort_by_name_ascending() {
        let viewer = EnvViewer::with_demo_data(100);
        // Default sort is ascending by name.
        let filtered = viewer.filtered_entries();
        assert!(filtered.len() >= 2);
        assert!(filtered[0].name.to_lowercase() <= filtered[1].name.to_lowercase());
    }

    #[test]
    fn sort_toggles_direction() {
        let mut viewer = EnvViewer::with_demo_data(100);
        // Default is ascending by name.
        assert!(viewer.sort_ascending);
        // Setting the same column again toggles direction.
        viewer.set_sort(EnvSortColumn::Name);
        assert!(!viewer.sort_ascending);
        // And again toggles back.
        viewer.set_sort(EnvSortColumn::Name);
        assert!(viewer.sort_ascending);
    }

    #[test]
    fn selected_value_returns_correct_entry() {
        let mut viewer = EnvViewer::with_demo_data(100);
        viewer.selected_index = Some(0);
        let val = viewer.selected_value();
        assert!(val.is_some());
    }

    // -- Priority tests ------------------------------------------------------

    #[test]
    fn priority_warning_for_realtime() {
        let mut sel = PrioritySelector::new(100, PriorityLevel::Normal);
        let action = sel.select(PriorityLevel::Realtime);
        // Should NOT emit action yet (shows warning).
        assert!(action.is_none());
        assert!(sel.warning_visible);

        // Confirm the warning.
        let action = sel.confirm_warning();
        assert_eq!(
            action,
            Some(ProcessAction::SetPriority(100, PriorityLevel::Realtime))
        );
        assert!(!sel.warning_visible);
        assert_eq!(sel.current, PriorityLevel::Realtime);
    }

    #[test]
    fn priority_no_warning_for_normal() {
        let mut sel = PrioritySelector::new(100, PriorityLevel::Normal);
        let action = sel.select(PriorityLevel::High);
        assert_eq!(
            action,
            Some(ProcessAction::SetPriority(100, PriorityLevel::High))
        );
        assert!(!sel.warning_visible);
    }

    #[test]
    fn dismiss_warning_cancels_change() {
        let mut sel = PrioritySelector::new(100, PriorityLevel::Normal);
        let _ = sel.select(PriorityLevel::Realtime);
        sel.dismiss_warning();
        assert!(!sel.warning_visible);
        assert_eq!(sel.current, PriorityLevel::Normal); // unchanged
    }

    // -- Window picker tests -------------------------------------------------

    #[test]
    fn window_picker_lifecycle() {
        let mut picker = WindowPicker::new();
        assert!(!picker.active);

        picker.activate();
        assert!(picker.active);
        assert!(picker.result.is_none());

        picker.hover(0x1234);
        assert_eq!(picker.hovered_window, Some(0x1234));

        picker.pick(WindowPicker::mock_pick());
        assert!(!picker.active);
        assert!(picker.result.is_some());
        assert_eq!(picker.result.as_ref().map(|r| r.pid), Some(203));
    }

    #[test]
    fn window_picker_cancel() {
        let mut picker = WindowPicker::new();
        picker.activate();
        picker.hover(0xABCD);
        picker.cancel();
        assert!(!picker.active);
        assert!(picker.hovered_window.is_none());
        assert!(picker.result.is_none());
    }

    // -- Memory map tests ----------------------------------------------------

    #[test]
    fn memory_map_regions_sorted() {
        let map = MemoryMap::with_demo_data(100);
        for i in 1..map.regions.len() {
            assert!(map.regions[i].start_addr >= map.regions[i - 1].start_addr);
        }
    }

    #[test]
    fn memory_map_totals() {
        let map = MemoryMap::with_demo_data(100);
        let expected_committed: u64 = map.regions.iter().map(|r| r.committed).sum();
        let expected_reserved: u64 = map.regions.iter().map(|r| r.reserved).sum();
        assert_eq!(map.total_committed, expected_committed);
        assert_eq!(map.total_reserved, expected_reserved);
    }

    #[test]
    fn region_size_calculation() {
        let r = MemoryRegion {
            start_addr: 0x1000,
            end_addr: 0x2000,
            region_type: RegionType::Code,
            protection: Protection { read: true, write: false, execute: true },
            backing: "test".to_string(),
            committed: 4096,
            reserved: 4096,
        };
        assert_eq!(r.size(), 0x1000);
    }

    #[test]
    fn protection_to_rwx() {
        let p = Protection { read: true, write: false, execute: true };
        assert_eq!(p.to_rwx(), "r-x");

        let p = Protection { read: true, write: true, execute: false };
        assert_eq!(p.to_rwx(), "rw-");

        let p = Protection { read: false, write: false, execute: false };
        assert_eq!(p.to_rwx(), "---");
    }

    // -- Rendering smoke tests -----------------------------------------------

    #[test]
    fn window_picker_render_active() {
        let mut picker = WindowPicker::new();
        picker.activate();
        let cmds = picker.render(0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn blocking_render_produces_output() {
        let analyzer = BlockingAnalyzer::with_demo_data();
        let cmds = analyzer.render(203, 0.0, 0.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn affinity_render_produces_output() {
        let mask = AffinityMask::all(8);
        let cmds = mask.render(100, 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn priority_render_produces_output() {
        let sel = PrioritySelector::new(100, PriorityLevel::Normal);
        let cmds = sel.render(0.0, 0.0, 300.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn env_viewer_render_produces_output() {
        let viewer = EnvViewer::with_demo_data(100);
        let cmds = viewer.render(0.0, 0.0, 500.0, 10);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn memory_map_render_produces_output() {
        let map = MemoryMap::with_demo_data(100);
        let cmds = map.render(0.0, 0.0, 800.0);
        assert!(!cmds.is_empty());
    }
}

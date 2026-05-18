//! OurOS Process Explorer
//!
//! Graphical system monitor and task manager with:
//! - Process list with sortable columns and tree view
//! - System overview (CPU, memory, load)
//! - Per-process details panel (threads, handles, env)
//! - Network connections and bandwidth
//! - Toolbar with actions and search
//!
//! Uses the guitk library for UI rendering. All data is gathered
//! through OurOS syscalls; the structs here define the presentation
//! layer while the OS provides the actual process/system information.

#[allow(dead_code)]
mod features;

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};

use std::collections::HashMap;

// ============================================================================
// Constants — layout and colors
// ============================================================================

/// Height of the toolbar at the top of the window.
const TOOLBAR_HEIGHT: f32 = 40.0;
/// Height of the tab bar below the toolbar.
const TAB_BAR_HEIGHT: f32 = 28.0;
/// Height of the status bar at the bottom.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Height of a single row in process/connection tables.
const ROW_HEIGHT: f32 = 22.0;
/// Height of column headers in tables.
const HEADER_HEIGHT: f32 = 24.0;
/// Number of historical samples kept for time-series graphs.
const GRAPH_HISTORY_LEN: usize = 60;
/// Default auto-refresh interval in milliseconds.
#[allow(dead_code)]
const DEFAULT_REFRESH_MS: u64 = 2000;

// -- Color palette ----------------------------------------------------------

/// Dark header background.
const COLOR_TOOLBAR_BG: Color = Color::rgb(40, 44, 52);
/// Tab bar background.
const COLOR_TAB_BG: Color = Color::rgb(50, 54, 62);
/// Active tab highlight.
const COLOR_TAB_ACTIVE: Color = Color::rgb(70, 130, 210);
/// Main content background.
const COLOR_CONTENT_BG: Color = Color::rgb(30, 33, 39);
/// Table header row background.
const COLOR_HEADER_BG: Color = Color::rgb(38, 42, 50);
/// Even row background.
const COLOR_ROW_EVEN: Color = Color::rgb(30, 33, 39);
/// Odd row background.
const COLOR_ROW_ODD: Color = Color::rgb(35, 38, 46);
/// Selected row highlight.
const COLOR_ROW_SELECTED: Color = Color::rgb(50, 80, 130);
/// Hovered row highlight.
const COLOR_ROW_HOVER: Color = Color::rgb(45, 50, 60);
/// Status bar background.
const COLOR_STATUS_BG: Color = Color::rgb(35, 38, 46);

/// Primary text color.
const COLOR_TEXT: Color = Color::rgb(210, 215, 225);
/// Dimmed/secondary text color.
const COLOR_TEXT_DIM: Color = Color::rgb(140, 145, 155);
/// Accent color (buttons, links).
const COLOR_ACCENT: Color = Color::rgb(80, 140, 220);
/// Error/danger color.
const COLOR_DANGER: Color = Color::rgb(220, 60, 60);

/// Status: running.
const COLOR_STATUS_RUNNING: Color = Color::rgb(80, 200, 80);
/// Status: sleeping.
const COLOR_STATUS_SLEEPING: Color = Color::rgb(80, 140, 220);
/// Status: stopped.
const COLOR_STATUS_STOPPED: Color = Color::rgb(220, 180, 40);
/// Status: zombie.
const COLOR_STATUS_ZOMBIE: Color = Color::rgb(220, 60, 60);

/// Graph line color for CPU.
const COLOR_GRAPH_CPU: Color = Color::rgb(80, 200, 120);
/// Graph line color for network in.
const COLOR_GRAPH_NET_IN: Color = Color::rgb(80, 160, 240);
/// Graph line color for network out.
const COLOR_GRAPH_NET_OUT: Color = Color::rgb(240, 140, 60);
/// Graph grid line color.
const COLOR_GRAPH_GRID: Color = Color::rgb(55, 60, 70);

// ============================================================================
// Process status
// ============================================================================

/// Process execution state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
    Sleeping,
    Stopped,
    Zombie,
    Idle,
}

impl ProcessStatus {
    /// Short label for table display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Sleeping => "Sleeping",
            Self::Stopped => "Stopped",
            Self::Zombie => "Zombie",
            Self::Idle => "Idle",
        }
    }

    /// Color associated with this status.
    pub fn color(self) -> Color {
        match self {
            Self::Running => COLOR_STATUS_RUNNING,
            Self::Sleeping => COLOR_STATUS_SLEEPING,
            Self::Stopped => COLOR_STATUS_STOPPED,
            Self::Zombie => COLOR_STATUS_ZOMBIE,
            Self::Idle => COLOR_TEXT_DIM,
        }
    }
}

// ============================================================================
// Process info
// ============================================================================

/// Information about a single process.
#[derive(Clone, Debug)]
pub struct ProcessInfo {
    /// Process ID.
    pub pid: u32,
    /// Parent process ID.
    pub ppid: u32,
    /// Process name (executable basename).
    pub name: String,
    /// Current status.
    pub status: ProcessStatus,
    /// CPU usage percentage (0.0 - 100.0).
    pub cpu_percent: f32,
    /// Resident memory in bytes.
    pub memory_bytes: u64,
    /// Virtual memory in bytes.
    pub virtual_bytes: u64,
    /// Shared memory in bytes.
    pub shared_bytes: u64,
    /// Number of threads.
    pub thread_count: u32,
    /// Priority value (lower = higher priority).
    pub priority: i32,
    /// User or owner name.
    pub user: String,
    /// Full command line.
    pub command_line: String,
    /// Start time as seconds since boot.
    pub start_time_secs: u64,
    /// Total CPU time consumed in milliseconds.
    pub cpu_time_ms: u64,
    /// Per-thread information.
    pub threads: Vec<ThreadInfo>,
    /// Open handles / capabilities.
    pub handles: Vec<HandleInfo>,
    /// Environment variables.
    pub environment: Vec<(String, String)>,
    /// Depth in the tree view (0 = root).
    pub tree_depth: u32,
}

/// Information about a single thread within a process.
#[derive(Clone, Debug)]
pub struct ThreadInfo {
    /// Thread ID.
    pub tid: u32,
    /// Thread name (if set).
    pub name: String,
    /// Current state.
    pub status: ProcessStatus,
    /// CPU usage percentage.
    pub cpu_percent: f32,
}

/// An open handle or capability held by a process.
#[derive(Clone, Debug)]
pub struct HandleInfo {
    /// Handle number.
    pub handle_id: u32,
    /// Type of resource.
    pub resource_type: String,
    /// Description / path / name.
    pub description: String,
}

// ============================================================================
// Network connection
// ============================================================================

/// An active network connection.
#[derive(Clone, Debug)]
pub struct ConnectionInfo {
    /// Protocol (TCP, UDP).
    pub protocol: String,
    /// Local address and port.
    pub local_addr: String,
    /// Remote address and port.
    pub remote_addr: String,
    /// Connection state (ESTABLISHED, LISTEN, etc.).
    pub state: String,
    /// Owning process ID.
    pub pid: u32,
    /// Owning process name.
    pub process_name: String,
}

// ============================================================================
// System information
// ============================================================================

/// Snapshot of overall system resource usage.
#[derive(Clone, Debug)]
pub struct SystemInfo {
    /// Total physical memory in bytes.
    pub total_memory: u64,
    /// Used memory in bytes.
    pub used_memory: u64,
    /// Free memory in bytes.
    pub free_memory: u64,
    /// Cached/buffered memory in bytes.
    pub cached_memory: u64,
    /// Total swap in bytes.
    pub swap_total: u64,
    /// Used swap in bytes.
    pub swap_used: u64,
    /// Per-CPU core utilization (0.0 - 100.0).
    pub cpu_per_core: Vec<f32>,
    /// Overall CPU utilization (0.0 - 100.0).
    pub cpu_overall: f32,
    /// System uptime in seconds.
    pub uptime_secs: u64,
    /// Load averages (1, 5, 15 minute).
    pub load_avg: [f32; 3],
    /// Total number of processes.
    pub process_count: u32,
    /// Number of running processes.
    pub running_count: u32,
}

// ============================================================================
// Graph history — ring buffer of f32 samples
// ============================================================================

/// Ring buffer holding the last `GRAPH_HISTORY_LEN` samples for a
/// time-series value (CPU %, bandwidth, etc.).
#[derive(Clone, Debug)]
pub struct GraphHistory {
    /// Fixed-size sample buffer.
    samples: Vec<f32>,
    /// Write cursor (next position to overwrite).
    cursor: usize,
    /// Number of samples written so far (clamped to capacity).
    count: usize,
}

impl GraphHistory {
    /// Create a new history buffer pre-filled with zeroes.
    pub fn new() -> Self {
        Self {
            samples: vec![0.0; GRAPH_HISTORY_LEN],
            cursor: 0,
            count: 0,
        }
    }

    /// Push a new sample, overwriting the oldest if full.
    pub fn push(&mut self, value: f32) {
        if let Some(slot) = self.samples.get_mut(self.cursor) {
            *slot = value;
        }
        self.cursor = (self.cursor + 1) % GRAPH_HISTORY_LEN;
        if self.count < GRAPH_HISTORY_LEN {
            self.count += 1;
        }
    }

    /// Iterate over samples from oldest to newest.
    pub fn iter_oldest_first(&self) -> impl Iterator<Item = f32> + '_ {
        let start = if self.count < GRAPH_HISTORY_LEN {
            0
        } else {
            self.cursor
        };
        let len = self.count;
        (0..len).map(move |i| {
            let idx = (start + i) % GRAPH_HISTORY_LEN;
            self.samples.get(idx).copied().unwrap_or(0.0)
        })
    }

    /// Number of recorded samples (up to `GRAPH_HISTORY_LEN`).
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl Default for GraphHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tabs, columns, sort, context menu, view mode
// ============================================================================

/// Application tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Processes,
    System,
    Network,
    Details,
}

impl Tab {
    /// Display label for the tab.
    pub fn label(self) -> &'static str {
        match self {
            Self::Processes => "Processes",
            Self::System => "System",
            Self::Network => "Network",
            Self::Details => "Details",
        }
    }

    /// All tabs in display order.
    pub const ALL: [Tab; 4] = [Tab::Processes, Tab::System, Tab::Network, Tab::Details];
}

/// Columns in the process table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessColumn {
    Pid,
    Name,
    Status,
    Cpu,
    Memory,
    Threads,
    Priority,
    User,
}

impl ProcessColumn {
    /// Header text for the column.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pid => "PID",
            Self::Name => "Name",
            Self::Status => "Status",
            Self::Cpu => "CPU%",
            Self::Memory => "Memory",
            Self::Threads => "Threads",
            Self::Priority => "Priority",
            Self::User => "User",
        }
    }

    /// Column width in pixels.
    pub fn width(self) -> f32 {
        match self {
            Self::Pid => 60.0,
            Self::Name => 180.0,
            Self::Status => 80.0,
            Self::Cpu => 65.0,
            Self::Memory => 85.0,
            Self::Threads => 65.0,
            Self::Priority => 65.0,
            Self::User => 90.0,
        }
    }

    /// All columns in display order.
    pub const ALL: [ProcessColumn; 8] = [
        ProcessColumn::Pid,
        ProcessColumn::Name,
        ProcessColumn::Status,
        ProcessColumn::Cpu,
        ProcessColumn::Memory,
        ProcessColumn::Threads,
        ProcessColumn::Priority,
        ProcessColumn::User,
    ];
}

/// Sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// View mode for the process list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Flat list of processes.
    List,
    /// Tree view showing parent-child relationships.
    Tree,
}

/// Context menu action for right-click on a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextAction {
    Kill,
    Pause,
    Resume,
    ChangePriority,
    OpenFileLocation,
}

impl ContextAction {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Kill => "Kill Process",
            Self::Pause => "Pause",
            Self::Resume => "Resume",
            Self::ChangePriority => "Change Priority...",
            Self::OpenFileLocation => "Open File Location",
        }
    }

    /// All menu items in order.
    pub const ALL: [ContextAction; 5] = [
        ContextAction::Kill,
        ContextAction::Pause,
        ContextAction::Resume,
        ContextAction::ChangePriority,
        ContextAction::OpenFileLocation,
    ];
}

/// Auto-refresh interval options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshInterval {
    OneSecond,
    TwoSeconds,
    FiveSeconds,
}

impl RefreshInterval {
    /// Interval in milliseconds.
    pub fn ms(self) -> u64 {
        match self {
            Self::OneSecond => 1000,
            Self::TwoSeconds => 2000,
            Self::FiveSeconds => 5000,
        }
    }

    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::OneSecond => "1s",
            Self::TwoSeconds => "2s",
            Self::FiveSeconds => "5s",
        }
    }

    /// Cycle to the next interval.
    pub fn next(self) -> Self {
        match self {
            Self::OneSecond => Self::TwoSeconds,
            Self::TwoSeconds => Self::FiveSeconds,
            Self::FiveSeconds => Self::OneSecond,
        }
    }
}

// ============================================================================
// Context menu state
// ============================================================================

/// State for the right-click context menu overlay.
#[derive(Clone, Debug)]
pub struct ContextMenu {
    /// Screen position of the menu.
    pub x: f32,
    pub y: f32,
    /// PID of the target process.
    pub target_pid: u32,
    /// Currently highlighted item index (if any).
    pub hover_index: Option<usize>,
}

// ============================================================================
// Application state
// ============================================================================

/// Top-level state for the process explorer application.
pub struct ProcessExplorerState {
    // -- Window --------------------------------------------------------------
    /// Window width in pixels.
    pub window_width: u32,
    /// Window height in pixels.
    pub window_height: u32,

    // -- Navigation ----------------------------------------------------------
    /// Currently active tab.
    pub active_tab: Tab,

    // -- Process list --------------------------------------------------------
    /// All known processes.
    pub processes: Vec<ProcessInfo>,
    /// Visible (filtered/sorted) process indices into `processes`.
    pub visible_indices: Vec<usize>,
    /// Currently selected process index (in `visible_indices`).
    pub selected_index: Option<usize>,
    /// Hovered row index (in `visible_indices`).
    pub hovered_index: Option<usize>,
    /// Sort column.
    pub sort_column: ProcessColumn,
    /// Sort direction.
    pub sort_direction: SortDirection,
    /// View mode (list or tree).
    pub view_mode: ViewMode,
    /// Scroll offset (number of rows scrolled).
    pub scroll_offset: usize,

    // -- Search / filter -----------------------------------------------------
    /// Filter text (search box content).
    pub filter_text: String,
    /// Whether the search box is focused.
    pub filter_focused: bool,

    // -- Context menu --------------------------------------------------------
    /// Active context menu, if any.
    pub context_menu: Option<ContextMenu>,

    // -- System overview -----------------------------------------------------
    /// Latest system info snapshot.
    pub system_info: SystemInfo,
    /// CPU history graph.
    pub cpu_history: GraphHistory,
    /// Per-core history graphs.
    pub core_histories: Vec<GraphHistory>,

    // -- Network -------------------------------------------------------------
    /// Active network connections.
    pub connections: Vec<ConnectionInfo>,
    /// Inbound bandwidth history (bytes/sec).
    pub net_in_history: GraphHistory,
    /// Outbound bandwidth history (bytes/sec).
    pub net_out_history: GraphHistory,

    // -- Refresh -------------------------------------------------------------
    /// Auto-refresh interval.
    pub refresh_interval: RefreshInterval,
    /// Milliseconds elapsed since last refresh.
    pub ms_since_refresh: u64,

    // -- Status bar ----------------------------------------------------------
    /// Status bar message.
    pub status_message: String,
}

impl ProcessExplorerState {
    /// Create a new process explorer with default state.
    pub fn new() -> Self {
        let system_info = SystemInfo {
            total_memory: 0,
            used_memory: 0,
            free_memory: 0,
            cached_memory: 0,
            swap_total: 0,
            swap_used: 0,
            cpu_per_core: Vec::new(),
            cpu_overall: 0.0,
            uptime_secs: 0,
            load_avg: [0.0; 3],
            process_count: 0,
            running_count: 0,
        };

        Self {
            window_width: 960,
            window_height: 680,
            active_tab: Tab::Processes,
            processes: Vec::new(),
            visible_indices: Vec::new(),
            selected_index: None,
            hovered_index: None,
            sort_column: ProcessColumn::Cpu,
            sort_direction: SortDirection::Descending,
            view_mode: ViewMode::List,
            scroll_offset: 0,
            filter_text: String::new(),
            filter_focused: false,
            context_menu: None,
            system_info,
            cpu_history: GraphHistory::new(),
            core_histories: Vec::new(),
            connections: Vec::new(),
            net_in_history: GraphHistory::new(),
            net_out_history: GraphHistory::new(),
            refresh_interval: RefreshInterval::TwoSeconds,
            ms_since_refresh: 0,
            status_message: String::new(),
        }
    }

    // ========================================================================
    // Data refresh
    // ========================================================================

    /// Refresh all data from the OS.
    ///
    /// In a real implementation this calls OurOS syscalls to enumerate
    /// processes, read system stats, and list network connections. Here
    /// we define the API shape; the actual syscalls are provided by the
    /// kernel's process and network subsystems.
    pub fn refresh(&mut self) {
        // Placeholder: in production, call kernel syscalls here:
        //   - sys_process_list() -> Vec<ProcessInfo>
        //   - sys_system_info() -> SystemInfo
        //   - sys_net_connections() -> Vec<ConnectionInfo>
        //
        // For now, the data vectors are populated externally or via
        // `load_demo_data()` for development/testing.

        self.rebuild_visible_list();
        self.update_histories();
        self.update_status();
    }

    /// Rebuild the filtered and sorted visible index list.
    pub fn rebuild_visible_list(&mut self) {
        self.visible_indices.clear();

        let filter_lower = self.filter_text.to_lowercase();

        for (i, proc) in self.processes.iter().enumerate() {
            if !filter_lower.is_empty()
                && !proc.name.to_lowercase().contains(&filter_lower)
                && !proc.pid.to_string().contains(&filter_lower)
            {
                continue;
            }
            self.visible_indices.push(i);
        }

        // Sort visible indices by the selected column.
        let processes = &self.processes;
        let col = self.sort_column;
        let dir = self.sort_direction;

        self.visible_indices.sort_by(|&a, &b| {
            let pa = match processes.get(a) {
                Some(p) => p,
                None => return std::cmp::Ordering::Equal,
            };
            let pb = match processes.get(b) {
                Some(p) => p,
                None => return std::cmp::Ordering::Equal,
            };

            let ord = match col {
                ProcessColumn::Pid => pa.pid.cmp(&pb.pid),
                ProcessColumn::Name => pa.name.to_lowercase().cmp(&pb.name.to_lowercase()),
                ProcessColumn::Status => pa.status.label().cmp(pb.status.label()),
                ProcessColumn::Cpu => pa.cpu_percent.partial_cmp(&pb.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal),
                ProcessColumn::Memory => pa.memory_bytes.cmp(&pb.memory_bytes),
                ProcessColumn::Threads => pa.thread_count.cmp(&pb.thread_count),
                ProcessColumn::Priority => pa.priority.cmp(&pb.priority),
                ProcessColumn::User => pa.user.cmp(&pb.user),
            };

            match dir {
                SortDirection::Ascending => ord,
                SortDirection::Descending => ord.reverse(),
            }
        });

        // If tree mode, reorder to parent-child depth-first ordering.
        if self.view_mode == ViewMode::Tree {
            self.arrange_tree();
        }

        // Clamp selection.
        if let Some(sel) = self.selected_index {
            if sel >= self.visible_indices.len() {
                self.selected_index = if self.visible_indices.is_empty() {
                    None
                } else {
                    Some(self.visible_indices.len().saturating_sub(1))
                };
            }
        }
    }

    /// Rearrange `visible_indices` into a depth-first tree based on PPID.
    fn arrange_tree(&mut self) {
        // Build a children map: parent_pid -> list of visible-index positions.
        let mut children_map: HashMap<u32, Vec<usize>> = HashMap::new();
        for &idx in &self.visible_indices {
            if let Some(proc) = self.processes.get(idx) {
                children_map.entry(proc.ppid).or_default().push(idx);
            }
        }

        // Walk from roots (ppid == 0 or ppid not in process set).
        let known_pids: Vec<u32> = self.visible_indices.iter()
            .filter_map(|&i| self.processes.get(i).map(|p| p.pid))
            .collect();

        let mut ordered = Vec::with_capacity(self.visible_indices.len());
        let mut stack: Vec<(usize, u32)> = Vec::new(); // (process_vec_index, depth)

        // Find roots: processes whose ppid is 0 or whose parent is not visible.
        let mut root_indices: Vec<usize> = Vec::new();
        for &idx in &self.visible_indices {
            if let Some(proc) = self.processes.get(idx) {
                if proc.ppid == 0 || !known_pids.contains(&proc.ppid) {
                    root_indices.push(idx);
                }
            }
        }

        // Push roots in reverse so the first comes out first.
        for &idx in root_indices.iter().rev() {
            stack.push((idx, 0));
        }

        while let Some((idx, depth)) = stack.pop() {
            // Set tree depth on the process.
            if let Some(proc) = self.processes.get_mut(idx) {
                proc.tree_depth = depth as u32;
            }
            ordered.push(idx);

            // Push children in reverse order.
            let pid = self.processes.get(idx).map(|p| p.pid).unwrap_or(0);
            if let Some(kids) = children_map.get(&pid) {
                for &child_idx in kids.iter().rev() {
                    stack.push((child_idx, depth + 1));
                }
            }
        }

        self.visible_indices = ordered;
    }

    /// Push the latest system values into history ring buffers.
    fn update_histories(&mut self) {
        self.cpu_history.push(self.system_info.cpu_overall);

        // Ensure per-core histories match the core count.
        while self.core_histories.len() < self.system_info.cpu_per_core.len() {
            self.core_histories.push(GraphHistory::new());
        }
        for (i, &usage) in self.system_info.cpu_per_core.iter().enumerate() {
            if let Some(hist) = self.core_histories.get_mut(i) {
                hist.push(usage);
            }
        }
    }

    /// Update the status bar message.
    fn update_status(&mut self) {
        let total = self.processes.len();
        let running = self.processes.iter().filter(|p| p.status == ProcessStatus::Running).count();
        self.status_message = format!(
            "{total} processes ({running} running) | CPU: {:.1}% | Mem: {} / {} | Refresh: {}",
            self.system_info.cpu_overall,
            format_bytes(self.system_info.used_memory),
            format_bytes(self.system_info.total_memory),
            self.refresh_interval.label(),
        );
        self.system_info.process_count = total as u32;
        self.system_info.running_count = running as u32;
    }

    // ========================================================================
    // Actions
    // ========================================================================

    /// Kill the selected process.
    pub fn kill_selected(&mut self) {
        if let Some(sel) = self.selected_index {
            if let Some(&proc_idx) = self.visible_indices.get(sel) {
                if let Some(proc) = self.processes.get(proc_idx) {
                    let pid = proc.pid;
                    let name = proc.name.clone();
                    // In production: sys_process_kill(pid)
                    self.status_message = format!("Killed process {name} (PID {pid})");
                    self.processes.remove(proc_idx);
                    self.rebuild_visible_list();
                }
            }
        }
    }

    /// Pause (stop) the selected process.
    pub fn pause_selected(&mut self) {
        if let Some(sel) = self.selected_index {
            if let Some(&proc_idx) = self.visible_indices.get(sel) {
                if let Some(proc) = self.processes.get_mut(proc_idx) {
                    // In production: sys_process_stop(proc.pid)
                    proc.status = ProcessStatus::Stopped;
                    self.status_message = format!("Paused {} (PID {})", proc.name, proc.pid);
                }
            }
        }
    }

    /// Resume the selected process.
    pub fn resume_selected(&mut self) {
        if let Some(sel) = self.selected_index {
            if let Some(&proc_idx) = self.visible_indices.get(sel) {
                if let Some(proc) = self.processes.get_mut(proc_idx) {
                    // In production: sys_process_continue(proc.pid)
                    proc.status = ProcessStatus::Running;
                    self.status_message = format!("Resumed {} (PID {})", proc.name, proc.pid);
                }
            }
        }
    }

    /// Set sort column. If the same column is clicked again, toggle direction.
    pub fn set_sort_column(&mut self, col: ProcessColumn) {
        if self.sort_column == col {
            self.sort_direction = match self.sort_direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.sort_column = col;
            self.sort_direction = SortDirection::Ascending;
        }
        self.rebuild_visible_list();
    }

    /// Toggle between list and tree view modes.
    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::List => ViewMode::Tree,
            ViewMode::Tree => ViewMode::List,
        };
        self.rebuild_visible_list();
    }

    /// Cycle the auto-refresh interval.
    pub fn cycle_refresh_interval(&mut self) {
        self.refresh_interval = self.refresh_interval.next();
        self.update_status();
    }

    /// Get the currently selected process (if any).
    pub fn selected_process(&self) -> Option<&ProcessInfo> {
        let sel = self.selected_index?;
        let &proc_idx = self.visible_indices.get(sel)?;
        self.processes.get(proc_idx)
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle an input event and return whether it was consumed.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Mouse(mouse_ev) => self.handle_mouse(mouse_ev),
            Event::Resize { width, height } => {
                self.window_width = *width;
                self.window_height = *height;
                EventResult::Consumed
            }
            Event::Tick { elapsed_ms } => {
                self.ms_since_refresh = self.ms_since_refresh.saturating_add(*elapsed_ms);
                if self.ms_since_refresh >= self.refresh_interval.ms() {
                    self.ms_since_refresh = 0;
                    self.refresh();
                }
                EventResult::Consumed
            }
            Event::CloseRequested => EventResult::Ignored,
            _ => EventResult::Ignored,
        }
    }

    /// Handle a keyboard event.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }

        // If filter box is focused, route text input there.
        if self.filter_focused {
            return self.handle_filter_key(key);
        }

        match key.key {
            // Delete = kill selected process
            Key::Delete if key.modifiers == Modifiers::NONE => {
                self.kill_selected();
                EventResult::Consumed
            }
            // F5 = refresh
            Key::F5 => {
                self.refresh();
                self.status_message = "Refreshed".to_string();
                EventResult::Consumed
            }
            // Ctrl+F = focus search box
            Key::F if key.modifiers.ctrl => {
                self.filter_focused = true;
                EventResult::Consumed
            }
            // Tab = next tab
            Key::Tab if key.modifiers == Modifiers::NONE => {
                self.active_tab = match self.active_tab {
                    Tab::Processes => Tab::System,
                    Tab::System => Tab::Network,
                    Tab::Network => Tab::Details,
                    Tab::Details => Tab::Processes,
                };
                EventResult::Consumed
            }
            // Shift+Tab = previous tab
            Key::Tab if key.modifiers.shift => {
                self.active_tab = match self.active_tab {
                    Tab::Processes => Tab::Details,
                    Tab::System => Tab::Processes,
                    Tab::Network => Tab::System,
                    Tab::Details => Tab::Network,
                };
                EventResult::Consumed
            }
            // Arrow keys for process list navigation
            Key::Up if key.modifiers == Modifiers::NONE => {
                self.move_selection(-1);
                EventResult::Consumed
            }
            Key::Down if key.modifiers == Modifiers::NONE => {
                self.move_selection(1);
                EventResult::Consumed
            }
            Key::PageUp => {
                self.move_selection(-10);
                EventResult::Consumed
            }
            Key::PageDown => {
                self.move_selection(10);
                EventResult::Consumed
            }
            Key::Home => {
                self.selected_index = if self.visible_indices.is_empty() {
                    None
                } else {
                    Some(0)
                };
                self.scroll_offset = 0;
                EventResult::Consumed
            }
            Key::End => {
                self.selected_index = if self.visible_indices.is_empty() {
                    None
                } else {
                    Some(self.visible_indices.len().saturating_sub(1))
                };
                EventResult::Consumed
            }
            // Enter on process list = open details tab
            Key::Enter if self.active_tab == Tab::Processes => {
                if self.selected_index.is_some() {
                    self.active_tab = Tab::Details;
                }
                EventResult::Consumed
            }
            // Escape = close context menu or clear filter
            Key::Escape => {
                if self.context_menu.is_some() {
                    self.context_menu = None;
                } else if self.filter_focused {
                    self.filter_focused = false;
                } else if !self.filter_text.is_empty() {
                    self.filter_text.clear();
                    self.rebuild_visible_list();
                }
                EventResult::Consumed
            }
            // V = toggle view mode
            Key::V if key.modifiers == Modifiers::NONE && !self.filter_focused => {
                self.toggle_view_mode();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle keyboard input when the filter box is focused.
    fn handle_filter_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.filter_focused = false;
                EventResult::Consumed
            }
            Key::Enter => {
                self.filter_focused = false;
                EventResult::Consumed
            }
            Key::Backspace => {
                self.filter_text.pop();
                self.rebuild_visible_list();
                EventResult::Consumed
            }
            _ => {
                if let Some(ch) = key.text {
                    if ch.is_ascii_graphic() || ch == ' ' {
                        self.filter_text.push(ch);
                        self.rebuild_visible_list();
                    }
                }
                EventResult::Consumed
            }
        }
    }

    /// Handle a mouse event.
    fn handle_mouse(&mut self, mouse: &guitk::event::MouseEvent) -> EventResult {
        let mx = mouse.x;
        let my = mouse.y;

        // If context menu is open, handle it first.
        if let Some(ref menu) = self.context_menu.clone() {
            if let MouseEventKind::Press(MouseButton::Left) = &mouse.kind {
                let menu_w = 180.0;
                let item_h = 24.0;
                let item_count = ContextAction::ALL.len() as f32;

                if mx >= menu.x
                    && mx <= menu.x + menu_w
                    && my >= menu.y
                    && my <= menu.y + item_h * item_count
                {
                    let index = ((my - menu.y) / item_h) as usize;
                    if let Some(&action) = ContextAction::ALL.get(index) {
                        self.execute_context_action(action, menu.target_pid);
                    }
                }
                self.context_menu = None;
                return EventResult::Consumed;
            }
        }

        match &mouse.kind {
            // Left click — tab bar, toolbar, column headers, process rows
            MouseEventKind::Press(MouseButton::Left) => {
                self.context_menu = None;

                // Tab bar click
                if my >= TOOLBAR_HEIGHT && my < TOOLBAR_HEIGHT + TAB_BAR_HEIGHT {
                    let mut tab_x = 0.0f32;
                    for tab in &Tab::ALL {
                        let tab_w = (tab.label().len() as f32) * 9.0 + 24.0;
                        if mx >= tab_x && mx < tab_x + tab_w {
                            self.active_tab = *tab;
                            return EventResult::Consumed;
                        }
                        tab_x += tab_w;
                    }
                    return EventResult::Consumed;
                }

                // Toolbar buttons (simplified hit regions)
                if my < TOOLBAR_HEIGHT {
                    return self.handle_toolbar_click(mx);
                }

                // Column header click (process tab only)
                let content_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
                if self.active_tab == Tab::Processes
                    && my >= content_y
                    && my < content_y + HEADER_HEIGHT
                {
                    let mut col_x = 0.0f32;
                    for col in &ProcessColumn::ALL {
                        let cw = col.width();
                        if mx >= col_x && mx < col_x + cw {
                            self.set_sort_column(*col);
                            return EventResult::Consumed;
                        }
                        col_x += cw;
                    }
                    return EventResult::Consumed;
                }

                // Process row click
                if self.active_tab == Tab::Processes && my >= content_y + HEADER_HEIGHT {
                    let row_f = (my - content_y - HEADER_HEIGHT) / ROW_HEIGHT;
                    let row_idx = row_f as usize + self.scroll_offset;
                    if row_idx < self.visible_indices.len() {
                        self.selected_index = Some(row_idx);
                    }
                    return EventResult::Consumed;
                }

                EventResult::Consumed
            }

            // Right click — context menu on process rows
            MouseEventKind::Press(MouseButton::Right) => {
                let content_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
                if self.active_tab == Tab::Processes && my >= content_y + HEADER_HEIGHT {
                    let row_f = (my - content_y - HEADER_HEIGHT) / ROW_HEIGHT;
                    let row_idx = row_f as usize + self.scroll_offset;
                    if row_idx < self.visible_indices.len() {
                        self.selected_index = Some(row_idx);
                        if let Some(&proc_idx) = self.visible_indices.get(row_idx) {
                            let pid = self.processes.get(proc_idx)
                                .map(|p| p.pid)
                                .unwrap_or(0);
                            self.context_menu = Some(ContextMenu {
                                x: mx,
                                y: my,
                                target_pid: pid,
                                hover_index: None,
                            });
                        }
                    }
                }
                EventResult::Consumed
            }

            // Scroll wheel — scroll the process list
            MouseEventKind::Scroll { dy, .. } => {
                if *dy < 0.0 {
                    self.scroll_offset = self.scroll_offset.saturating_add(3);
                } else if *dy > 0.0 {
                    self.scroll_offset = self.scroll_offset.saturating_sub(3);
                }
                // Clamp scroll offset.
                let max_scroll = self.visible_indices.len().saturating_sub(1);
                if self.scroll_offset > max_scroll {
                    self.scroll_offset = max_scroll;
                }
                EventResult::Consumed
            }

            // Mouse move — update hover state
            MouseEventKind::Move => {
                let content_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
                if self.active_tab == Tab::Processes && my >= content_y + HEADER_HEIGHT {
                    let row_f = (my - content_y - HEADER_HEIGHT) / ROW_HEIGHT;
                    let row_idx = row_f as usize + self.scroll_offset;
                    self.hovered_index = if row_idx < self.visible_indices.len() {
                        Some(row_idx)
                    } else {
                        None
                    };
                } else {
                    self.hovered_index = None;
                }

                // Update context menu hover.
                if let Some(ref mut menu) = self.context_menu {
                    let menu_w = 180.0;
                    let item_h = 24.0;
                    let item_count = ContextAction::ALL.len() as f32;
                    if mx >= menu.x
                        && mx <= menu.x + menu_w
                        && my >= menu.y
                        && my <= menu.y + item_h * item_count
                    {
                        menu.hover_index = Some(((my - menu.y) / item_h) as usize);
                    } else {
                        menu.hover_index = None;
                    }
                }

                EventResult::Consumed
            }

            _ => EventResult::Ignored,
        }
    }

    /// Handle a click in the toolbar region.
    fn handle_toolbar_click(&mut self, mx: f32) -> EventResult {
        // Button layout: [End Process 90px][New Task 80px][Refresh 70px][View 60px][gap][filter box]
        if mx < 90.0 {
            self.kill_selected();
        } else if mx < 170.0 {
            // New Task: in production, open a run dialog.
            self.status_message = "New Task dialog not yet implemented".to_string();
        } else if mx < 240.0 {
            self.refresh();
        } else if mx < 300.0 {
            self.toggle_view_mode();
        } else if mx >= self.window_width as f32 - 210.0 {
            self.filter_focused = true;
        }
        EventResult::Consumed
    }

    /// Execute a context menu action on a target process.
    fn execute_context_action(&mut self, action: ContextAction, target_pid: u32) {
        // Find the process by PID.
        let proc_idx = self.processes.iter().position(|p| p.pid == target_pid);

        match action {
            ContextAction::Kill => {
                if let Some(idx) = proc_idx {
                    let name = self.processes.get(idx)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    self.processes.remove(idx);
                    self.rebuild_visible_list();
                    self.status_message = format!("Killed {name} (PID {target_pid})");
                }
            }
            ContextAction::Pause => {
                if let Some(idx) = proc_idx {
                    if let Some(proc) = self.processes.get_mut(idx) {
                        proc.status = ProcessStatus::Stopped;
                        self.status_message = format!("Paused {} (PID {target_pid})", proc.name);
                    }
                }
            }
            ContextAction::Resume => {
                if let Some(idx) = proc_idx {
                    if let Some(proc) = self.processes.get_mut(idx) {
                        proc.status = ProcessStatus::Running;
                        self.status_message = format!("Resumed {} (PID {target_pid})", proc.name);
                    }
                }
            }
            ContextAction::ChangePriority => {
                self.status_message = format!("Change priority for PID {target_pid} (dialog NYI)");
            }
            ContextAction::OpenFileLocation => {
                self.status_message = format!("Open file location for PID {target_pid} (NYI)");
            }
        }
    }

    /// Move the selection by `delta` rows (negative = up, positive = down).
    fn move_selection(&mut self, delta: i32) {
        if self.visible_indices.is_empty() {
            return;
        }

        let current = self.selected_index.unwrap_or(0) as i32;
        let max_idx = (self.visible_indices.len() as i32).saturating_sub(1);
        let new_idx = (current + delta).clamp(0, max_idx) as usize;
        self.selected_index = Some(new_idx);

        // Ensure the selection is visible by adjusting scroll.
        let visible_rows = self.visible_row_count();
        if new_idx < self.scroll_offset {
            self.scroll_offset = new_idx;
        } else if new_idx >= self.scroll_offset + visible_rows {
            self.scroll_offset = new_idx.saturating_sub(visible_rows.saturating_sub(1));
        }
    }

    /// Number of process rows visible in the current window.
    fn visible_row_count(&self) -> usize {
        let content_h = self.window_height as f32
            - TOOLBAR_HEIGHT
            - TAB_BAR_HEIGHT
            - STATUS_BAR_HEIGHT
            - HEADER_HEIGHT;
        if content_h <= 0.0 {
            return 0;
        }
        (content_h / ROW_HEIGHT) as usize
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the complete process explorer UI into a `RenderTree`.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();
        let w = self.window_width as f32;
        let h = self.window_height as f32;

        // Background
        tree.fill_rect(0.0, 0.0, w, h, COLOR_CONTENT_BG);

        // Toolbar
        self.render_toolbar(&mut tree);

        // Tab bar
        self.render_tab_bar(&mut tree);

        // Content area (depends on active tab)
        match self.active_tab {
            Tab::Processes => self.render_process_tab(&mut tree),
            Tab::System => self.render_system_tab(&mut tree),
            Tab::Network => self.render_network_tab(&mut tree),
            Tab::Details => self.render_details_tab(&mut tree),
        }

        // Status bar
        self.render_status_bar(&mut tree);

        // Context menu overlay (drawn on top of everything)
        self.render_context_menu(&mut tree);

        tree
    }

    // -- Toolbar ------------------------------------------------------------

    /// Render the toolbar with action buttons and search box.
    fn render_toolbar(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        tree.fill_rect(0.0, 0.0, w, TOOLBAR_HEIGHT, COLOR_TOOLBAR_BG);

        let btn_h = 26.0;
        let btn_y = (TOOLBAR_HEIGHT - btn_h) / 2.0;
        let mut bx = 8.0;

        // End Process button
        let end_w = 90.0;
        tree.fill_rect(bx, btn_y, end_w, btn_h, COLOR_DANGER);
        self.render_bold_text(tree, bx + 8.0, btn_y + 6.0, "End Process", Color::WHITE, 11.0);
        bx += end_w + 6.0;

        // New Task button
        let new_w = 80.0;
        tree.fill_rect(bx, btn_y, new_w, btn_h, COLOR_ACCENT);
        self.render_bold_text(tree, bx + 10.0, btn_y + 6.0, "New Task", Color::WHITE, 11.0);
        bx += new_w + 6.0;

        // Refresh button
        let ref_w = 70.0;
        tree.fill_rect(bx, btn_y, ref_w, btn_h, Color::rgb(60, 65, 75));
        tree.text(bx + 12.0, btn_y + 6.0, "Refresh", COLOR_TEXT, 11.0);
        bx += ref_w + 6.0;

        // View mode toggle
        let view_w = 60.0;
        let view_label = match self.view_mode {
            ViewMode::List => "List",
            ViewMode::Tree => "Tree",
        };
        tree.fill_rect(bx, btn_y, view_w, btn_h, Color::rgb(60, 65, 75));
        tree.text(bx + 12.0, btn_y + 6.0, view_label, COLOR_TEXT, 11.0);

        // Filter / search box (right-aligned)
        let filter_w = 200.0;
        let filter_x = w - filter_w - 8.0;
        let filter_border = if self.filter_focused { COLOR_ACCENT } else { Color::rgb(70, 75, 85) };
        tree.stroke_rect(filter_x, btn_y, filter_w, btn_h, filter_border, 1.0);
        tree.fill_rect(filter_x + 1.0, btn_y + 1.0, filter_w - 2.0, btn_h - 2.0, Color::rgb(25, 28, 34));

        let filter_display = if self.filter_text.is_empty() {
            "Filter (Ctrl+F)"
        } else {
            &self.filter_text
        };
        let text_color = if self.filter_text.is_empty() { COLOR_TEXT_DIM } else { COLOR_TEXT };
        tree.text(filter_x + 8.0, btn_y + 6.0, filter_display, text_color, 11.0);

        // Cursor indicator when focused.
        if self.filter_focused {
            let cursor_x = filter_x + 8.0 + self.filter_text.len() as f32 * 7.0;
            tree.fill_rect(cursor_x, btn_y + 4.0, 1.0, btn_h - 8.0, COLOR_TEXT);
        }
    }

    // -- Tab bar ------------------------------------------------------------

    /// Render the tab bar.
    fn render_tab_bar(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let y = TOOLBAR_HEIGHT;
        tree.fill_rect(0.0, y, w, TAB_BAR_HEIGHT, COLOR_TAB_BG);

        let mut tx = 0.0f32;
        for tab in &Tab::ALL {
            let label = tab.label();
            let tab_w = label.len() as f32 * 9.0 + 24.0;
            let is_active = *tab == self.active_tab;

            if is_active {
                tree.fill_rect(tx, y, tab_w, TAB_BAR_HEIGHT, COLOR_TOOLBAR_BG);
                // Active indicator line at bottom
                tree.fill_rect(tx, y + TAB_BAR_HEIGHT - 2.0, tab_w, 2.0, COLOR_TAB_ACTIVE);
            }

            let text_color = if is_active { COLOR_TEXT } else { COLOR_TEXT_DIM };
            tree.text(tx + 12.0, y + 7.0, label, text_color, 12.0);
            tx += tab_w;
        }
    }

    // -- Status bar ---------------------------------------------------------

    /// Render the status bar at the bottom.
    fn render_status_bar(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let y = self.window_height as f32 - STATUS_BAR_HEIGHT;

        tree.fill_rect(0.0, y, w, STATUS_BAR_HEIGHT, COLOR_STATUS_BG);
        tree.text(8.0, y + 5.0, &self.status_message, COLOR_TEXT_DIM, 11.0);
    }

    // -- Process tab --------------------------------------------------------

    /// Render the Processes tab: column headers + process rows.
    fn render_process_tab(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let content_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
        let content_h = self.window_height as f32 - content_y - STATUS_BAR_HEIGHT;

        // Column headers
        tree.fill_rect(0.0, content_y, w, HEADER_HEIGHT, COLOR_HEADER_BG);

        let mut col_x = 0.0f32;
        for col in &ProcessColumn::ALL {
            let cw = col.width();
            let label = col.label();

            // Sort indicator
            let display = if *col == self.sort_column {
                let arrow = match self.sort_direction {
                    SortDirection::Ascending => " \u{25B2}",
                    SortDirection::Descending => " \u{25BC}",
                };
                format!("{label}{arrow}")
            } else {
                label.to_string()
            };

            let label_color = if *col == self.sort_column { COLOR_ACCENT } else { COLOR_TEXT_DIM };
            tree.text(col_x + 6.0, content_y + 5.0, &display, label_color, 11.0);

            // Column separator
            tree.fill_rect(col_x + cw - 1.0, content_y + 2.0, 1.0, HEADER_HEIGHT - 4.0, Color::rgb(55, 60, 70));
            col_x += cw;
        }

        // Process rows
        let rows_y = content_y + HEADER_HEIGHT;
        let row_area_h = content_h - HEADER_HEIGHT;
        let visible_rows = if row_area_h > 0.0 { (row_area_h / ROW_HEIGHT) as usize } else { 0 };

        tree.clip(0.0, rows_y, w, row_area_h);

        for vis_i in 0..visible_rows {
            let row_idx = vis_i + self.scroll_offset;
            let proc_vec_idx = match self.visible_indices.get(row_idx) {
                Some(&idx) => idx,
                None => break,
            };
            let proc = match self.processes.get(proc_vec_idx) {
                Some(p) => p,
                None => continue,
            };

            let ry = rows_y + vis_i as f32 * ROW_HEIGHT;

            // Row background
            let bg = if self.selected_index == Some(row_idx) {
                COLOR_ROW_SELECTED
            } else if self.hovered_index == Some(row_idx) {
                COLOR_ROW_HOVER
            } else if row_idx % 2 == 0 {
                COLOR_ROW_EVEN
            } else {
                COLOR_ROW_ODD
            };
            tree.fill_rect(0.0, ry, w, ROW_HEIGHT, bg);

            // Render each column cell
            let mut cx = 0.0f32;

            for col in &ProcessColumn::ALL {
                let cw = col.width();
                let indent = if self.view_mode == ViewMode::Tree
                    && *col == ProcessColumn::Name
                {
                    proc.tree_depth as f32 * 16.0
                } else {
                    0.0
                };

                match col {
                    ProcessColumn::Pid => {
                        tree.text(cx + 6.0, ry + 4.0, &proc.pid.to_string(), COLOR_TEXT_DIM, 11.0);
                    }
                    ProcessColumn::Name => {
                        // Tree connector prefix
                        if self.view_mode == ViewMode::Tree && proc.tree_depth > 0 {
                            tree.text(
                                cx + 6.0 + indent - 14.0,
                                ry + 4.0,
                                "\u{2514}\u{2500}",
                                Color::rgb(80, 85, 95),
                                11.0,
                            );
                        }
                        tree.text(cx + 6.0 + indent, ry + 4.0, &proc.name, COLOR_TEXT, 11.0);
                    }
                    ProcessColumn::Status => {
                        tree.text(cx + 6.0, ry + 4.0, proc.status.label(), proc.status.color(), 11.0);
                    }
                    ProcessColumn::Cpu => {
                        let cpu_str = format!("{:.1}", proc.cpu_percent);
                        let cpu_color = if proc.cpu_percent > 50.0 {
                            COLOR_DANGER
                        } else if proc.cpu_percent > 10.0 {
                            COLOR_STATUS_STOPPED
                        } else {
                            COLOR_TEXT
                        };
                        tree.text(cx + 6.0, ry + 4.0, &cpu_str, cpu_color, 11.0);
                    }
                    ProcessColumn::Memory => {
                        tree.text(cx + 6.0, ry + 4.0, &format_bytes(proc.memory_bytes), COLOR_TEXT, 11.0);
                    }
                    ProcessColumn::Threads => {
                        tree.text(cx + 6.0, ry + 4.0, &proc.thread_count.to_string(), COLOR_TEXT_DIM, 11.0);
                    }
                    ProcessColumn::Priority => {
                        tree.text(cx + 6.0, ry + 4.0, &proc.priority.to_string(), COLOR_TEXT_DIM, 11.0);
                    }
                    ProcessColumn::User => {
                        tree.text(cx + 6.0, ry + 4.0, &proc.user, COLOR_TEXT_DIM, 11.0);
                    }
                }
                cx += cw;
            }
        }

        tree.unclip();
    }

    // -- System tab ---------------------------------------------------------

    /// Render the System overview tab: CPU graph, memory bars, per-core bars.
    fn render_system_tab(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let content_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 8.0;
        let section_gap = 16.0;

        // -- CPU usage graph --
        let graph_x = 16.0;
        let graph_y = content_y;
        let graph_w = w - 32.0;
        let graph_h = 140.0;

        self.render_bold_text(tree, graph_x, graph_y, "CPU Usage", COLOR_TEXT, 13.0);
        let cpu_label = format!("{:.1}%", self.system_info.cpu_overall);
        tree.text(graph_x + 100.0, graph_y, &cpu_label, COLOR_GRAPH_CPU, 13.0);

        let chart_y = graph_y + 20.0;
        tree.fill_rect(graph_x, chart_y, graph_w, graph_h, Color::rgb(20, 22, 28));
        tree.stroke_rect(graph_x, chart_y, graph_w, graph_h, Color::rgb(50, 55, 65), 1.0);

        // Grid lines (25%, 50%, 75%)
        for pct in &[25.0f32, 50.0, 75.0] {
            let gy = chart_y + graph_h * (1.0 - pct / 100.0);
            self.render_dashed_hline(tree, graph_x + 1.0, gy, graph_w - 2.0, COLOR_GRAPH_GRID);
            let pct_label = format!("{:.0}%", pct);
            tree.text(graph_x + 2.0, gy - 10.0, &pct_label, COLOR_TEXT_DIM, 9.0);
        }

        // CPU history line
        self.render_line_graph(tree, graph_x, chart_y, graph_w, graph_h, &self.cpu_history, COLOR_GRAPH_CPU, 100.0);

        let mut cur_y = chart_y + graph_h + section_gap;

        // -- Memory usage bars --
        self.render_bold_text(tree, graph_x, cur_y, "Memory", COLOR_TEXT, 13.0);
        cur_y += 20.0;

        let bar_h = 20.0;
        let bar_w = graph_w - 120.0;

        // Total / Used / Free / Cached
        let mem_items: &[(&str, u64, Color)] = &[
            ("Used", self.system_info.used_memory, Color::rgb(80, 140, 220)),
            ("Cached", self.system_info.cached_memory, Color::rgb(120, 180, 80)),
            ("Free", self.system_info.free_memory, Color::rgb(60, 65, 75)),
        ];

        let total = self.system_info.total_memory.max(1);
        tree.fill_rect(graph_x, cur_y, bar_w, bar_h, Color::rgb(35, 38, 46));
        let mut fill_x = graph_x;

        for &(label, amount, color) in mem_items {
            let fraction = amount as f32 / total as f32;
            let fill_w = bar_w * fraction;
            if fill_w > 0.5 {
                tree.fill_rect(fill_x, cur_y, fill_w, bar_h, color);
            }
            fill_x += fill_w;

            // Legend entry
            let legend_y = cur_y + bar_h + 4.0;
            let legend_x = graph_x + mem_items.iter()
                .position(|&(l, _, _)| l == label)
                .unwrap_or(0) as f32 * 140.0;
            tree.fill_rect(legend_x, legend_y + 2.0, 10.0, 10.0, color);
            let legend_label = format!("{}: {}", label, format_bytes(amount));
            tree.text(legend_x + 14.0, legend_y, &legend_label, COLOR_TEXT_DIM, 10.0);
        }

        // Total label to the right of the bar
        tree.text(
            graph_x + bar_w + 8.0,
            cur_y + 3.0,
            &format!("Total: {}", format_bytes(self.system_info.total_memory)),
            COLOR_TEXT,
            11.0,
        );

        cur_y += bar_h + 28.0;

        // -- Swap usage --
        self.render_bold_text(tree, graph_x, cur_y, "Swap", COLOR_TEXT, 13.0);
        cur_y += 20.0;

        let swap_total = self.system_info.swap_total.max(1);
        let swap_frac = self.system_info.swap_used as f32 / swap_total as f32;
        tree.fill_rect(graph_x, cur_y, bar_w, 14.0, Color::rgb(35, 38, 46));
        let swap_fill_w = bar_w * swap_frac;
        if swap_fill_w > 0.5 {
            tree.fill_rect(graph_x, cur_y, swap_fill_w, 14.0, Color::rgb(200, 120, 60));
        }
        tree.text(
            graph_x + bar_w + 8.0,
            cur_y,
            &format!("{} / {}", format_bytes(self.system_info.swap_used), format_bytes(self.system_info.swap_total)),
            COLOR_TEXT_DIM,
            11.0,
        );
        cur_y += 24.0 + section_gap;

        // -- Per-CPU core bars --
        self.render_bold_text(tree, graph_x, cur_y, "Per-Core Utilization", COLOR_TEXT, 13.0);
        cur_y += 20.0;

        let core_bar_h = 14.0;
        let core_bar_gap = 4.0;
        for (i, &usage) in self.system_info.cpu_per_core.iter().enumerate() {
            let label = format!("Core {i}");
            tree.text(graph_x, cur_y, &label, COLOR_TEXT_DIM, 10.0);

            let cb_x = graph_x + 50.0;
            let cb_w = bar_w - 50.0;
            tree.fill_rect(cb_x, cur_y, cb_w, core_bar_h, Color::rgb(35, 38, 46));

            let fill_w = cb_w * (usage / 100.0);
            let bar_color = if usage > 80.0 {
                COLOR_DANGER
            } else if usage > 50.0 {
                COLOR_STATUS_STOPPED
            } else {
                COLOR_GRAPH_CPU
            };
            if fill_w > 0.5 {
                tree.fill_rect(cb_x, cur_y, fill_w, core_bar_h, bar_color);
            }

            let usage_str = format!("{usage:.0}%");
            tree.text(cb_x + cb_w + 6.0, cur_y, &usage_str, COLOR_TEXT_DIM, 10.0);

            cur_y += core_bar_h + core_bar_gap;
        }

        cur_y += section_gap;

        // -- Uptime and load average --
        let uptime = format_uptime(self.system_info.uptime_secs);
        tree.text(graph_x, cur_y, &format!("Uptime: {uptime}"), COLOR_TEXT, 12.0);
        cur_y += 18.0;

        let load = format!(
            "Load avg: {:.2}  {:.2}  {:.2}",
            self.system_info.load_avg[0],
            self.system_info.load_avg[1],
            self.system_info.load_avg[2],
        );
        tree.text(graph_x, cur_y, &load, COLOR_TEXT, 12.0);
    }

    // -- Network tab --------------------------------------------------------

    /// Render the Network tab: bandwidth graph + connections table.
    fn render_network_tab(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let content_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 8.0;

        // -- Bandwidth graph --
        let graph_x = 16.0;
        let graph_w = w - 32.0;
        let graph_h = 100.0;

        self.render_bold_text(tree, graph_x, content_y, "Network Bandwidth", COLOR_TEXT, 13.0);

        let chart_y = content_y + 20.0;
        tree.fill_rect(graph_x, chart_y, graph_w, graph_h, Color::rgb(20, 22, 28));
        tree.stroke_rect(graph_x, chart_y, graph_w, graph_h, Color::rgb(50, 55, 65), 1.0);

        // Determine max for scaling
        let max_bw = self.net_in_history.iter_oldest_first()
            .chain(self.net_out_history.iter_oldest_first())
            .fold(1.0f32, |acc, v| acc.max(v));

        self.render_line_graph(tree, graph_x, chart_y, graph_w, graph_h, &self.net_in_history, COLOR_GRAPH_NET_IN, max_bw);
        self.render_line_graph(tree, graph_x, chart_y, graph_w, graph_h, &self.net_out_history, COLOR_GRAPH_NET_OUT, max_bw);

        // Legend
        let legend_y = chart_y + graph_h + 4.0;
        tree.fill_rect(graph_x, legend_y + 2.0, 10.0, 10.0, COLOR_GRAPH_NET_IN);
        tree.text(graph_x + 14.0, legend_y, "In", COLOR_TEXT_DIM, 10.0);
        tree.fill_rect(graph_x + 50.0, legend_y + 2.0, 10.0, 10.0, COLOR_GRAPH_NET_OUT);
        tree.text(graph_x + 64.0, legend_y, "Out", COLOR_TEXT_DIM, 10.0);

        // -- Connections table --
        let table_y = legend_y + 24.0;
        self.render_bold_text(tree, graph_x, table_y, "Active Connections", COLOR_TEXT, 13.0);

        let hdr_y = table_y + 20.0;
        tree.fill_rect(0.0, hdr_y, w, HEADER_HEIGHT, COLOR_HEADER_BG);

        // Columns: Protocol, Local Address, Remote Address, State, PID, Process
        let net_cols: &[(&str, f32)] = &[
            ("Protocol", 70.0),
            ("Local Address", 180.0),
            ("Remote Address", 180.0),
            ("State", 100.0),
            ("PID", 60.0),
            ("Process", 140.0),
        ];

        let mut nx = 0.0f32;
        for &(label, col_w) in net_cols {
            tree.text(nx + 6.0, hdr_y + 5.0, label, COLOR_TEXT_DIM, 11.0);
            tree.fill_rect(nx + col_w - 1.0, hdr_y + 2.0, 1.0, HEADER_HEIGHT - 4.0, Color::rgb(55, 60, 70));
            nx += col_w;
        }

        // Connection rows
        let rows_y = hdr_y + HEADER_HEIGHT;
        let available_h = self.window_height as f32 - rows_y - STATUS_BAR_HEIGHT;
        let visible_rows = if available_h > 0.0 { (available_h / ROW_HEIGHT) as usize } else { 0 };

        tree.clip(0.0, rows_y, w, available_h);

        for (i, conn) in self.connections.iter().take(visible_rows).enumerate() {
            let ry = rows_y + i as f32 * ROW_HEIGHT;
            let bg = if i % 2 == 0 { COLOR_ROW_EVEN } else { COLOR_ROW_ODD };
            tree.fill_rect(0.0, ry, w, ROW_HEIGHT, bg);

            let mut cx = 0.0f32;
            let fields: &[&str] = &[
                &conn.protocol,
                &conn.local_addr,
                &conn.remote_addr,
                &conn.state,
                &conn.pid.to_string(),
                &conn.process_name,
            ];
            for (j, &field) in fields.iter().enumerate() {
                let col_w = net_cols.get(j).map(|c| c.1).unwrap_or(100.0);
                let color = if j == 3 {
                    // State column gets color coding.
                    match field {
                        "ESTABLISHED" => COLOR_STATUS_RUNNING,
                        "LISTEN" => COLOR_STATUS_SLEEPING,
                        "TIME_WAIT" | "CLOSE_WAIT" => COLOR_STATUS_STOPPED,
                        _ => COLOR_TEXT,
                    }
                } else {
                    COLOR_TEXT
                };
                tree.text(cx + 6.0, ry + 4.0, field, color, 11.0);
                cx += col_w;
            }
        }

        tree.unclip();
    }

    // -- Details tab --------------------------------------------------------

    /// Render the Details tab for the currently selected process.
    fn render_details_tab(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let content_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 8.0;
        let pad = 16.0;

        let proc = match self.selected_process() {
            Some(p) => p,
            None => {
                tree.text(pad, content_y + 20.0, "No process selected. Select a process on the Processes tab.", COLOR_TEXT_DIM, 13.0);
                return;
            }
        };

        // -- Header --
        self.render_bold_text(tree, pad, content_y, &format!("{} (PID {})", proc.name, proc.pid), COLOR_TEXT, 15.0);

        let mut cur_y = content_y + 24.0;

        // -- Basic info grid --
        let info_items: &[(&str, String)] = &[
            ("PPID", proc.ppid.to_string()),
            ("Status", proc.status.label().to_string()),
            ("Priority", proc.priority.to_string()),
            ("User", proc.user.clone()),
            ("Start time", format!("{}s after boot", proc.start_time_secs)),
            ("CPU time", format!("{}ms", proc.cpu_time_ms)),
            ("Threads", proc.thread_count.to_string()),
            ("CPU%", format!("{:.1}%", proc.cpu_percent)),
        ];

        let label_w = 90.0;
        let col_gap = 260.0;

        for (i, (label, value)) in info_items.iter().enumerate() {
            let col = i / 4;
            let row = i % 4;
            let lx = pad + col as f32 * col_gap;
            let ly = cur_y + row as f32 * 18.0;

            tree.text(lx, ly, &format!("{label}:"), COLOR_TEXT_DIM, 11.0);
            tree.text(lx + label_w, ly, value, COLOR_TEXT, 11.0);
        }

        cur_y += 4.0 * 18.0 + 12.0;

        // -- Memory breakdown --
        tree.fill_rect(pad, cur_y, w - 2.0 * pad, 1.0, Color::rgb(55, 60, 70));
        cur_y += 8.0;

        self.render_bold_text(tree, pad, cur_y, "Memory", COLOR_TEXT, 12.0);
        cur_y += 18.0;

        let mem_fields: &[(&str, String)] = &[
            ("Resident", format_bytes(proc.memory_bytes)),
            ("Virtual", format_bytes(proc.virtual_bytes)),
            ("Shared", format_bytes(proc.shared_bytes)),
        ];
        for (i, (label, value)) in mem_fields.iter().enumerate() {
            let lx = pad + i as f32 * 200.0;
            tree.text(lx, cur_y, &format!("{label}: {value}"), COLOR_TEXT, 11.0);
        }
        cur_y += 22.0;

        // -- Command line --
        tree.fill_rect(pad, cur_y, w - 2.0 * pad, 1.0, Color::rgb(55, 60, 70));
        cur_y += 8.0;
        self.render_bold_text(tree, pad, cur_y, "Command Line", COLOR_TEXT, 12.0);
        cur_y += 18.0;
        let cmd_display = if proc.command_line.is_empty() { "(none)" } else { &proc.command_line };
        tree.text(pad, cur_y, cmd_display, COLOR_TEXT_DIM, 11.0);
        cur_y += 22.0;

        // -- Thread list --
        tree.fill_rect(pad, cur_y, w - 2.0 * pad, 1.0, Color::rgb(55, 60, 70));
        cur_y += 8.0;
        self.render_bold_text(tree, pad, cur_y, &format!("Threads ({})", proc.threads.len()), COLOR_TEXT, 12.0);
        cur_y += 18.0;

        // Thread table header
        tree.fill_rect(pad, cur_y, w - 2.0 * pad, HEADER_HEIGHT, COLOR_HEADER_BG);
        let thread_cols: &[(&str, f32)] = &[
            ("TID", 60.0),
            ("Name", 200.0),
            ("Status", 80.0),
            ("CPU%", 70.0),
        ];
        let mut tx = pad;
        for &(label, col_w) in thread_cols {
            tree.text(tx + 6.0, cur_y + 5.0, label, COLOR_TEXT_DIM, 10.0);
            tx += col_w;
        }
        cur_y += HEADER_HEIGHT;

        let max_thread_rows = 6;
        for (ti, thread) in proc.threads.iter().take(max_thread_rows).enumerate() {
            let ry = cur_y + ti as f32 * ROW_HEIGHT;
            let bg = if ti % 2 == 0 { COLOR_ROW_EVEN } else { COLOR_ROW_ODD };
            tree.fill_rect(pad, ry, w - 2.0 * pad, ROW_HEIGHT, bg);

            let mut tcx = pad;
            tree.text(tcx + 6.0, ry + 4.0, &thread.tid.to_string(), COLOR_TEXT_DIM, 10.0);
            tcx += 60.0;
            tree.text(tcx + 6.0, ry + 4.0, &thread.name, COLOR_TEXT, 10.0);
            tcx += 200.0;
            tree.text(tcx + 6.0, ry + 4.0, thread.status.label(), thread.status.color(), 10.0);
            tcx += 80.0;
            tree.text(tcx + 6.0, ry + 4.0, &format!("{:.1}", thread.cpu_percent), COLOR_TEXT, 10.0);
        }
        cur_y += (proc.threads.len().min(max_thread_rows) as f32) * ROW_HEIGHT + 12.0;

        // -- Handles / capabilities --
        if !proc.handles.is_empty() {
            tree.fill_rect(pad, cur_y, w - 2.0 * pad, 1.0, Color::rgb(55, 60, 70));
            cur_y += 8.0;
            self.render_bold_text(tree, pad, cur_y, &format!("Handles ({})", proc.handles.len()), COLOR_TEXT, 12.0);
            cur_y += 18.0;

            let max_handles = 5;
            for handle in proc.handles.iter().take(max_handles) {
                let entry = format!("#{}: [{}] {}", handle.handle_id, handle.resource_type, handle.description);
                tree.text(pad + 8.0, cur_y, &entry, COLOR_TEXT_DIM, 10.0);
                cur_y += 16.0;
            }
            if proc.handles.len() > max_handles {
                let more = proc.handles.len() - max_handles;
                tree.text(pad + 8.0, cur_y, &format!("... and {more} more"), COLOR_TEXT_DIM, 10.0);
                cur_y += 16.0;
            }
            cur_y += 8.0;
        }

        // -- Environment variables (collapsed summary) --
        if !proc.environment.is_empty() {
            tree.fill_rect(pad, cur_y, w - 2.0 * pad, 1.0, Color::rgb(55, 60, 70));
            cur_y += 8.0;
            self.render_bold_text(tree, pad, cur_y, &format!("Environment ({})", proc.environment.len()), COLOR_TEXT, 12.0);
            cur_y += 18.0;

            let max_env = 8;
            for (key, val) in proc.environment.iter().take(max_env) {
                let entry = format!("{key}={val}");
                // Truncate long values for display.
                let display = if entry.len() > 80 {
                    format!("{}...", &entry[..77])
                } else {
                    entry
                };
                tree.text(pad + 8.0, cur_y, &display, COLOR_TEXT_DIM, 10.0);
                cur_y += 16.0;
            }
            if proc.environment.len() > max_env {
                let more = proc.environment.len() - max_env;
                tree.text(pad + 8.0, cur_y, &format!("... and {more} more"), COLOR_TEXT_DIM, 10.0);
            }
        }
    }

    // -- Context menu -------------------------------------------------------

    /// Render the right-click context menu overlay.
    fn render_context_menu(&self, tree: &mut RenderTree) {
        let menu = match &self.context_menu {
            Some(m) => m,
            None => return,
        };

        let menu_w = 180.0;
        let item_h = 24.0;
        let item_count = ContextAction::ALL.len() as f32;
        let menu_h = item_h * item_count;

        // Shadow
        tree.fill_rect(menu.x + 2.0, menu.y + 2.0, menu_w, menu_h, Color::rgba(0, 0, 0, 100));

        // Background
        tree.fill_rect(menu.x, menu.y, menu_w, menu_h, Color::rgb(50, 54, 62));
        tree.stroke_rect(menu.x, menu.y, menu_w, menu_h, Color::rgb(80, 85, 95), 1.0);

        for (i, action) in ContextAction::ALL.iter().enumerate() {
            let iy = menu.y + i as f32 * item_h;

            if menu.hover_index == Some(i) {
                tree.fill_rect(menu.x + 1.0, iy, menu_w - 2.0, item_h, Color::rgb(70, 100, 160));
            }

            let text_color = if *action == ContextAction::Kill {
                COLOR_DANGER
            } else {
                COLOR_TEXT
            };
            tree.text(menu.x + 12.0, iy + 5.0, action.label(), text_color, 11.0);
        }
    }

    // ========================================================================
    // Drawing helpers
    // ========================================================================

    /// Render a line graph from a `GraphHistory` into a rectangular area.
    ///
    /// `max_value` is the value that maps to the top of the graph area.
    fn render_line_graph(
        &self,
        tree: &mut RenderTree,
        area_x: f32,
        area_y: f32,
        area_w: f32,
        area_h: f32,
        history: &GraphHistory,
        color: Color,
        max_value: f32,
    ) {
        let count = history.len();
        if count < 2 {
            return;
        }

        let max_val = if max_value > 0.0 { max_value } else { 1.0 };
        let samples: Vec<f32> = history.iter_oldest_first().collect();

        let step_x = area_w / (GRAPH_HISTORY_LEN as f32 - 1.0);

        // Draw line segments between consecutive samples.
        let mut prev_x = area_x;
        let first_sample = samples.first().copied().unwrap_or(0.0);
        let mut prev_y = area_y + area_h - (first_sample / max_val * area_h).clamp(0.0, area_h);

        for (i, &sample) in samples.iter().enumerate().skip(1) {
            let sx = area_x + i as f32 * step_x;
            let sy = area_y + area_h - (sample / max_val * area_h).clamp(0.0, area_h);

            tree.push(RenderCommand::Line {
                x1: prev_x,
                y1: prev_y,
                x2: sx,
                y2: sy,
                color,
                width: 1.5,
            });

            prev_x = sx;
            prev_y = sy;
        }
    }

    /// Render a horizontal dashed line (approximated as short segments).
    fn render_dashed_hline(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        total_w: f32,
        color: Color,
    ) {
        let dash_len = 4.0;
        let gap_len = 4.0;
        let mut cx = x;
        while cx < x + total_w {
            let seg_end = (cx + dash_len).min(x + total_w);
            tree.push(RenderCommand::Line {
                x1: cx,
                y1: y,
                x2: seg_end,
                y2: y,
                color,
                width: 1.0,
            });
            cx += dash_len + gap_len;
        }
    }

    /// Render bold text using the `FontWeightHint::Bold` variant.
    fn render_bold_text(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        text: &str,
        color: Color,
        font_size: f32,
    ) {
        tree.push(RenderCommand::Text {
            x,
            y,
            text: text.to_string(),
            color,
            font_size,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    // ========================================================================
    // Demo data (for development/testing)
    // ========================================================================

    /// Populate the explorer with sample data for UI testing.
    pub fn load_demo_data(&mut self) {
        self.processes = vec![
            make_demo_process(1, 0, "init", ProcessStatus::Running, 0.1, 4_194_304, 2, 0, "root"),
            make_demo_process(2, 1, "kthread", ProcessStatus::Sleeping, 0.0, 0, 1, -20, "root"),
            make_demo_process(100, 1, "compositor", ProcessStatus::Running, 8.5, 67_108_864, 6, 0, "system"),
            make_demo_process(101, 1, "netd", ProcessStatus::Sleeping, 0.3, 12_582_912, 4, 0, "system"),
            make_demo_process(200, 100, "desktop", ProcessStatus::Running, 3.2, 104_857_600, 12, 0, "user"),
            make_demo_process(201, 200, "explorer", ProcessStatus::Running, 1.1, 52_428_800, 4, 0, "user"),
            make_demo_process(202, 200, "terminal", ProcessStatus::Sleeping, 0.4, 20_971_520, 3, 0, "user"),
            make_demo_process(203, 200, "editor", ProcessStatus::Running, 12.7, 157_286_400, 8, 0, "user"),
            make_demo_process(300, 1, "httpd", ProcessStatus::Running, 2.1, 33_554_432, 16, 5, "www"),
            make_demo_process(301, 300, "httpd-worker", ProcessStatus::Running, 5.4, 16_777_216, 1, 5, "www"),
            make_demo_process(302, 300, "httpd-worker", ProcessStatus::Sleeping, 0.0, 16_777_216, 1, 5, "www"),
            make_demo_process(400, 1, "sshd", ProcessStatus::Sleeping, 0.0, 8_388_608, 1, 0, "root"),
            make_demo_process(500, 1, "zombie_proc", ProcessStatus::Zombie, 0.0, 0, 0, 0, "user"),
        ];

        // Add threads and handles to a few processes.
        if let Some(compositor) = self.processes.iter_mut().find(|p| p.pid == 100) {
            compositor.threads = vec![
                ThreadInfo { tid: 1001, name: "render".to_string(), status: ProcessStatus::Running, cpu_percent: 5.0 },
                ThreadInfo { tid: 1002, name: "input".to_string(), status: ProcessStatus::Sleeping, cpu_percent: 1.0 },
                ThreadInfo { tid: 1003, name: "vsync".to_string(), status: ProcessStatus::Sleeping, cpu_percent: 2.5 },
            ];
            compositor.handles = vec![
                HandleInfo { handle_id: 1, resource_type: "channel".to_string(), description: "desktop-ipc".to_string() },
                HandleInfo { handle_id: 2, resource_type: "vmo".to_string(), description: "framebuffer".to_string() },
                HandleInfo { handle_id: 3, resource_type: "event".to_string(), description: "vsync-signal".to_string() },
            ];
            compositor.environment = vec![
                ("DISPLAY".to_string(), ":0".to_string()),
                ("GPU_DRIVER".to_string(), "virtio-gpu".to_string()),
            ];
            compositor.command_line = "/usr/bin/compositor --backend=virtio-gpu --vsync".to_string();
        }

        self.system_info = SystemInfo {
            total_memory: 8_589_934_592,    // 8 GiB
            used_memory: 3_435_973_837,     // ~3.2 GiB
            free_memory: 3_221_225_472,     // ~3 GiB
            cached_memory: 1_932_735_283,   // ~1.8 GiB
            swap_total: 2_147_483_648,      // 2 GiB
            swap_used: 104_857_600,         // 100 MiB
            cpu_per_core: vec![12.0, 45.0, 8.0, 67.0],
            cpu_overall: 33.0,
            uptime_secs: 86472,
            load_avg: [1.23, 0.98, 0.87],
            process_count: 0,
            running_count: 0,
        };

        self.connections = vec![
            ConnectionInfo {
                protocol: "TCP".to_string(),
                local_addr: "0.0.0.0:80".to_string(),
                remote_addr: "*:*".to_string(),
                state: "LISTEN".to_string(),
                pid: 300,
                process_name: "httpd".to_string(),
            },
            ConnectionInfo {
                protocol: "TCP".to_string(),
                local_addr: "10.0.2.15:80".to_string(),
                remote_addr: "192.168.1.50:49832".to_string(),
                state: "ESTABLISHED".to_string(),
                pid: 301,
                process_name: "httpd-worker".to_string(),
            },
            ConnectionInfo {
                protocol: "TCP".to_string(),
                local_addr: "0.0.0.0:22".to_string(),
                remote_addr: "*:*".to_string(),
                state: "LISTEN".to_string(),
                pid: 400,
                process_name: "sshd".to_string(),
            },
            ConnectionInfo {
                protocol: "UDP".to_string(),
                local_addr: "0.0.0.0:68".to_string(),
                remote_addr: "*:*".to_string(),
                state: "".to_string(),
                pid: 101,
                process_name: "netd".to_string(),
            },
        ];

        // Push some initial history data.
        let cpu_samples = [
            20.0, 25.0, 22.0, 30.0, 35.0, 28.0, 40.0, 38.0, 33.0, 36.0,
            42.0, 38.0, 35.0, 30.0, 28.0, 25.0, 30.0, 33.0, 37.0, 33.0,
        ];
        for &s in &cpu_samples {
            self.cpu_history.push(s);
        }

        let net_in_samples = [
            100.0, 150.0, 200.0, 180.0, 300.0, 500.0, 450.0, 350.0, 200.0, 150.0,
        ];
        let net_out_samples = [
            50.0, 80.0, 120.0, 100.0, 200.0, 180.0, 160.0, 130.0, 90.0, 70.0,
        ];
        for &s in &net_in_samples {
            self.net_in_history.push(s);
        }
        for &s in &net_out_samples {
            self.net_out_history.push(s);
        }

        self.refresh();
    }
}

impl Default for ProcessExplorerState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Create a demo `ProcessInfo` with reasonable defaults.
fn make_demo_process(
    pid: u32,
    ppid: u32,
    name: &str,
    status: ProcessStatus,
    cpu: f32,
    mem: u64,
    threads: u32,
    priority: i32,
    user: &str,
) -> ProcessInfo {
    ProcessInfo {
        pid,
        ppid,
        name: name.to_string(),
        status,
        cpu_percent: cpu,
        memory_bytes: mem,
        virtual_bytes: mem.saturating_mul(3),
        shared_bytes: mem / 4,
        thread_count: threads,
        priority,
        user: user.to_string(),
        command_line: format!("/usr/bin/{name}"),
        start_time_secs: pid as u64 * 10,
        cpu_time_ms: (cpu * 1000.0) as u64,
        threads: Vec::new(),
        handles: Vec::new(),
        environment: Vec::new(),
        tree_depth: 0,
    }
}

/// Format a byte count for human-readable display.
fn format_bytes(bytes: u64) -> String {
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

/// Format an uptime in seconds as "Xd Xh Xm Xs".
fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    if days > 0 {
        format!("{days}d {hours}h {minutes}m {seconds}s")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else {
        format!("{minutes}m {seconds}s")
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let mut explorer = ProcessExplorerState::new();

    // Load demo data for initial display.
    explorer.load_demo_data();

    // Render the initial view.
    let render_tree = explorer.render();
    println!("Process Explorer initialized");
    println!("  {} processes loaded", explorer.processes.len());
    println!("  {} visible (after filter)", explorer.visible_indices.len());
    println!("  {} render commands", render_tree.len());
    println!("  Status: {}", explorer.status_message);

    // Demonstrate tab switching.
    explorer.active_tab = Tab::System;
    let sys_tree = explorer.render();
    println!("\nSystem tab: {} render commands", sys_tree.len());

    explorer.active_tab = Tab::Network;
    let net_tree = explorer.render();
    println!("Network tab: {} render commands", net_tree.len());

    // Demonstrate sorting.
    explorer.active_tab = Tab::Processes;
    explorer.set_sort_column(ProcessColumn::Memory);
    println!(
        "\nSorted by Memory ({}): first visible = {}",
        match explorer.sort_direction {
            SortDirection::Ascending => "asc",
            SortDirection::Descending => "desc",
        },
        explorer.visible_indices.first()
            .and_then(|&i| explorer.processes.get(i))
            .map(|p| p.name.as_str())
            .unwrap_or("(none)"),
    );

    // Demonstrate tree view.
    explorer.toggle_view_mode();
    let tree_render = explorer.render();
    println!("Tree view: {} render commands", tree_render.len());

    // Demonstrate filtering.
    explorer.filter_text = "http".to_string();
    explorer.rebuild_visible_list();
    println!(
        "Filter 'http': {} matches",
        explorer.visible_indices.len()
    );

    // Demonstrate details tab.
    explorer.filter_text.clear();
    explorer.rebuild_visible_list();
    explorer.selected_index = Some(2); // compositor
    explorer.active_tab = Tab::Details;
    let details_tree = explorer.render();
    println!("Details tab: {} render commands", details_tree.len());

    println!("\nProcess Explorer ready.");
}

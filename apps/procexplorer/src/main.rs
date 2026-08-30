//! Slate OS Process Explorer
//!
//! Graphical system monitor and task manager with:
//! - Process list with sortable columns and tree view
//! - System overview (CPU, memory, load)
//! - Per-process details panel (threads, handles, env)
//! - Network connections and bandwidth
//! - Toolbar with actions and search
//!
//! Uses the guitk library for UI rendering. All data is gathered
//! through Slate OS syscalls; the structs here define the presentation
//! layer while the OS provides the actual process/system information.

mod features;

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
use guitk::history::SampleHistory;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::scroll_window;
use guitk::table::{Column, Fit, Table};
use guitk::text;
use guitk::wheel;

use std::collections::HashMap;

// ============================================================================
// Constants — layout and colors
// ============================================================================

/// Height of the toolbar at the top of the window.
const TOOLBAR_HEIGHT: f32 = 40.0;
/// Height of the tab bar below the toolbar.
const TAB_BAR_HEIGHT: f32 = 28.0;

/// Width of the tab drawn for `label`, including the 12 px padding each side.
///
/// One function for the renderer and the click handler, so the hit targets
/// cannot drift off the tabs they belong to.
fn tab_width(label: &str) -> f32 {
    text::padded_width(label, 12.0, 12.0, FontWeightHint::Regular)
}
/// Height of the status bar at the bottom.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Height of a single row in process/connection tables.
const ROW_HEIGHT: f32 = 22.0;
/// Height of column headers in tables.
const HEADER_HEIGHT: f32 = 24.0;

/// Inset between a table cell's text and each edge of its column.
///
/// Named because it appears on both sides of every fitted cell: the text starts
/// `CELL_PAD` in from the column's left edge, and must stop `CELL_PAD` short of
/// the next column's, so a cell's usable width is `column_width - 2 * CELL_PAD`.
/// Getting this from the same constant the `x` offset uses is what keeps the
/// two from drifting.
const CELL_PAD: f32 = 6.0;

/// How far the Details tab indents a list entry (a handle, an env var) from the
/// panel's own padding. The entry's usable width is what is left of the panel
/// after this indent and the padding on both sides.
const DETAIL_INDENT: f32 = 8.0;

/// Rows per column in the Details tab's basic-info grid. The grid fills down
/// then across, so this is what turns an item's index into its row and column —
/// and, with the item count, into how many columns there are, which is what
/// tells the last column that its room runs to the right margin.
const INFO_ROWS: usize = 4;

/// Gap left between an info-grid value and whatever is drawn to its right, so
/// two adjacent cells never touch even when both are fitted to their room.
const INFO_GUTTER: f32 = 8.0;

/// The Network tab's connection table.
///
/// These widths are what a cell may *use*. The old `&[(&str, f32)]` array held
/// the column pitch instead, and nothing ever subtracted the padding from it:
/// every cell was drawn with a bare `tree.text` at `cx + 6.0` with no width at
/// all, then the cursor advanced by the full pitch. So a long value did not
/// merely clip — it was never bounded in the first place and drew straight over
/// every column to its right.
///
/// That mattered here more than in most tables, because a connection row is
/// made almost entirely of values this machine did not choose: a remote address
/// is whatever the peer at the other end happens to be, and an IPv6 address is
/// three times the width of the IPv4 ones the 180px column was eyeballed for.
const NET_COLUMNS: &[Column] = &[
    Column {
        label: "Protocol",
        width: 70.0 - CELL_PAD * 2.0,
    },
    Column {
        label: "Local Address",
        width: 180.0 - CELL_PAD * 2.0,
    },
    Column {
        label: "Remote Address",
        width: 180.0 - CELL_PAD * 2.0,
    },
    Column {
        label: "State",
        width: 100.0 - CELL_PAD * 2.0,
    },
    Column {
        label: "PID",
        width: 60.0 - CELL_PAD * 2.0,
    },
    Column {
        label: "Process",
        width: 140.0 - CELL_PAD * 2.0,
    },
];
const NET_PROTOCOL: usize = 0;
const NET_LOCAL_ADDR: usize = 1;
const NET_REMOTE_ADDR: usize = 2;
const NET_STATE: usize = 3;
const NET_PID: usize = 4;
const NET_PROCESS: usize = 5;
/// Font size of the connection table's headings and cells.
const NET_FONT: f32 = 11.0;
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

/// The sample history behind each of this program's time-series graphs.
///
/// This used to be a ring buffer written out here — a sample vector, a cursor
/// wrapped with `% GRAPH_HISTORY_LEN`, and a count that stopped climbing once
/// the vector was full. The resource monitor in the shell and the other of
/// these two programs each had their own copy of the same thing, and the three
/// had already drifted: one advanced its cursor with `+ 1` and another with
/// `wrapping_add(1)`, and `max_value` folded from zero here while the shell's
/// `peak` folded from negative infinity, so identical samples gave different
/// answers. It lives in [`guitk::history`] now; the only thing left for this
/// program to decide is how many samples wide its graphs are.
pub type GraphHistory = SampleHistory;

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
    /// Carries the fraction of a row a precision device sends.
    ///
    /// `scroll_offset` is a whole row index and cannot hold a fraction, so
    /// something has to. Without this the handler could only read the *sign*
    /// of `dy` and moved three rows for any non-zero value: a trackpad's
    /// stream of 0.2-notch events scrolled fifteen times too far, and there
    /// was no way to move by a single row at all.
    wheel: wheel::Accumulator,

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
            wheel: wheel::Accumulator::default(),
            filter_text: String::new(),
            filter_focused: false,
            context_menu: None,
            system_info,
            cpu_history: GraphHistory::new(GRAPH_HISTORY_LEN),
            core_histories: Vec::new(),
            connections: Vec::new(),
            net_in_history: GraphHistory::new(GRAPH_HISTORY_LEN),
            net_out_history: GraphHistory::new(GRAPH_HISTORY_LEN),
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
    /// In a real implementation this calls Slate OS syscalls to enumerate
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
                ProcessColumn::Cpu => pa
                    .cpu_percent
                    .partial_cmp(&pb.cpu_percent)
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
        if let Some(sel) = self.selected_index
            && sel >= self.visible_indices.len()
        {
            self.selected_index = if self.visible_indices.is_empty() {
                None
            } else {
                Some(self.visible_indices.len().saturating_sub(1))
            };
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
        let known_pids: Vec<u32> = self
            .visible_indices
            .iter()
            .filter_map(|&i| self.processes.get(i).map(|p| p.pid))
            .collect();

        let mut ordered = Vec::with_capacity(self.visible_indices.len());
        let mut stack: Vec<(usize, u32)> = Vec::new(); // (process_vec_index, depth)

        // Find roots: processes whose ppid is 0 or whose parent is not visible.
        let mut root_indices: Vec<usize> = Vec::new();
        for &idx in &self.visible_indices {
            if let Some(proc) = self.processes.get(idx)
                && (proc.ppid == 0 || !known_pids.contains(&proc.ppid))
            {
                root_indices.push(idx);
            }
        }

        // Push roots in reverse so the first comes out first.
        for &idx in root_indices.iter().rev() {
            stack.push((idx, 0));
        }

        while let Some((idx, depth)) = stack.pop() {
            // Set tree depth on the process.
            if let Some(proc) = self.processes.get_mut(idx) {
                proc.tree_depth = depth;
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
            self.core_histories
                .push(GraphHistory::new(GRAPH_HISTORY_LEN));
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
        let running = self
            .processes
            .iter()
            .filter(|p| p.status == ProcessStatus::Running)
            .count();
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
        if let Some(sel) = self.selected_index
            && let Some(&proc_idx) = self.visible_indices.get(sel)
            && let Some(proc) = self.processes.get(proc_idx)
        {
            let pid = proc.pid;
            let name = proc.name.clone();
            // In production: sys_process_kill(pid)
            self.status_message = format!("Killed process {name} (PID {pid})");
            self.processes.remove(proc_idx);
            self.rebuild_visible_list();
        }
    }

    /// Pause (stop) the selected process.
    pub fn pause_selected(&mut self) {
        if let Some(sel) = self.selected_index
            && let Some(&proc_idx) = self.visible_indices.get(sel)
            && let Some(proc) = self.processes.get_mut(proc_idx)
        {
            // In production: sys_process_stop(proc.pid)
            proc.status = ProcessStatus::Stopped;
            self.status_message = format!("Paused {} (PID {})", proc.name, proc.pid);
        }
    }

    /// Resume the selected process.
    pub fn resume_selected(&mut self) {
        if let Some(sel) = self.selected_index
            && let Some(&proc_idx) = self.visible_indices.get(sel)
            && let Some(proc) = self.processes.get_mut(proc_idx)
        {
            // In production: sys_process_continue(proc.pid)
            proc.status = ProcessStatus::Running;
            self.status_message = format!("Resumed {} (PID {})", proc.name, proc.pid);
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
                let allowed: String = key
                    .typed()
                    .filter(|ch| ch.is_ascii_graphic() || *ch == ' ')
                    .collect();
                if !allowed.is_empty() {
                    self.filter_text.push_str(&allowed);
                    self.rebuild_visible_list();
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
        if let Some(ref menu) = self.context_menu.clone()
            && let MouseEventKind::Press(MouseButton::Left) = &mouse.kind
        {
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

        match &mouse.kind {
            // Left click — tab bar, toolbar, column headers, process rows
            MouseEventKind::Press(MouseButton::Left) => {
                self.context_menu = None;

                // Tab bar click
                if (TOOLBAR_HEIGHT..TOOLBAR_HEIGHT + TAB_BAR_HEIGHT).contains(&my) {
                    let mut tab_x = 0.0f32;
                    for tab in &Tab::ALL {
                        let tab_w = tab_width(tab.label());
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

                // Process row click. `row_at` carries the bound the clip
                // already implied: a click below the last drawn row -- or in
                // the status bar under it -- selects nothing.
                if let Some(row_idx) = self.row_at(my) {
                    self.selected_index = Some(row_idx);
                    return EventResult::Consumed;
                }

                EventResult::Consumed
            }

            // Right click — context menu on process rows
            MouseEventKind::Press(MouseButton::Right) => {
                // The one that matters: this menu's actions include Kill, and
                // it was being opened over a process the pointer was nowhere
                // near whenever the click landed in the status bar.
                if let Some(row_idx) = self.row_at(my) {
                    self.selected_index = Some(row_idx);
                    if let Some(&proc_idx) = self.visible_indices.get(row_idx) {
                        let pid = self.processes.get(proc_idx).map(|p| p.pid).unwrap_or(0);
                        self.context_menu = Some(ContextMenu {
                            x: mx,
                            y: my,
                            target_pid: pid,
                            hover_index: None,
                        });
                    }
                }
                EventResult::Consumed
            }

            // Scroll wheel — scroll the process list
            MouseEventKind::Scroll { dy, .. } => {
                let rows = self.wheel.rows(*dy);
                self.scroll_offset = scroll_window::shift(self.scroll_offset, rows);
                // Stop with the last row at the *bottom*, not at the top. The
                // bound used to be `len - 1`, which let a thousand-process list
                // scroll until one row sat above forty blank ones. `len -
                // capacity` is the policy `scroll_window::visible` already
                // applies to what it draws, so the stored offset now agrees
                // with what the renderer would have shown anyway.
                let max_scroll = self
                    .visible_indices
                    .len()
                    .saturating_sub(self.visible_row_count().max(1));
                if self.scroll_offset > max_scroll {
                    self.scroll_offset = max_scroll;
                }
                EventResult::Consumed
            }

            // Mouse move — update hover state
            MouseEventKind::Move => {
                self.hovered_index = self.row_at(my);

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
                    let name = self
                        .processes
                        .get(idx)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    self.processes.remove(idx);
                    self.rebuild_visible_list();
                    self.status_message = format!("Killed {name} (PID {target_pid})");
                }
            }
            ContextAction::Pause => {
                if let Some(idx) = proc_idx
                    && let Some(proc) = self.processes.get_mut(idx)
                {
                    proc.status = ProcessStatus::Stopped;
                    self.status_message = format!("Paused {} (PID {target_pid})", proc.name);
                }
            }
            ContextAction::Resume => {
                if let Some(idx) = proc_idx
                    && let Some(proc) = self.processes.get_mut(idx)
                {
                    proc.status = ProcessStatus::Running;
                    self.status_message = format!("Resumed {} (PID {target_pid})", proc.name);
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

    /// The y of the first process row -- the column header's bottom edge.
    ///
    /// This used to be spelled `content_y + HEADER_HEIGHT` in four places:
    /// the three pointer paths and the renderer. Four copies of one number is
    /// four chances for three of them to be updated.
    const fn rows_top() -> f32 {
        TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + HEADER_HEIGHT
    }

    /// How tall the process-row area is, from `rows_top` to the status bar.
    ///
    /// The bottom edge existed in exactly one place -- the renderer's clip --
    /// while the top edge existed in four. That asymmetry is the bug: the
    /// clip stopped above the status bar and the hit tests did not, so a
    /// click in the status bar was resolved to whatever row the arithmetic
    /// extrapolated to below the last one drawn.
    fn rows_height(&self) -> f32 {
        (self.window_height as f32 - Self::rows_top() - STATUS_BAR_HEIGHT).max(0.0)
    }

    /// The process row under `my`, or `None` if the pointer is outside the
    /// row area or past the end of the list.
    ///
    /// The single bound for all three pointer paths *and* the renderer's
    /// clip. `my` is rejected above `rows_top`, at or below the status bar,
    /// and beyond the last row -- and non-finite coordinates are rejected
    /// outright rather than saturating through the `as usize` cast.
    fn row_at(&self, my: f32) -> Option<usize> {
        if self.active_tab != Tab::Processes {
            return None;
        }
        let offset = my - Self::rows_top();
        if !offset.is_finite() || offset < 0.0 || offset >= self.rows_height() {
            return None;
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let slot = (offset / ROW_HEIGHT) as usize;
        self.row_of_slot(slot)
    }

    /// The list index drawn in the `slot`-th visible row, or `None` past the
    /// end of the list.
    ///
    /// The renderer and the hit test both need this mapping, and the last
    /// place they could still disagree was here -- the renderer added the
    /// offset with `+`, which panics on overflow in a debug build and wraps
    /// in a release one, while the hit test used `checked_add`.
    fn row_of_slot(&self, slot: usize) -> Option<usize> {
        let row = self.scroll_offset.checked_add(slot)?;
        if row < self.visible_indices.len() {
            Some(row)
        } else {
            None
        }
    }

    /// Number of process rows visible in the current window.
    fn visible_row_count(&self) -> usize {
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let rows = (self.rows_height() / ROW_HEIGHT) as usize;
        rows
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
        self.render_bold_text(
            tree,
            bx + 8.0,
            btn_y + 6.0,
            "End Process",
            Color::WHITE,
            11.0,
        );
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
        let filter_border = if self.filter_focused {
            COLOR_ACCENT
        } else {
            Color::rgb(70, 75, 85)
        };
        tree.stroke_rect(filter_x, btn_y, filter_w, btn_h, filter_border, 1.0);
        tree.fill_rect(
            filter_x + 1.0,
            btn_y + 1.0,
            filter_w - 2.0,
            btn_h - 2.0,
            Color::rgb(25, 28, 34),
        );

        let filter_display = if self.filter_text.is_empty() {
            "Filter (Ctrl+F)"
        } else {
            &self.filter_text
        };
        let text_color = if self.filter_text.is_empty() {
            COLOR_TEXT_DIM
        } else {
            COLOR_TEXT
        };
        tree.text(
            filter_x + 8.0,
            btn_y + 6.0,
            filter_display,
            text_color,
            11.0,
        );

        // Cursor indicator when focused.
        if self.filter_focused {
            // The caret sits where the glyphs actually end: a byte count put
            // it a whole character past every non-ASCII filter.
            let cursor_x = filter_x + 8.0 + text::width(&self.filter_text, 11.0);
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
            let tab_w = tab_width(label);
            let is_active = *tab == self.active_tab;

            if is_active {
                tree.fill_rect(tx, y, tab_w, TAB_BAR_HEIGHT, COLOR_TOOLBAR_BG);
                // Active indicator line at bottom
                tree.fill_rect(tx, y + TAB_BAR_HEIGHT - 2.0, tab_w, 2.0, COLOR_TAB_ACTIVE);
            }

            let text_color = if is_active {
                COLOR_TEXT
            } else {
                COLOR_TEXT_DIM
            };
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
        // The column header's *top*; `rows_top()` below is its bottom, which
        // is where the rows -- and the hit test -- begin.
        let content_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;

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

            let label_color = if *col == self.sort_column {
                COLOR_ACCENT
            } else {
                COLOR_TEXT_DIM
            };
            // Fitted like the cells below it: the header gains a sort arrow
            // when it is the sort column, so its width is not the constant the
            // label alone suggests, and the separator is drawn at `cw - 1`.
            tree.text_in(
                col_x + CELL_PAD,
                content_y + 5.0,
                (cw - CELL_PAD * 2.0).max(0.0),
                &display,
                label_color,
                11.0,
            );

            // Column separator
            tree.fill_rect(
                col_x + cw - 1.0,
                content_y + 2.0,
                1.0,
                HEADER_HEIGHT - 4.0,
                Color::rgb(55, 60, 70),
            );
            col_x += cw;
        }

        // Process rows. Read from the same two helpers the hit test uses, so
        // the clip below *is* the region the pointer is accepted in rather
        // than a second opinion about it.
        let rows_y = Self::rows_top();
        let row_area_h = self.rows_height();
        let visible_rows = self.visible_row_count();

        tree.clip(0.0, rows_y, w, row_area_h);

        for vis_i in 0..visible_rows {
            let Some(row_idx) = self.row_of_slot(vis_i) else {
                break;
            };
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
            } else if row_idx.is_multiple_of(2) {
                COLOR_ROW_EVEN
            } else {
                COLOR_ROW_ODD
            };
            tree.fill_rect(0.0, ry, w, ROW_HEIGHT, bg);

            // Render each column cell
            let mut cx = 0.0f32;

            for col in &ProcessColumn::ALL {
                let cw = col.width();
                let indent = if self.view_mode == ViewMode::Tree && *col == ProcessColumn::Name {
                    proc.tree_depth as f32 * 16.0
                } else {
                    0.0
                };

                // Every cell is fitted to the column it belongs to. `proc.name`
                // and `proc.user` are supplied by the process being listed, so
                // their length is not ours to assume — unfitted, a process
                // named with 200 characters draws straight across Status, CPU
                // and Memory, and the row becomes unreadable for every process
                // *except* the one that did it.
                let cell_w = (cw - CELL_PAD * 2.0).max(0.0);
                match col {
                    ProcessColumn::Pid => {
                        tree.text_in(
                            cx + CELL_PAD,
                            ry + 4.0,
                            cell_w,
                            &proc.pid.to_string(),
                            COLOR_TEXT_DIM,
                            11.0,
                        );
                    }
                    ProcessColumn::Name => {
                        // Tree connector prefix
                        if self.view_mode == ViewMode::Tree && proc.tree_depth > 0 {
                            tree.text(
                                cx + CELL_PAD + indent - 14.0,
                                ry + 4.0,
                                "\u{2514}\u{2500}",
                                Color::rgb(80, 85, 95),
                                11.0,
                            );
                        }
                        // The tree indent eats into the name's room, so it has
                        // to come out of the width too; deep in a process tree
                        // the name is what gets shortened, not the layout.
                        tree.text_in(
                            cx + CELL_PAD + indent,
                            ry + 4.0,
                            (cell_w - indent).max(0.0),
                            &proc.name,
                            COLOR_TEXT,
                            11.0,
                        );
                    }
                    ProcessColumn::Status => {
                        tree.text_in(
                            cx + CELL_PAD,
                            ry + 4.0,
                            cell_w,
                            proc.status.label(),
                            proc.status.color(),
                            11.0,
                        );
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
                        tree.text_in(cx + CELL_PAD, ry + 4.0, cell_w, &cpu_str, cpu_color, 11.0);
                    }
                    ProcessColumn::Memory => {
                        tree.text_in(
                            cx + CELL_PAD,
                            ry + 4.0,
                            cell_w,
                            &format_bytes(proc.memory_bytes),
                            COLOR_TEXT,
                            11.0,
                        );
                    }
                    ProcessColumn::Threads => {
                        tree.text_in(
                            cx + CELL_PAD,
                            ry + 4.0,
                            cell_w,
                            &proc.thread_count.to_string(),
                            COLOR_TEXT_DIM,
                            11.0,
                        );
                    }
                    ProcessColumn::Priority => {
                        tree.text_in(
                            cx + CELL_PAD,
                            ry + 4.0,
                            cell_w,
                            &proc.priority.to_string(),
                            COLOR_TEXT_DIM,
                            11.0,
                        );
                    }
                    ProcessColumn::User => {
                        tree.text_in(
                            cx + CELL_PAD,
                            ry + 4.0,
                            cell_w,
                            &proc.user,
                            COLOR_TEXT_DIM,
                            11.0,
                        );
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
        tree.stroke_rect(
            graph_x,
            chart_y,
            graph_w,
            graph_h,
            Color::rgb(50, 55, 65),
            1.0,
        );

        // Grid lines (25%, 50%, 75%)
        for pct in &[25.0f32, 50.0, 75.0] {
            let gy = chart_y + graph_h * (1.0 - pct / 100.0);
            self.render_dashed_hline(tree, graph_x + 1.0, gy, graph_w - 2.0, COLOR_GRAPH_GRID);
            let pct_label = format!("{:.0}%", pct);
            tree.text(graph_x + 2.0, gy - 10.0, &pct_label, COLOR_TEXT_DIM, 9.0);
        }

        // CPU history line
        self.render_line_graph(
            tree,
            graph_x,
            chart_y,
            graph_w,
            graph_h,
            &self.cpu_history,
            COLOR_GRAPH_CPU,
            100.0,
        );

        let mut cur_y = chart_y + graph_h + section_gap;

        // -- Memory usage bars --
        self.render_bold_text(tree, graph_x, cur_y, "Memory", COLOR_TEXT, 13.0);
        cur_y += 20.0;

        let bar_h = 20.0;
        let bar_w = graph_w - 120.0;

        // Total / Used / Free / Cached
        let mem_items: &[(&str, u64, Color)] = &[
            (
                "Used",
                self.system_info.used_memory,
                Color::rgb(80, 140, 220),
            ),
            (
                "Cached",
                self.system_info.cached_memory,
                Color::rgb(120, 180, 80),
            ),
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
            let legend_x = graph_x
                + mem_items
                    .iter()
                    .position(|&(l, _, _)| l == label)
                    .unwrap_or(0) as f32
                    * 140.0;
            tree.fill_rect(legend_x, legend_y + 2.0, 10.0, 10.0, color);
            let legend_label = format!("{}: {}", label, format_bytes(amount));
            tree.text(
                legend_x + 14.0,
                legend_y,
                &legend_label,
                COLOR_TEXT_DIM,
                10.0,
            );
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
            &format!(
                "{} / {}",
                format_bytes(self.system_info.swap_used),
                format_bytes(self.system_info.swap_total)
            ),
            COLOR_TEXT_DIM,
            11.0,
        );
        cur_y += 24.0 + section_gap;

        // -- Per-CPU core bars --
        self.render_bold_text(
            tree,
            graph_x,
            cur_y,
            "Per-Core Utilization",
            COLOR_TEXT,
            13.0,
        );
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
        tree.text(
            graph_x,
            cur_y,
            &format!("Uptime: {uptime}"),
            COLOR_TEXT,
            12.0,
        );
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

        self.render_bold_text(
            tree,
            graph_x,
            content_y,
            "Network Bandwidth",
            COLOR_TEXT,
            13.0,
        );

        let chart_y = content_y + 20.0;
        tree.fill_rect(graph_x, chart_y, graph_w, graph_h, Color::rgb(20, 22, 28));
        tree.stroke_rect(
            graph_x,
            chart_y,
            graph_w,
            graph_h,
            Color::rgb(50, 55, 65),
            1.0,
        );

        // Determine max for scaling
        let max_bw = self
            .net_in_history
            .iter_oldest_first()
            .chain(self.net_out_history.iter_oldest_first())
            .fold(1.0f32, |acc, v| acc.max(v));

        self.render_line_graph(
            tree,
            graph_x,
            chart_y,
            graph_w,
            graph_h,
            &self.net_in_history,
            COLOR_GRAPH_NET_IN,
            max_bw,
        );
        self.render_line_graph(
            tree,
            graph_x,
            chart_y,
            graph_w,
            graph_h,
            &self.net_out_history,
            COLOR_GRAPH_NET_OUT,
            max_bw,
        );

        // Legend
        let legend_y = chart_y + graph_h + 4.0;
        tree.fill_rect(graph_x, legend_y + 2.0, 10.0, 10.0, COLOR_GRAPH_NET_IN);
        tree.text(graph_x + 14.0, legend_y, "In", COLOR_TEXT_DIM, 10.0);
        tree.fill_rect(
            graph_x + 50.0,
            legend_y + 2.0,
            10.0,
            10.0,
            COLOR_GRAPH_NET_OUT,
        );
        tree.text(graph_x + 64.0, legend_y, "Out", COLOR_TEXT_DIM, 10.0);

        // -- Connections table --
        let table_y = legend_y + 24.0;
        self.render_bold_text(
            tree,
            graph_x,
            table_y,
            "Active Connections",
            COLOR_TEXT,
            13.0,
        );

        let hdr_y = table_y + 20.0;
        tree.fill_rect(0.0, hdr_y, w, HEADER_HEIGHT, COLOR_HEADER_BG);

        // `Table` inserts a full gap before the first column, but this layout's
        // leading inset is a single `CELL_PAD` — matching the Processes tab —
        // rather than the doubled one that separates two adjacent columns. The
        // anchor absorbs the difference, so the first cell still starts at
        // `CELL_PAD` and the columns still fall on the old pitch.
        let net = Table::with_gap(NET_COLUMNS, -CELL_PAD, CELL_PAD * 2.0);
        // Regular, not bold: the Processes and Threads tables in this app mark
        // their headings by colour alone, and a bold row here would be the odd
        // one out.
        net.header_weighted(
            &mut tree.commands,
            hdr_y + 5.0,
            COLOR_TEXT_DIM,
            NET_FONT,
            FontWeightHint::Regular,
        );
        for i in 0..net.len() {
            // The separator sits on the pitch boundary, one `CELL_PAD` past the
            // right edge of the text the column may hold.
            tree.fill_rect(
                net.right(i) + CELL_PAD - 1.0,
                hdr_y + 2.0,
                1.0,
                HEADER_HEIGHT - 4.0,
                Color::rgb(55, 60, 70),
            );
        }

        // Connection rows
        let rows_y = hdr_y + HEADER_HEIGHT;
        let available_h = self.window_height as f32 - rows_y - STATUS_BAR_HEIGHT;
        let visible_rows = if available_h > 0.0 {
            (available_h / ROW_HEIGHT) as usize
        } else {
            0
        };

        tree.clip(0.0, rows_y, w, available_h);

        for (i, conn) in self.connections.iter().take(visible_rows).enumerate() {
            let ry = rows_y + i as f32 * ROW_HEIGHT;
            let bg = if i % 2 == 0 {
                COLOR_ROW_EVEN
            } else {
                COLOR_ROW_ODD
            };
            tree.fill_rect(0.0, ry, w, ROW_HEIGHT, bg);

            // An address is cut at the *front*. Its tail carries the port, and
            // the port is most of what a connection row is read for — `:443`
            // and `:22` are the difference between two rows that are otherwise
            // the same peer. Cut the usual way, every connection to one host
            // renders as one indistinguishable string with the port gone.
            // Each cell names the column it goes in rather than relying on its
            // position in this array. The old code took the width from
            // `net_cols[j]` and the value from `fields[j]`, two arrays that
            // happened to line up; naming the column means adding, removing or
            // reordering a field cannot silently shift every cell after it into
            // the wrong column.
            let pid = conn.pid.to_string();
            let fields: [(usize, &str, Fit); 6] = [
                (NET_PROTOCOL, conn.protocol.as_str(), Fit::Start),
                (NET_LOCAL_ADDR, conn.local_addr.as_str(), Fit::End),
                (NET_REMOTE_ADDR, conn.remote_addr.as_str(), Fit::End),
                (NET_STATE, conn.state.as_str(), Fit::Start),
                (NET_PID, pid.as_str(), Fit::Start),
                (NET_PROCESS, conn.process_name.as_str(), Fit::Start),
            ];
            debug_assert_eq!(
                fields.len(),
                net.len(),
                "a cell with no column is positioned past the table and drawn empty",
            );
            for (column, field, fit) in fields {
                let color = if column == NET_STATE {
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
                net.cell(
                    &mut tree.commands,
                    column,
                    ry + 4.0,
                    field,
                    color,
                    NET_FONT,
                    fit,
                );
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
                tree.text(
                    pad,
                    content_y + 20.0,
                    "No process selected. Select a process on the Processes tab.",
                    COLOR_TEXT_DIM,
                    13.0,
                );
                return;
            }
        };

        // -- Header --
        self.render_bold_text(
            tree,
            pad,
            content_y,
            &format!("{} (PID {})", proc.name, proc.pid),
            COLOR_TEXT,
            15.0,
        );

        let mut cur_y = content_y + 24.0;

        // -- Basic info grid --
        let info_items: &[(&str, String)] = &[
            ("PPID", proc.ppid.to_string()),
            ("Status", proc.status.label().to_string()),
            ("Priority", proc.priority.to_string()),
            ("User", proc.user.clone()),
            (
                "Start time",
                format!("{}s after boot", proc.start_time_secs),
            ),
            ("CPU time", format!("{}ms", proc.cpu_time_ms)),
            ("Threads", proc.thread_count.to_string()),
            ("CPU%", format!("{:.1}%", proc.cpu_percent)),
        ];

        let label_w = 90.0;
        let col_gap = 260.0;
        let info_cols = info_items.len().div_ceil(INFO_ROWS);

        for (i, (label, value)) in info_items.iter().enumerate() {
            let col = i / INFO_ROWS;
            let row = i % INFO_ROWS;
            let lx = pad + col as f32 * col_gap;
            let ly = cur_y + row as f32 * 18.0;

            // The value's room runs to the next column's label, or to the right
            // margin for the last column. `User` is the one that matters: it is
            // whatever name the process runs as, so an over-long one used to be
            // drawn straight across "CPU time" and "Threads" beside it.
            let next_x = if col + 1 < info_cols {
                pad + (col + 1) as f32 * col_gap
            } else {
                w - pad
            };
            let value_room = (next_x - INFO_GUTTER - (lx + label_w)).max(0.0);

            tree.text_in(
                lx,
                ly,
                (label_w - INFO_GUTTER).max(0.0),
                &format!("{label}:"),
                COLOR_TEXT_DIM,
                11.0,
            );
            tree.text_in(lx + label_w, ly, value_room, value, COLOR_TEXT, 11.0);
        }

        cur_y += INFO_ROWS as f32 * 18.0 + 12.0;

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
        // The three fields used to sit at a flat 200px pitch, which is a bound
        // on nothing: at 480px wide the third field started at 416 and ran to
        // 488, past the panel's own right margin at 464. A pitch that divides
        // the panel fills it at every width instead of at one.
        let mem_col_w = (w - 2.0 * pad) / mem_fields.len() as f32;
        for (i, (label, value)) in mem_fields.iter().enumerate() {
            let lx = pad + i as f32 * mem_col_w;
            tree.text_in(
                lx,
                cur_y,
                (mem_col_w - INFO_GUTTER).max(0.0),
                &format!("{label}: {value}"),
                COLOR_TEXT,
                11.0,
            );
        }
        cur_y += 22.0;

        // -- Command line --
        tree.fill_rect(pad, cur_y, w - 2.0 * pad, 1.0, Color::rgb(55, 60, 70));
        cur_y += 8.0;
        self.render_bold_text(tree, pad, cur_y, "Command Line", COLOR_TEXT, 12.0);
        cur_y += 18.0;
        let cmd_display = if proc.command_line.is_empty() {
            "(none)"
        } else {
            &proc.command_line
        };
        // A command line is arbitrarily long and comes from the process itself,
        // so it has to be fitted to the panel or it runs off the right edge of
        // the window entirely.
        tree.text_in(
            pad,
            cur_y,
            (w - 2.0 * pad).max(0.0),
            cmd_display,
            COLOR_TEXT_DIM,
            11.0,
        );
        cur_y += 22.0;

        // -- Thread list --
        tree.fill_rect(pad, cur_y, w - 2.0 * pad, 1.0, Color::rgb(55, 60, 70));
        cur_y += 8.0;
        self.render_bold_text(
            tree,
            pad,
            cur_y,
            &format!("Threads ({})", proc.threads.len()),
            COLOR_TEXT,
            12.0,
        );
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
            tree.text_in(
                tx + CELL_PAD,
                cur_y + 5.0,
                (col_w - CELL_PAD * 2.0).max(0.0),
                label,
                COLOR_TEXT_DIM,
                10.0,
            );
            tx += col_w;
        }
        cur_y += HEADER_HEIGHT;

        let max_thread_rows = 6;
        for (ti, thread) in proc.threads.iter().take(max_thread_rows).enumerate() {
            let ry = cur_y + ti as f32 * ROW_HEIGHT;
            let bg = if ti % 2 == 0 {
                COLOR_ROW_EVEN
            } else {
                COLOR_ROW_ODD
            };
            tree.fill_rect(pad, ry, w - 2.0 * pad, ROW_HEIGHT, bg);

            // The cells walk `thread_cols` alongside the header above, so a
            // column's width is written once. Previously each row restated
            // 60/200/80 as literals — they agreed with the header by
            // coincidence, and nothing would have caught them drifting apart.
            let cells: [(String, Color); 4] = [
                (thread.tid.to_string(), COLOR_TEXT_DIM),
                (thread.name.clone(), COLOR_TEXT),
                (thread.status.label().to_string(), thread.status.color()),
                (format!("{:.1}", thread.cpu_percent), COLOR_TEXT),
            ];
            let mut tcx = pad;
            for (&(_, col_w), (cell, color)) in thread_cols.iter().zip(cells.iter()) {
                tree.text_in(
                    tcx + CELL_PAD,
                    ry + 4.0,
                    (col_w - CELL_PAD * 2.0).max(0.0),
                    cell,
                    *color,
                    10.0,
                );
                tcx += col_w;
            }
        }
        cur_y += (proc.threads.len().min(max_thread_rows) as f32) * ROW_HEIGHT + 12.0;

        // -- Handles / capabilities --
        if !proc.handles.is_empty() {
            tree.fill_rect(pad, cur_y, w - 2.0 * pad, 1.0, Color::rgb(55, 60, 70));
            cur_y += 8.0;
            self.render_bold_text(
                tree,
                pad,
                cur_y,
                &format!("Handles ({})", proc.handles.len()),
                COLOR_TEXT,
                12.0,
            );
            cur_y += 18.0;

            let max_handles = 5;
            for handle in proc.handles.iter().take(max_handles) {
                let entry = format!(
                    "#{}: [{}] {}",
                    handle.handle_id, handle.resource_type, handle.description
                );
                // A handle description is a path or object name supplied by the
                // process, so it is fitted to the panel rather than left to run
                // off the right edge of the window.
                tree.text_in(
                    pad + DETAIL_INDENT,
                    cur_y,
                    (w - pad - DETAIL_INDENT - pad).max(0.0),
                    &entry,
                    COLOR_TEXT_DIM,
                    10.0,
                );
                cur_y += 16.0;
            }
            if proc.handles.len() > max_handles {
                let more = proc.handles.len() - max_handles;
                tree.text(
                    pad + 8.0,
                    cur_y,
                    &format!("... and {more} more"),
                    COLOR_TEXT_DIM,
                    10.0,
                );
                cur_y += 16.0;
            }
            cur_y += 8.0;
        }

        // -- Environment variables (collapsed summary) --
        if !proc.environment.is_empty() {
            tree.fill_rect(pad, cur_y, w - 2.0 * pad, 1.0, Color::rgb(55, 60, 70));
            cur_y += 8.0;
            self.render_bold_text(
                tree,
                pad,
                cur_y,
                &format!("Environment ({})", proc.environment.len()),
                COLOR_TEXT,
                12.0,
            );
            cur_y += 18.0;

            let max_env = 8;
            for (key, val) in proc.environment.iter().take(max_env) {
                let entry = format!("{key}={val}");
                // This used to be cut with `&entry[..77]` behind an
                // `if entry.len() > 80` guard. That guard was anti-protective:
                // it fired only for strings long enough in *bytes*, and a
                // non-Latin environment value reaches 80 bytes at ~27
                // characters, so it selected for exactly the entries whose byte
                // 77 is a continuation byte — and aborted the whole panel.
                //
                // A byte budget was never the right constraint either: 80 bytes
                // is unrelated to the pixels this panel has, which depend on the
                // window width. Fitting to the panel makes the two agree.
                tree.text_in(
                    pad + DETAIL_INDENT,
                    cur_y,
                    (w - pad - DETAIL_INDENT - pad).max(0.0),
                    &entry,
                    COLOR_TEXT_DIM,
                    10.0,
                );
                cur_y += 16.0;
            }
            if proc.environment.len() > max_env {
                let more = proc.environment.len() - max_env;
                tree.text(
                    pad + 8.0,
                    cur_y,
                    &format!("... and {more} more"),
                    COLOR_TEXT_DIM,
                    10.0,
                );
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
        tree.fill_rect(
            menu.x + 2.0,
            menu.y + 2.0,
            menu_w,
            menu_h,
            Color::rgba(0, 0, 0, 100),
        );

        // Background
        tree.fill_rect(menu.x, menu.y, menu_w, menu_h, Color::rgb(50, 54, 62));
        tree.stroke_rect(menu.x, menu.y, menu_w, menu_h, Color::rgb(80, 85, 95), 1.0);

        for (i, action) in ContextAction::ALL.iter().enumerate() {
            let iy = menu.y + i as f32 * item_h;

            if menu.hover_index == Some(i) {
                tree.fill_rect(
                    menu.x + 1.0,
                    iy,
                    menu_w - 2.0,
                    item_h,
                    Color::rgb(70, 100, 160),
                );
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
    // self + tree + area rect + history + color + max_value. Each is
    // independently needed for the graph render math.
    #[allow(clippy::too_many_arguments)]
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
            overflow: TextOverflow::Clip,
        });
    }

    // ========================================================================
    // Demo data (for development/testing)
    // ========================================================================

    /// Populate the explorer with sample data for UI testing.
    pub fn load_demo_data(&mut self) {
        self.processes = vec![
            make_demo_process(
                1,
                0,
                "init",
                ProcessStatus::Running,
                0.1,
                4_194_304,
                2,
                0,
                "root",
            ),
            make_demo_process(
                2,
                1,
                "kthread",
                ProcessStatus::Sleeping,
                0.0,
                0,
                1,
                -20,
                "root",
            ),
            make_demo_process(
                100,
                1,
                "compositor",
                ProcessStatus::Running,
                8.5,
                67_108_864,
                6,
                0,
                "system",
            ),
            make_demo_process(
                101,
                1,
                "netd",
                ProcessStatus::Sleeping,
                0.3,
                12_582_912,
                4,
                0,
                "system",
            ),
            make_demo_process(
                200,
                100,
                "desktop",
                ProcessStatus::Running,
                3.2,
                104_857_600,
                12,
                0,
                "user",
            ),
            make_demo_process(
                201,
                200,
                "explorer",
                ProcessStatus::Running,
                1.1,
                52_428_800,
                4,
                0,
                "user",
            ),
            make_demo_process(
                202,
                200,
                "terminal",
                ProcessStatus::Sleeping,
                0.4,
                20_971_520,
                3,
                0,
                "user",
            ),
            make_demo_process(
                203,
                200,
                "editor",
                ProcessStatus::Running,
                12.7,
                157_286_400,
                8,
                0,
                "user",
            ),
            make_demo_process(
                300,
                1,
                "httpd",
                ProcessStatus::Running,
                2.1,
                33_554_432,
                16,
                5,
                "www",
            ),
            make_demo_process(
                301,
                300,
                "httpd-worker",
                ProcessStatus::Running,
                5.4,
                16_777_216,
                1,
                5,
                "www",
            ),
            make_demo_process(
                302,
                300,
                "httpd-worker",
                ProcessStatus::Sleeping,
                0.0,
                16_777_216,
                1,
                5,
                "www",
            ),
            make_demo_process(
                400,
                1,
                "sshd",
                ProcessStatus::Sleeping,
                0.0,
                8_388_608,
                1,
                0,
                "root",
            ),
            make_demo_process(
                500,
                1,
                "zombie_proc",
                ProcessStatus::Zombie,
                0.0,
                0,
                0,
                0,
                "user",
            ),
        ];

        // Add threads and handles to a few processes.
        if let Some(compositor) = self.processes.iter_mut().find(|p| p.pid == 100) {
            compositor.threads = vec![
                ThreadInfo {
                    tid: 1001,
                    name: "render".to_string(),
                    status: ProcessStatus::Running,
                    cpu_percent: 5.0,
                },
                ThreadInfo {
                    tid: 1002,
                    name: "input".to_string(),
                    status: ProcessStatus::Sleeping,
                    cpu_percent: 1.0,
                },
                ThreadInfo {
                    tid: 1003,
                    name: "vsync".to_string(),
                    status: ProcessStatus::Sleeping,
                    cpu_percent: 2.5,
                },
            ];
            compositor.handles = vec![
                HandleInfo {
                    handle_id: 1,
                    resource_type: "channel".to_string(),
                    description: "desktop-ipc".to_string(),
                },
                HandleInfo {
                    handle_id: 2,
                    resource_type: "vmo".to_string(),
                    description: "framebuffer".to_string(),
                },
                HandleInfo {
                    handle_id: 3,
                    resource_type: "event".to_string(),
                    description: "vsync-signal".to_string(),
                },
            ];
            compositor.environment = vec![
                ("DISPLAY".to_string(), ":0".to_string()),
                ("GPU_DRIVER".to_string(), "virtio-gpu".to_string()),
            ];
            compositor.command_line =
                "/usr/bin/compositor --backend=virtio-gpu --vsync".to_string();
        }

        self.system_info = SystemInfo {
            total_memory: 8_589_934_592,  // 8 GiB
            used_memory: 3_435_973_837,   // ~3.2 GiB
            free_memory: 3_221_225_472,   // ~3 GiB
            cached_memory: 1_932_735_283, // ~1.8 GiB
            swap_total: 2_147_483_648,    // 2 GiB
            swap_used: 104_857_600,       // 100 MiB
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
            20.0, 25.0, 22.0, 30.0, 35.0, 28.0, 40.0, 38.0, 33.0, 36.0, 42.0, 38.0, 35.0, 30.0,
            28.0, 25.0, 30.0, 33.0, 37.0, 33.0,
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
// 9 args mirror the ProcessInfo fields one-to-one for demo construction.
#[allow(clippy::too_many_arguments)]
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
    guitk::bytes::iec(bytes)
}

/// Format an uptime in seconds as "Xd Xh Xm Xs".
///
/// Process Explorer and System Monitor both read `system_info.uptime_secs`
/// and can be open side by side on one desktop. They used to render it two
/// ways: this one kept the seconds field past a day, sysmonitor's dropped
/// it, so a machine up for 90 061 s was `1d 1h 1m 1s` here and `1d 1h 1m`
/// there. Both now use the exact shape, because these are the two windows a
/// person opens *specifically* to read exact numbers.
fn format_uptime(secs: u64) -> String {
    guitk::duration::units(secs)
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
    println!(
        "  {} visible (after filter)",
        explorer.visible_indices.len()
    );
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
        explorer
            .visible_indices
            .first()
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
    println!("Filter 'http': {} matches", explorer.visible_indices.len());

    // Demonstrate details tab.
    explorer.filter_text.clear();
    explorer.rebuild_visible_list();
    explorer.selected_index = Some(2); // compositor
    explorer.active_tab = Tab::Details;
    let details_tree = explorer.render();
    println!("Details tab: {} render commands", details_tree.len());

    println!("\nProcess Explorer ready.");
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use guitk::event::MouseEvent;

    // --- The wheel ---

    /// A list of `n` processes, already filtered so `visible_indices` is
    /// populated, with the window at its default height.
    fn app_with_processes(n: usize) -> ProcessExplorerState {
        let mut app = ProcessExplorerState::new();
        app.processes = (0..n)
            .map(|i| {
                make_demo_process(
                    (i as u32).saturating_add(1),
                    0,
                    &format!("proc{i}"),
                    ProcessStatus::Running,
                    0.0,
                    0,
                    1,
                    0,
                    "user",
                )
            })
            .collect();
        app.rebuild_visible_list();
        app
    }

    fn wheel(app: &mut ProcessExplorerState, dy: f32) {
        app.handle_mouse(&MouseEvent {
            x: 100.0,
            y: 300.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        });
    }

    /// One detent is three rows, in the direction the delta reports.
    #[test]
    fn one_wheel_notch_moves_three_rows() {
        let mut app = app_with_processes(500);
        wheel(&mut app, -1.0);
        assert_eq!(app.scroll_offset, 3);
        wheel(&mut app, 1.0);
        assert_eq!(app.scroll_offset, 0);
    }

    /// A precision device sends fractions of a notch. Reading only the sign --
    /// which is what this did -- moved three rows for each of them, so a
    /// trackpad ran five times too fast and could not move a single row.
    #[test]
    fn a_trackpads_fractions_add_up_instead_of_scrolling_five_times_too_far() {
        let mut app = app_with_processes(500);
        for _ in 0..5 {
            wheel(&mut app, -0.2);
        }
        assert_eq!(app.scroll_offset, 3);
    }

    /// The list stops with its last row at the *bottom* of the pane. The bound
    /// used to be `len - 1`, which let a long list scroll until one row sat
    /// above a screenful of nothing.
    #[test]
    fn scrolling_to_the_end_leaves_a_full_pane_of_rows() {
        let mut app = app_with_processes(500);
        let capacity = app.visible_row_count();
        assert!(capacity > 1, "the default window must fit several rows");
        for _ in 0..500 {
            wheel(&mut app, -1.0);
        }
        assert_eq!(app.scroll_offset, 500usize.saturating_sub(capacity));
    }

    /// A list shorter than the pane cannot scroll at all -- `len - capacity`
    /// must saturate rather than underflow.
    #[test]
    fn a_list_shorter_than_the_pane_does_not_scroll() {
        let mut app = app_with_processes(3);
        wheel(&mut app, -1.0);
        assert_eq!(app.scroll_offset, 0);
    }

    // --- The row area's edges ---

    fn press(app: &mut ProcessExplorerState, button: MouseButton, my: f32) {
        app.selected_index = None;
        app.context_menu = None;
        app.handle_mouse(&MouseEvent {
            x: 100.0,
            y: my,
            kind: MouseEventKind::Press(button),
        });
    }

    fn hover(app: &mut ProcessExplorerState, my: f32) {
        app.hovered_index = None;
        app.handle_mouse(&MouseEvent {
            x: 100.0,
            y: my,
            kind: MouseEventKind::Move,
        });
    }

    /// The rectangle the renderer actually clipped the process rows to.
    ///
    /// Read out of the emitted commands rather than recomputed from the
    /// constants. Recomputing is what makes a layout test worthless: it
    /// re-derives the renderer's arithmetic and then checks the hit test
    /// against *that*, so the two can drift together and the test still
    /// passes. This asks the renderer what it drew.
    fn rows_clip(app: &ProcessExplorerState) -> (f32, f32) {
        app.render()
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::PushClip { x, y, height, .. } if *x == 0.0 => Some((*y, *height)),
                _ => None,
            })
            .expect("the process rows are drawn under a clip")
    }

    #[test]
    // Exact equality is the assertion, not an approximation of it: the
    // renderer passes these two helpers' return values straight into the
    // clip, so anything short of bit-for-bit identity means a third copy of
    // the arithmetic has appeared -- which is the bug being pinned.
    #[allow(clippy::float_cmp)]
    fn the_lists_clip_is_the_region_the_hit_test_accepts() {
        let mut app = app_with_processes(500);
        let (clip_y, clip_h) = rows_clip(&app);
        assert_eq!(clip_y, ProcessExplorerState::rows_top());
        assert_eq!(clip_h, app.rows_height());

        hover(&mut app, clip_y);
        assert!(app.hovered_index.is_some(), "the clip's top edge is dead");
        hover(&mut app, clip_y + clip_h - 0.5);
        assert!(
            app.hovered_index.is_some(),
            "the clip's bottom edge is dead"
        );
        hover(&mut app, clip_y + clip_h);
        assert_eq!(
            app.hovered_index, None,
            "the hit test runs past the clip the rows are painted in"
        );
    }

    #[test]
    fn the_status_bar_does_not_select_a_process() {
        // The bug: the row hit test had no lower bound, so it ran under the
        // opaque status bar and out to the bottom of the window.
        let mut app = app_with_processes(500);
        let in_status_bar = app.window_height as f32 - (STATUS_BAR_HEIGHT / 2.0);
        press(&mut app, MouseButton::Left, in_status_bar);
        assert_eq!(app.selected_index, None);
    }

    #[test]
    fn the_status_bar_does_not_open_the_kill_menu() {
        // The one that matters: this menu's actions include killing the
        // process it was opened over, and it was being opened over a process
        // the pointer was nowhere near.
        let mut app = app_with_processes(500);
        let in_status_bar = app.window_height as f32 - (STATUS_BAR_HEIGHT / 2.0);
        press(&mut app, MouseButton::Right, in_status_bar);
        assert!(app.context_menu.is_none());
        assert_eq!(app.selected_index, None);
    }

    #[test]
    fn hovering_the_status_bar_leaves_no_row_lit() {
        let mut app = app_with_processes(500);
        let in_status_bar = app.window_height as f32 - (STATUS_BAR_HEIGHT / 2.0);
        hover(&mut app, in_status_bar);
        assert_eq!(app.hovered_index, None);
    }

    #[test]
    fn the_column_header_is_not_the_first_row() {
        // Above the rows, not below them. `rows_top` is the header's *bottom*,
        // so a click one pixel above it must sort, not select.
        let mut app = app_with_processes(500);
        press(
            &mut app,
            MouseButton::Left,
            ProcessExplorerState::rows_top() - 1.0,
        );
        assert_eq!(app.selected_index, None);
    }

    #[test]
    fn empty_space_below_a_short_list_selects_nothing() {
        // Inside the row area but past the end of the list. A different
        // rejection from the status-bar one: here the pointer is over a
        // legitimate part of the pane that simply has no row in it.
        let mut app = app_with_processes(3);
        let (clip_y, clip_h) = rows_clip(&app);
        let below_last = clip_y + 3.0 * ROW_HEIGHT + 1.0;
        assert!(below_last < clip_y + clip_h, "the pane must fit >3 rows");
        press(&mut app, MouseButton::Left, below_last);
        assert_eq!(app.selected_index, None);
        hover(&mut app, below_last);
        assert_eq!(app.hovered_index, None);
    }

    #[test]
    fn the_three_pointer_paths_agree_on_every_row_edge() {
        // Left click, right click and hover each had their own copy of the
        // arithmetic. Probe every row's top and bottom edge through all
        // three and require the same answer.
        let mut app = app_with_processes(500);
        let (clip_y, clip_h) = rows_clip(&app);
        let rows = app.visible_row_count();
        for slot in 0..rows {
            for probe in [
                clip_y + slot as f32 * ROW_HEIGHT,
                clip_y + (slot as f32 + 1.0) * ROW_HEIGHT - 0.5,
            ] {
                if probe >= clip_y + clip_h {
                    continue;
                }
                press(&mut app, MouseButton::Left, probe);
                let left = app.selected_index;
                press(&mut app, MouseButton::Right, probe);
                let right = app.selected_index;
                hover(&mut app, probe);
                let moved = app.hovered_index;
                assert_eq!(left, Some(slot), "left click at {probe}");
                assert_eq!(right, left, "right click disagrees at {probe}");
                assert_eq!(moved, left, "hover disagrees at {probe}");
            }
        }
    }

    #[test]
    fn a_scrolled_list_selects_the_row_it_draws() {
        // The offset must be added to the slot, not to a row index that
        // already includes it.
        let mut app = app_with_processes(500);
        wheel(&mut app, -4.0); // 12 rows
        assert_eq!(app.scroll_offset, 12);
        press(
            &mut app,
            MouseButton::Left,
            ProcessExplorerState::rows_top() + 0.5,
        );
        assert_eq!(app.selected_index, Some(12));
    }

    #[test]
    // `max(0.0)` returns a literal zero, so the comparison is exact.
    #[allow(clippy::float_cmp)]
    fn a_window_shorter_than_its_own_chrome_has_no_rows() {
        // `rows_height` clamps at zero rather than going negative, so every
        // probe is rejected by the bound rather than by an accident of sign.
        let mut app = app_with_processes(500);
        app.window_height = 10;
        assert_eq!(app.rows_height(), 0.0);
        assert_eq!(app.visible_row_count(), 0);
        press(&mut app, MouseButton::Left, 5.0);
        assert_eq!(app.selected_index, None);
    }

    #[test]
    fn a_nonfinite_coordinate_selects_nothing() {
        let mut app = app_with_processes(500);
        for y in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            press(&mut app, MouseButton::Left, y);
            assert_eq!(app.selected_index, None, "selected on {y}");
            hover(&mut app, y);
            assert_eq!(app.hovered_index, None, "hovered on {y}");
        }
    }

    #[test]
    fn another_tab_has_no_process_rows() {
        // Every path is gated on the Processes tab; `row_at` carries that
        // gate now instead of each caller repeating it.
        let mut app = app_with_processes(500);
        app.active_tab = Tab::System;
        let mid = ProcessExplorerState::rows_top() + ROW_HEIGHT / 2.0;
        press(&mut app, MouseButton::Left, mid);
        assert_eq!(app.selected_index, None);
        hover(&mut app, mid);
        assert_eq!(app.hovered_index, None);
    }

    // --- Table cells stay in their columns ---

    /// The left edge of each column, derived from the same widths the renderer
    /// uses so this cannot drift from it.
    fn column_edges() -> Vec<(ProcessColumn, f32, f32)> {
        let mut edges = Vec::new();
        let mut x = 0.0f32;
        for col in &ProcessColumn::ALL {
            edges.push((*col, x, col.width()));
            x += col.width();
        }
        edges
    }

    /// A process list whose name and user are far too long for their columns —
    /// the case a process can create for itself just by being named that way.
    fn app_with_a_shouting_process() -> ProcessExplorerState {
        let mut app = ProcessExplorerState::new();
        app.processes = vec![
            make_demo_process(
                1,
                0,
                &"W".repeat(180),
                ProcessStatus::Running,
                12.5,
                4096,
                3,
                0,
                &"averyveryverylongusernameindeed".repeat(3),
            ),
            make_demo_process(
                2,
                1,
                "init",
                ProcessStatus::Sleeping,
                0.0,
                2048,
                1,
                0,
                "root",
            ),
        ];
        app.rebuild_visible_list();
        app
    }

    /// Render *only* the process table, not the whole window.
    ///
    /// Rendering the app and filtering by position does not work here: the
    /// toolbar's "End Process" button sits at x=16, which is inside the PID
    /// column's x-range, so a whole-window render fails a column-fit assertion
    /// on a command that is not a table cell at all. The panel is what is under
    /// test, so the panel is what gets rendered.
    fn process_tab_commands(app: &ProcessExplorerState) -> Vec<RenderCommand> {
        let mut tree = RenderTree::new();
        app.render_process_tab(&mut tree);
        tree.commands
    }

    /// No cell may be drawn wider than the column it belongs to. Without this,
    /// a process can overwrite every column to its right just by having a long
    /// name — the row becomes unreadable for the processes that did nothing
    /// wrong, which is exactly backwards.
    #[test]
    fn no_process_row_cell_escapes_its_column() {
        let app = app_with_a_shouting_process();
        let cmds = process_tab_commands(&app);
        let edges = column_edges();

        let mut checked = 0;
        for cmd in &cmds {
            let RenderCommand::Text {
                x,
                text,
                font_size,
                font_weight,
                ..
            } = cmd
            else {
                continue;
            };
            // Find the column this command starts in.
            let Some(&(_, col_x, col_w)) =
                edges.iter().find(|&&(_, cx, cw)| *x >= cx && *x < cx + cw)
            else {
                continue;
            };
            let right = *x + text::measure(text, *font_size, *font_weight);
            assert!(
                right <= col_x + col_w + 0.5,
                "cell {text:?} starting at {x} runs to {right}, past its column's \
                 right edge {}",
                col_x + col_w,
            );
            checked += 1;
        }
        assert!(
            checked >= 8,
            "expected a full row of cells, checked {checked}"
        );
    }

    /// The cut has to be visible, or a truncated name is indistinguishable from
    /// a short one and the reader has no idea they are seeing a fragment.
    #[test]
    fn an_overlong_process_name_is_marked_as_cut() {
        let app = app_with_a_shouting_process();
        let cmds = process_tab_commands(&app);
        let cut: Vec<&String> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } if text.starts_with('W') => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(cut.len(), 1, "expected one name cell: {cut:?}");
        assert!(
            cut[0].ends_with('…'),
            "the truncation must be marked, got {:?}",
            cut[0],
        );
        assert!(
            cut[0].chars().count() < 180,
            "the name should have been shortened, got {} chars",
            cut[0].chars().count(),
        );
    }

    /// A name that fits is left exactly as it is — eliding is for text that
    /// genuinely does not fit, not a blanket shortening of every cell.
    #[test]
    fn a_short_process_name_is_left_alone() {
        let app = app_with_a_shouting_process();
        let cmds = process_tab_commands(&app);
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text == "init"
            )),
            "a short name must be drawn verbatim",
        );
    }

    // --- Network tab: connection rows stay in their columns ---

    /// The connection table's geometry, built the way the renderer builds it.
    fn net_table() -> Table<'static> {
        Table::with_gap(NET_COLUMNS, -CELL_PAD, CELL_PAD * 2.0)
    }

    /// A connection list of the shape a real machine produces and the 180px
    /// address columns were never sized for: IPv6 peers on one host, differing
    /// only in their port, plus a process name longer than its column.
    ///
    /// Everything on these rows arrives from outside — the peer chooses its own
    /// address, and the port is the only thing telling two of these rows apart.
    fn app_with_long_connections() -> ProcessExplorerState {
        let mut app = ProcessExplorerState::new();
        app.active_tab = Tab::Network;
        app.connections = vec![
            ConnectionInfo {
                protocol: String::from("TCP"),
                local_addr: String::from("2001:0db8:85a3:0000:0000:8a2e:0370:7334:41244"),
                remote_addr: String::from("2001:0db8:85a3:0000:0000:8a2e:0370:1111:443"),
                state: String::from("ESTABLISHED"),
                pid: 1234,
                process_name: String::from("a-service-with-a-very-long-name"),
            },
            ConnectionInfo {
                protocol: String::from("TCP"),
                local_addr: String::from("2001:0db8:85a3:0000:0000:8a2e:0370:7334:41245"),
                remote_addr: String::from("2001:0db8:85a3:0000:0000:8a2e:0370:1111:22"),
                state: String::from("ESTABLISHED"),
                pid: 1235,
                process_name: String::from("sshd"),
            },
            ConnectionInfo {
                protocol: String::from("UDP"),
                local_addr: String::from("0.0.0.0:53"),
                remote_addr: String::from("*:*"),
                state: String::from("LISTEN"),
                pid: 42,
                process_name: String::from("resolved"),
            },
        ];
        app
    }

    /// Render *only* the Network tab, for the reason given on
    /// [`process_tab_commands`]: chrome elsewhere in the window shares x-ranges
    /// with the table's columns.
    fn network_tab_texts(app: &ProcessExplorerState) -> Vec<(f32, String, f32, FontWeightHint)> {
        let mut tree = RenderTree::new();
        app.render_network_tab(&mut tree);
        tree.commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    text,
                    font_size,
                    font_weight,
                    ..
                } => Some((*x, text.clone(), *font_size, *font_weight)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn no_connection_row_cell_escapes_its_column() {
        let app = app_with_long_connections();
        let table = net_table();
        let spans = table.spans();
        let mut checked = 0usize;
        for (x, text, size, weight) in network_tab_texts(&app) {
            let Some((_, right)) = spans.iter().copied().find(|(l, _)| (l - x).abs() < 0.01) else {
                continue;
            };
            let drawn = x + text::measure(&text, size, weight);
            assert!(
                drawn <= right + 0.01,
                "cell {text:?} at {x} draws to {drawn}, past its column edge {right}"
            );
            checked = checked.saturating_add(1);
        }
        assert!(
            checked >= NET_COLUMNS.len() * 4,
            "expected a header and three rows, checked {checked}"
        );
    }

    #[test]
    fn an_overlong_address_keeps_the_port_that_identifies_it() {
        // The two IPv6 rows in the fixture share every character but the port.
        // Cut at the end they would both render as the same prefix with the
        // port gone — one string for two different connections.
        let app = app_with_long_connections();
        let table = net_table();
        let remote_x = table.left(NET_REMOTE_ADDR);
        let cut: Vec<String> = network_tab_texts(&app)
            .into_iter()
            .filter(|(x, t, ..)| (x - remote_x).abs() < 0.01 && t.starts_with('…'))
            .map(|(_, t, ..)| t)
            .collect();
        assert_eq!(
            cut.len(),
            2,
            "expected the two IPv6 peers to be cut: {cut:?}"
        );
        assert!(
            cut.iter().any(|t| t.ends_with(":443")),
            "the https port was lost: {cut:?}"
        );
        assert!(
            cut.iter().any(|t| t.ends_with(":22")),
            "the ssh port was lost: {cut:?}"
        );
        assert_ne!(cut[0], cut[1], "two peers rendered identically: {cut:?}");
    }

    #[test]
    fn a_short_value_is_drawn_verbatim_in_its_own_column() {
        // Two properties at once, because checking only the first is how a
        // misfiled cell survives: a value that fits must be drawn untouched,
        // *and* it must land in the column it belongs to. The old code indexed
        // the widths array separately from the fields array, so those two could
        // disagree with nothing to catch it.
        let app = app_with_long_connections();
        let table = net_table();
        let drawn = network_tab_texts(&app);
        for (column, expected) in [
            (NET_PROTOCOL, "UDP"),
            (NET_LOCAL_ADDR, "0.0.0.0:53"),
            (NET_REMOTE_ADDR, "*:*"),
            (NET_STATE, "LISTEN"),
            (NET_PID, "42"),
            (NET_PROCESS, "resolved"),
        ] {
            let x = table.left(column);
            assert!(
                drawn
                    .iter()
                    .any(|(cx, t, ..)| (cx - x).abs() < 0.01 && t == expected),
                "{expected:?} should be drawn as-is at column {column} (x={x}): {drawn:?}"
            );
        }
    }

    #[test]
    fn the_connection_header_and_rows_agree_on_where_a_column_starts() {
        let app = app_with_long_connections();
        let table = net_table();
        let lefts: Vec<f32> = (0..table.len()).map(|i| table.left(i)).collect();
        let mut seen = vec![0usize; table.len()];
        for (x, ..) in network_tab_texts(&app) {
            if let Some(i) = lefts.iter().position(|l| (l - x).abs() < 0.01) {
                seen[i] = seen[i].saturating_add(1);
            }
        }
        // One heading plus three rows in every column, all at the same x.
        for (i, count) in seen.iter().enumerate() {
            assert_eq!(*count, 4, "column {i} drew {count} texts at its left edge");
        }
    }

    #[test]
    fn the_connection_columns_still_fall_on_the_old_pitch() {
        // The conversion had to preserve the layout exactly: the table anchors
        // at `-CELL_PAD` precisely so the first cell still starts at `CELL_PAD`
        // and each separator still sits on a 70/180/180/100/60/140 boundary.
        let table = net_table();
        let pitches = [70.0f32, 180.0, 180.0, 100.0, 60.0, 140.0];
        assert!((table.left(NET_PROTOCOL) - CELL_PAD).abs() < 0.01);
        let mut boundary = 0.0f32;
        for (i, pitch) in pitches.iter().enumerate() {
            boundary += pitch;
            assert!(
                (table.right(i) + CELL_PAD - boundary).abs() < 0.01,
                "column {i} ends at {} + pad, not on the pitch boundary {boundary}",
                table.right(i)
            );
        }
    }

    #[test]
    fn tabs_fit_their_labels() {
        for tab in &Tab::ALL {
            assert!(
                tab_width(tab.label())
                    >= text::measure(tab.label(), 12.0, FontWeightHint::Regular) + 24.0,
                "{} overflows its tab",
                tab.label()
            );
        }
    }

    #[test]
    fn clicking_a_tab_selects_that_tab() {
        // Renderer and click handler share tab_width, so a click at the centre
        // of the nth tab must land on the nth tab whatever the labels are.
        let mut app = ProcessExplorerState::new();
        let mut tab_x = 0.0f32;
        for tab in &Tab::ALL {
            let w = tab_width(tab.label());
            app.handle_mouse(&MouseEvent {
                x: tab_x + w / 2.0,
                y: TOOLBAR_HEIGHT + TAB_BAR_HEIGHT / 2.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            });
            assert_eq!(app.active_tab, *tab, "click missed {}", tab.label());
            tab_x += w;
        }
    }

    #[test]
    fn the_filter_caret_follows_the_glyphs() {
        let mut app = ProcessExplorerState::new();
        app.filter_focused = true;
        app.filter_text = String::from("\u{fc}ber");
        let tree = app.render();
        let text_i = tree
            .commands
            .iter()
            .position(|c| matches!(c, RenderCommand::Text { text, .. } if *text == app.filter_text))
            .expect("filter text not drawn");
        let origin = match tree.commands.get(text_i) {
            Some(RenderCommand::Text { x, .. }) => *x,
            _ => unreachable!("just matched a Text command"),
        };
        let caret = tree
            .commands
            .iter()
            .skip(text_i)
            .find_map(|c| match c {
                RenderCommand::FillRect { x, width, .. } if (width - 1.0).abs() < 0.01 => Some(*x),
                _ => None,
            })
            .expect("no caret drawn");
        // A byte count would put the caret a whole character past the glyphs,
        // because the u-umlaut is two bytes.
        let expected = origin + text::width(&app.filter_text, 11.0);
        assert!(
            (caret - expected).abs() < 0.01,
            "caret at {caret}, glyphs end at {expected}"
        );
    }

    // -- Details tab: entries are fitted to the panel, not cut at a byte count -

    /// Environment entries whose byte 77 is deliberately mid-character.
    ///
    /// The `if entry.len() > 80 { &entry[..77] }` cut this replaced was
    /// anti-protective: it fired *only* on strings long enough in bytes, and a
    /// non-Latin value reaches 80 bytes at ~27 characters, so the guard
    /// selected for exactly the entries that would abort on the slice.
    fn adversarial_environment() -> Vec<(String, String)> {
        vec![
            (
                "LANG".to_string(),
                "\u{3053}\u{308c}\u{306f}\u{74b0}\u{5883}\u{5909}\u{6570}\u{306e}\u{5024}\u{3067}\u{3059}\u{3001}\u{30d0}\u{30a4}\u{30c8}\u{6570}\u{306f}\u{6587}\u{5b57}\u{6570}\u{306e}\u{4e09}\u{500d}\u{3042}\u{308a}\u{307e}\u{3059}\u{304b}\u{3089}\u{6ce8}\u{610f}".to_string(),
            ),
            (
                "\u{41f}\u{423}\u{422}\u{42c}".to_string(),
                "\u{42d}\u{442}\u{43e} \u{43e}\u{447}\u{435}\u{43d}\u{44c} \u{434}\u{43b}\u{438}\u{43d}\u{43d}\u{43e}\u{435} \u{437}\u{43d}\u{430}\u{447}\u{435}\u{43d}\u{438}\u{435} \u{43f}\u{435}\u{440}\u{435}\u{43c}\u{435}\u{43d}\u{43d}\u{43e}\u{439} \u{43e}\u{43a}\u{440}\u{443}\u{436}\u{435}\u{43d}\u{438}\u{44f}".to_string(),
            ),
            (
                "EMOJI".to_string(),
                "\u{1f4cc}\u{1f4dd}\u{1f5d2}\u{fe0f}\u{1f4a1}\u{1f9e0}\u{1f4da}\u{1f4c8}\u{1f4c9}\u{1f4ca}\u{1f5c3}\u{fe0f}\u{1f4c1}\u{1f4c2}\u{1f5df}\u{fe0f}\u{1f4cc}\u{1f4dd}\u{1f5d2}\u{fe0f}\u{1f4a1}\u{1f9e0}".to_string(),
            ),
            // `KEY=` is 4 bytes, so byte 77 of the entry lands on byte 73 of the
            // value — the middle of this U+00E9.
            (
                "KEY".to_string(),
                format!("{}\u{e9}{}", "v".repeat(72), "w".repeat(40)),
            ),
            ("SHORT".to_string(), "ok".to_string()),
        ]
    }

    /// A process whose every self-supplied string is far too long for the
    /// Details tab — the case a process can arrange for itself.
    fn app_with_a_shouting_details_tab(window_width: u32) -> ProcessExplorerState {
        let mut app = ProcessExplorerState::new();
        let mut proc = make_demo_process(
            1,
            0,
            "loud",
            ProcessStatus::Running,
            12.5,
            4096,
            3,
            0,
            // A user name is not the process's to choose, but it is not the
            // explorer's to trust either.
            &"\u{3068}\u{3066}\u{3082}\u{9577}\u{3044}\u{30e6}\u{30fc}\u{30b6}\u{30fc}\u{540d}"
                .repeat(4),
        );
        proc.environment = adversarial_environment();
        proc.handles = vec![
            HandleInfo {
                handle_id: 3,
                resource_type: "file".to_string(),
                description: format!("/very/deep/{}/leaf.txt", "directory".repeat(20)),
            },
            HandleInfo {
                handle_id: 4,
                resource_type: "\u{30c1}\u{30e3}\u{30cd}\u{30eb}".to_string(),
                description: "\u{3053}\u{308c}\u{306f}\u{975e}\u{5e38}\u{306b}\u{9577}\u{3044}\u{30cf}\u{30f3}\u{30c9}\u{30eb}\u{306e}\u{8aac}\u{660e}\u{6587}\u{3067}\u{3059}".repeat(3),
            },
            HandleInfo {
                handle_id: 5,
                resource_type: "sock".to_string(),
                description: "short".to_string(),
            },
        ];
        app.processes = vec![proc];
        app.window_width = window_width;
        app.rebuild_visible_list();
        app.selected_index = Some(0);
        app
    }

    fn details_tab_commands(app: &ProcessExplorerState) -> Vec<RenderCommand> {
        let mut tree = RenderTree::new();
        app.render_details_tab(&mut tree);
        tree.commands
    }

    #[test]
    fn a_non_ascii_environment_does_not_abort_the_details_tab() {
        let app = app_with_a_shouting_details_tab(960);
        assert!(!details_tab_commands(&app).is_empty());
    }

    /// Nothing the process supplies may be drawn past the panel's right margin,
    /// at any window width the user might drag to.
    #[test]
    fn no_details_text_escapes_the_panel() {
        let mut checked = 0usize;
        for width in [480_u32, 640, 960, 1440] {
            let app = app_with_a_shouting_details_tab(width);
            let right_margin = width as f32 - 16.0;
            for cmd in &details_tab_commands(&app) {
                let RenderCommand::Text {
                    x,
                    text,
                    font_size,
                    font_weight,
                    ..
                } = cmd
                else {
                    continue;
                };
                let right = x + text::measure(text, *font_size, *font_weight);
                assert!(
                    right <= right_margin + 0.5,
                    "at width {width} the entry {text:?} at {x} runs to {right}, \
                     past the panel margin {right_margin}"
                );
                checked += 1;
            }
        }
        assert!(
            checked >= 40,
            "expected a full details tab, checked {checked}"
        );
    }

    /// The info grid's two columns must not overlap: a long `User` may not be
    /// drawn across `CPU time` and `Threads` in the column beside it.
    #[test]
    fn the_info_grid_columns_do_not_overlap() {
        let app = app_with_a_shouting_details_tab(960);
        let second_col_x = 16.0 + 260.0;
        // The grid's own four rows, so the memory row below it — which has its
        // own, different column pitch — is not mistaken for a third column.
        let grid_top = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 8.0 + 24.0;
        let grid_bottom = grid_top + (INFO_ROWS - 1) as f32 * 18.0;
        let mut checked = 0usize;
        for cmd in &details_tab_commands(&app) {
            let RenderCommand::Text {
                x,
                y,
                text,
                font_size,
                font_weight,
                ..
            } = cmd
            else {
                continue;
            };
            if *y < grid_top - 0.5 || *y > grid_bottom + 0.5 {
                continue;
            }
            // Only the first info column starts left of the second column's x.
            if *x >= second_col_x {
                continue;
            }
            let right = x + text::measure(text, *font_size, *font_weight);
            assert!(
                right <= second_col_x + 0.5,
                "info cell {text:?} at {x} runs to {right}, into the second \
                 column at {second_col_x}"
            );
            checked += 1;
        }
        assert!(
            checked >= 8,
            "expected a full info column, checked {checked}"
        );
    }

    /// A short entry must survive untouched — an elide that fires when it did
    /// not need to would be just as wrong as one that never fires.
    #[test]
    fn a_short_details_entry_is_drawn_verbatim() {
        let app = app_with_a_shouting_details_tab(960);
        let cmds = details_tab_commands(&app);
        let drawn: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            drawn.contains(&"SHORT=ok"),
            "a short env entry was altered: {drawn:?}"
        );
        assert!(
            drawn.contains(&"#5: [sock] short"),
            "a short handle entry was altered: {drawn:?}"
        );
    }
}

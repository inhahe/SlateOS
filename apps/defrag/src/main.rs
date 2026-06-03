//! defrag -- OurOS Disk Defragmenter & Optimizer
//!
//! A visual disk defragmentation and optimization utility. Provides drive
//! analysis, block-level visualization, multiple defragmentation modes,
//! scheduling, file-level fragmentation views, and SSD detection with
//! TRIM support.
//!
//! # Architecture
//!
//! ```text
//! DriveInfo        -- drive metadata (filesystem, capacity, SSD flag)
//!      |
//!      v
//! analyze_drive()  -- scan block map, compute fragmentation stats
//!      |
//!      v
//! BlockMap         -- per-block state (free/used/fragmented/system)
//!      |
//!      v
//! DefragEngine     -- move blocks to defragment, track progress
//!      |
//!      v
//! DefragUI         -- full GUI with multiple views and controls
//! ```

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

#[allow(unused_imports)]
use std::collections::BTreeMap;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COLOR_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const COLOR_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_CRUST: Color = Color::from_hex(0x11111B);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_PEACH: Color = Color::from_hex(0xFAB387);
const COLOR_LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1060.0;
const WINDOW_HEIGHT: f32 = 760.0;
const TOOLBAR_HEIGHT: f32 = 48.0;
const SIDEBAR_WIDTH: f32 = 240.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const PADDING: f32 = 10.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const FONT_SIZE_TITLE: f32 = 18.0;
const ROW_HEIGHT: f32 = 26.0;
const BUTTON_WIDTH: f32 = 100.0;
const BUTTON_HEIGHT: f32 = 30.0;
const CORNER_RADIUS: f32 = 6.0;
const BLOCK_SIZE: f32 = 6.0;
const BLOCK_GAP: f32 = 1.0;
const LEGEND_HEIGHT: f32 = 32.0;
const TAB_HEIGHT: f32 = 34.0;
const PROGRESS_BAR_HEIGHT: f32 = 20.0;

// ============================================================================
// Block state
// ============================================================================

/// State of a single disk block in the visualization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockState {
    /// Block is free / unallocated.
    Free,
    /// Block is used and contiguous with its file.
    Contiguous,
    /// Block is used but fragmented (non-contiguous with neighbors).
    Fragmented,
    /// Block is used by system/metadata (e.g., filesystem journal, MFT).
    System,
    /// Block is currently being moved during defragmentation.
    Moving,
    /// Block is reserved / bad.
    Reserved,
}

impl BlockState {
    /// Color for this block state in the visualization.
    pub fn color(self) -> Color {
        match self {
            Self::Free => COLOR_SURFACE0,
            Self::Contiguous => COLOR_GREEN,
            Self::Fragmented => COLOR_RED,
            Self::System => COLOR_BLUE,
            Self::Moving => COLOR_YELLOW,
            Self::Reserved => COLOR_OVERLAY0,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Contiguous => "Contiguous",
            Self::Fragmented => "Fragmented",
            Self::System => "System",
            Self::Moving => "Moving",
            Self::Reserved => "Reserved",
        }
    }
}

// ============================================================================
// Block map
// ============================================================================

/// A map of all blocks on a drive.
#[derive(Clone, Debug)]
pub struct BlockMap {
    /// State of each block.
    pub blocks: Vec<BlockState>,
    /// Block size in bytes (e.g., 4096 for 4 KiB blocks).
    pub block_size_bytes: u64,
    /// Total number of blocks.
    pub total_blocks: u64,
}

impl BlockMap {
    /// Create a new block map with all blocks set to `Free`.
    pub fn new(total_blocks: u64, block_size_bytes: u64) -> Self {
        let count = total_blocks as usize;
        Self {
            blocks: vec![BlockState::Free; count],
            block_size_bytes,
            total_blocks,
        }
    }

    /// Set the state of a specific block. Returns `false` if index is out of bounds.
    pub fn set_block(&mut self, index: usize, state: BlockState) -> bool {
        if let Some(b) = self.blocks.get_mut(index) {
            *b = state;
            true
        } else {
            false
        }
    }

    /// Get the state of a specific block.
    pub fn get_block(&self, index: usize) -> Option<BlockState> {
        self.blocks.get(index).copied()
    }

    /// Set a range of blocks to the given state.
    pub fn set_range(&mut self, start: usize, count: usize, state: BlockState) {
        for i in start..start.saturating_add(count) {
            if let Some(b) = self.blocks.get_mut(i) {
                *b = state;
            }
        }
    }

    /// Count blocks in a given state.
    pub fn count_state(&self, state: BlockState) -> u64 {
        self.blocks.iter().filter(|&&b| b == state).count() as u64
    }

    /// Count total free blocks.
    pub fn free_blocks(&self) -> u64 {
        self.count_state(BlockState::Free)
    }

    /// Count total used blocks (contiguous + fragmented + system + moving).
    pub fn used_blocks(&self) -> u64 {
        self.blocks
            .iter()
            .filter(|&&b| b != BlockState::Free && b != BlockState::Reserved)
            .count() as u64
    }

    /// Find the largest contiguous run of free blocks.
    pub fn largest_free_region(&self) -> u64 {
        let mut max_run = 0u64;
        let mut current_run = 0u64;
        for &b in &self.blocks {
            if b == BlockState::Free {
                current_run = current_run.saturating_add(1);
                if current_run > max_run {
                    max_run = current_run;
                }
            } else {
                current_run = 0;
            }
        }
        max_run
    }

    /// Calculate fragmentation percentage.
    /// Fragmentation = fragmented_blocks / (fragmented_blocks + contiguous_blocks) * 100.
    pub fn fragmentation_percent(&self) -> f32 {
        let fragmented = self.count_state(BlockState::Fragmented);
        let contiguous = self.count_state(BlockState::Contiguous);
        let total_data = fragmented.saturating_add(contiguous);
        if total_data == 0 {
            return 0.0;
        }
        (fragmented as f64 / total_data as f64 * 100.0) as f32
    }

    /// Count the number of fragment boundaries (transitions from contiguous
    /// to fragmented or vice-versa that indicate file discontinuities).
    pub fn fragment_count(&self) -> u64 {
        let mut count = 0u64;
        let mut in_fragment = false;
        for &b in &self.blocks {
            if b == BlockState::Fragmented {
                if !in_fragment {
                    count = count.saturating_add(1);
                    in_fragment = true;
                }
            } else {
                in_fragment = false;
            }
        }
        count
    }
}

// ============================================================================
// Drive information
// ============================================================================

/// Filesystem type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesystemType {
    Ext4,
    Fat32,
    Ntfs,
    Btrfs,
    Xfs,
    Unknown,
}

impl FilesystemType {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Fat32 => "FAT32",
            Self::Ntfs => "NTFS",
            Self::Btrfs => "btrfs",
            Self::Xfs => "XFS",
            Self::Unknown => "Unknown",
        }
    }
}

/// Storage device type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageType {
    Hdd,
    Ssd,
    NvMe,
    Unknown,
}

impl StorageType {
    /// Whether this is a solid-state device where defrag is unnecessary.
    pub fn is_solid_state(self) -> bool {
        matches!(self, Self::Ssd | Self::NvMe)
    }

    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hdd => "HDD",
            Self::Ssd => "SSD",
            Self::NvMe => "NVMe",
            Self::Unknown => "Unknown",
        }
    }
}

/// Information about a single drive/partition.
#[derive(Clone, Debug)]
pub struct DriveInfo {
    /// Drive identifier (e.g., "/dev/sda1" or "C:").
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Filesystem type.
    pub fs_type: FilesystemType,
    /// Storage device type.
    pub storage_type: StorageType,
    /// Total capacity in bytes.
    pub total_bytes: u64,
    /// Used space in bytes.
    pub used_bytes: u64,
    /// Mount point.
    pub mount_point: String,
    /// Block size in bytes.
    pub block_size: u64,
}

impl DriveInfo {
    /// Free space in bytes.
    pub fn free_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.used_bytes)
    }

    /// Used space as a percentage.
    pub fn used_percent(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.used_bytes as f64 / self.total_bytes as f64 * 100.0) as f32
    }

    /// Whether this drive is an SSD (defrag not recommended).
    pub fn is_ssd(&self) -> bool {
        self.storage_type.is_solid_state()
    }
}

// ============================================================================
// File fragmentation info
// ============================================================================

/// Fragmentation details for a single file.
#[derive(Clone, Debug)]
pub struct FileFragInfo {
    /// File path.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Number of fragments (1 = contiguous, >1 = fragmented).
    pub fragment_count: u32,
    /// Number of blocks used by this file.
    pub block_count: u32,
    /// Whether this file has been excluded from defragmentation.
    pub excluded: bool,
}

impl FileFragInfo {
    /// Whether this file is fragmented.
    pub fn is_fragmented(&self) -> bool {
        self.fragment_count > 1
    }

    /// Fragmentation severity (higher = worse). 0 for contiguous files.
    pub fn severity(&self) -> f32 {
        if self.fragment_count <= 1 {
            return 0.0;
        }
        // Severity scales with both fragment count and file size.
        let frag_factor = (self.fragment_count.saturating_sub(1)) as f32;
        let size_factor = (self.size_bytes as f64 / (1024.0 * 1024.0)) as f32; // MiB
        frag_factor * (1.0 + size_factor.min(100.0) * 0.01)
    }
}

// ============================================================================
// Analysis results
// ============================================================================

/// Results of a drive fragmentation analysis.
#[derive(Clone, Debug)]
pub struct AnalysisResult {
    /// Fragmentation percentage (0-100).
    pub fragmentation_percent: f32,
    /// Total number of fragment boundaries.
    pub total_fragments: u64,
    /// Number of fragmented files.
    pub fragmented_file_count: u64,
    /// Total files analyzed.
    pub total_file_count: u64,
    /// Largest contiguous free region in blocks.
    pub largest_free_region_blocks: u64,
    /// Largest contiguous free region in bytes.
    pub largest_free_region_bytes: u64,
    /// Total free space in bytes.
    pub free_space_bytes: u64,
    /// Total used space in bytes.
    pub used_space_bytes: u64,
    /// Per-file fragmentation details, sorted by severity.
    pub file_details: Vec<FileFragInfo>,
    /// Block map of the drive.
    pub block_map: BlockMap,
}

/// Analyze a drive given its block map and file information.
pub fn analyze_drive(
    block_map: &BlockMap,
    files: &[FileFragInfo],
) -> AnalysisResult {
    let fragmentation_percent = block_map.fragmentation_percent();
    let total_fragments = block_map.fragment_count();
    let fragmented_file_count = files.iter().filter(|f| f.is_fragmented()).count() as u64;
    let total_file_count = files.len() as u64;
    let largest_free_region_blocks = block_map.largest_free_region();
    let largest_free_region_bytes = largest_free_region_blocks
        .saturating_mul(block_map.block_size_bytes);
    let free_space_bytes = block_map.free_blocks()
        .saturating_mul(block_map.block_size_bytes);
    let used_space_bytes = block_map.used_blocks()
        .saturating_mul(block_map.block_size_bytes);

    let mut file_details = files.to_vec();
    file_details.sort_by(|a, b| {
        b.severity()
            .partial_cmp(&a.severity())
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    AnalysisResult {
        fragmentation_percent,
        total_fragments,
        fragmented_file_count,
        total_file_count,
        largest_free_region_blocks,
        largest_free_region_bytes,
        free_space_bytes,
        used_space_bytes,
        file_details,
        block_map: block_map.clone(),
    }
}

// ============================================================================
// Optimization modes
// ============================================================================

/// Defragmentation optimization mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptimizationMode {
    /// Defragment only the most fragmented files.
    Quick,
    /// Defragment all fragmented files.
    Full,
    /// Consolidate free space into contiguous regions.
    FreeSpace,
    /// Prioritize boot and system files for fast startup.
    BootOptimize,
}

impl OptimizationMode {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Quick => "Quick",
            Self::Full => "Full",
            Self::FreeSpace => "Free Space",
            Self::BootOptimize => "Boot Optimize",
        }
    }

    /// Description for the UI.
    pub fn description(self) -> &'static str {
        match self {
            Self::Quick => "Defragment only the most fragmented files",
            Self::Full => "Defragment all fragmented files on the drive",
            Self::FreeSpace => "Consolidate free space into large contiguous regions",
            Self::BootOptimize => "Prioritize boot and system files for faster startup",
        }
    }

    /// All available modes.
    pub fn all() -> &'static [Self] {
        &[Self::Quick, Self::Full, Self::FreeSpace, Self::BootOptimize]
    }
}

// ============================================================================
// Schedule
// ============================================================================

/// Recurrence interval for scheduled defragmentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleInterval {
    Daily,
    Weekly,
    Monthly,
}

impl ScheduleInterval {
    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
        }
    }
}

/// Day of the week for scheduling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayOfWeek {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl DayOfWeek {
    pub fn label(self) -> &'static str {
        match self {
            Self::Monday => "Monday",
            Self::Tuesday => "Tuesday",
            Self::Wednesday => "Wednesday",
            Self::Thursday => "Thursday",
            Self::Friday => "Friday",
            Self::Saturday => "Saturday",
            Self::Sunday => "Sunday",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Monday,
            Self::Tuesday,
            Self::Wednesday,
            Self::Thursday,
            Self::Friday,
            Self::Saturday,
            Self::Sunday,
        ]
    }
}

/// A scheduled defragmentation configuration.
#[derive(Clone, Debug)]
pub struct DefragSchedule {
    /// Whether the schedule is enabled.
    pub enabled: bool,
    /// Drive to defragment.
    pub drive_id: String,
    /// How often to run.
    pub interval: ScheduleInterval,
    /// Preferred day of the week (for weekly schedules).
    pub preferred_day: DayOfWeek,
    /// Preferred hour of the day (0-23).
    pub preferred_hour: u8,
    /// Optimization mode to use.
    pub mode: OptimizationMode,
}

impl DefragSchedule {
    /// Create a new default schedule for the given drive.
    pub fn new(drive_id: &str) -> Self {
        Self {
            enabled: false,
            drive_id: drive_id.to_string(),
            interval: ScheduleInterval::Weekly,
            preferred_day: DayOfWeek::Sunday,
            preferred_hour: 2,
            mode: OptimizationMode::Full,
        }
    }

    /// Human-readable summary of the schedule.
    pub fn summary(&self) -> String {
        if !self.enabled {
            return "Disabled".to_string();
        }
        let time = format!("{:02}:00", self.preferred_hour.min(23));
        match self.interval {
            ScheduleInterval::Daily => {
                format!("Daily at {time} ({})", self.mode.label())
            }
            ScheduleInterval::Weekly => {
                format!(
                    "Weekly on {} at {time} ({})",
                    self.preferred_day.label(),
                    self.mode.label(),
                )
            }
            ScheduleInterval::Monthly => {
                format!("Monthly, 1st at {time} ({})", self.mode.label())
            }
        }
    }

    /// Whether this schedule should run at the given day/hour.
    pub fn should_run(&self, day: DayOfWeek, hour: u8, day_of_month: u8) -> bool {
        if !self.enabled {
            return false;
        }
        if hour != self.preferred_hour.min(23) {
            return false;
        }
        match self.interval {
            ScheduleInterval::Daily => true,
            ScheduleInterval::Weekly => day == self.preferred_day,
            ScheduleInterval::Monthly => day_of_month == 1,
        }
    }
}

// ============================================================================
// Exclude patterns
// ============================================================================

/// An exclusion rule for skipping files/directories during defrag.
#[derive(Clone, Debug)]
pub struct ExcludePattern {
    /// The pattern string (e.g., "/tmp/*", "*.log").
    pub pattern: String,
    /// Whether this exclusion is active.
    pub enabled: bool,
}

impl ExcludePattern {
    /// Create a new enabled exclusion pattern.
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
            enabled: true,
        }
    }

    /// Check whether a file path matches this exclusion pattern.
    /// Supports simple wildcards: `*` matches any sequence within a path component,
    /// leading `/` matches from root, trailing `/*` matches all children.
    pub fn matches(&self, path: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let pattern = &self.pattern;

        // Exact match.
        if pattern == path {
            return true;
        }

        // Prefix match with trailing `/*` (directory and all children).
        if let Some(prefix) = pattern.strip_suffix("/*") {
            if path.starts_with(prefix)
                && path.len() > prefix.len()
                && path.as_bytes().get(prefix.len()) == Some(&b'/')
            {
                return true;
            }
            // Also match the directory itself.
            if path == prefix {
                return true;
            }
        }

        // Extension wildcard `*.ext`.
        if let Some(ext) = pattern.strip_prefix("*.")
            && let Some(file_ext) = path.rsplit_once('.')
                && file_ext.1 == ext {
                    return true;
                }

        // Simple prefix match (directory prefix).
        if pattern.ends_with('/') && path.starts_with(pattern.as_str()) {
            return true;
        }

        false
    }
}

/// Check whether a path is excluded by any of the given patterns.
pub fn is_excluded(path: &str, patterns: &[ExcludePattern]) -> bool {
    patterns.iter().any(|p| p.matches(path))
}

// ============================================================================
// Defrag engine
// ============================================================================

/// State of the defragmentation process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefragState {
    /// No defrag running.
    Idle,
    /// Analyzing the drive.
    Analyzing,
    /// Actively defragmenting.
    Running,
    /// Paused by the user.
    Paused,
    /// Completed successfully.
    Completed,
    /// Stopped due to an error.
    Error,
}

/// Progress of the defragmentation.
#[derive(Clone, Debug)]
pub struct DefragProgress {
    /// Current state.
    pub state: DefragState,
    /// Blocks moved so far.
    pub blocks_moved: u64,
    /// Total blocks that need to be moved.
    pub total_blocks_to_move: u64,
    /// Files defragmented so far.
    pub files_completed: u64,
    /// Total files to defragment.
    pub total_files: u64,
    /// Elapsed time in seconds.
    pub elapsed_secs: u64,
    /// Estimated remaining time in seconds.
    pub estimated_remaining_secs: u64,
    /// Name of the file currently being processed.
    pub current_file: String,
    /// Fragmentation before defrag (percentage).
    pub initial_fragmentation: f32,
    /// Current fragmentation (percentage, decreasing as defrag progresses).
    pub current_fragmentation: f32,
    /// Error message if state is Error.
    pub error_message: String,
}

impl Default for DefragProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl DefragProgress {
    /// Create a new idle progress tracker.
    pub fn new() -> Self {
        Self {
            state: DefragState::Idle,
            blocks_moved: 0,
            total_blocks_to_move: 0,
            files_completed: 0,
            total_files: 0,
            elapsed_secs: 0,
            estimated_remaining_secs: 0,
            current_file: String::new(),
            initial_fragmentation: 0.0,
            current_fragmentation: 0.0,
            error_message: String::new(),
        }
    }

    /// Progress as a fraction (0.0 to 1.0).
    pub fn fraction(&self) -> f32 {
        if self.total_blocks_to_move == 0 {
            return 0.0;
        }
        (self.blocks_moved as f64 / self.total_blocks_to_move as f64) as f32
    }

    /// Progress as a percentage (0.0 to 100.0).
    pub fn percent(&self) -> f32 {
        self.fraction() * 100.0
    }

    /// Improvement percentage (how much fragmentation was reduced).
    pub fn improvement_percent(&self) -> f32 {
        if self.initial_fragmentation <= 0.0 {
            return 0.0;
        }
        let reduction = self.initial_fragmentation - self.current_fragmentation;
        (reduction / self.initial_fragmentation * 100.0).max(0.0)
    }
}

/// Defragmentation engine: simulates moving blocks to defragment a drive.
#[derive(Clone, Debug)]
pub struct DefragEngine {
    /// The block map being defragmented.
    pub block_map: BlockMap,
    /// Current progress.
    pub progress: DefragProgress,
    /// Optimization mode.
    pub mode: OptimizationMode,
    /// Excluded patterns.
    pub excludes: Vec<ExcludePattern>,
    /// Files on the drive.
    pub files: Vec<FileFragInfo>,
}

impl DefragEngine {
    /// Create a new engine for the given block map and files.
    pub fn new(
        block_map: BlockMap,
        files: Vec<FileFragInfo>,
        mode: OptimizationMode,
        excludes: Vec<ExcludePattern>,
    ) -> Self {
        let total_to_move = block_map.count_state(BlockState::Fragmented);
        let total_files = files.iter().filter(|f| f.is_fragmented()).count() as u64;

        let initial_fragmentation = block_map.fragmentation_percent();

        let mut progress = DefragProgress::new();
        progress.total_blocks_to_move = total_to_move;
        progress.total_files = total_files;
        progress.initial_fragmentation = initial_fragmentation;
        progress.current_fragmentation = initial_fragmentation;

        Self {
            block_map,
            progress,
            mode,
            excludes,
            files,
        }
    }

    /// Start the defragmentation process.
    pub fn start(&mut self) {
        self.progress.state = DefragState::Running;
    }

    /// Pause the defragmentation.
    pub fn pause(&mut self) {
        if self.progress.state == DefragState::Running {
            self.progress.state = DefragState::Paused;
        }
    }

    /// Resume a paused defragmentation.
    pub fn resume(&mut self) {
        if self.progress.state == DefragState::Paused {
            self.progress.state = DefragState::Running;
        }
    }

    /// Whether defrag can be started or resumed.
    pub fn can_run(&self) -> bool {
        matches!(
            self.progress.state,
            DefragState::Idle | DefragState::Paused
        )
    }

    /// Whether defrag is currently active (running or paused).
    pub fn is_active(&self) -> bool {
        matches!(
            self.progress.state,
            DefragState::Running | DefragState::Paused | DefragState::Analyzing
        )
    }

    /// Simulate one step of defragmentation: moves one fragmented block
    /// to a free block, making it contiguous.
    ///
    /// Returns `true` if work was done, `false` if nothing left to do.
    pub fn step(&mut self) -> bool {
        if self.progress.state != DefragState::Running {
            return false;
        }

        // Find the first fragmented block.
        let frag_idx = self.block_map.blocks.iter().position(|&b| b == BlockState::Fragmented);
        let Some(frag_idx) = frag_idx else {
            // No more fragmented blocks: done.
            self.progress.state = DefragState::Completed;
            self.progress.current_fragmentation = 0.0;
            return false;
        };

        // Find the first free block.
        let free_idx = self.block_map.blocks.iter().position(|&b| b == BlockState::Free);
        let Some(free_idx) = free_idx else {
            // No free space to work with: error.
            self.progress.state = DefragState::Error;
            self.progress.error_message = "No free space available".to_string();
            return false;
        };

        // Move: mark old position as free, new position as contiguous.
        self.block_map.set_block(frag_idx, BlockState::Free);
        self.block_map.set_block(free_idx, BlockState::Contiguous);

        self.progress.blocks_moved = self.progress.blocks_moved.saturating_add(1);
        self.progress.current_fragmentation = self.block_map.fragmentation_percent();

        // Check if we've completed all work.
        if self.block_map.count_state(BlockState::Fragmented) == 0 {
            self.progress.state = DefragState::Completed;
        }

        true
    }

    /// Run multiple defrag steps at once (batch processing).
    pub fn step_batch(&mut self, count: u64) -> u64 {
        let mut done = 0u64;
        for _ in 0..count {
            if self.step() {
                done = done.saturating_add(1);
            } else {
                break;
            }
        }
        done
    }

    /// Get files filtered by the current optimization mode and exclusions.
    pub fn files_to_process(&self) -> Vec<&FileFragInfo> {
        let mut files: Vec<&FileFragInfo> = self
            .files
            .iter()
            .filter(|f| {
                if !f.is_fragmented() {
                    return false;
                }
                if is_excluded(&f.path, &self.excludes) {
                    return false;
                }
                true
            })
            .collect();

        match self.mode {
            OptimizationMode::Quick => {
                // Only the most fragmented files (top 20%).
                files.sort_by(|a, b| {
                    b.severity()
                        .partial_cmp(&a.severity())
                        .unwrap_or(core::cmp::Ordering::Equal)
                });
                let top = (files.len() / 5).max(1);
                files.truncate(top);
            }
            OptimizationMode::Full => {
                // All fragmented files.
            }
            OptimizationMode::FreeSpace => {
                // No specific file targeting; we consolidate free space.
                files.clear();
            }
            OptimizationMode::BootOptimize => {
                // Prioritize boot/system files.
                files.sort_by(|a, b| {
                    let a_boot = is_boot_file(&a.path);
                    let b_boot = is_boot_file(&b.path);
                    b_boot.cmp(&a_boot)
                });
            }
        }

        files
    }
}

/// Check whether a file path is a boot/system file.
fn is_boot_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.starts_with("/boot/")
        || lower.starts_with("/system/")
        || lower.starts_with("/etc/")
        || lower.contains("/kernel")
        || lower.contains("/initrd")
        || lower.contains("/vmlinuz")
        || lower.ends_with(".sys")
        || lower.ends_with(".service")
}

// ============================================================================
// Statistics / comparison
// ============================================================================

/// Before/after statistics for a completed defragmentation.
#[derive(Clone, Debug)]
pub struct DefragStats {
    /// Fragmentation before defrag (percentage).
    pub before_fragmentation: f32,
    /// Fragmentation after defrag (percentage).
    pub after_fragmentation: f32,
    /// Total blocks moved.
    pub blocks_moved: u64,
    /// Total time elapsed in seconds.
    pub elapsed_secs: u64,
    /// Improvement percentage.
    pub improvement_percent: f32,
    /// Number of files defragmented.
    pub files_defragmented: u64,
    /// Free space gained (largest contiguous region, in bytes).
    pub largest_free_region_after: u64,
}

impl DefragStats {
    /// Create stats from a completed engine.
    pub fn from_engine(engine: &DefragEngine) -> Self {
        let before = engine.progress.initial_fragmentation;
        let after = engine.progress.current_fragmentation;
        let improvement = if before > 0.0 {
            ((before - after) / before * 100.0).max(0.0)
        } else {
            0.0
        };
        Self {
            before_fragmentation: before,
            after_fragmentation: after,
            blocks_moved: engine.progress.blocks_moved,
            elapsed_secs: engine.progress.elapsed_secs,
            improvement_percent: improvement,
            files_defragmented: engine.progress.files_completed,
            largest_free_region_after: engine
                .block_map
                .largest_free_region()
                .saturating_mul(engine.block_map.block_size_bytes),
        }
    }
}

// ============================================================================
// Size formatting
// ============================================================================

/// Format a byte count into a human-readable string.
pub fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    const TIB: u64 = 1024 * 1024 * 1024 * 1024;

    if bytes >= TIB {
        let val = bytes as f64 / TIB as f64;
        format!("{val:.2} TiB")
    } else if bytes >= GIB {
        let val = bytes as f64 / GIB as f64;
        format!("{val:.2} GiB")
    } else if bytes >= MIB {
        let val = bytes as f64 / MIB as f64;
        format!("{val:.2} MiB")
    } else if bytes >= KIB {
        let val = bytes as f64 / KIB as f64;
        format!("{val:.2} KiB")
    } else {
        format!("{bytes} B")
    }
}

/// Format a duration in seconds as "Xh Ym Zs".
fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

/// Format a percentage with one decimal place.
fn format_percent(pct: f32) -> String {
    format!("{pct:.1}%")
}

// ============================================================================
// Color helpers
// ============================================================================

/// Lighten a color by adding `amount` to each channel (clamped).
fn lighten_color(color: Color, amount: u8) -> Color {
    Color::rgba(
        color.r.saturating_add(amount),
        color.g.saturating_add(amount),
        color.b.saturating_add(amount),
        color.a,
    )
}

/// Darken a color by subtracting `amount` from each channel (clamped).
fn darken_color(color: Color, amount: u8) -> Color {
    Color::rgba(
        color.r.saturating_sub(amount),
        color.g.saturating_sub(amount),
        color.b.saturating_sub(amount),
        color.a,
    )
}

// ============================================================================
// View tabs
// ============================================================================

/// Which view the user is currently looking at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewTab {
    /// Visual block map of the drive.
    DiskMap,
    /// List of most fragmented files.
    FileList,
    /// Before/after statistics.
    Statistics,
    /// Scheduling configuration.
    Schedule,
}

impl ViewTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::DiskMap => "Disk Map",
            Self::FileList => "Files",
            Self::Statistics => "Statistics",
            Self::Schedule => "Schedule",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::DiskMap, Self::FileList, Self::Statistics, Self::Schedule]
    }
}

// ============================================================================
// File sort
// ============================================================================

/// Column for sorting the file list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSortColumn {
    Path,
    Size,
    Fragments,
    Severity,
}

/// Sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn toggle(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

/// Sort file fragmentation info by the given column and direction.
pub fn sort_file_list(
    files: &mut [FileFragInfo],
    column: FileSortColumn,
    direction: SortDirection,
) {
    files.sort_by(|a, b| {
        let cmp = match column {
            FileSortColumn::Path => a.path.cmp(&b.path),
            FileSortColumn::Size => a.size_bytes.cmp(&b.size_bytes),
            FileSortColumn::Fragments => a.fragment_count.cmp(&b.fragment_count),
            FileSortColumn::Severity => a
                .severity()
                .partial_cmp(&b.severity())
                .unwrap_or(core::cmp::Ordering::Equal),
        };
        match direction {
            SortDirection::Ascending => cmp,
            SortDirection::Descending => cmp.reverse(),
        }
    });
}

// ============================================================================
// Main UI state
// ============================================================================

/// Complete UI state for the disk defragmenter.
pub struct DefragUI {
    /// Available drives.
    pub drives: Vec<DriveInfo>,
    /// Index of the currently selected drive.
    pub selected_drive: usize,
    /// Current view tab.
    pub view_tab: ViewTab,
    /// Analysis result for the selected drive.
    pub analysis: Option<AnalysisResult>,
    /// Defragmentation engine (active when defrag is running).
    pub engine: Option<DefragEngine>,
    /// Completed defrag statistics (after defrag finishes).
    pub stats: Option<DefragStats>,
    /// Selected optimization mode.
    pub optimization_mode: OptimizationMode,
    /// Defrag schedule for the selected drive.
    pub schedule: DefragSchedule,
    /// Exclusion patterns.
    pub excludes: Vec<ExcludePattern>,
    /// File list sort column.
    pub file_sort_column: FileSortColumn,
    /// File list sort direction.
    pub file_sort_direction: SortDirection,
    /// Scroll offset for the file list.
    pub file_scroll_offset: f32,
    /// Whether the SSD warning dialog is shown.
    pub show_ssd_warning: bool,
    /// Whether the exclude editor is shown.
    pub show_exclude_editor: bool,
    /// Text input for new exclude pattern.
    pub exclude_input: String,
}

impl Default for DefragUI {
    fn default() -> Self {
        Self::new()
    }
}

impl DefragUI {
    /// Create a new UI with default state.
    pub fn new() -> Self {
        Self {
            drives: Vec::new(),
            selected_drive: 0,
            view_tab: ViewTab::DiskMap,
            analysis: None,
            engine: None,
            stats: None,
            optimization_mode: OptimizationMode::Full,
            schedule: DefragSchedule::new(""),
            excludes: vec![
                ExcludePattern::new("/tmp/*"),
                ExcludePattern::new("*.log"),
                ExcludePattern::new("/proc/*"),
                ExcludePattern::new("/sys/*"),
            ],
            file_sort_column: FileSortColumn::Severity,
            file_sort_direction: SortDirection::Descending,
            file_scroll_offset: 0.0,
            show_ssd_warning: false,
            show_exclude_editor: false,
            exclude_input: String::new(),
        }
    }

    /// Set the list of available drives.
    pub fn set_drives(&mut self, drives: Vec<DriveInfo>) {
        self.drives = drives;
        self.selected_drive = 0;
        if let Some(drive) = self.drives.first() {
            self.schedule = DefragSchedule::new(&drive.id);
        }
    }

    /// Select a drive by index.
    pub fn select_drive(&mut self, index: usize) {
        if index < self.drives.len() {
            self.selected_drive = index;
            self.analysis = None;
            self.engine = None;
            self.stats = None;
            if let Some(drive) = self.drives.get(index) {
                self.schedule = DefragSchedule::new(&drive.id);
            }
        }
    }

    /// Get the currently selected drive.
    pub fn current_drive(&self) -> Option<&DriveInfo> {
        self.drives.get(self.selected_drive)
    }

    /// Set the active view tab.
    pub fn set_view_tab(&mut self, tab: ViewTab) {
        self.view_tab = tab;
    }

    /// Load an analysis result (simulating a completed scan).
    pub fn load_analysis(&mut self, result: AnalysisResult) {
        self.analysis = Some(result);
    }

    /// Start defragmentation with the current settings.
    pub fn start_defrag(&mut self) {
        if let Some(analysis) = &self.analysis {
            // Check for SSD.
            if let Some(drive) = self.current_drive()
                && drive.is_ssd() {
                    self.show_ssd_warning = true;
                    return;
                }

            let mut engine = DefragEngine::new(
                analysis.block_map.clone(),
                analysis.file_details.clone(),
                self.optimization_mode,
                self.excludes.clone(),
            );
            engine.start();
            self.engine = Some(engine);
        }
    }

    /// Force-start defrag even on SSD (user dismissed warning).
    pub fn force_start_defrag(&mut self) {
        self.show_ssd_warning = false;
        if let Some(analysis) = &self.analysis {
            let mut engine = DefragEngine::new(
                analysis.block_map.clone(),
                analysis.file_details.clone(),
                self.optimization_mode,
                self.excludes.clone(),
            );
            engine.start();
            self.engine = Some(engine);
        }
    }

    /// Pause the running defragmentation.
    pub fn pause_defrag(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.pause();
        }
    }

    /// Resume a paused defragmentation.
    pub fn resume_defrag(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.resume();
        }
    }

    /// Run one step of defragmentation (call in event loop).
    pub fn defrag_step(&mut self) {
        let completed = if let Some(engine) = &mut self.engine {
            engine.step();
            engine.progress.state == DefragState::Completed
        } else {
            false
        };
        if completed
            && let Some(engine) = &self.engine {
                self.stats = Some(DefragStats::from_engine(engine));
            }
    }

    /// Run a batch of defrag steps.
    pub fn defrag_step_batch(&mut self, count: u64) {
        let completed = if let Some(engine) = &mut self.engine {
            engine.step_batch(count);
            engine.progress.state == DefragState::Completed
        } else {
            false
        };
        if completed
            && let Some(engine) = &self.engine {
                self.stats = Some(DefragStats::from_engine(engine));
            }
    }

    /// Set file sort column, toggling direction if same column.
    pub fn set_file_sort(&mut self, column: FileSortColumn) {
        if self.file_sort_column == column {
            self.file_sort_direction = self.file_sort_direction.toggle();
        } else {
            self.file_sort_column = column;
            self.file_sort_direction = SortDirection::Descending;
        }
    }

    /// Add an exclusion pattern.
    pub fn add_exclude(&mut self, pattern: &str) {
        if !pattern.is_empty() {
            self.excludes.push(ExcludePattern::new(pattern));
        }
    }

    /// Remove an exclusion pattern by index.
    pub fn remove_exclude(&mut self, index: usize) {
        if index < self.excludes.len() {
            self.excludes.remove(index);
        }
    }

    /// Toggle an exclusion pattern's enabled state.
    pub fn toggle_exclude(&mut self, index: usize) {
        if let Some(excl) = self.excludes.get_mut(index) {
            excl.enabled = !excl.enabled;
        }
    }

    /// Current defrag state, or Idle if no engine.
    pub fn defrag_state(&self) -> DefragState {
        self.engine
            .as_ref()
            .map(|e| e.progress.state)
            .unwrap_or(DefragState::Idle)
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the complete UI to a `RenderTree`.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: COLOR_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut tree);
        self.render_sidebar(&mut tree);
        self.render_tabs(&mut tree);

        let content_x = SIDEBAR_WIDTH;
        let content_y = TOOLBAR_HEIGHT + TAB_HEIGHT;
        let content_w = WINDOW_WIDTH - SIDEBAR_WIDTH;
        let content_h =
            WINDOW_HEIGHT - TOOLBAR_HEIGHT - TAB_HEIGHT - STATUS_BAR_HEIGHT;

        match self.view_tab {
            ViewTab::DiskMap => {
                self.render_disk_map(&mut tree, content_x, content_y, content_w, content_h);
            }
            ViewTab::FileList => {
                self.render_file_list(&mut tree, content_x, content_y, content_w, content_h);
            }
            ViewTab::Statistics => {
                self.render_statistics(&mut tree, content_x, content_y, content_w, content_h);
            }
            ViewTab::Schedule => {
                self.render_schedule(&mut tree, content_x, content_y, content_w, content_h);
            }
        }

        self.render_status_bar(&mut tree);

        if self.show_ssd_warning {
            self.render_ssd_warning(&mut tree);
        }

        tree
    }

    // -- toolbar ---------------------------------------------------------------

    fn render_toolbar(&self, tree: &mut RenderTree) {
        // Toolbar background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: TOOLBAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        tree.push(RenderCommand::Text {
            x: PADDING,
            y: 14.0,
            text: "Disk Defragmenter".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_TITLE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Action buttons on the right side
        let mut btn_x = WINDOW_WIDTH - PADDING;

        // Optimization mode selector
        for mode in OptimizationMode::all().iter().rev() {
            let label = mode.label();
            let btn_w = (label.len() as f32) * 8.0 + 16.0;
            btn_x -= btn_w + 4.0;
            let bg_color = if self.optimization_mode == *mode {
                COLOR_BLUE
            } else {
                COLOR_SURFACE0
            };
            let text_color = if self.optimization_mode == *mode {
                COLOR_CRUST
            } else {
                COLOR_SUBTEXT0
            };
            tree.push(RenderCommand::FillRect {
                x: btn_x,
                y: 9.0,
                width: btn_w,
                height: BUTTON_HEIGHT,
                color: bg_color,
                corner_radii: CornerRadii::all(4.0),
            });
            tree.push(RenderCommand::Text {
                x: btn_x + 8.0,
                y: 16.0,
                text: label.to_string(),
                color: text_color,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(btn_w - 16.0),
            });
        }

        // Analyze / Start / Pause / Resume button
        let action_label;
        let action_color;
        match self.defrag_state() {
            DefragState::Idle | DefragState::Completed | DefragState::Error => {
                if self.analysis.is_some() {
                    action_label = "Defragment";
                    action_color = COLOR_GREEN;
                } else {
                    action_label = "Analyze";
                    action_color = COLOR_BLUE;
                }
            }
            DefragState::Analyzing => {
                action_label = "Analyzing...";
                action_color = COLOR_OVERLAY0;
            }
            DefragState::Running => {
                action_label = "Pause";
                action_color = COLOR_YELLOW;
            }
            DefragState::Paused => {
                action_label = "Resume";
                action_color = COLOR_GREEN;
            }
        }

        let action_w = (action_label.len() as f32) * 8.0 + 20.0;
        let action_x = SIDEBAR_WIDTH + PADDING;
        tree.push(RenderCommand::FillRect {
            x: action_x,
            y: 9.0,
            width: action_w,
            height: BUTTON_HEIGHT,
            color: action_color,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: action_x + 10.0,
            y: 16.0,
            text: action_label.to_string(),
            color: COLOR_CRUST,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(action_w - 20.0),
        });
    }

    // -- sidebar ---------------------------------------------------------------

    fn render_sidebar(&self, tree: &mut RenderTree) {
        let sidebar_y = TOOLBAR_HEIGHT;
        let sidebar_h = WINDOW_HEIGHT - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Sidebar background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: sidebar_y,
            width: SIDEBAR_WIDTH,
            height: sidebar_h,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // "Drives" header
        tree.push(RenderCommand::Text {
            x: PADDING,
            y: sidebar_y + PADDING,
            text: "Drives".to_string(),
            color: COLOR_SUBTEXT1,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 2.0 * PADDING),
        });

        // Drive list
        let mut dy = sidebar_y + PADDING + 24.0;
        for (i, drive) in self.drives.iter().enumerate() {
            let is_selected = i == self.selected_drive;
            let row_color = if is_selected {
                COLOR_SURFACE0
            } else {
                COLOR_MANTLE
            };
            let row_h = 64.0;

            // Row background
            tree.push(RenderCommand::FillRect {
                x: 4.0,
                y: dy,
                width: SIDEBAR_WIDTH - 8.0,
                height: row_h,
                color: row_color,
                corner_radii: CornerRadii::all(4.0),
            });

            // Drive label
            tree.push(RenderCommand::Text {
                x: PADDING + 4.0,
                y: dy + 6.0,
                text: drive.label.clone(),
                color: if is_selected { COLOR_TEXT } else { COLOR_SUBTEXT0 },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(SIDEBAR_WIDTH - 2.0 * PADDING - 8.0),
            });

            // Drive info line (fs type + device type)
            tree.push(RenderCommand::Text {
                x: PADDING + 4.0,
                y: dy + 22.0,
                text: format!(
                    "{} | {} | {}",
                    drive.fs_type.label(),
                    drive.storage_type.label(),
                    drive.mount_point,
                ),
                color: COLOR_OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 2.0 * PADDING - 8.0),
            });

            // Usage bar
            let bar_y = dy + 38.0;
            let bar_w = SIDEBAR_WIDTH - 2.0 * PADDING - 8.0;
            let bar_h = 8.0;
            // Background
            tree.push(RenderCommand::FillRect {
                x: PADDING + 4.0,
                y: bar_y,
                width: bar_w,
                height: bar_h,
                color: COLOR_SURFACE1,
                corner_radii: CornerRadii::all(4.0),
            });
            // Used portion
            let used_w = bar_w * (drive.used_percent() / 100.0);
            let bar_color = if drive.used_percent() > 90.0 {
                COLOR_RED
            } else if drive.used_percent() > 70.0 {
                COLOR_YELLOW
            } else {
                COLOR_BLUE
            };
            if used_w > 0.5 {
                tree.push(RenderCommand::FillRect {
                    x: PADDING + 4.0,
                    y: bar_y,
                    width: used_w,
                    height: bar_h,
                    color: bar_color,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Size text
            tree.push(RenderCommand::Text {
                x: PADDING + 4.0,
                y: dy + 50.0,
                text: format!(
                    "{} / {}",
                    format_size(drive.used_bytes),
                    format_size(drive.total_bytes),
                ),
                color: COLOR_OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(bar_w),
            });

            dy += row_h + 4.0;
        }

        // SSD indicator for selected drive
        if let Some(drive) = self.current_drive()
            && drive.is_ssd() {
                dy += 8.0;
                tree.push(RenderCommand::FillRect {
                    x: 4.0,
                    y: dy,
                    width: SIDEBAR_WIDTH - 8.0,
                    height: 28.0,
                    color: Color::rgba(COLOR_YELLOW.r, COLOR_YELLOW.g, COLOR_YELLOW.b, 40),
                    corner_radii: CornerRadii::all(4.0),
                });
                tree.push(RenderCommand::Text {
                    x: PADDING + 4.0,
                    y: dy + 7.0,
                    text: "SSD - TRIM recommended".to_string(),
                    color: COLOR_YELLOW,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(SIDEBAR_WIDTH - 2.0 * PADDING - 8.0),
                });
            }
    }

    // -- tabs ------------------------------------------------------------------

    fn render_tabs(&self, tree: &mut RenderTree) {
        let tabs_y = TOOLBAR_HEIGHT;
        let tabs_x = SIDEBAR_WIDTH;
        let tabs_w = WINDOW_WIDTH - SIDEBAR_WIDTH;

        // Tab bar background
        tree.push(RenderCommand::FillRect {
            x: tabs_x,
            y: tabs_y,
            width: tabs_w,
            height: TAB_HEIGHT,
            color: COLOR_CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let mut tx = tabs_x + PADDING;
        for tab in ViewTab::all() {
            let label = tab.label();
            let is_active = self.view_tab == *tab;
            let tab_w = (label.len() as f32) * 8.0 + 24.0;

            if is_active {
                // Active tab highlight
                tree.push(RenderCommand::FillRect {
                    x: tx,
                    y: tabs_y,
                    width: tab_w,
                    height: TAB_HEIGHT,
                    color: COLOR_BASE,
                    corner_radii: CornerRadii::ZERO,
                });
                // Bottom accent
                tree.push(RenderCommand::FillRect {
                    x: tx,
                    y: tabs_y + TAB_HEIGHT - 2.0,
                    width: tab_w,
                    height: 2.0,
                    color: COLOR_BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            tree.push(RenderCommand::Text {
                x: tx + 12.0,
                y: tabs_y + 10.0,
                text: label.to_string(),
                color: if is_active { COLOR_TEXT } else { COLOR_OVERLAY0 },
                font_size: FONT_SIZE,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_w - 24.0),
            });

            tx += tab_w + 2.0;
        }
    }

    // -- disk map view ---------------------------------------------------------

    fn render_disk_map(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        // Progress bar (if defrag is active)
        let mut map_y = y + PADDING;

        if let Some(engine) = &self.engine {
            self.render_progress_bar(tree, x + PADDING, map_y, w - 2.0 * PADDING, engine);
            map_y += PROGRESS_BAR_HEIGHT + PADDING + 20.0;
        }

        // Block map visualization
        let block_map = if let Some(engine) = &self.engine {
            Some(&engine.block_map)
        } else {
            self.analysis.as_ref().map(|a| &a.block_map)
        };

        if let Some(bmap) = block_map {
            let map_w = w - 2.0 * PADDING;
            let map_h = h - (map_y - y) - LEGEND_HEIGHT - PADDING * 2.0;

            // Map background
            tree.push(RenderCommand::FillRect {
                x: x + PADDING,
                y: map_y,
                width: map_w,
                height: map_h,
                color: COLOR_CRUST,
                corner_radii: CornerRadii::all(4.0),
            });

            // Draw blocks
            let cell = BLOCK_SIZE + BLOCK_GAP;
            let cols = ((map_w - 4.0) / cell) as usize;
            if cols > 0 {
                let total = bmap.blocks.len();
                // Downsample if too many blocks.
                let step = if total > cols * ((map_h / cell) as usize) {
                    total / (cols * ((map_h / cell) as usize)).max(1)
                } else {
                    1
                };

                let mut bx = x + PADDING + 2.0;
                let mut by = map_y + 2.0;
                let mut idx = 0;

                while idx < total && by + BLOCK_SIZE < map_y + map_h {
                    let state = bmap.blocks.get(idx).copied().unwrap_or(BlockState::Free);
                    tree.push(RenderCommand::FillRect {
                        x: bx,
                        y: by,
                        width: BLOCK_SIZE,
                        height: BLOCK_SIZE,
                        color: state.color(),
                        corner_radii: CornerRadii::ZERO,
                    });

                    bx += cell;
                    if bx + BLOCK_SIZE > x + PADDING + map_w - 2.0 {
                        bx = x + PADDING + 2.0;
                        by += cell;
                    }
                    idx = idx.saturating_add(step);
                }
            }

            // Legend
            let legend_y = y + h - LEGEND_HEIGHT;
            let legend_items: &[(BlockState, &str)] = &[
                (BlockState::Free, "Free"),
                (BlockState::Contiguous, "Contiguous"),
                (BlockState::Fragmented, "Fragmented"),
                (BlockState::System, "System"),
                (BlockState::Moving, "Moving"),
                (BlockState::Reserved, "Reserved"),
            ];

            let mut lx = x + PADDING;
            for (state, label) in legend_items {
                // Color swatch
                tree.push(RenderCommand::FillRect {
                    x: lx,
                    y: legend_y + 8.0,
                    width: 12.0,
                    height: 12.0,
                    color: state.color(),
                    corner_radii: CornerRadii::all(2.0),
                });
                // Label
                tree.push(RenderCommand::Text {
                    x: lx + 16.0,
                    y: legend_y + 8.0,
                    text: label.to_string(),
                    color: COLOR_SUBTEXT0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                lx += (label.len() as f32) * 7.0 + 30.0;
            }
        } else {
            // No analysis yet
            tree.push(RenderCommand::Text {
                x: x + w / 2.0 - 120.0,
                y: y + h / 2.0,
                text: "Click Analyze to scan the drive".to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_HEADING,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Analysis summary (if available)
        if let Some(analysis) = &self.analysis {
            let summary_y = y + PADDING;
            let summary_x = x + w - 300.0;
            if self.engine.is_none() {
                // Only show summary when not defragging (progress bar takes this space)
                self.render_analysis_summary(tree, summary_x, summary_y, analysis);
            }
        }
    }

    fn render_analysis_summary(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        analysis: &AnalysisResult,
    ) {
        // Summary panel
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: 280.0,
            height: 120.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let lines = [
            (
                "Fragmentation:",
                format_percent(analysis.fragmentation_percent),
            ),
            ("Fragments:", format!("{}", analysis.total_fragments)),
            (
                "Fragmented files:",
                format!(
                    "{} / {}",
                    analysis.fragmented_file_count, analysis.total_file_count
                ),
            ),
            (
                "Largest free region:",
                format_size(analysis.largest_free_region_bytes),
            ),
            ("Free space:", format_size(analysis.free_space_bytes)),
        ];

        for (i, (label, value)) in lines.iter().enumerate() {
            let ly = y + 10.0 + i as f32 * 20.0;
            tree.push(RenderCommand::Text {
                x: x + 10.0,
                y: ly,
                text: label.to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
            });
            tree.push(RenderCommand::Text {
                x: x + 140.0,
                y: ly,
                text: value.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(130.0),
            });
        }
    }

    fn render_progress_bar(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        engine: &DefragEngine,
    ) {
        let progress = &engine.progress;

        // Progress bar background
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: PROGRESS_BAR_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Progress fill
        let fill_w = w * progress.fraction();
        if fill_w > 0.5 {
            let fill_color = match progress.state {
                DefragState::Running => COLOR_BLUE,
                DefragState::Paused => COLOR_YELLOW,
                DefragState::Completed => COLOR_GREEN,
                DefragState::Error => COLOR_RED,
                _ => COLOR_BLUE,
            };
            tree.push(RenderCommand::FillRect {
                x,
                y,
                width: fill_w,
                height: PROGRESS_BAR_HEIGHT,
                color: fill_color,
                corner_radii: CornerRadii::all(4.0),
            });
        }

        // Percentage text
        tree.push(RenderCommand::Text {
            x: x + w / 2.0 - 20.0,
            y: y + 3.0,
            text: format_percent(progress.percent()),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Status line below progress bar
        let status = match progress.state {
            DefragState::Running => format!(
                "Blocks moved: {} | Frag: {} | ETA: {}",
                progress.blocks_moved,
                format_percent(progress.current_fragmentation),
                format_duration(progress.estimated_remaining_secs),
            ),
            DefragState::Paused => "Paused".to_string(),
            DefragState::Completed => format!(
                "Complete! {} blocks moved, improvement: {}",
                progress.blocks_moved,
                format_percent(progress.improvement_percent()),
            ),
            DefragState::Error => format!("Error: {}", progress.error_message),
            _ => String::new(),
        };

        if !status.is_empty() {
            tree.push(RenderCommand::Text {
                x,
                y: y + PROGRESS_BAR_HEIGHT + 4.0,
                text: status,
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w),
            });
        }
    }

    // -- file list view --------------------------------------------------------

    fn render_file_list(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let files = if let Some(analysis) = &self.analysis {
            &analysis.file_details
        } else {
            // No analysis: show placeholder
            tree.push(RenderCommand::Text {
                x: x + w / 2.0 - 140.0,
                y: y + h / 2.0,
                text: "Analyze a drive to see file details".to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_HEADING,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        };

        // Header row
        let header_y = y;
        tree.push(RenderCommand::FillRect {
            x,
            y: header_y,
            width: w,
            height: ROW_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let col_defs: &[(&str, f32)] = &[
            ("File Path", x + PADDING),
            ("Size", x + w * 0.55),
            ("Fragments", x + w * 0.70),
            ("Severity", x + w * 0.85),
        ];
        for (label, cx) in col_defs {
            tree.push(RenderCommand::Text {
                x: *cx,
                y: header_y + 6.0,
                text: label.to_string(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // File rows
        let row_area_y = header_y + ROW_HEIGHT;
        let max_visible = ((h - ROW_HEIGHT) / ROW_HEIGHT) as usize;

        let mut sorted_files = files.to_vec();
        sort_file_list(
            &mut sorted_files,
            self.file_sort_column,
            self.file_sort_direction,
        );

        for (i, file) in sorted_files.iter().enumerate() {
            if i >= max_visible {
                break;
            }
            let ry = row_area_y + i as f32 * ROW_HEIGHT;

            // Alternating row background
            if i % 2 == 0 {
                tree.push(RenderCommand::FillRect {
                    x,
                    y: ry,
                    width: w,
                    height: ROW_HEIGHT,
                    color: COLOR_SURFACE0,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Excluded indicator
            let path_color = if is_excluded(&file.path, &self.excludes) {
                COLOR_OVERLAY0
            } else if file.is_fragmented() {
                COLOR_RED
            } else {
                COLOR_TEXT
            };

            // Path
            tree.push(RenderCommand::Text {
                x: x + PADDING,
                y: ry + 6.0,
                text: file.path.clone(),
                color: path_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w * 0.50),
            });

            // Size
            tree.push(RenderCommand::Text {
                x: x + w * 0.55,
                y: ry + 6.0,
                text: format_size(file.size_bytes),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Fragment count
            let frag_color = if file.fragment_count > 10 {
                COLOR_RED
            } else if file.fragment_count > 3 {
                COLOR_YELLOW
            } else {
                COLOR_SUBTEXT0
            };
            tree.push(RenderCommand::Text {
                x: x + w * 0.70,
                y: ry + 6.0,
                text: format!("{}", file.fragment_count),
                color: frag_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Severity
            let severity = file.severity();
            let sev_color = if severity > 10.0 {
                COLOR_RED
            } else if severity > 3.0 {
                COLOR_YELLOW
            } else {
                COLOR_GREEN
            };
            tree.push(RenderCommand::Text {
                x: x + w * 0.85,
                y: ry + 6.0,
                text: format!("{severity:.1}"),
                color: sev_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    // -- statistics view -------------------------------------------------------

    fn render_statistics(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let stats = if let Some(s) = &self.stats {
            s
        } else if let Some(engine) = &self.engine {
            // Show live stats from engine
            let progress = &engine.progress;
            let card_x = x + PADDING;
            let card_y = y + PADDING;
            let card_w = w - 2.0 * PADDING;

            tree.push(RenderCommand::FillRect {
                x: card_x,
                y: card_y,
                width: card_w,
                height: 200.0,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            tree.push(RenderCommand::Text {
                x: card_x + PADDING,
                y: card_y + PADDING,
                text: "Live Statistics".to_string(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE_HEADING,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            let live_lines: &[(&str, String)] = &[
                (
                    "Initial fragmentation:",
                    format_percent(progress.initial_fragmentation),
                ),
                (
                    "Current fragmentation:",
                    format_percent(progress.current_fragmentation),
                ),
                ("Blocks moved:", format!("{}", progress.blocks_moved)),
                ("Elapsed:", format_duration(progress.elapsed_secs)),
                (
                    "Improvement:",
                    format_percent(progress.improvement_percent()),
                ),
            ];

            for (i, (label, value)) in live_lines.iter().enumerate() {
                let ly = card_y + 36.0 + i as f32 * 24.0;
                tree.push(RenderCommand::Text {
                    x: card_x + PADDING,
                    y: ly,
                    text: label.to_string(),
                    color: COLOR_SUBTEXT0,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                });
                tree.push(RenderCommand::Text {
                    x: card_x + 220.0,
                    y: ly,
                    text: value.clone(),
                    color: COLOR_TEXT,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(card_w - 240.0),
                });
            }
            return;
        } else {
            // No stats at all
            tree.push(RenderCommand::Text {
                x: x + w / 2.0 - 120.0,
                y: y + h / 2.0,
                text: "No statistics yet".to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_HEADING,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        };

        // Completed stats display
        let card_x = x + PADDING;
        let card_y = y + PADDING;
        let card_w = w - 2.0 * PADDING;

        // Before/After comparison card
        tree.push(RenderCommand::FillRect {
            x: card_x,
            y: card_y,
            width: card_w,
            height: 260.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        tree.push(RenderCommand::Text {
            x: card_x + PADDING,
            y: card_y + PADDING,
            text: "Defragmentation Results".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Before/After bars
        let bar_x = card_x + 180.0;
        let bar_max_w = card_w - 220.0;

        // Before
        let before_y = card_y + 44.0;
        tree.push(RenderCommand::Text {
            x: card_x + PADDING,
            y: before_y + 2.0,
            text: "Before:".to_string(),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(150.0),
        });
        tree.push(RenderCommand::FillRect {
            x: bar_x,
            y: before_y,
            width: bar_max_w,
            height: 18.0,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        let before_w = bar_max_w * (stats.before_fragmentation / 100.0).min(1.0);
        if before_w > 0.5 {
            tree.push(RenderCommand::FillRect {
                x: bar_x,
                y: before_y,
                width: before_w,
                height: 18.0,
                color: COLOR_RED,
                corner_radii: CornerRadii::all(3.0),
            });
        }
        tree.push(RenderCommand::Text {
            x: bar_x + bar_max_w + 8.0,
            y: before_y + 2.0,
            text: format_percent(stats.before_fragmentation),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // After
        let after_y = before_y + 30.0;
        tree.push(RenderCommand::Text {
            x: card_x + PADDING,
            y: after_y + 2.0,
            text: "After:".to_string(),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(150.0),
        });
        tree.push(RenderCommand::FillRect {
            x: bar_x,
            y: after_y,
            width: bar_max_w,
            height: 18.0,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        let after_w = bar_max_w * (stats.after_fragmentation / 100.0).min(1.0);
        if after_w > 0.5 {
            tree.push(RenderCommand::FillRect {
                x: bar_x,
                y: after_y,
                width: after_w,
                height: 18.0,
                color: COLOR_GREEN,
                corner_radii: CornerRadii::all(3.0),
            });
        }
        tree.push(RenderCommand::Text {
            x: bar_x + bar_max_w + 8.0,
            y: after_y + 2.0,
            text: format_percent(stats.after_fragmentation),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Detail lines
        let detail_lines: &[(&str, String)] = &[
            (
                "Improvement:",
                format_percent(stats.improvement_percent),
            ),
            ("Blocks moved:", format!("{}", stats.blocks_moved)),
            ("Time elapsed:", format_duration(stats.elapsed_secs)),
            (
                "Files defragmented:",
                format!("{}", stats.files_defragmented),
            ),
            (
                "Largest free region:",
                format_size(stats.largest_free_region_after),
            ),
        ];

        for (i, (label, value)) in detail_lines.iter().enumerate() {
            let ly = after_y + 36.0 + i as f32 * 24.0;
            tree.push(RenderCommand::Text {
                x: card_x + PADDING,
                y: ly,
                text: label.to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(160.0),
            });
            tree.push(RenderCommand::Text {
                x: card_x + 180.0,
                y: ly,
                text: value.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(card_w - 200.0),
            });
        }
    }

    // -- schedule view ---------------------------------------------------------

    fn render_schedule(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        _h: f32,
    ) {
        let card_x = x + PADDING;
        let card_y = y + PADDING;
        let card_w = w - 2.0 * PADDING;

        // Schedule card
        tree.push(RenderCommand::FillRect {
            x: card_x,
            y: card_y,
            width: card_w,
            height: 220.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        tree.push(RenderCommand::Text {
            x: card_x + PADDING,
            y: card_y + PADDING,
            text: "Defrag Schedule".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Enabled toggle
        let toggle_y = card_y + 40.0;
        let toggle_color = if self.schedule.enabled {
            COLOR_GREEN
        } else {
            COLOR_SURFACE1
        };
        tree.push(RenderCommand::FillRect {
            x: card_x + PADDING,
            y: toggle_y,
            width: 44.0,
            height: 22.0,
            color: toggle_color,
            corner_radii: CornerRadii::all(11.0),
        });
        // Toggle knob
        let knob_x = if self.schedule.enabled {
            card_x + PADDING + 24.0
        } else {
            card_x + PADDING + 2.0
        };
        tree.push(RenderCommand::FillRect {
            x: knob_x,
            y: toggle_y + 2.0,
            width: 18.0,
            height: 18.0,
            color: COLOR_TEXT,
            corner_radii: CornerRadii::all(9.0),
        });
        tree.push(RenderCommand::Text {
            x: card_x + PADDING + 52.0,
            y: toggle_y + 3.0,
            text: if self.schedule.enabled {
                "Enabled"
            } else {
                "Disabled"
            }
            .to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Schedule details
        let detail_y = toggle_y + 34.0;
        let sched_lines: &[(&str, String)] = &[
            ("Interval:", self.schedule.interval.label().to_string()),
            ("Day:", self.schedule.preferred_day.label().to_string()),
            (
                "Time:",
                format!("{:02}:00", self.schedule.preferred_hour.min(23)),
            ),
            ("Mode:", self.schedule.mode.label().to_string()),
            ("Summary:", self.schedule.summary()),
        ];

        for (i, (label, value)) in sched_lines.iter().enumerate() {
            let ly = detail_y + i as f32 * 24.0;
            tree.push(RenderCommand::Text {
                x: card_x + PADDING,
                y: ly,
                text: label.to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });
            tree.push(RenderCommand::Text {
                x: card_x + 120.0,
                y: ly,
                text: value.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(card_w - 140.0),
            });
        }

        // Exclude patterns card
        let excl_y = card_y + 240.0;
        tree.push(RenderCommand::FillRect {
            x: card_x,
            y: excl_y,
            width: card_w,
            height: 40.0 + self.excludes.len() as f32 * 24.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        tree.push(RenderCommand::Text {
            x: card_x + PADDING,
            y: excl_y + PADDING,
            text: "Exclude Patterns".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        for (i, excl) in self.excludes.iter().enumerate() {
            let ey = excl_y + 34.0 + i as f32 * 24.0;
            let text_color = if excl.enabled {
                COLOR_TEXT
            } else {
                COLOR_OVERLAY0
            };
            // Enabled indicator
            tree.push(RenderCommand::FillRect {
                x: card_x + PADDING,
                y: ey + 2.0,
                width: 14.0,
                height: 14.0,
                color: if excl.enabled {
                    COLOR_GREEN
                } else {
                    COLOR_SURFACE1
                },
                corner_radii: CornerRadii::all(2.0),
            });
            // Pattern text
            tree.push(RenderCommand::Text {
                x: card_x + PADDING + 22.0,
                y: ey + 2.0,
                text: excl.pattern.clone(),
                color: text_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(card_w - PADDING * 2.0 - 40.0),
            });
        }
    }

    // -- SSD warning dialog ----------------------------------------------------

    fn render_ssd_warning(&self, tree: &mut RenderTree) {
        // Overlay
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        // Dialog box
        let dw = 420.0;
        let dh = 180.0;
        let dx = (WINDOW_WIDTH - dw) / 2.0;
        let dy = (WINDOW_HEIGHT - dh) / 2.0;

        tree.push(RenderCommand::BoxShadow {
            x: dx,
            y: dy,
            width: dw,
            height: dh,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 20.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        tree.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dw,
            height: dh,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Warning title
        tree.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "SSD Detected".to_string(),
            color: COLOR_YELLOW,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dw - 2.0 * PADDING),
        });

        // Warning text
        tree.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 40.0,
            text: "Defragmentation is not recommended for SSDs.".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dw - 2.0 * PADDING),
        });
        tree.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 58.0,
            text: "It can reduce SSD lifespan without performance benefit.".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dw - 2.0 * PADDING),
        });
        tree.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 76.0,
            text: "Consider using TRIM instead.".to_string(),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dw - 2.0 * PADDING),
        });

        // Buttons
        let cancel_x = dx + dw - 2.0 * BUTTON_WIDTH - PADDING * 2.0 - 4.0;
        let proceed_x = dx + dw - BUTTON_WIDTH - PADDING;

        // Cancel button
        tree.push(RenderCommand::FillRect {
            x: cancel_x,
            y: dy + dh - BUTTON_HEIGHT - PADDING,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: cancel_x + 28.0,
            y: dy + dh - BUTTON_HEIGHT - PADDING + 8.0,
            text: "Cancel".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(BUTTON_WIDTH - 10.0),
        });

        // Proceed anyway button
        tree.push(RenderCommand::FillRect {
            x: proceed_x,
            y: dy + dh - BUTTON_HEIGHT - PADDING,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_RED,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: proceed_x + 18.0,
            y: dy + dh - BUTTON_HEIGHT - PADDING + 8.0,
            text: "Proceed".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(BUTTON_WIDTH - 10.0),
        });
    }

    // -- status bar ------------------------------------------------------------

    fn render_status_bar(&self, tree: &mut RenderTree) {
        let y = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: WINDOW_WIDTH,
            height: STATUS_BAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Left side: drive info
        let left_text = if let Some(drive) = self.current_drive() {
            format!(
                "{} | {} | {} free",
                drive.label,
                drive.fs_type.label(),
                format_size(drive.free_bytes()),
            )
        } else {
            "No drive selected".to_string()
        };

        tree.push(RenderCommand::Text {
            x: PADDING,
            y: y + 7.0,
            text: left_text,
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(WINDOW_WIDTH / 2.0),
        });

        // Right side: defrag state
        let right_text = match self.defrag_state() {
            DefragState::Idle => "Ready".to_string(),
            DefragState::Analyzing => "Analyzing...".to_string(),
            DefragState::Running => {
                if let Some(engine) = &self.engine {
                    format!("Defragmenting: {}", format_percent(engine.progress.percent()))
                } else {
                    "Defragmenting...".to_string()
                }
            }
            DefragState::Paused => "Paused".to_string(),
            DefragState::Completed => "Defragmentation complete".to_string(),
            DefragState::Error => "Error occurred".to_string(),
        };

        tree.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 250.0,
            y: y + 7.0,
            text: right_text,
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(240.0),
        });
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- test helpers ----------------------------------------------------------

    /// Create a sample block map with mixed states.
    fn sample_block_map() -> BlockMap {
        let mut bmap = BlockMap::new(100, 4096);
        // First 20 blocks: system
        bmap.set_range(0, 20, BlockState::System);
        // 20-50: contiguous data
        bmap.set_range(20, 30, BlockState::Contiguous);
        // 50-60: fragmented data
        bmap.set_range(50, 10, BlockState::Fragmented);
        // 60-70: free
        // 70-80: contiguous
        bmap.set_range(70, 10, BlockState::Contiguous);
        // 80-85: fragmented
        bmap.set_range(80, 5, BlockState::Fragmented);
        // 85-90: free
        // 90-95: contiguous
        bmap.set_range(90, 5, BlockState::Contiguous);
        // 95-98: reserved
        bmap.set_range(95, 3, BlockState::Reserved);
        // 98-100: free
        bmap
    }

    /// Create sample file fragmentation info.
    fn sample_files() -> Vec<FileFragInfo> {
        vec![
            FileFragInfo {
                path: "/home/user/large_video.mp4".to_string(),
                size_bytes: 1024 * 1024 * 500,
                fragment_count: 15,
                block_count: 122070,
                excluded: false,
            },
            FileFragInfo {
                path: "/home/user/document.txt".to_string(),
                size_bytes: 4096,
                fragment_count: 1,
                block_count: 1,
                excluded: false,
            },
            FileFragInfo {
                path: "/var/log/system.log".to_string(),
                size_bytes: 1024 * 1024 * 10,
                fragment_count: 8,
                block_count: 2441,
                excluded: false,
            },
            FileFragInfo {
                path: "/boot/vmlinuz".to_string(),
                size_bytes: 1024 * 1024 * 8,
                fragment_count: 3,
                block_count: 1953,
                excluded: false,
            },
            FileFragInfo {
                path: "/tmp/temp_data.bin".to_string(),
                size_bytes: 1024 * 512,
                fragment_count: 5,
                block_count: 125,
                excluded: false,
            },
            FileFragInfo {
                path: "/home/user/photos/vacation.jpg".to_string(),
                size_bytes: 1024 * 1024 * 4,
                fragment_count: 2,
                block_count: 977,
                excluded: false,
            },
        ]
    }

    /// Create sample drives.
    fn sample_drives() -> Vec<DriveInfo> {
        vec![
            DriveInfo {
                id: "/dev/sda1".to_string(),
                label: "System (sda1)".to_string(),
                fs_type: FilesystemType::Ext4,
                storage_type: StorageType::Hdd,
                total_bytes: 500 * 1024 * 1024 * 1024,
                used_bytes: 350 * 1024 * 1024 * 1024,
                mount_point: "/".to_string(),
                block_size: 4096,
            },
            DriveInfo {
                id: "/dev/sdb1".to_string(),
                label: "Data (sdb1)".to_string(),
                fs_type: FilesystemType::Ext4,
                storage_type: StorageType::Ssd,
                total_bytes: 1024u64 * 1024 * 1024 * 1024,
                used_bytes: 400 * 1024 * 1024 * 1024,
                mount_point: "/data".to_string(),
                block_size: 4096,
            },
            DriveInfo {
                id: "/dev/sdc1".to_string(),
                label: "Backup (sdc1)".to_string(),
                fs_type: FilesystemType::Fat32,
                storage_type: StorageType::Hdd,
                total_bytes: 2u64 * 1024 * 1024 * 1024 * 1024,
                used_bytes: 1500u64 * 1024 * 1024 * 1024,
                mount_point: "/backup".to_string(),
                block_size: 4096,
            },
        ]
    }

    fn populated_ui() -> DefragUI {
        let mut ui = DefragUI::new();
        ui.set_drives(sample_drives());
        let bmap = sample_block_map();
        let files = sample_files();
        let result = analyze_drive(&bmap, &files);
        ui.load_analysis(result);
        ui
    }

    // == BlockMap tests ========================================================

    #[test]
    fn test_block_map_new() {
        let bmap = BlockMap::new(1000, 4096);
        assert_eq!(bmap.total_blocks, 1000);
        assert_eq!(bmap.block_size_bytes, 4096);
        assert_eq!(bmap.blocks.len(), 1000);
        assert!(bmap.blocks.iter().all(|&b| b == BlockState::Free));
    }

    #[test]
    fn test_block_map_set_get() {
        let mut bmap = BlockMap::new(10, 4096);
        assert!(bmap.set_block(5, BlockState::Contiguous));
        assert_eq!(bmap.get_block(5), Some(BlockState::Contiguous));
        assert_eq!(bmap.get_block(0), Some(BlockState::Free));
    }

    #[test]
    fn test_block_map_set_out_of_bounds() {
        let mut bmap = BlockMap::new(10, 4096);
        assert!(!bmap.set_block(100, BlockState::Contiguous));
        assert_eq!(bmap.get_block(100), None);
    }

    #[test]
    fn test_block_map_set_range() {
        let mut bmap = BlockMap::new(20, 4096);
        bmap.set_range(5, 10, BlockState::Contiguous);
        for i in 0..5 {
            assert_eq!(bmap.get_block(i), Some(BlockState::Free));
        }
        for i in 5..15 {
            assert_eq!(bmap.get_block(i), Some(BlockState::Contiguous));
        }
        for i in 15..20 {
            assert_eq!(bmap.get_block(i), Some(BlockState::Free));
        }
    }

    #[test]
    fn test_block_map_count_state() {
        let bmap = sample_block_map();
        assert_eq!(bmap.count_state(BlockState::System), 20);
        assert_eq!(bmap.count_state(BlockState::Contiguous), 45);
        assert_eq!(bmap.count_state(BlockState::Fragmented), 15);
        assert_eq!(bmap.count_state(BlockState::Reserved), 3);
    }

    #[test]
    fn test_block_map_free_blocks() {
        let bmap = sample_block_map();
        // 100 total - 20 system - 45 contiguous - 15 fragmented - 3 reserved = 17 free
        assert_eq!(bmap.free_blocks(), 17);
    }

    #[test]
    fn test_block_map_used_blocks() {
        let bmap = sample_block_map();
        // system(20) + contiguous(45) + fragmented(15) = 80
        assert_eq!(bmap.used_blocks(), 80);
    }

    #[test]
    fn test_block_map_largest_free_region() {
        let bmap = sample_block_map();
        // Free regions: blocks 60-69 (10 blocks), 85-89 (5 blocks), 98-99 (2 blocks)
        assert_eq!(bmap.largest_free_region(), 10);
    }

    #[test]
    fn test_block_map_fragmentation_percent() {
        let bmap = sample_block_map();
        // fragmented=15, contiguous=45, total_data=60
        // frag% = 15/60 * 100 = 25.0
        let pct = bmap.fragmentation_percent();
        assert!((pct - 25.0).abs() < 0.1);
    }

    #[test]
    fn test_block_map_fragmentation_zero() {
        let mut bmap = BlockMap::new(50, 4096);
        bmap.set_range(0, 30, BlockState::Contiguous);
        assert!((bmap.fragmentation_percent() - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_block_map_fragmentation_empty() {
        let bmap = BlockMap::new(50, 4096);
        assert!((bmap.fragmentation_percent() - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_block_map_fragment_count() {
        let bmap = sample_block_map();
        // Two separate runs of Fragmented: 50-59 and 80-84 = 2 fragments
        assert_eq!(bmap.fragment_count(), 2);
    }

    #[test]
    fn test_block_map_fragment_count_none() {
        let mut bmap = BlockMap::new(20, 4096);
        bmap.set_range(0, 20, BlockState::Contiguous);
        assert_eq!(bmap.fragment_count(), 0);
    }

    // == DriveInfo tests =======================================================

    #[test]
    fn test_drive_info_free_bytes() {
        let drive = DriveInfo {
            id: "d".to_string(),
            label: "Test".to_string(),
            fs_type: FilesystemType::Ext4,
            storage_type: StorageType::Hdd,
            total_bytes: 1000,
            used_bytes: 600,
            mount_point: "/".to_string(),
            block_size: 4096,
        };
        assert_eq!(drive.free_bytes(), 400);
    }

    #[test]
    fn test_drive_info_used_percent() {
        let drive = DriveInfo {
            id: "d".to_string(),
            label: "Test".to_string(),
            fs_type: FilesystemType::Ext4,
            storage_type: StorageType::Hdd,
            total_bytes: 1000,
            used_bytes: 750,
            mount_point: "/".to_string(),
            block_size: 4096,
        };
        assert!((drive.used_percent() - 75.0).abs() < 0.1);
    }

    #[test]
    fn test_drive_info_used_percent_zero() {
        let drive = DriveInfo {
            id: "d".to_string(),
            label: "Test".to_string(),
            fs_type: FilesystemType::Ext4,
            storage_type: StorageType::Hdd,
            total_bytes: 0,
            used_bytes: 0,
            mount_point: "/".to_string(),
            block_size: 4096,
        };
        assert!((drive.used_percent() - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_drive_info_is_ssd() {
        let hdd = DriveInfo {
            id: "a".to_string(),
            label: "HDD".to_string(),
            fs_type: FilesystemType::Ext4,
            storage_type: StorageType::Hdd,
            total_bytes: 1000,
            used_bytes: 500,
            mount_point: "/".to_string(),
            block_size: 4096,
        };
        assert!(!hdd.is_ssd());

        let ssd = DriveInfo {
            id: "b".to_string(),
            label: "SSD".to_string(),
            fs_type: FilesystemType::Ext4,
            storage_type: StorageType::Ssd,
            total_bytes: 1000,
            used_bytes: 500,
            mount_point: "/".to_string(),
            block_size: 4096,
        };
        assert!(ssd.is_ssd());

        let nvme = DriveInfo {
            id: "c".to_string(),
            label: "NVMe".to_string(),
            fs_type: FilesystemType::Ext4,
            storage_type: StorageType::NvMe,
            total_bytes: 1000,
            used_bytes: 500,
            mount_point: "/".to_string(),
            block_size: 4096,
        };
        assert!(nvme.is_ssd());
    }

    // == StorageType tests =====================================================

    #[test]
    fn test_storage_type_is_solid_state() {
        assert!(!StorageType::Hdd.is_solid_state());
        assert!(StorageType::Ssd.is_solid_state());
        assert!(StorageType::NvMe.is_solid_state());
        assert!(!StorageType::Unknown.is_solid_state());
    }

    #[test]
    fn test_storage_type_labels() {
        assert_eq!(StorageType::Hdd.label(), "HDD");
        assert_eq!(StorageType::Ssd.label(), "SSD");
        assert_eq!(StorageType::NvMe.label(), "NVMe");
    }

    // == FilesystemType tests ==================================================

    #[test]
    fn test_filesystem_type_labels() {
        assert_eq!(FilesystemType::Ext4.label(), "ext4");
        assert_eq!(FilesystemType::Fat32.label(), "FAT32");
        assert_eq!(FilesystemType::Ntfs.label(), "NTFS");
        assert_eq!(FilesystemType::Btrfs.label(), "btrfs");
        assert_eq!(FilesystemType::Xfs.label(), "XFS");
        assert_eq!(FilesystemType::Unknown.label(), "Unknown");
    }

    // == FileFragInfo tests ====================================================

    #[test]
    fn test_file_frag_is_fragmented() {
        let f1 = FileFragInfo {
            path: "/a".to_string(),
            size_bytes: 100,
            fragment_count: 1,
            block_count: 1,
            excluded: false,
        };
        assert!(!f1.is_fragmented());

        let f2 = FileFragInfo {
            path: "/b".to_string(),
            size_bytes: 100,
            fragment_count: 5,
            block_count: 5,
            excluded: false,
        };
        assert!(f2.is_fragmented());
    }

    #[test]
    fn test_file_frag_severity_contiguous() {
        let f = FileFragInfo {
            path: "/a".to_string(),
            size_bytes: 1024 * 1024,
            fragment_count: 1,
            block_count: 256,
            excluded: false,
        };
        assert!((f.severity() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_file_frag_severity_fragmented() {
        let f = FileFragInfo {
            path: "/a".to_string(),
            size_bytes: 1024 * 1024 * 10,
            fragment_count: 10,
            block_count: 2560,
            excluded: false,
        };
        assert!(f.severity() > 0.0);
        // Higher fragment count and larger file = higher severity
    }

    #[test]
    fn test_file_frag_severity_ordering() {
        let small_few = FileFragInfo {
            path: "/a".to_string(),
            size_bytes: 4096,
            fragment_count: 2,
            block_count: 1,
            excluded: false,
        };
        let large_many = FileFragInfo {
            path: "/b".to_string(),
            size_bytes: 1024 * 1024 * 100,
            fragment_count: 20,
            block_count: 25000,
            excluded: false,
        };
        assert!(large_many.severity() > small_few.severity());
    }

    // == Analysis tests ========================================================

    #[test]
    fn test_analyze_drive() {
        let bmap = sample_block_map();
        let files = sample_files();
        let result = analyze_drive(&bmap, &files);

        assert!((result.fragmentation_percent - 25.0).abs() < 0.1);
        assert_eq!(result.total_fragments, 2);
        assert_eq!(result.total_file_count, 6);
        // Files with fragment_count > 1: large_video(15), system.log(8), vmlinuz(3), temp(5), vacation(2) = 5
        assert_eq!(result.fragmented_file_count, 5);
        assert_eq!(result.largest_free_region_blocks, 10);
        assert_eq!(result.largest_free_region_bytes, 10 * 4096);
    }

    #[test]
    fn test_analyze_drive_empty() {
        let bmap = BlockMap::new(100, 4096);
        let files: Vec<FileFragInfo> = Vec::new();
        let result = analyze_drive(&bmap, &files);

        assert!((result.fragmentation_percent - 0.0).abs() < 0.01);
        assert_eq!(result.total_fragments, 0);
        assert_eq!(result.total_file_count, 0);
        assert_eq!(result.fragmented_file_count, 0);
    }

    #[test]
    fn test_analyze_file_details_sorted_by_severity() {
        let bmap = sample_block_map();
        let files = sample_files();
        let result = analyze_drive(&bmap, &files);

        // file_details should be sorted by severity descending
        for i in 1..result.file_details.len() {
            assert!(
                result.file_details[i - 1].severity()
                    >= result.file_details[i].severity()
            );
        }
    }

    // == ExcludePattern tests ==================================================

    #[test]
    fn test_exclude_exact_match() {
        let p = ExcludePattern::new("/tmp/file.txt");
        assert!(p.matches("/tmp/file.txt"));
        assert!(!p.matches("/tmp/other.txt"));
    }

    #[test]
    fn test_exclude_directory_wildcard() {
        let p = ExcludePattern::new("/tmp/*");
        assert!(p.matches("/tmp/file.txt"));
        assert!(p.matches("/tmp/subdir/nested.txt"));
        assert!(p.matches("/tmp"));
        assert!(!p.matches("/var/tmp/file.txt"));
    }

    #[test]
    fn test_exclude_extension_wildcard() {
        let p = ExcludePattern::new("*.log");
        assert!(p.matches("/var/log/system.log"));
        assert!(p.matches("debug.log"));
        assert!(!p.matches("/var/log/system.txt"));
    }

    #[test]
    fn test_exclude_disabled() {
        let mut p = ExcludePattern::new("/tmp/*");
        p.enabled = false;
        assert!(!p.matches("/tmp/file.txt"));
    }

    #[test]
    fn test_exclude_trailing_slash() {
        let p = ExcludePattern::new("/var/log/");
        assert!(p.matches("/var/log/system.log"));
        assert!(!p.matches("/var/other.log"));
    }

    #[test]
    fn test_is_excluded() {
        let patterns = vec![
            ExcludePattern::new("/tmp/*"),
            ExcludePattern::new("*.log"),
        ];
        assert!(is_excluded("/tmp/file.txt", &patterns));
        assert!(is_excluded("/var/system.log", &patterns));
        assert!(!is_excluded("/home/user/data.txt", &patterns));
    }

    #[test]
    fn test_is_excluded_empty_patterns() {
        let patterns: Vec<ExcludePattern> = Vec::new();
        assert!(!is_excluded("/any/path", &patterns));
    }

    // == Schedule tests ========================================================

    #[test]
    fn test_schedule_default() {
        let sched = DefragSchedule::new("/dev/sda1");
        assert!(!sched.enabled);
        assert_eq!(sched.interval, ScheduleInterval::Weekly);
        assert_eq!(sched.preferred_day, DayOfWeek::Sunday);
        assert_eq!(sched.preferred_hour, 2);
    }

    #[test]
    fn test_schedule_summary_disabled() {
        let sched = DefragSchedule::new("/dev/sda1");
        assert_eq!(sched.summary(), "Disabled");
    }

    #[test]
    fn test_schedule_summary_daily() {
        let mut sched = DefragSchedule::new("/dev/sda1");
        sched.enabled = true;
        sched.interval = ScheduleInterval::Daily;
        sched.preferred_hour = 3;
        let summary = sched.summary();
        assert!(summary.contains("Daily"));
        assert!(summary.contains("03:00"));
    }

    #[test]
    fn test_schedule_summary_weekly() {
        let mut sched = DefragSchedule::new("/dev/sda1");
        sched.enabled = true;
        sched.interval = ScheduleInterval::Weekly;
        sched.preferred_day = DayOfWeek::Wednesday;
        sched.preferred_hour = 14;
        let summary = sched.summary();
        assert!(summary.contains("Weekly"));
        assert!(summary.contains("Wednesday"));
        assert!(summary.contains("14:00"));
    }

    #[test]
    fn test_schedule_summary_monthly() {
        let mut sched = DefragSchedule::new("/dev/sda1");
        sched.enabled = true;
        sched.interval = ScheduleInterval::Monthly;
        let summary = sched.summary();
        assert!(summary.contains("Monthly"));
    }

    #[test]
    fn test_schedule_should_run_disabled() {
        let sched = DefragSchedule::new("/dev/sda1");
        assert!(!sched.should_run(DayOfWeek::Sunday, 2, 1));
    }

    #[test]
    fn test_schedule_should_run_daily() {
        let mut sched = DefragSchedule::new("/dev/sda1");
        sched.enabled = true;
        sched.interval = ScheduleInterval::Daily;
        sched.preferred_hour = 3;
        assert!(sched.should_run(DayOfWeek::Monday, 3, 15));
        assert!(sched.should_run(DayOfWeek::Friday, 3, 1));
        assert!(!sched.should_run(DayOfWeek::Monday, 4, 15));
    }

    #[test]
    fn test_schedule_should_run_weekly() {
        let mut sched = DefragSchedule::new("/dev/sda1");
        sched.enabled = true;
        sched.interval = ScheduleInterval::Weekly;
        sched.preferred_day = DayOfWeek::Sunday;
        sched.preferred_hour = 2;
        assert!(sched.should_run(DayOfWeek::Sunday, 2, 10));
        assert!(!sched.should_run(DayOfWeek::Monday, 2, 10));
        assert!(!sched.should_run(DayOfWeek::Sunday, 3, 10));
    }

    #[test]
    fn test_schedule_should_run_monthly() {
        let mut sched = DefragSchedule::new("/dev/sda1");
        sched.enabled = true;
        sched.interval = ScheduleInterval::Monthly;
        sched.preferred_hour = 0;
        assert!(sched.should_run(DayOfWeek::Wednesday, 0, 1));
        assert!(!sched.should_run(DayOfWeek::Wednesday, 0, 2));
    }

    // == DefragEngine tests ====================================================

    #[test]
    fn test_defrag_engine_new() {
        let bmap = sample_block_map();
        let files = sample_files();
        let engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Full,
            Vec::new(),
        );
        assert_eq!(engine.progress.state, DefragState::Idle);
        assert_eq!(engine.progress.total_blocks_to_move, 15);
        assert!(engine.progress.initial_fragmentation > 0.0);
    }

    #[test]
    fn test_defrag_engine_start_pause_resume() {
        let bmap = sample_block_map();
        let files = sample_files();
        let mut engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Full,
            Vec::new(),
        );

        assert!(engine.can_run());
        engine.start();
        assert_eq!(engine.progress.state, DefragState::Running);
        assert!(!engine.can_run());

        engine.pause();
        assert_eq!(engine.progress.state, DefragState::Paused);
        assert!(engine.can_run());

        engine.resume();
        assert_eq!(engine.progress.state, DefragState::Running);
    }

    #[test]
    fn test_defrag_engine_step() {
        let bmap = sample_block_map();
        let files = sample_files();
        let mut engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Full,
            Vec::new(),
        );
        engine.start();

        assert!(engine.step());
        assert_eq!(engine.progress.blocks_moved, 1);
    }

    #[test]
    fn test_defrag_engine_step_not_running() {
        let bmap = sample_block_map();
        let files = sample_files();
        let mut engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Full,
            Vec::new(),
        );
        // Not started
        assert!(!engine.step());
        assert_eq!(engine.progress.blocks_moved, 0);
    }

    #[test]
    fn test_defrag_engine_step_batch() {
        let bmap = sample_block_map();
        let files = sample_files();
        let mut engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Full,
            Vec::new(),
        );
        engine.start();

        let done = engine.step_batch(5);
        assert_eq!(done, 5);
        assert_eq!(engine.progress.blocks_moved, 5);
    }

    #[test]
    fn test_defrag_engine_complete() {
        let bmap = sample_block_map();
        let files = sample_files();
        let mut engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Full,
            Vec::new(),
        );
        engine.start();

        // Run all steps until complete (15 fragmented blocks)
        let done = engine.step_batch(100);
        assert_eq!(done, 15);
        assert_eq!(engine.progress.state, DefragState::Completed);
        assert!((engine.progress.current_fragmentation - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_defrag_engine_no_free_space() {
        // All blocks used, no free space
        let mut bmap = BlockMap::new(10, 4096);
        bmap.set_range(0, 5, BlockState::Contiguous);
        bmap.set_range(5, 5, BlockState::Fragmented);

        let files = vec![FileFragInfo {
            path: "/file".to_string(),
            size_bytes: 40960,
            fragment_count: 5,
            block_count: 10,
            excluded: false,
        }];

        let mut engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Full,
            Vec::new(),
        );
        engine.start();
        engine.step();
        assert_eq!(engine.progress.state, DefragState::Error);
    }

    #[test]
    fn test_defrag_engine_is_active() {
        let bmap = BlockMap::new(10, 4096);
        let mut engine = DefragEngine::new(
            bmap,
            Vec::new(),
            OptimizationMode::Full,
            Vec::new(),
        );
        assert!(!engine.is_active());

        engine.start();
        assert!(engine.is_active());

        engine.pause();
        assert!(engine.is_active());
    }

    // == Optimization mode tests ===============================================

    #[test]
    fn test_optimization_mode_labels() {
        assert_eq!(OptimizationMode::Quick.label(), "Quick");
        assert_eq!(OptimizationMode::Full.label(), "Full");
        assert_eq!(OptimizationMode::FreeSpace.label(), "Free Space");
        assert_eq!(OptimizationMode::BootOptimize.label(), "Boot Optimize");
    }

    #[test]
    fn test_optimization_mode_all() {
        let all = OptimizationMode::all();
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn test_optimization_mode_descriptions() {
        for mode in OptimizationMode::all() {
            assert!(!mode.description().is_empty());
        }
    }

    #[test]
    fn test_files_to_process_quick() {
        let bmap = sample_block_map();
        let files = sample_files();
        let engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Quick,
            Vec::new(),
        );
        let to_process = engine.files_to_process();
        // Quick mode: top 20% of fragmented files
        // 5 fragmented files, 20% = 1 (max of 1 and len/5)
        assert!(to_process.len() <= 5);
        assert!(!to_process.is_empty());
    }

    #[test]
    fn test_files_to_process_full() {
        let bmap = sample_block_map();
        let files = sample_files();
        let engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Full,
            Vec::new(),
        );
        let to_process = engine.files_to_process();
        assert_eq!(to_process.len(), 5); // 5 fragmented files
    }

    #[test]
    fn test_files_to_process_freespace() {
        let bmap = sample_block_map();
        let files = sample_files();
        let engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::FreeSpace,
            Vec::new(),
        );
        let to_process = engine.files_to_process();
        assert!(to_process.is_empty()); // FreeSpace mode doesn't target files
    }

    #[test]
    fn test_files_to_process_boot_optimize() {
        let bmap = sample_block_map();
        let files = sample_files();
        let engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::BootOptimize,
            Vec::new(),
        );
        let to_process = engine.files_to_process();
        // Boot files should be prioritized (first in list)
        if !to_process.is_empty() {
            assert!(is_boot_file(&to_process[0].path));
        }
    }

    #[test]
    fn test_files_to_process_with_excludes() {
        let bmap = sample_block_map();
        let files = sample_files();
        let excludes = vec![ExcludePattern::new("/tmp/*")];
        let engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Full,
            excludes,
        );
        let to_process = engine.files_to_process();
        // /tmp/temp_data.bin should be excluded
        assert!(to_process.iter().all(|f| !f.path.starts_with("/tmp/")));
    }

    // == DefragProgress tests ==================================================

    #[test]
    fn test_defrag_progress_new() {
        let p = DefragProgress::new();
        assert_eq!(p.state, DefragState::Idle);
        assert_eq!(p.blocks_moved, 0);
        assert!((p.fraction() - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_defrag_progress_fraction() {
        let mut p = DefragProgress::new();
        p.total_blocks_to_move = 100;
        p.blocks_moved = 50;
        assert!((p.fraction() - 0.5).abs() < 0.01);
        assert!((p.percent() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_defrag_progress_fraction_zero_total() {
        let p = DefragProgress::new();
        assert!((p.fraction() - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_defrag_progress_improvement() {
        let mut p = DefragProgress::new();
        p.initial_fragmentation = 50.0;
        p.current_fragmentation = 10.0;
        // Improvement = (50 - 10) / 50 * 100 = 80%
        assert!((p.improvement_percent() - 80.0).abs() < 0.1);
    }

    #[test]
    fn test_defrag_progress_improvement_zero_initial() {
        let p = DefragProgress::new();
        assert!((p.improvement_percent() - 0.0).abs() < 0.01);
    }

    // == DefragStats tests =====================================================

    #[test]
    fn test_defrag_stats_from_engine() {
        let bmap = sample_block_map();
        let files = sample_files();
        let mut engine = DefragEngine::new(
            bmap,
            files,
            OptimizationMode::Full,
            Vec::new(),
        );
        engine.start();
        engine.step_batch(100);

        let stats = DefragStats::from_engine(&engine);
        assert!(stats.before_fragmentation > 0.0);
        assert!((stats.after_fragmentation - 0.0).abs() < 0.01);
        assert_eq!(stats.blocks_moved, 15);
        assert!(stats.improvement_percent > 0.0);
    }

    // == Sort tests ============================================================

    #[test]
    fn test_sort_file_list_by_fragments() {
        let mut files = sample_files();
        sort_file_list(&mut files, FileSortColumn::Fragments, SortDirection::Descending);
        for i in 1..files.len() {
            assert!(files[i - 1].fragment_count >= files[i].fragment_count);
        }
    }

    #[test]
    fn test_sort_file_list_by_size() {
        let mut files = sample_files();
        sort_file_list(&mut files, FileSortColumn::Size, SortDirection::Ascending);
        for i in 1..files.len() {
            assert!(files[i - 1].size_bytes <= files[i].size_bytes);
        }
    }

    #[test]
    fn test_sort_file_list_by_path() {
        let mut files = sample_files();
        sort_file_list(&mut files, FileSortColumn::Path, SortDirection::Ascending);
        for i in 1..files.len() {
            assert!(files[i - 1].path <= files[i].path);
        }
    }

    #[test]
    fn test_sort_file_list_by_severity() {
        let mut files = sample_files();
        sort_file_list(&mut files, FileSortColumn::Severity, SortDirection::Descending);
        for i in 1..files.len() {
            assert!(files[i - 1].severity() >= files[i].severity());
        }
    }

    #[test]
    fn test_sort_direction_toggle() {
        assert_eq!(SortDirection::Ascending.toggle(), SortDirection::Descending);
        assert_eq!(SortDirection::Descending.toggle(), SortDirection::Ascending);
    }

    // == is_boot_file tests ====================================================

    #[test]
    fn test_is_boot_file_positive() {
        assert!(is_boot_file("/boot/vmlinuz"));
        assert!(is_boot_file("/system/config"));
        assert!(is_boot_file("/etc/fstab"));
        assert!(is_boot_file("/lib/kernel-module.ko"));
        assert!(is_boot_file("/boot/initrd.img"));
        assert!(is_boot_file("driver.sys"));
    }

    #[test]
    fn test_is_boot_file_negative() {
        assert!(!is_boot_file("/home/user/document.txt"));
        assert!(!is_boot_file("/tmp/temp.bin"));
        assert!(!is_boot_file("/var/log/app.log"));
    }

    // == Format helpers tests ==================================================

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.00 KiB");
        assert_eq!(format_size(1024 * 1024), "1.00 MiB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GiB");
        assert_eq!(format_size(1024u64 * 1024 * 1024 * 1024), "1.00 TiB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(120), "2m 0s");
        assert_eq!(format_duration(3661), "1h 1m 1s");
    }

    #[test]
    fn test_format_percent() {
        assert_eq!(format_percent(0.0), "0.0%");
        assert_eq!(format_percent(50.0), "50.0%");
        assert_eq!(format_percent(99.9), "99.9%");
    }

    // == Color helpers tests ===================================================

    #[test]
    fn test_lighten_color() {
        let c = Color::rgb(100, 100, 100);
        let l = lighten_color(c, 50);
        assert_eq!(l.r, 150);
        assert_eq!(l.g, 150);
        assert_eq!(l.b, 150);
    }

    #[test]
    fn test_lighten_color_saturates() {
        let c = Color::rgb(250, 250, 250);
        let l = lighten_color(c, 50);
        assert_eq!(l.r, 255);
        assert_eq!(l.g, 255);
        assert_eq!(l.b, 255);
    }

    #[test]
    fn test_darken_color() {
        let c = Color::rgb(100, 100, 100);
        let d = darken_color(c, 50);
        assert_eq!(d.r, 50);
        assert_eq!(d.g, 50);
        assert_eq!(d.b, 50);
    }

    #[test]
    fn test_darken_color_saturates() {
        let c = Color::rgb(10, 10, 10);
        let d = darken_color(c, 50);
        assert_eq!(d.r, 0);
        assert_eq!(d.g, 0);
        assert_eq!(d.b, 0);
    }

    // == BlockState tests ======================================================

    #[test]
    fn test_block_state_colors() {
        // Each state should have a distinct color
        let states = [
            BlockState::Free,
            BlockState::Contiguous,
            BlockState::Fragmented,
            BlockState::System,
            BlockState::Moving,
            BlockState::Reserved,
        ];
        for state in &states {
            let _color = state.color(); // Should not panic
        }
    }

    #[test]
    fn test_block_state_labels() {
        assert_eq!(BlockState::Free.label(), "Free");
        assert_eq!(BlockState::Contiguous.label(), "Contiguous");
        assert_eq!(BlockState::Fragmented.label(), "Fragmented");
        assert_eq!(BlockState::System.label(), "System");
        assert_eq!(BlockState::Moving.label(), "Moving");
        assert_eq!(BlockState::Reserved.label(), "Reserved");
    }

    // == UI tests ==============================================================

    #[test]
    fn test_ui_new() {
        let ui = DefragUI::new();
        assert!(ui.drives.is_empty());
        assert_eq!(ui.selected_drive, 0);
        assert_eq!(ui.view_tab, ViewTab::DiskMap);
        assert!(ui.analysis.is_none());
        assert!(ui.engine.is_none());
        assert_eq!(ui.defrag_state(), DefragState::Idle);
    }

    #[test]
    fn test_ui_set_drives() {
        let mut ui = DefragUI::new();
        ui.set_drives(sample_drives());
        assert_eq!(ui.drives.len(), 3);
        assert_eq!(ui.selected_drive, 0);
    }

    #[test]
    fn test_ui_select_drive() {
        let mut ui = DefragUI::new();
        ui.set_drives(sample_drives());
        ui.select_drive(1);
        assert_eq!(ui.selected_drive, 1);
        assert!(ui.analysis.is_none()); // Analysis reset on drive change
    }

    #[test]
    fn test_ui_select_drive_out_of_range() {
        let mut ui = DefragUI::new();
        ui.set_drives(sample_drives());
        ui.select_drive(99);
        assert_eq!(ui.selected_drive, 0); // Should not change
    }

    #[test]
    fn test_ui_current_drive() {
        let mut ui = DefragUI::new();
        assert!(ui.current_drive().is_none());
        ui.set_drives(sample_drives());
        assert!(ui.current_drive().is_some());
    }

    #[test]
    fn test_ui_set_view_tab() {
        let mut ui = DefragUI::new();
        ui.set_view_tab(ViewTab::FileList);
        assert_eq!(ui.view_tab, ViewTab::FileList);
    }

    #[test]
    fn test_ui_ssd_warning_on_defrag() {
        let mut ui = DefragUI::new();
        ui.set_drives(sample_drives());
        ui.select_drive(1); // SSD drive

        let bmap = sample_block_map();
        let files = sample_files();
        let result = analyze_drive(&bmap, &files);
        ui.load_analysis(result);

        ui.start_defrag();
        assert!(ui.show_ssd_warning);
        assert!(ui.engine.is_none()); // Should NOT start
    }

    #[test]
    fn test_ui_force_start_on_ssd() {
        let mut ui = DefragUI::new();
        ui.set_drives(sample_drives());
        ui.select_drive(1); // SSD drive

        let bmap = sample_block_map();
        let files = sample_files();
        let result = analyze_drive(&bmap, &files);
        ui.load_analysis(result);

        ui.start_defrag();
        assert!(ui.show_ssd_warning);

        ui.force_start_defrag();
        assert!(!ui.show_ssd_warning);
        assert!(ui.engine.is_some());
    }

    #[test]
    fn test_ui_defrag_on_hdd() {
        let mut ui = DefragUI::new();
        ui.set_drives(sample_drives());
        ui.select_drive(0); // HDD drive

        let bmap = sample_block_map();
        let files = sample_files();
        let result = analyze_drive(&bmap, &files);
        ui.load_analysis(result);

        ui.start_defrag();
        assert!(!ui.show_ssd_warning);
        assert!(ui.engine.is_some());
    }

    #[test]
    fn test_ui_pause_resume() {
        let mut ui = populated_ui();
        ui.start_defrag();
        assert_eq!(ui.defrag_state(), DefragState::Running);

        ui.pause_defrag();
        assert_eq!(ui.defrag_state(), DefragState::Paused);

        ui.resume_defrag();
        assert_eq!(ui.defrag_state(), DefragState::Running);
    }

    #[test]
    fn test_ui_defrag_step() {
        let mut ui = populated_ui();
        ui.start_defrag();
        ui.defrag_step();
        if let Some(engine) = &ui.engine {
            assert_eq!(engine.progress.blocks_moved, 1);
        }
    }

    #[test]
    fn test_ui_defrag_step_batch_completes() {
        let mut ui = populated_ui();
        ui.start_defrag();
        ui.defrag_step_batch(100);
        assert_eq!(ui.defrag_state(), DefragState::Completed);
        assert!(ui.stats.is_some());
    }

    #[test]
    fn test_ui_set_file_sort() {
        let mut ui = DefragUI::new();
        ui.set_file_sort(FileSortColumn::Size);
        assert_eq!(ui.file_sort_column, FileSortColumn::Size);
        assert_eq!(ui.file_sort_direction, SortDirection::Descending);

        // Same column toggles direction
        ui.set_file_sort(FileSortColumn::Size);
        assert_eq!(ui.file_sort_direction, SortDirection::Ascending);
    }

    #[test]
    fn test_ui_add_exclude() {
        let mut ui = DefragUI::new();
        let initial = ui.excludes.len();
        ui.add_exclude("/new/pattern/*");
        assert_eq!(ui.excludes.len(), initial + 1);
    }

    #[test]
    fn test_ui_add_exclude_empty() {
        let mut ui = DefragUI::new();
        let initial = ui.excludes.len();
        ui.add_exclude("");
        assert_eq!(ui.excludes.len(), initial); // Should not add empty
    }

    #[test]
    fn test_ui_remove_exclude() {
        let mut ui = DefragUI::new();
        let initial = ui.excludes.len();
        ui.remove_exclude(0);
        assert_eq!(ui.excludes.len(), initial - 1);
    }

    #[test]
    fn test_ui_remove_exclude_out_of_range() {
        let mut ui = DefragUI::new();
        let initial = ui.excludes.len();
        ui.remove_exclude(999);
        assert_eq!(ui.excludes.len(), initial);
    }

    #[test]
    fn test_ui_toggle_exclude() {
        let mut ui = DefragUI::new();
        let initial_enabled = ui.excludes[0].enabled;
        ui.toggle_exclude(0);
        assert_ne!(ui.excludes[0].enabled, initial_enabled);
    }

    // == Rendering tests =======================================================

    #[test]
    fn test_render_empty_ui() {
        let ui = DefragUI::new();
        let tree = ui.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_drives() {
        let mut ui = DefragUI::new();
        ui.set_drives(sample_drives());
        let tree = ui.render();
        assert!(tree.len() > 20);
    }

    #[test]
    fn test_render_disk_map_view() {
        let ui = populated_ui();
        let tree = ui.render();
        assert!(tree.len() > 30);
    }

    #[test]
    fn test_render_file_list_view() {
        let mut ui = populated_ui();
        ui.set_view_tab(ViewTab::FileList);
        let tree = ui.render();
        assert!(tree.len() > 20);
    }

    #[test]
    fn test_render_statistics_view_no_stats() {
        let mut ui = DefragUI::new();
        ui.set_view_tab(ViewTab::Statistics);
        let tree = ui.render();
        assert!(tree.len() > 5);
    }

    #[test]
    fn test_render_statistics_view_with_stats() {
        let mut ui = populated_ui();
        ui.start_defrag();
        ui.defrag_step_batch(100);
        ui.set_view_tab(ViewTab::Statistics);
        let tree = ui.render();
        assert!(tree.len() > 20);
    }

    #[test]
    fn test_render_schedule_view() {
        let mut ui = DefragUI::new();
        ui.set_view_tab(ViewTab::Schedule);
        let tree = ui.render();
        assert!(tree.len() > 10);
    }

    #[test]
    fn test_render_ssd_warning() {
        let mut ui = DefragUI::new();
        ui.show_ssd_warning = true;
        let tree = ui.render();
        // Should have the overlay + dialog elements
        assert!(tree.len() > 10);
    }

    #[test]
    fn test_render_during_defrag() {
        let mut ui = populated_ui();
        ui.start_defrag();
        ui.defrag_step_batch(5);
        let tree = ui.render();
        assert!(tree.len() > 30);
    }

    #[test]
    fn test_render_live_stats() {
        let mut ui = populated_ui();
        ui.start_defrag();
        ui.defrag_step_batch(5);
        ui.set_view_tab(ViewTab::Statistics);
        let tree = ui.render();
        assert!(tree.len() > 15);
    }

    // == ViewTab tests =========================================================

    #[test]
    fn test_view_tab_all() {
        let all = ViewTab::all();
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn test_view_tab_labels() {
        assert_eq!(ViewTab::DiskMap.label(), "Disk Map");
        assert_eq!(ViewTab::FileList.label(), "Files");
        assert_eq!(ViewTab::Statistics.label(), "Statistics");
        assert_eq!(ViewTab::Schedule.label(), "Schedule");
    }

    // == DayOfWeek tests =======================================================

    #[test]
    fn test_day_of_week_all() {
        assert_eq!(DayOfWeek::all().len(), 7);
    }

    #[test]
    fn test_day_of_week_labels() {
        assert_eq!(DayOfWeek::Monday.label(), "Monday");
        assert_eq!(DayOfWeek::Sunday.label(), "Sunday");
    }

    // == ScheduleInterval tests ================================================

    #[test]
    fn test_schedule_interval_labels() {
        assert_eq!(ScheduleInterval::Daily.label(), "Daily");
        assert_eq!(ScheduleInterval::Weekly.label(), "Weekly");
        assert_eq!(ScheduleInterval::Monthly.label(), "Monthly");
    }
}

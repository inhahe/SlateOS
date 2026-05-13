//! File explorer status bar content generation.
//!
//! Generates the information displayed in the status bar at the bottom
//! of the file explorer window.  The status bar adapts its content based
//! on current state:
//!
//! - **No selection**: "N items" + disk free space
//! - **Single selection**: file name + size + type
//! - **Multiple selection**: "N items selected" + total size
//! - **Search active**: "N results found in M ms"
//! - **Loading**: progress indicator
//!
//! ## Architecture
//!
//! ```text
//! File explorer state changes
//!   → statusbar::generate(state) produces StatusContent
//!   → GUI renders left/center/right sections
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// What the explorer is currently doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerState {
    /// Idle, showing directory contents.
    Idle,
    /// Loading a directory.
    Loading,
    /// Search results displayed.
    SearchResults,
    /// File operation in progress.
    OperationInProgress,
}

/// Input state for status bar generation.
#[derive(Debug, Clone)]
pub struct StatusInput {
    /// Current directory path.
    pub directory: String,
    /// Total items in the current directory.
    pub total_items: u64,
    /// Number of files.
    pub file_count: u64,
    /// Number of directories.
    pub dir_count: u64,
    /// Number of hidden items (not shown).
    pub hidden_count: u64,
    /// Number of selected items.
    pub selected_count: u64,
    /// Total size of selected items.
    pub selected_size: u64,
    /// Explorer state.
    pub state: ExplorerState,
    /// Search query (if searching).
    pub search_query: String,
    /// Search result count.
    pub search_results: u64,
    /// Search duration in milliseconds.
    pub search_duration_ms: u64,
    /// Operation progress (0-100).
    pub operation_progress: u8,
    /// Operation description.
    pub operation_desc: String,
}

impl Default for StatusInput {
    fn default() -> Self {
        Self {
            directory: String::from("/"),
            total_items: 0,
            file_count: 0,
            dir_count: 0,
            hidden_count: 0,
            selected_count: 0,
            selected_size: 0,
            state: ExplorerState::Idle,
            search_query: String::new(),
            search_results: 0,
            search_duration_ms: 0,
            operation_progress: 0,
            operation_desc: String::new(),
        }
    }
}

/// Generated status bar content.
#[derive(Debug, Clone)]
pub struct StatusContent {
    /// Left section text (primary info).
    pub left: String,
    /// Center section text (usually empty or operation progress).
    pub center: String,
    /// Right section text (disk space, view info).
    pub right: String,
}

/// Disk space info for the status bar.
#[derive(Debug, Clone)]
pub struct DiskInfo {
    /// Free bytes.
    pub free_bytes: u64,
    /// Total bytes.
    pub total_bytes: u64,
    /// Filesystem type.
    pub fs_type: String,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static GENERATE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Generate status bar content from the current explorer state.
pub fn generate(input: &StatusInput) -> StatusContent {
    GENERATE_COUNT.fetch_add(1, Ordering::Relaxed);

    let left = generate_left(input);
    let center = generate_center(input);
    let right = generate_right(input);

    StatusContent { left, center, right }
}

/// Generate status bar content for a directory with automatic info gathering.
///
/// Convenience function that builds StatusInput from VFS queries.
pub fn generate_for_dir(path: &str, selected_count: u64, selected_size: u64) -> StatusContent {
    GENERATE_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut total_items = 0u64;
    let mut file_count = 0u64;
    let mut dir_count = 0u64;
    let mut hidden_count = 0u64;

    if let Ok(entries) = crate::fs::vfs::Vfs::readdir(path) {
        for entry in &entries {
            if entry.name.starts_with('.') {
                hidden_count = hidden_count.saturating_add(1);
            }
            match entry.entry_type {
                crate::fs::EntryType::File => file_count = file_count.saturating_add(1),
                crate::fs::EntryType::Directory => dir_count = dir_count.saturating_add(1),
                _ => {}
            }
            total_items = total_items.saturating_add(1);
        }
    }

    let input = StatusInput {
        directory: String::from(path),
        total_items,
        file_count,
        dir_count,
        hidden_count,
        selected_count,
        selected_size,
        state: ExplorerState::Idle,
        ..Default::default()
    };

    generate(&input)
}

/// Get disk information for the status bar.
pub fn disk_info(path: &str) -> Option<DiskInfo> {
    crate::fs::vfs::Vfs::statvfs(path).ok().map(|info| {
        DiskInfo {
            free_bytes: info.free_blocks.saturating_mul(info.block_size),
            total_bytes: info.total_blocks.saturating_mul(info.block_size),
            fs_type: info.fs_type,
        }
    })
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

fn generate_left(input: &StatusInput) -> String {
    match input.state {
        ExplorerState::Loading => {
            String::from("Loading...")
        }
        ExplorerState::SearchResults => {
            if input.search_results == 0 {
                alloc::format!("No results for \"{}\"", input.search_query)
            } else {
                alloc::format!("{} result{} for \"{}\" ({} ms)",
                               input.search_results,
                               if input.search_results == 1 { "" } else { "s" },
                               input.search_query,
                               input.search_duration_ms)
            }
        }
        ExplorerState::OperationInProgress => {
            if input.operation_desc.is_empty() {
                String::from("Operation in progress...")
            } else {
                input.operation_desc.clone()
            }
        }
        ExplorerState::Idle => {
            if input.selected_count > 0 {
                // Selection active.
                if input.selected_count == 1 {
                    alloc::format!("1 item selected ({})", format_size(input.selected_size))
                } else {
                    alloc::format!("{} items selected ({})",
                                   input.selected_count,
                                   format_size(input.selected_size))
                }
            } else {
                // No selection — show item count.
                let mut parts = Vec::new();
                if input.file_count > 0 {
                    parts.push(alloc::format!("{} file{}",
                                             input.file_count,
                                             if input.file_count == 1 { "" } else { "s" }));
                }
                if input.dir_count > 0 {
                    parts.push(alloc::format!("{} folder{}",
                                             input.dir_count,
                                             if input.dir_count == 1 { "" } else { "s" }));
                }
                if parts.is_empty() {
                    String::from("Empty folder")
                } else {
                    let joined = parts.join(", ");
                    if input.hidden_count > 0 {
                        alloc::format!("{} ({} hidden)", joined, input.hidden_count)
                    } else {
                        joined
                    }
                }
            }
        }
    }
}

fn generate_center(input: &StatusInput) -> String {
    match input.state {
        ExplorerState::OperationInProgress => {
            if input.operation_progress > 0 {
                alloc::format!("{}%", input.operation_progress)
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

fn generate_right(input: &StatusInput) -> String {
    // Show disk free space.
    match disk_info(&input.directory) {
        Some(info) => {
            alloc::format!("{} free", format_size(info.free_bytes))
        }
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format bytes into a human-readable size string.
fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return String::from("0 B");
    }
    if bytes < 1024 {
        alloc::format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        alloc::format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        alloc::format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        alloc::format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns generate_count.
pub fn stats() -> u64 {
    GENERATE_COUNT.load(Ordering::Relaxed)
}

/// Reset statistics.
pub fn reset_stats() {
    GENERATE_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the status bar module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: empty folder.
    {
        let input = StatusInput {
            total_items: 0,
            file_count: 0,
            dir_count: 0,
            ..Default::default()
        };
        let status = generate(&input);
        assert_eq!(status.left, "Empty folder");
        serial_println!("[statusbar] test 1 passed: empty folder");
    }

    // Test 2: folder with items.
    {
        let input = StatusInput {
            total_items: 15,
            file_count: 10,
            dir_count: 5,
            hidden_count: 2,
            ..Default::default()
        };
        let status = generate(&input);
        assert!(status.left.contains("10 files"));
        assert!(status.left.contains("5 folders"));
        assert!(status.left.contains("2 hidden"));
        serial_println!("[statusbar] test 2 passed: items display");
    }

    // Test 3: single selection.
    {
        let input = StatusInput {
            selected_count: 1,
            selected_size: 1024,
            ..Default::default()
        };
        let status = generate(&input);
        assert!(status.left.contains("1 item selected"));
        assert!(status.left.contains("1.0 KiB"));
        serial_println!("[statusbar] test 3 passed: single selection");
    }

    // Test 4: multiple selection.
    {
        let input = StatusInput {
            selected_count: 5,
            selected_size: 1024 * 1024 * 3,
            ..Default::default()
        };
        let status = generate(&input);
        assert!(status.left.contains("5 items selected"));
        assert!(status.left.contains("3.0 MiB"));
        serial_println!("[statusbar] test 4 passed: multi selection");
    }

    // Test 5: search results.
    {
        let input = StatusInput {
            state: ExplorerState::SearchResults,
            search_query: String::from("*.rs"),
            search_results: 42,
            search_duration_ms: 15,
            ..Default::default()
        };
        let status = generate(&input);
        assert!(status.left.contains("42 results"));
        assert!(status.left.contains("*.rs"));
        assert!(status.left.contains("15 ms"));
        serial_println!("[statusbar] test 5 passed: search results");
    }

    // Test 6: loading state.
    {
        let input = StatusInput {
            state: ExplorerState::Loading,
            ..Default::default()
        };
        let status = generate(&input);
        assert_eq!(status.left, "Loading...");
        serial_println!("[statusbar] test 6 passed: loading state");
    }

    // Test 7: generate_for_dir.
    {
        let status = generate_for_dir("/", 0, 0);
        // Root should have items.
        assert!(!status.left.is_empty());
        serial_println!("[statusbar] test 7 passed: generate_for_dir");
    }

    serial_println!("[statusbar] all 7 self-tests passed");
    Ok(())
}

//! File selection management for the file explorer.
//!
//! Tracks which files are selected in a file explorer window, supporting
//! single-click, shift-click (range), ctrl-click (toggle), and
//! select-all operations.  Also supports tristate checkbox tree
//! selection for directory-based selection dialogs (design spec
//! line 779: "tristate checkbox treeview - good for selecting files
//! and directories").
//!
//! ## Architecture
//!
//! ```text
//! File explorer UI
//!   → SelectionSet (per-window selection state)
//!   → Selection operations (click, shift-click, ctrl-click, select-all)
//!   → Integration with fileops (copy/move/delete selected items)
//!   → Integration with clipboard (copy selection to clipboard)
//!   → Integration with dragdrop (drag selected items)
//! ```
//!
//! ## Selection Modes
//!
//! - **Single**: Click replaces selection with single item.
//! - **Toggle**: Ctrl+click adds/removes individual items.
//! - **Range**: Shift+click selects all items from anchor to target.
//! - **SelectAll**: Selects every visible item in the listing.
//! - **Invert**: Flips selection state of all items.
//! - **Pattern**: Select by glob pattern (e.g., "*.rs").
//!
//! ## Tristate Checkbox Tree
//!
//! For directory selection dialogs, each node has three states:
//! - **Checked**: directory and all children selected
//! - **Unchecked**: nothing selected
//! - **Partial**: some children selected (displayed as filled square)

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum items in a single selection set.
const MAX_SELECTION: usize = 65536;

/// Maximum selection sets (one per window).
const MAX_SETS: usize = 256;

/// Maximum pattern length for pattern selection.
const MAX_PATTERN_LEN: usize = 256;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How a selection operation was performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectMode {
    /// Replace selection with single item.
    Single,
    /// Toggle an item (ctrl+click).
    Toggle,
    /// Range select from anchor to target (shift+click).
    Range,
}

/// Tristate checkbox value for directory tree selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// Fully unchecked — nothing selected.
    Unchecked,
    /// Fully checked — item and all children selected.
    Checked,
    /// Partially checked — some children selected.
    Partial,
}

/// A single selected item.
#[derive(Debug, Clone)]
pub struct SelectedItem {
    /// Full path of the selected item.
    pub path: String,
    /// Item name (filename).
    pub name: String,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// Size in bytes (0 for directories).
    pub size: u64,
}

/// Per-window selection state.
#[derive(Debug, Clone)]
pub struct SelectionSet {
    /// Unique set ID.
    pub id: u64,
    /// Directory being viewed.
    pub directory: String,
    /// Selected items (ordered by selection time).
    pub items: Vec<SelectedItem>,
    /// Anchor index for range selection (index into the visible listing).
    pub anchor: Option<usize>,
    /// Most recently selected index.
    pub cursor: Option<usize>,
    /// Total size of all selected files.
    pub total_size: u64,
    /// Count of selected files (not directories).
    pub file_count: u64,
    /// Count of selected directories.
    pub dir_count: u64,
}

/// Summary of current selection for status bar display.
#[derive(Debug, Clone)]
pub struct SelectionSummary {
    /// Total selected items.
    pub count: usize,
    /// Number of files.
    pub files: u64,
    /// Number of directories.
    pub dirs: u64,
    /// Total size of selected files.
    pub total_size: u64,
    /// Human-readable size string.
    pub size_display: String,
}

/// A node in the tristate checkbox tree.
#[derive(Debug, Clone)]
pub struct CheckTreeNode {
    /// Node path.
    pub path: String,
    /// Node name.
    pub name: String,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// Check state.
    pub state: CheckState,
    /// Children (for directories).
    pub children: Vec<CheckTreeNode>,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static NEXT_SET_ID: AtomicU64 = AtomicU64::new(1);
static SELECT_COUNT: AtomicU64 = AtomicU64::new(0);
static DESELECT_COUNT: AtomicU64 = AtomicU64::new(0);

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::vec;

/// All active selection sets.
static SETS: Mutex<Vec<SelectionSet>> = Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// SelectionSet methods
// ---------------------------------------------------------------------------

impl SelectionSet {
    /// Create a new empty selection set for a directory.
    fn new(directory: &str) -> Self {
        Self {
            id: NEXT_SET_ID.fetch_add(1, Ordering::Relaxed),
            directory: String::from(directory),
            items: Vec::new(),
            anchor: None,
            cursor: None,
            total_size: 0,
            file_count: 0,
            dir_count: 0,
        }
    }

    /// Whether the set is empty.
    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of selected items.
    fn count(&self) -> usize {
        self.items.len()
    }

    /// Check if a path is selected.
    fn contains(&self, path: &str) -> bool {
        self.items.iter().any(|i| i.path == path)
    }

    /// Add an item to the selection.
    fn add(&mut self, item: SelectedItem) {
        if self.items.len() >= MAX_SELECTION {
            return;
        }
        // No duplicates.
        if self.contains(&item.path) {
            return;
        }
        if item.is_dir {
            self.dir_count = self.dir_count.saturating_add(1);
        } else {
            self.file_count = self.file_count.saturating_add(1);
            self.total_size = self.total_size.saturating_add(item.size);
        }
        self.items.push(item);
        SELECT_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    /// Remove an item by path.
    fn remove(&mut self, path: &str) {
        if let Some(pos) = self.items.iter().position(|i| i.path == path) {
            let item = self.items.remove(pos);
            if item.is_dir {
                self.dir_count = self.dir_count.saturating_sub(1);
            } else {
                self.file_count = self.file_count.saturating_sub(1);
                self.total_size = self.total_size.saturating_sub(item.size);
            }
            DESELECT_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Clear all selections.
    fn clear(&mut self) {
        let count = self.items.len() as u64;
        self.items.clear();
        self.anchor = None;
        self.cursor = None;
        self.total_size = 0;
        self.file_count = 0;
        self.dir_count = 0;
        DESELECT_COUNT.fetch_add(count, Ordering::Relaxed);
    }

    /// Get a summary for status bar display.
    fn summary(&self) -> SelectionSummary {
        SelectionSummary {
            count: self.items.len(),
            files: self.file_count,
            dirs: self.dir_count,
            total_size: self.total_size,
            size_display: format_size(self.total_size),
        }
    }

    /// Get all selected paths.
    fn paths(&self) -> Vec<String> {
        self.items.iter().map(|i| i.path.clone()).collect()
    }
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Create a new selection set for a directory.
pub fn create(directory: &str) -> KernelResult<u64> {
    let mut sets = SETS.lock();
    if sets.len() >= MAX_SETS {
        return Err(KernelError::ResourceExhausted);
    }
    let set = SelectionSet::new(directory);
    let id = set.id;
    sets.push(set);
    Ok(id)
}

/// Destroy a selection set.
pub fn destroy(set_id: u64) -> KernelResult<()> {
    let mut sets = SETS.lock();
    if let Some(pos) = sets.iter().position(|s| s.id == set_id) {
        sets.remove(pos);
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// Perform a single-item selection (click).
///
/// Replaces the current selection with just this item.
pub fn select_single(set_id: u64, path: &str, index: usize) -> KernelResult<()> {
    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;
    set.clear();

    let item = make_item(path)?;
    set.add(item);
    set.anchor = Some(index);
    set.cursor = Some(index);
    Ok(())
}

/// Toggle selection of an item (ctrl+click).
///
/// If selected, deselects it. If not selected, adds it to selection.
pub fn select_toggle(set_id: u64, path: &str, index: usize) -> KernelResult<()> {
    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    if set.contains(path) {
        set.remove(path);
    } else {
        let item = make_item(path)?;
        set.add(item);
    }
    set.cursor = Some(index);
    // Anchor stays for potential future range select.
    if set.anchor.is_none() {
        set.anchor = Some(index);
    }
    Ok(())
}

/// Range selection (shift+click).
///
/// Selects all items from the anchor to the given index.
/// `listing` provides the full ordered listing of the directory.
pub fn select_range(
    set_id: u64,
    listing: &[&str],
    target_index: usize,
) -> KernelResult<()> {
    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    let anchor = set.anchor.unwrap_or(0);
    let (start, end) = if anchor <= target_index {
        (anchor, target_index)
    } else {
        (target_index, anchor)
    };

    // Clear previous selection then select range.
    set.clear();
    set.anchor = Some(anchor);
    set.cursor = Some(target_index);

    for idx in start..=end {
        if let Some(path) = listing.get(idx) {
            if !set.contains(path) {
                if let Ok(item) = make_item(path) {
                    set.add(item);
                }
            }
        }
    }
    Ok(())
}

/// Select all items in the listing.
pub fn select_all(set_id: u64, listing: &[&str]) -> KernelResult<()> {
    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    set.clear();
    for path in listing {
        if let Ok(item) = make_item(path) {
            set.add(item);
        }
    }
    if !listing.is_empty() {
        set.anchor = Some(0);
        set.cursor = Some(listing.len().saturating_sub(1));
    }
    Ok(())
}

/// Invert selection — toggle every item in the listing.
pub fn select_invert(set_id: u64, listing: &[&str]) -> KernelResult<()> {
    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    // Collect currently selected paths.
    let was_selected: Vec<String> = set.paths();
    set.clear();

    // Add items that were NOT selected before.
    for path in listing {
        if !was_selected.iter().any(|s| s.as_str() == *path) {
            if let Ok(item) = make_item(path) {
                set.add(item);
            }
        }
    }
    Ok(())
}

/// Select items matching a glob pattern.
pub fn select_pattern(set_id: u64, listing: &[&str], pattern: &str) -> KernelResult<()> {
    if pattern.len() > MAX_PATTERN_LEN {
        return Err(KernelError::InvalidArgument);
    }
    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    for path in listing {
        let name = path.rsplit('/').next().unwrap_or(path);
        if simple_glob(pattern, name) && !set.contains(path) {
            if let Ok(item) = make_item(path) {
                set.add(item);
            }
        }
    }
    Ok(())
}

/// Deselect items matching a glob pattern.
pub fn deselect_pattern(set_id: u64, pattern: &str) -> KernelResult<()> {
    if pattern.len() > MAX_PATTERN_LEN {
        return Err(KernelError::InvalidArgument);
    }
    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    let to_remove: Vec<String> = set.items.iter()
        .filter(|i| simple_glob(pattern, &i.name))
        .map(|i| i.path.clone())
        .collect();

    for path in &to_remove {
        set.remove(path);
    }
    Ok(())
}

/// Clear selection for a set.
pub fn clear(set_id: u64) -> KernelResult<()> {
    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;
    set.clear();
    Ok(())
}

/// Get the selection summary.
pub fn summary(set_id: u64) -> KernelResult<SelectionSummary> {
    let sets = SETS.lock();
    let set = find_set(&sets, set_id)?;
    Ok(set.summary())
}

/// Get all selected paths.
pub fn selected_paths(set_id: u64) -> KernelResult<Vec<String>> {
    let sets = SETS.lock();
    let set = find_set(&sets, set_id)?;
    Ok(set.paths())
}

/// Check if a path is selected.
pub fn is_selected(set_id: u64, path: &str) -> KernelResult<bool> {
    let sets = SETS.lock();
    let set = find_set(&sets, set_id)?;
    Ok(set.contains(path))
}

/// Get the number of items selected.
pub fn count(set_id: u64) -> KernelResult<usize> {
    let sets = SETS.lock();
    let set = find_set(&sets, set_id)?;
    Ok(set.count())
}

/// List all active selection sets.
pub fn list_sets() -> Vec<(u64, String, usize)> {
    let sets = SETS.lock();
    sets.iter().map(|s| (s.id, s.directory.clone(), s.count())).collect()
}

// ---------------------------------------------------------------------------
// Tristate checkbox tree
// ---------------------------------------------------------------------------

/// Build a tristate checkbox tree for a directory.
///
/// Populates a tree structure with files and subdirectories.
/// All nodes start unchecked.
pub fn build_check_tree(path: &str) -> KernelResult<CheckTreeNode> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let meta = crate::fs::vfs::Vfs::metadata(path)?;
    let is_dir = meta.entry_type == crate::fs::EntryType::Directory;

    let children = if is_dir {
        match crate::fs::vfs::Vfs::readdir(path) {
            Ok(entries) => {
                let mut kids = Vec::new();
                for entry in &entries {
                    let child_path = if path == "/" {
                        alloc::format!("/{}", entry.name)
                    } else {
                        alloc::format!("{}/{}", path, entry.name)
                    };
                    // Build shallow children (one level only for performance).
                    kids.push(CheckTreeNode {
                        path: child_path,
                        name: entry.name.clone(),
                        is_dir: entry.entry_type == crate::fs::EntryType::Directory,
                        state: CheckState::Unchecked,
                        children: Vec::new(),
                    });
                }
                kids
            }
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    Ok(CheckTreeNode {
        path: String::from(path),
        name: String::from(name),
        is_dir,
        state: CheckState::Unchecked,
        children,
    })
}

/// Toggle a node in the check tree.
///
/// If unchecked or partial → checked (and all children checked).
/// If checked → unchecked (and all children unchecked).
/// Returns the new state.
pub fn toggle_check_node(node: &mut CheckTreeNode) -> CheckState {
    let new_state = match node.state {
        CheckState::Unchecked | CheckState::Partial => CheckState::Checked,
        CheckState::Checked => CheckState::Unchecked,
    };
    set_state_recursive(node, new_state);
    new_state
}

/// Recompute a parent node's state based on children.
pub fn recompute_parent_state(node: &mut CheckTreeNode) {
    if node.children.is_empty() {
        return;
    }
    let all_checked = node.children.iter().all(|c| c.state == CheckState::Checked);
    let all_unchecked = node.children.iter().all(|c| c.state == CheckState::Unchecked);

    node.state = if all_checked {
        CheckState::Checked
    } else if all_unchecked {
        CheckState::Unchecked
    } else {
        CheckState::Partial
    };
}

/// Collect all checked paths from a check tree.
pub fn collect_checked(node: &CheckTreeNode) -> Vec<String> {
    let mut result = Vec::new();
    collect_checked_recursive(node, &mut result);
    result
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find a set by ID (immutable).
fn find_set(sets: &[SelectionSet], id: u64) -> KernelResult<&SelectionSet> {
    sets.iter()
        .find(|s| s.id == id)
        .ok_or(KernelError::NotFound)
}

/// Find a set by ID (mutable).
fn find_set_mut(sets: &mut [SelectionSet], id: u64) -> KernelResult<&mut SelectionSet> {
    sets.iter_mut()
        .find(|s| s.id == id)
        .ok_or(KernelError::NotFound)
}

/// Create a SelectedItem from a path by querying VFS.
fn make_item(path: &str) -> KernelResult<SelectedItem> {
    let meta = crate::fs::vfs::Vfs::metadata(path)?;
    let name = path.rsplit('/').next().unwrap_or(path);
    Ok(SelectedItem {
        path: String::from(path),
        name: String::from(name),
        is_dir: meta.entry_type == crate::fs::EntryType::Directory,
        size: meta.size,
    })
}

/// Set check state recursively on a node and all children.
fn set_state_recursive(node: &mut CheckTreeNode, state: CheckState) {
    node.state = state;
    for child in &mut node.children {
        set_state_recursive(child, state);
    }
}

/// Recursively collect checked leaf paths.
fn collect_checked_recursive(node: &CheckTreeNode, result: &mut Vec<String>) {
    if node.state == CheckState::Checked {
        // If fully checked, add this path (not children individually).
        result.push(node.path.clone());
        return;
    }
    if node.state == CheckState::Partial {
        // Partial — recurse into children.
        for child in &node.children {
            collect_checked_recursive(child, result);
        }
    }
    // Unchecked — skip entirely.
}

/// Simple glob pattern matching (supports `*` and `?`).
fn simple_glob(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match(&pat, 0, &txt, 0)
}

fn glob_match(pat: &[char], pi: usize, txt: &[char], ti: usize) -> bool {
    if pi == pat.len() {
        return ti == txt.len();
    }
    match pat.get(pi).copied() {
        Some('*') => {
            // Try matching zero or more characters.
            let mut t = ti;
            loop {
                if glob_match(pat, pi + 1, txt, t) {
                    return true;
                }
                if t >= txt.len() {
                    break;
                }
                t += 1;
            }
            false
        }
        Some('?') => {
            if ti < txt.len() {
                glob_match(pat, pi + 1, txt, ti + 1)
            } else {
                false
            }
        }
        Some(c) => {
            if ti < txt.len() && txt.get(ti).copied() == Some(c) {
                glob_match(pat, pi + 1, txt, ti + 1)
            } else {
                false
            }
        }
        None => ti == txt.len(),
    }
}

/// Format a byte size for display.
fn format_size(bytes: u64) -> String {
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

/// Returns (select_count, deselect_count, active_sets).
pub fn stats() -> (u64, u64, usize) {
    let sets = SETS.lock();
    (
        SELECT_COUNT.load(Ordering::Relaxed),
        DESELECT_COUNT.load(Ordering::Relaxed),
        sets.len(),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    SELECT_COUNT.store(0, Ordering::Relaxed);
    DESELECT_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the file selection module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: create and destroy a selection set.
    {
        let id = create("/")?;
        assert!(id > 0);
        let sets = list_sets();
        assert!(sets.iter().any(|(sid, _, _)| *sid == id));
        destroy(id)?;
        let sets = list_sets();
        assert!(!sets.iter().any(|(sid, _, _)| *sid == id));
        serial_println!("[fileselect] test 1 passed: create/destroy");
    }

    // Test 2: single selection.
    {
        let id = create("/")?;
        // Select root directory itself.
        select_single(id, "/", 0)?;
        assert_eq!(count(id)?, 1);
        assert!(is_selected(id, "/")?);
        let s = summary(id)?;
        assert_eq!(s.count, 1);
        destroy(id)?;
        serial_println!("[fileselect] test 2 passed: single select");
    }

    // Test 3: toggle selection.
    {
        let id = create("/")?;
        select_single(id, "/", 0)?;
        // Toggle same item off.
        select_toggle(id, "/", 0)?;
        assert_eq!(count(id)?, 0);
        // Toggle back on.
        select_toggle(id, "/", 0)?;
        assert_eq!(count(id)?, 1);
        destroy(id)?;
        serial_println!("[fileselect] test 3 passed: toggle select");
    }

    // Test 4: glob pattern matching.
    {
        assert!(simple_glob("*.rs", "main.rs"));
        assert!(simple_glob("*.rs", "lib.rs"));
        assert!(!simple_glob("*.rs", "main.py"));
        assert!(simple_glob("test?", "test1"));
        assert!(simple_glob("test?", "testX"));
        assert!(!simple_glob("test?", "test"));
        assert!(simple_glob("*", "anything"));
        assert!(simple_glob("a*b", "ab"));
        assert!(simple_glob("a*b", "aXXXb"));
        serial_println!("[fileselect] test 4 passed: glob matching");
    }

    // Test 5: tristate checkbox tree.
    {
        let mut tree = build_check_tree("/")?;
        assert_eq!(tree.state, CheckState::Unchecked);
        assert!(tree.is_dir);

        // Toggle checks entire tree.
        let new_state = toggle_check_node(&mut tree);
        assert_eq!(new_state, CheckState::Checked);
        for child in &tree.children {
            assert_eq!(child.state, CheckState::Checked);
        }

        // Toggle again unchecks.
        let new_state = toggle_check_node(&mut tree);
        assert_eq!(new_state, CheckState::Unchecked);
        serial_println!("[fileselect] test 5 passed: check tree");
    }

    // Test 6: format_size helper.
    {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GiB");
        serial_println!("[fileselect] test 6 passed: format_size");
    }

    // Test 7: collect_checked.
    {
        let mut tree = CheckTreeNode {
            path: String::from("/test"),
            name: String::from("test"),
            is_dir: true,
            state: CheckState::Partial,
            children: vec![
                CheckTreeNode {
                    path: String::from("/test/a"),
                    name: String::from("a"),
                    is_dir: false,
                    state: CheckState::Checked,
                    children: Vec::new(),
                },
                CheckTreeNode {
                    path: String::from("/test/b"),
                    name: String::from("b"),
                    is_dir: false,
                    state: CheckState::Unchecked,
                    children: Vec::new(),
                },
            ],
        };
        let checked = collect_checked(&tree);
        assert_eq!(checked.len(), 1);
        assert_eq!(checked.first().map(|s| s.as_str()), Some("/test/a"));

        // After toggling parent, all should be checked.
        toggle_check_node(&mut tree);
        let checked = collect_checked(&tree);
        // Parent is checked → just the parent path.
        assert_eq!(checked.len(), 1);
        assert_eq!(checked.first().map(|s| s.as_str()), Some("/test"));
        serial_println!("[fileselect] test 7 passed: collect_checked");
    }

    serial_println!("[fileselect] all 7 self-tests passed");
    Ok(())
}

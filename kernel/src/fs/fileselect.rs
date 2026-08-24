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
use crate::fs::path::{Path, PathBuf};

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
    pub path: PathBuf,
    /// Item name (filename).
    pub name: PathBuf,
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
    pub directory: PathBuf,
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
    pub path: PathBuf,
    /// Node name.
    pub name: PathBuf,
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
    fn new(directory: &Path) -> Self {
        Self {
            id: NEXT_SET_ID.fetch_add(1, Ordering::Relaxed),
            directory: directory.to_path_buf(),
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
    fn contains(&self, path: &Path) -> bool {
        self.items.iter().any(|i| i.path.as_path() == path)
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
    fn remove(&mut self, path: &Path) {
        if let Some(pos) = self.items.iter().position(|i| i.path.as_path() == path) {
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
    fn paths(&self) -> Vec<PathBuf> {
        self.items.iter().map(|i| i.path.clone()).collect()
    }
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Create a new selection set for a directory.
pub fn create(directory: impl AsRef<Path>) -> KernelResult<u64> {
    let directory = directory.as_ref();
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
pub fn select_single(set_id: u64, path: impl AsRef<Path>, index: usize) -> KernelResult<()> {
    let path = path.as_ref();
    // The stat happens before `SETS` is taken, never under it -- see
    // `make_items` for the lock order this preserves.
    let item = make_item(path)?;

    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;
    set.clear();
    set.add(item);
    set.anchor = Some(index);
    set.cursor = Some(index);
    Ok(())
}

/// Toggle selection of an item (ctrl+click).
///
/// If selected, deselects it. If not selected, adds it to selection.
pub fn select_toggle(set_id: u64, path: impl AsRef<Path>, index: usize) -> KernelResult<()> {
    let path = path.as_ref();
    // Whether this call adds or removes is only knowable under the lock, but
    // the stat may not run under it (see `make_items`), so the remove case
    // pays for one stat it then discards. The `?` stays on the add path so the
    // error a caller would have seen is unchanged.
    let item = make_item(path);

    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    if set.contains(path) {
        set.remove(path);
    } else {
        set.add(item?);
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
pub fn select_range(set_id: u64, listing: &[&Path], target_index: usize) -> KernelResult<()> {
    // The anchor lives in the set, but the items must be stat'ed outside the
    // lock (see `make_items`). So: read the anchor under a short lock, release
    // it for the stats, then retake and install. The anchor is written back
    // explicitly below, so a concurrent change to it between the two critical
    // sections cannot leave the set describing a different range than the one
    // whose items were just built.
    let anchor = {
        let sets = SETS.lock();
        find_set(&sets, set_id)?.anchor.unwrap_or(0)
    };
    let (start, end) = if anchor <= target_index {
        (anchor, target_index)
    } else {
        (target_index, anchor)
    };
    // A range that runs past the end of the listing selects the part that
    // exists, as the per-index `listing.get` this replaced did.
    let last = end.min(listing.len().saturating_sub(1));
    let items = make_items(listing.get(start..=last).unwrap_or(&[]));

    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    // Clear previous selection then select range.
    set.clear();
    set.anchor = Some(anchor);
    set.cursor = Some(target_index);

    for item in items {
        if !set.contains(&item.path) {
            set.add(item);
        }
    }
    Ok(())
}

/// Select all items in the listing.
pub fn select_all(set_id: u64, listing: &[&Path]) -> KernelResult<()> {
    let items = make_items(listing);

    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    set.clear();
    for item in items {
        set.add(item);
    }
    if !listing.is_empty() {
        set.anchor = Some(0);
        set.cursor = Some(listing.len().saturating_sub(1));
    }
    Ok(())
}

/// Invert selection — toggle every item in the listing.
pub fn select_invert(set_id: u64, listing: &[&Path]) -> KernelResult<()> {
    // Which entries survive depends on the current selection and so is only
    // knowable under the lock, while the stats may only happen outside it (see
    // `make_items`). Every entry is therefore stat'ed and the already-selected
    // ones dropped afterwards: N stats rather than N-K. The bounded extra work
    // is the price of not holding `SETS` across the VFS.
    let items = make_items(listing);

    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    // Collect currently selected paths.
    let was_selected: Vec<PathBuf> = set.paths();
    set.clear();

    // Add items that were NOT selected before.
    for item in items {
        if !was_selected.contains(&item.path) {
            set.add(item);
        }
    }
    Ok(())
}

/// Select items matching a glob pattern.
pub fn select_pattern(set_id: u64, listing: &[&Path], pattern: &str) -> KernelResult<()> {
    if pattern.len() > MAX_PATTERN_LEN {
        return Err(KernelError::InvalidArgument);
    }
    // The glob test is pure, so it can run before the lock and spare the stat
    // for every non-matching entry. Only the "is it already selected?" test
    // needs the lock, and that one is cheap.
    let matching: Vec<&Path> = listing
        .iter()
        .filter(|path| {
            // A path with no final component (the root, or all separators) has
            // no name to match, so no pattern can select it.
            path.file_name()
                .is_some_and(|name| simple_glob(pattern, name))
        })
        .copied()
        .collect();
    let items = make_items(&matching);

    let mut sets = SETS.lock();
    let set = find_set_mut(&mut sets, set_id)?;

    for item in items {
        if !set.contains(&item.path) {
            set.add(item);
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

    let to_remove: Vec<PathBuf> = set
        .items
        .iter()
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
pub fn selected_paths(set_id: u64) -> KernelResult<Vec<PathBuf>> {
    let sets = SETS.lock();
    let set = find_set(&sets, set_id)?;
    Ok(set.paths())
}

/// Check if a path is selected.
pub fn is_selected(set_id: u64, path: impl AsRef<Path>) -> KernelResult<bool> {
    let sets = SETS.lock();
    let set = find_set(&sets, set_id)?;
    Ok(set.contains(path.as_ref()))
}

/// Get the number of items selected.
pub fn count(set_id: u64) -> KernelResult<usize> {
    let sets = SETS.lock();
    let set = find_set(&sets, set_id)?;
    Ok(set.count())
}

/// List all active selection sets.
pub fn list_sets() -> Vec<(u64, PathBuf, usize)> {
    let sets = SETS.lock();
    sets.iter()
        .map(|s| (s.id, s.directory.clone(), s.count()))
        .collect()
}

// ---------------------------------------------------------------------------
// Tristate checkbox tree
// ---------------------------------------------------------------------------

/// Build a tristate checkbox tree for a directory.
///
/// Populates a tree structure with files and subdirectories.
/// All nodes start unchecked.
pub fn build_check_tree(path: impl AsRef<Path>) -> KernelResult<CheckTreeNode> {
    let path = path.as_ref();
    // The root has no final component; it names itself.
    let name = path.file_name().unwrap_or(path);
    let meta = crate::fs::vfs::Vfs::metadata(path)?;
    let is_dir = meta.entry_type == crate::fs::EntryType::Directory;

    let children = if is_dir {
        match crate::fs::vfs::Vfs::readdir(path) {
            Ok(entries) => {
                let mut kids = Vec::new();
                for entry in &entries {
                    // `join` collapses the root case and keeps the
                    // entry name's bytes verbatim.
                    let child_path = path.join(&entry.name);
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
        path: path.to_path_buf(),
        name: name.to_path_buf(),
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
    let all_unchecked = node
        .children
        .iter()
        .all(|c| c.state == CheckState::Unchecked);

    node.state = if all_checked {
        CheckState::Checked
    } else if all_unchecked {
        CheckState::Unchecked
    } else {
        CheckState::Partial
    };
}

/// Collect all checked paths from a check tree.
pub fn collect_checked(node: &CheckTreeNode) -> Vec<PathBuf> {
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
fn make_item(path: &Path) -> KernelResult<SelectedItem> {
    let meta = crate::fs::vfs::Vfs::metadata(path)?;
    // The root has no final component; it names itself.
    let name = path.file_name().unwrap_or(path);
    Ok(SelectedItem {
        path: path.to_path_buf(),
        name: name.to_path_buf(),
        is_dir: meta.entry_type == crate::fs::EntryType::Directory,
        size: meta.size,
    })
}

/// Stat a batch of paths through the VFS, dropping the ones that fail.
///
/// This exists so that callers can do their VFS work *before* taking `SETS`,
/// which is not a stylistic preference but the kernel's actual lock order.
/// `Vfs::metadata` resolves through the filesystem's own lock, and the VFS
/// holds that lock across content generation; for procfs, generating content
/// reaches back into arbitrary module-global state. The live order is
/// therefore `filesystem lock -> module state`, and holding `SETS` across a
/// `Vfs::` call would run `SETS -> filesystem lock` -- an AB/BA inversion that
/// wedges two CPUs. `scripts/check-vfs-under-lock.py` enforces this.
///
/// A failed stat is dropped rather than propagated because every batching
/// caller already ignored one: an entry that vanished between the directory
/// listing and the click is not an error for the selection as a whole. The
/// single-path callers (`select_single`, `select_toggle`) call `make_item`
/// directly and keep propagating.
fn make_items(paths: &[&Path]) -> Vec<SelectedItem> {
    paths.iter().filter_map(|p| make_item(p).ok()).collect()
}

/// Set check state recursively on a node and all children.
fn set_state_recursive(node: &mut CheckTreeNode, state: CheckState) {
    node.state = state;
    for child in &mut node.children {
        set_state_recursive(child, state);
    }
}

/// Recursively collect checked leaf paths.
fn collect_checked_recursive(node: &CheckTreeNode, result: &mut Vec<PathBuf>) {
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

/// Simple glob pattern matching (supports `*`, `?`, `[...]` and `\`).
///
/// Matching is over **bytes**, not `char`s.  `text` is a filename, which is an
/// uninterpreted byte string that need not be valid UTF-8, so `chars()` could
/// not be formed for it at all.  The visible consequence is that `?` matches
/// one byte rather than one code point — the same rule a POSIX shell applies
/// in a non-multibyte locale, and the only rule that is total over the names
/// the filesystem accepts.
///
/// Case-sensitive, because the filesystem is.  Delegates to the one shared
/// matcher in `fs::vfs`; the private recursive copy this replaced backtracked
/// by re-entering itself at every `*`, which is exponential on a pattern like
/// `a*a*a*b`.
fn simple_glob(pattern: &str, text: impl AsRef<Path>) -> bool {
    crate::fs::vfs::glob_match(text.as_ref(), pattern, false)
}

/// Format a byte size for display.
///
/// Delegates to [`crate::bytesize::iec`], which every size in the kernel now
/// goes through. The private copy this replaced stopped at GiB and did the
/// division in `f64`, which is silently lossy above 2^53 bytes.
fn format_size(bytes: u64) -> String {
    crate::bytesize::iec(bytes)
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
        // A name that is not valid UTF-8 still matches: the whole point of
        // globbing over bytes.
        assert!(simple_glob("*.rs", Path::new(b"ma\xffn.rs".as_slice())));
        assert!(!simple_glob("*.rs", Path::new(b"ma\xffn.py".as_slice())));
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
            path: PathBuf::from("/test"),
            name: PathBuf::from("test"),
            is_dir: true,
            state: CheckState::Partial,
            children: vec![
                CheckTreeNode {
                    path: PathBuf::from("/test/a"),
                    name: PathBuf::from("a"),
                    is_dir: false,
                    state: CheckState::Checked,
                    children: Vec::new(),
                },
                CheckTreeNode {
                    path: PathBuf::from("/test/b"),
                    name: PathBuf::from("b"),
                    is_dir: false,
                    state: CheckState::Unchecked,
                    children: Vec::new(),
                },
            ],
        };
        let checked = collect_checked(&tree);
        assert_eq!(checked.len(), 1);
        assert_eq!(
            checked.first().map(PathBuf::as_path),
            Some(Path::new("/test/a"))
        );

        // After toggling parent, all should be checked.
        toggle_check_node(&mut tree);
        let checked = collect_checked(&tree);
        // Parent is checked → just the parent path.
        assert_eq!(checked.len(), 1);
        assert_eq!(
            checked.first().map(PathBuf::as_path),
            Some(Path::new("/test"))
        );
        serial_println!("[fileselect] test 7 passed: collect_checked");
    }

    serial_println!("[fileselect] all 7 self-tests passed");
    Ok(())
}

//! File picker / save dialog — common dialog for opening and saving files.
//!
//! Provides the backend for a system-wide file open/save dialog that
//! any application can invoke. Includes directory navigation, filtering,
//! bookmarks, recent files, and preview integration.
//!
//! ## Design Reference
//!
//! design.txt line 926:
//! "used as a file save or file(s) load dialog for applications"
//!
//! design.txt line 927:
//! "view options - list, thumbnails (any size), select fields for
//!  column view, order by any column"
//!
//! ## Architecture
//!
//! ```text
//! Application calls open_file_dialog()
//!   → DialogState created
//!   → User navigates directories, applies filters
//!   → User selects file(s) and confirms
//!   → Dialog returns selected path(s)
//! ```
//!
//! The dialog can operate in several modes:
//! - **OpenFile**: select one existing file
//! - **OpenFiles**: select multiple existing files
//! - **SaveFile**: choose location and name for a new file
//! - **SelectFolder**: select a directory

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum active dialogs.
const MAX_DIALOGS: usize = 64;

/// Maximum filters per dialog.
const MAX_FILTERS: usize = 32;

/// Maximum recent directories tracked.
const MAX_RECENT_DIRS: usize = 32;

/// Maximum items per directory listing.
const MAX_LISTING: usize = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Mode of the file dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogMode {
    /// Select a single file to open.
    OpenFile,
    /// Select multiple files to open.
    OpenFiles,
    /// Choose a path to save a file.
    SaveFile,
    /// Select a directory.
    SelectFolder,
}

impl DialogMode {
    /// Window title for the dialog.
    pub fn title(self) -> &'static str {
        match self {
            Self::OpenFile => "Open File",
            Self::OpenFiles => "Open Files",
            Self::SaveFile => "Save File",
            Self::SelectFolder => "Select Folder",
        }
    }
}

/// View mode for the file listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Large icon grid.
    LargeIcons,
    /// Small icon grid.
    SmallIcons,
    /// Detailed list with columns.
    Details,
    /// Simple name list.
    List,
    /// Tile view.
    Tiles,
}

/// Sort column for the listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    /// Sort by file name.
    Name,
    /// Sort by size.
    Size,
    /// Sort by type/extension.
    Type,
    /// Sort by modification date.
    DateModified,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// A-Z, smallest first, oldest first.
    Ascending,
    /// Z-A, largest first, newest first.
    Descending,
}

/// A file type filter (e.g., "Images (*.png, *.jpg)").
#[derive(Debug, Clone)]
pub struct FileFilter {
    /// Display label (e.g., "Image Files").
    pub label: String,
    /// Extension patterns (e.g., ["png", "jpg", "gif"]).
    pub extensions: Vec<String>,
}

/// An item in the directory listing.
#[derive(Debug, Clone)]
pub struct ListingItem {
    /// File/directory name.
    pub name: String,
    /// Full path.
    pub path: String,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// File size in bytes (0 for directories).
    pub size: u64,
    /// Modification timestamp (nanoseconds).
    pub modified_ns: u64,
    /// MIME type (empty for directories).
    pub mime_type: String,
}

/// Result of a completed dialog.
#[derive(Debug, Clone)]
pub enum DialogResult {
    /// User confirmed selection.
    Confirmed(Vec<String>),
    /// User cancelled.
    Cancelled,
}

/// State of a file dialog instance.
#[derive(Debug, Clone)]
pub struct DialogState {
    /// Unique dialog ID.
    pub id: u64,
    /// Dialog mode.
    pub mode: DialogMode,
    /// Current directory path.
    pub current_dir: String,
    /// Current file name (for SaveFile mode).
    pub filename: String,
    /// Currently selected paths.
    pub selection: Vec<String>,
    /// Available file type filters.
    pub filters: Vec<FileFilter>,
    /// Active filter index.
    pub active_filter: usize,
    /// View mode.
    pub view_mode: ViewMode,
    /// Sort column.
    pub sort_column: SortColumn,
    /// Sort direction.
    pub sort_dir: SortDirection,
    /// Whether to show hidden files.
    pub show_hidden: bool,
    /// Items in current directory.
    pub listing: Vec<ListingItem>,
    /// Navigation history (back stack).
    pub history: Vec<String>,
    /// Whether dialog is still open.
    pub open: bool,
    /// Result once closed.
    pub result: Option<DialogResult>,
}

/// Quick-access bookmark for the file picker sidebar.
#[derive(Debug, Clone)]
pub struct PickerBookmark {
    /// Display label.
    pub label: String,
    /// Directory path.
    pub path: String,
    /// Icon name.
    pub icon: String,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct PickerState {
    /// Dialog ID → state.
    dialogs: Vec<DialogState>,
    /// Recent directories.
    recent_dirs: Vec<String>,
    /// Quick-access bookmarks.
    bookmarks: Vec<PickerBookmark>,
    /// Next dialog ID.
    next_id: u64,
}

impl PickerState {
    const fn new() -> Self {
        Self {
            dialogs: Vec::new(),
            recent_dirs: Vec::new(),
            bookmarks: Vec::new(),
            next_id: 1,
        }
    }
}

static PICKER: Mutex<PickerState> = Mutex::new(PickerState::new());
static OPEN_OPS: AtomicU64 = AtomicU64::new(0);
static NAV_OPS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push((c as u8 + 32) as char);
        } else {
            out.push(c);
        }
    }
    out
}

/// Extract extension from a filename.
fn extension(name: &str) -> &str {
    if let Some(dot) = name.rfind('.') {
        if dot > 0 && dot + 1 < name.len() {
            return &name[dot + 1..];
        }
    }
    ""
}

/// Check if a file matches the active filter.
fn matches_filter(name: &str, filter: &FileFilter) -> bool {
    if filter.extensions.is_empty() {
        return true; // "All Files"
    }
    let ext = to_lower(extension(name));
    filter.extensions.iter().any(|f| to_lower(f) == ext)
}

// ---------------------------------------------------------------------------
// Dialog lifecycle
// ---------------------------------------------------------------------------

/// Create and open a new file dialog.
pub fn create_dialog(mode: DialogMode, start_dir: &str,
                     filters: Vec<FileFilter>) -> KernelResult<u64> {
    if filters.len() > MAX_FILTERS {
        return Err(KernelError::InvalidArgument);
    }
    OPEN_OPS.fetch_add(1, Ordering::Relaxed);

    let mut picker = PICKER.lock();
    if picker.dialogs.len() >= MAX_DIALOGS {
        return Err(KernelError::ResourceExhausted);
    }

    let id = picker.next_id;
    picker.next_id = picker.next_id.saturating_add(1);

    let dir = if start_dir.is_empty() { "/" } else { start_dir };

    let state = DialogState {
        id,
        mode,
        current_dir: String::from(dir),
        filename: String::new(),
        selection: Vec::new(),
        filters,
        active_filter: 0,
        view_mode: ViewMode::Details,
        sort_column: SortColumn::Name,
        sort_dir: SortDirection::Ascending,
        show_hidden: false,
        listing: Vec::new(),
        history: Vec::new(),
        open: true,
        result: None,
    };

    picker.dialogs.push(state);
    Ok(id)
}

/// Get dialog state by ID.
pub fn get_dialog(id: u64) -> Option<DialogState> {
    let picker = PICKER.lock();
    picker.dialogs.iter().find(|d| d.id == id).cloned()
}

/// Navigate to a directory in the dialog.
pub fn navigate(id: u64, path: &str) -> KernelResult<()> {
    NAV_OPS.fetch_add(1, Ordering::Relaxed);
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    if !dialog.open {
        return Err(KernelError::InvalidArgument);
    }

    // Push current to history.
    dialog.history.push(dialog.current_dir.clone());
    dialog.current_dir = String::from(path);
    dialog.selection.clear();

    // Refresh listing from VFS.
    dialog.listing = build_listing(path, dialog.show_hidden,
        dialog.filters.get(dialog.active_filter));

    // Sort listing.
    sort_listing(&mut dialog.listing, dialog.sort_column, dialog.sort_dir);

    Ok(())
}

/// Go back in navigation history.
pub fn go_back(id: u64) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    if let Some(prev) = dialog.history.pop() {
        dialog.current_dir = prev.clone();
        dialog.selection.clear();
        dialog.listing = build_listing(&prev, dialog.show_hidden,
            dialog.filters.get(dialog.active_filter));
        sort_listing(&mut dialog.listing, dialog.sort_column, dialog.sort_dir);
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// Navigate up one directory.
pub fn go_up(id: u64) -> KernelResult<()> {
    let picker = PICKER.lock();
    let dialog = picker.dialogs.iter().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    let current = dialog.current_dir.clone();
    drop(picker);

    // Find parent directory.
    let parent = if current == "/" {
        return Ok(()); // Already at root.
    } else if let Some(slash) = current.rfind('/') {
        if slash == 0 { "/" } else { &current[..slash] }
    } else {
        "/"
    };

    navigate(id, parent)
}

/// Select a file or directory in the dialog.
pub fn select(id: u64, path: &str) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    match dialog.mode {
        DialogMode::OpenFile | DialogMode::SaveFile | DialogMode::SelectFolder => {
            dialog.selection.clear();
            dialog.selection.push(String::from(path));
        }
        DialogMode::OpenFiles => {
            if !dialog.selection.iter().any(|s| s == path) {
                dialog.selection.push(String::from(path));
            }
        }
    }
    Ok(())
}

/// Deselect a file.
pub fn deselect(id: u64, path: &str) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.selection.retain(|s| s != path);
    Ok(())
}

/// Set the filename in the input field (SaveFile mode).
pub fn set_filename(id: u64, name: &str) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.filename = String::from(name);
    Ok(())
}

/// Change the active filter.
pub fn set_filter(id: u64, filter_idx: usize) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    if filter_idx >= dialog.filters.len() {
        return Err(KernelError::InvalidArgument);
    }
    dialog.active_filter = filter_idx;

    // Refresh listing with new filter.
    let dir = dialog.current_dir.clone();
    let show_hidden = dialog.show_hidden;
    let filter = dialog.filters.get(dialog.active_filter).cloned();
    dialog.listing = build_listing(&dir, show_hidden, filter.as_ref());
    sort_listing(&mut dialog.listing, dialog.sort_column, dialog.sort_dir);
    Ok(())
}

/// Change sort column and direction.
pub fn set_sort(id: u64, column: SortColumn, dir: SortDirection) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.sort_column = column;
    dialog.sort_dir = dir;
    sort_listing(&mut dialog.listing, column, dir);
    Ok(())
}

/// Change view mode.
pub fn set_view_mode(id: u64, mode: ViewMode) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.view_mode = mode;
    Ok(())
}

/// Toggle hidden files.
pub fn toggle_hidden(id: u64) -> KernelResult<bool> {
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.show_hidden = !dialog.show_hidden;
    let show = dialog.show_hidden;

    // Refresh listing.
    let dir = dialog.current_dir.clone();
    let filter = dialog.filters.get(dialog.active_filter).cloned();
    dialog.listing = build_listing(&dir, show, filter.as_ref());
    sort_listing(&mut dialog.listing, dialog.sort_column, dialog.sort_dir);
    Ok(show)
}

/// Confirm the dialog (OK/Open/Save button).
pub fn confirm(id: u64) -> KernelResult<DialogResult> {
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    if !dialog.open {
        return Err(KernelError::InvalidArgument);
    }

    let result = match dialog.mode {
        DialogMode::SaveFile => {
            // Build full path from current_dir + filename.
            let path = if dialog.filename.is_empty() {
                return Err(KernelError::InvalidArgument);
            } else if dialog.current_dir == "/" {
                alloc::format!("/{}", dialog.filename)
            } else {
                alloc::format!("{}/{}", dialog.current_dir, dialog.filename)
            };
            DialogResult::Confirmed(alloc::vec![path])
        }
        _ => {
            if dialog.selection.is_empty() {
                return Err(KernelError::InvalidArgument);
            }
            DialogResult::Confirmed(dialog.selection.clone())
        }
    };

    // Record directory in recent.
    let dir = dialog.current_dir.clone();
    dialog.open = false;
    dialog.result = Some(result.clone());

    // Update recent dirs.
    picker.recent_dirs.retain(|d| d != &dir);
    picker.recent_dirs.insert(0, dir);
    if picker.recent_dirs.len() > MAX_RECENT_DIRS {
        picker.recent_dirs.truncate(MAX_RECENT_DIRS);
    }

    Ok(result)
}

/// Cancel the dialog.
pub fn cancel(id: u64) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker.dialogs.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.open = false;
    dialog.result = Some(DialogResult::Cancelled);
    Ok(())
}

/// Close and remove a completed dialog.
pub fn close(id: u64) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let idx = picker.dialogs.iter().position(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    picker.dialogs.remove(idx);
    Ok(())
}

// ---------------------------------------------------------------------------
// Listing helpers
// ---------------------------------------------------------------------------

/// Build a directory listing from VFS.
fn build_listing(dir: &str, show_hidden: bool,
                 filter: Option<&FileFilter>) -> Vec<ListingItem> {
    use crate::fs::vfs::Vfs;

    let entries = match Vfs::readdir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut items = Vec::new();
    for entry in entries.iter().take(MAX_LISTING) {
        // Skip hidden files if not showing them.
        if !show_hidden && entry.name.starts_with('.') {
            continue;
        }

        let is_dir = entry.entry_type == crate::fs::EntryType::Directory;

        // Apply filter (only to files, not directories).
        if !is_dir {
            if let Some(f) = filter {
                if !matches_filter(&entry.name, f) {
                    continue;
                }
            }
        }

        let path = if dir == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", dir, entry.name)
        };

        items.push(ListingItem {
            name: entry.name.clone(),
            path,
            is_dir,
            size: entry.size,
            modified_ns: 0, // Would come from stat() but avoid per-file stat overhead.
            mime_type: String::new(),
        });
    }

    items
}

/// Sort a listing in place.
fn sort_listing(items: &mut [ListingItem], column: SortColumn, dir: SortDirection) {
    // Directories always come first.
    items.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            return b.is_dir.cmp(&a.is_dir); // Dirs first.
        }
        let cmp = match column {
            SortColumn::Name => a.name.cmp(&b.name),
            SortColumn::Size => a.size.cmp(&b.size),
            SortColumn::Type => extension(&a.name).cmp(extension(&b.name)),
            SortColumn::DateModified => a.modified_ns.cmp(&b.modified_ns),
        };
        match dir {
            SortDirection::Ascending => cmp,
            SortDirection::Descending => cmp.reverse(),
        }
    });
}

// ---------------------------------------------------------------------------
// Bookmarks and recent
// ---------------------------------------------------------------------------

/// Add a quick-access bookmark.
pub fn add_bookmark(label: &str, path: &str, icon: &str) -> KernelResult<()> {
    if label.is_empty() || path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut picker = PICKER.lock();
    if picker.bookmarks.iter().any(|b| b.path == path) {
        return Err(KernelError::AlreadyExists);
    }
    picker.bookmarks.push(PickerBookmark {
        label: String::from(label),
        path: String::from(path),
        icon: String::from(icon),
    });
    Ok(())
}

/// Remove a bookmark.
pub fn remove_bookmark(path: &str) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let idx = picker.bookmarks.iter().position(|b| b.path == path)
        .ok_or(KernelError::NotFound)?;
    picker.bookmarks.remove(idx);
    Ok(())
}

/// Get bookmarks.
pub fn bookmarks() -> Vec<PickerBookmark> {
    let picker = PICKER.lock();
    picker.bookmarks.clone()
}

/// Get recent directories.
pub fn recent_dirs() -> Vec<String> {
    let picker = PICKER.lock();
    picker.recent_dirs.clone()
}

/// Initialize default bookmarks.
pub fn init_defaults() {
    let defaults = [
        ("Home", "/home", "icon-home"),
        ("Desktop", "/home/desktop", "icon-desktop"),
        ("Documents", "/home/documents", "icon-documents"),
        ("Downloads", "/home/downloads", "icon-downloads"),
        ("Pictures", "/home/pictures", "icon-pictures"),
        ("Music", "/home/music", "icon-music"),
        ("Videos", "/home/videos", "icon-video"),
    ];
    for (label, path, icon) in &defaults {
        let _ = add_bookmark(label, path, icon);
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (active_dialogs, total_dialogs, bookmarks, recent_dirs, open_ops, nav_ops).
pub fn stats() -> (usize, usize, usize, usize, u64, u64) {
    let picker = PICKER.lock();
    let active = picker.dialogs.iter().filter(|d| d.open).count();
    (
        active,
        picker.dialogs.len(),
        picker.bookmarks.len(),
        picker.recent_dirs.len(),
        OPEN_OPS.load(Ordering::Relaxed),
        NAV_OPS.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    OPEN_OPS.store(0, Ordering::Relaxed);
    NAV_OPS.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut picker = PICKER.lock();
    picker.dialogs.clear();
    picker.recent_dirs.clear();
    picker.bookmarks.clear();
    picker.next_id = 1;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the file picker.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: create dialog.
    {
        let id = create_dialog(DialogMode::OpenFile, "/", Vec::new())?;
        assert!(id > 0);
        let d = get_dialog(id).unwrap();
        assert_eq!(d.mode, DialogMode::OpenFile);
        assert_eq!(d.current_dir, "/");
        assert!(d.open);
        serial_println!("[filepicker] test 1 passed: create dialog");
    }

    // Test 2: navigate.
    {
        let id = create_dialog(DialogMode::OpenFile, "/", Vec::new())?;
        // Navigate somewhere (may fail if directory doesn't exist in VFS, that's OK).
        let _ = navigate(id, "/tmp");
        let d = get_dialog(id).unwrap();
        assert_eq!(d.current_dir, "/tmp");
        assert!(!d.history.is_empty());
        serial_println!("[filepicker] test 2 passed: navigate");
    }

    // Test 3: file filter.
    {
        let filters = alloc::vec![
            FileFilter {
                label: String::from("Text Files"),
                extensions: alloc::vec![String::from("txt"), String::from("md")],
            },
            FileFilter {
                label: String::from("All Files"),
                extensions: Vec::new(),
            },
        ];
        let id = create_dialog(DialogMode::OpenFile, "/", filters)?;
        let d = get_dialog(id).unwrap();
        assert_eq!(d.filters.len(), 2);
        assert_eq!(d.active_filter, 0);

        // Test filter matching.
        let f = &d.filters[0];
        assert!(matches_filter("readme.txt", f));
        assert!(matches_filter("README.TXT", f));
        assert!(!matches_filter("photo.png", f));
        serial_println!("[filepicker] test 3 passed: filters");
    }

    // Test 4: selection.
    {
        let id = create_dialog(DialogMode::OpenFiles, "/", Vec::new())?;
        select(id, "/file1.txt")?;
        select(id, "/file2.txt")?;
        let d = get_dialog(id).unwrap();
        assert_eq!(d.selection.len(), 2);

        deselect(id, "/file1.txt")?;
        let d = get_dialog(id).unwrap();
        assert_eq!(d.selection.len(), 1);
        serial_println!("[filepicker] test 4 passed: selection");
    }

    // Test 5: save file dialog.
    {
        let id = create_dialog(DialogMode::SaveFile, "/home", Vec::new())?;
        set_filename(id, "output.txt")?;
        let result = confirm(id)?;
        match result {
            DialogResult::Confirmed(paths) => {
                assert_eq!(paths.len(), 1);
                assert_eq!(paths[0], "/home/output.txt");
            }
            _ => panic!("Expected Confirmed"),
        }
        serial_println!("[filepicker] test 5 passed: save dialog");
    }

    // Test 6: cancel dialog.
    {
        let id = create_dialog(DialogMode::OpenFile, "/", Vec::new())?;
        cancel(id)?;
        let d = get_dialog(id).unwrap();
        assert!(!d.open);
        match d.result {
            Some(DialogResult::Cancelled) => {}
            _ => panic!("Expected Cancelled"),
        }
        serial_println!("[filepicker] test 6 passed: cancel");
    }

    // Test 7: bookmarks.
    {
        add_bookmark("Home", "/home", "icon-home")?;
        add_bookmark("Documents", "/home/documents", "icon-docs")?;
        let bms = bookmarks();
        assert_eq!(bms.len(), 2);
        assert_eq!(bms[0].label, "Home");

        // Duplicate should fail.
        assert!(add_bookmark("Home2", "/home", "icon-home").is_err());

        remove_bookmark("/home")?;
        let bms = bookmarks();
        assert_eq!(bms.len(), 1);
        serial_println!("[filepicker] test 7 passed: bookmarks");
    }

    clear_all();
    reset_stats();

    serial_println!("[filepicker] all 7 self-tests passed");
    Ok(())
}

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

use crate::fs::path::{Path, PathBuf};
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    ///
    /// These stay text because they are declared by the *application* that
    /// opens the dialog, not read off the disk.  The names they are matched
    /// against are raw bytes, so the comparison is byte-exact (ASCII-folded):
    /// a file whose extension is not valid UTF-8 simply matches no filter,
    /// which is the truthful answer, rather than its name being lossily
    /// decoded to make it match.
    pub extensions: Vec<String>,
}

/// An item in the directory listing.
#[derive(Debug, Clone)]
pub struct ListingItem {
    /// File/directory name.
    pub name: PathBuf,
    /// Full path.
    pub path: PathBuf,
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
    Confirmed(Vec<PathBuf>),
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
    pub current_dir: PathBuf,
    /// Current file name (for SaveFile mode).
    pub filename: PathBuf,
    /// Currently selected paths.
    pub selection: Vec<PathBuf>,
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
    pub history: Vec<PathBuf>,
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
    pub path: PathBuf,
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
    recent_dirs: Vec<PathBuf>,
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

/// Check if a file matches the active filter.
///
/// The extension comes from [`Path::extension`], which - unlike the
/// hand-rolled `rfind('.')` this replaces - declines to call a leading dot an
/// extension, so `.bashrc` is a dotfile rather than a file of type `bashrc`.
///
/// Folding is ASCII-only: a filename carries no declared encoding, so there is
/// nothing to consult that would say how to fold a byte >= 0x80, and folding
/// one by guessing an encoding makes two distinct names collide.
fn matches_filter(name: &Path, filter: &FileFilter) -> bool {
    if filter.extensions.is_empty() {
        return true; // "All Files"
    }
    let ext = name.extension().map_or(&[][..], Path::as_bytes);
    filter
        .extensions
        .iter()
        .any(|f| ext.eq_ignore_ascii_case(f.as_bytes()))
}

// ---------------------------------------------------------------------------
// Dialog lifecycle
// ---------------------------------------------------------------------------

/// Create and open a new file dialog.
pub fn create_dialog<P: AsRef<Path> + ?Sized>(
    mode: DialogMode,
    start_dir: &P,
    filters: Vec<FileFilter>,
) -> KernelResult<u64> {
    let start_dir = start_dir.as_ref();
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

    let dir = if start_dir.is_empty() {
        Path::new("/")
    } else {
        start_dir
    };

    let state = DialogState {
        id,
        mode,
        current_dir: dir.to_path_buf(),
        filename: PathBuf::new(),
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
pub fn navigate<P: AsRef<Path> + ?Sized>(id: u64, path: &P) -> KernelResult<()> {
    let path = path.as_ref();
    NAV_OPS.fetch_add(1, Ordering::Relaxed);
    let mut picker = PICKER.lock();
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    if !dialog.open {
        return Err(KernelError::InvalidArgument);
    }

    // Push current to history.
    dialog.history.push(dialog.current_dir.clone());
    dialog.current_dir = path.to_path_buf();
    dialog.selection.clear();

    // Refresh listing from VFS.
    dialog.listing = build_listing(
        path,
        dialog.show_hidden,
        dialog.filters.get(dialog.active_filter),
    );

    // Sort listing.
    sort_listing(&mut dialog.listing, dialog.sort_column, dialog.sort_dir);

    Ok(())
}

/// Go back in navigation history.
pub fn go_back(id: u64) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    if let Some(prev) = dialog.history.pop() {
        dialog.current_dir = prev.clone();
        dialog.selection.clear();
        dialog.listing = build_listing(
            &prev,
            dialog.show_hidden,
            dialog.filters.get(dialog.active_filter),
        );
        sort_listing(&mut dialog.listing, dialog.sort_column, dialog.sort_dir);
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// Navigate up one directory.
pub fn go_up(id: u64) -> KernelResult<()> {
    let picker = PICKER.lock();
    let dialog = picker
        .dialogs
        .iter()
        .find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    let current = dialog.current_dir.clone();
    drop(picker);

    // `Path::parent` yields `/` for a top-level directory and `None` at the
    // root, which is exactly the three-armed `rfind('/')` match this replaces -
    // including the "already at root, do nothing" case.
    let Some(parent) = current.parent() else {
        return Ok(());
    };
    let parent = parent.to_path_buf();

    navigate(id, &parent)
}

/// Select a file or directory in the dialog.
pub fn select<P: AsRef<Path> + ?Sized>(id: u64, path: &P) -> KernelResult<()> {
    let path = path.as_ref();
    let mut picker = PICKER.lock();
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    match dialog.mode {
        DialogMode::OpenFile | DialogMode::SaveFile | DialogMode::SelectFolder => {
            dialog.selection.clear();
            dialog.selection.push(path.to_path_buf());
        }
        DialogMode::OpenFiles => {
            if !dialog.selection.iter().any(|s| s.as_path() == path) {
                dialog.selection.push(path.to_path_buf());
            }
        }
    }
    Ok(())
}

/// Deselect a file.
pub fn deselect<P: AsRef<Path> + ?Sized>(id: u64, path: &P) -> KernelResult<()> {
    let path = path.as_ref();
    let mut picker = PICKER.lock();
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.selection.retain(|s| s.as_path() != path);
    Ok(())
}

/// Set the filename in the input field (SaveFile mode).
pub fn set_filename<P: AsRef<Path> + ?Sized>(id: u64, name: &P) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.filename = name.as_ref().to_path_buf();
    Ok(())
}

/// Change the active filter.
pub fn set_filter(id: u64, filter_idx: usize) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
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
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.sort_column = column;
    dialog.sort_dir = dir;
    sort_listing(&mut dialog.listing, column, dir);
    Ok(())
}

/// Change view mode.
pub fn set_view_mode(id: u64, mode: ViewMode) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.view_mode = mode;
    Ok(())
}

/// Toggle hidden files.
pub fn toggle_hidden(id: u64) -> KernelResult<bool> {
    let mut picker = PICKER.lock();
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
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
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    if !dialog.open {
        return Err(KernelError::InvalidArgument);
    }

    let result = match dialog.mode {
        DialogMode::SaveFile => {
            // Build full path from current_dir + filename.
            if dialog.filename.is_empty() {
                return Err(KernelError::InvalidArgument);
            }
            // `PathBuf::push` inserts exactly one separator, so the
            // `current_dir == "/"` arm that existed only to avoid doubling it
            // is gone.  It also lets an absolute filename replace the
            // directory outright, which is what a user typing a full path into
            // the name box means.
            let mut path = dialog.current_dir.clone();
            path.push(&dialog.filename);
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
    let dialog = picker
        .dialogs
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    dialog.open = false;
    dialog.result = Some(DialogResult::Cancelled);
    Ok(())
}

/// Close and remove a completed dialog.
pub fn close(id: u64) -> KernelResult<()> {
    let mut picker = PICKER.lock();
    let idx = picker
        .dialogs
        .iter()
        .position(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    picker.dialogs.remove(idx);
    Ok(())
}

// ---------------------------------------------------------------------------
// Listing helpers
// ---------------------------------------------------------------------------

/// Build a directory listing from VFS.
fn build_listing(dir: &Path, show_hidden: bool, filter: Option<&FileFilter>) -> Vec<ListingItem> {
    use crate::fs::vfs::Vfs;

    let entries = match Vfs::readdir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut items = Vec::new();
    for entry in entries.iter().take(MAX_LISTING) {
        // Skip hidden files if not showing them.  Byte compare, not
        // `Path::starts_with`: the latter matches whole components, so it
        // would ask whether the name *is* `.`.
        if !show_hidden && entry.name.as_bytes().starts_with(b".") {
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

        // `Path::join` collapses the root case itself, so the `dir == "/"`
        // arm that existed only to avoid a doubled separator is gone.
        let path = dir.join(&entry.name);

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
            SortColumn::Type => a.name.extension().cmp(&b.name.extension()),
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
pub fn add_bookmark<P: AsRef<Path> + ?Sized>(
    label: &str,
    path: &P,
    icon: &str,
) -> KernelResult<()> {
    let path = path.as_ref();
    if label.is_empty() || path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut picker = PICKER.lock();
    if picker.bookmarks.iter().any(|b| b.path.as_path() == path) {
        return Err(KernelError::AlreadyExists);
    }
    picker.bookmarks.push(PickerBookmark {
        label: String::from(label),
        path: path.to_path_buf(),
        icon: String::from(icon),
    });
    Ok(())
}

/// Remove a bookmark.
pub fn remove_bookmark<P: AsRef<Path> + ?Sized>(path: &P) -> KernelResult<()> {
    let path = path.as_ref();
    let mut picker = PICKER.lock();
    let idx = picker
        .bookmarks
        .iter()
        .position(|b| b.path.as_path() == path)
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
pub fn recent_dirs() -> Vec<PathBuf> {
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
        assert_eq!(d.current_dir, PathBuf::from("/"));
        assert!(d.open);
        serial_println!("[filepicker] test 1 passed: create dialog");
    }

    // Test 2: navigate.
    {
        let id = create_dialog(DialogMode::OpenFile, "/", Vec::new())?;
        // Navigate somewhere (may fail if directory doesn't exist in VFS, that's OK).
        let _ = navigate(id, "/tmp");
        let d = get_dialog(id).unwrap();
        assert_eq!(d.current_dir, PathBuf::from("/tmp"));
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
        assert!(matches_filter(Path::new("readme.txt"), f));
        assert!(matches_filter(Path::new("README.TXT"), f));
        assert!(!matches_filter(Path::new("photo.png"), f));
        // A dotfile has no extension, so it is not a file of type `bashrc`.
        assert!(!matches_filter(Path::new(".txt"), f));
        // A name that does not decode as UTF-8 still has an extension, and it
        // matches on the bytes.  Before the byte-path conversion this file
        // could not even be represented here, let alone filtered.
        assert!(matches_filter(Path::new(b"re\xffport.txt".as_slice()), f));
        assert!(!matches_filter(Path::new(b"re\xffport.png".as_slice()), f));
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
                assert_eq!(paths[0], PathBuf::from("/home/output.txt"));
            }
            _ => panic!("Expected Confirmed"),
        }
        serial_println!("[filepicker] test 5 passed: save dialog");
    }

    // Test 5b: a save name that is not valid UTF-8 round-trips.  The whole
    // dialog used to be `String`-typed, so such a name could not be entered,
    // joined onto the directory, or returned to the calling application.
    {
        let id = create_dialog(DialogMode::SaveFile, "/home", Vec::new())?;
        set_filename(id, b"dr\xffaft.txt".as_slice())?;
        match confirm(id)? {
            DialogResult::Confirmed(paths) => {
                assert_eq!(paths.len(), 1);
                assert_eq!(paths[0], PathBuf::from(b"/home/dr\xffaft.txt".as_slice()));
            }
            DialogResult::Cancelled => panic!("Expected Confirmed"),
        }
        // The directory just used must now head the recent list.
        assert_eq!(recent_dirs().first(), Some(&PathBuf::from("/home")));
        serial_println!("[filepicker] test 5b passed: non-UTF-8 save name");
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

    serial_println!("[filepicker] all 8 self-tests passed");
    Ok(())
}

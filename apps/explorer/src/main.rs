//! Slate OS File Explorer
//!
//! Graphical file manager with:
//! - Directory tree sidebar
//! - File/folder list with an extensible column set (see [`columns`]): the
//!   built-in Name/Size/Date/Type, plus whatever the directory's contents
//!   warrant — image dimensions, audio duration, source line counts
//! - Address bar with path navigation
//! - Toolbar (back, forward, up, new folder, delete, rename)
//! - Status bar (item count, selected size)
//! - Sort by name/size/date/type
//! - File operations: copy, cut, paste, delete, rename
//! - View modes: list, grid/icon, details
//! - Keyboard navigation
//! - Recycle bin integration
//! - File type associations
//!
//! Uses the guitk library for UI rendering.

// `Duration::from_days` / `from_hours` / `from_mins` — the constructors this
// lint asks for — are still nightly-gated (rust-lang/rust#120301). Taking the
// suggestion would pin the explorer to a nightly toolchain in exchange for a
// nicer-looking literal, so the seconds spelling stays and each site spells
// out the arithmetic that names the unit.
#![allow(clippy::duration_suboptimal_units)]

mod columns;
mod dropzone;
mod fileops;
mod thumbs;

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::listview::ListViewport;
use guitk::modal::{AlertDialog, DialogResult, InputDialog};
use guitk::render::RenderTree;
use guitk::scroll_window;
use guitk::scrollbar;
use guitk::wheel::Accumulator as WheelAccumulator;

use columns::{ColumnId, ColumnManager, ColumnValue, FileInfo, SortOrder};
use dropzone::{
    DragModifiers, DropOperation, DropResult, DropZone, DropZoneEvent, DropZoneManager, Rect,
};
use fileops::{
    ConflictPolicy, ErrorPolicy, FileOpEvent, FileOperation, OperationExecutor, OperationPlan,
    OperationSummary, RecycleBin, UndoStack, UndoTarget,
};
use thumbs::{
    ThumbCategory, ThumbConfig, Thumbnail, ThumbnailCache, ThumbnailGenerator, ThumbnailRequest,
};

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ============================================================================
// File entry
// ============================================================================

/// A file or directory entry displayed in the explorer.
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub file_type: FileType,
    pub selected: bool,
    pub icon_id: u32,
}

impl FileEntry {
    /// Label for the detail view's Type column.
    ///
    /// The category when the entry has a recognised one, and the bare
    /// extension otherwise, so an unrecognised `.qcow2` reads "QCOW2 File"
    /// rather than a bare "File" that says nothing.
    fn type_label(&self) -> String {
        if self.file_type != FileType::Unknown {
            return self.file_type.label().to_string();
        }
        match self.path.extension().and_then(|e| e.to_str()) {
            Some(ext) if !ext.is_empty() => format!("{} File", ext.to_uppercase()),
            _ => "File".to_string(),
        }
    }
}

/// Known file types for icon/association purposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Directory,
    Text,
    Image,
    Audio,
    Video,
    Archive,
    Executable,
    Document,
    Code,
    Unknown,
}

impl FileType {
    /// Determine file type from extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "txt" | "log" | "md" | "rst" => Self::Text,
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" => Self::Image,
            "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" => Self::Audio,
            "mp4" | "avi" | "mkv" | "webm" | "mov" | "flv" => Self::Video,
            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => Self::Archive,
            "exe" | "bin" | "sh" | "cmd" | "bat" => Self::Executable,
            "pdf" | "doc" | "docx" | "odt" | "xls" | "xlsx" => Self::Document,
            "rs" | "c" | "h" | "cpp" | "py" | "js" | "ts" | "html" | "css" | "java" | "go"
            | "toml" | "yaml" | "json" | "xml" => Self::Code,
            _ => Self::Unknown,
        }
    }

    /// Human-readable category name, as shown in the detail view's Type
    /// column.
    pub fn label(self) -> &'static str {
        match self {
            Self::Directory => "Folder",
            Self::Text => "Text Document",
            Self::Image => "Image",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Archive => "Archive",
            Self::Executable => "Application",
            Self::Document => "Document",
            Self::Code => "Source File",
            Self::Unknown => "File",
        }
    }

    /// Icon character for this file type (unicode placeholder).
    pub fn icon_char(self) -> char {
        match self {
            Self::Directory => '\u{1F4C1}', // folder
            Self::Text => '\u{1F4C4}',      // page
            Self::Image => '\u{1F5BC}',     // framed picture
            Self::Audio => '\u{1F3B5}',     // musical note
            Self::Video => '\u{1F3AC}',     // clapper board
            Self::Archive => '\u{1F4E6}',   // package
            Self::Executable => '\u{2699}', // gear
            Self::Document => '\u{1F4D1}',  // bookmark tabs
            Self::Code => '\u{1F4BB}',      // computer
            Self::Unknown => '\u{1F4C3}',   // page with curl
        }
    }
}

// ============================================================================
// View mode
// ============================================================================

/// How files are displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Details, // Table with columns
    List,    // Simple list
    Icons,   // Grid of icons
}

/// Height of the detail view's header bar. Matches the column system's own
/// `HEADER_HEIGHT`, which is what actually draws it.
const HEADER_H: f32 = 22.0;

/// Height of one detail-view row.
const ROW_H: f32 = 22.0;

/// Width reserved to the left of the first column for the entry's type icon.
const ICON_GUTTER: f32 = 28.0;

/// Width of one icon-view cell.
const ICON_CELL_W: f32 = 96.0;

/// Height of one icon-view cell: the thumbnail box, the gap, and two lines of
/// name beneath it.
const ICON_CELL_H: f32 = 108.0;

/// Side of the square a thumbnail is fitted into, inside its cell.
const ICON_THUMB_SIZE: f32 = 64.0;

/// Font size of the name label under an icon-view thumbnail.
const ICON_LABEL_SIZE: f32 = 11.0;

/// Height of one list-view row: taller than a detail row because it carries a
/// small thumbnail rather than a glyph.
const LIST_ROW_H: f32 = 32.0;

/// Side of the square thumbnail at the left of a list-view row.
const LIST_THUMB_SIZE: f32 = 24.0;

/// Height of one sidebar quick-access row.
const SIDEBAR_ROW_H: f32 = 24.0;

/// The sidebar's quick-access entries: the label drawn, and the directory it
/// stands for.
///
/// The two are separate fields rather than one string because `"/ (Root)"` is
/// not a path. They were one before this became a drop target, which was
/// harmless while the label was only ever drawn — and would have meant dropping
/// a file into a directory literally named `/ (Root)` the moment it was not.
const SIDEBAR_ITEMS: [(&str, &str); 5] = [
    ("/ (Root)", "/"),
    ("/home", "/home"),
    ("/tmp", "/tmp"),
    ("/var", "/var"),
    ("/usr", "/usr"),
];

/// How many thumbnails [`ExplorerState::pump_thumbnails`] generates per call
/// when the caller does not say.
///
/// Generation is synchronous, so this is a frame-budget knob rather than a
/// throughput one: a folder of ten thousand files must not stall the first
/// frame while every one of them is decoded. Eight is roughly one screenful of
/// icon cells per frame, which fills the visible grid within a few frames of
/// arriving and leaves the rest to trickle in behind the scroll.
const THUMB_BATCH_DEFAULT: usize = 8;

/// Sort criteria.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Size,
    Modified,
    Type,
}

/// Sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Ascending,
    Descending,
}

// ============================================================================
// Clipboard
// ============================================================================

/// What a file operation did, and whether the user must be made to see it.
///
/// Operations used to hand back a single formatted `String` that every caller
/// dropped into the status bar, so "Deleted 5 item(s)" and "Deleted 3
/// item(s), 2 failed -- /etc/x: permission denied" arrived in the same place,
/// in the same colour, and both vanished at the next click. A destructive
/// operation that half-failed is precisely the thing a user must not miss,
/// and the status bar is where things go to be missed.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Outcome {
    /// The status-bar line. Always present, success or failure.
    message: String,
    /// Present when some part of the operation did not happen. Its content is
    /// what the dialog says; the `message` still goes to the status bar, so
    /// the record survives after the dialog is dismissed.
    failure: Option<String>,
}

impl Outcome {
    /// Everything asked for happened.
    fn ok(message: String) -> Self {
        Self {
            message,
            failure: None,
        }
    }

    /// Nothing happened, or not all of it did.
    fn failed(message: String, detail: String) -> Self {
        Self {
            message,
            failure: Some(detail),
        }
    }
}

/// A modal the file manager is waiting on, and what to do when it answers.
///
/// One field rather than one `Option` per dialog kind: only one modal can be
/// up at a time -- that is what modal means -- and separate fields would make
/// "a delete confirmation and a rename box, both open" a representable state
/// that every reader has to rule out by hand.
enum Modal {
    /// A destructive action the user has been asked to confirm.
    Confirm {
        dialog: AlertDialog,
        action: PendingAction,
    },
    /// Something went wrong, and the user is being told so they cannot miss
    /// it. Its only answer is "OK" and nothing acts on it.
    Notice { dialog: AlertDialog },
    /// A rename in progress, awaiting the new name.
    Rename {
        dialog: InputDialog,
        /// The file being renamed, by path rather than by row index.
        ///
        /// A row index is only meaningful against the listing that produced
        /// it, and the listing is reloaded on every operation; an index held
        /// across a modal names whatever has since moved into that row. The
        /// path is the stable identifier, and it is looked up when the dialog
        /// answers.
        target: PathBuf,
    },
}

/// What a confirmation carries out if it is confirmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingAction {
    /// Move the selection to the recycle bin, where it can be restored.
    Recycle,
    /// Erase the selection outright. There is no undo for this one, which is
    /// why its dialog says so.
    DeletePermanently,
}

/// File operation pending in clipboard.
#[derive(Clone, Debug)]
pub enum ClipboardOp {
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
}

// ============================================================================
// Drag state
// ============================================================================

/// A drag in flight over the explorer window.
///
/// The zone, operation and validity are cached here rather than recomputed
/// while drawing, because deciding validity touches the filesystem — it
/// canonicalises both ends so that a nested drop reached through a symlink is
/// still caught, and it stats every source against the target to find
/// conflicts. A frame is drawn far more often than the pointer crosses a zone
/// boundary, so recomputing per frame would turn a hover into a syscall storm
/// while also making `render` need `&mut` for a reason that has nothing to do
/// with rendering.
#[derive(Clone, Debug)]
pub struct DragState {
    /// Files being dragged, as handed over by the source of the drag.
    pub sources: Vec<PathBuf>,
    /// Last known pointer position, in window coordinates.
    pub x: f32,
    pub y: f32,
    /// Modifier keys as of the last pointer movement.
    pub modifiers: DragModifiers,
    /// The zone the cached `operation`/`valid` were computed for.
    zone: DropZone,
    /// What releasing here would do.
    operation: DropOperation,
    /// Whether releasing here would be allowed.
    valid: bool,
    /// Why not, when `valid` is false — shown instead of the operation label.
    invalid_reason: Option<String>,
}

impl DragState {
    /// What releasing the drag at its current position would do.
    pub fn operation(&self) -> DropOperation {
        self.operation
    }

    /// Whether releasing the drag at its current position is allowed.
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Why the drop is disallowed, when it is.
    pub fn invalid_reason(&self) -> Option<&str> {
        self.invalid_reason.as_deref()
    }
}

// ============================================================================
// Explorer state
// ============================================================================

/// File explorer application state.
pub struct ExplorerState {
    /// Current directory.
    pub current_path: PathBuf,
    /// Entries in current directory.
    pub entries: Vec<FileEntry>,
    /// Which rows are on screen, and the cursor row the keyboard drags with it.
    ///
    /// Explorer keeps its own `selected_indices` for the multi-selection --
    /// Ctrl+A picks everything, and a viewport has one `selected` -- so this is
    /// used for *scrolling*, with the cursor row synced into it. A viewport
    /// that also owned the selection would need an anchor/extend notion, which
    /// is a file-manager concern rather than a list-widget one.
    pub viewport: ListViewport,
    /// Fractional wheel notches not yet spent on a row.
    ///
    /// Without it a trackpad, which sends many small deltas, scrolls not at
    /// all: each one truncates to zero rows on its own.
    wheel: WheelAccumulator,
    /// How far below the thumb's own top the pointer took hold, while dragging.
    ///
    /// Kept so the thumb follows the grab point rather than jumping its top to
    /// the pointer on the first move.
    thumb_grab: Option<f32>,
    /// Navigation history (back stack).
    pub history_back: VecDeque<PathBuf>,
    /// Navigation history (forward stack).
    pub history_forward: VecDeque<PathBuf>,
    /// View mode.
    pub view_mode: ViewMode,
    /// Sort criteria.
    pub sort_by: SortBy,
    /// Sort direction.
    pub sort_dir: SortDir,
    /// Show hidden files (names starting with '.').
    pub show_hidden: bool,
    /// Clipboard.
    pub clipboard: Option<ClipboardOp>,
    /// Selected entry indices.
    pub selected_indices: Vec<usize>,
    /// Address bar text (for editing).
    pub address_text: String,
    /// Whether address bar is being edited.
    pub address_editing: bool,
    /// Transient result of the last user-initiated operation.
    ///
    /// Kept separate from [`Self::dir_summary`] because every operation ends by
    /// reloading the directory, and the reload recomputes the summary. When
    /// both lived in one field the summary overwrote the result one line later,
    /// so no paste, delete, rename or error message was ever actually seen.
    /// Empty means "nothing to report"; the status bar then shows the summary.
    pub status_message: String,
    /// Derived one-line description of the current directory's contents.
    pub dir_summary: String,
    /// Tree sidebar expanded paths.
    pub tree_expanded: Vec<PathBuf>,
    /// Window dimensions.
    pub window_width: u32,
    pub window_height: u32,
    /// Sidebar width.
    pub sidebar_width: f32,
    /// Undo history for completed file operations.
    ///
    /// Every operation that moves or deletes data records how to reverse it.
    /// Copy/paste and move/paste were previously irreversible because they were
    /// run by hand rather than through the executor that produces these entries.
    pub undo: UndoStack,
    /// Recycle bin used by non-permanent delete.
    pub recycle: RecycleBin,
    /// The dialog currently taking the window's input, if any.
    ///
    /// While this is `Some`, every event goes to it and none reaches the
    /// listing: a confirmation that also let Delete move the selection would
    /// act on a different file than the one it named.
    modal: Option<Modal>,
    /// The detail view's column set: which columns are shown, in what order,
    /// at what widths, and which one carries the sort arrow.
    ///
    /// Re-derived from the directory's contents on every load, so a folder of
    /// images grows a Dimensions column and a folder of source grows Language
    /// and Lines without the user asking.
    pub columns: ColumnManager,
    /// Generated thumbnails, keyed by path + mtime + size.
    ///
    /// Read from [`Self::render`] through [`ThumbnailCache::peek`], never
    /// through `get`: drawing a frame must not be allowed to reorder the LRU,
    /// or scrolling a folder larger than the cache would make eviction follow
    /// the last frame drawn rather than the user's attention. (`render` takes
    /// `&self`, so `get` is not reachable from it in any case — the two facts
    /// are the same fact.)
    pub thumbs: ThumbnailCache,
    /// Pending thumbnail work, drained a few entries at a time by
    /// [`Self::pump_thumbnails`].
    pub thumb_gen: ThumbnailGenerator,
    /// Size and colours new thumbnails are generated at.
    pub thumb_config: ThumbConfig,
    /// Thumbnails generated but not yet handed to the compositor.
    ///
    /// Drained by [`Self::take_pending_uploads`]. The explorer cannot register
    /// an image itself — it is a client, and the upload is the host's call to
    /// make — so this is the handoff point rather than a place a compositor
    /// call would go.
    pending_uploads: Vec<(u64, Thumbnail)>,
    /// Image ids the host has confirmed the compositor holds pixels for.
    ///
    /// The icon view emits a [`RenderCommand::Image`](guitk::render::RenderCommand::Image)
    /// only for an id in this set, and draws the placeholder otherwise. Without
    /// it a thumbnail that had been *generated* but not yet *uploaded* would
    /// draw as `thumbs::render_thumbnail`'s frame with nothing inside it — an
    /// empty white box with a border — because an unregistered id draws nothing
    /// and does so silently. Degrading to the placeholder instead means the
    /// view is correct at every stage, including the stage where there is no
    /// compositor connection at all.
    uploaded: HashSet<u64>,
    /// Where a dropped file would land: the on-screen rectangles of the file
    /// rows, the sidebar entries and the list pane, rebuilt every frame.
    ///
    /// Rebuilt by [`Self::render`] rather than by the code that changes the
    /// listing, because a zone is a *screen* rectangle and only the renderer
    /// knows where anything ended up. That is also why `render` takes `&mut
    /// self`: registering the zones is the same pass that draws them, and a
    /// second pass computing the same layout is a second layout to keep in
    /// agreement with the first.
    pub dropzone: DropZoneManager,
    /// The drag currently over the window, if any.
    drag: Option<DragState>,
}

impl ExplorerState {
    pub fn new(start_path: &Path) -> Self {
        let mut state = Self {
            current_path: start_path.to_path_buf(),
            entries: Vec::new(),
            viewport: ListViewport::new(0),
            wheel: WheelAccumulator::default(),
            thumb_grab: None,
            history_back: VecDeque::new(),
            history_forward: VecDeque::new(),
            view_mode: ViewMode::Details,
            sort_by: SortBy::Name,
            sort_dir: SortDir::Ascending,
            show_hidden: false,
            clipboard: None,
            selected_indices: Vec::new(),
            address_text: start_path.to_string_lossy().to_string(),
            address_editing: false,
            status_message: String::new(),
            dir_summary: String::new(),
            tree_expanded: vec![PathBuf::from("/")],
            window_width: 900,
            window_height: 600,
            sidebar_width: 200.0,
            undo: UndoStack::new(),
            recycle: RecycleBin::default_location(),
            modal: None,
            columns: ColumnManager::with_defaults(),
            thumbs: ThumbnailCache::default_capacity(),
            thumb_gen: ThumbnailGenerator::with_default_disk_cache(),
            thumb_config: ThumbConfig::default(),
            pending_uploads: Vec::new(),
            uploaded: HashSet::new(),
            dropzone: DropZoneManager::new(start_path.to_path_buf()),
            drag: None,
        };
        state.sync_sort_indicator();
        state.load_directory();
        state
    }

    // ======================================================================
    // Navigation
    // ======================================================================

    /// Navigate to a new directory.
    pub fn navigate_to(&mut self, path: &Path) {
        if path == self.current_path {
            return;
        }
        self.history_back.push_back(self.current_path.clone());
        if self.history_back.len() > 50 {
            self.history_back.pop_front();
        }
        self.history_forward.clear();
        self.current_path = path.to_path_buf();
        self.address_text = self.current_path.to_string_lossy().to_string();
        self.selected_indices.clear();
        // The previous directory's operation result no longer applies here.
        self.status_message.clear();
        self.load_directory();
    }

    /// Go back in history.
    pub fn go_back(&mut self) {
        if let Some(prev) = self.history_back.pop_back() {
            self.history_forward.push_back(self.current_path.clone());
            self.current_path = prev;
            self.address_text = self.current_path.to_string_lossy().to_string();
            self.selected_indices.clear();
            self.status_message.clear();
            self.load_directory();
        }
    }

    /// Go forward in history.
    pub fn go_forward(&mut self) {
        if let Some(next) = self.history_forward.pop_back() {
            self.history_back.push_back(self.current_path.clone());
            self.current_path = next;
            self.address_text = self.current_path.to_string_lossy().to_string();
            self.selected_indices.clear();
            self.status_message.clear();
            self.load_directory();
        }
    }

    /// Navigate to parent directory.
    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_path.parent() {
            let parent = parent.to_path_buf();
            self.navigate_to(&parent);
        }
    }

    /// Open entry: navigate if directory, launch if file.
    pub fn open_entry(&mut self, index: usize) {
        if let Some(entry) = self.entries.get(index) {
            if entry.is_dir {
                let path = entry.path.clone();
                self.navigate_to(&path);
            } else {
                // In a real implementation, launch the associated application
                self.status_message = format!("Opening: {}", entry.name);
            }
        }
    }

    // ======================================================================
    // Directory loading
    // ======================================================================

    /// Load entries from the current directory.
    pub fn load_directory(&mut self) {
        self.entries.clear();
        // Every navigation path — forward, back, up, address bar, a reload
        // after an operation — ends here, so this is the one place the drop
        // target for empty space can be kept in step with what is on screen
        // without a caller being able to forget. Getting it wrong would drop
        // files into the directory the user navigated *away* from.
        self.dropzone.set_current_dir(self.current_path.clone());

        match fs::read_dir(&self.current_path) {
            Ok(read_dir) => {
                for entry_result in read_dir {
                    let entry = match entry_result {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    let name = entry.file_name().to_string_lossy().to_string();

                    // Skip hidden files if not showing them
                    if !self.show_hidden && name.starts_with('.') {
                        continue;
                    }

                    let path = entry.path();
                    let meta = fs::metadata(&path).ok();
                    let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
                    let size = meta.as_ref().map_or(0, |m| m.len());
                    let modified = meta.as_ref().and_then(|m| m.modified().ok());

                    let file_type = if is_dir {
                        FileType::Directory
                    } else {
                        let ext = path
                            .extension()
                            .map(|e| e.to_string_lossy().to_string())
                            .unwrap_or_default();
                        FileType::from_extension(&ext)
                    };

                    self.entries.push(FileEntry {
                        name,
                        path,
                        is_dir,
                        size,
                        modified,
                        file_type,
                        selected: false,
                        icon_id: 0,
                    });
                }
            }
            Err(e) => {
                self.status_message = format!("Error: {e}");
            }
        }

        self.sort_entries();
        self.update_status();
        detect_columns(&mut self.columns, &self.entries);
        self.queue_thumbnails();
    }

    // ======================================================================
    // Thumbnails
    // ======================================================================

    /// Whether the current view actually shows thumbnails.
    ///
    /// The detail view draws a glyph per row and never a picture, so generating
    /// thumbnails for it would decode every file in the folder to produce
    /// pixels nothing draws. Queueing is gated on this rather than on the
    /// generator being empty, so switching *into* an icon view fills the queue
    /// and switching out of one empties it.
    const fn view_wants_thumbnails(&self) -> bool {
        matches!(self.view_mode, ViewMode::Icons | ViewMode::List)
    }

    /// Queue a thumbnail for every entry the current view will draw one for.
    ///
    /// Cancels whatever was pending first. A directory change makes every
    /// outstanding request point at a file the user has navigated away from,
    /// and generating those would delay the ones now on screen behind a queue
    /// of work whose results go straight into the cache's eviction path.
    ///
    /// Entries already in the cache are not re-queued: the key carries mtime
    /// and size, so a hit is a hit on *this* version of the file, and a miss
    /// after an edit is automatic.
    pub fn queue_thumbnails(&mut self) {
        self.thumb_gen.cancel_all();
        if !self.view_wants_thumbnails() {
            return;
        }
        for entry in &self.entries {
            let mtime = mtime_secs(entry.modified);
            if self.thumbs.peek(&entry.path, mtime, entry.size).is_some() {
                continue;
            }
            self.thumb_gen.push(ThumbnailRequest {
                path: entry.path.clone(),
                mtime,
                size: entry.size,
                config: self.thumb_config.clone(),
            });
        }
    }

    /// Generate up to `batch` queued thumbnails and file the results.
    ///
    /// Returns how many were generated. Call it once per frame, or on idle;
    /// generation is synchronous, so the batch size is the frame budget.
    ///
    /// Each result lands in two places: the cache, which is what the renderer
    /// reads, and the pending-upload list drained by
    /// [`Self::take_pending_uploads`], which is what the host must hand to the
    /// compositor before the picture can actually appear. The two are separate
    /// because a thumbnail that exists is not a thumbnail that can be drawn.
    pub fn pump_thumbnails(&mut self, batch: usize) -> usize {
        let generated = self.thumb_gen.process_batch(batch);
        for (req, thumb) in self.thumb_gen.take_completed() {
            let id = thumbs::image_id(&req.path, req.mtime, req.size);
            self.pending_uploads.push((id, thumb.clone()));
            self.thumbs.insert(&req.path, req.mtime, req.size, thumb);
        }
        generated
    }

    /// [`Self::pump_thumbnails`] at the default per-frame budget.
    pub fn pump_thumbnails_default(&mut self) -> usize {
        self.pump_thumbnails(THUMB_BATCH_DEFAULT)
    }

    /// Take the thumbnails waiting to be registered with the compositor.
    ///
    /// Draining does *not* mark them uploaded: the caller registers each one
    /// and reports the ones that succeeded through [`Self::mark_uploaded`]. An
    /// upload that fails must leave the entry drawing its placeholder rather
    /// than an empty frame, which is exactly what not marking it achieves.
    pub fn take_pending_uploads(&mut self) -> Vec<(u64, Thumbnail)> {
        std::mem::take(&mut self.pending_uploads)
    }

    /// Record that the compositor now holds pixels for `image_id`.
    pub fn mark_uploaded(&mut self, image_id: u64) {
        self.uploaded.insert(image_id);
    }

    /// Record that the compositor no longer holds pixels for `image_id`.
    ///
    /// The counterpart of [`Self::mark_uploaded`], for a host that unregisters
    /// an image to reclaim memory. The entry falls back to its placeholder on
    /// the next frame instead of drawing an empty frame.
    pub fn mark_dropped(&mut self, image_id: u64) -> bool {
        self.uploaded.remove(&image_id)
    }

    /// Number of image ids the compositor is believed to hold.
    #[must_use]
    pub fn uploaded_count(&self) -> usize {
        self.uploaded.len()
    }

    /// The thumbnail to draw for `entry` and the id to draw it under, if one is
    /// both generated and uploaded.
    ///
    /// Both conditions, not either: a generated-but-not-uploaded thumbnail
    /// would draw as an empty white box, because the compositor discards an
    /// `Image` command naming an id it does not hold and says nothing about it.
    ///
    /// The id comes back with the thumbnail because it is derived from the
    /// entry — path, mtime, length — and not from the pixels; the renderer has
    /// the entry in hand here and would have to re-derive it downstream, which
    /// is one more place for the two derivations to drift apart.
    fn drawable_thumb(&self, entry: &FileEntry) -> Option<(u64, &Thumbnail)> {
        let mtime = mtime_secs(entry.modified);
        let thumb = self.thumbs.peek(&entry.path, mtime, entry.size)?;
        let id = thumbs::image_id(&entry.path, mtime, entry.size);
        self.uploaded.contains(&id).then_some((id, thumb))
    }

    /// The detail view's cells for one entry, in active-column order.
    ///
    /// Built from the [`FileEntry`] the listing already produced rather than
    /// from the entry's path, for two reasons. The facts are in hand — the
    /// `readdir` that made the row already stat'ed the file — so routing them
    /// back through [`ColumnManager::get_value`] would re-stat every file
    /// twice per row on every frame. And that call takes a `&str`, which a
    /// name that is not valid UTF-8 cannot become without either losing the
    /// row or corrupting the name.
    ///
    /// Columns a *provider* owns — image dimensions, audio duration — still
    /// need the path, and are the one place a non-UTF-8 name costs anything:
    /// a blank cell, never a wrong one.
    fn row_values(&self, entry: &FileEntry) -> Vec<ColumnValue> {
        let path = entry.path.to_str();
        self.columns
            .active_columns()
            .iter()
            .map(|&id| match id {
                ColumnId::NAME => ColumnValue::Text(entry.name.clone()),
                // A directory's own byte count is not what a Size column
                // means, so it stays blank — as it did before this view used
                // the column system at all.
                ColumnId::SIZE if entry.is_dir => ColumnValue::Empty,
                ColumnId::SIZE => ColumnValue::Size(entry.size),
                ColumnId::DATE_MODIFIED => entry
                    .modified
                    .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map_or(ColumnValue::Empty, |d| ColumnValue::DateTime(d.as_secs())),
                ColumnId::TYPE => ColumnValue::Text(entry.type_label()),
                other => path.map_or(ColumnValue::Empty, |p| self.columns.get_value(p, other)),
            })
            .collect()
    }

    /// Keep the detail header's sort arrow on the column the list is actually
    /// sorted by.
    ///
    /// The explorer owns the sort — [`Self::sort_entries`] does the work — so
    /// the column manager is told the answer rather than asked for one.
    fn sync_sort_indicator(&mut self) {
        let id = match self.sort_by {
            SortBy::Name => ColumnId::NAME,
            SortBy::Size => ColumnId::SIZE,
            SortBy::Modified => ColumnId::DATE_MODIFIED,
            SortBy::Type => ColumnId::TYPE,
        };
        let order = match self.sort_dir {
            SortDir::Ascending => SortOrder::Ascending,
            SortDir::Descending => SortOrder::Descending,
        };
        self.columns.set_sort(id, order);
    }

    /// Sort entries according to current sort settings.
    fn sort_entries(&mut self) {
        // Directories always come first
        self.entries.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                return if a.is_dir {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }

            let ord = match self.sort_by {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Modified => a.modified.cmp(&b.modified),
                SortBy::Type => {
                    let ext_a = a.path.extension().map(|e| e.to_string_lossy().to_string());
                    let ext_b = b.path.extension().map(|e| e.to_string_lossy().to_string());
                    ext_a.cmp(&ext_b)
                }
            };

            match self.sort_dir {
                SortDir::Ascending => ord,
                SortDir::Descending => ord.reverse(),
            }
        });
    }

    /// Recompute the directory summary shown when there is nothing else to say.
    fn update_status(&mut self) {
        let dir_count = self.entries.iter().filter(|e| e.is_dir).count();
        let file_count = self.entries.len().saturating_sub(dir_count);
        let total_size: u64 = self
            .entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.size)
            .sum();

        self.dir_summary = format!(
            "{} folder(s), {} file(s) — {}",
            dir_count,
            file_count,
            format_size(total_size)
        );
    }

    /// The text the status bar should display.
    ///
    /// An operation result takes precedence over the directory summary until
    /// the user navigates away.
    pub fn status_bar_text(&self) -> &str {
        if self.status_message.is_empty() {
            &self.dir_summary
        } else {
            &self.status_message
        }
    }

    // ======================================================================
    // Selection
    // ======================================================================

    pub fn select_single(&mut self, index: usize) {
        for (i, entry) in self.entries.iter_mut().enumerate() {
            entry.selected = i == index;
        }
        self.selected_indices = vec![index];
    }

    pub fn toggle_selection(&mut self, index: usize) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.selected = !entry.selected;
            if entry.selected {
                self.selected_indices.push(index);
            } else {
                self.selected_indices.retain(|&i| i != index);
            }
        }
    }

    pub fn select_all(&mut self) {
        self.selected_indices.clear();
        for (i, entry) in self.entries.iter_mut().enumerate() {
            entry.selected = true;
            self.selected_indices.push(i);
        }
    }

    pub fn deselect_all(&mut self) {
        for entry in &mut self.entries {
            entry.selected = false;
        }
        self.selected_indices.clear();
    }

    // ======================================================================
    // File operations
    // ======================================================================

    /// Copy selected files to clipboard.
    pub fn copy_selected(&mut self) {
        let paths: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.selected)
            .map(|e| e.path.clone())
            .collect();
        if !paths.is_empty() {
            // Count what was actually collected, not `selected_indices`, which
            // is a separate list that can fall out of step with `entry.selected`.
            let n = paths.len();
            self.clipboard = Some(ClipboardOp::Copy(paths));
            self.status_message = format!("{n} item(s) copied to clipboard");
        }
    }

    /// Cut selected files to clipboard.
    pub fn cut_selected(&mut self) {
        let paths: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.selected)
            .map(|e| e.path.clone())
            .collect();
        if !paths.is_empty() {
            let n = paths.len();
            self.clipboard = Some(ClipboardOp::Cut(paths));
            self.status_message = format!("{n} item(s) cut to clipboard");
        }
    }

    /// Paste clipboard contents into current directory.
    ///
    /// Runs through the [`fileops`] executor rather than calling `fs::copy` /
    /// `fs::rename` directly. That engine is the one that has a conflict
    /// policy, a crash-recovery journal, per-file error collection and undo
    /// entries; the hand-rolled loop this replaces had none of them, and in
    /// particular it overwrote an existing destination file without asking and
    /// then reported "Paste complete" whether or not anything had worked.
    pub fn paste(&mut self) {
        let op = match self.clipboard.take() {
            Some(op) => op,
            None => {
                self.status_message = "Nothing to paste".to_string();
                return;
            }
        };

        let (paths, operation) = match &op {
            ClipboardOp::Copy(paths) => (paths.clone(), FileOperation::Copy),
            ClipboardOp::Cut(paths) => (paths.clone(), FileOperation::Move),
        };

        // Rename on conflict: a paste must never silently destroy a file that
        // is already in the destination. The user can still overwrite by
        // deleting the old file first, which is an explicit act.
        let plan = match operation {
            FileOperation::Move => OperationPlan::plan_move(
                &paths,
                &self.current_path,
                ConflictPolicy::Rename,
                ErrorPolicy::SkipAndContinue,
            ),
            _ => OperationPlan::plan_copy(
                &paths,
                &self.current_path,
                ConflictPolicy::Rename,
                ErrorPolicy::SkipAndContinue,
            ),
        };

        let plan = match plan {
            Ok(plan) => plan,
            Err(e) => {
                self.status_message = format!("Paste failed: {e}");
                // A plan that could not even be built has changed nothing, so
                // the clipboard is still valid — keep it.
                self.clipboard = Some(op);
                return;
            }
        };

        let mut executor = OperationExecutor::new(plan);
        let events = executor.execute();
        self.report(Self::describe_outcome(&events, "Pasted"));

        let (undo_op, entries) = executor.into_undo_entries();
        if !entries.is_empty() {
            self.undo.push(undo_op, entries);
        }

        // A copy leaves the sources in place, so the clipboard stays usable for
        // a second paste. A cut consumed them, so it must not.
        if matches!(op, ClipboardOp::Copy(_)) {
            self.clipboard = Some(op);
        }

        self.load_directory();
    }

    /// Delete selected files (move to recycle bin or permanent delete).
    ///
    /// Non-permanent delete goes through [`RecycleBin`], which records the
    /// original path alongside the data so the item can be restored and so two
    /// files of the same name from different directories do not collide. The
    /// previous implementation renamed the file into a flat `/var/recycle`
    /// directory with no metadata: nothing there could be restored or even
    /// listed, and a second `notes.txt` silently destroyed the first.
    pub fn delete_selected(&mut self, permanent: bool) {
        let paths: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.selected)
            .map(|e| e.path.clone())
            .collect();

        if paths.is_empty() {
            self.status_message = "Nothing selected".to_string();
            return;
        }

        if permanent {
            let outcome = match OperationPlan::plan_delete(&paths, ErrorPolicy::SkipAndContinue) {
                Ok(plan) => {
                    let mut executor = OperationExecutor::new(plan);
                    let events = executor.execute();
                    Self::describe_outcome(&events, "Deleted")
                }
                Err(e) => Outcome::failed(format!("Delete failed: {e}"), e.to_string()),
            };
            self.report(outcome);
        } else {
            let mut recycled = Vec::new();
            let mut first_error = None;
            for path in &paths {
                match self.recycle.recycle(path) {
                    // The bin owns the moved data, so the undo record names the
                    // bin entry rather than a path. Recording `None` here --
                    // which is what this did -- was indistinguishable from a
                    // permanent delete, and undo skipped it silently.
                    Ok(id) => recycled.push((path.clone(), UndoTarget::Recycled(id))),
                    Err(e) => {
                        if first_error.is_none() {
                            first_error = Some(format!("{}: {e}", path.display()));
                        }
                    }
                }
            }
            let moved = recycled.len();
            if !recycled.is_empty() {
                self.undo.push(FileOperation::Recycle, recycled);
            }
            let outcome = match first_error {
                None => Outcome::ok(format!("{moved} item(s) moved to recycle bin")),
                Some(err) => Outcome::failed(
                    format!(
                        "{moved} of {} item(s) moved to recycle bin — {err}",
                        paths.len()
                    ),
                    format!(
                        "{} of {} item(s) could not be moved to the recycle bin.\n\n{err}",
                        paths.len().saturating_sub(moved),
                        paths.len()
                    ),
                ),
            };
            self.report(outcome);
        }

        self.load_directory();
    }

    /// Turn an executor's event stream into a one-line status message.
    ///
    /// Reports what actually happened. The counts come from the executor's own
    /// summary, so a failed or skipped file is visible to the user instead of
    /// being folded into an unconditional "complete".
    fn describe_outcome(events: &[FileOpEvent], verb: &str) -> Outcome {
        let summary = events.iter().find_map(|e| match e {
            FileOpEvent::Complete { summary } => Some(summary),
            _ => None,
        });

        let Some(OperationSummary {
            succeeded,
            skipped,
            failed,
            errors,
            ..
        }) = summary
        else {
            // No Complete event means the operation aborted before running —
            // the executor emits an Error event in that case.
            let reason = events
                .iter()
                .find_map(|e| match e {
                    FileOpEvent::Error { error, .. } => Some(error.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "operation did not complete".to_string());
            let message = format!("{verb} nothing — {reason}");
            return Outcome::failed(message, reason);
        };

        let mut msg = format!("{verb} {succeeded} item(s)");
        if *skipped > 0 {
            msg.push_str(&format!(", {skipped} skipped"));
        }
        if *failed == 0 {
            return Outcome::ok(msg);
        }

        msg.push_str(&format!(", {failed} failed"));
        // The dialog names the first failure in full. Listing all of them
        // would be the right thing for a queue view and the wrong thing for a
        // dialog, which has to be readable at a glance; the status bar keeps
        // the count, so nothing is lost.
        let detail = match errors.first() {
            Some(first) => {
                msg.push_str(&format!(" — {}: {}", first.path.display(), first.message));
                format!(
                    "{failed} of {} could not be done.\n\n{}: {}",
                    succeeded.saturating_add(*failed),
                    first.path.display(),
                    first.message
                )
            }
            // A failure count with no error to go with it is the executor
            // contradicting itself. Say so rather than showing an empty
            // dialog, which reads as a bug in the dialog.
            None => format!("{failed} item(s) could not be done, with no reason given."),
        };
        Outcome::failed(msg, detail)
    }

    /// Put an outcome where the user will see it.
    ///
    /// The status bar always gets the line. A failure additionally raises a
    /// dialog, because the status bar is a place a message can be missed
    /// entirely -- and the one time that matters is when the user asked for
    /// something destructive and only part of it happened.
    fn report(&mut self, outcome: Outcome) {
        self.status_message = outcome.message;
        if let Some(detail) = outcome.failure {
            let mut dialog = AlertDialog::error("Could not finish", &detail);
            dialog.show();
            self.modal = Some(Modal::Notice { dialog });
        }
    }

    /// Create a new folder.
    pub fn create_folder(&mut self, name: &str) {
        if let Err(reason) = validate_entry_name(name) {
            self.report(Outcome::failed(
                format!("Error creating folder: {reason}"),
                format!("\"{name}\" cannot be used as a name.\n\n{reason}"),
            ));
            return;
        }
        let path = self.current_path.join(name);
        match fs::create_dir(&path) {
            Ok(()) => {
                self.load_directory();
                self.report(Outcome::ok(format!("Created folder: {name}")));
            }
            Err(e) => {
                self.report(Outcome::failed(
                    format!("Error creating folder: {e}"),
                    format!("The folder \"{name}\" could not be created.\n\n{e}"),
                ));
            }
        }
    }

    /// Rename an entry.
    ///
    /// # Why this is more than one `fs::rename`
    ///
    /// `fs::rename` **overwrites its destination**. Renaming `draft.txt` onto
    /// an existing `notes.txt` therefore destroyed the notes and reported
    /// success. Paste already refuses to do that — see [`Self::paste`], which
    /// runs through the [`fileops`] engine's conflict policy precisely so a
    /// paste "must never silently destroy a file that is already in the
    /// destination" — and rename has to hold the same line, for the same
    /// reason and with the same escape hatch: delete the old file first, which
    /// is an explicit act.
    ///
    /// The name is also validated rather than trusted. `with_file_name` does
    /// not constrain its result to the same directory, so `../taken.txt`
    /// renamed the file *out* of the folder being viewed and onto whatever was
    /// already sitting there — a second way to destroy a file the user never
    /// selected.
    pub fn rename_entry(&mut self, index: usize, new_name: &str) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let old_path = entry.path.clone();

        if let Err(reason) = validate_entry_name(new_name) {
            self.report(Outcome::failed(
                format!("Rename failed: {reason}"),
                format!("\"{new_name}\" cannot be used as a name.\n\n{reason}"),
            ));
            return;
        }

        let new_path = old_path.with_file_name(new_name);

        // Renaming something to the name it already has is what pressing Enter
        // in the rename box does. It is a no-op, not a collision with itself.
        if new_path == old_path {
            self.report(Outcome::ok(format!("Renamed to: {new_name}")));
            return;
        }

        // The gap between this check and the rename is a race, and it is not
        // closable with `std` alone — there is no portable "rename only if the
        // destination is free". It is still worth checking: the realistic way
        // to hit this is the user typing a name they can see in the listing,
        // not another process creating that exact file inside the intervening
        // microsecond. `fileops`'s engine makes the same tradeoff.
        if new_path.exists() && !is_same_file(&old_path, &new_path) {
            self.report(Outcome::failed(
                format!("Rename failed: \"{new_name}\" already exists"),
                format!(
                    "\"{new_name}\" already exists here.\n\nRenaming onto it would \
                     destroy it, so nothing was changed. Delete it first, or pick \
                     another name."
                ),
            ));
            return;
        }

        match fs::rename(&old_path, &new_path) {
            Ok(()) => {
                self.load_directory();
                self.report(Outcome::ok(format!("Renamed to: {new_name}")));
            }
            Err(e) => {
                self.report(Outcome::failed(
                    format!("Rename failed: {e}"),
                    format!("\"{new_name}\" could not be used.\n\n{e}"),
                ));
            }
        }
    }

    // ======================================================================
    // Drag and drop
    // ======================================================================

    /// The drag currently over the window, if any.
    pub fn drag(&self) -> Option<&DragState> {
        self.drag.as_ref()
    }

    /// Begin tracking a drag of `sources` over the window.
    ///
    /// No zone is chosen yet: the pointer position arrives with the first
    /// movement, and highlighting a guess before then would flash a target the
    /// user never aimed at.
    pub fn drag_enter(&mut self, sources: Vec<PathBuf>) {
        self.drag = Some(DragState {
            sources,
            x: 0.0,
            y: 0.0,
            modifiers: DragModifiers::default(),
            zone: DropZone::None,
            operation: DropOperation::None,
            valid: false,
            invalid_reason: None,
        });
    }

    /// Record a pointer movement during a drag, returning the zone transition
    /// it caused.
    ///
    /// Returns `None` when no drag is in flight, which is also what the manager
    /// returns for a move that stays outside every zone — the two are
    /// indistinguishable to a caller that only wants to know whether to redraw,
    /// and both mean "nothing to show".
    pub fn drag_over(&mut self, x: f32, y: f32, modifiers: DragModifiers) -> Option<DropZoneEvent> {
        let mut drag = self.drag.take()?;
        drag.x = x;
        drag.y = y;

        let event = self.dropzone.update_hover(x, y, modifiers, &drag.sources);
        let zone = self.dropzone.current_hover().clone();

        // Re-decide only when the answer can have changed. `evaluate_drop`
        // canonicalises both ends of the drop and stats every source against
        // the target; doing that for each of the hundreds of pointer positions
        // that make up one traversal of a row would be a syscall per pixel.
        if zone != drag.zone || modifiers != drag.modifiers {
            let result = self.evaluate_drop(x, y, &drag.sources, modifiers);
            drag.operation = result.operation;
            drag.valid = result.valid;
            drag.invalid_reason = result.invalid_reason;
            drag.zone = zone;
            drag.modifiers = modifiers;
        }

        self.drag = Some(drag);
        event
    }

    /// Abandon the drag without dropping — the pointer left the window, or the
    /// user pressed Escape.
    pub fn drag_cancel(&mut self) {
        self.drag = None;
        self.dropzone.clear_hover();
    }

    /// Release the drag at `(x, y)`, performing the operation if it is allowed.
    ///
    /// Returns what was decided — including a rejected decision, whose
    /// `invalid_reason` the status bar shows — or `None` if no drag was in
    /// flight. The drag is over either way: a refused drop does not leave the
    /// pointer still holding the files, because the release already happened.
    pub fn drop_at(&mut self, x: f32, y: f32, modifiers: DragModifiers) -> Option<DropResult> {
        let drag = self.drag.take()?;
        self.dropzone.clear_hover();

        let result = self.evaluate_drop(x, y, &drag.sources, modifiers);
        if !result.valid {
            // A drop onto nothing is the user missing, not an error worth
            // interrupting them over; a drop onto a folder that refuses it is.
            if let Some(reason) = &result.invalid_reason
                && result.operation != DropOperation::None
            {
                self.status_message = reason.clone();
            }
            return Some(result);
        }

        self.execute_drop(&result);
        Some(result)
    }

    /// What releasing `sources` at `(x, y)` would do, and whether it is
    /// allowed.
    ///
    /// Wraps [`DropZoneManager::handle_drop`] with the two rules the manager
    /// cannot know because they belong to the *executor*, not to the layout:
    ///
    /// * `fileops` has no link operation, so an Alt-drag is refused here rather
    ///   than reported as `Link` and then silently not performed.
    /// * A move whose sources are already in the target directory has nothing
    ///   to do. Left alone it would be worse than nothing: the conflict policy
    ///   is `Rename`, so moving `notes.txt` into the folder it is already in
    ///   would produce `notes (2).txt` — a duplicate conjured by a drag the
    ///   user meant as a no-op. A *copy* into the same folder is not the same
    ///   case; duplicating a file that way is a thing people do on purpose.
    fn evaluate_drop(
        &self,
        x: f32,
        y: f32,
        sources: &[PathBuf],
        modifiers: DragModifiers,
    ) -> DropResult {
        let mut result = self.dropzone.handle_drop(x, y, sources, modifiers);
        if !result.valid {
            return result;
        }

        if result.operation == DropOperation::Move {
            let target = result.target_dir.clone();
            result
                .sources
                .retain(|s| s.parent() != Some(target.as_path()));
            result.conflicts.retain(|c| {
                result
                    .sources
                    .iter()
                    .any(|s| s.file_name() == c.file_name())
            });
            if result.sources.is_empty() {
                result.valid = false;
                result.invalid_reason = Some("Already in this folder".to_string());
            }
        }

        result
    }

    /// Carry out a validated drop through the same executor as paste.
    ///
    /// Not a second copy engine: the conflict policy, the crash journal, the
    /// per-file error collection and the undo entries all live in [`fileops`],
    /// and a drag-and-drop that wrote files itself would have none of them —
    /// which is precisely the state paste was rescued from.
    fn execute_drop(&mut self, result: &DropResult) {
        let (plan, verb) = match result.operation {
            DropOperation::Move => (
                OperationPlan::plan_move(
                    &result.sources,
                    &result.target_dir,
                    ConflictPolicy::Rename,
                    ErrorPolicy::SkipAndContinue,
                ),
                "Moved",
            ),
            DropOperation::Copy => (
                OperationPlan::plan_copy(
                    &result.sources,
                    &result.target_dir,
                    ConflictPolicy::Rename,
                    ErrorPolicy::SkipAndContinue,
                ),
                "Copied",
            ),
            DropOperation::Link => (
                Ok(OperationPlan::plan_link(
                    &result.sources,
                    &result.target_dir,
                    ConflictPolicy::Rename,
                    // The same policy the other two use, and for a reason
                    // specific to links: a filesystem that refuses them
                    // refuses each one separately -- Windows needs a
                    // privilege -- so a batch must report which failed rather
                    // than abandoning the ones that would have worked.
                    ErrorPolicy::SkipAndContinue,
                )),
                "Linked",
            ),
            // `evaluate_drop` refuses this, so reaching here would mean the
            // caller executed a result it was told was invalid.
            DropOperation::None => return,
        };

        let plan = match plan {
            Ok(plan) => plan,
            Err(e) => {
                self.status_message = format!("Drop failed: {e}");
                return;
            }
        };

        let mut executor = OperationExecutor::new(plan);
        let events = executor.execute();
        self.report(Self::describe_outcome(&events, verb));

        let (undo_op, entries) = executor.into_undo_entries();
        if !entries.is_empty() {
            self.undo.push(undo_op, entries);
        }

        self.load_directory();
    }

    // ======================================================================
    // Rendering
    // ======================================================================

    /// Render the complete file explorer UI, re-registering the drop zones as
    /// it goes.
    ///
    /// The zones are the rectangles a dragged file can be released onto, and
    /// they are a *product of the layout*: only this pass knows where row seven
    /// ended up, or that the pane was too narrow for a third icon column. So
    /// the drawing pass registers them, and a drag hit-tests what the last
    /// frame drew — which is exactly what the user was looking at when they
    /// aimed.
    ///
    /// The manager is moved out of `self` for the duration rather than borrowed
    /// from it, because the per-view helpers read `self.entries` while writing
    /// the manager and the compiler will not split a `&self` borrow that way.
    /// Moving it out and back also carries `current_hover` across the frame,
    /// which a freshly-constructed manager would drop — making the highlight
    /// flicker off on every frame of a stationary hover.
    pub fn render(&mut self) -> RenderTree {
        let mut tree = RenderTree::new();
        let w = self.window_width as f32;
        let h = self.window_height as f32;

        // The placeholder left in `self.dropzone` is never observed: nothing
        // between here and the restore below reads the field, and the helpers
        // are handed `zones` instead. It is an empty path rather than a clone
        // of the current one because a clone would be an allocation per frame
        // to construct a value with no reader.
        let mut zones = std::mem::replace(&mut self.dropzone, DropZoneManager::new(PathBuf::new()));
        // Only the zones are rebuilt. The current directory is *not* re-set
        // here: `load_directory` is the single place that tracks it, because
        // every navigation ends there and a second setter would be a second
        // thing to keep in step with the first.
        zones.clear_zones();

        // Background
        tree.fill_rect(0.0, 0.0, w, h, Color::from_hex(0xF5F5F5));

        // Toolbar (top)
        self.render_toolbar(&mut tree);

        // Address bar
        self.render_address_bar(&mut tree);

        // Sidebar (directory tree)
        self.render_sidebar(&mut tree, &mut zones);

        // File list
        self.render_file_list(&mut tree, &mut zones);

        // Status bar (bottom)
        self.render_status_bar(&mut tree);

        self.dropzone = zones;

        // Drop feedback last, so the highlight sits over the row it marks
        // rather than under it.
        self.render_drop_feedback(&mut tree);

        // The modal after even that, so it draws over the listing it is
        // asking about -- the drop feedback is the only other thing here that
        // floats above the window's own furniture, and a confirmation must
        // sit above it too.
        match self.modal.as_mut() {
            Some(Modal::Confirm { dialog, .. } | Modal::Notice { dialog }) => {
                dialog.render(w, h, &mut tree);
            }
            Some(Modal::Rename { dialog, .. }) => dialog.render(w, h, &mut tree),
            None => {}
        }

        tree
    }

    /// Overlay the highlight and the "Copy to …" label for a drag in flight.
    ///
    /// The zone is re-found from the rectangles this very frame registered,
    /// rather than reused from the one cached at the last pointer movement, so
    /// that a list which scrolled under a stationary pointer highlights the row
    /// now under it. The *operation* stays as cached: what a drop would do
    /// depends on the target, and re-deciding it here would mean stat-ing the
    /// filesystem once per frame for an answer that only changes when the
    /// pointer or a modifier key moves.
    fn render_drop_feedback(&self, tree: &mut RenderTree) {
        let Some(drag) = &self.drag else {
            return;
        };
        let zone = self.dropzone.find_zone(drag.x, drag.y);
        for cmd in dropzone::render_drop_feedback(
            &zone,
            drag.operation,
            drag.x,
            drag.y,
            self.dropzone.list_area(),
            drag.valid,
        ) {
            tree.push(cmd);
        }
    }

    fn render_toolbar(&self, tree: &mut RenderTree) {
        let toolbar_h = 36.0;
        tree.fill_rect(
            0.0,
            0.0,
            self.window_width as f32,
            toolbar_h,
            Color::from_hex(0xE8E8E8),
        );

        // Navigation buttons
        let buttons = [
            "\u{2190}",
            "\u{2192}",
            "\u{2191}",
            "|",
            "\u{1F4C1}+",
            "\u{2702}",
            "\u{1F4CB}",
        ];
        let mut x = 8.0;
        for btn_text in &buttons {
            if *btn_text == "|" {
                // Separator
                tree.fill_rect(x, 4.0, 1.0, toolbar_h - 8.0, Color::from_hex(0xC0C0C0));
                x += 12.0;
            } else {
                tree.fill_rect(x, 4.0, 28.0, 28.0, Color::from_hex(0xD0D0D0));
                tree.text(x + 6.0, 10.0, btn_text, Color::from_hex(0x333333), 14.0);
                x += 32.0;
            }
        }
    }

    fn render_address_bar(&self, tree: &mut RenderTree) {
        let bar_y = 36.0;
        let bar_h = 28.0;
        let w = self.window_width as f32;

        tree.fill_rect(0.0, bar_y, w, bar_h, Color::WHITE);
        tree.stroke_rect(
            4.0,
            bar_y + 2.0,
            w - 8.0,
            bar_h - 4.0,
            Color::from_hex(0xC0C0C0),
            1.0,
        );
        tree.text(12.0, bar_y + 7.0, &self.address_text, Color::BLACK, 13.0);
    }

    fn render_sidebar(&self, tree: &mut RenderTree, zones: &mut DropZoneManager) {
        let sidebar_y = 64.0;
        let sidebar_h = self.window_height as f32 - 64.0 - 24.0; // minus toolbar and status bar
        let sw = self.sidebar_width;

        tree.fill_rect(0.0, sidebar_y, sw, sidebar_h, Color::from_hex(0xF0F0F0));
        tree.stroke_rect(
            sw - 1.0,
            sidebar_y,
            1.0,
            sidebar_h,
            Color::from_hex(0xD0D0D0),
            1.0,
        );

        // Quick access items
        for (i, (label, path)) in SIDEBAR_ITEMS.iter().enumerate() {
            let iy = sidebar_y + 8.0 + i as f32 * SIDEBAR_ROW_H;
            tree.text(16.0, iy + 4.0, label, Color::from_hex(0x333333), 12.0);
            // The whole strip is the target, not just the glyphs: a drop aimed
            // at the gap beside a short name like "/tmp" is still aimed at
            // /tmp.
            zones.register_sidebar_item(Path::new(path), Rect::new(0.0, iy, sw, SIDEBAR_ROW_H));
        }
    }

    fn render_file_list(&self, tree: &mut RenderTree, zones: &mut DropZoneManager) {
        let list_x = self.sidebar_width;
        let list_y = 64.0;
        let list_w = self.window_width as f32 - self.sidebar_width;
        let list_h = self.window_height as f32 - 64.0 - 24.0;

        // The pane itself is the fallback target: anything inside it that is
        // not a folder row means "into the directory being shown".
        zones.set_list_area(Rect::new(list_x, list_y, list_w, list_h));

        match self.view_mode {
            ViewMode::Details => self.render_details(tree, zones, list_x, list_y, list_w, list_h),
            ViewMode::Icons => self.render_icons(tree, zones, list_x, list_y, list_w, list_h),
            ViewMode::List => self.render_list(tree, zones, list_x, list_y, list_w, list_h),
        }
        // After the view, so the bar sits over the rows rather than under them.
        self.render_scrollbar(tree);
    }

    /// The file pane's rectangle: below the toolbar, right of the sidebar.
    ///
    /// The renderer and the hit test both come here rather than each computing
    /// it, because a scrollbar drawn in one place and hit-tested in another is
    /// the class of bug nobody sees until they try to drag it.
    fn pane_rect(&self) -> Rect {
        Rect::new(
            self.sidebar_width,
            64.0,
            (self.window_width as f32 - self.sidebar_width).max(0.0),
            (self.window_height as f32 - 64.0 - 24.0).max(0.0),
        )
    }

    /// How many rows of the current view fit in the pane.
    ///
    /// In the grid a "row" is a row of icons, so this counts *entries* -- the
    /// unit the viewport offset is in -- by multiplying by the column count.
    fn visible_capacity(&self) -> usize {
        let pane = self.pane_rect();
        match self.view_mode {
            ViewMode::List => scroll_window::capacity(LIST_ROW_H, pane.height),
            ViewMode::Details => scroll_window::capacity(ROW_H, (pane.height - HEADER_H).max(0.0)),
            ViewMode::Icons => scroll_window::capacity(ICON_CELL_H, pane.height)
                .saturating_mul(self.icon_columns()),
        }
    }

    /// The scrollbar's track, when the listing is long enough to have one.
    fn scrollbar_track(&self) -> Option<Rect> {
        let capacity = self.visible_capacity();
        if !scrollbar::needed(self.entries.len(), capacity) {
            return None;
        }
        let pane = self.pane_rect();
        // The detail view's header is not part of the scrollable region, so the
        // track starts below it -- a thumb that ran up behind the column
        // headings would claim rows that are never drawn there.
        let top = match self.view_mode {
            ViewMode::Details => pane.y + HEADER_H,
            ViewMode::List | ViewMode::Icons => pane.y,
        };
        Some(Rect::new(
            pane.x + pane.width - scrollbar::WIDTH,
            top,
            scrollbar::WIDTH,
            (pane.y + pane.height - top).max(0.0),
        ))
    }

    /// The thumb's rectangle, in the toolkit's coordinates.
    fn scrollbar_thumb(&self) -> Option<guitk::frame::Rect> {
        let track = self.scrollbar_track()?;
        Some(scrollbar::thumb(
            guitk::frame::Rect::new(track.x, track.y, track.width, track.height),
            self.entries.len(),
            self.visible_capacity(),
            self.viewport.first_visible(),
        ))
    }

    /// Take hold of the thumb, if the press landed on it. A press elsewhere on
    /// the track pages towards it, which is what every scrollbar does and what
    /// makes the track worth drawing at all.
    fn press_scrollbar(&mut self, x: f32, y: f32) -> bool {
        let (Some(track), Some(thumb)) = (self.scrollbar_track(), self.scrollbar_thumb()) else {
            return false;
        };
        if x < track.x || x >= track.x + track.width {
            return false;
        }
        if y < track.y || y >= track.y + track.height {
            return false;
        }
        if y >= thumb.y && y < thumb.y + thumb.h {
            self.thumb_grab = Some(y - thumb.y);
        } else {
            // A page, in the direction of the click. `checked_neg` rather than
            // `-page`: a capacity that did not fit in an `isize` would be a
            // window taller than nine quintillion rows, but the negation is
            // still the one operation here that can fail, and paging by zero
            // is a better answer than wrapping to the far end of the list.
            let page = isize::try_from(self.visible_capacity()).unwrap_or(isize::MAX);
            let delta = if y < thumb.y {
                page.checked_neg().unwrap_or(0)
            } else {
                page
            };
            self.viewport.scroll_by(delta, self.entries.len());
        }
        true
    }

    /// Continue a thumb drag. Returns whether the view moved.
    fn drag_scrollbar(&mut self, y: f32) -> bool {
        let (Some(grab), Some(track), Some(thumb)) = (
            self.thumb_grab,
            self.scrollbar_track(),
            self.scrollbar_thumb(),
        ) else {
            // The listing got short enough to lose its scrollbar mid-drag.
            self.thumb_grab = None;
            return false;
        };
        let Some(first) = scrollbar::first_from_drag(
            guitk::frame::Rect::new(track.x, track.y, track.width, track.height),
            thumb.h,
            grab,
            y,
            self.entries.len(),
            self.visible_capacity(),
        ) else {
            return false;
        };
        if first == self.viewport.first_visible() {
            return false;
        }
        self.viewport.scroll_to(first, self.entries.len());
        true
    }

    /// Draw the scrollbar, if there is one.
    fn render_scrollbar(&self, tree: &mut RenderTree) {
        let Some(track) = self.scrollbar_track() else {
            return;
        };
        // The toolkit's `Rect` names its sides `w`/`h` where explorer's names
        // them `width`/`height`; converted here, at the one call that crosses.
        let track_gui = guitk::frame::Rect::new(track.x, track.y, track.width, track.height);
        let thumb = scrollbar::thumb(
            track_gui,
            self.entries.len(),
            self.visible_capacity(),
            self.viewport.first_visible(),
        );
        tree.fill_rect(
            track.x,
            track.y,
            track.width,
            track.height,
            Color::from_hex(0xF0F0F0),
        );
        tree.fill_rounded_rect(
            thumb.x + 1.0,
            thumb.y,
            thumb.w - 2.0,
            thumb.h,
            Color::from_hex(0xB0B0B0),
            guitk::style::CornerRadii::all(4.0),
        );
    }

    /// How many icon cells fit across the file pane.
    ///
    /// At least one, however narrow: a zero would make the row index a
    /// division by zero, and a pane too narrow for a cell should clip one
    /// rather than draw none. Shared by the renderer and the wheel, because a
    /// wheel stepping by a different column count than the grid is laid out in
    /// would move by a fraction of a row and feel stuck.
    fn icon_columns(&self) -> usize {
        let pane_w = (self.window_width as f32 - self.sidebar_width).max(0.0);
        ((pane_w / ICON_CELL_W) as usize).max(1)
    }

    /// The icon view: a grid of thumbnail cells, each captioned with its name.
    ///
    /// Every cell draws *something* at every stage. A file whose thumbnail has
    /// been generated and uploaded gets the picture; one that is still queued,
    /// or generated but not yet registered with the compositor, gets the
    /// category placeholder, which is built from primitives and needs nothing
    /// from the compositor at all. There is deliberately no fourth state where
    /// the cell is blank: an `Image` command naming an id the compositor does
    /// not hold draws nothing and reports nothing, so a view that emitted one
    /// optimistically would show an empty white frame with no way to tell
    /// whether the file was undrawable or the upload had simply not happened.
    fn render_icons(
        &self,
        tree: &mut RenderTree,
        zones: &mut DropZoneManager,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        // At least one column, however narrow the pane: a zero here would make
        // the row index a division by zero, and a pane too narrow for a cell
        // should clip one cell rather than draw none.
        let cols = ((w / ICON_CELL_W) as usize).max(1);
        // The offset is kept in *entries*, one number for all three views, so
        // changing view mode lands you at roughly the same place rather than
        // back at the top. The grid rounds it down to a whole row of icons:
        // starting mid-row would put the first cell in the middle of the pane
        // with a gap beside it.
        let first = self
            .viewport
            .first_visible()
            .checked_div(cols)
            .unwrap_or(0)
            .saturating_mul(cols);
        let icon_rows = scroll_window::capacity(ICON_CELL_H, h);
        let visible_cells = icon_rows.saturating_mul(cols);

        tree.translate(x, y);
        // The grid is clipped to the pane, not merely truncated to whole rows:
        // a partial row at the bottom should be cut off mid-cell, as a scrolled
        // list is, rather than vanish.
        tree.clip(0.0, 0.0, w, h);

        for (cell, entry) in self
            .entries
            .iter()
            .skip(first)
            .take(visible_cells)
            .enumerate()
        {
            // Laid out by the *visible* cell, identified by the *absolute*
            // index. Using one number for both would push the first row off
            // the top the moment the grid scrolled, and would register drop
            // zones under the wrong files.
            let i = first.saturating_add(cell);
            // `cols` is at least 1 by the `max` above, so neither `None` arm
            // is reachable — but writing the division as fallible keeps the
            // loop free of an operation whose safety the reader has to prove
            // from a line thirty above it.
            let cx = cell.checked_rem(cols).unwrap_or(0) as f32 * ICON_CELL_W;
            let cy = cell.checked_div(cols).unwrap_or(0) as f32 * ICON_CELL_H;

            // Registered in window coordinates, not the pane-local ones the
            // commands are emitted in: the pointer position a drop arrives
            // with is a window position, and translating one of the two at
            // hit-test time would mean the zone list only made sense to a
            // caller that knew which pane it came from.
            zones.register_file_row(
                i,
                &entry.path,
                Rect::new(x + cx, y + cy, ICON_CELL_W, ICON_CELL_H),
                entry.is_dir,
            );

            if entry.selected {
                tree.fill_rounded_rect(
                    cx + 2.0,
                    cy + 2.0,
                    ICON_CELL_W - 4.0,
                    ICON_CELL_H - 4.0,
                    Color::from_hex(0xCCE8FF),
                    guitk::style::CornerRadii::all(4.0),
                );
            }

            // Centre the thumbnail box horizontally in the cell; the caption
            // sits under it, using the cell's full width.
            let tx = cx + (ICON_CELL_W - ICON_THUMB_SIZE) / 2.0;
            let ty = cy + 8.0;
            self.push_thumb(tree, entry, tx, ty, ICON_THUMB_SIZE);

            let label_y = ty + ICON_THUMB_SIZE + 6.0;
            let name_color = if entry.is_dir {
                Color::from_hex(0x0066CC)
            } else {
                Color::BLACK
            };
            // Elided rather than clipped: a name cut mid-word with no mark is
            // read as the whole name, which is how one file gets mistaken for
            // another whose name it is a prefix of.
            tree.text_in(
                cx + 4.0,
                label_y,
                ICON_CELL_W - 8.0,
                &entry.name,
                name_color,
                ICON_LABEL_SIZE,
            );
        }

        tree.unclip();
        tree.untranslate();
    }

    /// The list view: one row per entry, a small thumbnail and the name.
    ///
    /// Distinct from the detail view in what it *omits* — no header, no
    /// columns, no size or date — and from the icon view in that it is one
    /// column, so a long name has the whole pane to be legible in. The
    /// thumbnail follows the same generated-and-uploaded rule as the icon
    /// view's; see [`Self::render_icons`].
    fn render_list(
        &self,
        tree: &mut RenderTree,
        zones: &mut DropZoneManager,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        // Computed from the pixel height rather than read off the viewport,
        // because a renderer has only `&self` and the window may have been
        // resized since the last event. `visible` also clamps a stale offset to
        // the last page instead of drawing blank space.
        let rows = scroll_window::visible(
            self.entries.len(),
            LIST_ROW_H,
            h,
            self.viewport.first_visible(),
        );

        tree.translate(x, y);
        tree.clip(0.0, 0.0, w, h);

        for (row, i) in (rows.start..rows.end()).enumerate() {
            let Some(entry) = self.entries.get(i) else {
                break;
            };
            let ry = row as f32 * LIST_ROW_H;

            // The *absolute* index, so a click on the third visible row names
            // the third visible file rather than the third file in the folder.
            zones.register_file_row(
                i,
                &entry.path,
                Rect::new(x, y + ry, w, LIST_ROW_H),
                entry.is_dir,
            );

            if entry.selected {
                tree.fill_rect(0.0, ry, w, LIST_ROW_H, Color::from_hex(0xCCE8FF));
            } else if i % 2 == 1 {
                tree.fill_rect(0.0, ry, w, LIST_ROW_H, Color::from_hex(0xFAFAFA));
            }

            let ty = ry + (LIST_ROW_H - LIST_THUMB_SIZE) / 2.0;
            self.push_thumb(tree, entry, 6.0, ty, LIST_THUMB_SIZE);

            let name_x = 6.0 + LIST_THUMB_SIZE + 8.0;
            let name_color = if entry.is_dir {
                Color::from_hex(0x0066CC)
            } else {
                Color::BLACK
            };
            tree.text_in(
                name_x,
                ry + (LIST_ROW_H - 13.0) / 2.0,
                (w - name_x - 8.0).max(0.0),
                &entry.name,
                name_color,
                13.0,
            );
        }

        tree.unclip();
        tree.untranslate();
    }

    /// Emit one entry's thumbnail, or its placeholder if there is not one that
    /// can be drawn.
    ///
    /// The single place the choice is made, so the icon and list views cannot
    /// disagree about when a picture is safe to emit.
    fn push_thumb(&self, tree: &mut RenderTree, entry: &FileEntry, x: f32, y: f32, size: f32) {
        let cmds = match self.drawable_thumb(entry) {
            Some((id, thumb)) => thumbs::render_thumbnail(thumb, id, x, y, size),
            None => thumbs::render_placeholder(entry_category(entry), None, x, y, size),
        };
        for cmd in cmds {
            tree.push(cmd);
        }
    }

    /// The detail view: a header bar plus one row per entry, laid out by the
    /// column system rather than by three hardcoded x-offsets.
    ///
    /// `columns::render_*` emit commands from `(0, 0)`, so the whole table is
    /// translated into the pane once instead of every cell carrying the pane's
    /// origin. The icon occupies a fixed gutter to the left of the first
    /// column; the header's own bar starts where the columns do, so the gutter
    /// strip of it is filled here to keep the bar continuous.
    fn render_details(
        &self,
        tree: &mut RenderTree,
        zones: &mut DropZoneManager,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let table_w = (w - ICON_GUTTER).max(0.0);
        // The header eats its own height before the rows get any.
        let rows = scroll_window::visible(
            self.entries.len(),
            ROW_H,
            (h - HEADER_H).max(0.0),
            self.viewport.first_visible(),
        );

        tree.translate(x, y);

        tree.fill_rect(0.0, 0.0, ICON_GUTTER, HEADER_H, Color::from_hex(0xE0E0E0));
        tree.translate(ICON_GUTTER, 0.0);
        for cmd in columns::render_column_header(&self.columns, table_w) {
            tree.push(cmd);
        }
        tree.untranslate();

        for (row, i) in (rows.start..rows.end()).enumerate() {
            let Some(entry) = self.entries.get(i) else {
                break;
            };
            let ey = HEADER_H + row as f32 * ROW_H;

            // The absolute index, so a click names the file under the pointer
            // rather than the one that many rows from the top of the folder.
            zones.register_file_row(i, &entry.path, Rect::new(x, y + ey, w, ROW_H), entry.is_dir);

            if entry.selected {
                tree.fill_rect(0.0, ey, w, ROW_H, Color::from_hex(0xCCE8FF));
            // Striped by *absolute* row, so the banding does not crawl as the
            // view scrolls -- a stripe that follows the screen position rather
            // than the file makes a scrolling list shimmer.
            } else if i % 2 == 1 {
                tree.fill_rect(0.0, ey, w, ROW_H, Color::from_hex(0xFAFAFA));
            }

            let mut icon = [0u8; 4];
            tree.text(
                8.0,
                ey + 3.0,
                entry.file_type.icon_char().encode_utf8(&mut icon),
                Color::BLACK,
                12.0,
            );

            // Directory names stay visually distinct from file names, as they
            // were when this view drew its own three columns.
            let name_color = entry.is_dir.then(|| Color::from_hex(0x0066CC));

            let values = self.row_values(entry);
            tree.translate(ICON_GUTTER, 0.0);
            for cmd in
                columns::render_column_values_from(&self.columns, &values, ey, table_w, name_color)
            {
                tree.push(cmd);
            }
            tree.untranslate();
        }

        tree.untranslate();
    }

    fn render_status_bar(&self, tree: &mut RenderTree) {
        let bar_y = self.window_height as f32 - 24.0;
        let w = self.window_width as f32;

        tree.fill_rect(0.0, bar_y, w, 24.0, Color::from_hex(0xE8E8E8));
        tree.text(
            8.0,
            bar_y + 5.0,
            self.status_bar_text(),
            Color::from_hex(0x555555),
            11.0,
        );
    }

    // ======================================================================
    // Sort
    // ======================================================================

    pub fn set_sort(&mut self, by: SortBy) {
        if self.sort_by == by {
            // Toggle direction
            self.sort_dir = match self.sort_dir {
                SortDir::Ascending => SortDir::Descending,
                SortDir::Descending => SortDir::Ascending,
            };
        } else {
            self.sort_by = by;
            self.sort_dir = SortDir::Ascending;
        }
        self.sync_sort_indicator();
        self.sort_entries();
    }

    /// Switch view modes, re-deriving what thumbnail work the new mode needs.
    ///
    /// Switching *into* a picture view fills the queue for a directory that was
    /// loaded while the detail view was up; switching *out* of one empties it,
    /// so a folder of ten thousand files stops decoding the moment the user
    /// stops looking at the pictures.
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        if self.view_mode == mode {
            return;
        }
        self.view_mode = mode;
        self.queue_thumbnails();
    }

    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.load_directory();
    }
}

// ============================================================================
// Utility functions
// ============================================================================

fn format_size(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

/// A listing entry's modification time as whole seconds since the epoch, for
/// use as part of a thumbnail cache key.
///
/// A file with no readable mtime keys as 0. That is a *stable* key, not a
/// missing one, which is what matters here: the alternative — refusing to
/// cache it — would re-decode the file on every frame it was visible. It costs
/// a stale thumbnail for a file whose mtime cannot be read *and* whose size
/// never changes, which the disk cache already accepts for the same reason.
fn mtime_secs(modified: Option<SystemTime>) -> u64 {
    modified
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// The placeholder category for a listing entry.
///
/// Derived from the entry's already-known [`FileType`] rather than by handing
/// its extension back to [`ThumbCategory::from_extension`]: the listing has
/// classified it once, the two tables agree on every category they share, and
/// routing through the string would mean a name that is not valid UTF-8 loses
/// its icon for no reason.
const fn entry_category(entry: &FileEntry) -> ThumbCategory {
    match entry.file_type {
        FileType::Directory => ThumbCategory::Folder,
        FileType::Image => ThumbCategory::Image,
        FileType::Text | FileType::Code => ThumbCategory::Text,
        FileType::Audio => ThumbCategory::Audio,
        FileType::Video => ThumbCategory::Video,
        FileType::Archive => ThumbCategory::Archive,
        FileType::Executable => ThumbCategory::Executable,
        // The listing's Document covers PDF along with office formats; the
        // thumbnail side has a PDF category and nothing broader, and a red
        // document badge is closer to right for a .docx than the blank
        // Unknown page is.
        FileType::Document => ThumbCategory::Pdf,
        FileType::Unknown => ThumbCategory::Unknown,
    }
}

/// Re-derive the active column set from what the directory actually holds.
///
/// A free function rather than a method because it borrows two fields of
/// [`ExplorerState`] at once — the entries immutably and the manager mutably —
/// which the borrow checker allows at a call site but not through `&mut self`.
///
/// Paths are converted with [`Path::to_str`], not `to_string_lossy`: a name
/// that is not valid UTF-8 simply does not vote on which columns appear, which
/// is right, since every extension auto-detection looks for is ASCII. Making
/// one up with replacement characters could only produce a wrong answer.
fn detect_columns(columns: &mut ColumnManager, entries: &[FileEntry]) {
    let infos: Vec<FileInfo<'_>> = entries
        .iter()
        .map(|e| FileInfo {
            path: e.path.to_str().unwrap_or(""),
            extension: e.path.extension().and_then(|x| x.to_str()).unwrap_or(""),
        })
        .collect();
    columns.auto_detect_columns(&infos);
}

/// Check that `name` is usable as a single entry name in a directory.
///
/// The rule the OS itself enforces is "all bytes except `/` and NUL" — see
/// `design.txt`'s filesystem section — so this deliberately does *not* impose
/// Windows' extra restrictions on the name's characters. What it does reject
/// is the set of strings that are not names at all, and whose common property
/// is that they silently redirect an operation somewhere the user was not
/// looking:
///
/// - `""` — `with_file_name("")` yields the *parent* directory;
/// - `.` and `..` — resolve to the directory itself and its parent, turning a
///   rename of a file into an operation on a directory;
/// - anything containing `/` or `\` — escapes the directory being viewed, so
///   `../taken.txt` renames the file out of the folder and onto whatever is
///   there. `\` is included because this app is developed and tested on
///   Windows, where it is also a separator, and a name that escapes on the
///   development host is a bug found late.
/// - anything containing a NUL byte — cannot be passed to the OS at all.
fn validate_entry_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("the name cannot be empty".to_string());
    }
    if name == "." || name == ".." {
        return Err(format!("\"{name}\" is not a usable name"));
    }
    if name.contains('/') || name.contains('\\') {
        return Err("the name cannot contain a path separator".to_string());
    }
    if name.contains('\0') {
        return Err("the name cannot contain a null byte".to_string());
    }
    Ok(())
}

/// Whether two paths refer to the same file on disk.
///
/// Used to tell a real collision apart from a rename that only changes the
/// spelling of the name. On a case-insensitive filesystem `notes.txt` and
/// `Notes.txt` are the same file, so `new_path.exists()` is true for a
/// perfectly legitimate rename; refusing it would make case corrections
/// impossible on the development host. SlateOS's own filesystem is
/// case-sensitive, where the two are distinct and `canonicalize` says so.
///
/// A path that cannot be canonicalised is reported as *not* the same file,
/// which makes the caller take the cautious branch and refuse the rename.
fn is_same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

// `copy_dir_recursive` used to live here. It was a second, weaker
// implementation of what `fileops::OperationPlan::plan_copy` plus
// `OperationExecutor` already do — with no conflict policy, no journal, no undo
// and no error reporting. Deleted rather than patched: two implementations of
// the same operation is how the weaker one ends up on the user-facing path.

// ============================================================================
// The window
// ============================================================================

/// How often to wake while there is thumbnail work left to do.
///
/// Generation is synchronous, so this is the rate at which batches of
/// [`THUMB_BATCH_DEFAULT`] files are decoded, not a frame rate. Fast enough
/// that a folder of photographs fills in while the user is still looking at
/// it, slow enough that each batch has the frame to itself.
const THUMB_TICK_MS: u64 = 60;

impl oswindow::app::App for ExplorerState {
    /// The folder's name first, then the application's.
    ///
    /// That order is what a task bar full of windows needs: the strip of
    /// buttons is elided from the right, so leading with the application name
    /// would give every open folder the same visible label.
    fn title(&self) -> String {
        match self.current_path.file_name() {
            Some(name) => format!("{} — Files", Path::new(name).display()),
            // The root of the tree has no file name of its own.
            None => format!("{} — Files", self.current_path.display()),
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        (self.window_width, self.window_height)
    }

    /// A clock only while thumbnails remain to be generated.
    ///
    /// Consulted after every event, so opening a folder of pictures arms it and
    /// the last thumbnail disarms it. Both halves matter: without the first, a
    /// folder entered by keyboard would never generate anything, because
    /// nothing else in this application produces the events that would pump the
    /// queue; without the second, a file manager left open on a folder of text
    /// files would wake sixty times a second forever and hold the whole desktop
    /// awake to discover each time that there was nothing to do.
    ///
    /// `completed_count` is in the condition as well as `pending_count`,
    /// because a batch that finished generating has still not been filed into
    /// the cache or queued for upload — that happens in
    /// [`ExplorerState::pump_thumbnails`], which needs one more tick to run.
    fn tick_interval(&self) -> Option<std::time::Duration> {
        let working = self.thumb_gen.pending_count() > 0 || self.thumb_gen.completed_count() > 0;
        working.then(|| std::time::Duration::from_millis(THUMB_TICK_MS))
    }

    fn on_event(&mut self, event: &Event) -> oswindow::app::Response {
        if matches!(event, Event::CloseRequested) {
            return oswindow::app::Response::Exit;
        }
        if self.handle_event(event) {
            oswindow::app::Response::Redraw
        } else {
            oswindow::app::Response::Idle
        }
    }

    /// Give back what has left the cache, then hand over what has entered it.
    ///
    /// **Drops before uploads, and that order is load-bearing** (see
    /// [`ImageChange`](oswindow::app::ImageChange)). Both lists are produced by
    /// the same pump: a batch that generated N thumbnails into a full cache
    /// evicted N others, and the link's image budget is checked against
    /// `held - freed + incoming`. Uploading first would ask the compositor to
    /// hold both sets at once and be refused at exactly the moment the cache is
    /// working as designed.
    ///
    /// A thumbnail whose bytes cannot be put in wire order is skipped rather
    /// than uploaded wrong. That can only happen for a `Thumbnail` assembled by
    /// hand with mismatched fields — [`Thumbnail::to_wire_bytes`] gets the
    /// length check for free — and the entry keeps drawing its placeholder,
    /// which is what an entry with no usable picture should do.
    fn take_images(&mut self) -> Vec<oswindow::app::ImageChange> {
        use oswindow::app::ImageChange;

        let mut changes = Vec::new();
        for id in self.thumbs.take_evicted_image_ids() {
            // Only announce a drop for something believed to be held. An id the
            // compositor never took costs nothing to drop, but the bookkeeping
            // must still come off `uploaded` or the entry would keep claiming
            // it was drawable.
            if self.mark_dropped(id) {
                changes.push(ImageChange::Drop(id));
            }
        }
        for (id, thumb) in self.take_pending_uploads() {
            let Some(bytes) = thumb.to_wire_bytes() else {
                continue;
            };
            changes.push(ImageChange::Upload {
                id,
                width: thumb.width,
                height: thumb.height,
                // `Canvas` never pads, so a row is exactly its pixels.
                stride: thumb.width.saturating_mul(4),
                format: oswindow::PixelFormat::Argb8888,
                bytes,
            });
            // Optimistic, and safe to be: the event loop propagates an upload
            // failure out of `apply_images`, which ends the loop — so there is
            // no frame in which this could be believed and be wrong.
            self.mark_uploaded(id);
        }
        changes
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the compositor last reported wins over the one this state
        // remembers. They agree whenever a `Resize` was delivered; the case
        // where they do not is the very first frame, drawn before any event has
        // arrived, which would otherwise be laid out at the size the explorer
        // *asked* for rather than the size it got.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            self.window_width = width.clamp(1.0, 16384.0) as u32;
            self.window_height = height.clamp(1.0, 16384.0) as u32;
        }
        ExplorerState::render(self)
    }
}

impl ExplorerState {
    /// Apply one event, reporting whether anything visible changed.
    ///
    /// Separate from [`App::on_event`](oswindow::app::App::on_event) so the
    /// tests can drive it without a compositor, which is the only way any of
    /// this is exercised on the development host.
    ///
    /// Hit-testing goes through [`DropZoneManager`], which holds the rectangles
    /// the *last frame* actually drew. That is deliberate: the icon view's
    /// column count depends on the pane width and the list view's row heights
    /// depend on the view mode, so a click handler that recomputed the layout
    /// would be a second copy of that arithmetic — and the first time the two
    /// disagreed, the user would click one file and open another.
    #[must_use]
    pub fn handle_event(&mut self, event: &Event) -> bool {
        // A modal owns the input while it is up. Falling through to the
        // listing as well is how a Delete confirmation also moves the
        // selection, so that confirming it acts on a different file than the
        // one the dialog named.
        if self.modal.is_some() {
            return self.handle_modal_event(event);
        }
        match event {
            Event::Mouse(m) => self.handle_mouse(m),
            Event::Key(k) => k.pressed && self.handle_key(k),
            Event::Resize { width, height } => {
                if self.window_width == *width && self.window_height == *height {
                    return false;
                }
                self.window_width = *width;
                self.window_height = *height;
                true
            }
            // Each tick retires a batch. A tick that retires nothing has
            // nothing new to draw, and saying so is what stops the loop
            // repainting the whole window sixty times a second for no reason.
            Event::Tick { .. } => self.pump_thumbnails_default() > 0,
            // `SettingsChanged` is a different kind of "no" from its
            // neighbours here, and is grouped with them only because the
            // answer happens to coincide. The others are events this window
            // has nothing to *do* about; this one is an announcement that the
            // user's settings were rewritten, which this program ignores
            // because it reads no settings file at all -- it draws in its own
            // palette and takes no preference from disk. If that ever stops
            // being true, this arm is where the re-read belongs, and moving it
            // out of this group is part of the change.
            Event::CloseRequested
            | Event::Moved { .. }
            | Event::FocusIn
            | Event::FocusOut
            | Event::ScaleChanged { .. }
            | Event::SettingsChanged { .. } => false,
        }
    }

    fn handle_mouse(&mut self, m: &MouseEvent) -> bool {
        match m.kind {
            // The scrollbar first: it is drawn over the rows, so a press on it
            // is not a press on the file underneath.
            MouseEventKind::Press(MouseButton::Left) => {
                self.press_scrollbar(m.x, m.y) || self.click_at(m.x, m.y)
            }
            MouseEventKind::Release(MouseButton::Left) => {
                let was = self.thumb_grab.take();
                was.is_some()
            }
            MouseEventKind::Move if self.thumb_grab.is_some() => self.drag_scrollbar(m.y),
            MouseEventKind::DoubleClick(MouseButton::Left) => self.open_at(m.x, m.y),
            // A file manager's back/forward thumb buttons are the one mouse
            // gesture users expect to work without a toolbar.
            MouseEventKind::Press(MouseButton::Back) => self.go_back_if_possible(),
            MouseEventKind::Press(MouseButton::Forward) => self.go_forward_if_possible(),
            // Accumulated because a trackpad sends many fractional notches and
            // each would truncate to zero rows on its own.
            //
            // Not negated here: `Accumulator::rows` has already done it. `dy`
            // is positive away from the user, and the accumulator returns rows
            // *towards the end of the list*, which is the direction a row index
            // grows. Negating again scrolled up from the top and therefore
            // nowhere -- the failure this test caught.
            MouseEventKind::Scroll { dy, .. } => {
                let rows = self.wheel.rows(dy);
                if rows == 0 {
                    return false;
                }
                // In the grid a "row" is a row of icons, which is `cols`
                // entries. Without this a notch moves three entries -- less
                // than one visible row on any pane wider than three cells --
                // and the view appears not to respond.
                let step = match self.view_mode {
                    ViewMode::Icons => rows.saturating_mul(self.icon_columns() as isize),
                    ViewMode::Details | ViewMode::List => rows,
                };
                self.viewport.scroll_by(step, self.entries.len());
                true
            }
            _ => false,
        }
    }

    /// A single left click: select the row under the pointer, follow the
    /// sidebar place under it, or clear the selection.
    fn click_at(&mut self, x: f32, y: f32) -> bool {
        if let Some(index) = self.dropzone.find_file_row(x, y) {
            self.select_single(index);
            return true;
        }
        if let Some(path) = self.dropzone.find_sidebar_item(x, y) {
            let path = path.to_path_buf();
            self.navigate_to(&path);
            return true;
        }
        if self.selected_indices.is_empty() {
            return false;
        }
        self.deselect_all();
        true
    }

    /// A double click opens whatever it landed on; a double click on empty
    /// space does nothing, rather than opening the last thing selected.
    fn open_at(&mut self, x: f32, y: f32) -> bool {
        let Some(index) = self.dropzone.find_file_row(x, y) else {
            return false;
        };
        self.open_entry(index);
        true
    }

    fn go_back_if_possible(&mut self) -> bool {
        if self.history_back.is_empty() {
            return false;
        }
        self.go_back();
        true
    }

    fn go_forward_if_possible(&mut self) -> bool {
        if self.history_forward.is_empty() {
            return false;
        }
        self.go_forward();
        true
    }

    /// The keyboard map. Returns whether anything visible changed.
    ///
    /// Deliberately the navigation and selection keys only. The keys that
    /// *edit* — Delete, F2, Ctrl+V — are not wired here because they need a
    /// confirmation and a rename field that this window does not have yet, and
    /// a Delete key that recycled a file with no prompt and no visible undo
    /// would be worse than one that does nothing. See
    /// `TD-C-EXPLORER-HAS-NO-EDITING-KEYS`.
    fn handle_key(&mut self, k: &KeyEvent) -> bool {
        let ctrl = k.modifiers.ctrl;
        match k.key {
            Key::A if ctrl => {
                if self.entries.is_empty() {
                    return false;
                }
                self.select_all();
                true
            }
            Key::Backspace => self.go_up_if_possible(),
            Key::Left if k.modifiers.alt => self.go_back_if_possible(),
            Key::Right if k.modifiers.alt => self.go_forward_if_possible(),
            Key::Up | Key::Left => self.move_selection(-1),
            Key::Down | Key::Right => self.move_selection(1),
            Key::Home => self.move_selection_to(0),
            Key::End => self.move_selection_to(self.entries.len().saturating_sub(1)),
            Key::Enter => match self.selected_indices.first() {
                Some(&index) => {
                    self.open_entry(index);
                    true
                }
                None => false,
            },
            Key::Escape => {
                if self.selected_indices.is_empty() {
                    return false;
                }
                self.deselect_all();
                true
            }
            Key::F5 => {
                self.load_directory();
                true
            }
            // Shift+Delete is the permanent one, by long convention. It is
            // matched first because `Key::Delete` below would otherwise take
            // it and quietly recycle instead.
            Key::Delete if k.modifiers.shift => self.ask_delete(PendingAction::DeletePermanently),
            Key::Delete => self.ask_delete(PendingAction::Recycle),
            Key::F2 => self.ask_rename(),
            Key::Z if ctrl => {
                self.undo_last();
                true
            }
            Key::C if ctrl => {
                self.copy_selected();
                true
            }
            Key::X if ctrl => {
                self.cut_selected();
                true
            }
            Key::V if ctrl => {
                self.paste();
                true
            }
            Key::H if ctrl => {
                self.toggle_hidden();
                true
            }
            _ => false,
        }
    }

    /// Count and name the selection, for a dialog that has to be specific.
    ///
    /// "Delete 3 items?" and "Delete 'notes.txt'?" are different questions,
    /// and the second is the one that lets a user notice they selected the
    /// wrong file. A dialog that always said "the selected items" would be
    /// exactly as true and no use at all.
    fn selection_summary(&self) -> Option<(usize, String)> {
        let mut selected = self.entries.iter().filter(|e| e.selected);
        let first = selected.next()?;
        let rest = selected.count();
        let name = first.path.file_name().map_or_else(
            || first.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        Some((rest.saturating_add(1), name))
    }

    /// Put up the confirmation for a delete, or refuse if nothing is selected.
    fn ask_delete(&mut self, action: PendingAction) -> bool {
        let Some((count, name)) = self.selection_summary() else {
            self.status_message = "Nothing selected".to_string();
            return true;
        };

        let subject = if count == 1 {
            format!("\"{name}\"")
        } else {
            format!("{count} items")
        };

        let dialog = match action {
            PendingAction::Recycle => AlertDialog::destructive(
                "Delete",
                &format!("Move {subject} to the recycle bin?"),
                "Delete",
            )
            .with_detail("You can put it back from the recycle bin, or with Ctrl+Z."),
            PendingAction::DeletePermanently => AlertDialog::destructive(
                "Delete permanently",
                &format!("Permanently delete {subject}?"),
                "Delete permanently",
            )
            .with_detail("This cannot be undone. The data is erased, not recycled."),
        };

        let mut dialog = dialog;
        dialog.show();
        self.modal = Some(Modal::Confirm { dialog, action });
        true
    }

    /// Put up the rename box for the first selected entry.
    ///
    /// A dialog rather than an edit field drawn into the row: the row editor
    /// is the nicer of the two and it does not exist, and a rename that works
    /// through a plain box is worth more than a rename that is still absent
    /// because the nicer version was a bigger job.
    fn ask_rename(&mut self) -> bool {
        let Some(&index) = self.selected_indices.first() else {
            self.status_message = "Nothing selected".to_string();
            return true;
        };
        let Some(entry) = self.entries.get(index) else {
            return false;
        };
        let target = entry.path.clone();
        let current = target
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());

        let mut dialog =
            InputDialog::prompt("Rename", "New name:", &current).with_initial_text(&current);
        dialog.show();
        self.modal = Some(Modal::Rename { dialog, target });
        true
    }

    /// Feed one event to the open modal, and act if it has answered.
    fn handle_modal_event(&mut self, event: &Event) -> bool {
        let Some(modal) = self.modal.as_mut() else {
            return false;
        };

        let consumed = match modal {
            Modal::Confirm { dialog, .. } | Modal::Notice { dialog } => dialog.handle_event(event),
            Modal::Rename { dialog, .. } => dialog.handle_event(event),
        } == EventResult::Consumed;

        let answer = match modal {
            Modal::Confirm { dialog, .. } | Modal::Notice { dialog } => dialog.result().cloned(),
            Modal::Rename { dialog, .. } => dialog.result().cloned(),
        };

        let Some(answer) = answer else {
            return consumed;
        };

        // Taken before acting: the action reloads the directory and sets a
        // status message, and doing that with the dialog still up would leave
        // it drawn over a listing that no longer matches what it asked about.
        let modal = self.modal.take();
        self.apply_modal_answer(modal, answer);
        true
    }

    /// Carry out what the answered modal was asking about.
    fn apply_modal_answer(&mut self, modal: Option<Modal>, answer: DialogResult) {
        match modal {
            Some(Modal::Confirm { action, .. }) => {
                // Only the affirmative acts. `Dismissed` covers Escape and a
                // click outside, and both mean no.
                if matches!(answer, DialogResult::Ok | DialogResult::Yes) {
                    self.delete_selected(action == PendingAction::DeletePermanently);
                } else {
                    self.status_message = "Delete cancelled".to_string();
                }
            }
            Some(Modal::Rename { target, .. }) => match answer {
                DialogResult::Text(name) => self.rename_path(&target, &name),
                _ => self.status_message = "Rename cancelled".to_string(),
            },
            // Dismissing a notice is the whole of what a notice does. It
            // has already been reported; there is nothing left to carry out.
            Some(Modal::Notice { .. }) | None => {}
        }
    }

    /// Rename the entry currently holding `target`.
    ///
    /// Looks the row up by path rather than trusting an index captured when
    /// the dialog opened; if the file has gone in the meantime, that is
    /// reported rather than renaming whatever now sits in that row.
    fn rename_path(&mut self, target: &Path, new_name: &str) {
        match self.entries.iter().position(|e| e.path == target) {
            Some(index) => self.rename_entry(index, new_name),
            None => {
                self.report(Outcome::failed(
                    "Rename failed: the file is no longer there".to_string(),
                    "That file is no longer in this folder, so it was not renamed.".to_string(),
                ));
            }
        }
    }

    /// Reverse the most recent operation, and say what actually came back.
    ///
    /// The count matters: a record can be legitimately un-undoable (a
    /// permanent delete leaves nothing to restore), and reporting "Undone"
    /// for that is the same lie the undo journal used to tell itself.
    fn undo_last(&mut self) {
        let Some(record) = self.undo.pop() else {
            self.status_message = "Nothing to undo".to_string();
            return;
        };

        let outcome = match fileops::execute_undo(&record, Some(&self.recycle)) {
            Ok(0) => {
                Outcome::ok("Nothing to undo: those items were deleted permanently".to_string())
            }
            Ok(n) => Outcome::ok(format!("Undone: {n} item(s) restored")),
            Err(e) => Outcome::failed(
                format!("Undo failed: {e}"),
                format!("The last operation could not be reversed.\n\n{e}"),
            ),
        };
        // Reload first: `report` may raise a dialog, and it should be drawn
        // over the listing as it is *after* the undo, not before.
        self.load_directory();
        self.report(outcome);
    }

    fn go_up_if_possible(&mut self) -> bool {
        if self.current_path.parent().is_none() {
            return false;
        }
        self.go_up();
        true
    }

    /// Move the selection by `delta` rows, clamped to the listing.
    ///
    /// Clamped rather than wrapping: holding Down in a long folder should stop
    /// at the last file, not return to the first, which is what every file
    /// manager does and what stops a held key from cycling forever.
    ///
    /// Saturating in both directions on purpose — `selected_indices` holds
    /// `usize`, so a naive `- 1` at row zero would wrap to the end of the
    /// listing rather than staying put.
    fn move_selection(&mut self, delta: isize) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let current = self.selected_indices.first().copied();
        let next = match (current, delta.is_negative()) {
            // Nothing selected: the first key press selects an end of the
            // listing rather than moving from an imaginary position.
            (None, true) => self.entries.len().saturating_sub(1),
            (None, false) => 0,
            (Some(i), true) => i.saturating_sub(delta.unsigned_abs()),
            (Some(i), false) => i.saturating_add(delta.unsigned_abs()),
        };
        self.move_selection_to(next)
    }

    fn move_selection_to(&mut self, index: usize) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let index = index.min(self.entries.len().saturating_sub(1));
        if self.selected_indices.as_slice() == [index] {
            return false;
        }
        self.select_single(index);
        // The viewport follows the cursor row, which is what stops the arrow
        // keys walking the selection off the bottom of the window -- the
        // symptom that made this look like a *lost* selection rather than an
        // invisible one.
        self.viewport.select(Some(index), self.entries.len());
        true
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() -> std::process::ExitCode {
    // The folder to open, then the home directory, then the root. A path given
    // on the command line is what makes "open containing folder" possible from
    // anywhere else in the desktop.
    let start_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"));

    let mut explorer = ExplorerState::new(&start_path);
    oswindow::app::launch("explorer", &mut explorer)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;
    use scratchdir::ScratchDir;
    use std::time::Duration;

    /// A private scratch directory for one test, removed when the returned
    /// guard drops.
    ///
    /// The name used to carry the system clock in nanoseconds, which is not
    /// unique: `cargo test` runs a binary's tests as threads of one process,
    /// and the clock a thread reads is only refreshed on a timer interrupt, so
    /// every test that starts within the same tick draws the same tag and they
    /// share — and corrupt — one directory. `ScratchDir` names itself from the
    /// process id and a per-process atomic counter, which is unique by
    /// construction.
    ///
    /// Bind the guard to a named local, never to `_`: `_` drops it immediately
    /// and the directory is gone before the test's first line.
    fn temp_dir(label: &str) -> ScratchDir {
        ScratchDir::new(&format!("explorer_test_{label}"))
    }

    fn write(path: &Path, content: &str) {
        fs::write(path, content).expect("write test file");
    }

    /// Build a state rooted at `dir` without touching the real home directory,
    /// and with a recycle bin and thumbnail cache that also live under `dir`.
    ///
    /// The thumbnail generator is redirected deliberately: the production
    /// default writes to `~/.cache/thumbs`, and a test suite that generated
    /// into a developer's real cache would both pollute it and read entries
    /// from it, so the same test would pass or fail depending on what the
    /// developer had browsed.
    fn state_at(dir: &Path) -> ExplorerState {
        let mut state = ExplorerState::new(dir);
        state.recycle = RecycleBin::new(dir.join(".recycle"), Duration::from_secs(3600));
        state.thumb_gen =
            ThumbnailGenerator::with_disk_cache(thumbs::DiskCache::new(dir.join(".thumbs")));
        state.queue_thumbnails();
        state
    }

    /// A directory holding `n` files, so a listing can be longer than a window.
    fn dir_with_files(dir: &Path, n: usize) {
        for i in 0..n {
            std::fs::write(dir.join(format!("file{i:03}.txt")), b"x").expect("write");
        }
    }

    /// The rows a details-view render actually drew, by name.
    fn drawn_rows(state: &ExplorerState, h: f32) -> Vec<String> {
        let rows = scroll_window::visible(
            state.entries.len(),
            ROW_H,
            (h - HEADER_H).max(0.0),
            state.viewport.first_visible(),
        );
        (rows.start..rows.end())
            .filter_map(|i| state.entries.get(i).map(|e| e.name.clone()))
            .collect()
    }

    #[test]
    fn a_listing_longer_than_the_window_can_be_scrolled_to_its_end() {
        // The defect: the renderer drew as many rows as fit and stopped, so
        // every file past the bottom edge was unreachable -- not merely
        // awkward to reach, but not drawn and not clickable at any point.
        let dir = ScratchDir::new("explorer-scroll");
        dir_with_files(&dir.path(""), 60);
        let mut state = state_at(&dir.path(""));
        let window_h = HEADER_H + 10.0 * ROW_H;

        let first_page = drawn_rows(&state, window_h);
        assert_eq!(first_page.len(), 10, "ten rows should fit");
        assert!(!first_page.iter().any(|n| n == "file059.txt"));

        state.viewport.scroll_by(60, state.entries.len());
        let last_page = drawn_rows(&state, window_h);
        assert!(
            last_page.iter().any(|n| n == "file059.txt"),
            "the last file is still unreachable: {last_page:?}"
        );
    }

    #[test]
    fn the_wheel_scrolls_and_a_trackpads_fractions_are_not_lost() {
        let dir = ScratchDir::new("explorer-wheel");
        dir_with_files(&dir.path(""), 60);
        let mut state = state_at(&dir.path(""));

        let wheel = |dy: f32| MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        };
        // One full notch away from the user moves down the list.
        state.handle_mouse(&wheel(-1.0));
        let after_one = state.viewport.first_visible();
        assert!(after_one > 0, "a whole notch scrolled nothing");

        // Ten tenth-notches are one notch. Each on its own truncates to zero
        // rows, so without the accumulator a trackpad would be dead.
        let before = state.viewport.first_visible();
        for _ in 0..10 {
            state.handle_mouse(&wheel(-0.1));
        }
        assert!(
            state.viewport.first_visible() > before,
            "ten tenth-notches scrolled nothing, so a trackpad is dead"
        );
    }

    #[test]
    fn the_wheel_does_not_scroll_past_either_end() {
        let dir = ScratchDir::new("explorer-wheel-ends");
        dir_with_files(&dir.path(""), 12);
        let mut state = state_at(&dir.path(""));
        state.viewport.set_height(10, state.entries.len());

        state.viewport.scroll_by(1_000, state.entries.len());
        let bottom = state.viewport.first_visible();
        assert!(
            bottom <= state.entries.len().saturating_sub(10),
            "scrolled past the last page to {bottom}"
        );

        state.viewport.scroll_by(-1_000, state.entries.len());
        assert_eq!(state.viewport.first_visible(), 0, "scrolled above the top");
    }

    #[test]
    fn arrowing_down_past_the_fold_brings_the_view_with_it() {
        // The symptom that made this read as a *lost* selection: the arrow
        // keys moved an index into the full listing, the renderer drew only
        // the first screenful, and the highlighted row was off screen.
        let dir = ScratchDir::new("explorer-follow");
        dir_with_files(&dir.path(""), 60);
        let mut state = state_at(&dir.path(""));
        state.viewport.set_height(10, state.entries.len());

        for _ in 0..20 {
            state.move_selection(1);
        }
        let cursor = state
            .selected_indices
            .first()
            .copied()
            .expect("a selection");
        let range = state.viewport.visible_range(state.entries.len());
        assert!(
            range.contains(&cursor),
            "the selected row {cursor} is outside the drawn range {range:?}"
        );
    }

    #[test]
    fn the_icon_grid_scrolls_and_reaches_the_last_file() {
        let dir = ScratchDir::new("explorer-icons-scroll");
        dir_with_files(&dir.path(""), 60);
        let mut state = state_at(&dir.path(""));
        state.view_mode = ViewMode::Icons;

        let cols = state.icon_columns();
        assert!(cols >= 1);
        state.viewport.scroll_by(60, state.entries.len());
        assert!(
            state.viewport.first_visible() > 0,
            "the grid did not scroll at all"
        );
    }

    #[test]
    fn a_wheel_notch_in_the_grid_moves_a_whole_row_of_icons() {
        // A notch is three rows, and in the grid a row is `cols` entries. If
        // the step were counted in entries, a notch would move three files --
        // less than one visible row on any pane wider than three cells -- and
        // the grid would look unresponsive.
        let dir = ScratchDir::new("explorer-icons-wheel");
        dir_with_files(&dir.path(""), 200);
        let mut state = state_at(&dir.path(""));
        state.view_mode = ViewMode::Icons;
        let cols = state.icon_columns();

        state.handle_mouse(&MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
        });

        assert!(
            state.viewport.first_visible() >= cols,
            "a notch moved {} entries, less than one row of {cols}",
            state.viewport.first_visible()
        );
    }

    #[test]
    fn a_scrolled_icon_cell_names_the_file_that_is_drawn_in_it() {
        // The sharpest thing that can go wrong in a scrolled grid: the cell is
        // laid out by its position on screen and identified by its index in
        // the listing, and using one number for both means the first cell
        // after a scroll claims to be the first file in the folder. A click
        // would then open the wrong file, silently.
        let dir = ScratchDir::new("explorer-icons-zones");
        dir_with_files(&dir.path(""), 60);
        let mut state = state_at(&dir.path(""));
        state.view_mode = ViewMode::Icons;
        let cols = state.icon_columns();
        state.viewport.scroll_by(cols as isize, state.entries.len());

        // Through the real path: render to register the zones, then click
        // the top-left cell and see which file the program thinks was hit.
        drop(state.render());
        let first_drawn = (state.viewport.first_visible() / cols) * cols;
        assert_ne!(
            first_drawn, 0,
            "the grid did not scroll, so this proves nothing"
        );

        let clicked = state.click_at(state.sidebar_width + 10.0, 64.0 + 10.0);
        assert!(clicked, "the top-left cell was not clickable");
        assert_eq!(
            state.selected_indices.as_slice(),
            [first_drawn],
            "the top-left cell named file {:?} instead of the one drawn in it",
            state.selected_indices
        );
    }

    fn press(state: &mut ExplorerState, x: f32, y: f32) -> bool {
        state.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    #[test]
    fn a_short_listing_has_no_scrollbar() {
        // A permanent grey stripe beside a three-item folder reads as broken.
        let dir = ScratchDir::new("explorer-sb-short");
        dir_with_files(&dir.path(""), 3);
        let state = state_at(&dir.path(""));
        assert!(state.scrollbar_track().is_none());
    }

    #[test]
    fn a_long_listing_has_one_and_the_thumb_tracks_the_view() {
        let dir = ScratchDir::new("explorer-sb-long");
        dir_with_files(&dir.path(""), 200);
        let mut state = state_at(&dir.path(""));
        let track = state.scrollbar_track().expect("a long listing needs a bar");
        let top = state.scrollbar_thumb().expect("and a thumb").y;

        state.viewport.scroll_by(200, state.entries.len());
        let bottom = state.scrollbar_thumb().expect("still a thumb").y;

        assert!(bottom > top, "the thumb did not move with the view");
        assert!(
            bottom + state.scrollbar_thumb().unwrap().h <= track.y + track.height + 0.01,
            "the thumb ran past the end of its track"
        );
    }

    #[test]
    fn dragging_the_thumb_scrolls_the_listing() {
        let dir = ScratchDir::new("explorer-sb-drag");
        dir_with_files(&dir.path(""), 200);
        let mut state = state_at(&dir.path(""));
        let track = state.scrollbar_track().expect("a bar");
        let thumb = state.scrollbar_thumb().expect("a thumb");

        // Take hold of the thumb, then drag to the bottom of the track.
        assert!(press(&mut state, track.x + 2.0, thumb.y + 2.0));
        state.handle_mouse(&MouseEvent {
            x: track.x + 2.0,
            y: track.y + track.height,
            kind: MouseEventKind::Move,
        });

        assert!(
            state.viewport.first_visible() > 0,
            "dragging the thumb to the bottom scrolled nothing"
        );
    }

    #[test]
    fn a_press_on_the_scrollbar_is_not_a_press_on_the_file_behind_it() {
        // The bar is drawn *over* the rows, so without the check the click
        // falls through and selects whatever file the thumb happens to cover --
        // the kind of wrong that looks like a misclick and is not.
        let dir = ScratchDir::new("explorer-sb-steal");
        dir_with_files(&dir.path(""), 200);
        let mut state = state_at(&dir.path(""));
        state.selected_indices.clear();
        let track = state.scrollbar_track().expect("a bar");
        let thumb = state.scrollbar_thumb().expect("a thumb");

        press(&mut state, track.x + 2.0, thumb.y + 2.0);

        assert!(
            state.selected_indices.is_empty(),
            "pressing the scrollbar selected a file: {:?}",
            state.selected_indices
        );
        assert!(
            state.thumb_grab.is_some(),
            "the press did not take the thumb"
        );
    }

    #[test]
    fn grabbing_the_thumb_low_and_not_moving_does_not_jump_the_view() {
        // The wiring the toolkit's own test cannot reach: `scrollbar` proves
        // the arithmetic honours a grab offset, but explorer has to *pass*
        // one. Passing zero compiles, drags smoothly, and jumps the view the
        // instant you take hold anywhere but the thumb's very top -- a defect
        // that looks like a twitchy scrollbar rather than a bug.
        let dir = ScratchDir::new("explorer-sb-grab");
        dir_with_files(&dir.path(""), 200);
        let mut state = state_at(&dir.path(""));
        state.viewport.scroll_by(80, state.entries.len());
        let before = state.viewport.first_visible();
        let track = state.scrollbar_track().expect("a bar");
        let thumb = state.scrollbar_thumb().expect("a thumb");

        // Take hold near the thumb's bottom, then move to exactly where the
        // pointer already is. Nothing has moved, so nothing should scroll.
        let grab_y = thumb.y + thumb.h - 2.0;
        press(&mut state, track.x + 2.0, grab_y);
        state.handle_mouse(&MouseEvent {
            x: track.x + 2.0,
            y: grab_y,
            kind: MouseEventKind::Move,
        });

        assert_eq!(
            state.viewport.first_visible(),
            before,
            "taking hold of the thumb moved the view without the pointer moving"
        );
    }

    #[test]
    fn a_release_lets_go_of_the_thumb() {
        let dir = ScratchDir::new("explorer-sb-release");
        dir_with_files(&dir.path(""), 200);
        let mut state = state_at(&dir.path(""));
        let track = state.scrollbar_track().expect("a bar");
        let thumb = state.scrollbar_thumb().expect("a thumb");
        press(&mut state, track.x + 2.0, thumb.y + 2.0);
        assert!(state.thumb_grab.is_some());

        state.handle_mouse(&MouseEvent {
            x: track.x + 2.0,
            y: thumb.y + 2.0,
            kind: MouseEventKind::Release(MouseButton::Left),
        });
        assert!(
            state.thumb_grab.is_none(),
            "the thumb is still held after the button came up"
        );
    }

    #[test]
    fn clicking_the_track_below_the_thumb_pages_down() {
        let dir = ScratchDir::new("explorer-sb-page");
        dir_with_files(&dir.path(""), 200);
        let mut state = state_at(&dir.path(""));
        let track = state.scrollbar_track().expect("a bar");
        let thumb = state.scrollbar_thumb().expect("a thumb");

        press(&mut state, track.x + 2.0, thumb.y + thumb.h + 4.0);

        assert!(
            state.viewport.first_visible() > 0,
            "clicking below the thumb did not page down"
        );
        assert!(
            state.thumb_grab.is_none(),
            "a click on the track should not grab the thumb"
        );
    }

    fn select_named(state: &mut ExplorerState, name: &str) {
        for entry in &mut state.entries {
            entry.selected = entry.name == name;
        }
    }

    // ------------------------------------------------------------------
    // Paste
    // ------------------------------------------------------------------

    #[test]
    fn pasting_over_an_existing_file_does_not_destroy_it() {
        let root_scratch = temp_dir("paste_conflict");
        let root = root_scratch.dir().to_path_buf();
        let src_dir = root.join("src");
        let dst_dir = root.join("dst");
        fs::create_dir_all(&src_dir).expect("src");
        fs::create_dir_all(&dst_dir).expect("dst");
        write(&src_dir.join("notes.txt"), "new");
        write(&dst_dir.join("notes.txt"), "OLD AND IRREPLACEABLE");

        let mut state = state_at(&dst_dir);
        state.clipboard = Some(ClipboardOp::Copy(vec![src_dir.join("notes.txt")]));
        state.paste();

        assert_eq!(
            fs::read_to_string(dst_dir.join("notes.txt")).expect("original must survive"),
            "OLD AND IRREPLACEABLE",
            "a paste must never silently overwrite an existing file"
        );
        let renamed: Vec<_> = fs::read_dir(&dst_dir)
            .expect("list dst")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "notes.txt")
            .collect();
        assert_eq!(
            renamed.len(),
            1,
            "the pasted copy should land beside the original, got {renamed:?}"
        );
    }

    #[test]
    fn a_paste_that_could_not_copy_anything_says_so() {
        let root_scratch = temp_dir("paste_missing");
        let root = root_scratch.dir().to_path_buf();
        let dst_dir = root.join("dst");
        fs::create_dir_all(&dst_dir).expect("dst");

        let mut state = state_at(&dst_dir);
        // A source that does not exist: planning fails, nothing is copied.
        state.clipboard = Some(ClipboardOp::Copy(vec![root.join("does_not_exist.txt")]));
        state.paste();

        assert!(
            !state.status_message.starts_with("Pasted 1"),
            "a paste that copied nothing must not claim it copied something: {}",
            state.status_message
        );
        assert!(
            state.clipboard.is_some(),
            "a paste that changed nothing should leave the clipboard usable"
        );
    }

    #[test]
    fn a_cut_clears_the_clipboard_but_a_copy_does_not() {
        let root_scratch = temp_dir("paste_clipboard");
        let root = root_scratch.dir().to_path_buf();
        let src_dir = root.join("src");
        let dst_dir = root.join("dst");
        fs::create_dir_all(&src_dir).expect("src");
        fs::create_dir_all(&dst_dir).expect("dst");
        write(&src_dir.join("a.txt"), "a");
        write(&src_dir.join("b.txt"), "b");

        let mut state = state_at(&dst_dir);
        state.clipboard = Some(ClipboardOp::Copy(vec![src_dir.join("a.txt")]));
        state.paste();
        assert!(
            state.clipboard.is_some(),
            "the sources of a copy are still there, so a second paste is meaningful"
        );

        state.clipboard = Some(ClipboardOp::Cut(vec![src_dir.join("b.txt")]));
        state.paste();
        assert!(
            state.clipboard.is_none(),
            "a cut consumed its sources; pasting again would find nothing"
        );
        assert!(
            !src_dir.join("b.txt").exists(),
            "a cut should have moved the source away"
        );
        assert!(dst_dir.join("b.txt").exists(), "and into the destination");
    }

    // ------------------------------------------------------------------
    // Delete
    // ------------------------------------------------------------------

    #[test]
    fn a_recycled_file_can_be_restored() {
        let root_scratch = temp_dir("delete_restore");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        state.delete_selected(false);

        assert!(!root.join("notes.txt").exists(), "the file should be gone");
        let listed = state.recycle.list().expect("the bin must be listable");
        assert_eq!(
            listed.len(),
            1,
            "a recycled file must appear in the bin, not vanish into a flat directory"
        );
        let id = listed.first().expect("one entry").id.clone();
        state.recycle.restore(&id).expect("restore must work");
        assert_eq!(
            fs::read_to_string(root.join("notes.txt")).expect("restored"),
            "keep me"
        );
    }

    /// The undo stack's record of a recycle must actually undo it.
    ///
    /// This is the undo that a Delete key's confirmation implicitly promises,
    /// and until this test it was never exercised: the only restore test goes
    /// through the bin's own listing and takes the id from there, which works
    /// and says nothing about the undo stack.
    #[test]
    fn the_undo_record_for_a_recycle_can_actually_be_undone() {
        let root_scratch = temp_dir("undo_recycle");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        state.delete_selected(false);
        assert!(!root.join("notes.txt").exists(), "the file should be gone");

        let record = state
            .undo
            .pop()
            .expect("a recycle must leave an undo record");
        let restored = fileops::execute_undo(&record, Some(&state.recycle))
            .expect("undoing a recycle must not error");
        assert_eq!(restored, 1, "undo must report the one file it put back");

        assert_eq!(
            fs::read_to_string(root.join("notes.txt")).expect("the file must be back"),
            "keep me",
            "undo reported success, so the file must actually be restored"
        );
    }

    #[test]
    fn two_recycled_files_of_the_same_name_do_not_collide() {
        let root_scratch = temp_dir("delete_collision");
        let root = root_scratch.dir().to_path_buf();
        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir_all(&a).expect("a");
        fs::create_dir_all(&b).expect("b");
        write(&a.join("notes.txt"), "from a");
        write(&b.join("notes.txt"), "from b");

        let mut state = state_at(&a);
        select_named(&mut state, "notes.txt");
        state.delete_selected(false);

        state.navigate_to(&b);
        select_named(&mut state, "notes.txt");
        state.delete_selected(false);

        let listed = state.recycle.list().expect("list");
        assert_eq!(
            listed.len(),
            2,
            "deleting a second file of the same name must not destroy the first"
        );
    }

    #[test]
    fn a_delete_that_failed_does_not_report_success() {
        let root_scratch = temp_dir("delete_failure");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("gone.txt"), "x");

        let mut state = state_at(&root);
        select_named(&mut state, "gone.txt");
        // Remove it behind the explorer's back, so the recycle attempt fails
        // on a path the UI still believes exists.
        fs::remove_file(root.join("gone.txt")).expect("remove");
        state.delete_selected(false);

        assert!(
            state.status_message.contains("0 of 1"),
            "a delete that moved nothing must say so, got: {}",
            state.status_message
        );
    }

    #[test]
    fn deleting_with_nothing_selected_reports_nothing_selected() {
        let root_scratch = temp_dir("delete_empty");
        let root = root_scratch.dir().to_path_buf();
        let mut state = state_at(&root);
        state.delete_selected(false);
        assert_eq!(state.status_message, "Nothing selected");
    }

    // ------------------------------------------------------------------
    // Status bar
    // ------------------------------------------------------------------

    #[test]
    fn an_operation_result_survives_the_directory_reload_that_follows_it() {
        let root_scratch = temp_dir("status_survives");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("gone.txt"), "x");

        let mut state = state_at(&root);
        select_named(&mut state, "gone.txt");
        state.delete_selected(false);

        // Every operation ends with `load_directory`, which recomputes the
        // folder/file summary. That must not erase what just happened.
        assert!(
            state.status_bar_text().contains("recycle bin"),
            "the status bar should still show the operation result, got: {}",
            state.status_bar_text()
        );
        assert!(
            state.dir_summary.contains("folder(s)"),
            "and the summary should have been recomputed alongside it"
        );
    }

    #[test]
    fn navigating_away_drops_the_previous_directorys_message() {
        let root_scratch = temp_dir("status_navigate");
        let root = root_scratch.dir().to_path_buf();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).expect("sub");
        write(&root.join("gone.txt"), "x");

        let mut state = state_at(&root);
        select_named(&mut state, "gone.txt");
        state.delete_selected(false);
        assert!(state.status_bar_text().contains("recycle bin"));

        state.navigate_to(&sub);
        assert_eq!(
            state.status_bar_text(),
            state.dir_summary,
            "a message about another directory should not follow the user around"
        );
    }

    #[test]
    fn an_unreadable_directory_reports_the_error_rather_than_an_empty_listing() {
        let root_scratch = temp_dir("status_unreadable");
        let root = root_scratch.dir().to_path_buf();
        let mut state = state_at(&root);
        // A path that is not a directory at all: `read_dir` fails.
        write(&root.join("plain.txt"), "x");
        state.current_path = root.join("plain.txt");
        state.status_message.clear();
        state.load_directory();

        assert!(
            state.status_bar_text().starts_with("Error:"),
            "a listing that failed must say so rather than claim zero files, got: {}",
            state.status_bar_text()
        );
    }

    // ------------------------------------------------------------------
    // Clipboard counts
    // ------------------------------------------------------------------

    #[test]
    fn the_copied_count_matches_what_was_actually_copied() {
        let root_scratch = temp_dir("copy_count");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "a");
        write(&root.join("b.txt"), "b");

        let mut state = state_at(&root);
        for entry in &mut state.entries {
            entry.selected = true;
        }
        // Deliberately out of step with `entry.selected`, which is what the
        // status line used to count.
        state.selected_indices = vec![0];
        state.copy_selected();
        assert_eq!(state.status_message, "2 item(s) copied to clipboard");
    }

    // ------------------------------------------------------------------
    // Rename and folder creation
    // ------------------------------------------------------------------

    fn index_of(state: &ExplorerState, name: &str) -> usize {
        state
            .entries
            .iter()
            .position(|e| e.name == name)
            .unwrap_or_else(|| panic!("no entry named {name}"))
    }

    /// `fs::rename` overwrites its destination. Renaming `draft.txt` to
    /// `notes.txt` when a `notes.txt` is sitting right there therefore
    /// destroyed the user's notes and reported "Renamed to: notes.txt".
    ///
    /// Paste was already fixed for exactly this ("it overwrote an existing
    /// destination file without asking"); rename kept the bug. The policy has
    /// to match: never silently destroy, make the user delete first.
    #[test]
    fn a_rename_onto_an_existing_file_does_not_destroy_it() {
        let root_scratch = temp_dir("rename_clobber");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "the notes I care about");
        write(&root.join("draft.txt"), "a throwaway draft");

        let mut state = state_at(&root);
        let idx = index_of(&state, "draft.txt");
        state.rename_entry(idx, "notes.txt");

        assert_eq!(
            fs::read_to_string(root.join("notes.txt")).unwrap(),
            "the notes I care about",
            "the existing file must survive"
        );
        assert!(
            root.join("draft.txt").exists(),
            "the rename must not happen"
        );
        assert!(
            state.status_message.contains("already exists"),
            "the user must be told why: {}",
            state.status_message
        );
    }

    /// `with_file_name` does not constrain the result to the same directory,
    /// so a name of `../taken.txt` renamed the file *out* of the folder the
    /// user was looking at — and onto whatever was already there.
    #[test]
    fn a_rename_cannot_escape_the_current_directory() {
        let root_scratch = temp_dir("rename_escape");
        let root = root_scratch.dir().to_path_buf();
        let inner = root.join("inner");
        fs::create_dir_all(&inner).unwrap();
        write(&inner.join("file.txt"), "inner file");
        write(&root.join("taken.txt"), "outside file");

        let mut state = state_at(&inner);
        let idx = index_of(&state, "file.txt");
        state.rename_entry(idx, "../taken.txt");

        assert_eq!(
            fs::read_to_string(root.join("taken.txt")).unwrap(),
            "outside file",
            "a file outside the directory must not be touched"
        );
        assert!(inner.join("file.txt").exists(), "the file must stay put");
    }

    /// The same escape through the folder-creation path.
    #[test]
    fn a_new_folder_cannot_escape_the_current_directory() {
        let root_scratch = temp_dir("mkdir_escape");
        let root = root_scratch.dir().to_path_buf();
        let inner = root.join("inner");
        fs::create_dir_all(&inner).unwrap();

        let mut state = state_at(&inner);
        state.create_folder("../escaped");

        assert!(
            !root.join("escaped").exists(),
            "a folder must not be created outside the directory being viewed"
        );
    }

    /// An empty name, `.` and `..` are not names. Left unchecked they turn a
    /// rename into an operation on the directory itself.
    #[test]
    fn a_rename_rejects_names_that_are_not_names() {
        let root_scratch = temp_dir("rename_badnames");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("file.txt"), "data");

        for bad in ["", ".", "..", "a/b"] {
            let mut state = state_at(&root);
            let idx = index_of(&state, "file.txt");
            state.rename_entry(idx, bad);
            assert!(
                root.join("file.txt").exists(),
                "rename to {bad:?} must not move the file"
            );
            assert!(
                state.status_message.contains("Rename failed"),
                "rename to {bad:?} must report a failure, got: {}",
                state.status_message
            );
        }
    }

    /// Renaming to the name it already has is a no-op the user may well
    /// trigger by pressing Enter in the rename box. It must not be reported as
    /// a collision with itself, and must not delete anything.
    #[test]
    fn a_rename_to_the_same_name_is_harmless() {
        let root_scratch = temp_dir("rename_noop");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("file.txt"), "data");

        let mut state = state_at(&root);
        let idx = index_of(&state, "file.txt");
        state.rename_entry(idx, "file.txt");

        assert_eq!(fs::read_to_string(root.join("file.txt")).unwrap(), "data");
        assert!(
            !state.status_message.contains("failed"),
            "a no-op rename is not an error: {}",
            state.status_message
        );
    }

    /// A genuine rename still has to work.
    #[test]
    fn an_ordinary_rename_still_renames() {
        let root_scratch = temp_dir("rename_ok");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("before.txt"), "data");

        let mut state = state_at(&root);
        let idx = index_of(&state, "before.txt");
        state.rename_entry(idx, "after.txt");

        assert!(!root.join("before.txt").exists());
        assert_eq!(fs::read_to_string(root.join("after.txt")).unwrap(), "data");
    }

    // ------------------------------------------------------------------
    // Detail view / columns
    // ------------------------------------------------------------------

    /// Every `Text` command in a render tree, in emission order.
    fn texts(tree: &RenderTree) -> Vec<String> {
        tree.commands
            .iter()
            .filter_map(|c| match c {
                guitk::render::RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn details_tree(state: &ExplorerState) -> RenderTree {
        let mut tree = RenderTree::new();
        let mut zones = DropZoneManager::new(state.current_path.clone());
        state.render_details(&mut tree, &mut zones, 0.0, 0.0, 600.0, 400.0);
        tree
    }

    #[test]
    fn the_detail_header_names_the_active_columns() {
        let root_scratch = temp_dir("cols_header");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "x");

        let state = state_at(&root);
        let drawn = texts(&details_tree(&state));

        for label in ["Name", "Size", "Date Modified", "Type"] {
            assert!(
                drawn.iter().any(|t| t == label),
                "the header should name every active column; {label:?} missing from {drawn:?}"
            );
        }
    }

    /// The regression this whole wiring risked: routing the detail view
    /// through the column system must not lose the two columns it already
    /// showed. `StandardColumns` used to return `Empty` for both.
    #[test]
    fn a_row_still_shows_the_size_it_showed_before() {
        let root_scratch = temp_dir("cols_size");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "0123456789");

        let state = state_at(&root);
        let drawn = texts(&details_tree(&state));

        let expected = format_size(10);
        assert!(
            drawn.contains(&expected),
            "the Size cell should read {expected:?}, got {drawn:?}"
        );
        assert!(
            drawn.iter().any(|t| t == "a.txt"),
            "the Name cell should read the entry's name, got {drawn:?}"
        );
    }

    /// A directory has no meaningful byte count, so its Size cell stays blank
    /// — which is what the hand-written view did, and what every file manager
    /// does.
    #[test]
    fn a_folder_row_leaves_the_size_cell_blank() {
        let root_scratch = temp_dir("cols_dir_size");
        let root = root_scratch.dir().to_path_buf();
        fs::create_dir_all(root.join("sub")).unwrap();

        let mut state = state_at(&root);
        let entry = state
            .entries
            .iter()
            .find(|e| e.name == "sub")
            .expect("the subdirectory should be listed")
            .clone();
        state.columns.set_columns(vec![ColumnId::SIZE]);

        assert_eq!(state.row_values(&entry), vec![ColumnValue::Empty]);
    }

    /// The row's cells come from the `FileEntry`, so a name that is not valid
    /// UTF-8 must still produce a row rather than being dropped or mangled.
    #[test]
    fn a_row_is_one_cell_per_active_column() {
        let root_scratch = temp_dir("cols_row_len");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("a.rs"), "fn main() {}");

        let mut state = state_at(&root);
        let entry = state.entries[0].clone();

        let n = state.columns.active_columns().len();
        assert_eq!(state.row_values(&entry).len(), n);

        state
            .columns
            .set_columns(vec![ColumnId::NAME, ColumnId::TYPE]);
        assert_eq!(
            state.row_values(&entry),
            vec![
                ColumnValue::Text("a.rs".to_string()),
                ColumnValue::Text("Source File".to_string()),
            ]
        );
    }

    /// A directory of source gains the code columns without the user asking.
    #[test]
    fn a_folder_of_source_grows_the_code_columns() {
        let root_scratch = temp_dir("cols_detect");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("main.rs"), "fn main() {}");

        let state = state_at(&root);
        assert!(
            state.columns.is_visible(ColumnId::LANGUAGE),
            "a .rs file should switch on the Language column: {:?}",
            state.columns.active_columns()
        );
    }

    /// The explorer owns the sort; the header arrow must follow it rather than
    /// keep a state of its own.
    #[test]
    fn the_header_arrow_follows_the_list_sort() {
        let root_scratch = temp_dir("cols_sort");
        let root = root_scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "x");

        let mut state = state_at(&root);
        assert_eq!(
            state.columns.current_sort(),
            (Some(ColumnId::NAME), SortOrder::Ascending),
            "a fresh explorer sorts by name ascending"
        );

        state.set_sort(SortBy::Size);
        assert_eq!(
            state.columns.current_sort(),
            (Some(ColumnId::SIZE), SortOrder::Ascending)
        );

        // Same column again: the explorer flips direction, and so must the
        // arrow. The manager's own three-state toggle would have gone to
        // `None` here, which is why the explorer sets rather than toggles.
        state.set_sort(SortBy::Size);
        assert_eq!(
            state.columns.current_sort(),
            (Some(ColumnId::SIZE), SortOrder::Descending)
        );
        assert_eq!(state.sort_dir, SortDir::Descending);
    }

    // ------------------------------------------------------------------
    // Icon and list views
    // ------------------------------------------------------------------

    /// Every `image_id` the tree asks the compositor to draw, in emission
    /// order.
    fn image_ids(tree: &RenderTree) -> Vec<u64> {
        tree.commands
            .iter()
            .filter_map(|c| match c {
                guitk::render::RenderCommand::Image { image_id, .. } => Some(*image_id),
                _ => None,
            })
            .collect()
    }

    /// The `y` of the first `Text` command drawing exactly `name`.
    fn text_y(tree: &RenderTree, name: &str) -> Option<f32> {
        tree.commands.iter().find_map(|c| match c {
            guitk::render::RenderCommand::Text { y, text, .. } if text == name => Some(*y),
            _ => None,
        })
    }

    /// Drive the whole pipeline to its resting state: generate every queued
    /// thumbnail, then acknowledge every upload as the host would.
    fn settle_thumbnails(state: &mut ExplorerState) {
        while state.pump_thumbnails_default() > 0 {}
        for (id, _) in state.take_pending_uploads() {
            state.mark_uploaded(id);
        }
    }

    fn dir_of(label: &str, names: &[&str]) -> ScratchDir {
        let scratch = temp_dir(label);
        for name in names {
            write(&scratch.dir().join(name), "content of a test file");
        }
        scratch
    }

    /// The icon view used to render an empty pane, because `render_file_list`
    /// only ever drew the detail case. Whatever else the three views disagree
    /// about, all three must put the file's name on the screen.
    #[test]
    fn every_view_mode_draws_the_files_name() {
        let scratch = dir_of("view_all_modes", &["alpha.txt", "beta.txt"]);
        let mut state = state_at(scratch.dir());

        for mode in [ViewMode::Details, ViewMode::Icons, ViewMode::List] {
            state.set_view_mode(mode);
            let drawn = texts(&state.render());
            assert!(
                drawn.iter().any(|s| s == "alpha.txt"),
                "{mode:?} drew no name for alpha.txt: {drawn:?}"
            );
        }
    }

    /// The gate that keeps the icon view honest. A thumbnail passes through
    /// three states — queued, generated, uploaded — and only the last one may
    /// produce an `Image` command, because the compositor discards a command
    /// naming an id it does not hold and says nothing about it. Emitting early
    /// would draw an empty white frame with a border and no way to tell why.
    #[test]
    fn a_thumbnail_is_not_drawn_until_the_compositor_has_its_pixels() {
        let scratch = dir_of("view_upload_gate", &["a.txt", "b.txt", "c.txt"]);
        let mut state = state_at(scratch.dir());
        state.set_view_mode(ViewMode::Icons);

        assert!(
            image_ids(&state.render()).is_empty(),
            "queued but not generated: nothing to draw yet"
        );

        while state.pump_thumbnails_default() > 0 {}
        assert!(
            image_ids(&state.render()).is_empty(),
            "generated is not drawable; the compositor has no pixels yet"
        );

        let uploads = state.take_pending_uploads();
        assert_eq!(uploads.len(), 3, "one upload per entry");
        assert!(
            image_ids(&state.render()).is_empty(),
            "taking an upload is a promise to register it, not proof that it \
             worked; a failed registration must keep the placeholder"
        );

        for (id, _) in &uploads {
            state.mark_uploaded(*id);
        }
        assert_eq!(
            image_ids(&state.render()).len(),
            3,
            "acknowledged uploads finally draw"
        );
    }

    /// The reverse edge. A host that reclaims memory by unregistering an image
    /// must get the placeholder back, not an empty frame.
    #[test]
    fn dropping_an_image_returns_the_entry_to_its_placeholder() {
        let scratch = dir_of("view_drop", &["a.txt"]);
        let mut state = state_at(scratch.dir());
        state.set_view_mode(ViewMode::Icons);
        settle_thumbnails(&mut state);

        let ids = image_ids(&state.render());
        assert_eq!(ids.len(), 1);

        assert!(state.mark_dropped(ids[0]), "the id was held");
        assert!(!state.mark_dropped(ids[0]), "and is not held twice");
        assert!(
            image_ids(&state.render()).is_empty(),
            "an unregistered image must fall back to its placeholder"
        );
        assert_eq!(state.uploaded_count(), 0);
    }

    /// The list view draws smaller pictures in a different layout, but it must
    /// not have its own opinion about when a picture is safe to emit —
    /// `push_thumb` is the single decision point precisely so the two cannot
    /// drift apart.
    #[test]
    fn the_list_view_gates_its_thumbnails_the_same_way() {
        let scratch = dir_of("view_list_gate", &["a.txt", "b.txt"]);
        let mut state = state_at(scratch.dir());
        state.set_view_mode(ViewMode::List);

        while state.pump_thumbnails_default() > 0 {}
        assert!(image_ids(&state.render()).is_empty());

        settle_thumbnails(&mut state);
        assert_eq!(image_ids(&state.render()).len(), 2);
    }

    /// The detail view draws a glyph per row and never a picture, so decoding
    /// every file in the folder for it would be pure waste. Queueing follows
    /// the view, both ways.
    #[test]
    fn the_detail_view_queues_no_thumbnail_work() {
        let scratch = dir_of("view_details_idle", &["a.txt", "b.txt"]);
        let mut state = state_at(scratch.dir());

        assert_eq!(state.view_mode, ViewMode::Details);
        assert_eq!(state.thumb_gen.pending_count(), 0);
        assert_eq!(state.pump_thumbnails_default(), 0);

        state.set_view_mode(ViewMode::Icons);
        assert_eq!(
            state.thumb_gen.pending_count(),
            2,
            "switching into a picture view fills the queue"
        );

        state.set_view_mode(ViewMode::Details);
        assert_eq!(
            state.thumb_gen.pending_count(),
            0,
            "switching out of one empties it"
        );
    }

    /// Every outstanding request after a directory change points at a file the
    /// user has navigated away from. Generating them would delay the ones now
    /// on screen behind work whose results go straight into the eviction path.
    #[test]
    fn changing_directory_cancels_the_old_directorys_thumbnails() {
        let scratch = dir_of("view_nav_cancel", &["a.txt", "b.txt", "c.txt"]);
        let root = scratch.dir().to_path_buf();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).expect("sub");
        write(&sub.join("only.txt"), "x");

        let mut state = state_at(&root);
        state.set_view_mode(ViewMode::Icons);
        assert_eq!(
            state.thumb_gen.pending_count(),
            4,
            "three files and one directory"
        );

        state.navigate_to(&sub);
        assert_eq!(
            state.thumb_gen.pending_count(),
            1,
            "only the new directory's single entry is queued"
        );
    }

    /// A cache hit is a hit on *this* version of the file — the key carries
    /// mtime and size — so re-queueing one is work with a known answer.
    #[test]
    fn a_cached_thumbnail_is_not_queued_again() {
        let scratch = dir_of("view_no_requeue", &["a.txt", "b.txt"]);
        let mut state = state_at(scratch.dir());
        state.set_view_mode(ViewMode::Icons);
        settle_thumbnails(&mut state);

        assert_eq!(state.thumbs.len(), 2);
        state.queue_thumbnails();
        assert_eq!(
            state.thumb_gen.pending_count(),
            0,
            "both entries are already cached"
        );

        // An edit changes size and mtime, so the key misses and the entry is
        // queued again without anyone having to invalidate anything.
        write(
            &scratch.dir().join("a.txt"),
            "a much longer body than before",
        );
        state.load_directory();
        assert_eq!(state.thumb_gen.pending_count(), 1);
    }

    /// The grid wraps at the pane width, and a pane too narrow for even one
    /// cell must clip a single column rather than divide by zero.
    #[test]
    fn the_icon_grid_wraps_and_survives_a_pane_narrower_than_a_cell() {
        let scratch = dir_of("view_grid_wrap", &["a.txt", "b.txt", "c.txt"]);
        let mut state = state_at(scratch.dir());
        state.set_view_mode(ViewMode::Icons);

        // Sidebar plus two cells' worth of pane: exactly two columns.
        state.window_width = state.sidebar_width as u32 + (ICON_CELL_W as u32) * 2;
        let tree = state.render();
        let (ya, yb, yc) = (
            text_y(&tree, "a.txt").expect("a"),
            text_y(&tree, "b.txt").expect("b"),
            text_y(&tree, "c.txt").expect("c"),
        );
        assert!((ya - yb).abs() < f32::EPSILON, "a and b share a row");
        assert!(yc > yb, "c wrapped onto the next row: {yc} vs {yb}");
        assert!(
            (yc - ya - ICON_CELL_H).abs() < 0.001,
            "exactly one cell height down: {yc} - {ya}"
        );

        // Narrower than one cell. One column, clipped — not zero columns, and
        // not a panic.
        state.window_width = state.sidebar_width as u32 + 10;
        let tree = state.render();
        assert!(
            text_y(&tree, "a.txt").is_some(),
            "a pane too narrow for a cell still draws the first one"
        );
    }

    // ======================================================================
    // Drag and drop
    //
    // A drop zone is a claim about where something is on screen, so every
    // test here goes through `render` to make the claim rather than calling
    // `register_file_row` with coordinates of its own. A test that registered
    // its own rectangles would keep passing after the layout it is describing
    // had moved out from under it — which is the failure that let this whole
    // module sit unreachable for as long as it did.
    // ======================================================================

    /// Middle of the row `name` occupies in the current view, in window
    /// coordinates — the point a user would actually be over.
    fn row_center(state: &ExplorerState, name: &str) -> (f32, f32) {
        let index = state
            .entries
            .iter()
            .position(|e| e.name == name)
            .unwrap_or_else(|| panic!("no entry named {name}"));
        let x = state.sidebar_width;
        let y = 64.0;
        let w = state.window_width as f32 - state.sidebar_width;
        match state.view_mode {
            ViewMode::Details => (
                x + w / 2.0,
                y + HEADER_H + index as f32 * ROW_H + ROW_H / 2.0,
            ),
            ViewMode::List => (
                x + w / 2.0,
                y + index as f32 * LIST_ROW_H + LIST_ROW_H / 2.0,
            ),
            ViewMode::Icons => {
                let cols = ((w / ICON_CELL_W) as usize).max(1);
                (
                    x + index.checked_rem(cols).unwrap_or(0) as f32 * ICON_CELL_W
                        + ICON_CELL_W / 2.0,
                    y + index.checked_div(cols).unwrap_or(0) as f32 * ICON_CELL_H
                        + ICON_CELL_H / 2.0,
                )
            }
        }
    }

    /// A point inside the file pane that is below every row drawn.
    fn empty_space(state: &ExplorerState) -> (f32, f32) {
        let x = state.sidebar_width;
        let w = state.window_width as f32 - state.sidebar_width;
        let bottom = state.window_height as f32 - 24.0;
        (x + w / 2.0, bottom - 4.0)
    }

    #[test]
    fn every_view_mode_registers_the_folder_row_it_draws() {
        let scratch = temp_dir("dz_modes");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("target")).unwrap();
        write(&root.join("a.txt"), "x");

        for mode in [ViewMode::Details, ViewMode::List, ViewMode::Icons] {
            let mut state = state_at(&root);
            state.set_view_mode(mode);
            let _ = state.render();

            let (fx, fy) = row_center(&state, "target");
            match state.dropzone.find_zone(fx, fy) {
                DropZone::Folder { path, rect } => {
                    assert_eq!(path, root.join("target"), "{mode:?}");
                    // The rectangle is in window coordinates, not the
                    // pane-local ones the row was drawn in: a hit test is fed
                    // a pointer position, which is a window position.
                    assert!(
                        rect.contains(fx, fy),
                        "{mode:?}: the registered rect covers the point it was found by"
                    );
                    assert!(
                        rect.x >= state.sidebar_width,
                        "{mode:?}: and starts at the pane, not at the window origin"
                    );
                }
                other => panic!("{mode:?}: expected the folder row, got {other:?}"),
            }

            // A *file* row is not a target — you cannot drop into a file — so
            // it falls through to the directory being shown.
            let (ax, ay) = row_center(&state, "a.txt");
            assert_eq!(
                state.dropzone.find_zone(ax, ay),
                DropZone::CurrentDirectory,
                "{mode:?}: a file row falls through to the current directory"
            );
        }
    }

    #[test]
    fn empty_space_below_the_rows_is_the_directory_being_shown() {
        let scratch = temp_dir("dz_empty");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "x");

        let mut state = state_at(&root);
        let _ = state.render();

        let (x, y) = empty_space(&state);
        assert_eq!(state.dropzone.find_zone(x, y), DropZone::CurrentDirectory);

        // Outside the window entirely is no zone at all, not a silent fallback
        // to the current directory: a drag released over the taskbar has not
        // been aimed at the explorer.
        assert_eq!(state.dropzone.find_zone(-5.0, -5.0), DropZone::None);
    }

    #[test]
    fn a_sidebar_row_targets_the_path_it_names_not_the_label_it_draws() {
        let scratch = temp_dir("dz_sidebar");
        let root = scratch.dir().to_path_buf();

        let mut state = state_at(&root);
        let _ = state.render();

        // The first quick-access row is labelled "/ (Root)" and stands for "/".
        let y = 64.0 + 8.0 + SIDEBAR_ROW_H / 2.0;
        match state.dropzone.find_zone(state.sidebar_width / 2.0, y) {
            DropZone::Sidebar { path, .. } => assert_eq!(path, PathBuf::from("/")),
            other => panic!("expected the root sidebar row, got {other:?}"),
        }
    }

    #[test]
    fn a_frames_zones_do_not_outlive_the_frame() {
        let scratch = temp_dir("dz_stale");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("gone")).unwrap();
        fs::create_dir(root.join("empty")).unwrap();

        let mut state = state_at(&root);
        let _ = state.render();
        let (x, y) = row_center(&state, "gone");
        assert!(matches!(
            state.dropzone.find_zone(x, y),
            DropZone::Folder { .. }
        ));

        // Navigate into the empty directory and draw again. The old rows are
        // not on screen any more, so a drop where one used to be must land in
        // the new directory rather than in a folder that is no longer visible.
        state.navigate_to(&root.join("empty"));
        let _ = state.render();
        assert_eq!(
            state.dropzone.find_zone(x, y),
            DropZone::CurrentDirectory,
            "the previous frame's folder rows are gone"
        );
        assert_eq!(state.dropzone.current_dir(), root.join("empty"));
    }

    #[test]
    fn dropping_a_file_on_a_folder_row_moves_it_in() {
        let scratch = temp_dir("dz_move");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("target")).unwrap();
        write(&root.join("note.txt"), "hello");

        let mut state = state_at(&root);
        let _ = state.render();

        let (x, y) = row_center(&state, "target");
        state.drag_enter(vec![root.join("note.txt")]);
        state.drag_over(x, y, DragModifiers::default());
        let result = state.drop_at(x, y, DragModifiers::default()).expect("drop");

        assert!(result.valid, "{:?}", result.invalid_reason);
        assert_eq!(result.operation, DropOperation::Move);
        assert!(root.join("target/note.txt").exists(), "arrived");
        assert!(!root.join("note.txt").exists(), "and left");

        // The drop went through the shared executor, so it is undoable — a
        // drag that moved a file irreversibly would be the one operation in
        // the explorer that could not be taken back.
        assert!(!state.undo.is_empty(), "the drop is on the undo stack");

        // And the listing was reloaded, so the moved file is no longer shown.
        assert!(!state.entries.iter().any(|e| e.name == "note.txt"));
    }

    #[test]
    fn a_drop_on_empty_space_lands_in_the_directory_being_shown() {
        let scratch = temp_dir("dz_here");
        let root = scratch.dir().to_path_buf();
        let outside = scratch.dir().join("outside");
        fs::create_dir(&outside).unwrap();
        write(&outside.join("note.txt"), "hello");
        fs::create_dir(root.join("here")).unwrap();

        let mut state = state_at(&root.join("here"));
        let _ = state.render();

        let (x, y) = empty_space(&state);
        state.drag_enter(vec![outside.join("note.txt")]);
        state.drag_over(x, y, DragModifiers::default());
        let result = state.drop_at(x, y, DragModifiers::default()).expect("drop");

        assert!(result.valid, "{:?}", result.invalid_reason);
        assert_eq!(result.target_dir, root.join("here"));
        assert!(root.join("here/note.txt").exists());
    }

    #[test]
    fn a_folder_cannot_be_dropped_into_itself() {
        let scratch = temp_dir("dz_nested");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("outer")).unwrap();

        let mut state = state_at(&root);
        let _ = state.render();

        let (x, y) = row_center(&state, "outer");
        state.drag_enter(vec![root.join("outer")]);
        state.drag_over(x, y, DragModifiers::default());
        let result = state.drop_at(x, y, DragModifiers::default()).expect("drop");

        assert!(!result.valid);
        assert!(
            result.invalid_reason.unwrap_or_default().contains("itself"),
            "the refusal says why"
        );
        assert!(root.join("outer").is_dir(), "and nothing happened to it");
    }

    /// The whole reason [`ExplorerState::evaluate_drop`] exists rather than the
    /// manager's verdict being used unchanged.
    ///
    /// The executor's conflict policy is `Rename`, so a move of `note.txt` into
    /// the directory it is already in would not be the no-op the user meant —
    /// it would conjure `note (2).txt` out of a drag that went nowhere.
    #[test]
    fn moving_a_file_into_the_folder_it_is_already_in_does_nothing() {
        let scratch = temp_dir("dz_selfmove");
        let root = scratch.dir().to_path_buf();
        write(&root.join("note.txt"), "hello");

        let mut state = state_at(&root);
        let _ = state.render();

        let (x, y) = empty_space(&state);
        state.drag_enter(vec![root.join("note.txt")]);
        state.drag_over(x, y, DragModifiers::default());
        let result = state.drop_at(x, y, DragModifiers::default()).expect("drop");

        assert!(!result.valid);
        assert_eq!(
            result.invalid_reason.as_deref(),
            Some("Already in this folder")
        );
        assert_eq!(
            fs::read_dir(&root)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().starts_with("note"))
                .count(),
            1,
            "no duplicate was conjured"
        );
    }

    /// The counterpart to the above: duplicating a file inside its own folder
    /// is a thing people do on purpose, and Ctrl says so explicitly.
    #[test]
    fn copying_a_file_into_its_own_folder_still_duplicates_it() {
        let scratch = temp_dir("dz_selfcopy");
        let root = scratch.dir().to_path_buf();
        write(&root.join("note.txt"), "hello");

        let mut state = state_at(&root);
        let _ = state.render();

        let ctrl = DragModifiers {
            ctrl: true,
            ..DragModifiers::default()
        };
        let (x, y) = empty_space(&state);
        state.drag_enter(vec![root.join("note.txt")]);
        state.drag_over(x, y, ctrl);
        let result = state.drop_at(x, y, ctrl).expect("drop");

        assert!(result.valid, "{:?}", result.invalid_reason);
        assert_eq!(result.operation, DropOperation::Copy);
        assert_eq!(
            fs::read_dir(&root)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().starts_with("note"))
                .count(),
            2,
            "the copy was renamed rather than overwriting the original"
        );
    }

    /// An Alt-drag makes a link, or says it could not -- never a copy.
    ///
    /// Both outcomes are accepted because only one of them is available on a
    /// given machine: Windows needs a privilege to create a symbolic link, so
    /// a host without it gets a per-file failure, which is the behaviour
    /// `ErrorPolicy::SkipAndContinue` was chosen for. What is asserted in
    /// *both* cases is the thing that must never happen -- a second
    /// independent file. Silently copying would give the user a duplicate that
    /// drifts out of step with the original with no sign it was ever meant to
    /// be a stand-in, which is worse than the gesture failing.
    #[test]
    fn an_alt_drag_makes_a_link_or_reports_that_it_could_not() {
        let scratch = temp_dir("dz_link");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("target")).unwrap();
        write(&root.join("note.txt"), "hello");

        let mut state = state_at(&root);
        let _ = state.render();

        let alt = DragModifiers {
            alt: true,
            ..DragModifiers::default()
        };
        let (x, y) = row_center(&state, "target");
        state.drag_enter(vec![root.join("note.txt")]);
        state.drag_over(x, y, alt);

        let drag = state.drag().expect("a drag is in flight");
        assert!(drag.is_valid(), "an Alt-drag is a supported gesture now");

        let result = state.drop_at(x, y, alt).expect("drop");
        assert!(result.valid);

        let made = root.join("target/note.txt");
        match fs::symlink_metadata(&made) {
            Ok(meta) => {
                assert!(
                    meta.file_type().is_symlink(),
                    "an Alt-drag produced a real file, not a link -- the exact \
                     silent downgrade this gesture must never do"
                );
                assert_eq!(
                    fs::read_to_string(&made).expect("the link resolves"),
                    "hello"
                );
            }
            Err(_) => {
                assert!(
                    state.status_message.contains("failed")
                        || state.status_message.contains("Linked 0"),
                    "no link was made and nothing said so: {:?}",
                    state.status_message
                );
            }
        }
    }

    #[test]
    fn no_drop_feedback_is_drawn_until_the_pointer_has_moved() {
        let scratch = temp_dir("dz_feedback");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("target")).unwrap();

        let mut state = state_at(&root);
        let baseline = state.render().commands.len();

        // A drag that has entered the window but not yet reported a position
        // has no target, so it must not highlight one.
        state.drag_enter(vec![PathBuf::from("/elsewhere/note.txt")]);
        assert_eq!(
            state.render().commands.len(),
            baseline,
            "nothing is highlighted before the first movement"
        );

        let (x, y) = row_center(&state, "target");
        state.drag_over(x, y, DragModifiers::default());
        assert!(
            state.render().commands.len() > baseline,
            "hovering a folder row draws the highlight and the label"
        );

        state.drag_cancel();
        assert_eq!(
            state.render().commands.len(),
            baseline,
            "cancelling takes the highlight away again"
        );
        assert_eq!(state.dropzone.current_hover(), &DropZone::None);
    }

    /// The hover survives the frame that redraws it.
    ///
    /// `render` takes the manager out of `self` and puts it back rather than
    /// building a fresh one, precisely so that this holds: a new manager per
    /// frame would reset `current_hover`, and every frame of a stationary hover
    /// would re-fire `DragEnter` for the zone the pointer had not left.
    #[test]
    fn a_stationary_hover_does_not_re_enter_its_zone_every_frame() {
        let scratch = temp_dir("dz_hover");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("target")).unwrap();

        let mut state = state_at(&root);
        let _ = state.render();

        let (x, y) = row_center(&state, "target");
        state.drag_enter(vec![PathBuf::from("/elsewhere/note.txt")]);
        assert!(matches!(
            state.drag_over(x, y, DragModifiers::default()),
            Some(DropZoneEvent::DragEnter { .. })
        ));

        let _ = state.render();
        assert!(
            matches!(
                state.drag_over(x, y, DragModifiers::default()),
                Some(DropZoneEvent::DragOver { .. })
            ),
            "still over the same zone after a redraw"
        );
    }

    #[test]
    fn a_drop_with_no_drag_in_flight_is_not_an_operation() {
        let scratch = temp_dir("dz_nodrag");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);
        let _ = state.render();

        let (x, y) = empty_space(&state);
        assert!(state.drop_at(x, y, DragModifiers::default()).is_none());
        assert!(state.drag_over(x, y, DragModifiers::default()).is_none());
    }

    // ------------------------------------------------------------------
    // The window: input, ticking, and getting pixels to the compositor
    // ------------------------------------------------------------------

    // The trait is named rather than imported anonymously because
    // `ExplorerState` has a `render` of its own taking no size, so every call
    // to the trait's has to say which one it means. Spelling it
    // `App::render(&mut state, w, h)` is also what a frame actually goes
    // through — it is the loop's call, not the internal one — so a test that
    // used the inherent method would skip the size handshake it is checking.
    use oswindow::app::{App, ImageChange};

    fn click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    fn double_click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::DoubleClick(MouseButton::Left),
        })
    }

    fn key(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        })
    }

    fn shift_key(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::shift(),
            text: String::new(),
        })
    }

    fn ctrl_key(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::ctrl(),
            text: String::new(),
        })
    }

    /// A click finds the row the *renderer* put under the pointer, not a row
    /// recomputed from the layout. That is the whole reason the drop-zone
    /// manager stores the index: the icon view's column count depends on the
    /// pane width, so a second copy of the arithmetic would eventually open a
    /// different file from the one clicked.
    #[test]
    fn a_click_selects_the_row_the_last_frame_drew_there() {
        let scratch = dir_of("win_click", &["alpha.txt", "beta.txt", "gamma.txt"]);
        let mut state = state_at(scratch.dir());

        for mode in [ViewMode::Details, ViewMode::List, ViewMode::Icons] {
            state.set_view_mode(mode);
            state.deselect_all();
            let _ = App::render(&mut state, 900.0, 600.0);

            let (x, y) = row_center(&state, "beta.txt");
            assert!(state.handle_event(&click(x, y)), "{mode:?}: click changed");
            let picked = state.selected_indices.first().copied();
            assert_eq!(
                picked.map(|i| state.entries[i].name.clone()),
                Some(String::from("beta.txt")),
                "{mode:?}: clicked beta.txt and got {picked:?}"
            );
        }
    }

    /// A click before any frame has been drawn hits nothing, because nothing
    /// has been registered yet. It must not select an arbitrary row, and above
    /// all must not panic — the compositor is free to deliver a click before
    /// the first frame goes out.
    #[test]
    fn a_click_before_the_first_frame_selects_nothing() {
        let scratch = dir_of("win_click_early", &["a.txt"]);
        let mut state = state_at(scratch.dir());
        assert!(!state.handle_event(&click(400.0, 300.0)));
        assert!(state.selected_indices.is_empty());
    }

    /// Double-clicking a folder enters it; double-clicking empty space does
    /// nothing rather than opening whatever happened to be selected.
    #[test]
    fn a_double_click_opens_the_folder_under_the_pointer_and_only_that() {
        let scratch = temp_dir("win_open");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("sub")).unwrap();

        let mut state = state_at(&root);
        let _ = App::render(&mut state, 900.0, 600.0);

        let (ex, ey) = empty_space(&state);
        assert!(!state.handle_event(&double_click(ex, ey)));
        assert_eq!(state.current_path, root);

        let (x, y) = row_center(&state, "sub");
        assert!(state.handle_event(&double_click(x, y)));
        assert_eq!(state.current_path, root.join("sub"));
    }

    /// Backspace goes up, Alt+Left goes back, Alt+Right goes forward — and
    /// each reports "nothing changed" when there is nowhere to go, so a held
    /// key at the top of the tree does not repaint the window forever.
    #[test]
    fn the_navigation_keys_move_and_say_so_only_when_they_moved() {
        let scratch = temp_dir("win_navkeys");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("sub")).unwrap();

        let mut state = state_at(&root);
        assert!(!state.handle_event(&Event::Key(KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: guitk::event::Modifiers::alt(),
            text: String::new(),
        })));

        state.navigate_to(&root.join("sub"));
        let back = Event::Key(KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: guitk::event::Modifiers::alt(),
            text: String::new(),
        });
        assert!(state.handle_event(&back));
        assert_eq!(state.current_path, root);

        let forward = Event::Key(KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: guitk::event::Modifiers::alt(),
            text: String::new(),
        });
        assert!(state.handle_event(&forward));
        assert_eq!(state.current_path, root.join("sub"));

        assert!(state.handle_event(&key(Key::Backspace)));
        assert_eq!(state.current_path, root);
    }

    /// Arrow keys walk the listing and stop at both ends. Clamping rather than
    /// wrapping matters twice over: at row zero a `usize` decrement would wrap
    /// to the *end* of the listing, and a held key that cycled would never
    /// settle.
    #[test]
    fn the_arrow_keys_walk_the_listing_and_stop_at_both_ends() {
        let scratch = dir_of("win_arrows", &["a.txt", "b.txt", "c.txt"]);
        let mut state = state_at(scratch.dir());
        let last = state.entries.len() - 1;

        // Nothing selected: Down selects the first row rather than moving from
        // an imaginary position.
        assert!(state.handle_event(&key(Key::Down)));
        assert_eq!(state.selected_indices, vec![0]);

        assert!(state.handle_event(&key(Key::Down)));
        assert_eq!(state.selected_indices, vec![1]);
        assert!(state.handle_event(&key(Key::Up)));
        assert_eq!(state.selected_indices, vec![0]);

        // At the top, Up stays put — a `usize` decrement here would wrap to the
        // *end* of the listing — and says nothing changed.
        assert!(
            !state.handle_event(&key(Key::Up)),
            "a key that moved nothing must not ask for a repaint"
        );
        assert_eq!(state.selected_indices, vec![0], "clamped at the top");

        assert!(state.handle_event(&key(Key::End)));
        assert_eq!(state.selected_indices, vec![last]);
        assert!(
            !state.handle_event(&key(Key::Down)),
            "clamped at the bottom"
        );

        assert!(state.handle_event(&key(Key::Home)));
        assert_eq!(state.selected_indices, vec![0]);
    }

    /// Ctrl+A selects everything and Escape clears it, both reporting honestly
    /// when there was nothing to do.
    #[test]
    fn select_all_and_escape_are_reported_only_when_they_change_something() {
        let scratch = dir_of("win_selall", &["a.txt", "b.txt"]);
        let mut state = state_at(scratch.dir());

        assert!(!state.handle_event(&key(Key::Escape)), "nothing selected");
        assert!(state.handle_event(&ctrl_key(Key::A)));
        assert_eq!(state.selected_indices.len(), state.entries.len());
        assert!(state.handle_event(&key(Key::Escape)));
        assert!(state.selected_indices.is_empty());
    }

    /// Key *releases* do nothing. Acting on both edges would move the
    /// selection two rows per press.
    #[test]
    fn a_key_release_is_not_a_second_key_press() {
        let scratch = dir_of("win_release", &["a.txt", "b.txt", "c.txt"]);
        let mut state = state_at(scratch.dir());
        state.select_single(0);

        let release = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: false,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        });
        assert!(!state.handle_event(&release));
        assert_eq!(state.selected_indices, vec![0]);
    }

    /// The clock is armed by there being work and disarmed by there being
    /// none. Without the first, a folder of pictures entered by keyboard never
    /// generates anything; without the second, a file manager left open holds
    /// the whole desktop awake for the rest of the session.
    #[test]
    fn the_clock_runs_only_while_there_are_thumbnails_left_to_make() {
        let scratch = dir_of("win_tick", &["a.txt", "b.txt"]);
        let mut state = state_at(scratch.dir());

        assert_eq!(
            state.tick_interval(),
            None,
            "the detail view queues no work, so there is nothing to wake for"
        );

        state.set_view_mode(ViewMode::Icons);
        assert!(
            state.tick_interval().is_some(),
            "work queued: arm the clock"
        );

        // Ticks retire the queue, and the last of them disarms it.
        let mut ticks = 0;
        while state.tick_interval().is_some() {
            let _ = state.handle_event(&Event::Tick { elapsed_ms: 60 });
            ticks += 1;
            assert!(ticks < 100, "the queue is not draining");
        }
        assert_eq!(state.thumb_gen.pending_count(), 0);
        assert!(!state.take_pending_uploads().is_empty(), "work was done");
    }

    /// A tick that retires nothing must not ask for a repaint, or the window
    /// redraws itself at the tick rate for as long as it is open.
    #[test]
    fn an_idle_tick_does_not_ask_for_a_repaint() {
        let scratch = dir_of("win_idle_tick", &["a.txt"]);
        let mut state = state_at(scratch.dir());
        assert!(!state.handle_event(&Event::Tick { elapsed_ms: 60 }));
    }

    /// The first frame is drawn at the size the compositor gave, not the size
    /// the explorer asked for — there is no `Resize` before it.
    #[test]
    fn the_first_frame_is_laid_out_at_the_size_the_compositor_gave() {
        let scratch = dir_of("win_size", &["a.txt"]);
        let mut state = state_at(scratch.dir());
        let _ = App::render(&mut state, 1280.0, 720.0);
        assert_eq!((state.window_width, state.window_height), (1280, 720));

        // And a resize to the size already in force is not a repaint.
        assert!(!state.handle_event(&Event::Resize {
            width: 1280,
            height: 720
        }));
        assert!(state.handle_event(&Event::Resize {
            width: 800,
            height: 600
        }));
        assert_eq!((state.window_width, state.window_height), (800, 600));
    }

    /// The upload's bytes are in the compositor's byte order, which is the
    /// *reverse* of the one the thumbnail is stored in. Both are called ARGB;
    /// getting it wrong is neither a compile error nor a panic, it is every
    /// thumbnail arriving with red and blue exchanged and its alpha read out of
    /// the blue channel.
    #[test]
    fn an_upload_carries_wire_order_bytes_and_not_the_stored_ones() {
        let scratch = dir_of("win_wire", &["a.txt"]);
        let mut state = state_at(scratch.dir());
        state.set_view_mode(ViewMode::Icons);
        while state.pump_thumbnails_default() > 0 {}

        let stored = state
            .thumbs
            .peek(
                &state.entries[0].path,
                mtime_secs(state.entries[0].modified),
                state.entries[0].size,
            )
            .expect("generated")
            .clone();

        let changes = state.take_images();
        let ImageChange::Upload {
            width,
            height,
            stride,
            format,
            ref bytes,
            ..
        } = changes[0]
        else {
            panic!("the first change should be the upload");
        };
        assert_eq!((width, height), (stored.width, stored.height));
        assert_eq!(stride, stored.width * 4, "a canvas row is never padded");
        assert_eq!(format, oswindow::PixelFormat::Argb8888);
        assert_eq!(bytes.len(), stored.pixels.len());

        // The pixel a decoder would read back out of these bytes is the pixel
        // the thumbnail holds, channel for channel.
        let from_wire =
            guitk::canvas::Canvas::from_argb8888(width, height, bytes.as_slice()).expect("wire");
        let from_store =
            guitk::canvas::Canvas::from_argb(stored.width, stored.height, &stored.pixels)
                .expect("stored");
        assert_eq!(from_wire, from_store);

        // And they are genuinely different bytes: passing the stored buffer
        // through unconverted is the bug this guards.
        assert_ne!(
            bytes.as_slice(),
            stored.pixels.as_slice(),
            "an opaque grey thumbnail must not serialise identically in both \
             orders, or this test proves nothing"
        );
    }

    /// Uploading marks the id held, so the very next frame draws the picture.
    /// The optimism is safe because the event loop propagates a failed upload
    /// out of `apply_images` and ends, so there is no frame in which this could
    /// be believed and be wrong.
    #[test]
    fn taking_the_images_is_what_makes_the_next_frame_draw_them() {
        let scratch = dir_of("win_take_images", &["a.txt", "b.txt"]);
        let mut state = state_at(scratch.dir());
        state.set_view_mode(ViewMode::Icons);
        while state.pump_thumbnails_default() > 0 {}

        assert!(image_ids(&App::render(&mut state, 900.0, 600.0)).is_empty());
        assert_eq!(state.take_images().len(), 2);
        assert_eq!(image_ids(&App::render(&mut state, 900.0, 600.0)).len(), 2);

        // Draining: the same pictures are not offered twice.
        assert!(state.take_images().is_empty());
    }

    /// **Drops come before uploads, and the order is load-bearing.** The link's
    /// image budget is checked against `held - freed + incoming`, so a batch
    /// that filled a full cache — evicting as many as it added — would be
    /// refused if it asked the compositor to hold both sets at once.
    #[test]
    fn what_leaves_the_cache_is_given_back_before_what_enters_it_is_asked_for() {
        let scratch = dir_of("win_evict", &["a.txt", "b.txt", "c.txt"]);
        let mut state = state_at(scratch.dir());
        // A cache with room for one entry turns every generation into an
        // eviction, which is the case the ordering exists for.
        state.thumbs = ThumbnailCache::new(1);
        state.set_view_mode(ViewMode::Icons);

        while state.pump_thumbnails_default() > 0 {}
        let first = state.take_images();
        assert!(
            first
                .iter()
                .all(|c| matches!(c, ImageChange::Upload { .. })),
            "nothing was held yet, so nothing can be given back"
        );

        // Re-generate the same folder into the now-full one-entry cache.
        state.thumbs.clear();
        let dropped = state.take_images();
        assert!(
            !dropped.is_empty() && dropped.iter().all(|c| matches!(c, ImageChange::Drop(_))),
            "clearing the cache gives every picture back: {dropped:?}"
        );

        state.queue_thumbnails();
        while state.pump_thumbnails_default() > 0 {}
        let mixed = state.take_images();
        let first_upload = mixed
            .iter()
            .position(|c| matches!(c, ImageChange::Upload { .. }));
        let last_drop = mixed
            .iter()
            .rposition(|c| matches!(c, ImageChange::Drop(_)));
        if let (Some(up), Some(drop)) = (first_upload, last_drop) {
            assert!(drop < up, "every drop must precede every upload: {mixed:?}");
        }
    }

    /// A drop is only announced for an id believed to be held. Announcing one
    /// for an id the compositor never took is not free: the id is derived from
    /// the file, so a later generation of the same file re-uses it, and a
    /// stale drop would then take the *new* pixels down with it.
    #[test]
    fn an_eviction_of_something_never_uploaded_is_not_announced() {
        let scratch = dir_of("win_evict_unheld", &["a.txt"]);
        let mut state = state_at(scratch.dir());
        state.set_view_mode(ViewMode::Icons);
        while state.pump_thumbnails_default() > 0 {}

        // Discard the pending uploads without taking them through `take_images`,
        // so the cache holds a thumbnail the compositor never saw.
        let _ = state.take_pending_uploads();
        assert_eq!(state.uploaded_count(), 0);

        state.thumbs.clear();
        assert!(
            state.take_images().is_empty(),
            "nothing was held, so there is nothing to give back"
        );
    }

    /// The title leads with the folder, because a task bar elides its buttons
    /// from the right and every open window would otherwise read "Files".
    #[test]
    fn the_title_leads_with_the_folder_name() {
        let scratch = temp_dir("win_title");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("Pictures")).unwrap();

        let mut state = state_at(&root);
        state.navigate_to(&root.join("Pictures"));
        assert!(
            state.title().starts_with("Pictures"),
            "got {:?}",
            state.title()
        );
    }

    // ------------------------------------------------------------------
    // Editing keys
    //
    // Delete, F2 and Ctrl+Z were bound to nothing at all: pressing them was
    // silent, which is indistinguishable from a broken window. Every piece
    // they needed already existed and was tested -- `delete_selected`,
    // `rename_entry`, the undo journal, `AlertDialog`, `InputDialog` -- so
    // what these tests cover is the wiring, and above all that the
    // destructive ones ask first.
    // ------------------------------------------------------------------

    /// Deliver an event and drop the redraw flag.
    ///
    /// `handle_event` returns "something changed, repaint me", which is not
    /// what these tests are about -- they assert on the file on disk and on
    /// the dialog's own state. Discarding it once here beats a `let _ =` on
    /// every line, and keeps the flag `#[must_use]` where it matters.
    fn send(state: &mut ExplorerState, event: &Event) {
        let _ = state.handle_event(event);
    }

    /// Confirm a dialog by focusing its affirmative button and pressing it.
    ///
    /// Tab rather than Enter alone: `destructive_cancel` starts focus on
    /// Cancel on purpose, so Enter by itself is a refusal. That is the
    /// behaviour, and a test that reached past it would be testing a dialog
    /// this app does not use.
    fn confirm_modal(state: &mut ExplorerState) {
        send(state, &key(Key::Tab));
        send(state, &key(Key::Enter));
    }

    #[test]
    fn pressing_delete_asks_before_it_deletes() {
        let scratch = temp_dir("del_asks");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        assert!(
            state.handle_event(&key(Key::Delete)),
            "the key must be reported as handled, or the window will not repaint"
        );

        assert!(
            root.join("notes.txt").exists(),
            "Delete must ask first, not act and then ask"
        );
        assert!(state.modal.is_some(), "a confirmation must be up");
    }

    #[test]
    fn confirming_a_delete_recycles_the_file() {
        let scratch = temp_dir("del_confirm");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        send(&mut state, &key(Key::Delete));
        confirm_modal(&mut state);

        assert!(state.modal.is_none(), "the dialog must close once answered");
        assert!(!root.join("notes.txt").exists(), "confirmed, so it goes");
        assert_eq!(
            state.recycle.list().expect("bin").len(),
            1,
            "the plain Delete key recycles rather than erases"
        );
    }

    #[test]
    fn cancelling_a_delete_keeps_the_file() {
        let scratch = temp_dir("del_cancel");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        send(&mut state, &key(Key::Delete));
        send(&mut state, &key(Key::Escape));

        assert!(state.modal.is_none(), "Escape must close the dialog");
        assert!(
            root.join("notes.txt").exists(),
            "a cancelled delete must not delete"
        );
        assert_eq!(state.recycle.list().expect("bin").len(), 0);
    }

    /// Enter alone is a refusal, because focus starts on Cancel.
    ///
    /// This is what stops a user dismissing a surprise dialog with the key
    /// they were already pressing, and losing a file to it.
    #[test]
    fn enter_alone_on_a_delete_confirmation_cancels() {
        let scratch = temp_dir("del_enter");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        send(&mut state, &key(Key::Delete));
        send(&mut state, &key(Key::Enter));

        assert!(
            root.join("notes.txt").exists(),
            "Enter must land on Cancel, not on Delete"
        );
    }

    #[test]
    fn shift_delete_erases_instead_of_recycling_and_says_so() {
        let scratch = temp_dir("del_perm");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        send(&mut state, &shift_key(Key::Delete));

        let detail = match state.modal.as_ref() {
            Some(Modal::Confirm { dialog, action }) => {
                assert_eq!(*action, PendingAction::DeletePermanently);
                dialog.detail().unwrap_or_default().to_string()
            }
            _ => panic!("Shift+Delete must raise a confirmation"),
        };
        assert!(
            detail.contains("cannot be undone"),
            "the permanent one must say it is permanent, got {detail:?}"
        );

        confirm_modal(&mut state);
        assert!(!root.join("notes.txt").exists());
        assert_eq!(
            state.recycle.list().expect("bin").len(),
            0,
            "a permanent delete must not leave a recoverable copy in the bin"
        );
    }

    /// While a dialog is up, the listing underneath must not also act.
    ///
    /// Otherwise the keys used to reach the buttons drag the selection along
    /// with them, and confirming deletes a file other than the one named.
    #[test]
    fn a_modal_takes_the_keys_the_listing_would_have_used() {
        let scratch = temp_dir("modal_keys");
        let root = scratch.dir().to_path_buf();
        dir_with_files(&root, 5);

        let mut state = state_at(&root);
        state.move_selection_to(0);
        send(&mut state, &key(Key::Delete));
        let before = state.selected_indices.clone();

        send(&mut state, &key(Key::Down));
        send(&mut state, &key(Key::Down));

        assert_eq!(
            state.selected_indices, before,
            "the listing must not move under an open dialog"
        );
    }

    #[test]
    fn the_confirmation_is_actually_drawn() {
        let scratch = temp_dir("modal_draw");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        send(&mut state, &key(Key::Delete));

        let drawn = texts(&state.render());
        // The whole sentence, deliberately. Asserting only that "notes.txt"
        // was drawn somewhere passes with the dialog entirely absent, because
        // the listing row behind it draws the same name -- which is what this
        // test did until it was mutation-checked.
        //
        // Joined because the dialog word-wraps its message into one text
        // command per line, so the phrase is split across two of them. Any
        // wrap point rejoins with the space it broke on.
        let joined = drawn.join(" ");
        assert!(
            joined.contains("Move \"notes.txt\" to the recycle bin?"),
            "the dialog must name the file and where it is going, got {drawn:?}"
        );
        assert!(
            drawn.iter().any(|t| t == "Cancel"),
            "and draw the buttons, or there is nothing to click, got {drawn:?}"
        );
    }

    #[test]
    fn f2_opens_a_rename_box_holding_the_current_name() {
        let scratch = temp_dir("rename_open");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        state.move_selection_to(0);
        send(&mut state, &key(Key::F2));

        match state.modal.as_ref() {
            Some(Modal::Rename { dialog, target }) => {
                assert_eq!(dialog.input_text(), "notes.txt", "prefilled, not empty");
                assert_eq!(target, &root.join("notes.txt"));
            }
            _ => panic!("F2 must open a rename box"),
        }
    }

    #[test]
    fn renaming_through_the_box_renames_the_file() {
        let scratch = temp_dir("rename_do");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        state.move_selection_to(0);
        send(&mut state, &key(Key::F2));
        match state.modal.as_mut() {
            Some(Modal::Rename { dialog, .. }) => dialog.set_input_text("renamed.txt"),
            _ => panic!("no rename box"),
        }
        send(&mut state, &key(Key::Enter));

        assert!(state.modal.is_none());
        assert!(!root.join("notes.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("renamed.txt")).expect("renamed"),
            "keep me"
        );
    }

    /// The rename box holds a path, not a row number.
    ///
    /// A row index captured when the dialog opened names whatever has since
    /// moved into that row. Here the original is removed while the box is
    /// open, so an index-based rename would rename its neighbour instead.
    #[test]
    fn a_rename_whose_target_vanished_is_refused_not_misapplied() {
        let scratch = temp_dir("rename_stale");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "first");
        write(&root.join("b.txt"), "second");

        let mut state = state_at(&root);
        state.move_selection_to(0);
        send(&mut state, &key(Key::F2));

        fs::remove_file(root.join("a.txt")).expect("remove");
        state.load_directory();

        match state.modal.as_mut() {
            Some(Modal::Rename { dialog, .. }) => dialog.set_input_text("c.txt"),
            _ => panic!("no rename box"),
        }
        send(&mut state, &key(Key::Enter));

        assert!(
            root.join("b.txt").exists(),
            "the surviving file must keep its name"
        );
        assert!(
            !root.join("c.txt").exists(),
            "and nothing takes the new one"
        );
        assert!(
            state.status_message.contains("no longer there"),
            "and it must say why, got {:?}",
            state.status_message
        );
    }

    #[test]
    fn ctrl_z_after_a_delete_puts_the_file_back() {
        let scratch = temp_dir("undo_key");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        send(&mut state, &key(Key::Delete));
        confirm_modal(&mut state);
        assert!(!root.join("notes.txt").exists());

        send(&mut state, &ctrl_key(Key::Z));

        assert_eq!(
            fs::read_to_string(root.join("notes.txt")).expect("restored"),
            "keep me",
            "Ctrl+Z is the undo the confirmation promises"
        );
    }

    /// Undo must not claim to have restored a permanent delete.
    #[test]
    fn ctrl_z_after_a_permanent_delete_says_nothing_came_back() {
        let scratch = temp_dir("undo_key_perm");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        send(&mut state, &shift_key(Key::Delete));
        confirm_modal(&mut state);

        send(&mut state, &ctrl_key(Key::Z));

        assert!(
            state.status_message.contains("Nothing to undo"),
            "an erased file cannot come back, and undo must not pretend, got {:?}",
            state.status_message
        );
        assert!(!root.join("notes.txt").exists());
    }

    #[test]
    fn ctrl_c_then_ctrl_v_copies_within_the_window() {
        let scratch = temp_dir("clip_keys");
        let root = scratch.dir().to_path_buf();
        write(&root.join("notes.txt"), "keep me");
        fs::create_dir(root.join("sub")).expect("mkdir");

        let mut state = state_at(&root);
        select_named(&mut state, "notes.txt");
        send(&mut state, &ctrl_key(Key::C));
        assert!(state.clipboard.is_some(), "Ctrl+C must fill the clipboard");

        state.navigate_to(&root.join("sub"));
        send(&mut state, &ctrl_key(Key::V));

        assert_eq!(
            fs::read_to_string(root.join("sub/notes.txt")).expect("pasted"),
            "keep me"
        );
        assert!(
            root.join("notes.txt").exists(),
            "a copy leaves the original"
        );
    }

    // ------------------------------------------------------------------
    // Failures are shown, not filed
    //
    // Every operation used to hand back one formatted string that the caller
    // dropped into the status bar, so a half-failed delete and a clean one
    // arrived in the same place and both vanished at the next click. These
    // cover the split: the status line still gets everything, and a failure
    // additionally raises a dialog.
    // ------------------------------------------------------------------

    /// The text of the open notice, or `None` if there is no notice up.
    fn notice_text(state: &ExplorerState) -> Option<String> {
        match state.modal.as_ref() {
            Some(Modal::Notice { dialog }) => Some(dialog.message().to_string()),
            _ => None,
        }
    }

    #[test]
    fn a_rename_onto_an_existing_name_says_so_in_a_dialog() {
        let scratch = temp_dir("fail_rename_exists");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "first");
        write(&root.join("b.txt"), "second");

        let mut state = state_at(&root);
        select_named(&mut state, "a.txt");
        state.move_selection_to(0);
        send(&mut state, &key(Key::F2));
        match state.modal.as_mut() {
            Some(Modal::Rename { dialog, .. }) => dialog.set_input_text("b.txt"),
            _ => panic!("no rename box"),
        }
        send(&mut state, &key(Key::Enter));

        let notice = notice_text(&state).expect("a refused rename must raise a dialog");
        assert!(
            notice.contains("already exists"),
            "and say why, got {notice:?}"
        );
        assert_eq!(
            fs::read_to_string(root.join("b.txt")).expect("intact"),
            "second",
            "and must not have destroyed the file it refused to overwrite"
        );
    }

    #[test]
    fn a_rename_that_works_raises_no_dialog() {
        let scratch = temp_dir("fail_rename_ok");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "first");

        let mut state = state_at(&root);
        state.move_selection_to(0);
        send(&mut state, &key(Key::F2));
        match state.modal.as_mut() {
            Some(Modal::Rename { dialog, .. }) => dialog.set_input_text("b.txt"),
            _ => panic!("no rename box"),
        }
        send(&mut state, &key(Key::Enter));

        assert!(
            state.modal.is_none(),
            "success must not interrupt the user with a dialog"
        );
        assert!(root.join("b.txt").exists());
    }

    #[test]
    fn a_folder_that_cannot_be_created_says_so_in_a_dialog() {
        let scratch = temp_dir("fail_mkdir");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("taken")).expect("mkdir");

        let mut state = state_at(&root);
        state.create_folder("taken");

        let notice = notice_text(&state).expect("a refused mkdir must raise a dialog");
        assert!(
            notice.contains("taken"),
            "the dialog must name the folder, got {notice:?}"
        );
    }

    #[test]
    fn a_name_the_filesystem_forbids_is_refused_in_a_dialog() {
        let scratch = temp_dir("fail_badname");
        let root = scratch.dir().to_path_buf();

        let mut state = state_at(&root);
        state.create_folder("a/b");

        assert!(
            notice_text(&state).is_some(),
            "a name with a separator in it cannot be a single entry"
        );
        assert!(!root.join("a").exists(), "and nothing was created");
    }

    #[test]
    fn dismissing_a_notice_closes_it_and_does_nothing_else() {
        let scratch = temp_dir("fail_dismiss");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("taken")).expect("mkdir");

        let mut state = state_at(&root);
        state.create_folder("taken");
        assert!(state.modal.is_some(), "up first");

        send(&mut state, &key(Key::Enter));

        assert!(state.modal.is_none(), "OK closes it");
        assert!(
            root.join("taken").is_dir(),
            "and dismissing a report must not act on anything"
        );
    }

    #[test]
    fn the_notice_is_actually_drawn() {
        let scratch = temp_dir("fail_draw");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("taken")).expect("mkdir");

        let mut state = state_at(&root);
        state.create_folder("taken");

        let drawn = texts(&state.render()).join(" ");
        assert!(
            drawn.contains("Could not finish"),
            "the dialog must be on screen, got {drawn:?}"
        );
    }

    /// The status bar keeps its line even when a dialog is raised, so the
    /// record survives being dismissed.
    #[test]
    fn a_failure_reaches_both_the_dialog_and_the_status_bar() {
        let scratch = temp_dir("fail_both");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("taken")).expect("mkdir");

        let mut state = state_at(&root);
        state.create_folder("taken");

        assert!(notice_text(&state).is_some());
        assert!(
            state.status_message.contains("Error creating folder"),
            "got {:?}",
            state.status_message
        );
    }
}

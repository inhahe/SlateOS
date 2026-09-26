//! Slate OS File Explorer
//!
//! Graphical file manager with:
//! - Quick-access sidebar (five fixed places, each a drop target)
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

mod columnprefs;
mod columns;
mod drives;
mod dropzone;
mod fileops;
mod manualorder;
mod search;

use appearance::Palette;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::layout::Axis;
use guitk::listview::ListViewport;
use guitk::modal::{AlertDialog, DialogResult, InputDialog};
use guitk::render::RenderTree;
use guitk::scroll_window;
use guitk::scrollbar;
use guitk::splitter;
use guitk::theme::with_alpha;
use guitk::wheel::Accumulator as WheelAccumulator;

use columns::{ColumnId, ColumnManager, ColumnValue, SortOrder};
use drives::DriveSet;
use guitk::disabled::DisabledState;
use guitk::filetypes::{self, FileCategory};
use guitk::menu::{ContextMenu, MenuItem};
use guitk::pathbar::{CompletionItem, PathBar, PathBarEvent};
use std::process;

use dropzone::{
    DragModifiers, DropOperation, DropResult, DropZone, DropZoneEvent, DropZoneManager, Rect,
};
use fileops::{
    ConflictPolicy, ErrorPolicy, FileOpEvent, FileOperation, OperationExecutor, OperationPlan,
    OperationProgress, OperationSummary, RecycleBin, UndoStack, UndoTarget,
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
        if self.file_type == FileType::Directory {
            return FileType::Directory.label().to_string();
        }
        // The registry's own words for it: "Rust Source File" rather than "RS
        // File", "Portable Network Graphics" rather than "Image". The bucket
        // this file falls in is the coarse answer used for thumbnails; the
        // Type column is where the exact one belongs.
        let ext = self.path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let info = filetypes::detect_from_extension(ext);
        if info.category != FileCategory::Unknown {
            return info.description.to_string();
        }
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
    ///
    /// **Asked of `guitk::filetypes`, not matched here.** That registry's own
    /// module doc says "every GUI component that needs to display, open, or
    /// classify a file should go through this module rather than hard-coding
    /// extension lists", and what was here was a hard-coded extension list --
    /// one of *three* in this application, against a registry with magic-byte
    /// signatures and MIME types that nothing in the tree had ever read. See
    /// `TD-C-SIX-TOOLKIT-WIDGETS-ARE-WRITTEN-TESTED-AND-USED-BY-NOTHING`.
    ///
    /// This enum stays, because it is a *coarser* question than the registry
    /// answers: sixteen categories collapse to the nine buckets that pick a
    /// thumbnail and an icon. `is_text` is what separates a `.txt` from a
    /// `.pdf` -- both are `Document` to the registry, and only one of them is
    /// something a text thumbnail can be made of.
    pub fn from_extension(ext: &str) -> Self {
        let info = filetypes::detect_from_extension(ext);
        match info.category {
            FileCategory::Executable | FileCategory::Library | FileCategory::System => {
                Self::Executable
            }
            FileCategory::Package | FileCategory::Archive | FileCategory::DiskImage => {
                Self::Archive
            }
            FileCategory::Image => Self::Image,
            FileCategory::Audio => Self::Audio,
            FileCategory::Video => Self::Video,
            FileCategory::Code => Self::Code,
            FileCategory::Config | FileCategory::Data => {
                if info.is_text {
                    Self::Text
                } else {
                    Self::Unknown
                }
            }
            FileCategory::Document | FileCategory::Spreadsheet | FileCategory::Presentation => {
                if info.is_text {
                    Self::Text
                } else {
                    Self::Document
                }
            }
            FileCategory::Unknown => Self::Unknown,
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

/// Every key this program answers, and what it does.
///
/// Nineteen bindings and, until this list existed, no way to learn one but
/// reading the source. `Ctrl+L` is the worst of them: it is the only way to
/// type a path, and the address bar gives no sign that it can be typed into.
/// `Ctrl+H` is next -- a user who cannot see a file they know is there has no
/// way to find out that hidden files are a thing this program has an opinion
/// about.
///
/// **Each row is a key this program actually answers**, checked by
/// `every_advertised_key_does_something`, which reads each label with
/// `guitk::shortcut` and presses every key it names.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Arrows", "Move the selection"),
    ("Home / End", "First / last item"),
    ("Enter", "Open the selected item"),
    ("Backspace", "Go up one folder"),
    (
        "Alt+Left / Alt+Right",
        "Back / forward through where you have been",
    ),
    ("1 / 2 / 3", "Details / list / icons"),
    ("F5", "Read the folder again"),
    ("F2", "Rename"),
    ("Delete", "Move to the recycle bin"),
    ("Shift+Delete", "Delete for good, without the bin"),
    ("Ctrl+A", "Select everything"),
    ("Ctrl+C / Ctrl+X / Ctrl+V", "Copy / cut / paste"),
    ("Ctrl+Z", "Undo the last file operation"),
    ("Ctrl+F", "Search this folder"),
    ("Ctrl+H", "Show or hide hidden files"),
    ("Ctrl+L", "Type a path into the address bar"),
    ("Escape", "Cancel, close the search, or drop the selection"),
    ("F1 / ?", "This list"),
];

/// Height of one icon-view cell: the thumbnail box, the gap, and two lines of
/// name beneath it.
/// An icon cell with no labels under it: the thumbnail and its padding.
const ICON_CELL_BASE_H: f32 = 92.0;

/// Height of one label line under an icon.
///
/// `ICON_CELL_BASE_H` plus one of these is 108, which is what the cell was
/// before the labels became choosable -- so the default view is unchanged to
/// the pixel, and only a user who asks for more lines gets taller cells.
const ICON_LABEL_LINE_H: f32 = 16.0;

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
    /// The user's own arrangement, saved per folder.
    ///
    /// Not a column: there is nothing to put in a header, and clicking one
    /// leaves this mode. `roadmap-detailed.md` §4.1 calls it "Custom" /
    /// "Manual" and requires that a column sort override it *temporarily* --
    /// which is why the arrangement lives on disk against the folder and not
    /// in the order of `entries`. Sorting by Name writes nothing, so the
    /// arrangement is still there when Custom comes back.
    Custom,
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
/// A control in the Transfers view.
///
/// Named rather than addressed by coordinates, like every other control in
/// this application: the painter and the click handler ask one layout where a
/// button is, so a button cannot be clickable somewhere other than where it is
/// drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferControl {
    /// Stop a running operation. It keeps its journal and can be resumed.
    CancelRunning(usize),
    /// Move a waiting operation one place earlier.
    MoveQueuedUp(usize),
    /// One place later.
    MoveQueuedDown(usize),
    /// Start a waiting one now, whatever drive it wants.
    StartQueuedNow(usize),
    /// Drop a waiting one. Free: it has written nothing.
    CancelQueued(usize),
}

/// A file operation that has not started, because another is running.
///
/// It holds the *plan*, which is the point: the scan, the conflict policy and
/// the error policy are all settled at the moment the user asked, so a queued
/// operation cannot quietly acquire different answers by the time its turn
/// comes. What it does not hold is a journal -- nothing has been written, and
/// nothing will be until it starts.
struct PendingOperation {
    plan: OperationPlan,
    verb: &'static str,
    keep_undo: bool,
    /// Which drives it will touch, resolved when the user asked rather than
    /// when its turn comes. A queued operation whose source has since
    /// vanished should fail at *start*, which is the only moment the check
    /// can be honest -- not be quietly re-scheduled onto a drive that is no
    /// longer the one it named.
    drives: DriveSet,
}

/// A bulk file operation the explorer is carrying out a slice at a time.
///
/// Deliberately not `Clone` or `PartialEq`: it owns a journal file handle and
/// a half-finished operation, and there is exactly one of it. A copy of one of
/// these would be a second executor writing the same journal.
struct RunningOperation {
    executor: OperationExecutor,
    /// The drives it is loading, which is what everything else waits on.
    drives: DriveSet,
    /// The past-tense verb for the status line: "Pasted", "Moved", "Deleted".
    verb: &'static str,
    /// How many files the plan covers, for "12 of 400".
    total_files: u32,
    /// Events gathered across every slice, so the summary at the end sees the
    /// whole operation. `execute` used to return these in one go.
    events: Vec<FileOpEvent>,
    /// Whether the executor's undo entries are worth keeping. A permanent
    /// delete has none to offer.
    keep_undo: bool,
}

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

/// A press on a row that may become a drag to a new position.
///
/// Held from the press rather than created on the first move, because the
/// press is the only event that knows *which* row is under the pointer -- a
/// move carries a position and nothing else. `active` is what separates a
/// click from a drag, so that selecting a file does not rearrange the folder.
#[derive(Clone, Debug, PartialEq)]
struct RowDrag {
    /// Where the press landed, to measure the threshold against.
    start_x: f32,
    start_y: f32,
    /// The rows being moved, as indices into `entries`.
    rows: Vec<usize>,
    /// Whether the pointer has travelled far enough to mean a drag.
    active: bool,
    /// Where the rows would land: before this index, or at the end.
    insert_at: usize,
}

/// The narrowest the listing may be squeezed to by the preview's divider.
///
/// Wide enough for a name and a size: a listing narrower than this is not a
/// listing, and a user who drags that far has overshot rather than asked for
/// it.
/// How many lines of a text file the preview pane reads.
///
/// More than the twenty a thumbnail uses, because this one is read rather
/// than glanced at, and bounded because a preview that reads a gigabyte to
/// show the top of it is a preview that stalls the window. `read_text_lines`
/// caps the bytes as well, so a file of one enormous line cannot beat this.
const PREVIEW_MAX_LINES: usize = 200;

const LIST_MIN_W: f32 = 240.0;

/// The narrowest the preview may be squeezed to.
const PREVIEW_MIN_W: f32 = 160.0;

/// The shortest the listing may be squeezed to, with the preview above or
/// below it. Smaller than the width minimum because a few rows is still a
/// usable listing, where a few pixels of width is not.
const LIST_MIN_H: f32 = 120.0;

/// The shortest the preview may be squeezed to.
const PREVIEW_MIN_H: f32 = 100.0;

/// How thick the line marking a pending drop is.
///
/// Centred on the boundary rather than drawn below it, so it reads as "between
/// these two rows" rather than "on this row" -- which is a different drop.
const INSERTION_LINE_H: f32 = 2.0;

/// How far the pointer must travel before a press becomes a rearrangement.
///
/// A file manager where a slightly unsteady click reorders the folder is worse
/// than one with no manual order at all: the damage is silent, persistent and
/// attributed to the wrong cause. Matches the threshold `guitk::dnd` uses for
/// starting a file drag, so the two feel the same.
const ROW_DRAG_THRESHOLD: f32 = 4.0;

/// A modal the file manager is waiting on, and what to do when it answers.
///
/// One field rather than one `Option` per dialog kind: only one modal can be
/// up at a time -- that is what modal means -- and separate fields would make
/// "a delete confirmation and a rename box, both open" a representable state
/// that every reader has to rule out by hand.
/// A button on the toolbar.
///
/// Named rather than addressed by position, for the reason every `*_rect`
/// accessor in this tree exists: the painter and the click handler ask one
/// function where a button is, so a button cannot be clickable somewhere other
/// than where it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarButton {
    Back,
    Forward,
    Up,
    NewFolder,
    Cut,
    Paste,
}

impl ToolbarButton {
    /// Every button, in the order they are drawn.
    ///
    /// Walked by a test that checks each one is reachable. Seven controls were
    /// painted here with nothing that could open them -- `click_at` knew about
    /// file rows and the sidebar and nothing else -- and iterating the list
    /// rather than trusting whoever adds the seventh is what catches the next
    /// one.
    pub const ALL: [Self; 6] = [
        Self::Back,
        Self::Forward,
        Self::Up,
        Self::NewFolder,
        Self::Cut,
        Self::Paste,
    ];

    /// The glyph on its face.
    const fn glyph(self) -> &'static str {
        match self {
            Self::Back => "\u{2190}",
            Self::Forward => "\u{2192}",
            Self::Up => "\u{2191}",
            Self::NewFolder => "\u{1F4C1}+",
            Self::Cut => "\u{2702}",
            Self::Paste => "\u{1F4CB}",
        }
    }

    /// Whether a separator is drawn before it.
    const fn starts_a_group(self) -> bool {
        matches!(self, Self::NewFolder)
    }
}

enum Modal {
    /// A destructive action the user has been asked to confirm.
    Confirm {
        dialog: AlertDialog,
        action: PendingAction,
    },
    /// Something went wrong, and the user is being told so they cannot miss
    /// it. Its only answer is "OK" and nothing acts on it.
    Notice { dialog: AlertDialog },
    /// A new folder awaiting its name.
    ///
    /// Its own variant rather than a `Rename` with an empty target: the two
    /// answer the same dialog and do opposite things with it, and a target
    /// that means "no file" is a `None` somebody will forget to check.
    NewFolder { dialog: InputDialog },
    /// A search awaiting the text to look for.
    Search { dialog: InputDialog },
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
    /// The user's colours, handed over by the framework (§822).
    pub palette: Palette,

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
    /// The address bar: breadcrumbs you can click, and a path you can type.
    ///
    /// `guitk::pathbar`, not a label. What was here was a `String` assigned
    /// from `current_path` and never read from input, drawn inside a stroked
    /// box that looks exactly like a text field -- so it promised editing and
    /// could not do it, which is worse than a dead button because the
    /// affordance itself is the claim. Meanwhile the toolkit's path bar, 1,945
    /// lines and 44 tests with breadcrumbs, edit mode and autocomplete, had no
    /// users at all. Same defect as the taskbar drawing its own clock beside
    /// an unused `calendar::ClockDisplay`; see design-decisions 469.
    pub pathbar: PathBar,
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
    /// What the pointer is resting on, when that is worth saying.
    ///
    /// Kept apart from `status_message` rather than written into it: a hover
    /// is transient and a status message is a *result*, and letting the
    /// pointer overwrite "Deleted 5 items, 2 failed" on its way past a button
    /// would lose the one line the user needed to read.
    hover_hint: String,
    /// The context menu a right-click opened, if any.
    ///
    /// `guitk::menu::ContextMenu`, not a list drawn here: the shell already
    /// uses that widget for the desktop menu and the tray overflow, and a
    /// second menu implementation in the file manager would be the third.
    menu: Option<ContextMenu>,
    /// The bulk file operations in flight.
    ///
    /// More than one, but never two that touch the same drive: that is the
    /// whole of `roadmap.md` 4.1. Two operations on one disk interleave two
    /// access patterns into a single device queue, so neither stream gets a
    /// contiguous run and both finish later than they would in turn.
    operations: Vec<RunningOperation>,
    /// Operations waiting for the one in front of them to finish.
    ///
    /// A queue rather than a refusal: a user who starts a second copy meant to
    /// start it, and telling them "no, try again later" makes them sit and
    /// watch for a moment that nothing announces. Nothing has been written for
    /// a waiting operation, so cancelling one is free.
    pending: VecDeque<PendingOperation>,
    /// Derived one-line description of the current directory's contents.
    pub dir_summary: String,
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
    /// Whether the shortcut list is up.
    show_help: bool,
    /// The query whose results are being shown, if the listing is a search.
    ///
    /// `Some` is the whole difference between "this folder" and "matches from
    /// this folder downwards", and it has to be held rather than inferred: the
    /// entries of a search look exactly like the entries of a directory, so
    /// nothing else on screen can tell Escape which one to undo.
    /// The arrangement in force for the folder on screen, if any.
    ///
    /// Cached from the settings document on navigation rather than read per
    /// comparison: `sort_by` runs this against every pair, and a YAML lookup
    /// per comparison would turn a sort into a parse.
    manual_order: Vec<String>,
    /// A row press that may be turning into a rearrangement.
    row_drag: Option<RowDrag>,
    /// Whether the preview panel is showing beside the listing.
    preview_open: bool,
    /// The listing's share of the file pane, along the split's axis.
    preview_split: f32,
    /// Which side of the listing the preview sits on.
    preview_side: columnprefs::PreviewSide,
    /// The lines of the file the preview pane is showing, and which file they
    /// came from.
    ///
    /// Keyed by path *and* mtime so that editing a file while it is selected
    /// re-reads it -- the same key the thumbnail cache uses, for the same
    /// reason. `None` when the pane is shut, nothing is selected, or the
    /// selection is not a text file.
    preview_text: Option<(PathBuf, u64, Vec<String>)>,
    /// Where inside the divider a pointer grabbed it, while dragging.
    ///
    /// The offset is kept rather than just a flag so the divider does not jump
    /// to centre itself under the pointer on the first move -- a drag should
    /// move what is under the finger, not snap it.
    divider_grab: Option<f32>,
    search_showing: Option<String>,
    /// The folder a search started from, to go back to when it is dismissed.
    ///
    /// Kept separately from `current_path` because opening a result navigates,
    /// and a user who then presses Escape means "stop searching", not "go back
    /// to wherever I last clicked".
    search_origin: Option<PathBuf>,
    /// The detail view's column set: which columns are shown, in what order,
    /// at what widths, and which one carries the sort arrow.
    ///
    /// Re-derived from the directory's contents on every load, so a folder of
    /// images grows a Dimensions column and a folder of source grows Language
    /// and Lines without the user asking.
    pub columns: ColumnManager,
    /// The saved column preferences, read once rather than per listing.
    ///
    /// Held rather than re-read on every navigation: `load_directory` runs on
    /// every step through the tree, and a settings file opened that often is a
    /// cost the user pays for a value that changes only when they change it.
    /// Re-read when the picker writes, which is the only thing that alters it.
    pub column_prefs: yamldoc::Document,
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
    /// Which labels the icon view draws under each thumbnail.
    pub icon_labels: columnprefs::IconLabels,
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
        // Read once, before the literal: two fields are derived from it, and
        // reading the file twice would let them disagree if it changed between.
        let prefs = settingsfile::load(columnprefs::CONFIG_NAME);
        let mut state = Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
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
            pathbar: PathBar::new(start_path),
            address_editing: false,
            status_message: String::new(),
            hover_hint: String::new(),
            menu: None,
            operations: Vec::new(),
            pending: VecDeque::new(),
            dir_summary: String::new(),
            window_width: 900,
            window_height: 600,
            sidebar_width: 200.0,
            undo: UndoStack::new(),
            recycle: RecycleBin::default_location(),
            modal: None,
            show_help: false,
            manual_order: Vec::new(),
            row_drag: None,
            preview_open: columnprefs::preview_open(&prefs),
            preview_split: columnprefs::preview_split(&prefs),
            preview_side: columnprefs::preview_side(&prefs),
            preview_text: None,
            divider_grab: None,
            search_showing: None,
            search_origin: None,
            columns: ColumnManager::with_defaults(),
            column_prefs: prefs,
            thumbs: ThumbnailCache::default_capacity(),
            thumb_gen: ThumbnailGenerator::with_default_disk_cache(),
            icon_labels: columnprefs::icon_labels(&settingsfile::load(columnprefs::CONFIG_NAME)),
            thumb_config: {
                // The size the user last chose, if they chose one. Applied
                // here rather than after construction so the first listing is
                // already generating at the right size -- otherwise every
                // thumbnail on screen at start-up is made twice.
                let mut config = ThumbConfig::default();
                if let Some(size) =
                    columnprefs::thumb_size(&settingsfile::load(columnprefs::CONFIG_NAME))
                {
                    config.size = size;
                }
                config
            },
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
        self.pathbar.set_path(&self.current_path);
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
            self.pathbar.set_path(&self.current_path);
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
            self.pathbar.set_path(&self.current_path);
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
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        if entry.is_dir {
            let path = entry.path.clone();
            self.navigate_to(&path);
            return;
        }
        let (path, name) = (entry.path.clone(), entry.name.clone());
        self.status_message = match Self::opener_for(&path) {
            Some(program) => match process::Command::new(&program).arg(&path).spawn() {
                Ok(_) => format!("Opening {name} with {program}"),
                // Named, because the interesting failures are all about
                // *which* program: an association carried over from another
                // machine names a path that is not here, and saying so is the
                // difference between "this file cannot be opened" and "that
                // association is wrong".
                Err(e) => format!("Could not start {program}: {e}"),
            },
            None => format!("Nothing is set to open {name}"),
        };
    }

    /// The program the user has chosen for this kind of file.
    ///
    /// Read from the File Associations program's own configuration, which
    /// records an **executable path** rather than an application id -- an id is
    /// a name only that program can resolve, and this one has no catalogue to
    /// resolve it in. Same rule as the taskbar's pinned apps.
    ///
    /// Read on every open rather than cached: the user can change an
    /// association in another window while this one is showing a folder, and a
    /// cache would open the previous choice with no way to notice.
    fn opener_for(path: &Path) -> Option<String> {
        // `to_str` rather than bytes: the associations are keys in a YAML
        // document, so they are text by construction and an extension that is
        // not UTF-8 could never match one. Answering `None` here is a refusal,
        // not a lossy conversion.
        let ext = path.extension().and_then(|e| e.to_str())?;
        let doc = settingsfile::load(associations::CONFIG_NAME);
        associations::program_for(&doc, ext)
    }

    /// Describe one path as a row.
    ///
    /// Extracted rather than written twice: search and the folder listing both
    /// need it, and two copies would be two answers to "what is a row" that
    /// could drift -- the type column deriving an extension one way here and
    /// another there. `None` when the path has no final component, which is
    /// the root, and the root is never a row in its own listing.
    fn entry_for(path: PathBuf) -> Option<FileEntry> {
        let name = path.file_name()?.to_string_lossy().to_string();
        let meta = fs::metadata(&path).ok();
        let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
        let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
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

        Some(FileEntry {
            name,
            path,
            is_dir,
            size,
            modified,
            file_type,
            selected: false,
            icon_id: 0,
        })
    }

    // ======================================================================
    // Search
    // ======================================================================

    /// How a search result names itself: its path below the search root.
    ///
    /// Falls back to the whole path when the result is not under `root`, which
    /// should not happen but is not worth a panic if it ever does -- an
    /// over-long label is a worse-looking row, while an unwrap here would be a
    /// crash in a file manager for a cosmetic reason.
    fn relative_label(path: &Path, root: &Path) -> String {
        let shown = path.strip_prefix(root).unwrap_or(path);
        // Lossy on purpose and safe here: this is the text a row is drawn
        // with, and `FileEntry::path` beside it is what opening the row uses.
        // Named rather than chained so the exemption in `lossy-decode.py` has
        // something specific to anchor on -- a bare `.to_string_lossy()` would
        // match every future lossy call in this file too.
        shown.to_string_lossy().into_owned()
    }

    /// Ask what to look for.
    fn open_search(&mut self) {
        // Pre-filled with the query in force, so refining a search is an edit
        // rather than a retype. Empty when this is a fresh one.
        let initial = self.search_showing.clone().unwrap_or_default();
        let dialog = InputDialog::prompt("Find", "Name contains:", &initial);
        self.modal = Some(Modal::Search { dialog });
    }

    /// Replace the listing with everything under this folder matching `query`.
    fn run_search(&mut self, query: &str) {
        // Remembered before the first search, not on every one: refining a
        // query must not move the origin to wherever the previous search left
        // the view.
        if self.search_origin.is_none() {
            self.search_origin = Some(self.current_path.clone());
        }
        let root = self
            .search_origin
            .clone()
            .unwrap_or_else(|| self.current_path.clone());

        let found = search::find(&root, query, self.show_hidden);
        self.status_message = search::describe(&found, query);

        self.entries.clear();
        for path in found.paths {
            if let Some(mut entry) = Self::entry_for(path) {
                // A search row says where it is, not only what it is. Every
                // row in a folder listing shares one parent, so the bare name
                // is enough there; results come from all over the subtree, and
                // two files called `notes.txt` in different folders are the
                // same row twice to anyone reading the screen.
                //
                // Qualifying the Name column rather than adding a Location
                // one, because a column that appears and disappears is the
                // view changing shape -- the thing roadmap-detailed.md §4.1
                // objects to -- and this needs no new column machinery to
                // answer the same question.
                entry.name = Self::relative_label(&entry.path, &root);
                self.entries.push(entry);
            }
        }
        self.selected_indices.clear();
        self.viewport.scroll_to(0, self.entries.len());
        self.search_showing = Some(query.to_string());
        self.sort_entries();
    }

    /// Put the folder listing back.
    fn leave_search(&mut self) {
        let Some(origin) = self.search_origin.take() else {
            return;
        };
        self.search_showing = None;
        // Through `navigate_to` rather than by reloading in place: the search
        // may have been left from a different folder, and every other thing
        // that has to stay in step with the current directory -- the address
        // bar, the drop target, the history -- is kept in step there.
        self.navigate_to(&origin);
        self.status_message = "Search cleared".to_string();
    }

    // ======================================================================
    // Directory loading
    // ======================================================================

    /// Where the insertion line goes, as (y, x, width), or `None`.
    ///
    /// The top edge of the row the drop would land before; the bottom edge of
    /// the last row when it would land at the end. `None` when nothing is
    /// being dragged, and when the frame holds no rows to measure against --
    /// which is every headless test, so this is the one part of the drag that
    /// the suite can only check the negative of.
    fn insertion_line(&self) -> Option<(f32, f32, f32)> {
        let drag = self.row_drag.as_ref()?;
        if !drag.active {
            return None;
        }
        if let Some(rect) = self.dropzone.file_row_rect(drag.insert_at) {
            return Some((rect.y, rect.x, rect.w));
        }
        // Past the last row: sit on its bottom edge rather than vanishing,
        // because "drop at the end" is a real target and a user aiming at it
        // should see the same feedback as any other.
        let last = self.entries.len().checked_sub(1)?;
        let rect = self.dropzone.file_row_rect(last)?;
        Some((rect.y + rect.h, rect.x, rect.w))
    }

    /// Track a press that is turning into a rearrangement.
    fn drag_row(&mut self, x: f32, y: f32) -> bool {
        let Some(drag) = self.row_drag.as_mut() else {
            return false;
        };
        if !drag.active {
            let dx = x - drag.start_x;
            let dy = y - drag.start_y;
            if dx.mul_add(dx, dy * dy) < ROW_DRAG_THRESHOLD * ROW_DRAG_THRESHOLD {
                return false;
            }
            drag.active = true;
        }
        // Before the row under the pointer; past the last row, at the end.
        // Simple enough for a user to predict without being shown an
        // insertion bar, which is the alternative and needs the row
        // rectangles this deliberately does not recompute.
        let target = self.dropzone.find_file_row(x, y);
        let len = self.entries.len();
        if let Some(drag) = self.row_drag.as_mut() {
            drag.insert_at = target.unwrap_or(len);
        }
        true
    }

    /// Finish a row drag, rearranging if it ever became one.
    fn drop_row(&mut self) -> bool {
        let Some(drag) = self.row_drag.take() else {
            return false;
        };
        if !drag.active {
            return false;
        }
        self.reorder_rows(drag.rows, drag.insert_at)
    }

    /// Read the folder's arrangement, and fall out of Custom if it has none.
    ///
    /// The fall-back matters: Custom with no arrangement is name order wearing
    /// a different label, and a user who navigates from an arranged folder to
    /// an unarranged one should see a mode that describes what they are
    /// looking at. `sort_by` is view state, so this is the one place that can
    /// keep it honest as the view moves.
    fn load_manual_order(&mut self) {
        self.manual_order =
            manualorder::for_folder(&self.column_prefs, &self.current_path).unwrap_or_default();
        if self.sort_by == SortBy::Custom && self.manual_order.is_empty() {
            self.sort_by = SortBy::Name;
        }
    }

    /// Record the listing's present order as this folder's arrangement.
    ///
    /// Answers whether it could be written down. `false` for a folder whose
    /// path is not text, and the caller says so rather than pretending --
    /// see [`manualorder::set_for_folder`].
    ///
    /// Names that are not text are left out of the saved list, because a YAML
    /// scalar cannot hold them. They keep their place on screen for this
    /// session and sort with the newcomers next time. That is a real
    /// limitation and it is C-Q24's; the alternative, writing a lossy
    /// rendering, would file the position under a name that belongs to a
    /// different file.
    fn save_manual_order(&mut self) -> bool {
        let names: Vec<String> = self
            .entries
            .iter()
            .filter_map(|e| e.path.file_name().and_then(|n| n.to_str()))
            .map(ToString::to_string)
            .collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let path = self.current_path.clone();
        if !manualorder::set_for_folder(&mut self.column_prefs, &path, &refs) {
            return false;
        }
        self.manual_order = names;
        self.status_message =
            match settingsfile::store(columnprefs::CONFIG_NAME, &self.column_prefs) {
                Ok(()) => String::from("Custom order saved for this folder"),
                // Reported rather than swallowed: the arrangement is on screen
                // either way, so a silent failure looks like success until the
                // folder is revisited and the order has gone.
                Err(e) => format!("Could not save the order: {e}"),
            };
        true
    }

    /// Move the rows at `from` to sit before the row currently at `to`.
    ///
    /// Indices into `entries`, which is what the view hands back from a drag.
    /// Out-of-range indices are ignored rather than clamped: a clamp turns a
    /// bug in the caller into a file moving somewhere the user did not point
    /// at, which is worse than nothing happening.
    fn reorder_rows(&mut self, mut from: Vec<usize>, to: usize) -> bool {
        if from.is_empty() || to > self.entries.len() {
            return false;
        }
        from.sort_unstable();
        from.dedup();
        if from.iter().any(|&i| i >= self.entries.len()) {
            return false;
        }

        // How many of the moved rows sit above the insertion point: the target
        // shifts down by that many once they are lifted out.
        let above = from.iter().filter(|&&i| i < to).count();
        let target = to.saturating_sub(above);

        let mut moved = Vec::with_capacity(from.len());
        for &i in from.iter().rev() {
            moved.push(self.entries.remove(i));
        }
        moved.reverse();

        let at = target.min(self.entries.len());
        for (offset, entry) in moved.into_iter().enumerate() {
            self.entries.insert(at.saturating_add(offset), entry);
        }

        // Arranging is what puts the view into Custom: a user who drags a file
        // has plainly stopped wanting name order, and leaving the mode alone
        // would re-sort their arrangement away on the next refresh.
        self.sort_by = SortBy::Custom;
        self.sync_sort_indicator();
        self.save_manual_order()
    }

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

                    // Hidden-ness from the bytes, so a name with no text
                    // form is judged by the same rule as every other. The
                    // leading dot is ASCII, so this agrees with the text test
                    // for every name that has one.
                    if !self.show_hidden
                        && entry.file_name().as_encoded_bytes().first() == Some(&b'.')
                    {
                        continue;
                    }

                    if let Some(file_entry) = Self::entry_for(entry.path()) {
                        self.entries.push(file_entry);
                    }
                }
            }
            Err(e) => {
                self.status_message = format!("Error: {e}");
            }
        }

        self.load_manual_order();
        self.sort_entries();
        self.update_status();
        // A folder shows what the user saved for it, the default they saved,
        // or the fixed out-of-the-box set -- and nothing derived from what is
        // inside it. `roadmap-detailed.md` §4.1 forbids content-based column
        // selection outright: it makes the view change shape as you navigate,
        // lets one odd file alter the columns, and leaves "why did my columns
        // change?" with no answer a user can reach. Until the picker landed,
        // the guess was the only way any extra column ever appeared, which is
        // why it outlived the rule.
        self.apply_saved_columns();
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
        self.columns
            .active_columns()
            .iter()
            .map(|&id| self.entry_value(entry, id))
            .collect()
    }

    /// One column's value for one entry.
    ///
    /// Extracted from [`Self::row_values`] so the icon view's labels come from
    /// the same place as the detail cells. §4.1 asks for exactly that -- "the
    /// date/size shown match" -- and the only way to be sure of it is for both
    /// to call one function rather than to format the same field twice.
    fn entry_value(&self, entry: &FileEntry, id: ColumnId) -> ColumnValue {
        match id {
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
            other => entry
                .path
                .to_str()
                .map_or(ColumnValue::Empty, |p| self.columns.get_value(p, other)),
        }
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
            // A hand arrangement is not a column, so no header carries an
            // arrow. Pointing one at Name would say the list is in name order
            // when it is in the user's own -- a header that lies about what it
            // is showing is worse than a header that says nothing.
            SortBy::Custom => {
                self.columns.set_sort(ColumnId::NAME, SortOrder::None);
                return;
            }
        };
        let order = match self.sort_dir {
            SortDir::Ascending => SortOrder::Ascending,
            SortDir::Descending => SortOrder::Descending,
        };
        self.columns.set_sort(id, order);
    }

    /// Sort entries according to current sort settings.
    fn sort_entries(&mut self) {
        // A hand arrangement is applied to a listing that is already in name
        // order, so that the files the user has *not* placed appear after the
        // ones they have, in an order that is stable rather than whatever the
        // filesystem returned. `sort_by` is stable, so the earlier pass shows
        // through wherever the arrangement ties.
        if self.sort_by == SortBy::Custom {
            self.entries.sort_by_key(|e| e.name.to_lowercase());
            let order = core::mem::take(&mut self.manual_order);
            self.entries.sort_by(|a, b| {
                manualorder::compare(
                    &order,
                    a.path.file_name().unwrap_or(a.path.as_os_str()),
                    b.path.file_name().unwrap_or(b.path.as_os_str()),
                )
            });
            self.manual_order = order;
            return;
        }

        // Directories always come first -- except in Custom, handled above.
        // The spec has the user arranging "files/folders" together, so forcing
        // folders to the top there would fight the arrangement it asks for:
        // a user who drags a folder below a file would watch it spring back.
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
                // Unreachable: handled above, before the folders-first rule
                // this arm sits inside. Spelled out rather than left to a
                // wildcard so that adding a mode is a compile error here
                // rather than a mode that silently sorts by nothing.
                SortBy::Custom => core::cmp::Ordering::Equal,
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

    // ======================================================================
    // Bulk operations, a slice at a time
    // ======================================================================

    /// Begin a bulk operation, to be carried out a slice at a time.
    ///
    /// Replaces the `executor.execute()` that used to run here. That call did
    /// the whole job before returning, inside an event handler, so the window
    /// froze for its duration and the progress this reports could not be
    /// drawn until it was already over.
    ///
    /// **An operation whose drives are free starts; one whose are not waits.**
    ///
    /// `roadmap.md` 4.1. Two operations on one drive are slower than the same
    /// work done in turn -- they interleave two access patterns into a single
    /// device queue, so neither gets a contiguous run and each one's readahead
    /// is repeatedly thrown away by the other -- and fewer in flight on a
    /// drive is fewer left half-done when it is unplugged. Operations on
    /// *different* drives do not have that problem and do not wait for each
    /// other, which is why this is keyed on the drive rather than being one
    /// lock.
    ///
    /// Waiting rather than being refused, because "no" is the wrong answer to
    /// a user who meant to start it: it makes them watch for a moment nothing
    /// announces, and a copy that quietly did nothing would simply be started
    /// again. Cancelling one before it starts costs nothing, since nothing has
    /// been written.
    fn start_operation(
        &mut self,
        plan: OperationPlan,
        verb: &'static str,
        keep_undo: bool,
    ) -> bool {
        let drives = plan.drives();
        if self.drives_busy(&drives) {
            self.pending.push_back(PendingOperation {
                plan,
                verb,
                keep_undo,
                drives,
            });
            self.update_operation_status();
            return true;
        }
        self.begin_operation(plan, verb, keep_undo, drives)
    }

    /// Whether anything in flight is already loading one of these drives.
    fn drives_busy(&self, drives: &DriveSet) -> bool {
        self.operations
            .iter()
            .any(|op| op.drives.shares_with(drives))
    }

    /// Put an operation into flight. Answers whether it started.
    fn begin_operation(
        &mut self,
        plan: OperationPlan,
        verb: &'static str,
        keep_undo: bool,
        drives: DriveSet,
    ) -> bool {
        let total_files = plan.total_files;
        let mut executor = OperationExecutor::new(plan);
        if !executor.begin() {
            self.report(Self::describe_outcome(&executor.take_events(), verb));
            return false;
        }
        self.operations.push(RunningOperation {
            executor,
            drives,
            verb,
            total_files,
            events: Vec::new(),
            keep_undo,
        });
        self.update_operation_status();
        true
    }

    /// Start every waiting operation whose drives have come free.
    ///
    /// Scans past a blocked one rather than stopping at it: an operation on a
    /// drive nothing is using has no reason to wait behind one that cannot
    /// start yet, and 4.1 says so in as many words -- "an operation on `F:`
    /// starts immediately while two are queued on `D:`".
    fn admit_pending(&mut self) {
        let mut index = 0;
        while index < self.pending.len() {
            let Some(next) = self.pending.get(index) else {
                break;
            };
            if self.drives_busy(&next.drives) {
                index = index.saturating_add(1);
                continue;
            }
            let Some(next) = self.pending.remove(index) else {
                break;
            };
            // Not advanced on a start: `remove` shifted the rest down, so the
            // same index is now the next candidate. Not advanced on a failure
            // either, for the same reason -- and a failure is reported by
            // `begin_operation` rather than swallowed here.
            self.begin_operation(next.plan, next.verb, next.keep_undo, next.drives);
        }
    }

    /// How many operations are waiting to start.
    #[must_use]
    pub fn queued_count(&self) -> usize {
        self.pending.len()
    }

    /// Stop the running operation and drop everything waiting behind it.
    ///
    /// One action rather than two, because it is one intent: a user pressing
    /// Escape during a bulk copy means "stop", not "stop this one and then
    /// watch the next of my own operations start by itself", which is what
    /// cancelling only the running one would do.
    ///
    /// The two halves cost differently and the message says so. Cancelling the
    /// *running* one leaves every file wholly moved or wholly not and keeps its
    /// journal, so it can be resumed; dropping the *waiting* ones is free,
    /// because nothing has been written for them at all.
    ///
    /// Answers whether there was anything to stop, so a caller cannot report a
    /// cancellation that did not happen.
    pub fn cancel_all_operations(&mut self) -> bool {
        let waiting = self.pending.len();
        self.pending.clear();
        let stopped = self.cancel_operation();
        // Stated here rather than in `cancel_operation`: `stopped` is now
        // "at least one was running", and the message below already says
        // "Stopping…" rather than naming one.
        if !stopped && waiting == 0 {
            return false;
        }
        self.status_message = match (stopped, waiting) {
            (true, 0) => "Stopping…".to_string(),
            (true, 1) => "Stopping… — 1 waiting operation dropped".to_string(),
            (true, n) => format!("Stopping… — {n} waiting operations dropped"),
            (false, 1) => "1 waiting operation dropped".to_string(),
            (false, n) => format!("{n} waiting operations dropped"),
        };
        true
    }

    /// Move a waiting operation up or down the queue.
    ///
    /// Answers whether it moved, so a caller cannot report a reorder that did
    /// not happen -- the ends of the queue are where a repeated key or a
    /// held button spends most of its time.
    ///
    /// Only the *waiting* ones. The running operations are not in an order the
    /// user can choose: they are already running, and 4.1's reordering is
    /// about what happens next.
    pub fn move_queued(&mut self, index: usize, delta: isize) -> bool {
        let Some(target) = index.checked_add_signed(delta) else {
            return false;
        };
        if index >= self.pending.len() || target >= self.pending.len() || target == index {
            return false;
        }
        let Some(op) = self.pending.remove(index) else {
            return false;
        };
        self.pending.insert(target, op);
        self.update_operation_status();
        true
    }

    /// Start a waiting operation now, whether or not its drives are free.
    ///
    /// **The per-drive rule is a default, not a prohibition** -- `roadmap.md`
    /// 4.1 says so in as many words, and this is what it means: the user may
    /// know something the scheduler does not. They may know the two operations
    /// are on different partitions of one disk and they do not care, or that
    /// one is three files and the other is four hours.
    ///
    /// What they cannot override is the honesty: it starts, alongside, and the
    /// status line says how many are running. Answers whether it started, so
    /// a caller cannot report a start that did not happen -- a plan whose
    /// journal will not open still fails here.
    pub fn start_queued_now(&mut self, index: usize) -> bool {
        if index >= self.pending.len() {
            return false;
        }
        let Some(next) = self.pending.remove(index) else {
            return false;
        };
        self.begin_operation(next.plan, next.verb, next.keep_undo, next.drives)
    }

    /// Drop a waiting operation. Free: it has written nothing.
    ///
    /// Answers whether there was one at that position, so a caller cannot
    /// quietly cancel nothing -- the failure mode the whole status line is
    /// written against.
    pub fn cancel_queued(&mut self, index: usize) -> bool {
        let removed = self.pending.remove(index).is_some();
        if removed {
            self.update_operation_status();
        }
        removed
    }

    /// Work at the operation for a slice of a frame. Answers whether anything
    /// changed on screen.
    ///
    /// **A time budget, not a fixed number of actions.** A step is one *file*,
    /// and files are not one size: a folder of thumbnails would crawl at one
    /// per frame, while a fixed batch of twenty would block for seconds on
    /// twenty videos. What the window cares about is how long until it can
    /// draw again, so that is what is measured.
    ///
    /// The honest limit, which the budget cannot fix: a step is a whole file,
    /// so a single enormous one still holds the loop for as long as its copy
    /// takes. Interrupting *within* a file needs chunked copying inside the
    /// engine and is recorded as its own entry.
    fn step_operation(&mut self) -> bool {
        if self.operations.is_empty() {
            return false;
        }
        // The frame's budget split between them, not given to each: two
        // operations must not cost twice the frame. An uneven split would let
        // whichever is first starve the rest.
        let share = OPERATION_SLICE
            .checked_div(u32::try_from(self.operations.len()).unwrap_or(1))
            .unwrap_or(OPERATION_SLICE);
        for running in &mut self.operations {
            // `checked_add`, not `+`: a deadline past the end of the monotonic
            // clock is not a real moment, and saturating to *now* is the safe
            // reading -- one action this frame rather than an unbounded slice.
            let now = std::time::Instant::now();
            let deadline = now.checked_add(share).unwrap_or(now);
            while !running.executor.is_done() && std::time::Instant::now() < deadline {
                running.executor.step();
            }
            running.events.append(&mut running.executor.take_events());
        }
        self.retire_finished();
        self.update_operation_status();
        true
    }

    /// Retire every finished operation: summary, undo entries, fresh listing.
    ///
    /// Then admit whatever was waiting on the drives they have just let go of.
    fn retire_finished(&mut self) {
        let mut done: Vec<RunningOperation> = Vec::new();
        // Partitioned rather than removed in place: retiring one calls
        // `self.report` and `self.load_directory`, which cannot borrow `self`
        // while the list is being walked.
        let mut still_running = Vec::with_capacity(self.operations.len());
        for op in self.operations.drain(..) {
            if op.executor.is_done() {
                done.push(op);
            } else {
                still_running.push(op);
            }
        }
        self.operations = still_running;

        let retired = !done.is_empty();
        for mut running in done {
            running.executor.finish();
            running.events.append(&mut running.executor.take_events());
            self.report(Self::describe_outcome(&running.events, running.verb));

            if running.keep_undo {
                let (undo_op, entries) = running.executor.into_undo_entries();
                if !entries.is_empty() {
                    self.undo.push(undo_op, entries);
                }
            }
        }
        if retired {
            self.load_directory();
            self.admit_pending();
        }
    }

    /// Ask the operation in progress to stop.
    ///
    /// Only reachable because the operation is stepped. What it leaves behind
    /// is `OperationExecutor::cancel`'s business: every file wholly done or
    /// wholly not, and the journal kept so it can be resumed.
    pub fn cancel_operation(&mut self) -> bool {
        if self.operations.is_empty() {
            return false;
        }
        for running in &mut self.operations {
            running.executor.cancel();
        }
        true
    }

    /// Progress of the operation in flight, for a caller that draws it.
    #[must_use]
    pub fn operation_progress(&self) -> Option<OperationProgress> {
        self.operations
            .first()
            .map(|r| r.executor.progress().clone())
    }

    /// Whether any file operation is running or waiting to.
    ///
    /// The question a caller asks to know whether to keep the clock going, and
    /// the one a test asks to know whether to keep ticking. Separate from
    /// [`operation_progress`](Self::operation_progress), which answers about
    /// *one* operation and would say "nothing here" while three waited.
    #[must_use]
    pub fn work_in_flight(&self) -> bool {
        !self.operations.is_empty() || !self.pending.is_empty()
    }

    /// How many operations are running at once.
    #[must_use]
    pub fn running_count(&self) -> usize {
        self.operations.len()
    }

    /// How far the operation in flight has got, from 0.0 to 1.0.
    ///
    /// **By files, not by bytes**, because that is what the status line beside
    /// it counts. The two numbers are the same claim in two forms, and a bar
    /// nine tenths full next to the words "3 of 10" is worse than either on
    /// its own: the reader has to decide which to believe, and nothing on
    /// screen helps them. Bytes would make a smoother bar and is the right
    /// answer once the line quotes bytes too.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a file count large enough to lose f32 precision is 16 million, \
                  against a track 140 pixels wide"
    )]
    pub fn operation_fraction(&self) -> Option<f32> {
        if self.operations.is_empty() {
            return None;
        }
        // Summed across everything in flight, because there is one bar. Two
        // bars for two operations would need two places to put them, and the
        // status bar has one; a bar that showed only the first would stall at
        // its end while the second was still going.
        let mut done = 0_u64;
        let mut total = 0_u64;
        for running in &self.operations {
            done = done.saturating_add(u64::from(running.executor.progress().completed_files));
            total = total.saturating_add(u64::from(running.total_files));
        }
        if total == 0 {
            // Plans of no files are over the moment they start. A full bar is
            // the honest picture of that, and it is also why this is not a
            // division.
            return Some(1.0);
        }
        Some((done as f32 / total as f32).clamp(0.0, 1.0))
    }

    /// The whole status line for a running operation.
    ///
    /// Separated from `update_operation_status` so that what it *says* can be
    /// checked without arranging for a copy slow enough to still be running
    /// when the assertion happens. That timing is why the ETA went unnoticed
    /// for so long: every test copies a few bytes, finishes inside one tick,
    /// and sees an estimate of zero, which is indistinguishable from an
    /// estimate that is never shown.
    fn operation_line(
        verb: &str,
        progress: &OperationProgress,
        total_files: u32,
        others: usize,
        waiting: usize,
    ) -> String {
        use std::fmt::Write as _;

        let current = Path::new(&progress.current_file)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut line = format!("{verb} {} of {total_files}", progress.completed_files);
        if !current.is_empty() {
            line.push_str(" — ");
            line.push_str(&current);
        }
        if let Some(eta) = Self::eta_text(progress.eta_secs) {
            line.push_str(" — ");
            line.push_str(&eta);
        }
        // The others are counted rather than named. One line cannot carry
        // three operations' filenames, and the count is what a user needs to
        // know they did not lose one. Every `write!` result here is discarded
        // deliberately: writing into a `String` cannot fail, and `?` would
        // mean this returns a `Result` nobody has anything to do with.
        if others > 0 {
            let _ = write!(line, " (+{others} running");
            if waiting > 0 {
                let _ = write!(line, ", {waiting} waiting");
            }
            line.push(')');
        } else if waiting > 0 {
            let _ = write!(line, " ({waiting} waiting)");
        }
        line
    }

    /// How much longer, in words, or `None` when saying would be worse.
    ///
    /// `OperationProgress::update_rates` has computed this on every tick since
    /// the executor was written, and until 2026-09-14 the only thing that ever
    /// read it was `progress_update_rates`, its own test. A copy dialog that
    /// knows how long it has left and does not say is the same defect as one
    /// that does not know.
    ///
    /// Bounded at both ends because an estimate is not always worth making:
    /// under a second it rounds to "0s left", which reads as finished while
    /// the bar is still moving, and over a day it is a number nobody acts on
    /// -- both cases are better served by the file count that is already on
    /// the line. Zero is also what `update_rates` stores when it has no
    /// throughput to divide by, so the lower bound doubles as "not known yet".
    fn eta_text(secs: f64) -> Option<String> {
        if !(1.0..=86_400.0).contains(&secs) {
            return None;
        }
        // Bounded above by the check, so the cast cannot truncate, and bounded
        // below by 1.0, so it cannot go negative.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let whole = secs.round() as u64;
        Some(match whole {
            0..60 => format!("{whole}s left"),
            60..3600 => format!("{}m {}s left", whole / 60, whole % 60),
            _ => format!("{}h {}m left", whole / 3600, (whole % 3600) / 60),
        })
    }

    /// Put the operation's progress where the status bar will find it.
    ///
    /// The waiting count is part of it and not a second line, because there is
    /// only one status bar: an operation the user started and cannot see is
    /// one they will start again.
    fn update_operation_status(&mut self) {
        let Some(running) = self.operations.first() else {
            return;
        };
        self.status_message = Self::operation_line(
            running.verb,
            running.executor.progress(),
            running.total_files,
            self.operations.len().saturating_sub(1),
            self.pending.len(),
        );
    }

    /// Where the Transfers view is, or `None` when there is nothing in it.
    ///
    /// Above the status bar and over the bottom of the listing rather than
    /// pushing it up: a panel that appeared and reflowed the rows would move
    /// the file under the pointer at the moment a copy started, which is the
    /// moment a user is most likely to be clicking.
    #[must_use]
    pub fn transfers_rect(&self) -> Option<Rect> {
        let rows = self.transfer_rows();
        if rows == 0 {
            return None;
        }
        let height = TRANSFER_ROW_H * rows as f32;
        let w = self.window_width as f32;
        let y = (self.window_height as f32 - STATUS_BAR_H - height).max(0.0);
        Some(Rect::new(0.0, y, w, height))
    }

    /// How many rows the view shows: everything in flight, up to its cap.
    fn transfer_rows(&self) -> usize {
        self.operations
            .len()
            .saturating_add(self.pending.len())
            .min(TRANSFERS_MAX_ROWS)
    }

    /// What each row says, in the order they are drawn.
    ///
    /// Running first, because they are what is happening; waiting after, in
    /// the order they will happen. A waiting row says what it is waiting for
    /// by naming the operation ahead of it, which is `roadmap.md` 4.1's
    /// "waiting for: Copy 12 GB to D:\Backup" -- a queue position on its own
    /// tells the user nothing they can act on.
    #[must_use]
    pub fn transfer_labels(&self) -> Vec<String> {
        let mut rows: Vec<String> = self
            .operations
            .iter()
            .map(|op| {
                format!(
                    "{} {} of {}",
                    op.verb,
                    op.executor.progress().completed_files,
                    op.total_files
                )
            })
            .collect();
        let waiting_for = self.operations.first().map_or_else(
            || "the operation ahead".to_string(),
            |op| op.verb.to_string(),
        );
        rows.extend(
            self.pending
                .iter()
                .map(|op| format!("Queued: {} — waiting for {waiting_for}", op.verb)),
        );
        rows.truncate(TRANSFERS_MAX_ROWS);
        rows
    }

    /// Every control in the view, with where it is drawn.
    ///
    /// The single source of the view's geometry, asked by the painter and by
    /// `transfers_control_at`.
    #[must_use]
    pub fn transfers_layout(&self) -> Vec<(TransferControl, Rect)> {
        let Some(panel) = self.transfers_rect() else {
            return Vec::new();
        };
        let mut controls = Vec::new();
        let running = self.operations.len();
        for row in 0..self.transfer_rows() {
            let y = panel.y + TRANSFER_ROW_H * row as f32 + 2.0;
            let h = TRANSFER_ROW_H - 4.0;
            // Laid out from the right edge inwards, so a long label is what
            // gets squeezed rather than the buttons sliding off the panel.
            let mut x = panel.x + panel.w - TRANSFER_BTN - 4.0;
            let mut place = |control: TransferControl, controls: &mut Vec<_>| {
                controls.push((control, Rect::new(x, y, TRANSFER_BTN, h)));
                x -= TRANSFER_BTN + 2.0;
            };
            if row < running {
                place(TransferControl::CancelRunning(row), &mut controls);
            } else {
                // `saturating_sub` only because the lint asks: this arm is
                // the `else` of `row < running`, so the subtraction cannot
                // go below zero.
                let queued = row.saturating_sub(running);
                place(TransferControl::CancelQueued(queued), &mut controls);
                place(TransferControl::StartQueuedNow(queued), &mut controls);
                place(TransferControl::MoveQueuedDown(queued), &mut controls);
                place(TransferControl::MoveQueuedUp(queued), &mut controls);
            }
        }
        controls
    }

    /// Which control is under a point, if any.
    #[must_use]
    pub fn transfers_control_at(&self, x: f32, y: f32) -> Option<TransferControl> {
        self.transfers_layout()
            .into_iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(control, _)| control)
    }

    /// Do what a Transfers control says. Answers whether anything changed.
    fn press_transfer_control(&mut self, control: TransferControl) -> bool {
        match control {
            TransferControl::CancelRunning(index) => {
                let Some(running) = self.operations.get_mut(index) else {
                    return false;
                };
                running.executor.cancel();
                true
            }
            TransferControl::MoveQueuedUp(index) => self.move_queued(index, -1),
            TransferControl::MoveQueuedDown(index) => self.move_queued(index, 1),
            TransferControl::StartQueuedNow(index) => self.start_queued_now(index),
            TransferControl::CancelQueued(index) => self.cancel_queued(index),
        }
    }

    /// Open the context menu for whatever is at `x, y`.
    ///
    /// Two menus, not one: what you can do to a *file* and what you can do to
    /// the *folder you are looking at* are different lists, and a single menu
    /// offering both would have half its rows greyed out at any moment.
    ///
    /// A right-click on a row also *selects* it, which is what every file
    /// manager does and what makes the menu's "Copy" mean the thing under the
    /// pointer rather than whatever was selected before.
    fn open_context_menu(&mut self, x: f32, y: f32) {
        let on_row = self.dropzone.find_file_row(x, y);
        if let Some(index) = on_row {
            self.select_single(index);
        }
        let items = if on_row.is_some() {
            self.file_menu_items()
        } else {
            self.folder_menu_items()
        };
        let mut menu = ContextMenu::new(items);
        menu.show(x, y, (self.window_width as f32, self.window_height as f32));
        self.menu = Some(menu);
    }

    /// Whether `(x, y)` is over the detail view's header row.
    ///
    /// Only in Details: the other views draw no header, and a menu offering to
    /// choose columns from a view that has none would be a control that cannot
    /// act.
    fn over_column_header(&self, x: f32, y: f32) -> bool {
        if self.view_mode != ViewMode::Details {
            return false;
        }
        let pane = self.pane_rect();
        x >= pane.x && x < pane.x + pane.w && y >= pane.y && y < pane.y + HEADER_H
    }

    /// How tall one icon cell is, given the labels in force.
    ///
    /// The grid stays even because every cell in it is the same height: the
    /// count of *lines* decides it, not the length of any one file's name.
    /// §4.1 asks for that directly -- long names ellipsize rather than wrap
    /// the cell taller than its neighbours.
    fn icon_cell_h(&self) -> f32 {
        ICON_CELL_BASE_H + f32::from(self.icon_labels.lines()) * ICON_LABEL_LINE_H
    }

    /// The three labels an icon may carry, ticked where they are drawn.
    fn icon_label_menu(&self) -> MenuItem {
        let labels = self.icon_labels;
        MenuItem::Submenu {
            id: MENU_ICON_LABEL_BASE,
            label: String::from("Show under icons"),
            icon: None,
            enabled: true,
            children: vec![
                Self::label_row(MENU_ICON_LABEL_BASE, "Name", labels.name),
                Self::label_row(MENU_ICON_LABEL_BASE + 1, "Date modified", labels.date),
                Self::label_row(MENU_ICON_LABEL_BASE + 2, "Size", labels.size),
            ],
        }
    }

    /// One tickable label row.
    fn label_row(id: u64, label: &str, on: bool) -> MenuItem {
        MenuItem::Action {
            id,
            label: label.to_string(),
            shortcut: None,
            icon: None,
            enabled: true,
            checked: Some(on),
        }
    }

    /// Toggle one icon label and remember the set. Answers whether it was ours.
    fn icon_label_action(&mut self, id: u64) -> bool {
        let Some(which) = id.checked_sub(MENU_ICON_LABEL_BASE) else {
            return false;
        };
        let labels = &mut self.icon_labels;
        match which {
            0 => labels.name = !labels.name,
            1 => labels.date = !labels.date,
            2 => labels.size = !labels.size,
            _ => return false,
        }

        let chosen = self.icon_labels;
        columnprefs::set_icon_labels(&mut self.column_prefs, chosen);
        self.status_message =
            match settingsfile::store(columnprefs::CONFIG_NAME, &self.column_prefs) {
                // Said as a count rather than a list, because the menu already
                // shows which: the sentence is here to confirm the change was
                // written down, not to repeat what is on screen.
                Ok(()) => match chosen.lines() {
                    0 => String::from("Icons now show no labels"),
                    1 => String::from("Icons now show 1 label"),
                    n => format!("Icons now show {n} labels"),
                },
                Err(e) => format!("The label choice was not saved: {e}"),
            };
        true
    }

    /// The thumbnail sizes offered, ticked at the one in force.
    ///
    /// A submenu rather than four rows in the folder menu: the sizes are one
    /// choice, and four siblings among the file actions would read as four
    /// unrelated commands.
    fn thumb_size_menu(&self) -> MenuItem {
        MenuItem::Submenu {
            id: MENU_THUMB_SIZE_BASE,
            label: String::from("Thumbnail size"),
            icon: None,
            enabled: true,
            children: columnprefs::THUMB_SIZES
                .iter()
                .map(|size| MenuItem::Action {
                    id: MENU_THUMB_SIZE_BASE.saturating_add(u64::from(*size)),
                    label: format!("{size} pixels"),
                    shortcut: None,
                    icon: None,
                    enabled: true,
                    checked: Some(self.thumb_config.size == *size),
                })
                .collect(),
        }
    }

    /// Adopt `size` for thumbnails and remember it. Answers whether it was ours.
    fn thumb_size_action(&mut self, id: u64) -> bool {
        let Some(offset) = id.checked_sub(MENU_THUMB_SIZE_BASE) else {
            return false;
        };
        let Ok(size) = u32::try_from(offset) else {
            return false;
        };
        if !columnprefs::THUMB_SIZES.contains(&size) {
            return false;
        }
        if self.thumb_config.size == size {
            return true;
        }
        self.thumb_config.size = size;

        // The in-memory cache holds pictures made at the *old* size, and
        // `queue_thumbnails` skips anything it already has -- so without this
        // the view keeps showing the previous size until the folder changes.
        // The cache on disk needs no such help: its filenames carry the size,
        // so a new size misses and regenerates while the old entries stay
        // valid for anyone who switches back.
        self.thumbs.clear();
        self.queue_thumbnails();

        columnprefs::set_thumb_size(&mut self.column_prefs, size);
        self.status_message =
            match settingsfile::store(columnprefs::CONFIG_NAME, &self.column_prefs) {
                Ok(()) => format!("Thumbnails are now {size} pixels"),
                Err(e) => {
                    format!("Thumbnails are now {size} pixels, but the choice was not saved: {e}")
                }
            };
        true
    }

    /// Write the view preferences, saying either `done` or what went wrong.
    ///
    /// One place rather than the same `match settingsfile::store(..)` at each
    /// call site: a preference that is applied on screen but not written down
    /// looks identical to one that was saved, right up until the next start.
    fn persist_view_prefs(&mut self, done: &str) {
        self.status_message =
            match settingsfile::store(columnprefs::CONFIG_NAME, &self.column_prefs) {
                Ok(()) => done.to_string(),
                Err(e) => format!("Could not save the view settings: {e}"),
            };
    }

    /// The file pane split into the listing and the preview, when it is open.
    ///
    /// `None` when the preview is closed, and also when the pane is too narrow
    /// to hold both at their minimums -- a window that cannot fit the preview
    /// shows the listing rather than two unusable slivers, and the preference
    /// stays on so it comes back when the window grows.
    fn preview_panes(&self) -> Option<(Rect, Rect)> {
        if !self.preview_open {
            return None;
        }
        let area = self.pane_rect();
        let side = self.preview_side;
        let axis = if side.is_horizontal() {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        // Measured along the axis being divided: a panel on the left needs
        // room across the width, one below needs it down the height, and
        // checking the wrong one would hide the panel on a window that could
        // hold it perfectly well.
        let span = if side.is_horizontal() { area.w } else { area.h };
        let (list_min, preview_min) = if side.is_horizontal() {
            (LIST_MIN_W, PREVIEW_MIN_W)
        } else {
            (LIST_MIN_H, PREVIEW_MIN_H)
        };
        if span < list_min + preview_min + splitter::DIVIDER {
            return None;
        }

        // `preview_split` is always the LISTING's share, whichever side the
        // panel is on. Storing it that way means moving the panel from right
        // to left keeps the listing the same size, instead of swapping the two
        // and surprising the user with a preview that suddenly fills the
        // window.
        let fractions = if side.is_first() {
            [1.0 - self.preview_split, self.preview_split]
        } else {
            [self.preview_split, 1.0 - self.preview_split]
        };
        let panes = splitter::panes(area, axis, &fractions, splitter::DIVIDER);
        let (first, second) = match (panes.first(), panes.get(1)) {
            (Some(a), Some(b)) => (*a, *b),
            _ => return None,
        };
        Some(if side.is_first() {
            (second, first)
        } else {
            (first, second)
        })
    }

    /// The column picker: every column, ticked when shown, and the two saves.
    fn open_column_menu(&mut self, x: f32, y: f32) {
        let mut items = self.column_menu_items();
        items.push(MenuItem::Separator);
        items.push(Self::menu_action(
            MENU_COLUMNS_SAVE_FOLDER,
            "Save as default for this folder",
            true,
        ));
        items.push(Self::menu_action(
            MENU_COLUMNS_SAVE_GLOBAL,
            "Save as default for all folders",
            true,
        ));
        let mut menu = ContextMenu::new(items);
        menu.show(x, y, (self.window_width as f32, self.window_height as f32));
        self.menu = Some(menu);
    }

    /// One row per column, ticked when it is currently shown.
    ///
    /// Every column the manager knows, not only the ones on screen -- a picker
    /// that listed only what is already visible could never add anything.
    fn column_menu_items(&self) -> Vec<MenuItem> {
        self.columns
            .all_column_defs()
            .iter()
            .map(|def| MenuItem::Action {
                id: MENU_COLUMN_BASE.saturating_add(u64::from(def.id.0)),
                label: def.label.clone(),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: Some(self.columns.is_visible(def.id)),
            })
            .collect()
    }

    /// Toggle a column, or save the current set. Answers whether it was ours.
    fn column_menu_action(&mut self, id: u64) -> bool {
        match id {
            id if (MENU_PREVIEW_SIDE_BASE..MENU_PREVIEW_SIDE_BASE.saturating_add(4))
                .contains(&id) =>
            {
                // Bounded at both ends: an unbounded `>=` would swallow every
                // later menu range, which is the defect the column range
                // already carries a comment about.
                let Some(offset) = usize::try_from(id.saturating_sub(MENU_PREVIEW_SIDE_BASE)).ok()
                else {
                    return false;
                };
                let Some(side) = columnprefs::PreviewSide::ALL.get(offset).copied() else {
                    return false;
                };
                self.preview_side = side;
                columnprefs::set_preview_side(&mut self.column_prefs, side);
                self.persist_view_prefs(side.label());
                true
            }
            MENU_PREVIEW_TOGGLE => {
                self.preview_open = !self.preview_open;
                columnprefs::set_preview_open(&mut self.column_prefs, self.preview_open);
                self.persist_view_prefs(if self.preview_open {
                    "Preview panel shown"
                } else {
                    "Preview panel hidden"
                });
                true
            }
            MENU_COLUMNS_SAVE_FOLDER => {
                let keys = self.columns.visible_keys();
                let saved =
                    columnprefs::set_for_folder(&mut self.column_prefs, &self.current_path, &keys);
                self.status_message = if saved {
                    match settingsfile::store(columnprefs::CONFIG_NAME, &self.column_prefs) {
                        Ok(()) => format!("{} columns saved for this folder", keys.len()),
                        Err(e) => format!("Could not save the columns: {e}"),
                    }
                } else {
                    // The path has no text form, so there is no key to save it
                    // under. Said plainly rather than failing silently.
                    String::from(
                        "This folder's name cannot be written to the settings file, so its columns cannot be saved",
                    )
                };
                true
            }
            MENU_COLUMNS_SAVE_GLOBAL => {
                let keys = self.columns.visible_keys();
                columnprefs::set_global(&mut self.column_prefs, &keys);
                self.status_message =
                    match settingsfile::store(columnprefs::CONFIG_NAME, &self.column_prefs) {
                        Ok(()) => format!("{} columns saved as the default", keys.len()),
                        Err(e) => format!("Could not save the columns: {e}"),
                    };
                true
            }
            // Bounded at both ends. This was `id >= MENU_COLUMN_BASE`, which
            // claimed every id allocated above it -- so the thumbnail sizes,
            // based at 2000 to stay clear, were swallowed here and never
            // reached their own handler. An open-ended range does not stay
            // clear of anything; it takes everything added later.
            _ if (MENU_COLUMN_BASE..MENU_THUMB_SIZE_BASE).contains(&id) => {
                let Ok(raw) = u32::try_from(id.saturating_sub(MENU_COLUMN_BASE)) else {
                    return false;
                };
                let column = ColumnId(raw);
                if self.columns.is_visible(column) {
                    // The last column is not removable: a header row with
                    // nothing in it shows a list the user cannot read.
                    if self.columns.active_columns().len() > 1 {
                        self.columns.remove_column(column);
                    } else {
                        self.status_message = String::from("At least one column has to stay");
                    }
                } else {
                    self.columns.add_column(column);
                }
                true
            }
            _ => false,
        }
    }

    /// What can be done to the file under the pointer.
    fn file_menu_items(&self) -> Vec<MenuItem> {
        vec![
            Self::menu_action(MENU_OPEN, "Open", true),
            Self::menu_action(MENU_CUT, "Cut", true),
            Self::menu_action(MENU_COPY, "Copy", true),
            Self::menu_action(MENU_RENAME, "Rename", true),
            Self::menu_action(MENU_DELETE, "Move to recycle bin", true),
            Self::menu_action(MENU_DELETE_FOREVER, "Delete permanently", true),
        ]
    }

    /// What can be done to the folder being shown.
    fn folder_menu_items(&self) -> Vec<MenuItem> {
        let mut items = vec![
            Self::menu_action(MENU_NEW_FOLDER, "New folder", true),
            // Greyed rather than absent when the clipboard is empty, for the
            // reason the toolbar's buttons are: a menu whose rows come and go
            // moves the others under the pointer between one opening and the
            // next.
            Self::menu_action(MENU_PASTE, "Paste", self.clipboard.is_some()),
            Self::menu_action(MENU_REFRESH, "Refresh", true),
        ];
        // Only where thumbnails are drawn. In Details and List the sizes
        // change nothing visible, and a submenu that silently does nothing is
        // the control-that-cannot-act this tree has plenty of already.
        if self.view_wants_thumbnails() {
            items.push(MenuItem::Separator);
            items.push(self.thumb_size_menu());
            items.push(self.icon_label_menu());
        }

        // Here rather than on the column header's menu, which only Details
        // view has: a panel that can only be switched on from one of three
        // views is a panel most users will never find. The thumbnail settings
        // above are here for the same reason.
        items.push(MenuItem::Separator);
        items.push(MenuItem::Action {
            id: MENU_PREVIEW_TOGGLE,
            label: String::from("Preview panel"),
            shortcut: None,
            icon: None,
            enabled: true,
            checked: Some(self.preview_open),
        });
        // The sides are offered only while the panel is showing: a choice of
        // where to put something invisible is a control with nothing to obey.
        if self.preview_open {
            for (i, side) in columnprefs::PreviewSide::ALL.into_iter().enumerate() {
                items.push(MenuItem::Action {
                    id: MENU_PREVIEW_SIDE_BASE.saturating_add(i as u64),
                    label: String::from(side.label()),
                    shortcut: None,
                    icon: None,
                    enabled: true,
                    checked: Some(self.preview_side == side),
                });
            }
        }
        items
    }

    /// One row of a menu.
    fn menu_action(id: u64, label: &str, enabled: bool) -> MenuItem {
        MenuItem::Action {
            id,
            label: label.to_string(),
            shortcut: None,
            icon: None,
            enabled,
            checked: None,
        }
    }

    /// A press while a context menu is open.
    ///
    /// Answers whether the press was spent here. It always is: a press either
    /// chose a row or dismissed the menu, and neither should also reach what
    /// is underneath -- acting on the thing behind a menu the user was in the
    /// middle of using is a click they could not see coming.
    fn click_menu(&mut self, x: f32, y: f32) -> bool {
        let Some(menu) = self.menu.as_mut() else {
            return false;
        };
        let chosen = menu.handle_click(x, y);
        self.menu = None;
        if let Some(id) = chosen {
            self.activate_menu_item(id);
        }
        true
    }

    /// Carry out a menu row.
    fn activate_menu_item(&mut self, id: u64) {
        // The column picker first: its per-column ids are allocated above
        // every action below, so asking it first costs one comparison and
        // keeps the two id spaces from having to be interleaved here.
        if self.column_menu_action(id) || self.thumb_size_action(id) || self.icon_label_action(id) {
            return;
        }
        match id {
            MENU_OPEN => {
                if let Some(&index) = self.selected_indices.first() {
                    self.open_entry(index);
                }
            }
            MENU_CUT => self.cut_selected(),
            MENU_COPY => self.copy_selected(),
            MENU_RENAME => {
                self.ask_rename();
            }
            MENU_DELETE => {
                self.ask_delete(PendingAction::Recycle);
            }
            MENU_DELETE_FOREVER => {
                self.ask_delete(PendingAction::DeletePermanently);
            }
            MENU_NEW_FOLDER => {
                self.ask_new_folder();
            }
            MENU_PASTE => self.paste(),
            MENU_REFRESH => self.load_directory(),
            // A row id this does not know is a row this did not put there.
            _ => {}
        }
    }

    /// The context menu's draw commands, empty when it is closed.
    #[must_use]
    pub fn render_menu(&self) -> Vec<guitk::render::RenderCommand> {
        self.menu
            .as_ref()
            .map(|menu| menu.render(&self.palette))
            .unwrap_or_default()
    }

    /// Say why the button under the pointer cannot be pressed, if it cannot.
    ///
    /// Answers whether the hint changed, which is what a caller repaints on.
    fn update_hover_hint(&mut self, x: f32, y: f32) -> bool {
        let hint = Self::toolbar_button_at(x, y)
            .map(|button| self.toolbar_button_state(button))
            .filter(DisabledState::is_disabled)
            .and_then(|state| state.reason().map(str::to_string))
            .unwrap_or_default();
        if hint == self.hover_hint {
            return false;
        }
        self.hover_hint = hint;
        true
    }

    /// The text the status bar should display.
    ///
    /// An operation result takes precedence over the directory summary until
    /// the user navigates away.
    ///
    /// **Live progress outranks a hover hint, and a hover hint outranks a
    /// finished operation's summary.** A copy running now is the most
    /// important thing on the bar; with nothing running, what the pointer is
    /// resting on is more useful than a result the user has already read, and
    /// it goes away again the moment the pointer does -- which a summary must
    /// not, since a half-failed delete is exactly what they need to still be
    /// there.
    pub fn status_bar_text(&self) -> &str {
        if self.work_in_flight() {
            return &self.status_message;
        }
        if !self.hover_hint.is_empty() {
            return &self.hover_hint;
        }
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

        // A copy leaves the sources in place, so the clipboard stays usable for
        // a second paste. A cut consumed them, so it must not. Decided here
        // rather than when the operation finishes, because it does not depend
        // on the outcome and the user may well want to paste again before this
        // one is done.
        let was_copy = matches!(op, ClipboardOp::Copy(_));
        if was_copy {
            self.clipboard = Some(op);
        }
        self.start_operation(plan, "Pasted", true);
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
            match OperationPlan::plan_delete(&paths, ErrorPolicy::SkipAndContinue) {
                // No undo entries: a permanent delete has nothing to put back.
                Ok(plan) => {
                    self.start_operation(plan, "Deleted", false);
                }
                Err(e) => {
                    self.report(Outcome::failed(
                        format!("Delete failed: {e}"),
                        e.to_string(),
                    ));
                }
            }
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

        self.start_operation(plan, verb, true);
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
        self.refresh_preview_text();
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
        tree.fill_rect(0.0, 0.0, w, h, self.palette.mantle);

        // Toolbar (top)
        self.render_toolbar(&mut tree);

        // Address bar
        self.render_address_bar(&mut tree);

        // Sidebar (directory tree)
        self.render_sidebar(&mut tree, &mut zones);

        // File list
        self.render_file_list(&mut tree, &mut zones);

        // The Transfers view over the bottom of the listing, before the
        // status bar it sits above.
        self.render_transfers(&mut tree);

        // Status bar (bottom)
        self.render_status_bar(&mut tree);

        // The menu over everything the window draws itself, because it is
        // drawn last and owns the press that follows it.
        tree.commands.extend(self.render_menu());

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
                dialog.render(&self.palette, w, h, &mut tree);
            }
            Some(
                Modal::Rename { dialog, .. }
                | Modal::NewFolder { dialog }
                | Modal::Search { dialog },
            ) => {
                dialog.render(&self.palette, w, h, &mut tree);
            }
            None => {}
        }

        // And the shortcut list over even the dialogs, because it is the one
        // thing a reader asked for explicitly.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut tree,
                &self.palette,
                (self.window_width as f32, self.window_height as f32),
                0.0,
                SHORTCUTS,
                "F1 or ? closes this",
            );
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
            &self.palette,
        ) {
            tree.push(cmd);
        }
    }

    /// Where every toolbar button is drawn.
    ///
    /// The single source of the toolbar's geometry. The painter and
    /// [`toolbar_button_at`](Self::toolbar_button_at) both walk it, so a
    /// button's face and its click band are two readings of one layout rather
    /// than two literals that drift.
    fn toolbar_layout() -> Vec<(ToolbarButton, Rect)> {
        let mut placed = Vec::with_capacity(ToolbarButton::ALL.len());
        let mut x = TOOLBAR_PAD;
        for button in ToolbarButton::ALL {
            if button.starts_a_group() {
                x += TOOLBAR_GROUP_GAP;
            }
            placed.push((button, Rect::new(x, 4.0, TOOLBAR_BTN, TOOLBAR_BTN)));
            x += TOOLBAR_BTN + 4.0;
        }
        placed
    }

    /// Which button is under a point, if any.
    #[must_use]
    pub fn toolbar_button_at(x: f32, y: f32) -> Option<ToolbarButton> {
        Self::toolbar_layout()
            .into_iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(button, _)| button)
    }

    /// Whether a button has anything to do right now, and if not, why not.
    ///
    /// `guitk::disabled::DisabledState`, which carries the reason -- that is
    /// what the type is for, and its own module doc says "shows reason on
    /// hover". This was a bare `bool` when the toolbar was first wired, which
    /// left a greyed button with nothing to say for itself; the toolkit had
    /// the better answer sitting unused. See
    /// `TD-C-SIX-TOOLKIT-WIDGETS-ARE-WRITTEN-TESTED-AND-USED-BY-NOTHING`.
    ///
    /// A disabled button is drawn grey and refuses the click, rather than
    /// being hidden: a toolbar whose buttons come and go moves the others
    /// under the pointer between one glance and the next.
    #[must_use]
    pub fn toolbar_button_state(&self, button: ToolbarButton) -> DisabledState {
        let reason = match button {
            ToolbarButton::Back if self.history_back.is_empty() => Some("Nothing to go back to"),
            ToolbarButton::Forward if self.history_forward.is_empty() => {
                Some("Nothing to go forward to")
            }
            ToolbarButton::Up if self.current_path.parent().is_none() => {
                Some("This is the top of the drive")
            }
            ToolbarButton::Cut if self.selected_indices.is_empty() => {
                Some("Select something to cut")
            }
            ToolbarButton::Paste if self.clipboard.is_none() => Some("The clipboard is empty"),
            _ => None,
        };
        reason.map_or(DisabledState::Enabled, |reason| DisabledState::Disabled {
            reason: Some(reason.to_string()),
        })
    }

    /// Whether a button has anything to do right now.
    #[must_use]
    pub fn toolbar_button_enabled(&self, button: ToolbarButton) -> bool {
        self.toolbar_button_state(button).is_enabled()
    }

    /// Do what a toolbar button says. Answers whether anything changed.
    fn press_toolbar_button(&mut self, button: ToolbarButton) -> bool {
        if !self.toolbar_button_enabled(button) {
            // Deliberately `true`: the press was *on* the button and is not
            // the file list's to interpret. Returning false here would send
            // the click through to the rows underneath and clear the
            // selection -- which is how pressing a greyed-out Cut would
            // deselect the thing you were about to cut.
            return true;
        }
        match button {
            ToolbarButton::Back => self.go_back_if_possible(),
            ToolbarButton::Forward => self.go_forward_if_possible(),
            ToolbarButton::Up => self.go_up_if_possible(),
            ToolbarButton::NewFolder => self.ask_new_folder(),
            ToolbarButton::Cut => {
                self.cut_selected();
                true
            }
            ToolbarButton::Paste => {
                self.paste();
                true
            }
        }
    }

    /// Ask for a name and make a folder with it.
    fn ask_new_folder(&mut self) -> bool {
        let mut dialog = InputDialog::prompt("New folder", "Name:", "");
        dialog.show();
        self.modal = Some(Modal::NewFolder { dialog });
        true
    }

    fn render_toolbar(&self, tree: &mut RenderTree) {
        let toolbar_h = 36.0;
        tree.fill_rect(
            0.0,
            0.0,
            self.window_width as f32,
            toolbar_h,
            self.palette.crust,
        );

        for (button, rect) in Self::toolbar_layout() {
            if button.starts_a_group() {
                tree.fill_rect(
                    rect.x - TOOLBAR_GROUP_GAP / 2.0,
                    4.0,
                    1.0,
                    toolbar_h - 8.0,
                    self.palette.surface1,
                );
            }
            tree.fill_rect(rect.x, rect.y, rect.w, rect.h, self.palette.surface0);
            let ink = if self.toolbar_button_state(button).is_enabled() {
                self.palette.text
            } else {
                // The palette's disabled grey, and this is what it is for:
                // off should look off. See `scripts/check-overlay0-ink.py`.
                self.palette.overlay0
            };
            tree.text_in(
                rect.x + 6.0,
                rect.y + 6.0,
                rect.w - 8.0,
                button.glyph(),
                ink,
                14.0,
            );
        }
    }

    /// Where the address bar is drawn.
    ///
    /// One function, asked by the painter and by the click handler, for the
    /// reason `toolbar_layout` is: a widget that receives clicks somewhere
    /// other than where it is painted is a widget that ignores them.
    fn address_bar_rect(&self) -> Rect {
        Rect::new(0.0, ADDRESS_BAR_Y, self.window_width as f32, ADDRESS_BAR_H)
    }

    fn render_address_bar(&mut self, tree: &mut RenderTree) {
        let rect = self.address_bar_rect();
        // Copied out before the widget borrows `self` mutably: `Palette` is
        // `Copy`, so this costs nothing and keeps the two borrows apart.
        let palette = self.palette;
        // The widget draws in its own space, from (0, 0) to its own size, and
        // knows nothing about where it lives. `PushTranslate` is what the
        // renderer honours, so extending the command list inside one places
        // the whole widget without it having to be told.
        tree.translate(rect.x, rect.y);
        let commands =
            self.pathbar
                .render(&palette, rect.w.max(0.0) as u32, rect.h.max(0.0) as u32);
        tree.commands.extend(commands);
        tree.untranslate();
    }

    /// Show the columns saved for this folder, or the saved default.
    ///
    /// Answers whether either existed. The folder's own preference wins; a
    /// folder with none falls back to the default the user saved for
    /// everything; with neither, the caller decides what to show.
    fn apply_saved_columns(&mut self) -> bool {
        let saved = columnprefs::for_folder(&self.column_prefs, &self.current_path)
            .or_else(|| columnprefs::global(&self.column_prefs));
        let Some(keys) = saved else {
            return false;
        };
        let unknown = self.columns.apply_keys(&keys);
        if !unknown.is_empty() {
            // Named rather than silently dropped: a column that vanishes from
            // a saved view with no explanation reads as a bug in the view.
            self.status_message = format!(
                "{} saved column(s) are not available here: {}",
                unknown.len(),
                unknown.join(", ")
            );
        }
        true
    }

    /// Hand an event to the address bar and act on what it says.
    ///
    /// Answers whether the widget took it.
    fn route_to_pathbar(&mut self, taken: EventResult) -> bool {
        let events = self.pathbar.drain_events();
        for event in events {
            match event {
                PathBarEvent::Navigate(target) => {
                    if target.is_dir() {
                        self.navigate_to(&target);
                    } else {
                        // Marked rather than navigated-to-and-failed: the
                        // widget stays in edit mode with what was typed still
                        // there, so a mistyped path can be corrected instead
                        // of retyped.
                        self.pathbar.set_path_valid(false);
                        // `display()` only because this is a sentence for a
                        // human; the path itself was carried here as bytes and
                        // is never rebuilt from this string.
                        self.status_message = format!("No such folder: {}", target.display());
                    }
                }
                PathBarEvent::RequestAutoComplete { prefix } => {
                    let items = Self::completions_for(&prefix);
                    self.pathbar.set_completions(items);
                }
                // Nothing outside the widget depends on which mode it is in.
                PathBarEvent::EditModeEntered | PathBarEvent::EditModeExited => {}
            }
        }
        taken == EventResult::Consumed
    }

    /// What could complete `prefix`, read from the filesystem.
    ///
    /// The widget deliberately does no I/O of its own -- it asks, and the host
    /// answers -- so that it can be tested without a disk.
    fn completions_for(prefix: &str) -> Vec<CompletionItem> {
        let (dir, partial) = match prefix.rsplit_once('/') {
            Some((dir, partial)) => (if dir.is_empty() { "/" } else { dir }, partial),
            None => (".", prefix),
        };
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut items: Vec<CompletionItem> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                // `to_str`, not `to_string_lossy`. A name that is not UTF-8
                // would come back with U+FFFD substituted for the bytes that
                // are not, and completing to it would produce a path that does
                // not exist -- the bar would offer a folder and then report
                // "No such folder" when it was chosen. Our filenames may hold
                // any byte but `/` and NUL, and this widget is a text field
                // that cannot represent them, so such an entry is skipped
                // rather than mangled. Skipping loses a completion; mangling
                // loses the user's trust in the ones that are offered.
                let name = entry.file_name().to_str()?.to_owned();
                if !name.starts_with(partial) {
                    return None;
                }
                Some(CompletionItem {
                    is_directory: entry.file_type().is_ok_and(|t| t.is_dir()),
                    name,
                })
            })
            .collect();
        // Sorted, because `read_dir` is in whatever order the filesystem
        // keeps and a list that reorders itself between two keystrokes is a
        // list you cannot aim at.
        items.sort_by(|a, b| a.name.cmp(&b.name));
        items
    }

    fn render_sidebar(&self, tree: &mut RenderTree, zones: &mut DropZoneManager) {
        let sidebar_y = 64.0;
        let sidebar_h = self.window_height as f32 - 64.0 - 24.0; // minus toolbar and status bar
        let sw = self.sidebar_width;

        tree.fill_rect(0.0, sidebar_y, sw, sidebar_h, self.palette.mantle);
        tree.stroke_rect(
            sw - 1.0,
            sidebar_y,
            1.0,
            sidebar_h,
            self.palette.surface0,
            1.0,
        );

        // Quick access items
        for (i, (label, path)) in SIDEBAR_ITEMS.iter().enumerate() {
            let iy = sidebar_y + 8.0 + i as f32 * SIDEBAR_ROW_H;
            tree.text(16.0, iy + 4.0, label, self.palette.text, 12.0);
            // The whole strip is the target, not just the glyphs: a drop aimed
            // at the gap beside a short name like "/tmp" is still aimed at
            // /tmp.
            zones.register_sidebar_item(Path::new(path), Rect::new(0.0, iy, sw, SIDEBAR_ROW_H));
        }
    }

    fn render_file_list(&self, tree: &mut RenderTree, zones: &mut DropZoneManager) {
        // With the preview open the listing gets the left pane; without it,
        // the whole thing. Both come from one place so the drop target, the
        // rows and the divider cannot be computed against different widths --
        // which would drop a file into the folder next to the one it was
        // dragged onto.
        // Both from `list_rect`, which is the one place that knows the panel
        // is open. The listing's width used to be decided here and again in
        // `icon_columns`, and the two disagreed.
        let list_rect = self.list_rect();
        let preview_rect = self.preview_panes().map(|(_, preview)| preview);
        let (list_x, list_y, list_w, list_h) = (list_rect.x, list_rect.y, list_rect.w, list_rect.h);

        // The pane itself is the fallback target: anything inside it that is
        // not a folder row means "into the directory being shown".
        zones.set_list_area(list_rect);

        match self.view_mode {
            ViewMode::Details => self.render_details(tree, zones, list_x, list_y, list_w, list_h),
            ViewMode::Icons => self.render_icons(tree, zones, list_x, list_y, list_w, list_h),
            ViewMode::List => self.render_list(tree, zones, list_x, list_y, list_w, list_h),
        }
        if let Some(preview) = preview_rect {
            self.render_preview(tree, preview);
            // The divider last, so it sits over both panes' edges rather than
            // being clipped by whichever drew second.
            tree.fill_rect(
                list_rect.x + list_rect.w,
                list_rect.y,
                splitter::DIVIDER,
                list_rect.h,
                self.palette.surface2,
            );
        }

        // Over the rows, under the scrollbar: the line marks a place
        // between two rows, so it has to be visible above them, but it is not
        // furniture and should not sit over the bar the user may be holding.
        if let Some((line_y, line_x, line_w)) = self.insertion_line() {
            tree.fill_rect(
                line_x,
                line_y - INSERTION_LINE_H / 2.0,
                line_w,
                INSERTION_LINE_H,
                self.palette.blue,
            );
        }
        // After the view, so the bar sits over the rows rather than under them.
        self.render_scrollbar(tree);
    }

    /// The divider's rectangle, between the two panes.
    ///
    /// One function, used by both the hit test and the renderer. A line drawn
    /// in one place and grabbed in another is the defect this crate's
    /// `pane_rect` comment already warns about.
    fn preview_divider_rect(&self) -> Option<Rect> {
        let (list, preview) = self.preview_panes()?;
        Some(if self.preview_side.is_horizontal() {
            let left = if list.x < preview.x { list } else { preview };
            Rect::new(left.x + left.w, left.y, splitter::DIVIDER, left.h)
        } else {
            let top = if list.y < preview.y { list } else { preview };
            Rect::new(top.x, top.y + top.h, top.w, splitter::DIVIDER)
        })
    }

    /// Where inside the divider `(x, y)` grabbed it, if it did.
    fn divider_grab_at(&self, x: f32, y: f32) -> Option<f32> {
        let rect = self.preview_divider_rect()?;
        // Grown by the same margin the toolkit uses, on whichever axis the
        // divider runs across: the grab region is deliberately wider than the
        // drawn line so nobody has to pixel-hunt for it.
        let grown = if self.preview_side.is_horizontal() {
            Rect::new(
                rect.x - splitter::GRAB_MARGIN,
                rect.y,
                rect.w + splitter::GRAB_MARGIN * 2.0,
                rect.h,
            )
        } else {
            Rect::new(
                rect.x,
                rect.y - splitter::GRAB_MARGIN,
                rect.w,
                rect.h + splitter::GRAB_MARGIN * 2.0,
            )
        };
        if !grown.contains(x, y) {
            return None;
        }
        // The offset from the divider's leading edge, so the line keeps its
        // position under the pointer instead of jumping to centre itself.
        Some(if self.preview_side.is_horizontal() {
            x - rect.x
        } else {
            y - rect.y
        })
    }

    /// Move the divider to follow the pointer. Answers whether it moved.
    fn drag_divider(&mut self, x: f32, y: f32) -> bool {
        let Some(offset) = self.divider_grab else {
            return false;
        };
        let area = self.pane_rect();
        let side = self.preview_side;
        let horizontal = side.is_horizontal();
        let (pointer, start, span) = if horizontal {
            (x, area.x, area.w)
        } else {
            (y, area.y, area.h)
        };
        let (list_min, preview_min) = if horizontal {
            (LIST_MIN_W, PREVIEW_MIN_W)
        } else {
            (LIST_MIN_H, PREVIEW_MIN_H)
        };

        // Fractions are in LAYOUT order, so the minimums must be too. With the
        // preview first, index 0 is the preview and its minimum belongs there;
        // passing them the other way round would let a drag squeeze whichever
        // pane happened to be leading past a limit that is not its own.
        let mut fractions = if side.is_first() {
            [1.0 - self.preview_split, self.preview_split]
        } else {
            [self.preview_split, 1.0 - self.preview_split]
        };
        let mins = if side.is_first() {
            [preview_min, list_min]
        } else {
            [list_min, preview_min]
        };

        let moved = splitter::resize(
            &mut fractions,
            0,
            pointer - offset - start,
            span,
            splitter::DIVIDER,
            &mins,
        );
        if moved {
            // Stored as the LISTING's share whichever side the panel is on, so
            // moving the panel across does not resize it.
            self.preview_split = if side.is_first() {
                fractions[1]
            } else {
                fractions[0]
            };
        }
        moved
    }

    /// Let go of the divider, remembering where it was left.
    ///
    /// Written on release rather than on every move: a drag is dozens of
    /// events and each one would be a file write, which is both wasteful and a
    /// way to leave a half-written settings file if the drag is interrupted.
    fn drop_divider(&mut self) -> bool {
        if self.divider_grab.take().is_none() {
            return false;
        }
        columnprefs::set_preview_split(&mut self.column_prefs, self.preview_split);
        self.persist_view_prefs("Preview panel resized");
        true
    }

    /// Draw the preview panel: the selected picture, or why there is none.
    ///
    /// Shows the thumbnail rather than decoding the original at full size. A
    /// preview pane is a few hundred pixels wide, the thumbnail is already
    /// decoded and already uploaded, and re-reading a forty-megapixel photo to
    /// fill it would stall the window on every arrow-key press. A larger
    /// thumbnail size is the lever if the preview looks soft, and that is a
    /// setting the user already has.
    /// Re-read the previewed file when the selection moves to another one.
    ///
    /// Here rather than at each of the eight places the selection changes: a
    /// refresh every one of those has to remember is a refresh one of them
    /// forgets. `render` already takes `&mut self`, so this costs a path
    /// comparison per frame and a read only when the answer changed.
    fn refresh_preview_text(&mut self) {
        if !self.preview_open {
            self.preview_text = None;
            return;
        }
        let selected = self
            .selected_indices
            .first()
            .and_then(|i| self.entries.get(*i))
            .map(|e| (e.path.clone(), mtime_secs(e.modified)));
        let Some((path, mtime)) = selected else {
            self.preview_text = None;
            return;
        };
        if matches!(&self.preview_text, Some((p, m, _)) if *p == path && *m == mtime) {
            return;
        }
        self.preview_text = thumbs::read_text_lines(&path, PREVIEW_MAX_LINES)
            .filter(|lines| !lines.is_empty())
            .map(|lines| (path, mtime, lines));
    }

    fn render_preview(&self, tree: &mut RenderTree, area: Rect) {
        tree.fill_rect(area.x, area.y, area.w, area.h, self.palette.surface0);

        let selected = self
            .selected_indices
            .first()
            .and_then(|i| self.entries.get(*i));

        let Some(entry) = selected else {
            self.preview_note(tree, area, "Select a file to preview it");
            return;
        };

        // Text before pictures, because a text file has both: the thumbnailer
        // draws its first lines as a 96-pixel minimap, and centred unscaled in
        // a pane this wide that is a picture *of* writing rather than writing.
        // The same lines, at a size a person can read.
        if let Some((path, _, lines)) = &self.preview_text
            && *path == entry.path
        {
            let pad = 12.0;
            let mut view = guitk::textview::SimpleTextView::new(
                (area.w - pad * 2.0).max(0.0),
                (area.h - pad * 2.0).max(0.0),
            );
            view.set_text(&lines.join(
                "
",
            ));
            tree.translate(area.x + pad, area.y + pad);
            view.render(&self.palette, tree);
            tree.untranslate();
            return;
        }

        let Some((id, thumb)) = self.drawable_thumb(entry) else {
            // Said plainly rather than left blank: a blank panel beside a
            // selected file reads as a broken preview, and "no picture" and
            // "not a picture" are different answers.
            self.preview_note(tree, area, "No preview for this file");
            return;
        };

        // Fitted inside the pane, never enlarged past its own pixels: blowing
        // a 96-pixel thumbnail up to fill a wide pane looks like a fault
        // rather than a preview.
        let pad = 12.0;
        let avail_w = (area.w - pad * 2.0).max(0.0);
        let avail_h = (area.h - pad * 2.0).max(0.0);
        let tw = thumb.width as f32;
        let th = thumb.height as f32;
        if tw <= 0.0 || th <= 0.0 || avail_w <= 0.0 || avail_h <= 0.0 {
            return;
        }
        let scale = (avail_w / tw).min(avail_h / th).min(1.0);
        let w = tw * scale;
        let h = th * scale;
        tree.push(guitk::render::RenderCommand::Image {
            x: area.x + (area.w - w) / 2.0,
            y: area.y + (area.h - h) / 2.0,
            width: w,
            height: h,
            image_id: id,
        });
    }

    /// One line of explanation, centred in the preview pane.
    fn preview_note(&self, tree: &mut RenderTree, area: Rect, text: &str) {
        tree.push(guitk::render::RenderCommand::Text {
            x: area.x + 12.0,
            y: area.y + area.h / 2.0,
            text: text.to_string(),
            color: self.palette.subtext0,
            font_size: 13.0,
            font_weight: guitk::render::FontWeightHint::Regular,
            max_width: Some((area.w - 24.0).max(0.0)),
            overflow: guitk::render::TextOverflow::Ellipsis,
        });
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
            ViewMode::List => scroll_window::capacity(LIST_ROW_H, pane.h),
            ViewMode::Details => scroll_window::capacity(ROW_H, (pane.h - HEADER_H).max(0.0)),
            ViewMode::Icons => scroll_window::capacity(self.icon_cell_h(), pane.h)
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
            pane.x + pane.w - scrollbar::WIDTH,
            top,
            scrollbar::WIDTH,
            (pane.y + pane.h - top).max(0.0),
        ))
    }

    /// The thumb's rectangle, in the toolkit's coordinates.
    fn scrollbar_thumb(&self) -> Option<guitk::frame::Rect> {
        let track = self.scrollbar_track()?;
        Some(scrollbar::thumb(
            track,
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
        if x < track.x || x >= track.x + track.w {
            return false;
        }
        if y < track.y || y >= track.y + track.h {
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
            track,
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
        let track_gui = track;
        let thumb = scrollbar::thumb(
            track_gui,
            self.entries.len(),
            self.visible_capacity(),
            self.viewport.first_visible(),
        );
        tree.fill_rect(track.x, track.y, track.w, track.h, self.palette.mantle);
        tree.fill_rounded_rect(
            thumb.x + 1.0,
            thumb.y,
            thumb.w - 2.0,
            thumb.h,
            self.palette.surface2,
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
        Self::columns_for(self.list_rect().w)
    }

    /// The part of the pane the *listing* gets.
    ///
    /// With the preview panel open that is the left half, not the whole pane,
    /// and the difference is not small: at 900x700 the grid goes from seven
    /// columns to four. Both the renderer and [`icon_columns`](Self::icon_columns)
    /// come here, because they used to disagree -- `icon_columns` measured the
    /// whole pane while `render_file_list` handed the grid the narrower list
    /// rect, so **with the preview open the wheel stepped seven entries per
    /// row through a grid four wide**, and the top-left cell reported whichever
    /// file the wheel's arithmetic thought was there. `icon_columns`'s own doc
    /// comment already said the two must not disagree; nothing held them to it
    /// until this existed.
    fn list_rect(&self) -> Rect {
        match self.preview_panes() {
            Some((list, _)) => list,
            None => self.pane_rect(),
        }
    }

    /// How many icon cells fit across `w`.
    ///
    /// One formula, called by the renderer and by the wheel. At least one
    /// column however narrow: a zero would make the row index a division by
    /// zero, and a pane too narrow for a cell should clip one rather than draw
    /// none.
    fn columns_for(w: f32) -> usize {
        ((w / ICON_CELL_W) as usize).max(1)
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
        let cols = Self::columns_for(w);
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
        let cell_h = self.icon_cell_h();
        let icon_rows = scroll_window::capacity(cell_h, h);
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
            let cy = cell.checked_div(cols).unwrap_or(0) as f32 * cell_h;

            // Registered in window coordinates, not the pane-local ones the
            // commands are emitted in: the pointer position a drop arrives
            // with is a window position, and translating one of the two at
            // hit-test time would mean the zone list only made sense to a
            // caller that knew which pane it came from.
            zones.register_file_row(
                i,
                &entry.path,
                Rect::new(x + cx, y + cy, ICON_CELL_W, cell_h),
                entry.is_dir,
            );

            if entry.selected {
                tree.fill_rounded_rect(
                    cx + 2.0,
                    cy + 2.0,
                    ICON_CELL_W - 4.0,
                    cell_h - 4.0,
                    with_alpha(self.palette.accent, 40),
                    guitk::style::CornerRadii::all(4.0),
                );
            }

            // Centre the thumbnail box horizontally in the cell; the caption
            // sits under it, using the cell's full width.
            let tx = cx + (ICON_CELL_W - ICON_THUMB_SIZE) / 2.0;
            let ty = cy + 8.0;
            self.push_thumb(tree, entry, tx, ty, ICON_THUMB_SIZE);

            let mut label_y = ty + ICON_THUMB_SIZE + 6.0;
            let name_color = if entry.is_dir {
                self.palette.accent
            } else {
                self.palette.text
            };
            // Elided rather than clipped: a name cut mid-word with no mark is
            // read as the whole name, which is how one file gets mistaken for
            // another whose name it is a prefix of.
            if self.icon_labels.name {
                tree.text_in(
                    cx + 4.0,
                    label_y,
                    ICON_CELL_W - 8.0,
                    &entry.name,
                    name_color,
                    ICON_LABEL_SIZE,
                );
                label_y += ICON_LABEL_LINE_H;
            }
            // Date and size come from `entry_value`, which is what the detail
            // cells use, so the two views cannot disagree about the same file.
            // Drawn in the dimmer ink: they are context for the name, and
            // three lines of equal weight under every icon reads as a table
            // that has lost its columns.
            for (wanted, id) in [
                (self.icon_labels.date, ColumnId::DATE_MODIFIED),
                (self.icon_labels.size, ColumnId::SIZE),
            ] {
                if !wanted {
                    continue;
                }
                let text = self.entry_value(entry, id).display();
                if !text.is_empty() {
                    tree.text_in(
                        cx + 4.0,
                        label_y,
                        ICON_CELL_W - 8.0,
                        &text,
                        self.palette.subtext0,
                        ICON_LABEL_SIZE,
                    );
                }
                // Advanced even when the value is blank -- a folder has no
                // size -- so every cell's lines land at the same heights and
                // the grid reads as rows rather than as drifting text.
                label_y += ICON_LABEL_LINE_H;
            }
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
                tree.fill_rect(0.0, ry, w, LIST_ROW_H, with_alpha(self.palette.accent, 40));
            } else if i % 2 == 1 {
                tree.fill_rect(0.0, ry, w, LIST_ROW_H, self.palette.base);
            }

            let ty = ry + (LIST_ROW_H - LIST_THUMB_SIZE) / 2.0;
            self.push_thumb(tree, entry, 6.0, ty, LIST_THUMB_SIZE);

            let name_x = 6.0 + LIST_THUMB_SIZE + 8.0;
            let name_color = if entry.is_dir {
                self.palette.accent
            } else {
                self.palette.text
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

        tree.fill_rect(0.0, 0.0, ICON_GUTTER, HEADER_H, self.palette.crust);
        tree.translate(ICON_GUTTER, 0.0);
        for cmd in columns::render_column_header(&self.columns, table_w, &self.palette) {
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
                tree.fill_rect(0.0, ey, w, ROW_H, with_alpha(self.palette.accent, 40));
            // Striped by *absolute* row, so the banding does not crawl as the
            // view scrolls -- a stripe that follows the screen position rather
            // than the file makes a scrolling list shimmer.
            } else if i % 2 == 1 {
                tree.fill_rect(0.0, ey, w, ROW_H, self.palette.base);
            }

            let mut icon = [0u8; 4];
            tree.text(
                8.0,
                ey + 3.0,
                entry.file_type.icon_char().encode_utf8(&mut icon),
                self.palette.text,
                12.0,
            );

            // Directory names stay visually distinct from file names, as they
            // were when this view drew its own three columns.
            let name_color = entry.is_dir.then_some(self.palette.accent);

            let values = self.row_values(entry);
            tree.translate(ICON_GUTTER, 0.0);
            for cmd in columns::render_column_values_from(
                &self.columns,
                &values,
                ey,
                table_w,
                name_color,
                &self.palette,
            ) {
                tree.push(cmd);
            }
            tree.untranslate();
        }

        tree.untranslate();
    }

    /// Draw the Transfers view, if there is anything in flight.
    ///
    /// The labels and the button rectangles come from
    /// [`transfer_labels`](Self::transfer_labels) and
    /// [`transfers_layout`](Self::transfers_layout), which the click handler
    /// also reads -- so what is painted and what can be pressed are two
    /// readings of one layout rather than two sets of literals.
    fn render_transfers(&self, tree: &mut RenderTree) {
        let Some(panel) = self.transfers_rect() else {
            return;
        };
        tree.fill_rect(panel.x, panel.y, panel.w, panel.h, self.palette.mantle);

        let controls = self.transfers_layout();
        // The leftmost button on each row is where its label has to stop.
        // Measured from the controls rather than assumed, because a running
        // row has one button and a waiting row has four.
        let labels = self.transfer_labels();
        for (row, label) in labels.iter().enumerate() {
            let y = panel.y + TRANSFER_ROW_H * row as f32;
            let row_band = y..y + TRANSFER_ROW_H;
            let leftmost = controls
                .iter()
                .filter(|(_, r)| row_band.contains(&(r.y + r.h / 2.0)))
                .map(|(_, r)| r.x)
                .fold(panel.x + panel.w, f32::min);
            let room = (leftmost - panel.x - 16.0).max(0.0);
            tree.text_in(panel.x + 8.0, y + 4.0, room, label, self.palette.text, 11.0);
        }

        for (control, rect) in controls {
            tree.fill_rect(rect.x, rect.y, rect.w, rect.h, self.palette.surface0);
            let glyph = match control {
                TransferControl::CancelRunning(_) | TransferControl::CancelQueued(_) => "\u{2715}",
                TransferControl::MoveQueuedUp(_) => "\u{25B2}",
                TransferControl::MoveQueuedDown(_) => "\u{25BC}",
                TransferControl::StartQueuedNow(_) => "\u{25B6}",
            };
            tree.text_in(
                rect.x + 4.0,
                rect.y + 2.0,
                rect.w - 6.0,
                glyph,
                self.palette.text,
                11.0,
            );
        }
    }

    fn render_status_bar(&self, tree: &mut RenderTree) {
        let bar_y = self.window_height as f32 - 24.0;
        let w = self.window_width as f32;

        tree.fill_rect(0.0, bar_y, w, 24.0, self.palette.crust);

        // The track takes its room out of the *text's*, and the text is
        // measured against what is left. `tree.text` sets no `max_width` at
        // all, so the line this replaces ran off the end of the window on any
        // path long enough -- and a silently clipped status line reads as a
        // short one, which is how a truncated path gets taken for the whole
        // thing. `text_in` elides and marks the cut.
        let mut text_width = (w - 16.0).max(0.0);
        if let Some(fraction) = self.operation_fraction() {
            // A third of the bar at most: on a narrow window the words matter
            // more than the picture, and below the floor there is no picture
            // worth having -- so the track is dropped rather than drawn as a
            // sliver the text then has to squeeze past.
            let track_w = PROGRESS_TRACK_W.min(w / 3.0);
            if track_w >= PROGRESS_TRACK_MIN_W {
                let track_x = w - 8.0 - track_w;
                text_width = (track_x - 16.0).max(0.0);
                tree.fill_rect(track_x, bar_y + 8.0, track_w, 8.0, self.palette.surface1);
                tree.fill_rect(
                    track_x,
                    bar_y + 8.0,
                    track_w * fraction,
                    8.0,
                    self.palette.blue,
                );
            }
        }

        tree.text_in(
            8.0,
            bar_y + 5.0,
            text_width,
            self.status_bar_text(),
            self.palette.subtext0,
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

/// How long a file operation may hold the loop before letting it draw.
///
/// Eight milliseconds of a sixteen-millisecond frame: enough that the copy is
/// not paying a frame's latency per file, little enough that the window still
/// answers. See `ExplorerState::step_operation` for why this is a time budget
/// rather than a count of files.
const OPERATION_SLICE: std::time::Duration = std::time::Duration::from_millis(8);

/// How often the loop comes back while an operation is running.
const OPERATION_TICK: std::time::Duration = std::time::Duration::from_millis(16);

// Context menu row ids. Numbered rather than positional, so inserting a row
// cannot silently reassign what the ones below it do.
const MENU_OPEN: u64 = 1;
/// Save the visible columns for the folder being shown.
const MENU_COLUMNS_SAVE_FOLDER: u64 = 100;
/// Save them as the default for folders with no preference of their own.
const MENU_COLUMNS_SAVE_GLOBAL: u64 = 101;
/// Show or hide the preview panel.
const MENU_PREVIEW_TOGGLE: u64 = 102;
/// Put the preview panel on one of the four sides. Offset by `PreviewSide`'s
/// index in `ALL`, which is the order the menu lists them in.
const MENU_PREVIEW_SIDE_BASE: u64 = 200;
/// One id per column, offset so it cannot collide with an action above.
/// `ColumnId` is a small integer, and 1000 is far above every action here.
const MENU_COLUMN_BASE: u64 = 1000;
/// One id per offered thumbnail size, offset clear of the column ids above.
const MENU_THUMB_SIZE_BASE: u64 = 2000;
/// One id per icon-view label toggle, clear of the sizes above.
const MENU_ICON_LABEL_BASE: u64 = 3000;
const MENU_CUT: u64 = 2;
const MENU_COPY: u64 = 3;
const MENU_RENAME: u64 = 4;
const MENU_DELETE: u64 = 5;
const MENU_DELETE_FOREVER: u64 = 6;
const MENU_NEW_FOLDER: u64 = 7;
const MENU_PASTE: u64 = 8;
const MENU_REFRESH: u64 = 9;

/// How tall the status bar is.
const STATUS_BAR_H: f32 = 24.0;
/// How tall one row of the Transfers view is.
const TRANSFER_ROW_H: f32 = 22.0;
/// The side of a square Transfers button.
const TRANSFER_BTN: f32 = 18.0;
/// How many operations the view lists at once.
///
/// A cap rather than a scroll: the panel sits over the listing, and one that
/// grew with the queue would cover the folder the user is working in. The
/// status bar carries the totals, so nothing is hidden -- only undrawn.
const TRANSFERS_MAX_ROWS: usize = 4;

/// Where the address bar starts, directly under the toolbar.
const ADDRESS_BAR_Y: f32 = 36.0;
/// And how tall it is.
const ADDRESS_BAR_H: f32 = 28.0;

/// The toolbar's left margin, and the size and spacing of its buttons.
const TOOLBAR_PAD: f32 = 8.0;
/// The side of a square toolbar button.
const TOOLBAR_BTN: f32 = 28.0;
/// The gap that separates the navigation group from the editing group.
const TOOLBAR_GROUP_GAP: f32 = 12.0;

/// How wide the status bar's progress track is, at most.
const PROGRESS_TRACK_W: f32 = 140.0;

/// And the width below which it is not drawn at all.
///
/// A track narrower than this cannot show the difference between a tenth and a
/// fifth, so it is a decoration that costs the status line room it needs for
/// words. Dropping it is the better answer on a narrow window.
const PROGRESS_TRACK_MIN_W: f32 = 40.0;

impl oswindow::app::App for ExplorerState {
    /// Adopt the user's colours (§822).
    ///
    /// Without this override the trait's default does nothing and the crate
    /// keeps its own colours -- which here were a hardcoded *light* theme, so
    /// a dark desktop got a white file manager.
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

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
        // A file operation asks for the frame interval, not the thumbnail
        // one: it is the thing the user is watching, and its progress line
        // should move as smoothly as anything else on screen. Named first
        // because it is the shorter of the two and this returns the one it
        // finds.
        if self.work_in_flight() {
            return Some(OPERATION_TICK);
        }
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
        // A modal owns the INPUT while it is up. Falling through to the
        // listing as well is how a Delete confirmation also moves the
        // selection, so that confirming it acts on a different file than the
        // one the dialog named.
        //
        // **A tick is not input, and returning on one was a bug.** The arm
        // below retires a batch of a running file operation, so a copy stopped
        // making progress for as long as any confirmation was on screen --
        // and `tick_interval` asks for the frame interval precisely because a
        // file operation is "the thing the user is watching". The modal wants
        // the tick too, for its fade, so both get it: the modal first, then
        // the work behind it.
        if self.modal.is_some() {
            let consumed = self.handle_modal_event(event);
            if !matches!(event, Event::Tick { .. }) {
                return consumed;
            }
            // `|`, not `||`: the operation must step even when the modal's
            // fade already said there is something to draw.
            return consumed | self.tick_work();
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
            Event::Tick { .. } => self.tick_work(),
            // `SettingsChanged` is a different kind of "no" from its
            // neighbours here, and is grouped with them only because the
            // answer happens to coincide. The others are events this window
            // has nothing to *do* about; this one is an announcement that the
            // user's settings were rewritten, which this program ignores
            // because it reads no settings file at all -- it draws in its own
            // palette and takes no preference from disk. If that ever stops
            // being true, this arm is where the re-read belongs, and moving it
            // out of this group is part of the change.
            //
            // `ModifierChord` is here for a third reason again: this program
            // never asks for one, so the compositor never sends it. Named
            // rather than swept up in a `_ =>` because a wildcard here would
            // also swallow the *next* event added to the vocabulary, which may
            // well be one this window should act on.
            Event::CloseRequested
            | Event::Moved { .. }
            | Event::FocusIn
            | Event::FocusOut
            | Event::ScaleChanged { .. }
            | Event::ModifierChord { .. }
            // Never arrives: this program claims no idle watch.
            | Event::SessionIdle
            // Cannot arrive here: a tray click is addressed to the connection
            // and `oswindow` hands it to `App::tray_icon_clicked` before the
            // window dispatch. Listed because this match is exhaustive on
            // purpose -- a wildcard would swallow the next event added, which
            // may well be one this program should act on.
            | Event::TrayIconClicked { .. }
            | Event::SettingsChanged { .. } => false,
        }
    }

    fn handle_mouse(&mut self, m: &MouseEvent) -> bool {
        match m.kind {
            // The scrollbar first: it is drawn over the rows, so a press on it
            // is not a press on the file underneath.
            // An open menu owns the next press, whatever button it is and
            // wherever it lands.
            MouseEventKind::Press(_) if self.menu.is_some() => self.click_menu(m.x, m.y),
            MouseEventKind::Press(MouseButton::Left) => {
                self.press_scrollbar(m.x, m.y) || self.click_at(m.x, m.y)
            }
            MouseEventKind::Press(MouseButton::Right) => {
                if self.over_column_header(m.x, m.y) {
                    self.open_column_menu(m.x, m.y);
                } else {
                    self.open_context_menu(m.x, m.y);
                }
                true
            }
            MouseEventKind::Release(MouseButton::Left) => {
                let was = self.thumb_grab.take();
                // A row drag is finished here whether or not it became active,
                // so an ordinary click cannot leave one armed to fire on the
                // next stray move.
                let dropped = self.drop_row();
                let divider = self.drop_divider();
                was.is_some() || dropped || divider
            }
            MouseEventKind::Move if self.thumb_grab.is_some() => self.drag_scrollbar(m.y),
            MouseEventKind::Move if self.divider_grab.is_some() => self.drag_divider(m.x, m.y),
            MouseEventKind::Move if self.row_drag.is_some() => self.drag_row(m.x, m.y),
            // Motion with nothing grabbed: the only thing the explorer does
            // with it is say why the button under the pointer is greyed.
            MouseEventKind::Move => self.update_hover_hint(m.x, m.y),
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
        // The divider first: it is drawn over the panes' edges, so a press on
        // it must be spent here rather than selecting whatever row happens to
        // end underneath. Its grab region is wider than the line, which is the
        // whole reason to ask the toolkit rather than compare against `x`.
        if let Some(offset) = self.divider_grab_at(x, y) {
            self.divider_grab = Some(offset);
            return true;
        }
        // The toolbar first. It is drawn above the list and does not overlap
        // it, but asking in draw order is what keeps that true when one of
        // them moves.
        if let Some(button) = Self::toolbar_button_at(x, y) {
            return self.press_toolbar_button(button);
        }
        // The Transfers view before the listing it covers. A press anywhere
        // inside it is spent there even when it named no button: the rows
        // underneath are hidden, and acting on a file the user cannot see is
        // worse than doing nothing.
        if let Some(panel) = self.transfers_rect()
            && panel.contains(x, y)
        {
            if let Some(control) = self.transfers_control_at(x, y) {
                self.press_transfer_control(control);
            }
            return true;
        }
        let address = self.address_bar_rect();
        if address.contains(x, y) {
            // Translated into the widget's own space, the way the drop zones
            // convert a screen point: the widget's hit tests are in the
            // coordinates it drew in.
            let taken = self.pathbar.handle_mouse_event(&MouseEvent {
                x: x - address.x,
                y: y - address.y,
                kind: MouseEventKind::Press(MouseButton::Left),
            });
            return self.route_to_pathbar(taken);
        }
        if let Some(index) = self.dropzone.find_file_row(x, y) {
            // Remembered on every row press, not only in Custom mode: dragging
            // a file is how a user *enters* Custom, so requiring the mode
            // first would make it unreachable by the gesture that is supposed
            // to create it.
            // Dragging a row that is already part of the selection moves the
            // whole selection; dragging an unselected row moves just it. Read
            // *before* the selection handling below runs, because that is
            // about to replace the selection with this row -- and the question
            // being asked is what the user had chosen when they grabbed it.
            let rows = if self.selected_indices.contains(&index) {
                self.selected_indices.clone()
            } else {
                vec![index]
            };
            self.row_drag = Some(RowDrag {
                start_x: x,
                start_y: y,
                rows,
                active: false,
                insert_at: index,
            });
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
        // The address bar first, while it is being edited -- and for the one
        // chord that *starts* editing. Routing only on `is_editing` would have
        // made `Ctrl+L` unreachable: the widget documents it as working
        // "regardless of current mode", and the only way into that mode from
        // the keyboard is the chord the guard would have withheld.
        //
        // Not unconditional: a widget that swallowed keys whenever it was
        // merely *visible* would take the arrow keys the file list needs.
        let starts_editing = k.modifiers.ctrl && k.key == Key::L;
        if self.pathbar.is_editing() || starts_editing {
            let taken = self.pathbar.handle_key_event(k);
            if self.route_to_pathbar(taken) {
                return true;
            }
        }
        // The shortcut list, after the address bar and only with no dialog up:
        // both of those take typed text, and `?` belongs in a filename or a
        // path before it belongs to help.
        if self.modal.is_none() {
            let asked = k.key == Key::F1 || (k.key == Key::Slash && k.modifiers.shift);
            if asked {
                self.show_help = !self.show_help;
                return true;
            }
            if k.key == Key::Escape && self.show_help {
                self.show_help = false;
                return true;
            }
        }

        let ctrl = k.modifiers.ctrl;
        match k.key {
            Key::A if ctrl => {
                if self.entries.is_empty() {
                    return false;
                }
                self.select_all();
                true
            }
            Key::F if ctrl => {
                self.open_search();
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
            // Escape stops file work before it clears a selection. A user
            // watching a copy they did not mean to start reaches for Escape,
            // and there is nothing else on the keyboard that means "stop
            // that"; a selection, by contrast, is cleared by clicking
            // anywhere. It does not do both at once, either -- one key with
            // two effects is how someone loses a selection they wanted while
            // trying to stop a copy.
            Key::Escape if self.work_in_flight() => {
                self.cancel_all_operations();
                true
            }
            // Ordered after cancelling work and before clearing a selection,
            // on the same reasoning the comment above gives: the more
            // surprising state to be left in is the one Escape should undo
            // first, and a listing that is secretly a search is exactly that.
            Key::Escape if self.search_showing.is_some() => {
                self.leave_search();
                true
            }
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
            // The three view modes. `set_view_mode` was the only writer of
            // `view_mode` and had no caller, so this window was permanently
            // in Details: `List` and `Icons` both render correctly, are
            // obeyed by the layout, the navigation step and the header, and
            // could never be seen.
            Key::Num1 => {
                self.set_view_mode(ViewMode::Details);
                true
            }
            Key::Num2 => {
                self.set_view_mode(ViewMode::List);
                true
            }
            Key::Num3 => {
                self.set_view_mode(ViewMode::Icons);
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
    /// What one tick moves on: the running file operation, and the thumbnail
    /// queue. Returns whether either produced something new to draw.
    ///
    /// Factored out of the `Event::Tick` arm so the modal path can call it
    /// too. A modal takes the input while it is up; it must not take the
    /// progress of work that was already running behind it.
    fn tick_work(&mut self) -> bool {
        // Both, not either: a copy running while thumbnails generate must not
        // stop the thumbnails, and `||` would short-circuit past the second
        // call rather than merely past its answer.
        let stepped = self.step_operation();
        let thumbed = self.pump_thumbnails_default() > 0;
        stepped || thumbed
    }

    fn handle_modal_event(&mut self, event: &Event) -> bool {
        let Some(modal) = self.modal.as_mut() else {
            return false;
        };

        let consumed = match modal {
            Modal::Confirm { dialog, .. } | Modal::Notice { dialog } => dialog.handle_event(event),
            Modal::Rename { dialog, .. }
            | Modal::NewFolder { dialog }
            | Modal::Search { dialog } => dialog.handle_event(event),
        } == EventResult::Consumed;

        let answer = match modal {
            Modal::Confirm { dialog, .. } | Modal::Notice { dialog } => dialog.result().cloned(),
            Modal::Rename { dialog, .. }
            | Modal::NewFolder { dialog }
            | Modal::Search { dialog } => dialog.result().cloned(),
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
            Some(Modal::Search { .. }) => match answer {
                // An empty query is a dismissal that happens to have been
                // typed, the same judgement `NewFolder` makes below: running
                // it would replace the listing with nothing and report no
                // matches for an empty string as though a question had
                // been asked.
                DialogResult::Text(query) if !query.trim().is_empty() => {
                    self.run_search(query.trim());
                }
                _ => self.status_message = "Search cancelled".to_string(),
            },
            Some(Modal::Rename { target, .. }) => match answer {
                DialogResult::Text(name) => self.rename_path(&target, &name),
                _ => self.status_message = "Rename cancelled".to_string(),
            },
            Some(Modal::NewFolder { .. }) => match answer {
                // An empty name is a dismissal that happens to have been
                // typed: `create_folder("")` would report a filesystem error
                // for something the user plainly meant as "never mind".
                DialogResult::Text(name) if !name.trim().is_empty() => {
                    self.create_folder(name.trim());
                }
                _ => self.status_message = "New folder cancelled".to_string(),
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

/// A scratch directory, resolved while holding the environment lock.
///
/// `ScratchDir::new` calls `std::env::temp_dir()`, which **reads** the
/// environment, and tests in this same binary **write** it --
/// `settingsfile::testing::with_scratch_config` removes `HOME` and sets
/// `XDG_CONFIG_HOME` under `ENV_LOCK`. The environment is process-global and
/// the tests are threads, so a scratch directory could be resolved while
/// another thread was rewriting the block. Concurrent read-and-write of the
/// environment is undefined, which is why `std::env::set_var` is `unsafe` in
/// Rust 2024.
///
/// `ENV_LOCK` only serialises the tests that take it, and **a reader is the
/// side that forgets**. Every scratch directory in this crate is now resolved
/// here, so the lock is taken once and cannot be omitted by a new test that
/// copies an old one.
///
/// See known-issues `TD-C-ONE-INTERMITTENT-TEST-FAILURE-IN-THE-WORKSPACE-SUITE`,
/// whose original unidentified instance was this crate's suite failing by
/// exactly one test.
#[cfg(test)]
pub(crate) fn guarded_scratch(label: &str) -> scratchdir::ScratchDir {
    let _turn = settingsfile::testing::config_turn();
    scratchdir::ScratchDir::new(label)
}

fn main() -> std::process::ExitCode {
    // A path given on the command line is what makes "open containing folder"
    // possible from anywhere else in the desktop.
    //
    // Parsed by `Args` and handed to `launch_with`. It was read as
    // `args_os().nth(1)` and then `launch` was called -- which parses the same
    // command line, found the path left over, and refused it: "exit 2,
    // unexpected argument", before the window opened. Every "open containing
    // folder" and every association that sends a file here opened nothing.
    let args = match oswindow::app::Args::from_env() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("explorer: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut explorer = explorer_for(&args.rest, home);
    oswindow::app::launch_with("explorer", args.display.as_deref(), &mut explorer)
}

/// The window a command line asks for.
///
/// A folder named opens on that folder. A file named opens on the folder it
/// is in, with the file selected: "show in folder", and what the associations
/// that send an archive or a disk image here mean. Nothing named opens on
/// `home`, then the root. A window shows one folder, so a second path named is
/// said not to have been opened rather than dropped without a word.
fn explorer_for(paths: &[String], home: Option<PathBuf>) -> ExplorerState {
    let Some((first, rest)) = paths.split_first() else {
        return ExplorerState::new(&home.unwrap_or_else(|| PathBuf::from("/")));
    };
    let named = PathBuf::from(first);
    // Made absolute, so the path bar and the history hold where the window
    // is rather than where it was started from. It fails only when the
    // working directory cannot be read, and then the path as given is the
    // best there is: the listing names it if it cannot be read either.
    let named = std::path::absolute(&named).unwrap_or(named);
    let mut state = match (named.is_file(), named.parent(), named.file_name()) {
        (true, Some(folder), Some(name)) => {
            let mut state = ExplorerState::new(folder);
            if let Some(index) = state
                .entries
                .iter()
                .position(|e| e.path.file_name() == Some(name))
            {
                state.move_selection_to(index);
            }
            state
        }
        // A folder, or nothing there at all: the listing says which.
        _ => ExplorerState::new(&named),
    };
    if !rest.is_empty() {
        state.status_message = format!("{} more not opened: a window shows one folder", rest.len());
    }
    state
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

    /// A copy part-way through, with `secs` of work left.
    fn mid_copy(secs: f64) -> OperationProgress {
        OperationProgress {
            total_bytes: 1_000_000,
            copied_bytes: 400_000,
            total_files: 9,
            completed_files: 3,
            current_file: "/home/u/photos/holiday.png".to_string(),
            elapsed_secs: 4.0,
            eta_secs: secs,
            bytes_per_sec: 100_000,
            state: fileops::OperationState::Running,
        }
    }

    fn line_for(secs: f64) -> String {
        ExplorerState::operation_line("Copying", &mid_copy(secs), 9, 0, 0)
    }

    /// **The status bar says how much longer.**
    ///
    /// `OperationProgress::update_rates` has computed `eta_secs` on every tick
    /// since the executor was written, and the only thing that had ever read
    /// it was its own test. The user watched a bar move with no idea whether
    /// it meant ten seconds or ten minutes.
    #[test]
    fn a_running_copy_says_how_much_longer() {
        let line = line_for(125.0);
        assert!(line.contains("2m 5s left"), "{line:?}");
        assert!(
            line.contains("holiday.png"),
            "the file is still named: {line:?}"
        );
        assert!(
            line.contains("3 of 9"),
            "the count is still there: {line:?}"
        );
    }

    #[test]
    fn a_short_estimate_is_given_in_seconds() {
        assert!(line_for(12.4).contains("12s left"));
    }

    #[test]
    fn a_long_estimate_is_given_in_hours_and_minutes() {
        assert!(line_for(7_530.0).contains("2h 5m left"));
    }

    /// Zero is what `update_rates` stores before it has any throughput to
    /// divide by, so it means "not known yet", not "finished".
    #[test]
    fn an_unknown_estimate_is_not_shown_as_zero() {
        let line = line_for(0.0);
        assert!(!line.contains("left"), "{line:?}");
        assert!(
            line.contains("3 of 9"),
            "the rest of the line survives: {line:?}"
        );
    }

    /// Over a day is a number nobody acts on, and it is usually a throughput
    /// figure distorted by a slow first second rather than a real forecast.
    #[test]
    fn an_absurd_estimate_is_withheld() {
        assert!(!line_for(90_000.0).contains("left"));
    }

    /// The parenthetical about other operations stays at the end, after the
    /// estimate -- it is about the queue, not about this copy.
    #[test]
    fn the_estimate_comes_before_the_queue_count() {
        let line = ExplorerState::operation_line("Copying", &mid_copy(30.0), 9, 2, 1);
        let eta = line.find("30s left").expect("no estimate");
        let queue = line.find("(+2 running").expect("no queue count");
        assert!(eta < queue, "{line:?}");
        assert!(line.contains("1 waiting"), "{line:?}");
    }

    /// Every colour the file manager draws comes from the user's palette.
    ///
    /// The guard §822 expects a converted crate to adopt, and the one that
    /// finds what a survey of constants cannot: inline literals, and text
    /// hardcoded on a themed fill.
    ///
    /// Thumbnails are deliberately out of its reach and that is correct --
    /// `thumbs.rs` paints into a pixel buffer rather than emitting
    /// `RenderCommand`s, so its file-type colours are content in the sense
    /// `apps/paint`'s swatch row is content.
    /// The states this window can be in that draw something different.
    ///
    /// Named, because the failure message has to say which one. "explorer
    /// (light=false)" tells a reader the theme is wrong *somewhere* in a
    /// program with a toolbar, a context menu, a path bar, a transfer list and
    /// a modal, and finding which is then most of the work.
    /// One state the window can be in: a name for the failure message, and
    /// the arrangement that puts the window into it.
    type PaletteState = (&'static str, fn(&mut ExplorerState));

    const PALETTE_STATES: [PaletteState; 9] = [
        ("a plain listing", |_| {}),
        ("one file selected", |s| s.select_single(0)),
        ("everything selected", |s| s.select_all()),
        ("a file's context menu", |s| {
            let (x, y) = row_center(s, "file000.txt");
            assert!(
                s.dropzone.find_file_row(x, y).is_some(),
                "the state meant to right-click a row did not land on one"
            );
            s.open_context_menu(x, y);
        }),
        ("empty space right-clicked", |s| {
            // Past the sidebar, below the last of five rows: a right-click on
            // no file, which opens the folder's menu rather than a file's.
            let (x, y) = row_center(s, "file000.txt");
            let below = y + ROW_H * 20.0;
            assert!(
                s.dropzone.find_file_row(x, below).is_none(),
                "the empty-space state landed on a row after all"
            );
            s.open_context_menu(x, below);
        }),
        ("a full clipboard", |s| {
            s.select_single(0);
            s.cut_selected();
        }),
        ("hidden files shown", ExplorerState::toggle_hidden),
        ("a copy in flight", |s| {
            s.select_all();
            s.copy_selected();
            s.paste();
        }),
        ("a copy just finished", |s| {
            s.select_all();
            s.copy_selected();
            s.paste();
            settle(s);
        }),
    ];

    #[test]
    fn every_colour_the_file_manager_draws_comes_from_its_palette() {
        // This rendered exactly one state until 2026-09-14: a brand-new window
        // on an *empty* directory, which is the single moment the program has
        // no rows, nothing selected, no menu open, no transfer running and a
        // toolbar with nothing to grey. Everything added to this app that day
        // -- the toolbar, the context menu, the real path bar, the transfer
        // list, the progress track, the disabled reasons -- was outside its
        // reach, and it reported success over all of it.
        //
        // The same defect, found the same week, in `check-scratch-config.py`
        // (a gate that enumerated its subjects by how they spelled a call) and
        // in `apps/benchmark`'s own version of this test (which rendered one
        // tab of six, resting, idle). A sweep over part of a program is a sweep
        // over part of a program, and its green says nothing about the rest.
        for light in [false, true] {
            for (name, arrange) in PALETTE_STATES {
                let scratch = temp_dir("palette");
                let root = scratch.dir().to_path_buf();
                dir_with_files(&root, 5);
                let mut app = state_at(&root);
                app.palette = Palette::for_mode(light);
                // Once before arranging: the drop zone learns where the rows
                // are by being drawn, so a state that right-clicks a row has
                // nothing to hit until a frame has been produced.
                let _ = app.render();
                arrange(&mut app);

                let tree = app.render();
                assert!(
                    tree.commands.len() > 20,
                    "{name}: the sweep examined {} commands, which is not a render",
                    tree.commands.len()
                );
                appearance::palette_check::assert_drawn_from(
                    &app.palette,
                    &tree.commands,
                    &[],
                    &format!("explorer, {name} (light={light})"),
                );
            }
        }
    }

    /// The states are not all the same picture.
    ///
    /// Without this, a state whose arranger quietly stopped working -- a
    /// selection that selects nothing, a menu that does not open -- would go on
    /// being swept as a duplicate of the plain listing, and the sweep would
    /// keep reporting nine states while looking at one.
    #[test]
    fn each_palette_state_draws_something_the_others_do_not() {
        let mut seen: Vec<(&str, usize)> = Vec::new();
        for (name, arrange) in PALETTE_STATES {
            let scratch = temp_dir("palette_distinct");
            let root = scratch.dir().to_path_buf();
            dir_with_files(&root, 5);
            let mut app = state_at(&root);
            let _ = app.render();
            arrange(&mut app);
            seen.push((name, app.render().commands.len()));
        }
        let plain = seen.first().map(|(_, n)| *n).unwrap_or_default();
        let differing = seen.iter().filter(|(_, n)| *n != plain).count();
        assert!(
            differing >= 5,
            "only {differing} of {} states drew a different number of commands \
             from the plain listing: {seen:?}",
            seen.len().saturating_sub(1)
        );
    }

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
    /// Run the file operation in flight to completion, as the event loop does.
    ///
    /// A bulk copy is no longer finished by the call that starts it -- it is
    /// carried out a slice at a time by the frame clock, so that the window can
    /// draw and answer a click while it runs. A test that starts one and then
    /// looks at the filesystem has to let the clock run first.
    ///
    /// Ticks rather than reaching for the executor: the path under test is the
    /// one a user takes, and a user's copy is finished by the clock. A helper
    /// that stepped the operation directly would keep passing if the tick
    /// wiring were deleted, which is precisely the fault
    /// `scripts/check-tick-wiring.py` exists for -- and the fault that left
    /// `apps/automator` unable to play back a macro for five days.
    ///
    /// Bounded, and it panics rather than looping for ever: an operation that
    /// never finishes is a bug this should report, not hang on.
    fn settle(state: &mut ExplorerState) {
        for _ in 0..100_000 {
            if !state.work_in_flight() {
                return;
            }
            let _ = state.handle_event(&Event::Tick { elapsed_ms: 16 });
        }
        panic!("the file operation never finished");
    }

    fn temp_dir(label: &str) -> ScratchDir {
        // `ScratchDir::new` calls `std::env::temp_dir()`, which READS the
        // environment -- and five tests in this binary call
        // `settingsfile::testing::with_scratch_config`, which WRITES it
        // (`remove_var("HOME")`, `set_var("XDG_CONFIG_HOME")`). The
        // environment is process-global and these tests are threads, so a
        // scratch directory could be resolved while another thread was
        // rewriting the block.
        //
        // `ENV_LOCK` only serialises the tests that take it, and a reader is
        // the side that forgets -- the same defect fixed in
        // `gui/desktop/src/icons.rs` tonight, and the likely mechanism behind
        // `TD-C-ONE-INTERMITTENT-TEST-FAILURE-IN-THE-WORKSPACE-SUITE`, whose
        // signature was this crate's own suite with exactly one failure.
        //
        // The guard only has to span the resolution: once the directory is
        // named, nothing later re-reads the environment.
        let _turn = settingsfile::testing::config_turn();
        crate::guarded_scratch(&format!("explorer_test_{label}"))
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
        // `ExplorerState::new` READS the configuration directory -- it takes
        // `preview_open`, `preview_split`, `preview_side`, the icon labels and
        // the thumbnail size from `settingsfile::load`. Five tests in this
        // binary WRITE the environment that resolves it, and
        // `the_preview_can_move_to_any_side` writes `side: bottom` into its
        // scratch config before restoring anything. Tests are threads of one
        // process, so a construction here could read that file.
        //
        // That is not hypothetical: it is the `7 -> 7` this crate's own
        // `the_preview_panel_narrows_the_grid_for_the_wheel_too` reported in
        // one workspace run and in no isolated one -- a preview on the bottom
        // divides the height, so the column count is unchanged and the test
        // that asserts it narrows looks broken.
        //
        // The turn is held only across the constructor because the values are
        // copied out of the document there; nothing later reads the
        // environment again. `settingsfile::testing`'s own doc names this
        // exact failure: "a reader that never called it would resolve
        // XDG_CONFIG_HOME in the middle of somebody else's scratch directory".
        let mut state = {
            let _turn = settingsfile::testing::config_turn();
            ExplorerState::new(dir)
        };
        state.recycle = RecycleBin::new(dir.join(".recycle"), Duration::from_secs(3600));
        state.thumb_gen =
            ThumbnailGenerator::with_disk_cache(thumbs::DiskCache::new(dir.join(".thumbs")));
        state.queue_thumbnails();
        state
    }

    /// A path on the command line: a folder opens on itself, a file on its
    /// folder with it selected, and nothing on home. It opened no window at
    /// all -- `launch` refused the path as an unexpected argument.
    #[test]
    fn a_path_on_the_command_line_opens_its_folder() {
        let scratch = temp_dir("command_line");
        let dir = scratch.dir().to_path_buf();
        fs::create_dir_all(dir.join("inner")).expect("mkdir");
        write(&dir.join("a.txt"), "a");
        write(&dir.join("b.txt"), "b");
        let text = |p: &Path| p.to_str().expect("a text path").to_owned();
        let _turn = settingsfile::testing::config_turn();

        let folder = explorer_for(&[text(&dir.join("inner"))], None);
        assert_eq!(folder.current_path, dir.join("inner"));
        assert!(folder.selected_indices.is_empty());

        let file = explorer_for(&[text(&dir.join("b.txt"))], None);
        assert_eq!(file.current_path, dir, "a file opens on its folder");
        let selected: Vec<&str> = file
            .selected_indices
            .iter()
            .map(|&i| file.entries[i].name.as_str())
            .collect();
        assert_eq!(selected, ["b.txt"], "with the file selected");

        let two = explorer_for(&[text(&dir), text(&dir.join("a.txt"))], None);
        assert_eq!(two.current_path, dir);
        assert!(
            two.status_message.contains("1 more not opened"),
            "{}",
            two.status_message
        );

        let home = explorer_for(&[], Some(dir.join("inner")));
        assert_eq!(
            home.current_path,
            dir.join("inner"),
            "nothing named opens on home"
        );
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

    /// The three view modes can all be reached.
    ///
    /// `set_view_mode` was the only writer of `view_mode` and had no caller,
    /// so this window was permanently in Details. `List` and `Icons` both
    /// render, are obeyed by the layout and the navigation step, and could
    /// never be seen.
    #[test]
    fn the_view_modes_can_be_reached() {
        let dir = temp_dir("view_modes");
        let root = dir.dir();
        write(&root.join("a.txt"), "a");
        let mut state = state_at(root);
        assert_eq!(
            state.view_mode,
            ViewMode::Details,
            "control: starts in Details"
        );

        send(&mut state, &key(Key::Num2));
        assert_eq!(state.view_mode, ViewMode::List, "2 did not select List");

        send(&mut state, &key(Key::Num3));
        assert_eq!(state.view_mode, ViewMode::Icons, "3 did not select Icons");

        send(&mut state, &key(Key::Num1));
        assert_eq!(
            state.view_mode,
            ViewMode::Details,
            "1 did not return to Details"
        );
    }

    /// And the window actually looks different in them.
    ///
    /// The field changing proves only that the field changed; Details draws a
    /// column header that the other two do not.
    #[test]
    fn a_view_mode_changes_what_is_drawn() {
        let dir = temp_dir("view_render");
        let root = dir.dir();
        write(&root.join("a.txt"), "a");
        let mut state = state_at(root);

        let details = state.render().commands.len();
        send(&mut state, &key(Key::Num3));
        let icons = state.render().commands.len();

        assert_ne!(
            details, icons,
            "Details and Icons drew the same number of commands"
        );
    }

    #[test]
    fn a_listing_longer_than_the_window_can_be_scrolled_to_its_end() {
        // The defect: the renderer drew as many rows as fit and stopped, so
        // every file past the bottom edge was unreachable -- not merely
        // awkward to reach, but not drawn and not clickable at any point.
        let dir = crate::guarded_scratch("explorer-scroll");
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
        let dir = crate::guarded_scratch("explorer-wheel");
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
        let dir = crate::guarded_scratch("explorer-wheel-ends");
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
        let dir = crate::guarded_scratch("explorer-follow");
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
        let dir = crate::guarded_scratch("explorer-icons-scroll");
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
        let dir = crate::guarded_scratch("explorer-icons-wheel");
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

    /// **Every key the shortcut list advertises is one this program answers.**
    ///
    /// The label is read by `guitk::shortcut` rather than matched against a
    /// table beside it here -- that table would be a third copy of the same
    /// fact, drifting from both the list and the handler.
    ///
    /// The property is "some reachable state answers this key", not "this key
    /// is taken right now". A window on an empty folder can select nothing,
    /// open nothing and rename nothing, so the states below put files in it.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = help_states()
                    .iter_mut()
                    .any(|(_dir, state)| state.handle_key(&stroke));
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// Windows chosen so that between them every advertised key has work.
    ///
    /// The `ScratchDir` is carried alongside each state because dropping it
    /// removes the directory the state is showing.
    fn help_states() -> Vec<(scratchdir::ScratchDir, ExplorerState)> {
        let mut out = Vec::new();

        // A folder with files, selection on the first.
        let dir = crate::guarded_scratch("explorer-help-plain");
        dir_with_files(&dir.path(""), 12);
        let mut plain = state_at(&dir.path(""));
        plain.move_selection(1);
        out.push((dir, plain));

        // ...with something on the clipboard and an operation to undo, which
        // is what `Ctrl+V` and `Ctrl+Z` each need before they will act.
        let dir = crate::guarded_scratch("explorer-help-clip");
        dir_with_files(&dir.path(""), 12);
        let mut copied = state_at(&dir.path(""));
        copied.move_selection(1);
        copied.copy_selected();
        out.push((dir, copied));

        // ...having walked into a subfolder, so `Alt+Left` has somewhere to
        // go back to; and then back out, so `Alt+Right` has somewhere to go
        // forward to. Neither key can act without the other's history, and no
        // single state holds both.
        let dir = crate::guarded_scratch("explorer-help-history");
        let sub = dir.path("into");
        std::fs::create_dir_all(&sub).expect("subfolder");
        dir_with_files(&sub, 3);
        let mut walked = state_at(&dir.path(""));
        walked.navigate_to(&sub);
        out.push((dir, walked));

        let dir2 = crate::guarded_scratch("explorer-help-forward");
        let sub2 = dir2.path("into");
        std::fs::create_dir_all(&sub2).expect("subfolder");
        dir_with_files(&sub2, 3);
        let mut forward = state_at(&dir2.path(""));
        forward.navigate_to(&sub2);
        forward.go_back_if_possible();
        out.push((dir2, forward));

        // ...and with the search panel up, the one state its Escape closes.
        let dir = crate::guarded_scratch("explorer-help-search");
        dir_with_files(&dir.path(""), 12);
        let mut searching = state_at(&dir.path(""));
        searching.open_search();
        out.push((dir, searching));

        out
    }

    /// **The shortcut list reaches the window.**
    ///
    /// The guard above reads the list against the handler; this reads it
    /// against the screen. `apps/rssreader`'s overlay drew twenty of its
    /// twenty-one rows for weeks, because its box was a third quantity
    /// agreeing with neither the list nor the handler.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let dir = crate::guarded_scratch("explorer-help-drawn");
        dir_with_files(&dir.path(""), 6);
        let mut state = state_at(&dir.path(""));
        assert!(
            !help_text(&mut state).contains("F1 or ? closes this"),
            "the list is up before anybody asked for it"
        );

        state.handle_key(&key_press(Key::F1));
        let shown = help_text(&mut state);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        state.handle_key(&key_press(Key::Escape));
        assert!(
            !help_text(&mut state).contains("F1 or ? closes this"),
            "Escape did not close it"
        );
    }

    /// Every string the window is drawing, joined.
    fn help_text(state: &mut ExplorerState) -> String {
        state
            .render()
            .commands
            .iter()
            .filter_map(|c| match c {
                guitk::render::RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// A plain press.
    fn key_press(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        }
    }

    #[test]
    fn a_scrolled_icon_cell_names_the_file_that_is_drawn_in_it() {
        // The sharpest thing that can go wrong in a scrolled grid: the cell is
        // laid out by its position on screen and identified by its index in
        // the listing, and using one number for both means the first cell
        // after a scroll claims to be the first file in the folder. A click
        // would then open the wrong file, silently.
        //
        // **Run with the preview panel shut and open**, because those are two
        // different grids -- seven columns and four at this size -- and the
        // program used to measure one and draw the other. With the panel open
        // the wheel stepped seven entries per row through a grid four wide,
        // and the top-left cell reported whichever file that arithmetic landed
        // on. This test found it *intermittently* before it took the panel in
        // hand: `preview_open` is read from persisted preferences at
        // construction, so whether the bug showed up depended on what some
        // other test had saved, in another thread, moments earlier.
        for open in [false, true] {
            let dir = crate::guarded_scratch("explorer-icons-zones");
            dir_with_files(&dir.path(""), 60);
            let mut state = state_at(&dir.path(""));
            state.view_mode = ViewMode::Icons;
            state.preview_open = open;

            let cols = state.icon_columns();
            state.viewport.scroll_by(cols as isize, state.entries.len());

            // Through the real path: render to register the zones, then click
            // the top-left cell and see which file the program thinks was hit.
            drop(state.render());
            let first_drawn = (state.viewport.first_visible() / cols) * cols;
            assert_ne!(
                first_drawn, 0,
                "the grid did not scroll with the preview {open}, so this proves nothing"
            );

            let clicked = state.click_at(state.sidebar_width + 10.0, 64.0 + 10.0);
            assert!(clicked, "the top-left cell was not clickable");
            assert_eq!(
                state.selected_indices.as_slice(),
                [first_drawn],
                "with the preview {open}, the top-left cell named file {:?}, not the one drawn in it",
                state.selected_indices
            );
        }
    }

    /// The wheel and the grid count the same columns.
    ///
    /// `icon_columns` measured the whole pane while the grid was drawn in the
    /// narrower list rect, and its own doc comment already said the two must
    /// not disagree -- nothing held them to it. The panel has to actually
    /// change the count for this to be checking anything, so it asserts that
    /// first.
    #[test]
    fn the_preview_panel_narrows_the_grid_for_the_wheel_too() {
        let dir = crate::guarded_scratch("explorer-icons-preview-cols");
        dir_with_files(&dir.path(""), 12);
        let mut state = state_at(&dir.path(""));
        state.view_mode = ViewMode::Icons;

        state.preview_open = false;
        let wide = state.icon_columns();
        state.preview_open = true;
        let narrow = state.icon_columns();
        assert!(
            narrow < wide,
            "the preview panel did not narrow the listing ({wide} -> {narrow}), so this checks nothing"
        );
        assert_eq!(
            narrow,
            ExplorerState::columns_for(state.list_rect().w),
            "the wheel counts columns across a different width than the grid"
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
        let dir = crate::guarded_scratch("explorer-sb-short");
        dir_with_files(&dir.path(""), 3);
        let state = state_at(&dir.path(""));
        assert!(state.scrollbar_track().is_none());
    }

    #[test]
    fn a_long_listing_has_one_and_the_thumb_tracks_the_view() {
        let dir = crate::guarded_scratch("explorer-sb-long");
        dir_with_files(&dir.path(""), 200);
        let mut state = state_at(&dir.path(""));
        let track = state.scrollbar_track().expect("a long listing needs a bar");
        let top = state.scrollbar_thumb().expect("and a thumb").y;

        state.viewport.scroll_by(200, state.entries.len());
        let bottom = state.scrollbar_thumb().expect("still a thumb").y;

        assert!(bottom > top, "the thumb did not move with the view");
        assert!(
            bottom + state.scrollbar_thumb().unwrap().h <= track.y + track.h + 0.01,
            "the thumb ran past the end of its track"
        );
    }

    #[test]
    fn dragging_the_thumb_scrolls_the_listing() {
        let dir = crate::guarded_scratch("explorer-sb-drag");
        dir_with_files(&dir.path(""), 200);
        let mut state = state_at(&dir.path(""));
        let track = state.scrollbar_track().expect("a bar");
        let thumb = state.scrollbar_thumb().expect("a thumb");

        // Take hold of the thumb, then drag to the bottom of the track.
        assert!(press(&mut state, track.x + 2.0, thumb.y + 2.0));
        state.handle_mouse(&MouseEvent {
            x: track.x + 2.0,
            y: track.y + track.h,
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
        let dir = crate::guarded_scratch("explorer-sb-steal");
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
        let dir = crate::guarded_scratch("explorer-sb-grab");
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
        let dir = crate::guarded_scratch("explorer-sb-release");
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
        let dir = crate::guarded_scratch("explorer-sb-page");
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

    // ---- the address bar ---------------------------------------------
    //
    // It was a `String` assigned from `current_path` and never read from
    // input, drawn inside a stroked box that looks exactly like a text field.
    // `guitk::pathbar` -- 1,945 lines and 44 tests, with breadcrumbs, edit
    // mode and autocomplete -- had no users at all.

    /// Click the middle of the address bar.
    fn press_address_bar(state: &mut ExplorerState) {
        let rect = state.address_bar_rect();
        send(
            state,
            &Event::Mouse(MouseEvent {
                x: rect.x + rect.w / 2.0,
                y: rect.y + rect.h / 2.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
    }

    /// Type one character into whatever has the keyboard.
    fn type_char(state: &mut ExplorerState, ch: char) {
        send(
            state,
            &Event::Key(KeyEvent {
                key: Key::A,
                pressed: true,
                modifiers: guitk::event::Modifiers::NONE,
                text: ch.to_string(),
            }),
        );
    }

    #[test]
    fn clicking_the_address_bar_starts_editing_it() {
        let scratch = temp_dir("addr_click");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);
        assert!(!state.pathbar.is_editing());

        press_address_bar(&mut state);

        assert!(
            state.pathbar.is_editing(),
            "the address bar ignored a click on itself"
        );
    }

    /// **`Ctrl+L` reaches the widget that documents it.**
    ///
    /// The first version of the routing only fed the path bar keys while it
    /// was *already* editing -- which made `Ctrl+L` unreachable, since the
    /// only way into that mode from the keyboard is the chord the guard
    /// withheld. A shortcut a widget documents and nothing can press is the
    /// same defect as a button with no click band.
    #[test]
    fn ctrl_l_puts_the_caret_in_the_address_bar() {
        let scratch = temp_dir("addr_ctrl_l");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);

        send(&mut state, &ctrl_key(Key::L));

        assert!(
            state.pathbar.is_editing(),
            "Ctrl+L did not reach the path bar"
        );
    }

    /// Typing a folder and pressing Enter goes there.
    #[test]
    fn typing_a_path_navigates_to_it() {
        let scratch = temp_dir("addr_type");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("sub")).expect("mkdir");
        let mut state = state_at(&root);

        press_address_bar(&mut state);
        // Edit mode starts with the current path and the caret at its end, so
        // this appends. A forward slash rather than the host's separator: it
        // is what the widget's own path model uses, and every platform this
        // runs on accepts it.
        for ch in "/sub".chars() {
            type_char(&mut state, ch);
        }
        send(&mut state, &key(Key::Enter));

        assert_eq!(
            state.current_path.canonicalize().ok(),
            root.join("sub").canonicalize().ok(),
            "Enter in the address bar did not navigate"
        );
    }

    /// **A folder that is not there is said so, not navigated to.**
    ///
    /// And the typed text stays, so a mistyped path can be corrected rather
    /// than retyped.
    #[test]
    fn a_path_that_does_not_exist_is_reported_rather_than_opened() {
        let scratch = temp_dir("addr_bad");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);

        press_address_bar(&mut state);
        for ch in "/no-such-folder".chars() {
            type_char(&mut state, ch);
        }
        send(&mut state, &key(Key::Enter));

        assert_eq!(state.current_path, root, "it navigated into thin air");
        assert!(
            state.status_bar_text().contains("No such folder"),
            "nothing was said about it: {:?}",
            state.status_bar_text()
        );
    }

    /// Navigating any other way keeps the address bar in step.
    #[test]
    fn the_address_bar_follows_the_listing() {
        let scratch = temp_dir("addr_follow");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("sub")).expect("mkdir");
        let mut state = state_at(&root);

        state.navigate_to(&root.join("sub"));

        // `ends_with` on a `Path` matches a whole final component, so this is
        // stricter than the substring test it replaced: a folder named
        // `subterranean` no longer satisfies it.
        assert!(
            state.pathbar.current_path().ends_with("sub"),
            "the address bar still shows the old folder: {:?}",
            state.pathbar.current_path()
        );
    }

    /// Turning a label on puts it under the icons; turning it off removes it.
    ///
    /// Drawn text is the check, not the flag: a toggle that flips a bool the
    /// renderer ignores would pass any test of the state alone.
    #[test]
    fn icon_labels_appear_and_disappear_as_they_are_toggled() {
        settingsfile::testing::with_scratch_config("explorer-icon-labels", |_root| {
            let scratch = temp_dir("icon_labels");
            let root = scratch.dir().to_path_buf();
            write(&root.join("note.txt"), "hello");

            let mut state = state_at(&root);
            state.view_mode = ViewMode::Icons;

            // The name alone by default, which is what this view always drew.
            let drawn = texts(&icons_tree(&state));
            assert!(drawn.iter().any(|t| t == "note.txt"), "no name: {drawn:?}");

            // Size on: the same text the detail view shows for that file.
            state.activate_menu_item(MENU_ICON_LABEL_BASE + 2);
            let entry = state
                .entries
                .iter()
                .find(|e| e.name == "note.txt")
                .expect("the file is in the listing")
                .clone();
            let expected = state.entry_value(&entry, ColumnId::SIZE).display();
            let drawn = texts(&icons_tree(&state));
            assert!(
                drawn.contains(&expected),
                "the size label {expected:?} is missing from {drawn:?}"
            );

            // Name off: the pure-image wall, with the size still there.
            state.activate_menu_item(MENU_ICON_LABEL_BASE);
            let drawn = texts(&icons_tree(&state));
            assert!(
                !drawn.iter().any(|t| t == "note.txt"),
                "the name is still drawn after being turned off: {drawn:?}"
            );
            assert!(drawn.contains(&expected), "the size went too");
        });
    }

    /// The cell grows by exactly one line per label.
    #[test]
    fn the_icon_cell_grows_with_the_labels() {
        let scratch = temp_dir("icon_cell_h");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "x");
        let mut state = state_at(&root);

        state.icon_labels = columnprefs::IconLabels {
            name: false,
            date: false,
            size: false,
        };
        let bare = state.icon_cell_h();
        state.icon_labels = columnprefs::IconLabels {
            name: true,
            date: false,
            size: false,
        };
        let one = state.icon_cell_h();
        state.icon_labels = columnprefs::IconLabels {
            name: true,
            date: true,
            size: true,
        };
        let three = state.icon_cell_h();

        assert!(
            (one - bare - ICON_LABEL_LINE_H).abs() < 0.001,
            "one line: {one} vs {bare}"
        );
        assert!(
            (three - bare - 3.0 * ICON_LABEL_LINE_H).abs() < 0.001,
            "three lines: {three} vs {bare}"
        );
        // The default has to be what the view was before any of this existed,
        // or every user's icons move on upgrade for a feature they never used.
        assert!(
            (one - 108.0).abs() < 0.001,
            "the default cell changed height: {one}"
        );
    }

    /// The choice survives a fresh window.
    #[test]
    fn icon_labels_are_remembered() {
        settingsfile::testing::with_scratch_config("explorer-icon-labels-saved", |_root| {
            let scratch = temp_dir("icon_labels_saved");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            state.activate_menu_item(MENU_ICON_LABEL_BASE + 1); // date on
            state.activate_menu_item(MENU_ICON_LABEL_BASE); // name off
            let chosen = state.icon_labels;

            let again = state_at(&root);
            assert_eq!(again.icon_labels, chosen, "the labels did not survive");
        });
    }

    /// Choosing a size applies it and it survives the next window.
    ///
    /// The loop the control exists for. Applying without remembering, or
    /// remembering without applying, both look like success from inside a
    /// single test.
    #[test]
    fn a_chosen_thumbnail_size_applies_and_is_remembered() {
        settingsfile::testing::with_scratch_config("explorer-thumb-size", |_root| {
            let scratch = temp_dir("thumb_size");
            let root = scratch.dir().to_path_buf();
            fs::write(root.join("a.txt"), "x").unwrap();

            let mut state = state_at(&root);
            let before = state.thumb_config.size;
            let wanted = columnprefs::THUMB_SIZES
                .iter()
                .copied()
                .find(|s| *s != before)
                .expect("the offered sizes are not all the same");

            state.activate_menu_item(MENU_THUMB_SIZE_BASE + u64::from(wanted));
            assert_eq!(state.thumb_config.size, wanted, "the size was not applied");

            let again = state_at(&root);
            assert_eq!(
                again.thumb_config.size, wanted,
                "the chosen size did not survive a fresh window"
            );
        });
    }

    /// A size we do not offer is not adopted from a menu id.
    ///
    /// The ids are derived by adding the size to a base, so an id from
    /// anywhere else lands in the same range. It has to be checked against the
    /// list rather than trusted for being in range.
    #[test]
    fn an_unoffered_thumbnail_size_is_refused() {
        let scratch = temp_dir("thumb_size_bad");
        let root = scratch.dir().to_path_buf();
        fs::write(root.join("a.txt"), "x").unwrap();
        let mut state = state_at(&root);
        let before = state.thumb_config.size;

        assert!(
            !state.thumb_size_action(MENU_THUMB_SIZE_BASE + 300),
            "a size outside the offered list was accepted"
        );
        assert_eq!(state.thumb_config.size, before);
    }

    /// The sizes are offered only where thumbnails are drawn.
    #[test]
    fn the_size_menu_is_absent_from_a_view_without_thumbnails() {
        let scratch = temp_dir("thumb_size_view");
        let root = scratch.dir().to_path_buf();
        fs::write(root.join("a.txt"), "x").unwrap();
        let mut state = state_at(&root);

        state.view_mode = ViewMode::Details;
        let details = state.folder_menu_items();
        assert!(
            !details
                .iter()
                .any(|i| matches!(i, MenuItem::Submenu { .. })),
            "the size submenu was offered in a view that draws no thumbnails"
        );

        state.view_mode = ViewMode::Icons;
        let icons = state.folder_menu_items();
        assert!(
            icons.iter().any(|i| matches!(i, MenuItem::Submenu { .. })),
            "the size submenu was missing from the icon view"
        );
    }

    /// Ticking a column in the picker shows it; ticking it again hides it.
    #[test]
    fn the_picker_toggles_a_column() {
        let scratch = temp_dir("picker_toggle");
        let root = scratch.dir().to_path_buf();
        fs::write(root.join("a.txt"), "x").unwrap();
        let mut state = state_at(&root);

        let target = ColumnId::DATE_CREATED;
        let id = MENU_COLUMN_BASE + u64::from(target.0);
        let before = state.columns.is_visible(target);

        state.activate_menu_item(id);
        assert_ne!(
            state.columns.is_visible(target),
            before,
            "the picker did not change the column"
        );
        state.activate_menu_item(id);
        assert_eq!(
            state.columns.is_visible(target),
            before,
            "ticking twice did not return to where it started"
        );
    }

    /// The last column cannot be turned off.
    ///
    /// A header row with nothing in it leaves a list nobody can read, and the
    /// picker is the only way to reach that state.
    #[test]
    fn the_picker_will_not_empty_the_header_row() {
        let scratch = temp_dir("picker_last");
        let root = scratch.dir().to_path_buf();
        fs::write(root.join("a.txt"), "x").unwrap();
        let mut state = state_at(&root);

        state.columns.set_columns(vec![ColumnId::NAME]);
        state.activate_menu_item(MENU_COLUMN_BASE + u64::from(ColumnId::NAME.0));
        assert_eq!(
            state.columns.visible_keys(),
            vec!["name"],
            "the last column was removed"
        );
        assert!(
            state.status_message.contains("at least one")
                || state.status_message.contains("At least one"),
            "no reason was given: {}",
            state.status_message
        );
    }

    /// Choose columns, save them for the folder, come back: they are there.
    ///
    /// The loop the feature exists for. Each half is tested on its own above,
    /// and neither proves the picker's save is the thing the next visit reads.
    #[test]
    fn columns_saved_from_the_picker_come_back_on_the_next_visit() {
        settingsfile::testing::with_scratch_config("explorer-picker-save", |_root| {
            let scratch = temp_dir("picker_save");
            let root = scratch.dir().to_path_buf();
            fs::write(root.join("a.txt"), "x").unwrap();

            let mut state = state_at(&root);
            state
                .columns
                .set_columns(vec![ColumnId::NAME, ColumnId::SIZE]);
            state.activate_menu_item(MENU_COLUMNS_SAVE_FOLDER);
            assert!(
                state.status_message.contains("saved"),
                "the save said nothing: {}",
                state.status_message
            );

            // A fresh window, which re-reads the settings file.
            let again = state_at(&root);
            assert_eq!(
                again.columns.visible_keys(),
                vec!["name", "size"],
                "the saved columns did not come back"
            );
        });
    }

    /// A column set saved for a folder is what that folder shows.
    ///
    /// The whole point of the preference, and the thing a wiring change can
    /// break without any unit test noticing: the module round-trips its keys,
    /// the manager applies them, and neither proves the explorer ever asks.
    #[test]
    fn a_folder_shows_the_columns_saved_for_it() {
        settingsfile::testing::with_scratch_config("explorer-saved-columns", |_root| {
            let scratch = temp_dir("saved_columns");
            let root = scratch.dir().to_path_buf();
            fs::write(root.join("a.txt"), "x").unwrap();

            // Saved before the state exists, because the preferences are read
            // once when it is built rather than on every listing.
            let mut doc = settingsfile::load(columnprefs::CONFIG_NAME);
            assert!(columnprefs::set_for_folder(
                &mut doc,
                &root,
                &["size", "name"]
            ));
            settingsfile::store(columnprefs::CONFIG_NAME, &doc)
                .expect("the scratch config is writable");

            let state = state_at(&root);
            assert_eq!(
                state.columns.visible_keys(),
                vec!["size", "name"],
                "the folder's saved columns were not applied, or not in order"
            );
        });
    }

    /// With nothing saved for the folder, the saved default is used.
    #[test]
    fn a_folder_with_no_preference_falls_back_to_the_default() {
        settingsfile::testing::with_scratch_config("explorer-default-columns", |_root| {
            let scratch = temp_dir("default_columns");
            let root = scratch.dir().to_path_buf();
            fs::write(root.join("a.txt"), "x").unwrap();

            let mut doc = settingsfile::load(columnprefs::CONFIG_NAME);
            columnprefs::set_global(&mut doc, &["name", "date_modified"]);
            settingsfile::store(columnprefs::CONFIG_NAME, &doc)
                .expect("the scratch config is writable");

            let state = state_at(&root);
            assert_eq!(state.columns.visible_keys(), vec!["name", "date_modified"]);
        });
    }

    /// A name the address bar cannot represent is skipped, not mangled.
    ///
    /// Windows-only because that is where a non-UTF-8 filename is
    /// constructible in a test. The defect is not platform-specific:
    /// `to_string_lossy` would offer a completion with U+FFFD where the real
    /// bytes are, and choosing it would report "No such folder" for a folder
    /// the user can see in the listing.
    #[cfg(windows)]
    #[test]
    fn a_name_that_is_not_utf8_is_not_offered_as_a_completion() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        let scratch = temp_dir("completion_nonutf8");
        let root = scratch.dir().to_path_buf();
        // An unpaired surrogate: a legal Windows filename with no UTF-8 form.
        let bad = root.join(OsString::from_wide(&[0x0041_u16, 0xD800]));
        std::fs::create_dir(&bad).expect("the scratch directory is writable");
        std::fs::create_dir(root.join("Alpha")).expect("writable");

        let prefix = format!("{}/A", root.to_str().expect("scratch path is UTF-8"));
        let names: Vec<String> = ExplorerState::completions_for(&prefix)
            .into_iter()
            .map(|c| c.name)
            .collect();

        assert!(
            names.contains(&String::from("Alpha")),
            "the representable sibling was not offered: {names:?}"
        );
        for name in &names {
            assert!(
                !name.contains(char::REPLACEMENT_CHARACTER),
                "offered a mangled name: {name:?}"
            );
        }
    }

    /// Completions are read off the disk, sorted, and filtered by the prefix.
    #[test]
    fn completions_come_from_the_filesystem_in_a_stable_order() {
        let scratch = temp_dir("addr_complete");
        let root = scratch.dir().to_path_buf();
        for name in ["apples", "apricots", "bananas"] {
            fs::create_dir(root.join(name)).expect("mkdir");
        }
        write(&root.join("apple.txt"), "x");

        let prefix = format!("{}/ap", root.to_string_lossy().replace('\\', "/"));
        let items = ExplorerState::completions_for(&prefix);

        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["apple.txt", "apples", "apricots"],
            "the completions are wrong or out of order"
        );
        assert!(!items[0].is_directory, "apple.txt is not a folder");
        assert!(items[1].is_directory, "apples is");
    }

    // ---- opening a file ------------------------------------------------

    /// **A double-click starts the program the user chose for that file type.**
    ///
    /// Until 2026-09-14 this set a status line reading "Opening: name" and did
    /// nothing else -- the comment in its place said "in a real implementation,
    /// launch the associated application". A file manager that cannot open a
    /// file is most of a file manager missing.
    ///
    /// The association names a program that is not installed, deliberately:
    /// the point under test is that the *choice was read and used*, and a test
    /// that really started a program would be a test that starts programs.
    #[test]
    fn opening_a_file_starts_what_the_user_chose_for_it() {
        settingsfile::testing::with_scratch_config("explorer-open-assoc", |_root| {
            let mut doc = yamldoc::Document::new();
            // Through the shared constants, because what this test is about is
            // that the file manager obeys the association -- not what the file
            // is called. Spelled out here, a rename would leave this writing one
            // file while the code read another.
            doc.set_str(
                &[associations::ASSOCIATIONS, "txt"],
                "/nowhere/chosen-editor",
            );
            settingsfile::store(associations::CONFIG_NAME, &doc)
                .expect("scratch config is writable");

            let scratch = temp_dir("open_assoc");
            let root = scratch.dir().to_path_buf();
            write(&root.join("notes.txt"), "hello");
            let mut state = state_at(&root);
            let index = state
                .entries
                .iter()
                .position(|e| e.name == "notes.txt")
                .expect("the file is in the listing");

            state.open_entry(index);

            assert!(
                state.status_message.contains("chosen-editor"),
                "the association was not used: {:?}",
                state.status_message
            );
        });
    }

    /// A type nobody has chosen a program for says so, rather than pretending.
    #[test]
    fn opening_a_file_with_no_association_says_so() {
        settingsfile::testing::with_scratch_config("explorer-open-none", |_root| {
            let scratch = temp_dir("open_none");
            let root = scratch.dir().to_path_buf();
            write(&root.join("mystery.zzz"), "x");
            let mut state = state_at(&root);
            let index = state
                .entries
                .iter()
                .position(|e| e.name == "mystery.zzz")
                .expect("the file is in the listing");

            state.open_entry(index);

            assert!(
                state.status_message.contains("Nothing is set to open"),
                "status was {:?}",
                state.status_message
            );
        });
    }

    /// A directory still navigates rather than launching anything.
    #[test]
    fn opening_a_directory_navigates_into_it() {
        let scratch = temp_dir("open_dir");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("sub")).expect("create dir");
        let mut state = state_at(&root);
        let index = state
            .entries
            .iter()
            .position(|e| e.name == "sub")
            .expect("the directory is in the listing");

        state.open_entry(index);

        assert_eq!(state.current_path, root.join("sub"));
    }

    // ---- why a button is greyed ---------------------------------------

    /// Move the pointer to a point.
    fn hover(state: &mut ExplorerState, x: f32, y: f32) {
        send(
            state,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Move,
            }),
        );
    }

    /// The middle of a toolbar button.
    fn toolbar_centre(button: ToolbarButton) -> (f32, f32) {
        let (_, rect) = ExplorerState::toolbar_layout()
            .into_iter()
            .find(|(b, _)| *b == button)
            .unwrap_or_else(|| panic!("{button:?} is not in the layout"));
        (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
    }

    /// **A greyed button says why**, which a bare `bool` could not.
    #[test]
    fn a_disabled_button_carries_its_reason() {
        let scratch = temp_dir("why_reason");
        let root = scratch.dir().to_path_buf();
        let state = state_at(&root);

        let back = state.toolbar_button_state(ToolbarButton::Back);
        assert!(back.is_disabled(), "there is history to go back to");
        assert_eq!(back.reason(), Some("Nothing to go back to"));

        let paste = state.toolbar_button_state(ToolbarButton::Paste);
        assert_eq!(paste.reason(), Some("The clipboard is empty"));

        // And a live one carries none, rather than a reason nobody should see.
        let new_folder = state.toolbar_button_state(ToolbarButton::NewFolder);
        assert!(new_folder.is_enabled());
        assert_eq!(new_folder.reason(), None);
    }

    /// Resting the pointer on it puts the reason in the status bar.
    #[test]
    fn hovering_a_greyed_button_says_why_in_the_status_bar() {
        let scratch = temp_dir("why_hover");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);
        let (cx, cy) = toolbar_centre(ToolbarButton::Back);

        hover(&mut state, cx, cy);
        assert_eq!(state.status_bar_text(), "Nothing to go back to");

        // And it goes away again when the pointer does.
        hover(&mut state, cx, cy + 200.0);
        assert_ne!(state.status_bar_text(), "Nothing to go back to");
    }

    /// A live button says nothing on hover.
    #[test]
    fn hovering_a_live_button_says_nothing() {
        let scratch = temp_dir("why_live");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);
        let before = state.status_bar_text().to_string();
        let (cx, cy) = toolbar_centre(ToolbarButton::NewFolder);

        hover(&mut state, cx, cy);

        assert_eq!(
            state.status_bar_text(),
            before,
            "a live button explained itself"
        );
    }

    /// **A hover never covers a running copy.**
    ///
    /// The progress line is the most important thing on the bar while it is
    /// there, and a pointer wandering past a greyed button must not take it
    /// away.
    #[test]
    fn a_hover_does_not_cover_a_running_operation() {
        let scratch = temp_dir("why_busy");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.paste();
        let (cx, cy) = toolbar_centre(ToolbarButton::Back);

        hover(&mut state, cx, cy);

        assert!(
            state.status_bar_text().starts_with("Pasted"),
            "the hover covered the progress: {:?}",
            state.status_bar_text()
        );
        settle(&mut state);
    }

    /// **And a hover does outrank a finished operation's summary**, which the
    /// user has already read, while the summary still outlives the pointer.
    #[test]
    fn a_hover_outranks_a_summary_but_does_not_erase_it() {
        let scratch = temp_dir("why_after");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 2);
        state.paste();
        settle(&mut state);
        let summary = state.status_bar_text().to_string();
        assert!(!summary.is_empty());

        let (cx, cy) = toolbar_centre(ToolbarButton::Back);
        hover(&mut state, cx, cy);
        assert_eq!(state.status_bar_text(), "Nothing to go back to");

        // Off the button again: the summary is still there.
        hover(&mut state, cx, cy + 200.0);
        assert_eq!(
            state.status_bar_text(),
            summary,
            "the hover ate the summary"
        );
    }

    // ---- the context menu ---------------------------------------------
    //
    // The file explorer had no right-click handling at all: not a menu, not a
    // `MouseButton::Right` arm, nothing. Every operation the menu offers
    // already existed and was reachable only from the keyboard.

    /// Right-click at a point.
    fn right_click(state: &mut ExplorerState, x: f32, y: f32) {
        send(
            state,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Right),
            }),
        );
    }

    /// A state showing one file, with the listing laid out.
    fn one_file(root: &Path, name: &str) -> ExplorerState {
        write(&root.join(name), "contents");
        let mut state = state_at(root);
        let _ = state.render();
        state
    }

    /// A point on the row showing `name`.
    ///
    /// Found by asking the drop zones, which are what a click is resolved
    /// against, rather than by recomputing the row height here -- a helper
    /// that derived the geometry itself would agree with a renderer that had
    /// drifted from the zones and prove nothing.
    fn row_centre(state: &ExplorerState, name: &str) -> (f32, f32) {
        let index = state
            .entries
            .iter()
            .position(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} is not listed"));
        let x = state.sidebar_width + 40.0;
        let mut y = 64.0;
        while y < state.window_height as f32 {
            if state.dropzone.find_file_row(x, y) == Some(index) {
                return (x, y);
            }
            y += 2.0;
        }
        panic!("{name} has no row the drop zones know about");
    }

    #[test]
    fn right_clicking_a_file_opens_a_menu_about_that_file() {
        let scratch = temp_dir("menu_file");
        let root = scratch.dir().to_path_buf();
        let mut state = one_file(&root, "notes.txt");
        let (cx, cy) = row_centre(&state, "notes.txt");

        right_click(&mut state, cx, cy);

        let drawn = format!("{:?}", state.render_menu());
        for label in ["Open", "Cut", "Copy", "Rename", "Move to recycle bin"] {
            assert!(drawn.contains(label), "the menu has no {label:?}: {drawn}");
        }
        assert!(
            !drawn.contains("New folder"),
            "a file's menu offered to make a folder"
        );
    }

    /// **And it selects the row it was opened on**, so "Copy" means the file
    /// under the pointer rather than whatever was selected before.
    #[test]
    fn right_clicking_a_file_selects_it_first() {
        let scratch = temp_dir("menu_select");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "a");
        write(&root.join("b.txt"), "b");
        let mut state = state_at(&root);
        let _ = state.render();
        let (ax, ay) = row_centre(&state, "a.txt");
        send(
            &mut state,
            &Event::Mouse(MouseEvent {
                x: ax,
                y: ay,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        let (bx, by) = row_centre(&state, "b.txt");

        right_click(&mut state, bx, by);

        let selected: Vec<&str> = state
            .selected_indices
            .iter()
            .filter_map(|i| state.entries.get(*i))
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(selected, vec!["b.txt"], "the menu is about the wrong file");
    }

    /// Empty space gets the folder's menu instead.
    #[test]
    fn right_clicking_empty_space_opens_the_folders_menu() {
        let scratch = temp_dir("menu_folder");
        let root = scratch.dir().to_path_buf();
        let mut state = one_file(&root, "notes.txt");
        let (x, y) = empty_space(&state);

        right_click(&mut state, x, y);

        let drawn = format!("{:?}", state.render_menu());
        assert!(
            drawn.contains("New folder"),
            "no way to make a folder: {drawn}"
        );
        assert!(drawn.contains("Refresh"), "no way to refresh: {drawn}");
        assert!(!drawn.contains("Rename"), "a folder's menu offered Rename");
    }

    /// **Paste is greyed when there is nothing to paste**, not hidden.
    ///
    /// A menu whose rows come and go moves the others under the pointer
    /// between one opening and the next.
    #[test]
    fn paste_is_offered_greyed_rather_than_withheld() {
        let scratch = temp_dir("menu_paste");
        let root = scratch.dir().to_path_buf();
        let mut state = one_file(&root, "notes.txt");

        let empty = state.folder_menu_items();
        state.clipboard = Some(ClipboardOp::Copy(vec![root.join("notes.txt")]));
        let full = state.folder_menu_items();

        assert_eq!(
            empty.len(),
            full.len(),
            "the row count changed with the clipboard"
        );
        let enabled = |items: &[MenuItem]| {
            items.iter().find_map(|item| match item {
                MenuItem::Action { label, enabled, .. } if label == "Paste" => Some(*enabled),
                _ => None,
            })
        };
        assert_eq!(
            enabled(&empty),
            Some(false),
            "Paste was live with nothing to paste"
        );
        assert_eq!(
            enabled(&full),
            Some(true),
            "Paste stayed dead with a full clipboard"
        );
    }

    /// Choosing a row does the thing.
    #[test]
    fn choosing_copy_from_the_menu_fills_the_clipboard() {
        let scratch = temp_dir("menu_copy");
        let root = scratch.dir().to_path_buf();
        let mut state = one_file(&root, "notes.txt");
        let (cx, cy) = row_centre(&state, "notes.txt");
        right_click(&mut state, cx, cy);
        assert!(state.clipboard.is_none());

        state.activate_menu_item(MENU_COPY);

        assert!(state.clipboard.is_some(), "Copy did not fill the clipboard");
    }

    /// **A press while the menu is open never reaches what is under it.**
    #[test]
    fn a_press_with_the_menu_open_is_spent_on_the_menu() {
        let scratch = temp_dir("menu_shield");
        let root = scratch.dir().to_path_buf();
        let mut state = one_file(&root, "notes.txt");
        let (cx, cy) = row_centre(&state, "notes.txt");
        right_click(&mut state, cx, cy);

        // Far from the menu: this dismisses it and does nothing else.
        send(
            &mut state,
            &Event::Mouse(MouseEvent {
                x: 4.0,
                y: 4.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );

        assert!(
            state.render_menu().is_empty(),
            "the menu survived a press outside it"
        );
        assert_eq!(
            state.current_path, root,
            "the dismissing press also navigated"
        );
    }

    /// An unknown row id does nothing rather than doing something else.
    #[test]
    fn a_row_id_the_menu_never_put_there_does_nothing() {
        let scratch = temp_dir("menu_unknown");
        let root = scratch.dir().to_path_buf();
        let mut state = one_file(&root, "notes.txt");

        state.activate_menu_item(9_999);

        assert!(state.clipboard.is_none());
        assert!(state.modal.is_none(), "an unknown id opened a dialog");
    }

    // ---- the Transfers view -------------------------------------------
    //
    // roadmap.md 4.1: "a queued operation appears immediately in the File
    // Operations / Transfers view alongside the running ones, in state
    // Queued, saying what it is waiting for […] the user can reorder the
    // queue, cancel a queued operation before it ever starts, or override and
    // start it now."

    /// Press a named Transfers control the way a pointer does.
    fn press_transfer(state: &mut ExplorerState, want: TransferControl) {
        let (_, rect) = state
            .transfers_layout()
            .into_iter()
            .find(|(c, _)| *c == want)
            .unwrap_or_else(|| panic!("{want:?} is not in the layout"));
        send(
            state,
            &Event::Mouse(MouseEvent {
                x: rect.x + rect.w / 2.0,
                y: rect.y + rect.h / 2.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
    }

    /// A state with one operation running and `queued` more waiting.
    fn with_queue(root: &Path, queued: usize) -> ExplorerState {
        let mut state = paste_of(root, 6);
        state.paste();
        for n in 0..queued {
            let name = format!("q{n}.txt");
            write(&root.join(&name), "x");
            state.clipboard = Some(ClipboardOp::Copy(vec![root.join(&name)]));
            state.paste();
        }
        assert_eq!(state.queued_count(), queued);
        state
    }

    /// **There is nothing to see when nothing is happening.**
    #[test]
    fn the_transfers_view_is_absent_while_the_explorer_is_idle() {
        let scratch = temp_dir("tr_idle");
        let root = scratch.dir().to_path_buf();
        let state = state_at(&root);
        assert!(state.transfers_rect().is_none());
        assert!(state.transfers_layout().is_empty());
        assert!(state.transfer_labels().is_empty());
    }

    /// A queued operation appears in it, and says what it is waiting for.
    #[test]
    fn a_queued_operation_appears_and_says_what_it_waits_for() {
        let scratch = temp_dir("tr_rows");
        let root = scratch.dir().to_path_buf();
        let mut state = with_queue(&root, 1);

        let labels = state.transfer_labels();
        assert_eq!(labels.len(), 2, "one running and one waiting: {labels:?}");
        assert!(labels[0].starts_with("Pasted"), "{labels:?}");
        assert!(
            labels[1].contains("Queued") && labels[1].contains("waiting for"),
            "a queued row that does not say what it waits for: {labels:?}"
        );
        assert!(state.transfers_rect().is_some());

        settle(&mut state);
        assert!(state.transfers_rect().is_none(), "it outlived the work");
    }

    /// The queue can be reordered.
    #[test]
    fn a_waiting_operation_can_be_moved_down_the_queue() {
        let scratch = temp_dir("tr_reorder");
        let root = scratch.dir().to_path_buf();
        let mut state = with_queue(&root, 2);

        // The first waiting one is `q0.txt`; send it behind `q1.txt`.
        press_transfer(&mut state, TransferControl::MoveQueuedDown(0));
        settle(&mut state);

        // Both still ran -- reordering is about *when*, never about whether.
        assert!(root.join("dst/q0.txt").exists());
        assert!(root.join("dst/q1.txt").exists());
    }

    /// Moving past either end does nothing, and says so.
    #[test]
    fn a_waiting_operation_cannot_be_moved_off_either_end() {
        let scratch = temp_dir("tr_ends");
        let root = scratch.dir().to_path_buf();
        let mut state = with_queue(&root, 2);

        assert!(!state.move_queued(0, -1), "the first moved up");
        assert!(!state.move_queued(1, 1), "the last moved down");
        assert!(!state.move_queued(9, -1), "a row that is not there moved");
        assert!(state.move_queued(0, 1), "a real move reported nothing");

        settle(&mut state);
    }

    /// **Start-now overrides the per-drive rule**, which 4.1 calls a default
    /// rather than a prohibition.
    #[test]
    fn a_waiting_operation_can_be_started_alongside_the_one_ahead_of_it() {
        let scratch = temp_dir("tr_now");
        let root = scratch.dir().to_path_buf();
        let mut state = with_queue(&root, 1);
        assert_eq!(state.running_count(), 1);

        press_transfer(&mut state, TransferControl::StartQueuedNow(0));

        assert_eq!(state.queued_count(), 0, "it is still waiting");
        assert_eq!(
            state.running_count(),
            2,
            "it did not start alongside the one it shares a drive with"
        );
        settle(&mut state);
        assert!(root.join("dst/q0.txt").exists());
    }

    /// Cancelling a waiting one costs nothing and it never runs.
    #[test]
    fn a_waiting_operation_can_be_cancelled_from_the_view() {
        let scratch = temp_dir("tr_cancel");
        let root = scratch.dir().to_path_buf();
        let mut state = with_queue(&root, 1);

        press_transfer(&mut state, TransferControl::CancelQueued(0));

        assert_eq!(state.queued_count(), 0);
        settle(&mut state);
        assert!(
            !root.join("dst/q0.txt").exists(),
            "a cancelled operation copied something anyway"
        );
    }

    /// And a running one can be stopped without touching the rest.
    #[test]
    fn a_running_operation_can_be_stopped_from_the_view() {
        let scratch = temp_dir("tr_stop");
        let root = scratch.dir().to_path_buf();
        let mut state = with_queue(&root, 1);

        press_transfer(&mut state, TransferControl::CancelRunning(0));
        settle(&mut state);

        let copied = (0..6)
            .filter(|n| root.join(format!("dst/f{n}.txt")).exists())
            .count();
        assert!(copied < 6, "cancelling copied everything anyway");
        assert!(
            root.join("dst/q0.txt").exists(),
            "stopping one operation took the queued one with it"
        );
    }

    /// **A press inside the panel never reaches the listing under it.**
    ///
    /// The rows down there are covered, and acting on a file the user cannot
    /// see is worse than doing nothing.
    #[test]
    fn a_press_on_the_panels_bare_space_does_not_reach_the_listing() {
        let scratch = temp_dir("tr_shield");
        let root = scratch.dir().to_path_buf();
        let mut state = with_queue(&root, 1);
        state.selected_indices = vec![0];
        let panel = state.transfers_rect().expect("the view is open");

        // The left end of a row, where no button is.
        send(
            &mut state,
            &Event::Mouse(MouseEvent {
                x: panel.x + 4.0,
                y: panel.y + 4.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );

        assert_eq!(
            state.selected_indices,
            vec![0],
            "the press fell through and cleared the selection"
        );
        settle(&mut state);
    }

    /// Every control the view paints can be pressed where it is painted.
    #[test]
    fn every_transfers_button_is_pressable_where_it_is_drawn() {
        let scratch = temp_dir("tr_bands");
        let root = scratch.dir().to_path_buf();
        let mut state = with_queue(&root, 1);

        let controls = state.transfers_layout();
        assert_eq!(
            controls.len(),
            5,
            "one running button and four waiting ones: {controls:?}"
        );
        let panel = state.transfers_rect().expect("the view is open");
        for (control, rect) in controls {
            assert!(
                panel.contains(rect.x, rect.y),
                "{control:?} is drawn outside the panel"
            );
            let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            assert_eq!(
                state.transfers_control_at(cx, cy),
                Some(control),
                "{control:?} is not pressable at its own centre"
            );
        }
        settle(&mut state);
    }

    // ---- the toolbar -------------------------------------------------
    //
    // Seven buttons were painted here with nothing that could press them:
    // `click_at` knew about file rows and the sidebar and stopped there. Back,
    // forward, up, new folder, cut and paste all did nothing, and every one of
    // them looked exactly as it does now.

    /// Press the middle of a toolbar button, the way a pointer does.
    fn press_toolbar(state: &mut ExplorerState, button: ToolbarButton) {
        let (_, rect) = ExplorerState::toolbar_layout()
            .into_iter()
            .find(|(b, _)| *b == button)
            .unwrap_or_else(|| panic!("{button:?} is not in the layout"));
        send(
            state,
            &Event::Mouse(MouseEvent {
                x: rect.x + rect.w / 2.0,
                y: rect.y + rect.h / 2.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
    }

    /// **Every painted button is clickable, at the rectangle it was painted
    /// at.**
    ///
    /// The two halves come from two independent places: the faces out of the
    /// render tree, the clickability out of the hit test. Neither can move the
    /// other with it, which is the whole point -- a band that drifted a row
    /// away from its own face would satisfy a test that measured both from the
    /// layout.
    #[test]
    fn every_painted_toolbar_button_can_be_pressed_where_it_is_drawn() {
        let scratch = temp_dir("toolbar_bands");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);

        let faces: Vec<(f32, f32)> = state
            .render()
            .commands
            .iter()
            .filter_map(|c| match c {
                guitk::render::RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if (*width - TOOLBAR_BTN).abs() < 0.01
                    && (*height - TOOLBAR_BTN).abs() < 0.01 =>
                {
                    Some((*x, *y))
                }
                _ => None,
            })
            .collect();

        assert_eq!(
            faces.len(),
            ToolbarButton::ALL.len(),
            "the toolbar painted {} faces for {} buttons",
            faces.len(),
            ToolbarButton::ALL.len()
        );

        let mut reached = Vec::new();
        for (x, y) in faces {
            let centre = (x + TOOLBAR_BTN / 2.0, y + TOOLBAR_BTN / 2.0);
            let button = ExplorerState::toolbar_button_at(centre.0, centre.1)
                .unwrap_or_else(|| panic!("a button painted at {x},{y} has no click band"));
            reached.push(button);
        }
        for button in ToolbarButton::ALL {
            assert!(
                reached.contains(&button),
                "{button:?} was not reachable at any painted face"
            );
        }
    }

    #[test]
    fn the_back_button_goes_back() {
        let scratch = temp_dir("toolbar_back");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("sub")).expect("mkdir");
        let mut state = state_at(&root);
        state.navigate_to(&root.join("sub"));
        assert_eq!(state.current_path, root.join("sub"));
        assert!(state.toolbar_button_enabled(ToolbarButton::Back));

        press_toolbar(&mut state, ToolbarButton::Back);

        assert_eq!(state.current_path, root, "Back did not go back");
    }

    #[test]
    fn the_up_button_goes_up() {
        let scratch = temp_dir("toolbar_up");
        let root = scratch.dir().to_path_buf();
        fs::create_dir(root.join("sub")).expect("mkdir");
        let mut state = state_at(&root.join("sub"));

        press_toolbar(&mut state, ToolbarButton::Up);

        assert_eq!(state.current_path, root, "Up did not go up");
    }

    /// **A greyed-out button refuses the click rather than passing it on.**
    ///
    /// Returning "not handled" would send the press through to the rows
    /// underneath and clear the selection -- so pressing a disabled Cut would
    /// deselect the thing you were about to cut.
    #[test]
    fn a_disabled_toolbar_button_does_nothing_and_keeps_the_selection() {
        let scratch = temp_dir("toolbar_disabled");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "a");
        let mut state = state_at(&root);
        state.selected_indices = vec![0];
        assert!(
            !state.toolbar_button_enabled(ToolbarButton::Back),
            "there is history, so this proves nothing"
        );

        press_toolbar(&mut state, ToolbarButton::Back);

        assert_eq!(state.current_path, root, "a disabled Back navigated");
        assert_eq!(
            state.selected_indices,
            vec![0],
            "a disabled button let the click through and cleared the selection"
        );
    }

    /// New folder asks for a name, and the name is what gets made.
    #[test]
    fn the_new_folder_button_asks_and_then_makes_it() {
        let scratch = temp_dir("toolbar_mkdir");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);

        press_toolbar(&mut state, ToolbarButton::NewFolder);
        assert!(
            matches!(state.modal, Some(Modal::NewFolder { .. })),
            "New folder did not ask for a name"
        );

        let asked = state.modal.take();
        state.apply_modal_answer(asked, DialogResult::Text("Photos".to_string()));
        assert!(root.join("Photos").is_dir(), "the folder was not created");
    }

    /// An empty name is a dismissal, not a filesystem error.
    #[test]
    fn a_new_folder_with_no_name_is_cancelled_rather_than_failed() {
        let scratch = temp_dir("toolbar_mkdir_blank");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);

        press_toolbar(&mut state, ToolbarButton::NewFolder);
        let asked = state.modal.take();
        state.apply_modal_answer(asked, DialogResult::Text("   ".to_string()));

        assert!(
            state.status_bar_text().contains("cancelled"),
            "a blank name was treated as an attempt: {:?}",
            state.status_bar_text()
        );
    }

    /// Cut fills the clipboard, and then Paste is no longer greyed out.
    #[test]
    fn cut_then_paste_works_from_the_toolbar() {
        let scratch = temp_dir("toolbar_cutpaste");
        let root = scratch.dir().to_path_buf();
        write(&root.join("note.txt"), "keep me");
        fs::create_dir(root.join("sub")).expect("mkdir");
        let mut state = state_at(&root);
        select_named(&mut state, "note.txt");
        state.selected_indices = vec![
            state
                .entries
                .iter()
                .position(|e| e.name == "note.txt")
                .expect("the file is listed"),
        ];

        assert!(
            !state.toolbar_button_enabled(ToolbarButton::Paste),
            "empty clipboard"
        );
        press_toolbar(&mut state, ToolbarButton::Cut);
        assert!(
            state.toolbar_button_enabled(ToolbarButton::Paste),
            "Cut did not fill the clipboard"
        );

        state.navigate_to(&root.join("sub"));
        press_toolbar(&mut state, ToolbarButton::Paste);
        settle(&mut state);

        assert!(root.join("sub/note.txt").exists(), "Paste did not paste");
        assert!(!root.join("note.txt").exists(), "a cut left the original");
    }

    // ---- a copy that does not freeze the window ----------------------
    //
    // The engine used to run every action inside the call that started it, so
    // for the length of a copy the explorer did not repaint, did not answer a
    // click, and could not move the progress it was already computing. These
    // are about the part a user can see; `fileops`'s own tests cover the
    // stepping underneath.

    /// A folder of files to copy, and a state showing the destination.
    fn paste_of(scratch: &Path, count: usize) -> ExplorerState {
        let src = scratch.join("src");
        let dst = scratch.join("dst");
        fs::create_dir_all(&src).expect("src");
        fs::create_dir_all(&dst).expect("dst");
        let mut sources = Vec::new();
        for n in 0..count {
            let path = src.join(format!("f{n}.txt"));
            write(&path, "some content");
            sources.push(path);
        }
        let mut state = state_at(&dst);
        state.clipboard = Some(ClipboardOp::Copy(sources));
        state
    }

    /// **The operation outlives the call that started it.**
    ///
    /// Checked before any tick, which is what makes it deterministic: `paste`
    /// opens the journal and returns, so nothing has been copied yet however
    /// small the files are.
    #[test]
    fn a_paste_starts_the_copy_and_hands_the_window_back() {
        let scratch = temp_dir("live_paste");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);

        state.paste();

        let progress = state
            .operation_progress()
            .expect("the copy is not in flight after paste returned");
        assert_eq!(
            progress.completed_files, 0,
            "paste copied files before returning, which is the freeze"
        );
        assert!(
            state.tick_interval().is_some(),
            "nothing will ever finish this copy: the loop was given no reason to come back"
        );

        settle(&mut state);
        assert!(state.operation_progress().is_none(), "it never finished");
        assert!(
            state.tick_interval().is_none(),
            "a finished copy must not hold the desktop awake"
        );
        for n in 0..6 {
            assert!(
                root.join(format!("dst/f{n}.txt")).exists(),
                "f{n} is missing"
            );
        }
    }

    /// The window still answers while the copy runs.
    #[test]
    fn a_click_still_works_while_a_copy_is_running() {
        let scratch = temp_dir("live_click");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.paste();
        assert!(state.operation_progress().is_some(), "nothing to run");

        // An ordinary event, handled as usual: this is the whole claim.
        state.navigate_to(&root.join("src"));
        assert!(
            state.entries.iter().any(|e| e.name == "f0.txt"),
            "the explorer did not respond while a copy was in flight"
        );

        settle(&mut state);
    }

    /// The status bar says how far it has got, while it is getting there.
    #[test]
    fn the_status_bar_counts_the_copy_up_as_it_goes() {
        let scratch = temp_dir("live_status");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 5);
        state.paste();

        let started = state.status_bar_text().to_string();
        assert!(
            started.contains("Pasted 0 of 5"),
            "the status bar says nothing about the copy: {started:?}"
        );

        settle(&mut state);
        let finished = state.status_bar_text().to_string();
        assert!(
            !finished.contains("0 of 5"),
            "the status bar is still showing the start: {finished:?}"
        );
    }

    /// **A second operation waits its turn, and says that it is waiting.**
    ///
    /// It used to be refused. "No, try again later" makes the user watch for a
    /// moment nothing announces, and a paste that quietly did nothing would
    /// simply be started again -- which is what `roadmap.md` 4.1 means by a
    /// copy that is queued rather than started having to say so.
    #[test]
    fn a_second_operation_waits_its_turn_and_says_so() {
        let scratch = temp_dir("queue_second");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.paste();
        assert_eq!(
            state.queued_count(),
            0,
            "the first one runs, it does not wait"
        );

        fs::create_dir(root.join("other")).expect("other");
        write(&root.join("other/extra.txt"), "extra");
        state.clipboard = Some(ClipboardOp::Copy(vec![root.join("other/extra.txt")]));
        state.paste();

        assert_eq!(state.queued_count(), 1, "the second one was not queued");
        let message = state.status_bar_text().to_string();
        assert!(
            message.contains("(1 waiting)"),
            "the status bar does not say anything is waiting: {message:?}"
        );

        settle(&mut state);

        // Both finished, and in order.
        assert_eq!(state.queued_count(), 0, "something is still waiting");
        for n in 0..6 {
            assert!(
                root.join(format!("dst/f{n}.txt")).exists(),
                "f{n} is missing"
            );
        }
        assert!(
            root.join("dst/extra.txt").exists(),
            "the queued operation never ran"
        );
    }

    /// **A copy keeps running while a confirmation is on screen.**
    ///
    /// `handle_event` returned early for every event while a modal was up, so
    /// `Event::Tick` never reached the arm that retires a batch. A paste
    /// stopped making progress for as long as any Delete confirmation was
    /// open, and `tick_interval` asks for the frame interval precisely because
    /// a file operation is the thing the user is watching. Found by
    /// `scripts/find-swallowed-ticks.py` -- never by hand, because explorer
    /// routes through `self.modal` and no search for a dialog field reached
    /// it.
    ///
    /// The control is the first half: it proves ticks DO finish this work, so
    /// that "it finished" in the second half means something.
    #[test]
    fn a_copy_keeps_running_while_a_confirmation_is_on_screen() {
        let scratch = temp_dir("tick_behind_modal");
        let root = scratch.dir().to_path_buf();

        // The control: no modal, ticks finish the paste.
        let mut control = paste_of(&root, 6);
        control.paste();
        assert!(control.work_in_flight(), "nothing to make progress on");
        settle(&mut control);
        assert!(
            !control.work_in_flight(),
            "the control is broken: ticks do not finish this work here, so the assertion below would hold for the wrong reason"
        );

        // The case: the same paste, with a confirmation up throughout.
        let mut state = paste_of(&root, 6);
        state.paste();
        assert!(state.work_in_flight(), "nothing to make progress on");
        // Put a modal up directly rather than through `ask_delete`, which
        // returns true and opens nothing when the selection is empty -- the
        // first version of this test asserted on that return value and got a
        // pass with no modal on screen.
        let mut dialog = AlertDialog::error("Could not finish", "something");
        dialog.show();
        state.modal = Some(Modal::Notice { dialog });
        assert!(state.modal.is_some(), "no modal to test behind");

        for _ in 0..100_000 {
            if !state.work_in_flight() {
                break;
            }
            let _ = state.handle_event(&Event::Tick { elapsed_ms: 16 });
        }
        assert!(
            !state.work_in_flight(),
            "the copy stopped because a confirmation was on screen"
        );
        assert!(
            state.modal.is_some(),
            "the ticks dismissed the confirmation"
        );
    }

    /// **An operation that collides with nothing does not wait.**
    ///
    /// The other half of the rule, and the half that is hard to observe: two
    /// *different* drives cannot be arranged portably -- a machine with one
    /// disk would make the test vacuous, and a machine with two would make it
    /// pass for a reason the code does not control. An operation touching **no**
    /// drive is the case that can be arranged anywhere, and it exercises the
    /// same line: `drives_busy` answers false, so it starts alongside rather
    /// than behind. `DriveSet::shares_with` carries the rest, with its own
    /// tests in `crate::drives`.
    #[test]
    fn an_operation_that_shares_no_drive_starts_alongside_rather_than_waiting() {
        let scratch = temp_dir("admit_free");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.paste();
        assert_eq!(state.running_count(), 1);

        // A plan over no sources touches no drive at all.
        let empty = OperationPlan::plan_copy(
            &[],
            &root.join("dst"),
            ConflictPolicy::Skip,
            ErrorPolicy::SkipAndContinue,
        )
        .expect("a plan over nothing is still a plan");
        assert!(
            empty.drives().is_empty(),
            "a plan over no paths touched a drive"
        );
        assert!(state.start_operation(empty, "Pasted", false));

        assert_eq!(
            state.queued_count(),
            0,
            "an operation sharing no drive was made to wait"
        );
        assert_eq!(state.running_count(), 2, "it did not start");

        settle(&mut state);
        assert!(!state.work_in_flight());
    }

    /// And the status line names the others rather than hiding them.
    #[test]
    fn the_status_line_counts_the_other_operations_in_flight() {
        let scratch = temp_dir("admit_status");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.paste();

        let empty = OperationPlan::plan_copy(
            &[],
            &root.join("dst"),
            ConflictPolicy::Skip,
            ErrorPolicy::SkipAndContinue,
        )
        .expect("a plan over nothing is still a plan");
        state.start_operation(empty, "Pasted", false);

        let line = state.status_bar_text().to_string();
        assert!(
            line.contains("(+1 running)"),
            "the second operation is invisible: {line:?}"
        );
        settle(&mut state);
    }

    /// Three deep, and the count is right at every depth.
    #[test]
    fn the_waiting_count_is_what_is_waiting() {
        let scratch = temp_dir("queue_depth");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.paste();

        for n in 0..3 {
            let name = format!("extra{n}.txt");
            write(&root.join(&name), "x");
            state.clipboard = Some(ClipboardOp::Copy(vec![root.join(&name)]));
            state.paste();
            assert_eq!(state.queued_count(), n + 1);
        }
        assert!(state.status_bar_text().contains("(3 waiting)"));

        settle(&mut state);
        assert_eq!(state.queued_count(), 0);
        for n in 0..3 {
            assert!(
                root.join(format!("dst/extra{n}.txt")).exists(),
                "queued operation {n} never ran"
            );
        }
    }

    /// Cancelling something that has not started is free, and says whether it
    /// cancelled anything.
    #[test]
    fn a_waiting_operation_can_be_dropped_before_it_writes_anything() {
        let scratch = temp_dir("queue_cancel");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.paste();

        write(&root.join("never.txt"), "x");
        state.clipboard = Some(ClipboardOp::Copy(vec![root.join("never.txt")]));
        state.paste();
        assert_eq!(state.queued_count(), 1);

        assert!(state.cancel_queued(0), "there was one to cancel");
        assert!(!state.cancel_queued(0), "and now there is not");
        assert_eq!(state.queued_count(), 0);
        assert!(
            !state.status_bar_text().contains("waiting"),
            "the status bar still claims something is waiting: {:?}",
            state.status_bar_text()
        );

        settle(&mut state);
        assert!(
            !root.join("dst/never.txt").exists(),
            "a cancelled operation copied something anyway"
        );
    }

    /// Cancelling stops it, which is only possible because it is stepped.
    #[test]
    fn a_running_copy_can_be_cancelled() {
        let scratch = temp_dir("live_cancel");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.paste();
        assert!(state.cancel_operation(), "there was nothing to cancel");

        settle(&mut state);
        assert!(state.operation_progress().is_none());
        let copied = (0..6)
            .filter(|n| root.join(format!("dst/f{n}.txt")).exists())
            .count();
        assert!(copied < 6, "cancelling copied everything anyway");
    }

    /// **Escape reaches the cancel.**
    ///
    /// `cancel_operation` and `cancel_queued` were written with no way to
    /// press them: the ability to stop a copy existed in the model and
    /// nothing on screen reached it, which is a dead control and the same
    /// defect as a settings page for a setting nothing reads.
    #[test]
    fn escape_stops_a_running_copy() {
        let scratch = temp_dir("esc_running");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.paste();

        assert!(
            state.handle_event(&key(Key::Escape)),
            "Escape was ignored while a copy was running"
        );
        assert!(
            state.status_bar_text().starts_with("Stopping"),
            "Escape said nothing: {:?}",
            state.status_bar_text()
        );

        settle(&mut state);
        let copied = (0..6)
            .filter(|n| root.join(format!("dst/f{n}.txt")).exists())
            .count();
        assert!(copied < 6, "Escape copied everything anyway");
    }

    /// And it drops what was waiting behind it, which costs nothing.
    #[test]
    fn escape_drops_the_waiting_operations_too() {
        let scratch = temp_dir("esc_waiting");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.paste();
        write(&root.join("never.txt"), "x");
        state.clipboard = Some(ClipboardOp::Copy(vec![root.join("never.txt")]));
        state.paste();
        assert_eq!(state.queued_count(), 1);

        send(&mut state, &key(Key::Escape));

        assert_eq!(state.queued_count(), 0, "the queue survived Escape");
        assert!(
            state.status_bar_text().contains("dropped"),
            "the dropped operations were not mentioned: {:?}",
            state.status_bar_text()
        );
        settle(&mut state);
        assert!(!root.join("dst/never.txt").exists());
    }

    /// **Escape does not also clear the selection.**
    ///
    /// One key with two effects is how a user loses a selection they wanted
    /// while trying to stop a copy.
    #[test]
    fn escape_stopping_a_copy_leaves_the_selection_alone() {
        let scratch = temp_dir("esc_selection");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 6);
        state.load_directory();
        state.selected_indices = vec![0];
        let selected_before = state.selected_indices.clone();
        assert!(!selected_before.is_empty(), "nothing was selected to keep");

        state.paste();
        send(&mut state, &key(Key::Escape));

        assert_eq!(
            state.selected_indices, selected_before,
            "stopping a copy also cleared the selection"
        );
        settle(&mut state);
    }

    /// With no file work in flight, Escape means what it always meant.
    #[test]
    fn escape_still_clears_the_selection_when_nothing_is_copying() {
        let scratch = temp_dir("esc_plain");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "a");
        let mut state = state_at(&root);
        state.selected_indices = vec![0];

        assert!(state.handle_event(&key(Key::Escape)));
        assert!(
            state.selected_indices.is_empty(),
            "Escape stopped clearing the selection"
        );
        assert!(
            !state.handle_event(&key(Key::Escape)),
            "Escape with nothing to do must not claim the key"
        );
    }

    // ---- the progress track -------------------------------------------

    /// The fraction agrees with the words beside it.
    ///
    /// Both are the same claim in two forms. A bar nine tenths full next to
    /// "3 of 10" is worse than either alone: the reader has to pick one to
    /// believe and nothing on screen helps them.
    #[test]
    fn the_track_and_the_count_tell_the_same_story() {
        let scratch = temp_dir("bar_fraction");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 4);
        assert_eq!(state.operation_fraction(), None, "nothing is running");

        state.paste();
        assert_eq!(state.operation_fraction(), Some(0.0), "nothing copied yet");

        let mut seen = Vec::new();
        while state.operation_progress().is_some() {
            let _ = state.handle_event(&Event::Tick { elapsed_ms: 16 });
            if let Some(f) = state.operation_fraction() {
                seen.push(f);
            }
        }
        assert_eq!(state.operation_fraction(), None, "it never finished");
        for f in &seen {
            assert!((0.0..=1.0).contains(f), "the track ran off its end: {f}");
        }
        assert!(
            seen.windows(2).all(|w| w[1] >= w[0]),
            "the track went backwards: {seen:?}"
        );
    }

    /// A plan of no files is over the moment it starts, and says so.
    #[test]
    fn an_empty_operation_shows_a_full_track_rather_than_dividing_by_zero() {
        let scratch = temp_dir("bar_empty");
        let root = scratch.dir().to_path_buf();
        let dst = root.join("dst");
        fs::create_dir_all(&dst).expect("dst");
        fs::create_dir(root.join("empty")).expect("empty");

        let mut state = state_at(&dst);
        state.clipboard = Some(ClipboardOp::Copy(vec![root.join("empty")]));
        state.paste();

        if let Some(fraction) = state.operation_fraction() {
            assert!(
                (0.0..=1.0).contains(&fraction),
                "an empty plan produced {fraction}"
            );
        }
        settle(&mut state);
    }

    /// Every fill drawn in the status bar's row, by width.
    ///
    /// Counted by *where it is* rather than by how many fills the whole frame
    /// has: copying files changes the listing above the bar, so a frame-wide
    /// count compares two different directories and proves nothing about the
    /// track.
    fn status_bar_fills(state: &mut ExplorerState) -> Vec<f32> {
        let track_y = f32::from(u16::try_from(state.window_height).unwrap_or(u16::MAX)) - 16.0;
        state
            .render()
            .commands
            .iter()
            .filter_map(|c| match c {
                guitk::render::RenderCommand::FillRect {
                    y, width, height, ..
                } if (*y - track_y).abs() < 0.01 && (*height - 8.0).abs() < 0.01 => Some(*width),
                _ => None,
            })
            .collect()
    }

    /// The track is drawn while a copy runs, and not otherwise.
    #[test]
    fn the_status_bar_grows_a_track_only_while_something_is_copying() {
        let scratch = temp_dir("bar_drawn");
        let root = scratch.dir().to_path_buf();
        let mut state = paste_of(&root, 4);

        assert!(
            status_bar_fills(&mut state).is_empty(),
            "an idle status bar has a progress track in it"
        );

        state.paste();
        let busy = status_bar_fills(&mut state);
        assert_eq!(busy.len(), 2, "a track is a groove and a fill: {busy:?}");
        assert!(busy[1] <= busy[0], "the fill is wider than its groove");

        settle(&mut state);
        assert!(
            status_bar_fills(&mut state).is_empty(),
            "the track outlived the copy"
        );
    }

    /// **A status line too long for the window is cut and marked, not run off
    /// the edge.**
    ///
    /// It used to be drawn with no `max_width` at all. A silently clipped line
    /// reads as a short one, which is how a truncated path gets taken for the
    /// whole thing.
    #[test]
    fn a_long_status_line_is_kept_inside_the_window() {
        let scratch = temp_dir("bar_long");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);
        state.window_width = 320;
        state.status_message = "x".repeat(400);

        let drawn = format!("{:?}", state.render());
        assert!(
            drawn.contains("max_width: Some("),
            "the status line was drawn with no width to stay inside"
        );
    }

    /// And with nothing running, cancelling is a no-op that says so.
    #[test]
    fn cancelling_nothing_reports_that_there_was_nothing() {
        let scratch = temp_dir("live_cancel_none");
        let root = scratch.dir().to_path_buf();
        let mut state = state_at(&root);
        assert!(!state.cancel_operation());
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
        settle(&mut state);
        assert!(
            state.clipboard.is_some(),
            "the sources of a copy are still there, so a second paste is meaningful"
        );

        state.clipboard = Some(ClipboardOp::Cut(vec![src_dir.join("b.txt")]));
        state.paste();
        settle(&mut state);
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

    fn icons_tree(state: &ExplorerState) -> RenderTree {
        let mut tree = RenderTree::new();
        let mut zones = DropZoneManager::new(state.current_path.clone());
        state.render_icons(&mut tree, &mut zones, 0.0, 0.0, 600.0, 400.0);
        tree
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

        // Asked of the manager rather than listed here: this test is about the
        // header drawing what is active, and a hand-written list makes it a
        // test of which columns ship visible as well -- which is how it failed
        // when the default set was trimmed to the three the spec names.
        for id in state.columns.active_columns() {
            let label = state
                .columns
                .column_def(*id)
                .expect("an active column with no definition")
                .label
                .clone();
            assert!(
                drawn.contains(&label),
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

    /// A directory's Size cell is blank today, and the spec says it should not
    /// be.
    ///
    /// This said "what every file manager does", which is true and is the
    /// reasoning `roadmap-detailed.md` §4.1 explicitly considered and
    /// rejected: *"Most file managers leave this blank because computing it on
    /// every directory listing is expensive; we cache instead."* The intended
    /// behaviour is a recursive total of the contents, served from the
    /// directory-size cache.
    ///
    /// The cache is not buildable yet -- its invalidation rides on the
    /// filesystem change-notification stream, which does not exist, and its
    /// shrinking on the kernel shrinker. Both are lane A's. So the blank cell
    /// stays, and this test pins it; what changed is that the reason is now
    /// "the cache it needs is not built" rather than "this is what everyone
    /// does", because the second reads as a decision that has been made.
    /// See known-issues TD-C-THE-SIZE-CELL-AGREES-WITH-CONVENTION-AND-NOT-WITH-THE-SPEC.
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
                // The registry's own words, not the explorer's bucket label.
                // This said "Source File" while `guitk::filetypes` -- which
                // knows it is Rust -- went unread.
                ColumnValue::Text("Rust Source File".to_string()),
            ]
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
            (yc - ya - state.icon_cell_h()).abs() < 0.001,
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
                    y + index.checked_div(cols).unwrap_or(0) as f32 * state.icon_cell_h()
                        + state.icon_cell_h() / 2.0,
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
        settle(&mut state);

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
        settle(&mut state);

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
        settle(&mut state);

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
        settle(&mut state);
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
        settle(&mut state);

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
        settle(&mut state);

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

    // ======================================================================
    // Preview panel
    // ======================================================================

    /// Closed by default, and the listing has the whole pane.
    #[test]
    fn the_preview_is_closed_until_asked_for() {
        settingsfile::testing::with_scratch_config("preview-default", |_root| {
            let scratch = temp_dir("preview_default");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let state = state_at(&root);
            assert!(!state.preview_open);
            assert!(state.preview_panes().is_none());
        });
    }

    /// Opening it splits the pane, and the two halves tile it exactly.
    /// **A text file previews as text, not as a picture of text.**
    ///
    /// The thumbnailer draws a text file's first lines as a 96-pixel minimap,
    /// and `render_preview` never enlarges a thumbnail past its own pixels --
    /// deliberately, since a blown-up thumbnail reads as a fault. Together
    /// those meant selecting a text file put a tiny picture of writing in the
    /// middle of a wide pane. The same lines are now drawn as text.
    ///
    /// Asserted on the drawn commands: a `Text` command carrying a line of the
    /// file, and no `Image`. Checking only that `preview_text` was populated
    /// would pass against a renderer that still drew the minimap.
    #[test]
    fn a_text_file_previews_as_readable_text() {
        settingsfile::testing::with_scratch_config("preview-text", |_root| {
            let scratch = temp_dir("preview_text");
            let root = scratch.dir().to_path_buf();
            write(&root.join("notes.txt"), "alpha line\nbeta line\n");

            let mut state = state_at(&root);
            assert!(state.column_menu_action(MENU_PREVIEW_TOGGLE));
            let at = state
                .entries
                .iter()
                .position(|e| e.name == "notes.txt")
                .expect("the file is listed");
            state.selected_indices = vec![at];

            let tree = state.render();
            let texts: Vec<&str> = tree
                .commands
                .iter()
                .filter_map(|c| match c {
                    guitk::render::RenderCommand::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                texts.iter().any(|t| t.contains("alpha line")),
                "the preview drew no line of the file: {texts:?}"
            );
            assert!(
                !tree
                    .commands
                    .iter()
                    .any(|c| matches!(c, guitk::render::RenderCommand::Image { .. })),
                "the preview drew the thumbnail as well as the text"
            );
        });
    }

    /// Closing the pane forgets the file it was showing.
    ///
    /// The cache is keyed by path, so a stale entry would be shown again the
    /// next time the pane opened on a different selection.
    #[test]
    fn closing_the_preview_drops_what_it_was_reading() {
        settingsfile::testing::with_scratch_config("preview-text-drop", |_root| {
            let scratch = temp_dir("preview_text_drop");
            let root = scratch.dir().to_path_buf();
            write(&root.join("notes.txt"), "alpha line\n");

            let mut state = state_at(&root);
            assert!(state.column_menu_action(MENU_PREVIEW_TOGGLE));
            let at = state
                .entries
                .iter()
                .position(|e| e.name == "notes.txt")
                .expect("the file is listed");
            state.selected_indices = vec![at];
            drop(state.render());
            assert!(state.preview_text.is_some(), "nothing was read");

            assert!(state.column_menu_action(MENU_PREVIEW_TOGGLE));
            assert!(!state.preview_open);
            drop(state.render());
            assert!(
                state.preview_text.is_none(),
                "the shut pane is still holding a file open in memory"
            );
        });
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "the last pane reaching the edge exactly is the property under test"
    )]
    fn opening_the_preview_splits_the_pane() {
        settingsfile::testing::with_scratch_config("preview-split", |_root| {
            let scratch = temp_dir("preview_split");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            assert!(state.column_menu_action(MENU_PREVIEW_TOGGLE));
            assert!(state.preview_open);

            let (list, preview) = state.preview_panes().expect("a split");
            let whole = state.pane_rect();
            assert!(list.w > 0.0 && preview.w > 0.0);
            // Exact, not a tolerance: the splitter gives the last pane what
            // remains precisely so this holds to the bit, and a tolerance here
            // would pass against an implementation that had dropped that --
            // which is exactly how the splitter's own tiling test managed to
            // prove nothing until it was sabotage-checked.
            assert_eq!(
                preview.x + preview.w,
                whole.x + whole.w,
                "the preview must reach the pane's edge exactly"
            );
            assert!(
                (preview.x - (list.x + list.w) - splitter::DIVIDER).abs() < 0.01,
                "the divider belongs between them"
            );
        });
    }

    /// The choice is remembered, not just applied.
    #[test]
    fn the_preview_preference_is_written_down() {
        settingsfile::testing::with_scratch_config("preview-persist", |_root| {
            let scratch = temp_dir("preview_persist");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            assert!(state.column_menu_action(MENU_PREVIEW_TOGGLE));

            let saved = settingsfile::load(columnprefs::CONFIG_NAME);
            assert!(
                columnprefs::preview_open(&saved),
                "the toggle was applied on screen but not saved"
            );
        });
    }

    /// A pane too narrow for both keeps the listing, and keeps the preference.
    ///
    /// Shrinking the window must not silently forget that a preview was
    /// wanted: it comes back when there is room for it.
    #[test]
    fn a_pane_too_narrow_for_both_shows_the_listing() {
        settingsfile::testing::with_scratch_config("preview-narrow", |_root| {
            let scratch = temp_dir("preview_narrow");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            state.preview_open = true;
            state.window_width = 300;

            assert!(
                state.preview_panes().is_none(),
                "two unusable slivers is worse than one listing"
            );
            assert!(state.preview_open, "the preference was silently dropped");
        });
    }

    /// Dragging the divider moves it and writes the new split down.
    #[test]
    fn dragging_the_divider_resizes_and_remembers() {
        settingsfile::testing::with_scratch_config("preview-drag", |_root| {
            let scratch = temp_dir("preview_drag");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            state.preview_open = true;
            let before = state.preview_split;

            let (list, _) = state.preview_panes().expect("a split");
            let divider_x = list.x + list.w;
            state.divider_grab = Some(0.0);
            assert!(
                state.drag_divider(divider_x - 120.0, 0.0),
                "the drag did nothing"
            );
            assert!(
                state.preview_split < before,
                "the listing should have shrunk"
            );

            assert!(state.drop_divider());
            let saved = settingsfile::load(columnprefs::CONFIG_NAME);
            assert!(
                (columnprefs::preview_split(&saved) - state.preview_split).abs() < 0.02,
                "the new split was applied but not saved"
            );
        });
    }

    /// The listing is never dragged below its minimum.
    #[test]
    fn the_divider_stops_at_the_listing_minimum() {
        settingsfile::testing::with_scratch_config("preview-min", |_root| {
            let scratch = temp_dir("preview_min");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            state.preview_open = true;
            state.divider_grab = Some(0.0);
            // Aim far past the left edge of the window.
            state.drag_divider(-2_000.0, 0.0);

            let (list, _) = state.preview_panes().expect("a split");
            assert!(
                list.w >= LIST_MIN_W - 1.0,
                "the listing was squeezed to {}, below its minimum",
                list.w
            );
        });
    }

    /// A press on the divider is spent there, not on the row beneath it.
    #[test]
    fn a_press_on_the_divider_does_not_reach_the_listing() {
        settingsfile::testing::with_scratch_config("preview-press", |_root| {
            let scratch = temp_dir("preview_press");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            state.preview_open = true;
            let (list, _) = state.preview_panes().expect("a split");

            let grabbed = state.divider_grab_at(list.x + list.w, list.y + 40.0);
            assert!(grabbed.is_some(), "the divider was not grabbable");
        });
    }

    /// With the preview closed there is no divider to grab.
    #[test]
    fn there_is_no_divider_when_the_preview_is_closed() {
        settingsfile::testing::with_scratch_config("preview-nodiv", |_root| {
            let scratch = temp_dir("preview_nodiv");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let state = state_at(&root);
            let whole = state.pane_rect();
            assert_eq!(
                state.divider_grab_at(whole.x + whole.w * 0.65, whole.y + 40.0),
                None
            );
        });
    }

    /// The panel can sit on any of the four sides, and the split follows.
    #[test]
    fn the_preview_can_move_to_any_side() {
        settingsfile::testing::with_scratch_config("preview-sides", |_root| {
            let scratch = temp_dir("preview_sides");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            state.preview_open = true;

            for (i, side) in columnprefs::PreviewSide::ALL.into_iter().enumerate() {
                assert!(state.column_menu_action(MENU_PREVIEW_SIDE_BASE.saturating_add(i as u64)));
                assert_eq!(state.preview_side, side);

                let (list, preview) = state.preview_panes().expect("a split");
                match side {
                    columnprefs::PreviewSide::Left => assert!(preview.x < list.x),
                    columnprefs::PreviewSide::Right => assert!(preview.x > list.x),
                    columnprefs::PreviewSide::Top => assert!(preview.y < list.y),
                    columnprefs::PreviewSide::Bottom => assert!(preview.y > list.y),
                }
            }
        });
    }

    /// The side survives a restart.
    ///
    /// The commit that added this said the choice "is remembered" and nothing
    /// checked it. A preference applied on screen and not written down looks
    /// identical to one that was saved, right up until the next start -- which
    /// is the failure `persist_view_prefs` exists to report and no test had
    /// yet pinned for this setting.
    #[test]
    fn the_panel_side_survives_a_restart() {
        settingsfile::testing::with_scratch_config("preview-side-persist", |_root| {
            let scratch = temp_dir("preview_side_persist");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            {
                let mut state = state_at(&root);
                state.preview_open = true;
                // Bottom, because it is neither the default nor adjacent to it
                // in `ALL` -- a test that picks the default proves nothing.
                let bottom = columnprefs::PreviewSide::ALL
                    .iter()
                    .position(|s| *s == columnprefs::PreviewSide::Bottom)
                    .expect("Bottom is one of the sides");
                assert!(
                    state.column_menu_action(MENU_PREVIEW_SIDE_BASE.saturating_add(bottom as u64))
                );
                assert_eq!(state.preview_side, columnprefs::PreviewSide::Bottom);
            }

            // A second explorer, reading the same settings file.
            let restarted = state_at(&root);
            assert_eq!(
                restarted.preview_side,
                columnprefs::PreviewSide::Bottom,
                "the side was applied but not written down"
            );
        });
    }

    /// A side nobody recognises leaves the panel where it was.
    ///
    /// The settings file is meant to be hand-editable, so `side: rihgt` is a
    /// thing that will happen. Falling back to the default would move a
    /// panel the user never asked to move; `from_yaml_name` answers `None` and
    /// lets the caller keep what it had.
    #[test]
    fn an_unrecognised_side_is_not_a_silent_move() {
        assert_eq!(columnprefs::PreviewSide::from_yaml_name("rihgt"), None);
        assert_eq!(
            columnprefs::PreviewSide::from_yaml_name("bottom"),
            Some(columnprefs::PreviewSide::Bottom)
        );
        // Whitespace is forgiven, since a person typed it.
        assert_eq!(
            columnprefs::PreviewSide::from_yaml_name("  top  "),
            Some(columnprefs::PreviewSide::Top)
        );
    }

    /// Moving the panel does not resize the listing.
    ///
    /// `preview_split` is the listing's share whichever side the panel is on,
    /// so swapping sides mirrors the layout without redistributing it. Stored
    /// the other way round, moving right-to-left would hand the listing's
    /// width to the preview and look like a bug in the drag.
    #[test]
    fn moving_the_panel_keeps_the_listing_the_same_size() {
        settingsfile::testing::with_scratch_config("preview-mirror", |_root| {
            let scratch = temp_dir("preview_mirror");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            state.preview_open = true;
            state.preview_side = columnprefs::PreviewSide::Right;
            let (right_list, _) = state.preview_panes().expect("a split");

            state.preview_side = columnprefs::PreviewSide::Left;
            let (left_list, _) = state.preview_panes().expect("a split");

            assert!(
                (right_list.w - left_list.w).abs() < 0.01,
                "the listing changed width when the panel moved: {} then {}",
                right_list.w,
                left_list.w
            );
        });
    }

    /// The divider is grabbable on a vertical split too.
    #[test]
    fn the_divider_can_be_grabbed_when_the_panel_is_below() {
        settingsfile::testing::with_scratch_config("preview-vgrab", |_root| {
            let scratch = temp_dir("preview_vgrab");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            state.preview_open = true;
            state.preview_side = columnprefs::PreviewSide::Bottom;

            let d = state.preview_divider_rect().expect("a divider");
            assert!(d.w > d.h, "a horizontal divider should be wide, not tall");
            assert!(
                state.divider_grab_at(d.x + d.w / 2.0, d.y).is_some(),
                "the divider was not grabbable"
            );
        });
    }

    /// The side choice is only offered while the panel is showing.
    #[test]
    fn the_sides_are_not_offered_for_a_hidden_panel() {
        settingsfile::testing::with_scratch_config("preview-hidden-sides", |_root| {
            let scratch = temp_dir("preview_hidden_sides");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let state = state_at(&root);
            assert!(!state.preview_open);
            let labels: Vec<String> = state
                .folder_menu_items()
                .iter()
                .filter_map(|i| match i {
                    MenuItem::Action { label, .. } => Some(label.clone()),
                    _ => None,
                })
                .collect();
            assert!(
                !labels.iter().any(|l| l.contains("Preview on")),
                "a hidden panel offered a choice of where to put it: {labels:?}"
            );
        });
    }

    // ======================================================================
    // Manual order
    // ======================================================================

    /// Dragging a row to the top puts it there and keeps it there.
    #[test]
    fn a_rearranged_folder_stays_rearranged() {
        settingsfile::testing::with_scratch_config("manual-basic", |_root| {
            let scratch = temp_dir("manual_basic");
            let root = scratch.dir().to_path_buf();
            for name in ["a.txt", "b.txt", "c.txt"] {
                write(&root.join(name), "x");
            }

            let mut state = state_at(&root);
            assert_eq!(state.entries[0].name, "a.txt");

            // Move the third row (c.txt) to the front.
            assert!(state.reorder_rows(vec![2], 0));
            assert_eq!(state.entries[0].name, "c.txt");
            assert_eq!(state.sort_by, SortBy::Custom);

            // Re-listing the folder must not undo it.
            state.load_directory();
            assert_eq!(
                state.entries[0].name, "c.txt",
                "the arrangement did not survive a refresh"
            );
        });
    }

    /// A column sort overrides the arrangement without discarding it.
    ///
    /// The precedence rule `roadmap-detailed.md` §4.1 states outright, and the
    /// one an implementation that reorders `entries` in place gets wrong: it
    /// looks correct until the user sorts by a column and comes back.
    #[test]
    fn a_column_sort_overrides_the_arrangement_without_losing_it() {
        settingsfile::testing::with_scratch_config("manual-prec", |_root| {
            let scratch = temp_dir("manual_precedence");
            let root = scratch.dir().to_path_buf();
            for name in ["a.txt", "b.txt", "c.txt"] {
                write(&root.join(name), "x");
            }

            let mut state = state_at(&root);
            assert!(state.reorder_rows(vec![2], 0));
            assert_eq!(state.entries[0].name, "c.txt");

            // Sort by name: the view changes...
            state.sort_by = SortBy::Name;
            state.sort_entries();
            assert_eq!(state.entries[0].name, "a.txt");

            // ...and switching back restores the arrangement intact.
            state.sort_by = SortBy::Custom;
            state.sort_entries();
            assert_eq!(
                state.entries[0].name, "c.txt",
                "the hand arrangement was discarded by a column sort"
            );
        });
    }

    /// A file that appears later lands at the end, not in the middle.
    #[test]
    fn a_new_file_joins_the_end_of_the_arrangement() {
        settingsfile::testing::with_scratch_config("manual-new", |_root| {
            let scratch = temp_dir("manual_newcomer");
            let root = scratch.dir().to_path_buf();
            for name in ["a.txt", "b.txt"] {
                write(&root.join(name), "x");
            }

            let mut state = state_at(&root);
            assert!(state.reorder_rows(vec![1], 0));
            assert_eq!(state.entries[0].name, "b.txt");

            write(&root.join("aaa-new.txt"), "x");
            state.load_directory();

            assert_eq!(state.entries[0].name, "b.txt", "the arrangement moved");
            assert_eq!(
                state.entries.last().expect("entries").name,
                "aaa-new.txt",
                "a newcomer jumped the arrangement despite sorting first by name"
            );
        });
    }

    /// Arrangements do not leak between folders.
    #[test]
    fn each_folder_keeps_its_own_arrangement() {
        settingsfile::testing::with_scratch_config("manual-perfolder", |_root| {
            let scratch = temp_dir("manual_perfolder");
            let root = scratch.dir().to_path_buf();
            let other = root.join("other");
            fs::create_dir_all(&other).expect("mkdir");
            for name in ["a.txt", "b.txt"] {
                write(&root.join(name), "x");
                write(&other.join(name), "x");
            }

            let mut state = state_at(&root);
            // `other/` is a row too, and folders sort first, so the files are at
            // 1 and 2 rather than 0 and 1. Naming the index by what is in it
            // rather than by counting: a fixture that silently means a different
            // row than the test says is how a green test proves nothing.
            let last = state.entries.len().saturating_sub(1);
            assert_eq!(state.entries[last].name, "b.txt");
            assert!(state.reorder_rows(vec![last], 0));
            assert_eq!(state.entries[0].name, "b.txt");

            state.navigate_to(&other);
            assert_eq!(
                state.entries[0].name, "a.txt",
                "one folder's arrangement was applied to another"
            );
            assert_eq!(
                state.sort_by,
                SortBy::Name,
                "Custom stuck on a folder with no arrangement, where it means nothing"
            );
        });
    }

    /// A press that does not move is a click, not a rearrangement.
    ///
    /// The whole reason for the threshold. A file manager where an unsteady
    /// click reorders the folder does silent, persistent damage that gets
    /// blamed on something else.
    #[test]
    fn a_press_without_movement_does_not_rearrange() {
        settingsfile::testing::with_scratch_config("manual-thresh", |_root| {
            let scratch = temp_dir("manual_threshold");
            let root = scratch.dir().to_path_buf();
            for name in ["a.txt", "b.txt"] {
                write(&root.join(name), "x");
            }

            let mut state = state_at(&root);
            state.row_drag = Some(RowDrag {
                start_x: 10.0,
                start_y: 10.0,
                rows: vec![1],
                active: false,
                insert_at: 0,
            });
            // A jitter of one pixel, well inside the threshold.
            let _ = state.drag_row(11.0, 10.0);
            assert!(!state.drop_row(), "a click rearranged the folder");
            assert_eq!(state.entries[0].name, "a.txt");
            assert_eq!(state.sort_by, SortBy::Name);
        });
    }

    /// Past the threshold, the same gesture rearranges.
    ///
    /// Dropped below every row -- which is what an empty hit-test means, and
    /// is the only thing a headless test can express, since the drop zones
    /// hold the rectangles the last *frame* drew and no frame has been drawn.
    /// So this pins the append case: a file dragged past the end goes last.
    #[test]
    fn a_press_that_travels_far_enough_rearranges() {
        settingsfile::testing::with_scratch_config("manual-thresh-pass", |_root| {
            let scratch = temp_dir("manual_threshold_pass");
            let root = scratch.dir().to_path_buf();
            for name in ["a.txt", "b.txt"] {
                write(&root.join(name), "x");
            }

            let mut state = state_at(&root);
            assert_eq!(state.entries[0].name, "a.txt");
            state.row_drag = Some(RowDrag {
                start_x: 10.0,
                start_y: 10.0,
                rows: vec![0],
                active: false,
                insert_at: 0,
            });
            let _ = state.drag_row(10.0, 60.0);
            assert!(state.drop_row(), "a real drag did nothing");
            assert_eq!(
                state.entries.last().expect("entries").name,
                "a.txt",
                "a row dropped past the end did not go last"
            );
            assert_eq!(state.sort_by, SortBy::Custom);
        });
    }

    /// Several rows move together and keep their relative order.
    #[test]
    fn a_multi_row_drag_moves_them_all_in_order() {
        settingsfile::testing::with_scratch_config("manual-multi", |_root| {
            let scratch = temp_dir("manual_multi");
            let root = scratch.dir().to_path_buf();
            for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
                write(&root.join(name), "x");
            }

            let mut state = state_at(&root);
            // Move b and c (rows 1 and 2) to the front, together.
            assert!(state.reorder_rows(vec![1, 2], 0));
            let names: Vec<&str> = state.entries.iter().map(|e| e.name.as_str()).collect();
            assert_eq!(
                names,
                vec!["b.txt", "c.txt", "a.txt", "d.txt"],
                "the moved rows lost their order relative to each other"
            );
        });
    }

    /// Moving rows downward accounts for the gap they leave behind.
    ///
    /// The off-by-one this arithmetic exists for: lifting two rows out from
    /// above the target shifts the target up by two, and not adjusting drops
    /// them two places further down than the user pointed.
    #[test]
    fn rows_moved_downward_land_where_they_were_dropped() {
        settingsfile::testing::with_scratch_config("manual-down", |_root| {
            let scratch = temp_dir("manual_down");
            let root = scratch.dir().to_path_buf();
            for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
                write(&root.join(name), "x");
            }

            let mut state = state_at(&root);
            // Drop a.txt (row 0) before d.txt (row 3).
            assert!(state.reorder_rows(vec![0], 3));
            let names: Vec<&str> = state.entries.iter().map(|e| e.name.as_str()).collect();
            assert_eq!(names, vec!["b.txt", "c.txt", "a.txt", "d.txt"]);
        });
    }

    /// The line sits on the top edge of the row a drop would land before.
    #[test]
    fn the_insertion_line_marks_the_row_it_would_drop_before() {
        let scratch = temp_dir("manual_line");
        let root = scratch.dir().to_path_buf();
        for name in ["a.txt", "b.txt", "c.txt"] {
            write(&root.join(name), "x");
        }

        let mut state = state_at(&root);
        // Stand in for a frame having been drawn: the indicator reads the
        // rectangles the last render registered, which is the same source the
        // hit-testing uses.
        for (i, entry) in state.entries.iter().enumerate() {
            let y = 100.0 + (i as f32) * 20.0;
            state.dropzone.register_file_row(
                i,
                &entry.path,
                Rect::new(10.0, y, 200.0, 20.0),
                entry.is_dir,
            );
        }

        state.row_drag = Some(RowDrag {
            start_x: 10.0,
            start_y: 10.0,
            rows: vec![2],
            active: true,
            insert_at: 1,
        });
        let (y, x, w) = state.insertion_line().expect("a line while dragging");
        assert!(
            (y - 120.0).abs() < 0.01,
            "line at {y}, expected the top of row 1"
        );
        assert!((x - 10.0).abs() < 0.01);
        assert!((w - 200.0).abs() < 0.01);
    }

    /// Dropping past the end marks the bottom of the last row.
    ///
    /// "At the end" is a real target, so it gets the same feedback as any
    /// other rather than the line disappearing when the user aims there.
    #[test]
    fn the_insertion_line_marks_the_end_when_the_drop_is_past_it() {
        let scratch = temp_dir("manual_line_end");
        let root = scratch.dir().to_path_buf();
        for name in ["a.txt", "b.txt"] {
            write(&root.join(name), "x");
        }

        let mut state = state_at(&root);
        for (i, entry) in state.entries.iter().enumerate() {
            let y = 100.0 + (i as f32) * 20.0;
            state.dropzone.register_file_row(
                i,
                &entry.path,
                Rect::new(10.0, y, 200.0, 20.0),
                entry.is_dir,
            );
        }

        state.row_drag = Some(RowDrag {
            start_x: 10.0,
            start_y: 10.0,
            rows: vec![0],
            active: true,
            insert_at: 2,
        });
        let (y, _, _) = state.insertion_line().expect("a line while dragging");
        assert!(
            (y - 140.0).abs() < 0.01,
            "line at {y}, expected the bottom of row 1"
        );
    }

    /// Nothing is drawn for a press that has not become a drag.
    ///
    /// The visible half of the threshold: a line that flickered on every click
    /// would make the folder look like it was about to rearrange itself.
    #[test]
    fn no_insertion_line_before_the_threshold() {
        let scratch = temp_dir("manual_line_none");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "x");

        let mut state = state_at(&root);
        assert!(state.insertion_line().is_none(), "a line with no drag");

        state.row_drag = Some(RowDrag {
            start_x: 10.0,
            start_y: 10.0,
            rows: vec![0],
            active: false,
            insert_at: 0,
        });
        assert!(
            state.insertion_line().is_none(),
            "a line for a press that is still just a click"
        );
    }

    /// An out-of-range target is refused rather than clamped.
    #[test]
    fn an_impossible_move_is_refused() {
        settingsfile::testing::with_scratch_config("manual-range", |_root| {
            let scratch = temp_dir("manual_range");
            let root = scratch.dir().to_path_buf();
            write(&root.join("a.txt"), "x");

            let mut state = state_at(&root);
            assert!(
                !state.reorder_rows(vec![9], 0),
                "moved a row that is not there"
            );
            assert!(!state.reorder_rows(vec![0], 99), "moved a row past the end");
            assert!(!state.reorder_rows(Vec::new(), 0), "moved nothing, loudly");
        });
    }

    // ======================================================================
    // Search
    // ======================================================================

    /// A search replaces the listing with matches from below the folder.
    #[test]
    fn a_search_shows_matches_from_the_whole_subtree() {
        let scratch = temp_dir("search_subtree");
        let root = scratch.dir().to_path_buf();
        fs::create_dir_all(root.join("sub")).expect("mkdir");
        write(&root.join("alpha-report.txt"), "x");
        write(&root.join("sub/beta-report.txt"), "x");
        write(&root.join("unrelated.dat"), "x");

        let mut state = state_at(&root);
        state.run_search("report");

        assert_eq!(state.entries.len(), 2, "{:?}", state.entries);
        assert!(state.search_showing.is_some(), "the view is a search");
    }

    /// A result in a subfolder says which subfolder.
    ///
    /// Without this, two files called `notes.txt` in different folders are the
    /// same row twice to anyone reading the screen.
    #[test]
    fn a_nested_result_is_labelled_with_its_folder() {
        let scratch = temp_dir("search_label");
        let root = scratch.dir().to_path_buf();
        fs::create_dir_all(root.join("sub")).expect("mkdir");
        write(&root.join("notes.txt"), "x");
        write(&root.join("sub/notes.txt"), "x");

        let mut state = state_at(&root);
        state.run_search("notes");

        let mut labels: Vec<&str> = state.entries.iter().map(|e| e.name.as_str()).collect();
        labels.sort_unstable();
        assert_eq!(labels.len(), 2, "{labels:?}");
        assert!(
            labels.iter().any(|l| l.contains("sub")),
            "the nested result does not say where it is: {labels:?}"
        );
        assert_ne!(labels[0], labels[1], "two rows are indistinguishable");
    }

    /// Escape puts the folder back.
    #[test]
    fn escape_leaves_a_search_and_restores_the_listing() {
        let scratch = temp_dir("search_escape");
        let root = scratch.dir().to_path_buf();
        fs::create_dir_all(root.join("sub")).expect("mkdir");
        write(&root.join("only-match.txt"), "x");
        write(&root.join("sub/also-match.txt"), "x");

        let mut state = state_at(&root);
        let before = state.entries.len();
        state.run_search("match");
        assert_eq!(state.entries.len(), 2);

        state.leave_search();
        assert!(state.search_showing.is_none(), "still in search mode");
        assert_eq!(state.current_path, root, "did not return to the folder");
        assert_eq!(
            state.entries.len(),
            before,
            "the folder listing was not restored"
        );
    }

    /// Leaving a search returns to where it started, not to a result's folder.
    ///
    /// The reason `search_origin` exists at all. Opening a result navigates
    /// away; Escape after that means "stop searching", and a user who is
    /// returned to the subfolder they happened to open has lost the place they
    /// were searching from.
    #[test]
    fn leaving_a_search_returns_to_where_it_started() {
        let scratch = temp_dir("search_origin");
        let root = scratch.dir().to_path_buf();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).expect("mkdir");
        write(&sub.join("deep-match.txt"), "x");

        let mut state = state_at(&root);
        state.run_search("match");
        // Simulate opening a result, which navigates into the subfolder.
        state.navigate_to(&sub);
        assert_eq!(state.current_path, sub);

        state.leave_search();
        assert_eq!(
            state.current_path, root,
            "Escape should return to the folder the search began in"
        );
    }

    /// Refining a search keeps the original starting folder.
    #[test]
    fn refining_a_search_does_not_move_its_origin() {
        let scratch = temp_dir("search_refine");
        let root = scratch.dir().to_path_buf();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).expect("mkdir");
        write(&sub.join("aaa-bbb.txt"), "x");

        let mut state = state_at(&root);
        state.run_search("aaa");
        state.navigate_to(&sub);
        state.run_search("bbb");

        state.leave_search();
        assert_eq!(state.current_path, root, "the origin moved on refinement");
    }

    /// A search that finds nothing says so, and does not look like an empty
    /// folder.
    #[test]
    fn a_search_with_no_matches_says_so() {
        let scratch = temp_dir("search_none");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "x");

        let mut state = state_at(&root);
        state.run_search("nothing-like-this");

        assert!(state.entries.is_empty());
        assert!(
            state.status_message.contains("No matches"),
            "{}",
            state.status_message
        );
    }

    /// Leaving a search that was never started does nothing.
    ///
    /// Escape in an ordinary listing must not navigate anywhere.
    #[test]
    fn leaving_when_not_searching_is_a_no_op() {
        let scratch = temp_dir("search_noop");
        let root = scratch.dir().to_path_buf();
        write(&root.join("a.txt"), "x");

        let mut state = state_at(&root);
        let before = state.current_path.clone();
        state.leave_search();
        assert_eq!(state.current_path, before);
        assert!(state.search_showing.is_none());
    }
}

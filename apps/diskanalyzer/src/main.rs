//! diskanalyzer -- Slate OS Disk Usage Analyzer
//!
//! A visual disk space analyzer similar to WinDirStat / Baobab / SpaceSniffer.
//! Scans a directory tree, presents a squarified treemap, a sortable file list,
//! and an extension-breakdown bar chart.
//!
//! # Architecture
//!
//! ```text
//! scan::Job          -- a real filesystem walk, on its own thread
//!       |
//!       v
//! scan::Outcome      -- FileNode tree + what could not be read
//!       |
//!       v
//! DirTree            -- root node + aggregate stats
//!       |
//!       v
//! compute_treemap()  -- squarified treemap layout
//!       |
//!       v
//! DiskAnalyzerUI     -- four views (treemap / list / extensions / largest)
//! ```
//!
//! The window is [`oswindow`]'s: [`DiskAnalyzerUI`] implements
//! [`oswindow::app::App`], so the compositor drives it, and
//! [`guitk::probe::Probe`], so the tests drive it by naming controls rather
//! than measuring them.

mod scan;

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::table::{Column, Fit, Table};
use guitk::{scroll_window, text, wheel};
use oswindow::app::{self, App, Response};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COLOR_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_PEACH: Color = Color::from_hex(0xFAB387);
const COLOR_MAUVE: Color = Color::from_hex(0xCBA6F7);
const COLOR_TEAL: Color = Color::from_hex(0x94E2D5);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 960.0;
const WINDOW_HEIGHT: f32 = 700.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const BREADCRUMB_HEIGHT: f32 = 32.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const PADDING: f32 = 10.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const ROW_HEIGHT: f32 = 28.0;
const BUTTON_WIDTH: f32 = 80.0;
const BUTTON_HEIGHT: f32 = 30.0;
/// Corner rounding for something the user clicks — button, tab, text field,
/// tooltip.
const CORNER_RADIUS: f32 = 4.0;
/// Corner rounding for a mark drawn inside a row, where a control-sized radius
/// would round away most of the shape.
const CORNER_RADIUS_SMALL: f32 = 2.0;
const INPUT_WIDTH: f32 = 400.0;
const INPUT_HEIGHT: f32 = 30.0;
const BAR_CHART_ROW_HEIGHT: f32 = 24.0;
const TREEMAP_MIN_RECT: f32 = 4.0;
const TABLE_HEADER_HEIGHT: f32 = 30.0;

// ============================================================================
// File-list column geometry
// ============================================================================

/// Room the three fixed-width columns need for their *text*; [`PADDING`] is the
/// gap to the next column and is not part of it.
///
/// Two paddings are charged against Type rather than one: the last column pays
/// for the right margin as well as the gap before it, so the table's right edge
/// mirrors the `PADDING` inset its left edge has.
const SIZE_COL_WIDTH: f32 = 120.0 - PADDING;
const PERCENT_COL_WIDTH: f32 = 80.0 - PADDING;
const KIND_COL_WIDTH: f32 = 100.0 - 2.0 * PADDING;

/// The list view's columns at a given window width, left to right.
///
/// Two things were wrong with the fixed table this replaces. The header carried
/// a width per column and then threw it away — the loop was literally
/// `for (label, cx, _cw)` with `max_width: None` — so a heading was free to
/// overrun its column, and of the four body cells only Name had any bound at
/// all. And the columns ended at x=660 in a 960px window, so a third of every
/// row was blank while the one column holding variable-length data, and the
/// only one that ever got clipped, was the narrowest it could be.
///
/// Name now takes whatever the other three leave, so the table ends one
/// `PADDING` short of the window edge at *any* width rather than only at 960.
/// Below [`MIN_NAME_WIDTH`] it stops shrinking and the table is allowed to run
/// past the edge instead: a name elided to nothing says strictly less than a
/// name that is merely clipped, and the window has a minimum size anyway.
///
/// Returned by value rather than as a `&'static [Column]` because the widths
/// are computed now. [`Table`] borrows its columns, so a caller has to keep the
/// returned array alive for as long as the table built from it.
fn list_columns(width: f32) -> [Column; 4] {
    // Five gaps, not four: `Table::with_gap` puts one before every column, and
    // the last column carries one more for the right margin.
    let fixed = 5.0f32.mul_add(PADDING, SIZE_COL_WIDTH + PERCENT_COL_WIDTH + KIND_COL_WIDTH);
    [
        Column {
            label: "Name",
            width: (width - fixed).max(MIN_NAME_WIDTH),
        },
        Column {
            label: "Size",
            width: SIZE_COL_WIDTH,
        },
        Column {
            label: "%",
            width: PERCENT_COL_WIDTH,
        },
        Column {
            label: "Type",
            width: KIND_COL_WIDTH,
        },
    ]
}

const NAME_COL: usize = 0;
const SIZE_COL: usize = 1;
const PERCENT_COL: usize = 2;
const KIND_COL: usize = 3;

/// Indent added to a Name cell per level of tree depth.
const DEPTH_INDENT: f32 = 20.0;

/// Room reserved at the head of a Name cell for its expand/collapse chevron.
///
/// The chevron is drawn as its own box rather than prepended to the name.
/// Prepended, it shares the name's fate: a name cut to keep its tail — which
/// is what a file name needs — would eat the chevron first, and the chevron is
/// the only thing on the row saying whether it can be opened.
const CHEVRON_WIDTH: f32 = 14.0;

/// Least room a name keeps, however deep its row is nested.
///
/// Tree depth is data and has no bound, so `depth * DEPTH_INDENT` eventually
/// exceeds the Name column and leaves the name nothing — an elide to zero
/// width is the empty string, so deep rows would go *blank* rather than
/// merely narrow, with nothing to say a row was there at all. Past this depth
/// the indent stops growing instead.
const MIN_NAME_WIDTH: f32 = 120.0;

/// Indent for a row at `depth`, capped so the name always keeps
/// [`MIN_NAME_WIDTH`].
fn row_indent(depth: u32, name_width: f32) -> f32 {
    let room = (name_width - CHEVRON_WIDTH - MIN_NAME_WIDTH).max(0.0);
    (f32::from(u16::try_from(depth).unwrap_or(u16::MAX)) * DEPTH_INDENT).min(room)
}

// ============================================================================
// Window geometry
// ============================================================================

/// Smallest window the layout is drawn for.
///
/// Every rectangle below is computed by subtraction from the window size, and
/// subtraction is what produces a negative width. A negative width is not a
/// small rectangle — it is one whose `right()` is left of its `x`, which makes
/// `contains` false everywhere and quietly deletes a control from the hit test
/// while leaving it on screen. Clamping the size once, at the top of `frame`,
/// means no downstream helper has to think about it.
const MIN_WIDTH: f32 = 480.0;
const MIN_HEIGHT: f32 = 320.0;

/// Longest path the toolbar's text field will accept.
///
/// A key event handler that appends without a bound is a memory leak with a
/// keyboard attached to it. This is far longer than any path the filesystem
/// will accept, so it never truncates something a user could really open.
const MAX_PATH_INPUT: usize = 4096;

/// Widest a single breadcrumb button is drawn, in pixels.
const BREADCRUMB_MAX_SEGMENT: f32 = 160.0;

/// The columns a click on the header can sort by, in the order they are drawn.
const SORTABLE_COLUMNS: [SortColumn; 4] = [
    SortColumn::Name,
    SortColumn::Size,
    SortColumn::Percentage,
    SortColumn::Type,
];

/// The area between the breadcrumb bar and the status bar, inset by [`PADDING`].
///
/// Used by the treemap and the extension chart. The list view deliberately does
/// not use it — see [`list_area`].
fn content_rect(size: (f32, f32)) -> Rect {
    let (w, h) = size;
    let top = TOOLBAR_HEIGHT + BREADCRUMB_HEIGHT + PADDING;
    Rect::new(
        PADDING,
        top,
        w - 2.0 * PADDING,
        h - TOOLBAR_HEIGHT - BREADCRUMB_HEIGHT - STATUS_BAR_HEIGHT - 2.0 * PADDING,
    )
}

/// The area the file list occupies.
///
/// Unlike [`content_rect`] this is *not* inset: the list draws its own row
/// stripes edge to edge, and a row whose highlight stops one `PADDING` short of
/// the window looks like a rendering fault rather than a margin. The columns
/// carry the inset instead — see [`list_columns`].
fn list_area(size: (f32, f32)) -> Rect {
    let (w, h) = size;
    let top = TOOLBAR_HEIGHT + BREADCRUMB_HEIGHT;
    Rect::new(0.0, top, w, h - top - STATUS_BAR_HEIGHT)
}

/// Width of a view-mode tab button, sized to its label.
///
/// The four labels differ by six characters between "List" and "Extensions",
/// so one `BUTTON_WIDTH` for all of them either clips the longest or leaves the
/// shortest floating in space.
fn view_button_width(label: &str) -> f32 {
    let text = text::measure(label, FONT_SIZE, FontWeightHint::Regular);
    (text + 2.0 * PADDING).max(BUTTON_WIDTH * 0.75)
}

/// Width of one breadcrumb button, sized to its segment and capped.
///
/// The cap matters: a single directory name has no length limit, and one long
/// name would otherwise push every ancestor off the right of the bar.
fn breadcrumb_width(segment: &str) -> f32 {
    (text::measure(segment, FONT_SIZE, FontWeightHint::Regular) + PADDING)
        .min(BREADCRUMB_MAX_SEGMENT)
}

// ============================================================================
// FileKind
// ============================================================================

/// Kind of filesystem entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    RegularFile,
    Directory,
    Symlink,
    Other,
}

// ============================================================================
// FileNode
// ============================================================================

/// A node in the scanned directory tree.
///
/// `name` and `path` are deliberately different types. The name is what is
/// *drawn*, so it is a `String` and a byte sequence that is not UTF-8 becomes
/// `U+FFFD` in it — a replacement character on screen is a cosmetic defect. The
/// path is what is *opened*, and the same lossy conversion there produces a path
/// that names a different file or no file at all, which is silent data
/// corruption. The tree's rule is that paths are bytes; this is that rule.
///
/// It also fixes an identity bug that the old all-`String` version had:
/// `expanded_paths` compares paths for equality, so two distinct byte paths that
/// lossily collided would expand and collapse together.
#[derive(Clone, Debug)]
pub struct FileNode {
    /// Display name (file or directory name, not the full path).
    pub name: String,
    /// Full path from the scan root.
    pub path: PathBuf,
    /// Size in bytes (for files: file size; for directories: sum of children).
    pub size_bytes: u64,
    /// Kind of this node.
    pub kind: FileKind,
    /// Children (non-empty only for directories).
    pub children: Vec<FileNode>,
    /// Depth in the tree (root = 0).
    pub depth: u32,
}

impl FileNode {
    /// Create a new file node.
    pub fn new_file(name: &str, path: impl Into<PathBuf>, size_bytes: u64) -> Self {
        Self {
            name: name.to_string(),
            path: path.into(),
            size_bytes,
            kind: FileKind::RegularFile,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Create a new directory node.
    pub fn new_dir(name: &str, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.to_string(),
            path: path.into(),
            size_bytes: 0,
            kind: FileKind::Directory,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Create a symlink node.
    pub fn new_symlink(name: &str, path: impl Into<PathBuf>, size_bytes: u64) -> Self {
        Self {
            name: name.to_string(),
            path: path.into(),
            size_bytes,
            kind: FileKind::Symlink,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Create an "other" kind node.
    pub fn new_other(name: &str, path: impl Into<PathBuf>, size_bytes: u64) -> Self {
        Self {
            name: name.to_string(),
            path: path.into(),
            size_bytes,
            kind: FileKind::Other,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// The child directly named `name`, if there is one.
    ///
    /// This is how the breadcrumb trail re-roots the view: names are unique
    /// within one directory, so a trail of names identifies exactly one node.
    #[must_use]
    pub fn child_named(&self, name: &str) -> Option<&FileNode> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Add a child to this directory node.
    pub fn add_child(&mut self, child: FileNode) {
        self.children.push(child);
    }

    /// File extension (lowercase, without leading dot), or empty string.
    pub fn extension(&self) -> String {
        if self.kind != FileKind::RegularFile {
            return String::new();
        }
        match self.name.rsplit_once('.') {
            Some((_, ext)) if !ext.is_empty() => ext.to_lowercase(),
            _ => String::new(),
        }
    }

    /// Total number of files in this subtree (counting self if a file).
    pub fn file_count(&self) -> u64 {
        if self.kind != FileKind::Directory {
            return 1;
        }
        self.children.iter().map(|c| c.file_count()).sum()
    }

    /// Total number of directories in this subtree (counting self if a dir).
    pub fn dir_count(&self) -> u64 {
        let self_count = if self.kind == FileKind::Directory {
            1u64
        } else {
            0
        };
        let child_count: u64 = self.children.iter().map(|c| c.dir_count()).sum();
        self_count.saturating_add(child_count)
    }

    /// Whether this node is a directory.
    pub fn is_dir(&self) -> bool {
        self.kind == FileKind::Directory
    }
}

// ============================================================================
// DirTree
// ============================================================================

/// Complete result of a directory scan.
#[derive(Clone, Debug)]
pub struct DirTree {
    /// Root node of the scanned tree.
    pub root: FileNode,
    /// Total size of all files found.
    pub total_size: u64,
    /// Total number of regular files.
    pub file_count: u64,
    /// Total number of directories.
    pub dir_count: u64,
    /// Timestamp (seconds since epoch) when scan started.
    pub scan_timestamp: u64,
    /// Duration of the scan in milliseconds.
    pub scan_duration_ms: u64,
}

// ============================================================================
// Tree summarising
// ============================================================================

/// Assign depths, roll directory sizes up from their children, and count.
///
/// Named `scan_directory` until [`scan`] existed, when the name became a lie:
/// this function has never touched a filesystem, and while it was the only
/// thing called "scan" the program's entire display was computed from whatever
/// tree a caller handed it. [`scan::walk`] does the reading; this is the pass
/// that turns what it read into totals.
pub fn summarize_tree(root: &mut FileNode) -> DirTree {
    assign_depths(root, 0);
    calculate_sizes(root);
    let total_size = root.size_bytes;
    let file_count = root.file_count();
    let dir_count = root.dir_count();
    DirTree {
        root: root.clone(),
        total_size,
        file_count,
        dir_count,
        scan_timestamp: 0,
        scan_duration_ms: 0,
    }
}

/// Recursively assign depth values starting from the given level.
fn assign_depths(node: &mut FileNode, depth: u32) {
    node.depth = depth;
    for child in &mut node.children {
        assign_depths(child, depth.saturating_add(1));
    }
}

/// Propagate sizes from leaf files up to parent directories.
pub fn calculate_sizes(node: &mut FileNode) {
    if node.kind == FileKind::Directory {
        for child in &mut node.children {
            calculate_sizes(child);
        }
        node.size_bytes = node.children.iter().map(|c| c.size_bytes).sum();
    }
    // Leaf files already have their size_bytes set.
}

/// The `n` largest regular files anywhere in the tree, biggest first.
///
/// Files only, not directories. A "largest" list that included directories
/// would be led by the root, then the root's biggest child, then *its* biggest
/// child — one chain down the tree, which the treemap already draws and which
/// answers nothing the user asked. What they asked is "what should I delete",
/// and only a file can be deleted.
///
/// `whole` is the size the percentage column is measured against — the whole
/// scan, not the current directory, because a file's share of the *disk* is the
/// figure that decides whether removing it is worth doing.
#[must_use]
pub fn find_largest(node: &FileNode, n: usize, whole: u64) -> Vec<ListRow> {
    let mut files: Vec<&FileNode> = Vec::new();
    collect_files(node, &mut files);
    files.sort_by_key(|f| std::cmp::Reverse(f.size_bytes));
    files.truncate(n);
    files
        .into_iter()
        .map(|f| ListRow {
            name: f.name.clone(),
            path: f.path.clone(),
            size_bytes: f.size_bytes,
            percentage: share_of(f.size_bytes, whole),
            kind: f.kind,
            is_expanded: false,
            // Flat by construction: these rows come from all over the tree, so
            // an indent would suggest a nesting the list does not have.
            depth: 0,
            has_children: false,
        })
        .collect()
}

/// Every non-directory node in the tree.
fn collect_files<'a>(node: &'a FileNode, out: &mut Vec<&'a FileNode>) {
    if node.kind != FileKind::Directory {
        out.push(node);
    }
    for child in &node.children {
        collect_files(child, out);
    }
}

/// Append the names leading from `node` down to the directory at `target`.
///
/// `trail` arrives holding the names of the ancestors already walked and is
/// left holding the full breadcrumb trail on success, or exactly as it was
/// found on failure — a partial trail written into the breadcrumb bar would
/// point at a directory nobody asked for.
///
/// Matches on the stored path rather than on the name, because names repeat all
/// over a tree and the path is what identifies a directory. Only directories
/// match: a *file* has no inside to navigate into.
fn trail_to(node: &FileNode, target: &Path, trail: &mut Vec<String>) -> bool {
    if node.path == target {
        return node.is_dir();
    }
    for child in &node.children {
        trail.push(child.name.clone());
        if trail_to(child, target, trail) {
            return true;
        }
        trail.pop();
    }
    false
}

/// `part` as a percentage of `whole`, and zero rather than a division by zero.
fn share_of(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_possible_truncation)]
    {
        (part as f64 / whole as f64 * 100.0) as f32
    }
}

// ============================================================================
// Treemap visualization
// ============================================================================

/// A rectangle in the treemap layout.
#[derive(Clone, Debug)]
pub struct TreemapRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Index into the flat node list used during layout.
    pub node_index: usize,
    /// Depth of the node in the original tree.
    pub depth: u32,
    /// Color for this rectangle.
    pub color: Color,
    /// Path of the file/directory.
    pub path: PathBuf,
    /// Display name.
    pub name: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Whether this rectangle can be drilled into.
    pub is_dir: bool,
}

/// Compute the squarified treemap layout for a directory node.
///
/// The algorithm partitions the given rectangle proportionally by the sizes
/// of the children, choosing the layout dimension that yields the best
/// (closest to 1:1) aspect ratios.
pub fn compute_treemap(
    node: &FileNode,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Vec<TreemapRect> {
    let mut rects = Vec::new();
    if node.size_bytes == 0 || width < TREEMAP_MIN_RECT || height < TREEMAP_MIN_RECT {
        return rects;
    }

    // Collect children sorted by size (descending).
    let mut children: Vec<&FileNode> = node.children.iter().collect();
    children.sort_by_key(|c| std::cmp::Reverse(c.size_bytes));

    // Filter out zero-size entries.
    let children: Vec<&FileNode> = children.into_iter().filter(|c| c.size_bytes > 0).collect();
    if children.is_empty() {
        return rects;
    }

    let sizes: Vec<f64> = children.iter().map(|c| c.size_bytes as f64).collect();
    let total: f64 = sizes.iter().sum();

    squarify_layout(&children, &sizes, total, x, y, width, height, &mut rects);

    rects
}

/// Squarified treemap recursive layout.
#[allow(clippy::too_many_arguments)]
fn squarify_layout(
    children: &[&FileNode],
    sizes: &[f64],
    total_size: f64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    rects: &mut Vec<TreemapRect>,
) {
    if children.is_empty() || total_size <= 0.0 {
        return;
    }
    if let [child] = children {
        rects.push(TreemapRect {
            x,
            y,
            width,
            height,
            node_index: rects.len(),
            depth: child.depth,
            color: color_for_node(child),
            path: child.path.clone(),
            name: child.name.clone(),
            size_bytes: child.size_bytes,
            is_dir: child.is_dir(),
        });
        return;
    }

    // Determine layout direction: lay out along the shorter axis.
    let lay_horizontal = width >= height;
    let full_length = if lay_horizontal { width } else { height };
    let cross = if lay_horizontal { height } else { width };

    // Find the split point that gives the best aspect ratios for a row.
    let mut best_split = 1;
    let mut best_worst_aspect = f64::MAX;

    let mut row_sum = 0.0;
    for (i, &sz) in sizes.iter().enumerate().take(children.len()) {
        row_sum += sz;
        let row_fraction = row_sum / total_size;
        let row_cross = (cross as f64) * row_fraction;

        if row_cross < 0.5 {
            continue;
        }

        // `..=i` is in bounds because `i` came from `sizes.iter().enumerate()`;
        // `get` keeps the slice panic-free without an `unwrap`.
        let Some(row) = sizes.get(..=i) else { continue };
        let worst = worst_aspect_in_row(row, row_sum, full_length as f64, row_cross);
        if worst < best_worst_aspect {
            best_worst_aspect = worst;
            best_split = i.saturating_add(1);
        } else if i > 0 {
            // Aspect getting worse — stop searching.
            break;
        }
    }

    // Lay out the first `best_split` items in a row. `best_split` is always
    // within bounds (it is `i + 1` for some enumerated `i`), but fall back to
    // the whole slice rather than panicking if that ever stops holding.
    let row_sizes = sizes.get(..best_split).unwrap_or(sizes);
    let row_sum: f64 = row_sizes.iter().sum();
    let row_fraction = if total_size > 0.0 {
        row_sum / total_size
    } else {
        0.0
    };

    let row_cross_pixels = (cross as f64 * row_fraction) as f32;

    let mut offset = 0.0f32;
    for (&sz, child) in row_sizes.iter().zip(children.iter().copied()) {
        let item_fraction = if row_sum > 0.0 { sz / row_sum } else { 0.0 };
        let item_length = (full_length as f64 * item_fraction) as f32;

        let (rx, ry, rw, rh) = if lay_horizontal {
            (x + offset, y, item_length, row_cross_pixels)
        } else {
            (x, y + offset, row_cross_pixels, item_length)
        };

        rects.push(TreemapRect {
            x: rx,
            y: ry,
            width: rw,
            height: rh,
            node_index: rects.len(),
            depth: child.depth,
            color: color_for_node(child),
            path: child.path.clone(),
            name: child.name.clone(),
            size_bytes: child.size_bytes,
            is_dir: child.is_dir(),
        });

        offset += item_length;
    }

    // Recurse into remaining items.
    if best_split < children.len() {
        let remaining_children = children.get(best_split..).unwrap_or(&[]);
        let remaining_sizes = sizes.get(best_split..).unwrap_or(&[]);
        let remaining_total: f64 = remaining_sizes.iter().sum();

        let (nx, ny, nw, nh) = if lay_horizontal {
            (x, y + row_cross_pixels, width, height - row_cross_pixels)
        } else {
            (x + row_cross_pixels, y, width - row_cross_pixels, height)
        };

        if nw > TREEMAP_MIN_RECT && nh > TREEMAP_MIN_RECT {
            squarify_layout(
                remaining_children,
                remaining_sizes,
                remaining_total,
                nx,
                ny,
                nw,
                nh,
                rects,
            );
        }
    }
}

/// Compute the worst (largest) aspect ratio among items in a row.
fn worst_aspect_in_row(sizes: &[f64], row_sum: f64, full_length: f64, row_cross: f64) -> f64 {
    let mut worst = 0.0f64;
    for &sz in sizes {
        let item_fraction = if row_sum > 0.0 { sz / row_sum } else { 0.0 };
        let item_length = full_length * item_fraction;
        if item_length <= 0.0 || row_cross <= 0.0 {
            continue;
        }
        let aspect = if item_length > row_cross {
            item_length / row_cross
        } else {
            row_cross / item_length
        };
        if aspect > worst {
            worst = aspect;
        }
    }
    worst
}

/// Pick a color for a treemap rectangle based on file type/extension.
fn color_for_node(node: &FileNode) -> Color {
    if node.kind == FileKind::Directory {
        return COLOR_SURFACE1;
    }
    let ext = node.extension();
    color_for_extension(&ext)
}

/// Map a file extension to a Catppuccin Mocha color.
fn color_for_extension(ext: &str) -> Color {
    match ext {
        // Video
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" => COLOR_BLUE,
        // Images
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "tiff" => COLOR_GREEN,
        // Documents
        "pdf" | "doc" | "docx" | "odt" | "txt" | "rtf" | "xls" | "xlsx" => COLOR_YELLOW,
        // Code
        "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "java" | "go" | "rb" | "toml" | "json"
        | "yaml" | "xml" | "html" | "css" => COLOR_PEACH,
        // Archives
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" => COLOR_RED,
        // Audio
        "mp3" | "flac" | "wav" | "ogg" | "aac" | "wma" => COLOR_MAUVE,
        // Executables / binaries
        "exe" | "dll" | "so" | "dylib" | "bin" | "elf" => COLOR_TEAL,
        // Fallback
        _ => COLOR_SURFACE0,
    }
}

/// Given a mouse position, find which treemap rectangle was hit.
///
/// Returns the index into the rects slice, or `None` if no hit.
pub fn treemap_hit_test(rects: &[TreemapRect], mx: f32, my: f32) -> Option<usize> {
    // Iterate in reverse so the last-drawn (topmost) rect wins ties.
    for (i, rect) in rects.iter().enumerate().rev() {
        if mx >= rect.x && mx < rect.x + rect.width && my >= rect.y && my < rect.y + rect.height {
            return Some(i);
        }
    }
    None
}

// ============================================================================
// Extension statistics
// ============================================================================

/// Aggregated statistics for one file extension.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionStat {
    pub extension: String,
    pub count: u64,
    pub total_size: u64,
    pub percentage: f32,
}

/// Compute per-extension statistics from a directory tree.
pub fn compute_extension_stats(node: &FileNode) -> Vec<ExtensionStat> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut sizes: BTreeMap<String, u64> = BTreeMap::new();
    collect_ext_stats(node, &mut counts, &mut sizes);

    let grand_total: u64 = sizes.values().sum();

    let mut stats: Vec<ExtensionStat> = counts
        .keys()
        .map(|ext| {
            let count = counts.get(ext).copied().unwrap_or(0);
            let total_size = sizes.get(ext).copied().unwrap_or(0);
            let percentage = share_of(total_size, grand_total);
            ExtensionStat {
                extension: ext.clone(),
                count,
                total_size,
                percentage,
            }
        })
        .collect();

    // Sort by total size descending.
    stats.sort_by_key(|s| std::cmp::Reverse(s.total_size));
    stats
}

fn collect_ext_stats(
    node: &FileNode,
    counts: &mut BTreeMap<String, u64>,
    sizes: &mut BTreeMap<String, u64>,
) {
    if node.kind == FileKind::RegularFile {
        let ext = node.extension();
        if !ext.is_empty() {
            let c = counts.entry(ext.clone()).or_insert(0);
            *c = c.saturating_add(1);
            let s = sizes.entry(ext).or_insert(0);
            *s = s.saturating_add(node.size_bytes);
        }
    }
    for child in &node.children {
        collect_ext_stats(child, counts, sizes);
    }
}

// ============================================================================
// Size formatting
// ============================================================================

/// Format a byte count into a human-readable string.
pub fn format_size(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

/// Format a percentage with one decimal place.
fn format_percent(pct: f32) -> String {
    format!("{pct:.1}%")
}

// ============================================================================
// Sorting
// ============================================================================

/// Column that the list view can be sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Percentage,
    Type,
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

/// A row in the list view table.
#[derive(Clone, Debug)]
pub struct ListRow {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub percentage: f32,
    pub kind: FileKind,
    pub is_expanded: bool,
    pub depth: u32,
    pub has_children: bool,
}

/// Flatten a file tree into list rows for display.
///
/// `node` is the directory the window is *showing*, which after a drill-down is
/// not the root of the scan. Both things that used to be read off
/// [`FileNode::depth`] — whether a row is auto-expanded, and how far it is
/// indented — are relative to `node`, not to the scan:
///
/// * `node.depth == 0` as the "always expanded" test meant that drilling into a
///   directory produced a list of exactly one collapsed row, because the
///   directory now being shown was at depth 1 and so was not the root any more.
/// * `node.depth` as the indent meant a drill-down into a directory seven
///   levels down opened a view whose every row started seven indents in, with
///   [`row_indent`] then capping them all to the same place — so nesting inside
///   the shown directory became invisible.
pub fn flatten_tree(node: &FileNode, parent_size: u64, expanded_paths: &[PathBuf]) -> Vec<ListRow> {
    let mut rows = Vec::new();
    flatten_node(node, parent_size, 0, expanded_paths, &mut rows);
    rows
}

fn flatten_node(
    node: &FileNode,
    parent_size: u64,
    depth: u32,
    expanded_paths: &[PathBuf],
    rows: &mut Vec<ListRow>,
) {
    let percentage = share_of(node.size_bytes, parent_size);
    // The directory being shown is always expanded: after a scan or a
    // drill-down `expanded_paths` says nothing about it, and a user who has
    // just opened a folder expects to see what is in it.
    let is_expanded = depth == 0 || expanded_paths.contains(&node.path);
    rows.push(ListRow {
        name: node.name.clone(),
        path: node.path.clone(),
        size_bytes: node.size_bytes,
        percentage,
        kind: node.kind,
        is_expanded,
        depth,
        has_children: !node.children.is_empty(),
    });

    if is_expanded {
        let mut children: Vec<&FileNode> = node.children.iter().collect();
        children.sort_by_key(|c| std::cmp::Reverse(c.size_bytes));
        for child in children {
            flatten_node(
                child,
                node.size_bytes,
                depth.saturating_add(1),
                expanded_paths,
                rows,
            );
        }
    }
}

/// Sort list rows by the given column and direction.
pub fn sort_rows(rows: &mut [ListRow], column: SortColumn, direction: SortDirection) {
    rows.sort_by(|a, b| {
        let cmp = match column {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Size => a.size_bytes.cmp(&b.size_bytes),
            SortColumn::Percentage => a
                .percentage
                .partial_cmp(&b.percentage)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortColumn::Type => {
                let a_ext = file_kind_label(a.kind);
                let b_ext = file_kind_label(b.kind);
                a_ext.cmp(b_ext)
            }
        };
        match direction {
            SortDirection::Ascending => cmp,
            SortDirection::Descending => cmp.reverse(),
        }
    });
}

/// Human-readable label for a `FileKind`.
fn file_kind_label(kind: FileKind) -> &'static str {
    match kind {
        FileKind::RegularFile => "File",
        FileKind::Directory => "Directory",
        FileKind::Symlink => "Symlink",
        FileKind::Other => "Other",
    }
}

// ============================================================================
// View mode
// ============================================================================

/// Which view the user is currently looking at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Treemap,
    List,
    Extensions,
    Largest,
}

impl ViewMode {
    /// The tabs, left to right, and the labels on them.
    const ALL: &'static [(Self, &'static str)] = &[
        (Self::Treemap, "Treemap"),
        (Self::List, "List"),
        (Self::Extensions, "Extensions"),
        (Self::Largest, "Largest"),
    ];
}

// ============================================================================
// Analyzer configuration
// ============================================================================

/// Configuration options for the disk analyzer.
///
/// Every field here is read by something. That was not true before: the struct
/// carried six knobs, all six were dead, and two of them could not have been
/// honoured at all.
///
/// * **`follow_symlinks` is gone, not unimplemented.** Following links
///   double-counts — the same megabyte appears under two paths, so the reported
///   total exceeds the size of the disk — and a link to an ancestor makes the
///   walk run until the entry budget stops it, filling the treemap with
///   `a/b/a/b/a`. For a tool whose whole job is "where did the space go", both
///   answers are worse than useless. `du` refuses for the same reason.
/// * **`cross_filesystems` is gone because it cannot be answered.** Telling one
///   filesystem from another needs the device id from `MetadataExt`, which
///   nothing in this tree implements yet. A knob that silently does nothing is
///   worse than no knob, because the user believes the scan obeyed it.
/// * **`min_display_size` is gone** because hiding rows below a size is how a
///   user comes to believe a file is not there. The treemap already drops
///   rectangles too small to draw, which is a statement about pixels rather
///   than a filter on the data.
#[derive(Clone, Debug)]
pub struct AnalyzerConfig {
    /// Root path to scan.
    pub scan_path: PathBuf,
    /// Deepest level below the root to descend into; 0 means [`scan::MAX_DEPTH`].
    pub max_scan_depth: u32,
    /// Ceiling on entries visited, so a pathological tree cannot hang the window.
    pub max_entries: u64,
    /// Number of rows the Largest view shows.
    pub top_n: usize,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            scan_path: PathBuf::from("/"),
            max_scan_depth: 0,
            max_entries: scan::DEFAULT_MAX_ENTRIES,
            top_n: 50,
        }
    }
}

impl AnalyzerConfig {
    /// The bounds the walk is given.
    #[must_use]
    pub fn limits(&self) -> scan::Limits {
        scan::Limits {
            max_depth: self.max_scan_depth,
            max_entries: self.max_entries,
        }
    }
}

// ============================================================================
// Hit-test targets
// ============================================================================

/// Everything on screen a click can land on.
///
/// The renderer records one of these against each control's rectangle as it
/// draws it, and [`DiskAnalyzerUI::handle_click`] asks the frame which one is
/// under the pointer. That is the whole reason the geometry is not duplicated
/// between a draw pass and a separate hit-test pass: a control that moves moves
/// its hit box with it, and a control the renderer skipped — a breadcrumb that
/// ran off the end of the bar, a row scrolled out of sight — has no hit box at
/// all and so cannot be clicked by accident.
///
/// The indices are into the vectors the renderer drew from, so they are only
/// meaningful for as long as those vectors are unchanged. Every handler
/// resolves an index to a name or a path before doing anything with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The toolbar's path text field.
    PathInput,
    /// The Scan button, which reads "Cancel" while a scan is running.
    Scan,
    /// One of the four view tabs.
    View(ViewMode),
    /// A breadcrumb segment; the index is a depth into `breadcrumbs`.
    Breadcrumb(usize),
    /// A treemap tile; the index is into `treemap_rects`.
    TreemapRect(usize),
    /// A file-list row; the index is into the *visible* rows.
    Row(usize),
    /// The expand/collapse arrow at the head of a row, which is a separate
    /// target from the row so that opening and expanding are separate clicks.
    RowChevron(usize),
    /// A sortable column heading.
    ColumnHeader(SortColumn),
}

/// This program's frame type: a render tree with a [`Target`] recorded against
/// each control it drew.
pub type Frame = guitk::frame::Frame<Target>;

// ============================================================================
// UI state
// ============================================================================

/// Complete UI state for the disk analyzer.
pub struct DiskAnalyzerUI {
    /// Active view mode.
    pub view_mode: ViewMode,
    /// Current configuration.
    pub config: AnalyzerConfig,
    /// Scanned directory tree (populated after a scan).
    pub dir_tree: Option<DirTree>,
    /// Computed treemap rectangles, for the directory the breadcrumbs name.
    pub treemap_rects: Vec<TreemapRect>,
    /// Extension statistics, for the directory the breadcrumbs name.
    pub extension_stats: Vec<ExtensionStat>,
    /// The trail from the scan root to the directory being shown.
    ///
    /// Names, not paths, and the first entry is the root's own name. Names are
    /// unique within a directory, so a trail of them identifies exactly one
    /// node — see [`DiskAnalyzerUI::current_node`], which is what makes the
    /// breadcrumbs *do* something. They used to be decoration: `drill_down`
    /// pushed a name and nothing else changed, so the trail grew while the
    /// treemap under it kept showing the whole disk.
    pub breadcrumbs: Vec<String>,
    /// Paths of expanded directories in the list view.
    pub expanded_paths: Vec<PathBuf>,
    /// Current sort column.
    pub sort_column: SortColumn,
    /// Current sort direction.
    pub sort_direction: SortDirection,
    /// Hovered treemap rect index.
    pub hovered_rect: Option<usize>,
    /// Tooltip text shown on hover.
    pub tooltip_text: String,
    /// Tooltip position.
    pub tooltip_x: f32,
    pub tooltip_y: f32,
    /// Live progress of the running scan, if there is one.
    pub progress: scan::Progress,
    /// The scan in flight. `Some` exactly while `scanning` would have been true.
    job: Option<scan::Job>,
    /// Paths the last scan could not read, and how many there were in total.
    pub unreadable: Vec<PathBuf>,
    pub unreadable_count: u64,
    /// Whether the last scan hit a depth or entry cap.
    pub truncated: bool,
    /// Whether the last scan was stopped by the user before it finished.
    pub cancelled: bool,
    /// Whether the last scan measured everything it was pointed at.
    ///
    /// Recorded from [`scan::Outcome::is_complete`] rather than recomputed here
    /// out of `truncated`, `cancelled` and `unreadable_count`: that method is
    /// the definition of complete, and a second copy of it would silently stop
    /// agreeing the first time the scanner learns a new way to come up short.
    pub complete: bool,
    /// Why the last scan produced nothing, if it did.
    pub scan_error: Option<String>,
    /// Text in the path input field.
    pub path_input: String,
    /// Whether the path field has the keyboard.
    pub path_focused: bool,
    /// Index of the first list-view row to draw.
    ///
    /// A row index rather than a pixel offset: the list draws whole rows only
    /// (see [`guitk::scroll_window`]), so a pixel offset would only be able to
    /// express positions the renderer then rounds away. Written by
    /// [`DiskAnalyzerUI::scroll_list_by`]; a value past the end is not an
    /// error, and shows the last full page.
    pub scroll_offset: usize,
    /// List rows (cached after sort/flatten).
    pub list_rows: Vec<ListRow>,
    /// Rows for the Largest view, computed over the whole scan rather than the
    /// current directory.
    pub largest_rows: Vec<ListRow>,
    /// The size the compositor last said the window is.
    ///
    /// A *record* of the last size, never the authority on it: every frame is
    /// drawn at the size `render` is handed, because the first frame goes out
    /// before any `Event::Resize` arrives. This copy exists so that an event
    /// handler — which is given no size — can hit-test against the same layout
    /// the user is looking at.
    window_size: (f32, f32),
    /// Leftover wheel movement between scroll events.
    ///
    /// A field and not a local, because that is the entire point of it: a
    /// high-resolution wheel or a touchpad sends fractions of a notch, and an
    /// accumulator created fresh per event rounds every one of them to zero.
    /// The list would then refuse to scroll at all on exactly the hardware that
    /// scrolls most smoothly.
    wheel: wheel::Accumulator,
}

impl Default for DiskAnalyzerUI {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskAnalyzerUI {
    /// Create a new UI with default state.
    #[must_use]
    pub fn new() -> Self {
        let config = AnalyzerConfig::default();
        let path_input = config.scan_path.display().to_string();
        Self {
            view_mode: ViewMode::Treemap,
            config,
            dir_tree: None,
            treemap_rects: Vec::new(),
            extension_stats: Vec::new(),
            breadcrumbs: vec!["/".to_string()],
            expanded_paths: Vec::new(),
            sort_column: SortColumn::Size,
            sort_direction: SortDirection::Descending,
            hovered_rect: None,
            tooltip_text: String::new(),
            tooltip_x: 0.0,
            tooltip_y: 0.0,
            progress: scan::Progress::default(),
            job: None,
            unreadable: Vec::new(),
            unreadable_count: 0,
            truncated: false,
            cancelled: false,
            complete: true,
            scan_error: None,
            path_input,
            path_focused: false,
            scroll_offset: 0,
            list_rows: Vec::new(),
            largest_rows: Vec::new(),
            window_size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            wheel: wheel::Accumulator::default(),
        }
    }

    /// Whether a scan is running.
    ///
    /// Derived from the job rather than stored beside it: a `scanning` flag and
    /// a live job are two answers to one question, and the pair goes wrong in
    /// the direction that leaves the window saying "Scanning…" forever.
    #[must_use]
    pub fn scanning(&self) -> bool {
        self.job.is_some()
    }

    /// Set the view mode.
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        if self.view_mode != mode {
            self.view_mode = mode;
            // Each view has its own list of a different length. Carrying one
            // view's offset into another shows the second one scrolled to a
            // position the user never chose, or blank.
            self.scroll_offset = 0;
        }
    }

    /// Scrolls the list view by `delta` rows: positive is towards the end.
    ///
    /// The host calls this from a wheel event or a Page key. Deliberately not
    /// clamped to the list length here — the length that matters is the one at
    /// the next *render*, and the renderer clamps against it. Clamping here
    /// instead would mean a scroll issued while a scan is still adding rows got
    /// pinned to a list that has since grown.
    pub fn scroll_list_by(&mut self, delta: isize) {
        self.scroll_offset = scroll_window::shift(self.scroll_offset, delta);
    }

    /// Scrolls the list view back to the first row.
    pub fn scroll_list_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    // -- scanning --------------------------------------------------------------

    /// Begin walking `root` on a background thread.
    ///
    /// Any scan already running is cancelled first: two walks writing two trees
    /// into one window is a race whose visible symptom is a treemap that flips
    /// between two disks.
    pub fn start_scan(&mut self, root: PathBuf) {
        self.cancel_scan();
        self.config.scan_path.clone_from(&root);
        self.path_input = root.display().to_string();
        self.scan_error = None;
        self.unreadable.clear();
        self.unreadable_count = 0;
        self.truncated = false;
        self.progress = scan::Progress::default();
        match scan::Job::spawn(root, self.config.limits()) {
            Ok(job) => self.job = Some(job),
            Err(err) => self.scan_error = Some(format!("could not start the scan: {err}")),
        }
    }

    /// Stop the scan in flight, if there is one, and forget it.
    pub fn cancel_scan(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel();
        }
    }

    /// Take a step of the running scan: refresh the progress line and, if the
    /// walk has finished, adopt its result.
    ///
    /// Returns whether anything the user can see changed. Called from
    /// `Event::Tick`; a scan that is never polled is a scan that never appears.
    pub fn poll_scan(&mut self, size: (f32, f32)) -> bool {
        let Some(job) = self.job.as_mut() else {
            return false;
        };
        let outcome = job.poll();
        let progress = job.progress();
        let Some(outcome) = outcome else {
            let changed = progress != self.progress;
            self.progress = progress;
            return changed;
        };
        self.job = None;
        self.progress = progress;
        self.adopt(outcome, size);
        true
    }

    /// Take a finished walk's result as what the window now shows.
    fn adopt(&mut self, outcome: scan::Outcome, size: (f32, f32)) {
        self.complete = outcome.is_complete();
        self.unreadable = outcome.unreadable;
        self.unreadable_count = outcome.unreadable_count;
        self.truncated = outcome.truncated;
        self.cancelled = outcome.cancelled;
        self.scan_error = outcome.root_error;
        self.load_dir_tree(outcome.tree, size);
    }

    /// Adopt an already-built tree. The scan path is not touched, so this is
    /// also how a test loads a fixture without a filesystem.
    pub fn load_dir_tree(&mut self, tree: DirTree, size: (f32, f32)) {
        self.breadcrumbs = vec![tree.root.name.clone()];
        self.expanded_paths.clear();
        self.scroll_offset = 0;
        self.hovered_rect = None;
        self.tooltip_text.clear();
        self.largest_rows = find_largest(&tree.root, self.config.top_n, tree.total_size);
        self.dir_tree = Some(tree);
        self.recompute(size);
    }

    /// Start a scan with the given pre-built tree (for testing / offline use).
    pub fn load_tree(&mut self, mut root: FileNode) {
        let tree = summarize_tree(&mut root);
        let size = self.window_size;
        self.load_dir_tree(tree, size);
    }

    // -- what is being shown ---------------------------------------------------

    /// The node the breadcrumb trail names, or the root when the trail has gone
    /// stale (a new scan under an old trail).
    #[must_use]
    pub fn current_node(&self) -> Option<&FileNode> {
        let tree = self.dir_tree.as_ref()?;
        let mut node = &tree.root;
        for name in self.breadcrumbs.iter().skip(1) {
            match node.child_named(name) {
                Some(child) => node = child,
                None => return Some(&tree.root),
            }
        }
        Some(node)
    }

    /// Recompute everything that depends on *which* directory is shown and on
    /// how big the window is.
    ///
    /// One function, called from every place that changes either, because the
    /// treemap rectangles are absolute pixel coordinates and the click handler
    /// hit-tests against them: a rectangle list computed for one window size and
    /// drawn at another is a set of controls that are not where they look.
    pub fn recompute(&mut self, size: (f32, f32)) {
        let Some(tree) = self.dir_tree.as_ref() else {
            self.treemap_rects.clear();
            self.extension_stats.clear();
            self.list_rows.clear();
            return;
        };
        // Re-borrowed rather than held across the mutations below: `current_node`
        // borrows `self`, and the rows it produces are owned copies anyway.
        let (rects, stats, rows) = {
            let node = self.current_node().unwrap_or(&tree.root);
            let area = content_rect(size);
            (
                compute_treemap(node, area.x, area.y, area.w, area.h),
                compute_extension_stats(node),
                flatten_tree(node, node.size_bytes, &self.expanded_paths),
            )
        };
        self.treemap_rects = rects;
        self.extension_stats = stats;
        self.list_rows = rows;
        sort_rows(&mut self.list_rows, self.sort_column, self.sort_direction);
    }

    /// The rows the active view is showing.
    #[must_use]
    pub fn visible_rows(&self) -> &[ListRow] {
        match self.view_mode {
            ViewMode::Largest => &self.largest_rows,
            _ => &self.list_rows,
        }
    }

    /// Toggle expansion of a path in the list view.
    pub fn toggle_expand(&mut self, path: &Path) {
        if let Some(pos) = self.expanded_paths.iter().position(|p| p == path) {
            self.expanded_paths.remove(pos);
        } else {
            self.expanded_paths.push(path.to_path_buf());
        }
        let size = self.window_size;
        self.recompute(size);
    }

    /// Set sort column, toggling direction if the same column is clicked again.
    pub fn set_sort(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_direction = self.sort_direction.toggle();
        } else {
            self.sort_column = column;
            self.sort_direction = SortDirection::Descending;
        }
        sort_rows(&mut self.list_rows, self.sort_column, self.sort_direction);
        sort_rows(
            &mut self.largest_rows,
            self.sort_column,
            self.sort_direction,
        );
    }

    /// Handle mouse hover over the treemap at (mx, my).
    pub fn hover_treemap(&mut self, mx: f32, my: f32) {
        self.hovered_rect = treemap_hit_test(&self.treemap_rects, mx, my);
        if let Some(idx) = self.hovered_rect {
            if let Some(rect) = self.treemap_rects.get(idx) {
                self.tooltip_text =
                    format!("{}\n{}", rect.path.display(), format_size(rect.size_bytes));
                self.tooltip_x = mx;
                self.tooltip_y = my;
            }
        } else {
            self.tooltip_text.clear();
        }
    }

    /// Navigate the breadcrumb trail to a specific depth.
    pub fn navigate_breadcrumb(&mut self, depth: usize, size: (f32, f32)) {
        if depth < self.breadcrumbs.len() {
            self.breadcrumbs.truncate(depth.saturating_add(1));
            self.scroll_offset = 0;
            self.recompute(size);
        }
    }

    /// Drill down into a directory of the directory currently shown.
    ///
    /// Refuses a name that is not a child, rather than pushing it and leaving
    /// the trail pointing at nothing.
    pub fn drill_down(&mut self, name: &str, size: (f32, f32)) -> bool {
        let is_child = self
            .current_node()
            .and_then(|node| node.child_named(name))
            .is_some_and(FileNode::is_dir);
        if !is_child {
            return false;
        }
        self.breadcrumbs.push(name.to_string());
        self.scroll_offset = 0;
        self.hovered_rect = None;
        self.tooltip_text.clear();
        self.recompute(size);
        true
    }

    /// Jump straight to `path`, wherever in the tree it is.
    ///
    /// [`drill_down`](Self::drill_down) can only descend one *child* of what is
    /// already shown, which is all a treemap tile or a nested list row ever
    /// needs. The Largest view is different: it is flat, and its rows are
    /// gathered from the whole tree, so the directory a user clicks there is
    /// generally not a child of anything currently on screen. Rebuilding the
    /// trail from the root is the only way to arrive somewhere the current view
    /// does not contain.
    ///
    /// Returns `false` if no directory in the tree has that path — including
    /// when `path` names a *file*, which has no inside to show — leaving the
    /// view exactly as it was.
    pub fn navigate_to_path(&mut self, path: &Path, size: (f32, f32)) -> bool {
        let Some(tree) = self.dir_tree.as_ref() else {
            return false;
        };
        let mut trail = vec![tree.root.name.clone()];
        if !trail_to(&tree.root, path, &mut trail) {
            return false;
        }
        self.breadcrumbs = trail;
        self.scroll_offset = 0;
        self.hovered_rect = None;
        self.tooltip_text.clear();
        self.recompute(size);
        true
    }

    // -- rendering -------------------------------------------------------------

    /// Draw the whole window at `width` by `height`, recording where every
    /// control ended up.
    ///
    /// Rendering and hit-testing are the same walk: a control is clickable
    /// because it was drawn, at the rectangle it was drawn at. The alternative —
    /// a renderer and a separate `hit_test` that each compute the layout — is
    /// two copies of the geometry, and they diverge on the first change to
    /// either.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let (w, h) = (width.max(MIN_WIDTH), height.max(MIN_HEIGHT));
        let mut frame = Frame::new(w, h);

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut frame, w);
        self.render_breadcrumbs(&mut frame, w);

        match self.view_mode {
            ViewMode::Treemap => self.render_treemap_view(&mut frame, (w, h)),
            ViewMode::List | ViewMode::Largest => self.render_list_view(&mut frame, (w, h)),
            ViewMode::Extensions => self.render_extension_view(&mut frame, (w, h)),
        }

        self.render_status_bar(&mut frame, (w, h));
        self.render_tooltip(&mut frame, (w, h));

        frame
    }

    // -- toolbar ---------------------------------------------------------------

    fn render_toolbar(&self, frame: &mut Frame, width: f32) {
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: TOOLBAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Path input field. It shrinks with the window rather than running off
        // the edge, and never below the width of the buttons beside it.
        let scan_width = BUTTON_WIDTH.max(70.0);
        let modes_width: f32 = ViewMode::ALL
            .iter()
            .map(|(_, label)| view_button_width(label) + 4.0)
            .sum();
        let input_width =
            (width - 4.0 * PADDING - scan_width - modes_width).clamp(80.0, INPUT_WIDTH);
        let input = Rect::new(PADDING, 7.0, input_width, INPUT_HEIGHT);
        frame.push(RenderCommand::FillRect {
            x: input.x,
            y: input.y,
            width: input.w,
            height: input.h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        if self.path_focused {
            // The caret would be the usual signal, and there is no caret here
            // because there is no text-editing widget behind this field. An
            // outline is what is left that still says "your typing goes here";
            // a focused field that looks exactly like an unfocused one is how a
            // user types a path into nothing.
            frame.push(RenderCommand::StrokeRect {
                x: input.x,
                y: input.y,
                width: input.w,
                height: input.h,
                color: COLOR_BLUE,
                line_width: 1.0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
        }
        frame.push(RenderCommand::Text {
            x: input.x + 8.0,
            y: 14.0,
            text: self.path_input.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(input.w - 16.0),
            // Cut at the front: a path's tail is the part that says which
            // directory it is, and the head is the part every sibling shares.
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(Target::PathInput, input);

        // Scan button — which is a Cancel button while a scan is running,
        // because a scan of a large disk is minutes long and a user who started
        // the wrong one has no other way out.
        let scan = Rect::new(input.right() + PADDING, 7.0, scan_width, BUTTON_HEIGHT);
        let scanning = self.scanning();
        frame.push(RenderCommand::FillRect {
            x: scan.x,
            y: scan.y,
            width: scan.w,
            height: scan.h,
            color: if scanning { COLOR_PEACH } else { COLOR_BLUE },
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        frame.push(RenderCommand::Text {
            x: scan.x + 8.0,
            y: 14.0,
            text: if scanning { "Cancel" } else { "Scan" }.to_string(),
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(scan.w - 16.0),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(Target::Scan, scan);

        // View mode tabs.
        let mut btn_x = scan.right() + PADDING;
        for (mode, label) in ViewMode::ALL {
            let btn = Rect::new(btn_x, 7.0, view_button_width(label), BUTTON_HEIGHT);
            let active = self.view_mode == *mode;
            frame.push(RenderCommand::FillRect {
                x: btn.x,
                y: btn.y,
                width: btn.w,
                height: btn.h,
                color: if active {
                    COLOR_SURFACE1
                } else {
                    COLOR_SURFACE0
                },
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            frame.push(RenderCommand::Text {
                x: btn.x + 8.0,
                y: 14.0,
                text: (*label).to_string(),
                color: if active { COLOR_TEXT } else { COLOR_SUBTEXT0 },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(btn.w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            frame.hit(Target::View(*mode), btn);
            btn_x = btn.right() + 4.0;
        }
    }

    // -- breadcrumbs -----------------------------------------------------------

    fn render_breadcrumbs(&self, frame: &mut Frame, width: f32) {
        let y = TOOLBAR_HEIGHT;
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width,
            height: BREADCRUMB_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let last = self.breadcrumbs.len().saturating_sub(1);
        let mut bx = PADDING;
        for (i, segment) in self.breadcrumbs.iter().enumerate() {
            if i > 0 {
                frame.push(RenderCommand::Text {
                    x: bx,
                    y: y + 8.0,
                    text: " / ".to_string(),
                    color: COLOR_OVERLAY0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                bx += 20.0;
            }
            let seg_w = breadcrumb_width(segment);
            // A trail longer than the bar stops being drawn rather than running
            // under the window edge, and — because the hit box is recorded from
            // the same rectangle — stops being clickable at the same point. A
            // segment that is drawn off-screen but still clickable is a control
            // nobody can see swallowing clicks aimed at the one behind it.
            if bx + seg_w > width - PADDING {
                break;
            }
            frame.push(RenderCommand::Text {
                x: bx,
                y: y + 8.0,
                text: segment.clone(),
                color: if i == last { COLOR_TEXT } else { COLOR_BLUE },
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(BREADCRUMB_MAX_SEGMENT),
                overflow: TextOverflow::Ellipsis,
            });
            frame.hit(
                Target::Breadcrumb(i),
                Rect::new(bx, y, seg_w, BREADCRUMB_HEIGHT),
            );
            bx += seg_w + 4.0;
        }
    }

    // -- treemap view ----------------------------------------------------------

    fn render_treemap_view(&self, frame: &mut Frame, size: (f32, f32)) {
        for (i, rect) in self.treemap_rects.iter().enumerate() {
            let is_hovered = self.hovered_rect == Some(i);
            let color = if is_hovered {
                lighten_color(rect.color, 30)
            } else {
                rect.color
            };

            frame.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                color,
                corner_radii: CornerRadii::all(CORNER_RADIUS_SMALL),
            });
            frame.push(RenderCommand::StrokeRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                color: COLOR_BASE,
                line_width: 1.0,
                corner_radii: CornerRadii::all(CORNER_RADIUS_SMALL),
            });

            // Label (only if rect is large enough)
            if rect.width > 60.0 && rect.height > 20.0 {
                frame.push(RenderCommand::Text {
                    x: rect.x + 4.0,
                    y: rect.y + 4.0,
                    text: rect.name.clone(),
                    color: COLOR_TEXT,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(rect.width - 8.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            // Size label if rect is tall enough
            if rect.width > 60.0 && rect.height > 36.0 {
                frame.push(RenderCommand::Text {
                    x: rect.x + 4.0,
                    y: rect.y + 18.0,
                    text: format_size(rect.size_bytes),
                    color: COLOR_SUBTEXT0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(rect.width - 8.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            frame.hit(
                Target::TreemapRect(i),
                Rect::new(rect.x, rect.y, rect.width, rect.height),
            );
        }

        if self.treemap_rects.is_empty() {
            self.render_placeholder(frame, size);
        }
    }

    /// What the content area says when there is nothing in it.
    fn render_placeholder(&self, frame: &mut Frame, size: (f32, f32)) {
        let message = self.empty_message();
        if message.is_empty() {
            return;
        }
        let area = content_rect(size);
        let (cx, cy) = area.centre();
        frame.push(RenderCommand::Text {
            x: (cx - 150.0).max(area.x),
            y: cy,
            text: message,
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Regular,
            max_width: Some(area.w),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Why the content area is empty, in the user's terms.
    ///
    /// Four different reasons produce the same blank rectangle — nothing has
    /// been scanned, a scan is running, the scan failed, the directory really is
    /// empty — and "No data. Click Scan to begin." was shown for all of them.
    /// Told to click Scan while a scan is already running, a user clicks it
    /// again.
    #[must_use]
    pub fn empty_message(&self) -> String {
        if self.scanning() {
            return "Scanning…".to_string();
        }
        if let Some(err) = &self.scan_error {
            return format!("Could not scan: {err}");
        }
        if self.dir_tree.is_none() {
            return "No data. Click Scan to begin.".to_string();
        }
        "This folder is empty.".to_string()
    }

    // -- list view -------------------------------------------------------------

    fn render_list_view(&self, frame: &mut Frame, size: (f32, f32)) {
        let (width, _) = size;
        let area = list_area(size);
        let columns = list_columns(width);
        let table = Table::with_gap(&columns, 0.0, PADDING);

        // Table header.
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: area.y,
            width,
            height: TABLE_HEADER_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        frame.draw_with(|cmds| table.header(cmds, area.y + 8.0, COLOR_TEXT, FONT_SIZE));
        for (index, column) in SORTABLE_COLUMNS.iter().enumerate() {
            frame.hit(
                Target::ColumnHeader(*column),
                Rect::new(
                    table.left(index),
                    area.y,
                    table.width(index),
                    TABLE_HEADER_HEIGHT,
                ),
            );
        }
        // The sort marker. Without it the list is sorted and refuses to say by
        // what, so the third click on a header looks like it did nothing.
        if let Some(index) = SORTABLE_COLUMNS.iter().position(|c| *c == self.sort_column) {
            frame.push(RenderCommand::Text {
                x: table.right(index) - 8.0,
                y: area.y + 8.0,
                text: match self.sort_direction {
                    SortDirection::Ascending => "^".to_string(),
                    SortDirection::Descending => "v".to_string(),
                },
                color: COLOR_BLUE,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(8.0),
                overflow: TextOverflow::Clip,
            });
        }

        let rows = self.visible_rows();
        if rows.is_empty() {
            self.render_placeholder(frame, size);
            return;
        }

        // Rows. `scroll_window` decides which of them are on screen: it
        // truncates to whole rows (so nothing is drawn across the status bar)
        // and pulls a stale offset back to the last full page, which is what
        // makes a listing that shrank -- a directory collapsed, a filter
        // applied -- show its tail rather than going blank.
        let row_area_y = area.y + TABLE_HEADER_HEIGHT;
        let window = scroll_window::visible(
            rows.len(),
            ROW_HEIGHT,
            area.h - TABLE_HEADER_HEIGHT,
            self.scroll_offset,
        );

        for (drawn, row) in rows
            .get(window.start..window.end())
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let i = window.start.saturating_add(drawn);
            #[allow(clippy::cast_precision_loss)]
            let ry = row_area_y + drawn as f32 * ROW_HEIGHT;
            // Alternating row background. Striped by absolute row index, not by
            // position on screen, so the stripes do not invert as you scroll.
            if i % 2 == 0 {
                frame.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: ry,
                    width,
                    height: ROW_HEIGHT,
                    color: COLOR_SURFACE0,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            let indent = row_indent(row.depth, table.width(NAME_COL));
            let name_x = table.left(NAME_COL) + indent;

            // Expand/collapse chevron, in its own box ahead of the name.
            let chevron = if row.has_children {
                if row.is_expanded { "v" } else { ">" }
            } else {
                ""
            };
            frame.draw_with(|cmds| {
                Table::fitted(
                    cmds,
                    name_x,
                    CHEVRON_WIDTH,
                    ry + 6.0,
                    chevron,
                    COLOR_OVERLAY0,
                    FONT_SIZE,
                    Fit::Start,
                    FontWeightHint::Regular,
                );
            });

            // Name. Cut at the front, because a listing sorted by size is
            // mostly siblings of one directory, and siblings are what share a
            // prefix — `VID_20260812_143022.mp4` beside `VID_20260812_150907.mp4`
            // are one string cut the usual way. The indent and the chevron come
            // out of the column rather than being added to it, so a nested row's
            // name is fitted to the room it actually has.
            let name_room = table.width(NAME_COL) - indent - CHEVRON_WIDTH;
            frame.draw_with(|cmds| {
                Table::fitted(
                    cmds,
                    name_x + CHEVRON_WIDTH,
                    name_room,
                    ry + 6.0,
                    &row.name,
                    if row.kind == FileKind::Directory {
                        COLOR_BLUE
                    } else {
                        COLOR_TEXT
                    },
                    FONT_SIZE,
                    Fit::End,
                    FontWeightHint::Regular,
                );
            });

            frame.draw_with(|cmds| {
                table.cell(
                    cmds,
                    SIZE_COL,
                    ry + 6.0,
                    &format_size(row.size_bytes),
                    COLOR_SUBTEXT0,
                    FONT_SIZE,
                    Fit::Start,
                );
            });
            frame.draw_with(|cmds| {
                table.cell(
                    cmds,
                    PERCENT_COL,
                    ry + 6.0,
                    &format_percent(row.percentage),
                    COLOR_SUBTEXT0,
                    FONT_SIZE,
                    Fit::Start,
                );
            });
            frame.draw_with(|cmds| {
                table.cell(
                    cmds,
                    KIND_COL,
                    ry + 6.0,
                    file_kind_label(row.kind),
                    COLOR_SUBTEXT0,
                    FONT_SIZE,
                    Fit::Start,
                );
            });

            // The chevron's own box first, so it wins over the row behind it:
            // clicking the arrow expands, clicking anywhere else on the row
            // opens. `Frame::hit_test` answers with the last matching box, so
            // order here is the reverse of what a painter's-algorithm reading
            // suggests — the row is recorded first and the chevron second.
            frame.hit(Target::Row(i), Rect::new(0.0, ry, width, ROW_HEIGHT));
            if row.has_children {
                frame.hit(
                    Target::RowChevron(i),
                    Rect::new(name_x, ry, CHEVRON_WIDTH, ROW_HEIGHT),
                );
            }
        }
    }

    // -- extension view --------------------------------------------------------

    fn render_extension_view(&self, frame: &mut Frame, size: (f32, f32)) {
        let area = content_rect(size);
        let content_w = area.w;

        frame.push(RenderCommand::Text {
            x: PADDING,
            y: area.y,
            text: "File Types by Size".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });

        let chart_y = area.y + 28.0;
        let max_bar_width = (content_w - 200.0).max(20.0);
        // How many bars fit, rather than a fixed twenty: at 700px twenty bars
        // ran through the status bar, and in a taller window the rows past the
        // twentieth were simply not drawn although there was room.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let max_bars = ((area.bottom() - chart_y) / (BAR_CHART_ROW_HEIGHT + 4.0)).max(0.0) as usize;

        let max_size = self
            .extension_stats
            .first()
            .map_or(1, |s| s.total_size)
            .max(1);

        for (i, stat) in self.extension_stats.iter().take(max_bars).enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let by = chart_y + i as f32 * (BAR_CHART_ROW_HEIGHT + 4.0);
            #[allow(clippy::cast_precision_loss)]
            let bar_fraction = stat.total_size as f32 / max_size as f32;
            let bar_w = (max_bar_width * bar_fraction).max(2.0);

            let label = if stat.extension.is_empty() {
                "(no ext)".to_string()
            } else {
                format!(".{}", stat.extension)
            };
            frame.push(RenderCommand::Text {
                x: PADDING,
                y: by + 4.0,
                text: label,
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0 - PADDING),
                overflow: TextOverflow::Ellipsis,
            });

            frame.push(RenderCommand::FillRect {
                x: 90.0,
                y: by,
                width: bar_w,
                height: BAR_CHART_ROW_HEIGHT,
                color: color_for_extension(&stat.extension),
                corner_radii: CornerRadii::all(CORNER_RADIUS_SMALL),
            });

            let text_x = 90.0 + bar_w + 8.0;
            frame.push(RenderCommand::Text {
                x: text_x,
                y: by + 4.0,
                text: format!(
                    "{} ({} files, {})",
                    format_size(stat.total_size),
                    stat.count,
                    format_percent(stat.percentage),
                ),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                // Measured against the room actually left to the right of the
                // bar. A fixed 300px let the longest bar's label run off the
                // window, which is the one row where the number matters most.
                max_width: Some((area.right() - text_x).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

        if self.extension_stats.is_empty() {
            self.render_placeholder(frame, size);
        }
    }

    // -- status bar ------------------------------------------------------------

    fn render_status_bar(&self, frame: &mut Frame, size: (f32, f32)) {
        let (width, height) = size;
        let y = height - STATUS_BAR_HEIGHT;
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width,
            height: STATUS_BAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        frame.push(RenderCommand::Text {
            x: PADDING,
            y: y + 6.0,
            text: self.status_text(),
            color: if self.scan_error.is_some() {
                COLOR_RED
            } else if !self.complete {
                COLOR_YELLOW
            } else {
                COLOR_SUBTEXT0
            },
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 2.0 * PADDING),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The line along the bottom of the window.
    ///
    /// Everything that makes the totals above it less than the whole truth is
    /// said here. A disk analyzer that reports "Total: 40 GB" when three
    /// directories were unreadable and the entry budget ran out is not
    /// reporting a total; it is reporting a lower bound and calling it a total,
    /// and the user goes looking for the missing space in the wrong place.
    #[must_use]
    pub fn status_text(&self) -> String {
        if let Some(err) = &self.scan_error {
            return format!("Scan failed — {err}");
        }
        if self.scanning() {
            let p = &self.progress;
            return format!(
                "Scanning: {} dirs, {} files, {} found | {}",
                p.dirs,
                p.files,
                format_size(p.bytes),
                p.current.display(),
            );
        }
        let Some(tree) = &self.dir_tree else {
            return "Ready".to_string();
        };
        let mut text = format!(
            "Total: {} | Files: {} | Dirs: {} | Scan: {}ms",
            format_size(tree.total_size),
            tree.file_count,
            tree.dir_count,
            tree.scan_duration_ms,
        );
        if self.cancelled {
            // Said before the other two because it is the one the user caused,
            // and a stopped scan looks exactly like a finished one otherwise:
            // the totals are drawn, they are simply wrong.
            text.push_str(" | STOPPED: totals cover only what was scanned");
        }
        if self.truncated {
            text.push_str(" | PARTIAL: the scan hit its limit");
        }
        if self.unreadable_count > 0 {
            let n = self.unreadable_count;
            let plural = if n == 1 { "" } else { "s" };
            text.push_str(&format!(" | {n} path{plural} unreadable"));
            if let Some(first) = self.unreadable.first() {
                text.push_str(&format!(" (e.g. {})", first.display()));
            }
        }
        text
    }

    // -- tooltip ---------------------------------------------------------------

    fn render_tooltip(&self, frame: &mut Frame, size: (f32, f32)) {
        if self.tooltip_text.is_empty() {
            return;
        }
        let (width, height) = size;
        let tw = 250.0f32.min(width - 8.0);
        let th = 44.0f32;
        let tx = (self.tooltip_x + 12.0).min(width - tw - 4.0).max(4.0);
        let ty = (self.tooltip_y + 12.0).min(height - th - 4.0).max(4.0);

        frame.push(RenderCommand::FillRect {
            x: tx,
            y: ty,
            width: tw,
            height: th,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        frame.push(RenderCommand::StrokeRect {
            x: tx,
            y: ty,
            width: tw,
            height: th,
            color: COLOR_OVERLAY0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let mut line_y = ty + 6.0;
        for line in self.tooltip_text.split('\n') {
            frame.push(RenderCommand::Text {
                x: tx + 8.0,
                y: line_y,
                text: line.to_string(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(tw - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            line_y += 16.0;
        }
        // Deliberately records no hit box. A tooltip follows the pointer, so a
        // clickable one would sit between the pointer and whatever the user is
        // pointing at and swallow the click that dismissed it.
    }

    // -- events ----------------------------------------------------------------

    /// What is under `(x, y)`, by drawing the frame and asking it.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32, size: (f32, f32)) -> Option<Target> {
        self.frame(size.0, size.1).hit_test(x, y)
    }

    /// Act on a click.
    pub fn handle_click(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        size: (f32, f32),
    ) -> Action {
        if button != MouseButton::Left {
            return Action::None;
        }
        let target = self.hit_test(x, y, size);
        // A click anywhere but the path field takes the keyboard away from it.
        // A field that keeps focus after the user has plainly moved on turns
        // the next keystroke into a path edit nobody asked for.
        if self.path_focused && target != Some(Target::PathInput) {
            self.path_focused = false;
        }
        match target {
            Some(Target::PathInput) => {
                self.path_focused = true;
                Action::Redraw
            }
            Some(Target::Scan) => {
                if self.scanning() {
                    self.cancel_scan();
                } else {
                    self.start_scan(PathBuf::from(&self.path_input));
                }
                Action::Redraw
            }
            Some(Target::View(mode)) => {
                if self.view_mode == mode {
                    return Action::None;
                }
                self.set_view_mode(mode);
                Action::Redraw
            }
            Some(Target::Breadcrumb(i)) => {
                if i.saturating_add(1) == self.breadcrumbs.len() {
                    // Already here.
                    return Action::None;
                }
                self.navigate_breadcrumb(i, size);
                Action::Redraw
            }
            Some(Target::TreemapRect(i)) => self.activate_treemap(i, size),
            Some(Target::RowChevron(i)) => {
                let Some(path) = self.row_path(i) else {
                    return Action::None;
                };
                self.toggle_expand(&path);
                Action::Redraw
            }
            Some(Target::Row(i)) => self.activate_row(i, size),
            Some(Target::ColumnHeader(column)) => {
                self.set_sort(column);
                Action::Redraw
            }
            None => {
                if self.tooltip_text.is_empty() {
                    Action::None
                } else {
                    self.tooltip_text.clear();
                    self.hovered_rect = None;
                    Action::Redraw
                }
            }
        }
    }

    /// The path of visible row `i`, if there is such a row.
    fn row_path(&self, i: usize) -> Option<PathBuf> {
        self.visible_rows().get(i).map(|r| r.path.clone())
    }

    /// A click on a treemap rectangle: drill into a directory, do nothing for a
    /// file.
    fn activate_treemap(&mut self, i: usize, size: (f32, f32)) -> Action {
        let Some(rect) = self.treemap_rects.get(i) else {
            return Action::None;
        };
        if !rect.is_dir {
            return Action::None;
        }
        let name = rect.name.clone();
        if self.drill_down(&name, size) {
            Action::Redraw
        } else {
            Action::None
        }
    }

    /// A click on a list row: expand a directory, do nothing for a file.
    fn activate_row(&mut self, i: usize, size: (f32, f32)) -> Action {
        let Some(row) = self.visible_rows().get(i) else {
            return Action::None;
        };
        if row.kind != FileKind::Directory {
            return Action::None;
        }
        let path = row.path.clone();
        // The Largest view is flat and its rows come from all over the tree, so
        // there is nothing to expand *in place*: expanding a row whose parent
        // is not on screen would put children under a row that is not their
        // parent's. It navigates to the directory instead. (Largest lists
        // files, so a directory only turns up there when the scan was aimed at
        // a tree of directories with no files in it — but then it is the only
        // thing the view offers, and refusing the click would leave the window
        // inert.)
        if self.view_mode == ViewMode::Largest {
            return if self.navigate_to_path(&path, size) {
                Action::Redraw
            } else {
                Action::None
            };
        }
        self.toggle_expand(&path);
        Action::Redraw
    }

    /// Act on a key.
    pub fn handle_key(&mut self, key: &KeyEvent, size: (f32, f32)) -> Action {
        if !key.pressed {
            // A release repeats the press's action if it is not filtered here,
            // so every keystroke would count twice.
            return Action::None;
        }
        if self.path_focused {
            return self.handle_path_key(key, size);
        }
        // Ctrl-L is the one shortcut here that wants a modifier; everything
        // else below is a bare key, and must not fire when one is held. Alt-Tab
        // in particular belongs to the window manager — treating it as a plain
        // Tab would switch views behind the user's back as they switched
        // windows, and they would never see it happen.
        if key.modifiers.ctrl && key.key == Key::L {
            self.path_focused = true;
            return Action::Redraw;
        }
        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {
            return Action::None;
        }
        match key.key {
            Key::Escape => {
                if self.scanning() {
                    self.cancel_scan();
                    return Action::Redraw;
                }
                Action::Quit
            }
            Key::Tab => {
                let step = if key.modifiers.shift { -1 } else { 1 };
                self.cycle_view(step);
                Action::Redraw
            }
            Key::Down => {
                self.scroll_list_by(1);
                Action::Redraw
            }
            Key::Up => {
                self.scroll_list_by(-1);
                Action::Redraw
            }
            Key::PageDown => {
                self.scroll_list_by(self.page_rows(size));
                Action::Redraw
            }
            Key::PageUp => {
                self.scroll_list_by(self.page_rows(size).saturating_neg());
                Action::Redraw
            }
            Key::Home => {
                self.scroll_list_to_top();
                Action::Redraw
            }
            Key::Backspace => {
                // Up one level, which is what Backspace does in every file
                // manager in the tree.
                if self.breadcrumbs.len() < 2 {
                    return Action::None;
                }
                self.navigate_breadcrumb(self.breadcrumbs.len().saturating_sub(2), size);
                Action::Redraw
            }
            Key::F5 => {
                let root = self.config.scan_path.clone();
                self.start_scan(root);
                Action::Redraw
            }
            _ => Action::None,
        }
    }

    /// Keys while the path field has focus.
    fn handle_path_key(&mut self, key: &KeyEvent, size: (f32, f32)) -> Action {
        match key.key {
            Key::Escape => {
                // Put back what is actually being shown, rather than leaving a
                // half-typed path in a field that no longer describes anything.
                self.path_input = self.config.scan_path.display().to_string();
                self.path_focused = false;
                Action::Redraw
            }
            Key::Enter => {
                self.path_focused = false;
                if self.path_input.is_empty() {
                    return Action::Redraw;
                }
                self.start_scan(PathBuf::from(&self.path_input));
                Action::Redraw
            }
            Key::Backspace => {
                if self.path_input.pop().is_some() {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            _ => {
                // Every character the keystroke typed, not just the first: one
                // press can produce several (a dead key composing, a paste
                // delivered as text), and taking only the first would silently
                // drop the rest.
                let before = self.path_input.len();
                for c in key.typed() {
                    if self.path_input.len() >= MAX_PATH_INPUT {
                        break;
                    }
                    self.path_input.push(c);
                }
                let _ = size;
                if self.path_input.len() == before {
                    Action::None
                } else {
                    Action::Redraw
                }
            }
        }
    }

    /// Move the view tab `step` places, wrapping.
    fn cycle_view(&mut self, step: isize) {
        let len = ViewMode::ALL.len();
        let here = ViewMode::ALL
            .iter()
            .position(|(m, _)| *m == self.view_mode)
            .unwrap_or(0);
        #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
        let next = (here as isize)
            .saturating_add(step)
            .rem_euclid(len.max(1) as isize) as usize;
        if let Some((mode, _)) = ViewMode::ALL.get(next) {
            self.set_view_mode(*mode);
        }
    }

    /// How many rows one Page key moves.
    fn page_rows(&self, size: (f32, f32)) -> isize {
        let area = list_area(size);
        let capacity = scroll_window::capacity(ROW_HEIGHT, area.h - TABLE_HEADER_HEIGHT);
        isize::try_from(capacity.max(1)).unwrap_or(1)
    }

    /// Act on any event the window loop hands over.
    pub fn handle_event(&mut self, event: &Event, size: (f32, f32)) -> Action {
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Press(button) => self.handle_click(mouse.x, mouse.y, button, size),
                MouseEventKind::Move => {
                    let before = (self.hovered_rect, self.tooltip_text.clone());
                    if self.view_mode == ViewMode::Treemap {
                        self.hover_treemap(mouse.x, mouse.y);
                    }
                    if before == (self.hovered_rect, self.tooltip_text.clone()) {
                        Action::None
                    } else {
                        Action::Redraw
                    }
                }
                MouseEventKind::Leave => {
                    if self.hovered_rect.is_none() && self.tooltip_text.is_empty() {
                        return Action::None;
                    }
                    self.hovered_rect = None;
                    self.tooltip_text.clear();
                    Action::Redraw
                }
                MouseEventKind::Scroll { dy, .. } => {
                    let rows = self.wheel.rows(dy);
                    if rows == 0 {
                        return Action::None;
                    }
                    // Not negated: `Accumulator::rows` already returns a delta
                    // in the direction of a row index, having done the flip
                    // from "positive dy is away from the user" itself. Negating
                    // it again turns every scroll into a scroll the wrong way,
                    // which at the top of a list looks exactly like a wheel
                    // that does not work at all.
                    self.scroll_list_by(rows);
                    Action::Redraw
                }
                _ => Action::None,
            },
            Event::Key(key) => self.handle_key(key, size),
            Event::Tick { .. } => {
                if self.poll_scan(size) {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            Event::CloseRequested => Action::Quit,
            _ => Action::None,
        }
    }
}

// ============================================================================
// What a handler asks the window to do next
// ============================================================================

/// The outcome of handing an event to [`DiskAnalyzerUI`].
///
/// Separate from [`Response`] so the state machine can be tested without a
/// compositor, and so the one place that translates between them is the `App`
/// impl below rather than every handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing the user can see changed.
    None,
    /// Something changed; the window needs repainting.
    Redraw,
    /// Close the window.
    Quit,
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

// ============================================================================
// Window integration
// ============================================================================

impl App for DiskAnalyzerUI {
    fn title(&self) -> String {
        "Disk Usage Analyzer".to_string()
    }

    fn app_id(&self) -> String {
        "diskanalyzer".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        }
    }

    /// Ask to be woken ten times a second.
    ///
    /// Without this the app receives no [`Event::Tick`] at all, and a scan runs
    /// to completion on its thread with nothing ever collecting the result: the
    /// window would sit on "Scanning…" forever over a finished scan. The
    /// interval is the progress readout's refresh rate, not the scan's — the
    /// walk runs at its own speed on its own thread — so it is chosen to be
    /// fast enough that the byte counter looks live and slow enough that an
    /// idle window is not repainting for nothing.
    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_millis(100))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        // Resize is handled here rather than in `handle_event` because it is
        // the one event that changes the layout instead of reacting to it: the
        // treemap rectangles are absolute pixel coordinates, so they have to be
        // recomputed before anything hit-tests against them.
        if let Event::Resize { width, height } = *event {
            let size = (
                f64::from(width).max(f64::from(MIN_WIDTH)) as f32,
                f64::from(height).max(f64::from(MIN_HEIGHT)) as f32,
            );
            self.window_size = size;
            self.recompute(size);
            return Response::Redraw;
        }
        let size = self.window_size;
        match self.handle_event(event, size) {
            Action::None => Response::Idle,
            Action::Redraw => Response::Redraw,
            Action::Quit => Response::Exit,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The handed size wins over the recorded one and is written back: the
        // first frame is drawn before any `Event::Resize` arrives, so trusting
        // the record would lay the first window out at a size it is not.
        if self.window_size != (width, height) {
            self.window_size = (width, height);
            self.recompute((width, height));
        }
        self.frame(width, height).into_tree()
    }
}

impl Probe for DiskAnalyzerUI {
    type Target = Target;
    type Outcome = Action;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> guitk::frame::Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.handle_click(x, y, button, size)
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.handle_key(key, size)
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    let mut ui = DiskAnalyzerUI::new();

    // The scan root is the first argument, so the file manager can hand this
    // program a directory ("Analyze disk usage…") rather than always opening on
    // `/` and making the user retype where they already were. Taken as an
    // `OsString`: a path is bytes, and a lossy conversion here would send the
    // scan to a directory that is not the one named on the command line.
    if let Some(root) = std::env::args_os().nth(1) {
        ui.config.scan_path = PathBuf::from(root);
        ui.path_input = ui.config.scan_path.display().to_string();
    }

    // Start scanning immediately rather than opening on an empty window with a
    // button on it. A tool whose entire purpose is one long-running operation
    // should be doing it by the time the user has finished looking at the
    // window; the Scan button then reads "Cancel", which is the control they
    // actually want in the first second.
    let root = ui.config.scan_path.clone();
    ui.start_scan(root);

    app::launch("diskanalyzer", &mut ui)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // -- helpers ---------------------------------------------------------------

    /// Build a small example tree for testing.
    fn sample_tree() -> FileNode {
        let mut root = FileNode::new_dir("root", "/root");

        let mut docs = FileNode::new_dir("docs", "/root/docs");
        docs.add_child(FileNode::new_file(
            "readme.txt",
            "/root/docs/readme.txt",
            1024,
        ));
        docs.add_child(FileNode::new_file("spec.pdf", "/root/docs/spec.pdf", 5120));
        root.add_child(docs);

        let mut src = FileNode::new_dir("src", "/root/src");
        src.add_child(FileNode::new_file("main.rs", "/root/src/main.rs", 2048));
        src.add_child(FileNode::new_file("lib.rs", "/root/src/lib.rs", 4096));
        src.add_child(FileNode::new_file("util.rs", "/root/src/util.rs", 1024));
        root.add_child(src);

        root.add_child(FileNode::new_file("logo.png", "/root/logo.png", 8192));

        root
    }

    /// Build a larger tree for treemap tests.
    fn large_tree() -> FileNode {
        let mut root = FileNode::new_dir("home", "/home");

        let mut videos = FileNode::new_dir("videos", "/home/videos");
        videos.add_child(FileNode::new_file(
            "movie.mp4",
            "/home/videos/movie.mp4",
            1_000_000,
        ));
        videos.add_child(FileNode::new_file(
            "clip.mkv",
            "/home/videos/clip.mkv",
            500_000,
        ));
        root.add_child(videos);

        let mut music = FileNode::new_dir("music", "/home/music");
        music.add_child(FileNode::new_file(
            "song.mp3",
            "/home/music/song.mp3",
            300_000,
        ));
        music.add_child(FileNode::new_file(
            "album.flac",
            "/home/music/album.flac",
            700_000,
        ));
        root.add_child(music);

        let mut code = FileNode::new_dir("code", "/home/code");
        code.add_child(FileNode::new_file("app.rs", "/home/code/app.rs", 10_000));
        code.add_child(FileNode::new_file("test.rs", "/home/code/test.rs", 5_000));
        code.add_child(FileNode::new_file(
            "data.json",
            "/home/code/data.json",
            20_000,
        ));
        root.add_child(code);

        root.add_child(FileNode::new_file(
            "archive.zip",
            "/home/archive.zip",
            200_000,
        ));
        root.add_child(FileNode::new_file("photo.jpg", "/home/photo.jpg", 150_000));

        root
    }

    // -- FileNode tests --------------------------------------------------------

    #[test]
    fn test_file_node_new_file() {
        let f = FileNode::new_file("test.txt", "/tmp/test.txt", 42);
        assert_eq!(f.name, "test.txt");
        assert_eq!(f.path, Path::new("/tmp/test.txt"));
        assert_eq!(f.size_bytes, 42);
        assert_eq!(f.kind, FileKind::RegularFile);
        assert!(f.children.is_empty());
    }

    #[test]
    fn test_file_node_new_dir() {
        let d = FileNode::new_dir("tmp", "/tmp");
        assert_eq!(d.kind, FileKind::Directory);
        assert_eq!(d.size_bytes, 0);
        assert!(d.is_dir());
    }

    #[test]
    fn test_file_node_new_symlink() {
        let s = FileNode::new_symlink("link", "/tmp/link", 100);
        assert_eq!(s.kind, FileKind::Symlink);
        assert_eq!(s.size_bytes, 100);
    }

    #[test]
    fn test_file_node_new_other() {
        let o = FileNode::new_other("dev", "/dev/null", 0);
        assert_eq!(o.kind, FileKind::Other);
    }

    #[test]
    fn test_file_node_add_child() {
        let mut d = FileNode::new_dir("dir", "/dir");
        d.add_child(FileNode::new_file("a.txt", "/dir/a.txt", 10));
        d.add_child(FileNode::new_file("b.txt", "/dir/b.txt", 20));
        assert_eq!(d.children.len(), 2);
    }

    #[test]
    fn test_extension_simple() {
        let f = FileNode::new_file("photo.JPG", "/photo.JPG", 100);
        assert_eq!(f.extension(), "jpg");
    }

    #[test]
    fn test_extension_no_ext() {
        let f = FileNode::new_file("Makefile", "/Makefile", 50);
        assert_eq!(f.extension(), "");
    }

    #[test]
    fn test_extension_multiple_dots() {
        let f = FileNode::new_file("archive.tar.gz", "/archive.tar.gz", 999);
        assert_eq!(f.extension(), "gz");
    }

    #[test]
    fn test_extension_directory() {
        let d = FileNode::new_dir("mydir.d", "/mydir.d");
        assert_eq!(d.extension(), "");
    }

    #[test]
    fn test_file_count_single_file() {
        let f = FileNode::new_file("a.txt", "/a.txt", 10);
        assert_eq!(f.file_count(), 1);
    }

    #[test]
    fn test_file_count_tree() {
        let tree = sample_tree();
        // readme.txt, spec.pdf, main.rs, lib.rs, util.rs, logo.png = 6
        assert_eq!(tree.file_count(), 6);
    }

    #[test]
    fn test_dir_count_tree() {
        let tree = sample_tree();
        // root, docs, src = 3
        assert_eq!(tree.dir_count(), 3);
    }

    // -- Tree scanning tests ---------------------------------------------------

    #[test]
    fn test_calculate_sizes() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        // docs: 1024 + 5120 = 6144
        // src: 2048 + 4096 + 1024 = 7168
        // root: 6144 + 7168 + 8192 = 21504
        assert_eq!(root.size_bytes, 21504);
    }

    #[test]
    fn test_calculate_sizes_nested() {
        let mut root = FileNode::new_dir("r", "/r");
        let mut a = FileNode::new_dir("a", "/r/a");
        let mut b = FileNode::new_dir("b", "/r/a/b");
        b.add_child(FileNode::new_file("f.txt", "/r/a/b/f.txt", 100));
        a.add_child(b);
        root.add_child(a);
        calculate_sizes(&mut root);
        assert_eq!(root.size_bytes, 100);
    }

    #[test]
    fn test_summarize_tree_total_size() {
        let mut root = sample_tree();
        let tree = summarize_tree(&mut root);
        assert_eq!(tree.total_size, 21504);
    }

    #[test]
    fn test_summarize_tree_counts() {
        let mut root = sample_tree();
        let tree = summarize_tree(&mut root);
        assert_eq!(tree.file_count, 6);
        assert_eq!(tree.dir_count, 3);
    }

    #[test]
    fn test_assign_depths() {
        let mut root = sample_tree();
        assign_depths(&mut root, 0);
        assert_eq!(root.depth, 0);
        assert_eq!(root.children[0].depth, 1); // docs
        assert_eq!(root.children[0].children[0].depth, 2); // readme.txt
    }

    #[test]
    fn test_scan_empty_dir() {
        let mut root = FileNode::new_dir("empty", "/empty");
        let tree = summarize_tree(&mut root);
        assert_eq!(tree.total_size, 0);
        assert_eq!(tree.file_count, 0);
        assert_eq!(tree.dir_count, 1);
    }

    // -- find_largest tests ----------------------------------------------------

    #[test]
    fn the_largest_view_ranks_files_by_size() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        assign_depths(&mut root, 0);
        let top = find_largest(&root, 3, root.size_bytes);
        assert_eq!(top.len(), 3);
        let names: Vec<&str> = top.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["logo.png", "spec.pdf", "lib.rs"]);
        assert_eq!(top[0].size_bytes, 8192);
    }

    #[test]
    fn the_largest_view_lists_files_and_not_the_directories_holding_them() {
        // The old ranking put the *root* at the top with the sum of everything
        // under it, then its biggest subdirectory, and so on — a list whose
        // first entry is always "the whole disk" and whose entries overlap.
        // Nothing there tells a user what to delete.
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        assign_depths(&mut root, 0);
        let top = find_largest(&root, 50, root.size_bytes);
        assert!(top.iter().all(|r| r.kind != FileKind::Directory));
        assert_eq!(top.len(), 6, "sample_tree holds six files");
    }

    #[test]
    fn a_percentage_in_the_largest_view_is_a_share_of_the_whole_scan() {
        // Not of the directory the file sits in: a file's share of the *disk*
        // is what decides whether deleting it is worth doing.
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        assign_depths(&mut root, 0);
        let top = find_largest(&root, 1, root.size_bytes);
        let expected = 8192.0 / 21504.0 * 100.0;
        assert!((top[0].percentage - expected).abs() < 0.01);
    }

    #[test]
    fn asking_for_more_of_the_largest_than_exist_yields_what_there_is() {
        let f = FileNode::new_file("solo.txt", "/solo.txt", 42);
        let top = find_largest(&f, 100, 42);
        assert_eq!(top.len(), 1);
    }

    // -- Treemap layout tests --------------------------------------------------

    #[test]
    fn test_treemap_basic() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        assign_depths(&mut root, 0);
        let rects = compute_treemap(&root, 0.0, 0.0, 400.0, 300.0);
        assert!(!rects.is_empty());
        // Should have one rect per child of root.
        assert_eq!(rects.len(), 3); // docs dir, src dir, logo.png
    }

    #[test]
    fn test_treemap_covers_area() {
        let mut root = large_tree();
        calculate_sizes(&mut root);
        assign_depths(&mut root, 0);
        let rects = compute_treemap(&root, 0.0, 0.0, 800.0, 600.0);

        // All rects should be within bounds.
        for rect in &rects {
            assert!(rect.x >= 0.0, "x={} out of bounds", rect.x);
            assert!(rect.y >= 0.0, "y={} out of bounds", rect.y);
            assert!(
                rect.x + rect.width <= 801.0,
                "right edge {} out of bounds",
                rect.x + rect.width
            );
            assert!(
                rect.y + rect.height <= 601.0,
                "bottom edge {} out of bounds",
                rect.y + rect.height
            );
        }
    }

    #[test]
    fn test_treemap_empty() {
        let root = FileNode::new_dir("empty", "/empty");
        let rects = compute_treemap(&root, 0.0, 0.0, 400.0, 300.0);
        assert!(rects.is_empty());
    }

    #[test]
    fn test_treemap_single_child() {
        let mut root = FileNode::new_dir("r", "/r");
        root.add_child(FileNode::new_file("f.txt", "/r/f.txt", 100));
        calculate_sizes(&mut root);
        assign_depths(&mut root, 0);
        let rects = compute_treemap(&root, 10.0, 20.0, 300.0, 200.0);
        assert_eq!(rects.len(), 1);
        let r = &rects[0];
        assert!((r.x - 10.0).abs() < 0.01);
        assert!((r.y - 20.0).abs() < 0.01);
        assert!((r.width - 300.0).abs() < 0.01);
        assert!((r.height - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_treemap_tiny_rect_skipped() {
        let root = FileNode::new_dir("r", "/r");
        let rects = compute_treemap(&root, 0.0, 0.0, 2.0, 2.0);
        assert!(rects.is_empty());
    }

    #[test]
    fn test_treemap_proportional_sizes() {
        let mut root = FileNode::new_dir("r", "/r");
        root.add_child(FileNode::new_file("big.dat", "/r/big.dat", 900));
        root.add_child(FileNode::new_file("small.dat", "/r/small.dat", 100));
        calculate_sizes(&mut root);
        assign_depths(&mut root, 0);

        let rects = compute_treemap(&root, 0.0, 0.0, 1000.0, 100.0);
        assert_eq!(rects.len(), 2);

        let big_area = rects[0].width * rects[0].height;
        let small_area = rects[1].width * rects[1].height;
        // Big should be roughly 9x the area of small.
        let ratio = big_area / small_area;
        assert!(
            ratio > 7.0 && ratio < 11.0,
            "area ratio {ratio} expected ~9.0"
        );
    }

    // -- Hit test --------------------------------------------------------------

    #[test]
    fn test_hit_test_basic() {
        let rects = vec![
            TreemapRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                node_index: 0,
                depth: 1,
                color: COLOR_BLUE,
                path: PathBuf::from("/a"),
                name: "a".to_string(),
                size_bytes: 100,
                is_dir: false,
            },
            TreemapRect {
                x: 100.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                node_index: 1,
                depth: 1,
                color: COLOR_RED,
                path: PathBuf::from("/b"),
                name: "b".to_string(),
                size_bytes: 200,
                is_dir: false,
            },
        ];
        assert_eq!(treemap_hit_test(&rects, 50.0, 50.0), Some(0));
        assert_eq!(treemap_hit_test(&rects, 150.0, 50.0), Some(1));
        assert_eq!(treemap_hit_test(&rects, 250.0, 50.0), None);
    }

    #[test]
    fn test_hit_test_empty() {
        assert_eq!(treemap_hit_test(&[], 10.0, 10.0), None);
    }

    #[test]
    fn test_hit_test_edge() {
        let rects = vec![TreemapRect {
            x: 10.0,
            y: 20.0,
            width: 50.0,
            height: 30.0,
            node_index: 0,
            depth: 0,
            color: COLOR_BLUE,
            path: PathBuf::from("/x"),
            name: "x".to_string(),
            size_bytes: 50,
            is_dir: false,
        }];
        // Exact top-left corner should hit.
        assert_eq!(treemap_hit_test(&rects, 10.0, 20.0), Some(0));
        // Just outside right edge.
        assert_eq!(treemap_hit_test(&rects, 60.0, 20.0), None);
        // Just outside bottom edge.
        assert_eq!(treemap_hit_test(&rects, 10.0, 50.0), None);
    }

    // -- Extension stats -------------------------------------------------------

    #[test]
    fn test_extension_stats_basic() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        let stats = compute_extension_stats(&root);
        assert!(!stats.is_empty());
        // Should be sorted by size descending.
        for i in 1..stats.len() {
            assert!(stats[i - 1].total_size >= stats[i].total_size);
        }
    }

    #[test]
    fn test_extension_stats_percentages() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        let stats = compute_extension_stats(&root);
        let total_pct: f32 = stats.iter().map(|s| s.percentage).sum();
        // Should sum to approximately 100%.
        assert!(
            (total_pct - 100.0).abs() < 1.0,
            "percentages sum to {total_pct}"
        );
    }

    #[test]
    fn test_extension_stats_empty_tree() {
        let root = FileNode::new_dir("empty", "/empty");
        let stats = compute_extension_stats(&root);
        assert!(stats.is_empty());
    }

    #[test]
    fn test_extension_stats_counts() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        let stats = compute_extension_stats(&root);
        let rs_stat = stats.iter().find(|s| s.extension == "rs");
        assert!(rs_stat.is_some());
        let rs_stat = rs_stat.unwrap();
        assert_eq!(rs_stat.count, 3); // main.rs, lib.rs, util.rs
        assert_eq!(rs_stat.total_size, 7168);
    }

    // -- Size formatting -------------------------------------------------------

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_size_kib() {
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(2048), "2.0 KiB");
    }

    #[test]
    fn test_format_size_mib() {
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(1_500_000), "1.4 MiB");
    }

    #[test]
    fn test_format_size_gib() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GiB");
    }

    #[test]
    fn test_format_size_tib() {
        assert_eq!(format_size(1024u64 * 1024 * 1024 * 1024), "1.0 TiB");
    }

    // -- Sorting ---------------------------------------------------------------

    #[test]
    fn test_sort_rows_by_size_desc() {
        let mut rows = vec![
            ListRow {
                name: "small".to_string(),
                path: PathBuf::from("/small"),
                size_bytes: 100,
                percentage: 10.0,
                kind: FileKind::RegularFile,
                is_expanded: false,
                depth: 0,
                has_children: false,
            },
            ListRow {
                name: "big".to_string(),
                path: PathBuf::from("/big"),
                size_bytes: 1000,
                percentage: 90.0,
                kind: FileKind::RegularFile,
                is_expanded: false,
                depth: 0,
                has_children: false,
            },
        ];
        sort_rows(&mut rows, SortColumn::Size, SortDirection::Descending);
        assert_eq!(rows[0].name, "big");
        assert_eq!(rows[1].name, "small");
    }

    #[test]
    fn test_sort_rows_by_name_asc() {
        let mut rows = vec![
            ListRow {
                name: "Zebra".to_string(),
                path: PathBuf::from("/z"),
                size_bytes: 1,
                percentage: 50.0,
                kind: FileKind::RegularFile,
                is_expanded: false,
                depth: 0,
                has_children: false,
            },
            ListRow {
                name: "Apple".to_string(),
                path: PathBuf::from("/a"),
                size_bytes: 2,
                percentage: 50.0,
                kind: FileKind::RegularFile,
                is_expanded: false,
                depth: 0,
                has_children: false,
            },
        ];
        sort_rows(&mut rows, SortColumn::Name, SortDirection::Ascending);
        assert_eq!(rows[0].name, "Apple");
        assert_eq!(rows[1].name, "Zebra");
    }

    #[test]
    fn test_sort_rows_by_percentage() {
        let mut rows = vec![
            ListRow {
                name: "a".to_string(),
                path: PathBuf::from("/a"),
                size_bytes: 10,
                percentage: 80.0,
                kind: FileKind::RegularFile,
                is_expanded: false,
                depth: 0,
                has_children: false,
            },
            ListRow {
                name: "b".to_string(),
                path: PathBuf::from("/b"),
                size_bytes: 20,
                percentage: 20.0,
                kind: FileKind::RegularFile,
                is_expanded: false,
                depth: 0,
                has_children: false,
            },
        ];
        sort_rows(&mut rows, SortColumn::Percentage, SortDirection::Ascending);
        assert_eq!(rows[0].name, "b");
    }

    #[test]
    fn test_sort_rows_by_type() {
        let mut rows = vec![
            ListRow {
                name: "dir".to_string(),
                path: PathBuf::from("/dir"),
                size_bytes: 100,
                percentage: 50.0,
                kind: FileKind::Directory,
                is_expanded: false,
                depth: 0,
                has_children: true,
            },
            ListRow {
                name: "a.txt".to_string(),
                path: PathBuf::from("/a.txt"),
                size_bytes: 50,
                percentage: 50.0,
                kind: FileKind::RegularFile,
                is_expanded: false,
                depth: 0,
                has_children: false,
            },
        ];
        sort_rows(&mut rows, SortColumn::Type, SortDirection::Ascending);
        // "Directory" < "File" lexicographically
        assert_eq!(rows[0].name, "dir");
    }

    // -- Filtering / flattening ------------------------------------------------

    #[test]
    fn test_flatten_tree_no_expansion() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        assign_depths(&mut root, 0);
        let rows = flatten_tree(&root, root.size_bytes, &[]);
        // Only root + immediate children (docs, src, logo.png), but not
        // their sub-children since nothing is expanded.
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn test_flatten_tree_with_expansion() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        assign_depths(&mut root, 0);
        let expanded = vec![PathBuf::from("/root"), PathBuf::from("/root/docs")];
        let rows = flatten_tree(&root, root.size_bytes, &expanded);
        // root + docs + readme.txt + spec.pdf + src + logo.png = 6
        // (src children not expanded)
        assert_eq!(rows.len(), 6);
    }

    // -- Config tests ----------------------------------------------------------

    #[test]
    fn the_default_scan_is_the_whole_tree_with_the_built_in_caps() {
        let cfg = AnalyzerConfig::default();
        assert_eq!(cfg.scan_path, Path::new("/"));
        assert_eq!(cfg.max_scan_depth, 0);
        assert_eq!(cfg.max_entries, scan::DEFAULT_MAX_ENTRIES);
        assert_eq!(cfg.top_n, 50);
    }

    #[test]
    fn a_depth_of_zero_in_the_config_still_reaches_the_scanners_ceiling() {
        // Zero means "no limit of my own", not "do not descend" — a config that
        // read literally would scan the root directory and nothing under it,
        // and the window would report the disk as almost empty.
        let cfg = AnalyzerConfig::default();
        assert_eq!(cfg.limits().depth_ceiling(), scan::MAX_DEPTH);
    }

    #[test]
    fn the_config_hands_its_limits_to_the_scanner_unchanged() {
        let cfg = AnalyzerConfig {
            scan_path: PathBuf::from("/home"),
            max_scan_depth: 5,
            max_entries: 1000,
            top_n: 20,
        };
        let limits = cfg.limits();
        assert_eq!(limits.max_depth, 5);
        assert_eq!(limits.max_entries, 1000);
        assert_eq!(limits.depth_ceiling(), 5);
    }

    // -- UI state management tests ---------------------------------------------

    #[test]
    fn test_ui_initial_state() {
        let ui = DiskAnalyzerUI::new();
        assert_eq!(ui.view_mode, ViewMode::Treemap);
        assert!(ui.dir_tree.is_none());
        assert!(ui.treemap_rects.is_empty());
        assert!(ui.extension_stats.is_empty());
        assert!(!ui.scanning());
    }

    #[test]
    fn test_ui_set_view_mode() {
        let mut ui = DiskAnalyzerUI::new();
        ui.set_view_mode(ViewMode::List);
        assert_eq!(ui.view_mode, ViewMode::List);
        ui.set_view_mode(ViewMode::Extensions);
        assert_eq!(ui.view_mode, ViewMode::Extensions);
    }

    #[test]
    fn test_ui_load_tree() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        assert!(ui.dir_tree.is_some());
        assert!(!ui.treemap_rects.is_empty());
        assert!(!ui.extension_stats.is_empty());
        assert!(!ui.scanning());
    }

    #[test]
    fn test_ui_set_sort() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        ui.set_sort(SortColumn::Name);
        assert_eq!(ui.sort_column, SortColumn::Name);
        assert_eq!(ui.sort_direction, SortDirection::Descending);
        // Click same column again toggles direction.
        ui.set_sort(SortColumn::Name);
        assert_eq!(ui.sort_direction, SortDirection::Ascending);
    }

    #[test]
    fn test_ui_toggle_expand() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        // The root is always expanded, so its immediate children (docs, src,
        // logo.png) are visible from the start. Expanding a child directory
        // reveals that directory's contents.
        let initial_rows = ui.list_rows.len();
        ui.toggle_expand(Path::new("/root/src"));
        assert!(ui.expanded_paths.contains(&PathBuf::from("/root/src")));
        let expanded_rows = ui.list_rows.len();
        assert!(expanded_rows > initial_rows);
        // Collapse.
        ui.toggle_expand(Path::new("/root/src"));
        assert!(!ui.expanded_paths.contains(&PathBuf::from("/root/src")));
    }

    #[test]
    fn test_ui_hover_treemap() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(large_tree());
        // Hover over a known rect area.
        if let Some(rect) = ui.treemap_rects.first() {
            let mx = rect.x + rect.width / 2.0;
            let my = rect.y + rect.height / 2.0;
            ui.hover_treemap(mx, my);
            assert!(ui.hovered_rect.is_some());
            assert!(!ui.tooltip_text.is_empty());
        }
    }

    #[test]
    fn test_ui_hover_treemap_miss() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(large_tree());
        ui.hover_treemap(-100.0, -100.0);
        assert!(ui.hovered_rect.is_none());
        assert!(ui.tooltip_text.is_empty());
    }

    #[test]
    fn test_ui_breadcrumb_navigation() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        assert!(ui.drill_down("docs", SIZE));
        assert_eq!(ui.breadcrumbs, ["root", "docs"]);
        ui.navigate_breadcrumb(0, SIZE);
        assert_eq!(ui.breadcrumbs, ["root"]);
    }

    #[test]
    fn drilling_into_something_that_is_not_a_child_directory_is_refused() {
        // The trail used to accept any name at all, so one click on a file —
        // or on a stale rectangle — left the breadcrumbs pointing at a
        // directory that does not exist, and every view below went blank with
        // no way back except the breadcrumb the user had just broken.
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        assert!(!ui.drill_down("nonexistent", SIZE));
        assert!(
            !ui.drill_down("logo.png", SIZE),
            "a file is not a directory"
        );
        assert_eq!(ui.breadcrumbs, ["root"]);
    }

    #[test]
    fn drilling_down_shows_that_directorys_contents_and_not_the_roots() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        assert!(ui.drill_down("src", SIZE));
        let names: Vec<&str> = ui.list_rows.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"main.rs"), "got {names:?}");
        assert!(!names.contains(&"logo.png"), "root's files leaked in");
    }

    #[test]
    fn the_largest_view_can_jump_to_a_directory_anywhere_in_the_tree() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        assert!(ui.navigate_to_path(Path::new("/root/docs"), SIZE));
        assert_eq!(ui.breadcrumbs, ["root", "docs"]);
    }

    #[test]
    fn jumping_to_a_path_that_is_not_a_directory_leaves_the_view_alone() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        assert!(!ui.navigate_to_path(Path::new("/root/logo.png"), SIZE));
        assert!(!ui.navigate_to_path(Path::new("/nowhere"), SIZE));
        assert_eq!(ui.breadcrumbs, ["root"]);
    }

    // -- Rendering tests -------------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let ui = DiskAnalyzerUI::new();
        let tree = ui.frame(SIZE.0, SIZE.1).into_tree();
        // Should always have at least the background rect, toolbar, etc.
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_treemap_view() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        ui.set_view_mode(ViewMode::Treemap);
        let tree = ui.frame(SIZE.0, SIZE.1).into_tree();
        // Should have fill rects for treemap cells + toolbar + background + etc.
        assert!(tree.len() > 10);
    }

    #[test]
    fn test_render_list_view() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        ui.set_view_mode(ViewMode::List);
        let tree = ui.frame(SIZE.0, SIZE.1).into_tree();
        assert!(tree.len() > 10);
    }

    #[test]
    fn test_render_extension_view() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        ui.set_view_mode(ViewMode::Extensions);
        let tree = ui.frame(SIZE.0, SIZE.1).into_tree();
        assert!(tree.len() > 5);
    }

    #[test]
    fn test_render_with_tooltip() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(large_tree());
        // Trigger tooltip.
        if let Some(rect) = ui.treemap_rects.first() {
            let mx = rect.x + 5.0;
            let my = rect.y + 5.0;
            ui.hover_treemap(mx, my);
        }
        let tree = ui.frame(SIZE.0, SIZE.1).into_tree();
        // Should have tooltip-related fill rects.
        assert!(tree.len() > 15);
    }

    /// Every frame in these tests is drawn at the window's default size.
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    // -- Color helpers ---------------------------------------------------------

    #[test]
    fn test_color_for_extension_videos() {
        assert_eq!(color_for_extension("mp4"), COLOR_BLUE);
        assert_eq!(color_for_extension("mkv"), COLOR_BLUE);
    }

    #[test]
    fn test_color_for_extension_images() {
        assert_eq!(color_for_extension("png"), COLOR_GREEN);
        assert_eq!(color_for_extension("jpg"), COLOR_GREEN);
    }

    #[test]
    fn test_color_for_extension_code() {
        assert_eq!(color_for_extension("rs"), COLOR_PEACH);
        assert_eq!(color_for_extension("py"), COLOR_PEACH);
    }

    #[test]
    fn test_color_for_extension_archives() {
        assert_eq!(color_for_extension("zip"), COLOR_RED);
        assert_eq!(color_for_extension("tar"), COLOR_RED);
    }

    #[test]
    fn test_color_for_extension_unknown() {
        assert_eq!(color_for_extension("xyz"), COLOR_SURFACE0);
    }

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

    // -- SortDirection ---------------------------------------------------------

    #[test]
    fn test_sort_direction_toggle() {
        assert_eq!(SortDirection::Ascending.toggle(), SortDirection::Descending);
        assert_eq!(SortDirection::Descending.toggle(), SortDirection::Ascending);
    }

    // -- format_percent --------------------------------------------------------

    #[test]
    fn test_format_percent() {
        assert_eq!(format_percent(50.0), "50.0%");
        assert_eq!(format_percent(99.9), "99.9%");
        assert_eq!(format_percent(0.0), "0.0%");
    }

    // -- Worst aspect ratio helper ---------------------------------------------

    #[test]
    fn test_worst_aspect_square() {
        // A single square item should have aspect ratio 1.0.
        let sizes = [100.0];
        let aspect = worst_aspect_in_row(&sizes, 100.0, 100.0, 100.0);
        assert!((aspect - 1.0).abs() < 0.01);
    }

    // -- File-list column fitting ----------------------------------------------

    fn list_row(name: &str, depth: u32, kind: FileKind, has_children: bool) -> ListRow {
        ListRow {
            name: String::from(name),
            path: PathBuf::from(format!("/root/{name}")),
            size_bytes: 4_294_967_296,
            percentage: 41.25,
            kind,
            is_expanded: has_children,
            depth,
            has_children,
        }
    }

    /// A list holding the long, prefix-sharing names a camera or a backup tool
    /// produces, at a range of tree depths.
    fn ui_with_long_names() -> DiskAnalyzerUI {
        let mut ui = DiskAnalyzerUI::new();
        ui.view_mode = ViewMode::List;
        ui.list_rows = vec![
            list_row("Pictures", 0, FileKind::Directory, true),
            list_row(
                "VID_20260812_143022_HDR_stabilised_final.mp4",
                1,
                FileKind::RegularFile,
                false,
            ),
            list_row(
                "VID_20260812_150907_HDR_stabilised_final.mp4",
                1,
                FileKind::RegularFile,
                false,
            ),
            list_row(
                "backup-of-the-entire-home-directory-2026-08-12T04-00-00Z.tar.zst",
                7,
                FileKind::RegularFile,
                false,
            ),
            list_row("deeply-nested-thing.bin", 40, FileKind::RegularFile, false),
        ];
        ui
    }

    fn list_view_texts(ui: &DiskAnalyzerUI) -> Vec<(f32, String, f32, FontWeightHint)> {
        let mut frame = Frame::new(SIZE.0, SIZE.1);
        ui.render_list_view(&mut frame, SIZE);
        frame
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
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
    fn the_file_list_fills_the_window() {
        // The old columns stopped at x=660 in a 960px window: a third of every
        // row blank, while the only column holding variable-length data was
        // the narrowest it could be.
        let columns = list_columns(SIZE.0);
        let table = Table::with_gap(&columns, 0.0, PADDING);
        let end = table.right(KIND_COL);
        assert!(
            end <= WINDOW_WIDTH - PADDING + 0.01,
            "the table ends at {end}, past the window edge {WINDOW_WIDTH}"
        );
        assert!(
            end >= WINDOW_WIDTH - PADDING - 0.01,
            "the table ends at {end}, leaving {} px of the window unused",
            WINDOW_WIDTH - PADDING - end
        );
    }

    #[test]
    fn no_file_list_cell_escapes_its_column() {
        let ui = ui_with_long_names();
        let columns = list_columns(SIZE.0);
        let table = Table::with_gap(&columns, 0.0, PADDING);
        let spans = table.spans();
        let mut checked = 0usize;
        for (x, drawn, size, weight) in list_view_texts(&ui) {
            let (_, right) = spans
                .iter()
                .copied()
                .find(|(l, r)| x >= l - 0.01 && x <= r + 0.01)
                .unwrap_or_else(|| panic!("cell {drawn:?} at {x} is not inside any column"));
            let ends = x + guitk::text::measure(&drawn, size, weight);
            assert!(
                ends <= right + 0.01,
                "cell {drawn:?} at {x} draws to {ends}, past its column edge {right}"
            );
            checked = checked.saturating_add(1);
        }
        // Four headings, plus chevron/name/size/percent/type per row.
        assert!(checked >= 4 + 5 * 5, "only {checked} cells checked");
    }

    #[test]
    fn a_deeply_nested_row_still_shows_its_name() {
        // Depth is data and unbounded. Uncapped, `depth * DEPTH_INDENT` passes
        // the Name column somewhere around depth 33 and the name elides to the
        // empty string -- the row would go blank rather than narrow.
        let columns = list_columns(SIZE.0);
        let table = Table::with_gap(&columns, 0.0, PADDING);
        let name_width = table.width(NAME_COL);
        for depth in [0_u32, 5, 33, 100, 10_000, u32::MAX] {
            let indent = row_indent(depth, name_width);
            let room = name_width - indent - CHEVRON_WIDTH;
            assert!(
                room >= MIN_NAME_WIDTH - 0.01,
                "at depth {depth} a name gets {room} px, under the {MIN_NAME_WIDTH} floor"
            );
        }

        let ui = ui_with_long_names();
        let deep: Vec<String> = list_view_texts(&ui)
            .into_iter()
            .filter(|(_, t, ..)| t.ends_with(".bin"))
            .map(|(_, t, ..)| t)
            .collect();
        assert_eq!(deep.len(), 1, "the depth-40 row's name vanished: {deep:?}");
    }

    #[test]
    fn a_cut_name_keeps_what_tells_it_apart_from_its_siblings() {
        // Two camera clips from the same day differ only past the 17th
        // character. Cut the usual way they are one string.
        let mut ui = ui_with_long_names();
        // Nest them far enough that the widened Name column still has to cut.
        for row in &mut ui.list_rows {
            row.depth = 20;
        }
        let clips: Vec<String> = list_view_texts(&ui)
            .into_iter()
            .filter(|(_, t, ..)| t.ends_with(".mp4"))
            .map(|(_, t, ..)| t)
            .collect();
        assert_eq!(clips.len(), 2, "expected both clips, got {clips:?}");
        assert!(
            clips.iter().all(|t| t.starts_with('…')),
            "the clips were not actually cut: {clips:?}"
        );
        assert_ne!(clips[0], clips[1], "cut clips collapsed to one string");
    }

    #[test]
    fn a_directory_keeps_its_chevron_however_long_its_name() {
        // The chevron is the only thing on the row saying it can be opened, so
        // it is drawn in its own box: prepended to the name it would be the
        // first thing a front-cut removed.
        let mut ui = ui_with_long_names();
        if let Some(row) = ui.list_rows.first_mut() {
            row.name = String::from(
                "a directory with an extremely long name that cannot possibly fit its column even now",
            );
            row.depth = 20;
        }
        let columns = list_columns(SIZE.0);
        let table = Table::with_gap(&columns, 0.0, PADDING);
        let indent = row_indent(20, table.width(NAME_COL));
        let chevron_x = table.left(NAME_COL) + indent;
        let chevrons: Vec<String> = list_view_texts(&ui)
            .into_iter()
            .filter(|(x, ..)| (x - chevron_x).abs() < 0.01)
            .map(|(_, t, ..)| t)
            .collect();
        assert!(
            chevrons.iter().any(|t| t == "v"),
            "the expanded directory's chevron was lost: {chevrons:?}"
        );
    }

    /// A list-view UI with `n` rows named `row0`..`row{n-1}`, so the drawn
    /// slice can be identified from the render commands alone.
    fn ui_with_numbered_rows(n: usize) -> DiskAnalyzerUI {
        let mut ui = DiskAnalyzerUI::new();
        ui.view_mode = ViewMode::List;
        ui.list_rows = (0..n)
            .map(|i| list_row(&format!("row{i}"), 0, FileKind::RegularFile, false))
            .collect();
        ui
    }

    /// The `rowN` names actually drawn, in the order they were drawn.
    fn drawn_row_names(ui: &DiskAnalyzerUI) -> Vec<String> {
        list_view_texts(ui)
            .into_iter()
            .map(|(_, t, ..)| t)
            .filter(|t| t.starts_with("row"))
            .collect()
    }

    /// How many whole rows the list view has room for.
    fn list_capacity() -> usize {
        let content_h = WINDOW_HEIGHT - TOOLBAR_HEIGHT - BREADCRUMB_HEIGHT - STATUS_BAR_HEIGHT;
        scroll_window::capacity(ROW_HEIGHT, content_h - TABLE_HEADER_HEIGHT)
    }

    #[test]
    fn the_list_view_stops_at_the_last_row_that_fits() {
        let page = list_capacity();
        assert!(page > 0, "the window must fit at least one row");
        let ui = ui_with_numbered_rows(page * 3);
        let drawn = drawn_row_names(&ui);
        assert_eq!(drawn.len(), page, "a long list must be cut to what fits");
        // Nothing may be drawn over the status bar.
        let content_y = TOOLBAR_HEIGHT + BREADCRUMB_HEIGHT;
        let bottom = content_y + TABLE_HEADER_HEIGHT + (page as f32) * ROW_HEIGHT;
        assert!(
            bottom <= WINDOW_HEIGHT - STATUS_BAR_HEIGHT,
            "{page} rows reach {bottom}, past the status bar"
        );
    }

    #[test]
    fn scrolling_the_list_reaches_the_rows_that_did_not_fit() {
        // `scroll_offset` used to be an `f32` that nothing read and nothing
        // wrote: the list was truncated from the top, so everything past the
        // first screenful was unreachable no matter what the user did.
        let page = list_capacity();
        let mut ui = ui_with_numbered_rows(page * 2);
        assert_eq!(
            drawn_row_names(&ui).first().map(String::as_str),
            Some("row0")
        );

        ui.scroll_list_by(3);
        assert_eq!(
            drawn_row_names(&ui).first().map(String::as_str),
            Some("row3"),
            "scrolling by three rows should start the list three rows down"
        );

        // The last row is reachable, which is the whole point.
        ui.scroll_list_by(isize::try_from(page).unwrap());
        let last = format!("row{}", page * 2 - 1);
        assert_eq!(
            drawn_row_names(&ui).last(),
            Some(&last),
            "the end of the list must be reachable by scrolling"
        );

        ui.scroll_list_to_top();
        assert_eq!(
            drawn_row_names(&ui).first().map(String::as_str),
            Some("row0")
        );
    }

    #[test]
    fn a_list_that_shrinks_under_a_stale_offset_shows_its_last_page() {
        let page = list_capacity();
        let mut ui = ui_with_numbered_rows(page * 4);
        ui.scroll_list_by(isize::try_from(page * 3).unwrap());
        // A directory is collapsed and most of the rows go away, without
        // anything resetting the scroll position.
        ui.list_rows.truncate(page + 1);
        let drawn = drawn_row_names(&ui);
        assert_eq!(drawn.len(), page, "the pane must not go blank");
        assert_eq!(
            drawn.last(),
            Some(&format!("row{page}")),
            "a stale offset should pin to the last page, not past the end"
        );
    }

    #[test]
    fn scrolling_up_from_the_top_stays_at_the_top() {
        let mut ui = ui_with_numbered_rows(50);
        ui.scroll_list_by(-1);
        ui.scroll_list_by(isize::MIN);
        assert_eq!(ui.scroll_offset, 0, "the offset must not wrap round");
        assert_eq!(
            drawn_row_names(&ui).first().map(String::as_str),
            Some("row0")
        );
        // ... and scrolling absurdly far down does not overflow either.
        ui.scroll_list_by(isize::MAX);
        ui.scroll_list_by(isize::MAX);
        assert_eq!(drawn_row_names(&ui).len(), list_capacity());
    }

    #[test]
    fn a_short_name_is_drawn_verbatim() {
        let mut ui = DiskAnalyzerUI::new();
        ui.view_mode = ViewMode::List;
        ui.list_rows = vec![list_row("notes.txt", 0, FileKind::RegularFile, false)];
        let drawn = list_view_texts(&ui);
        assert!(
            drawn.iter().any(|(_, t, ..)| t == "notes.txt"),
            "a name that fits was altered: {drawn:?}"
        );
        assert!(
            drawn
                .iter()
                .any(|(_, t, ..)| t == "Directory" || t == "File"),
            "the Type cell went missing: {drawn:?}"
        );
    }

    // ========================================================================
    // Interaction: what a click and a keystroke actually do
    //
    // These go through `guitk::probe`, which finds a control by asking the
    // *rendered frame* where it is and then clicks that point. A control that
    // stopped being drawn, or moved, fails here — which is the whole reason the
    // renderer records its own hit boxes rather than a second layout pass
    // computing them again.
    // ========================================================================

    use guitk::probe::{self, press, press_with};

    /// A UI showing `sample_tree`, laid out at the default window size.
    fn loaded() -> DiskAnalyzerUI {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        ui
    }

    #[test]
    fn every_view_tab_is_on_screen_and_switches_the_view() {
        let mut ui = loaded();
        for (mode, label) in ViewMode::ALL {
            assert!(
                probe::is_visible(&ui, Target::View(*mode)),
                "the {label} tab was not drawn"
            );
            if ui.view_mode == *mode {
                continue;
            }
            assert_eq!(probe::click(&mut ui, Target::View(*mode)), Action::Redraw);
            assert_eq!(ui.view_mode, *mode);
        }
        // And back to the first, which is no longer the one showing.
        let first = ViewMode::ALL[0].0;
        assert_eq!(probe::click(&mut ui, Target::View(first)), Action::Redraw);
        assert_eq!(ui.view_mode, first);
    }

    #[test]
    fn clicking_the_tab_already_showing_asks_for_no_repaint() {
        // Redrawing on a click that changed nothing is how a window ends up
        // repainting continuously under a resting cursor.
        let mut ui = loaded();
        assert_eq!(
            probe::click(&mut ui, Target::View(ViewMode::List)),
            Action::Redraw
        );
        assert_eq!(
            probe::click(&mut ui, Target::View(ViewMode::List)),
            Action::None
        );
    }

    #[test]
    fn switching_views_returns_to_the_top_of_the_list() {
        // Otherwise a scroll offset from a long list survives into a short one
        // and the new view opens past its own end, looking empty.
        let mut ui = loaded();
        ui.set_view_mode(ViewMode::List);
        ui.scroll_list_by(3);
        assert_ne!(ui.scroll_offset, 0);
        probe::click(&mut ui, Target::View(ViewMode::Extensions));
        assert_eq!(ui.scroll_offset, 0);
    }

    #[test]
    fn clicking_a_treemap_tile_drills_into_that_directory() {
        let mut ui = loaded();
        let index = ui
            .treemap_rects
            .iter()
            .position(|r| r.name == "src")
            .expect("src should have a tile");
        assert_eq!(
            probe::click(&mut ui, Target::TreemapRect(index)),
            Action::Redraw
        );
        assert_eq!(ui.breadcrumbs, ["root", "src"]);
    }

    #[test]
    fn clicking_a_treemap_tile_for_a_file_does_nothing_rather_than_breaking_the_trail() {
        let mut ui = loaded();
        let index = ui
            .treemap_rects
            .iter()
            .position(|r| !r.is_dir)
            .expect("sample_tree has files");
        assert_eq!(
            probe::click(&mut ui, Target::TreemapRect(index)),
            Action::None
        );
        assert_eq!(ui.breadcrumbs, ["root"]);
    }

    #[test]
    fn a_breadcrumb_goes_back_up_to_where_it_names() {
        let mut ui = loaded();
        assert!(ui.drill_down("src", SIZE));
        assert!(probe::is_visible(&ui, Target::Breadcrumb(0)));
        assert_eq!(probe::click(&mut ui, Target::Breadcrumb(0)), Action::Redraw);
        assert_eq!(ui.breadcrumbs, ["root"]);
    }

    #[test]
    fn clicking_the_breadcrumb_for_where_you_already_are_asks_for_no_repaint() {
        let mut ui = loaded();
        assert_eq!(probe::click(&mut ui, Target::Breadcrumb(0)), Action::None);
    }

    #[test]
    fn the_chevron_expands_a_row_and_the_rest_of_the_row_does_not() {
        // Two targets on one row, and the chevron is recorded second so it wins
        // the hit test. If they ever collapse into one, clicking a directory's
        // name would expand it in place instead of opening it, and there would
        // be no way to do the other thing at all.
        let mut ui = loaded();
        ui.set_view_mode(ViewMode::List);
        let row = ui
            .visible_rows()
            .iter()
            .position(|r| r.name == "src")
            .expect("src should be a row");
        let before = ui.list_rows.len();
        assert_eq!(
            probe::click(&mut ui, Target::RowChevron(row)),
            Action::Redraw
        );
        assert!(ui.expanded_paths.contains(&PathBuf::from("/root/src")));
        assert!(ui.list_rows.len() > before);
    }

    #[test]
    fn the_chevron_is_drawn_over_the_row_rather_than_beside_it() {
        let mut ui = loaded();
        ui.set_view_mode(ViewMode::List);
        let row = ui
            .visible_rows()
            .iter()
            .position(|r| r.name == "src")
            .expect("src should be a row");
        let chevron = probe::rect_of(&ui, Target::RowChevron(row)).expect("chevron");
        let whole = probe::rect_of(&ui, Target::Row(row)).expect("row");
        assert!(
            whole.contains(chevron.x + 1.0, chevron.y + 1.0),
            "chevron {chevron:?} is outside its row {whole:?}"
        );
        assert!(chevron.w < whole.w, "the chevron swallowed the whole row");
    }

    #[test]
    fn a_row_with_no_children_gets_no_chevron() {
        let mut ui = loaded();
        ui.set_view_mode(ViewMode::List);
        let row = ui
            .visible_rows()
            .iter()
            .position(|r| r.name == "logo.png")
            .expect("logo.png should be a row");
        assert!(probe::is_visible(&ui, Target::Row(row)));
        assert_eq!(probe::click(&mut ui, Target::Row(row)), Action::None);
        assert!(ui.expanded_paths.is_empty());
    }

    #[test]
    fn clicking_a_column_heading_sorts_by_it_and_clicking_again_reverses() {
        let mut ui = loaded();
        ui.set_view_mode(ViewMode::List);
        assert_eq!(
            probe::click(&mut ui, Target::ColumnHeader(SortColumn::Name)),
            Action::Redraw
        );
        assert_eq!(ui.sort_column, SortColumn::Name);
        let first = ui.sort_direction;
        probe::click(&mut ui, Target::ColumnHeader(SortColumn::Name));
        assert_eq!(ui.sort_column, SortColumn::Name);
        assert_ne!(ui.sort_direction, first, "a second click did not reverse");
    }

    #[test]
    fn sorting_the_list_sorts_the_largest_view_too() {
        // They are two vectors holding rows of the same shape. Sorting only the
        // one on screen means switching tabs silently changes the order.
        let mut ui = loaded();
        ui.set_view_mode(ViewMode::List);
        probe::click(&mut ui, Target::ColumnHeader(SortColumn::Name));
        ui.set_view_mode(ViewMode::Largest);
        let names: Vec<&str> = ui.visible_rows().iter().map(|r| r.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        if ui.sort_direction == SortDirection::Descending {
            sorted.reverse();
        }
        assert_eq!(names, sorted);
    }

    #[test]
    fn the_path_field_takes_and_gives_up_the_keyboard() {
        let mut ui = loaded();
        assert!(!ui.path_focused);
        assert_eq!(probe::click(&mut ui, Target::PathInput), Action::Redraw);
        assert!(ui.path_focused);
        // A click on anything else moves on, so the next keystroke is not
        // quietly swallowed by a field the user has stopped looking at.
        probe::click(&mut ui, Target::View(ViewMode::List));
        assert!(!ui.path_focused);
    }

    #[test]
    fn typing_in_the_path_field_edits_the_path_and_not_the_view() {
        let mut ui = loaded();
        probe::click(&mut ui, Target::PathInput);
        ui.path_input.clear();
        probe::type_str(&mut ui, "/home/pics");
        assert_eq!(ui.path_input, "/home/pics");
        // Tab cycles views when the field does *not* have focus; while it does,
        // it must not.
        let view = ui.view_mode;
        probe::key(&mut ui, &press(Key::Tab));
        assert_eq!(ui.view_mode, view);
    }

    #[test]
    fn escape_in_the_path_field_puts_back_what_is_being_shown() {
        let mut ui = loaded();
        let original = ui.path_input.clone();
        probe::click(&mut ui, Target::PathInput);
        probe::type_str(&mut ui, "junk");
        assert_ne!(ui.path_input, original);
        probe::key(&mut ui, &press(Key::Escape));
        assert_eq!(ui.path_input, original);
        assert!(!ui.path_focused);
    }

    #[test]
    fn the_path_field_will_not_grow_without_bound() {
        // An append-on-keystroke handler with no cap is a memory leak with a
        // keyboard attached to it.
        let mut ui = loaded();
        probe::click(&mut ui, Target::PathInput);
        ui.path_input = "x".repeat(MAX_PATH_INPUT);
        probe::type_str(&mut ui, "yz");
        assert_eq!(ui.path_input.len(), MAX_PATH_INPUT);
    }

    #[test]
    fn backspace_in_the_path_field_deletes_a_whole_character_not_a_byte() {
        // `String::pop` is by char, which is what makes this safe; a byte-wise
        // truncate would leave the field holding invalid UTF-8 and panic on the
        // next draw.
        let mut ui = loaded();
        probe::click(&mut ui, Target::PathInput);
        ui.path_input = "/é".to_string();
        probe::key(&mut ui, &press(Key::Backspace));
        assert_eq!(ui.path_input, "/");
    }

    #[test]
    fn tab_and_shift_tab_cycle_the_views_in_opposite_directions() {
        let mut ui = loaded();
        ui.set_view_mode(ViewMode::Treemap);
        probe::key(&mut ui, &press(Key::Tab));
        assert_eq!(ui.view_mode, ViewMode::List);
        probe::key(&mut ui, &probe::shift(Key::Tab));
        assert_eq!(ui.view_mode, ViewMode::Treemap);
        // And it wraps rather than stopping at the end.
        probe::key(&mut ui, &probe::shift(Key::Tab));
        assert_eq!(ui.view_mode, ViewMode::Largest);
    }

    #[test]
    fn backspace_outside_the_path_field_goes_up_one_level() {
        let mut ui = loaded();
        assert!(ui.drill_down("src", SIZE));
        assert_eq!(probe::key(&mut ui, &press(Key::Backspace)), Action::Redraw);
        assert_eq!(ui.breadcrumbs, ["root"]);
        // At the root there is nowhere to go, and nothing to repaint.
        assert_eq!(probe::key(&mut ui, &press(Key::Backspace)), Action::None);
    }

    #[test]
    fn the_arrow_keys_scroll_the_list_and_home_returns_to_the_top() {
        let mut ui = loaded();
        ui.set_view_mode(ViewMode::List);
        probe::key(&mut ui, &press(Key::Down));
        assert_eq!(ui.scroll_offset, 1);
        probe::key(&mut ui, &press(Key::Up));
        assert_eq!(ui.scroll_offset, 0);
        // Scrolling above the top stops there rather than wrapping to the end.
        probe::key(&mut ui, &press(Key::Up));
        assert_eq!(ui.scroll_offset, 0);
        probe::key(&mut ui, &press(Key::PageDown));
        assert!(ui.scroll_offset > 0);
        probe::key(&mut ui, &press(Key::Home));
        assert_eq!(ui.scroll_offset, 0);
    }

    #[test]
    fn a_key_release_does_nothing() {
        // Handling both edges makes every keystroke happen twice.
        let mut ui = loaded();
        let mut release = press(Key::Tab);
        release.pressed = false;
        let view = ui.view_mode;
        assert_eq!(ui.handle_key(&release, SIZE), Action::None);
        assert_eq!(ui.view_mode, view);
    }

    #[test]
    fn ctrl_l_puts_the_cursor_in_the_path_field() {
        let mut ui = loaded();
        assert_eq!(probe::key(&mut ui, &probe::ctrl(Key::L)), Action::Redraw);
        assert!(ui.path_focused);
    }

    #[test]
    fn a_plain_l_is_not_ctrl_l() {
        let mut ui = loaded();
        assert_eq!(probe::key(&mut ui, &press(Key::L)), Action::None);
        assert!(!ui.path_focused);
    }

    #[test]
    fn escape_closes_the_window_when_no_scan_is_running() {
        let mut ui = loaded();
        assert_eq!(probe::key(&mut ui, &press(Key::Escape)), Action::Quit);
    }

    #[test]
    fn only_the_left_button_activates_a_control() {
        // A right-click that switched tabs would fire the moment a context menu
        // was opened over one.
        let mut ui = loaded();
        let view = ui.view_mode;
        assert_eq!(
            probe::click_with(&mut ui, Target::View(ViewMode::List), MouseButton::Right),
            Action::None
        );
        assert_eq!(ui.view_mode, view);
    }

    #[test]
    fn the_wheel_scrolls_the_list_towards_the_end_when_pulled_towards_the_user() {
        let mut ui = loaded();
        ui.set_view_mode(ViewMode::List);
        let event = Event::Mouse(guitk::event::MouseEvent {
            x: 100.0,
            y: 300.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
        });
        assert_eq!(ui.handle_event(&event, SIZE), Action::Redraw);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let expected = wheel::ROWS_PER_NOTCH as usize;
        assert_eq!(ui.scroll_offset, expected);

        // And away from the user takes it back, rather than further on: the
        // accumulator has already flipped the sign, so a handler that flips it
        // again scrolls the wrong way in both directions.
        let back = Event::Mouse(guitk::event::MouseEvent {
            x: 100.0,
            y: 300.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
        });
        ui.handle_event(&back, SIZE);
        assert_eq!(ui.scroll_offset, 0);
    }

    #[test]
    fn a_fraction_of_a_wheel_notch_is_remembered_between_events() {
        // A precision wheel or a touchpad sends fractions. An accumulator built
        // fresh per event rounds every one of them to zero, so the list would
        // refuse to scroll on exactly the hardware that scrolls most smoothly.
        let mut ui = loaded();
        ui.set_view_mode(ViewMode::List);
        let nudge = Event::Mouse(guitk::event::MouseEvent {
            x: 100.0,
            y: 300.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -0.4 },
        });
        for _ in 0..3 {
            ui.handle_event(&nudge, SIZE);
        }
        assert!(ui.scroll_offset > 0, "three fifths of a notch went nowhere");
    }

    // -- What the window says when it has nothing to show ---------------------

    #[test]
    fn an_empty_window_says_which_kind_of_empty_it_is() {
        // Four different situations used to draw the same "No data" message, so
        // a failed scan was indistinguishable from a directory that really is
        // empty, and from one still being scanned.
        let mut ui = DiskAnalyzerUI::new();
        assert_eq!(ui.empty_message(), "No data. Click Scan to begin.");

        ui.scan_error = Some("permission denied".to_string());
        assert!(ui.empty_message().contains("permission denied"));

        ui.scan_error = None;
        ui.load_tree(FileNode::new_dir("empty", "/empty"));
        assert_eq!(ui.empty_message(), "This folder is empty.");
    }

    #[test]
    fn the_status_line_admits_when_the_totals_are_short() {
        let mut ui = loaded();
        assert!(ui.complete);
        assert!(!ui.status_text().contains("PARTIAL"));

        ui.truncated = true;
        ui.complete = false;
        assert!(ui.status_text().contains("PARTIAL"));

        ui.truncated = false;
        ui.cancelled = true;
        assert!(ui.status_text().contains("STOPPED"));

        ui.cancelled = false;
        ui.unreadable_count = 3;
        ui.unreadable = vec![PathBuf::from("/proc/1/mem")];
        let text = ui.status_text();
        assert!(text.contains("3 paths unreadable"), "got {text}");
        assert!(text.contains("/proc/1/mem"), "got {text}");
    }

    #[test]
    fn one_unreadable_path_is_reported_in_the_singular() {
        let mut ui = loaded();
        ui.unreadable_count = 1;
        assert!(ui.status_text().contains("1 path unreadable"));
    }

    #[test]
    fn a_failed_scan_says_so_instead_of_reporting_a_total_of_zero() {
        let mut ui = DiskAnalyzerUI::new();
        ui.scan_error = Some("no such file or directory".to_string());
        let text = ui.status_text();
        assert!(text.starts_with("Scan failed"), "got {text}");
        assert!(!text.contains("Total: 0 B"), "got {text}");
    }

    // -- Layout that has to survive a resize ----------------------------------

    #[test]
    fn every_control_is_still_reachable_in_a_narrow_window() {
        // The layout is computed by subtraction from the window size, and
        // subtraction is what makes a width negative. A negative-width rect is
        // not a small control — `contains` is false everywhere inside it — so
        // it stays on screen and stops being clickable.
        let mut ui = loaded();
        let narrow = (MIN_WIDTH, MIN_HEIGHT);
        for (mode, label) in ViewMode::ALL {
            assert!(
                probe::rect_of_sized(&ui, Target::View(*mode), narrow).is_some(),
                "the {label} tab vanished from a narrow window"
            );
        }
        assert!(probe::rect_of_sized(&ui, Target::PathInput, narrow).is_some());
        assert!(probe::rect_of_sized(&ui, Target::Scan, narrow).is_some());
        ui.set_view_mode(ViewMode::List);
        assert!(probe::rect_of_sized(&ui, Target::Row(0), narrow).is_some());
    }

    #[test]
    fn a_window_smaller_than_the_minimum_is_drawn_at_the_minimum() {
        let ui = loaded();
        let tiny = ui.frame(80.0, 60.0);
        let full = ui.frame(MIN_WIDTH, MIN_HEIGHT);
        assert_eq!(tiny.hits().len(), full.hits().len());
    }

    #[test]
    fn the_file_list_still_ends_one_padding_short_of_a_resized_window() {
        for width in [MIN_WIDTH, 800.0, WINDOW_WIDTH, 2560.0] {
            let columns = list_columns(width);
            let table = Table::with_gap(&columns, 0.0, PADDING);
            let right = table.right(KIND_COL);
            if width > MIN_WIDTH + 400.0 {
                assert!(
                    (right - (width - PADDING)).abs() < 0.5,
                    "at {width}px the table ends at {right}"
                );
            }
            assert!(columns[NAME_COL].width >= MIN_NAME_WIDTH);
        }
    }

    #[test]
    fn a_resize_moves_the_treemap_tiles_with_the_window() {
        // The tiles are absolute pixel coordinates and the click handler
        // hit-tests against the stored list, so a list computed at one size and
        // clicked at another is a set of controls that are not where they look.
        let mut ui = loaded();
        let before = ui.treemap_rects.first().map(|r| r.width).unwrap_or(0.0);
        ui.on_event(&Event::Resize {
            width: 1600,
            height: 900,
        });
        let after = ui.treemap_rects.first().map(|r| r.width).unwrap_or(0.0);
        assert!(after > before, "{before} -> {after}");
    }

    #[test]
    fn drawing_at_a_new_size_relays_out_before_anything_hit_tests() {
        // The first frame goes out before any `Event::Resize` arrives, so the
        // recorded size cannot be the authority on how big the window is.
        let mut ui = loaded();
        let _ = App::render(&mut ui, 1600.0, 900.0);
        let rect = probe::rect_of_sized(&ui, Target::Scan, (1600.0, 900.0)).expect("scan button");
        assert!(rect.right() < 1600.0);
    }

    // -- The scan, as the window sees it --------------------------------------

    #[test]
    fn the_window_asks_to_be_ticked_or_no_scan_would_ever_finish() {
        // Without a tick interval the app receives no `Event::Tick`, the
        // finished walk is never collected, and the window sits on "Scanning…"
        // over a scan that ended minutes ago.
        let ui = DiskAnalyzerUI::new();
        assert!(App::tick_interval(&ui).is_some());
    }

    #[test]
    fn a_real_scan_runs_to_completion_and_lands_in_the_window() {
        let dir = scratchdir::ScratchDir::new("diskanalyzer-ui");
        std::fs::write(dir.dir().join("a.bin"), vec![0u8; 3000]).unwrap();
        std::fs::create_dir(dir.dir().join("sub")).unwrap();
        std::fs::write(dir.dir().join("sub").join("b.bin"), vec![0u8; 1000]).unwrap();

        let mut ui = DiskAnalyzerUI::new();
        ui.start_scan(dir.dir().to_path_buf());
        assert!(ui.scanning());
        assert_eq!(ui.status_text().split(':').next(), Some("Scanning"));

        // Poll the way the event loop does, with a bound so a wedged scan fails
        // the test instead of hanging it.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while ui.scanning() {
            assert!(std::time::Instant::now() < deadline, "scan never finished");
            ui.poll_scan(SIZE);
            std::thread::sleep(Duration::from_millis(5));
        }

        let tree = ui.dir_tree.as_ref().expect("a finished scan has a tree");
        assert_eq!(tree.total_size, 4000);
        assert!(ui.complete);
        assert!(ui.status_text().contains("Total:"));
    }

    #[test]
    fn scanning_a_path_that_is_not_there_leaves_the_window_saying_why() {
        let mut ui = DiskAnalyzerUI::new();
        ui.start_scan(PathBuf::from("/definitely/not/a/real/path/12345"));
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while ui.scanning() {
            assert!(std::time::Instant::now() < deadline, "scan never finished");
            ui.poll_scan(SIZE);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(ui.scan_error.is_some());
        assert!(ui.status_text().starts_with("Scan failed"));
    }

    #[test]
    fn the_scan_button_reads_cancel_while_a_scan_is_running() {
        let dir = scratchdir::ScratchDir::new("diskanalyzer-cancel");
        let mut ui = DiskAnalyzerUI::new();
        ui.start_scan(dir.dir().to_path_buf());
        let labels: Vec<String> = ui
            .frame(SIZE.0, SIZE.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(labels.iter().any(|t| t == "Cancel"), "got {labels:?}");
        assert!(!labels.iter().any(|t| t == "Scan"));
    }

    #[test]
    fn escape_during_a_scan_stops_the_scan_rather_than_closing_the_window() {
        let dir = scratchdir::ScratchDir::new("diskanalyzer-esc");
        let mut ui = DiskAnalyzerUI::new();
        ui.start_scan(dir.dir().to_path_buf());
        assert_eq!(probe::key(&mut ui, &press(Key::Escape)), Action::Redraw);
        assert!(!ui.scanning());
        // And a second Escape, with nothing running, closes it.
        assert_eq!(probe::key(&mut ui, &press(Key::Escape)), Action::Quit);
    }

    #[test]
    fn clicking_scan_while_a_scan_runs_cancels_it() {
        let dir = scratchdir::ScratchDir::new("diskanalyzer-btn");
        let mut ui = DiskAnalyzerUI::new();
        ui.start_scan(dir.dir().to_path_buf());
        assert_eq!(probe::click(&mut ui, Target::Scan), Action::Redraw);
        assert!(!ui.scanning());
    }

    #[test]
    fn clicking_scan_starts_a_scan_of_whatever_the_path_field_says() {
        let dir = scratchdir::ScratchDir::new("diskanalyzer-start");
        let mut ui = DiskAnalyzerUI::new();
        ui.path_input = dir.dir().display().to_string();
        assert_eq!(probe::click(&mut ui, Target::Scan), Action::Redraw);
        assert!(ui.scanning());
        assert_eq!(ui.config.scan_path, dir.dir());
        ui.cancel_scan();
    }

    #[test]
    fn a_tick_with_no_scan_running_asks_for_nothing() {
        // Ten ticks a second that each request a repaint is a window that never
        // stops drawing while it sits idle.
        let mut ui = loaded();
        let tick = Event::Tick { elapsed_ms: 100 };
        assert_eq!(ui.handle_event(&tick, SIZE), Action::None);
    }

    #[test]
    fn closing_the_window_is_obeyed() {
        let mut ui = loaded();
        assert_eq!(ui.handle_event(&Event::CloseRequested, SIZE), Action::Quit);
    }

    #[test]
    fn the_app_identifies_itself() {
        let ui = DiskAnalyzerUI::new();
        assert!(!App::title(&ui).is_empty());
        assert_eq!(App::app_id(&ui), "diskanalyzer");
        let (w, h) = App::initial_size(&ui);
        assert!(f64::from(w) >= f64::from(MIN_WIDTH));
        assert!(f64::from(h) >= f64::from(MIN_HEIGHT));
    }

    #[test]
    fn a_modifier_the_program_does_not_use_is_not_silently_ignored_as_plain() {
        // Alt-Tab belongs to the window manager. Treating it as a bare Tab
        // would switch views behind the user's back as they switched windows.
        let mut ui = loaded();
        let view = ui.view_mode;
        let alt_tab = press_with(
            Key::Tab,
            guitk::event::Modifiers {
                alt: true,
                ..guitk::event::Modifiers::default()
            },
        );
        probe::key(&mut ui, &alt_tab);
        assert_eq!(ui.view_mode, view);
    }
}

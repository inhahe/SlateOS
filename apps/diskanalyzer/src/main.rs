//! diskanalyzer -- Slate OS Disk Usage Analyzer
//!
//! A visual disk space analyzer similar to WinDirStat / Baobab / SpaceSniffer.
//! Scans a directory tree, presents a squarified treemap, a sortable file list,
//! and an extension-breakdown bar chart.
//!
//! # Architecture
//!
//! ```text
//! scan_directory()   -- recursively builds FileNode tree
//!       |
//!       v
//! DirTree            -- root node + aggregate stats
//!       |
//!       v
//! compute_treemap()  -- squarified treemap layout
//!       |
//!       v
//! DiskAnalyzerUI     -- three views (treemap / list / extension)
//! ```

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::BTreeMap;

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
const CORNER_RADIUS: f32 = 6.0;
const INPUT_WIDTH: f32 = 400.0;
const INPUT_HEIGHT: f32 = 30.0;
const BAR_CHART_ROW_HEIGHT: f32 = 24.0;
const TREEMAP_MIN_RECT: f32 = 4.0;
const TABLE_HEADER_HEIGHT: f32 = 30.0;

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
#[derive(Clone, Debug)]
pub struct FileNode {
    /// Display name (file or directory name, not the full path).
    pub name: String,
    /// Full path from the scan root.
    pub path: String,
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
    pub fn new_file(name: &str, path: &str, size_bytes: u64) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            size_bytes,
            kind: FileKind::RegularFile,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Create a new directory node.
    pub fn new_dir(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            size_bytes: 0,
            kind: FileKind::Directory,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Create a symlink node.
    pub fn new_symlink(name: &str, path: &str, size_bytes: u64) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            size_bytes,
            kind: FileKind::Symlink,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Create an "other" kind node.
    pub fn new_other(name: &str, path: &str, size_bytes: u64) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            size_bytes,
            kind: FileKind::Other,
            children: Vec::new(),
            depth: 0,
        }
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
// ScanProgress
// ============================================================================

/// Phase of the scanning process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanPhase {
    Scanning,
    Calculating,
    Done,
}

/// Progress information during a directory scan.
#[derive(Clone, Debug)]
pub struct ScanProgress {
    pub dirs_scanned: u64,
    pub files_found: u64,
    pub bytes_found: u64,
    pub current_path: String,
    pub phase: ScanPhase,
}

impl Default for ScanProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanProgress {
    pub fn new() -> Self {
        Self {
            dirs_scanned: 0,
            files_found: 0,
            bytes_found: 0,
            current_path: String::new(),
            phase: ScanPhase::Scanning,
        }
    }
}

// ============================================================================
// Tree scanning
// ============================================================================

/// Recursively scan a directory tree building a `FileNode` hierarchy.
///
/// In a real deployment this calls into the VFS. The current implementation
/// provides the algorithm with stub filesystem calls that accept pre-built
/// `FileNode` trees for testing.
pub fn scan_directory(root: &mut FileNode) -> DirTree {
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

/// Return the top N largest files or directories from the tree.
pub fn find_largest(node: &FileNode, n: usize) -> Vec<(String, u64)> {
    let mut results: Vec<(String, u64)> = Vec::new();
    collect_all_entries(node, &mut results);
    results.sort_by_key(|&(_, size)| std::cmp::Reverse(size));
    results.truncate(n);
    results
}

/// Helper: collect (path, size) for every node in the tree.
fn collect_all_entries(node: &FileNode, out: &mut Vec<(String, u64)>) {
    out.push((node.path.clone(), node.size_bytes));
    for child in &node.children {
        collect_all_entries(child, out);
    }
}

/// Group files by extension and return total size per extension.
pub fn find_by_extension(node: &FileNode) -> BTreeMap<String, u64> {
    let mut map: BTreeMap<String, u64> = BTreeMap::new();
    collect_extensions(node, &mut map);
    map
}

fn collect_extensions(node: &FileNode, map: &mut BTreeMap<String, u64>) {
    if node.kind == FileKind::RegularFile {
        let ext = node.extension();
        if !ext.is_empty() {
            let entry = map.entry(ext).or_insert(0);
            *entry = entry.saturating_add(node.size_bytes);
        }
    }
    for child in &node.children {
        collect_extensions(child, map);
    }
}

/// Find all files whose size exceeds the given threshold.
pub fn filter_by_size(node: &FileNode, min_bytes: u64) -> Vec<(String, u64)> {
    let mut results: Vec<(String, u64)> = Vec::new();
    collect_by_size(node, min_bytes, &mut results);
    results
}

fn collect_by_size(node: &FileNode, min_bytes: u64, out: &mut Vec<(String, u64)>) {
    if node.kind == FileKind::RegularFile && node.size_bytes >= min_bytes {
        out.push((node.path.clone(), node.size_bytes));
    }
    for child in &node.children {
        collect_by_size(child, min_bytes, out);
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
    pub path: String,
    /// Display name.
    pub name: String,
    /// Size in bytes.
    pub size_bytes: u64,
}

/// Flat representation of a node for treemap layout.
struct FlatNode {
    name: String,
    path: String,
    size_bytes: u64,
    depth: u32,
    kind: FileKind,
    extension: String,
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
    if children.len() == 1 {
        let child = children[0];
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
    for i in 0..children.len() {
        row_sum += sizes[i];
        let row_fraction = row_sum / total_size;
        let row_cross = (cross as f64) * row_fraction;

        if row_cross < 0.5 {
            continue;
        }

        let worst = worst_aspect_in_row(&sizes[..=i], row_sum, full_length as f64, row_cross);
        if worst < best_worst_aspect {
            best_worst_aspect = worst;
            best_split = i + 1;
        } else if i > 0 {
            // Aspect getting worse — stop searching.
            break;
        }
    }

    // Lay out the first `best_split` items in a row.
    let row_sizes = &sizes[..best_split];
    let row_sum: f64 = row_sizes.iter().sum();
    let row_fraction = if total_size > 0.0 {
        row_sum / total_size
    } else {
        0.0
    };

    let row_cross_pixels = (cross as f64 * row_fraction) as f32;

    let mut offset = 0.0f32;
    for (i, &sz) in row_sizes.iter().enumerate() {
        let item_fraction = if row_sum > 0.0 { sz / row_sum } else { 0.0 };
        let item_length = (full_length as f64 * item_fraction) as f32;

        let (rx, ry, rw, rh) = if lay_horizontal {
            (x + offset, y, item_length, row_cross_pixels)
        } else {
            (x, y + offset, row_cross_pixels, item_length)
        };

        let child = children[i];
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
        });

        offset += item_length;
    }

    // Recurse into remaining items.
    if best_split < children.len() {
        let remaining_children = &children[best_split..];
        let remaining_sizes = &sizes[best_split..];
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
            let percentage = if grand_total > 0 {
                (total_size as f64 / grand_total as f64 * 100.0) as f32
            } else {
                0.0
            };
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
    pub path: String,
    pub size_bytes: u64,
    pub percentage: f32,
    pub kind: FileKind,
    pub is_expanded: bool,
    pub depth: u32,
    pub has_children: bool,
}

/// Flatten a file tree into list rows for display.
pub fn flatten_tree(node: &FileNode, parent_size: u64, expanded_paths: &[String]) -> Vec<ListRow> {
    let mut rows = Vec::new();
    flatten_node(node, parent_size, expanded_paths, &mut rows);
    rows
}

fn flatten_node(
    node: &FileNode,
    parent_size: u64,
    expanded_paths: &[String],
    rows: &mut Vec<ListRow>,
) {
    let percentage = if parent_size > 0 {
        (node.size_bytes as f64 / parent_size as f64 * 100.0) as f32
    } else {
        0.0
    };
    // The root (depth 0) is always expanded: after a scan `expanded_paths` is
    // empty, and the user expects to see the top-level contents immediately
    // rather than a single collapsed root row.
    let is_expanded = node.depth == 0 || expanded_paths.contains(&node.path);
    rows.push(ListRow {
        name: node.name.clone(),
        path: node.path.clone(),
        size_bytes: node.size_bytes,
        percentage,
        kind: node.kind,
        is_expanded,
        depth: node.depth,
        has_children: !node.children.is_empty(),
    });

    if is_expanded {
        let mut children: Vec<&FileNode> = node.children.iter().collect();
        children.sort_by_key(|c| std::cmp::Reverse(c.size_bytes));
        for child in children {
            flatten_node(child, node.size_bytes, expanded_paths, rows);
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
}

// ============================================================================
// Analyzer configuration
// ============================================================================

/// Configuration options for the disk analyzer.
#[derive(Clone, Debug)]
pub struct AnalyzerConfig {
    /// Root path to scan.
    pub scan_path: String,
    /// Minimum file size to include in treemap (bytes).
    pub min_display_size: u64,
    /// Maximum depth of tree to scan (0 = unlimited).
    pub max_scan_depth: u32,
    /// Number of top items to show in "largest files" list.
    pub top_n: usize,
    /// Whether to follow symlinks during scan.
    pub follow_symlinks: bool,
    /// Whether to cross filesystem boundaries.
    pub cross_filesystems: bool,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            scan_path: "/".to_string(),
            min_display_size: 0,
            max_scan_depth: 0,
            top_n: 50,
            follow_symlinks: false,
            cross_filesystems: false,
        }
    }
}

// ============================================================================
// UI state
// ============================================================================

/// Complete UI state for the disk analyzer.
pub struct DiskAnalyzerUI {
    /// Active view mode.
    pub view_mode: ViewMode,
    /// Current configuration.
    pub config: AnalyzerConfig,
    /// Scanned directory tree (populated after scan).
    pub dir_tree: Option<DirTree>,
    /// Computed treemap rectangles.
    pub treemap_rects: Vec<TreemapRect>,
    /// Extension statistics.
    pub extension_stats: Vec<ExtensionStat>,
    /// Current breadcrumb path segments for drill-down.
    pub breadcrumbs: Vec<String>,
    /// Paths of expanded directories in the list view.
    pub expanded_paths: Vec<String>,
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
    /// Scan progress.
    pub progress: ScanProgress,
    /// Whether a scan is in progress.
    pub scanning: bool,
    /// Text in the path input field.
    pub path_input: String,
    /// Scroll offset for list view.
    pub scroll_offset: f32,
    /// List rows (cached after sort/flatten).
    pub list_rows: Vec<ListRow>,
}

impl Default for DiskAnalyzerUI {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskAnalyzerUI {
    /// Create a new UI with default state.
    pub fn new() -> Self {
        Self {
            view_mode: ViewMode::Treemap,
            config: AnalyzerConfig::default(),
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
            progress: ScanProgress::new(),
            scanning: false,
            path_input: "/".to_string(),
            scroll_offset: 0.0,
            list_rows: Vec::new(),
        }
    }

    /// Set the view mode.
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
    }

    /// Start a scan with the given pre-built tree (for testing / offline use).
    pub fn load_tree(&mut self, mut root: FileNode) {
        let tree = scan_directory(&mut root);
        self.treemap_rects = compute_treemap(
            &tree.root,
            PADDING,
            TOOLBAR_HEIGHT + BREADCRUMB_HEIGHT + PADDING,
            WINDOW_WIDTH - 2.0 * PADDING,
            WINDOW_HEIGHT - TOOLBAR_HEIGHT - BREADCRUMB_HEIGHT - STATUS_BAR_HEIGHT - 2.0 * PADDING,
        );
        self.extension_stats = compute_extension_stats(&tree.root);
        self.breadcrumbs = vec![tree.root.name.clone()];
        self.refresh_list(&tree);
        self.dir_tree = Some(tree);
        self.scanning = false;
        self.progress.phase = ScanPhase::Done;
    }

    /// Refresh the list rows from the current tree.
    fn refresh_list(&mut self, tree: &DirTree) {
        self.list_rows = flatten_tree(&tree.root, tree.total_size, &self.expanded_paths);
        sort_rows(&mut self.list_rows, self.sort_column, self.sort_direction);
    }

    /// Toggle expansion of a path in the list view.
    pub fn toggle_expand(&mut self, path: &str) {
        if let Some(pos) = self.expanded_paths.iter().position(|p| p == path) {
            self.expanded_paths.remove(pos);
        } else {
            self.expanded_paths.push(path.to_string());
        }
        if let Some(tree) = &self.dir_tree {
            let tree_clone = tree.clone();
            self.refresh_list(&tree_clone);
        }
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
    }

    /// Handle mouse hover over the treemap at (mx, my).
    pub fn hover_treemap(&mut self, mx: f32, my: f32) {
        self.hovered_rect = treemap_hit_test(&self.treemap_rects, mx, my);
        if let Some(idx) = self.hovered_rect {
            if let Some(rect) = self.treemap_rects.get(idx) {
                self.tooltip_text = format!("{}\n{}", rect.path, format_size(rect.size_bytes),);
                self.tooltip_x = mx;
                self.tooltip_y = my;
            }
        } else {
            self.tooltip_text.clear();
        }
    }

    /// Navigate the breadcrumb trail to a specific depth.
    pub fn navigate_breadcrumb(&mut self, depth: usize) {
        if depth < self.breadcrumbs.len() {
            self.breadcrumbs.truncate(depth + 1);
        }
    }

    /// Drill down into a directory in the treemap.
    pub fn drill_down(&mut self, name: &str) {
        self.breadcrumbs.push(name.to_string());
    }

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
        self.render_breadcrumbs(&mut tree);

        match self.view_mode {
            ViewMode::Treemap => self.render_treemap_view(&mut tree),
            ViewMode::List => self.render_list_view(&mut tree),
            ViewMode::Extensions => self.render_extension_view(&mut tree),
        }

        self.render_status_bar(&mut tree);
        self.render_tooltip(&mut tree);

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

        // Path input field
        tree.push(RenderCommand::FillRect {
            x: PADDING,
            y: 7.0,
            width: INPUT_WIDTH,
            height: INPUT_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: PADDING + 8.0,
            y: 14.0,
            text: self.path_input.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(INPUT_WIDTH - 16.0),
        });

        // Scan button
        let scan_x = PADDING + INPUT_WIDTH + PADDING;
        let scan_color = if self.scanning {
            COLOR_OVERLAY0
        } else {
            COLOR_BLUE
        };
        tree.push(RenderCommand::FillRect {
            x: scan_x,
            y: 7.0,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: scan_color,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: scan_x + 20.0,
            y: 14.0,
            text: if self.scanning {
                "Scanning...".to_string()
            } else {
                "Scan".to_string()
            },
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(BUTTON_WIDTH - 10.0),
        });

        // View mode toggle buttons
        let toggle_x = scan_x + BUTTON_WIDTH + PADDING * 2.0;
        let modes = [
            (ViewMode::Treemap, "Treemap"),
            (ViewMode::List, "List"),
            (ViewMode::Extensions, "Extensions"),
        ];
        let mut btn_x = toggle_x;
        for (mode, label) in &modes {
            let btn_color = if self.view_mode == *mode {
                COLOR_SURFACE1
            } else {
                COLOR_SURFACE0
            };
            let text_color = if self.view_mode == *mode {
                COLOR_TEXT
            } else {
                COLOR_SUBTEXT0
            };
            let btn_w = (label.len() as f32) * 8.0 + 16.0;
            tree.push(RenderCommand::FillRect {
                x: btn_x,
                y: 7.0,
                width: btn_w,
                height: BUTTON_HEIGHT,
                color: btn_color,
                corner_radii: CornerRadii::all(4.0),
            });
            tree.push(RenderCommand::Text {
                x: btn_x + 8.0,
                y: 14.0,
                text: label.to_string(),
                color: text_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(btn_w - 16.0),
            });
            btn_x += btn_w + 4.0;
        }
    }

    // -- breadcrumbs -----------------------------------------------------------

    fn render_breadcrumbs(&self, tree: &mut RenderTree) {
        let y = TOOLBAR_HEIGHT;
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: WINDOW_WIDTH,
            height: BREADCRUMB_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let mut bx = PADDING;
        for (i, segment) in self.breadcrumbs.iter().enumerate() {
            if i > 0 {
                tree.push(RenderCommand::Text {
                    x: bx,
                    y: y + 8.0,
                    text: " / ".to_string(),
                    color: COLOR_OVERLAY0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                bx += 20.0;
            }
            let color = if i == self.breadcrumbs.len().saturating_sub(1) {
                COLOR_TEXT
            } else {
                COLOR_BLUE
            };
            tree.push(RenderCommand::Text {
                x: bx,
                y: y + 8.0,
                text: segment.clone(),
                color,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
            });
            bx += (segment.len() as f32) * 7.0 + 4.0;
        }
    }

    // -- treemap view ----------------------------------------------------------

    fn render_treemap_view(&self, tree: &mut RenderTree) {
        for (i, rect) in self.treemap_rects.iter().enumerate() {
            let is_hovered = self.hovered_rect == Some(i);
            let color = if is_hovered {
                lighten_color(rect.color, 30)
            } else {
                rect.color
            };

            tree.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                color,
                corner_radii: CornerRadii::all(2.0),
            });

            // Border
            tree.push(RenderCommand::StrokeRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                color: COLOR_BASE,
                line_width: 1.0,
                corner_radii: CornerRadii::all(2.0),
            });

            // Label (only if rect is large enough)
            if rect.width > 60.0 && rect.height > 20.0 {
                tree.push(RenderCommand::Text {
                    x: rect.x + 4.0,
                    y: rect.y + 4.0,
                    text: rect.name.clone(),
                    color: COLOR_TEXT,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(rect.width - 8.0),
                });
            }
            // Size label if rect is tall enough
            if rect.width > 60.0 && rect.height > 36.0 {
                tree.push(RenderCommand::Text {
                    x: rect.x + 4.0,
                    y: rect.y + 18.0,
                    text: format_size(rect.size_bytes),
                    color: COLOR_SUBTEXT0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(rect.width - 8.0),
                });
            }
        }

        if self.treemap_rects.is_empty() && !self.scanning {
            tree.push(RenderCommand::Text {
                x: WINDOW_WIDTH / 2.0 - 100.0,
                y: WINDOW_HEIGHT / 2.0,
                text: "No data. Click Scan to begin.".to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_HEADING,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    // -- list view -------------------------------------------------------------

    fn render_list_view(&self, tree: &mut RenderTree) {
        let content_y = TOOLBAR_HEIGHT + BREADCRUMB_HEIGHT;
        let content_h = WINDOW_HEIGHT - TOOLBAR_HEIGHT - BREADCRUMB_HEIGHT - STATUS_BAR_HEIGHT;

        // Table header
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: content_y,
            width: WINDOW_WIDTH,
            height: TABLE_HEADER_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let columns = [
            ("Name", 0.0, 360.0),
            ("Size", 360.0, 120.0),
            ("%", 480.0, 80.0),
            ("Type", 560.0, 100.0),
        ];
        for (label, cx, _cw) in &columns {
            tree.push(RenderCommand::Text {
                x: *cx + PADDING,
                y: content_y + 8.0,
                text: label.to_string(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Rows
        let row_area_y = content_y + TABLE_HEADER_HEIGHT;
        let max_visible = ((content_h - TABLE_HEADER_HEIGHT) / ROW_HEIGHT) as usize;

        for (i, row) in self.list_rows.iter().enumerate() {
            if i >= max_visible {
                break;
            }
            let ry = row_area_y + i as f32 * ROW_HEIGHT;
            // Alternating row background
            if i % 2 == 0 {
                tree.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: ry,
                    width: WINDOW_WIDTH,
                    height: ROW_HEIGHT,
                    color: COLOR_SURFACE0,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Indentation for depth
            let indent = row.depth as f32 * 20.0;

            // Expand/collapse indicator for directories
            let prefix = if row.has_children {
                if row.is_expanded { "v " } else { "> " }
            } else {
                "  "
            };

            // Name
            tree.push(RenderCommand::Text {
                x: PADDING + indent,
                y: ry + 6.0,
                text: format!("{prefix}{}", row.name),
                color: if row.kind == FileKind::Directory {
                    COLOR_BLUE
                } else {
                    COLOR_TEXT
                },
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(360.0 - indent - PADDING),
            });

            // Size
            tree.push(RenderCommand::Text {
                x: 360.0 + PADDING,
                y: ry + 6.0,
                text: format_size(row.size_bytes),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Percentage
            tree.push(RenderCommand::Text {
                x: 480.0 + PADDING,
                y: ry + 6.0,
                text: format_percent(row.percentage),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Type
            tree.push(RenderCommand::Text {
                x: 560.0 + PADDING,
                y: ry + 6.0,
                text: file_kind_label(row.kind).to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    // -- extension view --------------------------------------------------------

    fn render_extension_view(&self, tree: &mut RenderTree) {
        let content_y = TOOLBAR_HEIGHT + BREADCRUMB_HEIGHT + PADDING;
        let content_w = WINDOW_WIDTH - 2.0 * PADDING;

        tree.push(RenderCommand::Text {
            x: PADDING,
            y: content_y,
            text: "File Types by Size".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let chart_y = content_y + 28.0;
        let max_bars = 20usize;
        let max_bar_width = content_w - 200.0;

        // Find max size for scaling.
        let max_size = self
            .extension_stats
            .first()
            .map(|s| s.total_size)
            .unwrap_or(1);

        for (i, stat) in self.extension_stats.iter().enumerate() {
            if i >= max_bars {
                break;
            }
            let by = chart_y + i as f32 * (BAR_CHART_ROW_HEIGHT + 4.0);
            let bar_fraction = if max_size > 0 {
                stat.total_size as f32 / max_size as f32
            } else {
                0.0
            };
            let bar_w = max_bar_width * bar_fraction;

            // Extension label
            let label = if stat.extension.is_empty() {
                "(no ext)".to_string()
            } else {
                format!(".{}", stat.extension)
            };
            tree.push(RenderCommand::Text {
                x: PADDING,
                y: by + 4.0,
                text: label,
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
            });

            // Bar
            let bar_color = color_for_extension(&stat.extension);
            tree.push(RenderCommand::FillRect {
                x: 90.0,
                y: by,
                width: bar_w.max(2.0),
                height: BAR_CHART_ROW_HEIGHT,
                color: bar_color,
                corner_radii: CornerRadii::all(3.0),
            });

            // Size + count label
            tree.push(RenderCommand::Text {
                x: 90.0 + bar_w + 8.0,
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
                max_width: Some(300.0),
            });
        }

        if self.extension_stats.is_empty() && !self.scanning {
            tree.push(RenderCommand::Text {
                x: WINDOW_WIDTH / 2.0 - 100.0,
                y: WINDOW_HEIGHT / 2.0,
                text: "No data. Click Scan to begin.".to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_HEADING,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
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

        let status_text = if let Some(ref dt) = self.dir_tree {
            format!(
                "Total: {} | Files: {} | Dirs: {} | Scan: {}ms",
                format_size(dt.total_size),
                dt.file_count,
                dt.dir_count,
                dt.scan_duration_ms,
            )
        } else if self.scanning {
            format!(
                "Scanning: {} dirs, {} files, {} found | {}",
                self.progress.dirs_scanned,
                self.progress.files_found,
                format_size(self.progress.bytes_found),
                self.progress.current_path,
            )
        } else {
            "Ready".to_string()
        };

        tree.push(RenderCommand::Text {
            x: PADDING,
            y: y + 6.0,
            text: status_text,
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(WINDOW_WIDTH - 2.0 * PADDING),
        });
    }

    // -- tooltip ---------------------------------------------------------------

    fn render_tooltip(&self, tree: &mut RenderTree) {
        if self.tooltip_text.is_empty() {
            return;
        }
        let tw = 250.0f32;
        let th = 44.0f32;
        let tx = (self.tooltip_x + 12.0).min(WINDOW_WIDTH - tw - 4.0);
        let ty = (self.tooltip_y + 12.0).min(WINDOW_HEIGHT - th - 4.0);

        // Shadow / background
        tree.push(RenderCommand::FillRect {
            x: tx,
            y: ty,
            width: tw,
            height: th,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x: tx,
            y: ty,
            width: tw,
            height: th,
            color: COLOR_OVERLAY0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Render each line of the tooltip.
        let mut line_y = ty + 6.0;
        for line in self.tooltip_text.split('\n') {
            tree.push(RenderCommand::Text {
                x: tx + 8.0,
                y: line_y,
                text: line.to_string(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(tw - 16.0),
            });
            line_y += 16.0;
        }
    }
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
// Path helpers
// ============================================================================

/// Join two path segments with `/`.
fn join_path(base: &str, segment: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{segment}")
    } else {
        format!("{base}/{segment}")
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
        assert_eq!(f.path, "/tmp/test.txt");
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
    fn test_scan_directory_total_size() {
        let mut root = sample_tree();
        let tree = scan_directory(&mut root);
        assert_eq!(tree.total_size, 21504);
    }

    #[test]
    fn test_scan_directory_counts() {
        let mut root = sample_tree();
        let tree = scan_directory(&mut root);
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
        let tree = scan_directory(&mut root);
        assert_eq!(tree.total_size, 0);
        assert_eq!(tree.file_count, 0);
        assert_eq!(tree.dir_count, 1);
    }

    // -- find_largest tests ----------------------------------------------------

    #[test]
    fn test_find_largest_top_3() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        assign_depths(&mut root, 0);
        let top = find_largest(&root, 3);
        assert_eq!(top.len(), 3);
        // Largest should be root (21504), then logo.png (8192), then src dir (7168)
        assert_eq!(top[0].1, 21504);
        assert_eq!(top[1].1, 8192);
        assert_eq!(top[2].1, 7168);
    }

    #[test]
    fn test_find_largest_more_than_available() {
        let f = FileNode::new_file("solo.txt", "/solo.txt", 42);
        let top = find_largest(&f, 100);
        assert_eq!(top.len(), 1);
    }

    // -- find_by_extension tests -----------------------------------------------

    #[test]
    fn test_find_by_extension() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        let by_ext = find_by_extension(&root);
        // rs: main.rs(2048) + lib.rs(4096) + util.rs(1024) = 7168
        assert_eq!(by_ext.get("rs").copied(), Some(7168));
        // txt: readme.txt(1024)
        assert_eq!(by_ext.get("txt").copied(), Some(1024));
        // pdf: spec.pdf(5120)
        assert_eq!(by_ext.get("pdf").copied(), Some(5120));
        // png: logo.png(8192)
        assert_eq!(by_ext.get("png").copied(), Some(8192));
    }

    #[test]
    fn test_find_by_extension_empty() {
        let root = FileNode::new_dir("empty", "/empty");
        let by_ext = find_by_extension(&root);
        assert!(by_ext.is_empty());
    }

    // -- filter_by_size tests --------------------------------------------------

    #[test]
    fn test_filter_by_size() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        let big = filter_by_size(&root, 4096);
        // spec.pdf(5120), lib.rs(4096), logo.png(8192)
        assert_eq!(big.len(), 3);
    }

    #[test]
    fn test_filter_by_size_none_match() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        let big = filter_by_size(&root, 1_000_000);
        assert!(big.is_empty());
    }

    #[test]
    fn test_filter_by_size_all_match() {
        let mut root = sample_tree();
        calculate_sizes(&mut root);
        let all = filter_by_size(&root, 0);
        assert_eq!(all.len(), 6); // all 6 regular files
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
                path: "/a".to_string(),
                name: "a".to_string(),
                size_bytes: 100,
            },
            TreemapRect {
                x: 100.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                node_index: 1,
                depth: 1,
                color: COLOR_RED,
                path: "/b".to_string(),
                name: "b".to_string(),
                size_bytes: 200,
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
            path: "/x".to_string(),
            name: "x".to_string(),
            size_bytes: 50,
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
        assert_eq!(format_size(1024), "1.00 KiB");
        assert_eq!(format_size(2048), "2.00 KiB");
    }

    #[test]
    fn test_format_size_mib() {
        assert_eq!(format_size(1024 * 1024), "1.00 MiB");
        assert_eq!(format_size(1_500_000), "1.43 MiB");
    }

    #[test]
    fn test_format_size_gib() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GiB");
    }

    #[test]
    fn test_format_size_tib() {
        assert_eq!(format_size(1024u64 * 1024 * 1024 * 1024), "1.00 TiB");
    }

    // -- Sorting ---------------------------------------------------------------

    #[test]
    fn test_sort_rows_by_size_desc() {
        let mut rows = vec![
            ListRow {
                name: "small".to_string(),
                path: "/small".to_string(),
                size_bytes: 100,
                percentage: 10.0,
                kind: FileKind::RegularFile,
                is_expanded: false,
                depth: 0,
                has_children: false,
            },
            ListRow {
                name: "big".to_string(),
                path: "/big".to_string(),
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
                path: "/z".to_string(),
                size_bytes: 1,
                percentage: 50.0,
                kind: FileKind::RegularFile,
                is_expanded: false,
                depth: 0,
                has_children: false,
            },
            ListRow {
                name: "Apple".to_string(),
                path: "/a".to_string(),
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
                path: "/a".to_string(),
                size_bytes: 10,
                percentage: 80.0,
                kind: FileKind::RegularFile,
                is_expanded: false,
                depth: 0,
                has_children: false,
            },
            ListRow {
                name: "b".to_string(),
                path: "/b".to_string(),
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
                path: "/dir".to_string(),
                size_bytes: 100,
                percentage: 50.0,
                kind: FileKind::Directory,
                is_expanded: false,
                depth: 0,
                has_children: true,
            },
            ListRow {
                name: "a.txt".to_string(),
                path: "/a.txt".to_string(),
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
        let expanded = vec!["/root".to_string(), "/root/docs".to_string()];
        let rows = flatten_tree(&root, root.size_bytes, &expanded);
        // root + docs + readme.txt + spec.pdf + src + logo.png = 6
        // (src children not expanded)
        assert_eq!(rows.len(), 6);
    }

    // -- Config tests ----------------------------------------------------------

    #[test]
    fn test_default_config() {
        let cfg = AnalyzerConfig::default();
        assert_eq!(cfg.scan_path, "/");
        assert_eq!(cfg.min_display_size, 0);
        assert_eq!(cfg.max_scan_depth, 0);
        assert_eq!(cfg.top_n, 50);
        assert!(!cfg.follow_symlinks);
        assert!(!cfg.cross_filesystems);
    }

    #[test]
    fn test_config_custom() {
        let cfg = AnalyzerConfig {
            scan_path: "/home".to_string(),
            min_display_size: 4096,
            max_scan_depth: 5,
            top_n: 20,
            follow_symlinks: true,
            cross_filesystems: true,
        };
        assert_eq!(cfg.scan_path, "/home");
        assert_eq!(cfg.min_display_size, 4096);
        assert_eq!(cfg.max_scan_depth, 5);
        assert!(cfg.follow_symlinks);
    }

    // -- UI state management tests ---------------------------------------------

    #[test]
    fn test_ui_initial_state() {
        let ui = DiskAnalyzerUI::new();
        assert_eq!(ui.view_mode, ViewMode::Treemap);
        assert!(ui.dir_tree.is_none());
        assert!(ui.treemap_rects.is_empty());
        assert!(ui.extension_stats.is_empty());
        assert!(!ui.scanning);
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
        assert!(!ui.scanning);
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
        ui.toggle_expand("/root/src");
        assert!(ui.expanded_paths.contains(&"/root/src".to_string()));
        let expanded_rows = ui.list_rows.len();
        assert!(expanded_rows > initial_rows);
        // Collapse.
        ui.toggle_expand("/root/src");
        assert!(!ui.expanded_paths.contains(&"/root/src".to_string()));
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
        ui.drill_down("docs");
        ui.drill_down("specs");
        assert_eq!(ui.breadcrumbs.len(), 3);
        ui.navigate_breadcrumb(1);
        assert_eq!(ui.breadcrumbs.len(), 2);
    }

    #[test]
    fn test_ui_drill_down() {
        let mut ui = DiskAnalyzerUI::new();
        let initial = ui.breadcrumbs.len();
        ui.drill_down("subdir");
        assert_eq!(ui.breadcrumbs.len(), initial + 1);
        assert_eq!(ui.breadcrumbs.last().map(|s| s.as_str()), Some("subdir"));
    }

    // -- Rendering tests -------------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let ui = DiskAnalyzerUI::new();
        let tree = ui.render();
        // Should always have at least the background rect, toolbar, etc.
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_treemap_view() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        ui.set_view_mode(ViewMode::Treemap);
        let tree = ui.render();
        // Should have fill rects for treemap cells + toolbar + background + etc.
        assert!(tree.len() > 10);
    }

    #[test]
    fn test_render_list_view() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        ui.set_view_mode(ViewMode::List);
        let tree = ui.render();
        assert!(tree.len() > 10);
    }

    #[test]
    fn test_render_extension_view() {
        let mut ui = DiskAnalyzerUI::new();
        ui.load_tree(sample_tree());
        ui.set_view_mode(ViewMode::Extensions);
        let tree = ui.render();
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
        let tree = ui.render();
        // Should have tooltip-related fill rects.
        assert!(tree.len() > 15);
    }

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

    // -- ScanProgress ----------------------------------------------------------

    #[test]
    fn test_scan_progress_initial() {
        let p = ScanProgress::new();
        assert_eq!(p.dirs_scanned, 0);
        assert_eq!(p.files_found, 0);
        assert_eq!(p.bytes_found, 0);
        assert_eq!(p.phase, ScanPhase::Scanning);
    }

    // -- SortDirection ---------------------------------------------------------

    #[test]
    fn test_sort_direction_toggle() {
        assert_eq!(SortDirection::Ascending.toggle(), SortDirection::Descending);
        assert_eq!(SortDirection::Descending.toggle(), SortDirection::Ascending);
    }

    // -- Path helpers ----------------------------------------------------------

    #[test]
    fn test_join_path() {
        assert_eq!(join_path("/home", "user"), "/home/user");
        assert_eq!(join_path("/home/", "user"), "/home/user");
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
}

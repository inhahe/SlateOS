//! `SlateOS` File Diff/Compare Tool
//!
//! A desktop application for comparing files and directories with:
//! - Myers diff algorithm for optimal edit scripts
//! - Side-by-side view with synchronized scrolling
//! - Unified diff view with +/- markers
//! - Inline diff view with character-level highlighting
//! - Color coding: green additions, red deletions, yellow modifications
//! - Navigation: jump to next/previous change
//! - File loading for two-file comparison
//! - Statistics: line counts, change counts, similarity percentage
//! - Merge support: accept left/right/both per hunk
//! - Directory comparison mode
//! - Ignore options: whitespace, case, blank lines
//! - Search within diff panels
//!
//! Uses the guitk library for UI rendering with Catppuccin Mocha colors.

#![deny(clippy::all, clippy::pedantic)]
#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::unreadable_literal,
    clippy::module_name_repetitions,
    clippy::struct_excessive_bools
)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::layout::{FlexAlign, FlexDirection, FlexItem, FlexJustify, SizeConstraint};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::{Borders, CornerRadii, Edges, FontWeight, Style, TextAlign};
#[allow(unused_imports)]
use guitk::widget::{Widget, WidgetId, WidgetTree};

use std::collections::VecDeque;
use std::fmt;

// ============================================================================
// Catppuccin Mocha color palette
// ============================================================================

/// Catppuccin Mocha theme colors used throughout the diff tool.
pub mod colors {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);

    // Diff-specific background colors (semi-transparent effect via muted shades)
    pub const ADD_BG: Color = Color::rgba(166, 227, 161, 30);
    pub const DEL_BG: Color = Color::rgba(243, 139, 168, 30);
    pub const ADD_LINE_BG: Color = Color::rgba(166, 227, 161, 50);
    pub const DEL_LINE_BG: Color = Color::rgba(243, 139, 168, 50);
}

// ============================================================================
// Configuration constants
// ============================================================================

/// Font size for diff content display.
const CONTENT_FONT_SIZE: f32 = 13.0;

/// Font size for UI elements (toolbar, status bar).
const UI_FONT_SIZE: f32 = 12.0;

/// Height of each diff line in pixels.
const LINE_HEIGHT: f32 = 20.0;

/// Width of the line number gutter in pixels.
const GUTTER_WIDTH: f32 = 55.0;

/// Height of the toolbar area.
const TOOLBAR_HEIGHT: f32 = 38.0;

/// Height of the status bar.
const STATUS_BAR_HEIGHT: f32 = 26.0;

/// Approximate character width for monospace at content font size.
const CHAR_WIDTH: f32 = 7.8;

/// Maximum number of search results to track.
const MAX_SEARCH_RESULTS: usize = 10_000;

/// Scroll speed multiplier.
const SCROLL_SPEED: f32 = 3.0;

/// Padding inside panels.
const PANEL_PADDING: f32 = 4.0;

/// Separator width between side-by-side panels.
const SEPARATOR_WIDTH: f32 = 2.0;

// ============================================================================
// Myers diff algorithm
// ============================================================================

/// Type of change in a diff edit script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffOp {
    /// Line exists in both files (unchanged).
    Equal,
    /// Line was added (exists only in right/new file).
    Insert,
    /// Line was deleted (exists only in left/old file).
    Delete,
}

impl fmt::Display for DiffOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Equal => write!(f, " "),
            Self::Insert => write!(f, "+"),
            Self::Delete => write!(f, "-"),
        }
    }
}

/// A single edit in the diff result.
#[derive(Clone, Debug)]
pub struct DiffEdit {
    /// Operation type.
    pub op: DiffOp,
    /// Line number in the left/old file (None for inserts).
    pub left_line: Option<usize>,
    /// Line number in the right/new file (None for deletes).
    pub right_line: Option<usize>,
    /// The text content of the line.
    pub text: String,
}

/// A hunk of consecutive changes grouped together.
#[derive(Clone, Debug)]
pub struct DiffHunk {
    /// Starting line in the left file.
    pub left_start: usize,
    /// Number of lines from the left file in this hunk.
    pub left_count: usize,
    /// Starting line in the right file.
    pub right_start: usize,
    /// Number of lines from the right file in this hunk.
    pub right_count: usize,
    /// Edits within this hunk.
    pub edits: Vec<DiffEdit>,
}

/// Result of a diff computation.
#[derive(Clone, Debug)]
pub struct DiffResult {
    /// Full list of edits (equal + insert + delete).
    pub edits: Vec<DiffEdit>,
    /// Grouped hunks of changes (with context).
    pub hunks: Vec<DiffHunk>,
    /// Total lines in the left file.
    pub left_line_count: usize,
    /// Total lines in the right file.
    pub right_line_count: usize,
}

/// Character-level diff for inline highlighting within a line.
#[derive(Clone, Debug)]
pub struct InlineEdit {
    /// Start byte offset in the text.
    pub start: usize,
    /// End byte offset in the text (exclusive).
    pub end: usize,
    /// Whether this span is changed.
    pub changed: bool,
}

/// Compute the Myers diff between two slices of lines.
///
/// Returns the edit script as a list of `DiffEdit` entries.
/// Uses the classic Myers algorithm with linear-space optimization.
// The single-character bindings (n, m, k, x, y, v, d) are the canonical names
// from Myers' paper; renaming them would obscure the algorithm. The usize->isize
// casts are bounded by line counts, which never approach isize::MAX in practice.
#[allow(clippy::many_single_char_names, clippy::cast_possible_wrap)]
#[must_use]
pub fn myers_diff(left: &[&str], right: &[&str]) -> Vec<DiffEdit> {
    let n = left.len();
    let m = right.len();

    if n == 0 && m == 0 {
        return Vec::new();
    }

    if n == 0 {
        return right
            .iter()
            .enumerate()
            .map(|(i, line)| DiffEdit {
                op: DiffOp::Insert,
                left_line: None,
                right_line: Some(i),
                text: (*line).to_string(),
            })
            .collect();
    }

    if m == 0 {
        return left
            .iter()
            .enumerate()
            .map(|(i, line)| DiffEdit {
                op: DiffOp::Delete,
                left_line: Some(i),
                right_line: None,
                text: (*line).to_string(),
            })
            .collect();
    }

    // Myers algorithm: find the shortest edit script
    let max_d = n.saturating_add(m);
    let offset = max_d;
    let v_size = max_d.saturating_mul(2).saturating_add(1);
    let mut v: Vec<isize> = vec![0; v_size];
    let mut trace: Vec<Vec<isize>> = Vec::new();

    'outer: for d in 0..=max_d {
        trace.push(v.clone());
        let d_i = d as isize;

        let mut k = -d_i;
        while k <= d_i {
            let idx = (k as usize).wrapping_add(offset);
            let go_down = if k == -d_i {
                true
            } else if k == d_i {
                false
            } else {
                let left_idx = idx.wrapping_sub(1);
                let right_idx = idx.wrapping_add(1);
                let left_val = v.get(left_idx).copied().unwrap_or(0);
                let right_val = v.get(right_idx).copied().unwrap_or(0);
                left_val < right_val
            };

            let mut x = if go_down {
                let right_idx = idx.wrapping_add(1);
                v.get(right_idx).copied().unwrap_or(0)
            } else {
                let left_idx = idx.wrapping_sub(1);
                v.get(left_idx).copied().unwrap_or(0).saturating_add(1)
            };

            let mut y = x.saturating_sub(k);

            // Follow diagonal (equal lines)
            while (x as usize) < n
                && (y as usize) < m
                && left.get(x as usize) == right.get(y as usize)
            {
                x = x.saturating_add(1);
                y = y.saturating_add(1);
            }

            if let Some(slot) = v.get_mut(idx) {
                *slot = x;
            }

            if (x as usize) >= n && (y as usize) >= m {
                break 'outer;
            }

            k += 2;
        }
    }

    // Backtrack to reconstruct the edit script
    backtrack_edits(&trace, left, right, n, m, offset)
}

/// Backtrack through the Myers trace to reconstruct the edit script.
// Same Myers conventions as `myers_diff`: single-char names and bounded casts.
#[allow(clippy::many_single_char_names, clippy::cast_possible_wrap)]
fn backtrack_edits(
    trace: &[Vec<isize>],
    left: &[&str],
    right: &[&str],
    n: usize,
    m: usize,
    offset: usize,
) -> Vec<DiffEdit> {
    let mut edits = VecDeque::new();
    let mut x = n as isize;
    let mut y = m as isize;

    let num_traces = trace.len();
    for d in (0..num_traces).rev() {
        let Some(v_snap) = trace.get(d) else { break };
        let k = x.saturating_sub(y);
        let d_i = d as isize;

        let go_down = if k == -d_i {
            true
        } else if k == d_i {
            false
        } else {
            let left_idx = (k as usize).wrapping_add(offset).wrapping_sub(1);
            let right_idx = (k as usize).wrapping_add(offset).wrapping_add(1);
            let left_val = v_snap.get(left_idx).copied().unwrap_or(0);
            let right_val = v_snap.get(right_idx).copied().unwrap_or(0);
            left_val < right_val
        };

        let prev_k = if go_down {
            k.saturating_add(1)
        } else {
            k.saturating_sub(1)
        };
        let prev_idx = (prev_k as usize).wrapping_add(offset);
        let prev_x = v_snap.get(prev_idx).copied().unwrap_or(0);
        let prev_y = prev_x.saturating_sub(prev_k);

        // Diagonal (equal)
        while x > prev_x && y > prev_y {
            x = x.saturating_sub(1);
            y = y.saturating_sub(1);
            let lx = x as usize;
            let ly = y as usize;
            edits.push_front(DiffEdit {
                op: DiffOp::Equal,
                left_line: Some(lx),
                right_line: Some(ly),
                text: left.get(lx).unwrap_or(&"").to_string(),
            });
        }

        if d == 0 {
            break;
        }

        if go_down {
            // Insert
            y = y.saturating_sub(1);
            let ly = y as usize;
            edits.push_front(DiffEdit {
                op: DiffOp::Insert,
                left_line: None,
                right_line: Some(ly),
                text: right.get(ly).unwrap_or(&"").to_string(),
            });
        } else {
            // Delete
            x = x.saturating_sub(1);
            let lx = x as usize;
            edits.push_front(DiffEdit {
                op: DiffOp::Delete,
                left_line: Some(lx),
                right_line: None,
                text: left.get(lx).unwrap_or(&"").to_string(),
            });
        }
    }

    edits.into_iter().collect()
}

/// Apply ignore options to a line before comparison.
fn normalize_line(line: &str, opts: IgnoreOptions) -> String {
    let mut result = line.to_string();
    if opts.ignore_case {
        result = result.to_lowercase();
    }
    if opts.ignore_whitespace {
        result = result.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    result
}

/// Compute diff with options, producing a full `DiffResult`.
#[must_use]
pub fn compute_diff(left_text: &str, right_text: &str, opts: &IgnoreOptions) -> DiffResult {
    let left_lines: Vec<&str> = if left_text.is_empty() {
        Vec::new()
    } else {
        left_text.lines().collect()
    };
    let right_lines: Vec<&str> = if right_text.is_empty() {
        Vec::new()
    } else {
        right_text.lines().collect()
    };

    let left_count = left_lines.len();
    let right_count = right_lines.len();

    // Apply normalization for comparison if needed
    let edits = if opts.has_any() {
        let norm_left: Vec<String> = left_lines
            .iter()
            .map(|l| normalize_line(l, *opts))
            .collect();
        let norm_right: Vec<String> = right_lines
            .iter()
            .map(|l| normalize_line(l, *opts))
            .collect();
        let norm_left_refs: Vec<&str> = norm_left.iter().map(String::as_str).collect();
        let norm_right_refs: Vec<&str> = norm_right.iter().map(String::as_str).collect();

        let norm_edits = myers_diff(&norm_left_refs, &norm_right_refs);

        // Map back to original text
        norm_edits
            .into_iter()
            .map(|e| {
                let text = match e.op {
                    DiffOp::Delete | DiffOp::Equal => e
                        .left_line
                        .and_then(|i| left_lines.get(i))
                        .unwrap_or(&"")
                        .to_string(),
                    DiffOp::Insert => e
                        .right_line
                        .and_then(|i| right_lines.get(i))
                        .unwrap_or(&"")
                        .to_string(),
                };
                DiffEdit { text, ..e }
            })
            .collect()
    } else {
        myers_diff(&left_lines, &right_lines)
    };

    // Filter blank lines if requested
    let edits = if opts.ignore_blank_lines {
        edits
            .into_iter()
            .map(|e| {
                if e.text.trim().is_empty() && e.op != DiffOp::Equal {
                    DiffEdit {
                        op: DiffOp::Equal,
                        ..e
                    }
                } else {
                    e
                }
            })
            .collect()
    } else {
        edits
    };

    let hunks = group_into_hunks(&edits, 3);

    DiffResult {
        edits,
        hunks,
        left_line_count: left_count,
        right_line_count: right_count,
    }
}

/// Group edits into hunks with context lines.
fn group_into_hunks(edits: &[DiffEdit], context: usize) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut change_indices: Vec<usize> = Vec::new();

    for (i, edit) in edits.iter().enumerate() {
        if edit.op != DiffOp::Equal {
            change_indices.push(i);
        }
    }

    if change_indices.is_empty() {
        return hunks;
    }

    // Group changes that are within context lines of each other
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let Some(&first_change) = change_indices.first() else {
        return hunks;
    };
    let mut group_start = first_change;
    let mut group_end = group_start;

    for &idx in change_indices.iter().skip(1) {
        if idx.saturating_sub(group_end) <= context.saturating_mul(2).saturating_add(1) {
            group_end = idx;
        } else {
            groups.push((group_start, group_end));
            group_start = idx;
            group_end = idx;
        }
    }
    groups.push((group_start, group_end));

    for (start, end) in &groups {
        let hunk_start = start.saturating_sub(context);
        let hunk_end = (end.saturating_add(context).saturating_add(1)).min(edits.len());

        let hunk_edits: Vec<DiffEdit> = edits.get(hunk_start..hunk_end).unwrap_or(&[]).to_vec();

        let mut left_start: usize = 0;
        let mut right_start: usize = 0;
        let mut left_count: usize = 0;
        let mut right_count: usize = 0;

        if let Some(first) = hunk_edits.first() {
            left_start = first.left_line.unwrap_or(0);
            right_start = first.right_line.unwrap_or(0);
        }

        for edit in &hunk_edits {
            match edit.op {
                DiffOp::Equal => {
                    left_count = left_count.saturating_add(1);
                    right_count = right_count.saturating_add(1);
                }
                DiffOp::Delete => {
                    left_count = left_count.saturating_add(1);
                }
                DiffOp::Insert => {
                    right_count = right_count.saturating_add(1);
                }
            }
        }

        hunks.push(DiffHunk {
            left_start,
            left_count,
            right_start,
            right_count,
            edits: hunk_edits,
        });
    }

    hunks
}

/// Compute character-level inline diff between two lines.
#[must_use]
pub fn inline_diff(left: &str, right: &str) -> (Vec<InlineEdit>, Vec<InlineEdit>) {
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    let n = left_chars.len();
    let m = right_chars.len();

    if n == 0 && m == 0 {
        return (Vec::new(), Vec::new());
    }

    // Find common prefix
    let mut prefix_len = 0;
    while prefix_len < n
        && prefix_len < m
        && left_chars.get(prefix_len) == right_chars.get(prefix_len)
    {
        prefix_len = prefix_len.saturating_add(1);
    }

    // Find common suffix (not overlapping prefix)
    let mut suffix_len = 0;
    while suffix_len < (n.saturating_sub(prefix_len)) && suffix_len < (m.saturating_sub(prefix_len))
    {
        let li = n.saturating_sub(1).saturating_sub(suffix_len);
        let ri = m.saturating_sub(1).saturating_sub(suffix_len);
        if left_chars.get(li) == right_chars.get(ri) {
            suffix_len = suffix_len.saturating_add(1);
        } else {
            break;
        }
    }

    let left_mid_end = n.saturating_sub(suffix_len);
    let right_mid_end = m.saturating_sub(suffix_len);

    // Convert char offsets to byte offsets
    let left_prefix_bytes: usize = left_chars
        .get(..prefix_len)
        .unwrap_or(&[])
        .iter()
        .map(|c| c.len_utf8())
        .sum();
    let left_mid_bytes: usize = left_chars
        .get(prefix_len..left_mid_end)
        .unwrap_or(&[])
        .iter()
        .map(|c| c.len_utf8())
        .sum();

    let right_prefix_bytes: usize = right_chars
        .get(..prefix_len)
        .unwrap_or(&[])
        .iter()
        .map(|c| c.len_utf8())
        .sum();
    let right_mid_bytes: usize = right_chars
        .get(prefix_len..right_mid_end)
        .unwrap_or(&[])
        .iter()
        .map(|c| c.len_utf8())
        .sum();

    let mut left_spans = Vec::new();
    let mut right_spans = Vec::new();

    if prefix_len > 0 {
        left_spans.push(InlineEdit {
            start: 0,
            end: left_prefix_bytes,
            changed: false,
        });
        right_spans.push(InlineEdit {
            start: 0,
            end: right_prefix_bytes,
            changed: false,
        });
    }

    if left_mid_bytes > 0 {
        left_spans.push(InlineEdit {
            start: left_prefix_bytes,
            end: left_prefix_bytes.saturating_add(left_mid_bytes),
            changed: true,
        });
    }
    if right_mid_bytes > 0 {
        right_spans.push(InlineEdit {
            start: right_prefix_bytes,
            end: right_prefix_bytes.saturating_add(right_mid_bytes),
            changed: true,
        });
    }

    let left_suffix_start = left_prefix_bytes.saturating_add(left_mid_bytes);
    let right_suffix_start = right_prefix_bytes.saturating_add(right_mid_bytes);

    if suffix_len > 0 {
        left_spans.push(InlineEdit {
            start: left_suffix_start,
            end: left.len(),
            changed: false,
        });
        right_spans.push(InlineEdit {
            start: right_suffix_start,
            end: right.len(),
            changed: false,
        });
    }

    (left_spans, right_spans)
}

// ============================================================================
// Directory comparison
// ============================================================================

/// Status of a file in a directory comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileCompareStatus {
    /// File is identical in both directories.
    Same,
    /// File differs between directories.
    Different,
    /// File exists only in the left directory.
    OnlyLeft,
    /// File exists only in the right directory.
    OnlyRight,
}

impl fmt::Display for FileCompareStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Same => write!(f, "Same"),
            Self::Different => write!(f, "Different"),
            Self::OnlyLeft => write!(f, "Only in left"),
            Self::OnlyRight => write!(f, "Only in right"),
        }
    }
}

/// A single entry in a directory comparison result.
#[derive(Clone, Debug)]
pub struct DirCompareEntry {
    /// Relative path within the compared directories.
    pub path: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Comparison status.
    pub status: FileCompareStatus,
}

/// Result of comparing two directories.
#[derive(Clone, Debug, Default)]
pub struct DirCompareResult {
    /// All entries found during comparison.
    pub entries: Vec<DirCompareEntry>,
    /// Count of identical files.
    pub same_count: usize,
    /// Count of differing files.
    pub different_count: usize,
    /// Count of files only in left.
    pub only_left_count: usize,
    /// Count of files only in right.
    pub only_right_count: usize,
}

/// Compare two lists of filenames (simulated directory comparison).
#[must_use]
pub fn compare_directories(
    left_files: &[(&str, &str)],
    right_files: &[(&str, &str)],
) -> DirCompareResult {
    let mut result = DirCompareResult::default();

    let mut left_map: Vec<(&str, &str)> = left_files.to_vec();
    left_map.sort_by_key(|(name, _)| *name);

    let mut right_map: Vec<(&str, &str)> = right_files.to_vec();
    right_map.sort_by_key(|(name, _)| *name);

    let mut li = 0;
    let mut ri = 0;

    while li < left_map.len() && ri < right_map.len() {
        let (lname, lcontent) = left_map.get(li).copied().unwrap_or(("", ""));
        let (rname, rcontent) = right_map.get(ri).copied().unwrap_or(("", ""));

        match lname.cmp(rname) {
            std::cmp::Ordering::Equal => {
                if lcontent == rcontent {
                    result.entries.push(DirCompareEntry {
                        path: lname.to_string(),
                        is_dir: false,
                        status: FileCompareStatus::Same,
                    });
                    result.same_count = result.same_count.saturating_add(1);
                } else {
                    result.entries.push(DirCompareEntry {
                        path: lname.to_string(),
                        is_dir: false,
                        status: FileCompareStatus::Different,
                    });
                    result.different_count = result.different_count.saturating_add(1);
                }
                li = li.saturating_add(1);
                ri = ri.saturating_add(1);
            }
            std::cmp::Ordering::Less => {
                result.entries.push(DirCompareEntry {
                    path: lname.to_string(),
                    is_dir: false,
                    status: FileCompareStatus::OnlyLeft,
                });
                result.only_left_count = result.only_left_count.saturating_add(1);
                li = li.saturating_add(1);
            }
            std::cmp::Ordering::Greater => {
                result.entries.push(DirCompareEntry {
                    path: rname.to_string(),
                    is_dir: false,
                    status: FileCompareStatus::OnlyRight,
                });
                result.only_right_count = result.only_right_count.saturating_add(1);
                ri = ri.saturating_add(1);
            }
        }
    }

    while li < left_map.len() {
        let (lname, _) = left_map.get(li).copied().unwrap_or(("", ""));
        result.entries.push(DirCompareEntry {
            path: lname.to_string(),
            is_dir: false,
            status: FileCompareStatus::OnlyLeft,
        });
        result.only_left_count = result.only_left_count.saturating_add(1);
        li = li.saturating_add(1);
    }

    while ri < right_map.len() {
        let (rname, _) = right_map.get(ri).copied().unwrap_or(("", ""));
        result.entries.push(DirCompareEntry {
            path: rname.to_string(),
            is_dir: false,
            status: FileCompareStatus::OnlyRight,
        });
        result.only_right_count = result.only_right_count.saturating_add(1);
        ri = ri.saturating_add(1);
    }

    result
}

// ============================================================================
// Ignore options
// ============================================================================

/// Options for ignoring certain differences.
#[derive(Clone, Copy, Debug, Default)]
pub struct IgnoreOptions {
    /// Ignore leading/trailing whitespace and collapse internal whitespace.
    pub ignore_whitespace: bool,
    /// Ignore case differences.
    pub ignore_case: bool,
    /// Treat blank line insertions/deletions as equal.
    pub ignore_blank_lines: bool,
}

impl IgnoreOptions {
    /// Check if any ignore option is enabled.
    #[must_use]
    pub fn has_any(self) -> bool {
        self.ignore_whitespace || self.ignore_case
    }
}

// ============================================================================
// Search
// ============================================================================

/// A search match within the diff.
#[derive(Clone, Debug)]
pub struct SearchMatch {
    /// Which panel the match is in (0 = left, 1 = right).
    pub panel: u8,
    /// Edit index in the diff result.
    pub edit_index: usize,
    /// Byte offset within the line text.
    pub byte_offset: usize,
    /// Length of the match in bytes.
    pub match_len: usize,
}

/// Search state for find-in-diff.
#[derive(Clone, Debug, Default)]
pub struct SearchState {
    /// Current search query.
    pub query: String,
    /// Whether search is case-sensitive.
    pub case_sensitive: bool,
    /// All matches found.
    pub matches: Vec<SearchMatch>,
    /// Index of the currently focused match.
    pub current_match: usize,
    /// Whether the search bar is visible.
    pub visible: bool,
}

impl SearchState {
    /// Perform a search across diff edits.
    pub fn search(&mut self, edits: &[DiffEdit]) {
        self.matches.clear();
        self.current_match = 0;

        if self.query.is_empty() {
            return;
        }

        let query = if self.case_sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };

        for (i, edit) in edits.iter().enumerate() {
            let text = if self.case_sensitive {
                edit.text.clone()
            } else {
                edit.text.to_lowercase()
            };

            let mut start = 0;
            while let Some(pos) = text.get(start..).and_then(|s| s.find(&query)) {
                let byte_offset = start.saturating_add(pos);
                self.push_matches_for_edit(i, edit.op, byte_offset, query.len());
                start = byte_offset.saturating_add(1);
                if self.matches.len() >= MAX_SEARCH_RESULTS {
                    return;
                }
            }
        }
    }

    /// Push search matches for a given edit based on its operation type.
    fn push_matches_for_edit(
        &mut self,
        edit_index: usize,
        op: DiffOp,
        byte_offset: usize,
        match_len: usize,
    ) {
        match op {
            DiffOp::Equal => {
                self.matches.push(SearchMatch {
                    panel: 0,
                    edit_index,
                    byte_offset,
                    match_len,
                });
                self.matches.push(SearchMatch {
                    panel: 1,
                    edit_index,
                    byte_offset,
                    match_len,
                });
            }
            DiffOp::Delete => {
                self.matches.push(SearchMatch {
                    panel: 0,
                    edit_index,
                    byte_offset,
                    match_len,
                });
            }
            DiffOp::Insert => {
                self.matches.push(SearchMatch {
                    panel: 1,
                    edit_index,
                    byte_offset,
                    match_len,
                });
            }
        }
    }

    /// Move to the next match.
    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match.saturating_add(1)) % self.matches.len();
        }
    }

    /// Move to the previous match.
    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            if self.current_match == 0 {
                self.current_match = self.matches.len().saturating_sub(1);
            } else {
                self.current_match = self.current_match.saturating_sub(1);
            }
        }
    }

    /// Get the current match if any.
    #[must_use]
    pub fn current(&self) -> Option<&SearchMatch> {
        self.matches.get(self.current_match)
    }
}

// ============================================================================
// Merge support
// ============================================================================

/// Decision for merging a hunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeDecision {
    /// Not yet decided.
    Undecided,
    /// Accept the left (original) version.
    AcceptLeft,
    /// Accept the right (new) version.
    AcceptRight,
    /// Accept both (left then right).
    AcceptBoth,
}

/// Merge state for the entire diff.
#[derive(Clone, Debug)]
pub struct MergeState {
    /// Decision for each hunk (indexed by hunk index).
    pub decisions: Vec<MergeDecision>,
}

impl MergeState {
    /// Create a new merge state with all hunks undecided.
    #[must_use]
    pub fn new(hunk_count: usize) -> Self {
        Self {
            decisions: vec![MergeDecision::Undecided; hunk_count],
        }
    }

    /// Set the decision for a specific hunk.
    pub fn set_decision(&mut self, hunk_index: usize, decision: MergeDecision) {
        if let Some(slot) = self.decisions.get_mut(hunk_index) {
            *slot = decision;
        }
    }

    /// Get the decision for a specific hunk.
    #[must_use]
    pub fn get_decision(&self, hunk_index: usize) -> MergeDecision {
        self.decisions
            .get(hunk_index)
            .copied()
            .unwrap_or(MergeDecision::Undecided)
    }

    /// Count of decided hunks.
    #[must_use]
    pub fn decided_count(&self) -> usize {
        self.decisions
            .iter()
            .filter(|d| **d != MergeDecision::Undecided)
            .count()
    }

    /// Apply merge decisions to produce the final merged text.
    #[must_use]
    pub fn apply(&self, diff: &DiffResult) -> String {
        let mut output = String::new();

        for (hi, hunk) in diff.hunks.iter().enumerate() {
            let decision = self.get_decision(hi);
            for edit in &hunk.edits {
                match decision {
                    MergeDecision::Undecided | MergeDecision::AcceptRight => match edit.op {
                        DiffOp::Equal | DiffOp::Insert => {
                            output.push_str(&edit.text);
                            output.push('\n');
                        }
                        DiffOp::Delete => {}
                    },
                    MergeDecision::AcceptLeft => match edit.op {
                        DiffOp::Equal | DiffOp::Delete => {
                            output.push_str(&edit.text);
                            output.push('\n');
                        }
                        DiffOp::Insert => {}
                    },
                    MergeDecision::AcceptBoth => {
                        output.push_str(&edit.text);
                        output.push('\n');
                    }
                }
            }
        }

        output
    }
}

// ============================================================================
// Diff statistics
// ============================================================================

/// Statistics about a diff result.
#[derive(Clone, Debug, Default)]
pub struct DiffStats {
    /// Number of equal lines.
    pub equal_lines: usize,
    /// Number of inserted lines.
    pub inserted_lines: usize,
    /// Number of deleted lines.
    pub deleted_lines: usize,
    /// Total lines in left file.
    pub left_total: usize,
    /// Total lines in right file.
    pub right_total: usize,
    /// Similarity percentage (0.0 to 100.0).
    pub similarity: f32,
}

impl DiffStats {
    /// Compute statistics from a diff result.
    #[must_use]
    pub fn from_diff(diff: &DiffResult) -> Self {
        let mut equal_lines = 0usize;
        let mut inserted_lines = 0usize;
        let mut deleted_lines = 0usize;

        for edit in &diff.edits {
            match edit.op {
                DiffOp::Equal => equal_lines = equal_lines.saturating_add(1),
                DiffOp::Insert => inserted_lines = inserted_lines.saturating_add(1),
                DiffOp::Delete => deleted_lines = deleted_lines.saturating_add(1),
            }
        }

        let total = equal_lines
            .saturating_add(inserted_lines)
            .saturating_add(deleted_lines);
        let similarity = if total == 0 {
            100.0
        } else {
            (equal_lines as f32 / total as f32) * 100.0
        };

        Self {
            equal_lines,
            inserted_lines,
            deleted_lines,
            left_total: diff.left_line_count,
            right_total: diff.right_line_count,
            similarity,
        }
    }

    /// Number of change hunks.
    #[must_use]
    pub fn change_count(&self) -> usize {
        self.inserted_lines.saturating_add(self.deleted_lines)
    }
}

// ============================================================================
// View mode
// ============================================================================

/// Display mode for the diff viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Side-by-side panels.
    SideBySide,
    /// Unified diff format.
    Unified,
    /// Inline with character-level highlighting.
    Inline,
}

impl fmt::Display for ViewMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SideBySide => write!(f, "Side-by-Side"),
            Self::Unified => write!(f, "Unified"),
            Self::Inline => write!(f, "Inline"),
        }
    }
}

// ============================================================================
// Side-by-side pairing
// ============================================================================

/// A paired row for side-by-side display.
#[derive(Clone, Debug)]
struct SideBySidePair {
    left_line: Option<usize>,
    left_text: Option<String>,
    left_op: Option<DiffOp>,
    right_line: Option<usize>,
    right_text: Option<String>,
    right_op: Option<DiffOp>,
}

/// Build side-by-side pairs from an edit list.
///
/// Equal lines appear on both sides. Deletes appear on the left with a blank right.
/// Inserts appear on the right with a blank left. Consecutive delete+insert pairs
/// are aligned on the same row.
fn build_side_by_side_pairs(edits: &[DiffEdit]) -> Vec<SideBySidePair> {
    let mut pairs = Vec::new();
    let mut i = 0;

    while i < edits.len() {
        let Some(edit) = edits.get(i) else { break };

        match edit.op {
            DiffOp::Equal => {
                pairs.push(SideBySidePair {
                    left_line: edit.left_line,
                    left_text: Some(edit.text.clone()),
                    left_op: Some(DiffOp::Equal),
                    right_line: edit.right_line,
                    right_text: Some(edit.text.clone()),
                    right_op: Some(DiffOp::Equal),
                });
                i = i.saturating_add(1);
            }
            DiffOp::Delete => {
                // Check if the next edit is an insert (paired modification)
                let next = edits.get(i.saturating_add(1));
                if let Some(next_edit) = next
                    && next_edit.op == DiffOp::Insert
                {
                    // Paired: show delete on left, insert on right
                    pairs.push(SideBySidePair {
                        left_line: edit.left_line,
                        left_text: Some(edit.text.clone()),
                        left_op: Some(DiffOp::Delete),
                        right_line: next_edit.right_line,
                        right_text: Some(next_edit.text.clone()),
                        right_op: Some(DiffOp::Insert),
                    });
                    i = i.saturating_add(2);
                    continue;
                }
                // Unpaired delete
                pairs.push(SideBySidePair {
                    left_line: edit.left_line,
                    left_text: Some(edit.text.clone()),
                    left_op: Some(DiffOp::Delete),
                    right_line: None,
                    right_text: None,
                    right_op: None,
                });
                i = i.saturating_add(1);
            }
            DiffOp::Insert => {
                pairs.push(SideBySidePair {
                    left_line: None,
                    left_text: None,
                    left_op: None,
                    right_line: edit.right_line,
                    right_text: Some(edit.text.clone()),
                    right_op: Some(DiffOp::Insert),
                });
                i = i.saturating_add(1);
            }
        }
    }

    pairs
}

// ============================================================================
// Inline rows
// ============================================================================

/// A row in the inline diff view.
#[derive(Clone, Debug)]
struct InlineRow {
    op: DiffOp,
    line_num: Option<usize>,
    text: String,
    spans: Vec<InlineEdit>,
}

/// Build inline rows from edits, computing character-level diffs for change pairs.
fn build_inline_rows(edits: &[DiffEdit]) -> Vec<InlineRow> {
    let mut rows = Vec::new();
    let mut i = 0;

    while i < edits.len() {
        let Some(edit) = edits.get(i) else { break };

        match edit.op {
            DiffOp::Equal => {
                rows.push(InlineRow {
                    op: DiffOp::Equal,
                    line_num: edit.left_line,
                    text: edit.text.clone(),
                    spans: Vec::new(),
                });
                i = i.saturating_add(1);
            }
            DiffOp::Delete => {
                let next = edits.get(i.saturating_add(1));
                if let Some(next_edit) = next
                    && next_edit.op == DiffOp::Insert
                {
                    let (left_spans, right_spans) = inline_diff(&edit.text, &next_edit.text);

                    rows.push(InlineRow {
                        op: DiffOp::Delete,
                        line_num: edit.left_line,
                        text: edit.text.clone(),
                        spans: left_spans,
                    });
                    rows.push(InlineRow {
                        op: DiffOp::Insert,
                        line_num: next_edit.right_line,
                        text: next_edit.text.clone(),
                        spans: right_spans,
                    });
                    i = i.saturating_add(2);
                    continue;
                }
                rows.push(InlineRow {
                    op: DiffOp::Delete,
                    line_num: edit.left_line,
                    text: edit.text.clone(),
                    spans: Vec::new(),
                });
                i = i.saturating_add(1);
            }
            DiffOp::Insert => {
                rows.push(InlineRow {
                    op: DiffOp::Insert,
                    line_num: edit.right_line,
                    text: edit.text.clone(),
                    spans: Vec::new(),
                });
                i = i.saturating_add(1);
            }
        }
    }

    rows
}

// ============================================================================
// Diff line rendering data (avoids too-many-arguments on render methods)
// ============================================================================

/// Parameters for rendering a single diff line in side-by-side mode.
struct DiffLineParams<'a> {
    x: f32,
    y: f32,
    width: f32,
    line_num: Option<usize>,
    text: Option<&'a str>,
    op: Option<DiffOp>,
}

// ============================================================================
// Application state
// ============================================================================

/// Main application state.
pub struct FileDiffApp {
    /// Width of the application window.
    pub width: f32,
    /// Height of the application window.
    pub height: f32,

    /// Left file path.
    pub left_path: String,
    /// Right file path.
    pub right_path: String,
    /// Left file content.
    pub left_content: String,
    /// Right file content.
    pub right_content: String,

    /// Current diff result.
    pub diff: Option<DiffResult>,
    /// Diff statistics.
    pub stats: DiffStats,
    /// Merge state.
    pub merge: Option<MergeState>,

    /// Current view mode.
    pub view_mode: ViewMode,
    /// Scroll offset (in lines) for the left panel.
    pub scroll_left: f32,
    /// Scroll offset (in lines) for the right panel.
    pub scroll_right: f32,
    /// Whether scroll is synchronized between panels.
    pub sync_scroll: bool,
    /// Index of the current change being viewed.
    pub current_change_index: usize,
    /// Indices of change edits in the edit list (for navigation).
    pub change_indices: Vec<usize>,

    /// Ignore options.
    pub ignore_opts: IgnoreOptions,
    /// Search state.
    pub search: SearchState,

    /// Directory comparison result (when in directory mode).
    pub dir_compare: Option<DirCompareResult>,
    /// Whether we are in directory comparison mode.
    pub dir_mode: bool,

    /// Scroll offset for directory comparison view.
    pub dir_scroll: f32,

    /// Currently selected hunk index for merge operations.
    pub selected_hunk: usize,

    /// Whether the toolbar dropdown for view mode is open.
    pub view_mode_dropdown_open: bool,
}

impl Default for FileDiffApp {
    fn default() -> Self {
        Self::new()
    }
}

impl FileDiffApp {
    /// Create a new application instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            width: 1200.0,
            height: 800.0,
            left_path: String::new(),
            right_path: String::new(),
            left_content: String::new(),
            right_content: String::new(),
            diff: None,
            stats: DiffStats::default(),
            merge: None,
            view_mode: ViewMode::SideBySide,
            scroll_left: 0.0,
            scroll_right: 0.0,
            sync_scroll: true,
            current_change_index: 0,
            change_indices: Vec::new(),
            ignore_opts: IgnoreOptions::default(),
            search: SearchState::default(),
            dir_compare: None,
            dir_mode: false,
            dir_scroll: 0.0,
            selected_hunk: 0,
            view_mode_dropdown_open: false,
        }
    }

    /// Load two files for comparison.
    pub fn load_files(
        &mut self,
        left_path: &str,
        left_content: &str,
        right_path: &str,
        right_content: &str,
    ) {
        self.left_path = left_path.to_string();
        self.right_path = right_path.to_string();
        self.left_content = left_content.to_string();
        self.right_content = right_content.to_string();
        self.dir_mode = false;
        self.dir_compare = None;
        self.recompute_diff();
    }

    /// Recompute the diff with current options.
    pub fn recompute_diff(&mut self) {
        let diff = compute_diff(&self.left_content, &self.right_content, &self.ignore_opts);
        self.stats = DiffStats::from_diff(&diff);

        // Build change index list for navigation
        self.change_indices.clear();
        for (i, edit) in diff.edits.iter().enumerate() {
            if edit.op != DiffOp::Equal {
                self.change_indices.push(i);
            }
        }

        let hunk_count = diff.hunks.len();
        self.merge = Some(MergeState::new(hunk_count));
        self.diff = Some(diff);
        self.current_change_index = 0;
        self.scroll_left = 0.0;
        self.scroll_right = 0.0;
        self.selected_hunk = 0;

        // Re-run search if active
        if self.search.visible
            && !self.search.query.is_empty()
            && let Some(ref diff) = self.diff
        {
            self.search.search(&diff.edits);
        }
    }

    /// Navigate to the next change.
    pub fn next_change(&mut self) {
        if !self.change_indices.is_empty() {
            self.current_change_index =
                (self.current_change_index.saturating_add(1)) % self.change_indices.len();
            self.scroll_to_current_change();
        }
    }

    /// Navigate to the previous change.
    pub fn prev_change(&mut self) {
        if !self.change_indices.is_empty() {
            if self.current_change_index == 0 {
                self.current_change_index = self.change_indices.len().saturating_sub(1);
            } else {
                self.current_change_index = self.current_change_index.saturating_sub(1);
            }
            self.scroll_to_current_change();
        }
    }

    /// Scroll to make the current change visible.
    fn scroll_to_current_change(&mut self) {
        if let Some(&edit_idx) = self.change_indices.get(self.current_change_index) {
            let target_line = edit_idx as f32;
            let visible_lines = self.visible_line_count();
            let half_visible = visible_lines / 2.0;
            let target_scroll = (target_line - half_visible).max(0.0);
            self.scroll_left = target_scroll;
            if self.sync_scroll {
                self.scroll_right = target_scroll;
            }
        }
    }

    /// Number of lines visible in the content area.
    fn visible_line_count(&self) -> f32 {
        let content_height = self.height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;
        content_height / LINE_HEIGHT
    }

    /// Maximum scroll value.
    fn max_scroll(&self) -> f32 {
        if let Some(ref diff) = self.diff {
            let total_lines = diff.edits.len() as f32;
            (total_lines - self.visible_line_count()).max(0.0)
        } else {
            0.0
        }
    }

    /// Toggle an ignore option and recompute.
    pub fn toggle_ignore_whitespace(&mut self) {
        self.ignore_opts.ignore_whitespace = !self.ignore_opts.ignore_whitespace;
        self.recompute_diff();
    }

    /// Toggle case ignore and recompute.
    pub fn toggle_ignore_case(&mut self) {
        self.ignore_opts.ignore_case = !self.ignore_opts.ignore_case;
        self.recompute_diff();
    }

    /// Toggle blank line ignore and recompute.
    pub fn toggle_ignore_blank_lines(&mut self) {
        self.ignore_opts.ignore_blank_lines = !self.ignore_opts.ignore_blank_lines;
        self.recompute_diff();
    }

    /// Accept left side for the selected hunk.
    pub fn accept_left(&mut self) {
        if let Some(ref mut merge) = self.merge {
            merge.set_decision(self.selected_hunk, MergeDecision::AcceptLeft);
        }
    }

    /// Accept right side for the selected hunk.
    pub fn accept_right(&mut self) {
        if let Some(ref mut merge) = self.merge {
            merge.set_decision(self.selected_hunk, MergeDecision::AcceptRight);
        }
    }

    /// Accept both sides for the selected hunk.
    pub fn accept_both(&mut self) {
        if let Some(ref mut merge) = self.merge {
            merge.set_decision(self.selected_hunk, MergeDecision::AcceptBoth);
        }
    }

    /// Get the merged output text.
    #[must_use]
    pub fn merged_text(&self) -> Option<String> {
        match (&self.merge, &self.diff) {
            (Some(merge), Some(diff)) => Some(merge.apply(diff)),
            _ => None,
        }
    }

    /// Handle events from the UI.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Resize { width, height } => {
                self.width = *width as f32;
                self.height = *height as f32;
                EventResult::Consumed
            }
            Event::Key(key_event) if key_event.pressed => self.handle_key(key_event),
            Event::Mouse(mouse_event) => self.handle_mouse(mouse_event),
            _ => EventResult::Ignored,
        }
    }

    /// Handle keyboard input.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if self.search.visible {
            return self.handle_search_key(key);
        }

        match key.key {
            // Navigation
            Key::Down | Key::J => {
                self.scroll_left = (self.scroll_left + 1.0).min(self.max_scroll());
                if self.sync_scroll {
                    self.scroll_right = self.scroll_left;
                }
                EventResult::Consumed
            }
            Key::Up | Key::K => {
                self.scroll_left = (self.scroll_left - 1.0).max(0.0);
                if self.sync_scroll {
                    self.scroll_right = self.scroll_left;
                }
                EventResult::Consumed
            }
            Key::PageDown => {
                let page = self.visible_line_count();
                self.scroll_left = (self.scroll_left + page).min(self.max_scroll());
                if self.sync_scroll {
                    self.scroll_right = self.scroll_left;
                }
                EventResult::Consumed
            }
            Key::PageUp => {
                let page = self.visible_line_count();
                self.scroll_left = (self.scroll_left - page).max(0.0);
                if self.sync_scroll {
                    self.scroll_right = self.scroll_left;
                }
                EventResult::Consumed
            }
            Key::Home if key.modifiers.ctrl => {
                self.scroll_left = 0.0;
                if self.sync_scroll {
                    self.scroll_right = 0.0;
                }
                EventResult::Consumed
            }
            Key::End if key.modifiers.ctrl => {
                self.scroll_left = self.max_scroll();
                if self.sync_scroll {
                    self.scroll_right = self.scroll_left;
                }
                EventResult::Consumed
            }

            // Change navigation (F7/F8 or Ctrl+N/P)
            Key::F7 => {
                self.prev_change();
                EventResult::Consumed
            }
            Key::F8 => {
                self.next_change();
                EventResult::Consumed
            }
            Key::N if key.modifiers.ctrl => {
                self.next_change();
                EventResult::Consumed
            }
            Key::P if key.modifiers.ctrl => {
                self.prev_change();
                EventResult::Consumed
            }

            // View mode toggle
            Key::Num1 if key.modifiers.ctrl => {
                self.view_mode = ViewMode::SideBySide;
                EventResult::Consumed
            }
            Key::Num2 if key.modifiers.ctrl => {
                self.view_mode = ViewMode::Unified;
                EventResult::Consumed
            }
            Key::Num3 if key.modifiers.ctrl => {
                self.view_mode = ViewMode::Inline;
                EventResult::Consumed
            }

            // Sync scroll toggle
            Key::S if key.modifiers.ctrl && key.modifiers.shift => {
                self.sync_scroll = !self.sync_scroll;
                EventResult::Consumed
            }

            // Search
            Key::F if key.modifiers.ctrl => {
                self.search.visible = true;
                EventResult::Consumed
            }

            // Merge actions
            Key::Left if key.modifiers.alt => {
                self.accept_left();
                EventResult::Consumed
            }
            Key::Right if key.modifiers.alt => {
                self.accept_right();
                EventResult::Consumed
            }
            Key::B if key.modifiers.alt => {
                self.accept_both();
                EventResult::Consumed
            }

            // Hunk navigation for merge
            Key::Tab => {
                if let Some(ref diff) = self.diff {
                    let hunk_count = diff.hunks.len();
                    if hunk_count > 0 {
                        self.selected_hunk = (self.selected_hunk.saturating_add(1)) % hunk_count;
                    }
                }
                EventResult::Consumed
            }

            // Ignore toggles
            Key::W if key.modifiers.alt => {
                self.toggle_ignore_whitespace();
                EventResult::Consumed
            }
            Key::C if key.modifiers.alt => {
                self.toggle_ignore_case();
                EventResult::Consumed
            }

            _ => EventResult::Ignored,
        }
    }

    /// Handle keyboard input when search bar is active.
    fn handle_search_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.search.visible = false;
                EventResult::Consumed
            }
            Key::Enter => {
                self.search.next_match();
                EventResult::Consumed
            }
            Key::Backspace => {
                self.search.query.pop();
                if let Some(ref diff) = self.diff {
                    self.search.search(&diff.edits);
                }
                EventResult::Consumed
            }
            Key::F3 => {
                if key.modifiers.shift {
                    self.search.prev_match();
                } else {
                    self.search.next_match();
                }
                EventResult::Consumed
            }
            _ => {
                if let Some(ch) = key.text {
                    self.search.query.push(ch);
                    if let Some(ref diff) = self.diff {
                        self.search.search(&diff.edits);
                    }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    /// Handle mouse input.
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match &mouse.kind {
            MouseEventKind::Scroll { dy, .. } => {
                let delta = -dy * SCROLL_SPEED;
                let max = self.max_scroll();

                // Determine which panel was scrolled based on x position
                let mid_x = self.width / 2.0;
                if self.sync_scroll || self.view_mode != ViewMode::SideBySide {
                    self.scroll_left = (self.scroll_left + delta).clamp(0.0, max);
                    self.scroll_right = self.scroll_left;
                } else if mouse.x < mid_x {
                    self.scroll_left = (self.scroll_left + delta).clamp(0.0, max);
                } else {
                    self.scroll_right = (self.scroll_right + delta).clamp(0.0, max);
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Render the entire application to a render tree.
    #[must_use]
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: colors::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut tree);

        let content_y = TOOLBAR_HEIGHT;
        let content_height = self.height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        if self.dir_mode {
            self.render_dir_compare(&mut tree, content_y, content_height);
        } else if let Some(ref diff) = self.diff {
            tree.push(RenderCommand::PushClip {
                x: 0.0,
                y: content_y,
                width: self.width,
                height: content_height,
            });

            match self.view_mode {
                ViewMode::SideBySide => {
                    self.render_side_by_side(&mut tree, diff, content_y, content_height);
                }
                ViewMode::Unified => {
                    self.render_unified(&mut tree, diff, content_y, content_height);
                }
                ViewMode::Inline => {
                    self.render_inline(&mut tree, diff, content_y, content_height);
                }
            }

            tree.push(RenderCommand::PopClip);
        } else {
            // No diff loaded — show placeholder
            self.render_empty_state(&mut tree, content_y, content_height);
        }

        if self.search.visible {
            self.render_search_bar(&mut tree);
        }

        self.render_status_bar(&mut tree);

        tree
    }

    /// Render the toolbar area.
    fn render_toolbar(&self, tree: &mut RenderTree) {
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: TOOLBAR_HEIGHT,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let mut btn_x: f32 = 8.0;
        let btn_y: f32 = 6.0;
        let btn_h: f32 = 26.0;

        // View mode buttons
        self.render_view_mode_buttons(tree, &mut btn_x, btn_y, btn_h);

        // Separator
        btn_x += 8.0;
        tree.push(RenderCommand::Line {
            x1: btn_x,
            y1: 8.0,
            x2: btn_x,
            y2: TOOLBAR_HEIGHT - 8.0,
            color: colors::SURFACE1,
            width: 1.0,
        });
        btn_x += 14.0;

        // Navigation buttons
        self.render_nav_buttons(tree, &mut btn_x, btn_y, btn_h);

        // Separator
        btn_x += 8.0;
        tree.push(RenderCommand::Line {
            x1: btn_x,
            y1: 8.0,
            x2: btn_x,
            y2: TOOLBAR_HEIGHT - 8.0,
            color: colors::SURFACE1,
            width: 1.0,
        });
        btn_x += 14.0;

        // Ignore option toggles
        self.render_ignore_toggles(tree, &mut btn_x, btn_y, btn_h);

        // Sync scroll indicator (right-aligned)
        self.render_sync_indicator(tree, btn_y, btn_h);

        // Toolbar bottom border
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.width,
            y2: TOOLBAR_HEIGHT,
            color: colors::SURFACE0,
            width: 1.0,
        });
    }

    /// Render view mode toggle buttons.
    fn render_view_mode_buttons(
        &self,
        tree: &mut RenderTree,
        btn_x: &mut f32,
        btn_y: f32,
        btn_h: f32,
    ) {
        let modes = [
            (ViewMode::SideBySide, "Side-by-Side"),
            (ViewMode::Unified, "Unified"),
            (ViewMode::Inline, "Inline"),
        ];

        for (mode, label) in &modes {
            let btn_w = label.len() as f32 * CHAR_WIDTH + 16.0;
            let is_active = self.view_mode == *mode;

            tree.push(RenderCommand::FillRect {
                x: *btn_x,
                y: btn_y,
                width: btn_w,
                height: btn_h,
                color: if is_active {
                    colors::SURFACE1
                } else {
                    colors::SURFACE0
                },
                corner_radii: CornerRadii::all(4.0),
            });

            tree.push(RenderCommand::Text {
                x: *btn_x + 8.0,
                y: btn_y + 7.0,
                text: (*label).to_string(),
                color: if is_active {
                    colors::BLUE
                } else {
                    colors::TEXT
                },
                font_size: UI_FONT_SIZE,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            *btn_x += btn_w + 6.0;
        }
    }

    /// Render navigation buttons.
    // Kept as a `&self` method for consistency with the rest of the
    // `render_*` toolbar family, several of which do read `self`.
    #[allow(clippy::unused_self)]
    fn render_nav_buttons(&self, tree: &mut RenderTree, btn_x: &mut f32, btn_y: f32, btn_h: f32) {
        let nav_buttons = [("Prev", "F7"), ("Next", "F8")];
        for (label, shortcut) in &nav_buttons {
            let full_label = format!("{label} ({shortcut})");
            let btn_w = full_label.len() as f32 * CHAR_WIDTH + 16.0;

            tree.push(RenderCommand::FillRect {
                x: *btn_x,
                y: btn_y,
                width: btn_w,
                height: btn_h,
                color: colors::SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            tree.push(RenderCommand::Text {
                x: *btn_x + 8.0,
                y: btn_y + 7.0,
                text: full_label,
                color: colors::TEXT,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            *btn_x += btn_w + 6.0;
        }
    }

    /// Render ignore option toggle buttons.
    fn render_ignore_toggles(
        &self,
        tree: &mut RenderTree,
        btn_x: &mut f32,
        btn_y: f32,
        btn_h: f32,
    ) {
        let ignore_toggles = [
            ("WS", self.ignore_opts.ignore_whitespace),
            ("Case", self.ignore_opts.ignore_case),
            ("Blank", self.ignore_opts.ignore_blank_lines),
        ];
        for (label, active) in &ignore_toggles {
            let btn_w = label.len() as f32 * CHAR_WIDTH + 16.0;

            tree.push(RenderCommand::FillRect {
                x: *btn_x,
                y: btn_y,
                width: btn_w,
                height: btn_h,
                color: if *active {
                    colors::SURFACE1
                } else {
                    colors::SURFACE0
                },
                corner_radii: CornerRadii::all(4.0),
            });

            tree.push(RenderCommand::Text {
                x: *btn_x + 8.0,
                y: btn_y + 7.0,
                text: (*label).to_string(),
                color: if *active {
                    colors::TEAL
                } else {
                    colors::SUBTEXT0
                },
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            *btn_x += btn_w + 6.0;
        }
    }

    /// Render the scroll sync indicator button (right-aligned).
    fn render_sync_indicator(&self, tree: &mut RenderTree, btn_y: f32, btn_h: f32) {
        let sync_label = if self.sync_scroll {
            "Sync: ON"
        } else {
            "Sync: OFF"
        };
        let sync_w = sync_label.len() as f32 * CHAR_WIDTH + 16.0;
        let sync_x = self.width - sync_w - 8.0;

        tree.push(RenderCommand::FillRect {
            x: sync_x,
            y: btn_y,
            width: sync_w,
            height: btn_h,
            color: if self.sync_scroll {
                colors::SURFACE1
            } else {
                colors::SURFACE0
            },
            corner_radii: CornerRadii::all(4.0),
        });

        tree.push(RenderCommand::Text {
            x: sync_x + 8.0,
            y: btn_y + 7.0,
            text: sync_label.to_string(),
            color: if self.sync_scroll {
                colors::GREEN
            } else {
                colors::OVERLAY0
            },
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render side-by-side diff view.
    fn render_side_by_side(
        &self,
        tree: &mut RenderTree,
        diff: &DiffResult,
        content_y: f32,
        content_height: f32,
    ) {
        let panel_width = (self.width - SEPARATOR_WIDTH) / 2.0;
        let first_visible = self.scroll_left as usize;
        let visible_count = (content_height / LINE_HEIGHT) as usize + 2;

        // Left panel header
        render_panel_header(tree, 0.0, content_y, panel_width, &self.left_path);
        // Right panel header
        render_panel_header(
            tree,
            panel_width + SEPARATOR_WIDTH,
            content_y,
            panel_width,
            &self.right_path,
        );

        let header_h = LINE_HEIGHT;
        let lines_y = content_y + header_h;

        // Separator line
        tree.push(RenderCommand::FillRect {
            x: panel_width,
            y: content_y,
            width: SEPARATOR_WIDTH,
            height: content_height,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Render visible lines
        let pairs = build_side_by_side_pairs(&diff.edits);

        let end = (first_visible.saturating_add(visible_count)).min(pairs.len());
        for (vi, pair_idx) in (first_visible..end).enumerate() {
            let y = lines_y + vi as f32 * LINE_HEIGHT;
            if let Some(pair) = pairs.get(pair_idx) {
                // Left side
                render_diff_line(
                    tree,
                    &DiffLineParams {
                        x: 0.0,
                        y,
                        width: panel_width,
                        line_num: pair.left_line,
                        text: pair.left_text.as_deref(),
                        op: pair.left_op,
                    },
                );
                // Right side
                render_diff_line(
                    tree,
                    &DiffLineParams {
                        x: panel_width + SEPARATOR_WIDTH,
                        y,
                        width: panel_width,
                        line_num: pair.right_line,
                        text: pair.right_text.as_deref(),
                        op: pair.right_op,
                    },
                );
            }
        }

        // Scrollbars
        self.render_scrollbar(
            tree,
            panel_width - 8.0,
            lines_y,
            content_height - header_h,
            self.scroll_left,
            pairs.len() as f32,
        );
        self.render_scrollbar(
            tree,
            self.width - 8.0,
            lines_y,
            content_height - header_h,
            self.scroll_right,
            pairs.len() as f32,
        );
    }

    /// Render unified diff view.
    fn render_unified(
        &self,
        tree: &mut RenderTree,
        diff: &DiffResult,
        content_y: f32,
        content_height: f32,
    ) {
        let first_visible = self.scroll_left as usize;
        let visible_count = (content_height / LINE_HEIGHT) as usize + 2;

        let end = (first_visible.saturating_add(visible_count)).min(diff.edits.len());
        for (vi, edit_idx) in (first_visible..end).enumerate() {
            let y = content_y + vi as f32 * LINE_HEIGHT;
            if let Some(edit) = diff.edits.get(edit_idx) {
                self.render_unified_line(tree, y, edit);
            }
        }

        // Scrollbar
        self.render_scrollbar(
            tree,
            self.width - 8.0,
            content_y,
            content_height,
            self.scroll_left,
            diff.edits.len() as f32,
        );
    }

    /// Render a single unified diff line.
    fn render_unified_line(&self, tree: &mut RenderTree, y: f32, edit: &DiffEdit) {
        let (bg_color, prefix, text_color) = match edit.op {
            DiffOp::Equal => (colors::BASE, " ", colors::TEXT),
            DiffOp::Insert => (colors::ADD_BG, "+", colors::GREEN),
            DiffOp::Delete => (colors::DEL_BG, "-", colors::RED),
        };

        // Background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: LINE_HEIGHT,
            color: bg_color,
            corner_radii: CornerRadii::ZERO,
        });

        // Left line number
        if let Some(ln) = edit.left_line {
            let ln_text = format!("{}", ln.saturating_add(1));
            tree.push(RenderCommand::Text {
                x: PANEL_PADDING,
                y: y + 3.0,
                text: ln_text,
                color: colors::OVERLAY0,
                font_size: CONTENT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(GUTTER_WIDTH - 4.0),
            });
        }

        // Right line number
        if let Some(rn) = edit.right_line {
            let rn_text = format!("{}", rn.saturating_add(1));
            tree.push(RenderCommand::Text {
                x: GUTTER_WIDTH + PANEL_PADDING,
                y: y + 3.0,
                text: rn_text,
                color: colors::OVERLAY0,
                font_size: CONTENT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(GUTTER_WIDTH - 4.0),
            });
        }

        // Prefix
        let prefix_x = GUTTER_WIDTH * 2.0 + PANEL_PADDING;
        tree.push(RenderCommand::Text {
            x: prefix_x,
            y: y + 3.0,
            text: prefix.to_string(),
            color: text_color,
            font_size: CONTENT_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Text content
        let text_x = prefix_x + CHAR_WIDTH * 2.0;
        tree.push(RenderCommand::Text {
            x: text_x,
            y: y + 3.0,
            text: edit.text.clone(),
            color: text_color,
            font_size: CONTENT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - text_x - 12.0),
        });
    }

    /// Render inline diff view with character-level highlighting.
    fn render_inline(
        &self,
        tree: &mut RenderTree,
        diff: &DiffResult,
        content_y: f32,
        content_height: f32,
    ) {
        let first_visible = self.scroll_left as usize;
        let visible_count = (content_height / LINE_HEIGHT) as usize + 2;

        let inline_rows = build_inline_rows(&diff.edits);

        let end = (first_visible.saturating_add(visible_count)).min(inline_rows.len());
        for (vi, row_idx) in (first_visible..end).enumerate() {
            let y = content_y + vi as f32 * LINE_HEIGHT;
            if let Some(row) = inline_rows.get(row_idx) {
                self.render_inline_row(tree, y, row);
            }
        }

        // Scrollbar
        self.render_scrollbar(
            tree,
            self.width - 8.0,
            content_y,
            content_height,
            self.scroll_left,
            inline_rows.len() as f32,
        );
    }

    /// Render a single inline row with character-level highlights.
    fn render_inline_row(&self, tree: &mut RenderTree, y: f32, row: &InlineRow) {
        let bg_color = match row.op {
            DiffOp::Equal => colors::BASE,
            DiffOp::Insert => colors::ADD_BG,
            DiffOp::Delete => colors::DEL_BG,
        };

        // Background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: LINE_HEIGHT,
            color: bg_color,
            corner_radii: CornerRadii::ZERO,
        });

        // Line number
        if let Some(ln) = row.line_num {
            let ln_text = format!("{}", ln.saturating_add(1));
            tree.push(RenderCommand::Text {
                x: PANEL_PADDING,
                y: y + 3.0,
                text: ln_text,
                color: colors::OVERLAY0,
                font_size: CONTENT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(GUTTER_WIDTH - 4.0),
            });
        }

        // Prefix
        let (prefix, prefix_color) = match row.op {
            DiffOp::Equal => (" ", colors::TEXT),
            DiffOp::Insert => ("+", colors::GREEN),
            DiffOp::Delete => ("-", colors::RED),
        };
        tree.push(RenderCommand::Text {
            x: GUTTER_WIDTH + PANEL_PADDING,
            y: y + 3.0,
            text: prefix.to_string(),
            color: prefix_color,
            font_size: CONTENT_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Render text with inline highlights
        let text_x = GUTTER_WIDTH + PANEL_PADDING + CHAR_WIDTH * 2.0;
        if row.spans.is_empty() {
            tree.push(RenderCommand::Text {
                x: text_x,
                y: y + 3.0,
                text: row.text.clone(),
                color: prefix_color,
                font_size: CONTENT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - text_x - 12.0),
            });
        } else {
            self.render_inline_spans(tree, text_x, y, row);
        }
    }

    /// Render character-level spans for an inline row.
    // Kept as a `&self` method for consistency with the rest of the
    // `render_*` row family, several of which do read `self`.
    #[allow(clippy::unused_self)]
    fn render_inline_spans(&self, tree: &mut RenderTree, text_x: f32, y: f32, row: &InlineRow) {
        let mut char_offset: f32 = 0.0;
        for span in &row.spans {
            let span_text = row.text.get(span.start..span.end).unwrap_or("");
            if span_text.is_empty() {
                continue;
            }

            if span.changed {
                let span_w = span_text.len() as f32 * CHAR_WIDTH;
                let highlight_color = match row.op {
                    DiffOp::Insert => colors::ADD_LINE_BG,
                    DiffOp::Delete => colors::DEL_LINE_BG,
                    DiffOp::Equal => colors::BASE,
                };
                tree.push(RenderCommand::FillRect {
                    x: text_x + char_offset,
                    y,
                    width: span_w,
                    height: LINE_HEIGHT,
                    color: highlight_color,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            tree.push(RenderCommand::Text {
                x: text_x + char_offset,
                y: y + 3.0,
                text: span_text.to_string(),
                color: if span.changed {
                    match row.op {
                        DiffOp::Insert => colors::GREEN,
                        DiffOp::Delete => colors::RED,
                        DiffOp::Equal => colors::TEXT,
                    }
                } else {
                    colors::TEXT
                },
                font_size: CONTENT_FONT_SIZE,
                font_weight: if span.changed {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            char_offset += span_text.len() as f32 * CHAR_WIDTH;
        }
    }

    /// Render the empty state when no files are loaded.
    fn render_empty_state(&self, tree: &mut RenderTree, y: f32, height: f32) {
        let center_x = self.width / 2.0;
        let center_y = y + height / 2.0;

        tree.push(RenderCommand::Text {
            x: center_x - 120.0,
            y: center_y - 30.0,
            text: "File Diff/Compare Tool".to_string(),
            color: colors::TEXT,
            font_size: 20.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        tree.push(RenderCommand::Text {
            x: center_x - 140.0,
            y: center_y + 10.0,
            text: "Open two files to compare them".to_string(),
            color: colors::SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render directory comparison view.
    fn render_dir_compare(&self, tree: &mut RenderTree, y: f32, height: f32) {
        let Some(result) = &self.dir_compare else {
            return;
        };

        // Header
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: LINE_HEIGHT,
            color: colors::CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let summary = format!(
            "Directory Compare: {} same, {} different, {} left only, {} right only",
            result.same_count,
            result.different_count,
            result.only_left_count,
            result.only_right_count,
        );
        tree.push(RenderCommand::Text {
            x: PANEL_PADDING + 4.0,
            y: y + 3.0,
            text: summary,
            color: colors::SUBTEXT1,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - 16.0),
        });

        // Entries
        let list_y = y + LINE_HEIGHT;
        let first_visible = self.dir_scroll as usize;
        let visible_count = (height / LINE_HEIGHT) as usize;
        let end = (first_visible.saturating_add(visible_count)).min(result.entries.len());

        for (vi, entry_idx) in (first_visible..end).enumerate() {
            let ey = list_y + vi as f32 * LINE_HEIGHT;
            if let Some(entry) = result.entries.get(entry_idx) {
                render_dir_entry(tree, ey, entry);
            }
        }
    }

    /// Render the search bar overlay.
    fn render_search_bar(&self, tree: &mut RenderTree) {
        let bar_h: f32 = 36.0;
        let bar_y = TOOLBAR_HEIGHT;
        let bar_w = 400.0f32.min(self.width - 20.0);
        let bar_x = self.width - bar_w - 10.0;

        // Background
        tree.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Border
        tree.push(RenderCommand::StrokeRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: colors::BLUE,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Search label
        tree.push(RenderCommand::Text {
            x: bar_x + 8.0,
            y: bar_y + 10.0,
            text: "Find:".to_string(),
            color: colors::SUBTEXT0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Query text
        if !self.search.query.is_empty() {
            tree.push(RenderCommand::Text {
                x: bar_x + 48.0,
                y: bar_y + 10.0,
                text: self.search.query.clone(),
                color: colors::TEXT,
                font_size: CONTENT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(bar_w - 140.0),
            });
        }

        // Match count
        let match_info = if self.search.matches.is_empty() {
            "No matches".to_string()
        } else {
            format!(
                "{}/{}",
                self.search.current_match.saturating_add(1),
                self.search.matches.len()
            )
        };
        tree.push(RenderCommand::Text {
            x: bar_x + bar_w - 80.0,
            y: bar_y + 10.0,
            text: match_info,
            color: colors::SUBTEXT0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the status bar at the bottom.
    fn render_status_bar(&self, tree: &mut RenderTree) {
        let y = self.height - STATUS_BAR_HEIGHT;

        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: STATUS_BAR_HEIGHT,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.width,
            y2: y,
            color: colors::SURFACE0,
            width: 1.0,
        });

        let mut text_x: f32 = 10.0;
        let text_y = y + 7.0;

        // View mode
        tree.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: format!("{}", self.view_mode),
            color: colors::BLUE,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        text_x += 100.0;

        if self.diff.is_some() {
            self.render_status_stats(tree, &mut text_x, text_y);
        }
    }

    /// Render statistics section of the status bar.
    fn render_status_stats(&self, tree: &mut RenderTree, text_x: &mut f32, text_y: f32) {
        // Change navigation position
        let change_info = if self.change_indices.is_empty() {
            "No changes".to_string()
        } else {
            format!(
                "Change {}/{} ",
                self.current_change_index.saturating_add(1),
                self.change_indices.len()
            )
        };
        tree.push(RenderCommand::Text {
            x: *text_x,
            y: text_y,
            text: change_info,
            color: colors::PEACH,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        *text_x += 130.0;

        // Line stats
        let stats_text = format!(
            "+{} -{} ~{:.0}%",
            self.stats.inserted_lines, self.stats.deleted_lines, self.stats.similarity,
        );
        tree.push(RenderCommand::Text {
            x: *text_x,
            y: text_y,
            text: stats_text,
            color: colors::TEXT,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        *text_x += 140.0;

        // Left/right totals
        let totals_text = format!("L:{} R:{}", self.stats.left_total, self.stats.right_total);
        tree.push(RenderCommand::Text {
            x: *text_x,
            y: text_y,
            text: totals_text,
            color: colors::SUBTEXT0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Merge status (right-aligned)
        if let Some(ref merge) = self.merge {
            let total_hunks = merge.decisions.len();
            if total_hunks > 0 {
                let decided = merge.decided_count();
                let merge_text = format!("Merge: {decided}/{total_hunks}");
                let merge_w = merge_text.len() as f32 * CHAR_WIDTH + 8.0;
                tree.push(RenderCommand::Text {
                    x: self.width - merge_w - 8.0,
                    y: text_y,
                    text: merge_text,
                    color: if decided == total_hunks {
                        colors::GREEN
                    } else {
                        colors::YELLOW
                    },
                    font_size: UI_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }
    }

    /// Render a scrollbar track and thumb.
    fn render_scrollbar(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        height: f32,
        scroll_pos: f32,
        total_lines: f32,
    ) {
        if total_lines <= 0.0 {
            return;
        }
        let visible = self.visible_line_count();
        if visible >= total_lines {
            return;
        }

        let track_w: f32 = 6.0;

        // Track
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: track_w,
            height,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });

        // Thumb
        let ratio = visible / total_lines;
        let thumb_h = (height * ratio).max(20.0);
        let scroll_ratio = if total_lines > visible {
            scroll_pos / (total_lines - visible)
        } else {
            0.0
        };
        let thumb_y = y + scroll_ratio * (height - thumb_h);

        tree.push(RenderCommand::FillRect {
            x,
            y: thumb_y,
            width: track_w,
            height: thumb_h,
            color: colors::SURFACE2,
            corner_radii: CornerRadii::all(3.0),
        });
    }
}

// ============================================================================
// Free functions for rendering (avoid unused_self and too_many_arguments)
// ============================================================================

/// Render a panel header with file path.
fn render_panel_header(tree: &mut RenderTree, x: f32, y: f32, width: f32, path: &str) {
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: LINE_HEIGHT,
        color: colors::CRUST,
        corner_radii: CornerRadii::ZERO,
    });

    let display_path = if path.is_empty() { "(no file)" } else { path };
    tree.push(RenderCommand::Text {
        x: x + PANEL_PADDING + 4.0,
        y: y + 3.0,
        text: display_path.to_string(),
        color: colors::SUBTEXT1,
        font_size: UI_FONT_SIZE,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 12.0),
    });
}

/// Render a single diff line (used in side-by-side mode).
fn render_diff_line(tree: &mut RenderTree, params: &DiffLineParams<'_>) {
    let bg_color = match params.op {
        Some(DiffOp::Insert) => colors::ADD_BG,
        Some(DiffOp::Delete) => colors::DEL_BG,
        Some(DiffOp::Equal) | None => colors::BASE,
    };

    // Background
    tree.push(RenderCommand::FillRect {
        x: params.x,
        y: params.y,
        width: params.width,
        height: LINE_HEIGHT,
        color: bg_color,
        corner_radii: CornerRadii::ZERO,
    });

    // Gutter separator
    tree.push(RenderCommand::Line {
        x1: params.x + GUTTER_WIDTH,
        y1: params.y,
        x2: params.x + GUTTER_WIDTH,
        y2: params.y + LINE_HEIGHT,
        color: colors::SURFACE0,
        width: 1.0,
    });

    // Line number
    if let Some(ln) = params.line_num {
        let ln_text = format!("{}", ln.saturating_add(1));
        tree.push(RenderCommand::Text {
            x: params.x + GUTTER_WIDTH - ln_text.len() as f32 * CHAR_WIDTH - 4.0,
            y: params.y + 3.0,
            text: ln_text,
            color: colors::OVERLAY0,
            font_size: CONTENT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(GUTTER_WIDTH - 4.0),
        });
    }

    // Text
    if let Some(text) = params.text {
        let text_color = match params.op {
            Some(DiffOp::Insert) => colors::GREEN,
            Some(DiffOp::Delete) => colors::RED,
            _ => colors::TEXT,
        };

        tree.push(RenderCommand::Text {
            x: params.x + GUTTER_WIDTH + PANEL_PADDING,
            y: params.y + 3.0,
            text: text.to_string(),
            color: text_color,
            font_size: CONTENT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(params.width - GUTTER_WIDTH - PANEL_PADDING - 12.0),
        });
    }
}

/// Render a single directory comparison entry.
fn render_dir_entry(tree: &mut RenderTree, ey: f32, entry: &DirCompareEntry) {
    let (status_color, status_text) = match entry.status {
        FileCompareStatus::Same => (colors::GREEN, "Same"),
        FileCompareStatus::Different => (colors::YELLOW, "Diff"),
        FileCompareStatus::OnlyLeft => (colors::RED, "Left"),
        FileCompareStatus::OnlyRight => (colors::BLUE, "Right"),
    };

    // Status indicator
    tree.push(RenderCommand::FillRect {
        x: PANEL_PADDING,
        y: ey + 2.0,
        width: 4.0,
        height: LINE_HEIGHT - 4.0,
        color: status_color,
        corner_radii: CornerRadii::all(2.0),
    });

    // Status text
    tree.push(RenderCommand::Text {
        x: 14.0,
        y: ey + 3.0,
        text: status_text.to_string(),
        color: status_color,
        font_size: CONTENT_FONT_SIZE,
        font_weight: FontWeightHint::Bold,
        max_width: Some(50.0),
    });

    // File path
    tree.push(RenderCommand::Text {
        x: 70.0,
        y: ey + 3.0,
        text: entry.path.clone(),
        color: colors::TEXT,
        font_size: CONTENT_FONT_SIZE,
        font_weight: FontWeightHint::Regular,
        max_width: Some(1200.0),
    });
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let mut app = FileDiffApp::new();

    // Demo: load sample files for testing
    let left = "fn main() {\n    println!(\"Hello, world!\");\n    let x = 42;\n}\n";
    let right =
        "fn main() {\n    println!(\"Hello, Slate OS!\");\n    let x = 42;\n    let y = 100;\n}\n";
    app.load_files("left.rs", left, "right.rs", right);

    let _tree = app.render();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Myers diff tests ---

    #[test]
    fn test_diff_empty_both() {
        let result = myers_diff(&[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_diff_empty_left() {
        let right = ["a", "b"];
        let result = myers_diff(&[], &right);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].op, DiffOp::Insert);
        assert_eq!(result[1].op, DiffOp::Insert);
    }

    #[test]
    fn test_diff_empty_right() {
        let left = ["a", "b"];
        let result = myers_diff(&left, &[]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].op, DiffOp::Delete);
        assert_eq!(result[1].op, DiffOp::Delete);
    }

    #[test]
    fn test_diff_identical() {
        let lines = ["hello", "world"];
        let result = myers_diff(&lines, &lines);
        assert_eq!(result.len(), 2);
        for edit in &result {
            assert_eq!(edit.op, DiffOp::Equal);
        }
    }

    #[test]
    fn test_diff_single_insert() {
        let left = ["a", "c"];
        let right = ["a", "b", "c"];
        let result = myers_diff(&left, &right);
        let ops: Vec<DiffOp> = result.iter().map(|e| e.op).collect();
        assert!(ops.contains(&DiffOp::Insert));
        assert!(ops.contains(&DiffOp::Equal));
    }

    #[test]
    fn test_diff_single_delete() {
        let left = ["a", "b", "c"];
        let right = ["a", "c"];
        let result = myers_diff(&left, &right);
        let ops: Vec<DiffOp> = result.iter().map(|e| e.op).collect();
        assert!(ops.contains(&DiffOp::Delete));
        assert!(ops.contains(&DiffOp::Equal));
    }

    #[test]
    fn test_diff_complete_replacement() {
        let left = ["a", "b"];
        let right = ["c", "d"];
        let result = myers_diff(&left, &right);
        let del_count = result.iter().filter(|e| e.op == DiffOp::Delete).count();
        let ins_count = result.iter().filter(|e| e.op == DiffOp::Insert).count();
        assert_eq!(del_count, 2);
        assert_eq!(ins_count, 2);
    }

    #[test]
    fn test_diff_line_numbers_correct() {
        let left = ["a", "b", "c"];
        let right = ["a", "c"];
        let result = myers_diff(&left, &right);
        for edit in &result {
            match edit.op {
                DiffOp::Equal => {
                    assert!(edit.left_line.is_some());
                    assert!(edit.right_line.is_some());
                }
                DiffOp::Delete => {
                    assert!(edit.left_line.is_some());
                    assert!(edit.right_line.is_none());
                }
                DiffOp::Insert => {
                    assert!(edit.left_line.is_none());
                    assert!(edit.right_line.is_some());
                }
            }
        }
    }

    #[test]
    fn test_diff_preserves_text() {
        let left = ["hello", "world"];
        let right = ["hello", "rust"];
        let result = myers_diff(&left, &right);
        let texts: Vec<&str> = result.iter().map(|e| e.text.as_str()).collect();
        assert!(texts.contains(&"hello"));
        assert!(texts.contains(&"world") || texts.contains(&"rust"));
    }

    #[test]
    fn test_diff_preserves_order() {
        let left = ["a", "b", "c", "d"];
        let right = ["a", "x", "c", "d"];
        let result = myers_diff(&left, &right);
        assert_eq!(result[0].op, DiffOp::Equal);
        assert_eq!(result[0].text, "a");
    }

    // --- Compute diff tests ---

    #[test]
    fn test_compute_diff_basic() {
        let left = "hello\nworld\n";
        let right = "hello\nrust\n";
        let opts = IgnoreOptions::default();
        let result = compute_diff(left, right, &opts);
        assert!(!result.edits.is_empty());
        assert_eq!(result.left_line_count, 2);
        assert_eq!(result.right_line_count, 2);
    }

    #[test]
    fn test_compute_diff_ignore_case() {
        let left = "Hello\nWorld";
        let right = "hello\nworld";
        let opts = IgnoreOptions {
            ignore_case: true,
            ..Default::default()
        };
        let result = compute_diff(left, right, &opts);
        let equal_count = result
            .edits
            .iter()
            .filter(|e| e.op == DiffOp::Equal)
            .count();
        assert_eq!(equal_count, 2);
    }

    #[test]
    fn test_compute_diff_ignore_whitespace() {
        let left = "hello   world\n";
        let right = "hello world\n";
        let opts = IgnoreOptions {
            ignore_whitespace: true,
            ..Default::default()
        };
        let result = compute_diff(left, right, &opts);
        let equal_count = result
            .edits
            .iter()
            .filter(|e| e.op == DiffOp::Equal)
            .count();
        assert_eq!(equal_count, 1);
    }

    #[test]
    fn test_compute_diff_ignore_blank_lines() {
        let left = "a\n\nb";
        let right = "a\nb";
        let opts = IgnoreOptions {
            ignore_blank_lines: true,
            ..Default::default()
        };
        let result = compute_diff(left, right, &opts);
        let change_count = result
            .edits
            .iter()
            .filter(|e| e.op != DiffOp::Equal)
            .count();
        assert!(change_count <= 1);
    }

    #[test]
    fn test_compute_diff_empty_both() {
        let opts = IgnoreOptions::default();
        let result = compute_diff("", "", &opts);
        assert!(result.edits.is_empty());
        assert_eq!(result.left_line_count, 0);
        assert_eq!(result.right_line_count, 0);
    }

    // --- Hunk grouping tests ---

    #[test]
    fn test_hunk_grouping_single_change() {
        let left = "a\nb\nc\nd\ne";
        let right = "a\nx\nc\nd\ne";
        let opts = IgnoreOptions::default();
        let result = compute_diff(left, right, &opts);
        assert!(!result.hunks.is_empty());
    }

    #[test]
    fn test_hunk_grouping_multiple_changes() {
        // The two changes (line 2 and line 13) are separated by 10 unchanged
        // lines, which exceeds 2*context (=6) so they form two distinct hunks.
        // (Changes closer than that are correctly merged into one hunk, matching
        // `diff -U3` semantics.)
        let left = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn";
        let right = "a\nB\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nM\nn";
        let opts = IgnoreOptions::default();
        let result = compute_diff(left, right, &opts);
        assert!(result.hunks.len() >= 2);
    }

    #[test]
    fn test_hunk_no_changes() {
        let text = "a\nb\nc";
        let opts = IgnoreOptions::default();
        let result = compute_diff(text, text, &opts);
        assert!(result.hunks.is_empty());
    }

    // --- Inline diff tests ---

    #[test]
    fn test_inline_diff_identical() {
        let (left, right) = inline_diff("hello", "hello");
        for span in &left {
            assert!(!span.changed);
        }
        for span in &right {
            assert!(!span.changed);
        }
    }

    #[test]
    fn test_inline_diff_single_char_change() {
        let (left, right) = inline_diff("abc", "axc");
        assert!(left.iter().any(|s| s.changed));
        assert!(right.iter().any(|s| s.changed));
    }

    #[test]
    fn test_inline_diff_prefix_preserved() {
        let (left, right) = inline_diff("hello world", "hello rust");
        if let Some(first) = left.first() {
            assert!(!first.changed);
            assert!(first.end > 0);
        }
        if let Some(first) = right.first() {
            assert!(!first.changed);
            assert!(first.end > 0);
        }
    }

    #[test]
    fn test_inline_diff_empty_both() {
        let (left, right) = inline_diff("", "");
        assert!(left.is_empty());
        assert!(right.is_empty());
    }

    #[test]
    fn test_inline_diff_one_empty() {
        let (left, right) = inline_diff("hello", "");
        assert!(left.iter().any(|s| s.changed));
        assert!(right.is_empty());
    }

    // --- DiffStats tests ---

    #[test]
    fn test_stats_from_identical_files() {
        let text = "a\nb\nc";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(text, text, &opts);
        let stats = DiffStats::from_diff(&diff);
        assert_eq!(stats.equal_lines, 3);
        assert_eq!(stats.inserted_lines, 0);
        assert_eq!(stats.deleted_lines, 0);
        assert!((stats.similarity - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_stats_completely_different() {
        let left = "a\nb";
        let right = "c\nd";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let stats = DiffStats::from_diff(&diff);
        assert_eq!(stats.inserted_lines, 2);
        assert_eq!(stats.deleted_lines, 2);
        assert!(stats.similarity < 50.0);
    }

    #[test]
    fn test_stats_change_count() {
        let left = "a\nb\nc";
        let right = "a\nx\nc";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let stats = DiffStats::from_diff(&diff);
        assert!(stats.change_count() > 0);
    }

    #[test]
    fn test_stats_empty_diff() {
        let opts = IgnoreOptions::default();
        let diff = compute_diff("", "", &opts);
        let stats = DiffStats::from_diff(&diff);
        assert!((stats.similarity - 100.0).abs() < f32::EPSILON);
    }

    // --- Search tests ---

    #[test]
    fn test_search_finds_matches() {
        let left = "hello world\nfoo bar";
        let right = "hello earth\nfoo baz";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState {
            query: "foo".to_string(),
            ..Default::default()
        };
        search.search(&diff.edits);
        assert!(!search.matches.is_empty());
    }

    #[test]
    fn test_search_case_insensitive() {
        let left = "Hello World";
        let right = "hello world";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState {
            query: "hello".to_string(),
            case_sensitive: false,
            ..Default::default()
        };
        search.search(&diff.edits);
        assert!(!search.matches.is_empty());
    }

    #[test]
    fn test_search_empty_query() {
        let left = "hello";
        let right = "world";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState::default();
        search.search(&diff.edits);
        assert!(search.matches.is_empty());
    }

    #[test]
    fn test_search_navigation() {
        let left = "aaa\naaa\naaa";
        let right = "aaa\naaa\naaa";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState {
            query: "aaa".to_string(),
            case_sensitive: true,
            ..Default::default()
        };
        search.search(&diff.edits);
        let count = search.matches.len();
        assert!(count > 0);
        search.next_match();
        assert_eq!(search.current_match, 1);
        search.prev_match();
        assert_eq!(search.current_match, 0);
    }

    #[test]
    fn test_search_wraps_around() {
        let left = "abc\ndef";
        let right = "abc\ndef";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState {
            query: "abc".to_string(),
            case_sensitive: true,
            ..Default::default()
        };
        search.search(&diff.edits);
        let count = search.matches.len();
        assert!(count > 0);
        for _ in 0..=count {
            search.next_match();
        }
        assert!(search.current_match < count);
    }

    // --- Merge tests ---

    #[test]
    fn test_merge_state_new() {
        let merge = MergeState::new(3);
        assert_eq!(merge.decisions.len(), 3);
        for d in &merge.decisions {
            assert_eq!(*d, MergeDecision::Undecided);
        }
    }

    #[test]
    fn test_merge_set_get_decision() {
        let mut merge = MergeState::new(3);
        merge.set_decision(1, MergeDecision::AcceptLeft);
        assert_eq!(merge.get_decision(1), MergeDecision::AcceptLeft);
        assert_eq!(merge.get_decision(0), MergeDecision::Undecided);
    }

    #[test]
    fn test_merge_decided_count() {
        let mut merge = MergeState::new(4);
        assert_eq!(merge.decided_count(), 0);
        merge.set_decision(0, MergeDecision::AcceptLeft);
        merge.set_decision(2, MergeDecision::AcceptRight);
        assert_eq!(merge.decided_count(), 2);
    }

    #[test]
    fn test_merge_out_of_bounds() {
        let mut merge = MergeState::new(2);
        merge.set_decision(99, MergeDecision::AcceptLeft);
        assert_eq!(merge.get_decision(99), MergeDecision::Undecided);
    }

    #[test]
    fn test_merge_apply_accept_right() {
        let left = "a\nb\nc";
        let right = "a\nX\nc";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut merge = MergeState::new(diff.hunks.len());
        for i in 0..diff.hunks.len() {
            merge.set_decision(i, MergeDecision::AcceptRight);
        }
        let output = merge.apply(&diff);
        assert!(output.contains('X'));
    }

    #[test]
    fn test_merge_apply_accept_left() {
        let left = "a\nb\nc";
        let right = "a\nX\nc";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut merge = MergeState::new(diff.hunks.len());
        for i in 0..diff.hunks.len() {
            merge.set_decision(i, MergeDecision::AcceptLeft);
        }
        let output = merge.apply(&diff);
        assert!(output.contains('b'));
    }

    // --- Directory comparison tests ---

    #[test]
    fn test_dir_compare_identical() {
        let left = [("file.txt", "hello")];
        let right = [("file.txt", "hello")];
        let result = compare_directories(&left, &right);
        assert_eq!(result.same_count, 1);
        assert_eq!(result.different_count, 0);
    }

    #[test]
    fn test_dir_compare_different() {
        let left = [("file.txt", "hello")];
        let right = [("file.txt", "world")];
        let result = compare_directories(&left, &right);
        assert_eq!(result.different_count, 1);
    }

    #[test]
    fn test_dir_compare_only_left() {
        let left = [("a.txt", "x"), ("b.txt", "y")];
        let right = [("a.txt", "x")];
        let result = compare_directories(&left, &right);
        assert_eq!(result.only_left_count, 1);
        assert_eq!(result.same_count, 1);
    }

    #[test]
    fn test_dir_compare_only_right() {
        let left = [("a.txt", "x")];
        let right = [("a.txt", "x"), ("c.txt", "z")];
        let result = compare_directories(&left, &right);
        assert_eq!(result.only_right_count, 1);
    }

    #[test]
    fn test_dir_compare_empty() {
        let result = compare_directories(&[], &[]);
        assert_eq!(result.entries.len(), 0);
    }

    #[test]
    fn test_dir_compare_mixed() {
        let left = [("a.txt", "same"), ("b.txt", "old"), ("d.txt", "only_l")];
        let right = [("a.txt", "same"), ("b.txt", "new"), ("c.txt", "only_r")];
        let result = compare_directories(&left, &right);
        assert_eq!(result.same_count, 1);
        assert_eq!(result.different_count, 1);
        assert_eq!(result.only_left_count, 1);
        assert_eq!(result.only_right_count, 1);
    }

    // --- View mode tests ---

    #[test]
    fn test_view_mode_display() {
        assert_eq!(format!("{}", ViewMode::SideBySide), "Side-by-Side");
        assert_eq!(format!("{}", ViewMode::Unified), "Unified");
        assert_eq!(format!("{}", ViewMode::Inline), "Inline");
    }

    // --- App state tests ---

    #[test]
    fn test_app_new_defaults() {
        let app = FileDiffApp::new();
        assert!(app.diff.is_none());
        assert!(app.sync_scroll);
        assert_eq!(app.view_mode, ViewMode::SideBySide);
        assert!(app.scroll_left.abs() < f32::EPSILON);
    }

    #[test]
    fn test_app_load_files() {
        let mut app = FileDiffApp::new();
        app.load_files("left.txt", "hello\nworld", "right.txt", "hello\nrust");
        assert!(app.diff.is_some());
        assert_eq!(app.left_path, "left.txt");
        assert_eq!(app.right_path, "right.txt");
    }

    #[test]
    fn test_app_change_navigation() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "a\nb\nc", "b", "a\nX\nc");
        assert!(!app.change_indices.is_empty());
        let initial = app.current_change_index;
        app.next_change();
        assert!(app.current_change_index != initial || app.change_indices.len() <= 1);
    }

    #[test]
    fn test_app_prev_change_wraps() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "a\nb", "b", "a\nX");
        assert!(!app.change_indices.is_empty());
        app.prev_change();
        assert_eq!(
            app.current_change_index,
            app.change_indices.len().saturating_sub(1)
        );
    }

    #[test]
    fn test_app_toggle_ignore_options() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "Hello", "b", "hello");
        assert!(!app.ignore_opts.ignore_case);
        app.toggle_ignore_case();
        assert!(app.ignore_opts.ignore_case);
    }

    #[test]
    fn test_app_merge_operations() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "a\nb\nc", "b", "a\nX\nc");
        app.accept_left();
        if let Some(ref merge) = app.merge {
            assert_eq!(merge.get_decision(0), MergeDecision::AcceptLeft);
        }
    }

    // --- Render tests ---

    #[test]
    fn test_render_empty_state() {
        let app = FileDiffApp::new();
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_diff() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "hello\nworld", "b", "hello\nrust");
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_unified_view() {
        let mut app = FileDiffApp::new();
        app.view_mode = ViewMode::Unified;
        app.load_files("a", "hello\nworld", "b", "hello\nrust");
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_inline_view() {
        let mut app = FileDiffApp::new();
        app.view_mode = ViewMode::Inline;
        app.load_files("a", "hello world", "b", "hello rust");
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    // --- DiffOp display tests ---

    #[test]
    fn test_diffop_display() {
        assert_eq!(format!("{}", DiffOp::Equal), " ");
        assert_eq!(format!("{}", DiffOp::Insert), "+");
        assert_eq!(format!("{}", DiffOp::Delete), "-");
    }

    // --- Side-by-side pairing tests ---

    #[test]
    fn test_side_by_side_pairs_equal() {
        let edits = vec![DiffEdit {
            op: DiffOp::Equal,
            left_line: Some(0),
            right_line: Some(0),
            text: "hello".to_string(),
        }];
        let pairs = build_side_by_side_pairs(&edits);
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].left_text.is_some());
        assert!(pairs[0].right_text.is_some());
    }

    #[test]
    fn test_side_by_side_pairs_delete_insert_paired() {
        let edits = vec![
            DiffEdit {
                op: DiffOp::Delete,
                left_line: Some(0),
                right_line: None,
                text: "old".to_string(),
            },
            DiffEdit {
                op: DiffOp::Insert,
                left_line: None,
                right_line: Some(0),
                text: "new".to_string(),
            },
        ];
        let pairs = build_side_by_side_pairs(&edits);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].left_text.as_deref(), Some("old"));
        assert_eq!(pairs[0].right_text.as_deref(), Some("new"));
    }

    #[test]
    fn test_side_by_side_pairs_standalone_delete() {
        let edits = vec![DiffEdit {
            op: DiffOp::Delete,
            left_line: Some(0),
            right_line: None,
            text: "removed".to_string(),
        }];
        let pairs = build_side_by_side_pairs(&edits);
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].left_text.is_some());
        assert!(pairs[0].right_text.is_none());
    }

    // --- IgnoreOptions tests ---

    #[test]
    fn test_ignore_options_has_any() {
        let default_opts = IgnoreOptions::default();
        assert!(!default_opts.has_any());

        let ws_opts = IgnoreOptions {
            ignore_whitespace: true,
            ..Default::default()
        };
        assert!(ws_opts.has_any());
    }

    #[test]
    fn test_normalize_line_case() {
        let opts = IgnoreOptions {
            ignore_case: true,
            ..Default::default()
        };
        assert_eq!(normalize_line("Hello WORLD", opts), "hello world");
    }

    #[test]
    fn test_normalize_line_whitespace() {
        let opts = IgnoreOptions {
            ignore_whitespace: true,
            ..Default::default()
        };
        assert_eq!(normalize_line("  hello   world  ", opts), "hello world");
    }

    // --- FileCompareStatus tests ---

    #[test]
    fn test_file_compare_status_display() {
        assert_eq!(format!("{}", FileCompareStatus::Same), "Same");
        assert_eq!(format!("{}", FileCompareStatus::Different), "Different");
        assert_eq!(format!("{}", FileCompareStatus::OnlyLeft), "Only in left");
        assert_eq!(format!("{}", FileCompareStatus::OnlyRight), "Only in right");
    }

    // --- Event handling tests ---

    #[test]
    fn test_handle_resize() {
        let mut app = FileDiffApp::new();
        let result = app.handle_event(&Event::Resize {
            width: 1920,
            height: 1080,
        });
        assert_eq!(result, EventResult::Consumed);
        assert!((app.width - 1920.0).abs() < f32::EPSILON);
        assert!((app.height - 1080.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_handle_scroll_down() {
        let mut app = FileDiffApp::new();
        // Use more lines than fit in the viewport so there is room to scroll
        // (an 8-line file fits entirely on screen and would never scroll).
        let left: String = (0..60)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut right_lines: Vec<String> = (0..60).map(|i| format!("line{i}")).collect();
        right_lines[59] = "changed".to_string();
        let right = right_lines.join("\n");
        app.load_files("a", &left, "b", &right);
        let key = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let result = app.handle_event(&Event::Key(key));
        assert_eq!(result, EventResult::Consumed);
        assert!(app.scroll_left > 0.0);
    }

    #[test]
    fn test_handle_view_mode_switch() {
        let mut app = FileDiffApp::new();
        let key = KeyEvent {
            key: Key::Num2,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        };
        let result = app.handle_event(&Event::Key(key));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(app.view_mode, ViewMode::Unified);
    }

    // --- Merged text tests ---

    #[test]
    fn test_merged_text_none_without_diff() {
        let app = FileDiffApp::new();
        assert!(app.merged_text().is_none());
    }

    #[test]
    fn test_merged_text_with_diff() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "hello", "b", "world");
        assert!(app.merged_text().is_some());
    }

    // --- Default trait test ---

    #[test]
    fn test_default_app() {
        let app = FileDiffApp::default();
        assert!(app.diff.is_none());
        assert_eq!(app.view_mode, ViewMode::SideBySide);
    }

    // --- Additional search current tests ---

    #[test]
    fn test_search_current_empty() {
        let search = SearchState::default();
        assert!(search.current().is_none());
    }

    #[test]
    fn test_search_current_with_results() {
        let left = "hello";
        let right = "hello";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState {
            query: "hello".to_string(),
            case_sensitive: true,
            ..Default::default()
        };
        search.search(&diff.edits);
        assert!(search.current().is_some());
    }
}

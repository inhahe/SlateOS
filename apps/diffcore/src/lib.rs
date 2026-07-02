//! `SlateOS` shared diff/merge engine.
//!
//! This crate is the single source of truth for line- and character-level
//! text diffing and merging across the OS's desktop apps. It provides:
//!
//! - [`myers_diff`] — the classic Myers optimal edit-script algorithm over
//!   two slices of lines.
//! - [`compute_diff`] — a higher-level line diff producing a [`DiffResult`]
//!   with grouped [`DiffHunk`]s and honoring [`IgnoreOptions`]
//!   (whitespace/case/blank-line normalization).
//! - [`inline_diff`] — a character-level prefix/suffix diff for highlighting
//!   the changed span within a modified line.
//! - [`MergeState`] / [`MergeDecision`] — the File Diff tool's per-hunk
//!   accept-left/right/both 2-way merge model.
//! - [`three_way_merge`] — a proper diff3 three-way merge (base vs. ours vs.
//!   theirs). This is what the text editors use when a file is changed on
//!   disk while the user has unsaved edits: the last-loaded content is the
//!   common base, the editor buffer is "ours", and the new disk content is
//!   "theirs", so non-conflicting changes from both sides are combined
//!   automatically and genuine conflicts are reported for review.
//!
//! The engine is UI-agnostic (no `guitk` dependency) so it can be unit-tested
//! on the host and reused by any front end.

#![deny(clippy::all, clippy::pedantic)]
#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::unreadable_literal,
    clippy::module_name_repetitions
)]

use std::collections::VecDeque;
use std::fmt;

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
///
/// Returns `(left_spans, right_spans)`, each a list of [`InlineEdit`] byte
/// ranges tagged with whether they changed. Uses a common-prefix/suffix
/// heuristic (fast and good enough for single-line highlighting).
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
// Statistics
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
    /// Similarity percentage (0-100).
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
// Per-hunk 2-way merge (File Diff tool model)
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
    ///
    /// Note: this walks only the diff's hunks, so it is intended for the File
    /// Diff tool's hunk-oriented review UI (which renders full context), not
    /// for reconstructing a whole file from scattered hunks.
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
// diff3 three-way merge (text editor model)
// ============================================================================

/// One region of a three-way merge, in output order.
///
/// A merge is a sequence of these chunks; concatenating each chunk's chosen
/// lines reproduces the merged file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeChunk {
    /// Content identical across base, ours and theirs — carried through verbatim.
    Stable(Vec<String>),
    /// Only *our* side changed this base region; take our version.
    OursOnly(Vec<String>),
    /// Only *their* side changed this base region; take their version.
    TheirsOnly(Vec<String>),
    /// Both sides changed the region to the *same* new content; take it once.
    BothSame(Vec<String>),
    /// Both sides changed the same base region differently — a real conflict.
    Conflict {
        /// The original base lines.
        base: Vec<String>,
        /// Our version of the region.
        ours: Vec<String>,
        /// Their version of the region.
        theirs: Vec<String>,
    },
}

/// How to resolve a single [`MergeChunk::Conflict`] when materializing text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    /// Keep our version.
    Ours,
    /// Keep their version.
    Theirs,
    /// Keep our version followed by their version.
    Both,
    /// Keep the original base version.
    Base,
}

/// The result of a diff3 three-way merge.
#[derive(Clone, Debug)]
pub struct ThreeWayMerge {
    /// The merged regions, in output order.
    pub chunks: Vec<MergeChunk>,
}

impl ThreeWayMerge {
    /// Number of conflicting regions.
    #[must_use]
    pub fn conflict_count(&self) -> usize {
        self.chunks
            .iter()
            .filter(|c| matches!(c, MergeChunk::Conflict { .. }))
            .count()
    }

    /// Whether the merge has any conflicts.
    #[must_use]
    pub fn has_conflicts(&self) -> bool {
        self.chunks
            .iter()
            .any(|c| matches!(c, MergeChunk::Conflict { .. }))
    }

    /// The merged text when there are no conflicts.
    ///
    /// Returns `None` if any region conflicts (so the caller knows a manual
    /// review is required). When `Some`, every non-conflicting change from
    /// both sides has been combined.
    #[must_use]
    pub fn clean_merge(&self) -> Option<String> {
        if self.has_conflicts() {
            return None;
        }
        Some(self.materialize(&[]))
    }

    /// Materialize the merged text, resolving conflicts in output order using
    /// `choices` (one per conflict; missing/extra entries default to
    /// [`ConflictChoice::Theirs`], matching "reload from disk" for anything the
    /// user did not explicitly resolve).
    #[must_use]
    pub fn resolve(&self, choices: &[ConflictChoice]) -> String {
        self.materialize(choices)
    }

    fn materialize(&self, choices: &[ConflictChoice]) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut conflict_idx = 0usize;
        for chunk in &self.chunks {
            match chunk {
                MergeChunk::Stable(lines)
                | MergeChunk::OursOnly(lines)
                | MergeChunk::TheirsOnly(lines)
                | MergeChunk::BothSame(lines) => out.extend(lines.iter().cloned()),
                MergeChunk::Conflict {
                    base,
                    ours,
                    theirs,
                } => {
                    let choice = choices
                        .get(conflict_idx)
                        .copied()
                        .unwrap_or(ConflictChoice::Theirs);
                    conflict_idx = conflict_idx.saturating_add(1);
                    match choice {
                        ConflictChoice::Ours => out.extend(ours.iter().cloned()),
                        ConflictChoice::Theirs => out.extend(theirs.iter().cloned()),
                        ConflictChoice::Base => out.extend(base.iter().cloned()),
                        ConflictChoice::Both => {
                            out.extend(ours.iter().cloned());
                            out.extend(theirs.iter().cloned());
                        }
                    }
                }
            }
        }
        join_lines(&out)
    }

    /// Render the merged text with Git-style conflict markers around every
    /// unresolved conflict (`<<<<<<< ours` / `=======` / `>>>>>>> theirs`).
    ///
    /// Useful for dropping a conflicted merge straight into the editor buffer
    /// for manual resolution.
    #[must_use]
    pub fn text_with_markers(&self, ours_label: &str, theirs_label: &str) -> String {
        let mut out: Vec<String> = Vec::new();
        for chunk in &self.chunks {
            match chunk {
                MergeChunk::Stable(lines)
                | MergeChunk::OursOnly(lines)
                | MergeChunk::TheirsOnly(lines)
                | MergeChunk::BothSame(lines) => out.extend(lines.iter().cloned()),
                MergeChunk::Conflict { ours, theirs, .. } => {
                    out.push(format!("<<<<<<< {ours_label}"));
                    out.extend(ours.iter().cloned());
                    out.push("=======".to_string());
                    out.extend(theirs.iter().cloned());
                    out.push(format!(">>>>>>> {theirs_label}"));
                }
            }
        }
        join_lines(&out)
    }
}

/// Join lines back into a single string.
///
/// Emits `\n` between lines and no trailing newline, which round-trips cleanly
/// with [`split_lines`] (the inverse used by [`three_way_merge`]).
fn join_lines(lines: &[String]) -> String {
    lines.join("\n")
}

/// Split text into lines for three-way merging.
///
/// Unlike [`str::lines`], this splits on `'\n'` without dropping a trailing
/// empty element, so it round-trips exactly with [`join_lines`]: `"a\n"`
/// becomes `["a", ""]`, which re-joins to `"a\n"`. Empty input yields no
/// lines (round-tripping to `""`).
fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n').map(str::to_string).collect()
}

/// Perform a diff3 three-way merge of `base` → `ours` / `theirs`.
///
/// `base` is the common ancestor (for the editor: the file content as last
/// loaded/saved). `ours` is the local version (the editor buffer) and `theirs`
/// is the incoming version (the current on-disk content). Changes made on only
/// one side relative to `base` are taken automatically; regions changed on both
/// sides become [`MergeChunk::Conflict`]s (unless both sides made the identical
/// change, which is a [`MergeChunk::BothSame`]).
///
/// The algorithm anchors on lines that are unchanged (Equal) across *all three*
/// versions, using the two independent line diffs `base↔ours` and
/// `base↔theirs`, then classifies each inter-anchor segment.
// The isize casts below are bounded by line counts, which never approach
// isize::MAX; -1 sentinels frame the head segment, so signed indices are used.
#[allow(clippy::cast_possible_wrap)]
#[must_use]
pub fn three_way_merge(base: &str, ours: &str, theirs: &str) -> ThreeWayMerge {
    let base_lines = split_lines(base);
    let ours_lines = split_lines(ours);
    let theirs_lines = split_lines(theirs);

    let base_refs: Vec<&str> = base_lines.iter().map(String::as_str).collect();
    let ours_refs: Vec<&str> = ours_lines.iter().map(String::as_str).collect();
    let theirs_refs: Vec<&str> = theirs_lines.iter().map(String::as_str).collect();

    // Map each base line index to the matching "other" index when that base
    // line is preserved unchanged (Equal) in the other version.
    let a_eq = equal_map(&myers_diff(&base_refs, &ours_refs));
    let b_eq = equal_map(&myers_diff(&base_refs, &theirs_refs));

    // Anchors: base indices whose line is preserved in BOTH ours and theirs.
    // These are the only points where all three versions provably agree, so
    // they bound the segments we classify independently.
    let mut anchors: Vec<usize> = a_eq
        .keys()
        .filter(|bi| b_eq.contains_key(*bi))
        .copied()
        .collect();
    anchors.sort_unstable();

    let mut chunks: Vec<MergeChunk> = Vec::new();

    // Sentinels frame the head (before first anchor) and tail (after last).
    let mut prev_base: isize = -1;
    let mut prev_ours: isize = -1;
    let mut prev_theirs: isize = -1;

    // Walk anchors, emitting the segment strictly between the previous anchor
    // and the current one, then the anchor line itself as Stable.
    let anchor_iter = anchors
        .iter()
        .map(|&bi| {
            let oi = *a_eq.get(&bi).unwrap_or(&0);
            let ti = *b_eq.get(&bi).unwrap_or(&0);
            (bi as isize, oi as isize, ti as isize)
        })
        .chain(std::iter::once((
            base_lines.len() as isize,
            ours_lines.len() as isize,
            theirs_lines.len() as isize,
        )));

    for (cur_base, cur_ours, cur_theirs) in anchor_iter {
        let base_seg = slice_lines(&base_lines, prev_base, cur_base);
        let ours_seg = slice_lines(&ours_lines, prev_ours, cur_ours);
        let theirs_seg = slice_lines(&theirs_lines, prev_theirs, cur_theirs);

        classify_segment(&mut chunks, base_seg, ours_seg, theirs_seg);

        // Emit the anchor line itself. `get` returns None for the tail
        // sentinel (cur_base == len), so no explicit bounds check is needed.
        if let Some(line) = base_lines.get(cur_base as usize) {
            match chunks.last_mut() {
                Some(MergeChunk::Stable(lines)) => lines.push(line.clone()),
                _ => chunks.push(MergeChunk::Stable(vec![line.clone()])),
            }
        }

        prev_base = cur_base;
        prev_ours = cur_ours;
        prev_theirs = cur_theirs;
    }

    ThreeWayMerge { chunks }
}

/// Classify one inter-anchor segment and push the appropriate chunk(s).
fn classify_segment(
    chunks: &mut Vec<MergeChunk>,
    base_seg: Vec<String>,
    ours_seg: Vec<String>,
    theirs_seg: Vec<String>,
) {
    let ours_changed = ours_seg != base_seg;
    let theirs_changed = theirs_seg != base_seg;

    let chunk = if !ours_changed && !theirs_changed {
        if base_seg.is_empty() {
            return;
        }
        MergeChunk::Stable(base_seg)
    } else if ours_changed && !theirs_changed {
        if ours_seg.is_empty() {
            return;
        }
        MergeChunk::OursOnly(ours_seg)
    } else if !ours_changed && theirs_changed {
        if theirs_seg.is_empty() {
            return;
        }
        MergeChunk::TheirsOnly(theirs_seg)
    } else if ours_seg == theirs_seg {
        if ours_seg.is_empty() {
            return;
        }
        MergeChunk::BothSame(ours_seg)
    } else {
        MergeChunk::Conflict {
            base: base_seg,
            ours: ours_seg,
            theirs: theirs_seg,
        }
    };

    // Coalesce adjacent Stable chunks so anchors merge with unchanged runs.
    if let (MergeChunk::Stable(new_lines), Some(MergeChunk::Stable(prev))) =
        (&chunk, chunks.last_mut())
    {
        prev.extend(new_lines.iter().cloned());
        return;
    }
    chunks.push(chunk);
}

/// Extract `lines[(from+1)..to]` (the open interval strictly between two
/// anchor indices), clamped to the slice bounds. `from` may be `-1` (head).
fn slice_lines(lines: &[String], from: isize, to: isize) -> Vec<String> {
    let start = (from + 1).max(0) as usize;
    let end = to.max(0) as usize;
    if start >= end {
        return Vec::new();
    }
    lines.get(start..end.min(lines.len())).unwrap_or(&[]).to_vec()
}

/// Build a map from base line index → other-side line index for every Equal
/// edit in a `base↔other` diff (base is the left side of the diff).
fn equal_map(edits: &[DiffEdit]) -> std::collections::BTreeMap<usize, usize> {
    let mut map = std::collections::BTreeMap::new();
    for e in edits {
        if e.op == DiffOp::Equal
            && let (Some(b), Some(o)) = (e.left_line, e.right_line)
        {
            map.insert(b, o);
        }
    }
    map
}

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
    fn test_diff_preserves_text() {
        let left = ["alpha", "beta"];
        let right = ["alpha", "gamma"];
        let result = myers_diff(&left, &right);
        let texts: Vec<&str> = result.iter().map(|e| e.text.as_str()).collect();
        assert!(texts.contains(&"alpha"));
        assert!(texts.contains(&"beta"));
        assert!(texts.contains(&"gamma"));
    }

    // --- compute_diff / ignore options ---

    #[test]
    fn test_compute_diff_basic() {
        let d = compute_diff("a\nb\nc", "a\nx\nc", &IgnoreOptions::default());
        assert_eq!(d.left_line_count, 3);
        assert_eq!(d.right_line_count, 3);
        assert!(!d.hunks.is_empty());
    }

    #[test]
    fn test_compute_diff_ignore_case() {
        let opts = IgnoreOptions {
            ignore_case: true,
            ..IgnoreOptions::default()
        };
        let d = compute_diff("Hello", "hello", &opts);
        assert!(d.edits.iter().all(|e| e.op == DiffOp::Equal));
    }

    #[test]
    fn test_compute_diff_ignore_whitespace() {
        let opts = IgnoreOptions {
            ignore_whitespace: true,
            ..IgnoreOptions::default()
        };
        let d = compute_diff("a  b", "a b", &opts);
        assert!(d.edits.iter().all(|e| e.op == DiffOp::Equal));
    }

    #[test]
    fn test_hunk_no_changes() {
        let d = compute_diff("a\nb", "a\nb", &IgnoreOptions::default());
        assert!(d.hunks.is_empty());
    }

    // --- inline diff ---

    #[test]
    fn test_inline_diff_identical() {
        let (l, r) = inline_diff("same", "same");
        assert!(l.iter().all(|s| !s.changed));
        assert!(r.iter().all(|s| !s.changed));
    }

    #[test]
    fn test_inline_diff_prefix_preserved() {
        let (l, _r) = inline_diff("hello world", "hello there");
        // The "hello " prefix span should be unchanged.
        assert!(l.iter().any(|s| !s.changed));
        assert!(l.iter().any(|s| s.changed));
    }

    // --- stats ---

    #[test]
    fn test_stats_from_identical_files() {
        let d = compute_diff("a\nb", "a\nb", &IgnoreOptions::default());
        let s = DiffStats::from_diff(&d);
        assert_eq!(s.inserted_lines, 0);
        assert_eq!(s.deleted_lines, 0);
        assert!((s.similarity - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_stats_change_count() {
        let d = compute_diff("a\nb\nc", "a\nx\nc", &IgnoreOptions::default());
        let s = DiffStats::from_diff(&d);
        assert!(s.change_count() >= 1);
    }

    // --- 2-way MergeState ---

    #[test]
    fn test_merge_state_new() {
        let m = MergeState::new(3);
        assert_eq!(m.decisions.len(), 3);
        assert_eq!(m.decided_count(), 0);
    }

    #[test]
    fn test_merge_set_get_decision() {
        let mut m = MergeState::new(2);
        m.set_decision(0, MergeDecision::AcceptLeft);
        assert_eq!(m.get_decision(0), MergeDecision::AcceptLeft);
        assert_eq!(m.decided_count(), 1);
    }

    #[test]
    fn test_merge_out_of_bounds() {
        let mut m = MergeState::new(1);
        m.set_decision(5, MergeDecision::AcceptBoth); // no panic
        assert_eq!(m.get_decision(5), MergeDecision::Undecided);
    }

    // --- diff3 three-way merge ---

    #[test]
    fn test_three_way_no_changes() {
        let m = three_way_merge("a\nb\nc", "a\nb\nc", "a\nb\nc");
        assert!(!m.has_conflicts());
        assert_eq!(m.clean_merge().as_deref(), Some("a\nb\nc"));
    }

    #[test]
    fn test_three_way_ours_only() {
        // We changed line 2; disk unchanged.
        let m = three_way_merge("a\nb\nc", "a\nB\nc", "a\nb\nc");
        assert!(!m.has_conflicts());
        assert_eq!(m.clean_merge().as_deref(), Some("a\nB\nc"));
    }

    #[test]
    fn test_three_way_theirs_only() {
        // Disk changed line 2; we unchanged.
        let m = three_way_merge("a\nb\nc", "a\nb\nc", "a\nZ\nc");
        assert!(!m.has_conflicts());
        assert_eq!(m.clean_merge().as_deref(), Some("a\nZ\nc"));
    }

    #[test]
    fn test_three_way_disjoint_changes_merge() {
        // We changed the first line; disk changed the last line. Both apply.
        let m = three_way_merge("a\nb\nc", "A\nb\nc", "a\nb\nC");
        assert!(!m.has_conflicts(), "disjoint edits must not conflict");
        assert_eq!(m.clean_merge().as_deref(), Some("A\nb\nC"));
    }

    #[test]
    fn test_three_way_same_change_both_sides() {
        // Both sides made the identical edit — not a conflict.
        let m = three_way_merge("a\nb\nc", "a\nX\nc", "a\nX\nc");
        assert!(!m.has_conflicts());
        assert_eq!(m.clean_merge().as_deref(), Some("a\nX\nc"));
    }

    #[test]
    fn test_three_way_true_conflict() {
        // Both sides changed line 2 differently.
        let m = three_way_merge("a\nb\nc", "a\nOURS\nc", "a\nTHEIRS\nc");
        assert!(m.has_conflicts());
        assert_eq!(m.conflict_count(), 1);
        assert!(m.clean_merge().is_none());
        // Default resolution keeps theirs (disk).
        assert_eq!(m.resolve(&[]), "a\nTHEIRS\nc");
        assert_eq!(m.resolve(&[ConflictChoice::Ours]), "a\nOURS\nc");
        assert_eq!(m.resolve(&[ConflictChoice::Base]), "a\nb\nc");
        assert_eq!(m.resolve(&[ConflictChoice::Both]), "a\nOURS\nTHEIRS\nc");
    }

    #[test]
    fn test_three_way_conflict_markers() {
        let m = three_way_merge("a\nb\nc", "a\nOURS\nc", "a\nTHEIRS\nc");
        let marked = m.text_with_markers("buffer", "disk");
        assert!(marked.contains("<<<<<<< buffer"));
        assert!(marked.contains("OURS"));
        assert!(marked.contains("======="));
        assert!(marked.contains("THEIRS"));
        assert!(marked.contains(">>>>>>> disk"));
    }

    #[test]
    fn test_three_way_both_insert_disjoint() {
        // We insert after line 1; disk inserts after line 2. No overlap.
        let m = three_way_merge("a\nb", "a\nNEW\nb", "a\nb\nTAIL");
        assert!(!m.has_conflicts());
        assert_eq!(m.clean_merge().as_deref(), Some("a\nNEW\nb\nTAIL"));
    }

    #[test]
    fn test_three_way_empty_base() {
        // New file created on disk; buffer also has content -> conflict.
        let m = three_way_merge("", "mine", "theirs");
        assert!(m.has_conflicts());
    }

    #[test]
    fn test_split_join_roundtrip() {
        for s in ["", "a", "a\nb", "a\nb\n", "\n", "a\n\nb"] {
            let lines = split_lines(s);
            assert_eq!(join_lines(&lines), s, "roundtrip failed for {s:?}");
        }
    }
}

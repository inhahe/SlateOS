//! `Slate OS` terminal multiplexer (tmux)
//!
//! One window of many terminals: panes split side by side or one above the
//! other, windows of panes shown as tabs, and sessions of windows that can be
//! detached and attached again. Driven the way tmux is -- Ctrl+B, then a key
//! (F1 lists them) -- and by the pointer: the tabs, the panes, the status
//! bar's session and window names, the choosers and the detached screen all
//! answer a click, and the wheel scrolls the pane under it.
//!
//! **Every pane is a terminal with a shell in it.** A pane is an
//! `apps/terminal` [`TerminalState`] -- the same emulator the terminal app
//! runs, with its scroll regions, alternate screen, cursor-key modes and
//! replies -- attached to the user's shell on a kernel pseudo-terminal through
//! [`terminal::child::spawn_shell`]. Keys go to the active pane's shell; its
//! output is read on the tick; a pane's size reaches its shell as `TIOCSWINSZ`
//! whenever the layout changes; a shell that exits takes its pane with it, as
//! in tmux, unless it failed, when the pane stays to say how. Where there are
//! no pseudo-terminals, each pane says so.
//!
//! Until 2026-09-25 no pane had anything behind it. The multiplexer -- splits,
//! windows, sessions, the prompt -- was real, and every pane held a banner
//! saying the system had no PTY layer, long after `apps/terminal` had one.
//! Typing went nowhere; the pane's own ANSI parser could not have drawn a
//! full-screen program; and nothing answered the pointer. See
//! `known-issues.md` ->
//! `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`.
//!
//! What it is not is a server. Sessions last as long as this window does:
//! detaching hides a session and leaves its shells running, and closing the
//! window ends every one of them -- which the detached screen says.

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::cognitive_complexity)]

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use libcall::pty::WinSize;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use terminal::child::{Link, SpawnError};
use terminal::{Selection, Target as TermTarget, TerminalConfig, TerminalState};

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1200.0;
const WINDOW_HEIGHT: f32 = 800.0;
const STATUS_BAR_HEIGHT: f32 = 22.0;
const TAB_BAR_HEIGHT: f32 = 28.0;
const PANE_BORDER_WIDTH: f32 = 1.0;
/// The strip along a pane's top edge that carries its name.
///
/// Inside the pane rather than on its border, where the name used to be drawn
/// half over the tab bar: a split window's panes are told apart by these, and
/// a label cut in two by the chrome above it is not much of one.
const PANE_TITLE_HEIGHT: f32 = 17.0;
/// Between a pane's border and its terminal, on the other three sides.
const PANE_INSET: f32 = 2.0;

const PADDING: f32 = 4.0;
const SMALL_TEXT: f32 = 11.0;
const NORMAL_TEXT: f32 = 13.0;
const HEADER_TEXT: f32 = 14.0;
const MIN_PANE_SIZE: f32 = 40.0;
const RESIZE_STEP: f32 = 20.0;
/// How much of its split `prefix +` gives a pane at a time.
const GROW_STEP: f32 = 0.05;

/// How small a share of a split one side may be squeezed to.
///
/// A ratio of zero is a pane with no pixels, which `compute_bounds` then floors
/// at `MIN_PANE_SIZE` -- so the divider stops moving and cannot be brought
/// back, because the ratio it is stored as has already gone past the end.
const MIN_SPLIT_RATIO: f32 = 0.1;

const MAX_SESSIONS: usize = 64;
const MAX_WINDOWS: usize = 32;
const MAX_PANES: usize = 32;

/// How long a message stays on the status bar before the clock returns.
const STATUS_MESSAGE_MS: u64 = 5_000;

/// The choosers: a box this wide, rows this tall, the first row this far down.
const CHOOSER_WIDTH: f32 = 320.0;
const CHOOSER_ROW: f32 = 24.0;
const CHOOSER_TOP: f32 = 36.0;
const CHOOSER_MAX_HEIGHT: f32 = 400.0;

/// The gap between two tabs.
const TAB_GAP: f32 = 2.0;

// ============================================================================
// Panes
// ============================================================================

/// How a split divides a pane.
///
/// Named for what the user sees. These were `Horizontal` and `Vertical`, and
/// both readings of those words -- the direction of the divider, and the
/// arrangement of the panes -- had been used in different places, so
/// `:split-window -h` and the `even-horizontal` layout each did the opposite of
/// tmux's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitDir {
    /// One pane above the other: tmux's `split-window -v`, `prefix "`.
    Stacked,
    /// Side by side: tmux's `split-window -h`, `prefix %`.
    SideBySide,
}

/// A pane's identity, for as long as the multiplexer runs.
///
/// Never reused: a closed pane's number is not given to the next one, so a
/// click or a queued exit aimed at a pane that has gone cannot land on a new
/// pane that happens to have its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PaneId(usize);

/// One pane: a terminal, and what the multiplexer knows about it.
struct Pane {
    id: PaneId,
    /// The terminal -- emulator, scrollback, and the link to its shell.
    term: TerminalState,
    /// What was started in it: its name until the program running there
    /// names itself.
    command: String,
    /// Copy mode, tmux's `prefix [`: the keys move through the scrollback
    /// instead of going to the program.
    copy_mode: bool,
    /// Whether `v` has started a selection that the copy-mode keys extend.
    marking: bool,
}

impl Pane {
    fn new(id: PaneId, palette: &Palette) -> Self {
        let mut term = TerminalState::new(TerminalConfig::default());
        term.theme_changed(palette);
        // The terminal calls itself "Terminal" until a program says otherwise,
        // and a pane is better named by what it runs.
        term.title.clear();
        Self {
            id,
            term,
            command: "shell".into(),
            copy_mode: false,
            marking: false,
        }
    }

    /// The pane's name: what the program in it calls itself, or what was
    /// started there.
    fn title(&self) -> &str {
        if self.term.title.is_empty() {
            &self.command
        } else {
            &self.term.title
        }
    }

    fn enter_copy_mode(&mut self) {
        self.copy_mode = true;
    }

    /// Leave copy mode, back to the live screen, dropping a selection the
    /// copy-mode keys were making.
    fn exit_copy_mode(&mut self) {
        self.copy_mode = false;
        self.term.scroll_offset = 0;
        if self.marking {
            self.term.clear_selection();
        }
        self.marking = false;
    }

    /// One screenful, for the page keys. At least one line, so a pane squeezed
    /// to nothing still scrolls rather than ignoring the key.
    fn page(&self) -> usize {
        self.term.rows().max(1)
    }

    fn scroll_back(&mut self, lines: usize) {
        self.term.scroll_viewport_up(lines);
        self.extend_mark();
    }

    fn scroll_forward(&mut self, lines: usize) {
        self.term.scroll_viewport_down(lines);
        self.extend_mark();
    }

    fn scroll_to_top(&mut self) {
        self.term.scroll_viewport_up(self.term.buffer_len());
        self.extend_mark();
    }

    fn scroll_to_bottom(&mut self) {
        self.term.scroll_offset = 0;
        self.extend_mark();
    }

    /// `v` in copy mode: the top line of the view is one end of a selection,
    /// whole lines, and the view's top line as it moves is the other.
    ///
    /// Kept in the terminal's own selection rather than beside it, for two
    /// reasons: the terminal draws its selection, so the user sees what `y`
    /// will copy; and the terminal renumbers its selection when the oldest
    /// line falls off the scrollback, which a mark kept here would not know
    /// had happened.
    fn set_mark(&mut self) {
        let top = self.term.viewport_top();
        let last = self.term.cols().saturating_sub(1);
        self.term.selection = Some(Selection {
            start_row: top,
            start_col: 0,
            end_row: top,
            end_col: last,
            active: false,
        });
        self.marking = true;
    }

    /// Move the moving end of a `v` selection to the view's top line.
    ///
    /// Whole lines in either direction: the columns are set so that, once the
    /// terminal orders the two ends, the upper line starts at its first column
    /// and the lower one ends at its last.
    fn extend_mark(&mut self) {
        if !self.marking {
            return;
        }
        let here = self.term.viewport_top();
        let last = self.term.cols().saturating_sub(1);
        if let Some(sel) = self.term.selection.as_mut() {
            let (start_col, end_col) = if here >= sel.start_row {
                (0, last)
            } else {
                (last, 0)
            };
            sel.start_col = start_col;
            sel.end_row = here;
            sel.end_col = end_col;
        }
    }
}

// ============================================================================
// Layout tree
// ============================================================================

/// A node in the pane layout tree. Either a leaf (single pane) or a split
/// (two children with a split direction and ratio).
#[derive(Debug, Clone)]
enum LayoutNode {
    Leaf(PaneId),
    Split {
        direction: SplitDir,
        /// The first child's share, `0.0`-`1.0`.
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl LayoutNode {
    /// Move the divider of the innermost split containing `target`.
    ///
    /// `true` if a split was found and adjusted. A layout that is a single leaf
    /// has no ratio to change, which is why this reports whether it did
    /// anything: the caller shows "no split to resize" rather than nothing at
    /// all happening.
    ///
    /// The *innermost* split, not the outermost: with panes split twice, the
    /// divider a user means by "make this pane wider" is the one they can see
    /// beside it. Walking down to the deepest split that still contains the
    /// pane is what picks that one.
    ///
    /// `delta` moves the *divider*, not the active pane: Right and Down push it
    /// towards the far edge whichever side of it the pane is on. That is what
    /// tmux's `resize-pane -R` does and what an arrow key means -- "grow
    /// whichever pane I am in" would send the divider left for a right-hand
    /// pane and left-arrow would then send it right, which is a control that
    /// reverses itself depending on where the cursor happens to be.
    fn resize_containing(&mut self, target: PaneId, delta: f32) -> bool {
        let Self::Split {
            ratio,
            first,
            second,
            ..
        } = self
        else {
            return false;
        };

        // Deeper first: if a child split also holds the pane, it is the nearer
        // divider and the one to move.
        if first.resize_containing(target, delta) || second.resize_containing(target, delta) {
            return true;
        }

        if !first.contains(target) && !second.contains(target) {
            return false;
        }
        // Clamped well inside 0 and 1: a ratio of zero is a pane with no
        // pixels, which `compute_bounds` then floors at `MIN_PANE_SIZE`,
        // leaving the divider stuck against an edge it cannot come back from.
        *ratio = (*ratio + delta).clamp(MIN_SPLIT_RATIO, 1.0 - MIN_SPLIT_RATIO);
        true
    }

    /// Give `target` a larger share of the innermost split holding it -- or,
    /// with a negative `delta`, a smaller one.
    ///
    /// This replaced `adjust_ratio`, which moved the *outermost* split's ratio
    /// and always in the first child's favour: `prefix +`, "grow the pane",
    /// shrank every pane on the right or at the bottom.
    fn grow(&mut self, target: PaneId, delta: f32) -> bool {
        let Self::Split {
            ratio,
            first,
            second,
            ..
        } = self
        else {
            return false;
        };
        if first.grow(target, delta) || second.grow(target, delta) {
            return true;
        }
        let signed = if first.contains(target) {
            delta
        } else if second.contains(target) {
            -delta
        } else {
            return false;
        };
        *ratio = (*ratio + signed).clamp(MIN_SPLIT_RATIO, 1.0 - MIN_SPLIT_RATIO);
        true
    }

    /// Whether this subtree holds `target`.
    fn contains(&self, target: PaneId) -> bool {
        match self {
            Self::Leaf(id) => *id == target,
            Self::Split { first, second, .. } => first.contains(target) || second.contains(target),
        }
    }

    /// Where each pane of this subtree sits, given the rectangle it fills.
    fn compute_bounds(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        match self {
            Self::Leaf(id) => vec![(*id, area)],
            Self::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let mut result = Vec::new();
                match direction {
                    SplitDir::Stacked => {
                        let first_h = (area.h * ratio).max(MIN_PANE_SIZE);
                        let second_h = (area.h - first_h - PANE_BORDER_WIDTH).max(MIN_PANE_SIZE);
                        result.extend(
                            first.compute_bounds(Rect::new(area.x, area.y, area.w, first_h)),
                        );
                        result.extend(second.compute_bounds(Rect::new(
                            area.x,
                            area.y + first_h + PANE_BORDER_WIDTH,
                            area.w,
                            second_h,
                        )));
                    }
                    SplitDir::SideBySide => {
                        let first_w = (area.w * ratio).max(MIN_PANE_SIZE);
                        let second_w = (area.w - first_w - PANE_BORDER_WIDTH).max(MIN_PANE_SIZE);
                        result.extend(
                            first.compute_bounds(Rect::new(area.x, area.y, first_w, area.h)),
                        );
                        result.extend(second.compute_bounds(Rect::new(
                            area.x + first_w + PANE_BORDER_WIDTH,
                            area.y,
                            second_w,
                            area.h,
                        )));
                    }
                }
                result
            }
        }
    }

    /// Count the number of leaf (pane) nodes.
    fn pane_count(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Split { first, second, .. } => {
                first.pane_count().saturating_add(second.pane_count())
            }
        }
    }

    /// Collect all pane IDs in order.
    fn pane_ids(&self) -> Vec<PaneId> {
        match self {
            Self::Leaf(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut ids = first.pane_ids();
                ids.extend(second.pane_ids());
                ids
            }
        }
    }

    /// Remove a pane from the layout tree, its sibling taking its place.
    ///
    /// `false` if the pane is not here -- and for a tree that is only this
    /// pane, which cannot be removed from itself: the caller closes the window
    /// instead.
    fn remove_pane(&mut self, target: PaneId) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                if matches!(**first, Self::Leaf(id) if id == target) {
                    *self = (**second).clone();
                    return true;
                }
                if matches!(**second, Self::Leaf(id) if id == target) {
                    *self = (**first).clone();
                    return true;
                }
                first.remove_pane(target) || second.remove_pane(target)
            }
        }
    }

    /// Split a pane, replacing it with a split node containing the original
    /// pane and a new pane.
    fn split_pane(&mut self, target: PaneId, new_id: PaneId, direction: SplitDir) -> bool {
        match self {
            Self::Leaf(id) if *id == target => {
                *self = Self::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(Self::Leaf(target)),
                    second: Box::new(Self::Leaf(new_id)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_pane(target, new_id, direction)
                    || second.split_pane(target, new_id, direction)
            }
        }
    }

    /// Exchange two panes' places, leaving the shape of the tree alone.
    ///
    /// What `prefix }` promised. It was bound to "cycle the active pane
    /// forward" under a comment calling a real swap too complex -- and the
    /// card the user reads said "Swap this pane with the next".
    fn swap(&mut self, a: PaneId, b: PaneId) {
        match self {
            Self::Leaf(id) => {
                if *id == a {
                    *id = b;
                } else if *id == b {
                    *id = a;
                }
            }
            Self::Split { first, second, .. } => {
                first.swap(a, b);
                second.swap(a, b);
            }
        }
    }
}

// ============================================================================
// Layout presets
// ============================================================================

/// tmux's five layouts, by tmux's names and with tmux's meanings.
///
/// `even-horizontal` spreads the panes *horizontally* -- left to right -- and
/// `main-horizontal` puts the main pane on top with the rest in a row beneath
/// it. The two `even` layouts were the other way round here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutPreset {
    /// All panes side by side, left to right.
    EvenHorizontal,
    /// All panes one above the other.
    EvenVertical,
    /// One main pane on top, the rest side by side below it.
    MainHorizontal,
    /// One main pane on the left, the rest one above the other beside it.
    MainVertical,
    /// Rows of side-by-side panes.
    Tiled,
}

impl LayoutPreset {
    const ALL: [Self; 5] = [
        Self::EvenHorizontal,
        Self::EvenVertical,
        Self::MainHorizontal,
        Self::MainVertical,
        Self::Tiled,
    ];

    /// tmux's name for the layout, which `:select-layout` takes and the status
    /// bar reports when `prefix Space` changes it.
    fn name(self) -> &'static str {
        match self {
            Self::EvenHorizontal => "even-horizontal",
            Self::EvenVertical => "even-vertical",
            Self::MainHorizontal => "main-horizontal",
            Self::MainVertical => "main-vertical",
            Self::Tiled => "tiled",
        }
    }

    fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The layout after this one, for `prefix Space`.
    fn next(self) -> Self {
        match self {
            Self::EvenHorizontal => Self::EvenVertical,
            Self::EvenVertical => Self::MainHorizontal,
            Self::MainHorizontal => Self::MainVertical,
            Self::MainVertical => Self::Tiled,
            Self::Tiled => Self::EvenHorizontal,
        }
    }

    /// Build a layout tree from a preset and a list of pane IDs. `None` for
    /// no panes, which is no layout at all.
    fn build(self, panes: &[PaneId]) -> Option<LayoutNode> {
        let (&main_pane, rest) = panes.split_first()?;
        if rest.is_empty() {
            return Some(LayoutNode::Leaf(main_pane));
        }
        Some(match self {
            Self::EvenHorizontal => Self::build_even(panes, SplitDir::SideBySide)?,
            Self::EvenVertical => Self::build_even(panes, SplitDir::Stacked)?,
            Self::MainHorizontal => LayoutNode::Split {
                direction: SplitDir::Stacked,
                ratio: 0.6,
                first: Box::new(LayoutNode::Leaf(main_pane)),
                second: Box::new(Self::build_even(rest, SplitDir::SideBySide)?),
            },
            Self::MainVertical => LayoutNode::Split {
                direction: SplitDir::SideBySide,
                ratio: 0.6,
                first: Box::new(LayoutNode::Leaf(main_pane)),
                second: Box::new(Self::build_even(rest, SplitDir::Stacked)?),
            },
            Self::Tiled => Self::build_tiled(panes)?,
        })
    }

    /// `panes` in equal shares along one direction.
    fn build_even(panes: &[PaneId], direction: SplitDir) -> Option<LayoutNode> {
        if let [only] = panes {
            return Some(LayoutNode::Leaf(*only));
        }
        let mid = panes.len() / 2;
        let (front, back) = (panes.get(..mid)?, panes.get(mid..)?);
        Some(LayoutNode::Split {
            direction,
            ratio: mid as f32 / panes.len() as f32,
            first: Box::new(Self::build_even(front, direction)?),
            second: Box::new(Self::build_even(back, direction)?),
        })
    }

    /// Two rows of side-by-side panes; two panes are simply side by side.
    fn build_tiled(panes: &[PaneId]) -> Option<LayoutNode> {
        match panes {
            [] => None,
            [only] => Some(LayoutNode::Leaf(*only)),
            [left, right] => Some(LayoutNode::Split {
                direction: SplitDir::SideBySide,
                ratio: 0.5,
                first: Box::new(LayoutNode::Leaf(*left)),
                second: Box::new(LayoutNode::Leaf(*right)),
            }),
            _ => {
                let mid = panes.len() / 2;
                Some(LayoutNode::Split {
                    direction: SplitDir::Stacked,
                    ratio: 0.5,
                    first: Box::new(Self::build_even(panes.get(..mid)?, SplitDir::SideBySide)?),
                    second: Box::new(Self::build_even(panes.get(mid..)?, SplitDir::SideBySide)?),
                })
            }
        }
    }
}

// ============================================================================
// Window (tab)
// ============================================================================

/// A window's identity, which survives the windows before it closing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct WindowId(usize);

/// A tmux "window" — a tab containing one or more panes arranged by a layout.
#[derive(Debug, Clone)]
struct Window {
    /// What a click on its tab names: its position moves when a window before
    /// it closes, and this does not.
    id: WindowId,
    /// Its name, which `prefix ,` changes.
    name: String,
    /// Layout tree describing pane arrangement.
    layout: LayoutNode,
    /// The currently focused pane.
    active_pane: PaneId,
    /// The pane that was active before this one, for `prefix ;`.
    ///
    /// The card said ";" was "the pane you were in before", and the key moved
    /// to the previous pane in the layout's order -- which is the pane you
    /// were in before only when you got here with `o`.
    last_pane: Option<PaneId>,
    /// Layout preset applied to this window.
    preset: LayoutPreset,
    /// Whether the active pane is filling the window on its own.
    ///
    /// tmux's `prefix z`. Before this the key existed and set a status line
    /// reading "Pane zoom toggled" -- the word "zoom" appeared exactly once
    /// in this crate, in that string. It announced a thing it did not do.
    zoomed: bool,
    /// Its number: what its tab shows and what the prefix and a digit reach.
    ///
    /// The lowest number free when it was made, as in tmux. It was a count of
    /// every window ever made while the digits chose by *position*, so once a
    /// window had closed, `prefix 1` went to the window labelled 2.
    index: usize,
}

impl Window {
    fn new(id: WindowId, index: usize, initial_pane: PaneId) -> Self {
        Self {
            id,
            name: "shell".to_string(),
            layout: LayoutNode::Leaf(initial_pane),
            active_pane: initial_pane,
            last_pane: None,
            preset: LayoutPreset::Tiled,
            zoomed: false,
            index,
        }
    }

    /// Make `id` the active pane, remembering the one it replaces.
    fn select_pane(&mut self, id: PaneId) {
        if id != self.active_pane {
            self.last_pane = Some(self.active_pane);
            self.active_pane = id;
        }
    }

    /// Where each pane sits, given the window's pixel size.
    ///
    /// The one layout walk. The drawing paints these rectangles and
    /// [`Multiplexer::relayout`] turns them into each terminal's columns and
    /// rows; when the two computed the pane area separately, only one of them
    /// was ever told about a change.
    fn bounds(&self, width: f32, height: f32) -> Vec<(PaneId, Rect)> {
        let area = Rect::new(
            0.0,
            TAB_BAR_HEIGHT,
            width.max(0.0),
            (height - TAB_BAR_HEIGHT - STATUS_BAR_HEIGHT).max(0.0),
        );
        if self.zoomed {
            // The whole area, one pane. Here rather than in the drawing
            // because this is the only function that turns a layout into
            // rectangles: `relayout` reads it to size the terminal, so a
            // zoomed pane is given the cells it is drawn with rather than the
            // cells it had when it was a quarter of the screen.
            return vec![(self.active_pane, area)];
        }
        self.layout.compute_bounds(area)
    }
}

/// The terminal's part of a pane's rectangle: below the title strip, inside
/// the border.
fn pane_content(pane: Rect) -> Rect {
    Rect::new(
        pane.x + PANE_INSET,
        pane.y + PANE_TITLE_HEIGHT,
        (pane.w - PANE_INSET * 2.0).max(0.0),
        (pane.h - PANE_TITLE_HEIGHT - PANE_INSET).max(0.0),
    )
}

// ============================================================================
// Session
// ============================================================================

/// A session's identity, which survives the sessions before it ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SessionId(usize);

/// A tmux session — a collection of windows that can be detached/reattached.
#[derive(Debug, Clone)]
struct Session {
    /// What a click in the session chooser names.
    id: SessionId,
    /// Session name.
    name: String,
    /// Windows in this session, in the order of their numbers.
    windows: Vec<Window>,
    /// Active window's position in `windows`.
    active_window: usize,
    /// Next window ID counter.
    next_window_id: usize,
}

impl Session {
    fn new(id: SessionId, name: &str, first_pane: PaneId) -> Self {
        Self {
            id,
            name: name.to_string(),
            windows: vec![Window::new(WindowId(0), 0, first_pane)],
            active_window: 0,
            next_window_id: 1,
        }
    }

    fn alloc_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id = self.next_window_id.saturating_add(1);
        id
    }

    /// The lowest window number no window has.
    fn free_window_index(&self) -> usize {
        (0..=self.windows.len())
            .find(|i| !self.windows.iter().any(|w| w.index == *i))
            .unwrap_or(self.windows.len())
    }

    fn active_window(&self) -> Option<&Window> {
        self.windows.get(self.active_window)
    }

    fn active_window_mut(&mut self) -> Option<&mut Window> {
        self.windows.get_mut(self.active_window)
    }

    /// Where the window with this identity is in `windows`.
    fn position_of(&self, id: WindowId) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    /// Every pane in every window of the session.
    fn pane_ids(&self) -> Vec<PaneId> {
        self.windows
            .iter()
            .flat_map(|w| w.layout.pane_ids())
            .collect()
    }
}

// ============================================================================
// Modes
// ============================================================================

/// The prefix key mode state. tmux uses Ctrl+B as prefix key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefixState {
    /// Normal input — keys go to the active pane.
    Normal,
    /// Prefix key was pressed — next key is a tmux command.
    Prefix,
}

/// Something that ends a program, waiting for a yes.
///
/// tmux asks before `prefix x` and `prefix &` -- "kill-pane 3? (y/n)" -- and
/// here the question matters for the same reason it does there: a pane has a
/// shell in it now, and closing the pane ends the shell and whatever it was
/// running. One key next to the prefix is too easy to hit for that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    ClosePane(PaneId),
    CloseWindow(WindowId),
}

/// The prefix commands this multiplexer answers, as a reader sees them.
///
/// Every one is pressed *after* the prefix -- Ctrl+B, then the key -- which
/// is why the closing line says so and why none of these rows carries a
/// modifier. The window showed "Ctrl+B ..." while the prefix was armed and
/// named not one of the twenty-odd keys it was waiting for.
///
/// `F1` is the exception and is not a prefix command: a list you can only
/// reach by already knowing a key is not much of a list. Copy mode's keys work
/// with the prefix and, while copy mode is on, without it -- as in tmux, where
/// copy mode takes the keyboard.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F1", "This list"),
    ("?", "This list, after the prefix"),
    ("c", "New window"),
    ("n / p", "Next or previous window"),
    ("0-9", "The window with that number"),
    ("w", "Choose a window from a list"),
    ("s", "Choose a session from a list"),
    ("&", "Close the window (asks first)"),
    ("- or \"", "Split the pane, one above the other"),
    ("| or %", "Split the pane, side by side"),
    ("o", "Next pane"),
    (";", "The pane you were in before"),
    ("}", "Swap this pane with the next"),
    ("z", "Zoom this pane to fill the window, and back"),
    ("+", "Grow the pane"),
    ("Arrows", "Move the divider beside the pane"),
    ("x", "Close the pane (asks first)"),
    ("Space", "Next layout"),
    ("[", "Copy mode, to look back at what scrolled past"),
    ("k / j", "Copy mode: back or forward one line"),
    ("b / f", "Copy mode: back or forward one screen"),
    ("g / G", "Copy mode: the oldest line, or the live screen"),
    ("v", "Copy mode: start a selection"),
    ("y", "Copy the selection, or what the pointer selected"),
    ("q", "Leave copy mode"),
    ("]", "Paste into the program in the pane"),
    (",", "Rename the window"),
    (":", "Type a command"),
    ("d", "Detach"),
];

/// What a click can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// A window's tab: show that window.
    Tab(WindowId),
    /// After the tabs: a new window.
    NewWindow,
    /// At the end of the tab bar: the list of keys.
    Help,
    /// A pane's title strip: make it the active pane.
    PaneTitle(PaneId),
    /// A part of a pane's terminal -- its grid, or its scrollback bar.
    Pane(PaneId, TermTarget),
    /// The session's name on the status bar: the session chooser.
    SessionName,
    /// A window in the status bar's list.
    StatusWindow(WindowId),
    /// A session in the session chooser.
    SessionRow(SessionId),
    /// A window in the window chooser.
    WindowRow(WindowId),
    /// Behind an open chooser: closes it.
    Scrim,
    /// A chooser's box, between its rows: nothing, rather than the scrim.
    ChooserBox,
    /// The detached screen's button: attach again.
    Attach,
    /// The detached screen's other button: choose a session.
    ChooseSession,
    /// The confirmation's two answers.
    ConfirmYes,
    ConfirmNo,
    /// The shortcut card, anywhere on it: closes it.
    HelpCard,
}

/// How a pane's shell is started: `termchild`'s `spawn_shell` in a real
/// window, a script in a test.
type Spawner = Box<dyn FnMut(WinSize) -> Result<Box<dyn Link>, SpawnError>>;

/// The wall clock, in milliseconds since the epoch, or `None` if the system
/// does not know it. A function so a test can stop the clock.
fn system_clock() -> Option<u64> {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(since.as_millis()).ok()
}

// ============================================================================
// Multiplexer state
// ============================================================================

/// Top-level terminal multiplexer state.
struct Multiplexer {
    /// Every pane of every session. A pane is removed -- and its shell hung
    /// up -- when it closes; nothing else holds one.
    panes: Vec<Pane>,
    /// The next pane's number. Counted rather than taken from `panes.len()`,
    /// which repeats once panes are removed.
    next_pane_id: usize,
    /// All sessions.
    sessions: Vec<Session>,
    /// Next session ID counter.
    next_session_id: usize,
    /// Currently active session index.
    active_session: usize,
    /// Whether the window has detached from its session.
    ///
    /// One flag for the window rather than one per session: there is one
    /// client -- this window -- and it is attached to one session or to none.
    detached: bool,
    /// Whether the shortcut card is up.
    show_help: bool,
    /// Prefix key state.
    prefix_state: PrefixState,
    /// Whether the command prompt is shown (`:` command mode).
    command_mode: bool,
    /// Command input buffer.
    command_input: String,
    /// A close waiting for its yes.
    confirm: Option<Confirm>,
    /// Status message (shown in status bar until it is old).
    status_message: String,
    /// When the message was set, on the uptime clock.
    status_time: u64,
    /// Milliseconds this window has been open, from the ticks.
    uptime_ms: u64,
    /// The wall clock at the last tick, for the status bar's time.
    wall_ms: Option<u64>,
    /// Where the wall clock is read from.
    clock: fn() -> Option<u64>,
    /// Clipboard content (from copy mode).
    clipboard: String,
    /// Whether the session chooser is open.
    session_chooser: bool,
    /// Window chooser open flag.
    window_chooser: bool,
    /// Carries a precision wheel's fractions of a row in the choosers.
    chooser_wheel: wheel::Accumulator,
    /// How wide the window is, in pixels.
    window_width: f32,
    /// How tall the window is, in pixels.
    window_height: f32,
    /// Whether the window has the keyboard.
    focused: bool,
    /// The pane a press in its grid started a selection in, which the drag
    /// and the release belong to wherever the pointer goes.
    drag: Option<PaneId>,
    /// The user's colours, replaced whenever the theme changes.
    palette: Palette,
    /// How shells are started. `None` in the tests of everything else, where a
    /// pane is a terminal with nothing attached.
    spawner: Option<Spawner>,
    /// Set when the last session has ended: the window closes.
    quit: bool,
}

impl Multiplexer {
    /// A multiplexer whose panes run nothing: the tests of everything but
    /// the shells.
    #[cfg(test)]
    fn new() -> Self {
        Self::build(None)
    }

    /// A multiplexer whose panes run shells started by `spawner`.
    fn with_shells(spawner: Spawner) -> Self {
        Self::build(Some(spawner))
    }

    fn build(spawner: Option<Spawner>) -> Self {
        let mut mux = Self {
            panes: Vec::new(),
            next_pane_id: 0,
            sessions: Vec::new(),
            next_session_id: 0,
            active_session: 0,
            detached: false,
            show_help: false,
            prefix_state: PrefixState::Normal,
            command_mode: false,
            command_input: String::new(),
            confirm: None,
            status_message: String::new(),
            status_time: 0,
            uptime_ms: 0,
            wall_ms: system_clock(),
            clock: system_clock,
            clipboard: String::new(),
            session_chooser: false,
            window_chooser: false,
            chooser_wheel: wheel::Accumulator::default(),
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
            focused: true,
            drag: None,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            spawner,
            quit: false,
        };
        mux.new_session("main");
        // "Created session: main" is news only for a session the user asked
        // for.
        mux.status_message.clear();
        mux
    }

    // ========================================================================
    // Panes and their shells
    // ========================================================================

    /// A new pane with nothing in it yet.
    fn create_pane(&mut self) -> PaneId {
        let id = PaneId(self.next_pane_id);
        self.next_pane_id = self.next_pane_id.saturating_add(1);
        self.panes.push(Pane::new(id, &self.palette));
        id
    }

    /// Start the pane's shell -- once it is in a layout and sized, so the
    /// shell is born at the size it is drawn at.
    fn start_shell(&mut self, id: PaneId) {
        let Some(spawn) = self.spawner.as_mut() else {
            return;
        };
        if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id) {
            pane.term.start_with(spawn);
        }
    }

    /// Hang up a pane's shell and forget the pane.
    fn drop_pane(&mut self, id: PaneId) {
        if let Some(at) = self.panes.iter().position(|p| p.id == id) {
            let mut pane = self.panes.remove(at);
            pane.term.hang_up();
        }
        if self.drag == Some(id) {
            self.drag = None;
        }
    }

    fn find_pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }

    fn find_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|p| p.id == id)
    }

    fn active_session(&self) -> Option<&Session> {
        self.sessions.get(self.active_session)
    }

    fn active_session_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(self.active_session)
    }

    fn active_window(&self) -> Option<&Window> {
        self.active_session()?.active_window()
    }

    fn active_window_mut(&mut self) -> Option<&mut Window> {
        self.active_session_mut()?.active_window_mut()
    }

    fn active_pane_id(&self) -> Option<PaneId> {
        Some(self.active_window()?.active_pane)
    }

    /// The pane keystrokes are addressed to: the active window's active pane.
    fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        let id = self.active_pane_id()?;
        self.find_pane_mut(id)
    }

    fn set_status(&mut self, msg: &str) {
        self.status_message = msg.to_string();
        self.status_time = self.uptime_ms;
    }

    /// Tell every pane of the attached session the size it is drawn at.
    ///
    /// A terminal is only correct if the program writing into it and the pane
    /// painting it agree about the grid, and only the layout walk knows a
    /// pane's rectangle. Every window of the session is sized, not just the
    /// visible one, because a background window's program keeps running and
    /// writing -- it must not discover a new width only when the user switches
    /// to it. Each terminal tells its own shell (`TIOCSWINSZ`) when its grid
    /// changes, and does nothing when it has not, which is what makes calling
    /// this on every frame free.
    fn relayout(&mut self) {
        let (width, height) = (self.window_width, self.window_height);
        let Some(session) = self.active_session() else {
            return;
        };
        // Collected first because sizing a pane needs `&mut self.panes` while
        // the walk borrows `self.sessions`.
        let rects: Vec<(PaneId, Rect)> = session
            .windows
            .iter()
            .flat_map(|w| w.bounds(width, height))
            .collect();
        for (id, rect) in rects {
            let content = pane_content(rect);
            if let Some(pane) = self.find_pane_mut(id) {
                pane.term.resize_to_window(content.w, content.h);
            }
        }
        self.sync_focus();
    }

    /// Give the keyboard to the pane that will get the next key, and take it
    /// from every other.
    ///
    /// Only when it changes: giving a terminal the keyboard restarts its
    /// cursor's blink, and a blink restarted on every event never blinks.
    fn sync_focus(&mut self) {
        let modal = self.show_help
            || self.command_mode
            || self.confirm.is_some()
            || self.session_chooser
            || self.window_chooser;
        let keyboard = if self.focused && !self.detached && !modal {
            self.active_pane_id()
        } else {
            None
        };
        for pane in &mut self.panes {
            let want = Some(pane.id) == keyboard;
            if pane.term.is_focused() != want {
                pane.term.set_focused(want);
            }
        }
    }

    /// The panes on screen now.
    fn visible_panes(&self) -> Vec<PaneId> {
        if self.detached {
            return Vec::new();
        }
        self.active_window()
            .map(|w| {
                w.bounds(self.window_width, self.window_height)
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Where a pane of the active window is drawn -- or would be, while the
    /// window is detached.
    fn pane_rect(&self, id: PaneId) -> Option<Rect> {
        self.active_window()?
            .bounds(self.window_width, self.window_height)
            .into_iter()
            .find(|(p, _)| *p == id)
            .map(|(_, r)| r)
    }

    // ========================================================================
    // The clock
    // ========================================================================

    /// Advance the clocks and read every shell.
    ///
    /// Every pane, not only the ones on screen: a shell in a background window
    /// or a detached session keeps writing, and one whose output is never read
    /// fills its terminal's buffer and stops. `true` if anything on screen
    /// changed.
    fn tick(&mut self, elapsed_ms: u64) -> bool {
        let before = self.status_text();
        self.uptime_ms = self.uptime_ms.saturating_add(elapsed_ms);
        self.wall_ms = (self.clock)();
        let mut changed = self.status_text() != before;

        let visible = self.visible_panes();
        let mut finished = Vec::new();
        for pane in &mut self.panes {
            match pane.term.on_event(&Event::Tick { elapsed_ms }) {
                Response::Exit => finished.push(pane.id),
                // `KeepOpen` is a redraw to anything but a close request, and a
                // terminal never answers one with it.
                Response::Redraw | Response::KeepOpen => changed |= visible.contains(&pane.id),
                Response::Idle => {}
            }
        }
        for id in finished {
            self.pane_finished(id);
            changed = true;
        }
        changed
    }

    /// A shell exited cleanly: its pane closes, as in tmux -- and its window
    /// with it if it was the last, and its session if that was the last
    /// window.
    ///
    /// A shell that failed or was killed does not come here: its terminal
    /// says how it ended, and the pane stays until the user closes it.
    fn pane_finished(&mut self, id: PaneId) {
        self.remove_pane(id);
    }

    /// When the status bar next changes by itself: a message expiring, or
    /// the clock's minute turning.
    fn status_wake_ms(&self) -> u64 {
        let age = self.uptime_ms.saturating_sub(self.status_time);
        if !self.status_message.is_empty() && age < STATUS_MESSAGE_MS {
            return STATUS_MESSAGE_MS.saturating_sub(age).max(1);
        }
        match self.wall_ms {
            Some(ms) => 60_000_u64.saturating_sub(ms % 60_000).max(1),
            None => 60_000,
        }
    }

    /// The right-hand end of the status bar: a fresh message, or the time.
    fn status_text(&self) -> String {
        let age = self.uptime_ms.saturating_sub(self.status_time);
        if !self.status_message.is_empty() && age < STATUS_MESSAGE_MS {
            return self.status_message.clone();
        }
        self.wall_ms.map_or_else(String::new, clock_text)
    }

    // ========================================================================
    // Sessions
    // ========================================================================

    /// Create a new session, with one window of one shell, and attach to it.
    fn new_session(&mut self, name: &str) {
        if self.sessions.len() >= MAX_SESSIONS {
            self.set_status(&format!("at most {MAX_SESSIONS} sessions"));
            return;
        }
        if self.sessions.iter().any(|s| s.name == name) {
            // tmux refuses too: `attach` and `kill-session` find a session by
            // its name, and two of one name cannot both be found.
            self.set_status(&format!("duplicate session: {name}"));
            return;
        }
        let sid = SessionId(self.next_session_id);
        self.next_session_id = self.next_session_id.saturating_add(1);
        let pane = self.create_pane();
        self.sessions.push(Session::new(sid, name, pane));
        self.active_session = self.sessions.len().saturating_sub(1);
        self.detached = false;
        self.relayout();
        self.start_shell(pane);
        self.set_status(&format!("Created session: {name}"));
    }

    /// A name no session has, for `:new-session` without one.
    fn unused_session_name(&self) -> String {
        // One more candidate than there can be sessions, so one is free.
        (0..=MAX_SESSIONS)
            .map(|n| format!("session-{n}"))
            .find(|name| !self.sessions.iter().any(|s| &s.name == name))
            .unwrap_or_else(|| "session".to_string())
    }

    /// Detach from the current session. Its shells go on running.
    fn detach(&mut self) {
        if self.detached {
            return;
        }
        self.detached = true;
        self.session_chooser = false;
        self.window_chooser = false;
        if let Some(name) = self.active_session().map(|s| s.name.clone()) {
            self.set_status(&format!("Detached from session: {name}"));
        }
    }

    /// Attach to a session by index.
    fn attach(&mut self, index: usize) {
        let Some(name) = self.sessions.get(index).map(|s| s.name.clone()) else {
            self.set_status(&format!("no session {index}"));
            return;
        };
        self.active_session = index;
        self.detached = false;
        // A session that was not attached has not been sized since the window
        // last changed.
        self.relayout();
        self.set_status(&format!("Attached to session: {name}"));
    }

    /// A session named by the user: its number in the list, or its name.
    fn session_named(&self, arg: &str) -> Option<usize> {
        arg.parse::<usize>()
            .ok()
            .filter(|i| *i < self.sessions.len())
            .or_else(|| self.sessions.iter().position(|s| s.name == arg))
    }

    /// Move to the next session, wrapping at the end.
    fn next_session(&mut self) {
        if let Some(next) = self
            .active_session
            .saturating_add(1)
            .checked_rem(self.sessions.len())
        {
            self.active_session = next;
            self.relayout();
        }
    }

    /// Move to the previous session, wrapping at the start.
    fn prev_session(&mut self) {
        let Some(last) = self.sessions.len().checked_sub(1) else {
            return;
        };
        self.active_session = self
            .active_session
            .checked_sub(1)
            .map_or(last, |prev| prev.min(last));
        self.relayout();
    }

    /// End a session: every shell in it is hung up. The last session ending
    /// closes the window, as the last one ending ends tmux.
    fn kill_session(&mut self, index: usize) {
        let Some(session) = self.sessions.get(index) else {
            self.set_status(&format!("no session {index}"));
            return;
        };
        let name = session.name.clone();
        for id in session.pane_ids() {
            self.drop_pane(id);
        }
        self.sessions.remove(index);
        self.after_session_removed(index);
        self.set_status(&format!("Killed session: {name}"));
    }

    /// Keep `active_session` on a session after the one at `index` has gone.
    fn after_session_removed(&mut self, index: usize) {
        if self.sessions.is_empty() {
            self.quit = true;
            return;
        }
        if index < self.active_session {
            self.active_session = self.active_session.saturating_sub(1);
        }
        self.active_session = self
            .active_session
            .min(self.sessions.len().saturating_sub(1));
        self.relayout();
    }

    // ========================================================================
    // Windows
    // ========================================================================

    /// Create a new window in the current session, with a shell of its own.
    ///
    /// The limit is checked before the pane is made. It was checked after,
    /// so every refused window left a pane behind in the shared list -- and a
    /// pane now has a shell in it.
    fn new_window(&mut self) {
        let Some(session) = self.active_session() else {
            return;
        };
        if session.windows.len() >= MAX_WINDOWS {
            self.set_status(&format!("at most {MAX_WINDOWS} windows in a session"));
            return;
        }
        let pane = self.create_pane();
        let Some(session) = self.active_session_mut() else {
            return;
        };
        let wid = session.alloc_window_id();
        let index = session.free_window_index();
        let at = session
            .windows
            .iter()
            .position(|w| w.index > index)
            .unwrap_or(session.windows.len());
        session.windows.insert(at, Window::new(wid, index, pane));
        session.active_window = at;
        self.relayout();
        self.start_shell(pane);
    }

    /// Close a window and end every shell in it. The session's last window
    /// closing ends the session.
    fn close_window(&mut self, id: WindowId) {
        let Some(session) = self.active_session() else {
            return;
        };
        let Some(at) = session.position_of(id) else {
            return;
        };
        if session.windows.len() <= 1 {
            self.kill_session(self.active_session);
            return;
        }
        let panes = session
            .windows
            .get(at)
            .map(|w| w.layout.pane_ids())
            .unwrap_or_default();
        for pane in panes {
            self.drop_pane(pane);
        }
        if let Some(session) = self.active_session_mut() {
            session.windows.remove(at);
            if session.active_window >= session.windows.len() || session.active_window > at {
                session.active_window = session.active_window.saturating_sub(1);
            }
        }
        self.relayout();
    }

    /// Show the window at `position`.
    fn select_window(&mut self, position: usize) {
        if let Some(session) = self.active_session_mut()
            && position < session.windows.len()
        {
            session.active_window = position;
        }
        self.relayout();
    }

    /// Show the window with this identity.
    fn select_window_id(&mut self, id: WindowId) {
        if let Some(at) = self.active_session().and_then(|s| s.position_of(id)) {
            self.select_window(at);
        }
    }

    /// Show the window with this number, which is what its tab says.
    fn select_window_number(&mut self, number: usize) {
        let at = self
            .active_session()
            .and_then(|s| s.windows.iter().position(|w| w.index == number));
        match at {
            Some(at) => self.select_window(at),
            None => self.set_status(&format!("no window {number}")),
        }
    }

    /// Switch to next window.
    fn next_window(&mut self) {
        if let Some(session) = self.active_session_mut()
            && let Some(next) = session
                .active_window
                .saturating_add(1)
                .checked_rem(session.windows.len())
        {
            session.active_window = next;
        }
        self.relayout();
    }

    /// Switch to previous window.
    fn prev_window(&mut self) {
        if let Some(session) = self.active_session_mut()
            && let Some(last) = session.windows.len().checked_sub(1)
        {
            session.active_window = session
                .active_window
                .checked_sub(1)
                .map_or(last, |prev| prev.min(last));
        }
        self.relayout();
    }

    /// Rename current window.
    fn rename_window(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.set_status("a window needs a name");
            return;
        }
        if let Some(window) = self.active_window_mut() {
            window.name = name.to_string();
        }
    }

    // ========================================================================
    // Panes
    // ========================================================================

    /// Split the active pane, starting a shell in the new half.
    ///
    /// Refused when the pane is too small to halve -- tmux says "pane too
    /// small" -- rather than making two panes that overlap, which is what
    /// `compute_bounds` flooring each half at `MIN_PANE_SIZE` did to a pane
    /// narrower than two of them. And refused *before* the pane is made: the
    /// limit used to be checked after, leaving an orphan behind every time.
    fn split_pane(&mut self, direction: SplitDir) {
        let Some(window) = self.active_window() else {
            return;
        };
        if window.layout.pane_count() >= MAX_PANES {
            self.set_status(&format!("at most {MAX_PANES} panes in a window"));
            return;
        }
        let target = window.active_pane;
        if window.zoomed {
            // tmux unzooms to split: the new pane has to go somewhere visible.
            if let Some(window) = self.active_window_mut() {
                window.zoomed = false;
            }
        }
        let room = self.pane_rect(target).map_or(0.0, |r| match direction {
            SplitDir::Stacked => r.h,
            SplitDir::SideBySide => r.w,
        });
        if room < MIN_PANE_SIZE * 2.0 + PANE_BORDER_WIDTH {
            self.set_status("no room to split this pane");
            return;
        }
        let new = self.create_pane();
        if let Some(window) = self.active_window_mut() {
            window.layout.split_pane(target, new, direction);
            window.select_pane(new);
        }
        // The pane that was split is now half the size it was, and the new one
        // has only the placeholder grid. Both learn their real size here.
        self.relayout();
        self.start_shell(new);
    }

    /// Close a pane and end its shell, wherever it is. The last pane of a
    /// window closes the window; the last window of a session ends it; the
    /// last session closes this window.
    fn remove_pane(&mut self, id: PaneId) {
        let place = self.sessions.iter().enumerate().find_map(|(s, session)| {
            session
                .windows
                .iter()
                .position(|w| w.layout.contains(id))
                .map(|w| (s, w))
        });
        self.drop_pane(id);
        let Some((s, w)) = place else {
            return;
        };
        let Some(session) = self.sessions.get_mut(s) else {
            return;
        };
        let Some(window) = session.windows.get_mut(w) else {
            return;
        };
        if window.layout.remove_pane(id) {
            if window.active_pane == id {
                let next = window
                    .last_pane
                    .filter(|p| window.layout.contains(*p))
                    .or_else(|| window.layout.pane_ids().first().copied());
                if let Some(next) = next {
                    window.active_pane = next;
                }
            }
            if window.last_pane == Some(id) || window.last_pane == Some(window.active_pane) {
                window.last_pane = None;
            }
            if window.layout.pane_count() == 1 {
                window.zoomed = false;
            }
        } else {
            // The window's only pane: the window goes.
            session.windows.remove(w);
            if session.windows.is_empty() {
                self.sessions.remove(s);
                self.after_session_removed(s);
                return;
            }
            if session.active_window >= session.windows.len() || session.active_window > w {
                session.active_window = session.active_window.saturating_sub(1);
            }
        }
        self.relayout();
    }

    /// Navigate to the next pane in the active window.
    fn next_pane(&mut self) {
        if let Some(window) = self.active_window_mut() {
            let ids = window.layout.pane_ids();
            if let Some(pos) = ids.iter().position(|id| *id == window.active_pane)
                && let Some(id) = pos
                    .saturating_add(1)
                    .checked_rem(ids.len())
                    .and_then(|next| ids.get(next))
            {
                window.select_pane(*id);
            }
        }
        self.sync_focus();
    }

    /// Back to the pane that was active before this one.
    fn last_pane(&mut self) {
        let Some(window) = self.active_window_mut() else {
            return;
        };
        match window.last_pane.filter(|p| window.layout.contains(*p)) {
            Some(last) => window.select_pane(last),
            None => self.set_status("no last pane"),
        }
        self.sync_focus();
    }

    /// Make `id` the active pane of the window showing.
    fn select_pane(&mut self, id: PaneId) {
        if let Some(window) = self.active_window_mut()
            && window.layout.contains(id)
        {
            window.select_pane(id);
        }
        self.sync_focus();
    }

    /// Grow the active pane's share of the split beside it.
    fn grow_pane(&mut self) {
        let grew = self.active_window_mut().is_some_and(|window| {
            let id = window.active_pane;
            window.layout.grow(id, GROW_STEP)
        });
        if !grew {
            self.set_status("no split to resize");
        }
        self.relayout();
    }

    /// Move the divider beside the active pane.
    ///
    /// `delta` is in pixels, converted against the window so that a step moves
    /// the divider the same visible distance whichever way the split runs.
    fn resize_active_split(&mut self, delta: f32) {
        let span = self.window_width.max(1.0);
        let moved = self.active_window_mut().is_some_and(|window| {
            let id = window.active_pane;
            window.layout.resize_containing(id, delta / span)
        });
        if !moved {
            self.set_status("no split to resize");
        }
        self.relayout();
    }

    /// Apply a layout preset to the current window.
    fn apply_layout(&mut self, preset: LayoutPreset) {
        if let Some(window) = self.active_window_mut()
            && let Some(layout) = preset.build(&window.layout.pane_ids())
        {
            window.layout = layout;
            window.preset = preset;
        }
        self.relayout();
    }

    /// Swap the active pane with the next one; the active pane moves and stays
    /// active.
    fn swap_pane_next(&mut self) {
        if let Some(window) = self.active_window_mut() {
            let ids = window.layout.pane_ids();
            let here = window.active_pane;
            if let Some(pos) = ids.iter().position(|id| *id == here)
                && let Some(&other) = pos
                    .saturating_add(1)
                    .checked_rem(ids.len())
                    .and_then(|next| ids.get(next))
                && other != here
            {
                window.layout.swap(here, other);
            }
        }
        self.relayout();
    }

    /// Toggle the active pane filling the window.
    fn toggle_zoom(&mut self) {
        let zoomed = self.active_window_mut().and_then(|w| {
            if w.layout.pane_count() < 2 {
                return None;
            }
            w.zoomed = !w.zoomed;
            Some(w.zoomed)
        });
        self.relayout();
        match zoomed {
            Some(true) => self.set_status("pane zoomed -- the prefix and z again to unzoom"),
            Some(false) => self.set_status("pane unzoomed"),
            None => self.set_status("one pane: nothing to zoom over"),
        }
    }

    // ========================================================================
    // Copy and paste
    // ========================================================================

    /// Copy the active pane's selection -- made in copy mode or with the
    /// pointer -- and leave copy mode.
    fn yank_selection(&mut self) {
        let Some(pane) = self.active_pane_mut() else {
            return;
        };
        let text = pane.term.get_selection_text();
        if text.is_some() {
            pane.exit_copy_mode();
            pane.term.clear_selection();
        }
        let Some(text) = text else {
            self.set_status("nothing marked");
            return;
        };
        let lines = text.lines().count();
        self.clipboard = text;
        self.set_status(&format!("copied {lines} line(s)"));
    }

    /// Paste the clipboard into the active pane's program.
    ///
    /// To the program, as a paste -- not onto the screen, which is where this
    /// used to put it: fed through the pane's parser, the text appeared as
    /// though the program had printed it, and nothing had typed it.
    fn paste_clipboard(&mut self) {
        if self.clipboard.is_empty() {
            self.set_status("clipboard is empty");
            return;
        }
        let text = self.clipboard.clone();
        if let Some(pane) = self.active_pane_mut() {
            pane.term.paste(&text);
        }
    }
}

/// The status bar's time: hours and minutes, and the zone they are in.
///
/// UTC, said out loud, as every clock in this tree says it until there is a
/// per-process zone to read (`known-issues.md` ->
/// `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`). Through `tzrules` rather than
/// `% 86_400` so that a real zone is a one-line change here. The clock this
/// replaced counted the seconds the window had been open and printed them as
/// a time of day, so it read 00:00 at every launch.
fn clock_text(wall_ms: u64) -> String {
    let secs = i64::try_from(wall_ms / 1000).unwrap_or(i64::MAX);
    let zone = tzrules::Tz::utc();
    let info = zone.lookup(secs);
    let local = secs.saturating_add(i64::from(info.gmtoff));
    let into_day = local.rem_euclid(86_400);
    let name = std::str::from_utf8(info.name.as_bytes()).unwrap_or("UTC");
    format!("{:02}:{:02} {name}", into_day / 3600, (into_day / 60) % 60)
}

impl Multiplexer {
    // ========================================================================
    // Input
    // ========================================================================

    /// Handle one event from the window.
    fn handle_event(&mut self, event: &Event) -> EventResult {
        let result = match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                self.relayout();
                EventResult::Consumed
            }
            Event::FocusIn | Event::FocusOut => {
                self.focused = matches!(event, Event::FocusIn);
                EventResult::Consumed
            }
            Event::Tick { elapsed_ms } => {
                if self.tick(*elapsed_ms) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        };
        self.sync_focus();
        result
    }

    /// Handle a key press.
    ///
    /// In the order they take priority: F1's card; a close waiting for its
    /// yes; the `:` prompt, which swallows everything until Enter or Escape; the
    /// prefix, one key after Ctrl+B; an open chooser; the detached screen;
    /// copy mode, which has the keyboard as it does in tmux -- and then
    /// everything else, which is the program's in the active pane.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // Above everything that swallows keys: a list you cannot dismiss from
        // the state you reached it in is the failure this ordering avoids.
        if key.key == Key::F1 {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            // Modal: `d` detaches and `&` closes a window, and neither should
            // happen from behind a list somebody is reading.
            if matches!(key.key, Key::Escape | Key::Enter) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }
        if let Some(confirm) = self.confirm.take() {
            if key
                .typed()
                .next()
                .is_some_and(|c| c.eq_ignore_ascii_case(&'y'))
            {
                self.confirmed(confirm);
            } else {
                self.set_status("not closed");
            }
            return EventResult::Consumed;
        }
        if self.command_mode {
            return self.handle_command_key(key);
        }
        if self.prefix_state == PrefixState::Prefix {
            self.prefix_state = PrefixState::Normal;
            return self.handle_prefixed_key(key);
        }
        // Ctrl+B arms the prefix. This is the one chord the multiplexer keeps
        // for itself; everything else Ctrl is the pane's.
        if key.modifiers.ctrl && key.key == Key::B {
            self.prefix_state = PrefixState::Prefix;
            return EventResult::Consumed;
        }
        if self.session_chooser || self.window_chooser {
            return self.handle_chooser_key(key);
        }
        if self.detached {
            if key.key == Key::Enter {
                self.attach(self.active_session);
            }
            return EventResult::Consumed;
        }
        if self.active_pane_mut().is_some_and(|p| p.copy_mode) {
            return self.handle_copy_key(key);
        }
        // The program's. The terminal turns the key into what a terminal
        // sends for it and passes it down the pseudo-terminal: before this,
        // every key that was not a multiplexer command was dropped here.
        match self.active_pane_mut() {
            Some(pane) => {
                pane.term.handle_event(&Event::Key(key.clone()));
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    /// The key after the prefix.
    fn handle_prefixed_key(&mut self, key: &KeyEvent) -> EventResult {
        // An arrow moves the divider beside the active pane -- the layout's
        // ratio, a field nothing else ever wrote. Up and Down move a
        // top/bottom divider the way Left and Right move a left/right one;
        // which kind of divider it is comes from the split itself.
        match key.key {
            Key::Left | Key::Up => {
                self.resize_active_split(-RESIZE_STEP);
                return EventResult::Consumed;
            }
            Key::Right | Key::Down => {
                self.resize_active_split(RESIZE_STEP);
                return EventResult::Consumed;
            }
            _ => {}
        }
        // The prefix twice sends the program a Ctrl+B of its own, as tmux's
        // `send-prefix`: otherwise nothing running under the multiplexer could
        // ever be sent one.
        if key.modifiers.ctrl && key.key == Key::B {
            if let Some(pane) = self.active_pane_mut() {
                pane.term.handle_event(&Event::Key(key.clone()));
            }
            return EventResult::Consumed;
        }
        // Otherwise the prefix is followed by a *character*. A key that
        // carries none is not a command, and is spent rather than leaving the
        // prefix armed for whatever comes next.
        if let Some(ch) = key.typed().next() {
            self.process_prefix_key(ch);
        }
        EventResult::Consumed
    }

    /// Process a prefix command key.
    fn process_prefix_key(&mut self, key: char) {
        match key {
            '-' | '"' => self.split_pane(SplitDir::Stacked),
            '|' | '%' => self.split_pane(SplitDir::SideBySide),
            'o' => self.next_pane(),
            ';' => self.last_pane(),
            '}' => self.swap_pane_next(),
            '&' => self.ask_close_window(),
            'x' => self.ask_close_pane(),
            'n' => self.next_window(),
            'p' => self.prev_window(),
            'c' => self.new_window(),
            'd' => self.detach(),
            ',' => self.open_prompt(":rename-window "),
            ':' => self.open_prompt(":"),
            '?' => self.show_help = true,
            's' => {
                self.session_chooser = !self.session_chooser;
                self.window_chooser = false;
            }
            // Not while detached, where no window is showing to choose among:
            // the list would take the keyboard without being drawn.
            'w' if self.detached => self.set_status("attach to choose a window"),
            'w' => {
                self.window_chooser = !self.window_chooser;
                self.session_chooser = false;
            }
            '[' => {
                if let Some(pane) = self.active_pane_mut() {
                    pane.enter_copy_mode();
                }
            }
            ']' => self.paste_clipboard(),
            'y' => self.yank_selection(),
            'q' | 'k' | 'j' | 'b' | 'f' | 'g' | 'G' | 'v' => self.copy_command(key),
            ' ' => self.next_layout(),
            '+' => self.grow_pane(),
            'z' => self.toggle_zoom(),
            _ => match key.to_digit(10) {
                Some(n) => self.select_window_number(n as usize),
                None => self.set_status(&format!("Unknown key: {key}")),
            },
        }
    }

    /// A copy-mode command, from a bare key in copy mode or after the prefix.
    ///
    /// The keys that look *back* enter copy mode by themselves: asking to see
    /// earlier output is unambiguous, and needing `[` first would make the
    /// first press do nothing, which reads as the program being broken rather
    /// than modal. The forward keys do not, because scrolling towards a screen
    /// you are already looking at is a no-op worth ignoring.
    fn copy_command(&mut self, key: char) {
        let Some(pane) = self.active_pane_mut() else {
            return;
        };
        if key == 'q' {
            pane.exit_copy_mode();
            return;
        }
        if matches!(key, 'k' | 'b' | 'g') {
            pane.enter_copy_mode();
        }
        if !pane.copy_mode {
            return;
        }
        let page = pane.page();
        match key {
            'k' => pane.scroll_back(1),
            'j' => pane.scroll_forward(1),
            'b' => pane.scroll_back(page),
            'f' => pane.scroll_forward(page),
            'g' => pane.scroll_to_top(),
            'G' => pane.scroll_to_bottom(),
            'v' => pane.set_mark(),
            _ => {}
        }
        if key == 'v' {
            self.set_status("selection started");
        }
    }

    /// Keys while copy mode has the keyboard: tmux's vi keys and the keys a
    /// user reaches for anyway. Anything else is swallowed -- copy mode is a
    /// place to read, and a key that typed into the shell behind it would
    /// land somewhere the user is not looking.
    fn handle_copy_key(&mut self, key: &KeyEvent) -> EventResult {
        let command = match key.key {
            Key::Escape => Some('q'),
            Key::Up => Some('k'),
            Key::Down => Some('j'),
            Key::PageUp => Some('b'),
            Key::PageDown => Some('f'),
            Key::Home => Some('g'),
            Key::End => Some('G'),
            Key::Enter => Some('y'),
            _ => key.typed().next().map(|c| if c == ' ' { 'v' } else { c }),
        };
        match command {
            Some('y') => self.yank_selection(),
            Some(c @ ('q' | 'k' | 'j' | 'b' | 'f' | 'g' | 'G' | 'v')) => self.copy_command(c),
            _ => {}
        }
        EventResult::Consumed
    }

    /// Open the `:` prompt with `text` already typed.
    fn open_prompt(&mut self, text: &str) {
        self.command_mode = true;
        self.command_input = text.to_string();
    }

    /// Keys while the `:` prompt is open.
    fn handle_command_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.command_mode = false;
                self.command_input.clear();
            }
            Key::Enter => {
                let cmd = std::mem::take(&mut self.command_input);
                self.command_mode = false;
                self.process_command(&cmd);
            }
            Key::Backspace => {
                // Never past the `:` itself: the prompt is the mode indicator,
                // and a prompt that can be deleted leaves the user typing into
                // an empty line with no way to tell what it is.
                if self.command_input.chars().count() > 1 {
                    self.command_input.pop();
                }
            }
            _ => {
                let typed: String = key.typed().collect();
                if typed.is_empty() {
                    return EventResult::Ignored;
                }
                self.command_input.push_str(&typed);
            }
        }
        EventResult::Consumed
    }

    /// Keys while the session or window chooser is open. Modal: a key that
    /// fell through to the shell behind the list would type somewhere the
    /// user cannot see.
    fn handle_chooser_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.session_chooser = false;
                self.window_chooser = false;
            }
            Key::Down if self.window_chooser => self.next_window(),
            Key::Up if self.window_chooser => self.prev_window(),
            Key::Down => self.next_session(),
            Key::Up => self.prev_session(),
            Key::Enter => self.choose(),
            _ => {}
        }
        EventResult::Consumed
    }

    /// Enter in a chooser: the highlighted row is the one wanted.
    fn choose(&mut self) {
        if self.session_chooser {
            self.attach(self.active_session);
        }
        self.session_chooser = false;
        self.window_chooser = false;
    }

    /// Ask before closing the active pane.
    fn ask_close_pane(&mut self) {
        if let Some(id) = self.active_pane_id() {
            self.confirm = Some(Confirm::ClosePane(id));
        }
    }

    /// Ask before closing the active window.
    fn ask_close_window(&mut self) {
        if let Some(id) = self.active_window().map(|w| w.id) {
            self.confirm = Some(Confirm::CloseWindow(id));
        }
    }

    /// The user said yes.
    fn confirmed(&mut self, confirm: Confirm) {
        match confirm {
            Confirm::ClosePane(id) => self.remove_pane(id),
            Confirm::CloseWindow(id) => self.close_window(id),
        }
    }

    /// `prefix Space`: the next layout, by name on the status bar.
    fn next_layout(&mut self) {
        if let Some(next) = self.active_window().map(|w| w.preset.next()) {
            self.apply_layout(next);
            self.set_status(&format!("layout: {}", next.name()));
        }
    }

    /// Process a command string (from the `:` prompt).
    ///
    /// tmux's names and tmux's meanings. `split-window -h` splits side by
    /// side, and with no flag one above the other -- it did the opposite of
    /// both. `attach` and `kill-session` take a session's number or name and
    /// mean the current session with neither: `:attach` alone, the obvious
    /// thing to type on the detached screen, did nothing.
    fn process_command(&mut self, cmd: &str) {
        let cmd = cmd.trim_start_matches(':').trim();
        let (command, arg) = cmd.split_once(' ').unwrap_or((cmd, ""));
        let arg = arg.trim();

        match command {
            "new-session" | "new" => {
                let name = if arg.is_empty() {
                    self.unused_session_name()
                } else {
                    arg.to_string()
                };
                self.new_session(&name);
            }
            "kill-session" | "kill" => match self.session_or_current(arg) {
                Some(index) => self.kill_session(index),
                None => self.set_status(&format!("no session {arg}")),
            },
            "attach" | "attach-session" => match self.session_or_current(arg) {
                Some(index) => self.attach(index),
                None => self.set_status(&format!("no session {arg}")),
            },
            "detach" | "detach-client" => self.detach(),
            "rename-session" | "rename" => {
                if arg.is_empty() {
                    self.set_status("a session needs a name");
                } else if self.sessions.iter().any(|s| s.name == arg) {
                    self.set_status(&format!("duplicate session: {arg}"));
                } else if let Some(session) = self.active_session_mut() {
                    session.name = arg.to_string();
                }
            }
            "rename-window" | "renamew" => self.rename_window(arg),
            "new-window" | "neww" => self.new_window(),
            "kill-window" | "killw" => {
                if let Some(id) = self.active_window().map(|w| w.id) {
                    self.close_window(id);
                }
            }
            "select-window" | "selectw" => match arg.trim_start_matches("-t").trim().parse() {
                Ok(n) => self.select_window_number(n),
                Err(_) => self.set_status("select-window takes a window number"),
            },
            "split-window" | "splitw" => {
                let flags = arg.split_whitespace();
                if flags.clone().any(|f| f == "-h") {
                    self.split_pane(SplitDir::SideBySide);
                } else {
                    self.split_pane(SplitDir::Stacked);
                }
            }
            "kill-pane" | "killp" => {
                if let Some(id) = self.active_pane_id() {
                    self.remove_pane(id);
                }
            }
            "select-layout" | "selectl" | "layout" => match LayoutPreset::by_name(arg) {
                Some(preset) => self.apply_layout(preset),
                None => self.set_status(&format!("Unknown layout: {arg}")),
            },
            "list-sessions" | "ls" => {
                let msg = self
                    .sessions
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let here = i == self.active_session && !self.detached;
                        format!(
                            "{i}: {} ({} windows{})",
                            s.name,
                            s.windows.len(),
                            if here { ", attached" } else { "" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                self.set_status(&msg);
            }
            "" => {}
            _ => self.set_status(&format!("Unknown command: {command}")),
        }
    }

    /// A session named by the user, or the current one when nothing is named.
    fn session_or_current(&self, arg: &str) -> Option<usize> {
        let arg = arg.trim_start_matches("-t").trim();
        if arg.is_empty() {
            (self.active_session < self.sessions.len()).then_some(self.active_session)
        } else {
            self.session_named(arg)
        }
    }

    // ========================================================================
    // The pointer
    // ========================================================================

    /// Handle a pointer event.
    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                self.relayout();
                let size = (self.window_width, self.window_height);
                match self.frame_at(size).hit_test(event.x, event.y) {
                    Some(target) => {
                        self.press(target, event);
                        EventResult::Consumed
                    }
                    None => EventResult::Ignored,
                }
            }
            MouseEventKind::Move | MouseEventKind::Release(MouseButton::Left) => {
                // A selection's drag belongs to the pane it started in,
                // wherever the pointer has wandered since.
                let Some(id) = self.drag else {
                    return EventResult::Ignored;
                };
                if matches!(event.kind, MouseEventKind::Release(_)) {
                    self.drag = None;
                }
                self.forward_mouse(id, event);
                EventResult::Consumed
            }
            MouseEventKind::Scroll { dy, .. } => self.wheel(event, *dy),
            _ => EventResult::Ignored,
        }
    }

    /// A left press on `target`.
    fn press(&mut self, target: Target, event: &MouseEvent) {
        match target {
            Target::HelpCard => self.show_help = false,
            Target::Help => self.show_help = true,
            Target::Tab(id) | Target::StatusWindow(id) => self.select_window_id(id),
            Target::NewWindow => self.new_window(),
            Target::PaneTitle(id) => self.select_pane(id),
            Target::Pane(id, part) => {
                self.select_pane(id);
                if part == TermTarget::Grid {
                    // A press in the grid starts a selection with the pointer,
                    // which replaces one `v` was making.
                    if let Some(pane) = self.find_pane_mut(id) {
                        pane.marking = false;
                    }
                    self.drag = Some(id);
                }
                self.forward_mouse(id, event);
            }
            Target::SessionName => {
                self.session_chooser = !self.session_chooser;
                self.window_chooser = false;
            }
            Target::SessionRow(sid) => {
                if let Some(at) = self.sessions.iter().position(|s| s.id == sid) {
                    self.attach(at);
                }
                self.session_chooser = false;
            }
            Target::WindowRow(id) => {
                self.select_window_id(id);
                self.window_chooser = false;
            }
            Target::Scrim => {
                self.session_chooser = false;
                self.window_chooser = false;
            }
            Target::ChooserBox => {}
            Target::Attach => self.attach(self.active_session),
            Target::ChooseSession => {
                self.session_chooser = true;
                self.window_chooser = false;
            }
            Target::ConfirmYes => {
                if let Some(confirm) = self.confirm.take() {
                    self.confirmed(confirm);
                }
            }
            Target::ConfirmNo => {
                self.confirm = None;
                self.set_status("not closed");
            }
        }
    }

    /// The wheel: through a chooser's list, or the scrollback of the pane
    /// under the pointer.
    fn wheel(&mut self, event: &MouseEvent, dy: f32) -> EventResult {
        if self.show_help || self.confirm.is_some() {
            return EventResult::Ignored;
        }
        if self.session_chooser || self.window_chooser {
            // One row a notch: a chooser's rows are big targets, and three a
            // notch skips past the one being looked for.
            let rows = self.chooser_wheel.rows_at(dy, 1.0);
            for _ in 0..rows.unsigned_abs() {
                match (self.window_chooser, rows > 0) {
                    (true, true) => self.next_window(),
                    (true, false) => self.prev_window(),
                    (false, true) => self.next_session(),
                    (false, false) => self.prev_session(),
                }
            }
            return EventResult::Consumed;
        }
        self.relayout();
        let size = (self.window_width, self.window_height);
        match self.frame_at(size).hit_test(event.x, event.y) {
            Some(Target::Pane(id, _)) => {
                self.forward_mouse(id, event);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Hand a pointer event to a pane's terminal, in the terminal's own
    /// coordinates -- its selection, its scrollback bar and its wheel are its
    /// own business.
    fn forward_mouse(&mut self, id: PaneId, event: &MouseEvent) {
        let Some(rect) = self.pane_rect(id) else {
            return;
        };
        let content = pane_content(rect);
        if let Some(pane) = self.find_pane_mut(id) {
            pane.term.handle_event(&Event::Mouse(MouseEvent {
                x: event.x - content.x,
                y: event.y - content.y,
                kind: event.kind.clone(),
            }));
        }
    }

    // ========================================================================
    // Drawing
    // ========================================================================

    /// The whole window at `size`, with every control's hit box.
    ///
    /// Drawing and hit-testing are one walk (`guitk::frame`), so a control can
    /// only be clicked where it is drawn. None of this window's controls could
    /// be clicked before: it drew into a bare list of commands and recorded no
    /// targets at all.
    fn frame_at(&self, (width, height): (f32, f32)) -> Frame<Target> {
        let size = (width.max(0.0), height.max(0.0));
        let window = Rect::new(0.0, 0.0, size.0, size.1);
        let mut f = Frame::new(size.0, size.1);
        f.clip(window);
        self.palette
            .push_surface(&mut f, 0.0, 0.0, size.0, size.1, 0.0, Surface::Card);

        match self.active_session() {
            Some(session) if !self.detached => {
                self.draw_tab_bar(&mut f, session, size.0);
                if let Some(active) = session.active_window() {
                    self.draw_panes(&mut f, active, size);
                }
                self.draw_bottom(&mut f, Some(session), size);
                if self.window_chooser {
                    self.draw_window_chooser(&mut f, session, size);
                }
            }
            _ => {
                self.draw_detached(&mut f, size);
                self.draw_bottom(&mut f, None, size);
            }
        }
        if self.session_chooser {
            self.draw_session_chooser(&mut f, size);
        }
        if self.prefix_state == PrefixState::Prefix {
            self.draw_prefix_indicator(&mut f, size.0);
        }
        if self.show_help {
            // Modal: nothing behind the card can be clicked.
            f.discard_hits();
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                size,
                0.0,
                SHORTCUTS,
                "F1 closes this. The rest follow Ctrl+B -- in copy mode, its keys work alone.",
            );
            f.hit(Target::HelpCard, window);
        }
        f.unclip();
        f
    }

    /// The tab bar: a tab per window, a button for a new one, and one for the
    /// list of keys.
    fn draw_tab_bar(&self, f: &mut Frame<Target>, session: &Session, width: f32) {
        self.palette.push_surface(
            f,
            0.0,
            0.0,
            width,
            TAB_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        let keys = "F1 Keys";
        let keys_w = text::width(keys, SMALL_TEXT) + 16.0;
        let keys_rect = Rect::new(
            (width - keys_w - PADDING).max(0.0),
            4.0,
            keys_w,
            TAB_BAR_HEIGHT - 8.0,
        );
        self.button(f, keys_rect, keys, Target::Help);

        // The tabs, in the room left of the new-window button. When they do
        // not all fit, the row is slid along so the active one is in view --
        // a session can hold thirty-two windows, and a tab drawn past the
        // window's edge is a window that cannot be reached with the pointer.
        let plus_w = 24.0;
        let room = (keys_rect.x - PADDING * 2.0 - plus_w - TAB_GAP).max(0.0);
        let tabs: Vec<(WindowId, String, f32)> = session
            .windows
            .iter()
            .map(|w| {
                let label = format!("{}:{}", w.index, w.name);
                let tab_w = text::width(&label, SMALL_TEXT) + 24.0;
                (w.id, label, tab_w)
            })
            .collect();
        let active_end: f32 = tabs
            .iter()
            .take(session.active_window.saturating_add(1))
            .map(|(_, _, w)| w + TAB_GAP)
            .sum();
        let slide = (active_end - room).max(0.0);

        f.clip(Rect::new(PADDING, 0.0, room, TAB_BAR_HEIGHT));
        let mut x = PADDING - slide;
        for (i, (id, label, tab_w)) in tabs.iter().enumerate() {
            let active = i == session.active_window;
            let tab = Rect::new(x, 2.0, *tab_w, TAB_BAR_HEIGHT - 2.0);
            f.push(RenderCommand::FillRect {
                x: tab.x,
                y: tab.y,
                width: tab.w,
                height: tab.h,
                color: if active {
                    self.palette.surface0
                } else {
                    self.palette.mantle
                },
                corner_radii: CornerRadii {
                    top_left: 4.0,
                    top_right: 4.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });
            if active {
                fill(f, Rect::new(tab.x, tab.y, tab.w, 2.0), self.palette.blue);
            }
            let (color, weight) = if active {
                (self.palette.text, FontWeightHint::Bold)
            } else {
                (self.palette.subtext0, FontWeightHint::Regular)
            };
            label_at(
                f,
                (x + 12.0, 8.0),
                label,
                (SMALL_TEXT, weight),
                color,
                tab_w - 16.0,
            );
            f.hit(Target::Tab(*id), tab);
            x += tab_w + TAB_GAP;
        }
        f.unclip();

        let plus_x = x.min(PADDING + room) + TAB_GAP;
        self.button(
            f,
            Rect::new(plus_x, 4.0, plus_w, TAB_BAR_HEIGHT - 8.0),
            "+",
            Target::NewWindow,
        );
    }

    /// A small labelled button.
    fn button(&self, f: &mut Frame<Target>, rect: Rect, label: &str, target: Target) {
        self.palette.push_surface(
            f,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            4.0,
            Surface::ControlTrack,
        );
        let label_w = text::width(label, SMALL_TEXT);
        label_at(
            f,
            (
                rect.x + ((rect.w - label_w) / 2.0).max(0.0),
                rect.y + (rect.h - SMALL_TEXT) / 2.0 - 1.0,
            ),
            label,
            (SMALL_TEXT, FontWeightHint::Regular),
            self.palette.text,
            rect.w,
        );
        f.hit(target, rect);
    }

    /// Every pane of the window showing.
    fn draw_panes(&self, f: &mut Frame<Target>, window: &Window, (width, height): (f32, f32)) {
        for (id, rect) in window.bounds(width, height) {
            if let Some(pane) = self.find_pane(id) {
                self.draw_pane(f, pane, rect, id == window.active_pane);
            }
        }
    }

    /// One pane: its title strip, its terminal, its border.
    ///
    /// The terminal draws itself -- `TerminalState::frame`, the terminal app's
    /// own drawing -- and is placed in the pane by a translation, with its hit
    /// boxes carried over under this window's `Target::Pane`. So a pane's
    /// grid, its selection and its scrollback bar are the terminal's, drawn
    /// and clicked exactly as they are in the terminal's own window.
    fn draw_pane(&self, f: &mut Frame<Target>, pane: &Pane, rect: Rect, active: bool) {
        fill(f, rect, self.palette.base);
        // The whole pane, under everything else in it: a click on its border
        // makes it the active pane. The title strip is recorded again, on top,
        // below.
        f.hit(Target::PaneTitle(pane.id), rect);

        let strip = Rect::new(rect.x, rect.y, rect.w, PANE_TITLE_HEIGHT.min(rect.h));
        fill(
            f,
            strip,
            if active {
                self.palette.surface0
            } else {
                self.palette.mantle
            },
        );
        let mut title_room = (rect.w - 12.0).max(0.0);
        if pane.copy_mode {
            // How far back the view is, not just that copy mode is on:
            // scrolling through lines that repeat otherwise looks exactly like
            // a key doing nothing.
            let badge = format!("[COPY -{}]", pane.term.scroll_offset);
            let badge_w = text::measure(&badge, SMALL_TEXT, FontWeightHint::Bold) + 8.0;
            let badge_rect = Rect::new(
                (rect.right() - badge_w).max(rect.x),
                rect.y,
                badge_w.min(rect.w),
                strip.h,
            );
            fill(f, badge_rect, self.palette.yellow);
            label_at(
                f,
                (badge_rect.x + 4.0, rect.y + 2.0),
                &badge,
                (SMALL_TEXT, FontWeightHint::Bold),
                self.palette.crust,
                badge_rect.w,
            );
            title_room = (title_room - badge_w).max(0.0);
        }
        label_at(
            f,
            (rect.x + 6.0, rect.y + 2.0),
            &text::elide(
                pane.title(),
                title_room,
                "...",
                SMALL_TEXT,
                FontWeightHint::Regular,
            ),
            (SMALL_TEXT, FontWeightHint::Regular),
            if active {
                self.palette.text
            } else {
                self.palette.subtext0
            },
            title_room,
        );

        let content = pane_content(rect);
        if !content.is_empty() {
            let inner = pane.term.frame(content.w, content.h);
            f.translate(content.x, content.y);
            f.clip(Rect::new(0.0, 0.0, content.w, content.h));
            f.extend(inner.commands().iter().cloned());
            for (part, r) in inner.hits() {
                f.hit(Target::Pane(pane.id, *part), *r);
            }
            f.unclip();
            f.untranslate();
        }

        // The title strip over the terminal's own hit boxes, so that it is the
        // copy of this target a click finds first.
        f.hit(Target::PaneTitle(pane.id), strip);

        // The border last, over the edges of everything inside it.
        f.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if active {
                self.palette.blue
            } else {
                self.palette.surface1
            },
            line_width: PANE_BORDER_WIDTH,
            corner_radii: CornerRadii::ZERO,
        });
    }

    /// The bottom row: the status bar -- or, in its place, the prompt or a
    /// question waiting for its answer.
    fn draw_bottom(&self, f: &mut Frame<Target>, session: Option<&Session>, size: (f32, f32)) {
        if self.command_mode {
            self.draw_prompt(f, size);
        } else if let Some(confirm) = self.confirm {
            self.draw_confirm(f, confirm, size);
        } else if let Some(session) = session {
            self.draw_status_bar(f, session, size);
        }
    }

    /// The status bar: the session, its windows, and the time or a message.
    fn draw_status_bar(
        &self,
        f: &mut Frame<Target>,
        session: &Session,
        (width, height): (f32, f32),
    ) {
        let y = height - STATUS_BAR_HEIGHT;
        fill(
            f,
            Rect::new(0.0, y, width, STATUS_BAR_HEIGHT),
            self.palette.green,
        );
        let ink = self.palette.crust;

        // Right: measured first, so the window list knows where to stop.
        let right = self.status_text();
        let right_w = text::width(&right, SMALL_TEXT).min(width * 0.5);
        let right_x = (width - PADDING - right_w).max(0.0);
        label_at(
            f,
            (right_x, y + 4.0),
            &right,
            (SMALL_TEXT, FontWeightHint::Regular),
            ink,
            right_w + 1.0,
        );

        // Left: the session's name, which opens the session chooser.
        let name = format!("[{}]", session.name);
        let name_w = text::measure(&name, SMALL_TEXT, FontWeightHint::Bold).min(200.0);
        label_at(
            f,
            (PADDING, y + 4.0),
            &name,
            (SMALL_TEXT, FontWeightHint::Bold),
            ink,
            name_w + 1.0,
        );
        f.hit(
            Target::SessionName,
            Rect::new(PADDING, y, name_w, STATUS_BAR_HEIGHT),
        );

        // The windows, as many as there is room for, each one a click away.
        let end = right_x - 16.0;
        let mut x = PADDING + name_w + 16.0;
        for (i, window) in session.windows.iter().enumerate() {
            let active = i == session.active_window;
            let entry = format!(
                "{}:{}{}",
                window.index,
                window.name,
                if active { "*" } else { "" }
            );
            let weight = if active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };
            let entry_w = text::measure(&entry, SMALL_TEXT, weight);
            if x + entry_w > end {
                if x + 12.0 <= end {
                    label_at(
                        f,
                        (x, y + 4.0),
                        "...",
                        (SMALL_TEXT, FontWeightHint::Regular),
                        ink,
                        12.0,
                    );
                }
                break;
            }
            label_at(
                f,
                (x, y + 4.0),
                &entry,
                (SMALL_TEXT, weight),
                ink,
                entry_w + 1.0,
            );
            f.hit(
                Target::StatusWindow(window.id),
                Rect::new(x, y, entry_w, STATUS_BAR_HEIGHT),
            );
            x += entry_w + 12.0;
        }
    }

    /// The `:` prompt, in the status bar's place.
    fn draw_prompt(&self, f: &mut Frame<Target>, (width, height): (f32, f32)) {
        let y = height - STATUS_BAR_HEIGHT;
        self.palette.push_surface(
            f,
            0.0,
            y,
            width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );
        let ink = self.palette.ink(self.palette.yellow);
        let room = (width - PADDING * 2.0).max(0.0);
        label_at(
            f,
            (PADDING, y + 4.0),
            &self.command_input,
            (SMALL_TEXT, FontWeightHint::Regular),
            ink,
            room,
        );
        // Where the next character goes.
        let caret_x = PADDING + text::width(&self.command_input, SMALL_TEXT).min(room);
        fill(
            f,
            Rect::new(caret_x + 1.0, y + 4.0, 1.5, SMALL_TEXT + 2.0),
            ink,
        );
    }

    /// A question in the status bar's place, with its two answers.
    fn draw_confirm(&self, f: &mut Frame<Target>, confirm: Confirm, (width, height): (f32, f32)) {
        let y = height - STATUS_BAR_HEIGHT;
        fill(
            f,
            Rect::new(0.0, y, width, STATUS_BAR_HEIGHT),
            self.palette.yellow,
        );
        let question = match confirm {
            Confirm::ClosePane(id) => {
                let title = self.find_pane(id).map_or("the pane", Pane::title);
                format!("Close {title} and end what is running in it? (y/n)")
            }
            Confirm::CloseWindow(id) => {
                let name = self
                    .active_session()
                    .and_then(|s| s.windows.iter().find(|w| w.id == id))
                    .map_or_else(String::new, |w| format!("{}:{}", w.index, w.name));
                format!("Close window {name} and end every shell in it? (y/n)")
            }
        };
        let ink = self.palette.crust;
        let buttons_w = 2.0 * 44.0 + PADDING;
        let room = (width - buttons_w - PADDING * 3.0).max(0.0);
        label_at(
            f,
            (PADDING, y + 4.0),
            &question,
            (SMALL_TEXT, FontWeightHint::Bold),
            ink,
            room,
        );
        let yes = Rect::new(
            width - buttons_w - PADDING,
            y + 2.0,
            44.0,
            STATUS_BAR_HEIGHT - 4.0,
        );
        let no = Rect::new(
            yes.right() + PADDING,
            y + 2.0,
            44.0,
            STATUS_BAR_HEIGHT - 4.0,
        );
        self.button(f, yes, "Yes", Target::ConfirmYes);
        self.button(f, no, "No", Target::ConfirmNo);
    }

    /// The screen of a detached window: what is still running, and the way
    /// back.
    ///
    /// It said "Use :attach or tmux attach to reconnect". There is no `tmux`
    /// command to type anywhere, and `:attach` needed a number nobody had been
    /// told.
    fn draw_detached(&self, f: &mut Frame<Target>, (width, height): (f32, f32)) {
        let name = self.active_session().map_or("", |s| s.name.as_str());
        let centre = width / 2.0;
        let mut y = height / 2.0 - 70.0;
        let lines = [
            (
                "[detached]".to_string(),
                HEADER_TEXT,
                FontWeightHint::Bold,
                self.palette.text,
            ),
            (
                format!("Session {name} is still running, and so are the shells in it."),
                NORMAL_TEXT,
                FontWeightHint::Regular,
                self.palette.subtext0,
            ),
            (
                "Closing this window ends every session.".to_string(),
                SMALL_TEXT,
                FontWeightHint::Regular,
                self.palette.subtext0,
            ),
        ];
        for (line, size, weight, color) in &lines {
            let w = text::measure(line, *size, *weight).min(width);
            label_at(
                f,
                ((centre - w / 2.0).max(0.0), y),
                line,
                (*size, *weight),
                *color,
                w + 1.0,
            );
            y += size + 12.0;
        }
        y += 8.0;
        let attach = "Attach (Enter)";
        let attach_w = text::width(attach, SMALL_TEXT) + 24.0;
        let others = self.sessions.len() > 1;
        let sessions = format!("Sessions ({})", self.sessions.len());
        let sessions_w = text::width(&sessions, SMALL_TEXT) + 24.0;
        let total = attach_w + if others { sessions_w + 12.0 } else { 0.0 };
        let x = (centre - total / 2.0).max(0.0);
        self.button(f, Rect::new(x, y, attach_w, 26.0), attach, Target::Attach);
        if others {
            self.button(
                f,
                Rect::new(x + attach_w + 12.0, y, sessions_w, 26.0),
                &sessions,
                Target::ChooseSession,
            );
        }
    }

    /// The box a chooser of `rows` rows sits in, and how many of its rows fit.
    fn chooser_box(rows: usize, (width, height): (f32, f32)) -> (Rect, usize) {
        let want = rows as f32 * CHOOSER_ROW + CHOOSER_TOP + 8.0;
        let h = want.min(CHOOSER_MAX_HEIGHT).min((height - 16.0).max(0.0));
        let w = CHOOSER_WIDTH.min((width - 16.0).max(0.0));
        let x = ((width - w) / 2.0).max(0.0);
        let y = ((height - h) / 2.0).max(0.0);
        let fits = ((h - CHOOSER_TOP - 8.0) / CHOOSER_ROW).floor().max(0.0) as usize;
        (Rect::new(x, y, w, h), fits.max(1))
    }

    /// A chooser: a scrim that closes it, a box, a heading and rows.
    ///
    /// The rows that fit, slid so the highlighted one is among them. Every
    /// row used to be drawn, at the same pitch, whatever the box's height --
    /// the sixty-fourth session sat half a screen below a box four hundred
    /// pixels tall.
    fn draw_chooser(
        &self,
        f: &mut Frame<Target>,
        heading: &str,
        rows: &[(Target, String)],
        selected: usize,
        size: (f32, f32),
    ) {
        fill(
            f,
            Rect::new(0.0, 0.0, size.0, size.1),
            Color::rgba(0, 0, 0, 100),
        );
        let (area, fits) = Self::chooser_box(rows.len(), size);
        // The scrim is the four bands around the box, so that each of its hit
        // boxes is somewhere a click on it lands on it -- one box under the
        // whole window would have its middle covered by the chooser.
        for band in [
            Rect::new(0.0, 0.0, size.0, area.y),
            Rect::new(
                0.0,
                area.bottom(),
                size.0,
                (size.1 - area.bottom()).max(0.0),
            ),
            Rect::new(0.0, area.y, area.x, area.h),
            Rect::new(
                area.right(),
                area.y,
                (size.0 - area.right()).max(0.0),
                area.h,
            ),
        ] {
            f.hit(Target::Scrim, band);
        }
        f.push(RenderCommand::FillRect {
            x: area.x,
            y: area.y,
            width: area.w,
            height: area.h,
            color: self.palette.base,
            corner_radii: CornerRadii::all(8.0),
        });
        f.push(RenderCommand::StrokeRect {
            x: area.x,
            y: area.y,
            width: area.w,
            height: area.h,
            color: self.palette.surface1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });
        // The box itself takes a click that misses every row, rather than
        // passing it to the scrim and closing the list under the pointer.
        f.hit(Target::ChooserBox, area);
        label_at(
            f,
            (area.x + 12.0, area.y + 8.0),
            heading,
            (HEADER_TEXT, FontWeightHint::Bold),
            self.palette.text,
            area.w - 24.0,
        );

        let first = selected.saturating_add(1).saturating_sub(fits);
        f.clip(area);
        let mut row_y = area.y + CHOOSER_TOP;
        for (i, (target, row)) in rows.iter().enumerate().skip(first).take(fits) {
            let active = i == selected;
            let rect = Rect::new(area.x + 4.0, row_y, area.w - 8.0, CHOOSER_ROW - 2.0);
            if active {
                self.palette.push_surface(
                    f,
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    4.0,
                    Surface::Selected,
                );
            }
            label_at(
                f,
                (area.x + 12.0, row_y + 4.0),
                row,
                (
                    SMALL_TEXT,
                    if active {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                ),
                if active {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
                area.w - 24.0,
            );
            f.hit(*target, rect);
            row_y += CHOOSER_ROW;
        }
        f.unclip();
    }

    /// `prefix s`: every session, the current one highlighted.
    fn draw_session_chooser(&self, f: &mut Frame<Target>, size: (f32, f32)) {
        let rows: Vec<(Target, String)> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let here = i == self.active_session && !self.detached;
                (
                    Target::SessionRow(s.id),
                    format!(
                        "{i}: {} ({} windows{})",
                        s.name,
                        s.windows.len(),
                        if here { ", attached" } else { "" }
                    ),
                )
            })
            .collect();
        self.draw_chooser(f, "Sessions", &rows, self.active_session, size);
    }

    /// `prefix w`: the session's windows, the current one highlighted.
    fn draw_window_chooser(&self, f: &mut Frame<Target>, session: &Session, size: (f32, f32)) {
        let rows: Vec<(Target, String)> = session
            .windows
            .iter()
            .map(|w| {
                (
                    Target::WindowRow(w.id),
                    format!("{}: {} ({} panes)", w.index, w.name, w.layout.pane_count()),
                )
            })
            .collect();
        self.draw_chooser(f, "Windows", &rows, session.active_window, size);
    }

    /// While the prefix is armed: that it is, and where the keys are listed.
    fn draw_prefix_indicator(&self, f: &mut Frame<Target>, width: f32) {
        let hint = "Ctrl+B: now a key (? lists them)";
        let w = text::measure(hint, SMALL_TEXT, FontWeightHint::Bold) + 16.0;
        let rect = Rect::new((width - w).max(0.0), TAB_BAR_HEIGHT, w, 20.0);
        fill(f, rect, self.palette.yellow);
        label_at(
            f,
            (rect.x + 8.0, rect.y + 3.0),
            hint,
            (SMALL_TEXT, FontWeightHint::Bold),
            self.palette.crust,
            w,
        );
    }
}

/// One filled rectangle.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::ZERO,
    });
}

/// One line of text, cut with an ellipsis at `max_w`.
fn label_at(
    f: &mut Frame<Target>,
    (x, y): (f32, f32),
    s: &str,
    (size, weight): (f32, FontWeightHint),
    color: Color,
    max_w: f32,
) {
    f.push(RenderCommand::Text {
        x,
        y,
        text: s.to_string(),
        font_size: size,
        color,
        font_weight: weight,
        max_width: Some(max_w.max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
}

// ============================================================================
// The window
// ============================================================================

impl App for Multiplexer {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
        for pane in &mut self.panes {
            pane.term.theme_changed(palette);
        }
    }

    fn title(&self) -> String {
        // The session and the window inside it, which is what a multiplexer's
        // title bar is for -- the same two names its own status bar shows.
        match self.active_session() {
            Some(session) => {
                let window = session
                    .active_window()
                    .map_or(String::new(), |w| format!("{}:{}", w.index, w.name));
                format!("{}:{} - tmux", session.name, window)
            }
            None => "tmux".to_string(),
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// As often as the busiest pane needs -- its shell talking, its cursor
    /// blinking -- and otherwise when the status bar next changes by itself.
    ///
    /// It was a flat second, for a clock that counted the seconds the window
    /// had been open.
    fn tick_interval(&self) -> Option<Duration> {
        let panes = self
            .panes
            .iter()
            .filter_map(|p| App::tick_interval(&p.term))
            .min();
        let status = Duration::from_millis(self.status_wake_ms());
        Some(panes.map_or(status, |p| p.min(status)))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            // Every shell is told its terminal is gone, rather than left
            // running against a pseudo-terminal nobody reads.
            for pane in &mut self.panes {
                pane.term.hang_up();
            }
            return Response::Exit;
        }
        let result = self.handle_event(event);
        if self.quit {
            return Response::Exit;
        }
        match result {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // From the frame being drawn rather than the last `Resize`: `relayout`
        // turns these pixels into each terminal's columns and rows, so a stale
        // pair is a pane that wraps its text at a width it is not drawn at.
        self.window_width = width;
        self.window_height = height;
        self.relayout();
        self.frame_at((width, height)).into_tree()
    }
}

impl Probe for Multiplexer {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame_at(size)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
        self.window_width = size.0;
        self.window_height = size.1;
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.window_width = size.0;
        self.window_height = size.1;
        self.relayout();
        self.handle_event(&Event::Key(key.clone()))
    }

    fn scroll_at(&mut self, x: f32, y: f32, dy: f32, size: (f32, f32)) -> Option<EventResult> {
        self.window_width = size.0;
        self.window_height = size.1;
        Some(self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })))
    }
}

fn main() -> ExitCode {
    // The first shell starts here, before the window and its threads exist;
    // later panes' shells start with threads running, which `libcall::pty`'s
    // spawn is built for -- the child does nothing but exec.
    let mut mux = Multiplexer::with_shells(Box::new(terminal::child::spawn_shell));
    app::launch("tmux", &mut mux)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point of a test, so the defensive lints the
    // workspace applies to production code are lifted here.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::probe::{self, rect_of, scroll_at_point};
    use std::cell::RefCell;
    use std::rc::Rc;
    use terminal::child::Exit;
    use terminal::child::script::{Script, ScriptLink};

    // -- helpers --

    /// Every shell the multiplexer started, in the order it started them.
    type Shells = Rc<RefCell<Vec<Rc<RefCell<Script>>>>>;

    /// A multiplexer whose shells are scripts, and the scripts.
    fn scripted() -> (Multiplexer, Shells) {
        let shells: Shells = Rc::default();
        let log = Rc::clone(&shells);
        let spawner: Spawner = Box::new(move |_size| {
            let (link, script) = ScriptLink::new();
            log.borrow_mut().push(script);
            Ok(Box::new(link) as Box<dyn Link>)
        });
        (Multiplexer::with_shells(spawner), shells)
    }

    /// The `n`th shell started.
    fn shell(shells: &Shells, n: usize) -> Rc<RefCell<Script>> {
        Rc::clone(&shells.borrow()[n])
    }

    fn key_ev(key: Key, text: &str, ctrl: bool) -> Event {
        let mut modifiers = guitk::event::Modifiers::NONE;
        modifiers.ctrl = ctrl;
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: text.to_string(),
        })
    }

    fn press(k: Key) -> Event {
        key_ev(k, "", false)
    }

    fn types(c: char) -> Event {
        key_ev(Key::A, &c.to_string(), false)
    }

    fn type_str(mux: &mut Multiplexer, s: &str) {
        for c in s.chars() {
            mux.handle_event(&types(c));
        }
    }

    /// Ctrl+B, then the key.
    fn prefixed(mux: &mut Multiplexer, c: char) {
        mux.handle_event(&key_ev(Key::B, "", true));
        assert_eq!(mux.prefix_state, PrefixState::Prefix, "Ctrl+B did not arm");
        mux.handle_event(&types(c));
    }

    fn tick(mux: &mut Multiplexer) {
        mux.handle_event(&Event::Tick { elapsed_ms: 20 });
    }

    fn active_pane(mux: &Multiplexer) -> &Pane {
        let id = mux.active_pane_id().expect("an active pane");
        mux.find_pane(id).expect("the active pane exists")
    }

    fn panes_in_active_window(mux: &Multiplexer) -> usize {
        mux.active_window().map_or(0, |w| w.layout.pane_count())
    }

    fn windows(mux: &Multiplexer) -> usize {
        mux.active_session().map_or(0, |s| s.windows.len())
    }

    /// A pane's screen, row by row, trailing blanks trimmed.
    fn screen(mux: &Multiplexer, id: PaneId) -> String {
        let term = &mux.find_pane(id).expect("the pane").term;
        (0..term.rows())
            .filter_map(|r| term.line_at(term.buffer_row_of(r)))
            .map(|l| {
                l.cells
                    .iter()
                    .map(|c| c.ch)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Every string the frame draws.
    fn drawn_texts(mux: &Multiplexer) -> Vec<String> {
        mux.frame_at((mux.window_width, mux.window_height))
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn feed_lines(shells: &Shells, n: usize, count: usize) {
        let script = shell(shells, n);
        for i in 0..count {
            script
                .borrow_mut()
                .pending
                .extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
    }

    // -- shells --

    #[test]
    fn every_pane_gets_a_shell_of_its_own() {
        let (mut mux, shells) = scripted();
        assert_eq!(shells.borrow().len(), 1, "the first pane's shell");
        prefixed(&mut mux, '%');
        prefixed(&mut mux, 'c');
        mux.process_command(":new-session two");
        assert_eq!(shells.borrow().len(), 4);
        assert_eq!(mux.panes.len(), 4);
        assert!(mux.panes.iter().all(|p| p.term.child_is_live()));
    }

    #[test]
    fn a_shell_is_told_the_size_its_pane_is_drawn_at() {
        let (mut mux, shells) = scripted();
        let first = mux.active_pane_id().unwrap();
        let born = shell(&shells, 0).borrow().size;
        assert_eq!(born, Some(mux.find_pane(first).unwrap().term.win_size()));

        prefixed(&mut mux, '%');
        let now = shell(&shells, 0).borrow().size;
        assert_ne!(
            now, born,
            "the split halved the pane and its shell was not told"
        );
        assert_eq!(now, Some(mux.find_pane(first).unwrap().term.win_size()));
        let cols = |s: Option<WinSize>| s.map_or(0, |s| s.cols);
        assert!(cols(now) < cols(born));
    }

    #[test]
    fn typing_reaches_the_active_panes_shell() {
        // Every key that was not a multiplexer command was dropped: there was
        // nowhere for it to go.
        let (mut mux, shells) = scripted();
        type_str(&mut mux, "ls");
        mux.handle_event(&press(Key::Enter));
        assert_eq!(shell(&shells, 0).borrow().sent, b"ls\r");

        prefixed(&mut mux, '%');
        mux.handle_event(&types('x'));
        assert_eq!(
            shell(&shells, 1).borrow().sent,
            b"x",
            "the new pane is active"
        );
        assert_eq!(
            shell(&shells, 0).borrow().sent,
            b"ls\r",
            "and the old one heard nothing"
        );
        assert_eq!(
            panes_in_active_window(&mux),
            2,
            "`x` alone is not a command"
        );
    }

    #[test]
    fn what_a_shell_writes_appears_in_its_pane() {
        let (mut mux, shells) = scripted();
        let id = mux.active_pane_id().unwrap();
        shell(&shells, 0)
            .borrow_mut()
            .pending
            .extend_from_slice(b"$ hello");
        tick(&mut mux);
        assert!(screen(&mux, id).contains("$ hello"), "{}", screen(&mux, id));
    }

    #[test]
    fn a_shell_in_a_window_not_showing_is_read_too() {
        // A shell whose output nobody reads fills its terminal's buffer and
        // stops, so a background window's is read on every tick like any.
        let (mut mux, shells) = scripted();
        let first = mux.active_pane_id().unwrap();
        prefixed(&mut mux, 'c');
        shell(&shells, 0)
            .borrow_mut()
            .pending
            .extend_from_slice(b"meanwhile");
        tick(&mut mux);
        assert!(screen(&mux, first).contains("meanwhile"));
        assert!(shell(&shells, 0).borrow().pending.is_empty());
    }

    #[test]
    fn a_shell_that_exits_takes_its_pane_with_it() {
        let (mut mux, shells) = scripted();
        prefixed(&mut mux, '%');
        let second = mux.active_pane_id().unwrap();
        shell(&shells, 1).borrow_mut().exit = Some(Exit::Code(0));
        tick(&mut mux);
        assert_eq!(panes_in_active_window(&mux), 1);
        assert!(mux.find_pane(second).is_none(), "the pane was not let go");
        assert_ne!(mux.active_pane_id(), Some(second));
        assert!(!mux.quit);
    }

    #[test]
    fn the_last_shell_to_exit_closes_the_window() {
        let (mut mux, shells) = scripted();
        shell(&shells, 0).borrow_mut().exit = Some(Exit::Code(0));
        assert!(matches!(
            mux.on_event(&Event::Tick { elapsed_ms: 20 }),
            Response::Exit
        ));
    }

    #[test]
    fn a_shell_that_fails_leaves_its_pane_to_say_how() {
        let (mut mux, shells) = scripted();
        let id = mux.active_pane_id().unwrap();
        shell(&shells, 0).borrow_mut().exit = Some(Exit::Code(2));
        assert!(!matches!(
            mux.on_event(&Event::Tick { elapsed_ms: 20 }),
            Response::Exit
        ));
        assert!(mux.find_pane(id).is_some());
        assert!(
            screen(&mux, id).contains("[the shell"),
            "{}",
            screen(&mux, id)
        );
    }

    #[test]
    fn a_shell_that_cannot_start_says_why_in_its_pane() {
        let spawner: Spawner = Box::new(|_| {
            Err(SpawnError {
                program: "/bin/sh".into(),
                errno: libcall::ENOSYS,
            })
        });
        let mux = Multiplexer::with_shells(spawner);
        let text = screen(&mux, mux.active_pane_id().unwrap());
        assert!(text.contains("No shell."), "{text}");
        assert!(text.contains("no pseudo-terminals"), "{text}");
        assert!(
            !text.contains("PTY layer"),
            "the old banner claimed the system had no PTY layer"
        );
    }

    #[test]
    fn closing_a_pane_asks_first_and_hangs_up_its_shell() {
        let (mut mux, shells) = scripted();
        prefixed(&mut mux, '%');
        prefixed(&mut mux, 'x');
        assert!(mux.confirm.is_some(), "x closed a pane without asking");
        assert!(
            drawn_texts(&mux).iter().any(|t| t.contains("(y/n)")),
            "and the question is on the screen"
        );
        mux.handle_event(&types('n'));
        assert_eq!(panes_in_active_window(&mux), 2, "no leaves it open");
        assert_eq!(shell(&shells, 1).borrow().hang_ups, 0);

        prefixed(&mut mux, 'x');
        mux.handle_event(&types('y'));
        assert_eq!(panes_in_active_window(&mux), 1);
        assert_eq!(
            shell(&shells, 1).borrow().hang_ups,
            1,
            "its shell was not told"
        );
        assert_eq!(mux.panes.len(), 1);
    }

    #[test]
    fn closing_a_window_asks_and_ends_every_shell_in_it() {
        let (mut mux, shells) = scripted();
        prefixed(&mut mux, 'c');
        prefixed(&mut mux, '%');
        prefixed(&mut mux, '&');
        mux.handle_event(&types('y'));
        assert_eq!(windows(&mux), 1);
        assert_eq!(shell(&shells, 1).borrow().hang_ups, 1);
        assert_eq!(shell(&shells, 2).borrow().hang_ups, 1);
        assert_eq!(shell(&shells, 0).borrow().hang_ups, 0);
    }

    #[test]
    fn closing_the_window_hangs_up_every_shell() {
        let (mut mux, shells) = scripted();
        prefixed(&mut mux, '%');
        mux.process_command(":new-session two");
        assert!(matches!(
            mux.on_event(&Event::CloseRequested),
            Response::Exit
        ));
        for n in 0..3 {
            assert_eq!(shell(&shells, n).borrow().hang_ups, 1, "shell {n}");
        }
    }

    #[test]
    fn killing_a_session_ends_its_shells_and_the_last_one_closes_the_window() {
        let (mut mux, shells) = scripted();
        mux.process_command(":new-session two");
        prefixed(&mut mux, '%');
        mux.process_command(":kill-session two");
        assert_eq!(mux.sessions.len(), 1);
        assert_eq!(shell(&shells, 1).borrow().hang_ups, 1);
        assert_eq!(shell(&shells, 2).borrow().hang_ups, 1);
        assert_eq!(shell(&shells, 0).borrow().hang_ups, 0);
        assert_eq!(mux.panes.len(), 1);

        mux.process_command(":kill-session");
        assert!(
            mux.quit,
            "the last session ending ends the multiplexer, as in tmux"
        );
    }

    #[test]
    fn the_prefix_twice_sends_the_program_a_ctrl_b() {
        let (mut mux, shells) = scripted();
        mux.handle_event(&key_ev(Key::B, "", true));
        mux.handle_event(&key_ev(Key::B, "", true));
        assert_eq!(shell(&shells, 0).borrow().sent, [0x02]);
        assert_eq!(mux.prefix_state, PrefixState::Normal);
    }

    #[test]
    fn a_paste_goes_to_the_program_not_onto_the_screen() {
        // It was fed through the pane's parser, so the text appeared as though
        // the program had printed it -- and nothing had typed it.
        let (mut mux, shells) = scripted();
        let id = mux.active_pane_id().unwrap();
        mux.clipboard = "echo hi\n".to_string();
        prefixed(&mut mux, ']');
        assert_eq!(shell(&shells, 0).borrow().sent, b"echo hi\r");
        assert!(!screen(&mux, id).contains("echo hi"));
    }

    #[test]
    fn a_refused_window_leaves_no_pane_or_shell_behind() {
        let (mut mux, shells) = scripted();
        for _ in 1..MAX_WINDOWS {
            prefixed(&mut mux, 'c');
        }
        assert_eq!(windows(&mux), MAX_WINDOWS);
        let (panes, started) = (mux.panes.len(), shells.borrow().len());
        prefixed(&mut mux, 'c');
        assert_eq!(windows(&mux), MAX_WINDOWS);
        assert_eq!(
            mux.panes.len(),
            panes,
            "a refused window left a pane behind"
        );
        assert_eq!(shells.borrow().len(), started, "and started a shell for it");
    }

    #[test]
    fn a_pane_too_small_to_halve_is_not_split() {
        // Each half was floored at `MIN_PANE_SIZE`, so splitting a narrow pane
        // made two that overlapped.
        let mut mux = Multiplexer::new();
        mux.window_width = 300.0;
        mux.window_height = 300.0;
        for _ in 0..8 {
            prefixed(&mut mux, '%');
        }
        assert_eq!(mux.status_message, "no room to split this pane");
        let rects: Vec<Rect> = mux
            .active_window()
            .unwrap()
            .bounds(300.0, 300.0)
            .into_iter()
            .map(|(_, r)| r)
            .collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(a.right() <= 300.01, "pane {i} runs off the window: {a:?}");
            for b in rects.iter().skip(i + 1) {
                assert!(a.intersect(*b).is_none(), "{a:?} overlaps {b:?}");
            }
        }
    }

    // -- focus --

    #[test]
    fn only_the_active_pane_has_the_keyboard() {
        let mut mux = Multiplexer::new();
        let first = mux.active_pane_id().unwrap();
        prefixed(&mut mux, '%');
        let second = mux.active_pane_id().unwrap();
        let focused = |m: &Multiplexer, id| m.find_pane(id).unwrap().term.is_focused();
        assert!(focused(&mux, second) && !focused(&mux, first));

        probe::click(&mut mux, Target::PaneTitle(first));
        assert!(focused(&mux, first) && !focused(&mux, second));

        mux.handle_event(&Event::FocusOut);
        assert!(!focused(&mux, first), "the window lost the keyboard");
        mux.handle_event(&Event::FocusIn);
        assert!(focused(&mux, first));

        prefixed(&mut mux, ':');
        assert!(!focused(&mux, first), "the prompt has the keyboard");
    }

    // -- copy mode --

    #[test]
    fn copy_mode_selects_whole_lines_and_copies_them() {
        let (mut mux, shells) = scripted();
        feed_lines(&shells, 0, 200);
        tick(&mut mux);
        prefixed(&mut mux, '[');
        assert!(active_pane(&mux).copy_mode);

        // Bare keys now: copy mode has the keyboard.
        mux.handle_event(&types('k'));
        mux.handle_event(&types('k'));
        assert_eq!(active_pane(&mux).term.scroll_offset, 2);
        mux.handle_event(&types('v'));
        for _ in 0..3 {
            mux.handle_event(&types('k'));
        }
        assert!(
            shell(&shells, 0).borrow().sent.is_empty(),
            "a key in copy mode reached the shell"
        );
        mux.handle_event(&press(Key::Enter));

        let lines: Vec<&str> = mux.clipboard.lines().collect();
        assert_eq!(lines.len(), 4, "{:?}", mux.clipboard);
        let numbers: Vec<usize> = lines
            .iter()
            .map(|l| l.trim_start_matches("line ").parse().unwrap())
            .collect();
        assert!(numbers.windows(2).all(|w| w[1] == w[0] + 1), "{numbers:?}");
        assert!(!active_pane(&mux).copy_mode, "copying leaves copy mode");
        assert_eq!(active_pane(&mux).term.scroll_offset, 0);
    }

    #[test]
    fn leaving_copy_mode_returns_to_the_live_screen() {
        let (mut mux, shells) = scripted();
        feed_lines(&shells, 0, 200);
        tick(&mut mux);
        prefixed(&mut mux, 'b');
        assert!(active_pane(&mux).copy_mode, "b enters copy mode by itself");
        assert!(active_pane(&mux).term.scroll_offset > 0);
        mux.handle_event(&press(Key::Escape));
        assert!(!active_pane(&mux).copy_mode);
        assert_eq!(active_pane(&mux).term.scroll_offset, 0);

        prefixed(&mut mux, 'f');
        assert!(
            !active_pane(&mux).copy_mode,
            "a forward key does not enter it"
        );
        prefixed(&mut mux, 'g');
        let pane = active_pane(&mux);
        assert_eq!(pane.term.viewport_top(), 0, "g goes to the oldest line");
        mux.handle_event(&types('G'));
        assert_eq!(
            active_pane(&mux).term.scroll_offset,
            0,
            "G to the live screen"
        );
    }

    #[test]
    fn what_the_pointer_selected_can_be_copied() {
        let (mut mux, shells) = scripted();
        shell(&shells, 0)
            .borrow_mut()
            .pending
            .extend_from_slice(b"alpha bravo\r\n");
        tick(&mut mux);
        let id = mux.active_pane_id().unwrap();
        let grid = rect_of(&mux, Target::Pane(id, TermTarget::Grid)).expect("the grid");
        let y = grid.y + 3.0;
        mux.handle_event(&Event::Mouse(MouseEvent {
            x: grid.x + 1.0,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        mux.handle_event(&Event::Mouse(MouseEvent {
            x: grid.x + 200.0,
            y,
            kind: MouseEventKind::Move,
        }));
        mux.handle_event(&Event::Mouse(MouseEvent {
            x: grid.x + 200.0,
            y,
            kind: MouseEventKind::Release(MouseButton::Left),
        }));
        prefixed(&mut mux, 'y');
        assert_eq!(mux.clipboard, "alpha bravo");
    }

    #[test]
    fn copying_with_nothing_selected_says_so() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '[');
        prefixed(&mut mux, 'y');
        assert!(mux.clipboard.is_empty());
        assert_eq!(mux.status_message, "nothing marked");
    }

    #[test]
    fn pasting_an_empty_clipboard_says_so() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, ']');
        assert_eq!(mux.status_message, "clipboard is empty");
    }

    // -- the pointer --

    #[test]
    fn every_control_answers_the_pointer() {
        let mut mux = Multiplexer::new();
        let first_window = mux.active_window().unwrap().id;

        probe::click(&mut mux, Target::NewWindow);
        assert_eq!(windows(&mux), 2);
        let second_window = mux.active_window().unwrap().id;

        probe::click(&mut mux, Target::Tab(first_window));
        assert_eq!(mux.active_window().unwrap().id, first_window);
        probe::click(&mut mux, Target::StatusWindow(second_window));
        assert_eq!(mux.active_window().unwrap().id, second_window);

        probe::click(&mut mux, Target::Help);
        assert!(mux.show_help);
        assert!(
            rect_of(&mux, Target::NewWindow).is_none(),
            "behind the card, nothing can be clicked"
        );
        probe::click(&mut mux, Target::HelpCard);
        assert!(!mux.show_help);

        probe::click(&mut mux, Target::SessionName);
        assert!(mux.session_chooser);
        probe::click(&mut mux, Target::ChooserBox);
        assert!(
            mux.session_chooser,
            "a click between the rows keeps it open"
        );
        probe::click(&mut mux, Target::Scrim);
        assert!(!mux.session_chooser);

        mux.process_command(":new-session two");
        let first_session = mux.sessions[0].id;
        probe::click(&mut mux, Target::SessionName);
        probe::click(&mut mux, Target::SessionRow(first_session));
        assert_eq!(mux.active_session, 0);
        assert!(!mux.session_chooser);

        prefixed(&mut mux, 'w');
        probe::click(&mut mux, Target::WindowRow(first_window));
        assert_eq!(mux.active_window().unwrap().id, first_window);
        assert!(!mux.window_chooser);

        prefixed(&mut mux, '%');
        let right = mux.active_pane_id().unwrap();
        let left = mux.active_window().unwrap().layout.pane_ids()[0];
        probe::click(&mut mux, Target::PaneTitle(left));
        assert_eq!(mux.active_pane_id(), Some(left));
        probe::click(&mut mux, Target::Pane(right, TermTarget::Grid));
        assert_eq!(
            mux.active_pane_id(),
            Some(right),
            "a click in a grid focuses it"
        );
        assert_eq!(mux.drag, Some(right), "and starts a selection there");

        prefixed(&mut mux, 'x');
        probe::click(&mut mux, Target::ConfirmNo);
        assert_eq!(panes_in_active_window(&mux), 2);
        prefixed(&mut mux, 'x');
        probe::click(&mut mux, Target::ConfirmYes);
        assert_eq!(panes_in_active_window(&mux), 1);

        prefixed(&mut mux, 'd');
        assert!(mux.detached);
        probe::click(&mut mux, Target::ChooseSession);
        assert!(mux.session_chooser);
        mux.handle_event(&press(Key::Escape));
        probe::click(&mut mux, Target::Attach);
        assert!(!mux.detached);
    }

    #[test]
    fn the_wheel_scrolls_the_pane_under_the_pointer() {
        let (mut mux, shells) = scripted();
        prefixed(&mut mux, '%');
        let ids = mux.active_window().unwrap().layout.pane_ids();
        feed_lines(&shells, 0, 200);
        feed_lines(&shells, 1, 200);
        tick(&mut mux);
        scroll_at_point(&mut mux, Target::Pane(ids[0], TermTarget::Grid), 1.0);
        assert!(mux.find_pane(ids[0]).unwrap().term.scroll_offset > 0);
        assert_eq!(mux.find_pane(ids[1]).unwrap().term.scroll_offset, 0);
    }

    #[test]
    fn the_wheel_moves_through_a_chooser() {
        let mut mux = Multiplexer::new();
        mux.process_command(":new-session two");
        mux.process_command(":new-session three");
        prefixed(&mut mux, 's');
        let before = mux.active_session;
        let row = Target::SessionRow(mux.sessions[before].id);
        scroll_at_point(&mut mux, row, 1.0);
        assert_ne!(mux.active_session, before, "one notch, one row");
    }

    #[test]
    fn a_long_list_of_sessions_stays_inside_its_box() {
        let mut mux = Multiplexer::new();
        for n in 0..40 {
            mux.process_command(&format!(":new-session s{n}"));
        }
        prefixed(&mut mux, 's');
        let frame = mux.frame_at((WINDOW_WIDTH, WINDOW_HEIGHT));
        let area = frame
            .rect_of(|t| *t == Target::ChooserBox)
            .expect("the box");
        let rows: Vec<Rect> = frame
            .hits()
            .iter()
            .filter(|(t, _)| matches!(t, Target::SessionRow(_)))
            .map(|(_, r)| *r)
            .collect();
        assert!(!rows.is_empty());
        assert!(
            rows.len() < 41,
            "every row was drawn, whatever the box's height"
        );
        for r in &rows {
            assert!(
                r.bottom() <= area.bottom() + 0.01,
                "{r:?} is below {area:?}"
            );
        }
        let current = mux.sessions[mux.active_session].id;
        assert!(
            frame
                .rect_of(|t| *t == Target::SessionRow(current))
                .is_some(),
            "the highlighted session is among the rows shown"
        );
    }

    // -- tmux's meanings --

    fn split_dir(mux: &Multiplexer) -> Option<SplitDir> {
        match &mux.active_window()?.layout {
            LayoutNode::Split { direction, .. } => Some(*direction),
            LayoutNode::Leaf(_) => None,
        }
    }

    #[test]
    fn split_window_h_splits_side_by_side_as_tmux_does() {
        let mut mux = Multiplexer::new();
        mux.process_command(":split-window -h");
        assert_eq!(split_dir(&mux), Some(SplitDir::SideBySide));

        let mut mux = Multiplexer::new();
        mux.process_command(":split-window");
        assert_eq!(
            split_dir(&mux),
            Some(SplitDir::Stacked),
            "-v is the default"
        );

        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        assert_eq!(split_dir(&mux), Some(SplitDir::SideBySide));
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '"');
        assert_eq!(split_dir(&mux), Some(SplitDir::Stacked));
    }

    #[test]
    fn the_layouts_mean_what_tmux_means_by_them() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        prefixed(&mut mux, '%');
        let rects = |m: &Multiplexer| -> Vec<Rect> {
            m.active_window()
                .unwrap()
                .bounds(WINDOW_WIDTH, WINDOW_HEIGHT)
                .into_iter()
                .map(|(_, r)| r)
                .collect()
        };
        mux.process_command(":select-layout even-horizontal");
        let r = rects(&mux);
        assert!(
            r.windows(2)
                .all(|p| p[1].x > p[0].x && (p[1].y - p[0].y).abs() < 0.01),
            "{r:?}"
        );

        mux.process_command(":select-layout even-vertical");
        let r = rects(&mux);
        assert!(
            r.windows(2)
                .all(|p| p[1].y > p[0].y && (p[1].x - p[0].x).abs() < 0.01),
            "{r:?}"
        );

        mux.process_command(":select-layout main-horizontal");
        let r = rects(&mux);
        assert!(
            r[0].w > r[1].w && r[1].y > r[0].y,
            "the main pane on top: {r:?}"
        );

        mux.process_command(":select-layout main-vertical");
        let r = rects(&mux);
        assert!(
            r[0].h > r[1].h && r[1].x > r[0].x,
            "the main pane on the left: {r:?}"
        );

        mux.process_command(":select-layout diagonal");
        assert_eq!(mux.status_message, "Unknown layout: diagonal");
    }

    #[test]
    fn prefix_space_names_the_layout_it_chose() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        prefixed(&mut mux, ' ');
        assert_eq!(mux.status_message, "layout: even-horizontal");
        assert_eq!(
            mux.active_window().unwrap().preset,
            LayoutPreset::EvenHorizontal
        );
    }

    fn pane_width(mux: &Multiplexer, id: PaneId) -> f32 {
        mux.pane_rect(id).unwrap().w
    }

    #[test]
    fn growing_a_pane_grows_it_whichever_side_it_is_on() {
        // `prefix +` moved the outermost split in its first child's favour, so
        // it shrank every pane on the right or at the bottom.
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        let right = mux.active_pane_id().unwrap();
        let before = pane_width(&mux, right);
        prefixed(&mut mux, '+');
        assert!(
            pane_width(&mux, right) > before,
            "the right-hand pane shrank"
        );

        prefixed(&mut mux, 'o');
        let left = mux.active_pane_id().unwrap();
        let before = pane_width(&mux, left);
        prefixed(&mut mux, '+');
        assert!(pane_width(&mux, left) > before);

        let mut single = Multiplexer::new();
        prefixed(&mut single, '+');
        assert_eq!(single.status_message, "no split to resize");
    }

    #[test]
    fn swapping_moves_the_pane_and_keeps_it_active() {
        // `}` cycled the active pane under a comment calling a real swap too
        // complex, and the card said "Swap this pane with the next".
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        let ids = mux.active_window().unwrap().layout.pane_ids();
        let active = mux.active_pane_id().unwrap();
        assert_eq!(active, ids[1]);
        prefixed(&mut mux, '}');
        assert_eq!(
            mux.active_window().unwrap().layout.pane_ids(),
            vec![ids[1], ids[0]]
        );
        assert_eq!(mux.active_pane_id(), Some(active));
    }

    #[test]
    fn semicolon_goes_back_to_the_pane_you_were_in() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        prefixed(&mut mux, '"');
        let ids = mux.active_window().unwrap().layout.pane_ids();
        let third = mux.active_pane_id().unwrap();
        assert_eq!(third, ids[2]);
        prefixed(&mut mux, 'o');
        assert_eq!(mux.active_pane_id(), Some(ids[0]), "o wraps to the first");
        prefixed(&mut mux, ';');
        assert_eq!(
            mux.active_pane_id(),
            Some(third),
            "; is the one before, not the previous in order"
        );
        prefixed(&mut mux, ';');
        assert_eq!(mux.active_pane_id(), Some(ids[0]));
    }

    #[test]
    fn a_window_is_reached_by_the_number_on_its_tab() {
        // The digits chose by position and the tabs showed a creation count:
        // once a window had closed, prefix 1 went to the window labelled 2.
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, 'c');
        prefixed(&mut mux, 'c');
        prefixed(&mut mux, '1');
        prefixed(&mut mux, '&');
        mux.handle_event(&types('y'));
        let numbers: Vec<usize> = mux
            .active_session()
            .unwrap()
            .windows
            .iter()
            .map(|w| w.index)
            .collect();
        assert_eq!(numbers, vec![0, 2]);
        // From window 0: closing window 1 left window 2 active, and a digit
        // that went nowhere would look like one that went there.
        prefixed(&mut mux, '0');
        assert_eq!(mux.active_window().unwrap().index, 0);
        prefixed(&mut mux, '2');
        assert_eq!(
            mux.active_window().unwrap().index,
            2,
            "2 is the second place"
        );
        assert!(
            drawn_texts(&mux).iter().any(|t| t == "2:shell"),
            "the tab says 2"
        );

        prefixed(&mut mux, 'c');
        let numbers: Vec<usize> = mux
            .active_session()
            .unwrap()
            .windows
            .iter()
            .map(|w| w.index)
            .collect();
        assert_eq!(
            numbers,
            vec![0, 1, 2],
            "a new window takes the lowest number free"
        );
        assert_eq!(mux.active_window().unwrap().index, 1);
        prefixed(&mut mux, '7');
        assert_eq!(mux.status_message, "no window 7");
    }

    #[test]
    fn attach_and_kill_session_take_a_name_a_number_or_nothing() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, 'd');
        assert!(mux.detached);
        mux.process_command(":attach");
        assert!(!mux.detached, "attach alone attaches the current session");

        mux.process_command(":new-session work");
        mux.process_command(":attach main");
        assert_eq!(mux.active_session().unwrap().name, "main");
        mux.process_command(":attach 1");
        assert_eq!(mux.active_session().unwrap().name, "work");
        mux.process_command(":attach nowhere");
        assert_eq!(mux.status_message, "no session nowhere");

        mux.process_command(":kill-session main");
        assert_eq!(mux.sessions.len(), 1);
        assert_eq!(mux.active_session().unwrap().name, "work");
    }

    #[test]
    fn two_sessions_cannot_share_a_name() {
        let mut mux = Multiplexer::new();
        mux.process_command(":new-session main");
        assert_eq!(mux.sessions.len(), 1);
        assert_eq!(mux.status_message, "duplicate session: main");
        mux.process_command(":new-session");
        mux.process_command(":new-session");
        let names: Vec<&str> = mux.sessions.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names.len(), 3, "{names:?}");
        assert_ne!(names[1], names[2]);
    }

    #[test]
    fn the_detached_screen_says_what_runs_on_and_how_to_return() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, 'd');
        let texts = drawn_texts(&mux);
        assert!(
            texts.iter().any(|t| t.contains("still running")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t == "Closing this window ends every session.")
        );
        assert!(
            !texts.iter().any(|t| t.contains("tmux attach")),
            "there is no tmux command to type"
        );
        assert!(rect_of(&mux, Target::Attach).is_some());
        assert!(
            rect_of(&mux, Target::ChooseSession).is_none(),
            "one session: nothing to choose"
        );
        mux.handle_event(&press(Key::Enter));
        assert!(!mux.detached, "Enter attaches, as the button says");
    }

    // -- the clock --

    #[allow(
        clippy::unnecessary_wraps,
        reason = "it stands in for `system_clock`, whose type it must have"
    )]
    fn fixed_clock() -> Option<u64> {
        // 2025-09-25 11:33:20 UTC.
        Some(1_758_800_000_000)
    }

    #[test]
    fn the_status_bar_reads_the_wall_clock_in_utc() {
        // It printed the seconds the window had been open as a time of day,
        // so every launch began at 00:00.
        let mut mux = Multiplexer::new();
        mux.clock = fixed_clock;
        tick(&mut mux);
        assert!(
            drawn_texts(&mux).iter().any(|t| t == "11:33 UTC"),
            "{:?}",
            drawn_texts(&mux)
        );
    }

    #[test]
    fn a_status_message_gives_way_to_the_clock() {
        let mut mux = Multiplexer::new();
        mux.clock = fixed_clock;
        mux.set_status("hello");
        assert!(drawn_texts(&mux).iter().any(|t| t == "hello"));
        for _ in 0..6 {
            mux.handle_event(&Event::Tick { elapsed_ms: 1000 });
        }
        let after = drawn_texts(&mux);
        assert!(
            !after.iter().any(|t| t == "hello"),
            "the message outlived five seconds"
        );
        assert!(after.iter().any(|t| t == "11:33 UTC"));
    }

    #[test]
    fn the_clock_is_asked_for_only_as_often_as_something_moves() {
        let mut mux = Multiplexer::new();
        mux.clock = fixed_clock;
        tick(&mut mux);
        mux.handle_event(&Event::FocusOut);
        // No shell, no focused cursor to blink: the next change is the minute.
        let wait = mux.tick_interval().unwrap();
        assert_eq!(
            wait,
            Duration::from_secs(40),
            "the minute turns at 11:34:00"
        );

        mux.set_status("x");
        assert_eq!(
            mux.tick_interval().unwrap(),
            Duration::from_millis(STATUS_MESSAGE_MS)
        );

        let (mut live, _shells) = scripted();
        live.clock = fixed_clock;
        assert!(
            live.tick_interval().unwrap() <= Duration::from_millis(20),
            "a talking shell is read often"
        );
    }

    // -- keys --

    #[test]
    fn every_advertised_prefix_command_does_something() {
        for (label, what) in SHORTCUTS {
            if matches!(*label, "F1" | "Arrows") {
                continue; // not characters after the prefix; tested elsewhere
            }
            for ch in prefix_chars(label) {
                let mut mux = Multiplexer::new();
                mux.handle_event(&key_ev(Key::B, "", true));
                mux.handle_event(&key_ev(Key::A, &ch.to_string(), false));
                assert_ne!(
                    mux.status_message,
                    format!("Unknown key: {ch}"),
                    "the card advertises {label:?} for {what:?}, and the prefix does not know {ch:?}"
                );
            }
        }

        let mut mux = Multiplexer::new();
        mux.handle_event(&key_ev(Key::B, "", true));
        mux.handle_event(&key_ev(Key::A, "~", false));
        assert_eq!(
            mux.status_message, "Unknown key: ~",
            "control: the prefix answers a key nothing binds, so the loop above proves nothing"
        );
    }

    /// The characters a card row names, which are not always one.
    fn prefix_chars(label: &str) -> Vec<char> {
        match label {
            "0-9" => ('0'..='9').collect(),
            "Space" => vec![' '],
            _ => label
                .split(" or ")
                .flat_map(|part| part.split(" / "))
                .filter_map(|part| part.trim().chars().next())
                .collect(),
        }
    }

    #[test]
    fn ctrl_b_arms_the_prefix_and_the_next_key_is_a_command() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        assert_eq!(panes_in_active_window(&mux), 2);
        assert_eq!(mux.prefix_state, PrefixState::Normal, "one key only");
    }

    #[test]
    fn the_prefix_is_spent_by_whatever_follows_it() {
        let mut mux = Multiplexer::new();
        mux.handle_event(&key_ev(Key::B, "", true));
        mux.handle_event(&press(Key::F5));
        assert_eq!(mux.prefix_state, PrefixState::Normal);
    }

    #[test]
    fn the_command_prompt_takes_a_line_and_runs_it() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, ':');
        assert!(mux.command_mode);
        type_str(&mut mux, "new-window");
        assert_eq!(mux.command_input, ":new-window");
        mux.handle_event(&press(Key::Enter));
        assert!(!mux.command_mode);
        assert_eq!(windows(&mux), 2);
    }

    #[test]
    fn the_prompt_swallows_keys_and_backspace_stops_at_the_colon() {
        let (mut mux, shells) = scripted();
        prefixed(&mut mux, ':');
        mux.handle_event(&types('x'));
        assert_eq!(mux.command_input, ":x");
        assert!(
            shell(&shells, 0).borrow().sent.is_empty(),
            "the prompt's key reached the shell"
        );
        for _ in 0..5 {
            mux.handle_event(&press(Key::Backspace));
        }
        assert_eq!(mux.command_input, ":");
        mux.handle_event(&press(Key::Escape));
        assert!(!mux.command_mode);
        assert!(mux.command_input.is_empty());
    }

    #[test]
    fn the_choosers_choose_and_keep_the_keyboard() {
        let (mut mux, shells) = scripted();
        mux.process_command(":new-session second");
        prefixed(&mut mux, 's');
        let before = mux.active_session;
        mux.handle_event(&press(Key::Down));
        assert_ne!(mux.active_session, before);
        mux.handle_event(&types('x'));
        assert!(
            shells.borrow().iter().all(|s| s.borrow().sent.is_empty()),
            "the list is modal"
        );
        mux.handle_event(&press(Key::Enter));
        assert!(!mux.session_chooser);

        prefixed(&mut mux, 'c');
        prefixed(&mut mux, 'w');
        let before = mux.active_session().unwrap().active_window;
        mux.handle_event(&press(Key::Down));
        assert_ne!(mux.active_session().unwrap().active_window, before);
        mux.handle_event(&press(Key::Escape));
        assert!(!mux.window_chooser);
    }

    #[test]
    fn the_prefix_and_an_arrow_move_the_divider_and_stop_short_of_the_edge() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        let ratio = |m: &Multiplexer| match &m.active_window().unwrap().layout {
            LayoutNode::Split { ratio, .. } => *ratio,
            LayoutNode::Leaf(_) => panic!("no split"),
        };
        let before = ratio(&mux);
        mux.handle_event(&key_ev(Key::B, "", true));
        mux.handle_event(&press(Key::Right));
        assert!(ratio(&mux) > before);
        for _ in 0..200 {
            mux.handle_event(&key_ev(Key::B, "", true));
            mux.handle_event(&press(Key::Left));
        }
        assert!((MIN_SPLIT_RATIO..=1.0 - MIN_SPLIT_RATIO).contains(&ratio(&mux)));
    }

    #[test]
    fn zoom_fills_the_window_and_comes_back() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        let active = mux.active_pane_id().unwrap();
        let split = mux.pane_rect(active).unwrap();
        prefixed(&mut mux, 'z');
        let zoomed = mux
            .active_window()
            .unwrap()
            .bounds(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(zoomed.len(), 1);
        assert!(zoomed[0].1.w > split.w);
        let cols = mux.find_pane(active).unwrap().term.cols();
        prefixed(&mut mux, 'z');
        assert_eq!(
            mux.active_window()
                .unwrap()
                .bounds(WINDOW_WIDTH, WINDOW_HEIGHT)
                .len(),
            2
        );
        assert!(
            mux.find_pane(active).unwrap().term.cols() < cols,
            "and its terminal shrank back"
        );
    }

    #[test]
    fn the_title_names_the_session_and_window() {
        let mut mux = Multiplexer::new();
        assert_eq!(mux.title(), "main:0:shell - tmux");
        prefixed(&mut mux, 'c');
        assert_eq!(mux.title(), "main:1:shell - tmux");
    }

    // -- drawing --

    #[test]
    fn the_panes_are_measured_in_the_window_they_are_given() {
        let mut mux = Multiplexer::new();
        let _ = App::render(&mut mux, 600.0, 400.0);
        let narrow = active_pane(&mux).term.cols();
        let _ = App::render(&mut mux, 1600.0, 900.0);
        let wide = active_pane(&mux).term.cols();
        assert!(wide > narrow, "{narrow} -> {wide}");
    }

    #[test]
    fn each_terminal_is_as_big_as_the_space_it_is_drawn_in() {
        // In every window, not only the one showing: a background window's
        // program keeps writing, and must not learn the new width only when
        // the user switches to it.
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        prefixed(&mut mux, 'c');
        prefixed(&mut mux, '"');
        mux.handle_event(&Event::Resize {
            width: 1000,
            height: 700,
        });
        let cfg = TerminalConfig::default();
        let session = mux.active_session().unwrap();
        for window in &session.windows {
            for (id, rect) in window.bounds(1000.0, 700.0) {
                let content = pane_content(rect);
                let want =
                    terminal::Layout::solve(content.w, content.h, cfg.cell_width, cfg.cell_height);
                let term = &mux.find_pane(id).unwrap().term;
                assert_eq!(
                    (term.cols(), term.rows()),
                    (want.cols, want.rows),
                    "pane {id:?} in window {}",
                    window.index
                );
            }
        }
    }

    #[test]
    fn the_active_window_is_always_within_reach() {
        // Thirty-two tabs do not fit in a window, and a tab drawn past its
        // edge is a window the pointer cannot reach; nor may the status bar's
        // list of windows run on under the clock.
        let mut mux = Multiplexer::new();
        mux.clock = fixed_clock;
        tick(&mut mux);
        for _ in 1..MAX_WINDOWS {
            prefixed(&mut mux, 'c');
        }
        let active = mux.active_window().unwrap().id;
        let tab = rect_of(&mux, Target::Tab(active)).expect("the active tab is in view");
        assert!(tab.right() <= WINDOW_WIDTH);

        let frame = mux.frame_at((WINDOW_WIDTH, WINDOW_HEIGHT));
        let clock_x = WINDOW_WIDTH - PADDING - text::width("11:33 UTC", SMALL_TEXT);
        let listed = frame
            .hits()
            .iter()
            .filter(|(t, _)| matches!(t, Target::StatusWindow(_)))
            .inspect(|(t, r)| assert!(r.right() < clock_x, "{t:?} runs under the clock"))
            .count();
        assert!(listed > 0 && listed < MAX_WINDOWS, "{listed} listed");
    }

    #[test]
    fn a_panes_terminal_is_drawn_and_clicked_inside_its_pane() {
        let mut mux = Multiplexer::new();
        prefixed(&mut mux, '%');
        prefixed(&mut mux, '"');
        let frame = mux.frame_at((WINDOW_WIDTH, WINDOW_HEIGHT));
        for (id, rect) in mux
            .active_window()
            .unwrap()
            .bounds(WINDOW_WIDTH, WINDOW_HEIGHT)
        {
            let grid = frame
                .rect_of(|t| *t == Target::Pane(id, TermTarget::Grid))
                .expect("each pane's grid is hit-boxed");
            assert!(
                grid.x >= rect.x
                    && grid.y >= rect.y + PANE_TITLE_HEIGHT - 0.01
                    && grid.right() <= rect.right() + 0.01
                    && grid.bottom() <= rect.bottom() + 0.01,
                "{grid:?} is not inside {rect:?}"
            );
        }
    }

    #[test]
    fn the_frame_balances_at_every_size() {
        for size in [
            (0.0, 0.0),
            (50.0, 30.0),
            (300.0, 200.0),
            (1200.0, 800.0),
            (2400.0, 1400.0),
        ] {
            let mut mux = Multiplexer::new();
            mux.window_width = size.0;
            mux.window_height = size.1;
            mux.relayout();
            prefixed(&mut mux, '%');
            prefixed(&mut mux, 's');
            let frame = mux.frame_at(size);
            assert!(frame.is_balanced(), "{size:?}");
            for (t, r) in frame.hits() {
                assert!(
                    r.x >= -0.01
                        && r.y >= -0.01
                        && r.right() <= size.0 + 0.01
                        && r.bottom() <= size.1 + 0.01,
                    "{t:?} at {r:?} is outside a {size:?} window"
                );
            }
        }
    }

    #[test]
    fn a_pane_is_named_by_what_runs_in_it() {
        let (mut mux, shells) = scripted();
        let id = mux.active_pane_id().unwrap();
        assert!(drawn_texts(&mux).iter().any(|t| t == "shell"));
        shell(&shells, 0)
            .borrow_mut()
            .pending
            .extend_from_slice(b"\x1b]0;vim notes.txt\x07");
        tick(&mut mux);
        assert_eq!(mux.find_pane(id).unwrap().title(), "vim notes.txt");
        assert!(drawn_texts(&mux).iter().any(|t| t == "vim notes.txt"));
    }

    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        let mut mux = Multiplexer::new();
        let mut palette = mux.palette;
        palette.green = Color::rgb(1, 2, 3);
        App::theme_changed(&mut mux, &palette);
        let frame = mux.frame_at((WINDOW_WIDTH, WINDOW_HEIGHT));
        assert!(
            frame.commands().iter().any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == Color::rgb(1, 2, 3))),
            "the status bar did not take the theme's colour"
        );
    }

    // -- the layout tree --

    fn two(direction: SplitDir) -> LayoutNode {
        LayoutNode::Split {
            direction,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf(PaneId(0))),
            second: Box::new(LayoutNode::Leaf(PaneId(1))),
        }
    }

    fn area() -> Rect {
        Rect::new(0.0, 0.0, 800.0, 600.0)
    }

    #[test]
    fn a_stacked_split_puts_the_second_pane_below() {
        let b = two(SplitDir::Stacked).compute_bounds(area());
        assert!(b[1].1.y > b[0].1.y && (b[1].1.x - b[0].1.x).abs() < 0.01);
    }

    #[test]
    fn a_side_by_side_split_puts_the_second_pane_to_the_right() {
        let b = two(SplitDir::SideBySide).compute_bounds(area());
        assert!(b[1].1.x > b[0].1.x && (b[1].1.y - b[0].1.y).abs() < 0.01);
    }

    #[test]
    fn removing_a_pane_gives_its_place_to_its_sibling() {
        let mut layout = two(SplitDir::SideBySide);
        assert!(layout.remove_pane(PaneId(0)));
        assert_eq!(layout.pane_ids(), vec![PaneId(1)]);
        assert!(
            !layout.remove_pane(PaneId(1)),
            "the last pane is the window's to close"
        );
        assert!(!layout.remove_pane(PaneId(7)));
    }

    #[test]
    fn swapping_exchanges_two_leaves() {
        let mut layout = two(SplitDir::Stacked);
        layout.swap(PaneId(0), PaneId(1));
        assert_eq!(layout.pane_ids(), vec![PaneId(1), PaneId(0)]);
    }

    #[test]
    fn every_preset_keeps_every_pane() {
        let panes: Vec<PaneId> = (0..5).map(PaneId).collect();
        for preset in LayoutPreset::ALL {
            let layout = preset.build(&panes).expect("a layout");
            assert_eq!(layout.pane_ids().len(), 5, "{preset:?}");
            assert_eq!(LayoutPreset::by_name(preset.name()), Some(preset));
            assert_eq!(preset.build(&panes[..1]).unwrap().pane_count(), 1);
            assert!(preset.build(&[]).is_none());
        }
    }

    #[test]
    fn the_clock_text_is_hours_and_minutes_in_the_zone() {
        assert_eq!(clock_text(1_758_800_000_000), "11:33 UTC");
        assert_eq!(clock_text(0), "00:00 UTC");
        assert_eq!(clock_text(86_399_999), "23:59 UTC");
    }
}

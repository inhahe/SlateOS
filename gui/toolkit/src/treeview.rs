//! A tree of rows the caller owns, and everything about *looking* at it.
//!
//! `design.txt` asks the toolkit for a treeview and for a "tristate checkbox
//! treeview — good for selecting files and directories, have function to
//! populate it with a directory". This module is both: [`TreeView`] with
//! [`TreeView::checkable`] switched on is the second, and
//! [`crate::dirtree`] is the function that populates one from a directory.
//!
//! # Why the tree's *data* is not in here
//!
//! Before this existed, five applications drew a tree by hand —
//! `archivemanager` (the archive's folders), `jsonviewer`, `devicemanager`,
//! `dbviewer` and `diskanalyzer` — and each one had written the same four
//! things: a node type with an `expanded: bool`, a function that flattens the
//! expanded part into rows, a row painter with indentation and a disclosure
//! arrow, and keyboard handling that none of them finished. What differed
//! between them was only the *data*: an archive's folders, a JSON value, a
//! device list, a schema.
//!
//! So the data stays where it is. A caller implements [`TreeSource`] — "what
//! are the children of this node?" — over the structure it already has, and
//! [`TreeView`] keeps only what is about the *view*: which nodes are open,
//! which one is selected, how far it is scrolled, which boxes are ticked.
//! Copying the data into a widget-owned node tree would have been a second copy
//! of every application's model, and a second copy is the defect this tree
//! keeps finding.
//!
//! It also makes a lazily-loaded tree the ordinary case rather than a special
//! one: a source may answer "not loaded yet" for a node it has not read, and
//! read it when the view reports the node was opened ([`TreeEvent::Expanded`]).
//! A filesystem has to be browsed that way.
//!
//! # Why nodes are named by path and not by row
//!
//! Every piece of state here — the open set, the selection, the hover, the
//! scroll anchor, the ticks — is keyed by the node's **key path**: the keys
//! from the top level down to it, `["usr", "share", "fonts"]`. Never by row
//! number. Opening a node inserts rows below it and closing one removes them,
//! so a row number captured before a click names a *different node* after it.
//! `apps/archivemanager` learned this first: its `toggle_path` records that "an
//! index captured before the toggle names a different row after it, so a second
//! click would collapse the wrong directory". The row list here is a cache,
//! rebuilt from the paths whenever anything changes.
//!
//! # Keys, mouse, and what a double-click means
//!
//! The keyboard follows the convention every desktop tree shares: Up and Down
//! move, Right opens a closed node and then steps into it, Left closes an open
//! node and then steps out to its parent, Home/End/PageUp/PageDown do what
//! they say, a typed character jumps to the next row starting with it, Space
//! ticks a box, and Enter *activates*.
//!
//! Activation — Enter or a double-click on a row — opens or closes a node that
//! has children and reports [`TreeEvent::Activated`] for one that does not. A
//! folder's "open" is to show what is in it; the application decides what
//! opening a file means.
//!
//! A double-click reaches the toolkit *instead of* its second press, so on the
//! arrow and on a checkbox it is treated as that press: two quick clicks on a
//! box tick it and untick it again. The file dialog's rule that "only rows act
//! on the second click" is about buttons whose repetition would be wrong — Back
//! twice goes back twice. A toggle's second click is a request to toggle back,
//! and swallowing it would leave the control in the state the user had just
//! tried to leave.
//!
//! # Drawing and clicking are one walk
//!
//! As everywhere in this toolkit ([`crate::frame`]), the pass that paints the
//! tree also records where each control was painted, and a click is tested
//! against those records. [`TreeView::draw`] paints into a caller's own
//! [`Frame`], so a tree inside an application window is covered by a modal
//! dialog the same way the rest of the window is; [`TreeView::handle_mouse`]
//! builds a frame of its own for a caller that has none.
//!
//! # The tristate model
//!
//! See [`CheckRules`]. The short version: ticking a folder is a *rule* —
//! "everything in here" — and not a tick on each file in it, because the files
//! in an unopened folder have not been read, and the ones created tomorrow do
//! not exist yet.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Included, Unbounded};
use std::ops::Range;

use crate::color::Color;
use crate::disabled::{DISABLED_OPACITY, DisabledState, render_disabled};
use crate::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::{Frame, Rect};
use crate::palette::Palette;
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::scroll_window;
use crate::scrollbar;
use crate::style::CornerRadii;
use crate::surface::Surface;
use crate::text;
use crate::wheel;
use crate::widget::CheckState;

// ============================================================================
// What a source reports
// ============================================================================

/// One child node, as a [`TreeSource`] reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeItem<K> {
    /// What names this node among its siblings. Must be unique among them:
    /// the view addresses a node by the keys on the way down to it, and two
    /// siblings with one key are one node as far as it can tell.
    pub key: K,
    /// The text drawn for the node.
    pub label: String,
    /// Whether the node can have children, and so gets a disclosure arrow.
    ///
    /// "Can have", not "has": a folder is expandable before it has been read.
    /// A node that turns out to be empty once opened loses its arrow.
    pub expandable: bool,
    /// Secondary text drawn at the right-hand end of the row — a size, a count.
    pub detail: Option<String>,
    /// An image drawn before the label, by the id it was uploaded under.
    pub icon: Option<u64>,
    /// Whether the node can be acted on.
    ///
    /// A disabled node is drawn greyed and can still be selected and opened —
    /// that is how a keyboard user reaches it to find out *why* it is disabled
    /// ([`TreeView::hovered_reason`], [`TreeView::selected_reason`]). What it
    /// refuses is being activated or ticked.
    pub state: DisabledState,
}

impl<K> TreeItem<K> {
    /// A node with no children.
    pub fn leaf(key: K, label: impl Into<String>) -> Self {
        Self {
            key,
            label: label.into(),
            expandable: false,
            detail: None,
            icon: None,
            state: DisabledState::Enabled,
        }
    }

    /// A node that can be opened.
    pub fn branch(key: K, label: impl Into<String>) -> Self {
        Self {
            expandable: true,
            ..Self::leaf(key, label)
        }
    }

    /// The same node with secondary text at the end of its row.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// The same node with an icon before its label.
    #[must_use]
    pub fn with_icon(mut self, image_id: u64) -> Self {
        self.icon = Some(image_id);
        self
    }

    /// The same node, disabled, with the reason a user is shown.
    ///
    /// Takes a reason rather than offering a bare "disabled", because a
    /// control that is greyed out without saying why is the thing `design.txt`
    /// asks application authors to avoid.
    #[must_use]
    pub fn disabled(mut self, reason: impl Into<String>) -> Self {
        self.state = DisabledState::Disabled {
            reason: Some(reason.into()),
        };
        self
    }
}

/// The data behind a tree: what the children of a node are.
///
/// Implemented by the caller over whatever structure it already has.
pub trait TreeSource {
    /// What names a node among its siblings — a file name, an object key, an
    /// id. See [`TreeItem::key`].
    type Key: Clone + Ord;

    /// The children of the node at `parent`, in the order they are drawn.
    ///
    /// `parent` is the key path from the top level down; the **empty** path
    /// names the tree's invisible root, whose children are the top-level rows.
    ///
    /// `None` means "not known yet" — a node that has not been loaded. It is
    /// drawn as closed-and-openable until the source can answer; `Some` of an
    /// empty list means the node is known to have no children.
    fn children(&self, parent: &[Self::Key]) -> Option<Vec<TreeItem<Self::Key>>>;
}

// ============================================================================
// Ticks
// ============================================================================

/// Which nodes of a checkbox tree are ticked, as a set of rules.
///
/// # A tick on a folder is a rule, not a tick on each file
///
/// The obvious model stores a state per node and derives a folder's box from
/// its children. It does not survive contact with a real filesystem, for two
/// reasons:
///
/// - **Unread folders.** Ticking `/home` would have to tick every file under
///   it, which means reading every directory under it — before the user has
///   opened any of them.
/// - **Tomorrow's files.** A backup set that says "`/home`, except
///   `/home/u/.cache`" means the new folder created next week is in it. A list
///   of the folders that were ticked on the day it was saved does not.
///
/// So a tick is recorded where the user made it, as a rule over everything
/// beneath: `/home` included, `/home/u/.cache` excluded. Whether any node is
/// included — opened or not, existing yet or not — is the nearest rule at or
/// above it ([`is_included`](Self::is_included)), or the tree's default when
/// there is none.
///
/// # Why "partly ticked" needs no bookkeeping
///
/// The rules are kept **normalised**: no rule ever repeats what its node would
/// inherit anyway. Unticking `/home/u/.cache` and ticking it again removes the
/// rule rather than storing a second, redundant "included" under `/home`'s.
///
/// That invariant is what makes the third state free. A node's box is
/// [`CheckState::Indeterminate`] exactly when some rule sits strictly below it:
/// take the rule nearest the node on any branch; there is nothing between them,
/// so what it inherits is the node's own state, and because it is normalised it
/// says the opposite. So some descendant differs from the node, which is what
/// "partly" means. Conversely, with no rule below it, every descendant inherits
/// from the node. One ordered-map probe answers it
/// ([`has_rule_below`](Self::has_rule_below)), for opened and unopened nodes
/// alike.
///
/// # What a click does
///
/// [`toggle`](Self::toggle) follows the checkbox convention: an unticked or
/// partly-ticked box becomes ticked, a ticked one unticked. Either way the
/// click is about *everything* under the node, so it removes every rule
/// beneath it — a tick on a partly-ticked folder means the whole folder.
///
/// # When the last child goes, the folder goes with it
///
/// Rules alone disagree with every other checkbox tree in one case. Tick
/// `/home`, then untick each of its three folders in turn: `/home` still has
/// its "included" rule, now with three exceptions, so its box says *partly* —
/// the rule still covers whatever is created in `/home` tomorrow — while every
/// box the user can see under it is empty. A box that says "partly" over
/// nothing ticked is a box the user cannot reconcile with what they see.
///
/// So [`toggle_in`](Self::toggle_in), which is handed the tree's source, does
/// what those trees do: when a click leaves every child of a node in the same
/// state, the node takes that state and the children's rules fold into it —
/// `/home` becomes unticked, exceptions and all — and the check repeats one
/// level up. Ticking every child likewise ticks the parent, which then covers
/// tomorrow's files too; the box says "ticked", and that is what ticked means.
/// A node whose children the source does not know yet is left alone, because
/// "every child" cannot be established.
///
/// The display and the meaning therefore agree: in a tree whose nodes are all
/// loaded, a box is partly ticked exactly when what is under it is mixed. That
/// is not argued here but tested, against a brute-force model that stores a
/// tick per file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckRules<K> {
    /// `path -> included`, normalised: no entry equals what its node inherits.
    rules: BTreeMap<Vec<K>, bool>,
    /// Whether a node with no rule at or above it is included.
    default: bool,
}

impl<K: Clone + Ord> CheckRules<K> {
    /// No rules; every node is `default`.
    #[must_use]
    pub fn new(default: bool) -> Self {
        Self {
            rules: BTreeMap::new(),
            default,
        }
    }

    /// Rules loaded from somewhere — a saved backup set, a settings file —
    /// normalised on the way in.
    ///
    /// A rule for the empty path sets the default. When a path appears more
    /// than once, the last occurrence wins, as it would if the rules had been
    /// applied one at a time. A rule that repeats what its node would inherit
    /// is dropped, so a hand-edited file with redundant lines means what it
    /// looks like it means.
    pub fn with_rules(default: bool, rules: impl IntoIterator<Item = (Vec<K>, bool)>) -> Self {
        let mut raw: BTreeMap<Vec<K>, bool> = BTreeMap::new();
        let mut default = default;
        for (path, included) in rules {
            if path.is_empty() {
                default = included;
            } else {
                raw.insert(path, included);
            }
        }
        let mut out = Self::new(default);
        // Ascending key order visits every ancestor before its descendants (a
        // proper prefix sorts first), so `inherited` below only ever consults
        // rules already accepted -- which is what makes one pass enough.
        for (path, included) in raw {
            if included != out.inherited(&path) {
                out.rules.insert(path, included);
            }
        }
        out
    }

    /// Whether a node with no rule at or above it is included.
    #[must_use]
    pub fn default_included(&self) -> bool {
        self.default
    }

    /// Whether the node at `path` is included: the nearest rule at or above
    /// it, or the default.
    ///
    /// Answers for any path, loaded or not, existing or not — which is the
    /// point. Asking about `/home/u/new-folder` answers from `/home`'s rule.
    #[must_use]
    pub fn is_included(&self, path: &[K]) -> bool {
        for len in (1..=path.len()).rev() {
            if let Some(&included) = path.get(..len).and_then(|p| self.rules.get(p)) {
                return included;
            }
        }
        self.default
    }

    /// What the node's box shows.
    #[must_use]
    pub fn state(&self, path: &[K]) -> CheckState {
        if self.has_rule_below(path) {
            CheckState::Indeterminate
        } else if self.is_included(path) {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        }
    }

    /// Whether any rule sits strictly below `path`.
    ///
    /// Every extension of a path sorts immediately after it and before any
    /// path that leaves it, so the first key after `path` is a descendant if
    /// and only if one exists.
    #[must_use]
    pub fn has_rule_below(&self, path: &[K]) -> bool {
        self.rules
            .range::<[K], _>((Excluded(path), Unbounded))
            .next()
            .is_some_and(|(key, _)| key.len() > path.len() && key.starts_with(path))
    }

    /// Include or exclude the node at `path` and everything beneath it.
    ///
    /// Every rule at or below `path` is removed — the new state is about the
    /// whole subtree — and a rule is added at `path` only if it differs from
    /// what the node would inherit. The empty path sets the default and clears
    /// every rule.
    pub fn set(&mut self, path: &[K], included: bool) {
        let doomed: Vec<Vec<K>> = self
            .rules
            .range::<[K], _>((Included(path), Unbounded))
            .take_while(|(key, _)| key.starts_with(path))
            .map(|(key, _)| key.clone())
            .collect();
        for key in doomed {
            self.rules.remove(&key);
        }
        if path.is_empty() {
            self.default = included;
            return;
        }
        if included != self.inherited(path) {
            self.rules.insert(path.to_vec(), included);
        }
    }

    /// Click the node's box: ticked becomes unticked, anything else ticked.
    ///
    /// Returns whether the node is included afterwards.
    ///
    /// This is the form for a caller without the tree's source; it does not
    /// fold a parent whose children all end up alike. A tree does that through
    /// [`toggle_in`](Self::toggle_in), which is what [`TreeView`] calls.
    pub fn toggle(&mut self, path: &[K]) -> bool {
        let included = self.state(path).toggled() == CheckState::Checked;
        self.set(path, included);
        included
    }

    /// [`toggle`](Self::toggle), then fold each ancestor whose children the
    /// click left all in one state — see "When the last child goes" above.
    ///
    /// Returns whether the node is included afterwards.
    pub fn toggle_in<S: TreeSource<Key = K>>(&mut self, path: &[K], source: &S) -> bool {
        let included = self.toggle(path);
        self.fold_upwards(path, source);
        included
    }

    /// Walk up from `path`'s parent, giving each node the state its children
    /// now share, until a node's children disagree or are not known.
    ///
    /// A click changes nothing outside the clicked node's subtree except the
    /// derived state of its ancestors, so this chain is the only place a
    /// newly-uniform set of children can appear. And once one level is mixed,
    /// every level above it contains that mix, so the walk can stop there.
    fn fold_upwards<S: TreeSource<Key = K>>(&mut self, path: &[K], source: &S) {
        let mut child = path.to_vec();
        while child.pop().is_some() {
            let parent = child.as_slice();
            let Some(children) = source.children(parent) else {
                return;
            };
            let mut shared: Option<bool> = None;
            let mut probe = parent.to_vec();
            for item in children {
                probe.push(item.key);
                let state = self.state(&probe);
                probe.pop();
                let included = match state {
                    CheckState::Checked => true,
                    CheckState::Unchecked => false,
                    CheckState::Indeterminate => return,
                };
                match shared {
                    None => shared = Some(included),
                    Some(other) if other != included => return,
                    Some(_) => {}
                }
            }
            let Some(included) = shared else {
                return;
            };
            // `set` rather than a check-first: it is idempotent, and a parent
            // that already reads right has nothing below it to remove.
            self.set(parent, included);
        }
    }

    /// The rules, shallowest first — the form a caller saves.
    ///
    /// Together with [`default_included`](Self::default_included) they are
    /// the whole selection: `(["home"], true), (["home", "u", ".cache"],
    /// false)` reads "`/home`, except its cache".
    pub fn rules(&self) -> impl Iterator<Item = (&[K], bool)> {
        self.rules
            .iter()
            .map(|(path, &included)| (path.as_slice(), included))
    }

    /// How many rules there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether every node simply follows the default.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// What the node at `path` inherits: its parent's state.
    fn inherited(&self, path: &[K]) -> bool {
        match path.split_last() {
            Some((_, parent)) => self.is_included(parent),
            None => self.default,
        }
    }
}

// ============================================================================
// Rows, events and hit targets
// ============================================================================

/// One drawn row: a node the view has open all the way down to.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow<K> {
    /// The keys from the top level down to this node. Never empty.
    pub path: Vec<K>,
    /// See [`TreeItem::label`].
    pub label: String,
    /// See [`TreeItem::detail`].
    pub detail: Option<String>,
    /// See [`TreeItem::icon`].
    pub icon: Option<u64>,
    /// See [`TreeItem::state`].
    pub state: DisabledState,
    /// Whether the row shows a disclosure arrow: the node can have children,
    /// and is not an opened node that turned out to have none.
    pub expandable: bool,
    /// Whether the node is open.
    pub expanded: bool,
}

impl<K> TreeRow<K> {
    /// How far the row is indented: 0 for a top-level node.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.path.len().saturating_sub(1)
    }
}

/// Something the user did to the tree that its owner may want to act on.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeEvent<K> {
    /// The selection moved to this node.
    Selected(Vec<K>),
    /// The selected node went away — deleted from the source, with no
    /// ancestor left on screen to move to.
    SelectionCleared,
    /// A node without children was activated: Enter, or a double-click.
    Activated(Vec<K>),
    /// A node was opened. A lazily-loaded source reads it now and calls
    /// [`TreeView::refresh`].
    Expanded(Vec<K>),
    /// A node was closed.
    Collapsed(Vec<K>),
    /// A box was clicked; `included` is the node's state afterwards.
    CheckChanged {
        /// The node whose box was clicked.
        path: Vec<K>,
        /// Whether that node (and so everything beneath it) is now included.
        included: bool,
    },
    /// The secondary button was pressed — on a row, which is also selected,
    /// or on the empty part of the tree. `x`, `y` are where, in the same space
    /// as the tree's bounds, for placing the menu.
    ContextMenu {
        /// The row under the pointer, if there was one.
        path: Option<Vec<K>>,
        /// Pointer x.
        x: f32,
        /// Pointer y.
        y: f32,
    },
}

/// What part of a tree a point is over, as recorded by [`TreeView::draw`].
///
/// Carries key paths rather than row numbers, as [`Frame`]'s documentation
/// asks of every row-bearing target: a frame drawn before a node opened must
/// not direct a click at whatever row moved into the old position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeHit<K> {
    /// Inside the tree, below its last row.
    Blank,
    /// The body of a row.
    Row(Vec<K>),
    /// A row's disclosure arrow — the whole indentation cell it sits in, not
    /// only its ink, so it can be hit without aiming at a few pixels.
    Disclosure(Vec<K>),
    /// A row's checkbox, likewise the whole cell.
    Check(Vec<K>),
    /// The scrollbar's groove, above or below the thumb.
    ScrollTrack,
    /// The scrollbar's thumb.
    ScrollThumb,
}

// ============================================================================
// Geometry
// ============================================================================

/// Sizes of a tree's parts, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeMetrics {
    /// Height of one row.
    pub row_height: f32,
    /// How far each level is indented from the one above.
    pub indent: f32,
    /// Size of the label and detail text.
    pub font_size: f32,
    /// Whether to draw a faint vertical line down each open level, which is
    /// what lets an eye follow a deep tree back to a row's parent.
    pub guides: bool,
}

impl Default for TreeMetrics {
    fn default() -> Self {
        Self {
            row_height: 24.0,
            indent: 16.0,
            font_size: 13.0,
            guides: true,
        }
    }
}

/// Space left of the first level's disclosure cell.
const PAD_LEFT: f32 = 4.0;
/// Space right of the detail text.
const PAD_RIGHT: f32 = 6.0;
/// Width of the cell that holds a disclosure arrow.
const DISCLOSURE_W: f32 = 16.0;
/// Half the extent of the drawn arrow.
const ARROW: f32 = 3.5;
/// Side of a checkbox.
const CHECK_SIZE: f32 = 14.0;
/// Side of an icon.
const ICON_SIZE: f32 = 16.0;
/// Gap between a row's parts.
const GAP: f32 = 6.0;
/// The most of a row's width its detail text may take. The label is what
/// identifies a row, so it is the detail that gets squeezed.
const DETAIL_SHARE: f32 = 0.4;
/// Below this, a label has too little room to be worth keeping the detail for.
const MIN_LABEL: f32 = 48.0;

/// A row count as pixels.
///
/// The one place the conversion is spelled, so the lint it needs is argued
/// once: a tree large enough to lose `f32` precision in a row *count* has
/// sixteen million rows on screen.
#[allow(clippy::cast_precision_loss)]
fn px(rows: usize) -> f32 {
    rows as f32
}

// ============================================================================
// The view
// ============================================================================

/// The state of looking at a tree: which nodes are open, which is selected,
/// how far it is scrolled, and — for a checkbox tree — which are ticked.
///
/// See the [module documentation](self) for how it is meant to be used and
/// why the data lives elsewhere.
#[derive(Clone, Debug)]
pub struct TreeView<K> {
    /// The drawn rows, rebuilt from the source and the open set by
    /// [`refresh`](Self::refresh) and by every call that changes either.
    rows: Vec<TreeRow<K>>,
    /// Paths of the open nodes. A node stays in here while an ancestor is
    /// closed, so reopening the ancestor restores what was open inside it.
    expanded: BTreeSet<Vec<K>>,
    /// Path of the selected node.
    selected: Option<Vec<K>>,
    /// Path of the row under the pointer.
    hover: Option<Vec<K>>,
    /// Whether the pointer is over the tree at all -- a row, the empty space
    /// below the rows, or the scrollbar. Separate from `hover` because a
    /// disabled tree explains itself wherever the pointer is on it, not only
    /// over a row.
    pointer_in: bool,
    /// Index of the first drawn row.
    first_visible: usize,
    /// Where the tree is drawn, and so where it is clicked.
    bounds: Rect,
    /// Sizes.
    metrics: TreeMetrics,
    /// `Some` for a checkbox tree.
    checks: Option<CheckRules<K>>,
    /// Whether the whole control is disabled, and why.
    state: DisabledState,
    /// Fractions of a wheel notch not yet spent.
    wheel: wheel::Accumulator,
    /// While the thumb is dragged, how far below its top the pointer took it.
    thumb_grab: Option<f32>,
}

impl<K: Clone + Ord> Default for TreeView<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + Ord> TreeView<K> {
    /// A tree without checkboxes, nothing open, nothing selected.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            expanded: BTreeSet::new(),
            selected: None,
            hover: None,
            pointer_in: false,
            first_visible: 0,
            bounds: Rect::EMPTY,
            metrics: TreeMetrics::default(),
            checks: None,
            state: DisabledState::Enabled,
            wheel: wheel::Accumulator::default(),
            thumb_grab: None,
        }
    }

    /// A tree with a tristate checkbox on every row.
    ///
    /// `default_included` is the state of a node that no rule reaches — for a
    /// "choose what to back up" tree, normally `false`.
    #[must_use]
    pub fn checkable(default_included: bool) -> Self {
        Self {
            checks: Some(CheckRules::new(default_included)),
            ..Self::new()
        }
    }

    /// The same tree with different sizes.
    #[must_use]
    pub fn with_metrics(mut self, metrics: TreeMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    /// Sizes in use.
    #[must_use]
    pub fn metrics(&self) -> TreeMetrics {
        self.metrics
    }

    // ------------------------------------------------------------------
    // Reading
    // ------------------------------------------------------------------

    /// Every row the view has open, in drawing order — including the ones
    /// scrolled out of sight.
    #[must_use]
    pub fn rows(&self) -> &[TreeRow<K>] {
        &self.rows
    }

    /// The row index of the node at `path`, if it is drawn.
    #[must_use]
    pub fn index_of(&self, path: &[K]) -> Option<usize> {
        self.rows.iter().position(|row| row.path.as_slice() == path)
    }

    /// The selected node's path.
    #[must_use]
    pub fn selected(&self) -> Option<&[K]> {
        self.selected.as_deref()
    }

    /// The selected node's row.
    #[must_use]
    pub fn selected_row(&self) -> Option<&TreeRow<K>> {
        self.selected
            .as_deref()
            .and_then(|path| self.index_of(path))
            .and_then(|index| self.rows.get(index))
    }

    /// Whether the node at `path` is open.
    #[must_use]
    pub fn is_expanded(&self, path: &[K]) -> bool {
        self.expanded.contains(path)
    }

    /// The tick rules of a checkbox tree; `None` for a plain one.
    #[must_use]
    pub fn check_rules(&self) -> Option<&CheckRules<K>> {
        self.checks.as_ref()
    }

    /// The tick rules, for a caller restoring a saved selection.
    ///
    /// Changing them needs no [`refresh`](Self::refresh): a box's state is
    /// read from the rules each time it is drawn, not cached in a row.
    pub fn check_rules_mut(&mut self) -> Option<&mut CheckRules<K>> {
        self.checks.as_mut()
    }

    /// What the node's box shows; `None` for a plain tree.
    #[must_use]
    pub fn check_state(&self, path: &[K]) -> Option<CheckState> {
        self.checks.as_ref().map(|rules| rules.state(path))
    }

    /// Whether the whole control is disabled, and why.
    #[must_use]
    pub fn state(&self) -> &DisabledState {
        &self.state
    }

    /// Enable or disable the whole control.
    ///
    /// A disabled tree is drawn greyed and ignores input, except that it
    /// keeps tracking the pointer so [`hovered_reason`](Self::hovered_reason)
    /// can say why. Pass a reason: see [`TreeItem::disabled`].
    pub fn set_state(&mut self, state: DisabledState) {
        if state.is_disabled() {
            self.thumb_grab = None;
        }
        self.state = state;
    }

    /// Why the thing under the pointer cannot be used, for a tooltip: the
    /// whole tree's reason if it is disabled, else the hovered row's.
    ///
    /// `None` when the pointer is not on the tree, when nothing under it is
    /// disabled, and when something is but gave no reason.
    #[must_use]
    pub fn hovered_reason(&self) -> Option<&str> {
        if !self.pointer_in {
            return None;
        }
        if self.state.is_disabled() {
            return self.state.reason();
        }
        let path = self.hover.as_deref()?;
        let row = self.rows.get(self.index_of(path)?)?;
        row.state.reason()
    }

    /// Why the selected row cannot be used — the keyboard user's counterpart
    /// of [`hovered_reason`](Self::hovered_reason), which a pointer never
    /// reaches if the user is not using one.
    #[must_use]
    pub fn selected_reason(&self) -> Option<&str> {
        if self.state.is_disabled() {
            return self.state.reason();
        }
        self.selected_row()?.state.reason()
    }

    /// Where the tree is drawn.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Where to draw the tree — and so where it is clicked, since the two are
    /// the same walk. Call it whenever the space changes.
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
        self.clamp_scroll();
    }

    /// How many whole rows fit.
    #[must_use]
    pub fn capacity(&self) -> usize {
        scroll_window::capacity(self.metrics.row_height, self.bounds.h)
    }

    /// Index of the first drawn row.
    #[must_use]
    pub fn first_visible(&self) -> usize {
        self.first_visible
    }

    /// The rows on screen, as indices into [`rows`](Self::rows).
    #[must_use]
    pub fn visible_range(&self) -> Range<usize> {
        let rows =
            scroll_window::visible_count(self.rows.len(), self.capacity(), self.first_visible);
        rows.start..rows.end()
    }

    // ------------------------------------------------------------------
    // Changing what is open, selected and ticked
    // ------------------------------------------------------------------

    /// Rebuild the rows from the source.
    ///
    /// Call it whenever the source's answers may have changed — a folder was
    /// read, a file was deleted. The selection follows its node; if the node is
    /// gone, it moves to the nearest ancestor still drawn and reports that. The
    /// scroll position follows the node at the top of the view, so a row
    /// appearing above the view does not shift what the user is looking at.
    pub fn refresh<S: TreeSource<Key = K>>(&mut self, source: &S) -> Vec<TreeEvent<K>> {
        let anchor = self
            .rows
            .get(self.first_visible)
            .map(|row| row.path.clone());
        self.rows = flatten(source, &self.expanded);

        let mut events = Vec::new();
        if let Some(selected) = self.selected.take() {
            if self.index_of(&selected).is_some() {
                self.selected = Some(selected);
            } else {
                self.selected = self.deepest_drawn_ancestor(&selected);
                events.push(match &self.selected {
                    Some(path) => TreeEvent::Selected(path.clone()),
                    None => TreeEvent::SelectionCleared,
                });
            }
        }
        if let Some(path) = anchor {
            if let Some(index) = self.index_of(&path).or_else(|| {
                self.deepest_drawn_ancestor(&path)
                    .and_then(|a| self.index_of(&a))
            }) {
                self.first_visible = index;
            }
        }
        if self
            .hover
            .as_deref()
            .is_some_and(|path| self.index_of(path).is_none())
        {
            self.hover = None;
        }
        self.clamp_scroll();
        events
    }

    /// Open or close the node at `path`.
    ///
    /// Closing a node whose descendant is selected moves the selection to the
    /// node, since the selected row would otherwise vanish while still being
    /// the one Enter acts on.
    pub fn set_expanded<S: TreeSource<Key = K>>(
        &mut self,
        path: &[K],
        expand: bool,
        source: &S,
    ) -> Vec<TreeEvent<K>> {
        if path.is_empty() || self.is_expanded(path) == expand {
            return Vec::new();
        }
        // A node the source says cannot have children is not opened: the
        // event would send a lazily-loaded caller to read a file as a folder.
        // (A node not drawn yet -- under a closed ancestor -- is taken on
        // trust, as `reveal` needs.)
        if expand
            && self
                .index_of(path)
                .and_then(|index| self.rows.get(index))
                .is_some_and(|row| !row.expandable)
        {
            return Vec::new();
        }
        let mut events = Vec::new();
        if expand {
            self.expanded.insert(path.to_vec());
            events.push(TreeEvent::Expanded(path.to_vec()));
        } else {
            self.expanded.remove(path);
            events.push(TreeEvent::Collapsed(path.to_vec()));
            if self
                .selected
                .as_deref()
                .is_some_and(|sel| sel.len() > path.len() && sel.starts_with(path))
            {
                self.selected = Some(path.to_vec());
                events.push(TreeEvent::Selected(path.to_vec()));
            }
        }
        events.extend(self.refresh(source));
        events
    }

    /// Open the node if it is closed, close it if it is open.
    pub fn toggle_expanded<S: TreeSource<Key = K>>(
        &mut self,
        path: &[K],
        source: &S,
    ) -> Vec<TreeEvent<K>> {
        let expand = !self.is_expanded(path);
        self.set_expanded(path, expand, source)
    }

    /// Close every node.
    pub fn collapse_all<S: TreeSource<Key = K>>(&mut self, source: &S) -> Vec<TreeEvent<K>> {
        let mut events: Vec<TreeEvent<K>> = self
            .expanded
            .iter()
            .map(|path| TreeEvent::Collapsed(path.clone()))
            .collect();
        self.expanded.clear();
        if let Some(top) = self.selected.as_ref().and_then(|sel| sel.first()) {
            let top = vec![top.clone()];
            if self.selected.as_deref() != Some(top.as_slice()) {
                self.selected = Some(top.clone());
                events.push(TreeEvent::Selected(top));
            }
        }
        self.first_visible = 0;
        events.extend(self.refresh(source));
        events
    }

    /// Select the node at `path`, if it is drawn, and scroll it into view.
    ///
    /// Returns whether the selection moved. `None` clears it.
    pub fn select(&mut self, path: Option<&[K]>) -> bool {
        match path {
            None => {
                let moved = self.selected.is_some();
                self.selected = None;
                moved
            }
            Some(path) => {
                let Some(index) = self.index_of(path) else {
                    return false;
                };
                let moved = self.selected.as_deref() != Some(path);
                self.selected = Some(path.to_vec());
                self.reveal_index(index);
                moved
            }
        }
    }

    /// Open every ancestor of `path`, select it and scroll to it — "show me
    /// this file in the tree".
    ///
    /// For a lazily-loaded source the ancestors must already be loaded; the
    /// [`TreeEvent::Expanded`] events returned are the chance to load them, and
    /// a caller that did so calls this again. Whether the node ended up
    /// selected is [`selected`](Self::selected).
    pub fn reveal<S: TreeSource<Key = K>>(&mut self, path: &[K], source: &S) -> Vec<TreeEvent<K>> {
        let mut events = Vec::new();
        for len in 1..path.len() {
            let Some(ancestor) = path.get(..len) else {
                break;
            };
            if !self.is_expanded(ancestor) {
                self.expanded.insert(ancestor.to_vec());
                events.push(TreeEvent::Expanded(ancestor.to_vec()));
            }
        }
        events.extend(self.refresh(source));
        if self.select(Some(path)) {
            events.push(TreeEvent::Selected(path.to_vec()));
        }
        events
    }

    /// Click the box of the node at `path`. Refused — returning `None` — for
    /// a plain tree, a disabled tree, and a disabled row.
    ///
    /// Folds ancestors whose children end up alike; see [`CheckRules`].
    pub fn toggle_check<S: TreeSource<Key = K>>(
        &mut self,
        path: &[K],
        source: &S,
    ) -> Option<TreeEvent<K>> {
        if self.state.is_disabled() {
            return None;
        }
        if self
            .index_of(path)
            .and_then(|index| self.rows.get(index))
            .is_some_and(|row| row.state.is_disabled())
        {
            return None;
        }
        let included = self.checks.as_mut()?.toggle_in(path, source);
        Some(TreeEvent::CheckChanged {
            path: path.to_vec(),
            included,
        })
    }

    /// Scroll so that row `first` is at the top, without moving the selection.
    pub fn scroll_to(&mut self, first: usize) {
        self.first_visible = first;
        self.clamp_scroll();
    }

    /// Scroll by `delta` rows, positive towards the end, without moving the
    /// selection.
    pub fn scroll_by(&mut self, delta: isize) {
        self.first_visible = scroll_window::shift(self.first_visible, delta);
        self.clamp_scroll();
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    /// Act on a key. Returns what happened, which may be nothing.
    ///
    /// Keys with Ctrl, Alt or Super held are left alone: they are the
    /// application's shortcuts, not the tree's.
    pub fn handle_key<S: TreeSource<Key = K>>(
        &mut self,
        key: &KeyEvent,
        source: &S,
    ) -> Vec<TreeEvent<K>> {
        if !key.pressed || self.state.is_disabled() || self.rows.is_empty() {
            return Vec::new();
        }
        let mods = key.modifiers;
        if mods.ctrl || mods.alt || mods.super_key {
            return Vec::new();
        }
        let current = self
            .selected
            .as_deref()
            .and_then(|path| self.index_of(path));
        let last = self.rows.len().saturating_sub(1);
        let page = self.capacity().max(1);
        match key.key {
            Key::Up => self.move_to(current.map_or(0, |i| i.saturating_sub(1))),
            Key::Down => self.move_to(current.map_or(0, |i| i.saturating_add(1).min(last))),
            Key::PageUp => self.move_to(current.map_or(0, |i| i.saturating_sub(page))),
            Key::PageDown => self.move_to(current.map_or(0, |i| i.saturating_add(page).min(last))),
            Key::Home => self.move_to(0),
            Key::End => self.move_to(last),
            Key::Right => match current {
                None => self.move_to(0),
                Some(index) => self.step_in(index, source),
            },
            Key::Left => match current {
                None => self.move_to(0),
                Some(index) => self.step_out(index, source),
            },
            Key::Enter => current.map_or_else(Vec::new, |index| self.activate(index, source)),
            Key::Space => match self.selected.clone() {
                Some(path) => self.toggle_check(&path, source).into_iter().collect(),
                None => Vec::new(),
            },
            _ => match key.single_char() {
                Some(c) if !c.is_control() && !c.is_whitespace() => self.jump_to_char(c, current),
                _ => Vec::new(),
            },
        }
    }

    /// Act on a mouse event, hit-testing it against a frame of the tree's own.
    ///
    /// For a tree drawn with [`draw`](Self::draw) into an application's frame,
    /// hit-test there and call [`handle_hit`](Self::handle_hit) instead, so
    /// that anything drawn over the tree — a modal, a menu — takes the click.
    ///
    /// `event`'s coordinates are in the space the tree's bounds are in. Send
    /// every event, not only presses: a thumb drag is a press, a run of moves
    /// and a release.
    pub fn handle_mouse<S: TreeSource<Key = K>>(
        &mut self,
        event: &MouseEvent,
        source: &S,
    ) -> Vec<TreeEvent<K>> {
        // The hit boxes do not depend on colour, so any palette will do; see
        // `FileDialog::handle_mouse` for the same argument.
        let mut frame = Frame::new(self.bounds.right(), self.bounds.bottom());
        self.draw(&Palette::for_mode(false), &mut frame, |hit| hit);
        let hit = frame.hit_test(event.x, event.y);
        self.handle_hit(hit, event, source)
    }

    /// Act on a mouse event already hit-tested against a frame this tree was
    /// drawn into. `hit` is `None` when the pointer is not over the tree.
    pub fn handle_hit<S: TreeSource<Key = K>>(
        &mut self,
        hit: Option<TreeHit<K>>,
        event: &MouseEvent,
        source: &S,
    ) -> Vec<TreeEvent<K>> {
        // Hover is tracked even while disabled: it is how the tooltip that
        // explains the disabling finds its way to the pointer.
        match &event.kind {
            MouseEventKind::Leave => {
                self.hover = None;
                self.pointer_in = false;
                return Vec::new();
            }
            MouseEventKind::Move | MouseEventKind::Enter => {
                self.pointer_in = hit.is_some();
                self.hover = match &hit {
                    Some(TreeHit::Row(path) | TreeHit::Disclosure(path) | TreeHit::Check(path)) => {
                        Some(path.clone())
                    }
                    _ => None,
                };
            }
            _ => {}
        }
        if self.state.is_disabled() {
            return Vec::new();
        }
        match &event.kind {
            MouseEventKind::Move => {
                self.drag_thumb(event.y);
                Vec::new()
            }
            MouseEventKind::Release(MouseButton::Left) => {
                self.thumb_grab = None;
                Vec::new()
            }
            MouseEventKind::Scroll { dy, .. } => {
                let rows = self.wheel.rows(*dy);
                self.scroll_by(rows);
                Vec::new()
            }
            MouseEventKind::Press(MouseButton::Left) => self.press(hit, event.y, source),
            MouseEventKind::DoubleClick(MouseButton::Left) => match hit {
                // The second press of a pair, on a row: activate it. The first
                // press already selected it.
                Some(TreeHit::Row(path)) => match self.index_of(&path) {
                    Some(index) => self.activate(index, source),
                    None => Vec::new(),
                },
                // Anywhere else it *is* the second press. See the module docs.
                other => self.press(other, event.y, source),
            },
            MouseEventKind::Press(MouseButton::Right) => match hit {
                Some(TreeHit::Row(path) | TreeHit::Disclosure(path) | TreeHit::Check(path)) => {
                    let mut events = Vec::new();
                    if self.select(Some(&path)) {
                        events.push(TreeEvent::Selected(path.clone()));
                    }
                    events.push(TreeEvent::ContextMenu {
                        path: Some(path),
                        x: event.x,
                        y: event.y,
                    });
                    events
                }
                Some(TreeHit::Blank) => vec![TreeEvent::ContextMenu {
                    path: None,
                    x: event.x,
                    y: event.y,
                }],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // Drawing
    // ------------------------------------------------------------------

    /// Draw the tree as a standalone list of commands.
    #[must_use]
    pub fn render(&self, palette: &Palette) -> Vec<RenderCommand> {
        let mut frame: Frame<TreeHit<K>> = Frame::new(self.bounds.right(), self.bounds.bottom());
        self.draw(palette, &mut frame, |hit| hit);
        frame.into_tree().commands
    }

    /// Draw the tree into `frame`, recording each clickable part as
    /// `wrap(hit)` — the application's own target type — so that the frame's
    /// hit test answers for the tree too.
    pub fn draw<T>(&self, palette: &Palette, frame: &mut Frame<T>, wrap: impl Fn(TreeHit<K>) -> T) {
        let bounds = self.bounds;
        if bounds.is_empty() {
            return;
        }
        let m = self.metrics;
        let disabled_tree = self.state.is_disabled();
        let visible = self.visible_range();
        let scroll = self.scroll_geometry();
        let content = match scroll {
            Some((track, _)) => {
                Rect::new(bounds.x, bounds.y, (track.x - bounds.x).max(0.0), bounds.h)
            }
            None => bounds,
        };

        frame.clip(bounds);
        // Recorded first, so everything recorded after it wins where they
        // overlap: the rows below cover it, and what is left is empty space.
        frame.hit(wrap(TreeHit::Blank), bounds);

        let mut ink: Vec<RenderCommand> = Vec::new();
        let line_h = text::line_height(m.font_size, FontWeightHint::Regular);
        for (slot, index) in visible.enumerate() {
            let Some(row) = self.rows.get(index) else {
                break;
            };
            let row_rect = Rect::new(
                content.x,
                content.y + px(slot) * m.row_height,
                content.w,
                m.row_height,
            );
            let depth = px(row.depth());
            let selected = self.selected.as_deref() == Some(row.path.as_slice());
            let hovered = self.hover.as_deref() == Some(row.path.as_slice());
            let row_disabled = disabled_tree || row.state.is_disabled();

            frame.hit(wrap(TreeHit::Row(row.path.clone())), row_rect);

            // The pointer's row gets the faintest wash there is; the selected
            // row gets the selection surface, which under the bordered theme
            // is an accent outline that means "chosen" everywhere (§834).
            if hovered && !selected {
                ink.push(RenderCommand::FillRect {
                    x: row_rect.x,
                    y: row_rect.y,
                    width: row_rect.w,
                    height: row_rect.h,
                    color: palette.hint_fill(),
                    corner_radii: CornerRadii::ZERO,
                });
            }
            if selected {
                palette.push_surface(
                    &mut ink,
                    row_rect.x,
                    row_rect.y,
                    row_rect.w,
                    row_rect.h,
                    0.0,
                    Surface::Selected,
                );
            }

            if m.guides {
                // One line per ancestor level, through the middle of that
                // level's disclosure cell. Decorative: the faintest mark.
                for level in 0..row.depth() {
                    let gx = content.x + PAD_LEFT + px(level) * m.indent + DISCLOSURE_W / 2.0;
                    ink.push(RenderCommand::Line {
                        x1: gx,
                        y1: row_rect.y,
                        x2: gx,
                        y2: row_rect.y + row_rect.h,
                        color: palette.overlay0,
                        width: 1.0,
                    });
                }
            }

            let cell_x = content.x + PAD_LEFT + depth * m.indent;
            let mid_y = row_rect.y + row_rect.h / 2.0;
            if row.expandable {
                let cell = Rect::new(cell_x, row_rect.y, DISCLOSURE_W, row_rect.h);
                frame.hit(wrap(TreeHit::Disclosure(row.path.clone())), cell);
                let arrow_colour = if row_disabled {
                    palette.overlay0
                } else {
                    palette.subtext0
                };
                push_arrow(
                    &mut ink,
                    cell_x + DISCLOSURE_W / 2.0,
                    mid_y,
                    row.expanded,
                    arrow_colour,
                );
            }
            let mut x = cell_x + DISCLOSURE_W;

            if let Some(rules) = &self.checks {
                let cell = Rect::new(x, row_rect.y, CHECK_SIZE + GAP, row_rect.h);
                frame.hit(wrap(TreeHit::Check(row.path.clone())), cell);
                push_check_box(
                    &mut ink,
                    palette,
                    x,
                    mid_y - CHECK_SIZE / 2.0,
                    rules.state(&row.path),
                    row_disabled,
                );
                x += CHECK_SIZE + GAP;
            }

            if let Some(image_id) = row.icon {
                ink.push(RenderCommand::Image {
                    x,
                    y: mid_y - ICON_SIZE / 2.0,
                    width: ICON_SIZE,
                    height: ICON_SIZE,
                    image_id,
                });
                x += ICON_SIZE + GAP;
            }

            let text_y = row_rect.y + (row_rect.h - line_h) / 2.0;
            let right = content.x + content.w - PAD_RIGHT;
            let mut label_end = right;
            if let Some(detail) = &row.detail {
                let wanted = text::measure(detail, m.font_size, FontWeightHint::Regular);
                let width = wanted.min(content.w * DETAIL_SHARE);
                // Drop the detail rather than squeeze the label to nothing: a
                // row that says "4.2 MiB" and not what it is has lost its name.
                if right - width - GAP - x >= MIN_LABEL {
                    let detail_x = right - width;
                    ink.push(RenderCommand::Text {
                        x: detail_x,
                        y: text_y,
                        text: detail.clone(),
                        color: if row_disabled {
                            palette.overlay0
                        } else {
                            palette.subtext0
                        },
                        font_size: m.font_size,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width),
                        overflow: TextOverflow::Ellipsis,
                    });
                    label_end = detail_x - GAP;
                }
            }
            ink.push(RenderCommand::Text {
                x,
                y: text_y,
                text: row.label.clone(),
                color: if row_disabled {
                    palette.overlay0
                } else {
                    palette.text
                },
                font_size: m.font_size,
                font_weight: FontWeightHint::Regular,
                max_width: Some((label_end - x).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

        if let Some((track, thumb)) = scroll {
            frame.hit(wrap(TreeHit::ScrollTrack), track);
            frame.hit(wrap(TreeHit::ScrollThumb), thumb);
            // The same two paints as the file dialog's scrollbar, so the two
            // lists in one window do not disagree about what a scrollbar is.
            ink.push(RenderCommand::FillRect {
                x: track.x,
                y: track.y,
                width: track.w,
                height: track.h,
                color: palette.surface0,
                corner_radii: CornerRadii::ZERO,
            });
            palette.push_surface(
                &mut ink,
                thumb.x,
                thumb.y,
                thumb.w,
                thumb.h,
                3.0,
                Surface::ControlTrack,
            );
        }

        let ink = if disabled_tree {
            render_disabled(&ink, DISABLED_OPACITY)
        } else {
            ink
        };
        for command in ink {
            frame.push(command);
        }
        frame.unclip();
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    /// The scrollbar's track and thumb, when the rows overflow the bounds.
    ///
    /// Shared by the painter and the drag handler, so the thumb a user grabs
    /// is the thumb that was drawn.
    fn scroll_geometry(&self) -> Option<(Rect, Rect)> {
        let capacity = self.capacity();
        if !scrollbar::needed(self.rows.len(), capacity) {
            return None;
        }
        let b = self.bounds;
        let track = Rect::new(b.right() - scrollbar::WIDTH, b.y, scrollbar::WIDTH, b.h);
        let thumb = scrollbar::thumb(track, self.rows.len(), capacity, self.visible_range().start);
        Some((track, thumb))
    }

    /// A left press on `hit`.
    fn press<S: TreeSource<Key = K>>(
        &mut self,
        hit: Option<TreeHit<K>>,
        y: f32,
        source: &S,
    ) -> Vec<TreeEvent<K>> {
        match hit {
            Some(TreeHit::Disclosure(path)) => self.toggle_expanded(&path, source),
            Some(TreeHit::Check(path)) => self.toggle_check(&path, source).into_iter().collect(),
            Some(TreeHit::Row(path)) => {
                if self.select(Some(&path)) {
                    vec![TreeEvent::Selected(path)]
                } else {
                    Vec::new()
                }
            }
            Some(TreeHit::ScrollThumb) => {
                if let Some((_, thumb)) = self.scroll_geometry() {
                    self.thumb_grab = Some(y - thumb.y);
                }
                Vec::new()
            }
            Some(TreeHit::ScrollTrack) => {
                // A press in the groove pages towards the pointer, as every
                // scrollbar does, rather than jumping to it.
                if let Some((_, thumb)) = self.scroll_geometry() {
                    let page = isize::try_from(self.capacity().max(1)).unwrap_or(isize::MAX);
                    self.scroll_by(if y < thumb.y {
                        page.saturating_neg()
                    } else {
                        page
                    });
                }
                Vec::new()
            }
            Some(TreeHit::Blank) | None => Vec::new(),
        }
    }

    /// Follow a dragged thumb.
    fn drag_thumb(&mut self, pointer_y: f32) {
        let Some(grab) = self.thumb_grab else {
            return;
        };
        let Some((track, thumb)) = self.scroll_geometry() else {
            self.thumb_grab = None;
            return;
        };
        if let Some(first) = scrollbar::first_from_drag(
            track,
            thumb.h,
            grab,
            pointer_y,
            self.rows.len(),
            self.capacity(),
        ) {
            self.first_visible = first;
            self.clamp_scroll();
        }
    }

    /// Select row `index` (clamped) and scroll to it.
    fn move_to(&mut self, index: usize) -> Vec<TreeEvent<K>> {
        let Some(row) = self.rows.get(index.min(self.rows.len().saturating_sub(1))) else {
            return Vec::new();
        };
        let path = row.path.clone();
        if self.select(Some(&path)) {
            vec![TreeEvent::Selected(path)]
        } else {
            Vec::new()
        }
    }

    /// Right on row `index`: open it if closed, else step to its first child.
    fn step_in<S: TreeSource<Key = K>>(&mut self, index: usize, source: &S) -> Vec<TreeEvent<K>> {
        let Some(row) = self.rows.get(index) else {
            return Vec::new();
        };
        if row.expandable && !row.expanded {
            let path = row.path.clone();
            return self.set_expanded(&path, true, source);
        }
        let depth = row.depth();
        match self.rows.get(index.saturating_add(1)) {
            Some(next) if next.depth() > depth => self.move_to(index.saturating_add(1)),
            _ => Vec::new(),
        }
    }

    /// Left on row `index`: close it if open, else step out to its parent.
    fn step_out<S: TreeSource<Key = K>>(&mut self, index: usize, source: &S) -> Vec<TreeEvent<K>> {
        let Some(row) = self.rows.get(index) else {
            return Vec::new();
        };
        if row.expandable && row.expanded {
            let path = row.path.clone();
            return self.set_expanded(&path, false, source);
        }
        let Some((_, parent)) = row.path.split_last() else {
            return Vec::new();
        };
        match self.index_of(parent) {
            Some(parent_index) if !parent.is_empty() => self.move_to(parent_index),
            _ => Vec::new(),
        }
    }

    /// Enter, or a double-click on a row.
    fn activate<S: TreeSource<Key = K>>(&mut self, index: usize, source: &S) -> Vec<TreeEvent<K>> {
        let Some(row) = self.rows.get(index) else {
            return Vec::new();
        };
        let path = row.path.clone();
        if row.expandable {
            // Viewing is not acting, so a disabled node still opens: its
            // children may be the explanation.
            return self.toggle_expanded(&path, source);
        }
        if row.state.is_disabled() {
            return Vec::new();
        }
        vec![TreeEvent::Activated(path)]
    }

    /// Type-ahead: the next row after `current` whose label starts with `c`,
    /// ignoring case, wrapping round. Repeating the key walks through them.
    fn jump_to_char(&mut self, c: char, current: Option<usize>) -> Vec<TreeEvent<K>> {
        let len = self.rows.len();
        let start = current.map_or(0, |i| i.saturating_add(1));
        let found = (0..len)
            .filter_map(|step| start.saturating_add(step).checked_rem(len))
            .find(|&i| {
                self.rows
                    .get(i)
                    .and_then(|row| row.label.chars().next())
                    .is_some_and(|first| chars_match(first, c))
            });
        match found {
            Some(index) => self.move_to(index),
            None => Vec::new(),
        }
    }

    /// Scroll just far enough that row `index` is on screen.
    fn reveal_index(&mut self, index: usize) {
        let capacity = self.capacity();
        if capacity == 0 {
            return;
        }
        if index < self.first_visible {
            self.first_visible = index;
        } else if index >= self.first_visible.saturating_add(capacity) {
            self.first_visible = index.saturating_add(1).saturating_sub(capacity);
        }
        self.clamp_scroll();
    }

    /// Keep the view from hanging off the end of the rows.
    fn clamp_scroll(&mut self) {
        let last_page = self.rows.len().saturating_sub(self.capacity());
        self.first_visible = self.first_visible.min(last_page);
    }

    /// The deepest proper ancestor of `path` that is drawn.
    fn deepest_drawn_ancestor(&self, path: &[K]) -> Option<Vec<K>> {
        (1..path.len())
            .rev()
            .filter_map(|len| path.get(..len))
            .find(|ancestor| self.index_of(ancestor).is_some())
            .map(<[K]>::to_vec)
    }
}

/// Whether a label starting with `first` answers a type-ahead of `typed`.
fn chars_match(first: char, typed: char) -> bool {
    first == typed || first.to_lowercase().eq(typed.to_lowercase())
}

/// One level of [`flatten`]'s walk: a node's path, and its children not yet
/// emitted.
type Pending<K> = (Vec<K>, Vec<TreeItem<K>>);

/// The drawn rows: every node whose ancestors are all open, depth first.
///
/// Iterative rather than recursive. Only open nodes are descended into, so
/// depth is bounded by what a user opened — but "reveal" and a source with a
/// pathological nesting (a JSON document is allowed a thousand levels) can
/// open far more than a user would, and the stack should not be what decides
/// how deep a tree may go.
fn flatten<S: TreeSource>(source: &S, expanded: &BTreeSet<Vec<S::Key>>) -> Vec<TreeRow<S::Key>> {
    let mut rows = Vec::new();
    // Each frame: the path of the node whose children are being walked, and
    // those children not yet emitted, reversed so `pop` yields them in order.
    let mut stack: Vec<Pending<S::Key>> = Vec::new();
    if let Some(mut top) = source.children(&[]) {
        top.reverse();
        stack.push((Vec::new(), top));
    }
    while let Some((parent, pending)) = stack.last_mut() {
        let Some(item) = pending.pop() else {
            stack.pop();
            continue;
        };
        let mut path = parent.clone();
        path.push(item.key);
        let expanded_here = item.expandable && expanded.contains(&path);
        let children = if expanded_here {
            source.children(&path)
        } else {
            None
        };
        // An opened node that turned out empty loses its arrow; one that is
        // open but not loaded yet keeps it, since it may yet have children.
        let expandable = item.expandable && !children.as_ref().is_some_and(Vec::is_empty);
        rows.push(TreeRow {
            path: path.clone(),
            label: item.label,
            detail: item.detail,
            icon: item.icon,
            state: item.state,
            expandable,
            expanded: expanded_here,
        });
        if let Some(mut children) = children {
            if !children.is_empty() {
                children.reverse();
                stack.push((path, children));
            }
        }
    }
    rows
}

/// A disclosure arrow: right-pointing when closed, down when open.
///
/// Drawn as two strokes rather than as a `▸`/`▾` glyph, because whether a
/// glyph exists depends on the font a user installed, and a missing arrow is a
/// tree that cannot be opened by pointing.
fn push_arrow(ink: &mut Vec<RenderCommand>, cx: f32, cy: f32, open: bool, color: Color) {
    let (a, b, c) = if open {
        (
            (cx - ARROW, cy - ARROW / 2.0),
            (cx, cy + ARROW / 2.0),
            (cx + ARROW, cy - ARROW / 2.0),
        )
    } else {
        (
            (cx - ARROW / 2.0, cy - ARROW),
            (cx + ARROW / 2.0, cy),
            (cx - ARROW / 2.0, cy + ARROW),
        )
    };
    for ((x1, y1), (x2, y2)) in [(a, b), (b, c)] {
        ink.push(RenderCommand::Line {
            x1,
            y1,
            x2,
            y2,
            color,
            width: 1.5,
        });
    }
}

/// A tristate checkbox at `(x, y)`.
///
/// An empty box is outlined in `subtext0` rather than one of the surface
/// shades: a control's edge has to be seen to be used, and `subtext0` is the
/// faintest role the palette guarantees is legible on every ground it puts a
/// list on. Ticked and partly ticked fill with the accent and mark in
/// `on_accent`, the pair the palette keeps readable for whatever accent the
/// user chose. The marks are strokes, not `✓` glyphs, for the reason
/// [`push_arrow`] gives.
fn push_check_box(
    ink: &mut Vec<RenderCommand>,
    palette: &Palette,
    x: f32,
    y: f32,
    state: CheckState,
    disabled: bool,
) {
    let s = CHECK_SIZE;
    let radii = CornerRadii::all(3.0);
    let (edge, fill) = match (state, disabled) {
        (CheckState::Unchecked, false) => (palette.subtext0, None),
        (CheckState::Unchecked, true) => (palette.overlay0, None),
        (_, false) => (palette.accent, Some(palette.accent)),
        (_, true) => (palette.overlay0, Some(palette.overlay0)),
    };
    if let Some(color) = fill {
        ink.push(RenderCommand::FillRect {
            x,
            y,
            width: s,
            height: s,
            color,
            corner_radii: radii,
        });
    }
    ink.push(RenderCommand::StrokeRect {
        x,
        y,
        width: s,
        height: s,
        color: edge,
        line_width: 1.0,
        corner_radii: radii,
    });
    let mark = if disabled {
        palette.base
    } else {
        palette.on_accent()
    };
    match state {
        CheckState::Unchecked => {}
        CheckState::Checked => {
            let points = [
                (x + s * 0.22, y + s * 0.52),
                (x + s * 0.42, y + s * 0.72),
                (x + s * 0.78, y + s * 0.30),
            ];
            for pair in points.windows(2) {
                if let [(x1, y1), (x2, y2)] = *pair {
                    ink.push(RenderCommand::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        color: mark,
                        width: 2.0,
                    });
                }
            }
        }
        CheckState::Indeterminate => {
            ink.push(RenderCommand::FillRect {
                x: x + s * 0.22,
                y: y + s / 2.0 - 1.0,
                width: s * 0.56,
                height: 2.0,
                color: mark,
                corner_radii: CornerRadii::ZERO,
            });
        }
    }
}

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
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    use crate::event::Modifiers;
    use randrange::{RandomSource, SeededRng};

    // ------------------------------------------------------------------
    // A source over a literal tree
    // ------------------------------------------------------------------

    /// A node of the literal test tree: `(key, children)`, where `None`
    /// children means "not loaded" and a leaf is `Some(vec![])` with
    /// `expandable` false.
    #[derive(Clone, Debug)]
    struct Node {
        key: &'static str,
        expandable: bool,
        children: Option<Vec<Node>>,
        disabled: Option<&'static str>,
        detail: Option<&'static str>,
    }

    fn leaf(key: &'static str) -> Node {
        Node {
            key,
            expandable: false,
            children: Some(Vec::new()),
            disabled: None,
            detail: None,
        }
    }

    fn dir(key: &'static str, children: Vec<Node>) -> Node {
        Node {
            key,
            expandable: true,
            children: Some(children),
            disabled: None,
            detail: None,
        }
    }

    fn unloaded(key: &'static str) -> Node {
        Node {
            key,
            expandable: true,
            children: None,
            disabled: None,
            detail: None,
        }
    }

    struct Literal(Vec<Node>);

    impl Literal {
        fn node(&self, path: &[&'static str]) -> Option<&Node> {
            let mut level = &self.0;
            let mut found = None;
            for key in path {
                let node = level.iter().find(|n| n.key == *key)?;
                found = Some(node);
                level = node.children.as_ref()?;
            }
            found
        }

        fn node_mut(&mut self, path: &[&'static str]) -> Option<&mut Node> {
            let (first, rest) = path.split_first()?;
            let mut node = self.0.iter_mut().find(|n| n.key == *first)?;
            for key in rest {
                node = node.children.as_mut()?.iter_mut().find(|n| n.key == *key)?;
            }
            Some(node)
        }
    }

    impl TreeSource for Literal {
        type Key = &'static str;
        fn children(&self, parent: &[&'static str]) -> Option<Vec<TreeItem<&'static str>>> {
            let level = if parent.is_empty() {
                &self.0
            } else {
                self.node(parent)?.children.as_ref()?
            };
            Some(
                level
                    .iter()
                    .map(|n| {
                        let mut item = if n.expandable {
                            TreeItem::branch(n.key, n.key)
                        } else {
                            TreeItem::leaf(n.key, n.key)
                        };
                        if let Some(reason) = n.disabled {
                            item = item.disabled(reason);
                        }
                        if let Some(detail) = n.detail {
                            item = item.with_detail(detail);
                        }
                        item
                    })
                    .collect(),
            )
        }
    }

    /// home/{docs/{a.txt, b.txt}, pics/{c.png}, empty/}, etc/{hosts}, readme
    fn sample() -> Literal {
        Literal(vec![
            dir(
                "home",
                vec![
                    dir("docs", vec![leaf("a.txt"), leaf("b.txt")]),
                    dir("pics", vec![leaf("c.png")]),
                    dir("empty", vec![]),
                ],
            ),
            dir("etc", vec![leaf("hosts")]),
            leaf("readme"),
        ])
    }

    fn labels<K: Clone + Ord>(view: &TreeView<K>) -> Vec<String> {
        view.rows()
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth()), r.label))
            .collect()
    }

    fn key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn typed(c: char) -> KeyEvent {
        KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        }
    }

    fn mouse(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
        MouseEvent { x, y, kind }
    }

    /// A view over `source`, 200 x `rows` rows tall, refreshed.
    fn view_over(source: &Literal, rows: usize) -> TreeView<&'static str> {
        let mut view = TreeView::new();
        view.set_bounds(Rect::new(0.0, 0.0, 200.0, 24.0 * px(rows)));
        view.refresh(source);
        view
    }

    fn open(view: &mut TreeView<&'static str>, source: &Literal, path: &[&'static str]) {
        view.set_expanded(path, true, source);
    }

    /// The centre of the recorded box for `pred`, from a real frame.
    fn centre_of(
        view: &TreeView<&'static str>,
        pred: impl Fn(&TreeHit<&'static str>) -> bool,
    ) -> (f32, f32) {
        let mut frame = Frame::new(view.bounds().right(), view.bounds().bottom());
        view.draw(&Palette::for_mode(false), &mut frame, |h| h);
        frame.rect_of(pred).expect("target was drawn").centre()
    }

    // ------------------------------------------------------------------
    // Flattening
    // ------------------------------------------------------------------

    #[test]
    fn a_fresh_view_shows_the_top_level_closed() {
        let source = sample();
        let view = view_over(&source, 10);
        assert_eq!(labels(&view), ["home", "etc", "readme"]);
        assert!(view.rows()[0].expandable && !view.rows()[0].expanded);
        assert!(!view.rows()[2].expandable, "a leaf has no arrow");
    }

    #[test]
    fn opening_a_node_inserts_its_children_below_it_in_order() {
        let source = sample();
        let mut view = view_over(&source, 10);
        let events = view.set_expanded(&["home"], true, &source);
        assert_eq!(events, [TreeEvent::Expanded(vec!["home"])]);
        assert_eq!(
            labels(&view),
            ["home", "  docs", "  pics", "  empty", "etc", "readme"]
        );
        open(&mut view, &source, &["home", "docs"]);
        assert_eq!(
            labels(&view),
            [
                "home",
                "  docs",
                "    a.txt",
                "    b.txt",
                "  pics",
                "  empty",
                "etc",
                "readme"
            ]
        );
    }

    #[test]
    fn closing_an_ancestor_remembers_what_was_open_inside_it() {
        let source = sample();
        let mut view = view_over(&source, 10);
        open(&mut view, &source, &["home"]);
        open(&mut view, &source, &["home", "docs"]);
        view.set_expanded(&["home"], false, &source);
        assert_eq!(labels(&view), ["home", "etc", "readme"]);
        open(&mut view, &source, &["home"]);
        assert!(
            labels(&view).contains(&"    a.txt".to_string()),
            "docs reopened with it"
        );
    }

    #[test]
    fn an_opened_node_that_is_empty_loses_its_arrow() {
        let source = sample();
        let mut view = view_over(&source, 10);
        open(&mut view, &source, &["home"]);
        let empty = view.index_of(&["home", "empty"]).unwrap();
        assert!(
            view.rows()[empty].expandable,
            "before it is opened it might have children"
        );
        open(&mut view, &source, &["home", "empty"]);
        assert!(
            !view.rows()[empty].expandable,
            "once it is known to be empty it has none"
        );
    }

    #[test]
    fn an_opened_node_that_is_not_loaded_keeps_its_arrow_until_it_is() {
        let mut source = Literal(vec![unloaded("net")]);
        let mut view = view_over(&source, 10);
        let events = view.set_expanded(&["net"], true, &source);
        assert_eq!(events, [TreeEvent::Expanded(vec!["net"])]);
        assert_eq!(labels(&view), ["net"]);
        assert!(view.rows()[0].expandable && view.rows()[0].expanded);

        // The owner reacts to `Expanded` by loading, then refreshes.
        source.node_mut(&["net"]).unwrap().children = Some(vec![leaf("share")]);
        view.refresh(&source);
        assert_eq!(labels(&view), ["net", "  share"]);
    }

    #[test]
    fn a_thousand_levels_open_do_not_overflow_the_stack() {
        // Built iteratively so the fixture itself does not recurse either.
        let mut node = leaf("bottom");
        let mut path_len = 1;
        for _ in 0..1000 {
            node = dir("d", vec![node]);
            path_len += 1;
        }
        let source = Literal(vec![node]);
        let mut view = view_over(&source, 10);
        let mut path = Vec::new();
        for _ in 0..path_len - 1 {
            path.push("d");
            view.expanded.insert(path.clone());
        }
        view.refresh(&source);
        assert_eq!(view.rows().len(), path_len);
        assert_eq!(view.rows().last().unwrap().depth(), path_len - 1);
    }

    // ------------------------------------------------------------------
    // Selection following its node
    // ------------------------------------------------------------------

    #[test]
    fn closing_the_parent_of_the_selection_moves_the_selection_to_it() {
        let source = sample();
        let mut view = view_over(&source, 10);
        open(&mut view, &source, &["home"]);
        open(&mut view, &source, &["home", "docs"]);
        assert!(view.select(Some(&["home", "docs", "b.txt"])));
        let events = view.set_expanded(&["home"], false, &source);
        assert_eq!(
            events,
            [
                TreeEvent::Collapsed(vec!["home"]),
                TreeEvent::Selected(vec!["home"])
            ]
        );
        assert_eq!(view.selected(), Some(&["home"][..]));
    }

    #[test]
    fn a_deleted_selection_falls_back_to_its_nearest_drawn_ancestor() {
        let mut source = sample();
        let mut view = view_over(&source, 10);
        open(&mut view, &source, &["home"]);
        open(&mut view, &source, &["home", "pics"]);
        view.select(Some(&["home", "pics", "c.png"]));
        source.node_mut(&["home", "pics"]).unwrap().children = Some(vec![]);
        let events = view.refresh(&source);
        assert_eq!(events, [TreeEvent::Selected(vec!["home", "pics"])]);

        source.0.retain(|n| n.key != "home");
        let events = view.refresh(&source);
        assert_eq!(events, [TreeEvent::SelectionCleared]);
        assert_eq!(view.selected(), None);
    }

    #[test]
    fn a_row_appearing_above_the_view_does_not_move_what_is_on_screen() {
        let mut source = Literal((0..40).map(|_| leaf("x")).collect());
        // Unique keys, so the anchor is unambiguous.
        let keys: Vec<&'static str> = (0..40)
            .map(|i| &*Box::leak(format!("row{i:02}").into_boxed_str()))
            .collect();
        for (node, k) in source.0.iter_mut().zip(&keys) {
            node.key = k;
        }
        let mut view = view_over(&source, 5);
        view.scroll_to(20);
        assert_eq!(view.rows()[view.first_visible()].label, "row20");
        source.0.insert(0, leaf("new"));
        view.refresh(&source);
        assert_eq!(
            view.rows()[view.first_visible()].label,
            "row20",
            "anchored to the node, not the index"
        );
    }

    // ------------------------------------------------------------------
    // Keyboard
    // ------------------------------------------------------------------

    #[test]
    fn the_arrow_keys_walk_the_tree_the_way_every_desktop_does() {
        let source = sample();
        let mut view = view_over(&source, 10);
        // Nothing selected: the first key picks the first row.
        assert_eq!(
            view.handle_key(&key(Key::Down), &source),
            [TreeEvent::Selected(vec!["home"])]
        );
        // Right opens a closed node...
        assert_eq!(
            view.handle_key(&key(Key::Right), &source),
            [TreeEvent::Expanded(vec!["home"])]
        );
        assert_eq!(
            view.selected(),
            Some(&["home"][..]),
            "opening does not move"
        );
        // ...and then steps into it.
        assert_eq!(
            view.handle_key(&key(Key::Right), &source),
            [TreeEvent::Selected(vec!["home", "docs"])]
        );
        // Left on a closed child steps out to the parent...
        assert_eq!(
            view.handle_key(&key(Key::Left), &source),
            [TreeEvent::Selected(vec!["home"])]
        );
        // ...and on an open node closes it.
        assert_eq!(
            view.handle_key(&key(Key::Left), &source),
            [TreeEvent::Collapsed(vec!["home"])]
        );
        // Left on a closed top-level node has nowhere to go.
        assert!(view.handle_key(&key(Key::Left), &source).is_empty());
        // Right on a leaf does nothing.
        view.handle_key(&key(Key::End), &source);
        assert_eq!(view.selected(), Some(&["readme"][..]));
        assert!(view.handle_key(&key(Key::Right), &source).is_empty());
    }

    #[test]
    fn right_on_an_opened_empty_node_does_not_step_into_its_sibling() {
        let source = sample();
        let mut view = view_over(&source, 10);
        open(&mut view, &source, &["home"]);
        open(&mut view, &source, &["home", "empty"]);
        view.select(Some(&["home", "empty"]));
        // The next row is `etc`, a sibling of `home`: not a child.
        assert!(view.handle_key(&key(Key::Right), &source).is_empty());
        assert_eq!(view.selected(), Some(&["home", "empty"][..]));
    }

    #[test]
    fn up_down_home_end_and_paging_stop_at_the_ends() {
        let source = Literal(
            ["a", "b", "c", "d", "e", "f", "g", "h"]
                .into_iter()
                .map(leaf)
                .collect(),
        );
        let mut view = view_over(&source, 3);
        view.handle_key(&key(Key::Up), &source);
        assert_eq!(view.selected(), Some(&["a"][..]));
        view.handle_key(&key(Key::Up), &source);
        assert_eq!(view.selected(), Some(&["a"][..]), "no wrap at the top");
        view.handle_key(&key(Key::PageDown), &source);
        assert_eq!(
            view.selected(),
            Some(&["d"][..]),
            "a page is the visible row count"
        );
        assert!(
            view.visible_range().contains(&3),
            "paging scrolls the selection into view"
        );
        view.handle_key(&key(Key::End), &source);
        assert_eq!(view.selected(), Some(&["h"][..]));
        assert_eq!(view.visible_range(), 5..8);
        view.handle_key(&key(Key::Down), &source);
        assert_eq!(view.selected(), Some(&["h"][..]), "no wrap at the bottom");
        view.handle_key(&key(Key::PageUp), &source);
        assert_eq!(view.selected(), Some(&["e"][..]));
        view.handle_key(&key(Key::Home), &source);
        assert_eq!(view.selected(), Some(&["a"][..]));
        assert_eq!(view.visible_range(), 0..3);
    }

    #[test]
    fn enter_opens_a_folder_and_activates_a_file() {
        let source = sample();
        let mut view = view_over(&source, 10);
        view.select(Some(&["home"]));
        assert_eq!(
            view.handle_key(&key(Key::Enter), &source),
            [TreeEvent::Expanded(vec!["home"])]
        );
        assert_eq!(
            view.handle_key(&key(Key::Enter), &source),
            [TreeEvent::Collapsed(vec!["home"])]
        );
        view.select(Some(&["readme"]));
        assert_eq!(
            view.handle_key(&key(Key::Enter), &source),
            [TreeEvent::Activated(vec!["readme"])]
        );
    }

    #[test]
    fn a_disabled_file_is_reachable_and_says_why_but_does_not_activate() {
        let mut source = sample();
        source.node_mut(&["readme"]).unwrap().disabled = Some("Permission denied");
        let mut view = view_over(&source, 10);
        view.handle_key(&key(Key::End), &source);
        assert_eq!(
            view.selected(),
            Some(&["readme"][..]),
            "a keyboard user can still reach it"
        );
        assert_eq!(view.selected_reason(), Some("Permission denied"));
        assert!(view.handle_key(&key(Key::Enter), &source).is_empty());
    }

    #[test]
    fn typing_a_letter_jumps_to_the_next_row_starting_with_it() {
        let source = Literal(
            ["Alpha", "beta", "apple", "Bravo", "avocado"]
                .into_iter()
                .map(leaf)
                .collect(),
        );
        let mut view = view_over(&source, 10);
        assert_eq!(
            view.handle_key(&typed('a'), &source),
            [TreeEvent::Selected(vec!["Alpha"])]
        );
        assert_eq!(
            view.handle_key(&typed('a'), &source),
            [TreeEvent::Selected(vec!["apple"])]
        );
        assert_eq!(
            view.handle_key(&typed('A'), &source),
            [TreeEvent::Selected(vec!["avocado"])]
        );
        assert_eq!(
            view.handle_key(&typed('a'), &source),
            [TreeEvent::Selected(vec!["Alpha"])],
            "wraps round"
        );
        assert_eq!(
            view.handle_key(&typed('b'), &source),
            [TreeEvent::Selected(vec!["beta"])]
        );
        assert!(view.handle_key(&typed('z'), &source).is_empty());
    }

    #[test]
    fn shortcuts_releases_and_a_disabled_tree_are_left_alone() {
        let source = sample();
        let mut view = view_over(&source, 10);
        let mut ctrl = key(Key::Down);
        ctrl.modifiers.ctrl = true;
        assert!(view.handle_key(&ctrl, &source).is_empty());
        let mut released = key(Key::Down);
        released.pressed = false;
        assert!(view.handle_key(&released, &source).is_empty());
        view.set_state(DisabledState::Disabled {
            reason: Some("Scanning".into()),
        });
        assert!(view.handle_key(&key(Key::Down), &source).is_empty());
        assert_eq!(view.selected(), None);
    }

    // ------------------------------------------------------------------
    // Mouse
    // ------------------------------------------------------------------

    #[test]
    fn a_click_on_the_arrow_opens_and_a_click_on_the_row_selects() {
        let source = sample();
        let mut view = view_over(&source, 10);
        let (ax, ay) = centre_of(&view, |h| *h == TreeHit::Disclosure(vec!["home"]));
        let events = view.handle_mouse(
            &mouse(ax, ay, MouseEventKind::Press(MouseButton::Left)),
            &source,
        );
        assert_eq!(events, [TreeEvent::Expanded(vec!["home"])]);
        assert_eq!(view.selected(), None, "the arrow does not select");

        let (rx, ry) = centre_of(&view, |h| *h == TreeHit::Row(vec!["home", "pics"]));
        let events = view.handle_mouse(
            &mouse(rx, ry, MouseEventKind::Press(MouseButton::Left)),
            &source,
        );
        assert_eq!(events, [TreeEvent::Selected(vec!["home", "pics"])]);
    }

    #[test]
    fn a_double_click_on_a_folder_row_opens_it_and_on_a_file_activates_it() {
        let source = sample();
        let mut view = view_over(&source, 10);
        let (x, y) = centre_of(&view, |h| *h == TreeHit::Row(vec!["etc"]));
        view.handle_mouse(
            &mouse(x, y, MouseEventKind::Press(MouseButton::Left)),
            &source,
        );
        let events = view.handle_mouse(
            &mouse(x, y, MouseEventKind::DoubleClick(MouseButton::Left)),
            &source,
        );
        assert_eq!(events, [TreeEvent::Expanded(vec!["etc"])]);

        let (x, y) = centre_of(&view, |h| *h == TreeHit::Row(vec!["etc", "hosts"]));
        view.handle_mouse(
            &mouse(x, y, MouseEventKind::Press(MouseButton::Left)),
            &source,
        );
        let events = view.handle_mouse(
            &mouse(x, y, MouseEventKind::DoubleClick(MouseButton::Left)),
            &source,
        );
        assert_eq!(events, [TreeEvent::Activated(vec!["etc", "hosts"])]);
    }

    #[test]
    fn two_quick_clicks_on_an_arrow_open_and_close_it_again() {
        let source = sample();
        let mut view = view_over(&source, 10);
        let (x, y) = centre_of(&view, |h| *h == TreeHit::Disclosure(vec!["etc"]));
        view.handle_mouse(
            &mouse(x, y, MouseEventKind::Press(MouseButton::Left)),
            &source,
        );
        assert!(view.is_expanded(&["etc"]));
        let events = view.handle_mouse(
            &mouse(x, y, MouseEventKind::DoubleClick(MouseButton::Left)),
            &source,
        );
        assert_eq!(events, [TreeEvent::Collapsed(vec!["etc"])]);
        assert!(!view.is_expanded(&["etc"]));
    }

    #[test]
    fn a_right_click_selects_the_row_and_asks_for_a_menu() {
        let source = sample();
        let mut view = view_over(&source, 10);
        let (x, y) = centre_of(&view, |h| *h == TreeHit::Row(vec!["readme"]));
        let events = view.handle_mouse(
            &mouse(x, y, MouseEventKind::Press(MouseButton::Right)),
            &source,
        );
        assert_eq!(
            events,
            [
                TreeEvent::Selected(vec!["readme"]),
                TreeEvent::ContextMenu {
                    path: Some(vec!["readme"]),
                    x,
                    y
                }
            ]
        );
        // Below the last row: a menu for the tree itself.
        let events = view.handle_mouse(
            &mouse(100.0, 200.0, MouseEventKind::Press(MouseButton::Right)),
            &source,
        );
        assert_eq!(
            events,
            [TreeEvent::ContextMenu {
                path: None,
                x: 100.0,
                y: 200.0
            }]
        );
    }

    #[test]
    fn hover_follows_the_pointer_and_leaves_with_it() {
        let mut source = sample();
        source.node_mut(&["etc"]).unwrap().disabled = Some("Mounted read-only");
        let mut view = view_over(&source, 10);
        let (x, y) = centre_of(&view, |h| *h == TreeHit::Row(vec!["etc"]));
        view.handle_mouse(&mouse(x, y, MouseEventKind::Move), &source);
        assert_eq!(view.hovered_reason(), Some("Mounted read-only"));
        view.handle_mouse(&mouse(x, y, MouseEventKind::Leave), &source);
        assert_eq!(view.hovered_reason(), None);
    }

    #[test]
    fn a_leaf_cannot_be_opened_even_by_asking() {
        let source = sample();
        let mut view = view_over(&source, 10);
        assert!(view.set_expanded(&["readme"], true, &source).is_empty());
        assert!(!view.is_expanded(&["readme"]));
    }

    #[test]
    fn a_disabled_tree_explains_itself_only_while_the_pointer_is_on_it() {
        let source = sample();
        let mut view = view_over(&source, 10);
        view.set_state(DisabledState::Disabled {
            reason: Some("Scanning".into()),
        });
        assert_eq!(view.hovered_reason(), None, "the pointer has not arrived");
        // Below the last row: not over a row, but over the tree.
        view.handle_mouse(&mouse(100.0, 200.0, MouseEventKind::Move), &source);
        assert_eq!(view.hovered_reason(), Some("Scanning"));
        // Outside the bounds altogether.
        view.handle_mouse(&mouse(500.0, 500.0, MouseEventKind::Move), &source);
        assert_eq!(view.hovered_reason(), None);
    }

    #[test]
    fn a_disabled_tree_ignores_clicks_but_still_explains_itself() {
        let source = sample();
        let mut view = view_over(&source, 10);
        view.set_state(DisabledState::Disabled {
            reason: Some("Choose a backup drive first".into()),
        });
        let (x, y) = centre_of(&view, |h| *h == TreeHit::Disclosure(vec!["home"]));
        assert!(
            view.handle_mouse(
                &mouse(x, y, MouseEventKind::Press(MouseButton::Left)),
                &source
            )
            .is_empty()
        );
        assert!(!view.is_expanded(&["home"]));
        view.handle_mouse(&mouse(x, y, MouseEventKind::Move), &source);
        assert_eq!(view.hovered_reason(), Some("Choose a backup drive first"));
    }

    #[test]
    fn the_wheel_scrolls_without_moving_the_selection() {
        let source = Literal((0..30).map(|_| leaf("x")).collect());
        let mut view = view_over(&source, 5);
        view.select(Some(&["x"]));
        view.handle_mouse(
            &mouse(10.0, 10.0, MouseEventKind::Scroll { dx: 0.0, dy: -1.0 }),
            &source,
        );
        assert_eq!(view.first_visible(), 3, "one notch down is three rows");
        view.handle_mouse(
            &mouse(10.0, 10.0, MouseEventKind::Scroll { dx: 0.0, dy: 100.0 }),
            &source,
        );
        assert_eq!(view.first_visible(), 0, "clamped at the top");
    }

    #[test]
    fn the_thumb_can_be_dragged_and_the_groove_pages() {
        let names: Vec<&'static str> = (0..100)
            .map(|i| &*Box::leak(format!("n{i}").into_boxed_str()))
            .collect();
        let source = Literal(names.iter().map(|n| leaf(n)).collect());
        let mut view = view_over(&source, 10);
        let (tx, ty) = centre_of(&view, |h| *h == TreeHit::ScrollThumb);
        view.handle_mouse(
            &mouse(tx, ty, MouseEventKind::Press(MouseButton::Left)),
            &source,
        );
        view.handle_mouse(
            &mouse(tx, view.bounds().bottom(), MouseEventKind::Move),
            &source,
        );
        assert_eq!(view.first_visible(), 90, "dragged to the end");
        view.handle_mouse(
            &mouse(tx, ty, MouseEventKind::Release(MouseButton::Left)),
            &source,
        );
        view.handle_mouse(&mouse(tx, 0.0, MouseEventKind::Move), &source);
        assert_eq!(view.first_visible(), 90, "released: moves no longer drag");

        // A press in the groove above the thumb pages up by one screen.
        view.handle_mouse(
            &mouse(tx, 2.0, MouseEventKind::Press(MouseButton::Left)),
            &source,
        );
        assert_eq!(view.first_visible(), 80);
    }

    #[test]
    fn a_click_is_answered_by_whatever_the_application_drew_over_the_tree() {
        // The tree drawn into an application's own frame, then a modal over
        // it: the frame's hit test must not reach the tree.
        #[derive(Clone, Debug, PartialEq)]
        enum AppTarget {
            Tree(TreeHit<&'static str>),
            Modal,
        }
        let source = sample();
        let view = view_over(&source, 10);
        let mut frame: Frame<AppTarget> = Frame::new(400.0, 400.0);
        view.draw(&Palette::for_mode(false), &mut frame, AppTarget::Tree);
        let (x, y) = frame
            .rect_of(|t| *t == AppTarget::Tree(TreeHit::Row(vec!["etc"])))
            .unwrap()
            .centre();
        assert_eq!(
            frame.hit_test(x, y),
            Some(AppTarget::Tree(TreeHit::Row(vec!["etc"])))
        );
        frame.discard_hits();
        frame.hit(AppTarget::Modal, Rect::new(0.0, 0.0, 400.0, 400.0));
        assert_eq!(frame.hit_test(x, y), Some(AppTarget::Modal));
    }

    // ------------------------------------------------------------------
    // Checkboxes
    // ------------------------------------------------------------------

    #[test]
    fn ticking_a_folder_ticks_everything_under_it_loaded_or_not() {
        let mut rules = CheckRules::new(false);
        rules.set(&["home"], true);
        assert_eq!(rules.state(&["home"]), CheckState::Checked);
        assert!(rules.is_included(&["home", "docs", "a.txt"]));
        assert!(
            rules.is_included(&["home", "created", "tomorrow"]),
            "a path nobody has seen yet is covered by the rule"
        );
        assert!(!rules.is_included(&["etc"]));
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn unticking_inside_a_ticked_folder_makes_every_ancestor_partial() {
        let mut rules = CheckRules::new(false);
        rules.set(&["home"], true);
        rules.set(&["home", "u", ".cache"], false);
        assert_eq!(rules.state(&["home"]), CheckState::Indeterminate);
        assert_eq!(rules.state(&["home", "u"]), CheckState::Indeterminate);
        assert_eq!(rules.state(&["home", "u", ".cache"]), CheckState::Unchecked);
        assert_eq!(rules.state(&["home", "u", "docs"]), CheckState::Checked);
        assert_eq!(rules.state(&["etc"]), CheckState::Unchecked);
        let saved: Vec<(Vec<&str>, bool)> = rules.rules().map(|(p, i)| (p.to_vec(), i)).collect();
        assert_eq!(
            saved,
            [(vec!["home"], true), (vec!["home", "u", ".cache"], false)]
        );
    }

    #[test]
    fn a_rule_that_repeats_its_inheritance_is_never_kept() {
        let mut rules = CheckRules::new(false);
        rules.set(&["home"], true);
        rules.set(&["home", "u", ".cache"], false);
        rules.set(&["home", "u", ".cache"], true);
        assert_eq!(
            rules.len(),
            1,
            "re-ticking removed the exception rather than adding a rule"
        );
        assert_eq!(rules.state(&["home"]), CheckState::Checked);
        rules.set(&["etc"], false);
        assert_eq!(
            rules.len(),
            1,
            "excluding what is already excluded adds nothing"
        );
    }

    #[test]
    fn clicking_a_partial_box_ticks_the_whole_subtree() {
        let mut rules = CheckRules::new(false);
        rules.set(&["home", "u", "docs"], true);
        assert_eq!(rules.state(&["home"]), CheckState::Indeterminate);
        assert!(rules.toggle(&["home"]), "partial becomes ticked");
        assert_eq!(rules.state(&["home"]), CheckState::Checked);
        assert_eq!(rules.len(), 1, "the rule below was absorbed");
        assert!(!rules.toggle(&["home"]), "ticked becomes unticked");
        assert!(rules.is_empty());
    }

    #[test]
    fn unticking_every_child_unticks_the_folder_and_ticking_every_child_ticks_it() {
        let source = sample();
        let mut rules = CheckRules::new(false);
        rules.toggle_in(&["home"], &source);
        for child in ["docs", "pics", "empty"] {
            rules.toggle_in(&["home", child], &source);
        }
        assert_eq!(
            rules.state(&["home"]),
            CheckState::Unchecked,
            "nothing under it is ticked, so neither is it"
        );
        assert!(
            rules.is_empty(),
            "the folder's rule and its three exceptions folded away"
        );
        assert!(!rules.is_included(&["home", "created-later"]));

        for child in ["docs", "pics", "empty"] {
            rules.toggle_in(&["home", child], &source);
        }
        assert_eq!(rules.state(&["home"]), CheckState::Checked);
        let kept: Vec<(Vec<&str>, bool)> = rules.rules().map(|(p, i)| (p.to_vec(), i)).collect();
        assert_eq!(
            kept,
            [(vec!["home"], true)],
            "three rules became the folder's one"
        );
        assert!(
            rules.is_included(&["home", "created-later"]),
            "and it means what it shows"
        );
    }

    #[test]
    fn folding_climbs_while_each_level_agrees_and_can_reach_the_default() {
        let source = sample();
        let mut rules = CheckRules::new(false);
        // `etc` holds only `hosts`, so ticking it ticks `etc`...
        rules.toggle_in(&["etc", "hosts"], &source);
        assert_eq!(rules.state(&["etc"]), CheckState::Checked);
        // ...but `etc`'s siblings disagree, so it stops there.
        assert!(!rules.default_included());
        rules.toggle_in(&["home"], &source);
        rules.toggle_in(&["readme"], &source);
        // Every top-level row is now ticked: the root takes the state, which
        // is the default, and every rule folds into it.
        assert!(rules.default_included());
        assert!(rules.is_empty());
    }

    #[test]
    fn a_folder_whose_children_are_not_known_is_not_folded() {
        let source = Literal(vec![dir("top", vec![unloaded("lazy"), leaf("file")])]);
        let mut rules = CheckRules::new(false);
        rules.toggle_in(&["top", "lazy", "x"], &source);
        assert_eq!(
            rules.state(&["top", "lazy"]),
            CheckState::Indeterminate,
            "its other children are unread, so they cannot be shown to agree"
        );
        assert_eq!(rules.state(&["top"]), CheckState::Indeterminate);
    }

    #[test]
    fn saved_rules_come_back_normalised() {
        let rules = CheckRules::with_rules(
            false,
            [
                (vec!["home", "u"], true), // redundant under home
                (vec!["home"], true),
                (vec!["etc"], false), // redundant under the default
                (vec!["home", "u", "tmp"], false),
                (vec!["home", "u", "tmp"], true), // the later line wins -- and is redundant
            ],
        );
        let kept: Vec<(Vec<&str>, bool)> = rules.rules().map(|(p, i)| (p.to_vec(), i)).collect();
        assert_eq!(kept, [(vec!["home"], true)]);

        let rooted = CheckRules::with_rules(false, [(vec![], true), (vec!["tmp"], false)]);
        assert!(
            rooted.default_included(),
            "a rule for the root sets the default"
        );
        assert!(!rooted.is_included(&["tmp", "x"]));
        assert!(rooted.is_included(&["home"]));
    }

    /// The brute-force model the rules must agree with: an explicit state per
    /// node of a fixed universe, updated the naive way.
    #[test]
    fn the_rules_agree_with_a_brute_force_model_under_random_clicks() {
        // Every path of length 1..=3 over the alphabet {a, b, c}.
        let alphabet = ["a", "b", "c"];
        let mut universe: Vec<Vec<&str>> = Vec::new();
        for x in alphabet {
            universe.push(vec![x]);
            for y in alphabet {
                universe.push(vec![x, y]);
                for z in alphabet {
                    universe.push(vec![x, y, z]);
                }
            }
        }
        let leaves: Vec<&Vec<&str>> = universe.iter().filter(|p| p.len() == 3).collect();
        // The same universe as a source, every node loaded.
        let level = |children: Vec<Node>| -> Vec<Node> {
            alphabet.iter().map(|k| dir(k, children.clone())).collect()
        };
        let bottom: Vec<Node> = alphabet.iter().map(|k| leaf(k)).collect();
        let source = Literal(level(level(bottom)));
        let mut rng = SeededRng::new(0x0007_EE0F_C11C);
        for default in [false, true] {
            let mut rules = CheckRules::new(default);
            // The naive model: an explicit tick per leaf.
            let mut model: BTreeMap<Vec<&str>, bool> =
                leaves.iter().map(|p| ((*p).clone(), default)).collect();
            for _ in 0..400 {
                let target = &universe[rng.below(universe.len())];
                // What the naive model says the box shows before the click.
                let under: Vec<bool> = model
                    .iter()
                    .filter(|(p, _)| p.starts_with(target))
                    .map(|(_, &v)| v)
                    .collect();
                let shown = if under.iter().all(|&v| v) {
                    CheckState::Checked
                } else if under.iter().all(|&v| !v) {
                    CheckState::Unchecked
                } else {
                    CheckState::Indeterminate
                };
                assert_eq!(
                    rules.state(target),
                    shown,
                    "state of {target:?} before clicking"
                );
                let now = rules.toggle_in(target, &source);
                assert_eq!(now, shown != CheckState::Checked);
                for (p, v) in &mut model {
                    if p.starts_with(target) {
                        *v = now;
                    }
                }
                for leaf in &leaves {
                    assert_eq!(rules.is_included(leaf), model[*leaf], "leaf {leaf:?}");
                }
                // Normalisation holds after every click.
                for (path, included) in rules.rules() {
                    assert_ne!(
                        included,
                        rules.inherited(path),
                        "redundant rule at {path:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn space_and_the_box_tick_but_a_disabled_row_refuses() {
        let mut source = sample();
        source.node_mut(&["etc"]).unwrap().disabled = Some("Mounted read-only");
        let mut view = TreeView::checkable(false);
        view.set_bounds(Rect::new(0.0, 0.0, 300.0, 240.0));
        view.refresh(&source);

        view.select(Some(&["home"]));
        assert_eq!(
            view.handle_key(&key(Key::Space), &source),
            [TreeEvent::CheckChanged {
                path: vec!["home"],
                included: true
            }]
        );
        assert_eq!(view.check_state(&["home"]), Some(CheckState::Checked));

        let (x, y) = centre_of(&view, |h| *h == TreeHit::Check(vec!["readme"]));
        let events = view.handle_mouse(
            &mouse(x, y, MouseEventKind::Press(MouseButton::Left)),
            &source,
        );
        assert_eq!(
            events,
            [TreeEvent::CheckChanged {
                path: vec!["readme"],
                included: true
            }]
        );
        assert_eq!(
            view.selected(),
            Some(&["home"][..]),
            "ticking does not select"
        );

        let (x, y) = centre_of(&view, |h| *h == TreeHit::Check(vec!["etc"]));
        assert!(
            view.handle_mouse(
                &mouse(x, y, MouseEventKind::Press(MouseButton::Left)),
                &source
            )
            .is_empty()
        );
        assert_eq!(view.check_state(&["etc"]), Some(CheckState::Unchecked));
        // A disabled row still inherits: ticking its parent covers it. Here
        // `etc` is top-level, so tick the default instead.
        view.check_rules_mut().unwrap().set(&[], true);
        assert_eq!(view.check_state(&["etc"]), Some(CheckState::Checked));
    }

    #[test]
    fn a_plain_tree_has_no_boxes_to_tick() {
        let source = sample();
        let mut view = view_over(&source, 10);
        view.select(Some(&["home"]));
        assert!(view.handle_key(&key(Key::Space), &source).is_empty());
        assert_eq!(view.check_state(&["home"]), None);
        assert!(view.toggle_check(&["home"], &source).is_none());
    }

    // ------------------------------------------------------------------
    // Drawing
    // ------------------------------------------------------------------

    fn texts(cmds: &[RenderCommand]) -> Vec<(String, f32, Option<f32>)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text, x, max_width, ..
                } => Some((text.clone(), *x, *max_width)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn only_the_rows_on_screen_are_drawn_and_all_inside_the_bounds() {
        let names: Vec<&'static str> = (0..50)
            .map(|i| &*Box::leak(format!("item-{i}").into_boxed_str()))
            .collect();
        let source = Literal(
            names
                .iter()
                .map(|n| leaf(n).with_detail_for_test())
                .collect(),
        );
        let mut view = view_over(&source, 6);
        view.set_bounds(Rect::new(20.0, 30.0, 180.0, 6.0 * 24.0));
        view.scroll_to(10);
        let cmds = view.render(&Palette::for_mode(false));
        let drawn: Vec<String> = texts(&cmds)
            .into_iter()
            .map(|(t, _, _)| t)
            .filter(|t| t.starts_with("item-"))
            .collect();
        assert_eq!(
            drawn,
            [
                "item-10", "item-11", "item-12", "item-13", "item-14", "item-15"
            ]
        );
        let b = view.bounds();
        for (text, x, max_width) in texts(&cmds) {
            let w = max_width.expect("every text in a tree row is bounded");
            assert!(
                x >= b.x && x + w <= b.right() + 0.01,
                "{text:?} escapes the tree: {x} + {w}"
            );
        }
        assert!(
            matches!(cmds.first(), Some(RenderCommand::PushClip { .. }))
                && matches!(cmds.last(), Some(RenderCommand::PopClip)),
            "clipped to its bounds"
        );
    }

    impl Node {
        fn with_detail_for_test(mut self) -> Self {
            self.detail = Some("12.0 KiB");
            self
        }
    }

    #[test]
    fn a_detail_gives_way_before_the_label_does() {
        let source = Literal(vec![
            leaf("a-rather-long-file-name.txt").with_detail_for_test(),
        ]);
        let mut view = view_over(&source, 3);
        let wide = texts(&view.render(&Palette::for_mode(false)));
        assert!(
            wide.iter().any(|(t, _, _)| t == "12.0 KiB"),
            "room for both"
        );
        view.set_bounds(Rect::new(0.0, 0.0, 70.0, 72.0));
        let narrow = texts(&view.render(&Palette::for_mode(false)));
        assert!(
            !narrow.iter().any(|(t, _, _)| t == "12.0 KiB"),
            "no room: the detail goes"
        );
        assert!(
            narrow
                .iter()
                .any(|(t, _, _)| t == "a-rather-long-file-name.txt")
        );
    }

    #[test]
    fn each_box_state_draws_differently_and_in_theme_colours() {
        let palette = Palette::for_mode(true);
        let draw = |state| {
            let mut ink = Vec::new();
            push_check_box(&mut ink, &palette, 0.0, 0.0, state, false);
            ink
        };
        let unchecked = draw(CheckState::Unchecked);
        let checked = draw(CheckState::Checked);
        let partial = draw(CheckState::Indeterminate);
        // `RenderCommand` has no `PartialEq`; its `Debug` form is exact.
        let shape = |ink: &[RenderCommand]| format!("{ink:?}");
        assert_ne!(shape(&unchecked), shape(&checked));
        assert_ne!(shape(&checked), shape(&partial));
        assert_ne!(shape(&unchecked), shape(&partial));
        assert!(
            !unchecked
                .iter()
                .any(|c| matches!(c, RenderCommand::FillRect { .. })),
            "an empty box is an outline"
        );
        assert!(checked.iter().any(
            |c| matches!(c, RenderCommand::FillRect { color, .. } if *color == palette.accent)
        ));
        assert!(checked.iter().any(
            |c| matches!(c, RenderCommand::Line { color, .. } if *color == palette.on_accent())
        ));
        assert!(partial.iter().any(
            |c| matches!(c, RenderCommand::FillRect { color, .. } if *color == palette.on_accent())
        ));
    }

    #[test]
    fn the_selected_row_is_drawn_as_the_selection_surface() {
        let source = sample();
        let mut view = view_over(&source, 10);
        let palette = Palette::for_mode(false);
        let before = view.render(&palette);
        view.select(Some(&["etc"]));
        let after = view.render(&palette);
        let mut expected = Vec::new();
        let row = Rect::new(0.0, 24.0, view.bounds().w, 24.0);
        palette.push_surface(
            &mut expected,
            row.x,
            row.y,
            row.w,
            row.h,
            0.0,
            Surface::Selected,
        );
        assert!(
            !expected.is_empty(),
            "the selection surface paints something in this theme"
        );
        let debug =
            |ink: &[RenderCommand]| ink.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>();
        let (before, after) = (debug(&before), debug(&after));
        for command in debug(&expected) {
            assert!(after.contains(&command), "selection surface drawn at row 1");
            assert!(
                !before.contains(&command),
                "and only once something is selected"
            );
        }
    }

    #[test]
    fn a_disabled_tree_draws_the_same_shapes_at_half_strength() {
        let source = sample();
        let mut view = view_over(&source, 10);
        let palette = Palette::for_mode(false);
        let normal = view.render(&palette);
        view.set_state(DisabledState::Disabled { reason: None });
        let greyed = view.render(&palette);
        assert_eq!(normal.len(), greyed.len());
        assert_ne!(format!("{normal:?}"), format!("{greyed:?}"));
    }

    #[test]
    fn every_hit_box_lies_inside_the_bounds() {
        let source = sample();
        let mut view = TreeView::checkable(false);
        view.set_bounds(Rect::new(50.0, 60.0, 150.0, 48.0));
        view.refresh(&source);
        view.set_expanded(&["home"], true, &source);
        let mut frame = Frame::new(400.0, 400.0);
        view.draw(&Palette::for_mode(false), &mut frame, |h| h);
        let b = view.bounds();
        assert!(frame.is_balanced());
        for (hit, r) in frame.hits() {
            assert!(
                r.x >= b.x
                    && r.y >= b.y
                    && r.right() <= b.right() + 0.01
                    && r.bottom() <= b.bottom() + 0.01,
                "{hit:?} at {r:?} is outside {b:?}"
            );
        }
        assert!(
            frame.hits().iter().any(|(h, _)| *h == TreeHit::ScrollThumb),
            "six rows in two rows' space scroll"
        );
    }

    #[test]
    fn an_empty_or_zero_sized_tree_draws_nothing_and_survives_input() {
        let source = Literal(Vec::new());
        let mut view = view_over(&source, 10);
        assert!(view.rows().is_empty());
        for k in [
            Key::Up,
            Key::Down,
            Key::Home,
            Key::End,
            Key::Left,
            Key::Right,
            Key::Enter,
            Key::Space,
        ] {
            assert!(view.handle_key(&key(k), &source).is_empty());
        }
        let populated = sample();
        let mut flat = view_over(&populated, 10);
        flat.set_bounds(Rect::new(0.0, 0.0, 0.0, 0.0));
        assert!(flat.render(&Palette::for_mode(false)).is_empty());
        assert_eq!(flat.capacity(), 0);
        flat.handle_key(&key(Key::PageDown), &populated);
        flat.handle_mouse(
            &mouse(0.0, 0.0, MouseEventKind::Press(MouseButton::Left)),
            &populated,
        );
    }

    #[test]
    fn random_input_never_breaks_the_views_invariants() {
        let mut source = sample();
        source.node_mut(&["home", "pics"]).unwrap().disabled = Some("No access");
        let mut view = TreeView::checkable(false);
        view.set_bounds(Rect::new(0.0, 0.0, 220.0, 24.0 * 4.0));
        view.refresh(&source);
        let keys = [
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Enter,
            Key::Space,
        ];
        let mut rng = SeededRng::new(0x5EED_7E57);
        for step in 0..2000 {
            if rng.below(3) == 0 {
                let x = px(rng.below(240));
                let y = px(rng.below(110));
                let kind = match rng.below(5) {
                    0 => MouseEventKind::Press(MouseButton::Left),
                    1 => MouseEventKind::DoubleClick(MouseButton::Left),
                    2 => MouseEventKind::Move,
                    3 => MouseEventKind::Release(MouseButton::Left),
                    _ => MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
                };
                view.handle_mouse(&mouse(x, y, kind), &source);
            } else {
                let k = keys[rng.below(keys.len())];
                view.handle_key(&key(k), &source);
            }
            // Invariants: the selection, if any, is a drawn row; the view is
            // not scrolled past its last page; every row's parent is drawn
            // above it and open.
            if let Some(sel) = view.selected() {
                assert!(
                    view.index_of(sel).is_some(),
                    "step {step}: selection {sel:?} not drawn"
                );
            }
            assert!(view.first_visible() <= view.rows().len().saturating_sub(view.capacity()));
            for (i, row) in view.rows().iter().enumerate() {
                if let Some((_, parent)) = row.path.split_last() {
                    if !parent.is_empty() {
                        let p = view.index_of(parent).expect("parent drawn");
                        assert!(p < i && view.is_expanded(parent));
                    }
                }
            }
        }
    }
}

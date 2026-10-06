//! Dockable panels: named panels in tab groups, the groups in nested splits,
//! rearranged by the user dragging tabs.
//!
//! The other half of `roadmap-detailed.md`'s *Dockable Panel / Splitter
//! Layout Widget*. [`crate::splitter`] divides an area and resizes the
//! division; this is what an application with many panels -- an editor's
//! files, outline and terminal, say -- keeps on top of it:
//!
//! - **a layout** ([`Dock`]): a tree whose leaves are tab groups and whose
//!   inner nodes are splits;
//! - **geometry** ([`Dock::layout`]): where every group, tab and divider is
//!   in a given area, and [`Layout::panels`], the rectangle each visible
//!   panel's contents go in;
//! - **rearranging** ([`Dock::drop_at`], [`Dock::apply`]): a tab dragged onto
//!   another group's tabs joins them; onto an edge of a group's contents, it
//!   splits that group; onto the edge of the whole dock, it takes a column or
//!   a row of its own;
//! - **opening and closing** panels ([`Dock::open`], [`Dock::close`]) and the
//!   menu that offers both ([`Dock::menu`]);
//! - **saving** ([`Dock::to_text`], [`Dock::from_text`]): one line of text the
//!   application keeps in its settings file. It survives a resize, because a
//!   layout is fractions rather than pixels, and it survives a panel the
//!   application no longer offers, which is dropped rather than refused;
//! - **input** ([`DockInput`]): a press, drag and release turned into a tab
//!   brought to the front, a panel closed, a divider moved or a panel moved;
//! - **drawing** ([`draw`]): each group's tab bar through [`TabView`], so a
//!   dock's tabs look like every other tab bar, the dividers, and where a
//!   dragged tab would land;
//! - **tools** ([`DockAccess`]): its groups, tabs, close buttons and dividers
//!   shown to automation and assistive tools, each used as the pointer
//!   uses it.
//!
//! Like the splitter this is state and functions over it, and every case is
//! testable headless. The application owns the [`Dock`] and the panels'
//! contents, and draws each visible panel into the rectangle it is given.
//!
//! # The tree stays tidy
//!
//! After every change: no empty group; no split with fewer than two
//! children; no split directly inside a split on the same axis (it is merged
//! into its parent, its fractions scaled); fractions positive and summing to
//! one; every group's front tab in range; no panel open twice. So two
//! arrangements that look the same are the same tree, and a saved
//! arrangement is always the same text.

use core::fmt;

use crate::event::{Key, KeyEvent};
use crate::frame::Rect;
use crate::layout::Axis;
use crate::palette::Palette;
use crate::render::RenderCommand;
use crate::splitter;
use crate::style::CornerRadii;
use crate::surface::CommandSink;
use crate::tabs::{TAB_BAR_HEIGHT, Tab, TabPosition, TabRect, TabView};

mod accessible;
pub use accessible::{DockAccess, DockPart};

/// How far the pointer must travel with a tab held before it is a drag and
/// not a click -- the toolkit's drag threshold, as `dnd` and `grid` use.
pub const DRAG_THRESHOLD: f32 = 5.0;

/// The smallest a group may be made, along either axis, by dragging a
/// divider: its tab bar and a little of its contents. At the default text
/// size: a drag holds groups to it at the user's, as the bar grows with it
/// ([`crate::text::scaled`]).
pub const MIN_GROUP: f32 = TAB_BAR_HEIGHT + 48.0;

/// The share of a group's contents, from each edge, where a dropped tab
/// splits the group rather than joining it.
pub const EDGE_SHARE: f32 = 0.25;

/// How close to the dock's own edge a dropped tab takes a column or a row
/// of the whole dock, rather than going into whatever group is there.
pub const DOCK_EDGE: f32 = 12.0;

/// How close to the dock's top edge it must be over a tab bar.
///
/// The top-row groups' tab bars run along the dock's top edge, and a tab bar
/// is the commonest place to drop a tab: at the full [`DOCK_EDGE`], the top
/// third of every such bar would take the whole dock's top row instead of a
/// place among its tabs. So over a bar the edge is a thin strip at the very
/// top, still reached by pushing the pointer up against it.
pub const DOCK_EDGE_OVER_BAR: f32 = 4.0;

/// The share of the dock a panel dropped on its edge is given.
pub const EDGE_FRACTION: f32 = 0.25;

// ============================================================================
// Panels
// ============================================================================

/// A panel's name within one dock.
///
/// Its kind -- which of the panels the application offers it is -- and, for
/// a kind that can be open more than once, which one: `terminal`,
/// `terminal#2`. Letters, digits, `_`, `-` and `.`, and at most one `#`
/// followed by a number from 2 up. The limit is what lets the saved text do
/// without quoting.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PanelId(String);

impl PanelId {
    /// `id` as a panel name, or `None` if it is not one.
    #[must_use]
    pub fn new(id: &str) -> Option<Self> {
        let (kind, instance) = match id.split_once('#') {
            Some((kind, n)) => (kind, Some(n)),
            None => (id, None),
        };
        let kind_ok = !kind.is_empty()
            && kind
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
        // `terminal#1` would be a second spelling of `terminal`, and a number
        // with a leading zero a second spelling of another number: one panel,
        // one name.
        let instance_ok = instance.is_none_or(|n| {
            !n.starts_with('0')
                && n.bytes().all(|b| b.is_ascii_digit())
                && n.parse::<u32>().is_ok_and(|v| v >= 2)
        });
        (kind_ok && instance_ok).then(|| Self(id.to_owned()))
    }

    /// The `n`th panel of `kind`: `kind` itself for the first.
    #[must_use]
    pub fn of_kind(kind: &str, n: u32) -> Option<Self> {
        if n <= 1 {
            Self::new(kind)
        } else {
            Self::new(&format!("{kind}#{n}"))
        }
    }

    /// Which kind of panel this is.
    #[must_use]
    pub fn kind(&self) -> &str {
        self.0
            .split_once('#')
            .map_or(self.0.as_str(), |(kind, _)| kind)
    }

    /// Which panel of its kind this is, counting from 1.
    #[must_use]
    pub fn instance(&self) -> u32 {
        self.0
            .split_once('#')
            .and_then(|(_, n)| n.parse().ok())
            .unwrap_or(1)
    }

    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PanelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A kind of panel the application offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelKind {
    /// Its name, which every [`PanelId`] of this kind starts with.
    pub kind: String,
    /// What its tab and its menu entry say.
    pub title: String,
    /// Whether more than one can be open at once.
    pub multiple: bool,
}

impl PanelKind {
    /// A kind of panel, open at most once.
    #[must_use]
    pub fn new(kind: &str, title: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            title: title.to_owned(),
            multiple: false,
        }
    }

    /// A kind of panel that can be open any number of times.
    #[must_use]
    pub fn multiple(kind: &str, title: &str) -> Self {
        Self {
            multiple: true,
            ..Self::new(kind, title)
        }
    }
}

/// What a panel's tab says: its kind's title, and for a second or later panel
/// of that kind, its number. A panel whose kind is not offered says its name.
#[must_use]
pub fn title_of(panel: &PanelId, kinds: &[PanelKind]) -> String {
    let Some(kind) = kinds.iter().find(|k| k.kind == panel.kind()) else {
        return panel.as_str().to_owned();
    };
    match panel.instance() {
        1 => kind.title.clone(),
        n => format!("{} {n}", kind.title),
    }
}

// ============================================================================
// The tree
// ============================================================================

/// The panels that share one region, and which of them is in front.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    panels: Vec<PanelId>,
    active: usize,
}

impl Group {
    fn of(panel: PanelId) -> Self {
        Self {
            panels: vec![panel],
            active: 0,
        }
    }

    /// The panels, in tab order.
    #[must_use]
    pub fn panels(&self) -> &[PanelId] {
        &self.panels
    }

    /// The panel in front.
    #[must_use]
    pub fn active(&self) -> Option<&PanelId> {
        self.panels.get(self.active)
    }
}

/// One node of a dock's layout.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// An area divided along `axis`, each child taking its fraction.
    Split {
        /// `Horizontal`: the children side by side. `Vertical`: stacked.
        axis: Axis,
        /// The children, in order along the axis.
        children: Vec<Node>,
        /// Each child's share, summing to one.
        fractions: Vec<f32>,
    },
    /// A tab group.
    Group(Group),
}

/// The side of a group, or of the whole dock, a panel is dropped against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// To the left.
    Left,
    /// To the right.
    Right,
    /// Above.
    Top,
    /// Below.
    Bottom,
}

impl Side {
    /// The axis a split for this side divides along.
    #[must_use]
    pub const fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Top | Self::Bottom => Axis::Vertical,
        }
    }

    /// Whether the new panel comes first along that axis.
    const fn first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

/// Where a dragged panel would go.
#[derive(Clone, Debug, PartialEq)]
pub enum Drop {
    /// Into a group's tabs, before the tab at `index` (`index` equal to the
    /// number of tabs: after the last).
    Tabs {
        /// The group, by its path from the root.
        group: Vec<usize>,
        /// Where among its tabs.
        index: usize,
    },
    /// Beside a group, splitting its region with it.
    Beside {
        /// The group, by its path from the root.
        group: Vec<usize>,
        /// Which side of it.
        side: Side,
    },
    /// Along an edge of the whole dock.
    Edge {
        /// Which edge.
        side: Side,
    },
}

/// A dock: its panels, their groups, and the splits between them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dock {
    root: Option<Node>,
}

impl Dock {
    /// A dock with nothing open.
    #[must_use]
    pub const fn new() -> Self {
        Self { root: None }
    }

    /// A dock holding `panels` as tabs of one group, the first in front.
    /// A panel named twice is kept once.
    #[must_use]
    pub fn with(panels: &[PanelId]) -> Self {
        let mut dock = Self::new();
        for panel in panels {
            dock.open(panel.clone(), None);
        }
        dock.activate_first();
        dock
    }

    fn activate_first(&mut self) {
        if let Some(Node::Group(g)) = &mut self.root {
            g.active = 0;
        }
    }

    /// The layout tree, for a caller that wants to walk it.
    #[must_use]
    pub const fn root(&self) -> Option<&Node> {
        self.root.as_ref()
    }

    /// Whether nothing is open.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Every open panel, group by group in tree order, each group's in tab
    /// order.
    #[must_use]
    pub fn panels(&self) -> Vec<&PanelId> {
        self.groups()
            .into_iter()
            .flat_map(|(_, g)| g.panels.iter())
            .collect()
    }

    /// Whether `panel` is open.
    #[must_use]
    pub fn contains(&self, panel: &PanelId) -> bool {
        self.find(panel).is_some()
    }

    /// Every group, with its path, in tree order: left to right, top to
    /// bottom.
    #[must_use]
    pub fn groups(&self) -> Vec<(Vec<usize>, &Group)> {
        fn walk<'a>(node: &'a Node, path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, &'a Group)>) {
            match node {
                Node::Group(g) => out.push((path.clone(), g)),
                Node::Split { children, .. } => {
                    for (i, child) in children.iter().enumerate() {
                        path.push(i);
                        walk(child, path, out);
                        path.pop();
                    }
                }
            }
        }
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            walk(root, &mut Vec::new(), &mut out);
        }
        out
    }

    /// The group holding `panel`, and the panel's place in it.
    fn find(&self, panel: &PanelId) -> Option<(Vec<usize>, usize)> {
        self.groups()
            .into_iter()
            .find_map(|(path, g)| g.panels.iter().position(|p| p == panel).map(|i| (path, i)))
    }

    fn node(&self, path: &[usize]) -> Option<&Node> {
        let mut node = self.root.as_ref()?;
        for &i in path {
            match node {
                Node::Split { children, .. } => node = children.get(i)?,
                Node::Group(_) => return None,
            }
        }
        Some(node)
    }

    fn node_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let mut node = self.root.as_mut()?;
        for &i in path {
            match node {
                Node::Split { children, .. } => node = children.get_mut(i)?,
                Node::Group(_) => return None,
            }
        }
        Some(node)
    }

    fn group_mut(&mut self, path: &[usize]) -> Option<&mut Group> {
        match self.node_mut(path)? {
            Node::Group(g) => Some(g),
            Node::Split { .. } => None,
        }
    }

    /// Restore every invariant in the module documentation.
    fn tidy(&mut self) {
        self.root = self.root.take().and_then(tidy);
    }

    // ------------------------------------------------------------------------
    // Opening, closing, the front tab
    // ------------------------------------------------------------------------

    /// Open `panel` as a tab beside `near` -- in `near`'s group, just after
    /// it -- or, with no `near` or one that is not open, in the first group.
    /// It comes to the front. Answers whether it was newly opened: a panel
    /// already open is only brought to the front.
    pub fn open(&mut self, panel: PanelId, near: Option<&PanelId>) -> bool {
        if self.contains(&panel) {
            self.activate(&panel);
            return false;
        }
        let place = near
            .and_then(|n| self.find(n))
            .map(|(path, i)| (path, i.saturating_add(1)))
            .or_else(|| {
                self.groups()
                    .first()
                    .map(|(path, g)| (path.clone(), g.panels.len()))
            });
        match place.and_then(|(path, at)| Some((self.group_mut(&path)?, at))) {
            Some((group, at)) => {
                let at = at.min(group.panels.len());
                group.panels.insert(at, panel);
                group.active = at;
            }
            None => self.root = Some(Node::Group(Group::of(panel))),
        }
        true
    }

    /// Close `panel`. Answers whether it was open.
    pub fn close(&mut self, panel: &PanelId) -> bool {
        let Some((path, i)) = self.find(panel) else {
            return false;
        };
        if let Some(group) = self.group_mut(&path) {
            group.panels.remove(i);
            // The tab that slides into its place -- the one to its right --
            // comes forward, or the new last when it was the last: what the
            // toolkit's tab bar does (`TabView::remove_tab`), so a dock's tabs
            // close the way every other tab bar's do.
            if group.active > i || group.active >= group.panels.len() {
                group.active = group.active.saturating_sub(1);
            }
        }
        self.tidy();
        true
    }

    /// Bring `panel` to the front of its group. Answers whether anything
    /// changed.
    pub fn activate(&mut self, panel: &PanelId) -> bool {
        let Some((path, i)) = self.find(panel) else {
            return false;
        };
        match self.group_mut(&path) {
            Some(group) if group.active != i => {
                group.active = i;
                true
            }
            _ => false,
        }
    }

    /// Bring the next tab of `panel`'s group to the front -- or the previous,
    /// with `forward` false -- wrapping at both ends: Ctrl+Tab and
    /// Ctrl+Shift+Tab. Answers the panel now in front.
    pub fn cycle(&mut self, panel: &PanelId, forward: bool) -> Option<PanelId> {
        let (path, _) = self.find(panel)?;
        let group = self.group_mut(&path)?;
        let n = group.panels.len();
        group.active = if forward {
            crate::step::wrapping_after(n, group.active)
        } else {
            crate::step::wrapping_before(n, group.active)
        };
        group.active().cloned()
    }

    /// The keys a dock answers for the panel that has the keyboard: Ctrl+Tab
    /// and Ctrl+Shift+Tab bring the next and previous tab of its group
    /// forward, wrapping, as they do in the toolkit's tab bar. Anything else
    /// is the application's -- Ctrl+W among it, which an application may well
    /// want for its own documents rather than for the panel around them.
    pub fn handle_key(&mut self, focused: &PanelId, key: &KeyEvent) -> DockEvent {
        if !key.pressed || key.key != Key::Tab || !key.modifiers.ctrl || key.modifiers.alt {
            return DockEvent::None;
        }
        match self.cycle(focused, !key.modifiers.shift) {
            Some(front) if front != *focused => DockEvent::Activated(front),
            _ => DockEvent::None,
        }
    }

    // ------------------------------------------------------------------------
    // Moving
    // ------------------------------------------------------------------------

    /// Move `panel` as `drop` says. Answers whether anything moved.
    ///
    /// The target is re-found after the panel leaves its old place, by a
    /// panel of the target group rather than by the path `drop` carries:
    /// leaving can empty a group and collapse the split around it, which
    /// changes every path after it.
    pub fn apply(&mut self, panel: &PanelId, drop: &Drop) -> bool {
        let Some((from, from_index)) = self.find(panel) else {
            return false;
        };
        match drop {
            Drop::Tabs { group, index } if *group == from => {
                // A move within one tab bar: a reorder, which cannot change
                // the tree's shape.
                let Some(g) = self.group_mut(&from) else {
                    return false;
                };
                let to = if *index > from_index {
                    index.saturating_sub(1)
                } else {
                    *index
                }
                .min(g.panels.len().saturating_sub(1));
                if to == from_index {
                    return false;
                }
                let moved = g.panels.remove(from_index);
                g.panels.insert(to, moved);
                g.active = to;
                true
            }
            Drop::Tabs { group, index } => {
                let Some(anchor) = self.anchor(group, panel) else {
                    return false;
                };
                let index = *index;
                self.lift(panel);
                let Some((path, _)) = self.find(&anchor) else {
                    return false;
                };
                let Some(g) = self.group_mut(&path) else {
                    return false;
                };
                let at = index.min(g.panels.len());
                g.panels.insert(at, panel.clone());
                g.active = at;
                true
            }
            Drop::Beside { group, side } => {
                let Some(anchor) = self.anchor(group, panel) else {
                    return false;
                };
                self.lift(panel);
                let Some((path, _)) = self.find(&anchor) else {
                    return false;
                };
                self.split_beside(&path, panel.clone(), *side);
                true
            }
            Drop::Edge { side } => {
                if self.panels().len() <= 1 {
                    return false;
                }
                self.lift(panel);
                self.dock_edge(panel.clone(), *side);
                true
            }
        }
    }

    /// A panel of the group at `path` other than `moving`: the target's
    /// identity, which survives the tree changing shape underneath it.
    fn anchor(&self, path: &[usize], moving: &PanelId) -> Option<PanelId> {
        match self.node(path)? {
            Node::Group(g) => g.panels.iter().find(|p| *p != moving).cloned(),
            Node::Split { .. } => None,
        }
    }

    /// Take `panel` out of the tree, tidying behind it.
    fn lift(&mut self, panel: &PanelId) {
        if let Some((path, i)) = self.find(panel)
            && let Some(group) = self.group_mut(&path)
        {
            group.panels.remove(i);
            if group.active > i || group.active >= group.panels.len() {
                group.active = group.active.saturating_sub(1);
            }
        }
        self.tidy();
    }

    /// Put `panel` in a group of its own on `side` of the group at `path`.
    fn split_beside(&mut self, path: &[usize], panel: PanelId, side: Side) {
        let axis = side.axis();
        let new = Node::Group(Group::of(panel));
        // Beside a group already in a split along the same axis: a sibling,
        // taking half the group's share. Nesting a second split on the same
        // axis would only be flattened back into this one.
        if let Some((&last, parent)) = path.split_last()
            && let Some(Node::Split {
                axis: parent_axis,
                children,
                fractions,
            }) = self.node_mut(parent)
            && *parent_axis == axis
        {
            let share = fractions.get(last).copied().unwrap_or(0.0) / 2.0;
            if let Some(f) = fractions.get_mut(last) {
                *f = share;
            }
            let at = if side.first() {
                last
            } else {
                last.saturating_add(1)
            };
            children.insert(at.min(children.len()), new);
            fractions.insert(at.min(fractions.len()), share);
            self.tidy();
            return;
        }
        if let Some(target) = self.node_mut(path) {
            let old = core::mem::replace(
                target,
                Node::Group(Group {
                    panels: Vec::new(),
                    active: 0,
                }),
            );
            let children = if side.first() {
                vec![new, old]
            } else {
                vec![old, new]
            };
            *target = Node::Split {
                axis,
                children,
                fractions: vec![0.5, 0.5],
            };
        }
        self.tidy();
    }

    /// Put `panel` in a group of its own along `side` of the whole dock.
    fn dock_edge(&mut self, panel: PanelId, side: Side) {
        let new = Node::Group(Group::of(panel));
        let axis = side.axis();
        self.root = match self.root.take() {
            None => Some(new),
            Some(Node::Split {
                axis: root_axis,
                mut children,
                fractions,
            }) if root_axis == axis => {
                let mut fractions: Vec<f32> = fractions
                    .into_iter()
                    .map(|f| f * (1.0 - EDGE_FRACTION))
                    .collect();
                if side.first() {
                    children.insert(0, new);
                    fractions.insert(0, EDGE_FRACTION);
                } else {
                    children.push(new);
                    fractions.push(EDGE_FRACTION);
                }
                Some(Node::Split {
                    axis,
                    children,
                    fractions,
                })
            }
            Some(old) => Some(if side.first() {
                Node::Split {
                    axis,
                    children: vec![new, old],
                    fractions: vec![EDGE_FRACTION, 1.0 - EDGE_FRACTION],
                }
            } else {
                Node::Split {
                    axis,
                    children: vec![old, new],
                    fractions: vec![1.0 - EDGE_FRACTION, EDGE_FRACTION],
                }
            }),
        };
        self.tidy();
    }

    // ------------------------------------------------------------------------
    // Geometry
    // ------------------------------------------------------------------------

    /// Where everything is, laid out in `area`. Tab widths come from the
    /// panels' titles, so `kinds` is needed to measure them.
    #[must_use]
    pub fn layout(&self, area: Rect, kinds: &[PanelKind]) -> Layout {
        let mut layout = Layout {
            area,
            groups: Vec::new(),
            dividers: Vec::new(),
        };
        if let Some(root) = &self.root {
            lay_out(root, area, kinds, &mut Vec::new(), &mut layout);
        }
        layout
    }

    /// What is under `(x, y)`: a divider first, because its grab region
    /// reaches over the panes either side of it.
    #[must_use]
    pub fn hit(&self, layout: &Layout, x: f32, y: f32) -> Option<DockHit> {
        if let Some(d) = layout.divider_at(x, y) {
            return Some(DockHit::Divider {
                split: d.split.clone(),
                index: d.index,
            });
        }
        let g = layout.groups.iter().find(|g| g.rect.contains(x, y))?;
        let group = match self.node(&g.path)? {
            Node::Group(group) => group,
            Node::Split { .. } => return None,
        };
        if g.strip.contains(x, y) {
            for (t, panel) in g.tabs.iter().zip(&group.panels) {
                // A tab scrolled past the bar's end is not under the pointer
                // even if its rectangle says so.
                if !t.rect.contains(x, y) {
                    continue;
                }
                if t.close.is_some_and(|c| c.contains(x, y)) {
                    return Some(DockHit::Close {
                        panel: panel.clone(),
                    });
                }
                return Some(DockHit::Tab {
                    panel: panel.clone(),
                });
            }
            return Some(DockHit::Strip {
                group: g.path.clone(),
            });
        }
        group.active().map(|panel| DockHit::Content {
            panel: panel.clone(),
        })
    }

    /// Where `panel` would go if let go at `(x, y)`, or `None` where it would
    /// go nowhere -- including every place that would leave it where it is.
    #[must_use]
    pub fn drop_at(&self, layout: &Layout, panel: &PanelId, x: f32, y: f32) -> Option<Drop> {
        let area = layout.area;
        if !area.contains(x, y) {
            return None;
        }
        let (own, own_index) = self.find(panel)?;
        let own_len = match self.node(&own)? {
            Node::Group(g) => g.panels.len(),
            Node::Split { .. } => return None,
        };
        let alone = self.panels().len() <= 1;

        // The dock's own edge: the nearest one within reach.
        let over_bar = layout.groups.iter().any(|g| g.strip.contains(x, y));
        let reach = |side: Side| {
            if side == Side::Top && over_bar {
                DOCK_EDGE_OVER_BAR
            } else {
                DOCK_EDGE
            }
        };
        let edges = [
            (x - area.x, Side::Left),
            (area.right() - x, Side::Right),
            (y - area.y, Side::Top),
            (area.bottom() - y, Side::Bottom),
        ];
        if let Some(&(_, side)) = edges
            .iter()
            .filter(|(d, side)| *d < reach(*side))
            .min_by(|a, b| a.0.total_cmp(&b.0))
        {
            return (!alone).then_some(Drop::Edge { side });
        }

        let g = layout.groups.iter().find(|g| g.rect.contains(x, y))?;
        let is_own = g.path == own;
        if g.strip.contains(x, y) {
            // Before the first tab whose middle is past the pointer.
            let index = g
                .tabs
                .iter()
                .position(|t| x < t.rect.x + t.rect.w / 2.0)
                .unwrap_or(g.tabs.len());
            let stays = is_own && (index == own_index || index == own_index.saturating_add(1));
            return (!stays).then(|| Drop::Tabs {
                group: g.path.clone(),
                index,
            });
        }
        let c = g.content;
        if c.w <= 0.0 || c.h <= 0.0 || !c.contains(x, y) {
            return None;
        }
        let fx = (x - c.x) / c.w;
        let fy = (y - c.y) / c.h;
        let sides = [
            (fx, Side::Left),
            (1.0 - fx, Side::Right),
            (fy, Side::Top),
            (1.0 - fy, Side::Bottom),
        ];
        if let Some(&(d, side)) = sides.iter().min_by(|a, b| a.0.total_cmp(&b.0))
            && d < EDGE_SHARE
        {
            // Beside its own group when it is that group's only tab would
            // split the group from itself.
            return (!(is_own && own_len == 1)).then(|| Drop::Beside {
                group: g.path.clone(),
                side,
            });
        }
        (!is_own).then(|| Drop::Tabs {
            group: g.path.clone(),
            index: g.tabs.len(),
        })
    }

    /// Move divider `index` of the split at `split` to the pointer, keeping
    /// every group at least [`MIN_GROUP`]. Answers whether it moved.
    pub fn resize(
        &mut self,
        layout: &Layout,
        split: &[usize],
        index: usize,
        x: f32,
        y: f32,
    ) -> bool {
        let Some(d) = layout
            .dividers
            .iter()
            .find(|d| d.split == split && d.index == index)
        else {
            return false;
        };
        let (position, span) = match d.axis {
            Axis::Horizontal => (x - d.area.x, d.area.w),
            Axis::Vertical => (y - d.area.y, d.area.h),
        };
        match self.node_mut(split) {
            Some(Node::Split { fractions, .. }) => {
                let min = vec![crate::text::scaled(MIN_GROUP); fractions.len()];
                splitter::resize(fractions, index, position, span, splitter::DIVIDER, &min)
            }
            _ => false,
        }
    }

    // ------------------------------------------------------------------------
    // The panel menu
    // ------------------------------------------------------------------------

    /// The entries of a menu that opens and closes panels: one per kind the
    /// application offers. A kind open at most once is ticked while open and
    /// toggles; a kind that can be open many times opens another.
    #[must_use]
    pub fn menu(&self, kinds: &[PanelKind]) -> Vec<PanelMenuEntry> {
        kinds
            .iter()
            .filter_map(|kind| {
                // A kind that is not a panel name offers nothing. Checked
                // before the search below, which would otherwise ask four
                // billion numbers for a name none of them can make.
                let first = PanelId::new(&kind.kind)
                    .filter(|id| id.instance() == 1 && !kind.kind.contains('#'))?;
                if kind.multiple {
                    let next = (1..=u32::MAX)
                        .map_while(|n| PanelId::of_kind(&kind.kind, n))
                        .find(|id| !self.contains(id))?;
                    Some(PanelMenuEntry {
                        label: format!("New {}", kind.title),
                        checked: false,
                        action: PanelAction::Open(next),
                    })
                } else {
                    let id = first;
                    let open = self.contains(&id);
                    Some(PanelMenuEntry {
                        label: kind.title.clone(),
                        checked: open,
                        action: if open {
                            PanelAction::Close(id)
                        } else {
                            PanelAction::Open(id)
                        },
                    })
                }
            })
            .collect()
    }

    /// Do what a menu entry says, opening beside `near`. Answers whether
    /// anything changed.
    pub fn perform(&mut self, action: &PanelAction, near: Option<&PanelId>) -> bool {
        match action {
            PanelAction::Open(id) => self.open(id.clone(), near),
            PanelAction::Close(id) => self.close(id),
        }
    }

    // ------------------------------------------------------------------------
    // Saving
    // ------------------------------------------------------------------------

    /// The arrangement as one line of text, for a settings file:
    /// `h(0.25:[files],0.75:v(0.7:[*editor,outline],0.3:[terminal]))`.
    ///
    /// `h(..)` and `v(..)` are splits -- side by side and stacked -- each
    /// child after its share; `[..]` is a tab group, `*` marking the tab in
    /// front. An empty dock is the empty string.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        if let Some(root) = &self.root {
            write_node(root, &mut out);
        }
        out
    }

    /// The arrangement `text` describes, keeping only the panels whose kind
    /// `known` accepts: a panel the application no longer offers is dropped,
    /// and the layout closes up around it, rather than the whole saved
    /// arrangement being thrown away over one line of it.
    ///
    /// # Errors
    ///
    /// [`DockTextError`] for text that is not an arrangement at all -- the
    /// caller's default layout is then the right answer.
    pub fn from_text(text: &str, known: impl Fn(&str) -> bool) -> Result<Self, DockTextError> {
        let mut parser = Parser {
            text: text.as_bytes(),
            at: 0,
        };
        parser.skip_space();
        if parser.done() {
            return Ok(Self::new());
        }
        let node = parser.node(0)?;
        parser.skip_space();
        if !parser.done() {
            return Err(parser.error("text after the arrangement"));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut dock = Self {
            root: Some(keep_known(node, &known, &mut seen)),
        };
        dock.tidy();
        Ok(dock)
    }
}

/// Every invariant in the module documentation, for one subtree.
fn tidy(node: Node) -> Option<Node> {
    match node {
        Node::Group(mut g) => {
            let last = g.panels.len().checked_sub(1)?;
            g.active = g.active.min(last);
            Some(Node::Group(g))
        }
        Node::Split {
            axis,
            children,
            fractions,
        } => {
            let mut kids: Vec<(Node, f32)> = Vec::new();
            for (child, share) in children.into_iter().zip(fractions) {
                let share = if share.is_finite() && share > 0.0 {
                    share
                } else {
                    0.0
                };
                match tidy(child) {
                    None => {}
                    Some(Node::Split {
                        axis: child_axis,
                        children: grandchildren,
                        fractions: shares,
                    }) if child_axis == axis => {
                        for (g, s) in grandchildren.into_iter().zip(shares) {
                            kids.push((g, share * s));
                        }
                    }
                    Some(other) => kids.push((other, share)),
                }
            }
            match kids.len() {
                0 => None,
                1 => kids.pop().map(|(node, _)| node),
                n => {
                    // A child with no share would be drawn with no size and
                    // could not be dragged back open: it gets an equal share
                    // instead, as a new split would give it.
                    let even = 1.0 / n as f32;
                    for kid in &mut kids {
                        if kid.1 <= 0.0 {
                            kid.1 = even;
                        }
                    }
                    let total: f32 = kids.iter().map(|k| k.1).sum();
                    let (children, fractions) =
                        kids.into_iter().map(|(c, s)| (c, s / total)).unzip();
                    Some(Node::Split {
                        axis,
                        children,
                        fractions,
                    })
                }
            }
        }
    }
}

/// `node` with only the panels whose kind is `known`, each once. What that
/// empties, [`tidy`] removes.
fn keep_known(
    node: Node,
    known: &impl Fn(&str) -> bool,
    seen: &mut std::collections::BTreeSet<PanelId>,
) -> Node {
    match node {
        Node::Group(g) => {
            let front = g.panels.get(g.active).cloned();
            let panels: Vec<PanelId> = g
                .panels
                .into_iter()
                .filter(|p| known(p.kind()) && seen.insert(p.clone()))
                .collect();
            let active = front
                .and_then(|f| panels.iter().position(|p| *p == f))
                .unwrap_or(0);
            Node::Group(Group { panels, active })
        }
        Node::Split {
            axis,
            children,
            fractions,
        } => {
            let (children, fractions) = children
                .into_iter()
                .zip(fractions)
                .map(|(c, f)| (keep_known(c, known, seen), f))
                .unzip();
            Node::Split {
                axis,
                children,
                fractions,
            }
        }
    }
}

fn write_node(node: &Node, out: &mut String) {
    use core::fmt::Write as _;
    match node {
        Node::Group(g) => {
            out.push('[');
            for (i, panel) in g.panels.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if i == g.active {
                    out.push('*');
                }
                out.push_str(panel.as_str());
            }
            out.push(']');
        }
        Node::Split {
            axis,
            children,
            fractions,
        } => {
            out.push(match axis {
                Axis::Horizontal => 'h',
                Axis::Vertical => 'v',
            });
            out.push('(');
            for (i, (child, share)) in children.iter().zip(fractions).enumerate() {
                if i > 0 {
                    out.push(',');
                }
                // Four places: a ten-thousandth of a 4K screen is well under a
                // pixel, and the text stays short enough to read.
                let text = format!("{share:.4}");
                let text = text.trim_end_matches('0').trim_end_matches('.');
                let _ = write!(out, "{text}:");
                write_node(child, out);
            }
            out.push(')');
        }
    }
}

/// Why text is not an arrangement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockTextError {
    /// The byte offset where reading stopped.
    pub at: usize,
    /// What was expected there.
    pub what: &'static str,
}

impl fmt::Display for DockTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "not a panel arrangement at byte {}: {}",
            self.at, self.what
        )
    }
}

impl std::error::Error for DockTextError {}

/// How deep a saved arrangement may nest. Far past any real layout, and what
/// keeps a hostile settings file from exhausting the stack.
const MAX_DEPTH: usize = 32;

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn done(&self) -> bool {
        self.at >= self.text.len()
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.at = self.at.saturating_add(1);
        }
    }

    fn error(&self, what: &'static str) -> DockTextError {
        DockTextError { at: self.at, what }
    }

    fn expect(&mut self, byte: u8, what: &'static str) -> Result<(), DockTextError> {
        self.skip_space();
        if self.peek() == Some(byte) {
            self.at = self.at.saturating_add(1);
            Ok(())
        } else {
            Err(self.error(what))
        }
    }

    fn node(&mut self, depth: usize) -> Result<Node, DockTextError> {
        if depth > MAX_DEPTH {
            return Err(self.error("an arrangement nested this deep"));
        }
        self.skip_space();
        match self.peek() {
            Some(b'[') => self.group(),
            Some(b'h' | b'v') => self.split(depth),
            _ => Err(self.error("`[` for a tab group, or `h(`/`v(` for a split")),
        }
    }

    fn group(&mut self) -> Result<Node, DockTextError> {
        self.expect(b'[', "`[`")?;
        let mut panels = Vec::new();
        let mut active = 0;
        loop {
            self.skip_space();
            let front = self.peek() == Some(b'*');
            if front {
                self.at = self.at.saturating_add(1);
                active = panels.len();
            }
            let start = self.at;
            while self.peek().is_some_and(|b| {
                b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'#')
            }) {
                self.at = self.at.saturating_add(1);
            }
            let id = self
                .text
                .get(start..self.at)
                .and_then(|b| core::str::from_utf8(b).ok())
                .and_then(PanelId::new)
                .ok_or(DockTextError {
                    at: start,
                    what: "a panel name",
                })?;
            panels.push(id);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at = self.at.saturating_add(1),
                Some(b']') => {
                    self.at = self.at.saturating_add(1);
                    return Ok(Node::Group(Group { panels, active }));
                }
                _ => return Err(self.error("`,` or `]`")),
            }
        }
    }

    fn split(&mut self, depth: usize) -> Result<Node, DockTextError> {
        let axis = match self.peek() {
            Some(b'h') => Axis::Horizontal,
            _ => Axis::Vertical,
        };
        self.at = self.at.saturating_add(1);
        self.expect(b'(', "`(`")?;
        let mut children = Vec::new();
        let mut fractions = Vec::new();
        loop {
            self.skip_space();
            let start = self.at;
            while self.peek().is_some_and(|b| b.is_ascii_digit() || b == b'.') {
                self.at = self.at.saturating_add(1);
            }
            let share: f32 = self
                .text
                .get(start..self.at)
                .and_then(|b| core::str::from_utf8(b).ok())
                .and_then(|s| s.parse().ok())
                .filter(|v: &f32| v.is_finite())
                .ok_or(DockTextError {
                    at: start,
                    what: "a share, such as 0.5",
                })?;
            self.expect(b':', "`:` after a share")?;
            children.push(self.node(depth.saturating_add(1))?);
            fractions.push(share);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at = self.at.saturating_add(1),
                Some(b')') => {
                    self.at = self.at.saturating_add(1);
                    return Ok(Node::Split {
                        axis,
                        children,
                        fractions,
                    });
                }
                _ => return Err(self.error("`,` or `)`")),
            }
        }
    }
}

// ============================================================================
// Geometry
// ============================================================================

/// Where one group is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupLayout {
    /// The group, by its path from the root.
    pub path: Vec<usize>,
    /// The whole group: tab bar and contents.
    pub rect: Rect,
    /// Its tab bar.
    pub strip: Rect,
    /// Where the front panel's contents go.
    pub content: Rect,
    /// Its tabs, in order, as the bar draws them; each `id` is the tab's
    /// index.
    pub tabs: Vec<TabRect>,
}

/// Where one divider is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct DividerLayout {
    /// The split it divides, by its path from the root.
    pub split: Vec<usize>,
    /// It lies between child `index` and child `index + 1`.
    pub index: usize,
    /// The split's axis.
    pub axis: Axis,
    /// The divider as drawn.
    pub rect: Rect,
    /// The split's whole area, which a drag's position is measured in.
    pub area: Rect,
}

/// Where everything in a dock is, for one area.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// The area laid out.
    pub area: Rect,
    /// Every group, in tree order.
    pub groups: Vec<GroupLayout>,
    /// Every divider.
    pub dividers: Vec<DividerLayout>,
}

impl Layout {
    /// Every visible panel -- each group's front tab -- and the rectangle its
    /// contents go in.
    #[must_use]
    pub fn panels<'a>(&self, dock: &'a Dock) -> Vec<(&'a PanelId, Rect)> {
        self.groups
            .iter()
            .filter_map(|g| match dock.node(&g.path)? {
                Node::Group(group) => Some((group.active()?, g.content)),
                Node::Split { .. } => None,
            })
            .collect()
    }

    /// The divider whose grab region holds `(x, y)`, the nearest when two
    /// overlap.
    #[must_use]
    pub fn divider_at(&self, x: f32, y: f32) -> Option<&DividerLayout> {
        self.dividers
            .iter()
            .filter_map(|d| {
                let r = d.rect;
                let (inside, distance) = match d.axis {
                    Axis::Horizontal => (
                        y >= r.y
                            && y < r.bottom()
                            && (x - (r.x + r.w / 2.0)).abs() <= r.w / 2.0 + splitter::GRAB_MARGIN,
                        (x - (r.x + r.w / 2.0)).abs(),
                    ),
                    Axis::Vertical => (
                        x >= r.x
                            && x < r.right()
                            && (y - (r.y + r.h / 2.0)).abs() <= r.h / 2.0 + splitter::GRAB_MARGIN,
                        (y - (r.y + r.h / 2.0)).abs(),
                    ),
                };
                inside.then_some((d, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(d, _)| d)
    }
}

fn lay_out(node: &Node, area: Rect, kinds: &[PanelKind], path: &mut Vec<usize>, out: &mut Layout) {
    match node {
        Node::Group(group) => {
            let bar = crate::tabs::bar_height().min(area.h);
            let strip = Rect::new(area.x, area.y, area.w, bar);
            let content = Rect::new(area.x, area.y + bar, area.w, (area.h - bar).max(0.0));
            let tabs = tab_view(group, kinds).tab_rects(strip.x, strip.y);
            out.groups.push(GroupLayout {
                path: path.clone(),
                rect: area,
                strip,
                content,
                tabs,
            });
        }
        Node::Split {
            axis,
            children,
            fractions,
        } => {
            let panes = splitter::panes(area, *axis, fractions, splitter::DIVIDER);
            for (i, rect) in splitter::dividers(area, *axis, fractions, splitter::DIVIDER)
                .into_iter()
                .enumerate()
            {
                out.dividers.push(DividerLayout {
                    split: path.clone(),
                    index: i,
                    axis: *axis,
                    rect,
                    area,
                });
            }
            for (i, (child, pane)) in children.iter().zip(panes).enumerate() {
                path.push(i);
                lay_out(child, pane, kinds, path, out);
                path.pop();
            }
        }
    }
}

/// The tab bar a group is drawn with: each tab's id is its index.
fn tab_view(group: &Group, kinds: &[PanelKind]) -> TabView {
    let mut view = TabView::new(TabPosition::Top);
    for (i, panel) in group.panels.iter().enumerate() {
        view.add_tab(Tab::new(i as u64, title_of(panel, kinds)));
    }
    view.set_active(group.active as u64);
    view
}

// ============================================================================
// Input
// ============================================================================

/// What is under the pointer.
#[derive(Clone, Debug, PartialEq)]
pub enum DockHit {
    /// A divider: drag it to resize.
    Divider {
        /// The split it divides.
        split: Vec<usize>,
        /// Which of its dividers.
        index: usize,
    },
    /// A tab: click it to bring its panel forward, drag it to move it.
    Tab {
        /// Its panel.
        panel: PanelId,
    },
    /// A tab's close button.
    Close {
        /// The panel it closes.
        panel: PanelId,
    },
    /// The empty end of a tab bar.
    Strip {
        /// The group.
        group: Vec<usize>,
    },
    /// A visible panel's contents: the application's to handle.
    Content {
        /// The panel.
        panel: PanelId,
    },
}

/// What a press, drag or release did.
#[derive(Clone, Debug, PartialEq)]
pub enum DockEvent {
    /// Nothing the application needs to know.
    None,
    /// A panel came to the front of its group.
    Activated(PanelId),
    /// A panel was closed.
    Closed(PanelId),
    /// A panel moved somewhere else: save the arrangement.
    Moved(PanelId),
    /// A divider moved: save the arrangement.
    Resized,
    /// A press on a panel's contents, which the dock does not handle.
    Content(PanelId),
}

#[derive(Clone, Debug, PartialEq)]
enum Press {
    Tab {
        panel: PanelId,
        from: (f32, f32),
        dragging: bool,
    },
    Close(PanelId),
    Divider {
        split: Vec<usize>,
        index: usize,
    },
}

/// The pointer's hold on a dock, from a press to its release.
///
/// Feed it the application's mouse events with the dock and the layout they
/// were measured against; it answers what changed. A tab pressed and let go
/// where it was comes to the front; a tab dragged past [`DRAG_THRESHOLD`] is
/// moved where [`Dock::drop_at`] says on release, and [`DockInput::preview`]
/// says where while it is held; a close button closes its panel when a press
/// on it is also released on it -- so sliding off it is a way to not close.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DockInput {
    press: Option<Press>,
}

impl DockInput {
    /// A dock nobody is holding.
    #[must_use]
    pub const fn new() -> Self {
        Self { press: None }
    }

    /// Whether a tab is being dragged.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        matches!(self.press, Some(Press::Tab { dragging: true, .. }))
    }

    /// The pointer went down at `(x, y)`.
    pub fn press(&mut self, dock: &Dock, layout: &Layout, x: f32, y: f32) -> DockEvent {
        self.press = None;
        match dock.hit(layout, x, y) {
            Some(DockHit::Divider { split, index }) => {
                self.press = Some(Press::Divider { split, index });
                DockEvent::None
            }
            Some(DockHit::Tab { panel }) => {
                self.press = Some(Press::Tab {
                    panel,
                    from: (x, y),
                    dragging: false,
                });
                DockEvent::None
            }
            Some(DockHit::Close { panel }) => {
                self.press = Some(Press::Close(panel));
                DockEvent::None
            }
            Some(DockHit::Content { panel }) => DockEvent::Content(panel),
            Some(DockHit::Strip { .. }) | None => DockEvent::None,
        }
    }

    /// The pointer moved to `(x, y)` with the button down.
    pub fn drag_to(&mut self, dock: &mut Dock, layout: &Layout, x: f32, y: f32) -> DockEvent {
        match &mut self.press {
            Some(Press::Divider { split, index }) => {
                if dock.resize(layout, split, *index, x, y) {
                    DockEvent::Resized
                } else {
                    DockEvent::None
                }
            }
            Some(Press::Tab { from, dragging, .. }) => {
                if !*dragging && (x - from.0).hypot(y - from.1) >= DRAG_THRESHOLD {
                    *dragging = true;
                }
                DockEvent::None
            }
            Some(Press::Close(_)) | None => DockEvent::None,
        }
    }

    /// Where the tab being dragged would go if let go at `(x, y)`.
    #[must_use]
    pub fn preview(&self, dock: &Dock, layout: &Layout, x: f32, y: f32) -> Option<Drop> {
        match &self.press {
            Some(Press::Tab {
                panel,
                dragging: true,
                ..
            }) => dock.drop_at(layout, panel, x, y),
            _ => None,
        }
    }

    /// The pointer came up at `(x, y)`.
    pub fn release(&mut self, dock: &mut Dock, layout: &Layout, x: f32, y: f32) -> DockEvent {
        match self.press.take() {
            Some(Press::Tab {
                panel,
                dragging: false,
                ..
            }) => {
                dock.activate(&panel);
                DockEvent::Activated(panel)
            }
            Some(Press::Tab {
                panel,
                dragging: true,
                ..
            }) => match dock.drop_at(layout, &panel, x, y) {
                Some(drop) if dock.apply(&panel, &drop) => DockEvent::Moved(panel),
                _ => DockEvent::None,
            },
            Some(Press::Close(panel)) => {
                if dock.hit(layout, x, y)
                    == Some(DockHit::Close {
                        panel: panel.clone(),
                    })
                    && dock.close(&panel)
                {
                    DockEvent::Closed(panel)
                } else {
                    DockEvent::None
                }
            }
            Some(Press::Divider { .. }) | None => DockEvent::None,
        }
    }

    /// Let go of whatever is held, changing nothing more: Escape, or the
    /// pointer lost.
    pub fn cancel(&mut self) {
        self.press = None;
    }
}

// ============================================================================
// Drawing
// ============================================================================

/// The alpha of the wash over where a dragged tab would land: enough to see
/// on either theme, faint enough to read the panel under it through.
pub const DROP_WASH_ALPHA: u8 = 56;

/// Draw the dock: each group's tab bar, the dividers, and -- while a tab is
/// dragged -- where it would land. The panels' contents are the caller's to
/// draw, into [`Layout::panels`].
pub fn draw<S: CommandSink + ?Sized>(
    sink: &mut S,
    p: &Palette,
    dock: &Dock,
    layout: &Layout,
    kinds: &[PanelKind],
    drop: Option<&Drop>,
) {
    for g in &layout.groups {
        let Some(Node::Group(group)) = dock.node(&g.path) else {
            continue;
        };
        let (commands, _, _) =
            tab_view(group, kinds).render(p, g.rect.x, g.rect.y, g.rect.w, g.rect.h);
        for command in commands {
            sink.emit(command);
        }
    }
    for d in &layout.dividers {
        // The explorer's divider, which is the splitter's: surface2, drawn
        // over both panes' edges.
        sink.emit(RenderCommand::FillRect {
            x: d.rect.x,
            y: d.rect.y,
            width: d.rect.w,
            height: d.rect.h,
            color: p.surface2,
            corner_radii: CornerRadii::ZERO,
        });
    }
    if let Some(drop) = drop {
        draw_drop(sink, p, layout, drop);
    }
}

/// Where a dropped tab would land: a line between two tabs, or a wash over
/// the region the new group would take.
fn draw_drop<S: CommandSink + ?Sized>(sink: &mut S, p: &Palette, layout: &Layout, drop: &Drop) {
    let wash = crate::color::Color::rgba(p.accent.r, p.accent.g, p.accent.b, DROP_WASH_ALPHA);
    let region = match drop {
        Drop::Tabs { group, index } => {
            let Some(g) = layout.groups.iter().find(|g| &g.path == group) else {
                return;
            };
            let x = g.tabs.get(*index).map_or_else(
                || g.tabs.last().map_or(g.strip.x, |t| t.rect.right()),
                |t| t.rect.x,
            );
            sink.emit(RenderCommand::FillRect {
                x: (x - 1.0).max(g.strip.x),
                y: g.strip.y,
                width: 2.0,
                height: g.strip.h,
                color: p.accent,
                corner_radii: CornerRadii::ZERO,
            });
            return;
        }
        Drop::Beside { group, side } => {
            let Some(g) = layout.groups.iter().find(|g| &g.path == group) else {
                return;
            };
            half(g.rect, *side, 0.5)
        }
        Drop::Edge { side } => half(layout.area, *side, EDGE_FRACTION),
    };
    sink.emit(RenderCommand::FillRect {
        x: region.x,
        y: region.y,
        width: region.w,
        height: region.h,
        color: wash,
        corner_radii: CornerRadii::ZERO,
    });
    sink.emit(RenderCommand::StrokeRect {
        x: region.x,
        y: region.y,
        width: region.w,
        height: region.h,
        color: p.accent,
        line_width: 2.0,
        corner_radii: CornerRadii::ZERO,
    });
}

/// The `share` of `rect` along `side`.
fn half(rect: Rect, side: Side, share: f32) -> Rect {
    match side {
        Side::Left => Rect::new(rect.x, rect.y, rect.w * share, rect.h),
        Side::Right => Rect::new(
            rect.right() - rect.w * share,
            rect.y,
            rect.w * share,
            rect.h,
        ),
        Side::Top => Rect::new(rect.x, rect.y, rect.w, rect.h * share),
        Side::Bottom => Rect::new(
            rect.x,
            rect.bottom() - rect.h * share,
            rect.w,
            rect.h * share,
        ),
    }
}

// ============================================================================
// The panel menu
// ============================================================================

/// One entry of the panel menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelMenuEntry {
    /// What it says.
    pub label: String,
    /// Whether it is ticked: the panel is open.
    pub checked: bool,
    /// What choosing it does.
    pub action: PanelAction,
}

/// What a panel menu entry does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelAction {
    /// Open this panel.
    Open(PanelId),
    /// Close this panel.
    Close(PanelId),
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "a test that indexes out of range should fail loudly at the line that did it"
)]
mod tests {
    use super::*;

    fn id(name: &str) -> PanelId {
        PanelId::new(name).expect("a panel name")
    }

    /// A dock written as its saved text, every panel known.
    fn dock(text: &str) -> Dock {
        Dock::from_text(text, |_| true).expect("an arrangement")
    }

    fn kinds() -> Vec<PanelKind> {
        vec![
            PanelKind::new("files", "Files"),
            PanelKind::new("editor", "Editor"),
            PanelKind::new("outline", "Outline"),
            PanelKind::multiple("terminal", "Terminal"),
        ]
    }

    fn area() -> Rect {
        Rect::new(0.0, 0.0, 1000.0, 600.0)
    }

    /// Every invariant the module documentation promises.
    fn assert_tidy(dock: &Dock) {
        fn walk(node: &Node, parent_axis: Option<Axis>) {
            match node {
                Node::Group(g) => {
                    assert!(!g.panels.is_empty(), "an empty group");
                    assert!(g.active < g.panels.len(), "the front tab is out of range");
                }
                Node::Split {
                    axis,
                    children,
                    fractions,
                } => {
                    assert!(
                        children.len() >= 2,
                        "a split with {} children",
                        children.len()
                    );
                    assert_eq!(children.len(), fractions.len());
                    assert_ne!(
                        Some(*axis),
                        parent_axis,
                        "a split inside one on its own axis"
                    );
                    assert!(
                        fractions.iter().all(|f| *f > 0.0 && f.is_finite()),
                        "{fractions:?}"
                    );
                    let sum: f32 = fractions.iter().sum();
                    assert!((sum - 1.0).abs() < 1e-4, "shares sum to {sum}");
                    for child in children {
                        walk(child, Some(*axis));
                    }
                }
            }
        }
        if let Some(root) = dock.root() {
            walk(root, None);
        }
        let mut panels: Vec<&PanelId> = dock.panels();
        let n = panels.len();
        panels.sort();
        panels.dedup();
        assert_eq!(panels.len(), n, "a panel open twice");
    }

    // ---- Panel names -------------------------------------------------------

    #[test]
    fn a_panel_name_is_a_kind_and_perhaps_a_number() {
        let t = id("terminal#2");
        assert_eq!((t.kind(), t.instance()), ("terminal", 2));
        let f = id("files");
        assert_eq!((f.kind(), f.instance()), ("files", 1));
        assert_eq!(PanelId::of_kind("terminal", 1), Some(id("terminal")));
        assert_eq!(PanelId::of_kind("terminal", 3), Some(id("terminal#3")));
        // Every other spelling of a name is not a name: one panel, one name,
        // and nothing the saved text would need to quote.
        for bad in [
            "",
            "a b",
            "x#1",
            "x#01",
            "x#",
            "#2",
            "x#2#3",
            "caf\u{e9}",
            "a,b",
            "a]",
        ] {
            assert_eq!(PanelId::new(bad), None, "{bad:?} was taken as a name");
        }
    }

    #[test]
    fn the_second_panel_of_a_kind_is_numbered_on_its_tab() {
        let kinds = kinds();
        assert_eq!(title_of(&id("terminal"), &kinds), "Terminal");
        assert_eq!(title_of(&id("terminal#2"), &kinds), "Terminal 2");
        assert_eq!(
            title_of(&id("gone"), &kinds),
            "gone",
            "a kind no longer offered says its name"
        );
    }

    // ---- Opening, closing, the front tab -------------------------------------

    #[test]
    fn opening_into_an_empty_dock_makes_its_first_group() {
        let mut d = Dock::new();
        assert!(d.open(id("files"), None));
        assert_eq!(d.to_text(), "[*files]");
    }

    #[test]
    fn opening_beside_a_panel_puts_it_after_that_tab_and_in_front() {
        let mut d = dock("h(0.5:[*files,outline],0.5:[editor])");
        assert!(d.open(id("terminal"), Some(&id("files"))));
        assert_eq!(
            d.to_text(),
            "h(0.5:[files,*terminal,outline],0.5:[*editor])"
        );
        // With no neighbour named, the first group takes it.
        assert!(d.open(id("terminal#2"), None));
        assert_eq!(
            d.to_text(),
            "h(0.5:[files,terminal,outline,*terminal#2],0.5:[*editor])"
        );
        assert_tidy(&d);
    }

    #[test]
    fn opening_a_panel_already_open_only_brings_it_forward() {
        let mut d = dock("[*files,editor]");
        assert!(!d.open(id("editor"), None));
        assert_eq!(d.to_text(), "[files,*editor]");
    }

    #[test]
    fn closing_the_front_tab_brings_the_next_one_forward() {
        let mut d = dock("[files,*editor,outline]");
        assert!(d.close(&id("editor")));
        assert_eq!(d.to_text(), "[files,*outline]");
        // Closing the last tab brings the new last forward.
        assert!(d.close(&id("outline")));
        assert_eq!(d.to_text(), "[*files]");
        assert!(!d.close(&id("outline")), "it was already closed");
    }

    #[test]
    fn closing_a_tab_before_the_front_one_keeps_the_same_panel_in_front() {
        let mut d = dock("[files,editor,*outline]");
        d.close(&id("files"));
        assert_eq!(d.to_text(), "[editor,*outline]");
    }

    #[test]
    fn closing_a_groups_last_panel_takes_the_group_and_its_split_away() {
        let mut d = dock("h(0.3:[files],0.7:v(0.5:[*editor],0.5:[terminal]))");
        d.close(&id("terminal"));
        assert_eq!(d.to_text(), "h(0.3:[*files],0.7:[*editor])");
        d.close(&id("files"));
        assert_eq!(d.to_text(), "[*editor]");
        d.close(&id("editor"));
        assert!(d.is_empty());
        assert_eq!(d.to_text(), "");
    }

    #[test]
    fn ctrl_tab_brings_the_next_tab_forward_and_other_keys_pass() {
        let mut d = dock("[*files,editor,outline]");
        let key = |shift: bool, ctrl: bool| KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: crate::event::Modifiers {
                shift,
                ctrl,
                alt: false,
                super_key: false,
            },
            text: String::new(),
        };
        assert_eq!(
            d.handle_key(&id("files"), &key(false, true)),
            DockEvent::Activated(id("editor"))
        );
        assert_eq!(
            d.handle_key(&id("editor"), &key(true, true)),
            DockEvent::Activated(id("files"))
        );
        assert_eq!(
            d.handle_key(&id("files"), &key(false, false)),
            DockEvent::None,
            "plain Tab"
        );
        let alone = &mut dock("[*files]");
        assert_eq!(
            alone.handle_key(&id("files"), &key(false, true)),
            DockEvent::None,
            "one tab"
        );
    }

    #[test]
    fn cycling_wraps_at_both_ends() {
        let mut d = dock("[*files,editor,outline]");
        assert_eq!(d.cycle(&id("files"), false), Some(id("outline")));
        assert_eq!(d.cycle(&id("files"), true), Some(id("files")));
        assert_eq!(d.cycle(&id("files"), true), Some(id("editor")));
    }

    // ---- Moving --------------------------------------------------------------

    #[test]
    fn a_tab_dropped_among_another_groups_tabs_joins_them_in_front() {
        let mut d = dock("h(0.5:[*files,outline],0.5:[*editor,terminal])");
        let drop = Drop::Tabs {
            group: vec![1],
            index: 1,
        };
        assert!(d.apply(&id("outline"), &drop));
        assert_eq!(
            d.to_text(),
            "h(0.5:[*files],0.5:[editor,*outline,terminal])"
        );
        assert_tidy(&d);
    }

    #[test]
    fn a_tab_dropped_beside_a_group_splits_it_on_that_side() {
        for (side, want) in [
            (Side::Left, "h(0.5:[*outline],0.5:[*files])"),
            (Side::Right, "h(0.5:[*files],0.5:[*outline])"),
            (Side::Top, "v(0.5:[*outline],0.5:[*files])"),
            (Side::Bottom, "v(0.5:[*files],0.5:[*outline])"),
        ] {
            let mut d = dock("[*files,outline]");
            assert!(d.apply(
                &id("outline"),
                &Drop::Beside {
                    group: vec![],
                    side
                }
            ));
            assert_eq!(d.to_text(), want, "{side:?}");
            assert_tidy(&d);
        }
    }

    /// Beside a group already in a split along the same axis, the new group
    /// is a sibling taking half the group's share -- not a split nested in a
    /// split, which would only be flattened back.
    #[test]
    fn beside_a_group_in_a_split_along_that_axis_is_a_sibling() {
        let mut d = dock("h(0.5:[*files,terminal],0.5:[*editor])");
        assert!(d.apply(
            &id("terminal"),
            &Drop::Beside {
                group: vec![1],
                side: Side::Left
            }
        ));
        assert_eq!(
            d.to_text(),
            "h(0.5:[*files],0.25:[*terminal],0.25:[*editor])"
        );
        assert_tidy(&d);
    }

    #[test]
    fn a_tab_dropped_on_the_docks_edge_takes_a_column_of_the_whole_dock() {
        let mut d = dock("v(0.5:[*files,terminal],0.5:[*editor])");
        assert!(d.apply(&id("terminal"), &Drop::Edge { side: Side::Right }));
        assert_eq!(
            d.to_text(),
            "h(0.75:v(0.5:[*files],0.5:[*editor]),0.25:[*terminal])"
        );
        // Along an edge the root already divides along, it joins the row.
        assert!(d.apply(&id("files"), &Drop::Edge { side: Side::Left }));
        assert_eq!(
            d.to_text(),
            "h(0.25:[*files],0.5625:[*editor],0.1875:[*terminal])"
        );
        assert_tidy(&d);
    }

    /// The target is found again after the panel leaves: here leaving empties
    /// a group, the split around it collapses, and the target's path changes
    /// from `[1, 1]` to `[1]` underneath the drop.
    #[test]
    fn a_move_that_collapses_a_split_still_lands_where_it_was_aimed() {
        let mut d = dock("h(0.5:[*files],0.5:v(0.5:[*outline],0.5:[*terminal]))");
        let drop = Drop::Tabs {
            group: vec![1, 1],
            index: 1,
        };
        assert!(d.apply(&id("outline"), &drop));
        assert_eq!(d.to_text(), "h(0.5:[*files],0.5:[terminal,*outline])");
        assert_tidy(&d);
    }

    #[test]
    fn a_tab_moved_along_its_own_bar_is_reordered() {
        let mut d = dock("[*files,editor,outline]");
        let own = |index| Drop::Tabs {
            group: vec![],
            index,
        };
        assert!(d.apply(&id("files"), &own(3)));
        assert_eq!(d.to_text(), "[editor,outline,*files]");
        assert!(d.apply(&id("files"), &own(0)));
        assert_eq!(d.to_text(), "[*files,editor,outline]");
        assert!(!d.apply(&id("files"), &own(1)), "to where it already is");
    }

    #[test]
    fn a_move_that_would_change_nothing_changes_nothing() {
        let mut d = dock("h(0.5:[*files],0.5:[*editor])");
        let before = d.clone();
        let beside_itself = Drop::Beside {
            group: vec![0],
            side: Side::Right,
        };
        assert!(!d.apply(&id("files"), &beside_itself));
        assert!(
            !d.apply(&id("gone"), &Drop::Edge { side: Side::Left }),
            "a panel not open"
        );
        assert_eq!(d, before);
        let mut alone = dock("[*files]");
        assert!(!alone.apply(&id("files"), &Drop::Edge { side: Side::Left }));
    }

    /// Every move from every panel to every drop the layout offers, from a
    /// handful of starting arrangements: the tree stays tidy and no panel is
    /// lost or doubled.
    #[test]
    fn the_tree_is_tidy_after_every_move() {
        let starts = [
            "[*files,editor,outline,terminal]",
            "h(0.3:[*files],0.7:v(0.6:[*editor,outline],0.4:[*terminal]))",
            "v(0.5:h(0.5:[*files],0.5:[*editor]),0.5:h(0.5:[*outline],0.5:[*terminal]))",
        ];
        let sides = [Side::Left, Side::Right, Side::Top, Side::Bottom];
        let mut moves = 0;
        for start in starts {
            let base = dock(start);
            let mut drops = vec![];
            for (path, g) in base.groups() {
                for index in 0..=g.panels().len() {
                    drops.push(Drop::Tabs {
                        group: path.clone(),
                        index,
                    });
                }
                for side in sides {
                    drops.push(Drop::Beside {
                        group: path.clone(),
                        side,
                    });
                }
            }
            for side in sides {
                drops.push(Drop::Edge { side });
            }
            let everyone: Vec<PanelId> = base.panels().into_iter().cloned().collect();
            for panel in &everyone {
                for drop in &drops {
                    let mut d = base.clone();
                    d.apply(panel, drop);
                    assert_tidy(&d);
                    let mut now: Vec<PanelId> = d.panels().into_iter().cloned().collect();
                    now.sort();
                    let mut was = everyone.clone();
                    was.sort();
                    assert_eq!(
                        now, was,
                        "{start}: moving {panel} to {drop:?} lost or doubled a panel"
                    );
                    moves += 1;
                }
            }
        }
        assert!(moves > 200, "the sweep covered only {moves} moves");
    }

    // ---- Geometry --------------------------------------------------------------

    #[test]
    fn a_group_is_its_tab_bar_above_its_contents() {
        let d = dock("[files,*editor]");
        let layout = d.layout(area(), &kinds());
        assert_eq!(layout.groups.len(), 1);
        let g = &layout.groups[0];
        assert_eq!(g.strip, Rect::new(0.0, 0.0, 1000.0, TAB_BAR_HEIGHT));
        assert_eq!(
            g.content,
            Rect::new(0.0, TAB_BAR_HEIGHT, 1000.0, 600.0 - TAB_BAR_HEIGHT)
        );
        // The front panel is the one given the contents.
        assert_eq!(layout.panels(&d), vec![(&id("editor"), g.content)]);
    }

    #[test]
    fn the_groups_and_dividers_tile_the_area() {
        let d = dock("h(0.3:[*files],0.7:v(0.6:[*editor,outline],0.4:[*terminal]))");
        let layout = d.layout(area(), &kinds());
        assert_eq!(layout.groups.len(), 3);
        assert_eq!(layout.dividers.len(), 2);
        let covered: f32 = layout
            .groups
            .iter()
            .map(|g| g.rect.w * g.rect.h)
            .sum::<f32>()
            + layout
                .dividers
                .iter()
                .map(|d| d.rect.w * d.rect.h)
                .sum::<f32>();
        assert!((covered - 1000.0 * 600.0).abs() < 1.0, "covered {covered}");
        for (i, a) in layout.groups.iter().enumerate() {
            for b in &layout.groups[i + 1..] {
                let overlap_w = a.rect.right().min(b.rect.right()) - a.rect.x.max(b.rect.x);
                let overlap_h = a.rect.bottom().min(b.rect.bottom()) - a.rect.y.max(b.rect.y);
                assert!(
                    overlap_w <= 0.0 || overlap_h <= 0.0,
                    "{:?} overlaps {:?}",
                    a.rect,
                    b.rect
                );
            }
        }
        assert_eq!(layout.panels(&d).len(), 3, "one visible panel per group");
    }

    #[test]
    fn a_groups_tabs_are_where_its_tab_bar_draws_them() {
        let d = dock("[files,*editor]");
        let layout = d.layout(area(), &kinds());
        let g = &layout.groups[0];
        let Some(Node::Group(group)) = d.root() else {
            panic!("one group");
        };
        assert_eq!(
            g.tabs,
            tab_view(group, &kinds()).tab_rects(g.strip.x, g.strip.y)
        );
        assert_eq!(g.tabs.len(), 2);
    }

    // ---- Hit testing ------------------------------------------------------------------

    #[test]
    fn a_hit_names_the_divider_tab_close_button_bar_or_contents() {
        let d = dock("h(0.5:[*files,outline],0.5:[*editor])");
        let layout = d.layout(area(), &kinds());
        let divider = layout.dividers[0].rect;
        // On the divider, and just beside it within the grab margin.
        for dx in [
            0.0,
            -splitter::GRAB_MARGIN,
            divider.w + splitter::GRAB_MARGIN - 0.5,
        ] {
            assert_eq!(
                d.hit(&layout, divider.x + dx, 300.0),
                Some(DockHit::Divider {
                    split: vec![],
                    index: 0
                }),
                "{dx}"
            );
        }
        let g = &layout.groups[0];
        let tab = g.tabs[1];
        assert_eq!(
            d.hit(&layout, tab.rect.x + 3.0, tab.rect.y + 5.0),
            Some(DockHit::Tab {
                panel: id("outline")
            })
        );
        let close = tab.close.expect("a dock's tabs close");
        assert_eq!(
            d.hit(&layout, close.x + 1.0, close.y + 1.0),
            Some(DockHit::Close {
                panel: id("outline")
            })
        );
        assert_eq!(
            d.hit(&layout, g.strip.right() - 20.0, 10.0),
            Some(DockHit::Strip { group: vec![0] })
        );
        assert_eq!(
            d.hit(&layout, 100.0, 300.0),
            Some(DockHit::Content { panel: id("files") })
        );
    }

    // ---- Where a drop lands --------------------------------------------------------------

    #[test]
    fn a_drop_on_a_tab_bar_goes_between_the_tabs_either_side_of_the_pointer() {
        let d = dock("h(0.5:[*files,outline],0.5:[*editor,terminal])");
        let layout = d.layout(area(), &kinds());
        let right = &layout.groups[1];
        let first = right.tabs[0].rect;
        let second = right.tabs[1].rect;
        let at = |x: f32| d.drop_at(&layout, &id("files"), x, 10.0);
        assert_eq!(
            at(first.x + 2.0),
            Some(Drop::Tabs {
                group: vec![1],
                index: 0
            })
        );
        assert_eq!(
            at(first.right() - 2.0),
            Some(Drop::Tabs {
                group: vec![1],
                index: 1
            })
        );
        assert_eq!(
            at(second.right() - 2.0),
            Some(Drop::Tabs {
                group: vec![1],
                index: 2
            })
        );
        assert_eq!(
            at(right.strip.right() - 20.0),
            Some(Drop::Tabs {
                group: vec![1],
                index: 2
            })
        );
        // On its own bar, where it already is: nowhere. (Past the dock's left
        // edge zone, which the leftmost tab's first pixels are in.)
        let own = layout.groups[0].tabs[0].rect;
        assert_eq!(
            d.drop_at(&layout, &id("files"), own.x + DOCK_EDGE + 2.0, 10.0),
            None
        );
        assert_eq!(
            d.drop_at(&layout, &id("files"), own.right() - 2.0, 10.0),
            None
        );
    }

    #[test]
    fn a_drop_near_a_groups_edge_splits_it_and_in_the_middle_joins_it() {
        let d = dock("h(0.5:[*files,outline],0.5:[*editor])");
        let layout = d.layout(area(), &kinds());
        let c = layout.groups[1].content;
        let at =
            |fx: f32, fy: f32| d.drop_at(&layout, &id("outline"), c.x + c.w * fx, c.y + c.h * fy);
        let beside = |side| {
            Some(Drop::Beside {
                group: vec![1],
                side,
            })
        };
        assert_eq!(at(0.1, 0.5), beside(Side::Left));
        assert_eq!(at(0.9, 0.5), beside(Side::Right));
        assert_eq!(at(0.5, 0.1), beside(Side::Top));
        assert_eq!(at(0.5, 0.9), beside(Side::Bottom));
        assert_eq!(
            at(0.5, 0.5),
            Some(Drop::Tabs {
                group: vec![1],
                index: 1
            })
        );
        // Its own group: beside it splits it, the middle is where it is.
        let own = layout.groups[0].content;
        let mid_own = d.drop_at(
            &layout,
            &id("outline"),
            own.x + own.w / 2.0,
            own.y + own.h / 2.0,
        );
        assert_eq!(mid_own, None);
        let edge_own = d.drop_at(&layout, &id("outline"), own.x + 30.0, own.y + own.h / 2.0);
        assert_eq!(
            edge_own,
            Some(Drop::Beside {
                group: vec![0],
                side: Side::Left
            })
        );
    }

    /// Over a top-row tab bar the bar wins, bar the thin strip at the very
    /// top; away from the bars the dock's top edge has its full reach.
    #[test]
    fn a_tab_bar_along_the_docks_top_is_a_tab_target_and_not_its_edge() {
        let d = dock("h(0.5:[*files],0.5:[*editor])");
        let layout = d.layout(area(), &kinds());
        let x = layout.groups[1].strip.x + 3.0;
        assert_eq!(
            d.drop_at(&layout, &id("files"), x, 10.0),
            Some(Drop::Tabs {
                group: vec![1],
                index: 0
            })
        );
        assert_eq!(
            d.drop_at(&layout, &id("files"), x, 2.0),
            Some(Drop::Edge { side: Side::Top })
        );
    }

    #[test]
    fn the_docks_own_edge_takes_a_row_or_a_column_unless_there_is_one_panel() {
        let d = dock("h(0.5:[*files],0.5:[*editor])");
        let layout = d.layout(area(), &kinds());
        assert_eq!(
            d.drop_at(&layout, &id("files"), 995.0, 300.0),
            Some(Drop::Edge { side: Side::Right })
        );
        assert_eq!(
            d.drop_at(&layout, &id("files"), 500.0, 595.0),
            Some(Drop::Edge { side: Side::Bottom })
        );
        assert_eq!(
            d.drop_at(&layout, &id("files"), 1000.0, 300.0),
            None,
            "outside the dock"
        );
        let alone = dock("[*files]");
        let layout = alone.layout(area(), &kinds());
        assert_eq!(alone.drop_at(&layout, &id("files"), 995.0, 300.0), None);
    }

    // ---- Resizing -------------------------------------------------------------------

    #[test]
    fn dragging_a_divider_resizes_and_stops_at_the_smallest_group() {
        let mut d = dock("h(0.5:[*files],0.5:[*editor])");
        let layout = d.layout(area(), &kinds());
        assert!(d.resize(&layout, &[], 0, 300.0, 100.0));
        let layout = d.layout(area(), &kinds());
        assert!(
            (layout.groups[0].rect.w - 299.0).abs() < 1.0,
            "{:?}",
            layout.groups[0].rect
        );
        // Past the left group's minimum: it stops there.
        d.resize(&layout, &[], 0, 5.0, 100.0);
        let layout = d.layout(area(), &kinds());
        assert!(
            (layout.groups[0].rect.w - MIN_GROUP).abs() < 0.5,
            "{:?}",
            layout.groups[0].rect
        );
        assert!(!d.resize(&layout, &[0], 0, 5.0, 100.0), "no such split");
    }

    /// **The smallest group follows the user's text size** (on this test's
    /// thread): at twice the size a divider stops at twice the least size --
    /// room for a tab bar twice as tall, and the start of what is under it.
    #[test]
    fn the_smallest_group_follows_the_text_size() {
        crate::text::set_base_size(crate::text::DEFAULT_SIZE * 2.0);
        let mut d = dock("h(0.5:[*files],0.5:[*editor])");
        let layout = d.layout(area(), &kinds());
        assert!(d.resize(&layout, &[], 0, 5.0, 100.0));
        let layout = d.layout(area(), &kinds());
        assert!(
            (layout.groups[0].rect.w - MIN_GROUP * 2.0).abs() < 0.5,
            "{:?}",
            layout.groups[0].rect
        );
    }

    // ---- Input ----------------------------------------------------------------------

    #[test]
    fn a_tab_clicked_comes_forward_and_one_dragged_moves() {
        let mut d = dock("h(0.5:[*files,outline],0.5:[*editor])");
        let mut input = DockInput::new();
        let layout = d.layout(area(), &kinds());
        let tab = layout.groups[0].tabs[1].rect;
        let (x, y) = (tab.x + 5.0, tab.y + 10.0);

        // A press and a small wobble is still a click.
        input.press(&d, &layout, x, y);
        input.drag_to(&mut d, &layout, x + 2.0, y + 2.0);
        assert!(!input.is_dragging());
        assert_eq!(
            input.release(&mut d, &layout, x + 2.0, y + 2.0),
            DockEvent::Activated(id("outline"))
        );
        assert_eq!(d.to_text(), "h(0.5:[files,*outline],0.5:[*editor])");

        // Past the threshold it is a drag, previewed while held and moved on release.
        let layout = d.layout(area(), &kinds());
        input.press(&d, &layout, x, y);
        let right = layout.groups[1].content;
        let (tx, ty) = (right.x + right.w / 2.0, right.y + right.h / 2.0);
        input.drag_to(&mut d, &layout, tx, ty);
        assert!(input.is_dragging());
        assert_eq!(
            input.preview(&d, &layout, tx, ty),
            Some(Drop::Tabs {
                group: vec![1],
                index: 1
            })
        );
        assert_eq!(
            input.release(&mut d, &layout, tx, ty),
            DockEvent::Moved(id("outline"))
        );
        assert_eq!(d.to_text(), "h(0.5:[*files],0.5:[editor,*outline])");
    }

    #[test]
    fn a_close_button_closes_only_when_let_go_on_it() {
        let mut d = dock("[*files,outline]");
        let mut input = DockInput::new();
        let layout = d.layout(area(), &kinds());
        let close = layout.groups[0].tabs[1].close.expect("closeable");
        input.press(&d, &layout, close.x + 2.0, close.y + 2.0);
        // Slid off: nothing closes.
        assert_eq!(
            input.release(&mut d, &layout, 500.0, 300.0),
            DockEvent::None
        );
        assert!(d.contains(&id("outline")));
        input.press(&d, &layout, close.x + 2.0, close.y + 2.0);
        assert_eq!(
            input.release(&mut d, &layout, close.x + 3.0, close.y + 3.0),
            DockEvent::Closed(id("outline"))
        );
        assert!(!d.contains(&id("outline")));
    }

    #[test]
    fn a_divider_dragged_resizes_and_contents_are_the_applications() {
        let mut d = dock("h(0.5:[*files],0.5:[*editor])");
        let mut input = DockInput::new();
        let layout = d.layout(area(), &kinds());
        let divider = layout.dividers[0].rect;
        input.press(&d, &layout, divider.x + 1.0, 300.0);
        assert_eq!(
            input.drag_to(&mut d, &layout, 400.0, 300.0),
            DockEvent::Resized
        );
        assert_eq!(
            input.release(&mut d, &layout, 400.0, 300.0),
            DockEvent::None
        );
        assert_eq!(
            input.press(&d, &layout, 100.0, 300.0),
            DockEvent::Content(id("files"))
        );
        input.cancel();
        assert!(!input.is_dragging());
    }

    // ---- Saving --------------------------------------------------------------------

    #[test]
    fn an_arrangement_survives_being_saved_and_read_back() {
        for text in [
            "",
            "[*files]",
            "[files,*terminal,terminal#2]",
            "h(0.25:[*files],0.75:v(0.7:[*editor,outline],0.3:[*terminal]))",
            "v(0.5:h(0.5:[*files],0.5:[*editor]),0.5:h(0.3333:[*outline],0.6667:[*terminal]))",
        ] {
            let d = dock(text);
            assert_eq!(d.to_text(), text);
            assert_eq!(dock(&d.to_text()), d);
            assert_tidy(&d);
        }
    }

    #[test]
    fn spaces_are_ignored_and_the_arrangement_is_tidied_on_the_way_in() {
        let d = dock(" h( 1 : [ a , *b ] , 3 : h( 1:[c], 1:[d] ) ) ");
        assert_eq!(d.to_text(), "h(0.25:[a,*b],0.375:[*c],0.375:[*d])");
        let zero = dock("h(0:[a],1:[b])");
        assert_tidy(&zero);
    }

    /// A panel the application no longer offers is dropped and the layout
    /// closes up round it; the rest of the saved arrangement is kept.
    #[test]
    fn a_panel_no_longer_offered_is_dropped_rather_than_the_arrangement() {
        let known = |kind: &str| kind != "outline";
        let d =
            Dock::from_text("h(0.3:[*outline],0.7:[*editor,files,editor])", known).expect("reads");
        assert_eq!(d.to_text(), "[*editor,files]");
        let front_dropped = Dock::from_text("[files,*outline]", known).expect("reads");
        assert_eq!(front_dropped.to_text(), "[*files]");
    }

    #[test]
    fn text_that_is_not_an_arrangement_is_refused_with_where() {
        for (text, at) in [
            ("x", 0),
            ("[", 1),
            ("[a,]", 3),
            ("[a b]", 3),
            ("h(0.5:[a]", 9),
            ("h([a])", 2),
            ("[a] junk", 4),
            ("h(abc:[a])", 2),
            ("[caf\u{e9}]", 4),
            ("[a#1]", 1),
        ] {
            let err = Dock::from_text(text, |_| true).expect_err(text);
            assert_eq!(err.at, at, "{text:?}: {err}");
        }
        let deep = "h(1:".repeat(MAX_DEPTH + 2) + "[a]" + &")".repeat(MAX_DEPTH + 2);
        assert!(
            Dock::from_text(&deep, |_| true).is_err(),
            "nesting past the limit"
        );
        assert_eq!(Dock::from_text("   ", |_| true), Ok(Dock::new()));
    }

    // ---- The panel menu -----------------------------------------------------------

    #[test]
    fn the_menu_ticks_what_is_open_and_offers_another_of_a_kind_that_repeats() {
        let mut d = dock("[*files,terminal]");
        let menu = d.menu(&kinds());
        let labels: Vec<(&str, bool)> =
            menu.iter().map(|e| (e.label.as_str(), e.checked)).collect();
        assert_eq!(
            labels,
            vec![
                ("Files", true),
                ("Editor", false),
                ("Outline", false),
                ("New Terminal", false)
            ]
        );
        assert_eq!(menu[0].action, PanelAction::Close(id("files")));
        assert_eq!(menu[1].action, PanelAction::Open(id("editor")));
        assert_eq!(menu[3].action, PanelAction::Open(id("terminal#2")));
        assert!(d.perform(&menu[3].action, Some(&id("terminal"))));
        assert_eq!(d.to_text(), "[files,terminal,*terminal#2]");
        assert!(d.perform(&menu[0].action, None));
        assert!(!d.contains(&id("files")));
        // A kind whose name is no panel name offers nothing -- quickly.
        let odd = [
            PanelKind::multiple("bad name", "Bad"),
            PanelKind::multiple("x#2", "X"),
        ];
        assert!(d.menu(&odd).is_empty());
    }

    // ---- Drawing -------------------------------------------------------------------

    #[test]
    fn every_group_gets_its_tab_bar_and_every_divider_is_drawn() {
        let d = dock("h(0.3:[*files],0.7:v(0.6:[*editor,outline],0.4:[*terminal]))");
        let p = Palette::for_mode(false);
        let layout = d.layout(area(), &kinds());
        let mut out = Vec::new();
        draw(&mut out, &p, &d, &layout, &kinds(), None);
        let text = |s: &str| {
            out.iter()
                .filter(|c| matches!(c, RenderCommand::Text { text, .. } if text == s))
                .count()
        };
        for title in ["Files", "Editor", "Outline", "Terminal"] {
            assert_eq!(text(title), 1, "{title}'s tab");
        }
        let dividers = out
            .iter()
            .filter(|c| {
                layout.dividers.iter().any(|d| {
                    matches!(c, RenderCommand::FillRect { x, y, width, height, color, .. }
                        if *x == d.rect.x && *y == d.rect.y && *width == d.rect.w
                            && *height == d.rect.h && *color == p.surface2)
                })
            })
            .count();
        assert_eq!(dividers, 2);
    }

    #[test]
    fn where_a_drop_would_land_is_shown_in_the_accent() {
        let d = dock("h(0.5:[*files,outline],0.5:[*editor])");
        let mut p = Palette::for_mode(true);
        p.accent = crate::color::Color::from_hex(0x00C0_3070);
        let layout = d.layout(area(), &kinds());
        for drop in [
            Drop::Tabs {
                group: vec![1],
                index: 1,
            },
            Drop::Beside {
                group: vec![1],
                side: Side::Left,
            },
            Drop::Edge { side: Side::Bottom },
        ] {
            let mut out = Vec::new();
            draw(&mut out, &p, &d, &layout, &kinds(), Some(&drop));
            let accented = out
                .iter()
                .filter(|c| match c {
                    RenderCommand::FillRect { color, .. }
                    | RenderCommand::StrokeRect { color, .. } => {
                        (color.r, color.g, color.b) == (p.accent.r, p.accent.g, p.accent.b)
                    }
                    _ => false,
                })
                .count();
            // The tab bar marks its front tab in the accent too, one per group.
            assert!(
                accented > layout.groups.len(),
                "{drop:?}: {accented} accented"
            );
        }
    }
}

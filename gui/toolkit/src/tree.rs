//! TreeView widget for hierarchical data display.
//!
//! Provides a tree-structured view with expand/collapse, selection,
//! keyboard navigation, and scroll support. Renders using Catppuccin
//! Mocha theme colors for a modern dark-theme appearance.

use crate::color::Color;
use crate::event::{Key, KeyEvent, Modifiers};
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::style::CornerRadii;

/// Unique identifier for tree nodes.
pub type TreeNodeId = u64;

/// A node in the tree hierarchy.
#[derive(Clone, Debug)]
pub struct TreeNode {
    /// Unique identifier for this node.
    pub id: TreeNodeId,
    /// Display label text.
    pub label: String,
    /// Optional icon name/identifier for the node.
    pub icon: Option<String>,
    /// Whether this node's children are visible.
    pub expanded: bool,
    /// Whether this node is selected.
    pub selected: bool,
    /// Child nodes.
    pub children: Vec<TreeNode>,
    /// Depth level in the tree (computed during layout).
    pub depth: u32,
}

impl TreeNode {
    /// Create a new leaf node with no children.
    pub fn leaf(id: TreeNodeId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            icon: None,
            expanded: false,
            selected: false,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Create a new branch node with children.
    pub fn branch(id: TreeNodeId, label: impl Into<String>, children: Vec<TreeNode>) -> Self {
        Self {
            id,
            label: label.into(),
            icon: None,
            expanded: false,
            selected: false,
            children,
            depth: 0,
        }
    }

    /// Returns true if this node has child nodes.
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

/// Configuration for tree appearance and behavior.
#[derive(Clone, Debug)]
pub struct TreeConfig {
    /// Pixels of indentation per depth level.
    pub indent_width: f32,
    /// Height of each row in pixels.
    pub row_height: f32,
    /// Whether to draw connection lines between parent and child.
    pub show_lines: bool,
    /// Whether to show the icon column.
    pub show_icons: bool,
    /// Whether multiple nodes can be selected simultaneously.
    pub multi_select: bool,
}

impl Default for TreeConfig {
    fn default() -> Self {
        Self {
            indent_width: 20.0,
            row_height: 24.0,
            show_lines: true,
            show_icons: true,
            multi_select: false,
        }
    }
}

/// Events produced by tree interaction.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeEvent {
    /// A node was selected.
    Selected(TreeNodeId),
    /// A node was deselected.
    Deselected(TreeNodeId),
    /// A node was expanded.
    Expanded(TreeNodeId),
    /// A node was collapsed.
    Collapsed(TreeNodeId),
    /// A node was double-clicked.
    DoubleClicked(TreeNodeId),
    /// A context menu was requested at the given position.
    ContextMenu(TreeNodeId, f32, f32),
}

// --- Catppuccin Mocha theme colors ---

/// Background color for the tree view.
const BG_BASE: Color = Color::from_hex(0x1E1E2E);
/// Surface color for selection highlight.
const BG_SURFACE: Color = Color::from_hex(0x313244);
/// Hover highlight color.
const BG_HOVER: Color = Color::from_hex(0x45475A);
/// Text color for labels.
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
/// Subdued text color for expand indicators.
const TEXT_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
/// Accent color for focus ring.
const ACCENT_BLUE: Color = Color::from_hex(0x89B4FA);
/// Line color for tree connection lines.
const LINE_COLOR: Color = Color::from_hex(0x585B70);

/// Tree view state and logic.
///
/// Manages a hierarchical collection of nodes with expand/collapse,
/// selection, keyboard navigation, and scroll behavior.
pub struct TreeView {
    root_nodes: Vec<TreeNode>,
    config: TreeConfig,
    focused_id: Option<TreeNodeId>,
    scroll_offset: f32,
    viewport_height: f32,
    hover_id: Option<TreeNodeId>,
}

impl TreeView {
    /// Create a new tree view with the given configuration.
    pub fn new(config: TreeConfig) -> Self {
        Self {
            root_nodes: Vec::new(),
            config,
            focused_id: None,
            scroll_offset: 0.0,
            viewport_height: 0.0,
            hover_id: None,
        }
    }

    /// Replace all nodes in the tree. Recomputes depth values.
    pub fn set_nodes(&mut self, mut nodes: Vec<TreeNode>) {
        Self::compute_depths(&mut nodes, 0);
        self.root_nodes = nodes;
        // Clear focus/selection if nodes changed and old focus no longer exists
        if let Some(fid) = self.focused_id
            && Self::find_node(&self.root_nodes, fid).is_none()
        {
            self.focused_id = None;
        }
    }

    /// Get a reference to a node by its ID, searching the entire tree.
    pub fn get_node(&self, id: TreeNodeId) -> Option<&TreeNode> {
        Self::find_node(&self.root_nodes, id)
    }

    /// Get a mutable reference to a node by its ID.
    pub fn get_node_mut(&mut self, id: TreeNodeId) -> Option<&mut TreeNode> {
        Self::find_node_mut(&mut self.root_nodes, id)
    }

    /// Expand a node to show its children.
    pub fn expand(&mut self, id: TreeNodeId) {
        if let Some(node) = Self::find_node_mut(&mut self.root_nodes, id) {
            node.expanded = true;
        }
    }

    /// Collapse a node to hide its children.
    pub fn collapse(&mut self, id: TreeNodeId) {
        if let Some(node) = Self::find_node_mut(&mut self.root_nodes, id) {
            node.expanded = false;
        }
    }

    /// Toggle the expanded state of a node.
    pub fn toggle(&mut self, id: TreeNodeId) {
        if let Some(node) = Self::find_node_mut(&mut self.root_nodes, id) {
            node.expanded = !node.expanded;
        }
    }

    /// Expand all nodes in the tree.
    pub fn expand_all(&mut self) {
        Self::set_expanded_recursive(&mut self.root_nodes, true);
    }

    /// Collapse all nodes in the tree.
    pub fn collapse_all(&mut self) {
        Self::set_expanded_recursive(&mut self.root_nodes, false);
    }

    /// Select a specific node. If multi-select is disabled, deselects all others.
    pub fn select(&mut self, id: TreeNodeId) {
        if !self.config.multi_select {
            Self::deselect_all_recursive(&mut self.root_nodes);
        }
        if let Some(node) = Self::find_node_mut(&mut self.root_nodes, id) {
            node.selected = true;
        }
        self.focused_id = Some(id);
    }

    /// Deselect all nodes in the tree.
    pub fn deselect_all(&mut self) {
        Self::deselect_all_recursive(&mut self.root_nodes);
    }

    /// Returns all visible nodes (those whose ancestors are all expanded), in display order.
    pub fn visible_nodes(&self) -> Vec<&TreeNode> {
        let mut result = Vec::new();
        Self::collect_visible(&self.root_nodes, &mut result);
        result
    }

    /// Handle a keyboard event. Returns a `TreeEvent` if the key produced an action.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<TreeEvent> {
        if !key.pressed {
            return None;
        }

        let visible = self.visible_nodes_ids();
        let current_idx = self
            .focused_id
            .and_then(|fid| visible.iter().position(|&vid| vid == fid));

        match key.key {
            // With nothing focused yet, either arrow lands on the first row
            // rather than stepping away from an imaginary position. At the far
            // end, the step is refused: `checked_sub` has nowhere to go, and
            // `focus_row` finds no row past the last one.
            Key::Up if key.modifiers == Modifiers::NONE => {
                let target = match current_idx {
                    Some(idx) => idx.checked_sub(1)?,
                    None => 0,
                };
                return self.focus_row(&visible, target);
            }
            Key::Down if key.modifiers == Modifiers::NONE => {
                let target = match current_idx {
                    Some(idx) => idx.checked_add(1)?,
                    None => 0,
                };
                return self.focus_row(&visible, target);
            }
            Key::Right if key.modifiers == Modifiers::NONE => {
                if let Some(fid) = self.focused_id {
                    let should_expand = Self::find_node(&self.root_nodes, fid)
                        .is_some_and(|n| n.has_children() && !n.expanded);
                    if should_expand {
                        self.expand(fid);
                        return Some(TreeEvent::Expanded(fid));
                    }
                }
            }
            Key::Left if key.modifiers == Modifiers::NONE => {
                if let Some(fid) = self.focused_id {
                    let should_collapse = Self::find_node(&self.root_nodes, fid)
                        .is_some_and(|n| n.has_children() && n.expanded);
                    if should_collapse {
                        self.collapse(fid);
                        return Some(TreeEvent::Collapsed(fid));
                    }
                }
            }
            Key::Enter | Key::Space if key.modifiers == Modifiers::NONE => {
                if let Some(fid) = self.focused_id {
                    let has_children =
                        Self::find_node(&self.root_nodes, fid).is_some_and(|n| n.has_children());
                    if has_children {
                        let was_expanded =
                            Self::find_node(&self.root_nodes, fid).is_some_and(|n| n.expanded);
                        self.toggle(fid);
                        return if was_expanded {
                            Some(TreeEvent::Collapsed(fid))
                        } else {
                            Some(TreeEvent::Expanded(fid))
                        };
                    }
                }
            }
            Key::Home if key.modifiers == Modifiers::NONE => {
                let event = self.focus_row(&visible, 0)?;
                // Home goes to the top of the list, not merely to a position
                // from which the first row is visible.
                self.scroll_offset = 0.0;
                return Some(event);
            }
            Key::End if key.modifiers == Modifiers::NONE => {
                return self.focus_row(&visible, visible.len().saturating_sub(1));
            }
            _ => {}
        }
        None
    }

    /// The visible row at `y`, measured from the tree widget's own origin.
    ///
    /// The renderer draws row `idx` at `idx * row_height - scroll_offset`, so
    /// this is that arithmetic inverted, and it is the *only* place the
    /// inversion is written down: `handle_click` and `handle_context_menu`
    /// each used to carry their own copy of
    ///
    /// ```text
    /// ((y + self.scroll_offset) / self.config.row_height) as usize
    /// ```
    ///
    /// which answered row 0 for any point *above* the tree. A negative `f32`
    /// cast to `usize` saturates to zero in Rust — it does not wrap and it
    /// does not trap — and so does a NaN, so a click a few pixels above the
    /// first row, or at a coordinate some caller failed to compute, selected
    /// the first node rather than nothing. `row_height` is a public config
    /// field, so a zero there did the same to every click in the tree.
    ///
    /// Returns `None` above the first row, past the last one, and for any
    /// coordinate or row height that is not a positive finite number.
    fn row_at(&self, y: f32) -> Option<usize> {
        let row_h = self.config.row_height;
        if !row_h.is_finite() || row_h <= 0.0 {
            return None;
        }
        let content_y = y + self.scroll_offset;
        if !content_y.is_finite() || content_y < 0.0 {
            return None;
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let idx = (content_y / row_h) as usize;
        (idx < self.visible_nodes().len()).then_some(idx)
    }

    /// Handle a mouse click at position (x, y) relative to the tree widget origin.
    /// `double` indicates a double-click.
    pub fn handle_click(&mut self, x: f32, y: f32, double: bool) -> Option<TreeEvent> {
        let row_index = self.row_at(y)?;
        let visible = self.visible_nodes_ids();

        let node_id = visible.get(row_index).copied()?;

        if double {
            self.select(node_id);
            return Some(TreeEvent::DoubleClicked(node_id));
        }

        // Check if click was on the expand/collapse indicator
        let node_depth = Self::find_node(&self.root_nodes, node_id)
            .map(|n| n.depth)
            .unwrap_or(0);
        let indicator_x = node_depth as f32 * self.config.indent_width;
        let indicator_end = indicator_x + 16.0;

        let has_children =
            Self::find_node(&self.root_nodes, node_id).is_some_and(|n| n.has_children());

        if has_children && x >= indicator_x && x < indicator_end {
            let was_expanded =
                Self::find_node(&self.root_nodes, node_id).is_some_and(|n| n.expanded);
            self.toggle(node_id);
            self.focused_id = Some(node_id);
            return if was_expanded {
                Some(TreeEvent::Collapsed(node_id))
            } else {
                Some(TreeEvent::Expanded(node_id))
            };
        }

        // Regular selection click
        let prev_selected = Self::find_node(&self.root_nodes, node_id).is_some_and(|n| n.selected);

        self.select(node_id);

        if prev_selected {
            None
        } else {
            Some(TreeEvent::Selected(node_id))
        }
    }

    /// Handle a right-click at position (x, y) for context menu.
    pub fn handle_context_menu(&mut self, x: f32, y: f32) -> Option<TreeEvent> {
        let row_index = self.row_at(y)?;
        let visible = self.visible_nodes_ids();
        let node_id = visible.get(row_index).copied()?;
        self.select(node_id);
        Some(TreeEvent::ContextMenu(node_id, x, y))
    }

    /// Render the tree into a list of render commands.
    ///
    /// The tree is drawn within the rectangle at (x, y) with the given
    /// width and height. Nodes outside the visible viewport are culled.
    pub fn render(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        self.viewport_height_hint(height);
        let mut commands = Vec::new();

        // Background fill
        commands.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: BG_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Clip to viewport
        commands.push(RenderCommand::PushClip {
            x,
            y,
            width,
            height,
        });

        let visible = self.visible_nodes();
        let row_h = self.config.row_height;
        let start_row = (self.scroll_offset / row_h) as usize;
        // +2 for the partial rows at the top and bottom of the viewport.
        let visible_rows = ((height / row_h) as usize).saturating_add(2);
        let end_row = start_row.saturating_add(visible_rows);

        // `take` before `skip` so both count absolute rows, and the `take`
        // does the clamping to the list's length that the loop bound used to.
        for (idx, node) in visible.iter().enumerate().take(end_row).skip(start_row) {
            let row_y = y + (idx as f32 * row_h) - self.scroll_offset;
            let indent = node.depth as f32 * self.config.indent_width;

            // Selection/hover background
            if node.selected {
                commands.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: row_h,
                    color: BG_SURFACE,
                    corner_radii: CornerRadii::ZERO,
                });
            } else if self.hover_id == Some(node.id) {
                commands.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: row_h,
                    color: BG_HOVER,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Focus ring on focused node
            if self.focused_id == Some(node.id) {
                commands.push(RenderCommand::StrokeRect {
                    x: x + 1.0,
                    y: row_y + 1.0,
                    width: width - 2.0,
                    height: row_h - 2.0,
                    color: ACCENT_BLUE,
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            // Connection lines
            if self.config.show_lines && node.depth > 0 {
                let line_x = x + indent - self.config.indent_width / 2.0;
                let mid_y = row_y + row_h / 2.0;
                // Horizontal line from parent connector to node
                commands.push(RenderCommand::Line {
                    x1: line_x,
                    y1: mid_y,
                    x2: line_x + self.config.indent_width / 2.0 - 2.0,
                    y2: mid_y,
                    color: LINE_COLOR,
                    width: 1.0,
                });
            }

            // Expand/collapse indicator
            let indicator_x = x + indent;
            if node.has_children() {
                let indicator_text = if node.expanded {
                    "\u{25BC}"
                } else {
                    "\u{25B6}"
                };
                commands.push(RenderCommand::Text {
                    x: indicator_x,
                    y: row_y + (row_h - 12.0) / 2.0,
                    text: indicator_text.to_string(),
                    color: TEXT_SUBTEXT,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Label
            let label_x = indicator_x + 18.0;
            commands.push(RenderCommand::Text {
                x: label_x,
                y: row_y + (row_h - 14.0) / 2.0,
                text: node.label.clone(),
                color: TEXT_COLOR,
                font_size: 14.0,
                font_weight: if node.selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(width - (label_x - x) - 8.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        commands.push(RenderCommand::PopClip);
        commands
    }

    /// Set the scroll offset (for external scroll handling).
    pub fn set_scroll_offset(&mut self, offset: f32) {
        let max_scroll = self.max_scroll();
        self.scroll_offset = offset.clamp(0.0, max_scroll);
    }

    /// Get the current scroll offset.
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_offset
    }

    /// Set the hover node (for external mouse tracking).
    pub fn set_hover(&mut self, id: Option<TreeNodeId>) {
        self.hover_id = id;
    }

    /// Get the currently focused node ID.
    pub fn focused_id(&self) -> Option<TreeNodeId> {
        self.focused_id
    }

    // --- Private helpers ---

    fn viewport_height_hint(&self, height: f32) {
        // We use interior mutability trick through the render call
        // to update viewport height. Since render takes &self, we store
        // it and use it in scroll calculations via max_scroll.
        // For a no_std kernel context, we avoid RefCell; the caller
        // should call set_viewport_height before rendering.
        let _ = height;
    }

    /// Set viewport height for scroll calculations.
    pub fn set_viewport_height(&mut self, height: f32) {
        self.viewport_height = height;
    }

    fn max_scroll(&self) -> f32 {
        let total_rows = self.visible_nodes().len() as f32;
        let total_height = total_rows * self.config.row_height;
        (total_height - self.viewport_height).max(0.0)
    }

    /// Select the node on visible row `idx`, scroll it into view, and report
    /// the selection — or do nothing at all if there is no such row.
    ///
    /// Up, Down, Home and End were four copies of these three lines, and each
    /// copy proved "the row exists" its own way: `idx > 0` before an
    /// `idx - 1`, `idx + 1 < len` before an `idx + 1`, a `.last()` before a
    /// `len() - 1`. Every one of those proofs sat in a statement above the
    /// indexing it justified. Here the row is fetched and the existence
    /// established by the same `get`, so a caller can ask for any index —
    /// including one off either end — and get back a refusal rather than a
    /// panic.
    fn focus_row(&mut self, visible: &[TreeNodeId], idx: usize) -> Option<TreeEvent> {
        let &id = visible.get(idx)?;
        self.select(id);
        self.ensure_visible(idx);
        Some(TreeEvent::Selected(id))
    }

    fn ensure_visible(&mut self, row_index: usize) {
        let row_top = row_index as f32 * self.config.row_height;
        let row_bottom = row_top + self.config.row_height;

        if row_top < self.scroll_offset {
            self.scroll_offset = row_top;
        } else if row_bottom > self.scroll_offset + self.viewport_height {
            self.scroll_offset = row_bottom - self.viewport_height;
        }
    }

    fn visible_nodes_ids(&self) -> Vec<TreeNodeId> {
        self.visible_nodes().iter().map(|n| n.id).collect()
    }

    fn collect_visible<'a>(nodes: &'a [TreeNode], result: &mut Vec<&'a TreeNode>) {
        for node in nodes {
            result.push(node);
            if node.expanded {
                Self::collect_visible(&node.children, result);
            }
        }
    }

    fn find_node(nodes: &[TreeNode], id: TreeNodeId) -> Option<&TreeNode> {
        for node in nodes {
            if node.id == id {
                return Some(node);
            }
            if let Some(found) = Self::find_node(&node.children, id) {
                return Some(found);
            }
        }
        None
    }

    fn find_node_mut(nodes: &mut [TreeNode], id: TreeNodeId) -> Option<&mut TreeNode> {
        for node in nodes.iter_mut() {
            if node.id == id {
                return Some(node);
            }
            if let Some(found) = Self::find_node_mut(&mut node.children, id) {
                return Some(found);
            }
        }
        None
    }

    fn compute_depths(nodes: &mut [TreeNode], depth: u32) {
        for node in nodes.iter_mut() {
            node.depth = depth;
            Self::compute_depths(&mut node.children, depth.saturating_add(1));
        }
    }

    fn set_expanded_recursive(nodes: &mut [TreeNode], expanded: bool) {
        for node in nodes.iter_mut() {
            if !node.children.is_empty() {
                node.expanded = expanded;
            }
            Self::set_expanded_recursive(&mut node.children, expanded);
        }
    }

    fn deselect_all_recursive(nodes: &mut [TreeNode]) {
        for node in nodes.iter_mut() {
            node.selected = false;
            Self::deselect_all_recursive(&mut node.children);
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

    fn sample_tree() -> Vec<TreeNode> {
        vec![
            TreeNode::branch(
                1,
                "Root A",
                vec![
                    TreeNode::leaf(2, "Child A1"),
                    TreeNode::branch(3, "Child A2", vec![TreeNode::leaf(4, "Grandchild A2a")]),
                ],
            ),
            TreeNode::leaf(5, "Root B"),
        ]
    }

    // ── Row geometry: does a click land on the row that was drawn? ───────

    /// The `(top, height)` of every row background the tree painted, in the
    /// order it painted them, in widget-relative coordinates.
    ///
    /// These are read out of the commands `render` actually emitted rather
    /// than recomputed from `row_height`: a test that recomputes the
    /// renderer's arithmetic and checks the hit test against *that* is
    /// worthless, because the two drift together.
    fn painted_rows(tree: &TreeView, height: f32) -> Vec<(f32, f32)> {
        let (ox, oy, w) = (0.0, 0.0, 200.0);
        tree.render(ox, oy, w, height)
            .into_iter()
            .filter_map(|cmd| match cmd {
                // Exact equality on purpose: these are the very floats the
                // renderer pushed, not a measurement of them. The full-width
                // rect is a row background; the viewport fill behind them all
                // is the one with the viewport's own height.
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height: h,
                    ..
                } if x == ox && width == w && h != height => Some((y - oy, h)),
                _ => None,
            })
            .collect()
    }

    /// A tree with every node expanded, so the visible list is long enough to
    /// scroll and every row is reachable.
    fn expanded_tree() -> TreeView {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.expand(1);
        tree.expand(3);
        tree
    }

    #[test]
    fn every_row_answers_exactly_where_it_was_painted() {
        let mut tree = expanded_tree();
        // Select every row in turn so each one paints a background to measure.
        let ids = tree.visible_nodes_ids();
        assert!(ids.len() >= 4, "need several rows to sweep");
        for (idx, &id) in ids.iter().enumerate() {
            tree.select(id);
            let rows = painted_rows(&tree, 400.0);
            let &(top, h) = rows
                .first()
                .unwrap_or_else(|| panic!("row {idx} painted no background to measure"));
            // Sweep the painted row rather than probing its middle. A drift of
            // a few pixels is invisible at the centre of a 24-px row and
            // obvious at its edges — which is where the user aims.
            for step in 0..8 {
                let probe = top + (step as f32) * h / 8.0;
                assert_eq!(
                    tree.row_at(probe),
                    Some(idx),
                    "row {idx} was painted at {top}..{} but {probe} answers otherwise",
                    top + h
                );
            }
            // A row owns its top edge and not its bottom one.
            assert_ne!(tree.row_at(top + h), Some(idx));
            if idx > 0 {
                assert_ne!(tree.row_at(top - 0.001), Some(idx));
            }
        }
    }

    #[test]
    fn a_point_above_the_first_row_selects_nothing_rather_than_the_first_node() {
        // This is the fault the two hand-written copies had: a negative `f32`
        // cast to `usize` saturates to zero in Rust, so `-5.0 / 24.0` became
        // row 0 and a click a few pixels above the tree selected its first
        // node. Nothing about that is visible to the user, who sees a
        // selection appear under a pointer that is not over it.
        let mut tree = expanded_tree();
        assert_eq!(tree.row_at(-0.001), None);
        assert_eq!(tree.row_at(-5.0), None);
        assert_eq!(tree.row_at(-1000.0), None);
        assert_eq!(tree.handle_click(0.0, -5.0, false), None);
        assert_eq!(tree.handle_context_menu(0.0, -5.0), None);
        assert!(
            !tree.visible_nodes().iter().any(|n| n.selected),
            "nothing should have been selected"
        );
    }

    #[test]
    fn a_coordinate_that_is_not_a_number_selects_nothing() {
        // NaN casts to zero too, so an unchecked coordinate had exactly the
        // same effect as a click on the first row.
        let mut tree = expanded_tree();
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(tree.row_at(bad), None, "{bad} named a row");
            assert_eq!(tree.handle_click(0.0, bad, false), None);
            assert_eq!(tree.handle_context_menu(0.0, bad), None);
        }
        assert!(!tree.visible_nodes().iter().any(|n| n.selected));
    }

    #[test]
    fn a_row_height_of_zero_selects_nothing_rather_than_everything() {
        // `row_height` is a public config field. Zero made the division
        // infinite or NaN, and both ends of that cast land on a real row.
        let mut tree = expanded_tree();
        tree.config.row_height = 0.0;
        assert_eq!(tree.row_at(0.0), None);
        assert_eq!(tree.row_at(50.0), None);
        assert_eq!(tree.handle_click(0.0, 50.0, false), None);
        tree.config.row_height = f32::NAN;
        assert_eq!(tree.row_at(50.0), None);
        tree.config.row_height = -24.0;
        assert_eq!(tree.row_at(50.0), None);
    }

    #[test]
    fn nothing_past_the_last_row_answers() {
        let tree = expanded_tree();
        let n = tree.visible_nodes().len();
        let h = tree.config.row_height;
        assert_eq!(tree.row_at((n as f32) * h - 0.001), Some(n - 1));
        assert_eq!(tree.row_at((n as f32) * h), None);
        assert_eq!(tree.row_at((n as f32) * h + 1000.0), None);
        // An empty tree answers for no point at all.
        let empty = TreeView::new(TreeConfig::default());
        assert_eq!(empty.row_at(0.0), None);
        assert_eq!(empty.row_at(10.0), None);
    }

    #[test]
    fn scrolling_moves_the_rows_and_the_answers_together() {
        let mut tree = expanded_tree();
        tree.set_viewport_height(48.0);
        let ids = tree.visible_nodes_ids();
        let last = ids.len() - 1;
        tree.select(ids[last]);
        tree.ensure_visible(last);
        assert!(tree.scroll_offset > 0.0, "the list should have scrolled");
        // The last row is now painted somewhere inside the viewport; the hit
        // test must follow it there rather than staying where it was drawn
        // before the scroll.
        let rows = painted_rows(&tree, 48.0);
        let &(top, h) = rows.first().expect("the selected row must still paint");
        for step in 0..8 {
            let probe = top + (step as f32) * h / 8.0;
            assert_eq!(tree.row_at(probe), Some(last));
        }
    }

    #[test]
    fn test_new_tree_is_empty() {
        let tree = TreeView::new(TreeConfig::default());
        assert!(tree.visible_nodes().is_empty());
        assert_eq!(tree.focused_id(), None);
    }

    #[test]
    fn test_set_nodes_computes_depth() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        assert_eq!(tree.get_node(1).map(|n| n.depth), Some(0));
        assert_eq!(tree.get_node(2).map(|n| n.depth), Some(1));
        assert_eq!(tree.get_node(4).map(|n| n.depth), Some(2));
    }

    #[test]
    fn test_visible_nodes_collapsed() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        // All collapsed: only root nodes visible
        let visible = tree.visible_nodes();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].id, 1);
        assert_eq!(visible[1].id, 5);
    }

    #[test]
    fn test_expand_shows_children() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.expand(1);
        let visible = tree.visible_nodes();
        assert_eq!(visible.len(), 4); // Root A, Child A1, Child A2, Root B
        assert_eq!(visible[1].id, 2);
        assert_eq!(visible[2].id, 3);
    }

    #[test]
    fn test_expand_all() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.expand_all();
        let visible = tree.visible_nodes();
        assert_eq!(visible.len(), 5); // All nodes visible
    }

    #[test]
    fn test_collapse_all() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.expand_all();
        tree.collapse_all();
        let visible = tree.visible_nodes();
        assert_eq!(visible.len(), 2); // Only roots
    }

    #[test]
    fn test_select_deselects_others_in_single_mode() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.expand_all();
        tree.select(2);
        assert_eq!(tree.get_node(2).map(|n| n.selected), Some(true));

        tree.select(4);
        assert_eq!(tree.get_node(4).map(|n| n.selected), Some(true));
        assert_eq!(tree.get_node(2).map(|n| n.selected), Some(false));
    }

    #[test]
    fn test_multi_select_preserves_selection() {
        let config = TreeConfig {
            multi_select: true,
            ..TreeConfig::default()
        };
        let mut tree = TreeView::new(config);
        tree.set_nodes(sample_tree());
        tree.expand_all();
        tree.select(2);
        tree.select(4);
        assert_eq!(tree.get_node(2).map(|n| n.selected), Some(true));
        assert_eq!(tree.get_node(4).map(|n| n.selected), Some(true));
    }

    #[test]
    fn test_toggle_expand_collapse() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.toggle(1);
        assert_eq!(tree.get_node(1).map(|n| n.expanded), Some(true));
        tree.toggle(1);
        assert_eq!(tree.get_node(1).map(|n| n.expanded), Some(false));
    }

    #[test]
    fn test_key_down_navigates() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.set_viewport_height(200.0);

        let key = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let event = tree.handle_key(&key);
        assert_eq!(event, Some(TreeEvent::Selected(1)));
        assert_eq!(tree.focused_id(), Some(1));

        let event = tree.handle_key(&key);
        assert_eq!(event, Some(TreeEvent::Selected(5)));
    }

    fn press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    /// Pressing into the end of the list must stay put and say nothing, rather
    /// than wrapping, panicking, or reporting a selection that did not change.
    #[test]
    fn arrowing_off_either_end_of_the_list_does_nothing() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.set_viewport_height(200.0);

        tree.select(1);
        assert_eq!(tree.handle_key(&press(Key::Up)), None);
        assert_eq!(tree.focused_id(), Some(1), "still on the first row");

        tree.select(5);
        assert_eq!(tree.handle_key(&press(Key::Down)), None);
        assert_eq!(tree.focused_id(), Some(5), "still on the last row");
    }

    /// Every key that moves the selection has to cope with there being nowhere
    /// to move it. `End` was the sharp one: it computed `len() - 1`.
    #[test]
    fn navigating_an_empty_tree_is_refused_rather_than_fatal() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_viewport_height(200.0);
        for key in [Key::Up, Key::Down, Key::Home, Key::End] {
            assert_eq!(
                tree.handle_key(&press(key)),
                None,
                "{key:?} on an empty tree"
            );
            assert_eq!(tree.focused_id(), None);
        }
    }

    /// Home and End reach the ends of the *visible* list — which expanding a
    /// branch changes — and Home additionally scrolls the list fully to the top
    /// rather than only far enough to reveal the first row.
    #[test]
    fn home_and_end_reach_the_ends_of_the_visible_list() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.set_viewport_height(40.0);
        tree.expand_all();

        assert_eq!(
            tree.handle_key(&press(Key::End)),
            Some(TreeEvent::Selected(5))
        );
        assert!(
            tree.scroll_offset > 0.0,
            "the last row was scrolled into view"
        );

        assert_eq!(
            tree.handle_key(&press(Key::Home)),
            Some(TreeEvent::Selected(1))
        );
        assert_eq!(tree.scroll_offset, 0.0, "Home goes to the very top");
    }

    /// The render loop bounds its row range by an estimate from the viewport
    /// height; a viewport taller than the list must still stop at the last row.
    #[test]
    fn rendering_a_viewport_taller_than_the_tree_stops_at_the_last_row() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.set_viewport_height(10_000.0);

        let commands = tree.render(0.0, 0.0, 300.0, 10_000.0);
        let labels: Vec<&str> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"Root A") && labels.contains(&"Root B"));
        assert!(
            !labels.contains(&"Child A1"),
            "collapsed children are not rows to render"
        );
    }

    #[test]
    fn test_key_right_expands() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.set_viewport_height(200.0);
        tree.select(1);

        let key = KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let event = tree.handle_key(&key);
        assert_eq!(event, Some(TreeEvent::Expanded(1)));
        assert_eq!(tree.get_node(1).map(|n| n.expanded), Some(true));
    }

    #[test]
    fn test_click_selects_row() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.set_viewport_height(200.0);

        // Click on second row (Root B), y = row_height * 1 + half = 36.0
        let event = tree.handle_click(50.0, 36.0, false);
        assert_eq!(event, Some(TreeEvent::Selected(5)));
    }

    #[test]
    fn test_render_produces_commands() {
        let mut tree = TreeView::new(TreeConfig::default());
        tree.set_nodes(sample_tree());
        tree.set_viewport_height(200.0);

        let commands = tree.render(0.0, 0.0, 300.0, 200.0);
        // Should have: background + clip + per-row items + pop_clip
        assert!(!commands.is_empty());
        // First command is background fill
        assert!(matches!(commands[0], RenderCommand::FillRect { .. }));
        // Second is clip
        assert!(matches!(commands[1], RenderCommand::PushClip { .. }));
        // Last is pop clip
        assert!(matches!(commands.last(), Some(RenderCommand::PopClip)));
    }
}

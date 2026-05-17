#![allow(dead_code)]
//! Grid view widget for icon-based displays.
//!
//! Provides a scrollable grid of items suitable for file explorers, image galleries,
//! and other grid-based content displays. Supports single/multi-selection, keyboard
//! navigation, rubber-band selection, type-ahead search, and virtual scrolling for
//! large collections.
//!
//! # Architecture
//!
//! The grid arranges items in a row-major layout. Only visible rows are rendered
//! (virtual scrolling). Each cell consists of an icon/image area on top and a
//! text label at the bottom.
//!
//! # Integration
//!
//! Conceptually integrates with the drag-and-drop system (`dnd.rs`): when a drag
//! threshold is exceeded, selected items can be exported as a `DataObject` via
//! the `DragDropManager`.

use crate::color::Color;
use crate::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::render::{FontWeightHint, RenderCommand, RenderTree};
use crate::style::CornerRadii;

// --- Catppuccin Mocha palette ---
// Used for selection highlights, hover, and chrome.
mod catppuccin {
    use crate::color::Color;

    /// Surface0 - subtle background layer.
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    /// Surface1 - elevated surface.
    pub const SURFACE1: Color = Color::from_hex(0x45475a);
    /// Surface2 - highest elevation surface.
    pub const SURFACE2: Color = Color::from_hex(0x585b70);
    /// Base - primary background.
    pub const BASE: Color = Color::from_hex(0x1e1e2e);
    /// Mantle - slightly darker background.
    pub const MANTLE: Color = Color::from_hex(0x181825);
    /// Crust - darkest background.
    pub const CRUST: Color = Color::from_hex(0x11111b);
    /// Text - primary text color.
    pub const TEXT: Color = Color::from_hex(0xcdd6f4);
    /// Subtext0 - dimmer text.
    pub const SUBTEXT0: Color = Color::from_hex(0xa6adc8);
    /// Blue - primary accent.
    pub const BLUE: Color = Color::from_hex(0x89b4fa);
    /// Lavender - secondary accent.
    pub const LAVENDER: Color = Color::from_hex(0xb4befe);
    /// Sapphire - alternate accent.
    pub const SAPPHIRE: Color = Color::from_hex(0x74c7ec);
    /// Overlay0 - selection/rubber-band outline.
    pub const OVERLAY0: Color = Color::from_hex(0x6c7086);
}

// =============================================================================
// Configuration
// =============================================================================

/// How the cell size is determined.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CellSizing {
    /// Fixed width and height for each cell.
    Fixed { width: f32, height: f32 },
    /// Cell width computed automatically to fit columns within the container,
    /// with a fixed aspect ratio (height = width * ratio).
    Auto { min_width: f32, aspect_ratio: f32 },
}

impl Default for CellSizing {
    fn default() -> Self {
        Self::Fixed {
            width: 100.0,
            height: 120.0,
        }
    }
}

/// Grid layout configuration.
#[derive(Clone, Debug)]
pub struct GridConfig {
    /// How cells are sized.
    pub cell_sizing: CellSizing,
    /// Horizontal gap between cells.
    pub gap_x: f32,
    /// Vertical gap between cells.
    pub gap_y: f32,
    /// Padding around the entire grid content area.
    pub padding: f32,
    /// Height of the icon/image area within a cell (fraction of cell height, 0.0-1.0).
    pub icon_area_ratio: f32,
    /// Font size for item labels.
    pub label_font_size: f32,
    /// Whether to allow multi-selection.
    pub multi_select: bool,
    /// Drag threshold in pixels before a drag operation starts.
    pub drag_threshold: f32,
    /// Number of pixels per scroll step (for arrow key scrolling).
    pub scroll_step: f32,
    /// Whether smooth scrolling is enabled.
    pub smooth_scroll: bool,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            cell_sizing: CellSizing::default(),
            gap_x: 8.0,
            gap_y: 8.0,
            padding: 12.0,
            icon_area_ratio: 0.65,
            label_font_size: 12.0,
            multi_select: true,
            drag_threshold: 5.0,
            scroll_step: 40.0,
            smooth_scroll: true,
        }
    }
}

// =============================================================================
// Grid items
// =============================================================================

/// An item displayed in the grid.
#[derive(Clone, Debug)]
pub struct GridItem {
    /// Unique identifier for this item.
    pub id: u64,
    /// Display label (shown below the icon).
    pub label: String,
    /// Image/icon asset ID (references image in an asset store).
    pub icon_id: Option<u64>,
    /// Optional small badge/overlay (e.g. checkmark, file type indicator).
    pub badge: Option<Badge>,
    /// Application-defined data associated with this item.
    pub user_data: u64,
}

/// A badge overlay displayed on a grid item's icon area.
#[derive(Clone, Debug)]
pub struct Badge {
    /// Badge icon asset ID.
    pub icon_id: u64,
    /// Position of the badge relative to the cell.
    pub position: BadgePosition,
}

/// Where a badge is anchored within the cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgePosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

// =============================================================================
// Selection state
// =============================================================================

/// Tracks selected items and the selection anchor for range operations.
#[derive(Clone, Debug, Default)]
pub struct SelectionState {
    /// Set of selected item indices.
    selected: Vec<usize>,
    /// Anchor index for shift-click range selection.
    anchor: Option<usize>,
    /// Most recently focused item index (for keyboard navigation).
    focus: Option<usize>,
}

impl SelectionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if the given index is selected.
    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    /// Returns all selected indices (sorted).
    pub fn selected_indices(&self) -> &[usize] {
        &self.selected
    }

    /// Number of selected items.
    pub fn count(&self) -> usize {
        self.selected.len()
    }

    /// Clear all selection.
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.focus = None;
    }

    /// Select a single item, clearing previous selection.
    pub fn select_single(&mut self, index: usize) {
        self.selected.clear();
        self.selected.push(index);
        self.anchor = Some(index);
        self.focus = Some(index);
    }

    /// Toggle selection of a single item (Ctrl+Click behavior).
    pub fn toggle(&mut self, index: usize) {
        if let Some(pos) = self.selected.iter().position(|&i| i == index) {
            self.selected.remove(pos);
        } else {
            self.selected.push(index);
            self.selected.sort_unstable();
        }
        self.anchor = Some(index);
        self.focus = Some(index);
    }

    /// Select a range from anchor to the given index (Shift+Click behavior).
    /// If no anchor exists, behaves like select_single.
    pub fn select_range(&mut self, index: usize) {
        let anchor = self.anchor.unwrap_or(index);
        self.selected.clear();

        let (start, end) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };

        for i in start..=end {
            self.selected.push(i);
        }
        self.focus = Some(index);
    }

    /// Select all items up to the given count.
    pub fn select_all(&mut self, item_count: usize) {
        self.selected.clear();
        for i in 0..item_count {
            self.selected.push(i);
        }
        if item_count > 0 {
            self.focus = Some(0);
        }
    }

    /// Add a range of indices to the current selection (for rubber-band).
    pub fn add_range(&mut self, indices: &[usize]) {
        for &idx in indices {
            if !self.selected.contains(&idx) {
                self.selected.push(idx);
            }
        }
        self.selected.sort_unstable();
    }

    /// Set selection to exactly the given indices (for rubber-band replace).
    pub fn set_selection(&mut self, indices: &[usize]) {
        self.selected.clear();
        self.selected.extend_from_slice(indices);
        self.selected.sort_unstable();
        self.selected.dedup();
    }

    /// The currently focused item (for keyboard nav rendering).
    pub fn focused(&self) -> Option<usize> {
        self.focus
    }

    /// Set focused index without changing selection.
    pub fn set_focus(&mut self, index: usize) {
        self.focus = Some(index);
    }
}

// =============================================================================
// Rubber-band (lasso) selection
// =============================================================================

/// State for an active rubber-band selection drag.
#[derive(Clone, Debug)]
struct RubberBand {
    /// Starting point (where the mouse was pressed, in grid-local coordinates).
    start_x: f32,
    start_y: f32,
    /// Current point (where the mouse is now).
    current_x: f32,
    current_y: f32,
    /// Selection state at the time the rubber-band started (for additive mode).
    original_selection: Vec<usize>,
}

impl RubberBand {
    fn new(x: f32, y: f32, original_selection: Vec<usize>) -> Self {
        Self {
            start_x: x,
            start_y: y,
            current_x: x,
            current_y: y,
            original_selection,
        }
    }

    /// Returns the rectangle as (x, y, width, height), always positive dimensions.
    fn rect(&self) -> (f32, f32, f32, f32) {
        let x = self.start_x.min(self.current_x);
        let y = self.start_y.min(self.current_y);
        let w = (self.start_x - self.current_x).abs();
        let h = (self.start_y - self.current_y).abs();
        (x, y, w, h)
    }
}

// =============================================================================
// Drag state
// =============================================================================

/// State tracking for potential drag operations.
#[derive(Clone, Debug)]
enum DragState {
    /// No drag in progress.
    Idle,
    /// Mouse pressed on an item — waiting to exceed threshold.
    Pending {
        start_x: f32,
        start_y: f32,
        item_index: usize,
    },
    /// Drag is active (threshold exceeded).
    Dragging {
        item_index: usize,
    },
}

// =============================================================================
// Type-ahead search
// =============================================================================

/// State for type-ahead (incremental search by typing).
#[derive(Clone, Debug, Default)]
struct TypeAhead {
    /// Characters typed so far.
    buffer: String,
    /// Timestamp of last keystroke (for timeout reset), in milliseconds.
    last_input_ms: u64,
}

impl TypeAhead {
    /// Timeout after which the buffer resets (ms).
    const TIMEOUT_MS: u64 = 1000;

    /// Push a character, returning the current search string.
    /// Resets buffer if too much time has elapsed.
    fn push(&mut self, ch: char, now_ms: u64) -> &str {
        if now_ms.saturating_sub(self.last_input_ms) > Self::TIMEOUT_MS {
            self.buffer.clear();
        }
        self.buffer.push(ch);
        self.last_input_ms = now_ms;
        &self.buffer
    }

    fn clear(&mut self) {
        self.buffer.clear();
    }
}

// =============================================================================
// Events / callbacks
// =============================================================================

/// Events emitted by the grid view to application code.
#[derive(Clone, Debug)]
pub enum GridEvent {
    /// Selection changed. Contains new set of selected item indices.
    SelectionChanged(Vec<usize>),
    /// Item activated (double-click or Enter). Contains the item index.
    Activate(usize),
    /// Context menu requested (right-click). Contains item index and position.
    ContextMenu {
        index: Option<usize>,
        x: f32,
        y: f32,
    },
    /// A drag operation started with the given selected item indices.
    DragStarted(Vec<usize>),
}

// =============================================================================
// Computed layout cache
// =============================================================================

/// Precomputed grid layout metrics, recalculated when the container size
/// or configuration changes.
#[derive(Clone, Debug)]
struct LayoutCache {
    /// Width of each cell.
    cell_width: f32,
    /// Height of each cell.
    cell_height: f32,
    /// Number of columns that fit in the container.
    columns: usize,
    /// Total number of rows needed for all items.
    total_rows: usize,
    /// Total content height (for scrolling).
    content_height: f32,
    /// Container width used to compute this cache.
    container_width: f32,
    /// Container height (viewport).
    container_height: f32,
}

impl LayoutCache {
    fn compute(config: &GridConfig, container_width: f32, container_height: f32, item_count: usize) -> Self {
        let usable_width = (container_width - config.padding * 2.0).max(0.0);

        let (cell_width, cell_height) = match config.cell_sizing {
            CellSizing::Fixed { width, height } => (width, height),
            CellSizing::Auto { min_width, aspect_ratio } => {
                // Fit as many columns as possible with at least min_width per cell.
                let cols = ((usable_width + config.gap_x) / (min_width + config.gap_x))
                    .floor()
                    .max(1.0) as usize;
                let w = (usable_width - config.gap_x * (cols as f32 - 1.0).max(0.0)) / cols as f32;
                (w, w * aspect_ratio)
            }
        };

        // Number of columns that fit.
        let columns = if cell_width <= 0.0 {
            1
        } else {
            ((usable_width + config.gap_x) / (cell_width + config.gap_x))
                .floor()
                .max(1.0) as usize
        };

        let total_rows = if item_count == 0 {
            0
        } else {
            item_count.div_ceil(columns)
        };

        let content_height = if total_rows == 0 {
            0.0
        } else {
            config.padding * 2.0
                + total_rows as f32 * cell_height
                + (total_rows as f32 - 1.0).max(0.0) * config.gap_y
        };

        Self {
            cell_width,
            cell_height,
            columns,
            total_rows,
            content_height,
            container_width,
            container_height,
        }
    }

    /// Returns the (col, row) for a given item index.
    fn item_position(&self, index: usize) -> (usize, usize) {
        let col = index % self.columns;
        let row = index / self.columns;
        (col, row)
    }

    /// Returns the pixel position (x, y) of the top-left corner of a cell.
    fn cell_origin(&self, index: usize, padding: f32, gap_x: f32, gap_y: f32) -> (f32, f32) {
        let (col, row) = self.item_position(index);
        let x = padding + col as f32 * (self.cell_width + gap_x);
        let y = padding + row as f32 * (self.cell_height + gap_y);
        (x, y)
    }

    /// The range of rows visible at the given scroll offset.
    fn visible_rows(&self, scroll_y: f32, config: &GridConfig) -> (usize, usize) {
        if self.total_rows == 0 {
            return (0, 0);
        }

        let row_height = self.cell_height + config.gap_y;
        if row_height <= 0.0 {
            return (0, self.total_rows);
        }

        let first_visible = ((scroll_y - config.padding).max(0.0) / row_height).floor() as usize;
        let visible_count = (self.container_height / row_height).ceil() as usize + 2; // +2 for partial rows
        let last_visible = (first_visible + visible_count).min(self.total_rows);

        (first_visible, last_visible)
    }

    /// Hit-test: given a point in content coordinates, returns the item index (if any).
    fn hit_test(&self, x: f32, y: f32, padding: f32, gap_x: f32, gap_y: f32, item_count: usize) -> Option<usize> {
        let content_x = x - padding;
        let content_y = y - padding;

        if content_x < 0.0 || content_y < 0.0 {
            return None;
        }

        let col_stride = self.cell_width + gap_x;
        let row_stride = self.cell_height + gap_y;

        if col_stride <= 0.0 || row_stride <= 0.0 {
            return None;
        }

        let col = (content_x / col_stride).floor() as usize;
        let row = (content_y / row_stride).floor() as usize;

        // Check we are within the cell bounds (not in the gap).
        let cell_x = content_x - col as f32 * col_stride;
        let cell_y = content_y - row as f32 * row_stride;

        if cell_x > self.cell_width || cell_y > self.cell_height {
            return None; // Click was in the gap between cells.
        }

        if col >= self.columns {
            return None;
        }

        let index = row * self.columns + col;
        if index >= item_count {
            return None;
        }

        Some(index)
    }

    /// Returns all item indices whose cells intersect the given rectangle (in content coords).
    #[allow(clippy::too_many_arguments)]
    fn items_in_rect(
        &self,
        rx: f32,
        ry: f32,
        rw: f32,
        rh: f32,
        padding: f32,
        gap_x: f32,
        gap_y: f32,
        item_count: usize,
    ) -> Vec<usize> {
        let mut result = Vec::new();
        if self.columns == 0 || item_count == 0 {
            return result;
        }

        let col_stride = self.cell_width + gap_x;
        let row_stride = self.cell_height + gap_y;

        if col_stride <= 0.0 || row_stride <= 0.0 {
            return result;
        }

        // Determine the range of rows and columns the rectangle could overlap.
        let start_col = ((rx - padding).max(0.0) / col_stride).floor() as usize;
        let end_col = (((rx + rw - padding).max(0.0) / col_stride).floor() as usize).min(self.columns.saturating_sub(1));
        let start_row = ((ry - padding).max(0.0) / row_stride).floor() as usize;
        let end_row = ((ry + rh - padding).max(0.0) / row_stride).floor() as usize;

        for row in start_row..=end_row {
            for col in start_col..=end_col {
                let idx = row * self.columns + col;
                if idx >= item_count {
                    break;
                }

                // Verify actual intersection with the cell (not just the stride).
                let cx = padding + col as f32 * col_stride;
                let cy = padding + row as f32 * row_stride;

                let intersects = rx < cx + self.cell_width
                    && rx + rw > cx
                    && ry < cy + self.cell_height
                    && ry + rh > cy;

                if intersects {
                    result.push(idx);
                }
            }
        }

        result
    }
}

// =============================================================================
// GridView widget
// =============================================================================

/// A grid view widget that displays items in a scrollable grid layout.
///
/// Use this for file explorer icon views, image galleries, and other
/// grid-based displays with selection, keyboard navigation, and drag support.
pub struct GridView {
    /// Configuration.
    pub config: GridConfig,
    /// Items displayed in the grid.
    items: Vec<GridItem>,
    /// Selection state.
    selection: SelectionState,
    /// Current vertical scroll offset in pixels.
    scroll_y: f32,
    /// Target scroll offset for smooth scrolling.
    scroll_target_y: f32,
    /// Cached layout metrics.
    layout: Option<LayoutCache>,
    /// Container dimensions (last known).
    container_width: f32,
    container_height: f32,
    /// Rubber-band selection state.
    rubber_band: Option<RubberBand>,
    /// Drag state.
    drag: DragState,
    /// Type-ahead search state.
    type_ahead: TypeAhead,
    /// Monotonic time in ms (updated via Tick events).
    current_time_ms: u64,
    /// Pending events for the application to consume.
    pending_events: Vec<GridEvent>,
}

impl GridView {
    /// Create a new grid view with default configuration.
    pub fn new() -> Self {
        Self {
            config: GridConfig::default(),
            items: Vec::new(),
            selection: SelectionState::new(),
            scroll_y: 0.0,
            scroll_target_y: 0.0,
            layout: None,
            container_width: 0.0,
            container_height: 0.0,
            rubber_band: None,
            drag: DragState::Idle,
            type_ahead: TypeAhead::default(),
            current_time_ms: 0,
            pending_events: Vec::new(),
        }
    }

    /// Create a grid view with the given configuration.
    pub fn with_config(config: GridConfig) -> Self {
        Self {
            config,
            ..Self::new()
        }
    }

    // -------------------------------------------------------------------------
    // Item management
    // -------------------------------------------------------------------------

    /// Set the items displayed in the grid, clearing selection.
    pub fn set_items(&mut self, items: Vec<GridItem>) {
        self.items = items;
        self.selection.clear();
        self.invalidate_layout();
        self.clamp_scroll();
    }

    /// Get a reference to items.
    pub fn items(&self) -> &[GridItem] {
        &self.items
    }

    /// Number of items.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Get the selection state.
    pub fn selection(&self) -> &SelectionState {
        &self.selection
    }

    /// Mutably access the selection state.
    pub fn selection_mut(&mut self) -> &mut SelectionState {
        &mut self.selection
    }

    // -------------------------------------------------------------------------
    // Layout
    // -------------------------------------------------------------------------

    /// Update the container size (call when the widget is resized).
    pub fn set_container_size(&mut self, width: f32, height: f32) {
        if (self.container_width - width).abs() > 0.5
            || (self.container_height - height).abs() > 0.5
        {
            self.container_width = width;
            self.container_height = height;
            self.invalidate_layout();
            self.clamp_scroll();
        }
    }

    /// Force layout recalculation.
    fn invalidate_layout(&mut self) {
        self.layout = None;
    }

    /// Ensure layout cache is up to date.
    fn ensure_layout(&mut self) {
        if self.layout.is_none() {
            self.layout = Some(LayoutCache::compute(
                &self.config,
                self.container_width,
                self.container_height,
                self.items.len(),
            ));
        }
    }

    /// Get or compute the layout cache.
    fn layout(&self) -> LayoutCache {
        self.layout.clone().unwrap_or_else(|| {
            LayoutCache::compute(
                &self.config,
                self.container_width,
                self.container_height,
                self.items.len(),
            )
        })
    }

    /// Number of columns in the current layout.
    pub fn columns(&self) -> usize {
        self.layout().columns
    }

    /// Total content height (for external scrollbar integration).
    pub fn content_height(&self) -> f32 {
        self.layout().content_height
    }

    // -------------------------------------------------------------------------
    // Scrolling
    // -------------------------------------------------------------------------

    /// Current scroll offset.
    pub fn scroll_y(&self) -> f32 {
        self.scroll_y
    }

    /// Set scroll offset directly.
    pub fn set_scroll_y(&mut self, y: f32) {
        self.scroll_target_y = y;
        self.scroll_y = y;
        self.clamp_scroll();
    }

    /// Scroll to make the given item index visible.
    pub fn scroll_to_item(&mut self, index: usize) {
        self.ensure_layout();
        let layout = self.layout();

        let (_, row) = layout.item_position(index);
        let row_top = self.config.padding + row as f32 * (layout.cell_height + self.config.gap_y);
        let row_bottom = row_top + layout.cell_height;

        if row_top < self.scroll_y {
            self.scroll_target_y = row_top;
        } else if row_bottom > self.scroll_y + self.container_height {
            self.scroll_target_y = row_bottom - self.container_height;
        }

        if !self.config.smooth_scroll {
            self.scroll_y = self.scroll_target_y;
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let layout = self.layout();
        let max_scroll = (layout.content_height - self.container_height).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max_scroll);
        self.scroll_target_y = self.scroll_target_y.clamp(0.0, max_scroll);
    }

    /// Advance smooth scrolling animation. Call each frame/tick.
    pub fn tick_scroll(&mut self, _dt_ms: u64) {
        if !self.config.smooth_scroll {
            self.scroll_y = self.scroll_target_y;
            return;
        }
        // Exponential ease toward target.
        let diff = self.scroll_target_y - self.scroll_y;
        if diff.abs() < 0.5 {
            self.scroll_y = self.scroll_target_y;
        } else {
            self.scroll_y += diff * 0.2;
        }
    }

    // -------------------------------------------------------------------------
    // Event handling
    // -------------------------------------------------------------------------

    /// Drain pending grid events (call after handle_event).
    pub fn drain_events(&mut self) -> Vec<GridEvent> {
        core::mem::take(&mut self.pending_events)
    }

    /// Handle an input event. Returns whether the event was consumed.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Key(key) => self.handle_key(key),
            Event::Resize { width, height } => {
                self.set_container_size(*width as f32, *height as f32);
                EventResult::Consumed
            }
            Event::Tick { elapsed_ms } => {
                self.current_time_ms = *elapsed_ms;
                self.tick_scroll(*elapsed_ms);
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match &mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => self.handle_left_press(mouse.x, mouse.y),
            MouseEventKind::Release(MouseButton::Left) => self.handle_left_release(mouse.x, mouse.y),
            MouseEventKind::Move => self.handle_mouse_move(mouse.x, mouse.y),
            MouseEventKind::DoubleClick(MouseButton::Left) => self.handle_double_click(mouse.x, mouse.y),
            MouseEventKind::Press(MouseButton::Right) => self.handle_right_click(mouse.x, mouse.y),
            MouseEventKind::Scroll { dx: _, dy } => self.handle_scroll(*dy),
            _ => EventResult::Ignored,
        }
    }

    fn handle_left_press(&mut self, x: f32, y: f32) -> EventResult {
        self.ensure_layout();
        let content_y = y + self.scroll_y;
        let layout = self.layout();
        let hit = layout.hit_test(
            x, content_y,
            self.config.padding, self.config.gap_x, self.config.gap_y,
            self.items.len(),
        );

        if let Some(index) = hit {
            // Potential drag start — record press position.
            self.drag = DragState::Pending {
                start_x: x,
                start_y: y,
                item_index: index,
            };
            // Do not change selection yet — wait for release or drag threshold.
        } else {
            // Click on empty space — start rubber-band or clear selection.
            let original = if self.config.multi_select {
                // If Ctrl is held, we will handle modifiers in release;
                // for rubber-band start, preserve selection.
                self.selection.selected_indices().to_vec()
            } else {
                Vec::new()
            };
            self.rubber_band = Some(RubberBand::new(x, content_y, original));
            self.drag = DragState::Idle;
        }

        EventResult::Consumed
    }

    fn handle_left_release(&mut self, x: f32, y: f32) -> EventResult {
        // End rubber-band if active.
        if self.rubber_band.is_some() {
            self.rubber_band = None;
            self.emit_selection_changed();
            return EventResult::Consumed;
        }

        // Handle drag end or click-to-select.
        match core::mem::replace(&mut self.drag, DragState::Idle) {
            DragState::Pending { item_index, .. } => {
                // Did not exceed threshold — treat as a click.
                self.apply_click_selection(item_index, x, y);
                self.emit_selection_changed();
                EventResult::Consumed
            }
            DragState::Dragging { .. } => {
                // Drag ended — the drop target handles it via dnd system.
                EventResult::Consumed
            }
            DragState::Idle => EventResult::Ignored,
        }
    }

    fn handle_mouse_move(&mut self, x: f32, y: f32) -> EventResult {
        // Update rubber-band.
        if self.rubber_band.is_some() {
            let content_y = y + self.scroll_y;

            // Update position on the rubber band.
            let rb = self.rubber_band.as_mut().expect("checked above");
            rb.current_x = x;
            rb.current_y = content_y;

            // Extract the rectangle and original selection before releasing the borrow.
            let (rx, ry, rw, rh) = rb.rect();
            let original = rb.original_selection.clone();

            // Now compute layout (no longer borrowing rubber_band).
            self.ensure_layout();
            let layout = self.layout();
            let intersected = layout.items_in_rect(
                rx, ry, rw, rh,
                self.config.padding, self.config.gap_x, self.config.gap_y,
                self.items.len(),
            );

            // Merge with original selection (additive rubber-band).
            let mut combined = original;
            for idx in &intersected {
                if !combined.contains(idx) {
                    combined.push(*idx);
                }
            }
            combined.sort_unstable();
            self.selection.set_selection(&combined);
            return EventResult::Consumed;
        }

        // Check drag threshold.
        if let DragState::Pending { start_x, start_y, item_index } = self.drag {
            let dx = x - start_x;
            let dy = y - start_y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist >= self.config.drag_threshold {
                self.drag = DragState::Dragging { item_index };
                // Ensure the dragged item is selected.
                if !self.selection.is_selected(item_index) {
                    self.selection.select_single(item_index);
                }
                let selected = self.selection.selected_indices().to_vec();
                self.pending_events.push(GridEvent::DragStarted(selected));
                return EventResult::Consumed;
            }
        }

        EventResult::Ignored
    }

    fn handle_double_click(&mut self, x: f32, y: f32) -> EventResult {
        self.ensure_layout();
        let content_y = y + self.scroll_y;
        let layout = self.layout();
        let hit = layout.hit_test(
            x, content_y,
            self.config.padding, self.config.gap_x, self.config.gap_y,
            self.items.len(),
        );

        if let Some(index) = hit {
            self.selection.select_single(index);
            self.pending_events.push(GridEvent::Activate(index));
            return EventResult::Consumed;
        }

        EventResult::Ignored
    }

    fn handle_right_click(&mut self, x: f32, y: f32) -> EventResult {
        self.ensure_layout();
        let content_y = y + self.scroll_y;
        let layout = self.layout();
        let hit = layout.hit_test(
            x, content_y,
            self.config.padding, self.config.gap_x, self.config.gap_y,
            self.items.len(),
        );

        // If right-clicking an unselected item, select it first.
        if let Some(index) = hit.filter(|&i| !self.selection.is_selected(i)) {
            self.selection.select_single(index);
            self.emit_selection_changed();
        }

        self.pending_events.push(GridEvent::ContextMenu {
            index: hit,
            x,
            y,
        });

        EventResult::Consumed
    }

    fn handle_scroll(&mut self, dy: f32) -> EventResult {
        self.scroll_target_y -= dy;
        if !self.config.smooth_scroll {
            self.scroll_y = self.scroll_target_y;
        }
        self.clamp_scroll();
        EventResult::Consumed
    }

    /// Apply click selection with modifier awareness.
    /// We detect modifiers from the calling context (simplified: we pass dummy modifiers
    /// and rely on key state — in a real implementation, modifier state comes from the event).
    fn apply_click_selection(&mut self, index: usize, _x: f32, _y: f32) {
        // In the absence of inline modifier info in MouseEvent, we select single.
        // Modifier-aware selection is handled via handle_click_with_modifiers.
        self.selection.select_single(index);
    }

    /// Public method to apply a click with known modifiers (for host integration).
    pub fn handle_click_with_modifiers(&mut self, index: usize, modifiers: Modifiers) {
        if modifiers.ctrl && self.config.multi_select {
            self.selection.toggle(index);
        } else if modifiers.shift && self.config.multi_select {
            self.selection.select_range(index);
        } else {
            self.selection.select_single(index);
        }
        self.emit_selection_changed();
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }

        match key.key {
            Key::Up => self.navigate(-1, 0, key.modifiers),
            Key::Down => self.navigate(1, 0, key.modifiers),
            Key::Left => self.navigate(0, -1, key.modifiers),
            Key::Right => self.navigate(0, 1, key.modifiers),
            Key::Home => self.navigate_to(0, key.modifiers),
            Key::End => self.navigate_to(self.items.len().saturating_sub(1), key.modifiers),
            Key::PageUp => self.navigate_page(-1),
            Key::PageDown => self.navigate_page(1),
            Key::Enter => self.activate_focused(),
            Key::A if key.modifiers.ctrl => {
                self.selection.select_all(self.items.len());
                self.emit_selection_changed();
                EventResult::Consumed
            }
            _ => {
                // Type-ahead search: printable characters.
                if let Some(ch) = key.text.filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '.' || *c == '_' || *c == '-') {
                    return self.type_ahead_search(ch);
                }
                EventResult::Ignored
            }
        }
    }

    /// Navigate by row/column offset.
    fn navigate(&mut self, row_delta: i32, col_delta: i32, modifiers: Modifiers) -> EventResult {
        if self.items.is_empty() {
            return EventResult::Ignored;
        }

        self.ensure_layout();
        let layout = self.layout();
        let current = self.selection.focused().unwrap_or(0);
        let (col, row) = layout.item_position(current);

        let new_col = (col as i32 + col_delta).clamp(0, layout.columns as i32 - 1) as usize;
        let new_row = (row as i32 + row_delta).clamp(0, layout.total_rows as i32 - 1) as usize;
        let new_index = (new_row * layout.columns + new_col).min(self.items.len().saturating_sub(1));

        if modifiers.shift && self.config.multi_select {
            self.selection.select_range(new_index);
            self.selection.set_focus(new_index);
        } else {
            self.selection.select_single(new_index);
        }

        self.scroll_to_item(new_index);
        self.emit_selection_changed();
        EventResult::Consumed
    }

    /// Navigate directly to an index.
    fn navigate_to(&mut self, index: usize, modifiers: Modifiers) -> EventResult {
        if self.items.is_empty() {
            return EventResult::Ignored;
        }

        let target = index.min(self.items.len().saturating_sub(1));

        if modifiers.shift && self.config.multi_select {
            self.selection.select_range(target);
            self.selection.set_focus(target);
        } else {
            self.selection.select_single(target);
        }

        self.scroll_to_item(target);
        self.emit_selection_changed();
        EventResult::Consumed
    }

    /// Navigate by page (visible rows).
    fn navigate_page(&mut self, direction: i32) -> EventResult {
        if self.items.is_empty() {
            return EventResult::Ignored;
        }

        self.ensure_layout();
        let layout = self.layout();

        let rows_per_page = if layout.cell_height + self.config.gap_y > 0.0 {
            (self.container_height / (layout.cell_height + self.config.gap_y)).floor() as i32
        } else {
            1
        };

        let current = self.selection.focused().unwrap_or(0);
        let (col, row) = layout.item_position(current);
        let new_row = (row as i32 + direction * rows_per_page).clamp(0, layout.total_rows as i32 - 1) as usize;
        let new_index = (new_row * layout.columns + col).min(self.items.len().saturating_sub(1));

        self.selection.select_single(new_index);
        self.scroll_to_item(new_index);
        self.emit_selection_changed();
        EventResult::Consumed
    }

    /// Activate the currently focused item.
    fn activate_focused(&mut self) -> EventResult {
        if let Some(index) = self.selection.focused().filter(|&i| i < self.items.len()) {
            self.pending_events.push(GridEvent::Activate(index));
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    /// Type-ahead search: find first item whose label starts with the typed prefix.
    fn type_ahead_search(&mut self, ch: char) -> EventResult {
        let now = self.current_time_ms;
        let prefix = self.type_ahead.push(ch, now).to_lowercase();

        for (i, item) in self.items.iter().enumerate() {
            if item.label.to_lowercase().starts_with(&prefix) {
                self.selection.select_single(i);
                self.scroll_to_item(i);
                self.emit_selection_changed();
                return EventResult::Consumed;
            }
        }

        EventResult::Consumed
    }

    fn emit_selection_changed(&mut self) {
        let indices = self.selection.selected_indices().to_vec();
        self.pending_events.push(GridEvent::SelectionChanged(indices));
    }

    // -------------------------------------------------------------------------
    // Rendering
    // -------------------------------------------------------------------------

    /// Render the grid view into a render tree.
    pub fn render(&mut self, tree: &mut RenderTree) {
        self.ensure_layout();
        let layout = self.layout();

        // Background.
        tree.fill_rect(
            0.0, 0.0,
            self.container_width, self.container_height,
            catppuccin::BASE,
        );

        // Clip to container.
        tree.clip(0.0, 0.0, self.container_width, self.container_height);

        // Translate for scroll offset.
        tree.translate(0.0, -self.scroll_y);

        // Determine visible row range.
        let (first_row, last_row) = layout.visible_rows(self.scroll_y, &self.config);

        // Render visible items.
        let first_item = first_row * layout.columns;
        let last_item = (last_row * layout.columns).min(self.items.len());

        for index in first_item..last_item {
            if index >= self.items.len() {
                break;
            }
            self.render_cell(tree, &layout, index);
        }

        // Render rubber-band overlay.
        if let Some(ref rb) = self.rubber_band {
            let (rx, ry, rw, rh) = rb.rect();
            // Semi-transparent fill.
            tree.push(RenderCommand::FillRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: Color::rgba(
                    catppuccin::BLUE.r,
                    catppuccin::BLUE.g,
                    catppuccin::BLUE.b,
                    40,
                ),
                corner_radii: CornerRadii::ZERO,
            });
            // Border.
            tree.push(RenderCommand::StrokeRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: catppuccin::BLUE,
                line_width: 1.0,
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Undo scroll translate.
        tree.untranslate();
        tree.unclip();
    }

    /// Render a single grid cell.
    fn render_cell(&self, tree: &mut RenderTree, layout: &LayoutCache, index: usize) {
        let item = &self.items[index];
        let (cx, cy) = layout.cell_origin(index, self.config.padding, self.config.gap_x, self.config.gap_y);
        let cw = layout.cell_width;
        let ch = layout.cell_height;

        let is_selected = self.selection.is_selected(index);
        let is_focused = self.selection.focused() == Some(index);

        // Selection highlight background.
        if is_selected {
            tree.push(RenderCommand::FillRect {
                x: cx,
                y: cy,
                width: cw,
                height: ch,
                color: catppuccin::SURFACE1,
                corner_radii: CornerRadii::all(6.0),
            });
            // Focused item gets a border to distinguish it.
            if is_focused {
                tree.push(RenderCommand::StrokeRect {
                    x: cx,
                    y: cy,
                    width: cw,
                    height: ch,
                    color: catppuccin::LAVENDER,
                    line_width: 1.5,
                    corner_radii: CornerRadii::all(6.0),
                });
            }
        } else if is_focused {
            // Focused but not selected — subtle dashed outline.
            tree.push(RenderCommand::StrokeRect {
                x: cx,
                y: cy,
                width: cw,
                height: ch,
                color: catppuccin::OVERLAY0,
                line_width: 1.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Icon/image area.
        let icon_height = ch * self.config.icon_area_ratio;
        let icon_padding = 8.0;
        let icon_area_size = (icon_height - icon_padding * 2.0).min(cw - icon_padding * 2.0);

        if let Some(image_id) = item.icon_id {
            let icon_x = cx + (cw - icon_area_size) / 2.0;
            let icon_y = cy + icon_padding;
            tree.push(RenderCommand::Image {
                x: icon_x,
                y: icon_y,
                width: icon_area_size,
                height: icon_area_size,
                image_id,
            });
        } else {
            // Placeholder icon area (gray rounded rect).
            let icon_x = cx + (cw - icon_area_size) / 2.0;
            let icon_y = cy + icon_padding;
            tree.push(RenderCommand::FillRect {
                x: icon_x,
                y: icon_y,
                width: icon_area_size,
                height: icon_area_size,
                color: catppuccin::SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
        }

        // Badge overlay.
        if let Some(ref badge) = item.badge {
            let badge_size = 16.0;
            let (bx, by) = match badge.position {
                BadgePosition::TopLeft => (cx + 4.0, cy + 4.0),
                BadgePosition::TopRight => (cx + cw - badge_size - 4.0, cy + 4.0),
                BadgePosition::BottomLeft => (cx + 4.0, cy + icon_height - badge_size - 4.0),
                BadgePosition::BottomRight => (cx + cw - badge_size - 4.0, cy + icon_height - badge_size - 4.0),
            };
            tree.push(RenderCommand::Image {
                x: bx,
                y: by,
                width: badge_size,
                height: badge_size,
                image_id: badge.icon_id,
            });
        }

        // Text label (truncated with max_width).
        let label_y = cy + icon_height + 4.0;
        let label_color = if is_selected {
            catppuccin::TEXT
        } else {
            catppuccin::SUBTEXT0
        };

        tree.push(RenderCommand::Text {
            x: cx + 4.0,
            y: label_y,
            text: item.label.clone(),
            color: label_color,
            font_size: self.config.label_font_size,
            font_weight: FontWeightHint::Regular,
            max_width: Some(cw - 8.0),
        });
    }
}

impl Default for GridView {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Unit tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_items(n: usize) -> Vec<GridItem> {
        (0..n)
            .map(|i| GridItem {
                id: i as u64,
                label: format!("Item {i}"),
                icon_id: None,
                badge: None,
                user_data: 0,
            })
            .collect()
    }

    // -------------------------------------------------------------------------
    // Layout calculation tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_layout_columns_fixed_sizing() {
        // 400px container, 12px padding each side = 376px usable.
        // 100px cells + 8px gap: floor((376 + 8) / (100 + 8)) = floor(384/108) = 3 columns.
        let config = GridConfig {
            cell_sizing: CellSizing::Fixed { width: 100.0, height: 120.0 },
            gap_x: 8.0,
            gap_y: 8.0,
            padding: 12.0,
            ..GridConfig::default()
        };
        let layout = LayoutCache::compute(&config, 400.0, 600.0, 10);
        assert_eq!(layout.columns, 3);
    }

    #[test]
    fn test_layout_total_rows() {
        let config = GridConfig::default();
        // With 3 columns and 10 items: ceil(10/3) = 4 rows.
        let layout = LayoutCache::compute(&config, 400.0, 600.0, 10);
        let cols = layout.columns;
        let expected_rows = (10 + cols - 1) / cols;
        assert_eq!(layout.total_rows, expected_rows);
    }

    #[test]
    fn test_layout_zero_items() {
        let config = GridConfig::default();
        let layout = LayoutCache::compute(&config, 400.0, 600.0, 0);
        assert_eq!(layout.total_rows, 0);
        assert_eq!(layout.content_height, 0.0);
    }

    #[test]
    fn test_layout_single_item() {
        let config = GridConfig {
            cell_sizing: CellSizing::Fixed { width: 100.0, height: 120.0 },
            padding: 10.0,
            ..GridConfig::default()
        };
        let layout = LayoutCache::compute(&config, 400.0, 600.0, 1);
        assert_eq!(layout.total_rows, 1);
        // content_height = padding*2 + 1 row * cell_height + 0 gaps = 20 + 120 = 140.
        assert!((layout.content_height - 140.0).abs() < 0.01);
    }

    #[test]
    fn test_layout_auto_sizing() {
        let config = GridConfig {
            cell_sizing: CellSizing::Auto { min_width: 80.0, aspect_ratio: 1.2 },
            gap_x: 10.0,
            gap_y: 10.0,
            padding: 10.0,
            ..GridConfig::default()
        };
        // Container 400px, padding 10*2=20, usable=380.
        // cols = floor((380+10)/(80+10)) = floor(390/90) = 4 columns.
        // cell_width = (380 - 10*3) / 4 = (380-30)/4 = 87.5.
        let layout = LayoutCache::compute(&config, 400.0, 600.0, 12);
        assert_eq!(layout.columns, 4);
        assert!((layout.cell_width - 87.5).abs() < 0.01);
        assert!((layout.cell_height - 87.5 * 1.2).abs() < 0.01);
        assert_eq!(layout.total_rows, 3); // 12 items / 4 cols = 3 rows.
    }

    #[test]
    fn test_visible_rows() {
        let config = GridConfig {
            cell_sizing: CellSizing::Fixed { width: 100.0, height: 100.0 },
            gap_x: 0.0,
            gap_y: 10.0,
            padding: 0.0,
            ..GridConfig::default()
        };
        // 100 items, columns depend on width.
        let layout = LayoutCache::compute(&config, 500.0, 250.0, 100);
        // row_height = 100 + 10 = 110. container_height=250.
        // At scroll_y=0: first_visible=0, visible_count = ceil(250/110)+2 = 3+2 = 5.
        let (first, last) = layout.visible_rows(0.0, &config);
        assert_eq!(first, 0);
        assert!(last >= 3); // At least 3 rows visible with some buffer.

        // At scroll_y=220 (scrolled past 2 rows):
        // first_visible = floor(220/110) = 2
        let (first2, _last2) = layout.visible_rows(220.0, &config);
        assert_eq!(first2, 2);
    }

    #[test]
    fn test_hit_test_basic() {
        let config = GridConfig {
            cell_sizing: CellSizing::Fixed { width: 100.0, height: 100.0 },
            gap_x: 10.0,
            gap_y: 10.0,
            padding: 5.0,
            ..GridConfig::default()
        };
        let layout = LayoutCache::compute(&config, 500.0, 500.0, 20);

        // Click in the first cell (at padding offset).
        let hit = layout.hit_test(10.0, 10.0, 5.0, 10.0, 10.0, 20);
        assert_eq!(hit, Some(0));

        // Click in the second cell (col=1).
        // x = 5 + 1*(100+10) + something = 115 + offset.
        let hit = layout.hit_test(120.0, 10.0, 5.0, 10.0, 10.0, 20);
        assert_eq!(hit, Some(1));

        // Click in the gap between cells.
        let hit = layout.hit_test(108.0, 10.0, 5.0, 10.0, 10.0, 20);
        assert_eq!(hit, None);
    }

    #[test]
    fn test_hit_test_out_of_bounds() {
        let config = GridConfig {
            cell_sizing: CellSizing::Fixed { width: 100.0, height: 100.0 },
            gap_x: 10.0,
            gap_y: 10.0,
            padding: 5.0,
            ..GridConfig::default()
        };
        let layout = LayoutCache::compute(&config, 500.0, 500.0, 5);

        // Click way below all items.
        let hit = layout.hit_test(10.0, 9000.0, 5.0, 10.0, 10.0, 5);
        assert_eq!(hit, None);
    }

    // -------------------------------------------------------------------------
    // Selection tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_selection_single() {
        let mut sel = SelectionState::new();
        sel.select_single(3);
        assert!(sel.is_selected(3));
        assert!(!sel.is_selected(0));
        assert_eq!(sel.count(), 1);
        assert_eq!(sel.focused(), Some(3));
    }

    #[test]
    fn test_selection_toggle() {
        let mut sel = SelectionState::new();
        sel.select_single(1);
        sel.toggle(3);
        assert!(sel.is_selected(1));
        assert!(sel.is_selected(3));
        assert_eq!(sel.count(), 2);

        sel.toggle(1);
        assert!(!sel.is_selected(1));
        assert!(sel.is_selected(3));
        assert_eq!(sel.count(), 1);
    }

    #[test]
    fn test_selection_range() {
        let mut sel = SelectionState::new();
        sel.select_single(2); // Sets anchor to 2.
        sel.select_range(5);
        assert_eq!(sel.count(), 4);
        for i in 2..=5 {
            assert!(sel.is_selected(i));
        }
        assert!(!sel.is_selected(0));
        assert!(!sel.is_selected(6));
    }

    #[test]
    fn test_selection_range_reverse() {
        let mut sel = SelectionState::new();
        sel.select_single(5); // Anchor at 5.
        sel.select_range(2); // Range from 5 down to 2.
        assert_eq!(sel.count(), 4);
        for i in 2..=5 {
            assert!(sel.is_selected(i));
        }
    }

    #[test]
    fn test_selection_select_all() {
        let mut sel = SelectionState::new();
        sel.select_all(10);
        assert_eq!(sel.count(), 10);
        for i in 0..10 {
            assert!(sel.is_selected(i));
        }
    }

    #[test]
    fn test_selection_clear() {
        let mut sel = SelectionState::new();
        sel.select_all(5);
        sel.clear();
        assert_eq!(sel.count(), 0);
        assert_eq!(sel.focused(), None);
    }

    #[test]
    fn test_selection_set_selection() {
        let mut sel = SelectionState::new();
        sel.set_selection(&[1, 3, 5, 7]);
        assert_eq!(sel.count(), 4);
        assert!(sel.is_selected(1));
        assert!(sel.is_selected(3));
        assert!(!sel.is_selected(2));
    }

    // -------------------------------------------------------------------------
    // Keyboard navigation tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_navigate_right() {
        let mut grid = GridView::new();
        grid.set_items(make_items(20));
        grid.set_container_size(400.0, 600.0);

        // Start at item 0, navigate right.
        grid.selection.select_single(0);
        let key = KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        grid.handle_key(&key);
        assert_eq!(grid.selection.focused(), Some(1));
    }

    #[test]
    fn test_navigate_down() {
        let mut grid = GridView::new();
        grid.set_items(make_items(20));
        grid.set_container_size(400.0, 600.0);
        grid.ensure_layout();

        let cols = grid.columns();
        grid.selection.select_single(0);

        let key = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        grid.handle_key(&key);
        // Should move down one row (by `columns` items).
        assert_eq!(grid.selection.focused(), Some(cols));
    }

    #[test]
    fn test_navigate_home_end() {
        let mut grid = GridView::new();
        grid.set_items(make_items(20));
        grid.set_container_size(400.0, 600.0);

        grid.selection.select_single(10);

        let home = KeyEvent {
            key: Key::Home,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        grid.handle_key(&home);
        assert_eq!(grid.selection.focused(), Some(0));

        let end = KeyEvent {
            key: Key::End,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        grid.handle_key(&end);
        assert_eq!(grid.selection.focused(), Some(19));
    }

    #[test]
    fn test_navigate_with_shift_extends_selection() {
        let mut grid = GridView::new();
        grid.set_items(make_items(20));
        grid.set_container_size(400.0, 600.0);

        // With default config and 400px width, we get 3 columns.
        // Start at item 0 (col=0, row=0); shift+right should select 0..=1.
        grid.selection.select_single(0);

        let key = KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::shift(),
            text: None,
        };
        grid.handle_key(&key);
        // Anchor stays at 0, range extends to 1.
        assert!(grid.selection.is_selected(0));
        assert!(grid.selection.is_selected(1));
        assert_eq!(grid.selection.count(), 2);
    }

    #[test]
    fn test_ctrl_a_selects_all() {
        let mut grid = GridView::new();
        grid.set_items(make_items(15));
        grid.set_container_size(400.0, 600.0);

        let key = KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        };
        let result = grid.handle_key(&key);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(grid.selection.count(), 15);
    }

    // -------------------------------------------------------------------------
    // Scroll tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_scroll_clamps_to_bounds() {
        let mut grid = GridView::new();
        grid.set_items(make_items(5));
        grid.set_container_size(400.0, 600.0);

        // Try to scroll negative.
        grid.set_scroll_y(-100.0);
        assert_eq!(grid.scroll_y(), 0.0);

        // Try to scroll past content.
        grid.set_scroll_y(99999.0);
        let max = (grid.content_height() - 600.0).max(0.0);
        assert!((grid.scroll_y() - max).abs() < 0.01);
    }

    #[test]
    fn test_scroll_to_item_scrolls_down() {
        let mut grid = GridView::with_config(GridConfig {
            cell_sizing: CellSizing::Fixed { width: 100.0, height: 100.0 },
            gap_x: 0.0,
            gap_y: 0.0,
            padding: 0.0,
            smooth_scroll: false,
            ..GridConfig::default()
        });
        grid.set_items(make_items(100));
        grid.set_container_size(300.0, 200.0); // 3 cols, viewport shows 2 rows.

        // Item 20 is in row 6 (index 20 / 3 cols = row 6).
        // Row 6 top = 6 * 100 = 600, bottom = 700.
        // Viewport at scroll 0 shows 0-200. Need to scroll to make row 6 visible.
        grid.scroll_to_item(20);
        // After scrolling, the item's bottom should be within the viewport.
        assert!(grid.scroll_y() > 0.0);
    }

    #[test]
    fn test_page_navigation_moves_by_visible_rows() {
        let mut grid = GridView::with_config(GridConfig {
            cell_sizing: CellSizing::Fixed { width: 100.0, height: 100.0 },
            gap_x: 0.0,
            gap_y: 10.0,
            padding: 0.0,
            smooth_scroll: false,
            ..GridConfig::default()
        });
        grid.set_items(make_items(100));
        grid.set_container_size(300.0, 330.0); // 3 cols, ~3 rows visible (330/(100+10)=3).

        grid.selection.select_single(0);
        let result = grid.navigate_page(1);
        assert_eq!(result, EventResult::Consumed);

        // Should have moved down by ~3 rows.
        let focused = grid.selection.focused().unwrap_or(0);
        // 3 rows * 3 cols = index 9 (from row 0 to row 3).
        assert_eq!(focused, 9);
    }

    // -------------------------------------------------------------------------
    // Type-ahead search tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_type_ahead_finds_item() {
        let mut grid = GridView::new();
        let mut items = make_items(10);
        items[5].label = "Documents".to_string();
        items[3].label = "Downloads".to_string();
        grid.set_items(items);
        grid.set_container_size(400.0, 600.0);

        // Type 'D' — should find "Documents" (index 5? actually "Downloads" is at 3, first match).
        // Items are: Item 0..2, Downloads(3), Item 4, Documents(5), Item 6..9.
        // First match for "d" is "Downloads" at index 3.
        let key = KeyEvent {
            key: Key::D,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('d'),
        };
        grid.handle_key(&key);
        assert_eq!(grid.selection.focused(), Some(3));

        // Type 'o' to refine to "do" — still "Downloads" at 3.
        grid.current_time_ms = 100; // Within timeout.
        let key2 = KeyEvent {
            key: Key::O,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('o'),
        };
        grid.handle_key(&key2);
        assert_eq!(grid.selection.focused(), Some(3));

        // Type 'c' to refine to "doc" — now "Documents" at 5.
        grid.current_time_ms = 200;
        let key3 = KeyEvent {
            key: Key::C,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('c'),
        };
        grid.handle_key(&key3);
        assert_eq!(grid.selection.focused(), Some(5));
    }

    #[test]
    fn test_type_ahead_timeout_resets() {
        let mut grid = GridView::new();
        let mut items = make_items(5);
        items[1].label = "Alpha".to_string();
        items[2].label = "Beta".to_string();
        grid.set_items(items);
        grid.set_container_size(400.0, 600.0);

        // Type 'a' — matches "Alpha" at 1.
        grid.current_time_ms = 0;
        let key = KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('a'),
        };
        grid.handle_key(&key);
        assert_eq!(grid.selection.focused(), Some(1));

        // Wait past timeout, then type 'b' — should match "Beta" at 2, not "ab".
        grid.current_time_ms = 2000; // Well past 1000ms timeout.
        let key2 = KeyEvent {
            key: Key::B,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('b'),
        };
        grid.handle_key(&key2);
        assert_eq!(grid.selection.focused(), Some(2));
    }

    // -------------------------------------------------------------------------
    // Rubber-band item intersection
    // -------------------------------------------------------------------------

    #[test]
    fn test_items_in_rect() {
        let config = GridConfig {
            cell_sizing: CellSizing::Fixed { width: 50.0, height: 50.0 },
            gap_x: 10.0,
            gap_y: 10.0,
            padding: 0.0,
            ..GridConfig::default()
        };
        // 300px wide, 0 padding: cols = floor((300+10)/(50+10)) = floor(310/60) = 5.
        let layout = LayoutCache::compute(&config, 300.0, 300.0, 20);
        assert_eq!(layout.columns, 5);

        // Rectangle covering first two columns, first two rows.
        // Cells: (0,0)=0..50, (1,0)=60..110 in x. (0,0)=0..50, (0,1)=60..110 in y.
        // Rectangle from (0,0) to (110, 110) should cover items 0,1,5,6.
        let items = layout.items_in_rect(0.0, 0.0, 110.0, 110.0, 0.0, 10.0, 10.0, 20);
        assert!(items.contains(&0));
        assert!(items.contains(&1));
        assert!(items.contains(&5));
        assert!(items.contains(&6));
        assert!(!items.contains(&2)); // col 2 starts at 120, outside 110.
    }

    // -------------------------------------------------------------------------
    // GridView integration tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_grid_view_double_click_emits_activate() {
        let mut grid = GridView::new();
        grid.set_items(make_items(10));
        grid.set_container_size(400.0, 600.0);
        grid.config.smooth_scroll = false;

        // Double-click on first item (which should be near top-left + padding).
        let mouse = MouseEvent {
            x: grid.config.padding + 10.0,
            y: grid.config.padding + 10.0,
            kind: MouseEventKind::DoubleClick(MouseButton::Left),
        };
        let result = grid.handle_mouse(&mouse);
        assert_eq!(result, EventResult::Consumed);

        let events = grid.drain_events();
        assert!(events.iter().any(|e| matches!(e, GridEvent::Activate(0))));
    }

    #[test]
    fn test_grid_view_right_click_emits_context_menu() {
        let mut grid = GridView::new();
        grid.set_items(make_items(10));
        grid.set_container_size(400.0, 600.0);

        let mouse = MouseEvent {
            x: grid.config.padding + 10.0,
            y: grid.config.padding + 10.0,
            kind: MouseEventKind::Press(MouseButton::Right),
        };
        let result = grid.handle_mouse(&mouse);
        assert_eq!(result, EventResult::Consumed);

        let events = grid.drain_events();
        assert!(events.iter().any(|e| matches!(e, GridEvent::ContextMenu { index: Some(0), .. })));
    }

    #[test]
    fn test_grid_view_empty_handles_events_gracefully() {
        let mut grid = GridView::new();
        grid.set_container_size(400.0, 600.0);

        // Keyboard nav on empty grid should not panic.
        let key = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let result = grid.handle_key(&key);
        assert_eq!(result, EventResult::Ignored);
    }
}

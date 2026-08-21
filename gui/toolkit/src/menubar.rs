#![allow(dead_code)]
//! Application menu bar widget (File | Edit | View | Help).
//!
//! Renders a horizontal bar of top-level labels. Clicking a label opens a
//! dropdown menu; moving between labels while open performs hot-tracking.
//! Supports action items, separators, submenus, check items, keyboard
//! mnemonics (`&File` underlines **F**), and keyboard accelerator display.
//!
//! Uses the Catppuccin Mocha dark theme, consistent with `menu.rs`.

use crate::color::Color;
use crate::event::{EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::row_strip::RowStrip;
use crate::step;
use crate::style::CornerRadii;

// ─── Re-export the shared item-id type from the context-menu module ────────

pub use crate::menu::MenuItemId;

// ─── Catppuccin Mocha palette ──────────────────────────────────────────────

const BAR_BG: Color = Color::from_hex(0x1E1E2E);
const BAR_ACTIVE_BG: Color = Color::from_hex(0x313244);
const DROPDOWN_BG: Color = Color::from_hex(0x1E1E2E);
const HOVER_COLOR: Color = Color::from_hex(0x313244);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const DIM_TEXT_COLOR: Color = Color::from_hex(0x6C7086);
const ACCENT_COLOR: Color = Color::from_hex(0x89B4FA);
const SEPARATOR_COLOR: Color = Color::from_hex(0x45475A);
const BORDER_COLOR: Color = Color::from_hex(0x45475A);
const SHADOW_COLOR: Color = Color::rgba(0, 0, 0, 160);
const MNEMONIC_UNDERLINE: Color = Color::from_hex(0xCDD6F4);

// ─── Layout constants ──────────────────────────────────────────────────────

/// Height of the top menu bar.
const BAR_HEIGHT: f32 = 28.0;
/// Horizontal padding inside each top-level label.
const LABEL_HPAD: f32 = 12.0;
/// Height of a single dropdown item row.
const ITEM_HEIGHT: f32 = 28.0;
/// Height of a separator in the dropdown.
const SEPARATOR_HEIGHT: f32 = 9.0;
/// Width reserved for the icon/check column on the left of dropdown items.
const ICON_COL_WIDTH: f32 = 28.0;
/// Extra padding between label text and shortcut text.
const SHORTCUT_GAP: f32 = 40.0;
/// Horizontal padding inside the dropdown panel.
const DROPDOWN_HPAD: f32 = 8.0;
/// Vertical padding at top/bottom of the dropdown panel.
const DROPDOWN_VPAD: f32 = 4.0;
/// Font size for all menu text.
const FONT_SIZE: f32 = 13.0;
/// Corner radius for dropdown panels.
const CORNER_RADIUS: f32 = 6.0;
/// Corner radius for hover highlight rectangles inside the dropdown.
const ITEM_HOVER_RADIUS: f32 = 4.0;
/// Shadow blur radius for dropdown panels.
const SHADOW_BLUR: f32 = 12.0;
/// Shadow offset for dropdown panels.
const SHADOW_OFFSET: f32 = 4.0;
/// Width reserved for the submenu arrow indicator.
const SUBMENU_ARROW_WIDTH: f32 = 20.0;
/// Minimum dropdown panel width.
const MIN_DROPDOWN_WIDTH: f32 = 160.0;
/// Screen height a dropdown is allowed to occupy. Matches `menu.rs`; a panel
/// taller than this scrolls instead of being drawn off the bottom edge.
const DEFAULT_VIEWPORT_HEIGHT: f32 = 1080.0;
/// Screen width a dropdown is allowed to occupy, the companion to
/// [`DEFAULT_VIEWPORT_HEIGHT`]. A submenu that would hang off the right edge
/// flips to the left of its parent instead — see
/// [`DropdownPanel::child_origin`].
const DEFAULT_VIEWPORT_WIDTH: f32 = 1920.0;
/// Width of the scroll indicator down the right edge of a capped panel.
const SCROLLBAR_WIDTH: f32 = 4.0;
/// Gap between that indicator and the panel's border.
const SCROLLBAR_INSET: f32 = 2.0;
/// Shortest the thumb may get, so a two-hundred-row menu still shows one.
const SCROLLBAR_MIN_THUMB: f32 = 16.0;
/// Track behind the scroll thumb.
const SCROLLBAR_TRACK_COLOR: Color = Color::from_hex(0x313244);
/// The thumb itself.
const SCROLLBAR_THUMB_COLOR: Color = Color::from_hex(0x585B70);

/// Underline thickness drawn beneath mnemonic characters.
const MNEMONIC_UNDERLINE_THICKNESS: f32 = 1.0;
/// Vertical offset of the mnemonic underline below the text baseline.
const MNEMONIC_UNDERLINE_OFFSET: f32 = 2.0;

// ─── Public types ──────────────────────────────────────────────────────────

/// An event emitted by the menu bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuBarEvent {
    /// An action item was activated.
    ItemClicked(MenuItemId),
    /// A check item was toggled (new value).
    CheckToggled(MenuItemId, bool),
}

/// A single top-level menu in the bar (e.g. "File").
#[derive(Clone, Debug)]
pub struct MenuBarItem {
    /// Display text. Prefix a letter with `&` to mark it as the mnemonic
    /// (e.g. `"&File"` underlines **F** and binds Alt+F).
    pub label: String,
    /// Dropdown entries shown when this top-level item is open.
    pub children: Vec<MenuBarEntry>,
}

/// One row inside a dropdown menu.
#[derive(Clone, Debug)]
pub enum MenuBarEntry {
    /// A clickable action item.
    Action {
        label: String,
        shortcut: Option<String>,
        enabled: bool,
        id: MenuItemId,
    },
    /// A toggle item with a checkmark indicator.
    Check {
        label: String,
        checked: bool,
        id: MenuItemId,
    },
    /// A horizontal separator line.
    Separator,
    /// A nested submenu that opens to the right.
    SubMenu {
        label: String,
        children: Vec<MenuBarEntry>,
    },
}

// ─── Internal helpers for mnemonic parsing ─────────────────────────────────

/// Parsed label: the display string (without `&`) and the index of the
/// mnemonic character (if any).
#[derive(Clone, Debug)]
struct ParsedLabel {
    text: String,
    mnemonic_index: Option<usize>,
}

fn parse_mnemonic(raw: &str) -> ParsedLabel {
    let mut text = String::with_capacity(raw.len());
    let mut mnemonic_index: Option<usize> = None;
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            if let Some(next) = chars.next() {
                if next == '&' {
                    text.push('&');
                } else {
                    if mnemonic_index.is_none() {
                        mnemonic_index = Some(text.len());
                    }
                    text.push(next);
                }
            }
        } else {
            text.push(ch);
        }
    }
    ParsedLabel {
        text,
        mnemonic_index,
    }
}

/// Extract the mnemonic character (lowercased) from a raw label, if any.
fn mnemonic_char(raw: &str) -> Option<char> {
    let parsed = parse_mnemonic(raw);
    parsed
        .mnemonic_index
        .and_then(|i| parsed.text.chars().nth(i))
        .map(|c| c.to_ascii_lowercase())
}

/// Map a `Key` variant to its lowercase ASCII character, if applicable.
fn key_to_lower_char(key: &Key) -> Option<char> {
    match key {
        Key::A => Some('a'),
        Key::B => Some('b'),
        Key::C => Some('c'),
        Key::D => Some('d'),
        Key::E => Some('e'),
        Key::F => Some('f'),
        Key::G => Some('g'),
        Key::H => Some('h'),
        Key::I => Some('i'),
        Key::J => Some('j'),
        Key::K => Some('k'),
        Key::L => Some('l'),
        Key::M => Some('m'),
        Key::N => Some('n'),
        Key::O => Some('o'),
        Key::P => Some('p'),
        Key::Q => Some('q'),
        Key::R => Some('r'),
        Key::S => Some('s'),
        Key::T => Some('t'),
        Key::U => Some('u'),
        Key::V => Some('v'),
        Key::W => Some('w'),
        Key::X => Some('x'),
        Key::Y => Some('y'),
        Key::Z => Some('z'),
        _ => None,
    }
}

/// Width of `text`, as the compositor will actually draw it.
///
/// The menu bar places mnemonic underlines by measuring the text before the
/// underlined character, so this has to agree with the drawn glyphs exactly —
/// the old byte-count heuristic put the underline under the wrong letter as
/// soon as a label contained anything non-ASCII.
fn estimate_text_width(text: &str, font_size: f32) -> f32 {
    crate::text::width(text, font_size)
}

// ─── Where a dropdown panel is ─────────────────────────────────────────────

/// Where one dropdown or submenu panel is, and where its rows are inside it.
///
/// The single answer for the renderer, the click and the hover — of *both* the
/// primary dropdown and every panel in the submenu chain. Before this type the
/// same five-line description
///
/// ```text
/// let h = dropdown_content_height(entries) + DROPDOWN_VPAD * 2.0;
/// if mx >= x && mx < x + w && my >= y && my < y + h { … }
/// item_index_at_y(entries, my - y - DROPDOWN_VPAD)
/// ```
///
/// appeared once in `MenuBar::dropdown_rect`, once in
/// [`click_in_submenu_chain`], once in [`hover_in_submenu_chain`] and once in
/// [`MenuBar::render_submenu_chain`], with the renderer adding
/// `DROPDOWN_VPAD` back on in a fifth place. Four hit tests and a renderer
/// describing one rectangle from memory is four chances for one of them to be
/// wrong, and the symptom is a click landing on the row above the one under the
/// pointer.
///
/// It also has to be a value rather than a set of loose floats because the
/// panel now has *two* heights that are not the same number:
/// [`Self::content_height`] is how tall the rows are, and
/// [`Self::panel_height`] is how much of that is on screen. The old code had
/// only the first, which is why a dropdown taller than the display was drawn
/// running off the bottom of it with its last rows unreachable.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DropdownPanel {
    x: f32,
    y: f32,
    width: f32,
    /// Rows plus the padding above and below them — the panel's height if the
    /// screen were unbounded.
    content_height: f32,
    /// How much of that is actually on screen. Equal to `content_height` for
    /// every panel that fits, which is nearly all of them.
    panel_height: f32,
    /// How far the rows are scrolled up inside the panel. Always in
    /// `0..=max_scroll()`.
    scroll: f32,
}

impl DropdownPanel {
    /// Place a panel of `entries` whose preferred top-left is
    /// `(x, preferred_y)`, kept inside `min_y..DEFAULT_VIEWPORT_HEIGHT`.
    ///
    /// `min_y` is [`BAR_HEIGHT`] for every panel here: a dropdown that slid up
    /// over the menu bar would be drawn under labels that still take the click,
    /// because [`MenuBar::on_mouse_press`] tests the bar first. A panel too tall
    /// for the band gets the whole band and scrolls.
    fn place(
        x: f32,
        preferred_y: f32,
        min_y: f32,
        width: f32,
        entries: &[MenuBarEntry],
        scroll: f32,
    ) -> Self {
        let content_height = dropdown_content_height(entries) + DROPDOWN_VPAD * 2.0;
        let available = (DEFAULT_VIEWPORT_HEIGHT - min_y).max(0.0);
        let panel_height = content_height.min(available);
        // `min_y <= DEFAULT_VIEWPORT_HEIGHT - panel_height` holds because
        // `panel_height <= available`, so the clamp range is never inverted.
        let y = preferred_y.clamp(min_y, DEFAULT_VIEWPORT_HEIGHT - panel_height);
        let mut panel = Self {
            x,
            y,
            width,
            content_height,
            panel_height,
            scroll: 0.0,
        };
        panel.scroll = panel.clamped_scroll(scroll);
        panel
    }

    fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Where a child submenu `child_width` wide should be placed beside this
    /// panel.
    ///
    /// Normally at [`Self::right`], so the child sits against its parent. When
    /// that would push the child past [`DEFAULT_VIEWPORT_WIDTH`] it flips to
    /// the other side instead, ending flush with this panel's left edge — the
    /// same rule every desktop menu uses, and the reason a `File > Recent >`
    /// chain opened from a menu bar near the right of the screen does not walk
    /// off it one level at a time.
    ///
    /// The flipped position is floored at zero rather than allowed negative:
    /// a submenu wider than everything to its left has nowhere good to go, and
    /// being clipped on the right is at least reachable, whereas being drawn at
    /// a negative `x` is neither visible nor clickable. There is exactly one
    /// spelling of this rule because the three places that open a submenu —
    /// click, hover and the primary-level constructor — must agree; when they
    /// each said `panel.right()` they agreed only because none of them had a
    /// rule at all.
    fn child_origin(&self, child_width: f32) -> f32 {
        let at_right = self.right();
        if at_right + child_width > DEFAULT_VIEWPORT_WIDTH {
            (self.x - child_width).max(0.0)
        } else {
            at_right
        }
    }

    fn bottom(&self) -> f32 {
        self.y + self.panel_height
    }

    /// Whether the pointer is anywhere on the panel — border included, so a
    /// click on the padding is swallowed rather than closing the menu.
    fn contains(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx < self.right() && my >= self.y && my < self.bottom()
    }

    /// Top of the region the rows are drawn in and hit-tested against.
    fn viewport_top(&self) -> f32 {
        self.y + DROPDOWN_VPAD
    }

    /// One past its bottom. Never above [`Self::viewport_top`]: a zero-height
    /// row region is a panel with no room, and a negative-height clip is not a
    /// small clip.
    fn viewport_bottom(&self) -> f32 {
        (self.bottom() - DROPDOWN_VPAD).max(self.viewport_top())
    }

    fn viewport_height(&self) -> f32 {
        self.viewport_bottom() - self.viewport_top()
    }

    /// How much taller the rows are than the room they have. Zero when the
    /// panel fits.
    fn max_scroll(&self) -> f32 {
        (self.content_height - self.panel_height).max(0.0)
    }

    /// `value` brought into the range that shows content. A non-finite value is
    /// refused outright rather than clamped: `NaN` compares false against both
    /// ends of a `clamp`, and letting one reach the strip's origin would poison
    /// every row's position at once, leaving a panel that answers for nothing.
    fn clamped_scroll(&self, value: f32) -> f32 {
        if value.is_finite() {
            value.clamp(0.0, self.max_scroll())
        } else {
            self.scroll
        }
    }

    /// Where every row sits on screen. The **only** place [`Self::scroll`] is
    /// subtracted — a scroll offset is just a different origin, so the renderer
    /// and [`Self::index_at`] stay each other's inverse for free.
    fn strip(&self, entries: &[MenuBarEntry]) -> RowStrip {
        entry_strip(entries, self.viewport_top() - self.scroll)
    }

    /// Which entry the pointer is over, or `None` for a separator, for the
    /// panel's own padding, and for a row that is scrolled off the panel.
    ///
    /// The visible-region test is not redundant with the strip. Once the list
    /// scrolls it genuinely extends past both ends of the panel, so the strip
    /// alone would name a scrolled-away row for a pointer sitting in the
    /// panel's border. Without a scroll offset the two coincide, which is why
    /// one of them used to be enough.
    fn index_at(&self, entries: &[MenuBarEntry], my: f32) -> Option<usize> {
        if !(my >= self.viewport_top() && my < self.viewport_bottom()) {
            return None;
        }
        let idx = self.strip(entries).index_at(my)?;
        match entries.get(idx) {
            Some(MenuBarEntry::Separator) | None => None,
            Some(_) => Some(idx),
        }
    }

    /// Top of row `index` on screen, or `None` if there is no such row. This is
    /// where a submenu for that row hangs from.
    fn row_top(&self, entries: &[MenuBarEntry], index: usize) -> Option<f32> {
        self.strip(entries).top(index)
    }

    /// The offset that brings row `index` fully into view, moving as little as
    /// possible. Unchanged for a row already visible, or one that does not
    /// exist.
    fn scroll_showing(&self, entries: &[MenuBarEntry], index: usize) -> f32 {
        let strip = self.strip(entries);
        let (Some(top), Some(height)) = (strip.top(index), strip.height(index)) else {
            return self.scroll;
        };
        let (view_top, view_bottom) = (self.viewport_top(), self.viewport_bottom());
        if top < view_top {
            self.clamped_scroll(self.scroll - (view_top - top))
        } else if top + height > view_bottom {
            self.clamped_scroll(self.scroll + (top + height - view_bottom))
        } else {
            self.scroll
        }
    }
}

// ─── Open-submenu state (for nested dropdown submenus) ─────────────────────

/// Tracks an open submenu inside a dropdown.
#[derive(Debug)]
struct OpenSubmenu {
    /// Index within the parent's `children` that owns this submenu.
    parent_index: usize,
    /// Screen-space origin of the submenu panel.
    x: f32,
    y: f32,
    /// Computed width of the submenu panel.
    width: f32,
    /// Which item inside this submenu is hovered (if any).
    hover_index: Option<usize>,
    /// How far this panel's rows are scrolled. Its own, not the parent's: two
    /// panels are on screen at once and the wheel belongs to whichever one the
    /// pointer is over.
    scroll: f32,
    /// Recursively open child submenu.
    child: Option<Box<OpenSubmenu>>,
}

/// The panel an [`OpenSubmenu`] node occupies.
fn submenu_panel(entries: &[MenuBarEntry], sub: &OpenSubmenu) -> DropdownPanel {
    DropdownPanel::place(sub.x, sub.y, BAR_HEIGHT, sub.width, entries, sub.scroll)
}

// ─── Result type for submenu click/hover resolution (avoids borrow issues) ─

/// What happened when we probed a click inside the submenu chain.
enum SubmenuClickResult {
    /// Click was inside a submenu and an entry was activated.
    Activated(ActivatedEntry),
    /// Click was inside a submenu but on a sub-submenu item (need to open it).
    OpenChild {
        idx: usize,
        child_x: f32,
        child_y: f32,
        child_width: f32,
    },
    /// Click was inside a submenu but on a separator or non-actionable spot.
    ConsumedNoAction,
    /// Click was not inside any submenu.
    Miss,
}

/// What entry was activated.
enum ActivatedEntry {
    Action(MenuItemId),
    CheckToggle(MenuItemId, bool),
}

// ─── MenuBar ───────────────────────────────────────────────────────────────

/// Application menu bar widget.
///
/// Renders a horizontal strip of top-level labels at the top of a window.
/// Clicking a label opens its dropdown; moving the mouse to another label
/// while any dropdown is open switches dropdowns (hot-tracking).
pub struct MenuBar {
    /// Top-level menus.
    items: Vec<MenuBarItem>,
    /// Which top-level menu is currently open (`None` = bar is closed).
    open_index: Option<usize>,
    /// Hover highlight inside the currently open dropdown.
    dropdown_hover: Option<usize>,
    /// How far the primary dropdown's rows are scrolled. Reset every time a
    /// menu opens, so a menu always opens showing its own first row.
    dropdown_scroll: f32,
    /// Open nested submenu chain inside the current dropdown.
    open_submenu: Option<Box<OpenSubmenu>>,
    /// Pending events to be drained by the caller.
    events: Vec<MenuBarEvent>,
    /// Cached per-label metrics: `(x_offset, width, parsed_label)`.
    label_metrics: Vec<(f32, f32, ParsedLabel)>,
    /// Cached panel width of each top-level menu's dropdown.
    ///
    /// Widening a dropdown to fit its widest label means measuring every label
    /// in it, and measuring a label means shaping it through the font. That is
    /// affordable once per menu and not once per event: [`Self::dropdown_panel`]
    /// is consulted by the hit test, the renderer and every wheel notch, so a
    /// two-hundred-row dropdown was shaping two hundred labels several times per
    /// mouse move. The width depends only on the entries, which change only in
    /// [`Self::set_items`], so it is computed exactly where they do.
    dropdown_widths: Vec<f32>,
}

impl MenuBar {
    // ── Construction ────────────────────────────────────────────────────

    /// Create a new menu bar from the given top-level items.
    pub fn new(items: Vec<MenuBarItem>) -> Self {
        let label_metrics = Self::compute_label_metrics(&items);
        let dropdown_widths = Self::compute_dropdown_widths(&items);
        Self {
            items,
            open_index: None,
            dropdown_hover: None,
            dropdown_scroll: 0.0,
            open_submenu: None,
            events: Vec::new(),
            label_metrics,
            dropdown_widths,
        }
    }

    /// Replace the entire menu structure.
    pub fn set_items(&mut self, items: Vec<MenuBarItem>) {
        self.label_metrics = Self::compute_label_metrics(&items);
        self.dropdown_widths = Self::compute_dropdown_widths(&items);
        self.items = items;
        self.close();
    }

    fn compute_dropdown_widths(items: &[MenuBarItem]) -> Vec<f32> {
        items
            .iter()
            .map(|item| calculate_dropdown_width(&item.children))
            .collect()
    }

    // ── Queries ─────────────────────────────────────────────────────────

    /// Whether any dropdown is currently open.
    pub fn is_open(&self) -> bool {
        self.open_index.is_some()
    }

    /// Close any open dropdown.
    pub fn close(&mut self) {
        self.open_index = None;
        self.dropdown_hover = None;
        self.dropdown_scroll = 0.0;
        self.open_submenu = None;
    }

    /// Drain all pending events.
    pub fn drain_events(&mut self) -> Vec<MenuBarEvent> {
        core::mem::take(&mut self.events)
    }

    // ── Mouse handling ──────────────────────────────────────────────────

    /// Handle a mouse event. Coordinates are relative to the bar's origin
    /// (top-left of the window, typically `(0, 0)`).
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => self.on_mouse_press(event.x, event.y),
            MouseEventKind::Move => self.on_mouse_move(event.x, event.y),
            MouseEventKind::Scroll { dy, .. } => self.on_mouse_scroll(event.x, event.y, dy),
            _ => EventResult::Ignored,
        }
    }

    /// Scroll whichever open panel the pointer is over.
    ///
    /// The offset is a continuous pixel value rather than a row index, so this
    /// takes [`wheel::pixels`] and not an accumulator: a trackpad's fifth of a
    /// notch should move a fifth of a notch rather than being banked until it
    /// rounds to a whole row. Its sign already means "towards the end of the
    /// list", which is the direction the offset grows in.
    ///
    /// [`wheel::pixels`]: crate::wheel::pixels
    fn on_mouse_scroll(&mut self, mx: f32, my: f32, dy: f32) -> EventResult {
        let Some(top_idx) = self.open_index else {
            return EventResult::Ignored;
        };

        // The submenu chain is drawn on top of the dropdown, so it goes first.
        if let Some(mut sub) = self.open_submenu.take() {
            let took =
                scroll_in_submenu_chain(children_of(&self.items, top_idx), &mut sub, mx, my, dy);
            self.open_submenu = Some(sub);
            if took {
                return EventResult::Consumed;
            }
        }

        let panel = self.dropdown_panel(top_idx);
        if !panel.contains(mx, my) {
            return EventResult::Ignored;
        }
        // Consumed even with nothing to scroll: an open menu must not let the
        // wheel through to the document behind it.
        self.dropdown_scroll =
            panel.clamped_scroll(panel.scroll + crate::wheel::pixels(dy, ITEM_HEIGHT));
        EventResult::Consumed
    }

    fn on_mouse_press(&mut self, mx: f32, my: f32) -> EventResult {
        // --- Click on a top-level label? ---
        if (0.0..BAR_HEIGHT).contains(&my)
            && let Some(idx) = self.label_index_at_x(mx)
        {
            if self.open_index == Some(idx) {
                self.close();
            } else {
                self.open_menu(idx);
            }
            return EventResult::Consumed;
        }

        // --- Click inside an open dropdown? ---
        if let Some(top_idx) = self.open_index {
            // Try submenu chain first (take it out to avoid borrow conflict).
            if let Some(mut sub) = self.open_submenu.take() {
                let result =
                    click_in_submenu_chain(children_of(&self.items, top_idx), &mut sub, mx, my);
                match result {
                    SubmenuClickResult::Activated(act) => {
                        self.apply_activation(act);
                        return EventResult::Consumed;
                    }
                    SubmenuClickResult::ConsumedNoAction => {
                        self.open_submenu = Some(sub);
                        return EventResult::Consumed;
                    }
                    SubmenuClickResult::OpenChild {
                        idx,
                        child_x,
                        child_y,
                        child_width,
                    } => {
                        // Find the deepest submenu and attach the new child.
                        let deepest = deepest_submenu_mut(&mut sub);
                        deepest.child = Some(Box::new(OpenSubmenu {
                            parent_index: idx,
                            x: child_x,
                            y: child_y,
                            width: child_width,
                            hover_index: None,
                            scroll: 0.0,
                            child: None,
                        }));
                        self.open_submenu = Some(sub);
                        return EventResult::Consumed;
                    }
                    SubmenuClickResult::Miss => {
                        self.open_submenu = Some(sub);
                    }
                }
            }

            // Try the primary dropdown.
            let panel = self.dropdown_panel(top_idx);
            if panel.contains(mx, my) {
                let children = children_of(&self.items, top_idx);
                if let Some(item_idx) = panel.index_at(children, my) {
                    self.activate_entry(top_idx, item_idx);
                }
                return EventResult::Consumed;
            }
        }

        // --- Click outside everything — close. ---
        if self.is_open() {
            self.close();
            return EventResult::Consumed;
        }

        EventResult::Ignored
    }

    fn on_mouse_move(&mut self, mx: f32, my: f32) -> EventResult {
        // --- Hot-tracking across top-level labels. ---
        if (0.0..BAR_HEIGHT).contains(&my) {
            if self.is_open()
                && let Some(idx) = self.label_index_at_x(mx)
            {
                if self.open_index != Some(idx) {
                    self.open_menu(idx);
                }
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }

        // --- Hover inside open dropdown / submenus. ---
        if let Some(top_idx) = self.open_index {
            // Check submenu chain first (take to avoid borrow conflict).
            if let Some(mut sub) = self.open_submenu.take() {
                let in_sub =
                    hover_in_submenu_chain(children_of(&self.items, top_idx), &mut sub, mx, my);
                self.open_submenu = Some(sub);
                if in_sub {
                    self.dropdown_hover = None;
                    return EventResult::Consumed;
                }
            }

            let panel = self.dropdown_panel(top_idx);
            if panel.contains(mx, my) {
                let new_hover = panel.index_at(children_of(&self.items, top_idx), my);
                self.dropdown_hover = new_hover;

                // Open / close submenu on hover. Re-hovering the row whose
                // submenu is already showing must leave it alone; for every
                // other row `submenu_at` answers both questions at once, since
                // it is `None` for an entry that is not a submenu — which is
                // exactly when an open submenu should close.
                if let Some(hi) = new_hover
                    && self
                        .open_submenu
                        .as_ref()
                        .is_none_or(|s| s.parent_index != hi)
                {
                    self.open_submenu = self.submenu_at(top_idx, hi).map(Box::new);
                }
                return EventResult::Consumed;
            }
        }

        EventResult::Ignored
    }

    // ── Keyboard handling ───────────────────────────────────────────────

    /// Handle a keyboard event.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }

        // Alt+mnemonic opens the corresponding top-level menu.
        if event.modifiers.alt
            && !event.modifiers.ctrl
            && !event.modifiers.shift
            && let Some(ch) = key_to_lower_char(&event.key)
        {
            for (i, item) in self.items.iter().enumerate() {
                if mnemonic_char(&item.label) == Some(ch) {
                    self.open_menu(i);
                    return EventResult::Consumed;
                }
            }
        }

        if !self.is_open() {
            return EventResult::Ignored;
        }

        let top_idx = match self.open_index {
            Some(i) => i,
            None => return EventResult::Ignored,
        };

        match event.key {
            Key::Escape => {
                if self.open_submenu.is_some() {
                    self.open_submenu = None;
                } else {
                    self.close();
                }
                EventResult::Consumed
            }

            Key::Left => {
                if self.open_submenu.is_some() {
                    self.open_submenu = None;
                } else {
                    self.open_menu(step::wrapping_before(self.items.len(), top_idx));
                }
                EventResult::Consumed
            }

            Key::Right => {
                // If hover is on a submenu entry in the primary dropdown, open it.
                if self.open_submenu.is_none()
                    && let Some(hi) = self.dropdown_hover
                    && let Some(sub) = self.submenu_at(top_idx, hi)
                {
                    self.open_submenu = Some(Box::new(sub));
                    return EventResult::Consumed;
                }

                // Otherwise move to the next top-level menu.
                self.open_menu(step::wrapping_after(self.items.len(), top_idx));
                EventResult::Consumed
            }

            Key::Up => {
                if self.open_submenu.is_some() {
                    self.update_submenu_hover(top_idx, |entries, hover| {
                        next_selectable(entries, hover, -1)
                    });
                } else {
                    self.move_dropdown_hover(-1, top_idx);
                }
                EventResult::Consumed
            }

            Key::Down => {
                if self.open_submenu.is_some() {
                    self.update_submenu_hover(top_idx, |entries, hover| {
                        next_selectable(entries, hover, 1)
                    });
                } else {
                    self.move_dropdown_hover(1, top_idx);
                }
                EventResult::Consumed
            }

            Key::Enter => {
                if let Some(ref sub) = self.open_submenu {
                    let (deepest, entries) =
                        deepest_with_entries_ref(children_of(&self.items, top_idx), sub);
                    let act = deepest
                        .hover_index
                        .and_then(|hi| try_activate_entry(&entries, hi));
                    if let Some(act) = act {
                        self.apply_activation(act);
                    }
                } else if let Some(hi) = self.dropdown_hover {
                    self.activate_entry(top_idx, hi);
                }
                EventResult::Consumed
            }

            _ => {
                // Type-to-jump: letter key jumps to first matching item label.
                if let Some(ch) = key_to_lower_char(&event.key) {
                    if self.open_submenu.is_some() {
                        self.update_submenu_hover(top_idx, |entries, _| {
                            jump_to_letter(entries, ch)
                        });
                    } else {
                        let hover = jump_to_letter(children_of(&self.items, top_idx), ch);
                        self.set_dropdown_hover(hover, top_idx);
                    }
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
        }
    }

    // ── Rendering ───────────────────────────────────────────────────────

    /// Produce render commands for the menu bar (and any open dropdown).
    ///
    /// `bar_width` is the full width of the window (the bar stretches edge to
    /// edge).
    pub fn render(&self, bar_width: u32) -> Vec<RenderCommand> {
        let bar_w = bar_width as f32;
        let mut cmds = Vec::new();

        // --- Bar background ---
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: bar_w,
            height: BAR_HEIGHT,
            color: BAR_BG,
            corner_radii: CornerRadii::ZERO,
        });

        // --- Bottom border of bar ---
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: BAR_HEIGHT,
            x2: bar_w,
            y2: BAR_HEIGHT,
            color: BORDER_COLOR,
            width: 1.0,
        });

        // --- Top-level labels ---
        for (i, (x_off, w, parsed)) in self.label_metrics.iter().enumerate() {
            let is_open = self.open_index == Some(i);

            if is_open {
                cmds.push(RenderCommand::FillRect {
                    x: *x_off,
                    y: 0.0,
                    width: *w,
                    height: BAR_HEIGHT,
                    color: BAR_ACTIVE_BG,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            let text_y = (BAR_HEIGHT - FONT_SIZE) / 2.0;
            let text_x = *x_off + LABEL_HPAD;

            cmds.push(RenderCommand::Text {
                x: text_x,
                y: text_y,
                text: parsed.text.clone(),
                color: TEXT_COLOR,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Mnemonic underline.
            if let Some(mi) = parsed.mnemonic_index {
                let prefix = &parsed.text[..parsed
                    .text
                    .char_indices()
                    .nth(mi)
                    .map_or(parsed.text.len(), |(pos, _)| pos)];
                let prefix_w = estimate_text_width(prefix, FONT_SIZE);
                let char_w = estimate_text_width(
                    &parsed
                        .text
                        .chars()
                        .nth(mi)
                        .map_or(String::new(), |c| c.to_string()),
                    FONT_SIZE,
                );
                let ul_y = text_y + FONT_SIZE + MNEMONIC_UNDERLINE_OFFSET;
                cmds.push(RenderCommand::Line {
                    x1: text_x + prefix_w,
                    y1: ul_y,
                    x2: text_x + prefix_w + char_w,
                    y2: ul_y,
                    color: MNEMONIC_UNDERLINE,
                    width: MNEMONIC_UNDERLINE_THICKNESS,
                });
            }
        }

        // --- Open dropdown ---
        if let Some(top_idx) = self.open_index {
            self.render_dropdown(&mut cmds, top_idx);
        }

        cmds
    }

    // ─── Private: dropdown rendering ───────────────────────────────────

    fn render_dropdown(&self, cmds: &mut Vec<RenderCommand>, top_idx: usize) {
        let panel = self.dropdown_panel(top_idx);
        let children = children_of(&self.items, top_idx);

        render_panel(cmds, &panel);
        render_entries(cmds, children, &panel, self.dropdown_hover);

        // Submenu chain.
        if let Some(ref sub) = self.open_submenu {
            render_submenu_chain(cmds, children, sub);
        }
    }

    // ─── Private: geometry helpers ──────────────────────────────────────

    fn compute_label_metrics(items: &[MenuBarItem]) -> Vec<(f32, f32, ParsedLabel)> {
        let mut metrics = Vec::with_capacity(items.len());
        let mut x = 0.0_f32;
        for item in items {
            let parsed = parse_mnemonic(&item.label);
            let text_w = estimate_text_width(&parsed.text, FONT_SIZE);
            let slot_w = text_w + LABEL_HPAD * 2.0;
            metrics.push((x, slot_w, parsed));
            x += slot_w;
        }
        metrics
    }

    fn label_index_at_x(&self, mx: f32) -> Option<usize> {
        for (i, (x_off, w, _)) in self.label_metrics.iter().enumerate() {
            if mx >= *x_off && mx < *x_off + *w {
                return Some(i);
            }
        }
        None
    }

    /// Where the dropdown panel for top-level menu `idx` is, and where its rows
    /// are inside it. The one answer the renderer, the click and the hover all
    /// read.
    fn dropdown_panel(&self, idx: usize) -> DropdownPanel {
        let x = self.label_metrics.get(idx).map_or(0.0, |m| m.0);
        let children = children_of(&self.items, idx);
        let width = self
            .dropdown_widths
            .get(idx)
            .copied()
            .unwrap_or(MIN_DROPDOWN_WIDTH);
        DropdownPanel::place(
            x,
            BAR_HEIGHT,
            BAR_HEIGHT,
            width,
            children,
            self.dropdown_scroll,
        )
    }

    // ─── Private: state transitions ────────────────────────────────────

    fn open_menu(&mut self, idx: usize) {
        self.open_index = Some(idx);
        self.dropdown_hover = None;
        // Opening a menu shows the top of it. Hot-tracking from a menu that was
        // scrolled must not open the next one part-way down its own list.
        self.dropdown_scroll = 0.0;
        self.open_submenu = None;
    }

    /// Activate an entry in the primary dropdown.
    fn activate_entry(&mut self, top_idx: usize, item_idx: usize) {
        if let Some(act) = try_activate_entry(children_of(&self.items, top_idx), item_idx) {
            self.apply_activation(act);
            return;
        }
        // Not activatable: if it is a submenu, open it instead.
        if let Some(sub) = self.submenu_at(top_idx, item_idx) {
            self.open_submenu = Some(Box::new(sub));
        }
    }

    /// The submenu that entry `item_idx` of top-level menu `top_idx` would
    /// open, or `None` if that entry is not a submenu.
    ///
    /// This placement — panel to the right of the dropdown, top aligned with
    /// the parent row — was written out three times: once for a click, once for
    /// Right-arrow, once for hover. Three copies of a geometry calculation are
    /// three chances for it to drift, and the copies were what forced each site
    /// to reach back into `self.items` for the entry list a second time.
    ///
    /// Being the sole constructor of a primary-level [`OpenSubmenu`] also makes
    /// `parent_index` mean something it only happened to mean before: it always
    /// names an entry that *is* a submenu.
    fn submenu_at(&self, top_idx: usize, item_idx: usize) -> Option<OpenSubmenu> {
        let children = children_of(&self.items, top_idx);
        let MenuBarEntry::SubMenu {
            children: nested, ..
        } = children.get(item_idx)?
        else {
            return None;
        };
        let panel = self.dropdown_panel(top_idx);
        let width = calculate_dropdown_width(nested);
        Some(OpenSubmenu {
            parent_index: item_idx,
            x: panel.child_origin(width),
            // Hangs off where the row *is*, which is where the panel says it is
            // — including the scroll offset. Recomputing the row's place from
            // the entry list here is how a submenu ends up beside the wrong row
            // of a scrolled dropdown.
            y: panel
                .row_top(children, item_idx)
                .unwrap_or_else(|| panel.viewport_top()),
            width,
            hover_index: None,
            scroll: 0.0,
            child: None,
        })
    }

    fn apply_activation(&mut self, act: ActivatedEntry) {
        match act {
            ActivatedEntry::Action(id) => {
                self.events.push(MenuBarEvent::ItemClicked(id));
                self.close();
            }
            ActivatedEntry::CheckToggle(id, new_val) => {
                self.events.push(MenuBarEvent::CheckToggled(id, new_val));
                self.close();
            }
        }
    }

    /// Choose a new keyboard selection in the primary dropdown, bringing the
    /// chosen row onto the screen.
    ///
    /// Selecting a row is a claim that the user is looking at it, so a selection
    /// that lands past the panel's last visible row has to drag the panel with
    /// it. Without the scroll, arrowing down a menu taller than the screen walks
    /// the highlight off the bottom edge and the user watches an unmoving list
    /// with nothing selected in it.
    ///
    /// Every path that sets `dropdown_hover` from the *keyboard* goes through
    /// here — arrows and type-to-jump both — because a scroll wired into one of
    /// two assignments is a scroll missing from the other.
    ///
    /// The pointer paths deliberately do not: [`Self::on_mouse_move`] assigns
    /// `dropdown_hover` directly. The asymmetry is the point rather than an
    /// oversight. A row the keyboard picked may be anywhere in the list, so it
    /// has to be brought into view; a row a hit test found is on the screen
    /// already, by construction, and scrolling to "reveal" it would slide the
    /// list out from under the very pointer that chose it — one twitch of the
    /// mouse and the menu bolts. Hover follows the pointer; it never leads it.
    fn set_dropdown_hover(&mut self, hover: Option<usize>, top_idx: usize) {
        self.dropdown_hover = hover;
        if let Some(idx) = hover {
            let panel = self.dropdown_panel(top_idx);
            self.dropdown_scroll = panel.scroll_showing(children_of(&self.items, top_idx), idx);
        }
    }

    fn move_dropdown_hover(&mut self, dir: i32, top_idx: usize) {
        let hover = next_selectable(children_of(&self.items, top_idx), self.dropdown_hover, dir);
        self.set_dropdown_hover(hover, top_idx);
    }

    /// The submenu twin of [`Self::set_dropdown_hover`]: re-pick the deepest
    /// open submenu's selection with `pick`, then scroll that submenu so the
    /// selection is on the screen.
    ///
    /// `pick` rather than a direction because the two callers choose
    /// differently — one steps by one, the other jumps to a letter — while the
    /// three lines around the choice (find the deepest submenu, resolve its
    /// entries, scroll to what was chosen) are identical, and were previously
    /// written out once per caller.
    fn update_submenu_hover(
        &mut self,
        top_idx: usize,
        pick: impl FnOnce(&[MenuBarEntry], Option<usize>) -> Option<usize>,
    ) {
        let root_children = children_of(&self.items, top_idx);
        let Some(ref mut sub) = self.open_submenu else {
            return;
        };
        // Resolving the deepest node needs every level above it, so the walk
        // down and the resolve happen together — see [`deepest_with_entries`].
        let (deepest, entries) = deepest_with_entries(root_children, sub);
        deepest.hover_index = pick(&entries, deepest.hover_index);
        if let Some(idx) = deepest.hover_index {
            let panel = submenu_panel(&entries, deepest);
            deepest.scroll = panel.scroll_showing(&entries, idx);
        }
    }
}

// ─── Free-standing helpers (no &self borrows) ──────────────────────────────

/// The dropdown entries of top-level menu `idx`.
///
/// Every caller wants the entries; not one wants the [`MenuBarItem`] that holds
/// them. Handing a caller an index to re-index with is what made this file
/// write `self.items[top_idx]` nineteen separate times, each against a bound
/// proved in some earlier statement — `dropdown_rect` held both beliefs at
/// once, reading `label_metrics` through `.get()` and `items` through `[]` two
/// lines apart. An index with no menu behind it now reads as a menu with no
/// entries, which renders nothing and activates nothing.
///
/// It takes the item slice rather than `&self` so that it borrows exactly what
/// the indexing it replaces borrowed: four callers hold a `&mut` into
/// `open_submenu` across the call, and only a disjoint field borrow is allowed
/// to coexist with that.
fn children_of(items: &[MenuBarItem], idx: usize) -> &[MenuBarEntry] {
    items.get(idx).map_or(&[], |item| &item.children)
}

/// Try to activate an entry. Returns `None` for separators, disabled items,
/// and submenus (submenus need to be opened, not "activated").
fn try_activate_entry(entries: &[MenuBarEntry], idx: usize) -> Option<ActivatedEntry> {
    match entries.get(idx) {
        Some(MenuBarEntry::Action {
            id, enabled: true, ..
        }) => Some(ActivatedEntry::Action(*id)),
        Some(MenuBarEntry::Check { id, checked, .. }) => {
            Some(ActivatedEntry::CheckToggle(*id, !checked))
        }
        _ => None,
    }
}

/// Walk the submenu chain looking for a click hit. This is a free function
/// so we can pass the parent's entries separately from `&mut sub`.
///
/// `parent_entries` is the level directly above `sub` — the top-level menu's
/// children for the primary submenu, and each level's own resolved entries for
/// the one below it. See [`resolve_submenu_entries`].
fn click_in_submenu_chain(
    parent_entries: &[MenuBarEntry],
    sub: &mut OpenSubmenu,
    mx: f32,
    my: f32,
) -> SubmenuClickResult {
    // Resolved before descending, not after: the child's `parent_index` indexes
    // into *these* entries, so the recursion cannot be handed what we were.
    let entries = resolve_submenu_entries(parent_entries, sub);

    // Recurse into child first (deepest wins).
    if let Some(ref mut child) = sub.child {
        let r = click_in_submenu_chain(&entries, child, mx, my);
        match r {
            SubmenuClickResult::Miss => {} // Fall through to check this level.
            other => return other,
        }
    }

    let panel = submenu_panel(&entries, sub);

    if panel.contains(mx, my) {
        if let Some(idx) = panel.index_at(&entries, my) {
            // Try to activate.
            if let Some(act) = try_activate_entry(&entries, idx) {
                return SubmenuClickResult::Activated(act);
            }
            // If it's a submenu, signal to open it.
            if let Some(MenuBarEntry::SubMenu { children: sc, .. }) = entries.get(idx) {
                let child_width = calculate_dropdown_width(sc);
                return SubmenuClickResult::OpenChild {
                    idx,
                    child_x: panel.child_origin(child_width),
                    child_y: panel
                        .row_top(&entries, idx)
                        .unwrap_or_else(|| panel.viewport_top()),
                    child_width,
                };
            }
        }
        return SubmenuClickResult::ConsumedNoAction;
    }

    SubmenuClickResult::Miss
}

/// Hover inside submenu chain. Returns `true` if the point is inside.
///
/// `parent_entries` means what it does in [`click_in_submenu_chain`].
fn hover_in_submenu_chain(
    parent_entries: &[MenuBarEntry],
    sub: &mut OpenSubmenu,
    mx: f32,
    my: f32,
) -> bool {
    let entries = resolve_submenu_entries(parent_entries, sub);

    // Recurse into child first.
    if let Some(ref mut child) = sub.child
        && hover_in_submenu_chain(&entries, child, mx, my)
    {
        sub.hover_index = None;
        return true;
    }

    let panel = submenu_panel(&entries, sub);

    if panel.contains(mx, my) {
        let new_hover = panel.index_at(&entries, my);
        sub.hover_index = new_hover;

        // Open nested sub-submenu on hover.
        if let Some(hi) = new_hover {
            match entries.get(hi) {
                Some(MenuBarEntry::SubMenu { children: sc, .. }) => {
                    let already = sub.child.as_ref().is_some_and(|c| c.parent_index == hi);
                    if !already {
                        let width = calculate_dropdown_width(sc);
                        sub.child = Some(Box::new(OpenSubmenu {
                            parent_index: hi,
                            x: panel.child_origin(width),
                            y: panel
                                .row_top(&entries, hi)
                                .unwrap_or_else(|| panel.viewport_top()),
                            width,
                            hover_index: None,
                            scroll: 0.0,
                            child: None,
                        }));
                    }
                }
                _ => {
                    sub.child = None;
                }
            }
        }

        return true;
    }

    false
}

/// Send `dy` wheel notches to whichever panel of the chain the pointer is over.
/// Returns whether one took it.
fn scroll_in_submenu_chain(
    parent_entries: &[MenuBarEntry],
    sub: &mut OpenSubmenu,
    mx: f32,
    my: f32,
    dy: f32,
) -> bool {
    let entries = resolve_submenu_entries(parent_entries, sub);

    // Deepest panel is drawn on top, so it gets the wheel first.
    if let Some(ref mut child) = sub.child
        && scroll_in_submenu_chain(&entries, child, mx, my, dy)
    {
        return true;
    }

    let panel = submenu_panel(&entries, sub);
    if !panel.contains(mx, my) {
        return false;
    }
    // Consumed even with nothing to scroll: the wheel must not fall through a
    // popup to whatever it is covering.
    sub.scroll = panel.clamped_scroll(panel.scroll + crate::wheel::pixels(dy, ITEM_HEIGHT));
    true
}

/// Draw every panel of the submenu chain, parent first so children land on top.
///
/// A free function, and not a method, for the same reason the other three
/// walkers are: it needs the entries of the level above each node, and the only
/// way to have those is to resolve them on the way down. A method could reach
/// `self.items` at every level, which is exactly the mistake — it would resolve
/// each node against the root's children again. `parent_entries` means what it
/// does in [`click_in_submenu_chain`].
fn render_submenu_chain(
    cmds: &mut Vec<RenderCommand>,
    parent_entries: &[MenuBarEntry],
    sub: &OpenSubmenu,
) {
    let entries = resolve_submenu_entries(parent_entries, sub);
    let panel = submenu_panel(&entries, sub);

    render_panel(cmds, &panel);
    render_entries(cmds, &entries, &panel, sub.hover_index);

    if let Some(ref child) = sub.child {
        render_submenu_chain(cmds, &entries, child);
    }
}

/// The entries a submenu node shows, given the entries of the level directly
/// above it.
///
/// `parent_index` indexes into that level and nothing else: the primary
/// submenu's names an entry of the top-level menu's children, its child's
/// names an entry of the primary submenu's own entries, and so on down. So a
/// node can only be resolved by resolving every level above it in turn, which
/// is why each walker that descends the chain hands the recursion the entries
/// it just resolved instead of passing the root's children the whole way
/// down. Passing the root's children down is not a near-miss — at depth two it
/// reads `parent_index` against a list it does not describe, so a submenu
/// shows whichever unrelated entry happens to sit at that index in the *first*
/// level, or nothing at all when that entry is not itself a submenu.
///
/// An index that names no submenu resolves to no entries, which renders
/// nothing and activates nothing, rather than to a panic.
///
/// This clones the entry list because we cannot hold a borrow into the item
/// tree while also mutating submenu state.
fn resolve_submenu_entries(
    parent_entries: &[MenuBarEntry],
    sub: &OpenSubmenu,
) -> Vec<MenuBarEntry> {
    match parent_entries.get(sub.parent_index) {
        Some(MenuBarEntry::SubMenu { children, .. }) => children.clone(),
        _ => Vec::new(),
    }
}

/// The deepest open node of a submenu chain, together with the entries it
/// shows.
///
/// The keyboard has no pointer to follow down the chain the way the mouse
/// walkers do — it wants the bottom node and nothing else — but the bottom
/// node's entries are still only reachable by resolving every level above it,
/// so it cannot simply grab the node and resolve it against the root's
/// children. This walks and resolves together, which is the only way to get
/// both.
fn deepest_with_entries<'a>(
    parent_entries: &[MenuBarEntry],
    sub: &'a mut OpenSubmenu,
) -> (&'a mut OpenSubmenu, Vec<MenuBarEntry>) {
    let entries = resolve_submenu_entries(parent_entries, sub);
    // Phrased as `match` for the same reason as `deepest_submenu_mut` below:
    // the pre-polonius borrow checker rejects the `if let ... else` form.
    match sub.child {
        Some(ref mut child) => deepest_with_entries(&entries, child),
        None => (sub, entries),
    }
}

/// The immutable twin of [`deepest_with_entries`].
fn deepest_with_entries_ref<'a>(
    parent_entries: &[MenuBarEntry],
    sub: &'a OpenSubmenu,
) -> (&'a OpenSubmenu, Vec<MenuBarEntry>) {
    let entries = resolve_submenu_entries(parent_entries, sub);
    match sub.child {
        Some(ref child) => deepest_with_entries_ref(&entries, child),
        None => (sub, entries),
    }
}

/// Walk down to the deepest open submenu node (mutable).
///
/// Only for callers that want the *node* and not what it shows — attaching a
/// freshly-built child, say. A caller that needs the node's entries must use
/// [`deepest_with_entries`] instead, because entries cannot be resolved from
/// the node alone.
fn deepest_submenu_mut(sub: &mut OpenSubmenu) -> &mut OpenSubmenu {
    // NOTE: Phrased as `match` rather than `if let ... else` because the
    // current borrow checker (pre-polonius) doesn't reason about the disjoint
    // mutable borrow in the else arm. This form returns `sub` only in the
    // `None` arm, where no prior borrow of `sub.child` is live.
    match sub.child {
        Some(ref mut child) => deepest_submenu_mut(child),
        None => sub,
    }
}

/// How tall one dropdown row is. The single spelling of the rule.
///
/// This match used to appear four times in this file — once summing the
/// heights for the panel's total, once placing the rows in
/// [`render_entries`], once subtracting them back off in the hit test, and
/// once adding them up again to position a submenu. Four walks of one list is
/// four chances for three of them to be right; when they disagree the user
/// clicks one row and gets the one above.
const fn entry_height(entry: &MenuBarEntry) -> f32 {
    match entry {
        MenuBarEntry::Separator => SEPARATOR_HEIGHT,
        _ => ITEM_HEIGHT,
    }
}

/// Where every dropdown row sits, measured from `content_top`.
///
/// [`DropdownPanel::strip`] is the only caller that matters — it passes the
/// panel's content top less its scroll offset, and the renderer and every
/// pointer path read that one strip. [`dropdown_content_height`] passes `0.0`
/// because it wants a length rather than a position, and a length does not
/// depend on where the run starts.
fn entry_strip(entries: &[MenuBarEntry], content_top: f32) -> RowStrip {
    RowStrip::new(content_top, entries.iter().map(entry_height))
}

fn dropdown_content_height(entries: &[MenuBarEntry]) -> f32 {
    entry_strip(entries, 0.0).total_height()
}

fn calculate_dropdown_width(entries: &[MenuBarEntry]) -> f32 {
    let mut max_label: f32 = 0.0;
    let mut max_shortcut: f32 = 0.0;

    for entry in entries {
        match entry {
            MenuBarEntry::Action {
                label, shortcut, ..
            } => {
                max_label = max_label.max(estimate_text_width(label, FONT_SIZE));
                if let Some(sc) = shortcut {
                    max_shortcut = max_shortcut.max(estimate_text_width(sc, FONT_SIZE));
                }
            }
            MenuBarEntry::Check { label, .. } => {
                max_label = max_label.max(estimate_text_width(label, FONT_SIZE));
            }
            MenuBarEntry::SubMenu { label, .. } => {
                max_label = max_label.max(estimate_text_width(label, FONT_SIZE));
                max_shortcut = max_shortcut.max(SUBMENU_ARROW_WIDTH);
            }
            MenuBarEntry::Separator => {}
        }
    }

    let shortcut_space = if max_shortcut > 0.0 {
        SHORTCUT_GAP + max_shortcut
    } else {
        0.0
    };

    (DROPDOWN_HPAD * 2.0 + ICON_COL_WIDTH + max_label + shortcut_space + DROPDOWN_HPAD)
        .max(MIN_DROPDOWN_WIDTH)
}

/// Whether an entry can take the hover highlight.
///
/// Matched exhaustively rather than with a `_` arm: a new [`MenuBarEntry`]
/// variant should be a compile error here, not a row the keyboard silently
/// refuses to land on.
fn is_selectable(entry: &MenuBarEntry) -> bool {
    match entry {
        MenuBarEntry::Action { enabled, .. } => *enabled,
        MenuBarEntry::Check { .. } | MenuBarEntry::SubMenu { .. } => true,
        MenuBarEntry::Separator => false,
    }
}

/// Move hover in `direction` (+1 or -1), skipping separators and disabled items.
fn next_selectable(
    entries: &[MenuBarEntry],
    current: Option<usize>,
    direction: i32,
) -> Option<usize> {
    let len = entries.len();
    if len == 0 {
        return None;
    }
    let forward = direction >= 0;

    // Where the walk begins: one place beyond the current hover in the
    // direction of travel, or the near end of the list when nothing is hovered.
    let start = match current {
        Some(idx) if forward => step::wrapping_after(len, idx),
        Some(idx) => step::wrapping_before(len, idx),
        None if forward => 0,
        None => len.saturating_sub(1),
    };

    step::indices(len, start, forward).find(|idx| entries.get(*idx).is_some_and(is_selectable))
}

/// Jump to the first entry whose label starts with `ch`.
fn jump_to_letter(entries: &[MenuBarEntry], ch: char) -> Option<usize> {
    for (i, entry) in entries.iter().enumerate() {
        let label = match entry {
            MenuBarEntry::Action {
                label,
                enabled: true,
                ..
            } => Some(label.as_str()),
            MenuBarEntry::Check { label, .. } => Some(label.as_str()),
            MenuBarEntry::SubMenu { label, .. } => Some(label.as_str()),
            _ => None,
        };
        if let Some(l) = label
            && l.chars().next().map(|c| c.to_ascii_lowercase()) == Some(ch)
        {
            return Some(i);
        }
    }
    None
}

/// Render the shadow + background + border for a dropdown panel.
fn render_panel(cmds: &mut Vec<RenderCommand>, panel: &DropdownPanel) {
    let (x, y, w, h) = (panel.x, panel.y, panel.width, panel.panel_height);
    let radii = CornerRadii::all(CORNER_RADIUS);

    cmds.push(RenderCommand::BoxShadow {
        x,
        y,
        width: w,
        height: h,
        offset_x: SHADOW_OFFSET,
        offset_y: SHADOW_OFFSET,
        blur: SHADOW_BLUR,
        spread: 0.0,
        color: SHADOW_COLOR,
        corner_radii: radii,
    });

    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: DROPDOWN_BG,
        corner_radii: radii,
    });

    cmds.push(RenderCommand::StrokeRect {
        x,
        y,
        width: w,
        height: h,
        color: BORDER_COLOR,
        line_width: 1.0,
        corner_radii: radii,
    });
}

/// Render the item rows inside a dropdown or submenu panel.
fn render_entries(
    cmds: &mut Vec<RenderCommand>,
    entries: &[MenuBarEntry],
    panel: &DropdownPanel,
    hover: Option<usize>,
) {
    let (panel_x, panel_w) = (panel.x, panel.width);
    // Draw from the same strip the hit test reads. Advancing a running
    // `cur_y` here is what let this walk drift away from the three others
    // that used to exist.
    //
    // Clipped to the region `DropdownPanel::index_at` accepts, so a row cut off
    // by scrolling is drawn cut off at exactly the line past which it stops
    // answering. Rows entirely outside it are skipped rather than drawn under
    // the clip: the clip hides them either way, but a two-hundred-row menu
    // should not emit six hundred text commands to show thirty rows.
    let strip = panel.strip(entries);
    let (view_top, view_bottom) = (panel.viewport_top(), panel.viewport_bottom());
    cmds.push(RenderCommand::PushClip {
        x: panel_x,
        y: view_top,
        width: panel_w,
        height: panel.viewport_height(),
    });
    for (i, entry) in entries.iter().enumerate() {
        let Some(cur_y) = strip.top(i) else {
            continue;
        };
        let row_height = strip.height(i).unwrap_or(0.0);
        if cur_y + row_height <= view_top || cur_y >= view_bottom {
            continue;
        }
        match entry {
            MenuBarEntry::Separator => {
                let line_y = cur_y + SEPARATOR_HEIGHT / 2.0;
                cmds.push(RenderCommand::Line {
                    x1: panel_x + DROPDOWN_HPAD,
                    y1: line_y,
                    x2: panel_x + panel_w - DROPDOWN_HPAD,
                    y2: line_y,
                    color: SEPARATOR_COLOR,
                    width: 1.0,
                });
            }

            MenuBarEntry::Action {
                label,
                shortcut,
                enabled,
                ..
            } => {
                if hover == Some(i) && *enabled {
                    cmds.push(RenderCommand::FillRect {
                        x: panel_x + 4.0,
                        y: cur_y,
                        width: panel_w - 8.0,
                        height: ITEM_HEIGHT,
                        color: HOVER_COLOR,
                        corner_radii: CornerRadii::all(ITEM_HOVER_RADIUS),
                    });
                }

                let tc = if *enabled { TEXT_COLOR } else { DIM_TEXT_COLOR };
                let text_y = cur_y + (ITEM_HEIGHT - FONT_SIZE) / 2.0;

                cmds.push(RenderCommand::Text {
                    x: panel_x + DROPDOWN_HPAD + ICON_COL_WIDTH,
                    y: text_y,
                    text: label.clone(),
                    color: tc,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                if let Some(sc) = shortcut {
                    cmds.push(RenderCommand::Text {
                        x: panel_x + panel_w - DROPDOWN_HPAD - estimate_text_width(sc, FONT_SIZE),
                        y: text_y,
                        text: sc.clone(),
                        color: DIM_TEXT_COLOR,
                        font_size: FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }
            }

            MenuBarEntry::Check { label, checked, .. } => {
                if hover == Some(i) {
                    cmds.push(RenderCommand::FillRect {
                        x: panel_x + 4.0,
                        y: cur_y,
                        width: panel_w - 8.0,
                        height: ITEM_HEIGHT,
                        color: HOVER_COLOR,
                        corner_radii: CornerRadii::all(ITEM_HOVER_RADIUS),
                    });
                }

                let text_y = cur_y + (ITEM_HEIGHT - FONT_SIZE) / 2.0;

                if *checked {
                    cmds.push(RenderCommand::Text {
                        x: panel_x + DROPDOWN_HPAD + 4.0,
                        y: text_y,
                        text: "\u{2713}".to_string(),
                        color: ACCENT_COLOR,
                        font_size: FONT_SIZE,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }

                cmds.push(RenderCommand::Text {
                    x: panel_x + DROPDOWN_HPAD + ICON_COL_WIDTH,
                    y: text_y,
                    text: label.clone(),
                    color: TEXT_COLOR,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            MenuBarEntry::SubMenu { label, .. } => {
                if hover == Some(i) {
                    cmds.push(RenderCommand::FillRect {
                        x: panel_x + 4.0,
                        y: cur_y,
                        width: panel_w - 8.0,
                        height: ITEM_HEIGHT,
                        color: HOVER_COLOR,
                        corner_radii: CornerRadii::all(ITEM_HOVER_RADIUS),
                    });
                }

                let text_y = cur_y + (ITEM_HEIGHT - FONT_SIZE) / 2.0;

                cmds.push(RenderCommand::Text {
                    x: panel_x + DROPDOWN_HPAD + ICON_COL_WIDTH,
                    y: text_y,
                    text: label.clone(),
                    color: TEXT_COLOR,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Arrow indicator.
                cmds.push(RenderCommand::Text {
                    x: panel_x + panel_w - DROPDOWN_HPAD - SUBMENU_ARROW_WIDTH,
                    y: text_y,
                    text: "\u{25B8}".to_string(),
                    color: TEXT_COLOR,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }
    }
    cmds.push(RenderCommand::PopClip);
    render_scrollbar(cmds, panel);
}

/// The scroll indicator down a panel's right edge, drawn only when the panel is
/// showing less than it holds. Without it a capped panel looks exactly like a
/// panel that happens to end there.
fn render_scrollbar(cmds: &mut Vec<RenderCommand>, panel: &DropdownPanel) {
    let max_scroll = panel.max_scroll();
    let track_height = panel.viewport_height();
    if max_scroll <= 0.0 || track_height <= 0.0 {
        return;
    }
    let track_x = panel.right() - SCROLLBAR_INSET - SCROLLBAR_WIDTH;
    let radii = CornerRadii::all(SCROLLBAR_WIDTH / 2.0);
    cmds.push(RenderCommand::FillRect {
        x: track_x,
        y: panel.viewport_top(),
        width: SCROLLBAR_WIDTH,
        height: track_height,
        color: SCROLLBAR_TRACK_COLOR,
        corner_radii: radii,
    });
    // The thumb is as tall a fraction of the track as the panel is of the
    // content, floored so it stays visible, and then travels over whatever room
    // that leaves it. Dividing the travel by `max_scroll` rather than by the
    // content height is what keeps the thumb flush with the end of the track
    // when the list is scrolled to its end, at any thumb size.
    let visible_fraction = if panel.content_height > 0.0 {
        panel.panel_height / panel.content_height
    } else {
        1.0
    };
    let thumb_height = (track_height * visible_fraction)
        .max(SCROLLBAR_MIN_THUMB)
        .min(track_height);
    let thumb_y =
        panel.viewport_top() + (track_height - thumb_height) * (panel.scroll / max_scroll);
    cmds.push(RenderCommand::FillRect {
        x: track_x,
        y: thumb_y,
        width: SCROLLBAR_WIDTH,
        height: thumb_height,
        color: SCROLLBAR_THUMB_COLOR,
        corner_radii: radii,
    });
}

// ─── Tests ─────────────────────────────────────────────────────────────────

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

    // ── Test helpers ────────────────────────────────────────────────────

    fn make_bar() -> MenuBar {
        MenuBar::new(vec![
            MenuBarItem {
                label: "&File".to_string(),
                children: vec![
                    MenuBarEntry::Action {
                        label: "New".to_string(),
                        shortcut: Some("Ctrl+N".to_string()),
                        enabled: true,
                        id: 1,
                    },
                    MenuBarEntry::Action {
                        label: "Open".to_string(),
                        shortcut: Some("Ctrl+O".to_string()),
                        enabled: true,
                        id: 2,
                    },
                    MenuBarEntry::Separator,
                    MenuBarEntry::Action {
                        label: "Save".to_string(),
                        shortcut: Some("Ctrl+S".to_string()),
                        enabled: true,
                        id: 3,
                    },
                    MenuBarEntry::Action {
                        label: "Save As...".to_string(),
                        shortcut: None,
                        enabled: false,
                        id: 4,
                    },
                ],
            },
            MenuBarItem {
                label: "&Edit".to_string(),
                children: vec![
                    MenuBarEntry::Action {
                        label: "Undo".to_string(),
                        shortcut: Some("Ctrl+Z".to_string()),
                        enabled: true,
                        id: 10,
                    },
                    MenuBarEntry::Separator,
                    MenuBarEntry::Check {
                        label: "Word Wrap".to_string(),
                        checked: true,
                        id: 20,
                    },
                ],
            },
            MenuBarItem {
                label: "&View".to_string(),
                children: vec![
                    MenuBarEntry::SubMenu {
                        label: "Zoom".to_string(),
                        children: vec![
                            MenuBarEntry::Action {
                                label: "Zoom In".to_string(),
                                shortcut: Some("Ctrl++".to_string()),
                                enabled: true,
                                id: 30,
                            },
                            MenuBarEntry::Action {
                                label: "Zoom Out".to_string(),
                                shortcut: Some("Ctrl+-".to_string()),
                                enabled: true,
                                id: 31,
                            },
                        ],
                    },
                    MenuBarEntry::Action {
                        label: "Fullscreen".to_string(),
                        shortcut: Some("F11".to_string()),
                        enabled: true,
                        id: 32,
                    },
                ],
            },
        ])
    }

    fn press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    fn alt_press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::alt(),
            text: None,
        }
    }

    fn click(x: f32, y: f32) -> MouseEvent {
        MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }
    }

    fn mouse_move(x: f32, y: f32) -> MouseEvent {
        MouseEvent {
            x,
            y,
            kind: MouseEventKind::Move,
        }
    }

    // ── Dropdown geometry: does a click land on the row that was drawn? ──
    //
    // These read the rectangle `render()` actually emitted and probe *its*
    // edges. A test that recomputes the renderer's arithmetic and checks the
    // hit test against that is worthless: the two drift together, and the
    // whole failure mode being guarded here is a renderer and a hit test that
    // agree only by coincidence.

    /// A bar whose every dropdown row is enabled, so every row paints a hover
    /// highlight and can therefore be measured. Mixed heights on purpose.
    fn geometry_bar() -> MenuBar {
        fn action(id: u64, label: &str) -> MenuBarEntry {
            MenuBarEntry::Action {
                label: label.to_string(),
                shortcut: None,
                enabled: true,
                id,
            }
        }
        MenuBar::new(vec![MenuBarItem {
            label: "&File".to_string(),
            children: vec![
                action(1, "New"),
                action(2, "Open"),
                MenuBarEntry::Separator,
                MenuBarEntry::SubMenu {
                    label: "Recent".to_string(),
                    children: vec![action(31, "a.txt")],
                },
                MenuBarEntry::Check {
                    label: "Read only".to_string(),
                    checked: false,
                    id: 4,
                },
                MenuBarEntry::Separator,
                action(5, "Quit"),
            ],
        }])
    }

    /// Open the first dropdown and report its panel.
    fn open_first_dropdown(bar: &mut MenuBar) -> DropdownPanel {
        bar.handle_mouse_event(&click(10.0, BAR_HEIGHT / 2.0));
        assert!(bar.is_open(), "the bar should have opened a dropdown");
        bar.dropdown_panel(0)
    }

    /// The `(top, height)` of the hover highlight painted with the pointer at
    /// `py` — moved there through the real pointer path.
    fn highlight_after_pointer_at(bar: &mut MenuBar, py: f32) -> Option<(f32, f32)> {
        let panel = bar.dropdown_panel(0);
        bar.handle_mouse_event(&mouse_move(panel.x + 10.0, py));
        let (x, w) = (panel.x + 4.0, panel.width - 8.0);
        bar.render(800).into_iter().find_map(|cmd| match cmd {
            // Exact equality on purpose: these are the very floats the
            // renderer pushed, not a measurement of them.
            RenderCommand::FillRect {
                x: rx,
                y,
                width,
                height,
                color,
                ..
            } if rx == x && width == w && color == HOVER_COLOR => Some((y, height)),
            _ => None,
        })
    }

    /// The `(top, height)` of the hover highlight painted when the pointer is
    /// over dropdown row `idx`.
    fn painted_dropdown_row(bar: &mut MenuBar, idx: usize) -> Option<(f32, f32)> {
        let panel = bar.dropdown_panel(0);
        // Park the pointer using the layout's own answer for where the row is.
        // That is not circular: the *highlight it then paints* is what these
        // tests compare against the hit test, so a disagreement still shows.
        let probe = panel.row_top(children_of(&bar.items, 0), idx)? + 1.0;
        highlight_after_pointer_at(bar, probe)
    }

    #[test]
    fn every_dropdown_row_is_selectable_exactly_where_it_was_painted() {
        let mut bar = geometry_bar();
        let panel = open_first_dropdown(&mut bar);
        let count = children_of(&bar.items, 0).len();
        for idx in 0..count {
            if matches!(children_of(&bar.items, 0)[idx], MenuBarEntry::Separator) {
                continue;
            }
            let (top, height) = painted_dropdown_row(&mut bar, idx)
                .unwrap_or_else(|| panic!("row {idx} painted no highlight to measure"));
            let children = children_of(&bar.items, 0);
            // Sweep the painted row rather than probing its middle. A three
            // pixel drift is invisible at the centre of a 28-px row and
            // obvious at its edges — which is where the user aims.
            for step in 0..8 {
                let probe = top + (step as f32) * height / 8.0;
                assert_eq!(
                    panel.index_at(children, probe),
                    Some(idx),
                    "row {idx} was painted at {top}..{} but {probe} answers otherwise",
                    top + height
                );
            }
            // A row owns its top edge and not its bottom one, so the two sides
            // of a boundary never both answer for it.
            assert_ne!(panel.index_at(children, top - 0.001), Some(idx));
            assert_ne!(panel.index_at(children, top + height), Some(idx));
        }
    }

    #[test]
    fn a_dropdown_separator_is_drawn_inside_the_run_it_reserves_space_in() {
        let mut bar = geometry_bar();
        let panel = open_first_dropdown(&mut bar);
        let children = children_of(&bar.items, 0).to_vec();
        // The bar's own bottom border is drawn in the separator colour too,
        // so key on the panel's left inset as well — that is the x the
        // renderer actually pushed for a dropdown separator and nothing else.
        let sep_x = panel.x + DROPDOWN_HPAD;
        let lines: Vec<f32> = bar
            .render(800)
            .into_iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Line { x1, y1, color, .. }
                    if color == SEPARATOR_COLOR && x1 == sep_x =>
                {
                    Some(y1)
                }
                _ => None,
            })
            .collect();
        let sep_indices: Vec<usize> = children
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, MenuBarEntry::Separator))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(lines.len(), sep_indices.len());
        let strip = panel.strip(&children);
        for (&idx, &line_y) in sep_indices.iter().zip(&lines) {
            let top = strip.top(idx).unwrap();
            let height = strip.height(idx).unwrap();
            assert!(
                line_y >= top && line_y < top + height,
                "separator {idx} reserves {top}..{} but its line is drawn at {line_y}",
                top + height
            );
            // A separator has a place and a height like anything else; what it
            // does not have is selectability. That is the menu bar's rule, not
            // the strip's, and it is the one thing `DropdownPanel::index_at`
            // adds on top of the strip.
            assert_eq!(panel.index_at(&children, line_y), None);
            assert_eq!(panel.index_at(&children, top), None);
        }
    }

    #[test]
    fn a_submenu_hangs_off_the_dropdown_row_it_belongs_to() {
        // `DropdownPanel::row_top` is what positions an opening submenu. It
        // used to be its own walk of the heights; if it disagrees with the
        // renderer the submenu appears beside a different row than the one
        // the user is pointing at.
        let mut bar = geometry_bar();
        let panel = open_first_dropdown(&mut bar);
        let count = children_of(&bar.items, 0).len();
        for idx in 0..count {
            if matches!(children_of(&bar.items, 0)[idx], MenuBarEntry::Separator) {
                continue;
            }
            let (top, _) = painted_dropdown_row(&mut bar, idx).unwrap();
            let children = children_of(&bar.items, 0);
            assert_eq!(
                panel.row_top(children, idx),
                Some(top),
                "row {idx} is painted at {top} but a submenu would hang elsewhere"
            );
        }
        // Past the end there is no row, and a submenu falls back to the
        // panel's own top rather than to a position invented from nothing.
        assert_eq!(panel.row_top(children_of(&bar.items, 0), count), None);
    }

    #[test]
    fn a_dropdown_is_exactly_as_tall_as_the_rows_it_holds() {
        let mut bar = geometry_bar();
        let panel = open_first_dropdown(&mut bar);
        let count = children_of(&bar.items, 0).len();
        let (top, height) = painted_dropdown_row(&mut bar, count - 1).unwrap();
        assert_eq!(
            panel.bottom(),
            top + height + DROPDOWN_VPAD,
            "the panel must reach exactly one padding past the last row it \
             paints, or it clips a row or floats a gap"
        );
    }

    #[test]
    fn nothing_outside_the_dropdown_run_selects_a_row() {
        let mut bar = geometry_bar();
        let panel = open_first_dropdown(&mut bar);
        let children = children_of(&bar.items, 0);
        // The panel's own padding is its border, not row zero.
        assert_eq!(panel.index_at(children, panel.viewport_top() - 0.001), None);
        // Past the last row.
        assert_eq!(panel.index_at(children, panel.viewport_bottom()), None);
        assert_eq!(panel.index_at(children, f32::NAN), None);
        assert_eq!(panel.index_at(children, f32::INFINITY), None);
        // An empty dropdown answers for nothing at all.
        assert_eq!(panel.index_at(&[], panel.viewport_top()), None);
        assert_eq!(dropdown_content_height(&[]), 0.0);
    }

    // ── A dropdown taller than the screen ───────────────────────────────
    //
    // Everything above holds for a panel that fits, where the rows drawn and
    // the rows that exist are the same set. Once a panel scrolls they are not,
    // and the list genuinely extends past both ends of the panel — so a hit
    // test that consults only the row layout names a row nobody can see.

    /// A bar with one dropdown of `count` enabled rows, tall enough to need
    /// scrolling for any `count` past about forty.
    fn tall_bar(count: usize) -> MenuBar {
        MenuBar::new(vec![MenuBarItem {
            label: "&File".to_string(),
            children: (0..count)
                .map(|i| MenuBarEntry::Action {
                    label: format!("Item {i}"),
                    shortcut: None,
                    enabled: true,
                    id: i as u64,
                })
                .collect(),
        }])
    }

    fn wheel(x: f32, y: f32, dy: f32) -> MouseEvent {
        MouseEvent {
            x,
            y,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        }
    }

    /// The `(y, height)` of the clip the renderer pushed for the dropdown —
    /// the region it claims the rows are painted in.
    fn painted_clip(bar: &MenuBar) -> (f32, f32) {
        bar.render(800)
            .into_iter()
            .find_map(|cmd| match cmd {
                RenderCommand::PushClip { y, height, .. } => Some((y, height)),
                _ => None,
            })
            .expect("an open dropdown clips the region it draws its rows in")
    }

    /// The `((track_y, track_h), (thumb_y, thumb_h))` of the scroll indicator,
    /// or `None` when the panel drew none.
    fn scrollbar_rects(bar: &MenuBar) -> Option<((f32, f32), (f32, f32))> {
        let mut track = None;
        let mut thumb = None;
        for cmd in bar.render(800) {
            if let RenderCommand::FillRect {
                y, height, color, ..
            } = cmd
            {
                if color == SCROLLBAR_TRACK_COLOR {
                    track = Some((y, height));
                } else if color == SCROLLBAR_THUMB_COLOR {
                    thumb = Some((y, height));
                }
            }
        }
        Some((track?, thumb?))
    }

    #[test]
    fn a_dropdown_that_fits_neither_scrolls_nor_is_capped() {
        // The common case must be untouched by all of this: no cap, no
        // scrollbar, and a panel exactly as tall as its rows.
        let mut bar = geometry_bar();
        let panel = open_first_dropdown(&mut bar);
        assert_eq!(panel.panel_height, panel.content_height);
        assert_eq!(panel.max_scroll(), 0.0);
        assert_eq!(panel.scroll, 0.0);
        assert_eq!(scrollbar_rects(&bar), None);
    }

    #[test]
    fn a_dropdown_taller_than_the_screen_is_capped_rather_than_run_off_the_bottom() {
        // The defect this whole section exists for: a 200-row dropdown used to
        // be given its full content height, so the rows past the display's
        // bottom edge were drawn where no pointer could ever reach them.
        let mut bar = tall_bar(200);
        let panel = open_first_dropdown(&mut bar);
        assert!(
            panel.content_height > DEFAULT_VIEWPORT_HEIGHT,
            "the fixture must actually overflow the screen to test anything"
        );
        assert_eq!(panel.y, BAR_HEIGHT, "a capped panel starts under the bar");
        assert_eq!(panel.bottom(), DEFAULT_VIEWPORT_HEIGHT);
        assert!(panel.max_scroll() > 0.0);
    }

    #[test]
    fn every_row_of_a_tall_dropdown_can_be_reached_by_scrolling() {
        // Reachability is the user-visible claim: for every row there exists a
        // scroll offset at which a click selects it. Without the panel cap the
        // rows past the screen's bottom satisfy this for no offset at all.
        let mut bar = tall_bar(200);
        open_first_dropdown(&mut bar);
        let count = children_of(&bar.items, 0).len();
        let max = bar.dropdown_panel(0).max_scroll();
        let mut reached = vec![false; count];
        // Step by a whole row so no row can slip between two probes, and finish
        // at `max` itself — the last row only comes fully into view there, and a
        // sweep that stopped at the last whole step would leave it 20 px short.
        let mut offsets: Vec<f32> = Vec::new();
        let mut offset = 0.0_f32;
        while offset < max {
            offsets.push(offset);
            offset += ITEM_HEIGHT;
        }
        offsets.push(max);
        for offset in offsets {
            bar.dropdown_scroll = offset;
            let panel = bar.dropdown_panel(0);
            let children = children_of(&bar.items, 0);
            for idx in 0..count {
                if let Some(top) = panel.row_top(children, idx)
                    && panel.index_at(children, top + ITEM_HEIGHT / 2.0) == Some(idx)
                {
                    reached[idx] = true;
                }
            }
        }
        let unreachable: Vec<usize> = (0..count).filter(|&i| !reached[i]).collect();
        assert!(
            unreachable.is_empty(),
            "no scroll offset in 0..={max} brings rows {unreachable:?} under the pointer"
        );
    }

    #[test]
    fn every_visible_row_of_a_scrolled_dropdown_answers_exactly_where_it_was_painted() {
        // The property the whole design rests on: the strip the renderer draws
        // from and the strip the hit test reads are one strip, so scrolling
        // cannot make them disagree. Probed at several offsets because a
        // second subtraction of `scroll` somewhere would be invisible at zero.
        let mut bar = tall_bar(200);
        open_first_dropdown(&mut bar);
        let max = bar.dropdown_panel(0).max_scroll();
        for fraction in [0.0_f32, 0.13, 0.5, 0.87, 1.0] {
            bar.dropdown_scroll = max * fraction;
            let panel = bar.dropdown_panel(0);
            let (view_top, view_bottom) = (panel.viewport_top(), panel.viewport_bottom());
            let children = children_of(&bar.items, 0);
            for idx in 0..children.len() {
                let top = panel.row_top(children, idx).unwrap();
                let (lo, hi) = (top.max(view_top), (top + ITEM_HEIGHT).min(view_bottom));
                if hi <= lo {
                    // Scrolled clean off one end; it must answer for nothing.
                    assert_eq!(panel.index_at(children, top + ITEM_HEIGHT / 2.0), None);
                    continue;
                }
                // Sweep the visible part rather than probing its middle: a
                // three-pixel drift hides at the centre of a 28-px row.
                for step in 0..8 {
                    let probe = lo + (hi - lo) * (step as f32) / 8.0;
                    assert_eq!(
                        panel.index_at(children, probe),
                        Some(idx),
                        "at scroll {} row {idx} is drawn over {lo}..{hi} but {probe} answers \
                         otherwise",
                        panel.scroll
                    );
                }
            }
        }
    }

    #[test]
    fn the_padding_of_a_scrolled_dropdown_selects_nothing_even_though_a_row_is_under_it() {
        // This is what the visible-region test in `index_at` buys, and it only
        // bites once the panel scrolls: the list now extends into the padding
        // above the first visible row, so the strip alone would happily name
        // the row the padding is drawn over.
        let mut bar = tall_bar(200);
        open_first_dropdown(&mut bar);
        bar.dropdown_scroll = bar.dropdown_panel(0).max_scroll() / 2.0;
        let panel = bar.dropdown_panel(0);
        let children = children_of(&bar.items, 0);
        let above = panel.viewport_top() - DROPDOWN_VPAD / 2.0;
        assert!(
            panel.strip(children).index_at(above).is_some(),
            "the fixture must have a scrolled-away row under the top padding"
        );
        assert_eq!(panel.index_at(children, above), None);
        assert!(panel.contains(panel.x + 5.0, above), "still on the panel");
    }

    #[test]
    fn the_clip_a_dropdown_emits_is_the_region_the_pointer_lands_in() {
        // A clip that disagrees with the hit test is the same defect wearing a
        // different hat: rows answer where nothing was painted, or are painted
        // where nothing answers.
        for count in [5_usize, 200] {
            let mut bar = tall_bar(count);
            open_first_dropdown(&mut bar);
            bar.dropdown_scroll = bar.dropdown_panel(0).max_scroll() * 0.4;
            let (clip_y, clip_h) = painted_clip(&bar);
            let panel = bar.dropdown_panel(0);
            let children = children_of(&bar.items, 0);
            assert_eq!(clip_y, panel.viewport_top());
            assert_eq!(clip_y + clip_h, panel.viewport_bottom());
            // 400 probes down the whole screen: outside the painted region,
            // nothing answers.
            for step in 0..400 {
                let probe = (step as f32) * DEFAULT_VIEWPORT_HEIGHT / 400.0;
                if probe < clip_y || probe >= clip_y + clip_h {
                    assert_eq!(
                        panel.index_at(children, probe),
                        None,
                        "{probe} is outside the clip {clip_y}..{} but names a row in a \
                         {count}-row dropdown",
                        clip_y + clip_h
                    );
                }
            }
        }
    }

    #[test]
    fn the_wheel_scrolls_a_dropdown_to_each_end_and_stops_there() {
        let mut bar = tall_bar(200);
        let panel = open_first_dropdown(&mut bar);
        let max = panel.max_scroll();
        assert!(max > 0.0);
        let (px, py) = (panel.x + 10.0, panel.y + 40.0);
        for _ in 0..500 {
            bar.handle_mouse_event(&wheel(px, py, -1.0));
        }
        assert_eq!(bar.dropdown_scroll, max, "the wheel must reach the end");
        for _ in 0..500 {
            bar.handle_mouse_event(&wheel(px, py, 1.0));
        }
        assert_eq!(bar.dropdown_scroll, 0.0, "and come back to the start");
    }

    #[test]
    fn a_dropdown_swallows_the_wheel_even_with_nothing_to_scroll() {
        // Otherwise the notch falls through the panel to whatever it covers,
        // and the document behind the menu scrolls while a menu is open.
        let mut bar = geometry_bar();
        let panel = open_first_dropdown(&mut bar);
        assert_eq!(panel.max_scroll(), 0.0);
        let result = bar.handle_mouse_event(&wheel(panel.x + 10.0, panel.y + 10.0, -1.0));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(bar.dropdown_scroll, 0.0);
        // Off the panel it is not ours to take.
        let result = bar.handle_mouse_event(&wheel(panel.right() + 50.0, panel.y + 10.0, -1.0));
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn a_scroll_offset_that_is_not_a_number_is_refused_rather_than_clamped() {
        // `NaN` compares false against both ends of a clamp, so a clamp lets it
        // straight through to the strip's origin — and every row's position at
        // once becomes `NaN`, leaving a panel that answers for no pointer.
        let mut bar = tall_bar(200);
        open_first_dropdown(&mut bar);
        let panel = bar.dropdown_panel(0);
        let half = panel.max_scroll() / 2.0;
        bar.dropdown_scroll = half;
        let panel = bar.dropdown_panel(0);
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(panel.clamped_scroll(bad), half);
        }
        assert!(
            panel
                .index_at(children_of(&bar.items, 0), panel.viewport_top())
                .is_some()
        );
    }

    #[test]
    fn arrowing_down_a_tall_dropdown_brings_each_row_onto_the_screen() {
        // A selection the panel does not follow is a highlight the user cannot
        // see, on a list that appears not to respond to the arrow key at all.
        let mut bar = tall_bar(200);
        open_first_dropdown(&mut bar);
        for _ in 0..200 {
            bar.handle_key_event(&press(Key::Down));
            let hover = bar.dropdown_hover.expect("arrowing always selects a row");
            let panel = bar.dropdown_panel(0);
            let children = children_of(&bar.items, 0);
            let top = panel.row_top(children, hover).unwrap();
            assert!(
                top >= panel.viewport_top() && top + ITEM_HEIGHT <= panel.viewport_bottom(),
                "row {hover} is selected but sits at {top}, outside {}..{}",
                panel.viewport_top(),
                panel.viewport_bottom()
            );
        }
        // And back up again, which exercises the other branch of the scroll.
        for _ in 0..200 {
            bar.handle_key_event(&press(Key::Up));
            let hover = bar.dropdown_hover.unwrap();
            let panel = bar.dropdown_panel(0);
            let top = panel.row_top(children_of(&bar.items, 0), hover).unwrap();
            assert!(top >= panel.viewport_top() && top + ITEM_HEIGHT <= panel.viewport_bottom());
        }
    }

    #[test]
    fn the_dropdown_scroll_thumb_stays_in_its_track_and_reaches_both_ends() {
        let mut bar = tall_bar(200);
        open_first_dropdown(&mut bar);
        let max = bar.dropdown_panel(0).max_scroll();
        for fraction in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            bar.dropdown_scroll = max * fraction;
            let ((track_y, track_h), (thumb_y, thumb_h)) =
                scrollbar_rects(&bar).expect("a scrollable panel draws an indicator");
            assert!(
                thumb_y >= track_y && thumb_y + thumb_h <= track_y + track_h + 0.001,
                "at {fraction} the thumb {thumb_y}..{} leaves the track {track_y}..{}",
                thumb_y + thumb_h,
                track_y + track_h
            );
            if fraction == 0.0 {
                assert_eq!(thumb_y, track_y, "flush with the top at the start");
            }
            if fraction == 1.0 {
                assert!(
                    (thumb_y + thumb_h - (track_y + track_h)).abs() < 0.001,
                    "flush with the bottom at the end"
                );
            }
        }
    }

    #[test]
    fn reopening_a_scrolled_dropdown_shows_its_top() {
        // A menu that reopens where it was left is a menu that opens showing
        // rows unrelated to the label just clicked.
        let mut bar = tall_bar(200);
        open_first_dropdown(&mut bar);
        bar.dropdown_scroll = bar.dropdown_panel(0).max_scroll();
        bar.close();
        assert_eq!(bar.dropdown_scroll, 0.0);
        bar.open_menu(0);
        assert_eq!(bar.dropdown_scroll, 0.0);
        assert_eq!(
            bar.dropdown_panel(0).row_top(children_of(&bar.items, 0), 0),
            {
                let panel = bar.dropdown_panel(0);
                Some(panel.viewport_top())
            }
        );
    }

    #[test]
    fn a_submenu_of_a_scrolled_dropdown_hangs_off_the_row_it_belongs_to() {
        // The submenu's position is read from the same strip as everything
        // else, so scrolling the parent must carry the child with it. A
        // submenu placed from an unscrolled walk would hang beside whatever
        // row happens to occupy the old offset.
        fn sub_bar() -> MenuBar {
            let mut children: Vec<MenuBarEntry> = (0..60)
                .map(|i| MenuBarEntry::Action {
                    label: format!("Item {i}"),
                    shortcut: None,
                    enabled: true,
                    id: i,
                })
                .collect();
            children.push(MenuBarEntry::SubMenu {
                label: "More".to_string(),
                children: vec![MenuBarEntry::Action {
                    label: "Deep".to_string(),
                    shortcut: None,
                    enabled: true,
                    id: 999,
                }],
            });
            MenuBar::new(vec![MenuBarItem {
                label: "&File".to_string(),
                children,
            }])
        }
        let mut bar = sub_bar();
        open_first_dropdown(&mut bar);
        let sub_idx = children_of(&bar.items, 0).len() - 1;
        // Scroll the submenu row into view the way the user would, then open it.
        bar.dropdown_scroll = bar.dropdown_panel(0).max_scroll();
        let panel = bar.dropdown_panel(0);
        let row_top = panel
            .row_top(children_of(&bar.items, 0), sub_idx)
            .expect("the submenu row exists");
        bar.handle_mouse_event(&mouse_move(panel.x + 10.0, row_top + ITEM_HEIGHT / 2.0));
        assert_eq!(bar.dropdown_hover, Some(sub_idx));
        bar.handle_mouse_event(&click(panel.x + 10.0, row_top + ITEM_HEIGHT / 2.0));
        let sub = bar.open_submenu.as_ref().expect("the submenu opened");
        assert_eq!(sub.y, row_top, "the child must hang off the scrolled row");
        assert_eq!(sub.x, panel.right());
        assert_eq!(sub.scroll, 0.0, "a freshly opened child shows its top");
    }

    #[test]
    fn the_wheel_over_a_submenu_scrolls_the_submenu_and_not_its_parent() {
        // Two panels are on screen at once, and each carries its own offset.
        // A single shared offset would scroll the parent out from under the
        // child the user is pointing at.
        fn nested_bar() -> MenuBar {
            MenuBar::new(vec![MenuBarItem {
                label: "&File".to_string(),
                children: vec![MenuBarEntry::SubMenu {
                    label: "Many".to_string(),
                    children: (0..200)
                        .map(|i| MenuBarEntry::Action {
                            label: format!("Item {i}"),
                            shortcut: None,
                            enabled: true,
                            id: i,
                        })
                        .collect(),
                }],
            }])
        }
        let mut bar = nested_bar();
        let panel = open_first_dropdown(&mut bar);
        let row_top = panel.row_top(children_of(&bar.items, 0), 0).unwrap();
        bar.handle_mouse_event(&click(panel.x + 10.0, row_top + ITEM_HEIGHT / 2.0));
        let (sub_x, sub_y) = {
            let sub = bar.open_submenu.as_ref().expect("the submenu opened");
            (sub.x, sub.y)
        };
        let before = bar.dropdown_scroll;
        for _ in 0..5 {
            bar.handle_mouse_event(&wheel(sub_x + 10.0, sub_y + 40.0, -1.0));
        }
        let sub = bar.open_submenu.as_ref().unwrap();
        assert!(sub.scroll > 0.0, "the wheel scrolled the panel under it");
        assert_eq!(
            bar.dropdown_scroll, before,
            "and left the parent where it was"
        );
    }

    #[test]
    fn every_visible_row_of_a_scrolled_submenu_answers_where_it_was_painted() {
        // The submenu chain reads the same panel type, so this is the parent's
        // property restated for the path that used to carry its own copy of
        // the arithmetic.
        let entries: Vec<MenuBarEntry> = (0..200)
            .map(|i| MenuBarEntry::Action {
                label: format!("Item {i}"),
                shortcut: None,
                enabled: true,
                id: i,
            })
            .collect();
        let mut sub = OpenSubmenu {
            parent_index: 0,
            x: 200.0,
            y: BAR_HEIGHT,
            width: 180.0,
            hover_index: None,
            scroll: 0.0,
            child: None,
        };
        let max = submenu_panel(&entries, &sub).max_scroll();
        assert!(max > 0.0, "the fixture must overflow to test anything");
        for fraction in [0.0_f32, 0.37, 1.0] {
            sub.scroll = max * fraction;
            let panel = submenu_panel(&entries, &sub);
            let (view_top, view_bottom) = (panel.viewport_top(), panel.viewport_bottom());
            for idx in 0..entries.len() {
                let top = panel.row_top(&entries, idx).unwrap();
                let (lo, hi) = (top.max(view_top), (top + ITEM_HEIGHT).min(view_bottom));
                if hi <= lo {
                    assert_eq!(panel.index_at(&entries, top + ITEM_HEIGHT / 2.0), None);
                    continue;
                }
                for step in 0..8 {
                    let probe = lo + (hi - lo) * (step as f32) / 8.0;
                    assert_eq!(panel.index_at(&entries, probe), Some(idx));
                }
            }
        }
    }

    // ── Nested submenus: which entry list does level N show? ────────────
    //
    // `parent_index` indexes into the level directly above its node, so
    // resolving a node at depth 2 against the *root's* children reads that
    // index against a list it does not describe. The fixture below is built so
    // that mistake is visible rather than merely wrong: root child 0 is a
    // decoy submenu, so the old code resolved the depth-2 panel to the decoy's
    // entries and drew, hit-tested and activated them — a menu that looks
    // plausible and does the wrong thing.

    /// `File > Level One > Level Two`, with a decoy submenu parked at the root
    /// index that the depth-2 node's `parent_index` happens to equal.
    fn nested_decoy_bar() -> MenuBar {
        MenuBar::new(vec![MenuBarItem {
            label: "&File".to_string(),
            children: vec![
                MenuBarEntry::SubMenu {
                    label: "Decoy".to_string(),
                    children: vec![MenuBarEntry::Action {
                        label: "Wrong".to_string(),
                        shortcut: None,
                        enabled: true,
                        id: 700,
                    }],
                },
                MenuBarEntry::SubMenu {
                    label: "Level One".to_string(),
                    children: vec![
                        MenuBarEntry::SubMenu {
                            label: "Level Two".to_string(),
                            children: vec![
                                MenuBarEntry::Action {
                                    label: "Deep A".to_string(),
                                    shortcut: None,
                                    enabled: true,
                                    id: 900,
                                },
                                MenuBarEntry::Action {
                                    label: "Deep B".to_string(),
                                    shortcut: None,
                                    enabled: true,
                                    id: 901,
                                },
                            ],
                        },
                        MenuBarEntry::Action {
                            label: "One B".to_string(),
                            shortcut: None,
                            enabled: true,
                            id: 800,
                        },
                    ],
                },
            ],
        }])
    }

    /// Open `File > Level One > Level Two` through the real pointer path and
    /// hand back a point inside the depth-2 panel's first row.
    ///
    /// A submenu node's `x`/`y` *are* its panel's origin — [`submenu_panel`]
    /// passes them straight through — so a probe can be built from the node
    /// without resolving its entries, which is exactly what the test must not
    /// assume it can do correctly.
    fn open_depth_two(bar: &mut MenuBar) -> (f32, f32) {
        fn first_row_probe(sub: &OpenSubmenu) -> (f32, f32) {
            (sub.x + 10.0, sub.y + DROPDOWN_VPAD + ITEM_HEIGHT / 2.0)
        }

        let panel = open_first_dropdown(bar);
        // Row 1 of the dropdown is "Level One".
        let row = panel.row_top(children_of(&bar.items, 0), 1).unwrap();
        bar.handle_mouse_event(&mouse_move(panel.x + 10.0, row + ITEM_HEIGHT / 2.0));
        bar.handle_mouse_event(&click(panel.x + 10.0, row + ITEM_HEIGHT / 2.0));

        // Row 0 of that submenu is "Level Two"; hovering it opens the child.
        let (px, py) = first_row_probe(bar.open_submenu.as_ref().expect("Level One opened"));
        bar.handle_mouse_event(&mouse_move(px, py));

        let child = bar
            .open_submenu
            .as_ref()
            .and_then(|s| s.child.as_ref())
            .expect("Level Two opened as a child of Level One");
        first_row_probe(child)
    }

    #[test]
    fn a_third_level_submenu_shows_its_own_entries_and_not_the_roots() {
        let mut bar = nested_decoy_bar();
        let _ = open_depth_two(&mut bar);

        let labels: Vec<String> = bar
            .render(800)
            .into_iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            labels.iter().any(|t| t == "Deep A") && labels.iter().any(|t| t == "Deep B"),
            "the depth-2 panel must draw its own entries; drew {labels:?}"
        );
        assert!(
            !labels.iter().any(|t| t == "Wrong"),
            "and must not draw the decoy's, which is what indexing the root's \
             children with a depth-2 parent_index yields; drew {labels:?}"
        );
    }

    #[test]
    fn clicking_a_third_level_row_activates_the_entry_that_was_drawn_there() {
        let mut bar = nested_decoy_bar();
        let (px, py) = open_depth_two(&mut bar);
        bar.handle_mouse_event(&click(px, py));
        assert_eq!(
            bar.drain_events(),
            vec![MenuBarEvent::ItemClicked(900)],
            "row 0 of Level Two is Deep A; the decoy's id 700 means the hit \
             test resolved the wrong entry list"
        );
    }

    #[test]
    fn the_keyboard_walks_the_third_level_and_not_the_first() {
        // The keyboard jumps straight to the deepest node instead of
        // descending, so it needs its own way to resolve that node's entries.
        let mut bar = nested_decoy_bar();
        let _ = open_depth_two(&mut bar);

        bar.handle_key_event(&press(Key::Down));
        bar.handle_key_event(&press(Key::Down));
        {
            let deepest = bar
                .open_submenu
                .as_ref()
                .and_then(|s| s.child.as_ref())
                .expect("still at depth 2");
            assert_eq!(
                deepest.hover_index,
                Some(1),
                "two Downs reach Deep B, the second of two entries; the decoy \
                 has only one, so a wrong resolve stops at 0"
            );
        }
        bar.handle_key_event(&press(Key::Enter));
        assert_eq!(bar.drain_events(), vec![MenuBarEvent::ItemClicked(901)]);
    }

    #[test]
    fn the_wheel_over_a_third_level_submenu_finds_entries_to_scroll() {
        // A panel resolved to an empty list has nothing to scroll and no rows
        // to draw, so the wheel silently did nothing over a deep submenu. The
        // assertion is on the scroll actually moving, which is only possible if
        // the walker resolved a list long enough to overflow.
        let deep: Vec<MenuBarEntry> = (0..200)
            .map(|i| MenuBarEntry::Action {
                label: format!("Deep {i}"),
                shortcut: None,
                enabled: true,
                id: 1000 + i,
            })
            .collect();
        let mut bar = MenuBar::new(vec![MenuBarItem {
            label: "&File".to_string(),
            children: vec![
                MenuBarEntry::SubMenu {
                    label: "Decoy".to_string(),
                    children: vec![MenuBarEntry::Action {
                        label: "Wrong".to_string(),
                        shortcut: None,
                        enabled: true,
                        id: 700,
                    }],
                },
                MenuBarEntry::SubMenu {
                    label: "Level One".to_string(),
                    children: vec![MenuBarEntry::SubMenu {
                        label: "Level Two".to_string(),
                        children: deep,
                    }],
                },
            ],
        }]);
        let (px, py) = open_depth_two(&mut bar);
        for _ in 0..5 {
            bar.handle_mouse_event(&wheel(px, py, -1.0));
        }
        let child = bar
            .open_submenu
            .as_ref()
            .and_then(|s| s.child.as_ref())
            .unwrap();
        assert!(
            child.scroll > 0.0,
            "the deep panel overflows, so the wheel must move it"
        );
    }

    // ── Submenus near the right edge of the screen ──────────────────────

    #[test]
    fn a_child_sits_beside_its_parent_until_that_would_leave_the_screen() {
        let panel = |x: f32, width: f32| DropdownPanel {
            x,
            y: BAR_HEIGHT,
            width,
            content_height: 100.0,
            panel_height: 100.0,
            scroll: 0.0,
        };
        // Room to the right: the child goes there.
        assert_eq!(panel(100.0, 200.0).child_origin(180.0), 300.0);
        // Exactly filling the screen still counts as fitting.
        assert_eq!(panel(1500.0, 240.0).child_origin(180.0), 1740.0);
        // One pixel too wide, and it flips to end where its parent starts.
        assert_eq!(panel(1500.0, 240.0).child_origin(181.0), 1319.0);
        // Nowhere to go on either side: the left edge beats a negative x,
        // because a panel at x < 0 is neither visible nor clickable.
        assert_eq!(panel(50.0, 100.0).child_origin(1900.0), 0.0);
    }

    #[test]
    fn a_submenu_that_would_run_off_the_right_edge_opens_to_the_left() {
        let deep = vec![MenuBarEntry::Action {
            label: "Deep".to_string(),
            shortcut: None,
            enabled: true,
            id: 900,
        }];
        let parent_entries = vec![MenuBarEntry::SubMenu {
            label: "Holder".to_string(),
            children: vec![MenuBarEntry::SubMenu {
                label: "Opens Left".to_string(),
                children: deep,
            }],
        }];
        let mut sub = OpenSubmenu {
            parent_index: 0,
            // Far enough right that anything hung off its right edge is off
            // the screen.
            x: DEFAULT_VIEWPORT_WIDTH - 200.0,
            y: BAR_HEIGHT,
            width: 180.0,
            hover_index: None,
            scroll: 0.0,
            child: None,
        };
        let entries = resolve_submenu_entries(&parent_entries, &sub);
        let panel = submenu_panel(&entries, &sub);
        assert!(
            hover_in_submenu_chain(
                &parent_entries,
                &mut sub,
                panel.x + 10.0,
                panel.viewport_top() + ITEM_HEIGHT / 2.0,
            ),
            "the pointer is over the panel"
        );
        let child = sub.child.as_ref().expect("hovering a submenu row opens it");
        assert!(
            child.x + child.width <= DEFAULT_VIEWPORT_WIDTH,
            "child spans {}..{} of a {DEFAULT_VIEWPORT_WIDTH}-wide screen",
            child.x,
            child.x + child.width
        );
        assert_eq!(
            child.x + child.width,
            sub.x,
            "a flipped child ends flush with its parent's left edge"
        );
    }

    // ── Mnemonic parsing ────────────────────────────────────────────────

    #[test]
    fn parse_mnemonic_simple() {
        let p = parse_mnemonic("&File");
        assert_eq!(p.text, "File");
        assert_eq!(p.mnemonic_index, Some(0));
    }

    #[test]
    fn parse_mnemonic_mid_word() {
        let p = parse_mnemonic("E&xit");
        assert_eq!(p.text, "Exit");
        assert_eq!(p.mnemonic_index, Some(1));
    }

    #[test]
    fn parse_mnemonic_escaped_ampersand() {
        let p = parse_mnemonic("Save && Quit");
        assert_eq!(p.text, "Save & Quit");
        assert_eq!(p.mnemonic_index, None);
    }

    #[test]
    fn parse_mnemonic_no_ampersand() {
        let p = parse_mnemonic("Help");
        assert_eq!(p.text, "Help");
        assert_eq!(p.mnemonic_index, None);
    }

    #[test]
    fn mnemonic_char_extraction() {
        assert_eq!(mnemonic_char("&File"), Some('f'));
        assert_eq!(mnemonic_char("&Edit"), Some('e'));
        assert_eq!(mnemonic_char("Help"), None);
    }

    // ── Initial state ───────────────────────────────────────────────────

    #[test]
    fn initially_closed() {
        let bar = make_bar();
        assert!(!bar.is_open());
    }

    #[test]
    fn drain_events_empty_initially() {
        let mut bar = make_bar();
        assert!(bar.drain_events().is_empty());
    }

    // ── Open / close via mouse ──────────────────────────────────────────

    #[test]
    fn click_label_opens_dropdown() {
        let mut bar = make_bar();
        let x = bar.label_metrics[0].0 + 5.0;
        bar.handle_mouse_event(&click(x, BAR_HEIGHT / 2.0));
        assert!(bar.is_open());
        assert_eq!(bar.open_index, Some(0));
    }

    #[test]
    fn click_open_label_toggles_off() {
        let mut bar = make_bar();
        let x = bar.label_metrics[0].0 + 5.0;
        bar.handle_mouse_event(&click(x, BAR_HEIGHT / 2.0));
        assert!(bar.is_open());

        bar.handle_mouse_event(&click(x, BAR_HEIGHT / 2.0));
        assert!(!bar.is_open());
    }

    #[test]
    fn click_outside_closes() {
        let mut bar = make_bar();
        let x = bar.label_metrics[0].0 + 5.0;
        bar.handle_mouse_event(&click(x, BAR_HEIGHT / 2.0));
        assert!(bar.is_open());

        bar.handle_mouse_event(&click(9999.0, 9999.0));
        assert!(!bar.is_open());
    }

    // ── Hot-tracking ────────────────────────────────────────────────────

    #[test]
    fn hot_tracking_switches_menu() {
        let mut bar = make_bar();
        let x0 = bar.label_metrics[0].0 + 5.0;
        bar.handle_mouse_event(&click(x0, BAR_HEIGHT / 2.0));
        assert_eq!(bar.open_index, Some(0));

        let x1 = bar.label_metrics[1].0 + 5.0;
        bar.handle_mouse_event(&mouse_move(x1, BAR_HEIGHT / 2.0));
        assert_eq!(bar.open_index, Some(1));
    }

    // ── Click dropdown item generates event ─────────────────────────────

    #[test]
    fn click_action_item_emits_event() {
        let mut bar = make_bar();
        let lbl_x = bar.label_metrics[0].0 + 5.0;
        bar.handle_mouse_event(&click(lbl_x, BAR_HEIGHT / 2.0));

        let dd = bar.dropdown_panel(0);
        let item_y = dd.viewport_top() + ITEM_HEIGHT / 2.0;
        bar.handle_mouse_event(&click(dd.x + 40.0, item_y));

        let events = bar.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], MenuBarEvent::ItemClicked(1));
        assert!(!bar.is_open());
    }

    #[test]
    fn click_check_item_toggles() {
        let mut bar = make_bar();
        let lbl_x = bar.label_metrics[1].0 + 5.0;
        bar.handle_mouse_event(&click(lbl_x, BAR_HEIGHT / 2.0));

        let dd = bar.dropdown_panel(1);
        let item_y = dd.viewport_top() + ITEM_HEIGHT + SEPARATOR_HEIGHT + ITEM_HEIGHT / 2.0;
        bar.handle_mouse_event(&click(dd.x + 40.0, item_y));

        let events = bar.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], MenuBarEvent::CheckToggled(20, false));
    }

    // ── Keyboard: Alt+mnemonic ──────────────────────────────────────────

    #[test]
    fn alt_mnemonic_opens_menu() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        assert!(bar.is_open());
        assert_eq!(bar.open_index, Some(0));
    }

    #[test]
    fn alt_mnemonic_second_menu() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::E));
        assert_eq!(bar.open_index, Some(1));
    }

    // ── Keyboard: navigation ────────────────────────────────────────────

    #[test]
    fn arrow_down_moves_hover() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        bar.handle_key_event(&press(Key::Down));
        assert_eq!(bar.dropdown_hover, Some(0));

        bar.handle_key_event(&press(Key::Down));
        assert_eq!(bar.dropdown_hover, Some(1));

        // Skip separator (2) and disabled Save As (4) -> Save (3)
        bar.handle_key_event(&press(Key::Down));
        assert_eq!(bar.dropdown_hover, Some(3));
    }

    #[test]
    fn arrow_down_skips_disabled() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        bar.handle_key_event(&press(Key::Down)); // 0 = New
        bar.handle_key_event(&press(Key::Down)); // 1 = Open
        bar.handle_key_event(&press(Key::Down)); // 3 = Save (skips sep + disabled)
        assert_eq!(bar.dropdown_hover, Some(3));

        // Down again: Save As (4) is disabled, wraps to New (0).
        bar.handle_key_event(&press(Key::Down));
        assert_eq!(bar.dropdown_hover, Some(0));
    }

    #[test]
    fn arrow_up_wraps() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        bar.handle_key_event(&press(Key::Up)); // wrap to last selectable = Save (3)
        assert_eq!(bar.dropdown_hover, Some(3));
    }

    #[test]
    fn left_right_switch_menus() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F)); // File (0)
        assert_eq!(bar.open_index, Some(0));

        bar.handle_key_event(&press(Key::Right)); // Edit (1)
        assert_eq!(bar.open_index, Some(1));

        bar.handle_key_event(&press(Key::Right)); // View (2)
        assert_eq!(bar.open_index, Some(2));

        bar.handle_key_event(&press(Key::Right)); // wrap to File (0)
        assert_eq!(bar.open_index, Some(0));

        bar.handle_key_event(&press(Key::Left)); // wrap to View (2)
        assert_eq!(bar.open_index, Some(2));
    }

    #[test]
    fn enter_selects_hovered_item() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        bar.handle_key_event(&press(Key::Down)); // hover on New
        bar.handle_key_event(&press(Key::Enter));

        let events = bar.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], MenuBarEvent::ItemClicked(1));
        assert!(!bar.is_open());
    }

    #[test]
    fn escape_closes_menu() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        assert!(bar.is_open());

        bar.handle_key_event(&press(Key::Escape));
        assert!(!bar.is_open());
    }

    // ── Keyboard: type-to-jump ──────────────────────────────────────────

    #[test]
    fn type_letter_jumps_to_item() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        bar.handle_key_event(&press(Key::S)); // jump to "Save"
        assert_eq!(bar.dropdown_hover, Some(3));
    }

    // ── Keyboard: submenu via Right arrow ───────────────────────────────

    #[test]
    fn right_opens_submenu() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::V)); // open View
        bar.handle_key_event(&press(Key::Down)); // hover Zoom (submenu)
        assert_eq!(bar.dropdown_hover, Some(0));

        bar.handle_key_event(&press(Key::Right)); // open Zoom submenu
        assert!(bar.open_submenu.is_some());
    }

    // ── Rendering ───────────────────────────────────────────────────────

    #[test]
    fn render_closed_produces_bar_only() {
        let bar = make_bar();
        let cmds = bar.render(800);
        assert!(!cmds.is_empty());
        // No BoxShadow when closed (that only appears for dropdowns).
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, RenderCommand::BoxShadow { .. }))
        );
    }

    #[test]
    fn render_open_produces_dropdown() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        let cmds = bar.render(800);
        assert!(
            cmds.iter()
                .any(|c| matches!(c, RenderCommand::BoxShadow { .. }))
        );
    }

    // ── set_items replaces structure ────────────────────────────────────

    #[test]
    fn set_items_replaces_and_closes() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        assert!(bar.is_open());

        bar.set_items(vec![MenuBarItem {
            label: "&Help".to_string(),
            children: vec![MenuBarEntry::Action {
                label: "About".to_string(),
                shortcut: None,
                enabled: true,
                id: 100,
            }],
        }]);

        assert!(!bar.is_open());
        assert_eq!(bar.items.len(), 1);
        assert_eq!(mnemonic_char(&bar.items[0].label), Some('h'));
    }

    // ── close() is idempotent ───────────────────────────────────────────

    #[test]
    fn close_when_already_closed() {
        let mut bar = make_bar();
        bar.close();
        assert!(!bar.is_open());
    }

    // ── Edge: empty menu bar ────────────────────────────────────────────

    #[test]
    fn empty_bar_renders() {
        let bar = MenuBar::new(vec![]);
        let cmds = bar.render(400);
        // Bar background + bottom border line.
        assert_eq!(cmds.len(), 2);
    }

    // ── Edge: disabled action not activated ─────────────────────────────

    #[test]
    fn disabled_action_not_activated_by_keyboard() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        // Directly set hover to the disabled "Save As..." (index 4).
        bar.dropdown_hover = Some(4);

        bar.handle_key_event(&press(Key::Enter));
        let events = bar.drain_events();
        assert!(events.is_empty());
    }

    // ── Hover highlight in dropdown ─────────────────────────────────────

    #[test]
    fn mouse_move_in_dropdown_updates_hover() {
        let mut bar = make_bar();
        let lbl_x = bar.label_metrics[0].0 + 5.0;
        bar.handle_mouse_event(&click(lbl_x, BAR_HEIGHT / 2.0));

        let dd = bar.dropdown_panel(0);
        let item_y = dd.viewport_top() + ITEM_HEIGHT + ITEM_HEIGHT / 2.0;
        bar.handle_mouse_event(&mouse_move(dd.x + 40.0, item_y));
        assert_eq!(bar.dropdown_hover, Some(1)); // "Open"
    }

    // ── Invariants the shared helpers are there to hold ─────────────────

    /// A submenu must land in the same place however it was opened. The
    /// placement used to be written out once per input path, and nothing
    /// checked that the three copies agreed.
    #[test]
    fn a_submenu_opens_in_the_same_place_by_click_arrow_and_hover() {
        // "&View" is top-level index 2 and its entry 0 is the "Zoom" submenu.
        let geometry = |bar: &MenuBar| {
            let sub = bar.open_submenu.as_ref().expect("a submenu is open");
            (sub.parent_index, sub.x, sub.y, sub.width)
        };
        let dd = make_bar().dropdown_panel(2);
        let (row_x, row_y) = (dd.x + 5.0, dd.viewport_top() + ITEM_HEIGHT / 2.0);

        let by_click = {
            let mut bar = make_bar();
            bar.open_menu(2);
            bar.handle_mouse_event(&click(row_x, row_y));
            geometry(&bar)
        };
        let by_arrow = {
            let mut bar = make_bar();
            bar.open_menu(2);
            bar.handle_key_event(&press(Key::Down)); // hover lands on entry 0
            bar.handle_key_event(&press(Key::Right));
            geometry(&bar)
        };
        let by_hover = {
            let mut bar = make_bar();
            bar.open_menu(2);
            bar.handle_mouse_event(&mouse_move(row_x, row_y));
            geometry(&bar)
        };

        assert_eq!(by_click.0, 0, "the submenu hangs off entry 0");
        assert_eq!(by_click, by_arrow, "click and Right-arrow must agree");
        assert_eq!(by_click, by_hover, "click and hover must agree");
    }

    /// `set_items` closes the bar, so `open_index` should never name a menu
    /// that is not there. If it ever did, the bar must come out empty rather
    /// than panicking — nineteen sites used to index straight into `items`.
    #[test]
    fn a_top_level_index_with_no_menu_behind_it_is_inert() {
        let mut bar = make_bar();
        bar.open_index = Some(99);
        bar.dropdown_hover = Some(3);

        let _ = bar.render(800);
        bar.activate_entry(99, 3);

        assert!(children_of(&bar.items, 99).is_empty());
        assert!(bar.drain_events().is_empty(), "no entry can be activated");
        assert!(bar.open_submenu.is_none(), "no submenu can be opened");
    }

    /// A dropdown with no row the keyboard can land on must say so, not circle.
    #[test]
    fn a_dropdown_with_no_selectable_row_yields_no_hover() {
        let separators = vec![MenuBarEntry::Separator; 3];
        assert_eq!(next_selectable(&separators, None, 1), None);
        assert_eq!(next_selectable(&separators, None, -1), None);
        assert_eq!(next_selectable(&separators, Some(1), 1), None);
        assert_eq!(next_selectable(&[], None, 1), None);
    }

    #[test]
    fn left_from_the_first_menu_opens_the_last() {
        let mut bar = make_bar();
        bar.open_menu(0);
        bar.handle_key_event(&press(Key::Left));
        assert_eq!(bar.open_index, Some(2));
        bar.handle_key_event(&press(Key::Right));
        assert_eq!(bar.open_index, Some(0), "and Right comes back round");
    }
}

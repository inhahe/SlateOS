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
use crate::render::{FontWeightHint, RenderCommand};
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

/// Rough monospace-ish text width estimation (same heuristic as `menu.rs`).
fn estimate_text_width(text: &str, font_size: f32) -> f32 {
    text.len() as f32 * font_size * 0.6
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
    /// Recursively open child submenu.
    child: Option<Box<OpenSubmenu>>,
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
    /// Open nested submenu chain inside the current dropdown.
    open_submenu: Option<Box<OpenSubmenu>>,
    /// Pending events to be drained by the caller.
    events: Vec<MenuBarEvent>,
    /// Cached per-label metrics: `(x_offset, width, parsed_label)`.
    label_metrics: Vec<(f32, f32, ParsedLabel)>,
}

impl MenuBar {
    // ── Construction ────────────────────────────────────────────────────

    /// Create a new menu bar from the given top-level items.
    pub fn new(items: Vec<MenuBarItem>) -> Self {
        let label_metrics = Self::compute_label_metrics(&items);
        Self {
            items,
            open_index: None,
            dropdown_hover: None,
            open_submenu: None,
            events: Vec::new(),
            label_metrics,
        }
    }

    /// Replace the entire menu structure.
    pub fn set_items(&mut self, items: Vec<MenuBarItem>) {
        self.label_metrics = Self::compute_label_metrics(&items);
        self.items = items;
        self.close();
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
            _ => EventResult::Ignored,
        }
    }

    fn on_mouse_press(&mut self, mx: f32, my: f32) -> EventResult {
        // --- Click on a top-level label? ---
        if (0.0..BAR_HEIGHT).contains(&my)
            && let Some(idx) = self.label_index_at_x(mx) {
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
                    click_in_submenu_chain(&self.items[top_idx].children, &mut sub, mx, my);
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
            let dd = self.dropdown_rect(top_idx);
            if mx >= dd.0 && mx < dd.0 + dd.2 && my >= dd.1 && my < dd.1 + dd.3 {
                let children = &self.items[top_idx].children;
                if let Some(item_idx) = item_index_at_y(children, my - dd.1 - DROPDOWN_VPAD) {
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
                && let Some(idx) = self.label_index_at_x(mx) {
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
                    hover_in_submenu_chain(&self.items[top_idx].children, &mut sub, mx, my);
                self.open_submenu = Some(sub);
                if in_sub {
                    self.dropdown_hover = None;
                    return EventResult::Consumed;
                }
            }

            let dd = self.dropdown_rect(top_idx);
            if mx >= dd.0 && mx < dd.0 + dd.2 && my >= dd.1 && my < dd.1 + dd.3 {
                let new_hover =
                    item_index_at_y(&self.items[top_idx].children, my - dd.1 - DROPDOWN_VPAD);
                self.dropdown_hover = new_hover;

                // Open / close submenu on hover.
                if let Some(hi) = new_hover {
                    let is_submenu = matches!(
                        self.items[top_idx].children.get(hi),
                        Some(MenuBarEntry::SubMenu { .. })
                    );
                    let already_open = self
                        .open_submenu
                        .as_ref()
                        .is_some_and(|s| s.parent_index == hi);

                    if is_submenu && !already_open {
                        if let Some(MenuBarEntry::SubMenu { children: sc, .. }) =
                            self.items[top_idx].children.get(hi)
                        {
                            let sub_x = dd.0 + dd.2;
                            let sub_y = dd.1
                                + DROPDOWN_VPAD
                                + y_offset_for_index(&self.items[top_idx].children, hi);
                            let sub_w = calculate_dropdown_width(sc);
                            self.open_submenu = Some(Box::new(OpenSubmenu {
                                parent_index: hi,
                                x: sub_x,
                                y: sub_y,
                                width: sub_w,
                                hover_index: None,
                                child: None,
                            }));
                        }
                    } else if !is_submenu {
                        self.open_submenu = None;
                    }
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
        if event.modifiers.alt && !event.modifiers.ctrl && !event.modifiers.shift
            && let Some(ch) = key_to_lower_char(&event.key) {
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
                    let new = if top_idx == 0 {
                        self.items.len().saturating_sub(1)
                    } else {
                        top_idx - 1
                    };
                    self.open_menu(new);
                }
                EventResult::Consumed
            }

            Key::Right => {
                // If hover is on a submenu entry in the primary dropdown, open it.
                if self.open_submenu.is_none()
                    && let Some(hi) = self.dropdown_hover
                        && let Some(MenuBarEntry::SubMenu { children, .. }) =
                            self.items[top_idx].children.get(hi)
                        {
                            let dd = self.dropdown_rect(top_idx);
                            let sub_x = dd.0 + dd.2;
                            let sub_y = dd.1
                                + DROPDOWN_VPAD
                                + y_offset_for_index(&self.items[top_idx].children, hi);
                            let sub_w = calculate_dropdown_width(children);
                            self.open_submenu = Some(Box::new(OpenSubmenu {
                                parent_index: hi,
                                x: sub_x,
                                y: sub_y,
                                width: sub_w,
                                hover_index: None,
                                child: None,
                            }));
                            return EventResult::Consumed;
                        }

                // Otherwise move to the next top-level menu.
                let new = if top_idx + 1 >= self.items.len() {
                    0
                } else {
                    top_idx + 1
                };
                self.open_menu(new);
                EventResult::Consumed
            }

            Key::Up => {
                if let Some(ref mut sub) = self.open_submenu {
                    let deepest = deepest_submenu_mut(sub);
                    let entries =
                        resolve_submenu_entries(&self.items[top_idx].children, deepest);
                    deepest.hover_index = next_selectable(&entries, deepest.hover_index, -1);
                } else {
                    self.move_dropdown_hover(-1, top_idx);
                }
                EventResult::Consumed
            }

            Key::Down => {
                if let Some(ref mut sub) = self.open_submenu {
                    let deepest = deepest_submenu_mut(sub);
                    let entries =
                        resolve_submenu_entries(&self.items[top_idx].children, deepest);
                    deepest.hover_index = next_selectable(&entries, deepest.hover_index, 1);
                } else {
                    self.move_dropdown_hover(1, top_idx);
                }
                EventResult::Consumed
            }

            Key::Enter => {
                if let Some(ref sub) = self.open_submenu {
                    let deepest = find_deepest(sub);
                    if let Some(hi) = deepest.hover_index {
                        let entries =
                            resolve_submenu_entries(&self.items[top_idx].children, deepest);
                        if let Some(act) = try_activate_entry(&entries, hi) {
                            self.apply_activation(act);
                        }
                    }
                } else if let Some(hi) = self.dropdown_hover {
                    self.activate_entry(top_idx, hi);
                }
                EventResult::Consumed
            }

            _ => {
                // Type-to-jump: letter key jumps to first matching item label.
                if let Some(ch) = key_to_lower_char(&event.key) {
                    if let Some(ref mut sub) = self.open_submenu {
                        let deepest = deepest_submenu_mut(sub);
                        let entries =
                            resolve_submenu_entries(&self.items[top_idx].children, deepest);
                        deepest.hover_index = jump_to_letter(&entries, ch);
                    } else {
                        let children = &self.items[top_idx].children;
                        self.dropdown_hover = jump_to_letter(children, ch);
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
        let (dd_x, dd_y, dd_w, dd_h) = self.dropdown_rect(top_idx);
        let children = &self.items[top_idx].children;

        render_panel(cmds, dd_x, dd_y, dd_w, dd_h);
        render_entries(cmds, children, dd_x, dd_y, dd_w, self.dropdown_hover);

        // Submenu chain.
        if let Some(ref sub) = self.open_submenu {
            self.render_submenu_chain(cmds, sub, top_idx);
        }
    }

    fn render_submenu_chain(
        &self,
        cmds: &mut Vec<RenderCommand>,
        sub: &OpenSubmenu,
        top_idx: usize,
    ) {
        let entries = resolve_submenu_entries(&self.items[top_idx].children, sub);
        let height = dropdown_content_height(&entries) + DROPDOWN_VPAD * 2.0;

        render_panel(cmds, sub.x, sub.y, sub.width, height);
        render_entries(cmds, &entries, sub.x, sub.y, sub.width, sub.hover_index);

        if let Some(ref child) = sub.child {
            self.render_submenu_chain(cmds, child, top_idx);
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

    /// `(x, y, width, height)` of the dropdown panel for top-level index.
    fn dropdown_rect(&self, idx: usize) -> (f32, f32, f32, f32) {
        let x = self.label_metrics.get(idx).map_or(0.0, |m| m.0);
        let y = BAR_HEIGHT;
        let children = &self.items[idx].children;
        let w = calculate_dropdown_width(children);
        let h = dropdown_content_height(children) + DROPDOWN_VPAD * 2.0;
        (x, y, w, h)
    }

    // ─── Private: state transitions ────────────────────────────────────

    fn open_menu(&mut self, idx: usize) {
        self.open_index = Some(idx);
        self.dropdown_hover = None;
        self.open_submenu = None;
    }

    /// Activate an entry in the primary dropdown.
    fn activate_entry(&mut self, top_idx: usize, item_idx: usize) {
        let children = &self.items[top_idx].children;
        if let Some(act) = try_activate_entry(children, item_idx) {
            self.apply_activation(act);
            return;
        }
        // If it's a submenu, open it.
        if let Some(MenuBarEntry::SubMenu { children: sc, .. }) = children.get(item_idx) {
            let dd = self.dropdown_rect(top_idx);
            let sub_x = dd.0 + dd.2;
            let sub_y = dd.1 + DROPDOWN_VPAD + y_offset_for_index(children, item_idx);
            let sub_w = calculate_dropdown_width(sc);
            self.open_submenu = Some(Box::new(OpenSubmenu {
                parent_index: item_idx,
                x: sub_x,
                y: sub_y,
                width: sub_w,
                hover_index: None,
                child: None,
            }));
        }
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

    fn move_dropdown_hover(&mut self, dir: i32, top_idx: usize) {
        let children = &self.items[top_idx].children;
        self.dropdown_hover = next_selectable(children, self.dropdown_hover, dir);
    }
}

// ─── Free-standing helpers (no &self borrows) ──────────────────────────────

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
/// so we can pass `&items[top_idx].children` separately from `&mut sub`.
fn click_in_submenu_chain(
    root_children: &[MenuBarEntry],
    sub: &mut OpenSubmenu,
    mx: f32,
    my: f32,
) -> SubmenuClickResult {
    // Recurse into child first (deepest wins).
    if let Some(ref mut child) = sub.child {
        let r = click_in_submenu_chain(root_children, child, mx, my);
        match r {
            SubmenuClickResult::Miss => {} // Fall through to check this level.
            other => return other,
        }
    }

    let entries = resolve_submenu_entries(root_children, sub);
    let total_h = dropdown_content_height(&entries) + DROPDOWN_VPAD * 2.0;

    if mx >= sub.x && mx < sub.x + sub.width && my >= sub.y && my < sub.y + total_h {
        if let Some(idx) = item_index_at_y(&entries, my - sub.y - DROPDOWN_VPAD) {
            // Try to activate.
            if let Some(act) = try_activate_entry(&entries, idx) {
                return SubmenuClickResult::Activated(act);
            }
            // If it's a submenu, signal to open it.
            if let Some(MenuBarEntry::SubMenu { children: sc, .. }) = entries.get(idx) {
                return SubmenuClickResult::OpenChild {
                    idx,
                    child_x: sub.x + sub.width,
                    child_y: sub.y + DROPDOWN_VPAD + y_offset_for_index(&entries, idx),
                    child_width: calculate_dropdown_width(sc),
                };
            }
        }
        return SubmenuClickResult::ConsumedNoAction;
    }

    SubmenuClickResult::Miss
}

/// Hover inside submenu chain. Returns `true` if the point is inside.
fn hover_in_submenu_chain(
    root_children: &[MenuBarEntry],
    sub: &mut OpenSubmenu,
    mx: f32,
    my: f32,
) -> bool {
    // Recurse into child first.
    if let Some(ref mut child) = sub.child
        && hover_in_submenu_chain(root_children, child, mx, my) {
            sub.hover_index = None;
            return true;
        }

    let entries = resolve_submenu_entries(root_children, sub);
    let total_h = dropdown_content_height(&entries) + DROPDOWN_VPAD * 2.0;

    if mx >= sub.x && mx < sub.x + sub.width && my >= sub.y && my < sub.y + total_h {
        let new_hover = item_index_at_y(&entries, my - sub.y - DROPDOWN_VPAD);
        sub.hover_index = new_hover;

        // Open nested sub-submenu on hover.
        if let Some(hi) = new_hover {
            match entries.get(hi) {
                Some(MenuBarEntry::SubMenu { children: sc, .. }) => {
                    let already = sub.child.as_ref().is_some_and(|c| c.parent_index == hi);
                    if !already {
                        sub.child = Some(Box::new(OpenSubmenu {
                            parent_index: hi,
                            x: sub.x + sub.width,
                            y: sub.y + DROPDOWN_VPAD + y_offset_for_index(&entries, hi),
                            width: calculate_dropdown_width(sc),
                            hover_index: None,
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

/// Resolve the entries that an `OpenSubmenu` node refers to by walking the
/// item tree from `root_children` via the `parent_index` chain. We build the
/// path by collecting parent indices from the root submenu down to the
/// target, then walk `root_children` accordingly.
///
/// This clones the entry list because we cannot hold a borrow into the item
/// tree while also mutating submenu state.
fn resolve_submenu_entries(root_children: &[MenuBarEntry], sub: &OpenSubmenu) -> Vec<MenuBarEntry> {
    // We need the path of parent_index values from the top submenu down to `sub`.
    // Since OpenSubmenu forms a singly-linked list, we walk from `sub` upward
    // conceptually — but we only have downward links. Instead we collect indices
    // bottom-up and reverse. However, `sub` may be at any depth and we only have
    // a pointer to it, not to its ancestors. The simplest correct approach: walk
    // `root_children` using *just* `sub.parent_index` to get the direct children
    // of the entry at that index. But for nested submenus we need the full path.
    //
    // Since the caller always passes the correct node (the one whose entries we
    // want), we need to walk from root to this node. We do this by building a
    // path from the submenu chain.
    //
    // For the first-level submenu, parent_index indexes into root_children.
    // For deeper levels, we need the full chain. Since we can't walk up, the
    // resolve function simply returns the children of the entry at parent_index
    // in the *parent's* resolved entries. To handle arbitrary depth, we accept
    // the root_children and walk down using the node's parent_index as if it
    // only ever has one level. The caller must provide the correct root for
    // each level.
    //
    // Given the chain structure, the simplest design: the first OpenSubmenu's
    // parent_index indexes into root_children. Its child's parent_index indexes
    // into the first submenu's children, etc. So we can resolve by following the
    // chain from the top.

    // This function is called with a pointer to a *specific* node in the chain.
    // We need to find that node starting from the top. Since we don't have the
    // top pointer here, we simply look up sub.parent_index in root_children
    // for the first level. For deeper levels, the caller must pass the correct
    // root — but our architecture always passes the original root_children.
    //
    // Correct approach: walk from root using parent_index only.
    // For the FIRST submenu: root_children[parent_index].children
    // But `sub` might be deeper. We need to find the path.
    //
    // Since we can't walk up and we're given an arbitrary node, the only
    // reliable approach is to walk the chain from the very first submenu
    // node (which `sub` might be a descendant of). But we don't have the
    // root submenu pointer.
    //
    // Simplification: since `resolve_submenu_entries` is only called in
    // contexts where we have the correct item tree level, just look up
    // parent_index in root_children for the top level. For recursion, the
    // caller handles deeper levels via the chain structure.

    match root_children.get(sub.parent_index) {
        Some(MenuBarEntry::SubMenu { children, .. }) => children.clone(),
        _ => Vec::new(),
    }
}

/// Walk down to the deepest open submenu node (mutable).
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

/// Walk down to the deepest open submenu node (immutable).
fn find_deepest(sub: &OpenSubmenu) -> &OpenSubmenu {
    match sub.child {
        Some(ref child) => find_deepest(child),
        None => sub,
    }
}

fn dropdown_content_height(entries: &[MenuBarEntry]) -> f32 {
    entries
        .iter()
        .map(|e| match e {
            MenuBarEntry::Separator => SEPARATOR_HEIGHT,
            _ => ITEM_HEIGHT,
        })
        .sum()
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

/// Which entry index does a y-offset (relative to first item, after
/// vertical padding) land on? Returns `None` for separators.
fn item_index_at_y(entries: &[MenuBarEntry], rel_y: f32) -> Option<usize> {
    let mut cur = 0.0_f32;
    for (i, entry) in entries.iter().enumerate() {
        let h = match entry {
            MenuBarEntry::Separator => SEPARATOR_HEIGHT,
            _ => ITEM_HEIGHT,
        };
        if rel_y >= cur && rel_y < cur + h {
            if matches!(entry, MenuBarEntry::Separator) {
                return None;
            }
            return Some(i);
        }
        cur += h;
    }
    None
}

/// Y offset of the item at `target` index relative to the panel's content
/// origin (after vertical padding).
fn y_offset_for_index(entries: &[MenuBarEntry], target: usize) -> f32 {
    let mut offset = 0.0_f32;
    for (i, entry) in entries.iter().enumerate() {
        if i == target {
            return offset;
        }
        offset += match entry {
            MenuBarEntry::Separator => SEPARATOR_HEIGHT,
            _ => ITEM_HEIGHT,
        };
    }
    offset
}

/// Move hover in `direction` (+1 or -1), skipping separators and disabled items.
fn next_selectable(
    entries: &[MenuBarEntry],
    current: Option<usize>,
    direction: i32,
) -> Option<usize> {
    let count = entries.len();
    if count == 0 {
        return None;
    }

    let start = match current {
        Some(idx) => idx as i32 + direction,
        None => {
            if direction > 0 {
                0
            } else {
                count as i32 - 1
            }
        }
    };

    let mut pos = start;
    for _ in 0..count {
        if pos < 0 {
            pos = count as i32 - 1;
        } else if pos >= count as i32 {
            pos = 0;
        }

        let idx = pos as usize;
        let selectable = match entries.get(idx) {
            Some(MenuBarEntry::Action { enabled, .. }) => *enabled,
            Some(MenuBarEntry::Check { .. }) | Some(MenuBarEntry::SubMenu { .. }) => true,
            _ => false,
        };

        if selectable {
            return Some(idx);
        }
        pos += direction;
    }
    None
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
            && l.chars().next().map(|c| c.to_ascii_lowercase()) == Some(ch) {
                return Some(i);
            }
    }
    None
}

/// Render the shadow + background + border for a dropdown panel.
fn render_panel(cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
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
    panel_x: f32,
    panel_y: f32,
    panel_w: f32,
    hover: Option<usize>,
) {
    let mut cur_y = panel_y + DROPDOWN_VPAD;
    for (i, entry) in entries.iter().enumerate() {
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
                cur_y += SEPARATOR_HEIGHT;
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
                });

                if let Some(sc) = shortcut {
                    cmds.push(RenderCommand::Text {
                        x: panel_x + panel_w - DROPDOWN_HPAD
                            - estimate_text_width(sc, FONT_SIZE),
                        y: text_y,
                        text: sc.clone(),
                        color: DIM_TEXT_COLOR,
                        font_size: FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }

                cur_y += ITEM_HEIGHT;
            }

            MenuBarEntry::Check {
                label, checked, ..
            } => {
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
                });

                cur_y += ITEM_HEIGHT;
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
                });

                cur_y += ITEM_HEIGHT;
            }
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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

        let dd = bar.dropdown_rect(0);
        let item_y = dd.1 + DROPDOWN_VPAD + ITEM_HEIGHT / 2.0;
        bar.handle_mouse_event(&click(dd.0 + 40.0, item_y));

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

        let dd = bar.dropdown_rect(1);
        let item_y = dd.1 + DROPDOWN_VPAD + ITEM_HEIGHT + SEPARATOR_HEIGHT + ITEM_HEIGHT / 2.0;
        bar.handle_mouse_event(&click(dd.0 + 40.0, item_y));

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
        assert!(!cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::BoxShadow { .. })));
    }

    #[test]
    fn render_open_produces_dropdown() {
        let mut bar = make_bar();
        bar.handle_key_event(&alt_press(Key::F));
        let cmds = bar.render(800);
        assert!(cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::BoxShadow { .. })));
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

        let dd = bar.dropdown_rect(0);
        let item_y = dd.1 + DROPDOWN_VPAD + ITEM_HEIGHT + ITEM_HEIGHT / 2.0;
        bar.handle_mouse_event(&mouse_move(dd.0 + 40.0, item_y));
        assert_eq!(bar.dropdown_hover, Some(1)); // "Open"
    }
}

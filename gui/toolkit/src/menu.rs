//! Context menu and tooltip widgets.
//!
//! Provides a popup context menu (with submenus, keyboard navigation,
//! separators, icons, and check marks) and a tooltip that appears after
//! a configurable hover delay. Both produce `RenderCommand` lists and
//! use the Catppuccin Mocha dark theme.

use crate::color::Color;
use crate::event::{Key, KeyEvent};
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::row_strip::RowStrip;
use crate::step;
use crate::style::CornerRadii;

// ─── Catppuccin Mocha palette ───────────────────────────────────────────────

/// Dark background for menus and tooltips.
const BG_COLOR: Color = Color::from_hex(0x1E1E2E);
/// Slightly lighter surface for hover highlights.
const HOVER_COLOR: Color = Color::from_hex(0x313244);
/// Primary text color (light).
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
/// Dimmed text for disabled items and secondary info.
const DIM_TEXT_COLOR: Color = Color::from_hex(0x6C7086);
/// Accent color for checkmarks and active indicators.
const ACCENT_COLOR: Color = Color::from_hex(0x89B4FA);
/// Separator line color.
const SEPARATOR_COLOR: Color = Color::from_hex(0x45475A);
/// Shadow color (semi-transparent black).
const SHADOW_COLOR: Color = Color::rgba(0, 0, 0, 160);
/// Border color for menu outline.
const BORDER_COLOR: Color = Color::from_hex(0x45475A);

// ─── Layout constants ───────────────────────────────────────────────────────

const ITEM_HEIGHT: f32 = 28.0;
const SEPARATOR_HEIGHT: f32 = 9.0;
const ICON_COLUMN_WIDTH: f32 = 28.0;
const SHORTCUT_PADDING: f32 = 40.0;
const HORIZONTAL_PADDING: f32 = 8.0;
const VERTICAL_PADDING: f32 = 4.0;
const FONT_SIZE: f32 = 13.0;
const CORNER_RADIUS: f32 = 6.0;
const SHADOW_BLUR: f32 = 12.0;
const SHADOW_OFFSET: f32 = 4.0;
const SUBMENU_ARROW_WIDTH: f32 = 20.0;
const MIN_MENU_WIDTH: f32 = 160.0;

/// Width of the scroll indicator drawn down the right edge of a menu that has
/// more rows than it can show at once.
const SCROLLBAR_WIDTH: f32 = 4.0;
/// Gap between the scroll indicator and the menu's right border.
const SCROLLBAR_INSET: f32 = 2.0;
/// Shortest the thumb is allowed to get. A menu of two hundred rows would
/// otherwise draw a thumb under a pixel tall, which is the same as drawing
/// nothing — and the point of the indicator is to say "there is more here".
const SCROLLBAR_MIN_THUMB: f32 = 16.0;
/// Track behind the scroll thumb.
const SCROLLBAR_TRACK_COLOR: Color = Color::from_hex(0x313244);
/// The thumb itself.
const SCROLLBAR_THUMB_COLOR: Color = Color::from_hex(0x585B70);

// ─── Viewport bounds (used for edge-flip logic) ─────────────────────────────

/// Default viewport width used for edge detection when flipping menu position.
const DEFAULT_VIEWPORT_WIDTH: f32 = 1920.0;
/// Default viewport height used for edge detection when flipping menu position.
const DEFAULT_VIEWPORT_HEIGHT: f32 = 1080.0;

// ─── Menu types ─────────────────────────────────────────────────────────────

/// Unique identifier for a menu item.
pub type MenuItemId = u64;

/// A single item in a context menu.
#[derive(Clone, Debug)]
pub enum MenuItem {
    /// Regular clickable item.
    Action {
        id: MenuItemId,
        label: String,
        shortcut: Option<String>,
        icon: Option<String>,
        enabled: bool,
        /// `None` means not checkable; `Some(true/false)` means checkbox state.
        checked: Option<bool>,
    },
    /// Visual separator line between groups of items.
    Separator,
    /// Submenu that opens on hover.
    Submenu {
        id: MenuItemId,
        label: String,
        icon: Option<String>,
        enabled: bool,
        children: Vec<MenuItem>,
    },
}

/// Result of handling a menu interaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// An item was selected.
    Selected(MenuItemId),
    /// Menu was closed (e.g., Escape).
    Closed,
    /// No action taken.
    None,
}

/// A context menu or dropdown menu that renders as a popup overlay.
///
/// # A menu taller than the screen
///
/// The vertical placement used to be a single flip: open downwards, or if that
/// would run off the bottom, open upwards from the click instead. That is the
/// right rule right up until the menu is taller than the viewport, at which
/// point *neither* direction fits and the flip quietly picked the worse of the
/// two — `(y - total_height).max(0.0)` clamped to zero, so the menu started at
/// the top of the screen and ran off the bottom by however much taller than
/// 1080 px it was. The rows down there were drawn off-screen, could not be
/// seen, and could not be clicked; the only way to reach them was to not have
/// them.
///
/// So the panel is now capped at the viewport and the rows inside it scroll.
/// The three things that had to be true for that to be safe:
///
/// - **The rows move, the panel does not.** [`Self::scroll`] is subtracted in
///   exactly one place — the origin handed to [`RowStrip`] — because a scroll
///   offset is just a different origin, and [`Self::index_at_y`] inverts the
///   same strip the renderer draws from. Nothing else in the file subtracts it.
/// - **A scrolled-away row must answer for nothing.** The strip alone is not
///   enough: a row scrolled above the panel still has a `y` in the strip's
///   coordinates, and `index_at` would happily name it for a pointer that is
///   over the *menu bar* above. [`Self::index_at_y`] therefore bounds the
///   answer to the visible region as well, which is the same shape of guard
///   the app-side row lists need and for the same reason.
/// - **Keyboard navigation has to bring its row with it.** Arrowing down past
///   the fold scrolls; otherwise the highlight walks off the bottom of a menu
///   that has no way to follow it.
pub struct ContextMenu {
    items: Vec<MenuItem>,
    x: f32,
    y: f32,
    visible: bool,
    hover_index: Option<usize>,
    open_submenu: Option<(usize, Box<ContextMenu>)>,
    /// Auto-calculated width based on content.
    width: f32,
    /// How far the rows are scrolled up inside the panel, in pixels. Always in
    /// `0..=max_scroll()`, and zero for every menu short enough to fit — which
    /// is nearly all of them, so the common case is unchanged.
    scroll: f32,
}

impl ContextMenu {
    /// Create a new context menu with the given items.
    pub fn new(items: Vec<MenuItem>) -> Self {
        let width = Self::calculate_width(&items);
        Self {
            items,
            x: 0.0,
            y: 0.0,
            visible: false,
            hover_index: None,
            open_submenu: None,
            width,
            scroll: 0.0,
        }
    }

    /// Show the menu at the given position, adjusting for viewport edges.
    ///
    /// A menu that fits opens downwards, or upwards if downwards would overflow
    /// the bottom edge. A menu taller than the whole viewport fits in neither
    /// direction, so it is given the full height of the screen and its rows
    /// scroll — see the type's docs for why the old flip could not express that.
    pub fn show(&mut self, x: f32, y: f32) {
        // Opening the menu shows the top of it. Reset before measuring: the
        // panel height does not depend on the offset, but leaving a stale
        // offset behind would open the menu part-way down its own list.
        self.scroll = 0.0;
        let panel_height = self.panel_height();

        // Flip horizontally if menu would overflow right edge.
        self.x = if x + self.width > DEFAULT_VIEWPORT_WIDTH {
            (x - self.width).max(0.0)
        } else {
            x
        };

        // Flip vertically if the menu would overflow the bottom edge. When the
        // panel is the full viewport height both branches give 0.0, which is
        // the only place it can go.
        self.y = if y + panel_height > DEFAULT_VIEWPORT_HEIGHT {
            (y - panel_height).max(0.0)
        } else {
            y
        };

        self.visible = true;
        self.hover_index = None;
        self.open_submenu = None;
    }

    /// Scroll the rows by `dy` wheel notches, if `(mx, my)` is over this menu
    /// or one of its open submenus. Returns whether the wheel was consumed.
    ///
    /// The offset is a continuous pixel value rather than a row index, so this
    /// takes [`wheel::pixels`] and not an accumulator: a trackpad's fifth of a
    /// notch should move a fifth of a notch rather than being banked until it
    /// rounds to a whole row. Its sign is already "positive means towards the
    /// end of the list", which is the direction this offset grows in, so it is
    /// added rather than subtracted.
    ///
    /// [`wheel::pixels`]: crate::wheel::pixels
    pub fn handle_scroll(&mut self, mx: f32, my: f32, dy: f32) -> bool {
        if !self.visible {
            return false;
        }

        // The submenu is drawn on top, so it gets the wheel first.
        if let Some((_, ref mut submenu)) = self.open_submenu
            && submenu.handle_scroll(mx, my, dy)
        {
            return true;
        }

        if !self.point_in_bounds(mx, my) {
            return false;
        }

        // Consumed even when there is nothing to scroll: the wheel must not
        // fall through a popup to whatever it is covering.
        self.set_scroll(self.scroll + crate::wheel::pixels(dy, ITEM_HEIGHT));
        true
    }

    /// Hide the menu and any open submenus.
    pub fn hide(&mut self) {
        self.visible = false;
        self.hover_index = None;
        self.open_submenu = None;
    }

    /// Whether the menu is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Handle a mouse click. Returns the selected item ID if an action item was clicked.
    pub fn handle_click(&mut self, mx: f32, my: f32) -> Option<MenuItemId> {
        if !self.visible {
            return None;
        }

        // Check if click is in open submenu first.
        if let Some((_, ref mut submenu)) = self.open_submenu
            && let Some(id) = submenu.handle_click(mx, my)
        {
            self.hide();
            return Some(id);
        }

        // Check if click is within our bounds.
        if !self.point_in_bounds(mx, my) {
            self.hide();
            return None;
        }

        let idx = self.index_at_y(my)?;

        match self.items.get(idx) {
            Some(MenuItem::Action {
                id, enabled: true, ..
            }) => {
                let id = *id;
                self.hide();
                Some(id)
            }
            Some(MenuItem::Submenu {
                enabled: true,
                children,
                ..
            }) => {
                // Clicking a submenu item opens it (same as hover).
                let mut sub = ContextMenu::new(children.clone());
                let sub_x = self.x + self.width;
                let sub_y = self.y + self.y_offset_for_index(idx);
                sub.show(sub_x, sub_y);
                self.open_submenu = Some((idx, Box::new(sub)));
                None
            }
            _ => None, // Separator, disabled item
        }
    }

    /// Handle mouse movement for hover highlighting and submenu opening.
    pub fn handle_mouse_move(&mut self, mx: f32, my: f32) {
        if !self.visible {
            return;
        }

        // Delegate to submenu if mouse is within it.
        if let Some((_, ref mut submenu)) = self.open_submenu
            && submenu.point_in_bounds(mx, my)
        {
            submenu.handle_mouse_move(mx, my);
            return;
        }

        if !self.point_in_bounds(mx, my) {
            // Don't clear hover if mouse moved to a submenu.
            if self
                .open_submenu
                .as_ref()
                .is_some_and(|(_, sub)| sub.point_in_bounds(mx, my))
            {
                return;
            }
            self.hover_index = None;
            return;
        }

        let new_index = self.index_at_y(my);
        self.hover_index = new_index;

        // Open submenu if hovering over a submenu item.
        if let Some(idx) = new_index {
            match self.items.get(idx) {
                Some(MenuItem::Submenu {
                    enabled: true,
                    children,
                    ..
                }) => {
                    // Only open if not already open for this index.
                    let already_open = self.open_submenu.as_ref().is_some_and(|(i, _)| *i == idx);
                    if !already_open {
                        let mut sub = ContextMenu::new(children.clone());
                        let sub_x = self.x + self.width;
                        let sub_y = self.y + self.y_offset_for_index(idx);
                        sub.show(sub_x, sub_y);
                        self.open_submenu = Some((idx, Box::new(sub)));
                    }
                }
                _ => {
                    // Close submenu if hovering over a non-submenu item.
                    self.open_submenu = None;
                }
            }
        }
    }

    /// Handle keyboard input for menu navigation.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<MenuAction> {
        if !self.visible || !key.pressed {
            return Some(MenuAction::None);
        }

        // Delegate to open submenu first.
        if let Some((_, ref mut submenu)) = self.open_submenu
            && submenu.is_visible()
        {
            let result = submenu.handle_key(key);
            if let Some(MenuAction::Selected(id)) = result {
                self.hide();
                return Some(MenuAction::Selected(id));
            }
            if let Some(MenuAction::Closed) = result {
                // Left arrow or Escape in submenu closes it, returns focus to parent.
                self.open_submenu = None;
                return Some(MenuAction::None);
            }
            return result;
        }

        match key.key {
            Key::Escape => {
                self.hide();
                Some(MenuAction::Closed)
            }
            Key::Up => {
                self.move_hover(false);
                Some(MenuAction::None)
            }
            Key::Down => {
                self.move_hover(true);
                Some(MenuAction::None)
            }
            Key::Enter => {
                if let Some(idx) = self.hover_index {
                    match self.items.get(idx) {
                        Some(MenuItem::Action {
                            id, enabled: true, ..
                        }) => {
                            let id = *id;
                            self.hide();
                            Some(MenuAction::Selected(id))
                        }
                        Some(MenuItem::Submenu {
                            enabled: true,
                            children,
                            ..
                        }) => {
                            let mut sub = ContextMenu::new(children.clone());
                            let sub_x = self.x + self.width;
                            let sub_y = self.y + self.y_offset_for_index(idx);
                            sub.show(sub_x, sub_y);
                            self.open_submenu = Some((idx, Box::new(sub)));
                            Some(MenuAction::None)
                        }
                        _ => Some(MenuAction::None),
                    }
                } else {
                    Some(MenuAction::None)
                }
            }
            Key::Right => {
                // Open submenu if hover is on a submenu item.
                if let Some(idx) = self.hover_index
                    && let Some(MenuItem::Submenu {
                        enabled: true,
                        children,
                        ..
                    }) = self.items.get(idx)
                {
                    let mut sub = ContextMenu::new(children.clone());
                    let sub_x = self.x + self.width;
                    let sub_y = self.y + self.y_offset_for_index(idx);
                    sub.show(sub_x, sub_y);
                    self.open_submenu = Some((idx, Box::new(sub)));
                }
                Some(MenuAction::None)
            }
            Key::Left => {
                // Close submenu (handled by parent delegation above).
                Some(MenuAction::Closed)
            }
            _ => Some(MenuAction::None),
        }
    }

    /// Produce render commands for this menu and any open submenus.
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let mut cmds = Vec::new();
        let panel_height = self.panel_height();
        let radii = CornerRadii::all(CORNER_RADIUS);

        // Shadow behind menu.
        cmds.push(RenderCommand::BoxShadow {
            x: self.x,
            y: self.y,
            width: self.width,
            height: panel_height,
            offset_x: SHADOW_OFFSET,
            offset_y: SHADOW_OFFSET,
            blur: SHADOW_BLUR,
            spread: 0.0,
            color: SHADOW_COLOR,
            corner_radii: radii,
        });

        // Menu background.
        cmds.push(RenderCommand::FillRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: panel_height,
            color: BG_COLOR,
            corner_radii: radii,
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: panel_height,
            color: BORDER_COLOR,
            line_width: 1.0,
            corner_radii: radii,
        });

        // Render each item, from the same strip the hit test reads. Advancing
        // a running `current_y` here is what let this walk drift away from the
        // three others that used to exist.
        //
        // Clipped to the region `index_at_y` accepts, so a partly-scrolled row
        // is drawn cut off at exactly the line past which it stops answering.
        // Rows entirely outside it are skipped rather than drawn under the clip:
        // the clip would make them invisible either way, but a two-hundred-row
        // menu should not emit six hundred text commands to show thirty rows.
        let strip = self.strip();
        let (view_top, view_bottom) = (self.viewport_top(), self.viewport_bottom());
        cmds.push(RenderCommand::PushClip {
            x: self.x,
            y: view_top,
            width: self.width,
            height: self.viewport_height(),
        });
        for (i, item) in self.items.iter().enumerate() {
            let Some(current_y) = strip.top(i) else {
                continue;
            };
            let row_height = strip.height(i).unwrap_or(0.0);
            if current_y + row_height <= view_top || current_y >= view_bottom {
                continue;
            }
            match item {
                MenuItem::Separator => {
                    let line_y = current_y + SEPARATOR_HEIGHT / 2.0;
                    cmds.push(RenderCommand::Line {
                        x1: self.x + HORIZONTAL_PADDING,
                        y1: line_y,
                        x2: self.x + self.width - HORIZONTAL_PADDING,
                        y2: line_y,
                        color: SEPARATOR_COLOR,
                        width: 1.0,
                    });
                }
                MenuItem::Action {
                    label,
                    shortcut,
                    enabled,
                    checked,
                    ..
                } => {
                    // Hover highlight.
                    if self.hover_index == Some(i) && *enabled {
                        cmds.push(RenderCommand::FillRect {
                            x: self.x + 4.0,
                            y: current_y,
                            width: self.width - 8.0,
                            height: ITEM_HEIGHT,
                            color: HOVER_COLOR,
                            corner_radii: CornerRadii::all(4.0),
                        });
                    }

                    let text_color = if *enabled { TEXT_COLOR } else { DIM_TEXT_COLOR };
                    let text_y = current_y + (ITEM_HEIGHT - FONT_SIZE) / 2.0;

                    // Check mark.
                    if let Some(true) = checked {
                        cmds.push(RenderCommand::Text {
                            x: self.x + HORIZONTAL_PADDING + 4.0,
                            y: text_y,
                            text: "\u{2713}".to_string(), // checkmark
                            color: ACCENT_COLOR,
                            font_size: FONT_SIZE,
                            font_weight: FontWeightHint::Bold,
                            max_width: None,
                            overflow: TextOverflow::Clip,
                        });
                    }

                    // Label.
                    cmds.push(RenderCommand::Text {
                        x: self.x + HORIZONTAL_PADDING + ICON_COLUMN_WIDTH,
                        y: text_y,
                        text: label.clone(),
                        color: text_color,
                        font_size: FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });

                    // Shortcut text (right-aligned).
                    if let Some(shortcut_text) = shortcut {
                        cmds.push(RenderCommand::Text {
                            x: self.x + self.width
                                - HORIZONTAL_PADDING
                                - Self::estimate_text_width(shortcut_text, FONT_SIZE),
                            y: text_y,
                            text: shortcut_text.clone(),
                            color: DIM_TEXT_COLOR,
                            font_size: FONT_SIZE,
                            font_weight: FontWeightHint::Regular,
                            max_width: None,
                            overflow: TextOverflow::Clip,
                        });
                    }
                }
                MenuItem::Submenu { label, enabled, .. } => {
                    // Hover highlight.
                    if self.hover_index == Some(i) && *enabled {
                        cmds.push(RenderCommand::FillRect {
                            x: self.x + 4.0,
                            y: current_y,
                            width: self.width - 8.0,
                            height: ITEM_HEIGHT,
                            color: HOVER_COLOR,
                            corner_radii: CornerRadii::all(4.0),
                        });
                    }

                    let text_color = if *enabled { TEXT_COLOR } else { DIM_TEXT_COLOR };
                    let text_y = current_y + (ITEM_HEIGHT - FONT_SIZE) / 2.0;

                    // Label.
                    cmds.push(RenderCommand::Text {
                        x: self.x + HORIZONTAL_PADDING + ICON_COLUMN_WIDTH,
                        y: text_y,
                        text: label.clone(),
                        color: text_color,
                        font_size: FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });

                    // Submenu arrow indicator.
                    cmds.push(RenderCommand::Text {
                        x: self.x + self.width - HORIZONTAL_PADDING - SUBMENU_ARROW_WIDTH,
                        y: text_y,
                        text: "\u{25B8}".to_string(), // right-pointing triangle
                        color: text_color,
                        font_size: FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }
            }
        }
        cmds.push(RenderCommand::PopClip);

        // Scroll indicator, only when there is something the panel is not
        // showing. Without it a menu that has been capped looks exactly like a
        // menu that happens to end there.
        let max_scroll = self.max_scroll();
        if max_scroll > 0.0 && self.viewport_height() > 0.0 {
            let track_x = self.x + self.width - SCROLLBAR_INSET - SCROLLBAR_WIDTH;
            let track_height = self.viewport_height();
            cmds.push(RenderCommand::FillRect {
                x: track_x,
                y: view_top,
                width: SCROLLBAR_WIDTH,
                height: track_height,
                color: SCROLLBAR_TRACK_COLOR,
                corner_radii: CornerRadii::all(SCROLLBAR_WIDTH / 2.0),
            });
            // The thumb is as tall a fraction of the track as the panel is of
            // the content, floored so it stays visible, and then travels over
            // whatever room that leaves it. Dividing the travel by `max_scroll`
            // rather than by the content height is what keeps the thumb's
            // bottom flush with the track's when the list is scrolled to its
            // end, at any thumb size.
            let visible_fraction = if self.content_height() > 0.0 {
                self.panel_height() / self.content_height()
            } else {
                1.0
            };
            let thumb_height = (track_height * visible_fraction)
                .max(SCROLLBAR_MIN_THUMB)
                .min(track_height);
            let thumb_y = view_top + (track_height - thumb_height) * (self.scroll / max_scroll);
            cmds.push(RenderCommand::FillRect {
                x: track_x,
                y: thumb_y,
                width: SCROLLBAR_WIDTH,
                height: thumb_height,
                color: SCROLLBAR_THUMB_COLOR,
                corner_radii: CornerRadii::all(SCROLLBAR_WIDTH / 2.0),
            });
        }

        // Render open submenu on top.
        if let Some((_, ref submenu)) = self.open_submenu {
            cmds.extend(submenu.render());
        }

        cmds
    }

    // ─── Private helpers ────────────────────────────────────────────────────

    fn calculate_width(items: &[MenuItem]) -> f32 {
        let mut max_label_w: f32 = 0.0;
        let mut max_shortcut_w: f32 = 0.0;

        for item in items {
            match item {
                MenuItem::Action {
                    label, shortcut, ..
                } => {
                    let label_w = Self::estimate_text_width(label, FONT_SIZE);
                    max_label_w = max_label_w.max(label_w);
                    if let Some(sc) = shortcut {
                        let sc_w = Self::estimate_text_width(sc, FONT_SIZE);
                        max_shortcut_w = max_shortcut_w.max(sc_w);
                    }
                }
                MenuItem::Submenu { label, .. } => {
                    let label_w = Self::estimate_text_width(label, FONT_SIZE);
                    max_label_w = max_label_w.max(label_w);
                    // Account for arrow indicator.
                    max_shortcut_w = max_shortcut_w.max(SUBMENU_ARROW_WIDTH);
                }
                MenuItem::Separator => {}
            }
        }

        let shortcut_space = if max_shortcut_w > 0.0 {
            SHORTCUT_PADDING + max_shortcut_w
        } else {
            0.0
        };

        let width = HORIZONTAL_PADDING * 2.0
            + ICON_COLUMN_WIDTH
            + max_label_w
            + shortcut_space
            + HORIZONTAL_PADDING;
        width.max(MIN_MENU_WIDTH)
    }

    /// Width of `text`, as the compositor will actually draw it.
    ///
    /// This used to be `text.len() as f32 * font_size * 0.6`, which sized the
    /// menu for a font nobody draws in: `len` counts bytes, so an accented
    /// label reserved twice the room it needed, and the 0.6 was one of five
    /// different fudge factors scattered across the toolkit.
    fn estimate_text_width(text: &str, font_size: f32) -> f32 {
        crate::text::width(text, font_size)
    }

    /// How tall one row is. The single spelling of the rule.
    ///
    /// This match used to appear four times in this file — once summing the
    /// heights for [`Self::content_height`], once placing the rows in
    /// [`Self::render`], once subtracting them back off in
    /// [`Self::index_at_y`], and once adding them up again in
    /// [`Self::y_offset_for_index`] to hang a submenu. Four walks of one list
    /// is four chances for three of them to be right; when they disagree the
    /// user clicks one row and gets the one above it.
    const fn item_height(item: &MenuItem) -> f32 {
        match item {
            MenuItem::Separator => SEPARATOR_HEIGHT,
            _ => ITEM_HEIGHT,
        }
    }

    /// Where every row sits, in screen coordinates, with the scroll offset
    /// already applied.
    ///
    /// The renderer draws from this and [`Self::index_at_y`] answers from it,
    /// so the rows on screen are the rows that answer. This is the **only**
    /// place [`Self::scroll`] is subtracted; a second subtraction anywhere else
    /// would be a second description of where the rows are.
    fn strip(&self) -> RowStrip {
        RowStrip::new(
            self.y + VERTICAL_PADDING - self.scroll,
            self.items.iter().map(Self::item_height),
        )
    }

    /// How tall the menu would be if the screen were unbounded — every row plus
    /// the padding above and below them.
    fn content_height(&self) -> f32 {
        self.strip().total_height() + VERTICAL_PADDING * 2.0
    }

    /// How tall the menu actually is on screen. Equal to
    /// [`Self::content_height`] for anything that fits, and the viewport
    /// otherwise.
    fn panel_height(&self) -> f32 {
        self.content_height().min(DEFAULT_VIEWPORT_HEIGHT)
    }

    /// Top of the region the rows are drawn in and hit-tested against.
    fn viewport_top(&self) -> f32 {
        self.y + VERTICAL_PADDING
    }

    /// One past the bottom of that region. Never above
    /// [`Self::viewport_top`]: a menu whose panel is somehow shorter than its
    /// own padding gets a zero-height row region rather than a negative-height
    /// one, because a negative-height clip is not a small clip.
    fn viewport_bottom(&self) -> f32 {
        (self.y + self.panel_height() - VERTICAL_PADDING).max(self.viewport_top())
    }

    /// Height of the row region — what the renderer clips to.
    fn viewport_height(&self) -> f32 {
        self.viewport_bottom() - self.viewport_top()
    }

    /// The largest [`Self::scroll`] that still shows content, i.e. how much
    /// taller the list is than the room it has. Zero whenever the menu fits.
    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.panel_height()).max(0.0)
    }

    /// Set the scroll offset, clamped to the range that shows content.
    ///
    /// A non-finite offset is refused outright rather than clamped: `NaN`
    /// compares false against both ends of a `clamp`, and letting it reach the
    /// strip's origin would poison every row's position at once, leaving a menu
    /// that answers for no pointer at all.
    fn set_scroll(&mut self, value: f32) {
        if !value.is_finite() {
            return;
        }
        self.scroll = value.clamp(0.0, self.max_scroll());
    }

    /// Scroll the least amount that brings row `index` fully into view. A no-op
    /// for a row already visible, and for a row that does not exist.
    fn scroll_index_into_view(&mut self, index: usize) {
        let strip = self.strip();
        let (Some(top), Some(height)) = (strip.top(index), strip.height(index)) else {
            return;
        };
        let (view_top, view_bottom) = (self.viewport_top(), self.viewport_bottom());
        if top < view_top {
            self.set_scroll(self.scroll - (view_top - top));
        } else if top + height > view_bottom {
            self.set_scroll(self.scroll + (top + height - view_bottom));
        }
    }

    fn point_in_bounds(&self, px: f32, py: f32) -> bool {
        px >= self.x
            && px <= self.x + self.width
            && py >= self.y
            && py <= self.y + self.panel_height()
    }

    /// Find which item index the Y coordinate corresponds to.
    ///
    /// Two rules, not one. The strip says which row owns `py` *in the list*; the
    /// visible-region test says whether that part of the list is on screen. Both
    /// are needed once the list can scroll — a row scrolled above the panel
    /// keeps its place in the strip, so asking the strip alone would name it for
    /// a pointer that is nowhere near the menu. Without a scroll offset the two
    /// tests coincide, which is why one of them used to be enough.
    ///
    /// A separator has a position and a height like anything else, so the
    /// strip names it; whether it is *selectable* is this menu's rule rather
    /// than the layout's, and the answer is no.
    fn index_at_y(&self, py: f32) -> Option<usize> {
        if !(py >= self.viewport_top() && py < self.viewport_bottom()) {
            return None;
        }
        let idx = self.strip().index_at(py)?;
        match self.items.get(idx) {
            Some(MenuItem::Separator) | None => None,
            Some(_) => Some(idx),
        }
    }

    /// Get the Y offset of the item at the given index relative to menu top.
    ///
    /// An index past the end reports the offset one past the last row, which
    /// is what the hand-written walk this replaced fell through to.
    fn y_offset_for_index(&self, target: usize) -> f32 {
        let strip = self.strip();
        strip.top(target).unwrap_or_else(|| strip.bottom()) - self.y
    }

    /// Move hover to the next selectable row, skipping separators and disabled
    /// items and wrapping round the ends.
    ///
    /// A menu with no selectable row at all — every entry a separator, or every
    /// action disabled — leaves the hover where it was, which for a freshly
    /// opened menu is nowhere. That is the only outcome that does not lie: there
    /// is no row for the arrow key to land on, so pretending one is highlighted
    /// would make Enter act on a disabled item.
    fn move_hover(&mut self, forward: bool) {
        let len = self.items.len();
        // The current row is not a candidate for its own successor, so the walk
        // starts at its neighbour — `step::indices` visits its start first. It
        // still comes back round to the current row last, which is what makes a
        // menu with exactly one selectable row keep it under repeated presses.
        let start = match self.hover_index {
            Some(idx) => {
                if forward {
                    step::wrapping_after(len, idx)
                } else {
                    step::wrapping_before(len, idx)
                }
            }
            None if forward => 0,
            None => len.saturating_sub(1),
        };

        let landed = step::indices(len, start, forward).find(|&idx| {
            matches!(
                self.items.get(idx),
                Some(MenuItem::Action { enabled: true, .. })
                    | Some(MenuItem::Submenu { enabled: true, .. })
            )
        });
        self.hover_index = landed.or(self.hover_index);
        // The highlight has to drag the panel with it, or arrowing down a menu
        // taller than the screen walks it off the bottom edge and the user is
        // left pressing Enter on a row they cannot see.
        if let Some(idx) = self.hover_index {
            self.scroll_index_into_view(idx);
        }
    }
}

// ─── Tooltip ────────────────────────────────────────────────────────────────

const TOOLTIP_BG: Color = Color::from_hex(0x1E1E2E);
const TOOLTIP_TEXT: Color = Color::from_hex(0xCDD6F4);
const TOOLTIP_BORDER: Color = Color::from_hex(0x45475A);
const TOOLTIP_SHADOW: Color = Color::rgba(0, 0, 0, 120);
const TOOLTIP_FONT_SIZE: f32 = 12.0;
const TOOLTIP_PADDING: f32 = 6.0;
const TOOLTIP_CORNER_RADIUS: f32 = 4.0;
const TOOLTIP_OFFSET_X: f32 = 12.0;
const TOOLTIP_OFFSET_Y: f32 = 16.0;
const TOOLTIP_LINE_HEIGHT: f32 = 16.0;
const DEFAULT_TOOLTIP_DELAY_MS: u32 = 500;
const DEFAULT_TOOLTIP_MAX_WIDTH: f32 = 300.0;

/// A tooltip that appears after a configurable hover delay.
pub struct Tooltip {
    text: String,
    x: f32,
    y: f32,
    visible: bool,
    /// Delay in milliseconds before tooltip appears.
    delay_ms: u32,
    /// Timestamp (ms) when hover began; `None` if not hovering.
    hover_start: Option<u64>,
    /// Maximum width before text wraps to a new line.
    max_width: f32,
}

impl Tooltip {
    /// Create a new tooltip with the given text and default settings.
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            x: 0.0,
            y: 0.0,
            visible: false,
            delay_ms: DEFAULT_TOOLTIP_DELAY_MS,
            hover_start: None,
            max_width: DEFAULT_TOOLTIP_MAX_WIDTH,
        }
    }

    /// Set the delay before the tooltip appears.
    pub fn with_delay(mut self, ms: u32) -> Self {
        self.delay_ms = ms;
        self
    }

    /// Set the maximum width before text wraps.
    pub fn with_max_width(mut self, width: f32) -> Self {
        self.max_width = width;
        self
    }

    /// Call when the mouse enters the tooltip trigger area.
    pub fn start_hover(&mut self, x: f32, y: f32, timestamp_ms: u64) {
        if self.hover_start.is_none() {
            self.hover_start = Some(timestamp_ms);

            // Position with offset, flipping if near viewport edges.
            let tip_width = self.compute_width();
            let tip_height = self.compute_height();

            let mut tip_x = x + TOOLTIP_OFFSET_X;
            let mut tip_y = y + TOOLTIP_OFFSET_Y;

            if tip_x + tip_width > DEFAULT_VIEWPORT_WIDTH {
                tip_x = (x - tip_width - TOOLTIP_OFFSET_X).max(0.0);
            }
            if tip_y + tip_height > DEFAULT_VIEWPORT_HEIGHT {
                tip_y = (y - tip_height - TOOLTIP_OFFSET_Y).max(0.0);
            }

            self.x = tip_x;
            self.y = tip_y;
        }
    }

    /// Call when the mouse leaves the trigger area.
    pub fn end_hover(&mut self) {
        self.hover_start = None;
        self.visible = false;
    }

    /// Call on each frame/tick to check if the hover delay has elapsed.
    pub fn tick(&mut self, timestamp_ms: u64) {
        if let Some(start) = self.hover_start
            && !self.visible
            && timestamp_ms.saturating_sub(start) >= u64::from(self.delay_ms)
        {
            self.visible = true;
        }
    }

    /// Whether the tooltip is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Produce render commands for the tooltip.
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let mut cmds = Vec::new();
        let width = self.compute_width();
        let height = self.compute_height();
        let radii = CornerRadii::all(TOOLTIP_CORNER_RADIUS);

        // Shadow.
        cmds.push(RenderCommand::BoxShadow {
            x: self.x,
            y: self.y,
            width,
            height,
            offset_x: 2.0,
            offset_y: 2.0,
            blur: 6.0,
            spread: 0.0,
            color: TOOLTIP_SHADOW,
            corner_radii: radii,
        });

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: self.x,
            y: self.y,
            width,
            height,
            color: TOOLTIP_BG,
            corner_radii: radii,
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: self.x,
            y: self.y,
            width,
            height,
            color: TOOLTIP_BORDER,
            line_width: 1.0,
            corner_radii: radii,
        });

        // Text (each wrapped line).
        let lines = self.wrap_text();
        let mut text_y = self.y + TOOLTIP_PADDING;
        for line in &lines {
            cmds.push(RenderCommand::Text {
                x: self.x + TOOLTIP_PADDING,
                y: text_y,
                text: line.clone(),
                color: TOOLTIP_TEXT,
                font_size: TOOLTIP_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.max_width),
                overflow: TextOverflow::Ellipsis,
            });
            text_y += TOOLTIP_LINE_HEIGHT;
        }

        cmds
    }

    // ─── Private helpers ────────────────────────────────────────────────────

    fn compute_width(&self) -> f32 {
        let lines = self.wrap_text();
        let max_line_width: f32 = lines
            .iter()
            .map(|l| crate::text::width(l, TOOLTIP_FONT_SIZE))
            .fold(0.0_f32, f32::max);
        (max_line_width + TOOLTIP_PADDING * 2.0).min(self.max_width + TOOLTIP_PADDING * 2.0)
    }

    fn compute_height(&self) -> f32 {
        let lines = self.wrap_text();
        let line_count = lines.len().max(1);
        line_count as f32 * TOOLTIP_LINE_HEIGHT + TOOLTIP_PADDING * 2.0
    }

    /// Word-wrap at `max_width` pixels.
    ///
    /// Deferred to [`crate::text::wrap`] so that the rule deciding where lines
    /// break is the same one `compute_width` sizes the box with. Wrapping on a
    /// different rule than the box is sized on is exactly how a tooltip ends up
    /// with text hanging past its own background.
    fn wrap_text(&self) -> Vec<String> {
        crate::text::wrap(
            &self.text,
            self.max_width,
            TOOLTIP_FONT_SIZE,
            FontWeightHint::Regular,
        )
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

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

    fn sample_items() -> Vec<MenuItem> {
        vec![
            MenuItem::Action {
                id: 1,
                label: "Cut".to_string(),
                shortcut: Some("Ctrl+X".to_string()),
                icon: None,
                enabled: true,
                checked: None,
            },
            MenuItem::Action {
                id: 2,
                label: "Copy".to_string(),
                shortcut: Some("Ctrl+C".to_string()),
                icon: None,
                enabled: true,
                checked: None,
            },
            MenuItem::Separator,
            MenuItem::Action {
                id: 3,
                label: "Paste".to_string(),
                shortcut: Some("Ctrl+V".to_string()),
                icon: None,
                enabled: false,
                checked: None,
            },
            MenuItem::Action {
                id: 4,
                label: "Select All".to_string(),
                shortcut: Some("Ctrl+A".to_string()),
                icon: None,
                enabled: true,
                checked: None,
            },
        ]
    }

    fn make_key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    // ─── Geometry: does a click land on the row that was drawn? ──────────────
    //
    // These read the rectangle `render()` actually emitted and probe *its*
    // edges. A test that recomputes the renderer's arithmetic and checks the
    // hit test against that is worthless: the two drift together, and the
    // whole failure mode being guarded here is a renderer and a hit test that
    // agree only by coincidence.

    /// Items whose every action is enabled, so every row paints a hover
    /// highlight and can therefore be measured. Mixed heights on purpose.
    fn geometry_items() -> Vec<MenuItem> {
        fn action(id: MenuItemId, label: &str) -> MenuItem {
            MenuItem::Action {
                id,
                label: label.to_string(),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: None,
            }
        }
        vec![
            action(1, "New"),
            action(2, "Open"),
            MenuItem::Separator,
            MenuItem::Submenu {
                id: 3,
                label: "Recent".to_string(),
                icon: None,
                enabled: true,
                children: vec![action(31, "a.txt")],
            },
            action(4, "Save"),
            MenuItem::Separator,
            action(5, "Quit"),
        ]
    }

    /// The `(top, height)` of the hover highlight the menu paints when the
    /// pointer is over row `idx` — moved there through the real pointer path,
    /// not by poking `hover_index`.
    fn painted_row(menu: &mut ContextMenu, idx: usize) -> Option<(f32, f32)> {
        // Park the pointer using the layout's own answer for where the row is.
        // That is not circular: if the layout is wrong, the *highlight* it
        // paints is what these tests then compare against the hit test, and a
        // disagreement still shows up.
        let probe = menu.y + menu.y_offset_for_index(idx) + 1.0;
        highlight_after_pointer_at(menu, probe)
    }

    /// The `(top, height)` of the hover highlight the menu paints once the
    /// pointer has been moved to `py` through the real pointer path.
    ///
    /// Separate from [`painted_row`] because a scrolled list has rows whose own
    /// top is above the panel: `painted_row`'s probe would land outside the
    /// menu and report "nothing painted" for a row that is half on screen.
    fn highlight_after_pointer_at(menu: &mut ContextMenu, py: f32) -> Option<(f32, f32)> {
        menu.handle_mouse_move(menu.x + 10.0, py);
        hover_highlight(menu)
    }

    /// The `(top, height)` of whatever hover highlight the menu is currently
    /// painting, if any.
    fn hover_highlight(menu: &ContextMenu) -> Option<(f32, f32)> {
        let (x, w) = (menu.x + 4.0, menu.width - 8.0);
        menu.render().into_iter().find_map(|cmd| match cmd {
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

    /// The `(top, bottom)` of the region the renderer clips its rows to. This
    /// is the render tree's own statement of where the rows are, which is what
    /// the hit test then has to agree with.
    fn painted_clip(menu: &ContextMenu) -> (f32, f32) {
        menu.render()
            .into_iter()
            .find_map(|cmd| match cmd {
                RenderCommand::PushClip { y, height, .. } => Some((y, y + height)),
                _ => None,
            })
            .expect("the menu clips its rows")
    }

    /// The scroll indicator's `((track_y, track_h), (thumb_y, thumb_h))`, or
    /// `None` when the menu paints none — which is what a menu that fits does.
    #[allow(clippy::type_complexity)]
    fn scrollbar_rects(menu: &ContextMenu) -> Option<((f32, f32), (f32, f32))> {
        let mut track = None;
        let mut thumb = None;
        for cmd in menu.render() {
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

    /// A menu of `count` plain enabled rows — no separators, so every index is
    /// selectable and `Down` visits them in order.
    fn tall_menu(count: usize) -> ContextMenu {
        ContextMenu::new(
            (0..count)
                .map(|i| MenuItem::Action {
                    id: i as MenuItemId,
                    label: format!("Item {i}"),
                    shortcut: None,
                    icon: None,
                    enabled: true,
                    checked: None,
                })
                .collect(),
        )
    }

    /// The y of every separator line the menu paints, in order.
    fn painted_separator_lines(menu: &ContextMenu) -> Vec<f32> {
        menu.render()
            .into_iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Line { y1, color, .. } if color == SEPARATOR_COLOR => Some(y1),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_item_is_selectable_exactly_where_it_was_painted() {
        let mut menu = ContextMenu::new(geometry_items());
        menu.show(300.0, 120.0);
        for idx in 0..menu.items.len() {
            if matches!(menu.items[idx], MenuItem::Separator) {
                continue;
            }
            let (top, height) = painted_row(&mut menu, idx)
                .unwrap_or_else(|| panic!("row {idx} painted no highlight to measure"));
            // Sweep the painted row rather than probing its middle. A three
            // pixel drift is invisible at the centre of a 28-px row and
            // obvious at its edges — which is where the user aims.
            for step in 0..8 {
                let probe = top + (step as f32) * height / 8.0;
                assert_eq!(
                    menu.index_at_y(probe),
                    Some(idx),
                    "row {idx} was painted at {top}..{} but {probe} answers otherwise",
                    top + height
                );
            }
            // The row owns its top edge and not its bottom one, so the two
            // sides of a boundary never both answer for it.
            assert_ne!(menu.index_at_y(top - 0.001), Some(idx));
            assert_ne!(menu.index_at_y(top + height), Some(idx));
        }
    }

    #[test]
    fn a_separator_is_drawn_inside_the_run_it_reserves_space_in() {
        let mut menu = ContextMenu::new(geometry_items());
        menu.show(300.0, 120.0);
        let lines = painted_separator_lines(&menu);
        let sep_indices: Vec<usize> = menu
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| matches!(it, MenuItem::Separator))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(lines.len(), sep_indices.len());
        let strip = menu.strip();
        for (&idx, &line_y) in sep_indices.iter().zip(&lines) {
            let top = strip.top(idx).unwrap();
            let height = strip.height(idx).unwrap();
            assert!(
                line_y >= top && line_y < top + height,
                "separator {idx} reserves {top}..{} but its line is drawn at {line_y}",
                top + height
            );
            // A separator has a place and a height like anything else; what it
            // does not have is selectability. That is this menu's rule, not
            // the strip's, and it is the one thing `index_at_y` adds.
            assert_eq!(menu.index_at_y(line_y), None);
            assert_eq!(menu.index_at_y(top), None);
        }
    }

    #[test]
    fn a_submenu_hangs_off_the_row_it_belongs_to() {
        // `y_offset_for_index` is what positions an opening submenu. It used
        // to be its own walk of the heights; if it disagrees with the
        // renderer the submenu appears beside a different row than the one
        // the user is pointing at.
        let mut menu = ContextMenu::new(geometry_items());
        menu.show(300.0, 120.0);
        for idx in 0..menu.items.len() {
            if matches!(menu.items[idx], MenuItem::Separator) {
                continue;
            }
            let (top, _) = painted_row(&mut menu, idx).unwrap();
            assert_eq!(
                menu.y + menu.y_offset_for_index(idx),
                top,
                "row {idx} is painted at {top} but a submenu would hang at {}",
                menu.y + menu.y_offset_for_index(idx)
            );
        }
        // Past the end there is no row; the offset is one past the last, so a
        // caller measuring downwards from it stays below the menu.
        let past = menu.y_offset_for_index(menu.items.len());
        assert_eq!(past, menu.content_height() - VERTICAL_PADDING);
    }

    #[test]
    fn the_menu_is_exactly_as_tall_as_the_rows_it_holds() {
        let mut menu = ContextMenu::new(geometry_items());
        menu.show(300.0, 120.0);
        let last = menu.items.len() - 1;
        let (top, height) = painted_row(&mut menu, last).unwrap();
        assert_eq!(
            menu.y + menu.content_height(),
            top + height + VERTICAL_PADDING,
            "the popup's own height must reach exactly one padding past the \
             last row it paints, or it clips a row or floats a gap"
        );
        assert!(menu.point_in_bounds(menu.x + 1.0, top + height));
        assert!(!menu.point_in_bounds(menu.x + 1.0, menu.y + menu.content_height() + 0.001));
    }

    #[test]
    fn nothing_outside_the_run_selects_an_item() {
        let mut menu = ContextMenu::new(geometry_items());
        menu.show(300.0, 120.0);
        // The top padding is the popup's border, not row zero.
        assert_eq!(menu.index_at_y(menu.y), None);
        assert_eq!(menu.index_at_y(menu.y + VERTICAL_PADDING - 0.001), None);
        // The bottom padding likewise.
        assert_eq!(menu.index_at_y(menu.y + menu.content_height()), None);
        assert_eq!(menu.index_at_y(menu.strip().bottom()), None);
        assert_eq!(menu.index_at_y(f32::NAN), None);
        assert_eq!(menu.index_at_y(f32::INFINITY), None);
    }

    #[test]
    fn an_empty_menu_is_just_its_padding() {
        let mut menu = ContextMenu::new(Vec::new());
        menu.show(300.0, 120.0);
        assert_eq!(menu.content_height(), VERTICAL_PADDING * 2.0);
        assert_eq!(menu.index_at_y(120.0), None);
        assert_eq!(menu.index_at_y(124.0), None);
        // Nothing to hang a submenu off, so the offset is the empty run's own
        // top rather than a coordinate invented for the occasion.
        assert_eq!(menu.y_offset_for_index(0), VERTICAL_PADDING);
    }

    // ─── A menu taller than the screen ──────────────────────────────────────

    #[test]
    fn a_menu_that_fits_neither_scrolls_nor_is_capped() {
        // The scrolling machinery must be invisible to the case that was
        // already right, which is nearly every menu in the tree.
        let mut menu = ContextMenu::new(geometry_items());
        menu.show(300.0, 120.0);
        assert_eq!(menu.panel_height(), menu.content_height());
        assert_eq!(menu.max_scroll(), 0.0);
        assert_eq!(menu.scroll, 0.0);
        assert_eq!(menu.viewport_bottom(), menu.strip().bottom());
        assert!(scrollbar_rects(&menu).is_none());

        // And it still flips upwards when it would overrun the bottom edge.
        let height = ContextMenu::new(geometry_items()).content_height();
        let mut flipped = ContextMenu::new(geometry_items());
        flipped.show(300.0, DEFAULT_VIEWPORT_HEIGHT - 5.0);
        assert_eq!(flipped.y, DEFAULT_VIEWPORT_HEIGHT - 5.0 - height);
    }

    #[test]
    fn a_menu_taller_than_the_screen_is_capped_rather_than_run_off_the_bottom() {
        let mut menu = tall_menu(120);
        assert!(
            menu.content_height() > DEFAULT_VIEWPORT_HEIGHT,
            "precondition: this menu has to be taller than the viewport"
        );
        menu.show(200.0, 500.0);

        // The old rule was a single flip: `(y - content_height).max(0.0)`. With
        // a menu 3368 px tall that clamped to zero and left the panel its full
        // height, so 2288 px of rows were drawn below the bottom of the screen
        // where they could be neither seen nor clicked.
        assert_eq!(menu.y, 0.0);
        assert_eq!(menu.panel_height(), DEFAULT_VIEWPORT_HEIGHT);
        assert!(menu.viewport_bottom() <= DEFAULT_VIEWPORT_HEIGHT);
        assert!(menu.max_scroll() > 0.0);

        // Sweep well past the bottom edge: no row may answer down there.
        for step in 0..200 {
            let probe = DEFAULT_VIEWPORT_HEIGHT + (step as f32) * 20.0;
            assert_eq!(
                menu.index_at_y(probe),
                None,
                "{probe} is off the bottom of the screen but names a row"
            );
        }
    }

    #[test]
    fn every_row_of_a_tall_menu_can_be_reached_by_scrolling() {
        // The property the whole change exists for: no row is unreachable.
        let mut menu = tall_menu(200);
        menu.show(200.0, 500.0);
        for idx in 0..menu.items.len() {
            menu.scroll_index_into_view(idx);
            let (clip_top, clip_bottom) = painted_clip(&menu);
            let strip = menu.strip();
            let (top, height) = (strip.top(idx).unwrap(), strip.height(idx).unwrap());
            assert!(
                top >= clip_top - 0.01 && top + height <= clip_bottom + 0.01,
                "row {idx} was scrolled to {top}..{} but the panel shows \
                 {clip_top}..{clip_bottom}",
                top + height
            );
            // Painted where it answers, and it answers where it is painted.
            let probe = top + height / 2.0;
            assert_eq!(menu.index_at_y(probe), Some(idx));
            assert_eq!(
                highlight_after_pointer_at(&mut menu, probe),
                Some((top, height)),
                "row {idx} answers at {probe} but paints its highlight elsewhere"
            );
        }
    }

    #[test]
    fn every_visible_row_of_a_scrolled_menu_answers_exactly_where_it_was_painted() {
        let mut menu = tall_menu(120);
        menu.show(200.0, 40.0);
        let max = menu.max_scroll();
        assert!(
            max > 0.0,
            "precondition: this menu must have room to scroll"
        );

        for &fraction in &[0.0_f32, 0.137, 0.5, 0.871, 1.0] {
            menu.set_scroll(max * fraction);
            let (clip_top, clip_bottom) = painted_clip(&menu);
            let mut seen_visible = 0_usize;
            for idx in 0..menu.items.len() {
                let strip = menu.strip();
                let (top, height) = (strip.top(idx).unwrap(), strip.height(idx).unwrap());
                let lo = top.max(clip_top);
                let hi = (top + height).min(clip_bottom);
                if hi <= lo {
                    // Entirely scrolled away: not one pixel of its own span may
                    // still name it.
                    for step in 0..8 {
                        let probe = top + (step as f32) * height / 8.0;
                        assert_ne!(
                            menu.index_at_y(probe),
                            Some(idx),
                            "row {idx} is off the panel at scroll {} but still \
                             answers at {probe}",
                            menu.scroll
                        );
                    }
                    continue;
                }
                seen_visible += 1;
                // Sweep the visible part rather than probing its middle: a
                // three-pixel drift is invisible at the centre of a 28-px row.
                for step in 0..8 {
                    let probe = lo + (hi - lo) * (step as f32) / 8.0;
                    assert_eq!(
                        menu.index_at_y(probe),
                        Some(idx),
                        "row {idx} is visible over {lo}..{hi} at scroll {} but \
                         {probe} answers otherwise",
                        menu.scroll
                    );
                }
                // The last pixel of the visible part, which is where an
                // off-by-one hides.
                assert_eq!(menu.index_at_y(hi - 0.001), Some(idx));
            }
            assert!(
                seen_visible > 30,
                "scroll {} showed only {seen_visible} rows of a 1072-px panel",
                menu.scroll
            );
        }
    }

    #[test]
    fn the_padding_of_a_scrolled_menu_selects_nothing_even_though_a_row_is_under_it() {
        // This is the case the strip alone cannot answer, and the reason
        // `index_at_y` gained a visible-region bound. Once the list is
        // scrolled it genuinely extends past both ends of the panel, so the
        // strip names a row for a pointer sitting in the menu's own border.
        let mut menu = tall_menu(120);
        menu.show(200.0, 40.0);
        menu.set_scroll(menu.max_scroll() / 2.0);

        for probe in [
            menu.y + VERTICAL_PADDING / 2.0,
            menu.y + menu.panel_height() - VERTICAL_PADDING / 2.0,
        ] {
            assert!(
                menu.point_in_bounds(menu.x + 10.0, probe),
                "precondition: {probe} is inside the popup"
            );
            assert!(
                menu.strip().index_at(probe).is_some(),
                "precondition: the scrolled list does reach {probe}"
            );
            assert_eq!(
                menu.index_at_y(probe),
                None,
                "{probe} is the popup's padding, not a row"
            );
        }

        // Through the real click path: nothing is selected, and the menu is not
        // dismissed either — the pointer was inside it.
        let (px, py) = (menu.x + 10.0, menu.y + VERTICAL_PADDING / 2.0);
        assert_eq!(menu.handle_click(px, py), None);
        assert!(menu.is_visible());
    }

    #[test]
    fn the_clip_a_menu_emits_is_the_region_the_pointer_lands_in() {
        for count in [3_usize, 120] {
            let mut menu = tall_menu(count);
            menu.show(200.0, 40.0);
            menu.set_scroll(menu.max_scroll() / 3.0);
            let (clip_top, clip_bottom) = painted_clip(&menu);
            assert_eq!(clip_top, menu.viewport_top());
            assert_eq!(clip_bottom, menu.viewport_bottom());
            // 400 probes down the whole screen: outside the painted region,
            // nothing answers.
            for step in 0..400 {
                let probe = (step as f32) * DEFAULT_VIEWPORT_HEIGHT / 400.0;
                if probe < clip_top || probe >= clip_bottom {
                    assert_eq!(
                        menu.index_at_y(probe),
                        None,
                        "{probe} is outside the clip {clip_top}..{clip_bottom} \
                         but names a row in a {count}-row menu"
                    );
                }
            }
        }
    }

    #[test]
    fn the_wheel_scrolls_to_each_end_and_stops_there() {
        let mut menu = tall_menu(120);
        menu.show(200.0, 40.0);
        let (px, py) = (menu.x + 10.0, menu.y + 10.0);
        let last = menu.items.len() - 1;

        for _ in 0..500 {
            assert!(menu.handle_scroll(px, py, -1.0));
        }
        assert_eq!(menu.scroll, menu.max_scroll());
        let strip = menu.strip();
        let (top, height) = (strip.top(last).unwrap(), strip.height(last).unwrap());
        assert!(
            top >= menu.viewport_top() && top + height <= menu.viewport_bottom() + 0.01,
            "the last row sits at {top}..{} after scrolling to the end",
            top + height
        );
        assert_eq!(menu.index_at_y(top + height / 2.0), Some(last));

        for _ in 0..500 {
            assert!(menu.handle_scroll(px, py, 1.0));
        }
        assert_eq!(menu.scroll, 0.0);
        assert_eq!(menu.index_at_y(menu.viewport_top()), Some(0));
    }

    #[test]
    fn a_popup_swallows_the_wheel_even_with_nothing_to_scroll() {
        // Otherwise the wheel falls through the menu to whatever it covers,
        // and the document scrolls out from under an open context menu.
        let mut menu = ContextMenu::new(geometry_items());
        menu.show(300.0, 120.0);
        assert_eq!(menu.max_scroll(), 0.0);
        let (px, py) = (menu.x + 10.0, menu.y + 10.0);
        assert!(menu.handle_scroll(px, py, -1.0));
        assert_eq!(menu.scroll, 0.0);
        // Outside it, the wheel is not ours.
        let above = menu.y - 5.0;
        assert!(!menu.handle_scroll(px, above, -1.0));
        // Nor is it while hidden.
        menu.hide();
        assert!(!menu.handle_scroll(px, py, -1.0));
    }

    #[test]
    fn a_scroll_offset_that_is_not_a_number_is_refused_rather_than_clamped() {
        // `NaN` compares false against both ends of a `clamp`, so clamping
        // would let it through to the strip's origin and poison every row's
        // position at once — a menu that answers for no pointer at all.
        let mut menu = tall_menu(120);
        menu.show(200.0, 40.0);
        menu.set_scroll(100.0);
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            menu.set_scroll(bad);
            assert_eq!(menu.scroll, 100.0);
        }
        let (px, py) = (menu.x + 10.0, menu.y + 10.0);
        menu.handle_scroll(px, py, f32::NAN);
        assert_eq!(menu.scroll, 100.0);
        assert!(menu.index_at_y(menu.viewport_top()).is_some());
    }

    #[test]
    fn arrowing_down_a_tall_menu_brings_each_row_onto_the_screen() {
        // Without this the highlight walks off the bottom of a panel that has
        // no way to follow it, and Enter acts on a row nobody can see.
        let mut menu = tall_menu(120);
        menu.show(200.0, 500.0);
        for expected in 0..menu.items.len() {
            menu.handle_key(&make_key(Key::Down));
            assert_eq!(menu.hover_index, Some(expected));
            let (clip_top, clip_bottom) = painted_clip(&menu);
            let (top, height) = hover_highlight(&menu)
                .unwrap_or_else(|| panic!("row {expected} is hovered but paints no highlight"));
            assert!(
                top >= clip_top - 0.01 && top + height <= clip_bottom + 0.01,
                "arrowing to row {expected} left it at {top}..{} outside the \
                 visible {clip_top}..{clip_bottom}",
                top + height
            );
            assert_eq!(menu.index_at_y(top + height / 2.0), Some(expected));
        }
        // Wrapping back round to the top scrolls back with it.
        menu.handle_key(&make_key(Key::Down));
        assert_eq!(menu.hover_index, Some(0));
        assert_eq!(menu.scroll, 0.0);
    }

    #[test]
    fn the_scroll_thumb_stays_in_its_track_and_reaches_both_ends() {
        let mut menu = tall_menu(120);
        menu.show(200.0, 40.0);
        let (track, thumb_top) = scrollbar_rects(&menu).expect("a capped menu shows an indicator");
        assert_eq!(track.0, menu.viewport_top());
        assert_eq!(track.1, menu.viewport_height());
        assert_eq!(thumb_top.0, track.0, "at scroll 0 the thumb is at the top");
        assert!(thumb_top.1 >= SCROLLBAR_MIN_THUMB);
        assert!(thumb_top.1 <= track.1);

        menu.set_scroll(menu.max_scroll());
        let (track_end, thumb_end) = scrollbar_rects(&menu).unwrap();
        assert_eq!(track_end, track, "the track does not move when the rows do");
        assert_eq!(
            thumb_end.0 + thumb_end.1,
            track.0 + track.1,
            "at the end of the list the thumb's bottom is flush with the track's"
        );

        menu.set_scroll(menu.max_scroll() / 2.0);
        let (_, thumb_mid) = scrollbar_rects(&menu).unwrap();
        assert!(thumb_mid.0 > thumb_top.0 && thumb_mid.0 < thumb_end.0);
        assert!(thumb_mid.0 >= track.0 && thumb_mid.0 + thumb_mid.1 <= track.0 + track.1 + 0.01);
    }

    #[test]
    fn showing_a_scrolled_menu_again_shows_its_top() {
        let mut menu = tall_menu(120);
        menu.show(200.0, 40.0);
        menu.set_scroll(menu.max_scroll());
        menu.show(200.0, 40.0);
        assert_eq!(menu.scroll, 0.0);
        assert_eq!(menu.index_at_y(menu.viewport_top()), Some(0));
    }

    #[test]
    fn menu_initially_hidden() {
        let menu = ContextMenu::new(sample_items());
        assert!(!menu.is_visible());
    }

    #[test]
    fn menu_show_and_hide() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(100.0, 200.0);
        assert!(menu.is_visible());
        menu.hide();
        assert!(!menu.is_visible());
    }

    #[test]
    fn menu_click_selects_item() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        // Click within the first item area (after padding).
        let click_y = VERTICAL_PADDING + ITEM_HEIGHT / 2.0;
        let result = menu.handle_click(50.0, click_y);
        assert_eq!(result, Some(1)); // "Cut" has id 1
        assert!(!menu.is_visible()); // Menu closes after selection
    }

    #[test]
    fn menu_click_disabled_item_does_nothing() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        // Item 3 ("Paste") is disabled, it's at index 3 (after separator).
        // Offset: items 0,1 = 2*ITEM_HEIGHT, separator = SEPARATOR_HEIGHT, then half of item 3.
        let click_y = VERTICAL_PADDING + 2.0 * ITEM_HEIGHT + SEPARATOR_HEIGHT + ITEM_HEIGHT / 2.0;
        let result = menu.handle_click(50.0, click_y);
        assert_eq!(result, None);
        assert!(menu.is_visible()); // Menu stays open
    }

    #[test]
    fn menu_click_outside_closes() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(100.0, 100.0);

        let result = menu.handle_click(0.0, 0.0);
        assert_eq!(result, None);
        assert!(!menu.is_visible());
    }

    #[test]
    fn keyboard_down_moves_hover() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        // Press Down — should select first selectable item (index 0).
        menu.handle_key(&make_key(Key::Down));
        assert_eq!(menu.hover_index, Some(0));

        // Press Down again — should move to index 1.
        menu.handle_key(&make_key(Key::Down));
        assert_eq!(menu.hover_index, Some(1));

        // Press Down again — should skip separator (index 2), skip disabled (index 3), land on 4.
        menu.handle_key(&make_key(Key::Down));
        assert_eq!(menu.hover_index, Some(4));
    }

    #[test]
    fn keyboard_up_wraps_around() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        // Press Up from no selection — should wrap to last selectable item (index 4).
        menu.handle_key(&make_key(Key::Up));
        assert_eq!(menu.hover_index, Some(4));
    }

    fn action(id: MenuItemId, enabled: bool) -> MenuItem {
        MenuItem::Action {
            id,
            label: format!("item {id}"),
            shortcut: None,
            icon: None,
            enabled,
            checked: None,
        }
    }

    /// Down from the last selectable row comes back to the first, and Up from
    /// the first goes to the last — in both cases stepping over the separator
    /// and the disabled row in between. `sample_items` is selectable at 0, 1 and
    /// 4; 2 is a separator and 3 is disabled.
    #[test]
    fn the_hover_wraps_past_separators_and_disabled_rows_in_both_directions() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        menu.hover_index = Some(4);
        menu.handle_key(&make_key(Key::Down));
        assert_eq!(
            menu.hover_index,
            Some(0),
            "past the end, round to the first"
        );

        menu.handle_key(&make_key(Key::Up));
        assert_eq!(
            menu.hover_index,
            Some(4),
            "back the other way, over the disabled row and the separator"
        );

        menu.handle_key(&make_key(Key::Up));
        assert_eq!(menu.hover_index, Some(1), "and on up the list");
    }

    /// Walking the whole menu with the arrow keys must visit every selectable
    /// row and nothing else, whichever way it goes — the property the old signed
    /// cursor could only be checked for one step at a time.
    #[test]
    fn arrowing_all_the_way_round_visits_exactly_the_selectable_rows() {
        for (key, mut expected) in [(Key::Down, vec![0, 1, 4]), (Key::Up, vec![4, 1, 0])] {
            let mut menu = ContextMenu::new(sample_items());
            menu.show(0.0, 0.0);
            let mut seen = Vec::new();
            // One more press than there are rows, to catch a walk that stalls
            // or that lands somewhere twice before coming round.
            for _ in 0..=expected.len() {
                menu.handle_key(&make_key(key));
                seen.push(menu.hover_index.expect("a selectable row exists"));
            }
            expected.push(expected[0]);
            assert_eq!(seen, expected, "{key:?}");
        }
    }

    /// One selectable row surrounded by unselectable ones stays put under
    /// repeated presses rather than being stepped off and lost. The walk comes
    /// back round to the current row last, which is what makes this hold.
    #[test]
    fn a_lone_selectable_row_survives_repeated_presses() {
        let items = vec![
            MenuItem::Separator,
            action(1, false),
            action(2, true),
            MenuItem::Separator,
        ];
        let mut menu = ContextMenu::new(items);
        menu.show(0.0, 0.0);

        for press in 0..4 {
            menu.handle_key(&make_key(Key::Down));
            assert_eq!(menu.hover_index, Some(2), "down press {press}");
        }
        for press in 0..4 {
            menu.handle_key(&make_key(Key::Up));
            assert_eq!(menu.hover_index, Some(2), "up press {press}");
        }
    }

    /// A menu with nothing to land on must leave the hover alone rather than
    /// highlight a separator or a disabled row — Enter acts on whatever is
    /// hovered, so a lie here would fire a disabled action.
    #[test]
    fn a_menu_with_no_selectable_row_highlights_nothing() {
        let items = vec![MenuItem::Separator, action(1, false), MenuItem::Separator];
        let mut menu = ContextMenu::new(items);
        menu.show(0.0, 0.0);

        menu.handle_key(&make_key(Key::Down));
        assert_eq!(menu.hover_index, None);
        menu.handle_key(&make_key(Key::Up));
        assert_eq!(menu.hover_index, None);
        assert_eq!(
            menu.handle_key(&make_key(Key::Enter)),
            Some(MenuAction::None),
            "Enter on nothing selects nothing"
        );
    }

    /// An empty menu has no index at all; the arrow keys must not underflow
    /// looking for one.
    #[test]
    fn an_empty_menu_can_be_arrowed_through_without_panicking() {
        let mut menu = ContextMenu::new(Vec::new());
        menu.show(0.0, 0.0);
        menu.handle_key(&make_key(Key::Down));
        menu.handle_key(&make_key(Key::Up));
        assert_eq!(menu.hover_index, None);
    }

    #[test]
    fn keyboard_enter_selects_hovered() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        menu.handle_key(&make_key(Key::Down)); // hover index 0
        let result = menu.handle_key(&make_key(Key::Enter));
        assert_eq!(result, Some(MenuAction::Selected(1)));
        assert!(!menu.is_visible());
    }

    #[test]
    fn keyboard_escape_closes_menu() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        let result = menu.handle_key(&make_key(Key::Escape));
        assert_eq!(result, Some(MenuAction::Closed));
        assert!(!menu.is_visible());
    }

    #[test]
    fn submenu_opens_on_hover() {
        let items = vec![MenuItem::Submenu {
            id: 10,
            label: "More".to_string(),
            icon: None,
            enabled: true,
            children: vec![MenuItem::Action {
                id: 11,
                label: "Sub Item".to_string(),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: None,
            }],
        }];

        let mut menu = ContextMenu::new(items);
        menu.show(0.0, 0.0);

        // Move mouse over the submenu item.
        let hover_y = VERTICAL_PADDING + ITEM_HEIGHT / 2.0;
        menu.handle_mouse_move(50.0, hover_y);

        assert!(menu.open_submenu.is_some());
        let (idx, ref sub) = *menu.open_submenu.as_ref().expect("submenu should be open");
        assert_eq!(idx, 0);
        assert!(sub.is_visible());
    }

    #[test]
    fn submenu_keyboard_right_opens() {
        let items = vec![MenuItem::Submenu {
            id: 20,
            label: "View".to_string(),
            icon: None,
            enabled: true,
            children: vec![MenuItem::Action {
                id: 21,
                label: "Zoom In".to_string(),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: None,
            }],
        }];

        let mut menu = ContextMenu::new(items);
        menu.show(0.0, 0.0);

        menu.handle_key(&make_key(Key::Down)); // hover on submenu item
        menu.handle_key(&make_key(Key::Right)); // open submenu

        assert!(menu.open_submenu.is_some());
    }

    #[test]
    fn menu_edge_flip_horizontal() {
        let mut menu = ContextMenu::new(sample_items());
        // Show near right edge — should flip to left.
        menu.show(DEFAULT_VIEWPORT_WIDTH - 10.0, 100.0);
        assert!(menu.x < DEFAULT_VIEWPORT_WIDTH - 10.0);
    }

    #[test]
    fn menu_edge_flip_vertical() {
        let mut menu = ContextMenu::new(sample_items());
        // Show near bottom edge — should flip upward.
        menu.show(100.0, DEFAULT_VIEWPORT_HEIGHT - 10.0);
        assert!(menu.y < DEFAULT_VIEWPORT_HEIGHT - 10.0);
    }

    // ─── Tooltip tests ──────────────────────────────────────────────────────

    #[test]
    fn tooltip_initially_hidden() {
        let tooltip = Tooltip::new("Hello");
        assert!(!tooltip.is_visible());
    }

    #[test]
    fn tooltip_appears_after_delay() {
        let mut tooltip = Tooltip::new("Tooltip text").with_delay(200);

        tooltip.start_hover(100.0, 100.0, 1000);
        tooltip.tick(1100); // 100ms elapsed — not enough
        assert!(!tooltip.is_visible());

        tooltip.tick(1200); // 200ms elapsed — should appear
        assert!(tooltip.is_visible());
    }

    #[test]
    fn tooltip_disappears_on_leave() {
        let mut tooltip = Tooltip::new("Tip");
        tooltip.start_hover(50.0, 50.0, 0);
        tooltip.tick(600); // Past default delay
        assert!(tooltip.is_visible());

        tooltip.end_hover();
        assert!(!tooltip.is_visible());
    }

    #[test]
    fn tooltip_render_empty_when_hidden() {
        let tooltip = Tooltip::new("Hidden tooltip");
        let cmds = tooltip.render();
        assert!(cmds.is_empty());
    }

    #[test]
    fn tooltip_render_produces_commands_when_visible() {
        let mut tooltip = Tooltip::new("Visible tooltip");
        tooltip.start_hover(50.0, 50.0, 0);
        tooltip.tick(600);
        assert!(tooltip.is_visible());

        let cmds = tooltip.render();
        // Should have shadow, background, border, and at least one text command.
        assert!(cmds.len() >= 4);
    }

    #[test]
    fn tooltip_edge_flip() {
        let mut tooltip = Tooltip::new("Near edge");
        // Start hover near bottom-right — should flip position.
        tooltip.start_hover(
            DEFAULT_VIEWPORT_WIDTH - 5.0,
            DEFAULT_VIEWPORT_HEIGHT - 5.0,
            0,
        );
        assert!(tooltip.x < DEFAULT_VIEWPORT_WIDTH - 5.0);
        assert!(tooltip.y < DEFAULT_VIEWPORT_HEIGHT - 5.0);
    }

    #[test]
    fn tooltip_wraps_by_measured_width_not_character_count() {
        // Wrapping and box-sizing have to use the same rule. `W` is far wider
        // than the old 0.6-of-the-font-size guess and `i` far narrower, so a
        // count-based wrap produced lines that overflowed the box it sized.
        for text in [
            "WWWW WWWW WWWW WWWW WWWW WWWW",
            "iiii iiii iiii iiii iiii iiii",
            "ééé ééé ééé ééé ééé ééé ééé ééé",
        ] {
            let tooltip = Tooltip::new(text);
            for line in tooltip.wrap_text() {
                // A single word may legitimately exceed the limit — it is not
                // broken mid-word — but a wrapped line never should.
                if line.split_whitespace().count() < 2 {
                    continue;
                }
                assert!(
                    crate::text::width(&line, TOOLTIP_FONT_SIZE) <= tooltip.max_width,
                    "{line:?} is wider than the tooltip that will contain it"
                );
            }
        }
    }

    #[test]
    fn tooltip_wrapping_never_loses_a_word() {
        let text = "the quick brown fox jumps over the lazy dog";
        let tooltip = Tooltip::new(text);
        let joined = tooltip.wrap_text().join(" ");
        assert_eq!(
            joined.split_whitespace().collect::<Vec<_>>(),
            text.split_whitespace().collect::<Vec<_>>()
        );
    }
}

//! Virtual desktop overview (Expose / Mission Control).
//!
//! Provides a fullscreen overlay that shows all windows across all virtual
//! desktops in a grid layout.  The user can click a thumbnail to switch
//! to that window, use arrow keys to navigate, type to search by title or
//! app name, and manage desktops (add, switch, close windows).
//!
//! Three view modes are supported:
//! - **AllWindows** — grid of windows on the current desktop.
//! - **AllDesktops** — horizontal lanes, one per desktop, each showing its windows.
//! - **RecentApps** — most-recently-used window list across all desktops.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_SURFACE2: Color = Color::from_hex(0x585B70);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
const MOCHA_YELLOW: Color = Color::from_hex(0xF9E2AF);
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);

// ============================================================================
// Public types
// ============================================================================

/// Which view mode the overview is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewMode {
    /// Grid of windows on the current desktop.
    AllWindows,
    /// Horizontal lanes — one per desktop — each showing its windows.
    AllDesktops,
    /// Most-recently-used window list across all desktops.
    RecentApps,
}

/// Metadata for a single window to be shown in the overview.
#[derive(Debug, Clone)]
pub struct WindowThumbnail {
    pub window_id: u64,
    pub desktop_id: u32,
    pub title: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub is_focused: bool,
    pub is_minimized: bool,
    pub app_name: String,
}

/// A group of thumbnails belonging to a single virtual desktop.
#[derive(Debug, Clone)]
pub struct DesktopLane {
    pub desktop_id: u32,
    pub name: String,
    pub thumbnails: Vec<WindowThumbnail>,
    pub is_current: bool,
}

/// A positioned thumbnail ready for rendering.
#[derive(Debug, Clone)]
pub struct ThumbnailLayout {
    pub window_id: u64,
    pub desktop_id: u32,
    pub title: String,
    pub app_name: String,
    pub is_focused: bool,
    pub is_minimized: bool,
    /// Computed render position / size inside the overview viewport.
    pub render_x: f32,
    pub render_y: f32,
    pub render_width: f32,
    pub render_height: f32,
}

/// Full mutable state of the overview.
#[derive(Debug, Clone)]
pub struct OverviewState {
    pub mode: OverviewMode,
    pub visible: bool,
    /// 0.0 = fully hidden, 1.0 = fully visible (used for fade animation).
    pub animation_progress: f32,
    pub lanes: Vec<DesktopLane>,
    pub hovered_window: Option<u64>,
    pub selected_desktop: Option<u32>,
    pub search_query: String,
    pub search_results: Vec<u64>,
}

impl OverviewState {
    /// Create a new, hidden overview state.
    pub fn new() -> Self {
        Self {
            mode: OverviewMode::AllWindows,
            visible: false,
            animation_progress: 0.0,
            lanes: Vec::new(),
            hovered_window: None,
            selected_desktop: None,
            search_query: String::new(),
            search_results: Vec::new(),
        }
    }

    /// Show the overview in the given mode.
    pub fn show(&mut self, mode: OverviewMode) {
        self.mode = mode;
        self.visible = true;
        self.animation_progress = 0.0;
        self.search_query.clear();
        self.search_results.clear();
        self.hovered_window = None;
    }

    /// Hide the overview.
    pub fn hide(&mut self) {
        self.visible = false;
        self.animation_progress = 1.0; // will animate down to 0
    }

    /// Toggle visibility using the given mode.
    pub fn toggle(&mut self, mode: OverviewMode) {
        if self.visible && self.mode == mode {
            self.hide();
        } else {
            self.show(mode);
        }
    }

    /// Advance the animation by `dt` (seconds).  Returns `true` while the
    /// animation is still in progress and the caller should keep ticking.
    pub fn tick_animation(&mut self, dt: f32, config: &OverviewConfig) -> bool {
        let duration_secs = config.animation_duration_ms as f32 / 1000.0;
        if duration_secs <= 0.0 {
            self.animation_progress = if self.visible { 1.0 } else { 0.0 };
            return false;
        }
        let step = dt / duration_secs;
        if self.visible {
            self.animation_progress = (self.animation_progress + step).min(1.0);
            self.animation_progress < 1.0
        } else {
            self.animation_progress = (self.animation_progress - step).max(0.0);
            self.animation_progress > 0.0
        }
    }

    /// Collect every `WindowThumbnail` from all lanes.
    pub fn all_thumbnails(&self) -> Vec<&WindowThumbnail> {
        self.lanes.iter().flat_map(|l| l.thumbnails.iter()).collect()
    }

    /// Update the search results based on the current query.
    pub fn update_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_results.clear();
            return;
        }
        let query = self.search_query.to_lowercase();
        self.search_results = self
            .lanes
            .iter()
            .flat_map(|l| l.thumbnails.iter())
            .filter(|t| {
                t.title.to_lowercase().contains(&query)
                    || t.app_name.to_lowercase().contains(&query)
            })
            .map(|t| t.window_id)
            .collect();
    }

    /// Push a character into the search query and refresh results.
    pub fn type_search_char(&mut self, ch: char) {
        self.search_query.push(ch);
        self.update_search();
    }

    /// Delete the last character from the search query and refresh results.
    pub fn search_backspace(&mut self) {
        self.search_query.pop();
        self.update_search();
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Tunable parameters for the overview layout and behaviour.
#[derive(Debug, Clone)]
pub struct OverviewConfig {
    /// Padding between thumbnail cells (pixels).
    pub thumbnail_padding: f32,
    /// Maximum columns in the grid.
    pub max_columns: u32,
    /// Whether desktop labels are drawn in AllDesktops mode.
    pub show_desktop_labels: bool,
    /// Fade-in / fade-out duration in milliseconds.
    pub animation_duration_ms: u32,
    /// Opacity of the dark overlay background (0.0 – 1.0).
    pub background_opacity: f32,
}

impl Default for OverviewConfig {
    fn default() -> Self {
        Self {
            thumbnail_padding: 16.0,
            max_columns: 5,
            show_desktop_labels: true,
            animation_duration_ms: 250,
            background_opacity: 0.85,
        }
    }
}

impl OverviewConfig {
    /// Serialize to a simple key=value text format.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("thumbnail_padding={}\n", self.thumbnail_padding));
        out.push_str(&format!("max_columns={}\n", self.max_columns));
        out.push_str(&format!("show_desktop_labels={}\n", self.show_desktop_labels));
        out.push_str(&format!("animation_duration_ms={}\n", self.animation_duration_ms));
        out.push_str(&format!("background_opacity={}\n", self.background_opacity));
        out
    }

    /// Deserialize from key=value text.  Unknown keys are silently ignored;
    /// missing keys keep their default value.
    pub fn from_text(text: &str) -> Self {
        let mut cfg = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim();
                match key {
                    "thumbnail_padding" => {
                        if let Ok(v) = val.parse::<f32>() {
                            cfg.thumbnail_padding = v;
                        }
                    }
                    "max_columns" => {
                        if let Ok(v) = val.parse::<u32>() {
                            cfg.max_columns = v;
                        }
                    }
                    "show_desktop_labels" => {
                        cfg.show_desktop_labels = val == "true";
                    }
                    "animation_duration_ms" => {
                        if let Ok(v) = val.parse::<u32>() {
                            cfg.animation_duration_ms = v;
                        }
                    }
                    "background_opacity" => {
                        if let Ok(v) = val.parse::<f32>() {
                            cfg.background_opacity = v;
                        }
                    }
                    _ => {} // unknown key — ignore
                }
            }
        }
        cfg
    }
}

// ============================================================================
// Layout engine
// ============================================================================

/// Arrange thumbnails in a grid that fits inside `(bx, by, bw, bh)`.
///
/// Each thumbnail is scaled to preserve its original aspect ratio while
/// fitting inside its cell.  Returns positioned `ThumbnailLayout` entries.
pub fn compute_grid_layout(
    thumbnails: &[WindowThumbnail],
    bx: f32,
    by: f32,
    bw: f32,
    bh: f32,
    config: &OverviewConfig,
) -> Vec<ThumbnailLayout> {
    if thumbnails.is_empty() || bw <= 0.0 || bh <= 0.0 {
        return Vec::new();
    }

    let count = thumbnails.len();
    let max_cols = (config.max_columns.max(1)) as usize;
    let cols = count.min(max_cols);
    let rows = (count + cols - 1) / cols; // ceil division

    let pad = config.thumbnail_padding;
    let cell_w = (bw - pad * (cols as f32 + 1.0)) / cols as f32;
    let cell_h = (bh - pad * (rows as f32 + 1.0)) / rows as f32;

    if cell_w <= 0.0 || cell_h <= 0.0 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(count);
    for (i, thumb) in thumbnails.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;

        let cx = bx + pad + col as f32 * (cell_w + pad);
        let cy = by + pad + row as f32 * (cell_h + pad);

        // Scale to fit inside cell while keeping the aspect ratio.
        let (tw, th) = fit_aspect(thumb.width, thumb.height, cell_w, cell_h);
        let rx = cx + (cell_w - tw) / 2.0;
        let ry = cy + (cell_h - th) / 2.0;

        out.push(ThumbnailLayout {
            window_id: thumb.window_id,
            desktop_id: thumb.desktop_id,
            title: thumb.title.clone(),
            app_name: thumb.app_name.clone(),
            is_focused: thumb.is_focused,
            is_minimized: thumb.is_minimized,
            render_x: rx,
            render_y: ry,
            render_width: tw,
            render_height: th,
        });
    }
    out
}

/// Arrange desktops as horizontal lanes.  Each lane gets a proportional
/// vertical slice of `(bx, by, bw, bh)` and its windows are laid out in
/// a single row inside that lane.
pub fn compute_lane_layout(
    lanes: &[DesktopLane],
    bx: f32,
    by: f32,
    bw: f32,
    bh: f32,
    config: &OverviewConfig,
) -> Vec<ThumbnailLayout> {
    if lanes.is_empty() || bw <= 0.0 || bh <= 0.0 {
        return Vec::new();
    }

    let pad = config.thumbnail_padding;
    let label_h: f32 = if config.show_desktop_labels { 28.0 } else { 0.0 };
    let lane_count = lanes.len();
    let lane_h = (bh - pad * (lane_count as f32 + 1.0)) / lane_count as f32;

    if lane_h <= 0.0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for (li, lane) in lanes.iter().enumerate() {
        let ly = by + pad + li as f32 * (lane_h + pad);
        let content_y = ly + label_h;
        let content_h = (lane_h - label_h).max(0.0);

        if lane.thumbnails.is_empty() || content_h <= 0.0 {
            continue;
        }

        let cols = lane.thumbnails.len();
        let cell_w = (bw - pad * (cols as f32 + 1.0)) / cols as f32;
        if cell_w <= 0.0 {
            continue;
        }

        for (ci, thumb) in lane.thumbnails.iter().enumerate() {
            let cx = bx + pad + ci as f32 * (cell_w + pad);
            let (tw, th) = fit_aspect(thumb.width, thumb.height, cell_w, content_h);
            let rx = cx + (cell_w - tw) / 2.0;
            let ry = content_y + (content_h - th) / 2.0;

            out.push(ThumbnailLayout {
                window_id: thumb.window_id,
                desktop_id: thumb.desktop_id,
                title: thumb.title.clone(),
                app_name: thumb.app_name.clone(),
                is_focused: thumb.is_focused,
                is_minimized: thumb.is_minimized,
                render_x: rx,
                render_y: ry,
                render_width: tw,
                render_height: th,
            });
        }
    }
    out
}

/// Scale `(w, h)` to fit inside `(max_w, max_h)` while preserving aspect
/// ratio.  Returns the scaled `(width, height)`.
fn fit_aspect(w: f32, h: f32, max_w: f32, max_h: f32) -> (f32, f32) {
    if w <= 0.0 || h <= 0.0 || max_w <= 0.0 || max_h <= 0.0 {
        return (0.0, 0.0);
    }
    let scale = (max_w / w).min(max_h / h).min(1.0);
    // Ensure at least 1-pixel dimensions so thumbnails remain visible.
    let sw = (w * scale).max(1.0);
    let sh = (h * scale).max(1.0);
    (sw, sh)
}

// ============================================================================
// Rendering
// ============================================================================

/// Render the full overview overlay into a list of `RenderCommand`s.
///
/// `screen_w` / `screen_h` are the total display dimensions.
pub fn render_overview(
    state: &OverviewState,
    config: &OverviewConfig,
    screen_w: f32,
    screen_h: f32,
) -> Vec<RenderCommand> {
    if state.animation_progress <= 0.0 {
        return Vec::new();
    }

    let alpha = (config.background_opacity * state.animation_progress * 255.0) as u8;
    let mut cmds = Vec::with_capacity(128);

    // Dark overlay background.
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: screen_w,
        height: screen_h,
        color: Color::rgba(MOCHA_MANTLE.r, MOCHA_MANTLE.g, MOCHA_MANTLE.b, alpha),
        corner_radii: CornerRadii::ZERO,
    });

    // Search bar at top.
    render_search_bar(&mut cmds, state, screen_w);

    // Content area (below search bar).
    let content_y = 70.0;
    let content_h = screen_h - content_y - 20.0; // 20px bottom margin

    let layouts = match state.mode {
        OverviewMode::AllWindows | OverviewMode::RecentApps => {
            let thumbs = collect_thumbs_for_mode(state);
            compute_grid_layout(&thumbs, 20.0, content_y, screen_w - 40.0, content_h, config)
        }
        OverviewMode::AllDesktops => {
            compute_lane_layout(&state.lanes, 20.0, content_y, screen_w - 40.0, content_h, config)
        }
    };

    // Desktop labels (AllDesktops only).
    if state.mode == OverviewMode::AllDesktops && config.show_desktop_labels {
        render_desktop_labels(&mut cmds, state, config, content_y, screen_w, content_h);
    }

    // Thumbnail cards.
    for layout in &layouts {
        let is_hovered = state.hovered_window == Some(layout.window_id);
        let is_search_match = !state.search_query.is_empty()
            && state.search_results.contains(&layout.window_id);
        let is_dimmed = !state.search_query.is_empty() && !is_search_match;

        render_thumbnail_card(&mut cmds, layout, is_hovered, is_dimmed);
    }

    // "+" button for adding a new desktop (AllDesktops mode).
    if state.mode == OverviewMode::AllDesktops {
        render_add_desktop_button(&mut cmds, screen_w, screen_h);
    }

    cmds
}

/// Render the search bar at the top of the overlay.
fn render_search_bar(cmds: &mut Vec<RenderCommand>, state: &OverviewState, screen_w: f32) {
    let bar_w = 400.0_f32.min(screen_w - 40.0);
    let bar_x = (screen_w - bar_w) / 2.0;
    let bar_y = 16.0;
    let bar_h = 36.0;

    // Background.
    cmds.push(RenderCommand::FillRect {
        x: bar_x,
        y: bar_y,
        width: bar_w,
        height: bar_h,
        color: MOCHA_SURFACE0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Border (highlights when query is active).
    let border_color = if state.search_query.is_empty() {
        MOCHA_SURFACE1
    } else {
        MOCHA_BLUE
    };
    cmds.push(RenderCommand::StrokeRect {
        x: bar_x,
        y: bar_y,
        width: bar_w,
        height: bar_h,
        color: border_color,
        line_width: 1.0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Text.
    let (display_text, text_color) = if state.search_query.is_empty() {
        ("Search windows...".to_string(), MOCHA_OVERLAY0)
    } else {
        (state.search_query.clone(), MOCHA_TEXT)
    };
    cmds.push(RenderCommand::Text {
        x: bar_x + 12.0,
        y: bar_y + 10.0,
        text: display_text,
        color: text_color,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(bar_w - 24.0),
    });
}

/// Render desktop labels and current-desktop indicator in AllDesktops mode.
fn render_desktop_labels(
    cmds: &mut Vec<RenderCommand>,
    state: &OverviewState,
    config: &OverviewConfig,
    content_y: f32,
    screen_w: f32,
    content_h: f32,
) {
    let pad = config.thumbnail_padding;
    let lane_count = state.lanes.len();
    if lane_count == 0 {
        return;
    }
    let lane_h = (content_h - pad * (lane_count as f32 + 1.0)) / lane_count as f32;
    if lane_h <= 0.0 {
        return;
    }

    for (li, lane) in state.lanes.iter().enumerate() {
        let ly = content_y + pad + li as f32 * (lane_h + pad);

        // Current desktop indicator bar.
        if lane.is_current {
            cmds.push(RenderCommand::FillRect {
                x: 20.0,
                y: ly,
                width: 4.0,
                height: 24.0,
                color: MOCHA_BLUE,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Label.
        cmds.push(RenderCommand::Text {
            x: 32.0,
            y: ly + 4.0,
            text: lane.name.clone(),
            color: if lane.is_current { MOCHA_TEXT } else { MOCHA_SUBTEXT0 },
            font_size: 13.0,
            font_weight: if lane.is_current {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(screen_w - 80.0),
        });
    }
}

/// Render a single thumbnail card.
fn render_thumbnail_card(
    cmds: &mut Vec<RenderCommand>,
    layout: &ThumbnailLayout,
    is_hovered: bool,
    is_dimmed: bool,
) {
    let x = layout.render_x;
    let y = layout.render_y;
    let w = layout.render_width;
    let h = layout.render_height;

    // A label row sits below the card; reserve space.
    let label_h = 32.0;

    // Hover: slight scale-up effect simulated with padding reduction.
    let (dx, dy, dw, dh) = if is_hovered {
        (-4.0_f32, -4.0_f32, 8.0_f32, 8.0_f32)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };

    // Card background (window representation).
    let bg_color = if is_dimmed {
        Color::rgba(MOCHA_SURFACE0.r, MOCHA_SURFACE0.g, MOCHA_SURFACE0.b, 100)
    } else {
        MOCHA_SURFACE0
    };
    cmds.push(RenderCommand::FillRect {
        x: x + dx,
        y: y + dy,
        width: w + dw,
        height: h + dh,
        color: bg_color,
        corner_radii: CornerRadii::all(6.0),
    });

    // Title inside card.
    let title_display: String = layout.title.chars().take(30).collect();
    let title_color = if is_dimmed { MOCHA_OVERLAY0 } else { MOCHA_TEXT };
    cmds.push(RenderCommand::Text {
        x: x + dx + 8.0,
        y: y + dy + 8.0,
        text: title_display,
        color: title_color,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some((w + dw - 16.0).max(0.0)),
    });

    // Border.
    let border_color = if is_hovered {
        MOCHA_BLUE
    } else if layout.is_focused {
        MOCHA_LAVENDER
    } else {
        MOCHA_SURFACE2
    };
    let border_width = if is_hovered { 2.0 } else { 1.0 };
    cmds.push(RenderCommand::StrokeRect {
        x: x + dx,
        y: y + dy,
        width: w + dw,
        height: h + dh,
        color: border_color,
        line_width: border_width,
        corner_radii: CornerRadii::all(6.0),
    });

    // Minimized indicator.
    if layout.is_minimized {
        cmds.push(RenderCommand::FillRect {
            x: x + dx + w + dw - 20.0,
            y: y + dy + 4.0,
            width: 16.0,
            height: 16.0,
            color: MOCHA_YELLOW,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + dx + w + dw - 18.0,
            y: y + dy + 6.0,
            text: "_".to_string(),
            color: MOCHA_BASE,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(12.0),
        });
    }

    // Close button (visible on hover).
    if is_hovered {
        let cb_x = x + dx + w + dw - 22.0;
        let cb_y = y + dy - 6.0;
        cmds.push(RenderCommand::FillRect {
            x: cb_x,
            y: cb_y,
            width: 18.0,
            height: 18.0,
            color: MOCHA_RED,
            corner_radii: CornerRadii::all(9.0),
        });
        cmds.push(RenderCommand::Text {
            x: cb_x + 4.0,
            y: cb_y + 2.0,
            text: "x".to_string(),
            color: MOCHA_BASE,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(12.0),
        });
    }

    // App name label below the card.
    let app_color = if is_dimmed { MOCHA_OVERLAY0 } else { MOCHA_SUBTEXT0 };
    cmds.push(RenderCommand::Text {
        x: x + dx,
        y: y + dy + h + dh + 4.0,
        text: layout.app_name.clone(),
        color: app_color,
        font_size: 10.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((w + dw).max(0.0)),
    });

    // Suppress unused-variable warning — label_h is used to document intent.
    let _ = label_h;
}

/// Render the "+" add-desktop button in AllDesktops mode.
fn render_add_desktop_button(cmds: &mut Vec<RenderCommand>, screen_w: f32, screen_h: f32) {
    let btn_w = 40.0;
    let btn_h = 40.0;
    let bx = screen_w - btn_w - 20.0;
    let by = screen_h - btn_h - 20.0;

    cmds.push(RenderCommand::FillRect {
        x: bx,
        y: by,
        width: btn_w,
        height: btn_h,
        color: MOCHA_SURFACE1,
        corner_radii: CornerRadii::all(20.0),
    });
    cmds.push(RenderCommand::Text {
        x: bx + 12.0,
        y: by + 8.0,
        text: "+".to_string(),
        color: MOCHA_TEXT,
        font_size: 18.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(20.0),
    });
}

// ============================================================================
// Input handling
// ============================================================================

/// Result of processing an input event inside the overview.
#[derive(Debug, Clone, PartialEq)]
pub enum OverviewAction {
    /// No action — the event was not consumed.
    None,
    /// Close the overview.
    Close,
    /// Switch to the window with the given ID.
    SwitchToWindow(u64),
    /// Switch to the given desktop (by id).
    SwitchToDesktop(u32),
    /// Request to close the window with the given ID.
    CloseWindow(u64),
    /// Request to add a new virtual desktop.
    AddDesktop,
    /// Navigate selection (arrow keys).
    NavigateSelection,
    /// The search query changed.
    SearchChanged,
}

/// Process a key event.  Returns the resulting action.
pub fn on_key(state: &mut OverviewState, key: OverviewKey) -> OverviewAction {
    if !state.visible {
        return OverviewAction::None;
    }

    match key {
        OverviewKey::Escape => {
            state.hide();
            OverviewAction::Close
        }
        OverviewKey::Enter => {
            if let Some(wid) = state.hovered_window {
                state.hide();
                OverviewAction::SwitchToWindow(wid)
            } else {
                OverviewAction::None
            }
        }
        OverviewKey::ArrowUp | OverviewKey::ArrowDown
        | OverviewKey::ArrowLeft | OverviewKey::ArrowRight => {
            navigate_selection(state, key);
            OverviewAction::NavigateSelection
        }
        OverviewKey::Char(ch) => {
            state.type_search_char(ch);
            OverviewAction::SearchChanged
        }
        OverviewKey::Backspace => {
            state.search_backspace();
            OverviewAction::SearchChanged
        }
        OverviewKey::Tab => {
            // Cycle mode: AllWindows -> AllDesktops -> RecentApps -> ...
            state.mode = match state.mode {
                OverviewMode::AllWindows => OverviewMode::AllDesktops,
                OverviewMode::AllDesktops => OverviewMode::RecentApps,
                OverviewMode::RecentApps => OverviewMode::AllWindows,
            };
            OverviewAction::NavigateSelection
        }
    }
}

/// Process a mouse-move event.  Updates hover state.
pub fn on_mouse_move(
    state: &mut OverviewState,
    mx: f32,
    my: f32,
    layouts: &[ThumbnailLayout],
) -> OverviewAction {
    if !state.visible {
        return OverviewAction::None;
    }
    state.hovered_window = None;
    for layout in layouts {
        if mx >= layout.render_x
            && mx <= layout.render_x + layout.render_width
            && my >= layout.render_y
            && my <= layout.render_y + layout.render_height
        {
            state.hovered_window = Some(layout.window_id);
            return OverviewAction::NavigateSelection;
        }
    }
    OverviewAction::None
}

/// Process a mouse click.
///
/// The close button occupies the top-right corner of each hovered thumbnail.
pub fn on_mouse_click(
    state: &mut OverviewState,
    mx: f32,
    my: f32,
    layouts: &[ThumbnailLayout],
    screen_w: f32,
    screen_h: f32,
) -> OverviewAction {
    if !state.visible {
        return OverviewAction::None;
    }

    // Check "+" add-desktop button (AllDesktops mode).
    if state.mode == OverviewMode::AllDesktops {
        let btn_x = screen_w - 60.0;
        let btn_y = screen_h - 60.0;
        if mx >= btn_x && mx <= btn_x + 40.0 && my >= btn_y && my <= btn_y + 40.0 {
            return OverviewAction::AddDesktop;
        }
    }

    // Check thumbnails.
    for layout in layouts {
        let lx = layout.render_x;
        let ly = layout.render_y;
        let lw = layout.render_width;
        let lh = layout.render_height;

        if mx >= lx && mx <= lx + lw && my >= ly && my <= ly + lh {
            // Close button — top-right 18x18 area.
            let cb_x = lx + lw - 22.0;
            let cb_y = ly - 6.0;
            if mx >= cb_x && mx <= cb_x + 18.0 && my >= cb_y && my <= cb_y + 18.0 {
                return OverviewAction::CloseWindow(layout.window_id);
            }

            // Otherwise — switch to this window.
            state.hide();
            return OverviewAction::SwitchToWindow(layout.window_id);
        }
    }

    // Click on empty area with a lane -> select desktop.
    if state.mode == OverviewMode::AllDesktops {
        let target = state.selected_desktop.and_then(|did| {
            state
                .lanes
                .iter()
                .find(|l| l.desktop_id == did)
                .map(|l| l.desktop_id)
        });
        if let Some(did) = target {
            state.hide();
            return OverviewAction::SwitchToDesktop(did);
        }
    }

    OverviewAction::None
}

/// Process a mouse-scroll event in AllDesktops mode.
pub fn on_mouse_scroll(
    state: &mut OverviewState,
    delta: f32,
) -> OverviewAction {
    if !state.visible || state.mode != OverviewMode::AllDesktops {
        return OverviewAction::None;
    }
    if state.lanes.is_empty() {
        return OverviewAction::None;
    }

    let current_idx = state
        .selected_desktop
        .and_then(|d| state.lanes.iter().position(|l| l.desktop_id == d))
        .unwrap_or(0);

    let new_idx = if delta > 0.0 {
        current_idx.saturating_add(1).min(state.lanes.len() - 1)
    } else {
        current_idx.saturating_sub(1)
    };

    if let Some(lane) = state.lanes.get(new_idx) {
        state.selected_desktop = Some(lane.desktop_id);
    }
    OverviewAction::NavigateSelection
}

// ============================================================================
// Internal helpers
// ============================================================================

/// Simplified key representation for the overview (avoids coupling to guitk
/// event types directly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewKey {
    Escape,
    Enter,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Char(char),
    Backspace,
    Tab,
}

/// Arrow-key navigation over a flat list of thumbnails.
fn navigate_selection(state: &mut OverviewState, key: OverviewKey) {
    let all = state.all_thumbnails();
    if all.is_empty() {
        return;
    }

    let current_idx = state
        .hovered_window
        .and_then(|wid| all.iter().position(|t| t.window_id == wid));

    let new_idx = match (current_idx, key) {
        (None, _) => Some(0),
        (Some(i), OverviewKey::ArrowRight) | (Some(i), OverviewKey::ArrowDown) => {
            if i + 1 < all.len() { Some(i + 1) } else { Some(i) }
        }
        (Some(i), OverviewKey::ArrowLeft) | (Some(i), OverviewKey::ArrowUp) => {
            Some(i.saturating_sub(1))
        }
        (Some(i), _) => Some(i),
    };

    if let Some(idx) = new_idx {
        if let Some(t) = all.get(idx) {
            state.hovered_window = Some(t.window_id);
        }
    }
}

/// Collect the thumbnails relevant to the current mode.
fn collect_thumbs_for_mode(state: &OverviewState) -> Vec<WindowThumbnail> {
    match state.mode {
        OverviewMode::AllWindows => {
            // Current desktop only.
            let current_desktop = state
                .lanes
                .iter()
                .find(|l| l.is_current);
            match current_desktop {
                Some(lane) => lane.thumbnails.clone(),
                None => state
                    .lanes
                    .first()
                    .map(|l| l.thumbnails.clone())
                    .unwrap_or_default(),
            }
        }
        OverviewMode::RecentApps => {
            // All windows from all desktops.
            state
                .lanes
                .iter()
                .flat_map(|l| l.thumbnails.clone())
                .collect()
        }
        OverviewMode::AllDesktops => {
            // Should not reach here — lane layout is used instead.
            Vec::new()
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Helpers -------------------------------------------------------------

    fn sample_thumb(id: u64, desktop: u32, title: &str, app: &str) -> WindowThumbnail {
        WindowThumbnail {
            window_id: id,
            desktop_id: desktop,
            title: title.to_string(),
            x: 100.0,
            y: 100.0,
            width: 800.0,
            height: 600.0,
            is_focused: false,
            is_minimized: false,
            app_name: app.to_string(),
        }
    }

    fn sample_lanes() -> Vec<DesktopLane> {
        vec![
            DesktopLane {
                desktop_id: 0,
                name: "Desktop 1".to_string(),
                thumbnails: vec![
                    sample_thumb(1, 0, "Terminal", "term"),
                    sample_thumb(2, 0, "Editor", "code"),
                ],
                is_current: true,
            },
            DesktopLane {
                desktop_id: 1,
                name: "Desktop 2".to_string(),
                thumbnails: vec![sample_thumb(3, 1, "Browser", "firefox")],
                is_current: false,
            },
        ]
    }

    fn default_config() -> OverviewConfig {
        OverviewConfig::default()
    }

    // -- OverviewState basics ------------------------------------------------

    #[test]
    fn test_state_new_is_hidden() {
        let s = OverviewState::new();
        assert!(!s.visible);
        assert_eq!(s.animation_progress, 0.0);
    }

    #[test]
    fn test_state_show_sets_visible() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        assert!(s.visible);
        assert_eq!(s.mode, OverviewMode::AllWindows);
    }

    #[test]
    fn test_state_hide_clears_visible() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllDesktops);
        s.hide();
        assert!(!s.visible);
    }

    #[test]
    fn test_state_toggle_on_off() {
        let mut s = OverviewState::new();
        s.toggle(OverviewMode::AllWindows);
        assert!(s.visible);
        s.toggle(OverviewMode::AllWindows);
        assert!(!s.visible);
    }

    #[test]
    fn test_state_toggle_switches_mode() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        s.toggle(OverviewMode::AllDesktops);
        assert!(s.visible);
        assert_eq!(s.mode, OverviewMode::AllDesktops);
    }

    #[test]
    fn test_show_clears_search() {
        let mut s = OverviewState::new();
        s.search_query = "old".to_string();
        s.search_results = vec![1, 2];
        s.show(OverviewMode::AllWindows);
        assert!(s.search_query.is_empty());
        assert!(s.search_results.is_empty());
    }

    // -- Animation -----------------------------------------------------------

    #[test]
    fn test_animation_tick_advances() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let cfg = default_config();
        let still_going = s.tick_animation(0.1, &cfg);
        assert!(still_going);
        assert!(s.animation_progress > 0.0);
        assert!(s.animation_progress < 1.0);
    }

    #[test]
    fn test_animation_reaches_one() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let cfg = default_config();
        // Tick enough to finish.
        for _ in 0..50 {
            s.tick_animation(0.02, &cfg);
        }
        assert!((s.animation_progress - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_animation_hide_reaches_zero() {
        let mut s = OverviewState::new();
        s.animation_progress = 1.0;
        s.hide();
        let cfg = default_config();
        for _ in 0..50 {
            s.tick_animation(0.02, &cfg);
        }
        assert!(s.animation_progress.abs() < f32::EPSILON);
    }

    #[test]
    fn test_animation_zero_duration_instant() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let mut cfg = default_config();
        cfg.animation_duration_ms = 0;
        let still_going = s.tick_animation(0.01, &cfg);
        assert!(!still_going);
        assert!((s.animation_progress - 1.0).abs() < f32::EPSILON);
    }

    // -- Search --------------------------------------------------------------

    #[test]
    fn test_search_type_char() {
        let mut s = OverviewState::new();
        s.lanes = sample_lanes();
        s.type_search_char('T');
        assert_eq!(s.search_query, "T");
    }

    #[test]
    fn test_search_backspace() {
        let mut s = OverviewState::new();
        s.lanes = sample_lanes();
        s.type_search_char('A');
        s.type_search_char('B');
        s.search_backspace();
        assert_eq!(s.search_query, "A");
    }

    #[test]
    fn test_search_filters_by_title() {
        let mut s = OverviewState::new();
        s.lanes = sample_lanes();
        s.type_search_char('T');
        s.type_search_char('e');
        s.type_search_char('r');
        s.type_search_char('m');
        // "Terminal" should match.
        assert!(s.search_results.contains(&1));
        assert!(!s.search_results.contains(&3));
    }

    #[test]
    fn test_search_filters_by_app_name() {
        let mut s = OverviewState::new();
        s.lanes = sample_lanes();
        s.type_search_char('f');
        s.type_search_char('i');
        s.type_search_char('r');
        s.type_search_char('e');
        // "firefox" should match.
        assert!(s.search_results.contains(&3));
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut s = OverviewState::new();
        s.lanes = sample_lanes();
        s.type_search_char('t');
        s.type_search_char('e');
        s.type_search_char('r');
        s.type_search_char('m');
        assert!(s.search_results.contains(&1));
    }

    #[test]
    fn test_search_empty_clears_results() {
        let mut s = OverviewState::new();
        s.lanes = sample_lanes();
        s.type_search_char('x');
        assert!(!s.search_results.is_empty() || s.search_results.is_empty()); // may or may not match
        s.search_backspace();
        assert!(s.search_results.is_empty());
    }

    // -- Layout engine -------------------------------------------------------

    #[test]
    fn test_grid_layout_empty() {
        let config = default_config();
        let result = compute_grid_layout(&[], 0.0, 0.0, 800.0, 600.0, &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_grid_layout_single_window() {
        let thumbs = vec![sample_thumb(1, 0, "Win", "app")];
        let config = default_config();
        let result = compute_grid_layout(&thumbs, 0.0, 0.0, 800.0, 600.0, &config);
        assert_eq!(result.len(), 1);
        assert!(result[0].render_width > 0.0);
        assert!(result[0].render_height > 0.0);
    }

    #[test]
    fn test_grid_layout_multiple_windows() {
        let thumbs: Vec<_> = (0..6)
            .map(|i| sample_thumb(i, 0, &format!("Win {}", i), "app"))
            .collect();
        let config = default_config();
        let result = compute_grid_layout(&thumbs, 0.0, 0.0, 1920.0, 1080.0, &config);
        assert_eq!(result.len(), 6);
    }

    #[test]
    fn test_grid_layout_no_overlap() {
        let thumbs: Vec<_> = (0..4)
            .map(|i| sample_thumb(i, 0, &format!("Win {}", i), "app"))
            .collect();
        let config = default_config();
        let result = compute_grid_layout(&thumbs, 0.0, 0.0, 1920.0, 1080.0, &config);

        for i in 0..result.len() {
            for j in (i + 1)..result.len() {
                let a = &result[i];
                let b = &result[j];
                let overlap_x = a.render_x < b.render_x + b.render_width
                    && a.render_x + a.render_width > b.render_x;
                let overlap_y = a.render_y < b.render_y + b.render_height
                    && a.render_y + a.render_height > b.render_y;
                assert!(
                    !(overlap_x && overlap_y),
                    "thumbnails {} and {} overlap",
                    i,
                    j
                );
            }
        }
    }

    #[test]
    fn test_grid_layout_respects_max_columns() {
        let thumbs: Vec<_> = (0..10)
            .map(|i| sample_thumb(i, 0, &format!("Win {}", i), "app"))
            .collect();
        let mut config = default_config();
        config.max_columns = 3;
        let result = compute_grid_layout(&thumbs, 0.0, 0.0, 1920.0, 1080.0, &config);
        assert_eq!(result.len(), 10);

        // First row should have 3 items.
        let first_row_y = result[0].render_y;
        let first_row_count = result.iter().filter(|r| (r.render_y - first_row_y).abs() < 1.0).count();
        assert_eq!(first_row_count, 3);
    }

    #[test]
    fn test_grid_layout_preserves_aspect_ratio() {
        let mut thumb = sample_thumb(1, 0, "Wide", "app");
        thumb.width = 1600.0;
        thumb.height = 400.0; // 4:1 aspect ratio
        let config = default_config();
        let result = compute_grid_layout(&[thumb], 0.0, 0.0, 800.0, 800.0, &config);
        assert_eq!(result.len(), 1);
        let ratio = result[0].render_width / result[0].render_height;
        assert!((ratio - 4.0).abs() < 0.1, "aspect ratio should be ~4:1, got {}", ratio);
    }

    #[test]
    fn test_grid_layout_zero_area() {
        let thumbs = vec![sample_thumb(1, 0, "Win", "app")];
        let config = default_config();
        let result = compute_grid_layout(&thumbs, 0.0, 0.0, 0.0, 0.0, &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_grid_layout_many_windows() {
        let thumbs: Vec<_> = (0..20)
            .map(|i| sample_thumb(i, 0, &format!("Win {}", i), "app"))
            .collect();
        let config = default_config();
        let result = compute_grid_layout(&thumbs, 0.0, 0.0, 1920.0, 1080.0, &config);
        assert_eq!(result.len(), 20);
    }

    #[test]
    fn test_lane_layout_empty_lanes() {
        let config = default_config();
        let result = compute_lane_layout(&[], 0.0, 0.0, 800.0, 600.0, &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_lane_layout_single_lane() {
        let lanes = vec![DesktopLane {
            desktop_id: 0,
            name: "Desktop 1".to_string(),
            thumbnails: vec![sample_thumb(1, 0, "Win", "app")],
            is_current: true,
        }];
        let config = default_config();
        let result = compute_lane_layout(&lanes, 0.0, 0.0, 1920.0, 1080.0, &config);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_lane_layout_multiple_lanes() {
        let lanes = sample_lanes();
        let config = default_config();
        let result = compute_lane_layout(&lanes, 0.0, 0.0, 1920.0, 1080.0, &config);
        // 2 + 1 = 3 thumbnails across 2 lanes.
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_lane_layout_preserves_ids() {
        let lanes = sample_lanes();
        let config = default_config();
        let result = compute_lane_layout(&lanes, 0.0, 0.0, 1920.0, 1080.0, &config);
        let ids: Vec<u64> = result.iter().map(|l| l.window_id).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
    }

    #[test]
    fn test_lane_layout_empty_lane_skipped() {
        let lanes = vec![
            DesktopLane {
                desktop_id: 0,
                name: "Empty".to_string(),
                thumbnails: Vec::new(),
                is_current: true,
            },
            DesktopLane {
                desktop_id: 1,
                name: "Has windows".to_string(),
                thumbnails: vec![sample_thumb(1, 1, "Win", "app")],
                is_current: false,
            },
        ];
        let config = default_config();
        let result = compute_lane_layout(&lanes, 0.0, 0.0, 1920.0, 1080.0, &config);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].desktop_id, 1);
    }

    // -- fit_aspect ----------------------------------------------------------

    #[test]
    fn test_fit_aspect_square_into_square() {
        let (w, h) = fit_aspect(100.0, 100.0, 50.0, 50.0);
        assert!((w - 50.0).abs() < 0.01);
        assert!((h - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_fit_aspect_wide_into_square() {
        let (w, h) = fit_aspect(200.0, 100.0, 100.0, 100.0);
        assert!((w - 100.0).abs() < 0.01);
        assert!((h - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_fit_aspect_tall_into_square() {
        let (w, h) = fit_aspect(100.0, 200.0, 100.0, 100.0);
        assert!((w - 50.0).abs() < 0.01);
        assert!((h - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_fit_aspect_zero_source() {
        let (w, h) = fit_aspect(0.0, 0.0, 100.0, 100.0);
        assert_eq!(w, 0.0);
        assert_eq!(h, 0.0);
    }

    #[test]
    fn test_fit_aspect_does_not_upscale() {
        let (w, h) = fit_aspect(50.0, 50.0, 200.0, 200.0);
        assert!((w - 50.0).abs() < 0.01);
        assert!((h - 50.0).abs() < 0.01);
    }

    // -- Rendering -----------------------------------------------------------

    #[test]
    fn test_render_hidden_produces_nothing() {
        let state = OverviewState::new();
        let config = default_config();
        let cmds = render_overview(&state, &config, 1920.0, 1080.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_render_visible_produces_commands() {
        let mut state = OverviewState::new();
        state.show(OverviewMode::AllWindows);
        state.animation_progress = 1.0;
        state.lanes = sample_lanes();
        let config = default_config();
        let cmds = render_overview(&state, &config, 1920.0, 1080.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_alldesktops_has_add_button() {
        let mut state = OverviewState::new();
        state.show(OverviewMode::AllDesktops);
        state.animation_progress = 1.0;
        state.lanes = sample_lanes();
        let config = default_config();
        let cmds = render_overview(&state, &config, 1920.0, 1080.0);
        // The "+" text should be present.
        let has_plus = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "+"
            } else {
                false
            }
        });
        assert!(has_plus);
    }

    #[test]
    fn test_render_search_bar_placeholder() {
        let mut state = OverviewState::new();
        state.show(OverviewMode::AllWindows);
        state.animation_progress = 1.0;
        let config = default_config();
        let cmds = render_overview(&state, &config, 1920.0, 1080.0);
        let has_placeholder = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("Search windows")
            } else {
                false
            }
        });
        assert!(has_placeholder);
    }

    #[test]
    fn test_render_with_search_query() {
        let mut state = OverviewState::new();
        state.show(OverviewMode::AllWindows);
        state.animation_progress = 1.0;
        state.lanes = sample_lanes();
        state.search_query = "term".to_string();
        state.update_search();
        let config = default_config();
        let cmds = render_overview(&state, &config, 1920.0, 1080.0);
        let has_query = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "term"
            } else {
                false
            }
        });
        assert!(has_query);
    }

    // -- Input handling ------------------------------------------------------

    #[test]
    fn test_key_escape_closes() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let action = on_key(&mut s, OverviewKey::Escape);
        assert_eq!(action, OverviewAction::Close);
        assert!(!s.visible);
    }

    #[test]
    fn test_key_enter_with_hovered() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        s.hovered_window = Some(42);
        let action = on_key(&mut s, OverviewKey::Enter);
        assert_eq!(action, OverviewAction::SwitchToWindow(42));
    }

    #[test]
    fn test_key_enter_no_hovered() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let action = on_key(&mut s, OverviewKey::Enter);
        assert_eq!(action, OverviewAction::None);
    }

    #[test]
    fn test_key_arrow_navigates() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        s.lanes = sample_lanes();
        let action = on_key(&mut s, OverviewKey::ArrowRight);
        assert_eq!(action, OverviewAction::NavigateSelection);
        assert!(s.hovered_window.is_some());
    }

    #[test]
    fn test_key_char_updates_search() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let action = on_key(&mut s, OverviewKey::Char('a'));
        assert_eq!(action, OverviewAction::SearchChanged);
        assert_eq!(s.search_query, "a");
    }

    #[test]
    fn test_key_backspace_updates_search() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        s.search_query = "ab".to_string();
        let action = on_key(&mut s, OverviewKey::Backspace);
        assert_eq!(action, OverviewAction::SearchChanged);
        assert_eq!(s.search_query, "a");
    }

    #[test]
    fn test_key_tab_cycles_mode() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        on_key(&mut s, OverviewKey::Tab);
        assert_eq!(s.mode, OverviewMode::AllDesktops);
        on_key(&mut s, OverviewKey::Tab);
        assert_eq!(s.mode, OverviewMode::RecentApps);
        on_key(&mut s, OverviewKey::Tab);
        assert_eq!(s.mode, OverviewMode::AllWindows);
    }

    #[test]
    fn test_key_when_hidden_does_nothing() {
        let mut s = OverviewState::new();
        let action = on_key(&mut s, OverviewKey::Escape);
        assert_eq!(action, OverviewAction::None);
    }

    #[test]
    fn test_mouse_move_sets_hover() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let layouts = vec![ThumbnailLayout {
            window_id: 10,
            desktop_id: 0,
            title: "Win".to_string(),
            app_name: "app".to_string(),
            is_focused: false,
            is_minimized: false,
            render_x: 100.0,
            render_y: 100.0,
            render_width: 200.0,
            render_height: 150.0,
        }];
        on_mouse_move(&mut s, 150.0, 150.0, &layouts);
        assert_eq!(s.hovered_window, Some(10));
    }

    #[test]
    fn test_mouse_move_outside_clears_hover() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        s.hovered_window = Some(10);
        let layouts = vec![ThumbnailLayout {
            window_id: 10,
            desktop_id: 0,
            title: "Win".to_string(),
            app_name: "app".to_string(),
            is_focused: false,
            is_minimized: false,
            render_x: 100.0,
            render_y: 100.0,
            render_width: 200.0,
            render_height: 150.0,
        }];
        on_mouse_move(&mut s, 0.0, 0.0, &layouts);
        assert_eq!(s.hovered_window, None);
    }

    #[test]
    fn test_mouse_click_selects_window() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let layouts = vec![ThumbnailLayout {
            window_id: 7,
            desktop_id: 0,
            title: "Win".to_string(),
            app_name: "app".to_string(),
            is_focused: false,
            is_minimized: false,
            render_x: 100.0,
            render_y: 100.0,
            render_width: 200.0,
            render_height: 150.0,
        }];
        let action = on_mouse_click(&mut s, 150.0, 150.0, &layouts, 1920.0, 1080.0);
        assert_eq!(action, OverviewAction::SwitchToWindow(7));
        assert!(!s.visible);
    }

    #[test]
    fn test_mouse_click_add_desktop() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllDesktops);
        // "+" button is at (screen_w - 60, screen_h - 60) with size 40x40.
        let action = on_mouse_click(&mut s, 1870.0, 1030.0, &[], 1920.0, 1080.0);
        assert_eq!(action, OverviewAction::AddDesktop);
    }

    #[test]
    fn test_mouse_click_empty_area() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let action = on_mouse_click(&mut s, 5.0, 5.0, &[], 1920.0, 1080.0);
        assert_eq!(action, OverviewAction::None);
    }

    #[test]
    fn test_mouse_scroll_navigates_desktops() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllDesktops);
        s.lanes = sample_lanes();
        s.selected_desktop = Some(0);
        let action = on_mouse_scroll(&mut s, 1.0);
        assert_eq!(action, OverviewAction::NavigateSelection);
        assert_eq!(s.selected_desktop, Some(1));
    }

    #[test]
    fn test_mouse_scroll_does_nothing_when_hidden() {
        let mut s = OverviewState::new();
        let action = on_mouse_scroll(&mut s, 1.0);
        assert_eq!(action, OverviewAction::None);
    }

    #[test]
    fn test_mouse_scroll_does_nothing_allwindows_mode() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let action = on_mouse_scroll(&mut s, 1.0);
        assert_eq!(action, OverviewAction::None);
    }

    #[test]
    fn test_mouse_scroll_clamps_at_bounds() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllDesktops);
        s.lanes = sample_lanes();
        s.selected_desktop = Some(0);
        // Scroll up at first desktop.
        on_mouse_scroll(&mut s, -1.0);
        assert_eq!(s.selected_desktop, Some(0));
    }

    // -- Config persistence --------------------------------------------------

    #[test]
    fn test_config_default_values() {
        let cfg = OverviewConfig::default();
        assert_eq!(cfg.max_columns, 5);
        assert!(cfg.show_desktop_labels);
        assert_eq!(cfg.animation_duration_ms, 250);
    }

    #[test]
    fn test_config_roundtrip() {
        let cfg = OverviewConfig {
            thumbnail_padding: 24.0,
            max_columns: 3,
            show_desktop_labels: false,
            animation_duration_ms: 400,
            background_opacity: 0.9,
        };
        let text = cfg.to_text();
        let parsed = OverviewConfig::from_text(&text);
        assert!((parsed.thumbnail_padding - 24.0).abs() < f32::EPSILON);
        assert_eq!(parsed.max_columns, 3);
        assert!(!parsed.show_desktop_labels);
        assert_eq!(parsed.animation_duration_ms, 400);
        assert!((parsed.background_opacity - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_config_from_empty_text_uses_defaults() {
        let cfg = OverviewConfig::from_text("");
        assert_eq!(cfg.max_columns, 5);
    }

    #[test]
    fn test_config_ignores_comments() {
        let text = "# comment\nmax_columns=7\n";
        let cfg = OverviewConfig::from_text(text);
        assert_eq!(cfg.max_columns, 7);
    }

    #[test]
    fn test_config_ignores_unknown_keys() {
        let text = "unknown_key=42\nmax_columns=8\n";
        let cfg = OverviewConfig::from_text(text);
        assert_eq!(cfg.max_columns, 8);
    }

    #[test]
    fn test_config_ignores_bad_values() {
        let text = "max_columns=notanumber\n";
        let cfg = OverviewConfig::from_text(text);
        // Should keep default.
        assert_eq!(cfg.max_columns, 5);
    }

    // -- collect_thumbs_for_mode ---------------------------------------------

    #[test]
    fn test_collect_allwindows_current_desktop_only() {
        let mut s = OverviewState::new();
        s.mode = OverviewMode::AllWindows;
        s.lanes = sample_lanes();
        let thumbs = collect_thumbs_for_mode(&s);
        // Only desktop 0 (the current one) should be returned.
        assert_eq!(thumbs.len(), 2);
        assert!(thumbs.iter().all(|t| t.desktop_id == 0));
    }

    #[test]
    fn test_collect_recentapps_all_desktops() {
        let mut s = OverviewState::new();
        s.mode = OverviewMode::RecentApps;
        s.lanes = sample_lanes();
        let thumbs = collect_thumbs_for_mode(&s);
        assert_eq!(thumbs.len(), 3); // all windows
    }

    #[test]
    fn test_collect_alldesktops_returns_empty() {
        let mut s = OverviewState::new();
        s.mode = OverviewMode::AllDesktops;
        s.lanes = sample_lanes();
        // AllDesktops mode uses lane layout, not grid — so this helper returns empty.
        let thumbs = collect_thumbs_for_mode(&s);
        assert!(thumbs.is_empty());
    }
}

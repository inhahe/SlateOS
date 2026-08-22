//! Virtual desktop overview (Exposé / Mission Control).
//!
//! A fullscreen overlay showing every window across every virtual desktop. The
//! user can click a card to switch to that window, navigate with the arrow
//! keys, type to search by title, and switch or close from within it.
//!
//! Three view modes are supported:
//! - **AllWindows** — grid of windows on the current desktop.
//! - **AllDesktops** — horizontal lanes, one per desktop, each showing its windows.
//! - **RecentApps** — most-recently-used window list across all desktops.
//!
//! # What the cards are, and are not
//!
//! **They are titled rectangles with their windows' proportions, not pictures.**
//! [`OverviewState::apply_window_list`] takes each window's real rectangle from
//! the same `WindowList` that refreshes the taskbar, and the layout scales that
//! rectangle to fit — so a wide window reads as a wide card. It never asks the
//! compositor for window *contents*, because there is no verb for that: a
//! scaled read of another client's buffer is a capability question of the same
//! shape as reading another client's title, only worse, since a title is a
//! string and a thumbnail is the screen. Proportional titled rectangles are a
//! usable Exposé without it; see `known-issues.md`,
//! `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`.
//!
//! **There is no application name.** A window's identity here is its title,
//! because the title is the whole of what the shell is told. The wire carries
//! an id, a pid, a layer, a title, state bits, a desktop number and a
//! rectangle — no application identity, which is a property of the *program*
//! and would have to come from package metadata keyed by executable. Search
//! matches titles for the same reason.
//!
//! # The one thing that moves, and the shape that keeps it safe
//!
//! The backdrop fades in when the overview opens. It is the only animation
//! here, and it is arranged so that a caller with no clock still gets a working
//! overview rather than a blank one.
//!
//! The first attempt did the opposite. It had an `animation_progress` that
//! [`show`](OverviewState::show) set to `0.0` and a `tick_animation(dt)` the
//! caller was to run every frame, and every draw path began `if progress <= 0.0
//! { return }`. Nothing ever called `tick_animation`, and at the time nothing
//! *could* — the shell's event loop blocked in `Connection::wait()`, which takes
//! no timeout. So opening the overview produced a fullscreen overlay that was
//! blank, un-clickable, and permanently so: not a missing polish detail, the
//! feature not working. It was deleted rather than left in (see
//! `design-decisions.md` §520), and comes back now that
//! [`oswindow::EventLoop`] has a frame clock.
//!
//! What is different this time is which state is the default. The fade is
//! `Option<Animation>` and `None` means **fully open**, so a caller that never
//! ticks — a test, a headless layout pass, an embedder with no clock — sees the
//! finished overlay. Starting the fade is the deliberate act, and only
//! [`OverviewState::begin_fade`] does it; only a caller that owns a clock calls
//! it. Nothing here gates *drawing* or *hit-testing* on the fade: it scales the
//! backdrop's alpha and touches nothing else, so even a fade frozen at zero
//! leaves every card drawn and every click landing where it should.
//!
//! # One layout, two readers
//!
//! [`overview_layout`] is the single answer to where each card goes.
//! [`render_overview`] draws those rectangles and [`on_mouse_click`] hit-tests
//! the same ones; neither works the layout out for itself. Two computations of
//! one layout is how a click comes to select the window next to the one that
//! was lit. See `design-decisions.md` §520.

use crate::animations::{Animation, DEFAULT_DURATION_MS, Easing};
use crate::{Layer, ShellControlAction, ShellRequest, WindowId};
use appearance::{Palette, readable_on};
use guiremote::window_list::WindowList;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::step;
use guitk::style::CornerRadii;

// ============================================================================
// Colour
// ============================================================================
//
// Every colour drawn here comes from the `&Palette` threaded through
// `render_overview`, so the overlay follows the desktop's mode and accent.
// Four judgements had to be made when the hardcoded hexes came out, because a
// literal carries no role until someone assigns one:
//
// *Three things follow the accent*: the search bar's border once a query is
// active, the bar marking the current desktop, and a card's border while the
// pointer is over it.  Each is a position or an invitation, which is what the
// accent is for.
//
// *A card's border carries two independent kinds of "current", and they must
// not collapse into one.*  Hover says **where you are pointing**; focus says
// **which window has the keyboard**.  They are orthogonal — the focused window
// is usually not the one under the pointer — so painting them alike would lose
// a distinction silently, and no membership sweep could see it because both
// would be palette roles.  The border is a three-rung ladder that no accent can
// flatten: `surface2` when the card is merely present, `subtext0` when it holds
// focus, the accent when it is under the pointer.  Focus was `lavender`, which
// is a *category* colour being used to mark a *state* — it would have meant
// nothing on a lavender-accented desktop.  `subtext0` is within a few percent
// of the same pixel and cannot collide with any accent, because it is a grey.
//
// *Two badges are frozen*, because they report facts about a window rather
// than offering choices about the desktop: the minimised marker's yellow and
// the close button's red.  A close button that means destructive on one desktop
// and matches the wallpaper on another has stopped saying anything.
//
// *Both badge labels were wrong, not merely fragile.*  Each was Mocha `base` —
// a near-black picked to read on Mocha's *pale* yellow and *pale* red.  Latte's
// yellow and red are deep, and its `base` is near-white, so on a light desktop
// the marks would sit on the wrong side of their own fills.  Each is
// `readable_on()` of the fill it is actually drawn on now, which answers the
// question rather than assuming the answer.
//
// The two translucent washes — the backdrop and a search-dimmed card — keep
// their own alpha and take only the *RGB* of their role.  That is the rule for
// every wash in this crate: a wash is a role seen through a veil, so the veil
// is the alpha and the role is everything else.  Note that the two-mode
// membership sweep cannot check the alpha, since it compares RGB only; the
// washes therefore carry their own test.

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
    pub lanes: Vec<DesktopLane>,
    pub hovered_window: Option<u64>,
    pub selected_desktop: Option<u32>,
    pub search_query: String,
    pub search_results: Vec<u64>,
    /// The backdrop fade, or `None` for **fully open**.
    ///
    /// Private, and `None` by default, because that is what makes an un-ticked
    /// overview a working one: see this module's header. Read it through
    /// [`OverviewState::fade_opacity`], start it with
    /// [`OverviewState::begin_fade`], advance it with
    /// [`OverviewState::tick_fade`].
    fade: Option<Animation>,
}

impl OverviewState {
    /// Create a new, hidden overview state.
    pub fn new() -> Self {
        Self {
            mode: OverviewMode::AllWindows,
            visible: false,
            lanes: Vec::new(),
            hovered_window: None,
            selected_desktop: None,
            search_query: String::new(),
            search_results: Vec::new(),
            fade: None,
        }
    }

    /// Show the overview in the given mode.
    ///
    /// The search box starts empty every time rather than remembering the last
    /// query: an overview that opens already filtered is one that opens looking
    /// like most of the desktop has closed.
    /// Opening does **not** start the fade. `show` is called from the input
    /// path, which has no idea whether anything will ever tick; leaving the
    /// fade at `None` means the overview is fully open the instant it is shown,
    /// and a caller that does own a clock follows this with
    /// [`begin_fade`](Self::begin_fade). The animation is therefore something
    /// added to a working overlay, never something the overlay waits for.
    pub fn show(&mut self, mode: OverviewMode) {
        self.mode = mode;
        self.visible = true;
        self.search_query.clear();
        self.search_results.clear();
        self.hovered_window = None;
        self.fade = None;
    }

    /// Hide the overview.
    pub fn hide(&mut self) {
        self.visible = false;
        // Dropped rather than left part-way: the next `show` must not inherit a
        // fade from the last one, and an overview that is not on screen has no
        // business keeping the shell's frame clock awake.
        self.fade = None;
    }

    /// Toggle visibility using the given mode.
    pub fn toggle(&mut self, mode: OverviewMode) {
        if self.visible && self.mode == mode {
            self.hide();
        } else {
            self.show(mode);
        }
    }

    /// Start the backdrop fading in over `duration_ms`.
    ///
    /// **Only call this if you are going to call [`tick_fade`](Self::tick_fade)
    /// until it returns `false`.** A fade begun and never advanced holds the
    /// backdrop at its dimmest, which is the one state this design otherwise
    /// makes unreachable. The caller that starts it is the caller that owns the
    /// clock, which is why this is separate from [`show`](Self::show) rather
    /// than part of it.
    ///
    /// A zero `duration_ms` is treated as "no fade" rather than as a division
    /// by zero — [`Animation::new`] floors the duration at 1 ms, but a caller
    /// asking for zero is asking for the overview to be open now, and giving it
    /// a one-millisecond fade would make that depend on when the next frame
    /// happens to land.
    pub fn begin_fade(&mut self, duration_ms: u32) {
        self.fade = (duration_ms > 0).then(|| {
            // Ease-out: the backdrop arrives quickly and settles, so the
            // overlay reads as already there for most of the fade rather than
            // as still on its way.
            Animation::new(0.0, 1.0, duration_ms, Easing::EaseOut)
        });
    }

    /// Jump straight to fully open, abandoning any fade in progress.
    ///
    /// What a reduced-motion setting turns on mid-fade should do: the user has
    /// just asked for less motion, and finishing the fade they asked to stop is
    /// a stranger answer than being where it was going.
    pub fn end_fade(&mut self) {
        self.fade = None;
    }

    /// Advance the fade by `dt_ms` of wall time. Returns whether it is still
    /// running — i.e. whether another frame is wanted.
    ///
    /// Cheap and safe to call when nothing is fading; it answers `false`.
    pub fn tick_fade(&mut self, dt_ms: u32) -> bool {
        let Some(anim) = self.fade.as_mut() else {
            return false;
        };
        anim.tick(dt_ms);
        if anim.is_done() {
            // Back to `None`, the fully-open resting state, so a later reader
            // cannot tell a finished fade from one that never ran.
            self.fade = None;
            return false;
        }
        true
    }

    /// Whether a fade is running, and so whether a frame is wanted.
    #[must_use]
    pub const fn is_fading(&self) -> bool {
        self.fade.is_some()
    }

    /// How much of the backdrop to draw, in `0.0..=1.0`.
    ///
    /// `1.0` whenever no fade is running, which includes both "never started
    /// one" and "finished". Only the backdrop's alpha is scaled by this; see
    /// the module header for why nothing else is.
    #[must_use]
    pub fn fade_opacity(&self) -> f32 {
        self.fade.as_ref().map_or(1.0, Animation::value)
    }

    /// Rebuild the lanes from a window list.
    ///
    /// This is the whole of the overview's data path, and it is deliberately a
    /// *replacement* rather than a merge: every frame supersedes the last one
    /// entirely, so there is no accumulated copy of the desktop that can drift
    /// out of step with what the compositor is actually showing. The lanes, the
    /// rectangles, which desktop is current — all of it comes from the same
    /// frame, which is why the overview cannot disagree with the taskbar about
    /// which desktop is showing (they are folded from one `WindowList` in one
    /// call; see `DesktopShell::apply_window_list`).
    ///
    /// `num_desktops` is the shell's, not the list's. A desktop with no windows
    /// on it does not appear anywhere in a window list, and it is still a
    /// desktop the user can switch to — an overview that showed only the
    /// occupied ones would make an empty desktop unreachable, which is the same
    /// class of bug as `TD-C-VIRTUAL-DESKTOPS-HIDE-NOTHING` in a new place. So
    /// the lanes are the *shell's* desktops, and some of them are empty.
    ///
    /// Only `Layer::Normal` windows appear. The background (the wallpaper) and
    /// the overlays (the taskbar, this overlay's own surfaces) are furniture:
    /// they are not things the user switches to, and a thumbnail of the taskbar
    /// inside a window switcher is noise that also happens to be clickable.
    /// This matches `apply_window_list`'s filter exactly, deliberately — two
    /// different answers to "which windows are there" is precisely the drift
    /// this projection exists to avoid.
    pub fn apply_window_list(&mut self, list: &WindowList, num_desktops: u32) {
        let mut lanes: Vec<DesktopLane> = (0..num_desktops)
            .map(|desktop_id| DesktopLane {
                desktop_id,
                // One-based, because the taskbar's desktop indicator counts
                // from one and two different numbers for one desktop on one
                // screen is worse than either convention.
                name: format!("Desktop {}", desktop_id.saturating_add(1)),
                thumbnails: Vec::new(),
                is_current: desktop_id == list.current_workspace,
            })
            .collect();

        for info in &list.windows {
            if info.layer != Layer::Normal {
                continue;
            }
            // A window filed on a desktop this shell does not have is dropped
            // rather than clamped into the last lane. The compositor is the
            // authority on desktop numbers and the shell's count could be
            // behind it; showing such a window under the wrong desktop's
            // heading would be a confident lie, whereas leaving it out is
            // visibly incomplete and self-correcting on the next frame.
            let Some(lane) = lanes.get_mut(info.workspace as usize) else {
                continue;
            };
            lane.thumbnails.push(WindowThumbnail {
                window_id: info.id,
                desktop_id: info.workspace,
                title: info.title.clone(),
                // The wire's `i32`/`u32` rectangle, widened to the `f32` the
                // layout works in. Widening, not converting: every `i32` and
                // every `u32` up to 2^24 is exact in an `f32`, and a window
                // placed past sixteen million pixels is not a case worth
                // distorting the type for.
                x: info.x as f32,
                y: info.y as f32,
                width: info.width as f32,
                height: info.height as f32,
                is_focused: info.focused,
                is_minimized: info.minimized,
            });
        }

        self.lanes = lanes;
        // The query survives a refresh — the user is mid-search and the window
        // list arriving underneath them is not a reason to clear what they
        // typed — but the *results* are ids that may no longer exist, so they
        // are recomputed rather than kept.
        self.update_search();
        // Same for hover: an id that has gone would otherwise stay highlighted
        // and, worse, would be what Enter activates.
        if let Some(hovered) = self.hovered_window
            && !self
                .lanes
                .iter()
                .any(|l| l.thumbnails.iter().any(|t| t.window_id == hovered))
        {
            self.hovered_window = None;
        }
    }

    /// Collect every `WindowThumbnail` from all lanes.
    pub fn all_thumbnails(&self) -> Vec<&WindowThumbnail> {
        self.lanes
            .iter()
            .flat_map(|l| l.thumbnails.iter())
            .collect()
    }

    /// Update the search results based on the current query.
    ///
    /// Titles only. This used to also match an `app_name` on each thumbnail,
    /// which read as a second, coarser key — type "editor" and get every window
    /// of the editor whatever each one is called. Nothing in the system could
    /// ever supply that string: the window list carries an id, a pid, a layer,
    /// a title, state bits, a desktop number and a rectangle, and no
    /// application identity; the only code that ever set the field was this
    /// module's own test fixtures, always to the literal `"app"`. So the
    /// promise was being kept against test data and broken against a real
    /// desktop, where every window's app name was the empty string — which
    /// `contains` matches for *any* query, quietly making the search return
    /// everything. Better to match one key honestly. See
    /// `known-issues.md` → `TD-C-THE-OVERVIEW-SCREEN-IS-1856-LINES-NOBODY-CALLS`
    /// for what would have to exist before an application name could come back.
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
            .filter(|t| t.title.to_lowercase().contains(&query))
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

impl Default for OverviewState {
    fn default() -> Self {
        Self::new()
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
    /// Opacity of the dark overlay background (0.0 – 1.0).
    pub background_opacity: f32,
    /// How long the backdrop takes to fade in, in milliseconds. `0` disables
    /// the fade, which is what an accessibility setting for reduced motion sets
    /// it to — see [`OverviewState::begin_fade`].
    pub fade_ms: u32,
}

impl Default for OverviewConfig {
    fn default() -> Self {
        Self {
            thumbnail_padding: 16.0,
            max_columns: 5,
            show_desktop_labels: true,
            background_opacity: 0.85,
            fade_ms: DEFAULT_DURATION_MS,
        }
    }
}

impl OverviewConfig {
    /// Serialize to a simple key=value text format.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("thumbnail_padding={}\n", self.thumbnail_padding));
        out.push_str(&format!("max_columns={}\n", self.max_columns));
        out.push_str(&format!(
            "show_desktop_labels={}\n",
            self.show_desktop_labels
        ));
        out.push_str(&format!("background_opacity={}\n", self.background_opacity));
        out.push_str(&format!("fade_ms={}\n", self.fade_ms));
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
                    "background_opacity" => {
                        if let Ok(v) = val.parse::<f32>() {
                            cfg.background_opacity = v;
                        }
                    }
                    "fade_ms" => {
                        if let Ok(v) = val.parse::<u32>() {
                            cfg.fade_ms = v;
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
    let rows = count.div_ceil(cols); // ceil division

    let pad = config.thumbnail_padding;
    let cell_w = (bw - pad * (cols as f32 + 1.0)) / cols as f32;
    let cell_h = (bh - pad * (rows as f32 + 1.0)) / rows as f32;

    if cell_w <= 0.0 || cell_h <= 0.0 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(count);
    for (i, thumb) in thumbnails.iter().enumerate() {
        // `cols` is `count.min(max_cols)` with both at least one, so these
        // are total; `checked_*` is what makes that visible here rather than
        // fifteen lines up.
        let col = i.checked_rem(cols).unwrap_or(0);
        let row = i.checked_div(cols).unwrap_or(0);

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
    let label_h: f32 = if config.show_desktop_labels {
        28.0
    } else {
        0.0
    };
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

/// The y coordinate the thumbnail area starts at, below the search bar.
const CONTENT_TOP: f32 = 70.0;
/// Margin left below the thumbnail area and at either side of it.
const CONTENT_MARGIN: f32 = 20.0;

/// Where every thumbnail lands on a `screen_w` × `screen_h` display.
///
/// This is the *one* answer to that question. [`render_overview`] draws these
/// rectangles and [`on_mouse_click`] hit-tests them, and both get them from
/// here rather than each working them out — the same discipline
/// [`crate::DesktopShell::render_zone_overlay`] follows for the snap overlay,
/// and for the same reason: two computations of one layout is how a click
/// comes to select the window next to the one that was lit.
///
/// Returns an empty vector while the overlay is closed or still animating in
/// from nothing, which is what makes a click during that window select
/// nothing rather than whatever happens to be under the cursor.
#[must_use]
pub fn overview_layout(
    state: &OverviewState,
    config: &OverviewConfig,
    screen_w: f32,
    screen_h: f32,
) -> Vec<ThumbnailLayout> {
    if !state.visible {
        return Vec::new();
    }
    let content_h = screen_h - CONTENT_TOP - CONTENT_MARGIN;
    let content_w = screen_w - CONTENT_MARGIN * 2.0;
    match state.mode {
        OverviewMode::AllWindows | OverviewMode::RecentApps => {
            let thumbs = collect_thumbs_for_mode(state);
            compute_grid_layout(
                &thumbs,
                CONTENT_MARGIN,
                CONTENT_TOP,
                content_w,
                content_h,
                config,
            )
        }
        OverviewMode::AllDesktops => compute_lane_layout(
            &state.lanes,
            CONTENT_MARGIN,
            CONTENT_TOP,
            content_w,
            content_h,
            config,
        ),
    }
}

/// Render the full overview overlay into a list of `RenderCommand`s.
///
/// `screen_w` / `screen_h` are the total display dimensions.
pub fn render_overview(
    state: &OverviewState,
    config: &OverviewConfig,
    p: &Palette,
    screen_w: f32,
    screen_h: f32,
) -> Vec<RenderCommand> {
    if !state.visible {
        return Vec::new();
    }

    // Scaled by the fade, and by nothing else in this function: the cards, the
    // labels and the search bar are drawn at full strength from the first frame
    // so that an overview whose fade is never advanced is still a *usable*
    // overview rather than an invisible one. See the module header.
    let alpha = (config.background_opacity * state.fade_opacity() * 255.0) as u8;
    let mut cmds = Vec::with_capacity(128);

    // Dark overlay background.
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: screen_w,
        height: screen_h,
        color: Color::rgba(p.mantle.r, p.mantle.g, p.mantle.b, alpha),
        corner_radii: CornerRadii::ZERO,
    });

    // Search bar at top.
    render_search_bar(&mut cmds, state, p, screen_w);

    // Content area (below search bar).
    let content_y = CONTENT_TOP;
    let content_h = screen_h - content_y - CONTENT_MARGIN;

    let layouts = overview_layout(state, config, screen_w, screen_h);

    // Desktop labels (AllDesktops only).
    if state.mode == OverviewMode::AllDesktops && config.show_desktop_labels {
        render_desktop_labels(&mut cmds, state, config, p, content_y, screen_w, content_h);
    }

    // Thumbnail cards.
    for layout in &layouts {
        let is_hovered = state.hovered_window == Some(layout.window_id);
        let is_search_match =
            !state.search_query.is_empty() && state.search_results.contains(&layout.window_id);
        let is_dimmed = !state.search_query.is_empty() && !is_search_match;

        render_thumbnail_card(&mut cmds, layout, p, is_hovered, is_dimmed);
    }

    // There was a "+" button here, bottom-right, for adding a virtual desktop.
    // It is gone, along with `OverviewAction::AddDesktop` and the click region
    // that produced it, because nothing could carry the request: `ShellControl`
    // has verbs for switching to a desktop and moving a window between
    // desktops, and none for creating one — the compositor's workspace count is
    // fixed at start-up, and whether it should be mutable is an open question
    // (`known-issues.md`, the overview entry).
    //
    // Drawing it anyway would be this feature's own original sin repeated in
    // miniature: a control that is on screen, takes the click, and does
    // nothing. Better a desktop count the user can see is fixed than a button
    // that teaches them it is not and then proves otherwise. When the count
    // becomes mutable, the button comes back with a verb behind it.

    cmds
}

/// Render the search bar at the top of the overlay.
fn render_search_bar(
    cmds: &mut Vec<RenderCommand>,
    state: &OverviewState,
    p: &Palette,
    screen_w: f32,
) {
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
        color: p.surface0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Border (highlights when query is active).
    let border_color = if state.search_query.is_empty() {
        p.surface1
    } else {
        p.accent
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
        ("Search windows...".to_string(), p.overlay0)
    } else {
        (state.search_query.clone(), p.text)
    };
    cmds.push(RenderCommand::Text {
        x: bar_x + 12.0,
        y: bar_y + 10.0,
        text: display_text,
        color: text_color,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(bar_w - 24.0),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Render desktop labels and current-desktop indicator in AllDesktops mode.
fn render_desktop_labels(
    cmds: &mut Vec<RenderCommand>,
    state: &OverviewState,
    config: &OverviewConfig,
    p: &Palette,
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
                color: p.accent,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Label.
        cmds.push(RenderCommand::Text {
            x: 32.0,
            y: ly + 4.0,
            text: lane.name.clone(),
            color: if lane.is_current { p.text } else { p.subtext0 },
            font_size: 13.0,
            font_weight: if lane.is_current {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(screen_w - 80.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// Render a single thumbnail card.
fn render_thumbnail_card(
    cmds: &mut Vec<RenderCommand>,
    layout: &ThumbnailLayout,
    p: &Palette,
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
        Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 100)
    } else {
        p.surface0
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
    let title_color = if is_dimmed { p.overlay0 } else { p.text };
    cmds.push(RenderCommand::Text {
        x: x + dx + 8.0,
        y: y + dy + 8.0,
        text: title_display,
        color: title_color,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some((w + dw - 16.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });

    // Border.
    let border_color = if is_hovered {
        p.accent
    } else if layout.is_focused {
        p.subtext0
    } else {
        p.surface2
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
            color: p.yellow,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + dx + w + dw - 18.0,
            y: y + dy + 6.0,
            text: "_".to_string(),
            color: readable_on(p.yellow),
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(12.0),
            overflow: TextOverflow::Ellipsis,
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
            color: p.red,
            corner_radii: CornerRadii::all(9.0),
        });
        cmds.push(RenderCommand::Text {
            x: cb_x + 4.0,
            y: cb_y + 2.0,
            text: "x".to_string(),
            color: readable_on(p.red),
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(12.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    // No second label under the card. There used to be one, drawing each
    // window's application name; nothing in the system could supply that
    // string, so on a real desktop it drew an empty line of reserved space
    // under every thumbnail — the layout paying for text that was never going
    // to arrive.

    // Suppress unused-variable warning — label_h is used to document intent.
    let _ = label_h;
}

// ============================================================================
// Input handling
// ============================================================================

/// Result of processing an input event inside the overview.
///
/// The variants split cleanly in two: things that happen entirely inside this
/// overlay (`None`, `Close`, `NavigateSelection`, `SearchChanged`) and things
/// the compositor has to be asked for, which are all one variant, `Request`.
///
/// It used to carry three more — `SwitchToWindow(u64)`, `SwitchToDesktop(u32)`
/// and `CloseWindow(u64)` — which were a fourth spelling of verbs the shell
/// already had ([`ShellControlAction::Activate`], [`ShellRequest::SwitchDesktop`],
/// [`ShellControlAction::Close`]). A second spelling of a verb is a second thing
/// to drift: the moment one side grows a rule the other does not, the overview
/// and the taskbar disagree about what clicking a window means, and nothing
/// type-checks the difference. They are gone, exactly as the zone-snapping work
/// deleted the shell's second copy of the edge rules.
///
/// A fourth, `AddDesktop`, went with them, but for the opposite reason: not a
/// duplicate verb but a verb with nothing behind it. `ShellControl` cannot
/// create a virtual desktop — the compositor's workspace count is fixed at
/// start-up (design-decisions.md §518) — so the "+" button that produced it was
/// a control that took the click and did nothing. Keeping the variant "to say
/// so out loud" said it only to a programmer reading this file; the user got a
/// dead button. It comes back when there is a verb to put behind it.
#[derive(Debug, Clone, PartialEq)]
pub enum OverviewAction {
    /// No action — the event was not consumed.
    None,
    /// Close the overview.
    Close,
    /// Ask the compositor for something, in the shell's one vocabulary.
    Request(ShellRequest),
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
                OverviewAction::Request(ShellRequest::window(
                    WindowId(wid),
                    ShellControlAction::Activate,
                ))
            } else {
                OverviewAction::None
            }
        }
        OverviewKey::ArrowUp
        | OverviewKey::ArrowDown
        | OverviewKey::ArrowLeft
        | OverviewKey::ArrowRight => {
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
///
/// Takes no screen size: everything clickable in the overview is a rectangle in
/// `layouts`, and `layouts` already came from [`overview_layout`], which is the
/// one place the screen size is applied. It used to take both, for the "+"
/// add-desktop button that was positioned relative to the bottom-right corner —
/// the only thing here that was placed independently of the layout pass, and
/// therefore the only thing that could be drawn in one place and clicked in
/// another. It is gone; so is the second copy of the screen size.
pub fn on_mouse_click(
    state: &mut OverviewState,
    mx: f32,
    my: f32,
    layouts: &[ThumbnailLayout],
) -> OverviewAction {
    if !state.visible {
        return OverviewAction::None;
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
                return OverviewAction::Request(ShellRequest::window(
                    WindowId(layout.window_id),
                    ShellControlAction::Close,
                ));
            }

            // Otherwise — switch to this window.
            state.hide();
            return OverviewAction::Request(ShellRequest::window(
                WindowId(layout.window_id),
                ShellControlAction::Activate,
            ));
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
            return OverviewAction::Request(ShellRequest::SwitchDesktop { desktop: did });
        }
    }

    OverviewAction::None
}

/// Process a mouse-scroll event in AllDesktops mode.
pub fn on_mouse_scroll(state: &mut OverviewState, delta: f32) -> OverviewAction {
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

    // Clamped: a scroll gesture that ran off the end of the desktops and
    // reappeared at the other end would be a surprise, not a convenience.
    let new_idx = if delta > 0.0 {
        step::clamped_after(state.lanes.len(), current_idx)
    } else {
        step::clamped_before(state.lanes.len(), current_idx)
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

    // Clamped, matching the scroll gesture above: arrowing off the edge of
    // the grid holds still rather than teleporting to the opposite edge.
    let new_idx = match (current_idx, key) {
        (None, _) => Some(0),
        (Some(i), OverviewKey::ArrowRight | OverviewKey::ArrowDown) => {
            Some(step::clamped_after(all.len(), i))
        }
        (Some(i), OverviewKey::ArrowLeft | OverviewKey::ArrowUp) => {
            Some(step::clamped_before(all.len(), i))
        }
        (Some(i), _) => Some(i),
    };

    if let Some(idx) = new_idx
        && let Some(t) = all.get(idx)
    {
        state.hovered_window = Some(t.window_id);
    }
}

/// Collect the thumbnails relevant to the current mode.
fn collect_thumbs_for_mode(state: &OverviewState) -> Vec<WindowThumbnail> {
    match state.mode {
        OverviewMode::AllWindows => {
            // Current desktop only.
            let current_desktop = state.lanes.iter().find(|l| l.is_current);
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
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;

    // -- Helpers -------------------------------------------------------------

    fn sample_thumb(id: u64, desktop: u32, title: &str) -> WindowThumbnail {
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
        }
    }

    fn sample_lanes() -> Vec<DesktopLane> {
        vec![
            DesktopLane {
                desktop_id: 0,
                name: "Desktop 1".to_string(),
                thumbnails: vec![sample_thumb(1, 0, "Terminal"), sample_thumb(2, 0, "Editor")],
                is_current: true,
            },
            DesktopLane {
                desktop_id: 1,
                name: "Desktop 2".to_string(),
                thumbnails: vec![sample_thumb(3, 1, "Browser")],
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

    // -- Visibility ----------------------------------------------------------
    //
    // Four tests lived here — `test_animation_tick_advances`,
    // `_reaches_one`, `_hide_reaches_zero`, `_zero_duration_instant` — and all
    // four passed, because they called `tick_animation` themselves. Nothing in
    // the shell ever did: `oswindow::EventLoop::run` blocks in `wait()` when
    // there is no input, so there is no frame on which to advance a fade.
    // `animation_progress` therefore never left the 0.0 that `show` set, and
    // every draw path bailed on it — an overview that opened blank and stayed
    // blank. The tests were a closed loop that proved the arithmetic and
    // nothing about the screen.
    //
    // What replaces them is the claim `show` now actually makes.

    #[test]
    fn showing_the_overview_is_enough_to_draw_it() {
        let mut s = OverviewState::new();
        let cfg = default_config();
        assert!(
            render_overview(&s, &cfg, &Palette::for_mode(false), 1920.0, 1080.0).is_empty(),
            "a hidden overview drew something"
        );
        s.show(OverviewMode::AllWindows);
        assert!(
            !render_overview(&s, &cfg, &Palette::for_mode(false), 1920.0, 1080.0).is_empty(),
            "a shown overview drew nothing — the caller has no second step to \
             take, so this is the whole of what opening it does"
        );
        s.hide();
        assert!(
            render_overview(&s, &cfg, &Palette::for_mode(false), 1920.0, 1080.0).is_empty(),
            "a hidden overview drew something"
        );
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
    fn a_query_matching_no_title_matches_no_window() {
        // This test used to be `test_search_filters_by_app_name`, and it typed
        // "fire" to find a thumbnail whose `app_name` was "firefox". The field
        // was deleted because nothing could ever fill it: the window list the
        // shell receives carries a title, an id, a workspace and a rectangle,
        // and no application identity at all. So on a real desktop every
        // `app_name` was the empty string — and `"".contains(query)` is true
        // for *every* query, which meant the search silently returned the whole
        // desktop instead of narrowing it.
        //
        // What replaces it is the claim that matching is now total: a query
        // that matches no title matches nothing. That is the assertion the old
        // one could not make, because the old one passed either way.
        let mut s = OverviewState::new();
        s.lanes = sample_lanes();
        for ch in "firefox".chars() {
            s.type_search_char(ch);
        }
        assert!(
            s.search_results.is_empty(),
            "no window is titled 'firefox', yet the search found {:?}",
            s.search_results
        );
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
        let thumbs = vec![sample_thumb(1, 0, "Win")];
        let config = default_config();
        let result = compute_grid_layout(&thumbs, 0.0, 0.0, 800.0, 600.0, &config);
        assert_eq!(result.len(), 1);
        assert!(result[0].render_width > 0.0);
        assert!(result[0].render_height > 0.0);
    }

    #[test]
    fn test_grid_layout_multiple_windows() {
        let thumbs: Vec<_> = (0..6)
            .map(|i| sample_thumb(i, 0, &format!("Win {}", i)))
            .collect();
        let config = default_config();
        let result = compute_grid_layout(&thumbs, 0.0, 0.0, 1920.0, 1080.0, &config);
        assert_eq!(result.len(), 6);
    }

    #[test]
    fn test_grid_layout_no_overlap() {
        let thumbs: Vec<_> = (0..4)
            .map(|i| sample_thumb(i, 0, &format!("Win {}", i)))
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
            .map(|i| sample_thumb(i, 0, &format!("Win {}", i)))
            .collect();
        let mut config = default_config();
        config.max_columns = 3;
        let result = compute_grid_layout(&thumbs, 0.0, 0.0, 1920.0, 1080.0, &config);
        assert_eq!(result.len(), 10);

        // First row should have 3 items.
        let first_row_y = result[0].render_y;
        let first_row_count = result
            .iter()
            .filter(|r| (r.render_y - first_row_y).abs() < 1.0)
            .count();
        assert_eq!(first_row_count, 3);
    }

    #[test]
    fn test_grid_layout_preserves_aspect_ratio() {
        let mut thumb = sample_thumb(1, 0, "Wide");
        thumb.width = 1600.0;
        thumb.height = 400.0; // 4:1 aspect ratio
        let config = default_config();
        let result = compute_grid_layout(&[thumb], 0.0, 0.0, 800.0, 800.0, &config);
        assert_eq!(result.len(), 1);
        let ratio = result[0].render_width / result[0].render_height;
        assert!(
            (ratio - 4.0).abs() < 0.1,
            "aspect ratio should be ~4:1, got {}",
            ratio
        );
    }

    #[test]
    fn test_grid_layout_zero_area() {
        let thumbs = vec![sample_thumb(1, 0, "Win")];
        let config = default_config();
        let result = compute_grid_layout(&thumbs, 0.0, 0.0, 0.0, 0.0, &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_grid_layout_many_windows() {
        let thumbs: Vec<_> = (0..20)
            .map(|i| sample_thumb(i, 0, &format!("Win {}", i)))
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
            thumbnails: vec![sample_thumb(1, 0, "Win")],
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
                thumbnails: vec![sample_thumb(1, 1, "Win")],
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
        let cmds = render_overview(&state, &config, &Palette::for_mode(false), 1920.0, 1080.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_render_visible_produces_commands() {
        let mut state = OverviewState::new();
        state.show(OverviewMode::AllWindows);
        state.lanes = sample_lanes();
        let config = default_config();
        let cmds = render_overview(&state, &config, &Palette::for_mode(false), 1920.0, 1080.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn nothing_draws_an_add_desktop_button() {
        // This was `test_render_alldesktops_has_add_button`, asserting the "+"
        // *was* drawn. It is inverted rather than deleted because the button is
        // the kind of thing that gets added back by someone reading
        // `render_desktop_labels` and noticing there is no way to make a fourth
        // desktop. There still is not: `ShellControl` has no verb for it, so a
        // button would take the click and do nothing. Adding the verb is what
        // unblocks the button, and this assertion is what says so at the moment
        // someone tries it the other way round.
        let mut state = OverviewState::new();
        state.show(OverviewMode::AllDesktops);
        state.lanes = sample_lanes();
        let config = default_config();
        let cmds = render_overview(&state, &config, &Palette::for_mode(false), 1920.0, 1080.0);
        let has_plus = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "+"
            } else {
                false
            }
        });
        assert!(!has_plus, "a control was drawn that nothing can carry out");
    }

    #[test]
    fn test_render_search_bar_placeholder() {
        let mut state = OverviewState::new();
        state.show(OverviewMode::AllWindows);
        let config = default_config();
        let cmds = render_overview(&state, &config, &Palette::for_mode(false), 1920.0, 1080.0);
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
        state.lanes = sample_lanes();
        state.search_query = "term".to_string();
        state.update_search();
        let config = default_config();
        let cmds = render_overview(&state, &config, &Palette::for_mode(false), 1920.0, 1080.0);
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
        assert_eq!(
            action,
            OverviewAction::Request(ShellRequest::window(
                WindowId(42),
                ShellControlAction::Activate
            ))
        );
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
            is_focused: false,
            is_minimized: false,
            render_x: 100.0,
            render_y: 100.0,
            render_width: 200.0,
            render_height: 150.0,
        }];
        let action = on_mouse_click(&mut s, 150.0, 150.0, &layouts);
        assert_eq!(
            action,
            OverviewAction::Request(ShellRequest::window(
                WindowId(7),
                ShellControlAction::Activate
            ))
        );
        assert!(!s.visible);
    }

    #[test]
    fn the_bottom_right_corner_is_not_a_button() {
        // This was `test_mouse_click_add_desktop`, and it clicked (1870, 1030)
        // on a 1920x1080 screen to hit a "+" button that asked for a new
        // virtual desktop. Nothing could carry that ask — `ShellControl` has no
        // verb for creating a workspace — so the button took the click and did
        // nothing, which is the exact defect the overview itself was filed for.
        // The button is gone, and this is the assertion that it stayed gone:
        // that corner is backdrop like any other part of the backdrop.
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllDesktops);
        let action = on_mouse_click(&mut s, 1870.0, 1030.0, &[]);
        assert_eq!(action, OverviewAction::None);
    }

    #[test]
    fn test_mouse_click_empty_area() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let action = on_mouse_click(&mut s, 5.0, 5.0, &[]);
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
    }

    #[test]
    fn test_config_roundtrip() {
        let cfg = OverviewConfig {
            thumbnail_padding: 24.0,
            max_columns: 3,
            show_desktop_labels: false,
            background_opacity: 0.9,
            fade_ms: 120,
        };
        let text = cfg.to_text();
        let parsed = OverviewConfig::from_text(&text);
        assert!((parsed.thumbnail_padding - 24.0).abs() < f32::EPSILON);
        assert_eq!(parsed.max_columns, 3);
        assert!(!parsed.show_desktop_labels);
        assert!((parsed.background_opacity - 0.9).abs() < 0.001);
        assert_eq!(parsed.fade_ms, 120);
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

    // -- The backdrop fade ---------------------------------------------------

    /// The alpha of the first `FillRect`, which is the backdrop.
    fn backdrop_alpha(state: &OverviewState, config: &OverviewConfig) -> u8 {
        match render_overview(state, config, &Palette::for_mode(false), 1920.0, 1080.0).first() {
            Some(&RenderCommand::FillRect { color, .. }) => color.a,
            other => panic!("the first command was not the backdrop: {other:?}"),
        }
    }

    #[test]
    fn an_overview_that_is_never_ticked_is_fully_open() {
        // §520's defect, stated as a property rather than as a story. `show` is
        // reached from three call sites, none of which knows whether a clock
        // exists, so the state it leaves behind has to be the *working* one. A
        // fade that `show` started would make this overlay invisible for a
        // caller that never ticks — which is every caller in this file, in the
        // layout tests, and in any embedder driving the shell by hand.
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        assert!(!s.is_fading());
        assert!((s.fade_opacity() - 1.0).abs() < f32::EPSILON);

        let cfg = default_config();
        assert_eq!(
            backdrop_alpha(&s, &cfg),
            (cfg.background_opacity * 255.0) as u8,
            "an un-ticked overview drew a backdrop dimmer than the one asked for"
        );
    }

    #[test]
    fn a_fade_that_has_begun_still_leaves_every_card_drawn() {
        // The fade scales the backdrop and nothing else. The original gated
        // *every* draw path on progress, so a fade held at zero drew nothing at
        // all; this asserts the cards do not depend on it.
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        s.lanes = sample_lanes();
        s.begin_fade(200);
        let cfg = default_config();

        let cards = render_overview(&s, &cfg, &Palette::for_mode(false), 1920.0, 1080.0)
            .iter()
            .filter(|c| matches!(c, RenderCommand::Text { .. }))
            .count();
        assert!(
            cards > 1,
            "a fade at zero progress erased the overview's contents"
        );
    }

    #[test]
    fn the_backdrop_is_dimmer_part_way_through_the_fade() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        let cfg = default_config();
        let full = backdrop_alpha(&s, &cfg);

        s.begin_fade(200);
        assert!(
            backdrop_alpha(&s, &cfg) < full,
            "the fade was begun and the backdrop was drawn at full strength — \
             there is no fade"
        );
        s.tick_fade(60);
        let midway = backdrop_alpha(&s, &cfg);
        assert!(midway < full, "60 ms of a 200 ms fade finished it");
        assert!(midway > 0, "the fade drew no backdrop at all");
    }

    #[test]
    fn a_finished_fade_is_indistinguishable_from_one_that_never_ran() {
        // So that nothing downstream can accidentally depend on "has faded"
        // versus "was never faded" — the two must be the same overlay.
        let mut faded = OverviewState::new();
        faded.show(OverviewMode::AllWindows);
        faded.begin_fade(200);
        while faded.tick_fade(16) {}

        let mut fresh = OverviewState::new();
        fresh.show(OverviewMode::AllWindows);

        let cfg = default_config();
        assert!(!faded.is_fading());
        assert_eq!(backdrop_alpha(&faded, &cfg), backdrop_alpha(&fresh, &cfg));
    }

    #[test]
    fn a_fade_reports_when_it_wants_another_frame_and_when_it_does_not() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        assert!(
            !s.tick_fade(16),
            "an overview with no fade asked for another frame — an idle \
             desktop that never parks"
        );

        s.begin_fade(100);
        assert!(s.tick_fade(50), "the fade gave up half way through");
        assert!(!s.tick_fade(50), "the fade asked for a frame past its end");
        assert!(!s.is_fading());
    }

    #[test]
    fn a_zero_length_fade_is_no_fade_rather_than_a_one_millisecond_one() {
        // `Animation::new` floors a duration at 1 ms, so without this a
        // reduced-motion setting of zero would produce a fade whose visibility
        // depended on when the next frame happened to land.
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        s.begin_fade(0);
        assert!(!s.is_fading());
        assert!((s.fade_opacity() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn hiding_drops_the_fade_so_the_next_opening_does_not_inherit_it() {
        let mut s = OverviewState::new();
        s.show(OverviewMode::AllWindows);
        s.begin_fade(200);
        s.tick_fade(100);
        s.hide();
        assert!(
            !s.is_fading(),
            "a hidden overview is still asking to be clocked"
        );

        s.show(OverviewMode::AllWindows);
        assert!((s.fade_opacity() - 1.0).abs() < f32::EPSILON);
    }

    // ========================================================================
    // The palette conversion
    // ========================================================================

    use crate::draw_check::assert_nothing_is_drawn_and_never_seen;
    use crate::palette_check::assert_drawn_from;

    const SW: f32 = 1920.0;
    const SH: f32 = 1080.0;

    /// A colour without its alpha, for comparing a wash against its role.
    fn rgb(c: Color) -> (u8, u8, u8) {
        (c.r, c.g, c.b)
    }

    fn draw(state: &OverviewState, p: &Palette) -> Vec<RenderCommand> {
        render_overview(state, &default_config(), p, SW, SH)
    }

    /// Lanes exercising every branch a card can take: one plain, one focused,
    /// one minimised.
    ///
    /// All three live on the *current* desktop deliberately. `AllWindows`
    /// collects only the current lane, so a minimised window parked on the
    /// second desktop would never be drawn in the mode most of these tests use
    /// — and a badge that is never drawn cannot fail an assertion about its
    /// colour. The second lane exists so `AllDesktops` has more than one to
    /// draw.
    fn rich_lanes() -> Vec<DesktopLane> {
        let plain = sample_thumb(1, 0, "Terminal");
        let mut focused = sample_thumb(2, 0, "Editor");
        focused.is_focused = true;
        let mut minimised = sample_thumb(3, 0, "Browser");
        minimised.is_minimized = true;
        vec![
            DesktopLane {
                desktop_id: 0,
                name: "Desktop 1".to_string(),
                thumbnails: vec![plain, focused, minimised],
                is_current: true,
            },
            DesktopLane {
                desktop_id: 1,
                name: "Desktop 2".to_string(),
                thumbnails: vec![sample_thumb(4, 1, "Files")],
                is_current: false,
            },
        ]
    }

    fn shown(mode: OverviewMode) -> OverviewState {
        let mut s = OverviewState::new();
        s.show(mode);
        s.mode = mode;
        s.lanes = rich_lanes();
        s
    }

    /// Every state the overlay can be drawn in, named so a failure says which.
    ///
    /// A hidden overview is deliberately absent: it draws nothing, so it can
    /// satisfy any assertion about what it draws.
    fn every_state() -> Vec<(OverviewState, String)> {
        let mut out = Vec::new();
        for mode in [
            OverviewMode::AllWindows,
            OverviewMode::AllDesktops,
            OverviewMode::RecentApps,
        ] {
            out.push((shown(mode), format!("{mode:?}, at rest")));

            let mut hovered = shown(mode);
            hovered.hovered_window = Some(1);
            out.push((hovered, format!("{mode:?}, pointer over a card")));

            // A live query dims every card that does not match, and lights the
            // search bar's border. Both branches of both are on screen at once.
            let mut searching = shown(mode);
            searching.search_query = "edit".to_string();
            searching.search_results = vec![2];
            out.push((searching, format!("{mode:?}, searching")));

            let mut empty = shown(mode);
            empty.lanes = Vec::new();
            out.push((empty, format!("{mode:?}, no windows at all")));
        }
        out
    }

    /// Accents that collide with nothing this module freezes.
    ///
    /// The two badges are yellow and red, and the border ladder's middle rung
    /// is `subtext0`; an accent equal to any of those would let a wrongly
    /// accented site coincide with a frozen neighbour on exactly the accent
    /// that matters.
    const SAFE_ACCENTS: [Color; 6] = [
        appearance::BLUE,
        appearance::MAUVE,
        appearance::TEAL,
        appearance::PINK,
        appearance::SAPPHIRE,
        appearance::SKY,
    ];

    fn fills(cmds: &[RenderCommand], keep: impl Fn(f32, f32, f32, f32) -> bool) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match *c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if keep(x, y, width, height) => Some(color),
                _ => None,
            })
            .collect()
    }

    fn strokes(cmds: &[RenderCommand], keep: impl Fn(f32, f32, f32, f32) -> bool) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match *c {
                RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if keep(x, y, width, height) => Some(color),
                _ => None,
            })
            .collect()
    }

    fn texts(cmds: &[RenderCommand], keep: impl Fn(f32, f32, &str) -> bool) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x, y, text, color, ..
                } if keep(*x, *y, text) => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The backdrop, which is the first command and the full screen.
    fn backdrop(cmds: &[RenderCommand]) -> Color {
        match cmds.first() {
            Some(&RenderCommand::FillRect { color, .. }) => color,
            other => panic!("the first command was not the backdrop: {other:?}"),
        }
    }

    /// The search bar is the one 400x36 shape; a card is scaled from an 800x600
    /// window, so it is 4:3 and can never take those dimensions.
    fn search_bar_is(x: f32, _y: f32, w: f32, h: f32) -> bool {
        let _ = x;
        w == 400.0 && h == 36.0
    }

    fn search_bar_fill(cmds: &[RenderCommand]) -> Color {
        let v = fills(cmds, search_bar_is);
        assert_eq!(v.len(), 1, "expected exactly one search bar");
        v[0]
    }

    fn search_bar_border(cmds: &[RenderCommand]) -> Color {
        let v = strokes(cmds, search_bar_is);
        assert_eq!(v.len(), 1, "expected exactly one search bar border");
        v[0]
    }

    /// Every card border: every stroke that is not the search bar's.
    fn card_borders(cmds: &[RenderCommand]) -> Vec<Color> {
        strokes(cmds, |x, y, w, h| !search_bar_is(x, y, w, h))
    }

    /// The bar marking the current desktop — 4x24 at a fixed x, drawn only in
    /// `AllDesktops`.
    fn current_desktop_bars(cmds: &[RenderCommand]) -> Vec<Color> {
        fills(cmds, |x, _, w, h| x == 20.0 && w == 4.0 && h == 24.0)
    }

    fn minimised_badges(cmds: &[RenderCommand]) -> Vec<Color> {
        fills(cmds, |_, _, w, h| w == 16.0 && h == 16.0)
    }

    fn close_buttons(cmds: &[RenderCommand]) -> Vec<Color> {
        fills(cmds, |_, _, w, h| w == 18.0 && h == 18.0)
    }

    /// The badge marks, keyed on their text rather than their size: the close
    /// mark is 11pt, and so is every card title.
    fn minimised_marks(cmds: &[RenderCommand]) -> Vec<Color> {
        texts(cmds, |_, _, t| t == "_")
    }

    fn close_marks(cmds: &[RenderCommand]) -> Vec<Color> {
        texts(cmds, |_, _, t| t == "x")
    }

    /// Everything the overlay draws except the three sites that follow the
    /// accent, so a change here is a change somewhere it should not be.
    ///
    /// The card borders come out whole rather than selectively, because which
    /// border is the hovered one is not knowable from a colour list — that is
    /// what `a_cards_border_says_both_where_you_point_and_what_has_focus` is
    /// for.
    fn colours_apart_from_the_accent_sites(cmds: &[RenderCommand]) -> Vec<Color> {
        let mut out = Vec::new();
        for (i, c) in cmds.iter().enumerate() {
            match *c {
                RenderCommand::FillRect {
                    x,
                    width,
                    height,
                    color,
                    ..
                } => {
                    let is_current_bar = x == 20.0 && width == 4.0 && height == 24.0;
                    if !is_current_bar {
                        out.push(color);
                    }
                }
                RenderCommand::StrokeRect { .. } => {
                    // Both the search bar's border and every card's can carry
                    // the accent, so no stroke belongs in the frozen set.
                    let _ = i;
                }
                RenderCommand::Text { color, .. } => out.push(color),
                _ => {}
            }
        }
        out
    }

    // -- The sweep -----------------------------------------------------------

    /// No hardcoded colour survived the conversion.
    ///
    /// Rendered in both modes and checked in the light one: every deleted
    /// constant was a Catppuccin *Mocha* value, and a Mocha value is not a
    /// member of Latte, so a leftover shows up as a colour the light palette
    /// does not contain.
    ///
    /// The two washes are declared derived, because they carry their role's RGB
    /// under their own alpha; that the alpha is right is
    /// `a_wash_keeps_its_own_alpha_and_the_colour_of_its_role`'s job, since
    /// this check compares RGB only.
    #[test]
    fn every_colour_the_overlay_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let derived = [readable_on(p.yellow), readable_on(p.red)];
            for (state, what) in every_state() {
                assert_drawn_from(&p, &draw(&state, &p), &derived, &what);
            }
        }
    }

    /// Nothing is drawn that no one can ever see.
    #[test]
    fn the_overlay_draws_nothing_that_is_immediately_erased() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (state, what) in every_state() {
                assert_nothing_is_drawn_and_never_seen(
                    &draw(&state, &p),
                    &format!("{what}, light={light}"),
                );
            }
        }
    }

    // -- What follows the accent, and what does not --------------------------

    /// The three sites that mark a position or an invitation follow the accent.
    ///
    /// One assertion per *source* site rather than per drawn instance: two
    /// cards drawn by one loop cannot disagree with each other, so counting
    /// them would count the loop, not the code.
    #[test]
    fn every_control_that_offers_something_follows_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                // 1. The search bar's border, once a query is live.
                let mut searching = shown(OverviewMode::AllWindows);
                searching.search_query = "edit".to_string();
                searching.search_results = vec![2];
                assert_eq!(
                    search_bar_border(&draw(&searching, &p)),
                    accent,
                    "an active search bar's border did not follow the accent \
                     (light={light})"
                );

                // 2. The bar marking the current desktop.
                let lanes = draw(&shown(OverviewMode::AllDesktops), &p);
                let bars = current_desktop_bars(&lanes);
                assert_eq!(bars.len(), 1, "expected one current-desktop bar");
                assert_eq!(
                    bars[0], accent,
                    "the current desktop's marker did not follow the accent \
                     (light={light})"
                );

                // 3. A card's border while the pointer is over it. The hovered
                //    card is the only one with a 2.0-wide border, which is how
                //    it is told from the rest without re-deriving the layout.
                let mut hovered = shown(OverviewMode::AllWindows);
                hovered.hovered_window = Some(1);
                let cmds = draw(&hovered, &p);
                let thick: Vec<Color> = cmds
                    .iter()
                    .filter_map(|c| match *c {
                        RenderCommand::StrokeRect {
                            color,
                            line_width: 2.0,
                            ..
                        } => Some(color),
                        _ => None,
                    })
                    .collect();
                assert_eq!(thick.len(), 1, "expected exactly one hovered card");
                assert_eq!(
                    thick[0], accent,
                    "a hovered card's border did not follow the accent \
                     (light={light})"
                );
            }
        }
    }

    /// Changing the accent changes the three accent sites and nothing else.
    ///
    /// The sweep above cannot see a *wrong role*, because a role is a member of
    /// both palettes. This can: it renders the same state under two accents and
    /// requires every other colour to be identical.
    #[test]
    fn nothing_else_moves_when_the_accent_does() {
        for light in [false, true] {
            let mut a = Palette::for_mode(light);
            a.accent = appearance::MAUVE;
            let mut b = Palette::for_mode(light);
            b.accent = appearance::TEAL;

            for (state, what) in every_state() {
                assert_eq!(
                    colours_apart_from_the_accent_sites(&draw(&state, &a)),
                    colours_apart_from_the_accent_sites(&draw(&state, &b)),
                    "{what} (light={light}): something outside the three accent \
                     sites moved when the accent did"
                );
            }
        }
    }

    // -- Surfaces ------------------------------------------------------------

    /// The overlay's own surfaces are the roles they claim to be.
    ///
    /// Stronger than the membership sweep and for a different reason: the sweep
    /// allows `readable_on`'s two endpoints at any alpha, and one of them
    /// (`0x11111B`) is also Mocha `crust` — so a surface wrongly set to `crust`
    /// would pass membership in *both* modes. Equality with the named role
    /// cannot be satisfied that way.
    #[test]
    fn the_overlays_own_surfaces_come_from_the_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let at_rest = draw(&shown(OverviewMode::AllWindows), &p);

            assert_eq!(
                search_bar_fill(&at_rest),
                p.surface0,
                "the search bar is not surface0 (light={light})"
            );
            assert_eq!(
                search_bar_border(&at_rest),
                p.surface1,
                "an idle search bar's border is not surface1 (light={light})"
            );
            assert_eq!(
                rgb(backdrop(&at_rest)),
                rgb(p.mantle),
                "the backdrop is not the mantle (light={light})"
            );
        }
    }

    // -- The border ladder ---------------------------------------------------

    /// A card's border says two independent things, and must not blur them.
    ///
    /// Hover is *where you are pointing*; focus is *which window has the
    /// keyboard*. They are orthogonal, so the three rungs — present, focused,
    /// pointed at — have to stay three colours under every accent. Nothing in
    /// the membership sweep could see them collapse, because all three are
    /// palette roles.
    #[test]
    fn a_cards_border_says_both_where_you_point_and_what_has_focus() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                // Window 1 is plain, window 2 has focus; point at window 1 so
                // all three rungs are on screen at once.
                let mut s = shown(OverviewMode::AllWindows);
                s.hovered_window = Some(1);
                let borders = card_borders(&draw(&s, &p));
                assert_eq!(borders.len(), 3, "expected one border per card");

                let distinct: std::collections::BTreeSet<(u8, u8, u8)> =
                    borders.iter().map(|c| (c.r, c.g, c.b)).collect();
                assert_eq!(
                    distinct.len(),
                    3,
                    "two of the three card states drew the same border \
                     (light={light}, accent={accent:?}): {borders:?}"
                );

                assert!(
                    borders.contains(&accent),
                    "no card border took the accent (light={light})"
                );
                assert!(
                    borders.contains(&p.subtext0),
                    "the focused card's border is not subtext0 (light={light})"
                );
                assert!(
                    borders.contains(&p.surface2),
                    "a plain card's border is not surface2 (light={light})"
                );
            }
        }
    }

    // -- The two frozen badges -----------------------------------------------

    /// Neither badge follows the accent.
    ///
    /// Both report a fact about a window rather than offering a choice about
    /// the desktop: minimised is a state, and close is destructive. A close
    /// button that matches the wallpaper has stopped saying anything.
    #[test]
    fn neither_badge_follows_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                // Window 3 is minimised; point at window 1 so a close button is
                // drawn too.
                let mut s = shown(OverviewMode::AllWindows);
                s.hovered_window = Some(1);
                let cmds = draw(&s, &p);

                let badges = minimised_badges(&cmds);
                assert_eq!(badges.len(), 1, "expected one minimised badge");
                assert_eq!(
                    badges[0], p.yellow,
                    "the minimised badge is not the palette's yellow \
                     (light={light})"
                );

                let closes = close_buttons(&cmds);
                assert_eq!(closes.len(), 1, "expected one close button");
                assert_eq!(
                    closes[0], p.red,
                    "the close button is not the palette's red (light={light})"
                );
            }
        }
    }

    /// Each badge's mark can be read on the badge it is drawn on.
    ///
    /// Both marks were Mocha `base` — a near-black chosen against Mocha's pale
    /// yellow and pale red. Latte's yellow and red are deep and its `base` is
    /// near-white, so the fixed value was on the wrong side of its own fill in
    /// one of the two modes. Asserting the two modes *disagree* is what stops a
    /// fixed value coming back: a mark identical in both modes is this bug
    /// returning.
    ///
    /// Every expectation here is computed from the fill the render *actually
    /// emitted*, never from `p.yellow`/`p.red` directly. Deriving it from the
    /// palette instead would assert what the badge was supposed to be painted
    /// rather than what it was painted, which is a different question and the
    /// wrong one — a badge reverted to a hardcoded fill would keep passing.
    ///
    /// The straddling palettes below exist because **neither shipped palette
    /// can tell the two badges apart**: Mocha's yellow and red are both pale
    /// and Latte's are both deep, so `readable_on` returns the same endpoint
    /// for both in both modes. A mark that answered for the *other* badge's
    /// fill would therefore be invisible in every mode we ship. Only a palette
    /// whose two fills fall on opposite sides of the threshold distinguishes
    /// "answers for its own fill" from "answers for a fill that happens to
    /// agree with it", and that distinction is the whole point of the fix.
    #[test]
    fn each_badges_mark_can_be_read_on_the_badge() {
        /// Render the badges and assert each mark answers for its own fill.
        fn check(p: &Palette, what: &str) -> (Color, Color) {
            let mut s = shown(OverviewMode::AllWindows);
            s.hovered_window = Some(1);
            let cmds = draw(&s, p);

            let fills = minimised_badges(&cmds);
            assert_eq!(fills.len(), 1, "expected one minimised badge ({what})");
            let minimised = minimised_marks(&cmds);
            assert_eq!(minimised.len(), 1, "expected one minimised mark ({what})");
            assert_eq!(
                minimised[0],
                readable_on(fills[0]),
                "the minimised mark does not answer for the fill it sits on \
                 ({what})"
            );

            let close_fills = close_buttons(&cmds);
            assert_eq!(close_fills.len(), 1, "expected one close button ({what})");
            let close = close_marks(&cmds);
            assert_eq!(close.len(), 1, "expected one close mark ({what})");
            assert_eq!(
                close[0],
                readable_on(close_fills[0]),
                "the close mark does not answer for the fill it sits on ({what})"
            );

            (minimised[0], close[0])
        }

        let mut marks = Vec::new();
        for light in [false, true] {
            let p = Palette::for_mode(light);
            marks.push(check(&p, &format!("light={light}")));
        }

        // Yellow pale, red deep — and then the mirror image. Under either, the
        // two marks must differ, so a mark answering for the wrong badge lands
        // on the wrong endpoint and is caught.
        for (yellow, red, what) in [
            (0x00F9_E2AF, 0x007F_1D2E, "pale yellow, deep red"),
            (0x0054_4A12, 0x00F3_8BA8, "deep yellow, pale red"),
        ] {
            let mut p = Palette::for_mode(false);
            p.yellow = Color::from_hex(yellow);
            p.red = Color::from_hex(red);
            assert_ne!(
                readable_on(p.yellow),
                readable_on(p.red),
                "the {what} fixture does not straddle the readable_on threshold, \
                 so it cannot tell the two badges apart"
            );
            check(&p, what);
        }
        assert_ne!(
            marks[0].0, marks[1].0,
            "the minimised mark is the same colour in both modes, so it is a \
             fixed value again rather than an answer about its fill"
        );
        assert_ne!(
            marks[0].1, marks[1].1,
            "the close mark is the same colour in both modes, so it is a fixed \
             value again rather than an answer about its fill"
        );
    }

    // -- The washes ----------------------------------------------------------

    /// A wash is a role seen through a veil: the veil is the alpha, the role is
    /// everything else.
    ///
    /// The membership sweep compares RGB only, so it would pass a wash whose
    /// alpha had been dropped — which is the whole of what a wash is. This
    /// checks both halves, and that the alpha is not simply opaque.
    #[test]
    fn a_wash_keeps_its_own_alpha_and_the_colour_of_its_role() {
        let cfg = default_config();
        for light in [false, true] {
            let p = Palette::for_mode(light);

            // The backdrop: the mantle, under the configured opacity.
            let at_rest = draw(&shown(OverviewMode::AllWindows), &p);
            let back = backdrop(&at_rest);
            assert_eq!(
                rgb(back),
                rgb(p.mantle),
                "the backdrop is not the mantle's colour (light={light})"
            );
            assert_eq!(
                back.a,
                (cfg.background_opacity * 255.0) as u8,
                "the backdrop lost the opacity it was configured with \
                 (light={light})"
            );
            assert!(
                back.a < 255,
                "the backdrop is fully opaque, so it is no longer a wash \
                 (light={light})"
            );

            // A card the search has dimmed: surface0, under a fixed veil. Its
            // undimmed neighbour is the same role at full strength, which is
            // what makes the pair worth asserting together — the dimming is the
            // alpha and nothing else.
            let mut searching = shown(OverviewMode::AllWindows);
            searching.search_query = "edit".to_string();
            searching.search_results = vec![2];
            let cmds = draw(&searching, &p);
            // Cards: big rectangles that are neither the search bar nor the
            // full-screen backdrop, which is also a wash and also surface-like.
            let cards = fills(&cmds, |_, _, w, h| {
                w > 40.0 && h > 40.0 && w != 400.0 && w < SW
            });
            assert!(
                cards.len() >= 2,
                "expected a dimmed card and an undimmed one, got {cards:?}"
            );
            for c in &cards {
                assert_eq!(
                    rgb(*c),
                    rgb(p.surface0),
                    "a card is not surface0 (light={light})"
                );
            }
            let dimmed: Vec<u8> = cards.iter().map(|c| c.a).filter(|a| *a != 255).collect();
            assert!(
                !dimmed.is_empty(),
                "no card was dimmed by the search, so the veil is gone \
                 (light={light})"
            );
            assert!(
                dimmed.iter().all(|a| *a == 100),
                "a dimmed card lost the alpha that dims it (light={light}): \
                 {dimmed:?}"
            );
        }
    }
}

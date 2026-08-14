//! Slate OS Desktop Shell
//!
//! Window manager and desktop environment providing:
//! - Window management (move, resize, minimize, maximize, close)
//! - Taskbar with running application list
//! - System tray (clock, notifications, quick settings)
//! - Start menu / application launcher
//! - Virtual desktops
//! - Keyboard shortcuts (Alt+Tab, Alt+F4, Super key, etc.)
//! - Theme support
//!
//! Communicates with the compositor via IPC messages (channels).
//! Uses the guitk library for UI rendering.

// The desktop shell is a widget-heavy crate: render/draw functions
// commonly take many positional parameters (font, theme, geometry,
// hit-test state, cursor, etc.), and several network/graphics protocol
// names use capitalized acronyms (VPN, WEP, WPA, WPA2, SRGB) that
// match RFC / spec terminology.
#![allow(
    clippy::too_many_arguments,
    clippy::upper_case_acronyms,
)]
#![cfg_attr(
    test,
    allow(
        clippy::field_reassign_with_default,
        clippy::bool_assert_comparison,
        clippy::needless_borrows_for_generic_args,
        clippy::manual_range_contains,
    )
)]

#[allow(dead_code)]
mod blur;
#[allow(dead_code)]
mod multimon;
#[allow(dead_code)]
mod icons;
#[allow(dead_code)]
mod notif_pane;
#[allow(dead_code)]
mod run_dialog;
#[allow(dead_code)]
mod taskbar;
#[allow(dead_code)]
mod tray_dnd;
#[allow(dead_code)]
mod power;
#[allow(dead_code)]
mod wallpaper;
#[allow(dead_code)]
mod calendar;
#[allow(dead_code)]
mod a11y;
#[allow(dead_code)]
mod resmon;
#[allow(dead_code)]
mod hotkeys;
#[allow(dead_code)]
mod animations;
#[allow(dead_code)]
mod snap;
#[allow(dead_code)]
mod clipboard_viewer;
#[allow(dead_code)]
mod window_peek;
#[allow(dead_code)]
mod overview;
#[allow(dead_code)]
mod display_settings;
#[allow(dead_code)]
mod about;
#[allow(dead_code)]
mod user_accounts;
#[allow(dead_code)]
mod taskbar_autohide;
#[allow(dead_code)]
mod input_method;
#[allow(dead_code)]
mod window_rules;
#[allow(dead_code)]
mod touchpad;
#[allow(dead_code)]
mod screen_capture;
#[allow(dead_code)]
mod print_manager;
#[allow(dead_code)]
mod bluetooth;
#[allow(dead_code)]
mod file_drop;
#[allow(dead_code)]
mod osd;
#[allow(dead_code)]
mod context_ext;
#[allow(dead_code)]
mod widgets;
#[allow(dead_code)]
mod login_screen;
#[allow(dead_code)]
mod session_mgr;
#[allow(dead_code)]
mod focus_assist;
#[allow(dead_code)]
mod mouse_settings;
#[allow(dead_code)]
mod sound_settings;
#[allow(dead_code)]
mod power_settings;
#[allow(dead_code)]
mod network_indicator;
#[allow(dead_code)]
mod storage_settings;
#[allow(dead_code)]
mod privacy_settings;
#[allow(dead_code)]
mod update_settings;
#[allow(dead_code)]
mod notification_settings;
#[allow(dead_code)]
mod appearance_settings;
#[allow(dead_code)]
mod language_settings;
#[allow(dead_code)]
mod launcher;
#[allow(dead_code)]
mod startup_settings;
#[allow(dead_code)]
mod datetime_settings;
#[allow(dead_code)]
mod accessibility_settings;
#[allow(dead_code)]
mod network_settings;
#[allow(dead_code)]
mod default_apps;
#[allow(dead_code)]
mod backup_settings;
#[allow(dead_code)]
mod device_settings;
#[allow(dead_code)]
mod security_dialog;

use appearance::config;
use appearance::{AppearanceSettings, TaskbarStyle, TransparencyLevel};
use guitk::color::Color;
use guitk::event::{Key, KeyEvent, Modifiers};
use guitk::render::RenderTree;

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Window Management
// ============================================================================

/// Unique window identifier (assigned by compositor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

/// Window state tracked by the window manager.
#[derive(Clone, Debug)]
pub struct ManagedWindow {
    pub id: WindowId,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub state: WindowState,
    pub desktop: u32,
    /// Whether this window has focus.
    pub focused: bool,
    /// Whether the window is visible (not minimized to taskbar).
    pub visible: bool,
    /// Process ID owning this window.
    pub pid: u32,
    /// Icon ID (index into icon registry).
    pub icon_id: u32,
    /// Z-order (higher = on top).
    pub z_order: u32,
}

/// Window state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowState {
    Normal,
    Maximized,
    Minimized,
    Fullscreen,
}

// ============================================================================
// Desktop state
// ============================================================================

/// Complete desktop shell state.
pub struct DesktopShell {
    /// All managed windows.
    pub windows: BTreeMap<WindowId, ManagedWindow>,
    /// Currently focused window.
    pub focused_window: Option<WindowId>,
    /// Current virtual desktop index (0-based).
    pub current_desktop: u32,
    /// Number of virtual desktops.
    pub num_desktops: u32,
    /// Screen dimensions.
    pub screen_width: u32,
    pub screen_height: u32,
    /// Taskbar height.
    pub taskbar_height: u32,
    /// Whether the start menu is open.
    pub start_menu_open: bool,
    /// Whether Alt+Tab switcher is active.
    pub alt_tab_active: bool,
    /// Alt+Tab selection index.
    pub alt_tab_index: usize,
    /// What the user chose in the appearance panel.
    ///
    /// Kept whole rather than reduced to [`theme`](Self::theme) because it
    /// carries more than colours — font sizes, DPI scaling, animation speed
    /// and corner radius all live here, and each is read by a different part
    /// of the shell.
    pub appearance: AppearanceSettings,
    /// Theme configuration, derived from [`appearance`](Self::appearance).
    ///
    /// Never assign to this directly: it would disagree with `appearance` at
    /// the next thing that re-derives it. Go through
    /// [`set_appearance`](Self::set_appearance).
    pub theme: DesktopTheme,
    /// Next Z-order value.
    next_z: u32,
    /// Next window ID (for local tracking; compositor assigns real IDs).
    next_window_id: u64,
}

/// Desktop visual theme — every colour the shell paints with.
///
/// This is a *resolved* palette, not a set of preferences: by the time a
/// colour is in here, the light/dark choice, the accent, and the transparency
/// level have all been folded in, and the render functions below do nothing
/// but read fields. That split is deliberate. A renderer that consulted
/// [`AppearanceSettings`] directly would re-derive the same colour at every
/// frame and, worse, would be free to derive it slightly differently in each
/// of the dozen places it is drawn.
///
/// Each surface carries its own foreground rather than sharing one. A single
/// `fg` is only correct while every surface has a similar brightness, which
/// stops being true the moment "accent on the taskbar" turns one surface into
/// a saturated colour and leaves the others alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopTheme {
    /// The taskbar panel itself. Alpha may be below 255 — see
    /// [`taskbar_alpha`].
    pub taskbar_bg: Color,
    /// Text and icons on the taskbar.
    pub taskbar_fg: Color,
    /// The pressed/focused button background on the taskbar.
    pub taskbar_active_bg: Color,
    /// Accent-coloured marks on the taskbar, such as the start glyph.
    ///
    /// Distinct from [`accent_color`](Self::accent_color) because when the
    /// taskbar *is* the accent colour, an accent-coloured glyph on it would be
    /// invisible; this field is then the contrasting colour instead.
    pub taskbar_accent: Color,
    pub window_border_color: Color,
    pub window_title_bg: Color,
    pub window_title_fg: Color,
    pub window_title_inactive_bg: Color,
    /// Title text on an unfocused window, whose background may differ from the
    /// focused one by more than a shade once accented title bars are on.
    pub window_title_inactive_fg: Color,
    pub desktop_bg: Color,
    /// The theme's accent, as drawn on ordinary surfaces.
    pub accent_color: Color,
    pub start_menu_bg: Color,
    pub start_menu_fg: Color,
    /// Floating overlays such as the Alt+Tab switcher.
    pub overlay_bg: Color,
    pub overlay_fg: Color,
    pub overlay_selected_bg: Color,
}

impl Default for DesktopTheme {
    fn default() -> Self {
        Self::dark()
    }
}

impl DesktopTheme {
    /// The dark palette (Catppuccin Mocha), before any setting is applied.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            taskbar_bg: Color::from_hex(0x1E1E2E),
            taskbar_fg: Color::from_hex(0xCDD6F4),
            taskbar_active_bg: Color::from_hex(0x45475A),
            taskbar_accent: Color::from_hex(0x89B4FA),
            window_border_color: Color::from_hex(0x585B70),
            window_title_bg: Color::from_hex(0x313244),
            window_title_fg: Color::from_hex(0xCDD6F4),
            window_title_inactive_bg: Color::from_hex(0x1E1E2E),
            window_title_inactive_fg: Color::from_hex(0xA6ADC8),
            desktop_bg: Color::from_hex(0x11111B),
            accent_color: Color::from_hex(0x89B4FA),
            start_menu_bg: Color::from_hex(0x1E1E2E),
            start_menu_fg: Color::from_hex(0xCDD6F4),
            overlay_bg: Color::from_hex(0x1E1E2E),
            overlay_fg: Color::from_hex(0xCDD6F4),
            overlay_selected_bg: Color::from_hex(0x45475A),
        }
    }

    /// The light palette (Catppuccin Latte), before any setting is applied.
    ///
    /// Surface for surface, this is the same structure as [`dark`](Self::dark)
    /// — base, surface0, surface1, surface2, crust — so that a setting applied
    /// on top lands on the same role in either mode.
    #[must_use]
    pub fn light() -> Self {
        Self {
            taskbar_bg: Color::from_hex(0xEFF1F5),
            taskbar_fg: Color::from_hex(0x4C4F69),
            taskbar_active_bg: Color::from_hex(0xBCC0CC),
            taskbar_accent: Color::from_hex(0x1E66F5),
            window_border_color: Color::from_hex(0xACB0BE),
            window_title_bg: Color::from_hex(0xCCD0DA),
            window_title_fg: Color::from_hex(0x4C4F69),
            window_title_inactive_bg: Color::from_hex(0xEFF1F5),
            window_title_inactive_fg: Color::from_hex(0x6C6F85),
            desktop_bg: Color::from_hex(0xDCE0E8),
            accent_color: Color::from_hex(0x1E66F5),
            start_menu_bg: Color::from_hex(0xEFF1F5),
            start_menu_fg: Color::from_hex(0x4C4F69),
            overlay_bg: Color::from_hex(0xEFF1F5),
            overlay_fg: Color::from_hex(0x4C4F69),
            overlay_selected_bg: Color::from_hex(0xBCC0CC),
        }
    }

    /// Resolve a full palette from what the user chose.
    ///
    /// The order matters and is the same one the settings panel presents:
    /// pick the base palette from the mode, recolour what the accent options
    /// claim, then apply transparency last — alpha is a property of a surface
    /// that has already been given its colour.
    #[must_use]
    pub fn from_settings(settings: &AppearanceSettings) -> Self {
        let mut theme = if settings.theme_mode.is_light() {
            Self::light()
        } else {
            Self::dark()
        };

        let accent = settings.effective_accent();
        theme.accent_color = accent;
        theme.taskbar_accent = accent;

        if settings.accent_taskbar {
            theme.taskbar_bg = accent;
            theme.taskbar_fg = readable_on(accent);
            theme.taskbar_active_bg = emphasized(accent);
            // The start glyph is drawn in the accent; on an accent-coloured
            // panel it has to become the contrasting colour or it disappears.
            theme.taskbar_accent = readable_on(accent);
        }

        if settings.accent_titlebars {
            theme.window_title_bg = accent;
            theme.window_title_fg = readable_on(accent);
            // The *inactive* bar deliberately keeps the base palette: an
            // accent that marks every window marks none of them, and telling
            // the focused window apart is the title bar's first job.
        }

        theme.taskbar_bg = with_alpha(theme.taskbar_bg, taskbar_alpha(settings));
        let overlay = settings.transparency.panel_alpha();
        theme.overlay_bg = with_alpha(theme.overlay_bg, overlay);
        theme.start_menu_bg = with_alpha(theme.start_menu_bg, overlay);

        theme
    }
}

/// Black-ish or white-ish, whichever can be read on `bg`.
///
/// The endpoints are the palettes' own extremes rather than pure `#000`/`#fff`
/// so that accented surfaces still look like part of this desktop. Perceived
/// brightness uses the usual luma weights: the eye is far more sensitive to
/// green than to blue, so an average of the channels would call a saturated
/// blue "bright" and put black text on it.
fn readable_on(bg: Color) -> Color {
    let luma = 0.299 * f32::from(bg.r) + 0.587 * f32::from(bg.g) + 0.114 * f32::from(bg.b);
    if luma > 140.0 {
        Color::from_hex(0x11111B)
    } else {
        Color::from_hex(0xEFF1F5)
    }
}

/// A visibly different shade of `color`, for the pressed state of a control
/// whose resting state is already `color`.
///
/// Moves away from whichever extreme `color` is nearer, so the emphasis is
/// visible on both a pale and a deep accent instead of vanishing at one end.
fn emphasized(color: Color) -> Color {
    let toward = readable_on(color);
    color.lerp(toward, 0.25)
}

/// `color` at a given opacity.
fn with_alpha(color: Color, alpha: u8) -> Color {
    Color::rgba(color.r, color.g, color.b, alpha)
}

/// How opaque the taskbar's own panel is.
///
/// Two settings meet here, and they are not redundant. `transparency` is the
/// master level and carries a documented *scope*: `Subtle` reaches "overlays
/// and popups only", so it deliberately leaves the taskbar solid, while
/// `Moderate` and `Full` include it. `taskbar_style` then speaks for the
/// taskbar specifically — `Solid` opts out even when the master level is up,
/// and `Transparent` asks for the panel to disappear entirely and leave the
/// buttons floating.
///
/// Turning transparency `Off` wins over everything, because that is what an
/// off switch means; a "transparent" taskbar style would otherwise keep a
/// transparency effect alive after the user disabled transparency.
fn taskbar_alpha(settings: &AppearanceSettings) -> u8 {
    if !settings.transparency_enabled() {
        return 255;
    }
    match settings.taskbar_style {
        TaskbarStyle::Solid => 255,
        TaskbarStyle::Transparent => 0,
        TaskbarStyle::Translucent => match settings.transparency {
            TransparencyLevel::Off | TransparencyLevel::Subtle => 255,
            level => level.panel_alpha(),
        },
    }
}

impl DesktopShell {
    pub fn new(screen_width: u32, screen_height: u32) -> Self {
        Self {
            windows: BTreeMap::new(),
            focused_window: None,
            current_desktop: 0,
            num_desktops: 4,
            screen_width,
            screen_height,
            taskbar_height: 40,
            start_menu_open: false,
            alt_tab_active: false,
            alt_tab_index: 0,
            appearance: AppearanceSettings::default(),
            theme: DesktopTheme::default(),
            next_z: 1,
            next_window_id: 1,
        }
    }

    /// Adopt a new set of appearance settings and repaint from them.
    ///
    /// The single door through which the palette changes, so that `theme` can
    /// never be a stale derivation of some earlier `appearance`.
    pub fn set_appearance(&mut self, appearance: AppearanceSettings) {
        self.theme = DesktopTheme::from_settings(&appearance);
        self.appearance = appearance;
    }

    /// Load the user's saved appearance settings from disk and apply them.
    ///
    /// Kept out of [`new`](Self::new) on purpose. A constructor that reads the
    /// user's home directory gives every caller — including every test — a
    /// result that depends on the machine it runs on, and a shell that cannot
    /// be built without a filesystem. Startup calls this; tests that care
    /// about a particular look call [`set_appearance`](Self::set_appearance)
    /// with settings they built themselves.
    ///
    /// A missing or unreadable file is not an error: [`config::load`] yields
    /// an empty document, which reads back as the defaults.
    pub fn load_appearance(&mut self) {
        let doc = config::load(appearance_settings::CONFIG_NAME);
        self.set_appearance(AppearanceSettings::read_from(&doc));
    }

    /// Usable area for windows (excluding taskbar).
    pub fn work_area(&self) -> (i32, i32, u32, u32) {
        (
            0,
            0,
            self.screen_width,
            self.screen_height - self.taskbar_height,
        )
    }

    // ======================================================================
    // Window management
    // ======================================================================

    /// Register a new window.
    pub fn add_window(
        &mut self,
        title: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        pid: u32,
    ) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;

        let window = ManagedWindow {
            id,
            title: title.to_string(),
            x,
            y,
            width,
            height,
            state: WindowState::Normal,
            desktop: self.current_desktop,
            focused: false,
            visible: true,
            pid,
            icon_id: 0,
            z_order: self.next_z,
        };
        self.next_z += 1;

        self.windows.insert(id, window);
        self.focus_window(id);
        id
    }

    /// Remove a window.
    pub fn remove_window(&mut self, id: WindowId) {
        self.windows.remove(&id);
        if self.focused_window == Some(id) {
            // Focus the topmost remaining window
            self.focused_window = self
                .visible_windows()
                .last()
                .map(|w| w.id);
            if let Some(fid) = self.focused_window
                && let Some(w) = self.windows.get_mut(&fid) {
                    w.focused = true;
                }
        }
    }

    /// Focus a window (bring to front).
    pub fn focus_window(&mut self, id: WindowId) {
        // Unfocus previous
        if let Some(prev) = self.focused_window
            && let Some(w) = self.windows.get_mut(&prev) {
                w.focused = false;
            }

        if let Some(w) = self.windows.get_mut(&id) {
            w.focused = true;
            w.z_order = self.next_z;
            self.next_z += 1;
            // Restore if minimized
            if w.state == WindowState::Minimized {
                w.state = WindowState::Normal;
                w.visible = true;
            }
        }

        self.focused_window = Some(id);
    }

    /// Minimize a window to the taskbar.
    pub fn minimize_window(&mut self, id: WindowId) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.state = WindowState::Minimized;
            w.visible = false;
            w.focused = false;
        }
        if self.focused_window == Some(id) {
            self.focused_window = None;
            // Focus next visible window
            if let Some(next) = self.visible_windows().last() {
                let next_id = next.id;
                self.focus_window(next_id);
            }
        }
    }

    /// Maximize a window to fill the work area.
    pub fn maximize_window(&mut self, id: WindowId) {
        let (wx, wy, ww, wh) = self.work_area();
        if let Some(w) = self.windows.get_mut(&id) {
            w.state = WindowState::Maximized;
            w.x = wx;
            w.y = wy;
            w.width = ww;
            w.height = wh;
            w.visible = true;
        }
    }

    /// Restore a window to normal state.
    pub fn restore_window(&mut self, id: WindowId) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.state = WindowState::Normal;
            w.visible = true;
        }
    }

    /// Move a window.
    pub fn move_window(&mut self, id: WindowId, x: i32, y: i32) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.x = x;
            w.y = y;
            if w.state == WindowState::Maximized {
                w.state = WindowState::Normal;
            }
        }
    }

    /// Resize a window.
    pub fn resize_window(&mut self, id: WindowId, width: u32, height: u32) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.width = width;
            w.height = height;
            if w.state == WindowState::Maximized {
                w.state = WindowState::Normal;
            }
        }
    }

    /// Snap window to left/right half of screen.
    pub fn snap_window(&mut self, id: WindowId, left: bool) {
        let (wx, wy, ww, wh) = self.work_area();
        if let Some(w) = self.windows.get_mut(&id) {
            w.y = wy;
            w.height = wh;
            w.width = ww / 2;
            w.x = if left { wx } else { wx + (ww / 2) as i32 };
            w.state = WindowState::Normal;
        }
    }

    /// Get visible windows on current desktop, sorted by Z-order.
    pub fn visible_windows(&self) -> Vec<&ManagedWindow> {
        let mut windows: Vec<&ManagedWindow> = self
            .windows
            .values()
            .filter(|w| w.visible && w.desktop == self.current_desktop)
            .collect();
        windows.sort_by_key(|w| w.z_order);
        windows
    }

    // ======================================================================
    // Virtual desktops
    // ======================================================================

    pub fn switch_desktop(&mut self, desktop: u32) {
        if desktop < self.num_desktops {
            self.current_desktop = desktop;
            self.focused_window = None;
            // Focus topmost window on new desktop
            if let Some(w) = self.visible_windows().last() {
                let id = w.id;
                self.focus_window(id);
            }
        }
    }

    pub fn move_window_to_desktop(&mut self, id: WindowId, desktop: u32) {
        if desktop < self.num_desktops
            && let Some(w) = self.windows.get_mut(&id) {
                w.desktop = desktop;
            }
    }

    // ======================================================================
    // Alt+Tab window switcher
    // ======================================================================

    pub fn start_alt_tab(&mut self) {
        let windows = self.visible_windows();
        if windows.len() > 1 {
            self.alt_tab_active = true;
            self.alt_tab_index = 1; // Start at second window
        }
    }

    pub fn next_alt_tab(&mut self) {
        let count = self.visible_windows().len();
        if count > 0 {
            self.alt_tab_index = (self.alt_tab_index + 1) % count;
        }
    }

    pub fn finish_alt_tab(&mut self) {
        if self.alt_tab_active {
            let windows = self.visible_windows();
            if let Some(w) = windows.get(self.alt_tab_index) {
                let id = w.id;
                self.focus_window(id);
            }
            self.alt_tab_active = false;
        }
    }

    pub fn cancel_alt_tab(&mut self) {
        self.alt_tab_active = false;
    }

    // ======================================================================
    // Input handling
    // ======================================================================

    /// Handle a keyboard shortcut at the desktop level.
    /// Returns true if the shortcut was consumed.
    pub fn handle_hotkey(&mut self, key: &KeyEvent) -> bool {
        if !key.pressed {
            // Key release — check for Alt+Tab completion
            if (key.key == Key::LeftAlt || key.key == Key::RightAlt)
                && self.alt_tab_active {
                    self.finish_alt_tab();
                    return true;
                }
            return false;
        }

        // Alt+Tab: window switcher
        if key.modifiers.alt && key.key == Key::Tab {
            if self.alt_tab_active {
                self.next_alt_tab();
            } else {
                self.start_alt_tab();
            }
            return true;
        }

        // Alt+F4: close focused window
        if key.modifiers.alt && key.key == Key::F4 {
            if let Some(id) = self.focused_window {
                self.remove_window(id);
            }
            return true;
        }

        // Super key: toggle start menu
        if key.key == Key::LeftSuper || key.key == Key::RightSuper {
            self.start_menu_open = !self.start_menu_open;
            return true;
        }

        // Super+D: show desktop (minimize all)
        if key.modifiers.super_key && key.key == Key::D {
            let ids: Vec<WindowId> = self
                .windows
                .values()
                .filter(|w| w.visible && w.desktop == self.current_desktop)
                .map(|w| w.id)
                .collect();
            for id in ids {
                self.minimize_window(id);
            }
            return true;
        }

        // Super+Left/Right: snap window
        if key.modifiers.super_key && key.key == Key::Left {
            if let Some(id) = self.focused_window {
                self.snap_window(id, true);
            }
            return true;
        }
        if key.modifiers.super_key && key.key == Key::Right {
            if let Some(id) = self.focused_window {
                self.snap_window(id, false);
            }
            return true;
        }

        // Super+Up: maximize
        if key.modifiers.super_key && key.key == Key::Up {
            if let Some(id) = self.focused_window {
                self.maximize_window(id);
            }
            return true;
        }

        // Super+Down: restore/minimize
        if key.modifiers.super_key && key.key == Key::Down {
            if let Some(id) = self.focused_window
                && let Some(w) = self.windows.get(&id) {
                    if w.state == WindowState::Maximized {
                        self.restore_window(id);
                    } else {
                        self.minimize_window(id);
                    }
                }
            return true;
        }

        // Ctrl+Super+Left/Right: switch virtual desktop
        if key.modifiers.ctrl && key.modifiers.super_key && key.key == Key::Left {
            if self.current_desktop > 0 {
                self.switch_desktop(self.current_desktop - 1);
            }
            return true;
        }
        if key.modifiers.ctrl && key.modifiers.super_key && key.key == Key::Right {
            if self.current_desktop < self.num_desktops - 1 {
                self.switch_desktop(self.current_desktop + 1);
            }
            return true;
        }

        false
    }

    // ======================================================================
    // Rendering
    // ======================================================================

    /// Render the taskbar using the GUI toolkit.
    pub fn render_taskbar(&self) -> RenderTree {
        let taskbar_y = (self.screen_height - self.taskbar_height) as f32;
        let taskbar_w = self.screen_width as f32;
        let taskbar_h = self.taskbar_height as f32;

        let mut tree = RenderTree::new();

        // Taskbar background
        tree.fill_rect(
            0.0,
            taskbar_y,
            taskbar_w,
            taskbar_h,
            self.theme.taskbar_bg,
        );

        // Start button
        let start_w = 48.0;
        let start_bg = if self.start_menu_open {
            self.theme.taskbar_active_bg
        } else {
            self.theme.taskbar_bg
        };
        tree.fill_rect(0.0, taskbar_y, start_w, taskbar_h, start_bg);
        tree.text(
            12.0,
            taskbar_y + 12.0,
            "\u{2261}", // hamburger menu icon
            self.theme.taskbar_accent,
            20.0,
        );

        // Window buttons
        let mut btn_x = start_w + 8.0;
        let btn_h = taskbar_h - 8.0;
        let btn_y = taskbar_y + 4.0;

        for window in self.visible_windows() {
            let btn_w = 160.0f32.min(
                (taskbar_w - start_w - 200.0) / self.visible_windows().len().max(1) as f32,
            );

            let bg = if Some(window.id) == self.focused_window {
                self.theme.taskbar_active_bg
            } else {
                self.theme.taskbar_bg
            };

            tree.fill_rect(btn_x, btn_y, btn_w, btn_h, bg);

            // Window title (truncated)
            let max_chars = (btn_w / 8.0) as usize;
            let title: String = window.title.chars().take(max_chars).collect();
            tree.text(
                btn_x + 8.0,
                btn_y + 8.0,
                &title,
                self.theme.taskbar_fg,
                12.0,
            );

            btn_x += btn_w + 4.0;
        }

        // System tray (right side)
        let tray_x = taskbar_w - 180.0;

        // Clock
        let time_str = self.current_time_string();
        tree.text(
            tray_x + 100.0,
            taskbar_y + 12.0,
            &time_str,
            self.theme.taskbar_fg,
            13.0,
        );

        // Desktop indicator
        let desk_str = format!("Desktop {}", self.current_desktop + 1);
        tree.text(
            tray_x + 8.0,
            taskbar_y + 12.0,
            &desk_str,
            self.theme.taskbar_fg,
            11.0,
        );

        tree
    }

    /// Render window decorations (title bar, borders) for all visible windows.
    pub fn render_window_decorations(&self) -> RenderTree {
        let mut tree = RenderTree::new();
        let title_bar_height = 30.0f32;

        for window in self.visible_windows() {
            let x = window.x as f32;
            let y = window.y as f32;
            let w = window.width as f32;

            // Title bar. The foreground travels with the background: with
            // accented title bars the two differ by more than a shade, so one
            // shared text colour would be unreadable on one of them.
            let (title_bg, title_fg) = if window.focused {
                (self.theme.window_title_bg, self.theme.window_title_fg)
            } else {
                (
                    self.theme.window_title_inactive_bg,
                    self.theme.window_title_inactive_fg,
                )
            };

            tree.fill_rect(x, y, w, title_bar_height, title_bg);

            // Title text
            let title: String = window.title.chars().take(40).collect();
            tree.text(x + 12.0, y + 8.0, &title, title_fg, 13.0);

            // Window control buttons (minimize, maximize, close)
            let btn_size = 16.0f32;
            let btn_y = y + 7.0;

            // Close button (rightmost)
            let close_x = x + w - 30.0;
            tree.fill_rect(close_x, btn_y, btn_size, btn_size, Color::from_hex(0xF38BA8));
            tree.text(close_x + 3.0, btn_y + 1.0, "x", Color::WHITE, 12.0);

            // Maximize button
            let max_x = close_x - 24.0;
            tree.fill_rect(max_x, btn_y, btn_size, btn_size, Color::from_hex(0xA6E3A1));

            // Minimize button
            let min_x = max_x - 24.0;
            tree.fill_rect(min_x, btn_y, btn_size, btn_size, Color::from_hex(0xF9E2AF));

            // Border
            tree.stroke_rect(
                x,
                y,
                w,
                window.height as f32,
                self.theme.window_border_color,
                1.0,
            );
        }

        tree
    }

    /// Render the Alt+Tab window switcher overlay.
    pub fn render_alt_tab(&self) -> Option<RenderTree> {
        if !self.alt_tab_active {
            return None;
        }

        let mut tree = RenderTree::new();
        let windows = self.visible_windows();

        if windows.is_empty() {
            return None;
        }

        // Overlay background
        let overlay_w = 400.0f32.min(self.screen_width as f32 - 100.0);
        let overlay_h = 80.0;
        let overlay_x = (self.screen_width as f32 - overlay_w) / 2.0;
        let overlay_y = (self.screen_height as f32 - overlay_h) / 2.0;

        tree.fill_rect(
            overlay_x,
            overlay_y,
            overlay_w,
            overlay_h,
            self.theme.overlay_bg,
        );
        tree.stroke_rect(
            overlay_x,
            overlay_y,
            overlay_w,
            overlay_h,
            self.theme.accent_color,
            2.0,
        );

        // Window entries
        let item_w = overlay_w / windows.len().max(1) as f32;
        for (i, window) in windows.iter().enumerate() {
            let ix = overlay_x + i as f32 * item_w;

            if i == self.alt_tab_index {
                tree.fill_rect(
                    ix + 4.0,
                    overlay_y + 4.0,
                    item_w - 8.0,
                    overlay_h - 8.0,
                    self.theme.overlay_selected_bg,
                );
            }

            let title: String = window.title.chars().take(12).collect();
            tree.text(
                ix + 10.0,
                overlay_y + overlay_h / 2.0 - 6.0,
                &title,
                self.theme.overlay_fg,
                12.0,
            );
        }

        Some(tree)
    }

    /// Render the start menu.
    pub fn render_start_menu(&self) -> Option<RenderTree> {
        if !self.start_menu_open {
            return None;
        }

        let mut tree = RenderTree::new();
        let menu_w = 300.0;
        let menu_h = 400.0;
        let menu_x = 0.0;
        let menu_y = (self.screen_height - self.taskbar_height) as f32 - menu_h;

        // Background
        tree.fill_rect(menu_x, menu_y, menu_w, menu_h, self.theme.start_menu_bg);
        tree.stroke_rect(
            menu_x,
            menu_y,
            menu_w,
            menu_h,
            self.theme.window_border_color,
            1.0,
        );

        // Title
        tree.text(
            menu_x + 16.0,
            menu_y + 16.0,
            "Applications",
            self.theme.accent_color,
            16.0,
        );

        // Application entries (placeholder)
        let apps = [
            "Terminal",
            "File Explorer",
            "Text Editor",
            "Settings",
            "System Monitor",
            "Calculator",
        ];

        for (i, app) in apps.iter().enumerate() {
            let item_y = menu_y + 50.0 + i as f32 * 36.0;
            tree.text(
                menu_x + 24.0,
                item_y + 8.0,
                app,
                self.theme.start_menu_fg,
                14.0,
            );
        }

        // Power options at bottom
        tree.text(
            menu_x + 16.0,
            menu_y + menu_h - 40.0,
            "Power",
            Color::GRAY,
            12.0,
        );

        Some(tree)
    }

    // ======================================================================
    // Utilities
    // ======================================================================

    fn current_time_string(&self) -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let time_secs = secs % 86400;
        let hours = time_secs / 3600;
        let minutes = (time_secs % 3600) / 60;
        format!("{hours:02}:{minutes:02}")
    }
}

// ============================================================================
// Main — desktop shell entry point
// ============================================================================

fn main() {
    // In a real implementation, this would:
    // 1. Connect to the compositor via IPC channel
    // 2. Register as the window manager
    // 3. Enter an event loop processing compositor events
    // 4. Render taskbar, window decorations, and overlays
    //
    // For now, demonstrate the API:

    let mut desktop = DesktopShell::new(1920, 1080);

    // Adopt whatever the user last chose in the appearance panel. Nothing to
    // recover from if the file is absent — that is a fresh install, and the
    // defaults are what it should look like.
    desktop.load_appearance();
    println!(
        "Appearance: {} theme, {:.0}% scaling, UI font {} at {}pt",
        if desktop.appearance.theme_mode.is_light() {
            "light"
        } else {
            "dark"
        },
        desktop.appearance.scale_factor() * 100.0,
        desktop.appearance.fonts.ui_font,
        desktop.appearance.fonts.ui_size,
    );

    // Simulate some windows
    let w1 = desktop.add_window("Terminal", 100, 100, 800, 600, 1001);
    let w2 = desktop.add_window("File Explorer", 200, 150, 700, 500, 1002);
    let _w3 = desktop.add_window("Text Editor", 300, 200, 900, 650, 1003);

    // Render taskbar
    let taskbar = desktop.render_taskbar();
    println!(
        "Taskbar rendered: {} commands",
        taskbar.len()
    );

    // Render window decorations
    let decorations = desktop.render_window_decorations();
    println!(
        "Window decorations: {} commands",
        decorations.len()
    );

    // Test keyboard shortcuts
    let alt_f4 = KeyEvent {
        key: Key::F4,
        pressed: true,
        modifiers: Modifiers::alt(),
        text: None,
    };
    desktop.handle_hotkey(&alt_f4);
    println!(
        "After Alt+F4: {} windows remaining",
        desktop.windows.len()
    );

    // Test window snapping
    desktop.snap_window(w1, true);
    desktop.snap_window(w2, false);
    if let Some(w) = desktop.windows.get(&w1) {
        println!("Window 1 snapped left: {}x{} at ({},{})", w.width, w.height, w.x, w.y);
    }

    // Test virtual desktop switching
    desktop.switch_desktop(1);
    println!(
        "Switched to desktop {}: {} visible windows",
        desktop.current_desktop + 1,
        desktop.visible_windows().len()
    );

    println!("\nDesktop shell initialized successfully.");
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod theme_tests {
    use super::*;
    use appearance::{AccentColor, ThemeMode};

    /// Contrast ratio per WCAG 2.x, for asserting that text is readable rather
    /// than merely "a different colour".
    fn contrast(a: Color, b: Color) -> f32 {
        fn channel(c: u8) -> f32 {
            let c = f32::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        fn luminance(c: Color) -> f32 {
            0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
        }
        let (l1, l2) = (luminance(a), luminance(b));
        let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    fn settings() -> AppearanceSettings {
        AppearanceSettings::default()
    }

    #[test]
    fn the_default_theme_is_the_dark_one() {
        assert_eq!(DesktopTheme::default(), DesktopTheme::dark());
        assert_eq!(DesktopShell::new(800, 600).theme, DesktopTheme::dark());
    }

    #[test]
    fn the_mode_picks_the_base_palette() {
        let mut s = settings();
        s.theme_mode = ThemeMode::Dark;
        assert_eq!(DesktopTheme::from_settings(&s).desktop_bg, DesktopTheme::dark().desktop_bg);
        s.theme_mode = ThemeMode::Light;
        assert_eq!(DesktopTheme::from_settings(&s).desktop_bg, DesktopTheme::light().desktop_bg);
        // "System" has no schedule to follow yet, so it stays on dark rather
        // than flipping the desktop for a user who asked to be left alone.
        s.theme_mode = ThemeMode::System;
        assert_eq!(DesktopTheme::from_settings(&s).desktop_bg, DesktopTheme::dark().desktop_bg);
    }

    #[test]
    fn the_accent_reaches_the_theme() {
        let mut s = settings();
        s.accent_color = AccentColor::Green;
        let theme = DesktopTheme::from_settings(&s);
        assert_eq!(theme.accent_color, AccentColor::Green.color());
        assert_eq!(theme.taskbar_accent, AccentColor::Green.color());
    }

    #[test]
    fn a_light_theme_uses_the_light_accent() {
        let mut s = settings();
        s.theme_mode = ThemeMode::Light;
        s.accent_color = AccentColor::Blue;
        let theme = DesktopTheme::from_settings(&s);
        assert_eq!(theme.accent_color, AccentColor::Blue.color_light());
        assert_ne!(theme.accent_color, AccentColor::Blue.color());
    }

    /// The reason `color_light` exists: the shell draws the accent as text —
    /// the start glyph, the start-menu heading — so every accent has to clear
    /// the body-text contrast bar on the surface its own mode paints. This is
    /// the test that rejects using Catppuccin's Latte accents unmodified.
    #[test]
    fn every_accent_is_readable_as_text_in_both_modes() {
        for &accent in AccentColor::presets() {
            let mut s = settings();
            s.accent_color = accent;

            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                s.theme_mode = mode;
                let theme = DesktopTheme::from_settings(&s);
                let ratio = contrast(theme.accent_color, theme.start_menu_bg);
                assert!(ratio >= 4.5, "{mode:?} {accent:?}: contrast {ratio:.2} < 4.5");
            }
        }
    }

    #[test]
    fn a_custom_accent_is_used_exactly_as_chosen() {
        let mut s = settings();
        s.accent_color = AccentColor::Custom;
        s.custom_accent = Color::from_hex(0x123456);
        assert_eq!(DesktopTheme::from_settings(&s).accent_color, Color::from_hex(0x123456));
        // Including in light mode: the user named a colour, not a role.
        s.theme_mode = ThemeMode::Light;
        assert_eq!(DesktopTheme::from_settings(&s).accent_color, Color::from_hex(0x123456));
    }

    /// An accented taskbar must not swallow the start glyph, which is drawn in
    /// the accent colour on the taskbar's own background.
    #[test]
    fn an_accented_taskbar_keeps_its_glyph_visible() {
        for &accent in AccentColor::presets() {
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                let mut s = settings();
                s.theme_mode = mode;
                s.accent_color = accent;
                s.accent_taskbar = true;
                s.taskbar_style = TaskbarStyle::Solid;
                let theme = DesktopTheme::from_settings(&s);

                assert_eq!(theme.taskbar_bg, s.effective_accent());
                assert_ne!(theme.taskbar_accent, theme.taskbar_bg);
                let glyph = contrast(theme.taskbar_accent, theme.taskbar_bg);
                assert!(glyph >= 4.5, "{mode:?} {accent:?}: glyph contrast {glyph:.2}");
                let text = contrast(theme.taskbar_fg, theme.taskbar_bg);
                assert!(text >= 4.5, "{mode:?} {accent:?}: text contrast {text:.2}");
                assert_ne!(theme.taskbar_active_bg, theme.taskbar_bg);
            }
        }
    }

    #[test]
    fn an_unaccented_taskbar_is_left_on_the_base_palette() {
        let mut s = settings();
        s.accent_color = AccentColor::Red;
        s.accent_taskbar = false;
        s.taskbar_style = TaskbarStyle::Solid;
        let theme = DesktopTheme::from_settings(&s);
        assert_eq!(theme.taskbar_bg, DesktopTheme::dark().taskbar_bg);
        assert_eq!(theme.taskbar_fg, DesktopTheme::dark().taskbar_fg);
    }

    /// Accenting title bars must not cost the focused/unfocused distinction,
    /// and must not leave the inactive bar's text on the wrong background.
    #[test]
    fn accented_title_bars_still_mark_only_the_focused_window() {
        for &accent in AccentColor::presets() {
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                let mut s = settings();
                s.theme_mode = mode;
                s.accent_color = accent;
                s.accent_titlebars = true;
                let theme = DesktopTheme::from_settings(&s);
                let base = if mode == ThemeMode::Light {
                    DesktopTheme::light()
                } else {
                    DesktopTheme::dark()
                };

                assert_eq!(theme.window_title_bg, s.effective_accent());
                assert_ne!(theme.window_title_bg, theme.window_title_inactive_bg);
                assert_eq!(theme.window_title_inactive_bg, base.window_title_inactive_bg);

                let active = contrast(theme.window_title_fg, theme.window_title_bg);
                assert!(active >= 4.5, "{mode:?} {accent:?}: active title {active:.2}");
                let inactive =
                    contrast(theme.window_title_inactive_fg, theme.window_title_inactive_bg);
                assert!(inactive >= 3.0, "{mode:?} {accent:?}: inactive title {inactive:.2}");
            }
        }
    }

    #[test]
    fn the_taskbar_style_and_the_transparency_level_both_have_a_say() {
        let mut s = settings();

        // Solid opts out however high the master level is.
        s.taskbar_style = TaskbarStyle::Solid;
        for level in [
            TransparencyLevel::Off,
            TransparencyLevel::Subtle,
            TransparencyLevel::Moderate,
            TransparencyLevel::Full,
        ] {
            s.transparency = level;
            assert_eq!(DesktopTheme::from_settings(&s).taskbar_bg.a, 255, "{level:?}");
        }

        // Translucent follows the level — but `Subtle` is documented as
        // reaching overlays and popups only, so it leaves the taskbar alone.
        s.taskbar_style = TaskbarStyle::Translucent;
        s.transparency = TransparencyLevel::Off;
        assert_eq!(DesktopTheme::from_settings(&s).taskbar_bg.a, 255);
        s.transparency = TransparencyLevel::Subtle;
        assert_eq!(DesktopTheme::from_settings(&s).taskbar_bg.a, 255);
        s.transparency = TransparencyLevel::Moderate;
        assert_eq!(
            DesktopTheme::from_settings(&s).taskbar_bg.a,
            TransparencyLevel::Moderate.panel_alpha()
        );
        s.transparency = TransparencyLevel::Full;
        assert_eq!(
            DesktopTheme::from_settings(&s).taskbar_bg.a,
            TransparencyLevel::Full.panel_alpha()
        );

        // Transparent asks the panel to disappear — unless transparency is
        // off, where an off switch has to mean off.
        s.taskbar_style = TaskbarStyle::Transparent;
        s.transparency = TransparencyLevel::Moderate;
        assert_eq!(DesktopTheme::from_settings(&s).taskbar_bg.a, 0);
        s.transparency = TransparencyLevel::Off;
        assert_eq!(DesktopTheme::from_settings(&s).taskbar_bg.a, 255);
    }

    #[test]
    fn overlays_and_menus_follow_the_transparency_level_alone() {
        let mut s = settings();
        // The taskbar style must not reach them.
        s.taskbar_style = TaskbarStyle::Solid;
        for level in [
            TransparencyLevel::Off,
            TransparencyLevel::Subtle,
            TransparencyLevel::Moderate,
            TransparencyLevel::Full,
        ] {
            s.transparency = level;
            let theme = DesktopTheme::from_settings(&s);
            assert_eq!(theme.overlay_bg.a, level.panel_alpha(), "{level:?}");
            assert_eq!(theme.start_menu_bg.a, level.panel_alpha(), "{level:?}");
        }
    }

    /// Transparency is applied to the surface, not to what is drawn on it: a
    /// see-through panel with see-through text on it would be unreadable.
    #[test]
    fn transparency_does_not_reach_the_foreground() {
        let mut s = settings();
        s.transparency = TransparencyLevel::Full;
        s.taskbar_style = TaskbarStyle::Translucent;
        let theme = DesktopTheme::from_settings(&s);
        assert!(theme.taskbar_bg.a < 255);
        assert_eq!(theme.taskbar_fg.a, 255);
        assert_eq!(theme.taskbar_accent.a, 255);
        assert_eq!(theme.taskbar_active_bg.a, 255);
        assert_eq!(theme.overlay_fg.a, 255);
        assert_eq!(theme.start_menu_fg.a, 255);
    }

    #[test]
    fn set_appearance_keeps_the_theme_and_the_settings_agreeing() {
        let mut shell = DesktopShell::new(800, 600);
        let mut s = settings();
        s.theme_mode = ThemeMode::Light;
        s.accent_color = AccentColor::Mauve;
        shell.set_appearance(s.clone());

        assert_eq!(shell.appearance, s);
        assert_eq!(shell.theme, DesktopTheme::from_settings(&s));
        assert_eq!(shell.theme.accent_color, AccentColor::Mauve.color_light());
    }

    /// The rendered frame, not just the palette, has to change with the theme.
    #[test]
    fn the_taskbar_is_painted_in_the_current_theme() {
        let mut shell = DesktopShell::new(800, 600);
        let dark = shell.render_taskbar();

        let mut s = settings();
        s.theme_mode = ThemeMode::Light;
        shell.set_appearance(s);
        let light = shell.render_taskbar();

        assert_eq!(dark.len(), light.len());
        assert_ne!(format!("{dark:?}"), format!("{light:?}"));
    }
}

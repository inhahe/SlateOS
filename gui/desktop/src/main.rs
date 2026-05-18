//! OurOS Desktop Shell
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
mod startup_settings;
#[allow(dead_code)]
mod datetime_settings;

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
    /// Theme configuration.
    pub theme: DesktopTheme,
    /// Next Z-order value.
    next_z: u32,
    /// Next window ID (for local tracking; compositor assigns real IDs).
    next_window_id: u64,
}

/// Desktop visual theme.
#[derive(Clone, Debug)]
pub struct DesktopTheme {
    pub taskbar_bg: Color,
    pub taskbar_fg: Color,
    pub taskbar_active_bg: Color,
    pub window_border_color: Color,
    pub window_title_bg: Color,
    pub window_title_fg: Color,
    pub window_title_inactive_bg: Color,
    pub desktop_bg: Color,
    pub accent_color: Color,
    pub start_menu_bg: Color,
    pub start_menu_fg: Color,
}

impl Default for DesktopTheme {
    fn default() -> Self {
        Self {
            taskbar_bg: Color::from_hex(0x1E1E2E),
            taskbar_fg: Color::from_hex(0xCDD6F4),
            taskbar_active_bg: Color::from_hex(0x45475A),
            window_border_color: Color::from_hex(0x585B70),
            window_title_bg: Color::from_hex(0x313244),
            window_title_fg: Color::from_hex(0xCDD6F4),
            window_title_inactive_bg: Color::from_hex(0x1E1E2E),
            desktop_bg: Color::from_hex(0x11111B),
            accent_color: Color::from_hex(0x89B4FA),
            start_menu_bg: Color::from_hex(0x1E1E2E),
            start_menu_fg: Color::from_hex(0xCDD6F4),
        }
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
            theme: DesktopTheme::default(),
            next_z: 1,
            next_window_id: 1,
        }
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
            if let Some(fid) = self.focused_window {
                if let Some(w) = self.windows.get_mut(&fid) {
                    w.focused = true;
                }
            }
        }
    }

    /// Focus a window (bring to front).
    pub fn focus_window(&mut self, id: WindowId) {
        // Unfocus previous
        if let Some(prev) = self.focused_window {
            if let Some(w) = self.windows.get_mut(&prev) {
                w.focused = false;
            }
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
        if desktop < self.num_desktops {
            if let Some(w) = self.windows.get_mut(&id) {
                w.desktop = desktop;
            }
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
            if key.key == Key::LeftAlt || key.key == Key::RightAlt {
                if self.alt_tab_active {
                    self.finish_alt_tab();
                    return true;
                }
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
            if let Some(id) = self.focused_window {
                if let Some(w) = self.windows.get(&id) {
                    if w.state == WindowState::Maximized {
                        self.restore_window(id);
                    } else {
                        self.minimize_window(id);
                    }
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
            self.theme.accent_color,
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

            // Title bar
            let title_bg = if window.focused {
                self.theme.window_title_bg
            } else {
                self.theme.window_title_inactive_bg
            };

            tree.fill_rect(x, y, w, title_bar_height, title_bg);

            // Title text
            let title: String = window.title.chars().take(40).collect();
            tree.text(
                x + 12.0,
                y + 8.0,
                &title,
                self.theme.window_title_fg,
                13.0,
            );

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
            Color::rgba(30, 30, 46, 230),
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
                    self.theme.taskbar_active_bg,
                );
            }

            let title: String = window.title.chars().take(12).collect();
            tree.text(
                ix + 10.0,
                overlay_y + overlay_h / 2.0 - 6.0,
                &title,
                self.theme.taskbar_fg,
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

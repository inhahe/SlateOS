//! Taskbar auto-hide behavior module.
//!
//! Manages the auto-hide animation and detection for the taskbar:
//! - Slide-out animation when mouse leaves the taskbar area
//! - Slide-in when mouse approaches the screen edge
//! - Configurable delay before hide
//! - Peek mode (briefly show on notification)
//! - Lock to prevent auto-hide during certain operations
//! - Per-edge detection (supports taskbar on any screen edge)

use guitk::color::Color;
use guitk::render::RenderCommand;
use guitk::style::CornerRadii;

// ============================================================================
// Types
// ============================================================================

/// Which screen edge the taskbar is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenEdge {
    Bottom,
    Top,
    Left,
    Right,
}

/// Current visibility state of the taskbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoHideState {
    /// Fully visible.
    Visible,
    /// Sliding out (hiding).
    SlidingOut,
    /// Fully hidden.
    Hidden,
    /// Sliding in (showing).
    SlidingIn,
    /// Temporarily peeking (e.g., notification).
    Peeking,
}

/// Configuration for auto-hide behavior.
#[derive(Clone, Debug)]
pub struct AutoHideConfig {
    /// Whether auto-hide is enabled.
    pub enabled: bool,
    /// Which edge the taskbar is on.
    pub edge: ScreenEdge,
    /// Delay in ms before hiding after mouse leaves.
    pub hide_delay_ms: u64,
    /// How far (in pixels) the taskbar slides out of view.
    pub slide_distance: f32,
    /// Animation duration in ms.
    pub slide_duration_ms: u64,
    /// Width of the trigger zone at the screen edge (pixels).
    pub trigger_zone_width: f32,
    /// How long to peek for notifications (ms).
    pub peek_duration_ms: u64,
    /// Taskbar thickness (height if bottom/top, width if left/right).
    pub taskbar_size: f32,
    /// Screen width.
    pub screen_width: f32,
    /// Screen height.
    pub screen_height: f32,
}

impl Default for AutoHideConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            edge: ScreenEdge::Bottom,
            hide_delay_ms: 500,
            slide_distance: 48.0,
            slide_duration_ms: 200,
            trigger_zone_width: 4.0,
            peek_duration_ms: 3000,
            taskbar_size: 48.0,
            screen_width: 1920.0,
            screen_height: 1080.0,
        }
    }
}

impl AutoHideConfig {
    /// Parse config from key=value text.
    pub fn from_text(text: &str) -> Self {
        let mut config = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim();
                match key {
                    "enabled" => config.enabled = val == "true",
                    "edge" => {
                        config.edge = match val {
                            "top" => ScreenEdge::Top,
                            "left" => ScreenEdge::Left,
                            "right" => ScreenEdge::Right,
                            _ => ScreenEdge::Bottom,
                        }
                    }
                    "hide_delay_ms" => {
                        if let Ok(v) = val.parse::<u64>() {
                            config.hide_delay_ms = v;
                        }
                    }
                    "slide_duration_ms" => {
                        if let Ok(v) = val.parse::<u64>() {
                            config.slide_duration_ms = v;
                        }
                    }
                    "trigger_zone_width" => {
                        if let Ok(v) = val.parse::<f32>() {
                            config.trigger_zone_width = v.max(1.0);
                        }
                    }
                    "peek_duration_ms" => {
                        if let Ok(v) = val.parse::<u64>() {
                            config.peek_duration_ms = v;
                        }
                    }
                    _ => {}
                }
            }
        }
        config
    }

    /// Serialize to key=value text.
    pub fn to_text(&self) -> String {
        let edge_str = match self.edge {
            ScreenEdge::Bottom => "bottom",
            ScreenEdge::Top => "top",
            ScreenEdge::Left => "left",
            ScreenEdge::Right => "right",
        };
        format!(
            "# Taskbar auto-hide config\nenabled={}\nedge={}\nhide_delay_ms={}\nslide_duration_ms={}\ntrigger_zone_width={}\npeek_duration_ms={}\n",
            self.enabled,
            edge_str,
            self.hide_delay_ms,
            self.slide_duration_ms,
            self.trigger_zone_width,
            self.peek_duration_ms,
        )
    }
}

// ============================================================================
// Auto-hide manager
// ============================================================================

/// Manages the auto-hide state machine and animation.
#[derive(Clone, Debug)]
pub struct AutoHideManager {
    /// Configuration.
    pub config: AutoHideConfig,
    /// Current state.
    pub state: AutoHideState,
    /// Animation progress (0.0 = fully visible, 1.0 = fully hidden).
    pub hide_progress: f32,
    /// Whether the mouse is currently in the taskbar area.
    pub mouse_in_taskbar: bool,
    /// Whether the mouse is in the trigger zone (screen edge).
    pub mouse_in_trigger: bool,
    /// Timestamp when the mouse left the taskbar.
    pub mouse_left_at: u64,
    /// Timestamp when current animation started.
    pub anim_start_ms: u64,
    /// Timestamp when peek started.
    pub peek_start_ms: u64,
    /// Whether auto-hide is locked (e.g., during drag, menu open).
    pub locked: bool,
    /// Number of active locks (for nested locking).
    lock_count: u32,
}

impl AutoHideManager {
    /// Create a new auto-hide manager.
    pub fn new(config: AutoHideConfig) -> Self {
        Self {
            config,
            state: AutoHideState::Visible,
            hide_progress: 0.0,
            mouse_in_taskbar: false,
            mouse_in_trigger: false,
            mouse_left_at: 0,
            anim_start_ms: 0,
            peek_start_ms: 0,
            locked: false,
            lock_count: 0,
        }
    }

    /// Compute the taskbar's offset from its normal position.
    /// Returns (dx, dy) — the amount to translate the taskbar.
    pub fn taskbar_offset(&self) -> (f32, f32) {
        let dist = self.config.slide_distance * self.hide_progress;
        match self.config.edge {
            ScreenEdge::Bottom => (0.0, dist),
            ScreenEdge::Top => (0.0, -dist),
            ScreenEdge::Left => (-dist, 0.0),
            ScreenEdge::Right => (dist, 0.0),
        }
    }

    /// Get the trigger zone rectangle (the thin strip at the screen edge
    /// that re-shows the taskbar).
    pub fn trigger_zone(&self) -> (f32, f32, f32, f32) {
        let w = self.config.screen_width;
        let h = self.config.screen_height;
        let tz = self.config.trigger_zone_width;

        match self.config.edge {
            ScreenEdge::Bottom => (0.0, h - tz, w, tz),
            ScreenEdge::Top => (0.0, 0.0, w, tz),
            ScreenEdge::Left => (0.0, 0.0, tz, h),
            ScreenEdge::Right => (w - tz, 0.0, tz, h),
        }
    }

    /// Get the taskbar's visible area (accounting for auto-hide offset).
    pub fn taskbar_rect(&self) -> (f32, f32, f32, f32) {
        let (ox, oy) = self.taskbar_offset();
        let w = self.config.screen_width;
        let h = self.config.screen_height;
        let size = self.config.taskbar_size;

        match self.config.edge {
            ScreenEdge::Bottom => (0.0, h - size + oy, w, size),
            ScreenEdge::Top => (0.0, oy, w, size),
            ScreenEdge::Left => (ox, 0.0, size, h),
            ScreenEdge::Right => (w - size + ox, 0.0, size, h),
        }
    }

    /// Notify that the mouse entered the taskbar area.
    pub fn on_mouse_enter_taskbar(&mut self, now_ms: u64) {
        self.mouse_in_taskbar = true;

        match self.state {
            AutoHideState::Hidden | AutoHideState::SlidingOut => {
                self.state = AutoHideState::SlidingIn;
                self.anim_start_ms = now_ms;
            }
            AutoHideState::Peeking => {
                // Mouse entered during peek — stay visible
                self.state = AutoHideState::Visible;
                self.hide_progress = 0.0;
            }
            _ => {}
        }
    }

    /// Notify that the mouse left the taskbar area.
    pub fn on_mouse_leave_taskbar(&mut self, now_ms: u64) {
        self.mouse_in_taskbar = false;
        self.mouse_left_at = now_ms;
    }

    /// Notify that the mouse entered the trigger zone (screen edge).
    pub fn on_mouse_enter_trigger(&mut self, now_ms: u64) {
        self.mouse_in_trigger = true;
        if self.state == AutoHideState::Hidden {
            self.state = AutoHideState::SlidingIn;
            self.anim_start_ms = now_ms;
        }
    }

    /// Notify that the mouse left the trigger zone.
    pub fn on_mouse_leave_trigger(&mut self) {
        self.mouse_in_trigger = false;
    }

    /// Lock auto-hide (prevent hiding). Locks are counted — call unlock
    /// the same number of times.
    pub fn lock(&mut self) {
        self.lock_count = self.lock_count.saturating_add(1);
        self.locked = true;
    }

    /// Unlock auto-hide.
    pub fn unlock(&mut self) {
        self.lock_count = self.lock_count.saturating_sub(1);
        if self.lock_count == 0 {
            self.locked = false;
        }
    }

    /// Trigger a peek (briefly show taskbar, e.g., for notification).
    pub fn peek(&mut self, now_ms: u64) {
        if !self.config.enabled {
            return;
        }
        match self.state {
            AutoHideState::Hidden | AutoHideState::SlidingOut => {
                self.state = AutoHideState::SlidingIn;
                self.anim_start_ms = now_ms;
                self.peek_start_ms = now_ms;
            }
            AutoHideState::SlidingIn | AutoHideState::Visible => {
                // Already showing — reset peek timer
                self.peek_start_ms = now_ms;
            }
            AutoHideState::Peeking => {
                self.peek_start_ms = now_ms;
            }
        }
    }

    /// Advance the state machine. Call each frame with the current timestamp.
    /// Returns true if a repaint is needed.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if !self.config.enabled {
            if self.state != AutoHideState::Visible {
                self.state = AutoHideState::Visible;
                self.hide_progress = 0.0;
                return true;
            }
            return false;
        }

        match self.state {
            AutoHideState::Visible => {
                // Check if we should start hiding
                if !self.mouse_in_taskbar
                    && !self.mouse_in_trigger
                    && !self.locked
                    && self.mouse_left_at > 0
                {
                    let elapsed = now_ms.saturating_sub(self.mouse_left_at);
                    if elapsed >= self.config.hide_delay_ms {
                        self.state = AutoHideState::SlidingOut;
                        self.anim_start_ms = now_ms;
                        return true;
                    }
                }
                false
            }
            AutoHideState::SlidingOut => {
                let elapsed = now_ms.saturating_sub(self.anim_start_ms);
                let duration = self.config.slide_duration_ms.max(1);
                let progress = (elapsed as f32) / (duration as f32);

                if progress >= 1.0 {
                    self.hide_progress = 1.0;
                    self.state = AutoHideState::Hidden;
                } else {
                    self.hide_progress = progress;
                }
                true
            }
            AutoHideState::Hidden => {
                false
            }
            AutoHideState::SlidingIn => {
                let elapsed = now_ms.saturating_sub(self.anim_start_ms);
                let duration = self.config.slide_duration_ms.max(1);
                let progress = (elapsed as f32) / (duration as f32);

                if progress >= 1.0 {
                    self.hide_progress = 0.0;
                    // If this was triggered by a peek and mouse isn't in taskbar,
                    // enter peek mode
                    if !self.mouse_in_taskbar && self.peek_start_ms > 0 {
                        self.state = AutoHideState::Peeking;
                        self.peek_start_ms = now_ms;
                    } else {
                        self.state = AutoHideState::Visible;
                    }
                } else {
                    self.hide_progress = 1.0 - progress;
                }
                true
            }
            AutoHideState::Peeking => {
                if self.mouse_in_taskbar {
                    self.state = AutoHideState::Visible;
                    self.peek_start_ms = 0;
                    return true;
                }

                let elapsed = now_ms.saturating_sub(self.peek_start_ms);
                if elapsed >= self.config.peek_duration_ms {
                    self.state = AutoHideState::SlidingOut;
                    self.anim_start_ms = now_ms;
                    self.peek_start_ms = 0;
                    return true;
                }
                false
            }
        }
    }

    /// Whether the taskbar is fully visible (not hiding or hidden).
    pub fn is_fully_visible(&self) -> bool {
        self.state == AutoHideState::Visible && self.hide_progress == 0.0
    }

    /// Whether the taskbar is fully hidden.
    pub fn is_fully_hidden(&self) -> bool {
        self.state == AutoHideState::Hidden && self.hide_progress >= 1.0
    }

    /// Render a thin indicator line at the screen edge when taskbar is hidden.
    pub fn render_edge_indicator(&self) -> Vec<RenderCommand> {
        if self.state != AutoHideState::Hidden {
            return Vec::new();
        }

        let (tx, ty, tw, th) = self.trigger_zone();
        let indicator_color = Color::rgba(100, 100, 120, 80);

        vec![RenderCommand::FillRect {
            x: tx,
            y: ty,
            width: tw,
            height: th,
            color: indicator_color,
            corner_radii: CornerRadii::ZERO,
        }]
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> AutoHideConfig {
        AutoHideConfig {
            enabled: true,
            hide_delay_ms: 100,
            slide_duration_ms: 50,
            peek_duration_ms: 200,
            ..AutoHideConfig::default()
        }
    }

    // ---- Config tests ----

    #[test]
    fn test_config_default() {
        let c = AutoHideConfig::default();
        assert!(c.enabled);
        assert_eq!(c.edge, ScreenEdge::Bottom);
    }

    #[test]
    fn test_config_roundtrip() {
        let c = AutoHideConfig {
            enabled: true,
            edge: ScreenEdge::Top,
            hide_delay_ms: 300,
            ..AutoHideConfig::default()
        };
        let text = c.to_text();
        let c2 = AutoHideConfig::from_text(&text);
        assert_eq!(c2.edge, ScreenEdge::Top);
        assert_eq!(c2.hide_delay_ms, 300);
    }

    #[test]
    fn test_config_parse_empty() {
        let c = AutoHideConfig::from_text("");
        assert!(c.enabled);
    }

    #[test]
    fn test_config_parse_all_edges() {
        for (text, expected) in [
            ("edge=bottom", ScreenEdge::Bottom),
            ("edge=top", ScreenEdge::Top),
            ("edge=left", ScreenEdge::Left),
            ("edge=right", ScreenEdge::Right),
        ] {
            let c = AutoHideConfig::from_text(text);
            assert_eq!(c.edge, expected);
        }
    }

    // ---- Trigger zone tests ----

    #[test]
    fn test_trigger_zone_bottom() {
        let m = AutoHideManager::new(make_config());
        let (x, y, w, h) = m.trigger_zone();
        assert_eq!(x, 0.0);
        assert_eq!(w, 1920.0);
        assert!(y > 1070.0); // Near bottom
        assert!(h > 0.0);
    }

    #[test]
    fn test_trigger_zone_top() {
        let mut config = make_config();
        config.edge = ScreenEdge::Top;
        let m = AutoHideManager::new(config);
        let (x, y, _w, h) = m.trigger_zone();
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
        assert!(h > 0.0);
    }

    // ---- Taskbar offset tests ----

    #[test]
    fn test_offset_visible() {
        let m = AutoHideManager::new(make_config());
        let (dx, dy) = m.taskbar_offset();
        assert_eq!(dx, 0.0);
        assert_eq!(dy, 0.0);
    }

    #[test]
    fn test_offset_hidden_bottom() {
        let mut m = AutoHideManager::new(make_config());
        m.hide_progress = 1.0;
        let (dx, dy) = m.taskbar_offset();
        assert_eq!(dx, 0.0);
        assert!(dy > 0.0); // Moves down (off-screen)
    }

    #[test]
    fn test_offset_hidden_top() {
        let mut config = make_config();
        config.edge = ScreenEdge::Top;
        let mut m = AutoHideManager::new(config);
        m.hide_progress = 1.0;
        let (dx, dy) = m.taskbar_offset();
        assert_eq!(dx, 0.0);
        assert!(dy < 0.0); // Moves up (off-screen)
    }

    #[test]
    fn test_offset_hidden_left() {
        let mut config = make_config();
        config.edge = ScreenEdge::Left;
        let mut m = AutoHideManager::new(config);
        m.hide_progress = 1.0;
        let (dx, _dy) = m.taskbar_offset();
        assert!(dx < 0.0);
    }

    #[test]
    fn test_offset_hidden_right() {
        let mut config = make_config();
        config.edge = ScreenEdge::Right;
        let mut m = AutoHideManager::new(config);
        m.hide_progress = 1.0;
        let (dx, _dy) = m.taskbar_offset();
        assert!(dx > 0.0);
    }

    // ---- State machine tests ----

    #[test]
    fn test_initial_state_visible() {
        let m = AutoHideManager::new(make_config());
        assert_eq!(m.state, AutoHideState::Visible);
        assert!(m.is_fully_visible());
    }

    #[test]
    fn test_mouse_leave_starts_hide_after_delay() {
        let mut m = AutoHideManager::new(make_config());
        m.on_mouse_leave_taskbar(1000);
        assert_eq!(m.state, AutoHideState::Visible); // Still visible

        m.tick(1050); // Not enough time
        assert_eq!(m.state, AutoHideState::Visible);

        m.tick(1101); // Delay expired
        assert_eq!(m.state, AutoHideState::SlidingOut);
    }

    #[test]
    fn test_slide_out_animation() {
        let mut m = AutoHideManager::new(make_config());
        m.on_mouse_leave_taskbar(1000);
        m.tick(1101); // → SlidingOut

        m.tick(1126); // Midway (25ms into 50ms animation)
        assert_eq!(m.state, AutoHideState::SlidingOut);
        assert!(m.hide_progress > 0.0);
        assert!(m.hide_progress < 1.0);

        m.tick(1200); // Animation done
        assert_eq!(m.state, AutoHideState::Hidden);
        assert!(m.is_fully_hidden());
    }

    #[test]
    fn test_mouse_enter_trigger_shows_taskbar() {
        let mut m = AutoHideManager::new(make_config());
        // Force hidden
        m.state = AutoHideState::Hidden;
        m.hide_progress = 1.0;

        m.on_mouse_enter_trigger(2000);
        assert_eq!(m.state, AutoHideState::SlidingIn);
    }

    #[test]
    fn test_slide_in_animation() {
        let mut m = AutoHideManager::new(make_config());
        m.state = AutoHideState::Hidden;
        m.hide_progress = 1.0;

        m.on_mouse_enter_taskbar(2000);
        assert_eq!(m.state, AutoHideState::SlidingIn);

        m.tick(2060); // Animation done
        assert_eq!(m.state, AutoHideState::Visible);
        assert_eq!(m.hide_progress, 0.0);
    }

    #[test]
    fn test_mouse_enter_cancels_hide() {
        let mut m = AutoHideManager::new(make_config());
        m.on_mouse_leave_taskbar(1000);
        m.tick(1101); // → SlidingOut

        m.on_mouse_enter_taskbar(1120); // Mouse comes back
        assert_eq!(m.state, AutoHideState::SlidingIn);
    }

    #[test]
    fn test_lock_prevents_hide() {
        let mut m = AutoHideManager::new(make_config());
        m.lock();
        m.on_mouse_leave_taskbar(1000);
        m.tick(1200);
        assert_eq!(m.state, AutoHideState::Visible); // Still visible — locked
    }

    #[test]
    fn test_lock_unlock_counted() {
        let mut m = AutoHideManager::new(make_config());
        m.lock();
        m.lock();
        assert!(m.locked);
        m.unlock();
        assert!(m.locked); // Still locked (2 locks, 1 unlock)
        m.unlock();
        assert!(!m.locked); // Now unlocked
    }

    #[test]
    fn test_peek_shows_temporarily() {
        let mut m = AutoHideManager::new(make_config());
        m.state = AutoHideState::Hidden;
        m.hide_progress = 1.0;

        m.peek(3000);
        assert_eq!(m.state, AutoHideState::SlidingIn);

        // Let slide-in finish
        m.tick(3060);
        assert_eq!(m.state, AutoHideState::Peeking);

        // After peek duration, should start hiding again
        m.tick(3300); // 200ms peek duration
        assert_eq!(m.state, AutoHideState::SlidingOut);
    }

    #[test]
    fn test_peek_cancelled_by_mouse_enter() {
        let mut m = AutoHideManager::new(make_config());
        m.state = AutoHideState::Peeking;
        m.peek_start_ms = 3000;

        m.mouse_in_taskbar = true;
        m.tick(3050);
        assert_eq!(m.state, AutoHideState::Visible);
    }

    #[test]
    fn test_disabled_always_visible() {
        let mut config = make_config();
        config.enabled = false;
        let mut m = AutoHideManager::new(config);
        m.state = AutoHideState::Hidden;
        m.hide_progress = 1.0;

        m.tick(1000);
        assert_eq!(m.state, AutoHideState::Visible);
        assert_eq!(m.hide_progress, 0.0);
    }

    #[test]
    fn test_taskbar_rect_bottom() {
        let m = AutoHideManager::new(make_config());
        let (x, y, w, h) = m.taskbar_rect();
        assert_eq!(x, 0.0);
        assert_eq!(w, 1920.0);
        assert_eq!(h, 48.0);
        assert!((y - (1080.0 - 48.0)).abs() < 0.01);
    }

    #[test]
    fn test_edge_indicator_only_when_hidden() {
        let mut m = AutoHideManager::new(make_config());
        assert!(m.render_edge_indicator().is_empty());

        m.state = AutoHideState::Hidden;
        m.hide_progress = 1.0;
        let cmds = m.render_edge_indicator();
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_mouse_in_taskbar_prevents_hide() {
        let mut m = AutoHideManager::new(make_config());
        m.mouse_in_taskbar = true;
        m.on_mouse_leave_taskbar(1000);
        m.mouse_in_taskbar = true; // Simulate re-entry
        m.tick(1200);
        // Should not hide since mouse is in taskbar
        assert_eq!(m.state, AutoHideState::Visible);
    }

    #[test]
    fn test_peek_on_disabled_noop() {
        let mut config = make_config();
        config.enabled = false;
        let mut m = AutoHideManager::new(config);
        m.state = AutoHideState::Hidden;
        m.peek(1000);
        assert_eq!(m.state, AutoHideState::Hidden); // No change
    }
}

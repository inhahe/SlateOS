//! Touchpad and Gesture Settings
//!
//! Configuration for touchpad behavior and multi-finger gestures:
//!
//! - Tap-to-click (single, double, triple finger)
//! - Scroll direction (natural / traditional)
//! - Scroll speed and acceleration
//! - Edge scrolling vs two-finger scrolling
//! - Pinch-to-zoom
//! - Multi-finger swipe gestures (3-finger, 4-finger)
//! - Palm rejection sensitivity
//! - Disable while typing
//! - Custom gesture → action bindings

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_SURFACE2: Color = Color::from_hex(0x585B70);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_YELLOW: Color = Color::from_hex(0xF9E2AF);
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);
const MOCHA_LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ============================================================================
// Scroll settings
// ============================================================================

/// Scroll direction preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDirection {
    /// Content moves with finger (macOS default).
    Natural,
    /// Content moves opposite to finger (Windows default).
    Traditional,
}

/// Scroll method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollMethod {
    /// Two-finger scroll on the touchpad surface.
    TwoFinger,
    /// Scroll by dragging along the right/bottom edge.
    Edge,
    /// Disabled.
    Disabled,
}

/// Scroll acceleration profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccelerationProfile {
    /// Linear: speed proportional to finger movement.
    Linear,
    /// Adaptive: accelerates with faster movements.
    Adaptive,
    /// Flat: constant speed regardless of movement speed.
    Flat,
}

// ============================================================================
// Tap settings
// ============================================================================

/// What a tap gesture does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapAction {
    /// Left click.
    LeftClick,
    /// Right click (context menu).
    RightClick,
    /// Middle click (paste, open in new tab).
    MiddleClick,
    /// No action.
    Disabled,
}

/// Tap configuration.
#[derive(Clone, Debug)]
pub struct TapConfig {
    /// Enable tap-to-click at all.
    pub enabled: bool,
    /// Single-finger tap action.
    pub one_finger: TapAction,
    /// Two-finger tap action.
    pub two_finger: TapAction,
    /// Three-finger tap action.
    pub three_finger: TapAction,
    /// Tap-and-drag: hold after tap to start dragging.
    pub tap_and_drag: bool,
    /// Drag lock: lift finger without ending drag (re-tap to end).
    pub drag_lock: bool,
    /// Maximum time for a tap (milliseconds).
    pub tap_time_ms: u32,
    /// Maximum movement during tap (pixels).
    pub tap_move_threshold: u32,
}

impl TapConfig {
    pub fn default_config() -> Self {
        Self {
            enabled: true,
            one_finger: TapAction::LeftClick,
            two_finger: TapAction::RightClick,
            three_finger: TapAction::MiddleClick,
            tap_and_drag: true,
            drag_lock: false,
            tap_time_ms: 180,
            tap_move_threshold: 10,
        }
    }
}

impl Default for TapConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

// ============================================================================
// Multi-finger gestures
// ============================================================================

/// Direction of a swipe gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Action triggered by a gesture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GestureAction {
    /// No action.
    None,
    /// Switch to the virtual desktop in a given direction.
    SwitchDesktop(SwipeDirection),
    /// Show overview / exposé.
    ShowOverview,
    /// Show/hide desktop (minimize all).
    ShowDesktop,
    /// Open notification center.
    NotificationCenter,
    /// Volume up/down.
    VolumeUp,
    VolumeDown,
    /// Brightness up/down.
    BrightnessUp,
    BrightnessDown,
    /// Browser back/forward.
    BrowserBack,
    BrowserForward,
    /// Zoom in/out.
    ZoomIn,
    ZoomOut,
    /// Launch an application by name.
    LaunchApp(String),
    /// Fullscreen toggle.
    FullscreenToggle,
    /// Close window.
    CloseWindow,
    /// Minimize window.
    MinimizeWindow,
    /// Snap window left/right.
    SnapLeft,
    SnapRight,
    /// Custom key combo (modifier mask, key name).
    CustomKeybind(String),
}

impl GestureAction {
    /// Human-readable label.
    pub fn label(&self) -> String {
        match self {
            Self::None => "Nothing".to_string(),
            Self::SwitchDesktop(d) => format!("Switch desktop {:?}", d),
            Self::ShowOverview => "Show overview".to_string(),
            Self::ShowDesktop => "Show desktop".to_string(),
            Self::NotificationCenter => "Notification center".to_string(),
            Self::VolumeUp => "Volume up".to_string(),
            Self::VolumeDown => "Volume down".to_string(),
            Self::BrightnessUp => "Brightness up".to_string(),
            Self::BrightnessDown => "Brightness down".to_string(),
            Self::BrowserBack => "Browser back".to_string(),
            Self::BrowserForward => "Browser forward".to_string(),
            Self::ZoomIn => "Zoom in".to_string(),
            Self::ZoomOut => "Zoom out".to_string(),
            Self::LaunchApp(name) => format!("Launch: {}", name),
            Self::FullscreenToggle => "Toggle fullscreen".to_string(),
            Self::CloseWindow => "Close window".to_string(),
            Self::MinimizeWindow => "Minimize window".to_string(),
            Self::SnapLeft => "Snap window left".to_string(),
            Self::SnapRight => "Snap window right".to_string(),
            Self::CustomKeybind(k) => format!("Key: {}", k),
        }
    }
}

/// A multi-finger gesture binding.
#[derive(Clone, Debug)]
pub struct GestureBinding {
    /// Number of fingers (3 or 4).
    pub fingers: u8,
    /// Direction of the swipe.
    pub direction: SwipeDirection,
    /// Action to perform.
    pub action: GestureAction,
}

// ============================================================================
// Pinch gesture
// ============================================================================

/// Pinch gesture actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinchAction {
    /// Zoom in/out (default).
    Zoom,
    /// Volume control.
    Volume,
    /// Brightness control.
    Brightness,
    /// Disabled.
    Disabled,
}

// ============================================================================
// Palm rejection
// ============================================================================

/// Palm rejection sensitivity level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PalmRejection {
    /// No palm rejection.
    Off,
    /// Low sensitivity (accept more touches).
    Low,
    /// Medium (balanced).
    Medium,
    /// High (reject aggressively).
    High,
}

// ============================================================================
// Touchpad settings (full config)
// ============================================================================

/// Complete touchpad configuration.
#[derive(Clone, Debug)]
pub struct TouchpadConfig {
    /// Whether the touchpad is enabled at all.
    pub enabled: bool,
    /// Pointer speed multiplier (0.1 - 3.0).
    pub pointer_speed: f32,
    /// Acceleration profile.
    pub acceleration: AccelerationProfile,
    /// Scroll direction.
    pub scroll_direction: ScrollDirection,
    /// Scroll method.
    pub scroll_method: ScrollMethod,
    /// Scroll speed multiplier (0.1 - 5.0).
    pub scroll_speed: f32,
    /// Horizontal scroll enabled.
    pub horizontal_scroll: bool,
    /// Tap configuration.
    pub tap: TapConfig,
    /// Pinch gesture action.
    pub pinch_action: PinchAction,
    /// Palm rejection level.
    pub palm_rejection: PalmRejection,
    /// Disable touchpad while typing.
    pub disable_while_typing: bool,
    /// Delay after last keypress before re-enabling (ms).
    pub typing_disable_delay_ms: u32,
    /// Disable touchpad when external mouse is connected.
    pub disable_with_external_mouse: bool,
    /// Click pressure threshold (0.0 - 1.0).
    pub click_pressure: f32,
    /// Multi-finger gesture bindings.
    pub gestures: Vec<GestureBinding>,
}

impl TouchpadConfig {
    /// Create a config with sensible defaults.
    pub fn default_config() -> Self {
        Self {
            enabled: true,
            pointer_speed: 1.0,
            acceleration: AccelerationProfile::Adaptive,
            scroll_direction: ScrollDirection::Traditional,
            scroll_method: ScrollMethod::TwoFinger,
            scroll_speed: 1.0,
            horizontal_scroll: true,
            tap: TapConfig::default_config(),
            pinch_action: PinchAction::Zoom,
            palm_rejection: PalmRejection::Medium,
            disable_while_typing: true,
            typing_disable_delay_ms: 200,
            disable_with_external_mouse: false,
            click_pressure: 0.5,
            gestures: Self::default_gestures(),
        }
    }

    /// Default gesture bindings.
    fn default_gestures() -> Vec<GestureBinding> {
        vec![
            // 3-finger swipe: switch desktops and show overview/desktop
            GestureBinding {
                fingers: 3,
                direction: SwipeDirection::Up,
                action: GestureAction::ShowOverview,
            },
            GestureBinding {
                fingers: 3,
                direction: SwipeDirection::Down,
                action: GestureAction::ShowDesktop,
            },
            GestureBinding {
                fingers: 3,
                direction: SwipeDirection::Left,
                action: GestureAction::SwitchDesktop(SwipeDirection::Left),
            },
            GestureBinding {
                fingers: 3,
                direction: SwipeDirection::Right,
                action: GestureAction::SwitchDesktop(SwipeDirection::Right),
            },
            // 4-finger swipe: volume and brightness
            GestureBinding {
                fingers: 4,
                direction: SwipeDirection::Up,
                action: GestureAction::VolumeUp,
            },
            GestureBinding {
                fingers: 4,
                direction: SwipeDirection::Down,
                action: GestureAction::VolumeDown,
            },
            GestureBinding {
                fingers: 4,
                direction: SwipeDirection::Left,
                action: GestureAction::BrightnessDown,
            },
            GestureBinding {
                fingers: 4,
                direction: SwipeDirection::Right,
                action: GestureAction::BrightnessUp,
            },
        ]
    }

    /// Set pointer speed, clamped to valid range.
    pub fn set_pointer_speed(&mut self, speed: f32) {
        self.pointer_speed = speed.clamp(0.1, 3.0);
    }

    /// Set scroll speed, clamped to valid range.
    pub fn set_scroll_speed(&mut self, speed: f32) {
        self.scroll_speed = speed.clamp(0.1, 5.0);
    }

    /// Set click pressure, clamped to valid range.
    pub fn set_click_pressure(&mut self, pressure: f32) {
        self.click_pressure = pressure.clamp(0.0, 1.0);
    }

    /// Find the gesture binding for a given finger count and direction.
    pub fn find_gesture(&self, fingers: u8, direction: SwipeDirection) -> Option<&GestureBinding> {
        self.gestures.iter().find(|g| g.fingers == fingers && g.direction == direction)
    }

    /// Set a gesture binding (replaces existing, or adds new).
    pub fn set_gesture(&mut self, fingers: u8, direction: SwipeDirection, action: GestureAction) {
        if let Some(g) = self.gestures.iter_mut().find(|g| g.fingers == fingers && g.direction == direction) {
            g.action = action;
        } else {
            self.gestures.push(GestureBinding { fingers, direction, action });
        }
    }

    /// Remove a gesture binding.
    pub fn remove_gesture(&mut self, fingers: u8, direction: SwipeDirection) -> bool {
        let before = self.gestures.len();
        self.gestures.retain(|g| !(g.fingers == fingers && g.direction == direction));
        self.gestures.len() < before
    }

    /// Export config to text format.
    pub fn export(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("enabled|{}\n", self.enabled));
        out.push_str(&format!("pointer_speed|{}\n", self.pointer_speed));
        out.push_str(&format!("acceleration|{}\n", match self.acceleration {
            AccelerationProfile::Linear => "linear",
            AccelerationProfile::Adaptive => "adaptive",
            AccelerationProfile::Flat => "flat",
        }));
        out.push_str(&format!("scroll_direction|{}\n", match self.scroll_direction {
            ScrollDirection::Natural => "natural",
            ScrollDirection::Traditional => "traditional",
        }));
        out.push_str(&format!("scroll_method|{}\n", match self.scroll_method {
            ScrollMethod::TwoFinger => "two_finger",
            ScrollMethod::Edge => "edge",
            ScrollMethod::Disabled => "disabled",
        }));
        out.push_str(&format!("scroll_speed|{}\n", self.scroll_speed));
        out.push_str(&format!("horizontal_scroll|{}\n", self.horizontal_scroll));
        out.push_str(&format!("tap_enabled|{}\n", self.tap.enabled));
        out.push_str(&format!("tap_and_drag|{}\n", self.tap.tap_and_drag));
        out.push_str(&format!("drag_lock|{}\n", self.tap.drag_lock));
        out.push_str(&format!("palm_rejection|{}\n", match self.palm_rejection {
            PalmRejection::Off => "off",
            PalmRejection::Low => "low",
            PalmRejection::Medium => "medium",
            PalmRejection::High => "high",
        }));
        out.push_str(&format!("disable_while_typing|{}\n", self.disable_while_typing));
        out.push_str(&format!("typing_disable_delay_ms|{}\n", self.typing_disable_delay_ms));
        out.push_str(&format!("disable_with_external_mouse|{}\n", self.disable_with_external_mouse));
        out.push_str(&format!("click_pressure|{}\n", self.click_pressure));
        for g in &self.gestures {
            let dir = match g.direction {
                SwipeDirection::Up => "up",
                SwipeDirection::Down => "down",
                SwipeDirection::Left => "left",
                SwipeDirection::Right => "right",
            };
            out.push_str(&format!("gesture|{}|{}|{}\n", g.fingers, dir, g.action.label()));
        }
        out
    }
}

impl Default for TouchpadConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

// ============================================================================
// Touchpad manager
// ============================================================================

/// Detected touchpad device information.
#[derive(Clone, Debug)]
pub struct TouchpadDevice {
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub max_fingers: u8,
    pub has_pressure: bool,
    pub has_palm_detection: bool,
    pub resolution_x: u32,
    pub resolution_y: u32,
}

impl TouchpadDevice {
    /// Create a generic touchpad device.
    pub fn generic() -> Self {
        Self {
            name: "Generic Touchpad".to_string(),
            vendor_id: 0,
            product_id: 0,
            max_fingers: 5,
            has_pressure: true,
            has_palm_detection: true,
            resolution_x: 1024,
            resolution_y: 768,
        }
    }
}

/// Manages touchpad detection and configuration.
pub struct TouchpadManager {
    pub config: TouchpadConfig,
    pub devices: Vec<TouchpadDevice>,
    /// Currently selected device index.
    pub active_device: usize,
    /// Whether the touchpad is temporarily disabled (typing, external mouse).
    pub temporarily_disabled: bool,
    /// Last keypress timestamp for typing detection.
    pub last_keypress_ts: u64,
}

impl TouchpadManager {
    pub fn new() -> Self {
        Self {
            config: TouchpadConfig::default_config(),
            devices: vec![TouchpadDevice::generic()],
            active_device: 0,
            temporarily_disabled: false,
            last_keypress_ts: 0,
        }
    }

    /// Register a keypress for typing-disable detection.
    pub fn on_keypress(&mut self, timestamp: u64) {
        self.last_keypress_ts = timestamp;
        if self.config.disable_while_typing && self.config.enabled {
            self.temporarily_disabled = true;
        }
    }

    /// Check if the touchpad should be re-enabled after typing delay.
    pub fn check_typing_timeout(&mut self, current_ts: u64) {
        if self.temporarily_disabled && self.config.disable_while_typing {
            let elapsed = current_ts.saturating_sub(self.last_keypress_ts);
            if elapsed >= self.config.typing_disable_delay_ms as u64 {
                self.temporarily_disabled = false;
            }
        }
    }

    /// Whether the touchpad is currently accepting input.
    pub fn is_active(&self) -> bool {
        self.config.enabled && !self.temporarily_disabled
    }

    /// Process a tap event and return the action.
    pub fn process_tap(&self, finger_count: u8) -> TapAction {
        if !self.is_active() || !self.config.tap.enabled {
            return TapAction::Disabled;
        }
        match finger_count {
            1 => self.config.tap.one_finger,
            2 => self.config.tap.two_finger,
            3 => self.config.tap.three_finger,
            _ => TapAction::Disabled,
        }
    }

    /// Process a swipe gesture and return the action.
    pub fn process_swipe(&self, fingers: u8, direction: SwipeDirection) -> GestureAction {
        if !self.is_active() {
            return GestureAction::None;
        }
        self.config.find_gesture(fingers, direction)
            .map(|g| g.action.clone())
            .unwrap_or(GestureAction::None)
    }

    /// Add a detected touchpad device.
    pub fn add_device(&mut self, device: TouchpadDevice) {
        self.devices.push(device);
    }

    /// Remove a device by index.
    pub fn remove_device(&mut self, idx: usize) -> bool {
        if idx < self.devices.len() && self.devices.len() > 1 {
            self.devices.remove(idx);
            if self.active_device >= self.devices.len() {
                self.active_device = self.devices.len().saturating_sub(1);
            }
            true
        } else {
            false
        }
    }

    /// Get the active device.
    pub fn active_device(&self) -> Option<&TouchpadDevice> {
        self.devices.get(self.active_device)
    }

    /// Reset config to defaults.
    pub fn reset_defaults(&mut self) {
        self.config = TouchpadConfig::default_config();
    }
}

impl Default for TouchpadManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Settings UI
// ============================================================================

/// Touchpad settings panel sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchpadSettingsSection {
    General,
    Scroll,
    Taps,
    Gestures,
    Advanced,
}

/// State for the settings panel.
pub struct TouchpadSettingsUI {
    pub section: TouchpadSettingsSection,
    pub selected_gesture_idx: usize,
    pub scroll_offset: usize,
}

impl TouchpadSettingsUI {
    pub fn new() -> Self {
        Self {
            section: TouchpadSettingsSection::General,
            selected_gesture_idx: 0,
            scroll_offset: 0,
        }
    }

    /// Render the touchpad settings panel.
    pub fn render(&self, mgr: &TouchpadManager, x: f32, y: f32, w: f32, h: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title bar.
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: 40.0,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: x + 16.0, y: y + 12.0,
            text: "Touchpad & Gestures".to_string(),
            font_size: 16.0, color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Device name.
        if let Some(dev) = mgr.active_device() {
            cmds.push(RenderCommand::Text {
                x: x + w - 250.0, y: y + 14.0,
                text: dev.name.clone(),
                font_size: 12.0, color: MOCHA_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Section tabs.
        let tabs = [
            ("General", TouchpadSettingsSection::General),
            ("Scroll", TouchpadSettingsSection::Scroll),
            ("Taps", TouchpadSettingsSection::Taps),
            ("Gestures", TouchpadSettingsSection::Gestures),
            ("Advanced", TouchpadSettingsSection::Advanced),
        ];
        let tab_y = y + 44.0;
        let mut tx = x + 8.0;
        for (label, section) in &tabs {
            let active = self.section == *section;
            let tw = 90.0;
            cmds.push(RenderCommand::FillRect {
                x: tx, y: tab_y, width: tw, height: 28.0,
                color: if active { MOCHA_BLUE } else { MOCHA_SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0, y: tab_y + 7.0,
                text: label.to_string(),
                font_size: 11.0,
                color: if active { MOCHA_BASE } else { MOCHA_TEXT },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: None,
            });
            tx += tw + 6.0;
        }

        let content_y = tab_y + 36.0;

        match self.section {
            TouchpadSettingsSection::General => {
                self.render_general(&mut cmds, mgr, x + 16.0, content_y, w - 32.0);
            }
            TouchpadSettingsSection::Scroll => {
                self.render_scroll(&mut cmds, mgr, x + 16.0, content_y, w - 32.0);
            }
            TouchpadSettingsSection::Taps => {
                self.render_taps(&mut cmds, mgr, x + 16.0, content_y, w - 32.0);
            }
            TouchpadSettingsSection::Gestures => {
                self.render_gestures(&mut cmds, mgr, x + 16.0, content_y, w - 32.0);
            }
            TouchpadSettingsSection::Advanced => {
                self.render_advanced(&mut cmds, mgr, x + 16.0, content_y, w - 32.0);
            }
        }

        cmds
    }

    fn render_general(&self, cmds: &mut Vec<RenderCommand>, mgr: &TouchpadManager, x: f32, y: f32, _w: f32) {
        let mut cy = y;
        self.render_toggle(cmds, x, cy, "Touchpad enabled", mgr.config.enabled);
        cy += 32.0;
        self.render_slider_label(cmds, x, cy, "Pointer speed", mgr.config.pointer_speed, 0.1, 3.0);
        cy += 32.0;

        let accel_label = match mgr.config.acceleration {
            AccelerationProfile::Linear => "Linear",
            AccelerationProfile::Adaptive => "Adaptive",
            AccelerationProfile::Flat => "Flat",
        };
        self.render_choice(cmds, x, cy, "Acceleration", accel_label);
        cy += 32.0;

        // Status indicator.
        let (status, color) = if !mgr.config.enabled {
            ("Disabled", MOCHA_RED)
        } else if mgr.temporarily_disabled {
            ("Paused (typing)", MOCHA_YELLOW)
        } else {
            ("Active", MOCHA_GREEN)
        };
        cmds.push(RenderCommand::FillRect {
            x, y: cy, width: 12.0, height: 12.0,
            color,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 18.0, y: cy,
            text: format!("Status: {}", status),
            font_size: 12.0, color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_scroll(&self, cmds: &mut Vec<RenderCommand>, mgr: &TouchpadManager, x: f32, y: f32, _w: f32) {
        let mut cy = y;
        let dir_label = match mgr.config.scroll_direction {
            ScrollDirection::Natural => "Natural (content follows finger)",
            ScrollDirection::Traditional => "Traditional (scrollbar direction)",
        };
        self.render_choice(cmds, x, cy, "Direction", dir_label);
        cy += 32.0;

        let method_label = match mgr.config.scroll_method {
            ScrollMethod::TwoFinger => "Two-finger",
            ScrollMethod::Edge => "Edge scrolling",
            ScrollMethod::Disabled => "Disabled",
        };
        self.render_choice(cmds, x, cy, "Method", method_label);
        cy += 32.0;

        self.render_slider_label(cmds, x, cy, "Scroll speed", mgr.config.scroll_speed, 0.1, 5.0);
        cy += 32.0;

        self.render_toggle(cmds, x, cy, "Horizontal scrolling", mgr.config.horizontal_scroll);
    }

    fn render_taps(&self, cmds: &mut Vec<RenderCommand>, mgr: &TouchpadManager, x: f32, y: f32, _w: f32) {
        let mut cy = y;
        self.render_toggle(cmds, x, cy, "Tap-to-click", mgr.config.tap.enabled);
        cy += 32.0;

        let tap_labels = [
            ("1-finger tap", &mgr.config.tap.one_finger),
            ("2-finger tap", &mgr.config.tap.two_finger),
            ("3-finger tap", &mgr.config.tap.three_finger),
        ];
        for (label, action) in &tap_labels {
            let action_str = match action {
                TapAction::LeftClick => "Left click",
                TapAction::RightClick => "Right click",
                TapAction::MiddleClick => "Middle click",
                TapAction::Disabled => "Disabled",
            };
            self.render_choice(cmds, x, cy, label, action_str);
            cy += 28.0;
        }
        cy += 4.0;

        self.render_toggle(cmds, x, cy, "Tap-and-drag", mgr.config.tap.tap_and_drag);
        cy += 32.0;
        self.render_toggle(cmds, x, cy, "Drag lock", mgr.config.tap.drag_lock);
    }

    fn render_gestures(&self, cmds: &mut Vec<RenderCommand>, mgr: &TouchpadManager, x: f32, y: f32, _w: f32) {
        let mut cy = y;

        cmds.push(RenderCommand::Text {
            x, y: cy,
            text: "Multi-finger gestures".to_string(),
            font_size: 13.0, color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 24.0;

        // Column headers.
        cmds.push(RenderCommand::Text {
            x, y: cy,
            text: "Fingers".to_string(),
            font_size: 10.0, color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: x + 70.0, y: cy,
            text: "Direction".to_string(),
            font_size: 10.0, color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: x + 160.0, y: cy,
            text: "Action".to_string(),
            font_size: 10.0, color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 20.0;

        cmds.push(RenderCommand::Line {
            x1: x, y1: cy, x2: x + 400.0, y2: cy,
            color: MOCHA_SURFACE1, width: 1.0,
        });
        cy += 4.0;

        for (i, g) in mgr.config.gestures.iter().enumerate() {
            let selected = i == self.selected_gesture_idx;
            if selected {
                cmds.push(RenderCommand::FillRect {
                    x: x - 4.0, y: cy - 2.0, width: 420.0, height: 22.0,
                    color: MOCHA_SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x, y: cy + 2.0,
                text: format!("{}", g.fingers),
                font_size: 12.0, color: MOCHA_LAVENDER,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            let dir_str = match g.direction {
                SwipeDirection::Up => "Up",
                SwipeDirection::Down => "Down",
                SwipeDirection::Left => "Left",
                SwipeDirection::Right => "Right",
            };
            cmds.push(RenderCommand::Text {
                x: x + 70.0, y: cy + 2.0,
                text: dir_str.to_string(),
                font_size: 12.0, color: MOCHA_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: x + 160.0, y: cy + 2.0,
                text: g.action.label(),
                font_size: 12.0, color: MOCHA_BLUE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cy += 26.0;
        }

        // Pinch action.
        cy += 8.0;
        let pinch_label = match mgr.config.pinch_action {
            PinchAction::Zoom => "Zoom",
            PinchAction::Volume => "Volume",
            PinchAction::Brightness => "Brightness",
            PinchAction::Disabled => "Disabled",
        };
        self.render_choice(cmds, x, cy, "Pinch gesture", pinch_label);
    }

    fn render_advanced(&self, cmds: &mut Vec<RenderCommand>, mgr: &TouchpadManager, x: f32, y: f32, _w: f32) {
        let mut cy = y;

        let palm_label = match mgr.config.palm_rejection {
            PalmRejection::Off => "Off",
            PalmRejection::Low => "Low",
            PalmRejection::Medium => "Medium",
            PalmRejection::High => "High",
        };
        self.render_choice(cmds, x, cy, "Palm rejection", palm_label);
        cy += 32.0;

        self.render_toggle(cmds, x, cy, "Disable while typing", mgr.config.disable_while_typing);
        cy += 32.0;

        cmds.push(RenderCommand::Text {
            x, y: cy,
            text: format!("Typing delay: {} ms", mgr.config.typing_disable_delay_ms),
            font_size: 12.0, color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cy += 32.0;

        self.render_toggle(cmds, x, cy, "Disable with external mouse", mgr.config.disable_with_external_mouse);
        cy += 32.0;

        self.render_slider_label(cmds, x, cy, "Click pressure", mgr.config.click_pressure, 0.0, 1.0);
        cy += 40.0;

        // Reset button.
        cmds.push(RenderCommand::FillRect {
            x, y: cy, width: 120.0, height: 28.0,
            color: MOCHA_RED,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: cy + 7.0,
            text: "Reset to defaults".to_string(),
            font_size: 12.0, color: MOCHA_BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    // --- Shared rendering helpers ---

    fn render_toggle(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, label: &str, value: bool) {
        cmds.push(RenderCommand::Text {
            x, y: y + 2.0,
            text: label.to_string(),
            font_size: 12.0, color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        // Toggle track.
        let track_x = x + 250.0;
        cmds.push(RenderCommand::FillRect {
            x: track_x, y: y + 1.0, width: 36.0, height: 18.0,
            color: if value { MOCHA_GREEN } else { MOCHA_SURFACE2 },
            corner_radii: CornerRadii::all(9.0),
        });
        // Toggle knob.
        let knob_x = if value { track_x + 20.0 } else { track_x + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x, y: y + 3.0, width: 14.0, height: 14.0,
            color: MOCHA_TEXT,
            corner_radii: CornerRadii::all(7.0),
        });
    }

    fn render_slider_label(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, label: &str, value: f32, min: f32, max: f32) {
        cmds.push(RenderCommand::Text {
            x, y: y + 2.0,
            text: label.to_string(),
            font_size: 12.0, color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        // Slider track.
        let track_x = x + 250.0;
        let track_w = 150.0;
        cmds.push(RenderCommand::FillRect {
            x: track_x, y: y + 8.0, width: track_w, height: 4.0,
            color: MOCHA_SURFACE1,
            corner_radii: CornerRadii::all(2.0),
        });
        // Filled portion.
        let frac = (value - min) / (max - min);
        let fill_w = track_w * frac.clamp(0.0, 1.0);
        cmds.push(RenderCommand::FillRect {
            x: track_x, y: y + 8.0, width: fill_w, height: 4.0,
            color: MOCHA_BLUE,
            corner_radii: CornerRadii::all(2.0),
        });
        // Knob.
        cmds.push(RenderCommand::FillRect {
            x: track_x + fill_w - 6.0, y: y + 4.0, width: 12.0, height: 12.0,
            color: MOCHA_BLUE,
            corner_radii: CornerRadii::all(6.0),
        });
        // Value text.
        cmds.push(RenderCommand::Text {
            x: track_x + track_w + 10.0, y: y + 2.0,
            text: format!("{:.1}", value),
            font_size: 11.0, color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_choice(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, label: &str, value: &str) {
        cmds.push(RenderCommand::Text {
            x, y: y + 2.0,
            text: label.to_string(),
            font_size: 12.0, color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::FillRect {
            x: x + 250.0, y, width: 200.0, height: 22.0,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 258.0, y: y + 4.0,
            text: value.to_string(),
            font_size: 11.0, color: MOCHA_BLUE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

impl Default for TouchpadSettingsUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- ScrollDirection ---
    #[test]
    fn test_scroll_direction_variants() {
        assert_ne!(ScrollDirection::Natural, ScrollDirection::Traditional);
    }

    // --- TapConfig ---
    #[test]
    fn test_tap_config_defaults() {
        let tc = TapConfig::default_config();
        assert!(tc.enabled);
        assert_eq!(tc.one_finger, TapAction::LeftClick);
        assert_eq!(tc.two_finger, TapAction::RightClick);
        assert_eq!(tc.three_finger, TapAction::MiddleClick);
        assert!(tc.tap_and_drag);
        assert!(!tc.drag_lock);
    }

    // --- GestureAction ---
    #[test]
    fn test_gesture_action_labels() {
        assert_eq!(GestureAction::None.label(), "Nothing");
        assert_eq!(GestureAction::ShowOverview.label(), "Show overview");
        assert_eq!(GestureAction::VolumeUp.label(), "Volume up");
        assert_eq!(GestureAction::LaunchApp("Firefox".to_string()).label(), "Launch: Firefox");
        assert_eq!(GestureAction::CustomKeybind("Ctrl+N".to_string()).label(), "Key: Ctrl+N");
    }

    #[test]
    fn test_gesture_action_all_labels() {
        // Ensure no panic on any variant.
        let actions = vec![
            GestureAction::None,
            GestureAction::SwitchDesktop(SwipeDirection::Left),
            GestureAction::ShowOverview,
            GestureAction::ShowDesktop,
            GestureAction::NotificationCenter,
            GestureAction::VolumeUp,
            GestureAction::VolumeDown,
            GestureAction::BrightnessUp,
            GestureAction::BrightnessDown,
            GestureAction::BrowserBack,
            GestureAction::BrowserForward,
            GestureAction::ZoomIn,
            GestureAction::ZoomOut,
            GestureAction::FullscreenToggle,
            GestureAction::CloseWindow,
            GestureAction::MinimizeWindow,
            GestureAction::SnapLeft,
            GestureAction::SnapRight,
        ];
        for a in actions {
            assert!(!a.label().is_empty());
        }
    }

    // --- TouchpadConfig ---
    #[test]
    fn test_config_defaults() {
        let cfg = TouchpadConfig::default_config();
        assert!(cfg.enabled);
        assert_eq!(cfg.pointer_speed, 1.0);
        assert_eq!(cfg.scroll_direction, ScrollDirection::Traditional);
        assert_eq!(cfg.scroll_method, ScrollMethod::TwoFinger);
        assert_eq!(cfg.scroll_speed, 1.0);
        assert!(cfg.horizontal_scroll);
        assert_eq!(cfg.pinch_action, PinchAction::Zoom);
        assert_eq!(cfg.palm_rejection, PalmRejection::Medium);
        assert!(cfg.disable_while_typing);
    }

    #[test]
    fn test_default_gestures_count() {
        let cfg = TouchpadConfig::default_config();
        assert_eq!(cfg.gestures.len(), 8); // 4 three-finger + 4 four-finger
    }

    #[test]
    fn test_set_pointer_speed_clamp() {
        let mut cfg = TouchpadConfig::default_config();
        cfg.set_pointer_speed(0.0);
        assert_eq!(cfg.pointer_speed, 0.1);
        cfg.set_pointer_speed(10.0);
        assert_eq!(cfg.pointer_speed, 3.0);
        cfg.set_pointer_speed(1.5);
        assert_eq!(cfg.pointer_speed, 1.5);
    }

    #[test]
    fn test_set_scroll_speed_clamp() {
        let mut cfg = TouchpadConfig::default_config();
        cfg.set_scroll_speed(-1.0);
        assert_eq!(cfg.scroll_speed, 0.1);
        cfg.set_scroll_speed(100.0);
        assert_eq!(cfg.scroll_speed, 5.0);
    }

    #[test]
    fn test_set_click_pressure_clamp() {
        let mut cfg = TouchpadConfig::default_config();
        cfg.set_click_pressure(-0.5);
        assert_eq!(cfg.click_pressure, 0.0);
        cfg.set_click_pressure(2.0);
        assert_eq!(cfg.click_pressure, 1.0);
    }

    #[test]
    fn test_find_gesture() {
        let cfg = TouchpadConfig::default_config();
        let g = cfg.find_gesture(3, SwipeDirection::Up);
        assert!(g.is_some());
        assert_eq!(g.unwrap().action, GestureAction::ShowOverview);
    }

    #[test]
    fn test_find_gesture_not_found() {
        let cfg = TouchpadConfig::default_config();
        assert!(cfg.find_gesture(5, SwipeDirection::Up).is_none());
    }

    #[test]
    fn test_set_gesture_replace() {
        let mut cfg = TouchpadConfig::default_config();
        cfg.set_gesture(3, SwipeDirection::Up, GestureAction::CloseWindow);
        let g = cfg.find_gesture(3, SwipeDirection::Up).unwrap();
        assert_eq!(g.action, GestureAction::CloseWindow);
        assert_eq!(cfg.gestures.len(), 8); // Count unchanged
    }

    #[test]
    fn test_set_gesture_new() {
        let mut cfg = TouchpadConfig::default_config();
        cfg.set_gesture(5, SwipeDirection::Up, GestureAction::FullscreenToggle);
        assert_eq!(cfg.gestures.len(), 9);
    }

    #[test]
    fn test_remove_gesture() {
        let mut cfg = TouchpadConfig::default_config();
        assert!(cfg.remove_gesture(3, SwipeDirection::Up));
        assert_eq!(cfg.gestures.len(), 7);
        assert!(cfg.find_gesture(3, SwipeDirection::Up).is_none());
    }

    #[test]
    fn test_remove_gesture_not_found() {
        let mut cfg = TouchpadConfig::default_config();
        assert!(!cfg.remove_gesture(5, SwipeDirection::Up));
    }

    #[test]
    fn test_export_contains_key_settings() {
        let cfg = TouchpadConfig::default_config();
        let exported = cfg.export();
        assert!(exported.contains("enabled|true"));
        assert!(exported.contains("pointer_speed|1"));
        assert!(exported.contains("scroll_direction|traditional"));
        assert!(exported.contains("palm_rejection|medium"));
        assert!(exported.contains("gesture|3|up|"));
    }

    // --- TouchpadDevice ---
    #[test]
    fn test_generic_device() {
        let dev = TouchpadDevice::generic();
        assert_eq!(dev.name, "Generic Touchpad");
        assert_eq!(dev.max_fingers, 5);
        assert!(dev.has_pressure);
    }

    // --- TouchpadManager ---
    #[test]
    fn test_manager_new() {
        let mgr = TouchpadManager::new();
        assert!(mgr.is_active());
        assert_eq!(mgr.devices.len(), 1);
    }

    #[test]
    fn test_manager_keypress_disables() {
        let mut mgr = TouchpadManager::new();
        mgr.on_keypress(1000);
        assert!(mgr.temporarily_disabled);
        assert!(!mgr.is_active());
    }

    #[test]
    fn test_manager_typing_timeout_reenables() {
        let mut mgr = TouchpadManager::new();
        mgr.on_keypress(1000);
        assert!(!mgr.is_active());
        mgr.check_typing_timeout(1300); // 300ms > 200ms default delay
        assert!(mgr.is_active());
    }

    #[test]
    fn test_manager_typing_timeout_too_soon() {
        let mut mgr = TouchpadManager::new();
        mgr.on_keypress(1000);
        mgr.check_typing_timeout(1100); // 100ms < 200ms
        assert!(!mgr.is_active());
    }

    #[test]
    fn test_manager_disabled_config() {
        let mut mgr = TouchpadManager::new();
        mgr.config.enabled = false;
        assert!(!mgr.is_active());
    }

    #[test]
    fn test_process_tap_one_finger() {
        let mgr = TouchpadManager::new();
        assert_eq!(mgr.process_tap(1), TapAction::LeftClick);
    }

    #[test]
    fn test_process_tap_two_finger() {
        let mgr = TouchpadManager::new();
        assert_eq!(mgr.process_tap(2), TapAction::RightClick);
    }

    #[test]
    fn test_process_tap_three_finger() {
        let mgr = TouchpadManager::new();
        assert_eq!(mgr.process_tap(3), TapAction::MiddleClick);
    }

    #[test]
    fn test_process_tap_disabled() {
        let mut mgr = TouchpadManager::new();
        mgr.config.tap.enabled = false;
        assert_eq!(mgr.process_tap(1), TapAction::Disabled);
    }

    #[test]
    fn test_process_tap_invalid_fingers() {
        let mgr = TouchpadManager::new();
        assert_eq!(mgr.process_tap(0), TapAction::Disabled);
        assert_eq!(mgr.process_tap(4), TapAction::Disabled);
    }

    #[test]
    fn test_process_swipe() {
        let mgr = TouchpadManager::new();
        let action = mgr.process_swipe(3, SwipeDirection::Up);
        assert_eq!(action, GestureAction::ShowOverview);
    }

    #[test]
    fn test_process_swipe_when_disabled() {
        let mut mgr = TouchpadManager::new();
        mgr.config.enabled = false;
        let action = mgr.process_swipe(3, SwipeDirection::Up);
        assert_eq!(action, GestureAction::None);
    }

    #[test]
    fn test_process_swipe_unbound() {
        let mgr = TouchpadManager::new();
        let action = mgr.process_swipe(5, SwipeDirection::Up);
        assert_eq!(action, GestureAction::None);
    }

    #[test]
    fn test_add_device() {
        let mut mgr = TouchpadManager::new();
        let dev = TouchpadDevice {
            name: "Synaptics".to_string(),
            vendor_id: 0x06CB,
            product_id: 0x1234,
            max_fingers: 5,
            has_pressure: true,
            has_palm_detection: true,
            resolution_x: 2048,
            resolution_y: 1536,
        };
        mgr.add_device(dev);
        assert_eq!(mgr.devices.len(), 2);
    }

    #[test]
    fn test_remove_device() {
        let mut mgr = TouchpadManager::new();
        mgr.add_device(TouchpadDevice::generic());
        assert!(mgr.remove_device(1));
        assert_eq!(mgr.devices.len(), 1);
    }

    #[test]
    fn test_remove_last_device_fails() {
        let mut mgr = TouchpadManager::new();
        assert!(!mgr.remove_device(0)); // Can't remove the last one
    }

    #[test]
    fn test_active_device() {
        let mgr = TouchpadManager::new();
        assert!(mgr.active_device().is_some());
        assert_eq!(mgr.active_device().unwrap().name, "Generic Touchpad");
    }

    #[test]
    fn test_reset_defaults() {
        let mut mgr = TouchpadManager::new();
        mgr.config.pointer_speed = 2.5;
        mgr.config.scroll_direction = ScrollDirection::Natural;
        mgr.config.palm_rejection = PalmRejection::High;
        mgr.reset_defaults();
        assert_eq!(mgr.config.pointer_speed, 1.0);
        assert_eq!(mgr.config.scroll_direction, ScrollDirection::Traditional);
        assert_eq!(mgr.config.palm_rejection, PalmRejection::Medium);
    }

    // --- UI tests ---
    #[test]
    fn test_ui_new() {
        let ui = TouchpadSettingsUI::new();
        assert_eq!(ui.section, TouchpadSettingsSection::General);
    }

    #[test]
    fn test_ui_render_general() {
        let mgr = TouchpadManager::new();
        let ui = TouchpadSettingsUI::new();
        let cmds = ui.render(&mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_scroll() {
        let mgr = TouchpadManager::new();
        let mut ui = TouchpadSettingsUI::new();
        ui.section = TouchpadSettingsSection::Scroll;
        let cmds = ui.render(&mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_taps() {
        let mgr = TouchpadManager::new();
        let mut ui = TouchpadSettingsUI::new();
        ui.section = TouchpadSettingsSection::Taps;
        let cmds = ui.render(&mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_gestures() {
        let mgr = TouchpadManager::new();
        let mut ui = TouchpadSettingsUI::new();
        ui.section = TouchpadSettingsSection::Gestures;
        let cmds = ui.render(&mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_advanced() {
        let mgr = TouchpadManager::new();
        let mut ui = TouchpadSettingsUI::new();
        ui.section = TouchpadSettingsSection::Advanced;
        let cmds = ui.render(&mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_default_trait_impls() {
        let _ = TapConfig::default();
        let _ = TouchpadConfig::default();
        let _ = TouchpadManager::default();
        let _ = TouchpadSettingsUI::default();
    }

    #[test]
    fn test_acceleration_profiles() {
        assert_ne!(AccelerationProfile::Linear, AccelerationProfile::Adaptive);
        assert_ne!(AccelerationProfile::Adaptive, AccelerationProfile::Flat);
    }

    #[test]
    fn test_scroll_methods() {
        assert_ne!(ScrollMethod::TwoFinger, ScrollMethod::Edge);
        assert_ne!(ScrollMethod::Edge, ScrollMethod::Disabled);
    }

    #[test]
    fn test_palm_rejection_levels() {
        assert_ne!(PalmRejection::Off, PalmRejection::Low);
        assert_ne!(PalmRejection::Medium, PalmRejection::High);
    }

    #[test]
    fn test_pinch_actions() {
        assert_ne!(PinchAction::Zoom, PinchAction::Volume);
        assert_ne!(PinchAction::Brightness, PinchAction::Disabled);
    }

    #[test]
    fn test_typing_disable_no_renable_when_feature_off() {
        let mut mgr = TouchpadManager::new();
        mgr.config.disable_while_typing = false;
        mgr.on_keypress(1000);
        // Should not be disabled since feature is off.
        assert!(!mgr.temporarily_disabled);
    }

    #[test]
    fn test_swipe_directions() {
        assert_ne!(SwipeDirection::Up, SwipeDirection::Down);
        assert_ne!(SwipeDirection::Left, SwipeDirection::Right);
    }
}

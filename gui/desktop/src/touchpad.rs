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

use appearance::{Palette, readable_on};
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::scroll_window;
use guitk::style::CornerRadii;

// ============================================================================
// Colour
// ============================================================================
//
// Every colour this panel draws comes from the `&Palette` threaded through
// `render`, so the panel follows the desktop's mode and accent.  Four
// judgements had to be made when the hardcoded hexes came out, because a
// literal carries no role until someone assigns one:
//
// *Four things follow the accent*, at four source sites: the selected section
// pill, the filled portion of a slider, that slider's knob, and the on side of
// a toggle.  Each is a position or an invitation — the two things the accent
// is for.  The selected section's *label* is a fifth site but not a fifth
// accent: it is `on_accent()`, which answers for the pill it lands on rather
// than following it.  This is the first converted module with a slider, so it
// sets the rule for the rest: the track is a recessed surface, while the fill
// and the knob are the accent, because between them they say where the value
// currently sits and where you would take hold of it.
//
// *Two scales are frozen*, because they report facts rather than offering
// choices, and a fact must not change colour when the desktop's accent does:
// `touchpad_status` (disabled / paused / active) and the red of the reset
// button, which means destructive and would mean nothing on a red desktop if
// it followed the accent.
//
// *Four readouts are neither.*  The gesture list's finger count was lavender
// and its action column blue, and the current value inside every choice well
// was blue as well.  None of those is a position, an invitation or a
// category — each is a value being reported, and a reported value follows
// neither the accent nor a categorical hue.  All are `text` now; the emphasis
// the finger count needs is already carried by its weight, and the three
// gesture columns are told apart by their headings and their x-positions,
// which is what a table is.
//
// *The reset button's label was wrong, not merely fragile.*  It was Mocha
// `base` — a near-black picked to read on Mocha's pale red.  On a light
// palette the fill and that label are both pale and the button says nothing.
// It is `readable_on(p.red)` now, which answers for the fill it is actually
// drawn on.
//
// The toggle knob stays `text` on the accent track.  That contrast is poor
// for pale accents and is tracked as its own issue
// (`TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL`); changing it here
// would be a second change hiding inside this one.

/// What the touchpad is doing right now, and the colour that reports it.
///
/// A fact about the device rather than a choice about the desktop, so the
/// three states are fixed hues and none of them is the accent: a touchpad
/// switched off reads red on a red desktop and red on a green one.
///
/// Named rather than left inline in `render_general` because a scale buried in
/// a `RenderCommand` cannot be checked without rendering the whole panel and
/// hunting for a formatted string.
fn touchpad_status(mgr: &TouchpadManager, p: &Palette) -> (&'static str, Color) {
    if !mgr.config.enabled {
        ("Disabled", p.red)
    } else if mgr.temporarily_disabled {
        ("Paused (typing)", p.yellow)
    } else {
        ("Active", p.green)
    }
}

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
        self.gestures
            .iter()
            .find(|g| g.fingers == fingers && g.direction == direction)
    }

    /// Set a gesture binding (replaces existing, or adds new).
    pub fn set_gesture(&mut self, fingers: u8, direction: SwipeDirection, action: GestureAction) {
        if let Some(g) = self
            .gestures
            .iter_mut()
            .find(|g| g.fingers == fingers && g.direction == direction)
        {
            g.action = action;
        } else {
            self.gestures.push(GestureBinding {
                fingers,
                direction,
                action,
            });
        }
    }

    /// Remove a gesture binding.
    pub fn remove_gesture(&mut self, fingers: u8, direction: SwipeDirection) -> bool {
        let before = self.gestures.len();
        self.gestures
            .retain(|g| !(g.fingers == fingers && g.direction == direction));
        self.gestures.len() < before
    }

    /// Export config to text format.
    pub fn export(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("enabled|{}\n", self.enabled));
        out.push_str(&format!("pointer_speed|{}\n", self.pointer_speed));
        out.push_str(&format!(
            "acceleration|{}\n",
            match self.acceleration {
                AccelerationProfile::Linear => "linear",
                AccelerationProfile::Adaptive => "adaptive",
                AccelerationProfile::Flat => "flat",
            }
        ));
        out.push_str(&format!(
            "scroll_direction|{}\n",
            match self.scroll_direction {
                ScrollDirection::Natural => "natural",
                ScrollDirection::Traditional => "traditional",
            }
        ));
        out.push_str(&format!(
            "scroll_method|{}\n",
            match self.scroll_method {
                ScrollMethod::TwoFinger => "two_finger",
                ScrollMethod::Edge => "edge",
                ScrollMethod::Disabled => "disabled",
            }
        ));
        out.push_str(&format!("scroll_speed|{}\n", self.scroll_speed));
        out.push_str(&format!("horizontal_scroll|{}\n", self.horizontal_scroll));
        out.push_str(&format!("tap_enabled|{}\n", self.tap.enabled));
        out.push_str(&format!("tap_and_drag|{}\n", self.tap.tap_and_drag));
        out.push_str(&format!("drag_lock|{}\n", self.tap.drag_lock));
        out.push_str(&format!(
            "palm_rejection|{}\n",
            match self.palm_rejection {
                PalmRejection::Off => "off",
                PalmRejection::Low => "low",
                PalmRejection::Medium => "medium",
                PalmRejection::High => "high",
            }
        ));
        out.push_str(&format!(
            "disable_while_typing|{}\n",
            self.disable_while_typing
        ));
        out.push_str(&format!(
            "typing_disable_delay_ms|{}\n",
            self.typing_disable_delay_ms
        ));
        out.push_str(&format!(
            "disable_with_external_mouse|{}\n",
            self.disable_with_external_mouse
        ));
        out.push_str(&format!("click_pressure|{}\n", self.click_pressure));
        for g in &self.gestures {
            let dir = match g.direction {
                SwipeDirection::Up => "up",
                SwipeDirection::Down => "down",
                SwipeDirection::Left => "left",
                SwipeDirection::Right => "right",
            };
            out.push_str(&format!(
                "gesture|{}|{}|{}\n",
                g.fingers,
                dir,
                g.action.label()
            ));
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
        self.config
            .find_gesture(fingers, direction)
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
    pub fn render(
        &self,
        mgr: &TouchpadManager,
        p: &Palette,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title bar.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: 40.0,
            color: p.mantle,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "Touchpad & Gestures".to_string(),
            font_size: 16.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Device name.
        if let Some(dev) = mgr.active_device() {
            cmds.push(RenderCommand::Text {
                x: x + w - 250.0,
                y: y + 14.0,
                text: dev.name.clone(),
                font_size: 12.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                x: tx,
                y: tab_y,
                width: tw,
                height: 28.0,
                color: if active { p.accent } else { p.surface0 },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: tab_y + 7.0,
                text: label.to_string(),
                font_size: 11.0,
                color: if active { p.on_accent() } else { p.text },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            tx += tw + 6.0;
        }

        let content_y = tab_y + 36.0;

        match self.section {
            TouchpadSettingsSection::General => {
                self.render_general(&mut cmds, mgr, p, x + 16.0, content_y, w - 32.0);
            }
            TouchpadSettingsSection::Scroll => {
                self.render_scroll(&mut cmds, mgr, p, x + 16.0, content_y, w - 32.0);
            }
            TouchpadSettingsSection::Taps => {
                self.render_taps(&mut cmds, mgr, p, x + 16.0, content_y, w - 32.0);
            }
            TouchpadSettingsSection::Gestures => {
                // The gesture list is the one section that grows without bound
                // (`TouchpadConfig::set_gesture` appends), so it is the one that
                // needs to know where the panel ends.
                self.render_gestures(&mut cmds, mgr, p, x + 16.0, content_y, w - 32.0, y + h);
            }
            TouchpadSettingsSection::Advanced => {
                self.render_advanced(&mut cmds, mgr, p, x + 16.0, content_y, w - 32.0);
            }
        }

        cmds
    }

    fn render_general(
        &self,
        cmds: &mut Vec<RenderCommand>,
        mgr: &TouchpadManager,
        p: &Palette,
        x: f32,
        y: f32,
        _w: f32,
    ) {
        let mut cy = y;
        self.render_toggle(cmds, p, x, cy, "Touchpad enabled", mgr.config.enabled);
        cy += 32.0;
        self.render_slider_label(
            cmds,
            p,
            x,
            cy,
            "Pointer speed",
            mgr.config.pointer_speed,
            0.1,
            3.0,
        );
        cy += 32.0;

        let accel_label = match mgr.config.acceleration {
            AccelerationProfile::Linear => "Linear",
            AccelerationProfile::Adaptive => "Adaptive",
            AccelerationProfile::Flat => "Flat",
        };
        self.render_choice(cmds, p, x, cy, "Acceleration", accel_label);
        cy += 32.0;

        // Status indicator.
        let (status, color) = touchpad_status(mgr, p);
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width: 12.0,
            height: 12.0,
            color,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 18.0,
            y: cy,
            text: format!("Status: {}", status),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_scroll(
        &self,
        cmds: &mut Vec<RenderCommand>,
        mgr: &TouchpadManager,
        p: &Palette,
        x: f32,
        y: f32,
        _w: f32,
    ) {
        let mut cy = y;
        let dir_label = match mgr.config.scroll_direction {
            ScrollDirection::Natural => "Natural (content follows finger)",
            ScrollDirection::Traditional => "Traditional (scrollbar direction)",
        };
        self.render_choice(cmds, p, x, cy, "Direction", dir_label);
        cy += 32.0;

        let method_label = match mgr.config.scroll_method {
            ScrollMethod::TwoFinger => "Two-finger",
            ScrollMethod::Edge => "Edge scrolling",
            ScrollMethod::Disabled => "Disabled",
        };
        self.render_choice(cmds, p, x, cy, "Method", method_label);
        cy += 32.0;

        self.render_slider_label(
            cmds,
            p,
            x,
            cy,
            "Scroll speed",
            mgr.config.scroll_speed,
            0.1,
            5.0,
        );
        cy += 32.0;

        self.render_toggle(
            cmds,
            p,
            x,
            cy,
            "Horizontal scrolling",
            mgr.config.horizontal_scroll,
        );
    }

    fn render_taps(
        &self,
        cmds: &mut Vec<RenderCommand>,
        mgr: &TouchpadManager,
        p: &Palette,
        x: f32,
        y: f32,
        _w: f32,
    ) {
        let mut cy = y;
        self.render_toggle(cmds, p, x, cy, "Tap-to-click", mgr.config.tap.enabled);
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
            self.render_choice(cmds, p, x, cy, label, action_str);
            cy += 28.0;
        }
        cy += 4.0;

        self.render_toggle(cmds, p, x, cy, "Tap-and-drag", mgr.config.tap.tap_and_drag);
        cy += 32.0;
        self.render_toggle(cmds, p, x, cy, "Drag lock", mgr.config.tap.drag_lock);
    }

    /// Height of one gesture row, including the space beneath it.
    const GESTURE_ROW_H: f32 = 26.0;
    /// Height of the "showing a–b of n" line drawn under the list.
    const GESTURE_COUNTER_H: f32 = 16.0;
    /// Space the pinch control below the list needs: an 8px gap plus a 22px row.
    const GESTURE_TRAILER_H: f32 = 30.0;

    fn render_gestures(
        &self,
        cmds: &mut Vec<RenderCommand>,
        mgr: &TouchpadManager,
        p: &Palette,
        x: f32,
        y: f32,
        _w: f32,
        bottom: f32,
    ) {
        let mut cy = y;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Multi-finger gestures".to_string(),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 24.0;

        // Column headers.
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Fingers".to_string(),
            font_size: 10.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cmds.push(RenderCommand::Text {
            x: x + 70.0,
            y: cy,
            text: "Direction".to_string(),
            font_size: 10.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cmds.push(RenderCommand::Text {
            x: x + 160.0,
            y: cy,
            text: "Action".to_string(),
            font_size: 10.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 20.0;

        cmds.push(RenderCommand::Line {
            x1: x,
            y1: cy,
            x2: x + 400.0,
            y2: cy,
            color: p.surface1,
            width: 1.0,
        });
        cy += 4.0;

        // The list gets whatever vertical room is left once the counter line and
        // the pinch control below it have taken their fixed share, so both stay
        // on screen however many gestures are bound. Reserving unconditionally
        // (rather than only when the list overflows) keeps this arithmetic from
        // depending on its own result.
        let total = mgr.config.gestures.len();
        let rows = scroll_window::visible(
            total,
            Self::GESTURE_ROW_H,
            bottom - cy - Self::GESTURE_COUNTER_H - Self::GESTURE_TRAILER_H,
            self.scroll_offset,
        );
        let shown = mgr
            .config
            .gestures
            .get(rows.start..rows.end())
            .unwrap_or_default();

        for (row, g) in shown.iter().enumerate() {
            let i = rows.start.saturating_add(row);
            let selected = i == self.selected_gesture_idx;
            if selected {
                cmds.push(RenderCommand::FillRect {
                    x: x - 4.0,
                    y: cy - 2.0,
                    width: 420.0,
                    height: 22.0,
                    color: p.surface0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x,
                y: cy + 2.0,
                text: format!("{}", g.fingers),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            let dir_str = match g.direction {
                SwipeDirection::Up => "Up",
                SwipeDirection::Down => "Down",
                SwipeDirection::Left => "Left",
                SwipeDirection::Right => "Right",
            };
            cmds.push(RenderCommand::Text {
                x: x + 70.0,
                y: cy + 2.0,
                text: dir_str.to_string(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: x + 160.0,
                y: cy + 2.0,
                text: g.action.label(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cy += Self::GESTURE_ROW_H;
        }

        // Say how much of the list is on screen. Always drawn, not only when the
        // list overflows: a truncated list that looks complete is worse than a
        // long one, and a line that comes and goes would move the pinch control
        // under the pointer.
        let counter = if total == 0 {
            "No gestures bound".to_string()
        } else if rows.count == total {
            format!("{total} gesture{}", if total == 1 { "" } else { "s" })
        } else {
            format!(
                "Showing {}-{} of {total} - scroll for more",
                rows.start.saturating_add(1),
                rows.end()
            )
        };
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: counter,
            font_size: 10.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += Self::GESTURE_COUNTER_H;

        // Pinch action.
        cy += 8.0;
        let pinch_label = match mgr.config.pinch_action {
            PinchAction::Zoom => "Zoom",
            PinchAction::Volume => "Volume",
            PinchAction::Brightness => "Brightness",
            PinchAction::Disabled => "Disabled",
        };
        self.render_choice(cmds, p, x, cy, "Pinch gesture", pinch_label);
    }

    fn render_advanced(
        &self,
        cmds: &mut Vec<RenderCommand>,
        mgr: &TouchpadManager,
        p: &Palette,
        x: f32,
        y: f32,
        _w: f32,
    ) {
        let mut cy = y;

        let palm_label = match mgr.config.palm_rejection {
            PalmRejection::Off => "Off",
            PalmRejection::Low => "Low",
            PalmRejection::Medium => "Medium",
            PalmRejection::High => "High",
        };
        self.render_choice(cmds, p, x, cy, "Palm rejection", palm_label);
        cy += 32.0;

        self.render_toggle(
            cmds,
            p,
            x,
            cy,
            "Disable while typing",
            mgr.config.disable_while_typing,
        );
        cy += 32.0;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!("Typing delay: {} ms", mgr.config.typing_disable_delay_ms),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 32.0;

        self.render_toggle(
            cmds,
            p,
            x,
            cy,
            "Disable with external mouse",
            mgr.config.disable_with_external_mouse,
        );
        cy += 32.0;

        self.render_slider_label(
            cmds,
            p,
            x,
            cy,
            "Click pressure",
            mgr.config.click_pressure,
            0.0,
            1.0,
        );
        cy += 40.0;

        // Reset button.
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width: 120.0,
            height: 28.0,
            color: p.red,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 7.0,
            text: "Reset to defaults".to_string(),
            font_size: 12.0,
            color: readable_on(p.red),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // --- Shared rendering helpers ---

    fn render_toggle(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        label: &str,
        value: bool,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y: y + 2.0,
            text: label.to_string(),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        let track_x = x + 250.0;
        cmds.extend(crate::switch::switch(
            track_x,
            y + 1.0,
            36.0,
            18.0,
            value,
            if value { p.accent } else { p.surface2 },
        ));
    }

    fn render_slider_label(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        label: &str,
        value: f32,
        min: f32,
        max: f32,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y: y + 2.0,
            text: label.to_string(),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        // Slider track.
        let track_x = x + 250.0;
        let track_w = 150.0;
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: y + 8.0,
            width: track_w,
            height: 4.0,
            color: p.surface1,
            corner_radii: CornerRadii::all(2.0),
        });
        // Filled portion. A slider sitting on its floor has nothing to fill,
        // and a zero-width rectangle is a command the compositor has to carry
        // and cannot draw — on every frame, for as long as the value stays
        // there. Emit it only when it covers something.
        let frac = (value - min) / (max - min);
        let fill_w = track_w * frac.clamp(0.0, 1.0);
        if fill_w > 0.0 {
            cmds.push(RenderCommand::FillRect {
                x: track_x,
                y: y + 8.0,
                width: fill_w,
                height: 4.0,
                color: p.accent,
                corner_radii: CornerRadii::all(2.0),
            });
        }
        // Knob.
        cmds.push(RenderCommand::FillRect {
            x: track_x + fill_w - 6.0,
            y: y + 4.0,
            width: 12.0,
            height: 12.0,
            color: p.accent,
            corner_radii: CornerRadii::all(6.0),
        });
        // Value text.
        cmds.push(RenderCommand::Text {
            x: track_x + track_w + 10.0,
            y: y + 2.0,
            text: format!("{:.1}", value),
            font_size: 11.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_choice(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        label: &str,
        value: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y: y + 2.0,
            text: label.to_string(),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cmds.push(RenderCommand::FillRect {
            x: x + 250.0,
            y,
            width: 200.0,
            height: 22.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 258.0,
            y: y + 4.0,
            text: value.to_string(),
            font_size: 11.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
    use crate::draw_check::assert_nothing_is_drawn_and_never_seen;
    use crate::palette_check::assert_drawn_from;

    // --- Gesture list bounding ---------------------------------------------
    //
    // The gesture list is the only part of this panel that grows without bound,
    // and until 2026-08-19 it was drawn with no reference to the panel height at
    // all: `render` took `h`, used it for the background rectangle, and passed
    // nothing down. A user with more gestures bound than the panel was tall got
    // rows drawn straight through the bottom edge and over whatever was beneath,
    // with no way to scroll them back — `scroll_offset` was a public field that
    // nothing read. These tests pin the three properties that fix requires.

    /// A manager with `n` gesture rows, each identifiable by its finger count.
    ///
    /// Finger counts are unique per row so that reading the fingers column tells
    /// you exactly which slice of the list was drawn. Direction is held constant
    /// for the same reason.
    fn mgr_with_gestures(n: usize) -> TouchpadManager {
        let mut mgr = TouchpadManager::new();
        mgr.config.gestures.clear();
        for i in 0..n {
            mgr.config.gestures.push(GestureBinding {
                fingers: u8::try_from(i).unwrap_or(u8::MAX),
                direction: SwipeDirection::Up,
                action: GestureAction::ShowOverview,
            });
        }
        mgr
    }

    fn gestures_ui() -> TouchpadSettingsUI {
        let mut ui = TouchpadSettingsUI::new();
        ui.section = TouchpadSettingsSection::Gestures;
        ui
    }

    /// The finger counts actually drawn, in order.
    ///
    /// Picked out by the text being a bare integer, which no other string in
    /// this panel is — matching on position would also catch the pinch control's
    /// label, which shares the fingers column's x and font size.
    fn drawn_finger_counts(cmds: &[RenderCommand]) -> Vec<u8> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => text.parse::<u8>().ok(),
                _ => None,
            })
            .collect()
    }

    /// The lowest pixel any command touches, or `None` if nothing was drawn.
    fn lowest_pixel(cmds: &[RenderCommand]) -> Option<f32> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { y, height, .. } => Some(y + height),
                // A text's box is not reported, so charge it its font size,
                // which over- rather than under-states the space it takes.
                RenderCommand::Text { y, font_size, .. } => Some(y + font_size),
                RenderCommand::Line { y1, y2, .. } => Some(y1.max(*y2)),
                _ => None,
            })
            .fold(None, |acc: Option<f32>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            })
    }

    #[test]
    fn a_gesture_list_longer_than_the_panel_is_not_drawn_past_its_bottom_edge() {
        let ui = gestures_ui();
        // 400 gestures is far more than anyone would bind; the point is that the
        // count cannot affect where the drawing stops.
        for count in [0_usize, 1, 5, 20, 400] {
            let mgr = mgr_with_gestures(count);
            for h in [200.0_f32, 240.0, 317.0, 500.0, 900.0] {
                let cmds = ui.render(&mgr, &Palette::for_mode(false), 10.0, 20.0, 700.0, h);
                let bottom = 20.0 + h;
                let low = lowest_pixel(&cmds).unwrap_or(bottom);
                assert!(
                    low <= bottom,
                    "{count} gestures in a {h}px panel drew down to {low}, \
                     past the bottom edge at {bottom}"
                );
            }
        }
    }

    #[test]
    fn the_pinch_control_stays_on_screen_however_long_the_gesture_list_is() {
        // The list is bounded by a budget that reserves room for what follows
        // it, so the trailing control cannot be pushed off by a long list.
        let ui = gestures_ui();
        for count in [1_usize, 3, 40, 400] {
            let cmds = ui.render(
                &mgr_with_gestures(count),
                &Palette::for_mode(false),
                10.0,
                20.0,
                700.0,
                400.0,
            );
            let bottom = 20.0 + 400.0;
            // Being *emitted* is not the same as being *visible* — an earlier
            // version of this test only checked the former, and so passed with
            // the height budget removed entirely. Check where it landed.
            let label = cmds.iter().find_map(|c| match c {
                RenderCommand::Text {
                    text, y, font_size, ..
                } if text == "Pinch gesture" => Some(y + font_size),
                _ => None,
            });
            let label_bottom =
                label.unwrap_or_else(|| panic!("{count} gestures: no pinch control drawn at all"));
            assert!(
                label_bottom <= bottom,
                "{count} gestures pushed the pinch control down to {label_bottom}, \
                 past the panel's bottom edge at {bottom}"
            );
        }
    }

    #[test]
    fn scrolling_the_gesture_list_by_one_moves_the_first_row_by_one() {
        // The property that fails if `scroll_offset` is read but ignored — which
        // is exactly how it was possible for the field to be dead for so long
        // without any test noticing.
        let mgr = mgr_with_gestures(60);
        let mut ui = gestures_ui();

        ui.scroll_offset = 0;
        let first_page = drawn_finger_counts(&ui.render(
            &mgr,
            &Palette::for_mode(false),
            10.0,
            20.0,
            700.0,
            400.0,
        ));
        assert!(
            !first_page.is_empty(),
            "a 400px panel should show at least one gesture row"
        );
        let page_len = first_page.len();
        assert_eq!(
            first_page[0], 0,
            "an unscrolled list starts at the first row"
        );

        for offset in 1..=8_u8 {
            ui.scroll_offset = usize::from(offset);
            let rows = drawn_finger_counts(&ui.render(
                &mgr,
                &Palette::for_mode(false),
                10.0,
                20.0,
                700.0,
                400.0,
            ));
            assert_eq!(
                rows.first().copied(),
                Some(offset),
                "scrolling to {offset} should start the list at row {offset}"
            );
            assert_eq!(
                rows.len(),
                page_len,
                "a full page stays full while scrolling"
            );
        }
    }

    #[test]
    fn a_gesture_scroll_offset_past_the_end_shows_the_last_page_not_an_empty_one() {
        // A stale offset — the list shrank since it was set — must not blank the
        // panel. Nothing clamps the public field, so `render` clamps its effect.
        let mgr = mgr_with_gestures(9);
        let mut ui = gestures_ui();

        ui.scroll_offset = 0;
        let page_len = drawn_finger_counts(&ui.render(
            &mgr,
            &Palette::for_mode(false),
            10.0,
            20.0,
            700.0,
            400.0,
        ))
        .len();
        assert!(
            page_len > 0 && page_len < 9,
            "the panel must show a partial list for this test to mean anything"
        );

        for offset in [9_usize, 10, 500, usize::MAX] {
            ui.scroll_offset = offset;
            let rows = drawn_finger_counts(&ui.render(
                &mgr,
                &Palette::for_mode(false),
                10.0,
                20.0,
                700.0,
                400.0,
            ));
            assert_eq!(
                rows.len(),
                page_len,
                "offset {offset} should still show a full page"
            );
            assert_eq!(
                rows.last().copied(),
                Some(8),
                "offset {offset} should be pinned to the end of the list"
            );
        }
    }

    #[test]
    fn the_gesture_list_says_when_it_is_hiding_rows() {
        // A truncated list that looks complete is worse than a long one: the
        // user has no reason to try scrolling.
        let ui = gestures_ui();
        let short = ui.render(
            &mgr_with_gestures(3),
            &Palette::for_mode(false),
            10.0,
            20.0,
            700.0,
            400.0,
        );
        assert!(
            short.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text == "3 gestures"
            )),
            "a list that fits should report its size plainly"
        );

        let long = ui.render(
            &mgr_with_gestures(80),
            &Palette::for_mode(false),
            10.0,
            20.0,
            700.0,
            400.0,
        );
        assert!(
            long.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text.starts_with("Showing ") && text.ends_with("of 80 - scroll for more")
            )),
            "a truncated list should say so; drew {:?}",
            long.iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_gesture_panel_with_no_room_at_all_draws_no_rows_and_does_not_panic() {
        // Degenerate sizes reach this code through a window resize, so they must
        // be survivable even though nothing useful can be shown.
        let ui = gestures_ui();
        for h in [0.0_f32, 1.0, 50.0, -20.0, f32::NAN] {
            let cmds = ui.render(
                &mgr_with_gestures(50),
                &Palette::for_mode(false),
                10.0,
                20.0,
                700.0,
                h,
            );
            assert!(
                drawn_finger_counts(&cmds).is_empty(),
                "a {h}px panel has no room for gesture rows, but drew some"
            );
        }
    }

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
        assert_eq!(
            GestureAction::LaunchApp("Firefox".to_string()).label(),
            "Launch: Firefox"
        );
        assert_eq!(
            GestureAction::CustomKeybind("Ctrl+N".to_string()).label(),
            "Key: Ctrl+N"
        );
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
        let cmds = ui.render(&mgr, &Palette::for_mode(false), 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_scroll() {
        let mgr = TouchpadManager::new();
        let mut ui = TouchpadSettingsUI::new();
        ui.section = TouchpadSettingsSection::Scroll;
        let cmds = ui.render(&mgr, &Palette::for_mode(false), 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_taps() {
        let mgr = TouchpadManager::new();
        let mut ui = TouchpadSettingsUI::new();
        ui.section = TouchpadSettingsSection::Taps;
        let cmds = ui.render(&mgr, &Palette::for_mode(false), 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_gestures() {
        let mgr = TouchpadManager::new();
        let mut ui = TouchpadSettingsUI::new();
        ui.section = TouchpadSettingsSection::Gestures;
        let cmds = ui.render(&mgr, &Palette::for_mode(false), 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_advanced() {
        let mgr = TouchpadManager::new();
        let mut ui = TouchpadSettingsUI::new();
        ui.section = TouchpadSettingsSection::Advanced;
        let cmds = ui.render(&mgr, &Palette::for_mode(false), 0.0, 0.0, 800.0, 600.0);
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

    // ========================================================================
    // The palette conversion
    // ========================================================================
    //
    // These replace the twelve `const MOCHA_*` this module used to hold, and
    // they come in two kinds. The second kind is the one that is easy to
    // forget, so it is worth saying why both exist.
    //
    // The *membership sweep* renders every state twice, once per mode, and
    // checks that nothing drawn is outside the palette. It works only because
    // every deleted constant was a Catppuccin **Mocha** value: Latte does not
    // contain it, so a survivor gives itself away in the light render and the
    // failure names the colour back. What the sweep cannot see is a colour
    // converted to the *wrong role* — a role belongs to both palettes, so
    // `p.red` written where `p.accent` was meant passes it without complaint.
    //
    // Everything below the sweep therefore pins one specific judgement: what
    // follows the accent, what must not, what a slider's two halves are, and
    // what colour a value being reported is drawn in.

    /// Panel origin and size used by every test below.
    ///
    /// The origin is deliberately not `(0, 0)`: a site that forgot to offset
    /// by `x`/`y` then shows up as a coordinate an extractor misses rather
    /// than hiding behind a colour that happens to be right.
    const PX: f32 = 10.0;
    const PY: f32 = 20.0;
    const PW: f32 = 700.0;
    const PH: f32 = 460.0;

    /// Where the section tab strip is drawn, given `PY`. See `render`.
    const TAB_Y: f32 = PY + 44.0;

    fn draw(ui: &TouchpadSettingsUI, mgr: &TouchpadManager, p: &Palette) -> Vec<RenderCommand> {
        ui.render(mgr, p, PX, PY, PW, PH)
    }

    const SECTIONS: [(TouchpadSettingsSection, &str); 5] = [
        (TouchpadSettingsSection::General, "General"),
        (TouchpadSettingsSection::Scroll, "Scroll"),
        (TouchpadSettingsSection::Taps, "Taps"),
        (TouchpadSettingsSection::Gestures, "Gestures"),
        (TouchpadSettingsSection::Advanced, "Advanced"),
    ];

    /// A panel showing `section`, with the second gesture row under the cursor.
    fn on(section: TouchpadSettingsSection) -> TouchpadSettingsUI {
        TouchpadSettingsUI {
            section,
            selected_gesture_idx: 1,
            scroll_offset: 0,
        }
    }

    /// Every toggle on, a device attached, sliders part-way along.
    ///
    /// The touchpad is enabled and not paused, so the status light is green —
    /// the third rung of the ladder that `mgr_all_off` and `mgr_paused` do not
    /// reach.
    fn mgr_all_on() -> TouchpadManager {
        let mut mgr = TouchpadManager::new();
        mgr.config.enabled = true;
        mgr.config.horizontal_scroll = true;
        mgr.config.tap.enabled = true;
        mgr.config.tap.tap_and_drag = true;
        mgr.config.tap.drag_lock = true;
        mgr.config.disable_while_typing = true;
        mgr.config.disable_with_external_mouse = true;
        mgr.config.pointer_speed = 1.5;
        mgr.config.scroll_speed = 2.0;
        mgr.config.click_pressure = 0.5;
        mgr
    }

    /// Every toggle off, no device attached, every slider at its floor.
    ///
    /// This is the other arm of all seven two-armed colour choices in the
    /// panel, and the only state that skips the device-name text entirely.
    /// The touchpad is disabled, so the status light is red.
    fn mgr_all_off() -> TouchpadManager {
        let mut mgr = TouchpadManager::new();
        mgr.devices.clear();
        mgr.config.enabled = false;
        mgr.config.horizontal_scroll = false;
        mgr.config.tap.enabled = false;
        mgr.config.tap.tap_and_drag = false;
        mgr.config.tap.drag_lock = false;
        mgr.config.disable_while_typing = false;
        mgr.config.disable_with_external_mouse = false;
        mgr.config.pointer_speed = 0.1;
        mgr.config.scroll_speed = 0.1;
        mgr.config.click_pressure = 0.0;
        mgr
    }

    /// Enabled but paused by typing: the middle rung of the status ladder,
    /// which neither of the other two managers reaches.
    fn mgr_paused() -> TouchpadManager {
        let mut mgr = mgr_all_on();
        mgr.temporarily_disabled = true;
        mgr
    }

    /// Every state that selects a colour differently, in both list shapes.
    ///
    /// The sweep only catches a leftover constant in a state it actually
    /// renders, so this enumerates the branches of `render` that *choose* a
    /// colour rather than the ones that move geometry: five sections, three
    /// status rungs, both arms of every toggle, the device-absent branch, and
    /// the gesture list's three shapes.
    fn every_state() -> Vec<(TouchpadSettingsUI, TouchpadManager, String)> {
        let mut out = Vec::new();
        for (section, name) in SECTIONS {
            for (mgr, how) in [
                (mgr_all_on(), "everything on"),
                (mgr_all_off(), "everything off, no device"),
                (mgr_paused(), "paused by typing"),
            ] {
                out.push((on(section), mgr, format!("{name} section, {how}")));
            }
        }
        // The gesture list has three shapes of its own. Only the last two draw
        // the cursor fill, and only the first draws "No gestures bound".
        let mut empty = mgr_all_on();
        empty.config.gestures.clear();
        out.push((
            on(TouchpadSettingsSection::Gestures),
            empty,
            "gesture list empty".to_string(),
        ));
        out.push((
            TouchpadSettingsUI {
                section: TouchpadSettingsSection::Gestures,
                selected_gesture_idx: 34,
                scroll_offset: 30,
            },
            mgr_with_gestures(80),
            "gesture list scrolled past its first page".to_string(),
        ));
        out
    }

    // ---- Extractors --------------------------------------------------------
    //
    // Every extractor below is handed the render of **one named section**,
    // never a sweep across several, because two of this module's shapes are
    // ambiguous until the section is known — and one is ambiguous even within
    // a section, which is why these take coordinates and not just sizes:
    //
    //   * `12.0 x 12.0` is the status light on General *and* a slider knob on
    //     General, Scroll and Advanced. Within General both are drawn, so they
    //     are told apart by x: the light sits at the content margin, the knob
    //     out at the track.
    //   * `height: 4.0` is a slider's track *and* its filled portion, drawn in
    //     that order; the fill's width varies with the value and reaches the
    //     track's width at maximum, so width cannot separate them either.

    fn fills(cmds: &[RenderCommand], keep: impl Fn(f32, f32, f32, f32) -> bool) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if keep(*x, *y, *width, *height) => Some(*color),
                _ => None,
            })
            .collect()
    }

    fn texts(cmds: &[RenderCommand], keep: impl Fn(f32, f32, f32, &str) -> bool) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    font_size,
                    text,
                    color,
                    ..
                } if keep(*x, *y, *font_size, text) => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The five section pills, in tab order.
    fn section_pills(cmds: &[RenderCommand]) -> Vec<Color> {
        fills(cmds, |_, _, w, h| w == 90.0 && h == 28.0)
    }

    /// The five section labels, in tab order.
    fn section_labels(cmds: &[RenderCommand]) -> Vec<Color> {
        texts(cmds, |_, y, size, _| size == 11.0 && y == TAB_Y + 7.0)
    }

    /// The status light. General section only.
    fn status_light(cmds: &[RenderCommand]) -> Vec<Color> {
        fills(cmds, |x, _, w, h| w == 12.0 && h == 12.0 && x < PX + 100.0)
    }

    /// A slider's track and its filled portion, in that order.
    fn slider_track_and_fill(cmds: &[RenderCommand]) -> Vec<Color> {
        fills(cmds, |_, _, _, h| h == 4.0)
    }

    /// A slider's knob.
    fn slider_knobs(cmds: &[RenderCommand]) -> Vec<Color> {
        fills(cmds, |x, _, w, h| w == 12.0 && h == 12.0 && x > PX + 100.0)
    }

    /// The track of every toggle on this section, in draw order.
    fn toggle_tracks(cmds: &[RenderCommand]) -> Vec<Color> {
        fills(cmds, |_, _, w, h| w == 36.0 && h == 18.0)
    }

    /// The well behind a choice control's current value.
    fn choice_wells(cmds: &[RenderCommand]) -> Vec<Color> {
        fills(cmds, |_, _, w, h| w == 200.0 && h == 22.0)
    }

    /// The value shown inside a choice control's well.
    fn choice_values(cmds: &[RenderCommand]) -> Vec<Color> {
        texts(cmds, |x, _, size, _| size == 11.0 && x == PX + 16.0 + 258.0)
    }

    /// The reset button's fill. Advanced section only.
    fn reset_fill(cmds: &[RenderCommand]) -> Color {
        let v = fills(cmds, |_, _, w, h| w == 120.0 && h == 28.0);
        assert_eq!(v.len(), 1, "expected exactly one reset button");
        v[0]
    }

    /// The reset button's label. Advanced section only.
    fn reset_label(cmds: &[RenderCommand]) -> Color {
        let v = texts(cmds, |_, _, _, t| t == "Reset to defaults");
        assert_eq!(v.len(), 1, "expected exactly one reset label");
        v[0]
    }

    /// The three columns of the gesture table.
    ///
    /// Keyed on x rather than on font size, because `font_size: 12.0` is also
    /// the toggle, slider and choice labels on other sections and the status
    /// text on General. x alone is not enough for the *fingers* column: the
    /// pinch control below the table is a choice control, and a choice
    /// control's label is drawn at the same x with the same size. The table is
    /// drawn first, so the rows are the leading ones — how many is settled by
    /// the two columns that have no such collision.
    fn gesture_columns(cmds: &[RenderCommand]) -> Vec<Color> {
        let x0 = PX + 16.0;
        let directions = texts(cmds, |x, _, size, _| size == 12.0 && x == x0 + 70.0);
        let actions = texts(cmds, |x, _, size, _| size == 12.0 && x == x0 + 160.0);
        assert_eq!(
            directions.len(),
            actions.len(),
            "every gesture row has a direction and an action"
        );
        let mut fingers = texts(cmds, |x, _, size, _| size == 12.0 && x == x0);
        assert!(
            fingers.len() >= directions.len(),
            "the fingers column cannot be shorter than the direction column"
        );
        fingers.truncate(directions.len());
        let mut out = fingers;
        out.extend(directions);
        out.extend(actions);
        out
    }

    fn section_index(section: TouchpadSettingsSection) -> usize {
        SECTIONS
            .iter()
            .position(|(s, _)| *s == section)
            .expect("every section is in SECTIONS")
    }

    /// Accents that are not themselves a hue this module freezes.
    ///
    /// The status ladder uses red, yellow and green and the reset button uses
    /// red, so those three are excluded: an accent equal to a frozen hue would
    /// let a wrongly-accented site coincide with its frozen neighbour on
    /// exactly the accent that would have exposed it. Every remaining accent
    /// collides with nothing here.
    const SAFE_ACCENTS: [Color; 8] = [
        appearance::BLUE,
        appearance::PEACH,
        appearance::MAUVE,
        appearance::TEAL,
        appearance::PINK,
        appearance::SAPPHIRE,
        appearance::SKY,
        appearance::LAVENDER,
    ];

    // ---- The membership sweep ----------------------------------------------

    /// Nothing the panel draws is outside its palette.
    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (ui, mgr, what) in every_state() {
                // A switch knob is `readable_on` its own track — one of the two
                // extremes, not a role. The tracks are named rather than the
                // extremes, so the exemption stays tied to the fill it sits on.
                assert_drawn_from(
                    &p,
                    &draw(&ui, &mgr, &p),
                    &[p.on_accent(), readable_on(p.surface2)],
                    &format!("{what}, light={light}"),
                );
            }
        }
    }

    /// Nothing is painted and then erased before anyone could see it.
    #[test]
    fn the_panel_draws_nothing_that_is_immediately_erased() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (ui, mgr, what) in every_state() {
                assert_nothing_is_drawn_and_never_seen(
                    &draw(&ui, &mgr, &p),
                    &format!("{what}, light={light}"),
                );
            }
        }
    }

    // ---- What follows the accent -------------------------------------------

    /// The four things that follow the accent, one assertion per source site.
    ///
    /// The sites are the selected section's pill, a slider's filled portion,
    /// that slider's knob, and the on side of a toggle. Each is a position or
    /// an invitation, which is what the accent is for. The toggles are *drawn*
    /// from as many as three places on one section but *written* once, so they
    /// are one assertion with a count beside it — a loop cannot disagree with
    /// itself.
    ///
    /// Each site is pinned by equality with the accent, never by inequality
    /// with the literal it used to be. Equality over eight accents is the
    /// stronger claim, since no fixed value satisfies all eight; and the
    /// inequality cannot be written honestly anyway, because the accent set
    /// contains blue and a correctly-converted pill on a blue desktop *is* the
    /// old blue.
    #[test]
    fn every_control_that_offers_something_follows_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let ctx = format!("light={light}, accent={accent:?}");

                // 1. the selected section's pill, and only that one.
                for (section, name) in SECTIONS {
                    let cmds = draw(&on(section), &mgr_all_on(), &p);
                    let pills = section_pills(&cmds);
                    assert_eq!(pills.len(), 5, "five section pills ({ctx})");
                    for (i, pill) in pills.iter().enumerate() {
                        let want = if i == section_index(section) {
                            p.accent
                        } else {
                            p.surface0
                        };
                        assert_eq!(*pill, want, "section pill {i} while showing {name} ({ctx})");
                    }
                }

                // 2 & 3. a slider's filled portion and its knob. General draws
                // exactly one slider, so the pair is unambiguous there.
                let cmds = draw(&on(TouchpadSettingsSection::General), &mgr_all_on(), &p);
                let halves = slider_track_and_fill(&cmds);
                assert_eq!(halves.len(), 2, "one slider, two halves ({ctx})");
                assert_eq!(
                    halves[1], p.accent,
                    "the filled portion of a slider is the accent ({ctx})"
                );
                let knobs = slider_knobs(&cmds);
                assert_eq!(knobs.len(), 1, "one slider, one knob ({ctx})");
                assert_eq!(knobs[0], p.accent, "a slider's knob is the accent ({ctx})");

                // 4. the on side of a toggle, both arms.
                let on_cmds = draw(&on(TouchpadSettingsSection::Taps), &mgr_all_on(), &p);
                let on_tracks = toggle_tracks(&on_cmds);
                assert_eq!(on_tracks.len(), 3, "three toggles on Taps ({ctx})");
                for (i, t) in on_tracks.iter().enumerate() {
                    assert_eq!(*t, p.accent, "toggle {i} is on ({ctx})");
                }
                let off_cmds = draw(&on(TouchpadSettingsSection::Taps), &mgr_all_off(), &p);
                for (i, t) in toggle_tracks(&off_cmds).iter().enumerate() {
                    assert_eq!(*t, p.surface2, "toggle {i} is off ({ctx})");
                }
            }
        }
    }

    /// Everything that is *not* one of those four controls is frozen when the
    /// accent changes.
    ///
    /// Two accents, one union. Both are pale, which is what lets the selected
    /// section's label stay in the union: `readable_on` answers the same
    /// near-black for each, so a label wrongly painted with the accent itself
    /// is caught here as well as by the legibility test. Adding a dark accent
    /// to this pair means excluding the labels first.
    #[test]
    fn nothing_else_moves_when_the_accent_does() {
        for light in [false, true] {
            let mut a = Palette::for_mode(light);
            a.accent = appearance::MAUVE;
            let mut b = Palette::for_mode(light);
            b.accent = appearance::TEAL;
            for (ui, mgr, what) in every_state() {
                let (ca, cb) = (draw(&ui, &mgr, &a), draw(&ui, &mgr, &b));
                assert_eq!(
                    colors_apart_from_the_controls(&ca),
                    colors_apart_from_the_controls(&cb),
                    "{what}, light={light}: something outside the four accent \
                     controls changed colour with the accent"
                );
            }
        }
    }

    /// Every colour the panel draws that is not one of the four accent
    /// controls. See `nothing_else_moves_when_the_accent_does` for why the
    /// section labels are deliberately kept in.
    fn colors_apart_from_the_controls(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    width,
                    height,
                    color,
                    ..
                } => {
                    let is_control = (*width == 90.0 && *height == 28.0)
                        || *height == 4.0
                        || (*width == 12.0 && *height == 12.0 && *x > PX + 100.0)
                        || (*width == 36.0 && *height == 18.0);
                    if is_control { None } else { Some(*color) }
                }
                RenderCommand::Text { color, .. } => Some(*color),
                RenderCommand::Line { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The panel's own surfaces are the palette's, by name.
    ///
    /// Strictly stronger than the membership sweep, and it fails in *dark*
    /// mode where membership never could: the sweep must allow any role, so a
    /// surface reverted to the Mocha literal it came from is a colour the
    /// sweep is obliged to accept.
    #[test]
    fn the_panels_own_surfaces_come_from_the_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let ctx = format!("light={light}");

            let cmds = draw(&on(TouchpadSettingsSection::General), &mgr_all_on(), &p);
            assert_eq!(
                fills(&cmds, |_, _, w, h| w == PW && h == PH),
                vec![p.base],
                "the panel backdrop ({ctx})"
            );
            assert_eq!(
                fills(&cmds, |_, _, w, h| w == PW && h == 40.0),
                vec![p.mantle],
                "the title bar ({ctx})"
            );
            assert_eq!(
                slider_track_and_fill(&cmds)[0],
                p.surface1,
                "a slider's track is a recessed surface ({ctx})"
            );
            for (i, pill) in section_pills(&cmds).iter().enumerate().skip(1) {
                assert_eq!(*pill, p.surface0, "unselected section pill {i} ({ctx})");
            }
            for (i, well) in choice_wells(&cmds).iter().enumerate() {
                assert_eq!(*well, p.surface0, "choice well {i} ({ctx})");
            }
            for (i, track) in toggle_tracks(&draw(
                &on(TouchpadSettingsSection::Taps),
                &mgr_all_off(),
                &p,
            ))
            .iter()
            .enumerate()
            {
                assert_eq!(*track, p.surface2, "off toggle {i} ({ctx})");
            }

            // The gesture table's rule line, and the cursor behind the row you
            // are looking at.
            let g = draw(&on(TouchpadSettingsSection::Gestures), &mgr_all_on(), &p);
            let lines: Vec<Color> = g
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Line { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(lines, vec![p.surface1], "the gesture table's rule ({ctx})");
            assert_eq!(
                fills(&g, |_, _, w, h| w == 420.0 && h == 22.0),
                vec![p.surface0],
                "the cursor behind the selected gesture row ({ctx})"
            );
        }
    }

    /// The selected section's label can be read on the pill beneath it.
    ///
    /// It was a fixed near-black, which reads on Mocha's pale blue and
    /// disappears on a dark accent. `LIGHT_MAUVE` is in this list because it
    /// is dark: an accent that only ever gets pale values would let the old
    /// constant pass.
    #[test]
    fn the_selected_sections_label_is_legible_on_the_pill_beneath_it() {
        for light in [false, true] {
            for accent in [
                appearance::BLUE,
                appearance::PEACH,
                appearance::MAUVE,
                appearance::TEAL,
                appearance::LIGHT_MAUVE,
                appearance::LIGHT_SKY,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                for (section, name) in SECTIONS {
                    let cmds = draw(&on(section), &mgr_all_on(), &p);
                    let labels = section_labels(&cmds);
                    assert_eq!(labels.len(), 5, "five section labels");
                    let i = section_index(section);
                    assert_eq!(
                        labels[i],
                        readable_on(p.accent),
                        "the {name} label sits on the accent pill and must be \
                         legible on it (light={light}, accent={accent:?})"
                    );
                    for (j, l) in labels.iter().enumerate() {
                        if j != i {
                            assert_eq!(
                                *l, p.text,
                                "unselected section label {j} is body text \
                                 (light={light}, accent={accent:?})"
                            );
                        }
                    }
                }
            }
        }
    }

    // ---- What must not follow the accent -----------------------------------

    /// The status light reports a fact about the device, so it never follows
    /// the accent.
    #[test]
    fn no_touchpad_state_follows_the_accent() {
        for light in [false, true] {
            let plain = Palette::for_mode(light);
            for mgr in [mgr_all_on(), mgr_all_off(), mgr_paused()] {
                let want = status_light(&draw(&on(TouchpadSettingsSection::General), &mgr, &plain));
                assert_eq!(want.len(), 1, "one status light");
                for accent in SAFE_ACCENTS {
                    let mut p = Palette::for_mode(light);
                    p.accent = accent;
                    let got = status_light(&draw(&on(TouchpadSettingsSection::General), &mgr, &p));
                    assert_eq!(
                        got, want,
                        "the status light changed with the accent \
                         (light={light}, accent={accent:?})"
                    );
                }
            }
        }
    }

    /// The three rungs of the status ladder stay distinct under every accent.
    ///
    /// Distinctness is the whole point of a three-state light: a user who
    /// cannot tell paused from active learns nothing from it.
    #[test]
    fn every_touchpad_state_stays_distinct_under_every_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let seen: Vec<Color> = [mgr_all_on(), mgr_all_off(), mgr_paused()]
                    .iter()
                    .map(|m| status_light(&draw(&on(TouchpadSettingsSection::General), m, &p))[0])
                    .collect();
                for i in 0..seen.len() {
                    for j in (i + 1)..seen.len() {
                        assert_ne!(
                            seen[i], seen[j],
                            "status rungs {i} and {j} are the same colour \
                             (light={light}, accent={accent:?})"
                        );
                    }
                }
            }
        }
    }

    /// The status light shows the state the touchpad is actually in.
    ///
    /// Asserted against `touchpad_status` directly, which is the reason that
    /// ladder was lifted out of `render_general`: a scale buried in a
    /// `RenderCommand` can only be checked by rendering a whole panel and
    /// hunting for a formatted string.
    #[test]
    fn the_status_light_reports_the_state_it_is_in() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            assert_eq!(touchpad_status(&mgr_all_off(), &p), ("Disabled", p.red));
            assert_eq!(
                touchpad_status(&mgr_paused(), &p),
                ("Paused (typing)", p.yellow)
            );
            assert_eq!(touchpad_status(&mgr_all_on(), &p), ("Active", p.green));

            // And what the panel draws is what the ladder says.
            for mgr in [mgr_all_on(), mgr_all_off(), mgr_paused()] {
                let drawn = status_light(&draw(&on(TouchpadSettingsSection::General), &mgr, &p));
                assert_eq!(
                    drawn,
                    vec![touchpad_status(&mgr, &p).1],
                    "the light and the ladder disagree (light={light})"
                );
            }
        }
    }

    /// The reset button means destructive, and destructive is not an accent.
    ///
    /// On a desktop accented red the button would be indistinguishable from
    /// every other control if it followed the accent; on a green one it would
    /// look inviting.
    #[test]
    fn the_reset_button_does_not_follow_the_accent() {
        for light in [false, true] {
            let p0 = Palette::for_mode(light);
            let want = reset_fill(&draw(
                &on(TouchpadSettingsSection::Advanced),
                &mgr_all_on(),
                &p0,
            ));
            assert_eq!(want, p0.red, "the reset button is the palette's red");
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                assert_eq!(
                    reset_fill(&draw(
                        &on(TouchpadSettingsSection::Advanced),
                        &mgr_all_on(),
                        &p
                    )),
                    want,
                    "the reset button changed with the accent \
                     (light={light}, accent={accent:?})"
                );
            }
        }
    }

    /// The reset button's label can be read on the reset button.
    ///
    /// This is a fix, not a guard. The label was Mocha `base`, a near-black
    /// picked to read on Mocha's pale red; in light mode both the fill and
    /// that label are pale and the button said nothing. The two modes must
    /// therefore *disagree* about the label, which is the assertion below the
    /// equality — a value that is the same in both modes is the bug returning.
    #[test]
    fn the_reset_buttons_label_can_be_read_on_the_button() {
        let mut seen = Vec::new();
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let cmds = draw(&on(TouchpadSettingsSection::Advanced), &mgr_all_on(), &p);
            let label = reset_label(&cmds);
            assert_eq!(
                label,
                readable_on(reset_fill(&cmds)),
                "the reset label must be legible on the red it is drawn on \
                 (light={light})"
            );
            seen.push(label);
        }
        assert_ne!(
            seen[0], seen[1],
            "a label that is the same colour in both modes cannot be legible \
             on a red that is not"
        );
    }

    // ---- What a reported value is ------------------------------------------

    /// A value the panel is reporting is body text.
    ///
    /// The gesture table's finger count was lavender and its action column
    /// blue, and every choice control's current value was blue as well. None
    /// of those is a position, an invitation or a category — each is a value
    /// being reported back, which follows neither the accent nor a categorical
    /// hue. The columns are told apart by their headings and their
    /// x-positions, and the finger count by its weight, which is what a table
    /// is.
    #[test]
    fn a_reported_value_is_the_panels_body_text() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let ctx = format!("light={light}, accent={accent:?}");

                let g = draw(&on(TouchpadSettingsSection::Gestures), &mgr_all_on(), &p);
                let cols = gesture_columns(&g);
                assert!(
                    cols.len() >= 3,
                    "the gesture table draws at least one row ({ctx})"
                );
                assert_eq!(cols.len() % 3, 0, "three columns per row ({ctx})");
                for (i, c) in cols.iter().enumerate() {
                    assert_eq!(*c, p.text, "gesture table cell {i} ({ctx})");
                }

                for (section, name) in SECTIONS {
                    let cmds = draw(&on(section), &mgr_all_on(), &p);
                    for (i, v) in choice_values(&cmds).iter().enumerate() {
                        assert_eq!(*v, p.text, "the value in choice {i} on {name} ({ctx})");
                    }
                }
            }
        }
    }
}

//! Power management settings panel.
//!
//! Configures power plans, screen/sleep timeouts, battery thresholds,
//! lid-close behaviour, and power-button actions. Provides a battery
//! health overview with charge history and estimated remaining time.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Power plan
// ============================================================================

/// Predefined power plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerPlan {
    /// Maximum battery life.
    BatterySaver,
    /// Balanced performance and battery.
    Balanced,
    /// Maximum performance.
    HighPerformance,
    /// User-defined custom plan.
    Custom,
}

impl PowerPlan {
    pub fn label(self) -> &'static str {
        match self {
            Self::BatterySaver => "Battery Saver",
            Self::Balanced => "Balanced",
            Self::HighPerformance => "High Performance",
            Self::Custom => "Custom",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::BatterySaver => "Reduces performance and brightness to extend battery life",
            Self::Balanced => "Balances performance with energy consumption",
            Self::HighPerformance => "Maximizes performance at the cost of higher power draw",
            Self::Custom => "User-defined power settings",
        }
    }

    pub const ALL: [Self; 4] = [Self::BatterySaver, Self::Balanced, Self::HighPerformance, Self::Custom];
}

// ============================================================================
// Lid / button actions
// ============================================================================

/// Action to take when the lid is closed or power button pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    DoNothing,
    Sleep,
    Hibernate,
    Shutdown,
    LockScreen,
    TurnOffDisplay,
}

impl PowerAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::DoNothing => "Do nothing",
            Self::Sleep => "Sleep",
            Self::Hibernate => "Hibernate",
            Self::Shutdown => "Shut down",
            Self::LockScreen => "Lock screen",
            Self::TurnOffDisplay => "Turn off display",
        }
    }

    pub const ALL: [Self; 6] = [
        Self::DoNothing,
        Self::Sleep,
        Self::Hibernate,
        Self::Shutdown,
        Self::LockScreen,
        Self::TurnOffDisplay,
    ];
}

// ============================================================================
// Battery status
// ============================================================================

/// Battery charging state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChargeState {
    Discharging,
    Charging,
    Full,
    NotPresent,
}

impl ChargeState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Discharging => "Discharging",
            Self::Charging => "Charging",
            Self::Full => "Fully charged",
            Self::NotPresent => "No battery",
        }
    }
}

/// Battery health estimate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryHealth {
    Good,
    Fair,
    Poor,
    Critical,
}

impl BatteryHealth {
    pub fn label(self) -> &'static str {
        match self {
            Self::Good => "Good",
            Self::Fair => "Fair",
            Self::Poor => "Poor",
            Self::Critical => "Critical — consider replacing",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Good => GREEN,
            Self::Fair => YELLOW,
            Self::Poor => PEACH,
            Self::Critical => RED,
        }
    }
}

/// Current battery information.
#[derive(Clone, Debug)]
pub struct BatteryInfo {
    /// Charge percentage (0–100).
    pub charge_pct: u32,
    /// Charge state.
    pub state: ChargeState,
    /// Estimated time remaining in minutes, if discharging.
    pub remaining_mins: Option<u32>,
    /// Design capacity in mWh.
    pub design_capacity_mwh: u32,
    /// Current full-charge capacity in mWh.
    pub full_charge_capacity_mwh: u32,
    /// Current drain rate in mW (positive = discharging).
    pub drain_mw: u32,
    /// Cycle count.
    pub cycle_count: u32,
    /// Battery health.
    pub health: BatteryHealth,
    /// Temperature in tenths of degree Celsius (e.g. 350 = 35.0°C).
    pub temperature_dc: Option<u32>,
}

impl BatteryInfo {
    pub fn new_ac_only() -> Self {
        Self {
            charge_pct: 100,
            state: ChargeState::NotPresent,
            remaining_mins: None,
            design_capacity_mwh: 0,
            full_charge_capacity_mwh: 0,
            drain_mw: 0,
            cycle_count: 0,
            health: BatteryHealth::Good,
            temperature_dc: None,
        }
    }

    /// Health percentage: full_charge / design × 100.
    pub fn health_pct(&self) -> u32 {
        if self.design_capacity_mwh == 0 {
            return 100;
        }
        (self.full_charge_capacity_mwh.saturating_mul(100))
            .checked_div(self.design_capacity_mwh)
            .unwrap_or(100)
            .min(100)
    }

    /// Format remaining time as "Xh Ym" or "—".
    pub fn remaining_formatted(&self) -> String {
        match self.remaining_mins {
            Some(m) if m > 0 => {
                let hours = m / 60;
                let mins = m % 60;
                if hours > 0 {
                    format!("{}h {}m", hours, mins)
                } else {
                    format!("{}m", mins)
                }
            }
            _ => "—".into(),
        }
    }

    /// Temperature in human-readable format.
    pub fn temperature_formatted(&self) -> String {
        match self.temperature_dc {
            Some(t) => format!("{:.1}°C", t as f32 / 10.0),
            None => "—".into(),
        }
    }
}

// ============================================================================
// Power settings
// ============================================================================

/// Full power configuration.
#[derive(Clone, Debug)]
pub struct PowerConfig {
    /// Active power plan.
    pub plan: PowerPlan,
    /// Screen off timeout on battery, in minutes (0 = never).
    pub screen_off_battery_min: u32,
    /// Screen off timeout on AC, in minutes.
    pub screen_off_ac_min: u32,
    /// Sleep timeout on battery, in minutes (0 = never).
    pub sleep_battery_min: u32,
    /// Sleep timeout on AC, in minutes.
    pub sleep_ac_min: u32,
    /// Hibernate timeout on battery, in minutes (0 = never).
    pub hibernate_battery_min: u32,
    /// Hibernate timeout on AC, in minutes.
    pub hibernate_ac_min: u32,
    /// Lid close action on battery.
    pub lid_close_battery: PowerAction,
    /// Lid close action on AC.
    pub lid_close_ac: PowerAction,
    /// Power button action.
    pub power_button: PowerAction,
    /// Sleep button action.
    pub sleep_button: PowerAction,
    /// Low battery threshold (%).
    pub low_battery_pct: u32,
    /// Critical battery threshold (%).
    pub critical_battery_pct: u32,
    /// Action at critical battery.
    pub critical_battery_action: PowerAction,
    /// Whether battery saver auto-activates at low_battery_pct.
    pub auto_battery_saver: bool,
    /// Screen brightness on battery (0–100).
    pub brightness_battery: u32,
    /// Screen brightness on AC.
    pub brightness_ac: u32,
    /// Whether adaptive brightness is enabled.
    pub adaptive_brightness: bool,
    /// USB selective suspend enabled.
    pub usb_selective_suspend: bool,
    /// Wake-on-LAN enabled.
    pub wake_on_lan: bool,
    /// Fast startup (hybrid shutdown).
    pub fast_startup: bool,
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            plan: PowerPlan::Balanced,
            screen_off_battery_min: 5,
            screen_off_ac_min: 15,
            sleep_battery_min: 15,
            sleep_ac_min: 30,
            hibernate_battery_min: 60,
            hibernate_ac_min: 0,
            lid_close_battery: PowerAction::Sleep,
            lid_close_ac: PowerAction::DoNothing,
            power_button: PowerAction::Shutdown,
            sleep_button: PowerAction::Sleep,
            low_battery_pct: 20,
            critical_battery_pct: 5,
            critical_battery_action: PowerAction::Hibernate,
            auto_battery_saver: true,
            brightness_battery: 40,
            brightness_ac: 80,
            adaptive_brightness: true,
            usb_selective_suspend: true,
            wake_on_lan: false,
            fast_startup: true,
        }
    }
}

impl PowerConfig {
    /// Apply a power plan preset.
    pub fn apply_plan(&mut self, plan: PowerPlan) {
        self.plan = plan;
        match plan {
            PowerPlan::BatterySaver => {
                self.screen_off_battery_min = 2;
                self.screen_off_ac_min = 5;
                self.sleep_battery_min = 5;
                self.sleep_ac_min = 15;
                self.brightness_battery = 25;
                self.brightness_ac = 50;
            }
            PowerPlan::Balanced => {
                self.screen_off_battery_min = 5;
                self.screen_off_ac_min = 15;
                self.sleep_battery_min = 15;
                self.sleep_ac_min = 30;
                self.brightness_battery = 40;
                self.brightness_ac = 80;
            }
            PowerPlan::HighPerformance => {
                self.screen_off_battery_min = 15;
                self.screen_off_ac_min = 30;
                self.sleep_battery_min = 30;
                self.sleep_ac_min = 0; // never
                self.brightness_battery = 70;
                self.brightness_ac = 100;
            }
            PowerPlan::Custom => {
                // Keep current values.
            }
        }
    }

    pub fn set_brightness_battery(&mut self, pct: u32) {
        self.brightness_battery = pct.min(100);
    }

    pub fn set_brightness_ac(&mut self, pct: u32) {
        self.brightness_ac = pct.min(100);
    }

    pub fn set_low_battery_pct(&mut self, pct: u32) {
        self.low_battery_pct = pct.clamp(5, 50);
    }

    pub fn set_critical_battery_pct(&mut self, pct: u32) {
        self.critical_battery_pct = pct.clamp(1, self.low_battery_pct.saturating_sub(1).max(1));
    }
}

// ============================================================================
// Charge history entry
// ============================================================================

/// One sample in the charge history ring buffer.
#[derive(Clone, Debug)]
pub struct ChargeHistorySample {
    /// Timestamp (seconds since some epoch).
    pub timestamp_secs: u64,
    /// Charge percentage.
    pub charge_pct: u32,
    /// Whether on AC power.
    pub on_ac: bool,
}

// ============================================================================
// Power settings panel
// ============================================================================

/// UI state for the power settings panel.
pub struct PowerSettingsUI {
    config: PowerConfig,
    battery: BatteryInfo,
    /// Charge history samples (ring buffer, newest last).
    charge_history: Vec<ChargeHistorySample>,
    /// Maximum history samples.
    max_history: usize,
    /// Active tab: 0=Plans, 1=Timeouts, 2=Battery, 3=Advanced.
    active_tab: usize,
}

impl PowerSettingsUI {
    pub fn new() -> Self {
        Self {
            config: PowerConfig::default(),
            battery: BatteryInfo::new_ac_only(),
            charge_history: Vec::new(),
            max_history: 128,
            active_tab: 0,
        }
    }

    pub fn with_battery(battery: BatteryInfo) -> Self {
        Self {
            config: PowerConfig::default(),
            battery,
            charge_history: Vec::new(),
            max_history: 128,
            active_tab: 0,
        }
    }

    pub fn config(&self) -> &PowerConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut PowerConfig {
        &mut self.config
    }

    pub fn battery(&self) -> &BatteryInfo {
        &self.battery
    }

    pub fn update_battery(&mut self, info: BatteryInfo) {
        self.battery = info;
    }

    pub fn record_charge(&mut self, timestamp_secs: u64, charge_pct: u32, on_ac: bool) {
        if self.charge_history.len() >= self.max_history {
            self.charge_history.remove(0);
        }
        self.charge_history.push(ChargeHistorySample {
            timestamp_secs,
            charge_pct,
            on_ac,
        });
    }

    pub fn charge_history(&self) -> &[ChargeHistorySample] {
        &self.charge_history
    }

    pub fn active_tab(&self) -> usize {
        self.active_tab
    }

    pub fn set_active_tab(&mut self, tab: usize) {
        if tab <= 3 {
            self.active_tab = tab;
        }
    }

    const TAB_LABELS: [&'static str; 4] = [
        "Power Plans",
        "Timeouts",
        "Battery",
        "Advanced",
    ];

    // ------------------------------------------------------------------
    // Rendering
    // ------------------------------------------------------------------

    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let pad = 16.0_f32;
        let inner = width - 2.0 * pad;
        let mut cy = y;

        // Background
        cmds.push(RenderCommand::FillRect {
            x, y, width, height: 900.0,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad, y: cy,
            text: "Power & Battery Settings".into(),
            font_size: 20.0, color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
        });
        cy += 32.0;

        // Quick battery status bar
        if self.battery.state != ChargeState::NotPresent {
            cy = self.render_battery_summary(&mut cmds, x + pad, cy, inner);
            cy += 8.0;
        }

        // Tab bar
        let tab_w = inner / Self::TAB_LABELS.len() as f32;
        for (i, label) in Self::TAB_LABELS.iter().enumerate() {
            let tx = x + pad + tab_w * i as f32;
            let active = self.active_tab == i;
            cmds.push(RenderCommand::FillRect {
                x: tx, y: cy, width: tab_w - 2.0, height: 30.0,
                color: if active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 8.0, y: cy + 8.0,
                text: (*label).into(),
                font_size: 12.0,
                color: if active { BLUE } else { SUBTEXT0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tab_w - 16.0),
            });
        }
        cy += 38.0;

        match self.active_tab {
            0 => self.render_plans_tab(&mut cmds, x + pad, cy, inner),
            1 => self.render_timeouts_tab(&mut cmds, x + pad, cy, inner),
            2 => self.render_battery_tab(&mut cmds, x + pad, cy, inner),
            3 => self.render_advanced_tab(&mut cmds, x + pad, cy, inner),
            _ => {}
        };

        cmds
    }

    fn render_battery_summary(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) -> f32 {
        // Background bar
        cmds.push(RenderCommand::FillRect {
            x, y, width, height: 40.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });

        // Charge bar
        let charge_color = match self.battery.charge_pct {
            0..=10 => RED,
            11..=20 => PEACH,
            21..=50 => YELLOW,
            _ => GREEN,
        };
        let bar_w = (width - 16.0) * self.battery.charge_pct as f32 / 100.0;
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0, y: y + 28.0, width: bar_w, height: 6.0,
            color: charge_color,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0, y: y + 28.0, width: width - 16.0, height: 6.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        // Redraw fill on top (stacking order)
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0, y: y + 28.0, width: bar_w, height: 6.0,
            color: charge_color,
            corner_radii: CornerRadii::all(3.0),
        });

        // Text
        let status_text = format!(
            "{}% — {} — {}",
            self.battery.charge_pct,
            self.battery.state.label(),
            self.battery.remaining_formatted()
        );
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + 8.0,
            text: status_text,
            font_size: 13.0, color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });

        y + 44.0
    }

    fn render_plans_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        for plan in PowerPlan::ALL {
            let active = self.config.plan == plan;
            cmds.push(RenderCommand::FillRect {
                x, y, width, height: 56.0,
                color: if active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(6.0),
            });
            let indicator = if active { "● " } else { "○ " };
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: y + 8.0,
                text: format!("{}{}", indicator, plan.label()),
                font_size: 14.0,
                color: if active { BLUE } else { TEXT },
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 32.0, y: y + 30.0,
                text: plan.description().into(),
                font_size: 11.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 44.0),
            });
            y += 62.0;
        }
    }

    fn render_timeouts_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        let rows: &[(&str, u32, u32)] = &[
            ("Screen off", self.config.screen_off_battery_min, self.config.screen_off_ac_min),
            ("Sleep", self.config.sleep_battery_min, self.config.sleep_ac_min),
            ("Hibernate", self.config.hibernate_battery_min, self.config.hibernate_ac_min),
        ];

        // Header
        cmds.push(RenderCommand::Text {
            x: x + width * 0.4, y,
            text: "On Battery".into(),
            font_size: 12.0, color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width * 0.25),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.7, y,
            text: "On AC".into(),
            font_size: 12.0, color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width * 0.25),
        });
        y += 24.0;

        for (label, batt, ac) in rows {
            cmds.push(RenderCommand::FillRect {
                x, y, width, height: 28.0,
                color: MANTLE,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: y + 6.0,
                text: (*label).into(),
                font_size: 13.0, color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.35),
            });
            let batt_str = if *batt == 0 { "Never".to_string() } else { format!("{} min", batt) };
            let ac_str = if *ac == 0 { "Never".to_string() } else { format!("{} min", ac) };
            cmds.push(RenderCommand::Text {
                x: x + width * 0.4, y: y + 6.0,
                text: batt_str,
                font_size: 13.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.25),
            });
            cmds.push(RenderCommand::Text {
                x: x + width * 0.7, y: y + 6.0,
                text: ac_str,
                font_size: 13.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.25),
            });
            y += 34.0;
        }

        // Button actions
        y += 8.0;
        cmds.push(RenderCommand::Text {
            x, y,
            text: "Button & Lid Actions".into(),
            font_size: 14.0, color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        y += 24.0;

        let actions: &[(&str, &str)] = &[
            ("Lid close (battery)", self.config.lid_close_battery.label()),
            ("Lid close (AC)", self.config.lid_close_ac.label()),
            ("Power button", self.config.power_button.label()),
            ("Sleep button", self.config.sleep_button.label()),
        ];
        for (label, value) in actions {
            Self::render_kv(cmds, x, y, width, label, value);
            y += 24.0;
        }
    }

    fn render_battery_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        if self.battery.state == ChargeState::NotPresent {
            cmds.push(RenderCommand::Text {
                x, y,
                text: "No battery detected — running on AC power.".into(),
                font_size: 13.0, color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
            return;
        }

        let b = &self.battery;

        Self::render_kv(cmds, x, y, width, "Charge", &format!("{}%", b.charge_pct));
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Status", b.state.label());
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Time remaining", &b.remaining_formatted());
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Health", &format!("{} ({}%)", b.health.label(), b.health_pct()));
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Design capacity", &format!("{} mWh", b.design_capacity_mwh));
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Full charge capacity", &format!("{} mWh", b.full_charge_capacity_mwh));
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Cycle count", &format!("{}", b.cycle_count));
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Drain rate", &format!("{} mW", b.drain_mw));
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Temperature", &b.temperature_formatted());
        y += 32.0;

        // Battery thresholds
        cmds.push(RenderCommand::Text {
            x, y,
            text: "Battery Thresholds".into(),
            font_size: 14.0, color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Low battery", &format!("{}%", self.config.low_battery_pct));
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Critical battery", &format!("{}%", self.config.critical_battery_pct));
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Critical action", self.config.critical_battery_action.label());
        y += 24.0;

        let saver_label = if self.config.auto_battery_saver { "On" } else { "Off" };
        Self::render_kv(cmds, x, y, width, "Auto battery saver", saver_label);

        // Charge history mini-graph
        if !self.charge_history.is_empty() {
            let _ = y; // future: render a sparkline
        }
    }

    fn render_advanced_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        let rows: &[(&str, bool)] = &[
            ("Adaptive brightness", self.config.adaptive_brightness),
            ("USB selective suspend", self.config.usb_selective_suspend),
            ("Wake-on-LAN", self.config.wake_on_lan),
            ("Fast startup (hybrid shutdown)", self.config.fast_startup),
            ("Auto battery saver", self.config.auto_battery_saver),
        ];

        for (label, on) in rows {
            Self::render_toggle(cmds, x, y, width, label, *on);
            y += 28.0;
        }

        y += 8.0;
        Self::render_kv(cmds, x, y, width, "Brightness (battery)", &format!("{}%", self.config.brightness_battery));
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Brightness (AC)", &format!("{}%", self.config.brightness_ac));
    }

    // ------------------------------------------------------------------
    // Shared rendering helpers
    // ------------------------------------------------------------------

    fn render_kv(cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, key: &str, val: &str) {
        cmds.push(RenderCommand::Text {
            x: x + 8.0, y,
            text: key.into(),
            font_size: 13.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55, y,
            text: val.into(),
            font_size: 13.0, color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
        });
    }

    fn render_toggle(cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, label: &str, on: bool) {
        cmds.push(RenderCommand::Text {
            x: x + 8.0, y,
            text: label.into(),
            font_size: 13.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.65),
        });
        let tx = x + width - 48.0;
        let bg = if on { GREEN } else { SURFACE1 };
        cmds.push(RenderCommand::FillRect {
            x: tx, y, width: 40.0, height: 20.0,
            color: bg,
            corner_radii: CornerRadii::all(10.0),
        });
        let knob_x = if on { tx + 22.0 } else { tx + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x, y: y + 2.0, width: 16.0, height: 16.0,
            color: TEXT,
            corner_radii: CornerRadii::all(8.0),
        });
    }

    /// Hit-test for tab selection.
    pub fn hit_tab(&self, rel_x: f32, width: f32) -> Option<usize> {
        let pad = 16.0_f32;
        let inner = width - 2.0 * pad;
        let tab_w = inner / Self::TAB_LABELS.len() as f32;
        let offset = rel_x - pad;
        if offset < 0.0 || offset >= inner {
            return None;
        }
        let idx = (offset / tab_w) as usize;
        if idx < Self::TAB_LABELS.len() { Some(idx) } else { None }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_power_config() {
        let c = PowerConfig::default();
        assert_eq!(c.plan, PowerPlan::Balanced);
        assert_eq!(c.screen_off_battery_min, 5);
        assert_eq!(c.screen_off_ac_min, 15);
        assert_eq!(c.brightness_battery, 40);
    }

    #[test]
    fn apply_battery_saver_plan() {
        let mut c = PowerConfig::default();
        c.apply_plan(PowerPlan::BatterySaver);
        assert_eq!(c.plan, PowerPlan::BatterySaver);
        assert_eq!(c.screen_off_battery_min, 2);
        assert_eq!(c.brightness_battery, 25);
    }

    #[test]
    fn apply_high_performance_plan() {
        let mut c = PowerConfig::default();
        c.apply_plan(PowerPlan::HighPerformance);
        assert_eq!(c.sleep_ac_min, 0); // never
        assert_eq!(c.brightness_ac, 100);
    }

    #[test]
    fn apply_custom_plan_keeps_values() {
        let mut c = PowerConfig::default();
        c.brightness_battery = 55;
        c.apply_plan(PowerPlan::Custom);
        assert_eq!(c.brightness_battery, 55);
    }

    #[test]
    fn brightness_clamped() {
        let mut c = PowerConfig::default();
        c.set_brightness_battery(200);
        assert_eq!(c.brightness_battery, 100);
        c.set_brightness_ac(200);
        assert_eq!(c.brightness_ac, 100);
    }

    #[test]
    fn low_battery_pct_clamped() {
        let mut c = PowerConfig::default();
        c.set_low_battery_pct(1);
        assert_eq!(c.low_battery_pct, 5);
        c.set_low_battery_pct(99);
        assert_eq!(c.low_battery_pct, 50);
    }

    #[test]
    fn critical_battery_below_low() {
        let mut c = PowerConfig::default();
        c.set_low_battery_pct(20);
        c.set_critical_battery_pct(25);
        assert!(c.critical_battery_pct < c.low_battery_pct);
    }

    #[test]
    fn battery_health_pct() {
        let mut b = BatteryInfo::new_ac_only();
        b.design_capacity_mwh = 50000;
        b.full_charge_capacity_mwh = 40000;
        assert_eq!(b.health_pct(), 80);
    }

    #[test]
    fn battery_health_pct_zero_design() {
        let b = BatteryInfo::new_ac_only();
        assert_eq!(b.health_pct(), 100);
    }

    #[test]
    fn remaining_formatted_hours_and_mins() {
        let mut b = BatteryInfo::new_ac_only();
        b.remaining_mins = Some(150);
        assert_eq!(b.remaining_formatted(), "2h 30m");
    }

    #[test]
    fn remaining_formatted_minutes_only() {
        let mut b = BatteryInfo::new_ac_only();
        b.remaining_mins = Some(45);
        assert_eq!(b.remaining_formatted(), "45m");
    }

    #[test]
    fn remaining_formatted_none() {
        let b = BatteryInfo::new_ac_only();
        assert_eq!(b.remaining_formatted(), "—");
    }

    #[test]
    fn temperature_formatted() {
        let mut b = BatteryInfo::new_ac_only();
        b.temperature_dc = Some(350);
        assert_eq!(b.temperature_formatted(), "35.0°C");
    }

    #[test]
    fn temperature_formatted_none() {
        let b = BatteryInfo::new_ac_only();
        assert_eq!(b.temperature_formatted(), "—");
    }

    #[test]
    fn power_plan_labels() {
        for p in PowerPlan::ALL {
            assert!(!p.label().is_empty());
            assert!(!p.description().is_empty());
        }
    }

    #[test]
    fn power_action_labels() {
        for a in PowerAction::ALL {
            assert!(!a.label().is_empty());
        }
    }

    #[test]
    fn charge_state_labels() {
        assert!(!ChargeState::Charging.label().is_empty());
        assert!(!ChargeState::Discharging.label().is_empty());
    }

    #[test]
    fn battery_health_labels_and_colors() {
        for h in [BatteryHealth::Good, BatteryHealth::Fair, BatteryHealth::Poor, BatteryHealth::Critical] {
            assert!(!h.label().is_empty());
            let _ = h.color();
        }
    }

    #[test]
    fn ui_new() {
        let ui = PowerSettingsUI::new();
        assert_eq!(ui.active_tab(), 0);
        assert_eq!(ui.battery().state, ChargeState::NotPresent);
    }

    #[test]
    fn ui_with_battery() {
        let mut b = BatteryInfo::new_ac_only();
        b.state = ChargeState::Discharging;
        b.charge_pct = 42;
        let ui = PowerSettingsUI::with_battery(b);
        assert_eq!(ui.battery().charge_pct, 42);
    }

    #[test]
    fn ui_set_tab() {
        let mut ui = PowerSettingsUI::new();
        ui.set_active_tab(2);
        assert_eq!(ui.active_tab(), 2);
        ui.set_active_tab(99);
        assert_eq!(ui.active_tab(), 2);
    }

    #[test]
    fn ui_record_charge_history() {
        let mut ui = PowerSettingsUI::new();
        for i in 0..10 {
            ui.record_charge(i * 60, 100 - i as u32, false);
        }
        assert_eq!(ui.charge_history().len(), 10);
    }

    #[test]
    fn ui_charge_history_ring_buffer() {
        let mut ui = PowerSettingsUI::new();
        for i in 0..200 {
            ui.record_charge(i, 50, true);
        }
        assert_eq!(ui.charge_history().len(), 128);
    }

    #[test]
    fn ui_config_mut() {
        let mut ui = PowerSettingsUI::new();
        ui.config_mut().apply_plan(PowerPlan::HighPerformance);
        assert_eq!(ui.config().plan, PowerPlan::HighPerformance);
    }

    #[test]
    fn ui_update_battery() {
        let mut ui = PowerSettingsUI::new();
        let mut b = BatteryInfo::new_ac_only();
        b.charge_pct = 77;
        b.state = ChargeState::Charging;
        ui.update_battery(b);
        assert_eq!(ui.battery().charge_pct, 77);
    }

    #[test]
    fn ui_render_produces_commands() {
        let ui = PowerSettingsUI::new();
        let cmds = ui.render(0.0, 0.0, 500.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn ui_render_each_tab() {
        let mut ui = PowerSettingsUI::new();
        for i in 0..4 {
            ui.set_active_tab(i);
            let cmds = ui.render(0.0, 0.0, 500.0);
            assert!(!cmds.is_empty());
        }
    }

    #[test]
    fn ui_render_with_battery() {
        let mut b = BatteryInfo::new_ac_only();
        b.state = ChargeState::Discharging;
        b.charge_pct = 35;
        b.remaining_mins = Some(120);
        let mut ui = PowerSettingsUI::with_battery(b);
        ui.set_active_tab(2);
        let cmds = ui.render(0.0, 0.0, 500.0);
        let has_charge = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("35%")));
        assert!(has_charge);
    }

    #[test]
    fn ui_hit_tab() {
        let ui = PowerSettingsUI::new();
        assert!(ui.hit_tab(5.0, 500.0).is_none());
        let hit = ui.hit_tab(20.0, 500.0);
        assert_eq!(hit, Some(0));
    }

    #[test]
    fn ui_hit_tab_last() {
        let ui = PowerSettingsUI::new();
        let inner = 500.0 - 32.0;
        let last_start = 16.0 + 3.0 * (inner / 4.0);
        assert_eq!(ui.hit_tab(last_start + 5.0, 500.0), Some(3));
    }

    #[test]
    fn spatial_audio_labels() {
        use crate::sound_settings::SpatialAudioMode;
        for m in SpatialAudioMode::ALL {
            let _ = m.label();
        }
    }

    #[test]
    fn battery_no_battery_tab() {
        let mut ui = PowerSettingsUI::new();
        ui.set_active_tab(2);
        let cmds = ui.render(0.0, 0.0, 500.0);
        let has_no_battery = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("No battery")));
        assert!(has_no_battery);
    }

    #[test]
    fn battery_health_pct_caps_at_100() {
        let mut b = BatteryInfo::new_ac_only();
        b.design_capacity_mwh = 40000;
        b.full_charge_capacity_mwh = 50000;
        assert_eq!(b.health_pct(), 100);
    }
}

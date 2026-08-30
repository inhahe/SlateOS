//! Focus Assist / Do Not Disturb mode.
//!
//! Suppresses or prioritizes notifications based on rules. The user can
//! configure automatic activation (during presentations, games, specific
//! hours) and set per-app priority levels so critical alerts still come
//! through.
//!
//! # Colour
//!
//! Every colour here is read from the [`Palette`] the caller supplies; the
//! nine Catppuccin Mocha constants this module used to declare are gone. Three
//! judgements were needed to do that, and all three are about *what a colour
//! means* rather than which one it is.
//!
//! ## The tray pill's lettering was illegible in Light mode — in all three
//! modes at once
//!
//! [`render_tray_indicator`](FocusAssistManager::render_tray_indicator) draws
//! the mode's icon on a coloured pill, and the ink was the deleted `BASE`
//! (`#1E1E2E`, near-black). Against Mocha's pale hues that is 10:1 and
//! excellent. Against Latte's — which are *dark*, because a light theme needs
//! dark accents to be seen on a light page — it measures **3.02:1 on red,
//! 3.12:1 on yellow and 3.13:1 on blue**, all below the 4.5:1 floor and all
//! three at once, since the pill is the only thing the indicator draws.
//!
//! The ink is now [`appearance::readable_on`] of the pill's own fill, which
//! answers near-white for all three Latte hues (≈4.85:1) and near-black for
//! all three Mocha ones. Naming a near-black constant beside a pale fill
//! records the *result* of a contrast measurement without recording that a
//! measurement was taken, so every copy of the constant silently re-asserts it
//! in a theme where it is false.
//!
//! ## The mode hues are a severity code and never follow the accent
//!
//! `PriorityOnly` is blue, `AlarmsOnly` yellow and `TotalSilence` red, in the
//! tray pill. That is a scale the user *decodes* — mild, more, total — so the
//! three stay [`Palette::blue`], [`Palette::yellow`] and [`Palette::red`]. If
//! they followed the accent they would all be the same colour and the scale
//! would carry no information at all, which is a worse failure than the
//! contrast bug above: a wrong colour is hard to read, a uniform one says
//! nothing.
//!
//! The "Current: …" line is the same kind of signal — it is emphasised when
//! focus assist is engaged and quiet when it is off — so it too stays a hue
//! role (`p.blue`) rather than becoming the accent.
//!
//! ## …but the mode *picker* does follow the accent
//!
//! In [`render_settings`](FocusAssistManager::render_settings) the same
//! deleted `BLUE` also lit the selected row's icon, where it means "this is
//! the row you chose" — which is exactly what the accent is for, so that site
//! becomes [`Palette::accent`]. One deleted constant, two meanings, and the
//! stock accent being blue is what kept them looking identical: the merge is
//! invisible for precisely as long as nobody changes the accent.
//!
//! Colouring the picker's icons with each mode's own severity hue instead was
//! considered, since it would teach the tray's code on the page where the user
//! is already looking at all four modes. It is rejected because the row is a
//! *picker*: the accent is how "chosen" is spelled everywhere else in the
//! shell, and a page that spelled it differently would be the odd one out.

use appearance::{Palette, readable_on};
use guitk::color::Color;
use guitk::daywindow::DailyWindow;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// ============================================================================
// Types
// ============================================================================

/// Focus assist modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusMode {
    /// All notifications come through normally.
    Off,
    /// Only priority notifications (from priority apps/contacts).
    PriorityOnly,
    /// Only alarms. All other notifications are silenced.
    AlarmsOnly,
    /// Complete silence — no notifications at all.
    TotalSilence,
}

impl FocusMode {
    /// The four modes, in the order the settings page offers them.
    ///
    /// A constant rather than an array built inside the renderer, so a test
    /// can walk the same list the picker walks. Note the limit of that: a
    /// test that *also* takes its expected order from here cannot see this
    /// list being reordered, because its expectation is permuted along with
    /// the render. Pinning the order needs an expectation written out
    /// independently — see `every_site_draws_the_role_it_claims`.
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::PriorityOnly,
        Self::AlarmsOnly,
        Self::TotalSilence,
    ];

    pub fn label(&self) -> &str {
        match self {
            Self::Off => "Off",
            Self::PriorityOnly => "Priority Only",
            Self::AlarmsOnly => "Alarms Only",
            Self::TotalSilence => "Total Silence",
        }
    }

    pub fn icon(&self) -> &str {
        match self {
            Self::Off => "\u{1F514}",          // bell
            Self::PriorityOnly => "\u{1F515}", // bell with slash
            Self::AlarmsOnly => "\u{23F0}",    // alarm clock
            Self::TotalSilence => "\u{1F6AB}", // prohibited
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Self::Off => "All notifications are shown",
            Self::PriorityOnly => "Only priority app notifications are shown",
            Self::AlarmsOnly => "Only alarms come through",
            Self::TotalSilence => "No notifications at all",
        }
    }

    /// The hue that codes how much this mode silences — `None` for `Off`.
    ///
    /// Blue, yellow, red: mild, more, total. This is a scale the user
    /// *decodes*, so it is a function of the mode and never of the accent —
    /// four accent-coloured modes would be one colour saying four things.
    ///
    /// It lives here rather than inside the renderer because the tray is not
    /// the only place that could want it, and a code spelled out at each site
    /// is a code that drifts apart between sites.
    pub fn hue(&self, p: &Palette) -> Option<Color> {
        match self {
            Self::Off => None,
            Self::PriorityOnly => Some(p.blue),
            Self::AlarmsOnly => Some(p.yellow),
            Self::TotalSilence => Some(p.red),
        }
    }
}

/// Notification priority level for an app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotifPriority {
    /// Silenced — never shown in focus mode.
    Silent,
    /// Normal — follows focus mode rules.
    Normal,
    /// Priority — shown in PriorityOnly mode.
    Priority,
    /// Critical — always shown (alarms, security alerts).
    Critical,
}

impl NotifPriority {
    pub fn label(&self) -> &str {
        match self {
            Self::Silent => "Silent",
            Self::Normal => "Normal",
            Self::Priority => "Priority",
            Self::Critical => "Critical",
        }
    }
}

/// An automatic rule that activates focus assist.
#[derive(Clone, Debug, PartialEq)]
pub enum AutoRule {
    /// Activate during a window of the day, on the listed days.
    ///
    /// A [`DailyWindow`] rather than four public `u8`s. Unvalidated, a start
    /// hour of 25 became a minute count past the end of the day, which
    /// compared as an overnight window and then never opened -- the rule would
    /// silently stop firing. The notification daemon shipped exactly that.
    Schedule {
        window: DailyWindow,
        /// Which days (0=Sun..6=Sat).
        days: Vec<u8>,
        mode: FocusMode,
    },
    /// Activate when a fullscreen app is running.
    Fullscreen { mode: FocusMode },
    /// Activate when a presentation/screen share is active.
    Presentation { mode: FocusMode },
    /// Activate when a game is running (detected by capability or heuristic).
    Gaming { mode: FocusMode },
    /// Activate when on battery power below a threshold.
    BatteryLow {
        threshold_percent: u8,
        mode: FocusMode,
    },
}

impl AutoRule {
    pub fn label(&self) -> String {
        match self {
            Self::Schedule { window, .. } => format!(
                "Schedule {:02}:{:02}–{:02}:{:02}",
                window.start().hour(),
                window.start().minute(),
                window.end().hour(),
                window.end().minute(),
            ),
            Self::Fullscreen { .. } => "Fullscreen app".to_string(),
            Self::Presentation { .. } => "Presenting".to_string(),
            Self::Gaming { .. } => "Gaming".to_string(),
            Self::BatteryLow {
                threshold_percent, ..
            } => {
                format!("Battery below {threshold_percent}%")
            }
        }
    }

    pub fn mode(&self) -> FocusMode {
        match self {
            Self::Schedule { mode, .. }
            | Self::Fullscreen { mode }
            | Self::Presentation { mode }
            | Self::Gaming { mode }
            | Self::BatteryLow { mode, .. } => *mode,
        }
    }

    /// Check if a schedule rule is currently active.
    pub fn is_schedule_active(&self, hour: u8, minute: u8, day_of_week: u8) -> bool {
        let Self::Schedule { window, days, .. } = self else {
            return false;
        };
        // An empty day list means every day.
        if !days.is_empty() && !days.contains(&day_of_week) {
            return false;
        }
        window.contains_hm(hour, minute)
    }
}

/// Per-app notification override.
#[derive(Clone, Debug)]
pub struct AppNotifOverride {
    /// Application identifier.
    pub app_id: String,
    /// Display name.
    pub app_name: String,
    /// Priority level override.
    pub priority: NotifPriority,
    /// Whether to show banners for this app.
    pub show_banner: bool,
    /// Whether to play sound for this app.
    pub play_sound: bool,
}

impl AppNotifOverride {
    pub fn new(app_id: &str, app_name: &str) -> Self {
        Self {
            app_id: app_id.to_string(),
            app_name: app_name.to_string(),
            priority: NotifPriority::Normal,
            show_banner: true,
            play_sound: true,
        }
    }

    pub fn with_priority(mut self, priority: NotifPriority) -> Self {
        self.priority = priority;
        self
    }
}

// ============================================================================
// Focus Assist Manager
// ============================================================================

/// Manages focus assist state and rules.
pub struct FocusAssistManager {
    /// Current manual mode.
    pub manual_mode: FocusMode,
    /// Whether manual override is active (overrides auto rules).
    pub manual_override: bool,
    /// Automatic rules.
    pub auto_rules: Vec<AutoRule>,
    /// Per-app overrides.
    pub app_overrides: Vec<AppNotifOverride>,
    /// Whether auto rules are enabled.
    pub auto_rules_enabled: bool,
    /// Show summary when focus assist deactivates.
    pub show_summary: bool,
    /// Number of suppressed notifications (for summary).
    pub suppressed_count: u32,
    /// Whether currently in an auto-activated mode.
    auto_active: bool,
    /// Which auto mode is currently active.
    auto_mode: FocusMode,
    /// System state flags (set by external signals).
    pub fullscreen_active: bool,
    pub presenting: bool,
    pub gaming: bool,
    pub battery_percent: u8,
    pub on_battery: bool,
}

impl FocusAssistManager {
    pub fn new() -> Self {
        Self {
            manual_mode: FocusMode::Off,
            manual_override: false,
            auto_rules: Vec::new(),
            app_overrides: Vec::new(),
            auto_rules_enabled: true,
            show_summary: true,
            suppressed_count: 0,
            auto_active: false,
            auto_mode: FocusMode::Off,
            fullscreen_active: false,
            presenting: false,
            gaming: false,
            battery_percent: 100,
            on_battery: false,
        }
    }

    /// Set the manual focus mode.
    pub fn set_mode(&mut self, mode: FocusMode) {
        self.manual_mode = mode;
        self.manual_override = mode != FocusMode::Off;
        if mode == FocusMode::Off {
            self.suppressed_count = 0;
        }
    }

    /// Cycle through modes (for quick toggle).
    pub fn cycle_mode(&mut self) {
        self.manual_mode = match self.manual_mode {
            FocusMode::Off => FocusMode::PriorityOnly,
            FocusMode::PriorityOnly => FocusMode::AlarmsOnly,
            FocusMode::AlarmsOnly => FocusMode::TotalSilence,
            FocusMode::TotalSilence => FocusMode::Off,
        };
        self.manual_override = self.manual_mode != FocusMode::Off;
    }

    /// Get the effective mode (manual override > auto rules > off).
    pub fn effective_mode(&self) -> FocusMode {
        if self.manual_override {
            self.manual_mode
        } else if self.auto_active {
            self.auto_mode
        } else {
            FocusMode::Off
        }
    }

    /// Whether focus assist is currently active (any mode).
    pub fn is_active(&self) -> bool {
        self.effective_mode() != FocusMode::Off
    }

    /// Add an auto rule.
    pub fn add_auto_rule(&mut self, rule: AutoRule) {
        self.auto_rules.push(rule);
    }

    /// Remove an auto rule by index.
    pub fn remove_auto_rule(&mut self, index: usize) -> bool {
        if index < self.auto_rules.len() {
            self.auto_rules.remove(index);
            true
        } else {
            false
        }
    }

    /// Set an app override.
    pub fn set_app_override(&mut self, override_entry: AppNotifOverride) {
        // Replace existing or add new.
        if let Some(existing) = self
            .app_overrides
            .iter_mut()
            .find(|o| o.app_id == override_entry.app_id)
        {
            *existing = override_entry;
        } else {
            self.app_overrides.push(override_entry);
        }
    }

    /// Get the notification priority for an app.
    pub fn app_priority(&self, app_id: &str) -> NotifPriority {
        self.app_overrides
            .iter()
            .find(|o| o.app_id == app_id)
            .map(|o| o.priority)
            .unwrap_or(NotifPriority::Normal)
    }

    /// Should a notification from this app be shown right now?
    pub fn should_show_notification(&self, app_id: &str) -> bool {
        let mode = self.effective_mode();
        let priority = self.app_priority(app_id);

        match mode {
            FocusMode::Off => true,
            FocusMode::PriorityOnly => priority >= NotifPriority::Priority,
            FocusMode::AlarmsOnly => priority >= NotifPriority::Critical,
            FocusMode::TotalSilence => false,
        }
    }

    /// Record a suppressed notification.
    pub fn record_suppressed(&mut self) {
        self.suppressed_count = self.suppressed_count.saturating_add(1);
    }

    /// Evaluate auto rules given current time and system state.
    pub fn evaluate_auto_rules(&mut self, hour: u8, minute: u8, day_of_week: u8) {
        if !self.auto_rules_enabled || self.manual_override {
            self.auto_active = false;
            return;
        }

        // Find the highest-priority triggered rule.
        let mut triggered_mode: Option<FocusMode> = None;

        for rule in &self.auto_rules {
            let active = match rule {
                AutoRule::Schedule { .. } => rule.is_schedule_active(hour, minute, day_of_week),
                AutoRule::Fullscreen { .. } => self.fullscreen_active,
                AutoRule::Presentation { .. } => self.presenting,
                AutoRule::Gaming { .. } => self.gaming,
                AutoRule::BatteryLow {
                    threshold_percent, ..
                } => self.on_battery && self.battery_percent < *threshold_percent,
            };

            if active {
                let rule_mode = rule.mode();
                // Take the most restrictive mode.
                triggered_mode = Some(match triggered_mode {
                    None => rule_mode,
                    Some(current) => most_restrictive(current, rule_mode),
                });
            }
        }

        if let Some(mode) = triggered_mode {
            if !self.auto_active {
                self.suppressed_count = 0; // reset on activation
            }
            self.auto_active = true;
            self.auto_mode = mode;
        } else {
            self.auto_active = false;
            self.auto_mode = FocusMode::Off;
        }
    }

    /// Render the tray indicator for focus assist.
    pub fn render_tray_indicator(&self, p: &Palette, x: f32, y: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let mode = self.effective_mode();

        // `hue` answering `None` *is* the "Off draws nothing" rule, so the
        // early return reads it rather than re-deciding it. Two places that
        // both decide when the indicator is hidden is one place too many.
        let Some(color) = mode.hue(p) else {
            return commands;
        };

        // Background pill.
        let pill_w = 24.0;
        let pill_h = 16.0;

        commands.push(RenderCommand::FillRect {
            x,
            y,
            width: pill_w,
            height: pill_h,
            color,
            corner_radii: CornerRadii::all(pill_h / 2.0),
        });

        // Moon/bell icon, inked for the pill it sits on rather than for the
        // theme the pill was first drawn in — see this module's `# Colour`.
        commands.push(RenderCommand::Text {
            x: x + 5.0,
            y: y + 1.0,
            text: mode.icon().to_string(),
            font_size: 10.0,
            color: readable_on(color),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        commands
    }

    /// Render the focus assist settings panel.
    pub fn render_settings(&self, p: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let padding = 12.0;
        let mut cy = y + padding;

        // Title.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "Focus Assist".to_string(),
            font_size: 18.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 32.0;

        // Current mode.
        let mode = self.effective_mode();
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: format!("Current: {}", mode.label()),
            font_size: 14.0,
            // Engaged or not is *state*, so this is a hue role and not the
            // accent — the accent is reserved for "you chose this".
            color: if mode == FocusMode::Off {
                p.subtext0
            } else {
                p.blue
            },
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 24.0;

        if self.is_active() && self.suppressed_count > 0 {
            commands.push(RenderCommand::Text {
                x: x + padding,
                y: cy,
                text: format!("{} notifications suppressed", self.suppressed_count),
                font_size: 12.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cy += 20.0;
        }
        cy += 8.0;

        // Mode selector.
        for m in &FocusMode::ALL {
            let selected = self.manual_mode == *m;
            let bg = if selected { p.surface0 } else { p.mantle };
            commands.push(RenderCommand::FillRect {
                x: x + padding,
                y: cy,
                width: width - padding * 2.0,
                height: 40.0,
                color: bg,
                corner_radii: CornerRadii::all(8.0),
            });
            commands.push(RenderCommand::Text {
                x: x + padding + 12.0,
                y: cy + 4.0,
                text: m.icon().to_string(),
                font_size: 16.0,
                // Here `BLUE` meant "chosen", not "this much silence" — the
                // picker's only accent site.
                color: if selected { p.accent } else { p.subtext0 },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            commands.push(RenderCommand::Text {
                x: x + padding + 36.0,
                y: cy + 4.0,
                text: m.label().to_string(),
                font_size: 13.0,
                color: if selected { p.text } else { p.subtext0 },
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            commands.push(RenderCommand::Text {
                x: x + padding + 36.0,
                y: cy + 22.0,
                text: m.description().to_string(),
                font_size: 10.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - padding * 2.0 - 48.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 46.0;
        }
        cy += 12.0;

        // Auto rules section.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "Automatic Rules".to_string(),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 24.0;

        if self.auto_rules.is_empty() {
            commands.push(RenderCommand::Text {
                x: x + padding + 8.0,
                y: cy,
                text: "No automatic rules configured".to_string(),
                font_size: 12.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        } else {
            for rule in &self.auto_rules {
                commands.push(RenderCommand::FillRect {
                    x: x + padding,
                    y: cy,
                    width: width - padding * 2.0,
                    height: 28.0,
                    color: p.surface0,
                    corner_radii: CornerRadii::all(6.0),
                });
                commands.push(RenderCommand::Text {
                    x: x + padding + 8.0,
                    y: cy + 6.0,
                    text: rule.label(),
                    font_size: 12.0,
                    color: p.text,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                commands.push(RenderCommand::Text {
                    x: x + width - padding - 80.0,
                    y: cy + 7.0,
                    text: rule.mode().label().to_string(),
                    font_size: 10.0,
                    color: p.overlay0,
                    font_weight: FontWeightHint::Light,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                cy += 32.0;
            }
        }

        commands
    }
}

impl Default for FocusAssistManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Return the more restrictive of two focus modes.
fn most_restrictive(a: FocusMode, b: FocusMode) -> FocusMode {
    let rank = |m: FocusMode| match m {
        FocusMode::Off => 0,
        FocusMode::PriorityOnly => 1,
        FocusMode::AlarmsOnly => 2,
        FocusMode::TotalSilence => 3,
    };
    if rank(a) >= rank(b) { a } else { b }
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

    use super::*;
    use crate::palette_check::assert_drawn_from;
    use appearance::AccentColor;

    fn make_mgr() -> FocusAssistManager {
        FocusAssistManager::new()
    }

    // ---- Colour fixtures ----

    /// A palette wearing an accent that is in no palette.
    ///
    /// The whole point of a conversion test is to tell "read the accent" apart
    /// from "drew the colour the accent happens to be", and the stock accent
    /// *is* blue — which is also this module's `PriorityOnly` hue. With the
    /// stock accent the picker's chosen row and the mild-silence pill are the
    /// same pixels, so a test could not tell a severity code from a selection
    /// marker. Magenta is in neither palette and is not a mode hue.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0x00FF_00FF);
        assert!(
            !p.roles()
                .iter()
                .any(|(n, r)| *n != "accent" && *r == p.accent),
            "the fixture accent collided with a role, so 'this is the accent' \
             and 'this is that role' stopped being distinguishable"
        );
        for m in FocusMode::ALL {
            assert_ne!(
                m.hue(&p),
                Some(p.accent),
                "the fixture accent is also a mode hue, so the severity code \
                 and the selection marker are indistinguishable"
            );
        }
        p
    }

    /// Every colour a command will put on screen, in render order.
    fn colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The four mode rows as `(background, icon, label)`.
    ///
    /// Found by shape — a 40-pixel-high fill and the two `Text` runs that
    /// follow it — rather than by index, because an index into a render is a
    /// claim about layout and this module is not about layout. The rule rows
    /// later in the page are 28 high, so they cannot be mistaken for these.
    fn rows(cmds: &[RenderCommand]) -> Vec<(Color, Color, Color)> {
        let mut out = Vec::new();
        for (i, c) in cmds.iter().enumerate() {
            let RenderCommand::FillRect { height, color, .. } = c else {
                continue;
            };
            if (*height - 40.0).abs() > f32::EPSILON {
                continue;
            }
            let inks: Vec<Color> = cmds[i + 1..]
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { color, .. } => Some(*color),
                    _ => None,
                })
                .take(2)
                .collect();
            assert_eq!(inks.len(), 2, "a mode row lost its icon or its label");
            out.push((*color, inks[0], inks[1]));
        }
        assert_eq!(out.len(), 4, "the picker stopped offering four modes");
        out
    }

    /// A palette wearing one of the accents the appearance page actually
    /// offers, which — unlike the magenta fixture — differs between modes.
    fn wearing(light: bool, accent: AccentColor) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = if light {
            accent.color_light()
        } else {
            accent.color()
        };
        p
    }

    /// A manager with something to say: a mode set, a suppressed notification
    /// and a rule. A manager at defaults leaves three of the page's sites
    /// unrendered, and an unrendered site is an unchecked one.
    fn busy() -> FocusAssistManager {
        let mut mgr = FocusAssistManager::new();
        mgr.set_mode(FocusMode::AlarmsOnly);
        mgr.record_suppressed();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::TotalSilence,
        });
        mgr
    }

    // ---- FocusMode ----

    #[test]
    fn mode_labels() {
        assert_eq!(FocusMode::Off.label(), "Off");
        assert_eq!(FocusMode::PriorityOnly.label(), "Priority Only");
        assert_eq!(FocusMode::AlarmsOnly.label(), "Alarms Only");
        assert_eq!(FocusMode::TotalSilence.label(), "Total Silence");
    }

    #[test]
    fn mode_icons_not_empty() {
        assert!(!FocusMode::Off.icon().is_empty());
        assert!(!FocusMode::PriorityOnly.icon().is_empty());
    }

    // ---- Manual mode ----

    #[test]
    fn initial_state_off() {
        let mgr = make_mgr();
        assert_eq!(mgr.effective_mode(), FocusMode::Off);
        assert!(!mgr.is_active());
    }

    #[test]
    fn set_mode() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::PriorityOnly);
        assert_eq!(mgr.effective_mode(), FocusMode::PriorityOnly);
        assert!(mgr.is_active());
        assert!(mgr.manual_override);
    }

    #[test]
    fn set_mode_off_clears() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::TotalSilence);
        mgr.suppressed_count = 5;
        mgr.set_mode(FocusMode::Off);
        assert_eq!(mgr.suppressed_count, 0);
        assert!(!mgr.manual_override);
    }

    #[test]
    fn cycle_mode() {
        let mut mgr = make_mgr();
        mgr.cycle_mode();
        assert_eq!(mgr.manual_mode, FocusMode::PriorityOnly);
        mgr.cycle_mode();
        assert_eq!(mgr.manual_mode, FocusMode::AlarmsOnly);
        mgr.cycle_mode();
        assert_eq!(mgr.manual_mode, FocusMode::TotalSilence);
        mgr.cycle_mode();
        assert_eq!(mgr.manual_mode, FocusMode::Off);
    }

    // ---- Notification filtering ----

    #[test]
    fn all_shown_when_off() {
        let mgr = make_mgr();
        assert!(mgr.should_show_notification("any_app"));
    }

    #[test]
    fn priority_only_filters_normal() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::PriorityOnly);
        mgr.set_app_override(
            AppNotifOverride::new("chat", "Chat").with_priority(NotifPriority::Priority),
        );
        assert!(mgr.should_show_notification("chat"));
        assert!(!mgr.should_show_notification("other_app")); // Normal priority
    }

    #[test]
    fn alarms_only_filters_most() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::AlarmsOnly);
        mgr.set_app_override(
            AppNotifOverride::new("alarm", "Alarm").with_priority(NotifPriority::Critical),
        );
        mgr.set_app_override(
            AppNotifOverride::new("chat", "Chat").with_priority(NotifPriority::Priority),
        );
        assert!(mgr.should_show_notification("alarm"));
        assert!(!mgr.should_show_notification("chat"));
        assert!(!mgr.should_show_notification("other"));
    }

    #[test]
    fn total_silence_blocks_all() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::TotalSilence);
        mgr.set_app_override(
            AppNotifOverride::new("alarm", "Alarm").with_priority(NotifPriority::Critical),
        );
        assert!(!mgr.should_show_notification("alarm"));
    }

    // ---- App overrides ----

    #[test]
    fn app_priority_default() {
        let mgr = make_mgr();
        assert_eq!(mgr.app_priority("unknown"), NotifPriority::Normal);
    }

    #[test]
    fn set_app_override_replaces() {
        let mut mgr = make_mgr();
        mgr.set_app_override(
            AppNotifOverride::new("chat", "Chat").with_priority(NotifPriority::Silent),
        );
        mgr.set_app_override(
            AppNotifOverride::new("chat", "Chat").with_priority(NotifPriority::Priority),
        );
        assert_eq!(mgr.app_priority("chat"), NotifPriority::Priority);
        assert_eq!(mgr.app_overrides.len(), 1);
    }

    // ---- Auto rules ----

    #[test]
    fn schedule_rule_active_within_range() {
        let rule = AutoRule::Schedule {
            window: DailyWindow::from_hm(22, 0, 7, 0).unwrap(),
            days: vec![],
            mode: FocusMode::AlarmsOnly,
        };
        assert!(rule.is_schedule_active(23, 30, 0)); // 11:30 PM
        assert!(rule.is_schedule_active(3, 0, 0)); // 3:00 AM
        assert!(!rule.is_schedule_active(12, 0, 0)); // noon
    }

    #[test]
    fn schedule_rule_respects_days() {
        let rule = AutoRule::Schedule {
            window: DailyWindow::from_hm(9, 0, 17, 0).unwrap(),
            days: vec![1, 2, 3, 4, 5], // weekdays
            mode: FocusMode::PriorityOnly,
        };
        assert!(rule.is_schedule_active(10, 0, 1)); // Monday
        assert!(!rule.is_schedule_active(10, 0, 0)); // Sunday
    }

    #[test]
    fn schedule_rule_daytime() {
        let rule = AutoRule::Schedule {
            window: DailyWindow::from_hm(9, 0, 17, 0).unwrap(),
            days: vec![],
            mode: FocusMode::PriorityOnly,
        };
        assert!(rule.is_schedule_active(12, 0, 0));
        assert!(!rule.is_schedule_active(20, 0, 0));
    }

    #[test]
    fn evaluate_fullscreen_rule() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::AlarmsOnly,
        });
        mgr.fullscreen_active = true;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::AlarmsOnly);
    }

    #[test]
    fn evaluate_presentation_rule() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Presentation {
            mode: FocusMode::TotalSilence,
        });
        mgr.presenting = true;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::TotalSilence);
    }

    #[test]
    fn evaluate_gaming_rule() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Gaming {
            mode: FocusMode::PriorityOnly,
        });
        mgr.gaming = true;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::PriorityOnly);
    }

    #[test]
    fn evaluate_battery_rule() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::BatteryLow {
            threshold_percent: 20,
            mode: FocusMode::AlarmsOnly,
        });
        mgr.on_battery = true;
        mgr.battery_percent = 15;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::AlarmsOnly);
    }

    #[test]
    fn battery_rule_not_active_when_charging() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::BatteryLow {
            threshold_percent: 20,
            mode: FocusMode::AlarmsOnly,
        });
        mgr.on_battery = false;
        mgr.battery_percent = 15;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::Off);
    }

    #[test]
    fn manual_overrides_auto() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::AlarmsOnly,
        });
        mgr.fullscreen_active = true;
        mgr.set_mode(FocusMode::PriorityOnly);
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::PriorityOnly);
    }

    #[test]
    fn multiple_rules_most_restrictive() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::PriorityOnly,
        });
        mgr.add_auto_rule(AutoRule::Presentation {
            mode: FocusMode::TotalSilence,
        });
        mgr.fullscreen_active = true;
        mgr.presenting = true;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::TotalSilence);
    }

    #[test]
    fn remove_auto_rule() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::AlarmsOnly,
        });
        assert!(mgr.remove_auto_rule(0));
        assert!(mgr.auto_rules.is_empty());
    }

    #[test]
    fn remove_auto_rule_out_of_bounds() {
        let mut mgr = make_mgr();
        assert!(!mgr.remove_auto_rule(0));
    }

    // ---- most_restrictive ----

    #[test]
    fn most_restrictive_fn() {
        assert_eq!(
            most_restrictive(FocusMode::Off, FocusMode::PriorityOnly),
            FocusMode::PriorityOnly
        );
        assert_eq!(
            most_restrictive(FocusMode::AlarmsOnly, FocusMode::PriorityOnly),
            FocusMode::AlarmsOnly
        );
        assert_eq!(
            most_restrictive(FocusMode::TotalSilence, FocusMode::AlarmsOnly),
            FocusMode::TotalSilence
        );
    }

    // ---- Rendering ----

    #[test]
    fn tray_indicator_hidden_when_off() {
        let mgr = make_mgr();
        let cmds = mgr.render_tray_indicator(&accented(false), 0.0, 0.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn tray_indicator_shown_when_active() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::PriorityOnly);
        let cmds = mgr.render_tray_indicator(&accented(false), 0.0, 0.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn settings_render_not_empty() {
        let mgr = make_mgr();
        let cmds = mgr.render_settings(&accented(false), 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn settings_render_with_rules() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::AlarmsOnly,
        });
        mgr.add_auto_rule(AutoRule::Schedule {
            window: DailyWindow::from_hm(22, 0, 7, 0).unwrap(),
            days: vec![],
            mode: FocusMode::AlarmsOnly,
        });
        let cmds = mgr.render_settings(&accented(false), 0.0, 0.0, 400.0);
        assert!(cmds.len() > 10);
    }

    // ---- Colour: the conversion off this module's own palette ----

    /// Nothing either renderer draws is outside the palette it was handed.
    ///
    /// Rendered in *both* modes, because a leftover Mocha constant is a legal
    /// colour in the dark render and only names itself in the light one.
    #[test]
    fn every_colour_the_module_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            for m in FocusMode::ALL {
                let mut mgr = busy();
                mgr.set_mode(m);
                assert_drawn_from(&p, &mgr.render_tray_indicator(&p, 0.0, 0.0), &[], "tray");
                assert_drawn_from(
                    &p,
                    &mgr.render_settings(&p, 0.0, 0.0, 400.0),
                    &[],
                    "focus assist settings",
                );
            }
            // Again with no rules, which is the only way to reach the
            // empty-state line.
            assert_drawn_from(
                &p,
                &make_mgr().render_settings(&p, 0.0, 0.0, 400.0),
                &[],
                "focus assist settings, no rules",
            );
        }
    }

    /// None of the nine deleted constants is still drawn.
    ///
    /// The membership sweep above cannot catch these on its own: in the *dark*
    /// render every one of them is a legitimate role, so only the light render
    /// can say a Mocha value survived. This table is the half of the proof the
    /// sweep is structurally unable to do — and unlike most modules there is no
    /// hole in it, because this module inks nothing with `crust`.
    #[test]
    fn none_of_the_nine_deleted_constants_is_still_drawn() {
        const DELETED: [(u32, &str); 9] = [
            (0x001E_1E2E, "BASE"),
            (0x0018_1825, "MANTLE"),
            (0x0031_3244, "SURFACE0"),
            (0x00CD_D6F4, "TEXT"),
            (0x00A6_ADC8, "SUBTEXT0"),
            (0x0089_B4FA, "BLUE"),
            (0x00F3_8BA8, "RED"),
            (0x00F9_E2AF, "YELLOW"),
            (0x006C_7086, "OVERLAY0"),
        ];
        let p = accented(true);
        for m in FocusMode::ALL {
            let mut mgr = busy();
            mgr.set_mode(m);
            let mut cmds = mgr.render_tray_indicator(&p, 0.0, 0.0);
            cmds.extend(mgr.render_settings(&p, 0.0, 0.0, 400.0));
            cmds.extend(make_mgr().render_settings(&p, 0.0, 0.0, 400.0));
            for c in colors(&cmds) {
                let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
                for (value, name) in DELETED {
                    assert_ne!(
                        rgb, value,
                        "with {m:?} chosen, the light render still draws the \
                         deleted Mocha `{name}` — a constant survived the \
                         conversion"
                    );
                }
            }
        }
    }

    /// Every colour of the settings page, in order, is the role it claims.
    ///
    /// The expected order is written out here rather than derived from
    /// [`FocusMode::ALL`], which is what lets it see the picker being
    /// reordered — a test that walked `ALL` to build its expectation would
    /// have the expectation permuted along with the render.
    #[test]
    fn every_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let mgr = busy(); // AlarmsOnly chosen: the third row.
            let mut want = vec![p.text, p.blue, p.overlay0];
            for chosen in [false, false, true, false] {
                want.push(if chosen { p.surface0 } else { p.mantle });
                want.push(if chosen { p.accent } else { p.subtext0 });
                want.push(if chosen { p.text } else { p.subtext0 });
                want.push(p.overlay0);
            }
            want.extend([p.text, p.surface0, p.text, p.overlay0]);
            assert_eq!(
                colors(&mgr.render_settings(&p, 0.0, 0.0, 400.0)),
                want,
                "the settings page's colours are not the roles it claims, in \
                 order (light = {light})"
            );
            assert_eq!(
                colors(&mgr.render_tray_indicator(&p, 0.0, 0.0)),
                vec![p.yellow, readable_on(p.yellow)],
                "the Alarms Only tray pill is not its own hue lettered for \
                 itself (light = {light})"
            );

            // The page a *fresh* manager draws, which is a different page:
            // Off is chosen, nothing has been suppressed, and no rule exists,
            // so this is the only render that reaches the empty-state line.
            // A site nothing renders is a site nothing checks.
            let mut want = vec![p.text, p.subtext0];
            for chosen in [true, false, false, false] {
                want.push(if chosen { p.surface0 } else { p.mantle });
                want.push(if chosen { p.accent } else { p.subtext0 });
                want.push(if chosen { p.text } else { p.subtext0 });
                want.push(p.overlay0);
            }
            want.extend([p.text, p.overlay0]);
            assert_eq!(
                colors(&make_mgr().render_settings(&p, 0.0, 0.0, 400.0)),
                want,
                "the empty page's colours are not the roles it claims, in \
                 order (light = {light})"
            );
        }
    }

    /// The tray icon is legible on the pill it sits on, in every mode.
    ///
    /// This is the module's real bug fix. The deleted `BASE` measured 3.02:1
    /// on Latte red, 3.12:1 on Latte yellow and 3.13:1 on Latte blue — every
    /// active mode below the 4.5:1 floor at once. `readable_on` is a *step*
    /// function of the fill's brightness, so a test that sampled one mode in
    /// one theme would still pass with a hard-coded ink; the assertion at the
    /// end is what makes this a claim about the function rather than about a
    /// sample.
    #[test]
    fn the_tray_icon_is_inked_for_its_own_pill() {
        let mut saw_dark_ink = false;
        let mut saw_light_ink = false;
        for light in [false, true] {
            let p = accented(light);
            for m in FocusMode::ALL {
                let mut mgr = make_mgr();
                mgr.set_mode(m);
                let cmds = mgr.render_tray_indicator(&p, 0.0, 0.0);
                let Some(fill) = m.hue(&p) else {
                    assert!(cmds.is_empty(), "Off drew a tray pill");
                    continue;
                };
                let got = colors(&cmds);
                assert_eq!(got, vec![fill, readable_on(fill)], "{m:?} tray ink");
                if got[1] == Color::from_hex(0x0011_111B) {
                    saw_dark_ink = true;
                } else {
                    saw_light_ink = true;
                }
            }
        }
        assert!(
            saw_dark_ink && saw_light_ink,
            "the three mode hues produced only one of readable_on's two \
             answers, so this test would still pass if the ink were a \
             constant — the fixture set, not the code, needs widening"
        );
    }

    /// The mode hues are a severity code: three different colours, none of
    /// them the accent.
    ///
    /// If they followed the accent they would be one colour saying three
    /// things, and the tray would stop telling the user *how* silent they are.
    #[test]
    fn the_mode_hues_are_a_severity_code_and_never_the_accent() {
        for light in [false, true] {
            let p = accented(light);
            let hues: Vec<Color> = FocusMode::ALL.iter().filter_map(|m| m.hue(&p)).collect();
            assert_eq!(hues, vec![p.blue, p.yellow, p.red], "the severity scale");
            for (i, a) in hues.iter().enumerate() {
                assert_ne!(*a, p.accent, "mode hue {i} followed the accent");
                for b in &hues[i + 1..] {
                    assert_ne!(
                        a, b,
                        "two modes share a hue, so the scale has a rung missing"
                    );
                }
            }
        }
    }

    /// The picker marks exactly one row with the accent, and it is the chosen
    /// one — while the *unchosen* rows never wear it.
    #[test]
    fn the_picker_marks_the_chosen_row_with_the_accent() {
        for light in [false, true] {
            let p = accented(light);
            for (i, &m) in FocusMode::ALL.iter().enumerate() {
                let mut mgr = busy();
                mgr.set_mode(m);
                let bar = rows(&mgr.render_settings(&p, 0.0, 0.0, 400.0));
                let lit: Vec<usize> = bar
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, icon, _))| *icon == p.accent)
                    .map(|(j, _)| j)
                    .collect();
                assert_eq!(
                    lit,
                    vec![i],
                    "with {m:?} chosen, accented rows were {lit:?}"
                );
                for (j, (bg, icon, label)) in bar.iter().enumerate() {
                    let chosen = j == i;
                    assert_eq!(
                        *bg,
                        if chosen { p.surface0 } else { p.mantle },
                        "row {j} bg"
                    );
                    if !chosen {
                        assert_eq!(*icon, p.subtext0, "unchosen row {j} icon");
                        assert_eq!(*label, p.subtext0, "unchosen row {j} label");
                    }
                }
            }
        }
    }

    /// "Current:" is quiet when focus assist is off and speaks when it is on —
    /// and what it speaks is a hue role, not the accent.
    #[test]
    fn the_current_line_is_quiet_when_off_and_never_the_accent() {
        for light in [false, true] {
            let p = accented(light);
            for m in FocusMode::ALL {
                let mut mgr = make_mgr();
                mgr.set_mode(m);
                // Second colour of the page: the title, then this line.
                let got = colors(&mgr.render_settings(&p, 0.0, 0.0, 400.0))[1];
                let want = if m == FocusMode::Off {
                    p.subtext0
                } else {
                    p.blue
                };
                assert_eq!(got, want, "the Current line with {m:?} chosen");
                assert_ne!(got, p.accent, "the Current line followed the accent");
            }
        }
    }

    /// The suppressed-notification line exists only when there is one to
    /// report, so a test that never suppressed anything checks nothing.
    #[test]
    fn the_suppressed_line_is_overlay_and_only_drawn_when_there_is_one() {
        let p = accented(false);
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::TotalSilence);
        let quiet = colors(&mgr.render_settings(&p, 0.0, 0.0, 400.0));
        mgr.record_suppressed();
        let loud = colors(&mgr.render_settings(&p, 0.0, 0.0, 400.0));
        assert_eq!(
            loud.len(),
            quiet.len() + 1,
            "suppressing a notification did not add the line that reports it"
        );
        assert_eq!(
            loud[2], p.overlay0,
            "the suppressed-count line is not overlay0"
        );
    }

    /// Changing the mode changes what is drawn — at every site, not most.
    ///
    /// The accent has to move too, or the picker's chosen icon would be the
    /// one site that legitimately stayed put and would hide a site that did
    /// not follow the palette at all.
    #[test]
    fn every_site_changes_when_the_mode_does() {
        let dark = wearing(false, AccentColor::Mauve);
        let light = wearing(true, AccentColor::Mauve);
        let mgr = busy();
        let a = colors(&mgr.render_settings(&dark, 0.0, 0.0, 400.0));
        let b = colors(&mgr.render_settings(&light, 0.0, 0.0, 400.0));
        assert_eq!(a.len(), b.len(), "the two modes drew different pages");
        for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
            assert_ne!(x, y, "colour {i} is the same in both modes");
        }
        let t = colors(&mgr.render_tray_indicator(&dark, 0.0, 0.0));
        let u = colors(&mgr.render_tray_indicator(&light, 0.0, 0.0));
        for (i, (x, y)) in t.iter().zip(u.iter()).enumerate() {
            assert_ne!(x, y, "tray colour {i} is the same in both modes");
        }
    }

    #[test]
    fn suppressed_count() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::TotalSilence);
        mgr.record_suppressed();
        mgr.record_suppressed();
        assert_eq!(mgr.suppressed_count, 2);
    }

    // ---- AutoRule label ----

    #[test]
    fn auto_rule_labels() {
        let r = AutoRule::Schedule {
            window: DailyWindow::from_hm(22, 0, 7, 0).unwrap(),
            days: vec![],
            mode: FocusMode::AlarmsOnly,
        };
        assert!(r.label().contains("22:00"));
        assert_eq!(
            AutoRule::Fullscreen {
                mode: FocusMode::Off
            }
            .label(),
            "Fullscreen app"
        );
        assert_eq!(
            AutoRule::Presentation {
                mode: FocusMode::Off
            }
            .label(),
            "Presenting"
        );
        assert_eq!(
            AutoRule::Gaming {
                mode: FocusMode::Off
            }
            .label(),
            "Gaming"
        );
    }

    // ---- NotifPriority ordering ----

    #[test]
    fn priority_ordering() {
        assert!(NotifPriority::Critical > NotifPriority::Priority);
        assert!(NotifPriority::Priority > NotifPriority::Normal);
        assert!(NotifPriority::Normal > NotifPriority::Silent);
    }
}

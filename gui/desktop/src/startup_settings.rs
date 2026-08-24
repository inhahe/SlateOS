//! Startup and autorun settings panel for the desktop shell.
//!
//! Manages applications that run automatically at login, including
//! startup delay, impact assessment, and per-app enable/disable control.

use appearance::{Palette, readable_on};
use guitk::color::Color;
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

// ============================================================================
// Colour
// ============================================================================
//
// This panel draws no colour of its own: every one comes from the `Palette`
// threaded through `render`. Three judgements decide which role each site got.
//
// **Five sites follow the user's accent**, because each marks a position you
// can move or an invitation you can take: the active tab's pill, the per-entry
// enable switch in the apps list, and the three switches on the boot tab
// (measure boot time, fast startup, auto-disable failing apps).
//
// The three boot-tab switches and the per-entry switch were a hardcoded green
// before the conversion, which made this module the only one of the shell's
// settings panels whose switches did not follow the accent. Note that mapping
// them to `p.green` would have been just as much of a choice as mapping them to
// `p.accent` — a hardcoded Mocha hex has no role until someone assigns one, so
// there is no "leave it alone" option here. `p.accent` is the answer the rest
// of the desktop already gives, and it is also the safer one *within this
// module*: the apps list draws the enable switch and the impact badge on the
// same row, and the impact scale's lowest rung is green, so a green switch
// collides with a "None" badge on a stock install. Following the accent moves
// that collision from "always" to "only for a user who picks Green".
//
// **Three scales stay put under every accent**, because each reports a fact
// about the machine rather than a choice the user made:
//
//   * `StartupImpact::color` — green/yellow/red/grey. Note it is five variants
//     over four colours: `None` and `Low` are both green, deliberately, since
//     the colour is a three-band traffic light laid over a five-value label.
//     Distinctness is therefore a claim about the bands, not the variants.
//   * `boot_time_color` — the last boot reading, banded at 10s and 30s. A
//     measurement is never an invitation.
//   * The failure badge and the high-impact banner, both red: an error is a
//     category, and on a Red desktop an accented error would be unreadable as
//     an error.
//
// **Three labels are chosen from the fill beneath them**, not fixed: the
// active tab's label is `p.on_accent()`, and the impact badge's and failure
// badge's labels are `readable_on` of their own categorical fills. The impact
// badge is the one that was actually *wrong* rather than merely fragile — its
// label was a hardcoded near-black, which on the grey `NotMeasured` fill is
// poor contrast in dark mode and does not improve in light.
//
// The switch knobs remain `p.text` on the pill. That is low contrast when the
// pill is a pale accent, but it is the convention in all forty-nine shell
// modules and is tracked as a single cross-module sweep in `known-issues.md`
// (`TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL`); fixing it here alone
// would make this panel inconsistent with every other one.

// ============================================================================
// Startup impact
// ============================================================================

/// Estimated impact of a startup app on boot time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StartupImpact {
    /// Negligible impact (< 100ms).
    None,
    /// Low impact (100-500ms).
    Low,
    /// Medium impact (500ms-2s).
    Medium,
    /// High impact (> 2s).
    High,
    /// Impact not yet measured.
    NotMeasured,
}

impl StartupImpact {
    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::NotMeasured => "Not measured",
        }
    }

    /// The badge colour for this impact level.
    ///
    /// Five variants, four colours: `None` and `Low` share green because the
    /// colour is a three-band traffic light (fine / slow / bad, plus grey for
    /// unmeasured) laid over a finer-grained label. Any test asserting these
    /// stay distinct must therefore compare the bands and not the variants.
    fn color(self, p: &Palette) -> Color {
        match self {
            Self::None | Self::Low => p.green,
            Self::Medium => p.yellow,
            Self::High => p.red,
            Self::NotMeasured => p.overlay0,
        }
    }

    /// Classify impact from startup time in milliseconds.
    pub fn from_millis(ms: u64) -> Self {
        if ms < 100 {
            Self::None
        } else if ms < 500 {
            Self::Low
        } else if ms < 2000 {
            Self::Medium
        } else {
            Self::High
        }
    }
}

// ============================================================================
// Startup type
// ============================================================================

/// How a startup app was registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupType {
    /// User-configured autostart (e.g. added via settings).
    User,
    /// System service that starts at login.
    System,
    /// Package-installed autostart entry.
    Package,
    /// Scheduled task that runs at login.
    Scheduled,
}

impl StartupType {
    fn label(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::System => "System",
            Self::Package => "Package",
            Self::Scheduled => "Scheduled",
        }
    }
}

// ============================================================================
// Startup entry
// ============================================================================

/// A single startup/autorun entry.
#[derive(Clone, Debug)]
pub struct StartupEntry {
    /// Unique identifier.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Publisher/developer name.
    pub publisher: String,
    /// Executable path.
    pub command: String,
    /// Command-line arguments.
    pub args: String,
    /// Whether this entry is enabled.
    pub enabled: bool,
    /// How it was registered.
    pub startup_type: StartupType,
    /// Estimated impact on boot time.
    pub impact: StartupImpact,
    /// Measured startup time in milliseconds (if measured).
    pub startup_time_ms: Option<u64>,
    /// Delay before starting (milliseconds after login).
    pub delay_ms: u64,
    /// Whether to run minimized/hidden.
    pub run_hidden: bool,
    /// Date when this entry was added (seconds since epoch).
    pub added_at: u64,
    /// Last time this entry successfully started.
    pub last_run_at: Option<u64>,
    /// Number of consecutive failures.
    pub failure_count: u32,
}

impl StartupEntry {
    pub fn new(
        id: u64,
        name: impl Into<String>,
        publisher: impl Into<String>,
        command: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            publisher: publisher.into(),
            command: command.into(),
            args: String::new(),
            enabled: true,
            startup_type: StartupType::User,
            impact: StartupImpact::NotMeasured,
            startup_time_ms: None,
            delay_ms: 0,
            run_hidden: false,
            added_at: 0,
            last_run_at: None,
            failure_count: 0,
        }
    }

    /// Whether this entry has a delay configured.
    pub fn has_delay(&self) -> bool {
        self.delay_ms > 0
    }

    /// Human-readable delay string.
    ///
    /// `"Immediate"` stays here rather than moving into the shared formatter:
    /// it is a word for *no delay configured*, not a rendering of zero.
    ///
    /// Everything above it had no minutes branch, so a startup item held back
    /// five minutes — which the settings screen lets you configure — read
    /// `300.0s`.
    pub fn delay_text(&self) -> String {
        if self.delay_ms == 0 {
            "Immediate".to_string()
        } else {
            guitk::duration::units_ms(self.delay_ms)
        }
    }

    /// Whether this entry appears to be failing.
    pub fn is_failing(&self) -> bool {
        self.failure_count >= 3
    }
}

// ============================================================================
// Startup settings manager
// ============================================================================

/// Sort order for the startup list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupSort {
    Name,
    Impact,
    StartupType,
    Status,
}

impl StartupSort {
    fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Impact => "Impact",
            Self::StartupType => "Type",
            Self::Status => "Status",
        }
    }
}

/// Boot performance config.
#[derive(Clone, Debug)]
pub struct BootConfig {
    /// Whether to show boot time measurement.
    pub measure_boot_time: bool,
    /// Last measured total boot time (milliseconds).
    pub last_boot_time_ms: Option<u64>,
    /// Whether to use fast startup (hibernate-resume instead of full boot).
    pub fast_startup: bool,
    /// Maximum delay before all startup apps must be launched.
    pub max_startup_delay_ms: u64,
    /// Whether to auto-disable apps that fail too many times.
    pub auto_disable_failing: bool,
    /// Failure count threshold before auto-disabling.
    pub auto_disable_threshold: u32,
}

impl Default for BootConfig {
    fn default() -> Self {
        Self {
            measure_boot_time: true,
            last_boot_time_ms: None,
            fast_startup: true,
            max_startup_delay_ms: 30000,
            auto_disable_failing: true,
            auto_disable_threshold: 5,
        }
    }
}

/// Manages startup entries and boot configuration.
pub struct StartupSettings {
    /// All registered startup entries.
    pub entries: Vec<StartupEntry>,
    /// Boot performance configuration.
    pub boot_config: BootConfig,
    /// Source of entry IDs.
    ids: IdSeq,
}

impl StartupSettings {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            boot_config: BootConfig::default(),
            ids: IdSeq::new(),
        }
    }

    /// Add a new startup entry. Returns its ID.
    pub fn add_entry(
        &mut self,
        name: impl Into<String>,
        publisher: impl Into<String>,
        command: impl Into<String>,
    ) -> u64 {
        let id = self.ids.issue_infallible();
        self.entries
            .push(StartupEntry::new(id, name, publisher, command));
        id
    }

    /// Remove a startup entry by ID.
    pub fn remove_entry(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() < before
    }

    /// Get a startup entry by ID.
    pub fn get_entry(&self, id: u64) -> Option<&StartupEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Get a mutable startup entry by ID.
    pub fn get_entry_mut(&mut self, id: u64) -> Option<&mut StartupEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Enable a startup entry.
    pub fn enable(&mut self, id: u64) -> bool {
        if let Some(e) = self.get_entry_mut(id) {
            e.enabled = true;
            true
        } else {
            false
        }
    }

    /// Disable a startup entry.
    pub fn disable(&mut self, id: u64) -> bool {
        if let Some(e) = self.get_entry_mut(id) {
            e.enabled = false;
            true
        } else {
            false
        }
    }

    /// Toggle a startup entry's enabled state.
    pub fn toggle(&mut self, id: u64) -> bool {
        if let Some(e) = self.get_entry_mut(id) {
            e.enabled = !e.enabled;
            true
        } else {
            false
        }
    }

    /// Number of entries.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Number of enabled entries.
    pub fn enabled_count(&self) -> usize {
        self.entries.iter().filter(|e| e.enabled).count()
    }

    /// Number of disabled entries.
    pub fn disabled_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.enabled).count()
    }

    /// Entries with high impact.
    pub fn high_impact_entries(&self) -> Vec<&StartupEntry> {
        self.entries
            .iter()
            .filter(|e| e.impact == StartupImpact::High && e.enabled)
            .collect()
    }

    /// Entries that are failing.
    pub fn failing_entries(&self) -> Vec<&StartupEntry> {
        self.entries.iter().filter(|e| e.is_failing()).collect()
    }

    /// Total estimated startup impact in milliseconds (enabled entries only).
    pub fn total_impact_ms(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.enabled)
            .filter_map(|e| e.startup_time_ms)
            .sum()
    }

    /// Get entries sorted by the given criteria.
    pub fn sorted_entries(&self, sort: StartupSort) -> Vec<&StartupEntry> {
        let mut entries: Vec<_> = self.entries.iter().collect();
        match sort {
            StartupSort::Name => entries.sort_by_key(|a| a.name.to_lowercase()),
            StartupSort::Impact => entries.sort_by_key(|e| std::cmp::Reverse(e.impact)),
            StartupSort::StartupType => {
                entries.sort_by(|a, b| a.startup_type.label().cmp(b.startup_type.label()));
            }
            StartupSort::Status => entries.sort_by_key(|e| std::cmp::Reverse(e.enabled)),
        }
        entries
    }

    /// Auto-disable entries that have exceeded the failure threshold.
    pub fn auto_disable_failing(&mut self) -> Vec<u64> {
        if !self.boot_config.auto_disable_failing {
            return Vec::new();
        }
        let threshold = self.boot_config.auto_disable_threshold;
        let mut disabled = Vec::new();
        for entry in &mut self.entries {
            if entry.enabled && entry.failure_count >= threshold {
                entry.enabled = false;
                disabled.push(entry.id);
            }
        }
        disabled
    }

    /// Record a startup failure for an entry.
    pub fn record_failure(&mut self, id: u64) -> bool {
        if let Some(e) = self.get_entry_mut(id) {
            e.failure_count = e.failure_count.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Record a successful startup for an entry.
    pub fn record_success(&mut self, id: u64, timestamp: u64, startup_ms: u64) -> bool {
        if let Some(e) = self.get_entry_mut(id) {
            e.failure_count = 0;
            e.last_run_at = Some(timestamp);
            e.startup_time_ms = Some(startup_ms);
            e.impact = StartupImpact::from_millis(startup_ms);
            true
        } else {
            false
        }
    }
}

impl Default for StartupSettings {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// UI
// ============================================================================

/// The colour a last-boot reading of `ms` is drawn in.
///
/// A measurement, not a choice, so it does not follow the accent: a boot that
/// took forty seconds is red on a red desktop and red on a green one. The
/// bands are ten and thirty seconds.
fn boot_time_color(ms: u64, p: &Palette) -> Color {
    if ms < 10_000 {
        p.green
    } else if ms < 30_000 {
        p.yellow
    } else {
        p.red
    }
}

/// Active tab in the startup settings UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupTab {
    /// Startup apps list.
    Apps,
    /// Boot performance settings.
    Boot,
}

impl StartupTab {
    fn label(self) -> &'static str {
        match self {
            Self::Apps => "Startup Apps",
            Self::Boot => "Boot Performance",
        }
    }
}

/// Startup settings UI state.
pub struct StartupSettingsUI {
    /// Active tab.
    pub active_tab: StartupTab,
    /// Underlying settings.
    pub settings: StartupSettings,
    /// Current sort order.
    pub sort: StartupSort,
    /// Selected entry ID.
    pub selected_id: Option<u64>,
    /// Filter text.
    pub filter: String,
    /// Whether to show only enabled entries.
    pub show_enabled_only: bool,
}

impl StartupSettingsUI {
    pub fn new() -> Self {
        Self {
            active_tab: StartupTab::Apps,
            settings: StartupSettings::new(),
            sort: StartupSort::Name,
            selected_id: None,
            filter: String::new(),
            show_enabled_only: false,
        }
    }

    pub fn set_tab(&mut self, tab: StartupTab) {
        self.active_tab = tab;
    }

    /// Get filtered and sorted entries.
    fn visible_entries(&self) -> Vec<&StartupEntry> {
        let filter_lower = self.filter.to_lowercase();
        let mut entries: Vec<_> = self
            .settings
            .entries
            .iter()
            .filter(|e| {
                if self.show_enabled_only && !e.enabled {
                    return false;
                }
                if filter_lower.is_empty() {
                    return true;
                }
                e.name.to_lowercase().contains(&filter_lower)
                    || e.publisher.to_lowercase().contains(&filter_lower)
                    || e.command.to_lowercase().contains(&filter_lower)
            })
            .collect();

        match self.sort {
            StartupSort::Name => entries.sort_by_key(|a| a.name.to_lowercase()),
            StartupSort::Impact => entries.sort_by_key(|e| std::cmp::Reverse(e.impact)),
            StartupSort::StartupType => {
                entries.sort_by(|a, b| a.startup_type.label().cmp(b.startup_type.label()));
            }
            StartupSort::Status => entries.sort_by_key(|e| std::cmp::Reverse(e.enabled)),
        }

        entries
    }

    pub fn render(&self, p: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 24.0,
            text: "Startup Apps".into(),
            font_size: 22.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Stats
        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 50.0,
            text: format!(
                "{} apps ({} enabled, {} disabled)",
                self.settings.count(),
                self.settings.enabled_count(),
                self.settings.disabled_count(),
            ),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 48.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Tabs
        let tabs = [StartupTab::Apps, StartupTab::Boot];
        let tab_y = 72.0;
        let mut tx = 24.0;
        for &tab in &tabs {
            let active = tab == self.active_tab;
            let tw = text::padded_width_any_weight(tab.label(), 10.0, 13.0);
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: tab_y,
                width: tw,
                height: 32.0,
                color: if active { p.accent } else { p.surface0 },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: tab_y + 8.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if active { p.on_accent() } else { p.subtext0 },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tw - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
            tx += tw + 8.0;
        }

        let cy = tab_y + 48.0;
        let cw = width - 48.0;

        match self.active_tab {
            StartupTab::Apps => {
                self.render_apps_tab(p, &mut cmds, 24.0, cy, cw, height - cy - 16.0);
            }
            StartupTab::Boot => self.render_boot_tab(p, &mut cmds, 24.0, cy, cw),
        }

        cmds
    }

    fn render_apps_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let mut cy = y;

        // Filter bar
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 30.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(6.0),
        });
        let filter_text = if self.filter.is_empty() {
            "Filter startup apps...".to_string()
        } else {
            self.filter.clone()
        };
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: cy + 7.0,
            text: filter_text,
            font_size: 13.0,
            color: if self.filter.is_empty() {
                p.overlay0
            } else {
                p.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 40.0;

        // Sort indicator
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!("Sort: {}", self.sort.label()),
            font_size: 11.0,
            color: p.overlay0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 22.0;

        // Impact warnings
        let high_impact = self.settings.high_impact_entries();
        if !high_impact.is_empty() {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 28.0,
                color: Color::rgba(p.red.r, p.red.g, p.red.b, 40),
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 6.0,
                text: format!(
                    "{} high-impact apps slowing your startup",
                    high_impact.len()
                ),
                font_size: 12.0,
                color: p.red,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 36.0;
        }

        // Entry list
        let visible = self.visible_entries();
        if visible.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 20.0,
                text: "No startup apps".into(),
                font_size: 13.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        for entry in visible.iter().take(15) {
            let is_selected = self.selected_id == Some(entry.id);

            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 56.0,
                color: if is_selected { p.surface1 } else { p.surface0 },
                corner_radii: CornerRadii::all(6.0),
            });

            // Enable/disable toggle
            let toggle_color = if entry.enabled { p.accent } else { p.surface2 };
            cmds.push(RenderCommand::FillRect {
                x: x + 8.0,
                y: cy + 18.0,
                width: 36.0,
                height: 20.0,
                color: toggle_color,
                corner_radii: CornerRadii::all(10.0),
            });
            let knob_x = if entry.enabled { x + 26.0 } else { x + 10.0 };
            cmds.push(RenderCommand::FillRect {
                x: knob_x,
                y: cy + 20.0,
                width: 16.0,
                height: 16.0,
                color: p.text,
                corner_radii: CornerRadii::all(8.0),
            });

            // Name
            cmds.push(RenderCommand::Text {
                x: x + 52.0,
                y: cy + 6.0,
                text: entry.name.clone(),
                font_size: 14.0,
                color: if entry.enabled { p.text } else { p.overlay0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(width * 0.5),
                overflow: TextOverflow::Ellipsis,
            });

            // Publisher and type
            cmds.push(RenderCommand::Text {
                x: x + 52.0,
                y: cy + 24.0,
                text: format!("{} - {}", entry.publisher, entry.startup_type.label()),
                font_size: 11.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.5),
                overflow: TextOverflow::Ellipsis,
            });

            // Delay
            if entry.has_delay() {
                cmds.push(RenderCommand::Text {
                    x: x + 52.0,
                    y: cy + 40.0,
                    text: format!("Delay: {}", entry.delay_text()),
                    font_size: 10.0,
                    color: p.overlay0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Impact badge
            let impact_color = entry.impact.color(p);
            let impact_label = entry.impact.label();
            cmds.push(RenderCommand::FillRect {
                x: x + width - 90.0,
                y: cy + 8.0,
                width: 74.0,
                height: 20.0,
                color: impact_color,
                corner_radii: CornerRadii::all(10.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + width - 84.0,
                y: cy + 11.0,
                text: impact_label.into(),
                font_size: 11.0,
                color: readable_on(impact_color),
                font_weight: FontWeightHint::Bold,
                max_width: Some(64.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Failure indicator
            if entry.is_failing() {
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 90.0,
                    y: cy + 34.0,
                    width: 74.0,
                    height: 16.0,
                    color: p.red,
                    corner_radii: CornerRadii::all(8.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + width - 84.0,
                    y: cy + 36.0,
                    text: format!("{} fails", entry.failure_count),
                    font_size: 10.0,
                    color: readable_on(p.red),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(64.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            cy += 62.0;
        }
    }

    fn render_boot_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;
        let cfg = &self.settings.boot_config;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Boot Performance".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        // Last boot time
        if let Some(ms) = cfg.last_boot_time_ms {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 48.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 6.0,
                text: "Last Boot Time".into(),
                font_size: 12.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 24.0,
                text: format!("{:.1}s", ms as f64 / 1000.0),
                font_size: 18.0,
                color: boot_time_color(ms, p),
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 58.0;
        }

        // Total startup impact
        let total_ms = self.settings.total_impact_ms();
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!("Total startup app impact: {:.1}s", total_ms as f64 / 1000.0),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 28.0;

        // Toggle rows
        self.render_toggle_row(
            p,
            cmds,
            x,
            cy,
            width,
            "Measure Boot Time",
            cfg.measure_boot_time,
        );
        cy += 36.0;

        self.render_toggle_row(p, cmds, x, cy, width, "Fast Startup", cfg.fast_startup);
        cy += 36.0;

        self.render_toggle_row(
            p,
            cmds,
            x,
            cy,
            width,
            "Auto-disable Failing Apps",
            cfg.auto_disable_failing,
        );
        cy += 36.0;

        // Thresholds
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Thresholds".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            p,
            cmds,
            x,
            cy,
            width,
            "Max Startup Delay",
            &format!("{:.0}s", cfg.max_startup_delay_ms as f64 / 1000.0),
        );
        cy += 28.0;

        self.render_label_value(
            p,
            cmds,
            x,
            cy,
            width,
            "Fail Threshold",
            &format!("{} consecutive failures", cfg.auto_disable_threshold),
        );
        let _ = cy;
    }

    fn render_toggle_row(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        enabled: bool,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y: y + 4.0,
            text: label.into(),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 80.0),
            overflow: TextOverflow::Ellipsis,
        });
        let sw_x = x + width - 44.0;
        cmds.push(RenderCommand::FillRect {
            x: sw_x,
            y: y + 2.0,
            width: 40.0,
            height: 22.0,
            color: if enabled { p.accent } else { p.surface2 },
            corner_radii: CornerRadii::all(11.0),
        });
        let knob_x = if enabled { sw_x + 20.0 } else { sw_x + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 4.0,
            width: 18.0,
            height: 18.0,
            color: p.text,
            corner_radii: CornerRadii::all(9.0),
        });
    }

    fn render_label_value(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: label.into(),
            font_size: 13.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55,
            y,
            text: value.into(),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.45),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

impl Default for StartupSettingsUI {
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

    // ---- StartupImpact ----

    #[test]
    fn test_impact_from_millis() {
        assert_eq!(StartupImpact::from_millis(50), StartupImpact::None);
        assert_eq!(StartupImpact::from_millis(200), StartupImpact::Low);
        assert_eq!(StartupImpact::from_millis(800), StartupImpact::Medium);
        assert_eq!(StartupImpact::from_millis(3000), StartupImpact::High);
    }

    #[test]
    fn test_impact_labels() {
        assert_eq!(StartupImpact::None.label(), "None");
        assert_eq!(StartupImpact::High.label(), "High");
    }

    #[test]
    fn test_impact_ordering() {
        assert!(StartupImpact::None < StartupImpact::Low);
        assert!(StartupImpact::Low < StartupImpact::Medium);
        assert!(StartupImpact::Medium < StartupImpact::High);
    }

    // ---- StartupType ----

    #[test]
    fn test_startup_type_labels() {
        assert_eq!(StartupType::User.label(), "User");
        assert_eq!(StartupType::System.label(), "System");
    }

    // ---- StartupEntry ----

    #[test]
    fn test_entry_new() {
        let e = StartupEntry::new(1, "Test App", "Publisher", "/usr/bin/test");
        assert_eq!(e.id, 1);
        assert_eq!(e.name, "Test App");
        assert!(e.enabled);
        assert_eq!(e.startup_type, StartupType::User);
        assert_eq!(e.impact, StartupImpact::NotMeasured);
    }

    #[test]
    fn test_entry_has_delay() {
        let mut e = StartupEntry::new(1, "A", "B", "C");
        assert!(!e.has_delay());
        e.delay_ms = 5000;
        assert!(e.has_delay());
    }

    #[test]
    fn test_entry_delay_text() {
        let mut e = StartupEntry::new(1, "A", "B", "C");
        assert_eq!(e.delay_text(), "Immediate");
        e.delay_ms = 500;
        assert_eq!(e.delay_text(), "500ms");
        e.delay_ms = 2500;
        assert_eq!(e.delay_text(), "2.5s");
        // Regression: the old body's ladder ended at seconds, so a five-minute
        // delay — which this very screen lets you configure — read "300.0s".
        e.delay_ms = 300_000;
        assert_eq!(e.delay_text(), "5m 0s");
    }

    #[test]
    fn test_entry_is_failing() {
        let mut e = StartupEntry::new(1, "A", "B", "C");
        assert!(!e.is_failing());
        e.failure_count = 3;
        assert!(e.is_failing());
    }

    // ---- StartupSettings ----

    #[test]
    fn test_settings_new() {
        let s = StartupSettings::new();
        assert_eq!(s.count(), 0);
    }

    #[test]
    fn test_add_entry() {
        let mut s = StartupSettings::new();
        let id = s.add_entry("App", "Pub", "/bin/app");
        assert_eq!(s.count(), 1);
        assert!(s.get_entry(id).is_some());
    }

    #[test]
    fn test_remove_entry() {
        let mut s = StartupSettings::new();
        let id = s.add_entry("App", "Pub", "/bin/app");
        assert!(s.remove_entry(id));
        assert_eq!(s.count(), 0);
        assert!(!s.remove_entry(id));
    }

    #[test]
    fn test_enable_disable() {
        let mut s = StartupSettings::new();
        let id = s.add_entry("App", "Pub", "/bin/app");
        assert!(s.disable(id));
        assert!(!s.get_entry(id).unwrap().enabled);
        assert!(s.enable(id));
        assert!(s.get_entry(id).unwrap().enabled);
    }

    #[test]
    fn test_toggle() {
        let mut s = StartupSettings::new();
        let id = s.add_entry("App", "Pub", "/bin/app");
        assert!(s.get_entry(id).unwrap().enabled);
        s.toggle(id);
        assert!(!s.get_entry(id).unwrap().enabled);
        s.toggle(id);
        assert!(s.get_entry(id).unwrap().enabled);
    }

    #[test]
    fn test_enabled_disabled_count() {
        let mut s = StartupSettings::new();
        let id1 = s.add_entry("A", "P", "/a");
        let _id2 = s.add_entry("B", "P", "/b");
        s.disable(id1);
        assert_eq!(s.enabled_count(), 1);
        assert_eq!(s.disabled_count(), 1);
    }

    #[test]
    fn test_high_impact_entries() {
        let mut s = StartupSettings::new();
        let id = s.add_entry("Heavy App", "P", "/heavy");
        if let Some(e) = s.get_entry_mut(id) {
            e.impact = StartupImpact::High;
        }
        assert_eq!(s.high_impact_entries().len(), 1);
    }

    #[test]
    fn test_failing_entries() {
        let mut s = StartupSettings::new();
        let id = s.add_entry("Bad App", "P", "/bad");
        s.record_failure(id);
        s.record_failure(id);
        s.record_failure(id);
        assert_eq!(s.failing_entries().len(), 1);
    }

    #[test]
    fn test_total_impact() {
        let mut s = StartupSettings::new();
        let id1 = s.add_entry("A", "P", "/a");
        let id2 = s.add_entry("B", "P", "/b");
        s.record_success(id1, 1000, 300);
        s.record_success(id2, 1000, 500);
        assert_eq!(s.total_impact_ms(), 800);
    }

    #[test]
    fn test_total_impact_excludes_disabled() {
        let mut s = StartupSettings::new();
        let id1 = s.add_entry("A", "P", "/a");
        let id2 = s.add_entry("B", "P", "/b");
        s.record_success(id1, 1000, 300);
        s.record_success(id2, 1000, 500);
        s.disable(id2);
        assert_eq!(s.total_impact_ms(), 300);
    }

    #[test]
    fn test_sorted_by_name() {
        let mut s = StartupSettings::new();
        s.add_entry("Zapp", "P", "/z");
        s.add_entry("Alpha", "P", "/a");
        let sorted = s.sorted_entries(StartupSort::Name);
        assert_eq!(sorted[0].name, "Alpha");
        assert_eq!(sorted[1].name, "Zapp");
    }

    #[test]
    fn test_auto_disable_failing() {
        let mut s = StartupSettings::new();
        let id = s.add_entry("Bad", "P", "/bad");
        for _ in 0..5 {
            s.record_failure(id);
        }
        let disabled = s.auto_disable_failing();
        assert_eq!(disabled.len(), 1);
        assert!(!s.get_entry(id).unwrap().enabled);
    }

    #[test]
    fn test_auto_disable_off() {
        let mut s = StartupSettings::new();
        s.boot_config.auto_disable_failing = false;
        let id = s.add_entry("Bad", "P", "/bad");
        for _ in 0..10 {
            s.record_failure(id);
        }
        let disabled = s.auto_disable_failing();
        assert!(disabled.is_empty());
    }

    #[test]
    fn test_record_success() {
        let mut s = StartupSettings::new();
        let id = s.add_entry("App", "P", "/app");
        s.record_failure(id);
        s.record_failure(id);
        assert!(s.record_success(id, 5000, 250));
        let e = s.get_entry(id).unwrap();
        assert_eq!(e.failure_count, 0);
        assert_eq!(e.last_run_at, Some(5000));
        assert_eq!(e.startup_time_ms, Some(250));
        assert_eq!(e.impact, StartupImpact::Low);
    }

    #[test]
    fn test_record_failure_nonexistent() {
        let mut s = StartupSettings::new();
        assert!(!s.record_failure(999));
    }

    // ---- BootConfig ----

    #[test]
    fn test_boot_config_default() {
        let c = BootConfig::default();
        assert!(c.measure_boot_time);
        assert!(c.fast_startup);
        assert!(c.auto_disable_failing);
        assert_eq!(c.auto_disable_threshold, 5);
    }

    // ---- StartupSettingsUI ----

    #[test]
    fn test_ui_new() {
        let ui = StartupSettingsUI::new();
        assert_eq!(ui.active_tab, StartupTab::Apps);
        assert_eq!(ui.sort, StartupSort::Name);
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = StartupSettingsUI::new();
        ui.set_tab(StartupTab::Boot);
        assert_eq!(ui.active_tab, StartupTab::Boot);
    }

    #[test]
    fn test_ui_visible_entries_all() {
        let mut ui = StartupSettingsUI::new();
        ui.settings.add_entry("A", "P", "/a");
        ui.settings.add_entry("B", "P", "/b");
        assert_eq!(ui.visible_entries().len(), 2);
    }

    #[test]
    fn test_ui_visible_entries_filtered() {
        let mut ui = StartupSettingsUI::new();
        ui.settings.add_entry("Firefox", "Mozilla", "/firefox");
        ui.settings.add_entry("Thunderbird", "Mozilla", "/tb");
        ui.filter = "fire".to_string();
        assert_eq!(ui.visible_entries().len(), 1);
    }

    #[test]
    fn test_ui_visible_entries_enabled_only() {
        let mut ui = StartupSettingsUI::new();
        let id1 = ui.settings.add_entry("A", "P", "/a");
        ui.settings.add_entry("B", "P", "/b");
        ui.settings.disable(id1);
        ui.show_enabled_only = true;
        assert_eq!(ui.visible_entries().len(), 1);
    }

    #[test]
    fn test_ui_render_apps_tab() {
        let mut ui = StartupSettingsUI::new();
        ui.settings.add_entry("App", "Publisher", "/app");
        let cmds = ui.render(&Palette::for_mode(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_boot_tab() {
        let mut ui = StartupSettingsUI::new();
        ui.set_tab(StartupTab::Boot);
        ui.settings.boot_config.last_boot_time_ms = Some(8500);
        let cmds = ui.render(&Palette::for_mode(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_with_high_impact() {
        let mut ui = StartupSettingsUI::new();
        let id = ui.settings.add_entry("Heavy", "P", "/heavy");
        if let Some(e) = ui.settings.get_entry_mut(id) {
            e.impact = StartupImpact::High;
        }
        let cmds = ui.render(&Palette::for_mode(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_empty() {
        let ui = StartupSettingsUI::new();
        let cmds = ui.render(&Palette::for_mode(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    // ---- Tab labels ----

    #[test]
    fn test_tab_labels() {
        assert_eq!(StartupTab::Apps.label(), "Startup Apps");
        assert_eq!(StartupTab::Boot.label(), "Boot Performance");
    }

    // ---- Sort labels ----

    #[test]
    fn test_sort_labels() {
        assert_eq!(StartupSort::Name.label(), "Name");
        assert_eq!(StartupSort::Impact.label(), "Impact");
    }

    // ---- ID uniqueness ----

    #[test]
    fn test_id_uniqueness() {
        let mut s = StartupSettings::new();
        let id1 = s.add_entry("A", "P", "/a");
        let id2 = s.add_entry("B", "P", "/b");
        let id3 = s.add_entry("C", "P", "/c");
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
    }

    // ========================================================================
    // The palette conversion
    // ========================================================================

    /// A panel with one of everything on screen.
    ///
    /// `up` flips the boot toggles against each other and turns the selection
    /// on, so no `if` in the module keeps one arm unrendered across the two
    /// calls.
    fn wound(tab: StartupTab, up: bool) -> StartupSettingsUI {
        let mut ui = StartupSettingsUI::new();
        ui.active_tab = tab;
        ui.sort = StartupSort::Name;

        // One entry per impact band. `NotMeasured` has to be assigned by hand:
        // `from_millis` classifies a measurement and so can never return it —
        // but `StartupEntry::new` starts every entry there, which is what a
        // freshly-added app actually shows, so the grey badge is not a
        // hypothetical state.
        for (name, impact) in [
            ("Anvil", StartupImpact::None),
            ("Bellows", StartupImpact::Low),
            ("Crucible", StartupImpact::Medium),
            ("Drophammer", StartupImpact::High),
            ("Emery", StartupImpact::NotMeasured),
        ] {
            let id = ui.settings.add_entry(name, "Forge Ltd", "/bin/tool");
            if let Some(e) = ui.settings.get_entry_mut(id) {
                e.impact = impact;
                e.startup_time_ms = Some(1234);
            }
        }
        // Failing, so the failure badge is drawn.
        let bad = ui.settings.add_entry("Flux", "Forge Ltd", "/bin/flux");
        if let Some(e) = ui.settings.get_entry_mut(bad) {
            e.failure_count = 5;
        }
        // Delayed, so the delay line is drawn.
        let slow = ui.settings.add_entry("Grinder", "Forge Ltd", "/bin/grind");
        if let Some(e) = ui.settings.get_entry_mut(slow) {
            e.delay_ms = 300_000;
        }
        // Disabled, so the switch's off arm and the dimmed name are drawn.
        let off = ui.settings.add_entry("Hone", "Forge Ltd", "/bin/hone");
        ui.settings.disable(off);

        ui.selected_id = if up { Some(bad) } else { None };

        let cfg = &mut ui.settings.boot_config;
        // Deliberately disagreeing, so one render covers both switch arms.
        cfg.measure_boot_time = up;
        cfg.fast_startup = !up;
        cfg.auto_disable_failing = up;
        cfg.last_boot_time_ms = Some(if up { 40_000 } else { 5_000 });
        ui
    }

    /// Every state the panel can be in, so no branch escapes the sweep.
    fn every_state() -> Vec<(StartupSettingsUI, String)> {
        let mut out = Vec::new();
        for tab in [StartupTab::Apps, StartupTab::Boot] {
            for up in [false, true] {
                out.push((wound(tab, up), format!("startup panel ({tab:?}, up={up})")));
                // No high-impact app: the warning banner's other arm.
                let mut calm = wound(tab, up);
                for e in &mut calm.settings.entries {
                    if e.impact == StartupImpact::High {
                        e.impact = StartupImpact::Low;
                    }
                }
                out.push((calm, format!("startup panel ({tab:?}, up={up}, calm)")));
            }
            // Nothing registered at all: the "No startup apps" caption.
            let mut bare = StartupSettingsUI::new();
            bare.active_tab = tab;
            out.push((bare, format!("startup panel ({tab:?}, nothing registered)")));
        }
        // A filter that matches nothing draws the same caption over a *typed*
        // filter field, which is a different colour from the placeholder.
        let mut filtered = wound(StartupTab::Apps, true);
        filtered.filter = "no-such-app".to_string();
        out.push((
            filtered,
            "startup panel (Apps, filtered to nothing)".to_string(),
        ));
        // Enabled-only with everything off is a third route to the caption.
        let mut hidden = wound(StartupTab::Apps, false);
        for e in &mut hidden.settings.entries {
            e.enabled = false;
        }
        hidden.show_enabled_only = true;
        out.push((hidden, "startup panel (Apps, all hidden)".to_string()));
        // Every boot-time band, plus the reading being absent entirely.
        for ms in [None, Some(5_000), Some(20_000), Some(40_000)] {
            let mut boot = wound(StartupTab::Boot, true);
            boot.settings.boot_config.last_boot_time_ms = ms;
            out.push((boot, format!("startup panel (Boot, last={ms:?})")));
        }
        out
    }

    fn render(ui: &StartupSettingsUI, p: &Palette) -> Vec<RenderCommand> {
        ui.render(p, 600.0, 800.0)
    }

    /// The membership sweep: nothing the panel draws is outside its palette.
    ///
    /// Every constant this module used to hold was a Catppuccin *Mocha* value,
    /// so the light render is where a survivor gives itself away — Latte does
    /// not contain it, and the failure names the colour back.
    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            // The three computed inks named in the module header: the active
            // tab's label, the impact badge's label on each of its four
            // categorical fills, and the failure badge's on red. Written out
            // rather than read back from `StartupImpact::color`, so this is a
            // claim about the design and not an echo of the code under test.
            let ink = [
                p.on_accent(),
                readable_on(p.green),
                readable_on(p.yellow),
                readable_on(p.red),
                readable_on(p.overlay0),
            ];
            for (ui, what) in every_state() {
                assert_drawn_from(
                    &p,
                    &render(&ui, &p),
                    &ink,
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
            for (ui, what) in every_state() {
                assert_nothing_is_drawn_and_never_seen(
                    &render(&ui, &p),
                    &format!("{what}, light={light}"),
                );
            }
        }
    }

    // ---- Extractors, one per class of control the accent is meant to reach --

    /// The tab strip's pills, in draw order.
    ///
    /// Keyed on geometry and not on the label, because the panel *title* is
    /// also the string `"Startup Apps"`: a "last fill before this text" helper
    /// would have returned the backdrop and the title, and would have done so
    /// silently. The strip is the only thing 32 tall in the module (grepped:
    /// one `height: 32.0` in the whole file), and its labels are the only text
    /// at y 80 — every tab body starts at y 120.
    fn tab_pills(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    height: 32.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The tab strip's labels, in the same order as [`tab_pills`].
    fn tab_labels(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y: 80.0,
                    font_size: 13.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The per-entry enable switches: the only 36x20 fills in the module.
    ///
    /// The knob is 16x16 and the impact badge is 74x20, so neither the width
    /// nor the height alone would do — both bounds are load-bearing.
    fn entry_switches(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width: 36.0,
                    height: 20.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The boot tab's toggle switches: the only 40x22 fills in the module.
    fn boot_switches(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width: 40.0,
                    height: 22.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every `(fill, label)` pair among the fills that are exactly `w` by `h`.
    ///
    /// Both badges draw their label as the very next command after their own
    /// fill, so the pair is well defined. Resetting `pending` on a
    /// *non-matching* fill is what stops an unrelated caption being paired
    /// with a badge drawn earlier in the row.
    fn badges(cmds: &[RenderCommand], w: f32, h: f32) -> Vec<(Color, Color)> {
        let mut out = Vec::new();
        let mut pending: Option<Color> = None;
        for c in cmds {
            match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } => {
                    pending = if *width == w && *height == h {
                        Some(*color)
                    } else {
                        None
                    };
                }
                RenderCommand::Text { color, .. } => {
                    if let Some(fill) = pending.take() {
                        out.push((fill, *color));
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Every colour the panel draws that no control above claimed.
    ///
    /// This is the frozen half: an `assert_eq!` over it fails if *anything*
    /// that is not one of the three named controls moves with the accent,
    /// including sites nobody thought to name.
    ///
    /// The tab strip's *labels* are deliberately kept. They are
    /// `p.on_accent()`, which is `readable_on` of the accent, and both accents
    /// the test below uses are pale enough to resolve to the same near-black —
    /// so a correct label is frozen between them and a label wrongly painted
    /// with the accent itself is caught here. That is real coverage, but it
    /// does rest on the two accents sharing a lightness band: if a dark accent
    /// is ever added to that pair, the labels must move into this exclusion
    /// list, and the check that separates `p.on_accent()` from a hard-coded
    /// near-black is [`each_label_is_legible_on_the_fill_beneath_it`], not
    /// this one.
    fn colors_apart_from_the_controls(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter(|c| {
                !matches!(
                    c,
                    RenderCommand::FillRect { height: 32.0, .. }
                        | RenderCommand::FillRect {
                            width: 36.0,
                            height: 20.0,
                            ..
                        }
                        | RenderCommand::FillRect {
                            width: 40.0,
                            height: 22.0,
                            ..
                        }
                )
            })
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every control that offers something follows the accent — each proved
    /// separately.
    ///
    /// Three source sites means three `assert_ne!`s. Over their union one
    /// moving control would hide the other two, which is the failure the
    /// earlier modules in this conversion established. (The boot tab's three
    /// switches are three *rendered* controls but one call site,
    /// [`StartupSettingsUI::render_toggle_row`], so they are one assertion —
    /// and the length check beside it is what proves all three were found.)
    #[test]
    fn every_control_that_offers_something_follows_the_accent() {
        let mut a = Palette::for_mode(false);
        a.accent = appearance::MAUVE;
        let mut b = Palette::for_mode(false);
        b.accent = appearance::TEAL;

        // Walk both tabs as the active one: the pill's colour is chosen by a
        // boolean, so a fixture pinned to one tab leaves the other unproven.
        for tab in [StartupTab::Apps, StartupTab::Boot] {
            let ui = wound(tab, true);
            let x = render(&ui, &a);
            let y = render(&ui, &b);

            assert_eq!(tab_pills(&x).len(), 2, "two tabs have pills");
            assert_ne!(
                tab_pills(&x),
                tab_pills(&y),
                "the active tab's pill did not move with the accent (tab={tab:?})"
            );

            match tab {
                StartupTab::Apps => {
                    assert_eq!(entry_switches(&x).len(), 8, "eight entries are listed");
                    assert_ne!(
                        entry_switches(&x),
                        entry_switches(&y),
                        "an entry's enable switch did not move with the accent"
                    );
                    assert!(boot_switches(&x).is_empty(), "the boot tab is not drawn");
                }
                StartupTab::Boot => {
                    assert_eq!(boot_switches(&x).len(), 3, "three boot toggles");
                    assert_ne!(
                        boot_switches(&x),
                        boot_switches(&y),
                        "a boot-tab switch did not move with the accent"
                    );
                    assert!(entry_switches(&x).is_empty(), "the apps list is not drawn");
                }
            }

            assert_eq!(
                colors_apart_from_the_controls(&x),
                colors_apart_from_the_controls(&y),
                "something that is not a control moved with the accent \
                 (tab={tab:?}) — an app's impact, a boot-time reading and a \
                 failure count are all facts about the machine, and a fact \
                 read against its neighbours down a list must not be the \
                 accent"
            );
        }
    }

    /// The panel's own surfaces are the palette's, in both modes.
    ///
    /// This is what the membership sweep structurally cannot check. The sweep
    /// declares this module's `readable_on` inks as derived, and a declaration
    /// is a claim about a *value* rather than about where that value may
    /// appear — so `0x11111B`, which is both one of those inks and Mocha's
    /// `crust`, would pass the sweep wherever it turned up, including in the
    /// place a role belongs.
    ///
    /// Membership is the wrong question for a surface anyway. These are not
    /// "some palette colour", they are one specific role each, so the test
    /// names the role and asserts equality — which also fails in *dark* mode
    /// if the role is wrong, where a membership check could only ever fail in
    /// light.
    #[test]
    fn the_panels_own_surfaces_come_from_the_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);

            let apps = render(&wound(StartupTab::Apps, true), &p);
            let backdrop = apps.iter().find_map(|c| match c {
                RenderCommand::FillRect {
                    x: 0.0,
                    y: 0.0,
                    width: 600.0,
                    height: 800.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            });
            assert_eq!(
                backdrop,
                Some(p.base),
                "the panel's backdrop is not p.base (light={light})"
            );

            let filter_bar = apps.iter().find_map(|c| match c {
                RenderCommand::FillRect {
                    height: 30.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            });
            assert_eq!(
                filter_bar,
                Some(p.surface0),
                "the filter field is not p.surface0 (light={light})"
            );

            // The entry rows: `Flux` is selected in this fixture, and the list
            // is sorted by name, so it is the sixth of eight.
            let rows: Vec<Color> = apps
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect {
                        height: 56.0,
                        color,
                        ..
                    } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(rows.len(), 8, "eight entry rows (light={light})");
            assert_eq!(
                rows[5], p.surface1,
                "the selected entry's row is not p.surface1 (light={light})"
            );
            for (i, row) in rows.iter().enumerate().filter(|(i, _)| *i != 5) {
                assert_eq!(
                    *row, p.surface0,
                    "entry row {i} is not p.surface0 (light={light})"
                );
            }

            let boot = render(&wound(StartupTab::Boot, true), &p);
            let card = boot.iter().find_map(|c| match c {
                RenderCommand::FillRect {
                    height: 48.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            });
            assert_eq!(
                card,
                Some(p.surface0),
                "the last-boot-time card is not p.surface0 (light={light})"
            );
        }
    }

    /// Each label that sits on a coloured fill is picked *for that fill*.
    ///
    /// The accent test above cannot reach this. Every accent on offer is pale,
    /// so [`appearance::readable_on`] answers the same near-black for all of
    /// them, and an `assert_ne!` between two accents would fail on correct
    /// code. What separates a chosen label from a hard-coded `p.crust` is the
    /// *mode*: Latte's `crust` is near-white, which on a pale accent is
    /// illegible.
    ///
    /// The impact badge is here for a stronger reason than the tab label: its
    /// label was a hard-coded near-black *and that was already wrong*, not
    /// merely fragile. `NotMeasured` fills the badge with `overlay0`, a mid
    /// grey, and near-black on mid grey is poor contrast in dark mode and no
    /// better in light. The other four arms were legible only by coincidence
    /// of the two palettes — Mocha's green/yellow/red are pale and Latte's are
    /// deep, so a fixed dark label happens to read on both. Nobody maintains
    /// that coincidence; `readable_on` of the badge's own fill makes it a
    /// property.
    #[test]
    fn each_label_is_legible_on_the_fill_beneath_it() {
        for light in [false, true] {
            for accent in [
                appearance::BLUE,
                appearance::GREEN,
                appearance::RED,
                appearance::YELLOW,
                appearance::MAUVE,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let what = format!("light={light}, accent={accent:?}");

                // The active tab's pill is the accent, its label chosen for it.
                let cmds = render(&wound(StartupTab::Apps, true), &p);
                let pills = tab_pills(&cmds);
                let labels = tab_labels(&cmds);
                assert_eq!(labels.len(), 2, "two tabs are labelled ({what})");
                let want = readable_on(accent);
                assert_eq!(
                    (pills[0].r, pills[0].g, pills[0].b),
                    (accent.r, accent.g, accent.b),
                    "the active tab's pill is not the accent ({what})"
                );
                assert_eq!(
                    (labels[0].r, labels[0].g, labels[0].b),
                    (want.r, want.g, want.b),
                    "the active tab's label is not chosen for its own fill \
                     ({what}); a fixed colour is legible on one mode's \
                     accents and not the other's"
                );

                // Every impact badge, including the grey one.
                let impacts = badges(&cmds, 74.0, 20.0);
                assert_eq!(impacts.len(), 8, "eight impact badges ({what})");
                for (fill, label) in &impacts {
                    let want_badge = readable_on(*fill);
                    assert_eq!(
                        (label.r, label.g, label.b),
                        (want_badge.r, want_badge.g, want_badge.b),
                        "an impact badge's label is not chosen for its own \
                         fill ({what})"
                    );
                }
                // Named explicitly, because the grey arm is the one a fixed
                // near-black label was actually illegible on.
                assert!(
                    impacts.iter().any(|(fill, _)| (fill.r, fill.g, fill.b)
                        == (p.overlay0.r, p.overlay0.g, p.overlay0.b)),
                    "no unmeasured entry drew a grey badge ({what})"
                );

                // The failure badge: a categorical red fill, same rule.
                let fails = badges(&cmds, 74.0, 16.0);
                assert_eq!(fails.len(), 1, "one failing entry ({what})");
                let (fill, label) = fails[0];
                assert_eq!(
                    (fill.r, fill.g, fill.b),
                    (p.red.r, p.red.g, p.red.b),
                    "the failure badge is not p.red ({what})"
                );
                let want_red = readable_on(p.red);
                assert_eq!(
                    (label.r, label.g, label.b),
                    (want_red.r, want_red.g, want_red.b),
                    "the failure badge's label is not chosen for its own fill \
                     ({what})"
                );
            }
        }
    }

    /// A measurement is not the accent — proved by moving the accent onto it.
    ///
    /// The distinctness test below is necessary but not sufficient: a scale
    /// whose values were *all* rewritten to `p.accent` would collapse and be
    /// caught, but a scale where only one value became the accent would still
    /// be pairwise-distinct under most accents. This asserts the stronger
    /// property directly — every one of these is the same under two different
    /// accents, because none of them is the accent.
    #[test]
    fn no_category_follows_the_accent() {
        let mut a = Palette::for_mode(false);
        a.accent = appearance::MAUVE;
        let mut b = Palette::for_mode(false);
        b.accent = appearance::TEAL;

        for i in [
            StartupImpact::None,
            StartupImpact::Low,
            StartupImpact::Medium,
            StartupImpact::High,
            StartupImpact::NotMeasured,
        ] {
            assert_eq!(i.color(&a), i.color(&b), "{i:?} impact follows the accent");
        }
        for ms in [0, 5_000, 20_000, 40_000, 600_000] {
            assert_eq!(
                boot_time_color(ms, &a),
                boot_time_color(ms, &b),
                "a {ms}ms boot reading follows the accent"
            );
        }
    }

    /// Every categorical scale stays tellable apart, under every accent and in
    /// both modes.
    ///
    /// The impact badges are drawn down a list, one per row, so two values
    /// sharing a colour do not merely confuse a learnt code — they make a
    /// high-impact app look like a harmless one in the same glance. Several of
    /// the hues involved are themselves selectable accents, which is why the
    /// accent has to be varied and not merely defaulted.
    ///
    /// Asserted over **bands, not variants**: `None` and `Low` are one band
    /// and deliberately share green. Walking the variants pairwise would fail
    /// on correct code.
    #[test]
    fn every_category_stays_distinct_under_every_accent() {
        for light in [false, true] {
            for accent in [
                appearance::BLUE,
                appearance::GREEN,
                appearance::RED,
                appearance::YELLOW,
                appearance::PEACH,
                appearance::MAUVE,
                appearance::TEAL,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let what = format!("light={light}, accent={accent:?}");

                // One member of each impact band.
                let bands = [
                    StartupImpact::Low,
                    StartupImpact::Medium,
                    StartupImpact::High,
                    StartupImpact::NotMeasured,
                ];
                for (i, b1) in bands.iter().enumerate() {
                    for b2 in bands.iter().skip(i + 1) {
                        assert_ne!(
                            b1.color(&p),
                            b2.color(&p),
                            "{b1:?} and {b2:?} impact are the same colour ({what})"
                        );
                    }
                }

                // The boot-time ladder's three bands.
                let readings = [5_000, 20_000, 40_000];
                for (i, m1) in readings.iter().enumerate() {
                    for m2 in readings.iter().skip(i + 1) {
                        assert_ne!(
                            boot_time_color(*m1, &p),
                            boot_time_color(*m2, &p),
                            "{m1}ms and {m2}ms boots are the same colour ({what})"
                        );
                    }
                }
            }
        }
    }

    /// `None` and `Low` share a colour on purpose.
    ///
    /// Written down so that a future reader who notices five labels over four
    /// colours does not "fix" it. The badge is a three-band traffic light —
    /// fine, slow, bad — plus grey for a reading that does not exist yet; the
    /// label is finer-grained than the light because "None" and "Low" are both
    /// answers a user does not need to act on.
    #[test]
    fn the_impact_light_has_fewer_bands_than_the_impact_label() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            assert_eq!(
                StartupImpact::None.color(&p),
                StartupImpact::Low.color(&p),
                "None and Low are deliberately one band (light={light})"
            );
            assert_ne!(
                StartupImpact::None.label(),
                StartupImpact::Low.label(),
                "…but they are still two labels"
            );
        }
    }

    /// The boot-time bands are exactly where the doc comment says.
    ///
    /// The ladder used to be three arms of an `if` buried inside a
    /// `RenderCommand::Text`, unreachable from a test without rendering the
    /// whole tab and hunting for a string. A measurement scale is precisely
    /// the kind of thing that needs a boundary test, which is why it now has a
    /// name.
    #[test]
    fn the_boot_time_bands_are_where_they_say_they_are() {
        let p = Palette::for_mode(false);
        assert_eq!(boot_time_color(0, &p), p.green);
        assert_eq!(boot_time_color(9_999, &p), p.green);
        assert_eq!(boot_time_color(10_000, &p), p.yellow);
        assert_eq!(boot_time_color(29_999, &p), p.yellow);
        assert_eq!(boot_time_color(30_000, &p), p.red);
        assert_eq!(boot_time_color(u64::MAX, &p), p.red);
    }

    /// The high-impact warning is the red it warns about, made translucent.
    ///
    /// The banner was `Color::rgba(RED.r, RED.g, RED.b, 40)` — a Mocha hex
    /// destructured by hand. The membership sweep compares RGB and ignores
    /// alpha by design, so this is the test that says the RGB is a *role* and
    /// the alpha is not.
    #[test]
    fn the_high_impact_warning_is_the_red_it_warns_about() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let cmds = render(&wound(StartupTab::Apps, true), &p);
            let wash = cmds.iter().find_map(|c| match c {
                RenderCommand::FillRect {
                    height: 28.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            });
            let wash = wash.expect("a high-impact app draws the warning banner");
            assert_eq!(
                (wash.r, wash.g, wash.b),
                (p.red.r, p.red.g, p.red.b),
                "the warning banner is not p.red underneath its alpha \
                 (light={light})"
            );
            assert_eq!(
                wash.a, 40,
                "the warning banner is not a wash (light={light})"
            );
        }
    }
}

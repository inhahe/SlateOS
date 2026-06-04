//! Startup and autorun settings panel for the desktop shell.
//!
//! Manages applications that run automatically at login, including
//! startup delay, impact assessment, and per-app enable/disable control.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

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

    fn color(self) -> Color {
        match self {
            Self::None => GREEN,
            Self::Low => GREEN,
            Self::Medium => YELLOW,
            Self::High => RED,
            Self::NotMeasured => OVERLAY0,
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
    pub fn delay_text(&self) -> String {
        if self.delay_ms == 0 {
            "Immediate".to_string()
        } else if self.delay_ms < 1000 {
            format!("{}ms", self.delay_ms)
        } else {
            format!("{:.1}s", self.delay_ms as f64 / 1000.0)
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
    /// Next ID for new entries.
    next_id: u64,
}

impl StartupSettings {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            boot_config: BootConfig::default(),
            next_id: 1,
        }
    }

    /// Add a new startup entry. Returns its ID.
    pub fn add_entry(
        &mut self,
        name: impl Into<String>,
        publisher: impl Into<String>,
        command: impl Into<String>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.entries.push(StartupEntry::new(id, name, publisher, command));
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
        self.entries.iter().filter(|e| e.impact == StartupImpact::High && e.enabled).collect()
    }

    /// Entries that are failing.
    pub fn failing_entries(&self) -> Vec<&StartupEntry> {
        self.entries.iter().filter(|e| e.is_failing()).collect()
    }

    /// Total estimated startup impact in milliseconds (enabled entries only).
    pub fn total_impact_ms(&self) -> u64 {
        self.entries.iter()
            .filter(|e| e.enabled)
            .filter_map(|e| e.startup_time_ms)
            .sum()
    }

    /// Get entries sorted by the given criteria.
    pub fn sorted_entries(&self, sort: StartupSort) -> Vec<&StartupEntry> {
        let mut entries: Vec<_> = self.entries.iter().collect();
        match sort {
            StartupSort::Name => entries.sort_by_key(|a| a.name.to_lowercase()),
            StartupSort::Impact => entries.sort_by(|a, b| b.impact.cmp(&a.impact)),
            StartupSort::StartupType => entries.sort_by(|a, b| a.startup_type.label().cmp(b.startup_type.label())),
            StartupSort::Status => entries.sort_by(|a, b| b.enabled.cmp(&a.enabled)),
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

// ============================================================================
// UI
// ============================================================================

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
        let mut entries: Vec<_> = self.settings.entries.iter()
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
            StartupSort::Impact => entries.sort_by(|a, b| b.impact.cmp(&a.impact)),
            StartupSort::StartupType => entries.sort_by(|a, b| a.startup_type.label().cmp(b.startup_type.label())),
            StartupSort::Status => entries.sort_by(|a, b| b.enabled.cmp(&a.enabled)),
        }

        entries
    }

    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 24.0,
            text: "Startup Apps".into(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 48.0),
        });

        // Tabs
        let tabs = [StartupTab::Apps, StartupTab::Boot];
        let tab_y = 72.0;
        let mut tx = 24.0;
        for &tab in &tabs {
            let active = tab == self.active_tab;
            let tw = tab.label().len() as f32 * 8.0 + 20.0;
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: tab_y,
                width: tw,
                height: 32.0,
                color: if active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: tab_y + 8.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if active { CRUST } else { SUBTEXT0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tw - 20.0),
            });
            tx += tw + 8.0;
        }

        let cy = tab_y + 48.0;
        let cw = width - 48.0;

        match self.active_tab {
            StartupTab::Apps => self.render_apps_tab(&mut cmds, 24.0, cy, cw, height - cy - 16.0),
            StartupTab::Boot => self.render_boot_tab(&mut cmds, 24.0, cy, cw),
        }

        cmds
    }

    fn render_apps_tab(
        &self,
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
            color: SURFACE0,
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
            color: if self.filter.is_empty() { OVERLAY0 } else { TEXT },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 20.0),
        });
        cy += 40.0;

        // Sort indicator
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!("Sort: {}", self.sort.label()),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
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
                color: Color::rgba(RED.r, RED.g, RED.b, 40),
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 6.0,
                text: format!("{} high-impact apps slowing your startup", high_impact.len()),
                font_size: 12.0,
                color: RED,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 20.0),
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
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
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
                color: if is_selected { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });

            // Enable/disable toggle
            let toggle_color = if entry.enabled { GREEN } else { SURFACE2 };
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
                color: TEXT,
                corner_radii: CornerRadii::all(8.0),
            });

            // Name
            cmds.push(RenderCommand::Text {
                x: x + 52.0,
                y: cy + 6.0,
                text: entry.name.clone(),
                font_size: 14.0,
                color: if entry.enabled { TEXT } else { OVERLAY0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(width * 0.5),
            });

            // Publisher and type
            cmds.push(RenderCommand::Text {
                x: x + 52.0,
                y: cy + 24.0,
                text: format!("{} - {}", entry.publisher, entry.startup_type.label()),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.5),
            });

            // Delay
            if entry.has_delay() {
                cmds.push(RenderCommand::Text {
                    x: x + 52.0,
                    y: cy + 40.0,
                    text: format!("Delay: {}", entry.delay_text()),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                });
            }

            // Impact badge
            let impact_color = entry.impact.color();
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
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(64.0),
            });

            // Failure indicator
            if entry.is_failing() {
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 90.0,
                    y: cy + 34.0,
                    width: 74.0,
                    height: 16.0,
                    color: RED,
                    corner_radii: CornerRadii::all(8.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + width - 84.0,
                    y: cy + 36.0,
                    text: format!("{} fails", entry.failure_count),
                    font_size: 10.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(64.0),
                });
            }

            cy += 62.0;
        }
    }

    fn render_boot_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;
        let cfg = &self.settings.boot_config;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Boot Performance".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        // Last boot time
        if let Some(ms) = cfg.last_boot_time_ms {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 48.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 6.0,
                text: "Last Boot Time".into(),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 24.0,
                text: format!("{:.1}s", ms as f64 / 1000.0),
                font_size: 18.0,
                color: if ms < 10000 { GREEN } else if ms < 30000 { YELLOW } else { RED },
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
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
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });
        cy += 28.0;

        // Toggle rows
        self.render_toggle_row(cmds, x, cy, width, "Measure Boot Time", cfg.measure_boot_time);
        cy += 36.0;

        self.render_toggle_row(cmds, x, cy, width, "Fast Startup", cfg.fast_startup);
        cy += 36.0;

        self.render_toggle_row(cmds, x, cy, width, "Auto-disable Failing Apps", cfg.auto_disable_failing);
        cy += 36.0;

        // Thresholds
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Thresholds".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(
            cmds, x, cy, width,
            "Max Startup Delay",
            &format!("{:.0}s", cfg.max_startup_delay_ms as f64 / 1000.0),
        );
        cy += 28.0;

        self.render_label_value(
            cmds, x, cy, width,
            "Fail Threshold",
            &format!("{} consecutive failures", cfg.auto_disable_threshold),
        );
        let _ = cy;
    }

    fn render_toggle_row(
        &self,
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
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 80.0),
        });
        let sw_x = x + width - 44.0;
        cmds.push(RenderCommand::FillRect {
            x: sw_x,
            y: y + 2.0,
            width: 40.0,
            height: 22.0,
            color: if enabled { GREEN } else { SURFACE2 },
            corner_radii: CornerRadii::all(11.0),
        });
        let knob_x = if enabled { sw_x + 20.0 } else { sw_x + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 4.0,
            width: 18.0,
            height: 18.0,
            color: TEXT,
            corner_radii: CornerRadii::all(9.0),
        });
    }

    fn render_label_value(
        &self,
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55,
            y,
            text: value.into(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.45),
        });
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_boot_tab() {
        let mut ui = StartupSettingsUI::new();
        ui.set_tab(StartupTab::Boot);
        ui.settings.boot_config.last_boot_time_ms = Some(8500);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_with_high_impact() {
        let mut ui = StartupSettingsUI::new();
        let id = ui.settings.add_entry("Heavy", "P", "/heavy");
        if let Some(e) = ui.settings.get_entry_mut(id) {
            e.impact = StartupImpact::High;
        }
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_empty() {
        let ui = StartupSettingsUI::new();
        let cmds = ui.render(600.0, 800.0);
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
}

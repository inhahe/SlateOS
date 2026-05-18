//! Window Rules Engine
//!
//! Allows users to define rules that automatically apply window behavior
//! when windows are created or focused. Rules match by window title, process
//! name, or window class, and can control:
//!
//! - Initial position and size
//! - Virtual desktop assignment
//! - Always-on-top / always-on-bottom
//! - Start minimized / maximized / fullscreen
//! - Opacity / transparency
//! - Skip taskbar / skip alt-tab
//! - Force-assign to specific monitor
//! - Custom title bar visibility
//!
//! Rules are evaluated in priority order; first match wins (unless
//! `apply_all` is set, in which case all matching rules are merged).

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_SURFACE2: Color = Color::from_hex(0x585B70);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_YELLOW: Color = Color::from_hex(0xF9E2AF);
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Rule matching criteria
// ============================================================================

/// How a rule matches against window properties.
#[derive(Clone, Debug, PartialEq)]
pub enum MatchCriteria {
    /// Match window title exactly.
    TitleExact(String),
    /// Match if window title contains this substring (case-insensitive).
    TitleContains(String),
    /// Match against process/executable name (case-insensitive).
    ProcessName(String),
    /// Match by window class string.
    WindowClass(String),
    /// Match any window (used for global defaults).
    Any,
}

impl MatchCriteria {
    /// Test whether a window matches this criterion.
    pub fn matches(&self, title: &str, process: &str, class: &str) -> bool {
        match self {
            Self::TitleExact(t) => title == t,
            Self::TitleContains(sub) => {
                let lower_title = title.to_lowercase();
                let lower_sub = sub.to_lowercase();
                lower_title.contains(&lower_sub)
            }
            Self::ProcessName(name) => {
                process.eq_ignore_ascii_case(name)
            }
            Self::WindowClass(cls) => {
                class.eq_ignore_ascii_case(cls)
            }
            Self::Any => true,
        }
    }

    /// Human-readable description of this criterion.
    pub fn description(&self) -> String {
        match self {
            Self::TitleExact(t) => format!("Title = \"{}\"", t),
            Self::TitleContains(s) => format!("Title contains \"{}\"", s),
            Self::ProcessName(n) => format!("Process: {}", n),
            Self::WindowClass(c) => format!("Class: {}", c),
            Self::Any => "Any window".to_string(),
        }
    }
}

// ============================================================================
// Rule actions
// ============================================================================

/// Position specification for a window rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PositionSpec {
    /// Absolute pixel coordinates from top-left of primary monitor.
    Absolute { x: i32, y: i32 },
    /// Center on the specified monitor (0-based index).
    CenterOnMonitor(u32),
    /// Percentage of screen dimensions (0.0-1.0 for x, y).
    Percentage { x_pct: f32, y_pct: f32 },
    /// Remember last position for this window.
    RememberLast,
}

/// Size specification for a window rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SizeSpec {
    /// Exact pixel dimensions.
    Exact { width: u32, height: u32 },
    /// Percentage of screen dimensions.
    Percentage { w_pct: f32, h_pct: f32 },
    /// Remember last size.
    RememberLast,
}

/// Initial window state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialState {
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
}

/// Actions to apply when a rule matches.
#[derive(Clone, Debug)]
pub struct RuleActions {
    /// Override initial position.
    pub position: Option<PositionSpec>,
    /// Override initial size.
    pub size: Option<SizeSpec>,
    /// Assign to a specific virtual desktop (0-based).
    pub desktop: Option<u32>,
    /// Force always-on-top.
    pub always_on_top: Option<bool>,
    /// Force always-on-bottom (desktop-level).
    pub always_on_bottom: Option<bool>,
    /// Initial window state override.
    pub initial_state: Option<InitialState>,
    /// Custom opacity (0.0 = invisible, 1.0 = fully opaque).
    pub opacity: Option<f32>,
    /// Hide from taskbar.
    pub skip_taskbar: Option<bool>,
    /// Hide from Alt+Tab switcher.
    pub skip_alt_tab: Option<bool>,
    /// Force to specific monitor (0-based index).
    pub target_monitor: Option<u32>,
    /// Disable window decorations (title bar).
    pub no_decorations: Option<bool>,
    /// Minimum size constraint.
    pub min_size: Option<(u32, u32)>,
    /// Maximum size constraint.
    pub max_size: Option<(u32, u32)>,
    /// Prevent the window from being closed by the user.
    pub prevent_close: Option<bool>,
    /// Prevent the window from being moved.
    pub prevent_move: Option<bool>,
    /// Prevent the window from being resized.
    pub prevent_resize: Option<bool>,
    /// Custom snap zone override (snap layout preset index).
    pub snap_zone: Option<u32>,
}

impl RuleActions {
    /// Create empty actions (no overrides).
    pub fn new() -> Self {
        Self {
            position: None,
            size: None,
            desktop: None,
            always_on_top: None,
            always_on_bottom: None,
            initial_state: None,
            opacity: None,
            skip_taskbar: None,
            skip_alt_tab: None,
            target_monitor: None,
            no_decorations: None,
            min_size: None,
            max_size: None,
            prevent_close: None,
            prevent_move: None,
            prevent_resize: None,
            snap_zone: None,
        }
    }

    /// Merge another set of actions on top of this one (other's values
    /// take precedence where set).
    pub fn merge(&mut self, other: &Self) {
        if other.position.is_some() { self.position = other.position.clone(); }
        if other.size.is_some() { self.size = other.size; }
        if other.desktop.is_some() { self.desktop = other.desktop; }
        if other.always_on_top.is_some() { self.always_on_top = other.always_on_top; }
        if other.always_on_bottom.is_some() { self.always_on_bottom = other.always_on_bottom; }
        if other.initial_state.is_some() { self.initial_state = other.initial_state; }
        if other.opacity.is_some() { self.opacity = other.opacity; }
        if other.skip_taskbar.is_some() { self.skip_taskbar = other.skip_taskbar; }
        if other.skip_alt_tab.is_some() { self.skip_alt_tab = other.skip_alt_tab; }
        if other.target_monitor.is_some() { self.target_monitor = other.target_monitor; }
        if other.no_decorations.is_some() { self.no_decorations = other.no_decorations; }
        if other.min_size.is_some() { self.min_size = other.min_size; }
        if other.max_size.is_some() { self.max_size = other.max_size; }
        if other.prevent_close.is_some() { self.prevent_close = other.prevent_close; }
        if other.prevent_move.is_some() { self.prevent_move = other.prevent_move; }
        if other.prevent_resize.is_some() { self.prevent_resize = other.prevent_resize; }
        if other.snap_zone.is_some() { self.snap_zone = other.snap_zone; }
    }

    /// Count how many actions are actively set.
    pub fn active_count(&self) -> usize {
        let mut n = 0;
        if self.position.is_some() { n += 1; }
        if self.size.is_some() { n += 1; }
        if self.desktop.is_some() { n += 1; }
        if self.always_on_top.is_some() { n += 1; }
        if self.always_on_bottom.is_some() { n += 1; }
        if self.initial_state.is_some() { n += 1; }
        if self.opacity.is_some() { n += 1; }
        if self.skip_taskbar.is_some() { n += 1; }
        if self.skip_alt_tab.is_some() { n += 1; }
        if self.target_monitor.is_some() { n += 1; }
        if self.no_decorations.is_some() { n += 1; }
        if self.min_size.is_some() { n += 1; }
        if self.max_size.is_some() { n += 1; }
        if self.prevent_close.is_some() { n += 1; }
        if self.prevent_move.is_some() { n += 1; }
        if self.prevent_resize.is_some() { n += 1; }
        if self.snap_zone.is_some() { n += 1; }
        n
    }
}

impl Default for RuleActions {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Window Rule
// ============================================================================

/// A window rule: a match criterion plus the actions to take.
#[derive(Clone, Debug)]
pub struct WindowRule {
    /// Unique rule identifier.
    pub id: u32,
    /// Human-readable name for this rule.
    pub name: String,
    /// Match criterion.
    pub criteria: MatchCriteria,
    /// Actions to apply.
    pub actions: RuleActions,
    /// Priority (higher = evaluated first).
    pub priority: i32,
    /// Whether this rule is currently enabled.
    pub enabled: bool,
    /// Whether this is a one-shot rule (removed after first match).
    pub one_shot: bool,
    /// How many times this rule has been applied.
    pub match_count: u64,
}

impl WindowRule {
    /// Create a new rule with the given name and criterion.
    pub fn new(id: u32, name: &str, criteria: MatchCriteria) -> Self {
        Self {
            id,
            name: name.to_string(),
            criteria,
            actions: RuleActions::new(),
            priority: 0,
            enabled: true,
            one_shot: false,
            match_count: 0,
        }
    }

    /// Check if this rule matches a window.
    pub fn matches(&self, title: &str, process: &str, class: &str) -> bool {
        self.enabled && self.criteria.matches(title, process, class)
    }
}

// ============================================================================
// Remembered window state
// ============================================================================

/// Remembered position/size for "RememberLast" specs.
#[derive(Clone, Debug)]
struct RememberedState {
    /// Key: process name or window class.
    key: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    /// Last updated timestamp (monotonic counter).
    last_updated: u64,
}

// ============================================================================
// Rule evaluation mode
// ============================================================================

/// How to evaluate multiple matching rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalMode {
    /// First matching rule wins (highest priority).
    FirstMatch,
    /// All matching rules are merged (highest priority overrides).
    MergeAll,
}

// ============================================================================
// Window Rules Manager
// ============================================================================

/// Maximum number of rules allowed.
const MAX_RULES: usize = 256;

/// Maximum remembered window states.
const MAX_REMEMBERED: usize = 128;

/// Manages window rules and their evaluation.
pub struct WindowRulesManager {
    rules: Vec<WindowRule>,
    next_id: u32,
    eval_mode: EvalMode,
    /// Remembered positions for RememberLast.
    remembered: Vec<RememberedState>,
    /// Monotonic counter for remembered state timestamps.
    timestamp_counter: u64,
}

impl WindowRulesManager {
    /// Create a new manager with default rules.
    pub fn new() -> Self {
        let mut mgr = Self {
            rules: Vec::new(),
            next_id: 1,
            eval_mode: EvalMode::FirstMatch,
            remembered: Vec::new(),
            timestamp_counter: 0,
        };
        mgr.add_default_rules();
        mgr
    }

    /// Add sensible default rules.
    fn add_default_rules(&mut self) {
        // Terminal windows: remember last position and size
        let mut terminal_rule = WindowRule::new(
            self.alloc_id(), "Terminal: remember position",
            MatchCriteria::ProcessName("terminal".to_string()),
        );
        terminal_rule.actions.position = Some(PositionSpec::RememberLast);
        terminal_rule.actions.size = Some(SizeSpec::RememberLast);
        terminal_rule.priority = 10;
        self.rules.push(terminal_rule);

        // Settings: always center on primary monitor
        let mut settings_rule = WindowRule::new(
            self.alloc_id(), "Settings: center on primary",
            MatchCriteria::ProcessName("settings".to_string()),
        );
        settings_rule.actions.position = Some(PositionSpec::CenterOnMonitor(0));
        settings_rule.priority = 10;
        self.rules.push(settings_rule);

        // Dialog windows: prevent resize
        let mut dialog_rule = WindowRule::new(
            self.alloc_id(), "Dialogs: no resize",
            MatchCriteria::WindowClass("dialog".to_string()),
        );
        dialog_rule.actions.prevent_resize = Some(true);
        dialog_rule.priority = 5;
        self.rules.push(dialog_rule);
    }

    /// Allocate the next unique rule ID.
    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Set the evaluation mode.
    pub fn set_eval_mode(&mut self, mode: EvalMode) {
        self.eval_mode = mode;
    }

    /// Get the current evaluation mode.
    pub fn eval_mode(&self) -> EvalMode {
        self.eval_mode
    }

    /// Add a new rule. Returns the rule ID, or None if at capacity.
    pub fn add_rule(&mut self, mut rule: WindowRule) -> Option<u32> {
        if self.rules.len() >= MAX_RULES {
            return None;
        }
        let id = self.alloc_id();
        rule.id = id;
        self.rules.push(rule);
        Some(id)
    }

    /// Remove a rule by ID. Returns true if found.
    pub fn remove_rule(&mut self, id: u32) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        self.rules.len() < before
    }

    /// Enable or disable a rule by ID.
    pub fn set_enabled(&mut self, id: u32, enabled: bool) -> bool {
        if let Some(r) = self.rules.iter_mut().find(|r| r.id == id) {
            r.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Get all rules (sorted by priority, highest first).
    pub fn rules(&self) -> Vec<&WindowRule> {
        let mut sorted: Vec<&WindowRule> = self.rules.iter().collect();
        sorted.sort_by(|a, b| b.priority.cmp(&a.priority));
        sorted
    }

    /// Get a rule by ID.
    pub fn rule_by_id(&self, id: u32) -> Option<&WindowRule> {
        self.rules.iter().find(|r| r.id == id)
    }

    /// Get a mutable rule by ID.
    pub fn rule_by_id_mut(&mut self, id: u32) -> Option<&mut WindowRule> {
        self.rules.iter_mut().find(|r| r.id == id)
    }

    /// Evaluate rules for a window and return the merged actions.
    pub fn evaluate(&mut self, title: &str, process: &str, class: &str) -> RuleActions {
        // Sort by priority (highest first).
        let mut indices: Vec<usize> = (0..self.rules.len()).collect();
        indices.sort_by(|&a, &b| self.rules[b].priority.cmp(&self.rules[a].priority));

        let mut result = RuleActions::new();
        let mut matched_any = false;
        let mut one_shot_removals: Vec<u32> = Vec::new();

        for &idx in &indices {
            let rule = &self.rules[idx];
            if rule.matches(title, process, class) {
                if !matched_any {
                    result = rule.actions.clone();
                    matched_any = true;
                } else if self.eval_mode == EvalMode::MergeAll {
                    result.merge(&rule.actions);
                }

                // Track match count (we'll update after the loop to avoid borrow issues).
                if rule.one_shot {
                    one_shot_removals.push(rule.id);
                }

                // In FirstMatch mode, stop after the first hit.
                if self.eval_mode == EvalMode::FirstMatch && matched_any {
                    // Update match count for this rule.
                    self.rules[idx].match_count = self.rules[idx].match_count.saturating_add(1);
                    break;
                }
            }
        }

        // Update match counts for MergeAll mode.
        if self.eval_mode == EvalMode::MergeAll {
            for &idx in &indices {
                if self.rules[idx].matches(title, process, class) {
                    self.rules[idx].match_count = self.rules[idx].match_count.saturating_add(1);
                }
            }
        }

        // Remove one-shot rules that fired.
        for id in one_shot_removals {
            self.remove_rule(id);
        }

        // Resolve RememberLast references.
        self.resolve_remembered(&mut result, process, class);

        result
    }

    /// Resolve RememberLast position/size from stored state.
    fn resolve_remembered(&self, actions: &mut RuleActions, process: &str, class: &str) {
        let key = if !process.is_empty() {
            process.to_lowercase()
        } else {
            class.to_lowercase()
        };

        if let Some(PositionSpec::RememberLast) = actions.position {
            if let Some(state) = self.remembered.iter().find(|s| s.key == key) {
                actions.position = Some(PositionSpec::Absolute {
                    x: state.x,
                    y: state.y,
                });
            } else {
                // No remembered state; fall back to no override.
                actions.position = None;
            }
        }

        if let Some(SizeSpec::RememberLast) = actions.size {
            if let Some(state) = self.remembered.iter().find(|s| s.key == key) {
                actions.size = Some(SizeSpec::Exact {
                    width: state.width,
                    height: state.height,
                });
            } else {
                actions.size = None;
            }
        }
    }

    /// Record a window's current position/size for "RememberLast" rules.
    pub fn remember_state(
        &mut self,
        process: &str,
        class: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) {
        let key = if !process.is_empty() {
            process.to_lowercase()
        } else {
            class.to_lowercase()
        };

        if key.is_empty() {
            return;
        }

        self.timestamp_counter = self.timestamp_counter.saturating_add(1);

        // Update existing entry or create new one.
        if let Some(state) = self.remembered.iter_mut().find(|s| s.key == key) {
            state.x = x;
            state.y = y;
            state.width = width;
            state.height = height;
            state.last_updated = self.timestamp_counter;
        } else {
            // Evict oldest if at capacity.
            if self.remembered.len() >= MAX_REMEMBERED {
                let oldest_idx = self.remembered
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, s)| s.last_updated)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                self.remembered.swap_remove(oldest_idx);
            }
            self.remembered.push(RememberedState {
                key,
                x,
                y,
                width,
                height,
                last_updated: self.timestamp_counter,
            });
        }
    }

    /// Get the number of active (enabled) rules.
    pub fn active_rule_count(&self) -> usize {
        self.rules.iter().filter(|r| r.enabled).count()
    }

    /// Get total rules count.
    pub fn total_rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Move a rule's priority up (increase by 1).
    pub fn increase_priority(&mut self, id: u32) -> bool {
        if let Some(r) = self.rules.iter_mut().find(|r| r.id == id) {
            r.priority = r.priority.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Move a rule's priority down (decrease by 1).
    pub fn decrease_priority(&mut self, id: u32) -> bool {
        if let Some(r) = self.rules.iter_mut().find(|r| r.id == id) {
            r.priority = r.priority.saturating_sub(1);
            true
        } else {
            false
        }
    }

    /// Duplicate a rule with a new ID.
    pub fn duplicate_rule(&mut self, id: u32) -> Option<u32> {
        let rule = self.rules.iter().find(|r| r.id == id)?.clone();
        let new_id = self.alloc_id();
        let mut new_rule = rule;
        new_rule.id = new_id;
        new_rule.name = format!("{} (copy)", new_rule.name);
        new_rule.match_count = 0;
        if self.rules.len() < MAX_RULES {
            self.rules.push(new_rule);
            Some(new_id)
        } else {
            None
        }
    }

    /// Export rules to a config string format.
    pub fn export_config(&self) -> String {
        let mut out = String::from("# Window Rules Configuration\n");
        for rule in &self.rules {
            out.push_str(&format!("rule|{}|{}|{}|{}|{}\n",
                rule.id,
                rule.name,
                rule.priority,
                if rule.enabled { "on" } else { "off" },
                match &rule.criteria {
                    MatchCriteria::TitleExact(t) => format!("title_exact:{}", t),
                    MatchCriteria::TitleContains(s) => format!("title_contains:{}", s),
                    MatchCriteria::ProcessName(n) => format!("process:{}", n),
                    MatchCriteria::WindowClass(c) => format!("class:{}", c),
                    MatchCriteria::Any => "any".to_string(),
                },
            ));
            // Export actions.
            if let Some(ref pos) = rule.actions.position {
                match pos {
                    PositionSpec::Absolute { x, y } => {
                        out.push_str(&format!("  position|abs|{}|{}\n", x, y));
                    }
                    PositionSpec::CenterOnMonitor(m) => {
                        out.push_str(&format!("  position|center|{}\n", m));
                    }
                    PositionSpec::Percentage { x_pct, y_pct } => {
                        out.push_str(&format!("  position|pct|{}|{}\n", x_pct, y_pct));
                    }
                    PositionSpec::RememberLast => {
                        out.push_str("  position|remember\n");
                    }
                }
            }
            if let Some(ref sz) = rule.actions.size {
                match sz {
                    SizeSpec::Exact { width, height } => {
                        out.push_str(&format!("  size|exact|{}|{}\n", width, height));
                    }
                    SizeSpec::Percentage { w_pct, h_pct } => {
                        out.push_str(&format!("  size|pct|{}|{}\n", w_pct, h_pct));
                    }
                    SizeSpec::RememberLast => {
                        out.push_str("  size|remember\n");
                    }
                }
            }
            if let Some(d) = rule.actions.desktop {
                out.push_str(&format!("  desktop|{}\n", d));
            }
            if let Some(aot) = rule.actions.always_on_top {
                out.push_str(&format!("  always_on_top|{}\n", aot));
            }
            if let Some(state) = rule.actions.initial_state {
                let s = match state {
                    InitialState::Normal => "normal",
                    InitialState::Minimized => "minimized",
                    InitialState::Maximized => "maximized",
                    InitialState::Fullscreen => "fullscreen",
                };
                out.push_str(&format!("  initial_state|{}\n", s));
            }
            if let Some(op) = rule.actions.opacity {
                out.push_str(&format!("  opacity|{}\n", op));
            }
            if let Some(true) = rule.actions.skip_taskbar {
                out.push_str("  skip_taskbar|true\n");
            }
            if let Some(true) = rule.actions.skip_alt_tab {
                out.push_str("  skip_alt_tab|true\n");
            }
            if let Some(true) = rule.actions.no_decorations {
                out.push_str("  no_decorations|true\n");
            }
            if let Some(true) = rule.actions.prevent_close {
                out.push_str("  prevent_close|true\n");
            }
            if let Some(true) = rule.actions.prevent_move {
                out.push_str("  prevent_move|true\n");
            }
            if let Some(true) = rule.actions.prevent_resize {
                out.push_str("  prevent_resize|true\n");
            }
        }
        out
    }

    /// Parse a single rule from a config line (pipe-delimited).
    /// Returns None on malformed input.
    pub fn parse_rule_line(line: &str) -> Option<WindowRule> {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 5 || parts[0] != "rule" {
            return None;
        }
        let id: u32 = parts[1].parse().ok()?;
        let name = parts[2].to_string();
        let priority: i32 = parts[3].parse().ok()?;
        let enabled = parts[4] == "on";
        let criteria_str = if parts.len() > 5 { parts[5] } else { "any" };
        let criteria = if let Some(rest) = criteria_str.strip_prefix("title_exact:") {
            MatchCriteria::TitleExact(rest.to_string())
        } else if let Some(rest) = criteria_str.strip_prefix("title_contains:") {
            MatchCriteria::TitleContains(rest.to_string())
        } else if let Some(rest) = criteria_str.strip_prefix("process:") {
            MatchCriteria::ProcessName(rest.to_string())
        } else if let Some(rest) = criteria_str.strip_prefix("class:") {
            MatchCriteria::WindowClass(rest.to_string())
        } else {
            MatchCriteria::Any
        };

        let mut rule = WindowRule::new(id, &name, criteria);
        rule.priority = priority;
        rule.enabled = enabled;
        Some(rule)
    }
}

impl Default for WindowRulesManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Settings UI model
// ============================================================================

/// Which section of the rules settings UI is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulesSettingsTab {
    RuleList,
    EditRule,
    CreateRule,
}

/// State for the rules settings panel.
pub struct RulesSettingsUI {
    pub active_tab: RulesSettingsTab,
    pub selected_rule_idx: usize,
    pub scroll_offset: usize,
    pub editing_name: String,
    pub editing_criteria_type: usize, // 0=TitleExact, 1=TitleContains, 2=Process, 3=Class, 4=Any
    pub editing_criteria_value: String,
    pub editing_priority: i32,
    pub visible_rules: usize,
}

impl RulesSettingsUI {
    pub fn new() -> Self {
        Self {
            active_tab: RulesSettingsTab::RuleList,
            selected_rule_idx: 0,
            scroll_offset: 0,
            editing_name: String::new(),
            editing_criteria_type: 0,
            editing_criteria_value: String::new(),
            editing_priority: 0,
            visible_rules: 10,
        }
    }

    /// Render the rules settings panel.
    pub fn render(&self, manager: &WindowRulesManager, x: f32, y: f32, w: f32, h: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background panel.
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
            x: x + 16.0,
            y: y + 12.0,
            text: "Window Rules".to_string(),
            font_size: 16.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold, max_width: None,
        });

        // Rule count badge.
        let count_text = format!("{} rules ({} active)",
            manager.total_rule_count(),
            manager.active_rule_count(),
        );
        cmds.push(RenderCommand::Text {
            x: x + w - 200.0,
            y: y + 14.0,
            text: count_text,
            font_size: 12.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular, max_width: None,
        });

        // Eval mode indicator.
        let mode_text = match manager.eval_mode() {
            EvalMode::FirstMatch => "Mode: First Match",
            EvalMode::MergeAll => "Mode: Merge All",
        };
        cmds.push(RenderCommand::Text {
            x: x + w - 200.0,
            y: y + 28.0,
            text: mode_text.to_string(),
            font_size: 10.0,
            color: MOCHA_OVERLAY0,
            font_weight: FontWeightHint::Regular, max_width: None,
        });

        match self.active_tab {
            RulesSettingsTab::RuleList => {
                self.render_rule_list(&mut cmds, manager, x, y + 44.0, w, h - 44.0);
            }
            RulesSettingsTab::EditRule | RulesSettingsTab::CreateRule => {
                self.render_rule_editor(&mut cmds, x, y + 44.0, w, h - 44.0);
            }
        }

        cmds
    }

    fn render_rule_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        manager: &WindowRulesManager,
        x: f32,
        y: f32,
        w: f32,
        _h: f32,
    ) {
        let rules = manager.rules();
        let row_h = 48.0;

        // Column headers.
        let headers = [("Priority", 70.0), ("Name", 180.0), ("Match", 200.0), ("Actions", 60.0), ("Hits", 50.0), ("Status", 60.0)];
        let mut hx = x + 8.0;
        for (label, col_w) in &headers {
            cmds.push(RenderCommand::Text {
                x: hx,
                y: y + 4.0,
                text: label.to_string(),
                font_size: 11.0,
                color: MOCHA_SUBTEXT0,
                font_weight: FontWeightHint::Bold, max_width: None,
            });
            hx += col_w;
        }

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: x + 4.0,
            y1: y + 22.0,
            x2: x + w - 4.0,
            y2: y + 22.0,
            color: MOCHA_SURFACE1,
            width: 1.0,
        });

        // Rule rows.
        let start = self.scroll_offset;
        let end = (start + self.visible_rules).min(rules.len());
        for (i, rule) in rules.iter().enumerate().skip(start).take(end - start) {
            let ry = y + 26.0 + ((i - start) as f32) * row_h;
            let selected = i == self.selected_rule_idx;

            // Row background.
            if selected {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: ry,
                    width: w - 8.0,
                    height: row_h - 4.0,
                    color: MOCHA_SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let mut cx = x + 8.0;

            // Priority.
            let priority_color = if rule.priority > 50 {
                MOCHA_RED
            } else if rule.priority > 10 {
                MOCHA_YELLOW
            } else {
                MOCHA_SUBTEXT1
            };
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: format!("{}", rule.priority),
                font_size: 12.0,
                color: priority_color,
                font_weight: FontWeightHint::Bold, max_width: None,
            });
            cx += 70.0;

            // Name.
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: truncate_string(&rule.name, 24),
                font_size: 12.0,
                color: if rule.enabled { MOCHA_TEXT } else { MOCHA_OVERLAY0 },
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            cx += 180.0;

            // Match criteria.
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: truncate_string(&rule.criteria.description(), 28),
                font_size: 11.0,
                color: MOCHA_BLUE,
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            cx += 200.0;

            // Action count.
            let ac = rule.actions.active_count();
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: format!("{} act.", ac),
                font_size: 11.0,
                color: if ac > 0 { MOCHA_GREEN } else { MOCHA_OVERLAY0 },
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            cx += 60.0;

            // Match count.
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: format!("{}", rule.match_count),
                font_size: 11.0,
                color: MOCHA_SUBTEXT0,
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            cx += 50.0;

            // Status.
            let (status_text, status_color) = if rule.enabled {
                ("ON", MOCHA_GREEN)
            } else {
                ("OFF", MOCHA_RED)
            };
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: ry + 6.0,
                width: 32.0,
                height: 18.0,
                color: Color::rgba(status_color.r, status_color.g, status_color.b, 51),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 6.0,
                y: ry + 8.0,
                text: status_text.to_string(),
                font_size: 10.0,
                color: status_color,
                font_weight: FontWeightHint::Bold, max_width: None,
            });

            // One-shot indicator.
            if rule.one_shot {
                cmds.push(RenderCommand::Text {
                    x: cx + 40.0,
                    y: ry + 8.0,
                    text: "1x".to_string(),
                    font_size: 9.0,
                    color: MOCHA_PEACH,
                    font_weight: FontWeightHint::Bold, max_width: None,
                });
            }

            // Second row: action summary.
            let summary = action_summary(&rule.actions);
            if !summary.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + 78.0,
                    y: ry + 26.0,
                    text: truncate_string(&summary, 60),
                    font_size: 10.0,
                    color: MOCHA_OVERLAY0,
                    font_weight: FontWeightHint::Regular, max_width: None,
                });
            }
        }

        // "Add Rule" button area.
        let btn_y = y + 26.0 + ((end - start) as f32) * row_h + 8.0;
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: btn_y,
            width: 100.0,
            height: 28.0,
            color: MOCHA_BLUE,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 24.0,
            y: btn_y + 7.0,
            text: "+ Add Rule".to_string(),
            font_size: 12.0,
            color: MOCHA_BASE,
            font_weight: FontWeightHint::Bold, max_width: None,
        });
    }

    fn render_rule_editor(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        _h: f32,
    ) {
        let label_x = x + 16.0;
        let input_x = x + 140.0;
        let input_w = w - 170.0;
        let mut cy = y + 12.0;

        let title = if self.active_tab == RulesSettingsTab::CreateRule {
            "Create New Rule"
        } else {
            "Edit Rule"
        };
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cy,
            text: title.to_string(),
            font_size: 14.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold, max_width: None,
        });
        cy += 30.0;

        // Name field.
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cy + 4.0,
            text: "Name:".to_string(),
            font_size: 12.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular, max_width: None,
        });
        cmds.push(RenderCommand::FillRect {
            x: input_x,
            y: cy,
            width: input_w,
            height: 24.0,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: input_x + 8.0,
            y: cy + 5.0,
            text: if self.editing_name.is_empty() {
                "Enter rule name...".to_string()
            } else {
                self.editing_name.clone()
            },
            font_size: 12.0,
            color: if self.editing_name.is_empty() { MOCHA_OVERLAY0 } else { MOCHA_TEXT },
            font_weight: FontWeightHint::Regular, max_width: None,
        });
        cy += 36.0;

        // Match type selector.
        let criteria_labels = ["Title (exact)", "Title (contains)", "Process name", "Window class", "Any"];
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cy + 4.0,
            text: "Match:".to_string(),
            font_size: 12.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular, max_width: None,
        });
        for (i, label) in criteria_labels.iter().enumerate() {
            let bx = input_x + (i as f32) * 110.0;
            let selected = i == self.editing_criteria_type;
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: cy,
                width: 105.0,
                height: 24.0,
                color: if selected { MOCHA_BLUE } else { MOCHA_SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: cy + 6.0,
                text: label.to_string(),
                font_size: 10.0,
                color: if selected { MOCHA_BASE } else { MOCHA_TEXT },
                font_weight: if selected { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: None,
            });
        }
        cy += 36.0;

        // Match value (unless "Any").
        if self.editing_criteria_type < 4 {
            cmds.push(RenderCommand::Text {
                x: label_x,
                y: cy + 4.0,
                text: "Value:".to_string(),
                font_size: 12.0,
                color: MOCHA_SUBTEXT0,
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            cmds.push(RenderCommand::FillRect {
                x: input_x,
                y: cy,
                width: input_w,
                height: 24.0,
                color: MOCHA_SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: input_x + 8.0,
                y: cy + 5.0,
                text: if self.editing_criteria_value.is_empty() {
                    "Enter match value...".to_string()
                } else {
                    self.editing_criteria_value.clone()
                },
                font_size: 12.0,
                color: if self.editing_criteria_value.is_empty() { MOCHA_OVERLAY0 } else { MOCHA_TEXT },
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            cy += 36.0;
        }

        // Priority.
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cy + 4.0,
            text: "Priority:".to_string(),
            font_size: 12.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular, max_width: None,
        });
        cmds.push(RenderCommand::FillRect {
            x: input_x,
            y: cy,
            width: 80.0,
            height: 24.0,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: input_x + 8.0,
            y: cy + 5.0,
            text: format!("{}", self.editing_priority),
            font_size: 12.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular, max_width: None,
        });
        cy += 40.0;

        // Save / Cancel buttons.
        cmds.push(RenderCommand::FillRect {
            x: input_x,
            y: cy,
            width: 80.0,
            height: 28.0,
            color: MOCHA_GREEN,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: input_x + 20.0,
            y: cy + 7.0,
            text: "Save".to_string(),
            font_size: 12.0,
            color: MOCHA_BASE,
            font_weight: FontWeightHint::Bold, max_width: None,
        });
        cmds.push(RenderCommand::FillRect {
            x: input_x + 92.0,
            y: cy,
            width: 80.0,
            height: 28.0,
            color: MOCHA_SURFACE2,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: input_x + 108.0,
            y: cy + 7.0,
            text: "Cancel".to_string(),
            font_size: 12.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular, max_width: None,
        });
    }
}

impl Default for RulesSettingsUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Truncate a string to max chars, adding "..." if truncated.
fn truncate_string(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut result: String = s.chars().take(max.saturating_sub(3)).collect();
        result.push_str("...");
        result
    }
}

/// Build a human-readable summary of a rule's actions.
fn action_summary(actions: &RuleActions) -> String {
    let mut parts = Vec::new();
    if actions.position.is_some() { parts.push("position"); }
    if actions.size.is_some() { parts.push("size"); }
    if actions.desktop.is_some() { parts.push("desktop"); }
    if actions.always_on_top == Some(true) { parts.push("on-top"); }
    if actions.always_on_bottom == Some(true) { parts.push("on-bottom"); }
    if actions.initial_state.is_some() { parts.push("initial-state"); }
    if actions.opacity.is_some() { parts.push("opacity"); }
    if actions.skip_taskbar == Some(true) { parts.push("skip-taskbar"); }
    if actions.skip_alt_tab == Some(true) { parts.push("skip-alt-tab"); }
    if actions.target_monitor.is_some() { parts.push("monitor"); }
    if actions.no_decorations == Some(true) { parts.push("no-decor"); }
    if actions.prevent_close == Some(true) { parts.push("no-close"); }
    if actions.prevent_move == Some(true) { parts.push("no-move"); }
    if actions.prevent_resize == Some(true) { parts.push("no-resize"); }
    if actions.snap_zone.is_some() { parts.push("snap"); }
    parts.join(", ")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- MatchCriteria tests ---

    #[test]
    fn test_title_exact_match() {
        let c = MatchCriteria::TitleExact("Firefox".to_string());
        assert!(c.matches("Firefox", "", ""));
        assert!(!c.matches("firefox", "", ""));
        assert!(!c.matches("Firefox Browser", "", ""));
    }

    #[test]
    fn test_title_contains_case_insensitive() {
        let c = MatchCriteria::TitleContains("fire".to_string());
        assert!(c.matches("Firefox", "", ""));
        assert!(c.matches("FIREFOX", "", ""));
        assert!(c.matches("On Fire!", "", ""));
        assert!(!c.matches("Chrome", "", ""));
    }

    #[test]
    fn test_process_name_match() {
        let c = MatchCriteria::ProcessName("terminal".to_string());
        assert!(c.matches("", "terminal", ""));
        assert!(c.matches("", "TERMINAL", ""));
        assert!(c.matches("", "Terminal", ""));
        assert!(!c.matches("", "term", ""));
    }

    #[test]
    fn test_window_class_match() {
        let c = MatchCriteria::WindowClass("dialog".to_string());
        assert!(c.matches("", "", "dialog"));
        assert!(c.matches("", "", "DIALOG"));
        assert!(!c.matches("", "", "main_window"));
    }

    #[test]
    fn test_any_matches_everything() {
        let c = MatchCriteria::Any;
        assert!(c.matches("anything", "any", "thing"));
        assert!(c.matches("", "", ""));
    }

    #[test]
    fn test_criteria_description() {
        assert_eq!(
            MatchCriteria::TitleExact("foo".to_string()).description(),
            "Title = \"foo\""
        );
        assert_eq!(
            MatchCriteria::ProcessName("bar".to_string()).description(),
            "Process: bar"
        );
        assert_eq!(MatchCriteria::Any.description(), "Any window");
    }

    // --- RuleActions tests ---

    #[test]
    fn test_empty_actions() {
        let a = RuleActions::new();
        assert_eq!(a.active_count(), 0);
    }

    #[test]
    fn test_actions_count() {
        let mut a = RuleActions::new();
        a.position = Some(PositionSpec::CenterOnMonitor(0));
        a.always_on_top = Some(true);
        a.opacity = Some(0.8);
        assert_eq!(a.active_count(), 3);
    }

    #[test]
    fn test_actions_merge() {
        let mut base = RuleActions::new();
        base.position = Some(PositionSpec::CenterOnMonitor(0));
        base.opacity = Some(0.5);

        let mut overlay = RuleActions::new();
        overlay.opacity = Some(0.9);
        overlay.always_on_top = Some(true);

        base.merge(&overlay);
        assert_eq!(base.opacity, Some(0.9)); // overridden
        assert_eq!(base.always_on_top, Some(true)); // added
        assert!(base.position.is_some()); // preserved
    }

    #[test]
    fn test_merge_does_not_clear() {
        let mut base = RuleActions::new();
        base.desktop = Some(2);
        let empty = RuleActions::new();
        base.merge(&empty);
        assert_eq!(base.desktop, Some(2)); // Not cleared by empty merge
    }

    // --- WindowRule tests ---

    #[test]
    fn test_rule_matches_when_enabled() {
        let r = WindowRule::new(1, "test", MatchCriteria::ProcessName("vim".to_string()));
        assert!(r.matches("", "vim", ""));
    }

    #[test]
    fn test_rule_does_not_match_when_disabled() {
        let mut r = WindowRule::new(1, "test", MatchCriteria::ProcessName("vim".to_string()));
        r.enabled = false;
        assert!(!r.matches("", "vim", ""));
    }

    // --- WindowRulesManager tests ---

    #[test]
    fn test_manager_default_rules() {
        let mgr = WindowRulesManager::new();
        assert!(mgr.total_rule_count() >= 3);
        assert_eq!(mgr.active_rule_count(), mgr.total_rule_count());
    }

    #[test]
    fn test_add_rule() {
        let mut mgr = WindowRulesManager::new();
        let initial = mgr.total_rule_count();
        let rule = WindowRule::new(0, "new rule", MatchCriteria::Any);
        let id = mgr.add_rule(rule);
        assert!(id.is_some());
        assert_eq!(mgr.total_rule_count(), initial + 1);
    }

    #[test]
    fn test_remove_rule() {
        let mut mgr = WindowRulesManager::new();
        let rule = WindowRule::new(0, "temp", MatchCriteria::Any);
        let id = mgr.add_rule(rule).unwrap();
        let before = mgr.total_rule_count();
        assert!(mgr.remove_rule(id));
        assert_eq!(mgr.total_rule_count(), before - 1);
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut mgr = WindowRulesManager::new();
        assert!(!mgr.remove_rule(9999));
    }

    #[test]
    fn test_enable_disable() {
        let mut mgr = WindowRulesManager::new();
        let rule = WindowRule::new(0, "toggle", MatchCriteria::Any);
        let id = mgr.add_rule(rule).unwrap();
        assert!(mgr.set_enabled(id, false));
        assert!(!mgr.rule_by_id(id).unwrap().enabled);
        assert!(mgr.set_enabled(id, true));
        assert!(mgr.rule_by_id(id).unwrap().enabled);
    }

    #[test]
    fn test_evaluate_first_match() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        mgr.set_eval_mode(EvalMode::FirstMatch);

        let mut r1 = WindowRule::new(0, "high", MatchCriteria::Any);
        r1.priority = 100;
        r1.actions.opacity = Some(0.5);
        mgr.add_rule(r1);

        let mut r2 = WindowRule::new(0, "low", MatchCriteria::Any);
        r2.priority = 1;
        r2.actions.opacity = Some(0.9);
        r2.actions.always_on_top = Some(true);
        mgr.add_rule(r2);

        let result = mgr.evaluate("any", "any", "any");
        // First match (highest priority) wins.
        assert_eq!(result.opacity, Some(0.5));
        assert_eq!(result.always_on_top, None); // low-priority rule not applied
    }

    #[test]
    fn test_evaluate_merge_all() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        mgr.set_eval_mode(EvalMode::MergeAll);

        let mut r1 = WindowRule::new(0, "high", MatchCriteria::Any);
        r1.priority = 100;
        r1.actions.opacity = Some(0.5);
        mgr.add_rule(r1);

        let mut r2 = WindowRule::new(0, "low", MatchCriteria::Any);
        r2.priority = 1;
        r2.actions.always_on_top = Some(true);
        mgr.add_rule(r2);

        let result = mgr.evaluate("any", "any", "any");
        // Both rules merged; high-priority values override where both set.
        assert_eq!(result.opacity, Some(0.5));
        assert_eq!(result.always_on_top, Some(true));
    }

    #[test]
    fn test_evaluate_no_match() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        let mut r = WindowRule::new(0, "specific", MatchCriteria::ProcessName("firefox".to_string()));
        r.actions.opacity = Some(0.5);
        mgr.add_rule(r);

        let result = mgr.evaluate("", "chrome", "");
        assert_eq!(result.active_count(), 0);
    }

    #[test]
    fn test_one_shot_removal() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        let mut r = WindowRule::new(0, "once", MatchCriteria::Any);
        r.one_shot = true;
        r.actions.always_on_top = Some(true);
        let id = mgr.add_rule(r).unwrap();

        let result = mgr.evaluate("x", "y", "z");
        assert_eq!(result.always_on_top, Some(true));

        // Rule should be removed after one-shot.
        assert!(mgr.rule_by_id(id).is_none());
        assert_eq!(mgr.total_rule_count(), 0);
    }

    #[test]
    fn test_match_count_incremented() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        let r = WindowRule::new(0, "counter", MatchCriteria::Any);
        let id = mgr.add_rule(r).unwrap();

        mgr.evaluate("x", "y", "z");
        mgr.evaluate("a", "b", "c");

        assert_eq!(mgr.rule_by_id(id).unwrap().match_count, 2);
    }

    #[test]
    fn test_remember_state() {
        let mut mgr = WindowRulesManager::new();
        mgr.remember_state("terminal", "", 100, 200, 800, 600);

        // Create a rule that uses RememberLast.
        mgr.rules.clear();
        let mut r = WindowRule::new(0, "term", MatchCriteria::ProcessName("terminal".to_string()));
        r.actions.position = Some(PositionSpec::RememberLast);
        r.actions.size = Some(SizeSpec::RememberLast);
        mgr.add_rule(r);

        let result = mgr.evaluate("", "terminal", "");
        assert_eq!(result.position, Some(PositionSpec::Absolute { x: 100, y: 200 }));
        assert_eq!(result.size, Some(SizeSpec::Exact { width: 800, height: 600 }));
    }

    #[test]
    fn test_remember_state_updates() {
        let mut mgr = WindowRulesManager::new();
        mgr.remember_state("vim", "", 10, 20, 100, 100);
        mgr.remember_state("vim", "", 50, 60, 200, 300);

        mgr.rules.clear();
        let mut r = WindowRule::new(0, "vim", MatchCriteria::ProcessName("vim".to_string()));
        r.actions.position = Some(PositionSpec::RememberLast);
        mgr.add_rule(r);

        let result = mgr.evaluate("", "vim", "");
        assert_eq!(result.position, Some(PositionSpec::Absolute { x: 50, y: 60 }));
    }

    #[test]
    fn test_remember_no_state_returns_none() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        let mut r = WindowRule::new(0, "unknown", MatchCriteria::ProcessName("unknown".to_string()));
        r.actions.position = Some(PositionSpec::RememberLast);
        mgr.add_rule(r);

        let result = mgr.evaluate("", "unknown", "");
        assert_eq!(result.position, None); // No remembered state, cleared to None
    }

    #[test]
    fn test_remember_eviction() {
        let mut mgr = WindowRulesManager::new();
        // Fill to capacity.
        for i in 0..MAX_REMEMBERED {
            mgr.remember_state(&format!("app{}", i), "", 0, 0, 100, 100);
        }
        // One more should evict the oldest.
        mgr.remember_state("newest", "", 999, 999, 999, 999);
        assert!(mgr.remembered.len() <= MAX_REMEMBERED);
    }

    #[test]
    fn test_priority_change() {
        let mut mgr = WindowRulesManager::new();
        let r = WindowRule::new(0, "pr", MatchCriteria::Any);
        let id = mgr.add_rule(r).unwrap();

        assert!(mgr.increase_priority(id));
        assert_eq!(mgr.rule_by_id(id).unwrap().priority, 1);

        assert!(mgr.decrease_priority(id));
        assert_eq!(mgr.rule_by_id(id).unwrap().priority, 0);
    }

    #[test]
    fn test_duplicate_rule() {
        let mut mgr = WindowRulesManager::new();
        let mut r = WindowRule::new(0, "original", MatchCriteria::Any);
        r.actions.opacity = Some(0.7);
        r.match_count = 42;
        let id = mgr.add_rule(r).unwrap();

        let dup_id = mgr.duplicate_rule(id).unwrap();
        let dup = mgr.rule_by_id(dup_id).unwrap();
        assert_eq!(dup.name, "original (copy)");
        assert_eq!(dup.actions.opacity, Some(0.7));
        assert_eq!(dup.match_count, 0); // Reset
    }

    #[test]
    fn test_export_config() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        let mut r = WindowRule::new(1, "test", MatchCriteria::ProcessName("vim".to_string()));
        r.priority = 10;
        r.enabled = true;
        r.actions.always_on_top = Some(true);
        r.id = 1;
        mgr.rules.push(r);

        let config = mgr.export_config();
        assert!(config.contains("rule|1|test|10|on|process:vim"));
        assert!(config.contains("always_on_top|true"));
    }

    #[test]
    fn test_parse_rule_line() {
        let line = "rule|5|My Rule|20|on|process:firefox";
        let rule = WindowRulesManager::parse_rule_line(line).unwrap();
        assert_eq!(rule.id, 5);
        assert_eq!(rule.name, "My Rule");
        assert_eq!(rule.priority, 20);
        assert!(rule.enabled);
        assert_eq!(rule.criteria, MatchCriteria::ProcessName("firefox".to_string()));
    }

    #[test]
    fn test_parse_rule_line_invalid() {
        assert!(WindowRulesManager::parse_rule_line("").is_none());
        assert!(WindowRulesManager::parse_rule_line("not|a|valid|line").is_none());
        assert!(WindowRulesManager::parse_rule_line("rule|abc|name|0|on").is_none()); // bad id
    }

    #[test]
    fn test_rules_sorted_by_priority() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();

        let mut r1 = WindowRule::new(0, "low", MatchCriteria::Any);
        r1.priority = 1;
        mgr.add_rule(r1);

        let mut r2 = WindowRule::new(0, "high", MatchCriteria::Any);
        r2.priority = 100;
        mgr.add_rule(r2);

        let mut r3 = WindowRule::new(0, "mid", MatchCriteria::Any);
        r3.priority = 50;
        mgr.add_rule(r3);

        let sorted = mgr.rules();
        assert_eq!(sorted[0].name, "high");
        assert_eq!(sorted[1].name, "mid");
        assert_eq!(sorted[2].name, "low");
    }

    #[test]
    fn test_eval_mode_switch() {
        let mut mgr = WindowRulesManager::new();
        assert_eq!(mgr.eval_mode(), EvalMode::FirstMatch);
        mgr.set_eval_mode(EvalMode::MergeAll);
        assert_eq!(mgr.eval_mode(), EvalMode::MergeAll);
    }

    #[test]
    fn test_remember_empty_key_ignored() {
        let mut mgr = WindowRulesManager::new();
        mgr.remember_state("", "", 100, 200, 800, 600);
        assert!(mgr.remembered.is_empty());
    }

    #[test]
    fn test_rule_by_id_mut() {
        let mut mgr = WindowRulesManager::new();
        let r = WindowRule::new(0, "mutable", MatchCriteria::Any);
        let id = mgr.add_rule(r).unwrap();
        mgr.rule_by_id_mut(id).unwrap().name = "changed".to_string();
        assert_eq!(mgr.rule_by_id(id).unwrap().name, "changed");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("a very long string indeed", 10), "a very...");
        assert_eq!(truncate_string("", 5), "");
    }

    #[test]
    fn test_action_summary() {
        let mut a = RuleActions::new();
        assert_eq!(action_summary(&a), "");
        a.always_on_top = Some(true);
        a.opacity = Some(0.5);
        let s = action_summary(&a);
        assert!(s.contains("on-top"));
        assert!(s.contains("opacity"));
    }

    #[test]
    fn test_max_rules_cap() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        for i in 0..MAX_RULES {
            let r = WindowRule::new(0, &format!("rule{}", i), MatchCriteria::Any);
            assert!(mgr.add_rule(r).is_some());
        }
        // One more should fail.
        let r = WindowRule::new(0, "overflow", MatchCriteria::Any);
        assert!(mgr.add_rule(r).is_none());
    }

    #[test]
    fn test_ui_creation() {
        let ui = RulesSettingsUI::new();
        assert_eq!(ui.active_tab, RulesSettingsTab::RuleList);
        assert_eq!(ui.selected_rule_idx, 0);
    }

    #[test]
    fn test_ui_render_no_panic() {
        let mgr = WindowRulesManager::new();
        let ui = RulesSettingsUI::new();
        let cmds = ui.render(&mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_edit_tab() {
        let mgr = WindowRulesManager::new();
        let mut ui = RulesSettingsUI::new();
        ui.active_tab = RulesSettingsTab::EditRule;
        let cmds = ui.render(&mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_create_tab() {
        let mgr = WindowRulesManager::new();
        let mut ui = RulesSettingsUI::new();
        ui.active_tab = RulesSettingsTab::CreateRule;
        let cmds = ui.render(&mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_position_spec_variants() {
        let abs = PositionSpec::Absolute { x: 100, y: 200 };
        let center = PositionSpec::CenterOnMonitor(1);
        let pct = PositionSpec::Percentage { x_pct: 0.5, y_pct: 0.5 };
        let rem = PositionSpec::RememberLast;
        // Just ensure they're distinct.
        assert_ne!(abs, center);
        assert_ne!(pct, rem);
    }

    #[test]
    fn test_size_spec_variants() {
        let exact = SizeSpec::Exact { width: 800, height: 600 };
        let pct = SizeSpec::Percentage { w_pct: 0.5, h_pct: 0.5 };
        let rem = SizeSpec::RememberLast;
        assert_ne!(exact, pct);
        assert_ne!(pct, rem);
    }

    #[test]
    fn test_initial_state_variants() {
        assert_ne!(InitialState::Normal, InitialState::Maximized);
        assert_ne!(InitialState::Minimized, InitialState::Fullscreen);
    }

    #[test]
    fn test_default_trait_impls() {
        let _ = RuleActions::default();
        let _ = WindowRulesManager::default();
        let _ = RulesSettingsUI::default();
    }
}

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

use crate::scroll_window;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

// Rule-list column widths. Defined once because the header row and the rule
// rows both lay out against them, and two copies of a column width drift.
const COL_PRIORITY: f32 = 70.0;
const COL_NAME: f32 = 180.0;
const COL_MATCH: f32 = 200.0;
const COL_ACTIONS: f32 = 60.0;
const COL_HITS: f32 = 50.0;
const COL_STATUS: f32 = 60.0;
/// Room left between a column's text and the next column's edge.
const COL_GUTTER: f32 = 8.0;
/// Horizontal room the action-summary line gives up to the icon on its left
/// and the row's right margin.
const SUMMARY_INSET: f32 = 86.0;

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
            Self::ProcessName(name) => process.eq_ignore_ascii_case(name),
            Self::WindowClass(cls) => class.eq_ignore_ascii_case(cls),
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

/// Declare [`RuleActions`]'s fields once, and derive from that list every
/// traversal of them.
///
/// The three traversals — construct-all-empty, merge, and count-the-set-ones —
/// were previously written out by hand, seventeen fields each, fifty-one lines
/// of `if other.x.is_some() { self.x = other.x; }`. They agreed, but only
/// because someone checked: adding an eighteenth action and forgetting one of
/// the three lists gives a rule setting that saves, loads, displays, and is
/// silently dropped when two rules are merged. The macro makes that class of
/// bug unrepresentable — there is one list, and it is the struct definition.
macro_rules! rule_actions {
    ($( $(#[$doc:meta])* $name:ident : $ty:ty ),+ $(,)?) => {
        /// Actions to apply when a rule matches.
        ///
        /// Every field is optional, and `None` means "this rule expresses no
        /// opinion" rather than "off" — which is what lets several rules be
        /// merged without a rule that says nothing about opacity resetting the
        /// opacity a higher-priority rule asked for.
        #[derive(Clone, Debug)]
        pub struct RuleActions {
            $( $(#[$doc])* pub $name: Option<$ty>, )+
        }

        impl RuleActions {
            /// Create empty actions (no overrides).
            #[must_use]
            pub const fn new() -> Self {
                Self { $( $name: None, )+ }
            }

            /// Merge another set of actions on top of this one.
            ///
            /// `other`'s values win wherever it sets one; fields it leaves
            /// unset keep whatever this side had. Callers layering several
            /// rules therefore have to apply them in *increasing* order of
            /// authority, so the one that should win is merged last.
            pub fn merge(&mut self, other: &Self) {
                $( if other.$name.is_some() { self.$name = other.$name; } )+
            }

            /// Count how many actions are actively set.
            #[must_use]
            pub fn active_count(&self) -> usize {
                [ $( self.$name.is_some(), )+ ]
                    .into_iter()
                    .filter(|set| *set)
                    .count()
            }
        }
    };
}

rule_actions! {
    /// Override initial position.
    position: PositionSpec,
    /// Override initial size.
    size: SizeSpec,
    /// Assign to a specific virtual desktop (0-based).
    desktop: u32,
    /// Force always-on-top.
    always_on_top: bool,
    /// Force always-on-bottom (desktop-level).
    always_on_bottom: bool,
    /// Initial window state override.
    initial_state: InitialState,
    /// Custom opacity (0.0 = invisible, 1.0 = fully opaque).
    opacity: f32,
    /// Hide from taskbar.
    skip_taskbar: bool,
    /// Hide from Alt+Tab switcher.
    skip_alt_tab: bool,
    /// Force to specific monitor (0-based index).
    target_monitor: u32,
    /// Disable window decorations (title bar).
    no_decorations: bool,
    /// Minimum size constraint.
    min_size: (u32, u32),
    /// Maximum size constraint.
    max_size: (u32, u32),
    /// Prevent the window from being closed by the user.
    prevent_close: bool,
    /// Prevent the window from being moved.
    prevent_move: bool,
    /// Prevent the window from being resized.
    prevent_resize: bool,
    /// Custom snap zone override (snap layout preset index).
    snap_zone: u32,
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
            self.alloc_id(),
            "Terminal: remember position",
            MatchCriteria::ProcessName("terminal".to_string()),
        );
        terminal_rule.actions.position = Some(PositionSpec::RememberLast);
        terminal_rule.actions.size = Some(SizeSpec::RememberLast);
        terminal_rule.priority = 10;
        self.rules.push(terminal_rule);

        // Settings: always center on primary monitor
        let mut settings_rule = WindowRule::new(
            self.alloc_id(),
            "Settings: center on primary",
            MatchCriteria::ProcessName("settings".to_string()),
        );
        settings_rule.actions.position = Some(PositionSpec::CenterOnMonitor(0));
        settings_rule.priority = 10;
        self.rules.push(settings_rule);

        // Dialog windows: prevent resize
        let mut dialog_rule = WindowRule::new(
            self.alloc_id(),
            "Dialogs: no resize",
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
        sorted.sort_by_key(|r| std::cmp::Reverse(r.priority));
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
    ///
    /// In [`FirstMatch`](EvalMode::FirstMatch) mode only the highest-priority
    /// matching rule applies. In [`MergeAll`](EvalMode::MergeAll) every
    /// matching rule contributes, and where two of them set the same action
    /// **the higher-priority one wins**.
    ///
    /// That last sentence is a fix, not a description: this merged the matches
    /// from highest priority downwards, and `RuleActions::merge` lets the
    /// incoming side win, so each contested field was handed to the *lowest*-
    /// priority rule that mentioned it — the exact inverse of what the
    /// priority field exists to express. The old test passed because its two
    /// rules set disjoint fields, so nothing was ever contested; its comment
    /// ("high-priority values override where both set") described the intent
    /// the code did not implement.
    pub fn evaluate(&mut self, title: &str, process: &str, class: &str) -> RuleActions {
        // Highest priority first. `sort_by_key` is stable, so rules of equal
        // priority keep the order they were added in — the same tie-break the
        // settings list shows.
        let mut matched: Vec<&WindowRule> = self
            .rules
            .iter()
            .filter(|r| r.matches(title, process, class))
            .collect();
        matched.sort_by_key(|r| std::cmp::Reverse(r.priority));
        if self.eval_mode == EvalMode::FirstMatch {
            matched.truncate(1);
        }

        // Applied in *ascending* priority, so the highest-priority rule merges
        // last and its values are the ones that survive.
        let mut result = RuleActions::new();
        for rule in matched.iter().rev() {
            result.merge(&rule.actions);
        }

        // Which rules fired, and which of those asked to be forgotten after
        // firing. Collected as ids because the updates below need `self`
        // mutably, and an index would go stale the moment a one-shot rule is
        // removed.
        let fired: Vec<u32> = matched.iter().map(|r| r.id).collect();
        let one_shot: Vec<u32> = matched
            .iter()
            .filter(|r| r.one_shot)
            .map(|r| r.id)
            .collect();

        for id in fired {
            if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                rule.match_count = rule.match_count.saturating_add(1);
            }
        }
        for id in one_shot {
            self.remove_rule(id);
        }

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
                let oldest_idx = self
                    .remembered
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
            out.push_str(&format!(
                "rule|{}|{}|{}|{}|{}\n",
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
        // Taking the fields off the iterator with `?` is the same test as the
        // `parts.len() < 5` this replaces, except that the length check and
        // the accesses it licenses are now one expression instead of two —
        // so no later edit can add a sixth mandatory field and leave the
        // check saying five.
        let mut parts = line.split('|');
        if parts.next()? != "rule" {
            return None;
        }
        let id: u32 = parts.next()?.parse().ok()?;
        let name = parts.next()?.to_string();
        let priority: i32 = parts.next()?.parse().ok()?;
        let enabled = parts.next()? == "on";
        let criteria_str = parts.next().unwrap_or("any");
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
    pub fn render(
        &self,
        manager: &WindowRulesManager,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background panel.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title bar.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: 40.0,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "Window Rules".to_string(),
            font_size: 16.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Rule count badge.
        let count_text = format!(
            "{} rules ({} active)",
            manager.total_rule_count(),
            manager.active_rule_count(),
        );
        cmds.push(RenderCommand::Text {
            x: x + w - 200.0,
            y: y + 14.0,
            text: count_text,
            font_size: 12.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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

        // Column headers. The widths come from the shared constants rather
        // than being written out again here: the row loop below advances by
        // the same figures, and two copies of a column width drift.
        let headers = [
            ("Priority", COL_PRIORITY),
            ("Name", COL_NAME),
            ("Match", COL_MATCH),
            ("Actions", COL_ACTIONS),
            ("Hits", COL_HITS),
            ("Status", COL_STATUS),
        ];
        let mut hx = x + 8.0;
        for (label, col_w) in &headers {
            cmds.push(RenderCommand::Text {
                x: hx,
                y: y + 4.0,
                text: label.to_string(),
                font_size: 11.0,
                color: MOCHA_SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
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

        // Rule rows. `scroll_offset` is a public field that nothing clamps, so
        // it can name a row past the end of a list that has since shrunk — which
        // used to be an `end - start` underflow here. The shared window is now
        // the only copy of that arithmetic (`touchpad.rs` and `bluetooth.rs`
        // each had their own, differently broken), and it goes one better than
        // the local fix did: a stale offset shows the last page rather than a
        // blank list.
        let window =
            scroll_window::visible_count(rules.len(), self.visible_rules, self.scroll_offset);
        let visible = rules.get(window.start..window.end()).unwrap_or_default();
        for (row, rule) in visible.iter().enumerate() {
            let i = window.start.saturating_add(row);
            let ry = y + 26.0 + (row as f32) * row_h;
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
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cx += COL_PRIORITY;

            // Name. These cells carry no `max_width`, so the elision is the
            // only thing keeping a long rule name -- which the user types --
            // out of the next column. Cut it to the column it occupies.
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: text::elide(
                    &rule.name,
                    COL_NAME - COL_GUTTER,
                    "...",
                    12.0,
                    FontWeightHint::Regular,
                ),
                font_size: 12.0,
                color: if rule.enabled {
                    MOCHA_TEXT
                } else {
                    MOCHA_OVERLAY0
                },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cx += COL_NAME;

            // Match criteria.
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: text::elide(
                    &rule.criteria.description(),
                    COL_MATCH - COL_GUTTER,
                    "...",
                    11.0,
                    FontWeightHint::Regular,
                ),
                font_size: 11.0,
                color: MOCHA_BLUE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cx += COL_MATCH;

            // Action count.
            let ac = rule.actions.active_count();
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: format!("{} act.", ac),
                font_size: 11.0,
                color: if ac > 0 { MOCHA_GREEN } else { MOCHA_OVERLAY0 },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cx += 60.0;

            // Match count.
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: format!("{}", rule.match_count),
                font_size: 11.0,
                color: MOCHA_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // One-shot indicator.
            if rule.one_shot {
                cmds.push(RenderCommand::Text {
                    x: cx + 40.0,
                    y: ry + 8.0,
                    text: "1x".to_string(),
                    font_size: 9.0,
                    color: MOCHA_PEACH,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Second row: action summary.
            let summary = action_summary(&rule.actions);
            if !summary.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + 78.0,
                    y: ry + 26.0,
                    // Spans the rest of the row rather than a fixed column, so
                    // it is elided to the room actually left beside the icon.
                    text: text::elide(
                        &summary,
                        (w - SUMMARY_INSET).max(0.0),
                        "...",
                        10.0,
                        FontWeightHint::Regular,
                    ),
                    font_size: 10.0,
                    color: MOCHA_OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }

        // "Add Rule" button area.
        let btn_y = y + 26.0 + (visible.len() as f32) * row_h + 8.0;
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
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_rule_editor(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, _h: f32) {
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
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 30.0;

        // Name field.
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cy + 4.0,
            text: "Name:".to_string(),
            font_size: 12.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            color: if self.editing_name.is_empty() {
                MOCHA_OVERLAY0
            } else {
                MOCHA_TEXT
            },
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 36.0;

        // Match type selector.
        let criteria_labels = [
            "Title (exact)",
            "Title (contains)",
            "Process name",
            "Window class",
            "Any",
        ];
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cy + 4.0,
            text: "Match:".to_string(),
            font_size: 12.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                font_weight: if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
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
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                color: if self.editing_criteria_value.is_empty() {
                    MOCHA_OVERLAY0
                } else {
                    MOCHA_TEXT
                },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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

// `truncate_string` lived here: it compared `s.len()` (bytes) against a
// character budget, and each of its three call sites passed a budget picked
// independently of the column the text was drawn in. Replaced by
// `text::elide`, which measures against the actual column width. See
// known-issues.md TD-APPS-ESTIMATE-TEXT-WIDTH.

/// Build a human-readable summary of a rule's actions.
fn action_summary(actions: &RuleActions) -> String {
    let mut parts = Vec::new();
    if actions.position.is_some() {
        parts.push("position");
    }
    if actions.size.is_some() {
        parts.push("size");
    }
    if actions.desktop.is_some() {
        parts.push("desktop");
    }
    if actions.always_on_top == Some(true) {
        parts.push("on-top");
    }
    if actions.always_on_bottom == Some(true) {
        parts.push("on-bottom");
    }
    if actions.initial_state.is_some() {
        parts.push("initial-state");
    }
    if actions.opacity.is_some() {
        parts.push("opacity");
    }
    if actions.skip_taskbar == Some(true) {
        parts.push("skip-taskbar");
    }
    if actions.skip_alt_tab == Some(true) {
        parts.push("skip-alt-tab");
    }
    if actions.target_monitor.is_some() {
        parts.push("monitor");
    }
    if actions.no_decorations == Some(true) {
        parts.push("no-decor");
    }
    if actions.prevent_close == Some(true) {
        parts.push("no-close");
    }
    if actions.prevent_move == Some(true) {
        parts.push("no-move");
    }
    if actions.prevent_resize == Some(true) {
        parts.push("no-resize");
    }
    if actions.snap_zone.is_some() {
        parts.push("snap");
    }
    parts.join(", ")
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
        // Both rules contribute. The two set disjoint fields, so this says
        // nothing about who wins a contested one — see
        // `the_higher_priority_rule_wins_a_field_both_rules_set` for that.
        assert_eq!(result.opacity, Some(0.5));
        assert_eq!(result.always_on_top, Some(true));
    }

    #[test]
    fn test_evaluate_no_match() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        let mut r = WindowRule::new(
            0,
            "specific",
            MatchCriteria::ProcessName("firefox".to_string()),
        );
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
        let mut r = WindowRule::new(
            0,
            "term",
            MatchCriteria::ProcessName("terminal".to_string()),
        );
        r.actions.position = Some(PositionSpec::RememberLast);
        r.actions.size = Some(SizeSpec::RememberLast);
        mgr.add_rule(r);

        let result = mgr.evaluate("", "terminal", "");
        assert_eq!(
            result.position,
            Some(PositionSpec::Absolute { x: 100, y: 200 })
        );
        assert_eq!(
            result.size,
            Some(SizeSpec::Exact {
                width: 800,
                height: 600
            })
        );
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
        assert_eq!(
            result.position,
            Some(PositionSpec::Absolute { x: 50, y: 60 })
        );
    }

    #[test]
    fn test_remember_no_state_returns_none() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        let mut r = WindowRule::new(
            0,
            "unknown",
            MatchCriteria::ProcessName("unknown".to_string()),
        );
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
        assert_eq!(
            rule.criteria,
            MatchCriteria::ProcessName("firefox".to_string())
        );
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
    fn a_rule_name_stays_inside_its_column() {
        // These cells carry no `max_width`, so nothing downstream will save a
        // name that overruns -- it draws straight over the Match column. The
        // old character budget could not prevent that, because 24 characters
        // is a different width for every 24 characters.
        let mut mgr = WindowRulesManager::new();
        for (i, name) in [
            "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW",
            "a rule name with accents ééééééééééééééé",
            "short",
        ]
        .iter()
        .enumerate()
        {
            mgr.add_rule(WindowRule::new(i as u32, name, MatchCriteria::Any));
        }
        let ui = RulesSettingsUI::new();
        let mut cmds = Vec::new();
        ui.render_rule_list(&mut cmds, &mgr, 0.0, 0.0, 900.0, 600.0);

        let mut checked = 0;
        for cmd in &cmds {
            if let RenderCommand::Text {
                text,
                font_size,
                font_weight,
                ..
            } = cmd
                && (*font_size - 12.0).abs() < 0.01
                && text != "Priority"
            {
                let w = guitk::text::measure(text, *font_size, *font_weight);
                assert!(
                    w <= COL_NAME - COL_GUTTER + 0.01,
                    "rule name {text:?} measures {w}, wider than its {} px column",
                    COL_NAME - COL_GUTTER
                );
                checked += 1;
            }
        }
        // Without this the test passes on a render that drew no names at all.
        assert!(checked >= 3, "expected three rule names, checked {checked}");
    }

    #[test]
    fn a_short_rule_name_is_not_elided() {
        let mut mgr = WindowRulesManager::new();
        mgr.add_rule(WindowRule::new(1, "short", MatchCriteria::Any));
        let ui = RulesSettingsUI::new();
        let mut cmds = Vec::new();
        ui.render_rule_list(&mut cmds, &mgr, 0.0, 0.0, 900.0, 600.0);
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text == "short"
            )),
            "a name that fits its column should be drawn whole"
        );
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
        let pct = PositionSpec::Percentage {
            x_pct: 0.5,
            y_pct: 0.5,
        };
        let rem = PositionSpec::RememberLast;
        // Just ensure they're distinct.
        assert_ne!(abs, center);
        assert_ne!(pct, rem);
    }

    #[test]
    fn test_size_spec_variants() {
        let exact = SizeSpec::Exact {
            width: 800,
            height: 600,
        };
        let pct = SizeSpec::Percentage {
            w_pct: 0.5,
            h_pct: 0.5,
        };
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

    // -- evaluate: priority ------------------------------------------------

    #[test]
    fn the_higher_priority_rule_wins_a_field_both_rules_set() {
        // This is the case the old merge test left untested, and the case the
        // old implementation got backwards: it merged from the top of the
        // priority order downwards, and `merge` lets the incoming side win, so
        // the *last* rule merged — the least important one — decided every
        // field the two disagreed about.
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        mgr.set_eval_mode(EvalMode::MergeAll);

        let mut low = WindowRule::new(0, "low", MatchCriteria::Any);
        low.priority = 1;
        low.actions.opacity = Some(0.9);
        low.actions.desktop = Some(7);
        mgr.add_rule(low);

        let mut high = WindowRule::new(0, "high", MatchCriteria::Any);
        high.priority = 100;
        high.actions.opacity = Some(0.5);
        mgr.add_rule(high);

        let result = mgr.evaluate("any", "any", "any");
        assert_eq!(result.opacity, Some(0.5), "the priority-100 rule must win");
        // A field only the low-priority rule mentions still applies: `None`
        // means "no opinion", not "off".
        assert_eq!(result.desktop, Some(7));
    }

    #[test]
    fn priority_order_does_not_depend_on_the_order_rules_were_added() {
        // Same two rules, added the other way round. A merge that happened to
        // read as "last one added wins" would pass one of these and fail the
        // other.
        for high_first in [true, false] {
            let mut mgr = WindowRulesManager::new();
            mgr.rules.clear();
            mgr.set_eval_mode(EvalMode::MergeAll);

            let mut low = WindowRule::new(0, "low", MatchCriteria::Any);
            low.priority = 1;
            low.actions.desktop = Some(1);
            let mut high = WindowRule::new(0, "high", MatchCriteria::Any);
            high.priority = 100;
            high.actions.desktop = Some(100);

            if high_first {
                mgr.add_rule(high);
                mgr.add_rule(low);
            } else {
                mgr.add_rule(low);
                mgr.add_rule(high);
            }
            assert_eq!(mgr.evaluate("a", "b", "c").desktop, Some(100));
        }
    }

    #[test]
    fn the_two_modes_break_a_priority_tie_the_same_way() {
        // Equal priorities are ordered by insertion, so the earlier-added rule
        // sorts first — which is the rule `FirstMatch` picks, and the rule the
        // settings list shows at the top. `MergeAll` therefore has to let that
        // same rule win a contested field, or the two modes would disagree
        // about which of two identical-priority rules is the authoritative
        // one, and the list would be showing the wrong order for one of them.
        for mode in [EvalMode::FirstMatch, EvalMode::MergeAll] {
            let mut mgr = WindowRulesManager::new();
            mgr.rules.clear();
            mgr.set_eval_mode(mode);

            let mut first = WindowRule::new(0, "first", MatchCriteria::Any);
            first.actions.desktop = Some(1);
            mgr.add_rule(first);
            let mut second = WindowRule::new(0, "second", MatchCriteria::Any);
            second.actions.desktop = Some(2);
            mgr.add_rule(second);

            assert_eq!(
                mgr.evaluate("a", "b", "c").desktop,
                Some(1),
                "{mode:?} should defer to the earlier-added rule"
            );
            assert_eq!(
                mgr.rules().first().map(|r| r.name.clone()),
                Some("first".to_string()),
                "and the settings list should show it first"
            );
        }
    }

    #[test]
    fn first_match_counts_only_the_rule_that_fired() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        mgr.set_eval_mode(EvalMode::FirstMatch);

        let mut high = WindowRule::new(0, "high", MatchCriteria::Any);
        high.priority = 100;
        let high_id = mgr.add_rule(high).unwrap();
        let mut low = WindowRule::new(0, "low", MatchCriteria::Any);
        low.priority = 1;
        let low_id = mgr.add_rule(low).unwrap();

        mgr.evaluate("a", "b", "c");
        assert_eq!(mgr.rule_by_id(high_id).unwrap().match_count, 1);
        assert_eq!(
            mgr.rule_by_id(low_id).unwrap().match_count,
            0,
            "a rule that never applied has not been hit"
        );
    }

    #[test]
    fn merge_all_counts_every_rule_that_applied() {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        mgr.set_eval_mode(EvalMode::MergeAll);
        let a = mgr
            .add_rule(WindowRule::new(0, "a", MatchCriteria::Any))
            .unwrap();
        let b = mgr
            .add_rule(WindowRule::new(0, "b", MatchCriteria::Any))
            .unwrap();

        mgr.evaluate("x", "y", "z");
        assert_eq!(mgr.rule_by_id(a).unwrap().match_count, 1);
        assert_eq!(mgr.rule_by_id(b).unwrap().match_count, 1);
    }

    #[test]
    fn a_one_shot_rule_that_did_not_apply_survives_first_match() {
        // In FirstMatch mode only the winner fires, so a lower-priority
        // one-shot rule must still be there next time.
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        mgr.set_eval_mode(EvalMode::FirstMatch);

        let mut high = WindowRule::new(0, "high", MatchCriteria::Any);
        high.priority = 100;
        mgr.add_rule(high);
        let mut once = WindowRule::new(0, "once", MatchCriteria::Any);
        once.priority = 1;
        once.one_shot = true;
        let once_id = mgr.add_rule(once).unwrap();

        mgr.evaluate("a", "b", "c");
        assert!(mgr.rule_by_id(once_id).is_some());
    }

    // -- RuleActions -------------------------------------------------------

    #[test]
    fn every_action_field_takes_part_in_merge_and_in_the_count() {
        // The point of generating the three traversals from one field list is
        // that they cannot drift apart. This checks the property directly: set
        // every field on one side, merge into an empty one, and the count must
        // come back equal — which it cannot if `merge` skips a field the
        // struct declares.
        let mut all = RuleActions::new();
        all.position = Some(PositionSpec::CenterOnMonitor(0));
        all.size = Some(SizeSpec::Exact {
            width: 800,
            height: 600,
        });
        all.desktop = Some(1);
        all.always_on_top = Some(true);
        all.always_on_bottom = Some(true);
        all.initial_state = Some(InitialState::Normal);
        all.opacity = Some(0.5);
        all.skip_taskbar = Some(true);
        all.skip_alt_tab = Some(true);
        all.target_monitor = Some(1);
        all.no_decorations = Some(true);
        all.min_size = Some((1, 1));
        all.max_size = Some((2, 2));
        all.prevent_close = Some(true);
        all.prevent_move = Some(true);
        all.prevent_resize = Some(true);
        all.snap_zone = Some(1);

        let declared = all.active_count();
        assert!(declared >= 17, "every declared field should be set here");

        let mut empty = RuleActions::new();
        assert_eq!(empty.active_count(), 0);
        empty.merge(&all);
        assert_eq!(
            empty.active_count(),
            declared,
            "a field the struct declares but merge does not copy"
        );
    }

    #[test]
    fn merging_an_empty_set_of_actions_clears_nothing() {
        let mut actions = RuleActions::new();
        actions.opacity = Some(0.25);
        actions.merge(&RuleActions::new());
        assert_eq!(actions.opacity, Some(0.25));
    }

    // -- rule list rendering -----------------------------------------------

    #[test]
    fn scrolling_past_the_end_of_a_shrunken_rule_list_shows_the_last_page() {
        // `scroll_offset` is public and nothing clamps it, so a list that
        // shrinks under a scrolled view leaves it pointing past the end. That
        // used to be an `end - start` underflow — a panic in the shell's render
        // path, reachable from removing rules while scrolled down. The first fix
        // made it render an empty list; sharing `scroll_window` with the other
        // panels makes it render the last page instead, which is what the user
        // scrolled down to look at.
        //
        // This test was previously named "renders nothing" but only asserted
        // that *something* drew, so it would have passed either way. It now
        // names the row it expects to see.
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        mgr.add_rule(WindowRule::new(0, "only", MatchCriteria::Any));

        let mut ui = RulesSettingsUI::new();
        ui.scroll_offset = 50;
        let cmds = ui.render(&mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty(), "the chrome still draws");
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text == "only"
            )),
            "the one surviving rule should be pulled back into view"
        );
    }

    #[test]
    fn parse_rule_line_needs_all_five_mandatory_fields() {
        assert!(WindowRulesManager::parse_rule_line("rule|1|name|0").is_none());
        assert!(WindowRulesManager::parse_rule_line("rule|1|name|0|on").is_some());
        assert!(WindowRulesManager::parse_rule_line("notarule|1|name|0|on").is_none());
        assert!(WindowRulesManager::parse_rule_line("").is_none());
    }
}

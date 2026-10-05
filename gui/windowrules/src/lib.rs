//! Window rules: what happens to a program's windows as they open -- the
//! rules, the engine that applies them, and the file a user keeps them in.
//!
//! A rule matches a window by its title or by the program it belongs to (its
//! app id), and says what to do with it: where it goes and how big it is,
//! which virtual desktop, whether it opens minimised or maximised, whether it
//! has a taskbar button or a place in Alt+Tab, and so on ([`RuleActions`]).
//! [`WindowRulesManager::evaluate`] answers, for a window as it arrives, what
//! the matching rules ask -- the highest-priority rule's answer where two
//! disagree.
//!
//! # Why this is a crate of its own
//!
//! It was a module of the desktop shell, which applies the rules. But the
//! rules are the user's: they are written by Settings (lane E's
//! `apps/settings`), kept in a file ([`file`](mod@file)), and read by the shell -- and
//! a settings program that linked the whole desktop shell to read one file
//! would invert the dependency every other program in the tree keeps. So the
//! model and its file format are here, and both sides build on them. The
//! shell keeps its settings panel; the engine and the file are one copy.
//!
//! # What the shell can carry out
//!
//! Not every action reaches the screen yet: the desktop shell's own
//! `window_rules` module says which, and why the others are kept rather than
//! faked (`known-issues.md`
//! `TD-C-TWELVE-OF-SEVENTEEN-WINDOW-RULE-ACTIONS-HAVE-NOWHERE-TO-GO`).

// ============================================================================
// Rule matching criteria
// ============================================================================

/// How a rule matches against window properties.
///
/// # Why there is one program criterion and not two
///
/// This had a `ProcessName` and a `WindowClass`, matched against two separate
/// strings the caller was expected to supply. Neither existed anywhere: a
/// window on this desktop carries a title, a pid and — since the app-id work —
/// an id its program declares, and nothing on the wire has ever carried a
/// "class". The two variants were X11's duality (`WM_CLASS`'s instance and
/// class) transplanted into a system that does not have it, and they were the
/// reason every default rule below was unmatchable.
///
/// So both collapse into [`AppId`](Self::AppId), which matches the one
/// identifier a window actually has. That is the Wayland model, and it is the
/// honest one here: there is exactly one answer to "which program is this?",
/// so there is exactly one criterion for it.
#[derive(Clone, Debug, PartialEq)]
pub enum MatchCriteria {
    /// Match window title exactly.
    TitleExact(String),
    /// Match if window title contains this substring (case-insensitive).
    TitleContains(String),
    /// Match the program the window belongs to (case-insensitive).
    ///
    /// The window's app id -- the program it belongs to, as it says -- conventionally
    /// the executable's file stem, lower-cased. A window that named no program
    /// has an empty id and matches **nothing**: a rule about an unnamed program
    /// would otherwise be a rule about all of them, which is the one reading no
    /// user could intend.
    AppId(String),
    /// Match any window (used for global defaults).
    Any,
}

impl MatchCriteria {
    /// Test whether a window matches this criterion.
    #[must_use]
    pub fn matches(&self, title: &str, app_id: &str) -> bool {
        match self {
            Self::TitleExact(t) => title == t,
            Self::TitleContains(sub) => {
                let lower_title = title.to_lowercase();
                let lower_sub = sub.to_lowercase();
                lower_title.contains(&lower_sub)
            }
            // The empty check is not redundant with the comparison: a *rule*
            // written with an empty id would otherwise match every window that
            // declined to name itself, which is a rule the settings panel lets
            // you write by pressing Save with the value box untouched.
            Self::AppId(name) => {
                !app_id.is_empty() && !name.is_empty() && app_id.eq_ignore_ascii_case(name)
            }
            Self::Any => true,
        }
    }

    /// Human-readable description of this criterion.
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::TitleExact(t) => format!("Title = \"{t}\""),
            Self::TitleContains(s) => format!("Title contains \"{s}\""),
            Self::AppId(n) => format!("App: {n}"),
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
        ///
        /// `PartialEq` and not `Eq`: opacity and the percentages are `f32`.
        /// What compares them is the engine asking whether a rule read again
        /// is the rule it already had, and every value the file can produce is
        /// finite, so the NaN that would make a rule unequal to itself never
        /// arises from it.
        #[derive(Clone, Debug, PartialEq)]
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
    /// Snap the window into a zone on arrival.
    ///
    /// The number is a slot index across *all* the presets at once — what
    /// `guiremote::zones::SnapSlot::index` returns, and what
    /// `SnapSlot::from_index` reads back — not a zone within a preset, because
    /// a rule has to name the layout as well as the cell. Anything from
    /// `SnapSlot::COUNT` up names no slot and is dropped.
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

/// Identifier for a window rule.
///
/// 64 bits rather than 32 so that the sequence handing these out cannot run
/// out — see `guitk::idseq` for why that is a design choice and not an
/// arbitrary width.
pub type RuleId = u64;

/// A window rule: a match criterion plus the actions to take.
#[derive(Clone, Debug)]
pub struct WindowRule {
    /// Unique rule identifier.
    pub id: RuleId,
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
    pub fn new(id: RuleId, name: &str, criteria: MatchCriteria) -> Self {
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
    #[must_use]
    pub fn matches(&self, title: &str, app_id: &str) -> bool {
        self.enabled && self.criteria.matches(title, app_id)
    }
}

// ============================================================================
// Remembered window state
// ============================================================================

/// Remembered position/size for "RememberLast" specs.
#[derive(Clone, Debug)]
struct RememberedState {
    /// Key: the app id, lower-cased. Never empty — see `remember_state`.
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
pub const MAX_RULES: usize = 256;

/// Maximum remembered window states.
const MAX_REMEMBERED: usize = 128;

/// Manages window rules and their evaluation.
pub struct WindowRulesManager {
    rules: Vec<WindowRule>,
    /// The next rule id to issue: ids are never reused.
    next_id: RuleId,
    eval_mode: EvalMode,
    /// Remembered positions for RememberLast.
    remembered: Vec<RememberedState>,
    /// Monotonic counter for remembered state timestamps.
    timestamp_counter: u64,
    /// The one-shot rules used up this session, as they were when used.
    ///
    /// Kept because the rules are read again whenever their file changes, and
    /// a once-rule the file still says the same thing about must not come
    /// back armed: the user adding an unrelated rule would otherwise send the
    /// next matching window wherever the once-rule had sent the first. See
    /// [`replace_rules`](Self::replace_rules). At most one entry per rule the
    /// file holds, since `replace_rules` forgets what the file stopped saying.
    spent: Vec<WindowRule>,
}

/// The rules a user has before they write any of their own -- what
/// [`WindowRulesManager::new`] holds, and what a rules file that says nothing
/// about rules (or no file at all) means.
///
/// In priority order, highest first; their ids are left at nought for the
/// manager to issue.
#[must_use]
pub fn default_rules() -> Vec<WindowRule> {
    // The shell's own surfaces are not applications.
    //
    // This replaces a "Dialogs: no resize" default keyed on the window
    // class, which nothing could ever match: there was no class on the
    // wire, and a dialog is not a program, so there is no app id that
    // means "a dialog" either. The compositor knows dialogs by `Layer`,
    // which this engine does not match on.
    //
    // What stands here instead is a rule that can both match and be
    // carried out. `apply_window_list` already drops everything outside
    // `Layer::Normal`, so today it is belt and braces — but the shell's
    // four surfaces all say `slateos-shell`, and if any of them ever
    // moved into the normal layer the taskbar would sprout a button
    // labelled "Taskbar" and Alt+Tab would offer to switch to the
    // wallpaper.
    let mut chrome = WindowRule::new(
        0,
        "Shell chrome: not an application",
        MatchCriteria::AppId("slateos-shell".to_string()),
    );
    chrome.actions.skip_taskbar = Some(true);
    chrome.actions.skip_alt_tab = Some(true);
    chrome.priority = 100;

    // Terminal windows: remember last position and size.
    let mut terminal = WindowRule::new(
        0,
        "Terminal: remember position",
        MatchCriteria::AppId("terminal".to_string()),
    );
    terminal.actions.position = Some(PositionSpec::RememberLast);
    terminal.actions.size = Some(SizeSpec::RememberLast);
    terminal.priority = 10;

    // Settings: always centred on the primary monitor.
    let mut settings = WindowRule::new(
        0,
        "Settings: center on primary",
        MatchCriteria::AppId("settings".to_string()),
    );
    settings.actions.position = Some(PositionSpec::CenterOnMonitor(0));
    settings.priority = 10;

    vec![chrome, terminal, settings]
}

/// Whether `a` and `b` say the same thing: the same name, matching the same
/// way and doing the same. Whether either is turned on, and where it stands
/// among the others, is not part of it -- see
/// [`WindowRulesManager::replace_rules`].
fn same_rule(a: &WindowRule, b: &WindowRule) -> bool {
    a.name == b.name && a.criteria == b.criteria && a.actions == b.actions
}

impl WindowRulesManager {
    /// Create a new manager with default rules -- the rules a user has before
    /// they write any of their own ([`default_rules`]).
    #[must_use]
    pub fn new() -> Self {
        let mut mgr = Self::empty();
        mgr.replace_rules(default_rules());
        mgr
    }

    /// A manager holding no rules at all, not even the defaults.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            rules: Vec::new(),
            next_id: 1,
            eval_mode: EvalMode::FirstMatch,
            remembered: Vec::new(),
            timestamp_counter: 0,
            spent: Vec::new(),
        }
    }

    /// Hold `rules` -- the user's, read from their file -- in place of every
    /// rule held now, the defaults included: a user's file is the whole of
    /// their rules. Each is given a fresh id. Geometry remembered for
    /// "remember last" rules is kept, since it describes windows, not rules.
    ///
    /// # What a rule has done carries over to the same rule read again
    ///
    /// The file is read again whenever it changes, and most of a change is
    /// usually about other rules. So a rule that says what one held already
    /// said -- the same name, matching the same way, doing the same -- keeps
    /// its count of windows matched, and a one-shot
    /// rule already used this session stays used: it is left out rather than
    /// armed again. Changing the rule itself, in any of those three, makes it
    /// a new rule, so a once-rule the user edits fires once more. Turning a
    /// rule off and on again, or moving it up or down, does not.
    ///
    /// Answers how many were dropped for being past [`MAX_RULES`].
    pub fn replace_rules(&mut self, rules: Vec<WindowRule>) -> usize {
        let before = std::mem::take(&mut self.rules);
        // What the file stopped saying is forgotten, so a used once-rule the
        // user deletes and later writes again is armed again -- and so this
        // list never holds more than the file does.
        self.spent
            .retain(|used| rules.iter().any(|rule| same_rule(used, rule)));
        let mut dropped: usize = 0;
        for mut rule in rules {
            if rule.one_shot && self.spent.iter().any(|used| same_rule(used, &rule)) {
                continue;
            }
            if let Some(held) = before.iter().find(|held| same_rule(held, &rule)) {
                rule.match_count = held.match_count;
            }
            if self.add_rule(rule).is_none() {
                // Bounded by the length of `rules`, so it cannot saturate.
                dropped = dropped.saturating_add(1);
            }
        }
        dropped
    }

    /// Allocate the next unique rule ID.
    fn alloc_id(&mut self) -> RuleId {
        // A u64 issued once per rule cannot run out -- at a rule a nanosecond
        // it would last five centuries -- so the saturation is never reached;
        // it is there because the workspace forbids arithmetic that can wrap.
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
    pub fn add_rule(&mut self, mut rule: WindowRule) -> Option<RuleId> {
        if self.rules.len() >= MAX_RULES {
            return None;
        }
        let id = self.alloc_id();
        rule.id = id;
        self.rules.push(rule);
        Some(id)
    }

    /// Remove a rule by ID. Returns true if found.
    pub fn remove_rule(&mut self, id: RuleId) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        self.rules.len() < before
    }

    /// Enable or disable a rule by ID.
    pub fn set_enabled(&mut self, id: RuleId, enabled: bool) -> bool {
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
    pub fn rule_by_id(&self, id: RuleId) -> Option<&WindowRule> {
        self.rules.iter().find(|r| r.id == id)
    }

    /// Get a mutable rule by ID.
    pub fn rule_by_id_mut(&mut self, id: RuleId) -> Option<&mut WindowRule> {
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
    ///
    /// # Call this once per window, when the window arrives
    ///
    /// It takes `&mut self` because it is not a pure query: it counts each
    /// firing on the rule, and it *deletes* one-shot rules. Calling it for
    /// every window of every window list — the shape the shell's other
    /// projections have — would destroy every one-shot rule on the first frame
    /// after it was written, and would re-apply `initial_state` to a window
    /// each time the compositor said anything about it, so a window the user
    /// un-maximised would snap back within the frame.
    pub fn evaluate(&mut self, title: &str, app_id: &str) -> RuleActions {
        // Highest priority first. `sort_by_key` is stable, so rules of equal
        // priority keep the order they were added in — the same tie-break the
        // settings list shows.
        let mut matched: Vec<&WindowRule> = self
            .rules
            .iter()
            .filter(|r| r.matches(title, app_id))
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
        let fired: Vec<RuleId> = matched.iter().map(|r| r.id).collect();
        let one_shot: Vec<RuleId> = matched
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
            // Remembered as used before it goes, so that reading the file
            // again does not arm it again (`replace_rules`).
            if let Some(at) = self.rules.iter().position(|r| r.id == id) {
                let used = self.rules.remove(at);
                if !self.spent.iter().any(|old| same_rule(old, &used)) {
                    // Rules read from the file never reach the cap -- it holds
                    // at most MAX_RULES, and `replace_rules` prunes this to
                    // what it holds -- but rules added one at a time between
                    // reads could, and the oldest is the one to let go.
                    if self.spent.len() >= MAX_RULES {
                        self.spent.remove(0);
                    }
                    self.spent.push(used);
                }
            }
        }

        self.resolve_remembered(&mut result, app_id);

        result
    }

    /// Resolve RememberLast position/size from stored state.
    ///
    /// Keyed on the app id alone. It used to fall back to the window class when
    /// the process name was empty, which meant an unnamed window inherited the
    /// remembered geometry of every other unnamed window — one shared entry
    /// under the empty key. An empty id now resolves to nothing, so a window
    /// that declines to name itself simply gets no remembered geometry.
    fn resolve_remembered(&self, actions: &mut RuleActions, app_id: &str) {
        let key = app_id.to_lowercase();

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
    pub fn remember_state(&mut self, app_id: &str, x: i32, y: i32, width: u32, height: u32) {
        let key = app_id.to_lowercase();

        // A window that named no program is not remembered at all. The empty
        // key is not a program — every anonymous window on the desktop would
        // share one entry and overwrite each other's geometry in turn.
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
    pub fn increase_priority(&mut self, id: RuleId) -> bool {
        if let Some(r) = self.rules.iter_mut().find(|r| r.id == id) {
            r.priority = r.priority.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Move a rule's priority down (decrease by 1).
    pub fn decrease_priority(&mut self, id: RuleId) -> bool {
        if let Some(r) = self.rules.iter_mut().find(|r| r.id == id) {
            r.priority = r.priority.saturating_sub(1);
            true
        } else {
            false
        }
    }

    /// Duplicate a rule with a new ID.
    pub fn duplicate_rule(&mut self, id: RuleId) -> Option<RuleId> {
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
}

impl Default for WindowRulesManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleActions {
    /// The actions a rule takes, named in a few words each -- for a list
    /// of rules, where a row has room for a line and not a form.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.position.is_some() {
            parts.push("position");
        }
        if self.size.is_some() {
            parts.push("size");
        }
        if self.desktop.is_some() {
            parts.push("desktop");
        }
        if self.always_on_top == Some(true) {
            parts.push("on-top");
        }
        if self.always_on_bottom == Some(true) {
            parts.push("on-bottom");
        }
        if self.initial_state.is_some() {
            parts.push("initial-state");
        }
        if self.opacity.is_some() {
            parts.push("opacity");
        }
        if self.skip_taskbar == Some(true) {
            parts.push("skip-taskbar");
        }
        if self.skip_alt_tab == Some(true) {
            parts.push("skip-alt-tab");
        }
        if self.target_monitor.is_some() {
            parts.push("monitor");
        }
        if self.no_decorations == Some(true) {
            parts.push("no-decor");
        }
        if self.prevent_close == Some(true) {
            parts.push("no-close");
        }
        if self.prevent_move == Some(true) {
            parts.push("no-move");
        }
        if self.prevent_resize == Some(true) {
            parts.push("no-resize");
        }
        if self.snap_zone.is_some() {
            parts.push("snap");
        }
        parts.join(", ")
    }
}

pub mod file;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

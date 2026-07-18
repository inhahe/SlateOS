//! Window Rules — per-window placement and behavior rules.
//!
//! Allows defining rules that automatically apply to windows based on
//! app name, window title, or window class. Rules control placement,
//! workspace assignment, opacity, always-on-top, and decorations.
//!
//! ## Architecture
//!
//! ```text
//! Window opens
//!   → windowrules::match_rules(app, title) → matching rules
//!   → windowrules::apply_rule(window) → set placement/behavior
//!
//! Configuration
//!   → windowrules::add_rule(match, actions)
//!   → windowrules::remove_rule(id)
//!
//! Integration:
//!   → winsnap (snap behavior)
//!   → wintiling (tiling overrides)
//!   → vdesktop (workspace assignment)
//!   → snaplayout (snap zone selection)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How to match a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    /// Exact app name match.
    AppExact,
    /// App name contains substring.
    AppContains,
    /// Window title contains substring.
    TitleContains,
    /// Window class exact match.
    ClassExact,
    /// Match all windows.
    Any,
}

impl MatchType {
    pub fn label(self) -> &'static str {
        match self {
            Self::AppExact => "App (exact)",
            Self::AppContains => "App (contains)",
            Self::TitleContains => "Title (contains)",
            Self::ClassExact => "Class (exact)",
            Self::Any => "Any",
        }
    }
}

/// Action to apply to matching windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    /// Set initial position (x, y).
    SetPosition,
    /// Set initial size (w, h).
    SetSize,
    /// Move to specific workspace.
    MoveToWorkspace,
    /// Set always-on-top.
    AlwaysOnTop,
    /// Set always-on-bottom.
    AlwaysOnBottom,
    /// Set window opacity (percent).
    SetOpacity,
    /// Remove window decorations.
    NoDecorations,
    /// Start maximized.
    StartMaximized,
    /// Start minimized.
    StartMinimized,
    /// Start fullscreen.
    StartFullscreen,
    /// Skip taskbar.
    SkipTaskbar,
    /// Skip pager/task view.
    SkipPager,
    /// Pin to all workspaces.
    PinAllWorkspaces,
    /// Center on screen.
    CenterOnScreen,
}

impl RuleAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::SetPosition => "Set Position",
            Self::SetSize => "Set Size",
            Self::MoveToWorkspace => "Move to Workspace",
            Self::AlwaysOnTop => "Always on Top",
            Self::AlwaysOnBottom => "Always on Bottom",
            Self::SetOpacity => "Set Opacity",
            Self::NoDecorations => "No Decorations",
            Self::StartMaximized => "Start Maximized",
            Self::StartMinimized => "Start Minimized",
            Self::StartFullscreen => "Start Fullscreen",
            Self::SkipTaskbar => "Skip Taskbar",
            Self::SkipPager => "Skip Pager",
            Self::PinAllWorkspaces => "Pin All Workspaces",
            Self::CenterOnScreen => "Center on Screen",
        }
    }
}

/// An action with optional integer parameters.
#[derive(Debug, Clone)]
pub struct ActionEntry {
    pub action: RuleAction,
    pub param1: i32,
    pub param2: i32,
}

/// A window rule.
#[derive(Debug, Clone)]
pub struct WindowRule {
    pub id: u32,
    pub name: String,
    pub match_type: MatchType,
    pub match_value: String,
    pub actions: Vec<ActionEntry>,
    pub enabled: bool,
    pub hit_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RULES: usize = 200;

struct State {
    rules: Vec<WindowRule>,
    next_id: u32,
    total_matches: u64,
    total_applied: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        rules: alloc::vec![
            WindowRule {
                id: 1,
                name: String::from("Terminal always on top"),
                match_type: MatchType::AppContains,
                match_value: String::from("terminal"),
                actions: alloc::vec![
                    ActionEntry { action: RuleAction::AlwaysOnTop, param1: 1, param2: 0 },
                ],
                enabled: false,
                hit_count: 0,
            },
            WindowRule {
                id: 2,
                name: String::from("Media player no decorations"),
                match_type: MatchType::AppContains,
                match_value: String::from("mediaplayer"),
                actions: alloc::vec![
                    ActionEntry { action: RuleAction::NoDecorations, param1: 1, param2: 0 },
                ],
                enabled: false,
                hit_count: 0,
            },
        ],
        next_id: 3,
        total_matches: 0,
        total_applied: 0,
        ops: 0,
    });
}

/// Add a new window rule.
pub fn add_rule(name: &str, match_type: MatchType, match_value: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.rules.len() >= MAX_RULES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.rules.push(WindowRule {
            id,
            name: String::from(name),
            match_type,
            match_value: String::from(match_value),
            actions: Vec::new(),
            enabled: true,
            hit_count: 0,
        });
        Ok(id)
    })
}

/// Add an action to a rule.
pub fn add_action(rule_id: u32, action: RuleAction, param1: i32, param2: i32) -> KernelResult<()> {
    with_state(|state| {
        let rule = state.rules.iter_mut().find(|r| r.id == rule_id)
            .ok_or(KernelError::NotFound)?;
        rule.actions.push(ActionEntry { action, param1, param2 });
        Ok(())
    })
}

/// Remove a rule.
pub fn remove_rule(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.rules.len();
        state.rules.retain(|r| r.id != id);
        if state.rules.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Enable/disable a rule.
pub fn set_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let rule = state.rules.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        rule.enabled = enabled;
        Ok(())
    })
}

/// Match rules against a window. Returns matching rule IDs and their actions.
pub fn match_rules(app_name: &str, title: &str, class: &str) -> Vec<(u32, Vec<ActionEntry>)> {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let app_lower = app_name.to_lowercase();
    let title_lower = title.to_lowercase();
    let mut matches = Vec::new();
    for rule in &state.rules {
        if !rule.enabled { continue; }
        let matched = match rule.match_type {
            MatchType::AppExact => app_name == rule.match_value,
            MatchType::AppContains => app_lower.contains(&rule.match_value.to_lowercase()),
            MatchType::TitleContains => title_lower.contains(&rule.match_value.to_lowercase()),
            MatchType::ClassExact => class == rule.match_value,
            MatchType::Any => true,
        };
        if matched {
            matches.push((rule.id, rule.actions.clone()));
        }
    }
    matches
}

/// Record a rule match (update hit count and stats).
pub fn record_match(rule_id: u32) -> KernelResult<usize> {
    with_state(|state| {
        let rule = state.rules.iter_mut().find(|r| r.id == rule_id)
            .ok_or(KernelError::NotFound)?;
        rule.hit_count += 1;
        let action_count = rule.actions.len();
        state.total_matches += 1;
        state.total_applied += action_count as u64;
        Ok(action_count)
    })
}

/// Get a rule by id.
pub fn get_rule(id: u32) -> Option<WindowRule> {
    STATE.lock().as_ref().and_then(|s| s.rules.iter().find(|r| r.id == id).cloned())
}

/// List all rules.
pub fn list_rules() -> Vec<WindowRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rules.clone())
}

/// Statistics: (rule_count, enabled_count, total_matches, total_applied, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let enabled = s.rules.iter().filter(|r| r.enabled).count();
            (s.rules.len(), enabled, s.total_matches, s.total_applied, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("windowrules::self_test() — running tests...");
    init_defaults();

    // 1: Default rules exist.
    assert_eq!(list_rules().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Add rule.
    let id = add_rule("Browser centered", MatchType::AppContains, "browser").expect("add");
    add_action(id, RuleAction::CenterOnScreen, 0, 0).expect("action1");
    add_action(id, RuleAction::SetSize, 1280, 720).expect("action2");
    assert_eq!(list_rules().len(), 3);
    crate::serial_println!("  [2/8] add rule: OK");

    // 3: Match rules.
    let matches = match_rules("web_browser", "Home", "");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].1.len(), 2);
    crate::serial_println!("  [3/8] match: OK");

    // 4: No match for unrelated app.
    let matches = match_rules("calculator", "Calc", "");
    assert_eq!(matches.len(), 0);
    crate::serial_println!("  [4/8] no match: OK");

    // 5: Record match.
    let actions = record_match(id).expect("record");
    assert_eq!(actions, 2);
    let rule = get_rule(id).expect("get");
    assert_eq!(rule.hit_count, 1);
    crate::serial_println!("  [5/8] record match: OK");

    // 6: Disable rule.
    set_enabled(id, false).expect("disable");
    let matches = match_rules("web_browser", "Home", "");
    assert_eq!(matches.len(), 0);
    crate::serial_println!("  [6/8] disable: OK");

    // 7: Remove rule.
    remove_rule(id).expect("remove");
    assert_eq!(list_rules().len(), 2);
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (rules, _enabled, total_matches, total_applied, ops) = stats();
    assert_eq!(rules, 2);
    assert_eq!(total_matches, 1);
    assert_eq!(total_applied, 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("windowrules::self_test() — all 8 tests passed");
}

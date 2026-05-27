//! File Rules — automatic file organization rules.
//!
//! Defines rules for automatically organizing files based on
//! extension, name pattern, size, and date criteria.
//!
//! ## Architecture
//!
//! ```text
//! File event
//!   → filerules::evaluate(file) → matching rules
//!   → filerules::apply(rule, file) → move/rename/tag
//!
//! Integration:
//!   → notify (filesystem events)
//!   → search (file search)
//!   → tags (file tagging)
//!   → dirsync (directory sync)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Rule action type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    MoveTo,
    CopyTo,
    Rename,
    AddTag,
    SetPermission,
    Compress,
    Delete,
    Notify,
}

impl RuleAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::MoveTo => "Move To",
            Self::CopyTo => "Copy To",
            Self::Rename => "Rename",
            Self::AddTag => "Tag",
            Self::SetPermission => "Permission",
            Self::Compress => "Compress",
            Self::Delete => "Delete",
            Self::Notify => "Notify",
        }
    }
}

/// Match condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchCondition {
    ExtensionIs,
    NameContains,
    NameStartsWith,
    SizeGreaterThan,
    SizeLessThan,
    InDirectory,
    AnyFile,
}

impl MatchCondition {
    pub fn label(self) -> &'static str {
        match self {
            Self::ExtensionIs => "Extension Is",
            Self::NameContains => "Name Contains",
            Self::NameStartsWith => "Name Starts With",
            Self::SizeGreaterThan => "Size >",
            Self::SizeLessThan => "Size <",
            Self::InDirectory => "In Directory",
            Self::AnyFile => "Any File",
        }
    }
}

/// A file organization rule.
#[derive(Debug, Clone)]
pub struct FileRule {
    pub id: u32,
    pub name: String,
    pub condition: MatchCondition,
    pub pattern: String,
    pub action: RuleAction,
    pub action_param: String,
    pub enabled: bool,
    pub hit_count: u64,
    pub last_hit_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RULES: usize = 200;

struct State {
    rules: Vec<FileRule>,
    next_id: u32,
    total_evaluations: u64,
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
            FileRule { id: 1, name: String::from("Move downloads"), condition: MatchCondition::InDirectory, pattern: String::from("/downloads"), action: RuleAction::MoveTo, action_param: String::from("/documents/sorted"), enabled: false, hit_count: 0, last_hit_ns: 0 },
            FileRule { id: 2, name: String::from("Tag images"), condition: MatchCondition::ExtensionIs, pattern: String::from("png"), action: RuleAction::AddTag, action_param: String::from("image"), enabled: true, hit_count: 0, last_hit_ns: 0 },
            FileRule { id: 3, name: String::from("Compress large logs"), condition: MatchCondition::SizeGreaterThan, pattern: String::from("10485760"), action: RuleAction::Compress, action_param: String::from("gzip"), enabled: true, hit_count: 0, last_hit_ns: 0 },
        ],
        next_id: 4,
        total_evaluations: 0,
        total_matches: 0,
        total_applied: 0,
        ops: 0,
    });
}

/// Add a rule.
pub fn add_rule(name: &str, condition: MatchCondition, pattern: &str, action: RuleAction, param: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.rules.len() >= MAX_RULES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.rules.push(FileRule {
            id, name: String::from(name), condition,
            pattern: String::from(pattern), action,
            action_param: String::from(param), enabled: true,
            hit_count: 0, last_hit_ns: 0,
        });
        Ok(id)
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

/// Evaluate a file against all enabled rules. Returns matching rule IDs and actions.
pub fn evaluate(filename: &str, extension: &str, size_bytes: u64, directory: &str) -> KernelResult<Vec<(u32, RuleAction, String)>> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        state.total_evaluations += 1;
        let mut matches = Vec::new();
        let fname_lower = filename.to_lowercase();
        let ext_lower = extension.to_lowercase();
        let dir_lower = directory.to_lowercase();

        for rule in &mut state.rules {
            if !rule.enabled { continue; }
            let matched = match rule.condition {
                MatchCondition::ExtensionIs => ext_lower == rule.pattern.to_lowercase(),
                MatchCondition::NameContains => fname_lower.contains(&rule.pattern.to_lowercase()),
                MatchCondition::NameStartsWith => fname_lower.starts_with(&rule.pattern.to_lowercase()),
                MatchCondition::SizeGreaterThan => {
                    rule.pattern.parse::<u64>().is_ok_and(|limit| size_bytes > limit)
                }
                MatchCondition::SizeLessThan => {
                    rule.pattern.parse::<u64>().is_ok_and(|limit| size_bytes < limit)
                }
                MatchCondition::InDirectory => dir_lower.starts_with(&rule.pattern.to_lowercase()),
                MatchCondition::AnyFile => true,
            };
            if matched {
                rule.hit_count += 1;
                rule.last_hit_ns = now;
                state.total_matches += 1;
                matches.push((rule.id, rule.action, rule.action_param.clone()));
            }
        }
        Ok(matches)
    })
}

/// Record that a rule was applied.
pub fn record_applied(count: usize) -> KernelResult<()> {
    with_state(|state| {
        state.total_applied += count as u64;
        Ok(())
    })
}

/// List all rules.
pub fn list_rules() -> Vec<FileRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rules.clone())
}

/// Statistics: (rule_count, total_evaluations, total_matches, total_applied, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.rules.len(), s.total_evaluations, s.total_matches, s.total_applied, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("filerules::self_test() — running tests...");
    init_defaults();

    // 1: Default rules.
    assert_eq!(list_rules().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Extension match.
    let matches = evaluate("photo.png", "png", 1000, "/home/user").expect("eval1");
    assert!(!matches.is_empty());
    assert_eq!(matches[0].1, RuleAction::AddTag);
    crate::serial_println!("  [2/8] extension match: OK");

    // 3: Size match.
    let matches = evaluate("huge.log", "log", 20_000_000, "/var/log").expect("eval2");
    assert!(!matches.is_empty());
    assert_eq!(matches[0].1, RuleAction::Compress);
    crate::serial_println!("  [3/8] size match: OK");

    // 4: No match.
    let matches = evaluate("readme.txt", "txt", 100, "/home").expect("eval3");
    assert!(matches.is_empty());
    crate::serial_println!("  [4/8] no match: OK");

    // 5: Add custom rule.
    let rid = add_rule("Backup docs", MatchCondition::ExtensionIs, "doc", RuleAction::CopyTo, "/backup/docs").expect("add");
    let matches = evaluate("report.doc", "doc", 5000, "/work").expect("eval4");
    assert!(matches.iter().any(|m| m.0 == rid));
    crate::serial_println!("  [5/8] custom rule: OK");

    // 6: Disable rule.
    set_enabled(rid, false).expect("disable");
    let matches = evaluate("report.doc", "doc", 5000, "/work").expect("eval5");
    assert!(matches.iter().all(|m| m.0 != rid));
    crate::serial_println!("  [6/8] disable: OK");

    // 7: Remove rule.
    remove_rule(rid).expect("remove");
    assert_eq!(list_rules().len(), 3);
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (rules, evals, matches, _applied, ops) = stats();
    assert_eq!(rules, 3);
    assert!(evals >= 5);
    assert!(matches >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("filerules::self_test() — all 8 tests passed");
}

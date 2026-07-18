//! Usage Time — per-app usage time tracking and reports.
//!
//! Tracks how long each application is used (foreground time),
//! generates daily/weekly reports, and supports usage limits.
//!
//! ## Architecture
//!
//! ```text
//! App gains focus
//!   → usagetime::app_focused(app) → start tracking
//! App loses focus
//!   → usagetime::app_blurred(app) → stop tracking
//!
//! Reports
//!   → usagetime::daily_report() → today's usage
//!   → usagetime::top_apps(n) → most-used apps
//!
//! Integration:
//!   → screentime (screen time limits)
//!   → focussession (focus tracking)
//!   → parental (parental controls)
//!   → notifcenter (usage alerts)
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

/// Per-app usage record.
#[derive(Debug, Clone)]
pub struct AppUsage {
    pub app_name: String,
    pub total_foreground_ms: u64,
    pub session_count: u64,
    pub last_used_ns: u64,
    pub current_session_start: Option<u64>,
    pub daily_limit_ms: Option<u64>,
}

/// Usage category for reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageCategory {
    Productivity,
    Communication,
    Entertainment,
    Social,
    Utility,
    Development,
    Other,
}

impl UsageCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Productivity => "Productivity",
            Self::Communication => "Communication",
            Self::Entertainment => "Entertainment",
            Self::Social => "Social",
            Self::Utility => "Utility",
            Self::Development => "Development",
            Self::Other => "Other",
        }
    }
}

/// Category assignment for an app.
#[derive(Debug, Clone)]
pub struct CategoryAssignment {
    pub app_name: String,
    pub category: UsageCategory,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_APPS: usize = 500;
const MAX_CATEGORIES: usize = 500;

struct State {
    apps: Vec<AppUsage>,
    categories: Vec<CategoryAssignment>,
    tracking_enabled: bool,
    total_sessions: u64,
    total_tracked_ms: u64,
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
        apps: Vec::new(),
        categories: Vec::new(),
        tracking_enabled: true,
        total_sessions: 0,
        total_tracked_ms: 0,
        ops: 0,
    });
}

/// Record app gaining focus.
pub fn app_focused(app_name: &str) -> KernelResult<()> {
    with_state(|state| {
        if !state.tracking_enabled { return Ok(()); }
        let now = crate::hpet::elapsed_ns();
        if let Some(app) = state.apps.iter_mut().find(|a| a.app_name == app_name) {
            if app.current_session_start.is_none() {
                app.current_session_start = Some(now);
                app.session_count += 1;
                state.total_sessions += 1;
            }
        } else {
            if state.apps.len() >= MAX_APPS {
                return Err(KernelError::ResourceExhausted);
            }
            state.apps.push(AppUsage {
                app_name: String::from(app_name),
                total_foreground_ms: 0,
                session_count: 1,
                last_used_ns: now,
                current_session_start: Some(now),
                daily_limit_ms: None,
            });
            state.total_sessions += 1;
        }
        Ok(())
    })
}

/// Record app losing focus.
pub fn app_blurred(app_name: &str) -> KernelResult<u64> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let app = state.apps.iter_mut().find(|a| a.app_name == app_name)
            .ok_or(KernelError::NotFound)?;
        if let Some(start) = app.current_session_start.take() {
            let duration_ms = now.saturating_sub(start) / 1_000_000;
            app.total_foreground_ms += duration_ms;
            app.last_used_ns = now;
            state.total_tracked_ms += duration_ms;
            Ok(duration_ms)
        } else {
            Ok(0)
        }
    })
}

/// Set daily usage limit for an app.
pub fn set_limit(app_name: &str, limit_ms: u64) -> KernelResult<()> {
    with_state(|state| {
        if let Some(app) = state.apps.iter_mut().find(|a| a.app_name == app_name) {
            app.daily_limit_ms = Some(limit_ms);
        } else {
            if state.apps.len() >= MAX_APPS {
                return Err(KernelError::ResourceExhausted);
            }
            state.apps.push(AppUsage {
                app_name: String::from(app_name),
                total_foreground_ms: 0,
                session_count: 0,
                last_used_ns: 0,
                current_session_start: None,
                daily_limit_ms: Some(limit_ms),
            });
        }
        Ok(())
    })
}

/// Remove daily limit.
pub fn remove_limit(app_name: &str) -> KernelResult<()> {
    with_state(|state| {
        let app = state.apps.iter_mut().find(|a| a.app_name == app_name)
            .ok_or(KernelError::NotFound)?;
        app.daily_limit_ms = None;
        Ok(())
    })
}

/// Check if an app is over its limit.
pub fn is_over_limit(app_name: &str) -> bool {
    STATE.lock().as_ref().is_some_and(|s| {
        s.apps.iter().find(|a| a.app_name == app_name).is_some_and(|a| {
            a.daily_limit_ms.is_some_and(|limit| a.total_foreground_ms >= limit)
        })
    })
}

/// Assign a category to an app.
pub fn set_category(app_name: &str, category: UsageCategory) -> KernelResult<()> {
    with_state(|state| {
        if let Some(c) = state.categories.iter_mut().find(|c| c.app_name == app_name) {
            c.category = category;
        } else {
            if state.categories.len() >= MAX_CATEGORIES {
                return Err(KernelError::ResourceExhausted);
            }
            state.categories.push(CategoryAssignment {
                app_name: String::from(app_name),
                category,
            });
        }
        Ok(())
    })
}

/// Enable/disable tracking.
pub fn set_tracking(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.tracking_enabled = enabled;
        Ok(())
    })
}

/// Get top apps by usage time.
pub fn top_apps(max: usize) -> Vec<AppUsage> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut apps = s.apps.clone();
        apps.sort_by_key(|e| core::cmp::Reverse(e.total_foreground_ms));
        apps.truncate(max);
        apps
    })
}

/// Get usage for a specific app.
pub fn get_usage(app_name: &str) -> Option<AppUsage> {
    STATE.lock().as_ref().and_then(|s| s.apps.iter().find(|a| a.app_name == app_name).cloned())
}

/// Get category for an app.
pub fn get_category(app_name: &str) -> Option<UsageCategory> {
    STATE.lock().as_ref().and_then(|s| {
        s.categories.iter().find(|c| c.app_name == app_name).map(|c| c.category)
    })
}

/// Reset all usage data.
pub fn reset_usage() -> KernelResult<()> {
    with_state(|state| {
        for app in &mut state.apps {
            app.total_foreground_ms = 0;
            app.session_count = 0;
            app.current_session_start = None;
        }
        state.total_tracked_ms = 0;
        state.total_sessions = 0;
        Ok(())
    })
}

/// Statistics: (app_count, total_sessions, total_tracked_ms, apps_with_limits, ops).
pub fn stats() -> (usize, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let limited = s.apps.iter().filter(|a| a.daily_limit_ms.is_some()).count();
            (s.apps.len(), s.total_sessions, s.total_tracked_ms, limited, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("usagetime::self_test() — running tests...");
    // Start from a clean, freshly-defaulted state so the assertions below are
    // exact and the tracked apps / limits / categories this test creates do not
    // leak into the live /proc/usagetime table afterward (the kshell
    // `usagetime test` subcommand calls this directly, and /proc/usagetime lists
    // per-app foreground time — leaked fixtures would look like real usage).
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no tracked apps, zeroed counters. init_defaults seeds
    //    only the tracking_enabled flag (config), never fabricated usage records.
    assert_eq!(top_apps(10).len(), 0);
    let (a0, s0, t0, l0, _) = stats();
    assert_eq!((a0, s0, t0, l0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Track app focus — focus then blur records one session for "browser".
    app_focused("browser").expect("focus1");
    app_blurred("browser").expect("blur1");
    let usage = get_usage("browser").expect("get");
    assert_eq!(usage.session_count, 1);
    crate::serial_println!("  [2/8] focus/blur: OK");

    // 3: Multiple sessions — a second browser session plus one editor session.
    app_focused("browser").expect("focus2");
    app_blurred("browser").expect("blur2");
    app_focused("editor").expect("focus3");
    app_blurred("editor").expect("blur3");
    assert_eq!(get_usage("browser").expect("get2").session_count, 2);
    assert_eq!(get_usage("editor").expect("get-ed").session_count, 1);
    crate::serial_println!("  [3/8] multi-session: OK");

    // 4: Top apps — exactly the two tracked apps.
    assert_eq!(top_apps(10).len(), 2);
    crate::serial_println!("  [4/8] top apps: OK");

    // 5: Set limit — 60s daily limit on browser.
    set_limit("browser", 60_000).expect("limit");
    assert!(get_usage("browser").expect("get3").daily_limit_ms.is_some());
    crate::serial_println!("  [5/8] limit: OK");

    // 6: Category — assign and read back.
    set_category("browser", UsageCategory::Productivity).expect("cat");
    assert_eq!(get_category("browser"), Some(UsageCategory::Productivity));
    crate::serial_println!("  [6/8] category: OK");

    // 7: Reset — zeroes per-app usage/sessions (keeps the app entries, their
    //    limits and categories).
    reset_usage().expect("reset");
    let usage = get_usage("browser").expect("get4");
    assert_eq!(usage.session_count, 0);
    assert_eq!(usage.total_foreground_ms, 0);
    crate::serial_println!("  [7/8] reset: OK");

    // 8: Stats — still 2 known apps, 1 with a limit, sessions reset to 0.
    let (apps, sessions, _tracked_ms, limited, ops) = stats();
    assert_eq!((apps, sessions, limited), (2, 0, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Restore the clean default state so no test fixtures (tracked apps, limits,
    // categories) leak into the live module.
    *STATE.lock() = None;
    init_defaults();
    crate::serial_println!("usagetime::self_test() — all 8 tests passed");
}

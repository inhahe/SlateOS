//! Screen time — user activity and app usage tracking.
//!
//! Tracks active/idle time per user, per-app focus duration, and daily
//! usage summaries.  Used by parental controls, wellbeing features,
//! and the settings panel usage dashboard.
//!
//! ## Architecture
//!
//! ```text
//! Compositor focus change
//!   → screentime::app_focus(app_id) → records focus switch
//!
//! Idle detector / input events
//!   → screentime::mark_active() / mark_idle()
//!
//! Settings panel → Digital Wellbeing
//!   → screentime::daily_summary() → usage breakdown
//!
//! Integration:
//!   → parental (time limits enforcement)
//!   → sessionmgr (per-session tracking)
//!   → focusassist (DND mode pauses tracking)
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

/// User activity state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Active,
    Idle,
    Locked,
    Suspended,
}

impl ActivityState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Locked => "locked",
            Self::Suspended => "suspended",
        }
    }
}

/// Per-app usage record.
#[derive(Debug, Clone)]
pub struct AppUsage {
    /// Application identifier.
    pub app_id: String,
    /// Display name.
    pub app_name: String,
    /// Total focus time in seconds.
    pub focus_secs: u64,
    /// Number of times the app gained focus.
    pub focus_count: u32,
    /// Last focus timestamp (ns since boot).
    pub last_focus_ns: u64,
}

/// Daily usage summary.
#[derive(Debug, Clone)]
pub struct DailySummary {
    /// Day index (0 = today, 1 = yesterday, etc.).
    pub day_offset: u32,
    /// Total active seconds.
    pub active_secs: u64,
    /// Total idle seconds.
    pub idle_secs: u64,
    /// Number of app switches.
    pub app_switches: u32,
    /// Most used app.
    pub top_app: String,
    /// Top app focus seconds.
    pub top_app_secs: u64,
}

/// Screen time limits for wellbeing.
#[derive(Debug, Clone)]
pub struct UsageLimits {
    /// Daily limit in minutes (0 = unlimited).
    pub daily_limit_mins: u32,
    /// Reminder interval in minutes (0 = no reminders).
    pub reminder_interval_mins: u32,
    /// Bedtime start hour (0-23, or 255 for disabled).
    pub bedtime_start_hour: u8,
    /// Bedtime end hour.
    pub bedtime_end_hour: u8,
    /// Whether to show usage notifications.
    pub show_notifications: bool,
}

impl Default for UsageLimits {
    fn default() -> Self {
        Self {
            daily_limit_mins: 0,
            reminder_interval_mins: 0,
            bedtime_start_hour: 255,
            bedtime_end_hour: 255,
            show_notifications: true,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_APPS: usize = 200;
const MAX_DAILY_HISTORY: usize = 30;

struct State {
    enabled: bool,
    activity: ActivityState,
    /// Per-app usage data.
    apps: Vec<AppUsage>,
    /// Currently focused app.
    current_app: String,
    /// Timestamp when current app gained focus (ns).
    focus_start_ns: u64,
    /// Total active seconds today.
    active_secs_today: u64,
    /// Total idle seconds today.
    idle_secs_today: u64,
    /// App switches today.
    switches_today: u32,
    /// Daily history summaries.
    daily_history: Vec<DailySummary>,
    /// Usage limits.
    limits: UsageLimits,
    /// Total app focus events.
    total_focus_events: u64,
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
        enabled: true,
        activity: ActivityState::Active,
        apps: Vec::new(),
        current_app: String::new(),
        focus_start_ns: 0,
        active_secs_today: 0,
        idle_secs_today: 0,
        switches_today: 0,
        daily_history: Vec::new(),
        limits: UsageLimits::default(),
        total_focus_events: 0,
        ops: 0,
    });
}

/// Set tracking enabled/disabled.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.enabled = enabled; Ok(()) })
}

pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.enabled)
}

/// Record that an app gained focus.
pub fn app_focus(app_id: &str, app_name: &str) -> KernelResult<()> {
    with_state(|state| {
        if !state.enabled { return Ok(()); }

        let now = crate::hpet::elapsed_ns();

        // Close out previous app's focus time.
        if !state.current_app.is_empty() && state.focus_start_ns > 0 {
            let elapsed_secs = (now.saturating_sub(state.focus_start_ns)) / 1_000_000_000;
            if let Some(app) = state.apps.iter_mut().find(|a| a.app_id == state.current_app) {
                app.focus_secs += elapsed_secs;
            }
        }

        // Update or create app record.
        if let Some(app) = state.apps.iter_mut().find(|a| a.app_id == app_id) {
            app.focus_count += 1;
            app.last_focus_ns = now;
        } else {
            if state.apps.len() >= MAX_APPS {
                // Remove least-used app.
                if let Some(min_idx) = state.apps.iter().enumerate()
                    .min_by_key(|(_, a)| a.focus_secs)
                    .map(|(i, _)| i)
                {
                    state.apps.remove(min_idx);
                }
            }
            state.apps.push(AppUsage {
                app_id: String::from(app_id),
                app_name: String::from(app_name),
                focus_secs: 0,
                focus_count: 1,
                last_focus_ns: now,
            });
        }

        state.current_app = String::from(app_id);
        state.focus_start_ns = now;
        state.switches_today += 1;
        state.total_focus_events += 1;

        Ok(())
    })
}

/// Mark user as active (input detected).
pub fn mark_active() -> KernelResult<()> {
    with_state(|state| {
        state.activity = ActivityState::Active;
        Ok(())
    })
}

/// Mark user as idle (no input for threshold).
pub fn mark_idle() -> KernelResult<()> {
    with_state(|state| {
        state.activity = ActivityState::Idle;
        Ok(())
    })
}

/// Get current activity state.
pub fn activity_state() -> ActivityState {
    STATE.lock().as_ref().map_or(ActivityState::Active, |s| s.activity)
}

/// Add active time (called periodically by the idle detector).
pub fn add_active_time(secs: u64) -> KernelResult<()> {
    with_state(|state| {
        if state.activity == ActivityState::Active {
            state.active_secs_today += secs;
        } else {
            state.idle_secs_today += secs;
        }
        Ok(())
    })
}

/// Get per-app usage sorted by focus time (descending).
pub fn app_usage() -> Vec<AppUsage> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let mut apps = s.apps.clone();
            apps.sort_by_key(|e| core::cmp::Reverse(e.focus_secs));
            apps
        }
        None => Vec::new(),
    }
}

/// Get today's summary.
pub fn today_summary() -> DailySummary {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let top = s.apps.iter().max_by_key(|a| a.focus_secs);
            DailySummary {
                day_offset: 0,
                active_secs: s.active_secs_today,
                idle_secs: s.idle_secs_today,
                app_switches: s.switches_today,
                top_app: top.map_or(String::from("none"), |a| a.app_name.clone()),
                top_app_secs: top.map_or(0, |a| a.focus_secs),
            }
        }
        None => DailySummary {
            day_offset: 0, active_secs: 0, idle_secs: 0,
            app_switches: 0, top_app: String::from("none"), top_app_secs: 0,
        },
    }
}

/// Set daily usage limit.
pub fn set_daily_limit(minutes: u32) -> KernelResult<()> {
    with_state(|state| { state.limits.daily_limit_mins = minutes; Ok(()) })
}

/// Set reminder interval.
pub fn set_reminder_interval(minutes: u32) -> KernelResult<()> {
    with_state(|state| { state.limits.reminder_interval_mins = minutes; Ok(()) })
}

/// Get usage limits.
pub fn get_limits() -> UsageLimits {
    STATE.lock().as_ref().map_or(UsageLimits::default(), |s| s.limits.clone())
}

/// Check if daily limit is exceeded.
pub fn limit_exceeded() -> bool {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            if s.limits.daily_limit_mins == 0 { return false; }
            let active_mins = s.active_secs_today / 60;
            active_mins >= s.limits.daily_limit_mins as u64
        }
        None => false,
    }
}

/// Reset daily counters (called at midnight by the timer subsystem).
pub fn reset_daily() -> KernelResult<()> {
    with_state(|state| {
        let top = state.apps.iter().max_by_key(|a| a.focus_secs);
        let summary = DailySummary {
            day_offset: 0,
            active_secs: state.active_secs_today,
            idle_secs: state.idle_secs_today,
            app_switches: state.switches_today,
            top_app: top.map_or(String::from("none"), |a| a.app_name.clone()),
            top_app_secs: top.map_or(0, |a| a.focus_secs),
        };

        // Shift existing history.
        for h in state.daily_history.iter_mut() {
            h.day_offset += 1;
        }
        state.daily_history.insert(0, summary);
        while state.daily_history.len() > MAX_DAILY_HISTORY {
            state.daily_history.pop();
        }

        // Reset today's counters.
        state.active_secs_today = 0;
        state.idle_secs_today = 0;
        state.switches_today = 0;
        for app in state.apps.iter_mut() {
            app.focus_secs = 0;
            app.focus_count = 0;
        }

        Ok(())
    })
}

/// Statistics: (app_count, active_secs_today, idle_secs_today, switches, focus_events, ops).
pub fn stats() -> (usize, u64, u64, u32, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.apps.len(), s.active_secs_today, s.idle_secs_today,
            s.switches_today, s.total_focus_events, s.ops,
        ),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("screentime::self_test() — running tests...");
    // Start from a clean, freshly-defaulted state so the assertions below are
    // exact and the tracked apps / daily-history / limit fixtures this test
    // creates do not leak into the live /proc/screentime table afterward (the
    // kshell `screentime test` subcommand calls this directly, and
    // /proc/screentime reports tracked_apps / focus events — leaked fixtures
    // would look like real activity).
    *STATE.lock() = None;
    init_defaults();

    // 1: Enabled by default (config), with NO fabricated apps — init_defaults
    //    seeds only the enabled flag, Active state and default (unlimited) limits.
    assert!(is_enabled());
    assert_eq!(app_usage().len(), 0);
    let (a0, ac0, id0, sw0, fe0, _) = stats();
    assert_eq!((a0, ac0, id0, sw0, fe0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/11] clean defaults: OK");

    // 2: Activity state starts Active.
    assert_eq!(activity_state(), ActivityState::Active);
    crate::serial_println!("  [2/11] activity state: OK");

    // 3: App focus — two distinct apps tracked.
    app_focus("org.editor", "Text Editor").expect("focus 1");
    app_focus("org.browser", "Web Browser").expect("focus 2");
    assert_eq!(app_usage().len(), 2);
    crate::serial_println!("  [3/11] app focus: OK");

    // 4: Focus count — re-focusing the editor bumps its count to 2.
    app_focus("org.editor", "Text Editor").expect("focus 3");
    let apps = app_usage();
    let editor = apps.iter().find(|a| a.app_id == "org.editor").expect("find editor");
    assert_eq!(editor.focus_count, 2);
    crate::serial_println!("  [4/11] focus count: OK");

    // 5: Mark idle.
    mark_idle().expect("mark idle");
    assert_eq!(activity_state(), ActivityState::Idle);
    crate::serial_println!("  [5/11] mark idle: OK");

    // 6: Mark active.
    mark_active().expect("mark active");
    assert_eq!(activity_state(), ActivityState::Active);
    crate::serial_println!("  [6/11] mark active: OK");

    // 7: Add active time — 120s accrues to today's active total.
    add_active_time(120).expect("add time");
    let (_, active, _, _, _, _) = stats();
    assert_eq!(active, 120);
    crate::serial_println!("  [7/11] add active time: OK");

    // 8: Today's summary — 120 active secs and 3 app switches (3 app_focus calls).
    let summary = today_summary();
    assert_eq!(summary.active_secs, 120);
    assert_eq!(summary.app_switches, 3);
    crate::serial_println!("  [8/11] today summary: OK");

    // 9: Daily limit — 60-min limit not exceeded by 120s (= 2 min) of activity.
    set_daily_limit(60).expect("set limit");
    assert!(!limit_exceeded());
    crate::serial_println!("  [9/11] daily limit: OK");

    // 10: Reset daily — today's counters zero out (app entries are kept, with
    //     their focus_secs/count reset).
    reset_daily().expect("reset");
    let (_, active, _, switches, _, _) = stats();
    assert_eq!((active, switches), (0, 0));
    crate::serial_println!("  [10/11] reset daily: OK");

    // 11: Stats — still 2 known apps, exactly 3 focus events recorded.
    let (apps, _, _, _, focus_events, ops) = stats();
    assert_eq!((apps, focus_events), (2, 3));
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    // Restore the clean default state so no test fixtures (tracked apps, daily
    // history, limit) leak into the live module.
    *STATE.lock() = None;
    init_defaults();
    crate::serial_println!("screentime::self_test() — all 11 tests passed");
}

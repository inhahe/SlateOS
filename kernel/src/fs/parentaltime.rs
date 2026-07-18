//! Parental time — time-based usage restrictions.
//!
//! Manages daily and weekly time allowances for user accounts,
//! enforces scheduled downtime periods, and tracks usage against
//! configured limits. Complements the basic parental controls module.
//!
//! ## Architecture
//!
//! ```text
//! User session active
//!   → parentaltime::tick(user, seconds) → update usage
//!   → parentaltime::check_allowed(user) → allow/deny
//!
//! Settings panel → Parental Controls → Time Limits
//!   → parentaltime::set_daily_limit(user, minutes)
//!   → parentaltime::set_schedule(user, day, start, end)
//!
//! Integration:
//!   → parental (content filtering, app restrictions)
//!   → sessionmgr (session enforcement)
//!   → loginscreen (block login outside hours)
//!   → notifcenter (time warnings)
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

/// Day of week.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayOfWeek {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl DayOfWeek {
    pub fn label(self) -> &'static str {
        match self {
            Self::Monday => "Mon",
            Self::Tuesday => "Tue",
            Self::Wednesday => "Wed",
            Self::Thursday => "Thu",
            Self::Friday => "Fri",
            Self::Saturday => "Sat",
            Self::Sunday => "Sun",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Monday => 0,
            Self::Tuesday => 1,
            Self::Wednesday => 2,
            Self::Thursday => 3,
            Self::Friday => 4,
            Self::Saturday => 5,
            Self::Sunday => 6,
        }
    }
}

/// Time limit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitStatus {
    /// Within limits.
    Allowed,
    /// Near limit (within 15 min).
    Warning,
    /// Limit exceeded.
    Exceeded,
    /// Outside allowed schedule.
    OutsideSchedule,
    /// No limits configured.
    Unlimited,
}

impl LimitStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allowed => "Allowed",
            Self::Warning => "Warning",
            Self::Exceeded => "Exceeded",
            Self::OutsideSchedule => "Outside Schedule",
            Self::Unlimited => "Unlimited",
        }
    }
}

/// A time schedule window (allowed hours).
#[derive(Debug, Clone, Copy)]
pub struct ScheduleWindow {
    /// Start hour (0-23).
    pub start_hour: u8,
    /// Start minute (0-59).
    pub start_minute: u8,
    /// End hour (0-23).
    pub end_hour: u8,
    /// End minute (0-59).
    pub end_minute: u8,
}

/// Per-user time configuration.
#[derive(Debug, Clone)]
pub struct UserTimeConfig {
    /// Config ID.
    pub id: u32,
    /// Username.
    pub username: String,
    /// Daily limit in minutes (0 = unlimited).
    pub daily_limit_minutes: u32,
    /// Weekly limit in minutes (0 = unlimited).
    pub weekly_limit_minutes: u32,
    /// Schedule windows per day (7 days).
    pub schedule: [Option<ScheduleWindow>; 7],
    /// Minutes used today.
    pub used_today_minutes: u32,
    /// Minutes used this week.
    pub used_week_minutes: u32,
    /// Whether enforcement is active.
    pub active: bool,
    /// Warning threshold (minutes before limit).
    pub warning_minutes: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CONFIGS: usize = 50;

struct State {
    configs: Vec<UserTimeConfig>,
    next_id: u32,
    total_enforcements: u64,
    total_warnings: u64,
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
        configs: Vec::new(),
        next_id: 1,
        total_enforcements: 0,
        total_warnings: 0,
        ops: 0,
    });
}

/// Create a time config for a user.
pub fn create_config(username: &str, daily_limit_min: u32, weekly_limit_min: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.configs.len() >= MAX_CONFIGS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.configs.iter().any(|c| c.username == username) {
            return Err(KernelError::AlreadyExists);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.configs.push(UserTimeConfig {
            id, username: String::from(username),
            daily_limit_minutes: daily_limit_min,
            weekly_limit_minutes: weekly_limit_min,
            schedule: [None; 7],
            used_today_minutes: 0, used_week_minutes: 0,
            active: true, warning_minutes: 15,
        });
        Ok(id)
    })
}

/// Set a schedule window for a day.
pub fn set_schedule(id: u32, day: DayOfWeek, start_hour: u8, start_min: u8, end_hour: u8, end_min: u8) -> KernelResult<()> {
    with_state(|state| {
        let config = state.configs.iter_mut().find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        if start_hour > 23 || end_hour > 23 || start_min > 59 || end_min > 59 {
            return Err(KernelError::InvalidArgument);
        }
        config.schedule[day.index()] = Some(ScheduleWindow {
            start_hour, start_minute: start_min,
            end_hour, end_minute: end_min,
        });
        Ok(())
    })
}

/// Clear schedule for a day.
pub fn clear_schedule(id: u32, day: DayOfWeek) -> KernelResult<()> {
    with_state(|state| {
        let config = state.configs.iter_mut().find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        config.schedule[day.index()] = None;
        Ok(())
    })
}

/// Set daily limit.
pub fn set_daily_limit(id: u32, minutes: u32) -> KernelResult<()> {
    with_state(|state| {
        let config = state.configs.iter_mut().find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        config.daily_limit_minutes = minutes;
        Ok(())
    })
}

/// Record usage time.
pub fn record_usage(id: u32, minutes: u32) -> KernelResult<LimitStatus> {
    with_state(|state| {
        let config = state.configs.iter_mut().find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;

        if !config.active {
            return Ok(LimitStatus::Unlimited);
        }

        config.used_today_minutes += minutes;
        config.used_week_minutes += minutes;

        if config.daily_limit_minutes > 0 && config.used_today_minutes >= config.daily_limit_minutes {
            state.total_enforcements += 1;
            return Ok(LimitStatus::Exceeded);
        }
        if config.weekly_limit_minutes > 0 && config.used_week_minutes >= config.weekly_limit_minutes {
            state.total_enforcements += 1;
            return Ok(LimitStatus::Exceeded);
        }
        if config.daily_limit_minutes > 0 {
            let remaining = config.daily_limit_minutes.saturating_sub(config.used_today_minutes);
            if remaining <= config.warning_minutes {
                state.total_warnings += 1;
                return Ok(LimitStatus::Warning);
            }
        }
        Ok(LimitStatus::Allowed)
    })
}

/// Reset daily usage.
pub fn reset_daily(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let config = state.configs.iter_mut().find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        config.used_today_minutes = 0;
        Ok(())
    })
}

/// Reset weekly usage.
pub fn reset_weekly(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let config = state.configs.iter_mut().find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        config.used_today_minutes = 0;
        config.used_week_minutes = 0;
        Ok(())
    })
}

/// Enable/disable enforcement.
pub fn set_active(id: u32, active: bool) -> KernelResult<()> {
    with_state(|state| {
        let config = state.configs.iter_mut().find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        config.active = active;
        Ok(())
    })
}

/// Get config by ID.
pub fn get_config(id: u32) -> KernelResult<UserTimeConfig> {
    with_state(|state| {
        state.configs.iter().find(|c| c.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Find config by username.
pub fn find_by_user(username: &str) -> KernelResult<UserTimeConfig> {
    with_state(|state| {
        state.configs.iter().find(|c| c.username == username).cloned().ok_or(KernelError::NotFound)
    })
}

/// List all configs.
pub fn list_configs() -> Vec<UserTimeConfig> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.configs.clone())
}

/// Remove a config.
pub fn remove_config(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.configs.iter().position(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        state.configs.remove(pos);
        Ok(())
    })
}

/// Statistics: (config_count, total_enforcements, total_warnings, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.configs.len(), s.total_enforcements, s.total_warnings, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("parentaltime::self_test() — running tests...");
    init_defaults();

    // 1: Empty initial.
    assert!(list_configs().is_empty());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Create config.
    let id = create_config("child1", 120, 600).expect("create");
    assert!(id > 0);
    crate::serial_println!("  [2/11] create config: OK");

    // 3: Get config.
    let cfg = get_config(id).expect("get");
    assert_eq!(cfg.username, "child1");
    assert_eq!(cfg.daily_limit_minutes, 120);
    crate::serial_println!("  [3/11] get config: OK");

    // 4: Record usage - allowed.
    let status = record_usage(id, 60).expect("record");
    assert_eq!(status, LimitStatus::Allowed);
    crate::serial_println!("  [4/11] usage allowed: OK");

    // 5: Near limit warning.
    let status = record_usage(id, 50).expect("record2");
    assert_eq!(status, LimitStatus::Warning);
    crate::serial_println!("  [5/11] warning: OK");

    // 6: Exceeded limit.
    let status = record_usage(id, 20).expect("record3");
    assert_eq!(status, LimitStatus::Exceeded);
    crate::serial_println!("  [6/11] exceeded: OK");

    // 7: Reset daily.
    reset_daily(id).expect("reset");
    let cfg = get_config(id).expect("get2");
    assert_eq!(cfg.used_today_minutes, 0);
    crate::serial_println!("  [7/11] reset daily: OK");

    // 8: Set schedule.
    set_schedule(id, DayOfWeek::Monday, 8, 0, 20, 0).expect("schedule");
    let cfg = get_config(id).expect("get3");
    assert!(cfg.schedule[0].is_some());
    let win = cfg.schedule[0].unwrap();
    assert_eq!(win.start_hour, 8);
    assert_eq!(win.end_hour, 20);
    crate::serial_println!("  [8/11] set schedule: OK");

    // 9: Duplicate user rejected.
    let r = create_config("child1", 60, 300);
    assert!(r.is_err());
    crate::serial_println!("  [9/11] duplicate rejected: OK");

    // 10: Disable enforcement.
    set_active(id, false).expect("disable");
    let status = record_usage(id, 999).expect("record4");
    assert_eq!(status, LimitStatus::Unlimited);
    crate::serial_println!("  [10/11] disable: OK");

    // 11: Stats.
    let (count, enforcements, warnings, ops) = stats();
    assert_eq!(count, 1);
    assert!(enforcements >= 1);
    assert!(warnings >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("parentaltime::self_test() — all 11 tests passed");
}

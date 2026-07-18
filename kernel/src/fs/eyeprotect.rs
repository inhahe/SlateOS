//! Eye Protection — break reminders and eye strain reduction.
//!
//! Implements the 20-20-20 rule (every 20 minutes, look at something 20 feet
//! away for 20 seconds) and configurable break scheduling for eye health.
//!
//! ## Architecture
//!
//! ```text
//! Timer expires
//!   → eyeprotect::check_break() → show break reminder
//!   → eyeprotect::start_break() → dim screen / show overlay
//!
//! Configuration
//!   → eyeprotect::set_interval(minutes)
//!   → eyeprotect::set_break_duration(seconds)
//!
//! Integration:
//!   → nightlight (blue light reduction)
//!   → brightness (screen dimming during breaks)
//!   → notifcenter (break notifications)
//!   → focusassist (respect DND mode)
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

/// Break reminder mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderMode {
    /// Notification only.
    Notification,
    /// Dim screen overlay.
    DimOverlay,
    /// Full screen break screen.
    FullScreen,
    /// Subtle taskbar indicator.
    Indicator,
}

impl ReminderMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Notification => "Notification",
            Self::DimOverlay => "Dim Overlay",
            Self::FullScreen => "Full Screen",
            Self::Indicator => "Indicator",
        }
    }
}

/// Break state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakState {
    Working,
    BreakDue,
    OnBreak,
    Snoozed,
}

impl BreakState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::BreakDue => "Break Due",
            Self::OnBreak => "On Break",
            Self::Snoozed => "Snoozed",
        }
    }
}

/// Eye protection profile.
#[derive(Debug, Clone)]
pub struct EyeProfile {
    pub id: u32,
    pub name: String,
    /// Interval between breaks in minutes.
    pub interval_mins: u32,
    /// Break duration in seconds.
    pub break_duration_secs: u32,
    pub reminder_mode: ReminderMode,
    /// Snooze duration in minutes.
    pub snooze_mins: u32,
    /// Dim brightness during break (0-100).
    pub break_brightness: u32,
    /// Respect focus assist / DND.
    pub respect_dnd: bool,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROFILES: usize = 10;

struct State {
    profiles: Vec<EyeProfile>,
    active_profile_id: u32,
    break_state: BreakState,
    next_id: u32,
    /// Timestamp of last break end (nanoseconds).
    last_break_ns: u64,
    /// Timestamp of current break start.
    break_start_ns: u64,
    total_breaks: u64,
    total_snoozes: u64,
    total_skips: u64,
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
        profiles: alloc::vec![
            EyeProfile {
                id: 1,
                name: String::from("20-20-20"),
                interval_mins: 20,
                break_duration_secs: 20,
                reminder_mode: ReminderMode::Notification,
                snooze_mins: 5,
                break_brightness: 30,
                respect_dnd: true,
                enabled: true,
            },
            EyeProfile {
                id: 2,
                name: String::from("Hourly"),
                interval_mins: 60,
                break_duration_secs: 300,
                reminder_mode: ReminderMode::DimOverlay,
                snooze_mins: 10,
                break_brightness: 20,
                respect_dnd: true,
                enabled: false,
            },
        ],
        active_profile_id: 1,
        break_state: BreakState::Working,
        next_id: 3,
        last_break_ns: 0,
        break_start_ns: 0,
        total_breaks: 0,
        total_snoozes: 0,
        total_skips: 0,
        ops: 0,
    });
}

/// Check if a break is due.
pub fn check_break() -> KernelResult<BreakState> {
    with_state(|state| {
        let profile = state.profiles.iter().find(|p| p.id == state.active_profile_id);
        let profile = match profile {
            Some(p) if p.enabled => p,
            _ => return Ok(BreakState::Working),
        };

        if state.break_state == BreakState::OnBreak {
            return Ok(BreakState::OnBreak);
        }

        let now = crate::hpet::elapsed_ns();
        let interval_ns = (profile.interval_mins as u64) * 60 * 1_000_000_000;
        let elapsed = now.saturating_sub(state.last_break_ns);

        if elapsed >= interval_ns {
            state.break_state = BreakState::BreakDue;
        }
        Ok(state.break_state)
    })
}

/// Start a break.
pub fn start_break() -> KernelResult<()> {
    with_state(|state| {
        state.break_state = BreakState::OnBreak;
        state.break_start_ns = crate::hpet::elapsed_ns();
        state.total_breaks += 1;
        Ok(())
    })
}

/// End a break.
pub fn end_break() -> KernelResult<()> {
    with_state(|state| {
        state.break_state = BreakState::Working;
        state.last_break_ns = crate::hpet::elapsed_ns();
        Ok(())
    })
}

/// Snooze the break reminder.
pub fn snooze() -> KernelResult<()> {
    with_state(|state| {
        state.break_state = BreakState::Snoozed;
        state.total_snoozes += 1;
        // Extend last_break_ns by snooze duration.
        let profile = state.profiles.iter().find(|p| p.id == state.active_profile_id);
        if let Some(p) = profile {
            let snooze_ns = (p.snooze_mins as u64) * 60 * 1_000_000_000;
            state.last_break_ns = crate::hpet::elapsed_ns().saturating_sub(
                (p.interval_mins as u64) * 60 * 1_000_000_000
            ).saturating_add(snooze_ns);
        }
        Ok(())
    })
}

/// Skip the break.
pub fn skip() -> KernelResult<()> {
    with_state(|state| {
        state.break_state = BreakState::Working;
        state.last_break_ns = crate::hpet::elapsed_ns();
        state.total_skips += 1;
        Ok(())
    })
}

/// Set break interval.
pub fn set_interval(profile_id: u32, minutes: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.interval_mins = minutes.clamp(1, 120);
        Ok(())
    })
}

/// Set break duration.
pub fn set_break_duration(profile_id: u32, seconds: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.break_duration_secs = seconds.clamp(5, 1800);
        Ok(())
    })
}

/// Set active profile.
pub fn set_active(profile_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.profiles.iter().any(|p| p.id == profile_id) {
            return Err(KernelError::NotFound);
        }
        state.active_profile_id = profile_id;
        Ok(())
    })
}

/// Enable/disable a profile.
pub fn set_profile_enabled(profile_id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.enabled = enabled;
        Ok(())
    })
}

/// Get current break state.
pub fn break_state() -> BreakState {
    STATE.lock().as_ref().map_or(BreakState::Working, |s| s.break_state)
}

/// List profiles.
pub fn list_profiles() -> Vec<EyeProfile> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.profiles.clone())
}

/// Get active profile.
pub fn get_active() -> Option<EyeProfile> {
    STATE.lock().as_ref().and_then(|s| {
        s.profiles.iter().find(|p| p.id == s.active_profile_id).cloned()
    })
}

/// Statistics: (profile_count, total_breaks, total_snoozes, total_skips, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.profiles.len(), s.total_breaks, s.total_snoozes, s.total_skips, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("eyeprotect::self_test() — running tests...");
    // Start from a clean, freshly-defaulted state so the assertions below are
    // exact and the break/snooze/skip activity counters and the interval change
    // this test makes do not leak into the live /proc/eyeprotect table afterward
    // (the kshell `eyeprotect test` subcommand calls this directly, and a leak
    // would both report fabricated break activity and corrupt the shipped
    // 20-20-20 default profile's interval).
    *STATE.lock() = None;
    init_defaults();

    // 1: Default profiles — these are CONFIGURATION (shipped break-reminder
    //    presets), not fabricated observations: their activity counters
    //    (total_breaks/snoozes/skips) all start at 0.
    let profiles = list_profiles();
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].name, "20-20-20");
    assert_eq!(profiles[0].interval_mins, 20);
    let (_, b0, sn0, sk0, _) = stats();
    assert_eq!((b0, sn0, sk0), (0, 0, 0));
    crate::serial_println!("  [1/8] default profiles (zeroed activity): OK");

    // 2: Active profile.
    let active = get_active().expect("active");
    assert_eq!(active.name, "20-20-20");
    assert!(active.enabled);
    crate::serial_println!("  [2/8] active profile: OK");

    // 3: Start break.
    start_break().expect("start");
    assert_eq!(break_state(), BreakState::OnBreak);
    crate::serial_println!("  [3/8] start break: OK");

    // 4: End break.
    end_break().expect("end");
    assert_eq!(break_state(), BreakState::Working);
    crate::serial_println!("  [4/8] end break: OK");

    // 5: Snooze.
    start_break().expect("start2");
    snooze().expect("snooze");
    assert_eq!(break_state(), BreakState::Snoozed);
    crate::serial_println!("  [5/8] snooze: OK");

    // 6: Skip.
    skip().expect("skip");
    assert_eq!(break_state(), BreakState::Working);
    crate::serial_println!("  [6/8] skip: OK");

    // 7: Set interval.
    set_interval(1, 30).expect("interval");
    let p = get_active().expect("active2");
    assert_eq!(p.interval_mins, 30);
    crate::serial_println!("  [7/8] set interval: OK");

    // 8: Stats — 2 breaks started, 1 snooze, 1 skip.
    let (profiles, breaks, snoozes, skips, ops) = stats();
    assert_eq!((profiles, breaks, snoozes, skips), (2, 2, 1, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Restore the clean default config/state so no test fixtures (break activity
    // counters, the mutated 20-20-20 interval) leak into the live module.
    *STATE.lock() = None;
    init_defaults();
    crate::serial_println!("eyeprotect::self_test() — all 8 tests passed");
}

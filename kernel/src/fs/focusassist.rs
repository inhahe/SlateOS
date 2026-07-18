//! Focus Assist / Do Not Disturb mode.
//!
//! Provides system-wide notification suppression based on user-defined
//! profiles, time-based schedules, and activity triggers.  Modern
//! desktop OSes (Windows Focus Assist, macOS Focus, Android DND, iOS
//! Focus) all include this feature — it's essential for a usable desktop.
//!
//! ## Architecture
//!
//! ```text
//! notifcenter::send(notification)
//!   → focusassist::should_suppress(app_id, priority)
//!       → checks active profile, priority apps, schedule
//!       → returns true/false
//!   → if suppressed → record_missed()
//!   → if not suppressed → deliver normally
//!
//! Settings panel → Focus Assist
//!   → focusassist::list_profiles()
//!   → focusassist::activate(profile_id)
//!   → focusassist::deactivate()
//!
//! System tray
//!   → focusassist::is_active() → show moon/DND icon
//!   → focusassist::missed_count() → badge count
//! ```
//!
//! ## Integration Points
//!
//! - **notifcenter**: calls `should_suppress()` before delivering
//! - **appnotify**: priority apps use app_id from appnotify registry
//! - **systray**: displays DND icon and missed count badge
//! - **power**: can auto-activate on presentation/gaming mode

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_PROFILES: usize = 32;
const MAX_PRIORITY_APPS: usize = 64;
const MAX_MISSED: usize = 512;
const MAX_SCHEDULES: usize = 16;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How aggressively to suppress notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusMode {
    /// Only priority apps break through.
    PriorityOnly,
    /// Only alarms and timers break through.
    AlarmsOnly,
    /// Complete silence — nothing breaks through.
    Total,
}

/// What triggered the current focus session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    /// User manually activated via settings or hotkey.
    Manual,
    /// Time-based schedule.
    Schedule,
    /// Fullscreen application detected.
    FullscreenApp,
    /// Presentation mode (external display detected).
    Presentation,
    /// Gaming activity detected.
    Gaming,
}

/// Time-based schedule for automatic focus activation.
#[derive(Debug, Clone)]
pub struct FocusSchedule {
    pub id: u64,
    pub name: String,
    pub enabled: bool,
    /// Which days of the week (0=Sunday through 6=Saturday).
    pub days: [bool; 7],
    pub start_hour: u8,
    pub start_minute: u8,
    pub end_hour: u8,
    pub end_minute: u8,
    /// Which profile to activate.
    pub profile_id: u64,
}

/// A named focus profile with suppression rules.
#[derive(Debug, Clone)]
pub struct FocusProfile {
    pub id: u64,
    pub name: String,
    pub mode: FocusMode,
    /// App IDs that can always break through in PriorityOnly mode.
    pub priority_apps: Vec<String>,
    /// Allow alarm-type notifications even in Total mode.
    pub allow_alarms: bool,
    /// Allow reminder-type notifications.
    pub allow_reminders: bool,
    /// Show summary of missed notifications when focus ends.
    pub show_summary: bool,
    /// Optional auto-reply message for messaging apps.
    pub auto_reply: Option<String>,
    /// Whether this is a built-in profile (cannot be deleted).
    pub builtin: bool,
    /// Whether this profile is enabled (can be activated).
    pub enabled: bool,
}

/// Priority level for notification suppression decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotifPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
    Alarm = 4,
}

/// A notification that was suppressed during a focus session.
#[derive(Debug, Clone)]
pub struct MissedNotification {
    pub id: u64,
    pub app_id: String,
    pub title: String,
    pub priority: NotifPriority,
    pub timestamp_ns: u64,
}

/// Result of a suppression check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressResult {
    /// Deliver notification normally.
    Allow,
    /// Suppress and record as missed.
    Suppress,
    /// Suppress silently (don't even record).
    SuppressSilent,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct FocusState {
    profiles: Vec<FocusProfile>,
    schedules: Vec<FocusSchedule>,
    /// Currently active profile ID (None = focus off).
    active_profile_id: Option<u64>,
    /// What triggered the current session.
    trigger: TriggerKind,
    /// When the current session started (ns).
    activated_at_ns: u64,
    /// Missed notifications during current session.
    missed: Vec<MissedNotification>,
    /// Total missed notifications across all sessions.
    total_missed: u64,
    /// Total sessions activated.
    total_sessions: u64,
    /// Auto-activate on fullscreen app.
    auto_fullscreen: bool,
    /// Auto-activate on gaming activity.
    auto_gaming: bool,
    /// Auto-activate on presentation mode.
    auto_presentation: bool,
    /// Default profile for auto-triggers.
    auto_profile_id: u64,
    /// Next ID for profiles.
    next_profile_id: u64,
    /// Next ID for schedules.
    next_schedule_id: u64,
    /// Next ID for missed notifications.
    next_missed_id: u64,
    /// Operation counter.
    ops: u64,
}

static STATE: Mutex<Option<FocusState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut FocusState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize focus assist with built-in profiles.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let profiles = vec![
        FocusProfile {
            id: 1,
            name: String::from("Priority Only"),
            mode: FocusMode::PriorityOnly,
            priority_apps: Vec::new(),
            allow_alarms: true,
            allow_reminders: true,
            show_summary: true,
            auto_reply: None,
            builtin: true,
            enabled: true,
        },
        FocusProfile {
            id: 2,
            name: String::from("Alarms Only"),
            mode: FocusMode::AlarmsOnly,
            priority_apps: Vec::new(),
            allow_alarms: true,
            allow_reminders: false,
            show_summary: true,
            auto_reply: None,
            builtin: true,
            enabled: true,
        },
        FocusProfile {
            id: 3,
            name: String::from("Do Not Disturb"),
            mode: FocusMode::Total,
            priority_apps: Vec::new(),
            allow_alarms: false,
            allow_reminders: false,
            show_summary: true,
            auto_reply: Some(String::from("I'm currently unavailable.")),
            builtin: true,
            enabled: true,
        },
        FocusProfile {
            id: 4,
            name: String::from("Gaming"),
            mode: FocusMode::PriorityOnly,
            priority_apps: Vec::new(),
            allow_alarms: true,
            allow_reminders: false,
            show_summary: true,
            auto_reply: None,
            builtin: true,
            enabled: true,
        },
        FocusProfile {
            id: 5,
            name: String::from("Presenting"),
            mode: FocusMode::Total,
            priority_apps: Vec::new(),
            allow_alarms: false,
            allow_reminders: false,
            show_summary: true,
            auto_reply: Some(String::from("In a presentation right now.")),
            builtin: true,
            enabled: true,
        },
        FocusProfile {
            id: 6,
            name: String::from("Sleeping"),
            mode: FocusMode::AlarmsOnly,
            priority_apps: Vec::new(),
            allow_alarms: true,
            allow_reminders: false,
            show_summary: true,
            auto_reply: None,
            builtin: true,
            enabled: true,
        },
    ];

    // Default schedule: quiet hours 10 PM - 7 AM weekdays
    let schedules = vec![
        FocusSchedule {
            id: 1,
            name: String::from("Quiet Hours"),
            enabled: false,  // Off by default per design: few things enabled by default
            days: [false, true, true, true, true, true, false], // Mon-Fri
            start_hour: 22,
            start_minute: 0,
            end_hour: 7,
            end_minute: 0,
            profile_id: 6, // Sleeping
        },
    ];

    *guard = Some(FocusState {
        profiles,
        schedules,
        active_profile_id: None,
        trigger: TriggerKind::Manual,
        activated_at_ns: 0,
        missed: Vec::new(),
        total_missed: 0,
        total_sessions: 0,
        auto_fullscreen: false,
        auto_gaming: false,
        auto_presentation: false,
        auto_profile_id: 1, // Priority Only
        next_profile_id: 7,
        next_schedule_id: 2,
        next_missed_id: 1,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Core API — suppression check
// ---------------------------------------------------------------------------

/// Check whether a notification should be suppressed.
///
/// This is the primary integration point — notifcenter calls this before
/// delivering any notification.
pub fn should_suppress(app_id: &str, priority: NotifPriority) -> SuppressResult {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return SuppressResult::Allow,
    };

    let profile_id = match state.active_profile_id {
        Some(id) => id,
        None => return SuppressResult::Allow, // Focus not active
    };

    let profile = match state.profiles.iter().find(|p| p.id == profile_id) {
        Some(p) => p,
        None => return SuppressResult::Allow,
    };

    match profile.mode {
        FocusMode::PriorityOnly => {
            // Critical and Alarm always break through in PriorityOnly
            if priority >= NotifPriority::Critical {
                return SuppressResult::Allow;
            }
            // Alarms break through if allowed
            if priority == NotifPriority::Alarm && profile.allow_alarms {
                return SuppressResult::Allow;
            }
            // Priority apps break through
            if profile.priority_apps.iter().any(|a| a == app_id) {
                return SuppressResult::Allow;
            }
            SuppressResult::Suppress
        }
        FocusMode::AlarmsOnly => {
            // Only alarms break through
            if priority == NotifPriority::Alarm && profile.allow_alarms {
                return SuppressResult::Allow;
            }
            // Critical still breaks through in AlarmsOnly (safety)
            if priority >= NotifPriority::Critical {
                return SuppressResult::Allow;
            }
            SuppressResult::Suppress
        }
        FocusMode::Total => {
            // Only alarms if explicitly allowed
            if priority == NotifPriority::Alarm && profile.allow_alarms {
                return SuppressResult::Allow;
            }
            SuppressResult::Suppress
        }
    }
}

/// Whether focus assist is currently active.
pub fn is_active() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| s.active_profile_id.is_some())
}

/// Get the currently active profile, if any.
pub fn active_profile() -> Option<FocusProfile> {
    let guard = STATE.lock();
    let state = guard.as_ref()?;
    let id = state.active_profile_id?;
    state.profiles.iter().find(|p| p.id == id).cloned()
}

// ---------------------------------------------------------------------------
// Activation / Deactivation
// ---------------------------------------------------------------------------

/// Activate a focus profile by ID.
pub fn activate(profile_id: u64) -> KernelResult<()> {
    activate_with_trigger(profile_id, TriggerKind::Manual)
}

/// Activate a focus profile with a specific trigger reason.
pub fn activate_with_trigger(profile_id: u64, trigger: TriggerKind) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        if !profile.enabled {
            return Err(KernelError::NotSupported);
        }

        // If already active, deactivate first (clears missed)
        if state.active_profile_id.is_some() {
            state.missed.clear();
        }

        state.active_profile_id = Some(profile_id);
        state.trigger = trigger;
        state.activated_at_ns = crate::hpet::elapsed_ns();
        state.total_sessions += 1;
        Ok(())
    })
}

/// Deactivate focus assist (return to normal notification delivery).
///
/// Returns the list of missed notifications if show_summary was enabled.
pub fn deactivate() -> KernelResult<Vec<MissedNotification>> {
    with_state(|state| {
        if state.active_profile_id.is_none() {
            return Err(KernelError::NotFound);
        }

        let profile_id = state.active_profile_id.unwrap_or(0);
        let show_summary = state.profiles.iter()
            .find(|p| p.id == profile_id)
            .is_some_and(|p| p.show_summary);

        let missed = if show_summary {
            state.missed.clone()
        } else {
            Vec::new()
        };

        state.active_profile_id = None;
        state.missed.clear();
        Ok(missed)
    })
}

/// Record a missed notification (called when should_suppress returns Suppress).
pub fn record_missed(app_id: &str, title: &str, priority: NotifPriority) {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return,
    };
    if state.active_profile_id.is_none() {
        return;
    }

    let id = state.next_missed_id;
    state.next_missed_id += 1;
    state.total_missed += 1;

    if state.missed.len() >= MAX_MISSED {
        // Drop oldest
        state.missed.remove(0);
    }

    state.missed.push(MissedNotification {
        id,
        app_id: String::from(app_id),
        title: String::from(title),
        priority,
        timestamp_ns: crate::hpet::elapsed_ns(),
    });
}

/// Get missed notifications from the current session.
pub fn missed_notifications() -> Vec<MissedNotification> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.missed.clone())
}

/// Count of missed notifications in the current session.
pub fn missed_count() -> usize {
    let guard = STATE.lock();
    guard.as_ref().map_or(0, |s| s.missed.len())
}

// ---------------------------------------------------------------------------
// Profile management
// ---------------------------------------------------------------------------

/// Create a custom focus profile.
pub fn create_profile(name: &str, mode: FocusMode) -> KernelResult<u64> {
    with_state(|state| {
        if state.profiles.len() >= MAX_PROFILES {
            return Err(KernelError::ResourceExhausted);
        }
        if name.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        // Check for duplicate name
        if state.profiles.iter().any(|p| p.name == name) {
            return Err(KernelError::AlreadyExists);
        }

        let id = state.next_profile_id;
        state.next_profile_id += 1;

        state.profiles.push(FocusProfile {
            id,
            name: String::from(name),
            mode,
            priority_apps: Vec::new(),
            allow_alarms: true,
            allow_reminders: false,
            show_summary: true,
            auto_reply: None,
            builtin: false,
            enabled: true,
        });

        Ok(id)
    })
}

/// Remove a custom focus profile (built-in profiles cannot be removed).
pub fn remove_profile(profile_id: u64) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.profiles.iter().position(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        if state.profiles[idx].builtin {
            return Err(KernelError::PermissionDenied);
        }
        // Cannot remove active profile
        if state.active_profile_id == Some(profile_id) {
            return Err(KernelError::NotSupported);
        }
        state.profiles.remove(idx);
        // Remove schedules pointing to this profile
        state.schedules.retain(|s| s.profile_id != profile_id);
        Ok(())
    })
}

/// Get a profile by ID.
pub fn get_profile(profile_id: u64) -> KernelResult<FocusProfile> {
    with_state(|state| {
        state.profiles.iter().find(|p| p.id == profile_id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List all profiles.
pub fn list_profiles() -> Vec<FocusProfile> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.profiles.clone())
}

/// Set the mode for a profile.
pub fn set_mode(profile_id: u64, mode: FocusMode) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        profile.mode = mode;
        Ok(())
    })
}

/// Set whether a profile is enabled.
pub fn set_enabled(profile_id: u64, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        profile.enabled = enabled;
        Ok(())
    })
}

/// Set auto-reply message for a profile (None to disable).
pub fn set_auto_reply(profile_id: u64, message: Option<&str>) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        profile.auto_reply = message.map(String::from);
        Ok(())
    })
}

/// Set whether to show missed notifications summary when focus ends.
pub fn set_show_summary(profile_id: u64, show: bool) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        profile.show_summary = show;
        Ok(())
    })
}

/// Set whether alarms can break through for a profile.
pub fn set_allow_alarms(profile_id: u64, allow: bool) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        profile.allow_alarms = allow;
        Ok(())
    })
}

/// Set whether reminders can break through for a profile.
pub fn set_allow_reminders(profile_id: u64, allow: bool) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        profile.allow_reminders = allow;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Priority apps
// ---------------------------------------------------------------------------

/// Add an app to a profile's priority list (breaks through in PriorityOnly mode).
pub fn add_priority_app(profile_id: u64, app_id: &str) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        if profile.priority_apps.len() >= MAX_PRIORITY_APPS {
            return Err(KernelError::ResourceExhausted);
        }
        if profile.priority_apps.iter().any(|a| a == app_id) {
            return Err(KernelError::AlreadyExists);
        }
        profile.priority_apps.push(String::from(app_id));
        Ok(())
    })
}

/// Remove an app from a profile's priority list.
pub fn remove_priority_app(profile_id: u64, app_id: &str) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        let idx = profile.priority_apps.iter().position(|a| a == app_id)
            .ok_or(KernelError::NotFound)?;
        profile.priority_apps.remove(idx);
        Ok(())
    })
}

/// List priority apps for a profile.
pub fn priority_apps(profile_id: u64) -> KernelResult<Vec<String>> {
    with_state(|state| {
        let profile = state.profiles.iter().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        Ok(profile.priority_apps.clone())
    })
}

// ---------------------------------------------------------------------------
// Schedule management
// ---------------------------------------------------------------------------

/// Add a focus schedule.
pub fn add_schedule(
    name: &str,
    days: [bool; 7],
    start_hour: u8,
    start_minute: u8,
    end_hour: u8,
    end_minute: u8,
    profile_id: u64,
) -> KernelResult<u64> {
    with_state(|state| {
        if state.schedules.len() >= MAX_SCHEDULES {
            return Err(KernelError::ResourceExhausted);
        }
        if start_hour > 23 || start_minute > 59 || end_hour > 23 || end_minute > 59 {
            return Err(KernelError::InvalidArgument);
        }
        if !state.profiles.iter().any(|p| p.id == profile_id) {
            return Err(KernelError::NotFound);
        }

        let id = state.next_schedule_id;
        state.next_schedule_id += 1;

        state.schedules.push(FocusSchedule {
            id,
            name: String::from(name),
            enabled: true,
            days,
            start_hour,
            start_minute,
            end_hour,
            end_minute,
            profile_id,
        });

        Ok(id)
    })
}

/// Remove a schedule.
pub fn remove_schedule(schedule_id: u64) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.schedules.iter().position(|s| s.id == schedule_id)
            .ok_or(KernelError::NotFound)?;
        state.schedules.remove(idx);
        Ok(())
    })
}

/// Enable or disable a schedule.
pub fn set_schedule_enabled(schedule_id: u64, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let sched = state.schedules.iter_mut().find(|s| s.id == schedule_id)
            .ok_or(KernelError::NotFound)?;
        sched.enabled = enabled;
        Ok(())
    })
}

/// List all schedules.
pub fn list_schedules() -> Vec<FocusSchedule> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.schedules.clone())
}

/// Check if any schedule should activate now.
///
/// Returns the profile_id to activate, or None if no schedule matches.
pub fn check_schedule(hour: u8, minute: u8, day_of_week: u8) -> Option<u64> {
    let guard = STATE.lock();
    let state = guard.as_ref()?;

    if day_of_week > 6 {
        return None;
    }

    for sched in &state.schedules {
        if !sched.enabled {
            continue;
        }
        if !sched.days[day_of_week as usize] {
            continue;
        }

        let now_minutes = (hour as u32) * 60 + (minute as u32);
        let start_minutes = (sched.start_hour as u32) * 60 + (sched.start_minute as u32);
        let end_minutes = (sched.end_hour as u32) * 60 + (sched.end_minute as u32);

        let in_range = if start_minutes <= end_minutes {
            // Same-day range (e.g., 09:00 - 17:00)
            now_minutes >= start_minutes && now_minutes < end_minutes
        } else {
            // Overnight range (e.g., 22:00 - 07:00)
            now_minutes >= start_minutes || now_minutes < end_minutes
        };

        if in_range {
            return Some(sched.profile_id);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Auto-trigger settings
// ---------------------------------------------------------------------------

/// Set whether fullscreen apps auto-activate focus.
pub fn set_auto_fullscreen(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.auto_fullscreen = enabled;
        Ok(())
    })
}

/// Set whether gaming activity auto-activates focus.
pub fn set_auto_gaming(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.auto_gaming = enabled;
        Ok(())
    })
}

/// Set whether presentation mode auto-activates focus.
pub fn set_auto_presentation(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.auto_presentation = enabled;
        Ok(())
    })
}

/// Set the profile used for auto-triggers.
pub fn set_auto_profile(profile_id: u64) -> KernelResult<()> {
    with_state(|state| {
        if !state.profiles.iter().any(|p| p.id == profile_id) {
            return Err(KernelError::NotFound);
        }
        state.auto_profile_id = profile_id;
        Ok(())
    })
}

/// Notify focus assist of an activity (called by compositor/window manager).
/// If auto-trigger is enabled for this activity, activates the auto profile.
pub fn notify_activity(trigger: TriggerKind) -> bool {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return false,
    };

    let should_activate = match trigger {
        TriggerKind::FullscreenApp => state.auto_fullscreen,
        TriggerKind::Gaming => state.auto_gaming,
        TriggerKind::Presentation => state.auto_presentation,
        _ => false,
    };

    if !should_activate {
        return false;
    }

    if state.active_profile_id.is_some() {
        return false; // Already active
    }

    let profile_id = state.auto_profile_id;
    if !state.profiles.iter().any(|p| p.id == profile_id && p.enabled) {
        return false;
    }

    state.active_profile_id = Some(profile_id);
    state.trigger = trigger;
    state.activated_at_ns = crate::hpet::elapsed_ns();
    state.total_sessions += 1;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    true
}

/// Notify that an auto-trigger condition has ended.
/// Only deactivates if the current session was auto-triggered by the same kind.
pub fn notify_activity_ended(trigger: TriggerKind) -> bool {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return false,
    };

    if state.active_profile_id.is_none() {
        return false;
    }

    // Only auto-deactivate if the trigger matches
    if state.trigger != trigger {
        return false;
    }

    // Don't auto-deactivate manual sessions
    if state.trigger == TriggerKind::Manual {
        return false;
    }

    state.active_profile_id = None;
    state.missed.clear();
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    true
}

// ---------------------------------------------------------------------------
// Auto-reply
// ---------------------------------------------------------------------------

/// Get the auto-reply message for the active profile, if set.
pub fn auto_reply() -> Option<String> {
    let guard = STATE.lock();
    let state = guard.as_ref()?;
    let id = state.active_profile_id?;
    state.profiles.iter()
        .find(|p| p.id == id)
        .and_then(|p| p.auto_reply.clone())
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (profile_count, schedule_count, is_active, missed_count, total_sessions, ops).
pub fn stats() -> (usize, usize, bool, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.profiles.len(),
            s.schedules.len(),
            s.active_profile_id.is_some(),
            s.missed.len(),
            s.total_sessions,
            s.ops,
        ),
        None => (0, 0, false, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the focus assist module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[focusassist] Running self-tests...");

    // Reset state for testing
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state — focus off, notifications allowed
    {
        assert!(!is_active());
        let result = should_suppress("test-app", NotifPriority::Normal);
        assert_eq!(result, SuppressResult::Allow);
        serial_println!("[focusassist]   1. Initial state: focus off, allow all — OK");
    }

    // Test 2: activate Priority Only — suppress normal, allow priority apps
    {
        activate(1).expect("activate Priority Only");
        assert!(is_active());
        let result = should_suppress("random-app", NotifPriority::Normal);
        assert_eq!(result, SuppressResult::Suppress);
        // Critical always breaks through
        let result = should_suppress("random-app", NotifPriority::Critical);
        assert_eq!(result, SuppressResult::Allow);
        serial_println!("[focusassist]   2. Priority Only: suppress normal, allow critical — OK");
    }

    // Test 3: priority apps break through in PriorityOnly mode
    {
        add_priority_app(1, "messaging-app").expect("add priority app");
        let result = should_suppress("messaging-app", NotifPriority::Normal);
        assert_eq!(result, SuppressResult::Allow);
        let result = should_suppress("other-app", NotifPriority::Normal);
        assert_eq!(result, SuppressResult::Suppress);
        remove_priority_app(1, "messaging-app").expect("remove priority app");
        serial_println!("[focusassist]   3. Priority app breaks through — OK");
    }

    // Test 4: deactivate — returns missed, clears state
    {
        record_missed("app1", "Message 1", NotifPriority::Normal);
        record_missed("app2", "Message 2", NotifPriority::High);
        assert_eq!(missed_count(), 2);
        let missed = deactivate().expect("deactivate");
        assert_eq!(missed.len(), 2);
        assert!(!is_active());
        assert_eq!(missed_count(), 0);
        serial_println!("[focusassist]   4. Deactivate returns missed, clears state — OK");
    }

    // Test 5: Alarms Only mode — only alarms break through
    {
        activate(2).expect("activate Alarms Only");
        let result = should_suppress("app", NotifPriority::Normal);
        assert_eq!(result, SuppressResult::Suppress);
        let result = should_suppress("app", NotifPriority::High);
        assert_eq!(result, SuppressResult::Suppress);
        let result = should_suppress("app", NotifPriority::Alarm);
        assert_eq!(result, SuppressResult::Allow);
        // Critical still breaks through for safety
        let result = should_suppress("app", NotifPriority::Critical);
        assert_eq!(result, SuppressResult::Allow);
        let _ = deactivate();
        serial_println!("[focusassist]   5. Alarms Only: suppress all except alarm/critical — OK");
    }

    // Test 6: Total mode — suppress everything except alarms if allowed
    {
        activate(3).expect("activate DND");
        // DND profile has allow_alarms=false
        let result = should_suppress("app", NotifPriority::Alarm);
        assert_eq!(result, SuppressResult::Suppress);
        let result = should_suppress("app", NotifPriority::Critical);
        assert_eq!(result, SuppressResult::Suppress);
        let _ = deactivate();
        serial_println!("[focusassist]   6. Total mode: suppress everything — OK");
    }

    // Test 7: custom profile creation and deletion
    {
        let id = create_profile("Work Focus", FocusMode::PriorityOnly)
            .expect("create profile");
        assert!(id >= 7);
        let profiles = list_profiles();
        assert!(profiles.iter().any(|p| p.name == "Work Focus"));
        remove_profile(id).expect("remove profile");
        let profiles = list_profiles();
        assert!(!profiles.iter().any(|p| p.name == "Work Focus"));
        serial_println!("[focusassist]   7. Custom profile create/delete — OK");
    }

    // Test 8: cannot delete builtin profiles
    {
        let result = remove_profile(1);
        assert!(result.is_err());
        serial_println!("[focusassist]   8. Cannot delete builtin profile — OK");
    }

    // Test 9: schedule check — overnight range
    {
        // Quiet Hours: 22:00-07:00 Mon-Fri, profile 6
        set_schedule_enabled(1, true).expect("enable schedule");
        // 23:00 Monday should match
        let result = check_schedule(23, 0, 1); // Monday
        assert_eq!(result, Some(6));
        // 03:00 Tuesday should match (overnight)
        let result = check_schedule(3, 0, 2); // Tuesday
        assert_eq!(result, Some(6));
        // 12:00 Monday should NOT match
        let result = check_schedule(12, 0, 1);
        assert!(result.is_none());
        // Sunday should NOT match
        let result = check_schedule(23, 0, 0); // Sunday
        assert!(result.is_none());
        set_schedule_enabled(1, false).expect("disable schedule");
        serial_println!("[focusassist]   9. Schedule check with overnight range — OK");
    }

    // Test 10: auto-trigger
    {
        set_auto_fullscreen(true).expect("set auto fullscreen");
        set_auto_profile(4).expect("set auto profile to Gaming");
        let activated = notify_activity(TriggerKind::FullscreenApp);
        assert!(activated);
        assert!(is_active());
        let profile = active_profile().expect("active profile");
        assert_eq!(profile.id, 4); // Gaming
        let ended = notify_activity_ended(TriggerKind::FullscreenApp);
        assert!(ended);
        assert!(!is_active());
        set_auto_fullscreen(false).expect("reset");
        serial_println!("[focusassist]  10. Auto-trigger fullscreen activate/deactivate — OK");
    }

    // Test 11: auto-reply
    {
        activate(3).expect("activate DND");
        let reply = auto_reply();
        assert!(reply.is_some());
        assert!(reply.unwrap_or_default().contains("unavailable"));
        let _ = deactivate();
        // Priority Only has no auto-reply
        activate(1).expect("activate Priority Only");
        assert!(auto_reply().is_none());
        let _ = deactivate();
        serial_println!("[focusassist]  11. Auto-reply text — OK");
    }

    serial_println!("[focusassist] All 11 self-tests passed.");
}

//! Parental controls — content filtering, screen time, app restrictions.
//!
//! Provides child account management with configurable restrictions
//! including web filtering, app allow/block lists, screen time limits,
//! and activity reporting.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Users → Parental Controls
//!   → parental::set_screen_time() / add_blocked_app()
//!
//! Enforcement points
//!   → parental::check_app_allowed(uid, app) before app launch
//!   → parental::check_web_allowed(uid, url) in browser
//!   → parental::check_time_allowed(uid) on login / periodic
//!
//! Integration:
//!   → useracct (child account identification)
//!   → appregistry (app metadata)
//!   → notifcenter (time limit warnings)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_CHILD_PROFILES: usize = 16;
const MAX_BLOCKED_APPS: usize = 128;
const MAX_BLOCKED_SITES: usize = 256;
const MAX_ALLOWED_APPS: usize = 128;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Content filter level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterLevel {
    /// No filtering.
    None,
    /// Light — block explicit content.
    Light,
    /// Moderate — block explicit + violence.
    Moderate,
    /// Strict — whitelist-only browsing.
    Strict,
}

impl FilterLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Light => "Light",
            Self::Moderate => "Moderate",
            Self::Strict => "Strict",
        }
    }
}

/// App restriction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRestrictionMode {
    /// All apps allowed (except blocked list).
    AllowAll,
    /// Only allowed list apps.
    AllowList,
    /// All blocked except system apps.
    BlockAll,
}

impl AppRestrictionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::AllowAll => "Allow All",
            Self::AllowList => "Allow List",
            Self::BlockAll => "Block All",
        }
    }
}

/// Day of week for schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayOfWeek {
    Monday, Tuesday, Wednesday, Thursday, Friday, Saturday, Sunday,
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
}

/// Screen time schedule for a day.
#[derive(Debug, Clone)]
pub struct DaySchedule {
    pub day: DayOfWeek,
    /// Allowed start hour (0-23).
    pub start_hour: u8,
    /// Allowed end hour (0-23).
    pub end_hour: u8,
    /// Maximum minutes of use on this day.
    pub max_minutes: u32,
}

/// Child profile with restrictions.
#[derive(Debug, Clone)]
pub struct ChildProfile {
    /// User ID.
    pub uid: u32,
    /// Profile name (child's name).
    pub name: String,
    /// Whether controls are active.
    pub enabled: bool,
    /// Content filter level.
    pub filter_level: FilterLevel,
    /// App restriction mode.
    pub app_mode: AppRestrictionMode,
    /// Blocked app IDs.
    pub blocked_apps: Vec<String>,
    /// Allowed app IDs (for AllowList mode).
    pub allowed_apps: Vec<String>,
    /// Blocked website patterns.
    pub blocked_sites: Vec<String>,
    /// Weekly schedule.
    pub schedule: Vec<DaySchedule>,
    /// Daily screen time limit (minutes, 0 = unlimited).
    pub daily_limit_minutes: u32,
    /// Time used today (minutes).
    pub time_used_today: u32,
    /// Safe search enforcement.
    pub safe_search: bool,
    /// Block in-app purchases.
    pub block_purchases: bool,
    /// Require approval for app installs.
    pub require_install_approval: bool,
    /// Activity logging.
    pub log_activity: bool,
    /// Total blocked attempts.
    pub blocked_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct ParentalState {
    profiles: Vec<ChildProfile>,
    ops: u64,
}

static STATE: Mutex<Option<ParentalState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut ParentalState) -> KernelResult<R>,
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

/// Initialize parental controls.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(ParentalState {
        profiles: Vec::new(),
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Profile management
// ---------------------------------------------------------------------------

/// Create a child profile.
pub fn create_profile(uid: u32, name: &str) -> KernelResult<()> {
    if name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.profiles.len() >= MAX_CHILD_PROFILES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.profiles.iter().any(|p| p.uid == uid) {
            return Err(KernelError::AlreadyExists);
        }

        let default_schedule = alloc::vec![
            DaySchedule { day: DayOfWeek::Monday, start_hour: 7, end_hour: 21, max_minutes: 120 },
            DaySchedule { day: DayOfWeek::Tuesday, start_hour: 7, end_hour: 21, max_minutes: 120 },
            DaySchedule { day: DayOfWeek::Wednesday, start_hour: 7, end_hour: 21, max_minutes: 120 },
            DaySchedule { day: DayOfWeek::Thursday, start_hour: 7, end_hour: 21, max_minutes: 120 },
            DaySchedule { day: DayOfWeek::Friday, start_hour: 7, end_hour: 22, max_minutes: 180 },
            DaySchedule { day: DayOfWeek::Saturday, start_hour: 8, end_hour: 22, max_minutes: 240 },
            DaySchedule { day: DayOfWeek::Sunday, start_hour: 8, end_hour: 21, max_minutes: 180 },
        ];

        state.profiles.push(ChildProfile {
            uid,
            name: String::from(name),
            enabled: true,
            filter_level: FilterLevel::Moderate,
            app_mode: AppRestrictionMode::AllowAll,
            blocked_apps: Vec::new(),
            allowed_apps: Vec::new(),
            blocked_sites: Vec::new(),
            schedule: default_schedule,
            daily_limit_minutes: 120,
            time_used_today: 0,
            safe_search: true,
            block_purchases: true,
            require_install_approval: true,
            log_activity: true,
            blocked_count: 0,
        });
        Ok(())
    })
}

/// Remove a child profile.
pub fn remove_profile(uid: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.profiles.iter().position(|p| p.uid == uid) {
            state.profiles.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Get a child profile.
pub fn get_profile(uid: u32) -> KernelResult<ChildProfile> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.profiles.iter()
        .find(|p| p.uid == uid)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all child profiles.
pub fn list_profiles() -> Vec<ChildProfile> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.profiles.clone())
}

/// Enable or disable controls.
pub fn set_enabled(uid: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        profile.enabled = enabled;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Content filtering
// ---------------------------------------------------------------------------

/// Set content filter level.
pub fn set_filter_level(uid: u32, level: FilterLevel) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        profile.filter_level = level;
        Ok(())
    })
}

/// Set safe search enforcement.
pub fn set_safe_search(uid: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        profile.safe_search = enabled;
        Ok(())
    })
}

/// Add a blocked website pattern.
pub fn add_blocked_site(uid: u32, pattern: &str) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        if profile.blocked_sites.len() >= MAX_BLOCKED_SITES {
            return Err(KernelError::ResourceExhausted);
        }
        profile.blocked_sites.push(String::from(pattern));
        Ok(())
    })
}

/// Remove a blocked site.
pub fn remove_blocked_site(uid: u32, pattern: &str) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        if let Some(pos) = profile.blocked_sites.iter().position(|s| s == pattern) {
            profile.blocked_sites.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

// ---------------------------------------------------------------------------
// App restrictions
// ---------------------------------------------------------------------------

/// Set app restriction mode.
pub fn set_app_mode(uid: u32, mode: AppRestrictionMode) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        profile.app_mode = mode;
        Ok(())
    })
}

/// Add a blocked app.
pub fn add_blocked_app(uid: u32, app_id: &str) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        if profile.blocked_apps.len() >= MAX_BLOCKED_APPS {
            return Err(KernelError::ResourceExhausted);
        }
        if !profile.blocked_apps.iter().any(|a| a == app_id) {
            profile.blocked_apps.push(String::from(app_id));
        }
        Ok(())
    })
}

/// Remove a blocked app.
pub fn remove_blocked_app(uid: u32, app_id: &str) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        if let Some(pos) = profile.blocked_apps.iter().position(|a| a == app_id) {
            profile.blocked_apps.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Add an allowed app (for AllowList mode).
pub fn add_allowed_app(uid: u32, app_id: &str) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        if profile.allowed_apps.len() >= MAX_ALLOWED_APPS {
            return Err(KernelError::ResourceExhausted);
        }
        if !profile.allowed_apps.iter().any(|a| a == app_id) {
            profile.allowed_apps.push(String::from(app_id));
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Screen time
// ---------------------------------------------------------------------------

/// Set daily screen time limit.
pub fn set_daily_limit(uid: u32, minutes: u32) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        profile.daily_limit_minutes = minutes;
        Ok(())
    })
}

/// Record screen time usage.
pub fn add_time_used(uid: u32, minutes: u32) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        profile.time_used_today = profile.time_used_today.saturating_add(minutes);
        Ok(())
    })
}

/// Reset daily time counter.
pub fn reset_daily_time(uid: u32) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        profile.time_used_today = 0;
        Ok(())
    })
}

/// Set schedule for a day.
pub fn set_schedule(uid: u32, day_index: usize, start: u8, end: u8, max_min: u32) -> KernelResult<()> {
    if start > 23 || end > 23 || day_index >= 7 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.uid == uid).ok_or(KernelError::NotFound)?;
        if day_index < profile.schedule.len() {
            profile.schedule[day_index].start_hour = start;
            profile.schedule[day_index].end_hour = end;
            profile.schedule[day_index].max_minutes = max_min;
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Checking (enforcement points)
// ---------------------------------------------------------------------------

/// Check if an app is allowed for a user.
pub fn check_app_allowed(uid: u32, app_id: &str) -> bool {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return true,
    };

    let profile = match state.profiles.iter_mut().find(|p| p.uid == uid) {
        Some(p) => p,
        None => return true, // Not a child account.
    };

    if !profile.enabled {
        return true;
    }

    let allowed = match profile.app_mode {
        AppRestrictionMode::AllowAll => !profile.blocked_apps.iter().any(|a| a == app_id),
        AppRestrictionMode::AllowList => profile.allowed_apps.iter().any(|a| a == app_id),
        AppRestrictionMode::BlockAll => false,
    };

    if !allowed {
        profile.blocked_count += 1;
    }
    allowed
}

/// Check if current time is allowed.
pub fn check_time_allowed(uid: u32, hour: u8, day_index: usize) -> bool {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return true,
    };

    let profile = match state.profiles.iter().find(|p| p.uid == uid) {
        Some(p) => p,
        None => return true,
    };

    if !profile.enabled {
        return true;
    }

    // Check daily limit.
    if profile.daily_limit_minutes > 0 && profile.time_used_today >= profile.daily_limit_minutes {
        return false;
    }

    // Check schedule.
    if day_index < profile.schedule.len() {
        let sched = &profile.schedule[day_index];
        if sched.start_hour <= sched.end_hour {
            hour >= sched.start_hour && hour < sched.end_hour
        } else {
            // Overnight schedule.
            hour >= sched.start_hour || hour < sched.end_hour
        }
    } else {
        true
    }
}

/// Check if a website is allowed.
pub fn check_web_allowed(uid: u32, url: &str) -> bool {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return true,
    };

    let profile = match state.profiles.iter_mut().find(|p| p.uid == uid) {
        Some(p) => p,
        None => return true,
    };

    if !profile.enabled {
        return true;
    }

    for pattern in &profile.blocked_sites {
        if url.contains(pattern.as_str()) {
            profile.blocked_count += 1;
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (profile_count, total_blocked, ops).
pub fn stats() -> (usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let blocked: u64 = s.profiles.iter().map(|p| p.blocked_count).sum();
            (s.profiles.len(), blocked, s.ops)
        }
        None => (0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for parental controls.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[parental] Running self-tests...");

    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state.
    {
        assert!(list_profiles().is_empty());
    }
    serial_println!("[parental]  1/11 initial state OK");

    // Test 2: create profile.
    {
        create_profile(1001, "Alice").unwrap();
        let p = get_profile(1001).unwrap();
        assert_eq!(p.name, "Alice");
        assert!(p.enabled);
        assert_eq!(p.filter_level, FilterLevel::Moderate);
        assert_eq!(p.daily_limit_minutes, 120);
    }
    serial_println!("[parental]  2/11 create profile OK");

    // Test 3: duplicate check.
    {
        assert!(create_profile(1001, "Alice2").is_err());
    }
    serial_println!("[parental]  3/11 duplicate check OK");

    // Test 4: app blocking.
    {
        add_blocked_app(1001, "game1").unwrap();
        assert!(!check_app_allowed(1001, "game1"));
        assert!(check_app_allowed(1001, "textedit"));
    }
    serial_println!("[parental]  4/11 app blocking OK");

    // Test 5: allow list mode.
    {
        set_app_mode(1001, AppRestrictionMode::AllowList).unwrap();
        add_allowed_app(1001, "textedit").unwrap();
        assert!(check_app_allowed(1001, "textedit"));
        assert!(!check_app_allowed(1001, "game2"));
        set_app_mode(1001, AppRestrictionMode::AllowAll).unwrap();
    }
    serial_println!("[parental]  5/11 allow list OK");

    // Test 6: web blocking.
    {
        add_blocked_site(1001, "badsite.com").unwrap();
        assert!(!check_web_allowed(1001, "https://badsite.com/page"));
        assert!(check_web_allowed(1001, "https://goodsite.com"));
    }
    serial_println!("[parental]  6/11 web blocking OK");

    // Test 7: screen time.
    {
        set_daily_limit(1001, 60).unwrap();
        add_time_used(1001, 30).unwrap();
        assert!(check_time_allowed(1001, 12, 0));
        add_time_used(1001, 30).unwrap();
        assert!(!check_time_allowed(1001, 12, 0)); // At limit.
    }
    serial_println!("[parental]  7/11 screen time OK");

    // Test 8: schedule.
    {
        reset_daily_time(1001).unwrap();
        // Default weekday: 7-21.
        assert!(check_time_allowed(1001, 12, 0));
        assert!(!check_time_allowed(1001, 22, 0)); // After 21.
        assert!(!check_time_allowed(1001, 5, 0)); // Before 7.
    }
    serial_println!("[parental]  8/11 schedule OK");

    // Test 9: filter level.
    {
        set_filter_level(1001, FilterLevel::Strict).unwrap();
        assert_eq!(get_profile(1001).unwrap().filter_level, FilterLevel::Strict);
    }
    serial_println!("[parental]  9/11 filter level OK");

    // Test 10: disable controls.
    {
        set_enabled(1001, false).unwrap();
        assert!(check_app_allowed(1001, "game1")); // Controls off.
        set_enabled(1001, true).unwrap();
    }
    serial_println!("[parental] 10/11 disable OK");

    // Test 11: remove profile.
    {
        remove_profile(1001).unwrap();
        assert!(list_profiles().is_empty());
        // Non-child user always allowed.
        assert!(check_app_allowed(9999, "anything"));
    }
    serial_println!("[parental] 11/11 remove OK");

    serial_println!("[parental] All self-tests passed.");
}

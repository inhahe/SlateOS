//! Location services — privacy-aware geolocation and per-app permissions.
//!
//! Provides location data from available sources (GPS, WiFi, IP-based)
//! with strict per-application permission controls and location history.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Privacy → Location
//!   → location::set_enabled() / set_app_permission()
//!
//! Applications
//!   → location::request_location(app_id) → allowed / denied
//!   → location::current_location() → (lat, lon, accuracy)
//!
//! Integration:
//!   → notifcenter (location access notifications)
//!   → nightlight (sunset/sunrise calculation)
//!   → timezone (auto-detect from location)
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

/// Location accuracy level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccuracyLevel {
    /// High accuracy (GPS, <10m).
    High,
    /// Medium accuracy (WiFi, ~100m).
    Medium,
    /// Low accuracy (IP-based, ~10km).
    Low,
    /// City-level only.
    City,
}

impl AccuracyLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
            Self::City => "City",
        }
    }
}

/// Location source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationSource {
    Gps,
    Wifi,
    CellTower,
    IpBased,
    Manual,
}

impl LocationSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gps => "GPS",
            Self::Wifi => "WiFi",
            Self::CellTower => "Cell Tower",
            Self::IpBased => "IP-based",
            Self::Manual => "Manual",
        }
    }
}

/// Per-application location permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationPermission {
    /// Always allowed.
    Allow,
    /// Only while app is in foreground.
    WhileInUse,
    /// Denied.
    Deny,
    /// Not yet decided (will prompt).
    AskNextTime,
}

impl LocationPermission {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::WhileInUse => "While In Use",
            Self::Deny => "Deny",
            Self::AskNextTime => "Ask",
        }
    }
}

/// A location fix.
#[derive(Debug, Clone)]
pub struct LocationFix {
    /// Latitude in microdegrees (deg * 1_000_000).
    pub latitude_ud: i64,
    /// Longitude in microdegrees (deg * 1_000_000).
    pub longitude_ud: i64,
    /// Altitude in millimeters above sea level.
    pub altitude_mm: i64,
    /// Horizontal accuracy in meters.
    pub accuracy_m: u32,
    /// Source of this fix.
    pub source: LocationSource,
    /// Timestamp (ns since boot).
    pub timestamp_ns: u64,
}

/// Per-app permission entry.
#[derive(Debug, Clone)]
pub struct AppLocationPerm {
    /// Application ID.
    pub app_id: String,
    /// Permission level.
    pub permission: LocationPermission,
    /// Number of times location was requested.
    pub request_count: u64,
    /// Last request timestamp.
    pub last_request_ns: u64,
    /// Accuracy level permitted.
    pub max_accuracy: AccuracyLevel,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HISTORY: usize = 1000;

struct State {
    enabled: bool,
    current: Option<LocationFix>,
    history: Vec<LocationFix>,
    app_perms: Vec<AppLocationPerm>,
    default_accuracy: AccuracyLevel,
    record_history: bool,
    total_requests: u64,
    total_denied: u64,
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

/// Initialise location services.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        enabled: false, // Disabled by default for privacy.
        current: None,
        history: Vec::new(),
        app_perms: Vec::new(),
        default_accuracy: AccuracyLevel::Medium,
        record_history: false,
        total_requests: 0,
        total_denied: 0,
        ops: 0,
    });
}

/// Enable or disable location services globally.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled = enabled;
        if !enabled {
            state.current = None;
        }
        Ok(())
    })
}

/// Check if location services are enabled.
pub fn is_enabled() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| s.enabled)
}

/// Update the current location fix (called by location provider).
pub fn update_location(
    latitude_ud: i64,
    longitude_ud: i64,
    altitude_mm: i64,
    accuracy_m: u32,
    source: LocationSource,
) -> KernelResult<()> {
    with_state(|state| {
        if !state.enabled {
            return Err(KernelError::NotSupported);
        }
        let now = crate::hpet::elapsed_ns();
        let fix = LocationFix {
            latitude_ud,
            longitude_ud,
            altitude_mm,
            accuracy_m,
            source,
            timestamp_ns: now,
        };

        if state.record_history {
            state.history.push(fix.clone());
            while state.history.len() > MAX_HISTORY {
                state.history.remove(0);
            }
        }

        state.current = Some(fix);
        Ok(())
    })
}

/// Get the current location (if available and enabled).
pub fn current_location() -> Option<LocationFix> {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| {
        if s.enabled { s.current.clone() } else { None }
    })
}

/// Request location access for an app. Returns the fix or an error.
pub fn request_location(app_id: &str) -> KernelResult<LocationFix> {
    with_state(|state| {
        state.total_requests += 1;

        if !state.enabled {
            state.total_denied += 1;
            return Err(KernelError::NotSupported);
        }

        // Check permission.
        let perm_entry = state.app_perms.iter_mut().find(|p| p.app_id == app_id);
        let permission = match perm_entry {
            Some(ref entry) => entry.permission,
            None => LocationPermission::AskNextTime,
        };

        match permission {
            LocationPermission::Deny => {
                state.total_denied += 1;
                if let Some(entry) = state.app_perms.iter_mut().find(|p| p.app_id == app_id) {
                    entry.request_count += 1;
                    entry.last_request_ns = crate::hpet::elapsed_ns();
                }
                return Err(KernelError::PermissionDenied);
            }
            LocationPermission::AskNextTime => {
                state.total_denied += 1;
                return Err(KernelError::PermissionDenied);
            }
            _ => {} // Allow or WhileInUse
        }

        // Update request stats.
        if let Some(entry) = state.app_perms.iter_mut().find(|p| p.app_id == app_id) {
            entry.request_count += 1;
            entry.last_request_ns = crate::hpet::elapsed_ns();
        }

        state.current.clone().ok_or(KernelError::NotFound)
    })
}

/// Set per-app location permission.
pub fn set_app_permission(app_id: &str, permission: LocationPermission) -> KernelResult<()> {
    with_state(|state| {
        if let Some(entry) = state.app_perms.iter_mut().find(|p| p.app_id == app_id) {
            entry.permission = permission;
        } else {
            state.app_perms.push(AppLocationPerm {
                app_id: String::from(app_id),
                permission,
                request_count: 0,
                last_request_ns: 0,
                max_accuracy: state.default_accuracy,
            });
        }
        Ok(())
    })
}

/// Get per-app permission.
pub fn get_app_permission(app_id: &str) -> KernelResult<AppLocationPerm> {
    with_state(|state| {
        state.app_perms.iter().find(|p| p.app_id == app_id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List all app permissions.
pub fn list_app_permissions() -> Vec<AppLocationPerm> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.app_perms.clone(),
        None => Vec::new(),
    }
}

/// Set whether to record location history.
pub fn set_record_history(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.record_history = enabled;
        Ok(())
    })
}

/// Clear location history.
pub fn clear_history() -> KernelResult<usize> {
    with_state(|state| {
        let count = state.history.len();
        state.history.clear();
        Ok(count)
    })
}

/// Get location history.
pub fn get_history() -> Vec<LocationFix> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.history.clone(),
        None => Vec::new(),
    }
}

/// Set default accuracy level.
pub fn set_default_accuracy(level: AccuracyLevel) -> KernelResult<()> {
    with_state(|state| {
        state.default_accuracy = level;
        Ok(())
    })
}

/// Statistics: (enabled, app_perm_count, total_requests, total_denied, history_len, ops).
pub fn stats() -> (bool, usize, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.enabled, s.app_perms.len(), s.total_requests, s.total_denied, s.history.len(), s.ops),
        None => (false, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("location::self_test() — running tests...");

    init_defaults();

    // Test 1: Disabled by default.
    assert!(!is_enabled());
    crate::serial_println!("  [1/11] disabled by default: OK");

    // Test 2: Enable location.
    set_enabled(true).expect("enable");
    assert!(is_enabled());
    crate::serial_println!("  [2/11] enable location: OK");

    // Test 3: Update location.
    update_location(47_606_209, -122_332_069, 56_000, 15, LocationSource::Gps).expect("update");
    let fix = current_location().expect("current");
    assert_eq!(fix.latitude_ud, 47_606_209);
    crate::serial_println!("  [3/11] update location: OK");

    // Test 4: App permission — deny by default (AskNextTime).
    let result = request_location("com.test.app");
    assert!(result.is_err());
    crate::serial_println!("  [4/11] deny by default: OK");

    // Test 5: Grant permission.
    set_app_permission("com.test.app", LocationPermission::Allow).expect("grant");
    let fix = request_location("com.test.app").expect("request allowed");
    assert_eq!(fix.latitude_ud, 47_606_209);
    crate::serial_println!("  [5/11] grant and request: OK");

    // Test 6: Deny permission.
    set_app_permission("com.blocked.app", LocationPermission::Deny).expect("deny");
    let result = request_location("com.blocked.app");
    assert!(result.is_err());
    crate::serial_println!("  [6/11] deny permission: OK");

    // Test 7: List permissions.
    let perms = list_app_permissions();
    assert_eq!(perms.len(), 2);
    crate::serial_println!("  [7/11] list permissions: OK");

    // Test 8: History recording.
    set_record_history(true).expect("enable history");
    update_location(48_000_000, -122_000_000, 0, 50, LocationSource::Wifi).expect("update 2");
    let history = get_history();
    assert_eq!(history.len(), 1); // Only new one, first was before history was enabled.
    crate::serial_println!("  [8/11] history recording: OK");

    // Test 9: Clear history.
    let cleared = clear_history().expect("clear");
    assert_eq!(cleared, 1);
    assert!(get_history().is_empty());
    crate::serial_println!("  [9/11] clear history: OK");

    // Test 10: Disable stops providing location.
    set_enabled(false).expect("disable");
    assert!(current_location().is_none());
    crate::serial_println!("  [10/11] disable clears current: OK");

    // Test 11: Stats.
    let (enabled, perm_count, requests, denied, _hist_len, ops) = stats();
    assert!(!enabled);
    assert_eq!(perm_count, 2);
    assert!(requests >= 2);
    assert!(denied >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("location::self_test() — all 11 tests passed");
}

//! Application Compatibility — compatibility layer and shim management.
//!
//! Manages application compatibility settings: API version targeting,
//! compatibility shims, deprecated API support, and per-app overrides.
//!
//! ## Architecture
//!
//! ```text
//! App launches
//!   → appcompat::get_profile(app) → compatibility settings
//!   → appcompat::apply_shims(app) → activate compatibility shims
//!
//! Configuration
//!   → appcompat::set_compat(app, version)
//!   → appcompat::add_shim(app, shim)
//!
//! Integration:
//!   → appregistry (app registry)
//!   → apppermissions (permissions)
//!   → pkgmgr (package manager)
//!   → appsandbox (sandbox settings)
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

/// API compatibility level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatLevel {
    /// Current OS version.
    Current,
    /// Previous major version.
    Legacy1,
    /// Two versions back.
    Legacy2,
    /// Maximum compatibility (all shims enabled).
    MaxCompat,
}

impl CompatLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "Current",
            Self::Legacy1 => "Legacy (v1)",
            Self::Legacy2 => "Legacy (v2)",
            Self::MaxCompat => "Max Compatibility",
        }
    }
}

/// Compatibility shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shim {
    /// Redirect old API calls to new ones.
    ApiRedirect,
    /// Emulate deprecated filesystem paths.
    PathRedirect,
    /// Emulate old display scaling behavior.
    LegacyDpi,
    /// Disable hardware acceleration.
    SoftwareRendering,
    /// Force single-threaded execution.
    SingleThread,
    /// Emulate older timer resolution.
    LegacyTimer,
    /// Allow deprecated permissions.
    PermissiveMode,
    /// Redirect old config locations.
    ConfigRedirect,
}

impl Shim {
    pub fn label(self) -> &'static str {
        match self {
            Self::ApiRedirect => "API Redirect",
            Self::PathRedirect => "Path Redirect",
            Self::LegacyDpi => "Legacy DPI",
            Self::SoftwareRendering => "Software Rendering",
            Self::SingleThread => "Single Thread",
            Self::LegacyTimer => "Legacy Timer",
            Self::PermissiveMode => "Permissive Mode",
            Self::ConfigRedirect => "Config Redirect",
        }
    }
}

/// Compatibility profile for an app.
#[derive(Debug, Clone)]
pub struct CompatProfile {
    pub app_name: String,
    pub compat_level: CompatLevel,
    pub shims: Vec<Shim>,
    pub enabled: bool,
    pub launch_count: u64,
    pub last_launch_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROFILES: usize = 200;

struct State {
    profiles: Vec<CompatProfile>,
    global_enabled: bool,
    total_launches: u64,
    total_shim_activations: u64,
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
        profiles: Vec::new(),
        global_enabled: true,
        total_launches: 0,
        total_shim_activations: 0,
        ops: 0,
    });
}

/// Set compatibility level for an app.
pub fn set_compat(app_name: &str, level: CompatLevel) -> KernelResult<()> {
    with_state(|state| {
        if let Some(p) = state.profiles.iter_mut().find(|p| p.app_name == app_name) {
            p.compat_level = level;
        } else {
            if state.profiles.len() >= MAX_PROFILES {
                return Err(KernelError::ResourceExhausted);
            }
            state.profiles.push(CompatProfile {
                app_name: String::from(app_name),
                compat_level: level,
                shims: Vec::new(),
                enabled: true,
                launch_count: 0,
                last_launch_ns: 0,
            });
        }
        Ok(())
    })
}

/// Add a shim to an app's profile.
pub fn add_shim(app_name: &str, shim: Shim) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.app_name == app_name)
            .ok_or(KernelError::NotFound)?;
        if !profile.shims.contains(&shim) {
            profile.shims.push(shim);
        }
        Ok(())
    })
}

/// Remove a shim.
pub fn remove_shim(app_name: &str, shim: Shim) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.app_name == app_name)
            .ok_or(KernelError::NotFound)?;
        profile.shims.retain(|s| *s != shim);
        Ok(())
    })
}

/// Get compatibility profile for an app (apply on launch).
pub fn get_profile(app_name: &str) -> Option<CompatProfile> {
    STATE.lock().as_ref().and_then(|s| {
        s.profiles.iter().find(|p| p.app_name == app_name && p.enabled).cloned()
    })
}

/// Record an app launch with compatibility applied.
pub fn record_launch(app_name: &str) -> KernelResult<usize> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let profile = state.profiles.iter_mut().find(|p| p.app_name == app_name)
            .ok_or(KernelError::NotFound)?;
        profile.launch_count += 1;
        profile.last_launch_ns = now;
        let shim_count = profile.shims.len();
        state.total_launches += 1;
        state.total_shim_activations += shim_count as u64;
        Ok(shim_count)
    })
}

/// Enable/disable a profile.
pub fn set_profile_enabled(app_name: &str, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.app_name == app_name)
            .ok_or(KernelError::NotFound)?;
        profile.enabled = enabled;
        Ok(())
    })
}

/// Remove a profile entirely.
pub fn remove_profile(app_name: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.profiles.len();
        state.profiles.retain(|p| p.app_name != app_name);
        if state.profiles.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Enable/disable compatibility globally.
pub fn set_global_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        Ok(())
    })
}

/// List all profiles.
pub fn list_profiles() -> Vec<CompatProfile> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.profiles.clone())
}

/// Statistics: (profile_count, total_launches, total_shim_activations, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.profiles.len(), s.total_launches, s.total_shim_activations, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("appcompat::self_test() — running tests...");
    init_defaults();

    // 1: No profiles initially.
    assert_eq!(list_profiles().len(), 0);
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Set compat level.
    set_compat("legacy_app", CompatLevel::Legacy1).expect("set");
    assert_eq!(list_profiles().len(), 1);
    crate::serial_println!("  [2/8] set compat: OK");

    // 3: Add shims.
    add_shim("legacy_app", Shim::ApiRedirect).expect("shim1");
    add_shim("legacy_app", Shim::LegacyDpi).expect("shim2");
    let p = get_profile("legacy_app").expect("profile");
    assert_eq!(p.shims.len(), 2);
    crate::serial_println!("  [3/8] add shims: OK");

    // 4: No duplicate shims.
    add_shim("legacy_app", Shim::ApiRedirect).expect("shim_dup");
    let p = get_profile("legacy_app").expect("profile2");
    assert_eq!(p.shims.len(), 2);
    crate::serial_println!("  [4/8] no duplicates: OK");

    // 5: Record launch.
    let shim_count = record_launch("legacy_app").expect("launch");
    assert_eq!(shim_count, 2);
    crate::serial_println!("  [5/8] launch: OK");

    // 6: Remove shim.
    remove_shim("legacy_app", Shim::LegacyDpi).expect("rm_shim");
    let p = get_profile("legacy_app").expect("profile3");
    assert_eq!(p.shims.len(), 1);
    crate::serial_println!("  [6/8] remove shim: OK");

    // 7: Disable profile.
    set_profile_enabled("legacy_app", false).expect("disable");
    assert!(get_profile("legacy_app").is_none()); // Disabled profiles not returned.
    crate::serial_println!("  [7/8] disable: OK");

    // 8: Stats.
    let (profiles, launches, shim_acts, ops) = stats();
    assert_eq!(profiles, 1);
    assert_eq!(launches, 1);
    assert_eq!(shim_acts, 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("appcompat::self_test() — all 8 tests passed");
}

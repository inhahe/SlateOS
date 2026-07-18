//! App Defaults — per-application default settings management.
//!
//! Stores and manages default preferences for applications,
//! providing a centralized registry for app configuration.
//!
//! ## Architecture
//!
//! ```text
//! App queries settings
//!   → appdefaults::get(app, key) → value
//!   → appdefaults::set(app, key, value) → store
//!   → appdefaults::reset(app) → clear to defaults
//!
//! Integration:
//!   → appregistry (application registry)
//!   → defaultapps (default app associations)
//!   → apppermissions (app permissions)
//!   → backup (settings backup)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Value type for a preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefValue {
    Str(String),
    Int(i64),
    Bool(bool),
}

impl PrefValue {
    pub fn type_label(&self) -> &'static str {
        match self {
            Self::Str(_) => "string",
            Self::Int(_) => "int",
            Self::Bool(_) => "bool",
        }
    }

    pub fn display_value(&self) -> String {
        match self {
            Self::Str(s) => s.clone(),
            Self::Int(v) => format!("{}", v),
            Self::Bool(v) => if *v { String::from("true") } else { String::from("false") },
        }
    }
}

/// A single preference entry.
#[derive(Debug, Clone)]
pub struct PrefEntry {
    pub key: String,
    pub value: PrefValue,
    pub modified_ns: u64,
}

/// Per-app preferences container.
#[derive(Debug, Clone)]
pub struct AppPrefs {
    pub app_name: String,
    pub entries: Vec<PrefEntry>,
    pub total_reads: u64,
    pub total_writes: u64,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_APPS: usize = 200;
const MAX_PREFS_PER_APP: usize = 100;

struct State {
    apps: Vec<AppPrefs>,
    total_reads: u64,
    total_writes: u64,
    total_resets: u64,
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
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        apps: alloc::vec![
            AppPrefs {
                app_name: String::from("system"),
                entries: alloc::vec![
                    PrefEntry { key: String::from("theme"), value: PrefValue::Str(String::from("dark")), modified_ns: now },
                    PrefEntry { key: String::from("font_size"), value: PrefValue::Int(14), modified_ns: now },
                    PrefEntry { key: String::from("animations"), value: PrefValue::Bool(true), modified_ns: now },
                ],
                total_reads: 0, total_writes: 0, created_ns: now,
            },
        ],
        total_reads: 0,
        total_writes: 0,
        total_resets: 0,
        ops: 0,
    });
}

/// Get a preference value.
pub fn get(app: &str, key: &str) -> KernelResult<Option<PrefValue>> {
    with_state(|state| {
        state.total_reads += 1;
        if let Some(a) = state.apps.iter_mut().find(|a| a.app_name == app) {
            a.total_reads += 1;
            Ok(a.entries.iter().find(|e| e.key == key).map(|e| e.value.clone()))
        } else {
            Ok(None)
        }
    })
}

/// Set a preference value (creates app entry if needed).
pub fn set(app: &str, key: &str, value: PrefValue) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        state.total_writes += 1;
        let app_entry = if let Some(idx) = state.apps.iter().position(|a| a.app_name == app) {
            &mut state.apps[idx]
        } else {
            if state.apps.len() >= MAX_APPS {
                return Err(KernelError::ResourceExhausted);
            }
            state.apps.push(AppPrefs {
                app_name: String::from(app),
                entries: Vec::new(),
                total_reads: 0, total_writes: 0, created_ns: now,
            });
            state.apps.last_mut().ok_or(KernelError::InternalError)?
        };
        app_entry.total_writes += 1;
        if let Some(e) = app_entry.entries.iter_mut().find(|e| e.key == key) {
            e.value = value;
            e.modified_ns = now;
        } else {
            if app_entry.entries.len() >= MAX_PREFS_PER_APP {
                return Err(KernelError::ResourceExhausted);
            }
            app_entry.entries.push(PrefEntry {
                key: String::from(key), value, modified_ns: now,
            });
        }
        Ok(())
    })
}

/// Delete a specific preference.
pub fn delete(app: &str, key: &str) -> KernelResult<()> {
    with_state(|state| {
        let a = state.apps.iter_mut().find(|a| a.app_name == app)
            .ok_or(KernelError::NotFound)?;
        let before = a.entries.len();
        a.entries.retain(|e| e.key != key);
        if a.entries.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Reset all preferences for an app.
pub fn reset(app: &str) -> KernelResult<()> {
    with_state(|state| {
        let a = state.apps.iter_mut().find(|a| a.app_name == app)
            .ok_or(KernelError::NotFound)?;
        a.entries.clear();
        state.total_resets += 1;
        Ok(())
    })
}

/// Remove an app's entire preferences.
pub fn remove_app(app: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.apps.len();
        state.apps.retain(|a| a.app_name != app);
        if state.apps.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// List all apps with preferences.
pub fn list_apps() -> Vec<String> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.apps.iter().map(|a| a.app_name.clone()).collect()
    })
}

/// Get all preferences for an app.
pub fn get_app_prefs(app: &str) -> Option<AppPrefs> {
    STATE.lock().as_ref().and_then(|s| {
        s.apps.iter().find(|a| a.app_name == app).cloned()
    })
}

/// List all keys for an app.
pub fn list_keys(app: &str) -> Vec<String> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.apps.iter().find(|a| a.app_name == app)
            .map_or(Vec::new(), |a| a.entries.iter().map(|e| e.key.clone()).collect())
    })
}

/// Statistics: (app_count, total_reads, total_writes, total_resets, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.apps.len(), s.total_reads, s.total_writes, s.total_resets, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("appdefaults::self_test() — running tests...");
    init_defaults();

    // 1: Default system prefs.
    let apps = list_apps();
    assert!(!apps.is_empty());
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Read existing preference.
    let val = get("system", "theme").expect("get");
    assert_eq!(val, Some(PrefValue::Str(String::from("dark"))));
    crate::serial_println!("  [2/8] read: OK");

    // 3: Set preference (new app).
    set("browser", "homepage", PrefValue::Str(String::from("https://example.com"))).expect("set");
    let val = get("browser", "homepage").expect("get2");
    assert_eq!(val, Some(PrefValue::Str(String::from("https://example.com"))));
    crate::serial_println!("  [3/8] write new: OK");

    // 4: Update existing preference.
    set("browser", "homepage", PrefValue::Str(String::from("https://other.com"))).expect("update");
    let val = get("browser", "homepage").expect("get3");
    assert_eq!(val, Some(PrefValue::Str(String::from("https://other.com"))));
    crate::serial_println!("  [4/8] update: OK");

    // 5: Multiple types.
    set("browser", "tab_count", PrefValue::Int(5)).expect("int");
    set("browser", "dark_mode", PrefValue::Bool(true)).expect("bool");
    let keys = list_keys("browser");
    assert_eq!(keys.len(), 3);
    crate::serial_println!("  [5/8] types: OK");

    // 6: Delete preference.
    delete("browser", "tab_count").expect("delete");
    let val = get("browser", "tab_count").expect("get4");
    assert_eq!(val, None);
    crate::serial_println!("  [6/8] delete: OK");

    // 7: Reset app.
    reset("browser").expect("reset");
    let keys = list_keys("browser");
    assert!(keys.is_empty());
    crate::serial_println!("  [7/8] reset: OK");

    // 8: Stats.
    let (apps, reads, writes, resets, ops) = stats();
    assert!(apps >= 1);
    assert!(reads >= 4);
    assert!(writes >= 3);
    assert_eq!(resets, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("appdefaults::self_test() — all 8 tests passed");
}

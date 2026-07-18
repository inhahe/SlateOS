//! Keyboard Shortcuts — custom keyboard shortcut management.
//!
//! Manages user-defined and system keyboard shortcuts with conflict
//! detection, categories, and per-app overrides.
//!
//! ## Architecture
//!
//! ```text
//! Key combination pressed
//!   → kbshortcuts::lookup(modifiers, key) → action
//!   → kbshortcuts::execute(action)
//!
//! User customization
//!   → kbshortcuts::bind(combo, action)
//!   → kbshortcuts::unbind(combo)
//!
//! Integration:
//!   → hotkeys (system hotkey registration)
//!   → kbsettings (keyboard configuration)
//!   → keylayout (key mapping)
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

/// Modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

impl Modifiers {
    pub fn none() -> Self {
        Self { ctrl: false, alt: false, shift: false, super_key: false }
    }

    pub fn label(self) -> String {
        let mut parts = Vec::new();
        if self.ctrl { parts.push("Ctrl"); }
        if self.alt { parts.push("Alt"); }
        if self.shift { parts.push("Shift"); }
        if self.super_key { parts.push("Super"); }
        if parts.is_empty() {
            String::from("(none)")
        } else {
            let joined: String = parts.join("+");
            joined
        }
    }
}

/// Shortcut category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutCategory {
    System,
    Navigation,
    WindowManagement,
    Application,
    Accessibility,
    Media,
    Custom,
}

impl ShortcutCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Navigation => "Navigation",
            Self::WindowManagement => "Window Management",
            Self::Application => "Application",
            Self::Accessibility => "Accessibility",
            Self::Media => "Media",
            Self::Custom => "Custom",
        }
    }
}

/// A keyboard shortcut binding.
#[derive(Debug, Clone)]
pub struct Shortcut {
    pub id: u32,
    pub modifiers: Modifiers,
    pub key: String,
    pub action: String,
    pub description: String,
    pub category: ShortcutCategory,
    pub enabled: bool,
    pub user_defined: bool,
    pub use_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SHORTCUTS: usize = 500;

struct State {
    shortcuts: Vec<Shortcut>,
    next_id: u32,
    total_binds: u64,
    total_triggers: u64,
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

    let defaults = alloc::vec![
        Shortcut { id: 1, modifiers: Modifiers { ctrl: true, alt: false, shift: false, super_key: false },
            key: String::from("C"), action: String::from("copy"), description: String::from("Copy selection"),
            category: ShortcutCategory::Application, enabled: true, user_defined: false, use_count: 0 },
        Shortcut { id: 2, modifiers: Modifiers { ctrl: true, alt: false, shift: false, super_key: false },
            key: String::from("V"), action: String::from("paste"), description: String::from("Paste clipboard"),
            category: ShortcutCategory::Application, enabled: true, user_defined: false, use_count: 0 },
        Shortcut { id: 3, modifiers: Modifiers { ctrl: true, alt: false, shift: false, super_key: false },
            key: String::from("Z"), action: String::from("undo"), description: String::from("Undo"),
            category: ShortcutCategory::Application, enabled: true, user_defined: false, use_count: 0 },
        Shortcut { id: 4, modifiers: Modifiers { ctrl: false, alt: true, shift: false, super_key: false },
            key: String::from("Tab"), action: String::from("switch_window"), description: String::from("Switch window"),
            category: ShortcutCategory::WindowManagement, enabled: true, user_defined: false, use_count: 0 },
        Shortcut { id: 5, modifiers: Modifiers { ctrl: false, alt: false, shift: false, super_key: true },
            key: String::from("L"), action: String::from("lock_screen"), description: String::from("Lock screen"),
            category: ShortcutCategory::System, enabled: true, user_defined: false, use_count: 0 },
        Shortcut { id: 6, modifiers: Modifiers { ctrl: false, alt: false, shift: false, super_key: false },
            key: String::from("PrintScreen"), action: String::from("screenshot"), description: String::from("Take screenshot"),
            category: ShortcutCategory::System, enabled: true, user_defined: false, use_count: 0 },
    ];

    *guard = Some(State {
        shortcuts: defaults,
        next_id: 7,
        total_binds: 0,
        total_triggers: 0,
        ops: 0,
    });
}

/// Bind a new shortcut.
pub fn bind(modifiers: Modifiers, key: &str, action: &str, description: &str, category: ShortcutCategory) -> KernelResult<u32> {
    with_state(|state| {
        if state.shortcuts.len() >= MAX_SHORTCUTS {
            return Err(KernelError::ResourceExhausted);
        }
        // Check for conflict.
        if state.shortcuts.iter().any(|s| s.modifiers == modifiers && s.key == key && s.enabled) {
            return Err(KernelError::AlreadyExists);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.total_binds += 1;
        state.shortcuts.push(Shortcut {
            id, modifiers,
            key: String::from(key),
            action: String::from(action),
            description: String::from(description),
            category, enabled: true, user_defined: true,
            use_count: 0,
        });
        Ok(id)
    })
}

/// Unbind (remove) a shortcut.
pub fn unbind(shortcut_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.shortcuts.iter().position(|s| s.id == shortcut_id)
            .ok_or(KernelError::NotFound)?;
        state.shortcuts.remove(pos);
        Ok(())
    })
}

/// Lookup a shortcut by key combination.
pub fn lookup(modifiers: Modifiers, key: &str) -> Option<String> {
    STATE.lock().as_ref().and_then(|s| {
        s.shortcuts.iter()
            .find(|sc| sc.modifiers == modifiers && sc.key == key && sc.enabled)
            .map(|sc| sc.action.clone())
    })
}

/// Trigger a shortcut (increments use count).
pub fn trigger(shortcut_id: u32) -> KernelResult<String> {
    with_state(|state| {
        let sc = state.shortcuts.iter_mut().find(|s| s.id == shortcut_id)
            .ok_or(KernelError::NotFound)?;
        sc.use_count += 1;
        state.total_triggers += 1;
        Ok(sc.action.clone())
    })
}

/// Enable/disable a shortcut.
pub fn set_enabled(shortcut_id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let sc = state.shortcuts.iter_mut().find(|s| s.id == shortcut_id)
            .ok_or(KernelError::NotFound)?;
        sc.enabled = enabled;
        Ok(())
    })
}

/// List shortcuts by category.
pub fn list_by_category(category: ShortcutCategory) -> Vec<Shortcut> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.shortcuts.iter().filter(|sc| sc.category == category).cloned().collect()
    })
}

/// List all shortcuts.
pub fn list_shortcuts() -> Vec<Shortcut> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.shortcuts.clone())
}

/// Get a shortcut.
pub fn get_shortcut(id: u32) -> KernelResult<Shortcut> {
    with_state(|state| {
        state.shortcuts.iter().find(|s| s.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Statistics: (shortcut_count, total_binds, total_triggers, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.shortcuts.len(), s.total_binds, s.total_triggers, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("kbshortcuts::self_test() — running tests...");
    init_defaults();

    // 1: Default shortcuts.
    let all = list_shortcuts();
    assert_eq!(all.len(), 6);
    crate::serial_println!("  [1/8] default shortcuts: OK");

    // 2: Lookup by key.
    let action = lookup(Modifiers { ctrl: true, alt: false, shift: false, super_key: false }, "C");
    assert_eq!(action, Some(String::from("copy")));
    crate::serial_println!("  [2/8] lookup: OK");

    // 3: Bind new shortcut.
    let id = bind(
        Modifiers { ctrl: true, alt: true, shift: false, super_key: false },
        "T", "open_terminal", "Open terminal", ShortcutCategory::Custom,
    ).expect("bind");
    assert_eq!(list_shortcuts().len(), 7);
    crate::serial_println!("  [3/8] bind: OK");

    // 4: Conflict rejected.
    let result = bind(
        Modifiers { ctrl: true, alt: true, shift: false, super_key: false },
        "T", "other_action", "Conflict", ShortcutCategory::Custom,
    );
    assert!(result.is_err());
    crate::serial_println!("  [4/8] conflict: OK");

    // 5: Trigger.
    let action = trigger(id).expect("trigger");
    assert_eq!(action, "open_terminal");
    let sc = get_shortcut(id).expect("get");
    assert_eq!(sc.use_count, 1);
    crate::serial_println!("  [5/8] trigger: OK");

    // 6: Disable.
    set_enabled(id, false).expect("disable");
    let found = lookup(Modifiers { ctrl: true, alt: true, shift: false, super_key: false }, "T");
    assert!(found.is_none()); // disabled shortcuts not found
    crate::serial_println!("  [6/8] disable: OK");

    // 7: Category filter.
    let system = list_by_category(ShortcutCategory::System);
    assert!(system.len() >= 2);
    crate::serial_println!("  [7/8] category filter: OK");

    // 8: Stats.
    let (count, binds, triggers, ops) = stats();
    assert_eq!(count, 7);
    assert_eq!(binds, 1);
    assert_eq!(triggers, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("kbshortcuts::self_test() — all 8 tests passed");
}

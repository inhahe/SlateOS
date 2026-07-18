//! Global hotkey manager — system-wide keyboard shortcuts.
//!
//! Manages keyboard shortcut bindings that map key combinations to
//! system actions, application launches, or custom commands. Users
//! can add, modify, and delete bindings through the settings panel.
//!
//! ## Design Reference
//!
//! design.txt lines 1317-1329:
//! - "set a hotkey - capture from keyboard, select function to apply"
//! - "modify or delete a hotkey from the list"
//! - "purposely comes with very few hotkeys enabled by default"
//! - "functions include: minimize all, change desktops, logoff, ..."
//!
//! Default hotkeys (from design pushback):
//! - Alt+F4: close window
//! - Alt+Tab: switch windows
//! - Ctrl+C/V/X: copy/paste/cut (GUI)
//! - Ctrl+Z: undo
//! - Print Screen: screenshot
//! - Ctrl+R: run dialog
//!
//! ## Architecture
//!
//! ```text
//! Keyboard driver → input subsystem
//!   → hotkeys::dispatch(keycombo)
//!   → if matched: execute action, return true (consumed)
//!   → if not matched: return false (pass to focused app)
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum registered hotkeys.
const MAX_HOTKEYS: usize = 512;

/// Maximum actions per hotkey (for multi-action bindings).
const MAX_ACTIONS_PER_KEY: usize = 4;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Super, // Windows/Command/Meta key.
}

impl Modifier {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Super => "Super",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ctrl" | "control" => Some(Self::Ctrl),
            "alt" => Some(Self::Alt),
            "shift" => Some(Self::Shift),
            "super" | "win" | "meta" | "cmd" => Some(Self::Super),
            _ => None,
        }
    }
}

/// A key combination (modifiers + key).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeyCombo {
    /// Modifier keys (sorted).
    pub modifiers: Vec<Modifier>,
    /// Primary key (e.g., "F4", "Tab", "C", "PrintScreen").
    pub key: String,
}

impl KeyCombo {
    /// Create a new key combo.
    pub fn new(mut modifiers: Vec<Modifier>, key: &str) -> Self {
        modifiers.sort();
        modifiers.dedup();
        Self {
            modifiers,
            key: String::from(key),
        }
    }

    /// Display string (e.g., "Ctrl+Shift+S").
    pub fn display(&self) -> String {
        let mut parts: Vec<&str> = self.modifiers.iter().map(|m| m.label()).collect();
        parts.push(&self.key);
        let mut out = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                out.push('+');
            }
            out.push_str(p);
        }
        out
    }

    /// Parse from a string like "Ctrl+Shift+S" or "Alt+F4".
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('+').collect();
        if parts.is_empty() {
            return None;
        }
        let mut modifiers = Vec::new();
        let mut key = None;
        for (i, part) in parts.iter().enumerate() {
            let lower = to_lower(part);
            if i < parts.len() - 1 {
                // All but last should be modifiers.
                modifiers.push(Modifier::from_str(&lower)?);
            } else {
                // Last part is the key.
                key = Some(String::from(*part));
            }
        }
        key.map(|k| {
            modifiers.sort();
            modifiers.dedup();
            KeyCombo { modifiers, key: k }
        })
    }
}

/// What a hotkey does when triggered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyAction {
    /// Close the focused window.
    CloseWindow,
    /// Switch to next window (Alt+Tab).
    SwitchWindow,
    /// Minimize all windows.
    MinimizeAll,
    /// Open the run dialog.
    RunDialog,
    /// Open the start menu.
    StartMenu,
    /// Take a screenshot.
    Screenshot,
    /// Lock the screen.
    LockScreen,
    /// Log out.
    LogOut,
    /// Copy (GUI clipboard).
    Copy,
    /// Cut (GUI clipboard).
    Cut,
    /// Paste (GUI clipboard).
    Paste,
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// Select all.
    SelectAll,
    /// Launch an application by ID.
    LaunchApp(String),
    /// Execute a command string.
    RunCommand(String),
    /// Switch to virtual desktop N.
    SwitchDesktop(u32),
    /// Custom action ID (for extensibility).
    Custom(String),
}

impl HotkeyAction {
    /// Display label.
    pub fn label(&self) -> String {
        match self {
            Self::CloseWindow => String::from("Close window"),
            Self::SwitchWindow => String::from("Switch window"),
            Self::MinimizeAll => String::from("Minimize all"),
            Self::RunDialog => String::from("Run dialog"),
            Self::StartMenu => String::from("Start menu"),
            Self::Screenshot => String::from("Screenshot"),
            Self::LockScreen => String::from("Lock screen"),
            Self::LogOut => String::from("Log out"),
            Self::Copy => String::from("Copy"),
            Self::Cut => String::from("Cut"),
            Self::Paste => String::from("Paste"),
            Self::Undo => String::from("Undo"),
            Self::Redo => String::from("Redo"),
            Self::SelectAll => String::from("Select all"),
            Self::LaunchApp(id) => alloc::format!("Launch: {}", id),
            Self::RunCommand(cmd) => alloc::format!("Run: {}", cmd),
            Self::SwitchDesktop(n) => alloc::format!("Desktop {}", n),
            Self::Custom(id) => alloc::format!("Custom: {}", id),
        }
    }

    /// Parse from a type:value string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "close" | "close-window" => Some(Self::CloseWindow),
            "switch" | "switch-window" | "alt-tab" => Some(Self::SwitchWindow),
            "minimize-all" | "show-desktop" => Some(Self::MinimizeAll),
            "run" | "run-dialog" => Some(Self::RunDialog),
            "start" | "start-menu" => Some(Self::StartMenu),
            "screenshot" | "print-screen" => Some(Self::Screenshot),
            "lock" | "lock-screen" => Some(Self::LockScreen),
            "logout" | "log-out" => Some(Self::LogOut),
            "copy" => Some(Self::Copy),
            "cut" => Some(Self::Cut),
            "paste" => Some(Self::Paste),
            "undo" => Some(Self::Undo),
            "redo" => Some(Self::Redo),
            "select-all" => Some(Self::SelectAll),
            _ => {
                if let Some(rest) = s.strip_prefix("launch:") {
                    Some(Self::LaunchApp(String::from(rest)))
                } else if let Some(rest) = s.strip_prefix("cmd:") {
                    Some(Self::RunCommand(String::from(rest)))
                } else if let Some(rest) = s.strip_prefix("desktop:") {
                    rest.parse::<u32>().ok().map(Self::SwitchDesktop)
                } else {
                    None
                }
            }
        }
    }
}

/// A registered hotkey binding.
#[derive(Debug, Clone)]
pub struct Hotkey {
    /// Key combination.
    pub combo: KeyCombo,
    /// Action(s) to execute.
    pub actions: Vec<HotkeyAction>,
    /// Whether this is a system-default (vs user-defined).
    pub is_default: bool,
    /// Whether the binding is enabled.
    pub enabled: bool,
    /// Description.
    pub description: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push((c as u8 + 32) as char);
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct HotkeyState {
    /// Key combo display string → Hotkey.
    bindings: BTreeMap<String, Hotkey>,
}

impl HotkeyState {
    const fn new() -> Self {
        Self {
            bindings: BTreeMap::new(),
        }
    }
}

static HOTKEYS: Mutex<HotkeyState> = Mutex::new(HotkeyState::new());
static DISPATCH_COUNT: AtomicU64 = AtomicU64::new(0);
static HIT_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Register a hotkey binding.
pub fn bind(combo: KeyCombo, action: HotkeyAction, desc: &str, is_default: bool) -> KernelResult<()> {
    let key = combo.display();
    let mut state = HOTKEYS.lock();

    if let Some(existing) = state.bindings.get_mut(&key) {
        // Add action to existing binding.
        if existing.actions.len() >= MAX_ACTIONS_PER_KEY {
            return Err(KernelError::ResourceExhausted);
        }
        if !existing.actions.contains(&action) {
            existing.actions.push(action);
        }
        return Ok(());
    }

    if state.bindings.len() >= MAX_HOTKEYS {
        return Err(KernelError::ResourceExhausted);
    }

    state.bindings.insert(key, Hotkey {
        combo,
        actions: alloc::vec![action],
        is_default,
        enabled: true,
        description: String::from(desc),
    });
    Ok(())
}

/// Remove a hotkey binding entirely.
pub fn unbind(combo: &KeyCombo) -> KernelResult<()> {
    let key = combo.display();
    let mut state = HOTKEYS.lock();
    state.bindings.remove(&key).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// Enable or disable a hotkey.
pub fn set_enabled(combo: &KeyCombo, enabled: bool) -> KernelResult<()> {
    let key = combo.display();
    let mut state = HOTKEYS.lock();
    let binding = state.bindings.get_mut(&key).ok_or(KernelError::NotFound)?;
    binding.enabled = enabled;
    Ok(())
}

/// Dispatch a key combo. Returns matching actions if found and enabled.
pub fn dispatch(combo: &KeyCombo) -> Vec<HotkeyAction> {
    DISPATCH_COUNT.fetch_add(1, Ordering::Relaxed);
    let key = combo.display();
    let state = HOTKEYS.lock();
    if let Some(binding) = state.bindings.get(&key) {
        if binding.enabled {
            HIT_COUNT.fetch_add(1, Ordering::Relaxed);
            return binding.actions.clone();
        }
    }
    Vec::new()
}

/// Look up which key combo is bound to an action.
pub fn find_binding(action: &HotkeyAction) -> Option<KeyCombo> {
    let state = HOTKEYS.lock();
    for binding in state.bindings.values() {
        if binding.actions.contains(action) {
            return Some(binding.combo.clone());
        }
    }
    None
}

/// Get a binding by combo.
pub fn get_binding(combo: &KeyCombo) -> Option<Hotkey> {
    let key = combo.display();
    let state = HOTKEYS.lock();
    state.bindings.get(&key).cloned()
}

/// List all bindings.
pub fn list_all() -> Vec<Hotkey> {
    let state = HOTKEYS.lock();
    state.bindings.values().cloned().collect()
}

/// List only enabled bindings.
pub fn list_enabled() -> Vec<Hotkey> {
    let state = HOTKEYS.lock();
    state.bindings.values()
        .filter(|h| h.enabled)
        .cloned()
        .collect()
}

/// Search bindings by action description or key combo.
pub fn search(query: &str) -> Vec<Hotkey> {
    if query.is_empty() {
        return Vec::new();
    }
    let q = to_lower(query);
    let state = HOTKEYS.lock();
    state.bindings.values()
        .filter(|h| {
            to_lower(&h.combo.display()).contains(&q) ||
            to_lower(&h.description).contains(&q) ||
            h.actions.iter().any(|a| to_lower(&a.label()).contains(&q))
        })
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Default bindings
// ---------------------------------------------------------------------------

/// Register the default system hotkeys.
pub fn register_defaults() -> KernelResult<()> {
    let defaults = [
        ("Alt+F4", HotkeyAction::CloseWindow, "Close active window"),
        ("Alt+Tab", HotkeyAction::SwitchWindow, "Switch between windows"),
        ("Ctrl+C", HotkeyAction::Copy, "Copy to clipboard"),
        ("Ctrl+X", HotkeyAction::Cut, "Cut to clipboard"),
        ("Ctrl+V", HotkeyAction::Paste, "Paste from clipboard"),
        ("Ctrl+Z", HotkeyAction::Undo, "Undo"),
        ("Ctrl+Shift+Z", HotkeyAction::Redo, "Redo"),
        ("Ctrl+Y", HotkeyAction::Redo, "Redo (alt)"),
        ("Ctrl+A", HotkeyAction::SelectAll, "Select all"),
        ("Ctrl+R", HotkeyAction::RunDialog, "Open Run dialog"),
        ("Super+R", HotkeyAction::RunDialog, "Open Run dialog (alt)"),
        ("PrintScreen", HotkeyAction::Screenshot, "Take screenshot"),
        ("Super+L", HotkeyAction::LockScreen, "Lock screen"),
        ("Super+M", HotkeyAction::MinimizeAll, "Minimize all windows"),
        ("Super+D", HotkeyAction::MinimizeAll, "Show desktop"),
    ];

    for (combo_str, action, desc) in &defaults {
        if let Some(combo) = KeyCombo::parse(combo_str) {
            bind(combo, action.clone(), desc, true)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (binding_count, enabled_count, dispatch_ops, hit_ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = HOTKEYS.lock();
    let enabled = state.bindings.values().filter(|h| h.enabled).count();
    (
        state.bindings.len(),
        enabled,
        DISPATCH_COUNT.load(Ordering::Relaxed),
        HIT_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    DISPATCH_COUNT.store(0, Ordering::Relaxed);
    HIT_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all bindings.
pub fn clear_all() {
    let mut state = HOTKEYS.lock();
    state.bindings.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the hotkey system.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: parse and bind.
    {
        let combo = KeyCombo::parse("Ctrl+S").unwrap();
        assert_eq!(combo.modifiers, alloc::vec![Modifier::Ctrl]);
        assert_eq!(combo.key, "S");
        assert_eq!(combo.display(), "Ctrl+S");

        bind(combo, HotkeyAction::Custom(String::from("save")), "Save", false)?;
        serial_println!("[hotkeys] test 1 passed: parse/bind");
    }

    // Test 2: dispatch.
    {
        let combo = KeyCombo::parse("Ctrl+S").unwrap();
        let actions = dispatch(&combo);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], HotkeyAction::Custom(String::from("save")));

        // Non-existent combo returns empty.
        let other = KeyCombo::parse("Ctrl+Q").unwrap();
        let actions = dispatch(&other);
        assert!(actions.is_empty());
        serial_println!("[hotkeys] test 2 passed: dispatch");
    }

    // Test 3: enable/disable.
    {
        let combo = KeyCombo::parse("Ctrl+S").unwrap();
        set_enabled(&combo, false)?;
        let actions = dispatch(&combo);
        assert!(actions.is_empty());

        set_enabled(&combo, true)?;
        let actions = dispatch(&combo);
        assert_eq!(actions.len(), 1);
        serial_println!("[hotkeys] test 3 passed: enable/disable");
    }

    // Test 4: find_binding.
    {
        let combo = find_binding(&HotkeyAction::Custom(String::from("save")));
        assert!(combo.is_some());
        assert_eq!(combo.unwrap().display(), "Ctrl+S");
        serial_println!("[hotkeys] test 4 passed: find_binding");
    }

    // Test 5: register defaults.
    {
        register_defaults()?;
        let all = list_all();
        assert!(all.len() >= 10); // At least our defaults.

        // Alt+F4 should close window.
        let combo = KeyCombo::parse("Alt+F4").unwrap();
        let actions = dispatch(&combo);
        assert!(!actions.is_empty());
        assert_eq!(actions[0], HotkeyAction::CloseWindow);
        serial_println!("[hotkeys] test 5 passed: defaults");
    }

    // Test 6: search.
    {
        let results = search("clipboard");
        assert!(results.len() >= 2); // Copy, Cut, Paste.
        serial_println!("[hotkeys] test 6 passed: search");
    }

    // Test 7: unbind.
    {
        let combo = KeyCombo::parse("Ctrl+S").unwrap();
        unbind(&combo)?;
        let actions = dispatch(&combo);
        assert!(actions.is_empty());
        serial_println!("[hotkeys] test 7 passed: unbind");
    }

    clear_all();
    reset_stats();

    serial_println!("[hotkeys] all 7 self-tests passed");
    Ok(())
}

//! Notification Badges — app icon badge/counter management.
//!
//! Manages per-app notification badge counts displayed on taskbar
//! icons and app launcher entries.
//!
//! ## Architecture
//!
//! ```text
//! App sends notification
//!   → notifbadge::increment(app) → badge count +1
//!   → notifbadge::set(app, count) → exact count
//!   → notifbadge::clear(app) → remove badge
//!
//! Integration:
//!   → notifcenter (notification center)
//!   → taskbar (taskbar icon badges)
//!   → appregistry (app identity)
//!   → startmenu (launcher badges)
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

/// Badge style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeStyle {
    /// Numeric count.
    Count,
    /// Simple dot indicator.
    Dot,
    /// Attention indicator (e.g., exclamation).
    Attention,
    /// Progress indicator (0-100).
    Progress,
}

impl BadgeStyle {
    pub fn label(self) -> &'static str {
        match self {
            Self::Count => "Count",
            Self::Dot => "Dot",
            Self::Attention => "Attention",
            Self::Progress => "Progress",
        }
    }
}

/// A badge entry for an app.
#[derive(Debug, Clone)]
pub struct Badge {
    pub app_name: String,
    pub style: BadgeStyle,
    pub count: u32,
    /// For Progress style: 0-100.
    pub progress: u32,
    pub visible: bool,
    pub last_updated_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_BADGES: usize = 200;

struct State {
    badges: Vec<Badge>,
    global_enabled: bool,
    total_updates: u64,
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
        badges: Vec::new(),
        global_enabled: true,
        total_updates: 0,
        ops: 0,
    });
}

/// Set badge count for an app.
pub fn set_count(app_name: &str, count: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(badge) = state.badges.iter_mut().find(|b| b.app_name == app_name) {
            badge.count = count;
            badge.style = BadgeStyle::Count;
            badge.visible = count > 0;
            badge.last_updated_ns = now;
        } else {
            if state.badges.len() >= MAX_BADGES {
                return Err(KernelError::ResourceExhausted);
            }
            state.badges.push(Badge {
                app_name: String::from(app_name),
                style: BadgeStyle::Count,
                count, progress: 0,
                visible: count > 0,
                last_updated_ns: now,
            });
        }
        state.total_updates += 1;
        Ok(())
    })
}

/// Increment badge count.
pub fn increment(app_name: &str) -> KernelResult<u32> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(badge) = state.badges.iter_mut().find(|b| b.app_name == app_name) {
            badge.count = badge.count.saturating_add(1);
            badge.visible = true;
            badge.last_updated_ns = now;
            state.total_updates += 1;
            Ok(badge.count)
        } else {
            if state.badges.len() >= MAX_BADGES {
                return Err(KernelError::ResourceExhausted);
            }
            state.badges.push(Badge {
                app_name: String::from(app_name),
                style: BadgeStyle::Count,
                count: 1, progress: 0,
                visible: true,
                last_updated_ns: now,
            });
            state.total_updates += 1;
            Ok(1)
        }
    })
}

/// Set dot-style badge (no count).
pub fn set_dot(app_name: &str, visible: bool) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(badge) = state.badges.iter_mut().find(|b| b.app_name == app_name) {
            badge.style = BadgeStyle::Dot;
            badge.visible = visible;
            badge.last_updated_ns = now;
        } else {
            if state.badges.len() >= MAX_BADGES {
                return Err(KernelError::ResourceExhausted);
            }
            state.badges.push(Badge {
                app_name: String::from(app_name),
                style: BadgeStyle::Dot,
                count: 0, progress: 0,
                visible,
                last_updated_ns: now,
            });
        }
        state.total_updates += 1;
        Ok(())
    })
}

/// Set progress badge (0-100).
pub fn set_progress(app_name: &str, progress: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let pct = progress.min(100);
        if let Some(badge) = state.badges.iter_mut().find(|b| b.app_name == app_name) {
            badge.style = BadgeStyle::Progress;
            badge.progress = pct;
            badge.visible = true;
            badge.last_updated_ns = now;
        } else {
            if state.badges.len() >= MAX_BADGES {
                return Err(KernelError::ResourceExhausted);
            }
            state.badges.push(Badge {
                app_name: String::from(app_name),
                style: BadgeStyle::Progress,
                count: 0, progress: pct,
                visible: true,
                last_updated_ns: now,
            });
        }
        state.total_updates += 1;
        Ok(())
    })
}

/// Clear badge for an app.
pub fn clear(app_name: &str) -> KernelResult<()> {
    with_state(|state| {
        state.badges.retain(|b| b.app_name != app_name);
        state.total_updates += 1;
        Ok(())
    })
}

/// Clear all badges.
pub fn clear_all() -> KernelResult<()> {
    with_state(|state| {
        state.badges.clear();
        state.total_updates += 1;
        Ok(())
    })
}

/// Get badge for an app.
pub fn get_badge(app_name: &str) -> Option<Badge> {
    STATE.lock().as_ref().and_then(|s| {
        s.badges.iter().find(|b| b.app_name == app_name).cloned()
    })
}

/// List all visible badges.
pub fn list_visible() -> Vec<Badge> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.badges.iter().filter(|b| b.visible && s.global_enabled).cloned().collect()
    })
}

/// Enable/disable all badges globally.
pub fn set_global_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        Ok(())
    })
}

/// Statistics: (badge_count, visible_count, total_updates, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let visible = s.badges.iter().filter(|b| b.visible).count();
            (s.badges.len(), visible, s.total_updates, s.ops)
        }
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("notifbadge::self_test() — running tests...");
    init_defaults();

    // 1: No badges initially.
    assert_eq!(list_visible().len(), 0);
    crate::serial_println!("  [1/8] no badges: OK");

    // 2: Set count.
    set_count("email", 5).expect("set");
    let b = get_badge("email").expect("badge");
    assert_eq!(b.count, 5);
    assert_eq!(b.style, BadgeStyle::Count);
    crate::serial_println!("  [2/8] set count: OK");

    // 3: Increment.
    let c = increment("email").expect("inc");
    assert_eq!(c, 6);
    crate::serial_println!("  [3/8] increment: OK");

    // 4: Dot badge.
    set_dot("chat", true).expect("dot");
    let b = get_badge("chat").expect("badge2");
    assert_eq!(b.style, BadgeStyle::Dot);
    assert!(b.visible);
    crate::serial_println!("  [4/8] dot badge: OK");

    // 5: Progress badge.
    set_progress("downloader", 75).expect("prog");
    let b = get_badge("downloader").expect("badge3");
    assert_eq!(b.style, BadgeStyle::Progress);
    assert_eq!(b.progress, 75);
    crate::serial_println!("  [5/8] progress badge: OK");

    // 6: Visible list.
    let visible = list_visible();
    assert_eq!(visible.len(), 3);
    crate::serial_println!("  [6/8] visible list: OK");

    // 7: Clear specific.
    clear("chat").expect("clear");
    assert!(get_badge("chat").is_none());
    assert_eq!(list_visible().len(), 2);
    crate::serial_println!("  [7/8] clear: OK");

    // 8: Stats.
    let (count, visible, updates, ops) = stats();
    assert_eq!(count, 2);
    assert_eq!(visible, 2);
    assert!(updates >= 5);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("notifbadge::self_test() — all 8 tests passed");
}

//! Notification Grouping — group and bundle notifications.
//!
//! Groups notifications by app, category, or conversation to reduce
//! notification clutter. Supports summary notifications and expand/collapse.
//!
//! ## Architecture
//!
//! ```text
//! Notification arrives
//!   → notifgroup::classify(notif) → find/create group
//!   → notifgroup::add_to_group(group, notif) → update group
//!
//! Display
//!   → notifgroup::get_groups() → grouped notification list
//!   → notifgroup::expand_group(id) → show individual notifications
//!
//! Integration:
//!   → notifcenter (notification center)
//!   → notifprefs (notification preferences)
//!   → notifbadge (badge counts)
//!   → focusassist (DND filtering)
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

/// How notifications are grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupingMode {
    /// Group by sending application.
    ByApp,
    /// Group by notification category.
    ByCategory,
    /// Group by conversation/thread.
    ByConversation,
    /// No grouping.
    None,
}

impl GroupingMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ByApp => "By App",
            Self::ByCategory => "By Category",
            Self::ByConversation => "By Conversation",
            Self::None => "None",
        }
    }
}

/// Notification priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotifPriority {
    Low,
    Normal,
    High,
    Urgent,
}

impl NotifPriority {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
            Self::Urgent => "Urgent",
        }
    }
}

/// A single notification in a group.
#[derive(Debug, Clone)]
pub struct GroupedNotif {
    pub id: u32,
    pub app_name: String,
    pub title: String,
    pub body: String,
    pub priority: NotifPriority,
    pub timestamp_ns: u64,
    pub read: bool,
}

/// A notification group.
#[derive(Debug, Clone)]
pub struct NotifGroupEntry {
    pub group_id: u32,
    pub group_key: String,
    pub app_name: String,
    pub notifications: Vec<GroupedNotif>,
    pub expanded: bool,
    pub muted: bool,
    pub latest_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_GROUPS: usize = 100;
const MAX_NOTIFS_PER_GROUP: usize = 50;

struct State {
    groups: Vec<NotifGroupEntry>,
    mode: GroupingMode,
    next_group_id: u32,
    next_notif_id: u32,
    auto_dismiss_read: bool,
    total_notifications: u64,
    total_groups_created: u64,
    total_dismissed: u64,
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
        groups: Vec::new(),
        mode: GroupingMode::ByApp,
        next_group_id: 1,
        next_notif_id: 1,
        auto_dismiss_read: false,
        total_notifications: 0,
        total_groups_created: 0,
        total_dismissed: 0,
        ops: 0,
    });
}

/// Set grouping mode.
pub fn set_mode(mode: GroupingMode) -> KernelResult<()> {
    with_state(|state| {
        state.mode = mode;
        Ok(())
    })
}

/// Add a notification (auto-grouped).
pub fn add_notification(app_name: &str, title: &str, body: &str, priority: NotifPriority) -> KernelResult<(u32, u32)> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        state.total_notifications += 1;

        let group_key = match state.mode {
            GroupingMode::ByApp => String::from(app_name),
            GroupingMode::ByCategory => format!("{}:{}", app_name, title),
            GroupingMode::ByConversation => format!("{}:conv", app_name),
            GroupingMode::None => format!("single:{}", state.next_notif_id),
        };

        let nid = state.next_notif_id;
        state.next_notif_id += 1;
        let notif = GroupedNotif {
            id: nid,
            app_name: String::from(app_name),
            title: String::from(title),
            body: String::from(body),
            priority,
            timestamp_ns: now,
            read: false,
        };

        // Find existing group.
        if let Some(group) = state.groups.iter_mut().find(|g| g.group_key == group_key) {
            if group.notifications.len() >= MAX_NOTIFS_PER_GROUP {
                group.notifications.remove(0);
            }
            group.notifications.push(notif);
            group.latest_ns = now;
            return Ok((group.group_id, nid));
        }

        // Create new group.
        if state.groups.len() >= MAX_GROUPS {
            // Remove oldest group.
            if !state.groups.is_empty() {
                let oldest_idx = state.groups.iter().enumerate()
                    .min_by_key(|(_, g)| g.latest_ns)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                state.groups.remove(oldest_idx);
            }
        }

        let gid = state.next_group_id;
        state.next_group_id += 1;
        state.total_groups_created += 1;
        state.groups.push(NotifGroupEntry {
            group_id: gid,
            group_key,
            app_name: String::from(app_name),
            notifications: alloc::vec![notif],
            expanded: false,
            muted: false,
            latest_ns: now,
        });
        Ok((gid, nid))
    })
}

/// Mark a notification as read.
pub fn mark_read(notif_id: u32) -> KernelResult<()> {
    with_state(|state| {
        for group in &mut state.groups {
            if let Some(n) = group.notifications.iter_mut().find(|n| n.id == notif_id) {
                n.read = true;
                return Ok(());
            }
        }
        Err(KernelError::NotFound)
    })
}

/// Mark all in a group as read.
pub fn mark_group_read(group_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let group = state.groups.iter_mut().find(|g| g.group_id == group_id)
            .ok_or(KernelError::NotFound)?;
        for n in &mut group.notifications {
            n.read = true;
        }
        Ok(())
    })
}

/// Expand/collapse a group.
pub fn set_expanded(group_id: u32, expanded: bool) -> KernelResult<()> {
    with_state(|state| {
        let group = state.groups.iter_mut().find(|g| g.group_id == group_id)
            .ok_or(KernelError::NotFound)?;
        group.expanded = expanded;
        Ok(())
    })
}

/// Mute/unmute a group.
pub fn set_muted(group_id: u32, muted: bool) -> KernelResult<()> {
    with_state(|state| {
        let group = state.groups.iter_mut().find(|g| g.group_id == group_id)
            .ok_or(KernelError::NotFound)?;
        group.muted = muted;
        Ok(())
    })
}

/// Dismiss a group.
pub fn dismiss_group(group_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.groups.len();
        let dismissed_count = state.groups.iter()
            .find(|g| g.group_id == group_id)
            .map_or(0, |g| g.notifications.len() as u64);
        state.groups.retain(|g| g.group_id != group_id);
        if state.groups.len() == before { return Err(KernelError::NotFound); }
        state.total_dismissed += dismissed_count;
        Ok(())
    })
}

/// Dismiss all notifications.
pub fn dismiss_all() -> KernelResult<()> {
    with_state(|state| {
        let total: u64 = state.groups.iter().map(|g| g.notifications.len() as u64).sum();
        state.groups.clear();
        state.total_dismissed += total;
        Ok(())
    })
}

/// Get all groups sorted by latest timestamp.
pub fn get_groups() -> Vec<NotifGroupEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut groups = s.groups.clone();
        groups.sort_by_key(|e| core::cmp::Reverse(e.latest_ns));
        groups
    })
}

/// Get unread count.
pub fn unread_count() -> usize {
    STATE.lock().as_ref().map_or(0, |s| {
        s.groups.iter().flat_map(|g| g.notifications.iter()).filter(|n| !n.read).count()
    })
}

/// Get grouping mode.
pub fn get_mode() -> GroupingMode {
    STATE.lock().as_ref().map_or(GroupingMode::ByApp, |s| s.mode)
}

/// Statistics: (group_count, total_notifications, unread, total_dismissed, ops).
pub fn stats() -> (usize, u64, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let unread = s.groups.iter().flat_map(|g| g.notifications.iter()).filter(|n| !n.read).count();
            (s.groups.len(), s.total_notifications, unread, s.total_dismissed, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("notifgroup::self_test() — running tests...");
    init_defaults();

    // 1: Empty.
    assert_eq!(get_groups().len(), 0);
    assert_eq!(unread_count(), 0);
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Add notifications (same app = same group).
    let (g1, n1) = add_notification("mail", "New email", "From Alice", NotifPriority::Normal).expect("add1");
    let (g2, _n2) = add_notification("mail", "New email", "From Bob", NotifPriority::Normal).expect("add2");
    assert_eq!(g1, g2); // Same group.
    assert_eq!(get_groups().len(), 1);
    assert_eq!(unread_count(), 2);
    crate::serial_println!("  [2/8] grouping: OK");

    // 3: Different app = different group.
    let (g3, _) = add_notification("chat", "Message", "Hello", NotifPriority::High).expect("add3");
    assert_ne!(g1, g3);
    assert_eq!(get_groups().len(), 2);
    crate::serial_println!("  [3/8] separate groups: OK");

    // 4: Mark read.
    mark_read(n1).expect("read");
    assert_eq!(unread_count(), 2);
    mark_group_read(g1).expect("group_read");
    assert_eq!(unread_count(), 1);
    crate::serial_println!("  [4/8] mark read: OK");

    // 5: Expand/collapse.
    set_expanded(g1, true).expect("expand");
    let groups = get_groups();
    let mail_group = groups.iter().find(|g| g.group_id == g1).expect("find");
    assert!(mail_group.expanded);
    crate::serial_println!("  [5/8] expand: OK");

    // 6: Mute group.
    set_muted(g3, true).expect("mute");
    let groups = get_groups();
    let chat_group = groups.iter().find(|g| g.group_id == g3).expect("find2");
    assert!(chat_group.muted);
    crate::serial_println!("  [6/8] mute: OK");

    // 7: Dismiss group.
    dismiss_group(g1).expect("dismiss");
    assert_eq!(get_groups().len(), 1);
    crate::serial_println!("  [7/8] dismiss: OK");

    // 8: Stats.
    let (groups, total, _unread, dismissed, ops) = stats();
    assert_eq!(groups, 1);
    assert_eq!(total, 3);
    assert_eq!(dismissed, 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("notifgroup::self_test() — all 8 tests passed");
}

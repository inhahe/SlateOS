//! Desktop notification center.
//!
//! Provides the backend for the notification pane (toast notifications)
//! that applications send to alert users.  Notifications appear as
//! toasts and are stored in a scrollable notification history panel.
//!
//! ## Design Reference
//!
//! design.txt line 718: "a notification pane like on Windows - option
//! for any notification to not show notifications from that application
//! again"
//!
//! design.txt lines 1144-1147: "A notification daemon that programs can
//! send notifications to (like D-Bus notifications on Linux)"
//!
//! ## Architecture
//!
//! ```text
//! Application
//!   → notifcenter::send(Notification { app, title, body, ... })
//!   → notification stored in ring buffer
//!   → if app not muted → appears as toast in notification pane
//!
//! Notification pane (GUI)
//!   → notifcenter::unread() → list of unread notifications
//!   → notifcenter::history(limit) → all notifications
//!   → user clicks dismiss → notifcenter::dismiss(id)
//!   → user mutes app → notifcenter::mute_app("app-name")
//! ```
//!
//! ## Features
//!
//! - Per-app mute/unmute (notifications still stored, just not shown)
//! - Notification categories (Info, Warning, Error, Success, Progress)
//! - Priority levels (Low, Normal, High, Critical)
//! - Action buttons (up to 3 per notification)
//! - Automatic expiry (configurable TTL)
//! - Grouping by app

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum notifications in history.
const MAX_NOTIFICATIONS: usize = 1024;

/// Maximum actions per notification.
const MAX_ACTIONS: usize = 3;

/// Maximum muted apps.
const MAX_MUTED_APPS: usize = 256;

/// Default TTL in nanoseconds (24 hours).
const DEFAULT_TTL_NS: u64 = 24 * 60 * 60 * 1_000_000_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Notification category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Informational.
    Info,
    /// Warning.
    Warning,
    /// Error.
    Error,
    /// Success.
    Success,
    /// Progress update.
    Progress,
}

impl Category {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Success => "success",
            Self::Progress => "progress",
        }
    }
}

/// Notification priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Low — no sound, auto-dismiss quickly.
    Low,
    /// Normal — default behavior.
    Normal,
    /// High — plays notification sound, stays longer.
    High,
    /// Critical — stays until dismissed, cannot be muted.
    Critical,
}

impl Priority {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// An action button on a notification.
#[derive(Debug, Clone)]
pub struct NotifAction {
    /// Action identifier (sent back to the app when clicked).
    pub id: String,
    /// Display label.
    pub label: String,
}

/// A notification.
#[derive(Debug, Clone)]
pub struct Notification {
    /// Unique notification ID.
    pub id: u64,
    /// Application name that sent this notification.
    pub app: String,
    /// Notification title.
    pub title: String,
    /// Notification body text.
    pub body: String,
    /// Category.
    pub category: Category,
    /// Priority.
    pub priority: Priority,
    /// Icon identifier.
    pub icon: String,
    /// Action buttons.
    pub actions: Vec<NotifAction>,
    /// Timestamp (nanoseconds, monotonic).
    pub timestamp_ns: u64,
    /// Whether this notification has been read/dismissed.
    pub read: bool,
    /// TTL in nanoseconds (0 = no expiry).
    pub ttl_ns: u64,
    /// Optional group ID (for grouping related notifications).
    pub group: String,
    /// Optional progress percentage (0-100, for Progress category).
    pub progress: u8,
}

/// Summary of notifications for an app.
#[derive(Debug, Clone)]
pub struct AppSummary {
    /// Application name.
    pub app: String,
    /// Total notifications.
    pub total: usize,
    /// Unread notifications.
    pub unread: usize,
    /// Whether this app is muted.
    pub muted: bool,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct NotifCenter {
    /// Notifications (newest first).
    notifications: Vec<Notification>,
    /// Next notification ID.
    next_id: u64,
    /// Muted application names.
    muted_apps: BTreeSet<String>,
    /// Per-app notification count.
    app_counts: BTreeMap<String, usize>,
}

impl NotifCenter {
    const fn new() -> Self {
        Self {
            notifications: Vec::new(),
            next_id: 1,
            muted_apps: BTreeSet::new(),
            app_counts: BTreeMap::new(),
        }
    }
}

static CENTER: Mutex<NotifCenter> = Mutex::new(NotifCenter::new());
static SEND_COUNT: AtomicU64 = AtomicU64::new(0);
static DISMISS_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Send a notification.
///
/// Returns the notification ID and whether it should be shown as a toast
/// (false if the app is muted).
pub fn send(
    app: &str,
    title: &str,
    body: &str,
    category: Category,
    priority: Priority,
) -> (u64, bool) {
    send_full(app, title, body, category, priority, "", &[], "", 0)
}

/// Send a notification with full options.
pub fn send_full(
    app: &str,
    title: &str,
    body: &str,
    category: Category,
    priority: Priority,
    icon: &str,
    actions: &[(&str, &str)],
    group: &str,
    progress: u8,
) -> (u64, bool) {
    SEND_COUNT.fetch_add(1, Ordering::Relaxed);

    let now = crate::timekeeping::clock_monotonic();
    let mut center = CENTER.lock();

    let id = center.next_id;
    center.next_id = center.next_id.wrapping_add(1);

    let action_vec: Vec<NotifAction> = actions.iter()
        .take(MAX_ACTIONS)
        .map(|(aid, lbl)| NotifAction {
            id: String::from(*aid),
            label: String::from(*lbl),
        })
        .collect();

    let notif = Notification {
        id,
        app: String::from(app),
        title: String::from(title),
        body: String::from(body),
        category,
        priority,
        icon: String::from(icon),
        actions: action_vec,
        timestamp_ns: now,
        read: false,
        ttl_ns: DEFAULT_TTL_NS,
        group: String::from(group),
        progress: if progress > 100 { 100 } else { progress },
    };

    // Evict oldest if full.
    if center.notifications.len() >= MAX_NOTIFICATIONS {
        if let Some(removed) = center.notifications.pop() {
            if let Some(count) = center.app_counts.get_mut(&removed.app) {
                *count = count.saturating_sub(1);
            }
        }
    }

    let show_toast = !center.muted_apps.contains(app)
        || priority == Priority::Critical;

    // Update app counter.
    *center.app_counts.entry(String::from(app)).or_insert(0) += 1;

    center.notifications.insert(0, notif);

    (id, show_toast)
}

/// Dismiss (mark as read) a notification by ID.
pub fn dismiss(id: u64) -> KernelResult<()> {
    DISMISS_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut center = CENTER.lock();
    for notif in &mut center.notifications {
        if notif.id == id {
            notif.read = true;
            return Ok(());
        }
    }
    Err(KernelError::NotFound)
}

/// Dismiss all notifications from an app.
pub fn dismiss_app(app: &str) -> usize {
    DISMISS_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut center = CENTER.lock();
    let mut count = 0;
    for notif in &mut center.notifications {
        if notif.app == app && !notif.read {
            notif.read = true;
            count += 1;
        }
    }
    count
}

/// Dismiss all notifications.
pub fn dismiss_all() -> usize {
    DISMISS_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut center = CENTER.lock();
    let mut count = 0;
    for notif in &mut center.notifications {
        if !notif.read {
            notif.read = true;
            count += 1;
        }
    }
    count
}

/// Remove a notification entirely.
pub fn remove(id: u64) -> KernelResult<()> {
    let mut center = CENTER.lock();
    let len_before = center.notifications.len();
    let app = center.notifications.iter()
        .find(|n| n.id == id)
        .map(|n| n.app.clone());

    center.notifications.retain(|n| n.id != id);
    if center.notifications.len() == len_before {
        return Err(KernelError::NotFound);
    }
    if let Some(app_name) = app {
        if let Some(count) = center.app_counts.get_mut(&app_name) {
            *count = count.saturating_sub(1);
        }
    }
    Ok(())
}

/// Clear all notifications.
pub fn clear_all_notifications() {
    let mut center = CENTER.lock();
    center.notifications.clear();
    center.app_counts.clear();
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Get unread notifications.
pub fn unread() -> Vec<Notification> {
    let center = CENTER.lock();
    center.notifications.iter()
        .filter(|n| !n.read)
        .cloned()
        .collect()
}

/// Get unread count.
pub fn unread_count() -> usize {
    let center = CENTER.lock();
    center.notifications.iter().filter(|n| !n.read).count()
}

/// Get notification history (newest first).
pub fn history(limit: usize) -> Vec<Notification> {
    let center = CENTER.lock();
    let max = if limit == 0 { MAX_NOTIFICATIONS } else { limit };
    center.notifications.iter().take(max).cloned().collect()
}

/// Get notifications for a specific app.
pub fn app_notifications(app: &str, limit: usize) -> Vec<Notification> {
    let center = CENTER.lock();
    let max = if limit == 0 { MAX_NOTIFICATIONS } else { limit };
    center.notifications.iter()
        .filter(|n| n.app == app)
        .take(max)
        .cloned()
        .collect()
}

/// Get per-app summaries.
pub fn app_summaries() -> Vec<AppSummary> {
    let center = CENTER.lock();
    let mut apps: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for notif in &center.notifications {
        let entry = apps.entry(notif.app.clone()).or_insert((0, 0));
        entry.0 += 1;
        if !notif.read {
            entry.1 += 1;
        }
    }
    apps.into_iter().map(|(app, (total, unread_n))| {
        AppSummary {
            muted: center.muted_apps.contains(&app),
            app,
            total,
            unread: unread_n,
        }
    }).collect()
}

/// Get a notification by ID.
pub fn get(id: u64) -> Option<Notification> {
    let center = CENTER.lock();
    center.notifications.iter().find(|n| n.id == id).cloned()
}

// ---------------------------------------------------------------------------
// Mute management
// ---------------------------------------------------------------------------

/// Mute notifications from an app (they're still stored, just not toasted).
pub fn mute_app(app: &str) -> KernelResult<()> {
    let mut center = CENTER.lock();
    if center.muted_apps.len() >= MAX_MUTED_APPS {
        return Err(KernelError::ResourceExhausted);
    }
    center.muted_apps.insert(String::from(app));
    Ok(())
}

/// Unmute an app.
pub fn unmute_app(app: &str) -> KernelResult<()> {
    let mut center = CENTER.lock();
    if !center.muted_apps.remove(app) {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

/// Check if an app is muted.
pub fn is_muted(app: &str) -> bool {
    let center = CENTER.lock();
    center.muted_apps.contains(app)
}

/// List muted apps.
pub fn muted_apps() -> Vec<String> {
    let center = CENTER.lock();
    center.muted_apps.iter().cloned().collect()
}

// ---------------------------------------------------------------------------
// Expiry
// ---------------------------------------------------------------------------

/// Remove expired notifications.
pub fn expire() -> usize {
    let now = crate::timekeeping::clock_monotonic();
    let mut center = CENTER.lock();
    let len_before = center.notifications.len();
    center.notifications.retain(|n| {
        if n.ttl_ns == 0 {
            return true; // No expiry.
        }
        now.saturating_sub(n.timestamp_ns) < n.ttl_ns
    });
    len_before - center.notifications.len()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (total, unread, muted_apps, send_ops, dismiss_ops).
pub fn stats() -> (usize, usize, usize, u64, u64) {
    let center = CENTER.lock();
    let total = center.notifications.len();
    let unread_n = center.notifications.iter().filter(|n| !n.read).count();
    let muted = center.muted_apps.len();
    (
        total,
        unread_n,
        muted,
        SEND_COUNT.load(Ordering::Relaxed),
        DISMISS_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    SEND_COUNT.store(0, Ordering::Relaxed);
    DISMISS_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut center = CENTER.lock();
    center.notifications.clear();
    center.next_id = 1;
    center.muted_apps.clear();
    center.app_counts.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the notification center.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: send and retrieve.
    {
        let (id, show) = send("TestApp", "Hello", "World", Category::Info, Priority::Normal);
        assert!(id > 0);
        assert!(show);
        let notif = get(id).unwrap();
        assert_eq!(notif.title, "Hello");
        assert_eq!(notif.body, "World");
        assert!(!notif.read);
        serial_println!("[notifcenter] test 1 passed: send/get");
    }

    // Test 2: unread count.
    {
        send("TestApp", "Second", "Notification", Category::Info, Priority::Normal);
        send("OtherApp", "Third", "Alert", Category::Warning, Priority::High);
        assert_eq!(unread_count(), 3);
        serial_println!("[notifcenter] test 2 passed: unread count");
    }

    // Test 3: dismiss.
    {
        let unread_list = unread();
        assert_eq!(unread_list.len(), 3);
        dismiss(unread_list[0].id)?;
        assert_eq!(unread_count(), 2);
        serial_println!("[notifcenter] test 3 passed: dismiss");
    }

    // Test 4: mute app.
    {
        mute_app("TestApp")?;
        assert!(is_muted("TestApp"));
        let (_, show) = send("TestApp", "Muted", "Test", Category::Info, Priority::Normal);
        assert!(!show); // Muted, should not show.
        // But critical notifications bypass mute.
        let (_, show) = send("TestApp", "Critical", "Alert", Category::Error, Priority::Critical);
        assert!(show);
        serial_println!("[notifcenter] test 4 passed: mute/unmute");
    }

    // Test 5: app summaries.
    {
        let summaries = app_summaries();
        assert!(summaries.len() >= 2);
        let test_app = summaries.iter().find(|s| s.app == "TestApp");
        assert!(test_app.is_some());
        assert!(test_app.unwrap().muted);
        serial_println!("[notifcenter] test 5 passed: app summaries");
    }

    // Test 6: dismiss all for app.
    {
        let dismissed = dismiss_app("TestApp");
        assert!(dismissed > 0);
        let remaining = app_notifications("TestApp", 0);
        assert!(remaining.iter().all(|n| n.read));
        serial_println!("[notifcenter] test 6 passed: dismiss app");
    }

    // Test 7: remove and history.
    {
        let hist = history(100);
        let first_id = hist[0].id;
        remove(first_id)?;
        let hist2 = history(100);
        assert_eq!(hist2.len(), hist.len() - 1);
        serial_println!("[notifcenter] test 7 passed: remove/history");
    }

    clear_all();
    reset_stats();

    serial_println!("[notifcenter] all 7 self-tests passed");
    Ok(())
}

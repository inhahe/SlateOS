//! Data usage monitoring — per-application network bandwidth tracking.
//!
//! Tracks network data usage per application, per interface, and over
//! time.  Provides daily/monthly summaries, usage limits with alerts,
//! and metered connection support.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Network → Data Usage
//!   → datausage::set_limit() / usage_summary()
//!
//! Network stack integration
//!   → datausage::record_usage(app_id, rx, tx) per packet/connection
//!
//! Integration:
//!   → netsettings (interface identification)
//!   → appregistry (app name lookup)
//!   → notifcenter (limit exceeded alerts)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_APPS: usize = 256;
const MAX_DAILY_RECORDS: usize = 90;   // 90 days of history
const MAX_LIMITS: usize = 16;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Time period for usage tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsagePeriod {
    Today,
    ThisWeek,
    ThisMonth,
    Last30Days,
    AllTime,
}

impl UsagePeriod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::ThisWeek => "This Week",
            Self::ThisMonth => "This Month",
            Self::Last30Days => "Last 30 Days",
            Self::AllTime => "All Time",
        }
    }
}

/// Per-application data usage entry.
#[derive(Debug, Clone)]
pub struct AppUsage {
    /// Application ID.
    pub app_id: String,
    /// Total bytes received.
    pub rx_bytes: u64,
    /// Total bytes sent.
    pub tx_bytes: u64,
    /// Connection count.
    pub connection_count: u64,
    /// Last activity timestamp (ns).
    pub last_activity_ns: u64,
}

impl AppUsage {
    pub fn total_bytes(&self) -> u64 {
        self.rx_bytes.saturating_add(self.tx_bytes)
    }
}

/// Daily usage summary.
#[derive(Debug, Clone)]
pub struct DailyUsage {
    /// Day identifier (days since epoch, simplified).
    pub day: u32,
    /// Total bytes received.
    pub rx_bytes: u64,
    /// Total bytes sent.
    pub tx_bytes: u64,
    /// Per-app breakdown.
    pub apps: Vec<(String, u64, u64)>, // (app_id, rx, tx)
}

/// Data usage limit.
#[derive(Debug, Clone)]
pub struct UsageLimit {
    /// Limit name.
    pub name: String,
    /// Byte limit.
    pub limit_bytes: u64,
    /// Period (days).
    pub period_days: u32,
    /// Whether to alert when approaching limit.
    pub alert_enabled: bool,
    /// Alert threshold (percentage, 0-100).
    pub alert_pct: u8,
    /// Whether the limit has been exceeded.
    pub exceeded: bool,
    /// Interface filter (empty = all).
    pub interface: String,
    /// Whether to block traffic after limit.
    pub block_on_exceed: bool,
}

/// Metered connection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeteredStatus {
    /// Not metered — unlimited usage.
    Unmetered,
    /// Metered — reduce background data.
    Metered,
    /// Roaming — minimize all data.
    Roaming,
}

impl MeteredStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unmetered => "Unmetered",
            Self::Metered => "Metered",
            Self::Roaming => "Roaming",
        }
    }
}

/// Usage summary for a period.
#[derive(Debug, Clone)]
pub struct UsageSummary {
    pub period: UsagePeriod,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub app_count: usize,
    pub top_apps: Vec<(String, u64)>, // (app_id, total bytes)
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct DataUsageState {
    apps: Vec<AppUsage>,
    daily: Vec<DailyUsage>,
    limits: Vec<UsageLimit>,
    metered: MeteredStatus,
    current_day: u32,
    total_rx: u64,
    total_tx: u64,
    ops: u64,
}

static STATE: Mutex<Option<DataUsageState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut DataUsageState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

fn current_day() -> u32 {
    // Approximate day from elapsed nanoseconds.
    // In a real OS this would use the RTC/wall clock.
    (crate::hpet::elapsed_ns() / (86_400_000_000_000u64)) as u32
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the data usage monitoring subsystem.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(DataUsageState {
        apps: Vec::new(),
        daily: Vec::new(),
        limits: Vec::new(),
        metered: MeteredStatus::Unmetered,
        current_day: current_day(),
        total_rx: 0,
        total_tx: 0,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Recording usage
// ---------------------------------------------------------------------------

/// Record network usage for an application.
///
/// Called by the network stack for each connection/packet batch.
pub fn record_usage(app_id: &str, rx_bytes: u64, tx_bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();

        // Update per-app totals.
        if let Some(app) = state.apps.iter_mut().find(|a| a.app_id == app_id) {
            app.rx_bytes = app.rx_bytes.saturating_add(rx_bytes);
            app.tx_bytes = app.tx_bytes.saturating_add(tx_bytes);
            app.connection_count += 1;
            app.last_activity_ns = now;
        } else {
            if state.apps.len() >= MAX_APPS {
                // Remove least recently used.
                if let Some(pos) = state.apps.iter()
                    .enumerate()
                    .min_by_key(|(_, a)| a.last_activity_ns)
                    .map(|(i, _)| i)
                {
                    state.apps.remove(pos);
                }
            }
            state.apps.push(AppUsage {
                app_id: String::from(app_id),
                rx_bytes,
                tx_bytes,
                connection_count: 1,
                last_activity_ns: now,
            });
        }

        // Update daily totals.
        let today = current_day();
        if today != state.current_day {
            state.current_day = today;
        }

        if let Some(daily) = state.daily.iter_mut().find(|d| d.day == today) {
            daily.rx_bytes = daily.rx_bytes.saturating_add(rx_bytes);
            daily.tx_bytes = daily.tx_bytes.saturating_add(tx_bytes);
            // Update per-app in daily.
            if let Some(app_entry) = daily.apps.iter_mut().find(|(id, _, _)| id == app_id) {
                app_entry.1 = app_entry.1.saturating_add(rx_bytes);
                app_entry.2 = app_entry.2.saturating_add(tx_bytes);
            } else {
                daily.apps.push((String::from(app_id), rx_bytes, tx_bytes));
            }
        } else {
            if state.daily.len() >= MAX_DAILY_RECORDS {
                state.daily.remove(0);
            }
            state.daily.push(DailyUsage {
                day: today,
                rx_bytes,
                tx_bytes,
                apps: alloc::vec![(String::from(app_id), rx_bytes, tx_bytes)],
            });
        }

        state.total_rx = state.total_rx.saturating_add(rx_bytes);
        state.total_tx = state.total_tx.saturating_add(tx_bytes);

        // Check limits.
        for limit in &mut state.limits {
            let usage = state.total_rx.saturating_add(state.total_tx);
            if usage >= limit.limit_bytes {
                limit.exceeded = true;
            }
        }

        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Metered connection
// ---------------------------------------------------------------------------

/// Set metered connection status.
pub fn set_metered(status: MeteredStatus) -> KernelResult<()> {
    with_state(|state| {
        state.metered = status;
        Ok(())
    })
}

/// Get metered connection status.
pub fn metered_status() -> MeteredStatus {
    let guard = STATE.lock();
    guard.as_ref().map_or(MeteredStatus::Unmetered, |s| s.metered)
}

/// Check if background data should be restricted.
pub fn should_restrict_background() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| {
        s.metered != MeteredStatus::Unmetered
            || s.limits.iter().any(|l| l.exceeded && l.block_on_exceed)
    })
}

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Add a data usage limit.
pub fn add_limit(name: &str, limit_bytes: u64, period_days: u32) -> KernelResult<()> {
    if name.is_empty() || limit_bytes == 0 || period_days == 0 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.limits.len() >= MAX_LIMITS {
            return Err(KernelError::ResourceExhausted);
        }
        state.limits.push(UsageLimit {
            name: String::from(name),
            limit_bytes,
            period_days,
            alert_enabled: true,
            alert_pct: 80,
            exceeded: false,
            interface: String::new(),
            block_on_exceed: false,
        });
        Ok(())
    })
}

/// Remove a limit by name.
pub fn remove_limit(name: &str) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.limits.iter().position(|l| l.name == name) {
            state.limits.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// List all limits.
pub fn list_limits() -> Vec<UsageLimit> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.limits.clone())
}

/// Set block-on-exceed for a limit.
pub fn set_block_on_exceed(name: &str, block: bool) -> KernelResult<()> {
    with_state(|state| {
        let limit = state.limits.iter_mut()
            .find(|l| l.name == name)
            .ok_or(KernelError::NotFound)?;
        limit.block_on_exceed = block;
        Ok(())
    })
}

/// Set alert threshold percentage.
pub fn set_alert_threshold(name: &str, pct: u8) -> KernelResult<()> {
    if pct > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let limit = state.limits.iter_mut()
            .find(|l| l.name == name)
            .ok_or(KernelError::NotFound)?;
        limit.alert_pct = pct;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Get per-app usage list sorted by total bytes (descending).
pub fn app_usage() -> Vec<AppUsage> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        let mut apps = s.apps.clone();
        apps.sort_by_key(|e| core::cmp::Reverse(e.total_bytes()));
        apps
    })
}

/// Get usage for a specific app.
pub fn usage_for_app(app_id: &str) -> KernelResult<AppUsage> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.apps.iter()
        .find(|a| a.app_id == app_id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// Get daily usage history.
pub fn daily_history() -> Vec<DailyUsage> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.daily.clone())
}

/// Get usage summary for a period.
pub fn usage_summary(period: UsagePeriod) -> UsageSummary {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return UsageSummary {
            period,
            rx_bytes: 0,
            tx_bytes: 0,
            app_count: 0,
            top_apps: Vec::new(),
        },
    };

    // For simplicity, use cumulative totals (a real implementation
    // would filter by date ranges).
    let (rx, tx) = match period {
        UsagePeriod::AllTime => (state.total_rx, state.total_tx),
        _ => {
            // Use daily records for period.
            let days = match period {
                UsagePeriod::Today => 1,
                UsagePeriod::ThisWeek => 7,
                UsagePeriod::ThisMonth | UsagePeriod::Last30Days => 30,
                UsagePeriod::AllTime => 0, // handled above
            };
            let today = current_day();
            let start = today.saturating_sub(days);
            let mut rx = 0u64;
            let mut tx = 0u64;
            for d in &state.daily {
                if d.day >= start {
                    rx = rx.saturating_add(d.rx_bytes);
                    tx = tx.saturating_add(d.tx_bytes);
                }
            }
            (rx, tx)
        }
    };

    let mut top: Vec<(String, u64)> = state.apps.iter()
        .map(|a| (a.app_id.clone(), a.total_bytes()))
        .collect();
    top.sort_by_key(|e| core::cmp::Reverse(e.1));
    top.truncate(10);

    UsageSummary {
        period,
        rx_bytes: rx,
        tx_bytes: tx,
        app_count: state.apps.len(),
        top_apps: top,
    }
}

/// Reset all usage counters.
pub fn reset_usage() -> KernelResult<()> {
    with_state(|state| {
        state.apps.clear();
        state.daily.clear();
        state.total_rx = 0;
        state.total_tx = 0;
        for limit in &mut state.limits {
            limit.exceeded = false;
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format byte count as human-readable string.
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{}.{} GB", bytes / 1_073_741_824, (bytes % 1_073_741_824) / 107_374_182)
    } else if bytes >= 1_048_576 {
        format!("{}.{} MB", bytes / 1_048_576, (bytes % 1_048_576) / 104_857)
    } else if bytes >= 1024 {
        format!("{}.{} KB", bytes / 1024, (bytes % 1024) / 102)
    } else {
        format!("{} B", bytes)
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (app_count, daily_records, total_rx, total_tx, limit_count, ops).
pub fn stats() -> (usize, usize, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.apps.len(), s.daily.len(), s.total_rx, s.total_tx, s.limits.len(), s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the data usage module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[datausage] Running self-tests...");

    // Reset state.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state.
    {
        let (apps, daily, rx, tx, limits, _) = stats();
        assert_eq!(apps, 0);
        assert_eq!(daily, 0);
        assert_eq!(rx, 0);
        assert_eq!(tx, 0);
        assert_eq!(limits, 0);
    }
    serial_println!("[datausage]  1/11 initial state OK");

    // Test 2: record usage.
    {
        record_usage("firefox", 1024, 256).unwrap();
        let app = usage_for_app("firefox").unwrap();
        assert_eq!(app.rx_bytes, 1024);
        assert_eq!(app.tx_bytes, 256);
        assert_eq!(app.total_bytes(), 1280);
    }
    serial_println!("[datausage]  2/11 record usage OK");

    // Test 3: cumulative tracking.
    {
        record_usage("firefox", 2048, 512).unwrap();
        let app = usage_for_app("firefox").unwrap();
        assert_eq!(app.rx_bytes, 3072);
        assert_eq!(app.tx_bytes, 768);
    }
    serial_println!("[datausage]  3/11 cumulative OK");

    // Test 4: multiple apps.
    {
        record_usage("chromium", 5000, 1000).unwrap();
        let apps = app_usage();
        assert_eq!(apps.len(), 2);
        // chromium has more total bytes.
        assert_eq!(apps.first().unwrap().app_id, "chromium");
    }
    serial_println!("[datausage]  4/11 multiple apps OK");

    // Test 5: metered status.
    {
        assert_eq!(metered_status(), MeteredStatus::Unmetered);
        set_metered(MeteredStatus::Metered).unwrap();
        assert_eq!(metered_status(), MeteredStatus::Metered);
        assert!(should_restrict_background());
        set_metered(MeteredStatus::Unmetered).unwrap();
    }
    serial_println!("[datausage]  5/11 metered status OK");

    // Test 6: add limit.
    {
        add_limit("Monthly Cap", 10_000_000_000, 30).unwrap();
        let limits = list_limits();
        assert_eq!(limits.len(), 1);
        assert_eq!(limits.first().unwrap().name, "Monthly Cap");
    }
    serial_println!("[datausage]  6/11 add limit OK");

    // Test 7: limit exceeded.
    {
        add_limit("Tiny Limit", 100, 1).unwrap();
        // Already have more than 100 bytes recorded.
        record_usage("test", 1, 1).unwrap();
        let limits = list_limits();
        let tiny = limits.iter().find(|l| l.name == "Tiny Limit").unwrap();
        assert!(tiny.exceeded);
    }
    serial_println!("[datausage]  7/11 limit exceeded OK");

    // Test 8: remove limit.
    {
        remove_limit("Tiny Limit").unwrap();
        let limits = list_limits();
        assert!(!limits.iter().any(|l| l.name == "Tiny Limit"));
    }
    serial_println!("[datausage]  8/11 remove limit OK");

    // Test 9: usage summary.
    {
        let summary = usage_summary(UsagePeriod::AllTime);
        assert!(summary.rx_bytes > 0);
        assert!(summary.tx_bytes > 0);
        assert!(summary.app_count > 0);
        assert!(!summary.top_apps.is_empty());
    }
    serial_println!("[datausage]  9/11 usage summary OK");

    // Test 10: format_bytes.
    {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        let mb = format_bytes(1_500_000);
        assert!(mb.contains("MB"));
    }
    serial_println!("[datausage] 10/11 format_bytes OK");

    // Test 11: reset.
    {
        reset_usage().unwrap();
        let (apps, _, rx, tx, _, _) = stats();
        assert_eq!(apps, 0);
        assert_eq!(rx, 0);
        assert_eq!(tx, 0);
    }
    serial_println!("[datausage] 11/11 reset OK");

    // Leave no residue for later callers / the live /proc/datausage view:
    // reset_usage() clears per-app totals but the "Monthly Cap" limit added in
    // test 6 persists, and these are test fixtures, not real recorded usage.
    // Reset to None so the procfs view and `datausage` shell command report an
    // empty, never-measured state.
    *STATE.lock() = None;

    serial_println!("[datausage] All self-tests passed.");
}

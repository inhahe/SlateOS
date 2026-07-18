//! Time Synchronization — NTP client and clock management.
//!
//! Manages system clock synchronization via NTP servers, tracks
//! drift, skew, and sync status, and maintains a list of configured
//! time sources.
//!
//! ## Architecture
//!
//! ```text
//! Time synchronization
//!   → timesync::sync_now() → trigger NTP poll (simulated)
//!   → timesync::status() → current sync state
//!   → timesync::add_server(addr) → add NTP server
//!
//! Integration:
//!   → timezone (time zone)
//!   → netsettings (network configuration)
//!   → sysinfo (system information)
//!   → eventlog (event logging)
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

/// Sync status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    Unsynchronized,
    Synchronizing,
    Synchronized,
    Error,
}

impl SyncStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unsynchronized => "Unsynchronized",
            Self::Synchronizing => "Synchronizing",
            Self::Synchronized => "Synchronized",
            Self::Error => "Error",
        }
    }
}

/// NTP server stratum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stratum {
    Primary,     // Stratum 1
    Secondary,   // Stratum 2
    Tertiary,    // Stratum 3
    Unknown,
}

impl Stratum {
    pub fn label(self) -> &'static str {
        match self {
            Self::Primary => "Stratum 1",
            Self::Secondary => "Stratum 2",
            Self::Tertiary => "Stratum 3",
            Self::Unknown => "Unknown",
        }
    }
}

/// An NTP server entry.
#[derive(Debug, Clone)]
pub struct NtpServer {
    pub id: u32,
    pub address: String,
    pub stratum: Stratum,
    pub enabled: bool,
    pub last_poll_ns: u64,
    pub offset_us: i64,     // Clock offset in microseconds.
    pub delay_us: u64,      // Round-trip delay in microseconds.
    pub jitter_us: u64,     // Jitter in microseconds.
    pub poll_count: u64,
    pub error_count: u64,
}

/// Sync history record.
#[derive(Debug, Clone)]
pub struct SyncRecord {
    pub timestamp_ns: u64,
    pub server_id: u32,
    pub offset_us: i64,
    pub delay_us: u64,
    pub success: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SERVERS: usize = 16;
const MAX_HISTORY: usize = 200;

struct State {
    servers: Vec<NtpServer>,
    history: Vec<SyncRecord>,
    next_id: u32,
    status: SyncStatus,
    last_sync_ns: u64,
    total_syncs: u64,
    total_errors: u64,
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
        servers: alloc::vec![
            NtpServer {
                id: 1, address: String::from("pool.ntp.org"),
                stratum: Stratum::Secondary, enabled: true,
                last_poll_ns: 0, offset_us: 0, delay_us: 0,
                jitter_us: 0, poll_count: 0, error_count: 0,
            },
            NtpServer {
                id: 2, address: String::from("time.google.com"),
                stratum: Stratum::Primary, enabled: true,
                last_poll_ns: 0, offset_us: 0, delay_us: 0,
                jitter_us: 0, poll_count: 0, error_count: 0,
            },
        ],
        history: Vec::new(),
        next_id: 3,
        status: SyncStatus::Unsynchronized,
        last_sync_ns: 0,
        total_syncs: 0,
        total_errors: 0,
        ops: 0,
    });
}

/// List all NTP servers.
pub fn list_servers() -> Vec<NtpServer> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.servers.clone())
}

/// Add an NTP server.
pub fn add_server(address: &str, stratum: Stratum) -> KernelResult<u32> {
    with_state(|state| {
        if state.servers.len() >= MAX_SERVERS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.servers.iter().any(|s| s.address == address) {
            return Err(KernelError::AlreadyExists);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.servers.push(NtpServer {
            id, address: String::from(address), stratum, enabled: true,
            last_poll_ns: 0, offset_us: 0, delay_us: 0,
            jitter_us: 0, poll_count: 0, error_count: 0,
        });
        Ok(id)
    })
}

/// Remove an NTP server.
pub fn remove_server(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.servers.len();
        state.servers.retain(|s| s.id != id);
        if state.servers.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Simulate an NTP sync.
pub fn sync_now() -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let mut synced = false;
        for srv in state.servers.iter_mut() {
            if !srv.enabled { continue; }
            srv.poll_count += 1;
            srv.last_poll_ns = now;
            // Simulated offset/delay.
            srv.offset_us = 42;
            srv.delay_us = 1200;
            srv.jitter_us = 15;
            synced = true;
            if state.history.len() >= MAX_HISTORY {
                state.history.remove(0);
            }
            state.history.push(SyncRecord {
                timestamp_ns: now, server_id: srv.id,
                offset_us: srv.offset_us, delay_us: srv.delay_us,
                success: true,
            });
        }
        if synced {
            state.status = SyncStatus::Synchronized;
            state.last_sync_ns = now;
            state.total_syncs += 1;
        } else {
            state.status = SyncStatus::Error;
            state.total_errors += 1;
        }
        Ok(())
    })
}

/// Get current sync status.
pub fn get_status() -> SyncStatus {
    STATE.lock().as_ref().map_or(SyncStatus::Unsynchronized, |s| s.status)
}

/// Get sync history.
pub fn sync_history() -> Vec<SyncRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.history.clone())
}

/// Enable/disable a server.
pub fn set_server_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let srv = state.servers.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        srv.enabled = enabled;
        Ok(())
    })
}

/// Statistics: (server_count, total_syncs, total_errors, last_sync_ns, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.servers.len(), s.total_syncs, s.total_errors, s.last_sync_ns, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("timesync::self_test() — running tests...");
    init_defaults();

    // 1: Default servers.
    assert_eq!(list_servers().len(), 2);
    assert_eq!(get_status(), SyncStatus::Unsynchronized);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Add server.
    let id = add_server("time.nist.gov", Stratum::Primary).expect("add");
    assert_eq!(list_servers().len(), 3);
    crate::serial_println!("  [2/8] add server: OK");

    // 3: Duplicate rejected.
    assert!(add_server("time.nist.gov", Stratum::Primary).is_err());
    crate::serial_println!("  [3/8] duplicate: OK");

    // 4: Sync.
    sync_now().expect("sync");
    assert_eq!(get_status(), SyncStatus::Synchronized);
    crate::serial_println!("  [4/8] sync: OK");

    // 5: History.
    let hist = sync_history();
    assert!(hist.len() >= 3); // 3 enabled servers polled.
    assert!(hist.iter().all(|r| r.success));
    crate::serial_println!("  [5/8] history: OK");

    // 6: Disable server.
    set_server_enabled(id, false).expect("disable");
    let srvs = list_servers();
    let srv = srvs.iter().find(|s| s.id == id).expect("find");
    assert!(!srv.enabled);
    crate::serial_println!("  [6/8] disable: OK");

    // 7: Remove server.
    remove_server(id).expect("remove");
    assert_eq!(list_servers().len(), 2);
    assert!(remove_server(999).is_err());
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (count, syncs, errors, last, ops) = stats();
    assert_eq!(count, 2);
    assert!(syncs >= 1);
    let _ = errors;
    assert!(last > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("timesync::self_test() — all 8 tests passed");
}

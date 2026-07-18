//! FS Cache — filesystem cache policy management.
//!
//! Manages per-device cache policies (write-back, write-through),
//! read-ahead settings, dirty ratio thresholds, and cache flush
//! scheduling.
//!
//! ## Architecture
//!
//! ```text
//! FS cache management
//!   → fscache::set_policy(dev, policy) → set cache policy
//!   → fscache::set_readahead(dev, pages) → set read-ahead
//!   → fscache::flush(dev) → force cache flush
//!   → fscache::stats() → cache statistics
//!
//! Integration:
//!   → cache (buffer cache)
//!   → diskio (disk I/O)
//!   → iosched (I/O scheduler)
//!   → perfmon (performance monitor)
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

/// Cache write policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    WriteBack,
    WriteThrough,
    WriteAround,
    None,
}

impl CachePolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::WriteBack => "Write-back",
            Self::WriteThrough => "Write-through",
            Self::WriteAround => "Write-around",
            Self::None => "None",
        }
    }
}

/// Per-device cache configuration.
#[derive(Debug, Clone)]
pub struct DeviceCacheConfig {
    pub device_name: String,
    pub policy: CachePolicy,
    pub readahead_pages: u32,
    pub dirty_ratio_pct: u32,
    pub dirty_bg_ratio_pct: u32,
    pub flush_interval_ms: u64,
    pub cached_pages: u64,
    pub dirty_pages: u64,
    pub total_flushes: u64,
    pub total_readaheads: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 32;

struct State {
    devices: Vec<DeviceCacheConfig>,
    total_flushes: u64,
    total_readaheads: u64,
    total_evictions: u64,
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
        devices: alloc::vec![
            DeviceCacheConfig {
                device_name: String::from("sda"),
                policy: CachePolicy::WriteBack, readahead_pages: 128,
                dirty_ratio_pct: 20, dirty_bg_ratio_pct: 10,
                flush_interval_ms: 5000, cached_pages: 1024,
                dirty_pages: 50, total_flushes: 0, total_readaheads: 0,
            },
            DeviceCacheConfig {
                device_name: String::from("nvme0n1"),
                policy: CachePolicy::WriteThrough, readahead_pages: 256,
                dirty_ratio_pct: 40, dirty_bg_ratio_pct: 20,
                flush_interval_ms: 3000, cached_pages: 4096,
                dirty_pages: 0, total_flushes: 0, total_readaheads: 0,
            },
        ],
        total_flushes: 0,
        total_readaheads: 0,
        total_evictions: 0,
        ops: 0,
    });
}

/// List device cache configs.
pub fn list_devices() -> Vec<DeviceCacheConfig> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Get config for a device.
pub fn get_device(name: &str) -> Option<DeviceCacheConfig> {
    STATE.lock().as_ref().and_then(|s| s.devices.iter().find(|d| d.device_name == name).cloned())
}

/// Set cache policy for a device.
pub fn set_policy(device: &str, policy: CachePolicy) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device_name == device)
            .ok_or(KernelError::NotFound)?;
        dev.policy = policy;
        Ok(())
    })
}

/// Set read-ahead size.
pub fn set_readahead(device: &str, pages: u32) -> KernelResult<()> {
    with_state(|state| {
        if pages > 8192 { return Err(KernelError::InvalidArgument); }
        let dev = state.devices.iter_mut().find(|d| d.device_name == device)
            .ok_or(KernelError::NotFound)?;
        dev.readahead_pages = pages;
        Ok(())
    })
}

/// Set dirty ratio.
pub fn set_dirty_ratio(device: &str, ratio_pct: u32, bg_ratio_pct: u32) -> KernelResult<()> {
    with_state(|state| {
        if ratio_pct > 100 || bg_ratio_pct > 100 || bg_ratio_pct > ratio_pct {
            return Err(KernelError::InvalidArgument);
        }
        let dev = state.devices.iter_mut().find(|d| d.device_name == device)
            .ok_or(KernelError::NotFound)?;
        dev.dirty_ratio_pct = ratio_pct;
        dev.dirty_bg_ratio_pct = bg_ratio_pct;
        Ok(())
    })
}

/// Flush cache for a device (simulated).
pub fn flush(device: &str) -> KernelResult<u64> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device_name == device)
            .ok_or(KernelError::NotFound)?;
        let flushed = dev.dirty_pages;
        dev.dirty_pages = 0;
        dev.total_flushes += 1;
        state.total_flushes += 1;
        Ok(flushed)
    })
}

/// Add a device.
pub fn add_device(name: &str, policy: CachePolicy) -> KernelResult<()> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES { return Err(KernelError::ResourceExhausted); }
        if state.devices.iter().any(|d| d.device_name == name) { return Err(KernelError::AlreadyExists); }
        state.devices.push(DeviceCacheConfig {
            device_name: String::from(name), policy, readahead_pages: 128,
            dirty_ratio_pct: 20, dirty_bg_ratio_pct: 10,
            flush_interval_ms: 5000, cached_pages: 0,
            dirty_pages: 0, total_flushes: 0, total_readaheads: 0,
        });
        Ok(())
    })
}

/// Statistics: (device_count, total_flushes, total_readaheads, total_evictions, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_flushes, s.total_readaheads, s.total_evictions, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("fscache::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_devices().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get device.
    let d = get_device("sda").expect("get");
    assert_eq!(d.policy, CachePolicy::WriteBack);
    assert_eq!(d.readahead_pages, 128);
    crate::serial_println!("  [2/8] get: OK");

    // 3: Set policy.
    set_policy("sda", CachePolicy::WriteThrough).expect("policy");
    let d = get_device("sda").expect("get2");
    assert_eq!(d.policy, CachePolicy::WriteThrough);
    crate::serial_println!("  [3/8] policy: OK");

    // 4: Read-ahead.
    set_readahead("sda", 512).expect("ra");
    let d = get_device("sda").expect("get3");
    assert_eq!(d.readahead_pages, 512);
    assert!(set_readahead("sda", 99999).is_err());
    crate::serial_println!("  [4/8] readahead: OK");

    // 5: Dirty ratio.
    set_dirty_ratio("sda", 30, 15).expect("ratio");
    assert!(set_dirty_ratio("sda", 10, 20).is_err()); // bg > ratio.
    crate::serial_println!("  [5/8] dirty ratio: OK");

    // 6: Flush.
    set_policy("sda", CachePolicy::WriteBack).expect("wb");
    let flushed = flush("sda").expect("flush");
    assert_eq!(flushed, 50); // From init dirty_pages.
    let d = get_device("sda").expect("get4");
    assert_eq!(d.dirty_pages, 0);
    crate::serial_println!("  [6/8] flush: OK");

    // 7: Add device.
    add_device("vdb", CachePolicy::WriteAround).expect("add");
    assert_eq!(list_devices().len(), 3);
    assert!(add_device("vdb", CachePolicy::None).is_err());
    crate::serial_println!("  [7/8] add: OK");

    // 8: Stats.
    let (devs, flushes, readaheads, evictions, ops) = stats();
    assert_eq!(devs, 3);
    assert!(flushes >= 1);
    let _ = (readaheads, evictions);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("fscache::self_test() — all 8 tests passed");
}

//! Swap configuration — swap file/partition management and tuning.
//!
//! Manages swap space for the OS: creating, resizing, and removing swap
//! files or swap partitions, setting swappiness, and monitoring usage.
//!
//! ## Design Reference
//!
//! design.txt line 1252: "change size of swap file (if we're not using a
//! swap partition instead)"
//!
//! design.txt line 1348 (installer): "Swap files are much more convenient
//! (resize without repartitioning). On SSDs, the performance difference is
//! negligible. Use a swap file as default."
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Memory → Swap
//!   → swapcfg::set_swap_size(bytes)
//!   → swapcfg::set_swappiness(0-100)
//!   → swapcfg::enable()/disable()
//!
//! Memory manager
//!   → swapcfg::get_config() → SwapConfig
//!   → swapcfg::swap_usage() → current usage stats
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

/// Type of swap space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapType {
    /// Swap file on the root filesystem (default, recommended).
    File,
    /// Dedicated swap partition.
    Partition,
    /// Compressed swap in RAM (zswap/zram).
    Compressed,
}

/// Priority level for a swap area (higher = used first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SwapPriority(pub i32);

/// A swap area (file or partition).
#[derive(Debug, Clone)]
pub struct SwapArea {
    /// Unique ID.
    pub id: u64,
    /// Type of swap.
    pub swap_type: SwapType,
    /// Path to swap file or device.
    pub path: String,
    /// Total size in bytes.
    pub size_bytes: u64,
    /// Currently used bytes.
    pub used_bytes: u64,
    /// Priority (higher = preferred).
    pub priority: SwapPriority,
    /// Whether this swap area is active.
    pub active: bool,
    /// Label/description.
    pub label: String,
}

/// Global swap configuration.
#[derive(Debug, Clone)]
pub struct SwapConfig {
    /// Swappiness value (0-100). 0 = avoid swap, 100 = swap aggressively.
    pub swappiness: u32,
    /// Whether swap is globally enabled.
    pub enabled: bool,
    /// Minimum free memory before swap starts (bytes).
    pub min_free_bytes: u64,
    /// Whether to use zswap (compressed cache before disk swap).
    pub zswap_enabled: bool,
    /// zswap compression algorithm.
    pub zswap_algorithm: String,
    /// zswap maximum pool size as percent of total RAM (1-50).
    pub zswap_max_pool_pct: u32,
    /// Hibernate swap area ID (0 = none).
    pub hibernate_swap_id: u64,
}

/// Swap usage statistics.
#[derive(Debug, Clone)]
pub struct SwapUsage {
    /// Total swap space in bytes.
    pub total_bytes: u64,
    /// Used swap space in bytes.
    pub used_bytes: u64,
    /// Free swap space in bytes.
    pub free_bytes: u64,
    /// Number of active swap areas.
    pub active_areas: u32,
    /// Page-in count.
    pub pages_in: u64,
    /// Page-out count.
    pub pages_out: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: SwapConfig,
    areas: Vec<SwapArea>,
    pages_in: u64,
    pages_out: u64,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    config: SwapConfig {
        swappiness: 60,
        enabled: true,
        min_free_bytes: 64 * 1024 * 1024, // 64 MiB
        zswap_enabled: false,
        zswap_algorithm: String::new(),
        zswap_max_pool_pct: 20,
        hibernate_swap_id: 0,
    },
    areas: Vec::new(),
    pages_in: 0,
    pages_out: 0,
    changes: 0,
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get current swap configuration.
pub fn get_config() -> SwapConfig {
    STATE.lock().config.clone()
}

/// Set swappiness (0-100).
pub fn set_swappiness(value: u32) -> KernelResult<()> {
    if value > 100 {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    state.config.swappiness = value;
    state.changes += 1;
    Ok(())
}

/// Enable or disable swap globally.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.enabled = enabled;
    state.changes += 1;
    Ok(())
}

/// Set minimum free memory threshold.
pub fn set_min_free(bytes: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.config.min_free_bytes = bytes;
    state.changes += 1;
    Ok(())
}

/// Enable or disable zswap (compressed swap cache).
pub fn set_zswap(enabled: bool, algorithm: &str, max_pool_pct: u32) -> KernelResult<()> {
    if max_pool_pct > 50 {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    state.config.zswap_enabled = enabled;
    state.config.zswap_algorithm = String::from(algorithm);
    state.config.zswap_max_pool_pct = max_pool_pct;
    state.changes += 1;
    Ok(())
}

/// Set which swap area to use for hibernation.
pub fn set_hibernate_swap(swap_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if swap_id != 0 && !state.areas.iter().any(|a| a.id == swap_id) {
        return Err(KernelError::NotFound);
    }
    state.config.hibernate_swap_id = swap_id;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Swap areas
// ---------------------------------------------------------------------------

/// Add a swap area (file or partition).
pub fn add_swap(
    swap_type: SwapType,
    path: &str,
    size_bytes: u64,
    priority: i32,
    label: &str,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.areas.len() >= 32 {
        return Err(KernelError::ResourceExhausted);
    }
    if state.areas.iter().any(|a| a.path == path) {
        return Err(KernelError::AlreadyExists);
    }
    if size_bytes == 0 {
        return Err(KernelError::InvalidArgument);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.areas.push(SwapArea {
        id,
        swap_type,
        path: String::from(path),
        size_bytes,
        used_bytes: 0,
        priority: SwapPriority(priority),
        active: false,
        label: String::from(label),
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Remove a swap area (must be inactive).
pub fn remove_swap(swap_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let area = state
        .areas
        .iter()
        .find(|a| a.id == swap_id)
        .ok_or(KernelError::NotFound)?;
    if area.active {
        return Err(KernelError::WouldBlock);
    }
    state.areas.retain(|a| a.id != swap_id);
    // Clear hibernate reference if needed.
    if state.config.hibernate_swap_id == swap_id {
        state.config.hibernate_swap_id = 0;
    }
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Activate a swap area.
pub fn activate(swap_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let area = state
        .areas
        .iter_mut()
        .find(|a| a.id == swap_id)
        .ok_or(KernelError::NotFound)?;
    area.active = true;
    state.changes += 1;
    Ok(())
}

/// Deactivate a swap area.
pub fn deactivate(swap_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let area = state
        .areas
        .iter_mut()
        .find(|a| a.id == swap_id)
        .ok_or(KernelError::NotFound)?;
    if area.used_bytes > 0 {
        // Would need to migrate pages first — simplified.
        area.used_bytes = 0;
    }
    area.active = false;
    state.changes += 1;
    Ok(())
}

/// Resize a swap area (must be inactive).
pub fn resize(swap_id: u64, new_size_bytes: u64) -> KernelResult<()> {
    if new_size_bytes == 0 {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    let area = state
        .areas
        .iter_mut()
        .find(|a| a.id == swap_id)
        .ok_or(KernelError::NotFound)?;
    if area.active {
        return Err(KernelError::WouldBlock);
    }
    area.size_bytes = new_size_bytes;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Set priority for a swap area.
pub fn set_priority(swap_id: u64, priority: i32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let area = state
        .areas
        .iter_mut()
        .find(|a| a.id == swap_id)
        .ok_or(KernelError::NotFound)?;
    area.priority = SwapPriority(priority);
    state.changes += 1;
    Ok(())
}

/// Get a swap area by ID.
pub fn get_swap(swap_id: u64) -> KernelResult<SwapArea> {
    let state = STATE.lock();
    state
        .areas
        .iter()
        .find(|a| a.id == swap_id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all swap areas.
pub fn list_swaps() -> Vec<SwapArea> {
    STATE.lock().areas.clone()
}

/// Get current swap usage statistics.
pub fn usage() -> SwapUsage {
    let state = STATE.lock();
    let total: u64 = state.areas.iter().filter(|a| a.active).map(|a| a.size_bytes).sum();
    let used: u64 = state.areas.iter().filter(|a| a.active).map(|a| a.used_bytes).sum();
    let active = state.areas.iter().filter(|a| a.active).count() as u32;
    SwapUsage {
        total_bytes: total,
        used_bytes: used,
        free_bytes: total.saturating_sub(used),
        active_areas: active,
        pages_in: state.pages_in,
        pages_out: state.pages_out,
    }
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialize with default swap configuration.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.areas.is_empty() {
        return;
    }

    state.config = SwapConfig {
        swappiness: 60,
        enabled: true,
        min_free_bytes: 64 * 1024 * 1024, // 64 MiB
        zswap_enabled: true,
        zswap_algorithm: String::from("lz4"),
        zswap_max_pool_pct: 20,
        hibernate_swap_id: 0,
    };

    // Default swap file — recommended over partition per design.txt.
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.areas.push(SwapArea {
        id,
        swap_type: SwapType::File,
        path: String::from("/swapfile"),
        size_bytes: 4 * 1024 * 1024 * 1024, // 4 GiB
        used_bytes: 0,
        priority: SwapPriority(0),
        active: true,
        label: String::from("default"),
    });

    // Compressed swap (zram).
    let id2 = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.areas.push(SwapArea {
        id: id2,
        swap_type: SwapType::Compressed,
        path: String::from("/dev/zram0"),
        size_bytes: 2 * 1024 * 1024 * 1024, // 2 GiB
        used_bytes: 0,
        priority: SwapPriority(100), // higher priority = used first
        active: true,
        label: String::from("zram"),
    });

    state.changes += 1;
}

/// Return (area_count, active_count, total_bytes, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = STATE.lock();
    let total = state.areas.len();
    let active = state.areas.iter().filter(|a| a.active).count();
    let bytes: u64 = state.areas.iter().map(|a| a.size_bytes).sum();
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (total, active, bytes, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.areas.clear();
    state.pages_in = 0;
    state.pages_out = 0;
    state.config = SwapConfig {
        swappiness: 60,
        enabled: true,
        min_free_bytes: 64 * 1024 * 1024,
        zswap_enabled: false,
        zswap_algorithm: String::new(),
        zswap_max_pool_pct: 20,
        hibernate_swap_id: 0,
    };
    state.changes = 0;
    NEXT_ID.store(1, Ordering::Relaxed);
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: add swap areas.
    serial_println!("swapcfg::self_test 1: add swap areas");
    let s1 = add_swap(SwapType::File, "/swapfile", 4_000_000_000, 0, "main")?;
    let s2 = add_swap(SwapType::Compressed, "/dev/zram0", 2_000_000_000, 100, "zram")?;
    assert_eq!(list_swaps().len(), 2);
    // Duplicate path fails.
    assert!(add_swap(SwapType::File, "/swapfile", 1_000, 0, "dup").is_err());

    // Test 2: activate/deactivate.
    serial_println!("swapcfg::self_test 2: activate/deactivate");
    activate(s1)?;
    activate(s2)?;
    let u = usage();
    assert_eq!(u.active_areas, 2);
    assert_eq!(u.total_bytes, 6_000_000_000);
    deactivate(s1)?;
    let u = usage();
    assert_eq!(u.active_areas, 1);

    // Test 3: resize.
    serial_println!("swapcfg::self_test 3: resize");
    resize(s1, 8_000_000_000)?;
    let area = get_swap(s1)?;
    assert_eq!(area.size_bytes, 8_000_000_000);
    // Cannot resize active area.
    assert!(resize(s2, 1_000_000_000).is_err());

    // Test 4: swappiness and config.
    serial_println!("swapcfg::self_test 4: configuration");
    set_swappiness(80)?;
    set_min_free(128 * 1024 * 1024)?;
    let cfg = get_config();
    assert_eq!(cfg.swappiness, 80);
    assert_eq!(cfg.min_free_bytes, 128 * 1024 * 1024);
    // Invalid swappiness rejected.
    assert!(set_swappiness(101).is_err());

    // Test 5: zswap settings.
    serial_println!("swapcfg::self_test 5: zswap");
    set_zswap(true, "lz4", 25)?;
    let cfg = get_config();
    assert!(cfg.zswap_enabled);
    assert_eq!(cfg.zswap_algorithm, "lz4");
    assert_eq!(cfg.zswap_max_pool_pct, 25);
    assert!(set_zswap(true, "lz4", 60).is_err()); // > 50%

    // Test 6: priority.
    serial_println!("swapcfg::self_test 6: priority");
    set_priority(s1, 50)?;
    let area = get_swap(s1)?;
    assert_eq!(area.priority.0, 50);

    // Test 7: remove.
    serial_println!("swapcfg::self_test 7: remove");
    // Cannot remove active area.
    assert!(remove_swap(s2).is_err());
    deactivate(s2)?;
    remove_swap(s2)?;
    assert_eq!(list_swaps().len(), 1);
    remove_swap(s1)?;
    assert!(list_swaps().is_empty());

    clear_all();
    serial_println!("swapcfg::self_test: all 7 tests passed");
    Ok(())
}

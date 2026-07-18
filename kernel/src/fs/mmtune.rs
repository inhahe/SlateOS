//! Memory management tuning parameters — settings panel for MM configuration.
//!
//! Provides a settings interface for choosing and tuning the memory management
//! and paging model.  Users can select workload-type presets that populate
//! tuning parameters with recommended values.  Shows advantages and disadvantages
//! of each configuration.
//!
//! ## Design Reference
//!
//! design.txt line 1276: memory management model/tuning parameters
//! design.txt line 1278: paging model/tuning parameters
//! design.txt line 1279: show advantages and disadvantages of each profile
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Memory Tuning
//!   → mmtune::list_profiles() → available tuning profiles
//!   → mmtune::apply_profile(id) → set active configuration
//!   → mmtune::tradeoffs(id) → advantages & disadvantages
//!
//! Kernel MM reads active config
//!   → mmtune::active_config() → current tuning parameters
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

/// Page allocator model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocModel {
    /// Buddy allocator (Linux-style, power-of-2 splitting).
    Buddy,
    /// Slab allocator for small objects + buddy for large.
    SlabBuddy,
    /// Bitmap allocator (simple, no fragmentation, but O(n) scan).
    Bitmap,
    /// Zone-based (separate pools for DMA, normal, high memory).
    ZoneBased,
}

/// Page reclaim strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimStrategy {
    /// LRU (least recently used) page eviction.
    Lru,
    /// Clock algorithm (second-chance LRU).
    Clock,
    /// MGLRU (multi-generational LRU, Linux 6.1+).
    MultiGenLru,
    /// Working set model (track per-process working sets).
    WorkingSet,
}

/// Overcommit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OvercommitMode {
    /// Never overcommit — all allocations must be backed.
    Never,
    /// Heuristic overcommit (allow reasonable excess).
    Heuristic,
    /// Always overcommit (accept all allocations, OOM-kill later).
    Always,
}

/// Huge page mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HugePageMode {
    /// No huge pages.
    Disabled,
    /// Explicit madvise-only huge pages.
    MadviseOnly,
    /// Transparent huge pages (auto-promoted).
    Transparent,
    /// Always use huge pages where possible.
    Always,
}

/// Compaction (defragmentation) aggressiveness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactLevel {
    /// No proactive compaction.
    Off,
    /// Light compaction on allocation failure.
    Light,
    /// Background compaction daemon.
    Background,
    /// Aggressive compaction (always try to maintain contiguous regions).
    Aggressive,
}

/// Workload type for preset selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadType {
    /// General desktop.
    Desktop,
    /// Server / database.
    Server,
    /// Gaming — minimise page faults.
    Gaming,
    /// Development — frequent allocation patterns.
    Development,
    /// Embedded / low-memory.
    LowMemory,
    /// Virtualisation host.
    VmHost,
}

/// A complete set of memory management tuning parameters.
#[derive(Debug, Clone)]
pub struct MmConfig {
    /// Unique ID.
    pub id: u64,
    /// Profile name.
    pub name: String,
    /// Workload type.
    pub workload: WorkloadType,
    /// Page allocator model.
    pub alloc_model: AllocModel,
    /// Page reclaim strategy.
    pub reclaim: ReclaimStrategy,
    /// Overcommit mode.
    pub overcommit: OvercommitMode,
    /// Overcommit ratio (0-100, percentage of physical RAM).
    pub overcommit_ratio: u8,
    /// Huge page mode.
    pub huge_pages: HugePageMode,
    /// Reserved huge pages count.
    pub huge_page_reserve: u32,
    /// Compaction level.
    pub compact_level: CompactLevel,
    /// Swappiness (0-200, tendency to swap vs reclaim cache).
    pub swappiness: u16,
    /// Dirty ratio (percent of RAM before synchronous writeback).
    pub dirty_ratio: u8,
    /// Background dirty ratio (percent, triggers async writeback).
    pub dirty_bg_ratio: u8,
    /// Dirty expire centiseconds (how long dirty pages can linger).
    pub dirty_expire_cs: u32,
    /// VFS cache pressure (0-1000, higher = more aggressive inode/dentry reclaim).
    pub vfs_cache_pressure: u16,
    /// Min free kibibytes (watermark for emergency reserves).
    pub min_free_kib: u32,
    /// Whether to enable ZRAM (compressed swap in RAM).
    pub zram_enabled: bool,
    /// ZRAM max percent of RAM.
    pub zram_max_pct: u8,
    /// Whether to zero pages on free (security feature).
    pub zero_on_free: bool,
    /// Whether to use per-CPU page caches.
    pub per_cpu_pages: bool,
    /// NUMA interleave for anonymous pages.
    pub numa_interleave: bool,
    /// Kernel stack size in pages (1-4).
    pub kernel_stack_pages: u8,
    /// Whether this is a built-in profile.
    pub builtin: bool,
    /// Whether this profile is currently active.
    pub active: bool,
    /// Requires recompile to apply (e.g., alloc model change).
    pub requires_recompile: bool,
    /// Requires reboot to apply.
    pub requires_reboot: bool,
}

/// Tradeoff description for user display.
#[derive(Debug, Clone)]
pub struct TradeoffInfo {
    /// Short label.
    pub label: String,
    /// Advantages.
    pub advantages: Vec<String>,
    /// Disadvantages.
    pub disadvantages: Vec<String>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    profiles: Vec<MmConfig>,
    tradeoffs: Vec<(u64, TradeoffInfo)>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    profiles: Vec::new(),
    tradeoffs: Vec::new(),
    changes: 0,
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Profile management
// ---------------------------------------------------------------------------

/// Create a tuning profile.
pub fn create_profile(
    name: &str,
    workload: WorkloadType,
    alloc_model: AllocModel,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.profiles.len() >= 64 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let cfg = defaults_for(id, name, workload, alloc_model);
    state.profiles.push(cfg);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

fn defaults_for(id: u64, name: &str, workload: WorkloadType, alloc_model: AllocModel) -> MmConfig {
    let (reclaim, overcommit, oc_ratio, huge, compact, swappiness,
         dirty_r, dirty_bg, dirty_exp, cache_press, min_free, zram, zram_pct,
         zero_free, numa_il) = match workload {
        WorkloadType::Desktop => (
            ReclaimStrategy::MultiGenLru, OvercommitMode::Never, 0,
            HugePageMode::Transparent, CompactLevel::Light,
            60, 20, 10, 3000, 100, 32768, true, 25, false, false,
        ),
        WorkloadType::Server => (
            ReclaimStrategy::MultiGenLru, OvercommitMode::Never, 0,
            HugePageMode::MadviseOnly, CompactLevel::Background,
            10, 40, 10, 500, 50, 65536, false, 0, false, true,
        ),
        WorkloadType::Gaming => (
            ReclaimStrategy::WorkingSet, OvercommitMode::Never, 0,
            HugePageMode::Transparent, CompactLevel::Aggressive,
            10, 30, 15, 3000, 100, 65536, true, 50, false, false,
        ),
        WorkloadType::Development => (
            ReclaimStrategy::MultiGenLru, OvercommitMode::Heuristic, 50,
            HugePageMode::Transparent, CompactLevel::Light,
            80, 20, 10, 3000, 100, 32768, true, 30, false, false,
        ),
        WorkloadType::LowMemory => (
            ReclaimStrategy::Clock, OvercommitMode::Never, 0,
            HugePageMode::Disabled, CompactLevel::Off,
            100, 10, 5, 1000, 200, 16384, true, 50, false, false,
        ),
        WorkloadType::VmHost => (
            ReclaimStrategy::Lru, OvercommitMode::Heuristic, 80,
            HugePageMode::Always, CompactLevel::Background,
            30, 40, 20, 500, 50, 131072, false, 0, false, true,
        ),
    };

    MmConfig {
        id,
        name: String::from(name),
        workload,
        alloc_model,
        reclaim,
        overcommit,
        overcommit_ratio: oc_ratio,
        huge_pages: huge,
        huge_page_reserve: 0,
        compact_level: compact,
        swappiness,
        dirty_ratio: dirty_r,
        dirty_bg_ratio: dirty_bg,
        dirty_expire_cs: dirty_exp,
        vfs_cache_pressure: cache_press,
        min_free_kib: min_free,
        zram_enabled: zram,
        zram_max_pct: zram_pct,
        zero_on_free: zero_free,
        per_cpu_pages: true,
        numa_interleave: numa_il,
        kernel_stack_pages: 2,
        builtin: false,
        active: false,
        requires_recompile: alloc_model != AllocModel::SlabBuddy,
        requires_reboot: true,
    }
}

/// Remove a profile (built-in and active are protected).
pub fn remove_profile(profile_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    if p.builtin || p.active {
        return Err(KernelError::PermissionDenied);
    }
    state.profiles.retain(|p| p.id != profile_id);
    state.tradeoffs.retain(|(pid, _)| *pid != profile_id);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get a profile by ID.
pub fn get_profile(profile_id: u64) -> KernelResult<MmConfig> {
    STATE.lock().profiles.iter().find(|p| p.id == profile_id).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all profiles.
pub fn list_profiles() -> Vec<MmConfig> {
    STATE.lock().profiles.clone()
}

/// Get the active profile.
pub fn active_profile() -> KernelResult<MmConfig> {
    STATE.lock().profiles.iter().find(|p| p.active).cloned()
        .ok_or(KernelError::NotFound)
}

/// Activate a profile.
pub fn apply_profile(profile_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.profiles.iter().any(|p| p.id == profile_id) {
        return Err(KernelError::NotFound);
    }
    for p in &mut state.profiles {
        p.active = p.id == profile_id;
    }
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Parameter tuning
// ---------------------------------------------------------------------------

/// Set overcommit mode.
pub fn set_overcommit(profile_id: u64, mode: OvercommitMode) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.overcommit = mode;
    state.changes += 1;
    Ok(())
}

/// Set overcommit ratio.
pub fn set_overcommit_ratio(profile_id: u64, ratio: u8) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.overcommit_ratio = ratio.min(100);
    state.changes += 1;
    Ok(())
}

/// Set huge page mode.
pub fn set_huge_pages(profile_id: u64, mode: HugePageMode) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.huge_pages = mode;
    state.changes += 1;
    Ok(())
}

/// Set compaction level.
pub fn set_compact_level(profile_id: u64, level: CompactLevel) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.compact_level = level;
    state.changes += 1;
    Ok(())
}

/// Set swappiness (0-200).
pub fn set_swappiness(profile_id: u64, val: u16) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.swappiness = val.min(200);
    state.changes += 1;
    Ok(())
}

/// Set dirty ratio.
pub fn set_dirty_ratio(profile_id: u64, ratio: u8) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.dirty_ratio = ratio.clamp(1, 90);
    state.changes += 1;
    Ok(())
}

/// Set background dirty ratio.
pub fn set_dirty_bg_ratio(profile_id: u64, ratio: u8) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.dirty_bg_ratio = ratio.clamp(1, 50);
    state.changes += 1;
    Ok(())
}

/// Set VFS cache pressure.
pub fn set_cache_pressure(profile_id: u64, val: u16) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.vfs_cache_pressure = val.min(1000);
    state.changes += 1;
    Ok(())
}

/// Set ZRAM enabled.
pub fn set_zram_enabled(profile_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.zram_enabled = enabled;
    state.changes += 1;
    Ok(())
}

/// Set zero-on-free.
pub fn set_zero_on_free(profile_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.zero_on_free = enabled;
    state.changes += 1;
    Ok(())
}

/// Set reclaim strategy.
pub fn set_reclaim(profile_id: u64, strategy: ReclaimStrategy) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.reclaim = strategy;
    state.changes += 1;
    Ok(())
}

/// Set min free kibibytes.
pub fn set_min_free(profile_id: u64, kib: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.min_free_kib = kib.clamp(1024, 1_048_576);
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tradeoffs
// ---------------------------------------------------------------------------

/// Get tradeoff info for a profile.
pub fn tradeoffs(profile_id: u64) -> KernelResult<TradeoffInfo> {
    let state = STATE.lock();
    if !state.profiles.iter().any(|p| p.id == profile_id) {
        return Err(KernelError::NotFound);
    }
    state.tradeoffs.iter()
        .find(|(pid, _)| *pid == profile_id)
        .map(|(_, info)| info.clone())
        .ok_or(KernelError::NotFound)
}

/// List all tradeoffs.
pub fn list_tradeoffs() -> Vec<(u64, TradeoffInfo)> {
    STATE.lock().tradeoffs.clone()
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

fn add_builtin(state: &mut State, name: &str, workload: WorkloadType, alloc_model: AllocModel,
    active: bool, advantages: &[&str], disadvantages: &[&str])
{
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut cfg = defaults_for(id, name, workload, alloc_model);
    cfg.builtin = true;
    cfg.active = active;
    state.profiles.push(cfg);

    let info = TradeoffInfo {
        label: String::from(name),
        advantages: advantages.iter().map(|s| String::from(*s)).collect(),
        disadvantages: disadvantages.iter().map(|s| String::from(*s)).collect(),
    };
    state.tradeoffs.push((id, info));
}

/// Initialise default memory tuning profiles.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.profiles.is_empty() {
        return;
    }

    add_builtin(&mut state, "Desktop (Default)", WorkloadType::Desktop,
        AllocModel::SlabBuddy, true,
        &["Balanced memory usage and performance",
          "Transparent huge pages reduce TLB misses",
          "MGLRU provides good page aging accuracy",
          "ZRAM extends effective memory with compression",
          "Committed memory prevents OOM surprises"],
        &["Slightly higher memory overhead than minimal config",
          "Compaction can cause brief latency spikes"]);

    add_builtin(&mut state, "Server / Database", WorkloadType::Server,
        AllocModel::SlabBuddy, false,
        &["High dirty ratios improve write throughput",
          "Low swappiness keeps working set in RAM",
          "Background compaction maintains contiguous regions",
          "NUMA interleave reduces hotspot contention",
          "Large min_free reserve prevents allocation stalls"],
        &["Higher memory overhead for reserves",
          "Madvise-only huge pages require application support",
          "Low swappiness may cause OOM under memory pressure"]);

    add_builtin(&mut state, "Gaming", WorkloadType::Gaming,
        AllocModel::SlabBuddy, false,
        &["Working set tracking minimises page faults",
          "Aggressive compaction reduces allocation latency",
          "Transparent huge pages for game memory",
          "High ZRAM ratio for background process compression",
          "Very low swappiness keeps game in RAM"],
        &["High memory consumption",
          "Aggressive compaction uses CPU cycles",
          "Background apps may be heavily compressed"]);

    add_builtin(&mut state, "Development", WorkloadType::Development,
        AllocModel::SlabBuddy, false,
        &["Heuristic overcommit allows fork-heavy workflows",
          "Higher swappiness helps with many open projects",
          "MGLRU handles diverse allocation patterns well",
          "ZRAM compression for build caches"],
        &["Overcommit may lead to OOM under extreme load",
          "Higher swappiness may swap out active data"]);

    add_builtin(&mut state, "Low Memory / Embedded", WorkloadType::LowMemory,
        AllocModel::Bitmap, false,
        &["Minimal allocator overhead",
          "Maximum swap usage frees physical RAM",
          "No huge page overhead",
          "High ZRAM compression ratio",
          "Aggressive VFS cache reclaim"],
        &["Slower allocation (bitmap scan)",
          "No huge page benefits (more TLB misses)",
          "Heavy swapping hurts responsiveness",
          "Requires recompile for allocator change"]);

    add_builtin(&mut state, "VM Host", WorkloadType::VmHost,
        AllocModel::ZoneBased, false,
        &["Zone-based allocation isolates VM memory",
          "Always-on huge pages for VM backing",
          "NUMA interleave for balanced VM placement",
          "Background compaction for huge page availability",
          "Large reserves prevent host-level OOM"],
        &["High memory overhead for huge page reserves",
          "Zone-based allocator requires recompile",
          "Overcommit risk if VMs oversubscribed"]);

    state.changes += 1;
}

/// Return (profile_count, active_count, tradeoff_count, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    let total = state.profiles.len();
    let active = state.profiles.iter().filter(|p| p.active).count();
    let tradeoffs = state.tradeoffs.len();
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (total, active, tradeoffs, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.profiles.clear();
    state.tradeoffs.clear();
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

    // Test 1: init defaults.
    serial_println!("mmtune::self_test 1: init defaults");
    init_defaults();
    let profiles = list_profiles();
    assert!(profiles.len() >= 6);
    let active = active_profile()?;
    assert_eq!(active.workload, WorkloadType::Desktop);

    // Test 2: create custom.
    serial_println!("mmtune::self_test 2: create custom");
    clear_all();
    let p1 = create_profile("Custom1", WorkloadType::Gaming, AllocModel::SlabBuddy)?;
    let p2 = create_profile("Custom2", WorkloadType::Server, AllocModel::Buddy)?;
    assert_eq!(list_profiles().len(), 2);

    // Test 3: apply.
    serial_println!("mmtune::self_test 3: apply");
    apply_profile(p1)?;
    let a = active_profile()?;
    assert_eq!(a.id, p1);
    apply_profile(p2)?;
    let a = active_profile()?;
    assert_eq!(a.id, p2);

    // Test 4: tune parameters.
    serial_println!("mmtune::self_test 4: tuning");
    set_overcommit(p1, OvercommitMode::Heuristic)?;
    set_overcommit_ratio(p1, 75)?;
    set_huge_pages(p1, HugePageMode::Always)?;
    set_compact_level(p1, CompactLevel::Aggressive)?;
    set_swappiness(p1, 30)?;
    set_dirty_ratio(p1, 40)?;
    set_dirty_bg_ratio(p1, 20)?;
    set_cache_pressure(p1, 200)?;
    set_zram_enabled(p1, false)?;
    set_zero_on_free(p1, true)?;
    set_reclaim(p1, ReclaimStrategy::Lru)?;
    set_min_free(p1, 65536)?;
    let cfg = get_profile(p1)?;
    assert_eq!(cfg.overcommit, OvercommitMode::Heuristic);
    assert_eq!(cfg.overcommit_ratio, 75);
    assert_eq!(cfg.huge_pages, HugePageMode::Always);
    assert_eq!(cfg.swappiness, 30);
    assert_eq!(cfg.dirty_ratio, 40);
    assert!(cfg.zero_on_free);
    assert_eq!(cfg.reclaim, ReclaimStrategy::Lru);

    // Test 5: remove protection.
    serial_println!("mmtune::self_test 5: remove protection");
    assert!(remove_profile(p2).is_err()); // p2 is active
    apply_profile(p1)?;
    remove_profile(p2)?;
    assert_eq!(list_profiles().len(), 1);

    // Test 6: builtin protection.
    serial_println!("mmtune::self_test 6: builtin protection");
    clear_all();
    init_defaults();
    let builtins = list_profiles();
    assert!(remove_profile(builtins[0].id).is_err());

    // Test 7: tradeoffs.
    serial_println!("mmtune::self_test 7: tradeoffs");
    let info = tradeoffs(builtins[0].id)?;
    assert!(!info.advantages.is_empty());
    assert!(!info.disadvantages.is_empty());

    clear_all();
    serial_println!("mmtune::self_test: all 7 tests passed");
    Ok(())
}

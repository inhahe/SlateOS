//! Filesystem tuning parameters — per-filesystem configuration and presets.
//!
//! Manages tunable parameters for filesystem creation and runtime behaviour:
//! block size, journal mode, reserved space, inode density, commit intervals,
//! and workload-type presets that set sensible defaults.
//!
//! ## Design Reference
//!
//! design.txt line 1280: "filesystem tuning parameters? (obviously, would
//! have to change every file and directory in the partition, but we could
//! also set these for a filesystem not yet put onto a partition)"
//!
//! design.txt line 1279: "show advantages and disadvantages of each
//! model/param profile for ... filesystem"
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Storage → Filesystem Tuning
//!   → fstune::get_profile(fs_id) → TuneProfile
//!   → fstune::apply_profile(fs_id, WorkloadType)
//!   → fstune::set_param(fs_id, param, value)
//!
//! mkfs / partition manager
//!   → fstune::defaults_for(fs_type, workload) → TuneProfile
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

/// Filesystem type for tuning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsType {
    Ext4,
    Btrfs,
    Xfs,
    F2fs,
    Fat32,
}

/// Workload type determines default tuning parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadType {
    /// General desktop use: balanced read/write, many small files.
    Desktop,
    /// Database server: large sequential writes, fsync-heavy.
    Database,
    /// File/web server: high concurrency, many open files.
    Server,
    /// Software development: many small files, frequent metadata ops.
    Development,
    /// Gaming: large sequential reads, fast load times.
    Gaming,
}

/// Journal mode for journaling filesystems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalMode {
    /// Journal metadata only (default, fastest).
    Ordered,
    /// Journal data and metadata (safest, slowest).
    Journal,
    /// Write data before metadata; no data journaling (fastest, risk).
    Writeback,
    /// No journal (not recommended).
    Off,
}

/// Block allocation strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocStrategy {
    /// Spread allocations across groups (default for general use).
    Spread,
    /// Pack allocations tightly (better for many small files).
    Pack,
    /// Prefer sequential allocation (better for large files).
    Sequential,
}

/// A single tunable parameter value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamValue {
    U32(u32),
    U64(u64),
    Bool(bool),
    Str(String),
}

/// Complete tuning profile for a filesystem.
#[derive(Debug, Clone)]
pub struct TuneProfile {
    /// Unique ID for this profile instance.
    pub id: u64,
    /// Human-readable name.
    pub name: String,
    /// Filesystem type.
    pub fs_type: FsType,
    /// Workload this profile is optimised for.
    pub workload: WorkloadType,
    /// Block size in bytes (typically 4096 or 16384).
    pub block_size: u32,
    /// Journal mode.
    pub journal_mode: JournalMode,
    /// Journal commit interval in seconds (1-600).
    pub commit_interval_secs: u32,
    /// Percentage of blocks reserved for root (0-50).
    pub reserved_pct: u32,
    /// Inode ratio — bytes per inode for inode table sizing.
    /// Lower = more inodes (good for many small files).
    /// Typical: 4096 (many files) to 65536 (few large files).
    pub inode_ratio: u32,
    /// Enable directory indexing (htree for ext4).
    pub dir_index: bool,
    /// Block allocation strategy.
    pub alloc_strategy: AllocStrategy,
    /// Enable lazy initialisation of inode tables.
    pub lazy_init: bool,
    /// Enable discard/TRIM for SSD.
    pub discard: bool,
    /// Enable extended attributes.
    pub xattr: bool,
    /// Stripe width for RAID (0 = not RAID).
    pub stripe_width: u32,
    /// Max directory size hint in entries (0 = unlimited).
    pub max_dir_size: u32,
    /// Enable data checksumming (btrfs/f2fs).
    pub data_checksum: bool,
    /// Enable transparent compression (btrfs/f2fs).
    pub compression: bool,
    /// Compression algorithm name (if compression enabled).
    pub compression_algo: String,
    /// Whether this profile has been applied to a live filesystem.
    pub applied: bool,
    /// Timestamp of last modification (ns).
    pub modified_ns: u64,
}

/// Advantage/disadvantage description for a tuning choice.
#[derive(Debug, Clone)]
pub struct TradeoffInfo {
    pub param_name: String,
    pub value_desc: String,
    pub advantages: Vec<String>,
    pub disadvantages: Vec<String>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    profiles: Vec<TuneProfile>,
    tradeoffs: Vec<TradeoffInfo>,
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
// Default profiles per workload
// ---------------------------------------------------------------------------

fn base_profile(name: &str, fs_type: FsType, workload: WorkloadType) -> TuneProfile {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    TuneProfile {
        id,
        name: String::from(name),
        fs_type,
        workload,
        block_size: 4096,
        journal_mode: JournalMode::Ordered,
        commit_interval_secs: 5,
        reserved_pct: 5,
        inode_ratio: 16384,
        dir_index: true,
        alloc_strategy: AllocStrategy::Spread,
        lazy_init: true,
        discard: false,
        xattr: true,
        stripe_width: 0,
        max_dir_size: 0,
        data_checksum: false,
        compression: false,
        compression_algo: String::new(),
        applied: false,
        modified_ns: 0,
    }
}

/// Build a profile with workload-specific defaults.
pub fn defaults_for(fs_type: FsType, workload: WorkloadType) -> TuneProfile {
    let name = match workload {
        WorkloadType::Desktop => "desktop",
        WorkloadType::Database => "database",
        WorkloadType::Server => "server",
        WorkloadType::Development => "development",
        WorkloadType::Gaming => "gaming",
    };
    let mut p = base_profile(name, fs_type, workload);

    match workload {
        WorkloadType::Desktop => {
            // Balanced defaults.
            p.block_size = 4096;
            p.inode_ratio = 16384;
            p.commit_interval_secs = 5;
            p.reserved_pct = 5;
            p.discard = true; // SSDs common on desktops
        }
        WorkloadType::Database => {
            // Larger blocks, safer journal, lower commit interval.
            p.block_size = 4096;
            p.inode_ratio = 65536; // few large files
            p.journal_mode = JournalMode::Journal; // safest
            p.commit_interval_secs = 1; // frequent fsync
            p.reserved_pct = 1;
            p.alloc_strategy = AllocStrategy::Sequential;
        }
        WorkloadType::Server => {
            // Many concurrent opens, balanced allocation.
            p.block_size = 4096;
            p.inode_ratio = 8192; // moderate file count
            p.commit_interval_secs = 3;
            p.reserved_pct = 5;
            p.dir_index = true;
        }
        WorkloadType::Development => {
            // Many small files (source trees, node_modules, etc.).
            p.block_size = 4096;
            p.inode_ratio = 4096; // many inodes for small files
            p.commit_interval_secs = 5;
            p.reserved_pct = 3;
            p.alloc_strategy = AllocStrategy::Pack;
        }
        WorkloadType::Gaming => {
            // Large sequential reads, few huge files.
            p.block_size = 4096;
            p.inode_ratio = 65536; // few files
            p.journal_mode = JournalMode::Writeback; // speed over safety
            p.commit_interval_secs = 15; // less frequent commits
            p.reserved_pct = 1;
            p.alloc_strategy = AllocStrategy::Sequential;
            p.discard = true;
        }
    }

    // Filesystem-specific overrides.
    match fs_type {
        FsType::Btrfs => {
            p.data_checksum = true;
            if workload == WorkloadType::Server || workload == WorkloadType::Development {
                p.compression = true;
                p.compression_algo = String::from("zstd");
            }
        }
        FsType::F2fs => {
            p.discard = true; // F2FS is flash-optimised
            p.data_checksum = true;
        }
        FsType::Fat32 => {
            // FAT32 has very limited tuning.
            p.journal_mode = JournalMode::Off;
            p.dir_index = false;
            p.xattr = false;
            p.reserved_pct = 0;
        }
        _ => {}
    }

    p
}

// ---------------------------------------------------------------------------
// Profile management
// ---------------------------------------------------------------------------

/// Create a new tuning profile from workload defaults.
pub fn create_profile(
    name: &str,
    fs_type: FsType,
    workload: WorkloadType,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.profiles.len() >= 64 {
        return Err(KernelError::ResourceExhausted);
    }
    if state.profiles.iter().any(|p| p.name == name) {
        return Err(KernelError::AlreadyExists);
    }
    let mut profile = defaults_for(fs_type, workload);
    profile.name = String::from(name);
    let id = profile.id;
    state.profiles.push(profile);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Remove a profile.
pub fn remove_profile(profile_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state
        .profiles
        .iter()
        .position(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    state.profiles.remove(idx);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get a profile by ID.
pub fn get_profile(profile_id: u64) -> KernelResult<TuneProfile> {
    let state = STATE.lock();
    state
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all profiles.
pub fn list_profiles() -> Vec<TuneProfile> {
    STATE.lock().profiles.clone()
}

/// Apply workload preset to an existing profile (resets tunables to workload defaults).
pub fn apply_workload(profile_id: u64, workload: WorkloadType) -> KernelResult<()> {
    let mut state = STATE.lock();
    let profile = state
        .profiles
        .iter_mut()
        .find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    let defaults = defaults_for(profile.fs_type, workload);
    let saved_id = profile.id;
    let saved_name = core::mem::take(&mut profile.name);
    *profile = defaults;
    profile.id = saved_id;
    profile.name = saved_name;
    profile.workload = workload;
    profile.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Individual parameter setters
// ---------------------------------------------------------------------------

/// Set block size (must be power of two, 512..=65536).
pub fn set_block_size(profile_id: u64, size: u32) -> KernelResult<()> {
    if !size.is_power_of_two() || size < 512 || size > 65536 {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.block_size = size;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set journal mode.
pub fn set_journal_mode(profile_id: u64, mode: JournalMode) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    if p.fs_type == FsType::Fat32 && mode != JournalMode::Off {
        return Err(KernelError::NotSupported);
    }
    p.journal_mode = mode;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set commit interval (1-600 seconds).
pub fn set_commit_interval(profile_id: u64, secs: u32) -> KernelResult<()> {
    if secs == 0 || secs > 600 {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.commit_interval_secs = secs;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set reserved blocks percentage (0-50).
pub fn set_reserved_pct(profile_id: u64, pct: u32) -> KernelResult<()> {
    if pct > 50 {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.reserved_pct = pct;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set inode ratio (bytes per inode: 1024..=1048576).
pub fn set_inode_ratio(profile_id: u64, ratio: u32) -> KernelResult<()> {
    if ratio < 1024 || ratio > 1_048_576 {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.inode_ratio = ratio;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set block allocation strategy.
pub fn set_alloc_strategy(profile_id: u64, strategy: AllocStrategy) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.alloc_strategy = strategy;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set discard/TRIM enablement.
pub fn set_discard(profile_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.discard = enabled;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set data checksumming (btrfs/f2fs).
pub fn set_data_checksum(profile_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    if enabled && p.fs_type != FsType::Btrfs && p.fs_type != FsType::F2fs {
        return Err(KernelError::NotSupported);
    }
    p.data_checksum = enabled;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set compression.
pub fn set_compression(profile_id: u64, enabled: bool, algo: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    if enabled && p.fs_type != FsType::Btrfs && p.fs_type != FsType::F2fs {
        return Err(KernelError::NotSupported);
    }
    p.compression = enabled;
    p.compression_algo = if enabled { String::from(algo) } else { String::new() };
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set dir_index enablement.
pub fn set_dir_index(profile_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.dir_index = enabled;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set lazy init.
pub fn set_lazy_init(profile_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.lazy_init = enabled;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Set stripe width for RAID (0 = not RAID).
pub fn set_stripe_width(profile_id: u64, width: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.stripe_width = width;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

/// Mark profile as applied to a live filesystem.
pub fn mark_applied(profile_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.applied = true;
    p.modified_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tradeoff information (design.txt line 1279)
// ---------------------------------------------------------------------------

/// Get tradeoff descriptions for all tunable parameters.
pub fn tradeoffs() -> Vec<TradeoffInfo> {
    STATE.lock().tradeoffs.clone()
}

fn add_tradeoff(state: &mut State, param: &str, value: &str, adv: &[&str], disadv: &[&str]) {
    state.tradeoffs.push(TradeoffInfo {
        param_name: String::from(param),
        value_desc: String::from(value),
        advantages: adv.iter().map(|s| String::from(*s)).collect(),
        disadvantages: disadv.iter().map(|s| String::from(*s)).collect(),
    });
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise default profiles and tradeoff information.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.profiles.is_empty() {
        return;
    }

    // Create one default profile per workload type (ext4).
    let workloads = [
        (WorkloadType::Desktop, "ext4-desktop"),
        (WorkloadType::Database, "ext4-database"),
        (WorkloadType::Server, "ext4-server"),
        (WorkloadType::Development, "ext4-development"),
        (WorkloadType::Gaming, "ext4-gaming"),
    ];

    for (wl, name) in workloads {
        let mut p = defaults_for(FsType::Ext4, wl);
        p.name = String::from(name);
        state.profiles.push(p);
    }

    // Populate tradeoff information per design.txt line 1279.
    add_tradeoff(
        &mut state,
        "journal_mode",
        "ordered (default)",
        &["Good balance of safety and speed", "Metadata always consistent"],
        &["Data may be stale after crash if not fsynced"],
    );
    add_tradeoff(
        &mut state,
        "journal_mode",
        "journal (full data journaling)",
        &["Safest: both data and metadata journaled", "Best crash recovery"],
        &["~30-50% write throughput penalty", "Doubles write amplification"],
    );
    add_tradeoff(
        &mut state,
        "journal_mode",
        "writeback",
        &["Fastest writes", "Lowest CPU overhead"],
        &["Data corruption risk on crash", "Not recommended for databases"],
    );
    add_tradeoff(
        &mut state,
        "inode_ratio",
        "4096 (many inodes)",
        &["Handles millions of small files", "Good for source trees, node_modules"],
        &["More space used for inode table", "Wasted on large-file workloads"],
    );
    add_tradeoff(
        &mut state,
        "inode_ratio",
        "65536 (few inodes)",
        &["Less metadata overhead", "More space for data blocks"],
        &["May run out of inodes with many small files"],
    );
    add_tradeoff(
        &mut state,
        "alloc_strategy",
        "sequential",
        &["Best for large sequential reads/writes", "Reduces fragmentation for big files"],
        &["Poor for many small concurrent allocations"],
    );
    add_tradeoff(
        &mut state,
        "alloc_strategy",
        "pack",
        &["Efficient space usage", "Groups small files together"],
        &["May fragment large files"],
    );
    add_tradeoff(
        &mut state,
        "commit_interval",
        "1 second",
        &["Minimal data loss window", "Best for databases"],
        &["High write amplification", "SSD wear increase"],
    );
    add_tradeoff(
        &mut state,
        "commit_interval",
        "15 seconds",
        &["Better batching of writes", "Less SSD wear"],
        &["Larger data loss window on crash"],
    );
    add_tradeoff(
        &mut state,
        "discard",
        "enabled (TRIM)",
        &["SSD performance maintained over time", "Prevents write amplification"],
        &["Small overhead per delete/trim", "Not needed on HDDs"],
    );
    add_tradeoff(
        &mut state,
        "data_checksum",
        "enabled",
        &["Detects silent data corruption (bit rot)", "Essential for long-term storage"],
        &["CPU overhead on every read/write", "Only supported on btrfs/f2fs"],
    );

    state.changes += 1;
}

/// Return (profile_count, tradeoff_count, applied_count, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    let profiles = state.profiles.len();
    let tradeoffs = state.tradeoffs.len();
    let applied = state.profiles.iter().filter(|p| p.applied).count();
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (profiles, tradeoffs, applied, ops)
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

    // Test 1: create profiles.
    serial_println!("fstune::self_test 1: create profiles");
    let p1 = create_profile("test-desktop", FsType::Ext4, WorkloadType::Desktop)?;
    let p2 = create_profile("test-db", FsType::Ext4, WorkloadType::Database)?;
    assert_eq!(list_profiles().len(), 2);
    // Duplicate name rejected.
    assert!(create_profile("test-desktop", FsType::Btrfs, WorkloadType::Server).is_err());

    // Test 2: workload defaults are correct.
    serial_println!("fstune::self_test 2: workload defaults");
    let desktop = get_profile(p1)?;
    assert_eq!(desktop.inode_ratio, 16384);
    assert!(desktop.discard);
    let db = get_profile(p2)?;
    assert_eq!(db.inode_ratio, 65536);
    assert_eq!(db.commit_interval_secs, 1);

    // Test 3: set parameters.
    serial_println!("fstune::self_test 3: set parameters");
    set_block_size(p1, 8192)?;
    set_journal_mode(p1, JournalMode::Journal)?;
    set_commit_interval(p1, 10)?;
    set_reserved_pct(p1, 3)?;
    set_inode_ratio(p1, 8192)?;
    let updated = get_profile(p1)?;
    assert_eq!(updated.block_size, 8192);
    assert_eq!(updated.reserved_pct, 3);
    assert_eq!(updated.inode_ratio, 8192);

    // Test 4: parameter validation.
    serial_println!("fstune::self_test 4: validation");
    assert!(set_block_size(p1, 7000).is_err()); // not power of two
    assert!(set_block_size(p1, 256).is_err()); // too small
    assert!(set_commit_interval(p1, 0).is_err());
    assert!(set_commit_interval(p1, 601).is_err());
    assert!(set_reserved_pct(p1, 51).is_err());
    assert!(set_inode_ratio(p1, 512).is_err());

    // Test 5: fs-type constraints.
    serial_println!("fstune::self_test 5: fs-type constraints");
    let p3 = create_profile("test-fat", FsType::Fat32, WorkloadType::Desktop)?;
    assert!(set_journal_mode(p3, JournalMode::Journal).is_err()); // FAT32 has no journal
    assert!(set_data_checksum(p1, true).is_err()); // ext4 not supported

    // Test 6: apply workload preset.
    serial_println!("fstune::self_test 6: apply workload");
    apply_workload(p1, WorkloadType::Gaming)?;
    let gaming = get_profile(p1)?;
    assert_eq!(gaming.inode_ratio, 65536);
    assert_eq!(gaming.commit_interval_secs, 15);

    // Test 7: remove + defaults_for.
    serial_println!("fstune::self_test 7: remove and defaults_for");
    remove_profile(p3)?;
    assert_eq!(list_profiles().len(), 2);
    let btrfs = defaults_for(FsType::Btrfs, WorkloadType::Server);
    assert!(btrfs.data_checksum);
    assert!(btrfs.compression);

    clear_all();
    serial_println!("fstune::self_test: all 7 tests passed");
    Ok(())
}

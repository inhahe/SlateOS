//! Binary Format — executable format loader statistics.
//!
//! Tracks binary format detection, ELF/script/shebang loading,
//! load times, and format-specific errors. Essential for
//! diagnosing process execution issues.
//!
//! ## Architecture
//!
//! ```text
//! Binary format monitoring
//!   → binfmt::record_load(format, path, ns) → track successful load
//!   → binfmt::record_error(format, reason) → track load failure
//!   → binfmt::register_format(name) → register format handler
//!   → binfmt::format_stats() → per-format statistics
//!
//! Integration:
//!   → procstat (process stats)
//!   → taskstats (per-task accounting)
//!   → fscache (filesystem cache)
//!   → secpolicy (security policy)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Binary format type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinFormat {
    Elf64,
    Elf32,
    Script,
    FlatBin,
    Wasm,
    Unknown,
}

impl BinFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Elf64 => "elf64",
            Self::Elf32 => "elf32",
            Self::Script => "script",
            Self::FlatBin => "flat",
            Self::Wasm => "wasm",
            Self::Unknown => "unknown",
        }
    }
}

/// Load error reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadError {
    BadMagic,
    UnsupportedArch,
    CorruptedHeaders,
    MissingInterpreter,
    PermissionDenied,
    OutOfMemory,
}

impl LoadError {
    pub fn label(self) -> &'static str {
        match self {
            Self::BadMagic => "bad_magic",
            Self::UnsupportedArch => "unsupported_arch",
            Self::CorruptedHeaders => "corrupted",
            Self::MissingInterpreter => "no_interp",
            Self::PermissionDenied => "perm_denied",
            Self::OutOfMemory => "oom",
        }
    }
}

/// Per-format statistics.
#[derive(Debug, Clone)]
pub struct FormatStats {
    pub format: BinFormat,
    pub loads: u64,
    pub errors: u64,
    pub total_load_ns: u64,
    pub avg_load_ns: u64,
    pub max_load_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    formats: Vec<FormatStats>,
    error_counts: [u64; 6], // Indexed by LoadError ordinal.
    total_loads: u64,
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

fn error_index(e: LoadError) -> usize {
    match e {
        LoadError::BadMagic => 0,
        LoadError::UnsupportedArch => 1,
        LoadError::CorruptedHeaders => 2,
        LoadError::MissingInterpreter => 3,
        LoadError::PermissionDenied => 4,
        LoadError::OutOfMemory => 5,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the binary-format loader statistics state.
///
/// Starts with NO registered formats and zero load/error totals (the per-error
/// breakdown array is all zeros too). A format handler is added through
/// [`register_format`] when the kernel actually registers a binary-format
/// loader (the ELF64/ELF32 loader, the script/shebang handler, …), and its
/// per-format load/error/timing counters advance only through real
/// [`record_load`] / [`record_error`] calls on the `exec` path. The
/// `/proc/binfmt` generator and the `binfmt` kshell command surface the format
/// list (and [`format_stats`] / [`error_breakdown`] / [`stats`]) as if it
/// reflects the real loader activity, so seeding it with phantom formats and
/// load counts would be fabricated procfs data — it would claim hundreds of
/// thousands of executions that never happened.
///
/// (Previously this seeded three fictional formats — Elf64 (500,000 loads /
/// 1,000 errors / 500s total / 1ms avg / 50ms max), Script (100,000 / 5,000 /
/// 20s / 200µs / 10ms) and Elf32 (10,000 / 200 / 5s / 500µs / 20ms) — plus an
/// error breakdown of [3000, 500, 200, 1500, 800, 200] and global totals of
/// 610,000 loads / 6,200 errors.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        formats: Vec::new(),
        error_counts: [0; 6],
        total_loads: 0,
        total_errors: 0,
        ops: 0,
    });
}

/// Register a binary-format handler.
///
/// The real format loaders (ELF64/ELF32, script/shebang, flat binary, WASM)
/// call this when they install themselves so that subsequent [`record_load`] /
/// [`record_error`] calls have a per-format row to accumulate into. Returns
/// [`KernelError::AlreadyExists`] if the format is already registered.
pub fn register_format(bin_format: BinFormat) -> KernelResult<()> {
    with_state(|state| {
        if state.formats.iter().any(|f| f.format == bin_format) {
            return Err(KernelError::AlreadyExists);
        }
        state.formats.push(FormatStats {
            format: bin_format,
            loads: 0,
            errors: 0,
            total_load_ns: 0,
            avg_load_ns: 0,
            max_load_ns: 0,
        });
        Ok(())
    })
}

/// Record a successful binary load.
pub fn record_load(bin_format: BinFormat, load_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let f = state.formats.iter_mut().find(|f| f.format == bin_format)
            .ok_or(KernelError::NotFound)?;
        f.loads += 1;
        f.total_load_ns += load_ns;
        if load_ns > f.max_load_ns { f.max_load_ns = load_ns; }
        // Running average.
        if f.loads > 0 {
            f.avg_load_ns = f.total_load_ns / f.loads;
        }
        state.total_loads += 1;
        Ok(())
    })
}

/// Record a load error.
pub fn record_error(bin_format: BinFormat, error: LoadError) -> KernelResult<()> {
    with_state(|state| {
        if let Some(f) = state.formats.iter_mut().find(|f| f.format == bin_format) {
            f.errors += 1;
        }
        state.error_counts[error_index(error)] += 1;
        state.total_errors += 1;
        Ok(())
    })
}

/// Per-format statistics.
pub fn format_stats() -> Vec<FormatStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.formats.clone())
}

/// Error breakdown.
pub fn error_breakdown() -> [(LoadError, u64); 6] {
    let guard = STATE.lock();
    let counts = guard.as_ref().map_or([0u64; 6], |s| s.error_counts);
    [
        (LoadError::BadMagic, counts[0]),
        (LoadError::UnsupportedArch, counts[1]),
        (LoadError::CorruptedHeaders, counts[2]),
        (LoadError::MissingInterpreter, counts[3]),
        (LoadError::PermissionDenied, counts[4]),
        (LoadError::OutOfMemory, counts[5]),
    ]
}

/// Statistics: (format_count, total_loads, total_errors, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.formats.len(), s.total_loads, s.total_errors, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("binfmt::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and no
    // fixtures leak into the live format table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom formats, zeroed error breakdown and totals.
    assert_eq!(format_stats().len(), 0);
    let (f0, l0, e0, _) = stats();
    assert_eq!((f0, l0, e0), (0, 0, 0));
    for (_, count) in error_breakdown() {
        assert_eq!(count, 0);
    }
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register — Elf64 appears zeroed; a duplicate is AlreadyExists.
    register_format(BinFormat::Elf64).expect("register");
    assert!(register_format(BinFormat::Elf64).is_err());
    assert_eq!(format_stats().len(), 1);
    let f = format_stats().into_iter().find(|f| f.format == BinFormat::Elf64).expect("find");
    assert_eq!((f.loads, f.errors, f.total_load_ns, f.avg_load_ns, f.max_load_ns), (0, 0, 0, 0, 0));
    crate::serial_println!("  [2/8] register: OK");

    // 3: Record load — per-format load count, total time, and running average.
    record_load(BinFormat::Elf64, 400_000).expect("load");
    let f = format_stats().into_iter().find(|f| f.format == BinFormat::Elf64).expect("p3");
    assert_eq!((f.loads, f.total_load_ns, f.avg_load_ns), (1, 400_000, 400_000));
    assert_eq!(stats().1, 1); // total_loads
    crate::serial_println!("  [3/8] load + average: OK");

    // 4: Max + average — a second, larger load updates max and recomputes avg.
    record_load(BinFormat::Elf64, 100_000_000).expect("big_load");
    let f = format_stats().into_iter().find(|f| f.format == BinFormat::Elf64).expect("p4");
    assert_eq!(f.max_load_ns, 100_000_000);
    assert_eq!(f.avg_load_ns, (400_000 + 100_000_000) / 2);
    crate::serial_println!("  [4/8] max + average: OK");

    // 5: Record error — bumps the registered format's error count, the per-error
    //    breakdown slot, and the global error total.
    register_format(BinFormat::Script).expect("register2");
    record_error(BinFormat::Script, LoadError::MissingInterpreter).expect("error");
    let f = format_stats().into_iter().find(|f| f.format == BinFormat::Script).expect("p5");
    assert_eq!(f.errors, 1);
    assert_eq!(stats().2, 1); // total_errors
    crate::serial_println!("  [5/8] error: OK");

    // 6: Error breakdown — exactly one MissingInterpreter, all other slots zero.
    let breakdown = error_breakdown();
    assert_eq!(breakdown[3].1, 1); // MissingInterpreter
    assert_eq!(breakdown[0].1 + breakdown[1].1 + breakdown[2].1 + breakdown[4].1 + breakdown[5].1, 0);
    crate::serial_println!("  [6/8] breakdown: OK");

    // 7: Not found — loading an unregistered format errors.
    assert!(record_load(BinFormat::Unknown, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Final stats reflect only the real activity above: 2 formats, 2 loads,
    //    1 error.
    let (fmts, loads, errors, ops) = stats();
    assert_eq!((fmts, loads, errors), (2, 2, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("binfmt::self_test() — all 8 tests passed");
}

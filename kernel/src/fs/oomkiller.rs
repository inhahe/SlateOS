//! OOM Killer — Out-of-Memory killer policy and scoring.
//!
//! Manages OOM kill policies, per-process OOM scores, and
//! kill history. Allows score adjustment and process exemption.
//!
//! ## Architecture
//!
//! ```text
//! OOM killer
//!   → oomkiller::score(pid) → get OOM score
//!   → oomkiller::adjust(pid, adj) → adjust score
//!   → oomkiller::select_victim() → pick process to kill
//!   → oomkiller::kill_history() → past OOM kills
//!
//! Integration:
//!   → memdiag (memory diagnostics)
//!   → sysresource (resource monitoring)
//!   → perfmon (performance monitor)
//!   → kernlog (kernel logging)
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

/// OOM policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OomPolicy {
    Kill,         // Kill highest-scoring process.
    Panic,        // Kernel panic on OOM.
    Reap,         // Reap caches first, then kill.
    Disabled,     // Never kill.
}

impl OomPolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Kill => "Kill",
            Self::Panic => "Panic",
            Self::Reap => "Reap+Kill",
            Self::Disabled => "Disabled",
        }
    }
}

/// Per-process OOM score.
#[derive(Debug, Clone)]
pub struct OomScore {
    pub pid: u32,
    pub process_name: String,
    pub score: i32,          // 0..1000 (higher = more likely to kill).
    pub adj: i32,            // User adjustment (-1000..1000).
    pub memory_bytes: u64,
    pub exempt: bool,
}

/// OOM kill record.
#[derive(Debug, Clone)]
pub struct OomKillRecord {
    pub pid: u32,
    pub process_name: String,
    pub score: i32,
    pub memory_freed: u64,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SCORES: usize = 1024;
const MAX_HISTORY: usize = 100;

struct State {
    scores: Vec<OomScore>,
    history: Vec<OomKillRecord>,
    policy: OomPolicy,
    total_kills: u64,
    total_memory_freed: u64,
    total_invocations: u64,
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
    // Start with an empty score table. OOM scores describe *live* processes —
    // their pid, name, memory footprint, and kill likelihood. Seeding invented
    // processes (init/sshd/browser/game) here would surface fabricated victims
    // through /proc/oomkiller and the `oom` shell command as if they were real.
    // The Kill policy is the genuine default; everything else stays empty until
    // real processes register via register_process().
    //
    // DEFERRED PROPER FIX: wire register_process() to the real process table so
    // /proc/oomkiller reflects actual processes and their memory footprints.
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        scores: Vec::new(),
        history: Vec::new(),
        policy: OomPolicy::Kill,
        total_kills: 0,
        total_memory_freed: 0,
        total_invocations: 0,
        ops: 0,
    });
}

/// Get OOM score for a process.
pub fn get_score(pid: u32) -> Option<OomScore> {
    STATE.lock().as_ref().and_then(|s| s.scores.iter().find(|p| p.pid == pid).cloned())
}

/// List all scores, sorted by effective score descending.
pub fn list_scores() -> Vec<OomScore> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut scores = s.scores.clone();
        scores.sort_by(|a, b| {
            let ea = (a.score + a.adj).max(0);
            let eb = (b.score + b.adj).max(0);
            eb.cmp(&ea)
        });
        scores
    })
}

/// Adjust OOM score for a process.
pub fn adjust_score(pid: u32, adj: i32) -> KernelResult<()> {
    with_state(|state| {
        if adj < -1000 || adj > 1000 {
            return Err(KernelError::InvalidArgument);
        }
        let score = state.scores.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        score.adj = adj;
        Ok(())
    })
}

/// Set a process as exempt from OOM killing.
pub fn set_exempt(pid: u32, exempt: bool) -> KernelResult<()> {
    with_state(|state| {
        let score = state.scores.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        score.exempt = exempt;
        if exempt { score.adj = -1000; }
        Ok(())
    })
}

/// Register a process for OOM scoring.
pub fn register_process(pid: u32, name: &str, memory: u64) -> KernelResult<()> {
    with_state(|state| {
        if state.scores.len() >= MAX_SCORES { return Err(KernelError::ResourceExhausted); }
        if state.scores.iter().any(|p| p.pid == pid) { return Err(KernelError::AlreadyExists); }
        // Base score proportional to memory (crude: memory_bytes / 1024, capped at 1000).
        let base = ((memory / 1024) as i32).min(1000);
        state.scores.push(OomScore {
            pid, process_name: String::from(name), score: base,
            adj: 0, memory_bytes: memory, exempt: false,
        });
        Ok(())
    })
}

/// Select a victim (highest effective score, non-exempt).
pub fn select_victim() -> KernelResult<OomScore> {
    with_state(|state| {
        state.total_invocations += 1;
        if state.policy == OomPolicy::Disabled {
            return Err(KernelError::PermissionDenied);
        }
        state.scores.iter()
            .filter(|p| !p.exempt)
            .max_by_key(|p| (p.score + p.adj).max(0))
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// Simulate killing a process (removes it and records).
pub fn kill(pid: u32) -> KernelResult<u64> {
    with_state(|state| {
        let idx = state.scores.iter().position(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let victim = state.scores.remove(idx);
        let now = crate::hpet::elapsed_ns();
        if state.history.len() >= MAX_HISTORY { state.history.remove(0); }
        state.history.push(OomKillRecord {
            pid: victim.pid, process_name: victim.process_name,
            score: victim.score + victim.adj,
            memory_freed: victim.memory_bytes, timestamp_ns: now,
        });
        state.total_kills += 1;
        state.total_memory_freed += victim.memory_bytes;
        Ok(victim.memory_bytes)
    })
}

/// Get kill history.
pub fn kill_history() -> Vec<OomKillRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.history.clone())
}

/// Set OOM policy.
pub fn set_policy(policy: OomPolicy) -> KernelResult<()> {
    with_state(|state| { state.policy = policy; Ok(()) })
}

/// Get OOM policy.
pub fn get_policy() -> OomPolicy {
    STATE.lock().as_ref().map_or(OomPolicy::Kill, |s| s.policy)
}

/// Statistics: (process_count, total_kills, total_freed, total_invocations, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.scores.len(), s.total_kills, s.total_memory_freed, s.total_invocations, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("oomkiller::self_test() — running tests...");

    // Residue-free: start from a clean, controlled State so assertions hold
    // regardless of prior kshell/procfs activity.
    *STATE.lock() = None;
    init_defaults();

    // 1: Defaults — empty score table, genuine Kill policy.
    assert_eq!(list_scores().len(), 0);
    assert_eq!(get_policy(), OomPolicy::Kill);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register processes via the real API. Base score is derived from the
    //    supplied memory footprint (memory / 1024, capped at 1000).
    register_process(1, "init", 4096).expect("reg init");      // base 4
    register_process(100, "sshd", 16384).expect("reg sshd");   // base 16
    register_process(200, "browser", 524288).expect("reg br"); // base 512
    register_process(300, "game", 1048576).expect("reg game"); // base 1000 (capped)
    set_exempt(1, true).expect("exempt init");
    assert_eq!(list_scores().len(), 4);
    let s = get_score(200).expect("get");
    assert_eq!(s.process_name, "browser");
    assert_eq!(s.score, 512);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Select victim (highest effective score, non-exempt).
    let victim = select_victim().expect("victim");
    assert_eq!(victim.pid, 300); // game has highest effective score (1000).
    crate::serial_println!("  [3/8] victim: OK");

    // 4: Adjust score.
    adjust_score(200, 500).expect("adj");
    let s = get_score(200).expect("get2");
    assert_eq!(s.adj, 500);
    assert!(adjust_score(200, 9999).is_err());
    crate::serial_println!("  [4/8] adjust: OK");

    // 5: Exempt.
    set_exempt(200, true).expect("exempt");
    let s = get_score(200).expect("get3");
    assert!(s.exempt);
    crate::serial_println!("  [5/8] exempt: OK");

    // 6: Kill.
    let freed = kill(300).expect("kill");
    assert_eq!(freed, 1048576);
    assert_eq!(list_scores().len(), 3);
    crate::serial_println!("  [6/8] kill: OK");

    // 7: History.
    let hist = kill_history();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].pid, 300);
    crate::serial_println!("  [7/8] history: OK");

    // 8: Stats.
    let (count, kills, freed_total, invocations, ops) = stats();
    assert_eq!(count, 3);
    assert_eq!(kills, 1);
    assert_eq!(freed_total, 1048576);
    assert!(invocations >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue for later callers / boot-time tests.
    *STATE.lock() = None;

    crate::serial_println!("oomkiller::self_test() — all 8 tests passed");
}

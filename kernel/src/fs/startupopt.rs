//! Startup Optimization — boot profiling and optimization.
//!
//! Measures boot stage timings, identifies slow services, and provides
//! optimization suggestions to reduce startup time.
//!
//! ## Architecture
//!
//! ```text
//! Boot process
//!   → startupopt::begin_stage(name) → mark stage start
//!   → startupopt::end_stage(name) → mark stage end
//!   → startupopt::analyze() → optimization suggestions
//!
//! Integration:
//!   → autostart (startup programs)
//!   → servicemgr (service startup order)
//!   → prefetch (prefetch optimization)
//!   → bootcfg (boot configuration)
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

/// Boot stage category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageCategory {
    /// Firmware/UEFI initialization.
    Firmware,
    /// Bootloader.
    Bootloader,
    /// Kernel early init.
    KernelEarly,
    /// Driver initialization.
    Drivers,
    /// Service startup.
    Services,
    /// User session setup.
    UserSession,
    /// Desktop loading.
    Desktop,
    /// Auto-start programs.
    AutoStart,
}

impl StageCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Firmware => "Firmware",
            Self::Bootloader => "Bootloader",
            Self::KernelEarly => "Kernel Early",
            Self::Drivers => "Drivers",
            Self::Services => "Services",
            Self::UserSession => "User Session",
            Self::Desktop => "Desktop",
            Self::AutoStart => "Auto Start",
        }
    }
}

/// Optimization suggestion priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl SuggestionPriority {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Critical => "Critical",
        }
    }
}

/// A boot stage timing record.
#[derive(Debug, Clone)]
pub struct BootStage {
    pub name: String,
    pub category: StageCategory,
    pub start_ns: u64,
    pub end_ns: u64,
    pub duration_ms: u64,
    pub completed: bool,
}

/// An optimization suggestion.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub id: u32,
    pub priority: SuggestionPriority,
    pub description: String,
    pub estimated_savings_ms: u64,
    pub applied: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_STAGES: usize = 200;
const MAX_SUGGESTIONS: usize = 50;

struct State {
    stages: Vec<BootStage>,
    suggestions: Vec<Suggestion>,
    next_suggestion_id: u32,
    boot_count: u64,
    last_boot_ms: u64,
    fastest_boot_ms: u64,
    total_analyses: u64,
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
        stages: Vec::new(),
        suggestions: Vec::new(),
        next_suggestion_id: 1,
        boot_count: 0,
        last_boot_ms: 0,
        fastest_boot_ms: u64::MAX,
        total_analyses: 0,
        ops: 0,
    });
}

/// Begin a boot stage.
pub fn begin_stage(name: &str, category: StageCategory) -> KernelResult<()> {
    with_state(|state| {
        if state.stages.iter().any(|s| s.name == name && !s.completed) {
            return Err(KernelError::AlreadyExists);
        }
        if state.stages.len() >= MAX_STAGES {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        state.stages.push(BootStage {
            name: String::from(name),
            category,
            start_ns: now,
            end_ns: 0,
            duration_ms: 0,
            completed: false,
        });
        Ok(())
    })
}

/// End a boot stage.
pub fn end_stage(name: &str) -> KernelResult<u64> {
    with_state(|state| {
        let stage = state.stages.iter_mut().rev().find(|s| s.name == name && !s.completed)
            .ok_or(KernelError::NotFound)?;
        let now = crate::hpet::elapsed_ns();
        stage.end_ns = now;
        stage.duration_ms = (now.saturating_sub(stage.start_ns)) / 1_000_000;
        stage.completed = true;
        Ok(stage.duration_ms)
    })
}

/// Record a complete boot (call after all stages done).
pub fn record_boot() -> KernelResult<u64> {
    with_state(|state| {
        let total_ms: u64 = state.stages.iter()
            .filter(|s| s.completed)
            .map(|s| s.duration_ms)
            .sum();
        state.boot_count += 1;
        state.last_boot_ms = total_ms;
        if total_ms < state.fastest_boot_ms {
            state.fastest_boot_ms = total_ms;
        }
        Ok(total_ms)
    })
}

/// Analyze boot stages and generate optimization suggestions.
pub fn analyze() -> KernelResult<usize> {
    with_state(|state| {
        state.suggestions.clear();
        state.total_analyses += 1;
        let mut suggestion_count = 0u32;

        // Find slow stages (> 2000ms).
        for stage in &state.stages {
            if !stage.completed { continue; }
            if stage.duration_ms > 2000 {
                let id = state.next_suggestion_id;
                state.next_suggestion_id += 1;
                state.suggestions.push(Suggestion {
                    id,
                    priority: if stage.duration_ms > 5000 {
                        SuggestionPriority::Critical
                    } else {
                        SuggestionPriority::High
                    },
                    description: format!("Slow stage '{}' took {}ms — consider deferring or parallelizing",
                        stage.name, stage.duration_ms),
                    estimated_savings_ms: stage.duration_ms / 2,
                    applied: false,
                });
                suggestion_count += 1;
            }
        }

        // Check for too many autostart items.
        let autostart_count = state.stages.iter()
            .filter(|s| matches!(s.category, StageCategory::AutoStart))
            .count();
        if autostart_count > 5 {
            let id = state.next_suggestion_id;
            state.next_suggestion_id += 1;
            state.suggestions.push(Suggestion {
                id,
                priority: SuggestionPriority::Medium,
                description: format!("Too many auto-start items ({}). Consider deferring some.", autostart_count),
                estimated_savings_ms: (autostart_count as u64).saturating_sub(5) * 500,
                applied: false,
            });
            suggestion_count += 1;
        }

        // Check sequential stages that could be parallel.
        let service_stages: Vec<&BootStage> = state.stages.iter()
            .filter(|s| matches!(s.category, StageCategory::Services) && s.completed)
            .collect();
        if service_stages.len() > 3 {
            let total_service_ms: u64 = service_stages.iter().map(|s| s.duration_ms).sum();
            if total_service_ms > 3000 {
                let id = state.next_suggestion_id;
                state.next_suggestion_id += 1;
                state.suggestions.push(Suggestion {
                    id,
                    priority: SuggestionPriority::High,
                    description: format!("Service startup took {}ms across {} services — enable parallel startup",
                        total_service_ms, service_stages.len()),
                    estimated_savings_ms: total_service_ms / 3,
                    applied: false,
                });
                suggestion_count += 1;
            }
        }

        Ok(suggestion_count as usize)
    })
}

/// Mark a suggestion as applied.
pub fn apply_suggestion(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let s = state.suggestions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        s.applied = true;
        Ok(())
    })
}

/// Clear stages for a new boot.
pub fn clear_stages() -> KernelResult<()> {
    with_state(|state| {
        state.stages.clear();
        Ok(())
    })
}

/// Get stages sorted by duration (slowest first).
pub fn get_stages_by_duration() -> Vec<BootStage> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut stages: Vec<BootStage> = s.stages.iter()
            .filter(|s| s.completed)
            .cloned()
            .collect();
        stages.sort_by_key(|e| core::cmp::Reverse(e.duration_ms));
        stages
    })
}

/// Get stages by category.
pub fn get_stages_by_category(cat: StageCategory) -> Vec<BootStage> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.stages.iter()
            .filter(|s| s.category == cat && s.completed)
            .cloned()
            .collect()
    })
}

/// Get current suggestions.
pub fn get_suggestions() -> Vec<Suggestion> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.suggestions.clone())
}

/// Statistics: (stage_count, boot_count, last_boot_ms, fastest_boot_ms, analyses, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let fastest = if s.fastest_boot_ms == u64::MAX { 0 } else { s.fastest_boot_ms };
            (s.stages.len(), s.boot_count, s.last_boot_ms, fastest, s.total_analyses, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("startupopt::self_test() — running tests...");
    // Start from a clean, freshly-defaulted state so the assertions below are
    // exact and the boot-stage / suggestion / boot-count fixtures this test
    // creates do not leak into the live /proc/startupopt table afterward (the
    // kshell `startupopt test` subcommand calls this directly, and
    // /proc/startupopt reports boot_count / total_analyses — leaked fixtures
    // would look like a real boot profile).
    *STATE.lock() = None;
    init_defaults();

    // 1: No stages initially, zeroed counters — init_defaults seeds NO
    //    fabricated boot profile (stages/suggestions empty, all counters 0).
    assert_eq!(get_stages_by_duration().len(), 0);
    let (st0, b0, lm0, fm0, an0, _) = stats();
    assert_eq!((st0, b0, lm0, fm0, an0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Begin/end stages — two completed stages recorded.
    begin_stage("kernel_init", StageCategory::KernelEarly).expect("begin1");
    end_stage("kernel_init").expect("end1");
    begin_stage("drivers", StageCategory::Drivers).expect("begin2");
    end_stage("drivers").expect("end2");
    assert_eq!(get_stages_by_duration().len(), 2);
    crate::serial_println!("  [2/8] stages: OK");

    // 3: Can't begin a duplicate active stage (AlreadyExists until it ends).
    begin_stage("test_stage", StageCategory::Services).expect("begin3");
    assert!(begin_stage("test_stage", StageCategory::Services).is_err());
    end_stage("test_stage").expect("end3");
    crate::serial_println!("  [3/8] no duplicates: OK");

    // 4: Record boot — returns the summed duration of all 3 completed stages
    //    (timing-dependent, so just verify it succeeds) and bumps boot_count.
    record_boot().expect("boot");
    crate::serial_println!("  [4/8] record boot: OK");

    // 5: Stages by category — exactly one KernelEarly stage (kernel_init).
    let kernel_stages = get_stages_by_category(StageCategory::KernelEarly);
    assert_eq!(kernel_stages.len(), 1);
    assert_eq!(kernel_stages[0].name, "kernel_init");
    crate::serial_println!("  [5/8] by category: OK");

    // 6: Analyze — the three test stages are sub-second, there are no AutoStart
    //    stages and only one Services stage, so NONE of the slow-stage / too-many-
    //    autostart / too-many-services heuristics fire: exactly 0 suggestions.
    assert_eq!(analyze().expect("analyze"), 0);
    assert_eq!(get_suggestions().len(), 0);
    crate::serial_println!("  [6/8] analyze: OK");

    // 7: Clear stages.
    clear_stages().expect("clear");
    assert_eq!(get_stages_by_duration().len(), 0);
    crate::serial_println!("  [7/8] clear: OK");

    // 8: Stats — stages cleared, exactly 1 boot recorded, 1 analysis run.
    let (stages, boots, _last_ms, _fastest_ms, analyses, ops) = stats();
    assert_eq!((stages, boots, analyses), (0, 1, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Restore the clean default state so no test fixtures (boot count, analyses,
    // any stages/suggestions) leak into the live module.
    *STATE.lock() = None;
    init_defaults();
    crate::serial_println!("startupopt::self_test() — all 8 tests passed");
}

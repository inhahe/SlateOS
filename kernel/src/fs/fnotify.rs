//! File Notification Statistics — inotify/fanotify monitoring.
//!
//! Tracks file notification watches, events generated, queue
//! depths, and overflow conditions. Essential for understanding
//! filesystem event monitoring overhead.
//!
//! ## Architecture
//!
//! ```text
//! File notification monitoring
//!   → fnotify::add_watch(type) → add inotify/fanotify watch
//!   → fnotify::remove_watch(type) → remove watch
//!   → fnotify::record_event(type, kind) → event generated
//!   → fnotify::per_type() → per-type stats
//!
//! Integration:
//!   → changetrack (change tracking)
//!   → fswatch (filesystem watch)
//!   → fdtable (file descriptor table)
//!   → inodestat (inode stats)
//! ```

#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Notification type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyType {
    Inotify,
    Fanotify,
    Dnotify,
}

impl NotifyType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Inotify => "inotify",
            Self::Fanotify => "fanotify",
            Self::Dnotify => "dnotify",
        }
    }
}

/// Event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Create,
    Delete,
    Modify,
    Access,
    Attrib,
    MovedFrom,
    MovedTo,
    Open,
    Close,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Delete => "delete",
            Self::Modify => "modify",
            Self::Access => "access",
            Self::Attrib => "attrib",
            Self::MovedFrom => "move_from",
            Self::MovedTo => "move_to",
            Self::Open => "open",
            Self::Close => "close",
        }
    }
}

/// Per-type notification stats.
#[derive(Debug, Clone)]
pub struct TypeStats {
    pub notify_type: NotifyType,
    pub watches: u64,
    pub max_watches: u64,
    pub events: u64,
    pub overflows: u64,
    pub queue_depth: u32,
    pub max_queue_depth: u32,
    pub event_counts: [u64; 9], // Indexed by EventKind
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    types: [TypeStats; 3],
    total_events: u64,
    total_overflows: u64,
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

fn type_index(t: NotifyType) -> usize {
    match t {
        NotifyType::Inotify => 0,
        NotifyType::Fanotify => 1,
        NotifyType::Dnotify => 2,
    }
}

fn event_index(k: EventKind) -> usize {
    match k {
        EventKind::Create => 0,
        EventKind::Delete => 1,
        EventKind::Modify => 2,
        EventKind::Access => 3,
        EventKind::Attrib => 4,
        EventKind::MovedFrom => 5,
        EventKind::MovedTo => 6,
        EventKind::Open => 7,
        EventKind::Close => 8,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the file-notification statistics.
///
/// Seeds the real three-type taxonomy (inotify / fanotify / dnotify) and their
/// CAPACITY limits (`max_watches` and `max_queue_depth` — sysctl-style
/// configuration, the analogue of `/proc/sys/fs/inotify/max_user_watches`) with
/// ALL ACTIVITY COUNTERS ZEROED: no live watches, no events, no overflows, empty
/// queues and zeroed per-event-kind breakdowns. The `/proc/fnotify` generator
/// and the `fnotify` kshell command surface these counters AS IF REAL, so
/// seeding observed watch/event/overflow totals would be fabricated procfs data
/// — it would claim millions of filesystem-notification events had been
/// delivered when the inotify/fanotify/dnotify subsystems are not even
/// implemented yet. Real watches and events are recorded through [`add_watch`]
/// and [`record_event`] when a notification backend exists.
///
/// (Previously this seeded fabricated activity — inotify 500 watches /
/// 10,000,000 events / 5 overflows, fanotify 50 watches / 5,000,000 events,
/// dnotify 10 watches / 100,000 events, totalling 15,100,000 phantom events with
/// invented per-event-kind breakdowns — none backed by a real notification
/// subsystem.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        types: [
            TypeStats {
                notify_type: NotifyType::Inotify, watches: 0, max_watches: 8192,
                events: 0, overflows: 0, queue_depth: 0, max_queue_depth: 4096,
                event_counts: [0; 9],
            },
            TypeStats {
                notify_type: NotifyType::Fanotify, watches: 0, max_watches: 1024,
                events: 0, overflows: 0, queue_depth: 0, max_queue_depth: 2048,
                event_counts: [0; 9],
            },
            TypeStats {
                notify_type: NotifyType::Dnotify, watches: 0, max_watches: 256,
                events: 0, overflows: 0, queue_depth: 0, max_queue_depth: 512,
                event_counts: [0; 9],
            },
        ],
        total_events: 0,
        total_overflows: 0,
        ops: 0,
    });
}

/// Add a watch.
pub fn add_watch(notify_type: NotifyType) -> KernelResult<()> {
    with_state(|state| {
        let t = &mut state.types[type_index(notify_type)];
        if t.watches >= t.max_watches { return Err(KernelError::ResourceExhausted); }
        t.watches += 1;
        Ok(())
    })
}

/// Remove a watch.
pub fn remove_watch(notify_type: NotifyType) -> KernelResult<()> {
    with_state(|state| {
        let t = &mut state.types[type_index(notify_type)];
        if t.watches == 0 { return Err(KernelError::NotFound); }
        t.watches -= 1;
        Ok(())
    })
}

/// Record an event.
pub fn record_event(notify_type: NotifyType, kind: EventKind) -> KernelResult<()> {
    with_state(|state| {
        let t = &mut state.types[type_index(notify_type)];
        t.events += 1;
        t.event_counts[event_index(kind)] += 1;
        t.queue_depth += 1;
        if t.queue_depth > t.max_queue_depth {
            t.queue_depth = t.max_queue_depth;
            t.overflows += 1;
            state.total_overflows += 1;
        }
        state.total_events += 1;
        Ok(())
    })
}

/// Drain events from queue.
pub fn drain_events(notify_type: NotifyType, count: u32) -> KernelResult<()> {
    with_state(|state| {
        let t = &mut state.types[type_index(notify_type)];
        t.queue_depth = t.queue_depth.saturating_sub(count);
        Ok(())
    })
}

/// Per-type stats.
pub fn per_type() -> [TypeStats; 3] {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.types.clone(),
        None => [
            TypeStats { notify_type: NotifyType::Inotify, watches: 0, max_watches: 0, events: 0, overflows: 0, queue_depth: 0, max_queue_depth: 0, event_counts: [0; 9] },
            TypeStats { notify_type: NotifyType::Fanotify, watches: 0, max_watches: 0, events: 0, overflows: 0, queue_depth: 0, max_queue_depth: 0, event_counts: [0; 9] },
            TypeStats { notify_type: NotifyType::Dnotify, watches: 0, max_watches: 0, events: 0, overflows: 0, queue_depth: 0, max_queue_depth: 0, event_counts: [0; 9] },
        ],
    }
}

/// Statistics: (total_watches, total_events, total_overflows, ops).
pub fn stats() -> (u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let watches: u64 = s.types.iter().map(|t| t.watches).sum();
            (watches, s.total_events, s.total_overflows, s.ops)
        }
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("fnotify::self_test() — running tests...");
    // Start from a clean, freshly-defaulted state so the assertions below are
    // exact and the watch/event fixtures this test creates do not leak into the
    // live /proc/fnotify table afterward (the kshell `fnotify test` subcommand
    // calls this directly, and /proc/fnotify reports watch/event/overflow totals
    // — leaked fixtures would look like real notification activity).
    *STATE.lock() = None;
    init_defaults();

    // 1: Defaults — the three-type taxonomy with CAPACITY limits kept but ALL
    //    activity ZEROED (no fabricated watches/events/overflows).
    let types = per_type();
    assert_eq!((types[0].watches, types[0].events, types[0].max_watches), (0, 0, 8192));
    assert_eq!((types[1].watches, types[1].max_watches), (0, 1024));
    assert_eq!((types[2].watches, types[2].max_watches), (0, 256));
    let (w0, e0, o0, _) = stats();
    assert_eq!((w0, e0, o0), (0, 0, 0)); // watches/events/overflows all 0
    crate::serial_println!("  [1/8] zeroed defaults: OK");

    // 2: Add watches — three inotify watches.
    for _ in 0..3 { add_watch(NotifyType::Inotify).expect("add"); }
    assert_eq!(per_type()[0].watches, 3);
    crate::serial_println!("  [2/8] add watch: OK");

    // 3: Remove watch — back down to two.
    remove_watch(NotifyType::Inotify).expect("remove");
    assert_eq!(per_type()[0].watches, 2);
    crate::serial_println!("  [3/8] remove watch: OK");

    // 4: Record event — first inotify Create event.
    record_event(NotifyType::Inotify, EventKind::Create).expect("event");
    let types = per_type();
    assert_eq!(types[0].events, 1);
    assert_eq!(types[0].queue_depth, 1);
    crate::serial_println!("  [4/8] record event: OK");

    // 5: Event counts — two Modify events accumulate at index 2; Create stays 1.
    record_event(NotifyType::Inotify, EventKind::Modify).expect("ev2");
    record_event(NotifyType::Inotify, EventKind::Modify).expect("ev3");
    let types = per_type();
    assert_eq!(types[0].event_counts[0], 1); // Create
    assert_eq!(types[0].event_counts[2], 2); // Modify
    assert_eq!(types[0].events, 3);
    crate::serial_println!("  [5/8] event counts: OK");

    // 6: Drain — three queued events minus two drained leaves one.
    drain_events(NotifyType::Inotify, 2).expect("drain");
    assert_eq!(per_type()[0].queue_depth, 1);
    crate::serial_println!("  [6/8] drain: OK");

    // 7: Empty remove fails — dnotify has no watches.
    assert!(remove_watch(NotifyType::Dnotify).is_err());
    crate::serial_println!("  [7/8] empty remove: OK");

    // 8: Stats — 2 live watches, 3 events recorded, no overflows.
    let (watches, events, overflows, ops) = stats();
    assert_eq!((watches, events, overflows), (2, 3, 0));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Restore the clean zeroed baseline so no test fixtures leak into the live
    // module's /proc/fnotify table.
    *STATE.lock() = None;
    init_defaults();
    crate::serial_println!("fnotify::self_test() — all 8 tests passed");
}

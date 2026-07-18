//! Kernel event bus — publish/subscribe for cross-subsystem notifications.
//!
//! Provides a lightweight, type-safe event notification system that allows
//! kernel subsystems to communicate without direct coupling.  Publishers
//! emit events; subscribers receive them asynchronously.
//!
//! ## Problem Solved
//!
//! Kernel subsystems often need to react to events in other subsystems:
//! - Thermal module → cpufreq: "temperature critical, throttle NOW"
//! - Memory pressure → kswapd: "free memory low, start reclaiming"
//! - CPU hotplug → scheduler: "CPU going offline, migrate tasks"
//! - Block device → filesystem: "disk I/O error detected"
//! - Network → firewall: "interface state changed"
//!
//! Without an event bus, each pair requires explicit function calls,
//! creating a web of cross-module dependencies.  The event bus decouples
//! publishers from subscribers.
//!
//! ## Design
//!
//! - **Events are typed** (enum variants) so subscribers can filter.
//! - **Delivery is synchronous** (in the publisher's context) for
//!   simplicity.  Callbacks must be fast (no sleeping, no heavy work).
//! - **Fixed-capacity subscriber lists** per event type (no heap on the
//!   delivery path).
//! - **Priority ordering**: subscribers can specify a priority; higher
//!   priority callbacks are invoked first.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::kevent::{self, Event, subscribe};
//!
//! // Subscriber:
//! fn on_thermal_critical(event: &Event) {
//!     if let Event::ThermalCritical { cpu, temp_c } = event {
//!         // Throttle this CPU...
//!     }
//! }
//! subscribe(EventKind::ThermalCritical, on_thermal_critical, 100);
//!
//! // Publisher:
//! kevent::publish(Event::ThermalCritical { cpu: 0, temp_c: 95 });
//! ```
//!
//! ## Thread Safety
//!
//! The subscriber list is protected by a spinlock (brief hold time).
//! Callbacks are invoked after releasing the lock to prevent deadlocks.
//! Callbacks must not re-enter the event bus (no publishing from a
//! subscriber callback).
//!
//! ## References
//!
//! - Linux `kernel/notifier.c` — blocking/atomic notifier chains
//! - Linux `include/linux/notifier.h` — notifier_call_chain()
//! - Windows `IoRegisterPlugPlayNotification` — PnP event callbacks

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::Mutex;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// Categories of kernel events.
///
/// Each variant represents a class of events that can be subscribed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EventKind {
    /// Memory pressure changed (low/moderate/high/critical).
    MemoryPressure = 0,
    /// Thermal threshold crossed.
    ThermalCritical = 1,
    /// CPU going online/offline.
    CpuHotplug = 2,
    /// Block device error.
    BlockDeviceError = 3,
    /// Network interface state change.
    NetIfaceChange = 4,
    /// OOM event (kill decision needed or completed).
    OomEvent = 5,
    /// Power state change (suspend/resume preparation).
    PowerStateChange = 6,
    /// Filesystem mount/unmount.
    FsMount = 7,
    /// Panic imminent (last chance to do cleanup).
    PanicImminent = 8,
    /// Timer tick (for low-priority periodic subscribers).
    PeriodicTick = 9,
    /// Generic user-defined event.
    Custom = 10,
}

/// Number of event kinds.
const EVENT_KIND_COUNT: usize = 11;

/// A kernel event with associated data.
#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// Memory pressure level changed.
    MemoryPressure { level: u8, score: u8 },
    /// CPU temperature exceeded threshold.
    ThermalCritical { cpu: usize, temp_c: u8 },
    /// CPU hotplug state change.
    CpuHotplug { cpu: usize, going_online: bool },
    /// Block device encountered an error.
    BlockDeviceError { device_id: u16, error_code: u16 },
    /// Network interface state changed.
    NetIfaceChange { iface_id: u16, is_up: bool },
    /// OOM event occurred.
    Oom { victim_pid: u32, freed_pages: u32 },
    /// Power state changing.
    PowerStateChange { entering_sleep: bool, sleep_state: u8 },
    /// Filesystem mounted or unmounted.
    FsMount { mounted: bool, device_id: u16 },
    /// Panic is about to happen (best-effort notification).
    PanicImminent,
    /// Periodic tick (every N ticks as configured).
    PeriodicTick { tick_count: u64 },
    /// Custom event with opaque data.
    Custom { code: u32, data: u64 },
}

impl Event {
    /// Get the kind of this event (for routing).
    #[must_use]
    pub fn kind(&self) -> EventKind {
        match self {
            Self::MemoryPressure { .. } => EventKind::MemoryPressure,
            Self::ThermalCritical { .. } => EventKind::ThermalCritical,
            Self::CpuHotplug { .. } => EventKind::CpuHotplug,
            Self::BlockDeviceError { .. } => EventKind::BlockDeviceError,
            Self::NetIfaceChange { .. } => EventKind::NetIfaceChange,
            Self::Oom { .. } => EventKind::OomEvent,
            Self::PowerStateChange { .. } => EventKind::PowerStateChange,
            Self::FsMount { .. } => EventKind::FsMount,
            Self::PanicImminent => EventKind::PanicImminent,
            Self::PeriodicTick { .. } => EventKind::PeriodicTick,
            Self::Custom { .. } => EventKind::Custom,
        }
    }
}

// ---------------------------------------------------------------------------
// Subscriber management
// ---------------------------------------------------------------------------

/// Maximum subscribers per event kind.
const MAX_SUBSCRIBERS_PER_KIND: usize = 8;

/// Subscriber callback type.
///
/// Receives the event by reference.  Must be fast and non-blocking.
/// Must not call `publish()` (re-entrancy is not supported).
pub type SubscriberFn = fn(&Event);

/// A registered subscriber.
#[derive(Clone, Copy)]
struct Subscriber {
    /// Callback function.
    func: SubscriberFn,
    /// Priority (higher = called first, 0-255).
    priority: u8,
    /// Whether this slot is active.
    active: bool,
}

impl Subscriber {
    const fn empty() -> Self {
        // Dummy function for empty slots.
        Self {
            func: dummy_handler,
            priority: 0,
            active: false,
        }
    }
}

fn dummy_handler(_: &Event) {}

/// Per-event-kind subscriber list.
struct SubscriberList {
    subscribers: [Subscriber; MAX_SUBSCRIBERS_PER_KIND],
    count: usize,
}

impl SubscriberList {
    const fn new() -> Self {
        Self {
            subscribers: [Subscriber::empty(); MAX_SUBSCRIBERS_PER_KIND],
            count: 0,
        }
    }

    fn add(&mut self, func: SubscriberFn, priority: u8) -> bool {
        if self.count >= MAX_SUBSCRIBERS_PER_KIND {
            return false;
        }
        self.subscribers[self.count] = Subscriber {
            func,
            priority,
            active: true,
        };
        self.count += 1;
        // Sort by priority (descending) — simple insertion sort.
        for i in (1..self.count).rev() {
            if self.subscribers[i].priority > self.subscribers[i - 1].priority {
                self.subscribers.swap(i, i - 1);
            } else {
                break;
            }
        }
        true
    }

    fn remove(&mut self, func: SubscriberFn) -> bool {
        let func_ptr = func as usize;
        for i in 0..self.count {
            if self.subscribers[i].func as usize == func_ptr {
                // Shift remaining elements.
                for j in i..self.count.saturating_sub(1) {
                    self.subscribers[j] = self.subscribers[j + 1];
                }
                self.count -= 1;
                self.subscribers[self.count] = Subscriber::empty();
                return true;
            }
        }
        false
    }
}

/// Global event bus state.
static BUS: Mutex<EventBus> = Mutex::new(EventBus::new());

struct EventBus {
    lists: [SubscriberList; EVENT_KIND_COUNT],
}

impl EventBus {
    const fn new() -> Self {
        const EMPTY: SubscriberList = SubscriberList::new();
        Self {
            lists: [EMPTY; EVENT_KIND_COUNT],
        }
    }
}

/// Re-entrancy guard.
static PUBLISHING: AtomicBool = AtomicBool::new(false);

/// Statistics.
static EVENTS_PUBLISHED: AtomicU64 = AtomicU64::new(0);
static EVENTS_DELIVERED: AtomicU64 = AtomicU64::new(0);
static EVENTS_DROPPED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Subscribe to events of a given kind.
///
/// `priority`: 0-255, higher priority subscribers are called first.
/// Returns `true` if registered, `false` if the subscriber list is full.
pub fn subscribe(kind: EventKind, func: SubscriberFn, priority: u8) -> bool {
    let mut bus = BUS.lock();
    let idx = kind as usize;
    if idx >= EVENT_KIND_COUNT {
        return false;
    }
    bus.lists[idx].add(func, priority)
}

/// Unsubscribe a callback from events of a given kind.
///
/// Returns `true` if the subscriber was found and removed.
pub fn unsubscribe(kind: EventKind, func: SubscriberFn) -> bool {
    let mut bus = BUS.lock();
    let idx = kind as usize;
    if idx >= EVENT_KIND_COUNT {
        return false;
    }
    bus.lists[idx].remove(func)
}

/// Publish an event to all subscribers of its kind.
///
/// Callbacks are invoked synchronously in the publisher's context,
/// ordered by priority (highest first).
///
/// Returns the number of subscribers notified.
///
/// # Re-entrancy
///
/// If a subscriber callback calls `publish()`, the nested publish
/// is dropped (returns 0) to prevent stack overflow.
pub fn publish(event: Event) -> usize {
    // Re-entrancy guard.
    if PUBLISHING.swap(true, Ordering::Acquire) {
        EVENTS_DROPPED.fetch_add(1, Ordering::Relaxed);
        return 0;
    }

    EVENTS_PUBLISHED.fetch_add(1, Ordering::Relaxed);

    let kind = event.kind();
    let idx = kind as usize;

    // Collect callbacks under the lock, then invoke outside the lock.
    // This prevents deadlocks if a callback tries to subscribe/unsubscribe.
    let callbacks: [(SubscriberFn, bool); MAX_SUBSCRIBERS_PER_KIND];
    let count;
    {
        let bus = BUS.lock();
        if idx >= EVENT_KIND_COUNT {
            PUBLISHING.store(false, Ordering::Release);
            return 0;
        }
        let list = &bus.lists[idx];
        count = list.count;
        let mut cbs = [(dummy_handler as SubscriberFn, false); MAX_SUBSCRIBERS_PER_KIND];
        for i in 0..count.min(MAX_SUBSCRIBERS_PER_KIND) {
            cbs[i] = (list.subscribers[i].func, list.subscribers[i].active);
        }
        callbacks = cbs;
    }

    // Invoke callbacks outside the lock.
    let mut delivered = 0usize;
    for i in 0..count.min(MAX_SUBSCRIBERS_PER_KIND) {
        let (func, active) = callbacks[i];
        if active {
            (func)(&event);
            delivered += 1;
        }
    }

    EVENTS_DELIVERED.fetch_add(delivered as u64, Ordering::Relaxed);
    PUBLISHING.store(false, Ordering::Release);
    delivered
}

/// Get event bus statistics.
#[must_use]
pub fn stats() -> EventBusStats {
    let bus = BUS.lock();
    let mut subscriber_counts = [0u8; EVENT_KIND_COUNT];
    for (i, list) in bus.lists.iter().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        { subscriber_counts[i] = list.count as u8; }
    }
    EventBusStats {
        events_published: EVENTS_PUBLISHED.load(Ordering::Relaxed),
        events_delivered: EVENTS_DELIVERED.load(Ordering::Relaxed),
        events_dropped: EVENTS_DROPPED.load(Ordering::Relaxed),
        subscriber_counts,
    }
}

/// Statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct EventBusStats {
    /// Total events published.
    pub events_published: u64,
    /// Total event deliveries (sum of all subscriber invocations).
    pub events_delivered: u64,
    /// Events dropped due to re-entrancy.
    pub events_dropped: u64,
    /// Number of subscribers per event kind.
    pub subscriber_counts: [u8; EVENT_KIND_COUNT],
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the kernel event bus.
pub fn self_test() {
    serial_println!("[kevent] Running self-test...");

    // Test 1: Subscribe.
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_handler(event: &Event) {
        if let Event::Custom { code, data } = event {
            TEST_COUNTER.store(
                (*code as u64).wrapping_mul(1000).wrapping_add(*data),
                Ordering::Relaxed,
            );
        }
    }

    assert!(subscribe(EventKind::Custom, test_handler, 100));
    serial_println!("[kevent]   Subscribe: OK");

    // Test 2: Publish.
    let delivered = publish(Event::Custom { code: 7, data: 42 });
    assert_eq!(delivered, 1);
    let val = TEST_COUNTER.load(Ordering::Relaxed);
    assert_eq!(val, 7042, "Expected 7*1000+42=7042, got {}", val);
    serial_println!("[kevent]   Publish + delivery: OK (value={})", val);

    // Test 3: Priority ordering.
    static PRIORITY_ORDER: Mutex<[u8; 4]> = Mutex::new([0; 4]);
    static PRIORITY_IDX: AtomicU64 = AtomicU64::new(0);

    fn handler_low(_: &Event) {
        let idx = PRIORITY_IDX.fetch_add(1, Ordering::Relaxed) as usize;
        if let Some(slot) = PRIORITY_ORDER.lock().get_mut(idx) {
            *slot = 1; // low priority = 1
        }
    }
    fn handler_high(_: &Event) {
        let idx = PRIORITY_IDX.fetch_add(1, Ordering::Relaxed) as usize;
        if let Some(slot) = PRIORITY_ORDER.lock().get_mut(idx) {
            *slot = 2; // high priority = 2
        }
    }

    assert!(subscribe(EventKind::MemoryPressure, handler_low, 50));
    assert!(subscribe(EventKind::MemoryPressure, handler_high, 200));

    PRIORITY_IDX.store(0, Ordering::Relaxed);
    let delivered = publish(Event::MemoryPressure { level: 1, score: 30 });
    assert_eq!(delivered, 2);

    let order = *PRIORITY_ORDER.lock();
    assert_eq!(order[0], 2, "High priority should be called first");
    assert_eq!(order[1], 1, "Low priority should be called second");
    serial_println!("[kevent]   Priority ordering: OK");

    // Test 4: Unsubscribe.
    assert!(unsubscribe(EventKind::Custom, test_handler));
    let delivered = publish(Event::Custom { code: 99, data: 0 });
    assert_eq!(delivered, 0);
    serial_println!("[kevent]   Unsubscribe: OK");

    // Test 5: Stats.
    let st = stats();
    assert!(st.events_published >= 3);
    assert!(st.events_delivered >= 3);
    serial_println!("[kevent]   Stats: OK (published={}, delivered={}, dropped={})",
        st.events_published, st.events_delivered, st.events_dropped);

    // Cleanup.
    unsubscribe(EventKind::MemoryPressure, handler_low);
    unsubscribe(EventKind::MemoryPressure, handler_high);

    serial_println!("[kevent] Self-test PASSED");
}

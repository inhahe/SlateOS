//! Display hotplug detection framework.
//!
//! Provides a unified mechanism for detecting display connect/disconnect
//! events across all DRM backends and notifying interested parties
//! (compositor, window manager, userspace).
//!
//! ## Architecture
//!
//! ```text
//!   ┌─────────────┐     ┌──────────────┐     ┌──────────────┐
//!   │  HPD IRQ     │     │ virtio-gpu   │     │  Polling     │
//!   │  (real HW)   │────▸│  event       │────▸│  timer       │
//!   └──────────────┘     └──────────────┘     └──────────────┘
//!           │                    │                    │
//!           ▼                    ▼                    ▼
//!   ┌─────────────────────────────────────────────────────┐
//!   │              Hotplug Event Queue                     │
//!   │   (ring buffer, lock-free producer, locked consumer) │
//!   └──────────────────────────────────────────────────────┘
//!           │
//!           ▼
//!   ┌────────────────────────────────────┐
//!   │         Notifier callbacks          │
//!   │  • Compositor: re-enumerate outputs │
//!   │  • Userspace:  uevent / IPC msg     │
//!   └────────────────────────────────────┘
//! ```
//!
//! ## Hotplug Sources
//!
//! - **Real hardware**: HPD (Hot Plug Detect) interrupt on HDMI/DP/DVI.
//!   The GPU driver's ISR calls `submit_event()`.
//! - **virtio-gpu**: The host signals display configuration changes.
//!   The virtio-gpu driver polls or handles IRQ, calls `submit_event()`.
//! - **Limine**: No hotplug (fixed framebuffer). Always "connected".
//! - **Polling fallback**: `poll_all_devices()` checks connector status
//!   periodically for backends that lack interrupt-driven detection.
//!
//! ## References
//!
//! - Linux `drivers/gpu/drm/drm_probe_helper.c` — polling connector status
//! - Linux `drivers/gpu/drm/drm_connector.c` — hotplug uevent

extern crate alloc;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use super::connector::ConnectorStatus;
use super::DrmObjectId;

// ---------------------------------------------------------------------------
// Hotplug event
// ---------------------------------------------------------------------------

/// A display hotplug event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotplugEvent {
    /// DRM device index.
    pub device_index: usize,
    /// Connector that changed.
    pub connector_id: DrmObjectId,
    /// New status.
    pub new_status: ConnectorStatus,
    /// Monotonic timestamp (from TSC or timer tick).
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Event queue (lock-free ring buffer)
// ---------------------------------------------------------------------------

/// Maximum queued hotplug events before oldest are dropped.
///
/// Hotplug events are rare (seconds between them at fastest), so a
/// small queue suffices.  If the consumer doesn't drain in time,
/// we drop the oldest event — display state is eventually consistent
/// via the next poll cycle.
const EVENT_QUEUE_SIZE: usize = 16;

/// Lock-free single-producer multi-consumer event queue.
///
/// Producers (ISR, polling thread) write via `submit_event()`.
/// The consumer (compositor/notifier) reads via `drain_events()`.
///
/// Uses a simple array with atomic head/tail indices.  Safe for
/// single producer; if multiple producers exist, external locking
/// is needed on the producer side (the DRM device lock serves this).
struct EventQueue {
    events: [HotplugEvent; EVENT_QUEUE_SIZE],
    /// Next write position (only modified by producer).
    head: AtomicU64,
    /// Next read position (only modified by consumer).
    tail: AtomicU64,
}

impl EventQueue {
    const fn new() -> Self {
        Self {
            events: [HotplugEvent {
                device_index: 0,
                connector_id: DrmObjectId::new(0),
                new_status: ConnectorStatus::Unknown,
                timestamp: 0,
            }; EVENT_QUEUE_SIZE],
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
        }
    }

    /// Push an event into the queue.
    ///
    /// If the queue is full, the oldest unread event is silently dropped.
    /// This is acceptable because hotplug state is idempotent — a full
    /// poll cycle will re-discover the correct state.
    #[allow(clippy::arithmetic_side_effects)]
    fn push(&mut self, event: HotplugEvent) {
        let head = self.head.load(Ordering::Relaxed);
        let idx = (head as usize) % EVENT_QUEUE_SIZE;
        self.events[idx] = event;
        self.head.store(head.wrapping_add(1), Ordering::Release);

        // If head caught up to tail, advance tail (drop oldest).
        let tail = self.tail.load(Ordering::Relaxed);
        if head.wrapping_sub(tail) >= EVENT_QUEUE_SIZE as u64 {
            self.tail.store(tail.wrapping_add(1), Ordering::Release);
        }
    }

    /// Drain all pending events into the provided buffer.
    ///
    /// Returns the number of events drained.
    #[allow(clippy::arithmetic_side_effects)]
    fn drain(&self, out: &mut [HotplugEvent]) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let mut tail = self.tail.load(Ordering::Relaxed);
        let mut count = 0;

        while tail != head && count < out.len() {
            let idx = (tail as usize) % EVENT_QUEUE_SIZE;
            out[count] = self.events[idx];
            count += 1;
            tail = tail.wrapping_add(1);
        }

        self.tail.store(tail, Ordering::Release);
        count
    }

    /// Check if there are pending events.
    fn has_pending(&self) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        head != tail
    }
}

// ---------------------------------------------------------------------------
// Global hotplug state
// ---------------------------------------------------------------------------

/// Global event queue, protected by a spinlock.
///
/// The lock is held briefly for push/drain operations.  This is
/// acceptable because hotplug events are rare (seconds apart at most).
static EVENT_QUEUE: Mutex<EventQueue> = Mutex::new(EventQueue::new());

/// Whether hotplug detection is enabled.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Total events submitted (diagnostic counter).
static TOTAL_EVENTS: AtomicU64 = AtomicU64::new(0);

/// Total events processed by consumers (diagnostic counter).
static TOTAL_PROCESSED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Notifier callbacks
// ---------------------------------------------------------------------------

/// Maximum registered notifiers.
const MAX_NOTIFIERS: usize = 8;

/// A hotplug notifier callback.
///
/// Called when a display connect/disconnect event is detected.
/// The callback receives the event and should perform any necessary
/// reconfiguration (e.g., updating the compositor's output list).
///
/// Callbacks run in the context of `process_pending_events()` —
/// typically a kernel worker thread, NOT interrupt context.
pub type NotifierFn = fn(&HotplugEvent);

/// Registered notifiers, protected by a spinlock.
static NOTIFIERS: Mutex<NotifierState> = Mutex::new(NotifierState::new());

/// Notifier registration state.
struct NotifierState {
    callbacks: [Option<NotifierFn>; MAX_NOTIFIERS],
    count: usize,
}

impl NotifierState {
    const fn new() -> Self {
        Self {
            callbacks: [None; MAX_NOTIFIERS],
            count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Enable hotplug detection.
///
/// Called during DRM initialization after all backends are registered.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable hotplug detection.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether hotplug detection is currently enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Submit a hotplug event.
///
/// Called by DRM backends when a connector's status changes.
/// Uses a spinlock internally — avoid calling from hard IRQ context
/// if the lock is already held (hotplug events are rare, so this is
/// not a practical concern).
pub fn submit_event(event: HotplugEvent) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    EVENT_QUEUE.lock().push(event);
    TOTAL_EVENTS.fetch_add(1, Ordering::Relaxed);

    serial_println!(
        "[drm-hp] Event: dev={} conn={} status={:?}",
        event.device_index,
        event.connector_id,
        event.new_status,
    );
}

/// Register a notifier callback for hotplug events.
///
/// Returns the notifier slot index on success.
pub fn register_notifier(callback: NotifierFn) -> KernelResult<usize> {
    let mut state = NOTIFIERS.lock();
    if state.count >= MAX_NOTIFIERS {
        return Err(KernelError::OutOfMemory);
    }

    let slot = state.count;
    state.callbacks[slot] = Some(callback);
    state.count = state.count.saturating_add(1);
    drop(state);

    serial_println!("[drm-hp] Registered notifier (slot {})", slot);
    Ok(slot)
}

/// Process all pending hotplug events.
///
/// Drains the event queue and calls each registered notifier for
/// every event.  Should be called periodically from a kernel worker
/// thread (or from the compositor's main loop).
///
/// Returns the number of events processed.
pub fn process_pending_events() -> usize {
    if !ENABLED.load(Ordering::Relaxed) {
        return 0;
    }

    let mut buf = [HotplugEvent {
        device_index: 0,
        connector_id: DrmObjectId::new(0),
        new_status: ConnectorStatus::Unknown,
        timestamp: 0,
    }; EVENT_QUEUE_SIZE];

    // Drain events while holding the queue lock (briefly).
    let count = EVENT_QUEUE.lock().drain(&mut buf);

    if count > 0 {
        // Call notifiers outside the queue lock to avoid deadlock if
        // a notifier submits a new event.
        let notifier_state = NOTIFIERS.lock();
        for event in &buf[..count] {
            for slot in 0..notifier_state.count {
                if let Some(cb) = notifier_state.callbacks[slot] {
                    cb(event);
                }
            }
        }
        drop(notifier_state);
        TOTAL_PROCESSED.fetch_add(count as u64, Ordering::Relaxed);
    }

    count
}

/// Check if there are unprocessed hotplug events.
#[must_use]
pub fn has_pending_events() -> bool {
    EVENT_QUEUE.lock().has_pending()
}

/// Poll all DRM devices for connector status changes.
///
/// Compares each connector's last-known status against the backend's
/// current detection.  Submits hotplug events for any changes.
///
/// This is the fallback detection path for backends without interrupt
/// support.  Real hardware drivers should use HPD interrupts instead.
pub fn poll_all_devices() {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let device_count = super::device_count();
    for dev_idx in 0..device_count {
        let _ = super::with_device(dev_idx, |dev| {
            for connector in dev.connectors() {
                // For now, virtual connectors are always Connected.
                // Real hardware backends will implement detect() which
                // reads the HPD pin or DDC presence.
                let detected = match connector.connector_type {
                    super::connector::ConnectorType::Virtual => {
                        ConnectorStatus::Connected
                    }
                    _ => {
                        // Future: call backend-specific detect() method.
                        // For now, report unknown for non-virtual.
                        ConnectorStatus::Unknown
                    }
                };

                // Only submit an event if the status actually changed.
                if detected != connector.status {
                    let timestamp = crate::bench::rdtsc();
                    submit_event(HotplugEvent {
                        device_index: dev_idx,
                        connector_id: connector.id,
                        new_status: detected,
                        timestamp,
                    });
                }
            }
            Ok(())
        });
    }
}

/// Diagnostic: total events submitted since boot.
#[must_use]
pub fn total_events() -> u64 {
    TOTAL_EVENTS.load(Ordering::Relaxed)
}

/// Diagnostic: total events processed by notifiers.
#[must_use]
pub fn total_processed() -> u64 {
    TOTAL_PROCESSED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run hotplug framework self-tests.
pub(crate) fn self_test() -> KernelResult<()> {
    // Save current state.
    let was_enabled = ENABLED.load(Ordering::Relaxed);
    let prev_events = TOTAL_EVENTS.load(Ordering::Relaxed);

    // 1. Enable hotplug.
    enable();
    if !is_enabled() {
        serial_println!("[drm-hp]   FAIL: enable() didn't set flag");
        return Err(KernelError::InternalError);
    }

    // 2. Submit a synthetic event.
    let test_event = HotplugEvent {
        device_index: 0,
        connector_id: DrmObjectId::new(42),
        new_status: ConnectorStatus::Connected,
        timestamp: 12345,
    };
    submit_event(test_event);

    if !has_pending_events() {
        serial_println!("[drm-hp]   FAIL: submit_event didn't queue");
        return Err(KernelError::InternalError);
    }

    // 3. Register a test notifier.
    static TEST_NOTIFIED: AtomicBool = AtomicBool::new(false);
    fn test_notifier(event: &HotplugEvent) {
        if event.connector_id == DrmObjectId::new(42) {
            TEST_NOTIFIED.store(true, Ordering::Release);
        }
    }
    TEST_NOTIFIED.store(false, Ordering::Release);
    register_notifier(test_notifier)?;

    // 4. Process events — should call our notifier.
    let processed = process_pending_events();
    if processed != 1 {
        serial_println!("[drm-hp]   FAIL: processed {} events (expected 1)", processed);
        return Err(KernelError::InternalError);
    }

    if !TEST_NOTIFIED.load(Ordering::Acquire) {
        serial_println!("[drm-hp]   FAIL: notifier was not called");
        return Err(KernelError::InternalError);
    }

    // 5. Queue should be empty now.
    if has_pending_events() {
        serial_println!("[drm-hp]   FAIL: queue not empty after drain");
        return Err(KernelError::InternalError);
    }

    // 6. Total events counter should have incremented.
    let new_events = TOTAL_EVENTS.load(Ordering::Relaxed);
    if new_events <= prev_events {
        serial_println!("[drm-hp]   FAIL: event counter didn't increment");
        return Err(KernelError::InternalError);
    }

    // 7. Disable and verify.
    disable();
    submit_event(test_event); // Should be silently dropped.
    if has_pending_events() {
        serial_println!("[drm-hp]   FAIL: event queued while disabled");
        return Err(KernelError::InternalError);
    }

    // Restore previous state.
    if was_enabled {
        enable();
    }

    serial_println!("[drm-hp]   Hotplug framework: OK");
    Ok(())
}

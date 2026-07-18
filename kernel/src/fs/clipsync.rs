//! Clipboard Sync — cross-device clipboard synchronization.
//!
//! Synchronizes clipboard contents between paired devices using
//! encrypted transfer with content filtering and size limits.
//!
//! ## Architecture
//!
//! ```text
//! Clipboard change
//!   → clipsync::on_copy(content) → queue for sync
//!   → clipsync::sync_to_device(device) → send content
//!   → clipsync::receive(content) → paste on local
//!
//! Integration:
//!   → clipboard (local clipboard)
//!   → multiclip (clipboard history)
//!   → mobilelink (phone link)
//!   → encrypt (transfer encryption)
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

/// Content type filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncContentType {
    Text,
    Image,
    File,
    RichText,
    Url,
}

impl SyncContentType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Image => "Image",
            Self::File => "File",
            Self::RichText => "Rich Text",
            Self::Url => "URL",
        }
    }
}

/// Sync direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    SendOnly,
    ReceiveOnly,
    Bidirectional,
}

impl SyncDirection {
    pub fn label(self) -> &'static str {
        match self {
            Self::SendOnly => "Send Only",
            Self::ReceiveOnly => "Receive Only",
            Self::Bidirectional => "Bidirectional",
        }
    }
}

/// A paired sync device.
#[derive(Debug, Clone)]
pub struct SyncDevice {
    pub id: u32,
    pub name: String,
    pub direction: SyncDirection,
    pub enabled: bool,
    pub last_sync_ns: u64,
    pub items_sent: u64,
    pub items_received: u64,
}

/// A sync queue entry.
#[derive(Debug, Clone)]
pub struct SyncEntry {
    pub id: u32,
    pub content_type: SyncContentType,
    pub preview: String,
    pub size_bytes: u64,
    pub synced: bool,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 20;
const MAX_QUEUE: usize = 100;

struct State {
    devices: Vec<SyncDevice>,
    queue: Vec<SyncEntry>,
    next_device_id: u32,
    next_entry_id: u32,
    enabled: bool,
    encrypted: bool,
    max_size_bytes: u64,
    allowed_types: Vec<SyncContentType>,
    total_sent: u64,
    total_received: u64,
    total_bytes_synced: u64,
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
        devices: Vec::new(),
        queue: Vec::new(),
        next_device_id: 1,
        next_entry_id: 1,
        enabled: false,
        encrypted: true,
        max_size_bytes: 10 * 1024 * 1024, // 10 MB.
        allowed_types: alloc::vec![SyncContentType::Text, SyncContentType::Url, SyncContentType::RichText],
        total_sent: 0,
        total_received: 0,
        total_bytes_synced: 0,
        ops: 0,
    });
}

/// Enable/disable clipboard sync.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled = enabled;
        Ok(())
    })
}

/// Set max sync size.
pub fn set_max_size(bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        state.max_size_bytes = bytes;
        Ok(())
    })
}

/// Add/remove an allowed content type.
pub fn set_type_allowed(ctype: SyncContentType, allowed: bool) -> KernelResult<()> {
    with_state(|state| {
        if allowed {
            if !state.allowed_types.contains(&ctype) {
                state.allowed_types.push(ctype);
            }
        } else {
            state.allowed_types.retain(|t| *t != ctype);
        }
        Ok(())
    })
}

/// Add a sync device.
pub fn add_device(name: &str, direction: SyncDirection) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_device_id;
        state.next_device_id += 1;
        state.devices.push(SyncDevice {
            id, name: String::from(name), direction,
            enabled: true, last_sync_ns: 0,
            items_sent: 0, items_received: 0,
        });
        Ok(id)
    })
}

/// Remove a sync device.
pub fn remove_device(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.devices.len();
        state.devices.retain(|d| d.id != id);
        if state.devices.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Queue content for sync (on local copy).
pub fn on_copy(content_type: SyncContentType, preview: &str, size_bytes: u64) -> KernelResult<u32> {
    with_state(|state| {
        if !state.enabled {
            return Err(KernelError::NotSupported);
        }
        if !state.allowed_types.contains(&content_type) {
            return Err(KernelError::PermissionDenied);
        }
        if size_bytes > state.max_size_bytes {
            return Err(KernelError::FileTooLarge);
        }
        if state.queue.len() >= MAX_QUEUE {
            state.queue.remove(0);
        }
        let id = state.next_entry_id;
        state.next_entry_id += 1;
        let now = crate::hpet::elapsed_ns();
        state.queue.push(SyncEntry {
            id, content_type,
            preview: String::from(preview),
            size_bytes, synced: false, timestamp_ns: now,
        });
        Ok(id)
    })
}

/// Simulate syncing to a device.
pub fn sync_to_device(device_id: u32) -> KernelResult<usize> {
    with_state(|state| {
        let device = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        if !device.enabled {
            return Err(KernelError::NotSupported);
        }
        let now = crate::hpet::elapsed_ns();
        let mut count = 0usize;
        for entry in &mut state.queue {
            if !entry.synced {
                entry.synced = true;
                device.items_sent += 1;
                state.total_sent += 1;
                state.total_bytes_synced += entry.size_bytes;
                count += 1;
            }
        }
        device.last_sync_ns = now;
        Ok(count)
    })
}

/// Receive content from a remote device.
pub fn receive(device_id: u32, content_type: SyncContentType, preview: &str, size_bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let device = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        device.items_received += 1;
        let now = crate::hpet::elapsed_ns();
        device.last_sync_ns = now;
        state.total_received += 1;
        state.total_bytes_synced += size_bytes;
        if state.queue.len() >= MAX_QUEUE { state.queue.remove(0); }
        let id = state.next_entry_id;
        state.next_entry_id += 1;
        state.queue.push(SyncEntry {
            id, content_type,
            preview: String::from(preview),
            size_bytes, synced: true, timestamp_ns: now,
        });
        Ok(())
    })
}

/// List devices.
pub fn list_devices() -> Vec<SyncDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Get sync queue.
pub fn get_queue(max: usize) -> Vec<SyncEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut q = s.queue.clone();
        q.reverse();
        q.truncate(max);
        q
    })
}

/// Is sync enabled?
pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.enabled)
}

/// Statistics: (device_count, total_sent, total_received, total_bytes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_sent, s.total_received, s.total_bytes_synced, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("clipsync::self_test() — running tests...");
    init_defaults();

    // 1: Disabled by default.
    assert!(!is_enabled());
    crate::serial_println!("  [1/8] disabled: OK");

    // 2: Enable and add device.
    set_enabled(true).expect("enable");
    let d1 = add_device("Phone", SyncDirection::Bidirectional).expect("add");
    assert_eq!(list_devices().len(), 1);
    crate::serial_println!("  [2/8] device: OK");

    // 3: Queue content.
    let _eid = on_copy(SyncContentType::Text, "Hello world", 11).expect("copy");
    let queue = get_queue(10);
    assert_eq!(queue.len(), 1);
    assert!(!queue[0].synced);
    crate::serial_println!("  [3/8] queue: OK");

    // 4: Sync to device.
    let count = sync_to_device(d1).expect("sync");
    assert_eq!(count, 1);
    let queue = get_queue(10);
    assert!(queue[0].synced);
    crate::serial_println!("  [4/8] sync: OK");

    // 5: Receive from device.
    receive(d1, SyncContentType::Url, "https://example.com", 20).expect("recv");
    assert_eq!(get_queue(10).len(), 2);
    crate::serial_println!("  [5/8] receive: OK");

    // 6: Type filtering.
    assert!(on_copy(SyncContentType::Image, "img", 1000).is_err()); // Image not in allowed types.
    set_type_allowed(SyncContentType::Image, true).expect("allow");
    assert!(on_copy(SyncContentType::Image, "img", 1000).is_ok());
    crate::serial_println!("  [6/8] filtering: OK");

    // 7: Size limit.
    assert!(on_copy(SyncContentType::Text, "big", 100_000_000).is_err());
    crate::serial_println!("  [7/8] size limit: OK");

    // 8: Stats.
    let (devices, sent, received, bytes, ops) = stats();
    assert_eq!(devices, 1);
    assert_eq!(sent, 1);
    assert_eq!(received, 1);
    assert!(bytes > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("clipsync::self_test() — all 8 tests passed");
}

//! IPC Log — inter-process communication message logging.
//!
//! Records IPC messages between processes for debugging and
//! auditing. Tracks channel usage, message sizes, and latency.
//! Supports filtering by source/destination PID or channel.
//!
//! ## Architecture
//!
//! ```text
//! IPC logging
//!   → ipclog::record(msg) → log an IPC message
//!   → ipclog::query(filter) → query log
//!   → ipclog::channel_stats(ch) → per-channel stats
//!   → ipclog::summary() → system-wide IPC summary
//!
//! Integration:
//!   → tracemon (trace monitor)
//!   → audit (audit logging)
//!   → perfmon (performance monitor)
//!   → procstat (process statistics)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// IPC message type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgType {
    Send,
    Receive,
    Reply,
    Signal,
    Broadcast,
    Error,
}

impl MsgType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Receive => "recv",
            Self::Reply => "reply",
            Self::Signal => "signal",
            Self::Broadcast => "bcast",
            Self::Error => "error",
        }
    }
}

/// A logged IPC message.
#[derive(Debug, Clone)]
pub struct IpcMessage {
    pub seq: u64,
    pub msg_type: MsgType,
    pub src_pid: u32,
    pub dst_pid: u32,
    pub channel_id: u32,
    pub size_bytes: u32,
    pub latency_us: u64,
    pub timestamp_ns: u64,
    pub label: String,
}

/// Per-channel statistics.
#[derive(Debug, Clone)]
pub struct ChannelStats {
    pub channel_id: u32,
    pub name: String,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub avg_latency_us: u64,
    pub errors: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_MESSAGES: usize = 4096;
const MAX_CHANNELS: usize = 256;

struct State {
    messages: Vec<IpcMessage>,
    channels: Vec<ChannelStats>,
    next_seq: u64,
    total_messages: u64,
    total_bytes: u64,
    total_errors: u64,
    logging_enabled: bool,
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
        messages: Vec::new(),
        channels: alloc::vec![
            ChannelStats { channel_id: 1, name: String::from("system_bus"), messages_sent: 100, messages_received: 100, bytes_sent: 25600, bytes_received: 12800, avg_latency_us: 15, errors: 0 },
            ChannelStats { channel_id: 2, name: String::from("vfs_channel"), messages_sent: 500, messages_received: 500, bytes_sent: 204800, bytes_received: 1048576, avg_latency_us: 45, errors: 2 },
            ChannelStats { channel_id: 3, name: String::from("gui_events"), messages_sent: 1000, messages_received: 980, bytes_sent: 64000, bytes_received: 32000, avg_latency_us: 5, errors: 0 },
        ],
        next_seq: 1,
        total_messages: 0,
        total_bytes: 0,
        total_errors: 0,
        logging_enabled: true,
        ops: 0,
    });
}

/// Record an IPC message.
pub fn record(msg_type: MsgType, src: u32, dst: u32, channel: u32, size: u32, latency_us: u64, label: &str) -> KernelResult<u64> {
    with_state(|state| {
        if !state.logging_enabled { return Ok(0); }
        let now = crate::hpet::elapsed_ns();
        let seq = state.next_seq;
        state.next_seq += 1;
        // Update channel stats.
        if let Some(ch) = state.channels.iter_mut().find(|c| c.channel_id == channel) {
            match msg_type {
                MsgType::Send | MsgType::Broadcast | MsgType::Signal => {
                    ch.messages_sent += 1;
                    ch.bytes_sent += size as u64;
                }
                MsgType::Receive | MsgType::Reply => {
                    ch.messages_received += 1;
                    ch.bytes_received += size as u64;
                }
                MsgType::Error => { ch.errors += 1; }
            }
            // Running average latency.
            let total = ch.messages_sent + ch.messages_received;
            if total > 0 {
                ch.avg_latency_us = (ch.avg_latency_us * (total - 1) + latency_us) / total;
            }
        } else if state.channels.len() < MAX_CHANNELS {
            let mut cs = ChannelStats {
                channel_id: channel, name: format!("ch_{}", channel),
                messages_sent: 0, messages_received: 0,
                bytes_sent: 0, bytes_received: 0,
                avg_latency_us: latency_us, errors: 0,
            };
            match msg_type {
                MsgType::Send | MsgType::Broadcast | MsgType::Signal => { cs.messages_sent = 1; cs.bytes_sent = size as u64; }
                MsgType::Receive | MsgType::Reply => { cs.messages_received = 1; cs.bytes_received = size as u64; }
                MsgType::Error => { cs.errors = 1; }
            }
            state.channels.push(cs);
        }
        state.total_messages += 1;
        state.total_bytes += size as u64;
        if msg_type == MsgType::Error { state.total_errors += 1; }
        // Store message.
        if state.messages.len() >= MAX_MESSAGES { state.messages.remove(0); }
        state.messages.push(IpcMessage {
            seq, msg_type, src_pid: src, dst_pid: dst, channel_id: channel,
            size_bytes: size, latency_us, timestamp_ns: now,
            label: String::from(label),
        });
        Ok(seq)
    })
}

/// Query messages by PID (src or dst).
pub fn query_pid(pid: u32, last_n: usize) -> Vec<IpcMessage> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let filtered: Vec<_> = s.messages.iter()
            .filter(|m| m.src_pid == pid || m.dst_pid == pid)
            .cloned().collect();
        let start = if last_n >= filtered.len() { 0 } else { filtered.len() - last_n };
        filtered[start..].to_vec()
    })
}

/// Query messages by channel.
pub fn query_channel(channel_id: u32, last_n: usize) -> Vec<IpcMessage> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let filtered: Vec<_> = s.messages.iter()
            .filter(|m| m.channel_id == channel_id)
            .cloned().collect();
        let start = if last_n >= filtered.len() { 0 } else { filtered.len() - last_n };
        filtered[start..].to_vec()
    })
}

/// Get recent messages.
pub fn recent(n: usize) -> Vec<IpcMessage> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if n >= s.messages.len() { 0 } else { s.messages.len() - n };
        s.messages[start..].to_vec()
    })
}

/// Get per-channel stats.
pub fn channel_stats(channel_id: u32) -> Option<ChannelStats> {
    STATE.lock().as_ref().and_then(|s| s.channels.iter().find(|c| c.channel_id == channel_id).cloned())
}

/// List all channels.
pub fn list_channels() -> Vec<ChannelStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.channels.clone())
}

/// Enable/disable logging.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.logging_enabled = enabled; Ok(()) })
}

/// Clear message log.
pub fn clear() -> KernelResult<()> {
    with_state(|state| { state.messages.clear(); Ok(()) })
}

/// Statistics: (channel_count, message_count, total_messages, total_bytes, total_errors, enabled, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.channels.len(), s.messages.len(), s.total_messages, s.total_bytes, s.total_errors, s.logging_enabled, s.ops),
        None => (0, 0, 0, 0, 0, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("ipclog::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_channels().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record message.
    let seq = record(MsgType::Send, 1, 100, 1, 256, 10, "test_msg").expect("record");
    assert!(seq >= 1);
    crate::serial_println!("  [2/8] record: OK");

    // 3: Query by PID.
    let msgs = query_pid(1, 10);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].src_pid, 1);
    crate::serial_println!("  [3/8] query pid: OK");

    // 4: Query by channel.
    record(MsgType::Receive, 100, 1, 1, 128, 12, "reply").expect("record2");
    let msgs = query_channel(1, 10);
    assert_eq!(msgs.len(), 2);
    crate::serial_println!("  [4/8] query channel: OK");

    // 5: Channel stats updated.
    let ch = channel_stats(1).expect("ch");
    assert!(ch.messages_sent >= 101);
    crate::serial_println!("  [5/8] channel stats: OK");

    // 6: New channel auto-create.
    record(MsgType::Send, 200, 300, 99, 512, 20, "new_ch").expect("record3");
    assert!(channel_stats(99).is_some());
    crate::serial_println!("  [6/8] auto channel: OK");

    // 7: Clear.
    clear().expect("clear");
    assert_eq!(recent(10).len(), 0);
    crate::serial_println!("  [7/8] clear: OK");

    // 8: Stats.
    let (channels, msgs, total, bytes, errors, enabled, ops) = stats();
    assert!(channels >= 4);
    assert_eq!(msgs, 0); // Cleared.
    assert!(total >= 3);
    assert!(bytes > 0);
    let _ = errors;
    assert!(enabled);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("ipclog::self_test() — all 8 tests passed");
}

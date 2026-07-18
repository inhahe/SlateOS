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

/// Initialise an **empty** IPC log.
///
/// Seeds NO channels and NO messages.  Channels and messages are tracked through
/// [`record`] as the IPC subsystem services sends/receives; until that wiring
/// exists, `/proc/ipclog` and the `ipclog` kshell command report an empty log
/// rather than fabricated traffic — the kernel's hard "never invent data in
/// procfs" rule.  `logging_enabled` defaults to `true` (a real setting: once
/// `record` is wired the log captures immediately).
///
/// (Previously this seeded three fabricated channels — `system_bus` (channel 1,
/// 100 sent / 100 recv / 25600 / 12800 bytes / 15us), `vfs_channel` (channel 2,
/// 500 / 500 / 204800 / 1048576 / 45us / 2 errors) and `gui_events` (channel 3,
/// 1000 / 980 / 64000 / 32000 / 5us) — which `/proc/ipclog` and the `list_channels`
/// view then displayed as if they were real measured IPC traffic.  The per-channel
/// counts were even inconsistent with the system totals, which were seeded at 0.
/// None of [`record`]'s callers are real — the IPC subsystem does not yet call it —
/// so the module is entirely unwired; the real channel registry lives in
/// `crate::ipc`.  See the DEFERRED PROPER FIX note in todo.txt.  The self-test now
/// builds its own fixtures via the real API — see [`self_test`].)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        messages: Vec::new(),
        channels: Vec::new(),
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
    // Start from a clean slate so the fixtures built below can never leak into
    // the live /proc/ipclog table (this self-test now runs at boot).
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated channels or messages.
    assert_eq!(list_channels().len(), 0);
    let (c0, m0, t0, b0, e0, en0, _o0) = stats();
    assert_eq!((c0, m0, t0, b0, e0), (0, 0, 0, 0, 0));
    assert!(en0); // logging enabled by default.
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Record a message — auto-creates channel 1.
    let seq = record(MsgType::Send, 1, 100, 1, 256, 10, "test_msg").expect("record");
    assert_eq!(seq, 1);
    crate::serial_println!("  [2/8] record: OK");

    // 3: Query by PID.
    let msgs = query_pid(1, 10);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].src_pid, 1);
    crate::serial_println!("  [3/8] query pid: OK");

    // 4: Query by channel — second message on channel 1.
    record(MsgType::Receive, 100, 1, 1, 128, 12, "reply").expect("record2");
    let msgs = query_channel(1, 10);
    assert_eq!(msgs.len(), 2);
    crate::serial_println!("  [4/8] query channel: OK");

    // 5: Channel stats — exactly 1 send + 1 receive recorded on channel 1.
    let ch = channel_stats(1).expect("ch");
    assert_eq!(ch.messages_sent, 1);
    assert_eq!(ch.messages_received, 1);
    crate::serial_println!("  [5/8] channel stats: OK");

    // 6: New channel auto-create.
    record(MsgType::Send, 200, 300, 99, 512, 20, "new_ch").expect("record3");
    assert!(channel_stats(99).is_some());
    crate::serial_println!("  [6/8] auto channel: OK");

    // 7: Clear — the message buffer empties (channel stats persist).
    clear().expect("clear");
    assert_eq!(recent(10).len(), 0);
    crate::serial_println!("  [7/8] clear: OK");

    // 8: Stats — exact totals (2 channels, message buffer cleared, 3 recorded,
    //    896 bytes, no errors, logging still enabled).
    let (channels, msgs, total, bytes, errors, enabled, ops) = stats();
    assert_eq!(channels, 2);
    assert_eq!(msgs, 0);
    assert_eq!(total, 3);
    assert_eq!(bytes, 896);
    assert_eq!(errors, 0);
    assert!(enabled);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Reset so the boot self-test leaves no fixtures behind in /proc/ipclog.
    *STATE.lock() = None;

    crate::serial_println!("ipclog::self_test() — all 8 tests passed");
}

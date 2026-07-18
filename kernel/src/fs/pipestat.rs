//! Pipe Statistics — pipe/FIFO I/O monitoring.
//!
//! Tracks pipe creation, data throughput, buffer utilization,
//! and reader/writer blocking events. Essential for IPC
//! performance diagnostics.
//!
//! ## Architecture
//!
//! ```text
//! Pipe monitoring
//!   → pipestat::create(pid) → track pipe creation
//!   → pipestat::destroy(id) → track pipe destruction
//!   → pipestat::record_write(id, bytes) → write data
//!   → pipestat::record_read(id, bytes) → read data
//!
//! Integration:
//!   → fdtable (file descriptor table)
//!   → ipclog (IPC logging)
//!   → taskstats (per-task accounting)
//!   → epollstat (event polling)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Pipe type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeType {
    Anonymous,
    Named,
}

impl PipeType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Anonymous => "anon",
            Self::Named => "named",
        }
    }
}

/// An active pipe.
#[derive(Debug, Clone)]
pub struct Pipe {
    pub id: u32,
    pub pipe_type: PipeType,
    pub reader_pid: u32,
    pub writer_pid: u32,
    pub buffer_size: u32,
    pub buffered_bytes: u32,
    pub bytes_written: u64,
    pub bytes_read: u64,
    pub write_blocks: u64,
    pub read_blocks: u64,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PIPES: usize = 512;

struct State {
    pipes: Vec<Pipe>,
    next_id: u32,
    total_created: u64,
    total_destroyed: u64,
    total_bytes: u64,
    total_blocks: u64,
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

/// Initialise an **empty** pipe table.
///
/// Seeds NO pipe rows and zero totals.  Real pipe accounting is wired through
/// [`create`]/[`destroy`]/[`record_write`]/[`record_read`]; until those are
/// called the table is genuinely empty, so the `/proc/pipestat` file and the
/// `pipestat` kshell command report zeros rather than fabricated numbers —
/// the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded two fictional pipes (an anonymous pipe with
/// bytes_written 500_000_000, and a named pipe) plus invented aggregate
/// totals (total_created 50_000, total_bytes 1_199_996_000), which
/// `/proc/pipestat` then displayed as if they were real IPC throughput
/// statistics.  That demo data was removed; the self-test now builds its own
/// fixtures explicitly via the real API (see [`self_test`]).  The pipe/FIFO
/// syscall path is expected to call [`create`] when a pipe is made and the
/// `record_*` functions as data flows.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        pipes: Vec::new(),
        next_id: 1,
        total_created: 0,
        total_destroyed: 0,
        total_bytes: 0,
        total_blocks: 0,
        ops: 0,
    });
}

/// Create a pipe.
pub fn create(reader_pid: u32, writer_pid: u32, pipe_type: PipeType, buffer_size: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.pipes.len() >= MAX_PIPES { return Err(KernelError::ResourceExhausted); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.total_created += 1;
        state.pipes.push(Pipe {
            id, pipe_type, reader_pid, writer_pid, buffer_size,
            buffered_bytes: 0, bytes_written: 0, bytes_read: 0,
            write_blocks: 0, read_blocks: 0, created_ns: now,
        });
        Ok(id)
    })
}

/// Destroy a pipe.
pub fn destroy(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.pipes.iter().position(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        state.pipes.remove(idx);
        state.total_destroyed += 1;
        Ok(())
    })
}

/// Record a write to a pipe.
pub fn record_write(id: u32, bytes: u32, blocked: bool) -> KernelResult<()> {
    with_state(|state| {
        let p = state.pipes.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        p.bytes_written += bytes as u64;
        p.buffered_bytes = (p.buffered_bytes + bytes).min(p.buffer_size);
        if blocked { p.write_blocks += 1; state.total_blocks += 1; }
        state.total_bytes += bytes as u64;
        Ok(())
    })
}

/// Record a read from a pipe.
pub fn record_read(id: u32, bytes: u32, blocked: bool) -> KernelResult<()> {
    with_state(|state| {
        let p = state.pipes.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        p.bytes_read += bytes as u64;
        p.buffered_bytes = p.buffered_bytes.saturating_sub(bytes);
        if blocked { p.read_blocks += 1; state.total_blocks += 1; }
        state.total_bytes += bytes as u64;
        Ok(())
    })
}

/// List active pipes.
pub fn list() -> Vec<Pipe> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.pipes.clone())
}

/// Pipes by PID (as reader or writer).
pub fn by_pid(pid: u32) -> Vec<Pipe> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.pipes.iter().filter(|p| p.reader_pid == pid || p.writer_pid == pid).cloned().collect()
    })
}

/// Statistics: (active_pipes, total_created, total_destroyed, total_bytes, total_blocks, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.pipes.len(), s.total_created, s.total_destroyed, s.total_bytes, s.total_blocks, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("pipestat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/pipestat must never surface).
    // Resetting first clears any residue from a prior `pipestat test` run so
    // the totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated rows.
    assert_eq!(list().len(), 0);
    let (a0, c0, d0, b0, bl0, _o0) = stats();
    assert_eq!((a0, c0, d0, b0, bl0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Create (ids start at 1).
    let id = create(300, 200, PipeType::Anonymous, 65536).expect("create");
    assert_eq!(id, 1);
    assert_eq!(list().len(), 1);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Write (exact, from zero; buffered tracks bytes in flight).
    record_write(id, 1024, false).expect("write");
    let p = list().iter().find(|p| p.id == id).cloned().expect("pipe");
    assert_eq!(p.bytes_written, 1024);
    assert_eq!(p.buffered_bytes, 1024);
    crate::serial_println!("  [3/8] write: OK");

    // 4: Read drains the buffer.
    record_read(id, 512, false).expect("read");
    let p = list().iter().find(|p| p.id == id).cloned().expect("pipe");
    assert_eq!(p.bytes_read, 512);
    assert_eq!(p.buffered_bytes, 512);
    crate::serial_println!("  [4/8] read: OK");

    // 5: A blocked write bumps write_blocks; recording on an unknown id fails.
    record_write(id, 100, true).expect("block_write");
    let p = list().iter().find(|p| p.id == id).cloned().expect("pipe");
    assert_eq!(p.write_blocks, 1);
    assert!(record_write(9999, 1, false).is_err()); // NotFound
    crate::serial_println!("  [5/8] blocking: OK");

    // 6: A second pipe; by_pid reflects exact membership (pid 200 is reader on
    //    pipe 1 and writer on pipe 2).
    let id2 = create(100, 200, PipeType::Named, 65536).expect("create2");
    assert_eq!(by_pid(200).len(), 2);
    assert_eq!(by_pid(300).len(), 1); // only writer of pipe 1
    crate::serial_println!("  [6/8] by pid: OK");

    // 7: Destroy; double-destroy fails.
    destroy(id).expect("destroy");
    assert_eq!(list().len(), 1); // only pipe 2 remains
    assert!(destroy(id).is_err());
    let _ = id2;
    crate::serial_println!("  [7/8] destroy: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (active, created, destroyed, bytes, blocks, ops) = stats();
    assert_eq!(active, 1); // pipe 2
    assert_eq!(created, 2); // two creates
    assert_eq!(destroyed, 1); // one destroy
    assert_eq!(bytes, 1636); // 1024 write + 512 read + 100 write
    assert_eq!(blocks, 1); // one blocked write
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/pipestat table with its fixtures.  Reset to the uninitialised
    // state so production reads report an empty table until the pipe syscall
    // path wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("pipestat::self_test() — all 8 tests passed");
}

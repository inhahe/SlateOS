//! A-Q15's load harness (`design-decisions.md` §972): design A (one ring for
//! every socket) against design B (a ring per socket), under the same loads, in
//! the same boot.
//!
//! Everything runs over the netstack daemon's own loopback: a connection to our
//! own address is diverted in-process to a listener, so between the two ends
//! there is nothing but the kernel socket layer, the ring design under test and
//! the daemon -- no slirp, no host network, no variance that belongs to neither.
//!
//! ## Loads
//!
//! Each runs once per [`RingMode`], at tiers of `k` connections:
//!
//! 1. **Request/response:** `k` kernel tasks, one per connection, each making
//!    [`REQ_RESP_ROUNDS`] 64-byte round trips at once. Per-exchange latency.
//!    This is where A's single queue is contended: every task's round-trips
//!    wait for the ring's lock, where under B each task queues on its own ring.
//! 2. **Idle beside busy:** `k` idle connections probed in turn (a 16-byte
//!    round trip each, every [`PROBE_GAP_MS`]) while one more connection sends
//!    without pause from a task of its own. The probes' latency is the cost a
//!    busy socket imposes on quiet ones -- the head-of-line effect A-Q15 asks
//!    about.
//! 3. **Bulk:** one connection sends [`BULK_BYTES`]. Throughput, for the
//!    single-socket case where the designs should not differ.
//!
//! ## What the numbers mean under QEMU
//!
//! QEMU emulates the CPU, so absolute times are the emulator's, not a
//! machine's: they are printed, and labelled, but are not evidence about real
//! hardware. What *is* comparable is A against B in one boot: same emulator,
//! same host load, same daemon, alternating the mode and nothing else. Each
//! comparison line prints B's figure as a ratio of A's, and that ratio is the
//! result §972 asks for under QEMU. Absolute latencies and throughput wait for
//! bare metal (`deferred-questions.md`).
//!
//! Memory is exact rather than measured: design A maps one ring, design B one
//! per socket, each one shared-memory region, and the harness reports the rings
//! live at each tier ([`netstack_client::rings_live`]).
//!
//! ## Output
//!
//! One `[ring-bench] {json}` line per (shape, mode, tier), then one
//! `[ring-bench] {json}` comparison line per (shape, tier). [`run`] is the full
//! harness, run by the bench suite (`boot-test.sh --bench`); [`self_test`] is a
//! small version every boot runs so the harness cannot rot between bench boots.

use crate::error::{KernelError, KernelResult};
use crate::net::netstack_client::{self, RingMode};
use crate::net::socket::{self, SocketHandle};
use crate::serial_println;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Loopback port for the harness's listener, unused by the self-tests.
const PORT: u16 = 9120;

/// Most connections one load uses (the largest tier).
const MAX_K: usize = 24;

/// Round trips each request/response task makes.
const REQ_RESP_ROUNDS: usize = 32;

/// Bytes per request and per response.
const REQ_RESP_LEN: usize = 64;

/// Bytes per idle-connection probe.
const PROBE_LEN: usize = 16;

/// Probe rounds in the idle-beside-busy load (each probes every idle connection).
const PROBE_ROUNDS: usize = 8;

/// Pause between probe rounds.
const PROBE_GAP_MS: u64 = 50;

/// Bytes the bulk load sends.
const BULK_BYTES: usize = 256 * 1024;

/// Chunk the bulk and busy senders use: the daemon's send window.
const CHUNK: usize = 1024;

/// How long any load may take before it is abandoned as stuck.
const LOAD_DEADLINE_MS: u64 = 120_000;

/// A connection over the loopback: the client end, and the end the listener
/// accepted.
#[derive(Clone, Copy)]
struct Pair {
    client: SocketHandle,
    server: SocketHandle,
}

/// Send all of `buf`, blocking.
fn send_all(h: SocketHandle, buf: &[u8]) -> KernelResult<()> {
    let mut off = 0usize;
    while off < buf.len() {
        let rest = buf.get(off..).unwrap_or(&[]);
        let n = socket::send(h, rest, false)?;
        let n = usize::try_from(n).map_err(|_| KernelError::InternalError)?;
        if n == 0 {
            return Err(KernelError::BrokenPipe);
        }
        off = off.saturating_add(n);
    }
    Ok(())
}

/// Receive exactly `len` bytes into `buf`, blocking; end-of-stream is an error.
fn recv_exact(h: SocketHandle, buf: &mut [u8], len: usize) -> KernelResult<()> {
    let mut got = 0usize;
    while got < len {
        let dst = buf.get_mut(got..len).ok_or(KernelError::InternalError)?;
        let n = socket::recv(h, dst, false, false)?;
        let n = usize::try_from(n).map_err(|_| KernelError::InternalError)?;
        if n == 0 {
            return Err(KernelError::BrokenPipe);
        }
        got = got.saturating_add(n);
    }
    Ok(())
}

/// One round trip of `len` bytes: client to server, and back. Returns its
/// duration in nanoseconds.
fn round_trip(p: Pair, len: usize) -> KernelResult<u64> {
    let out = [0x5au8; REQ_RESP_LEN];
    let mut back = [0u8; REQ_RESP_LEN];
    let msg = out.get(..len).ok_or(KernelError::InvalidArgument)?;
    let t0 = crate::hrtimer::now_ns();
    send_all(p.client, msg)?;
    recv_exact(p.server, &mut back, len)?;
    send_all(p.server, msg)?;
    recv_exact(p.client, &mut back, len)?;
    Ok(crate::hrtimer::now_ns().saturating_sub(t0))
}

/// A listener and `k` connections accepted on it, all opened in `mode`.
///
/// Connections are made and accepted one at a time: the daemon's backlog holds
/// only a few, and a tier is larger.
fn open_pairs(mode: RingMode, k: usize) -> KernelResult<(SocketHandle, Vec<Pair>)> {
    let me_ip = crate::net::interface::ip().0;
    if me_ip == [0, 0, 0, 0] {
        return Err(KernelError::NotConnected);
    }
    netstack_client::set_ring_mode(Some(mode));
    let opened = (|| {
        let srv = socket::create(2)?;
        let setup = (|| {
            socket::bind_stream(srv, PORT)?;
            socket::listen(srv, 8)?;
            let mut pairs = Vec::new();
            pairs.try_reserve(k).map_err(|_| KernelError::OutOfMemory)?;
            for _ in 0..k {
                let c = socket::create(2)?;
                if let Err(e) = socket::connect(c, &me_ip, PORT, true) {
                    socket::close(c);
                    return Err(e);
                }
                let mut accepted = Err(KernelError::WouldBlock);
                for _ in 0..256u32 {
                    accepted = socket::accept(srv).map(|(h, _)| h);
                    if accepted.is_ok() {
                        break;
                    }
                    crate::sched::yield_now();
                }
                match accepted {
                    Ok(a) => pairs.push(Pair {
                        client: c,
                        server: a,
                    }),
                    Err(e) => {
                        socket::close(c);
                        close_pairs(&pairs);
                        return Err(e);
                    }
                }
            }
            Ok(pairs)
        })();
        match setup {
            Ok(pairs) => Ok((srv, pairs)),
            Err(e) => {
                socket::close(srv);
                Err(e)
            }
        }
    })();
    // Back to the boot's choice however the setup went; sockets keep their rings.
    netstack_client::set_ring_mode(None);
    opened
}

fn close_pairs(pairs: &[Pair]) {
    for p in pairs {
        socket::close(p.client);
        socket::close(p.server);
    }
}

// ---------------------------------------------------------------------------
// Workers: the request/response tasks and the busy sender
// ---------------------------------------------------------------------------

/// A worker's connection, as raw handles; 0 when unused.
static WORKER_CLIENT: [AtomicU64; MAX_K] = [const { AtomicU64::new(0) }; MAX_K];
static WORKER_SERVER: [AtomicU64; MAX_K] = [const { AtomicU64::new(0) }; MAX_K];

/// Each request/response exchange's latency, in microseconds (saturating):
/// worker `i`'s round `j` at `i * REQ_RESP_ROUNDS + j`. 0 means it failed.
static LATENCY_US: [AtomicU32; MAX_K * REQ_RESP_ROUNDS] =
    [const { AtomicU32::new(0) }; MAX_K * REQ_RESP_ROUNDS];

/// Rounds that failed, across all workers.
static WORKER_ERRORS: AtomicU32 = AtomicU32::new(0);

/// Workers finished.
static WORKERS_DONE: AtomicU32 = AtomicU32::new(0);

/// A request/response worker: [`REQ_RESP_ROUNDS`] round trips on its pair.
extern "C" fn req_resp_worker(idx: u64) {
    let Ok(i) = usize::try_from(idx) else {
        WORKERS_DONE.fetch_add(1, Ordering::AcqRel);
        return;
    };
    let client = WORKER_CLIENT
        .get(i)
        .map_or(0, |a| a.load(Ordering::Acquire));
    let server = WORKER_SERVER
        .get(i)
        .map_or(0, |a| a.load(Ordering::Acquire));
    let pair = Pair {
        client: SocketHandle::from_raw(client),
        server: SocketHandle::from_raw(server),
    };
    for j in 0..REQ_RESP_ROUNDS {
        let slot = i.saturating_mul(REQ_RESP_ROUNDS).saturating_add(j);
        match round_trip(pair, REQ_RESP_LEN) {
            Ok(ns) => {
                let us = u32::try_from(ns / 1_000).unwrap_or(u32::MAX).max(1);
                if let Some(a) = LATENCY_US.get(slot) {
                    a.store(us, Ordering::Release);
                }
            }
            Err(_) => {
                WORKER_ERRORS.fetch_add(1, Ordering::AcqRel);
            }
        }
    }
    WORKERS_DONE.fetch_add(1, Ordering::AcqRel);
}

/// The busy sender's pair, as raw handles.
static BUSY_CLIENT: AtomicU64 = AtomicU64::new(0);
static BUSY_SERVER: AtomicU64 = AtomicU64::new(0);
/// Set to stop the busy sender; it clears [`BUSY_RUNNING`] when it has.
static BUSY_STOP: AtomicBool = AtomicBool::new(false);
static BUSY_RUNNING: AtomicBool = AtomicBool::new(false);
/// Bytes the busy sender moved, for the report.
static BUSY_BYTES: AtomicU64 = AtomicU64::new(0);

/// The busy connection: send a chunk, drain it at the other end, until told
/// to stop.
extern "C" fn busy_sender(_arg: u64) {
    let pair = Pair {
        client: SocketHandle::from_raw(BUSY_CLIENT.load(Ordering::Acquire)),
        server: SocketHandle::from_raw(BUSY_SERVER.load(Ordering::Acquire)),
    };
    let chunk = [0xa5u8; CHUNK];
    let mut sink = [0u8; CHUNK];
    while !BUSY_STOP.load(Ordering::Acquire) {
        if send_all(pair.client, &chunk).is_err()
            || recv_exact(pair.server, &mut sink, CHUNK).is_err()
        {
            break;
        }
        BUSY_BYTES.fetch_add(CHUNK as u64, Ordering::Relaxed);
    }
    BUSY_RUNNING.store(false, Ordering::Release);
}

/// Wait until `done()` or the load deadline; `false` on the deadline.
fn wait_for(done: impl Fn() -> bool) -> bool {
    let deadline =
        crate::hrtimer::now_ns().saturating_add(LOAD_DEADLINE_MS.saturating_mul(1_000_000));
    while !done() {
        if crate::hrtimer::now_ns() >= deadline {
            return false;
        }
        crate::sched::sleep_ms(5);
    }
    true
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// The latency figures of one load.
#[derive(Clone, Copy, Default)]
struct Stats {
    samples: usize,
    p50_us: u32,
    p99_us: u32,
    max_us: u32,
}

/// Percentiles of `v` (sorted in place).
fn stats(v: &mut [u32]) -> Stats {
    v.sort_unstable();
    let pick = |pct: usize| -> u32 {
        if v.is_empty() {
            return 0;
        }
        let i = v.len().saturating_sub(1).saturating_mul(pct) / 100;
        v.get(i).copied().unwrap_or(0)
    };
    Stats {
        samples: v.len(),
        p50_us: pick(50),
        p99_us: pick(99),
        max_us: v.last().copied().unwrap_or(0),
    }
}

fn mode_name(mode: RingMode) -> &'static str {
    match mode {
        RingMode::Shared => "A",
        RingMode::PerSocket => "B",
    }
}

/// One result line.
fn report(shape: &str, mode: RingMode, k: usize, st: Stats, errors: u32, extra: &str) {
    let (rings, ring_bytes) = netstack_client::rings_live();
    serial_println!(
        "[ring-bench] {{\"shape\":\"{}\",\"mode\":\"{}\",\"k\":{},\"samples\":{},\
         \"p50_us\":{},\"p99_us\":{},\"max_us\":{},\"errors\":{},\"rings\":{},\
         \"ring_bytes\":{}{}}}",
        shape,
        mode_name(mode),
        k,
        st.samples,
        st.p50_us,
        st.p99_us,
        st.max_us,
        errors,
        rings,
        ring_bytes,
        extra
    );
}

// ---------------------------------------------------------------------------
// The loads
// ---------------------------------------------------------------------------

/// Load 1: `k` tasks, `REQ_RESP_ROUNDS` round trips each, at once.
fn req_resp(mode: RingMode, k: usize) -> KernelResult<Stats> {
    let k = k.min(MAX_K);
    let (srv, pairs) = open_pairs(mode, k)?;
    for a in &LATENCY_US {
        a.store(0, Ordering::Release);
    }
    WORKER_ERRORS.store(0, Ordering::Release);
    WORKERS_DONE.store(0, Ordering::Release);
    let mut started = 0u32;
    for (i, p) in pairs.iter().enumerate() {
        if let (Some(c), Some(s)) = (WORKER_CLIENT.get(i), WORKER_SERVER.get(i)) {
            c.store(p.client.raw(), Ordering::Release);
            s.store(p.server.raw(), Ordering::Release);
        }
        if crate::sched::spawn(b"ring-bench", 16, req_resp_worker, i as u64, 0).is_ok() {
            started = started.saturating_add(1);
        }
    }
    let finished = wait_for(|| WORKERS_DONE.load(Ordering::Acquire) >= started);
    let errors = WORKER_ERRORS.load(Ordering::Acquire);
    let mut lat: Vec<u32> = LATENCY_US
        .iter()
        .take(k.saturating_mul(REQ_RESP_ROUNDS))
        .map(|a| a.load(Ordering::Acquire))
        .filter(|&us| us != 0)
        .collect();
    let st = stats(&mut lat);
    report(
        "req_resp",
        mode,
        k,
        st,
        errors,
        if finished { "" } else { ",\"timed_out\":true" },
    );
    close_pairs(&pairs);
    socket::close(srv);
    if !finished || started as usize != k {
        return Err(KernelError::TimedOut);
    }
    Ok(st)
}

/// Load 2: probe `k` idle connections while one more sends without pause.
fn idle_beside_busy(mode: RingMode, k: usize) -> KernelResult<Stats> {
    let k = k.min(MAX_K);
    // k idle pairs plus the busy one, last.
    let (srv, pairs) = open_pairs(mode, k.saturating_add(1))?;
    let (idle, busy) = pairs.split_at(k);
    let Some(&busy) = busy.first() else {
        close_pairs(&pairs);
        socket::close(srv);
        return Err(KernelError::InternalError);
    };
    BUSY_CLIENT.store(busy.client.raw(), Ordering::Release);
    BUSY_SERVER.store(busy.server.raw(), Ordering::Release);
    BUSY_STOP.store(false, Ordering::Release);
    BUSY_BYTES.store(0, Ordering::Release);
    BUSY_RUNNING.store(true, Ordering::Release);
    if crate::sched::spawn(b"ring-bench-busy", 16, busy_sender, 0, 0).is_err() {
        BUSY_RUNNING.store(false, Ordering::Release);
    }
    let t0 = crate::hrtimer::now_ns();
    let mut lat = Vec::new();
    let mut errors = 0u32;
    for _ in 0..PROBE_ROUNDS {
        for p in idle {
            match round_trip(*p, PROBE_LEN) {
                Ok(ns) => {
                    if lat.try_reserve(1).is_ok() {
                        lat.push(u32::try_from(ns / 1_000).unwrap_or(u32::MAX).max(1));
                    }
                }
                Err(_) => errors = errors.saturating_add(1),
            }
        }
        crate::sched::sleep_ms(PROBE_GAP_MS);
    }
    BUSY_STOP.store(true, Ordering::Release);
    let stopped = wait_for(|| !BUSY_RUNNING.load(Ordering::Acquire));
    let secs_ms = crate::hrtimer::now_ns().saturating_sub(t0) / 1_000_000;
    let st = stats(&mut lat);
    let extra = alloc::format!(
        ",\"busy_bytes\":{},\"window_ms\":{}{}",
        BUSY_BYTES.load(Ordering::Acquire),
        secs_ms,
        if stopped { "" } else { ",\"busy_stuck\":true" }
    );
    report("idle_beside_busy", mode, k, st, errors, &extra);
    close_pairs(&pairs);
    socket::close(srv);
    Ok(st)
}

/// Load 3: one connection sends `BULK_BYTES`. Returns bytes per second.
fn bulk(mode: RingMode) -> KernelResult<u64> {
    let (srv, pairs) = open_pairs(mode, 1)?;
    let Some(&p) = pairs.first() else {
        socket::close(srv);
        return Err(KernelError::InternalError);
    };
    let chunk = [0x3cu8; CHUNK];
    let mut sink = [0u8; CHUNK];
    let t0 = crate::hrtimer::now_ns();
    let mut sent = 0usize;
    let mut outcome = Ok(());
    while sent < BULK_BYTES {
        if let Err(e) =
            send_all(p.client, &chunk).and_then(|()| recv_exact(p.server, &mut sink, CHUNK))
        {
            outcome = Err(e);
            break;
        }
        sent = sent.saturating_add(CHUNK);
    }
    let ns = crate::hrtimer::now_ns().saturating_sub(t0).max(1);
    let bps = (sent as u64)
        .saturating_mul(1_000_000_000)
        .checked_div(ns)
        .unwrap_or(0);
    let extra = alloc::format!(",\"bytes\":{},\"bytes_per_sec\":{}", sent, bps);
    report(
        "bulk",
        mode,
        1,
        Stats::default(),
        u32::from(outcome.is_err()),
        &extra,
    );
    close_pairs(&pairs);
    socket::close(srv);
    outcome.map(|()| bps)
}

/// A comparison line: B's figure as a ratio of A's, in percent.
fn compare(shape: &str, k: usize, a: u32, b: u32, what: &str) {
    // 0 when A is 0: no ratio to give.
    let pct = u64::from(b)
        .saturating_mul(100)
        .checked_div(u64::from(a))
        .unwrap_or(0);
    serial_println!(
        "[ring-bench] {{\"compare\":\"{}\",\"k\":{},\"metric\":\"{}\",\"a\":{},\"b\":{},\
         \"b_as_pct_of_a\":{},\"basis\":\"relative: same boot, same emulator\"}}",
        shape,
        k,
        what,
        a,
        b,
        pct
    );
}

/// The full harness: every load, both designs, the tiers `2, 8, 24`.
///
/// # Errors
///
/// The first load that fails or times out; every line printed before it
/// stands.
pub fn run() -> KernelResult<()> {
    serial_println!(
        "[ring-bench] A-Q15 load harness: design A (shared ring) against B (ring per socket)"
    );
    for k in [2usize, 8, 24] {
        let a = req_resp(RingMode::Shared, k)?;
        let b = req_resp(RingMode::PerSocket, k)?;
        compare("req_resp", k, a.p50_us, b.p50_us, "p50_us");
        compare("req_resp", k, a.p99_us, b.p99_us, "p99_us");
        let a = idle_beside_busy(RingMode::Shared, k)?;
        let b = idle_beside_busy(RingMode::PerSocket, k)?;
        compare("idle_beside_busy", k, a.p50_us, b.p50_us, "p50_us");
        compare("idle_beside_busy", k, a.p99_us, b.p99_us, "p99_us");
    }
    let a = bulk(RingMode::Shared)?;
    let b = bulk(RingMode::PerSocket)?;
    compare(
        "bulk",
        1,
        u32::try_from(a / 1024).unwrap_or(u32::MAX),
        u32::try_from(b / 1024).unwrap_or(u32::MAX),
        "kib_per_sec",
    );
    Ok(())
}

/// A small run of the harness on every boot, so it cannot rot between bench
/// boots: two connections, both designs, request/response and idle-beside-busy.
///
/// Returns `Ok(None)` when the interface has no IPv4 address yet: the
/// loopback divert keys on a non-zero local IP.
///
/// # Errors
///
/// A load that fails, times out, or produced no samples.
pub fn self_test() -> KernelResult<Option<()>> {
    if crate::net::interface::ip().0 == [0, 0, 0, 0] {
        return Ok(None);
    }
    for mode in [RingMode::Shared, RingMode::PerSocket] {
        let st = req_resp(mode, 2)?;
        if st.samples == 0 {
            serial_println!(
                "[ring-bench]   FAIL: request/response in design {} produced no samples",
                mode_name(mode)
            );
            return Err(KernelError::InternalError);
        }
        let st = idle_beside_busy(mode, 2)?;
        if st.samples == 0 {
            serial_println!(
                "[ring-bench]   FAIL: idle-beside-busy in design {} produced no samples",
                mode_name(mode)
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[ring-bench] self-test: both loads ran in both designs: OK");
    Ok(Some(()))
}

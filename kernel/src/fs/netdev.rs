//! Network Device — NIC-level packet/byte statistics.
//!
//! Tracks per-interface packet counts, byte totals, errors,
//! drops, and link state. Essential for diagnosing network
//! hardware and driver performance.
//!
//! ## Architecture
//!
//! ```text
//! Network device monitoring
//!   → netdev::record_rx(iface, bytes, pkts) → track received
//!   → netdev::record_tx(iface, bytes, pkts) → track transmitted
//!   → netdev::record_error(iface, dir) → track errors
//!   → netdev::set_link_state(iface, up) → link state change
//!
//! Integration:
//!   → netmon (network monitor)
//!   → netsock (socket stats)
//!   → netfilter (packet filtering)
//!   → sysdiag (diagnostics)
//! ```
//!
//! ## Where the numbers actually come from
//!
//! There are two sources, and the distinction matters for reading `/proc`:
//!
//! 1. **The kernel's own network stack**, [`crate::net::interface`], which is
//!    the real thing — it counts every frame that goes out through
//!    [`crate::net::send_frame`] and every frame that comes in through
//!    [`crate::net::poll`], for whichever NIC driver came up (virtio-net,
//!    e1000 or rtl8139). Those counters are already relaxed atomics and
//!    [`crate::net::interface::stats`] reads them. This module **projects**
//!    that snapshot into a synthetic [`NIC_IFACE`] row whenever a reader
//!    asks, rather than having the frame path call in on every packet — see
//!    below.
//! 2. **Registered interfaces**, whose counters move only when something calls
//!    [`record_rx`] and friends. Nothing in the tree does yet; those exist for
//!    a future per-NIC source and for the self-test.
//!
//! **Why projection rather than `record_*` calls on the frame path.** The
//! obvious wiring — have `net::poll` call `record_rx("eth0", …)` — is wrong for
//! the same reason it was wrong for [`crate::fs::pagecache`]. `record_rx` takes
//! this module's spin lock and then does a *string compare per registered
//! interface* to find its row; that would put a lock acquisition and a linear
//! scan on the path of every single frame, to produce a total the reader could
//! have got for free. `net::interface`'s counters are already there, already
//! fed, and already say what a monitoring reader wanted. Joining them at *read*
//! time costs nothing per frame and loses no information.
//!
//! **Why the projected row is called `eth0`.** It was called `kernel` until
//! 2026-08-27, because `net::interface` kept one set of counters fed by both the
//! NIC *and* `net::veth::poll`, so the number included frames that never touched
//! the wire and naming the row after a device would have been false. That is
//! fixed: `net::interface` now keeps a counter group per source, and
//! [`nic_row`] projects [`crate::net::interface::Source::Nic`] — frames that
//! genuinely crossed the wire and nothing else. The row can therefore carry the
//! device's real name, matching `netstat -i`. Veth traffic is not lost; it is
//! reported under its own source there.
//!
//! The name stays **reserved** against [`register_iface`] for the same reason it
//! always was: two rows called `eth0` — one projected, one recorded — would make
//! it impossible to tell which source a number came from. If a genuine per-NIC
//! recorder is ever added it should replace this projection, not sit beside it.
//!
//! The projected row reports `speed_mbps` and `mtu` as zero, and `rx_errors`,
//! `tx_drops` and `collisions` as zero, because `net::interface` genuinely
//! tracks none of them — it has a transmit-error counter and a receive-drop
//! counter and no others. Those zeros are the one place in this file where zero
//! does not mean "measured zero"; if any of them is added to
//! `net::interface`, extend [`nic_row`] with it.

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Network interface type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicType {
    Ethernet,
    Wifi,
    Loopback,
    Bridge,
    Veth,
    Tun,
}

impl NicType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ethernet => "ethernet",
            Self::Wifi => "wifi",
            Self::Loopback => "loopback",
            Self::Bridge => "bridge",
            Self::Veth => "veth",
            Self::Tun => "tun",
        }
    }
}

/// Per-interface statistics.
#[derive(Debug, Clone)]
pub struct IfaceStats {
    pub name: String,
    pub nic_type: NicType,
    pub link_up: bool,
    pub speed_mbps: u32,
    pub mtu: u32,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_drops: u64,
    pub tx_drops: u64,
    pub collisions: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_IFACES: usize = 32;

struct State {
    ifaces: Vec<IfaceStats>,
    total_rx_bytes: u64,
    total_tx_bytes: u64,
    total_errors: u64,
    total_drops: u64,
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
// Projection of the kernel's own network stack
// ---------------------------------------------------------------------------

/// Name of the synthetic row that reports the NIC's counters from
/// [`crate::net::interface`].
///
/// Reserved: [`register_iface`] refuses it, because a recorded row with this
/// name would make the projected one ambiguous — a reader could not tell which
/// of the two sources a number came from. See the module docs for why it is
/// called `eth0` and what it was called before.
pub const NIC_IFACE: &str = "eth0";

/// Project the network stack's live counters as an [`IfaceStats`] row, or
/// `None` if the stack has not seen a single frame and the link is down.
///
/// Returning `None` on an all-zero snapshot is deliberate: a row of zeros in
/// `/proc/netdev` before the stack has done anything is exactly the "fabricated
/// data" this module's `init_defaults` was cleaned up to stop producing. A row
/// appears the moment there is something real to report — including a link that
/// is up but idle, which *is* a real thing to report.
///
/// **Call this before taking `STATE`, never while holding it.**
/// [`crate::net::interface::is_up`] takes that module's `IFACE` lock, so calling
/// it under `STATE` would establish a `STATE` → `IFACE` order. Nothing
/// establishes the reverse order today — `net::interface` does not call into
/// this module, which is the whole reason this projection exists — but the
/// readers below are written to avoid the nesting entirely rather than to rely
/// on that staying true. `scripts/check-recursive-locks.py` is the live gate.
fn nic_row() -> Option<IfaceStats> {
    let up = crate::net::interface::is_up();
    // The NIC's counter group specifically — `stats()` is defined as
    // `stats_for(Source::Nic)`. Projecting the cross-source total here would
    // put container-to-container frames in a row named after a device.
    let s = crate::net::interface::stats();
    if !up && s.is_zero() {
        return None;
    }
    Some(IfaceStats {
        name: String::from(NIC_IFACE),
        nic_type: NicType::Ethernet,
        link_up: up,
        // Not tracked by net::interface; see the module docs.
        speed_mbps: 0,
        mtu: 0,
        rx_bytes: s.rx_bytes,
        tx_bytes: s.tx_bytes,
        rx_packets: s.rx_packets,
        tx_packets: s.tx_packets,
        // net::interface counts transmit errors and receive drops only.
        rx_errors: 0,
        tx_errors: s.tx_errors,
        rx_drops: s.rx_drops,
        tx_drops: 0,
        collisions: 0,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise an **empty** network-device table.
///
/// Seeds NO interfaces and zero counters.  Real interfaces are wired through
/// [`register_iface`] (one row per NIC the network stack brings up) and their
/// counters through the `record_rx`/`record_tx`/`record_error`/`record_drop`
/// functions; until those are called the table is genuinely empty, so
/// `/proc/netdev` and the `netdev` kshell command report nothing rather than
/// fabricated numbers — the kernel's hard "never invent data in procfs" rule.
///
/// "Empty" here means empty of *recorded* rows.  The readers additionally
/// project the network stack's own counters as the [`NIC_IFACE`] row, which
/// is not stored in this table at all and so is unaffected by initialisation or
/// by the self-test's reset — see the module docs for why that is a projection
/// and not a call on the frame path.
///
/// NOTE: this previously seeded three fictional interfaces ("lo": loopback /
/// 1 GB rx+tx / 5M packets each; "eth0": 1 Gbps ethernet / 50 GB rx / 10 GB tx /
/// 100M rx packets / 50M tx packets / 500 rx errors / 100 tx errors / 1000 rx
/// drops / 200 tx drops / 50 collisions; "wlan0": idle wifi) plus invented
/// aggregate totals (total_rx_bytes 51 GB, total_tx_bytes 11 GB, total_errors
/// 600, total_drops 1200), which `/proc/netdev` (and the `list`/`get` views) then
/// displayed as if they were real measured NIC traffic.  That demo data was
/// removed; the self-test now builds its own fixtures explicitly via the real API
/// (see [`self_test`]).  A future per-NIC source is expected to call
/// [`register_iface`] when an interface comes up and the record functions on
/// every packet event; the single interface the kernel has today is reported by
/// projection instead, because its counters already exist.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        ifaces: Vec::new(),
        total_rx_bytes: 0,
        total_tx_bytes: 0,
        total_errors: 0,
        total_drops: 0,
        ops: 0,
    });
}

/// Register a network interface.
///
/// Creates a zeroed [`IfaceStats`] row with the supplied link parameters
/// (`nic_type`, `speed_mbps`, `mtu`); the link starts down with all traffic
/// counters at zero.  Duplicate interface names return
/// [`KernelError::AlreadyExists`]; exceeding [`MAX_IFACES`] returns
/// [`KernelError::ResourceExhausted`].  [`NIC_IFACE`] is reserved for the
/// projected network-stack row and is refused with
/// [`KernelError::AlreadyExists`] too, because two rows of that name would make
/// it impossible to tell projected traffic from recorded traffic.
pub fn register_iface(
    name: &str,
    nic_type: NicType,
    speed_mbps: u32,
    mtu: u32,
) -> KernelResult<()> {
    if name == NIC_IFACE {
        return Err(KernelError::AlreadyExists);
    }
    with_state(|state| {
        if state.ifaces.len() >= MAX_IFACES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.ifaces.iter().any(|d| d.name == name) {
            return Err(KernelError::AlreadyExists);
        }
        state.ifaces.push(IfaceStats {
            name: String::from(name),
            nic_type,
            link_up: false,
            speed_mbps,
            mtu,
            rx_bytes: 0,
            tx_bytes: 0,
            rx_packets: 0,
            tx_packets: 0,
            rx_errors: 0,
            tx_errors: 0,
            rx_drops: 0,
            tx_drops: 0,
            collisions: 0,
        });
        Ok(())
    })
}

/// Record received traffic.
pub fn record_rx(iface: &str, bytes: u64, packets: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state
            .ifaces
            .iter_mut()
            .find(|d| d.name == iface)
            .ok_or(KernelError::NotFound)?;
        dev.rx_bytes += bytes;
        dev.rx_packets += packets;
        state.total_rx_bytes += bytes;
        Ok(())
    })
}

/// Record transmitted traffic.
pub fn record_tx(iface: &str, bytes: u64, packets: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state
            .ifaces
            .iter_mut()
            .find(|d| d.name == iface)
            .ok_or(KernelError::NotFound)?;
        dev.tx_bytes += bytes;
        dev.tx_packets += packets;
        state.total_tx_bytes += bytes;
        Ok(())
    })
}

/// Record an error.
pub fn record_error(iface: &str, is_rx: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state
            .ifaces
            .iter_mut()
            .find(|d| d.name == iface)
            .ok_or(KernelError::NotFound)?;
        if is_rx {
            dev.rx_errors += 1;
        } else {
            dev.tx_errors += 1;
        }
        state.total_errors += 1;
        Ok(())
    })
}

/// Record a drop.
pub fn record_drop(iface: &str, is_rx: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state
            .ifaces
            .iter_mut()
            .find(|d| d.name == iface)
            .ok_or(KernelError::NotFound)?;
        if is_rx {
            dev.rx_drops += 1;
        } else {
            dev.tx_drops += 1;
        }
        state.total_drops += 1;
        Ok(())
    })
}

/// Set link state.
pub fn set_link_state(iface: &str, up: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state
            .ifaces
            .iter_mut()
            .find(|d| d.name == iface)
            .ok_or(KernelError::NotFound)?;
        dev.link_up = up;
        Ok(())
    })
}

/// List all interfaces.
///
/// Registered interfaces first, in registration order, then the projected
/// [`NIC_IFACE`] row if the network stack has come up or moved a frame.  The
/// projected row is appended rather than prepended so that a caller holding an
/// index into a previous result is not silently re-pointed at a different
/// interface when the NIC comes up.
pub fn list() -> Vec<IfaceStats> {
    // Sampled before the lock: see `nic_row`.
    let kernel = nic_row();
    let mut rows = STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.ifaces.clone());
    if let Some(k) = kernel {
        rows.push(k);
    }
    rows
}

/// Get specific interface.
///
/// Answers for [`NIC_IFACE`] from the projection, so a caller that saw the
/// row in [`list`] can look it up by name and get the same thing.
pub fn get(iface: &str) -> Option<IfaceStats> {
    if iface == NIC_IFACE {
        return nic_row();
    }
    STATE
        .lock()
        .as_ref()
        .and_then(|s| s.ifaces.iter().find(|d| d.name == iface).cloned())
}

/// Statistics: (iface_count, total_rx_bytes, total_tx_bytes, total_errors, total_drops, ops).
///
/// Byte totals, errors and drops combine registered interfaces with the
/// projected network stack, and `iface_count` counts the projected row when it
/// is present, so the tuple always describes exactly the rows [`list`] returns.
/// `ops` counts operations against *this* table and is deliberately not
/// inflated by projection — nothing "operated" on it.
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    // Sampled before the lock: see `nic_row`.
    let kernel = nic_row();
    let guard = STATE.lock();
    let (ifaces, rx, tx, errors, drops, ops) = match guard.as_ref() {
        Some(s) => (
            s.ifaces.len(),
            s.total_rx_bytes,
            s.total_tx_bytes,
            s.total_errors,
            s.total_drops,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0, 0),
    };
    match kernel {
        Some(k) => (
            ifaces.saturating_add(1),
            rx.saturating_add(k.rx_bytes),
            tx.saturating_add(k.tx_bytes),
            errors
                .saturating_add(k.rx_errors)
                .saturating_add(k.tx_errors),
            drops.saturating_add(k.rx_drops).saturating_add(k.tx_drops),
            ops,
        ),
        None => (ifaces, rx, tx, errors, drops, ops),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// The number of registered rows, with no projection counted.
///
/// The self-test used to assert `list().len()`, which was only safe while the
/// table could not contain anything the test had not itself put there.  Now
/// that a projected row may be present, that assertion would fail on any kernel
/// whose NIC came up — which is every kernel that has a NIC.
fn recorded_len() -> usize {
    STATE.lock().as_ref().map_or(0, |s| s.ifaces.len())
}

/// The registered halves of the totals, with no projection mixed in.
///
/// Exists so the self-test can assert exact numbers.  The public [`stats`] adds
/// the network stack, which is live and can advance between two calls — a
/// single arriving ARP reply would do it — so asserting equality against it
/// would be a flake waiting for a busy enough network.
fn recorded_totals() -> (usize, u64, u64, u64, u64) {
    STATE.lock().as_ref().map_or((0, 0, 0, 0, 0), |s| {
        (
            s.ifaces.len(),
            s.total_rx_bytes,
            s.total_tx_bytes,
            s.total_errors,
            s.total_drops,
        )
    })
}

/// Name of the *recorded* interface this self-test builds its fixtures on.
///
/// It must not be [`NIC_IFACE`], which is reserved for the projected row and
/// refused by [`register_iface`]. It was literally `"eth0"` until 2026-08-27,
/// when the projected row took that name over from `"kernel"` — and this rung
/// found the collision by panicking on the refusal at case 2, which is exactly
/// what a reserved name is supposed to do. Named for what it is instead: a
/// fixture, not a device anyone has.
const DEV: &str = "testnic0";

/// Compile-time guard on the line above.
///
/// If [`DEV`] is ever set to [`NIC_IFACE`], `register_iface` refuses it and
/// every run of this self-test panics at case 2 without having tested anything.
/// That is not hypothetical — it is exactly what happened on 2026-08-27 when the
/// projected row was renamed `kernel` → `eth0` while the fixture was still
/// called `"eth0"`, and the only thing that caught it was a kernel panic
/// eighteen minutes into a boot test. Two string constants in one file agreeing
/// by accident deserves better than that, so it is a build error here instead.
const _: () = assert!(
    !const_str_eq(DEV, NIC_IFACE),
    "netdev self-test fixture must not use the reserved NIC_IFACE name"
);

/// Byte-wise `&str` equality usable in a `const` context.
///
/// `str`'s `PartialEq` is not `const`, and this is only ever evaluated at
/// compile time, so the loop's cost is nil.
#[allow(
    clippy::indexing_slicing,
    reason = "const context: an out-of-range index is a compile error, not a runtime panic, \
              and the bound is checked immediately above"
)]
const fn const_str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

pub fn self_test() {
    crate::serial_println!("netdev::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/netdev must never surface).  Resetting
    // first clears any residue from a prior `netdev test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated interfaces or counters; record on an
    // unregistered iface fails.  Asserted against the RECORDED table, not the
    // public views: the projected `eth0` row is live and is present on any
    // kernel whose NIC came up, so `list()` and `stats()` are legitimately
    // non-empty here and always will be.
    assert_eq!(recorded_len(), 0);
    assert_eq!(recorded_totals(), (0, 0, 0, 0, 0));
    assert!(record_rx(DEV, 1, 1).is_err()); // no phantom iface exists yet
    // The reserved name is refused on an EMPTY table, which is what makes it a
    // reservation rather than ordinary duplicate detection: there is no row it
    // could be colliding with. Case 9 re-checks it with a row present.
    assert!(register_iface(NIC_IFACE, NicType::Ethernet, 1000, 1500).is_err());
    assert_eq!(recorded_len(), 0); // the refusal did not add a row
    crate::serial_println!("  [1/9] empty init: OK");

    // 2: Register — zeroed counters, link down, params preserved; dup fails.
    register_iface(DEV, NicType::Ethernet, 1000, 1500).expect("register");
    let d = get(DEV).expect("get");
    assert_eq!(d.nic_type, NicType::Ethernet);
    assert_eq!((d.speed_mbps, d.mtu), (1000, 1500));
    assert!(!d.link_up);
    assert_eq!((d.rx_bytes, d.tx_bytes, d.rx_packets), (0, 0, 0));
    assert!(register_iface(DEV, NicType::Ethernet, 1000, 1500).is_err());
    crate::serial_println!("  [2/9] register: OK");

    // 3: Record RX — bytes + packets accumulate; total_rx rises.
    record_rx(DEV, 1500, 1).expect("rx");
    record_rx(DEV, 500, 1).expect("rx2");
    let d = get(DEV).expect("get");
    assert_eq!(d.rx_bytes, 2000);
    assert_eq!(d.rx_packets, 2);
    crate::serial_println!("  [3/9] rx: OK");

    // 4: Record TX — independent counters.
    record_tx(DEV, 1000, 1).expect("tx");
    let d = get(DEV).expect("get");
    assert_eq!(d.tx_bytes, 1000);
    assert_eq!(d.tx_packets, 1);
    crate::serial_println!("  [4/9] tx: OK");

    // 5: Error/drop direction routing — rx vs tx counters update correctly.
    record_error(DEV, true).expect("rx error");
    record_error(DEV, false).expect("tx error");
    record_drop(DEV, true).expect("rx drop");
    record_drop(DEV, false).expect("tx drop");
    let d = get(DEV).expect("get");
    assert_eq!((d.rx_errors, d.tx_errors), (1, 1));
    assert_eq!((d.rx_drops, d.tx_drops), (1, 1));
    crate::serial_println!("  [5/9] error/drop: OK");

    // 6: Link state toggles.
    set_link_state(DEV, true).expect("link_up");
    assert!(get(DEV).expect("get").link_up);
    set_link_state(DEV, false).expect("link_down");
    assert!(!get(DEV).expect("get").link_up);
    crate::serial_println!("  [6/9] link state: OK");

    // 7: Unknown iface → NotFound on every record/link path.
    assert!(record_rx("fake", 0, 0).is_err());
    assert!(record_tx("fake", 0, 0).is_err());
    assert!(record_error("fake", true).is_err());
    assert!(record_drop("fake", true).is_err());
    assert!(set_link_state("fake", true).is_err());
    crate::serial_println!("  [7/9] not found: OK");

    // 8: Aggregate totals are exact: rx 2000, tx 1000, 2 errors, 2 drops.
    // Against the recorded halves, for the reason given on `recorded_totals`.
    assert_eq!(recorded_totals(), (1, 2000, 1000, 2, 2));
    let (_ifaces, rx, tx, errors, drops, ops) = stats();
    assert!(ops > 0);
    // The public view must never be *below* the recorded one: projection only
    // ever adds.  Deliberately `>=` and not `==` — the stack is live.
    assert!(rx >= 2000 && tx >= 1000 && errors >= 2 && drops >= 2);
    crate::serial_println!("  [8/9] stats: OK");

    // 9: The projected NIC row — the whole point of this module now, and
    // the rung that would have caught the defect it fixes.  The name is
    // reserved so a recorded row can never shadow it, and when the row is
    // present it must agree with its source rather than being a second,
    // separately-accumulated copy.
    assert!(register_iface(NIC_IFACE, NicType::Ethernet, 1000, 1500).is_err());
    assert_eq!(recorded_len(), 1); // the refusal did not add a row
    let before = crate::net::interface::stats();
    match get(NIC_IFACE) {
        Some(k) => {
            // Sampled after `before`, so it can only have advanced.  Comparing
            // with `>=` rather than `==` is the difference between a test and a
            // flake on a kernel that is answering ARP while this runs.
            assert!(k.rx_bytes >= before.rx_bytes);
            assert!(k.tx_bytes >= before.tx_bytes);
            assert!(k.rx_packets >= before.rx_packets);
            assert!(k.tx_packets >= before.tx_packets);
            assert!(list().iter().any(|i| i.name == NIC_IFACE));
            crate::serial_println!(
                "  [9/9] projection: OK (nic iface: rx {} B/{} pkt, tx {} B/{} pkt, link {})",
                k.rx_bytes,
                k.rx_packets,
                k.tx_bytes,
                k.tx_packets,
                if k.link_up { "up" } else { "down" }
            );
        }
        None => {
            // No NIC and not a frame moved — legitimate under QEMU with `-net
            // none`.  The row is absent rather than a row of zeros, which is
            // the behaviour `nic_row` exists to give.
            assert!(!list().iter().any(|i| i.name == NIC_IFACE));
            crate::serial_println!("  [9/9] projection: OK (no NIC, row absent)");
        }
    }

    // Leave the table EMPTY, not DEAD: clear the fixtures, then re-open it.
    // Clearing alone would switch this module off for the rest of the boot
    // -- `init_defaults` runs once, that once is here, and every later write
    // would take the `NotSupported` arm and be dropped by a caller that must
    // not let statistics fail a real operation.  known-issues.md:
    // A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT.
    //
    // /proc/netdev keeps reporting the `eth0` row across this reset: it is
    // projected from net::interface at read time, not stored here, so there is
    // nothing in it for the wipe to take away.
    *STATE.lock() = None;
    init_defaults();

    crate::serial_println!("netdev::self_test() — all 9 tests passed");
}

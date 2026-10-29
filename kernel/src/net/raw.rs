//! Raw Ethernet frame boundary for the userspace network stack.
//!
//! This is the foundation of the "move networking to userspace" migration
//! (design-decisions.md §63, Path B).  It exposes the physical NIC's layer-2
//! send/receive to a **single** privileged userspace process — the `netstack`
//! daemon — *without* moving the driver out of the kernel.  The kernel keeps
//! the thin virtio-net / e1000 driver; the daemon owns the protocol stack.
//!
//! ## Exclusive claim
//!
//! Raw access bypasses the entire in-kernel protocol stack and firewall, so it
//! is exclusive: one claim at a time, of one of two kinds ([`Card`]).  The
//! primary claim is the whole network: while it is held, [`is_claimed`]
//! returns `true` and [`crate::net::poll`] stops draining physical-NIC frames
//! itself (they belong to the daemon, delivered via [`receive`]).  A claim on
//! one card alone, named by its MAC address, takes that card only: the
//! kernel's stack keeps sending through its primary and reading the others.
//! Exactly one owner processes a card's frames at a time.
//!
//! The claim self-heals: if the owning process dies without releasing (crash,
//! kill), the next liveness check reclaims the NIC for the in-kernel stack so
//! connectivity is never permanently wedged by a dead daemon.
//!
//! ## Capability
//!
//! Opening a raw handle requires a [`crate::cap::ResourceType::NetRaw`]
//! capability with `WRITE` rights — strictly more privileged than an ordinary
//! `Socket`, since it grants unfiltered link-layer access.  Gating lives in the
//! syscall handler ([`crate::syscall`]); this module is the mechanism.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::{ProcessId, ProcessState};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Sentinel PID meaning "no owner".
const NO_OWNER: u64 = 0;

/// Whether a raw handle is currently claimed.
static CLAIMED: AtomicBool = AtomicBool::new(false);

/// The current raw owner and the card it holds, one word (meaningful only
/// while `CLAIMED` is `true`): the PID below bit [`CARD_SHIFT`], the
/// [`Card`] above -- so that a claim's owner and card change together.
static OWNER: AtomicU64 = AtomicU64::new(NO_OWNER);

/// Where [`OWNER`] keeps the card: above every PID.
const CARD_SHIFT: u32 = 56;

/// `pid` holding `card`, as [`OWNER`] keeps it.
fn packed(pid: ProcessId, card: Card) -> u64 {
    pid | u64::from(card as u8) << CARD_SHIFT
}

/// The PID [`OWNER`]'s `word` holds.
fn pid_of(word: u64) -> ProcessId {
    word & ((1 << CARD_SHIFT) - 1)
}

/// The card [`OWNER`]'s `word` holds.
#[allow(clippy::cast_possible_truncation)] // The top byte, shifted down.
fn card_of(word: u64) -> Card {
    Card::from_u8((word >> CARD_SHIFT) as u8)
}

/// Which card a raw claim holds. The primary is the classic claim: the card
/// the kernel sends through, and every card it reads, so that the kernel's
/// own stack reads none while it is held. Another card is claimed alone,
/// named by its MAC address: the kernel's stack keeps sending through its
/// primary and reading the others, and leaves that one to the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Card {
    Primary = 0,
    VirtioNet = 1,
    E1000 = 2,
    Rtl8139 = 3,
}

impl Card {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::VirtioNet,
            2 => Self::E1000,
            3 => Self::Rtl8139,
            _ => Self::Primary,
        }
    }

    /// The card whose MAC address is `mac`, `None` for none: the primary
    /// when it is the card the kernel sends through (the first present of
    /// virtio-net, e1000, rtl8139: `net::send_frame`'s order).
    #[must_use]
    pub fn with_mac(mac: [u8; 6]) -> Option<Self> {
        let cards = [
            (
                Self::VirtioNet,
                crate::virtio::net::with_device(|d| d.mac().0),
            ),
            (Self::E1000, crate::e1000::with_device(|d| d.mac().0)),
            (Self::Rtl8139, crate::rtl8139::with_device(|d| d.mac())),
        ];
        let primary = cards.iter().find(|(_, m)| m.is_some()).map(|&(c, _)| c);
        let card = cards
            .iter()
            .find(|(_, m)| *m == Some(mac))
            .map(|&(c, _)| c)?;
        Some(if Some(card) == primary {
            Self::Primary
        } else {
            card
        })
    }
}

/// Is a process considered dead (gone or zombied) for claim-ownership purposes?
fn owner_is_dead(pid: u64) -> bool {
    match crate::proc::pcb::state(pid) {
        None => true,                       // no such process
        Some(ProcessState::Zombie) => true, // exited, not yet reaped
        Some(_) => false,
    }
}

/// Force-release the claim (used when a stale/dead owner is detected).
fn release_stale() {
    OWNER.store(NO_OWNER, Ordering::Release);
    CLAIMED.store(false, Ordering::Release);
}

/// The card a **live** process holds a raw claim on, if any.
///
/// Cheap in the common (unclaimed) case — a single relaxed load — and only
/// touches the process table when a claim is actually held.
#[must_use]
pub fn claimed_card() -> Option<Card> {
    if !CLAIMED.load(Ordering::Acquire) {
        return None;
    }
    let cur = OWNER.load(Ordering::Acquire);
    if cur == NO_OWNER || owner_is_dead(pid_of(cur)) {
        // Owner died without releasing — hand the NIC back to the in-kernel
        // stack so connectivity resumes.
        release_stale();
        return None;
    }
    Some(card_of(cur))
}

/// True while a **live** process holds the primary claim ([`Card::Primary`]):
/// the network is its, not the kernel's stack's.
///
/// [`crate::net::poll`] consults this each poll to decide whether to drain the
/// physical NICs itself; while a card alone is claimed it drains the others
/// ([`claimed_card`]).
pub fn is_claimed() -> bool {
    claimed_card() == Some(Card::Primary)
}

/// PID of the current (live) raw owner, whichever card it holds, or `None`
/// if unclaimed.
#[must_use]
pub fn owner() -> Option<ProcessId> {
    claimed_card()?;
    let cur = pid_of(OWNER.load(Ordering::Acquire));
    (cur != NO_OWNER).then_some(cur)
}

/// Claim exclusive raw access to `card` for `pid`. One claim at a time,
/// whichever card it names.
///
/// Succeeds if the NIC is unclaimed, already owned by `pid` for the same
/// card (idempotent), or the current owner has died without releasing (the
/// stale claim is reclaimed).
///
/// # Errors
///
/// [`KernelError::DeviceBusy`] if a *different, live* process holds the
/// claim, or `pid` holds it for another card.
pub fn claim(pid: ProcessId, card: Card) -> KernelResult<()> {
    // Fast path: take an unclaimed NIC.
    if CLAIMED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        OWNER.store(packed(pid, card), Ordering::Release);
        return Ok(());
    }

    // Already claimed — inspect the owner.
    let cur = OWNER.load(Ordering::Acquire);
    if pid_of(cur) == pid {
        // An idempotent re-claim by the same process, of the same card.
        return if card_of(cur) == card {
            Ok(())
        } else {
            Err(KernelError::DeviceBusy)
        };
    }
    if cur == NO_OWNER || owner_is_dead(pid_of(cur)) {
        // Reclaim from a dead/absent owner without dropping `CLAIMED`.
        if OWNER
            .compare_exchange(cur, packed(pid, card), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ok(());
        }
        // Lost the race to another claimant; fall through to the live check.
        if OWNER.load(Ordering::Acquire) == packed(pid, card) {
            return Ok(());
        }
    }
    Err(KernelError::DeviceBusy)
}

/// Release a claim held by `pid`.
///
/// No-op (returns `Ok`) if `pid` is not the current owner — release is
/// idempotent and a non-owner releasing must not disturb the real owner.
///
/// # Errors
///
/// Never returns `Err` currently; the signature is `KernelResult` for
/// forward-compatibility with per-interface claims.
// `unnecessary_wraps`: deliberate, per the `# Errors` note above.  Releasing a
// *named interface* — rather than the single global claim this module has
// today — can fail (no such interface; not owned by `pid`), and every caller
// is already written against `KernelResult`.  Narrowing the return type to
// `()` now would mean re-widening it later and touching every call site, to no
// benefit in the interim.
#[allow(clippy::unnecessary_wraps)]
pub fn release(pid: ProcessId) -> KernelResult<()> {
    // Only the owner may clear the claim, whichever card it holds.
    let cur = OWNER.load(Ordering::Acquire);
    if cur != NO_OWNER
        && pid_of(cur) == pid
        && OWNER
            .compare_exchange(cur, NO_OWNER, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        CLAIMED.store(false, Ordering::Release);
        // The owner's multicast filter goes with its claim.
        super::mcast_filter::raw_released();
    }
    Ok(())
}

/// Transmit a raw Ethernet frame out the claimed card: the primary's way
/// (`net::send_frame`), or through the one card claimed.
///
/// The frame is sent verbatim — no protocol processing, no firewall.  Only the
/// raw owner should call this (enforced by the syscall capability gate).
///
/// # Errors
///
/// The card's driver's errors, and [`KernelError::NoSuchDevice`] when no card
/// is claimed or the claimed one is gone.
pub fn transmit(frame: &[u8]) -> KernelResult<()> {
    match claimed_card().ok_or(KernelError::NoSuchDevice)? {
        Card::Primary => super::send_frame(frame),
        Card::VirtioNet => {
            crate::virtio::net::with_device(|d| d.send(frame)).ok_or(KernelError::NoSuchDevice)?
        }
        Card::E1000 => {
            crate::e1000::with_device(|d| d.send(frame)).ok_or(KernelError::NoSuchDevice)?
        }
        Card::Rtl8139 => crate::rtl8139::send(frame),
    }
}

/// Receive one raw Ethernet frame from the claimed card (non-blocking).
///
/// Returns `Some(frame)` if a frame was pending, or `None` if the NIC queue is
/// empty.  Pulls directly from the driver; because [`claimed_card`] gates the
/// in-kernel drain in [`crate::net::poll`], the owner does not race the kernel
/// stack for frames.
#[must_use]
pub fn receive() -> Option<alloc::vec::Vec<u8>> {
    match claimed_card()? {
        Card::Primary => super::recv_frame_except(None),
        Card::VirtioNet => crate::virtio::net::with_device(|d| d.recv()).flatten(),
        Card::E1000 => crate::e1000::with_device(|d| d.recv()).flatten(),
        Card::Rtl8139 => crate::rtl8139::recv(),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------
//
// These checks were a `#[cfg(test)] mod tests`, which the kernel crate never
// compiles (`test = false`, no lib target — see `known-issues.md`
// → `A-KERNEL-UNIT-TESTS-NEVER-RUN`), so they had never run.  Lane B's
// `raced-globals.py` reported the two of them as racing on `CLAIMED`/`OWNER`;
// the interleaving it traced was real, but could not occur, because neither
// test executed.  As boot self-tests they are serialised by construction —
// boot self-tests run sequentially on one CPU — so the race is answered by
// moving them somewhere they run rather than by a mutex.
//
// Two things the unit-test versions got away with and this cannot:
//
//   * **The process table is real here.**  They hardcoded PID 4242 and relied
//     on `owner_is_dead` treating an unknown PID as dead.  That is an
//     assumption about live kernel state, so it is now checked.
//   * **These statics are live.**  Writing them is writing the actual NIC
//     claim, so the test refuses to run if one is held and restores the
//     prior state on every exit path.

/// Restore the module to the unclaimed state.
fn reset_claim_state() {
    OWNER.store(NO_OWNER, Ordering::SeqCst);
    CLAIMED.store(false, Ordering::SeqCst);
}

/// Find a PID with no process behind it, so `owner_is_dead` reports it dead.
///
/// Returns `None` if 64 consecutive candidates are all live, which would mean
/// the test cannot construct the state it needs.
fn find_absent_pid(start: u64) -> Option<ProcessId> {
    let mut pid = start;
    for _ in 0..64u32 {
        if crate::proc::pcb::state(pid).is_none() {
            return Some(pid);
        }
        pid = pid.saturating_add(1);
    }
    None
}

/// An unclaimed NIC reports itself unclaimed and ownerless.
fn check_unclaimed() -> KernelResult<()> {
    reset_claim_state();
    if is_claimed() {
        crate::serial_println!("[net-raw]   FAIL: unclaimed NIC reports claimed");
        return Err(KernelError::InternalError);
    }
    if let Some(pid) = owner() {
        crate::serial_println!("[net-raw]   FAIL: unclaimed NIC reports owner {pid}");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[net-raw]   unclaimed reports unclaimed: OK");
    Ok(())
}

/// A non-owner releasing must not disturb the real owner's claim.
fn check_release_by_non_owner(owner_pid: ProcessId, other_pid: ProcessId) -> KernelResult<()> {
    reset_claim_state();
    // Install the claim directly rather than via `claim()`: `owner_pid` names
    // no live process, so `claim()` is not the path under test here.
    OWNER.store(owner_pid, Ordering::SeqCst);
    CLAIMED.store(true, Ordering::SeqCst);

    // `release` cannot currently fail, and its result is irrelevant to the
    // property under test — that a non-owner's release is a no-op for the
    // owner's claim, which is asserted on `OWNER` below.
    let _ = release(other_pid);

    let after = OWNER.load(Ordering::SeqCst);
    if after != owner_pid {
        crate::serial_println!(
            "[net-raw]   FAIL: release by non-owner {other_pid} changed owner {owner_pid} -> {after}"
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[net-raw]   release by a non-owner is a no-op: OK");
    Ok(())
}

/// A claim held by a dead process is reclaimed for the in-kernel stack.
///
/// The module's headline promise — "the claim self-heals" — had no test at
/// all.  It is also the behaviour that makes `is_claimed()` a *writer* despite
/// its name, which is what made lane B's report subtle: the query is what
/// performs the repair.
fn check_dead_owner_self_heals(dead_pid: ProcessId) -> KernelResult<()> {
    reset_claim_state();
    OWNER.store(dead_pid, Ordering::SeqCst);
    CLAIMED.store(true, Ordering::SeqCst);

    if is_claimed() {
        crate::serial_println!(
            "[net-raw]   FAIL: claim by dead PID {dead_pid} still reported live"
        );
        return Err(KernelError::InternalError);
    }
    // The repair must be durable, not just a `false` return.
    if CLAIMED.load(Ordering::SeqCst) || OWNER.load(Ordering::SeqCst) != NO_OWNER {
        crate::serial_println!("[net-raw]   FAIL: stale claim reported dead but not cleared");
        return Err(KernelError::InternalError);
    }
    // And a fresh claim must now succeed.
    if claim(dead_pid, Card::Primary).is_err() {
        crate::serial_println!("[net-raw]   FAIL: NIC not reclaimable after self-heal");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[net-raw]   dead owner's claim self-heals: OK");
    Ok(())
}

/// A claim on one card alone, by a live process: the network stays the
/// kernel's stack's (`is_claimed` false), the claim names its card and its
/// owner, the owner cannot take another card while it holds one, and its
/// release clears the claim.
fn check_card_claim() -> KernelResult<()> {
    reset_claim_state();
    let pid = crate::proc::pcb::create("raw-card-claim", 0);
    let checked = (|| {
        claim(pid, Card::E1000).map_err(|_| "a claim on one card was refused")?;
        if is_claimed() {
            return Err("a claim on one card alone took the network from the kernel's stack");
        }
        if claimed_card() != Some(Card::E1000) || owner() != Some(pid) {
            return Err("a claim on one card did not name its card and owner");
        }
        if claim(pid, Card::Primary) != Err(KernelError::DeviceBusy) {
            return Err("an owner of one card took another while holding it");
        }
        // `release` cannot fail (its doc); what is checked is the state after.
        let _ = release(pid);
        if claimed_card().is_some() || owner().is_some() {
            return Err("a released claim on one card is still held");
        }
        Ok(())
    })();
    reset_claim_state();
    crate::proc::pcb::destroy(pid);
    if let Err(msg) = checked {
        crate::serial_println!("[net-raw]   FAIL: {}", msg);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[net-raw]   a claim on one card leaves the network the kernel's: OK");
    Ok(())
}

/// [`Card::with_mac`]: an address no card has names none; with a virtio-net
/// and an e1000 side by side -- the boot test's pair -- the virtio-net's names
/// the primary, the card the kernel sends through, and the e1000's that card
/// alone.
fn check_card_by_mac() -> KernelResult<()> {
    let wrong = if Card::with_mac([0xff; 6]).is_some() {
        Some("the broadcast address named a card")
    } else {
        let virtio = crate::virtio::net::with_device(|d| d.mac().0);
        let e1000 = crate::e1000::with_device(|d| d.mac().0);
        let (Some(virtio), Some(e1000)) = (virtio, e1000) else {
            crate::serial_println!(
                "[net-raw]   a card named by its MAC address: SKIPPED (no virtio-net and e1000 side by side)"
            );
            return Ok(());
        };
        (Card::with_mac(virtio) != Some(Card::Primary)
            || Card::with_mac(e1000) != Some(Card::E1000))
        .then_some("a card named by its MAC address was taken for another")
    };
    if let Some(msg) = wrong {
        crate::serial_println!("[net-raw]   FAIL: {}", msg);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[net-raw]   a card named by its MAC address is that card: OK");
    Ok(())
}

/// While a live process holds the e1000 alone, the kernel's stack polling
/// leaves that card's frames to it: an ARP request for the kernel's gateway
/// goes out of the claimed card -- QEMU's user networks each answer for the
/// same gateway address -- the kernel's stack is polled for a while, and the
/// claim must then read the answer. Skipped without a virtio-net (the
/// primary) beside an e1000, or without a gateway.
fn check_claimed_card_left_alone() -> KernelResult<()> {
    use crate::net::interface::Ipv4Addr;
    let gateway = crate::net::interface::info().gateway;
    let mac = crate::e1000::with_device(|d| d.mac());
    let primary = crate::virtio::net::with_device(|_| ()).is_some();
    let (Some(mac), true, false) = (mac, primary, gateway.is_unspecified()) else {
        crate::serial_println!(
            "[net-raw]   a card claimed alone, its frames left to it: SKIPPED (no virtio-net and e1000 side by side, or no gateway)"
        );
        return Ok(());
    };
    reset_claim_state();
    let pid = crate::proc::pcb::create("raw-card-alone", 0);
    let checked = (|| {
        claim(pid, Card::E1000).map_err(|_| "a claim on the e1000 alone was refused")?;
        // What the card holds already is not this check's.
        while receive().is_some() {}
        let ask = crate::net::arp::request_frame(&mac, Ipv4Addr::UNSPECIFIED, gateway);
        transmit(&ask).map_err(|_| "the e1000 claimed alone could not send")?;
        let until = crate::hrtimer::now_ns().saturating_add(100_000_000);
        while crate::hrtimer::now_ns() < until {
            crate::net::poll();
            core::hint::spin_loop();
        }
        core::iter::from_fn(receive)
            .any(|frame| is_arp_answer(&frame, &mac.0, gateway))
            .then_some(())
            .ok_or("the gateway's answer to the e1000 claimed alone did not reach its claim")
    })();
    reset_claim_state();
    crate::proc::pcb::destroy(pid);
    if let Err(msg) = checked {
        crate::serial_println!("[net-raw]   FAIL: {}", msg);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[net-raw]   a card claimed alone, its frames left to it by the kernel's stack: OK"
    );
    Ok(())
}

/// Whether `frame` is an ARP answer to `mac` from `ip`.
fn is_arp_answer(frame: &[u8], mac: &[u8; 6], ip: crate::net::interface::Ipv4Addr) -> bool {
    const ETHERTYPE_ARP: [u8; 2] = [0x08, 0x06];
    const ARP_REPLY: [u8; 2] = [0, 2];
    frame.get(..6) == Some(mac.as_slice())
        && frame.get(12..14) == Some(ETHERTYPE_ARP.as_slice())
        && frame.get(20..22) == Some(ARP_REPLY.as_slice())
        && frame.get(28..32) == Some(ip.0.as_slice())
}

/// Boot self-test for the exclusive-claim state machine.
///
/// Skips itself if a raw claim is live: these checks write `CLAIMED`/`OWNER`
/// directly, and clobbering the `netstack` daemon's claim would hand the NIC
/// back to the in-kernel stack behind its back.  In boot order nothing has
/// claimed yet, so the guard is against a future reordering rather than an
/// expected case.
///
/// # Errors
/// [`KernelError::InternalError`] if any claim transition misbehaves; the
/// failing case is logged first.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[net-raw] Running self-test...");

    if CLAIMED.load(Ordering::Acquire) {
        crate::serial_println!("[net-raw]   SKIPPED: a raw claim is already held");
        return Ok(());
    }

    // Two PIDs that name no process, so `owner_is_dead` reports both dead.
    let Some(absent) = find_absent_pid(4242) else {
        crate::serial_println!("[net-raw]   SKIPPED: no absent PID available");
        return Ok(());
    };
    let Some(other) = find_absent_pid(absent.saturating_add(1)) else {
        crate::serial_println!("[net-raw]   SKIPPED: no second absent PID available");
        return Ok(());
    };

    let result = check_unclaimed()
        .and_then(|()| check_release_by_non_owner(absent, other))
        .and_then(|()| check_dead_owner_self_heals(absent))
        .and_then(|()| check_card_claim())
        .and_then(|()| check_card_by_mac())
        .and_then(|()| check_claimed_card_left_alone());

    // Leave the NIC unclaimed however we exited: a self-test that fails
    // half-way must not leave the in-kernel stack locked out of the NIC.
    reset_claim_state();

    result?;
    crate::serial_println!("[net-raw] Self-test PASSED");
    Ok(())
}

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
//! One claim at a time is what its users need: the netstack daemon's primary
//! claim, and `nic-bench`'s claim on the e1000 alone, which runs before the
//! daemon starts. A claim per card -- and with it a multicast filter per
//! card, where a card's owner now gets the kernel's set -- waits for two
//! owners that must hold cards at once.
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
use core::sync::atomic::{AtomicU64, Ordering};

/// The claim, one word: [`NO_CLAIM`], or the owner's PID below bit
/// [`CARD_SHIFT`] and its [`Card`] above. One word, so that a claim is
/// installed, taken over from a dead owner and cleared each by a single
/// compare-and-swap, and no reader ever sees an owner without its card or a
/// claim half made.
static CLAIM: AtomicU64 = AtomicU64::new(NO_CLAIM);

/// [`CLAIM`] when nothing is claimed. PID 0 is the kernel's, never an owner.
const NO_CLAIM: u64 = 0;

/// Where [`CLAIM`] keeps the card: above every PID.
const CARD_SHIFT: u32 = 56;

/// `pid` holding `card`, as [`CLAIM`] keeps it.
fn packed(pid: ProcessId, card: Card) -> u64 {
    pid | u64::from(card as u8) << CARD_SHIFT
}

/// The PID [`CLAIM`]'s `word` holds.
fn pid_of(word: u64) -> ProcessId {
    word & ((1 << CARD_SHIFT) - 1)
}

/// The card [`CLAIM`]'s `word` holds; `None` for bits no card has, which
/// [`live_claim`] then reads as no claim at all -- the kernel's stack keeping
/// the network -- rather than as some card's.
fn card_of(word: u64) -> Option<Card> {
    let bits = word >> CARD_SHIFT;
    Card::ALL.into_iter().find(|&c| u64::from(c as u8) == bits)
}

/// Which card a raw claim holds. The primary is the classic claim: the card
/// the kernel sends through, and every card it reads, so that the kernel's
/// own stack reads none while it is held. A card other than the primary is
/// claimed alone, named by its MAC address ([`Card::alone_with_mac`]): the
/// kernel's stack keeps sending through its primary and reading the other
/// cards, and leaves that one to the owner. Virtio-net, the first card
/// `net::send_frame` tries, is the primary whenever it is present, so it is
/// never claimed alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Card {
    Primary = 0,
    E1000 = 1,
    Rtl8139 = 2,
}

impl Card {
    /// Every card, for decoding [`CLAIM`]. A card left out of it reads back
    /// as no claim ([`card_of`]), and the raw NIC self-test fails on it.
    const ALL: [Self; 3] = [Self::Primary, Self::E1000, Self::Rtl8139];

    /// The card whose MAC address is `mac`, to be claimed alone.
    ///
    /// # Errors
    ///
    /// [`KernelError::NotFound`] for a MAC address no card of the kernel's
    /// has; [`KernelError::InvalidArgument`] for the primary's, the card the
    /// kernel's stack sends through (the first present of virtio-net, e1000,
    /// rtl8139: `net::send_frame`'s order), which is claimed as the primary
    /// or not at all.
    pub fn alone_with_mac(mac: [u8; 6]) -> KernelResult<Self> {
        let virtio = crate::virtio::net::with_device(|d| d.mac().0);
        let e1000 = crate::e1000::with_device(|d| d.mac().0);
        let rtl8139 = crate::rtl8139::with_device(|d| d.mac());
        if virtio.or(e1000).or(rtl8139) == Some(mac) {
            return Err(KernelError::InvalidArgument);
        }
        if e1000 == Some(mac) {
            Ok(Self::E1000)
        } else if rtl8139 == Some(mac) {
            Ok(Self::Rtl8139)
        } else {
            Err(KernelError::NotFound)
        }
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

/// The claim a **live** process holds, if any: its owner and its card.
///
/// Cheap in the common (unclaimed) case -- a single load -- and only touches
/// the process table when a claim is held. A claim whose owner died without
/// releasing is cleared here, so the kernel's stack has the network back:
/// that exact claim only, by compare-and-swap, so a claim made meanwhile by
/// another process is never the one cleared.
#[must_use]
pub fn live_claim() -> Option<(ProcessId, Card)> {
    let cur = CLAIM.load(Ordering::Acquire);
    if cur == NO_CLAIM {
        return None;
    }
    let pid = pid_of(cur);
    if owner_is_dead(pid) {
        // Failing means the claim changed under us -- released, or taken over
        // from the dead owner by a live one -- which is no longer this one to
        // clear.
        let _ = CLAIM.compare_exchange(cur, NO_CLAIM, Ordering::AcqRel, Ordering::Acquire);
        return None;
    }
    Some((pid, card_of(cur)?))
}

/// The card a live process holds a raw claim on, if any ([`live_claim`]).
#[must_use]
pub fn claimed_card() -> Option<Card> {
    live_claim().map(|(_, card)| card)
}

/// True while a **live** process holds the primary claim ([`Card::Primary`]):
/// the network is its, not the kernel's stack's.
///
/// [`crate::net::poll`] consults the claim each poll to decide which physical
/// NICs to drain itself: none under the primary claim, every one but the
/// claimed card under a claim on a card alone ([`claimed_card`]).
pub fn is_claimed() -> bool {
    claimed_card() == Some(Card::Primary)
}

/// PID of the current (live) raw owner, whichever card it holds, or `None`
/// if unclaimed.
#[must_use]
pub fn owner() -> Option<ProcessId> {
    live_claim().map(|(pid, _)| pid)
}

/// Claim exclusive raw access to `card` for `pid`. One claim at a time,
/// whichever card it names.
///
/// Succeeds if nothing is claimed, `pid` already holds `card` (idempotent),
/// or the holder has died without releasing (its claim is taken over).
///
/// # Errors
///
/// [`KernelError::DeviceBusy`] if a *different, live* process holds the
/// claim, or `pid` holds it for another card; [`KernelError::InvalidArgument`]
/// for a PID the claim cannot hold -- 0, which would read as no claim, or one
/// reaching the card's bits, which would read as another owner's.
pub fn claim(pid: ProcessId, card: Card) -> KernelResult<()> {
    if pid == NO_CLAIM || pid_of(pid) != pid {
        return Err(KernelError::InvalidArgument);
    }
    let want = packed(pid, card);
    // Each lost race means the claim changed: it is looked at again. A few
    // rounds settle it; past that, someone else keeps winning, which is busy.
    for _ in 0..4 {
        let cur = CLAIM.load(Ordering::Acquire);
        if cur == want {
            return Ok(());
        }
        if cur != NO_CLAIM && !owner_is_dead(pid_of(cur)) {
            return Err(KernelError::DeviceBusy);
        }
        if CLAIM
            .compare_exchange(cur, want, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
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
    let cur = CLAIM.load(Ordering::Acquire);
    if cur != NO_CLAIM
        && pid_of(cur) == pid
        && CLAIM
            .compare_exchange(cur, NO_CLAIM, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        && card_of(cur) == Some(Card::Primary)
    {
        // The owner's multicast filter goes with the primary claim, the only
        // one that can set it.
        super::mcast_filter::raw_released();
    }
    Ok(())
}

/// Transmit a raw Ethernet frame out `card`, the claim's ([`live_claim`]):
/// the primary's way (`net::send_frame`), or through the one card claimed.
///
/// The frame is sent verbatim — no protocol processing, no firewall.  Only the
/// raw owner should call this (enforced by the syscall capability gate).
///
/// # Errors
///
/// The card's driver's errors; [`KernelError::NoSuchDevice`] when the e1000
/// claimed is gone, the rtl8139's [`KernelError::NotFound`] when it is.
pub fn transmit(card: Card, frame: &[u8]) -> KernelResult<()> {
    match card {
        Card::Primary => super::send_frame(frame),
        Card::E1000 => {
            crate::e1000::with_device(|d| d.send(frame)).ok_or(KernelError::NoSuchDevice)?
        }
        Card::Rtl8139 => crate::rtl8139::send(frame),
    }
}

/// Receive one raw Ethernet frame from `card`, the claim's ([`live_claim`]),
/// without waiting.
///
/// Returns `Some(frame)` if a frame was pending, or `None` if the NIC queue is
/// empty.  Pulls directly from the driver; because the claim gates the
/// in-kernel drain in [`crate::net::poll`], the owner does not race the kernel
/// stack for frames.
#[must_use]
pub fn receive(card: Card) -> Option<alloc::vec::Vec<u8>> {
    match card {
        Card::Primary => super::recv_frame_except(None),
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
// → `A-KERNEL-UNIT-TESTS-NEVER-RUN`), so they had never run.  As boot
// self-tests they are serialised by construction — boot self-tests run
// sequentially on one CPU.
//
// Two things the unit-test versions got away with and this cannot:
//
//   * **The process table is real here.**  They hardcoded PID 4242 and relied
//     on `owner_is_dead` treating an unknown PID as dead.  That is an
//     assumption about live kernel state, so it is now checked.
//   * **The claim is live.**  Writing `CLAIM` is writing the actual NIC
//     claim, so the test refuses to run if one is held and restores the
//     unclaimed state on every exit path.

/// Restore the module to the unclaimed state.
fn reset_claim_state() {
    CLAIM.store(NO_CLAIM, Ordering::SeqCst);
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

/// `check`, run with a live scratch process named `name` as its argument:
/// the claim left unclaimed and the process gone after, however it went. A
/// failure is logged as `FAIL: <its message>`.
fn with_live_pid(
    name: &'static str,
    check: impl FnOnce(ProcessId) -> Result<(), &'static str>,
) -> KernelResult<()> {
    reset_claim_state();
    let pid = crate::proc::pcb::create(name, 0);
    let checked = check(pid);
    reset_claim_state();
    crate::proc::pcb::destroy(pid);
    checked.map_err(|msg| {
        crate::serial_println!("[net-raw]   FAIL: {}", msg);
        KernelError::InternalError
    })
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
    let held = packed(owner_pid, Card::Primary);
    CLAIM.store(held, Ordering::SeqCst);

    // `release` cannot currently fail, and its result is irrelevant to the
    // property under test — that a non-owner's release is a no-op for the
    // owner's claim, which is asserted on `CLAIM` below.
    let _ = release(other_pid);

    let after = CLAIM.load(Ordering::SeqCst);
    if after != held {
        crate::serial_println!(
            "[net-raw]   FAIL: release by non-owner {other_pid} changed the claim {held:#x} -> {after:#x}"
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[net-raw]   release by a non-owner is a no-op: OK");
    Ok(())
}

/// A claim held by a dead process is reclaimed for the in-kernel stack.
///
/// The module's headline promise — "the claim self-heals".  It is also the
/// behaviour that makes `is_claimed()` a *writer* despite its name: the query
/// is what performs the repair.
fn check_dead_owner_self_heals(dead_pid: ProcessId) -> KernelResult<()> {
    reset_claim_state();
    CLAIM.store(packed(dead_pid, Card::Primary), Ordering::SeqCst);

    if is_claimed() {
        crate::serial_println!(
            "[net-raw]   FAIL: claim by dead PID {dead_pid} still reported live"
        );
        return Err(KernelError::InternalError);
    }
    // The repair must be durable, not just a `false` return.
    if CLAIM.load(Ordering::SeqCst) != NO_CLAIM {
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

/// A live process claiming over a dead owner's claim, before anything has
/// noticed the death, takes it over -- card and all, in the one word.
fn check_takeover_from_dead(dead_pid: ProcessId) -> KernelResult<()> {
    with_live_pid("raw-takeover", |pid| {
        CLAIM.store(packed(dead_pid, Card::E1000), Ordering::SeqCst);
        claim(pid, Card::Primary).map_err(|_| "a live claim over a dead owner's was refused")?;
        if live_claim() != Some((pid, Card::Primary)) {
            return Err("a claim taken over from a dead owner kept the dead owner's card");
        }
        crate::serial_println!("[net-raw]   a dead owner's claim is taken over whole: OK");
        Ok(())
    })
}

/// A claim on one card alone, by a live process: the network stays the
/// kernel's stack's (`is_claimed` false), the claim names its card and its
/// owner, the owner cannot take another card while it holds one, and its
/// release clears the claim.
fn check_card_claim() -> KernelResult<()> {
    with_live_pid("raw-card-claim", |pid| {
        claim(pid, Card::E1000).map_err(|_| "a claim on one card was refused")?;
        if is_claimed() {
            return Err("a claim on one card alone took the network from the kernel's stack");
        }
        if live_claim() != Some((pid, Card::E1000)) {
            return Err("a claim on one card did not name its card and owner");
        }
        if claim(pid, Card::Primary) != Err(KernelError::DeviceBusy) {
            return Err("an owner of one card took another while holding it");
        }
        // `release` cannot fail (its doc); what is checked is the state after.
        let _ = release(pid);
        if live_claim().is_some() {
            return Err("a released claim on one card is still held");
        }
        crate::serial_println!(
            "[net-raw]   a claim on one card leaves the network the kernel's: OK"
        );
        Ok(())
    })
}

/// Every card reads back from [`CLAIM`] as itself, and bits no card has
/// read back as no claim -- the network the kernel's -- not as some card's.
fn check_card_codes() -> KernelResult<()> {
    with_live_pid("raw-card-codes", |pid| {
        for card in Card::ALL {
            CLAIM.store(packed(pid, card), Ordering::SeqCst);
            if live_claim() != Some((pid, card)) {
                return Err("a card did not read back from the claim as itself");
            }
        }
        CLAIM.store(pid | 0xEE << CARD_SHIFT, Ordering::SeqCst);
        if live_claim().is_some() || is_claimed() {
            return Err("bits no card has were read as a card's claim");
        }
        reset_claim_state();
        let unholdable = [(NO_CLAIM, Card::Primary), (1 << CARD_SHIFT, Card::E1000)];
        if unholdable
            .into_iter()
            .any(|(p, card)| claim(p, card) != Err(KernelError::InvalidArgument))
        {
            return Err("a PID the claim cannot hold was given a claim");
        }
        crate::serial_println!(
            "[net-raw]   every card reads back as itself, no card as none, an unholdable PID refused: OK"
        );
        Ok(())
    })
}

/// [`Card::alone_with_mac`]: an address no card has names none; with a
/// virtio-net and an e1000 side by side -- the boot test's pair -- the
/// virtio-net, the primary, is refused, and the e1000's names that card.
fn check_card_by_mac() -> KernelResult<()> {
    let wrong = if Card::alone_with_mac([0xff; 6]) != Err(KernelError::NotFound) {
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
        if Card::alone_with_mac(virtio) != Err(KernelError::InvalidArgument) {
            Some("the primary card was given to be claimed alone")
        } else if Card::alone_with_mac(e1000) != Ok(Card::E1000) {
            Some("a card named by its MAC address was taken for another")
        } else {
            None
        }
    };
    if let Some(msg) = wrong {
        crate::serial_println!("[net-raw]   FAIL: {}", msg);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[net-raw]   a card named by its MAC address is that card, the primary refused: OK"
    );
    Ok(())
}

/// While a live process holds the e1000 alone, the kernel's stack polling
/// leaves that card's frames to it: every frame the card hands out while the
/// kernel's stack is polled must reach the claim. To have frames come, an ARP
/// request for the kernel's gateway goes out of the claimed card -- under
/// QEMU's user networks each answers for the same address. Where the card's
/// network answers nothing there is nothing to check, and the check skips.
/// Skipped too without a virtio-net (the primary) beside an e1000, or without
/// a gateway.
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
    let handed_out = || crate::e1000::with_device(|d| d.frames_received()).unwrap_or(0);
    let mut nothing_came = false;
    with_live_pid("raw-card-alone", |pid| {
        claim(pid, Card::E1000).map_err(|_| "a claim on the e1000 alone was refused")?;
        // What the card holds already is not this check's.
        while receive(Card::E1000).is_some() {}
        let before = handed_out();
        let ask = crate::net::arp::request_frame(&mac, Ipv4Addr::UNSPECIFIED, gateway);
        transmit(Card::E1000, &ask).map_err(|_| "the e1000 claimed alone could not send")?;
        let until = crate::hrtimer::now_ns().saturating_add(100_000_000);
        while crate::hrtimer::now_ns() < until {
            crate::net::poll();
            core::hint::spin_loop();
        }
        let reached = core::iter::from_fn(|| receive(Card::E1000)).count() as u64;
        let came = handed_out().wrapping_sub(before);
        nothing_came = came == 0;
        if reached == came {
            Ok(())
        } else {
            Err("the kernel's stack took frames from the e1000 claimed alone")
        }
    })?;
    if nothing_came {
        crate::serial_println!(
            "[net-raw]   a card claimed alone, its frames left to it: SKIPPED (nothing came to the e1000: its network did not answer)"
        );
    } else {
        crate::serial_println!(
            "[net-raw]   a card claimed alone, its frames left to it by the kernel's stack: OK"
        );
    }
    Ok(())
}

/// Boot self-test for the exclusive-claim state machine.
///
/// Skips itself if a raw claim is live: these checks write `CLAIM` directly,
/// and clobbering the `netstack` daemon's claim would hand the NIC back to the
/// in-kernel stack behind its back.  In boot order nothing has claimed yet, so
/// the guard is against a future reordering rather than an expected case.
///
/// # Errors
/// [`KernelError::InternalError`] if any claim transition misbehaves; the
/// failing case is logged first.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[net-raw] Running self-test...");

    if CLAIM.load(Ordering::Acquire) != NO_CLAIM {
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
        .and_then(|()| check_takeover_from_dead(absent))
        .and_then(|()| check_card_claim())
        .and_then(|()| check_card_codes())
        .and_then(|()| check_card_by_mac())
        .and_then(|()| check_claimed_card_left_alone());

    // Leave the NIC unclaimed however we exited: a self-test that fails
    // half-way must not leave the in-kernel stack locked out of the NIC.
    reset_claim_state();

    result?;
    crate::serial_println!("[net-raw] Self-test PASSED");
    Ok(())
}

//! A network card a driver process serves (design-decisions 1589), as the
//! daemon's NIC: the service `nic.<name>` connected to, the ring it is
//! given in shared memory attached, and frames sent and received through
//! it. With a card attached, [`super::raw_tx`] and [`super::raw_rx`] come
//! here instead of to the kernel's raw-frame calls.
//!
//! The ring's slots are the daemon's: the first half posted to receive
//! into, each posted again once its frame is read; the second half to send
//! from, each free again once its send completes. A submission's token is
//! its slot.
//!
//! Doorbells (a byte on the connection) go only to a side that sleeps
//! (`netring`'s sleep words): the daemon says in the ring when it sleeps
//! waiting for completions, and rings the driver after submitting a frame
//! to send only when the driver said it sleeps. A slot posted to receive
//! into needs no doorbell: the driver looks for slots when a frame comes.
//!
//! Single-threaded daemon: the card is a `static mut` no borrow of which
//! outlives a call here, as [`super::loopback`]'s state is.

use netipc::nicport::{DOORBELL, Port, SLOT_SIZE};
use netipc::ring::{ERR_INVALID, ERR_MSG_SIZE, OP_RECV, OP_SEND, Sqe};
use netring::Ring;

use super::{
    E_WOULD_BLOCK, SHM_MAP_RW, SYS_CHANNEL_CLOSE, SYS_CHANNEL_RECV_TIMEOUT, SYS_CHANNEL_SEND,
    SYS_SHM_MAP, now_ns, sleep_ns, syscall1, syscall2, syscall3, syscall4,
};

const SYS_CHANNEL_TRY_RECV: u64 = 203;
const SYS_SHM_SIZE: u64 = 231;
const SYS_SERVICE_CONNECT: u64 = 281;
const SYS_SHM_GRANT: u64 = 1190;
/// SlateOS's `NotFound` (no such service yet) and `ChannelClosed` (the
/// driver has gone).
const NOT_FOUND: i64 = -500;
const CHANNEL_CLOSED: i64 = -300;
/// What `send` answers once the card has gone: `ENETDOWN`.
const NET_DOWN: i64 = -100;
/// The most slots a card's ring may have: the daemon's tables' size.
const MAX_SLOTS: usize = 256;
/// How long a send waits for a send slot to come free: 100 ms.
const SLOT_WAIT_NS: u64 = 100_000_000;
/// The most doorbells one wait takes after the first.
const DOORBELLS_A_WAIT: usize = 64;
/// An Ethernet header and a VLAN tag: what a frame carries besides its MTU.
const ETH_HLEN: usize = 14;
const VLAN_HLEN: usize = 4;

struct Card {
    channel: u64,
    ring: Ring,
    port: Port,
    /// Slots `0..rx` are for receiving, `rx..port.slots` for sending.
    rx: u32,
    /// Send slots free, a stack: the first `nfree`.
    free: [u32; MAX_SLOTS],
    nfree: usize,
    /// Frames received and not yet read, `(slot, length)`, in the order
    /// they came: `got_len` of them from `got_head`.
    got: [(u32, u32); MAX_SLOTS],
    got_head: usize,
    got_len: usize,
    /// Sends the driver completed with an error.
    send_errors: u64,
    /// The daemon's own sleep number, new for each wait.
    slept: u32,
    /// The driver's sleep last rung for.
    rung: u32,
    gone: bool,
}

static mut CARD: Option<Card> = None;

/// The card, if one is attached.
fn card() -> Option<&'static mut Card> {
    // SAFETY: single-threaded daemon; every caller takes it once and holds
    // it only for the call (the module's documentation).
    unsafe { (*core::ptr::addr_of_mut!(CARD)).as_mut() }
}

/// Why attaching to a card failed.
pub struct AttachError {
    /// What went wrong, for the log.
    pub why: &'static str,
    /// The failed system call's answer, or 0 where none failed.
    pub answer: i64,
    /// The driver closed the connection before attaching it: it turns away
    /// a process without the card's key.
    pub turned_away: bool,
}

impl AttachError {
    fn new(why: &'static str, answer: i64) -> Self {
        Self {
            why,
            answer,
            turned_away: false,
        }
    }
}

/// Attach to the card served as `service` (`nic.<name>`), waiting up to
/// `within_ns` for its driver to register it and answer: the card's
/// description, or why it could not be attached.
pub fn attach(service: &[u8], within_ns: u64) -> Result<Port, AttachError> {
    let deadline = now_ns().saturating_add(within_ns);
    let channel = loop {
        let got = syscall2(
            SYS_SERVICE_CONNECT,
            service.as_ptr() as u64,
            service.len() as u64,
        );
        if let Ok(channel) = u64::try_from(got) {
            break channel;
        }
        if got != NOT_FOUND || now_ns() >= deadline {
            return Err(AttachError::new(
                "could not connect to the card's service",
                got,
            ));
        }
        sleep_ns(20_000_000);
    };
    match ring_for(channel, deadline) {
        Ok(c) => {
            let port = c.port;
            // SAFETY: as in `card`; nothing holds the old value.
            unsafe { *core::ptr::addr_of_mut!(CARD) = Some(c) };
            if let Some(c) = card() {
                for slot in 0..c.rx {
                    post(c, slot);
                }
            }
            Ok(port)
        }
        Err(failed) => {
            syscall1(SYS_CHANNEL_CLOSE, channel);
            Err(failed)
        }
    }
}

/// The driver's attach message on `channel`, before `deadline`, and the
/// ring it names mapped and attached.
fn ring_for(channel: u64, deadline: u64) -> Result<Card, AttachError> {
    let mut message = [0u8; 64];
    let wait = deadline.saturating_sub(now_ns()).max(1);
    let got = syscall4(
        SYS_CHANNEL_RECV_TIMEOUT,
        channel,
        message.as_mut_ptr() as u64,
        message.len() as u64,
        wait,
    );
    if got == CHANNEL_CLOSED {
        return Err(AttachError {
            why: "the card's driver closed the connection without attaching it",
            answer: got,
            turned_away: true,
        });
    }
    let Ok(len) = usize::try_from(got) else {
        return Err(AttachError::new(
            "no attach message from the card's driver",
            got,
        ));
    };
    let port = Port::decode(message.get(..len).unwrap_or(&[]))
        .ok_or_else(|| AttachError::new("a malformed attach message", 0))?;
    if port.slots < 2 || port.slots as usize > MAX_SLOTS {
        return Err(AttachError::new(
            "an attach message with a slot count the daemon cannot use",
            0,
        ));
    }
    let mapped = syscall2(SYS_SHM_MAP, port.region, SHM_MAP_RW);
    let Ok(at) = u64::try_from(mapped) else {
        return Err(AttachError::new(
            "the card's ring could not be mapped",
            mapped,
        ));
    };
    let sized = syscall1(SYS_SHM_SIZE, port.region);
    let Ok(size) = usize::try_from(sized) else {
        return Err(AttachError::new(
            "the card's ring's size could not be asked for",
            sized,
        ));
    };
    // SAFETY: the region is mapped at `at`, `size` bytes, page-aligned, for
    // the rest of the daemon's life (it is never unmapped).
    let ring = unsafe { Ring::attach(at as *mut u8, size) }
        .ok_or_else(|| AttachError::new("the card's ring is malformed", 0))?;
    let need = (port.slots as usize).saturating_mul(SLOT_SIZE as usize);
    if ring.sq_entries() < port.slots || ring.cq_entries() < port.slots || ring.data_len() < need {
        return Err(AttachError::new(
            "the card's ring is smaller than its attach message says",
            0,
        ));
    }
    let rx = port.slots / 2;
    let mut free = [0u32; MAX_SLOTS];
    let mut nfree = 0usize;
    for (to, slot) in free.iter_mut().zip(rx..port.slots) {
        *to = slot;
        nfree += 1;
    }
    Ok(Card {
        channel,
        ring,
        port,
        rx,
        free,
        nfree,
        got: [(0, 0); MAX_SLOTS],
        got_head: 0,
        got_len: 0,
        send_errors: 0,
        slept: 0,
        rung: 0,
        gone: false,
    })
}

/// Receive slot `slot` posted.
fn post(c: &mut Card, slot: u32) {
    let sqe = Sqe {
        op: OP_RECV,
        conn_id: 0,
        data_off: c.port.slot_offset(slot).unwrap_or(0),
        data_len: SLOT_SIZE,
        user_data: u64::from(slot),
        aux: 0,
    };
    // The queue has an entry per slot, and each slot is submitted at most
    // once at a time: it cannot be full.
    let _pushed = c.ring.sq_push(&sqe);
}

/// A frame submitted: the driver rung if it said it sleeps and was not rung
/// for that sleep. A connection with no room holds a doorbell already.
fn ring_if_asleep(c: &mut Card) {
    let sleeper = c.ring.sq_sleeper();
    if sleeper == 0 || sleeper == c.rung {
        return;
    }
    c.rung = sleeper;
    let bell = [DOORBELL];
    if syscall3(SYS_CHANNEL_SEND, c.channel, bell.as_ptr() as u64, 1) == CHANNEL_CLOSED {
        c.gone = true;
    }
}

/// The completions taken -- a queue's worth at most: a frame received
/// queued to be read (a failed receive's slot posted again), a send's slot
/// freed. A completion queue the driver wrote broken is the card gone.
fn reap(c: &mut Card) {
    if c.ring.cq_broken() {
        c.gone = true;
        return;
    }
    for _ in 0..c.ring.cq_entries() {
        let Some(cqe) = c.ring.cq_pop() else {
            return;
        };
        let Ok(slot) = u32::try_from(cqe.user_data) else {
            continue;
        };
        if slot < c.rx {
            match u32::try_from(cqe.result) {
                Ok(len) if c.got_len < MAX_SLOTS => {
                    let at = (c.got_head + c.got_len) % MAX_SLOTS;
                    c.got[at] = (slot, len);
                    c.got_len += 1;
                }
                _ => post(c, slot),
            }
        } else if slot < c.port.slots && c.nfree < MAX_SLOTS {
            if cqe.result < 0 {
                c.send_errors += 1;
            }
            c.free[c.nfree] = slot;
            c.nfree += 1;
        }
    }
}

/// `frame` sent on the card: its length, or a negative errno -- at once for
/// a frame the card cannot send: one with no header, or longer than a
/// header, a VLAN tag and the card's MTU. `None` with no card attached.
pub fn send(frame: &[u8]) -> Option<i64> {
    let c = card()?;
    if c.gone {
        return Some(NET_DOWN);
    }
    if frame.len() < ETH_HLEN {
        return Some(i64::from(ERR_INVALID));
    }
    if frame.len() > usize::from(c.port.mtu) + ETH_HLEN + VLAN_HLEN
        || frame.len() > SLOT_SIZE as usize
    {
        return Some(i64::from(ERR_MSG_SIZE));
    }
    reap(c);
    let deadline = now_ns().saturating_add(SLOT_WAIT_NS);
    while c.nfree == 0 && !c.gone && now_ns() < deadline {
        let _rang = wait_on(c, deadline.saturating_sub(now_ns()));
        reap(c);
    }
    if c.nfree == 0 {
        return Some(if c.gone { NET_DOWN } else { E_WOULD_BLOCK });
    }
    c.nfree -= 1;
    let slot = c.free[c.nfree];
    let off = c.port.slot_offset(slot).unwrap_or(0);
    let sqe = Sqe {
        op: OP_SEND,
        conn_id: 0,
        data_off: off,
        data_len: frame.len() as u32,
        user_data: u64::from(slot),
        aux: 0,
    };
    // The slot is inside the data area and the queue has an entry per slot:
    // neither can fail while the driver keeps to the protocol. If one does,
    // the slot is given back and the frame not sent.
    if !c.ring.write_data(off as usize, frame) || !c.ring.sq_push(&sqe) {
        c.nfree += 1;
        return Some(E_WOULD_BLOCK);
    }
    ring_if_asleep(c);
    Some(frame.len() as i64)
}

/// The next frame received, into `buf`: its length (cut to `buf`'s), or
/// [`E_WOULD_BLOCK`] for none yet. `None` with no card attached.
pub fn recv(buf: &mut [u8]) -> Option<i64> {
    let c = card()?;
    if c.got_len == 0 {
        reap(c);
    }
    if c.got_len == 0 {
        return Some(if c.gone { NET_DOWN } else { E_WOULD_BLOCK });
    }
    let (slot, len) = c.got[c.got_head];
    c.got_head = (c.got_head + 1) % MAX_SLOTS;
    c.got_len -= 1;
    let n = (len as usize).min(buf.len()).min(SLOT_SIZE as usize);
    let off = c.port.slot_offset(slot).unwrap_or(0) as usize;
    let read = c.ring.read_data(off, &mut buf[..n]);
    post(c, slot);
    Some(if read {
        n as i64
    } else {
        i64::from(ERR_INVALID)
    })
}

/// How a [`wait`] ended.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Woke {
    /// The driver rang, or completions were already there.
    Rang,
    /// The time ran out with no doorbell.
    Slept,
    /// The card has gone (or none is attached).
    Gone,
}

/// Wait up to `timeout_ns` for the driver's doorbell, unless a frame is
/// already there to read.
pub fn wait(timeout_ns: u64) -> Woke {
    let Some(c) = card() else {
        return Woke::Gone;
    };
    reap(c);
    let rang = c.got_len != 0 || c.gone || wait_on(c, timeout_ns);
    match (c.gone, rang) {
        (true, _) => Woke::Gone,
        (false, true) => Woke::Rang,
        (false, false) => Woke::Slept,
    }
}

/// The driver's doorbell waited for, up to `timeout_ns` -- the daemon
/// saying in the ring that it sleeps, unless completions are there already
/// -- and any more that came taken: whether it rang, or completions were
/// there.
fn wait_on(c: &mut Card, timeout_ns: u64) -> bool {
    c.slept = c.slept.wrapping_add(1).max(1);
    if !c.ring.cq_sleep(c.slept) {
        return true;
    }
    let mut byte = [0u8; 16];
    let got = syscall4(
        SYS_CHANNEL_RECV_TIMEOUT,
        c.channel,
        byte.as_mut_ptr() as u64,
        byte.len() as u64,
        timeout_ns.max(1),
    );
    c.ring.cq_wake();
    if got == CHANNEL_CLOSED {
        c.gone = true;
        return false;
    }
    let mut rang = got > 0;
    for _ in 0..DOORBELLS_A_WAIT {
        let more = syscall3(
            SYS_CHANNEL_TRY_RECV,
            c.channel,
            byte.as_mut_ptr() as u64,
            byte.len() as u64,
        );
        if more == CHANNEL_CLOSED {
            c.gone = true;
        }
        if more <= 0 {
            break;
        }
        rang = true;
    }
    rang
}

/// Sends the driver completed with an error, so far.
pub fn send_errors() -> u64 {
    card().map_or(0, |c| c.send_errors)
}

/// `SYS_SHM_GRANT` asked to pass on a region the daemon was never granted
/// -- one that cannot exist, its id far past any made -- to the driver: the
/// kernel's answer, which must be `PermissionDenied`. The boot test's check
/// that a process can give away only what it holds.
pub fn grant_unheld() -> i64 {
    let Some(c) = card() else {
        return 0;
    };
    syscall2(SYS_SHM_GRANT, c.port.region ^ (1 << 40), c.channel)
}

/// The card let go: the connection closed, which the driver sees as the
/// daemon leaving.
pub fn detach() {
    // SAFETY: as in `card`.
    if let Some(c) = unsafe { (*core::ptr::addr_of_mut!(CARD)).take() } {
        syscall1(SYS_CHANNEL_CLOSE, c.channel);
    }
}

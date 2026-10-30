//! A network card a driver process serves (design-decisions 1589), as the
//! daemon's NIC: the service `nic.<name>` connected to, the ring it is
//! given in shared memory attached, and frames sent and received through
//! it. With a card attached, [`super::nic::send`] and [`super::nic::recv`] come
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
//! The driver's other messages are read as they come with its doorbells:
//! the card's link, kept for [`link`], and the answer to the last multicast
//! list. The daemon's lists go through [`set_mcast`], which keeps the
//! latest and sends it once the last is answered -- the protocol allows one
//! at a time -- so the card ends with the latest asked for.
//!
//! Single-threaded daemon: the card is a `static mut` no borrow of which
//! outlives a call here, as [`super::loopback`]'s state is.

use netipc::nicport::{
    ATTACH_LEN, DOORBELL, MESSAGE_MAX, McastList, Message, Port, REFUSED_NO_KEY, REFUSED_NO_RING,
    SLOT_SIZE,
};
use netipc::ring::{ERR_INVALID, ERR_MSG_SIZE, OP_RECV, OP_SEND, Sqe};
use netring::Ring;

use super::wait::{KIND_CHANNEL, KIND_SERVICE, WaitFailed, WaitItem, wait_multiple};

use super::{
    E_WOULD_BLOCK, SHM_MAP_RW, SYS_CHANNEL_CLOSE, SYS_CHANNEL_RECV_TIMEOUT, SYS_CHANNEL_SEND,
    SYS_SHM_MAP, now_ns, sleep_ns, syscall1, syscall2, syscall3, syscall4,
};

const SYS_CHANNEL_TRY_RECV: u64 = 203;
const SYS_SHM_SIZE: u64 = 231;
const SYS_SHM_CLOSE: u64 = 232;
const SYS_SHM_UNMAP: u64 = 234;
const SYS_SERVICE_CONNECT: u64 = 281;
const SYS_SHM_GRANT: u64 = 1190;
/// SlateOS's `NotFound` (no such service yet) and `ChannelClosed` (the
/// driver has gone).
const NOT_FOUND: i64 = -500;
const CHANNEL_CLOSED: i64 = -300;
/// What `send` answers once the card has gone: `ENETDOWN`. (While its link
/// is down a frame is dropped, as on a wire with nothing at its end.)
pub const NET_DOWN: i64 = -100;
/// The most slots a card's ring may have: the daemon's tables' size.
const MAX_SLOTS: usize = 256;
/// How long a send waits for a send slot to come free: 100 ms.
const SLOT_WAIT_NS: u64 = 100_000_000;
/// The most messages one wait takes after the first.
const MESSAGES_A_WAIT: usize = 64;
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
    /// The card's link as the driver last told it; `None` before it has.
    link: Option<bool>,
    /// How many times the driver has told it.
    links_told: u32,
    /// Whether the link has come back up since it was last told down, and
    /// the daemon has not yet taken that ([`take_link_back`]).
    link_back: bool,
    /// The multicast list asked for and not yet sent.
    mcast_want: Option<McastList>,
    /// A list was sent and is not yet answered.
    mcast_sent: bool,
    /// The last list's answer: 0 in force, or the driver's errno; `None`
    /// before any.
    mcast_result: Option<i32>,
    /// Frames dropped for being sent while the link was down.
    dropped_down: u64,
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
    /// The kernel's answer to the system call that failed, if one did.
    pub answer: Option<i64>,
    /// Why the driver refused the daemon, when it did
    /// (`netipc::nicport::REFUSED_*`).
    pub refused: Option<u8>,
}

impl AttachError {
    /// `why`, a system call having answered `answer`.
    fn call(why: &'static str, answer: i64) -> Self {
        Self {
            why,
            answer: Some(answer),
            refused: None,
        }
    }

    /// `why`, no system call having failed.
    fn plain(why: &'static str) -> Self {
        Self {
            why,
            answer: None,
            refused: None,
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
            return Err(AttachError::call(
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
/// ring it names mapped and attached -- or its refusal. On every way out
/// but the first, the daemon's reference to the region the message names
/// (its grant's) is given back, and the region unmapped if it was mapped.
fn ring_for(channel: u64, deadline: u64) -> Result<Card, AttachError> {
    // One byte more than the message, so that a longer one is told apart.
    let mut message = [0u8; ATTACH_LEN + 1];
    let wait = deadline.saturating_sub(now_ns()).max(1);
    let got = syscall4(
        SYS_CHANNEL_RECV_TIMEOUT,
        channel,
        message.as_mut_ptr() as u64,
        message.len() as u64,
        wait,
    );
    if got == CHANNEL_CLOSED {
        return Err(AttachError::call(
            "the card's driver closed the connection without a word",
            got,
        ));
    }
    let Ok(len) = usize::try_from(got) else {
        return Err(AttachError::call(
            "no attach message from the card's driver",
            got,
        ));
    };
    let first = message.get(..len).unwrap_or(&[]);
    if let Some(Message::Refused { reason }) = Message::decode(first) {
        return Err(AttachError {
            why: match reason {
                REFUSED_NO_KEY => {
                    "the card's driver refused the daemon: it holds no key for the card"
                }
                REFUSED_NO_RING => "the card's driver refused the daemon: it could not make a ring",
                _ => "the card's driver refused the daemon, for a reason this daemon does not know",
            },
            answer: None,
            refused: Some(reason),
        });
    }
    if len > ATTACH_LEN {
        return Err(AttachError::plain(
            "an attach message longer than the protocol's",
        ));
    }
    let port =
        Port::decode(first).ok_or_else(|| AttachError::plain("a malformed attach message"))?;
    let give_back = |e: AttachError| {
        // A region never granted is no handle of the daemon's: the close
        // fails, and there is nothing to give back.
        syscall1(SYS_SHM_CLOSE, port.region);
        e
    };
    if port.slots < 2 || port.slots as usize > MAX_SLOTS {
        return Err(give_back(AttachError::plain(
            "an attach message with a slot count the daemon cannot use",
        )));
    }
    let sized = syscall1(SYS_SHM_SIZE, port.region);
    let Ok(size) = usize::try_from(sized) else {
        return Err(give_back(AttachError::call(
            "the card's ring's size could not be asked for",
            sized,
        )));
    };
    let mapped = syscall2(SYS_SHM_MAP, port.region, SHM_MAP_RW);
    let Ok(at) = u64::try_from(mapped) else {
        return Err(give_back(AttachError::call(
            "the card's ring could not be mapped",
            mapped,
        )));
    };
    let unmap_and_give_back = |e: AttachError| {
        syscall2(SYS_SHM_UNMAP, at, size as u64);
        give_back(e)
    };
    // SAFETY: the region is mapped at `at`, `size` bytes, page-aligned: on
    // success for the rest of the daemon's life; otherwise unmapped below,
    // after which the ring -- a view of the mapping, with nothing to drop --
    // is never used.
    let Some(ring) = (unsafe { Ring::attach(at as *mut u8, size) }) else {
        return Err(unmap_and_give_back(AttachError::plain(
            "the card's ring is malformed",
        )));
    };
    let need = (port.slots as usize).saturating_mul(SLOT_SIZE as usize);
    if ring.sq_entries() < port.slots || ring.cq_entries() < port.slots || ring.data_len() < need {
        return Err(unmap_and_give_back(AttachError::plain(
            "the card's ring is smaller than its attach message says",
        )));
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
        link: None,
        links_told: 0,
        link_back: false,
        mcast_want: None,
        mcast_sent: false,
        mcast_result: None,
        dropped_down: 0,
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
/// for that sleep. A connection with no room holds messages already, which
/// wake the driver as a doorbell would.
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
/// header, a VLAN tag and the card's MTU; `ENETDOWN` for a card gone. A
/// frame sent while the link is down is dropped ([`dropped_while_down`]).
/// `None` with no card attached.
pub fn send(frame: &[u8]) -> Option<i64> {
    let c = card()?;
    if c.gone {
        return Some(NET_DOWN);
    }
    // A link down drops the frame, as a wire with nothing at its end would,
    // rather than queue it in the driver to go out stale once the link is
    // back -- or fail the send, which TCP would take for the connection's
    // end where its retransmissions ride a short flap out.
    if c.link == Some(false) {
        c.dropped_down = c.dropped_down.saturating_add(1);
        return Some(frame.len() as i64);
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
    /// The driver rang or sent another message, or completions were
    /// already there.
    Rang,
    /// The time ran out with nothing from the driver.
    Slept,
    /// The card has gone (or none is attached).
    Gone,
}

/// Wait up to `timeout_ns` for the driver's doorbell or another message,
/// unless a frame is already there to read. Either way the driver's
/// messages that have come are taken ([`see_to_channel`]): a card busy with
/// frames does not leave its link and answers unread behind them.
pub fn wait(timeout_ns: u64) -> Woke {
    let Some(c) = card() else {
        return Woke::Gone;
    };
    reap(c);
    let rang = if c.got_len != 0 || c.gone {
        see_to_channel(c);
        true
    } else {
        wait_on(c, timeout_ns)
    };
    woke(c, rang)
}

/// [`wait`], woken as well by a connection waiting on the service
/// `listener` (the handle `SYS_SERVICE_REGISTER` answered) -- for a daemon
/// that serves its clients and its card together, the caller then asking
/// the listener. A frame already there answers at once, without looking at
/// the listener. An error ([`WaitFailed`]) when the wait itself failed.
pub fn wait_or_client(listener: u64, timeout_ns: u64) -> Result<Woke, WaitFailed> {
    let Some(c) = card() else {
        return Ok(Woke::Gone);
    };
    reap(c);
    let rang = if c.got_len != 0 || c.gone {
        see_to_channel(c);
        true
    } else {
        wait_on_or_client(c, timeout_ns, listener)?
    };
    Ok(woke(c, rang))
}

/// How the card woke: gone, rung (or with something to read), or neither.
fn woke(c: &Card, rang: bool) -> Woke {
    match (c.gone, rang) {
        (true, _) => Woke::Gone,
        (false, true) => Woke::Rang,
        (false, false) => Woke::Slept,
    }
}

/// The driver's doorbell or another message waited for, up to `timeout_ns`
/// -- the daemon saying in the ring that it sleeps, unless completions are
/// there already -- and any more that came taken: whether one came, or
/// completions were there. A failed receive is the card gone
/// ([`take_message`]).
fn wait_on(c: &mut Card, timeout_ns: u64) -> bool {
    c.slept = c.slept.wrapping_add(1).max(1);
    if !c.ring.cq_sleep(c.slept) {
        see_to_channel(c);
        return true;
    }
    let mut buf = [0u8; MESSAGE_MAX];
    let got = syscall4(
        SYS_CHANNEL_RECV_TIMEOUT,
        c.channel,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
        timeout_ns.max(1),
    );
    c.ring.cq_wake();
    let came = take_message(c, got, &buf);
    see_to_channel(c) || came
}

/// [`wait_on`], woken as well by a connection waiting on the service
/// `listener`; the driver's messages taken after, as any that come while
/// the daemon is awake are. An error when the wait itself failed.
fn wait_on_or_client(c: &mut Card, timeout_ns: u64, listener: u64) -> Result<bool, WaitFailed> {
    c.slept = c.slept.wrapping_add(1).max(1);
    if !c.ring.cq_sleep(c.slept) {
        see_to_channel(c);
        return Ok(true);
    }
    let mut items = [
        WaitItem::readable(c.channel, KIND_CHANNEL),
        WaitItem::readable(listener, KIND_SERVICE),
    ];
    let waited = wait_multiple(&mut items, timeout_ns);
    c.ring.cq_wake();
    waited?;
    Ok(see_to_channel(c))
}

/// The driver's messages that have come, taken without waiting --
/// [`MESSAGES_A_WAIT`] at most -- and the multicast list asked for sent if
/// it can be now: whether any came.
fn see_to_channel(c: &mut Card) -> bool {
    let mut buf = [0u8; MESSAGE_MAX];
    let mut came = false;
    for _ in 0..MESSAGES_A_WAIT {
        if c.gone {
            break;
        }
        let more = syscall3(
            SYS_CHANNEL_TRY_RECV,
            c.channel,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        );
        if !take_message(c, more, &buf) {
            break;
        }
        came = true;
    }
    send_mcast(c);
    came
}

/// The receive that answered `got`, the message in `buf`, taken: whether
/// there was one. A connection closed is the card gone, and so is a
/// message the protocol has no place for from a driver -- too long,
/// unreadable, or a stack's.
fn take_message(c: &mut Card, got: i64, buf: &[u8]) -> bool {
    if got == CHANNEL_CLOSED {
        c.gone = true;
        return false;
    }
    let Ok(len) = usize::try_from(got) else {
        return false;
    };
    if len == 0 {
        return false;
    }
    match buf.get(..len).and_then(Message::decode) {
        Some(Message::Doorbell) => {}
        Some(Message::Link { up }) => {
            c.link_back |= up && c.link == Some(false);
            c.link = Some(up);
            c.links_told = c.links_told.saturating_add(1);
        }
        Some(Message::McastDone { result }) if c.mcast_sent => {
            c.mcast_sent = false;
            c.mcast_result = Some(result);
        }
        _ => {
            super::print(
                "[netstack] the card's driver sent a message the protocol has no place for; the card is let go\n",
            );
            c.gone = true;
        }
    }
    true
}

/// The multicast list asked for sent, if there is one and the last was
/// answered; kept for a later wait if the connection has no room for it.
fn send_mcast(c: &mut Card) {
    let Some(want) = c.mcast_want.filter(|_| !c.mcast_sent && !c.gone) else {
        return;
    };
    let mut message = [0u8; MESSAGE_MAX];
    let Some(n) = want.encode(&mut message) else {
        // MESSAGE_MAX holds any list.
        c.mcast_want = None;
        return;
    };
    let sent = syscall3(
        SYS_CHANNEL_SEND,
        c.channel,
        message.as_ptr() as u64,
        n as u64,
    );
    if sent == CHANNEL_CLOSED {
        c.gone = true;
    } else if sent == 0 {
        c.mcast_want = None;
        c.mcast_sent = true;
    }
}

/// Whether a card is attached -- gone or not.
pub fn attached() -> bool {
    card().is_some()
}

/// Whether the card attached has gone: its driver died, or let it go.
pub fn gone() -> bool {
    card().is_some_and(|c| c.gone)
}

/// The card's link as its driver last told it: `None` with no card
/// attached, or before the driver has said.
pub fn link() -> Option<bool> {
    card().and_then(|c| c.link)
}

/// How many times the driver has told the card's link -- once on
/// attaching, then at each change -- 0 with no card attached.
pub fn links_told() -> u32 {
    card().map_or(0, |c| c.links_told)
}

/// Whether the card's link has come back up since it was told down -- a
/// cable plugged back, say -- since this last answered: for the daemon to
/// confirm its lease and announce its addresses again, what the link's
/// other end knew of it perhaps gone. False with no card attached.
pub fn take_link_back() -> bool {
    card().is_some_and(|c| core::mem::replace(&mut c.link_back, false))
}

/// The multicast addresses the card must pass from now on (`McastList::new`:
/// `addrs`, any that is not a multicast address left out, and every one with
/// `all` or with more than one list holds). Sent now if the driver has
/// answered the last list, else once it has. Whether a card is attached --
/// and not gone -- to take it.
pub fn set_mcast(all: bool, addrs: &[[u8; 6]]) -> bool {
    let Some(c) = card().filter(|c| !c.gone) else {
        return false;
    };
    c.mcast_want = Some(McastList::new(all, addrs));
    send_mcast(c);
    true
}

/// How many frames were dropped for being sent while the card's link was
/// down; 0 with no card attached.
pub fn dropped_while_down() -> u64 {
    card().map_or(0, |c| c.dropped_down)
}

/// How the multicast lists asked for went: `None` while one is still to be
/// sent or answered, before the driver has answered any, or with no card
/// attached; else the last one's answer -- 0 in force, or the driver's
/// negative errno.
pub fn mcast_settled() -> Option<i32> {
    card()
        .filter(|c| c.mcast_want.is_none() && !c.mcast_sent)
        .and_then(|c| c.mcast_result)
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

/// A message the protocol has no place for from a stack -- a link, which
/// only a driver tells -- sent to the driver: the boot test's check that a
/// driver lets go of a stack that breaks the protocol. Nothing with no card.
pub fn send_out_of_place() {
    let Some(c) = card() else {
        return;
    };
    let mut message = [0u8; 2];
    if let Some(n) = netipc::nicport::encode_link(true, &mut message) {
        // Not sent -- the card gone already -- is what the check then sees.
        let _sent = syscall3(
            SYS_CHANNEL_SEND,
            c.channel,
            message.as_ptr() as u64,
            n as u64,
        );
    }
}

/// The card let go: the connection closed, which the driver sees as the
/// daemon leaving.
pub fn detach() {
    // SAFETY: as in `card`.
    if let Some(c) = unsafe { (*core::ptr::addr_of_mut!(CARD)).take() } {
        syscall1(SYS_CHANNEL_CLOSE, c.channel);
    }
}

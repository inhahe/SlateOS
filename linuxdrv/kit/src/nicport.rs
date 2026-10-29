//! A network card served to the network stack (design-decisions 1589): the
//! card registered as the service `nic.<name>`, the stack that connects
//! given a ring in shared memory, and the ring driven for the C half's
//! `lx_nic.c`, which sends the frames the stack submits and receives into
//! the slots it posts.
//!
//! Between tasks, at every turn, the loop sees to the connection
//! ([`slx_nicport_turn`], through the C half's turn hook): a stack
//! connecting -- served only when it holds the card's key -- its messages,
//! its leaving, and its submissions sorted ([`crate::frames`]); the card's
//! link told it when it changed; then, completions pushed, the stack rung
//! if it sleeps. With nothing to run, the loop waits for the listener or
//! the connection as well ([`wait_items`]), having said in the ring that it
//! sleeps. Messages are looked for when the wait says one came, and every
//! [`LOOK_EVERY_NS`] besides, so that a loop kept busy still sees a stack
//! leave; the ring itself is read at every turn.
//!
//! A multicast list the stack sends is kept until the C half's task takes
//! it ([`slx_nicport_mcast_next`]), puts it in force and says how that went
//! ([`slx_nicport_mcast_done`]), which the stack is told.
//!
//! Doorbells go only to a side that sleeps (`netring`'s sleep words): each
//! says in the ring that it is about to sleep, and the other rings once for
//! that sleep. So a busy driver and stack exchange frames with no system
//! call between them.
//!
//! Nothing the stack does is trusted: what it submits is checked, at most a
//! queue's worth is taken a turn, at most [`MESSAGES_A_LOOK`] messages a
//! look, and a ring whose indices it wrote past what the ring holds, or a
//! message the protocol has no place for, ends its attachment. One stack at
//! a time: another that connects waits until the first has gone.

use netipc::nicport::{
    ATTACH_LEN, DOORBELL, MCAST_MAX, MESSAGE_MAX, Message, Port, REFUSED_NO_KEY, REFUSED_NO_RING,
    SERVICE_PREFIX, SLOT_SIZE, encode_link, encode_mcast_done, encode_refused,
};
use netipc::ring::{Cqe, ERR_INVALID, ERR_NO_BUFS};
use netring::Ring;

use crate::frames::{self, Queue, Sorted};
use crate::sys::{Single, log, syscall4};
use crate::wait::{KIND_CHANNEL, KIND_SERVICE, POLLERR, POLLIN, POLLNVAL, WaitItem};

const SYS_CHANNEL_SEND: u64 = 201;
const SYS_CHANNEL_TRY_RECV: u64 = 203;
const SYS_CHANNEL_CLOSE: u64 = 204;
const SYS_SHM_CREATE: u64 = 230;
const SYS_SHM_CLOSE: u64 = 232;
const SYS_SHM_MAP: u64 = 233;
const SYS_SHM_UNMAP: u64 = 234;
const SYS_SERVICE_REGISTER: u64 = 280;
const SYS_SERVICE_TRY_ACCEPT: u64 = 283;
const SYS_SERVICE_UNREGISTER: u64 = 285;
const SYS_CHANNEL_PEER_HAS_KEY: u64 = 1103;
const SYS_SHM_GRANT: u64 = 1190;
/// `SYS_SHM_MAP`'s flags: read and write.
const MAP_READ_WRITE: u64 = (1 << 0) | (1 << 1);
/// SlateOS's `ChannelClosed`: the stack has gone.
const CHANNEL_CLOSED: i64 = -300;
/// SlateOS's `WouldBlock`: no stack waiting to be accepted.
const WOULD_BLOCK: i64 = -4;

/// The ring's slots, and its queues' entries: 128 frames in flight each
/// way at most, as many as a card's own rings commonly hold.
const SLOTS: u32 = 128;
/// The queues of sorted submissions hold as many: a stack that keeps to
/// its slots never has more outstanding.
const QUEUED: usize = SLOTS as usize;
/// How often a busy loop looks at the connection anyway: 10 ms.
const LOOK_EVERY_NS: u64 = 10_000_000;
/// The most messages one look takes: a stack sending faster than the
/// driver turns is answered at the next look, not by a loop it keeps going.
const MESSAGES_A_LOOK: usize = 64;
/// What the loop's idle wait may ask of this module: one item.
pub(crate) const WAIT_ITEMS: usize = 1;
// The C half's list buffer is `SLX_NICPORT_MCAST_MAX` (slx.h) addresses.
const _: () = assert!(MCAST_MAX == 64);
/// The longest name a card is served under, after the prefix.
const NAME_MAX: usize = 59;

/// A stack attached: its connection, the ring's region and mapping, and
/// its submissions sorted and not yet completed.
struct Client {
    channel: u64,
    region: u64,
    mapping: u64,
    /// The bytes mapped at `mapping`: what is unmapped.
    size: u64,
    ring: Ring,
    sends: Queue<QUEUED>,
    receives: Queue<QUEUED>,
    /// The send the C half was handed and has not finished: its token.
    sending: Option<u64>,
    /// Whether the C half holds the front receive slot.
    receiving: bool,
    /// Completions pushed and not yet looked at for the stack's sleep.
    kick: bool,
    /// The stack's sleep last rung for (its CQ sleep word).
    rung: u32,
    /// The driver's own sleep number, new for each idle wait.
    slept: u32,
    /// The link the stack was last told of; `None` before the first.
    told_link: Option<bool>,
    /// The multicast list the stack last sent, not yet answered.
    mcast: Option<McastList>,
    /// The answer to the last list, not yet sent: the connection had no
    /// room for it.
    answer: Option<i32>,
    /// The connection closed, or the stack broke the protocol: no more work
    /// taken, the attachment ended once the C half holds nothing of it.
    gone: bool,
}

/// A multicast list a stack sent: `len` addresses of `addrs`, or every
/// multicast frame with `all`; `taken` once the C half has it.
struct McastList {
    addrs: [[u8; 6]; MCAST_MAX],
    len: usize,
    all: bool,
    taken: bool,
}

struct Served {
    /// The card's listener while it is served.
    listener: Option<u64>,
    /// Whether the card was served: a listener lost since ends the serving.
    opened: bool,
    mac: [u8; 6],
    mtu: u16,
    /// The card's link as of the last turn.
    link: bool,
    client: Option<Client>,
    /// The wait said the listener or the connection has something.
    look: bool,
    looked_ns: u64,
    /// The stack submitted something after the loop's last turn: the idle
    /// wait must not sleep.
    busy: bool,
    /// Attachments ended and not yet told the C half.
    ended: u32,
    /// Completions the stack left no room for: it broke the protocol, and
    /// they were dropped.
    lost: u64,
}

static SERVED: Single<Served> = Single::new(Served {
    listener: None,
    opened: false,
    mac: [0; 6],
    mtu: 0,
    link: false,
    client: None,
    look: false,
    looked_ns: 0,
    busy: false,
    ended: 0,
    lost: 0,
});

/// The state. Every function of this module takes it once and calls none
/// of the others that take it, nor anything that reaches them.
fn served() -> &'static mut Served {
    // SAFETY: the one thread; the functions of this module do not re-enter
    // one another (above).
    unsafe { SERVED.get() }
}

/// `slx_nicport_open`: serve the card whose MAC address is the six bytes at
/// `mac`, carrying `mtu` bytes a frame, as `nic.<name>` (`name` a C
/// string): 0, or the kernel's error -- `PermissionDenied` (-400) for a
/// process holding no `Service` capability for the name, `AlreadyExists`
/// (-501) for a name taken. One card a process.
///
/// # Safety
///
/// `name` is a NUL-terminated string; `mac` points at six bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slx_nicport_open(name: *const u8, mac: *const u8, mtu: u32) -> i64 {
    /// SlateOS's `InvalidArgument` and `AlreadyExists`.
    const INVALID_ARGUMENT: i64 = -3;
    const ALREADY_EXISTS: i64 = -501;
    let s = served();
    if s.opened {
        return ALREADY_EXISTS;
    }
    let Ok(mtu) = u16::try_from(mtu) else {
        return INVALID_ARGUMENT;
    };
    let mut full = [0u8; SERVICE_PREFIX.len() + NAME_MAX];
    let mut len = 0usize;
    for (to, &from) in full.iter_mut().zip(SERVICE_PREFIX) {
        *to = from;
        len = len.saturating_add(1);
    }
    let mut at = name;
    loop {
        // SAFETY: the caller's: `name` is NUL-terminated, and this stops at
        // its NUL.
        let byte = unsafe { at.read() };
        if byte == 0 {
            break;
        }
        let Some(to) = full.get_mut(len) else {
            return INVALID_ARGUMENT;
        };
        *to = byte;
        len = len.saturating_add(1);
        at = at.wrapping_add(1);
    }
    if len == SERVICE_PREFIX.len() {
        return INVALID_ARGUMENT;
    }
    // SAFETY: the caller's: `mac` points at six bytes.
    unsafe { core::ptr::copy_nonoverlapping(mac, s.mac.as_mut_ptr(), 6) };
    s.mtu = mtu;
    let listener = syscall4(SYS_SERVICE_REGISTER, full.as_ptr() as u64, len as u64, 0, 0);
    let Ok(listener) = u64::try_from(listener) else {
        return listener;
    };
    s.listener = Some(listener);
    s.opened = true;
    0
}

/// `slx_nicport_turn`: a turn of the loop, between tasks, the clock reading
/// `now_ns` and the card's link `link` (up or down): the connection seen to
/// -- a stack accepted and attached, its messages taken, its leaving
/// noticed -- its submissions sorted, the link told it if it changed, and
/// the stack rung if completions were pushed while it sleeps. Whether the
/// C half's task has something to do (`slx_nicport_work`).
#[unsafe(no_mangle)]
pub extern "C" fn slx_nicport_turn(now_ns: u64, link: bool) -> bool {
    let s = served();
    if !s.opened {
        return false;
    }
    s.link = link;
    if s.look || now_ns.saturating_sub(s.looked_ns) >= LOOK_EVERY_NS {
        s.look = false;
        s.looked_ns = now_ns;
        match (s.client.as_mut(), s.listener) {
            (Some(c), _) => take_messages(c),
            (None, Some(listener)) => {
                match accept(listener, s.mac, s.mtu) {
                    Ok(client) => s.client = client,
                    Err(ListenerFailed) => {
                        log(b"[lx] nicport: the card's listener failed; the card is no longer served\n");
                        drop_listener(s);
                    }
                }
            }
            (None, None) => {}
        }
    }
    let Some(c) = s.client.as_mut() else {
        return s.listener.is_none();
    };
    if !c.gone {
        sort_submissions(c, s.mtu, &mut s.lost);
    }
    if !c.gone && c.told_link != Some(s.link) {
        tell_link(c, s.link);
    }
    if !c.gone {
        tell_answer(c);
    }
    if c.kick && !c.gone {
        ring_if_asleep(c);
    }
    let applying = c.mcast.as_ref().is_some_and(|m| m.taken);
    if c.gone && c.sending.is_none() && !c.receiving && !applying {
        if let Some(c) = s.client.take() {
            detach(&c);
        }
        if s.lost != 0 {
            log(b"[lx] nicport: the stack left completions no room, and some were dropped\n");
            s.lost = 0;
        }
        s.ended = s.ended.saturating_add(1);
        return true;
    }
    has_work(c)
}

/// Whether the C half's task has something to do for `c`: frames to send,
/// or a multicast list to put in force.
fn has_work(c: &Client) -> bool {
    !c.gone && (!c.sends.is_empty() || c.mcast.as_ref().is_some_and(|m| !m.taken))
}

/// `slx_nicport_work`: whether the C half's task has something to do --
/// frames to send, a multicast list to put in force, an attachment ended,
/// or the card no longer served -- without changing anything.
#[unsafe(no_mangle)]
pub extern "C" fn slx_nicport_work() -> bool {
    let s = served();
    s.ended != 0
        || (s.opened && s.listener.is_none() && s.client.is_none())
        || s.client.as_ref().is_some_and(has_work)
}

/// `slx_nicport_mcast_next`: the multicast list the stack sent and the C
/// half has not yet taken: the addresses written to `list`, six bytes each,
/// whether every multicast frame is wanted to `*all`, and how many there
/// are; or -1 for none. [`slx_nicport_mcast_done`] says how it went before
/// the next is given.
///
/// # Safety
///
/// `list` points at `6 * MCAST_MAX` bytes to write, `all` at a bool.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slx_nicport_mcast_next(list: *mut u8, all: *mut bool) -> i64 {
    let s = served();
    let Some(m) = s
        .client
        .as_mut()
        .filter(|c| !c.gone)
        .and_then(|c| c.mcast.as_mut())
        .filter(|m| !m.taken)
    else {
        return -1;
    };
    m.taken = true;
    let mut to = list;
    for addr in m.addrs.get(..m.len).unwrap_or(&[]) {
        // SAFETY: the caller's: `list` holds `6 * MCAST_MAX` bytes, and at
        // most `MCAST_MAX` addresses are written, six bytes each.
        unsafe { core::ptr::copy_nonoverlapping(addr.as_ptr(), to, 6) };
        to = to.wrapping_add(6);
    }
    // SAFETY: the caller's.
    unsafe { all.write(m.all) };
    i64::try_from(m.len).unwrap_or(-1)
}

/// `slx_nicport_mcast_done`: the list [`slx_nicport_mcast_next`] gave is in
/// force (`result` 0) or failed (a negative errno): the stack told, and the
/// next list it sends taken.
#[unsafe(no_mangle)]
pub extern "C" fn slx_nicport_mcast_done(result: i32) {
    let s = served();
    let Some(c) = s.client.as_mut() else {
        return;
    };
    if !c.mcast.as_ref().is_some_and(|m| m.taken) {
        return;
    }
    c.mcast = None;
    c.answer = Some(result);
    if !c.gone {
        tell_answer(c);
    }
}

/// `slx_nicport_serving`: whether the card is still served -- its listener
/// there, or a stack still attached.
#[unsafe(no_mangle)]
pub extern "C" fn slx_nicport_serving() -> bool {
    let s = served();
    s.listener.is_some() || s.client.is_some()
}

/// `slx_nicport_ended`: how many attachments have ended since last asked.
#[unsafe(no_mangle)]
pub extern "C" fn slx_nicport_ended() -> u32 {
    core::mem::take(&mut served().ended)
}

/// `slx_nicport_tx_next`: the next frame the stack submitted, made the one
/// being sent: its length, its bytes at `*frame` -- the ring's, which the
/// stack can change at any time, so copied before they are relied on -- or
/// 0 for none. [`slx_nicport_tx_done`] finishes it before the next.
///
/// # Safety
///
/// `frame` points at a pointer to write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slx_nicport_tx_next(frame: *mut *const u8) -> i64 {
    let s = served();
    let Some(c) = s.client.as_mut().filter(|c| !c.gone && c.sending.is_none()) else {
        return 0;
    };
    sort_submissions(c, s.mtu, &mut s.lost);
    while let Some(w) = c.sends.pop() {
        // Sorted: inside the data area.
        let Some(at) = c.ring.data_window(w.off as usize, w.len as usize) else {
            complete(c, w.user_data, ERR_INVALID, &mut s.lost);
            continue;
        };
        c.sending = Some(w.user_data);
        // SAFETY: the caller's.
        unsafe { frame.write(at.cast_const()) };
        return i64::from(w.len);
    }
    0
}

/// `slx_nicport_tx_done`: the frame being sent finished: `result` its
/// length, sent, or a negative errno, as the stack is told.
#[unsafe(no_mangle)]
pub extern "C" fn slx_nicport_tx_done(result: i64) {
    let s = served();
    let Some(c) = s.client.as_mut() else {
        return;
    };
    if let Some(token) = c.sending.take() {
        let result = i32::try_from(result).unwrap_or(ERR_INVALID);
        complete(c, token, result, &mut s.lost);
    }
}

/// `slx_nicport_rx_slot`: the slot the next frame received goes into: its
/// capacity, its bytes at `*slot`, or 0 when the stack has none posted --
/// the frame then dropped. [`slx_nicport_rx_done`] says what became of it.
///
/// # Safety
///
/// `slot` points at a pointer to write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slx_nicport_rx_slot(slot: *mut *mut u8) -> i64 {
    let s = served();
    let Some(c) = s.client.as_mut().filter(|c| !c.gone) else {
        return 0;
    };
    if c.receives.is_empty() {
        sort_submissions(c, s.mtu, &mut s.lost);
    }
    let Some(w) = c.receives.front() else {
        return 0;
    };
    // Sorted: inside the data area.
    let Some(at) = c.ring.data_window(w.off as usize, w.len as usize) else {
        return 0;
    };
    c.receiving = true;
    // SAFETY: the caller's.
    unsafe { slot.write(at) };
    i64::from(w.len)
}

/// `slx_nicport_rx_done`: the slot [`slx_nicport_rx_slot`] gave holds a
/// frame of `len` bytes, the stack is told; or, `len` negative, it was not
/// used -- the frame longer than the card takes -- and stays posted.
#[unsafe(no_mangle)]
pub extern "C" fn slx_nicport_rx_done(len: i64) {
    let s = served();
    let Some(c) = s.client.as_mut() else {
        return;
    };
    if !core::mem::take(&mut c.receiving) || len < 0 {
        return;
    }
    if let Some(w) = c.receives.pop() {
        let len = i32::try_from(len.min(i64::from(w.len))).unwrap_or(0);
        complete(c, w.user_data, len, &mut s.lost);
    }
}

/// The loop's idle wait: the connection while a stack is attached -- the
/// driver saying in the ring that it sleeps, so that the stack rings it --
/// the listener while none is. With a submission there already, the wait
/// must not sleep ([`must_not_sleep`]).
pub(crate) fn wait_items(items: &mut [WaitItem]) -> usize {
    let s = served();
    let (handle, kind) = match (s.client.as_mut(), s.listener) {
        (Some(c), _) if !c.gone => {
            c.slept = c.slept.wrapping_add(1).max(1);
            if !c.ring.sq_sleep(c.slept) {
                s.busy = true;
            }
            (c.channel, KIND_CHANNEL)
        }
        (None, Some(listener)) => (listener, KIND_SERVICE),
        _ => return 0,
    };
    let Some(item) = items.first_mut() else {
        return 0;
    };
    *item = WaitItem {
        handle,
        kind,
        events: POLLIN,
        ..WaitItem::default()
    };
    1
}

/// Whether the idle wait about to start must not sleep: the stack submitted
/// something the loop has not yet seen.
pub(crate) fn must_not_sleep() -> bool {
    core::mem::take(&mut served().busy)
}

/// After the wait: the driver awake in the ring again, and what fired
/// looked at in the next turn. A listener the kernel says cannot be waited
/// on is given up, the card no longer served: waiting on it would return
/// at once, every time.
pub(crate) fn waited(items: &[WaitItem]) {
    let s = served();
    if let Some(c) = s.client.as_ref() {
        c.ring.sq_wake();
    }
    for item in items {
        if item.revents == 0 {
            continue;
        }
        s.look = true;
        if item.kind == KIND_SERVICE && item.revents & (POLLERR | POLLNVAL) != 0 {
            log(
                b"[lx] nicport: the card's listener can no longer be waited on; no longer served\n",
            );
            drop_listener(s);
        }
    }
}

/// The card's listener failed: no stack can be accepted.
struct ListenerFailed;

/// The card's listener given up -- closed, its name free again -- and the
/// card no longer served once any stack attached has gone.
fn drop_listener(s: &mut Served) {
    if let Some(listener) = s.listener.take() {
        syscall4(SYS_SERVICE_UNREGISTER, listener, 0, 0, 0);
    }
}

/// A stack that connected, attached: if it holds the card's key, the ring
/// made in a region it is granted, and the attach message sent. `None` with
/// none waiting, or one turned away -- any failure to attach it is said, and
/// its connection closed.
fn accept(listener: u64, mac: [u8; 6], mtu: u16) -> Result<Option<Client>, ListenerFailed> {
    let got = syscall4(SYS_SERVICE_TRY_ACCEPT, listener, 0, 0, 0);
    let Ok(channel) = u64::try_from(got) else {
        return if got == WOULD_BLOCK {
            Ok(None)
        } else {
            Err(ListenerFailed)
        };
    };
    if syscall4(SYS_CHANNEL_PEER_HAS_KEY, channel, 0, 0, 0) != 1 {
        log(b"[lx] nicport: a process without the card's key connected; it was turned away\n");
        turn_away(channel, REFUSED_NO_KEY);
        return Ok(None);
    }
    let port = Port {
        mac,
        mtu,
        slots: SLOTS,
        region: 0,
    };
    let size = port.region_size();
    let Ok(region) = u64::try_from(syscall4(SYS_SHM_CREATE, size as u64, 0, 0, 0)) else {
        log(b"[lx] nicport: no memory for a stack's ring; it was turned away\n");
        turn_away(channel, REFUSED_NO_RING);
        return Ok(None);
    };
    let mapping = syscall4(SYS_SHM_MAP, region, MAP_READ_WRITE, 0, 0);
    // SAFETY: when mapped, the region's `size` bytes at least, page-aligned,
    // which no one else may touch until the stack is told of it below.
    let ring = u64::try_from(mapping).ok().and_then(|at| unsafe {
        Ring::init(
            at as *mut u8,
            size,
            SLOTS,
            SLOTS,
            SLOTS.saturating_mul(SLOT_SIZE),
        )
    });
    let mut message = [0u8; ATTACH_LEN];
    let told = ring.is_some()
        && syscall4(SYS_SHM_GRANT, region, channel, 0, 0) == 0
        && Port { region, ..port }.encode(&mut message) == Some(ATTACH_LEN)
        && syscall4(
            SYS_CHANNEL_SEND,
            channel,
            message.as_ptr() as u64,
            ATTACH_LEN as u64,
            0,
        ) == 0;
    let (Some(ring), true) = (ring, told) else {
        log(b"[lx] nicport: a stack's ring could not be made and given it; it was turned away\n");
        if let Ok(at) = u64::try_from(mapping) {
            syscall4(SYS_SHM_UNMAP, at, size as u64, 0, 0);
        }
        syscall4(SYS_SHM_CLOSE, region, 0, 0, 0);
        turn_away(channel, REFUSED_NO_RING);
        return Ok(None);
    };
    Ok(Some(Client {
        channel,
        region,
        mapping: mapping.cast_unsigned(),
        size: size as u64,
        ring,
        sends: Queue::new(),
        receives: Queue::new(),
        sending: None,
        receiving: false,
        kick: false,
        rung: 0,
        slept: 0,
        told_link: None,
        mcast: None,
        answer: None,
        gone: false,
    }))
}

/// A stack that connected on `channel` told it is not served, and why
/// (`REFUSED_*`), and its connection closed.
fn turn_away(channel: u64, reason: u8) {
    let mut message = [0u8; 2];
    if let Some(n) = encode_refused(reason, &mut message) {
        // Not sent -- the stack gone already, say -- it still sees the
        // connection close.
        let _sent = syscall4(
            SYS_CHANNEL_SEND,
            channel,
            message.as_ptr() as u64,
            n as u64,
            0,
        );
    }
    syscall4(SYS_CHANNEL_CLOSE, channel, 0, 0, 0);
}

/// The stack's messages taken -- [`MESSAGES_A_LOOK`] at most -- and its
/// leaving noticed. Whatever a doorbell rang for is in the ring, read at
/// every turn; a multicast list is kept for the C half. A message the
/// protocol has no place for from a stack -- one too long or unreadable, a
/// driver's, or a list sent before the last was answered -- ends its
/// attachment.
fn take_messages(c: &mut Client) {
    let mut buf = [0u8; MESSAGE_MAX];
    for _ in 0..MESSAGES_A_LOOK {
        let got = syscall4(
            SYS_CHANNEL_TRY_RECV,
            c.channel,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
            0,
        );
        let Ok(len) = usize::try_from(got) else {
            c.gone = true;
            return;
        };
        if len == 0 {
            return;
        }
        match buf.get(..len).and_then(Message::decode) {
            Some(Message::Doorbell) => {}
            Some(Message::Mcast { all, addrs }) if c.mcast.is_none() && c.answer.is_none() => {
                let mut list = McastList {
                    addrs: [[0; 6]; MCAST_MAX],
                    len: 0,
                    all,
                    taken: false,
                };
                // Decoded: at most MCAST_MAX of them.
                list.len = list
                    .addrs
                    .iter_mut()
                    .zip(addrs.chunks_exact(6))
                    .map(|(to, from)| to.copy_from_slice(from))
                    .count();
                c.mcast = Some(list);
            }
            _ => {
                log(b"[lx] nicport: the stack sent a message the protocol has no place for; it is let go\n");
                c.gone = true;
                return;
            }
        }
    }
}

/// `message` sent to the stack: whether it went. A connection closed is
/// the stack gone; one with no room keeps it for a later turn.
fn send_message(c: &mut Client, message: &[u8]) -> bool {
    let sent = syscall4(
        SYS_CHANNEL_SEND,
        c.channel,
        message.as_ptr() as u64,
        message.len() as u64,
        0,
    );
    if sent == CHANNEL_CLOSED {
        c.gone = true;
    }
    sent == 0
}

/// The stack told the card's link is `up`, unless the connection has no
/// room for it yet.
fn tell_link(c: &mut Client, up: bool) {
    let mut message = [0u8; 2];
    let told = encode_link(up, &mut message)
        .is_some_and(|n| send_message(c, message.get(..n).unwrap_or(&[])));
    if told {
        c.told_link = Some(up);
    }
}

/// The stack told how its last multicast list went, if it has not been.
fn tell_answer(c: &mut Client) {
    let Some(result) = c.answer else {
        return;
    };
    let mut message = [0u8; 8];
    let told = encode_mcast_done(result, &mut message)
        .is_some_and(|n| send_message(c, message.get(..n).unwrap_or(&[])));
    if told {
        c.answer = None;
    }
}

/// Completions were pushed: the stack rung if it sleeps waiting for them
/// and has not been rung for that sleep. Awake, it reaps them before it
/// sleeps (`Ring::cq_sleep`); a connection with no room for the doorbell
/// holds one already.
fn ring_if_asleep(c: &mut Client) {
    c.kick = false;
    let sleeper = c.ring.cq_sleeper();
    if sleeper == 0 || sleeper == c.rung {
        return;
    }
    c.rung = sleeper;
    // Not sent, the connection has no room: it holds a doorbell already.
    let _sent = send_message(c, &[DOORBELL]);
}

/// The submissions in the ring taken and sorted -- a queue's worth at most:
/// frames to send and slots to receive into queued, the rest refused at
/// once. A ring the stack wrote broken ends its attachment.
fn sort_submissions(c: &mut Client, mtu: u16, lost: &mut u64) {
    if c.ring.sq_broken() {
        log(b"[lx] nicport: the stack wrote its ring's indices past what the ring holds; it is let go\n");
        c.gone = true;
        return;
    }
    let area = c.ring.data_len();
    for _ in 0..c.ring.sq_entries() {
        let Some(sqe) = c.ring.sq_pop() else {
            return;
        };
        let refused = match frames::sort(&sqe, area, u32::from(mtu)) {
            Sorted::Send(w) => (!c.sends.push(w)).then_some(w.user_data),
            Sorted::Recv(w) => (!c.receives.push(w)).then_some(w.user_data),
            Sorted::Refused(token, errno) => {
                complete(c, token, errno, lost);
                None
            }
        };
        if let Some(token) = refused {
            complete(c, token, ERR_NO_BUFS, lost);
        }
    }
}

/// The submission `token` completed with `result`, the stack's sleep to be
/// looked at. A completion the stack left no room for is counted and
/// dropped.
fn complete(c: &mut Client, token: u64, result: i32, lost: &mut u64) {
    let cqe = Cqe {
        user_data: token,
        result,
        flags: 0,
    };
    if c.ring.cq_push(&cqe) {
        c.kick = true;
    } else {
        *lost = lost.saturating_add(1);
    }
}

/// The attachment ended: the ring unmapped and its region given back --
/// the stack's own mapping and reference keep it until the stack lets go --
/// and the connection closed.
fn detach(c: &Client) {
    syscall4(SYS_SHM_UNMAP, c.mapping, c.size, 0, 0);
    syscall4(SYS_SHM_CLOSE, c.region, 0, 0, 0);
    syscall4(SYS_CHANNEL_CLOSE, c.channel, 0, 0, 0);
}

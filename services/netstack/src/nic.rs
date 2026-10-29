//! The daemon's network card, and every use of it once chosen.
//!
//! The card is chosen once, at start-up: the kernel's NIC, claimed raw
//! (`SYS_NET_RAW_OPEN`), or a card a driver process serves, attached
//! ([`super::port`], design-decisions 1589) -- a serving mode named one.
//! Everything the daemon does with it after goes through here, each call
//! taking the one path for the card chosen: frames sent ([`send`]) and
//! received ([`recv`]), a frame waited for ([`idle`], [`wait_until`]), a
//! client or a frame waited for ([`wait_or_client`]), the multicast filter
//! set ([`set_mcast`]), its link's coming back taken ([`take_link_back`]),
//! the card let go ([`release`]). A frame to the
//! daemon's own address reaches neither card ([`super::loopback`]).

use super::{
    POLL_SLEEP_NS, SYS_NET_RAW_MCAST, SYS_NET_RAW_RX, SYS_NET_RAW_TX, SYS_YIELD, loopback, now_ns,
    port, print, sleep_ns, syscall0, syscall2,
};

/// Whether the card is a driver process's ([`super::port`]) rather than the
/// kernel's NIC.
pub fn over_card() -> bool {
    port::attached()
}

/// Transmit one raw Ethernet frame: its length, or a negative errno.
///
/// A frame destined to *our own IP* never reaches the wire — QEMU's slirp (and a
/// real switch) would drop a packet a host sends to itself — so we divert it into
/// the in-process [`loopback`] RX queue instead. That lets the daemon speak to its
/// own listening sockets: a client connection opened to `me.ip:port` reaches a
/// `Listener` bound on that port entirely within the daemon (the server-socket
/// self-test, and genuine intra-host connections). Diversion is purely by
/// destination IP, so the (otherwise gateway-shaped) Ethernet/next-hop MAC is
/// irrelevant for loopback frames. Non-loopback frames go out the card unchanged.
pub fn send(frame: &[u8]) -> i64 {
    if loopback::maybe_divert(frame) {
        return frame.len() as i64;
    }
    if let Some(sent) = port::send(frame) {
        return sent;
    }
    syscall2(SYS_NET_RAW_TX, frame.as_ptr() as u64, frame.len() as u64)
}

/// Receive one raw Ethernet frame into `buf`. Returns bytes read, or a
/// negative errno ([`super::E_WOULD_BLOCK`] when no frame is queued).
///
/// Any queued [`loopback`] frame is delivered first (ahead of the card) so a
/// self-connection makes progress; only when the loopback queue is empty is
/// the card read.
pub fn recv(buf: &mut [u8]) -> i64 {
    if let Some(n) = loopback::maybe_deliver(buf) {
        return n;
    }
    if let Some(n) = port::recv(buf) {
        return n;
    }
    syscall2(SYS_NET_RAW_RX, buf.as_mut_ptr() as u64, buf.len() as u64)
}

/// Until a frame may have come, [`POLL_SLEEP_NS`] at most: on a driver
/// process's card, its doorbell waited for -- the driver rings once it has
/// filled a slot -- so that a frame is read as it comes; on the kernel's
/// NIC, which has no wait, a sleep. A card gone sleeps too, so that the
/// loops waiting on it end at their deadlines rather than spin.
///
/// Capped so that a loop waiting on a frame still runs its timers -- TCP's
/// retransmissions -- as often as it did when it polled.
pub fn idle() {
    if over_card() && port::wait(POLL_SLEEP_NS) != port::Woke::Gone {
        return;
    }
    sleep_ns(POLL_SLEEP_NS);
}

/// Until a frame may have come, `deadline` at most, as fast as the card
/// allows: on a driver process's card, its doorbell waited for; on the
/// kernel's NIC, which has no wait and whose receive call reads the card's
/// ring itself, the CPU given up once -- quicker than [`idle`]'s sleep, for
/// a wait timed to the frame (`nic-bench`, DHCP). An error when a driver
/// process's card has gone.
pub fn wait_until(deadline: u64) -> Result<(), &'static str> {
    if !over_card() {
        syscall0(SYS_YIELD);
        return Ok(());
    }
    match port::wait(deadline.saturating_sub(now_ns())) {
        port::Woke::Gone => Err("FAIL: the card's driver went away"),
        port::Woke::Rang | port::Woke::Slept => Ok(()),
    }
}

/// Until a frame may have come or a client is waiting on the service
/// `listener` (the handle `SYS_SERVICE_REGISTER` answered), `timeout_ns` at
/// most: on a driver process's card, the card's doorbell and the listener
/// waited on together; on the kernel's NIC, which has no wait, the listener
/// alone. An error when the card has gone, or the wait itself failed.
pub fn wait_or_client(listener: u64, timeout_ns: u64) -> Result<(), &'static str> {
    if !over_card() {
        return super::wait::listener(listener, timeout_ns)
            .map_err(|_| "FAIL: the service's listener could not be waited on");
    }
    match port::wait_or_client(listener, timeout_ns) {
        Ok(port::Woke::Gone) => Err("FAIL: the card's driver went away"),
        Ok(port::Woke::Rang | port::Woke::Slept) => Ok(()),
        Err(_) => Err("FAIL: the card and the service's listener could not be waited on"),
    }
}

/// The multicast addresses the card must pass from now on: the kernel's
/// cards told (`SYS_NET_RAW_MCAST`), or a driver process's card
/// ([`port::set_mcast`], sent once the driver has answered the last list).
/// Whether it was taken -- by the kernel at once, by a card's driver once it
/// answers ([`mcast_refused`]).
pub fn set_mcast(addrs: &[[u8; 6]]) -> bool {
    if over_card() {
        return port::set_mcast(false, addrs);
    }
    syscall2(SYS_NET_RAW_MCAST, addrs.as_ptr() as u64, addrs.len() as u64) >= 0
}

/// Whether a driver process's card refused the last multicast list it
/// answered: for the list to be asked for again at the next change, as one
/// the kernel refused is. Never for the kernel's NIC, which answers at once.
pub fn mcast_refused() -> bool {
    port::mcast_settled().is_some_and(|result| result < 0)
}

/// Whether the multicast filter last set is in force: at once on the
/// kernel's NIC, which answers as it is told; on a driver process's card,
/// once its driver has answered a list, the last without an error.
pub fn mcast_in_force() -> bool {
    !over_card() || port::mcast_settled() == Some(0)
}

/// Whether the card's link is up: a driver process's card's as its driver
/// last told it; the kernel's NIC's always, its link the kernel's own stack's
/// to follow.
pub fn link_up() -> bool {
    !over_card() || port::link() == Some(true)
}

/// Whether a driver process's card's link has come back up since it was
/// told down, since this last answered ([`port::take_link_back`]): for the
/// daemon to renew its lease and announce its addresses. Never for the
/// kernel's NIC, whose link the kernel's own stack follows.
pub fn take_link_back() -> bool {
    port::take_link_back()
}

/// Let go of the card: the kernel's NIC's claim released
/// (`SYS_NET_RAW_CLOSE`), or a driver process's card, `name`, detached.
pub fn release(name: &[u8]) {
    if over_card() {
        port::detach();
        super::Line::new()
            .put(b"[netstack] let ")
            .put(name)
            .put(b" go\n")
            .print();
    } else {
        super::raw_close();
        print("[netstack] released raw NIC\n");
    }
}

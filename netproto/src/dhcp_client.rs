//! A DHCPv4 client's life after its first lease (RFC 2131 §4.4): the lease
//! renewed at T1 from the server that gave it, rebound at T2 from any, given
//! up at its end, and confirmed when the client's link comes back
//! (INIT-REBOOT, §4.3.2). A state machine of time and messages, with no I/O
//! of its own: the caller asks it what to send ([`Client::poll`]), sends
//! that, and hands it the replies ([`Client::on_reply`]); it answers with
//! what happened ([`Event`]). The first lease -- DISCOVER, OFFER, REQUEST,
//! ACK -- is the caller's, and the client starts bound to it.
//!
//! Times are nanoseconds on the caller's monotonic clock.

use crate::dhcp::{self, Message};
use crate::{Ipv4Addr, MacAddr};

const SEC: u64 = 1_000_000_000;
/// The shortest wait between two renewals or rebinds (§4.4.5: "down to a
/// minimum of 60 seconds").
const RETRY_MIN_NS: u64 = 60 * SEC;
/// How many times an INIT-REBOOT request is sent, a second apart, before
/// the unexpired lease is kept unanswered (§3.2).
const REBOOT_TRIES: u32 = 5;
const REBOOT_GAP_NS: u64 = SEC;

/// What a lease holds that the client's life after it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lease {
    /// The client's address.
    pub ip: Ipv4Addr,
    /// The server that gave or last extended it: the one renewing asks.
    pub server: Ipv4Addr,
    /// How long it runs from when it was given, in seconds.
    pub secs: u32,
}

/// Where the client is in its lease's life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Holding the lease, before T1.
    Bound,
    /// Past T1: asking the server that gave it to extend it.
    Renewing,
    /// Past T2: asking any server.
    Rebinding,
    /// The link came back: confirming the lease.
    Rebooting,
    /// The lease is gone -- ended unrenewed, or refused. The address is no
    /// longer the client's.
    Gone,
}

/// What happened to the lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A server extended it (an ACK while renewing or rebinding).
    Renewed,
    /// The server confirmed it after the link came back.
    Confirmed,
    /// No server answered the INIT-REBOOT: the unexpired lease is kept, as
    /// §3.2 lets a client that hears neither ACK nor NAK.
    Kept,
    /// A server refused it (a DHCPNAK, or an ACK for another address): the
    /// address is no longer the client's.
    Refused,
    /// It ran out unrenewed.
    Expired,
}

/// A message [`Client::poll`] built: its length, and where it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Send {
    pub len: usize,
    /// The server it is for (renewing), or `None` for every server
    /// (broadcast: rebinding, rebooting).
    pub to: Option<Ipv4Addr>,
    /// Whether it goes from the client's address (renewing, rebinding),
    /// or from 0.0.0.0 (rebooting: the address is not yet known to be
    /// the client's).
    pub bound: bool,
}

/// What time passing asks of the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Polled {
    /// Nothing yet: ask again by [`Client::next_ns`].
    Idle,
    /// Send the message built.
    Send(Send),
    /// A phase ended without an answer.
    Event(Event),
}

/// A client holding a lease.
#[derive(Clone, Debug)]
pub struct Client {
    mac: MacAddr,
    lease: Lease,
    /// When the lease was given or last extended.
    obtained_ns: u64,
    phase: Phase,
    /// The transaction asked about; a new one each phase.
    xid: u32,
    /// When the next message is due.
    next_ns: u64,
    /// INIT-REBOOT requests sent.
    tries: u32,
}

impl Client {
    /// The client of MAC `mac` bound to `lease`, given at `obtained_ns`;
    /// `xid` seeds its transaction ids.
    #[must_use]
    pub fn new(mac: MacAddr, lease: Lease, obtained_ns: u64, xid: u32) -> Self {
        let mut client = Self {
            mac,
            lease,
            obtained_ns,
            phase: Phase::Bound,
            xid,
            next_ns: 0,
            tries: 0,
        };
        client.next_ns = client.t1();
        client
    }

    /// The lease held.
    #[must_use]
    pub fn lease(&self) -> &Lease {
        &self.lease
    }

    /// Where the lease's life is.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    fn at_fraction(&self, eighths: u64) -> u64 {
        let ns = u64::from(self.lease.secs).saturating_mul(SEC) / 8 * eighths;
        self.obtained_ns.saturating_add(ns)
    }

    /// T1: half the lease, when renewing starts (§4.4.5).
    fn t1(&self) -> u64 {
        self.at_fraction(4)
    }

    /// T2: seven eighths of it, when rebinding starts.
    fn t2(&self) -> u64 {
        self.at_fraction(7)
    }

    /// Its end.
    fn expiry(&self) -> u64 {
        self.at_fraction(8)
    }

    /// Renew the lease from now rather than at T1 -- asked for by whoever
    /// runs the client -- unless it is gone or already being asked about.
    pub fn renew(&mut self, now_ns: u64) {
        if self.phase == Phase::Bound {
            self.begin(Phase::Renewing, now_ns);
        }
    }

    /// The link came back: confirm the lease from now (INIT-REBOOT), what
    /// the link's other end knew of the client perhaps gone with it.
    pub fn reboot(&mut self, now_ns: u64) {
        if self.phase == Phase::Gone {
            return;
        }
        self.phase = Phase::Rebooting;
        self.xid = self.xid.wrapping_add(1);
        self.tries = 0;
        self.next_ns = now_ns;
    }

    /// When [`poll`](Self::poll) next has something to do: a message due, or
    /// a phase's end. `u64::MAX` once the lease is gone.
    #[must_use]
    pub fn next_ns(&self) -> u64 {
        match self.phase {
            Phase::Gone => u64::MAX,
            Phase::Bound => self.t1(),
            Phase::Renewing => self.next_ns.min(self.t2()),
            Phase::Rebinding | Phase::Rebooting => self.next_ns.min(self.expiry()),
        }
    }

    /// Time passing, at `now_ns`: a message to send now, built into `out`;
    /// an event when a phase ended unanswered; or nothing.
    pub fn poll(&mut self, now_ns: u64, out: &mut [u8]) -> Polled {
        if self.phase != Phase::Gone && now_ns >= self.expiry() {
            self.phase = Phase::Gone;
            return Polled::Event(Event::Expired);
        }
        if self.phase == Phase::Bound && now_ns >= self.t1() {
            self.begin(Phase::Renewing, now_ns);
        }
        if self.phase == Phase::Renewing && now_ns >= self.t2() {
            self.begin(Phase::Rebinding, now_ns);
        }
        if self.phase == Phase::Gone || self.phase == Phase::Bound || now_ns < self.next_ns {
            return Polled::Idle;
        }
        let (len, to, bound) = match self.phase {
            Phase::Renewing => {
                self.next_ns = now_ns.saturating_add(half_until(now_ns, self.t2()));
                (
                    dhcp::build_renewal(out, self.xid, &self.mac, &self.lease.ip),
                    Some(self.lease.server),
                    true,
                )
            }
            Phase::Rebinding => {
                self.next_ns = now_ns.saturating_add(half_until(now_ns, self.expiry()));
                (
                    dhcp::build_renewal(out, self.xid, &self.mac, &self.lease.ip),
                    None,
                    true,
                )
            }
            Phase::Rebooting if self.tries >= REBOOT_TRIES => {
                // Unanswered: the unexpired lease is kept, its timers as
                // they were.
                self.phase = Phase::Bound;
                self.next_ns = self.t1();
                return Polled::Event(Event::Kept);
            }
            Phase::Rebooting => {
                self.tries += 1;
                self.next_ns = now_ns.saturating_add(REBOOT_GAP_NS);
                (
                    dhcp::build_request(out, self.xid, &self.mac, &self.lease.ip, None),
                    None,
                    false,
                )
            }
            Phase::Bound | Phase::Gone => return Polled::Idle,
        };
        len.map_or(Polled::Idle, |len| Polled::Send(Send { len, to, bound }))
    }

    fn begin(&mut self, phase: Phase, now_ns: u64) {
        self.phase = phase;
        self.xid = self.xid.wrapping_add(1);
        self.next_ns = now_ns;
    }

    /// A DHCP reply that came at `now_ns`: what it did to the lease, if it
    /// answered the transaction asked about. An ACK extends or confirms the
    /// lease -- its time and server as it says, from now -- unless it is
    /// for another address; a NAK, or an ACK for another address, refuses
    /// it.
    pub fn on_reply(&mut self, now_ns: u64, msg: &Message<'_>) -> Option<Event> {
        let asking = matches!(
            self.phase,
            Phase::Renewing | Phase::Rebinding | Phase::Rebooting
        );
        if !asking || msg.op != dhcp::OP_REPLY || msg.xid != self.xid {
            return None;
        }
        match msg.msg_type() {
            Some(dhcp::NAK) => {
                self.phase = Phase::Gone;
                Some(Event::Refused)
            }
            Some(dhcp::ACK) if msg.yiaddr != self.lease.ip => {
                self.phase = Phase::Gone;
                Some(Event::Refused)
            }
            Some(dhcp::ACK) => {
                let event = if self.phase == Phase::Rebooting {
                    Event::Confirmed
                } else {
                    Event::Renewed
                };
                self.lease.secs = msg.lease_secs().unwrap_or(self.lease.secs);
                self.lease.server = msg.server_id().unwrap_or(self.lease.server);
                self.obtained_ns = now_ns;
                self.phase = Phase::Bound;
                self.next_ns = self.t1();
                Some(event)
            }
            _ => None,
        }
    }
}

/// Half the time from `now_ns` until `until_ns`, a minute at least: when a
/// renewal or rebind unanswered is asked again (§4.4.5).
fn half_until(now_ns: u64, until_ns: u64) -> u64 {
    (until_ns.saturating_sub(now_ns) / 2).max(RETRY_MIN_NS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: MacAddr = [0x52, 0x54, 0x00, 0x12, 0x34, 0x59];
    const IP: Ipv4Addr = [10, 0, 2, 15];
    const SERVER: Ipv4Addr = [10, 0, 2, 2];
    /// A lease of 800 s: T1 at 400 s, T2 at 700 s, its end at 800 s.
    const LEASE: Lease = Lease {
        ip: IP,
        server: SERVER,
        secs: 800,
    };

    fn client() -> Client {
        Client::new(MAC, LEASE, 0, 100)
    }

    /// A reply of type `kind` to transaction `xid`, for `yiaddr`, giving
    /// `secs`.
    fn reply(kind: u8, xid: u32, yiaddr: Ipv4Addr, secs: u32) -> [u8; 300] {
        let mut b = [0u8; 300];
        b[0] = dhcp::OP_REPLY;
        b[1] = 1;
        b[2] = 6;
        b[4..8].copy_from_slice(&xid.to_be_bytes());
        b[16..20].copy_from_slice(&yiaddr);
        b[236..240].copy_from_slice(&dhcp::MAGIC_COOKIE.to_be_bytes());
        let opts = [
            dhcp::OPT_MSG_TYPE,
            1,
            kind,
            dhcp::OPT_LEASE_TIME,
            4,
            (secs >> 24) as u8,
            (secs >> 16) as u8,
            (secs >> 8) as u8,
            secs as u8,
            dhcp::OPT_SERVER_ID,
            4,
            SERVER[0],
            SERVER[1],
            SERVER[2],
            SERVER[3],
            255,
        ];
        b[240..240 + opts.len()].copy_from_slice(&opts);
        b
    }

    fn sent(c: &mut Client, at: u64) -> (Send, u32, [u8; 300]) {
        let mut out = [0u8; 300];
        match c.poll(at, &mut out) {
            Polled::Send(s) => {
                let xid = Message::parse(&out[..s.len]).unwrap().xid;
                (s, xid, out)
            }
            other => panic!("expected a send at {at}, got {other:?}"),
        }
    }

    #[test]
    fn a_bound_client_waits_for_t1_then_asks_its_server() {
        let mut c = client();
        let mut out = [0u8; 300];
        assert_eq!(c.poll(399 * SEC, &mut out), Polled::Idle);
        assert_eq!(c.next_ns(), 400 * SEC);
        let (s, _, msg) = sent(&mut c, 400 * SEC);
        assert_eq!(s.to, Some(SERVER), "renewing asks the server that gave it");
        assert!(s.bound, "from the address held");
        assert_eq!(c.phase(), Phase::Renewing);
        assert_eq!(&msg[12..16], &IP, "ciaddr is the address held");
    }

    #[test]
    fn an_unanswered_renewal_is_asked_again_halfway_to_t2_and_a_minute_at_least() {
        let mut c = client();
        let _ = sent(&mut c, 400 * SEC);
        // Half of the 300 s to T2: 150 s on.
        assert_eq!(c.next_ns(), 550 * SEC);
        assert_eq!(c.poll(549 * SEC, &mut [0u8; 300]), Polled::Idle);
        let _ = sent(&mut c, 550 * SEC);
        // Half of 150 s is 75 s; then the minute floor.
        assert_eq!(c.next_ns(), 625 * SEC);
        let _ = sent(&mut c, 625 * SEC);
        assert_eq!(c.next_ns(), 685 * SEC, "a minute, not 37.5 s");
    }

    #[test]
    fn past_t2_it_asks_every_server_and_at_its_end_the_lease_is_gone() {
        let mut c = client();
        let _ = sent(&mut c, 400 * SEC);
        let (s, _, _) = sent(&mut c, 700 * SEC);
        assert_eq!(s.to, None, "rebinding broadcasts");
        assert_eq!(c.phase(), Phase::Rebinding);
        assert_eq!(
            c.poll(800 * SEC, &mut [0u8; 300]),
            Polled::Event(Event::Expired)
        );
        assert_eq!(c.phase(), Phase::Gone);
        assert_eq!(c.next_ns(), u64::MAX);
        assert_eq!(c.poll(900 * SEC, &mut [0u8; 300]), Polled::Idle);
    }

    #[test]
    fn an_ack_extends_the_lease_from_when_it_came() {
        let mut c = client();
        let (_, xid, _) = sent(&mut c, 400 * SEC);
        let ack = reply(dhcp::ACK, xid, IP, 1000);
        let msg = Message::parse(&ack).unwrap();
        assert_eq!(c.on_reply(410 * SEC, &msg), Some(Event::Renewed));
        assert_eq!(c.phase(), Phase::Bound);
        assert_eq!(c.lease().secs, 1000);
        // T1 is now 410 s + 500 s.
        assert_eq!(c.next_ns(), 910 * SEC);
    }

    #[test]
    fn a_nak_or_an_ack_for_another_address_refuses_the_lease() {
        let mut c = client();
        let (_, xid, _) = sent(&mut c, 400 * SEC);
        let nak = reply(dhcp::NAK, xid, [0; 4], 0);
        assert_eq!(
            c.on_reply(401 * SEC, &Message::parse(&nak).unwrap()),
            Some(Event::Refused)
        );
        assert_eq!(c.phase(), Phase::Gone);

        let mut c = client();
        let (_, xid, _) = sent(&mut c, 400 * SEC);
        let moved = reply(dhcp::ACK, xid, [10, 0, 2, 16], 800);
        assert_eq!(
            c.on_reply(401 * SEC, &Message::parse(&moved).unwrap()),
            Some(Event::Refused)
        );
    }

    #[test]
    fn a_reply_to_another_transaction_or_while_bound_changes_nothing() {
        let mut c = client();
        let stray = reply(dhcp::ACK, 7, IP, 5);
        assert_eq!(
            c.on_reply(1, &Message::parse(&stray).unwrap()),
            None,
            "bound: asks nothing"
        );
        let (_, xid, _) = sent(&mut c, 400 * SEC);
        let other = reply(dhcp::NAK, xid.wrapping_add(1), [0; 4], 0);
        assert_eq!(
            c.on_reply(401 * SEC, &Message::parse(&other).unwrap()),
            None
        );
        assert_eq!(c.phase(), Phase::Renewing);
    }

    #[test]
    fn a_link_back_confirms_the_lease_by_init_reboot() {
        let mut c = client();
        c.reboot(10 * SEC);
        let (s, xid, msg) = sent(&mut c, 10 * SEC);
        assert_eq!(s.to, None, "INIT-REBOOT broadcasts");
        assert!(
            !s.bound,
            "from 0.0.0.0: the address is not yet known to be ours"
        );
        let m = Message::parse(&msg[..s.len]).unwrap();
        assert_eq!(m.option(dhcp::OPT_REQUESTED_IP), Some(&IP[..]));
        assert_eq!(m.server_id(), None);
        assert_eq!(&msg[12..16], &[0; 4], "ciaddr stays zero");
        let ack = reply(dhcp::ACK, xid, IP, 800);
        assert_eq!(
            c.on_reply(11 * SEC, &Message::parse(&ack).unwrap()),
            Some(Event::Confirmed)
        );
        assert_eq!(c.phase(), Phase::Bound);
    }

    #[test]
    fn an_unanswered_init_reboot_keeps_the_lease_after_five_tries() {
        let mut c = client();
        c.reboot(10 * SEC);
        for i in 0..5 {
            let _ = sent(&mut c, (10 + i) * SEC);
            assert_eq!(c.poll((10 + i) * SEC + 1, &mut [0u8; 300]), Polled::Idle);
        }
        assert_eq!(
            c.poll(15 * SEC, &mut [0u8; 300]),
            Polled::Event(Event::Kept)
        );
        assert_eq!(c.phase(), Phase::Bound);
        assert_eq!(c.next_ns(), 400 * SEC, "the lease's own timers resume");
    }

    #[test]
    fn a_gone_lease_is_not_rebooted() {
        let mut c = client();
        assert_eq!(
            c.poll(800 * SEC, &mut [0u8; 300]),
            Polled::Event(Event::Expired)
        );
        c.reboot(801 * SEC);
        assert_eq!(c.phase(), Phase::Gone);
    }

    #[test]
    fn a_renewal_asked_for_starts_at_once_and_only_when_bound() {
        let mut c = client();
        c.renew(5 * SEC);
        assert_eq!(c.phase(), Phase::Renewing);
        let (s, _, _) = sent(&mut c, 5 * SEC);
        assert_eq!(s.to, Some(SERVER));
        c.reboot(6 * SEC);
        c.renew(6 * SEC);
        assert_eq!(
            c.phase(),
            Phase::Rebooting,
            "a confirmation under way is not cut short"
        );
    }

    #[test]
    fn a_message_with_no_room_is_not_sent() {
        let mut c = client();
        assert_eq!(c.poll(400 * SEC, &mut [0u8; 10]), Polled::Idle);
    }
}

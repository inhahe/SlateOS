//! TCP retransmission timing: when an unacknowledged segment is due to be sent
//! again, and when to stop trying.
//!
//! The policy for the netstack daemon's sender, which keeps at most one
//! segment in flight per connection. It is RFC 6298's shape without the
//! round-trip estimator: a fixed initial timeout that doubles on every resend
//! (§5.5, "back off the timer"), and a bounded number of resends after which
//! the peer is presumed gone and the connection times out (RFC 1122 §4.2.3.5's
//! "R2", much shortened for a stack whose peers are a LAN or QEMU's slirp).
//!
//! Split out of the daemon so the decision can be tested on the host: the
//! daemon supplies the clock and does the sending, and this module only
//! decides. Before it existed the daemon's one retransmit rule lived inside
//! its *blocking* receive and send loops, keyed to a counter local to one call
//! (resend at the 40th idle 5 ms poll, at most three times), so nothing resent
//! a lost segment unless some caller happened to be blocked on that very
//! connection -- which is what kept the kernel from waiting for TCP data
//! itself (known-issues.md `A-BLOCKING-TCP-RECV-REPORTS-EOF-AFTER-2S`).

/// First retransmission timeout, in nanoseconds.
///
/// RFC 6298 recommends 1 s for a path whose round trip has not been measured.
/// 200 ms is what the daemon's previous rule used, and its peers are a LAN or
/// QEMU's slirp, whose round trips are microseconds to milliseconds.
pub const INITIAL_RTO_NS: u64 = 200_000_000;

/// Resends of one segment before the connection times out.
///
/// With [`INITIAL_RTO_NS`] doubling, the resends go out 0.2, 0.6, 1.4, 3.0 and
/// 6.2 s after the original send, and the connection is declared timed out at
/// 12.6 s.
pub const MAX_RETRANSMITS: u32 = 5;

/// What to do about the segment in flight, at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtxAction {
    /// Nothing: no segment is in flight, or its timeout has not expired.
    Wait,
    /// Send the in-flight segment again, now.
    Resend,
    /// Every resend has gone unanswered: the connection has timed out.
    GiveUp,
}

/// The retransmission state of one connection's single in-flight segment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Retransmit {
    /// When the in-flight segment was last sent or resent; `None` when nothing
    /// is in flight.
    sent_ns: Option<u64>,
    /// Resends of the in-flight segment so far.
    retries: u32,
}

impl Retransmit {
    /// Nothing in flight.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sent_ns: None,
            retries: 0,
        }
    }

    /// A new segment went out at `now_ns`: its timer starts afresh.
    pub fn sent(&mut self, now_ns: u64) {
        self.sent_ns = Some(now_ns);
        self.retries = 0;
    }

    /// The in-flight segment was acknowledged: nothing left to time.
    pub fn acked(&mut self) {
        self.sent_ns = None;
        self.retries = 0;
    }

    /// Whether a segment is in flight.
    #[must_use]
    pub const fn in_flight(&self) -> bool {
        self.sent_ns.is_some()
    }

    /// Resends of the in-flight segment so far.
    #[must_use]
    pub const fn retries(&self) -> u32 {
        self.retries
    }

    /// The timeout in force: [`INITIAL_RTO_NS`], doubled once per resend.
    #[must_use]
    pub fn rto_ns(&self) -> u64 {
        INITIAL_RTO_NS.saturating_mul(1u64 << self.retries.min(MAX_RETRANSMITS))
    }

    /// What to do at `now_ns`.
    ///
    /// [`RtxAction::Resend`] is recorded before it is returned -- the timer
    /// restarts from `now_ns`, doubled -- so the caller must then actually
    /// resend, and asking twice in one instant resends once. A clock reading
    /// earlier than the send is treated as no time elapsed rather than as a
    /// huge one. [`RtxAction::GiveUp`] is final: it is returned for every later
    /// instant until the segment is [`acked`](Self::acked) or a new one
    /// [`sent`](Self::sent).
    pub fn poll(&mut self, now_ns: u64) -> RtxAction {
        let Some(sent) = self.sent_ns else {
            return RtxAction::Wait;
        };
        if now_ns.saturating_sub(sent) < self.rto_ns() {
            return RtxAction::Wait;
        }
        if self.retries >= MAX_RETRANSMITS {
            return RtxAction::GiveUp;
        }
        self.retries += 1;
        self.sent_ns = Some(now_ns);
        RtxAction::Resend
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn nothing_in_flight_never_resends() {
        let mut r = Retransmit::new();
        assert!(!r.in_flight());
        assert_eq!(r.poll(0), RtxAction::Wait);
        assert_eq!(r.poll(u64::MAX), RtxAction::Wait);
    }

    #[test]
    fn a_segment_is_resent_exactly_at_its_timeout() {
        let mut r = Retransmit::new();
        r.sent(1_000 * MS);
        assert!(r.in_flight());
        assert_eq!(r.poll(1_000 * MS + INITIAL_RTO_NS - 1), RtxAction::Wait);
        assert_eq!(r.poll(1_000 * MS + INITIAL_RTO_NS), RtxAction::Resend);
        assert_eq!(r.retries(), 1);
    }

    #[test]
    fn asking_twice_in_one_instant_resends_once() {
        let mut r = Retransmit::new();
        r.sent(0);
        assert_eq!(r.poll(INITIAL_RTO_NS), RtxAction::Resend);
        assert_eq!(r.poll(INITIAL_RTO_NS), RtxAction::Wait);
    }

    #[test]
    fn the_timeout_doubles_and_the_connection_times_out_at_twelve_point_six_seconds() {
        let mut r = Retransmit::new();
        r.sent(0);
        // Resends at 0.2, 0.6, 1.4, 3.0, 6.2 s, each just after its timeout.
        let mut resent_at = [0u64; MAX_RETRANSMITS as usize];
        let mut now = 0;
        let mut i = 0;
        while i < resent_at.len() {
            now += MS;
            match r.poll(now) {
                RtxAction::Wait => {}
                RtxAction::Resend => {
                    resent_at[i] = now;
                    i += 1;
                }
                RtxAction::GiveUp => panic!("gave up after {i} resends"),
            }
        }
        assert_eq!(
            resent_at,
            [200 * MS, 600 * MS, 1_400 * MS, 3_000 * MS, 6_200 * MS]
        );
        assert_eq!(r.poll(12_600 * MS - 1), RtxAction::Wait);
        assert_eq!(r.poll(12_600 * MS), RtxAction::GiveUp);
    }

    #[test]
    fn giving_up_is_final() {
        let mut r = Retransmit::new();
        r.sent(0);
        let mut now = 0;
        while r.poll(now) != RtxAction::GiveUp {
            now += 10 * MS;
        }
        assert_eq!(r.poll(now), RtxAction::GiveUp);
        assert_eq!(r.poll(u64::MAX), RtxAction::GiveUp);
    }

    #[test]
    fn an_ack_stops_the_timer_and_a_new_send_restarts_it() {
        let mut r = Retransmit::new();
        r.sent(0);
        assert_eq!(r.poll(INITIAL_RTO_NS), RtxAction::Resend);
        r.acked();
        assert!(!r.in_flight());
        assert_eq!(r.retries(), 0);
        assert_eq!(r.poll(u64::MAX), RtxAction::Wait);
        r.sent(10_000 * MS);
        assert_eq!(r.rto_ns(), INITIAL_RTO_NS);
        assert_eq!(r.poll(10_000 * MS + INITIAL_RTO_NS), RtxAction::Resend);
    }

    #[test]
    fn a_clock_behind_the_send_is_no_time_at_all() {
        let mut r = Retransmit::new();
        r.sent(5_000 * MS);
        assert_eq!(r.poll(0), RtxAction::Wait);
        assert_eq!(r.poll(4_999 * MS), RtxAction::Wait);
    }

    #[test]
    fn a_send_near_the_end_of_the_clock_does_not_overflow() {
        let mut r = Retransmit::new();
        r.sent(u64::MAX - 10);
        assert_eq!(r.poll(u64::MAX), RtxAction::Wait);
        // And the timeout itself stays finite however many resends.
        assert!(r.rto_ns() >= INITIAL_RTO_NS);
    }
}

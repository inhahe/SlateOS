//! The two numbers a new TCP connection picks for itself: its ephemeral local
//! port (RFC 6056) and its initial sequence number (RFC 6528).
//!
//! Both must differ from the previous connection to the same peer, or the
//! peer -- or a NAT between -- can take the new connection's SYN for a
//! retransmission of the old one's and answer it with the old connection's
//! state: a reset, or an ACK for the wrong sequence. Both must also be
//! unpredictable to anyone who cannot see the traffic, or an off-path attacker
//! can guess them and inject segments.
//!
//! The netstack daemon used to derive both from one 16-bit counter that every
//! new ring session reseeded from its control-request count. Two sessions a
//! multiple of 16 requests apart gave their first connections the same port and
//! the same ISN: a byte-identical SYN for a connection that had just closed.
//! Both answers here are keyed hashes of the connection's addresses under a
//! secret drawn at start-up ([`crate::siphash`]).

use crate::siphash::siphash24;

/// A connection's addresses, as the keyed functions below read them.
///
/// Addresses are their raw bytes: 4 for IPv4, 16 for IPv6. The lengths are
/// hashed along with the bytes, so a v4 tuple and a v6 tuple never collide by
/// construction.
#[derive(Debug, Clone, Copy)]
pub struct Tuple<'a> {
    /// Our address.
    pub local_ip: &'a [u8],
    /// Our port.
    pub local_port: u16,
    /// The peer's address.
    pub remote_ip: &'a [u8],
    /// The peer's port.
    pub remote_port: u16,
}

/// Longest encoding [`hash_of`] feeds the hash: two length bytes, two
/// 16-byte addresses and two ports.
const MAX_ENCODED: usize = 1 + 16 + 2 + 1 + 16 + 2;

/// Keyed hash of `local_ip`, optionally `local_port`, `remote_ip` and
/// `remote_port`. An address longer than 16 bytes is cut to 16.
fn hash_of(key: &[u8; 16], t: &Tuple<'_>, with_local_port: bool) -> u64 {
    let mut buf = [0u8; MAX_ENCODED];
    let mut len = 0usize;
    let mut put = |bytes: &[u8]| {
        for &b in bytes {
            if let Some(slot) = buf.get_mut(len) {
                *slot = b;
                len += 1;
            }
        }
    };
    let local = t.local_ip.get(..16).unwrap_or(t.local_ip);
    let remote = t.remote_ip.get(..16).unwrap_or(t.remote_ip);
    #[allow(clippy::cast_possible_truncation)] // both lengths are at most 16
    put(&[local.len() as u8]);
    put(local);
    if with_local_port {
        put(&t.local_port.to_be_bytes());
    }
    #[allow(clippy::cast_possible_truncation)]
    put(&[remote.len() as u8]);
    put(remote);
    put(&t.remote_port.to_be_bytes());
    siphash24(key, buf.get(..len).unwrap_or(&buf))
}

/// RFC 6528's initial sequence number: a clock that advances every 4 µs, plus
/// a keyed hash of the connection's four addresses.
///
/// The clock makes a new incarnation of a 4-tuple start above the old one's
/// sequence space (RFC 6191 relies on this to reopen a pair in TIME_WAIT); the
/// hash keeps the starting point secret, and different for every pair.
#[must_use]
pub fn isn(clock_ns: u64, key: &[u8; 16], t: &Tuple<'_>) -> u32 {
    #[allow(clippy::cast_possible_truncation)] // the 4 µs clock wraps, as RFC 793's does
    let m = (clock_ns / 4_000) as u32;
    #[allow(clippy::cast_possible_truncation)] // any 32 bits of the hash will do
    let f = hash_of(key, t, true) as u32;
    m.wrapping_add(f)
}

/// First port of the IANA dynamic range, which RFC 6056 §3.2 says ephemeral
/// ports should be drawn from.
pub const EPHEMERAL_MIN: u16 = 49152;

/// Last port of the dynamic range.
pub const EPHEMERAL_MAX: u16 = 65535;

/// Ephemeral-port selection, RFC 6056 §3.3.3 ("Algorithm 3: Simple
/// Hash-Based Port Selection").
///
/// Each destination gets its own keyed offset into the range, and one shared
/// counter advances past every port handed out. Successive connections to one
/// peer therefore walk the whole range before reusing a port, while
/// connections to different peers do not reveal each other's ports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EphemeralPorts {
    /// The counter RFC 6056 calls `next_ephemeral`.
    next: u32,
}

impl EphemeralPorts {
    /// A selector whose counter starts at 0.
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// A selector resuming from a counter kept elsewhere, as by
    /// [`counter`](Self::counter). For a caller that keeps the one
    /// system-wide counter RFC 6056 describes in its own storage, such as an
    /// atomic.
    #[must_use]
    pub const fn from_counter(next: u32) -> Self {
        Self { next }
    }

    /// The counter, for storing and resuming with
    /// [`from_counter`](Self::from_counter).
    #[must_use]
    pub const fn counter(&self) -> u32 {
        self.next
    }

    /// A port for a connection to `remote_ip`:`remote_port` from `local_ip`,
    /// skipping any for which `in_use` answers true. `None` when every port in
    /// the range is in use.
    ///
    /// `t.local_port` is ignored: it is what is being chosen.
    pub fn pick(
        &mut self,
        key: &[u8; 16],
        t: &Tuple<'_>,
        mut in_use: impl FnMut(u16) -> bool,
    ) -> Option<u16> {
        let span = u32::from(EPHEMERAL_MAX - EPHEMERAL_MIN) + 1;
        #[allow(clippy::cast_possible_truncation)] // reduced modulo `span` below
        let offset = hash_of(key, t, false) as u32;
        for _ in 0..span {
            let slot = self.next.wrapping_add(offset) % span;
            self.next = self.next.wrapping_add(1);
            // `slot < span`, so the sum is at most EPHEMERAL_MAX.
            let port = u16::try_from(u32::from(EPHEMERAL_MIN) + slot).ok()?;
            if !in_use(port) {
                return Some(port);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 16] = *b"slateos-test-key";
    const ME: [u8; 4] = [10, 0, 2, 15];
    const PEER: [u8; 4] = [172, 66, 147, 243];

    fn to_peer(port: u16) -> Tuple<'static> {
        Tuple {
            local_ip: &ME,
            local_port: port,
            remote_ip: &PEER,
            remote_port: 80,
        }
    }

    #[test]
    fn successive_connections_to_one_peer_get_different_ports() {
        let mut ports = EphemeralPorts::new();
        let a = ports.pick(&KEY, &to_peer(0), |_| false).unwrap();
        let b = ports.pick(&KEY, &to_peer(0), |_| false).unwrap();
        assert_ne!(a, b);
        assert!((EPHEMERAL_MIN..=EPHEMERAL_MAX).contains(&a));
        assert!((EPHEMERAL_MIN..=EPHEMERAL_MAX).contains(&b));
    }

    #[test]
    fn one_peer_sees_the_whole_range_before_a_port_repeats() {
        let mut ports = EphemeralPorts::new();
        let span = usize::from(EPHEMERAL_MAX - EPHEMERAL_MIN) + 1;
        let mut seen = vec![false; span];
        for _ in 0..span {
            let p = ports.pick(&KEY, &to_peer(0), |_| false).unwrap();
            let i = usize::from(p - EPHEMERAL_MIN);
            assert!(!seen[i], "port {p} repeated before the range was used up");
            seen[i] = true;
        }
    }

    #[test]
    fn a_port_in_use_is_skipped() {
        let mut probe = EphemeralPorts::new();
        let first = probe.pick(&KEY, &to_peer(0), |_| false).unwrap();
        let mut ports = EphemeralPorts::new();
        let got = ports.pick(&KEY, &to_peer(0), |p| p == first).unwrap();
        assert_ne!(got, first);
    }

    #[test]
    fn a_counter_kept_elsewhere_resumes_where_it_left_off() {
        let mut whole = EphemeralPorts::new();
        let a = whole.pick(&KEY, &to_peer(0), |_| false).unwrap();
        let b = whole.pick(&KEY, &to_peer(0), |_| false).unwrap();

        let mut first = EphemeralPorts::new();
        assert_eq!(first.pick(&KEY, &to_peer(0), |_| false), Some(a));
        let mut resumed = EphemeralPorts::from_counter(first.counter());
        assert_eq!(resumed.pick(&KEY, &to_peer(0), |_| false), Some(b));
    }

    #[test]
    fn a_full_range_is_none_not_a_loop() {
        let mut ports = EphemeralPorts::new();
        assert_eq!(ports.pick(&KEY, &to_peer(0), |_| true), None);
    }

    #[test]
    fn the_isn_advances_with_the_clock_for_one_tuple() {
        let t = to_peer(50000);
        let a = isn(1_000_000, &KEY, &t);
        let b = isn(1_000_000 + 4_000 * 1000, &KEY, &t);
        assert_eq!(b.wrapping_sub(a), 1000);
    }

    #[test]
    fn different_tuples_start_at_different_isns() {
        let a = isn(0, &KEY, &to_peer(50000));
        let b = isn(0, &KEY, &to_peer(50001));
        assert_ne!(a, b);
    }

    #[test]
    fn the_key_is_what_makes_the_isn_unpredictable() {
        let t = to_peer(50000);
        let mut other = KEY;
        other[15] ^= 0x80;
        assert_ne!(isn(0, &KEY, &t), isn(0, &other, &t));
    }

    #[test]
    fn a_v4_and_a_v6_tuple_do_not_share_an_encoding() {
        let v6 = [0u8; 16];
        let a = Tuple {
            local_ip: &ME,
            local_port: 1,
            remote_ip: &PEER,
            remote_port: 2,
        };
        let b = Tuple {
            local_ip: &v6,
            local_port: 1,
            remote_ip: &PEER,
            remote_port: 2,
        };
        assert_ne!(isn(0, &KEY, &a), isn(0, &KEY, &b));
    }
}

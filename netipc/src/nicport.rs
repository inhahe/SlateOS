//! A network card served by a driver process (design-decisions 1589): the
//! attach message, the ring's layout, and the doorbell, shared by the driver
//! process's kit (`linuxdrv/kit`) and the network stack (`services/netstack`).
//!
//! The driver process registers its card as the service `nic.<name>`. A
//! stack connects; the driver makes a shared region holding a [`crate::ring`]
//! -- submission and completion queues of [`Port::slots`] entries each, and a
//! data area cut into that many slots of [`SLOT_SIZE`] bytes -- lets the
//! stack map it (`SYS_SHM_GRANT`), and sends the attach message
//! ([`Port::encode`]), which names the region.
//!
//! On the ring the stack submits and owns the slots: `OP_SEND` names a slot
//! holding a frame to send (`data_off`, `data_len`), `OP_RECV` posts an empty
//! slot for a frame to be received into (`data_off`, `data_len` its
//! capacity, which must hold the longest frame: header, VLAN tag and MTU).
//! The driver completes every submission with its `user_data` -- sends in
//! the order submitted, with the length sent or a negative errno; receives
//! in the order posted, as frames come, with the frame's length.
//!
//! A side about to sleep waiting for the other says so in the ring (its
//! queue's sleep word, `ring::OFF_SQ_SLEEP` / `OFF_CQ_SLEEP`); the other,
//! having pushed, sends [`DOORBELL`] on the connection only then, once for
//! that sleep. The stack rings after frames to send, not after slots posted
//! to receive into, which the driver looks for when a frame comes.
//!
//! Besides doorbells the connection carries [`Message`]s, each a channel
//! message of its own, its first byte its kind:
//!
//! - a driver that will not serve a stack says why ([`Message::Refused`])
//!   in place of the attach message, and closes the connection;
//! - the driver tells the stack the card's link ([`Message::Link`]) right
//!   after the attach message and again whenever it changes;
//! - the stack gives the driver the multicast addresses the card must pass
//!   ([`Message::Mcast`]), which replace the last it gave, and the driver
//!   answers each, once it is in force or has failed
//!   ([`Message::McastDone`]). One at a time: the stack sends the next only
//!   once the last is answered; one sent before then breaks the protocol.
//!
//! A side reads every message into a buffer of [`MESSAGE_MAX`] bytes; one
//! that is longer, or that [`Message::decode`] refuses, the peer has broken
//! the protocol with.

/// The prefix of a card's service name.
pub const SERVICE_PREFIX: &[u8] = b"nic.";

/// A slot's size: a whole Ethernet frame with its header, 1518 bytes, and
/// room to spare.
pub const SLOT_SIZE: u32 = 2048;

/// The doorbell: one byte, sent on the connection after pushing to a side
/// that said it sleeps.
pub const DOORBELL: u8 = b'D';

/// The attach message's magic ("NPRT") and version: 2 since the connection
/// carries [`Message`]s besides doorbells.
pub const MAGIC: u32 = 0x4E50_5254;
pub const VERSION: u32 = 2;

/// The kinds of [`Message`]: their first byte.
const KIND_REFUSED: u8 = b'R';
const KIND_LINK: u8 = b'L';
const KIND_MCAST: u8 = b'M';
const KIND_MCAST_DONE: u8 = b'm';

/// Why a driver refuses a stack ([`Message::Refused`]): it holds no key for
/// the card -- the `Service` capability for its name, with `READ`.
pub const REFUSED_NO_KEY: u8 = 1;
/// ... the driver could not make the ring, or give it to the stack.
pub const REFUSED_NO_RING: u8 = 2;

/// [`Message::Mcast`]'s flag: pass every multicast frame, whatever the list.
const MCAST_ALL: u8 = 1;

/// The most addresses one [`Message::Mcast`] lists. A stack in more groups
/// asks for every multicast frame instead, as a card with too small a
/// filter passes them all.
pub const MCAST_MAX: usize = 64;

/// The longest message on the connection: a [`Message::Mcast`] listing
/// [`MCAST_MAX`] addresses.
pub const MESSAGE_MAX: usize = 4 + 6 * MCAST_MAX;

/// A message on the connection, the attach message aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message<'a> {
    /// The peer pushed to a queue its side said it sleeps on.
    Doorbell,
    /// Driver to stack, in place of the attach message: it will not serve
    /// this stack, for `reason` ([`REFUSED_NO_KEY`], [`REFUSED_NO_RING`], or
    /// one a later driver adds), and closes the connection.
    Refused { reason: u8 },
    /// Driver to stack: the card's link is up, or down.
    Link { up: bool },
    /// Stack to driver: the multicast addresses the card must pass from now
    /// on -- every one, with `all` -- the six bytes of each in turn, each a
    /// multicast address.
    Mcast { all: bool, addrs: &'a [u8] },
    /// Driver to stack: the last [`Message::Mcast`] is in force (0), or
    /// failed (a negative errno) -- the card then passing none of its
    /// addresses, as if it had listed none.
    McastDone { result: i32 },
}

impl<'a> Message<'a> {
    /// The message read from `b`, the whole of one channel message: `None`
    /// for an unknown kind, a wrong length, or a list naming more than
    /// [`MCAST_MAX`] addresses, or one that is not a multicast address.
    #[must_use]
    pub fn decode(b: &'a [u8]) -> Option<Self> {
        match *b {
            [DOORBELL] => Some(Self::Doorbell),
            [KIND_REFUSED, reason] => Some(Self::Refused { reason }),
            [KIND_LINK, up @ (0 | 1)] => Some(Self::Link { up: up == 1 }),
            [KIND_MCAST, flags, lo, hi, ref addrs @ ..] => {
                let count = usize::from(u16::from_le_bytes([lo, hi]));
                let all = match flags {
                    0 => false,
                    MCAST_ALL => true,
                    _ => return None,
                };
                let whole = count <= MCAST_MAX
                    && addrs.len() == count.checked_mul(6)?
                    && addrs.chunks_exact(6).all(is_multicast);
                whole.then_some(Self::Mcast { all, addrs })
            }
            [KIND_MCAST_DONE, 0, 0, 0, a, b2, c, d] => Some(Self::McastDone {
                result: i32::from_le_bytes([a, b2, c, d]),
            }),
            _ => None,
        }
    }
}

/// Whether `mac` is a multicast address: the first octet's low bit set.
#[must_use]
pub fn is_multicast(mac: &[u8]) -> bool {
    mac.first().is_some_and(|b| b & 1 == 1)
}

/// A multicast list, as a [`Message::Mcast`] carries it: the first `len` of
/// `addrs`, each a multicast address -- or every multicast frame, with
/// `all`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McastList {
    addrs: [[u8; 6]; MCAST_MAX],
    len: usize,
    /// Every multicast frame is wanted, whatever the list.
    pub all: bool,
}

impl McastList {
    /// The list a stack wants: `addrs` -- any that is not a multicast
    /// address left out -- or, with `all` or more than [`MCAST_MAX`] of
    /// them, every multicast frame and no list.
    #[must_use]
    pub fn new(all: bool, addrs: &[[u8; 6]]) -> Self {
        let mut list = Self {
            addrs: [[0; 6]; MCAST_MAX],
            len: 0,
            all: all || addrs.len() > MCAST_MAX,
        };
        if !list.all {
            list.len = list
                .addrs
                .iter_mut()
                .zip(addrs.iter().filter(|a| is_multicast(&a[..])))
                .map(|(to, from)| *to = *from)
                .count();
        }
        list
    }

    /// The list a [`Message::Mcast`] carries: `addrs` is its six-byte
    /// addresses in turn, as [`Message::decode`] checked them.
    #[must_use]
    pub fn from_message(all: bool, addrs: &[u8]) -> Self {
        let mut list = Self {
            addrs: [[0; 6]; MCAST_MAX],
            len: 0,
            all,
        };
        list.len = list
            .addrs
            .iter_mut()
            .zip(addrs.chunks_exact(6))
            .map(|(to, from)| to.copy_from_slice(from))
            .count();
        list
    }

    /// Its addresses.
    #[must_use]
    pub fn addrs(&self) -> &[[u8; 6]] {
        self.addrs.get(..self.len).unwrap_or(&[])
    }

    /// The [`Message::Mcast`] saying it, written into `out`: its length,
    /// `None` when `out` is too short.
    #[must_use]
    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        encode_mcast(self.all, self.addrs(), out)
    }
}

/// [`Message::Refused`] written into `out`: its length, `None` when `out`
/// is too short.
#[must_use]
pub fn encode_refused(reason: u8, out: &mut [u8]) -> Option<usize> {
    out.get_mut(..2)?.copy_from_slice(&[KIND_REFUSED, reason]);
    Some(2)
}

/// [`Message::Link`] written into `out`: its length, `None` when `out` is
/// too short.
#[must_use]
pub fn encode_link(up: bool, out: &mut [u8]) -> Option<usize> {
    out.get_mut(..2)?
        .copy_from_slice(&[KIND_LINK, u8::from(up)]);
    Some(2)
}

/// [`Message::Mcast`] written into `out`, listing `addrs`: its length;
/// `None` for more than [`MCAST_MAX`] addresses, one that is not a
/// multicast address, or `out` too short.
#[must_use]
pub fn encode_mcast(all: bool, addrs: &[[u8; 6]], out: &mut [u8]) -> Option<usize> {
    if addrs.len() > MCAST_MAX || !addrs.iter().all(|a| is_multicast(a)) {
        return None;
    }
    let len = 4 + 6 * addrs.len();
    let out = out.get_mut(..len)?;
    let count = u16::try_from(addrs.len()).ok()?.to_le_bytes();
    out.get_mut(..4)?.copy_from_slice(&[
        KIND_MCAST,
        if all { MCAST_ALL } else { 0 },
        count[0],
        count[1],
    ]);
    for (to, from) in out.get_mut(4..)?.chunks_exact_mut(6).zip(addrs) {
        to.copy_from_slice(from);
    }
    Some(len)
}

/// [`Message::McastDone`] written into `out`: its length, `None` when `out`
/// is too short.
#[must_use]
pub fn encode_mcast_done(result: i32, out: &mut [u8]) -> Option<usize> {
    let out = out.get_mut(..8)?;
    out.get_mut(..4)?
        .copy_from_slice(&[KIND_MCAST_DONE, 0, 0, 0]);
    out.get_mut(4..)?.copy_from_slice(&result.to_le_bytes());
    Some(8)
}

/// The attach message's length on the wire.
pub const ATTACH_LEN: usize = 32;

/// What a stack is told on attaching: the card's MAC address and MTU, the
/// ring's slot count -- its submission and completion queues have that many
/// entries each, and its data area that many slots of [`SLOT_SIZE`] -- and
/// the shared memory region holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Port {
    pub mac: [u8; 6],
    pub mtu: u16,
    pub slots: u32,
    /// The region's handle (`SYS_SHM_MAP`'s), which the stack was granted.
    pub region: u64,
}

impl Port {
    /// The attach message, little-endian: magic, version, MAC, MTU, slot
    /// size, slot count, region. `None` when `out` is shorter than
    /// [`ATTACH_LEN`].
    #[must_use]
    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        let out = out.get_mut(..ATTACH_LEN)?;
        out.get_mut(0..4)?.copy_from_slice(&MAGIC.to_le_bytes());
        out.get_mut(4..8)?.copy_from_slice(&VERSION.to_le_bytes());
        out.get_mut(8..14)?.copy_from_slice(&self.mac);
        out.get_mut(14..16)?
            .copy_from_slice(&self.mtu.to_le_bytes());
        out.get_mut(16..20)?
            .copy_from_slice(&SLOT_SIZE.to_le_bytes());
        out.get_mut(20..24)?
            .copy_from_slice(&self.slots.to_le_bytes());
        out.get_mut(24..32)?
            .copy_from_slice(&self.region.to_le_bytes());
        Some(ATTACH_LEN)
    }

    /// An attach message read: `None` for a wrong magic, version or slot
    /// size, a slot count that is no power of two (the queues' entries must
    /// be), or a message too short.
    #[must_use]
    pub fn decode(b: &[u8]) -> Option<Self> {
        let u32_at = |at: usize| -> Option<u32> {
            Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
        };
        if u32_at(0)? != MAGIC || u32_at(4)? != VERSION || u32_at(16)? != SLOT_SIZE {
            return None;
        }
        let slots = u32_at(20)?;
        if !crate::ring::is_power_of_two(slots) {
            return None;
        }
        let mut mac = [0u8; 6];
        mac.copy_from_slice(b.get(8..14)?);
        let mtu = u16::from_le_bytes(b.get(14..16)?.try_into().ok()?);
        let region = u64::from_le_bytes(b.get(24..32)?.try_into().ok()?);
        Some(Self {
            mac,
            mtu,
            slots,
            region,
        })
    }

    /// The bytes the ring's region needs: its header, the two queues, and
    /// the slots.
    #[must_use]
    pub fn region_size(&self) -> usize {
        crate::ring::region_size(self.slots, self.slots, self.slots.saturating_mul(SLOT_SIZE))
    }

    /// Where slot `i` begins in the data area; `None` past the last.
    #[must_use]
    pub fn slot_offset(&self, i: u32) -> Option<u32> {
        (i < self.slots).then(|| i.saturating_mul(SLOT_SIZE))
    }
}

/// What the netstack daemon's `nic-check` -- the boot test's check of a card
/// served this way, which the kernel's self-test runs and reads -- exits
/// with: what failed, [`check::OK`] for nothing.
pub mod check {
    pub const OK: i32 = 0;
    /// It could not attach to the card.
    pub const NO_CARD: i32 = 10;
    /// The kernel let it pass on a region it was never granted.
    pub const GRANT: i32 = 11;
    pub const NO_ADDRESS: i32 = 12;
    pub const NO_GATEWAY: i32 = 13;
    pub const PINGS_LOST: i32 = 14;
    /// The card's driver refused it for want of the card's key
    /// ([`super::REFUSED_NO_KEY`]).
    pub const TURNED_AWAY: i32 = 15;
    /// The card's link was never told, or did not follow its cable.
    pub const LINK: i32 = 16;
    /// The card did not pass multicast frames as its list said.
    pub const MCAST: i32 = 17;
    /// The card's driver kept a stack that broke the protocol.
    pub const PROTOCOL: i32 = 18;
    /// It was started with arguments it does not take.
    pub const USAGE: i32 = 19;
    /// Its lease on the card was not kept: lost -- refused, or run out -- or
    /// not confirmed by its server when the card's link came back.
    pub const LEASE: i32 = 20;

    /// What exit `code` says went wrong.
    #[must_use]
    pub fn failure(code: i32) -> &'static str {
        match code {
            OK => "it passed",
            NO_CARD => "it could not attach to the card",
            GRANT => "the kernel let it pass on a region it was never granted",
            NO_ADDRESS => "it took no address by DHCP",
            NO_GATEWAY => "the gateway did not answer its ARP requests",
            PINGS_LOST => {
                "its pings went wrong: unanswered, answered otherwise or twice, sent again too often, or waited for with no doorbell"
            }
            TURNED_AWAY => "the card's driver refused it for want of the card's key",
            LINK => "the card's link was never told, or did not follow its cable",
            MCAST => "the card did not pass multicast frames as its list said",
            PROTOCOL => "the card's driver kept a stack that broke the protocol",
            USAGE => "it was started with arguments it does not take",
            LEASE => "its lease on the card was lost, or not confirmed when the link came back",
            _ => "it ended otherwise",
        }
    }

    /// `nic-check`'s third argument, what it may take of the network beyond
    /// the card -- one word each side of the kernel/daemon boundary reads,
    /// the kernel to start it, the daemon to parse it. None at all: QEMU's
    /// network, whose router answers a router solicitation to all nodes (the
    /// multicast check's signal), the cable left alone.
    pub mod mode {
        /// QEMU's network, the card's cable pulled and plugged back from
        /// outside when asked (the boot test, through QEMU's monitor).
        pub const CABLE: &[u8] = b"cable";
        /// A network that need not behave as QEMU's does -- a real one: its
        /// router need not answer a solicitation to all nodes, so the
        /// multicast check is said and skipped; a ping may be lost, so a few
        /// are allowed; the cable is left alone.
        pub const LAN: &[u8] = b"lan";
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORT: Port = Port {
        mac: [0x52, 0x54, 0, 0x12, 0x34, 0x59],
        mtu: 1500,
        slots: 128,
        region: 0x1_0000_0007,
    };

    #[test]
    fn nic_checks_modes_are_distinct_nonempty_words() {
        use check::mode::{CABLE, LAN};
        for word in [CABLE, LAN] {
            assert!(
                !word.is_empty() && word.iter().all(u8::is_ascii_lowercase),
                "{word:?}"
            );
        }
        assert_ne!(CABLE, LAN);
    }

    #[test]
    fn an_attach_message_reads_back_as_written() {
        let mut b = [0u8; 40];
        assert_eq!(PORT.encode(&mut b), Some(ATTACH_LEN));
        assert_eq!(Port::decode(&b[..ATTACH_LEN]), Some(PORT));
        assert_eq!(PORT.encode(&mut b[..ATTACH_LEN - 1]), None, "no room");
    }

    #[test]
    fn a_wrong_or_short_message_is_refused() {
        let mut b = [0u8; ATTACH_LEN];
        assert_eq!(PORT.encode(&mut b), Some(ATTACH_LEN));
        // Byte 4 is the version's low byte: version 1's driver sends no
        // link and takes no list. Byte 17 is the slot size's (2048 = 0x800)
        // one non-zero byte.
        for (at, value) in [(0, 0xFF), (4, 1), (4, 3), (17, 0), (20, 3)] {
            let mut bad = b;
            bad[at] = value;
            assert_eq!(Port::decode(&bad), None, "byte {at} = {value}");
        }
        assert_eq!(Port::decode(&b[..ATTACH_LEN - 1]), None);
    }

    const ALL_NODES: [u8; 6] = [0x33, 0x33, 0, 0, 0, 1];
    const ALL_HOSTS: [u8; 6] = [0x01, 0x00, 0x5E, 0, 0, 1];

    #[test]
    fn messages_read_back_as_written() {
        let mut b = [0u8; MESSAGE_MAX];
        assert_eq!(Message::decode(&[DOORBELL]), Some(Message::Doorbell));
        for reason in [REFUSED_NO_KEY, REFUSED_NO_RING, 200] {
            let n = encode_refused(reason, &mut b).unwrap();
            assert_eq!(Message::decode(&b[..n]), Some(Message::Refused { reason }));
        }
        assert_ne!(REFUSED_NO_KEY, REFUSED_NO_RING);
        for up in [false, true] {
            let n = encode_link(up, &mut b).unwrap();
            assert_eq!(Message::decode(&b[..n]), Some(Message::Link { up }));
        }
        for result in [0, -12, i32::MIN] {
            let n = encode_mcast_done(result, &mut b).unwrap();
            assert_eq!(
                Message::decode(&b[..n]),
                Some(Message::McastDone { result })
            );
        }
        let n = encode_mcast(false, &[ALL_NODES, ALL_HOSTS], &mut b).unwrap();
        assert_eq!(n, 16);
        let Some(Message::Mcast { all, addrs }) = Message::decode(&b[..n]) else {
            panic!("not a list: {:?}", Message::decode(&b[..n]));
        };
        assert!(!all);
        assert_eq!(addrs, &[ALL_NODES, ALL_HOSTS].concat()[..]);
        let n = encode_mcast(true, &[], &mut b).unwrap();
        assert_eq!(
            Message::decode(&b[..n]),
            Some(Message::Mcast {
                all: true,
                addrs: &[]
            })
        );
    }

    #[test]
    fn the_longest_list_fills_the_longest_message() {
        let mut b = [0u8; MESSAGE_MAX + 1];
        let many = [ALL_NODES; MCAST_MAX];
        assert_eq!(encode_mcast(false, &many, &mut b), Some(MESSAGE_MAX));
        assert!(Message::decode(&b[..MESSAGE_MAX]).is_some());
        let too_many = [ALL_NODES; MCAST_MAX + 1];
        assert_eq!(encode_mcast(false, &too_many, &mut b), None);
        assert_eq!(
            encode_mcast(false, &many, &mut b[..MESSAGE_MAX - 1]),
            None,
            "no room"
        );
    }

    #[test]
    fn a_unicast_address_is_never_listed() {
        let mut b = [0u8; MESSAGE_MAX];
        assert_eq!(encode_mcast(false, &[ALL_NODES, PORT.mac], &mut b), None);
        // Written by hand, as a stack that does not keep to the protocol may.
        let n = encode_mcast(false, &[ALL_NODES, ALL_HOSTS], &mut b).unwrap();
        b[10] = PORT.mac[0];
        assert_eq!(Message::decode(&b[..n]), None);
    }

    #[test]
    fn a_wrong_message_is_refused() {
        let mut b = [0u8; MESSAGE_MAX + 6];
        let n = encode_mcast(false, &[ALL_NODES], &mut b).unwrap();
        let bad: [&[u8]; 14] = [
            &[],
            &[DOORBELL, 0],
            b"X",
            &[KIND_REFUSED],
            &[KIND_REFUSED, REFUSED_NO_KEY, 0],
            &[KIND_LINK],
            &[KIND_LINK, 2],
            &[KIND_LINK, 1, 0],
            &[KIND_MCAST_DONE, 0, 0, 0, 0, 0, 0],
            &[KIND_MCAST_DONE, 1, 0, 0, 0, 0, 0, 0],
            // The count says two, the list holds one; and one and a half.
            &[KIND_MCAST, 0, 2, 0, 0x33, 0x33, 0, 0, 0, 1],
            &[KIND_MCAST, 0, 1, 0, 0x33, 0x33, 0, 0, 0, 1, 0x33, 0x33, 0],
            // An unknown flag.
            &[KIND_MCAST, 2, 1, 0, 0x33, 0x33, 0, 0, 0, 1],
            &b[..n - 1],
        ];
        for m in bad {
            assert_eq!(Message::decode(m), None, "{m:?}");
        }
        // A count past the most, its list whole.
        let count = u16::try_from(MCAST_MAX + 1).unwrap().to_le_bytes();
        b[..4].copy_from_slice(&[KIND_MCAST, 0, count[0], count[1]]);
        for at in (4..4 + 6 * (MCAST_MAX + 1)).step_by(6) {
            b[at..at + 6].copy_from_slice(&ALL_NODES);
        }
        assert_eq!(Message::decode(&b[..4 + 6 * (MCAST_MAX + 1)]), None);
    }

    #[test]
    fn a_list_keeps_multicast_addresses_and_asks_for_all_past_the_most() {
        let list = McastList::new(false, &[ALL_NODES, PORT.mac, ALL_HOSTS]);
        assert!(!list.all);
        assert_eq!(
            list.addrs(),
            &[ALL_NODES, ALL_HOSTS],
            "the unicast one left out"
        );
        let many = McastList::new(false, &[ALL_NODES; MCAST_MAX + 1]);
        assert!(
            many.all && many.addrs().is_empty(),
            "past the most: all, no list"
        );
        let full = McastList::new(false, &[ALL_HOSTS; MCAST_MAX]);
        assert!(!full.all && full.addrs().len() == MCAST_MAX);
        let all = McastList::new(true, &[ALL_NODES]);
        assert!(all.all && all.addrs().is_empty());
    }

    #[test]
    fn a_list_reads_back_from_its_message() {
        let mut b = [0u8; MESSAGE_MAX];
        for list in [
            McastList::new(false, &[ALL_NODES, ALL_HOSTS]),
            McastList::new(true, &[]),
            McastList::new(false, &[]),
            McastList::new(false, &[ALL_HOSTS; MCAST_MAX]),
        ] {
            let n = list.encode(&mut b).unwrap();
            let Some(Message::Mcast { all, addrs }) = Message::decode(&b[..n]) else {
                panic!("not a list");
            };
            assert_eq!(McastList::from_message(all, addrs), list);
        }
    }

    #[test]
    fn every_check_exit_says_its_own_failure() {
        use check::*;
        let codes = [
            OK,
            NO_CARD,
            GRANT,
            NO_ADDRESS,
            NO_GATEWAY,
            PINGS_LOST,
            TURNED_AWAY,
            LINK,
            MCAST,
            PROTOCOL,
        ];
        let said: Vec<_> = codes.iter().map(|&c| failure(c)).collect();
        for (i, a) in said.iter().enumerate() {
            assert_ne!(*a, failure(-1), "exit {} is described", codes[i]);
            for b in &said[i + 1..] {
                assert_ne!(a, b, "two exits described alike");
            }
        }
    }

    #[test]
    fn slots_are_laid_out_one_after_another() {
        assert_eq!(PORT.slot_offset(0), Some(0));
        assert_eq!(PORT.slot_offset(3), Some(3 * SLOT_SIZE));
        assert_eq!(PORT.slot_offset(128), None);
        assert_eq!(
            PORT.region_size(),
            crate::ring::region_size(128, 128, 128 * SLOT_SIZE)
        );
    }
}

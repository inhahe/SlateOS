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

/// The prefix of a card's service name.
pub const SERVICE_PREFIX: &[u8] = b"nic.";

/// A slot's size: a whole Ethernet frame with its header, 1518 bytes, and
/// room to spare.
pub const SLOT_SIZE: u32 = 2048;

/// The doorbell: one byte, sent on the connection after pushing to a side
/// that said it sleeps.
pub const DOORBELL: u8 = b'D';

/// The attach message's magic ("NPRT") and version.
pub const MAGIC: u32 = 0x4E50_5254;
pub const VERSION: u32 = 1;

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
    /// The card's driver closed the connection before attaching it -- as
    /// it does to a process without the card's key.
    pub const TURNED_AWAY: i32 = 15;

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
            TURNED_AWAY => "the card's driver closed the connection before attaching it",
            _ => "it ended otherwise",
        }
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
        // Byte 17 is the slot size's (2048 = 0x800) one non-zero byte.
        for (at, value) in [(0, 0xFF), (4, 2), (17, 0), (20, 3)] {
            let mut bad = b;
            bad[at] = value;
            assert_eq!(Port::decode(&bad), None, "byte {at} = {value}");
        }
        assert_eq!(Port::decode(&b[..ATTACH_LEN - 1]), None);
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

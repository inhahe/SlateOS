//! The parts of the probe driver that are functions of what it reads, kept
//! apart from `main.rs`'s system calls and register accesses so that the host
//! can test them:
//!
//! ```sh
//! cd services/devprobe
//! cargo test --lib --target x86_64-pc-windows-gnu
//! ```
//!
//! On the target this is `no_std`, like the binary that uses it.

#![cfg_attr(not(test), no_std)]
#![deny(clippy::all, clippy::pedantic)]

/// The card the boot test attaches for the probe: an Intel 82574 (QEMU's
/// `e1000e`), with the MAC address `scripts/boot-test.sh` gives it.
pub const INTEL: u32 = 0x8086;
pub const E1000E_82574: u32 = 0x10D3;
pub const EXPECTED_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x59];

/// The smallest BAR0 an 82574 has: its register window is 128 KiB.
pub const MIN_BAR0: u64 = 0x2_0000;

/// 82574 registers, as offsets into BAR0: receive address 0, low and high
/// halves, and the EEPROM read register.
pub const RAL0: usize = 0x5400;
pub const RAH0: usize = 0x5404;
pub const EERD: usize = 0x0014;

/// RAH's Address Valid bit: the receive address holds a MAC.
const RAH_AV: u32 = 1 << 31;
/// EERD: write START with a word address in bits 15:2; the card sets DONE
/// with the word in bits 31:16.
const EERD_START: u32 = 1;
const EERD_DONE: u32 = 1 << 1;
const EERD_ADDR_SHIFT: u32 = 2;

/// The MAC address receive address 0 holds, given RAL0 and RAH0 as read, or
/// `None` when RAH0 says the address is not valid. The address is stored
/// little-endian: its first four bytes in RAL0, the last two in RAH0's low
/// half.
#[must_use]
pub fn mac_from_receive_address(ral: u32, rah: u32) -> Option<[u8; 6]> {
    if rah & RAH_AV == 0 {
        return None;
    }
    let low = ral.to_le_bytes();
    let high = rah.to_le_bytes();
    Some([low[0], low[1], low[2], low[3], high[0], high[1]])
}

/// What to write to EERD to read EEPROM word `word`.
#[must_use]
pub fn eerd_read_command(word: u16) -> u32 {
    (u32::from(word) << EERD_ADDR_SHIFT) | EERD_START
}

/// The word an EERD read returned, given EERD as read, or `None` while the
/// read is not done.
#[must_use]
pub fn eerd_data(eerd: u32) -> Option<u16> {
    if eerd & EERD_DONE == 0 {
        return None;
    }
    let [_, _, low, high] = eerd.to_le_bytes();
    Some(u16::from_le_bytes([low, high]))
}

/// The MAC address in the EEPROM's first three words, as read: each word
/// holds two of its bytes, the earlier one in the low half.
#[must_use]
pub fn mac_from_eeprom_words(words: [u16; 3]) -> [u8; 6] {
    let mut mac = [0u8; 6];
    for (pair, word) in mac.chunks_exact_mut(2).zip(words) {
        pair.copy_from_slice(&word.to_le_bytes());
    }
    mac
}

/// What the probe exits with: `Ok` when every step worked, and otherwise the
/// step that failed, so the kernel's self-test can name it. Each value is
/// distinct, and none is `Ok`'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Exit {
    Ok = 42,
    OpenedNone = 10,
    OpenedMore = 11,
    WrongDevice = 12,
    BarUnusable = 13,
    MapFailed = 14,
    WrongMac = 15,
    DmaFailed = 16,
    IrqSetupFailed = 17,
    IrqTakeFailed = 18,
    Panicked = 99,
}

impl Exit {
    /// The exit code itself.
    #[must_use]
    pub fn code(self) -> u64 {
        u64::from(self as u8)
    }
}

/// Step 1's verdict: of the functions on bus 0, exactly one may open -- the
/// one the probe was granted. `opened` is how many did, `last` the handle of
/// the last that did.
///
/// # Errors
///
/// `OpenedNone` when none opened, `OpenedMore` when more than one did: the
/// capability check let through a function it was not granted.
pub fn only_granted_opened(opened: u32, last: Option<u64>) -> Result<u64, Exit> {
    match (opened, last) {
        (1, Some(handle)) => Ok(handle),
        (0, _) | (_, None) => Err(Exit::OpenedNone),
        _ => Err(Exit::OpenedMore),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Receive address 0 holding 52:54:00:12:34:59, valid.
    const RAL_59: u32 = 0x1200_5452;
    const RAH_59: u32 = 0x8000_5934;

    #[test]
    fn receive_address_decodes_little_endian() {
        assert_eq!(mac_from_receive_address(RAL_59, RAH_59), Some(EXPECTED_MAC));
    }

    #[test]
    fn receive_address_without_av_is_none() {
        assert_eq!(mac_from_receive_address(RAL_59, RAH_59 & !RAH_AV), None);
        assert_eq!(mac_from_receive_address(0, 0), None);
    }

    #[test]
    fn receive_address_ignores_rah_bits_above_the_address() {
        // Bits 16 and 17 of RAH are the address-select field, not address
        // bytes.
        assert_eq!(
            mac_from_receive_address(RAL_59, RAH_59 | 0x0003_0000),
            Some(EXPECTED_MAC)
        );
    }

    #[test]
    fn eerd_command_places_the_address_and_start() {
        assert_eq!(eerd_read_command(0), 0b01);
        assert_eq!(eerd_read_command(1), 0b101);
        assert_eq!(eerd_read_command(2), 0b1001);
        assert_eq!(eerd_read_command(0x3FFF), 0xFFFD);
    }

    #[test]
    fn eerd_data_waits_for_done() {
        assert_eq!(eerd_data(0x5452_0000 | eerd_read_command(0)), None);
        assert_eq!(eerd_data(0x5452_0002 | eerd_read_command(0)), Some(0x5452));
        assert_eq!(eerd_data(0xFFFF_0002), Some(0xFFFF));
    }

    #[test]
    fn eeprom_words_decode_in_order() {
        assert_eq!(
            mac_from_eeprom_words([0x5452, 0x1200, 0x5934]),
            EXPECTED_MAC
        );
    }

    #[test]
    fn exit_codes_are_distinct_and_only_ok_is_42() {
        let all = [
            Exit::Ok,
            Exit::OpenedNone,
            Exit::OpenedMore,
            Exit::WrongDevice,
            Exit::BarUnusable,
            Exit::MapFailed,
            Exit::WrongMac,
            Exit::DmaFailed,
            Exit::IrqSetupFailed,
            Exit::IrqTakeFailed,
            Exit::Panicked,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.code(), b.code(), "{a:?} and {b:?}");
            }
        }
        assert_eq!(Exit::Ok.code(), 42);
    }

    #[test]
    fn exactly_one_open_is_the_only_pass() {
        assert_eq!(only_granted_opened(1, Some(7)), Ok(7));
        assert_eq!(only_granted_opened(0, None), Err(Exit::OpenedNone));
        assert_eq!(only_granted_opened(2, Some(7)), Err(Exit::OpenedMore));
        assert_eq!(only_granted_opened(255, Some(7)), Err(Exit::OpenedMore));
        // A count with no handle is a bookkeeping slip, read as nothing opened.
        assert_eq!(only_granted_opened(1, None), Err(Exit::OpenedNone));
    }
}

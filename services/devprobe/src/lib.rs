//! The parts of the probe driver that are functions of what it reads, kept
//! apart from `main.rs`'s system calls and register accesses so that the host
//! can test them:
//!
//! ```sh
//! cd services/devprobe
//! cargo test --lib --target x86_64-pc-windows-gnu
//! ```
//!
//! The kernel's self-test that runs the probe (`devhandle::self_test_ring3`)
//! uses this library too, for what the two sides must agree on: the card,
//! its MAC address, the registers it is read through, and the exit codes.
//!
//! On the target this is `no_std`, like the binary that uses it.

#![cfg_attr(not(test), no_std)]

/// Intel's PCI vendor id.
pub const INTEL: u16 = 0x8086;
/// The 82574's device id: QEMU's `e1000e`, which the boot test attaches for
/// the probe.
pub const E1000E_82574: u16 = 0x10D3;
/// The MAC address `scripts/boot-test.sh` gives the probe's card. It is what
/// marks the card as the boot test's: QEMU's prefix, and an address no other
/// card in the boot test has. The kernel selects only a card with it, so a
/// real 82574 on the same machine is never handed to the probe.
pub const EXPECTED_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x59];

/// The smallest BAR0 an 82574 has: its register window is 128 KiB.
pub const MIN_BAR0: u64 = 0x2_0000;

/// The 82574's receive address 0, low half: the MAC's first four bytes.
pub const RAL0: usize = 0x5400;
/// Receive address 0, high half: the MAC's last two bytes and the Address
/// Valid bit.
pub const RAH0: usize = 0x5404;
/// The 82574's EEPROM read register.
pub const EERD: usize = 0x0014;
/// The 82574's interrupt cause read register: reading it clears it.
pub const ICR: usize = 0x00C0;
/// Interrupt cause set: a cause written here is raised as if it had happened.
pub const ICS: usize = 0x00C8;
/// Interrupt mask set: the causes written here interrupt.
pub const IMS: usize = 0x00D0;
/// Interrupt mask clear: the causes written here no longer interrupt.
pub const IMC: usize = 0x00D8;
/// The cause the probe has the card raise: transmit descriptor written back.
/// Any cause would do; this one the card never raises by itself while it
/// sends nothing.
pub const TEST_CAUSE: u32 = 1;

/// One item of `SYS_WAIT_MULTIPLE`'s array, as the kernel documents it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct WaitItem {
    /// The handle waited on.
    pub handle: u64,
    /// Its kind: a `cap::ResourceType` discriminant.
    pub kind: u32,
    /// The `poll(2)` events asked about.
    pub events: u32,
    /// The events that came, written by the kernel.
    pub revents: u32,
    /// Tail padding: the item is 24 bytes, aligned to 8.
    pub pad: u32,
}

/// `cap::ResourceType::PciDevice`: the kind of a device handle in a
/// [`WaitItem`]. (The kernel asserts at compile time that the two agree.)
pub const KIND_PCI_DEVICE: u32 = 36;
/// `poll(2)`'s `POLLIN`: a device handle is readable when its interrupts are
/// counted and not yet taken.
pub const POLLIN: u32 = 0x0001;

/// The PCI command register's offset in configuration space.
pub const PCI_COMMAND: u64 = 0x04;
/// The command register's Memory Space bit: the function decodes its
/// memory BARs.
pub const COMMAND_MEMORY_SPACE: u64 = 1 << 1;
/// The command register's Bus Master bit: the function may start DMA, and
/// send MSIs, which are memory writes.
pub const COMMAND_BUS_MASTER: u64 = 1 << 2;

/// What `SYS_PCI_DEVICE_OPEN` returns for a function the caller holds no
/// capability for: `KernelError::PermissionDenied`'s code. (The kernel
/// asserts at compile time that the two agree.)
pub const PERMISSION_DENIED: i64 = -400;

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
    let [b0, b1, b2, b3] = ral.to_le_bytes();
    let [b4, b5, _, _] = rah.to_le_bytes();
    Some([b0, b1, b2, b3, b4, b5])
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

/// Whether a 32-bit register at `offset` lies inside a BAR of `len` bytes:
/// 4-aligned, and wholly inside it.
#[must_use]
pub fn register_inside(offset: usize, len: u64) -> bool {
    offset.is_multiple_of(4)
        && u64::try_from(offset)
            .ok()
            .and_then(|start| start.checked_add(4))
            .is_some_and(|end| end <= len)
}

/// What the probe exits with: `Ok` when every step worked, and otherwise the
/// step that failed, so the kernel's self-test can name it. Each value is
/// distinct, none is `Ok`'s, and none is 128 or more, where the kernel puts
/// the codes of a process killed by a fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Exit {
    /// Every step worked.
    Ok = 42,
    /// Step 1: no function opened, so the granted one did not.
    OpenedNone = 10,
    /// Step 1: a second function opened: the capability check let through a
    /// function the probe was not granted.
    OpenedMore = 11,
    /// Step 2: the opened function is not an 82574.
    WrongDevice = 12,
    /// Step 3: BAR0 is not a memory BAR of at least [`MIN_BAR0`] bytes.
    BarUnusable = 13,
    /// Step 3: BAR0 could not be mapped.
    MapFailed = 14,
    /// Step 3: the MAC read through BAR0 is not [`EXPECTED_MAC`].
    WrongMac = 15,
    /// Step 4: no DMA arena.
    DmaFailed = 16,
    /// Step 5: the card's MSI could not be set up.
    IrqSetupFailed = 17,
    /// Step 5: the card's pending interrupts could not be taken.
    IrqTakeFailed = 18,
    /// Step 1: an open failed with something other than
    /// [`PERMISSION_DENIED`]: either the granted function failed to open, or
    /// another was refused for a reason other than the capability check.
    OpenFailed = 19,
    /// Step 3: decoding and bus mastering could not be turned on through the
    /// command register.
    CommandRefused = 20,
    /// Step 5: `SYS_WAIT_MULTIPLE` said the device was ready before it had
    /// interrupted, or failed.
    ReadyTooSoon = 21,
    /// Step 5: the interrupt the card was made to raise did not make the
    /// device ready in `SYS_WAIT_MULTIPLE`, or was not counted.
    InterruptLost = 22,
    /// The probe panicked: a bug in it.
    Panicked = 99,
}

impl Exit {
    /// Every exit code.
    pub const ALL: [Exit; 15] = [
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
        Exit::OpenFailed,
        Exit::CommandRefused,
        Exit::ReadyTooSoon,
        Exit::InterruptLost,
        Exit::Panicked,
    ];

    /// The exit code itself.
    #[must_use]
    pub fn code(self) -> u64 {
        u64::from(self as u8)
    }

    /// The exit whose code is `code`, if any.
    #[must_use]
    pub fn from_code(code: i64) -> Option<Exit> {
        Self::ALL
            .into_iter()
            .find(|e| u64::try_from(code).is_ok_and(|c| c == e.code()))
    }
}

/// Step 1, one open: `result` is what `SYS_PCI_DEVICE_OPEN` returned for one
/// function on bus 0, and `found` the handle of the one that opened before,
/// if any. Exactly one may open -- the one granted -- and every other must be
/// refused by the capability check, with [`PERMISSION_DENIED`].
///
/// # Errors
///
/// [`Exit::OpenedMore`] for a second function opened, and
/// [`Exit::OpenFailed`] for any other failure.
pub fn note_open(found: &mut Option<u64>, result: i64) -> Result<(), Exit> {
    if result == PERMISSION_DENIED {
        return Ok(());
    }
    let handle = u64::try_from(result)
        .ok()
        .filter(|&h| h != 0)
        .ok_or(Exit::OpenFailed)?;
    if found.replace(handle).is_some() {
        return Err(Exit::OpenedMore);
    }
    Ok(())
}

#[cfg(test)]
// Tests may panic and index: that is how they fail.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
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
    fn registers_must_be_aligned_and_inside_the_bar() {
        assert!(register_inside(0, MIN_BAR0));
        assert!(register_inside(RAH0, MIN_BAR0));
        assert!(register_inside(0x1_FFFC, MIN_BAR0));
        assert!(!register_inside(0x2_0000, MIN_BAR0));
        assert!(!register_inside(0x1_FFFE, MIN_BAR0));
        assert!(!register_inside(2, MIN_BAR0));
        assert!(!register_inside(usize::MAX - 3, u64::MAX));
    }

    #[test]
    fn exit_codes_are_distinct_below_128_and_decode() {
        for (i, a) in Exit::ALL.iter().enumerate() {
            assert!(a.code() < 128, "{a:?}");
            assert_eq!(Exit::from_code(a.code().cast_signed()), Some(*a));
            for b in &Exit::ALL[i + 1..] {
                assert_ne!(a.code(), b.code(), "{a:?} and {b:?}");
            }
        }
        assert_eq!(Exit::Ok.code(), 42);
        assert_eq!(Exit::from_code(0), None);
        assert_eq!(Exit::from_code(-1), None);
        assert_eq!(Exit::from_code(139), None);
    }

    #[test]
    fn a_wait_item_is_the_kernels_24_bytes() {
        assert_eq!(core::mem::size_of::<WaitItem>(), 24);
        assert_eq!(core::mem::align_of::<WaitItem>(), 8);
        assert_eq!(core::mem::offset_of!(WaitItem, kind), 8);
        assert_eq!(core::mem::offset_of!(WaitItem, events), 12);
        assert_eq!(core::mem::offset_of!(WaitItem, revents), 16);
    }

    #[test]
    fn exactly_one_open_and_every_other_refused() {
        let mut found = None;
        assert_eq!(note_open(&mut found, PERMISSION_DENIED), Ok(()));
        assert_eq!(note_open(&mut found, 7), Ok(()));
        assert_eq!(found, Some(7));
        assert_eq!(note_open(&mut found, PERMISSION_DENIED), Ok(()));
        assert_eq!(note_open(&mut found, 8), Err(Exit::OpenedMore));
    }

    #[test]
    fn a_refusal_other_than_the_capability_check_fails() {
        // Other error codes, and 0, which is no handle.
        for result in [-1, -402, 0] {
            assert_eq!(
                note_open(&mut None, result),
                Err(Exit::OpenFailed),
                "{result}"
            );
        }
    }
}

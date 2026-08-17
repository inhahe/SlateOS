//! ATI/AMD legacy display driver — R100 (Radeon 7000-series) and Rage 128.
//!
//! ## Why this part, and not a modern AMDGPU
//!
//! `design.txt` asks for an AMD GPU driver, and the obvious reading of that is
//! GCN/RDNA — the parts in machines people actually own. This module targets
//! the twenty-five-year-old R100/Rage 128 display block instead, for one
//! reason: **it is the only AMD-family display engine this project can
//! execute.** QEMU emulates no GCN or RDNA part at all, and the development
//! machine has an NVIDIA GPU in it. A modern AMDGPU port would therefore be
//! written blind — never once run, its register writes never once observed by
//! anything — and a display driver that has never driven a display is not a
//! driver, it is a plausible-looking document.
//!
//! QEMU's `ati-vga` device, by contrast, implements the real Rage 128 Pro
//! (`0x5046`) and RV100 (`0x5159`) register interfaces, including the CRTC
//! timing registers this module programs. That makes the whole path
//! executable, which is the property that decides whether the code is worth
//! having. The two families share the display block's register layout with the
//! later R300/R500 parts, so this is also the foundation a wider port extends,
//! not a detour away from one.
//!
//! See `design-decisions.md` §217, and the entry in `open-questions.md` on
//! whether the modern-AMDGPU goal should be pursued blind, on hardware we do
//! not have, or dropped in favour of the emulable set.
//!
//! ## Structure
//!
//! The module is split by *testability*, which is the axis that matters when
//! the hardware is often absent:
//!
//! - [`regs`] — register offsets, bitfield packing, CRTC timing arithmetic.
//!   Entirely pure; no MMIO. This is where display drivers actually go wrong,
//!   and being pure it is verified in full on every boot.
//! - [`timing`] — the VESA DMT mode table and the full timing type the generic
//!   [`super::mode::DrmMode`] does not carry.
//! - [`tests`] — the boot-time self-test over both of the above.
//!
//! MMIO probing and the [`super::DrmBackend`] integration land on top of this
//! layer, and are validated against QEMU's `ati-vga` rather than by assertion.

pub mod regs;
pub mod tests;
pub mod timing;

use crate::error::KernelResult;
use crate::serial_println;

/// PCI vendor ID for ATI Technologies, inherited by AMD.
pub const ATI_VENDOR_ID: u16 = 0x1002;

/// Which register-layout generation a device belongs to.
///
/// The distinction is real but narrow: both families present the same CRTC
/// timing registers at the same offsets, which is why one timing
/// implementation serves both. They diverge in memory-controller and PLL
/// details, which is where the family is consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsicFamily {
    /// Rage 128 / Rage 128 Pro — the pre-Radeon generation.
    Rage128,
    /// R100 — the first Radeon. RV100 is the cut-down member of the family.
    R100,
}

impl AsicFamily {
    /// Human-readable family name, for logging.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rage128 => "Rage128",
            Self::R100 => "R100",
        }
    }
}

/// A recognised device: its PCI ID, family, and marketing name.
#[derive(Debug, Clone, Copy)]
pub struct AsicInfo {
    /// PCI device ID, paired with [`ATI_VENDOR_ID`].
    pub device_id: u16,
    /// Register-layout generation.
    pub family: AsicFamily,
    /// The name the part was sold under.
    pub name: &'static str,
}

/// Devices this driver claims.
///
/// Deliberately short. Every ATI part of this era shares the display block, so
/// the temptation is to claim the whole vendor ID and treat anything else as
/// "probably compatible". That would be a guess made at the worst possible
/// moment — during probe, against an unknown device, with the display about to
/// be reprogrammed. An unrecognised ID is left alone, which costs a fallback
/// to the bootloader framebuffer and nothing else. IDs are added here as they
/// are actually tested.
pub static KNOWN_DEVICES: &[AsicInfo] = &[
    AsicInfo {
        device_id: 0x5046,
        family: AsicFamily::Rage128,
        name: "Rage 128 Pro",
    },
    AsicInfo {
        device_id: 0x5159,
        family: AsicFamily::R100,
        name: "Radeon 7000 (RV100)",
    },
];

/// Identify a PCI device, if this driver claims it.
///
/// Returns `None` for anything not in [`KNOWN_DEVICES`] — including other ATI
/// parts. See that table's documentation for why the vendor ID alone is not
/// enough.
#[must_use]
pub fn identify(vendor_id: u16, device_id: u16) -> Option<&'static AsicInfo> {
    if vendor_id != ATI_VENDOR_ID {
        return None;
    }
    KNOWN_DEVICES.iter().find(|d| d.device_id == device_id)
}

/// Run the ATI driver's self-tests.
///
/// # Errors
///
/// Propagates the first failure reported by [`tests::run`].
pub fn self_test() -> KernelResult<()> {
    serial_println!("[ati] Running self-test...");
    tests::run()
}

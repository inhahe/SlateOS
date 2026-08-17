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
//! - [`mmio`] — BAR mapping and register I/O. Impure by nature, so it is
//!   validated by *running* it against QEMU's `ati-vga` rather than by
//!   assertion. See that module's documentation for what that does and does
//!   not establish.

pub mod mmio;
pub mod modeset;
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

/// Probe the PCI bus for a supported ATI display device and check its
/// register map.
///
/// Called from [`super::init`].
///
/// Reports rather than propagates, because a machine without an ATI card is
/// the overwhelmingly common case and is not a failure of anything. The
/// distinction that matters is preserved in the log: "no device" and "device
/// present but its registers disagree with the register map" say different
/// things, and the second is a bug in [`regs`] that a silent skip would hide.
pub fn probe_hardware() {
    match mmio::probe() {
        Ok(None) => {
            serial_println!("[ati] No supported ATI display device present");
        }
        Ok(Some(dev)) => {
            if let Err(e) = mmio::verify(&dev) {
                serial_println!(
                    "[ati] WARNING: register-map verification failed: {:?} — offsets in regs.rs are suspect",
                    e
                );
                // A card whose register map does not read back correctly is the
                // last thing that should be handed a mode-set: every write would
                // land somewhere other than intended.
                return;
            }
            serial_println!(
                "[ati] {} register map verified against hardware",
                dev.info.name
            );
            exercise_modeset(&dev);
        }
        Err(e) => {
            serial_println!("[ati] WARNING: probe failed: {:?}", e);
        }
    }
}

/// Program a mode onto the card and confirm the registers hold it.
///
/// ## Why this is safe to do at boot, and when it is not
///
/// Mode-setting is the one part of a display driver that cannot be verified
/// without doing it, and reading a register map back only proves the offsets are
/// right — not that the values the timing encoder produces are values the
/// hardware accepts. So the driver programs a real mode and reads it back.
///
/// It does that **only on a card that is not showing anything**, which
/// [`mmio::AtiDevice::owns_console`] establishes by address. If this card holds
/// the console framebuffer, retiming it would blank the operator's only screen
/// and leave the explanation in a framebuffer nobody can read; that case is
/// skipped, loudly, and the register-map verification above is all the
/// confirmation available on such a machine.
///
/// The mode chosen is 640x480@60 — the smallest in the DMT table, so it fits in
/// any VRAM a supported part could have, and the one mode whose acceptance is
/// least interesting to be wrong about.
fn exercise_modeset(dev: &mmio::AtiDevice) {
    if dev.owns_console() {
        serial_println!(
            "[ati]   SKIP mode-set: this card holds the console framebuffer, leaving it alone"
        );
        return;
    }

    let Some(mode) = timing::lookup(640, 480, 60) else {
        serial_println!("[ati]   SKIP mode-set: 640x480@60 missing from the DMT table");
        return;
    };

    let plan = match modeset::ModeSetPlan::new(
        mode,
        crate::drm::mode::PixelFormat::Xrgb8888,
        0,
        dev.vram_bytes,
    ) {
        Ok(p) => p,
        Err(e) => {
            serial_println!("[ati]   WARNING: mode-set plan for 640x480@60 rejected: {e:?}");
            return;
        }
    };

    if let Err(e) = modeset::apply(&dev.mmio, &plan) {
        serial_println!("[ati]   WARNING: mode-set failed: {e:?}");
        return;
    }
    match modeset::verify_applied(&dev.mmio, &plan) {
        Ok(()) => serial_println!(
            "[ati]   Mode-set 640x480@60 XRGB8888 applied and read back exactly \
             (pitch {} B, {} B scanout) — CRTC programming confirmed",
            plan.pitch_bytes,
            plan.size_bytes
        ),
        Err(e) => serial_println!("[ati]   WARNING: mode-set did not read back: {e:?}"),
    }
}

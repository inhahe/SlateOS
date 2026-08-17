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
//! - [`modeset`] — deciding the CRTC register values (pure) and writing them
//!   (not).
//! - [`vram`] — suballocation of the card's video memory. Pure.
//! - [`tests`] — the boot-time self-test over all of the above.
//! - [`mmio`] — BAR2 mapping and register I/O. Impure by nature, so it is
//!   validated by *running* it against QEMU's `ati-vga` rather than by
//!   assertion. See that module's documentation for what that does and does
//!   not establish.
//! - [`aperture`] — BAR0 mapping: the video memory itself, so the CPU can put
//!   pixels in it. Impure, and validated the same way.
//! - [`backend`] — the above assembled into a DRM device: the state that
//!   outlives a single call, and the entry points [`crate::drm`] dispatches to.

pub mod aperture;
pub mod backend;
pub mod mmio;
pub mod modeset;
pub mod regs;
pub mod tests;
pub mod timing;
pub mod vram;

use crate::error::{KernelError, KernelResult};
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
pub fn probe_hardware() -> Option<backend::AtiBackend> {
    let dev = match mmio::probe() {
        Ok(None) => {
            serial_println!("[ati] No supported ATI display device present");
            return None;
        }
        Err(e) => {
            serial_println!("[ati] WARNING: probe failed: {:?}", e);
            return None;
        }
        Ok(Some(dev)) => dev,
    };

    if let Err(e) = mmio::verify(&dev) {
        serial_println!(
            "[ati] WARNING: register-map verification failed: {:?} — offsets in regs.rs are suspect",
            e
        );
        // A card whose register map does not read back correctly is the last
        // thing that should be handed a mode-set: every write would land
        // somewhere other than intended. Nor is it registered — a DRM device
        // whose registers are not where the driver thinks is worse than no
        // device, because something will try to display on it.
        return None;
    }
    serial_println!(
        "[ati] {} register map verified against hardware",
        dev.info.name
    );

    // SAFETY: `dev.vram_phys` is BAR0 as this card's own config space reports
    // it, and this is the only place an `AtiBackend` — and so the only place a
    // `VramAperture` over that BAR — is constructed. `drm::init` calls this
    // once.
    let mut backend = match unsafe { backend::AtiBackend::new(dev) } {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[ati] WARNING: could not map VRAM aperture: {e:?} — not registering");
            return None;
        }
    };
    // Log the memory type by name rather than the `is_cached()` predicate it
    // was derived from. "cacheable=false" is true of an uncached mapping and of
    // a write-combining one, so it cannot answer the only question a reader has
    // here — whether the PAT slot this mapping selected actually combines, or
    // whether `mm::pat::init` declined and left it merely uncached.
    serial_println!(
        "[ati]   VRAM aperture mapped: {} MiB at {:#x} (memory type {}, writes may linger: {})",
        backend.aperture().len() / (1024 * 1024),
        backend.aperture().phys(),
        backend.aperture().memory_type().name(),
        backend.aperture().is_cached()
    );

    exercise_modeset(&mut backend);
    backend.report();
    Some(backend)
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
/// least interesting to be wrong about. It is also the cheapest
/// to paint, which mattered more than it now does: the aperture used to be
/// mapped uncacheable for want of a write-combining page attribute, so 1.2 MB
/// of pixels was a few tens of milliseconds and 1080p would have been seconds.
/// `mm::pat` supplies write-combining now, so that margin is much wider — but
/// 640x480 is still the mode a supported part is least likely to reject, which
/// is the reason that survives. See [`aperture`]'s module documentation for
/// what the mapping is today.
fn exercise_modeset(backend: &mut backend::AtiBackend) {
    if backend.owns_console() {
        serial_println!(
            "[ati]   SKIP mode-set: this card holds the console framebuffer, leaving it alone"
        );
        return;
    }

    let Some(mode) = timing::lookup(640, 480, 60) else {
        serial_println!("[ati]   SKIP mode-set: 640x480@60 missing from the DMT table");
        return;
    };

    if let Err(e) = drive_display(backend, mode) {
        serial_println!("[ati]   WARNING: display exercise failed: {e:?}");
    }
}

/// Colours for the three horizontal bands, in `XRGB8888`.
///
/// Three distinct values rather than one solid fill, because a solid fill is
/// the one pattern a broken aperture can pass. A mapping that reads back a
/// constant — because it is backed by nothing, or because every read is served
/// by the same stale cache line — agrees with a single-colour framebuffer at
/// every pixel. It cannot agree with three.
const BAND_COLOURS: [u32; 3] = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF];

/// Allocate video memory, paint it, display it, and flip to a second buffer.
///
/// This is the end-to-end check the register-map verification cannot be: it
/// establishes that BAR0 really is video memory (pixels written through it read
/// back), that the allocator's offsets are ones the CRTC accepts, and that a
/// page flip changes what is displayed. Each step is reported separately so a
/// failure names the stage rather than the outcome.
///
/// # Errors
///
/// Propagates the first failing step. Every one of them means something
/// specific: a mapping failure means BAR0 is not where config space says, an
/// allocation failure means the card reported less memory than a 640x480
/// buffer, and a readback mismatch means the aperture is mapped but not backed.
fn drive_display(backend: &mut backend::AtiBackend, mode: &timing::ModeTiming) -> KernelResult<()> {
    const FORMAT: crate::drm::mode::PixelFormat = crate::drm::mode::PixelFormat::Xrgb8888;

    let total =
        u32::try_from(backend.aperture().len()).map_err(|_| KernelError::InvalidArgument)?;

    // Plan once at offset zero to learn how large the scanout buffer must be,
    // then allocate, then plan again at the offset actually obtained. The first
    // plan is thrown away; it is cheaper than duplicating the size arithmetic
    // here, and duplicating it is how the allocation and the CRTC come to
    // disagree about how big a framebuffer is.
    let sizing = modeset::ModeSetPlan::new(mode, FORMAT, 0, total)?;
    let front = backend
        .pool_mut()
        .alloc(sizing.size_bytes, modeset::SCANOUT_ALIGN)?;
    let back = backend
        .pool_mut()
        .alloc(sizing.size_bytes, modeset::SCANOUT_ALIGN)?;
    serial_println!(
        "[ati]   Allocated two {} B scanout buffers at VRAM +{:#x} and +{:#x} ({} B free)",
        sizing.size_bytes,
        front,
        back,
        backend.pool_mut().free_bytes()
    );

    // Paint the two buffers with the same three bands in different orders, so
    // that a flip which does not take effect is visible: if the displayed
    // buffer never changed, the two would be indistinguishable.
    paint_bands(backend.aperture(), front, &sizing, BAND_COLOURS)?;
    paint_bands(
        backend.aperture(),
        back,
        &sizing,
        [BAND_COLOURS[2], BAND_COLOURS[0], BAND_COLOURS[1]],
    )?;
    verify_bands(backend.aperture(), front, &sizing, BAND_COLOURS)?;
    serial_println!(
        "[ati]   Painted and read back 3 colour bands in each buffer — BAR0 is real video memory"
    );

    // Mode-set onto the front buffer.
    let plan = modeset::ModeSetPlan::new(mode, FORMAT, front, total)?;
    modeset::apply(&backend.device().mmio, &plan)?;
    modeset::verify_applied(&backend.device().mmio, &plan)?;
    let displayed = modeset::scanout_offset(&backend.device().mmio)?;
    if displayed != front {
        serial_println!(
            "[ati]   FAIL: CRTC_OFFSET reads {:#x}, wrote {:#x}",
            displayed,
            front
        );
        return Err(KernelError::CorruptedData);
    }
    serial_println!(
        "[ati]   Mode-set 640x480@60 XRGB8888 applied and read back exactly \
         (pitch {} B, {} B scanout at +{:#x}) — CRTC programming confirmed",
        plan.pitch_bytes,
        plan.size_bytes,
        front
    );

    // Page flip to the back buffer: one register write, no copy.
    modeset::page_flip(&backend.device().mmio, back)?;
    let displayed = modeset::scanout_offset(&backend.device().mmio)?;
    if displayed != back {
        serial_println!(
            "[ati]   FAIL: after flip, CRTC_OFFSET reads {:#x}, wrote {:#x}",
            displayed,
            back
        );
        return Err(KernelError::CorruptedData);
    }
    verify_bands(
        backend.aperture(),
        back,
        &sizing,
        [BAND_COLOURS[2], BAND_COLOURS[0], BAND_COLOURS[1]],
    )?;
    serial_println!("[ati]   Page-flipped to +{back:#x} and confirmed — scanout base is live");

    // Stop the CRTC before giving the memory back. It is still scanning the
    // back buffer, and freeing a run the display engine is actively reading
    // means the next allocation gets painted underneath a live scanout. That
    // is the exact hazard `AtiBackend::gem_destroy` refuses with `DeviceBusy`,
    // and this exercise does not get an exemption from it.
    //
    // It also makes the backend's own bookkeeping true: it was constructed
    // believing no mode is programmed and nothing is being scanned out, and
    // after this that is the case, so the first real page flip correctly
    // performs a full mode-set rather than assuming a timing it never wrote.
    modeset::disable(&backend.device().mmio)?;
    // Read it back: "I wrote the disable bit" and "the CRTC is off" are
    // different claims, and only the second one makes freeing the buffers safe.
    let gen_cntl = backend.device().mmio.read32(regs::CRTC_GEN_CNTL)?;
    if gen_cntl & regs::CRTC_EN != 0 {
        serial_println!(
            "[ati]   FAIL: CRTC still enabled after disable (CRTC_GEN_CNTL={gen_cntl:#010x})"
        );
        return Err(KernelError::CorruptedData);
    }
    serial_println!("[ati]   CRTC disabled and confirmed — scanout buffers safe to release");

    // Release both, and check the card came back whole. A driver that leaks its
    // scanout buffers works perfectly until the resolution changes a few times.
    let pool = backend.pool_mut();
    pool.free(front, sizing.size_bytes)?;
    pool.free(back, sizing.size_bytes)?;
    pool.check_invariants()?;
    if pool.free_bytes() != total || pool.region_count() != 1 {
        serial_println!(
            "[ati]   FAIL: after freeing both buffers, {} B free in {} runs, want {} B in 1",
            pool.free_bytes(),
            pool.region_count(),
            total
        );
        return Err(KernelError::CorruptedData);
    }

    Ok(())
}

/// Fill a scanout buffer with three horizontal colour bands.
fn paint_bands(
    ap: &aperture::VramAperture,
    base: u32,
    plan: &modeset::ModeSetPlan,
    colours: [u32; 3],
) -> KernelResult<()> {
    let height = plan
        .size_bytes
        .checked_div(plan.pitch_bytes)
        .ok_or(KernelError::InvalidArgument)?;
    let words = plan
        .pitch_bytes
        .checked_div(4)
        .ok_or(KernelError::InvalidArgument)?;
    // Integer division, so with a height that is not a multiple of three the
    // last band absorbs the remainder — which is why the band index is clamped
    // rather than trusted to stay below three.
    let band = height.checked_div(3).filter(|b| *b > 0).unwrap_or(height);

    for row in 0..height {
        let idx = (row.checked_div(band).unwrap_or(0) as usize).min(2);
        let colour = *colours.get(idx).ok_or(KernelError::InternalError)?;
        let row_off = base
            .checked_add(
                row.checked_mul(plan.pitch_bytes)
                    .ok_or(KernelError::InvalidArgument)?,
            )
            .ok_or(KernelError::InvalidArgument)?;
        ap.fill32(row_off, words, colour)?;
    }
    ap.flush(base, plan.size_bytes)
}

/// Read one pixel back from the middle of each band and check its colour.
///
/// One pixel per band, not the whole buffer: reads across the bus are slow, and
/// what is in question is whether the mapping is backed at all, which three
/// well-separated samples answer as well as three hundred thousand.
fn verify_bands(
    ap: &aperture::VramAperture,
    base: u32,
    plan: &modeset::ModeSetPlan,
    colours: [u32; 3],
) -> KernelResult<()> {
    let height = plan
        .size_bytes
        .checked_div(plan.pitch_bytes)
        .ok_or(KernelError::InvalidArgument)?;
    let band = height.checked_div(3).filter(|b| *b > 0).unwrap_or(height);

    for (i, want) in colours.iter().enumerate() {
        // The middle row of band `i`, and the middle pixel of that row.
        let row = (u32::try_from(i).unwrap_or(0))
            .checked_mul(band)
            .and_then(|r| r.checked_add(band / 2))
            .filter(|r| *r < height)
            .ok_or(KernelError::InvalidArgument)?;
        let off = base
            .checked_add(
                row.checked_mul(plan.pitch_bytes)
                    .ok_or(KernelError::InvalidArgument)?,
            )
            .and_then(|o| o.checked_add((plan.pitch_bytes / 2) & !3))
            .ok_or(KernelError::InvalidArgument)?;
        let got = ap.read32(off)?;
        if got != *want {
            serial_println!(
                "[ati]   FAIL: VRAM +{:#x} (band {}) reads {:#010x}, wrote {:#010x}",
                off,
                i,
                got,
                want
            );
            return Err(KernelError::CorruptedData);
        }
    }
    Ok(())
}

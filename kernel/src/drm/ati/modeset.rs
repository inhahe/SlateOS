//! CRTC mode-setting: deciding the register values, then writing them.
//!
//! ## Why this is split in two
//!
//! [`ModeSetPlan::new`] computes every value that will be written and validates
//! all of it, touching no hardware. [`apply`] does nothing but write a plan out
//! in the right order. The split is the same one the rest of this module is
//! built on, and it buys the same thing: the deciding — which is where
//! mode-setting actually goes wrong — is pure, so [`super::tests`] checks it on
//! every boot whether or not a card is present, while the part that needs a
//! card is small enough to read in one sitting.
//!
//! It also means a plan can be validated *before* the display is touched.
//! Discovering half-way through a mode-set that the framebuffer does not fit in
//! VRAM leaves a CRTC programmed with a timing whose scanout address is
//! garbage, which is a black or torn screen with no route back; the same
//! discovery made by [`ModeSetPlan::new`] is an error return and an untouched
//! display.
//!
//! ## Order of writes
//!
//! The CRTC is switched **off** for the retiming and back on afterwards, which
//! is what Linux's `radeon_legacy_crtc.c` does around the same registers and is
//! not merely cosmetic. Timing registers are not latched as a group: changing
//! the total while the CRTC is scanning it out means one or more frames driven
//! with a mixture of the old and new timings. A monitor asked to lock onto that
//! will at best flicker and at worst drop the link and take seconds to
//! re-acquire it, which on a display being reconfigured looks exactly like a
//! driver that has hung.

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use super::mmio::AtiMmio;
use super::regs::{self, CrtcTiming, PixWidth};
use super::timing::ModeTiming;
use crate::drm::mode::PixelFormat;

/// Alignment required of a scanout base address, in bytes.
///
/// The CRTC fetches in bursts, and a base address that is not burst-aligned is
/// handled by the memory controller shifting the whole image rather than by
/// refusing — so the failure mode is a picture skewed by a few pixels, which
/// reads as "the driver's timing is wrong" and sends the search to the wrong
/// place entirely. 256 bytes is what Linux aligns to for these parts.
pub const SCANOUT_ALIGN: u32 = 256;

/// Every register value a mode-set will write, validated.
///
/// Constructing one cannot fail part-way and leave anything half-applied,
/// because constructing one writes nothing.
#[derive(Debug, Clone, Copy)]
pub struct ModeSetPlan {
    /// Packed CRTC timing registers.
    pub timing: CrtcTiming,
    /// Pixel format, encoded for `CRTC_GEN_CNTL`.
    pub pix_width: PixWidth,
    /// Scanout stride, encoded in 8-pixel characters for `CRTC_PITCH`.
    pub pitch: u32,
    /// Byte offset of the scanout base within VRAM, for `CRTC_OFFSET`.
    pub offset: u32,
    /// Unencoded stride in bytes. Retained for logging and for the caller,
    /// which needs it to write pixels; the hardware never sees this form.
    pub pitch_bytes: u32,
    /// Bytes the scanout occupies: `pitch_bytes * vdisplay`.
    pub size_bytes: u32,
}

impl ModeSetPlan {
    /// Decide and validate every value a mode-set for `mode` will write.
    ///
    /// The stride is `hdisplay * bpp` with no padding. That is deliberate and
    /// is not the same choice [`PixelFormat::pitch`] makes: that helper rounds
    /// up to 64 bytes for cache-line reasons, but this register counts 8-pixel
    /// characters, and a stride padded in *bytes* is not generally a whole
    /// number of characters. `hdisplay` is already required to be a multiple of
    /// 8 by [`CrtcTiming::from_mode`], so the unpadded stride always is.
    ///
    /// # Errors
    ///
    /// - `InvalidArgument` if the timing cannot be encoded (propagated from
    ///   [`CrtcTiming::from_mode`]), if `fb_offset` is not [`SCANOUT_ALIGN`]-
    ///   aligned, or if the stride is not encodable.
    /// - `NotSupported` if the pixel format has no CRTC encoding.
    /// - `InvalidArgument` if the scanout does not fit in `vram_bytes` at
    ///   `fb_offset`. Checked here rather than trusted: the CRTC does not fault
    ///   on a scanout that runs off the end of VRAM, it wraps or reads zero,
    ///   and either way the diagnosis from the screen alone is very hard.
    pub fn new(
        mode: &ModeTiming,
        format: PixelFormat,
        fb_offset: u32,
        vram_bytes: u32,
    ) -> KernelResult<Self> {
        let timing = CrtcTiming::from_mode(mode)?;
        let pix_width = PixWidth::from_format(format)?;

        if !fb_offset.is_multiple_of(SCANOUT_ALIGN) {
            return Err(KernelError::InvalidArgument);
        }

        let bpp = format.bpp();
        let pitch_bytes = mode
            .hdisplay
            .checked_mul(bpp)
            .ok_or(KernelError::InvalidArgument)?;
        let pitch = regs::encode_pitch(pitch_bytes, bpp)?;

        let size_bytes = pitch_bytes
            .checked_mul(mode.vdisplay)
            .ok_or(KernelError::InvalidArgument)?;
        let end = fb_offset
            .checked_add(size_bytes)
            .ok_or(KernelError::InvalidArgument)?;
        if end > vram_bytes {
            return Err(KernelError::InvalidArgument);
        }

        Ok(Self {
            timing,
            pix_width,
            pitch,
            offset: fb_offset,
            pitch_bytes,
            size_bytes,
        })
    }

    /// The `CRTC_GEN_CNTL` value this plan wants, with the CRTC enabled.
    ///
    /// Built by clearing the fields this driver owns out of `current` and
    /// setting them, rather than by writing a value composed from scratch:
    /// `CRTC_GEN_CNTL` also carries bits this driver does not model (cursor
    /// enables, and on some parts DPMS state), and overwriting them with zero
    /// is a change nobody asked for whose effects show up as unrelated
    /// misbehaviour later.
    #[must_use]
    pub const fn gen_cntl(&self, current: u32) -> u32 {
        let preserved = current & !(regs::CRTC_PIX_WIDTH_MASK | regs::CRTC_INTERLACE_EN);
        preserved | self.pix_width.shifted() | regs::CRTC_EN
    }
}

/// Program a CRTC from a validated plan.
///
/// The CRTC is disabled for the retiming and re-enabled at the end — see the
/// module documentation for why that ordering is load-bearing.
///
/// # Errors
///
/// Propagates any register access that fails its bounds check, which would mean
/// an offset in [`regs`] is outside the mapped aperture.
pub fn apply(mmio: &AtiMmio, plan: &ModeSetPlan) -> KernelResult<()> {
    // Read first: the value written back at the end preserves the bits this
    // driver does not model.
    let gen_cntl = mmio.read32(regs::CRTC_GEN_CNTL)?;

    // 1. CRTC off for the duration. `CRTC_DISPLAY_DIS` blanks the output as
    //    well as stopping the fetch, so the monitor sees a clean loss of signal
    //    rather than a few frames of malformed one.
    mmio.write32(
        regs::CRTC_GEN_CNTL,
        (gen_cntl & !regs::CRTC_EN) | regs::CRTC_DISPLAY_DIS,
    )?;

    // 2. Timing. All four registers while the CRTC is not scanning them.
    mmio.write32(regs::CRTC_H_TOTAL_DISP, plan.timing.h_total_disp)?;
    mmio.write32(regs::CRTC_H_SYNC_STRT_WID, plan.timing.h_sync_strt_wid)?;
    mmio.write32(regs::CRTC_V_TOTAL_DISP, plan.timing.v_total_disp)?;
    mmio.write32(regs::CRTC_V_SYNC_STRT_WID, plan.timing.v_sync_strt_wid)?;

    // 3. Scanout address and stride. `CRTC_OFFSET_CNTL` is zeroed to select a
    //    plain linear framebuffer: its non-zero values select tiled layouts and
    //    a tiling mode left over from whatever ran before would reinterpret
    //    every address, for a picture that looks scrambled in blocks.
    mmio.write32(regs::CRTC_OFFSET_CNTL, 0)?;
    mmio.write32(regs::CRTC_OFFSET, plan.offset)?;
    mmio.write32(regs::CRTC_PITCH, plan.pitch)?;

    // 4. Depth, then on. `CRTC_DISPLAY_DIS` is cleared and `CRTC_CRT_ON` set in
    //    `CRTC_EXT_CNTL` so the DAC actually drives the connector.
    mmio.write32(regs::CRTC_GEN_CNTL, plan.gen_cntl(gen_cntl))?;
    let ext = mmio.read32(regs::CRTC_EXT_CNTL)?;
    mmio.write32(
        regs::CRTC_EXT_CNTL,
        (ext | regs::CRTC_CRT_ON) & !regs::CRTC_DISPLAY_DIS,
    )?;

    Ok(())
}

/// Read the timing registers back and confirm they hold what was written.
///
/// This is the check that makes a mode-set falsifiable. Every value comes from
/// [`ModeSetPlan`], which the self-test already verifies against hand-computed
/// constants, so agreement here means the values reached the registers the
/// driver aimed them at — and disagreement localises the fault to a specific
/// register rather than leaving "the screen is wrong".
///
/// Only the four timing registers are compared. `CRTC_GEN_CNTL` and
/// `CRTC_EXT_CNTL` are deliberately excluded: they carry status bits the
/// hardware itself updates, so a mismatch there would be expected behaviour
/// and an assertion on them would fail for reasons that are not bugs.
///
/// # Errors
///
/// `CorruptedData` if any timing register reads back a different value than was
/// written to it.
pub fn verify_applied(mmio: &AtiMmio, plan: &ModeSetPlan) -> KernelResult<()> {
    let mut mismatches = 0u32;
    for (label, offset, want) in [
        (
            "CRTC_H_TOTAL_DISP",
            regs::CRTC_H_TOTAL_DISP,
            plan.timing.h_total_disp,
        ),
        (
            "CRTC_H_SYNC_STRT_WID",
            regs::CRTC_H_SYNC_STRT_WID,
            plan.timing.h_sync_strt_wid,
        ),
        (
            "CRTC_V_TOTAL_DISP",
            regs::CRTC_V_TOTAL_DISP,
            plan.timing.v_total_disp,
        ),
        (
            "CRTC_V_SYNC_STRT_WID",
            regs::CRTC_V_SYNC_STRT_WID,
            plan.timing.v_sync_strt_wid,
        ),
    ] {
        let got = mmio.read32(offset)?;
        if got == want {
            continue;
        }
        mismatches = mismatches.saturating_add(1);
        serial_println!(
            "[ati]   FAIL: {} reads {:#010x}, wrote {:#010x} (differs in {:#010x})",
            label,
            got,
            want,
            got ^ want
        );
    }

    if mismatches > 0 {
        return Err(KernelError::CorruptedData);
    }
    Ok(())
}

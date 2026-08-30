//! ATI/AMD register map and bitfield packing for the R100/Rage128 display block.
//!
//! Every value in this file is a *pure* function of its inputs — nothing here
//! touches MMIO. That split is deliberate. The part of a display driver that
//! actually goes wrong is not the store to a register, it is the arithmetic
//! that decided what to store: the fields on these chips are packed in units
//! of 8 pixels, biased by one, and split across non-adjacent bit ranges, so a
//! plausible-looking expression can be wrong by 8 pixels or by one character
//! clock and still produce a picture that merely looks slightly off. Keeping
//! the arithmetic separate from the bus access is what lets the whole of it be
//! checked on every boot, on a machine with no ATI hardware in it at all.
//!
//! Register offsets follow the register block at BAR2 as documented in the
//! Radeon R100 register specification and implemented by Linux's `radeon`
//! driver (`drivers/gpu/drm/radeon/radeon_reg.h`).

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Register offsets (byte offsets into the MMIO BAR)
// ---------------------------------------------------------------------------

/// Indirect register index port.
///
/// The MMIO aperture only directly exposes the low registers; anything above
/// is reached by writing its offset here and then reading/writing [`MM_DATA`].
pub const MM_INDEX: u32 = 0x0000;
/// Indirect register data port. See [`MM_INDEX`].
pub const MM_DATA: u32 = 0x0004;

/// PLL register index port (indirect, like [`MM_INDEX`] but a separate space).
pub const CLOCK_CNTL_INDEX: u32 = 0x0008;
/// PLL register data port. See [`CLOCK_CNTL_INDEX`].
pub const CLOCK_CNTL_DATA: u32 = 0x000C;

/// Bus control / endianness and aperture configuration.
pub const CNFG_CNTL: u32 = 0x00E0;
/// Total video RAM, in bytes, as reported by the hardware.
pub const CNFG_MEMSIZE: u32 = 0x00F8;

/// General CRTC control (enable, depth, interlace).
pub const CRTC_GEN_CNTL: u32 = 0x0050;
/// Extended CRTC control (VGA vs. extended display path, DPMS blanking).
pub const CRTC_EXT_CNTL: u32 = 0x0054;
/// DAC control (power, palette format).
pub const DAC_CNTL: u32 = 0x0058;

/// Horizontal total and displayed, packed in units of 8 pixels.
pub const CRTC_H_TOTAL_DISP: u32 = 0x0200;
/// Horizontal sync start and width.
pub const CRTC_H_SYNC_STRT_WID: u32 = 0x0204;
/// Vertical total and displayed, in scanlines.
pub const CRTC_V_TOTAL_DISP: u32 = 0x0208;
/// Vertical sync start and width.
pub const CRTC_V_SYNC_STRT_WID: u32 = 0x020C;

/// Scanout base address, in bytes from the start of VRAM.
pub const CRTC_OFFSET: u32 = 0x0224;
/// Scanout addressing mode (tiling / flip control).
pub const CRTC_OFFSET_CNTL: u32 = 0x0228;
/// Scanout stride, in units of 8 pixels.
pub const CRTC_PITCH: u32 = 0x022C;

// --- CRTC_GEN_CNTL bits -----------------------------------------------------

/// Enable the CRTC (start generating a display signal).
pub const CRTC_EN: u32 = 1 << 25;
/// Select interlaced scanout.
pub const CRTC_INTERLACE_EN: u32 = 1 << 1;
/// Colour depth field position within `CRTC_GEN_CNTL`.
pub const CRTC_PIX_WIDTH_SHIFT: u32 = 8;
/// Colour depth field mask (4 bits), pre-shifted.
pub const CRTC_PIX_WIDTH_MASK: u32 = 0xF << CRTC_PIX_WIDTH_SHIFT;

// --- CRTC_EXT_CNTL bits -----------------------------------------------------

/// Route the display through the extended (non-VGA) path.
pub const CRTC_CRT_ON: u32 = 1 << 15;
/// Blank the display (DPMS off).
pub const CRTC_DISPLAY_DIS: u32 = 1 << 10;

/// Colour-depth encoding for the `CRTC_PIX_WIDTH` field of `CRTC_GEN_CNTL`.
///
/// These are the chip's own codes, not a bytes-per-pixel count: the numbering
/// is not monotonic in bit depth and cannot be derived arithmetically, which
/// is exactly why it is a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PixWidth {
    /// 8 bits per pixel, palette indexed.
    Bpp8 = 2,
    /// 15 bits per pixel (5-5-5, one unused bit).
    Bpp15 = 3,
    /// 16 bits per pixel (5-6-5).
    Bpp16 = 4,
    /// 24 bits per pixel, packed.
    Bpp24 = 5,
    /// 32 bits per pixel.
    Bpp32 = 6,
}

impl PixWidth {
    /// The chip's encoding for this depth, already shifted into place.
    #[must_use]
    pub const fn shifted(self) -> u32 {
        (self as u32) << CRTC_PIX_WIDTH_SHIFT
    }

    /// Map a DRM pixel format onto the chip's depth encoding.
    ///
    /// # Errors
    ///
    /// `NotSupported` for a format the display block cannot scan out. This
    /// returns an error rather than silently substituting a nearby depth:
    /// a CRTC programmed for the wrong depth still produces a picture, just a
    /// corrupt one, which is far harder to diagnose than a refused mode set.
    pub const fn from_format(fmt: super::super::mode::PixelFormat) -> KernelResult<Self> {
        use super::super::mode::PixelFormat;
        match fmt {
            PixelFormat::Xrgb8888 | PixelFormat::Argb8888 => Ok(Self::Bpp32),
            PixelFormat::Rgb565 => Ok(Self::Bpp16),
            // The R100 display block scans out BGR-ordered pixels only; the
            // channel-swapped formats would need the byte-swapper in
            // CNFG_CNTL, which is a separate piece of state this driver does
            // not yet program. Refusing is honest; accepting would give the
            // caller a picture with red and blue exchanged.
            PixelFormat::Xbgr8888 | PixelFormat::Abgr8888 => Err(KernelError::NotSupported),
        }
    }
}

// ---------------------------------------------------------------------------
// CRTC timing register packing
// ---------------------------------------------------------------------------

/// The four CRTC timing registers, computed from a mode.
///
/// Held as a struct rather than written directly so the arithmetic can be
/// tested without a device: these four `u32`s are the entire observable result
/// of mode-set timing computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrtcTiming {
    /// Value for [`CRTC_H_TOTAL_DISP`].
    pub h_total_disp: u32,
    /// Value for [`CRTC_H_SYNC_STRT_WID`].
    pub h_sync_strt_wid: u32,
    /// Value for [`CRTC_V_TOTAL_DISP`].
    pub v_total_disp: u32,
    /// Value for [`CRTC_V_SYNC_STRT_WID`].
    pub v_sync_strt_wid: u32,
}

/// Horizontal quantities are expressed in units of 8 pixels ("characters").
pub const H_CHAR: u32 = 8;

/// Largest encodable horizontal total, in characters (10-bit field).
const H_TOTAL_MAX: u32 = 0x3FF;
/// Largest encodable horizontal displayed, in characters (9-bit field).
const H_DISP_MAX: u32 = 0x1FF;
/// Largest encodable horizontal sync start, in characters (10-bit field).
const H_SYNC_STRT_CHAR_MAX: u32 = 0x3FF;
/// Largest encodable horizontal sync width, in *characters* (6-bit field).
///
/// Characters, not pixels — 63 characters is 504 pixels. Reading this field as
/// pixels is not a subtle error: the narrowest standard mode, 640x480@60, has a
/// 96-pixel sync pulse, so a pixel-denominated encoder overflows the field on
/// the single most universally-supported mode there is.
const H_SYNC_WID_MAX: u32 = 0x3F;
/// Largest encodable vertical total, in lines (12-bit field).
const V_TOTAL_MAX: u32 = 0xFFF;
/// Largest encodable vertical displayed, in lines (12-bit field).
const V_DISP_MAX: u32 = 0xFFF;
/// Largest encodable vertical sync start, in lines (12-bit field).
///
/// Unlike the totals, sync start is stored unbiased — the raw line number goes
/// straight into the register.
const V_SYNC_STRT_MAX: u32 = 0xFFF;
/// Largest encodable vertical sync width, in lines (5-bit field).
const V_SYNC_WID_MAX: u32 = 0x1F;

/// Sync polarity bit, shared by the horizontal and vertical sync registers.
pub const CRTC_SYNC_NEG: u32 = 1 << 23;

impl CrtcTiming {
    /// Compute the CRTC timing registers for a mode.
    ///
    /// Four separate encoding quirks apply here, and each is a place a
    /// plausible expression is wrong:
    ///
    /// - **Horizontal values are in characters of 8 pixels, not pixels.** A
    ///   horizontal total that is not a multiple of 8 cannot be represented,
    ///   so it is rejected rather than rounded — rounding would shift every
    ///   subsequent edge and detune the mode.
    /// - **Totals are stored biased by one.** The chip counts from zero, so
    ///   the stored value is `total/8 - 1`. Omitting the bias costs exactly
    ///   one character clock per line, which is the classic "image drifts
    ///   slowly sideways" fault.
    /// - **Horizontal sync start is split.** Its low 3 bits are a pixel
    ///   offset within the character at bits 0-2, and the character index sits
    ///   at bits 3-12 — so a sync start that is not character-aligned is still
    ///   representable, unlike the total.
    /// - **Horizontal sync *width* is in characters too, and is the one
    ///   quantity here that is rounded rather than rejected.** See below.
    ///
    /// ## Why sync width rounds when everything else refuses
    ///
    /// The rule this file otherwise follows is "refuse what cannot be
    /// represented exactly", because a silently-adjusted timing produces a
    /// picture that is subtly wrong instead of an error that names the
    /// problem. Sync width is the deliberate exception, and the distinction is
    /// between two different kinds of unrepresentable:
    ///
    /// - *Not on the granularity grid* — a 44-pixel sync pulse in a field that
    ///   counts 8-pixel characters. The error from truncating is bounded by
    ///   under 8 pixels of a pulse whose acceptable range spans tens of
    ///   pixels, and it is not avoidable: no encoding of this register can
    ///   express 44. Refusing would reject VESA 1920x1080@60, whose sync is
    ///   exactly 44 pixels wide — a mode this driver must support. So it
    ///   truncates. A width that truncates to zero rounds *up* to one instead,
    ///   because a zero-width pulse is not a narrow sync, it is no sync, and
    ///   the display would not lock at all.
    /// - *Beyond the field's magnitude* — a pulse wider than 63 characters
    ///   (504 pixels). That is not a granularity artifact; nothing legitimate
    ///   asks for it, and clamping would silently change the pulse by an
    ///   unbounded amount. So that is rejected, like every other overflow
    ///   here.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` for a mode that this CRTC cannot encode: a
    /// horizontal total that is not a multiple of 8, any field that exceeds
    /// its bit width, or a degenerate mode whose edges are not ordered
    /// `display <= sync_start < sync_end <= total`.
    pub fn from_mode(m: &super::timing::ModeTiming) -> KernelResult<Self> {
        // Ordering first: everything below assumes these hold, and a
        // subtraction that underflows would otherwise wrap to a huge value
        // that then fails a range check with a misleading message.
        if m.hdisplay > m.hsync_start
            || m.hsync_start >= m.hsync_end
            || m.hsync_end > m.htotal
            || m.vdisplay > m.vsync_start
            || m.vsync_start >= m.vsync_end
            || m.vsync_end > m.vtotal
        {
            return Err(KernelError::InvalidArgument);
        }
        // A zero-sized active area is not a mode. Checked explicitly because
        // the bias-by-one below would turn it into `0 - 1`.
        if m.hdisplay == 0 || m.vdisplay == 0 {
            return Err(KernelError::InvalidArgument);
        }

        if !m.htotal.is_multiple_of(H_CHAR) || !m.hdisplay.is_multiple_of(H_CHAR) {
            return Err(KernelError::InvalidArgument);
        }

        // Bias by one: the chip's counters are zero-based.
        let h_total = m
            .htotal
            .checked_div(H_CHAR)
            .and_then(|v| v.checked_sub(1))
            .ok_or(KernelError::InvalidArgument)?;
        let h_disp = m
            .hdisplay
            .checked_div(H_CHAR)
            .and_then(|v| v.checked_sub(1))
            .ok_or(KernelError::InvalidArgument)?;

        // Sync width in characters, truncating. `max(1)` because the ordering
        // check above only guarantees a pulse of at least one *pixel*, and a
        // sub-character pulse would truncate to a zero-width one — which the
        // chip emits as no sync pulse at all, so the display never locks.
        let h_sync_wid = m
            .hsync_end
            .checked_sub(m.hsync_start)
            .and_then(|w| w.checked_div(H_CHAR))
            .ok_or(KernelError::InvalidArgument)?
            .max(1);
        let h_sync_char = m.hsync_start.checked_div(H_CHAR).unwrap_or(0);
        let h_sync_pix = m.hsync_start % H_CHAR;

        let v_total = m
            .vtotal
            .checked_sub(1)
            .ok_or(KernelError::InvalidArgument)?;
        let v_disp = m
            .vdisplay
            .checked_sub(1)
            .ok_or(KernelError::InvalidArgument)?;
        let v_sync_wid = m
            .vsync_end
            .checked_sub(m.vsync_start)
            .ok_or(KernelError::InvalidArgument)?;

        // The `vsync_start` bound is, as the code stands, unreachable: the
        // ordering check requires `vsync_start < vsync_end <= vtotal`, and
        // `v_total = vtotal - 1 <= V_TOTAL_MAX` already caps vtotal at 4096, so
        // vsync_start cannot exceed 4095. It is stated anyway rather than left
        // implicit, because it is a property of *this field* and not of the
        // vertical total — the two happen to coincide today, and a later change
        // to either bound should not silently remove a check that was never
        // written down.
        if h_total > H_TOTAL_MAX
            || h_disp > H_DISP_MAX
            || h_sync_char > H_SYNC_STRT_CHAR_MAX
            || h_sync_wid > H_SYNC_WID_MAX
            || v_total > V_TOTAL_MAX
            || v_disp > V_DISP_MAX
            || m.vsync_start > V_SYNC_STRT_MAX
            || v_sync_wid > V_SYNC_WID_MAX
        {
            return Err(KernelError::InvalidArgument);
        }

        let h_neg = if m.hsync_negative { CRTC_SYNC_NEG } else { 0 };
        let v_neg = if m.vsync_negative { CRTC_SYNC_NEG } else { 0 };

        Ok(Self {
            h_total_disp: h_total | (h_disp << 16),
            h_sync_strt_wid: h_sync_pix | (h_sync_char << 3) | (h_sync_wid << 16) | h_neg,
            v_total_disp: v_total | (v_disp << 16),
            v_sync_strt_wid: m.vsync_start | (v_sync_wid << 16) | v_neg,
        })
    }
}

/// Encode a scanout stride for `CRTC_PITCH`.
///
/// The register counts in units of 8 pixels, *not* bytes — so the byte pitch
/// must be converted through the pixel size, and a stride that is not a whole
/// number of 8-pixel groups cannot be expressed.
///
/// # Errors
///
/// `InvalidArgument` if the pitch is not a multiple of 8 pixels, if the pixel
/// size is zero, or if the result does not fit the field.
pub fn encode_pitch(pitch_bytes: u32, bytes_per_pixel: u32) -> KernelResult<u32> {
    if bytes_per_pixel == 0 {
        return Err(KernelError::InvalidArgument);
    }
    if !pitch_bytes.is_multiple_of(bytes_per_pixel) {
        return Err(KernelError::InvalidArgument);
    }
    let pixels = pitch_bytes
        .checked_div(bytes_per_pixel)
        .ok_or(KernelError::InvalidArgument)?;
    if !pixels.is_multiple_of(H_CHAR) {
        return Err(KernelError::InvalidArgument);
    }
    let chars = pixels
        .checked_div(H_CHAR)
        .ok_or(KernelError::InvalidArgument)?;
    // The pitch field is 11 bits on these parts.
    if chars > 0x7FF {
        return Err(KernelError::InvalidArgument);
    }
    Ok(chars)
}

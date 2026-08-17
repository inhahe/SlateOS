//! Full display timings, and the VESA DMT table they come from.
//!
//! [`super::super::mode::DrmMode`] carries only `hdisplay`/`vdisplay`/
//! `htotal`/`vtotal`, which is enough for a paravirtualised backend where the
//! host owns the signal, and not enough to program a real CRTC: nothing in it
//! says where sync begins, how long it lasts, or which way round its polarity
//! goes. [`ModeTiming`] is that missing detail.
//!
//! The values are a **table of published VESA DMT timings**, not a formula.
//! That is a deliberate choice. A generator (CVT, GTF) will produce timings
//! that are *arithmetically* reasonable and differ from the standard ones a
//! monitor's EDID advertises, and a display handed a mode a few pixels away
//! from the one it expects does not fail cleanly — it may sync, and show a
//! shifted or shimmering picture. Since the standard modes are a short fixed
//! list, storing them exactly removes that whole class of near-miss. A
//! generator remains the right answer for modes *not* in the list, which is
//! why [`ModeTiming::new`] exists to build one directly.

use crate::error::{KernelError, KernelResult};

/// A complete CRTC timing: every edge, in pixels and lines.
///
/// Invariant, checked by [`ModeTiming::validate`] and relied upon by
/// [`super::regs::CrtcTiming::from_mode`]:
/// `display <= sync_start < sync_end <= total` on both axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeTiming {
    /// Pixel clock in kHz.
    pub clock_khz: u32,
    /// Horizontal active pixels.
    pub hdisplay: u32,
    /// Pixel at which horizontal sync asserts.
    pub hsync_start: u32,
    /// Pixel at which horizontal sync deasserts.
    pub hsync_end: u32,
    /// Total pixels per line, including blanking.
    pub htotal: u32,
    /// Vertical active lines.
    pub vdisplay: u32,
    /// Line at which vertical sync asserts.
    pub vsync_start: u32,
    /// Line at which vertical sync deasserts.
    pub vsync_end: u32,
    /// Total lines per frame, including blanking.
    pub vtotal: u32,
    /// Horizontal sync is active-low.
    pub hsync_negative: bool,
    /// Vertical sync is active-low.
    pub vsync_negative: bool,
}

impl ModeTiming {
    /// Build a timing from explicit edges.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if the edges are not ordered, or the mode is
    /// degenerate (zero active area, zero clock).
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        clock_khz: u32,
        hdisplay: u32,
        hsync_start: u32,
        hsync_end: u32,
        htotal: u32,
        vdisplay: u32,
        vsync_start: u32,
        vsync_end: u32,
        vtotal: u32,
        hsync_negative: bool,
        vsync_negative: bool,
    ) -> KernelResult<Self> {
        let m = Self {
            clock_khz,
            hdisplay,
            hsync_start,
            hsync_end,
            htotal,
            vdisplay,
            vsync_start,
            vsync_end,
            vtotal,
            hsync_negative,
            vsync_negative,
        };
        match m.validate() {
            Ok(()) => Ok(m),
            Err(e) => Err(e),
        }
    }

    /// Check the edge-ordering invariant.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if any edge is out of order or the mode is
    /// degenerate.
    pub const fn validate(&self) -> KernelResult<()> {
        if self.clock_khz == 0 || self.hdisplay == 0 || self.vdisplay == 0 {
            return Err(KernelError::InvalidArgument);
        }
        if self.hdisplay > self.hsync_start
            || self.hsync_start >= self.hsync_end
            || self.hsync_end > self.htotal
        {
            return Err(KernelError::InvalidArgument);
        }
        if self.vdisplay > self.vsync_start
            || self.vsync_start >= self.vsync_end
            || self.vsync_end > self.vtotal
        {
            return Err(KernelError::InvalidArgument);
        }
        Ok(())
    }

    /// Refresh rate in centi-hertz (hundredths of a hertz).
    ///
    /// Centi-hertz rather than hertz because the useful distinctions here are
    /// fractional: 59.94 Hz and 60 Hz are different modes, and integer hertz
    /// cannot tell them apart. Returns `None` if the mode is degenerate, which
    /// is reported rather than papered over with a zero — a caller comparing
    /// refresh rates must not read a computation failure as "0 Hz".
    #[must_use]
    pub fn refresh_centihz(&self) -> Option<u32> {
        let total = u64::from(self.htotal).checked_mul(u64::from(self.vtotal))?;
        if total == 0 {
            return None;
        }
        // clock is kHz; ×100_000 converts to centi-hertz before the divide, so
        // the division happens once, at the end, at full precision.
        let num = u64::from(self.clock_khz).checked_mul(100_000)?;
        u32::try_from(num.checked_div(total)?).ok()
    }
}

/// One entry of the VESA DMT table.
pub struct DmtMode {
    /// Active width.
    pub width: u32,
    /// Active height.
    pub height: u32,
    /// Nominal refresh in Hz, as the standard names it.
    pub refresh: u32,
    /// The published timing.
    pub timing: ModeTiming,
}

/// Build a `ModeTiming` in a const context, panicking on a malformed literal.
///
/// This is only ever applied to the fixed literals in [`DMT_MODES`] below, so
/// the panic is a compile-time assertion about this file's own contents rather
/// than a runtime failure path: a typo'd constant fails the build.
macro_rules! dmt {
    (
        $w:expr, $h:expr, $r:expr, $clk:expr,
        $hss:expr, $hse:expr, $ht:expr,
        $vss:expr, $vse:expr, $vt:expr,
        $hneg:expr, $vneg:expr
    ) => {
        DmtMode {
            width: $w,
            height: $h,
            refresh: $r,
            timing: match ModeTiming::new(
                $clk, $w, $hss, $hse, $ht, $h, $vss, $vse, $vt, $hneg, $vneg,
            ) {
                Ok(m) => m,
                Err(_) => panic!("malformed DMT literal in ati::timing"),
            },
        }
    };
}

/// The VESA DMT modes this driver can set.
///
/// Sourced from the VESA Discrete Monitor Timings standard; the same values
/// Linux carries in `drm_dmt_modes[]`. Ordered by area then refresh so a
/// "largest mode that fits" search can walk it backwards.
pub static DMT_MODES: &[DmtMode] = &[
    // 640x480@60 — the universally-supported fallback. Both syncs negative.
    dmt!(
        640, 480, 60, 25_175, 656, 752, 800, 490, 492, 525, true, true
    ),
    // 800x600@60 — both syncs positive, unlike its 640x480 neighbour.
    dmt!(
        800, 600, 60, 40_000, 840, 968, 1056, 601, 605, 628, false, false
    ),
    // 1024x768@60 — both negative again. The polarity alternation across
    // these three is not a pattern with a rule; it is why the table stores it.
    dmt!(
        1024, 768, 60, 65_000, 1048, 1184, 1344, 771, 777, 806, true, true
    ),
    // 1280x1024@60
    dmt!(
        1280, 1024, 60, 108_000, 1328, 1440, 1688, 1025, 1028, 1066, false, false
    ),
    // 1920x1080@60
    dmt!(
        1920, 1080, 60, 148_500, 2008, 2052, 2200, 1084, 1089, 1125, false, false
    ),
];

/// Look up a published timing by resolution and nominal refresh.
///
/// Returns `None` rather than the nearest match: a caller asking for a mode
/// this driver has no exact timing for needs to know that, not to be silently
/// given a different one.
#[must_use]
pub fn lookup(width: u32, height: u32, refresh: u32) -> Option<&'static ModeTiming> {
    DMT_MODES
        .iter()
        .find(|m| m.width == width && m.height == height && m.refresh == refresh)
        .map(|m| &m.timing)
}

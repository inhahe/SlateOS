//! Boot-time self-test for the pure ATI register/timing layer.
//!
//! This runs on every boot, on a machine that need not have an ATI device in
//! it, because everything under test is a pure function of its inputs. That is
//! the point of the pure/MMIO split described in [`super`]: the arithmetic
//! that decides *what* to write to a CRTC is where display drivers go wrong,
//! and it is checkable without a CRTC.
//!
//! The register expectations below are written as literal constants, computed
//! by hand from the encoding rules and cross-checked against the published
//! VESA DMT timings. They are deliberately not recomputed from the same
//! expressions the code under test uses — a test that re-derives its
//! expectation with the implementation's own formula passes whatever that
//! formula says, including a wrong one. Every value here is independent of the
//! code that produces it.

use super::mmio;
use super::modeset;
use super::regs::{self, CrtcTiming, PixWidth};
use super::timing::{self, DMT_MODES, ModeTiming};
use super::vram;
use crate::drm::mode::PixelFormat;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Running tally of checks.
///
/// Counts rather than panicking so one wrong constant reports every other
/// failure alongside it. A self-test that aborts on the first failure tells
/// you one thing per boot; this tells you the shape of the breakage in a
/// single run, which is the difference between "the bias is missing" and
/// "the entire horizontal encoder is wrong".
struct Check {
    passed: u32,
    failed: u32,
}

impl Check {
    const fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
        }
    }

    fn pass(&mut self) {
        self.passed = self.passed.saturating_add(1);
    }

    fn fail(&mut self) {
        self.failed = self.failed.saturating_add(1);
    }

    /// Assert a `u32` equals its expected value, reporting both in hex.
    fn eq_u32(&mut self, what: &str, got: u32, want: u32) {
        if got == want {
            self.pass();
        } else {
            self.fail();
            serial_println!(
                "[ati]   FAIL: {}: got {:#010x}, want {:#010x}",
                what,
                got,
                want
            );
        }
    }

    /// Assert a boolean condition.
    fn is_true(&mut self, what: &str, cond: bool) {
        if cond {
            self.pass();
        } else {
            self.fail();
            serial_println!("[ati]   FAIL: {}", what);
        }
    }

    /// Assert a result is the specific error expected.
    ///
    /// The error *value* is checked, not merely that one occurred: these
    /// functions distinguish "this CRTC cannot encode that" (`InvalidArgument`)
    /// from "this display block cannot scan out that format at all"
    /// (`NotSupported`), and a test that accepted any error would let the two
    /// be swapped silently.
    fn is_err<T>(&mut self, what: &str, got: KernelResult<T>, want: KernelError) {
        match got {
            Err(e) if e == want => self.pass(),
            Err(e) => {
                self.fail();
                serial_println!(
                    "[ati]   FAIL: {}: wrong error {:?}, want {:?}",
                    what,
                    e,
                    want
                );
            }
            Ok(_) => {
                self.fail();
                serial_println!("[ati]   FAIL: {}: accepted, expected {:?}", what, want);
            }
        }
    }

    /// Unwrap a result that must succeed, reporting instead of panicking.
    fn expect_ok<T>(&mut self, what: &str, got: KernelResult<T>) -> Option<T> {
        match got {
            Ok(v) => {
                self.pass();
                Some(v)
            }
            Err(e) => {
                self.fail();
                serial_println!("[ati]   FAIL: {}: unexpected error {:?}", what, e);
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Expected CRTC register values for the DMT table
// ---------------------------------------------------------------------------

/// One mode's expected encoding.
struct TimingCase {
    label: &'static str,
    width: u32,
    height: u32,
    refresh: u32,
    h_total_disp: u32,
    h_sync_strt_wid: u32,
    v_total_disp: u32,
    v_sync_strt_wid: u32,
}

/// Hand-computed expectations for every entry of [`DMT_MODES`].
///
/// Worked example, 640x480@60 (htotal 800, hdisplay 640, hsync 656..752;
/// vtotal 525, vdisplay 480, vsync 490..492; both syncs negative):
///
/// - `h_total = 800/8 - 1 = 99 = 0x63`, `h_disp = 640/8 - 1 = 79 = 0x4F`,
///   packed as `h_disp << 16 | h_total` = `0x004F_0063`.
/// - `h_sync_wid = (752-656)/8 = 12`, sync start 656 goes in raw (its low 3
///   bits are the pixel offset, 0 here), negative polarity sets bit 23:
///   `0x0080_0000 | 0x000C_0000 | 0x290` = `0x008C_0290`.
/// - `v_total = 524 = 0x20C`, `v_disp = 479 = 0x1DF` → `0x01DF_020C`.
/// - `v_sync_wid = 2` lines, start 490 = 0x1EA raw, negative → `0x0082_01EA`.
static TIMING_CASES: &[TimingCase] = &[
    TimingCase {
        label: "640x480@60",
        width: 640,
        height: 480,
        refresh: 60,
        h_total_disp: 0x004F_0063,
        h_sync_strt_wid: 0x008C_0290,
        v_total_disp: 0x01DF_020C,
        v_sync_strt_wid: 0x0082_01EA,
    },
    TimingCase {
        label: "800x600@60",
        width: 800,
        height: 600,
        refresh: 60,
        // 1056/8-1 = 131 = 0x83; 800/8-1 = 99 = 0x63.
        h_total_disp: 0x0063_0083,
        // (968-840)/8 = 16 = 0x10; start 840 = 0x348; positive polarity.
        h_sync_strt_wid: 0x0010_0348,
        // 627 = 0x273; 599 = 0x257.
        v_total_disp: 0x0257_0273,
        // 605-601 = 4; start 601 = 0x259.
        v_sync_strt_wid: 0x0004_0259,
    },
    TimingCase {
        label: "1024x768@60",
        width: 1024,
        height: 768,
        refresh: 60,
        // 1344/8-1 = 167 = 0xA7; 1024/8-1 = 127 = 0x7F.
        h_total_disp: 0x007F_00A7,
        // (1184-1048)/8 = 17 = 0x11; start 1048 = 0x418; negative.
        h_sync_strt_wid: 0x0091_0418,
        // 805 = 0x325; 767 = 0x2FF.
        v_total_disp: 0x02FF_0325,
        // 777-771 = 6; start 771 = 0x303; negative.
        v_sync_strt_wid: 0x0086_0303,
    },
    TimingCase {
        label: "1280x1024@60",
        width: 1280,
        height: 1024,
        refresh: 60,
        // 1688/8-1 = 210 = 0xD2; 1280/8-1 = 159 = 0x9F.
        h_total_disp: 0x009F_00D2,
        // (1440-1328)/8 = 14 = 0xE; start 1328 = 0x530; positive.
        h_sync_strt_wid: 0x000E_0530,
        // 1065 = 0x429; 1023 = 0x3FF.
        v_total_disp: 0x03FF_0429,
        // 1028-1025 = 3; start 1025 = 0x401.
        v_sync_strt_wid: 0x0003_0401,
    },
    TimingCase {
        label: "1920x1080@60",
        width: 1920,
        height: 1080,
        refresh: 60,
        // 2200/8-1 = 274 = 0x112; 1920/8-1 = 239 = 0xEF.
        h_total_disp: 0x00EF_0112,
        // (2052-2008) = 44 pixels, which is 5.5 characters: this is the mode
        // that forces the truncation rule, encoding as 5.  Start 2008 = 0x7D8.
        h_sync_strt_wid: 0x0005_07D8,
        // 1124 = 0x464; 1079 = 0x437.
        v_total_disp: 0x0437_0464,
        // 1089-1084 = 5; start 1084 = 0x43C.
        v_sync_strt_wid: 0x0005_043C,
    },
];

// ---------------------------------------------------------------------------
// Mode construction helpers
// ---------------------------------------------------------------------------

/// A valid baseline mode (640x480@60) to perturb one field at a time.
///
/// Built by struct literal rather than [`ModeTiming::new`] so the rejection
/// tests below can produce modes that `new` would refuse — the point being to
/// check that `from_mode` refuses them on its own, rather than inheriting a
/// guarantee from a constructor a future caller might bypass.
const fn base_mode() -> ModeTiming {
    ModeTiming {
        clock_khz: 25_175,
        hdisplay: 640,
        hsync_start: 656,
        hsync_end: 752,
        htotal: 800,
        vdisplay: 480,
        vsync_start: 490,
        vsync_end: 492,
        vtotal: 525,
        hsync_negative: true,
        vsync_negative: true,
    }
}

// ---------------------------------------------------------------------------
// Test groups
// ---------------------------------------------------------------------------

/// The DMT table is internally consistent and every entry encodes.
fn test_dmt_table(c: &mut Check) {
    c.is_true("DMT table is non-empty", !DMT_MODES.is_empty());

    for m in DMT_MODES {
        // The table's declared resolution must match the timing it carries;
        // a mismatch would make `lookup` return a timing for a different mode.
        c.is_true(
            "DMT width matches timing hdisplay",
            m.width == m.timing.hdisplay,
        );
        c.is_true(
            "DMT height matches timing vdisplay",
            m.height == m.timing.vdisplay,
        );
        c.is_true("DMT entry validates", m.timing.validate().is_ok());
        c.is_true(
            "DMT entry encodes to CRTC registers",
            CrtcTiming::from_mode(&m.timing).is_ok(),
        );
    }
}

/// Every DMT mode encodes to its hand-computed register values.
fn test_dmt_registers(c: &mut Check) {
    for case in TIMING_CASES {
        let Some(m) = timing::lookup(case.width, case.height, case.refresh) else {
            c.fail();
            serial_println!("[ati]   FAIL: {}: not found in DMT table", case.label);
            continue;
        };
        let Some(t) = c.expect_ok(case.label, CrtcTiming::from_mode(m)) else {
            continue;
        };
        c.eq_u32(case.label, t.h_total_disp, case.h_total_disp);
        c.eq_u32(case.label, t.h_sync_strt_wid, case.h_sync_strt_wid);
        c.eq_u32(case.label, t.v_total_disp, case.v_total_disp);
        c.eq_u32(case.label, t.v_sync_strt_wid, case.v_sync_strt_wid);
    }
}

/// The four encoding quirks, each isolated.
fn test_encoding_quirks(c: &mut Check) {
    // --- Quirk 1: totals are biased by one -------------------------------
    //
    // Checked as an explicit inequality as well as an equality, because the
    // failure this guards against is a *missing* bias, and `!= htotal/8` is
    // the direct statement of that.  800/8 = 100, so the field must read 99.
    let m = base_mode();
    if let Some(t) = c.expect_ok("bias baseline encodes", CrtcTiming::from_mode(&m)) {
        c.eq_u32("h_total is biased by one", t.h_total_disp & 0x3FF, 99);
        c.is_true(
            "h_total is not the unbiased quotient",
            (t.h_total_disp & 0x3FF) != 100,
        );
        c.eq_u32(
            "h_disp is biased by one",
            (t.h_total_disp >> 16) & 0x1FF,
            79,
        );
        c.eq_u32("v_total is biased by one", t.v_total_disp & 0xFFF, 524);
        c.eq_u32(
            "v_disp is biased by one",
            (t.v_total_disp >> 16) & 0xFFF,
            479,
        );
    }

    // --- Quirk 2: horizontal sync start is split, pixel offset in bits 0-2 -
    //
    // 660 = 82 characters + 4 pixels.  The two halves recombine to the
    // original value, which is why the implementation can store it whole; the
    // test decomposes it to prove the halves land in the right bits rather
    // than merely that the sum is right.
    let mut m = base_mode();
    m.hsync_start = 660;
    if let Some(t) = c.expect_ok("unaligned hsync encodes", CrtcTiming::from_mode(&m)) {
        c.eq_u32("hsync pixel offset in bits 0-2", t.h_sync_strt_wid & 0x7, 4);
        c.eq_u32(
            "hsync character index in bits 3-12",
            (t.h_sync_strt_wid >> 3) & 0x3FF,
            82,
        );
        c.eq_u32(
            "hsync start recombines to the raw pixel value",
            t.h_sync_strt_wid & 0x1FFF,
            660,
        );
    }

    // --- Quirk 3: sync width truncates to characters ----------------------
    //
    // A 44-pixel pulse is 5.5 characters and must encode as 5.  This is the
    // one rounding in the whole encoder, and 1920x1080@60 is the mode that
    // forces it, so it is checked directly rather than only via the table.
    let mut m = base_mode();
    m.hsync_start = 648;
    m.hsync_end = 692; // 44 pixels
    if let Some(t) = c.expect_ok("44px sync encodes", CrtcTiming::from_mode(&m)) {
        c.eq_u32(
            "44px sync truncates to 5 characters",
            (t.h_sync_strt_wid >> 16) & 0x3F,
            5,
        );
    }

    // A sub-character pulse must round *up* to one, never down to zero: zero
    // is not a narrow sync pulse, it is the absence of one, and the display
    // would not lock at all.
    let mut m = base_mode();
    m.hsync_start = 648;
    m.hsync_end = 652; // 4 pixels — truncates to 0
    if let Some(t) = c.expect_ok("4px sync encodes", CrtcTiming::from_mode(&m)) {
        c.eq_u32(
            "sub-character sync rounds up to 1, not down to 0",
            (t.h_sync_strt_wid >> 16) & 0x3F,
            1,
        );
    }

    // --- Quirk 4: sync polarity ------------------------------------------
    //
    // Both polarities are exercised in both axes.  The DMT table contains
    // modes of each polarity, but a driver that ignored the flag entirely
    // would still pass those if only one polarity were checked here.
    let mut m = base_mode();
    m.hsync_negative = false;
    m.vsync_negative = false;
    if let Some(t) = c.expect_ok("positive-polarity mode encodes", CrtcTiming::from_mode(&m)) {
        c.is_true(
            "positive hsync clears the polarity bit",
            t.h_sync_strt_wid & regs::CRTC_SYNC_NEG == 0,
        );
        c.is_true(
            "positive vsync clears the polarity bit",
            t.v_sync_strt_wid & regs::CRTC_SYNC_NEG == 0,
        );
    }
    let mut m = base_mode();
    m.hsync_negative = true;
    m.vsync_negative = true;
    if let Some(t) = c.expect_ok("negative-polarity mode encodes", CrtcTiming::from_mode(&m)) {
        c.is_true(
            "negative hsync sets the polarity bit",
            t.h_sync_strt_wid & regs::CRTC_SYNC_NEG != 0,
        );
        c.is_true(
            "negative vsync sets the polarity bit",
            t.v_sync_strt_wid & regs::CRTC_SYNC_NEG != 0,
        );
    }

    // The two polarities must be independent — a driver that derived vsync's
    // polarity from hsync's would pass every DMT mode in the table, because
    // all five happen to use the same polarity on both axes.
    let mut m = base_mode();
    m.hsync_negative = true;
    m.vsync_negative = false;
    if let Some(t) = c.expect_ok("mixed-polarity mode encodes", CrtcTiming::from_mode(&m)) {
        c.is_true(
            "mixed polarity: hsync negative",
            t.h_sync_strt_wid & regs::CRTC_SYNC_NEG != 0,
        );
        c.is_true(
            "mixed polarity: vsync positive",
            t.v_sync_strt_wid & regs::CRTC_SYNC_NEG == 0,
        );
    }
}

/// Modes this CRTC cannot encode must be refused, not approximated.
fn test_rejections(c: &mut Check) {
    let bad = KernelError::InvalidArgument;

    // Horizontal quantities must be whole characters.  Rounding a total would
    // shift every subsequent edge and detune the mode, so it is refused.
    let mut m = base_mode();
    m.htotal = 804;
    c.is_err("htotal not a multiple of 8", CrtcTiming::from_mode(&m), bad);

    let mut m = base_mode();
    m.hdisplay = 644;
    m.hsync_start = 656;
    c.is_err(
        "hdisplay not a multiple of 8",
        CrtcTiming::from_mode(&m),
        bad,
    );

    // Edge ordering: display <= sync_start < sync_end <= total, both axes.
    let mut m = base_mode();
    m.hdisplay = 664; // past hsync_start
    c.is_err("hdisplay past hsync_start", CrtcTiming::from_mode(&m), bad);

    let mut m = base_mode();
    m.hsync_end = 656; // equal to start: zero-width sync
    c.is_err(
        "hsync_end equals hsync_start",
        CrtcTiming::from_mode(&m),
        bad,
    );

    let mut m = base_mode();
    m.hsync_end = 808; // past htotal
    c.is_err("hsync_end past htotal", CrtcTiming::from_mode(&m), bad);

    let mut m = base_mode();
    m.vdisplay = 496; // past vsync_start
    c.is_err("vdisplay past vsync_start", CrtcTiming::from_mode(&m), bad);

    let mut m = base_mode();
    m.vsync_end = 490; // equal to start
    c.is_err(
        "vsync_end equals vsync_start",
        CrtcTiming::from_mode(&m),
        bad,
    );

    // vsync_start is moved up with it so the resulting pulse is only 10 lines
    // wide: overshooting vtotal from the baseline start of 490 would also
    // overflow the 5-bit width field, and the case would no longer isolate the
    // ordering check it is named for.
    let mut m = base_mode();
    m.vsync_start = 520;
    m.vsync_end = 530; // past vtotal
    c.is_err("vsync_end past vtotal", CrtcTiming::from_mode(&m), bad);

    // Degenerate active area.  Checked explicitly because the bias-by-one
    // subtraction would otherwise underflow on a zero.
    let mut m = base_mode();
    m.hdisplay = 0;
    c.is_err("zero hdisplay", CrtcTiming::from_mode(&m), bad);

    let mut m = base_mode();
    m.vdisplay = 0;
    c.is_err("zero vdisplay", CrtcTiming::from_mode(&m), bad);

    // Field overflow.  Distinct from the granularity cases above: these are
    // magnitudes the register cannot hold at any rounding.
    let mut m = base_mode();
    m.htotal = 8200; // 8200/8-1 = 1024, one past the 10-bit field
    c.is_err("htotal overflows its field", CrtcTiming::from_mode(&m), bad);

    let mut m = base_mode();
    m.hdisplay = 4104; // 4104/8-1 = 512, one past the 9-bit field
    m.hsync_start = 4112;
    m.hsync_end = 4120;
    m.htotal = 4200;
    c.is_err(
        "hdisplay overflows its field",
        CrtcTiming::from_mode(&m),
        bad,
    );

    let mut m = base_mode();
    m.vtotal = 4098; // 4097, one past the 12-bit field
    c.is_err("vtotal overflows its field", CrtcTiming::from_mode(&m), bad);

    // vtotal is widened alongside vsync_end here on purpose: leaving it at the
    // baseline 525 would put vsync_end past vtotal, and the mode would be
    // refused by the *ordering* check before the width check was ever reached.
    // The test would still have passed, while testing nothing it claimed to.
    let mut m = base_mode();
    m.vsync_end = 530; // 40 lines, past the 5-bit field
    m.vtotal = 600;
    c.is_err(
        "vsync width overflows its field",
        CrtcTiming::from_mode(&m),
        bad,
    );

    // A sync pulse wider than 63 characters (504 pixels) is not a rounding
    // problem, so unlike the 44-pixel case it is refused rather than clamped.
    let mut m = base_mode();
    m.hsync_start = 648;
    m.hsync_end = 1168; // 520 pixels = 65 characters
    m.htotal = 1176;
    c.is_err(
        "hsync width overflows its field",
        CrtcTiming::from_mode(&m),
        bad,
    );
}

/// Pixel-format mapping, including the formats that must be refused.
fn test_pix_width(c: &mut Check) {
    c.is_true(
        "Xrgb8888 maps to 32bpp",
        matches!(
            PixWidth::from_format(PixelFormat::Xrgb8888),
            Ok(PixWidth::Bpp32)
        ),
    );
    c.is_true(
        "Argb8888 maps to 32bpp",
        matches!(
            PixWidth::from_format(PixelFormat::Argb8888),
            Ok(PixWidth::Bpp32)
        ),
    );
    c.is_true(
        "Rgb565 maps to 16bpp",
        matches!(
            PixWidth::from_format(PixelFormat::Rgb565),
            Ok(PixWidth::Bpp16)
        ),
    );

    // The channel-swapped formats are refused rather than aliased onto their
    // RGB counterparts.  Accepting them would produce a picture with red and
    // blue exchanged — a fault that looks like a bug anywhere but here.
    c.is_err(
        "Xbgr8888 is refused",
        PixWidth::from_format(PixelFormat::Xbgr8888),
        KernelError::NotSupported,
    );
    c.is_err(
        "Abgr8888 is refused",
        PixWidth::from_format(PixelFormat::Abgr8888),
        KernelError::NotSupported,
    );

    // The chip's depth codes are not a bytes-per-pixel count and cannot be
    // derived arithmetically, so the shifted encodings are checked literally.
    c.eq_u32("Bpp8 shifted", PixWidth::Bpp8.shifted(), 0x0000_0200);
    c.eq_u32("Bpp15 shifted", PixWidth::Bpp15.shifted(), 0x0000_0300);
    c.eq_u32("Bpp16 shifted", PixWidth::Bpp16.shifted(), 0x0000_0400);
    c.eq_u32("Bpp24 shifted", PixWidth::Bpp24.shifted(), 0x0000_0500);
    c.eq_u32("Bpp32 shifted", PixWidth::Bpp32.shifted(), 0x0000_0600);
    c.is_true(
        "depth codes land inside the pix-width field",
        PixWidth::Bpp32.shifted() & !regs::CRTC_PIX_WIDTH_MASK == 0,
    );
}

/// Scanout stride encoding.
fn test_pitch(c: &mut Check) {
    // 1920 pixels at 4 bytes = 7680 bytes = 240 characters.
    if let Some(v) = c.expect_ok("1920x32bpp pitch encodes", regs::encode_pitch(7680, 4)) {
        c.eq_u32("1920x32bpp pitch", v, 240);
    }
    // 640 pixels at 4 bytes = 2560 bytes = 80 characters.
    if let Some(v) = c.expect_ok("640x32bpp pitch encodes", regs::encode_pitch(2560, 4)) {
        c.eq_u32("640x32bpp pitch", v, 80);
    }
    // 640 pixels at 2 bytes = 1280 bytes = 80 characters.  Same pixel count,
    // half the bytes: the register counts pixels, so the answer must not move.
    if let Some(v) = c.expect_ok("640x16bpp pitch encodes", regs::encode_pitch(1280, 2)) {
        c.eq_u32("640x16bpp pitch counts pixels, not bytes", v, 80);
    }

    let bad = KernelError::InvalidArgument;
    c.is_err("zero bytes-per-pixel", regs::encode_pitch(7680, 0), bad);
    c.is_err(
        "pitch not a whole pixel count",
        regs::encode_pitch(7681, 4),
        bad,
    );
    c.is_err(
        "pitch not a whole character count",
        regs::encode_pitch(16, 4),
        bad,
    );
    // 16384 pixels = 2048 characters, past the 11-bit field.
    c.is_err(
        "pitch overflows its field",
        regs::encode_pitch(65_536, 4),
        bad,
    );
}

/// Refresh-rate computation, in centi-hertz.
fn test_refresh(c: &mut Check) {
    // 640x480@60 is really 59.94 Hz, and 1920x1080@60 is exactly 60.00.  Both
    // are checked exactly, because telling those two apart is the entire
    // reason the function returns hundredths rather than whole hertz — an
    // integer-hertz version would report both as 60 and lose the distinction.
    if let Some(m) = timing::lookup(640, 480, 60) {
        match m.refresh_centihz() {
            Some(r) => c.eq_u32("640x480@60 is 59.94 Hz", r, 5994),
            None => {
                c.fail();
                serial_println!("[ati]   FAIL: 640x480@60 refresh returned None");
            }
        }
    }
    if let Some(m) = timing::lookup(1920, 1080, 60) {
        match m.refresh_centihz() {
            Some(r) => c.eq_u32("1920x1080@60 is exactly 60.00 Hz", r, 6000),
            None => {
                c.fail();
                serial_println!("[ati]   FAIL: 1920x1080@60 refresh returned None");
            }
        }
    }

    // Every table entry should land near its nominal rate.  A loose band, on
    // purpose: this catches a broken computation (wrong scale, wrong operand
    // order) without asserting a value that is really a property of the VESA
    // standard rather than of this code.
    for m in DMT_MODES {
        match m.timing.refresh_centihz() {
            Some(r) => c.is_true(
                "DMT refresh is within 1 Hz of nominal",
                r >= 5900 && r <= 6100,
            ),
            None => {
                c.fail();
                serial_println!("[ati]   FAIL: DMT entry refresh returned None");
            }
        }
    }

    // A degenerate mode reports failure rather than "0 Hz".  A caller
    // comparing refresh rates must not read a computation failure as a real
    // measurement.
    let mut m = base_mode();
    m.htotal = 0;
    m.vtotal = 0;
    c.is_true(
        "degenerate mode has no refresh rate",
        m.refresh_centihz().is_none(),
    );
}

/// `ModeTiming::new` validation and table lookup.
fn test_mode_lookup(c: &mut Check) {
    // The validating constructor accepts a good mode...
    let good = ModeTiming::new(25_175, 640, 656, 752, 800, 480, 490, 492, 525, true, true);
    c.is_true("ModeTiming::new accepts a valid mode", good.is_ok());

    // ...and refuses the degenerate and mis-ordered ones.
    let bad = KernelError::InvalidArgument;
    c.is_err(
        "ModeTiming::new refuses a zero clock",
        ModeTiming::new(0, 640, 656, 752, 800, 480, 490, 492, 525, true, true),
        bad,
    );
    c.is_err(
        "ModeTiming::new refuses mis-ordered hsync",
        ModeTiming::new(25_175, 640, 760, 752, 800, 480, 490, 492, 525, true, true),
        bad,
    );
    c.is_err(
        "ModeTiming::new refuses a zero active area",
        ModeTiming::new(25_175, 0, 656, 752, 800, 480, 490, 492, 525, true, true),
        bad,
    );

    // Lookup returns the exact mode asked for, or nothing.  A near match is
    // worse than no match: the caller would program a timing it did not ask
    // for and could not tell apart from the one it wanted.
    c.is_true(
        "lookup finds 1024x768@60",
        timing::lookup(1024, 768, 60).is_some(),
    );
    c.is_true(
        "lookup misses an absent resolution",
        timing::lookup(1366, 768, 60).is_none(),
    );
    c.is_true(
        "lookup misses an absent refresh rate",
        timing::lookup(1920, 1080, 75).is_none(),
    );
}

/// PCI device identification.
fn test_identify(c: &mut Check) {
    match super::identify(super::ATI_VENDOR_ID, 0x5046) {
        Some(info) => c.is_true(
            "Rage 128 Pro identifies",
            info.family == super::AsicFamily::Rage128,
        ),
        None => {
            c.fail();
            serial_println!("[ati]   FAIL: 0x5046 not identified");
        }
    }
    match super::identify(super::ATI_VENDOR_ID, 0x5159) {
        Some(info) => c.is_true("RV100 identifies", info.family == super::AsicFamily::R100),
        None => {
            c.fail();
            serial_println!("[ati]   FAIL: 0x5159 not identified");
        }
    }

    // An unknown ATI part is *not* claimed.  Claiming the vendor ID wholesale
    // would mean guessing at a register layout during probe, with the display
    // about to be reprogrammed; declining costs only a fallback to the
    // bootloader framebuffer.
    c.is_true(
        "an unknown ATI device is not claimed",
        super::identify(super::ATI_VENDOR_ID, 0x1234).is_none(),
    );
    c.is_true(
        "another vendor's matching device ID is not claimed",
        super::identify(0x10DE, 0x5046).is_none(),
    );
}

/// Register-offset validation, the one part of the MMIO layer that is
/// decidable without a device.
///
/// Worth testing on every boot rather than reasoning about once: this function
/// is the sole thing standing between a caller's arbitrary `u32` and a
/// `read_volatile` through a raw pointer, so a hole in it is not a wrong
/// picture, it is a stray access outside the aperture — into whichever
/// device's registers happen to be mapped next.
fn test_mmio_offsets(c: &mut Check) {
    /// A 16 KiB aperture, matching what [`super::mmio`] maps.
    const LEN: u64 = 16 * 1024;

    // Every register this driver names must be accepted.
    for (label, off) in [
        ("MM_INDEX", regs::MM_INDEX),
        ("MM_DATA", regs::MM_DATA),
        ("CNFG_MEMSIZE", regs::CNFG_MEMSIZE),
        ("CRTC_GEN_CNTL", regs::CRTC_GEN_CNTL),
        ("CRTC_H_TOTAL_DISP", regs::CRTC_H_TOTAL_DISP),
        ("CRTC_H_SYNC_STRT_WID", regs::CRTC_H_SYNC_STRT_WID),
        ("CRTC_V_TOTAL_DISP", regs::CRTC_V_TOTAL_DISP),
        ("CRTC_V_SYNC_STRT_WID", regs::CRTC_V_SYNC_STRT_WID),
        ("CRTC_OFFSET", regs::CRTC_OFFSET),
        ("CRTC_PITCH", regs::CRTC_PITCH),
    ] {
        match mmio::check_offset(off, LEN) {
            Ok(got) => c.eq_u32(label, u32::try_from(got).unwrap_or(u32::MAX), off),
            Err(_) => {
                c.fail();
                serial_println!(
                    "[ati]   FAIL: {} ({:#x}) rejected by check_offset",
                    label,
                    off
                );
            }
        }
    }

    // Misalignment is refused. x86 would not fault on these, which is exactly
    // why the check has to exist in software: a byte-offset read would be split
    // across two registers and quietly do something to both.
    c.is_err(
        "offset 1 is refused (misaligned)",
        mmio::check_offset(1, LEN),
        KernelError::InvalidArgument,
    );
    c.is_err(
        "offset 0x202 is refused (2-byte aligned, not 4)",
        mmio::check_offset(0x202, LEN),
        KernelError::InvalidArgument,
    );

    // The boundary. `LEN - 4` is the last whole `u32` in the aperture and must
    // be accepted; `LEN` is one past the end. `LEN - 2` is the case a naive
    // `offset < len` test lets through — in range itself, but naming four bytes
    // that are not — and it is also misaligned, so it is checked alongside an
    // aligned out-of-range offset that isolates the range check on its own.
    match mmio::check_offset(
        u32::try_from(LEN).unwrap_or(u32::MAX).saturating_sub(4),
        LEN,
    ) {
        Ok(_) => c.pass(),
        Err(_) => {
            c.fail();
            serial_println!("[ati]   FAIL: last aligned u32 in the aperture was rejected");
        }
    }
    c.is_err(
        "offset == len is refused",
        mmio::check_offset(u32::try_from(LEN).unwrap_or(u32::MAX), LEN),
        KernelError::InvalidArgument,
    );
    c.is_err(
        "offset len-2 is refused (u32 would straddle the end)",
        mmio::check_offset(
            u32::try_from(LEN).unwrap_or(u32::MAX).saturating_sub(2),
            LEN,
        ),
        KernelError::InvalidArgument,
    );
    c.is_err(
        "a far out-of-range aligned offset is refused",
        mmio::check_offset(0x8000_0000, LEN),
        KernelError::InvalidArgument,
    );

    // A zero-length aperture accepts nothing, including offset zero. This is
    // the state a failed mapping would leave behind, and it must not read.
    c.is_err(
        "offset 0 is refused against a zero-length aperture",
        mmio::check_offset(0, 0),
        KernelError::InvalidArgument,
    );
}

/// Mode-set planning: the decisions a mode-set makes before it writes anything.
///
/// Worth testing on every boot for the reason the plan/apply split exists at
/// all: these are the checks that stop a mode-set from being started and then
/// abandoned. A plan that wrongly succeeds leaves a CRTC retimed and pointed at
/// an address outside VRAM, which the hardware does not fault on — it wraps or
/// reads zero, and the only symptom is a wrong picture.
fn test_modeset_plan(c: &mut Check) {
    /// 16 MiB, what the emulated RV100 reports.
    const VRAM: u32 = 16 * 1024 * 1024;

    let Some(mode) = timing::lookup(640, 480, 60) else {
        c.fail();
        serial_println!("[ati]   FAIL: 640x480@60 missing from the DMT table");
        return;
    };

    // The stride is width * bpp with no padding, NOT PixelFormat::pitch, which
    // rounds up to 64 bytes. For 640x480 the two happen to agree (2560 is
    // already a multiple of 64), so the case that distinguishes them is checked
    // below with a width whose unpadded stride is not 64-aligned.
    if let Some(p) = c.expect_ok(
        "640x480@60 XRGB8888 plans",
        modeset::ModeSetPlan::new(mode, PixelFormat::Xrgb8888, 0, VRAM),
    ) {
        c.eq_u32("plan pitch_bytes is 640*4", p.pitch_bytes, 2560);
        c.eq_u32("plan size_bytes is 640*4*480", p.size_bytes, 2560 * 480);
        c.eq_u32("plan offset is what was asked for", p.offset, 0);
        // Pitch is denominated in 8-pixel characters: 640 px / 8 = 80.
        c.eq_u32("plan pitch encodes to 80 characters", p.pitch, 80);
        c.is_true(
            "plan carries the 32bpp encoding",
            matches!(p.pix_width, PixWidth::Bpp32),
        );

        // `gen_cntl` sets the depth and the enable bit while preserving bits
        // the driver does not model. 1 << 30 stands in for such a bit here:
        // if the implementation composed the register from scratch instead of
        // masking, this bit would come back clear.
        let composed = p.gen_cntl(1 << 30);
        c.is_true(
            "gen_cntl preserves unmodelled bits",
            composed & (1 << 30) != 0,
        );
        c.is_true("gen_cntl sets CRTC_EN", composed & regs::CRTC_EN != 0);
        c.eq_u32(
            "gen_cntl sets the pixel width field",
            composed & regs::CRTC_PIX_WIDTH_MASK,
            PixWidth::Bpp32.shifted(),
        );
        // A stale depth in the incoming value must be replaced, not OR-ed
        // together with the new one — which would leave a nonsense field.
        let over_stale = p.gen_cntl(PixWidth::Bpp8.shifted());
        c.eq_u32(
            "gen_cntl replaces a stale pixel width",
            over_stale & regs::CRTC_PIX_WIDTH_MASK,
            PixWidth::Bpp32.shifted(),
        );
        // Interlace is cleared: this driver only plans progressive modes, and
        // an interlace bit left set by firmware would halve the picture.
        c.is_true(
            "gen_cntl clears interlace",
            p.gen_cntl(regs::CRTC_INTERLACE_EN) & regs::CRTC_INTERLACE_EN == 0,
        );
    }

    // 1920x1080 at 32bpp is 8.3 MB, which fits in 16 MiB; the same mode does
    // not fit in the 4 MiB an older part might report. The second case is the
    // one that matters — it is the check that stops a scanout running off the
    // end of VRAM.
    if let Some(mode) = timing::lookup(1920, 1080, 60) {
        c.is_true(
            "1920x1080 XRGB8888 fits in 16 MiB",
            modeset::ModeSetPlan::new(mode, PixelFormat::Xrgb8888, 0, VRAM).is_ok(),
        );
        c.is_err(
            "1920x1080 XRGB8888 does not fit in 4 MiB",
            modeset::ModeSetPlan::new(mode, PixelFormat::Xrgb8888, 0, 4 * 1024 * 1024),
            KernelError::InvalidArgument,
        );
        // Fits by size, but not at that offset. Checking the offset is included
        // in the bound, rather than only the size.
        c.is_err(
            "a scanout that fits but is pushed past the end is refused",
            modeset::ModeSetPlan::new(mode, PixelFormat::Xrgb8888, 15 * 1024 * 1024, VRAM),
            KernelError::InvalidArgument,
        );
    }

    // Scanout base must be burst-aligned. A misaligned base is not refused by
    // the hardware, it shifts the whole image — which reads as a timing bug and
    // sends the search somewhere else entirely.
    c.is_err(
        "a misaligned scanout base is refused",
        modeset::ModeSetPlan::new(mode, PixelFormat::Xrgb8888, 128, VRAM),
        KernelError::InvalidArgument,
    );
    c.is_true(
        "an aligned scanout base is accepted",
        modeset::ModeSetPlan::new(mode, PixelFormat::Xrgb8888, modeset::SCANOUT_ALIGN, VRAM)
            .is_ok(),
    );

    // The format rejection propagates from PixWidth::from_format, and must keep
    // its own error rather than being flattened into InvalidArgument: "that
    // offset is wrong" and "this display block cannot scan out that format" ask
    // for different responses from a caller.
    c.is_err(
        "a channel-swapped format is refused by the planner",
        modeset::ModeSetPlan::new(mode, PixelFormat::Xbgr8888, 0, VRAM),
        KernelError::NotSupported,
    );

    // Zero VRAM accepts nothing. This is what a card whose CNFG_MEMSIZE read
    // failed would present, and it must not be planned against.
    c.is_err(
        "no VRAM means no plan",
        modeset::ModeSetPlan::new(mode, PixelFormat::Xrgb8888, 0, 0),
        KernelError::InvalidArgument,
    );
}

// ---------------------------------------------------------------------------
// VRAM suballocation
// ---------------------------------------------------------------------------

/// Assert an allocator's free list still satisfies its invariants.
///
/// Called after every mutation below rather than only at the end. An allocator
/// that returns correct offsets while its list quietly stops being sorted is
/// correct until it is catastrophically not, and the boundary between the two
/// is the operation that broke the invariant — which is only identifiable if
/// the invariants are checked at every step.
fn vram_intact(c: &mut Check, what: &str, a: &vram::VramAllocator) {
    if let Err(e) = a.check_invariants() {
        c.fail();
        serial_println!("[ati]   FAIL: free list broken after {}: {:?}", what, e);
    } else {
        c.pass();
    }
}

/// [`vram::VramAllocator`]: allocation, alignment, freeing, and coalescing.
#[allow(clippy::too_many_lines)]
fn test_vram(c: &mut Check) {
    // 16 MiB, which is what QEMU's ati-vga reports by default and what the
    // probe reads out of CNFG_MEMSIZE on the boot test.
    const TOTAL: u32 = 16 * 1024 * 1024;

    let Some(mut a) = c.expect_ok(
        "VramAllocator::new(16 MiB)",
        vram::VramAllocator::new(TOTAL),
    ) else {
        return;
    };
    c.eq_u32("fresh allocator is entirely free", a.free_bytes(), TOTAL);
    c.eq_u32("fresh allocator is one run", a.largest_free(), TOTAL);
    vram_intact(c, "new", &a);

    // A card that reports no memory is not an allocator with nothing free; it
    // is a card that was never configured, and must be refused up front.
    c.is_err(
        "zero-sized VRAM refused",
        vram::VramAllocator::new(0),
        KernelError::InvalidArgument,
    );

    // First allocation comes from offset 0 — first fit over a single free run.
    let fb0 = c.expect_ok("alloc 640x480 scanout", a.alloc(1_228_800, 256));
    c.eq_u32("first allocation starts at 0", fb0.unwrap_or(u32::MAX), 0);
    c.eq_u32(
        "free bytes drop by exactly the request",
        a.free_bytes(),
        TOTAL.saturating_sub(1_228_800),
    );
    vram_intact(c, "first alloc", &a);

    // A second allocation with a coarser alignment must skip forward to the
    // aligned offset, and the skipped bytes must stay free rather than being
    // consumed. 1_228_800 is 16 KiB-aligned already (it is 75 * 16384), so pick
    // a size that is not, to make the padding real.
    let odd = c.expect_ok("alloc an unaligned-length buffer", a.alloc(1000, 4));
    c.eq_u32(
        "second allocation follows the first",
        odd.unwrap_or(u32::MAX),
        1_228_800,
    );
    vram_intact(c, "unaligned-length alloc", &a);

    let aligned = c.expect_ok("alloc at 16 KiB alignment", a.alloc(4096, 16384));
    // 1_228_800 + 1000 = 1_229_800; rounded up to 16384 gives 1_245_184.
    c.eq_u32(
        "alignment padding is skipped, not silently absorbed",
        aligned.unwrap_or(u32::MAX),
        1_245_184,
    );
    // The padding must have been returned to the free list as its own run:
    // 1_229_800 .. 1_245_184 is 15_384 bytes that are still allocatable.
    c.is_true(
        "alignment padding stayed free",
        a.free_bytes()
            == TOTAL
                .saturating_sub(1_228_800)
                .saturating_sub(1000)
                .saturating_sub(4096),
    );
    c.is_true("free list split into two runs", a.region_count() == 2);
    vram_intact(c, "aligned alloc", &a);

    // And it must actually be usable, not merely counted. 15_384 bytes are
    // free at 1_229_800; a 4-byte-aligned request for 15_384 fits exactly.
    let hole = c.expect_ok("alloc exactly fills the padding hole", a.alloc(15_384, 8));
    c.eq_u32(
        "padding hole reused at its own offset",
        hole.unwrap_or(u32::MAX),
        1_229_800,
    );
    c.is_true("filling the hole rejoins the list", a.region_count() == 1);
    vram_intact(c, "hole refill", &a);

    // Freeing the middle of three adjacent allocations must not merge with
    // anything — the neighbours are still live.
    let before = a.region_count();
    c.expect_ok("free the middle allocation", a.free(1_228_800, 1000));
    c.is_true(
        "freeing an isolated run adds a list entry",
        a.region_count() == before.saturating_add(1),
    );
    vram_intact(c, "isolated free", &a);

    // Freeing it again must be refused. This is the check the allocator exists
    // to make: a double free that succeeded would hand the same VRAM to two
    // scanout buffers.
    c.is_err(
        "double free refused",
        a.free(1_228_800, 1000),
        KernelError::AlreadyExists,
    );
    // As must a free of a range that merely overlaps a free one.
    c.is_err(
        "overlapping free refused",
        a.free(1_228_000, 2000),
        KernelError::AlreadyExists,
    );
    // ... and one that runs off the end of VRAM.
    c.is_err(
        "free past the end of VRAM refused",
        a.free(TOTAL.saturating_sub(16), 32),
        KernelError::InvalidArgument,
    );
    c.is_err(
        "zero-length free refused",
        a.free(0, 0),
        KernelError::InvalidArgument,
    );
    vram_intact(c, "rejected frees", &a);

    // Freeing the two neighbours must coalesce all three runs — plus the large
    // tail — back into one, which is the invariant that stops a mode change
    // from fragmenting VRAM into unusable slivers.
    c.expect_ok("free the first allocation", a.free(0, 1_228_800));
    vram_intact(c, "free neighbour 1", &a);
    c.expect_ok(
        "free the padding-hole allocation",
        a.free(1_229_800, 15_384),
    );
    vram_intact(c, "free neighbour 2", &a);
    c.expect_ok("free the aligned allocation", a.free(1_245_184, 4096));
    vram_intact(c, "free neighbour 3", &a);
    c.eq_u32(
        "everything freed means everything free",
        a.free_bytes(),
        TOTAL,
    );
    c.is_true(
        "everything freed coalesces to a single run",
        a.region_count() == 1,
    );
    c.eq_u32("and that run is the whole card", a.largest_free(), TOTAL);

    // Exhaustion. A request one byte larger than the card is refused as out of
    // memory, not as a bad argument: the caller asked for something coherent
    // and this card cannot provide it.
    c.is_err(
        "request larger than VRAM is OutOfMemory",
        a.alloc(TOTAL.saturating_add(1), 1),
        KernelError::OutOfMemory,
    );
    // A request for exactly the whole card succeeds and leaves nothing.
    let whole = c.expect_ok("alloc the entire card", a.alloc(TOTAL, 256));
    c.eq_u32(
        "whole-card allocation is at 0",
        whole.unwrap_or(u32::MAX),
        0,
    );
    c.eq_u32("nothing left", a.free_bytes(), 0);
    c.is_true("empty free list", a.region_count() == 0);
    vram_intact(c, "full allocation", &a);
    c.is_err(
        "allocation from an empty card refused",
        a.alloc(1, 1),
        KernelError::OutOfMemory,
    );
    c.expect_ok("free the whole card back", a.free(0, TOTAL));
    c.eq_u32("card fully restored", a.free_bytes(), TOTAL);
    vram_intact(c, "full free", &a);

    // Argument validation.
    c.is_err(
        "zero-length allocation refused",
        a.alloc(0, 256),
        KernelError::InvalidArgument,
    );
    c.is_err(
        "zero alignment refused",
        a.alloc(16, 0),
        KernelError::InvalidArgument,
    );
    c.is_err(
        "non-power-of-two alignment refused",
        a.alloc(16, 96),
        KernelError::InvalidArgument,
    );

    // An alignment larger than the card cannot be satisfied, and must report
    // that rather than overflowing to a small offset that appears to fit.
    c.is_err(
        "alignment beyond the card is OutOfMemory",
        a.alloc(16, 0x8000_0000),
        KernelError::OutOfMemory,
    );
    vram_intact(c, "rejected allocations", &a);

    // The scanout alignment the mode-set planner requires must be satisfiable,
    // since every framebuffer this driver displays has to meet it.
    let scanout = c.expect_ok(
        "alloc at SCANOUT_ALIGN",
        a.alloc(1024, modeset::SCANOUT_ALIGN),
    );
    c.is_true(
        "SCANOUT_ALIGN allocation is scanout-aligned",
        scanout.unwrap_or(1).is_multiple_of(modeset::SCANOUT_ALIGN),
    );
    vram_intact(c, "scanout-aligned alloc", &a);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run every check, reporting a tally.
///
/// # Errors
///
/// `InternalError` if any check failed. The count is reported first so a
/// failing boot names every broken constant, not just the first.
pub fn run() -> KernelResult<()> {
    let mut c = Check::new();

    test_dmt_table(&mut c);
    test_dmt_registers(&mut c);
    test_encoding_quirks(&mut c);
    test_rejections(&mut c);
    test_pix_width(&mut c);
    test_pitch(&mut c);
    test_refresh(&mut c);
    test_mode_lookup(&mut c);
    test_identify(&mut c);
    test_mmio_offsets(&mut c);
    test_modeset_plan(&mut c);
    test_vram(&mut c);

    serial_println!("[ati] Self-test: {} passed, {} failed.", c.passed, c.failed);
    if c.failed > 0 {
        return Err(KernelError::InternalError);
    }
    Ok(())
}

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

use super::regs::{self, CrtcTiming, PixWidth};
use super::timing::{self, ModeTiming, DMT_MODES};
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
                serial_println!("[ati]   FAIL: {}: wrong error {:?}, want {:?}", what, e, want);
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
        c.is_true("DMT width matches timing hdisplay", m.width == m.timing.hdisplay);
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
        c.eq_u32("h_disp is biased by one", (t.h_total_disp >> 16) & 0x1FF, 79);
        c.eq_u32("v_total is biased by one", t.v_total_disp & 0xFFF, 524);
        c.eq_u32("v_disp is biased by one", (t.v_total_disp >> 16) & 0xFFF, 479);
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
    c.is_err("hdisplay not a multiple of 8", CrtcTiming::from_mode(&m), bad);

    // Edge ordering: display <= sync_start < sync_end <= total, both axes.
    let mut m = base_mode();
    m.hdisplay = 664; // past hsync_start
    c.is_err("hdisplay past hsync_start", CrtcTiming::from_mode(&m), bad);

    let mut m = base_mode();
    m.hsync_end = 656; // equal to start: zero-width sync
    c.is_err("hsync_end equals hsync_start", CrtcTiming::from_mode(&m), bad);

    let mut m = base_mode();
    m.hsync_end = 808; // past htotal
    c.is_err("hsync_end past htotal", CrtcTiming::from_mode(&m), bad);

    let mut m = base_mode();
    m.vdisplay = 496; // past vsync_start
    c.is_err("vdisplay past vsync_start", CrtcTiming::from_mode(&m), bad);

    let mut m = base_mode();
    m.vsync_end = 490; // equal to start
    c.is_err("vsync_end equals vsync_start", CrtcTiming::from_mode(&m), bad);

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
    c.is_err("hdisplay overflows its field", CrtcTiming::from_mode(&m), bad);

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
    c.is_err("vsync width overflows its field", CrtcTiming::from_mode(&m), bad);

    // A sync pulse wider than 63 characters (504 pixels) is not a rounding
    // problem, so unlike the 44-pixel case it is refused rather than clamped.
    let mut m = base_mode();
    m.hsync_start = 648;
    m.hsync_end = 1168; // 520 pixels = 65 characters
    m.htotal = 1176;
    c.is_err("hsync width overflows its field", CrtcTiming::from_mode(&m), bad);
}

/// Pixel-format mapping, including the formats that must be refused.
fn test_pix_width(c: &mut Check) {
    c.is_true(
        "Xrgb8888 maps to 32bpp",
        matches!(PixWidth::from_format(PixelFormat::Xrgb8888), Ok(PixWidth::Bpp32)),
    );
    c.is_true(
        "Argb8888 maps to 32bpp",
        matches!(PixWidth::from_format(PixelFormat::Argb8888), Ok(PixWidth::Bpp32)),
    );
    c.is_true(
        "Rgb565 maps to 16bpp",
        matches!(PixWidth::from_format(PixelFormat::Rgb565), Ok(PixWidth::Bpp16)),
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
    c.is_err("pitch not a whole pixel count", regs::encode_pitch(7681, 4), bad);
    c.is_err("pitch not a whole character count", regs::encode_pitch(16, 4), bad);
    // 16384 pixels = 2048 characters, past the 11-bit field.
    c.is_err("pitch overflows its field", regs::encode_pitch(65_536, 4), bad);
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
    c.is_true("degenerate mode has no refresh rate", m.refresh_centihz().is_none());
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
    c.is_true("lookup finds 1024x768@60", timing::lookup(1024, 768, 60).is_some());
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
        Some(info) => c.is_true("Rage 128 Pro identifies", info.family == super::AsicFamily::Rage128),
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

    serial_println!("[ati] Self-test: {} passed, {} failed.", c.passed, c.failed);
    if c.failed > 0 {
        return Err(KernelError::InternalError);
    }
    Ok(())
}

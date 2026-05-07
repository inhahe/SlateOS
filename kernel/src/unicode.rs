//! Unicode support for the framebuffer console.
//!
//! Provides:
//! - UTF-8 byte-sequence decoding
//! - Unicode character width classification (narrow vs. wide)
//! - Extended glyph generation for non-ASCII codepoints:
//!   - Box Drawing (U+2500–U+257F): procedurally generated from segment descriptors
//!   - Block Elements (U+2580–U+259F): procedurally generated fill patterns
//!   - Common symbols (arrows, bullets, check marks, etc.): hand-coded bitmaps
//!
//! The console calls [`glyph_for_codepoint`] when a complete UTF-8 sequence
//! decodes to a codepoint outside the ASCII/Latin-1 font range.  The function
//! returns an 8×16 bitmap and a width flag (single or double cell).

// ---------------------------------------------------------------------------
// UTF-8 decoding
// ---------------------------------------------------------------------------

/// Return the expected total byte length of a UTF-8 sequence given
/// its lead byte.  Returns 0 for invalid lead bytes (continuation
/// bytes 0x80–0xBF and overlong indicators 0xC0–0xC1).
#[must_use]
pub const fn utf8_seq_len(lead: u8) -> u8 {
    match lead {
        0x00..=0x7F => 1,
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 0, // continuation byte, overlong, or out-of-range lead
    }
}

/// Decode a complete UTF-8 sequence from `buf[0..len]` into a Unicode
/// codepoint.  Returns U+FFFD (REPLACEMENT CHARACTER) for invalid or
/// overlong sequences.
///
/// `len` must be 1–4 and all continuation bytes must have been validated
/// by the caller (the console's accumulator rejects non-0x80 continuations).
#[must_use]
pub fn decode_utf8(buf: [u8; 4], len: u8) -> u32 {
    let cp = match len {
        1 => u32::from(buf[0]),
        2 => {
            let b0 = u32::from(buf[0] & 0x1F);
            let b1 = u32::from(buf[1] & 0x3F);
            (b0 << 6) | b1
        }
        3 => {
            let b0 = u32::from(buf[0] & 0x0F);
            let b1 = u32::from(buf[1] & 0x3F);
            let b2 = u32::from(buf[2] & 0x3F);
            (b0 << 12) | (b1 << 6) | b2
        }
        4 => {
            let b0 = u32::from(buf[0] & 0x07);
            let b1 = u32::from(buf[1] & 0x3F);
            let b2 = u32::from(buf[2] & 0x3F);
            let b3 = u32::from(buf[3] & 0x3F);
            (b0 << 18) | (b1 << 12) | (b2 << 6) | b3
        }
        _ => return 0xFFFD,
    };

    // Reject overlong encodings and surrogates.
    match len {
        2 if cp < 0x80 => 0xFFFD,
        3 if cp < 0x800 => 0xFFFD,
        4 if cp < 0x10000 => 0xFFFD,
        _ if (0xD800..=0xDFFF).contains(&cp) => 0xFFFD,
        _ if cp > 0x0010_FFFF => 0xFFFD,
        _ => cp,
    }
}

// ---------------------------------------------------------------------------
// Character width
// ---------------------------------------------------------------------------

/// Return the display width of a codepoint (1 or 2 cells).
///
/// Wide characters (CJK ideographs, fullwidth forms, etc.) occupy 2 cells.
/// Control characters and zero-width codepoints return 0, but the console
/// should handle those before calling this.
#[must_use]
pub fn char_width(cp: u32) -> u8 {
    if is_wide(cp) { 2 } else { 1 }
}

/// Return `true` if the codepoint is a wide (double-cell) character.
///
/// Based on Unicode East Asian Width property (simplified).  Covers the
/// most important ranges: CJK Unified Ideographs, Hangul, fullwidth forms,
/// and related blocks.
#[must_use]
pub fn is_wide(cp: u32) -> bool {
    matches!(cp,
        // Hangul Jamo
        0x1100..=0x115F |
        // Hangul compatibility Jamo
        0x2329..=0x232A |
        // CJK Radicals Supplement .. Enclosed CJK Letters
        0x2E80..=0x303E |
        // Hiragana .. Katakana
        0x3041..=0x33BF |
        // CJK Compatibility (to end of block)
        0x3400..=0x4DBF |
        // CJK Unified Ideographs
        0x4E00..=0x9FFF |
        // Yi Syllables .. Yi Radicals
        0xA000..=0xA4CF |
        // Hangul Syllables
        0xAC00..=0xD7AF |
        // CJK Compatibility Ideographs
        0xF900..=0xFAFF |
        // Vertical Forms
        0xFE10..=0xFE19 |
        // CJK Compatibility Forms
        0xFE30..=0xFE6F |
        // Fullwidth Forms
        0xFF01..=0xFF60 |
        0xFFE0..=0xFFE6 |
        // CJK Unified Ideographs Extension B+
        0x1F300..=0x1F9FF |
        0x20000..=0x2FFFF |
        0x30000..=0x3FFFF
    )
}

// ---------------------------------------------------------------------------
// Glyph lookup for Unicode codepoints
// ---------------------------------------------------------------------------

/// Replacement character glyph (U+FFFD).
///
/// A filled diamond-like shape that's clearly not a normal character.
static REPLACEMENT_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x18, // ···##···
    0x3C, 0x7E, 0xFF, 0xFF, // ·####·  ######  ########  ########
    0xFF, 0x7E, 0x3C, 0x18, // ########  ######  ·####·  ···##···
    0x00, 0x00, 0x00, 0x00,
];

/// Look up a glyph bitmap for a Unicode codepoint.
///
/// Returns `(bitmap, is_wide)`.  For characters outside the supported
/// ranges, returns the replacement character glyph.
#[must_use]
pub fn glyph_for_codepoint(cp: u32) -> ([u8; 16], bool) {
    #[allow(clippy::arithmetic_side_effects)]
    match cp {
        // ASCII range → use font module directly.
        0x00..=0x7F => {
            #[allow(clippy::cast_possible_truncation)]
            let g = *crate::font::glyph(cp as u8);
            (g, false)
        }
        // Specific Latin-1 characters with custom glyphs (before the
        // 0x80..=0xFF catch-all).
        0x00B7 => (MIDDLE_DOT_GLYPH, false),       // ·
        0x00AB => (GUILLEMET_LEFT_GLYPH, false),   // «
        0x00BB => (GUILLEMET_RIGHT_GLYPH, false),  // »
        // Latin-1 Supplement catch-all — no glyphs for these yet.
        // Combined with FFFD to avoid identical-body clippy lint.
        0x80..=0xFF | 0xFFFD => (REPLACEMENT_GLYPH, false),
        // Miscellaneous symbols (before geometric range catch-all).
        0x2013 => (EN_DASH_GLYPH, false),          // –
        0x2014 => (EM_DASH_GLYPH, false),          // —
        0x2018 | 0x2019 => (SINGLE_QUOTE_GLYPH, false), // '' (smart quotes → ')
        0x201C | 0x201D => (DOUBLE_QUOTE_GLYPH, false), // "" (smart quotes → ")
        0x2022 => (BULLET_GLYPH, false),          // •
        0x2023 => (TRIANGLE_RIGHT_GLYPH, false),   // ‣
        0x2026 => (ELLIPSIS_GLYPH, false),         // …
        // Arrows
        0x2190..=0x2199 => {
            #[allow(clippy::cast_possible_truncation)]
            let idx = (cp - 0x2190) as usize;
            match ARROW_GLYPHS.get(idx) {
                Some(g) => (**g, false),
                None => (REPLACEMENT_GLYPH, false),
            }
        }
        // Box Drawing (U+2500–U+257F)
        0x2500..=0x257F => {
            #[allow(clippy::cast_possible_truncation)]
            let idx = (cp - 0x2500) as u8;
            (box_drawing_glyph(idx), false)
        }
        // Block Elements (U+2580–U+259F)
        0x2580..=0x259F => {
            #[allow(clippy::cast_possible_truncation)]
            let idx = (cp - 0x2580) as u8;
            (block_element_glyph(idx), false)
        }
        // Geometric Shapes subset (specific symbols before range).
        0x25CB => (EMPTY_CIRCLE_GLYPH, false),     // ○
        0x25CF => (FILLED_CIRCLE_GLYPH, false),    // ●
        0x25A0..=0x25FF => (geometric_glyph(cp), false),
        // Stars, checks, crosses
        0x2605 => (FILLED_STAR_GLYPH, false),      // ★
        0x2606 => (EMPTY_STAR_GLYPH, false),       // ☆
        0x2713 | 0x2714 => (CHECK_GLYPH, false),   // ✓ ✔
        0x2717 | 0x2718 => (CROSS_GLYPH, false),    // ✗ ✘
        _ => {
            let wide = is_wide(cp);
            (REPLACEMENT_GLYPH, wide)
        }
    }
}

// ---------------------------------------------------------------------------
// Box Drawing (U+2500–U+257F) — procedural generation
// ---------------------------------------------------------------------------
//
// Each box drawing character is described by four segment weights:
//   left, right, up, down — each is 0 (none), 1 (light), 2 (heavy), 3 (double)
//
// The bitmap is generated by drawing line segments from the center
// of the 8×16 cell to the edges.
//
// Center point: column 4, row 8 (0-indexed).
// Light:  1 pixel  (row 8 for horiz, col 4 for vert)
// Heavy:  3 pixels (rows 7-9 for horiz, cols 3-5 for vert)
// Double: 2 pixels with gap (rows 6,10 for horiz, cols 2,6 for vert)
//
// Packed descriptor: one byte per character.
//   bits 7-6: left weight   (0-3)
//   bits 5-4: right weight  (0-3)
//   bits 3-2: up weight     (0-3)
//   bits 1-0: down weight   (0-3)

/// Descriptor table for box drawing characters U+2500 through U+257F.
///
/// Index = codepoint - 0x2500.  Value = packed segment descriptor.
/// See encoding above.  0x00 means "not a standard box drawing
/// character" and falls back to a custom handler or replacement.
#[allow(clippy::unreadable_literal)]
static BOX_DESC: [u8; 128] = [
    // U+2500 ─   U+2501 ━   U+2502 │   U+2503 ┃
    0x50, 0xA0, 0x05, 0x0A,
    // U+2504 ┄   U+2505 ┅   U+2506 ┆   U+2507 ┇  (dashed — special)
    0xF1, 0xF2, 0xF3, 0xF4,
    // U+2508 ┈   U+2509 ┉   U+250A ┊   U+250B ┋  (dashed — special)
    0xF5, 0xF6, 0xF7, 0xF8,
    // U+250C ┌   U+250D ┍   U+250E ┎   U+250F ┏
    0x11, 0x21, 0x12, 0x22,
    // U+2510 ┐   U+2511 ┑   U+2512 ┒   U+2513 ┓
    0x41, 0x81, 0x42, 0x82,
    // U+2514 └   U+2515 ┕   U+2516 ┖   U+2517 ┗
    0x14, 0x24, 0x18, 0x28,
    // U+2518 ┘   U+2519 ┙   U+251A ┚   U+251B ┛
    0x44, 0x84, 0x48, 0x88,
    // U+251C ├   U+251D ┝   U+251E ┞   U+251F ┟
    0x15, 0x25, 0x19, 0x16,
    // U+2520 ┠   U+2521 ┡   U+2522 ┢   U+2523 ┣
    0x1A, 0x29, 0x26, 0x2A,
    // U+2524 ┤   U+2525 ┥   U+2526 ┦   U+2527 ┧
    0x45, 0x85, 0x49, 0x46,
    // U+2528 ┨   U+2529 ┩   U+252A ┪   U+252B ┫
    0x4A, 0x89, 0x86, 0x8A,
    // U+252C ┬   U+252D ┭   U+252E ┮   U+252F ┯
    0x51, 0x91, 0x61, 0xA1,
    // U+2530 ┰   U+2531 ┱   U+2532 ┲   U+2533 ┳
    0x52, 0x92, 0x62, 0xA2,
    // U+2534 ┴   U+2535 ┵   U+2536 ┶   U+2537 ┷
    0x54, 0x94, 0x64, 0xA4,
    // U+2538 ┸   U+2539 ┹   U+253A ┺   U+253B ┻
    0x58, 0x98, 0x68, 0xA8,
    // U+253C ┼   U+253D ┽   U+253E ┾   U+253F ┿
    0x55, 0x95, 0x65, 0xA5,
    // U+2540 ╀   U+2541 ╁   U+2542 ╂   U+2543 ╃
    0x59, 0x56, 0x5A, 0x99,
    // U+2544 ╄   U+2545 ╅   U+2546 ╆   U+2547 ╇
    0x69, 0x96, 0x66, 0xA9,
    // U+2548 ╈   U+2549 ╉   U+254A ╊   U+254B ╋
    0xA6, 0x9A, 0x6A, 0xAA,
    // U+254C ╌   U+254D ╍   U+254E ╎   U+254F ╏  (dashed — special)
    0xF9, 0xFA, 0xFB, 0xFC,
    // U+2550 ═   U+2551 ║   U+2552 ╒   U+2553 ╓
    0xD0, 0x0D, 0x31, 0x13,
    // U+2554 ╔   U+2555 ╕   U+2556 ╖   U+2557 ╗
    0x33, 0xC1, 0x43, 0xC3,
    // U+2558 ╘   U+2559 ╙   U+255A ╚   U+255B ╛
    0x34, 0x1C, 0x3C, 0xC4,
    // U+255C ╜   U+255D ╝   U+255E ╞   U+255F ╟
    0x4C, 0xCC, 0x35, 0x1D,
    // U+2560 ╠   U+2561 ╡   U+2562 ╢   U+2563 ╣
    0x3D, 0xC5, 0x4D, 0xCD,
    // U+2564 ╤   U+2565 ╥   U+2566 ╦   U+2567 ╧
    0xD1, 0x53, 0xD3, 0xD4,
    // U+2568 ╨   U+2569 ╩   U+256A ╪   U+256B ╫
    0x5C, 0xDC, 0xD5, 0x5D,
    // U+256C ╬   U+256D ╭   U+256E ╮   U+256F ╯
    0xDD, 0xFD, 0xFD, 0xFD,
    // U+2570 ╰   U+2571 ╱   U+2572 ╲   U+2573 ╳
    0xFD, 0xFE, 0xFE, 0xFE,
    // U+2574 ╴   U+2575 ╵   U+2576 ╶   U+2577 ╷
    0x40, 0x04, 0x10, 0x01,
    // U+2578 ╸   U+2579 ╹   U+257A ╺   U+257B ╻
    0x80, 0x08, 0x20, 0x02,
    // U+257C ╼   U+257D ╽   U+257E ╾   U+257F ╿
    0x60, 0x06, 0x90, 0x09,
];

// Weights extracted from the packed descriptor byte.
const fn desc_left(d: u8) -> u8 { (d >> 6) & 3 }
const fn desc_right(d: u8) -> u8 { (d >> 4) & 3 }
const fn desc_up(d: u8) -> u8 { (d >> 2) & 3 }
const fn desc_down(d: u8) -> u8 { d & 3 }

/// Horizontal center row for box drawing (0-indexed within 16-row glyph).
const CY: usize = 8;
/// Vertical center column bit position (0-indexed, 0=MSB=leftmost).
const CX: u8 = 4;

/// Generate an 8×16 bitmap for a box drawing character.
///
/// `idx` = codepoint − 0x2500 (0..128).
fn box_drawing_glyph(idx: u8) -> [u8; 16] {
    let Some(&desc) = BOX_DESC.get(idx as usize) else {
        return REPLACEMENT_GLYPH;
    };

    // Special-case markers in the descriptor table.
    // 0xF1–0xFC: dashed lines
    // 0xFD: rounded corners (╭╮╯╰) — render as normal corners
    // 0xFE: diagonal lines (╱╲╳) — render a simple X or slash
    if desc >= 0xF0 {
        return special_box_glyph(idx, desc);
    }

    let left = desc_left(desc);
    let right = desc_right(desc);
    let up = desc_up(desc);
    let down = desc_down(desc);

    let mut bmp = [0u8; 16];

    // Draw horizontal segments.
    draw_horiz_segment(&mut bmp, left, true);   // left half
    draw_horiz_segment(&mut bmp, right, false); // right half

    // Draw vertical segments.
    draw_vert_segment(&mut bmp, up, true);   // upper half
    draw_vert_segment(&mut bmp, down, false); // lower half

    bmp
}

/// Pre-computed horizontal masks for box drawing.
///
/// Left half (columns 0–CX inclusive): bits 7..=(7-CX).
/// CX=4 → bits 7,6,5,4,3 → 0xF8.
const HORIZ_MASK_LEFT: u8 = 0xF8;

/// Right half (columns CX..7 inclusive): bits (7-CX)..=0.
/// CX=4 → bits 3,2,1,0 → 0x0F.  But include CX → 0x1F.
const HORIZ_MASK_RIGHT: u8 = 0x0F;

/// Draw a horizontal segment (left half or right half).
///
/// `weight`: 0=none, 1=light, 2=heavy, 3=double.
/// `is_left`: true → draw from left edge to center (inclusive);
///            false → draw from center to right edge.
fn draw_horiz_segment(bmp: &mut [u8; 16], weight: u8, is_left: bool) {
    if weight == 0 {
        return;
    }

    let mask = if is_left { HORIZ_MASK_LEFT } else { HORIZ_MASK_RIGHT };

    match weight {
        1 => {
            // Light: single row at CY.
            bmp[CY] |= mask;
        }
        2 => {
            // Heavy: 3 rows centered on CY.
            if let Some(r) = bmp.get_mut(CY.wrapping_sub(1)) { *r |= mask; }
            bmp[CY] |= mask;
            if let Some(r) = bmp.get_mut(CY.wrapping_add(1)) { *r |= mask; }
        }
        3 => {
            // Double: 2 rows with a gap.
            if let Some(r) = bmp.get_mut(CY.wrapping_sub(2)) { *r |= mask; }
            if let Some(r) = bmp.get_mut(CY.wrapping_add(2)) { *r |= mask; }
        }
        _ => {}
    }
}

/// Bit mask for a single-pixel vertical line at column CX.
/// CX=4 → bit (7-4) = bit 3 → 0x08.
const VERT_BIT: u8 = 1 << (7 - CX);

/// Bit mask for a heavy (3-pixel) vertical line centered on CX.
/// CX=4 → columns 3,4,5 → bits 4,3,2 → 0x1C.
const VERT_HEAVY_MASK: u8 = (1 << (7 - CX + 1)) | (1 << (7 - CX)) | (1 << (7 - CX - 1));

/// Bit mask for a double vertical line (2 pixels with gap around CX).
/// CX=4 → columns 2,6 → bits 5,1 → 0x22.
const VERT_DOUBLE_MASK: u8 = (1 << (7 - CX + 2)) | (1 << (7 - CX - 2));

/// Draw a vertical segment (upper half or lower half).
///
/// `weight`: 0=none, 1=light, 2=heavy, 3=double.
/// `is_up`: true → draw from top edge to center (inclusive);
///          false → draw from center to bottom edge.
fn draw_vert_segment(bmp: &mut [u8; 16], weight: u8, is_up: bool) {
    if weight == 0 {
        return;
    }

    let (y_start, y_end) = if is_up {
        (0, CY)  // inclusive of CY
    } else {
        (CY, 15) // inclusive of CY and bottom
    };

    let bits = match weight {
        1 => VERT_BIT,
        2 => VERT_HEAVY_MASK,
        3 => VERT_DOUBLE_MASK,
        _ => return,
    };

    for row in bmp.iter_mut().skip(y_start).take(y_end.wrapping_sub(y_start).wrapping_add(1)) {
        *row |= bits;
    }
}

/// Handle special box drawing characters (dashed lines, rounded corners,
/// diagonal lines).
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::needless_range_loop)]
fn special_box_glyph(idx: u8, desc: u8) -> [u8; 16] {
    let mut bmp = [0u8; 16];
    match desc {
        // Dashed horizontal lines (light/heavy × 3-dash/4-dash/2-dash).
        0xF1 | 0xF5 | 0xF9 => {
            // Light dashed horizontal.
            bmp[CY] = 0b1100_1100;
        }
        0xF2 | 0xF6 | 0xFA => {
            // Heavy dashed horizontal.
            if let Some(r) = bmp.get_mut(CY.wrapping_sub(1)) { *r = 0b1100_1100; }
            bmp[CY] = 0b1100_1100;
            if let Some(r) = bmp.get_mut(CY.wrapping_add(1)) { *r = 0b1100_1100; }
        }
        0xF3 | 0xF7 | 0xFB => {
            // Light dashed vertical.
            for y in (0..16usize).step_by(3) {
                bmp[y] |= VERT_BIT;
                if let Some(row) = bmp.get_mut(y.wrapping_add(1)) {
                    *row |= VERT_BIT;
                }
            }
        }
        0xF4 | 0xF8 | 0xFC => {
            // Heavy dashed vertical.
            for y in (0..16usize).step_by(3) {
                bmp[y] |= VERT_HEAVY_MASK;
                if let Some(row) = bmp.get_mut(y.wrapping_add(1)) {
                    *row |= VERT_HEAVY_MASK;
                }
            }
        }
        // Rounded corners (╭╮╯╰) — render as normal light corners.
        0xFD => {
            // Determine which corner based on idx.
            let corner_desc = match idx {
                0x6D => 0x11u8, // ╭ = ┌ (down+right)
                0x6E => 0x41u8, // ╮ = ┐ (down+left)
                0x6F => 0x44u8, // ╯ = ┘ (up+left)
                0x70 => 0x14u8, // ╰ = └ (up+right)
                _ => return bmp,
            };
            let left = desc_left(corner_desc);
            let right = desc_right(corner_desc);
            let up = desc_up(corner_desc);
            let down = desc_down(corner_desc);
            draw_horiz_segment(&mut bmp, left, true);
            draw_horiz_segment(&mut bmp, right, false);
            draw_vert_segment(&mut bmp, up, true);
            draw_vert_segment(&mut bmp, down, false);
        }
        // Diagonal lines (╱╲╳).
        0xFE => {
            match idx {
                0x71 => {
                    // ╱ — forward slash (bottom-left to top-right).
                    for y in 0..16usize {
                        let x = 7usize.saturating_sub(y / 2);
                        if x < 8 { bmp[y] |= 1 << (7 - x); }
                    }
                }
                0x72 => {
                    // ╲ — backslash (top-left to bottom-right).
                    for y in 0..16usize {
                        let x = y / 2;
                        if x < 8 { bmp[y] |= 1 << (7 - x); }
                    }
                }
                0x73 => {
                    // ╳ — X (both diagonals).
                    for y in 0..16usize {
                        let x1 = 7usize.saturating_sub(y / 2);
                        let x2 = y / 2;
                        if x1 < 8 { bmp[y] |= 1 << (7 - x1); }
                        if x2 < 8 { bmp[y] |= 1 << (7 - x2); }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    bmp
}

// ---------------------------------------------------------------------------
// Block Elements (U+2580–U+259F) — procedural generation
// ---------------------------------------------------------------------------

/// Generate an 8×16 bitmap for a block element character.
///
/// `idx` = codepoint − 0x2580 (0..32).
#[allow(clippy::cast_possible_truncation, clippy::too_many_lines)]
fn block_element_glyph(idx: u8) -> [u8; 16] {
    let mut bmp = [0u8; 16];
    match idx {
        // U+2580 ▀ Upper half block
        0x00 => {
            for row in bmp.iter_mut().take(8) { *row = 0xFF; }
        }
        // U+2581 ▁ Lower one eighth block
        0x01 => {
            for row in bmp.iter_mut().skip(14) { *row = 0xFF; }
        }
        // U+2582 ▂ Lower one quarter block
        0x02 => {
            for row in bmp.iter_mut().skip(12) { *row = 0xFF; }
        }
        // U+2583 ▃ Lower three eighths block
        0x03 => {
            for row in bmp.iter_mut().skip(10) { *row = 0xFF; }
        }
        // U+2584 ▄ Lower half block
        0x04 => {
            for row in bmp.iter_mut().skip(8) { *row = 0xFF; }
        }
        // U+2585 ▅ Lower five eighths block
        0x05 => {
            for row in bmp.iter_mut().skip(6) { *row = 0xFF; }
        }
        // U+2586 ▆ Lower three quarters block
        0x06 => {
            for row in bmp.iter_mut().skip(4) { *row = 0xFF; }
        }
        // U+2587 ▇ Lower seven eighths block
        0x07 => {
            for row in bmp.iter_mut().skip(2) { *row = 0xFF; }
        }
        // U+2588 █ Full block
        0x08 => {
            bmp.fill(0xFF);
        }
        // U+2589 ▉ Left seven eighths block
        0x09 => {
            bmp.fill(0xFE);
        }
        // U+258A ▊ Left three quarters block
        0x0A => {
            bmp.fill(0xFC);
        }
        // U+258B ▋ Left five eighths block
        0x0B => {
            bmp.fill(0xF8);
        }
        // U+258C ▌ Left half block
        0x0C => {
            bmp.fill(0xF0);
        }
        // U+258D ▍ Left three eighths block
        0x0D => {
            bmp.fill(0xE0);
        }
        // U+258E ▎ Left one quarter block
        0x0E => {
            bmp.fill(0xC0);
        }
        // U+258F ▏ Left one eighth block
        0x0F => {
            bmp.fill(0x80);
        }
        // U+2590 ▐ Right half block
        0x10 => {
            bmp.fill(0x0F);
        }
        // U+2591 ░ Light shade (25% fill)
        0x11 => {
            for (i, row) in bmp.iter_mut().enumerate() {
                *row = if i % 4 == 0 { 0x88 } else if i % 4 == 2 { 0x22 } else { 0x00 };
            }
        }
        // U+2592 ▒ Medium shade (50% fill)
        0x12 => {
            for (i, row) in bmp.iter_mut().enumerate() {
                *row = if i % 2 == 0 { 0xAA } else { 0x55 };
            }
        }
        // U+2593 ▓ Dark shade (75% fill)
        0x13 => {
            for (i, row) in bmp.iter_mut().enumerate() {
                *row = if i % 4 == 0 { 0x77 } else if i % 4 == 2 { 0xDD } else { 0xFF };
            }
        }
        // U+2594 ▔ Upper one eighth block
        0x14 => {
            for row in bmp.iter_mut().take(2) { *row = 0xFF; }
        }
        // U+2595 ▕ Right one eighth block
        0x15 => {
            bmp.fill(0x01);
        }
        // U+2596 ▖ Quadrant lower left
        0x16 => {
            for row in bmp.iter_mut().skip(8) { *row = 0xF0; }
        }
        // U+2597 ▗ Quadrant lower right
        0x17 => {
            for row in bmp.iter_mut().skip(8) { *row = 0x0F; }
        }
        // U+2598 ▘ Quadrant upper left
        0x18 => {
            for row in bmp.iter_mut().take(8) { *row = 0xF0; }
        }
        // U+2599 ▙ Quadrant upper left and lower left and lower right
        0x19 => {
            for (i, row) in bmp.iter_mut().enumerate() {
                *row = if i < 8 { 0xF0 } else { 0xFF };
            }
        }
        // U+259A ▚ Quadrant upper left and lower right
        0x1A => {
            for (i, row) in bmp.iter_mut().enumerate() {
                *row = if i < 8 { 0xF0 } else { 0x0F };
            }
        }
        // U+259B ▛ Quadrant upper left and upper right and lower left
        0x1B => {
            for (i, row) in bmp.iter_mut().enumerate() {
                *row = if i < 8 { 0xFF } else { 0xF0 };
            }
        }
        // U+259C ▜ Quadrant upper left and upper right and lower right
        0x1C => {
            for (i, row) in bmp.iter_mut().enumerate() {
                *row = if i < 8 { 0xFF } else { 0x0F };
            }
        }
        // U+259D ▝ Quadrant upper right
        0x1D => {
            for row in bmp.iter_mut().take(8) { *row = 0x0F; }
        }
        // U+259E ▞ Quadrant upper right and lower left
        0x1E => {
            for (i, row) in bmp.iter_mut().enumerate() {
                *row = if i < 8 { 0x0F } else { 0xF0 };
            }
        }
        // U+259F ▟ Quadrant upper right and lower left and lower right
        0x1F => {
            for (i, row) in bmp.iter_mut().enumerate() {
                *row = if i < 8 { 0x0F } else { 0xFF };
            }
        }
        _ => {}
    }
    bmp
}

// ---------------------------------------------------------------------------
// Geometric Shapes subset (U+25A0–U+25FF)
// ---------------------------------------------------------------------------

/// Generate a glyph for common geometric shapes.
fn geometric_glyph(cp: u32) -> [u8; 16] {
    match cp {
        // U+25A0 ■ Black Square
        0x25A0 => {
            let mut bmp = [0u8; 16];
            for row in bmp.iter_mut().skip(3).take(10) { *row = 0x7E; }
            bmp
        }
        // U+25A1 □ White Square
        0x25A1 => {
            let mut bmp = [0u8; 16];
            bmp[3] = 0x7E;
            for row in bmp.iter_mut().skip(4).take(8) { *row = 0x42; }
            bmp[12] = 0x7E;
            bmp
        }
        // U+25AA ▪ Black Small Square
        0x25AA => {
            let mut bmp = [0u8; 16];
            for row in bmp.iter_mut().skip(5).take(6) { *row = 0x3C; }
            bmp
        }
        // U+25AB ▫ White Small Square
        0x25AB => {
            let mut bmp = [0u8; 16];
            bmp[5] = 0x3C;
            for row in bmp.iter_mut().skip(6).take(4) { *row = 0x24; }
            bmp[10] = 0x3C;
            bmp
        }
        // U+25B2 ▲ Black Up-Pointing Triangle
        0x25B2 => [
            0x00, 0x00, 0x00, 0x08, 0x1C, 0x1C, 0x3E, 0x3E,
            0x7F, 0x7F, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00,
        ],
        // U+25B6 ▶ Black Right-Pointing Triangle
        0x25B6 => [
            0x00, 0x00, 0x00, 0x40, 0x60, 0x70, 0x78, 0x7C,
            0x78, 0x70, 0x60, 0x40, 0x00, 0x00, 0x00, 0x00,
        ],
        // U+25BC ▼ Black Down-Pointing Triangle
        0x25BC => [
            0x00, 0x00, 0x00, 0xFF, 0xFF, 0x7F, 0x7F, 0x3E,
            0x3E, 0x1C, 0x1C, 0x08, 0x00, 0x00, 0x00, 0x00,
        ],
        // U+25C0 ◀ Black Left-Pointing Triangle
        0x25C0 => [
            0x00, 0x00, 0x00, 0x02, 0x06, 0x0E, 0x1E, 0x3E,
            0x1E, 0x0E, 0x06, 0x02, 0x00, 0x00, 0x00, 0x00,
        ],
        // U+25C6 ◆ Black Diamond
        0x25C6 => [
            0x00, 0x00, 0x00, 0x08, 0x1C, 0x3E, 0x7F, 0xFF,
            0x7F, 0x3E, 0x1C, 0x08, 0x00, 0x00, 0x00, 0x00,
        ],
        // U+25C7 ◇ White Diamond
        0x25C7 => [
            0x00, 0x00, 0x00, 0x08, 0x14, 0x22, 0x41, 0x82,
            0x41, 0x22, 0x14, 0x08, 0x00, 0x00, 0x00, 0x00,
        ],
        // U+25CF ● Black Circle (also in glyph_for_codepoint)
        0x25CF => FILLED_CIRCLE_GLYPH,
        // U+25CB ○ White Circle (also in glyph_for_codepoint)
        0x25CB => EMPTY_CIRCLE_GLYPH,
        _ => REPLACEMENT_GLYPH,
    }
}

// ---------------------------------------------------------------------------
// Arrow glyphs (U+2190–U+2199)
// ---------------------------------------------------------------------------

/// Arrow characters: ← ↑ → ↓ ↔ ↕ ↖ ↗ ↘ ↙
static ARROW_GLYPHS: [&[u8; 16]; 10] = [
    // U+2190 ← Leftwards Arrow
    &[0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x40, 0xFF,
      0x40, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    // U+2191 ↑ Upwards Arrow
    &[0x00, 0x00, 0x00, 0x08, 0x1C, 0x3E, 0x08, 0x08,
      0x08, 0x08, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00],
    // U+2192 → Rightwards Arrow
    &[0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x02, 0xFF,
      0x02, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    // U+2193 ↓ Downwards Arrow
    &[0x00, 0x00, 0x00, 0x08, 0x08, 0x08, 0x08, 0x08,
      0x3E, 0x1C, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00],
    // U+2194 ↔ Left Right Arrow
    &[0x00, 0x00, 0x00, 0x00, 0x00, 0x24, 0x42, 0xFF,
      0x42, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    // U+2195 ↕ Up Down Arrow
    &[0x00, 0x00, 0x08, 0x1C, 0x3E, 0x08, 0x08, 0x08,
      0x08, 0x08, 0x3E, 0x1C, 0x08, 0x00, 0x00, 0x00],
    // U+2196 ↖ North West Arrow
    &[0x00, 0x00, 0x00, 0x7C, 0x60, 0x50, 0x48, 0x04,
      0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    // U+2197 ↗ North East Arrow
    &[0x00, 0x00, 0x00, 0x3E, 0x06, 0x0A, 0x12, 0x20,
      0x40, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    // U+2198 ↘ South East Arrow
    &[0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x40, 0x20,
      0x12, 0x0A, 0x06, 0x3E, 0x00, 0x00, 0x00, 0x00],
    // U+2199 ↙ South West Arrow
    &[0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x04,
      0x48, 0x50, 0x60, 0x7C, 0x00, 0x00, 0x00, 0x00],
];

// ---------------------------------------------------------------------------
// Miscellaneous symbol glyphs
// ---------------------------------------------------------------------------

/// • Bullet (U+2022)
static BULLET_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x3C,
    0x3C, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// … Horizontal Ellipsis (U+2026)
static ELLIPSIS_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x92, 0x00, 0x00, 0x00,
];

/// · Middle Dot (U+00B7)
static MIDDLE_DOT_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18,
    0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// ✓/✔ Check Mark (U+2713/U+2714)
static CHECK_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x04,
    0x88, 0x50, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// ✗/✘ Ballot X (U+2717/U+2718)
static CROSS_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x82, 0x44, 0x28, 0x10,
    0x28, 0x44, 0x82, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// ● Filled Circle (U+25CF)
static FILLED_CIRCLE_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x18, 0x3C, 0x7E, 0x7E,
    0x7E, 0x7E, 0x3C, 0x18, 0x00, 0x00, 0x00, 0x00,
];

/// ○ Empty Circle (U+25CB)
static EMPTY_CIRCLE_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x18, 0x24, 0x42, 0x42,
    0x42, 0x42, 0x24, 0x18, 0x00, 0x00, 0x00, 0x00,
];

/// ★ Filled Star (U+2605)
static FILLED_STAR_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x08, 0x08, 0x1C, 0x7F, 0x3E,
    0x1C, 0x36, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// ☆ Empty Star (U+2606)
static EMPTY_STAR_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x08, 0x08, 0x14, 0x63, 0x36,
    0x14, 0x36, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// ‣ Triangle Right (U+2023)
static TRIANGLE_RIGHT_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x40, 0x60, 0x70, 0x78,
    0x70, 0x60, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// « Left-Pointing Double Angle Quotation Mark (U+00AB)
static GUILLEMET_LEFT_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0x24, 0x48,
    0x24, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// » Right-Pointing Double Angle Quotation Mark (U+00BB)
static GUILLEMET_RIGHT_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0x24, 0x12,
    0x24, 0x48, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// — Em Dash (U+2014)
static EM_DASH_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// – En Dash (U+2013)
static EN_DASH_GLYPH: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7E,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// ' Smart single quote → regular apostrophe shape (U+2018/U+2019)
static SINGLE_QUOTE_GLYPH: [u8; 16] = [
    0x00, 0x0C, 0x0C, 0x0C, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// " Smart double quote → regular quote shape (U+201C/U+201D)
static DOUBLE_QUOTE_GLYPH: [u8; 16] = [
    0x00, 0x36, 0x36, 0x36, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run self-tests for Unicode support.
///
/// Verifies UTF-8 decoding, character width detection, glyph generation
/// for box drawing and block elements, and the replacement character
/// fallback.
pub fn self_test() {
    // 1. UTF-8 sequence length detection.
    assert_eq!(utf8_seq_len(b'A'), 1);
    assert_eq!(utf8_seq_len(0xC3), 2); // lead byte of 2-byte seq (ü etc.)
    assert_eq!(utf8_seq_len(0xE2), 3); // lead byte of 3-byte seq (box drawing)
    assert_eq!(utf8_seq_len(0xF0), 4); // lead byte of 4-byte seq (emoji)
    assert_eq!(utf8_seq_len(0x80), 0); // continuation byte → invalid
    assert_eq!(utf8_seq_len(0xC0), 0); // overlong lead → invalid
    crate::serial_println!("[unicode]   UTF-8 sequence length: OK");

    // 2. UTF-8 decoding.
    // 'A' = U+0041
    let mut buf = [0u8; 4];
    buf[0] = b'A';
    assert_eq!(decode_utf8(buf, 1), 0x41);

    // 'ü' = U+00FC = 0xC3 0xBC
    buf = [0xC3, 0xBC, 0, 0];
    assert_eq!(decode_utf8(buf, 2), 0xFC);

    // '─' = U+2500 = 0xE2 0x94 0x80
    buf = [0xE2, 0x94, 0x80, 0];
    assert_eq!(decode_utf8(buf, 3), 0x2500);

    // '😀' = U+1F600 = 0xF0 0x9F 0x98 0x80
    buf = [0xF0, 0x9F, 0x98, 0x80];
    assert_eq!(decode_utf8(buf, 4), 0x1F600);

    // Overlong encoding (U+0041 encoded as 2 bytes) → replacement.
    buf = [0xC1, 0x81, 0, 0];
    assert_eq!(decode_utf8(buf, 2), 0xFFFD);
    crate::serial_println!("[unicode]   UTF-8 decoding: OK");

    // 3. Character width.
    assert_eq!(char_width(u32::from(b'A')), 1);
    assert_eq!(char_width(0x2500), 1); // box drawing
    assert_eq!(char_width(0x4E00), 2); // CJK ideograph
    assert_eq!(char_width(0xAC00), 2); // Hangul syllable
    crate::serial_println!("[unicode]   Character width: OK");

    // 4. Box drawing glyph generation.
    // U+2500 ─ : horizontal line at CY, full width.
    let g = box_drawing_glyph(0); // ─
    assert!(g[CY] != 0, "─ must have pixels at center row");
    // All rows except CY should be empty for a simple light horizontal.
    for (i, row) in g.iter().enumerate() {
        if i != CY {
            assert_eq!(*row, 0, "─ row {i} should be empty");
        }
    }

    // U+2502 │ : vertical line at CX, full height.
    let g = box_drawing_glyph(2); // │
    let vert_bit = 1u8 << (7 - CX);
    for row in &g {
        assert!((*row & vert_bit) != 0, "│ must have center column set");
    }

    // U+250C ┌ : right + down from center.
    let g = box_drawing_glyph(0x0C); // ┌
    // Center row should have right segment.
    assert!(g[CY] != 0, "┌ center row should have pixels");
    // Below center should have vertical segment.
    assert!((g[CY + 1] & vert_bit) != 0, "┌ should have vert below center");
    // Above center should be empty.
    assert_eq!(g[0], 0, "┌ top row should be empty");
    crate::serial_println!("[unicode]   Box drawing generation: OK");

    // 5. Block element glyph generation.
    // U+2588 █ : full block — all bytes 0xFF.
    let g = block_element_glyph(0x08); // █
    for row in &g {
        assert_eq!(*row, 0xFF, "█ should be fully filled");
    }

    // U+2580 ▀ : upper half — top 8 rows filled, bottom 8 empty.
    let g = block_element_glyph(0x00); // ▀
    for row in g.iter().take(8) {
        assert_eq!(*row, 0xFF, "▀ upper rows should be filled");
    }
    for row in g.iter().skip(8) {
        assert_eq!(*row, 0x00, "▀ lower rows should be empty");
    }

    // U+258C ▌ : left half — each row 0xF0.
    let g = block_element_glyph(0x0C); // ▌
    for row in &g {
        assert_eq!(*row, 0xF0, "▌ should be 0xF0");
    }
    crate::serial_println!("[unicode]   Block element generation: OK");

    // 6. Glyph lookup dispatch.
    let (g, w) = glyph_for_codepoint(u32::from(b'A'));
    assert!(!w, "ASCII should not be wide");
    assert_ne!(g, [0u8; 16], "ASCII 'A' should not be blank");

    let (g, w) = glyph_for_codepoint(0x2500); // ─
    assert!(!w, "box drawing should not be wide");
    assert!(g[CY] != 0, "─ should have center row");

    let (_, w) = glyph_for_codepoint(0x4E00); // 一 (CJK)
    assert!(w, "CJK should be wide");

    let (g, _) = glyph_for_codepoint(0xFFFD); // replacement
    assert_eq!(g, REPLACEMENT_GLYPH);
    crate::serial_println!("[unicode]   Glyph lookup dispatch: OK");

    crate::serial_println!("[unicode] Self-test PASSED");
}

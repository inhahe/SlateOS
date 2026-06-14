//! EDID (Extended Display Identification Data) parser.
//!
//! Parses the 128-byte base EDID block (and optional CEA-861 extension
//! blocks) to extract display modes, manufacturer info, and physical
//! characteristics.
//!
//! ## EDID Structure (Base Block — 128 bytes)
//!
//! | Offset | Length | Description                               |
//! |--------|--------|-------------------------------------------|
//! | 0      | 8      | Header: `00 FF FF FF FF FF FF 00`         |
//! | 8      | 2      | Manufacturer ID (3 letters, 5 bits each)  |
//! | 10     | 2      | Product code (LE u16)                     |
//! | 12     | 4      | Serial number (LE u32)                    |
//! | 16     | 1      | Week of manufacture                       |
//! | 17     | 1      | Year of manufacture (+ 1990)              |
//! | 18     | 1      | EDID version                              |
//! | 19     | 1      | EDID revision                             |
//! | 20     | 1      | Video input definition                    |
//! | 21     | 1      | Horizontal screen size (cm)               |
//! | 22     | 1      | Vertical screen size (cm)                 |
//! | 23     | 1      | Gamma (= gamma * 100 - 100)               |
//! | 24     | 1      | Feature support flags                     |
//! | 25     | 10     | Chromaticity coordinates                  |
//! | 35     | 3      | Established timing bitmap                 |
//! | 38     | 16     | Standard timings (8 entries, 2 bytes each) |
//! | 54     | 72     | 4 Detailed descriptors (18 bytes each)    |
//! | 126    | 1      | Number of extension blocks                |
//! | 127    | 1      | Checksum (all 128 bytes sum to 0)         |
//!
//! ## References
//!
//! - VESA E-EDID Standard v1.4 (2006)
//! - CEA-861-G (2017) for extension block parsing
//! - Linux `drivers/gpu/drm/drm_edid.c` for real-world quirks

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use super::mode::{DrmMode, DrmModeFlags};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// EDID base block size.
const EDID_BLOCK_SIZE: usize = 128;

/// Required header bytes.
const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// CEA extension block tag.
const CEA_EXTENSION_TAG: u8 = 0x02;

// ---------------------------------------------------------------------------
// Parsed EDID result
// ---------------------------------------------------------------------------

/// Parsed EDID data from a display.
#[derive(Debug, Clone)]
pub struct EdidInfo {
    /// Three-letter manufacturer ID (PNP ID), e.g., "SAM" for Samsung.
    pub manufacturer: [u8; 3],
    /// Product code.
    pub product_code: u16,
    /// Serial number (0 if not specified).
    pub serial_number: u32,
    /// Week of manufacture (0 or 1-54).
    pub manufacture_week: u8,
    /// Year of manufacture (actual year, e.g. 2024).
    pub manufacture_year: u16,
    /// EDID version (typically 1).
    pub version: u8,
    /// EDID revision (typically 3 or 4).
    pub revision: u8,
    /// Whether the display uses a digital input (true) or analog (false).
    pub digital_input: bool,
    /// Horizontal screen size in centimeters (0 if not specified).
    pub screen_width_cm: u8,
    /// Vertical screen size in centimeters (0 if not specified).
    pub screen_height_cm: u8,
    /// Display gamma * 100 (e.g., 220 for gamma 2.20). 0 if not specified.
    pub gamma_x100: u16,
    /// Number of extension blocks present.
    pub extension_count: u8,
    /// Display modes extracted from EDID (established, standard, detailed).
    ///
    /// Ordered by preference: detailed timings first (highest quality),
    /// then standard timings, then established timings.
    pub modes: Vec<DrmMode>,
    /// Monitor name from display descriptor (if present).
    pub monitor_name: Option<[u8; 13]>,
    /// Length of the monitor name.
    pub monitor_name_len: u8,
}

impl EdidInfo {
    /// Get the monitor name as a byte slice.
    #[must_use]
    pub fn name_str(&self) -> &[u8] {
        match &self.monitor_name {
            Some(name) => &name[..self.monitor_name_len as usize],
            None => b"Unknown",
        }
    }

    /// Get the manufacturer ID as a string slice.
    #[must_use]
    pub fn manufacturer_str(&self) -> &[u8; 3] {
        &self.manufacturer
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a raw EDID data blob.
///
/// `data` must be at least 128 bytes (one EDID block).  Extension blocks
/// (if present) start at byte 128 and are 128 bytes each.
///
/// Returns parsed display information including all detected modes.
pub fn parse(data: &[u8]) -> KernelResult<EdidInfo> {
    if data.len() < EDID_BLOCK_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    // --- Validate header ---
    if data[..8] != EDID_HEADER {
        return Err(KernelError::InvalidArgument);
    }

    // --- Validate checksum ---
    if !validate_checksum(&data[..EDID_BLOCK_SIZE]) {
        return Err(KernelError::InvalidArgument);
    }

    // --- Manufacturer ID (bytes 8-9) ---
    // Encoded as three 5-bit characters: bits 14-10, 9-5, 4-0 of u16 BE.
    let mfg_raw = u16::from_be_bytes([data[8], data[9]]);
    let manufacturer = decode_manufacturer_id(mfg_raw);

    // --- Product code (bytes 10-11, LE) ---
    let product_code = u16::from_le_bytes([data[10], data[11]]);

    // --- Serial number (bytes 12-15, LE) ---
    let serial_number = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    // --- Manufacture date (bytes 16-17) ---
    let manufacture_week = data[16];
    #[allow(clippy::arithmetic_side_effects)]
    let manufacture_year = (data[17] as u16) + 1990;

    // --- Version/revision (bytes 18-19) ---
    let version = data[18];
    let revision = data[19];

    // --- Video input (byte 20) ---
    let digital_input = (data[20] & 0x80) != 0;

    // --- Screen size (bytes 21-22) ---
    let screen_width_cm = data[21];
    let screen_height_cm = data[22];

    // --- Gamma (byte 23) ---
    // Stored as (gamma * 100) - 100.  Value 0xFF means gamma is defined
    // in an extension block.
    #[allow(clippy::arithmetic_side_effects)]
    let gamma_x100 = if data[23] == 0xFF {
        0
    } else {
        (data[23] as u16) + 100
    };

    // --- Extension count (byte 126) ---
    let extension_count = data[126];

    // --- Extract modes ---
    let mut modes = Vec::new();

    // 1. Detailed timing descriptors (bytes 54-125, 4 * 18 bytes).
    //    These are the highest-quality mode descriptions.
    for i in 0..4 {
        #[allow(clippy::arithmetic_side_effects)]
        let offset = 54 + i * 18;
        if let Some(mode) = parse_detailed_timing(&data[offset..offset + 18]) {
            modes.push(mode);
        }
    }

    // 2. Standard timings (bytes 38-53, 8 * 2 bytes).
    for i in 0..8 {
        #[allow(clippy::arithmetic_side_effects)]
        let offset = 38 + i * 2;
        if let Some(mode) = parse_standard_timing(data[offset], data[offset + 1], version, revision) {
            // Don't add duplicates (detailed timings already cover the preferred mode).
            if !modes.iter().any(|m| m.hdisplay == mode.hdisplay && m.vdisplay == mode.vdisplay && m.vrefresh == mode.vrefresh) {
                modes.push(mode);
            }
        }
    }

    // 3. Established timings (bytes 35-37).
    let established = parse_established_timings(data[35], data[36], data[37]);
    for mode in established {
        if !modes.iter().any(|m| m.hdisplay == mode.hdisplay && m.vdisplay == mode.vdisplay && m.vrefresh == mode.vrefresh) {
            modes.push(mode);
        }
    }

    // 4. CEA extension blocks (if present).
    if extension_count > 0 {
        let ext_start = EDID_BLOCK_SIZE;
        for ext_idx in 0..extension_count as usize {
            #[allow(clippy::arithmetic_side_effects)]
            let ext_offset = ext_start + ext_idx * EDID_BLOCK_SIZE;
            #[allow(clippy::arithmetic_side_effects)]
            let ext_end = ext_offset + EDID_BLOCK_SIZE;
            if ext_end <= data.len() {
                let ext_block = &data[ext_offset..ext_end];
                if ext_block[0] == CEA_EXTENSION_TAG && validate_checksum(ext_block) {
                    let cea_modes = parse_cea_extension(ext_block);
                    for mode in cea_modes {
                        if !modes.iter().any(|m| m.hdisplay == mode.hdisplay && m.vdisplay == mode.vdisplay && m.vrefresh == mode.vrefresh) {
                            modes.push(mode);
                        }
                    }
                }
            }
        }
    }

    // Mark the first mode as preferred (the first detailed timing is the
    // display's native/preferred mode per the EDID spec).
    if let Some(first) = modes.first_mut() {
        first.flags = DrmModeFlags::PREFERRED;
    }

    // --- Extract monitor name from display descriptors ---
    let (monitor_name, monitor_name_len) = extract_monitor_name(data);

    Ok(EdidInfo {
        manufacturer,
        product_code,
        serial_number,
        manufacture_week,
        manufacture_year,
        version,
        revision,
        digital_input,
        screen_width_cm,
        screen_height_cm,
        gamma_x100,
        extension_count,
        modes,
        monitor_name,
        monitor_name_len,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Validate that all bytes in a 128-byte block sum to 0 (mod 256).
fn validate_checksum(block: &[u8]) -> bool {
    let sum: u8 = block.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    sum == 0
}

/// Decode the 2-byte manufacturer ID into three ASCII letters.
///
/// Each letter is encoded as a 5-bit value (A=1, B=2, ..., Z=26)
/// packed into a big-endian u16: bits 14-10 = first, 9-5 = second, 4-0 = third.
#[allow(clippy::arithmetic_side_effects)]
fn decode_manufacturer_id(raw: u16) -> [u8; 3] {
    let c1 = ((raw >> 10) & 0x1F) as u8;
    let c2 = ((raw >> 5) & 0x1F) as u8;
    let c3 = (raw & 0x1F) as u8;
    // Convert 1-based to ASCII: 1='A', 2='B', ...
    [
        if c1 > 0 && c1 <= 26 { b'A' + c1 - 1 } else { b'?' },
        if c2 > 0 && c2 <= 26 { b'A' + c2 - 1 } else { b'?' },
        if c3 > 0 && c3 <= 26 { b'A' + c3 - 1 } else { b'?' },
    ]
}

/// Parse a detailed timing descriptor (18 bytes).
///
/// Returns `None` if the descriptor is a display descriptor (first two
/// bytes are zero) or if the timing data is invalid.
#[allow(clippy::arithmetic_side_effects)]
fn parse_detailed_timing(desc: &[u8]) -> Option<DrmMode> {
    // Pixel clock in 10 kHz units (LE u16).  Zero means display descriptor.
    let pixel_clock_10khz = u16::from_le_bytes([desc[0], desc[1]]);
    if pixel_clock_10khz == 0 {
        return None;
    }

    // Horizontal active pixels.
    let h_active_lo = desc[2] as u32;
    let h_active_hi = ((desc[4] >> 4) & 0x0F) as u32;
    let h_active = (h_active_hi << 8) | h_active_lo;

    // Horizontal blanking pixels.
    let h_blank_lo = desc[3] as u32;
    let h_blank_hi = (desc[4] & 0x0F) as u32;
    let h_blank = (h_blank_hi << 8) | h_blank_lo;

    // Vertical active lines.
    let v_active_lo = desc[5] as u32;
    let v_active_hi = ((desc[7] >> 4) & 0x0F) as u32;
    let v_active = (v_active_hi << 8) | v_active_lo;

    // Vertical blanking lines.
    let v_blank_lo = desc[6] as u32;
    let v_blank_hi = (desc[7] & 0x0F) as u32;
    let v_blank = (v_blank_hi << 8) | v_blank_lo;

    if h_active == 0 || v_active == 0 {
        return None;
    }

    let htotal = h_active + h_blank;
    let vtotal = v_active + v_blank;

    // Pixel clock in kHz.
    let clock_khz = (pixel_clock_10khz as u32) * 10;

    // Refresh rate = pixel_clock / (htotal * vtotal).
    let vrefresh = if htotal > 0 && vtotal > 0 {
        // Use u64 to avoid overflow: clock_khz * 1000 / (htotal * vtotal).
        let numer = (clock_khz as u64) * 1000;
        let denom = (htotal as u64) * (vtotal as u64);
        if denom > 0 { (numer / denom) as u32 } else { 60 }
    } else {
        60
    };

    // Interlace flag (byte 17, bit 7).
    let interlaced = (desc[17] & 0x80) != 0;
    let flags = if interlaced {
        DrmModeFlags::INTERLACE
    } else {
        DrmModeFlags::empty()
    };

    let mut mode = DrmMode::from_resolution(h_active, v_active, vrefresh);
    mode.clock = clock_khz;
    mode.htotal = htotal;
    mode.vtotal = vtotal;
    mode.flags = flags;

    Some(mode)
}

/// Parse a standard timing entry (2 bytes).
///
/// Returns `None` for unused entries (both bytes 0x01 or both 0x00).
#[allow(clippy::arithmetic_side_effects)]
fn parse_standard_timing(byte0: u8, byte1: u8, version: u8, revision: u8) -> Option<DrmMode> {
    // Unused entries are marked with 0x0101 or 0x0000.
    if (byte0 == 0x01 && byte1 == 0x01) || (byte0 == 0x00 && byte1 == 0x00) {
        return None;
    }

    // Horizontal active pixels = (byte0 + 31) * 8.
    let h_active = ((byte0 as u32) + 31) * 8;
    if h_active < 256 {
        return None;
    }

    // Aspect ratio (bits 7-6 of byte1).
    let aspect = (byte1 >> 6) & 0x03;
    let v_active = match aspect {
        0b00 => {
            // EDID 1.3+: 16:10.  EDID 1.2 and earlier: 1:1.
            if version > 1 || (version == 1 && revision >= 3) {
                h_active * 10 / 16
            } else {
                h_active
            }
        }
        0b01 => h_active * 3 / 4,   // 4:3
        0b10 => h_active * 4 / 5,   // 5:4
        0b11 => h_active * 9 / 16,  // 16:9
        _ => return None,
    };

    // Vertical frequency = (bits 5-0 of byte1) + 60.
    let vrefresh = ((byte1 & 0x3F) as u32) + 60;

    Some(DrmMode::from_resolution(h_active, v_active, vrefresh))
}

/// Parse the established timings bitmap (3 bytes).
///
/// Returns a list of standard modes indicated by the bitmap.
fn parse_established_timings(byte0: u8, byte1: u8, byte2: u8) -> Vec<DrmMode> {
    let mut modes = Vec::new();

    // Established Timing I (byte 0).
    let established_i: [(u8, u32, u32, u32); 8] = [
        (0x80, 720, 400, 70),
        (0x40, 720, 400, 88),
        (0x20, 640, 480, 60),
        (0x10, 640, 480, 67),
        (0x08, 640, 480, 72),
        (0x04, 640, 480, 75),
        (0x02, 800, 600, 56),
        (0x01, 800, 600, 60),
    ];

    for &(mask, w, h, hz) in &established_i {
        if byte0 & mask != 0 {
            modes.push(DrmMode::from_resolution(w, h, hz));
        }
    }

    // Established Timing II (byte 1).
    let established_ii: [(u8, u32, u32, u32); 8] = [
        (0x80, 800, 600, 72),
        (0x40, 800, 600, 75),
        (0x20, 832, 624, 75),
        (0x10, 1024, 768, 87),  // Interlaced, but we report as 87Hz.
        (0x08, 1024, 768, 60),
        (0x04, 1024, 768, 70),
        (0x02, 1024, 768, 75),
        (0x01, 1280, 1024, 75),
    ];

    for &(mask, w, h, hz) in &established_ii {
        if byte1 & mask != 0 {
            modes.push(DrmMode::from_resolution(w, h, hz));
        }
    }

    // Manufacturer-specific (byte 2) — bit 7 only is defined.
    if byte2 & 0x80 != 0 {
        modes.push(DrmMode::from_resolution(1152, 870, 75));
    }

    modes
}

/// Parse a CEA-861 extension block for additional modes.
///
/// CEA blocks contain a data block collection with video data blocks
/// (tag 0x02) listing supported CEA/CTA video modes by their VIC
/// (Video Identification Code).
#[allow(clippy::arithmetic_side_effects)]
fn parse_cea_extension(block: &[u8]) -> Vec<DrmMode> {
    let mut modes = Vec::new();

    // Byte 0: tag (0x02)
    // Byte 1: revision (should be 3 for CEA-861-D+)
    // Byte 2: DTD offset (byte within this block where DTDs start; 0 = no DTDs)
    // Byte 3: number of native DTDs in lower 4 bits, feature flags in upper
    let revision = block[1];
    if revision < 3 {
        return modes;
    }

    let dtd_offset = block[2] as usize;
    if dtd_offset < 4 {
        // No data blocks (or invalid offset).
        return modes;
    }

    // Parse data blocks from byte 4 to dtd_offset.
    let mut pos = 4;
    while pos < dtd_offset && pos < EDID_BLOCK_SIZE {
        let header = block[pos];
        let tag = (header >> 5) & 0x07;
        let length = (header & 0x1F) as usize;

        if pos + 1 + length > dtd_offset || pos + 1 + length > EDID_BLOCK_SIZE {
            break;
        }

        // Tag 0x02 = Video Data Block — contains VIC numbers.
        if tag == 0x02 {
            for i in 0..length {
                let vic = block[pos + 1 + i] & 0x7F; // Bit 7 = native flag.
                if let Some(mode) = vic_to_mode(vic) {
                    modes.push(mode);
                }
            }
        }

        pos += 1 + length;
    }

    // Parse detailed timing descriptors after dtd_offset.
    if dtd_offset > 0 && dtd_offset < EDID_BLOCK_SIZE {
        let mut dtd_pos = dtd_offset;
        while dtd_pos + 18 <= EDID_BLOCK_SIZE {
            // Stop at padding (all zeros).
            if block[dtd_pos] == 0 && block[dtd_pos + 1] == 0 {
                break;
            }
            if let Some(mode) = parse_detailed_timing(&block[dtd_pos..dtd_pos + 18]) {
                modes.push(mode);
            }
            dtd_pos += 18;
        }
    }

    modes
}

/// Convert a CEA VIC (Video Identification Code) to a display mode.
///
/// Only the most common VICs are included.  Full table: CEA-861-G Table 1.
fn vic_to_mode(vic: u8) -> Option<DrmMode> {
    let (w, h, hz) = match vic {
        1 => (640, 480, 60),
        2 | 3 => (720, 480, 60),
        4 => (1280, 720, 60),
        5 => (1920, 1080, 60),    // 1080i, but report as 60Hz
        16 => (1920, 1080, 60),
        17 | 18 => (720, 576, 50),
        19 => (1280, 720, 50),
        20 => (1920, 1080, 50),   // 1080i @ 50
        31 => (1920, 1080, 50),
        32 => (1920, 1080, 24),
        33 => (1920, 1080, 25),
        34 => (1920, 1080, 30),
        39 => (1920, 1080, 50),
        40 => (1920, 1080, 100),
        41 => (1280, 720, 100),
        46 => (1920, 1080, 120),
        47 => (1280, 720, 120),
        60 => (1280, 720, 24),
        61 => (1280, 720, 25),
        62 => (1280, 720, 30),
        63 => (1920, 1080, 120),
        64 => (1920, 1080, 100),
        // 4K modes (CTA-861-G)
        93 => (3840, 2160, 24),
        94 => (3840, 2160, 25),
        95 => (3840, 2160, 30),
        96 => (3840, 2160, 50),
        97 => (3840, 2160, 60),
        98 => (4096, 2160, 24),
        99 => (4096, 2160, 25),
        100 => (4096, 2160, 30),
        101 => (4096, 2160, 50),
        102 => (4096, 2160, 60),
        _ => return None,
    };
    Some(DrmMode::from_resolution(w, h, hz))
}

/// Extract monitor name from display descriptors.
///
/// Scans the four 18-byte descriptor slots for a "Monitor Name"
/// descriptor (tag 0xFC).  Returns the name and its length.
#[allow(clippy::arithmetic_side_effects)]
fn extract_monitor_name(data: &[u8]) -> (Option<[u8; 13]>, u8) {
    for i in 0..4 {
        let offset = 54 + i * 18;
        // Display descriptor: first two bytes are 0, byte 3 is tag.
        if data[offset] == 0 && data[offset + 1] == 0 {
            let tag = data[offset + 3];
            if tag == 0xFC {
                // Monitor name is in bytes 5-17 of the descriptor,
                // padded with 0x0A (newline) and trailing spaces.
                let mut name = [0u8; 13];
                let mut len = 0u8;
                for j in 0..13 {
                    let c = data[offset + 5 + j];
                    if c == 0x0A || c == 0x00 {
                        break;
                    }
                    name[j] = c;
                    len += 1;
                }
                return (Some(name), len);
            }
        }
    }
    (None, 0)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run EDID parser self-tests.
pub(crate) fn self_test() -> KernelResult<()> {
    // Build a synthetic EDID block for a 1920x1080@60 display.
    let edid = build_test_edid();

    // 1. Parse should succeed.
    let info = parse(&edid)?;

    // 2. Manufacturer ID should decode correctly.
    if info.manufacturer != *b"TST" {
        serial_println!(
            "[drm]   FAIL: EDID manufacturer mismatch: {:?}",
            info.manufacturer
        );
        return Err(KernelError::InternalError);
    }

    // 3. Product code.
    if info.product_code != 0x1234 {
        serial_println!(
            "[drm]   FAIL: EDID product code mismatch: {:#x}",
            info.product_code
        );
        return Err(KernelError::InternalError);
    }

    // 4. Should have extracted at least one mode.
    if info.modes.is_empty() {
        serial_println!("[drm]   FAIL: EDID parsed zero modes");
        return Err(KernelError::InternalError);
    }

    // 5. The first (preferred) mode should be 1920x1080@60.
    let preferred = &info.modes[0];
    if preferred.hdisplay != 1920 || preferred.vdisplay != 1080 {
        serial_println!(
            "[drm]   FAIL: EDID preferred mode mismatch: {}x{}",
            preferred.hdisplay,
            preferred.vdisplay,
        );
        return Err(KernelError::InternalError);
    }
    if preferred.flags != DrmModeFlags::PREFERRED {
        serial_println!("[drm]   FAIL: preferred mode not flagged PREFERRED");
        return Err(KernelError::InternalError);
    }

    // 6. Should be a digital display.
    if !info.digital_input {
        serial_println!("[drm]   FAIL: expected digital input");
        return Err(KernelError::InternalError);
    }

    // 7. Monitor name should be "Test Monitor".
    let name = info.name_str();
    if name != b"Test Monitor" {
        serial_println!("[drm]   FAIL: monitor name mismatch");
        return Err(KernelError::InternalError);
    }

    // 8. Established timings: we set 640x480@60 and 800x600@60.
    let has_640 = info.modes.iter().any(|m| m.hdisplay == 640 && m.vdisplay == 480 && m.vrefresh == 60);
    let has_800 = info.modes.iter().any(|m| m.hdisplay == 800 && m.vdisplay == 600 && m.vrefresh == 60);
    if !has_640 || !has_800 {
        serial_println!("[drm]   FAIL: established timings not found");
        return Err(KernelError::InternalError);
    }

    // 9. Checksum validation: corrupt the block and verify failure.
    let mut bad_edid = edid.clone();
    #[allow(clippy::arithmetic_side_effects)]
    {
        bad_edid[127] = bad_edid[127].wrapping_add(1);
    }
    if parse(&bad_edid).is_ok() {
        serial_println!("[drm]   FAIL: corrupt EDID accepted");
        return Err(KernelError::InternalError);
    }

    // 10. Too-short data should fail.
    if parse(&edid[..64]).is_ok() {
        serial_println!("[drm]   FAIL: short EDID accepted");
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[drm]   EDID parser: OK ({} modes, mfg={}{}{}, \"{}\")",
        info.modes.len(),
        info.manufacturer[0] as char,
        info.manufacturer[1] as char,
        info.manufacturer[2] as char,
        core::str::from_utf8(name).unwrap_or("?"),
    );
    Ok(())
}

/// Build a synthetic 128-byte EDID block for testing.
///
/// Encodes a 1920x1080@60 display with manufacturer "TST", product 0x1234,
/// digital input, established timings for 640x480@60 and 800x600@60,
/// and a monitor name of "Test Monitor".
#[allow(clippy::arithmetic_side_effects)]
fn build_test_edid() -> Vec<u8> {
    let mut edid = vec![0u8; EDID_BLOCK_SIZE];

    // Header.
    edid[..8].copy_from_slice(&EDID_HEADER);

    // Manufacturer "TST": T=20, S=19, T=20.
    // Packed: (20 << 10) | (19 << 5) | 20 = 0x52_74
    let mfg: u16 = (20 << 10) | (19 << 5) | 20;
    edid[8..10].copy_from_slice(&mfg.to_be_bytes());

    // Product code 0x1234 (LE).
    edid[10..12].copy_from_slice(&0x1234u16.to_le_bytes());

    // Serial 0x00000001 (LE).
    edid[12..16].copy_from_slice(&1u32.to_le_bytes());

    // Manufacture week 1, year 2024 (2024 - 1990 = 34).
    edid[16] = 1;
    edid[17] = 34;

    // EDID version 1.4.
    edid[18] = 1;
    edid[19] = 4;

    // Digital input (bit 7 set), 8 bpc (bits 6-4 = 010 = 8 bpc for EDID 1.4).
    edid[20] = 0x80 | (0b010 << 4);

    // Screen size: 53 cm x 30 cm (~24" diagonal).
    edid[21] = 53;
    edid[22] = 30;

    // Gamma 2.20 → stored as 220 - 100 = 120.
    edid[23] = 120;

    // Feature support: RGB 4:4:4, preferred timing in DTD1.
    edid[24] = 0x06; // bit 2 = preferred timing mode, bit 1 = RGB 4:4:4

    // Chromaticity: all zeros (good enough for test).
    // bytes 25-34 left at 0.

    // Established timings: 640x480@60 (byte 35 bit 5) + 800x600@60 (byte 35 bit 0).
    edid[35] = 0x21; // 0b00100001
    edid[36] = 0x00;
    edid[37] = 0x00;

    // Standard timings: all unused (0x0101).
    for i in 0..8 {
        edid[38 + i * 2] = 0x01;
        edid[38 + i * 2 + 1] = 0x01;
    }

    // Detailed Timing Descriptor 1: 1920x1080@60Hz.
    // Pixel clock: 148500 kHz = 14850 * 10 kHz → LE u16 = 14850.
    //
    // Timing values from the standard 1080p60 CEA mode:
    // H active = 1920, H blank = 280 (total 2200)
    // V active = 1080, V blank = 45  (total 1125)
    let dtd1 = &mut edid[54..72];
    let pclk: u16 = 14850; // 148.50 MHz in 10 kHz units
    dtd1[0..2].copy_from_slice(&pclk.to_le_bytes());
    // Bind the timing values so the lo/hi-nibble splits are computed from
    // runtime values rather than folded to constants (the formula is the
    // generic EDID DTD packing, valid for any resolution).
    let h_active: u32 = 1920;
    let h_blank: u32 = 280;
    let v_active: u32 = 1080;
    let v_blank: u32 = 45;
    dtd1[2] = (h_active & 0xFF) as u8;         // H active lo
    dtd1[3] = (h_blank & 0xFF) as u8;          // H blank lo
    dtd1[4] = (((h_active >> 8) & 0x0F) << 4) as u8 | ((h_blank >> 8) & 0x0F) as u8;
    dtd1[5] = (v_active & 0xFF) as u8;         // V active lo
    dtd1[6] = (v_blank & 0xFF) as u8;          // V blank lo
    dtd1[7] = (((v_active >> 8) & 0x0F) << 4) as u8 | ((v_blank >> 8) & 0x0F) as u8;
    // H front porch = 88, H sync = 44.
    dtd1[8] = 88;  // H front porch lo
    dtd1[9] = 44;  // H sync width lo
    // V front porch = 4, V sync = 5.
    dtd1[10] = (4 << 4) | 5; // V front porch hi nibble + V sync width lo nibble
    dtd1[11] = 0;   // High bits of front porches/syncs.
    // Image size: 530mm x 300mm (matching our 53cm x 30cm).
    dtd1[12] = (530 & 0xFF) as u8;
    dtd1[13] = (300 & 0xFF) as u8;
    dtd1[14] = (((530 >> 8) & 0x0F) << 4) as u8 | ((300 >> 8) & 0x0F) as u8;
    // No border.
    dtd1[15] = 0;
    dtd1[16] = 0;
    // Features: non-interlaced, normal display.
    dtd1[17] = 0x18; // Digital separate sync, H pos, V pos

    // Descriptor 2: Monitor Name descriptor (tag 0xFC).
    let desc2 = &mut edid[72..90];
    desc2[0] = 0x00;
    desc2[1] = 0x00;
    desc2[2] = 0x00;
    desc2[3] = 0xFC; // Monitor name tag
    desc2[4] = 0x00;
    let name = b"Test Monitor\x0a";
    desc2[5..5 + name.len()].copy_from_slice(name);

    // Descriptors 3-4: unused (all zeros = fine).

    // Extension count: 0.
    edid[126] = 0;

    // Checksum: make all bytes sum to 0.
    let sum: u8 = edid[..127].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    edid[127] = 0u8.wrapping_sub(sum);

    edid
}

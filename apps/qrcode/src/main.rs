//! `SlateOS` QR Code Generator
//!
//! A QR code and barcode generation tool with:
//! - QR code generation from scratch (byte mode, Reed-Solomon EC, versions 1-10)
//! - Input modes: text, URL, email, phone, `WiFi`, vCard
//! - Customizable module size and foreground/background colors
//! - Code128 barcode generation
//! - History of recently generated codes
//! - Multi-panel UI: input, preview, and options panels
//!
//! Uses the guitk library for UI rendering.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
// QR-code generation is dense Reed-Solomon / Galois-field arithmetic on
// fixed-size lookup tables and matrix-grid indexing. The defensive
// `arithmetic_side_effects` and `indexing_slicing` lints fire on every
// table lookup and matrix poke with no real DoS risk: indices are
// computed from QR-version metadata, all bounded by the matrix
// dimension; arithmetic is on small u8/u16 finite-field values. Allow
// the lints file-wide; workspace discipline stays in place elsewhere.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::struct_excessive_bools)]
#![allow(dead_code)]

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);

// ============================================================================
// Layout constants
// ============================================================================

const TOOLBAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const LEFT_PANEL_WIDTH: f32 = 320.0;
const RIGHT_PANEL_WIDTH: f32 = 220.0;
const CORNER_RADIUS: f32 = 4.0;

// ============================================================================
// Galois Field GF(2^8) arithmetic for Reed-Solomon
// ============================================================================

/// Generator polynomial primitive: x^8 + x^4 + x^3 + x^2 + 1 (0x11D)
const GF_PRIMITIVE: u16 = 0x11D;

/// Compute GF(2^8) log and exp tables at compile time is not trivial,
/// so we build them at init. These are used by Reed-Solomon encoding.
struct GfTables {
    exp_table: [u8; 256],
    log_table: [u8; 256],
}

impl GfTables {
    fn new() -> Self {
        let mut exp_table = [0u8; 256];
        let mut log_table = [0u8; 256];

        let mut val: u16 = 1;
        for i in 0u16..255 {
            exp_table[i as usize] = val as u8;
            log_table[val as usize] = i as u8;
            val <<= 1;
            if val >= 256 {
                val ^= GF_PRIMITIVE;
            }
        }
        // exp[255] = exp[0] for wrap-around
        exp_table[255] = exp_table[0];

        Self {
            exp_table,
            log_table,
        }
    }

    fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            return 0;
        }
        let log_a = u16::from(self.log_table[a as usize]);
        let log_b = u16::from(self.log_table[b as usize]);
        let log_sum = (log_a + log_b) % 255;
        self.exp_table[log_sum as usize]
    }

    fn exp(&self, power: u8) -> u8 {
        self.exp_table[(u16::from(power) % 255) as usize]
    }
}

// ============================================================================
// Reed-Solomon error correction
// ============================================================================

/// Compute Reed-Solomon error correction codewords.
fn rs_encode(data: &[u8], ec_count: usize, gf: &GfTables) -> Vec<u8> {
    // Build generator polynomial
    let gen_poly = rs_generator_poly(ec_count, gf);

    let mut message = Vec::with_capacity(data.len() + ec_count);
    message.extend_from_slice(data);
    message.resize(data.len() + ec_count, 0);

    for i in 0..data.len() {
        let coef = message[i];
        if coef != 0 {
            for (j, &g) in gen_poly.iter().enumerate().skip(1) {
                let idx = i + j;
                if idx < message.len() {
                    message[idx] ^= gf.mul(g, coef);
                }
            }
        }
    }

    // The remainder (EC codewords) is in message[data.len()..]
    message[data.len()..].to_vec()
}

/// Build the generator polynomial for `count` EC codewords.
fn rs_generator_poly(count: usize, gf: &GfTables) -> Vec<u8> {
    let mut poly = vec![1u8];
    for i in 0..count {
        let root = gf.exp(i as u8);
        let mut new_poly = vec![0u8; poly.len() + 1];
        for (j, &coef) in poly.iter().enumerate() {
            let idx_plus = j + 1;
            if idx_plus < new_poly.len() {
                new_poly[idx_plus] ^= coef;
            }
            new_poly[j] ^= gf.mul(coef, root);
        }
        poly = new_poly;
    }
    poly
}

// ============================================================================
// QR Code data types and tables
// ============================================================================

/// Error correction levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcLevel {
    L, // ~7% recovery
    M, // ~15% recovery
    Q, // ~25% recovery
    H, // ~30% recovery
}

impl EcLevel {
    fn label(self) -> &'static str {
        match self {
            Self::L => "L (7%)",
            Self::M => "M (15%)",
            Self::Q => "Q (25%)",
            Self::H => "H (30%)",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Self::L => "L",
            Self::M => "M",
            Self::Q => "Q",
            Self::H => "H",
        }
    }

    fn format_bits(self) -> u8 {
        match self {
            Self::L => 0b01,
            Self::M => 0b00,
            Self::Q => 0b11,
            Self::H => 0b10,
        }
    }

    fn all() -> &'static [EcLevel] {
        &[EcLevel::L, EcLevel::M, EcLevel::Q, EcLevel::H]
    }
}

/// QR version info: (version, `ec_level`) -> (`total_codewords`, `ec_codewords_per_block`, `num_blocks`)
/// Simplified table for versions 1-10.
struct VersionInfo {
    version: u8,
    ec_level: EcLevel,
    data_capacity_bytes: usize,
    ec_codewords_per_block: usize,
    num_blocks: usize,
    total_codewords: usize,
}

/// Get version info for a given version and EC level.
fn get_version_info(version: u8, ec_level: EcLevel) -> Option<VersionInfo> {
    // Table of (version, ec_level, data_cap_bytes, ec_per_block, num_blocks, total_codewords)
    // Data from QR spec for byte mode encoding
    let table: &[(u8, EcLevel, usize, usize, usize, usize)] = &[
        // Version 1
        (1, EcLevel::L, 17, 7, 1, 26),
        (1, EcLevel::M, 14, 10, 1, 26),
        (1, EcLevel::Q, 11, 13, 1, 26),
        (1, EcLevel::H, 7, 17, 1, 26),
        // Version 2
        (2, EcLevel::L, 32, 10, 1, 44),
        (2, EcLevel::M, 26, 16, 1, 44),
        (2, EcLevel::Q, 20, 22, 1, 44),
        (2, EcLevel::H, 14, 28, 1, 44),
        // Version 3
        (3, EcLevel::L, 53, 15, 1, 70),
        (3, EcLevel::M, 42, 26, 1, 70),
        (3, EcLevel::Q, 32, 18, 2, 70),
        (3, EcLevel::H, 24, 22, 2, 70),
        // Version 4
        (4, EcLevel::L, 78, 20, 1, 100),
        (4, EcLevel::M, 62, 18, 2, 100),
        (4, EcLevel::Q, 46, 26, 2, 100),
        (4, EcLevel::H, 34, 16, 4, 100),
        // Version 5
        (5, EcLevel::L, 106, 26, 1, 134),
        (5, EcLevel::M, 84, 24, 2, 134),
        (5, EcLevel::Q, 60, 18, 2, 134),
        (5, EcLevel::H, 44, 22, 2, 134),
        // Version 6
        (6, EcLevel::L, 134, 18, 2, 172),
        (6, EcLevel::M, 106, 16, 4, 172),
        (6, EcLevel::Q, 74, 24, 2, 172),
        (6, EcLevel::H, 58, 28, 2, 172),
        // Version 7
        (7, EcLevel::L, 154, 20, 2, 196),
        (7, EcLevel::M, 122, 18, 4, 196),
        (7, EcLevel::Q, 86, 18, 2, 196),
        (7, EcLevel::H, 64, 26, 2, 196),
        // Version 8
        (8, EcLevel::L, 192, 24, 2, 242),
        (8, EcLevel::M, 152, 22, 2, 242),
        (8, EcLevel::Q, 108, 22, 2, 242),
        (8, EcLevel::H, 84, 26, 2, 242),
        // Version 9
        (9, EcLevel::L, 230, 30, 2, 292),
        (9, EcLevel::M, 180, 22, 3, 292),
        (9, EcLevel::Q, 130, 20, 3, 292),
        (9, EcLevel::H, 98, 24, 3, 292),
        // Version 10
        (10, EcLevel::L, 271, 18, 2, 346),
        (10, EcLevel::M, 213, 26, 2, 346),
        (10, EcLevel::Q, 151, 24, 3, 346),
        (10, EcLevel::H, 119, 28, 3, 346),
    ];

    for &(v, ec, dc, ecpb, nb, tc) in table {
        if v == version && ec == ec_level {
            return Some(VersionInfo {
                version: v,
                ec_level: ec,
                data_capacity_bytes: dc,
                ec_codewords_per_block: ecpb,
                num_blocks: nb,
                total_codewords: tc,
            });
        }
    }
    None
}

/// Select the smallest version that can hold the given number of data bytes.
fn select_version(data_len: usize, ec_level: EcLevel) -> Option<u8> {
    for v in 1..=10 {
        if let Some(info) = get_version_info(v, ec_level)
            && info.data_capacity_bytes >= data_len
        {
            return Some(v);
        }
    }
    None
}

/// Get the size of a QR code (modules per side) for a version.
fn qr_size(version: u8) -> usize {
    // Version 1 = 21x21, each version adds 4
    17_usize.saturating_add(4_usize.saturating_mul(version as usize))
}

// ============================================================================
// QR Code matrix construction
// ============================================================================

/// Module state in the QR matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Module {
    /// Not yet assigned.
    Empty,
    /// Function pattern (finder, timing, etc) - dark.
    FunctionDark,
    /// Function pattern - light.
    FunctionLight,
    /// Data/EC bit - dark.
    DataDark,
    /// Data/EC bit - light.
    DataLight,
}

impl Module {
    fn is_dark(self) -> bool {
        matches!(self, Self::FunctionDark | Self::DataDark)
    }

    fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }

    fn is_function(self) -> bool {
        matches!(self, Self::FunctionDark | Self::FunctionLight)
    }
}

/// A QR code matrix.
#[derive(Clone, Debug)]
pub struct QrMatrix {
    size: usize,
    modules: Vec<Module>,
}

impl QrMatrix {
    fn new(size: usize) -> Self {
        Self {
            size,
            modules: vec![Module::Empty; size.saturating_mul(size)],
        }
    }

    fn get(&self, row: usize, col: usize) -> Module {
        if row < self.size && col < self.size {
            self.modules
                .get(row.saturating_mul(self.size).saturating_add(col))
                .copied()
                .unwrap_or(Module::Empty)
        } else {
            Module::Empty
        }
    }

    fn set(&mut self, row: usize, col: usize, val: Module) {
        if row < self.size && col < self.size {
            let idx = row.saturating_mul(self.size).saturating_add(col);
            if let Some(cell) = self.modules.get_mut(idx) {
                *cell = val;
            }
        }
    }

    fn is_dark(&self, row: usize, col: usize) -> bool {
        self.get(row, col).is_dark()
    }

    /// Place finder pattern with top-left at (row, col).
    fn place_finder_pattern(&mut self, row: usize, col: usize) {
        for r in 0..7 {
            for c in 0..7 {
                let dark = r == 0
                    || r == 6
                    || c == 0
                    || c == 6
                    || ((2..=4).contains(&r) && (2..=4).contains(&c));
                let module = if dark {
                    Module::FunctionDark
                } else {
                    Module::FunctionLight
                };
                self.set(row.saturating_add(r), col.saturating_add(c), module);
            }
        }
    }

    /// Place separator (light) around a finder pattern.
    fn place_separator(&mut self, row: usize, col: usize, h_dir: i32, v_dir: i32) {
        // Horizontal separator
        for c in 0..8 {
            let sr = if v_dir > 0 {
                row.saturating_add(7)
            } else {
                row.saturating_sub(1)
            };
            let sc = if h_dir > 0 {
                col.saturating_add(c)
            } else {
                col.wrapping_add(c).wrapping_sub(1)
            };
            if sr < self.size && sc < self.size {
                self.set(sr, sc, Module::FunctionLight);
            }
        }
        // Vertical separator
        for r in 0..8 {
            let sr = if v_dir > 0 {
                row.saturating_add(r)
            } else {
                row.wrapping_add(r).wrapping_sub(1)
            };
            let sc = if h_dir > 0 {
                col.saturating_add(7)
            } else {
                col.saturating_sub(1)
            };
            if sr < self.size && sc < self.size {
                self.set(sr, sc, Module::FunctionLight);
            }
        }
    }

    /// Place timing patterns (row 6 and column 6).
    fn place_timing_patterns(&mut self) {
        for i in 8..self.size.saturating_sub(8) {
            let module = if i % 2 == 0 {
                Module::FunctionDark
            } else {
                Module::FunctionLight
            };
            if self.get(6, i).is_empty() {
                self.set(6, i, module);
            }
            if self.get(i, 6).is_empty() {
                self.set(i, 6, module);
            }
        }
    }

    /// Place the dark module (always present at (4*version+9, 8)).
    fn place_dark_module(&mut self, version: u8) {
        let row = 4_usize.saturating_mul(version as usize).saturating_add(9);
        if row < self.size {
            self.set(row, 8, Module::FunctionDark);
        }
    }

    /// Reserve format information areas (they'll be written after masking).
    fn reserve_format_info(&mut self) {
        // Around top-left finder
        for i in 0..9 {
            if i < self.size {
                if self.get(8, i).is_empty() {
                    self.set(8, i, Module::FunctionLight);
                }
                if self.get(i, 8).is_empty() {
                    self.set(i, 8, Module::FunctionLight);
                }
            }
        }
        // Around bottom-left finder
        for i in 0..8 {
            let row = self.size.saturating_sub(1).saturating_sub(i);
            if self.get(row, 8).is_empty() {
                self.set(row, 8, Module::FunctionLight);
            }
        }
        // Around top-right finder
        for i in 0..8 {
            let col = self.size.saturating_sub(8).saturating_add(i);
            if self.get(8, col).is_empty() {
                self.set(8, col, Module::FunctionLight);
            }
        }
    }

    /// Place alignment pattern centered at (row, col).
    fn place_alignment_pattern(&mut self, center_row: usize, center_col: usize) {
        for r in 0..5 {
            for c in 0..5 {
                let dr = center_row.saturating_add(r).saturating_sub(2);
                let dc = center_col.saturating_add(c).saturating_sub(2);
                if dr < self.size && dc < self.size {
                    let dark = r == 0 || r == 4 || c == 0 || c == 4 || (r == 2 && c == 2);
                    let module = if dark {
                        Module::FunctionDark
                    } else {
                        Module::FunctionLight
                    };
                    // Only place if not already occupied by function pattern
                    if self.get(dr, dc).is_empty() || !self.get(dr, dc).is_function() {
                        self.set(dr, dc, module);
                    }
                }
            }
        }
    }
}

/// Get alignment pattern positions for a version.
fn alignment_positions(version: u8) -> Vec<usize> {
    // Versions 1 has no alignment patterns
    // Versions 2-10 have one alignment pattern
    match version {
        1 => vec![],
        2 => vec![6, 18],
        3 => vec![6, 22],
        4 => vec![6, 26],
        5 => vec![6, 30],
        6 => vec![6, 34],
        7 => vec![6, 22, 38],
        8 => vec![6, 24, 42],
        9 => vec![6, 26, 46],
        10 => vec![6, 28, 52],
        _ => vec![],
    }
}

// ============================================================================
// QR Code encoding
// ============================================================================

/// Encode data into QR code byte-mode data stream.
fn encode_data_bits(data: &[u8], version: u8, ec_level: EcLevel) -> Option<Vec<u8>> {
    let info = get_version_info(version, ec_level)?;

    // Mode indicator: 0100 (byte mode)
    // Character count indicator: 8 bits for versions 1-9, 16 bits for versions 10+
    let count_bits = if version <= 9 { 8 } else { 16 };

    let mut bits = BitWriter::new();

    // Mode indicator: byte mode = 0b0100
    bits.write_bits(0b0100, 4);

    // Character count
    bits.write_bits(data.len() as u32, count_bits);

    // Data bytes
    for &byte in data {
        bits.write_bits(u32::from(byte), 8);
    }

    // Terminator (up to 4 zero bits)
    let total_data_bits = info
        .total_codewords
        .saturating_sub(info.ec_codewords_per_block.saturating_mul(info.num_blocks))
        .saturating_mul(8);
    let remaining = total_data_bits.saturating_sub(bits.len());
    let terminator = remaining.min(4);
    bits.write_bits(0, terminator);

    // Pad to byte boundary
    let pad_to_byte = (8_usize.saturating_sub(bits.len() % 8)) % 8;
    bits.write_bits(0, pad_to_byte);

    // Pad with 0xEC, 0x11 alternating
    let target_bytes = total_data_bits / 8;
    let mut pad_toggle = false;
    while bits.len() / 8 < target_bytes {
        bits.write_bits(if pad_toggle { 0x11 } else { 0xEC }, 8);
        pad_toggle = !pad_toggle;
    }

    Some(bits.to_bytes())
}

/// Apply error correction and interleave blocks.
fn apply_error_correction(
    data_codewords: &[u8],
    version: u8,
    ec_level: EcLevel,
) -> Option<Vec<u8>> {
    let info = get_version_info(version, ec_level)?;
    let gf = GfTables::new();

    let ec_per_block = info.ec_codewords_per_block;
    let num_blocks = info.num_blocks;
    let total_data = data_codewords.len();
    let base_block_size = total_data / num_blocks;
    let extra_blocks = total_data % num_blocks;

    // Split data into blocks
    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(num_blocks);
    let mut offset: usize = 0;
    for i in 0..num_blocks {
        let block_size = if i < num_blocks.saturating_sub(extra_blocks) {
            base_block_size
        } else {
            base_block_size.saturating_add(1)
        };
        let end = offset.saturating_add(block_size).min(total_data);
        blocks.push(data_codewords[offset..end].to_vec());
        offset = end;
    }

    // Compute EC for each block
    let mut ec_blocks: Vec<Vec<u8>> = Vec::with_capacity(num_blocks);
    for block in &blocks {
        ec_blocks.push(rs_encode(block, ec_per_block, &gf));
    }

    // Interleave data codewords
    let max_data_len = blocks.iter().map(Vec::len).max().unwrap_or(0);
    let mut result = Vec::with_capacity(info.total_codewords);
    for i in 0..max_data_len {
        for block in &blocks {
            if let Some(&byte) = block.get(i) {
                result.push(byte);
            }
        }
    }

    // Interleave EC codewords
    for i in 0..ec_per_block {
        for ec_block in &ec_blocks {
            if let Some(&byte) = ec_block.get(i) {
                result.push(byte);
            }
        }
    }

    Some(result)
}

// ============================================================================
// Bit writer utility
// ============================================================================

struct BitWriter {
    data: Vec<u8>,
    bit_count: usize,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            bit_count: 0,
        }
    }

    fn write_bits(&mut self, value: u32, num_bits: usize) {
        for i in (0..num_bits).rev() {
            let bit = (value >> i) & 1;
            let byte_idx = self.bit_count / 8;
            let bit_idx = 7_usize.saturating_sub(self.bit_count % 8);

            if byte_idx >= self.data.len() {
                self.data.push(0);
            }
            if let Some(byte) = self.data.get_mut(byte_idx)
                && bit == 1
            {
                *byte |= 1u8 << bit_idx;
            }
            self.bit_count = self.bit_count.saturating_add(1);
        }
    }

    fn len(&self) -> usize {
        self.bit_count
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.data.clone()
    }
}

// ============================================================================
// Data placement and masking
// ============================================================================

/// Place data bits into the QR matrix using the upward-column zigzag pattern.
fn place_data_bits(matrix: &mut QrMatrix, data: &[u8]) {
    let size = matrix.size;
    let mut bit_idx: usize = 0;
    let total_bits = data.len().saturating_mul(8);

    // Columns go right-to-left in pairs, skipping column 6 (timing)
    let mut col = size.saturating_sub(1);
    let mut going_up = true;

    loop {
        // Skip the vertical timing pattern column
        if col == 6 {
            if col == 0 {
                break;
            }
            col = col.saturating_sub(1);
        }

        let row_iter: Vec<usize> = if going_up {
            (0..size).rev().collect()
        } else {
            (0..size).collect()
        };

        for row in row_iter {
            // Two columns: col and col-1
            for dc in 0..2u8 {
                let actual_col = col.saturating_sub(dc as usize);
                if actual_col < size && matrix.get(row, actual_col).is_empty() {
                    if bit_idx < total_bits {
                        let byte_idx = bit_idx / 8;
                        let bit_offset = 7_usize.saturating_sub(bit_idx % 8);
                        let bit_val = data.get(byte_idx).map_or(0, |b| (b >> bit_offset) & 1);
                        let module = if bit_val == 1 {
                            Module::DataDark
                        } else {
                            Module::DataLight
                        };
                        matrix.set(row, actual_col, module);
                    } else {
                        matrix.set(row, actual_col, Module::DataLight);
                    }
                    bit_idx = bit_idx.saturating_add(1);
                }
            }
        }

        going_up = !going_up;

        if col < 2 {
            break;
        }
        col = col.saturating_sub(2);
    }
}

/// Apply a mask pattern to the matrix (only affects data modules).
fn apply_mask(matrix: &mut QrMatrix, mask_pattern: u8) {
    let size = matrix.size;
    for row in 0..size {
        for col in 0..size {
            let m = matrix.get(row, col);
            if m == Module::DataDark || m == Module::DataLight {
                let should_flip = match mask_pattern {
                    0 => (row + col) % 2 == 0,
                    1 => row % 2 == 0,
                    2 => col % 3 == 0,
                    3 => (row + col) % 3 == 0,
                    4 => (row / 2 + col / 3) % 2 == 0,
                    5 => (row * col) % 2 + (row * col) % 3 == 0,
                    6 => ((row * col) % 2 + (row * col) % 3) % 2 == 0,
                    7 => ((row + col) % 2 + (row * col) % 3) % 2 == 0,
                    _ => false,
                };
                if should_flip {
                    let new_m = if m == Module::DataDark {
                        Module::DataLight
                    } else {
                        Module::DataDark
                    };
                    matrix.set(row, col, new_m);
                }
            }
        }
    }
}

/// Evaluate a masked matrix for penalty score (lower is better).
fn evaluate_penalty(matrix: &QrMatrix) -> u32 {
    let size = matrix.size;
    let mut penalty: u32 = 0;

    // Rule 1: Runs of same color (5+ consecutive same-colored modules)
    for row in 0..size {
        let mut run = 1u32;
        for col in 1..size {
            if matrix.is_dark(row, col) == matrix.is_dark(row, col.saturating_sub(1)) {
                run = run.saturating_add(1);
            } else {
                if run >= 5 {
                    penalty = penalty.saturating_add(run.saturating_sub(2));
                }
                run = 1;
            }
        }
        if run >= 5 {
            penalty = penalty.saturating_add(run.saturating_sub(2));
        }
    }

    for col in 0..size {
        let mut run = 1u32;
        for row in 1..size {
            if matrix.is_dark(row, col) == matrix.is_dark(row.saturating_sub(1), col) {
                run = run.saturating_add(1);
            } else {
                if run >= 5 {
                    penalty = penalty.saturating_add(run.saturating_sub(2));
                }
                run = 1;
            }
        }
        if run >= 5 {
            penalty = penalty.saturating_add(run.saturating_sub(2));
        }
    }

    // Rule 2: 2x2 blocks of same color
    for row in 0..size.saturating_sub(1) {
        for col in 0..size.saturating_sub(1) {
            let d = matrix.is_dark(row, col);
            if d == matrix.is_dark(row, col + 1)
                && d == matrix.is_dark(row + 1, col)
                && d == matrix.is_dark(row + 1, col + 1)
            {
                penalty = penalty.saturating_add(3);
            }
        }
    }

    // Rule 3: Finder-like patterns (1011101)
    for row in 0..size {
        for col in 0..size.saturating_sub(6) {
            if check_finder_like(matrix, row, col, true) {
                penalty = penalty.saturating_add(40);
            }
        }
    }
    for col in 0..size {
        for row in 0..size.saturating_sub(6) {
            if check_finder_like(matrix, row, col, false) {
                penalty = penalty.saturating_add(40);
            }
        }
    }

    // Rule 4: Proportion of dark modules
    let total = (size * size) as u32;
    let mut dark_count: u32 = 0;
    for row in 0..size {
        for col in 0..size {
            if matrix.is_dark(row, col) {
                dark_count = dark_count.saturating_add(1);
            }
        }
    }
    let percentage = dark_count.saturating_mul(100) / total.max(1);
    let prev_five = (percentage / 5).saturating_mul(5);
    let next_five = prev_five.saturating_add(5);
    let dev_prev = prev_five.abs_diff(50);
    let dev_next = next_five.abs_diff(50);
    let min_dev = dev_prev.min(dev_next);
    penalty = penalty.saturating_add(min_dev.saturating_mul(2));

    penalty
}

/// Check for finder-like pattern (1011101 0000 or 0000 1011101).
fn check_finder_like(matrix: &QrMatrix, row: usize, col: usize, horizontal: bool) -> bool {
    let pattern: [bool; 7] = [true, false, true, true, true, false, true];
    for (i, &expected) in pattern.iter().enumerate() {
        let dark = if horizontal {
            matrix.is_dark(row, col.saturating_add(i))
        } else {
            matrix.is_dark(row.saturating_add(i), col)
        };
        if dark != expected {
            return false;
        }
    }
    true
}

/// Write format information into the matrix.
fn write_format_info(matrix: &mut QrMatrix, ec_level: EcLevel, mask_pattern: u8) {
    let format_data = (u16::from(ec_level.format_bits()) << 3) | u16::from(mask_pattern);
    let format_ecc = format_info_ecc(format_data);
    let format_bits = (u32::from(format_data) << 10) | u32::from(format_ecc);
    // XOR with mask pattern 101010000010010
    let format_bits = format_bits ^ 0x5412;

    let size = matrix.size;

    // Place format bits around top-left finder
    for i in 0..15 {
        let bit = ((format_bits >> (14u32.saturating_sub(i as u32))) & 1) == 1;
        let module = if bit {
            Module::FunctionDark
        } else {
            Module::FunctionLight
        };

        // Horizontal placement (row 8)
        let col = match i {
            0..=5 => i,
            6 => 7,
            7 => 8,
            _ => size.saturating_sub(15).saturating_add(i),
        };
        matrix.set(8, col, module);

        // Vertical placement (column 8)
        let row = match i {
            0 | 1 => size.saturating_sub(1).saturating_sub(i),
            2..=5 => size.saturating_sub(1).saturating_sub(i),
            6 => size.saturating_sub(1).saturating_sub(i),
            7 => 8_usize.saturating_sub(i.saturating_sub(7)),
            i_val => {
                let offset = 14_usize.saturating_sub(i_val);
                if offset > 5 {
                    offset.saturating_add(1)
                } else {
                    offset
                }
            }
        };
        if row < size {
            matrix.set(row, 8, module);
        }
    }
}

/// Compute format information ECC (BCH code).
fn format_info_ecc(data: u16) -> u16 {
    let mut remainder = u32::from(data) << 10;
    let generator: u32 = 0b10100110111;

    for i in (0..=4).rev() {
        if remainder & (1 << (i + 10)) != 0 {
            remainder ^= generator << i;
        }
    }

    remainder as u16
}

// ============================================================================
// Full QR code generation
// ============================================================================

/// A generated QR code.
#[derive(Clone, Debug)]
pub struct QrCode {
    pub matrix: QrMatrix,
    pub version: u8,
    pub ec_level: EcLevel,
    pub mask_pattern: u8,
    pub data_len: usize,
}

impl QrCode {
    /// Generate a QR code from the given data bytes.
    pub fn encode(data: &[u8], ec_level: EcLevel) -> Option<Self> {
        if data.is_empty() {
            return None;
        }

        let version = select_version(data.len(), ec_level)?;
        let size = qr_size(version);

        // Encode data bits
        let data_codewords = encode_data_bits(data, version, ec_level)?;

        // Apply error correction
        let final_data = apply_error_correction(&data_codewords, version, ec_level)?;

        // Build matrix with function patterns
        let mut base_matrix = QrMatrix::new(size);

        // Place finder patterns
        base_matrix.place_finder_pattern(0, 0); // top-left
        base_matrix.place_finder_pattern(0, size.saturating_sub(7)); // top-right
        base_matrix.place_finder_pattern(size.saturating_sub(7), 0); // bottom-left

        // Place separators
        // Top-left
        for i in 0..8 {
            if 7 < size {
                base_matrix.set(7, i, Module::FunctionLight);
                base_matrix.set(i, 7, Module::FunctionLight);
            }
        }
        // Top-right
        for i in 0..8 {
            let col_start = size.saturating_sub(8);
            base_matrix.set(7, col_start.saturating_add(i), Module::FunctionLight);
            if i < 7 {
                base_matrix.set(i, col_start, Module::FunctionLight);
            }
        }
        // Bottom-left
        for i in 0..8 {
            let row_start = size.saturating_sub(8);
            base_matrix.set(row_start, i, Module::FunctionLight);
            if i < 7 {
                let r = row_start.saturating_add(1).saturating_add(i);
                if r < size {
                    base_matrix.set(r, 7, Module::FunctionLight);
                }
            }
        }

        // Place alignment patterns
        let positions = alignment_positions(version);
        for &r in &positions {
            for &c in &positions {
                // Skip if overlapping with finder patterns
                let overlaps_finder = (r <= 8 && (c <= 8 || c >= size.saturating_sub(8)))
                    || (r >= size.saturating_sub(8) && c <= 8);
                if !overlaps_finder {
                    base_matrix.place_alignment_pattern(r, c);
                }
            }
        }

        // Place timing patterns
        base_matrix.place_timing_patterns();

        // Place dark module
        base_matrix.place_dark_module(version);

        // Reserve format info
        base_matrix.reserve_format_info();

        // Place data bits
        place_data_bits(&mut base_matrix, &final_data);

        // Try all 8 mask patterns and pick the best
        let mut best_mask = 0u8;
        let mut best_penalty = u32::MAX;

        for mask in 0..8u8 {
            let mut candidate = base_matrix.clone();
            apply_mask(&mut candidate, mask);
            write_format_info(&mut candidate, ec_level, mask);
            let penalty = evaluate_penalty(&candidate);
            if penalty < best_penalty {
                best_penalty = penalty;
                best_mask = mask;
            }
        }

        // Apply best mask
        apply_mask(&mut base_matrix, best_mask);
        write_format_info(&mut base_matrix, ec_level, best_mask);

        Some(QrCode {
            matrix: base_matrix,
            version,
            ec_level,
            mask_pattern: best_mask,
            data_len: data.len(),
        })
    }

    /// Get the size of the QR code (modules per side).
    pub fn size(&self) -> usize {
        self.matrix.size
    }

    /// Check if a module at (row, col) is dark.
    pub fn is_dark(&self, row: usize, col: usize) -> bool {
        self.matrix.is_dark(row, col)
    }
}

// ============================================================================
// Code128 barcode generation
// ============================================================================

/// Code128 character set B values and patterns.
/// Each pattern is a sequence of bar/space widths (bars are odd indices, spaces even).
const CODE128_PATTERNS: &[[u8; 6]] = &[
    [2, 1, 2, 2, 2, 2], // 0: space
    [2, 2, 2, 1, 2, 2], // 1: !
    [2, 2, 2, 2, 2, 1], // 2: "
    [1, 2, 1, 2, 2, 3], // 3: #
    [1, 2, 1, 3, 2, 2], // 4: $
    [1, 3, 1, 2, 2, 2], // 5: %
    [1, 2, 2, 2, 1, 3], // 6: &
    [1, 2, 2, 3, 1, 2], // 7: '
    [1, 3, 2, 2, 1, 2], // 8: (
    [2, 2, 1, 2, 1, 3], // 9: )
    [2, 2, 1, 3, 1, 2], // 10: *
    [2, 3, 1, 2, 1, 2], // 11: +
    [1, 1, 2, 2, 3, 2], // 12: ,
    [1, 2, 2, 1, 3, 2], // 13: -
    [1, 2, 2, 2, 3, 1], // 14: .
    [1, 1, 3, 2, 2, 2], // 15: /
    [1, 2, 3, 1, 2, 2], // 16: 0
    [1, 2, 3, 2, 2, 1], // 17: 1
    [2, 2, 3, 2, 1, 1], // 18: 2
    [2, 2, 1, 1, 3, 2], // 19: 3
    [2, 2, 1, 2, 3, 1], // 20: 4
    [2, 1, 3, 2, 1, 2], // 21: 5
    [2, 2, 3, 1, 1, 2], // 22: 6
    [3, 1, 2, 1, 3, 1], // 23: 7
    [3, 1, 1, 2, 2, 2], // 24: 8
    [3, 2, 1, 1, 2, 2], // 25: 9
    [3, 2, 1, 2, 2, 1], // 26: :
    [3, 1, 2, 2, 1, 2], // 27: ;
    [3, 2, 2, 1, 1, 2], // 28: <
    [3, 2, 2, 2, 1, 1], // 29: =
    [2, 1, 2, 1, 2, 3], // 30: >
    [2, 1, 2, 3, 2, 1], // 31: ?
    [2, 3, 2, 1, 2, 1], // 32: @
    [1, 1, 1, 3, 2, 3], // 33: A
    [1, 3, 1, 1, 2, 3], // 34: B
    [1, 3, 1, 3, 2, 1], // 35: C
    [1, 1, 2, 3, 2, 2], // 36: D (originally index 36, char D)
    [1, 3, 2, 1, 2, 2], // 37: E (originally index 37)
    [1, 3, 2, 3, 2, 0], // 38: F -- placeholder, widths adjusted
    [2, 1, 1, 3, 1, 3], // 39: G
    [2, 3, 1, 1, 1, 3], // 40: H
    [2, 3, 1, 3, 1, 1], // 41: I
    [1, 1, 2, 1, 3, 3], // 42: J
    [1, 1, 2, 3, 3, 1], // 43: K
    [1, 3, 2, 1, 3, 1], // 44: L
    [1, 1, 3, 1, 2, 3], // 45: M
    [1, 1, 3, 3, 2, 1], // 46: N
    [1, 3, 3, 1, 2, 1], // 47: O
    [3, 1, 3, 1, 2, 1], // 48: P
    [2, 1, 1, 3, 3, 1], // 49: Q
    [2, 3, 1, 1, 3, 1], // 50: R
    [2, 1, 3, 1, 1, 3], // 51: S
    [2, 1, 3, 3, 1, 1], // 52: T
    [2, 1, 3, 1, 3, 1], // 53: U
    [3, 1, 1, 1, 2, 3], // 54: V
    [3, 1, 1, 3, 2, 1], // 55: W
    [3, 3, 1, 1, 2, 1], // 56: X
    [3, 1, 2, 1, 1, 3], // 57: Y
    [3, 1, 2, 3, 1, 1], // 58: Z
    [3, 3, 2, 1, 1, 1], // 59: [
    [2, 1, 1, 2, 1, 4], // 60: backslash
    [2, 1, 1, 4, 1, 2], // 61: ]
    [4, 1, 1, 2, 1, 2], // 62: ^
    [2, 4, 1, 2, 1, 1], // 63: _
    [2, 2, 1, 1, 1, 4], // 64: NUL / ` in B
    [4, 1, 2, 1, 1, 2], // 65: a (Code B value 65)
    [4, 2, 1, 1, 1, 2], // 66: b
    [2, 1, 2, 1, 4, 1], // 67: c
    [2, 1, 4, 1, 2, 1], // 68: d
    [4, 1, 2, 1, 2, 1], // 69: e
    [1, 1, 1, 1, 4, 3], // 70: f
    [1, 1, 1, 3, 4, 1], // 71: g
    [4, 1, 1, 1, 1, 3], // 72: h (placeholder)
    [1, 1, 4, 1, 1, 3], // 73: i
    [1, 1, 4, 3, 1, 1], // 74: j
    [4, 1, 1, 1, 3, 1], // 75: k
    [1, 1, 3, 1, 4, 1], // 76: l
    [1, 1, 4, 1, 3, 1], // 77: m
    [3, 1, 1, 1, 4, 1], // 78: n
    [4, 1, 1, 1, 3, 1], // 79: o  (duplicate width check, placeholder)
    [2, 1, 1, 4, 1, 2], // 80: p
    [1, 2, 1, 1, 2, 4], // 81: q (placeholder)
    [1, 4, 1, 1, 2, 2], // 82: r
    [1, 4, 1, 2, 2, 1], // 83: s
    [1, 2, 2, 4, 1, 1], // 84: t
    [1, 2, 4, 2, 1, 1], // 85: u
    [1, 4, 2, 2, 1, 1], // 86: v
    [4, 1, 2, 2, 1, 1], // 87: w
    [4, 2, 2, 1, 1, 1], // 88: x
    [2, 1, 2, 1, 1, 4], // 89: y
    [2, 1, 1, 1, 2, 4], // 90: z
    [1, 3, 4, 1, 1, 1], // 91: {
    [1, 1, 1, 2, 4, 2], // 92: |
    [1, 2, 1, 1, 4, 2], // 93: }
    [1, 2, 1, 2, 4, 1], // 94: ~
    [1, 1, 4, 2, 1, 2], // 95: DEL
    [1, 2, 4, 1, 1, 2], // 96: FNC3
    [1, 2, 4, 2, 1, 1], // 97: FNC2
    [2, 4, 2, 1, 1, 1], // 98: SHIFT
    [2, 2, 4, 1, 1, 1], // 99: CODE_C
    [1, 1, 1, 1, 4, 3], // 100: CODE_B (FNC4 in A)
    [1, 1, 1, 3, 4, 1], // 101: CODE_A (FNC4 in B)
    [1, 3, 1, 1, 4, 1], // 102: FNC1
    [2, 1, 1, 4, 1, 2], // 103: START_A
    [2, 1, 1, 2, 1, 4], // 104: START_B
    [2, 1, 1, 2, 3, 2], // 105: START_C
];

/// Stop pattern for Code128 (7 modules wide).
const CODE128_STOP: [u8; 7] = [2, 3, 3, 1, 1, 1, 2];

/// A generated Code128 barcode.
#[derive(Clone, Debug)]
pub struct Code128Barcode {
    /// Bar pattern: true = black bar, false = space.
    pub bars: Vec<bool>,
    pub data: String,
}

impl Code128Barcode {
    /// Encode a string as a Code128 barcode (Code Set B for ASCII 32-127).
    pub fn encode(data: &str) -> Option<Self> {
        if data.is_empty() {
            return None;
        }

        let start_code = 104u32; // START B
        let mut values: Vec<u32> = vec![start_code];

        // Convert characters to Code B values
        for ch in data.chars() {
            let ascii_val = ch as u32;
            if !(32..=127).contains(&ascii_val) {
                continue; // Skip non-printable for Code B
            }
            values.push(ascii_val.saturating_sub(32));
        }

        if values.len() <= 1 {
            return None;
        }

        // Calculate checksum
        let mut checksum: u32 = start_code;
        for (i, &val) in values.iter().enumerate().skip(1) {
            checksum = checksum.saturating_add(val.saturating_mul(i as u32));
        }
        checksum %= 103;
        values.push(checksum);

        // Convert to bars
        let mut bars = Vec::new();

        // Quiet zone
        bars.extend(core::iter::repeat_n(false, 10));

        for &val in &values {
            if let Some(pattern) = CODE128_PATTERNS.get(val as usize) {
                for (idx, &width) in pattern.iter().enumerate() {
                    let is_bar = idx % 2 == 0; // Even indices are bars
                    for _ in 0..width {
                        bars.push(is_bar);
                    }
                }
            }
        }

        // Stop pattern
        for (idx, &width) in CODE128_STOP.iter().enumerate() {
            let is_bar = idx % 2 == 0;
            for _ in 0..width {
                bars.push(is_bar);
            }
        }

        // Quiet zone
        bars.extend(core::iter::repeat_n(false, 10));

        Some(Code128Barcode {
            bars,
            data: data.to_owned(),
        })
    }

    /// Get the total width in modules.
    pub fn width(&self) -> usize {
        self.bars.len()
    }
}

// ============================================================================
// Input modes and formatting
// ============================================================================

/// Input mode for generating QR content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Text,
    Url,
    Email,
    Phone,
    Wifi,
    VCard,
}

impl InputMode {
    fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Url => "URL",
            Self::Email => "Email",
            Self::Phone => "Phone",
            Self::Wifi => "WiFi",
            Self::VCard => "vCard",
        }
    }

    fn all() -> &'static [InputMode] {
        &[
            InputMode::Text,
            InputMode::Url,
            InputMode::Email,
            InputMode::Phone,
            InputMode::Wifi,
            InputMode::VCard,
        ]
    }
}

/// `WiFi` configuration for QR encoding.
#[derive(Clone, Debug)]
pub struct WifiConfig {
    pub ssid: String,
    pub password: String,
    pub encryption: WifiEncryption,
    pub hidden: bool,
}

impl Default for WifiConfig {
    fn default() -> Self {
        Self {
            ssid: String::new(),
            password: String::new(),
            encryption: WifiEncryption::Wpa,
            hidden: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiEncryption {
    None,
    Wep,
    Wpa,
}

impl WifiEncryption {
    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Wep => "WEP",
            Self::Wpa => "WPA/WPA2",
        }
    }
}

/// vCard contact information.
#[derive(Clone, Debug, Default)]
pub struct VCardInfo {
    pub first_name: String,
    pub last_name: String,
    pub phone: String,
    pub email: String,
    pub organization: String,
}

/// Format data according to input mode.
fn format_qr_data(mode: InputMode, text: &str, wifi: &WifiConfig, vcard: &VCardInfo) -> String {
    match mode {
        InputMode::Text => text.to_owned(),
        InputMode::Url => {
            if text.starts_with("http://") || text.starts_with("https://") {
                text.to_owned()
            } else {
                format!("https://{text}")
            }
        }
        InputMode::Email => format!("mailto:{text}"),
        InputMode::Phone => format!("tel:{text}"),
        InputMode::Wifi => {
            let enc = match wifi.encryption {
                WifiEncryption::None => "nopass",
                WifiEncryption::Wep => "WEP",
                WifiEncryption::Wpa => "WPA",
            };
            let hidden = if wifi.hidden { "H:true" } else { "" };
            format!(
                "WIFI:T:{enc};S:{ssid};P:{pw};{hidden};",
                ssid = wifi.ssid,
                pw = wifi.password,
            )
        }
        InputMode::VCard => {
            let mut card = String::from("BEGIN:VCARD\nVERSION:3.0\n");
            card.push_str(&format!("N:{};{}\n", vcard.last_name, vcard.first_name));
            card.push_str(&format!("FN:{} {}\n", vcard.first_name, vcard.last_name));
            if !vcard.phone.is_empty() {
                card.push_str(&format!("TEL:{}\n", vcard.phone));
            }
            if !vcard.email.is_empty() {
                card.push_str(&format!("EMAIL:{}\n", vcard.email));
            }
            if !vcard.organization.is_empty() {
                card.push_str(&format!("ORG:{}\n", vcard.organization));
            }
            card.push_str("END:VCARD");
            card
        }
    }
}

// ============================================================================
// Code type selection
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeType {
    QrCode,
    Barcode128,
}

impl CodeType {
    fn label(self) -> &'static str {
        match self {
            Self::QrCode => "QR Code",
            Self::Barcode128 => "Code128",
        }
    }
}

// ============================================================================
// Module size presets
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleSize {
    Small,
    Medium,
    Large,
}

impl ModuleSize {
    fn pixels(self) -> f32 {
        match self {
            Self::Small => 3.0,
            Self::Medium => 5.0,
            Self::Large => 8.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Small => "Small (3px)",
            Self::Medium => "Medium (5px)",
            Self::Large => "Large (8px)",
        }
    }

    fn all() -> &'static [ModuleSize] {
        &[ModuleSize::Small, ModuleSize::Medium, ModuleSize::Large]
    }
}

// ============================================================================
// History
// ============================================================================

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub data: String,
    pub mode: InputMode,
    pub code_type: CodeType,
    pub ec_level: EcLevel,
    pub timestamp: u64,
}

// ============================================================================
// Application state
// ============================================================================

pub struct QrApp {
    pub input_text: String,
    pub input_mode: InputMode,
    pub code_type: CodeType,
    pub ec_level: EcLevel,
    pub module_size: ModuleSize,
    pub fg_color: Color,
    pub bg_color: Color,
    pub wifi_config: WifiConfig,
    pub vcard_info: VCardInfo,
    pub current_qr: Option<QrCode>,
    pub current_barcode: Option<Code128Barcode>,
    pub history: Vec<HistoryEntry>,
    pub error_message: Option<String>,
    pub window_width: f32,
    pub window_height: f32,
    timestamp: u64,
}

impl Default for QrApp {
    fn default() -> Self {
        Self::new()
    }
}

impl QrApp {
    pub fn new() -> Self {
        Self {
            input_text: String::new(),
            input_mode: InputMode::Text,
            code_type: CodeType::QrCode,
            ec_level: EcLevel::M,
            module_size: ModuleSize::Medium,
            fg_color: Color::BLACK,
            bg_color: Color::WHITE,
            wifi_config: WifiConfig::default(),
            vcard_info: VCardInfo::default(),
            current_qr: None,
            current_barcode: None,
            history: Vec::new(),
            error_message: None,
            window_width: 1100.0,
            window_height: 700.0,
            timestamp: 1000,
        }
    }

    fn tick(&mut self) -> u64 {
        self.timestamp = self.timestamp.saturating_add(1);
        self.timestamp
    }

    /// Generate a code from current settings.
    pub fn generate(&mut self) {
        self.error_message = None;

        let data = format_qr_data(
            self.input_mode,
            &self.input_text,
            &self.wifi_config,
            &self.vcard_info,
        );

        if data.is_empty() {
            self.error_message = Some("No input data provided".to_owned());
            return;
        }

        let ts = self.tick();

        match self.code_type {
            CodeType::QrCode => match QrCode::encode(data.as_bytes(), self.ec_level) {
                Some(qr) => {
                    self.current_qr = Some(qr);
                    self.current_barcode = None;
                    self.history.push(HistoryEntry {
                        data: data.clone(),
                        mode: self.input_mode,
                        code_type: self.code_type,
                        ec_level: self.ec_level,
                        timestamp: ts,
                    });
                }
                None => {
                    self.error_message = Some("Data too long for QR version 1-10".to_owned());
                }
            },
            CodeType::Barcode128 => match Code128Barcode::encode(&data) {
                Some(barcode) => {
                    self.current_barcode = Some(barcode);
                    self.current_qr = None;
                    self.history.push(HistoryEntry {
                        data: data.clone(),
                        mode: self.input_mode,
                        code_type: self.code_type,
                        ec_level: self.ec_level,
                        timestamp: ts,
                    });
                }
                None => {
                    self.error_message = Some("Cannot encode data as Code128".to_owned());
                }
            },
        }
    }

    /// Set input text and auto-generate.
    pub fn set_input(&mut self, text: &str) {
        self.input_text = text.to_owned();
    }

    /// Clear history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut cmds, width);
        self.render_status_bar(&mut cmds, width, height);

        let content_y = TOOLBAR_HEIGHT;
        let content_h = height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Left panel: input controls
        self.render_input_panel(&mut cmds, content_y, content_h);

        // Center panel: preview
        let center_x = LEFT_PANEL_WIDTH;
        let center_w = width - LEFT_PANEL_WIDTH - RIGHT_PANEL_WIDTH;
        self.render_preview_panel(&mut cmds, center_x, content_y, center_w, content_h);

        // Right panel: options
        let right_x = width - RIGHT_PANEL_WIDTH;
        self.render_options_panel(&mut cmds, right_x, content_y, RIGHT_PANEL_WIDTH, content_h);

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: TOOLBAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // App title
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "QR Code Generator".to_owned(),
            color: BLUE,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Code type toggle
        let types = [CodeType::QrCode, CodeType::Barcode128];
        let mut tx = 200.0;
        for ct in &types {
            let is_active = *ct == self.code_type;
            let btn_w = ct.label().len() as f32 * 8.0 + 20.0;
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: 8.0,
                width: btn_w,
                height: 24.0,
                color: if is_active { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: 14.0,
                text: ct.label().to_owned(),
                color: if is_active { BLUE } else { SUBTEXT0 },
                font_size: 11.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(btn_w - 16.0),
            });
            tx += btn_w + 4.0;
        }

        // Generate button
        let gen_btn_w = 100.0;
        let gen_btn_x = width - gen_btn_w - 12.0;
        cmds.push(RenderCommand::FillRect {
            x: gen_btn_x,
            y: 8.0,
            width: gen_btn_w,
            height: 24.0,
            color: GREEN,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: gen_btn_x + 16.0,
            y: 14.0,
            text: "Generate".to_owned(),
            color: CRUST,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(gen_btn_w - 24.0),
        });

        // Divider line
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: width,
            y2: TOOLBAR_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        let bar_y = height - STATUS_BAR_HEIGHT;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let status = if let Some(ref err) = self.error_message {
            err.clone()
        } else if let Some(ref qr) = self.current_qr {
            format!(
                "QR v{} | EC: {} | Mask: {} | Size: {}x{} | {} bytes",
                qr.version,
                qr.ec_level.short_label(),
                qr.mask_pattern,
                qr.size(),
                qr.size(),
                qr.data_len,
            )
        } else if let Some(ref bc) = self.current_barcode {
            format!(
                "Code128 | Width: {} modules | Data: {} chars",
                bc.width(),
                bc.data.len()
            )
        } else {
            "Ready — enter data and click Generate".to_owned()
        };

        let status_color = if self.error_message.is_some() {
            RED
        } else {
            SUBTEXT0
        };
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: bar_y + 6.0,
            text: status,
            color: status_color,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });
    }

    fn render_input_panel(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: LEFT_PANEL_WIDTH,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Right border
        cmds.push(RenderCommand::Line {
            x1: LEFT_PANEL_WIDTH,
            y1: y,
            x2: LEFT_PANEL_WIDTH,
            y2: y + height,
            color: SURFACE0,
            width: 1.0,
        });

        let lx = 12.0;
        let max_w = LEFT_PANEL_WIDTH - 24.0;
        let mut cy = y + 12.0;

        // Input mode selector
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "INPUT MODE".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        cy += 18.0;

        let mut mx = lx;
        for mode in InputMode::all() {
            let is_active = *mode == self.input_mode;
            let btn_w = mode.label().len() as f32 * 7.5 + 16.0;
            if mx + btn_w > LEFT_PANEL_WIDTH - 12.0 {
                mx = lx;
                cy += 26.0;
            }
            cmds.push(RenderCommand::FillRect {
                x: mx,
                y: cy,
                width: btn_w,
                height: 22.0,
                color: if is_active { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: mx + 8.0,
                y: cy + 5.0,
                text: mode.label().to_owned(),
                color: if is_active { LAVENDER } else { SUBTEXT0 },
                font_size: 10.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(btn_w - 12.0),
            });
            mx += btn_w + 4.0;
        }
        cy += 34.0;

        // Input field
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "DATA".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        cy += 16.0;

        cmds.push(RenderCommand::FillRect {
            x: lx,
            y: cy,
            width: max_w,
            height: 60.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: lx,
            y: cy,
            width: max_w,
            height: 60.0,
            color: SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let display_text = if self.input_text.is_empty() {
            match self.input_mode {
                InputMode::Text => "Enter text...",
                InputMode::Url => "Enter URL...",
                InputMode::Email => "Enter email address...",
                InputMode::Phone => "Enter phone number...",
                InputMode::Wifi => "Configure below...",
                InputMode::VCard => "Configure below...",
            }
        } else {
            &self.input_text
        };
        let text_color = if self.input_text.is_empty() {
            OVERLAY0
        } else {
            TEXT_COLOR
        };
        cmds.push(RenderCommand::Text {
            x: lx + 8.0,
            y: cy + 8.0,
            text: display_text.to_owned(),
            color: text_color,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w - 16.0),
        });
        cy += 72.0;

        // Mode-specific fields
        match self.input_mode {
            InputMode::Wifi => {
                self.render_wifi_fields(cmds, lx, &mut cy, max_w);
            }
            InputMode::VCard => {
                self.render_vcard_fields(cmds, lx, &mut cy, max_w);
            }
            _ => {}
        }

        // History section
        cy += 12.0;
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: format!("HISTORY ({})", self.history.len()),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        cy += 18.0;

        for entry in self.history.iter().rev().take(8) {
            if cy > y + height - 20.0 {
                break;
            }

            cmds.push(RenderCommand::FillRect {
                x: lx,
                y: cy,
                width: max_w,
                height: 24.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            // Mode indicator
            let mode_color = match entry.mode {
                InputMode::Text => TEXT_COLOR,
                InputMode::Url => BLUE,
                InputMode::Email => PEACH,
                InputMode::Phone => GREEN,
                InputMode::Wifi => TEAL,
                InputMode::VCard => LAVENDER,
            };
            cmds.push(RenderCommand::FillRect {
                x: lx + 4.0,
                y: cy + 8.0,
                width: 8.0,
                height: 8.0,
                color: mode_color,
                corner_radii: CornerRadii::all(4.0),
            });

            // Truncate data for display
            let display = if entry.data.len() > 30 {
                let truncated: String = entry.data.chars().take(27).collect();
                format!("{truncated}...")
            } else {
                entry.data.clone()
            };

            cmds.push(RenderCommand::Text {
                x: lx + 18.0,
                y: cy + 6.0,
                text: display,
                color: SUBTEXT1,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w - 26.0),
            });
            cy += 28.0;
        }
    }

    fn render_wifi_fields(&self, cmds: &mut Vec<RenderCommand>, lx: f32, cy: &mut f32, max_w: f32) {
        let fields = [
            ("SSID", &self.wifi_config.ssid),
            ("Password", &self.wifi_config.password),
        ];

        for (label, value) in &fields {
            cmds.push(RenderCommand::Text {
                x: lx,
                y: *cy,
                text: (*label).to_owned(),
                color: SUBTEXT0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w),
            });
            *cy += 14.0;

            cmds.push(RenderCommand::FillRect {
                x: lx,
                y: *cy,
                width: max_w,
                height: 24.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            let disp = if value.is_empty() {
                format!("Enter {label}...")
            } else {
                (*value).clone()
            };
            let color = if value.is_empty() {
                OVERLAY0
            } else {
                TEXT_COLOR
            };
            cmds.push(RenderCommand::Text {
                x: lx + 8.0,
                y: *cy + 6.0,
                text: disp,
                color,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w - 16.0),
            });
            *cy += 30.0;
        }

        // Encryption selector
        cmds.push(RenderCommand::Text {
            x: lx,
            y: *cy,
            text: format!("Encryption: {}", self.wifi_config.encryption.label()),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w),
        });
        *cy += 18.0;
    }

    fn render_vcard_fields(
        &self,
        cmds: &mut Vec<RenderCommand>,
        lx: f32,
        cy: &mut f32,
        max_w: f32,
    ) {
        let fields = [
            ("First Name", &self.vcard_info.first_name),
            ("Last Name", &self.vcard_info.last_name),
            ("Phone", &self.vcard_info.phone),
            ("Email", &self.vcard_info.email),
            ("Organization", &self.vcard_info.organization),
        ];

        for (label, value) in &fields {
            cmds.push(RenderCommand::Text {
                x: lx,
                y: *cy,
                text: (*label).to_owned(),
                color: SUBTEXT0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w),
            });
            *cy += 14.0;

            cmds.push(RenderCommand::FillRect {
                x: lx,
                y: *cy,
                width: max_w,
                height: 22.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            let disp = if value.is_empty() {
                format!("Enter {label}...")
            } else {
                (*value).clone()
            };
            let color = if value.is_empty() {
                OVERLAY0
            } else {
                TEXT_COLOR
            };
            cmds.push(RenderCommand::Text {
                x: lx + 8.0,
                y: *cy + 5.0,
                text: disp,
                color,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w - 16.0),
            });
            *cy += 26.0;
        }
    }

    fn render_preview_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let lx = x + 12.0;
        let max_w = width - 24.0;
        let mut cy = y + 12.0;

        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "PREVIEW".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        cy += 22.0;

        if let Some(ref qr) = self.current_qr {
            self.render_qr_preview(cmds, qr, lx, cy, max_w, height - 50.0);
        } else if let Some(ref barcode) = self.current_barcode {
            self.render_barcode_preview(cmds, barcode, lx, cy, max_w);
        } else {
            // Placeholder
            let placeholder_h = 200.0;
            let placeholder_w = 200.0;
            let px = lx + (max_w - placeholder_w) / 2.0;
            let py = cy + 40.0;

            cmds.push(RenderCommand::FillRect {
                x: px,
                y: py,
                width: placeholder_w,
                height: placeholder_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: px,
                y: py,
                width: placeholder_w,
                height: placeholder_h,
                color: SURFACE2,
                line_width: 2.0,
                corner_radii: CornerRadii::all(8.0),
            });

            cmds.push(RenderCommand::Text {
                x: px + 30.0,
                y: py + 85.0,
                text: "No code generated".to_owned(),
                color: OVERLAY0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(placeholder_w - 20.0),
            });
            cmds.push(RenderCommand::Text {
                x: px + 20.0,
                y: py + 105.0,
                text: "Enter data and click Generate".to_owned(),
                color: OVERLAY0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(placeholder_w - 20.0),
            });
        }
    }

    fn render_qr_preview(
        &self,
        cmds: &mut Vec<RenderCommand>,
        qr: &QrCode,
        panel_x: f32,
        panel_y: f32,
        panel_w: f32,
        _panel_h: f32,
    ) {
        let module_px = self.module_size.pixels();
        let qr_size = qr.size();
        let quiet_zone = 4; // 4-module quiet zone
        let total_modules = qr_size + quiet_zone * 2;
        let total_px = total_modules as f32 * module_px;

        // Center the QR code in the panel
        let qr_x = panel_x + (panel_w - total_px) / 2.0;
        let qr_y = panel_y + 20.0;

        // Background (quiet zone + code)
        cmds.push(RenderCommand::FillRect {
            x: qr_x,
            y: qr_y,
            width: total_px,
            height: total_px,
            color: self.bg_color,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Draw modules
        for row in 0..qr_size {
            for col in 0..qr_size {
                if qr.is_dark(row, col) {
                    let mx = qr_x + (col + quiet_zone) as f32 * module_px;
                    let my = qr_y + (row + quiet_zone) as f32 * module_px;
                    cmds.push(RenderCommand::FillRect {
                        x: mx,
                        y: my,
                        width: module_px,
                        height: module_px,
                        color: self.fg_color,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }

        // Info below QR
        let info_y = qr_y + total_px + 12.0;
        cmds.push(RenderCommand::Text {
            x: panel_x,
            y: info_y,
            text: format!(
                "Version {} | {} | {}x{} modules",
                qr.version,
                qr.ec_level.label(),
                qr_size,
                qr_size,
            ),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_w),
        });
    }

    fn render_barcode_preview(
        &self,
        cmds: &mut Vec<RenderCommand>,
        barcode: &Code128Barcode,
        panel_x: f32,
        panel_y: f32,
        panel_w: f32,
    ) {
        let bar_width = 2.0_f32;
        let bar_height = 80.0_f32;
        let total_w = barcode.width() as f32 * bar_width;

        let bc_x = panel_x + (panel_w - total_w) / 2.0;
        let bc_y = panel_y + 40.0;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: bc_x - 10.0,
            y: bc_y - 10.0,
            width: total_w + 20.0,
            height: bar_height + 40.0,
            color: self.bg_color,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Draw bars
        for (i, &is_bar) in barcode.bars.iter().enumerate() {
            if is_bar {
                cmds.push(RenderCommand::FillRect {
                    x: bc_x + i as f32 * bar_width,
                    y: bc_y,
                    width: bar_width,
                    height: bar_height,
                    color: self.fg_color,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        // Data text below barcode
        cmds.push(RenderCommand::Text {
            x: bc_x,
            y: bc_y + bar_height + 8.0,
            text: barcode.data.clone(),
            color: Color::BLACK,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(total_w),
        });

        // Info
        let info_y = bc_y + bar_height + 40.0;
        cmds.push(RenderCommand::Text {
            x: panel_x,
            y: info_y,
            text: format!("Code128 | {} modules wide", barcode.width()),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_w),
        });
    }

    fn render_options_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Left border
        cmds.push(RenderCommand::Line {
            x1: x,
            y1: y,
            x2: x,
            y2: y + height,
            color: SURFACE0,
            width: 1.0,
        });

        let lx = x + 12.0;
        let max_w = width - 24.0;
        let mut cy = y + 12.0;

        // Error correction
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "ERROR CORRECTION".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        cy += 18.0;

        for ec in EcLevel::all() {
            let is_active = *ec == self.ec_level;
            cmds.push(RenderCommand::FillRect {
                x: lx,
                y: cy,
                width: max_w,
                height: 22.0,
                color: if is_active { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: lx + 8.0,
                y: cy + 5.0,
                text: ec.label().to_owned(),
                color: if is_active { GREEN } else { SUBTEXT0 },
                font_size: 10.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(max_w - 16.0),
            });
            cy += 26.0;
        }

        // Module size
        cy += 8.0;
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "MODULE SIZE".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        cy += 18.0;

        for ms in ModuleSize::all() {
            let is_active = *ms == self.module_size;
            cmds.push(RenderCommand::FillRect {
                x: lx,
                y: cy,
                width: max_w,
                height: 22.0,
                color: if is_active { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: lx + 8.0,
                y: cy + 5.0,
                text: ms.label().to_owned(),
                color: if is_active { YELLOW } else { SUBTEXT0 },
                font_size: 10.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(max_w - 16.0),
            });
            cy += 26.0;
        }

        // Colors
        cy += 8.0;
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "COLORS".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        cy += 18.0;

        // Foreground color swatch
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "Foreground".to_owned(),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w - 30.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: lx + max_w - 24.0,
            y: cy - 1.0,
            width: 20.0,
            height: 14.0,
            color: self.fg_color,
            corner_radii: CornerRadii::all(2.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: lx + max_w - 24.0,
            y: cy - 1.0,
            width: 20.0,
            height: 14.0,
            color: SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });
        cy += 20.0;

        // Background color swatch
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "Background".to_owned(),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w - 30.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: lx + max_w - 24.0,
            y: cy - 1.0,
            width: 20.0,
            height: 14.0,
            color: self.bg_color,
            corner_radii: CornerRadii::all(2.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: lx + max_w - 24.0,
            y: cy - 1.0,
            width: 20.0,
            height: 14.0,
            color: SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });
        cy += 24.0;

        // QR info section
        cy += 8.0;
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "INFO".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        cy += 18.0;

        if let Some(ref qr) = self.current_qr {
            let info_lines = [
                format!("Version: {}", qr.version),
                format!("Size: {}x{}", qr.size(), qr.size()),
                format!("EC Level: {}", qr.ec_level.label()),
                format!("Mask: {}", qr.mask_pattern),
                format!("Data: {} bytes", qr.data_len),
            ];
            for line in &info_lines {
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y: cy,
                    text: line.clone(),
                    color: TEXT_COLOR,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(max_w),
                });
                cy += 16.0;
            }
        } else if let Some(ref bc) = self.current_barcode {
            let info_lines = [
                "Type: Code128".to_string(),
                format!("Width: {} modules", bc.width()),
                format!("Data: {} chars", bc.data.len()),
            ];
            for line in &info_lines {
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y: cy,
                    text: line.clone(),
                    color: TEXT_COLOR,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(max_w),
                });
                cy += 16.0;
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: lx,
                y: cy,
                text: "No code generated yet".to_owned(),
                color: OVERLAY0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w),
            });
        }
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let mut app = QrApp::new();

    // Generate a sample QR code
    app.set_input("Hello, SlateOS!");
    app.generate();

    // Generate a URL example
    app.input_mode = InputMode::Url;
    app.set_input("example.com");
    app.generate();

    // Switch to barcode
    app.code_type = CodeType::Barcode128;
    app.input_mode = InputMode::Text;
    app.set_input("CODE128-TEST");
    app.generate();

    // Back to QR
    app.code_type = CodeType::QrCode;
    app.input_mode = InputMode::Text;
    app.set_input("QR Code Generator for SlateOS");
    app.generate();

    let cmds = app.render(1100.0, 700.0);
    let _ = cmds.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // --- GF(2^8) tests ---

    #[test]
    fn test_gf_tables_init() {
        let gf = GfTables::new();
        // exp[0] should be 1 (alpha^0 = 1)
        assert_eq!(gf.exp_table[0], 1);
        // exp[1] should be 2 (alpha^1 = 2)
        assert_eq!(gf.exp_table[1], 2);
    }

    #[test]
    fn test_gf_mul_identity() {
        let gf = GfTables::new();
        assert_eq!(gf.mul(1, 1), 1);
        assert_eq!(gf.mul(5, 1), 5);
        assert_eq!(gf.mul(1, 42), 42);
    }

    #[test]
    fn test_gf_mul_zero() {
        let gf = GfTables::new();
        assert_eq!(gf.mul(0, 100), 0);
        assert_eq!(gf.mul(100, 0), 0);
        assert_eq!(gf.mul(0, 0), 0);
    }

    #[test]
    fn test_gf_mul_known() {
        let gf = GfTables::new();
        // 2 * 2 = 4 in GF(256)
        assert_eq!(gf.mul(2, 2), 4);
        // Multiplication should be commutative
        assert_eq!(gf.mul(7, 13), gf.mul(13, 7));
    }

    // --- Reed-Solomon tests ---

    #[test]
    fn test_rs_generator_poly_length() {
        let gf = GfTables::new();
        let poly = rs_generator_poly(10, &gf);
        // Generator poly for n EC codewords has degree n, so n+1 coefficients
        assert_eq!(poly.len(), 11);
    }

    #[test]
    fn test_rs_encode_produces_correct_length() {
        let gf = GfTables::new();
        let data = vec![32, 91, 11, 120, 209, 114, 220, 77];
        let ec = rs_encode(&data, 10, &gf);
        assert_eq!(ec.len(), 10);
    }

    #[test]
    fn test_rs_encode_deterministic() {
        let gf = GfTables::new();
        let data = vec![1, 2, 3, 4, 5];
        let ec1 = rs_encode(&data, 7, &gf);
        let ec2 = rs_encode(&data, 7, &gf);
        assert_eq!(ec1, ec2);
    }

    // --- Version selection tests ---

    #[test]
    fn test_select_version_small_data() {
        let v = select_version(5, EcLevel::L);
        assert_eq!(v, Some(1));
    }

    #[test]
    fn test_select_version_medium_data() {
        let v = select_version(50, EcLevel::M);
        assert!(v.is_some());
        let ver = v.unwrap();
        assert!(ver >= 3);
    }

    #[test]
    fn test_select_version_too_large() {
        let v = select_version(1000, EcLevel::H);
        assert!(v.is_none());
    }

    #[test]
    fn test_select_version_all_ec_levels() {
        for ec in EcLevel::all() {
            let v = select_version(10, *ec);
            assert!(v.is_some());
        }
    }

    // --- QR size tests ---

    #[test]
    fn test_qr_size_v1() {
        assert_eq!(qr_size(1), 21);
    }

    #[test]
    fn test_qr_size_v5() {
        assert_eq!(qr_size(5), 37);
    }

    #[test]
    fn test_qr_size_v10() {
        assert_eq!(qr_size(10), 57);
    }

    // --- Matrix tests ---

    #[test]
    fn test_matrix_new() {
        let m = QrMatrix::new(21);
        assert_eq!(m.size, 21);
        assert!(m.get(0, 0).is_empty());
    }

    #[test]
    fn test_matrix_set_get() {
        let mut m = QrMatrix::new(21);
        m.set(5, 5, Module::FunctionDark);
        assert_eq!(m.get(5, 5), Module::FunctionDark);
        assert!(m.get(5, 5).is_dark());
    }

    #[test]
    fn test_matrix_out_of_bounds() {
        let m = QrMatrix::new(21);
        assert!(m.get(100, 100).is_empty());
    }

    #[test]
    fn test_finder_pattern_placement() {
        let mut m = QrMatrix::new(21);
        m.place_finder_pattern(0, 0);
        // Corners should be dark
        assert!(m.get(0, 0).is_dark());
        assert!(m.get(0, 6).is_dark());
        assert!(m.get(6, 0).is_dark());
        assert!(m.get(6, 6).is_dark());
        // Center of outer ring is dark
        assert!(m.get(0, 3).is_dark());
        // Inner area (1,1) should be light
        assert!(!m.get(1, 1).is_dark());
        // Center of 3x3 inner square
        assert!(m.get(3, 3).is_dark());
    }

    #[test]
    fn test_timing_patterns() {
        let mut m = QrMatrix::new(21);
        m.place_finder_pattern(0, 0);
        m.place_finder_pattern(0, 14);
        m.place_finder_pattern(14, 0);
        m.place_timing_patterns();
        // Timing on row 6 alternates starting with dark at col 8
        assert!(m.get(6, 8).is_dark());
        assert!(!m.get(6, 9).is_dark());
        assert!(m.get(6, 10).is_dark());
    }

    // --- QR code generation tests ---

    #[test]
    fn test_qr_encode_simple() {
        let qr = QrCode::encode(b"Hello", EcLevel::M);
        assert!(qr.is_some());
        let qr = qr.unwrap();
        assert_eq!(qr.version, 1);
        assert_eq!(qr.size(), 21);
    }

    #[test]
    fn test_qr_encode_empty() {
        let qr = QrCode::encode(b"", EcLevel::M);
        assert!(qr.is_none());
    }

    #[test]
    fn test_qr_encode_url() {
        let qr = QrCode::encode(b"https://example.com", EcLevel::L);
        assert!(qr.is_some());
    }

    #[test]
    fn test_qr_encode_various_ec_levels() {
        let data = b"Test Data";
        for ec in EcLevel::all() {
            let qr = QrCode::encode(data, *ec);
            assert!(qr.is_some(), "Failed for EC level {:?}", ec);
        }
    }

    #[test]
    fn test_qr_encode_max_v1() {
        // Version 1-L can hold 17 bytes
        let data = vec![b'A'; 17];
        let qr = QrCode::encode(&data, EcLevel::L);
        assert!(qr.is_some());
        assert_eq!(qr.unwrap().version, 1);
    }

    #[test]
    fn test_qr_version_auto_select() {
        // Data too large for version 1 should auto-select higher version
        let data = vec![b'X'; 30];
        let qr = QrCode::encode(&data, EcLevel::M);
        assert!(qr.is_some());
        let qr = qr.unwrap();
        assert!(qr.version >= 2);
    }

    #[test]
    fn test_qr_mask_is_valid() {
        let qr = QrCode::encode(b"Mask test", EcLevel::M);
        assert!(qr.is_some());
        let qr = qr.unwrap();
        assert!(qr.mask_pattern < 8);
    }

    // --- Bit writer tests ---

    #[test]
    fn test_bit_writer_basic() {
        let mut bw = BitWriter::new();
        bw.write_bits(0b1010, 4);
        assert_eq!(bw.len(), 4);
        let bytes = bw.to_bytes();
        assert_eq!(bytes.len(), 1);
        // 1010_0000 = 0xA0
        assert_eq!(bytes[0], 0xA0);
    }

    #[test]
    fn test_bit_writer_full_byte() {
        let mut bw = BitWriter::new();
        bw.write_bits(0xFF, 8);
        assert_eq!(bw.len(), 8);
        assert_eq!(bw.to_bytes(), vec![0xFF]);
    }

    #[test]
    fn test_bit_writer_multi_byte() {
        let mut bw = BitWriter::new();
        bw.write_bits(0xAB, 8);
        bw.write_bits(0xCD, 8);
        assert_eq!(bw.len(), 16);
        assert_eq!(bw.to_bytes(), vec![0xAB, 0xCD]);
    }

    // --- Code128 tests ---

    #[test]
    fn test_code128_encode_simple() {
        let bc = Code128Barcode::encode("Hello");
        assert!(bc.is_some());
        let bc = bc.unwrap();
        assert!(!bc.bars.is_empty());
        assert_eq!(bc.data, "Hello");
    }

    #[test]
    fn test_code128_encode_empty() {
        let bc = Code128Barcode::encode("");
        assert!(bc.is_none());
    }

    #[test]
    fn test_code128_encode_digits() {
        let bc = Code128Barcode::encode("123456");
        assert!(bc.is_some());
    }

    #[test]
    fn test_code128_width() {
        let bc = Code128Barcode::encode("Test").unwrap();
        // Width should be > 0
        assert!(bc.width() > 0);
    }

    #[test]
    fn test_code128_starts_ends_quiet() {
        let bc = Code128Barcode::encode("A").unwrap();
        // First 10 should be quiet zone (false)
        for i in 0..10 {
            assert!(!bc.bars[i], "Expected quiet zone at position {i}");
        }
        // Last 10 should be quiet zone
        let len = bc.bars.len();
        for i in (len - 10)..len {
            assert!(!bc.bars[i], "Expected quiet zone at position {i}");
        }
    }

    // --- Input mode formatting tests ---

    #[test]
    fn test_format_text() {
        let wifi = WifiConfig::default();
        let vcard = VCardInfo::default();
        let result = format_qr_data(InputMode::Text, "Hello", &wifi, &vcard);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_format_url_without_scheme() {
        let wifi = WifiConfig::default();
        let vcard = VCardInfo::default();
        let result = format_qr_data(InputMode::Url, "example.com", &wifi, &vcard);
        assert_eq!(result, "https://example.com");
    }

    #[test]
    fn test_format_url_with_scheme() {
        let wifi = WifiConfig::default();
        let vcard = VCardInfo::default();
        let result = format_qr_data(InputMode::Url, "http://example.com", &wifi, &vcard);
        assert_eq!(result, "http://example.com");
    }

    #[test]
    fn test_format_email() {
        let wifi = WifiConfig::default();
        let vcard = VCardInfo::default();
        let result = format_qr_data(InputMode::Email, "user@example.com", &wifi, &vcard);
        assert_eq!(result, "mailto:user@example.com");
    }

    #[test]
    fn test_format_phone() {
        let wifi = WifiConfig::default();
        let vcard = VCardInfo::default();
        let result = format_qr_data(InputMode::Phone, "+1234567890", &wifi, &vcard);
        assert_eq!(result, "tel:+1234567890");
    }

    #[test]
    fn test_format_wifi() {
        let wifi = WifiConfig {
            ssid: "MyNetwork".to_owned(),
            password: "secret123".to_owned(),
            encryption: WifiEncryption::Wpa,
            hidden: false,
        };
        let vcard = VCardInfo::default();
        let result = format_qr_data(InputMode::Wifi, "", &wifi, &vcard);
        assert!(result.contains("WIFI:"));
        assert!(result.contains("MyNetwork"));
        assert!(result.contains("secret123"));
        assert!(result.contains("WPA"));
    }

    #[test]
    fn test_format_vcard() {
        let wifi = WifiConfig::default();
        let vcard = VCardInfo {
            first_name: "John".to_owned(),
            last_name: "Doe".to_owned(),
            phone: "+1234567890".to_owned(),
            email: "john@example.com".to_owned(),
            organization: "ACME".to_owned(),
        };
        let result = format_qr_data(InputMode::VCard, "", &wifi, &vcard);
        assert!(result.contains("BEGIN:VCARD"));
        assert!(result.contains("END:VCARD"));
        assert!(result.contains("Doe;John"));
        assert!(result.contains("TEL:+1234567890"));
        assert!(result.contains("EMAIL:john@example.com"));
        assert!(result.contains("ORG:ACME"));
    }

    // --- Penalty evaluation tests ---

    #[test]
    fn test_penalty_all_dark() {
        let mut m = QrMatrix::new(21);
        for r in 0..21 {
            for c in 0..21 {
                m.set(r, c, Module::DataDark);
            }
        }
        let penalty = evaluate_penalty(&m);
        // All-dark should have significant penalty
        assert!(penalty > 0);
    }

    #[test]
    fn test_penalty_checkerboard() {
        let mut m = QrMatrix::new(21);
        for r in 0..21 {
            for c in 0..21 {
                let module = if (r + c) % 2 == 0 {
                    Module::DataDark
                } else {
                    Module::DataLight
                };
                m.set(r, c, module);
            }
        }
        let penalty = evaluate_penalty(&m);
        // Checkerboard should have relatively low penalty
        assert!(penalty < 500);
    }

    // --- Application tests ---

    #[test]
    fn test_app_new() {
        let app = QrApp::new();
        assert!(app.input_text.is_empty());
        assert_eq!(app.input_mode, InputMode::Text);
        assert_eq!(app.code_type, CodeType::QrCode);
        assert_eq!(app.ec_level, EcLevel::M);
        assert!(app.current_qr.is_none());
    }

    #[test]
    fn test_app_generate_qr() {
        let mut app = QrApp::new();
        app.set_input("Test QR");
        app.generate();
        assert!(app.current_qr.is_some());
        assert!(app.error_message.is_none());
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn test_app_generate_barcode() {
        let mut app = QrApp::new();
        app.code_type = CodeType::Barcode128;
        app.set_input("BARCODE");
        app.generate();
        assert!(app.current_barcode.is_some());
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn test_app_generate_empty() {
        let mut app = QrApp::new();
        app.generate();
        assert!(app.error_message.is_some());
    }

    #[test]
    fn test_app_history() {
        let mut app = QrApp::new();
        app.set_input("First");
        app.generate();
        app.set_input("Second");
        app.generate();
        assert_eq!(app.history.len(), 2);
        app.clear_history();
        assert!(app.history.is_empty());
    }

    #[test]
    fn test_app_render_empty() {
        let app = QrApp::new();
        let cmds = app.render(1100.0, 700.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_with_qr() {
        let mut app = QrApp::new();
        app.set_input("Render test");
        app.generate();
        let cmds = app.render(1100.0, 700.0);
        assert!(!cmds.is_empty());
        // Should have many more commands when QR is rendered
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_app_render_with_barcode() {
        let mut app = QrApp::new();
        app.code_type = CodeType::Barcode128;
        app.set_input("Barcode");
        app.generate();
        let cmds = app.render(1100.0, 700.0);
        assert!(!cmds.is_empty());
    }

    // --- Alignment pattern tests ---

    #[test]
    fn test_alignment_positions_v1() {
        assert!(alignment_positions(1).is_empty());
    }

    #[test]
    fn test_alignment_positions_v2() {
        let pos = alignment_positions(2);
        assert_eq!(pos, vec![6, 18]);
    }

    #[test]
    fn test_alignment_positions_v7() {
        let pos = alignment_positions(7);
        assert_eq!(pos.len(), 3);
    }

    // --- Format info ECC ---

    #[test]
    fn test_format_info_ecc() {
        // Known test vector: data bits 00101 -> format_bits = 0b00101
        let ecc = format_info_ecc(0b00101);
        // ECC should be 10-bit value
        assert!(ecc < 1024);
    }

    // --- EC level tests ---

    #[test]
    fn test_ec_level_labels() {
        assert_eq!(EcLevel::L.label(), "L (7%)");
        assert_eq!(EcLevel::M.label(), "M (15%)");
        assert_eq!(EcLevel::Q.label(), "Q (25%)");
        assert_eq!(EcLevel::H.label(), "H (30%)");
    }

    #[test]
    fn test_ec_level_format_bits() {
        assert_eq!(EcLevel::L.format_bits(), 0b01);
        assert_eq!(EcLevel::M.format_bits(), 0b00);
        assert_eq!(EcLevel::Q.format_bits(), 0b11);
        assert_eq!(EcLevel::H.format_bits(), 0b10);
    }

    // --- Module size tests ---

    #[test]
    fn test_module_size_pixels() {
        assert!((ModuleSize::Small.pixels() - 3.0).abs() < f32::EPSILON);
        assert!((ModuleSize::Medium.pixels() - 5.0).abs() < f32::EPSILON);
        assert!((ModuleSize::Large.pixels() - 8.0).abs() < f32::EPSILON);
    }

    // --- Data encoding tests ---

    #[test]
    fn test_encode_data_bits_v1() {
        let data = b"Hi";
        let result = encode_data_bits(data, 1, EcLevel::L);
        assert!(result.is_some());
    }

    #[test]
    fn test_apply_error_correction() {
        let data = b"Test";
        let encoded = encode_data_bits(data, 1, EcLevel::M);
        assert!(encoded.is_some());
        let ec_result = apply_error_correction(&encoded.unwrap(), 1, EcLevel::M);
        assert!(ec_result.is_some());
    }

    // --- Wifi encryption label ---

    #[test]
    fn test_wifi_encryption_labels() {
        assert_eq!(WifiEncryption::None.label(), "None");
        assert_eq!(WifiEncryption::Wep.label(), "WEP");
        assert_eq!(WifiEncryption::Wpa.label(), "WPA/WPA2");
    }

    // --- Module type tests ---

    #[test]
    fn test_module_is_dark() {
        assert!(Module::FunctionDark.is_dark());
        assert!(Module::DataDark.is_dark());
        assert!(!Module::FunctionLight.is_dark());
        assert!(!Module::DataLight.is_dark());
        assert!(!Module::Empty.is_dark());
    }

    #[test]
    fn test_module_is_function() {
        assert!(Module::FunctionDark.is_function());
        assert!(Module::FunctionLight.is_function());
        assert!(!Module::DataDark.is_function());
        assert!(!Module::DataLight.is_function());
        assert!(!Module::Empty.is_function());
    }
}

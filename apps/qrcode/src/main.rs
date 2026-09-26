//! `Slate OS` QR Code Generator
//!
//! A QR code and barcode generation tool with:
//! - QR code generation from scratch (byte mode, Reed-Solomon EC, versions 1-10)
//! - Input modes: text, URL, email, phone, `WiFi`, vCard
//! - Customizable module size and foreground/background colors
//! - Code128 barcode generation
//! - History of the codes made, one entry per code (not per keystroke); a
//!   press brings one back
//! - Multi-panel UI: input, preview, and options panels, every control of
//!   which answers the pointer as well as the keys (F1 lists them)
//! - Saving the code as an SVG picture, at the module size, in its colours
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
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::struct_excessive_bools)]

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use core::num::NonZeroUsize;

use guitk::Color;
use guitk::colorpicker::{ColorPickerDialog, ColorPickerEvent};
use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text::{self, TextCursor};
use guitk::textedit;
use guitk::textinput::TextInput;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

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
/// Every key this program answers, and what it does.
///
/// One list, drawn by `F1` and checked by `every_advertised_key_does_something`
/// -- so a key cannot be bound without being findable, and cannot be
/// advertised without working. Before this the program had eleven keys and
/// printed none of them: `Ctrl+E` changed the error correction of a code
/// somebody was about to print, and the only way to know was to read the
/// source.
const SHORTCUTS: &[(&str, &str)] = &[
    (
        "Ctrl+I / Ctrl+Shift+I",
        "Next / previous kind of thing to encode",
    ),
    (
        "Tab / Shift+Tab",
        "Next / previous box, where a kind has more than one",
    ),
    ("Left / Right / Home / End", "Move in the box"),
    ("Backspace / Delete", "Delete a character"),
    (
        "Ctrl+A / Ctrl+C / Ctrl+X / Ctrl+V",
        "Select all / copy / cut / paste, in the box",
    ),
    ("Escape", "Empty the box"),
    ("Ctrl+Q / Ctrl+B", "Make a QR code / a Code128 barcode"),
    (
        "Ctrl+E",
        "Error correction: more of it survives more damage",
    ),
    ("Ctrl+M", "How big each square is drawn and saved"),
    ("Ctrl+T", "WiFi: open, WEP or WPA"),
    ("Ctrl+H", "WiFi: whether the network is hidden"),
    ("Ctrl+S", "Save the code as a picture (SVG)"),
    ("Ctrl+K", "Forget the history"),
    ("F1", "This list"),
];

/// The most characters one box takes. A QR code this program makes holds at
/// most 271 bytes, so this is far past anything that can be encoded; it is
/// there so a paste cannot make a box without end.
const MAX_FIELD_CHARS: usize = 1000;

/// The most codes the history keeps; the oldest goes first.
const HISTORY_CAP: usize = 100;

/// A history row's height, with the gap under it.
const HISTORY_ROW_H: f32 = 28.0;

/// Everything in the window a pointer can press, as the renderer records it.
///
/// Nothing answered the pointer: the code-type toggle, the six kinds, the
/// boxes, the history, the error-correction and size lists and the colour
/// swatches were drawn as controls and were pictures of them (`known-issues.md`
/// -> `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    CodeType(CodeType),
    Save,
    Help,
    Mode(InputMode),
    Field(Field),
    /// The WiFi code's security.
    Encryption,
    /// Whether the WiFi network is hidden.
    Hidden,
    HistoryList,
    /// A history entry, by its place in `QrApp::history`.
    HistoryRow(usize),
    ClearHistory,
    Ec(EcLevel),
    Size(ModuleSize),
    Foreground,
    Background,
    ResetColors,
    HelpCard,
}

/// Which colour the colour dialog is choosing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Swatch {
    Foreground,
    Background,
}

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
///
/// The table states the same fact twice on purpose: `data_capacity_bytes` is
/// the published byte-mode capacity, and `total_codewords`, `num_blocks` and
/// `ec_codewords_per_block` imply it. That redundancy is the transcription's
/// only proofreader — see [`Self::byte_mode_capacity`] and the test that
/// compares the two for every row — and it is what caught sixteen of the forty
/// rows carrying a block count that did not match their own capacity.
struct VersionInfo {
    // `version` and `ec_level` are read only by `byte_mode_capacity` and the
    // test that calls it. That is the point rather than an oversight: the
    // redundancy between a row's stated capacity and the capacity its own
    // block counts imply is the transcription's only proofreader, and it
    // caught sixteen of the forty rows. `dead_code` is a per-target analysis,
    // so a test-only reader does not count for the binary.
    version: u8,
    #[allow(dead_code, reason = "read by the table's proofreading test")]
    ec_level: EcLevel,
    data_capacity_bytes: usize,
    ec_codewords_per_block: usize,
    num_blocks: usize,
    total_codewords: usize,
}

/// Bits in the byte-mode character-count indicator: 8 for versions 1-9, 16
/// from version 10 up (ISO/IEC 18004 §8.4.1).
fn count_indicator_bits(version: u8) -> usize {
    if version <= 9 { 8 } else { 16 }
}

impl VersionInfo {
    /// Codewords left for data once error correction has taken its share.
    ///
    /// This is what the encoder pads up to and what the interleaver splits
    /// into blocks, so both must read it from here rather than each spelling
    /// out `total - ec_per_block * num_blocks`.
    fn data_codewords(&self) -> usize {
        self.total_codewords
            .saturating_sub(self.ec_codewords_per_block.saturating_mul(self.num_blocks))
    }

    /// Payload bytes that fit in byte mode, derived from the block structure.
    ///
    /// The 4-bit mode indicator and the character-count indicator come out of
    /// the data codewords before any payload does; the remainder rounds down
    /// to whole bytes because a partial byte cannot hold a character.
    #[allow(
        dead_code,
        reason = "the table's proofreader; called by the test that compares                   every row's stated capacity against its implied one"
    )]
    fn byte_mode_capacity(&self) -> usize {
        let header_bits = 4_usize.saturating_add(count_indicator_bits(self.version));
        self.data_codewords()
            .saturating_mul(8)
            .saturating_sub(header_bits)
            / 8
    }
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
        (5, EcLevel::Q, 60, 18, 4, 134),
        (5, EcLevel::H, 44, 22, 4, 134),
        // Version 6
        (6, EcLevel::L, 134, 18, 2, 172),
        (6, EcLevel::M, 106, 16, 4, 172),
        (6, EcLevel::Q, 74, 24, 4, 172),
        (6, EcLevel::H, 58, 28, 4, 172),
        // Version 7
        (7, EcLevel::L, 154, 20, 2, 196),
        (7, EcLevel::M, 122, 18, 4, 196),
        (7, EcLevel::Q, 86, 18, 6, 196),
        (7, EcLevel::H, 64, 26, 5, 196),
        // Version 8
        (8, EcLevel::L, 192, 24, 2, 242),
        (8, EcLevel::M, 152, 22, 4, 242),
        (8, EcLevel::Q, 108, 22, 6, 242),
        (8, EcLevel::H, 84, 26, 6, 242),
        // Version 9
        (9, EcLevel::L, 230, 30, 2, 292),
        (9, EcLevel::M, 180, 22, 5, 292),
        (9, EcLevel::Q, 130, 20, 8, 292),
        (9, EcLevel::H, 98, 24, 8, 292),
        // Version 10
        (10, EcLevel::L, 271, 18, 4, 346),
        (10, EcLevel::M, 213, 26, 5, 346),
        (10, EcLevel::Q, 151, 24, 8, 346),
        (10, EcLevel::H, 119, 28, 8, 346),
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

    /// Place the version information, which versions 7 and up carry twice:
    /// eighteen bits -- the version in six, a BCH check in twelve -- in a 6x3
    /// block beside the top-right finder and a 3x6 one above the bottom-left.
    ///
    /// It was not placed at all, so a version 7-10 symbol had its data where
    /// the version belongs: every bit after the first block was one place
    /// from where a scanner reads it, and the code was unreadable. Anything
    /// past about 120 bytes -- most contacts, a long WiFi password -- needs
    /// version 7.
    fn place_version_info(&mut self, version: u8) {
        if version < 7 {
            return;
        }
        let bits = version_info_bits(version);
        for i in 0..18_usize {
            let module = if (bits >> i) & 1 == 1 {
                Module::FunctionDark
            } else {
                Module::FunctionLight
            };
            let across = self.size - 11 + i % 3;
            let down = i / 3;
            self.set(down, across, module);
            self.set(across, down, module);
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
        // 50, not 52: the last centre is always seven in from the edge, and
        // a version-10 symbol is 57 modules across. At 52 every version-10
        // code had its bottom-right alignment patterns where no scanner looks.
        10 => vec![6, 28, 50],
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
    let count_bits = count_indicator_bits(version);

    let mut bits = BitWriter::new();

    // Mode indicator: byte mode = 0b0100
    bits.write_bits(0b0100, 4);

    // Character count
    bits.write_bits(data.len() as u32, count_bits);

    // Data bytes
    for &byte in data {
        bits.write_bits(u32::from(byte), 8);
    }

    // Terminator (up to 4 zero bits). The target is the version's data
    // codeword count, read from the one place that derives it -- padding to a
    // different number than `apply_error_correction` splits into blocks is how
    // a symbol ends up structurally invalid while still filling the matrix.
    let total_data_bits = info.data_codewords().saturating_mul(8);
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
    // A table row claiming zero blocks would divide by zero two lines below.
    // The function already returns `Option`, so refusing to encode is both
    // cheaper and more honest than trusting the table from a distance.
    let num_blocks = NonZeroUsize::new(info.num_blocks)?;
    let total_data = data_codewords.len();
    // The spec's two groups: `num_blocks - extra` blocks of `base` codewords
    // followed by `extra` blocks of `base + 1`, smaller group first.
    let base_block_size = total_data / num_blocks;
    let extra_blocks = total_data % num_blocks;

    // Split data into blocks
    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(num_blocks.get());
    let mut offset: usize = 0;
    for i in 0..num_blocks.get() {
        let block_size = if i < num_blocks.get().saturating_sub(extra_blocks) {
            base_block_size
        } else {
            base_block_size.saturating_add(1)
        };
        let end = offset.saturating_add(block_size).min(total_data);
        blocks.push(data_codewords.get(offset..end).unwrap_or(&[]).to_vec());
        offset = end;
    }

    // Compute EC for each block
    let mut ec_blocks: Vec<Vec<u8>> = Vec::with_capacity(num_blocks.get());
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

/// A version's eighteen information bits: the version in the top six, and a
/// BCH(18,6) check with generator 0x1F25 in the low twelve.
fn version_info_bits(version: u8) -> u32 {
    let mut rem = u32::from(version);
    for _ in 0..12 {
        rem = (rem << 1) ^ ((rem >> 11) * 0x1F25);
    }
    (u32::from(version) << 12) | rem
}

/// The fifteen format bits: the error correction level and the mask, a
/// BCH(15,5) check, and the fixed pattern 101010000010010 over all of it.
fn format_bits(ec_level: EcLevel, mask_pattern: u8) -> u32 {
    let format_data = (u16::from(ec_level.format_bits()) << 3) | u16::from(mask_pattern);
    let format_ecc = format_info_ecc(format_data);
    ((u32::from(format_data) << 10) | u32::from(format_ecc)) ^ 0x5412
}

/// Write format information into the matrix: bit `i` of `format_bits` at
/// the standard's place for it, in both copies.
///
/// The copy under the top-right finder was one bit short: bit 7, at row 8
/// column `size - 8`, was reserved and never written, so it read 0 whatever
/// it should have been. A scanner corrects one bad bit in fifteen, which is
/// why it went unnoticed; it is also one of the three a damaged code can
/// spare.
fn write_format_info(matrix: &mut QrMatrix, ec_level: EcLevel, mask_pattern: u8) {
    let bits = format_bits(ec_level, mask_pattern);
    let size = matrix.size;
    let module = |i: usize| {
        if (bits >> i) & 1 == 1 {
            Module::FunctionDark
        } else {
            Module::FunctionLight
        }
    };
    // Beside the top-left finder: down column 8, then back along row 8,
    // stepping over the timing pattern each way.
    for i in 0..=5 {
        matrix.set(i, 8, module(i));
    }
    matrix.set(7, 8, module(6));
    matrix.set(8, 8, module(7));
    matrix.set(8, 7, module(8));
    for i in 9..15 {
        matrix.set(8, 14 - i, module(i));
    }
    // The copy: along row 8 under the top-right finder, then down column 8
    // beside the bottom-left one.
    for i in 0..8 {
        matrix.set(8, size - 1 - i, module(i));
    }
    for i in 8..15 {
        matrix.set(size - 15 + i, 8, module(i));
    }
    // The dark module, which the copy runs past and which is always dark.
    matrix.set(size - 8, 8, Module::FunctionDark);
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

        // Place the version information (versions 7 and up)
        base_matrix.place_version_info(version);

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
    // The widths of bar, space, bar, space, bar, space for each value, from
    // the standard's table. Values 36-38 and 60 onward were wrong -- entries
    // marked "placeholder", one with a space zero modules wide, several the
    // same as others -- so every lowercase letter and most punctuation made
    // bars no scanner could read. `every_code128_pattern_is_well_formed`
    // checks the rules each must keep and `the_code128_table_is_the_standards`
    // a sample against the published table.
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
    [1, 1, 2, 3, 1, 3], // 36: D
    [1, 3, 2, 1, 1, 3], // 37: E
    [1, 3, 2, 3, 1, 1], // 38: F
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
    [3, 1, 4, 1, 1, 1], // 60: backslash
    [2, 2, 1, 4, 1, 1], // 61: ]
    [4, 3, 1, 1, 1, 1], // 62: ^
    [1, 1, 1, 2, 2, 4], // 63: _
    [1, 1, 1, 4, 2, 2], // 64: `
    [1, 2, 1, 1, 2, 4], // 65: a
    [1, 2, 1, 4, 2, 1], // 66: b
    [1, 4, 1, 1, 2, 2], // 67: c
    [1, 4, 1, 2, 2, 1], // 68: d
    [1, 1, 2, 2, 1, 4], // 69: e
    [1, 1, 2, 4, 1, 2], // 70: f
    [1, 2, 2, 1, 1, 4], // 71: g
    [1, 2, 2, 4, 1, 1], // 72: h
    [1, 4, 2, 1, 1, 2], // 73: i
    [1, 4, 2, 2, 1, 1], // 74: j
    [2, 4, 1, 2, 1, 1], // 75: k
    [2, 2, 1, 1, 1, 4], // 76: l
    [4, 1, 3, 1, 1, 1], // 77: m
    [2, 4, 1, 1, 1, 2], // 78: n
    [1, 3, 4, 1, 1, 1], // 79: o
    [1, 1, 1, 2, 4, 2], // 80: p
    [1, 2, 1, 1, 4, 2], // 81: q
    [1, 2, 1, 2, 4, 1], // 82: r
    [1, 1, 4, 2, 1, 2], // 83: s
    [1, 2, 4, 1, 1, 2], // 84: t
    [1, 2, 4, 2, 1, 1], // 85: u
    [4, 1, 1, 2, 1, 2], // 86: v
    [4, 2, 1, 1, 1, 2], // 87: w
    [4, 2, 1, 2, 1, 1], // 88: x
    [2, 1, 2, 1, 4, 1], // 89: y
    [2, 1, 4, 1, 2, 1], // 90: z
    [4, 1, 2, 1, 2, 1], // 91: {
    [1, 1, 1, 1, 4, 3], // 92: |
    [1, 1, 1, 3, 4, 1], // 93: }
    [1, 3, 1, 1, 4, 1], // 94: ~
    [1, 1, 4, 1, 1, 3], // 95: DEL
    [1, 1, 4, 3, 1, 1], // 96: FNC3
    [4, 1, 1, 1, 1, 3], // 97: FNC2
    [4, 1, 1, 3, 1, 1], // 98: SHIFT
    [1, 1, 3, 1, 4, 1], // 99: CODE_C
    [1, 1, 4, 1, 3, 1], // 100: CODE_B (FNC4 in A)
    [3, 1, 1, 1, 4, 1], // 101: CODE_A (FNC4 in B)
    [4, 1, 1, 1, 3, 1], // 102: FNC1
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

        // Convert characters to Code B values. A character Code B has no
        // value for refuses the whole barcode: it was skipped, so "Caf\u{e9}"
        // made a barcode that scans as "Caf" under a label reading "Caf\u{e9}".
        for ch in data.chars() {
            let ascii_val = ch as u32;
            if !(32..=127).contains(&ascii_val) {
                return None;
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

/// Why `data` cannot be a Code128 barcode, if it cannot: Code B holds the
/// printable ASCII characters and nothing else.
fn code128_refusal(data: &str) -> Option<String> {
    let bad = data
        .chars()
        .find(|c| !(32..=127).contains(&u32::from(*c)))?;
    let shown = if bad.is_control() {
        format!("U+{:04X}", u32::from(bad))
    } else {
        format!("\u{201c}{bad}\u{201d}")
    };
    Some(format!(
        "A Code128 barcode holds letters, digits and ASCII punctuation only, and {shown} is none of them"
    ))
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

    /// The boxes this mode shows, in the order they are drawn.
    ///
    /// The four text-shaped modes share one box: `format_qr_data` wraps what
    /// is typed -- `https://`, `mailto:`, `tel:` -- rather than asking for
    /// anything extra.
    #[must_use]
    pub fn fields(self) -> &'static [Field] {
        match self {
            Self::Text | Self::Url | Self::Email | Self::Phone => &[Field::Text],
            Self::Wifi => &[Field::Ssid, Field::Password],
            Self::VCard => &[
                Field::FirstName,
                Field::LastName,
                Field::Phone,
                Field::Email,
                Field::Organization,
            ],
        }
    }

    /// The next mode along `all()`, wrapping.
    #[must_use]
    pub fn step(self, forward: bool) -> Self {
        let modes = Self::all();
        let at = modes.iter().position(|m| *m == self).unwrap_or(0);
        let last = modes.len().saturating_sub(1);
        let next = if forward {
            if at >= last { 0 } else { at.saturating_add(1) }
        } else if at == 0 {
            last
        } else {
            at.saturating_sub(1)
        };
        modes.get(next).copied().unwrap_or(Self::Text)
    }
}

/// One typed-into box of the left panel.
///
/// Six input modes were drawn as a row of buttons with the active one
/// highlighted, and `input_mode` was `Text` at construction with no writer in
/// the crate -- so `format_qr_data`'s Url, Email, Phone, Wifi and VCard arms,
/// all written and all tested, encoded nothing for anybody. Making the row
/// work exposed the next layer: the Wifi and VCard modes draw labelled boxes
/// reading "Enter SSID..." and there was no way to type into one, because
/// `wifi_config` and `vcard_info` had no writers either.
///
/// So the boxes are this list. `InputMode::fields` says which of them a mode
/// shows, the renderer walks that, Tab walks that, and typing goes to the one
/// the walk has landed on. A box cannot be drawn without being typeable,
/// because being drawn means being in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// The single box the four text-shaped modes share.
    Text,
    Ssid,
    Password,
    FirstName,
    LastName,
    Phone,
    Email,
    Organization,
}

impl Field {
    /// The label above the box.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Content",
            Self::Ssid => "SSID",
            Self::Password => "Password",
            Self::FirstName => "First Name",
            Self::LastName => "Last Name",
            Self::Phone => "Phone",
            Self::Email => "Email",
            Self::Organization => "Organization",
        }
    }
}

/// `WiFi` configuration for QR encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

    /// The same, as a whole number of pixels for a saved picture.
    fn whole_pixels(self) -> usize {
        match self {
            Self::Small => 3,
            Self::Medium => 5,
            Self::Large => 8,
        }
    }

    fn all() -> &'static [ModuleSize] {
        &[ModuleSize::Small, ModuleSize::Medium, ModuleSize::Large]
    }
}

// ============================================================================
// History
// ============================================================================

/// A code that was made, with what made it, so a press brings it back.
///
/// One per code, not per keystroke: typing "Hello" added five entries --
/// "H", "He", "Hel", "Hell", "Hello" -- because every keystroke re-encodes.
/// The newest entry is updated while its code is being typed, and a new one
/// starts when a new code does (`QrApp::history_open`). The `timestamp` that
/// was here counted from 1000 and was shown nowhere.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub data: String,
    pub mode: InputMode,
    pub code_type: CodeType,
    pub ec_level: EcLevel,
    pub input_text: String,
    pub wifi: WifiConfig,
    pub vcard: VCardInfo,
}

impl HistoryEntry {
    /// Whether two entries are the same code.
    fn same_code(&self, other: &Self) -> bool {
        self.data == other.data && self.code_type == other.code_type && self.mode == other.mode
    }
}

// ============================================================================
// Application state
// ============================================================================

pub struct QrApp {
    pub input_text: String,
    pub input_mode: InputMode,
    /// Which of this mode's boxes the keyboard is typing into.
    pub focused_field: usize,
    /// Whether the shortcut list is up.
    pub show_help: bool,
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
    /// Why the last attempt made no code: too long, or a character a barcode
    /// cannot hold.
    pub error_message: Option<String>,
    /// Why there is no code when nothing is wrong: the box is empty, or a
    /// WiFi code has no network name yet.
    pub waiting_for: Option<&'static str>,
    pub window_width: f32,
    pub window_height: f32,
    /// The box being typed into, as a field with a caret and a selection. It
    /// edits the text of `focused_field()`, and is reloaded from it whenever
    /// the two differ, so the strings stay the one record of what is typed.
    editor: TextInput,
    /// The boxes' clipboard.
    clipboard: String,
    /// Whether the newest history entry is the code being typed, so the next
    /// change updates it rather than adding another.
    history_open: bool,
    /// How far the history list is scrolled, in rows.
    history_scroll: usize,
    /// What the last save said.
    notice: Option<String>,
    /// Where to save the code.
    picker: FilePicker,
    /// A colour being chosen, and the dialog choosing it.
    color_dialog: Option<(Swatch, ColorPickerDialog)>,
    /// What the pointer is over, so it can be drawn lit.
    hover: Option<Target>,
    /// Every box the last paint recorded, for hover and the wheel.
    last_hits: Vec<(Target, Rect)>,
    /// The wheel's remainder.
    wheel: wheel::Accumulator,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

impl Default for QrApp {
    fn default() -> Self {
        Self::new()
    }
}

impl QrApp {
    pub fn new() -> Self {
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            input_text: String::new(),
            input_mode: InputMode::Text,
            focused_field: 0,
            show_help: false,
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
            waiting_for: Some(NOTHING_TYPED),
            window_width: 1100.0,
            window_height: 700.0,
            editor: TextInput::new(),
            clipboard: String::new(),
            history_open: false,
            history_scroll: 0,
            notice: None,
            picker: FilePicker::default(),
            color_dialog: None,
            hover: None,
            last_hits: Vec::new(),
            wheel: wheel::Accumulator::default(),
        }
    }

    /// Why there is nothing to encode, if there is not.
    fn missing_input(&self) -> Option<&'static str> {
        match self.input_mode {
            InputMode::Text | InputMode::Url | InputMode::Email | InputMode::Phone => {
                self.input_text.is_empty().then_some(NOTHING_TYPED)
            }
            InputMode::Wifi => self
                .wifi_config
                .ssid
                .is_empty()
                .then_some("A WiFi code needs the network's name (SSID)"),
            InputMode::VCard => {
                let v = &self.vcard_info;
                [
                    &v.first_name,
                    &v.last_name,
                    &v.phone,
                    &v.email,
                    &v.organization,
                ]
                .iter()
                .all(|s| s.is_empty())
                .then_some("A contact needs a name, a phone number or an email address")
            }
        }
    }

    /// Generate a code from current settings.
    ///
    /// With nothing to encode there is no code: the one for what was there
    /// before stayed on screen, a code for text no longer in the box. And the
    /// four text-shaped kinds encoded their prefix alone -- an empty URL box
    /// made a code for `https://`.
    pub fn generate(&mut self) {
        self.error_message = None;
        self.waiting_for = self.missing_input();
        if self.waiting_for.is_some() {
            self.current_qr = None;
            self.current_barcode = None;
            // The code being typed is finished with; what is typed next is
            // a new one.
            self.history_open = false;
            return;
        }

        let data = format_qr_data(
            self.input_mode,
            &self.input_text,
            &self.wifi_config,
            &self.vcard_info,
        );

        match self.code_type {
            CodeType::QrCode => match QrCode::encode(data.as_bytes(), self.ec_level) {
                Some(qr) => {
                    self.current_qr = Some(qr);
                    self.current_barcode = None;
                    self.remember(data);
                }
                None => {
                    self.current_qr = None;
                    self.current_barcode = None;
                    let most =
                        get_version_info(10, self.ec_level).map_or(0, |v| v.byte_mode_capacity());
                    self.error_message = Some(format!(
                        "Too long for a QR code: {} bytes, and at error correction {} the most is {most}",
                        data.len(),
                        self.ec_level.short_label()
                    ));
                }
            },
            CodeType::Barcode128 => {
                if let Some(why) = code128_refusal(&data) {
                    self.current_qr = None;
                    self.current_barcode = None;
                    self.error_message = Some(why);
                    return;
                }
                match Code128Barcode::encode(&data) {
                    Some(barcode) => {
                        self.current_barcode = Some(barcode);
                        self.current_qr = None;
                        self.remember(data);
                    }
                    None => {
                        self.current_qr = None;
                        self.current_barcode = None;
                        self.error_message = Some("Cannot encode data as Code128".to_owned());
                    }
                }
            }
        }
    }

    /// Put the code just made in the history: over the newest entry while
    /// that code is being typed, as a new one otherwise. An older entry for
    /// the same code goes, so one code is one row.
    fn remember(&mut self, data: String) {
        let entry = HistoryEntry {
            data,
            mode: self.input_mode,
            code_type: self.code_type,
            ec_level: self.ec_level,
            input_text: self.input_text.clone(),
            wifi: self.wifi_config.clone(),
            vcard: self.vcard_info.clone(),
        };
        match self.history.last_mut() {
            Some(last) if self.history_open => *last = entry,
            _ => {
                self.history.push(entry);
                self.history_open = true;
            }
        }
        let newest = self.history.len().saturating_sub(1);
        if let Some(newest_entry) = self.history.get(newest).cloned()
            && let Some(older) = self
                .history
                .iter()
                .take(newest)
                .position(|e| e.same_code(&newest_entry))
        {
            self.history.remove(older);
        }
        if self.history.len() > HISTORY_CAP {
            self.history.remove(0);
        }
    }

    /// Bring history entry `index` back: its kind, its boxes and its
    /// settings, as the newest entry.
    fn restore(&mut self, index: usize) {
        if index >= self.history.len() {
            return;
        }
        let entry = self.history.remove(index);
        self.input_mode = entry.mode;
        self.code_type = entry.code_type;
        self.ec_level = entry.ec_level;
        self.input_text.clone_from(&entry.input_text);
        self.wifi_config = entry.wifi.clone();
        self.vcard_info = entry.vcard.clone();
        self.focused_field = 0;
        self.history.push(entry);
        self.history_open = true;
        self.history_scroll = 0;
        self.notice = None;
        self.load_editor();
        self.generate();
    }

    /// Set input text and auto-generate.
    ///
    /// A whole new text, not an edit of the last, so it starts a new
    /// history entry.
    pub fn set_input(&mut self, text: &str) {
        self.input_text = text.to_owned();
        self.history_open = false;
        self.load_editor();
    }

    /// The box the keyboard is typing into.
    ///
    /// Clamped rather than stored blindly: the mode decides how many boxes
    /// there are, and moving from vCard's five to WiFi's two must not leave
    /// the cursor pointing past the end.
    #[must_use]
    pub fn focused_field(&self) -> Field {
        let fields = self.input_mode.fields();
        fields
            .get(self.focused_field.min(fields.len().saturating_sub(1)))
            .copied()
            .unwrap_or(Field::Text)
    }

    /// What is in a box.
    #[must_use]
    pub fn field_text(&self, field: Field) -> &str {
        match field {
            Field::Text => &self.input_text,
            Field::Ssid => &self.wifi_config.ssid,
            Field::Password => &self.wifi_config.password,
            Field::FirstName => &self.vcard_info.first_name,
            Field::LastName => &self.vcard_info.last_name,
            Field::Phone => &self.vcard_info.phone,
            Field::Email => &self.vcard_info.email,
            Field::Organization => &self.vcard_info.organization,
        }
    }

    fn field_text_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Text => &mut self.input_text,
            Field::Ssid => &mut self.wifi_config.ssid,
            Field::Password => &mut self.wifi_config.password,
            Field::FirstName => &mut self.vcard_info.first_name,
            Field::LastName => &mut self.vcard_info.last_name,
            Field::Phone => &mut self.vcard_info.phone,
            Field::Email => &mut self.vcard_info.email,
            Field::Organization => &mut self.vcard_info.organization,
        }
    }

    /// Move the keyboard to the next box of this mode, wrapping.
    fn step_field(&mut self, forward: bool) {
        let count = self.input_mode.fields().len();
        if count <= 1 {
            self.focused_field = 0;
            return;
        }
        let at = self.focused_field.min(count.saturating_sub(1));
        let last = count.saturating_sub(1);
        self.focused_field = if forward {
            if at >= last { 0 } else { at.saturating_add(1) }
        } else if at == 0 {
            last
        } else {
            at.saturating_sub(1)
        };
    }

    /// Change what kind of thing is being encoded.
    fn set_input_mode(&mut self, mode: InputMode) {
        self.input_mode = mode;
        // The new mode's boxes are a different list; starting anywhere but
        // its first box would put the cursor somewhere the eye has to hunt
        // for, and possibly past the end.
        self.focused_field = 0;
        // The same text as a URL is a different code from it as text.
        self.history_open = false;
        self.load_editor();
        self.generate();
    }

    /// Clear history.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.history_open = false;
        self.history_scroll = 0;
    }

    /// Load the box being typed into from what it holds, caret at the end.
    fn load_editor(&mut self) {
        let text = self.field_text(self.focused_field()).to_owned();
        self.editor.set_text(&text);
    }

    /// Apply a keystroke to the box being typed into, and answer whether it
    /// changed anything.
    fn edit(&mut self, key: &KeyEvent) -> EventResult {
        let field = self.focused_field();
        if self.editor.text() != self.field_text(field) {
            // Changed from outside the box -- a restore, a test -- so start
            // from what it holds, not from what the editor last saw.
            self.load_editor();
        }
        let before = (
            self.editor.text().to_owned(),
            self.editor.cursor(),
            self.editor.selection_anchor(),
        );
        let clipboard = self.clipboard.clone();
        let done = textline::apply_key(&mut self.editor, key, MAX_FIELD_CHARS, &clipboard, 12.0);
        let copied = done.copied.is_some();
        if let Some(text) = done.copied {
            self.clipboard = text;
        }
        let typed = self.editor.text() != before.0;
        if typed {
            *self.field_text_mut(field) = self.editor.text().to_owned();
            self.notice = None;
            self.generate();
        }
        if typed
            || copied
            || self.editor.cursor() != before.1
            || self.editor.selection_anchor() != before.2
        {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    /// Empty the box being typed into.
    fn clear_box(&mut self) -> EventResult {
        let field = self.focused_field();
        if self.field_text(field).is_empty() {
            return EventResult::Ignored;
        }
        self.field_text_mut(field).clear();
        self.load_editor();
        self.generate();
        EventResult::Consumed
    }

    /// The WiFi code's security, one step on. Only in WiFi mode.
    fn step_encryption(&mut self) -> EventResult {
        if self.input_mode != InputMode::Wifi {
            return EventResult::Ignored;
        }
        self.wifi_config.encryption = match self.wifi_config.encryption {
            WifiEncryption::None => WifiEncryption::Wep,
            WifiEncryption::Wep => WifiEncryption::Wpa,
            WifiEncryption::Wpa => WifiEncryption::None,
        };
        self.generate();
        EventResult::Consumed
    }

    /// Whether the WiFi network is hidden, the other way. Only in WiFi mode.
    fn toggle_hidden(&mut self) -> EventResult {
        if self.input_mode != InputMode::Wifi {
            return EventResult::Ignored;
        }
        self.wifi_config.hidden = !self.wifi_config.hidden;
        self.generate();
        EventResult::Consumed
    }

    // ------------------------------------------------------------------
    // Saving
    // ------------------------------------------------------------------

    /// Ask where to save the code on screen.
    ///
    /// There was no way to keep a code at all: the toolbar's Generate button
    /// did nothing (typing already generates), and a code could only be
    /// scanned off the screen.
    fn ask_where_to_save(&mut self) -> EventResult {
        if self.current_qr.is_none() && self.current_barcode.is_none() {
            self.notice = Some(String::from("Nothing to save yet: there is no code"));
            return EventResult::Consumed;
        }
        self.picker.open_to_write(match self.code_type {
            CodeType::QrCode => "qrcode.svg",
            CodeType::Barcode128 => "barcode.svg",
        });
        EventResult::Consumed
    }

    /// Write the code on screen to `path` as SVG, and say how that went.
    pub fn save_svg(&mut self, path: &Path) -> String {
        let Some(svg) = self.svg() else {
            return String::from("Nothing to save: there is no code");
        };
        match safeio::write_str_atomically(path, &svg) {
            Ok(()) => format!("Saved {}", path.display()),
            Err(err) => format!("Could not save {}: {err}", path.display()),
        }
    }

    /// The code on screen as an SVG picture: its colours, the quiet margin a
    /// scanner needs round it, and the module size as its size in pixels. A
    /// vector picture, so it prints sharp at any size.
    #[must_use]
    pub fn svg(&self) -> Option<String> {
        let scale = self.module_size.whole_pixels();
        let fg = svg_paint(self.fg_color);
        let bg = svg_paint(self.bg_color);
        if let Some(qr) = &self.current_qr {
            // Four modules of margin, which the standard requires.
            let quiet = 4;
            let size = qr.size();
            let side = size + 2 * quiet;
            let mut d = String::new();
            for row in 0..size {
                let mut col = 0;
                while col < size {
                    if !qr.is_dark(row, col) {
                        col += 1;
                        continue;
                    }
                    let start = col;
                    while col < size && qr.is_dark(row, col) {
                        col += 1;
                    }
                    let run = col - start;
                    d.push_str(&format!(
                        "M{} {}h{run}v1h-{run}z",
                        start + quiet,
                        row + quiet
                    ));
                }
            }
            return Some(format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {side} {side}\" \
                 width=\"{w}\" height=\"{w}\" shape-rendering=\"crispEdges\">\n\
                 <rect width=\"{side}\" height=\"{side}\" {bg}/>\n\
                 <path {fg} d=\"{d}\"/>\n</svg>\n",
                w = side * scale,
            ));
        }
        let bc = self.current_barcode.as_ref()?;
        // `bars` carries its ten-module quiet zones already.
        let width = bc.width();
        let bars_h = 50;
        let height = bars_h + 14;
        let mut d = String::new();
        let mut x = 0;
        while x < width {
            if !bc.bars.get(x).copied().unwrap_or(false) {
                x += 1;
                continue;
            }
            let start = x;
            while x < width && bc.bars.get(x).copied().unwrap_or(false) {
                x += 1;
            }
            let run = x - start;
            d.push_str(&format!("M{start} 0h{run}v{bars_h}h-{run}z"));
        }
        Some(format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" \
             width=\"{w}\" height=\"{h}\" shape-rendering=\"crispEdges\">\n\
             <rect width=\"{width}\" height=\"{height}\" {bg}/>\n\
             <path {fg} d=\"{d}\"/>\n\
             <text x=\"{mid}\" y=\"{text_y}\" font-family=\"monospace\" font-size=\"9\" \
             text-anchor=\"middle\" {fg}>{label}</text>\n</svg>\n",
            w = width * scale,
            h = height * scale,
            mid = width / 2,
            text_y = bars_h + 11,
            label = xml_escape(&bc.data),
        ))
    }

    /// Whether a scanner can be expected to read the code in its colours:
    /// the squares darker than the ground, by a margin.
    #[must_use]
    pub fn colors_scannable(&self) -> bool {
        let (dark, light) = (luminance(self.fg_color), luminance(self.bg_color));
        dark < light && (light + 0.05) / (dark + 0.05) >= 3.0
    }

    // ------------------------------------------------------------------
    // The colour dialog
    // ------------------------------------------------------------------

    /// Hand an event to the colour dialog, and take its answer.
    fn color_dialog_event(&mut self, event: &Event) -> EventResult {
        let (width, height) = (self.window_width, self.window_height);
        let Some((swatch, dialog)) = self.color_dialog.as_mut() else {
            return EventResult::Ignored;
        };
        let swatch = *swatch;
        let outcome = match event {
            Event::Key(key) if key.pressed => dialog.handle_key(key),
            Event::Mouse(mouse) => dialog.handle_mouse(mouse, width, height),
            _ => None,
        };
        match outcome {
            Some(ColorPickerEvent::Confirmed(color)) => {
                match swatch {
                    Swatch::Foreground => self.fg_color = color,
                    Swatch::Background => self.bg_color = color,
                }
                self.color_dialog = None;
            }
            Some(ColorPickerEvent::Cancelled) => self.color_dialog = None,
            // The live preview, which the dialog draws for itself.
            _ => {}
        }
        // Everything reaches the dialog while it is up, and it is drawn afresh
        // for each -- a drag across its square included.
        EventResult::Consumed
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------

    /// Route a compositor event into the app.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The save dialog takes input first while it is up, or a filename is
        // typed into the box behind it.
        match self
            .picker
            .handle(event, self.window_width, self.window_height)
        {
            Picked::Chose(path) => {
                self.notice = Some(self.save_svg(&path));
                return EventResult::Consumed;
            }
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        if self.color_dialog.is_some() && !matches!(event, Event::Resize { .. }) {
            return self.color_dialog_event(event);
        }
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.window_width = *width as f32;
                    self.window_height = *height as f32;
                }
                // Not `Consumed`: a resize is not by itself a reason to redraw.
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Apply a key press.
    ///
    /// The box always has the keys, because a program whose entire input is
    /// "the thing to encode" should not need a keystroke before it will accept
    /// it. Everything else is therefore on Ctrl.
    pub fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        let ctrl = key.modifiers.ctrl;
        if key.key == Key::F1 {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        // The list of keys is modal while it is up.
        if self.show_help {
            if matches!(key.key, Key::Escape | Key::Enter) {
                self.show_help = false;
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }
        match key.key {
            // What kind of thing is being encoded. Six modes were drawn as
            // a row of buttons with the active one highlighted, and nothing
            // could move the highlight, so `format_qr_data`'s Url, Email,
            // Phone, Wifi and VCard arms encoded nothing for anybody.
            Key::I if ctrl => {
                self.set_input_mode(self.input_mode.step(!key.modifiers.shift));
                EventResult::Consumed
            }
            // Between this mode's boxes. Modes with one box answer by doing
            // nothing, which is the honest reply to "next box" when there
            // is not one.
            Key::Tab => {
                if self.input_mode.fields().len() <= 1 {
                    return EventResult::Ignored;
                }
                self.step_field(!key.modifiers.shift);
                self.load_editor();
                EventResult::Consumed
            }
            // The box the cursor is on, for the same reason Backspace works
            // on that one: clearing a box the user is not looking at is a
            // surprise.
            Key::Escape => self.clear_box(),
            // Ctrl+Q and Ctrl+B pick what kind of code to make.
            Key::Q if ctrl => self.set_code_type(CodeType::QrCode),
            Key::B if ctrl => self.set_code_type(CodeType::Barcode128),
            // Ctrl+E cycles the error-correction level, which is the setting
            // that changes the picture rather than the data.
            Key::E if ctrl => {
                self.ec_level = match self.ec_level {
                    EcLevel::L => EcLevel::M,
                    EcLevel::M => EcLevel::Q,
                    EcLevel::Q => EcLevel::H,
                    EcLevel::H => EcLevel::L,
                };
                self.generate();
                EventResult::Consumed
            }
            Key::M if ctrl => {
                self.module_size = match self.module_size {
                    ModuleSize::Small => ModuleSize::Medium,
                    ModuleSize::Medium => ModuleSize::Large,
                    ModuleSize::Large => ModuleSize::Small,
                };
                // The module size is how big each square is drawn; the code
                // itself does not change, so there is nothing to regenerate.
                EventResult::Consumed
            }
            // What the WiFi code claims about the network. Both were fixed at
            // construction, so a code for an open network told the phone it
            // was WPA -- a connection that fails with no useful message --
            // and a hidden network's code left out the flag that makes a
            // phone go looking for it. On T and H, the letters the code
            // itself writes them under (`WIFI:T:...;H:true;`); T was S, which
            // everywhere else saves, and here changed the code the user was
            // about to print.
            Key::T if ctrl => self.step_encryption(),
            Key::H if ctrl => self.toggle_hidden(),
            Key::S if ctrl => self.ask_where_to_save(),
            Key::K if ctrl => {
                if self.history.is_empty() {
                    return EventResult::Ignored;
                }
                self.clear_history();
                EventResult::Consumed
            }
            // Into the box the cursor is on: typing, moving, deleting,
            // selecting, copying and pasting.
            _ => self.edit(key),
        }
    }

    /// Switch between a QR code and a barcode, regenerating from the same text.
    fn set_code_type(&mut self, code_type: CodeType) -> EventResult {
        if self.code_type == code_type {
            return EventResult::Ignored;
        }
        self.code_type = code_type;
        self.generate();
        EventResult::Consumed
    }

    // ------------------------------------------------------------------
    // The pointer
    // ------------------------------------------------------------------

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self
                .frame(self.window_width, self.window_height)
                .hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let frame = self.frame(self.window_width, self.window_height);
                let Some(target) = frame.hit_test(event.x, event.y) else {
                    return EventResult::Ignored;
                };
                let rect = frame.rect_of(|t| *t == target);
                self.press(target, event.x, rect)
            }
            MouseEventKind::Move => {
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Leave => {
                if self.hover.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Scroll { dy, .. } => self.wheel_at(event.x, event.y, dy),
            _ => EventResult::Ignored,
        }
    }

    /// A left press on `target`, at `x` in the box `rect` it was drawn in.
    fn press(&mut self, target: Target, x: f32, rect: Option<Rect>) -> EventResult {
        match target {
            Target::HelpCard => self.show_help = false,
            Target::Help => self.show_help = true,
            Target::CodeType(code_type) => return self.set_code_type(code_type),
            Target::Save => return self.ask_where_to_save(),
            Target::Mode(mode) => {
                if mode == self.input_mode {
                    return EventResult::Ignored;
                }
                self.set_input_mode(mode);
            }
            Target::Field(field) => {
                let Some(at) = self.input_mode.fields().iter().position(|f| *f == field) else {
                    return EventResult::Ignored;
                };
                let was = self.focused_field();
                self.focused_field = at;
                if was != field || self.editor.text() != self.field_text(field) {
                    self.load_editor();
                }
                if let Some(rect) = rect {
                    self.place_caret(rect, x, was == field);
                }
            }
            Target::Encryption => return self.step_encryption(),
            Target::Hidden => return self.toggle_hidden(),
            Target::HistoryRow(index) => self.restore(index),
            Target::ClearHistory => {
                if self.history.is_empty() {
                    return EventResult::Ignored;
                }
                self.clear_history();
            }
            Target::Ec(level) => {
                if level == self.ec_level {
                    return EventResult::Ignored;
                }
                self.ec_level = level;
                self.generate();
            }
            Target::Size(size) => {
                if size == self.module_size {
                    return EventResult::Ignored;
                }
                self.module_size = size;
            }
            Target::Foreground => {
                self.color_dialog =
                    Some((Swatch::Foreground, ColorPickerDialog::new(self.fg_color)));
            }
            Target::Background => {
                self.color_dialog =
                    Some((Swatch::Background, ColorPickerDialog::new(self.bg_color)));
            }
            Target::ResetColors => {
                if (self.fg_color, self.bg_color) == (Color::BLACK, Color::WHITE) {
                    return EventResult::Ignored;
                }
                self.fg_color = Color::BLACK;
                self.bg_color = Color::WHITE;
            }
            Target::HistoryList => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Put the box's caret under the pointer at `x`, measured against the
    /// box as it was drawn: from its start when the keys were elsewhere,
    /// scrolled to its caret when they were in it.
    fn place_caret(&mut self, rect: Rect, x: f32, was_focused: bool) {
        let drawn = if was_focused {
            self.editor.cursor()
        } else {
            TextCursor::default()
        };
        let cursor = textedit::cursor_at_click(
            self.editor.text(),
            drawn,
            (rect.w - 16.0).max(0.0),
            12.0,
            FontWeightHint::Regular,
            x - rect.x - 8.0,
        );
        self.editor.set_selection_anchor(None);
        self.editor.set_cursor(cursor);
    }

    /// The wheel over the history.
    fn wheel_at(&mut self, x: f32, y: f32, dy: f32) -> EventResult {
        if !matches!(
            self.target_at(x, y),
            Some(Target::HistoryList | Target::HistoryRow(_))
        ) {
            return EventResult::Ignored;
        }
        let rows = self.wheel.rows(dy);
        let last = self.history_last_scroll();
        let now = self.history_scroll;
        let next = if rows < 0 {
            now.saturating_sub(rows.unsigned_abs())
        } else {
            now.saturating_add(rows.unsigned_abs())
        }
        .min(last);
        if next == now {
            return EventResult::Ignored;
        }
        self.history_scroll = next;
        EventResult::Consumed
    }

    /// How many history rows the list shows at once.
    fn history_rows(&self) -> usize {
        let (_, pane) = self.input_layout();
        ((pane.h / HISTORY_ROW_H).floor().max(1.0)) as usize
    }

    /// The furthest the history list scrolls.
    fn history_last_scroll(&self) -> usize {
        self.history.len().saturating_sub(self.history_rows())
    }

    // ------------------------------------------------------------------
    // Rendering
    // ------------------------------------------------------------------

    /// Named `render_commands` and not `render`: this takes a width and a
    /// height, exactly as `oswindow::app::App::render` does, and at equal arity
    /// an inherent method silently wins method lookup over the trait's — so an
    /// app that keeps the name draws nothing and reports no error.
    ///
    /// For the tests: the window draws `frame`, whose boxes it keeps.
    #[cfg(test)]
    pub fn render_commands(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        self.frame(width, height).into_tree().commands
    }

    /// Draw the window, recording every control where it is drawn: both the
    /// picture and the hit test.
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut f, width);
        self.render_status_bar(&mut f, width, height);

        let content_y = TOOLBAR_HEIGHT;
        let content_h = (height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT).max(0.0);

        // Each panel is held to its own area: the preview of a large code ran
        // over both of its neighbours.
        f.clip(Rect::new(0.0, content_y, LEFT_PANEL_WIDTH, content_h));
        self.render_input_panel(&mut f, content_y, content_h);
        f.unclip();

        let center_w = (width - LEFT_PANEL_WIDTH - RIGHT_PANEL_WIDTH).max(0.0);
        f.clip(Rect::new(LEFT_PANEL_WIDTH, content_y, center_w, content_h));
        self.render_preview_panel(&mut f, LEFT_PANEL_WIDTH, content_y, center_w, content_h);
        f.unclip();

        let right_x = width - RIGHT_PANEL_WIDTH;
        f.clip(Rect::new(right_x, content_y, RIGHT_PANEL_WIDTH, content_h));
        self.render_options_panel(&mut f, right_x, content_y, RIGHT_PANEL_WIDTH, content_h);
        f.unclip();

        if let Some((_, dialog)) = &self.color_dialog {
            f.extend(dialog.render(&self.palette, width, height));
        }
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));
        }
        // Last, so it is above everything.
        f.extend(self.picker.render(&self.palette, width, height));
        f
    }

    /// A button, lit while the pointer is on it; one with nothing to do is
    /// drawn dim and records no box.
    fn button(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        label: &str,
        target: Target,
        enabled: bool,
    ) {
        let lit = enabled && self.hover == Some(target);
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if lit {
                self.palette.surface2
            } else {
                self.palette.surface1
            },
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        f.push(RenderCommand::Text {
            x: text::center_x(label, rect.x + rect.w / 2.0, 11.0, FontWeightHint::Regular)
                .max(rect.x + 4.0),
            y: rect.y + (rect.h - 11.0) / 2.0,
            text: label.to_owned(),
            color: if enabled {
                self.palette.text
            } else {
                self.palette.overlay0
            },
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((rect.w - 8.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if enabled {
            f.hit(target, rect);
        }
    }

    /// One of a set of choices, drawn chosen or not, lit under the pointer.
    fn choice(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        label: &str,
        target: Target,
        (chosen, ink): (bool, Color),
    ) {
        self.plate(f, rect, chosen, self.hover == Some(target));
        f.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + (rect.h - 10.0) / 2.0,
            text: label.to_owned(),
            color: if chosen { ink } else { self.palette.subtext0 },
            font_size: 10.0,
            font_weight: if chosen {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some((rect.w - 12.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(target, rect);
    }

    /// The ground of a row or a choice: chosen, lit under the pointer, or
    /// neither.
    fn plate(&self, f: &mut Frame<Target>, rect: Rect, chosen: bool, lit: bool) {
        if lit && !chosen {
            f.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: self.palette.surface1,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            return;
        }
        self.palette.push_surface(
            f,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            CORNER_RADIUS,
            if chosen {
                Surface::Selected
            } else {
                Surface::Card
            },
        );
    }

    /// A small heading.
    fn heading(&self, f: &mut Frame<Target>, x: f32, y: f32, text: String, width: f32) {
        f.push(RenderCommand::Text {
            x,
            y,
            text,
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// A line of quiet text.
    fn note(&self, f: &mut Frame<Target>, x: f32, y: f32, text: String, width: f32) {
        f.push(RenderCommand::Text {
            x,
            y,
            text,
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_toolbar(&self, f: &mut Frame<Target>, width: f32) {
        self.palette.push_surface(
            f,
            0.0,
            0.0,
            width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        f.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "QR Code Generator".to_owned(),
            color: self.palette.ink(self.palette.blue),
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(180.0),
            overflow: TextOverflow::Ellipsis,
        });

        // What kind of code to make.
        let mut tx = 200.0;
        for ct in [CodeType::QrCode, CodeType::Barcode128] {
            let btn_w = text::padded_width(ct.label(), 10.0, 11.0, FontWeightHint::Regular);
            self.choice(
                f,
                Rect::new(tx, 8.0, btn_w, 24.0),
                ct.label(),
                Target::CodeType(ct),
                (ct == self.code_type, self.palette.ink(self.palette.blue)),
            );
            tx += btn_w + 4.0;
        }

        // Saving, and the keys. There was a Generate button here that did
        // nothing: typing already generates.
        let has_code = self.current_qr.is_some() || self.current_barcode.is_some();
        let save = Rect::new(width - 12.0 - 100.0, 8.0, 100.0, 24.0);
        self.button(f, save, "Save SVG\u{2026}", Target::Save, has_code);
        self.button(
            f,
            Rect::new(save.x - 8.0 - 84.0, 8.0, 84.0, 24.0),
            "Keys (F1)",
            Target::Help,
            true,
        );

        f.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: width,
            y2: TOOLBAR_HEIGHT,
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    fn render_status_bar(&self, f: &mut Frame<Target>, width: f32, height: f32) {
        let bar_y = height - STATUS_BAR_HEIGHT;
        self.palette.push_surface(
            f,
            0.0,
            bar_y,
            width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );

        let (status, status_color) = if let Some(err) = &self.error_message {
            (err.clone(), self.palette.ink(self.palette.red))
        } else if let Some(notice) = &self.notice {
            (notice.clone(), self.palette.subtext0)
        } else if let Some(qr) = &self.current_qr {
            (
                format!(
                    "QR v{} | EC: {} | Mask: {} | Size: {}x{} | {} bytes",
                    qr.version,
                    qr.ec_level.short_label(),
                    qr.mask_pattern,
                    qr.size(),
                    qr.size(),
                    qr.data_len,
                ),
                self.palette.subtext0,
            )
        } else if let Some(bc) = &self.current_barcode {
            (
                format!(
                    "Code128 | Width: {} modules | Data: {} chars",
                    bc.width(),
                    bc.data.len()
                ),
                self.palette.subtext0,
            )
        } else {
            (
                self.waiting_for.unwrap_or(NOTHING_TYPED).to_owned(),
                self.palette.subtext0,
            )
        };
        f.push(RenderCommand::Text {
            x: 12.0,
            y: bar_y + 6.0,
            text: status,
            color: status_color,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((width - 24.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Where the kind buttons go, wrapping onto a second row as they must;
    /// and the height they take.
    fn mode_rects(top: f32) -> (Vec<(InputMode, Rect)>, f32) {
        let lx = 12.0;
        let mut x = lx;
        let mut y = top;
        let mut rects = Vec::new();
        for mode in InputMode::all() {
            let w = text::padded_width(mode.label(), 8.0, 11.0, FontWeightHint::Regular);
            if x + w > LEFT_PANEL_WIDTH - 12.0 {
                x = lx;
                y += 26.0;
            }
            rects.push((*mode, Rect::new(x, y, w, 22.0)));
            x += w + 4.0;
        }
        (rects, y + 22.0 - top)
    }

    /// The left panel's layout: where each box goes, and the history pane.
    fn input_layout(&self) -> (Vec<(Field, Rect)>, Rect) {
        let top = TOOLBAR_HEIGHT;
        let height = (self.window_height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT).max(0.0);
        let (_, modes_h) = Self::mode_rects(top + 30.0);
        let mut y = top + 30.0 + modes_h + 16.0;
        let mut boxes = Vec::new();
        for field in self.input_mode.fields() {
            boxes.push((
                *field,
                Rect::new(12.0, y + 14.0, LEFT_PANEL_WIDTH - 24.0, 28.0),
            ));
            y += 50.0;
        }
        if self.input_mode == InputMode::Wifi {
            y += 32.0;
        }
        // The history's heading, then its rows to the panel's foot.
        let list_top = y + 12.0 + 24.0;
        let pane = Rect::new(
            12.0,
            list_top,
            LEFT_PANEL_WIDTH - 24.0,
            (top + height - 8.0 - list_top).max(0.0),
        );
        (boxes, pane)
    }

    /// What a box is called: the one box the text-shaped kinds share is
    /// named for what it holds.
    fn field_caption(&self, field: Field) -> &'static str {
        match (field, self.input_mode) {
            (Field::Text, InputMode::Url) => "Web address",
            (Field::Text, InputMode::Email) => "Email address",
            (Field::Text, InputMode::Phone) => "Phone number",
            (Field::Text, _) => "Text",
            (Field::Ssid, _) => "Network name (SSID)",
            (Field::Password, _) => "Password",
            _ => field.label(),
        }
    }

    /// What an empty box says it wants.
    fn placeholder(&self, field: Field) -> &'static str {
        match (field, self.input_mode) {
            (Field::Text, InputMode::Url) => "example.com/page",
            (Field::Text, InputMode::Email) => "name@example.com",
            (Field::Text, InputMode::Phone) => "+1 555 0100",
            (Field::Text, _) => "Type or paste what to encode",
            (Field::Password, _) => "Empty for an open network",
            (Field::Ssid, _) => "The network's name",
            _ => "Optional",
        }
    }

    fn render_input_panel(&self, f: &mut Frame<Target>, y: f32, height: f32) {
        self.palette
            .push_surface(f, 0.0, y, LEFT_PANEL_WIDTH, height, 0.0, Surface::Card);
        f.push(RenderCommand::Line {
            x1: LEFT_PANEL_WIDTH,
            y1: y,
            x2: LEFT_PANEL_WIDTH,
            y2: y + height,
            color: self.palette.surface0,
            width: 1.0,
        });

        let lx = 12.0;
        let max_w = LEFT_PANEL_WIDTH - 24.0;
        self.heading(f, lx, y + 12.0, String::from("WHAT TO ENCODE"), max_w);
        let (modes, _) = Self::mode_rects(y + 30.0);
        for (mode, rect) in modes {
            self.choice(
                f,
                rect,
                mode.label(),
                Target::Mode(mode),
                (
                    mode == self.input_mode,
                    self.palette.ink(self.palette.lavender),
                ),
            );
        }

        let (boxes, pane) = self.input_layout();
        let focused = self.focused_field();
        for (field, rect) in &boxes {
            self.render_box(f, *field, *rect, *field == focused);
        }

        if self.input_mode == InputMode::Wifi {
            // What the WiFi code says about the network: `format_qr_data`
            // writes `T:` from one and `H:true` from the other.
            let row_y = boxes.last().map_or(y + 100.0, |(_, r)| r.bottom() + 12.0);
            let half = (max_w - 6.0) / 2.0;
            self.button(
                f,
                Rect::new(lx, row_y, half, 24.0),
                &format!("Security: {}", self.wifi_config.encryption.label()),
                Target::Encryption,
                true,
            );
            self.button(
                f,
                Rect::new(lx + half + 6.0, row_y, half, 24.0),
                if self.wifi_config.hidden {
                    "Hidden network: yes"
                } else {
                    "Hidden network: no"
                },
                Target::Hidden,
                true,
            );
        }

        // The history, newest first.
        let heading_y = pane.y - 24.0;
        self.heading(
            f,
            lx,
            heading_y + 4.0,
            format!("HISTORY ({})", self.history.len()),
            max_w - 80.0,
        );
        self.button(
            f,
            Rect::new(lx + max_w - 70.0, heading_y, 70.0, 20.0),
            "Forget",
            Target::ClearHistory,
            !self.history.is_empty(),
        );
        f.hit(Target::HistoryList, pane);
        if self.history.is_empty() {
            self.note(
                f,
                lx,
                pane.y + 4.0,
                String::from("Each code made is kept here; a press brings one back."),
                max_w,
            );
        }
        let rows = self.history_rows();
        for (shown, index) in (0..self.history.len())
            .rev()
            .skip(self.history_scroll)
            .take(rows.saturating_add(1))
            .enumerate()
        {
            let Some(entry) = self.history.get(index) else {
                continue;
            };
            let row = Rect::new(lx, pane.y + shown as f32 * HISTORY_ROW_H, max_w, 24.0);
            let target = Target::HistoryRow(index);
            self.plate(f, row, false, self.hover == Some(target));
            let mode_color = match entry.mode {
                InputMode::Text => self.palette.text,
                InputMode::Url => self.palette.blue,
                InputMode::Email => self.palette.peach,
                InputMode::Phone => self.palette.green,
                InputMode::Wifi => self.palette.teal,
                InputMode::VCard => self.palette.lavender,
            };
            f.push(RenderCommand::FillRect {
                x: row.x + 4.0,
                y: row.y + 8.0,
                width: 8.0,
                height: 8.0,
                color: mode_color,
                corner_radii: CornerRadii::all(4.0),
            });
            // One line, however many the data has: a vCard is seven.
            let one_line: String = entry
                .data
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            f.push(RenderCommand::Text {
                x: row.x + 18.0,
                y: row.y + 6.0,
                text: format!(
                    "{}{}",
                    if entry.code_type == CodeType::Barcode128 {
                        "Code128: "
                    } else {
                        ""
                    },
                    one_line
                ),
                color: self.palette.subtext1,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(row.w - 26.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(target, row);
        }
    }

    /// A box to type into: its caption, and what it holds with the caret
    /// when the keys are in it.
    fn render_box(&self, f: &mut Frame<Target>, field: Field, rect: Rect, focused: bool) {
        f.push(RenderCommand::Text {
            x: rect.x,
            y: rect.y - 14.0,
            text: self.field_caption(field).to_owned(),
            color: if focused {
                self.palette.ink(self.palette.blue)
            } else {
                self.palette.subtext0
            },
            font_size: 10.0,
            font_weight: if focused {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(rect.w),
            overflow: TextOverflow::Ellipsis,
        });
        let mut paint = self.palette.surface_paint(Surface::Card);
        paint.border = Some(if focused {
            self.palette.blue
        } else {
            paint.border.unwrap_or(self.palette.surface2)
        });
        self.palette.push_paint_radii(
            f,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            CornerRadii::all(CORNER_RADIUS),
            paint,
        );
        let value = if focused && self.editor.text() == self.field_text(field) {
            self.editor.text()
        } else {
            self.field_text(field)
        };
        if value.is_empty() && !focused {
            f.push(RenderCommand::Text {
                x: rect.x + 8.0,
                y: rect.y + 8.0,
                text: self.placeholder(field).to_owned(),
                color: self.palette.subtext0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((rect.w - 16.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            let editing = focused && self.editor.text() == value;
            let mut tree = RenderTree::new();
            textedit::draw(
                &mut tree,
                &textedit::SingleLine {
                    text: value,
                    cursor: if editing {
                        self.editor.cursor()
                    } else {
                        TextCursor::from(value.len())
                    },
                    selection_anchor: if editing {
                        self.editor.selection_anchor()
                    } else {
                        None
                    },
                    focused,
                    x: rect.x + 8.0,
                    y: rect.y + 5.0,
                    width: (rect.w - 16.0).max(0.0),
                    line_height: 18.0,
                    font_size: 12.0,
                    weight: FontWeightHint::Regular,
                    color: self.palette.text,
                    selection_bg: self.palette.blue,
                    selection_fg: self.palette.crust,
                    caret_width: textedit::CARET_WIDTH,
                },
            );
            f.extend(tree.commands);
        }
        // A press puts the keys in the box, and the caret under the pointer.
        f.hit(Target::Field(field), rect);
    }

    fn render_preview_panel(&self, f: &mut Frame<Target>, x: f32, y: f32, width: f32, height: f32) {
        let lx = x + 12.0;
        let max_w = (width - 24.0).max(0.0);
        let cy = y + 12.0;
        self.heading(f, lx, cy, String::from("PREVIEW"), max_w);

        // Room for the code, under the heading and above its info line and
        // the colour warning.
        let room_h = (height - 22.0 - 12.0 - 60.0).max(0.0);
        let bottom = if let Some(qr) = &self.current_qr {
            self.render_qr_preview(f, qr, lx, cy + 22.0, max_w, room_h)
        } else if let Some(barcode) = &self.current_barcode {
            self.render_barcode_preview(f, barcode, lx, cy + 22.0, max_w)
        } else {
            let (w, h) = (max_w.min(260.0), 160.0_f32.min(room_h.max(60.0)));
            let px = lx + (max_w - w) / 2.0;
            let py = cy + 40.0;
            self.palette
                .push_surface(f, px, py, w, h, 8.0, Surface::Card);
            f.push(RenderCommand::StrokeRect {
                x: px,
                y: py,
                width: w,
                height: h,
                color: self.palette.surface2,
                line_width: 2.0,
                corner_radii: CornerRadii::all(8.0),
            });
            f.push(RenderCommand::Text {
                x: px + 16.0,
                y: py + h / 2.0 - 16.0,
                text: "No code yet".to_owned(),
                color: self.palette.subtext0,
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some((w - 32.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            let why = self
                .error_message
                .clone()
                .unwrap_or_else(|| self.waiting_for.unwrap_or(NOTHING_TYPED).to_owned());
            let lines = text::wrap(&why, (w - 32.0).max(1.0), 10.0, FontWeightHint::Regular);
            for (i, line) in lines.iter().take(4).enumerate() {
                f.push(RenderCommand::Text {
                    x: px + 16.0,
                    y: py + h / 2.0 + 4.0 + i as f32 * 14.0,
                    text: line.clone(),
                    color: self.palette.subtext0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some((w - 32.0).max(0.0)),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            py + h
        };

        if (self.current_qr.is_some() || self.current_barcode.is_some()) && !self.colors_scannable()
        {
            f.push(RenderCommand::Text {
                x: lx,
                y: bottom + 28.0,
                text: String::from(
                    "Scanners may not read this: the squares need to be much darker than the ground.",
                ),
                color: self.palette.ink(self.palette.yellow),
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(max_w),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// The QR code at its module size, or smaller when that would not fit;
    /// answers where it ends.
    fn render_qr_preview(
        &self,
        f: &mut Frame<Target>,
        qr: &QrCode,
        panel_x: f32,
        panel_y: f32,
        panel_w: f32,
        panel_h: f32,
    ) -> f32 {
        let qr_size = qr.size();
        let quiet_zone = 4; // 4-module quiet zone
        let total_modules = qr_size + quiet_zone * 2;
        let fit = (panel_w.min(panel_h - 20.0) / total_modules as f32)
            .floor()
            .max(1.0);
        let module_px = self.module_size.pixels().min(fit);
        let total_px = total_modules as f32 * module_px;

        let qr_x = panel_x + ((panel_w - total_px) / 2.0).max(0.0);
        let qr_y = panel_y + 20.0;

        f.push(RenderCommand::FillRect {
            x: qr_x,
            y: qr_y,
            width: total_px,
            height: total_px,
            color: self.bg_color,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        for row in 0..qr_size {
            for col in 0..qr_size {
                if qr.is_dark(row, col) {
                    f.push(RenderCommand::FillRect {
                        x: qr_x + (col + quiet_zone) as f32 * module_px,
                        y: qr_y + (row + quiet_zone) as f32 * module_px,
                        width: module_px,
                        height: module_px,
                        color: self.fg_color,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }

        let info_y = qr_y + total_px + 12.0;
        f.push(RenderCommand::Text {
            x: panel_x,
            y: info_y,
            text: format!(
                "Version {} | {} | {}x{} modules{}",
                qr.version,
                qr.ec_level.label(),
                qr_size,
                qr_size,
                if module_px < self.module_size.pixels() {
                    " | shown smaller to fit"
                } else {
                    ""
                },
            ),
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_w),
            overflow: TextOverflow::Ellipsis,
        });
        info_y
    }

    /// The barcode, as wide as fits; answers where it ends.
    fn render_barcode_preview(
        &self,
        f: &mut Frame<Target>,
        barcode: &Code128Barcode,
        panel_x: f32,
        panel_y: f32,
        panel_w: f32,
    ) -> f32 {
        let bar_width = (panel_w / barcode.width().max(1) as f32)
            .floor()
            .clamp(1.0, 2.0);
        let bar_height = 80.0_f32;
        let total_w = barcode.width() as f32 * bar_width;

        let bc_x = panel_x + ((panel_w - total_w) / 2.0).max(0.0);
        let bc_y = panel_y + 40.0;

        f.push(RenderCommand::FillRect {
            x: bc_x - 10.0,
            y: bc_y - 10.0,
            width: total_w + 20.0,
            height: bar_height + 40.0,
            color: self.bg_color,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        for (i, &is_bar) in barcode.bars.iter().enumerate() {
            if is_bar {
                f.push(RenderCommand::FillRect {
                    x: bc_x + i as f32 * bar_width,
                    y: bc_y,
                    width: bar_width,
                    height: bar_height,
                    color: self.fg_color,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
        // In the code's own ink: it was always black, and so invisible on a
        // dark ground.
        f.push(RenderCommand::Text {
            x: bc_x,
            y: bc_y + bar_height + 8.0,
            text: barcode.data.clone(),
            color: self.fg_color,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(total_w),
            overflow: TextOverflow::Ellipsis,
        });

        let info_y = bc_y + bar_height + 40.0;
        f.push(RenderCommand::Text {
            x: panel_x,
            y: info_y,
            text: format!(
                "Code128 | {} modules wide{}",
                barcode.width(),
                if total_w > panel_w {
                    " | wider than this window"
                } else {
                    ""
                }
            ),
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_w),
            overflow: TextOverflow::Ellipsis,
        });
        info_y
    }

    fn render_options_panel(&self, f: &mut Frame<Target>, x: f32, y: f32, width: f32, height: f32) {
        self.palette
            .push_surface(f, x, y, width, height, 0.0, Surface::Card);
        f.push(RenderCommand::Line {
            x1: x,
            y1: y,
            x2: x,
            y2: y + height,
            color: self.palette.surface0,
            width: 1.0,
        });

        let lx = x + 12.0;
        let max_w = width - 24.0;
        let mut cy = y + 12.0;

        self.heading(f, lx, cy, String::from("ERROR CORRECTION"), max_w);
        cy += 18.0;
        for ec in EcLevel::all() {
            self.choice(
                f,
                Rect::new(lx, cy, max_w, 22.0),
                ec.label(),
                Target::Ec(*ec),
                (*ec == self.ec_level, self.palette.ink(self.palette.green)),
            );
            cy += 26.0;
        }

        cy += 8.0;
        self.heading(f, lx, cy, String::from("MODULE SIZE"), max_w);
        cy += 18.0;
        for ms in ModuleSize::all() {
            self.choice(
                f,
                Rect::new(lx, cy, max_w, 22.0),
                ms.label(),
                Target::Size(*ms),
                (
                    *ms == self.module_size,
                    self.palette.ink(self.palette.yellow),
                ),
            );
            cy += 26.0;
        }

        // The colours: a press opens the colour dialog. They were drawn as
        // swatches that nothing could change.
        cy += 8.0;
        self.heading(f, lx, cy, String::from("COLOURS"), max_w);
        cy += 18.0;
        for (label, color, target) in [
            ("Squares", self.fg_color, Target::Foreground),
            ("Ground", self.bg_color, Target::Background),
        ] {
            let row = Rect::new(lx, cy, max_w, 22.0);
            self.plate(f, row, false, self.hover == Some(target));
            f.push(RenderCommand::Text {
                x: row.x + 8.0,
                y: row.y + 6.0,
                text: label.to_owned(),
                color: self.palette.subtext0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w - 40.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::FillRect {
                x: row.right() - 28.0,
                y: row.y + 4.0,
                width: 20.0,
                height: 14.0,
                color,
                corner_radii: CornerRadii::all(2.0),
            });
            f.push(RenderCommand::StrokeRect {
                x: row.right() - 28.0,
                y: row.y + 4.0,
                width: 20.0,
                height: 14.0,
                color: self.palette.surface2,
                line_width: 1.0,
                corner_radii: CornerRadii::all(2.0),
            });
            f.hit(target, row);
            cy += 26.0;
        }
        self.button(
            f,
            Rect::new(lx, cy, max_w, 22.0),
            "Black on white",
            Target::ResetColors,
            (self.fg_color, self.bg_color) != (Color::BLACK, Color::WHITE),
        );
        cy += 34.0;

        self.heading(f, lx, cy, String::from("INFO"), max_w);
        cy += 18.0;
        let info_lines: Vec<String> = if let Some(qr) = &self.current_qr {
            vec![
                format!("Version: {}", qr.version),
                format!("Size: {}x{}", qr.size(), qr.size()),
                format!("EC Level: {}", qr.ec_level.label()),
                format!("Mask: {}", qr.mask_pattern),
                format!("Data: {} bytes", qr.data_len),
            ]
        } else if let Some(bc) = &self.current_barcode {
            vec![
                "Type: Code128".to_string(),
                format!("Width: {} modules", bc.width()),
                format!("Data: {} chars", bc.data.len()),
            ]
        } else {
            vec![String::from("No code yet")]
        };
        for line in info_lines {
            f.push(RenderCommand::Text {
                x: lx,
                y: cy,
                text: line,
                color: self.palette.text,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 16.0;
        }
    }
}

impl App for QrApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        match self.code_type {
            CodeType::QrCode => "QR Code".to_owned(),
            CodeType::Barcode128 => "Barcode".to_owned(),
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.window_width as u32, self.window_height as u32)
        }
    }

    /// No clock.
    ///
    /// A code is produced when the text changes. Nothing here ages, so a tick
    /// would redraw an identical frame.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Reconciled with the size we are handed rather than trusted from the
        // last `Resize`: the compositor may grant a size that was never asked
        // for, and the first frame is drawn before any `Resize` arrives.
        self.window_width = width;
        self.window_height = height;
        let frame = self.frame(width, height);
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }
}

/// What the preview and the status bar say before anything is typed.
const NOTHING_TYPED: &str = "Nothing to encode yet: type or paste into the box on the left";

/// A colour as SVG paint: `fill="#rrggbb"`, and its opacity when it has one.
fn svg_paint(c: Color) -> String {
    let rgb = format!("fill=\"#{:02x}{:02x}{:02x}\"", c.r, c.g, c.b);
    if c.a == 255 {
        rgb
    } else {
        format!("{rgb} fill-opacity=\"{:.3}\"", f32::from(c.a) / 255.0)
    }
}

/// Text made safe to put in XML.
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// A colour's relative luminance, as WCAG defines it.
fn luminance(c: Color) -> f32 {
    let channel = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

fn main() -> ExitCode {
    // Empty, and saying why: it opened on a code for "Hello, Slate OS!",
    // with that in its history, which the user had not made.
    app::launch("qrcode", &mut QrApp::new())
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

    // ------------------------------------------------------------------
    // Events
    //
    // The app had no input handling at all until it was wired to the
    // compositor: the text to encode was set by a caller.
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn press_ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        })
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        })
    }

    #[test]
    fn typing_encodes_without_a_keystroke_to_get_started() {
        // The text field always has focus: a program whose entire input is
        // "the thing to encode" should not need a key pressed first.
        let mut app = QrApp::new();
        for c in "HI".chars() {
            assert_eq!(app.handle_event(&typed(c)), EventResult::Consumed);
        }
        assert_eq!(app.input_text, "HI");
        assert!(
            app.current_qr.is_some(),
            "typing should have produced a code"
        );
    }

    #[test]
    fn backspace_re_encodes_and_stops_at_an_empty_field() {
        let mut app = QrApp::new();
        for c in "HI".chars() {
            app.handle_event(&typed(c));
        }
        assert_eq!(
            app.handle_event(&press(Key::Backspace)),
            EventResult::Consumed
        );
        assert_eq!(app.input_text, "H");
        app.handle_event(&press(Key::Backspace));
        assert_eq!(app.input_text, "");
        assert_eq!(
            app.handle_event(&press(Key::Backspace)),
            EventResult::Ignored,
            "backspace on an empty field is not a redraw"
        );
    }

    #[test]
    fn escape_clears_the_field_and_does_nothing_when_it_is_clear() {
        let mut app = QrApp::new();
        assert_eq!(app.handle_event(&press(Key::Escape)), EventResult::Ignored);
        app.handle_event(&typed('X'));
        assert_eq!(app.handle_event(&press(Key::Escape)), EventResult::Consumed);
        assert!(app.input_text.is_empty());
    }

    #[test]
    fn ctrl_b_makes_a_barcode_from_the_same_text_and_ctrl_q_a_qr_code() {
        let mut app = QrApp::new();
        for c in "ABC123".chars() {
            app.handle_event(&typed(c));
        }
        assert!(app.current_qr.is_some());
        assert_eq!(app.handle_event(&press_ctrl(Key::B)), EventResult::Consumed);
        assert_eq!(app.code_type, CodeType::Barcode128);
        assert!(
            app.current_barcode.is_some(),
            "switching should have re-encoded the same text as a barcode"
        );
        assert_eq!(app.input_text, "ABC123", "the text should be untouched");
        // Asking again for the kind already showing is not a redraw.
        assert_eq!(app.handle_event(&press_ctrl(Key::B)), EventResult::Ignored);
        assert_eq!(app.handle_event(&press_ctrl(Key::Q)), EventResult::Consumed);
        assert_eq!(app.code_type, CodeType::QrCode);
    }

    #[test]
    fn a_ctrl_chord_is_not_typed_into_the_field() {
        // Every bare printable key is text, so a chord that carried one would
        // otherwise be encoded along with it.
        let mut app = QrApp::new();
        let chord = Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: "q".to_owned(),
        });
        assert_eq!(app.handle_event(&chord), EventResult::Ignored);
        assert!(
            app.input_text.is_empty(),
            "a chord was typed into the field"
        );
    }

    #[test]
    fn cycling_the_error_correction_level_comes_back_round() {
        let mut app = QrApp::new();
        app.handle_event(&typed('A'));
        let first = app.ec_level;
        let mut seen = vec![first];
        for _ in 0..3 {
            assert_eq!(app.handle_event(&press_ctrl(Key::E)), EventResult::Consumed);
            seen.push(app.ec_level);
        }
        for (i, a) in seen.iter().enumerate() {
            for b in seen.iter().skip(i + 1) {
                assert_ne!(a, b, "the cycle repeated before visiting all four");
            }
        }
        app.handle_event(&press_ctrl(Key::E));
        assert_eq!(app.ec_level, first, "the cycle should return to the start");
    }

    #[test]
    fn the_module_size_key_does_not_re_encode() {
        // How big each square is drawn is a rendering choice; the code itself
        // is the same, and regenerating would be work for nothing.
        let mut app = QrApp::new();
        app.handle_event(&typed('A'));
        let before = app.history.len();
        let size = app.module_size;
        assert_eq!(app.handle_event(&press_ctrl(Key::M)), EventResult::Consumed);
        assert_ne!(app.module_size, size);
        assert_eq!(app.history.len(), before, "changing the size re-encoded");
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = QrApp::new();
        let release = Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: false,
            modifiers: Modifiers::NONE,
            text: "z".to_owned(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert!(app.input_text.is_empty());
    }

    #[test]
    fn the_title_says_which_kind_of_code_is_showing() {
        let mut app = QrApp::new();
        assert!(app.title().contains("QR"));
        app.handle_event(&press_ctrl(Key::B));
        assert!(app.title().contains("Barcode"), "title: {:?}", app.title());
    }

    #[test]
    fn rendering_draws_something_at_an_awkward_size() {
        let mut app = QrApp::new();
        app.set_input("Hello");
        app.generate();
        for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
            assert!(
                !app.render(w, h).commands.is_empty(),
                "drew nothing at {w}x{h}"
            );
        }
    }

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

    // --- Version table consistency ---

    #[test]
    fn every_version_row_agrees_with_its_own_capacity() {
        // The table states the byte-mode capacity outright *and* implies it
        // from the block structure. Nothing compared the two, and sixteen of
        // the forty rows disagreed: v5-Q through v10-H carried block counts
        // copied down from the row above rather than the spec's own.
        //
        // The consequence was not a panic. `encode_data_bits` padded to
        // `total - ec_per_block * num_blocks` -- 98 data codewords for v5-Q
        // instead of 62 -- and `apply_error_correction` then added exactly
        // enough EC to reach `total_codewords`, so the symbol filled the
        // matrix and looked right. It simply was not the codeword layout any
        // conforming decoder de-interleaves, so it did not scan.
        for version in 1..=10_u8 {
            for ec in EcLevel::all() {
                let Some(info) = get_version_info(version, *ec) else {
                    panic!("missing table row for v{version}-{ec:?}");
                };
                assert!(
                    info.num_blocks >= 1,
                    "v{version}-{ec:?}: zero blocks would divide by zero"
                );
                assert!(
                    info.ec_codewords_per_block.saturating_mul(info.num_blocks)
                        < info.total_codewords,
                    "v{version}-{ec:?}: error correction claims the whole symbol"
                );
                assert_eq!(
                    info.byte_mode_capacity(),
                    info.data_capacity_bytes,
                    "v{version}-{ec:?}: {} data codewords in {} blocks of {} EC \
                     imply {} payload bytes, but the table says {}",
                    info.data_codewords(),
                    info.num_blocks,
                    info.ec_codewords_per_block,
                    info.byte_mode_capacity(),
                    info.data_capacity_bytes,
                );
            }
        }
    }

    #[test]
    fn a_full_symbol_is_exactly_total_codewords_at_every_version() {
        // The data half and the EC half are computed by different functions
        // from the same table row. If they read different numbers out of it,
        // the interleaved result is the wrong length for the matrix -- which
        // is what a wrong `num_blocks` used to cause everywhere except that
        // the two errors happened to cancel in the total.
        for version in 1..=10_u8 {
            for ec in EcLevel::all() {
                let Some(info) = get_version_info(version, *ec) else {
                    continue;
                };
                let payload = vec![b'A'; info.data_capacity_bytes];
                let Some(data) = encode_data_bits(&payload, version, *ec) else {
                    panic!("v{version}-{ec:?}: full-capacity payload did not encode");
                };
                assert_eq!(
                    data.len(),
                    info.data_codewords(),
                    "v{version}-{ec:?}: padded to the wrong data codeword count"
                );
                let Some(full) = apply_error_correction(&data, version, *ec) else {
                    panic!("v{version}-{ec:?}: error correction failed");
                };
                assert_eq!(
                    full.len(),
                    info.total_codewords,
                    "v{version}-{ec:?}: symbol is not the version's codeword count"
                );
            }
        }
    }

    #[test]
    fn block_sizes_follow_the_specs_two_groups() {
        // The spec splits the data into `n - extra` blocks of `c` codewords
        // and `extra` blocks of `c + 1`, smaller group first. Everything
        // downstream -- the interleave, and every decoder -- assumes it.
        for version in 1..=10_u8 {
            for ec in EcLevel::all() {
                let Some(info) = get_version_info(version, *ec) else {
                    continue;
                };
                let d = info.data_codewords();
                let n = info.num_blocks;
                let base = d / n;
                let extra = d % n;
                assert!(
                    base >= 1,
                    "v{version}-{ec:?}: {d} data codewords do not fill {n} blocks"
                );
                assert_eq!(
                    base * (n - extra) + (base + 1) * extra,
                    d,
                    "v{version}-{ec:?}: the two groups do not account for every codeword"
                );
            }
        }
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

    /// Nothing typed is not an error, and it is not a code either.
    #[test]
    fn test_app_generate_empty() {
        let mut app = QrApp::new();
        app.generate();
        assert!(app.error_message.is_none(), "an empty box is not an error");
        assert_eq!(app.waiting_for, Some(NOTHING_TYPED));
        assert!(app.current_qr.is_none() && app.current_barcode.is_none());
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
        let cmds = app.render_commands(1100.0, 700.0);
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

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut QrApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = QrApp::new();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // ------------------------------------------------------------------
    // The input modes, and the boxes they need
    // ------------------------------------------------------------------

    /// States a shortcut might need to be able to answer in.
    fn help_states() -> Vec<QrApp> {
        let typed = || {
            let mut app = QrApp::new();
            app.set_input("hello");
            app.generate();
            app
        };
        // With history, so `Ctrl+K` has something to forget.
        let mut with_history = typed();
        with_history.generate();
        // In WiFi mode, so `Ctrl+T`, `Ctrl+H` and `Tab` have a reason to act.
        let mut wifi = QrApp::new();
        wifi.set_input_mode(InputMode::Wifi);
        wifi.wifi_config.ssid = String::from("net");
        // With the list up, so `Escape` has something to close.
        let mut helping = QrApp::new();
        helping.show_help = true;
        // Making a barcode, so `Ctrl+Q` has somewhere to switch *to*. It
        // declines when the program is already making what it asks for, and
        // every other state here is already a QR code -- which is what the
        // guard caught.
        let mut barcode = typed();
        barcode.set_code_type(CodeType::Barcode128);
        // The caret in the middle of the box, so every movement moves it and
        // Delete has a character in front of it.
        let mut middle = typed();
        middle.handle_key(&press_key(Key::Left));
        middle.handle_key(&press_key(Key::Left));
        // Everything selected and something on the clipboard, for Ctrl+C,
        // Ctrl+X and Ctrl+V.
        let mut selected = typed();
        selected.handle_key(&ctrl_key(Key::A));
        selected.clipboard = String::from("pasted");
        vec![
            typed(),
            with_history,
            wifi,
            helping,
            barcode,
            middle,
            selected,
        ]
    }

    /// Every key the list advertises does something somewhere.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = help_states()
                    .iter_mut()
                    .any(|app| app.handle_key(&stroke) == EventResult::Consumed);
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// And the list reaches the window.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = QrApp::new();
        assert!(
            !drawn_text(&app).contains("F1 closes this"),
            "the list is up before anybody asked for it"
        );

        app.handle_key(&press_key(Key::F1));
        let shown = drawn_text(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        app.handle_key(&press_key(Key::Escape));
        assert!(
            !drawn_text(&app).contains("F1 closes this"),
            "Escape did not close it"
        );
    }

    /// Every input mode can be reached, and each encodes differently.
    ///
    /// Six modes were drawn as a row of buttons with the active one
    /// highlighted and nothing could move the highlight, so `format_qr_data`'s
    /// Url, Email, Phone, Wifi and VCard arms -- all written, all tested --
    /// encoded nothing for anybody.
    #[test]
    fn every_input_mode_can_be_reached_and_encodes_its_own_way() {
        let mut app = QrApp::new();
        let mut seen = vec![app.input_mode];
        for _ in 1..InputMode::all().len() {
            assert_eq!(
                app.handle_key(&ctrl_key(Key::I)),
                EventResult::Consumed,
                "Ctrl+I was ignored"
            );
            seen.push(app.input_mode);
        }
        for mode in InputMode::all() {
            assert!(seen.contains(mode), "Ctrl+I never reached {}", mode.label());
        }
        app.handle_key(&ctrl_key(Key::I));
        assert_eq!(
            app.input_mode,
            InputMode::Text,
            "the modes do not come back round"
        );

        // The same typing encodes differently in each of the four text modes.
        let mut encoded = Vec::new();
        for mode in [
            InputMode::Text,
            InputMode::Url,
            InputMode::Email,
            InputMode::Phone,
        ] {
            let mut a = QrApp::new();
            a.set_input_mode(mode);
            for c in "example.com".chars() {
                a.handle_key(&typed_key(c));
            }
            encoded.push(format_qr_data(
                a.input_mode,
                &a.input_text,
                &a.wifi_config,
                &a.vcard_info,
            ));
        }
        for (i, one) in encoded.iter().enumerate() {
            for (j, other) in encoded.iter().enumerate() {
                assert!(
                    i == j || one != other,
                    "two modes encode the same typing identically: {one:?}"
                );
            }
        }
    }

    /// The WiFi and vCard boxes can be typed into.
    ///
    /// Making the mode row work exposed the next layer: those modes draw
    /// labelled boxes reading "Enter SSID..." and `wifi_config` and
    /// `vcard_info` had no writers, so the boxes looked editable and were
    /// not.
    #[test]
    fn every_box_a_mode_draws_can_be_typed_into() {
        for mode in InputMode::all() {
            for (i, field) in mode.fields().iter().enumerate() {
                let mut app = QrApp::new();
                app.set_input_mode(*mode);
                app.focused_field = i;
                assert_eq!(
                    app.focused_field(),
                    *field,
                    "the cursor does not land on {} in {}",
                    field.label(),
                    mode.label()
                );
                for c in "abc".chars() {
                    assert_eq!(
                        app.handle_key(&typed_key(c)),
                        EventResult::Consumed,
                        "{} in {} refused a character",
                        field.label(),
                        mode.label()
                    );
                }
                assert_eq!(
                    app.field_text(*field),
                    "abc",
                    "{} in {} did not take what was typed",
                    field.label(),
                    mode.label()
                );
                app.handle_key(&press_key(Key::Backspace));
                assert_eq!(
                    app.field_text(*field),
                    "ab",
                    "Backspace did not reach {} in {}",
                    field.label(),
                    mode.label()
                );
            }
        }
    }

    /// Tab walks the boxes of a mode that has more than one, and the drawn
    /// panel shows where it has got to.
    #[test]
    fn tab_walks_the_boxes_and_the_panel_shows_which() {
        let mut app = QrApp::new();
        app.set_input_mode(InputMode::VCard);
        let fields = InputMode::VCard.fields();
        assert!(fields.len() > 1, "control: vCard should have several boxes");

        let focused_label_is_bold = |a: &QrApp, want: Field| {
            a.render_commands(1200.0, 800.0).iter().any(|c| match c {
                RenderCommand::Text {
                    text, font_weight, ..
                } => text == want.label() && *font_weight == FontWeightHint::Bold,
                _ => false,
            })
        };
        assert!(
            focused_label_is_bold(&app, fields[0]),
            "the first box is not marked as the one being typed into"
        );

        for expected in fields.iter().skip(1) {
            assert_eq!(
                app.handle_key(&press_key(Key::Tab)),
                EventResult::Consumed,
                "Tab was ignored in vCard mode"
            );
            assert_eq!(app.focused_field(), *expected, "Tab skipped a box");
            assert!(
                focused_label_is_bold(&app, *expected),
                "{} is focused and the panel does not show it",
                expected.label()
            );
        }

        // Round the end, and back the other way.
        app.handle_key(&press_key(Key::Tab));
        assert_eq!(app.focused_field(), fields[0], "Tab does not wrap");
        let mut shift_tab = press_key(Key::Tab);
        shift_tab.modifiers.shift = true;
        app.handle_key(&shift_tab);
        assert_eq!(
            app.focused_field(),
            fields[fields.len() - 1],
            "Shift+Tab does not go back"
        );

        // A mode with one box answers Tab by declining, rather than
        // pretending to move a cursor that has nowhere to go.
        let mut single = QrApp::new();
        assert_eq!(single.input_mode.fields().len(), 1);
        assert_eq!(
            single.handle_key(&press_key(Key::Tab)),
            EventResult::Ignored,
            "Tab claimed to move a cursor in a mode with one box"
        );
    }

    /// The two WiFi settings reach the code that is produced.
    ///
    /// `format_qr_data` writes `T:WPA` or `T:nopass` from one and `H:true`
    /// from the other. Both were fixed at construction, so a code for an open
    /// network told the phone it was encrypted -- which is a connection that
    /// fails with no useful message.
    #[test]
    fn the_wifi_settings_reach_the_encoded_text() {
        let mut app = QrApp::new();
        app.set_input_mode(InputMode::Wifi);
        app.wifi_config.ssid = String::from("net");
        let encoded =
            |a: &QrApp| format_qr_data(a.input_mode, &a.input_text, &a.wifi_config, &a.vcard_info);

        assert!(encoded(&app).contains("T:WPA"), "control: WPA by default");
        app.handle_key(&ctrl_key(Key::T));
        assert!(
            !encoded(&app).contains("T:WPA"),
            "Ctrl+T did not change what the code says about encryption"
        );

        assert!(
            !encoded(&app).contains("H:true"),
            "control: not hidden by default"
        );
        app.handle_key(&ctrl_key(Key::H));
        assert!(
            encoded(&app).contains("H:true"),
            "Ctrl+H did not mark the network hidden"
        );
    }

    /// Those two keys belong to WiFi mode and decline elsewhere.
    #[test]
    fn the_wifi_keys_decline_outside_wifi_mode() {
        let mut app = QrApp::new();
        assert_eq!(app.input_mode, InputMode::Text);
        assert_eq!(
            app.handle_key(&ctrl_key(Key::T)),
            EventResult::Ignored,
            "Ctrl+T acted on a setting this mode does not show"
        );
        assert_eq!(
            app.handle_key(&ctrl_key(Key::H)),
            EventResult::Ignored,
            "Ctrl+H acted on a setting this mode does not show"
        );
    }

    fn drawn_text(app: &QrApp) -> String {
        app.render_commands(1200.0, 800.0)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn press_key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn ctrl_key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        }
    }

    fn typed_key(c: char) -> KeyEvent {
        KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        }
    }

    // ------------------------------------------------------------------
    // The pointer, the boxes, the history, saving and the colours
    // ------------------------------------------------------------------

    use guitk::probe::{self, Probe};

    impl Probe for QrApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (1100.0, 700.0);

        /// Drawn at the app's own size, which these tests leave at `SIZE`.
        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame(self.window_width, self.window_height)
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            _size: (f32, f32),
        ) -> EventResult {
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> EventResult {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<EventResult> {
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    fn texts(app: &QrApp) -> Vec<String> {
        app.frame(app.window_width, app.window_height)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn press_at(app: &mut QrApp, x: f32, y: f32) -> EventResult {
        app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }))
    }

    /// **Typing "Hello" added five entries**, "H" to "Hello": every
    /// keystroke re-encodes, and every encoding was remembered.
    #[test]
    fn typing_one_thing_leaves_one_history_entry() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "Hello");
        assert_eq!(
            app.history.len(),
            1,
            "{:?}",
            app.history.iter().map(|e| &e.data).collect::<Vec<_>>()
        );
        assert_eq!(app.history[0].data, "Hello");
        // Changing the error correction is the same code, made differently.
        app.handle_key(&ctrl_key(Key::E));
        assert_eq!(app.history.len(), 1);
        assert_eq!(app.history[0].ec_level, app.ec_level);
    }

    #[test]
    fn a_new_code_starts_a_new_entry() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "first");
        // Emptied, then typed again: a second code.
        app.handle_key(&press_key(Key::Escape));
        probe::type_str(&mut app, "second");
        assert_eq!(app.history.len(), 2);
        // Another kind, with the box emptied first: nothing to encode yet.
        app.handle_key(&press_key(Key::Escape));
        probe::click(&mut app, Target::Mode(InputMode::Url));
        assert_eq!(
            app.history.len(),
            2,
            "switching kinds with the box empty made a code"
        );
        probe::type_str(&mut app, "example.com");
        assert_eq!(app.history.len(), 3);
        assert_eq!(app.history[2].data, "https://example.com");
    }

    /// The same words as a web address are a different code, so a
    /// different entry.
    #[test]
    fn another_kind_of_the_same_words_is_a_new_entry() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "example.com");
        probe::click(&mut app, Target::Mode(InputMode::Url));
        let data: Vec<&str> = app.history.iter().map(|e| e.data.as_str()).collect();
        assert_eq!(data, ["example.com", "https://example.com"]);
    }

    /// A box changed from outside -- a restore, a caller -- is typed onto
    /// as it now stands, not as the editor last saw it.
    #[test]
    fn a_box_changed_from_outside_is_typed_onto() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "x");
        app.input_text = String::from("abc");
        probe::type_str(&mut app, "d");
        assert_eq!(app.input_text, "abcd");
    }

    /// The label under a barcode is in the code's own ink: it was black
    /// whatever the ground, so it vanished on a dark one.
    #[test]
    fn the_barcode_label_is_in_the_codes_ink() {
        let mut app = QrApp::new();
        app.fg_color = Color::rgba(0x10, 0x20, 0x60, 255);
        app.handle_key(&ctrl_key(Key::B));
        probe::type_str(&mut app, "LABEL");
        let ink = app
            .frame(app.window_width, app.window_height)
            .commands()
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == "LABEL" => Some(*color),
                _ => None,
            })
            .expect("the label is not drawn");
        assert_eq!(ink, app.fg_color);
    }

    #[test]
    fn the_same_code_twice_is_one_entry() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "again");
        app.handle_key(&press_key(Key::Escape));
        probe::type_str(&mut app, "other");
        app.handle_key(&press_key(Key::Escape));
        probe::type_str(&mut app, "again");
        let data: Vec<&str> = app.history.iter().map(|e| e.data.as_str()).collect();
        assert_eq!(data, ["other", "again"]);
    }

    #[test]
    fn a_history_row_brings_its_code_back() {
        let mut app = QrApp::new();
        probe::click(&mut app, Target::Mode(InputMode::Wifi));
        probe::type_str(&mut app, "Home");
        probe::key(&mut app, &press_key(Key::Tab));
        probe::type_str(&mut app, "secret");
        probe::click(&mut app, Target::Hidden);
        let wifi_entry = app.history.len() - 1;
        probe::click(&mut app, Target::Mode(InputMode::Text));
        probe::type_str(&mut app, "something else");
        assert_eq!(app.input_mode, InputMode::Text);
        probe::click(&mut app, Target::HistoryRow(wifi_entry));
        assert_eq!(app.input_mode, InputMode::Wifi);
        assert_eq!(app.wifi_config.ssid, "Home");
        assert_eq!(app.wifi_config.password, "secret");
        assert!(app.wifi_config.hidden);
        assert!(app.current_qr.is_some());
        let kinds: Vec<InputMode> = app.history.iter().map(|e| e.mode).collect();
        assert_eq!(
            kinds,
            [InputMode::Text, InputMode::Wifi],
            "the brought-back code is not the newest, or the text code was lost"
        );
        assert_eq!(app.history[0].data, "something else");
        // And the box has it, caret at the end, so typing adds to it.
        probe::type_str(&mut app, "2");
        assert_eq!(app.wifi_config.ssid, "Home2");
    }

    /// Emptying the box left the last code on screen: a code for text that
    /// was no longer there.
    #[test]
    fn emptying_the_box_takes_the_code_away() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "abc");
        assert!(app.current_qr.is_some());
        for _ in 0..3 {
            app.handle_key(&press_key(Key::Backspace));
        }
        assert!(app.current_qr.is_none(), "the code outlived its text");
        assert!(texts(&app).iter().any(|t| t == "No code yet"));
    }

    /// An empty web-address box encoded `https://`.
    #[test]
    fn an_empty_box_is_no_code_in_any_kind() {
        for mode in [
            InputMode::Url,
            InputMode::Email,
            InputMode::Phone,
            InputMode::VCard,
        ] {
            let mut app = QrApp::new();
            probe::click(&mut app, Target::Mode(mode));
            assert!(app.current_qr.is_none(), "{mode:?} made a code of nothing");
            assert!(app.waiting_for.is_some());
        }
        let mut app = QrApp::new();
        probe::click(&mut app, Target::Mode(InputMode::Wifi));
        app.handle_key(&press_key(Key::Tab));
        probe::type_str(&mut app, "password only");
        assert!(
            app.current_qr.is_none(),
            "a WiFi code with no network was made"
        );
        assert_eq!(
            app.waiting_for,
            Some("A WiFi code needs the network's name (SSID)")
        );
    }

    /// Code B has no value for a character outside printable ASCII; it was
    /// skipped, so the barcode scanned as something other than its label.
    #[test]
    fn a_barcode_refuses_what_it_cannot_hold() {
        assert!(
            Code128Barcode::encode("Caf\u{e9}").is_none(),
            "a character was dropped"
        );
        let mut app = QrApp::new();
        app.handle_key(&ctrl_key(Key::B));
        probe::type_str(&mut app, "Caf\u{e9}");
        assert!(app.current_barcode.is_none());
        let why = app.error_message.clone().expect("no reason given");
        assert!(why.contains('\u{e9}'), "{why}");
        assert!(texts(&app).contains(&why), "the reason is not on screen");
    }

    #[test]
    fn too_long_says_how_much_fits() {
        let mut app = QrApp::new();
        let long = "x".repeat(400);
        app.set_input(&long);
        app.generate();
        let why = app.error_message.clone().expect("no reason given");
        let most = get_version_info(10, app.ec_level)
            .unwrap()
            .byte_mode_capacity();
        assert!(
            why.contains("400") && why.contains(&most.to_string()),
            "{why}"
        );
        assert!(app.current_qr.is_none());
    }

    #[test]
    fn the_caret_moves_and_typing_goes_where_it_is() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "held");
        app.handle_key(&press_key(Key::Left));
        app.handle_key(&press_key(Key::Left));
        probe::type_str(&mut app, "x");
        assert_eq!(app.input_text, "hexld");
        app.handle_key(&press_key(Key::Home));
        app.handle_key(&press_key(Key::Delete));
        assert_eq!(app.input_text, "exld");
        app.handle_key(&ctrl_key(Key::A));
        probe::type_str(&mut app, "new");
        assert_eq!(
            app.input_text, "new",
            "typing over a selection did not replace it"
        );
    }

    #[test]
    fn copy_and_paste_between_boxes() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "Ada");
        app.handle_key(&ctrl_key(Key::A));
        assert_eq!(app.handle_key(&ctrl_key(Key::C)), EventResult::Consumed);
        probe::click(&mut app, Target::Mode(InputMode::VCard));
        assert_eq!(app.handle_key(&ctrl_key(Key::V)), EventResult::Consumed);
        assert_eq!(app.vcard_info.first_name, "Ada");
        app.handle_key(&ctrl_key(Key::A));
        app.handle_key(&ctrl_key(Key::X));
        assert!(app.vcard_info.first_name.is_empty(), "cut left the text");
    }

    #[test]
    fn a_press_in_a_box_puts_the_keys_there() {
        let mut app = QrApp::new();
        probe::click(&mut app, Target::Mode(InputMode::VCard));
        probe::click(&mut app, Target::Field(Field::Email));
        assert_eq!(app.focused_field(), Field::Email);
        probe::type_str(&mut app, "ada@example.com");
        assert_eq!(app.vcard_info.email, "ada@example.com");
        assert!(
            app.current_qr.is_some(),
            "a contact with an email made no code"
        );
        probe::click(&mut app, Target::Field(Field::FirstName));
        probe::type_str(&mut app, "Ada");
        assert_eq!(app.vcard_info.first_name, "Ada");
        assert_eq!(
            app.vcard_info.email, "ada@example.com",
            "the other box changed"
        );
    }

    #[test]
    fn every_setting_answers_the_pointer() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "ABC");
        probe::click(&mut app, Target::CodeType(CodeType::Barcode128));
        assert!(app.current_barcode.is_some());
        probe::click(&mut app, Target::CodeType(CodeType::QrCode));
        assert!(app.current_qr.is_some());
        probe::click(&mut app, Target::Ec(EcLevel::H));
        assert_eq!(app.current_qr.as_ref().unwrap().ec_level, EcLevel::H);
        probe::click(&mut app, Target::Size(ModuleSize::Large));
        assert_eq!(app.module_size, ModuleSize::Large);
        probe::click(&mut app, Target::Mode(InputMode::Wifi));
        probe::type_str(&mut app, "net");
        probe::click(&mut app, Target::Encryption);
        assert_eq!(app.wifi_config.encryption, WifiEncryption::None);
        probe::click(&mut app, Target::Hidden);
        assert!(app.wifi_config.hidden);
        assert!(!app.history.is_empty());
        probe::click(&mut app, Target::ClearHistory);
        assert!(app.history.is_empty());
        assert!(
            probe::rect_of(&app, Target::ClearHistory).is_none(),
            "Forget is offered with nothing to forget"
        );
        probe::click(&mut app, Target::Help);
        assert!(app.show_help);
        probe::click(&mut app, Target::HelpCard);
        assert!(!app.show_help);
    }

    #[test]
    fn the_list_of_keys_is_modal() {
        let mut app = QrApp::new();
        app.handle_key(&press_key(Key::F1));
        assert_eq!(app.handle_key(&typed_key('x')), EventResult::Ignored);
        assert!(
            app.input_text.is_empty(),
            "a key reached the box under the list"
        );
        let mode = probe::rect_of(&app, Target::HelpCard).unwrap();
        let url = {
            let mut closed = QrApp::new();
            closed.show_help = false;
            probe::rect_of(&closed, Target::Mode(InputMode::Url)).unwrap()
        };
        assert!(mode.contains(url.x + 2.0, url.y + 2.0));
        press_at(&mut app, url.x + 2.0, url.y + 2.0);
        assert!(!app.show_help, "a press left the list up");
        assert_eq!(
            app.input_mode,
            InputMode::Text,
            "the press went through the list"
        );
    }

    #[test]
    fn the_history_scrolls() {
        let mut app = QrApp::new();
        for i in 0..40 {
            app.set_input(&format!("code {i}"));
            app.generate();
        }
        let rows = app.history_rows();
        assert!(rows < 40, "the list is not long enough to scroll");
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::HistoryList, -3.0),
            EventResult::Consumed
        );
        assert!(app.history_scroll > 0);
        for _ in 0..50 {
            probe::scroll_at_point(&mut app, Target::HistoryList, -3.0);
        }
        assert_eq!(app.history_scroll, 40 - rows);
        // The oldest is on screen now, and a press brings it back.
        probe::click(&mut app, Target::HistoryRow(0));
        assert_eq!(app.input_text, "code 0");
    }

    #[test]
    fn saving_is_offered_only_with_a_code() {
        let mut app = QrApp::new();
        assert!(
            probe::rect_of(&app, Target::Save).is_none(),
            "Save with nothing to save"
        );
        app.handle_key(&ctrl_key(Key::S));
        assert!(!app.picker.is_open());
        probe::type_str(&mut app, "x");
        probe::click(&mut app, Target::Save);
        assert!(app.picker.is_open(), "Save asked nothing");
    }

    /// The picture saved is the code on screen, square for square.
    #[test]
    fn a_saved_picture_is_the_code() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "https://example.com/");
        let qr = app.current_qr.clone().unwrap();
        let svg = app.svg().expect("no picture");
        let side = qr.size() + 8;
        assert!(
            svg.contains(&format!("viewBox=\"0 0 {side} {side}\"")),
            "{svg}"
        );
        assert!(
            svg.contains(&format!("width=\"{}\"", side * 5)),
            "not at the module size"
        );
        assert!(svg.contains("fill=\"#000000\"") && svg.contains("fill=\"#ffffff\""));
        let mut dark = 0;
        for row in 0..qr.size() {
            for col in 0..qr.size() {
                if qr.is_dark(row, col) {
                    dark += 1;
                }
            }
        }
        // Each run is `M{x} {y}h{run}v1h-{run}z`.
        let path = svg
            .split(" d=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let drawn: usize = path
            .split('M')
            .skip(1)
            .map(|run| {
                let across = run.split('h').nth(1).unwrap();
                across.split('v').next().unwrap().parse::<usize>().unwrap()
            })
            .sum();
        assert_eq!(drawn, dark, "the picture has a different number of squares");

        let file = std::env::temp_dir().join(format!("qrcode-test-{}.svg", std::process::id()));
        let said = app.save_svg(&file);
        assert!(said.starts_with("Saved "), "{said}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), svg);
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn a_saved_barcode_keeps_its_label_as_text() {
        let mut app = QrApp::new();
        app.handle_key(&ctrl_key(Key::B));
        probe::type_str(&mut app, "A&B<1>");
        let svg = app.svg().expect("no picture");
        assert!(svg.contains(">A&amp;B&lt;1&gt;</text>"), "{svg}");
    }

    #[test]
    fn the_colours_are_chosen_in_the_colour_dialog() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "x");
        probe::click(&mut app, Target::Foreground);
        assert!(app.color_dialog.is_some(), "no colour dialog");
        app.handle_event(&press(Key::Escape));
        assert!(app.color_dialog.is_none());
        assert_eq!(app.fg_color, Color::BLACK, "a cancelled colour was used");
        probe::click(&mut app, Target::Foreground);
        if let Some((_, dialog)) = app.color_dialog.as_mut() {
            dialog.picker_mut().set_rgb(0x20, 0x40, 0x80);
        }
        app.handle_event(&press(Key::Enter));
        assert!(app.color_dialog.is_none());
        assert_eq!(
            (app.fg_color.r, app.fg_color.g, app.fg_color.b),
            (0x20, 0x40, 0x80)
        );
        assert!(
            app.svg().unwrap().contains("fill=\"#204080\""),
            "the picture is not in the colour"
        );
        probe::click(&mut app, Target::ResetColors);
        assert_eq!((app.fg_color, app.bg_color), (Color::BLACK, Color::WHITE));
        assert!(probe::rect_of(&app, Target::ResetColors).is_none());
    }

    #[test]
    fn pale_squares_are_warned_about() {
        let mut app = QrApp::new();
        probe::type_str(&mut app, "x");
        let warning =
            "Scanners may not read this: the squares need to be much darker than the ground.";
        assert!(app.colors_scannable());
        assert!(!texts(&app).iter().any(|t| t == warning));
        app.fg_color = Color::WHITE;
        app.bg_color = Color::BLACK;
        assert!(!app.colors_scannable(), "an inverted code is not flagged");
        assert!(texts(&app).iter().any(|t| t == warning));
        app.fg_color = Color::rgba(0xcc, 0xcc, 0xcc, 255);
        app.bg_color = Color::WHITE;
        assert!(!app.colors_scannable(), "pale grey on white is not flagged");
    }

    #[test]
    fn a_big_code_is_shown_smaller_to_fit() {
        let mut app = QrApp::new();
        app.window_width = 900.0;
        app.window_height = 600.0;
        app.module_size = ModuleSize::Large;
        app.set_input(&"y".repeat(200));
        app.generate();
        assert!(app.current_qr.is_some(), "{:?}", app.error_message);
        assert!(
            texts(&app)
                .iter()
                .any(|t| t.ends_with("| shown smaller to fit"))
        );
        let panel_right = app.window_width - RIGHT_PANEL_WIDTH;
        // The code's squares: in its ink, and five pixels across -- the
        // "Squares" swatch is in the same ink, and twenty.
        for c in app.frame(app.window_width, app.window_height).commands() {
            if let RenderCommand::FillRect {
                x, width, color, ..
            } = c
                && *color == app.fg_color
                && (*width - 5.0).abs() < 0.01
            {
                assert!(
                    x + width <= panel_right + 0.5,
                    "a square past the preview at {x}"
                );
            }
        }
    }

    #[test]
    fn a_window_starts_empty_and_says_why() {
        let app = QrApp::new();
        assert!(app.history.is_empty(), "history the user did not make");
        assert!(app.current_qr.is_none());
        assert!(texts(&app).iter().any(|t| t == NOTHING_TYPED));
    }

    // ------------------------------------------------------------------
    // Reading a symbol back, from the standard's layout
    //
    // Nothing checked the symbol a scanner reads: the tests checked the
    // codewords and the tables, and version 7-10 symbols had their data in
    // the version information's place, version 10's alignment patterns two
    // modules off, and one format bit never written. These read a symbol the
    // way a scanner does, with the layout worked out again from the
    // standard rather than taken from the encoder -- so the two have to
    // agree, and a mistake in one cannot agree with itself.
    // ------------------------------------------------------------------

    /// The alignment-pattern centres of versions 1 to 10, from the standard.
    fn standard_alignment(version: u8) -> &'static [usize] {
        match version {
            1 => &[],
            2 => &[6, 18],
            3 => &[6, 22],
            4 => &[6, 26],
            5 => &[6, 30],
            6 => &[6, 34],
            7 => &[6, 22, 38],
            8 => &[6, 24, 42],
            9 => &[6, 26, 46],
            10 => &[6, 28, 50],
            _ => panic!("version {version} is past this program"),
        }
    }

    /// Which modules a symbol of `version` gives to function patterns.
    fn reserved_modules(version: u8) -> Vec<Vec<bool>> {
        let size = 17 + 4 * usize::from(version);
        let mut reserved = vec![vec![false; size]; size];
        // The three finders with their separators: the 8x8 corners.
        for i in 0..8 {
            for j in 0..8 {
                reserved[i][j] = true;
                reserved[i][size - 1 - j] = true;
                reserved[size - 1 - i][j] = true;
            }
        }
        // The timing patterns.
        for i in 0..size {
            reserved[6][i] = true;
            reserved[i][6] = true;
        }
        // The alignment patterns, but for the three a finder covers.
        let centres = standard_alignment(version);
        if let (Some(&first), Some(&last)) = (centres.first(), centres.last()) {
            for &r in centres {
                for &c in centres {
                    if [(first, first), (first, last), (last, first)].contains(&(r, c)) {
                        continue;
                    }
                    for row in reserved.iter_mut().take(r + 3).skip(r - 2) {
                        for cell in row.iter_mut().take(c + 3).skip(c - 2) {
                            *cell = true;
                        }
                    }
                }
            }
        }
        // Both copies of the format information, and the dark module.
        for i in 0..9 {
            reserved[8][i] = true;
            reserved[i][8] = true;
        }
        for i in 0..8 {
            reserved[8][size - 1 - i] = true;
            reserved[size - 1 - i][8] = true;
        }
        // Both copies of the version information, from version 7.
        if version >= 7 {
            for i in 0..6 {
                for j in 0..3 {
                    reserved[i][size - 11 + j] = true;
                    reserved[size - 11 + j][i] = true;
                }
            }
        }
        reserved
    }

    /// The modules left over past the last codeword: seven for versions 2
    /// to 6, none for the others up to 13.
    fn remainder_bits(version: u8) -> usize {
        if (2..=6).contains(&version) { 7 } else { 0 }
    }

    /// Whether mask `mask` inverts the module at `(row, col)`.
    fn mask_inverts(mask: u32, row: usize, col: usize) -> bool {
        match mask {
            0 => (row + col).is_multiple_of(2),
            1 => row.is_multiple_of(2),
            2 => col.is_multiple_of(3),
            3 => (row + col).is_multiple_of(3),
            4 => (row / 2 + col / 3).is_multiple_of(2),
            5 => (row * col) % 2 + (row * col) % 3 == 0,
            6 => ((row * col) % 2 + (row * col) % 3).is_multiple_of(2),
            7 => ((row + col) % 2 + (row * col) % 3).is_multiple_of(2),
            _ => panic!("mask {mask}"),
        }
    }

    /// What a scanner reads from a symbol.
    struct ReadBack {
        /// The two copies of the format information.
        format: (u32, u32),
        /// The two copies of the version information (0 below version 7).
        version: (u32, u32),
        /// The bytes the data codewords carry.
        data: Vec<u8>,
    }

    fn read_back(qr: &QrCode) -> ReadBack {
        let size = qr.size();
        let version = qr.version;
        assert_eq!(size, 17 + 4 * usize::from(version));
        let dark = |r: usize, c: usize| u32::from(qr.is_dark(r, c));

        let copy1: Vec<(usize, usize)> = (0..=5)
            .map(|i| (i, 8))
            .chain([(7, 8), (8, 8), (8, 7)])
            .chain((9..15).map(|i| (8, 14 - i)))
            .collect();
        let copy2: Vec<(usize, usize)> = (0..8)
            .map(|i| (8, size - 1 - i))
            .chain((8..15).map(|i| (size - 15 + i, 8)))
            .collect();
        let read = |places: &[(usize, usize)]| {
            places
                .iter()
                .enumerate()
                .fold(0, |acc, (i, &(r, c))| acc | (dark(r, c) << i))
        };
        let format = (read(&copy1), read(&copy2));
        assert_eq!(dark(size - 8, 8), 1, "the dark module is not dark");

        let version_info = if version >= 7 {
            let mut top_right = 0;
            let mut bottom_left = 0;
            for i in 0..18 {
                top_right |= dark(i / 3, size - 11 + i % 3) << i;
                bottom_left |= dark(size - 11 + i % 3, i / 3) << i;
            }
            (top_right, bottom_left)
        } else {
            (0, 0)
        };

        // The data: two columns at a time from the right, skipping the
        // timing column, up and down in turn.
        let reserved = reserved_modules(version);
        let mask = ((format.0 ^ 0x5412) >> 10) & 0b111;
        let mut bits = Vec::new();
        let mut right = size - 1;
        let mut upward = true;
        loop {
            if right == 6 {
                right = 5;
            }
            for step in 0..size {
                let row = if upward { size - 1 - step } else { step };
                for col in [right, right - 1] {
                    if !reserved[row][col] {
                        bits.push(qr.is_dark(row, col) ^ mask_inverts(mask, row, col));
                    }
                }
            }
            upward = !upward;
            if right < 2 {
                break;
            }
            right -= 2;
        }
        let info = get_version_info(version, qr.ec_level).unwrap();
        assert_eq!(
            bits.len(),
            info.total_codewords * 8 + remainder_bits(version),
            "v{version}: the symbol has room for other than exactly its codewords"
        );
        let codewords: Vec<u8> = bits
            .chunks(8)
            .take(info.total_codewords)
            .map(|b| b.iter().fold(0u8, |acc, &x| (acc << 1) | u8::from(x)))
            .collect();

        // The data codewords, un-interleaved: the short blocks first.
        let data_total = info.data_codewords();
        let blocks = info.num_blocks;
        let short = data_total / blocks;
        let long_from = blocks - data_total % blocks;
        let lengths: Vec<usize> = (0..blocks)
            .map(|b| short + usize::from(b >= long_from))
            .collect();
        let mut per_block = vec![Vec::new(); blocks];
        let mut next = codewords.iter();
        for i in 0..=short {
            for (b, block) in per_block.iter_mut().enumerate() {
                if i < lengths[b] {
                    block.push(*next.next().unwrap());
                }
            }
        }
        let stream: Vec<bool> = per_block
            .concat()
            .iter()
            .flat_map(|byte| (0..8).rev().map(move |i| (byte >> i) & 1 == 1))
            .collect();
        let take = |at: &mut usize, n: usize| {
            let value = stream[*at..*at + n]
                .iter()
                .fold(0usize, |acc, &b| (acc << 1) | usize::from(b));
            *at += n;
            value
        };
        let mut at = 0;
        assert_eq!(take(&mut at, 4), 0b0100, "not byte mode");
        let count = take(&mut at, if version <= 9 { 8 } else { 16 });
        let data = (0..count).map(|_| take(&mut at, 8) as u8).collect();
        ReadBack {
            format,
            version: version_info,
            data,
        }
    }

    /// Every version this program makes, at every level, reads back as what
    /// was encoded -- with both copies of the format and version
    /// information whole.
    #[test]
    fn every_symbol_reads_back_as_what_was_encoded() {
        let mut seen = std::collections::BTreeSet::new();
        for ec in EcLevel::all() {
            for len in [1_usize, 10, 20, 40, 60, 90, 120, 150, 180, 210, 240, 270] {
                let text: Vec<u8> = (0..len).map(|i| b"QR:qr/09+x"[i % 10]).collect();
                let Some(qr) = QrCode::encode(&text, *ec) else {
                    continue;
                };
                seen.insert(qr.version);
                let back = read_back(&qr);
                assert_eq!(
                    back.data, text,
                    "v{} {ec:?} read back as something else",
                    qr.version
                );
                let expected = format_bits(*ec, qr.mask_pattern);
                assert_eq!(
                    back.format.0, expected,
                    "v{} {ec:?}: the first format copy",
                    qr.version
                );
                assert_eq!(
                    back.format.1, expected,
                    "v{} {ec:?}: the second format copy",
                    qr.version
                );
                if qr.version >= 7 {
                    let v = version_info_bits(qr.version);
                    assert_eq!(
                        back.version,
                        (v, v),
                        "v{}: the version information",
                        qr.version
                    );
                }
            }
        }
        assert_eq!(seen.len(), 10, "not every version was made: {seen:?}");
    }

    /// The version information is the standard's, whose table gives these.
    #[test]
    fn version_information_is_the_standards() {
        assert_eq!(version_info_bits(7), 0x07C94);
        assert_eq!(version_info_bits(8), 0x085BC);
        assert_eq!(version_info_bits(9), 0x09A99);
        assert_eq!(version_info_bits(10), 0x0A4D3);
    }

    /// The format information is the standard's for mask 0 at each level.
    #[test]
    fn format_information_is_the_standards() {
        assert_eq!(format_bits(EcLevel::L, 0), 0b111_0111_1100_0100);
        assert_eq!(format_bits(EcLevel::M, 0), 0b101_0100_0001_0010);
        assert_eq!(format_bits(EcLevel::Q, 0), 0b011_0101_0101_1111);
        assert_eq!(format_bits(EcLevel::H, 0), 0b001_0110_1000_1001);
    }

    /// The rules every Code128 symbol keeps: three bars and three spaces,
    /// each one to four modules, eleven in all, the bars an even number of
    /// modules; and no two alike, or two values would scan as one.
    #[test]
    fn every_code128_pattern_is_well_formed() {
        assert_eq!(CODE128_PATTERNS.len(), 106);
        for (value, pattern) in CODE128_PATTERNS.iter().enumerate() {
            assert!(
                pattern.iter().all(|w| (1..=4).contains(w)),
                "{value}: {pattern:?}"
            );
            assert_eq!(
                pattern.iter().map(|w| u32::from(*w)).sum::<u32>(),
                11,
                "{value}: {pattern:?}"
            );
            let bars: u32 = pattern.iter().step_by(2).map(|w| u32::from(*w)).sum();
            assert_eq!(
                bars % 2,
                0,
                "{value}: the bars are an odd number of modules"
            );
            for (other, earlier) in CODE128_PATTERNS.iter().enumerate().take(value) {
                assert_ne!(pattern, earlier, "{value} and {other} are the same pattern");
            }
        }
        assert_eq!(CODE128_STOP.iter().map(|w| u32::from(*w)).sum::<u32>(), 13);
    }

    /// A sample of the published table, from both ends of it.
    #[test]
    fn the_code128_table_is_the_standards() {
        let sample: [(usize, [u8; 6]); 8] = [
            (0, [2, 1, 2, 2, 2, 2]),
            (33, [1, 1, 1, 3, 2, 3]),
            (36, [1, 1, 2, 3, 1, 3]),
            (60, [3, 1, 4, 1, 1, 1]),
            (65, [1, 2, 1, 1, 2, 4]),
            (90, [2, 1, 4, 1, 2, 1]),
            (95, [1, 1, 4, 1, 1, 3]),
            (104, [2, 1, 1, 2, 1, 4]),
        ];
        for (value, widths) in sample {
            assert_eq!(CODE128_PATTERNS[value], widths, "value {value}");
        }
    }

    /// A barcode reads back as its text, through the table it was made
    /// from, with the check value the standard's sum gives.
    #[test]
    fn a_barcode_reads_back_as_its_text() {
        let text = "Wikipedia, 2026!";
        let barcode = Code128Barcode::encode(text).unwrap();
        // Strip the quiet zones, then read eleven modules at a time.
        let bars: Vec<bool> = barcode.bars[10..barcode.bars.len() - 10].to_vec();
        let mut widths = Vec::new();
        let mut run = 1_u8;
        for pair in bars.windows(2) {
            if pair[0] == pair[1] {
                run += 1;
            } else {
                widths.push(run);
                run = 1;
            }
        }
        widths.push(run);
        let stop = widths.split_off(widths.len() - 7);
        assert_eq!(stop, CODE128_STOP);
        let values: Vec<usize> = widths
            .chunks(6)
            .map(|w| {
                CODE128_PATTERNS
                    .iter()
                    .position(|p| p[..] == *w)
                    .unwrap_or_else(|| panic!("{w:?} is no symbol"))
            })
            .collect();
        assert_eq!(values[0], 104, "not Start B");
        let (body, check) = values[1..].split_at(values.len() - 2);
        let decoded: String = body.iter().map(|v| char::from(*v as u8 + 32)).collect();
        assert_eq!(decoded, text);
        let sum = body
            .iter()
            .enumerate()
            .fold(104, |acc, (i, v)| acc + (i + 1) * v);
        assert_eq!(check[0], sum % 103, "the check value");
    }
}

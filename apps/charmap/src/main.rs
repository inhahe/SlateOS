//! Character Map — Unicode character browser and picker for SlateOS.
//!
//! Features:
//! - Browse Unicode blocks (Basic Latin, Latin Extended, Greek, Cyrillic, CJK, Arrows,
//!   Mathematical, Box Drawing, Braille, Emoji, etc.)
//! - Grid display with character cells, hover/select detail
//! - Search by character name, codepoint (U+XXXX), or literal character
//! - Recently used characters list
//! - Favorites with persist
//! - Copy to clipboard
//! - Character detail: codepoint, name, block, category, UTF-8 bytes, HTML entity
//! - Font size preview (small/medium/large/jumbo)
//! - Filter by Unicode general category (Letter, Number, Symbol, Punctuation, etc.)
//!
//! The window is drawn as a [`Frame`] and every clickable thing records its box
//! as it is painted, so there is one expression for where a cell is rather than
//! one in the renderer and a second in the click handler. That mattered here
//! more than usual: the old renderer worked out the grid's scroll position and
//! its column count from the window width *while drawing*, threw both away, and
//! left the navigation code using a `grid_columns: 16` that no window of any
//! size actually had.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::{scroll_window, wheel};
use oswindow::app::{self, App, Response};

use std::num::NonZeroUsize;
use std::process::ExitCode;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
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
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const TEAL: Color = Color::from_hex(0x94E2D5);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Unicode General Categories ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GeneralCategory {
    UppercaseLetter,
    LowercaseLetter,
    TitlecaseLetter,
    ModifierLetter,
    OtherLetter,
    NonspacingMark,
    SpacingMark,
    EnclosingMark,
    DecimalNumber,
    LetterNumber,
    OtherNumber,
    ConnectorPunctuation,
    DashPunctuation,
    OpenPunctuation,
    ClosePunctuation,
    InitialPunctuation,
    FinalPunctuation,
    OtherPunctuation,
    MathSymbol,
    CurrencySymbol,
    ModifierSymbol,
    OtherSymbol,
    SpaceSeparator,
    LineSeparator,
    ParagraphSeparator,
    Control,
    Format,
    Surrogate,
    PrivateUse,
    Unassigned,
}

impl GeneralCategory {
    fn label(self) -> &'static str {
        match self {
            Self::UppercaseLetter => "Uppercase Letter (Lu)",
            Self::LowercaseLetter => "Lowercase Letter (Ll)",
            Self::TitlecaseLetter => "Titlecase Letter (Lt)",
            Self::ModifierLetter => "Modifier Letter (Lm)",
            Self::OtherLetter => "Other Letter (Lo)",
            Self::NonspacingMark => "Nonspacing Mark (Mn)",
            Self::SpacingMark => "Spacing Mark (Mc)",
            Self::EnclosingMark => "Enclosing Mark (Me)",
            Self::DecimalNumber => "Decimal Number (Nd)",
            Self::LetterNumber => "Letter Number (Nl)",
            Self::OtherNumber => "Other Number (No)",
            Self::ConnectorPunctuation => "Connector Punct (Pc)",
            Self::DashPunctuation => "Dash Punct (Pd)",
            Self::OpenPunctuation => "Open Punct (Ps)",
            Self::ClosePunctuation => "Close Punct (Pe)",
            Self::InitialPunctuation => "Initial Punct (Pi)",
            Self::FinalPunctuation => "Final Punct (Pf)",
            Self::OtherPunctuation => "Other Punct (Po)",
            Self::MathSymbol => "Math Symbol (Sm)",
            Self::CurrencySymbol => "Currency Symbol (Sc)",
            Self::ModifierSymbol => "Modifier Symbol (Sk)",
            Self::OtherSymbol => "Other Symbol (So)",
            Self::SpaceSeparator => "Space Separator (Zs)",
            Self::LineSeparator => "Line Separator (Zl)",
            Self::ParagraphSeparator => "Paragraph Sep (Zp)",
            Self::Control => "Control (Cc)",
            Self::Format => "Format (Cf)",
            Self::Surrogate => "Surrogate (Cs)",
            Self::PrivateUse => "Private Use (Co)",
            Self::Unassigned => "Unassigned (Cn)",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Self::UppercaseLetter => "Lu",
            Self::LowercaseLetter => "Ll",
            Self::TitlecaseLetter => "Lt",
            Self::ModifierLetter => "Lm",
            Self::OtherLetter => "Lo",
            Self::NonspacingMark => "Mn",
            Self::SpacingMark => "Mc",
            Self::EnclosingMark => "Me",
            Self::DecimalNumber => "Nd",
            Self::LetterNumber => "Nl",
            Self::OtherNumber => "No",
            Self::ConnectorPunctuation => "Pc",
            Self::DashPunctuation => "Pd",
            Self::OpenPunctuation => "Ps",
            Self::ClosePunctuation => "Pe",
            Self::InitialPunctuation => "Pi",
            Self::FinalPunctuation => "Pf",
            Self::OtherPunctuation => "Po",
            Self::MathSymbol => "Sm",
            Self::CurrencySymbol => "Sc",
            Self::ModifierSymbol => "Sk",
            Self::OtherSymbol => "So",
            Self::SpaceSeparator => "Zs",
            Self::LineSeparator => "Zl",
            Self::ParagraphSeparator => "Zp",
            Self::Control => "Cc",
            Self::Format => "Cf",
            Self::Surrogate => "Cs",
            Self::PrivateUse => "Co",
            Self::Unassigned => "Cn",
        }
    }

    fn is_letter(self) -> bool {
        matches!(
            self,
            Self::UppercaseLetter
                | Self::LowercaseLetter
                | Self::TitlecaseLetter
                | Self::ModifierLetter
                | Self::OtherLetter
        )
    }

    fn is_number(self) -> bool {
        matches!(
            self,
            Self::DecimalNumber | Self::LetterNumber | Self::OtherNumber
        )
    }

    fn is_symbol(self) -> bool {
        matches!(
            self,
            Self::MathSymbol | Self::CurrencySymbol | Self::ModifierSymbol | Self::OtherSymbol
        )
    }

    fn is_punctuation(self) -> bool {
        matches!(
            self,
            Self::ConnectorPunctuation
                | Self::DashPunctuation
                | Self::OpenPunctuation
                | Self::ClosePunctuation
                | Self::InitialPunctuation
                | Self::FinalPunctuation
                | Self::OtherPunctuation
        )
    }
}

// ── Unicode Block Definitions ──────────────────────────────────────────────

#[derive(Debug, Clone)]
struct UnicodeBlock {
    name: &'static str,
    start: u32,
    end: u32, // inclusive
}

impl UnicodeBlock {
    const fn new(name: &'static str, start: u32, end: u32) -> Self {
        Self { name, start, end }
    }

    fn len(&self) -> u32 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }

    fn contains(&self, cp: u32) -> bool {
        cp >= self.start && cp <= self.end
    }
}

/// All supported Unicode blocks (a representative subset).
fn unicode_blocks() -> Vec<UnicodeBlock> {
    vec![
        UnicodeBlock::new("Basic Latin", 0x0000, 0x007F),
        UnicodeBlock::new("Latin-1 Supplement", 0x0080, 0x00FF),
        UnicodeBlock::new("Latin Extended-A", 0x0100, 0x017F),
        UnicodeBlock::new("Latin Extended-B", 0x0180, 0x024F),
        UnicodeBlock::new("IPA Extensions", 0x0250, 0x02AF),
        UnicodeBlock::new("Spacing Modifier Letters", 0x02B0, 0x02FF),
        UnicodeBlock::new("Combining Diacritical Marks", 0x0300, 0x036F),
        UnicodeBlock::new("Greek and Coptic", 0x0370, 0x03FF),
        UnicodeBlock::new("Cyrillic", 0x0400, 0x04FF),
        UnicodeBlock::new("Armenian", 0x0530, 0x058F),
        UnicodeBlock::new("Hebrew", 0x0590, 0x05FF),
        UnicodeBlock::new("Arabic", 0x0600, 0x06FF),
        UnicodeBlock::new("Devanagari", 0x0900, 0x097F),
        UnicodeBlock::new("Thai", 0x0E00, 0x0E7F),
        UnicodeBlock::new("Georgian", 0x10A0, 0x10FF),
        UnicodeBlock::new("Hangul Jamo", 0x1100, 0x11FF),
        UnicodeBlock::new("General Punctuation", 0x2000, 0x206F),
        UnicodeBlock::new("Superscripts and Subscripts", 0x2070, 0x209F),
        UnicodeBlock::new("Currency Symbols", 0x20A0, 0x20CF),
        UnicodeBlock::new("Letterlike Symbols", 0x2100, 0x214F),
        UnicodeBlock::new("Number Forms", 0x2150, 0x218F),
        UnicodeBlock::new("Arrows", 0x2190, 0x21FF),
        UnicodeBlock::new("Mathematical Operators", 0x2200, 0x22FF),
        UnicodeBlock::new("Miscellaneous Technical", 0x2300, 0x23FF),
        UnicodeBlock::new("Control Pictures", 0x2400, 0x243F),
        UnicodeBlock::new("Enclosed Alphanumerics", 0x2460, 0x24FF),
        UnicodeBlock::new("Box Drawing", 0x2500, 0x257F),
        UnicodeBlock::new("Block Elements", 0x2580, 0x259F),
        UnicodeBlock::new("Geometric Shapes", 0x25A0, 0x25FF),
        UnicodeBlock::new("Miscellaneous Symbols", 0x2600, 0x26FF),
        UnicodeBlock::new("Dingbats", 0x2700, 0x27BF),
        UnicodeBlock::new("Braille Patterns", 0x2800, 0x28FF),
        UnicodeBlock::new("CJK Radicals Supplement", 0x2E80, 0x2EFF),
        UnicodeBlock::new("CJK Symbols and Punctuation", 0x3000, 0x303F),
        UnicodeBlock::new("Hiragana", 0x3040, 0x309F),
        UnicodeBlock::new("Katakana", 0x30A0, 0x30FF),
        UnicodeBlock::new("CJK Unified Ideographs (sample)", 0x4E00, 0x4E7F),
        UnicodeBlock::new("Hangul Syllables (sample)", 0xAC00, 0xAC7F),
        UnicodeBlock::new("Private Use Area (sample)", 0xE000, 0xE07F),
        UnicodeBlock::new("Alphabetic Presentation Forms", 0xFB00, 0xFB4F),
        UnicodeBlock::new("Halfwidth and Fullwidth Forms", 0xFF00, 0xFFEF),
        UnicodeBlock::new("Specials", 0xFFF0, 0xFFFD),
        UnicodeBlock::new("Musical Symbols (sample)", 0x1D100, 0x1D17F),
        UnicodeBlock::new("Mathematical Alphanumeric", 0x1D400, 0x1D4FF),
        UnicodeBlock::new("Emoticons", 0x1F600, 0x1F64F),
        UnicodeBlock::new("Transport and Map Symbols", 0x1F680, 0x1F6FF),
        UnicodeBlock::new("Miscellaneous Symbols & Pictographs", 0x1F300, 0x1F3FF),
    ]
}

// ── Character Info ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct CharInfo {
    codepoint: u32,
    name: String,
    category: GeneralCategory,
    block_name: String,
}

impl CharInfo {
    fn display_char(&self) -> String {
        if let Some(ch) = char::from_u32(self.codepoint) {
            if ch.is_control() || self.codepoint == 0xFFFE || self.codepoint == 0xFFFF {
                format!("U+{:04X}", self.codepoint)
            } else {
                ch.to_string()
            }
        } else {
            format!("U+{:04X}", self.codepoint)
        }
    }

    fn codepoint_str(&self) -> String {
        if self.codepoint <= 0xFFFF {
            format!("U+{:04X}", self.codepoint)
        } else {
            format!("U+{:05X}", self.codepoint)
        }
    }

    fn utf8_bytes(&self) -> Vec<u8> {
        let mut buf = [0u8; 4];
        if let Some(ch) = char::from_u32(self.codepoint) {
            let s = ch.encode_utf8(&mut buf);
            s.as_bytes().to_vec()
        } else {
            Vec::new()
        }
    }

    fn utf8_hex(&self) -> String {
        self.utf8_bytes()
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn html_entity(&self) -> String {
        format!("&#{};", self.codepoint)
    }

    fn html_hex_entity(&self) -> String {
        format!("&#x{:X};", self.codepoint)
    }

    fn css_escape(&self) -> String {
        format!("\\{:04X}", self.codepoint)
    }

    fn rust_escape(&self) -> String {
        format!("'\\u{{{:04X}}}'", self.codepoint)
    }
}

/// Classify a codepoint into a general category (simplified — covers basic ranges).
fn classify_codepoint(cp: u32) -> GeneralCategory {
    match cp {
        0x0000..=0x001F | 0x007F..=0x009F => GeneralCategory::Control,
        0x0020 | 0x00A0 | 0x1680 | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000 => {
            GeneralCategory::SpaceSeparator
        }
        0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0xFEFF | 0xFFF9..=0xFFFB => {
            GeneralCategory::Format
        }
        0x0030..=0x0039 | 0x0660..=0x0669 | 0x06F0..=0x06F9 | 0x0966..=0x096F | 0x0E50..=0x0E59 => {
            GeneralCategory::DecimalNumber
        }
        0x2160..=0x2182 | 0x3007 | 0x3021..=0x3029 => GeneralCategory::LetterNumber,
        0x00B2
        | 0x00B3
        | 0x00B9
        | 0x00BC..=0x00BE
        | 0x2070..=0x2079
        | 0x2080..=0x2089
        | 0x2150..=0x215F
        | 0x2460..=0x2473
        | 0x2474..=0x2487
        | 0x2488..=0x249B => GeneralCategory::OtherNumber,
        0x0041..=0x005A | 0x00C0..=0x00D6 | 0x00D8..=0x00DE | 0x0410..=0x042F => {
            GeneralCategory::UppercaseLetter
        }
        0x0061..=0x007A | 0x00DF..=0x00F6 | 0x00F8..=0x00FF | 0x0430..=0x044F => {
            GeneralCategory::LowercaseLetter
        }
        0x01C5 | 0x01C8 | 0x01CB | 0x01F2 => GeneralCategory::TitlecaseLetter,
        0x02B0..=0x02FF => GeneralCategory::ModifierLetter,
        0x0300..=0x036F | 0x0591..=0x05BD | 0x064B..=0x065F | 0x0E31 | 0x0E34..=0x0E3A => {
            GeneralCategory::NonspacingMark
        }
        0x0903 | 0x093E..=0x0940 | 0x0949..=0x094C | 0x0E33 => GeneralCategory::SpacingMark,
        // The enclosing marks (Me) — a combining ring, square or diamond drawn
        // *around* the previous character. Tiny category, but the detail panel
        // already had a label for it, and a label no codepoint can ever reach is
        // a claim the program does not keep.
        0x0488 | 0x0489 | 0x1ABE | 0x20DD..=0x20E0 | 0x20E2..=0x20E4 | 0xA670..=0xA672 => {
            GeneralCategory::EnclosingMark
        }
        0x0021 | 0x0022 | 0x0023 | 0x0025 | 0x0026 | 0x0027 | 0x002A | 0x002C | 0x002E | 0x002F
        | 0x003A | 0x003B | 0x003F | 0x0040 | 0x005C | 0x00A1 | 0x00A7 | 0x00B6 | 0x00BF => {
            GeneralCategory::OtherPunctuation
        }
        0x005F => GeneralCategory::ConnectorPunctuation,
        0x002D | 0x2010..=0x2015 | 0x2E17 | 0x2E1A | 0xFE58 | 0xFE63 | 0xFF0D => {
            GeneralCategory::DashPunctuation
        }
        0x0028
        | 0x005B
        | 0x007B
        | 0x2045
        | 0x207D
        | 0x208D
        | 0x2308
        | 0x230A
        | 0x2329
        | 0x27E6..=0x27EF
        | 0x2983..=0x2998
        | 0xFF08
        | 0xFF3B
        | 0xFF5B => GeneralCategory::OpenPunctuation,
        0x0029 | 0x005D | 0x007D | 0x2046 | 0x207E | 0x208E | 0x2309 | 0x230B | 0x232A | 0xFF09
        | 0xFF3D | 0xFF5D => GeneralCategory::ClosePunctuation,
        0x00AB | 0x2018 | 0x201B | 0x201C | 0x201F | 0x2039 => GeneralCategory::InitialPunctuation,
        0x00BB | 0x2019 | 0x201D | 0x203A => GeneralCategory::FinalPunctuation,
        0x002B
        | 0x003C..=0x003E
        | 0x007C
        | 0x007E
        | 0x00AC
        | 0x00B1
        | 0x00D7
        | 0x00F7
        | 0x2200..=0x22FF
        | 0x27C0..=0x27EF
        | 0x2980..=0x29FF
        | 0x2A00..=0x2AFF => GeneralCategory::MathSymbol,
        0x0024
        | 0x00A2..=0x00A5
        | 0x058F
        | 0x060B
        | 0x09F2..=0x09F3
        | 0x0AF1
        | 0x0BF9
        | 0x20A0..=0x20CF
        | 0xFE69
        | 0xFF04
        | 0xFFE0..=0xFFE1
        | 0xFFE5..=0xFFE6 => GeneralCategory::CurrencySymbol,
        0x005E | 0x0060 | 0x00A8 | 0x00AF | 0x00B4 | 0x00B8 => GeneralCategory::ModifierSymbol,
        0x00A6
        | 0x00A9
        | 0x00AE
        | 0x00B0
        | 0x2100..=0x214F
        | 0x2190..=0x21FF
        | 0x2300..=0x23FF
        | 0x2400..=0x243F
        | 0x2440..=0x245F
        | 0x2500..=0x257F
        | 0x2580..=0x259F
        | 0x25A0..=0x25FF
        | 0x2600..=0x26FF
        | 0x2700..=0x27BF
        | 0x2800..=0x28FF
        | 0xFFFD => GeneralCategory::OtherSymbol,
        0x2028 => GeneralCategory::LineSeparator,
        0x2029 => GeneralCategory::ParagraphSeparator,
        0xD800..=0xDFFF => GeneralCategory::Surrogate,
        0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD => GeneralCategory::PrivateUse,
        // Emoticons and pictographs
        0x1F300..=0x1F3FF
        | 0x1F400..=0x1F4FF
        | 0x1F500..=0x1F5FF
        | 0x1F600..=0x1F64F
        | 0x1F680..=0x1F6FF
        | 0x1F900..=0x1F9FF => GeneralCategory::OtherSymbol,
        // Musical symbols
        0x1D100..=0x1D1FF => GeneralCategory::OtherSymbol,
        // Mathematical alphanumeric
        0x1D400..=0x1D7FF => GeneralCategory::UppercaseLetter,
        // CJK ideographs
        0x4E00..=0x9FFF | 0x3400..=0x4DBF => GeneralCategory::OtherLetter,
        // Hiragana, Katakana
        0x3040..=0x30FF => GeneralCategory::OtherLetter,
        // Hangul
        0xAC00..=0xD7AF => GeneralCategory::OtherLetter,
        // Greek letters
        0x0370..=0x03FF => GeneralCategory::LowercaseLetter,
        // Hebrew
        0x05D0..=0x05EA => GeneralCategory::OtherLetter,
        // Arabic letters
        0x0621..=0x064A => GeneralCategory::OtherLetter,
        // Devanagari
        0x0901..=0x0963 => GeneralCategory::OtherLetter,
        // Thai
        0x0E01..=0x0E30 | 0x0E32 | 0x0E40..=0x0E46 => GeneralCategory::OtherLetter,
        // Georgian
        0x10A0..=0x10C5 | 0x10D0..=0x10FA => GeneralCategory::OtherLetter,
        // Hangul Jamo
        0x1100..=0x11FF => GeneralCategory::OtherLetter,
        // Armenian
        0x0531..=0x0556 => GeneralCategory::UppercaseLetter,
        0x0561..=0x0587 => GeneralCategory::LowercaseLetter,
        // Latin Extended
        0x0100..=0x024F => GeneralCategory::LowercaseLetter,
        // IPA
        0x0250..=0x02AF => GeneralCategory::LowercaseLetter,
        // Alphabetic Presentation Forms
        0xFB00..=0xFB4F => GeneralCategory::LowercaseLetter,
        // Fullwidth Latin
        0xFF01..=0xFFEF => GeneralCategory::OtherSymbol,
        // Specials
        0xFFFE..=0xFFFF => GeneralCategory::Unassigned,
        // CJK symbols
        0x3000..=0x303F => GeneralCategory::OtherPunctuation,
        0x2E80..=0x2EFF => GeneralCategory::OtherSymbol,
        _ => GeneralCategory::Unassigned,
    }
}

/// Generate a name for a codepoint (simplified — covers well-known ranges).
fn codepoint_name(cp: u32) -> String {
    // Named characters for common ones
    match cp {
        0x0000 => return "NULL".into(),
        0x0001 => return "START OF HEADING".into(),
        0x0002 => return "START OF TEXT".into(),
        0x0003 => return "END OF TEXT".into(),
        0x0004 => return "END OF TRANSMISSION".into(),
        0x0007 => return "BELL".into(),
        0x0008 => return "BACKSPACE".into(),
        0x0009 => return "CHARACTER TABULATION".into(),
        0x000A => return "LINE FEED".into(),
        0x000B => return "LINE TABULATION".into(),
        0x000C => return "FORM FEED".into(),
        0x000D => return "CARRIAGE RETURN".into(),
        0x001B => return "ESCAPE".into(),
        0x0020 => return "SPACE".into(),
        0x0021 => return "EXCLAMATION MARK".into(),
        0x0022 => return "QUOTATION MARK".into(),
        0x0023 => return "NUMBER SIGN".into(),
        0x0024 => return "DOLLAR SIGN".into(),
        0x0025 => return "PERCENT SIGN".into(),
        0x0026 => return "AMPERSAND".into(),
        0x0027 => return "APOSTROPHE".into(),
        0x0028 => return "LEFT PARENTHESIS".into(),
        0x0029 => return "RIGHT PARENTHESIS".into(),
        0x002A => return "ASTERISK".into(),
        0x002B => return "PLUS SIGN".into(),
        0x002C => return "COMMA".into(),
        0x002D => return "HYPHEN-MINUS".into(),
        0x002E => return "FULL STOP".into(),
        0x002F => return "SOLIDUS".into(),
        0x003A => return "COLON".into(),
        0x003B => return "SEMICOLON".into(),
        0x003C => return "LESS-THAN SIGN".into(),
        0x003D => return "EQUALS SIGN".into(),
        0x003E => return "GREATER-THAN SIGN".into(),
        0x003F => return "QUESTION MARK".into(),
        0x0040 => return "COMMERCIAL AT".into(),
        0x005B => return "LEFT SQUARE BRACKET".into(),
        0x005C => return "REVERSE SOLIDUS".into(),
        0x005D => return "RIGHT SQUARE BRACKET".into(),
        0x005E => return "CIRCUMFLEX ACCENT".into(),
        0x005F => return "LOW LINE".into(),
        0x0060 => return "GRAVE ACCENT".into(),
        0x007B => return "LEFT CURLY BRACKET".into(),
        0x007C => return "VERTICAL LINE".into(),
        0x007D => return "RIGHT CURLY BRACKET".into(),
        0x007E => return "TILDE".into(),
        0x007F => return "DELETE".into(),
        0x00A0 => return "NO-BREAK SPACE".into(),
        0x00A9 => return "COPYRIGHT SIGN".into(),
        0x00AE => return "REGISTERED SIGN".into(),
        0x00B0 => return "DEGREE SIGN".into(),
        0x00B1 => return "PLUS-MINUS SIGN".into(),
        0x00D7 => return "MULTIPLICATION SIGN".into(),
        0x00F7 => return "DIVISION SIGN".into(),
        0x2014 => return "EM DASH".into(),
        0x2018 => return "LEFT SINGLE QUOTATION MARK".into(),
        0x2019 => return "RIGHT SINGLE QUOTATION MARK".into(),
        0x201C => return "LEFT DOUBLE QUOTATION MARK".into(),
        0x201D => return "RIGHT DOUBLE QUOTATION MARK".into(),
        0x2022 => return "BULLET".into(),
        0x2026 => return "HORIZONTAL ELLIPSIS".into(),
        0x20AC => return "EURO SIGN".into(),
        0x2122 => return "TRADE MARK SIGN".into(),
        0x2190 => return "LEFTWARDS ARROW".into(),
        0x2191 => return "UPWARDS ARROW".into(),
        0x2192 => return "RIGHTWARDS ARROW".into(),
        0x2193 => return "DOWNWARDS ARROW".into(),
        0x2194 => return "LEFT RIGHT ARROW".into(),
        0x2260 => return "NOT EQUAL TO".into(),
        0x2264 => return "LESS-THAN OR EQUAL TO".into(),
        0x2265 => return "GREATER-THAN OR EQUAL TO".into(),
        0x221E => return "INFINITY".into(),
        0x2248 => return "ALMOST EQUAL TO".into(),
        0x00B5 => return "MICRO SIGN".into(),
        0x2030 => return "PER MILLE SIGN".into(),
        0x00A3 => return "POUND SIGN".into(),
        0x00A5 => return "YEN SIGN".into(),
        0x00A2 => return "CENT SIGN".into(),
        0x2713 => return "CHECK MARK".into(),
        0x2714 => return "HEAVY CHECK MARK".into(),
        0x2716 => return "HEAVY MULTIPLICATION X".into(),
        0x2717 => return "BALLOT X".into(),
        0x2764 => return "HEAVY BLACK HEART".into(),
        0x2605 => return "BLACK STAR".into(),
        0x2606 => return "WHITE STAR".into(),
        0x266A => return "EIGHTH NOTE".into(),
        0x266B => return "BEAMED EIGHTH NOTES".into(),
        0xFFFD => return "REPLACEMENT CHARACTER".into(),
        _ => {}
    }

    // Range-based naming
    if let Some(ch) = char::from_u32(cp) {
        if (0x0041..=0x005A).contains(&cp) {
            return format!("LATIN CAPITAL LETTER {ch}");
        }
        if (0x0061..=0x007A).contains(&cp) {
            return format!("LATIN SMALL LETTER {}", ch.to_uppercase());
        }
        if (0x0030..=0x0039).contains(&cp) {
            let digit_names = [
                "ZERO", "ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX", "SEVEN", "EIGHT", "NINE",
            ];
            if let Some(idx) = (cp).checked_sub(0x0030)
                && let Some(name) = digit_names.get(idx as usize)
            {
                return format!("DIGIT {name}");
            }
        }
    }

    // Generic name from block + offset
    let blocks = unicode_blocks();
    for block in &blocks {
        if block.contains(cp) {
            return format!("{} (U+{:04X})", block.name, cp);
        }
    }

    format!("UNNAMED CHARACTER U+{cp:04X}")
}

/// Build CharInfo for a codepoint.
fn char_info(cp: u32) -> CharInfo {
    let blocks = unicode_blocks();
    let block_name = blocks
        .iter()
        .find(|b| b.contains(cp))
        .map(|b| b.name.to_string())
        .unwrap_or_else(|| "Unknown Block".to_string());

    CharInfo {
        codepoint: cp,
        name: codepoint_name(cp),
        category: classify_codepoint(cp),
        block_name,
    }
}

// ── Category Filter ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CategoryFilter {
    All,
    Letters,
    Numbers,
    Symbols,
    Punctuation,
    Other,
}

impl CategoryFilter {
    const ALL_FILTERS: [Self; 6] = [
        Self::All,
        Self::Letters,
        Self::Numbers,
        Self::Symbols,
        Self::Punctuation,
        Self::Other,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Letters => "Letters",
            Self::Numbers => "Numbers",
            Self::Symbols => "Symbols",
            Self::Punctuation => "Punctuation",
            Self::Other => "Other",
        }
    }

    fn matches(self, cat: GeneralCategory) -> bool {
        match self {
            Self::All => true,
            Self::Letters => cat.is_letter(),
            Self::Numbers => cat.is_number(),
            Self::Symbols => cat.is_symbol(),
            Self::Punctuation => cat.is_punctuation(),
            Self::Other => {
                !cat.is_letter() && !cat.is_number() && !cat.is_symbol() && !cat.is_punctuation()
            }
        }
    }
}

// ── Preview Size ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewSize {
    Small,
    Medium,
    Large,
    Jumbo,
}

impl PreviewSize {
    fn font_size(self) -> f32 {
        match self {
            Self::Small => 14.0,
            Self::Medium => 24.0,
            Self::Large => 48.0,
            Self::Jumbo => 96.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::Jumbo => "Jumbo",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Small => Self::Medium,
            Self::Medium => Self::Large,
            Self::Large => Self::Jumbo,
            Self::Jumbo => Self::Small,
        }
    }
}

// ── Application State ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Blocks,
    Grid,
    Detail,
    Search,
    Recent,
    Favorites,
}

struct CharMapApp {
    // Block browser
    blocks: Vec<UnicodeBlock>,
    selected_block: usize,
    block_scroll: usize,

    // Character grid (populated from selected block, filtered)
    grid_chars: Vec<u32>,
    selected_char: usize,
    /// First grid row drawn, in rows rather than pixels.
    ///
    /// Kept here rather than recomputed while drawing, which is what it used to
    /// be: `render_grid` derived a scroll position from the selection, used it
    /// for that one frame and discarded it, so the value the rest of the program
    /// read was whatever had last been written by hand. A scroll position that
    /// only exists inside the renderer cannot be scrolled by a wheel.
    grid_scroll: usize,

    // Category filter
    category_filter: CategoryFilter,

    // Search
    search_query: String,
    search_results: Vec<u32>,
    search_active: bool,
    search_selected: usize,

    // Recently used
    recent: Vec<u32>,
    max_recent: usize,

    // Favorites
    favorites: Vec<u32>,

    // Clipboard history (last copied)
    clipboard: Option<String>,
    status_message: String,

    // Active panel
    active_panel: Panel,

    // Preview size
    preview_size: PreviewSize,

    // Viewport
    width: f32,
    height: f32,

    /// Banks fractions of a wheel notch for the block list and for the grid
    /// separately, so half a notch spent on one does not spill into the other.
    block_wheel: wheel::Accumulator,
    grid_wheel: wheel::Accumulator,
}

impl CharMapApp {
    fn new() -> Self {
        let blocks = unicode_blocks();
        let mut app = Self {
            blocks,
            selected_block: 0,
            block_scroll: 0,
            grid_chars: Vec::new(),
            selected_char: 0,
            grid_scroll: 0,
            category_filter: CategoryFilter::All,
            search_query: String::new(),
            search_results: Vec::new(),
            search_active: false,
            search_selected: 0,
            recent: Vec::new(),
            max_recent: 64,
            favorites: Vec::new(),
            clipboard: None,
            status_message: "Select a character to view details".into(),
            active_panel: Panel::Grid,
            preview_size: PreviewSize::Medium,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            block_wheel: wheel::Accumulator::default(),
            grid_wheel: wheel::Accumulator::default(),
        };
        app.populate_grid();
        app
    }

    /// Populate the grid from the selected block, applying category filter.
    fn populate_grid(&mut self) {
        self.grid_chars.clear();
        if let Some(block) = self.blocks.get(self.selected_block) {
            let start = block.start;
            let end = block.end;
            let filter = self.category_filter;
            let mut cp = start;
            while cp <= end {
                let cat = classify_codepoint(cp);
                if filter.matches(cat) {
                    self.grid_chars.push(cp);
                }
                cp = cp.saturating_add(1);
            }
        }
        self.selected_char = 0;
        self.grid_scroll = 0;
    }

    /// The codepoints the grid is currently showing.
    ///
    /// Search results when a search is open, the selected block otherwise. One
    /// function rather than the `if self.search_active` that used to be spelled
    /// out at every site that needed it: those all had to agree about which list
    /// was on screen, and a renderer that drew one while a click handler indexed
    /// the other would put the selection on the wrong character.
    fn shown(&self) -> &[u32] {
        if self.search_active {
            &self.search_results
        } else {
            &self.grid_chars
        }
    }

    /// Index of the selected cell within [`Self::shown`].
    fn selection(&self) -> usize {
        if self.search_active {
            self.search_selected
        } else {
            self.selected_char
        }
    }

    /// Move the selection, clamped to the list that is actually on screen.
    fn set_selection(&mut self, index: usize) {
        let last = self.shown().len().saturating_sub(1);
        let index = index.min(last);
        if self.search_active {
            self.search_selected = index;
        } else {
            self.selected_char = index;
        }
        self.scroll_selection_into_view();
    }

    /// The codepoint under the selection, if the list is not empty.
    fn selected_codepoint(&self) -> Option<u32> {
        self.shown().get(self.selection()).copied()
    }

    /// Get info for the currently selected character.
    fn selected_char_info(&self) -> Option<CharInfo> {
        self.selected_codepoint().map(char_info)
    }

    /// Copy the selected character to clipboard.
    fn copy_selected(&mut self) {
        if let Some(cp) = self.selected_codepoint()
            && let Some(ch) = char::from_u32(cp)
        {
            let s = ch.to_string();
            self.clipboard = Some(s.clone());
            self.add_to_recent(cp);
            self.status_message = format!("Copied '{s}' (U+{cp:04X}) to clipboard");
        }
    }

    /// Add a codepoint to recent list (most recent first, no duplicates).
    fn add_to_recent(&mut self, cp: u32) {
        self.recent.retain(|&c| c != cp);
        self.recent.insert(0, cp);
        if self.recent.len() > self.max_recent {
            self.recent.truncate(self.max_recent);
        }
    }

    /// Toggle favorite.
    fn toggle_favorite(&mut self) {
        if let Some(cp) = self.selected_codepoint() {
            if self.favorites.contains(&cp) {
                self.favorites.retain(|&c| c != cp);
                self.status_message = format!("Removed U+{:04X} from favorites", cp);
            } else {
                self.favorites.push(cp);
                self.status_message = format!("Added U+{:04X} to favorites", cp);
            }
        }
    }

    /// Perform search.
    fn perform_search(&mut self) {
        self.search_results.clear();
        self.search_selected = 0;
        let query = self.search_query.trim().to_lowercase();

        if query.is_empty() {
            return;
        }

        // Search by U+XXXX codepoint
        if let Some(hex_str) = query
            .strip_prefix("u+")
            .or_else(|| query.strip_prefix("0x"))
        {
            if let Ok(cp) = u32::from_str_radix(hex_str, 16)
                && (char::from_u32(cp).is_some() || cp <= 0x10FFFF)
            {
                self.search_results.push(cp);
            }
            return;
        }

        // Search by literal single character
        let chars: Vec<char> = query.chars().collect();
        if chars.len() == 1
            && let Some(&ch) = chars.first()
        {
            self.search_results.push(ch as u32);
        }

        // Search by name across all blocks
        let blocks = unicode_blocks();
        for block in &blocks {
            let mut cp = block.start;
            while cp <= block.end {
                let name = codepoint_name(cp).to_lowercase();
                if name.contains(&query) && !self.search_results.contains(&cp) {
                    self.search_results.push(cp);
                }
                if self.search_results.len() >= 500 {
                    break;
                }
                cp = cp.saturating_add(1);
            }
            if self.search_results.len() >= 500 {
                break;
            }
        }

        if self.search_results.is_empty() {
            self.status_message = format!("No results for '{}'", self.search_query);
        } else {
            self.status_message = format!(
                "Found {} results for '{}'",
                self.search_results.len(),
                self.search_query
            );
        }
    }

    /// Navigate block list.
    fn select_block(&mut self, idx: usize) {
        if idx < self.blocks.len() {
            self.selected_block = idx;
            self.populate_grid();
            self.scroll_block_into_view();
        }
    }

    fn next_block(&mut self) {
        let next = self.selected_block.saturating_add(1);
        if next < self.blocks.len() {
            self.select_block(next);
        }
    }

    fn prev_block(&mut self) {
        if self.selected_block > 0 {
            self.select_block(self.selected_block.saturating_sub(1));
        }
    }

    /// How many cells the grid puts on a row, at the current window size.
    ///
    /// Asked of the layout rather than stored, because the answer changes with
    /// the window width and a stored one does not. This used to be a
    /// `grid_columns: 16` field that no window ever agreed with: at the size the
    /// program opened the grid drew a different number of columns, so Down moved
    /// the selection by sixteen while the eye followed a row of some other
    /// length, and Page Down jumped by five rows of a grid that did not exist.
    fn columns(&self) -> usize {
        self.layout().columns.get()
    }

    /// Navigate grid.
    fn grid_right(&mut self) {
        self.set_selection(self.selection().saturating_add(1));
    }

    fn grid_left(&mut self) {
        self.set_selection(self.selection().saturating_sub(1));
    }

    fn grid_down(&mut self) {
        // Clamped by `set_selection` rather than refused: a Down on the last
        // partial row should land on the last character, not do nothing, which
        // is what a bounds check that rejects the whole move gives you.
        let cols = self.columns();
        self.set_selection(self.selection().saturating_add(cols));
    }

    fn grid_up(&mut self) {
        let cols = self.columns();
        self.set_selection(self.selection().saturating_sub(cols));
    }

    /// Move the selection a screenful, in whichever direction `down` says.
    fn grid_page(&mut self, down: bool) {
        let layout = self.layout();
        // A page is what the window is showing, not a constant five rows. The
        // constant was wrong on every window that was not the one it was written
        // for, and silently: paging simply skipped content or barely moved.
        let step = layout
            .grid_rows
            .max(1)
            .saturating_mul(layout.columns.get())
            .max(1);
        let from = self.selection();
        self.set_selection(if down {
            from.saturating_add(step)
        } else {
            from.saturating_sub(step)
        });
    }

    /// Cycle category filter.
    fn next_category_filter(&mut self) {
        let idx = CategoryFilter::ALL_FILTERS
            .iter()
            .position(|&f| f == self.category_filter)
            .unwrap_or(0);
        let next_idx = idx
            .wrapping_add(1)
            .checked_rem(CategoryFilter::ALL_FILTERS.len())
            .unwrap_or(0);
        self.category_filter = CategoryFilter::ALL_FILTERS
            .get(next_idx)
            .copied()
            .unwrap_or(CategoryFilter::All);
        self.populate_grid();
        self.status_message = format!("Filter: {}", self.category_filter.label());
    }

    /// Open or close the search box.
    ///
    /// One place rather than the four that used to set `search_active` by hand,
    /// because opening a search swaps the list the grid is showing and every
    /// index into the old one — the selection and the scroll position both —
    /// stops meaning anything at that instant.
    fn set_search_active(&mut self, on: bool) {
        self.search_active = on;
        self.grid_scroll = 0;
        self.grid_wheel.reset();
        if on {
            self.active_panel = Panel::Search;
            self.status_message = "Type search query...".into();
        } else {
            self.active_panel = Panel::Grid;
            self.status_message = "Search closed".into();
        }
    }

    /// Handle keyboard input. Returns whether anything changed.
    fn handle_key(&mut self, event: &KeyEvent) -> bool {
        // A release is the same keystroke arriving a second time. Acting on both
        // edges makes every key repeat itself once.
        if !event.pressed {
            return false;
        }
        let ctrl = event.modifiers.ctrl;
        match event.key {
            Key::F if ctrl => self.set_search_active(true),
            Key::C if ctrl => self.copy_selected(),
            Key::Escape if self.search_active => self.set_search_active(false),
            Key::Tab if !ctrl => {
                self.active_panel = match self.active_panel {
                    Panel::Blocks => Panel::Grid,
                    Panel::Grid => Panel::Detail,
                    Panel::Detail => Panel::Search,
                    Panel::Search => Panel::Recent,
                    Panel::Recent => Panel::Favorites,
                    Panel::Favorites => Panel::Blocks,
                };
            }
            Key::Enter => self.copy_selected(),
            Key::Backspace if self.search_active => {
                self.search_query.pop();
                self.perform_search();
            }
            Key::Left => self.grid_left(),
            Key::Right => self.grid_right(),
            Key::Up => {
                if self.active_panel == Panel::Blocks {
                    self.prev_block();
                } else {
                    self.grid_up();
                }
            }
            Key::Down => {
                if self.active_panel == Panel::Blocks {
                    self.next_block();
                } else {
                    self.grid_down();
                }
            }
            Key::PageUp => {
                if self.active_panel == Panel::Blocks {
                    self.select_block(self.selected_block.saturating_sub(BLOCK_PAGE));
                } else {
                    self.grid_page(false);
                }
            }
            Key::PageDown => {
                if self.active_panel == Panel::Blocks {
                    let last = self.blocks.len().saturating_sub(1);
                    self.select_block(self.selected_block.saturating_add(BLOCK_PAGE).min(last));
                } else {
                    self.grid_page(true);
                }
            }
            Key::Home => self.set_selection(0),
            Key::End => self.set_selection(usize::MAX),
            Key::F2 => self.next_category_filter(),
            Key::F3 => {
                self.preview_size = self.preview_size.next();
                self.status_message = format!("Preview: {}", self.preview_size.label());
            }
            // Space is a favourite toggle only while nobody is typing. During a
            // search it is a character like any other, and swallowing it would
            // make "latin small" unsearchable.
            Key::Space if !self.search_active => self.toggle_favorite(),
            _ => {
                // `text`, not the key name: it is the only field that survives a
                // keyboard layout, a held Shift, or a dead key composing `´`
                // and `e` into the single `é` that a name search wants.
                if self.search_active && !ctrl && !event.text.is_empty() {
                    self.search_query.push_str(&event.text);
                    self.perform_search();
                } else {
                    return false;
                }
            }
        }
        true
    }

    // ── Scrolling ──────────────────────────────────────────────────────────

    /// Largest first-row index that still fills the grid.
    fn max_grid_scroll(&self) -> usize {
        let layout = self.layout();
        let rows = self
            .shown()
            .len()
            .div_ceil(layout.columns.get().max(1))
            .max(1);
        rows.saturating_sub(layout.grid_rows)
    }

    /// Scroll the grid so the selected cell is on screen.
    fn scroll_selection_into_view(&mut self) {
        let layout = self.layout();
        let row = self
            .selection()
            .checked_div(layout.columns.get())
            .unwrap_or(0);
        let visible = layout.grid_rows.max(1);
        if row < self.grid_scroll {
            self.grid_scroll = row;
        } else if row >= self.grid_scroll.saturating_add(visible) {
            self.grid_scroll = row.saturating_sub(visible).saturating_add(1);
        }
        self.grid_scroll = self.grid_scroll.min(self.max_grid_scroll());
    }

    /// Scroll the block list so the selected block is on screen.
    fn scroll_block_into_view(&mut self) {
        let visible = self.layout().block_rows.max(1);
        if self.selected_block < self.block_scroll {
            self.block_scroll = self.selected_block;
        } else if self.selected_block >= self.block_scroll.saturating_add(visible) {
            self.block_scroll = self
                .selected_block
                .saturating_sub(visible)
                .saturating_add(1);
        }
        let max = self.blocks.len().saturating_sub(visible);
        self.block_scroll = self.block_scroll.min(max);
    }

    /// A wheel notch over `x, y`. Returns whether anything moved.
    fn handle_scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let layout = self.layout();
        // Which list scrolls is decided by where the pointer is, not by which
        // panel has focus: a wheel is aimed with the hand, and scrolling the
        // sidebar because the keyboard last touched it is the behaviour that
        // makes people stop trusting the wheel.
        if let Some(sidebar) = layout.sidebar.as_ref()
            && sidebar.list.contains(x, y)
        {
            let rows = self.block_wheel.rows(dy);
            let max = self.blocks.len().saturating_sub(layout.block_rows.max(1));
            let next = scroll_window::shift(self.block_scroll, rows).min(max);
            let moved = next != self.block_scroll;
            self.block_scroll = next;
            return moved;
        }
        if layout.grid.contains(x, y) {
            let rows = self.grid_wheel.rows(dy);
            let next = scroll_window::shift(self.grid_scroll, rows).min(self.max_grid_scroll());
            let moved = next != self.grid_scroll;
            self.grid_scroll = next;
            return moved;
        }
        false
    }

    // ── Clicking ───────────────────────────────────────────────────────────

    /// Left-click at `x, y`. Returns whether anything changed.
    fn handle_click(&mut self, x: f32, y: f32) -> bool {
        let Some(target) = self.target_at(x, y) else {
            return false;
        };
        match target {
            Target::Block(idx) => {
                self.active_panel = Panel::Blocks;
                self.select_block(idx);
            }
            Target::Cell(idx) => {
                self.active_panel = if self.search_active {
                    Panel::Search
                } else {
                    Panel::Grid
                };
                // A second click on the cell already selected copies it. The
                // alternative is that the fastest route to the one thing this
                // program exists to do — put a character on the clipboard —
                // runs from wherever the character is to a button on the far
                // side of the window and back.
                if idx == self.selection() {
                    self.copy_selected();
                } else {
                    self.set_selection(idx);
                }
            }
            // A recent or a favourite is a character somebody has already gone
            // looking for once, so a click on one copies it rather than
            // selecting it: selecting would mean finding it in the grid, which
            // may be in a block that is not even open.
            Target::RecentTile(idx) => self.copy_tile(self.recent.get(idx).copied()),
            Target::FavoriteTile(idx) => self.copy_tile(self.favorites.get(idx).copied()),
            Target::Filter => self.next_category_filter(),
            Target::SearchBox => {
                let open = self.search_active;
                self.set_search_active(!open);
            }
            Target::PreviewSize => {
                self.preview_size = self.preview_size.next();
                self.status_message = format!("Preview: {}", self.preview_size.label());
            }
            Target::Star => self.toggle_favorite(),
            Target::Copy => self.copy_selected(),
        }
        true
    }

    /// Copy a character named by a tile rather than by the selection.
    fn copy_tile(&mut self, cp: Option<u32>) {
        if let Some(cp) = cp
            && let Some(ch) = char::from_u32(cp)
        {
            let s = ch.to_string();
            self.clipboard = Some(s.clone());
            self.add_to_recent(cp);
            self.status_message = format!("Copied '{s}' (U+{cp:04X}) to clipboard");
        }
    }

    /// Adopt a new window size.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = sane(width);
        self.height = sane(height);
        // The scroll positions were valid for the old size and need not be for
        // the new one: a window made taller can leave the grid scrolled past
        // content that now fits, which draws an empty grid over a full list.
        self.grid_scroll = self.grid_scroll.min(self.max_grid_scroll());
        let max_block = self
            .blocks
            .len()
            .saturating_sub(self.layout().block_rows.max(1));
        self.block_scroll = self.block_scroll.min(max_block);
    }

    /// The geometry of the current window.
    fn layout(&self) -> Layout {
        Layout::new(self.width, self.height)
    }

    /// What is under `x, y`, if anything.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    // ── Rendering ──────────────────────────────────────────────────────────

    /// Draw the whole window, recording a hit box for every clickable thing.
    fn frame(&self, width: f32, height: f32) -> Frame {
        let width = sane(width);
        let height = sane(height);
        let layout = Layout::new(width, height);
        let mut frame = Frame::new(width, height);

        fill(&mut frame, layout.window, BASE, 0.0);

        if let Some(sidebar) = layout.sidebar.as_ref() {
            self.render_sidebar(&mut frame, sidebar, layout.block_rows);
        }
        self.render_header(&mut frame, &layout);
        self.render_grid(&mut frame, &layout);
        if let Some(detail) = layout.detail.as_ref() {
            self.render_detail(&mut frame, detail);
        }
        self.render_status(&mut frame, layout.status);
        frame
    }

    fn render_sidebar(&self, frame: &mut Frame, sidebar: &Sidebar, rows: usize) {
        fill(frame, sidebar.panel, MANTLE, 0.0);
        frame.clip(sidebar.panel);

        label(
            frame,
            sidebar.panel.x + 8.0,
            sidebar.panel.y + 6.0,
            "Unicode Blocks",
            13.0,
            BLUE,
            FontWeightHint::Bold,
            sidebar.panel.w - 16.0,
        );

        // The filter is a button, not a caption with a keystroke written beside
        // it. `[F2]` in a label is only discoverable by reading it.
        fill(frame, sidebar.filter, SURFACE0, 4.0);
        frame.hit(Target::Filter, sidebar.filter);
        label(
            frame,
            sidebar.filter.x + 6.0,
            sidebar.filter.y + 3.0,
            &format!("Filter: {} [F2]", self.category_filter.label()),
            10.0,
            SUBTEXT0,
            FontWeightHint::Regular,
            sidebar.filter.w - 12.0,
        );

        frame.clip(sidebar.list);
        let shown = scroll_window::visible(
            self.blocks.len(),
            BLOCK_ROW_H,
            sidebar.list.h,
            self.block_scroll,
        );
        for (vi, idx) in (shown.start..shown.end()).enumerate() {
            let Some(block) = self.blocks.get(idx) else {
                break;
            };
            let row = Rect::new(
                sidebar.list.x,
                sidebar.list.y + row_offset(vi, BLOCK_ROW_H),
                sidebar.list.w,
                BLOCK_ROW_H,
            );
            frame.hit(Target::Block(idx), row);
            let selected = idx == self.selected_block;
            if selected {
                fill(
                    frame,
                    Rect::new(row.x + 2.0, row.y, row.w - 4.0, row.h),
                    SURFACE0,
                    4.0,
                );
            }
            label(
                frame,
                row.x + 10.0,
                row.y + 3.0,
                block.name,
                11.0,
                if selected { TEXT_COLOR } else { SUBTEXT1 },
                FontWeightHint::Regular,
                row.w - 20.0,
            );
            label(
                frame,
                row.x + 10.0,
                row.y + 13.0,
                &format!("{:04X}–{:04X} ({})", block.start, block.end, block.len()),
                8.0,
                OVERLAY0,
                FontWeightHint::Regular,
                row.w - 20.0,
            );
        }
        frame.unclip();

        // A hairline saying the list has more below it. Without it a list that
        // exactly fills its space and one that is scrolled look identical.
        if rows < self.blocks.len() {
            fill(
                frame,
                Rect::new(
                    sidebar.list.x,
                    sidebar.list.bottom() - 2.0,
                    sidebar.list.w,
                    2.0,
                ),
                SURFACE2,
                0.0,
            );
        }
        fill(
            frame,
            Rect::new(
                sidebar.panel.right() - 1.0,
                sidebar.panel.y,
                1.0,
                sidebar.panel.h,
            ),
            SURFACE0,
            0.0,
        );
        frame.unclip();
    }

    fn render_header(&self, frame: &mut Frame, layout: &Layout) {
        fill(frame, layout.header, CRUST, 0.0);
        frame.clip(layout.header);

        let title = if self.search_active {
            format!(
                "Search: '{}' ({} results)",
                self.search_query,
                self.search_results.len()
            )
        } else {
            let name = self
                .blocks
                .get(self.selected_block)
                .map_or("???", |b| b.name);
            format!("{} ({} chars)", name, self.grid_chars.len())
        };
        label(
            frame,
            layout.header.x + 8.0,
            layout.header.y + 8.0,
            &title,
            12.0,
            TEXT_COLOR,
            FontWeightHint::Bold,
            (layout.search.x - layout.header.x - 16.0).max(0.0),
        );

        fill(
            frame,
            layout.search,
            if self.search_active { BLUE } else { SURFACE0 },
            4.0,
        );
        frame.hit(Target::SearchBox, layout.search);
        label(
            frame,
            layout.search.x + 6.0,
            layout.search.y + 4.0,
            if self.search_active {
                "Close [Esc]"
            } else {
                "Search [Ctrl+F]"
            },
            10.0,
            if self.search_active { CRUST } else { SUBTEXT0 },
            FontWeightHint::Regular,
            layout.search.w - 12.0,
        );
        frame.unclip();
    }

    fn render_grid(&self, frame: &mut Frame, layout: &Layout) {
        frame.clip(layout.grid);
        let chars = self.shown();
        let selected = self.selection();
        let cols = layout.columns.get();

        if chars.is_empty() {
            label(
                frame,
                layout.grid.x + 8.0,
                layout.grid.y + 8.0,
                if self.search_active {
                    "No characters match that search"
                } else {
                    "No characters in this block pass the filter"
                },
                11.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                layout.grid.w - 16.0,
            );
            frame.unclip();
            return;
        }

        for vi in 0..layout.grid_rows {
            let data_row = self.grid_scroll.saturating_add(vi);
            for col in 0..cols {
                let idx = data_row.saturating_mul(cols).saturating_add(col);
                let Some(&cp) = chars.get(idx) else {
                    break;
                };
                let cell = Rect::new(
                    layout.grid.x + row_offset(col, GRID_CELL),
                    layout.grid.y + row_offset(vi, GRID_CELL),
                    GRID_CELL - 2.0,
                    GRID_CELL - 2.0,
                );
                frame.hit(Target::Cell(idx), cell);

                let is_sel = idx == selected;
                let is_fav = self.favorites.contains(&cp);
                let bg = if is_sel {
                    BLUE
                } else if is_fav {
                    SURFACE1
                } else {
                    SURFACE0
                };
                fill(frame, cell, bg, 4.0);
                if is_fav {
                    label(
                        frame,
                        cell.right() - 8.0,
                        cell.y + 1.0,
                        "*",
                        8.0,
                        YELLOW,
                        FontWeightHint::Bold,
                        8.0,
                    );
                }
                label(
                    frame,
                    cell.x + 4.0,
                    cell.y + 6.0,
                    &cell_glyph(cp),
                    16.0,
                    if is_sel { CRUST } else { TEXT_COLOR },
                    FontWeightHint::Regular,
                    cell.w - 8.0,
                );
                label(
                    frame,
                    cell.x + 2.0,
                    cell.bottom() - 10.0,
                    &format!("{cp:04X}"),
                    7.0,
                    if is_sel { MANTLE } else { OVERLAY0 },
                    FontWeightHint::Regular,
                    cell.w - 4.0,
                );
            }
        }
        frame.unclip();
    }

    fn render_detail(&self, frame: &mut Frame, detail: &Detail) {
        fill(
            frame,
            Rect::new(detail.panel.x, detail.panel.y, 1.0, detail.panel.h),
            SURFACE0,
            0.0,
        );
        fill(
            frame,
            Rect::new(
                detail.panel.x + 1.0,
                detail.panel.y,
                (detail.panel.w - 1.0).max(0.0),
                detail.panel.h,
            ),
            MANTLE,
            0.0,
        );
        frame.clip(detail.panel);

        label(
            frame,
            detail.panel.x + 10.0,
            detail.panel.y + 8.0,
            "Character Detail",
            13.0,
            BLUE,
            FontWeightHint::Bold,
            detail.panel.w - 20.0,
        );

        let Some(info) = self.selected_char_info() else {
            label(
                frame,
                detail.panel.x + 10.0,
                detail.preview.y,
                "No character selected",
                11.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                detail.panel.w - 20.0,
            );
            frame.unclip();
            return;
        };

        fill(frame, detail.preview, SURFACE0, 8.0);
        label(
            frame,
            detail.preview.x + 10.0,
            detail.preview.y + 10.0,
            &info.display_char(),
            self.preview_size.font_size(),
            TEXT_COLOR,
            FontWeightHint::Regular,
            detail.preview.w - 20.0,
        );
        fill(frame, detail.size_button, SURFACE1, 3.0);
        frame.hit(Target::PreviewSize, detail.size_button);
        label(
            frame,
            detail.size_button.x + 4.0,
            detail.size_button.y + 2.0,
            &format!("[F3] {}", self.preview_size.label()),
            8.0,
            SUBTEXT0,
            FontWeightHint::Regular,
            detail.size_button.w - 8.0,
        );

        let fields: [(&str, String); 9] = [
            ("Codepoint", info.codepoint_str()),
            ("Name", info.name.clone()),
            ("Block", info.block_name.clone()),
            (
                "Category",
                format!(
                    "{} — {}",
                    info.category.short_label(),
                    info.category.label()
                ),
            ),
            ("UTF-8", info.utf8_hex()),
            ("HTML Dec", info.html_entity()),
            ("HTML Hex", info.html_hex_entity()),
            ("CSS", info.css_escape()),
            ("Rust", info.rust_escape()),
        ];
        for (i, (name, value)) in fields.iter().enumerate() {
            let y = detail.fields.y + row_offset(i, FIELD_ROW_H);
            label(
                frame,
                detail.fields.x,
                y,
                &format!("{name}:"),
                10.0,
                SUBTEXT0,
                FontWeightHint::Bold,
                70.0,
            );
            label(
                frame,
                detail.fields.x + 70.0,
                y,
                value,
                10.0,
                TEXT_COLOR,
                FontWeightHint::Regular,
                (detail.fields.w - 70.0).max(0.0),
            );
        }

        let is_fav = self.favorites.contains(&info.codepoint);
        fill(
            frame,
            detail.star,
            if is_fav { SURFACE1 } else { SURFACE0 },
            3.0,
        );
        frame.hit(Target::Star, detail.star);
        label(
            frame,
            detail.star.x + 6.0,
            detail.star.y + 3.0,
            if is_fav {
                "* Favourite — click to remove [Space]"
            } else {
                "Add to favourites [Space]"
            },
            10.0,
            if is_fav { YELLOW } else { OVERLAY0 },
            FontWeightHint::Regular,
            detail.star.w - 12.0,
        );

        fill(frame, detail.copy, SURFACE0, 3.0);
        frame.hit(Target::Copy, detail.copy);
        label(
            frame,
            detail.copy.x + 6.0,
            detail.copy.y + 3.0,
            "Copy to clipboard [Enter]",
            10.0,
            GREEN,
            FontWeightHint::Regular,
            detail.copy.w - 12.0,
        );

        self.render_tiles(
            frame,
            detail,
            detail.recent,
            &format!("Recently Used ({})", self.recent.len()),
            TEAL,
            &self.recent,
            SURFACE0,
            Target::RecentTile,
        );
        self.render_tiles(
            frame,
            detail,
            detail.favorites,
            &format!("Favourites ({})", self.favorites.len()),
            YELLOW,
            &self.favorites,
            SURFACE1,
            Target::FavoriteTile,
        );

        frame.unclip();
    }

    /// A titled block of clickable character tiles.
    ///
    /// Recent and Favourites differ in nothing but their colour, their title and
    /// which list they read, so they are one function. Two copies is how the
    /// favourites grid ended up bounds-checking its rows against the panel while
    /// the recent grid above it did not.
    #[allow(clippy::too_many_arguments)]
    fn render_tiles(
        &self,
        frame: &mut Frame,
        detail: &Detail,
        area: Rect,
        title: &str,
        title_color: Color,
        chars: &[u32],
        tile_color: Color,
        target: fn(usize) -> Target,
    ) {
        if area.is_empty() {
            return;
        }
        label(
            frame,
            area.x,
            area.y,
            title,
            11.0,
            title_color,
            FontWeightHint::Bold,
            area.w,
        );
        let grid = Rect::new(
            area.x,
            area.y + TILE_TITLE_H,
            area.w,
            (area.h - TILE_TITLE_H).max(0.0),
        );
        frame.clip(grid);
        let cols = detail.tile_cols.get();
        for (i, &cp) in chars.iter().enumerate() {
            let Some(ch) = char::from_u32(cp) else {
                continue;
            };
            if ch.is_control() {
                continue;
            }
            let tile = Rect::new(
                grid.x + row_offset(i.checked_rem(cols).unwrap_or(0), TILE),
                grid.y + row_offset(i.checked_div(cols).unwrap_or(0), TILE),
                TILE - 2.0,
                TILE - 2.0,
            );
            // Past the bottom of the area there is nothing more to draw, and
            // `frame.hit` would have dropped the box anyway — but stopping here
            // means a favourites list of sixty does not paint fifty-five tiles
            // nobody will ever see.
            if tile.y >= grid.bottom() {
                break;
            }
            fill(frame, tile, tile_color, 3.0);
            frame.hit(target(i), tile);
            label(
                frame,
                tile.x + 3.0,
                tile.y + 3.0,
                &ch.to_string(),
                12.0,
                TEXT_COLOR,
                FontWeightHint::Regular,
                tile.w - 6.0,
            );
        }
        frame.unclip();
    }

    fn render_status(&self, frame: &mut Frame, status: Rect) {
        fill(frame, status, CRUST, 0.0);
        frame.clip(status);
        label(
            frame,
            status.x + 8.0,
            status.y + 8.0,
            &self.status_message,
            11.0,
            SUBTEXT1,
            FontWeightHint::Regular,
            status.w * 0.6,
        );
        let panel = match self.active_panel {
            Panel::Blocks => "Blocks",
            Panel::Grid => "Grid",
            Panel::Detail => "Detail",
            Panel::Search => "Search",
            Panel::Recent => "Recent",
            Panel::Favorites => "Favourites",
        };
        let right = format!("Panel: {panel}  |  [Tab] Switch  |  [Enter] Copy");
        label(
            frame,
            (status.right() - 260.0).max(status.x),
            status.y + 8.0,
            &right,
            10.0,
            OVERLAY0,
            FontWeightHint::Regular,
            250.0,
        );
        frame.unclip();
    }
}

// ── Window geometry ────────────────────────────────────────────────────────

/// The window this program asks for.
///
/// Wide enough for the three columns it wants — block list, grid, detail — at
/// their natural widths, plus enough grid to be worth looking at. Narrower than
/// this and [`Layout`] starts dropping panels, which is the right behaviour but
/// a poor thing to open with.
const WINDOW_WIDTH: f32 = 980.0;
const WINDOW_HEIGHT: f32 = 660.0;

const SIDEBAR_W: f32 = 200.0;
const DETAIL_W: f32 = 260.0;
const STATUS_H: f32 = 28.0;
const HEADER_H: f32 = 30.0;
const BLOCK_ROW_H: f32 = 22.0;
const GRID_CELL: f32 = 36.0;
const TILE: f32 = 24.0;
const TILE_TITLE_H: f32 = 16.0;
const FIELD_ROW_H: f32 = 16.0;
const PREVIEW_H: f32 = 80.0;
const BUTTON_H: f32 = 16.0;
const PAD: f32 = 10.0;

/// The narrowest grid worth keeping. Below it a panel is dropped instead.
const MIN_GRID_W: f32 = GRID_CELL * 4.0;

/// How many blocks Page Up and Page Down move by.
const BLOCK_PAGE: usize = 10;

/// A size that can be laid out.
///
/// NaN is the input that matters. It compares false against everything, so a NaN
/// width propagated into the layout would make every `contains` miss and the
/// whole window would look painted-on and dead rather than obviously broken.
fn sane(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Where item `index` starts in a run of `pitch`-sized slots.
#[allow(clippy::cast_precision_loss)]
fn row_offset(index: usize, pitch: f32) -> f32 {
    (index as f32) * pitch
}

/// How many whole `pitch`-sized rows fit in `span`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn rows_in(span: f32, pitch: f32) -> usize {
    if !span.is_finite() || span <= 0.0 || pitch <= 0.0 {
        return 0;
    }
    (span / pitch) as usize
}

/// Take `want` off the top of the band starting at `*y`, never past `limit`.
fn take_band(y: &mut f32, limit: f32, x: f32, width: f32, want: f32) -> Rect {
    let h = want.min((limit - *y).max(0.0));
    let band = Rect::new(x, *y, width, h);
    *y += h;
    band
}

/// The left column.
#[derive(Clone, Debug)]
struct Sidebar {
    panel: Rect,
    filter: Rect,
    list: Rect,
}

/// The right column.
#[derive(Clone, Debug)]
struct Detail {
    panel: Rect,
    preview: Rect,
    size_button: Rect,
    fields: Rect,
    star: Rect,
    copy: Rect,
    recent: Rect,
    favorites: Rect,
    tile_cols: NonZeroUsize,
}

/// Every rectangle in the window, derived from its size.
///
/// Built fresh on each frame and never remembered. A layout kept in a field is a
/// second copy of a fact the window already knows, and the two disagree from the
/// first resize that arrives while nothing is being drawn.
#[derive(Clone, Debug)]
struct Layout {
    window: Rect,
    sidebar: Option<Sidebar>,
    header: Rect,
    search: Rect,
    grid: Rect,
    columns: NonZeroUsize,
    grid_rows: usize,
    block_rows: usize,
    detail: Option<Detail>,
    status: Rect,
}

impl Layout {
    fn new(width: f32, height: f32) -> Self {
        let width = sane(width);
        let height = sane(height);
        let window = Rect::new(0.0, 0.0, width, height);
        let status_h = STATUS_H.min(height);
        let body_h = (height - status_h).max(0.0);
        let status = Rect::new(0.0, body_h, width, status_h);

        // Panels are dropped rather than squeezed. A 40px-wide block list shows
        // no block name and a 40px detail panel shows no character, so what a
        // narrow window gets is fewer panels at their real widths, not three
        // panels too small to read.
        let mut sidebar_w = 0.0;
        let mut detail_w = 0.0;
        if width - SIDEBAR_W >= MIN_GRID_W {
            sidebar_w = SIDEBAR_W;
        }
        if width - sidebar_w - DETAIL_W >= MIN_GRID_W {
            detail_w = DETAIL_W;
        }
        let main_x = sidebar_w;
        let main_w = (width - sidebar_w - detail_w).max(0.0);

        let block_list_top = 24.0 + BUTTON_H + 4.0;
        let sidebar = if sidebar_w > 0.0 {
            Some(Sidebar {
                panel: Rect::new(0.0, 0.0, sidebar_w, body_h),
                filter: Rect::new(8.0, 24.0, (sidebar_w - 16.0).max(0.0), BUTTON_H),
                list: Rect::new(
                    0.0,
                    block_list_top.min(body_h),
                    sidebar_w,
                    (body_h - block_list_top).max(0.0),
                ),
            })
        } else {
            None
        };
        let block_rows = sidebar
            .as_ref()
            .map_or(0, |s| rows_in(s.list.h, BLOCK_ROW_H));

        let header_h = HEADER_H.min(body_h);
        let header = Rect::new(main_x, 0.0, main_w, header_h);
        let search_w = 96.0_f32.min((main_w - 16.0).max(0.0));
        let search = Rect::new(
            header.right() - search_w - 8.0,
            header.y + 6.0,
            search_w,
            BUTTON_H + 2.0,
        );
        let grid = Rect::new(
            main_x + 4.0,
            header_h + 4.0,
            (main_w - 8.0).max(0.0),
            (body_h - header_h - 8.0).max(0.0),
        );
        let columns = guitk::grid::columns_across(grid.w, GRID_CELL, 0.0);
        let grid_rows = rows_in(grid.h, GRID_CELL);

        let detail = if detail_w > 0.0 {
            Some(Detail::new(Rect::new(
                main_x + main_w,
                0.0,
                detail_w,
                body_h,
            )))
        } else {
            None
        };

        Self {
            window,
            sidebar,
            header,
            search,
            grid,
            columns,
            grid_rows,
            block_rows,
            detail,
            status,
        }
    }
}

impl Detail {
    fn new(panel: Rect) -> Self {
        let x = panel.x + PAD;
        let w = (panel.w - PAD * 2.0).max(0.0);
        let limit = panel.bottom();
        let mut y = (panel.y + 30.0).min(limit);

        let preview = take_band(&mut y, limit, x, w, PREVIEW_H);
        let size_button = Rect::new(
            (preview.right() - 74.0).max(preview.x),
            (preview.bottom() - 18.0).max(preview.y),
            72.0_f32.min(preview.w),
            14.0_f32.min(preview.h),
        );
        y += 10.0;
        let fields = take_band(&mut y, limit, x, w, FIELD_ROW_H * 9.0);
        y += 8.0;
        let star = take_band(&mut y, limit, x, w, BUTTON_H);
        y += 3.0;
        let copy = take_band(&mut y, limit, x, w, BUTTON_H);
        y += 10.0;

        let tile_cols = guitk::grid::columns_across(w, TILE, 0.0);
        // Recent gets three rows and Favourites gets whatever is left, rather
        // than both getting three: the window is the thing that decides how much
        // room there is, and a fixed split leaves a gap at the bottom of a tall
        // window and overflows a short one.
        let recent = take_band(&mut y, limit, x, w, TILE_TITLE_H + TILE * 3.0);
        y += 6.0;
        let rest = (limit - y).max(0.0);
        let favorites = take_band(&mut y, limit, x, w, rest);

        Self {
            panel,
            preview,
            size_button,
            fields,
            star,
            copy,
            recent,
            favorites,
            tile_cols,
        }
    }
}

// ── Frame targets ──────────────────────────────────────────────────────────

/// Everything in the window that can be clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A row of the block list, by index into `blocks`.
    Block(usize),
    /// A character cell, by index into whichever list the grid is showing.
    Cell(usize),
    /// A tile in the Recently Used strip.
    RecentTile(usize),
    /// A tile in the Favourites strip.
    FavoriteTile(usize),
    /// The category-filter button.
    Filter,
    /// The search toggle in the grid's header.
    SearchBox,
    /// The preview-size cycle button.
    PreviewSize,
    /// Add or remove the selected character from favourites.
    Star,
    /// Copy the selected character.
    Copy,
}

/// This program's frame type.
pub type Frame = guitk::frame::Frame<Target>;

/// What a cell draws for a codepoint.
///
/// A control character has no picture, so it gets its number instead. Drawing
/// the character itself would send a `BEL` or a `SUB` to the text shaper and get
/// back whatever that font uses for "nothing", which is indistinguishable
/// between the thirty-three of them.
fn cell_glyph(cp: u32) -> String {
    match char::from_u32(cp) {
        Some(ch) if !ch.is_control() => ch.to_string(),
        _ => format!("{cp:02X}"),
    }
}

/// A filled rectangle.
fn fill(frame: &mut Frame, rect: Rect, color: Color, radius: f32) {
    if rect.is_empty() {
        return;
    }
    frame.push(RenderCommand::FillRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color,
        corner_radii: if radius > 0.0 {
            CornerRadii::all(radius)
        } else {
            CornerRadii::ZERO
        },
    });
}

/// A line of text, elided rather than overrun.
#[allow(clippy::too_many_arguments)]
fn label(
    frame: &mut Frame,
    x: f32,
    y: f32,
    text: &str,
    font_size: f32,
    color: Color,
    font_weight: FontWeightHint,
    max_width: f32,
) {
    if max_width <= 0.0 {
        return;
    }
    frame.push(RenderCommand::Text {
        x,
        y,
        text: text.to_string(),
        font_size,
        color,
        font_weight,
        max_width: Some(max_width),
        overflow: TextOverflow::Ellipsis,
    });
}

// ── Event plumbing ─────────────────────────────────────────────────────────

/// One event, applied to the app.
///
/// A free function so that [`App::on_event`] and [`Probe::click_at`] are both
/// thin adapters over the same body rather than two dispatchers that have to be
/// kept in step — which is the arrangement where a test passes against a code
/// path the window never takes.
fn handle_event(app: &mut CharMapApp, event: &Event) -> EventResult {
    fn result(changed: bool) -> EventResult {
        if changed {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
    match event {
        Event::Mouse(m) => match m.kind {
            MouseEventKind::Press(MouseButton::Left) => result(app.handle_click(m.x, m.y)),
            MouseEventKind::Scroll { dy, .. } => result(app.handle_scroll(m.x, m.y, dy)),
            _ => EventResult::Ignored,
        },
        Event::Key(k) => result(app.handle_key(k)),
        Event::Resize { width, height } => {
            #[allow(clippy::cast_precision_loss)]
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for CharMapApp {
    fn title(&self) -> String {
        String::from("Character Map")
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// No tick.
    ///
    /// Nothing here moves on its own: a Unicode block contains what it contained
    /// a second ago. A timer would repaint an identical picture forever and keep
    /// the machine awake to do it.
    fn tick_interval(&self) -> Option<std::time::Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for CharMapApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(
            self,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
        )
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

impl Default for CharMapApp {
    fn default() -> Self {
        Self::new()
    }
}

// ── Entry point ────────────────────────────────────────────────────────────

fn main() -> ExitCode {
    let mut app = CharMapApp::new();
    app::launch("charmap", &mut app)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::probe;

    // ── Unicode Block tests ────────────────────────────────────────────

    #[test]
    fn test_block_creation() {
        let block = UnicodeBlock::new("Basic Latin", 0x0000, 0x007F);
        assert_eq!(block.name, "Basic Latin");
        assert_eq!(block.start, 0x0000);
        assert_eq!(block.end, 0x007F);
    }

    #[test]
    fn test_block_len() {
        let block = UnicodeBlock::new("Test", 0x0000, 0x007F);
        assert_eq!(block.len(), 128);
    }

    #[test]
    fn test_block_contains() {
        let block = UnicodeBlock::new("Test", 0x0041, 0x005A);
        assert!(block.contains(0x0041));
        assert!(block.contains(0x004D));
        assert!(block.contains(0x005A));
        assert!(!block.contains(0x0040));
        assert!(!block.contains(0x005B));
    }

    #[test]
    fn test_unicode_blocks_count() {
        let blocks = unicode_blocks();
        assert!(blocks.len() >= 40);
    }

    #[test]
    fn test_blocks_non_overlapping_within() {
        let blocks = unicode_blocks();
        for block in &blocks {
            assert!(
                block.start <= block.end,
                "Block '{}' has start > end",
                block.name
            );
        }
    }

    // ── Category Classification tests ──────────────────────────────────

    #[test]
    fn test_classify_uppercase() {
        assert_eq!(classify_codepoint(0x0041), GeneralCategory::UppercaseLetter); // 'A'
        assert_eq!(classify_codepoint(0x005A), GeneralCategory::UppercaseLetter); // 'Z'
    }

    #[test]
    fn test_classify_lowercase() {
        assert_eq!(classify_codepoint(0x0061), GeneralCategory::LowercaseLetter); // 'a'
        assert_eq!(classify_codepoint(0x007A), GeneralCategory::LowercaseLetter); // 'z'
    }

    #[test]
    fn test_classify_digit() {
        assert_eq!(classify_codepoint(0x0030), GeneralCategory::DecimalNumber); // '0'
        assert_eq!(classify_codepoint(0x0039), GeneralCategory::DecimalNumber); // '9'
    }

    #[test]
    fn test_classify_control() {
        assert_eq!(classify_codepoint(0x0000), GeneralCategory::Control);
        assert_eq!(classify_codepoint(0x001F), GeneralCategory::Control);
        assert_eq!(classify_codepoint(0x007F), GeneralCategory::Control);
    }

    #[test]
    fn test_classify_space() {
        assert_eq!(classify_codepoint(0x0020), GeneralCategory::SpaceSeparator);
        assert_eq!(classify_codepoint(0x00A0), GeneralCategory::SpaceSeparator);
    }

    #[test]
    fn test_classify_math_symbol() {
        assert_eq!(classify_codepoint(0x002B), GeneralCategory::MathSymbol); // '+'
        assert_eq!(classify_codepoint(0x003D), GeneralCategory::MathSymbol); // '='
    }

    #[test]
    fn test_classify_currency() {
        assert_eq!(classify_codepoint(0x0024), GeneralCategory::CurrencySymbol); // '$'
        assert_eq!(classify_codepoint(0x20AC), GeneralCategory::CurrencySymbol); // Euro
    }

    #[test]
    fn test_classify_punctuation() {
        assert_eq!(
            classify_codepoint(0x0021),
            GeneralCategory::OtherPunctuation
        ); // '!'
        assert_eq!(
            classify_codepoint(0x003F),
            GeneralCategory::OtherPunctuation
        ); // '?'
    }

    #[test]
    fn test_classify_open_close_punct() {
        assert_eq!(classify_codepoint(0x0028), GeneralCategory::OpenPunctuation); // '('
        assert_eq!(
            classify_codepoint(0x0029),
            GeneralCategory::ClosePunctuation
        ); // ')'
        assert_eq!(classify_codepoint(0x005B), GeneralCategory::OpenPunctuation); // '['
        assert_eq!(
            classify_codepoint(0x005D),
            GeneralCategory::ClosePunctuation
        ); // ']'
    }

    #[test]
    fn test_classify_cjk() {
        assert_eq!(classify_codepoint(0x4E00), GeneralCategory::OtherLetter); // CJK
    }

    #[test]
    fn test_classify_emoji() {
        assert_eq!(classify_codepoint(0x1F600), GeneralCategory::OtherSymbol);
    }

    // ── Category filter tests ──────────────────────────────────────────

    #[test]
    fn test_category_filter_all() {
        let filter = CategoryFilter::All;
        assert!(filter.matches(GeneralCategory::UppercaseLetter));
        assert!(filter.matches(GeneralCategory::Control));
        assert!(filter.matches(GeneralCategory::MathSymbol));
    }

    #[test]
    fn test_category_filter_letters() {
        let filter = CategoryFilter::Letters;
        assert!(filter.matches(GeneralCategory::UppercaseLetter));
        assert!(filter.matches(GeneralCategory::LowercaseLetter));
        assert!(!filter.matches(GeneralCategory::DecimalNumber));
        assert!(!filter.matches(GeneralCategory::MathSymbol));
    }

    #[test]
    fn test_category_filter_numbers() {
        let filter = CategoryFilter::Numbers;
        assert!(filter.matches(GeneralCategory::DecimalNumber));
        assert!(filter.matches(GeneralCategory::LetterNumber));
        assert!(!filter.matches(GeneralCategory::UppercaseLetter));
    }

    #[test]
    fn test_category_filter_symbols() {
        let filter = CategoryFilter::Symbols;
        assert!(filter.matches(GeneralCategory::MathSymbol));
        assert!(filter.matches(GeneralCategory::CurrencySymbol));
        assert!(!filter.matches(GeneralCategory::UppercaseLetter));
    }

    #[test]
    fn test_category_filter_punctuation() {
        let filter = CategoryFilter::Punctuation;
        assert!(filter.matches(GeneralCategory::OtherPunctuation));
        assert!(filter.matches(GeneralCategory::DashPunctuation));
        assert!(!filter.matches(GeneralCategory::UppercaseLetter));
    }

    // ── CharInfo tests ─────────────────────────────────────────────────

    #[test]
    fn test_char_info_ascii() {
        let info = char_info(0x0041);
        assert_eq!(info.codepoint, 0x0041);
        assert!(info.name.contains("LATIN CAPITAL LETTER A"));
        assert_eq!(info.display_char(), "A");
        assert_eq!(info.codepoint_str(), "U+0041");
    }

    #[test]
    fn test_char_info_utf8_bytes() {
        let info = char_info(0x0041); // 'A' — single byte
        assert_eq!(info.utf8_bytes(), vec![0x41]);

        let info2 = char_info(0x00E9); // 'é' — two bytes
        assert_eq!(info2.utf8_bytes(), vec![0xC3, 0xA9]);

        let info3 = char_info(0x20AC); // '€' — three bytes
        assert_eq!(info3.utf8_bytes(), vec![0xE2, 0x82, 0xAC]);
    }

    #[test]
    fn test_char_info_html_entity() {
        let info = char_info(0x0041);
        assert_eq!(info.html_entity(), "&#65;");
        assert_eq!(info.html_hex_entity(), "&#x41;");
    }

    #[test]
    fn test_char_info_css_escape() {
        let info = char_info(0x20AC);
        assert_eq!(info.css_escape(), "\\20AC");
    }

    #[test]
    fn test_char_info_rust_escape() {
        let info = char_info(0x0041);
        assert_eq!(info.rust_escape(), "'\\u{0041}'");
    }

    #[test]
    fn test_char_info_control_display() {
        let info = char_info(0x0000);
        assert_eq!(info.display_char(), "U+0000");
    }

    #[test]
    fn test_char_info_emoji_display() {
        let info = char_info(0x1F600);
        // Should be a valid character display
        assert!(!info.display_char().is_empty());
    }

    #[test]
    fn test_char_info_high_codepoint_str() {
        let info = char_info(0x1F600);
        assert_eq!(info.codepoint_str(), "U+1F600");
    }

    // ── Codepoint naming tests ─────────────────────────────────────────

    #[test]
    fn test_name_letters() {
        assert!(codepoint_name(0x0041).contains("LATIN CAPITAL LETTER A"));
        assert!(codepoint_name(0x0061).contains("LATIN SMALL LETTER A"));
    }

    #[test]
    fn test_name_digits() {
        assert!(codepoint_name(0x0030).contains("DIGIT ZERO"));
        assert!(codepoint_name(0x0039).contains("DIGIT NINE"));
    }

    #[test]
    fn test_name_special() {
        assert_eq!(codepoint_name(0x0020), "SPACE");
        assert_eq!(codepoint_name(0x000A), "LINE FEED");
        assert_eq!(codepoint_name(0xFFFD), "REPLACEMENT CHARACTER");
    }

    #[test]
    fn test_name_symbols() {
        assert_eq!(codepoint_name(0x0024), "DOLLAR SIGN");
        assert_eq!(codepoint_name(0x20AC), "EURO SIGN");
        assert_eq!(codepoint_name(0x00A9), "COPYRIGHT SIGN");
    }

    // ── App construction tests ─────────────────────────────────────────

    #[test]
    fn test_app_creation() {
        let app = CharMapApp::new();
        assert!(!app.blocks.is_empty());
        assert_eq!(app.selected_block, 0);
        assert!(!app.grid_chars.is_empty());
    }

    #[test]
    fn test_app_default_grid_is_basic_latin() {
        let app = CharMapApp::new();
        // Basic Latin block: 0x0000-0x007F
        assert!(app.grid_chars.contains(&0x0041)); // 'A'
        assert!(app.grid_chars.contains(&0x0061)); // 'a'
    }

    #[test]
    fn test_app_select_block() {
        let mut app = CharMapApp::new();
        app.select_block(1); // Latin-1 Supplement
        assert_eq!(app.selected_block, 1);
        assert!(app.grid_chars.contains(&0x00C0)); // 'À'
    }

    #[test]
    fn test_app_next_prev_block() {
        let mut app = CharMapApp::new();
        app.next_block();
        assert_eq!(app.selected_block, 1);
        app.prev_block();
        assert_eq!(app.selected_block, 0);
        // prev at 0 stays 0
        app.prev_block();
        assert_eq!(app.selected_block, 0);
    }

    #[test]
    fn test_app_category_filter() {
        let mut app = CharMapApp::new();
        let all_count = app.grid_chars.len();
        app.category_filter = CategoryFilter::Letters;
        app.populate_grid();
        let letter_count = app.grid_chars.len();
        assert!(letter_count < all_count);
        assert!(letter_count > 0);
    }

    #[test]
    fn test_app_next_category_filter() {
        let mut app = CharMapApp::new();
        assert_eq!(app.category_filter, CategoryFilter::All);
        app.next_category_filter();
        assert_eq!(app.category_filter, CategoryFilter::Letters);
        app.next_category_filter();
        assert_eq!(app.category_filter, CategoryFilter::Numbers);
    }

    // ── Navigation tests ───────────────────────────────────────────────

    #[test]
    fn test_grid_navigation() {
        let mut app = CharMapApp::new();
        assert_eq!(app.selected_char, 0);
        app.grid_right();
        assert_eq!(app.selected_char, 1);
        app.grid_left();
        assert_eq!(app.selected_char, 0);
        // left at 0 stays 0
        app.grid_left();
        assert_eq!(app.selected_char, 0);
    }

    #[test]
    fn test_grid_down_up() {
        let mut app = CharMapApp::new();
        // The step is asked of the layout rather than asserted as a constant,
        // because the whole point of the rewrite is that there is no second
        // opinion about how wide a row is. A hardcoded 16 here would only
        // re-create the disagreement it is meant to rule out.
        let columns = app.layout().columns.get();
        app.grid_down();
        assert_eq!(app.selected_char, columns);
        app.grid_up();
        assert_eq!(app.selected_char, 0);
    }

    // ── Copy and clipboard tests ───────────────────────────────────────

    #[test]
    fn test_copy_selected() {
        let mut app = CharMapApp::new();
        // Navigate to 'A' (0x0041) — it should be in the grid
        if let Some(pos) = app.grid_chars.iter().position(|&cp| cp == 0x0041) {
            app.selected_char = pos;
        }
        app.copy_selected();
        assert_eq!(app.clipboard, Some("A".to_string()));
        assert!(app.status_message.contains("Copied"));
    }

    #[test]
    fn test_copy_adds_to_recent() {
        let mut app = CharMapApp::new();
        if let Some(pos) = app.grid_chars.iter().position(|&cp| cp == 0x0041) {
            app.selected_char = pos;
        }
        app.copy_selected();
        assert!(app.recent.contains(&0x0041));
    }

    // ── Recent list tests ──────────────────────────────────────────────

    #[test]
    fn test_add_to_recent() {
        let mut app = CharMapApp::new();
        app.add_to_recent(0x0041);
        app.add_to_recent(0x0042);
        app.add_to_recent(0x0043);
        assert_eq!(app.recent.first(), Some(&0x0043));
        assert_eq!(app.recent.len(), 3);
    }

    #[test]
    fn test_recent_no_duplicates() {
        let mut app = CharMapApp::new();
        app.add_to_recent(0x0041);
        app.add_to_recent(0x0042);
        app.add_to_recent(0x0041); // re-add A
        assert_eq!(app.recent.len(), 2);
        assert_eq!(app.recent.first(), Some(&0x0041)); // A is now first
    }

    #[test]
    fn test_recent_max_limit() {
        let mut app = CharMapApp::new();
        app.max_recent = 5;
        for i in 0u32..10 {
            app.add_to_recent(0x0041u32.saturating_add(i));
        }
        assert_eq!(app.recent.len(), 5);
    }

    // ── Favorites tests ────────────────────────────────────────────────

    #[test]
    fn test_toggle_favorite_add() {
        let mut app = CharMapApp::new();
        if let Some(pos) = app.grid_chars.iter().position(|&cp| cp == 0x0041) {
            app.selected_char = pos;
        }
        app.toggle_favorite();
        assert!(app.favorites.contains(&0x0041));
    }

    #[test]
    fn test_toggle_favorite_remove() {
        let mut app = CharMapApp::new();
        app.favorites.push(0x0041);
        if let Some(pos) = app.grid_chars.iter().position(|&cp| cp == 0x0041) {
            app.selected_char = pos;
        }
        app.toggle_favorite();
        assert!(!app.favorites.contains(&0x0041));
    }

    // ── Search tests ───────────────────────────────────────────────────

    #[test]
    fn test_search_by_codepoint() {
        let mut app = CharMapApp::new();
        app.search_query = "U+0041".into();
        app.perform_search();
        assert!(app.search_results.contains(&0x0041));
    }

    #[test]
    fn test_search_by_hex() {
        let mut app = CharMapApp::new();
        app.search_query = "0x20AC".into();
        app.perform_search();
        assert!(app.search_results.contains(&0x20AC));
    }

    #[test]
    fn test_search_by_name() {
        let mut app = CharMapApp::new();
        app.search_query = "DOLLAR".into();
        app.perform_search();
        assert!(app.search_results.contains(&0x0024));
    }

    #[test]
    fn test_search_by_literal_char() {
        let mut app = CharMapApp::new();
        app.search_query = "A".into();
        app.perform_search();
        assert!(app.search_results.contains(&0x0041));
    }

    #[test]
    fn test_search_empty_query() {
        let mut app = CharMapApp::new();
        app.search_query = String::new();
        app.perform_search();
        assert!(app.search_results.is_empty());
    }

    #[test]
    fn test_search_result_limit() {
        let mut app = CharMapApp::new();
        // This should match many characters
        app.search_query = "LATIN".into();
        app.perform_search();
        assert!(app.search_results.len() <= 500);
    }

    // ── Key handling tests ─────────────────────────────────────────────

    #[test]
    fn test_key_tab_cycles_panels() {
        let mut app = CharMapApp::new();
        assert_eq!(app.active_panel, Panel::Grid);
        probe::key(&mut app, &probe::press(Key::Tab));
        assert_eq!(app.active_panel, Panel::Detail);
        probe::key(&mut app, &probe::press(Key::Tab));
        assert_eq!(app.active_panel, Panel::Search);
    }

    #[test]
    fn test_key_ctrl_f_activates_search() {
        let mut app = CharMapApp::new();
        assert!(!app.search_active);
        probe::key(&mut app, &probe::ctrl(Key::F));
        assert!(app.search_active);
        assert_eq!(app.active_panel, Panel::Search);
    }

    #[test]
    fn test_key_escape_closes_search() {
        let mut app = CharMapApp::new();
        app.set_search_active(true);
        probe::key(&mut app, &probe::press(Key::Escape));
        assert!(!app.search_active);
    }

    #[test]
    fn test_key_enter_copies() {
        let mut app = CharMapApp::new();
        if let Some(pos) = app.grid_chars.iter().position(|&cp| cp == 0x0041) {
            app.selected_char = pos;
        }
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_eq!(app.clipboard, Some("A".to_string()));
    }

    #[test]
    fn test_key_space_toggles_favorite() {
        let mut app = CharMapApp::new();
        if let Some(pos) = app.grid_chars.iter().position(|&cp| cp == 0x0041) {
            app.selected_char = pos;
        }
        probe::key(&mut app, &probe::press(Key::Space));
        assert!(app.favorites.contains(&0x0041));
    }

    /// Space types a space while a search is open instead of toggling a star.
    ///
    /// "LATIN SMALL" is two words, and a Space key that always meant "favourite"
    /// would make every multi-word name in the Unicode tables unsearchable.
    #[test]
    fn a_space_typed_into_a_search_is_a_space_not_a_favourite() {
        let mut app = CharMapApp::new();
        app.set_search_active(true);
        app.search_query = "LATIN".into();
        // A real Space keystroke carries its own text; `press` alone does not,
        // because a key name is not what a keyboard layout produces.
        let mut space = probe::press(Key::Space);
        space.text = " ".into();
        probe::key(&mut app, &space);
        assert_eq!(app.search_query, "LATIN ");
        assert!(app.favorites.is_empty());
    }

    #[test]
    fn test_key_f2_cycles_filter() {
        let mut app = CharMapApp::new();
        let before = app.category_filter;
        probe::key(&mut app, &probe::press(Key::F2));
        assert_ne!(app.category_filter, before);
    }

    #[test]
    fn test_key_f3_cycles_preview() {
        let mut app = CharMapApp::new();
        assert_eq!(app.preview_size, PreviewSize::Medium);
        probe::key(&mut app, &probe::press(Key::F3));
        assert_eq!(app.preview_size, PreviewSize::Large);
    }

    #[test]
    fn test_search_typing() {
        let mut app = CharMapApp::new();
        app.set_search_active(true);
        probe::type_str(&mut app, "DOL");
        assert_eq!(app.search_query, "DOL");
    }

    /// A key with no `text` cannot type, whatever its name is.
    ///
    /// The old handler dispatched on a key *name* string, so a synthetic
    /// `"F13"`-shaped name would have landed in the query. Typing is decided by
    /// the one field a keyboard layout actually fills in.
    #[test]
    fn a_key_that_produces_no_text_types_nothing() {
        let mut app = CharMapApp::new();
        app.set_search_active(true);
        probe::key(&mut app, &probe::press(Key::F5));
        assert!(app.search_query.is_empty());
    }

    #[test]
    fn test_search_backspace() {
        let mut app = CharMapApp::new();
        app.set_search_active(true);
        app.search_query = "DOL".into();
        probe::key(&mut app, &probe::press(Key::Backspace));
        assert_eq!(app.search_query, "DO");
    }

    /// A key release does nothing, so no keystroke happens twice.
    #[test]
    fn a_key_release_is_not_a_second_keystroke() {
        let mut app = CharMapApp::new();
        app.set_search_active(true);
        let mut down = probe::typing("A");
        probe::key(&mut app, &down);
        down.pressed = false;
        probe::key(&mut app, &down);
        assert_eq!(app.search_query, "A");
    }

    // ── Preview size tests ─────────────────────────────────────────────

    #[test]
    fn test_preview_size_cycle() {
        assert_eq!(PreviewSize::Small.next(), PreviewSize::Medium);
        assert_eq!(PreviewSize::Medium.next(), PreviewSize::Large);
        assert_eq!(PreviewSize::Large.next(), PreviewSize::Jumbo);
        assert_eq!(PreviewSize::Jumbo.next(), PreviewSize::Small);
    }

    #[test]
    fn test_preview_size_font() {
        assert!(PreviewSize::Small.font_size() < PreviewSize::Medium.font_size());
        assert!(PreviewSize::Medium.font_size() < PreviewSize::Large.font_size());
        assert!(PreviewSize::Large.font_size() < PreviewSize::Jumbo.font_size());
    }

    // ── Render tests ───────────────────────────────────────────────────

    /// Every frame the app draws is a well-formed one.
    ///
    /// `is_balanced` is the check that no clip or translate was pushed and left
    /// pushed. An unbalanced frame does not fail here — it fails as a panel
    /// drawn somewhere else entirely, three states later.
    fn assert_frame_ok(frame: &Frame) {
        assert!(!frame.commands().is_empty());
        assert!(frame.is_balanced());
    }

    #[test]
    fn test_render_produces_commands() {
        let app = CharMapApp::new();
        assert_frame_ok(&app.draw(CharMapApp::SIZE));
    }

    #[test]
    fn test_render_contains_background() {
        let app = CharMapApp::new();
        let frame = app.draw(CharMapApp::SIZE);
        let has_bg = frame.commands().iter().any(
            |cmd| matches!(cmd, RenderCommand::FillRect { x, y, .. } if *x == 0.0 && *y == 0.0),
        );
        assert!(has_bg);
    }

    #[test]
    fn test_render_with_selection() {
        let mut app = CharMapApp::new();
        app.set_selection(5);
        assert_frame_ok(&app.draw(CharMapApp::SIZE));
    }

    #[test]
    fn test_render_with_search_active() {
        let mut app = CharMapApp::new();
        app.set_search_active(true);
        app.search_query = "DOLLAR".into();
        app.perform_search();
        assert_frame_ok(&app.draw(CharMapApp::SIZE));
    }

    #[test]
    fn test_render_with_favorites() {
        let mut app = CharMapApp::new();
        app.favorites.push(0x0041);
        app.favorites.push(0x0042);
        assert_frame_ok(&app.draw(CharMapApp::SIZE));
    }

    #[test]
    fn test_render_with_recent() {
        let mut app = CharMapApp::new();
        app.add_to_recent(0x0041);
        app.add_to_recent(0x0042);
        assert_frame_ok(&app.draw(CharMapApp::SIZE));
    }

    // ── Window tests ───────────────────────────────────────────────────

    /// The window opens at the size the layout constants describe.
    #[test]
    fn the_window_opens_at_the_size_the_layout_is_drawn_for() {
        let app = CharMapApp::new();
        assert_eq!(app.title(), "Character Map");
        assert_eq!(
            app.initial_size(),
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        );
        // Nothing in a Unicode table changes on its own, so a repaint timer
        // would redraw an identical picture forever and keep the CPU awake.
        assert!(app.tick_interval().is_none());
    }

    /// Closing the window exits rather than being ignored as an unknown event.
    #[test]
    fn a_close_request_exits() {
        let mut app = CharMapApp::new();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    /// A click lands on the cell the eye sees, because both read one frame.
    #[test]
    fn clicking_a_cell_selects_the_character_drawn_in_it() {
        let mut app = CharMapApp::new();
        let rect = probe::rect_of(&app, Target::Cell(7)).expect("cell 7 is on screen");
        let (x, y) = rect.centre();
        assert_eq!(
            probe::click_with(&mut app, Target::Cell(7), MouseButton::Left),
            EventResult::Consumed
        );
        assert_eq!(app.selected_char, 7);
        // And the box the click used is the box the renderer drew.
        assert!(rect.contains(x, y));
    }

    /// A second click on the selected cell copies it.
    #[test]
    fn clicking_the_selected_cell_a_second_time_copies_it() {
        let mut app = CharMapApp::new();
        let index = app
            .grid_chars
            .iter()
            .position(|&cp| cp == 0x0041)
            .expect("basic latin contains A");
        probe::click(&mut app, Target::Cell(index));
        assert_eq!(app.clipboard, None);
        probe::click(&mut app, Target::Cell(index));
        assert_eq!(app.clipboard, Some("A".to_string()));
    }

    /// A favourite tile copies on one click rather than selecting.
    ///
    /// Selecting it would mean finding it in the grid, which may be showing a
    /// different block entirely.
    #[test]
    fn clicking_a_favourite_tile_copies_it_without_changing_the_grid() {
        let mut app = CharMapApp::new();
        app.favorites.push(0x20AC);
        let before = app.selected_char;
        probe::click(&mut app, Target::FavoriteTile(0));
        assert_eq!(app.clipboard, Some("\u{20AC}".to_string()));
        assert_eq!(app.selected_char, before);
        assert!(app.recent.contains(&0x20AC));
    }

    /// The Copy button and Ctrl+C put the same character on the clipboard.
    #[test]
    fn the_copy_button_and_the_shortcut_agree() {
        let mut by_button = CharMapApp::new();
        let mut by_key = CharMapApp::new();
        probe::click(&mut by_button, Target::Copy);
        probe::key(&mut by_key, &probe::ctrl(Key::C));
        assert!(by_button.clipboard.is_some());
        assert_eq!(by_button.clipboard, by_key.clipboard);
    }

    /// A click on empty background changes nothing.
    #[test]
    fn clicking_nothing_does_nothing() {
        let mut app = CharMapApp::new();
        assert_eq!(probe::click_background(&mut app), EventResult::Ignored);
    }

    /// An app showing a block too big to fit on one screen.
    ///
    /// The default block is Basic Latin — 128 characters, which fit whole at
    /// every window size this test file uses. A scrolling test against it would
    /// pass by proving nothing.
    fn app_on_a_scrollable_block() -> CharMapApp {
        let mut app = CharMapApp::new();
        let big = app
            .blocks
            .iter()
            .enumerate()
            .max_by_key(|(_, block)| block.len())
            .map(|(index, _)| index)
            .expect("the block list is not empty");
        app.select_block(big);
        assert!(app.max_grid_scroll() > 0, "the block still fits on screen");
        app
    }

    /// The wheel scrolls whichever list the pointer is over, not the focused one.
    #[test]
    fn the_wheel_scrolls_the_list_under_the_pointer() {
        let mut app = app_on_a_scrollable_block();
        app.active_panel = Panel::Grid;
        let layout = app.layout();
        let list = layout
            .sidebar
            .as_ref()
            .expect("a sidebar at full size")
            .list;
        // The block chosen above is near the end of the list, so selecting it
        // has already scrolled the sidebar to its stop. Wind it back so the
        // wheel has somewhere to go.
        app.block_scroll = 0;
        let (x, y) = list.centre();
        let blocks_before = app.block_scroll;
        let grid_before = app.grid_scroll;
        // Aimed at the block list even though the grid has focus.
        assert!(app.handle_scroll(x, y, -3.0 * wheel::ROWS_PER_NOTCH));
        assert!(app.block_scroll > blocks_before);
        assert_eq!(app.grid_scroll, grid_before);

        let (gx, gy) = layout.grid.centre();
        let blocks_now = app.block_scroll;
        assert!(app.handle_scroll(gx, gy, -3.0 * wheel::ROWS_PER_NOTCH));
        assert!(app.grid_scroll > grid_before);
        assert_eq!(app.block_scroll, blocks_now);
    }

    /// A scroll cannot run off either end of either list.
    #[test]
    fn scrolling_stops_at_the_ends() {
        let mut app = app_on_a_scrollable_block();
        let layout = app.layout();
        let (gx, gy) = layout.grid.centre();
        for _ in 0..500 {
            app.handle_scroll(gx, gy, -30.0);
        }
        assert_eq!(app.grid_scroll, app.max_grid_scroll());
        for _ in 0..500 {
            app.handle_scroll(gx, gy, 30.0);
        }
        assert_eq!(app.grid_scroll, 0);
    }

    /// Arrow-Down moves by exactly the row length the grid is drawn with.
    ///
    /// This is the bug the rewrite exists to make impossible: the old app kept a
    /// `grid_columns: 16` while the renderer worked out its own column count
    /// from the window width, so the cursor and the eye followed different rows.
    #[test]
    fn a_row_step_matches_the_row_the_renderer_draws() {
        for width in [420.0_f32, 700.0, 980.0, 1600.0] {
            let mut app = CharMapApp::new();
            app.resize(width, 660.0);
            let columns = app.layout().columns.get();
            let first = probe::rect_of_sized(&app, Target::Cell(0), (width, 660.0));
            let below = probe::rect_of_sized(&app, Target::Cell(columns), (width, 660.0));
            if let (Some(first), Some(below)) = (first, below) {
                assert!(
                    below.y > first.y,
                    "cell {columns} should be on the next row at width {width}"
                );
                assert!((below.x - first.x).abs() < 0.5);
            }
            app.grid_down();
            assert_eq!(app.selected_char, columns, "at width {width}");
        }
    }

    /// Selecting a character far down the block scrolls it into view.
    #[test]
    fn the_end_key_scrolls_the_last_character_onto_the_screen() {
        let mut app = CharMapApp::new();
        app.select_block(
            app.blocks
                .iter()
                .position(|b| b.len() > 200)
                .expect("some block is bigger than one screen"),
        );
        probe::key(&mut app, &probe::press(Key::End));
        let last = app.grid_chars.len().saturating_sub(1);
        assert_eq!(app.selected_char, last);
        assert!(probe::rect_of(&app, Target::Cell(last)).is_some());
    }

    /// A narrow window drops panels rather than squeezing them to unreadable
    /// slivers, and the dropped panels stop being clickable with them.
    #[test]
    fn a_narrow_window_drops_panels_instead_of_squeezing_them() {
        let mut app = CharMapApp::new();
        app.resize(300.0, 660.0);
        let layout = app.layout();
        assert!(layout.sidebar.is_none());
        assert!(layout.detail.is_none());
        assert!(probe::rect_of_sized(&app, Target::Block(0), (300.0, 660.0)).is_none());
        assert!(probe::rect_of_sized(&app, Target::Copy, (300.0, 660.0)).is_none());
        // The grid survives, because it is the program.
        assert!(probe::rect_of_sized(&app, Target::Cell(0), (300.0, 660.0)).is_some());
        assert_frame_ok(&app.draw((300.0, 660.0)));
    }

    /// No window size makes the renderer produce a malformed frame.
    #[test]
    fn every_window_size_draws_a_balanced_frame() {
        for (w, h) in [
            (0.0_f32, 0.0_f32),
            (1.0, 1.0),
            (120.0, 90.0),
            (300.0, 200.0),
            (980.0, 660.0),
            (2400.0, 1400.0),
            (f32::NAN, f32::NAN),
            (-50.0, -50.0),
        ] {
            let mut app = CharMapApp::new();
            app.resize(w, h);
            let frame = app.draw((w, h));
            assert!(frame.is_balanced(), "unbalanced at {w}x{h}");
            // A click anywhere must not panic, whatever the size.
            let _ = app.handle_click(w / 2.0, h / 2.0);
        }
    }

    /// A resize does not leave the grid scrolled past its own end.
    #[test]
    fn growing_the_window_pulls_the_scroll_back_into_range() {
        let mut app = app_on_a_scrollable_block();
        app.resize(980.0, 300.0);
        let (gx, gy) = app.layout().grid.centre();
        for _ in 0..200 {
            app.handle_scroll(gx, gy, -30.0);
        }
        assert!(app.grid_scroll > 0);
        app.resize(980.0, 2000.0);
        assert!(app.grid_scroll <= app.max_grid_scroll());
    }

    /// Opening a search resets the scroll, because it swaps the list underneath.
    #[test]
    fn opening_a_search_resets_a_scroll_that_indexed_the_other_list() {
        let mut app = app_on_a_scrollable_block();
        let (gx, gy) = app.layout().grid.centre();
        app.handle_scroll(gx, gy, -30.0);
        assert!(app.grid_scroll > 0);
        probe::key(&mut app, &probe::ctrl(Key::F));
        assert_eq!(app.grid_scroll, 0);
    }

    /// Every control the renderer draws is reachable by a click.
    #[test]
    fn every_drawn_control_is_clickable() {
        let mut app = CharMapApp::new();
        app.add_to_recent(0x0041);
        app.favorites.push(0x0042);
        let names = probe::control_names(&app);
        assert!(names.iter().any(|n| n.contains("Copy")), "{names:?}");
        assert!(names.iter().any(|n| n.contains("Star")), "{names:?}");
        assert!(names.iter().any(|n| n.contains("Filter")), "{names:?}");
        assert!(names.iter().any(|n| n.contains("SearchBox")), "{names:?}");
        for target in [
            Target::Copy,
            Target::Star,
            Target::Filter,
            Target::SearchBox,
        ] {
            assert_eq!(
                probe::click(&mut app, target),
                EventResult::Consumed,
                "{target:?} drew a box that does nothing when clicked"
            );
        }
    }

    // ── General Category label tests ───────────────────────────────────

    #[test]
    fn test_category_labels() {
        assert_eq!(GeneralCategory::UppercaseLetter.short_label(), "Lu");
        assert_eq!(GeneralCategory::MathSymbol.short_label(), "Sm");
        assert_eq!(GeneralCategory::DecimalNumber.short_label(), "Nd");
    }

    #[test]
    fn test_category_is_methods() {
        assert!(GeneralCategory::UppercaseLetter.is_letter());
        assert!(!GeneralCategory::UppercaseLetter.is_number());
        assert!(GeneralCategory::DecimalNumber.is_number());
        assert!(GeneralCategory::MathSymbol.is_symbol());
        assert!(GeneralCategory::DashPunctuation.is_punctuation());
    }

    // ── Edge case tests ────────────────────────────────────────────────

    #[test]
    fn test_empty_block_select() {
        let mut app = CharMapApp::new();
        app.select_block(9999); // out of bounds
        // Should remain unchanged
        assert_eq!(app.selected_block, 0);
    }

    #[test]
    fn test_grid_navigation_empty() {
        let mut app = CharMapApp::new();
        app.grid_chars.clear();
        // Should not panic
        app.grid_right();
        app.grid_left();
        app.grid_up();
        app.grid_down();
        assert_eq!(app.selected_char, 0);
    }

    #[test]
    fn test_copy_empty_grid() {
        let mut app = CharMapApp::new();
        app.grid_chars.clear();
        app.copy_selected(); // should not panic
        assert!(app.clipboard.is_none());
    }

    #[test]
    fn test_toggle_favorite_empty_grid() {
        let mut app = CharMapApp::new();
        app.grid_chars.clear();
        app.toggle_favorite(); // should not panic
        assert!(app.favorites.is_empty());
    }

    #[test]
    fn test_search_navigation_in_results() {
        let mut app = CharMapApp::new();
        app.search_active = true;
        app.search_query = "DOLLAR".into();
        app.perform_search();
        if !app.search_results.is_empty() {
            app.grid_right();
            // If only one result, stays at 0 (clamped)
        }
    }

    #[test]
    fn test_utf8_hex_display() {
        let info = char_info(0x0041);
        assert_eq!(info.utf8_hex(), "41");

        let info2 = char_info(0x00E9);
        assert_eq!(info2.utf8_hex(), "C3 A9");
    }
}

#![allow(dead_code)]
//! Character Map — Unicode character browser and picker for OurOS.
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

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const SKY: Color = Color::from_hex(0x89DCEB);

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
        0x0030..=0x0039 | 0x0660..=0x0669 | 0x06F0..=0x06F9 | 0x0966..=0x096F
        | 0x0E50..=0x0E59 => GeneralCategory::DecimalNumber,
        0x2160..=0x2182 | 0x3007 | 0x3021..=0x3029 => GeneralCategory::LetterNumber,
        0x00B2 | 0x00B3 | 0x00B9 | 0x00BC..=0x00BE | 0x2070..=0x2079 | 0x2080..=0x2089
        | 0x2150..=0x215F | 0x2460..=0x2473 | 0x2474..=0x2487 | 0x2488..=0x249B => {
            GeneralCategory::OtherNumber
        }
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
        0x0021 | 0x0022 | 0x0023 | 0x0025 | 0x0026 | 0x0027 | 0x002A | 0x002C | 0x002E
        | 0x002F | 0x003A | 0x003B | 0x003F | 0x0040 | 0x005C | 0x00A1 | 0x00A7 | 0x00B6
        | 0x00BF => GeneralCategory::OtherPunctuation,
        0x005F => GeneralCategory::ConnectorPunctuation,
        0x002D | 0x2010..=0x2015 | 0x2E17 | 0x2E1A | 0xFE58 | 0xFE63 | 0xFF0D => {
            GeneralCategory::DashPunctuation
        }
        0x0028 | 0x005B | 0x007B | 0x2045 | 0x207D | 0x208D | 0x2308 | 0x230A | 0x2329
        | 0x27E6..=0x27EF | 0x2983..=0x2998 | 0xFF08 | 0xFF3B | 0xFF5B => {
            GeneralCategory::OpenPunctuation
        }
        0x0029 | 0x005D | 0x007D | 0x2046 | 0x207E | 0x208E | 0x2309 | 0x230B | 0x232A
        | 0xFF09 | 0xFF3D | 0xFF5D => GeneralCategory::ClosePunctuation,
        0x00AB | 0x2018 | 0x201B | 0x201C | 0x201F | 0x2039 => {
            GeneralCategory::InitialPunctuation
        }
        0x00BB | 0x2019 | 0x201D | 0x203A => GeneralCategory::FinalPunctuation,
        0x002B | 0x003C..=0x003E | 0x007C | 0x007E | 0x00AC | 0x00B1 | 0x00D7 | 0x00F7
        | 0x2200..=0x22FF | 0x27C0..=0x27EF | 0x2980..=0x29FF | 0x2A00..=0x2AFF => {
            GeneralCategory::MathSymbol
        }
        0x0024 | 0x00A2..=0x00A5 | 0x058F | 0x060B | 0x09F2..=0x09F3 | 0x0AF1 | 0x0BF9
        | 0x20A0..=0x20CF | 0xFE69 | 0xFF04 | 0xFFE0..=0xFFE1 | 0xFFE5..=0xFFE6 => {
            GeneralCategory::CurrencySymbol
        }
        0x005E | 0x0060 | 0x00A8 | 0x00AF | 0x00B4 | 0x00B8 => {
            GeneralCategory::ModifierSymbol
        }
        0x00A6 | 0x00A9 | 0x00AE | 0x00B0 | 0x2100..=0x214F | 0x2190..=0x21FF
        | 0x2300..=0x23FF | 0x2400..=0x243F | 0x2440..=0x245F | 0x2500..=0x257F
        | 0x2580..=0x259F | 0x25A0..=0x25FF | 0x2600..=0x26FF | 0x2700..=0x27BF
        | 0x2800..=0x28FF | 0xFFFD => GeneralCategory::OtherSymbol,
        0x2028 => GeneralCategory::LineSeparator,
        0x2029 => GeneralCategory::ParagraphSeparator,
        0xD800..=0xDFFF => GeneralCategory::Surrogate,
        0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD => GeneralCategory::PrivateUse,
        // Emoticons and pictographs
        0x1F300..=0x1F3FF | 0x1F400..=0x1F4FF | 0x1F500..=0x1F5FF | 0x1F600..=0x1F64F
        | 0x1F680..=0x1F6FF | 0x1F900..=0x1F9FF => GeneralCategory::OtherSymbol,
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
                && let Some(name) = digit_names.get(idx as usize) {
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
    grid_columns: usize,
    selected_char: usize,
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
}

impl CharMapApp {
    fn new() -> Self {
        let blocks = unicode_blocks();
        let mut app = Self {
            blocks,
            selected_block: 0,
            block_scroll: 0,
            grid_chars: Vec::new(),
            grid_columns: 16,
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
            width: 1024.0,
            height: 768.0,
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

    /// Get info for the currently selected character.
    fn selected_char_info(&self) -> Option<CharInfo> {
        if self.search_active {
            self.search_results
                .get(self.search_selected)
                .map(|&cp| char_info(cp))
        } else {
            self.grid_chars
                .get(self.selected_char)
                .map(|&cp| char_info(cp))
        }
    }

    /// Copy the selected character to clipboard.
    fn copy_selected(&mut self) {
        let cp = if self.search_active {
            self.search_results.get(self.search_selected).copied()
        } else {
            self.grid_chars.get(self.selected_char).copied()
        };

        if let Some(cp) = cp
            && let Some(ch) = char::from_u32(cp) {
                let s = ch.to_string();
                self.clipboard = Some(s.clone());
                self.add_to_recent(cp);
                self.status_message = format!("Copied '{}' (U+{:04X}) to clipboard", s, cp);
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
        let cp = if self.search_active {
            self.search_results.get(self.search_selected).copied()
        } else {
            self.grid_chars.get(self.selected_char).copied()
        };

        if let Some(cp) = cp {
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
        if let Some(hex_str) = query.strip_prefix("u+").or_else(|| query.strip_prefix("0x")) {
            if let Ok(cp) = u32::from_str_radix(hex_str, 16)
                && (char::from_u32(cp).is_some() || cp <= 0x10FFFF) {
                    self.search_results.push(cp);
                }
            return;
        }

        // Search by literal single character
        let chars: Vec<char> = query.chars().collect();
        if chars.len() == 1
            && let Some(&ch) = chars.first() {
                self.search_results.push(ch as u32);
            }

        // Search by name across all blocks
        let blocks = unicode_blocks();
        for block in &blocks {
            let mut cp = block.start;
            while cp <= block.end {
                let name = codepoint_name(cp).to_lowercase();
                if name.contains(&query)
                    && !self.search_results.contains(&cp) {
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
            self.status_message =
                format!("Found {} results for '{}'", self.search_results.len(), self.search_query);
        }
    }

    /// Navigate block list.
    fn select_block(&mut self, idx: usize) {
        if idx < self.blocks.len() {
            self.selected_block = idx;
            self.populate_grid();
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

    /// Navigate grid.
    fn grid_right(&mut self) {
        if self.search_active {
            let next = self.search_selected.saturating_add(1);
            if next < self.search_results.len() {
                self.search_selected = next;
            }
        } else {
            let next = self.selected_char.saturating_add(1);
            if next < self.grid_chars.len() {
                self.selected_char = next;
            }
        }
    }

    fn grid_left(&mut self) {
        if self.search_active {
            self.search_selected = self.search_selected.saturating_sub(1);
        } else {
            self.selected_char = self.selected_char.saturating_sub(1);
        }
    }

    fn grid_down(&mut self) {
        let cols = self.grid_columns;
        if self.search_active {
            let next = self.search_selected.saturating_add(cols);
            if next < self.search_results.len() {
                self.search_selected = next;
            }
        } else {
            let next = self.selected_char.saturating_add(cols);
            if next < self.grid_chars.len() {
                self.selected_char = next;
            }
        }
    }

    fn grid_up(&mut self) {
        let cols = self.grid_columns;
        if self.search_active {
            self.search_selected = self.search_selected.saturating_sub(cols);
        } else {
            self.selected_char = self.selected_char.saturating_sub(cols);
        }
    }

    /// Cycle category filter.
    fn next_category_filter(&mut self) {
        let idx = CategoryFilter::ALL_FILTERS
            .iter()
            .position(|&f| f == self.category_filter)
            .unwrap_or(0);
        let next_idx = (idx.wrapping_add(1)) % CategoryFilter::ALL_FILTERS.len();
        self.category_filter = CategoryFilter::ALL_FILTERS
            .get(next_idx)
            .copied()
            .unwrap_or(CategoryFilter::All);
        self.populate_grid();
        self.status_message = format!("Filter: {}", self.category_filter.label());
    }

    /// Handle keyboard input.
    fn handle_key(&mut self, key: &str, ctrl: bool, _shift: bool) {
        match key {
            "Tab" if !ctrl => {
                self.active_panel = match self.active_panel {
                    Panel::Blocks => Panel::Grid,
                    Panel::Grid => Panel::Detail,
                    Panel::Detail => Panel::Search,
                    Panel::Search => Panel::Recent,
                    Panel::Recent => Panel::Favorites,
                    Panel::Favorites => Panel::Blocks,
                };
            }
            "Escape"
                if self.search_active => {
                    self.search_active = false;
                    self.status_message = "Search closed".into();
                }
            // Ctrl+F to search
            "f" if ctrl => {
                self.search_active = true;
                self.active_panel = Panel::Search;
                self.status_message = "Type search query...".into();
            }
            // Ctrl+C to copy
            "c" if ctrl => {
                self.copy_selected();
            }
            // Enter to copy
            "Return" | "Enter" => {
                self.copy_selected();
            }
            // Space to toggle favorite
            " " => {
                self.toggle_favorite();
            }
            // Arrow keys
            "Left" => self.grid_left(),
            "Right" => self.grid_right(),
            "Up" => {
                if self.active_panel == Panel::Blocks {
                    self.prev_block();
                } else {
                    self.grid_up();
                }
            }
            "Down" => {
                if self.active_panel == Panel::Blocks {
                    self.next_block();
                } else {
                    self.grid_down();
                }
            }
            // Page navigation
            "PageUp" | "Prior" => {
                if self.active_panel == Panel::Blocks {
                    let target = self.selected_block.saturating_sub(10);
                    self.select_block(target);
                } else {
                    let rows: usize = 5;
                    let cols = self.grid_columns;
                    let amount = rows.saturating_mul(cols);
                    self.selected_char = self.selected_char.saturating_sub(amount);
                }
            }
            "PageDown" | "Next" => {
                if self.active_panel == Panel::Blocks {
                    let target = self
                        .selected_block
                        .saturating_add(10)
                        .min(self.blocks.len().saturating_sub(1));
                    self.select_block(target);
                } else {
                    let rows: usize = 5;
                    let cols = self.grid_columns;
                    let amount = rows.saturating_mul(cols);
                    let next = self.selected_char.saturating_add(amount);
                    if next < self.grid_chars.len() {
                        self.selected_char = next;
                    }
                }
            }
            // Category filter
            "F2" => {
                self.next_category_filter();
            }
            // Preview size cycle
            "F3" => {
                self.preview_size = self.preview_size.next();
                self.status_message = format!("Preview: {}", self.preview_size.label());
            }
            // Search mode type chars
            _ if self.search_active && key.len() == 1 && !ctrl => {
                self.search_query.push_str(key);
                self.perform_search();
            }
            "BackSpace" if self.search_active => {
                self.search_query.pop();
                self.perform_search();
            }
            _ => {}
        }
    }

    // ── Rendering ──────────────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Layout:
        // [Sidebar: Blocks (200px)] [Grid (main)] [Detail (260px)]
        // [Status bar (bottom 28px)]

        let sidebar_w: f32 = 200.0;
        let detail_w: f32 = 260.0;
        let status_h: f32 = 28.0;
        let main_x = sidebar_w;
        let main_w = (self.width - sidebar_w - detail_w).max(200.0);
        let main_h = self.height - status_h;

        // ── Sidebar: Block List ────────────────────────────────────────────
        self.render_block_sidebar(&mut cmds, 0.0, 0.0, sidebar_w, main_h);

        // ── Main: Character Grid ───────────────────────────────────────────
        self.render_grid(&mut cmds, main_x, 0.0, main_w, main_h);

        // ── Detail Panel ───────────────────────────────────────────────────
        let detail_x = main_x + main_w;
        self.render_detail(&mut cmds, detail_x, 0.0, detail_w, main_h);

        // ── Status Bar ─────────────────────────────────────────────────────
        self.render_status_bar(&mut cmds, 0.0, main_h, self.width, status_h);

        cmds
    }

    fn render_block_sidebar(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 6.0,
            text: "Unicode Blocks".into(),
            font_size: 13.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 16.0),
        });

        // Filter indicator
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 22.0,
            text: format!("Filter: {} [F2]", self.category_filter.label()),
            font_size: 10.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 16.0),
        });

        let item_h: f32 = 22.0;
        let start_y = y + 40.0;
        let visible = ((h - 40.0) / item_h) as usize;

        for (vi, idx) in (self.block_scroll..).enumerate() {
            if vi >= visible {
                break;
            }
            if let Some(block) = self.blocks.get(idx) {
                let item_y = start_y + (vi as f32) * item_h;
                let is_selected = idx == self.selected_block;

                if is_selected {
                    cmds.push(RenderCommand::FillRect {
                        x: x + 2.0,
                        y: item_y,
                        width: w - 4.0,
                        height: item_h,
                        color: SURFACE0,
                        corner_radii: CornerRadii::all(4.0),
                    });
                }

                let text_color = if is_selected { TEXT_COLOR } else { SUBTEXT1 };
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: item_y + 4.0,
                    text: block.name.to_string(),
                    font_size: 11.0,
                    color: text_color,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 20.0),
                });

                // Show range
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: item_y + 14.0,
                    text: format!("{:04X}–{:04X} ({})", block.start, block.end, block.len()),
                    font_size: 8.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 20.0),
                });
            }
        }

        // Separator line
        cmds.push(RenderCommand::FillRect {
            x: x + w - 1.0,
            y,
            width: 1.0,
            height: h,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        let chars = if self.search_active {
            &self.search_results
        } else {
            &self.grid_chars
        };
        let selected = if self.search_active {
            self.search_selected
        } else {
            self.selected_char
        };

        // Title bar for grid
        let title_h: f32 = 30.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: title_h,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let title = if self.search_active {
            format!(
                "Search: '{}' ({} results)",
                self.search_query,
                chars.len()
            )
        } else {
            let block_name = self
                .blocks
                .get(self.selected_block)
                .map(|b| b.name)
                .unwrap_or("???");
            format!("{} ({} chars)", block_name, chars.len())
        };

        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 8.0,
            text: title,
            font_size: 12.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 16.0),
        });

        // Character grid
        let grid_y = y + title_h;
        let grid_h = h - title_h;
        let cell_size: f32 = 36.0;
        let cols = ((w - 8.0) / cell_size).max(1.0) as usize;
        let visible_rows = ((grid_h - 4.0) / cell_size) as usize;

        // Ensure scroll is valid
        let row_of_selected = selected.checked_div(cols).unwrap_or(0);
        let scroll_row = if row_of_selected < self.grid_scroll {
            row_of_selected
        } else if row_of_selected >= self.grid_scroll.saturating_add(visible_rows) {
            row_of_selected
                .saturating_sub(visible_rows)
                .saturating_add(1)
        } else {
            self.grid_scroll
        };

        for vi_row in 0..visible_rows {
            let data_row = scroll_row.saturating_add(vi_row);
            for col in 0..cols {
                let idx = data_row.saturating_mul(cols).saturating_add(col);
                if idx >= chars.len() {
                    break;
                }
                let cp = match chars.get(idx) {
                    Some(&c) => c,
                    None => break,
                };
                let cx = x + 4.0 + (col as f32) * cell_size;
                let cy = grid_y + 4.0 + (vi_row as f32) * cell_size;

                let is_sel = idx == selected;
                let is_fav = self.favorites.contains(&cp);

                // Cell background
                let bg = if is_sel {
                    BLUE
                } else if is_fav {
                    SURFACE1
                } else {
                    SURFACE0
                };
                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: cell_size - 2.0,
                    height: cell_size - 2.0,
                    color: bg,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Favorite indicator
                if is_fav {
                    cmds.push(RenderCommand::Text {
                        x: cx + cell_size - 10.0,
                        y: cy + 1.0,
                        text: "*".into(),
                        font_size: 8.0,
                        color: YELLOW,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }

                // Character display
                let display = if let Some(ch) = char::from_u32(cp) {
                    if ch.is_control() {
                        format!("{:02X}", cp)
                    } else {
                        ch.to_string()
                    }
                } else {
                    format!("{:02X}", cp)
                };

                let text_color = if is_sel { CRUST } else { TEXT_COLOR };
                cmds.push(RenderCommand::Text {
                    x: cx + 4.0,
                    y: cy + 6.0,
                    text: display,
                    font_size: 16.0,
                    color: text_color,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(cell_size - 8.0),
                });

                // Codepoint label below
                cmds.push(RenderCommand::Text {
                    x: cx + 2.0,
                    y: cy + cell_size - 12.0,
                    text: format!("{:04X}", cp),
                    font_size: 7.0,
                    color: if is_sel { MANTLE } else { OVERLAY0 },
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(cell_size - 4.0),
                });
            }
        }
    }

    fn render_detail(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        // Separator
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 1.0,
            height: h,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Background
        cmds.push(RenderCommand::FillRect {
            x: x + 1.0,
            y,
            width: w - 1.0,
            height: h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: y + 8.0,
            text: "Character Detail".into(),
            font_size: 13.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 20.0),
        });

        let info = match self.selected_char_info() {
            Some(i) => i,
            None => {
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: y + 40.0,
                    text: "No character selected".into(),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 20.0),
                });
                return;
            }
        };

        // Large preview
        let preview_y = y + 30.0;
        cmds.push(RenderCommand::FillRect {
            x: x + 10.0,
            y: preview_y,
            width: w - 20.0,
            height: 80.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 20.0,
            y: preview_y + 10.0,
            text: info.display_char(),
            font_size: self.preview_size.font_size(),
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 40.0),
        });

        // Preview size label
        cmds.push(RenderCommand::Text {
            x: x + w - 60.0,
            y: preview_y + 64.0,
            text: format!("[F3] {}", self.preview_size.label()),
            font_size: 8.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Detail fields
        let mut detail_y = preview_y + 90.0;
        let line_h: f32 = 16.0;

        let fields: Vec<(&str, String)> = vec![
            ("Codepoint", info.codepoint_str()),
            ("Name", info.name.clone()),
            ("Block", info.block_name.clone()),
            ("Category", info.category.label().to_string()),
            ("UTF-8", info.utf8_hex()),
            ("HTML Dec", info.html_entity()),
            ("HTML Hex", info.html_hex_entity()),
            ("CSS", info.css_escape()),
            ("Rust", info.rust_escape()),
        ];

        for (label, value) in &fields {
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: detail_y,
                text: format!("{label}:"),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(70.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 80.0,
                y: detail_y,
                text: value.clone(),
                font_size: 10.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 90.0),
            });
            detail_y += line_h;
        }

        // Favorite status
        detail_y += 8.0;
        let is_fav = self
            .selected_char_info()
            .map(|i| self.favorites.contains(&i.codepoint))
            .unwrap_or(false);
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: detail_y,
            text: if is_fav {
                "* Favorite [Space to remove]".into()
            } else {
                "[Space] Add to favorites".into()
            },
            font_size: 10.0,
            color: if is_fav { YELLOW } else { OVERLAY0 },
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 20.0),
        });
        detail_y += line_h;

        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: detail_y,
            text: "[Enter/Ctrl+C] Copy to clipboard".into(),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 20.0),
        });

        // Recent section
        detail_y += 24.0;
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: detail_y,
            text: format!("Recently Used ({})", self.recent.len()),
            font_size: 11.0,
            color: TEAL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 20.0),
        });
        detail_y += 16.0;

        let recent_cell: f32 = 24.0;
        let recent_cols = ((w - 20.0) / recent_cell) as usize;
        let max_recent_show: usize = recent_cols.saturating_mul(3);
        let mut ri: usize = 0;
        for &cp in self.recent.iter().take(max_recent_show) {
            let col = ri % recent_cols;
            let row = ri / recent_cols;
            let rx = x + 10.0 + (col as f32) * recent_cell;
            let ry = detail_y + (row as f32) * recent_cell;

            if let Some(ch) = char::from_u32(cp)
                && !ch.is_control() {
                    cmds.push(RenderCommand::FillRect {
                        x: rx,
                        y: ry,
                        width: recent_cell - 2.0,
                        height: recent_cell - 2.0,
                        color: SURFACE0,
                        corner_radii: CornerRadii::all(3.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: rx + 3.0,
                        y: ry + 3.0,
                        text: ch.to_string(),
                        font_size: 12.0,
                        color: TEXT_COLOR,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(recent_cell - 6.0),
                    });
                }
            ri = ri.saturating_add(1);
        }

        // Favorites section
        let fav_y = detail_y + (((max_recent_show / recent_cols.max(1)) as f32) + 1.0) * recent_cell + 8.0;
        if fav_y + 20.0 < y + h {
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: fav_y,
                text: format!("Favorites ({})", self.favorites.len()),
                font_size: 11.0,
                color: YELLOW,
                font_weight: FontWeightHint::Bold,
                max_width: Some(w - 20.0),
            });

            let mut fi: usize = 0;
            for &cp in self.favorites.iter().take(max_recent_show) {
                let col = fi % recent_cols;
                let row = fi / recent_cols;
                let fx = x + 10.0 + (col as f32) * recent_cell;
                let fy = fav_y + 16.0 + (row as f32) * recent_cell;

                if fy + recent_cell > y + h {
                    break;
                }

                if let Some(ch) = char::from_u32(cp)
                    && !ch.is_control() {
                        cmds.push(RenderCommand::FillRect {
                            x: fx,
                            y: fy,
                            width: recent_cell - 2.0,
                            height: recent_cell - 2.0,
                            color: SURFACE1,
                            corner_radii: CornerRadii::all(3.0),
                        });
                        cmds.push(RenderCommand::Text {
                            x: fx + 3.0,
                            y: fy + 3.0,
                            text: ch.to_string(),
                            font_size: 12.0,
                            color: TEXT_COLOR,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(recent_cell - 6.0),
                        });
                    }
                fi = fi.saturating_add(1);
            }
        }
    }

    fn render_status_bar(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Status message
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 8.0,
            text: self.status_message.clone(),
            font_size: 11.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w * 0.6),
        });

        // Active panel indicator
        let panel_label = match self.active_panel {
            Panel::Blocks => "Blocks",
            Panel::Grid => "Grid",
            Panel::Detail => "Detail",
            Panel::Search => "Search",
            Panel::Recent => "Recent",
            Panel::Favorites => "Favorites",
        };
        cmds.push(RenderCommand::Text {
            x: x + w - 200.0,
            y: y + 8.0,
            text: format!("Panel: {panel_label}  |  [Ctrl+F] Search  [Tab] Switch"),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });
    }
}

// ── Entry point ────────────────────────────────────────────────────────────

fn main() {
    let _app = CharMapApp::new();
    // In the real OS, this would enter the GUI event loop.
    // For now, the app is structurally complete and tested below.
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(block.start <= block.end, "Block '{}' has start > end", block.name);
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
        assert_eq!(classify_codepoint(0x0021), GeneralCategory::OtherPunctuation); // '!'
        assert_eq!(classify_codepoint(0x003F), GeneralCategory::OtherPunctuation); // '?'
    }

    #[test]
    fn test_classify_open_close_punct() {
        assert_eq!(classify_codepoint(0x0028), GeneralCategory::OpenPunctuation); // '('
        assert_eq!(classify_codepoint(0x0029), GeneralCategory::ClosePunctuation); // ')'
        assert_eq!(classify_codepoint(0x005B), GeneralCategory::OpenPunctuation); // '['
        assert_eq!(classify_codepoint(0x005D), GeneralCategory::ClosePunctuation); // ']'
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
        assert!(info.display_char().len() > 0);
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
        app.grid_columns = 16;
        app.grid_down();
        assert_eq!(app.selected_char, 16);
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
        app.search_query = "".into();
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
        app.handle_key("Tab", false, false);
        assert_eq!(app.active_panel, Panel::Detail);
        app.handle_key("Tab", false, false);
        assert_eq!(app.active_panel, Panel::Search);
    }

    #[test]
    fn test_key_ctrl_f_activates_search() {
        let mut app = CharMapApp::new();
        assert!(!app.search_active);
        app.handle_key("f", true, false);
        assert!(app.search_active);
        assert_eq!(app.active_panel, Panel::Search);
    }

    #[test]
    fn test_key_escape_closes_search() {
        let mut app = CharMapApp::new();
        app.search_active = true;
        app.handle_key("Escape", false, false);
        assert!(!app.search_active);
    }

    #[test]
    fn test_key_enter_copies() {
        let mut app = CharMapApp::new();
        if let Some(pos) = app.grid_chars.iter().position(|&cp| cp == 0x0041) {
            app.selected_char = pos;
        }
        app.handle_key("Return", false, false);
        assert_eq!(app.clipboard, Some("A".to_string()));
    }

    #[test]
    fn test_key_space_toggles_favorite() {
        let mut app = CharMapApp::new();
        if let Some(pos) = app.grid_chars.iter().position(|&cp| cp == 0x0041) {
            app.selected_char = pos;
        }
        app.handle_key(" ", false, false);
        assert!(app.favorites.contains(&0x0041));
    }

    #[test]
    fn test_key_f2_cycles_filter() {
        let mut app = CharMapApp::new();
        let before = app.category_filter;
        app.handle_key("F2", false, false);
        assert_ne!(app.category_filter, before);
    }

    #[test]
    fn test_key_f3_cycles_preview() {
        let mut app = CharMapApp::new();
        assert_eq!(app.preview_size, PreviewSize::Medium);
        app.handle_key("F3", false, false);
        assert_eq!(app.preview_size, PreviewSize::Large);
    }

    #[test]
    fn test_search_typing() {
        let mut app = CharMapApp::new();
        app.search_active = true;
        app.handle_key("D", false, false);
        app.handle_key("O", false, false);
        app.handle_key("L", false, false);
        assert_eq!(app.search_query, "DOL");
    }

    #[test]
    fn test_search_backspace() {
        let mut app = CharMapApp::new();
        app.search_active = true;
        app.search_query = "DOL".into();
        app.handle_key("BackSpace", false, false);
        assert_eq!(app.search_query, "DO");
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

    #[test]
    fn test_render_produces_commands() {
        let app = CharMapApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_contains_background() {
        let app = CharMapApp::new();
        let cmds = app.render();
        let has_bg = cmds.iter().any(|cmd| matches!(cmd, RenderCommand::FillRect { x, y, .. } if *x == 0.0 && *y == 0.0));
        assert!(has_bg);
    }

    #[test]
    fn test_render_with_selection() {
        let mut app = CharMapApp::new();
        app.selected_char = 5;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_search_active() {
        let mut app = CharMapApp::new();
        app.search_active = true;
        app.search_query = "DOLLAR".into();
        app.perform_search();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_favorites() {
        let mut app = CharMapApp::new();
        app.favorites.push(0x0041);
        app.favorites.push(0x0042);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_recent() {
        let mut app = CharMapApp::new();
        app.add_to_recent(0x0041);
        app.add_to_recent(0x0042);
        let cmds = app.render();
        assert!(!cmds.is_empty());
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

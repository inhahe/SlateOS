//! Slate OS Emoji Picker
//!
//! A taskbar popup that provides a searchable, categorized grid of emoji.
//! The picker presents emoji organized by category with:
//! - Category tab bar with icons
//! - Live search filtering by name and keywords
//! - 6-column scrollable grid with hover preview
//! - Skin tone modifier selector (Fitzpatrick scale)
//! - Recently-used emoji tracking (up to 32 entries)
//!
//! Renders via guitk into a 360x480 popup window. Uses the Catppuccin Mocha
//! dark theme for all colors.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::wheel;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

/// Catppuccin Mocha dark theme colors.
#[allow(dead_code)]
mod mocha {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const PINK: Color = Color::from_hex(0xF5C2E7);
    pub const FLAMINGO: Color = Color::from_hex(0xF2CDCD);
    pub const SKY: Color = Color::from_hex(0x89DCFE);
    pub const ROSEWATER: Color = Color::from_hex(0xF5E0DC);
}

// ============================================================================
// Constants
// ============================================================================

/// Popup window width in pixels.
const WINDOW_WIDTH: f32 = 360.0;
/// Popup window height in pixels.
const WINDOW_HEIGHT: f32 = 480.0;
/// Number of emoji columns in the grid.
const GRID_COLUMNS: usize = 6;
/// Size of each emoji cell in the grid (square).
const CELL_SIZE: f32 = 48.0;
/// Padding inside each emoji cell.
const CELL_PADDING: f32 = 4.0;
/// Height of the category tab bar.
const TAB_BAR_HEIGHT: f32 = 40.0;
/// Height of the search field area.
const SEARCH_HEIGHT: f32 = 36.0;
/// Height of the preview area at the bottom.
const PREVIEW_HEIGHT: f32 = 56.0;
/// Height of the skin tone selector strip.
const SKIN_TONE_HEIGHT: f32 = 28.0;
/// Maximum number of recently used emoji to track.
const MAX_RECENT: usize = 32;
/// Diameter of each skin-tone indicator circle.
const SKIN_TONE_CIRCLE: f32 = 18.0;
/// Spacing between skin-tone circles.
const SKIN_TONE_SPACING: f32 = 6.0;
/// Border radius for rounded UI elements.
const CORNER_RADIUS: f32 = 8.0;
/// Font size for emoji glyphs in the grid.
const EMOJI_FONT_SIZE: f32 = 24.0;
/// Font size for the enlarged preview emoji.
const PREVIEW_EMOJI_SIZE: f32 = 32.0;
/// Font size for labels and search text.
const LABEL_FONT_SIZE: f32 = 13.0;
/// Font size for tab icons (emoji used as category icons).
const TAB_ICON_SIZE: f32 = 16.0;
/// Inner padding of the grid area.
const GRID_PADDING: f32 = 8.0;

// ============================================================================
// Emoji category
// ============================================================================

/// Emoji category, matching Unicode CLDR groupings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EmojiCategory {
    SmileysAndPeople,
    AnimalsAndNature,
    FoodAndDrink,
    TravelAndPlaces,
    Activities,
    Objects,
    Symbols,
    Flags,
}

impl EmojiCategory {
    /// All categories in display order.
    pub const ALL: &[EmojiCategory] = &[
        EmojiCategory::SmileysAndPeople,
        EmojiCategory::AnimalsAndNature,
        EmojiCategory::FoodAndDrink,
        EmojiCategory::TravelAndPlaces,
        EmojiCategory::Activities,
        EmojiCategory::Objects,
        EmojiCategory::Symbols,
        EmojiCategory::Flags,
    ];

    /// A representative emoji icon for the category tab.
    pub fn icon(self) -> &'static str {
        match self {
            Self::SmileysAndPeople => "\u{1F600}", // grinning face
            Self::AnimalsAndNature => "\u{1F43E}", // paw prints
            Self::FoodAndDrink => "\u{1F354}",     // hamburger
            Self::TravelAndPlaces => "\u{2708}",   // airplane
            Self::Activities => "\u{26BD}",        // soccer ball
            Self::Objects => "\u{1F4A1}",          // light bulb
            Self::Symbols => "\u{2764}",           // red heart
            Self::Flags => "\u{1F3F4}",            // black flag
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::SmileysAndPeople => "Smileys & People",
            Self::AnimalsAndNature => "Animals & Nature",
            Self::FoodAndDrink => "Food & Drink",
            Self::TravelAndPlaces => "Travel & Places",
            Self::Activities => "Activities",
            Self::Objects => "Objects",
            Self::Symbols => "Symbols",
            Self::Flags => "Flags",
        }
    }
}

// ============================================================================
// Skin tone modifier
// ============================================================================

/// Fitzpatrick skin-tone scale modifiers for emoji.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SkinToneModifier {
    /// No modifier (default yellow).
    #[default]
    None,
    /// Type I-II (light).
    Light,
    /// Type III (medium-light).
    MediumLight,
    /// Type IV (medium).
    Medium,
    /// Type V (medium-dark).
    MediumDark,
    /// Type VI (dark).
    Dark,
}

/// Codepoint ranges carrying the Unicode `Emoji_Modifier_Base` property,
/// sorted and non-overlapping so membership is a binary search.
///
/// This is the property that decides whether an emoji *has skin*. A raised hand
/// or a dancer does; a slice of pizza, a flag, and -- the surprising one -- a
/// smiley face do not. Appending a Fitzpatrick modifier to something absent
/// from this list does not tint it: it produces the emoji followed by a bare
/// coloured square. Of the entries in this picker's database, seven are on this
/// list and the rest are not.
///
/// Transcribed from Unicode 15.1 `emoji-data.txt`.
const EMOJI_MODIFIER_BASE: &[(u32, u32)] = &[
    (0x261D, 0x261D),
    (0x26F9, 0x26F9),
    (0x270A, 0x270D),
    (0x1F385, 0x1F385),
    (0x1F3C2, 0x1F3C4),
    (0x1F3C7, 0x1F3C7),
    (0x1F3CA, 0x1F3CC),
    (0x1F442, 0x1F443),
    (0x1F446, 0x1F450),
    (0x1F466, 0x1F478),
    (0x1F47C, 0x1F47C),
    (0x1F481, 0x1F483),
    (0x1F485, 0x1F487),
    (0x1F48F, 0x1F48F),
    (0x1F491, 0x1F491),
    (0x1F4AA, 0x1F4AA),
    (0x1F574, 0x1F575),
    (0x1F57A, 0x1F57A),
    (0x1F590, 0x1F590),
    (0x1F595, 0x1F596),
    (0x1F645, 0x1F647),
    (0x1F64B, 0x1F64F),
    (0x1F6A3, 0x1F6A3),
    (0x1F6B4, 0x1F6B6),
    (0x1F6C0, 0x1F6C0),
    (0x1F6CC, 0x1F6CC),
    (0x1F90C, 0x1F90C),
    (0x1F90F, 0x1F90F),
    (0x1F918, 0x1F91F),
    (0x1F926, 0x1F926),
    (0x1F930, 0x1F939),
    (0x1F93C, 0x1F93E),
    (0x1F977, 0x1F977),
    (0x1F9B5, 0x1F9B6),
    (0x1F9B8, 0x1F9B9),
    (0x1F9BB, 0x1F9BB),
    (0x1F9CD, 0x1F9DD),
    (0x1FAC3, 0x1FAC5),
    (0x1FAF0, 0x1FAF8),
];

/// Whether a skin-tone modifier may follow `c`.
fn is_emoji_modifier_base(c: char) -> bool {
    let cp = c as u32;
    EMOJI_MODIFIER_BASE
        .binary_search_by(|&(lo, hi)| {
            if cp < lo {
                core::cmp::Ordering::Greater
            } else if cp > hi {
                core::cmp::Ordering::Less
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Whether `emoji` accepts a Fitzpatrick skin-tone modifier at all.
///
/// Decided by the first scalar, because a modifier tints the character it
/// directly follows -- an emoji that does not *start* with a modifier base has
/// nowhere to put one.
///
/// Known limit: a ZWJ sequence can carry a base in a later segment (man
/// technologist is a man joined to a laptop), and toning those means toning
/// each base segment rather than just the first. Every ZWJ entry in this
/// database is flag-shaped and takes no tone at all, so the distinction does
/// not bite yet; adding a person-shaped ZWJ sequence is what should trigger
/// generalising this.
pub fn takes_skin_tone(emoji: &str) -> bool {
    emoji.chars().next().is_some_and(is_emoji_modifier_base)
}

impl SkinToneModifier {
    /// All variants in order, including None.
    pub const ALL: &[SkinToneModifier] = &[
        SkinToneModifier::None,
        SkinToneModifier::Light,
        SkinToneModifier::MediumLight,
        SkinToneModifier::Medium,
        SkinToneModifier::MediumDark,
        SkinToneModifier::Dark,
    ];

    /// The Unicode code point for this Fitzpatrick modifier, if any.
    fn modifier_char(self) -> Option<char> {
        match self {
            Self::None => Option::None,
            Self::Light => Some('\u{1F3FB}'),
            Self::MediumLight => Some('\u{1F3FC}'),
            Self::Medium => Some('\u{1F3FD}'),
            Self::MediumDark => Some('\u{1F3FE}'),
            Self::Dark => Some('\u{1F3FF}'),
        }
    }

    /// Tint `base_emoji` with this Fitzpatrick modifier.
    ///
    /// Returns the emoji unchanged when the modifier is `None`, and *also*
    /// when the emoji is not something a skin tone can apply to -- see
    /// [`takes_skin_tone`]. That second case is the majority of any emoji set:
    /// tinting a pizza yields "\u{1F355}\u{1F3FE}", which draws as a pizza
    /// followed by a bare brown square rather than as a darker pizza.
    pub fn apply(self, base_emoji: &str) -> String {
        let Some(modifier) = self.modifier_char() else {
            return base_emoji.to_string();
        };
        let mut chars = base_emoji.chars();
        let Some(base) = chars.next() else {
            return String::new();
        };
        if !is_emoji_modifier_base(base) {
            return base_emoji.to_string();
        }
        // The modifier goes directly after the base it tints, not at the end of
        // the whole sequence. A variation selector in between is dropped: a
        // modifier already forces emoji presentation, so `base FE0F modifier`
        // is not a well-formed emoji_modifier_sequence (UTS #51).
        let rest = chars.as_str();
        let rest = rest.strip_prefix('\u{FE0F}').unwrap_or(rest);
        // 4 is the UTF-8 length of every skin-tone modifier, so this is the
        // final size or a little over when a selector was dropped.
        let mut result = String::with_capacity(base_emoji.len().saturating_add(4));
        result.push(base);
        result.push(modifier);
        result.push_str(rest);
        result
    }

    /// Display color for the skin-tone indicator circle.
    pub fn swatch_color(self) -> Color {
        match self {
            Self::None => Color::from_hex(0xFFCC4D),
            Self::Light => Color::from_hex(0xFADCBC),
            Self::MediumLight => Color::from_hex(0xE0BB95),
            Self::Medium => Color::from_hex(0xBF8B68),
            Self::MediumDark => Color::from_hex(0x9B643D),
            Self::Dark => Color::from_hex(0x594539),
        }
    }
}

// ============================================================================
// Emoji entry
// ============================================================================

/// A single emoji with metadata.
#[derive(Clone, Debug)]
pub struct EmojiEntry {
    /// The emoji character(s) (e.g. "\u{1F600}").
    pub emoji: String,
    /// Descriptive name (e.g. "grinning face").
    pub name: String,
    /// Category this emoji belongs to.
    pub category: EmojiCategory,
    /// Search keywords (lowercase).
    pub keywords: Vec<String>,
}

// ============================================================================
// Emoji database
// ============================================================================

/// The collection of all available emoji with search and recency tracking.
pub struct EmojiDatabase {
    /// All emoji entries.
    pub entries: Vec<EmojiEntry>,
    /// Recently used emoji characters, most-recent first. Capped at `MAX_RECENT`.
    pub recent: Vec<String>,
}

impl EmojiDatabase {
    /// Build a new database pre-populated with common emoji.
    pub fn new() -> Self {
        let entries = Self::build_entries();
        Self {
            entries,
            recent: Vec::new(),
        }
    }

    /// Search emoji by name and keywords. Returns entries whose name or any
    /// keyword contains the query (case-insensitive substring match).
    pub fn search<'a>(&'a self, query: &str) -> Vec<&'a EmojiEntry> {
        if query.is_empty() {
            return self.entries.iter().collect();
        }
        let q = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&q) || e.keywords.iter().any(|kw| kw.contains(&q))
            })
            .collect()
    }

    /// Return all emoji in the given category.
    pub fn by_category(&self, cat: EmojiCategory) -> Vec<&EmojiEntry> {
        self.entries.iter().filter(|e| e.category == cat).collect()
    }

    /// Record an emoji use, pushing it to the front of the recent list.
    /// Duplicates are moved to the front. List is capped at `MAX_RECENT`.
    pub fn record_use(&mut self, emoji: &str) {
        // Remove any existing occurrence so we can re-insert at front.
        self.recent.retain(|e| e != emoji);
        self.recent.insert(0, emoji.to_string());
        if self.recent.len() > MAX_RECENT {
            self.recent.truncate(MAX_RECENT);
        }
    }

    /// Return recently used emoji entries, most-recent first.
    /// Only includes emoji that still exist in the database.
    pub fn recent_entries(&self) -> Vec<&EmojiEntry> {
        self.recent
            .iter()
            .filter_map(|r| self.entries.iter().find(|e| e.emoji == *r))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Pre-built emoji database (~80+ common emoji across all 8 categories)
    // -----------------------------------------------------------------------

    fn entry(emoji: &str, name: &str, cat: EmojiCategory, kw: &[&str]) -> EmojiEntry {
        EmojiEntry {
            emoji: emoji.to_string(),
            name: name.to_string(),
            category: cat,
            keywords: kw.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn build_entries() -> Vec<EmojiEntry> {
        use EmojiCategory::{
            Activities, AnimalsAndNature, Flags, FoodAndDrink, Objects, SmileysAndPeople, Symbols,
            TravelAndPlaces,
        };
        let e = Self::entry;
        vec![
            // -- Smileys & People (16) --
            e(
                "\u{1F600}",
                "grinning face",
                SmileysAndPeople,
                &["happy", "smile", "joy"],
            ),
            e(
                "\u{1F602}",
                "face with tears of joy",
                SmileysAndPeople,
                &["laugh", "cry", "lol"],
            ),
            e(
                "\u{1F60D}",
                "smiling face with heart-eyes",
                SmileysAndPeople,
                &["love", "crush", "adore"],
            ),
            e(
                "\u{1F914}",
                "thinking face",
                SmileysAndPeople,
                &["think", "hmm", "consider"],
            ),
            e(
                "\u{1F44D}",
                "thumbs up",
                SmileysAndPeople,
                &["ok", "agree", "yes", "like"],
            ),
            e(
                "\u{1F44E}",
                "thumbs down",
                SmileysAndPeople,
                &["dislike", "no", "bad"],
            ),
            e(
                "\u{1F44B}",
                "waving hand",
                SmileysAndPeople,
                &["hello", "hi", "bye", "wave"],
            ),
            e(
                "\u{1F64F}",
                "folded hands",
                SmileysAndPeople,
                &["pray", "please", "thanks"],
            ),
            e(
                "\u{1F622}",
                "crying face",
                SmileysAndPeople,
                &["sad", "cry", "tear"],
            ),
            e(
                "\u{1F621}",
                "angry face",
                SmileysAndPeople,
                &["mad", "angry", "rage"],
            ),
            e(
                "\u{1F60E}",
                "smiling face with sunglasses",
                SmileysAndPeople,
                &["cool", "sunglasses"],
            ),
            e(
                "\u{1F917}",
                "hugging face",
                SmileysAndPeople,
                &["hug", "embrace"],
            ),
            e(
                "\u{1F631}",
                "face screaming in fear",
                SmileysAndPeople,
                &["scream", "horror", "fear"],
            ),
            e(
                "\u{1F4AA}",
                "flexed biceps",
                SmileysAndPeople,
                &["strong", "muscle", "power"],
            ),
            e(
                "\u{270B}",
                "raised hand",
                SmileysAndPeople,
                &["stop", "hand", "high five"],
            ),
            e(
                "\u{1F44F}",
                "clapping hands",
                SmileysAndPeople,
                &["clap", "bravo", "applause"],
            ),
            // -- Animals & Nature (12) --
            e(
                "\u{1F436}",
                "dog face",
                AnimalsAndNature,
                &["dog", "puppy", "pet"],
            ),
            e(
                "\u{1F431}",
                "cat face",
                AnimalsAndNature,
                &["cat", "kitten", "pet"],
            ),
            e(
                "\u{1F42D}",
                "mouse face",
                AnimalsAndNature,
                &["mouse", "rodent"],
            ),
            e("\u{1F43B}", "bear", AnimalsAndNature, &["bear", "animal"]),
            e(
                "\u{1F981}",
                "lion",
                AnimalsAndNature,
                &["lion", "king", "cat"],
            ),
            e(
                "\u{1F422}",
                "turtle",
                AnimalsAndNature,
                &["turtle", "slow", "shell"],
            ),
            e(
                "\u{1F98B}",
                "butterfly",
                AnimalsAndNature,
                &["butterfly", "insect", "pretty"],
            ),
            e(
                "\u{1F33B}",
                "sunflower",
                AnimalsAndNature,
                &["flower", "sun", "plant"],
            ),
            e(
                "\u{1F332}",
                "evergreen tree",
                AnimalsAndNature,
                &["tree", "pine", "forest"],
            ),
            e(
                "\u{1F335}",
                "cactus",
                AnimalsAndNature,
                &["cactus", "desert", "plant"],
            ),
            e(
                "\u{1F340}",
                "four leaf clover",
                AnimalsAndNature,
                &["luck", "clover", "irish"],
            ),
            e(
                "\u{1F308}",
                "rainbow",
                AnimalsAndNature,
                &["rainbow", "colors", "weather"],
            ),
            // -- Food & Drink (10) --
            e(
                "\u{1F34E}",
                "red apple",
                FoodAndDrink,
                &["apple", "fruit", "healthy"],
            ),
            e(
                "\u{1F354}",
                "hamburger",
                FoodAndDrink,
                &["burger", "fast food", "meat"],
            ),
            e(
                "\u{1F355}",
                "pizza",
                FoodAndDrink,
                &["pizza", "food", "italian"],
            ),
            e(
                "\u{1F382}",
                "birthday cake",
                FoodAndDrink,
                &["cake", "birthday", "party"],
            ),
            e(
                "\u{2615}",
                "hot beverage",
                FoodAndDrink,
                &["coffee", "tea", "drink", "hot"],
            ),
            e(
                "\u{1F37A}",
                "beer mug",
                FoodAndDrink,
                &["beer", "drink", "alcohol"],
            ),
            e(
                "\u{1F377}",
                "wine glass",
                FoodAndDrink,
                &["wine", "drink", "alcohol"],
            ),
            e(
                "\u{1F370}",
                "shortcake",
                FoodAndDrink,
                &["cake", "dessert", "sweet"],
            ),
            e(
                "\u{1F363}",
                "sushi",
                FoodAndDrink,
                &["sushi", "japanese", "fish"],
            ),
            e(
                "\u{1F36B}",
                "chocolate bar",
                FoodAndDrink,
                &["chocolate", "candy", "sweet"],
            ),
            // -- Travel & Places (10) --
            e(
                "\u{1F697}",
                "automobile",
                TravelAndPlaces,
                &["car", "drive", "vehicle"],
            ),
            e(
                "\u{2708}\u{FE0F}",
                "airplane",
                TravelAndPlaces,
                &["plane", "fly", "travel"],
            ),
            e(
                "\u{1F3E0}",
                "house",
                TravelAndPlaces,
                &["home", "house", "building"],
            ),
            e(
                "\u{1F3D6}\u{FE0F}",
                "beach with umbrella",
                TravelAndPlaces,
                &["beach", "vacation", "sun"],
            ),
            e(
                "\u{26F0}\u{FE0F}",
                "mountain",
                TravelAndPlaces,
                &["mountain", "climb", "nature"],
            ),
            e(
                "\u{1F680}",
                "rocket",
                TravelAndPlaces,
                &["rocket", "space", "launch"],
            ),
            e(
                "\u{1F30D}",
                "globe europe-africa",
                TravelAndPlaces,
                &["earth", "world", "globe"],
            ),
            e(
                "\u{1F3F0}",
                "castle",
                TravelAndPlaces,
                &["castle", "medieval", "palace"],
            ),
            e(
                "\u{26F2}",
                "fountain",
                TravelAndPlaces,
                &["fountain", "water", "park"],
            ),
            e(
                "\u{1F6A2}",
                "ship",
                TravelAndPlaces,
                &["ship", "boat", "cruise"],
            ),
            // -- Activities (10) --
            e(
                "\u{26BD}",
                "soccer ball",
                Activities,
                &["soccer", "football", "sport"],
            ),
            e(
                "\u{1F3C0}",
                "basketball",
                Activities,
                &["basketball", "sport", "nba"],
            ),
            e(
                "\u{1F3BE}",
                "tennis",
                Activities,
                &["tennis", "sport", "racket"],
            ),
            e(
                "\u{1F3AE}",
                "video game",
                Activities,
                &["game", "controller", "play"],
            ),
            e(
                "\u{1F3A8}",
                "artist palette",
                Activities,
                &["art", "paint", "draw"],
            ),
            e(
                "\u{1F3B5}",
                "musical note",
                Activities,
                &["music", "note", "song"],
            ),
            e(
                "\u{1F3AC}",
                "clapper board",
                Activities,
                &["movie", "film", "cinema"],
            ),
            e(
                "\u{1F3A4}",
                "microphone",
                Activities,
                &["microphone", "sing", "karaoke"],
            ),
            e(
                "\u{1F3C6}",
                "trophy",
                Activities,
                &["trophy", "win", "champion"],
            ),
            e(
                "\u{1F3AF}",
                "bullseye",
                Activities,
                &["target", "goal", "dart"],
            ),
            // -- Objects (10) --
            e(
                "\u{1F4A1}",
                "light bulb",
                Objects,
                &["idea", "light", "bulb"],
            ),
            e(
                "\u{1F4BB}",
                "laptop",
                Objects,
                &["computer", "laptop", "tech"],
            ),
            e(
                "\u{1F4F1}",
                "mobile phone",
                Objects,
                &["phone", "cell", "mobile"],
            ),
            e(
                "\u{1F4DA}",
                "books",
                Objects,
                &["book", "read", "study", "library"],
            ),
            e(
                "\u{1F4E7}",
                "e-mail",
                Objects,
                &["email", "mail", "message"],
            ),
            e("\u{1F511}", "key", Objects, &["key", "lock", "security"]),
            e(
                "\u{1F6E0}\u{FE0F}",
                "hammer and wrench",
                Objects,
                &["tools", "fix", "repair"],
            ),
            e(
                "\u{23F0}",
                "alarm clock",
                Objects,
                &["alarm", "clock", "time", "wake"],
            ),
            e(
                "\u{1F4B0}",
                "money bag",
                Objects,
                &["money", "rich", "bag", "dollar"],
            ),
            e(
                "\u{1F50D}",
                "magnifying glass",
                Objects,
                &["search", "find", "look"],
            ),
            // -- Symbols (8) --
            e(
                "\u{2764}\u{FE0F}",
                "red heart",
                Symbols,
                &["heart", "love", "romance"],
            ),
            e(
                "\u{1F494}",
                "broken heart",
                Symbols,
                &["heartbreak", "sad", "love"],
            ),
            e(
                "\u{2705}",
                "check mark",
                Symbols,
                &["check", "yes", "done", "correct"],
            ),
            e(
                "\u{274C}",
                "cross mark",
                Symbols,
                &["no", "wrong", "error", "cancel"],
            ),
            e("\u{2B50}", "star", Symbols, &["star", "favorite", "rating"]),
            e(
                "\u{26A0}\u{FE0F}",
                "warning",
                Symbols,
                &["warning", "caution", "alert"],
            ),
            e(
                "\u{267B}\u{FE0F}",
                "recycling symbol",
                Symbols,
                &["recycle", "environment", "green"],
            ),
            e(
                "\u{1F4AF}",
                "hundred points",
                Symbols,
                &["hundred", "perfect", "score"],
            ),
            // -- Flags (6) --
            e(
                "\u{1F3F3}\u{FE0F}",
                "white flag",
                Flags,
                &["flag", "surrender", "peace"],
            ),
            e("\u{1F3F4}", "black flag", Flags, &["flag", "pirate"]),
            e(
                "\u{1F3C1}",
                "chequered flag",
                Flags,
                &["flag", "race", "finish"],
            ),
            e(
                "\u{1F6A9}",
                "triangular flag",
                Flags,
                &["flag", "post", "marker"],
            ),
            e(
                "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}",
                "rainbow flag",
                Flags,
                &["flag", "pride", "rainbow", "lgbtq"],
            ),
            e("\u{2690}", "white pennant", Flags, &["flag", "pennant"]),
        ]
    }
}

impl Default for EmojiDatabase {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Virtual tab — categories + special tabs
// ============================================================================

/// Tabs shown in the tab bar: special tabs (recent, search) plus each category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Recent,
    Search,
    Category(EmojiCategory),
}

impl Tab {
    /// Icon for the tab.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Recent => "\u{1F552}", // clock face
            Self::Search => "\u{1F50E}", // magnifying glass tilted right
            Self::Category(c) => c.icon(),
        }
    }
}

// ============================================================================
// Layout
// ============================================================================
//
// Every number deciding *where* something is drawn lives here, because every
// one of them is asked twice: once by the renderer, and once by the hit test
// that has to agree with it. They used to be written out separately at each
// site -- `grid_left` five times, the bottom edge of the grid six times, the
// tab list twice in full -- so nothing but care kept the rectangle a user sees
// and the rectangle a click resolves to in the same place.

/// Left edge of the emoji grid: the columns are centred in the window.
fn grid_left() -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let columns = GRID_COLUMNS as f32;
    (WINDOW_WIDTH - columns * CELL_SIZE) / 2.0
}

/// Top edge of the skin-tone strip.
fn skin_tone_bar_y() -> f32 {
    WINDOW_HEIGHT - PREVIEW_HEIGHT - SKIN_TONE_HEIGHT
}

/// Bottom edge of the scrollable grid area.
///
/// The grid ends exactly where the skin-tone strip begins. Deriving one from
/// the other is what keeps that true: written separately, a change to the
/// strip's height would leave the grid either overlapping it or short of it.
fn grid_bottom() -> f32 {
    skin_tone_bar_y()
}

/// Distance between one skin-tone swatch's left edge and the next one's.
const SKIN_TONE_PITCH: f32 = SKIN_TONE_CIRCLE + SKIN_TONE_SPACING;

/// Left edge of the first skin-tone swatch, clear of the "Skin tone:" label.
const SKIN_TONE_FIRST_X: f32 = 80.0;

/// Left edge of skin-tone swatch `index`.
fn skin_tone_swatch_x(index: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let index = index as f32;
    SKIN_TONE_FIRST_X + index * SKIN_TONE_PITCH
}

/// The skin-tone swatch a click at `x` within the strip selects, if any.
///
/// A swatch's click cell is its whole pitch, not just the 18px circle it draws
/// inside. The six-pixel gaps between the circles used to belong to nobody, so
/// a click three pixels to the right of a swatch did nothing at all -- a dead
/// strip a quarter as wide as the swatches themselves.
fn skin_tone_swatch_at(x: f32) -> Option<usize> {
    let strip_left = SKIN_TONE_FIRST_X - SKIN_TONE_SPACING / 2.0;
    if x < strip_left {
        return Option::None;
    }
    // A float-to-integer cast saturates rather than wrapping, so a very large
    // `x` gives a very large index, which the bound below rejects.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = ((x - strip_left) / SKIN_TONE_PITCH) as usize;
    if index < SkinToneModifier::ALL.len() {
        Some(index)
    } else {
        Option::None
    }
}

/// How many tabs the bar holds: recently-used, every category, and search.
const TAB_COUNT: usize = EmojiCategory::ALL.len() + 2;

/// The tabs, in the order the bar draws them.
fn tabs() -> Vec<Tab> {
    let mut v = Vec::with_capacity(TAB_COUNT);
    v.push(Tab::Recent);
    v.extend(EmojiCategory::ALL.iter().copied().map(Tab::Category));
    v.push(Tab::Search);
    v
}

/// Width of one tab: the bar divides the window evenly.
fn tab_width() -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let count = TAB_COUNT as f32;
    WINDOW_WIDTH / count
}

/// Left edge of tab `index`.
fn tab_x(index: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let index = index as f32;
    index * tab_width()
}

/// The tab a click at `x` within the tab bar selects, if any.
fn tab_at(x: f32) -> Option<usize> {
    if x < 0.0 {
        // A float-to-integer cast saturates at zero, so a negative `x` would
        // otherwise land on tab 0 -- a click off the left edge of the window
        // silently switching to recently-used.
        return Option::None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = (x / tab_width()) as usize;
    if index < TAB_COUNT {
        Some(index)
    } else {
        Option::None
    }
}

// ============================================================================
// Picker state
// ============================================================================

/// Mutable state for the emoji picker popup.
pub struct EmojiPickerState {
    /// The currently active tab.
    pub active_tab: Tab,
    /// The category to show when a category tab is active.
    pub selected_category: EmojiCategory,
    /// Current search query text.
    pub search_query: String,
    /// Index of the emoji currently hovered in the visible grid, if any.
    pub hovered_emoji: Option<usize>,
    /// Vertical scroll offset of the grid (in pixels).
    pub scroll_offset: f32,
    /// Active skin tone modifier.
    pub skin_tone: SkinToneModifier,
    /// The emoji database.
    pub database: EmojiDatabase,
    /// Whether the picker is open (visible).
    pub is_open: bool,
    /// The last emoji that was selected (for clipboard / IPC output).
    pub last_selected: Option<String>,
    /// Whether the search field is focused.
    pub search_focused: bool,
}

impl EmojiPickerState {
    /// Create a new picker state with an initialized database.
    pub fn new() -> Self {
        Self {
            active_tab: Tab::Category(EmojiCategory::SmileysAndPeople),
            selected_category: EmojiCategory::SmileysAndPeople,
            search_query: String::new(),
            hovered_emoji: Option::None,
            scroll_offset: 0.0,
            skin_tone: SkinToneModifier::None,
            database: EmojiDatabase::new(),
            is_open: true,
            last_selected: Option::None,
            search_focused: false,
        }
    }

    /// The list of emoji currently visible in the grid based on the active tab
    /// and search query.
    pub fn visible_emoji(&self) -> Vec<&EmojiEntry> {
        match self.active_tab {
            Tab::Recent => self.database.recent_entries(),
            Tab::Search => self.database.search(&self.search_query),
            Tab::Category(cat) => self.database.by_category(cat),
        }
    }

    /// Total content height of the grid for the current visible emoji set.
    pub fn grid_content_height(&self) -> f32 {
        let count = self.visible_emoji().len();
        let rows = count.div_ceil(GRID_COLUMNS);
        rows as f32 * CELL_SIZE + GRID_PADDING * 2.0
    }

    /// The Y position where the scrollable grid area starts.
    pub fn grid_top(&self) -> f32 {
        TAB_BAR_HEIGHT + SEARCH_HEIGHT
    }

    /// The height available for the scrollable grid area.
    pub fn grid_area_height(&self) -> f32 {
        grid_bottom() - self.grid_top()
    }

    /// Maximum scroll offset (clamped to zero if content fits).
    pub fn max_scroll(&self) -> f32 {
        let content = self.grid_content_height();
        let visible = self.grid_area_height();
        if content > visible {
            content - visible
        } else {
            0.0
        }
    }

    /// Clamp the current scroll offset to valid bounds.
    pub fn clamp_scroll(&mut self) {
        let max = self.max_scroll();
        if self.scroll_offset < 0.0 {
            self.scroll_offset = 0.0;
        }
        if self.scroll_offset > max {
            self.scroll_offset = max;
        }
    }
}

impl Default for EmojiPickerState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Rendering
// ============================================================================

/// Render the complete emoji picker popup into a `RenderTree`.
pub fn render(state: &EmojiPickerState) -> RenderTree {
    let mut tree = RenderTree::new();
    if !state.is_open {
        return tree;
    }

    // Window background with rounded corners.
    tree.fill_rounded_rect(
        0.0,
        0.0,
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        mocha::BASE,
        CornerRadii::all(CORNER_RADIUS),
    );

    render_tab_bar(state, &mut tree);
    render_search_field(state, &mut tree);
    render_grid(state, &mut tree);
    render_skin_tone_bar(state, &mut tree);
    render_preview(state, &mut tree);

    tree
}

/// Render the category tab bar.
fn render_tab_bar(state: &EmojiPickerState, tree: &mut RenderTree) {
    // Tab bar background.
    tree.fill_rect(0.0, 0.0, WINDOW_WIDTH, TAB_BAR_HEIGHT, mocha::MANTLE);

    let tab_width = tab_width();

    for (i, &tab) in tabs().iter().enumerate() {
        let x = tab_x(i);
        let is_active = tab == state.active_tab;

        // Highlight active tab.
        if is_active {
            tree.fill_rounded_rect(
                x + 2.0,
                2.0,
                tab_width - 4.0,
                TAB_BAR_HEIGHT - 4.0,
                mocha::SURFACE0,
                CornerRadii::all(6.0),
            );
        }

        // Tab icon (emoji text).
        let icon_x = x + (tab_width - TAB_ICON_SIZE) / 2.0;
        let icon_y = (TAB_BAR_HEIGHT - TAB_ICON_SIZE) / 2.0;
        let color = if is_active {
            mocha::BLUE
        } else {
            mocha::OVERLAY0
        };
        tree.push(guitk::render::RenderCommand::Text {
            x: icon_x,
            y: icon_y,
            text: tab.icon().to_string(),
            color,
            font_size: TAB_ICON_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(tab_width),
            overflow: TextOverflow::Ellipsis,
        });
    }

    // Bottom divider line.
    tree.push(guitk::render::RenderCommand::Line {
        x1: 0.0,
        y1: TAB_BAR_HEIGHT,
        x2: WINDOW_WIDTH,
        y2: TAB_BAR_HEIGHT,
        color: mocha::SURFACE0,
        width: 1.0,
    });
}

/// Render the search input field.
fn render_search_field(state: &EmojiPickerState, tree: &mut RenderTree) {
    let y = TAB_BAR_HEIGHT;
    let field_margin = 8.0;
    let field_x = field_margin;
    let field_y = y + 4.0;
    let field_w = WINDOW_WIDTH - field_margin * 2.0;
    let field_h = SEARCH_HEIGHT - 8.0;

    // Field background.
    tree.fill_rounded_rect(
        field_x,
        field_y,
        field_w,
        field_h,
        mocha::SURFACE0,
        CornerRadii::all(6.0),
    );

    // Focus border.
    if state.search_focused {
        tree.push(guitk::render::RenderCommand::StrokeRect {
            x: field_x,
            y: field_y,
            width: field_w,
            height: field_h,
            color: mocha::BLUE,
            line_width: 1.5,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    // Search icon.
    tree.push(guitk::render::RenderCommand::Text {
        x: field_x + 8.0,
        y: field_y + (field_h - LABEL_FONT_SIZE) / 2.0,
        text: "\u{1F50D}".to_string(),
        color: mocha::OVERLAY0,
        font_size: LABEL_FONT_SIZE,
        font_weight: FontWeightHint::Regular,
        max_width: Option::None,
        overflow: TextOverflow::Clip,
    });

    // Query text or placeholder.
    let text_x = field_x + 28.0;
    let text_y = field_y + (field_h - LABEL_FONT_SIZE) / 2.0;
    if state.search_query.is_empty() {
        tree.push(guitk::render::RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: "Search emoji...".to_string(),
            color: mocha::OVERLAY0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 36.0),
            overflow: TextOverflow::Ellipsis,
        });
    } else {
        tree.push(guitk::render::RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: state.search_query.clone(),
            color: mocha::TEXT,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 36.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// Render the scrollable emoji grid.
fn render_grid(state: &EmojiPickerState, tree: &mut RenderTree) {
    let grid_top = state.grid_top();
    let grid_height = state.grid_area_height();
    let emoji_list = state.visible_emoji();

    // Clip to the grid area.
    tree.clip(0.0, grid_top, WINDOW_WIDTH, grid_height);
    tree.translate(0.0, -state.scroll_offset);

    let grid_left = grid_left();

    for (i, entry) in emoji_list.iter().enumerate() {
        let col = i % GRID_COLUMNS;
        let row = i / GRID_COLUMNS;
        let x = grid_left + col as f32 * CELL_SIZE;
        let y = grid_top + GRID_PADDING + row as f32 * CELL_SIZE;

        // Hover highlight.
        if state.hovered_emoji == Some(i) {
            tree.fill_rounded_rect(
                x + 1.0,
                y + 1.0,
                CELL_SIZE - 2.0,
                CELL_SIZE - 2.0,
                mocha::SURFACE1,
                CornerRadii::all(6.0),
            );
        }

        // Emoji glyph, wearing the chosen skin tone if it can. Showing the
        // untinted emoji here and the tinted one only in the preview would
        // make the grid disagree with what a click actually yields.
        let text_x = x + CELL_PADDING;
        let text_y = y + CELL_PADDING;
        tree.push(guitk::render::RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: state.skin_tone.apply(&entry.emoji),
            color: mocha::TEXT,
            font_size: EMOJI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(CELL_SIZE - CELL_PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    tree.untranslate();
    tree.unclip();
}

/// Render the skin tone selector strip.
fn render_skin_tone_bar(state: &EmojiPickerState, tree: &mut RenderTree) {
    let bar_y = skin_tone_bar_y();

    // Background.
    tree.fill_rect(0.0, bar_y, WINDOW_WIDTH, SKIN_TONE_HEIGHT, mocha::MANTLE);

    // Label.
    tree.push(guitk::render::RenderCommand::Text {
        x: 8.0,
        y: bar_y + (SKIN_TONE_HEIGHT - LABEL_FONT_SIZE) / 2.0,
        text: "Skin tone:".to_string(),
        color: mocha::SUBTEXT0,
        font_size: LABEL_FONT_SIZE - 1.0,
        font_weight: FontWeightHint::Regular,
        max_width: Option::None,
        overflow: TextOverflow::Clip,
    });

    // Skin tone circles.
    for (i, &tone) in SkinToneModifier::ALL.iter().enumerate() {
        let cx = skin_tone_swatch_x(i);
        let cy = bar_y + (SKIN_TONE_HEIGHT - SKIN_TONE_CIRCLE) / 2.0;

        // Circle background.
        tree.fill_rounded_rect(
            cx,
            cy,
            SKIN_TONE_CIRCLE,
            SKIN_TONE_CIRCLE,
            tone.swatch_color(),
            CornerRadii::all(SKIN_TONE_CIRCLE / 2.0),
        );

        // Selection ring: the swatch outset by 2px on every side, with a
        // corner radius of half its own width so it draws as a circle.
        if state.skin_tone == tone {
            let ring = SKIN_TONE_CIRCLE + 4.0;
            tree.push(guitk::render::RenderCommand::StrokeRect {
                x: cx - 2.0,
                y: cy - 2.0,
                width: ring,
                height: ring,
                color: mocha::BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::all(ring / 2.0),
            });
        }
    }
}

/// Render the preview area at the bottom of the popup.
fn render_preview(state: &EmojiPickerState, tree: &mut RenderTree) {
    let preview_y = WINDOW_HEIGHT - PREVIEW_HEIGHT;

    // Background.
    tree.fill_rounded_rect(
        0.0,
        preview_y,
        WINDOW_WIDTH,
        PREVIEW_HEIGHT,
        mocha::CRUST,
        CornerRadii {
            top_left: 0.0,
            top_right: 0.0,
            bottom_left: CORNER_RADIUS,
            bottom_right: CORNER_RADIUS,
        },
    );

    // Show the hovered emoji preview, or a hint.
    let emoji_list = state.visible_emoji();
    let hovered = state
        .hovered_emoji
        .and_then(|idx| emoji_list.get(idx).copied());

    match hovered {
        Some(entry) => {
            let modified = state.skin_tone.apply(&entry.emoji);
            // Large emoji.
            tree.push(guitk::render::RenderCommand::Text {
                x: 12.0,
                y: preview_y + 10.0,
                text: modified,
                color: mocha::TEXT,
                font_size: PREVIEW_EMOJI_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Option::None,
                overflow: TextOverflow::Clip,
            });
            // Name.
            tree.push(guitk::render::RenderCommand::Text {
                x: 56.0,
                y: preview_y + 12.0,
                text: entry.name.clone(),
                color: mocha::SUBTEXT1,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(WINDOW_WIDTH - 64.0),
                overflow: TextOverflow::Ellipsis,
            });
            // Category.
            tree.push(guitk::render::RenderCommand::Text {
                x: 56.0,
                y: preview_y + 30.0,
                text: entry.category.label().to_string(),
                color: mocha::OVERLAY0,
                font_size: LABEL_FONT_SIZE - 2.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(WINDOW_WIDTH - 64.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        Option::None => {
            tree.push(guitk::render::RenderCommand::Text {
                x: 12.0,
                y: preview_y + (PREVIEW_HEIGHT - LABEL_FONT_SIZE) / 2.0,
                text: "Hover over an emoji to preview".to_string(),
                color: mocha::OVERLAY0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(WINDOW_WIDTH - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

// ============================================================================
// Event handling
// ============================================================================

/// Handle an input event, returning whether it was consumed.
pub fn handle_event(state: &mut EmojiPickerState, event: &Event) -> EventResult {
    match event {
        Event::Key(key_ev) if key_ev.pressed => handle_key(state, key_ev),
        Event::Mouse(mouse_ev) => handle_mouse(state, mouse_ev),
        _ => EventResult::Ignored,
    }
}

/// Process a keyboard event.
fn handle_key(state: &mut EmojiPickerState, key: &KeyEvent) -> EventResult {
    match key.key {
        Key::Escape => {
            state.is_open = false;
            EventResult::Consumed
        }
        Key::Backspace if state.search_focused => {
            state.search_query.pop();
            state.scroll_offset = 0.0;
            if !state.search_query.is_empty() {
                state.active_tab = Tab::Search;
            }
            EventResult::Consumed
        }
        _ if state.search_focused => {
            if let Some(ch) = key.text
                && !ch.is_control()
            {
                state.search_query.push(ch);
                state.active_tab = Tab::Search;
                state.scroll_offset = 0.0;
                return EventResult::Consumed;
            }
            EventResult::Ignored
        }
        _ => EventResult::Ignored,
    }
}

/// Process a mouse event.
fn handle_mouse(state: &mut EmojiPickerState, mouse: &MouseEvent) -> EventResult {
    let x = mouse.x;
    let y = mouse.y;

    match &mouse.kind {
        MouseEventKind::Press(MouseButton::Left) => {
            // Tab bar click.
            if y < TAB_BAR_HEIGHT {
                return handle_tab_click(state, x);
            }

            // Search field click — focus it.
            if (TAB_BAR_HEIGHT..TAB_BAR_HEIGHT + SEARCH_HEIGHT).contains(&y) {
                state.search_focused = true;
                return EventResult::Consumed;
            }

            // Grid click — select emoji.
            let grid_top = state.grid_top();
            let grid_bottom = grid_bottom();
            if y >= grid_top && y < grid_bottom {
                state.search_focused = false;
                if let Some(idx) = grid_hit_test(state, x, y) {
                    let emoji_list = state.visible_emoji();
                    if let Some(entry) = emoji_list.get(idx) {
                        let base_emoji = entry.emoji.clone();
                        let modified = state.skin_tone.apply(&base_emoji);
                        state.database.record_use(&base_emoji);
                        state.last_selected = Some(modified);
                    }
                }
                return EventResult::Consumed;
            }

            // Skin tone bar click.
            let skin_bar_y = skin_tone_bar_y();
            if y >= skin_bar_y && y < skin_bar_y + SKIN_TONE_HEIGHT {
                return handle_skin_tone_click(state, x);
            }

            state.search_focused = false;
            EventResult::Consumed
        }

        MouseEventKind::Move => {
            // Update hover in grid area.
            let grid_top = state.grid_top();
            let grid_bottom = grid_bottom();
            if y >= grid_top && y < grid_bottom {
                state.hovered_emoji = grid_hit_test(state, x, y);
            } else {
                state.hovered_emoji = Option::None;
            }
            EventResult::Consumed
        }

        MouseEventKind::Scroll { dy, .. } => {
            let grid_top = state.grid_top();
            let grid_bottom = grid_bottom();
            if y >= grid_top && y < grid_bottom {
                // `dy` is a notch count, not a distance. Subtracting it
                // raw moved the grid one pixel per notch — a 48px cell
                // took 48 notches to clear, so the wheel looked broken.
                state.scroll_offset += wheel::pixels(*dy, CELL_SIZE);
                state.clamp_scroll();
                return EventResult::Consumed;
            }
            EventResult::Ignored
        }

        _ => EventResult::Ignored,
    }
}

/// Handle a click on the tab bar, selecting the corresponding tab.
fn handle_tab_click(state: &mut EmojiPickerState, x: f32) -> EventResult {
    let tabs = tabs();
    if let Some(&tab) = tab_at(x).and_then(|idx| tabs.get(idx)) {
        state.active_tab = tab;
        state.scroll_offset = 0.0;
        if let Tab::Category(cat) = tab {
            state.selected_category = cat;
        }
        state.search_focused = tab == Tab::Search;
    }
    EventResult::Consumed
}

/// Handle a click on the skin tone selector bar.
///
/// The caller has already established that the click is inside the strip, so
/// only `x` decides which swatch. Consumed either way: a click that lands on
/// the strip's label rather than a swatch is still a click on the popup, and
/// reporting it as ignored would offer it to whatever is behind.
fn handle_skin_tone_click(state: &mut EmojiPickerState, x: f32) -> EventResult {
    if let Some(&tone) = skin_tone_swatch_at(x).and_then(|i| SkinToneModifier::ALL.get(i)) {
        state.skin_tone = tone;
    }
    EventResult::Consumed
}

/// Given a mouse position within the grid area, return the index of the emoji
/// under the cursor, if any.
fn grid_hit_test(state: &EmojiPickerState, x: f32, y: f32) -> Option<usize> {
    let grid_top = state.grid_top();
    let grid_left = grid_left();

    // Adjust for scroll.
    let adjusted_y = y - grid_top + state.scroll_offset - GRID_PADDING;
    let adjusted_x = x - grid_left;

    if adjusted_x < 0.0 || adjusted_y < 0.0 {
        return Option::None;
    }

    let col = (adjusted_x / CELL_SIZE) as usize;
    let row = (adjusted_y / CELL_SIZE) as usize;

    if col >= GRID_COLUMNS {
        return Option::None;
    }

    // A float-to-integer cast saturates, so a pointer far below the grid —
    // or a scroll offset that has grown large — gives a row near
    // `usize::MAX`. Multiplying that out overflows and panics in a debug
    // build, so a row that cannot be indexed is reported as a miss instead
    // of being computed.
    let idx = row
        .checked_mul(GRID_COLUMNS)
        .and_then(|i| i.checked_add(col))?;
    let count = state.visible_emoji().len();
    if idx < count { Some(idx) } else { Option::None }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let mut state = EmojiPickerState::new();

    // Initial render to verify everything works.
    let tree = render(&state);
    let _ = tree.len();
    let _ = &mut state;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // --- Database population ---

    #[test]
    fn database_has_at_least_80_emoji() {
        let db = EmojiDatabase::new();
        assert!(
            db.entries.len() >= 80,
            "expected at least 80 emoji, got {}",
            db.entries.len()
        );
    }

    #[test]
    fn database_covers_all_categories() {
        let db = EmojiDatabase::new();
        for &cat in EmojiCategory::ALL {
            let count = db.by_category(cat).len();
            assert!(count > 0, "category {:?} has no emoji", cat);
        }
    }

    #[test]
    fn all_entries_have_non_empty_fields() {
        let db = EmojiDatabase::new();
        for entry in &db.entries {
            assert!(!entry.emoji.is_empty(), "emoji string is empty");
            assert!(!entry.name.is_empty(), "name is empty for {}", entry.emoji);
        }
    }

    #[test]
    fn no_duplicate_emoji() {
        let db = EmojiDatabase::new();
        let mut seen = std::collections::HashSet::new();
        for entry in &db.entries {
            assert!(
                seen.insert(&entry.emoji),
                "duplicate emoji: {}",
                entry.emoji
            );
        }
    }

    // --- Search ---

    #[test]
    fn search_exact_name_match() {
        let db = EmojiDatabase::new();
        let results = db.search("grinning face");
        assert!(
            results.iter().any(|e| e.name == "grinning face"),
            "exact name search should find 'grinning face'"
        );
    }

    #[test]
    fn search_partial_name_match() {
        let db = EmojiDatabase::new();
        let results = db.search("grin");
        assert!(
            results.iter().any(|e| e.name.contains("grin")),
            "partial search 'grin' should match"
        );
    }

    #[test]
    fn search_keyword_match() {
        let db = EmojiDatabase::new();
        // "happy" is a keyword for grinning face
        let results = db.search("happy");
        assert!(
            !results.is_empty(),
            "'happy' keyword search should return results"
        );
    }

    #[test]
    fn search_case_insensitive() {
        let db = EmojiDatabase::new();
        let lower = db.search("pizza");
        let upper = db.search("PIZZA");
        assert_eq!(
            lower.len(),
            upper.len(),
            "search should be case-insensitive"
        );
    }

    #[test]
    fn search_no_results() {
        let db = EmojiDatabase::new();
        let results = db.search("xyznonexistent");
        assert!(
            results.is_empty(),
            "nonsense query should return no results"
        );
    }

    #[test]
    fn search_empty_query_returns_all() {
        let db = EmojiDatabase::new();
        let results = db.search("");
        assert_eq!(
            results.len(),
            db.entries.len(),
            "empty query should return all emoji"
        );
    }

    #[test]
    fn search_multiple_keyword_hits() {
        let db = EmojiDatabase::new();
        // "drink" should match multiple food/drink emoji
        let results = db.search("drink");
        assert!(
            results.len() >= 2,
            "expected multiple matches for 'drink', got {}",
            results.len()
        );
    }

    // --- Category filtering ---

    #[test]
    fn by_category_smileys() {
        let db = EmojiDatabase::new();
        let smileys = db.by_category(EmojiCategory::SmileysAndPeople);
        assert!(smileys.len() >= 10, "should have at least 10 smileys");
        for e in &smileys {
            assert_eq!(e.category, EmojiCategory::SmileysAndPeople);
        }
    }

    #[test]
    fn by_category_flags() {
        let db = EmojiDatabase::new();
        let flags = db.by_category(EmojiCategory::Flags);
        assert!(!flags.is_empty());
        for e in &flags {
            assert_eq!(e.category, EmojiCategory::Flags);
        }
    }

    #[test]
    fn category_counts_sum_to_total() {
        let db = EmojiDatabase::new();
        let total: usize = EmojiCategory::ALL
            .iter()
            .map(|&cat| db.by_category(cat).len())
            .sum();
        assert_eq!(
            total,
            db.entries.len(),
            "sum of category counts should equal total entries"
        );
    }

    // --- Recent emoji tracking ---

    #[test]
    fn record_use_adds_to_recent() {
        let mut db = EmojiDatabase::new();
        assert!(db.recent.is_empty());
        db.record_use("\u{1F600}");
        assert_eq!(db.recent.len(), 1);
        assert_eq!(db.recent[0], "\u{1F600}");
    }

    #[test]
    fn record_use_moves_duplicate_to_front() {
        let mut db = EmojiDatabase::new();
        db.record_use("A");
        db.record_use("B");
        db.record_use("A");
        assert_eq!(db.recent.len(), 2);
        assert_eq!(db.recent[0], "A");
        assert_eq!(db.recent[1], "B");
    }

    #[test]
    fn record_use_caps_at_max_recent() {
        let mut db = EmojiDatabase::new();
        for i in 0..MAX_RECENT + 10 {
            db.record_use(&format!("E{}", i));
        }
        assert_eq!(db.recent.len(), MAX_RECENT);
    }

    #[test]
    fn recent_entries_returns_matching_database_entries() {
        let mut db = EmojiDatabase::new();
        let first_emoji = db.entries[0].emoji.clone();
        db.record_use(&first_emoji);
        let recent = db.recent_entries();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].emoji, first_emoji);
    }

    #[test]
    fn recent_entries_ignores_unknown_emoji() {
        let mut db = EmojiDatabase::new();
        db.record_use("NOT_IN_DB");
        let recent = db.recent_entries();
        assert!(
            recent.is_empty(),
            "unknown emoji should not appear in recent_entries"
        );
    }

    #[test]
    fn recent_preserves_order() {
        let mut db = EmojiDatabase::new();
        db.record_use("A");
        db.record_use("B");
        db.record_use("C");
        assert_eq!(db.recent[0], "C");
        assert_eq!(db.recent[1], "B");
        assert_eq!(db.recent[2], "A");
    }

    // --- Skin tone modifier ---

    #[test]
    fn skin_tone_none_returns_original() {
        let result = SkinToneModifier::None.apply("\u{1F44D}");
        assert_eq!(result, "\u{1F44D}");
    }

    #[test]
    fn skin_tone_light_appends_modifier() {
        let result = SkinToneModifier::Light.apply("\u{1F44D}");
        assert!(result.starts_with("\u{1F44D}"));
        assert!(result.contains('\u{1F3FB}'));
    }

    #[test]
    fn skin_tone_dark_appends_modifier() {
        let result = SkinToneModifier::Dark.apply("\u{1F44D}");
        assert!(result.contains('\u{1F3FF}'));
    }

    #[test]
    fn skin_tone_all_variants_are_distinct() {
        let base = "\u{1F44D}";
        let results: Vec<String> = SkinToneModifier::ALL
            .iter()
            .map(|t| t.apply(base))
            .collect();
        let unique: std::collections::HashSet<&String> = results.iter().collect();
        assert_eq!(
            unique.len(),
            SkinToneModifier::ALL.len(),
            "all skin tone variants should produce distinct strings"
        );
    }

    #[test]
    fn skin_tone_medium_modifier_char() {
        let ch = SkinToneModifier::Medium.modifier_char();
        assert_eq!(ch, Some('\u{1F3FD}'));
    }

    #[test]
    fn skin_tone_swatch_colors_are_all_opaque() {
        for &tone in SkinToneModifier::ALL {
            let color = tone.swatch_color();
            assert_eq!(color.a, 255, "swatch color should be fully opaque");
        }
    }

    // --- Skin tone applies only to emoji that have skin ----------------------
    //
    // The tests above this block all tint "\u{1F44D}", which is one of the
    // seven entries in the shipped database that a Fitzpatrick modifier may
    // legally follow. The other seventy-five were being tinted too, and no
    // test noticed, because none of them was ever passed to `apply`.

    /// Everything in the database, split by whether Unicode says it has skin.
    fn entries_by_skin(state: &EmojiPickerState) -> (Vec<String>, Vec<String>) {
        let mut with = Vec::new();
        let mut without = Vec::new();
        for entry in state.database.search("") {
            if takes_skin_tone(&entry.emoji) {
                with.push(entry.emoji.clone());
            } else {
                without.push(entry.emoji.clone());
            }
        }
        assert!(
            !with.is_empty() && !without.is_empty(),
            "the database must hold some of each kind, or these tests are vacuous"
        );
        (with, without)
    }

    /// Every Fitzpatrick modifier, as characters.
    fn modifier_chars() -> Vec<char> {
        SkinToneModifier::ALL
            .iter()
            .filter_map(|t| t.modifier_char())
            .collect()
    }

    /// The bug, stated directly: choosing a skin tone must not staple a
    /// coloured square onto a slice of pizza.
    #[test]
    fn a_tone_is_not_stuck_onto_emoji_that_have_no_skin() {
        let state = EmojiPickerState::new();
        let (_, without) = entries_by_skin(&state);
        for emoji in &without {
            for &tone in SkinToneModifier::ALL {
                assert_eq!(
                    &tone.apply(emoji),
                    emoji,
                    "{tone:?} changed {emoji}, which takes no skin tone"
                );
            }
        }
    }

    /// ...and must still tint the ones that do, exactly once.
    #[test]
    fn an_emoji_that_has_skin_gets_exactly_one_modifier() {
        let state = EmojiPickerState::new();
        let (with, _) = entries_by_skin(&state);
        let modifiers = modifier_chars();
        for emoji in &with {
            for &tone in SkinToneModifier::ALL {
                let toned = tone.apply(emoji);
                let count = toned.chars().filter(|c| modifiers.contains(c)).count();
                match tone.modifier_char() {
                    Some(ch) => {
                        assert_eq!(count, 1, "{emoji} + {tone:?} = {toned}");
                        assert!(toned.contains(ch), "{emoji} + {tone:?} = {toned}");
                        assert!(toned.starts_with(emoji.chars().next().unwrap_or(' ')));
                    }
                    Option::None => assert_eq!(&toned, emoji),
                }
            }
        }
    }

    /// A handful of codepoints checked against Unicode by hand, so the table
    /// is answerable to something other than itself. The two faces are the
    /// interesting cases: they look like people and are not modifier bases.
    #[test]
    fn the_modifier_base_table_agrees_with_unicode() {
        for (c, expected) in [
            ('\u{1F44D}', true),  // thumbs up
            ('\u{270B}', true),   // raised hand
            ('\u{1F4AA}', true),  // flexed biceps
            ('\u{1F9D1}', true),  // person -- inside 1F9CD..1F9DD
            ('\u{1FAF6}', true),  // heart hands -- inside 1FAF0..1FAF8
            ('\u{1F600}', false), // grinning face
            ('\u{1F602}', false), // face with tears of joy
            ('\u{1F355}', false), // pizza
            ('\u{1F436}', false), // dog face
            ('\u{1F3F3}', false), // white flag
            ('\u{1F3FB}', false), // a modifier is not itself a base
            ('a', false),
        ] {
            assert_eq!(is_emoji_modifier_base(c), expected, "U+{:04X}", c as u32);
        }
    }

    /// The binary search in `is_emoji_modifier_base` is only correct if the
    /// table is sorted and its ranges do not touch or overlap.
    #[test]
    fn the_modifier_base_table_is_sorted_and_disjoint() {
        for pair in EMOJI_MODIFIER_BASE.windows(2) {
            let (lo, hi) = pair[0];
            let (next_lo, _) = pair[1];
            assert!(lo <= hi, "range U+{lo:04X}..U+{hi:04X} runs backwards");
            assert!(
                hi < next_lo,
                "U+{hi:04X} and U+{next_lo:04X} overlap or should be one range"
            );
        }
    }

    /// The modifier tints the character it *follows*, so it goes directly
    /// after the base and not at the end of the whole sequence. Every
    /// tone-taking entry in the shipped database is a single character, which
    /// makes the two placements indistinguishable there -- so this uses a ZWJ
    /// sequence, where they differ.
    ///
    /// It also pins the known limit in `takes_skin_tone`'s doc comment: the
    /// leading man is tinted and the laptop he is joined to is not, which is
    /// correct here and would not be for two joined people.
    #[test]
    fn the_modifier_goes_after_the_base_it_tints_not_at_the_end() {
        assert_eq!(
            SkinToneModifier::Dark.apply("\u{1F468}\u{200D}\u{1F4BB}"),
            "\u{1F468}\u{1F3FF}\u{200D}\u{1F4BB}"
        );
    }

    /// A variation selector asks for emoji presentation; so does a skin-tone
    /// modifier. Leaving the selector between the two makes the sequence
    /// ill-formed, so it is dropped rather than carried along.
    #[test]
    fn a_variation_selector_does_not_come_between_a_base_and_its_tone() {
        assert_eq!(
            SkinToneModifier::Light.apply("\u{261D}\u{FE0F}"),
            "\u{261D}\u{1F3FB}"
        );
    }

    /// A picker showing every emoji at once, so a cell can be looked up
    /// without first working out which tab its entry lives under.
    fn every_emoji() -> EmojiPickerState {
        let mut state = EmojiPickerState::new();
        state.active_tab = Tab::Search;
        state.search_query = String::new();
        state
    }

    /// Where the grid painted the cell for `base`, taken from the render tree
    /// rather than recomputed, so a click driven from it lands where the
    /// emoji actually is.
    ///
    /// Matched on `base` as a *prefix*: the painted text carries whatever skin
    /// tone is currently chosen, and asking for the exact tinted string would
    /// mean asking the code under test where to click.
    fn painted_cell(state: &EmojiPickerState, base: &str) -> (f32, f32) {
        let tree = render(state);
        let found = tree.commands.iter().find_map(|cmd| match cmd {
            guitk::render::RenderCommand::Text {
                x,
                y,
                text,
                font_size,
                ..
            } if text.starts_with(base) && (*font_size - EMOJI_FONT_SIZE).abs() < 0.01 => {
                Some((*x, *y))
            }
            _ => Option::None,
        });
        match found {
            Some(xy) => xy,
            Option::None => panic!("the grid never painted {base}"),
        }
    }

    /// Click the grid cell that `emoji` was painted in.
    fn click_painted_cell(state: &mut EmojiPickerState, emoji: &str) {
        let (x, y) = painted_cell(state, emoji);
        handle_event(
            state,
            &Event::Mouse(MouseEvent {
                x: x + 1.0,
                y: y + 1.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
    }

    /// End to end, which is how a user would have met this: choose a dark
    /// skin tone, click a dog, and the picker used to hand over a dog
    /// followed by a brown square.
    #[test]
    fn picking_a_toneless_emoji_yields_it_unchanged() {
        let mut state = every_emoji();
        state.skin_tone = SkinToneModifier::Dark;
        click_painted_cell(&mut state, "\u{1F436}");
        assert_eq!(state.last_selected.as_deref(), Some("\u{1F436}"));
    }

    /// ...while an emoji that does have skin still comes back tinted.
    #[test]
    fn picking_an_emoji_with_skin_yields_it_tinted() {
        let mut state = every_emoji();
        state.skin_tone = SkinToneModifier::Dark;
        click_painted_cell(&mut state, "\u{1F44D}");
        assert_eq!(state.last_selected.as_deref(), Some("\u{1F44D}\u{1F3FF}"));
    }

    /// What the grid shows and what a click yields are the same string. The
    /// grid used to draw every emoji untinted whatever the tone was set to,
    /// so the choice was invisible until after it had been made.
    #[test]
    fn the_grid_draws_the_emoji_the_chosen_tone_would_produce() {
        let mut state = every_emoji();
        state.skin_tone = SkinToneModifier::MediumDark;
        // Painted at all -- `painted_cell` panics otherwise.
        let _ = painted_cell(&state, "\u{1F44D}\u{1F3FE}");
        let _ = painted_cell(&state, "\u{1F436}");

        let tree = render(&state);
        let untinted = tree.commands.iter().any(|cmd| {
            matches!(
                cmd,
                guitk::render::RenderCommand::Text { text, font_size, .. }
                    if text == "\u{1F44D}" && (*font_size - EMOJI_FONT_SIZE).abs() < 0.01
            )
        });
        assert!(
            !untinted,
            "the grid drew the plain thumbs-up while a skin tone was chosen"
        );
    }

    /// Recently-used tracks the emoji itself, not the tinted copy, so that
    /// changing the tone later re-tints the recent list instead of leaving a
    /// row frozen at whatever tone was active when it was picked.
    #[test]
    fn recently_used_records_the_untinted_emoji() {
        let mut state = every_emoji();
        state.skin_tone = SkinToneModifier::Light;
        click_painted_cell(&mut state, "\u{1F44D}");
        state.skin_tone = SkinToneModifier::Dark;
        state.active_tab = Tab::Recent;
        let _ = painted_cell(&state, "\u{1F44D}\u{1F3FF}");
    }

    // --- Category enumeration ---

    #[test]
    fn category_all_has_eight_entries() {
        assert_eq!(EmojiCategory::ALL.len(), 8);
    }

    #[test]
    fn category_icons_are_non_empty() {
        for &cat in EmojiCategory::ALL {
            assert!(!cat.icon().is_empty(), "{:?} icon is empty", cat);
        }
    }

    #[test]
    fn category_labels_are_non_empty() {
        for &cat in EmojiCategory::ALL {
            assert!(!cat.label().is_empty(), "{:?} label is empty", cat);
        }
    }

    // --- Render tree generation ---

    #[test]
    fn render_produces_non_empty_tree() {
        let state = EmojiPickerState::new();
        let tree = render(&state);
        assert!(!tree.is_empty(), "render should produce commands");
    }

    #[test]
    fn render_closed_picker_is_empty() {
        let mut state = EmojiPickerState::new();
        state.is_open = false;
        let tree = render(&state);
        assert!(tree.is_empty(), "closed picker should produce no commands");
    }

    #[test]
    fn render_with_hover_includes_more_commands() {
        let state_no_hover = EmojiPickerState::new();
        let tree_no = render(&state_no_hover);

        let mut state_hover = EmojiPickerState::new();
        state_hover.hovered_emoji = Some(0);
        let tree_yes = render(&state_hover);

        assert!(
            tree_yes.len() > tree_no.len(),
            "hovering should add extra render commands (highlight + preview)"
        );
    }

    #[test]
    fn render_with_search_text_shows_query() {
        let mut state = EmojiPickerState::new();
        state.search_query = "test query".to_string();
        let tree = render(&state);
        // The tree should contain a text command with the query.
        let has_query = tree.commands.iter().any(|cmd| {
            matches!(cmd, guitk::render::RenderCommand::Text { text, .. } if text == "test query")
        });
        assert!(has_query, "render should include the search query text");
    }

    // --- Event handling ---

    #[test]
    fn escape_closes_picker() {
        let mut state = EmojiPickerState::new();
        assert!(state.is_open);
        let event = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Option::None,
        });
        let result = handle_event(&mut state, &event);
        assert_eq!(result, EventResult::Consumed);
        assert!(!state.is_open);
    }

    #[test]
    fn category_tab_click_switches_category() {
        let mut state = EmojiPickerState::new();
        // Tab order: Recent, SmileysAndPeople, AnimalsAndNature, ...
        // so the middle of tab 2 is AnimalsAndNature.
        let event = Event::Mouse(MouseEvent {
            x: tab_x(2) + tab_width() / 2.0,
            y: TAB_BAR_HEIGHT / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        let result = handle_event(&mut state, &event);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(
            state.active_tab,
            Tab::Category(EmojiCategory::AnimalsAndNature)
        );
    }

    #[test]
    fn search_text_input_updates_query() {
        let mut state = EmojiPickerState::new();
        state.search_focused = true;
        let event = Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('a'),
        });
        handle_event(&mut state, &event);
        assert_eq!(state.search_query, "a");
        assert_eq!(state.active_tab, Tab::Search);
    }

    #[test]
    fn backspace_removes_character() {
        let mut state = EmojiPickerState::new();
        state.search_focused = true;
        state.search_query = "ab".to_string();
        let event = Event::Key(KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Option::None,
        });
        handle_event(&mut state, &event);
        assert_eq!(state.search_query, "a");
    }

    /// A picker whose grid is genuinely taller than the panel it draws into.
    ///
    /// It has to be the *search* tab: the shipped database holds 82 emoji
    /// across eight categories, and the largest of them (16) fills 160px of a
    /// 320px panel, so no category tab can scroll at all. Searching "a"
    /// matches 73 of them, which overflows. A scroll test on a grid that fits
    /// would assert nothing.
    fn scrollable_picker() -> EmojiPickerState {
        let mut state = EmojiPickerState::new();
        state.active_tab = Tab::Search;
        state.search_query = "a".to_string();
        assert!(
            state.max_scroll() > CELL_SIZE,
            "the fixture must overflow the panel, or these tests prove nothing"
        );
        state
    }

    /// One turn of the wheel over the grid, as the compositor sends it: a
    /// notch *count*, not a pixel distance.
    fn wheel_over_grid(state: &EmojiPickerState, dy: f32) -> Event {
        Event::Mouse(MouseEvent {
            x: WINDOW_WIDTH / 2.0,
            y: state.grid_top() + 10.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })
    }

    /// The regression test. `state.scroll_offset -= dy` moved the grid **one
    /// pixel** per notch, so clearing a single 48px row of emoji took 48 turns
    /// of the wheel.
    ///
    /// The test this replaces sent `dy: -30.0` — a pixel distance, picked so
    /// that the raw subtraction would produce a visible number — and then
    /// asserted only that the offset was `>= 0.0`, which is equally true of an
    /// offset that never moved. It could not have failed.
    #[test]
    fn one_notch_scrolls_the_grid_a_visible_distance() {
        let mut state = scrollable_picker();
        let event = wheel_over_grid(&state, -1.0);
        handle_event(&mut state, &event);
        assert!(
            state.scroll_offset >= CELL_SIZE,
            "one notch must clear at least one {CELL_SIZE}px row, but moved {}px",
            state.scroll_offset
        );
    }

    /// Away from the user scrolls down the grid; back towards the user returns.
    ///
    /// The down leg asserts a whole *row*, not merely a non-zero number:
    /// `> 0.0` also holds under the one-pixel-per-notch defect this file was
    /// fixed for, and a direction test that cannot tell 1px from 144px is
    /// only half a test.
    // The `0.0` is exact: a negative offset is clamped by assigning the
    // literal, with no arithmetic left to round.
    #[allow(clippy::float_cmp)]
    #[test]
    fn the_wheel_scrolls_the_grid_both_ways() {
        let mut state = scrollable_picker();

        let down = wheel_over_grid(&state, -1.0);
        handle_event(&mut state, &down);
        assert!(state.scroll_offset >= CELL_SIZE);

        let up = wheel_over_grid(&state, 1.0);
        handle_event(&mut state, &up);
        assert_eq!(
            state.scroll_offset, 0.0,
            "the same notch back returns to the top"
        );
    }

    /// The wheel stays inside the content: the clamp still applies.
    ///
    /// Ten notches, not five hundred: at three rows a notch that is 1440px
    /// against a 320px range, so it overruns several times over while still
    /// being a count sized to the *converter* rather than one padded until it
    /// happened to overrun at the old rate of a pixel a notch.
    // Exact: the clamp assigns `max_scroll()` itself, so the two sides are
    // the same expression rather than two roundings of one quantity.
    #[allow(clippy::float_cmp)]
    #[test]
    fn the_wheel_cannot_scroll_past_the_end_of_the_grid() {
        let mut state = scrollable_picker();
        for _ in 0..10 {
            let event = wheel_over_grid(&state, -1.0);
            handle_event(&mut state, &event);
        }
        assert_eq!(state.scroll_offset, state.max_scroll());
    }

    /// A scroll outside the grid area is not the grid's to consume.
    // Exact: the point of the test is that nothing is written, so the offset
    // is still the constructor's literal `0.0`.
    #[allow(clippy::float_cmp)]
    #[test]
    fn a_scroll_over_the_tab_bar_leaves_the_grid_alone() {
        let mut state = scrollable_picker();
        let event = Event::Mouse(MouseEvent {
            x: WINDOW_WIDTH / 2.0,
            y: 2.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
        });
        assert_eq!(handle_event(&mut state, &event), EventResult::Ignored);
        assert_eq!(state.scroll_offset, 0.0);
    }

    /// A pointer far below the grid must miss, not overflow.
    ///
    /// `grid_hit_test` derives its row with `(adjusted_y / CELL_SIZE) as
    /// usize`, and a float-to-integer cast *saturates* rather than wrapping —
    /// so a large `y` produces a row near `usize::MAX`, and the row-major
    /// `row * GRID_COLUMNS` that follows overflowed and panicked in a debug
    /// build. Nothing bounds `y`: it arrives from the event.
    #[test]
    fn a_pointer_far_below_the_grid_misses_instead_of_overflowing() {
        let state = scrollable_picker();
        for y in [1.0e9_f32, 1.0e30, f32::MAX, f32::INFINITY] {
            assert_eq!(
                grid_hit_test(&state, WINDOW_WIDTH / 2.0, y),
                Option::None,
                "y={y} should miss the grid"
            );
        }
    }

    #[test]
    fn mouse_move_updates_hover() {
        let mut state = EmojiPickerState::new();
        let grid_top = state.grid_top();
        let event = Event::Mouse(MouseEvent {
            x: grid_left() + CELL_SIZE / 2.0,
            y: grid_top + GRID_PADDING + CELL_SIZE / 2.0,
            kind: MouseEventKind::Move,
        });
        handle_event(&mut state, &event);
        assert_eq!(state.hovered_emoji, Some(0));
    }

    #[test]
    fn click_emoji_records_selection() {
        let mut state = EmojiPickerState::new();
        let grid_top = state.grid_top();
        let event = Event::Mouse(MouseEvent {
            x: grid_left() + CELL_SIZE / 2.0,
            y: grid_top + GRID_PADDING + CELL_SIZE / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        handle_event(&mut state, &event);
        assert!(
            state.last_selected.is_some(),
            "clicking an emoji should set last_selected"
        );
    }

    #[test]
    fn skin_tone_click_changes_tone() {
        let mut state = EmojiPickerState::new();
        // The centre of the second circle, which is the Light skin tone.
        let event = Event::Mouse(MouseEvent {
            x: skin_tone_swatch_x(1) + SKIN_TONE_CIRCLE / 2.0,
            y: skin_tone_bar_y() + SKIN_TONE_HEIGHT / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        handle_event(&mut state, &event);
        assert_eq!(state.skin_tone, SkinToneModifier::Light);
    }

    // --- Where things are drawn is where clicks land ------------------------
    //
    // The renderer and the hit tests now read one set of layout functions, so
    // a tile cannot be painted anywhere a click does not follow. The cost of
    // that collapse is that a wrong layout function moves both together and no
    // test comparing them can see it -- so the checks below also measure the
    // *painted* geometry against the window, which shares no origin with it.

    /// The skin-tone swatch circles the picker painted, left to right, as
    /// `(x, y)`.
    fn painted_swatches(state: &EmojiPickerState) -> Vec<(f32, f32)> {
        render(state)
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                guitk::render::RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if (*width - SKIN_TONE_CIRCLE).abs() < 0.01
                    && (*height - SKIN_TONE_CIRCLE).abs() < 0.01 =>
                {
                    Some((*x, *y))
                }
                _ => Option::None,
            })
            .collect()
    }

    /// The tab icons the picker painted, left to right, as `(x, y)`.
    fn painted_tab_icons(state: &EmojiPickerState) -> Vec<(f32, f32)> {
        render(state)
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                guitk::render::RenderCommand::Text {
                    x, y, font_size, ..
                } if (*font_size - TAB_ICON_SIZE).abs() < 0.01 => Some((*x, *y)),
                _ => Option::None,
            })
            .collect()
    }

    /// Click at (`x`, `y`) and report the resulting skin tone.
    fn click_at(state: &mut EmojiPickerState, x: f32, y: f32) -> EventResult {
        handle_event(
            state,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        )
    }

    /// Every swatch is pressable at the place it was painted, including at the
    /// very edges of its cell -- a band is a rectangle, and a probe at its
    /// centre only tests a point.
    #[test]
    fn every_skin_tone_swatch_is_pressable_where_it_was_painted() {
        let swatches = painted_swatches(&EmojiPickerState::new());
        assert_eq!(swatches.len(), SkinToneModifier::ALL.len());

        let bar_mid = skin_tone_bar_y() + SKIN_TONE_HEIGHT / 2.0;
        for (i, &(x, _)) in swatches.iter().enumerate() {
            for probe in [
                x + 0.5,
                x + SKIN_TONE_CIRCLE / 2.0,
                x + SKIN_TONE_CIRCLE - 0.5,
            ] {
                let mut state = EmojiPickerState::new();
                click_at(&mut state, probe, bar_mid);
                assert_eq!(
                    Some(state.skin_tone),
                    SkinToneModifier::ALL.get(i).copied(),
                    "a click at x={probe} did not select swatch {i}"
                );
            }
        }
    }

    /// Sweep the whole width of the strip, a pixel at a time.
    ///
    /// Two claims at once. Between the circles nothing is dead -- the six-pixel
    /// gaps used to belong to nobody, because the hit band was the 18px circle
    /// rather than the 24px cell, so a click three pixels right of a swatch did
    /// nothing at all. And outside the run of swatches nothing is selected, so
    /// a click at the far right of the strip cannot wrap around onto one.
    #[test]
    fn the_swatch_strip_selects_the_nearest_swatch_and_nothing_beyond_them() {
        let swatches = painted_swatches(&EmojiPickerState::new());
        let bar_mid = skin_tone_bar_y() + SKIN_TONE_HEIGHT / 2.0;

        let mut x = 0.0;
        while x < WINDOW_WIDTH {
            let mut state = EmojiPickerState::new();
            // A tone no swatch would produce by accident of position, so
            // "unchanged" is distinguishable from "selected swatch 0".
            state.skin_tone = SkinToneModifier::MediumDark;
            let before = state.skin_tone;
            click_at(&mut state, x, bar_mid);

            // The cell a click falls in is the one whose painted circle it is
            // nearest to, centre to centre. The cell is half-open, so the pixel
            // exactly between two swatches belongs to the right-hand one.
            let chosen = swatches.iter().position(|&(sx, _)| {
                let from_centre = x - (sx + SKIN_TONE_CIRCLE / 2.0);
                from_centre >= -SKIN_TONE_PITCH / 2.0 && from_centre < SKIN_TONE_PITCH / 2.0
            });
            let expected = match chosen {
                Some(i) => SkinToneModifier::ALL.get(i).copied(),
                Option::None => Some(before),
            };
            assert_eq!(
                Some(state.skin_tone),
                expected,
                "a click at x={x} on the skin-tone strip"
            );
            x += 1.0;
        }
    }

    /// A click on the strip that is not on any swatch is still consumed: it
    /// landed on the popup, and reporting it as ignored would offer it to
    /// whatever is behind.
    #[test]
    fn a_click_on_the_strip_label_is_consumed_without_changing_the_tone() {
        let mut state = EmojiPickerState::new();
        state.skin_tone = SkinToneModifier::Medium;
        let result = click_at(&mut state, 4.0, skin_tone_bar_y() + SKIN_TONE_HEIGHT / 2.0);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.skin_tone, SkinToneModifier::Medium);
    }

    /// Measured against the window, which the layout functions know nothing
    /// about: the swatches march left to right without overlapping, clear of
    /// the label, and inside the strip they are drawn on.
    #[test]
    fn the_swatches_stand_apart_inside_the_strip() {
        let swatches = painted_swatches(&EmojiPickerState::new());
        let bar_y = skin_tone_bar_y();
        for pair in swatches.windows(2) {
            assert!(
                pair[1].0 >= pair[0].0 + SKIN_TONE_CIRCLE,
                "swatches at x={} and x={} overlap",
                pair[0].0,
                pair[1].0
            );
        }
        for &(x, y) in &swatches {
            assert!(x > 0.0, "a swatch is drawn off the left of the window");
            assert!(
                x + SKIN_TONE_CIRCLE <= WINDOW_WIDTH,
                "a swatch at x={x} runs off the right of the window"
            );
            assert!(
                y >= bar_y && y + SKIN_TONE_CIRCLE <= bar_y + SKIN_TONE_HEIGHT,
                "a swatch at y={y} is drawn outside the strip"
            );
        }
    }

    /// The grid is centred in the window.
    ///
    /// Measured from the painted cells against the window width, which is the
    /// one thing `grid_left` does not itself decide. Collapsing the paint and
    /// the hit test onto one layout function means a wrong `grid_left` moves
    /// both together and no test comparing them can see it -- so something with
    /// no shared origin has to hold it to account. Being *centred* is the
    /// claim: equal margins left and right.
    #[test]
    fn the_grid_columns_are_evenly_spaced_and_centred_in_the_window() {
        let state = every_emoji();
        let mut columns: Vec<f32> = render(&state)
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                guitk::render::RenderCommand::Text { x, font_size, .. }
                    if (*font_size - EMOJI_FONT_SIZE).abs() < 0.01 =>
                {
                    Some(*x - CELL_PADDING)
                }
                _ => Option::None,
            })
            .collect();
        columns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        columns.dedup_by(|a, b| (*a - *b).abs() < 0.01);

        assert_eq!(
            columns.len(),
            GRID_COLUMNS,
            "the grid painted {} distinct columns",
            columns.len()
        );
        for pair in columns.windows(2) {
            assert!(
                (pair[1] - pair[0] - CELL_SIZE).abs() < 0.01,
                "columns at x={} and x={} are not one cell apart",
                pair[0],
                pair[1]
            );
        }

        let left = columns[0];
        let right = columns[columns.len() - 1] + CELL_SIZE;
        assert!(
            left > 0.0 && right <= WINDOW_WIDTH,
            "the grid runs off the window"
        );
        assert!(
            (left - (WINDOW_WIDTH - right)).abs() < 0.01,
            "the grid has a {left}px margin on the left and {}px on the right",
            WINDOW_WIDTH - right
        );
    }

    /// Every tab is reachable at the place its icon was painted, and at both
    /// edges of its share of the bar.
    #[test]
    fn every_tab_is_reachable_across_its_whole_width() {
        let icons = painted_tab_icons(&EmojiPickerState::new());
        assert_eq!(
            icons.len(),
            TAB_COUNT,
            "the bar painted {} icons",
            icons.len()
        );

        let expected = tabs();
        for (i, &(icon_x, _)) in icons.iter().enumerate() {
            for probe in [icon_x, tab_x(i) + 0.5, tab_x(i) + tab_width() - 0.5] {
                let mut state = EmojiPickerState::new();
                click_at(&mut state, probe, TAB_BAR_HEIGHT / 2.0);
                assert_eq!(
                    Some(state.active_tab),
                    expected.get(i).copied(),
                    "a click at x={probe} did not select tab {i}"
                );
            }
        }
    }

    /// Measured against the window: the tab icons march left to right inside
    /// it, none of them overlapping and none hanging off an edge.
    #[test]
    fn the_tab_icons_stand_apart_inside_the_bar() {
        let icons = painted_tab_icons(&EmojiPickerState::new());
        for pair in icons.windows(2) {
            assert!(
                pair[1].0 > pair[0].0,
                "tab icons at x={} and x={} are out of order",
                pair[0].0,
                pair[1].0
            );
        }
        for &(x, y) in &icons {
            assert!(
                x >= 0.0 && x + TAB_ICON_SIZE <= WINDOW_WIDTH,
                "icon at x={x}"
            );
            assert!(
                y >= 0.0 && y + TAB_ICON_SIZE <= TAB_BAR_HEIGHT,
                "icon at y={y} is drawn outside the tab bar"
            );
        }
    }

    /// A float-to-integer cast saturates at zero, so `(x / tab_width) as usize`
    /// mapped every negative x onto tab 0. A click off the left edge of the
    /// window used to switch the picker to recently-used.
    #[test]
    fn a_click_left_of_the_tab_bar_selects_nothing() {
        let mut state = EmojiPickerState::new();
        let before = state.active_tab;
        click_at(&mut state, -5.0, TAB_BAR_HEIGHT / 2.0);
        assert_eq!(state.active_tab, before);
    }

    /// The grid stops exactly where the skin-tone strip starts.
    ///
    /// Checked between two renderers rather than against the layout function
    /// both of them read: `render_grid` clips to the grid area and
    /// `render_skin_tone_bar` fills the strip, and the clip's bottom edge has
    /// to be the strip's top edge or the grid either overlaps the strip or
    /// stops short of it.
    #[test]
    fn the_grid_is_clipped_to_exactly_where_the_strip_begins() {
        let state = EmojiPickerState::new();
        let tree = render(&state);

        let clip = tree.commands.iter().find_map(|cmd| match cmd {
            guitk::render::RenderCommand::PushClip { y, height, .. } => Some((*y, *height)),
            _ => Option::None,
        });
        let strip = tree.commands.iter().find_map(|cmd| match cmd {
            guitk::render::RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } if *x == 0.0
                && (*width - WINDOW_WIDTH).abs() < 0.01
                && (*height - SKIN_TONE_HEIGHT).abs() < 0.01 =>
            {
                Some(*y)
            }
            _ => Option::None,
        });

        let (clip_y, clip_h) = clip.expect("the grid never clipped to its area");
        let strip_y = strip.expect("the skin-tone strip was never painted");
        assert!(
            (clip_y + clip_h - strip_y).abs() < 0.01,
            "the grid is clipped to {}..{} but the strip starts at {strip_y}",
            clip_y,
            clip_y + clip_h
        );
    }

    /// The tab list and the width the bar divides itself into are two
    /// statements of how many tabs there are, and they have to agree.
    #[test]
    fn the_tab_count_matches_the_tab_list() {
        assert_eq!(tabs().len(), TAB_COUNT);
        assert!((tab_width() * TAB_COUNT as f32 - WINDOW_WIDTH).abs() < 0.01);
    }

    // --- Picker state ---

    // Exact: the constructor assigns the literal 0.0.
    #[allow(clippy::float_cmp)]
    #[test]
    fn initial_state_defaults() {
        let state = EmojiPickerState::new();
        assert!(state.is_open);
        assert_eq!(state.search_query, "");
        assert_eq!(state.scroll_offset, 0.0);
        assert_eq!(state.skin_tone, SkinToneModifier::None);
        assert!(state.hovered_emoji.is_none());
        assert!(state.last_selected.is_none());
    }

    #[test]
    fn visible_emoji_for_category() {
        let state = EmojiPickerState::new();
        let visible = state.visible_emoji();
        // Default tab is SmileysAndPeople.
        for e in &visible {
            assert_eq!(e.category, EmojiCategory::SmileysAndPeople);
        }
    }

    #[test]
    fn grid_content_height_is_positive() {
        let state = EmojiPickerState::new();
        assert!(state.grid_content_height() > 0.0);
    }

    // Exact: the clamp assigns the literal 0.0, with nothing to round.
    #[allow(clippy::float_cmp)]
    #[test]
    fn clamp_scroll_handles_negative() {
        let mut state = EmojiPickerState::new();
        state.scroll_offset = -100.0;
        state.clamp_scroll();
        assert_eq!(state.scroll_offset, 0.0);
    }

    #[test]
    fn clamp_scroll_handles_overflow() {
        let mut state = EmojiPickerState::new();
        state.scroll_offset = 100_000.0;
        state.clamp_scroll();
        assert!(state.scroll_offset <= state.max_scroll());
    }

    // --- Tab system ---

    #[test]
    fn tab_icons_are_non_empty() {
        let tabs = [
            Tab::Recent,
            Tab::Search,
            Tab::Category(EmojiCategory::Flags),
        ];
        for tab in &tabs {
            assert!(!tab.icon().is_empty());
        }
    }

    #[test]
    fn search_field_click_focuses() {
        let mut state = EmojiPickerState::new();
        assert!(!state.search_focused);
        let event = Event::Mouse(MouseEvent {
            x: WINDOW_WIDTH / 2.0,
            y: TAB_BAR_HEIGHT + SEARCH_HEIGHT / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        handle_event(&mut state, &event);
        assert!(state.search_focused);
    }
}

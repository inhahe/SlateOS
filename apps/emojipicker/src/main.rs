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
//! Opens as a real window, 360x480 to start with and resizable from there.
//! Uses the Catppuccin Mocha dark theme for all colors.
//!
//! The picker is drawn as a [`Frame`]: every clickable thing records the box it
//! was painted in, as it is painted, and the hit test reads those boxes back.
//! That matters here because the picker used to answer "where is this cell"
//! twice -- once in `render_grid` and once in `grid_hit_test` -- from the same
//! constants, which is agreement by coincidence rather than by construction.

use appearance::Palette;
use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text::{self, TextCursor};
use guitk::textedit;
use guitk::textinput::TextInput;
use guitk::{grid, wheel};
use oswindow::app::{self, App, Response};

use std::num::NonZeroUsize;
use std::process::ExitCode;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

/// Catppuccin Mocha dark theme colors.
mod mocha {

    // Pink, flamingo and rosewater are palette roles now, not constants.
    // They survived this crate's conversion for one reason: the shared
    // palette had no rung for them, which was true of five applications at
    // once and so was the palette's gap rather than this file's.
    // `0x89DCEB`. Was `0x89DCFE` — a transposed byte pair copied from
    // `gui/appearance`. See known-issues.md
    // TD-C-EVERY-APPLICATION-CARRIES-ITS-OWN-COPY-OF-THE-PALETTE-TOO.
}

// ============================================================================
// Constants
// ============================================================================

/// Width the popup opens at. Not the width it stays: the column count and
/// every band's position are derived from the size the window actually has.
const WINDOW_WIDTH: f32 = 360.0;
/// Height the popup opens at.
const WINDOW_HEIGHT: f32 = 480.0;
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
/// Where the search box's text starts, in from the box's left edge: past the
/// magnifying glass drawn there.
const SEARCH_TEXT_INSET: f32 = 28.0;
/// Room the search box's text leaves at the box's right edge.
const SEARCH_TEXT_RIGHT: f32 = 8.0;
/// The most the search box holds, in characters. A search for an emoji is a
/// word or two.
const SEARCH_CAPACITY: usize = 128;

/// The keys this window answers, as the F1 list shows them.
///
/// It had no list -- and no keys to put in one beyond Escape: the grid could
/// be used only with the pointer, so an emoji could not be chosen from the
/// keyboard at all.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F1 / ?", "This list"),
    ("Arrows", "Move through the emoji"),
    ("PageUp / PageDown", "A page at a time"),
    ("Home / End", "The first or the last"),
    ("Enter / Space", "Pick the lit emoji"),
    ("Ctrl+Tab / Ctrl+Shift+Tab", "The next or the last tab"),
    ("Ctrl+F / Tab", "To the search, or back to the emoji"),
    ("Escape", "Close the picker"),
];

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
// Every number deciding *where* something is drawn is worked out here, once per
// frame, from the size the window actually has. Nothing remembers it: a
// `Layout` is built, drawn from, hit-tested through, and dropped.
//
// Two things used to be wrong with that.
//
// The geometry was fixed. `WINDOW_WIDTH` and `WINDOW_HEIGHT` appeared in
// nineteen expressions between them, so the picker drew a 360x480 popup into
// whatever window it was given. Nothing caught it, because there was no window:
// `main` rendered one tree, took its length, and exited.
//
// And it was written down twice. `grid_hit_test` re-derived the cell grid from
// the same constants `render_grid` used, so the two agreed for exactly as long
// as somebody kept them equal by hand. [`Frame`] removes the second copy: the
// renderer records the box it painted for each control as it paints it, and the
// hit test reads those boxes back. A cell scrolled out of the viewport is
// dropped from the record rather than staying clickable, which the arithmetic
// version had no way to express at all.

/// A clickable thing in the picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Tab `index` of the bar, numbered as [`tabs`] orders them.
    Tab(usize),
    /// The search field.
    SearchField,
    /// Grid cell `index`, into the current [`EmojiPickerState::visible_emoji`].
    Cell(usize),
    /// Skin-tone swatch `index`, into [`SkinToneModifier::ALL`].
    Swatch(usize),
}

/// One frame of this app's drawing, carrying the boxes it recorded.
pub type Frame = guitk::frame::Frame<Target>;

/// Distance between one skin-tone swatch's left edge and the next one's.
const SKIN_TONE_PITCH: f32 = SKIN_TONE_CIRCLE + SKIN_TONE_SPACING;

/// Left edge of the first skin-tone swatch when the window is wide enough for
/// it, clear of the "Skin tone:" label.
const SKIN_TONE_FIRST_X: f32 = 80.0;

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

/// Whether a band `want` tall can be taken out of `top..bottom` and still leave
/// a row of emoji behind.
///
/// This is the whole shrinking policy. The grid is what the picker *is*, so it
/// is never the band that gives way: a strip or a preview that would squeeze it
/// below one clickable row is dropped outright instead, hit box and all. A
/// picker with a two-pixel grid is not a smaller picker, it is a broken one.
fn band_fits(top: f32, bottom: f32, want: f32) -> bool {
    bottom - top - want >= CELL_SIZE
}

/// Where everything goes, for one window size.
///
/// Built fresh on every frame and never stored. A layout kept in a field is a
/// second opinion about the window that is right until the window is resized.
pub struct Layout {
    /// The whole window.
    pub window: Rect,
    /// The tab bar across the top.
    pub tab_bar: Rect,
    /// The band the search field sits in, or `None` in a window too short.
    pub search_row: Option<Rect>,
    /// The scrolling grid's viewport.
    pub grid: Rect,
    /// Left edge of the first column, in window coordinates.
    pub grid_left: f32,
    /// How many columns of emoji the grid's width holds.
    pub columns: NonZeroUsize,
    /// The skin-tone strip, or `None` in a window too short.
    pub strip: Option<Rect>,
    /// The preview band, or `None` in a window too short.
    pub preview: Option<Rect>,
}

impl Layout {
    /// Work out where everything goes in a window of `width` by `height`.
    pub fn new(width: f32, height: f32) -> Self {
        let width = width.max(0.0);
        let height = height.max(0.0);
        let window = Rect::new(0.0, 0.0, width, height);

        let tab_bar = Rect::new(0.0, 0.0, width, TAB_BAR_HEIGHT.min(height));
        let mut top = tab_bar.bottom();
        let mut bottom = height;

        let search_row = if band_fits(top, bottom, SEARCH_HEIGHT) {
            let row = Rect::new(0.0, top, width, SEARCH_HEIGHT);
            top = row.bottom();
            Some(row)
        } else {
            Option::None
        };

        // The bottom two bands, in the order they are drawn: the preview sits
        // under the skin-tone strip. When only one of them fits, the preview is
        // the one that goes -- it names an emoji that is already on screen a row
        // above, whereas the strip is the only place a skin tone can be chosen
        // at all.
        let (strip, preview) = if band_fits(top, bottom, SKIN_TONE_HEIGHT + PREVIEW_HEIGHT) {
            let preview = Rect::new(0.0, bottom - PREVIEW_HEIGHT, width, PREVIEW_HEIGHT);
            let strip = Rect::new(0.0, preview.y - SKIN_TONE_HEIGHT, width, SKIN_TONE_HEIGHT);
            bottom = strip.y;
            (Some(strip), Some(preview))
        } else if band_fits(top, bottom, SKIN_TONE_HEIGHT) {
            let strip = Rect::new(0.0, bottom - SKIN_TONE_HEIGHT, width, SKIN_TONE_HEIGHT);
            bottom = strip.y;
            (Some(strip), Option::None)
        } else {
            (Option::None, Option::None)
        };

        let grid = Rect::new(0.0, top, width, (bottom - top).max(0.0));
        // No gap: the cells are 48px boxes drawn edge to edge, and the padding
        // that separates the glyphs is inside them.
        let columns = grid::columns_across(grid.w, CELL_SIZE, 0.0);
        let span = columns.get() as f32 * CELL_SIZE;
        let grid_left = ((grid.w - span) / 2.0).max(0.0);

        Self {
            window,
            tab_bar,
            search_row,
            grid,
            grid_left,
            columns,
            strip,
            preview,
        }
    }

    /// Width of one tab: the bar divides the window evenly.
    pub fn tab_width(&self) -> f32 {
        self.window.w / TAB_COUNT as f32
    }

    /// Left edge of tab `index`.
    pub fn tab_x(&self, index: usize) -> f32 {
        index as f32 * self.tab_width()
    }

    /// The box a click on tab `index` falls in.
    pub fn tab_cell(&self, index: usize) -> Rect {
        Rect::new(
            self.tab_x(index),
            self.tab_bar.y,
            self.tab_width(),
            self.tab_bar.h,
        )
    }

    /// The rounded box drawn inside the search row, inset from it.
    pub fn search_field(&self) -> Option<Rect> {
        let row = self.search_row?;
        let margin = 8.0;
        Some(Rect::new(
            row.x + margin,
            row.y + 4.0,
            (row.w - margin * 2.0).max(0.0),
            (row.h - 8.0).max(0.0),
        ))
    }

    /// Left edge of the first skin-tone swatch.
    ///
    /// Normally clear of the "Skin tone:" label. In a window too narrow for
    /// both, the swatches win and the label is what gets covered: a label you
    /// could read on a wider window is worth less than six controls you can
    /// press on this one.
    fn swatch_first_x(&self) -> f32 {
        let run = SKIN_TONE_PITCH * SkinToneModifier::ALL.len() as f32 - SKIN_TONE_SPACING;
        let latest = (self.window.w - run - 4.0).max(4.0);
        SKIN_TONE_FIRST_X.min(latest)
    }

    /// The circle drawn for swatch `index`.
    pub fn swatch(&self, index: usize) -> Option<Rect> {
        let strip = self.strip?;
        Some(Rect::new(
            self.swatch_first_x() + index as f32 * SKIN_TONE_PITCH,
            strip.y + (strip.h - SKIN_TONE_CIRCLE) / 2.0,
            SKIN_TONE_CIRCLE,
            SKIN_TONE_CIRCLE,
        ))
    }

    /// The box a click selecting swatch `index` may fall in.
    ///
    /// The whole pitch and the whole height of the strip, not just the 18px
    /// circle. The six-pixel gaps between the circles used to belong to nobody,
    /// so a click three pixels right of a swatch did nothing at all -- a dead
    /// band a quarter as wide as the swatches themselves.
    pub fn swatch_cell(&self, index: usize) -> Option<Rect> {
        let strip = self.strip?;
        let circle = self.swatch(index)?;
        Some(Rect::new(
            circle.x - SKIN_TONE_SPACING / 2.0,
            strip.y,
            SKIN_TONE_PITCH,
            strip.h,
        ))
    }

    /// The cell for grid entry `index`, in the grid's own unscrolled space --
    /// which is what both the draw commands and [`Frame::hit`] take.
    pub fn cell(&self, index: usize) -> Rect {
        let cols = self.columns.get();
        let col = index.checked_rem(cols).unwrap_or(0);
        let row = index.checked_div(cols).unwrap_or(0);
        Rect::new(
            self.grid_left + col as f32 * CELL_SIZE,
            self.grid.y + GRID_PADDING + row as f32 * CELL_SIZE,
            CELL_SIZE,
            CELL_SIZE,
        )
    }

    /// How tall `count` emoji are when laid out across this grid's columns.
    pub fn content_height(&self, count: usize) -> f32 {
        let rows = count.div_ceil(self.columns.get());
        rows as f32 * CELL_SIZE + GRID_PADDING * 2.0
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
    /// The search box's editor -- its caret and selection over
    /// `search_query` -- reloaded when the query changed under it.
    search_editor: TextInput,
    /// What the search box's Ctrl+C and Ctrl+X took, for its Ctrl+V.
    search_clipboard: String,
    /// Index of the emoji lit in the visible grid, if any: the one the
    /// pointer is over, or the one the arrow keys moved to -- the same light,
    /// so the preview names whichever the reader last pointed at, and Enter
    /// picks it.
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
    /// Whether the pointer is over the search field's band: its edge is
    /// warmed towards the accent then, as every field's is.
    search_hovered: bool,
    /// The user's focus width, which the search field's focus mark is
    /// drawn at (`App::appearance_changed`).
    focus_ring_width: f32,
    /// Whether the list of keys is up.
    pub show_help: bool,
    /// Width of the window being drawn into.
    pub width: f32,
    /// Height of the window being drawn into.
    pub height: f32,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

impl EmojiPickerState {
    /// Create a new picker state with an initialized database.
    pub fn new() -> Self {
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            active_tab: Tab::Category(EmojiCategory::SmileysAndPeople),
            selected_category: EmojiCategory::SmileysAndPeople,
            search_query: String::new(),
            search_editor: TextInput::new(),
            search_clipboard: String::new(),
            hovered_emoji: Option::None,
            scroll_offset: 0.0,
            skin_tone: SkinToneModifier::None,
            database: EmojiDatabase::new(),
            is_open: true,
            last_selected: Option::None,
            search_focused: false,
            search_hovered: false,
            focus_ring_width: guitk::style::FOCUS_RING_WIDTH,
            show_help: false,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    /// Adopt a new window size.
    ///
    /// The scroll is re-clamped rather than left alone: growing the window
    /// shortens the scrollable range, and an offset from the old range would
    /// leave the grid parked past its own last row.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
        self.clamp_scroll();
    }

    /// Where everything goes at the current window size.
    pub fn layout(&self) -> Layout {
        Layout::new(self.width, self.height)
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
        self.layout().content_height(self.visible_emoji().len())
    }

    /// The height available for the scrollable grid area.
    pub fn grid_area_height(&self) -> f32 {
        self.layout().grid.h
    }

    /// Maximum scroll offset (zero when the content fits).
    pub fn max_scroll(&self) -> f32 {
        (self.grid_content_height() - self.grid_area_height()).max(0.0)
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

    /// Record a pick of the visible entry at `index`.
    ///
    /// The database remembers the *untinted* emoji and `last_selected` carries
    /// the tinted one: recently-used should re-tint when the tone changes
    /// rather than freeze a row at whatever tone was active when it was picked,
    /// but what leaves the picker is what the grid showed.
    fn pick(&mut self, index: usize) {
        let Some(base) = self.visible_emoji().get(index).map(|e| e.emoji.clone()) else {
            return;
        };
        let modified = self.skin_tone.apply(&base);
        self.database.record_use(&base);
        self.last_selected = Some(modified);
    }

    /// Switch to tab `index` of the bar.
    ///
    /// The light goes out: it was an index into the tab being left.
    fn select_tab(&mut self, index: usize) {
        let Some(&tab) = tabs().get(index) else {
            return;
        };
        self.active_tab = tab;
        self.scroll_offset = 0.0;
        self.hovered_emoji = Option::None;
        if let Tab::Category(cat) = tab {
            self.selected_category = cat;
        }
        self.search_focused = tab == Tab::Search;
        if self.search_focused {
            self.search_editor.set_text(&self.search_query);
        }
    }

    /// Ctrl+Tab: the next tab of the bar, after the last the first; with
    /// Shift, the one before.
    fn step_tab(&mut self, forward: bool) {
        let all = tabs();
        let here = all.iter().position(|t| *t == self.active_tab).unwrap_or(0);
        // Back one is on one short of a lap, which keeps the wrap unsigned.
        let step = if forward {
            1
        } else {
            all.len().saturating_sub(1)
        };
        if let Some(next) = here.saturating_add(step).checked_rem(all.len()) {
            self.select_tab(next);
        }
    }

    /// Light the emoji at `index` -- the last, past the end -- and scroll it
    /// into view. With nothing shown, nothing is lit.
    fn light(&mut self, index: usize) {
        let Some(last) = self.visible_emoji().len().checked_sub(1) else {
            self.hovered_emoji = Option::None;
            return;
        };
        let at = index.min(last);
        self.hovered_emoji = Some(at);
        self.reveal(at);
    }

    /// The keyboard's move through the grid: `delta` emoji on, a row being as
    /// many as the grid is wide, stopping at either end. From nothing lit,
    /// the first.
    ///
    /// The grid answered only the pointer: no key lit an emoji or picked one.
    fn move_highlight(&mut self, delta: isize) {
        let next = match self.hovered_emoji {
            Option::None => 0,
            Some(at) if delta < 0 => at.saturating_sub(delta.unsigned_abs()),
            Some(at) => at.saturating_add(delta.unsigned_abs()),
        };
        self.light(next);
    }

    /// Scroll the grid just far enough that the cell at `index` is wholly in
    /// view, with the grid's padding around it -- so the first row scrolls to
    /// the very top and the last to the very bottom, as the wheel leaves them.
    fn reveal(&mut self, index: usize) {
        let layout = self.layout();
        let cell = layout.cell(index);
        let (top, bottom) = (cell.y - GRID_PADDING, cell.bottom() + GRID_PADDING);
        if top - self.scroll_offset < layout.grid.y {
            self.scroll_offset = top - layout.grid.y;
        } else if bottom - self.scroll_offset > layout.grid.bottom() {
            self.scroll_offset = bottom - layout.grid.bottom();
        }
        self.clamp_scroll();
    }

    /// How many emoji one page is: the whole rows in view, and at least one.
    fn page_cells(&self) -> isize {
        let layout = self.layout();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a count of rows on a screen, floored and at least one"
        )]
        let rows = (layout.grid.h / CELL_SIZE).floor().max(1.0) as isize;
        rows.saturating_mul(isize::try_from(layout.columns.get()).unwrap_or(1))
    }

    /// Enter: pick the lit emoji -- or, with none lit, the first one shown, so
    /// a word typed and Enter picks the best match.
    fn pick_lit(&mut self) {
        self.pick(self.hovered_emoji.unwrap_or(0));
    }

    /// Give the search box the keyboard, its caret after the query.
    fn focus_search(&mut self) {
        self.search_focused = true;
        self.search_editor.set_text(&self.search_query);
    }

    /// Where the search box's caret is drawn: the editor's while the box has
    /// the keyboard (the end, if the query changed under the editor), the
    /// start otherwise. One answer for the drawing and for a press.
    fn search_cursor(&self) -> TextCursor {
        if !self.search_focused {
            TextCursor::default()
        } else if self.search_editor.text() == self.search_query {
            self.search_editor.cursor()
        } else {
            TextCursor::from(self.search_query.len())
        }
    }

    /// A key for the search box, which has the keyboard: the caret keys,
    /// Backspace and Delete at the caret, Ctrl+A, C, X and V, and typing --
    /// AltGr's among it, and no command's letter. A query that changed shows
    /// the search tab from its top, with nothing lit.
    ///
    /// The box took typing at its end and Backspace from it, and nothing else.
    fn search_key(&mut self, key: &KeyEvent) -> EventResult {
        if self.search_editor.text() != self.search_query {
            self.search_editor.set_text(&self.search_query);
        }
        let edit = textline::apply_key(
            &mut self.search_editor,
            key,
            SEARCH_CAPACITY,
            &self.search_clipboard,
            LABEL_FONT_SIZE,
        );
        if let Some(copied) = edit.copied {
            self.search_clipboard = copied;
        }
        if self.search_editor.text() != self.search_query {
            self.search_query = self.search_editor.text().to_owned();
            if !self.search_query.is_empty() {
                self.active_tab = Tab::Search;
            }
            self.scroll_offset = 0.0;
            self.hovered_emoji = Option::None;
        }
        if edit.handled {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    /// A press in the search box at `x`: it takes the keyboard, with the caret
    /// under the pointer, measured against the box as it was drawn.
    fn press_search(&mut self, x: f32) {
        let Some(field) = self.layout().search_field() else {
            return;
        };
        let drawn = self.search_cursor();
        self.search_focused = true;
        if self.search_editor.text() != self.search_query {
            self.search_editor.set_text(&self.search_query);
        }
        let cursor = textedit::cursor_at_click(
            &self.search_query,
            drawn,
            (field.w - SEARCH_TEXT_INSET - SEARCH_TEXT_RIGHT).max(0.0),
            LABEL_FONT_SIZE,
            FontWeightHint::Regular,
            x - field.x - SEARCH_TEXT_INSET,
        );
        self.search_editor.set_selection_anchor(Option::None);
        self.search_editor.set_cursor(cursor);
    }

    /// The control under `(x, y)`, or `None` for bare background.
    ///
    /// Answered by drawing the frame and reading its boxes back, so there is no
    /// arithmetic here that could disagree with the arithmetic in the renderer.
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    /// Draw the picker, recording a hit box for every control as it is painted.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let mut frame = Frame::new(width, height);
        if !self.is_open {
            return frame;
        }
        let layout = Layout::new(width, height);

        fill(&mut frame, layout.window, self.palette.base, CORNER_RADIUS);
        self.draw_tab_bar(&mut frame, &layout);
        self.draw_search_field(&mut frame, &layout);
        self.draw_grid(&mut frame, &layout);
        self.draw_skin_tone_bar(&mut frame, &layout);
        self.draw_preview(&mut frame, &layout);
        // The list of keys over everything: it is the one thing on screen a
        // reader asked for explicitly.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut frame,
                &self.palette,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 or ? closes this",
            );
        }
        frame
    }

    /// The category tab bar.
    fn draw_tab_bar(&self, frame: &mut Frame, layout: &Layout) {
        fill(frame, layout.tab_bar, self.palette.mantle, 0.0);
        let tab_w = layout.tab_width();

        for (i, &tab) in tabs().iter().enumerate() {
            let cell = layout.tab_cell(i);
            let is_active = tab == self.active_tab;

            if is_active {
                fill(
                    frame,
                    Rect::new(
                        cell.x + 2.0,
                        cell.y + 2.0,
                        (cell.w - 4.0).max(0.0),
                        (cell.h - 4.0).max(0.0),
                    ),
                    self.palette.surface0,
                    6.0,
                );
            }

            label(
                frame,
                cell.x + (tab_w - TAB_ICON_SIZE) / 2.0,
                cell.y + (cell.h - TAB_ICON_SIZE) / 2.0,
                tab.icon(),
                TAB_ICON_SIZE,
                if is_active {
                    self.palette.blue
                } else {
                    self.palette.overlay0
                },
                FontWeightHint::Regular,
                Some(tab_w),
            );
            frame.hit(Target::Tab(i), cell);
        }

        frame.push(RenderCommand::Line {
            x1: 0.0,
            y1: layout.tab_bar.bottom(),
            x2: layout.window.w,
            y2: layout.tab_bar.bottom(),
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    /// The search input field.
    fn draw_search_field(&self, frame: &mut Frame, layout: &Layout) {
        let (Some(row), Some(field)) = (layout.search_row, layout.search_field()) else {
            return;
        };

        // The toolkit's field, in the theme's shape (lane C,
        // c-e-a-theme-can-shape-the-controls): its well, its edge and, while
        // it has the keyboard, its focus mark at the user's width.
        guitk::field::draw(
            frame,
            &self.palette,
            field,
            guitk::field::State {
                hovered: self.search_hovered,
                focused: self.search_focused && !self.show_help,
                disabled: false,
                invalid: false,
            },
            self.focus_ring_width,
        );

        let text_y = field.y + (field.h - LABEL_FONT_SIZE) / 2.0;
        label(
            frame,
            field.x + 8.0,
            text_y,
            "\u{1F50D}",
            LABEL_FONT_SIZE,
            self.palette.overlay0,
            FontWeightHint::Regular,
            Option::None,
        );

        // The query, or what the box is for, faint, while it is empty -- and,
        // with the keyboard, the caret and the selection where they are. An
        // empty box with the keyboard draws its caret before what it is for,
        // as the toolkit's own input dialog does. It drew neither: the caret
        // was nowhere, because there was no caret to move.
        let line = text::line_height(LABEL_FONT_SIZE, FontWeightHint::Regular);
        let (x, y, avail) = (
            field.x + SEARCH_TEXT_INSET,
            field.y + (field.h - line) / 2.0,
            (field.w - SEARCH_TEXT_INSET - SEARCH_TEXT_RIGHT).max(0.0),
        );
        let focused = self.search_focused && !self.show_help;
        if self.search_query.is_empty() {
            label(
                frame,
                x,
                y,
                "Search emoji...",
                LABEL_FONT_SIZE,
                self.palette.subtext0,
                FontWeightHint::Regular,
                Some(avail),
            );
            if focused {
                let mut tree = RenderTree::new();
                textedit::push_caret(
                    &mut tree,
                    x,
                    y,
                    line,
                    self.palette.text,
                    textedit::CARET_WIDTH,
                );
                for cmd in tree.commands {
                    frame.push(cmd);
                }
            }
        } else {
            let editing = self.search_focused && self.search_editor.text() == self.search_query;
            let mut tree = RenderTree::new();
            textedit::draw(
                &mut tree,
                &textedit::SingleLine {
                    text: &self.search_query,
                    cursor: self.search_cursor(),
                    selection_anchor: if editing {
                        self.search_editor.selection_anchor()
                    } else {
                        Option::None
                    },
                    focused,
                    x,
                    y,
                    width: avail,
                    line_height: line,
                    font_size: LABEL_FONT_SIZE,
                    weight: FontWeightHint::Regular,
                    color: self.palette.text,
                    selection_bg: self.palette.accent,
                    selection_fg: self.palette.crust,
                    caret_width: textedit::CARET_WIDTH,
                },
            );
            for cmd in tree.commands {
                frame.push(cmd);
            }
        }

        // The whole band, not the rounded box drawn inside it: the eight pixels
        // of margin around the field look like part of it, and a click there
        // that un-focused the field instead of focusing it would read as the
        // field refusing to take focus.
        frame.hit(Target::SearchField, row);
    }

    /// The scrollable emoji grid.
    fn draw_grid(&self, frame: &mut Frame, layout: &Layout) {
        let list = self.visible_emoji();

        frame.clip(layout.grid);
        frame.translate(0.0, -self.scroll_offset);

        for (i, entry) in list.iter().enumerate() {
            let cell = layout.cell(i);

            if self.hovered_emoji == Some(i) {
                fill(
                    frame,
                    Rect::new(cell.x + 1.0, cell.y + 1.0, cell.w - 2.0, cell.h - 2.0),
                    self.palette.surface1,
                    6.0,
                );
            }

            // The glyph wears the chosen skin tone if it can take one. Drawing
            // the untinted emoji here and the tinted one only in the preview
            // would make the grid disagree with what a click actually yields.
            label(
                frame,
                cell.x + CELL_PADDING,
                cell.y + CELL_PADDING,
                &self.skin_tone.apply(&entry.emoji),
                EMOJI_FONT_SIZE,
                self.palette.text,
                FontWeightHint::Regular,
                Some(CELL_SIZE - CELL_PADDING * 2.0),
            );

            // Recorded inside the clip and the translation, so a cell scrolled
            // out of the viewport is dropped rather than left clickable behind
            // the tab bar.
            frame.hit(Target::Cell(i), cell);
        }

        frame.untranslate();
        frame.unclip();
    }

    /// The skin tone selector strip.
    fn draw_skin_tone_bar(&self, frame: &mut Frame, layout: &Layout) {
        let Some(strip) = layout.strip else {
            return;
        };
        fill(frame, strip, self.palette.mantle, 0.0);
        label(
            frame,
            strip.x + 8.0,
            strip.y + (strip.h - LABEL_FONT_SIZE) / 2.0,
            "Skin tone:",
            LABEL_FONT_SIZE - 1.0,
            self.palette.subtext0,
            FontWeightHint::Regular,
            Option::None,
        );

        for (i, &tone) in SkinToneModifier::ALL.iter().enumerate() {
            let Some(circle) = layout.swatch(i) else {
                continue;
            };
            fill(frame, circle, tone.swatch_color(), SKIN_TONE_CIRCLE / 2.0);

            // Selection ring: the swatch outset by 2px on every side, with a
            // corner radius of half its own width so it draws as a circle.
            if self.skin_tone == tone {
                let ring = Rect::new(
                    circle.x - 2.0,
                    circle.y - 2.0,
                    circle.w + 4.0,
                    circle.h + 4.0,
                );
                stroke(frame, ring, self.palette.blue, 2.0, ring.w / 2.0);
            }

            if let Some(cell) = layout.swatch_cell(i) {
                frame.hit(Target::Swatch(i), cell);
            }
        }
    }

    /// The preview area at the bottom of the popup.
    fn draw_preview(&self, frame: &mut Frame, layout: &Layout) {
        let Some(preview) = layout.preview else {
            return;
        };
        fill_radii(
            frame,
            preview,
            self.palette.crust,
            CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: CORNER_RADIUS,
                bottom_right: CORNER_RADIUS,
            },
        );

        let list = self.visible_emoji();
        let hovered = self.hovered_emoji.and_then(|idx| list.get(idx).copied());
        let text_width = (preview.w - 64.0).max(0.0);

        match hovered {
            Some(entry) => {
                label(
                    frame,
                    preview.x + 12.0,
                    preview.y + 10.0,
                    &self.skin_tone.apply(&entry.emoji),
                    PREVIEW_EMOJI_SIZE,
                    self.palette.text,
                    FontWeightHint::Regular,
                    Option::None,
                );
                label(
                    frame,
                    preview.x + 56.0,
                    preview.y + 12.0,
                    &entry.name,
                    LABEL_FONT_SIZE,
                    self.palette.subtext1,
                    FontWeightHint::Bold,
                    Some(text_width),
                );
                label(
                    frame,
                    preview.x + 56.0,
                    preview.y + 30.0,
                    entry.category.label(),
                    LABEL_FONT_SIZE - 2.0,
                    self.palette.overlay0,
                    FontWeightHint::Regular,
                    Some(text_width),
                );
            }
            Option::None => {
                label(
                    frame,
                    preview.x + 12.0,
                    preview.y + (preview.h - LABEL_FONT_SIZE) / 2.0,
                    // The arrows light one too, now: the hint said only the
                    // pointer could.
                    "Point at an emoji, or move to one with the arrows",
                    LABEL_FONT_SIZE,
                    self.palette.overlay0,
                    FontWeightHint::Regular,
                    Some((preview.w - 24.0).max(0.0)),
                );
            }
        }
    }
}

impl Default for EmojiPickerState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Drawing helpers
// ============================================================================

/// A filled rectangle with per-corner radii.
fn fill_radii(frame: &mut Frame, rect: Rect, color: Color, corner_radii: CornerRadii) {
    if rect.is_empty() {
        return;
    }
    frame.push(RenderCommand::FillRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color,
        corner_radii,
    });
}

/// A filled rectangle with the same radius on every corner.
fn fill(frame: &mut Frame, rect: Rect, color: Color, radius: f32) {
    fill_radii(frame, rect, color, CornerRadii::all(radius));
}

/// An outlined rectangle.
fn stroke(frame: &mut Frame, rect: Rect, color: Color, line_width: f32, radius: f32) {
    if rect.is_empty() {
        return;
    }
    frame.push(RenderCommand::StrokeRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color,
        line_width,
        corner_radii: CornerRadii::all(radius),
    });
}

/// A line of text, elided rather than overrun when a width is given.
fn label(
    frame: &mut Frame,
    x: f32,
    y: f32,
    text: &str,
    font_size: f32,
    color: Color,
    font_weight: FontWeightHint,
    max_width: Option<f32>,
) {
    if max_width.is_some_and(|w| w <= 0.0) {
        return;
    }
    frame.push(RenderCommand::Text {
        x,
        y,
        text: text.to_string(),
        color,
        font_size,
        font_weight,
        max_width,
        overflow: if max_width.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

// ============================================================================
// Event handling
// ============================================================================

/// Handle an input event, returning whether it was consumed.
///
/// A free function so that [`App::on_event`] and [`Probe::click_at`] are both
/// thin adapters over the same body rather than two dispatchers that have to be
/// kept in step -- which is the arrangement where a test passes against a code
/// path the window never takes.
pub fn handle_event(state: &mut EmojiPickerState, event: &Event) -> EventResult {
    match event {
        Event::Key(key_ev) if key_ev.pressed => handle_key(state, key_ev),
        Event::Mouse(mouse_ev) => handle_mouse(state, mouse_ev),
        Event::Resize { width, height } => {
            state.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// Process a keyboard event.
///
/// The grid answered only the pointer: no key lit an emoji or picked one, so
/// an emoji could not be chosen from the keyboard at all. The arrows, the page
/// keys, Home and End light one now; Enter or Space picks it.
fn handle_key(state: &mut EmojiPickerState, key: &KeyEvent) -> EventResult {
    // The list of keys first: it is drawn over everything, and modal while it
    // is up -- a plain F1, `?` or Escape puts it away, and no other key
    // reaches what it covers, where Escape would close the picker.
    let plain = textline::is_plain(key.modifiers);
    let question = key.key == Key::Slash && key.modifiers.shift;
    if state.show_help {
        if plain && (matches!(key.key, Key::F1 | Key::Escape) || question) {
            state.show_help = false;
        }
        return EventResult::Consumed;
    }
    // F1 raises it from anywhere, the search box included, because it is
    // never typed; `?` is typed in the box, so it raises the list only from
    // outside it.
    if plain && (key.key == Key::F1 || question && !state.search_focused) {
        state.show_help = true;
        return EventResult::Consumed;
    }

    // The picker's own chords, from the box or the grid. Ctrl chords, not
    // Ctrl held: AltGr arrives as Ctrl+Alt, and types.
    let ctrl = textline::is_ctrl_chord(key.modifiers);
    if ctrl && key.key == Key::Tab {
        state.step_tab(!key.modifiers.shift);
        return EventResult::Consumed;
    }
    if ctrl && key.key == Key::F {
        state.focus_search();
        return EventResult::Consumed;
    }

    // The keys that mean the same from the box and from the grid: a word
    // typed, then Down and Enter, picks from what it found.
    if plain {
        let columns = isize::try_from(state.layout().columns.get()).unwrap_or(1);
        let delta = match key.key {
            Key::Escape => {
                state.is_open = false;
                return EventResult::Consumed;
            }
            Key::Enter => {
                state.pick_lit();
                return EventResult::Consumed;
            }
            // Between the box and the grid, which it lights if nothing is.
            Key::Tab => {
                if state.search_focused {
                    state.search_focused = false;
                    if state.hovered_emoji.is_none() {
                        state.light(0);
                    }
                } else {
                    state.focus_search();
                }
                return EventResult::Consumed;
            }
            Key::Up => Some(columns.saturating_neg()),
            Key::Down => Some(columns),
            Key::PageUp => Some(state.page_cells().saturating_neg()),
            Key::PageDown => Some(state.page_cells()),
            _ => Option::None,
        };
        if let Some(delta) = delta {
            state.move_highlight(delta);
            return EventResult::Consumed;
        }
    }

    // Every other key is the box's while it has the keyboard.
    if state.search_focused {
        return state.search_key(key);
    }
    // The grid's own keys, taken plain: a chord with Alt or the Windows key is
    // the window manager's or the desktop's.
    if !plain {
        return EventResult::Ignored;
    }
    match key.key {
        Key::Left => state.move_highlight(-1),
        Key::Right => state.move_highlight(1),
        Key::Home => state.light(0),
        Key::End => state.light(usize::MAX),
        Key::Space => state.pick_lit(),
        _ => return EventResult::Ignored,
    }
    EventResult::Consumed
}

/// Process a mouse event.
fn handle_mouse(state: &mut EmojiPickerState, mouse: &MouseEvent) -> EventResult {
    let (x, y) = (mouse.x, mouse.y);

    // The list of keys is modal for the pointer as it is for the keys: a press
    // with any button puts it away and does nothing else -- it picks no emoji
    // under it -- and the wheel scrolls nothing it covers.
    if state.show_help {
        match mouse.kind {
            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {
                state.show_help = false;
                return EventResult::Consumed;
            }
            MouseEventKind::Scroll { .. } => return EventResult::Ignored,
            _ => {}
        }
    }

    match &mouse.kind {
        MouseEventKind::Press(MouseButton::Left) => {
            match state.target_at(x, y) {
                Some(Target::Tab(i)) => state.select_tab(i),
                Some(Target::SearchField) => state.press_search(x),
                Some(Target::Cell(i)) => {
                    state.search_focused = false;
                    state.pick(i);
                }
                Some(Target::Swatch(i)) => {
                    if let Some(&tone) = SkinToneModifier::ALL.get(i) {
                        state.skin_tone = tone;
                    }
                }
                // Consumed either way: the click landed on the popup, and
                // reporting it as ignored would offer it to whatever is behind.
                Option::None => state.search_focused = false,
            }
            EventResult::Consumed
        }

        MouseEventKind::Move => {
            let target = state.target_at(x, y);
            state.hovered_emoji = match target {
                Some(Target::Cell(i)) => Some(i),
                _ => Option::None,
            };
            state.search_hovered = matches!(target, Some(Target::SearchField));
            EventResult::Consumed
        }

        // Nothing is under a pointer that has left: a cell or the field
        // left lit would say otherwise until the pointer came back.
        MouseEventKind::Leave => {
            state.hovered_emoji = Option::None;
            state.search_hovered = false;
            EventResult::Consumed
        }

        MouseEventKind::Scroll { dy, .. } => {
            if state.layout().grid.contains(x, y) {
                // `dy` is a notch count, not a distance. Subtracting it raw
                // moved the grid one pixel per notch -- a 48px cell took 48
                // notches to clear, so the wheel looked broken.
                state.scroll_offset += wheel::pixels(*dy, CELL_SIZE);
                state.clamp_scroll();
                return EventResult::Consumed;
            }
            EventResult::Ignored
        }

        _ => EventResult::Ignored,
    }
}

// ============================================================================
// Window
// ============================================================================

impl App for EmojiPickerState {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn appearance_changed(&mut self, settings: &appearance::AppearanceSettings) {
        self.focus_ring_width = settings.focus_ring_width();
    }

    fn title(&self) -> String {
        String::from("Emoji Picker")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// No tick.
    ///
    /// Nothing here moves on its own: the emoji set is a compiled-in table.
    /// A timer would repaint an identical picture forever and keep the machine
    /// awake to do it.
    fn tick_interval(&self) -> Option<std::time::Duration> {
        Option::None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        let result = handle_event(self, event);
        // Escape closes the picker, and for a popup that is the window closing
        // rather than a state to sit in: a picker that has stopped drawing
        // anything is an empty rectangle nailed over the desktop.
        if !self.is_open {
            return Response::Exit;
        }
        match result {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for EmojiPickerState {
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

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    let mut state = EmojiPickerState::new();
    app::launch("emojipicker", &mut state)
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
    use guitk::probe;

    /// The tree the picker would hand the compositor at its current size.
    ///
    /// Production code no longer builds one of these: [`App::render`] goes
    /// straight from the frame to the tree. The tests keep the step named
    /// because what most of them ask is what was *painted*, and a tree is the
    /// form that question is asked in.
    fn render(state: &EmojiPickerState) -> RenderTree {
        state.frame(state.width, state.height).into_tree()
    }

    /// The layout of a picker at the size it opens at.
    fn default_layout() -> Layout {
        EmojiPickerState::new().layout()
    }

    /// The skin-tone strip of a picker at the size it opens at.
    ///
    /// It is an `Option` in the layout because a window short enough to leave
    /// no room for a row of emoji drops the strip rather than squeezing the
    /// grid. 480px is not that window.
    fn default_strip() -> Rect {
        default_layout()
            .strip
            .expect("the strip fits at the opening size")
    }

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
        // The tree should contain a text command with the query -- rich text,
        // as the toolkit's editor draws it, so the selection can be coloured
        // glyph by glyph.
        let has_query = tree.commands.iter().any(|cmd| {
            matches!(
                cmd,
                guitk::render::RenderCommand::Text { text, .. }
                    | guitk::render::RenderCommand::RichText { text, .. }
                    if text == "test query"
            )
        });
        assert!(has_query, "render should include the search query text");
    }

    // --- Event handling ---

    #[test]
    fn escape_closes_picker() {
        let mut state = EmojiPickerState::new();
        assert!(state.is_open);
        let result = probe::key(&mut state, &probe::press(Key::Escape));
        assert_eq!(result, EventResult::Consumed);
        assert!(!state.is_open);
    }

    #[test]
    fn category_tab_click_switches_category() {
        let mut state = EmojiPickerState::new();
        // Tab order: Recent, SmileysAndPeople, AnimalsAndNature, ...
        // so the middle of tab 2 is AnimalsAndNature.
        let layout = state.layout();
        let event = Event::Mouse(MouseEvent {
            x: layout.tab_x(2) + layout.tab_width() / 2.0,
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
        probe::key(&mut state, &probe::typing("a"));
        assert_eq!(state.search_query, "a");
        assert_eq!(state.active_tab, Tab::Search);
    }

    #[test]
    fn backspace_removes_character() {
        let mut state = EmojiPickerState::new();
        state.search_focused = true;
        state.search_query = "ab".to_string();
        probe::key(&mut state, &probe::press(Key::Backspace));
        assert_eq!(state.search_query, "a");
    }

    /// A picker whose grid is genuinely taller than the panel it draws into.
    ///
    /// It has to be the *search* tab: the shipped database holds 82 emoji
    /// across eight categories, and at the default size — seven 48px columns
    /// in a 320px panel — the largest category (16) needs three rows, 160px,
    /// so no category tab can scroll at all. Searching "a" matches 73, which
    /// needs eleven rows and overflows. A scroll test on a grid that fits
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
            x: state.width / 2.0,
            y: state.layout().grid.y + 10.0,
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
    /// The arithmetic this guarded against is gone: the old `grid_hit_test`
    /// derived its row with `(adjusted_y / CELL_SIZE) as usize`, and a
    /// float-to-integer cast *saturates* rather than wrapping — so a large
    /// `y` produced a row near `usize::MAX`, and the row-major `row *
    /// GRID_COLUMNS` that followed overflowed and panicked in a debug build.
    /// Nothing bounds `y`: it arrives from the event.
    ///
    /// A hit test that reads back painted rectangles cannot express that bug —
    /// there is no row number to overflow, only a list of boxes none of which
    /// contains the point. The test stays because the input is still
    /// unbounded, and a miss is still the required answer.
    #[test]
    fn a_pointer_far_below_the_grid_misses_instead_of_overflowing() {
        let state = scrollable_picker();
        for y in [1.0e9_f32, 1.0e30, f32::MAX, f32::INFINITY] {
            assert_eq!(
                state.target_at(state.width / 2.0, y),
                Option::None,
                "y={y} should miss the grid"
            );
        }
    }

    #[test]
    fn mouse_move_updates_hover() {
        let mut state = EmojiPickerState::new();
        let layout = state.layout();
        let event = Event::Mouse(MouseEvent {
            x: layout.grid_left + CELL_SIZE / 2.0,
            y: layout.grid.y + GRID_PADDING + CELL_SIZE / 2.0,
            kind: MouseEventKind::Move,
        });
        handle_event(&mut state, &event);
        assert_eq!(state.hovered_emoji, Some(0));
    }

    #[test]
    fn click_emoji_records_selection() {
        let mut state = EmojiPickerState::new();
        let layout = state.layout();
        let event = Event::Mouse(MouseEvent {
            x: layout.grid_left + CELL_SIZE / 2.0,
            y: layout.grid.y + GRID_PADDING + CELL_SIZE / 2.0,
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
        let circle = state.layout().swatch(1).expect("the strip fits at 360x480");
        let event = Event::Mouse(MouseEvent {
            x: circle.centre().0,
            y: circle.centre().1,
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

        let bar_mid = default_strip().centre().1;
        for (i, &(x, _)) in swatches.iter().enumerate() {
            for at in [
                x + 0.5,
                x + SKIN_TONE_CIRCLE / 2.0,
                x + SKIN_TONE_CIRCLE - 0.5,
            ] {
                let mut state = EmojiPickerState::new();
                click_at(&mut state, at, bar_mid);
                assert_eq!(
                    Some(state.skin_tone),
                    SkinToneModifier::ALL.get(i).copied(),
                    "a click at x={at} did not select swatch {i}"
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
        let bar_mid = default_strip().centre().1;

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
        let result = click_at(&mut state, 4.0, default_strip().centre().1);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.skin_tone, SkinToneModifier::Medium);
    }

    /// Measured against the window, which the layout functions know nothing
    /// about: the swatches march left to right without overlapping, clear of
    /// the label, and inside the strip they are drawn on.
    #[test]
    fn the_swatches_stand_apart_inside_the_strip() {
        let swatches = painted_swatches(&EmojiPickerState::new());
        let bar_y = default_strip().y;
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
            default_layout().columns.get(),
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
        let layout = default_layout();
        for (i, &(icon_x, _)) in icons.iter().enumerate() {
            for at in [
                icon_x,
                layout.tab_x(i) + 0.5,
                layout.tab_x(i) + layout.tab_width() - 0.5,
            ] {
                let mut state = EmojiPickerState::new();
                click_at(&mut state, at, TAB_BAR_HEIGHT / 2.0);
                assert_eq!(
                    Some(state.active_tab),
                    expected.get(i).copied(),
                    "a click at x={at} did not select tab {i}"
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
        assert!((default_layout().tab_width() * TAB_COUNT as f32 - WINDOW_WIDTH).abs() < 0.01);
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

    /// Whether `cmds` hold the toolkit's field box at `rect` in `state`, at
    /// focus width `width`, command for command.
    fn draws_field(
        cmds: &[RenderCommand],
        palette: &Palette,
        rect: Rect,
        state: guitk::field::State,
        width: f32,
    ) -> bool {
        let seq = |s: guitk::field::State| {
            let mut want: Vec<RenderCommand> = Vec::new();
            guitk::field::draw(&mut want, palette, rect, s, width);
            want
        };
        let has = |want: &[RenderCommand]| {
            !want.is_empty() && cmds.windows(want.len()).any(|w| w == want)
        };
        // Not focused means no focus mark either: an unfocused box's commands
        // begin a focused one's under a ring or an underline, so finding them
        // alone says nothing about the mark.
        has(&seq(state))
            && (state.focused
                || !has(&seq(guitk::field::State {
                    focused: true,
                    ..state
                })))
    }

    /// The search field is the toolkit's, in the theme's shape (lane C,
    /// c-e-a-theme-can-shape-the-controls): lit under the pointer, out when
    /// the pointer leaves, and marked at the user's focus width while it has
    /// the keyboard.
    #[test]
    fn the_search_field_is_the_toolkits_lit_under_the_pointer_and_focused_at_the_users_width() {
        let mut state = EmojiPickerState::new();
        let mut palette = state.palette;
        palette.widget_style.field.focus = guitk::widget_style::FocusMark::Ring;
        App::theme_changed(&mut state, &palette);
        let settings = appearance::AppearanceSettings {
            focus_ring_scale: 2.5,
            ..Default::default()
        };
        let width = settings.focus_ring_width();
        assert!(width > guitk::style::FOCUS_RING_WIDTH);
        App::appearance_changed(&mut state, &settings);
        let field = state.layout().search_field().expect("a search field");
        let drawn = |state: &EmojiPickerState, s: guitk::field::State| {
            let frame = state.frame(state.width, state.height);
            draws_field(frame.commands(), &palette, field, s, width)
        };
        let idle = guitk::field::State::default();
        assert!(drawn(&state, idle), "the field is not the toolkit's");

        let (cx, cy) = field.centre();
        let at = |kind| Event::Mouse(MouseEvent { x: cx, y: cy, kind });
        handle_event(&mut state, &at(MouseEventKind::Move));
        let hovered = guitk::field::State {
            hovered: true,
            ..idle
        };
        assert!(
            drawn(&state, hovered),
            "the field is not lit under the pointer"
        );
        handle_event(
            &mut state,
            &Event::Mouse(MouseEvent {
                x: -1.0,
                y: -1.0,
                kind: MouseEventKind::Leave,
            }),
        );
        assert!(
            drawn(&state, idle),
            "the field stayed lit after the pointer left"
        );

        handle_event(&mut state, &at(MouseEventKind::Press(MouseButton::Left)));
        let focused = guitk::field::State {
            focused: true,
            ..idle
        };
        assert!(
            drawn(&state, focused),
            "the focused field is not marked at the user's width"
        );
    }

    #[test]
    fn leaving_the_window_puts_out_the_cell_under_the_pointer() {
        let mut state = EmojiPickerState::new();
        let cell = probe::rect_of(&state, Target::Cell(0)).expect("a cell");
        let (x, y) = cell.centre();
        handle_event(
            &mut state,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Move,
            }),
        );
        assert_eq!(state.hovered_emoji, Some(0));
        handle_event(
            &mut state,
            &Event::Mouse(MouseEvent {
                x: -1.0,
                y: -1.0,
                kind: MouseEventKind::Leave,
            }),
        );
        assert_eq!(
            state.hovered_emoji, None,
            "the cell stayed lit after the pointer left"
        );
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
    // --- The window -------------------------------------------------------
    //
    // The picker used to be a function that built a render tree. `main` called
    // it once, took the length of the result, and returned; nothing opened,
    // nothing was clickable, and the fixed 360x480 geometry was never
    // contradicted because there was no other size for it to be wrong at.

    /// The size the picker asks the compositor to open at is the size its
    /// layout was measured for.
    #[test]
    fn the_window_opens_at_the_size_the_layout_expects() {
        let state = EmojiPickerState::new();
        let (w, h) = state.initial_size();
        assert_eq!((w as f32, h as f32), (WINDOW_WIDTH, WINDOW_HEIGHT));
        assert_eq!(
            <EmojiPickerState as Probe>::SIZE,
            (WINDOW_WIDTH, WINDOW_HEIGHT)
        );
        assert!(!state.title().is_empty());
    }

    /// Rendering at a size adopts it: the next hit test answers for the window
    /// that was drawn, not for the one the picker was built at.
    #[test]
    fn rendering_at_a_size_is_what_teaches_the_picker_that_size() {
        let mut state = EmojiPickerState::new();
        let narrow = 240.0;
        let tree = state.render(narrow, 400.0);
        assert!(!tree.commands.is_empty());
        assert!((state.width - narrow).abs() < 0.01);
        assert_eq!(
            state.layout().columns.get(),
            5,
            "240px holds five 48px columns"
        );
    }

    /// A wider window is more columns, not a wider gutter. This is the whole
    /// point of deriving the layout per frame: the old code multiplied a
    /// `GRID_COLUMNS` constant by `CELL_SIZE` and centred the result in
    /// `WINDOW_WIDTH`, so a resized window grew its margins and nothing else.
    #[test]
    fn a_wider_window_fits_more_columns() {
        let narrow = Layout::new(240.0, WINDOW_HEIGHT).columns.get();
        let wide = Layout::new(720.0, WINDOW_HEIGHT).columns.get();
        assert!(
            wide > narrow,
            "720px gave {wide} columns and 240px gave {narrow}"
        );
        assert_eq!(narrow, 5);
        assert_eq!(wide, 15);
    }

    /// However narrow the window, at least one column is offered: a zero-column
    /// grid would divide by zero when turning an index into a row.
    #[test]
    fn even_an_impossibly_narrow_window_keeps_one_column() {
        for width in [0.0_f32, 1.0, 10.0, CELL_SIZE - 0.5] {
            let layout = Layout::new(width, WINDOW_HEIGHT);
            assert_eq!(layout.columns.get(), 1, "at width={width}");
            // And the cell arithmetic that divides by it stays finite.
            assert!(layout.cell(3).y.is_finite());
        }
    }

    /// A window too short for everything drops bands rather than squeezing the
    /// grid: the grid is the picker, and a picker with no room to show an
    /// emoji is not a smaller picker but a broken one.
    #[test]
    fn a_short_window_drops_bands_instead_of_crushing_the_grid() {
        // Tall enough for all four bands.
        let full = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(full.search_row.is_some());
        assert!(full.strip.is_some());
        assert!(full.preview.is_some());
        assert!(full.grid.h >= CELL_SIZE);

        // Short enough that something has to give, at every height down to
        // nothing. Two things hold the whole way down: the bands never claim
        // more than the window has, and no optional band is kept at the price
        // of a row of emoji. Below the height where even a bare grid can show
        // a row, everything else is already gone and the grid keeps whatever
        // is left -- a clipped row being more use than an empty window.
        let mut height = WINDOW_HEIGHT;
        while height >= 0.0 {
            let layout = Layout::new(WINDOW_WIDTH, height);
            let bands = layout.tab_bar.h
                + layout.search_row.map_or(0.0, |r| r.h)
                + layout.grid.h
                + layout.strip.map_or(0.0, |r| r.h)
                + layout.preview.map_or(0.0, |r| r.h);
            assert!(
                bands <= height + 0.01,
                "at height={height} the bands total {bands}"
            );
            if layout.grid.h < CELL_SIZE {
                assert!(
                    layout.search_row.is_none()
                        && layout.strip.is_none()
                        && layout.preview.is_none(),
                    "at height={height} the grid is only {}px, yet a band was kept",
                    layout.grid.h
                );
            }
            height -= 1.0;
        }
    }

    /// The preview goes before the strip does. It names an emoji that is
    /// already on screen a row above; the strip is the only place a skin tone
    /// can be chosen at all.
    #[test]
    fn the_preview_is_the_first_band_sacrificed() {
        let mut saw_strip_without_preview = false;
        let mut height = WINDOW_HEIGHT;
        while height >= 0.0 {
            let layout = Layout::new(WINDOW_WIDTH, height);
            if layout.preview.is_some() {
                assert!(
                    layout.strip.is_some(),
                    "at height={height} the preview survived but the strip did not"
                );
            } else if layout.strip.is_some() {
                saw_strip_without_preview = true;
            }
            height -= 1.0;
        }
        assert!(
            saw_strip_without_preview,
            "no height dropped only the preview, so the ordering is untested"
        );
    }

    /// When the strip is gone, so is the target it carried: a click where it
    /// used to be must not select a skin tone.
    #[test]
    fn a_dropped_band_takes_its_clickable_targets_with_it() {
        let mut state = EmojiPickerState::new();
        state.resize(WINDOW_WIDTH, TAB_BAR_HEIGHT + CELL_SIZE + 4.0);
        let layout = state.layout();
        assert!(layout.strip.is_none(), "this height must drop the strip");
        assert!(layout.preview.is_none());

        let frame = state.frame(state.width, state.height);
        assert!(
            !frame
                .hits()
                .iter()
                .any(|&(t, _)| matches!(t, Target::Swatch(_))),
            "a swatch is still clickable in a window with no strip"
        );
    }

    /// A cell scrolled up behind the tab bar is not clickable.
    ///
    /// This is the bug the arithmetic hit test could not have avoided. It
    /// subtracted the scroll offset from `y` and divided, so a point that had
    /// scrolled out of the viewport still resolved to whatever row the
    /// subtraction landed on: nothing in the calculation knew about the
    /// viewport. Recording hit boxes as they are painted, inside the clip,
    /// drops them instead.
    #[test]
    fn an_emoji_scrolled_out_of_the_viewport_is_not_clickable() {
        let mut state = scrollable_picker();
        let (x, y) = state.layout().cell(0).centre();

        assert_eq!(state.target_at(x, y), Some(Target::Cell(0)));

        state.scroll_offset = state.max_scroll();
        assert!(state.scroll_offset > CELL_SIZE);
        assert_ne!(
            state.target_at(x, y),
            Some(Target::Cell(0)),
            "the first cell is scrolled away but still answers at its old place"
        );
    }

    /// Nothing outside the window is clickable, and nothing anywhere panics.
    #[test]
    fn a_hit_test_anywhere_at_any_size_is_answerable() {
        for (w, h) in [
            (WINDOW_WIDTH, WINDOW_HEIGHT),
            (120.0, 90.0),
            (1200.0, 200.0),
            (48.0, 48.0),
        ] {
            let mut state = scrollable_picker();
            state.resize(w, h);
            let mut y = -20.0;
            while y < h + 20.0 {
                let mut x = -20.0;
                while x < w + 20.0 {
                    let hit = state.target_at(x, y);
                    if x < 0.0 || y < 0.0 || x >= w || y >= h {
                        assert_eq!(hit, Option::None, "a hit at ({x}, {y}) outside {w}x{h}");
                    }
                    x += 7.0;
                }
                y += 7.0;
            }
        }
    }

    /// A closed picker draws nothing and asks the window to go away.
    #[test]
    fn closing_the_picker_ends_the_program() {
        let mut state = EmojiPickerState::new();
        let response = state.on_event(&Event::Key(probe::press(Key::Escape)));
        assert_eq!(response, Response::Exit);
        assert!(state.frame(state.width, state.height).commands().is_empty());
    }

    /// The compositor asking to close is also an exit, without the picker
    /// having had to be closed from the inside first.
    #[test]
    fn the_close_button_ends_the_program() {
        let mut state = EmojiPickerState::new();
        assert_eq!(state.on_event(&Event::CloseRequested), Response::Exit);
    }

    /// A resize arriving as an event has the same effect as one arriving as a
    /// render size -- the picker must not hold two ideas of how big it is.
    #[test]
    fn a_resize_event_and_a_render_size_agree() {
        let mut by_event = EmojiPickerState::new();
        handle_event(
            &mut by_event,
            &Event::Resize {
                width: 500,
                height: 300,
            },
        );

        let mut by_render = EmojiPickerState::new();
        let _ = by_render.render(500.0, 300.0);

        assert_eq!(by_event.layout().columns, by_render.layout().columns);
        assert!((by_event.layout().grid.h - by_render.layout().grid.h).abs() < 0.01);
    }

    /// Scrolling to the end, then making the window taller, must not leave the
    /// grid scrolled past its own content.
    #[test]
    fn growing_the_window_pulls_the_scroll_back_into_range() {
        let mut state = scrollable_picker();
        state.scroll_offset = state.max_scroll();
        state.resize(WINDOW_WIDTH, 1200.0);
        assert!(
            state.scroll_offset <= state.max_scroll() + 0.01,
            "offset {} exceeds the new maximum {}",
            state.scroll_offset,
            state.max_scroll()
        );
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

        fn fills(app: &mut EmojiPickerState) -> Vec<Color> {
            app.render(600.0, 500.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = EmojiPickerState::new();

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

    // --- The keyboard: the grid, the search box, and the list of keys ---

    fn key_of(k: Key, modifiers: guitk::event::Modifiers, text: &str) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: text.to_owned(),
        }
    }

    fn hit(
        state: &mut EmojiPickerState,
        k: Key,
        modifiers: guitk::event::Modifiers,
    ) -> EventResult {
        handle_event(state, &Event::Key(key_of(k, modifiers, "")))
    }

    fn plain(state: &mut EmojiPickerState, k: Key) -> EventResult {
        hit(state, k, guitk::event::Modifiers::NONE)
    }

    fn type_in(state: &mut EmojiPickerState, text: &str) {
        for c in text.chars() {
            handle_event(
                state,
                &Event::Key(key_of(
                    Key::A,
                    guitk::event::Modifiers::NONE,
                    &c.to_string(),
                )),
            );
        }
    }

    fn drawn_strings(state: &EmojiPickerState) -> Vec<String> {
        render(state)
            .commands
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } | RenderCommand::RichText { text, .. } => {
                    Some(text)
                }
                _ => Option::None,
            })
            .collect()
    }

    /// The emoji at `index` of what the grid shows.
    fn shown(state: &EmojiPickerState, index: usize) -> String {
        state.visible_emoji()[index].emoji.clone()
    }

    /// **The arrows light an emoji, and Enter picks it.** The grid answered
    /// only the pointer: no key lit an emoji or picked one.
    #[test]
    fn the_arrows_light_an_emoji_and_enter_picks_it() {
        let mut state = scrollable_picker();
        let columns = state.layout().columns.get();
        let count = state.visible_emoji().len();
        assert!(
            count > columns * 2 && state.max_scroll() > CELL_SIZE,
            "the test needs a grid longer than the window"
        );

        plain(&mut state, Key::Right);
        assert_eq!(
            state.hovered_emoji,
            Some(0),
            "an arrow from nothing lit lit nothing"
        );
        plain(&mut state, Key::Right);
        assert_eq!(state.hovered_emoji, Some(1));
        plain(&mut state, Key::Down);
        assert_eq!(state.hovered_emoji, Some(1 + columns), "Down is a row on");
        plain(&mut state, Key::Up);
        assert_eq!(state.hovered_emoji, Some(1), "Up is a row back");
        for _ in 0..3 {
            plain(&mut state, Key::Left);
        }
        assert_eq!(state.hovered_emoji, Some(0), "Left went past the first");

        plain(&mut state, Key::End);
        assert_eq!(state.hovered_emoji, Some(count - 1), "End");
        assert!(
            state.scroll_offset > 0.0,
            "the last emoji was lit out of sight"
        );
        let layout = state.layout();
        let cell = layout.cell(count - 1);
        assert!(
            cell.bottom() - state.scroll_offset <= layout.grid.bottom() + 0.5,
            "the lit emoji is below the grid"
        );
        plain(&mut state, Key::Down);
        assert_eq!(
            state.hovered_emoji,
            Some(count - 1),
            "Down went past the last"
        );
        plain(&mut state, Key::Home);
        assert_eq!(state.hovered_emoji, Some(0), "Home");
        assert!(
            state.scroll_offset.abs() < 0.5,
            "the first emoji was lit out of sight"
        );
        plain(&mut state, Key::PageDown);
        let page = state.hovered_emoji.expect("lit");
        assert!(
            page >= columns && page.is_multiple_of(columns),
            "PageDown went {page}"
        );
        plain(&mut state, Key::PageUp);
        assert_eq!(state.hovered_emoji, Some(0), "PageUp");

        plain(&mut state, Key::Right);
        let lit = shown(&state, 1);
        assert!(
            drawn_strings(&state).contains(&state.visible_emoji()[1].name),
            "the preview does not name the lit emoji"
        );
        plain(&mut state, Key::Enter);
        assert_eq!(state.last_selected.as_deref(), Some(lit.as_str()), "Enter");
        plain(&mut state, Key::Right);
        let next = shown(&state, 2);
        plain(&mut state, Key::Space);
        assert_eq!(state.last_selected.as_deref(), Some(next.as_str()), "Space");
    }

    /// **A word typed and Enter picks the best match**; Down goes from the box
    /// to what it found.
    #[test]
    fn a_word_typed_and_enter_picks_the_best_match() {
        let mut state = EmojiPickerState::new();
        for _ in 0..3 {
            plain(&mut state, Key::Right);
        }
        hit(&mut state, Key::F, guitk::event::Modifiers::ctrl());
        assert!(state.search_focused, "Ctrl+F did not reach the search box");
        type_in(&mut state, "heart");
        assert_eq!(state.active_tab, Tab::Search);
        assert_eq!(
            state.hovered_emoji,
            Option::None,
            "the light lit before the search stayed on, on a different emoji"
        );
        assert!(
            state.visible_emoji().len() > 1,
            "the test needs several hearts"
        );
        let first = shown(&state, 0);
        plain(&mut state, Key::Enter);
        assert_eq!(state.last_selected.as_deref(), Some(first.as_str()));

        plain(&mut state, Key::Down);
        assert_eq!(
            state.hovered_emoji,
            Some(0),
            "Down from the box lit nothing"
        );
        assert!(state.search_focused, "Down took the keyboard from the box");
        plain(&mut state, Key::Tab);
        assert!(!state.search_focused, "Tab left the keyboard in the box");
        plain(&mut state, Key::Right);
        let second = shown(&state, 1);
        plain(&mut state, Key::Enter);
        assert_eq!(state.last_selected.as_deref(), Some(second.as_str()));
        plain(&mut state, Key::Tab);
        assert!(
            state.search_focused,
            "Tab from the grid did not reach the box"
        );
    }

    /// **The search box edits at a caret**: it took typing at its end and
    /// Backspace from it, and nothing else.
    #[test]
    fn the_search_box_edits_at_a_caret() {
        let mut state = EmojiPickerState::new();
        assert!(
            drawn_strings(&state).iter().any(|t| t == "Search emoji..."),
            "the empty box does not say what it is for"
        );
        let (x, y) = {
            let field = state.layout().search_field().expect("a search box");
            (field.x + field.w / 2.0, field.y + field.h / 2.0)
        };
        handle_event(
            &mut state,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        assert!(state.search_focused);
        let field = state.layout().search_field().expect("a search box");
        let carets = |state: &EmojiPickerState| {
            render(state)
                .commands
                .iter()
                .filter(|c| {
                    matches!(
                        c,
                        RenderCommand::Line { x1, x2, .. }
                            if (x1 - x2).abs() < f32::EPSILON && field.contains(*x1, y)
                    )
                })
                .count()
        };
        assert_eq!(
            carets(&state),
            1,
            "the empty box with the keyboard draws no caret"
        );
        type_in(&mut state, "dgo");
        plain(&mut state, Key::Left);
        plain(&mut state, Key::Left);
        type_in(&mut state, "o");
        plain(&mut state, Key::End);
        plain(&mut state, Key::Backspace);
        assert_eq!(state.search_query, "dog", "the caret did not move");
        hit(&mut state, Key::A, guitk::event::Modifiers::ctrl());
        hit(&mut state, Key::X, guitk::event::Modifiers::ctrl());
        assert_eq!(state.search_query, "", "Ctrl+X");
        hit(&mut state, Key::V, guitk::event::Modifiers::ctrl());
        hit(&mut state, Key::V, guitk::event::Modifiers::ctrl());
        assert_eq!(state.search_query, "dogdog", "Ctrl+V");
        plain(&mut state, Key::Home);
        plain(&mut state, Key::Delete);
        assert_eq!(state.search_query, "ogdog", "Delete at the caret");

        // A press past the text's end puts the caret at its end, and one at
        // the text's start puts it there.
        handle_event(
            &mut state,
            &Event::Mouse(MouseEvent {
                x: field.right() - 2.0,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        type_in(&mut state, "s");
        assert_eq!(
            state.search_query, "ogdogs",
            "a press past the text did not put the caret at its end"
        );
        handle_event(
            &mut state,
            &Event::Mouse(MouseEvent {
                x: field.x + SEARCH_TEXT_INSET,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        type_in(&mut state, "d");
        assert_eq!(
            state.search_query, "dogdogs",
            "a press at the text's start did not put the caret there"
        );
        assert_eq!(carets(&state), 1, "no caret is drawn in the box");
        hit(&mut state, Key::Escape, guitk::event::Modifiers::NONE);
    }

    /// **Ctrl+Tab walks the tabs**, wrapping, and Ctrl+Shift+Tab back; AltGr+Tab
    /// is not Ctrl+Tab.
    #[test]
    fn ctrl_tab_walks_the_tabs() {
        let mut state = EmojiPickerState::new();
        let all = tabs();
        let start = all
            .iter()
            .position(|t| *t == state.active_tab)
            .expect("a tab");
        plain(&mut state, Key::Right);
        assert!(state.hovered_emoji.is_some());
        hit(&mut state, Key::Tab, guitk::event::Modifiers::ctrl());
        assert_eq!(state.active_tab, all[(start + 1) % all.len()]);
        assert_eq!(
            state.hovered_emoji,
            Option::None,
            "the light stayed on, on an emoji of the tab it was not lit on"
        );
        let back = guitk::event::Modifiers {
            ctrl: true,
            shift: true,
            ..guitk::event::Modifiers::NONE
        };
        hit(&mut state, Key::Tab, back);
        hit(&mut state, Key::Tab, back);
        assert_eq!(state.active_tab, all[(start + all.len() - 1) % all.len()]);
        let altgr = guitk::event::Modifiers {
            ctrl: true,
            alt: true,
            ..guitk::event::Modifiers::NONE
        };
        let was = state.active_tab;
        hit(&mut state, Key::Tab, altgr);
        assert_eq!(state.active_tab, was, "AltGr+Tab was taken for Ctrl+Tab");
        // On a tab with emoji to light: Recently Used, where this has ended
        // up, is empty in a picker nobody has used.
        hit(&mut state, Key::Tab, guitk::event::Modifiers::ctrl());
        assert!(
            !state.visible_emoji().is_empty(),
            "the test needs emoji to light"
        );
        hit(&mut state, Key::Right, guitk::event::Modifiers::alt());
        assert_eq!(state.hovered_emoji, Option::None, "Alt+Right lit an emoji");
    }

    /// **Every key the list of keys advertises is answered by this window.**
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let mut state = EmojiPickerState::new();
                assert_eq!(
                    handle_event(&mut state, &Event::Key(stroke.clone())),
                    EventResult::Consumed,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// **The list of keys reaches the window**, and goes on F1, `?` or a plain
    /// Escape -- not on Alt+Escape, which is the desktop's.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        use guitk::event::Modifiers;
        let mut state = EmojiPickerState::new();
        assert!(
            !drawn_strings(&state)
                .iter()
                .any(|t| t.contains("F1 or ? closes this")),
            "the list is up before anybody asked for it"
        );
        for chord in [Modifiers::alt(), Modifiers::super_key()] {
            hit(&mut state, Key::F1, chord);
            assert!(
                !state.show_help,
                "{chord:?}+F1, the desktop's, raised the list"
            );
        }
        plain(&mut state, Key::F1);
        let missing = guitk::shortcut::missing_rows(&drawn_strings(&state), SHORTCUTS);
        assert!(missing.is_empty(), "{missing:?}");
        hit(&mut state, Key::Escape, Modifiers::alt());
        assert!(state.show_help, "Alt+Escape put the list away");
        plain(&mut state, Key::Escape);
        assert!(!state.show_help, "Escape left the list up");
        assert!(state.is_open, "Escape under the list closed the picker");
        handle_event(
            &mut state,
            &Event::Key(key_of(Key::Slash, Modifiers::shift(), "?")),
        );
        assert!(state.show_help, "? raised nothing");
        handle_event(
            &mut state,
            &Event::Key(key_of(Key::Slash, Modifiers::shift(), "?")),
        );
        assert!(!state.show_help, "? left the list up");
        plain(&mut state, Key::F1);
        plain(&mut state, Key::F1);
        assert!(!state.show_help, "F1 left the list up");
    }

    /// **`?` is typed into the search box**, where it is a character, and F1
    /// raises the list from there, where it never is.
    #[test]
    fn a_question_mark_is_typed_into_the_search_and_f1_still_raises_the_list() {
        use guitk::event::Modifiers;
        let mut state = EmojiPickerState::new();
        hit(&mut state, Key::F, Modifiers::ctrl());
        handle_event(
            &mut state,
            &Event::Key(key_of(Key::Slash, Modifiers::shift(), "?")),
        );
        assert!(!state.show_help, "? raised the list from the search box");
        assert_eq!(state.search_query, "?", "? was not typed");
        plain(&mut state, Key::F1);
        assert!(
            state.show_help,
            "F1 did not raise the list from the search box"
        );
        let field = state.layout().search_field().expect("a search box");
        let drawn = render(&state).commands;
        let mark = |focused: bool| {
            let mut v: Vec<RenderCommand> = Vec::new();
            guitk::field::draw(
                &mut v,
                &state.palette,
                field,
                guitk::field::State {
                    hovered: false,
                    focused,
                    disabled: false,
                    invalid: false,
                },
                state.focus_ring_width,
            );
            drawn.windows(v.len()).any(|w| w == v.as_slice())
        };
        assert!(
            mark(false) && !mark(true),
            "the box's keyboard mark is drawn under the list"
        );
        assert!(
            !drawn.iter().any(|c| matches!(
                c,
                RenderCommand::Line { x1, x2, y1, .. }
                    if (x1 - x2).abs() < f32::EPSILON && field.contains(*x1, *y1 + 1.0)
            )),
            "the box's caret is drawn under the list"
        );
    }

    /// **The list of keys is modal**: with it up, no key, press or turn of the
    /// wheel reaches what it covers -- Enter picks nothing, Escape closes the
    /// list and not the picker. The grid is the same with it down.
    #[test]
    fn the_shortcut_list_takes_the_keys_and_a_press() {
        let mut state = scrollable_picker();
        let cell = {
            let layout = state.layout();
            let c = layout.cell(3);
            (c.x + c.w / 2.0, c.y + c.h / 2.0)
        };
        plain(&mut state, Key::F1);
        for k in [Key::Right, Key::Enter, Key::Space, Key::Tab, Key::Down] {
            assert_eq!(
                plain(&mut state, k),
                EventResult::Consumed,
                "{k:?} was not taken"
            );
        }
        assert!(
            state.show_help,
            "a key other than F1, ? or Escape put the list away"
        );
        assert_eq!(
            state.hovered_emoji,
            Option::None,
            "an arrow lit an emoji under it"
        );
        assert_eq!(
            state.last_selected,
            Option::None,
            "Enter picked an emoji under it"
        );
        assert!(!state.search_focused, "Tab took the search box under it");
        let wheel = wheel_over_grid(&state, -1.0);
        assert_eq!(handle_event(&mut state, &wheel), EventResult::Ignored);
        assert!(
            state.scroll_offset.abs() < 0.5,
            "the wheel scrolled the grid under it"
        );
        handle_event(
            &mut state,
            &Event::Mouse(MouseEvent {
                x: cell.0,
                y: cell.1,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        assert!(!state.show_help, "the press did not put the list away");
        assert_eq!(
            state.last_selected,
            Option::None,
            "the press picked an emoji under it"
        );
        plain(&mut state, Key::F1);
        handle_event(
            &mut state,
            &Event::Mouse(MouseEvent {
                x: cell.0,
                y: cell.1,
                kind: MouseEventKind::Press(MouseButton::Right),
            }),
        );
        assert!(!state.show_help, "a right-button press left the list up");

        // The grid.
        plain(&mut state, Key::Right);
        assert_eq!(state.hovered_emoji, Some(0));
    }
}

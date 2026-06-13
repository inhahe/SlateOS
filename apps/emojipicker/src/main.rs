//! SlateOS Emoji Picker
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
use guitk::render::{FontWeightHint, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

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

    /// Append the Fitzpatrick modifier to `base_emoji`.
    ///
    /// If the modifier is `None`, returns the original emoji unchanged.
    /// Otherwise appends the corresponding Unicode skin-tone codepoint.
    pub fn apply(self, base_emoji: &str) -> String {
        match self.modifier_char() {
            Some(ch) => {
                let mut result = String::with_capacity(base_emoji.len() + 4);
                result.push_str(base_emoji);
                result.push(ch);
                result
            }
            Option::None => base_emoji.to_string(),
        }
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
        use EmojiCategory::*;
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
        WINDOW_HEIGHT - self.grid_top() - PREVIEW_HEIGHT - SKIN_TONE_HEIGHT
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

    // Ordered tabs: Recent, then each category, then search.
    let tabs: Vec<Tab> = {
        let mut v = vec![Tab::Recent];
        for &cat in EmojiCategory::ALL {
            v.push(Tab::Category(cat));
        }
        v.push(Tab::Search);
        v
    };

    let tab_count = tabs.len() as f32;
    let tab_width = WINDOW_WIDTH / tab_count;

    for (i, &tab) in tabs.iter().enumerate() {
        let x = i as f32 * tab_width;
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

    let grid_left = (WINDOW_WIDTH - (GRID_COLUMNS as f32 * CELL_SIZE)) / 2.0;

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

        // Emoji glyph.
        let text_x = x + CELL_PADDING;
        let text_y = y + CELL_PADDING;
        tree.push(guitk::render::RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: entry.emoji.clone(),
            color: mocha::TEXT,
            font_size: EMOJI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(CELL_SIZE - CELL_PADDING * 2.0),
        });
    }

    tree.untranslate();
    tree.unclip();
}

/// Render the skin tone selector strip.
fn render_skin_tone_bar(state: &EmojiPickerState, tree: &mut RenderTree) {
    let bar_y = WINDOW_HEIGHT - PREVIEW_HEIGHT - SKIN_TONE_HEIGHT;

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
    });

    // Skin tone circles.
    let circles_start_x = 80.0;
    for (i, &tone) in SkinToneModifier::ALL.iter().enumerate() {
        let cx = circles_start_x + i as f32 * (SKIN_TONE_CIRCLE + SKIN_TONE_SPACING);
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

        // Selection ring.
        if state.skin_tone == tone {
            tree.push(guitk::render::RenderCommand::StrokeRect {
                x: cx - 2.0,
                y: cy - 2.0,
                width: SKIN_TONE_CIRCLE + 4.0,
                height: SKIN_TONE_CIRCLE + 4.0,
                color: mocha::BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::all((SKIN_TONE_CIRCLE + 4.0) / 2.0),
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
            let grid_bottom = WINDOW_HEIGHT - PREVIEW_HEIGHT - SKIN_TONE_HEIGHT;
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
            let skin_bar_y = WINDOW_HEIGHT - PREVIEW_HEIGHT - SKIN_TONE_HEIGHT;
            if y >= skin_bar_y && y < skin_bar_y + SKIN_TONE_HEIGHT {
                return handle_skin_tone_click(state, x, skin_bar_y);
            }

            state.search_focused = false;
            EventResult::Consumed
        }

        MouseEventKind::Move => {
            // Update hover in grid area.
            let grid_top = state.grid_top();
            let grid_bottom = WINDOW_HEIGHT - PREVIEW_HEIGHT - SKIN_TONE_HEIGHT;
            if y >= grid_top && y < grid_bottom {
                state.hovered_emoji = grid_hit_test(state, x, y);
            } else {
                state.hovered_emoji = Option::None;
            }
            EventResult::Consumed
        }

        MouseEventKind::Scroll { dy, .. } => {
            let grid_top = state.grid_top();
            let grid_bottom = WINDOW_HEIGHT - PREVIEW_HEIGHT - SKIN_TONE_HEIGHT;
            if y >= grid_top && y < grid_bottom {
                state.scroll_offset -= dy;
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
    let tabs: Vec<Tab> = {
        let mut v = vec![Tab::Recent];
        for &cat in EmojiCategory::ALL {
            v.push(Tab::Category(cat));
        }
        v.push(Tab::Search);
        v
    };

    let tab_width = WINDOW_WIDTH / tabs.len() as f32;
    let idx = (x / tab_width) as usize;

    if let Some(&tab) = tabs.get(idx) {
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
fn handle_skin_tone_click(state: &mut EmojiPickerState, x: f32, bar_y: f32) -> EventResult {
    let _ = bar_y; // Only x matters for circle hit testing.
    let circles_start_x = 80.0;
    for (i, &tone) in SkinToneModifier::ALL.iter().enumerate() {
        let cx = circles_start_x + i as f32 * (SKIN_TONE_CIRCLE + SKIN_TONE_SPACING);
        if x >= cx && x <= cx + SKIN_TONE_CIRCLE {
            state.skin_tone = tone;
            return EventResult::Consumed;
        }
    }
    EventResult::Ignored
}

/// Given a mouse position within the grid area, return the index of the emoji
/// under the cursor, if any.
fn grid_hit_test(state: &EmojiPickerState, x: f32, y: f32) -> Option<usize> {
    let grid_top = state.grid_top();
    let grid_left = (WINDOW_WIDTH - (GRID_COLUMNS as f32 * CELL_SIZE)) / 2.0;

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

    let idx = row * GRID_COLUMNS + col;
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
        // Click in the third tab area (index 2 = first category after Recent).
        // Tab order: Recent, SmileysAndPeople, AnimalsAndNature, ...
        // Tab width = 360 / 10 = 36.0
        // Click index 2 => AnimalsAndNature
        let tab_width = WINDOW_WIDTH / 10.0;
        let click_x = tab_width * 2.0 + tab_width / 2.0;
        let event = Event::Mouse(MouseEvent {
            x: click_x,
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

    #[test]
    fn scroll_event_changes_offset() {
        let mut state = EmojiPickerState::new();
        let grid_y = state.grid_top() + 10.0;
        let event = Event::Mouse(MouseEvent {
            x: WINDOW_WIDTH / 2.0,
            y: grid_y,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -30.0 },
        });
        handle_event(&mut state, &event);
        assert!(
            state.scroll_offset >= 0.0,
            "scroll offset should be non-negative"
        );
    }

    #[test]
    fn mouse_move_updates_hover() {
        let mut state = EmojiPickerState::new();
        let grid_top = state.grid_top();
        let grid_left = (WINDOW_WIDTH - (GRID_COLUMNS as f32 * CELL_SIZE)) / 2.0;
        let event = Event::Mouse(MouseEvent {
            x: grid_left + CELL_SIZE / 2.0,
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
        let grid_left = (WINDOW_WIDTH - (GRID_COLUMNS as f32 * CELL_SIZE)) / 2.0;
        let event = Event::Mouse(MouseEvent {
            x: grid_left + CELL_SIZE / 2.0,
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
        let bar_y = WINDOW_HEIGHT - PREVIEW_HEIGHT - SKIN_TONE_HEIGHT;
        // Click on the second circle (Light skin tone).
        let cx = 80.0 + (SKIN_TONE_CIRCLE + SKIN_TONE_SPACING) + SKIN_TONE_CIRCLE / 2.0;
        let event = Event::Mouse(MouseEvent {
            x: cx,
            y: bar_y + SKIN_TONE_HEIGHT / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        handle_event(&mut state, &event);
        assert_eq!(state.skin_tone, SkinToneModifier::Light);
    }

    // --- Picker state ---

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

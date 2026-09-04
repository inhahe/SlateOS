//! `Slate OS` Password Generator & Strength Analyzer
//!
//! A password utility tool with:
//! - Configurable password generation (length, character classes)
//! - Passphrase generation using word lists (Diceware-style)
//! - Password strength analysis (entropy, crack time estimation)
//! - Pattern detection (dictionary words, keyboard sequences, repeats)
//! - Breach check simulation (hash-based lookup)
//! - Password history (generated passwords, not stored passwords)
//! - Bulk generation with export
//! - PIN generator with configurable length
//! - Pronounceable password generator
//! - Password policy compliance checking
//! - Multi-panel UI with generator, analyzer, and history
//!
//! Uses the guitk library for UI rendering.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml. This keeps the discipline
// centralised rather than diverging per-crate.

use guitk::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SecretSource, SeededRng, SystemRandom};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ============================================================================
// Layout constants
// ============================================================================

const TOOLBAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const LEFT_PANEL_WIDTH: f32 = 400.0;
const ITEM_HEIGHT: f32 = 28.0;
const CORNER_RADIUS: f32 = 4.0;

/// Height of one row in the analyzer's detected-pattern list.
const PATTERN_ROW_HEIGHT: f32 = 15.0;

/// Vertical pitch of one row in the history list: the card plus its gap.
const HISTORY_ROW_PITCH: f32 = ITEM_HEIGHT + 4.0;

/// Most history entries the list will draw, however tall the panel is.
///
/// The history itself is longer; the list is the recent end of it, and the
/// heading states the full count.
const HISTORY_MAX_ROWS: usize = 20;

/// How many rows of `row_height` fit between `top` and `bottom`.
///
/// Counted rather than divided so there is no float-to-integer cast to get
/// wrong at the boundary, and so a zero-or-negative gap yields zero rather
/// than a wrapped-around count.
fn rows_that_fit(top: f32, bottom: f32, row_height: f32) -> usize {
    if row_height <= 0.0 {
        return 0;
    }
    let mut rows = 0usize;
    let mut probe = top;
    while probe + row_height <= bottom {
        rows = rows.saturating_add(1);
        probe += row_height;
    }
    rows
}

/// Draw the analyzer's detected-pattern list starting at `top`, returning the
/// cursor position just past the last row drawn.
///
/// The list is bounded by the room that actually exists between `top` and
/// `bottom` rather than by a fixed count: the pattern list is unbounded — an
/// adversarial password like `"aaabbbccc…"` yields one entry per run — but the
/// panel is not, and rows drawn past `bottom` are invisible. When the list does
/// not fit, the last row that does is spent on a count of what was left out; a
/// user who cannot see that four more patterns were found reads the truncated
/// list as the whole answer.
///
/// This returns the cursor rather than taking `&mut cy` so that the height the
/// list occupies is derived from the rows it actually emitted — the caller
/// cannot disagree with it about how much space was used.
fn render_pattern_list(
    cmds: &mut Vec<RenderCommand>,
    patterns: &[PatternMatch],
    x: f32,
    top: f32,
    bottom: f32,
    max_width: f32,
) -> f32 {
    let total = patterns.len();
    let room = rows_that_fit(top, bottom, PATTERN_ROW_HEIGHT);
    let overflowing = total > room;
    // The marker costs a row, so it displaces a pattern.
    let shown = if overflowing {
        room.saturating_sub(1)
    } else {
        total
    };
    let mut cy = top;
    for pattern in patterns.iter().take(shown) {
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!("[{}] {}", pattern.kind.label(), pattern.description),
            color: PEACH,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += PATTERN_ROW_HEIGHT;
    }
    if overflowing {
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!("+{} more", total.saturating_sub(shown)),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += PATTERN_ROW_HEIGHT;
    }
    cy
}

// ============================================================================
// Character sets
// ============================================================================

const LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!@#$%^&*()-_=+[]{}|;:',.<>?/~`";
const AMBIGUOUS: &str = "0O1lI|";

/// Word list for passphrase generation (subset of EFF Diceware).
const WORD_LIST: &[&str] = &[
    "abandon", "ability", "able", "about", "above", "absent", "absorb", "abstract", "absurd",
    "abuse", "access", "accident", "account", "accuse", "achieve", "acid", "across", "action",
    "actor", "actual", "adapt", "address", "adjust", "admit", "adult", "advance", "advice",
    "affair", "afford", "afraid", "again", "agent", "agree", "ahead", "airport", "alarm", "album",
    "alert", "alien", "allow", "almost", "alone", "alpha", "already", "alter", "always", "amateur",
    "amazing", "among", "amount", "amused", "anchor", "ancient", "anger", "angle", "angry",
    "animal", "ankle", "annual", "another", "answer", "antenna", "antique", "anxiety", "apart",
    "apology", "appear", "apple", "approve", "april", "arctic", "arena", "argue", "armor", "army",
    "arrange", "arrest", "arrive", "arrow", "artist", "asthma", "atom", "attack", "attend",
    "attract", "auction", "august", "aunt", "autumn", "average", "avoid", "awake", "awesome",
    "awful", "axis", "baby", "bachelor", "bacon", "badge", "balance", "balcony", "bamboo",
    "banana", "banner", "barely", "bargain", "barrel", "basket", "battle", "beach", "beauty",
    "become", "before", "begin", "behave", "behind", "believe", "bench", "benefit", "best",
    "betray", "beyond", "bicycle", "bird", "bitter", "blade", "blanket", "blast", "blaze", "bleak",
    "bless", "blind", "blood", "blossom", "blue", "blur", "board", "boat", "bonus", "book",
    "border", "boring", "borrow", "bottom", "bounce", "box", "bracket", "brain", "brand", "brave",
    "bread", "bridge", "brief", "bright", "bring", "broken", "brother", "brown", "brush", "bubble",
    "buddy", "budget", "buffalo", "build", "bullet", "bundle", "burden", "burger", "burst",
    "butter", "cabin", "cable", "cactus", "cage", "camera", "camp", "canal", "cancel", "candy",
    "cannon", "canvas", "canyon", "captain", "carbon", "cargo", "carpet", "carry", "castle",
    "casual", "catalog", "catch", "cattle", "caught", "cause", "caution", "cave", "ceiling",
    "celery", "cement", "census", "century", "cereal", "certain", "chair", "chalk", "chapter",
    "charge", "chase", "cheap", "check", "cheese", "cherry", "chest", "chicken", "chief",
    "chimney", "choice", "chunk", "circle", "citizen", "civil", "claim", "clap", "clarify",
    "classic", "clean", "clever", "cliff", "climb", "clinic", "clock", "close", "cloud", "clown",
    "cluster", "coach", "coast", "coconut", "coffee", "collect", "color", "column", "combine",
    "comfort", "common", "company", "concept", "conduct", "confirm", "connect", "correct", "couch",
    "country", "couple", "course", "cousin", "cover", "coyote", "cradle", "craft", "crane",
    "crash", "crater", "crawl", "crazy", "cream", "credit", "creek", "crew", "cricket", "crime",
    "crisp", "critic", "crop", "cross", "crowd", "cruel", "cruise", "crumble", "crush", "crystal",
    "culture", "cupboard", "curious", "current", "curtain", "curve", "custom", "cycle", "damage",
    "dance", "danger", "daring", "dawn", "debate", "decade", "december", "decide", "decline",
    "decorate", "decrease", "deer", "defense", "define", "defy", "degree", "delay", "deliver",
    "demand", "denial", "dentist", "deny", "depart", "depend", "deposit", "depth", "derive",
    "describe", "desert", "design", "detect", "develop", "device", "devote", "diagram", "diamond",
    "diary", "diesel", "differ", "digital", "dignity", "dilemma", "dinner", "dinosaur", "direct",
    "dirt", "discover", "disease", "dish", "dismiss", "display", "distance", "divert", "dizzy",
    "doctor", "dolphin", "domain", "donate", "donkey", "donor", "door", "double", "dragon",
    "drama", "dream", "dress", "drift", "drink", "drip", "drive", "drop", "drum", "duck", "dumb",
    "dune", "during", "dust", "dutch", "dwarf", "dynamic", "eager", "eagle", "early", "earn",
    "earth", "easily", "echo", "ecology", "economy", "edge", "edit", "educate", "effort", "eight",
    "elbow", "elder", "electric", "elegant", "element", "elephant", "elevator", "elite", "embrace",
    "emerge", "emotion", "employ", "empower", "enable", "endorse", "enemy", "energy", "enforce",
    "engage", "engine", "enjoy", "enough", "ensure", "enter", "entire", "entry", "envelop",
    "episode", "equal", "equip", "erosion", "error", "escape", "essay", "essence", "estate",
    "eternal", "evening", "evidence", "evil", "evolve", "exact", "example", "excess", "exchange",
    "excite", "exclude", "excuse", "execute", "exercise", "exhaust", "exhibit", "exile", "exist",
    "expand", "expect", "expire", "explain", "expose", "express", "extend", "extra", "fabric",
    "face", "faculty", "faint", "faith", "false", "family", "famous", "fancy", "fantasy", "fatal",
    "father", "fatigue", "fault", "favorite", "feature", "february", "federal", "fence",
    "festival", "fetch", "fever", "fiber", "fiction", "field", "figure", "filter", "final",
    "finger", "finish", "fire", "fiscal", "fitness", "flag", "flame", "flash", "flavor", "flight",
    "float", "flock", "floor", "flower", "fluid", "flush", "focus", "foil", "follow", "force",
    "forest", "forget", "forward", "fossil", "foster", "found", "fragile", "frame", "frequent",
    "fresh", "friend", "fringe", "frog", "frozen", "fruit", "fuel", "funny", "furnace", "fury",
    "future", "gadget", "galaxy", "gallery", "garage", "garden", "garlic", "gather", "gauge",
    "general", "genius", "genre", "gentle", "genuine", "gesture", "ghost", "giant", "gift",
    "giggle", "ginger", "giraffe", "glad", "glance", "glass", "globe", "gloom", "glory", "glove",
    "glucose", "goat", "goddess", "golden", "gospel", "gossip", "govern", "grace", "grain",
    "grant", "grape", "grass", "gravity", "great", "green", "grief", "grill", "grocery", "ground",
    "group", "grow", "growth", "guard", "guitar", "gummy",
];

/// Consonants and vowels for pronounceable passwords.
const CONSONANTS: &str = "bcdfghjklmnpqrstvwxyz";
const VOWELS: &str = "aeiou";

// ============================================================================
// Password generation options
// ============================================================================

/// Configuration for password generation.
#[derive(Clone, Debug)]
pub struct PasswordOptions {
    pub length: usize,
    pub use_lowercase: bool,
    pub use_uppercase: bool,
    pub use_digits: bool,
    pub use_symbols: bool,
    pub exclude_ambiguous: bool,
    pub custom_exclude: String,
    pub must_include_each_class: bool,
}

impl Default for PasswordOptions {
    fn default() -> Self {
        Self {
            length: 16,
            use_lowercase: true,
            use_uppercase: true,
            use_digits: true,
            use_symbols: true,
            exclude_ambiguous: false,
            custom_exclude: String::new(),
            must_include_each_class: true,
        }
    }
}

impl PasswordOptions {
    /// Build the character pool based on options.
    pub fn build_pool(&self) -> Vec<char> {
        let mut pool = Vec::new();
        if self.use_lowercase {
            pool.extend(LOWERCASE.chars());
        }
        if self.use_uppercase {
            pool.extend(UPPERCASE.chars());
        }
        if self.use_digits {
            pool.extend(DIGITS.chars());
        }
        if self.use_symbols {
            pool.extend(SYMBOLS.chars());
        }

        // Remove ambiguous characters
        if self.exclude_ambiguous {
            pool.retain(|c| !AMBIGUOUS.contains(*c));
        }

        // Remove custom exclusions
        if !self.custom_exclude.is_empty() {
            pool.retain(|c| !self.custom_exclude.contains(*c));
        }

        pool
    }

    /// Count the number of active character classes.
    pub fn active_classes(&self) -> usize {
        let mut count = 0usize;
        if self.use_lowercase {
            count = count.saturating_add(1);
        }
        if self.use_uppercase {
            count = count.saturating_add(1);
        }
        if self.use_digits {
            count = count.saturating_add(1);
        }
        if self.use_symbols {
            count = count.saturating_add(1);
        }
        count
    }

    /// Calculate entropy per character (log2 of pool size).
    pub fn entropy_per_char(&self) -> f64 {
        let pool = self.build_pool();
        if pool.is_empty() {
            return 0.0;
        }
        (pool.len() as f64).log2()
    }

    /// Calculate total entropy for the password.
    pub fn total_entropy(&self) -> f64 {
        self.entropy_per_char() * self.length as f64
    }
}

// ============================================================================
// Passphrase options
// ============================================================================

#[derive(Clone, Debug)]
pub struct PassphraseOptions {
    pub word_count: usize,
    pub separator: String,
    pub capitalize: bool,
    pub add_number: bool,
    pub add_symbol: bool,
}

impl Default for PassphraseOptions {
    fn default() -> Self {
        Self {
            word_count: 4,
            separator: "-".to_owned(),
            capitalize: true,
            add_number: true,
            add_symbol: false,
        }
    }
}

impl PassphraseOptions {
    /// Entropy for a passphrase (`log2(word_list_size)` per word).
    pub fn entropy(&self) -> f64 {
        let bits_per_word = (WORD_LIST.len() as f64).log2();
        let mut total = bits_per_word * self.word_count as f64;
        if self.add_number {
            total += (10.0_f64).log2(); // One digit
        }
        if self.add_symbol {
            total += (SYMBOLS.len() as f64).log2();
        }
        total
    }
}

// ============================================================================
// Where the randomness comes from
// ============================================================================
//
// This used to be a hand-rolled xorshift64 seeded from a `u64` the caller
// passed in — and `main` passed the literal `42`. Every user on every machine
// therefore got the *same* passwords, PINs and passphrases, in the same order,
// from the first launch onwards. Even seeded from a clock it would have been
// wrong: xorshift is trivially invertible, so one generated password reveals
// the state and hence every other password that session.
//
// The fix is not a better PRNG, it is the right *kind* of source: a secret
// comes from the kernel CSPRNG or it does not get generated at all. The
// generators below are written against `guitk::rng::RandomSource` so that the
// tests can still drive them from a reproducible `SeededRng`, and `AppRandom`
// makes which one is in use a fact the app can check before it shows anything
// to the user.

/// The source of randomness behind a running generator.
///
/// The variants are deliberately not interchangeable: [`Self::is_trustworthy`]
/// is what stands between a seeded test generator and a password shown to a
/// user, and it is consulted both before a secret is drawn and after.
#[derive(Debug)]
pub enum AppRandom {
    /// The kernel CSPRNG — the only source a real secret may come from.
    ///
    /// Boxed because its buffer dwarfs the other variants; an app holds one
    /// of these for its whole life, so the indirection costs nothing that
    /// matters and keeps the enum pointer-sized.
    System(Box<SystemRandom>),
    /// A reproducible generator. Tests only; never shown to a user.
    Seeded(SeededRng),
    /// The kernel CSPRNG did not answer. Generation is refused outright
    /// rather than falling back to something weaker, because a password the
    /// user believes is random and is not is worse than no password at all.
    Unavailable,
}

impl AppRandom {
    /// Open the kernel CSPRNG, or record that it could not be opened.
    #[must_use]
    pub fn from_system() -> Self {
        match SystemRandom::open() {
            Ok(source) => Self::System(Box::new(source)),
            Err(_) => Self::Unavailable,
        }
    }

    /// A reproducible source, for tests.
    #[must_use]
    pub const fn seeded(seed: u64) -> Self {
        Self::Seeded(SeededRng::new(seed))
    }
}

/// Both sides of the draw are checked by [`SecretSource::secret`], which is
/// where the rule now lives — this crate, `apps/credmanager` and
/// `gui/credentials` each used to carry their own copy of it.
impl SecretSource for AppRandom {
    /// False for [`Self::Unavailable`], and false for a [`Self::System`]
    /// source whose refill has failed at any point — including part-way
    /// through the secret currently being built, which is why `secret` checks
    /// this *after* generating as well as before.
    fn is_trustworthy(&self) -> bool {
        match self {
            Self::System(source) => source.is_healthy(),
            // A seeded generator always produces what it promises; it is just
            // not a secret. Callers that must have real entropy check
            // `is_system` instead.
            Self::Seeded(_) => true,
            Self::Unavailable => false,
        }
    }
}

impl RandomSource for AppRandom {
    fn next_u64(&mut self) -> u64 {
        match self {
            Self::System(source) => source.next_u64(),
            Self::Seeded(source) => source.next_u64(),
            // Unreachable through `secret`, which refuses first. Zero is
            // returned rather than anything that could pass for random.
            Self::Unavailable => 0,
        }
    }
}

/// One of `chars`, or `'?'` if there are none.
fn pick_char<R: RandomSource>(rng: &mut R, chars: &[char]) -> char {
    rng.choose(chars).copied().unwrap_or('?')
}

// ============================================================================
// Password generators
// ============================================================================

/// Generate a password using the given options and randomness.
pub fn generate_password<R: RandomSource>(opts: &PasswordOptions, rng: &mut R) -> String {
    let pool = opts.build_pool();
    if pool.is_empty() || opts.length == 0 {
        return String::new();
    }

    let mut password: Vec<char> = Vec::with_capacity(opts.length);

    // If must_include_each_class, place one from each active class first
    if opts.must_include_each_class && opts.length >= opts.active_classes() {
        let classes: Vec<Vec<char>> = [
            if opts.use_lowercase {
                Some(LOWERCASE.chars().collect::<Vec<_>>())
            } else {
                None
            },
            if opts.use_uppercase {
                Some(UPPERCASE.chars().collect::<Vec<_>>())
            } else {
                None
            },
            if opts.use_digits {
                Some(DIGITS.chars().collect::<Vec<_>>())
            } else {
                None
            },
            if opts.use_symbols {
                Some(SYMBOLS.chars().collect::<Vec<_>>())
            } else {
                None
            },
        ]
        .into_iter()
        .flatten()
        .collect();

        for class in &classes {
            let mut filtered = class.clone();
            if opts.exclude_ambiguous {
                filtered.retain(|c| !AMBIGUOUS.contains(*c));
            }
            if !filtered.is_empty() {
                password.push(pick_char(rng, &filtered));
            }
        }
    }

    // Fill remaining with random characters from the full pool
    while password.len() < opts.length {
        password.push(pick_char(rng, &pool));
    }

    // Shuffle the password (Fisher-Yates)
    let len = password.len();
    for i in (1..len).rev() {
        let j = rng.below(i.saturating_add(1));
        password.swap(i, j);
    }

    password.into_iter().collect()
}

/// Generate a passphrase.
pub fn generate_passphrase<R: RandomSource>(opts: &PassphraseOptions, rng: &mut R) -> String {
    let mut words: Vec<String> = Vec::with_capacity(opts.word_count);

    for _ in 0..opts.word_count {
        let word = rng
            .choose(WORD_LIST)
            .copied()
            .unwrap_or("unknown")
            .to_owned();
        if opts.capitalize {
            let mut chars = word.chars();
            let capitalized = match chars.next() {
                Some(c) => {
                    let mut s = c.to_uppercase().to_string();
                    s.push_str(chars.as_str());
                    s
                }
                None => word,
            };
            words.push(capitalized);
        } else {
            words.push(word);
        }
    }

    let mut result = words.join(&opts.separator);

    if opts.add_number {
        let digit = rng.below(10);
        result.push_str(&digit.to_string());
    }
    if opts.add_symbol {
        let sym_chars: Vec<char> = SYMBOLS.chars().collect();
        result.push(pick_char(rng, &sym_chars));
    }

    result
}

/// Generate a PIN.
pub fn generate_pin<R: RandomSource>(length: usize, rng: &mut R) -> String {
    let digits: Vec<char> = DIGITS.chars().collect();
    (0..length).map(|_| pick_char(rng, &digits)).collect()
}

/// Generate a pronounceable password (alternating consonant-vowel).
pub fn generate_pronounceable<R: RandomSource>(length: usize, rng: &mut R) -> String {
    let consonants: Vec<char> = CONSONANTS.chars().collect();
    let vowels: Vec<char> = VOWELS.chars().collect();
    let mut result = String::with_capacity(length);
    for i in 0..length {
        if i % 2 == 0 {
            result.push(pick_char(rng, &consonants));
        } else {
            result.push(pick_char(rng, &vowels));
        }
    }
    result
}

// ============================================================================
// Password strength analysis
// ============================================================================

/// Strength rating.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrengthRating {
    VeryWeak,
    Weak,
    Fair,
    Strong,
    VeryStrong,
}

impl StrengthRating {
    pub fn label(self) -> &'static str {
        match self {
            Self::VeryWeak => "Very Weak",
            Self::Weak => "Weak",
            Self::Fair => "Fair",
            Self::Strong => "Strong",
            Self::VeryStrong => "Very Strong",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::VeryWeak => RED,
            Self::Weak => PEACH,
            Self::Fair => YELLOW,
            Self::Strong => GREEN,
            Self::VeryStrong => TEAL,
        }
    }

    pub fn score(self) -> u8 {
        match self {
            Self::VeryWeak => 1,
            Self::Weak => 2,
            Self::Fair => 3,
            Self::Strong => 4,
            Self::VeryStrong => 5,
        }
    }
}

/// Full analysis result.
#[derive(Clone, Debug)]
pub struct PasswordAnalysis {
    pub length: usize,
    pub entropy_bits: f64,
    pub rating: StrengthRating,
    pub crack_time: CrackTime,
    pub has_lowercase: bool,
    pub has_uppercase: bool,
    pub has_digits: bool,
    pub has_symbols: bool,
    pub char_classes_used: usize,
    pub patterns_found: Vec<PatternMatch>,
    pub is_common: bool,
    pub score: u8,
}

/// Detected pattern in a password.
#[derive(Clone, Debug)]
pub struct PatternMatch {
    pub kind: PatternKind,
    pub description: String,
    pub penalty_bits: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternKind {
    DictionaryWord,
    KeyboardSequence,
    RepeatedChars,
    SequentialChars,
    CommonPassword,
    DatePattern,
}

impl PatternKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::DictionaryWord => "Dictionary Word",
            Self::KeyboardSequence => "Keyboard Sequence",
            Self::RepeatedChars => "Repeated Characters",
            Self::SequentialChars => "Sequential Characters",
            Self::CommonPassword => "Common Password",
            Self::DatePattern => "Date Pattern",
        }
    }
}

/// Estimated crack time at various speeds.
#[derive(Clone, Debug)]
pub struct CrackTime {
    pub online_throttled: String,
    pub online_unthrottled: String,
    pub offline_slow: String,
    pub offline_fast: String,
}

impl CrackTime {
    pub fn from_entropy(entropy: f64) -> Self {
        // Guesses = 2^entropy (on average, half the keyspace)
        let guesses = 2.0_f64.powf(entropy) / 2.0;

        Self {
            online_throttled: format_crack_time(guesses, 10.0),
            online_unthrottled: format_crack_time(guesses, 100.0),
            offline_slow: format_crack_time(guesses, 10_000.0),
            offline_fast: format_crack_time(guesses, 10_000_000_000.0),
        }
    }
}

fn format_crack_time(guesses: f64, rate_per_sec: f64) -> String {
    if rate_per_sec <= 0.0 {
        return "N/A".to_owned();
    }
    let seconds = guesses / rate_per_sec;

    if seconds < 1.0 {
        return "Instant".to_owned();
    }
    if seconds < 60.0 {
        return format!("{seconds:.0} seconds");
    }
    let minutes = seconds / 60.0;
    if minutes < 60.0 {
        return format!("{minutes:.0} minutes");
    }
    let hours = minutes / 60.0;
    if hours < 24.0 {
        return format!("{hours:.0} hours");
    }
    let days = hours / 24.0;
    if days < 365.0 {
        return format!("{days:.0} days");
    }
    let years = days / 365.25;
    if years < 1_000.0 {
        return format!("{years:.0} years");
    }
    if years < 1_000_000.0 {
        return format!("{:.0} thousand years", years / 1_000.0);
    }
    if years < 1_000_000_000.0 {
        return format!("{:.0} million years", years / 1_000_000.0);
    }
    format!("{:.0} billion years", years / 1_000_000_000.0)
}

/// Analyze a password's strength.
pub fn analyze_password(password: &str) -> PasswordAnalysis {
    let length = password.len();
    let has_lowercase = password.chars().any(|c| c.is_ascii_lowercase());
    let has_uppercase = password.chars().any(|c| c.is_ascii_uppercase());
    let has_digits = password.chars().any(|c| c.is_ascii_digit());
    let has_symbols = password.chars().any(|c| !c.is_ascii_alphanumeric());

    let mut classes = 0usize;
    if has_lowercase {
        classes = classes.saturating_add(1);
    }
    if has_uppercase {
        classes = classes.saturating_add(1);
    }
    if has_digits {
        classes = classes.saturating_add(1);
    }
    if has_symbols {
        classes = classes.saturating_add(1);
    }

    // Calculate pool size based on actual character classes
    let mut pool_size = 0usize;
    if has_lowercase {
        pool_size = pool_size.saturating_add(26);
    }
    if has_uppercase {
        pool_size = pool_size.saturating_add(26);
    }
    if has_digits {
        pool_size = pool_size.saturating_add(10);
    }
    if has_symbols {
        pool_size = pool_size.saturating_add(30);
    }

    let entropy = if pool_size > 0 && length > 0 {
        (pool_size as f64).log2() * length as f64
    } else {
        0.0
    };

    // Pattern detection
    let mut patterns = Vec::new();
    detect_patterns(password, &mut patterns);

    // Penalty for patterns
    let pattern_penalty: f64 = patterns.iter().map(|p| p.penalty_bits).sum();
    let effective_entropy = (entropy - pattern_penalty).max(0.0);

    // Check against common passwords
    let is_common = is_common_password(password);
    let final_entropy = if is_common { 0.0 } else { effective_entropy };

    // Rating based on entropy
    let rating = if final_entropy < 25.0 {
        StrengthRating::VeryWeak
    } else if final_entropy < 40.0 {
        StrengthRating::Weak
    } else if final_entropy < 60.0 {
        StrengthRating::Fair
    } else if final_entropy < 80.0 {
        StrengthRating::Strong
    } else {
        StrengthRating::VeryStrong
    };

    let crack_time = CrackTime::from_entropy(final_entropy);

    PasswordAnalysis {
        length,
        entropy_bits: final_entropy,
        rating,
        crack_time,
        has_lowercase,
        has_uppercase,
        has_digits,
        has_symbols,
        char_classes_used: classes,
        patterns_found: patterns,
        is_common,
        score: rating.score(),
    }
}

/// Detect patterns in a password.
fn detect_patterns(password: &str, patterns: &mut Vec<PatternMatch>) {
    let lower = password.to_lowercase();

    // Repeated characters (3+)
    let chars: Vec<char> = password.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars.get(i).copied().unwrap_or('\0');
        let mut count = 1usize;
        while i.saturating_add(count) < chars.len()
            && chars.get(i.saturating_add(count)).copied() == Some(ch)
        {
            count = count.saturating_add(1);
        }
        if count >= 3 {
            patterns.push(PatternMatch {
                kind: PatternKind::RepeatedChars,
                description: format!("'{ch}' repeated {count} times"),
                penalty_bits: (count as f64 - 1.0) * 3.0,
            });
        }
        i = i.saturating_add(count);
    }

    // Sequential characters (abc, 123, etc.)
    let mut seq_len = 1usize;
    for idx in 1..chars.len() {
        let prev = chars.get(idx.saturating_sub(1)).copied().unwrap_or('\0');
        let curr = chars.get(idx).copied().unwrap_or('\0');
        if curr as u32 == (prev as u32).saturating_add(1) {
            seq_len = seq_len.saturating_add(1);
        } else {
            if seq_len >= 3 {
                patterns.push(PatternMatch {
                    kind: PatternKind::SequentialChars,
                    description: format!("{seq_len} sequential characters"),
                    penalty_bits: seq_len as f64 * 2.0,
                });
            }
            seq_len = 1;
        }
    }
    if seq_len >= 3 {
        patterns.push(PatternMatch {
            kind: PatternKind::SequentialChars,
            description: format!("{seq_len} sequential characters"),
            penalty_bits: seq_len as f64 * 2.0,
        });
    }

    // Keyboard sequences
    let keyboard_sequences = [
        "qwerty",
        "asdfgh",
        "zxcvbn",
        "qweasd",
        "1234567890",
        "!@#$%^",
        "poiuyt",
        "lkjhgf",
    ];
    for seq in &keyboard_sequences {
        if lower.contains(seq) {
            patterns.push(PatternMatch {
                kind: PatternKind::KeyboardSequence,
                description: format!("Keyboard sequence: {seq}"),
                penalty_bits: 10.0,
            });
        }
    }

    // Simple dictionary word check (from our word list)
    if lower.len() >= 4 {
        for word in WORD_LIST {
            if word.len() >= 4 && lower.contains(word) {
                patterns.push(PatternMatch {
                    kind: PatternKind::DictionaryWord,
                    description: format!("Contains word: {word}"),
                    penalty_bits: 5.0,
                });
                break; // Only report first match
            }
        }
    }

    // Date patterns (YYYY, MMDD, etc.)
    let date_patterns = ["19", "20", "2024", "2025", "2026", "1234", "0000"];
    for dp in &date_patterns {
        if lower.contains(dp) {
            patterns.push(PatternMatch {
                kind: PatternKind::DatePattern,
                description: format!("Date-like pattern: {dp}"),
                penalty_bits: 3.0,
            });
            break;
        }
    }
}

/// Check if a password is in the common passwords list.
fn is_common_password(password: &str) -> bool {
    let common = [
        "password",
        "123456",
        "12345678",
        "qwerty",
        "abc123",
        "monkey",
        "1234567",
        "letmein",
        "trustno1",
        "dragon",
        "baseball",
        "iloveyou",
        "master",
        "sunshine",
        "ashley",
        "bailey",
        "shadow",
        "123123",
        "654321",
        "superman",
        "qazwsx",
        "michael",
        "football",
        "password1",
        "password123",
        "admin",
        "welcome",
        "login",
        "princess",
        "starwars",
    ];
    let lower = password.to_lowercase();
    common.iter().any(|c| *c == lower)
}

// ============================================================================
// Password policy
// ============================================================================

/// Policy rules for password compliance checking.
#[derive(Clone, Debug)]
pub struct PasswordPolicy {
    pub min_length: usize,
    pub max_length: Option<usize>,
    pub require_lowercase: bool,
    pub require_uppercase: bool,
    pub require_digit: bool,
    pub require_symbol: bool,
    pub min_classes: usize,
    pub min_entropy: f64,
    pub disallow_common: bool,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            min_length: 8,
            max_length: None,
            require_lowercase: true,
            require_uppercase: true,
            require_digit: true,
            require_symbol: false,
            min_classes: 3,
            min_entropy: 40.0,
            disallow_common: true,
        }
    }
}

impl PasswordPolicy {
    /// Check compliance, returning a list of violations.
    pub fn check(&self, password: &str) -> Vec<String> {
        let analysis = analyze_password(password);
        let mut violations = Vec::new();

        if password.len() < self.min_length {
            violations.push(format!("Too short (minimum {} chars)", self.min_length));
        }
        if let Some(max) = self.max_length
            && password.len() > max
        {
            violations.push(format!("Too long (maximum {max} chars)"));
        }
        if self.require_lowercase && !analysis.has_lowercase {
            violations.push("Must contain lowercase letter".to_owned());
        }
        if self.require_uppercase && !analysis.has_uppercase {
            violations.push("Must contain uppercase letter".to_owned());
        }
        if self.require_digit && !analysis.has_digits {
            violations.push("Must contain digit".to_owned());
        }
        if self.require_symbol && !analysis.has_symbols {
            violations.push("Must contain symbol".to_owned());
        }
        if analysis.char_classes_used < self.min_classes {
            violations.push(format!(
                "Must use at least {} character classes (using {})",
                self.min_classes, analysis.char_classes_used
            ));
        }
        if analysis.entropy_bits < self.min_entropy {
            violations.push(format!(
                "Entropy too low ({:.0} bits, minimum {:.0})",
                analysis.entropy_bits, self.min_entropy
            ));
        }
        if self.disallow_common && analysis.is_common {
            violations.push("Password is commonly used".to_owned());
        }

        violations
    }

    pub fn is_compliant(&self, password: &str) -> bool {
        self.check(password).is_empty()
    }
}

// ============================================================================
// History entry
// ============================================================================

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub password: String,
    pub strength: StrengthRating,
    pub entropy: f64,
    pub gen_type: String,
    pub timestamp: u64,
}

// ============================================================================
// Active tab
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveTab {
    Generator,
    Analyzer,
    History,
}

impl ActiveTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Generator => "Generator",
            Self::Analyzer => "Analyzer",
            Self::History => "History",
        }
    }
}

// ============================================================================
// Main application
// ============================================================================

/// The password generator/analyzer application.
/// The ranges the length keys move within.
///
/// The generators themselves impose no bound — `generate_password` will happily
/// be asked for a million characters, and `generate_pin` for zero. These are
/// the limits of the *control*, chosen so a held-down arrow key cannot put the
/// app somewhere useless: a password shorter than eight characters is not worth
/// generating, and one longer than 128 does not fit the field it is drawn in.
const MIN_PASSWORD_LEN: usize = 8;
const MAX_PASSWORD_LEN: usize = 128;
/// Four words is the familiar diceware minimum; twelve is where the phrase
/// stops fitting on one line.
const MIN_WORDS: usize = 3;
const MAX_WORDS: usize = 12;
/// A PIN below four digits is not a PIN; twelve is the longest any card asks
/// for.
const MIN_PIN_LEN: usize = 4;
const MAX_PIN_LEN: usize = 12;
/// Bulk generation is bounded by what the list can show without scrolling
/// becoming the point of the tab.
const MAX_BULK: usize = 100;

/// Move `value` by `delta`, staying within `[lo, hi]`.
///
/// A free function because four generator kinds need the same clamp on four
/// different fields, and four copies of a saturating step is four chances for one of them
/// to have the wrong bound.
fn step(value: usize, delta: isize, lo: usize, hi: usize) -> usize {
    let moved = if delta < 0 {
        value.saturating_sub(delta.unsigned_abs())
    } else {
        value.saturating_add(delta.unsigned_abs())
    };
    moved.clamp(lo, hi)
}

/// What the generator tab is currently producing.
///
/// The three `ActiveTab` values are the *screens*; this is the choice within
/// the generator screen. It exists because the length arrows and the "another
/// one" key both need to know which of the five generators they are talking
/// about, and before there was any input at all nothing had to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenKind {
    Password,
    Passphrase,
    Pin,
    Pronounceable,
    Bulk,
}

pub struct PasswordApp {
    pub password_opts: PasswordOptions,
    pub passphrase_opts: PassphraseOptions,
    pub current_password: String,
    pub current_analysis: Option<PasswordAnalysis>,
    pub analyzer_input: String,
    pub history: Vec<HistoryEntry>,
    pub policy: PasswordPolicy,
    pub active_tab: ActiveTab,
    /// What the generator tab is producing; see [`GenKind`].
    pub gen_kind: GenKind,
    pub pin_length: usize,
    pub bulk_count: usize,
    pub bulk_results: Vec<String>,
    pub window_width: f32,
    pub window_height: f32,
    /// Set when a generation was refused because the kernel CSPRNG was not
    /// available. Shown in place of the password, so that the refusal is
    /// visible rather than looking like a button that did nothing.
    pub last_error: Option<String>,
    rng: AppRandom,
    timestamp: u64,
}

/// What the user is told when there is no entropy to generate from.
pub const NO_ENTROPY_MESSAGE: &str =
    "Cannot generate: the system random number generator is unavailable";

impl Default for PasswordApp {
    /// The same thing [`PasswordApp::new`] builds — note that this opens the
    /// kernel CSPRNG, and yields an app that refuses to generate if it cannot.
    fn default() -> Self {
        Self::new()
    }
}

impl PasswordApp {
    /// The application as the user gets it, drawing from the kernel CSPRNG.
    #[must_use]
    pub fn new() -> Self {
        Self::with_random(AppRandom::from_system())
    }

    /// A reproducible instance, for tests.
    ///
    /// Kept separate from [`new`](Self::new), and named so that no call site
    /// can reach it by accident, because the defect this replaces was exactly
    /// a seeded generator standing in for a real one.
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        Self::with_random(AppRandom::seeded(seed))
    }

    fn with_random(rng: AppRandom) -> Self {
        Self {
            password_opts: PasswordOptions::default(),
            passphrase_opts: PassphraseOptions::default(),
            current_password: String::new(),
            current_analysis: None,
            analyzer_input: String::new(),
            history: Vec::new(),
            policy: PasswordPolicy::default(),
            active_tab: ActiveTab::Generator,
            gen_kind: GenKind::Password,
            pin_length: 6,
            bulk_count: 10,
            bulk_results: Vec::new(),
            window_width: 1100.0,
            window_height: 700.0,
            last_error: None,
            rng,
            timestamp: 1000,
        }
    }

    fn tick(&mut self) -> u64 {
        self.timestamp = self.timestamp.saturating_add(1);
        self.timestamp
    }

    /// Record a freshly-generated secret as the current one.
    fn record(&mut self, secret: String, kind: &str) {
        let analysis = analyze_password(&secret);
        let ts = self.tick();
        self.history.push(HistoryEntry {
            password: secret.clone(),
            strength: analysis.rating,
            entropy: analysis.entropy_bits,
            gen_type: kind.to_owned(),
            timestamp: ts,
        });
        self.current_analysis = Some(analysis);
        self.current_password = secret;
        self.last_error = None;
    }

    /// Report that a generation was refused for want of entropy.
    ///
    /// Nothing at all is recorded — no history entry, no analysis — and any
    /// previously shown password is cleared, so there is no way to mistake a
    /// stale value for the one the button was just pressed for.
    fn refuse(&mut self) {
        self.current_password.clear();
        self.current_analysis = None;
        self.last_error = Some(NO_ENTROPY_MESSAGE.to_owned());
    }

    /// Generate a new password.
    pub fn gen_password(&mut self) {
        match self
            .rng
            .secret(|rng| generate_password(&self.password_opts, rng))
        {
            Some(pw) => self.record(pw, "Password"),
            None => self.refuse(),
        }
    }

    /// Generate a new passphrase.
    pub fn gen_passphrase(&mut self) {
        match self
            .rng
            .secret(|rng| generate_passphrase(&self.passphrase_opts, rng))
        {
            Some(pp) => self.record(pp, "Passphrase"),
            None => self.refuse(),
        }
    }

    /// Generate a PIN.
    pub fn gen_pin(&mut self) {
        match self.rng.secret(|rng| generate_pin(self.pin_length, rng)) {
            Some(pin) => self.record(pin, "PIN"),
            None => self.refuse(),
        }
    }

    /// Generate a pronounceable password.
    pub fn gen_pronounceable(&mut self) {
        match self
            .rng
            .secret(|rng| generate_pronounceable(self.password_opts.length, rng))
        {
            Some(pw) => self.record(pw, "Pronounceable"),
            None => self.refuse(),
        }
    }

    /// Bulk generate passwords.
    pub fn gen_bulk(&mut self) {
        self.bulk_results.clear();
        for _ in 0..self.bulk_count {
            let Some(pw) = self
                .rng
                .secret(|rng| generate_password(&self.password_opts, rng))
            else {
                // Entropy ran out part-way through the batch. Throw away what
                // was produced rather than return a short list the user has no
                // way of telling is short.
                self.bulk_results.clear();
                self.refuse();
                return;
            };
            self.bulk_results.push(pw);
        }
        self.last_error = None;
    }

    /// Analyze a password from the analyzer input.
    pub fn analyze_input(&mut self) {
        self.current_analysis = Some(analyze_password(&self.analyzer_input));
    }

    /// Set analyzer input.
    pub fn set_analyzer_input(&mut self, input: &str) {
        self.analyzer_input = input.to_owned();
    }

    /// Check policy compliance for current password.
    pub fn check_policy(&self) -> Vec<String> {
        self.policy.check(&self.current_password)
    }

    /// Clear history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Export history as text.
    pub fn export_history(&self) -> String {
        let mut out = String::new();
        out.push_str("Password Generation History\n");
        out.push_str("==========================\n\n");
        for (i, entry) in self.history.iter().enumerate() {
            out.push_str(&format!(
                "{}. [{}] {} — {} ({:.0} bits)\n",
                i.saturating_add(1),
                entry.gen_type,
                entry.password,
                entry.strength.label(),
                entry.entropy,
            ));
        }
        out
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------

    /// Route a compositor event into the app.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
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
    /// The app had no input handling at all: every generator, the analyser and
    /// the history were reachable only by a caller invoking the method. On the
    /// analyser tab every printable key is the password being analysed, which
    /// is why that branch comes first — otherwise typing a "p" into a password
    /// would generate a new one instead of measuring the one being typed.
    pub fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        if self.active_tab == ActiveTab::Analyzer {
            match key.key {
                Key::Backspace => {
                    let mut input = self.analyzer_input.clone();
                    if input.pop().is_none() {
                        return EventResult::Ignored;
                    }
                    self.set_analyzer_input(&input);
                    // `set_analyzer_input` only stores the text. Measuring it
                    // is the entire purpose of this tab, so a keystroke that
                    // stored without measuring would leave the strength meter
                    // showing the previous password's score.
                    self.analyze_input();
                    return EventResult::Consumed;
                }
                // The tab keys keep working, or there is no way out of the box.
                Key::Tab | Key::Num1 | Key::Num2 | Key::Num3 => {}
                _ => {
                    if key.text.is_empty() || key.modifiers.ctrl {
                        return EventResult::Ignored;
                    }
                    let mut input = self.analyzer_input.clone();
                    input.push_str(&key.text);
                    self.set_analyzer_input(&input);
                    self.analyze_input();
                    return EventResult::Consumed;
                }
            }
        }
        match key.key {
            Key::Num1 => self.set_tab(ActiveTab::Generator),
            Key::Num2 => self.set_tab(ActiveTab::Analyzer),
            Key::Num3 => self.set_tab(ActiveTab::History),
            Key::Tab => {
                let next = match self.active_tab {
                    ActiveTab::Generator => ActiveTab::Analyzer,
                    ActiveTab::Analyzer => ActiveTab::History,
                    ActiveTab::History => ActiveTab::Generator,
                };
                self.set_tab(next)
            }
            // What to generate. Each key both chooses the kind and produces
            // one, because choosing without producing would leave the field
            // showing the previous kind's output.
            Key::P => self.generate(GenKind::Password),
            Key::W => self.generate(GenKind::Passphrase),
            Key::N => self.generate(GenKind::Pin),
            Key::R => self.generate(GenKind::Pronounceable),
            Key::B => self.generate(GenKind::Bulk),
            // Another one of the same kind. Space and Enter both, because
            // "give me another" is the thing this program is for.
            Key::Space | Key::Enter => {
                let kind = self.gen_kind;
                self.generate(kind)
            }
            // Length, applied to whichever kind is showing.
            Key::Left => self.adjust_length(-1),
            Key::Right => self.adjust_length(1),
            Key::C => {
                if self.history.is_empty() {
                    return EventResult::Ignored;
                }
                self.clear_history();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Switch tabs, reporting whether anything changed.
    fn set_tab(&mut self, tab: ActiveTab) -> EventResult {
        if self.active_tab == tab {
            return EventResult::Ignored;
        }
        self.active_tab = tab;
        EventResult::Consumed
    }

    /// Produce one of `kind`, and remember it as what the arrows now size.
    fn generate(&mut self, kind: GenKind) -> EventResult {
        self.gen_kind = kind;
        // Generating is only meaningful on the generator tab, and pressing a
        // generator key elsewhere plainly means "go and do that".
        self.active_tab = ActiveTab::Generator;
        match kind {
            GenKind::Password => self.gen_password(),
            GenKind::Passphrase => self.gen_passphrase(),
            GenKind::Pin => self.gen_pin(),
            GenKind::Pronounceable => self.gen_pronounceable(),
            GenKind::Bulk => self.gen_bulk(),
        }
        EventResult::Consumed
    }

    /// Lengthen or shorten what the current kind produces, and produce one.
    ///
    /// Each length is clamped to the range its own control accepts, so a
    /// held-down arrow cannot ask for a one-character password or a
    /// thousand-word passphrase.
    fn adjust_length(&mut self, delta: isize) -> EventResult {
        let changed = match self.gen_kind {
            GenKind::Password | GenKind::Pronounceable => {
                let before = self.password_opts.length;
                self.password_opts.length = step(before, delta, MIN_PASSWORD_LEN, MAX_PASSWORD_LEN);
                self.password_opts.length != before
            }
            GenKind::Passphrase => {
                let before = self.passphrase_opts.word_count;
                self.passphrase_opts.word_count = step(before, delta, MIN_WORDS, MAX_WORDS);
                self.passphrase_opts.word_count != before
            }
            GenKind::Pin => {
                let before = self.pin_length;
                self.pin_length = step(before, delta, MIN_PIN_LEN, MAX_PIN_LEN);
                self.pin_length != before
            }
            GenKind::Bulk => {
                let before = self.bulk_count;
                self.bulk_count = step(before, delta, 1, MAX_BULK);
                self.bulk_count != before
            }
        };
        if !changed {
            return EventResult::Ignored;
        }
        let kind = self.gen_kind;
        self.generate(kind)
    }

    /// Named `render_commands` and not `render`: this takes a width and a
    /// height, exactly as `oswindow::app::App::render` does, and at equal arity
    /// an inherent method silently wins method lookup over the trait's — so an
    /// app that keeps the name draws nothing and reports no error.
    pub fn render_commands(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

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

        // Left panel: generator/controls
        self.render_left_panel(&mut cmds, content_y, content_h);

        // Right panel: results/analysis
        let right_x = LEFT_PANEL_WIDTH;
        let right_w = width - LEFT_PANEL_WIDTH;
        self.render_right_panel(&mut cmds, right_x, content_y, right_w, content_h);

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

        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "Password Generator".to_owned(),
            color: BLUE,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Tab buttons
        let tabs = [
            ActiveTab::Generator,
            ActiveTab::Analyzer,
            ActiveTab::History,
        ];
        let mut tx = 220.0;
        for tab in &tabs {
            let is_active = *tab == self.active_tab;
            let btn_w = text::padded_width_any_weight(tab.label(), 10.0, 11.0);
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
                text: tab.label().to_owned(),
                color: if is_active { BLUE } else { SUBTEXT0 },
                font_size: 11.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(btn_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            tx += btn_w + 4.0;
        }

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

        let status = format!(
            "{} passwords generated  |  Policy: {}",
            self.history.len(),
            if self.policy.is_compliant(&self.current_password) {
                "Compliant"
            } else {
                "Non-compliant"
            },
        );
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: bar_y + 6.0,
            text: status,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_left_panel(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: LEFT_PANEL_WIDTH,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Line {
            x1: LEFT_PANEL_WIDTH,
            y1: y,
            x2: LEFT_PANEL_WIDTH,
            y2: y + height,
            color: SURFACE0,
            width: 1.0,
        });

        let mut cy = y + 12.0;
        let lx = 12.0;
        let max_w = LEFT_PANEL_WIDTH - 24.0;

        // Current password display
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "GENERATED PASSWORD".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 18.0;

        cmds.push(RenderCommand::FillRect {
            x: lx,
            y: cy,
            width: max_w,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        // A refusal takes this slot: the user pressed Generate, so the answer
        // to "where is my password" belongs where the password would be, not
        // in a corner they have no reason to look at.
        let (pw_display, pw_color) = match (&self.last_error, self.current_password.is_empty()) {
            (Some(message), _) => (message.clone(), RED),
            (None, true) => ("Click Generate to create a password".to_owned(), OVERLAY0),
            (None, false) => (self.current_password.clone(), TEXT),
        };
        cmds.push(RenderCommand::Text {
            x: lx + 8.0,
            y: cy + 9.0,
            text: pw_display,
            color: pw_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w - 16.0),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 44.0;

        // Generation buttons
        let buttons = [
            ("Generate Password", GREEN),
            ("Generate Passphrase", TEAL),
            ("Generate PIN", YELLOW),
            ("Pronounceable", MAUVE),
        ];

        for (label, color) in &buttons {
            let btn_w = text::padded_width(label, 12.0, 11.0, FontWeightHint::Bold);
            cmds.push(RenderCommand::FillRect {
                x: lx,
                y: cy,
                width: btn_w.min(max_w),
                height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: lx + 12.0,
                y: cy + 8.0,
                text: (*label).to_owned(),
                color: *color,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(btn_w - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 32.0;
        }

        cy += 12.0;

        // Options
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "OPTIONS".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 18.0;

        let options = [
            (format!("Length: {}", self.password_opts.length), true),
            (
                format!(
                    "Lowercase: {}",
                    if self.password_opts.use_lowercase {
                        "Yes"
                    } else {
                        "No"
                    }
                ),
                self.password_opts.use_lowercase,
            ),
            (
                format!(
                    "Uppercase: {}",
                    if self.password_opts.use_uppercase {
                        "Yes"
                    } else {
                        "No"
                    }
                ),
                self.password_opts.use_uppercase,
            ),
            (
                format!(
                    "Digits: {}",
                    if self.password_opts.use_digits {
                        "Yes"
                    } else {
                        "No"
                    }
                ),
                self.password_opts.use_digits,
            ),
            (
                format!(
                    "Symbols: {}",
                    if self.password_opts.use_symbols {
                        "Yes"
                    } else {
                        "No"
                    }
                ),
                self.password_opts.use_symbols,
            ),
            (
                format!(
                    "Exclude Ambiguous: {}",
                    if self.password_opts.exclude_ambiguous {
                        "Yes"
                    } else {
                        "No"
                    }
                ),
                self.password_opts.exclude_ambiguous,
            ),
        ];

        for (label, active) in &options {
            let text_color = if *active { TEXT } else { OVERLAY0 };
            cmds.push(RenderCommand::Text {
                x: lx + 8.0,
                y: cy,
                text: label.clone(),
                color: text_color,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 18.0;
        }
    }

    fn render_right_panel(
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

        match self.active_tab {
            ActiveTab::Generator | ActiveTab::Analyzer => {
                // Analysis results
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y: cy,
                    text: "STRENGTH ANALYSIS".to_owned(),
                    color: OVERLAY0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(max_w),
                    overflow: TextOverflow::Ellipsis,
                });
                cy += 22.0;

                if let Some(ref analysis) = self.current_analysis {
                    // Rating badge
                    let badge_w = text::padded_width(
                        analysis.rating.label(),
                        10.0,
                        13.0,
                        FontWeightHint::Bold,
                    );
                    cmds.push(RenderCommand::FillRect {
                        x: lx,
                        y: cy,
                        width: badge_w,
                        height: 28.0,
                        color: analysis.rating.color(),
                        corner_radii: CornerRadii::all(CORNER_RADIUS),
                    });
                    cmds.push(RenderCommand::Text {
                        x: lx + 10.0,
                        y: cy + 8.0,
                        text: analysis.rating.label().to_owned(),
                        color: CRUST,
                        font_size: 13.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(badge_w - 16.0),
                        overflow: TextOverflow::Ellipsis,
                    });

                    // Score
                    cmds.push(RenderCommand::Text {
                        x: lx + badge_w + 12.0,
                        y: cy + 8.0,
                        text: format!("Score: {}/5", analysis.score),
                        color: TEXT,
                        font_size: 13.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                    cy += 40.0;

                    // Stats
                    let stats_lines = [
                        format!("Length: {} characters", analysis.length),
                        format!("Entropy: {:.1} bits", analysis.entropy_bits),
                        format!("Character classes: {}/4", analysis.char_classes_used),
                    ];
                    for line in &stats_lines {
                        cmds.push(RenderCommand::Text {
                            x: lx,
                            y: cy,
                            text: line.clone(),
                            color: TEXT,
                            font_size: 12.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(max_w),
                            overflow: TextOverflow::Ellipsis,
                        });
                        cy += 18.0;
                    }

                    // Crack times
                    cy += 8.0;
                    cmds.push(RenderCommand::Text {
                        x: lx,
                        y: cy,
                        text: "CRACK TIME ESTIMATES".to_owned(),
                        color: OVERLAY0,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(max_w),
                        overflow: TextOverflow::Ellipsis,
                    });
                    cy += 18.0;

                    let crack_lines = [
                        ("Online (throttled):", &analysis.crack_time.online_throttled),
                        ("Online (fast):", &analysis.crack_time.online_unthrottled),
                        ("Offline (slow hash):", &analysis.crack_time.offline_slow),
                        ("Offline (fast hash):", &analysis.crack_time.offline_fast),
                    ];
                    for (label, value) in &crack_lines {
                        cmds.push(RenderCommand::Text {
                            x: lx,
                            y: cy,
                            text: (*label).to_owned(),
                            color: SUBTEXT0,
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(150.0),
                            overflow: TextOverflow::Ellipsis,
                        });
                        cmds.push(RenderCommand::Text {
                            x: lx + 160.0,
                            y: cy,
                            text: (*value).clone(),
                            color: TEXT,
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(max_w - 170.0),
                            overflow: TextOverflow::Ellipsis,
                        });
                        cy += 16.0;
                    }

                    // Patterns
                    if !analysis.patterns_found.is_empty() {
                        cy += 8.0;
                        cmds.push(RenderCommand::Text {
                            x: lx,
                            y: cy,
                            text: "PATTERNS DETECTED".to_owned(),
                            color: OVERLAY0,
                            font_size: 10.0,
                            font_weight: FontWeightHint::Bold,
                            max_width: Some(max_w),
                            overflow: TextOverflow::Ellipsis,
                        });
                        cy += 18.0;

                        // The pattern list is the last thing this tab draws, so
                        // the cursor it returns has nowhere further to go. Bind
                        // it anyway: whatever gets appended below must start
                        // from where the list actually ended, not from a second
                        // guess at how tall it was.
                        let _list_bottom = render_pattern_list(
                            cmds,
                            &analysis.patterns_found,
                            lx + 4.0,
                            cy,
                            y + height,
                            max_w - 8.0,
                        );
                    }
                } else {
                    cmds.push(RenderCommand::Text {
                        x: lx,
                        y: cy,
                        text: "Generate a password to see analysis".to_owned(),
                        color: OVERLAY0,
                        font_size: 13.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_w),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
            ActiveTab::History => {
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y: cy,
                    text: format!("HISTORY ({} entries)", self.history.len()),
                    color: OVERLAY0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(max_w),
                    overflow: TextOverflow::Ellipsis,
                });
                cy += 22.0;

                // The old bound here was `if cy > y + height { break }`, which
                // tested the row's *top*: the last row could start just inside
                // the panel and be drawn half outside it. Rows are counted
                // against the space that fits a whole row, and the entries that
                // do not fit are counted rather than silently dropped.
                let total = self.history.len();
                let room = rows_that_fit(cy, y + height, HISTORY_ROW_PITCH).min(HISTORY_MAX_ROWS);
                let overflowing = total > room;
                let shown = if overflowing {
                    room.saturating_sub(1)
                } else {
                    total
                };
                for entry in self.history.iter().rev().take(shown) {
                    cmds.push(RenderCommand::FillRect {
                        x: lx,
                        y: cy,
                        width: max_w,
                        height: ITEM_HEIGHT,
                        color: SURFACE0,
                        corner_radii: CornerRadii::all(CORNER_RADIUS),
                    });

                    // Strength dot
                    cmds.push(RenderCommand::FillRect {
                        x: lx + 8.0,
                        y: cy + 10.0,
                        width: 8.0,
                        height: 8.0,
                        color: entry.strength.color(),
                        corner_radii: CornerRadii::all(4.0),
                    });

                    // A 28px row cannot wrap, so a password too long for the
                    // column is elided — but the cut is *marked*, so nobody
                    // reads a clipped 128-character password as the whole
                    // thing and copies it down.
                    cmds.push(RenderCommand::Text {
                        x: lx + 22.0,
                        y: cy + 8.0,
                        text: text::elide(
                            &entry.password,
                            max_w - 140.0,
                            "…",
                            11.0,
                            FontWeightHint::Regular,
                        ),
                        color: TEXT,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_w - 140.0),
                        overflow: TextOverflow::Ellipsis,
                    });

                    cmds.push(RenderCommand::Text {
                        x: lx + max_w - 110.0,
                        y: cy + 8.0,
                        text: format!("[{}] {:.0}b", entry.gen_type, entry.entropy),
                        color: SUBTEXT1,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(100.0),
                        overflow: TextOverflow::Ellipsis,
                    });

                    cy += HISTORY_ROW_PITCH;
                }
                if overflowing {
                    cmds.push(RenderCommand::Text {
                        x: lx + 4.0,
                        y: cy + 8.0,
                        text: format!("+{} older", total.saturating_sub(shown)),
                        color: OVERLAY0,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_w - 8.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
        }
    }
}

// ============================================================================
// Main
// ============================================================================

impl App for PasswordApp {
    fn title(&self) -> String {
        "Password Generator".to_owned()
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
    /// A password appears when one is asked for. Nothing here ages, and a
    /// generator that produced a new secret on a timer would be actively worse
    /// than one that did not — the one on screen is the one the user is in the
    /// middle of copying.
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
        RenderTree {
            commands: self.render_commands(width, height),
        }
    }
}

fn main() -> ExitCode {
    let mut app = PasswordApp::new();
    // So the first frame shows something on every tab rather than an empty
    // field the user has to press a key to fill.
    app.gen_password();
    app.gen_passphrase();
    app.gen_pin();
    app::launch("passwordgen", &mut app)
}

// ============================================================================
// Tests
// ============================================================================

// Panicking on bad data is the point of a test, so the workspace's defensive
// lints are relaxed here — the same opt-out the sibling apps use.
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
    // compositor: every generator was reachable only by a caller.
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;

    /// An app with a reproducible source.
    ///
    /// `PasswordApp::new` takes the system CSPRNG, which is not available in a
    /// test process — the generators then *refuse*, which is this app's
    /// documented behaviour and the reason a test that called `new` saw empty
    /// output rather than a bug.
    fn seeded_app() -> PasswordApp {
        PasswordApp::with_seed(42)
    }

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
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
    fn each_generator_key_produces_its_own_kind() {
        let mut app = seeded_app();
        for (k, kind) in [
            (Key::W, GenKind::Passphrase),
            (Key::N, GenKind::Pin),
            (Key::R, GenKind::Pronounceable),
            (Key::B, GenKind::Bulk),
            (Key::P, GenKind::Password),
        ] {
            assert_eq!(app.handle_event(&press(k)), EventResult::Consumed);
            assert_eq!(app.gen_kind, kind, "{k:?} chose the wrong kind");
            assert_eq!(
                app.active_tab,
                ActiveTab::Generator,
                "a generator key should show the generator"
            );
        }
    }

    #[test]
    fn space_produces_another_of_the_same_kind() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::N));
        let kind = app.gen_kind;
        let first = app.current_password.clone();
        assert_eq!(app.handle_event(&press(Key::Space)), EventResult::Consumed);
        assert_eq!(app.gen_kind, kind, "Space changed the kind");
        // Two PINs of the same length are not guaranteed to differ, so this
        // asserts the history grew rather than that the text changed.
        assert!(
            app.history.len() >= 2,
            "Space should have generated again: {} entries",
            app.history.len()
        );
        let _ = first;
    }

    #[test]
    fn the_length_arrows_stay_inside_the_range_for_each_kind() {
        // A held-down arrow must not be able to ask for a one-character
        // password or a thousand-word passphrase.
        let mut app = seeded_app();
        app.handle_event(&press(Key::P));
        for _ in 0..400 {
            app.handle_event(&press(Key::Right));
        }
        assert_eq!(app.password_opts.length, MAX_PASSWORD_LEN);
        for _ in 0..400 {
            app.handle_event(&press(Key::Left));
        }
        assert_eq!(app.password_opts.length, MIN_PASSWORD_LEN);
        // And at the end of the range the key stops reporting a redraw.
        assert_eq!(app.handle_event(&press(Key::Left)), EventResult::Ignored);

        app.handle_event(&press(Key::W));
        for _ in 0..40 {
            app.handle_event(&press(Key::Left));
        }
        assert_eq!(app.passphrase_opts.word_count, MIN_WORDS);

        app.handle_event(&press(Key::N));
        for _ in 0..40 {
            app.handle_event(&press(Key::Right));
        }
        assert_eq!(app.pin_length, MAX_PIN_LEN);
    }

    #[test]
    fn a_longer_password_is_actually_longer() {
        // The arrow changes a number; this checks the number reaches the
        // generator rather than only the label.
        let mut app = seeded_app();
        app.handle_event(&press(Key::P));
        let short = app.current_password.chars().count();
        for _ in 0..8 {
            app.handle_event(&press(Key::Right));
        }
        let long = app.current_password.chars().count();
        assert!(
            long > short,
            "lengthening produced {long} characters, was {short}"
        );
        assert_eq!(long, app.password_opts.length);
    }

    #[test]
    fn typing_on_the_analyser_tab_measures_rather_than_generates() {
        // "p" generates a password everywhere else.
        let mut app = seeded_app();
        app.handle_event(&press(Key::Num2));
        assert_eq!(app.active_tab, ActiveTab::Analyzer);
        let before = app.current_password.clone();
        for c in "p4ssw0rd".chars() {
            app.handle_event(&typed(c));
        }
        assert_eq!(app.analyzer_input, "p4ssw0rd");
        assert_eq!(
            app.current_password, before,
            "typing into the analyser generated a password"
        );
        // `is_some()` alone proves nothing: generating a password already
        // leaves an analysis behind, so the assertion has to be that the
        // analysis is of *this* text.
        assert_eq!(
            app.current_analysis.as_ref().map(|a| a.length),
            Some("p4ssw0rd".len()),
            "the strength meter is not measuring what was typed"
        );
        // Backspace shortens it — and re-measures, which is a separate call
        // from the one the typing branch makes and needs its own assertion.
        app.handle_event(&press(Key::Backspace));
        assert_eq!(app.analyzer_input, "p4ssw0r");
        assert_eq!(
            app.current_analysis.as_ref().map(|a| a.length),
            Some("p4ssw0r".len()),
            "backspace stored without re-measuring"
        );
        for _ in 0.."p4ssw0r".len() {
            app.handle_event(&press(Key::Backspace));
        }
        assert_eq!(app.analyzer_input, "");
        assert_eq!(
            app.handle_event(&press(Key::Backspace)),
            EventResult::Ignored
        );
    }

    #[test]
    fn the_tab_keys_still_work_from_inside_the_analyser() {
        // Otherwise there is no way out of the text box.
        let mut app = seeded_app();
        app.handle_event(&press(Key::Num2));
        assert_eq!(app.handle_event(&press(Key::Num1)), EventResult::Consumed);
        assert_eq!(app.active_tab, ActiveTab::Generator);
    }

    #[test]
    fn a_key_the_app_has_no_use_for_is_not_consumed() {
        let mut app = seeded_app();
        assert_eq!(app.handle_event(&press(Key::F9)), EventResult::Ignored);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = seeded_app();
        let before = app.current_password.clone();
        let release = Event::Key(KeyEvent {
            key: Key::P,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(app.current_password, before);
    }

    #[test]
    fn clearing_an_empty_history_is_not_a_redraw() {
        let mut app = seeded_app();
        app.clear_history();
        assert_eq!(app.handle_event(&press(Key::C)), EventResult::Ignored);
    }

    #[test]
    fn the_app_asks_for_no_clock() {
        // A generator that produced a new secret on a timer would replace the
        // one the user is in the middle of copying.
        let app = seeded_app();
        assert_eq!(app.tick_interval(), None);
    }

    #[test]
    fn rendering_draws_something_on_every_tab_at_an_awkward_size() {
        let mut app = seeded_app();
        for tab in [
            ActiveTab::Generator,
            ActiveTab::Analyzer,
            ActiveTab::History,
        ] {
            app.active_tab = tab;
            for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
                assert!(
                    !app.render(w, h).commands.is_empty(),
                    "{tab:?} drew nothing at {w}x{h}"
                );
            }
        }
    }

    // --- Measured-width tests ---

    #[test]
    fn the_rating_badge_fits_its_label() {
        for rating in [
            StrengthRating::VeryWeak,
            StrengthRating::Weak,
            StrengthRating::Fair,
            StrengthRating::Strong,
            StrengthRating::VeryStrong,
        ] {
            let label = rating.label();
            let w = text::padded_width(label, 10.0, 13.0, FontWeightHint::Bold);
            assert!(
                w >= text::measure(label, 13.0, FontWeightHint::Bold) + 20.0,
                "{label} overflows its badge"
            );
        }
    }

    #[test]
    fn a_toolbar_tab_keeps_its_width_when_selected() {
        // The tab strip is laid out left to right from a fixed origin, so a tab
        // that grew when selected would push every tab after it sideways.
        let widths: Vec<f32> = [
            ActiveTab::Generator,
            ActiveTab::Analyzer,
            ActiveTab::History,
        ]
        .iter()
        .map(|t| text::padded_width_any_weight(t.label(), 10.0, 11.0))
        .collect();
        for (i, tab) in [
            ActiveTab::Generator,
            ActiveTab::Analyzer,
            ActiveTab::History,
        ]
        .iter()
        .enumerate()
        {
            for weight in [FontWeightHint::Regular, FontWeightHint::Bold] {
                let needed = text::measure(tab.label(), 11.0, weight) + 20.0;
                assert!(
                    widths.get(i).copied().unwrap_or(0.0) >= needed,
                    "{} overflows at {weight:?}",
                    tab.label()
                );
            }
        }
    }

    // --- Password generation ---

    #[test]
    fn test_generate_password_length() {
        let mut rng = SeededRng::new(1);
        let opts = PasswordOptions {
            length: 20,
            ..PasswordOptions::default()
        };
        let pw = generate_password(&opts, &mut rng);
        assert_eq!(pw.len(), 20);
    }

    #[test]
    fn test_generate_password_includes_classes() {
        let mut rng = SeededRng::new(42);
        let opts = PasswordOptions {
            length: 20,
            must_include_each_class: true,
            ..PasswordOptions::default()
        };
        let pw = generate_password(&opts, &mut rng);
        assert!(pw.chars().any(|c| c.is_ascii_lowercase()));
        assert!(pw.chars().any(|c| c.is_ascii_uppercase()));
        assert!(pw.chars().any(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_generate_password_no_symbols() {
        let mut rng = SeededRng::new(1);
        let opts = PasswordOptions {
            length: 50,
            use_symbols: false,
            must_include_each_class: false,
            ..PasswordOptions::default()
        };
        let pw = generate_password(&opts, &mut rng);
        assert!(pw.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_generate_password_empty_pool() {
        let mut rng = SeededRng::new(1);
        let opts = PasswordOptions {
            length: 10,
            use_lowercase: false,
            use_uppercase: false,
            use_digits: false,
            use_symbols: false,
            ..PasswordOptions::default()
        };
        let pw = generate_password(&opts, &mut rng);
        assert!(pw.is_empty());
    }

    #[test]
    fn test_generate_password_zero_length() {
        let mut rng = SeededRng::new(1);
        let opts = PasswordOptions {
            length: 0,
            ..PasswordOptions::default()
        };
        let pw = generate_password(&opts, &mut rng);
        assert!(pw.is_empty());
    }

    // --- Passphrase ---

    #[test]
    fn test_generate_passphrase() {
        let mut rng = SeededRng::new(42);
        let opts = PassphraseOptions::default();
        let pp = generate_passphrase(&opts, &mut rng);
        assert!(!pp.is_empty());
        // Should contain separator
        assert!(pp.contains('-'));
    }

    #[test]
    fn test_passphrase_word_count() {
        let mut rng = SeededRng::new(42);
        let opts = PassphraseOptions {
            word_count: 6,
            capitalize: false,
            add_number: false,
            add_symbol: false,
            ..PassphraseOptions::default()
        };
        let pp = generate_passphrase(&opts, &mut rng);
        let words: Vec<&str> = pp.split('-').collect();
        assert_eq!(words.len(), 6);
    }

    // --- PIN ---

    #[test]
    fn test_generate_pin() {
        let mut rng = SeededRng::new(1);
        let pin = generate_pin(6, &mut rng);
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
    }

    // --- Pronounceable ---

    #[test]
    fn test_generate_pronounceable() {
        let mut rng = SeededRng::new(1);
        let pw = generate_pronounceable(10, &mut rng);
        assert_eq!(pw.len(), 10);
        // Alternating consonant-vowel pattern
        for (i, c) in pw.chars().enumerate() {
            if i % 2 == 0 {
                assert!(
                    CONSONANTS.contains(c),
                    "Expected consonant at pos {i}, got {c}"
                );
            } else {
                assert!(VOWELS.contains(c), "Expected vowel at pos {i}, got {c}");
            }
        }
    }

    // --- Strength analysis ---

    #[test]
    fn test_analyze_strong_password() {
        let analysis = analyze_password("kX9$mQ!2pL@7nR#4");
        assert!(analysis.entropy_bits > 60.0);
        assert!(analysis.rating >= StrengthRating::Strong);
    }

    #[test]
    fn test_analyze_weak_password() {
        let analysis = analyze_password("abc");
        assert!(analysis.entropy_bits < 25.0);
        assert_eq!(analysis.rating, StrengthRating::VeryWeak);
    }

    #[test]
    fn test_analyze_common_password() {
        let analysis = analyze_password("password");
        assert!(analysis.is_common);
        assert_eq!(analysis.rating, StrengthRating::VeryWeak);
    }

    #[test]
    fn test_analyze_empty() {
        let analysis = analyze_password("");
        assert_eq!(analysis.length, 0);
        assert!(analysis.entropy_bits.abs() < f64::EPSILON);
    }

    #[test]
    fn test_detect_repeated_chars() {
        let analysis = analyze_password("aaabbbccc");
        let has_repeat = analysis
            .patterns_found
            .iter()
            .any(|p| p.kind == PatternKind::RepeatedChars);
        assert!(has_repeat);
    }

    #[test]
    fn test_detect_sequential_chars() {
        let analysis = analyze_password("abcdefgh");
        let has_seq = analysis
            .patterns_found
            .iter()
            .any(|p| p.kind == PatternKind::SequentialChars);
        assert!(has_seq);
    }

    #[test]
    fn test_detect_keyboard_sequence() {
        let analysis = analyze_password("myqwertypassword");
        let has_kb = analysis
            .patterns_found
            .iter()
            .any(|p| p.kind == PatternKind::KeyboardSequence);
        assert!(has_kb);
    }

    // --- Crack time ---

    #[test]
    fn test_crack_time_instant() {
        let ct = CrackTime::from_entropy(0.0);
        assert_eq!(ct.offline_fast, "Instant");
    }

    #[test]
    fn test_crack_time_high_entropy() {
        let ct = CrackTime::from_entropy(128.0);
        assert!(ct.offline_fast.contains("billion") || ct.offline_fast.contains("million"));
    }

    #[test]
    fn test_format_crack_time() {
        assert_eq!(format_crack_time(0.5, 1.0), "Instant");
        assert_eq!(format_crack_time(30.0, 1.0), "30 seconds");
        assert_eq!(format_crack_time(120.0, 1.0), "2 minutes");
        assert_eq!(format_crack_time(7200.0, 1.0), "2 hours");
        assert_eq!(format_crack_time(172800.0, 1.0), "2 days");
    }

    // --- Password options ---

    #[test]
    fn test_options_pool_size() {
        let opts = PasswordOptions::default();
        let pool = opts.build_pool();
        // 26 + 26 + 10 + 30 = 92
        assert!(pool.len() >= 90);
    }

    #[test]
    fn test_options_exclude_ambiguous() {
        let opts = PasswordOptions {
            exclude_ambiguous: true,
            ..PasswordOptions::default()
        };
        let pool = opts.build_pool();
        assert!(!pool.contains(&'O'));
        assert!(!pool.contains(&'0'));
        assert!(!pool.contains(&'l'));
    }

    #[test]
    fn test_options_entropy() {
        let opts = PasswordOptions::default();
        assert!(opts.total_entropy() > 0.0);
        assert!(opts.entropy_per_char() > 0.0);
    }

    #[test]
    fn test_passphrase_entropy() {
        let opts = PassphraseOptions::default();
        assert!(opts.entropy() > 30.0);
    }

    // --- Password policy ---

    #[test]
    fn test_policy_compliant() {
        let policy = PasswordPolicy::default();
        let violations = policy.check("Str0ng!Password");
        assert!(violations.is_empty(), "Violations: {:?}", violations);
    }

    #[test]
    fn test_policy_too_short() {
        let policy = PasswordPolicy {
            min_length: 12,
            ..PasswordPolicy::default()
        };
        let violations = policy.check("Abc1!");
        assert!(violations.iter().any(|v| v.contains("short")));
    }

    #[test]
    fn test_policy_missing_uppercase() {
        let policy = PasswordPolicy::default();
        let violations = policy.check("alllowercase123!");
        assert!(violations.iter().any(|v| v.contains("uppercase")));
    }

    #[test]
    fn test_policy_common_password() {
        let policy = PasswordPolicy::default();
        let violations = policy.check("password");
        assert!(violations.iter().any(|v| v.contains("commonly")));
    }

    // --- App tests ---

    #[test]
    fn test_app_gen_password() {
        let mut app = PasswordApp::with_seed(42);
        app.gen_password();
        assert!(!app.current_password.is_empty());
        assert!(app.current_analysis.is_some());
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn test_app_gen_passphrase() {
        let mut app = PasswordApp::with_seed(42);
        app.gen_passphrase();
        assert!(!app.current_password.is_empty());
        assert!(app.current_password.contains('-'));
    }

    #[test]
    fn test_app_gen_pin() {
        let mut app = PasswordApp::with_seed(42);
        app.pin_length = 4;
        app.gen_pin();
        assert_eq!(app.current_password.len(), 4);
    }

    #[test]
    fn test_app_gen_pronounceable() {
        let mut app = PasswordApp::with_seed(42);
        app.gen_pronounceable();
        assert!(!app.current_password.is_empty());
    }

    #[test]
    fn test_app_bulk_generate() {
        let mut app = PasswordApp::with_seed(42);
        app.bulk_count = 5;
        app.gen_bulk();
        assert_eq!(app.bulk_results.len(), 5);
    }

    #[test]
    fn test_app_clear_history() {
        let mut app = PasswordApp::with_seed(42);
        app.gen_password();
        app.gen_passphrase();
        assert_eq!(app.history.len(), 2);
        app.clear_history();
        assert!(app.history.is_empty());
    }

    #[test]
    fn test_app_export_history() {
        let mut app = PasswordApp::with_seed(42);
        app.gen_password();
        let export = app.export_history();
        assert!(export.contains("Password Generation History"));
    }

    #[test]
    fn test_app_render() {
        let mut app = PasswordApp::with_seed(42);
        app.gen_password();
        let cmds = app.render_commands(1100.0, 700.0);
        assert!(!cmds.is_empty());
    }

    // --- Bounded lists in the right panel ---

    const TEST_WINDOW_W: f32 = 1100.0;
    const TEST_WINDOW_H: f32 = 700.0;

    /// Every text command drawn, as `(y, text)`.
    fn text_rows(cmds: &[RenderCommand]) -> Vec<(f32, String)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { y, text, .. } => Some((*y, text.clone())),
                _ => None,
            })
            .collect()
    }

    /// The bottom edge the right panel's content must stay above.
    fn right_panel_bottom() -> f32 {
        TEST_WINDOW_H - STATUS_BAR_HEIGHT
    }

    /// An adversarial password yields one "repeated characters" entry per run,
    /// so the pattern list is unbounded while the panel is not. The list must
    /// stop at the panel's edge rather than drawing off the bottom of it.
    #[test]
    fn the_pattern_list_stays_inside_its_panel() {
        let mut app = PasswordApp::with_seed(42);
        // 40 runs of three identical characters: 40 detected patterns.
        let mut adversarial = String::new();
        for n in 0..40_u8 {
            let ch = char::from(b'a'.saturating_add(n % 26));
            adversarial.extend([ch, ch, ch]);
        }
        app.active_tab = ActiveTab::Analyzer;
        app.set_analyzer_input(&adversarial);
        app.analyze_input();
        let analysis = app
            .current_analysis
            .as_ref()
            .expect("analyze_input sets an analysis");
        assert!(
            analysis.patterns_found.len() > 20,
            "test needs a genuinely long pattern list, got {}",
            analysis.patterns_found.len(),
        );

        let cmds = app.render_commands(TEST_WINDOW_W, TEST_WINDOW_H);
        let rows = text_rows(&cmds);
        let mut checked = 0;
        for (y, text) in &rows {
            if text.starts_with('[') || text.ends_with(" more") {
                assert!(
                    *y + PATTERN_ROW_HEIGHT <= right_panel_bottom(),
                    "pattern row {text:?} at y={y} runs past the panel bottom {}",
                    right_panel_bottom(),
                );
                checked += 1;
            }
        }
        assert!(checked >= 5, "expected pattern rows, checked {checked}");
        assert!(
            rows.iter().any(|(_, t)| t.ends_with(" more")),
            "the hidden patterns must be counted, not silently dropped",
        );
    }

    /// When they all fit, no marker appears and none are dropped.
    #[test]
    fn a_short_pattern_list_is_shown_whole() {
        let mut app = PasswordApp::with_seed(42);
        app.active_tab = ActiveTab::Analyzer;
        app.set_analyzer_input("aaa123qwerty");
        app.analyze_input();
        let expected = app
            .current_analysis
            .as_ref()
            .expect("an analysis")
            .patterns_found
            .len();
        assert!(expected > 0, "test needs at least one pattern");

        let rows = text_rows(&app.render_commands(TEST_WINDOW_W, TEST_WINDOW_H));
        let drawn = rows.iter().filter(|(_, t)| t.starts_with('[')).count();
        assert_eq!(drawn, expected, "expected every pattern drawn: {rows:?}");
        assert!(
            !rows.iter().any(|(_, t)| t.ends_with(" more")),
            "no overflow marker should appear",
        );
    }

    /// The history list is capped, and says how many entries it is not showing.
    #[test]
    fn a_long_history_says_how_much_it_is_not_showing() {
        let mut app = PasswordApp::with_seed(42);
        for _ in 0..40 {
            app.gen_password();
        }
        app.active_tab = ActiveTab::History;
        assert!(
            app.history.len() > HISTORY_MAX_ROWS,
            "test needs a long history"
        );

        let rows = text_rows(&app.render_commands(TEST_WINDOW_W, TEST_WINDOW_H));
        let marker = rows
            .iter()
            .find(|(_, t)| t.ends_with(" older"))
            .map(|(_, t)| t.clone());
        assert!(
            marker.is_some(),
            "expected an overflow marker for a {}-entry history: {rows:?}",
            app.history.len(),
        );
    }

    /// A password too long for the history column is elided *and marked*, so a
    /// clipped password is never mistaken for the whole one.
    #[test]
    fn a_long_history_password_is_marked_where_it_is_cut() {
        let mut app = PasswordApp::with_seed(42);
        app.gen_password();
        if let Some(entry) = app.history.first_mut() {
            entry.password = "W".repeat(200);
        }
        app.active_tab = ActiveTab::History;
        let column_w = (TEST_WINDOW_W - LEFT_PANEL_WIDTH - 24.0) - 140.0;
        let rows: Vec<String> = text_rows(&app.render_commands(TEST_WINDOW_W, TEST_WINDOW_H))
            .into_iter()
            .map(|(_, t)| t)
            .filter(|t| t.starts_with('W'))
            .collect();
        assert_eq!(rows.len(), 1, "expected one password row: {rows:?}");
        assert!(
            rows[0].ends_with('…'),
            "expected the cut marked: {:?}",
            rows[0]
        );
        let measured = text::measure(&rows[0], 11.0, FontWeightHint::Regular);
        assert!(
            measured <= column_w + 0.5,
            "password row measures {measured} in a {column_w} column",
        );
    }

    #[test]
    fn test_strength_rating_ordering() {
        assert!(StrengthRating::VeryWeak < StrengthRating::Weak);
        assert!(StrengthRating::Weak < StrengthRating::Fair);
        assert!(StrengthRating::Fair < StrengthRating::Strong);
        assert!(StrengthRating::Strong < StrengthRating::VeryStrong);
    }

    // --- Where the randomness comes from ---

    /// The defect this replaced: `main` built the app from the constant seed
    /// `42`, so every user on every machine got the same passwords, PINs and
    /// passphrases, in the same order, from first launch onwards.
    ///
    /// Two independently-opened apps must therefore never agree. Where there
    /// is no kernel CSPRNG to open — the host test toolchain — they must
    /// agree only in producing nothing at all, which is the other half of the
    /// same property: never a shared *password*.
    #[test]
    fn two_freshly_opened_apps_never_generate_the_same_password() {
        let mut first = PasswordApp::new();
        let mut second = PasswordApp::new();
        first.gen_password();
        second.gen_password();

        if first.last_error.is_some() {
            assert_eq!(second.last_error, first.last_error);
            assert!(first.current_password.is_empty());
            assert!(first.history.is_empty(), "a refusal records nothing");
            return;
        }
        assert_ne!(first.current_password, second.current_password);
    }

    /// Every generator must fail closed. A password the user believes is
    /// random and is not is worse than no password at all, so the button
    /// produces an explanation rather than a weak secret.
    #[test]
    fn every_generator_refuses_when_there_is_no_entropy() {
        /// One of the app's Generate buttons, named for the failure message.
        type Generator = (&'static str, fn(&mut PasswordApp));

        let generators: [Generator; 4] = [
            ("password", PasswordApp::gen_password),
            ("passphrase", PasswordApp::gen_passphrase),
            ("pin", PasswordApp::gen_pin),
            ("pronounceable", PasswordApp::gen_pronounceable),
        ];
        for (name, generate) in generators {
            let mut app = PasswordApp::with_random(AppRandom::Unavailable);
            generate(&mut app);
            assert!(app.current_password.is_empty(), "{name} produced a secret");
            assert!(
                app.current_analysis.is_none(),
                "{name} recorded an analysis"
            );
            assert!(app.history.is_empty(), "{name} recorded history");
            assert_eq!(app.last_error.as_deref(), Some(NO_ENTROPY_MESSAGE));
        }
    }

    #[test]
    fn a_bulk_run_with_no_entropy_yields_an_empty_list_not_a_short_one() {
        let mut app = PasswordApp::with_random(AppRandom::Unavailable);
        app.bulk_count = 10;
        app.gen_bulk();
        assert!(app.bulk_results.is_empty());
        assert_eq!(app.last_error.as_deref(), Some(NO_ENTROPY_MESSAGE));
    }

    /// A refusal must not leave the previous password on screen, or the user
    /// reads a stale secret as the one they just asked for.
    #[test]
    fn a_refusal_clears_the_password_that_was_showing() {
        let mut app = PasswordApp::with_seed(42);
        app.gen_password();
        assert!(!app.current_password.is_empty());

        app.rng = AppRandom::Unavailable;
        app.gen_password();
        assert!(app.current_password.is_empty());
        assert!(app.current_analysis.is_none());
        assert_eq!(app.last_error.as_deref(), Some(NO_ENTROPY_MESSAGE));
    }

    /// The refusal goes where the password would have gone, so it is seen.
    #[test]
    fn the_refusal_is_rendered_in_place_of_the_password() {
        let mut app = PasswordApp::with_random(AppRandom::Unavailable);
        app.gen_password();
        let shown = app.render_commands(1100.0, 700.0).into_iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { ref text, color, .. }
                if text == NO_ENTROPY_MESSAGE && color == RED)
        });
        assert!(
            shown,
            "the refusal must be drawn where the password would be"
        );
    }

    /// A successful generation must clear a refusal left over from an earlier
    /// one, or the message outlives the condition it describes.
    #[test]
    fn a_successful_generation_clears_an_earlier_refusal() {
        let mut app = PasswordApp::with_random(AppRandom::Unavailable);
        app.gen_password();
        assert!(app.last_error.is_some());

        app.rng = AppRandom::seeded(7);
        app.gen_password();
        assert!(app.last_error.is_none());
        assert!(!app.current_password.is_empty());
    }

    #[test]
    fn an_unavailable_source_is_never_trustworthy() {
        assert!(!AppRandom::Unavailable.is_trustworthy());
        assert!(AppRandom::seeded(1).is_trustworthy());
    }

    #[test]
    fn test_is_common_password() {
        assert!(is_common_password("password"));
        assert!(is_common_password("123456"));
        assert!(is_common_password("Password")); // Case-insensitive
        assert!(!is_common_password("xK9mQ2pL7nR4"));
    }
}

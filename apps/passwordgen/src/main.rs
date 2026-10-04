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
//! - Checking rules the user sets on the Rules tab, kept in `passwordgen.yaml`
//! - Multi-panel UI with generator, analyzer, history and rules
//!
//! Uses the guitk library for UI rendering.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml. This keeps the discipline
// centralised rather than diverging per-crate.

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::Color;
use guitk::dialog::{DialogAction, FileDialog};
use guitk::event::{Event, EventResult, Key, KeyEvent};
use guitk::field;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SecretSource, SeededRng, SystemRandom};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::textedit;
use guitk::textinput::TextInput;
use oswindow::app::{self, App, Response};
use pathtext::ShowPath;
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
    pal: &Palette,
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
            color: pal.ink(pal.peach),
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
            color: pal.subtext0,
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

/// A kind of character the generator may draw from.
///
/// An enum rather than four methods because the guard against turning the
/// last one off has to see them as one set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharClass {
    /// a-z
    Lower,
    /// A-Z
    Upper,
    /// 0-9
    Digits,
    /// Punctuation.
    Symbols,
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

/// One switchable row of the options panel: its key, its name, what it
/// reads, and what pressing the key does.
///
/// The panel used to build its rows inline and the key handler matched keys
/// separately, so the two lists could drift -- and they had: the panel drew
/// five rows and `must_include_each_class` was a sixth option with no row and
/// no key, while `PassphraseOptions` had three more that were neither drawn
/// nor reachable. Every passphrase this program produced was capitalised and
/// ended in a digit, for everybody, because `capitalize` and `add_number`
/// were `true` at construction and nothing else ever assigned them.
///
/// Holding the key beside the row is what keeps them together: a row cannot
/// be drawn without naming the key that changes it, and `from_key` is the
/// same list read the other way round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionRow {
    Lowercase,
    Uppercase,
    Digits,
    Symbols,
    ExcludeAmbiguous,
    EveryClass,
    Capitalize,
    EndWithDigit,
    EndWithSymbol,
}

impl OptionRow {
    /// Every row, in the order the panel draws them.
    pub const ALL: [OptionRow; 9] = [
        Self::Lowercase,
        Self::Uppercase,
        Self::Digits,
        Self::Symbols,
        Self::ExcludeAmbiguous,
        Self::EveryClass,
        Self::Capitalize,
        Self::EndWithDigit,
        Self::EndWithSymbol,
    ];

    /// The keystroke, as the panel prints it.
    pub fn keys(self) -> &'static str {
        match self {
            Self::Lowercase => "L",
            Self::Uppercase => "U",
            Self::Digits => "D",
            Self::Symbols => "S",
            Self::ExcludeAmbiguous => "A",
            Self::EveryClass => "M",
            Self::Capitalize => "Shift+C",
            Self::EndWithDigit => "Shift+D",
            Self::EndWithSymbol => "Shift+S",
        }
    }

    /// The name in the panel.
    pub fn label(self) -> &'static str {
        match self {
            Self::Lowercase => "Lowercase",
            Self::Uppercase => "Uppercase",
            Self::Digits => "Digits",
            Self::Symbols => "Symbols",
            Self::ExcludeAmbiguous => "Exclude Ambiguous",
            Self::EveryClass => "One Of Each Kind",
            Self::Capitalize => "Capitalise Words",
            Self::EndWithDigit => "End With A Digit",
            Self::EndWithSymbol => "End With A Symbol",
        }
    }

    /// Which keystroke changes it, read from the printed label itself.
    ///
    /// `guitk::shortcut::keystrokes` parses the same string the panel
    /// draws, so there is no third list to drift: the row prints
    /// "Shift+C", and that string is what decides whether Shift+C matched.
    /// A label this cannot parse is caught by
    /// `every_option_row_names_a_keystroke_that_parses` rather than
    /// quietly matching nothing.
    pub fn from_key(key: Key, shift: bool) -> Option<Self> {
        Self::ALL.into_iter().find(|row| {
            guitk::shortcut::keystrokes(row.keys()).is_ok_and(|presses| {
                presses
                    .iter()
                    .any(|p| p.key == key && p.modifiers.shift == shift)
            })
        })
    }

    /// Whether the row applies to what is being generated.
    ///
    /// The three passphrase rows are drawn dimmed rather than hidden in the
    /// other modes, for the same reason the character classes are: a row that
    /// appears and disappears as the kind changes is harder to find than one
    /// that is always in the same place.
    pub fn is_on(self, app: &PasswordApp) -> bool {
        match self {
            Self::Lowercase => app.password_opts.use_lowercase,
            Self::Uppercase => app.password_opts.use_uppercase,
            Self::Digits => app.password_opts.use_digits,
            Self::Symbols => app.password_opts.use_symbols,
            Self::ExcludeAmbiguous => app.password_opts.exclude_ambiguous,
            Self::EveryClass => app.password_opts.must_include_each_class,
            Self::Capitalize => app.passphrase_opts.capitalize,
            Self::EndWithDigit => app.passphrase_opts.add_number,
            Self::EndWithSymbol => app.passphrase_opts.add_symbol,
        }
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

    pub fn color(self, pal: &Palette) -> Color {
        match self {
            Self::VeryWeak => pal.red,
            Self::Weak => pal.peach,
            Self::Fair => pal.yellow,
            Self::Strong => pal.green,
            Self::VeryStrong => pal.teal,
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
    // Characters, not bytes. "pässwörd" is eight characters long, and every
    // figure below is per character typed: counting bytes made an accented
    // letter count twice towards the length a rule asks for, and twice again
    // towards the strength.
    let length = password.chars().count();
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

/// The rules a password is checked against.
///
/// Until 2026-09-27 these were compiled in and the same for everybody: the
/// status bar said "Compliant" against rules nobody could see or change
/// (C-Q26). They are drawn and changed on the Rules tab now, and kept in
/// `passwordgen.yaml` -- see [`PasswordPolicy::from_settings`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordPolicy {
    /// The fewest characters a password may have.
    pub min_length: usize,
    /// The most it may have; `None` is no limit.
    pub max_length: Option<usize>,
    pub require_lowercase: bool,
    pub require_uppercase: bool,
    pub require_digit: bool,
    pub require_symbol: bool,
    /// How many of the four kinds of character -- lowercase, uppercase,
    /// digits and symbols -- it must mix.
    pub min_classes: usize,
    /// The least strength it may have, in bits of entropy.
    pub min_bits: u32,
    /// Whether a password on the list of commonly used ones is refused.
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
            min_bits: 40,
            disallow_common: true,
        }
    }
}

/// The settings file the rules are kept in: `<config>/passwordgen.yaml`, one
/// file per program (C-Q26, option A -- `design-decisions.md` §1418).
const CONFIG_NAME: &str = "passwordgen";

/// The mapping in that file the rules are kept under.
const RULES_KEY: &str = "rules";

/// The longest a length rule can name: the longest password the generator
/// makes.
const RULE_LENGTH_MAX: usize = MAX_PASSWORD_LEN;

/// The strength rule's ceiling and its step, in bits. Two hundred is past
/// anything a password is asked to be; five-bit steps take the arrow keys
/// from the default forty to anything a site asks for in a few presses.
const RULE_BITS_MAX: u32 = 200;
const RULE_BITS_STEP: u32 = 5;

impl PasswordPolicy {
    /// Every rule `password` breaks, one sentence each; empty when it meets
    /// them all.
    #[must_use]
    pub fn check(&self, password: &str) -> Vec<String> {
        let analysis = analyze_password(password);
        let length = analysis.length;
        let mut violations = Vec::new();

        if length < self.min_length {
            violations.push(format!(
                "Too short: {}, and the rules ask for at least {}",
                characters(length),
                self.min_length
            ));
        }
        if let Some(max) = self.max_length
            && length > max
        {
            violations.push(format!(
                "Too long: {}, and the rules allow at most {max}",
                characters(length)
            ));
        }
        if self.require_lowercase && !analysis.has_lowercase {
            violations.push("Has no lowercase letter".to_owned());
        }
        if self.require_uppercase && !analysis.has_uppercase {
            violations.push("Has no uppercase letter".to_owned());
        }
        if self.require_digit && !analysis.has_digits {
            violations.push("Has no digit".to_owned());
        }
        if self.require_symbol && !analysis.has_symbols {
            violations.push("Has no symbol".to_owned());
        }
        if analysis.char_classes_used < self.min_classes {
            violations.push(format!(
                "Mixes {} of the four kinds of character, and the rules ask for {}",
                analysis.char_classes_used, self.min_classes
            ));
        }
        // Rounded down for the sentence: 39.6 bits said as "40" beside a rule
        // of 40 would read as a rule met and reported broken.
        if analysis.entropy_bits < f64::from(self.min_bits) {
            violations.push(format!(
                "Too weak: {:.0} bits, and the rules ask for {}",
                analysis.entropy_bits.floor(),
                self.min_bits
            ));
        }
        if self.disallow_common && analysis.is_common {
            violations.push("Is one of the most commonly used passwords".to_owned());
        }

        violations
    }

    #[must_use]
    pub fn is_compliant(&self, password: &str) -> bool {
        self.check(password).is_empty()
    }

    /// The rules kept in `doc`, over the defaults -- and one sentence for
    /// each value there that could not be used, which keeps its default.
    ///
    /// A value is refused rather than clamped into range: a file that says
    /// the shortest allowed is 500 was written by somebody who meant
    /// something, and quietly checking against a different rule is worse
    /// than saying so.
    #[must_use]
    pub fn from_settings(doc: &yamldoc::Document) -> (Self, Vec<String>) {
        let mut rules = Self::default();
        let mut problems = Vec::new();
        let longest_rule = i64::try_from(RULE_LENGTH_MAX).unwrap_or(i64::MAX);

        if let Some(n) = kept_number(doc, "shortest", 1, longest_rule, &mut problems)
            .and_then(|n| usize::try_from(n).ok())
        {
            rules.min_length = n;
        }
        if let Some(n) = kept_number(doc, "longest", 1, longest_rule, &mut problems)
            .and_then(|n| usize::try_from(n).ok())
        {
            if n >= rules.min_length {
                rules.max_length = Some(n);
            } else {
                problems.push(format!(
                    "{CONFIG_NAME}.yaml: {RULES_KEY}.longest ({n}) is less than the shortest \
                     allowed ({}), so no longest is set",
                    rules.min_length
                ));
            }
        }
        for (key, rule) in [
            ("lowercase", &mut rules.require_lowercase),
            ("uppercase", &mut rules.require_uppercase),
            ("digit", &mut rules.require_digit),
            ("symbol", &mut rules.require_symbol),
            ("refuse_common", &mut rules.disallow_common),
        ] {
            if let Some(on) = kept_switch(doc, key, &mut problems) {
                *rule = on;
            }
        }
        if let Some(n) =
            kept_number(doc, "kinds", 1, 4, &mut problems).and_then(|n| usize::try_from(n).ok())
        {
            rules.min_classes = n;
        }
        if let Some(n) = kept_number(doc, "strength", 0, i64::from(RULE_BITS_MAX), &mut problems)
            .and_then(|n| u32::try_from(n).ok())
        {
            rules.min_bits = n;
        }
        (rules, problems)
    }

    /// Write the rules into `doc` under `rules:`, leaving everything else in
    /// it -- comments included -- as it was.
    pub fn store_into(&self, doc: &mut yamldoc::Document) {
        // Every length here is at most `RULE_LENGTH_MAX`, so the fallback is
        // never taken; it is there so this cannot panic.
        let whole = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
        doc.set_i64(&[RULES_KEY, "shortest"], whole(self.min_length));
        if let Some(n) = self.max_length {
            doc.set_i64(&[RULES_KEY, "longest"], whole(n));
        } else {
            // No limit is no key, so the file reads the way the rule does.
            // Whether there was one to take out does not matter.
            doc.remove(&[RULES_KEY, "longest"]);
        }
        doc.set_bool(&[RULES_KEY, "lowercase"], self.require_lowercase);
        doc.set_bool(&[RULES_KEY, "uppercase"], self.require_uppercase);
        doc.set_bool(&[RULES_KEY, "digit"], self.require_digit);
        doc.set_bool(&[RULES_KEY, "symbol"], self.require_symbol);
        doc.set_i64(&[RULES_KEY, "kinds"], whole(self.min_classes));
        doc.set_i64(&[RULES_KEY, "strength"], i64::from(self.min_bits));
        doc.set_bool(&[RULES_KEY, "refuse_common"], self.disallow_common);
    }
}

/// The whole number kept at `rules.<key>`, if it is one from `lo` to `hi`.
///
/// No key is `None` and says nothing: the rule was never changed. A key
/// holding anything else is `None` too, with a sentence in `problems` saying
/// which and why.
fn kept_number(
    doc: &yamldoc::Document,
    key: &str,
    lo: i64,
    hi: i64,
    problems: &mut Vec<String>,
) -> Option<i64> {
    let path = [RULES_KEY, key];
    if !doc.contains(&path) {
        return None;
    }
    let n = doc.get_i64(&path).filter(|n| (lo..=hi).contains(n));
    if n.is_none() {
        problems.push(format!(
            "{CONFIG_NAME}.yaml: {RULES_KEY}.{key} is not a whole number from {lo} to {hi}, \
             so the built-in rule is used"
        ));
    }
    n
}

/// The on-or-off rule kept at `rules.<key>`, read the way [`kept_number`]
/// reads a number.
fn kept_switch(doc: &yamldoc::Document, key: &str, problems: &mut Vec<String>) -> Option<bool> {
    let path = [RULES_KEY, key];
    if !doc.contains(&path) {
        return None;
    }
    let on = doc.get_bool(&path);
    if on.is_none() {
        problems.push(format!(
            "{CONFIG_NAME}.yaml: {RULES_KEY}.{key} is not true or false, so the built-in rule \
             is used"
        ));
    }
    on
}

/// "1 character", "8 characters".
fn characters(n: usize) -> String {
    if n == 1 {
        "1 character".to_owned()
    } else {
        format!("{n} characters")
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
    /// The rules a password is checked against, and what they say of the
    /// ones on show.
    Rules,
}

impl ActiveTab {
    /// Every tab, in the order the toolbar draws them and `Tab` visits them.
    pub const ALL: [ActiveTab; 4] = [Self::Generator, Self::Analyzer, Self::History, Self::Rules];

    pub fn label(self) -> &'static str {
        match self {
            Self::Generator => "Generator",
            Self::Analyzer => "Analyzer",
            Self::History => "History",
            Self::Rules => "Rules",
        }
    }

    /// The tab after this one, round to the first.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Generator => Self::Analyzer,
            Self::Analyzer => Self::History,
            Self::History => Self::Rules,
            Self::Rules => Self::Generator,
        }
    }
}

// ============================================================================
// The Rules tab
// ============================================================================

/// One line of the Rules tab: a rule, the words it is drawn with, and how
/// the arrow keys change it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleRow {
    Shortest,
    Longest,
    Lowercase,
    Uppercase,
    Digit,
    Symbol,
    Kinds,
    Strength,
    Common,
}

impl RuleRow {
    /// Every row, in the order the tab draws them.
    pub const ALL: [RuleRow; 9] = [
        Self::Shortest,
        Self::Longest,
        Self::Lowercase,
        Self::Uppercase,
        Self::Digit,
        Self::Symbol,
        Self::Kinds,
        Self::Strength,
        Self::Common,
    ];

    /// What the rule is, as the tab says it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Shortest => "Shortest allowed",
            Self::Longest => "Longest allowed",
            Self::Lowercase => "Must have a lowercase letter",
            Self::Uppercase => "Must have an uppercase letter",
            Self::Digit => "Must have a digit",
            Self::Symbol => "Must have a symbol",
            Self::Kinds => "Kinds of character it must mix",
            Self::Strength => "Least strength",
            Self::Common => "Refuse the most common passwords",
        }
    }

    /// What it is set to, as the tab says it.
    #[must_use]
    pub fn value(self, rules: &PasswordPolicy) -> String {
        let yes_no = |on: bool| if on { "Yes" } else { "No" }.to_owned();
        match self {
            Self::Shortest => characters(rules.min_length),
            Self::Longest => rules
                .max_length
                .map_or_else(|| "No limit".to_owned(), characters),
            Self::Lowercase => yes_no(rules.require_lowercase),
            Self::Uppercase => yes_no(rules.require_uppercase),
            Self::Digit => yes_no(rules.require_digit),
            Self::Symbol => yes_no(rules.require_symbol),
            Self::Kinds => format!("{} of the four", rules.min_classes),
            Self::Strength => format!("{} bits", rules.min_bits),
            Self::Common => yes_no(rules.disallow_common),
        }
    }

    /// Whether the rule is on or off, rather than a number.
    #[must_use]
    pub fn is_switch(self) -> bool {
        matches!(
            self,
            Self::Lowercase | Self::Uppercase | Self::Digit | Self::Symbol | Self::Common
        )
    }
}

/// Move the longest-allowed rule by `delta`. "No limit" sits just above the
/// largest number, so Right from the top turns the limit off and Left from
/// "No limit" turns it back on at the top; nothing goes below `shortest`.
fn step_longest(current: Option<usize>, delta: isize, shortest: usize) -> Option<usize> {
    let no_limit = RULE_LENGTH_MAX.saturating_add(1);
    let lowest = shortest.clamp(1, RULE_LENGTH_MAX);
    let moved = step(current.unwrap_or(no_limit), delta, lowest, no_limit);
    (moved < no_limit).then_some(moved)
}

/// How the Rules tab's keys are said on the tab itself.
const RULES_HINT: &str =
    "Up / Down chooses a rule, Left / Right changes it, Space turns it on or off";

/// The Rules tab's rows: a row, and the gap under it.
const RULE_ROW_PITCH: f32 = ITEM_HEIGHT + 4.0;

/// One line of what the rules say of a password.
const RULE_LINE_HEIGHT: f32 = 16.0;

/// What the analyser draws for each character it is not showing.
const MASK: char = '\u{2022}';

/// The most the analyser's box holds, in characters: a password, or a
/// passphrase of many words, and a paste of a page is neither.
const ANALYZER_CAPACITY: usize = 1024;

/// The size the password box's text is drawn at.
const PASSWORD_TEXT_SIZE: f32 = 13.0;

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
    // `clamp` panics on an empty range. No caller builds one, and this keeps
    // a range worked out at run time -- a rule read from a file, say -- from
    // ever being the first.
    if lo > hi {
        return value;
    }
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

/// The keys this program answers, raised by `F1`.
///
/// `?` is not a second way in: on the analyser tab every printable key is part
/// of the password being measured, so a `?` has somewhere to go -- the
/// `apps/spreadsheet` case in design-decisions 863.
///
/// The five generator letters name what they make, which is the only reason
/// they are letters rather than a menu.
///
/// **The option keys are deliberately not here.** Each `OptionRow` prints its
/// own keystroke in the panel and `OptionRow::from_key` parses that printed
/// label to decide the match -- one string, drawn and matched, with
/// `every_option_row_names_a_keystroke_that_parses` and
/// `every_option_row_answers_its_key_and_the_panel_shows_it` already holding
/// it. Copying those keys into this list would be the third copy that
/// arrangement exists to avoid.
///
/// An earlier draft of this list carried a `Up / Down` row for "move through
/// the options". The app bound neither key then; the row was written from a
/// glance at `toggle_option` and was pure invention.
/// `every_advertised_key_does_something` caught it on its first run, which is
/// the guard working on the author rather than on the app. The `Up / Down`
/// row here now is the Rules tab's, which does bind them.
const SHORTCUTS: &[(&str, &str)] = &[
    ("1-4", "Generator, analyser, history, rules"),
    ("Tab", "The next tab"),
    ("P", "A password"),
    ("W", "A passphrase"),
    ("N", "A PIN"),
    ("R", "A pronounceable one"),
    ("B", "A batch of them"),
    ("Space / Enter", "Another of the same kind"),
    ("Left / Right", "Shorter / longer"),
    ("Up / Down", "Choose a rule, on the rules tab"),
    ("C", "Clear the history"),
    ("Ctrl+E", "Export"),
    ("Ctrl+R", "Show or hide what the analyser measures"),
    ("F1", "This list"),
];

pub struct PasswordApp {
    pub password_opts: PasswordOptions,
    pub passphrase_opts: PassphraseOptions,
    pub current_password: String,
    pub current_analysis: Option<PasswordAnalysis>,
    pub analyzer_input: String,
    pub history: Vec<HistoryEntry>,
    pub policy: PasswordPolicy,
    /// Which rule the Rules tab's cursor is on, as an index into
    /// [`RuleRow::ALL`].
    pub rule_cursor: usize,
    /// Whether the analyser draws what is typed into it, rather than a dot
    /// for each character. Off to begin with: the analyser is where somebody
    /// types a password they use, and a screen is read over a shoulder.
    pub analyzer_revealed: bool,
    /// The analyser's editor -- its caret and selection over
    /// `analyzer_input` -- reloaded when the input changed under it.
    analyzer_editor: TextInput,
    /// What the analyser's Ctrl+C and Ctrl+X took while it was shown, for
    /// its Ctrl+V. Nothing is taken from it hidden.
    clipboard: String,
    /// Whether this window keeps the rules in `passwordgen.yaml`. Set by
    /// [`PasswordApp::with_settings`], which `main` calls; a window a test
    /// builds keeps nothing, so no test writes the developer's own settings.
    keeps_settings: bool,
    /// What in `passwordgen.yaml` could not be used when the window opened,
    /// one sentence each -- drawn on the Rules tab until a rule is changed,
    /// which writes the file whole again.
    pub settings_problems: Vec<String>,
    pub active_tab: ActiveTab,
    /// What the generator tab is producing; see [`GenKind`].
    pub gen_kind: GenKind,
    pub pin_length: usize,
    pub bulk_count: usize,
    pub bulk_results: Vec<String>,
    pub window_width: f32,
    pub window_height: f32,
    /// The save dialog, when the user is choosing where to export to.
    ///
    /// `export_history` has rendered the generated passwords as text since the
    /// program was written and was called by nothing but its own test: the
    /// string had nowhere to go. This is the somewhere.
    pub dialog: Option<FileDialog>,
    /// How wide the mark is round the password box while it has the
    /// keyboard: the user's focus width (`App::appearance_changed`), the
    /// toolkit's until it is known.
    pub focus_ring_width: f32,
    /// What the last export did, shown in the status bar until the next one.
    ///
    /// Writing a file is the one thing this program does that the window does
    /// not already show. A generated password is visible; a file either
    /// appeared or did not, and the user is looking at the wrong window to
    /// find out.
    pub status: Option<String>,
    /// Set when a generation was refused because the kernel CSPRNG was not
    /// available. Shown in place of the password, so that the refusal is
    /// visible rather than looking like a button that did nothing.
    pub last_error: Option<String>,
    rng: AppRandom,
    timestamp: u64,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// Whether the shortcut card is up.
    show_help: bool,
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
            show_help: false,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            password_opts: PasswordOptions::default(),
            passphrase_opts: PassphraseOptions::default(),
            current_password: String::new(),
            current_analysis: None,
            analyzer_input: String::new(),
            history: Vec::new(),
            policy: PasswordPolicy::default(),
            rule_cursor: 0,
            analyzer_revealed: false,
            analyzer_editor: TextInput::new(),
            clipboard: String::new(),
            keeps_settings: false,
            settings_problems: Vec::new(),
            active_tab: ActiveTab::Generator,
            gen_kind: GenKind::Password,
            pin_length: 6,
            bulk_count: 10,
            bulk_results: Vec::new(),
            window_width: 1100.0,
            window_height: 700.0,
            dialog: None,
            focus_ring_width: guitk::style::FOCUS_RING_WIDTH,
            status: None,
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
    ///
    /// Nothing typed is nothing measured: an empty field rated "Very Weak"
    /// would be a verdict on a password nobody has.
    pub fn analyze_input(&mut self) {
        self.current_analysis =
            (!self.analyzer_input.is_empty()).then(|| analyze_password(&self.analyzer_input));
    }

    /// Set analyzer input.
    pub fn set_analyzer_input(&mut self, input: &str) {
        self.analyzer_input = input.to_owned();
    }

    /// Every rule the password on show breaks -- see
    /// [`PasswordApp::shown_password`].
    #[must_use]
    pub fn check_policy(&self) -> Vec<String> {
        self.policy.check(self.shown_password())
    }

    /// The password the window is showing: the one being typed on the
    /// analyser tab, the generated one on every other.
    #[must_use]
    pub fn shown_password(&self) -> &str {
        if self.active_tab == ActiveTab::Analyzer {
            &self.analyzer_input
        } else {
            &self.current_password
        }
    }

    /// This window, with the rules the user set last time -- and keeping any
    /// they change from now on.
    #[must_use]
    pub fn with_settings(mut self) -> Self {
        self.keeps_settings = true;
        self.read_settings();
        self
    }

    /// Read the rules from `passwordgen.yaml`, and say what in it could not
    /// be used: when the window opens, and again whenever the desktop says
    /// the file changed.
    fn read_settings(&mut self) {
        let (rules, problems) = PasswordPolicy::from_settings(&settingsfile::load(CONFIG_NAME));
        self.policy = rules;
        if let Some(first) = problems.first() {
            // The status bar has room for one; the Rules tab lists them all.
            self.status = Some(match problems.len() {
                1 => first.clone(),
                n => format!(
                    "{first} (and {} more: see the Rules tab)",
                    n.saturating_sub(1)
                ),
            });
        }
        self.settings_problems = problems;
    }

    /// Read the rules again after the desktop said `passwordgen.yaml`
    /// changed -- another window's Rules tab, or a hand edit (§1418, §1434).
    /// A window that keeps no settings, as a test's does not, reads none.
    /// Whether what the window shows changed.
    fn reread_settings(&mut self) -> bool {
        if !self.keeps_settings {
            return false;
        }
        let before = (self.policy.clone(), self.settings_problems.clone());
        self.read_settings();
        before != (self.policy.clone(), self.settings_problems.clone())
    }

    /// Move the Rules tab's cursor by `delta` rows.
    fn move_rule_cursor(&mut self, delta: isize) -> EventResult {
        let before = self.rule_cursor;
        self.rule_cursor = step(before, delta, 0, RuleRow::ALL.len().saturating_sub(1));
        if self.rule_cursor == before {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    /// Change the rule under the cursor: a number moves one step the way the
    /// key points, a switch flips.
    ///
    /// The two lengths hold each other in place -- the shortest cannot pass
    /// the longest, nor the longest the shortest -- so a rule no password
    /// could meet cannot be set from here.
    fn change_rule(&mut self, delta: isize) -> EventResult {
        let Some(row) = RuleRow::ALL.get(self.rule_cursor).copied() else {
            return EventResult::Ignored;
        };
        let before = self.policy.clone();
        let rules = &mut self.policy;
        match row {
            RuleRow::Shortest => {
                let most = rules
                    .max_length
                    .unwrap_or(RULE_LENGTH_MAX)
                    .clamp(1, RULE_LENGTH_MAX);
                rules.min_length = step(rules.min_length, delta, 1, most);
            }
            RuleRow::Longest => {
                rules.max_length = step_longest(rules.max_length, delta, rules.min_length);
            }
            RuleRow::Kinds => rules.min_classes = step(rules.min_classes, delta, 1, 4),
            RuleRow::Strength => {
                rules.min_bits = if delta < 0 {
                    rules.min_bits.saturating_sub(RULE_BITS_STEP)
                } else {
                    rules
                        .min_bits
                        .saturating_add(RULE_BITS_STEP)
                        .min(RULE_BITS_MAX)
                };
            }
            RuleRow::Lowercase => rules.require_lowercase = !rules.require_lowercase,
            RuleRow::Uppercase => rules.require_uppercase = !rules.require_uppercase,
            RuleRow::Digit => rules.require_digit = !rules.require_digit,
            RuleRow::Symbol => rules.require_symbol = !rules.require_symbol,
            RuleRow::Common => rules.disallow_common = !rules.disallow_common,
        }
        if self.policy == before {
            return EventResult::Ignored;
        }
        self.keep_rules();
        EventResult::Consumed
    }

    /// Space or Enter on the Rules tab: flips a switch, and does nothing to
    /// a number, which has two directions that Space names neither of.
    fn toggle_rule(&mut self) -> EventResult {
        match RuleRow::ALL.get(self.rule_cursor) {
            Some(row) if row.is_switch() => self.change_rule(1),
            _ => EventResult::Ignored,
        }
    }

    /// Keep the rules in `passwordgen.yaml`, if this window keeps settings,
    /// and say so when they could not be kept.
    fn keep_rules(&mut self) {
        if !self.keeps_settings {
            return;
        }
        let mut doc = settingsfile::load(CONFIG_NAME);
        self.policy.store_into(&mut doc);
        match settingsfile::store(CONFIG_NAME, &doc) {
            Ok(()) => {
                // Written whole, so whatever the file held that could not be
                // used when the window opened has been replaced -- and the
                // user, who was told about it, is told that too.
                if !self.settings_problems.is_empty() {
                    self.settings_problems.clear();
                    self.status = Some(format!(
                        "{CONFIG_NAME}.yaml now holds the rules as they are shown"
                    ));
                }
            }
            Err(e) => {
                self.status = Some(format!(
                    "The rule holds until the window closes -- it was not saved: {e}"
                ));
            }
        }
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
        // The rules on disk changed and the desktop says so. Before the
        // picker, which would take this as it takes every event but a
        // resize: a change to the file is not an input the dialog owns.
        if let Event::SettingsChanged { group } = event
            && group.file_name() == CONFIG_NAME
        {
            return if self.reread_settings() {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            };
        }
        // The picker is modal and answers everything but a resize. Every key
        // on the generator tab produces a password, so a keystroke that fell
        // through to the tab behind would generate one while the user was
        // typing a filename.
        if !matches!(event, Event::Resize { .. })
            && let Some(result) = self.dialog_event(event)
        {
            return result;
        }
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
    /// Put up the save dialog for the generated-password history.
    ///
    /// Refused when there is nothing to write: a file picker for an empty
    /// export is a question with one useless answer, and an empty file on disk
    /// is worse than no file.
    fn open_export_dialog(&mut self) -> EventResult {
        if self.history.is_empty() {
            self.status = Some("Nothing generated yet, so nothing to export".to_string());
            return EventResult::Consumed;
        }
        let start = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let mut dialog = FileDialog::save()
            .with_initial_path(start)
            .with_filename("passwords.txt");
        dialog.set_entries(guitk::dialog::list_directory(dialog.current_path()));
        self.dialog = Some(dialog);
        EventResult::Consumed
    }

    /// Write the history to `path`, and say what happened.
    ///
    /// Atomic, because a half-written export is a file of passwords that looks
    /// complete and is not -- and the reader has no way to tell, since a
    /// truncated list of random strings is indistinguishable from a short one.
    fn export_to(&mut self, path: &std::path::Path) {
        let text = self.export_history();
        self.status = Some(match safeio::write_str_atomically(path, &text) {
            Ok(()) => format!("Exported {} to {}", self.history.len(), path.shown()),
            Err(e) => format!("Could not write {}: {e}", path.shown()),
        });
    }

    /// Give the picker an event; `None` when there is no picker up.
    fn dialog_event(&mut self, event: &Event) -> Option<EventResult> {
        let (width, height) = (self.window_width, self.window_height);
        let action = {
            let dialog = self.dialog.as_mut()?;
            match event {
                Event::Key(key) if key.pressed => dialog.handle_event(key, height),
                Event::Mouse(mouse) => dialog.handle_mouse(mouse, width, height),
                _ => DialogAction::None,
            }
        };
        match action {
            DialogAction::Selected(path) => {
                self.dialog = None;
                self.export_to(&path);
            }
            DialogAction::Cancelled => self.dialog = None,
            DialogAction::NavigatedTo(path) => {
                if let Some(dialog) = self.dialog.as_mut() {
                    dialog.set_entries(guitk::dialog::list_directory(&path));
                }
            }
            DialogAction::None => {}
        }
        Some(EventResult::Consumed)
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        // Before the analyser branch, for the same reason Ctrl+E is: on that
        // tab every printable key is the password being measured, and `F1` is
        // not printable.
        if key.key == Key::F1 {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            // Modal. Letting keys through would mean generating a password the
            // reader cannot see, over the one they were looking at.
            if matches!(key.key, Key::Escape | Key::Enter | Key::F1) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }

        // Before the analyser branch: on that tab every printable key is the
        // password being measured, and Ctrl+E must not be typed into it.
        // Ctrl without Alt: AltGr arrives as Ctrl+Alt, and AltGr+E is `€` on
        // most European keyboards -- a character of the password, not a
        // way to the export dialog.
        if textline::is_ctrl_chord(key.modifiers) && key.key == Key::E {
            return self.open_export_dialog();
        }
        if self.active_tab == ActiveTab::Analyzer {
            match key.key {
                // Tab still moves on, or there would be no way out of the box.
                // The digit keys do not: here a digit is part of the password,
                // and "abc123" used to jump to the generator at the "1".
                Key::Tab => {}
                // A dot for each character until asked: see
                // `analyzer_revealed`.
                Key::R if textline::is_ctrl_chord(key.modifiers) => {
                    self.analyzer_revealed = !self.analyzer_revealed;
                    return EventResult::Consumed;
                }
                _ => return self.analyzer_key(key),
            }
        }
        // Every key from here on is a bare key, and one held with Ctrl, Alt
        // or the Windows key is not this window's. Ctrl+C cleared the
        // history as C does -- the key a user presses to copy a password --
        // and Ctrl+P made a new one over the one on screen; the Windows
        // key's are the desktop's, and AltGr types letters.
        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {
            return EventResult::Ignored;
        }
        // On the Rules tab the arrows and Space are the rules'. Everything
        // else means what it means on any tab.
        if self.active_tab == ActiveTab::Rules {
            match key.key {
                Key::Up => return self.move_rule_cursor(-1),
                Key::Down => return self.move_rule_cursor(1),
                Key::Left => return self.change_rule(-1),
                Key::Right => return self.change_rule(1),
                Key::Space | Key::Enter => return self.toggle_rule(),
                _ => {}
            }
        }
        match key.key {
            Key::Num1 => self.set_tab(ActiveTab::Generator),
            Key::Num2 => self.set_tab(ActiveTab::Analyzer),
            Key::Num3 => self.set_tab(ActiveTab::History),
            Key::Num4 => self.set_tab(ActiveTab::Rules),
            Key::Tab => {
                let next = self.active_tab.next();
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
            // The character classes. Every one of these was drawn in the
            // options panel as "Yes" or "No" and none could be changed:
            // `length` was the only field of `password_opts` with a writer.
            // So every password this program produced contained symbols, and
            // sites that forbid symbols are common enough to make a generator
            // that cannot drop them useless for them. Nor could the
            // easy-to-misread characters be left out.
            _ if OptionRow::from_key(key.key, key.modifiers.shift).is_some() => {
                // `is_some` above, unwrapped here: the guard and the body ask
                // the same list the same question, one keystroke apart.
                match OptionRow::from_key(key.key, key.modifiers.shift) {
                    Some(row) => self.toggle_option(row),
                    None => EventResult::Ignored,
                }
            }
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

    /// A key for the password being measured: the caret keys, Backspace and
    /// Delete at the caret, Ctrl+A and V, and typing -- what AltGr types
    /// among it (`®` is AltGr+R on US International), and no command's
    /// letter, which a chord carries as text, nor a control character: an
    /// Enter carrying a "\r" is a key, not part of a password. Hidden, the
    /// box is a masked field (`textline::apply_masked_key`): the arrows step
    /// a character at a time, and nothing is copied or cut from it. Shown,
    /// it copies as any field does. `Consumed` where the key changed the
    /// password, its caret or its selection; a password changed is measured
    /// at once, or the strength meter would go on showing the last one's
    /// score. The box took typing at its end and Backspace from it, and
    /// nothing else.
    fn analyzer_key(&mut self, key: &KeyEvent) -> EventResult {
        if self.analyzer_editor.text() != self.analyzer_input {
            self.analyzer_editor.set_text(&self.analyzer_input);
        }
        let editor = &self.analyzer_editor;
        let before = (
            editor.text().to_owned(),
            editor.cursor(),
            editor.selection_anchor(),
        );
        let edit = if self.analyzer_revealed {
            textline::apply_key(
                &mut self.analyzer_editor,
                key,
                ANALYZER_CAPACITY,
                &self.clipboard,
                PASSWORD_TEXT_SIZE,
            )
        } else {
            textline::apply_masked_key(
                &mut self.analyzer_editor,
                key,
                ANALYZER_CAPACITY,
                &self.clipboard,
            )
        };
        if let Some(copied) = edit.copied {
            self.clipboard = copied;
        }
        if !edit.handled {
            return EventResult::Ignored;
        }
        if self.analyzer_editor.text() != self.analyzer_input {
            let typed = self.analyzer_editor.text().to_owned();
            self.set_analyzer_input(&typed);
            self.analyze_input();
        }
        let editor = &self.analyzer_editor;
        let after = (
            editor.text().to_owned(),
            editor.cursor(),
            editor.selection_anchor(),
        );
        if after == before {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    /// Where the analyser's caret and selection anchor are in the password:
    /// the editor's while it holds the password, after it and none
    /// otherwise.
    fn analyzer_caret(&self) -> (text::TextCursor, Option<usize>) {
        if self.analyzer_editor.text() == self.analyzer_input {
            (
                self.analyzer_editor.cursor(),
                self.analyzer_editor.selection_anchor(),
            )
        } else {
            (text::TextCursor::from(self.analyzer_input.len()), None)
        }
    }

    /// Turn a character class on or off, and produce one with the new set.
    ///
    /// Produces immediately for the reason the kind keys do: leaving the
    /// previous password on screen after changing what a password may contain
    /// invites reading the old one as the new setting's output.
    ///
    /// Refuses to turn off the last class. `generate_password` answers an
    /// empty pool with an empty string, so a generator with nothing selected
    /// would produce nothing at all and say nothing about why.
    fn toggle_class(&mut self, class: CharClass) -> EventResult {
        let on = match class {
            CharClass::Lower => self.password_opts.use_lowercase,
            CharClass::Upper => self.password_opts.use_uppercase,
            CharClass::Digits => self.password_opts.use_digits,
            CharClass::Symbols => self.password_opts.use_symbols,
        };
        if on && self.password_opts.active_classes() <= 1 {
            self.status = Some("A password needs at least one kind of character".to_owned());
            return EventResult::Consumed;
        }
        match class {
            CharClass::Lower => self.password_opts.use_lowercase = !on,
            CharClass::Upper => self.password_opts.use_uppercase = !on,
            CharClass::Digits => self.password_opts.use_digits = !on,
            CharClass::Symbols => self.password_opts.use_symbols = !on,
        }
        let kind = self.gen_kind;
        self.generate(kind)
    }

    /// Change one option row, whichever it is.
    ///
    /// The class rows go through `toggle_class` so they keep its guard
    /// against turning off the last one; the rest flip directly. Every arm
    /// produces a new secret afterwards for the reason the kind keys do --
    /// leaving the old one on screen invites reading it as the new setting's
    /// output.
    fn toggle_option(&mut self, row: OptionRow) -> EventResult {
        match row {
            OptionRow::Lowercase => return self.toggle_class(CharClass::Lower),
            OptionRow::Uppercase => return self.toggle_class(CharClass::Upper),
            OptionRow::Digits => return self.toggle_class(CharClass::Digits),
            OptionRow::Symbols => return self.toggle_class(CharClass::Symbols),
            OptionRow::ExcludeAmbiguous => return self.toggle_ambiguous(),
            OptionRow::EveryClass => {
                self.password_opts.must_include_each_class =
                    !self.password_opts.must_include_each_class;
            }
            OptionRow::Capitalize => {
                self.passphrase_opts.capitalize = !self.passphrase_opts.capitalize;
            }
            OptionRow::EndWithDigit => {
                self.passphrase_opts.add_number = !self.passphrase_opts.add_number;
            }
            OptionRow::EndWithSymbol => {
                self.passphrase_opts.add_symbol = !self.passphrase_opts.add_symbol;
            }
        }
        let kind = self.gen_kind;
        self.generate(kind)
    }

    /// Include or leave out the characters that are easy to misread.
    ///
    /// `l` and `1`, `O` and `0`: the difference between a password you can
    /// read off a screen and one you cannot.
    fn toggle_ambiguous(&mut self) -> EventResult {
        self.password_opts.exclude_ambiguous = !self.password_opts.exclude_ambiguous;
        let kind = self.gen_kind;
        self.generate(kind)
    }

    /// Switch tabs, reporting whether anything changed.
    fn set_tab(&mut self, tab: ActiveTab) -> EventResult {
        if self.active_tab == tab {
            return EventResult::Ignored;
        }
        self.active_tab = tab;
        // The strength on show is of the password on show. Carried across
        // unchanged, the generator tab went on showing the strength of
        // whatever had last been typed into the analyser, beside a different
        // password -- and the analyser showed the generated one's before
        // anything was typed.
        let shown = self.shown_password();
        self.current_analysis = (!shown.is_empty()).then(|| analyze_password(shown));
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
            color: self.palette.base,
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

        if self.show_help {
            guitk::shortcut::render_card(
                &mut cmds,
                &self.palette,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
        }

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        self.palette.push_surface(
            cmds,
            0.0,
            0.0,
            width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "Password Generator".to_owned(),
            color: self.palette.ink(self.palette.blue),
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Tab buttons
        let mut tx = 220.0;
        for tab in &ActiveTab::ALL {
            let is_active = *tab == self.active_tab;
            let btn_w = text::padded_width_any_weight(tab.label(), 10.0, 11.0);
            self.palette.push_surface(
                cmds,
                tx,
                8.0,
                btn_w,
                24.0,
                CORNER_RADIUS,
                if is_active {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: 14.0,
                text: tab.label().to_owned(),
                color: if is_active {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
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
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        let bar_y = height - STATUS_BAR_HEIGHT;
        self.palette.push_surface(
            cmds,
            0.0,
            bar_y,
            width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );

        // The export's outcome displaces the counts while it is showing.
        // Both would not fit, and of the two, "did the file get written" is
        // the one the window cannot otherwise answer: a generated password is
        // on screen, a file on disk is not.
        let status = if let Some(message) = &self.status {
            message.clone()
        } else {
            // Of the password on show -- the typed one on the analyser tab.
            // This used to judge the generated one on every tab, beside a
            // strength meter measuring the typed one. And nothing is judged
            // when there is nothing to judge: an empty field "breaking" the
            // shortest-length rule is not news.
            let shown = self.shown_password();
            let rules = if shown.is_empty() {
                String::new()
            } else {
                match self.policy.check(shown).len() {
                    0 => "  |  Meets your rules".to_owned(),
                    1 => "  |  Breaks 1 of your rules (the Rules tab says which)".to_owned(),
                    n => format!("  |  Breaks {n} of your rules (the Rules tab says which)"),
                }
            };
            format!("{} passwords generated{rules}", self.history.len())
        };
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: bar_y + 6.0,
            text: status,
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_left_panel(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        self.palette
            .push_surface(cmds, 0.0, y, LEFT_PANEL_WIDTH, height, 0.0, Surface::Card);

        cmds.push(RenderCommand::Line {
            x1: LEFT_PANEL_WIDTH,
            y1: y,
            x2: LEFT_PANEL_WIDTH,
            y2: y + height,
            color: self.palette.surface0,
            width: 1.0,
        });

        if self.active_tab == ActiveTab::Analyzer {
            self.render_analyzer_input(cmds, y, height);
            return;
        }

        let mut cy = y + 12.0;
        let lx = 12.0;
        let max_w = LEFT_PANEL_WIDTH - 24.0;

        // Current password display
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "GENERATED PASSWORD".to_owned(),
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 18.0;

        self.palette
            .push_surface(cmds, lx, cy, max_w, 32.0, CORNER_RADIUS, Surface::Card);
        // A refusal takes this slot: the user pressed Generate, so the answer
        // to "where is my password" belongs where the password would be, not
        // in a corner they have no reason to look at.
        let (pw_display, pw_color) = match (&self.last_error, self.current_password.is_empty()) {
            (Some(message), _) => (message.clone(), self.palette.red),
            (None, true) => (
                "Click Generate to create a password".to_owned(),
                self.palette.overlay0,
            ),
            (None, false) => (self.current_password.clone(), self.palette.text),
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
            ("Generate Password", self.palette.green),
            ("Generate Passphrase", self.palette.teal),
            ("Generate PIN", self.palette.yellow),
            ("Pronounceable", self.palette.mauve),
        ];

        for (label, color) in &buttons {
            let btn_w = text::padded_width(label, 12.0, 11.0, FontWeightHint::Bold);
            self.palette.push_surface(
                cmds,
                lx,
                cy,
                btn_w.min(max_w),
                28.0,
                CORNER_RADIUS,
                Surface::Card,
            );
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
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 18.0;

        let mut options = vec![(
            format!("Length (Left/Right): {}", self.password_opts.length),
            true,
        )];
        // Drawn from the same list the keys are read from, so a row
        // cannot appear here without a key that changes it.
        options.extend(OptionRow::ALL.map(|row| {
            (
                format!(
                    "{} ({}): {}",
                    row.label(),
                    row.keys(),
                    if row.is_on(self) { "Yes" } else { "No" }
                ),
                row.is_on(self),
            )
        }));

        for (label, active) in &options {
            let text_color = if *active {
                self.palette.text
            } else {
                self.palette.overlay0
            };
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
                    color: self.palette.subtext0,
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
                        color: analysis.rating.color(&self.palette),
                        corner_radii: CornerRadii::all(CORNER_RADIUS),
                    });
                    cmds.push(RenderCommand::Text {
                        x: lx + 10.0,
                        y: cy + 8.0,
                        text: analysis.rating.label().to_owned(),
                        color: self.palette.crust,
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
                        color: self.palette.text,
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
                            color: self.palette.text,
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
                        color: self.palette.subtext0,
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
                            color: self.palette.subtext0,
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(150.0),
                            overflow: TextOverflow::Ellipsis,
                        });
                        cmds.push(RenderCommand::Text {
                            x: lx + 160.0,
                            y: cy,
                            text: (*value).clone(),
                            color: self.palette.text,
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
                            color: self.palette.subtext0,
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
                            &self.palette,
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
                        // What to do to see one, which is not the same on the
                        // two tabs that draw it.
                        text: if self.active_tab == ActiveTab::Analyzer {
                            "Type a password on the left to see how strong it is"
                        } else {
                            "Generate a password to see analysis"
                        }
                        .to_owned(),
                        color: self.palette.subtext0,
                        font_size: 13.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_w),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
            ActiveTab::Rules => self.render_rules(cmds, lx, cy, max_w, y + height),
            ActiveTab::History => {
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y: cy,
                    text: format!("HISTORY ({} entries)", self.history.len()),
                    color: self.palette.subtext0,
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
                    self.palette.push_surface(
                        cmds,
                        lx,
                        cy,
                        max_w,
                        ITEM_HEIGHT,
                        CORNER_RADIUS,
                        Surface::Card,
                    );

                    // Strength dot
                    cmds.push(RenderCommand::FillRect {
                        x: lx + 8.0,
                        y: cy + 10.0,
                        width: 8.0,
                        height: 8.0,
                        color: entry.strength.color(&self.palette),
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
                        color: self.palette.text,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_w - 140.0),
                        overflow: TextOverflow::Ellipsis,
                    });

                    cmds.push(RenderCommand::Text {
                        x: lx + max_w - 110.0,
                        y: cy + 8.0,
                        text: format!("[{}] {:.0}b", entry.gen_type, entry.entropy),
                        color: self.palette.subtext1,
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
                        color: self.palette.subtext0,
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

/// Where a list is drawn: its left edge, its width, and the line it must not
/// cross.
#[derive(Clone, Copy)]
struct Column {
    x: f32,
    width: f32,
    bottom: f32,
}

/// A section heading, in the small capitals every panel here uses.
fn heading(pal: &Palette, label: &str, x: f32, y: f32, width: f32) -> RenderCommand {
    RenderCommand::Text {
        x,
        y,
        text: label.to_owned(),
        color: pal.subtext0,
        font_size: 10.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width),
        overflow: TextOverflow::Ellipsis,
    }
}

/// Draw `lines` from `top` in `color`, as many as fit above the column's
/// bottom; when they do not all fit, the last row that does says how many
/// were left out. Returns the cursor after the last row drawn.
///
/// `render_pattern_list`'s rule, for its reason: a list cut short without
/// saying so reads as the whole of it.
fn push_bounded_lines(
    cmds: &mut Vec<RenderCommand>,
    pal: &Palette,
    lines: &[String],
    color: Color,
    column: Column,
    top: f32,
) -> f32 {
    let total = lines.len();
    let room = rows_that_fit(top, column.bottom, RULE_LINE_HEIGHT);
    let overflowing = total > room;
    let shown = if overflowing {
        room.saturating_sub(1)
    } else {
        total
    };
    let mut cy = top;
    for line in lines.iter().take(shown) {
        cmds.push(RenderCommand::Text {
            x: column.x,
            y: cy,
            text: line.clone(),
            color,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(column.width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += RULE_LINE_HEIGHT;
    }
    if overflowing && room > 0 {
        cmds.push(RenderCommand::Text {
            x: column.x,
            y: cy,
            text: format!("+{} more", total.saturating_sub(shown)),
            color: pal.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(column.width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += RULE_LINE_HEIGHT;
    }
    cy
}

impl PasswordApp {
    /// What the rules say of `password`, from `top`: "Meets every rule", or
    /// each rule it breaks. Returns the cursor after.
    fn push_verdict(
        &self,
        cmds: &mut Vec<RenderCommand>,
        password: &str,
        column: Column,
        top: f32,
    ) -> f32 {
        let broken = self.policy.check(password);
        if broken.is_empty() {
            let met = ["Meets every rule".to_owned()];
            return push_bounded_lines(
                cmds,
                &self.palette,
                &met,
                self.palette.ink(self.palette.green),
                column,
                top,
            );
        }
        push_bounded_lines(
            cmds,
            &self.palette,
            &broken,
            self.palette.ink(self.palette.red),
            column,
            top,
        )
    }

    /// The analyser's half of the window: the password being measured -- a
    /// dot for each character until the user asks to see it -- and what the
    /// rules say of it.
    ///
    /// Until 2026-09-27 this tab drew the generator here, so what was typed
    /// was never on screen at all: the meter measured a password nobody
    /// could see, and a typo in it could not be found.
    /// How the password box is drawn: with the keyboard -- every printable
    /// key on this tab is the password -- unless the list of keys or the
    /// export dialog is over it. Never lit under the pointer: it has no
    /// press of its own. Never red: what is in it is being measured, not
    /// judged; the rules say what they make of it, below it.
    fn password_box_state(&self) -> field::State {
        field::State {
            hovered: false,
            focused: self.active_tab == ActiveTab::Analyzer
                && !self.show_help
                && self.dialog.is_none(),
            disabled: false,
            invalid: false,
        }
    }

    /// The password box at `rect`, holding `typed` characters: the toolkit's
    /// field, with the password -- a dot a character until Ctrl+R shows it --
    /// and its caret and selection where they are, on the same characters
    /// of the dots, scrolled so the caret stays in view; a grey hint while
    /// it is empty. It was a card holding the text, elided at its start,
    /// with no caret at all; then a caret fixed at the end, the only place
    /// the keys could type.
    fn render_password_box(
        &self,
        cmds: &mut Vec<RenderCommand>,
        rect: guitk::frame::Rect,
        typed: usize,
    ) {
        let state = self.password_box_state();
        field::draw(cmds, &self.palette, rect, state, self.focus_ring_width);
        let (size, weight) = (PASSWORD_TEXT_SIZE, FontWeightHint::Bold);
        let line = text::line_height(size, weight);
        let (tx, ty, tw) = (
            rect.x + 8.0,
            rect.y + (rect.h - line) / 2.0,
            (rect.w - 16.0).max(0.0),
        );
        if typed == 0 && !state.focused {
            cmds.push(RenderCommand::Text {
                x: tx,
                y: ty,
                text: "Type a password to measure it".to_owned(),
                color: self.palette.subtext0,
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some(tw),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }
        let (cursor, anchor) = self.analyzer_caret();
        let (shown, cursor, selection_anchor) = if self.analyzer_revealed {
            (self.analyzer_input.clone(), cursor, anchor)
        } else {
            let (dots, at, anchor) =
                textline::masked(&self.analyzer_input, cursor.byte(), anchor, MASK);
            (dots, text::TextCursor::from(at), anchor)
        };
        let mut tree = RenderTree::new();
        textedit::draw(
            &mut tree,
            &textedit::SingleLine {
                text: &shown,
                cursor,
                selection_anchor,
                focused: state.focused,
                x: tx,
                y: ty,
                width: tw,
                line_height: line,
                font_size: size,
                weight,
                color: self.palette.text,
                selection_bg: self.palette.accent,
                selection_fg: self.palette.crust,
                caret_width: textedit::CARET_WIDTH,
            },
        );
        cmds.extend(tree.commands);
    }

    fn render_analyzer_input(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        let lx = 12.0;
        let max_w = LEFT_PANEL_WIDTH - 24.0;
        let column = Column {
            x: lx,
            width: max_w,
            bottom: y + height,
        };
        let mut cy = y + 12.0;

        cmds.push(heading(&self.palette, "PASSWORD TO MEASURE", lx, cy, max_w));
        cy += 18.0;
        let typed = self.analyzer_input.chars().count();
        self.render_password_box(cmds, guitk::frame::Rect::new(lx, cy, max_w, 32.0), typed);
        cy += 44.0;

        let about = [
            format!(
                "{}. Ctrl+R {} it.",
                characters(typed),
                if self.analyzer_revealed {
                    "hides"
                } else {
                    "shows"
                }
            ),
            "Digits are part of the password here, not tab keys.".to_owned(),
            "Backspace takes the last character off.".to_owned(),
            "Tab moves on to the next tab.".to_owned(),
            "Nothing typed here is kept or written anywhere.".to_owned(),
        ];
        cy = push_bounded_lines(
            cmds,
            &self.palette,
            &about,
            self.palette.subtext0,
            column,
            cy,
        );

        if typed > 0 && cy + 30.0 + RULE_LINE_HEIGHT <= column.bottom {
            cy += 12.0;
            cmds.push(heading(&self.palette, "YOUR RULES", lx, cy, max_w));
            cy += 18.0;
            self.push_verdict(cmds, &self.analyzer_input, column, cy);
        }
    }

    /// The Rules tab: every rule and what it is set to, the cursor on one,
    /// and what the rules say of the passwords on show.
    fn render_rules(
        &self,
        cmds: &mut Vec<RenderCommand>,
        lx: f32,
        top: f32,
        max_w: f32,
        bottom: f32,
    ) {
        let column = Column {
            x: lx,
            width: max_w,
            bottom,
        };
        let mut cy = top;
        cmds.push(heading(&self.palette, "YOUR RULES", lx, cy, max_w));
        cy += 18.0;
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: RULES_HINT.to_owned(),
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 22.0;

        // Every row when they fit; when they do not, the ones up to the
        // cursor, so the rule the arrows are changing is always on screen.
        let room = rows_that_fit(cy, bottom, RULE_ROW_PITCH).min(RuleRow::ALL.len());
        let first = self.rule_cursor.saturating_add(1).saturating_sub(room);
        for (i, row) in RuleRow::ALL.iter().enumerate().skip(first).take(room) {
            let selected = i == self.rule_cursor;
            self.palette.push_surface(
                cmds,
                lx,
                cy,
                max_w,
                ITEM_HEIGHT,
                CORNER_RADIUS,
                if selected {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            let value = row.value(&self.policy);
            let value_w = text::measure(&value, 12.0, FontWeightHint::Bold);
            cmds.push(RenderCommand::Text {
                x: lx + 10.0,
                y: cy + 8.0,
                text: row.label().to_owned(),
                color: self.palette.text,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((max_w - value_w - 30.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: lx + max_w - 10.0 - value_w,
                y: cy + 8.0,
                text: value,
                color: if selected {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.text
                },
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cy += RULE_ROW_PITCH;
        }

        // What the rules say, section by section, while there is room for a
        // heading and a line under it.
        let fits = |cy: f32| cy + 26.0 + RULE_LINE_HEIGHT <= bottom;
        if !fits(cy) {
            return;
        }
        cy += 8.0;
        cmds.push(heading(
            &self.palette,
            "THE GENERATED PASSWORD",
            lx,
            cy,
            max_w,
        ));
        cy += 18.0;
        cy = if self.current_password.is_empty() {
            let none = ["Nothing generated yet".to_owned()];
            push_bounded_lines(
                cmds,
                &self.palette,
                &none,
                self.palette.subtext0,
                column,
                cy,
            )
        } else {
            self.push_verdict(cmds, &self.current_password, column, cy)
        };
        if !self.analyzer_input.is_empty() && fits(cy) {
            cy += 8.0;
            cmds.push(heading(
                &self.palette,
                "THE ONE IN THE ANALYSER",
                lx,
                cy,
                max_w,
            ));
            cy += 18.0;
            cy = self.push_verdict(cmds, &self.analyzer_input, column, cy);
        }
        if !self.settings_problems.is_empty() && fits(cy) {
            cy += 8.0;
            cmds.push(heading(&self.palette, "YOUR SETTINGS FILE", lx, cy, max_w));
            cy += 18.0;
            push_bounded_lines(
                cmds,
                &self.palette,
                &self.settings_problems,
                self.palette.ink(self.palette.peach),
                column,
                cy,
            );
        }
    }
}

// ============================================================================
// Main
// ============================================================================

impl App for PasswordApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn appearance_changed(&mut self, settings: &appearance::AppearanceSettings) {
        self.focus_ring_width = settings.focus_ring_width();
    }

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
        let mut commands = self.render_commands(width, height);
        // Over everything the app draws: it is modal, and a picker drawn under
        // the history list would be a picker the user could not read.
        if let Some(dialog) = &self.dialog {
            commands.extend(dialog.render(&self.palette, width, height));
        }
        RenderTree { commands }
    }
}

fn main() -> ExitCode {
    let mut app = PasswordApp::new().with_settings();
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

    /// Every string the window draws, joined. (The other `drawn` in this
    /// module takes `&mut` and a different size; this one is for the card.)
    fn card_text(app: &PasswordApp) -> String {
        app.render_commands(1100.0, 760.0)
            .iter()
            .filter_map(|c| match c {
                // The password box's text is the toolkit's field's, drawn as
                // rich text.
                RenderCommand::Text { text, .. } | RenderCommand::RichText { text, .. } => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// **Every key the list advertises is one this program answers.**
    ///
    /// Seeded rather than `new`, because the system CSPRNG is unavailable in a
    /// test process and the generators then *refuse* -- documented behaviour,
    /// and it would read here as five dead letters.
    ///
    /// `C` clears the history and is refused with an empty one, so the fixture
    /// generates first.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                // Every tab, because `set_tab` deliberately answers `Ignored`
                // for the tab you are already on -- so no single state can
                // answer all four digits -- and the arrows and Space mean the
                // rules' on the Rules tab. Its cursor starts on the second
                // rule, where Up and Down both have somewhere to go.
                let answered = ActiveTab::ALL.into_iter().any(|tab| {
                    let mut app = seeded_app();
                    app.handle_event(&press(Key::P));
                    app.active_tab = tab;
                    app.rule_cursor = 1;
                    app.handle_event(&Event::Key(stroke.clone())) == EventResult::Consumed
                });
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// **The shortcut list reaches the window, and nothing acts behind it.**
    ///
    /// The control is the half that matters: `P` behind the card must not
    /// replace the password on screen, and asserting only that it does not
    /// would pass on an app that had lost `P` altogether.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::P));
        assert!(
            !card_text(&app).contains("F1 closes this"),
            "the list is up before anybody asked for it"
        );

        app.handle_event(&press(Key::F1));
        let shown = card_text(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        let before = app.current_password.clone();
        assert!(
            !before.is_empty(),
            "the fixture must have generated something"
        );
        app.handle_event(&press(Key::P));
        assert_eq!(
            app.current_password, before,
            "P generated a new password through the shortcut card"
        );

        app.handle_event(&press(Key::Escape));
        assert!(
            !card_text(&app).contains("F1 closes this"),
            "Escape did not close it"
        );

        app.handle_event(&press(Key::P));
        assert_ne!(
            app.current_password, before,
            "control: P does nothing even with the card down"
        );
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        })
    }

    fn ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        })
    }

    /// Every option row names a keystroke the shortcut parser understands.
    ///
    /// `OptionRow::from_key` decides by parsing the very string the panel
    /// prints, and an unparseable one would simply match nothing -- a row
    /// drawn with a key that does not work, which is the defect this whole
    /// list exists to prevent. So the labels are checked directly.
    #[test]
    fn every_option_row_names_a_keystroke_that_parses() {
        for row in OptionRow::ALL {
            let parsed = guitk::shortcut::keystrokes(row.keys());
            assert!(
                parsed.is_ok(),
                "{} is drawn with keys {:?}, which the shortcut parser \
rejects: {:?}",
                row.label(),
                row.keys(),
                parsed.err()
            );
        }
    }

    /// Pressing a row's key changes that row, and the panel says so.
    #[test]
    fn every_option_row_answers_its_key_and_the_panel_shows_it() {
        for row in OptionRow::ALL {
            let mut app = seeded_app();
            let before = row.is_on(&app);
            let want_before = format!(
                "{} ({}): {}",
                row.label(),
                row.keys(),
                if before { "Yes" } else { "No" }
            );
            assert!(
                drawn(&mut app).contains(&want_before),
                "control: the panel does not draw {want_before:?}"
            );

            let presses = guitk::shortcut::keystrokes(row.keys())
                .unwrap_or_else(|e| panic!("{} has unparseable keys: {e:?}", row.label()));
            let first = presses.first().expect("no keystroke");
            let handled = app.handle_event(&Event::Key(KeyEvent {
                key: first.key,
                pressed: true,
                modifiers: first.modifiers,
                text: String::new(),
            }));
            assert_eq!(
                handled,
                EventResult::Consumed,
                "{} ignored its own key {}",
                row.label(),
                row.keys()
            );
            assert_ne!(
                row.is_on(&app),
                before,
                "{} did not change when {} was pressed",
                row.label(),
                row.keys()
            );

            let want_after = format!(
                "{} ({}): {}",
                row.label(),
                row.keys(),
                if row.is_on(&app) { "Yes" } else { "No" }
            );
            let after_texts = drawn(&mut app);
            assert!(
                after_texts.contains(&want_after),
                "{} changed and the panel still does not draw {want_after:?}",
                row.label()
            );
            assert!(
                !after_texts.contains(&want_before),
                "{} changed and the panel still draws the old {want_before:?}",
                row.label()
            );
        }
    }

    /// The passphrase options reach the passphrase.
    ///
    /// `capitalize` and `add_number` were `true` at construction with no
    /// writer in the crate, so every passphrase this program had ever
    /// produced began each word with a capital and ended in a digit. A test
    /// that only checked the flag would have passed against that.
    #[test]
    fn the_passphrase_options_reach_the_passphrase() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::W));
        let with_both = app.current_password.clone();
        assert!(
            with_both.chars().any(char::is_uppercase),
            "control: the default passphrase {with_both:?} is not capitalised"
        );
        assert!(
            with_both.chars().last().is_some_and(|c| c.is_ascii_digit()),
            "control: the default passphrase {with_both:?} does not end in a digit"
        );

        app.handle_event(&shift_press(Key::C));
        assert!(
            !app.current_password.chars().any(char::is_uppercase),
            "Shift+C left the passphrase {:?} capitalised",
            app.current_password
        );

        app.handle_event(&shift_press(Key::D));
        assert!(
            !app.current_password
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_digit()),
            "Shift+D left the passphrase {:?} ending in a digit",
            app.current_password
        );

        app.handle_event(&shift_press(Key::S));
        assert!(
            app.current_password
                .chars()
                .last()
                .is_some_and(|c| SYMBOLS.contains(c)),
            "Shift+S did not put a symbol on the end of {:?}",
            app.current_password
        );
    }

    /// `M` decides whether one of every selected kind is guaranteed.
    #[test]
    fn m_changes_whether_every_kind_is_guaranteed() {
        let mut app = seeded_app();
        let before = app.password_opts.must_include_each_class;
        app.handle_event(&press(Key::M));
        assert_ne!(
            app.password_opts.must_include_each_class, before,
            "M did not move the one-of-each-kind option"
        );
    }

    /// Plain `C` still clears the history: the shifted rows must not have
    /// swallowed the unshifted key.
    #[test]
    fn plain_c_still_clears_the_history() {
        let mut app = seeded_app();
        app.gen_password();
        assert!(!app.history.is_empty(), "the fixture generated nothing");
        app.handle_event(&press(Key::C));
        assert!(app.history.is_empty(), "C stopped clearing the history");
    }

    fn shift_press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::shift(),
            text: String::new(),
        })
    }

    /// Every string the app draws.
    fn drawn(app: &mut PasswordApp) -> Vec<String> {
        app.render(900.0, 700.0)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// **The generated passwords reach a file the user chose.**
    ///
    /// `export_history` rendered them as text from the day it was written and
    /// was called by nothing but its own test: the string had nowhere to go.
    /// This drives the whole path -- shortcut, picker, write -- and reads the
    /// file back off the disk rather than asking the app what it thinks it
    /// did.
    /// `S` drops symbols, and the next password has none.
    ///
    /// `length` was the only field of `password_opts` with a writer, so every
    /// password this program made contained symbols -- and a site that
    /// forbids them made the generator useless.
    #[test]
    fn s_turns_symbols_off() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::P));
        assert!(
            app.password_opts.use_symbols,
            "control: symbols are on by default"
        );

        app.handle_event(&press(Key::S));

        assert!(!app.password_opts.use_symbols, "S did not turn symbols off");
        assert!(
            !app.current_password.is_empty(),
            "the password went missing entirely"
        );
        assert!(
            app.current_password.chars().all(|c| c.is_alphanumeric()),
            "a symbol survived in {}",
            app.current_password
        );
    }

    /// Toggling produces a new password rather than leaving the old one up.
    ///
    /// The old password was made under the old settings, so leaving it on
    /// screen invites reading it as the new setting's output.
    #[test]
    fn toggling_a_class_produces_a_new_password() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::P));
        let before = app.current_password.clone();

        app.handle_event(&press(Key::S));

        assert_ne!(app.current_password, before, "the old password is still up");
    }

    /// The last class cannot be turned off.
    ///
    /// `generate_password` answers an empty pool with an empty string, so a
    /// generator with nothing selected would produce nothing and say nothing.
    #[test]
    fn the_last_character_class_cannot_be_turned_off() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::P));
        app.handle_event(&press(Key::U));
        app.handle_event(&press(Key::D));
        app.handle_event(&press(Key::S));
        assert_eq!(
            app.password_opts.active_classes(),
            1,
            "control: one class should be left"
        );

        app.handle_event(&press(Key::L));

        assert!(
            app.password_opts.use_lowercase,
            "the last class was turned off"
        );
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.contains("at least one")),
            "it refused without saying why: {:?}",
            app.status
        );
        assert!(
            !app.current_password.is_empty(),
            "the generator produced an empty password"
        );
    }

    /// `A` leaves out the characters that are easy to misread.
    #[test]
    fn a_excludes_ambiguous_characters() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::P));
        assert!(
            !app.password_opts.exclude_ambiguous,
            "control: they are included by default"
        );

        app.handle_event(&press(Key::A));

        assert!(
            app.password_opts.exclude_ambiguous,
            "A did not exclude the ambiguous characters"
        );
    }

    /// The options panel says which key changes each option.
    ///
    /// It listed them as "Lowercase: Yes" for a program in which no key could
    /// make it say "No".
    #[test]
    fn the_options_panel_names_its_keys() {
        let app = seeded_app();
        let text: Vec<String> = app
            .render_commands(1100.0, 700.0)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        for hint in ["Lowercase (L)", "Symbols (S)", "Exclude Ambiguous (A)"] {
            assert!(
                text.iter().any(|t| t.contains(hint)),
                "the panel never says {hint}"
            );
        }
    }

    #[test]
    fn ctrl_e_exports_the_history_to_the_chosen_file() {
        let dir = scratchdir::ScratchDir::new("passwordgen_export");
        let mut app = seeded_app();
        app.gen_password();
        app.gen_password();
        assert_eq!(app.history.len(), 2, "the fixture generated nothing");

        app.handle_event(&ctrl(Key::E));
        let dialog = app.dialog.as_mut().expect("Ctrl+E put up no picker");
        dialog.navigate_to(dir.dir());
        dialog.set_entries(guitk::dialog::list_directory(dir.dir()));
        dialog.set_filename("out.txt");
        app.handle_event(&press(Key::Enter));

        assert!(app.dialog.is_none(), "the picker stayed up");
        let written = std::fs::read_to_string(dir.path("out.txt"))
            .unwrap_or_else(|e| panic!("nothing was written: {e}; status {:?}", app.status));
        for entry in &app.history {
            assert!(
                written.contains(&entry.password),
                "a generated password is missing from the export"
            );
        }
    }

    /// The status bar says the export happened, because the window cannot
    /// otherwise show that a file appeared.
    #[test]
    fn the_status_bar_reports_the_export() {
        let dir = scratchdir::ScratchDir::new("passwordgen_export_status");
        let mut app = seeded_app();
        app.gen_password();

        app.handle_event(&ctrl(Key::E));
        let dialog = app.dialog.as_mut().expect("no picker");
        dialog.navigate_to(dir.dir());
        dialog.set_entries(guitk::dialog::list_directory(dir.dir()));
        dialog.set_filename("out.txt");
        app.handle_event(&press(Key::Enter));

        let texts = drawn(&mut app);
        assert!(
            texts.iter().any(|t| t.starts_with("Exported")),
            "the export is invisible: {texts:?}"
        );
    }

    /// Exporting nothing is refused, and says why.
    ///
    /// A picker for an empty export is a question with one useless answer, and
    /// an empty file of passwords is worse than no file.
    #[test]
    fn exporting_an_empty_history_is_refused_with_a_reason() {
        let mut app = seeded_app();
        app.clear_history();

        app.handle_event(&ctrl(Key::E));

        assert!(app.dialog.is_none(), "a picker came up for an empty export");
        assert!(
            app.status
                .as_deref()
                .unwrap_or_default()
                .contains("Nothing generated"),
            "status was {:?}",
            app.status
        );
    }

    /// While the picker is up, a keystroke does not generate a password.
    #[test]
    fn the_generator_does_not_run_while_the_picker_is_up() {
        let mut app = seeded_app();
        app.gen_password();
        let before = app.history.len();

        app.handle_event(&ctrl(Key::E));
        app.handle_event(&typed('p'));
        app.handle_event(&press(Key::Space));

        assert_eq!(
            app.history.len(),
            before,
            "a keystroke meant for the filename generated a password"
        );
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
    fn the_tab_key_still_leaves_the_analyser() {
        // Otherwise there is no way out of the text box: the digit keys are
        // part of the password in there.
        let mut app = seeded_app();
        app.handle_event(&press(Key::Num2));
        assert_eq!(app.handle_event(&press(Key::Tab)), EventResult::Consumed);
        assert_eq!(app.active_tab, ActiveTab::History);
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

    fn held(k: Key, modifiers: Modifiers, text: &str) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: text.to_owned(),
        })
    }

    /// Ctrl+Alt, as Windows and a remote client on it report AltGr.
    const ALTGR: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: true,
        super_key: false,
    };

    /// **AltGr types into the analyser and runs no chord.** AltGr arrives
    /// as Ctrl+Alt: `€` is AltGr+E on most European keyboards, and opened
    /// the export dialog; `®` -- AltGr+R on US International -- showed the
    /// password rather than going into it. A command carries its letter as
    /// text on a real machine, and types none of it.
    #[test]
    fn altgr_types_into_the_analyser_and_runs_no_chord() {
        let mut app = seeded_app();
        // Something to export, or Ctrl+E would only say there is nothing.
        app.handle_event(&press(Key::P));
        app.handle_event(&press(Key::Num2));
        assert_eq!(app.active_tab, ActiveTab::Analyzer);
        let revealed = app.analyzer_revealed;
        for (k, text) in [(Key::E, "\u{20ac}"), (Key::R, "\u{ae}")] {
            assert_eq!(
                app.handle_event(&held(k, ALTGR, text)),
                EventResult::Consumed,
                "AltGr+{k:?} was not typed"
            );
        }
        assert_eq!(app.analyzer_input, "\u{20ac}\u{ae}");
        assert!(app.dialog.is_none(), "AltGr+E opened the export dialog");
        assert_eq!(app.analyzer_revealed, revealed, "AltGr+R revealed it");
        for (k, modifiers, text) in [
            (Key::K, Modifiers::ctrl(), "k"),
            (Key::F, Modifiers::alt(), "f"),
            (Key::D, Modifiers::super_key(), "d"),
        ] {
            assert_eq!(
                app.handle_event(&held(k, modifiers, text)),
                EventResult::Ignored,
                "{modifiers:?}+{k:?} was typed"
            );
        }
        assert_eq!(app.analyzer_input, "\u{20ac}\u{ae}");
    }

    /// **A key held with Ctrl, Alt or the Windows key is no bare key.**
    /// Ctrl+C -- the key a user presses to copy a password -- cleared the
    /// history as C does, and Ctrl+P made a new password over the one on
    /// screen; AltGr, Alt and the Windows key did the same.
    #[test]
    fn a_key_held_with_ctrl_alt_or_the_windows_key_is_no_bare_key() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::P));
        let history = app.history.len();
        let password = app.current_password.clone();
        assert!(history > 0, "the test needs a history to clear");
        for modifiers in [
            Modifiers::ctrl(),
            ALTGR,
            Modifiers::alt(),
            Modifiers::super_key(),
        ] {
            for k in [Key::C, Key::P] {
                assert_eq!(
                    app.handle_event(&held(k, modifiers, "")),
                    EventResult::Ignored,
                    "{modifiers:?}+{k:?}"
                );
            }
            assert_eq!(app.history.len(), history, "{modifiers:?}+C cleared it");
            assert_eq!(app.current_password, password, "{modifiers:?}+P made one");
        }
        // Bare, C still clears.
        assert_eq!(app.handle_event(&press(Key::C)), EventResult::Consumed);
        assert!(app.history.is_empty());
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
        for tab in ActiveTab::ALL {
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
        let widths: Vec<f32> = ActiveTab::ALL
            .iter()
            .map(|t| text::padded_width_any_weight(t.label(), 10.0, 11.0))
            .collect();
        for (i, tab) in ActiveTab::ALL.iter().enumerate() {
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut app = PasswordApp::with_random(AppRandom::Unavailable);
        app.gen_password();
        let shown = app.render_commands(1100.0, 700.0).into_iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { ref text, color, .. }
                if text == NO_ENTROPY_MESSAGE && color == pal.red)
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

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        use oswindow::app::App as _;
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

        fn fills(app: &mut PasswordApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = PasswordApp::new();

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

    // == The rules are the user's, and each tab judges what it shows =========
    //
    // 2026-09-27, C-Q26: the rules were compiled in; the analyser never drew
    // what was typed into it; the digit keys could not be typed there; and
    // the strength and the verdict on show could belong to a different
    // password from the one beside them.

    /// A key as the compositor sends it: the digit keys carry their digit.
    fn key_with_text(k: Key, text: &str) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: text.to_owned(),
        })
    }

    /// A seeded app with a password generated, on the Rules tab.
    fn on_rules_tab() -> PasswordApp {
        let mut app = seeded_app();
        app.handle_event(&press(Key::P));
        assert_eq!(app.handle_event(&press(Key::Num4)), EventResult::Consumed);
        assert_eq!(app.active_tab, ActiveTab::Rules);
        app
    }

    #[test]
    fn the_arrows_change_the_rule_under_the_cursor() {
        let mut app = on_rules_tab();
        let password = app.current_password.clone();
        // The cursor starts on the shortest length.
        assert_eq!(app.handle_event(&press(Key::Right)), EventResult::Consumed);
        assert_eq!(app.policy.min_length, 9);
        assert_eq!(app.handle_event(&press(Key::Left)), EventResult::Consumed);
        assert_eq!(app.policy.min_length, 8);
        // Down to "must have a symbol", and Space turns it on.
        for _ in 0..5 {
            app.handle_event(&press(Key::Down));
        }
        assert_eq!(RuleRow::ALL[app.rule_cursor], RuleRow::Symbol);
        assert!(!app.policy.require_symbol);
        assert_eq!(app.handle_event(&press(Key::Space)), EventResult::Consumed);
        assert!(app.policy.require_symbol);
        // Space names no direction, so it does nothing to a number.
        app.handle_event(&press(Key::Down));
        assert_eq!(RuleRow::ALL[app.rule_cursor], RuleRow::Kinds);
        assert_eq!(app.handle_event(&press(Key::Space)), EventResult::Ignored);
        assert_eq!(app.handle_event(&press(Key::Right)), EventResult::Consumed);
        assert_eq!(app.policy.min_classes, 4);
        assert_eq!(
            app.handle_event(&press(Key::Right)),
            EventResult::Ignored,
            "there are four kinds of character, not five"
        );
        // Strength moves in fives.
        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Right));
        assert_eq!(app.policy.min_bits, 45);
        // None of it generated anything or left the tab.
        assert_eq!(app.active_tab, ActiveTab::Rules);
        assert_eq!(app.current_password, password);
    }

    #[test]
    fn the_cursor_stays_on_the_list() {
        let mut app = on_rules_tab();
        assert_eq!(app.handle_event(&press(Key::Up)), EventResult::Ignored);
        for _ in 0..20 {
            app.handle_event(&press(Key::Down));
        }
        assert_eq!(app.rule_cursor, RuleRow::ALL.len() - 1);
        assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Ignored);
    }

    #[test]
    fn no_limit_sits_above_the_longest_length_and_the_lengths_hold_each_other() {
        let mut app = on_rules_tab();
        app.handle_event(&press(Key::Down));
        assert_eq!(RuleRow::ALL[app.rule_cursor], RuleRow::Longest);
        assert_eq!(app.policy.max_length, None);
        assert_eq!(
            app.handle_event(&press(Key::Right)),
            EventResult::Ignored,
            "there is nothing above no limit"
        );
        app.handle_event(&press(Key::Left));
        assert_eq!(app.policy.max_length, Some(RULE_LENGTH_MAX));
        app.handle_event(&press(Key::Right));
        assert_eq!(app.policy.max_length, None);
        // The longest cannot go below the shortest...
        app.policy.max_length = Some(9);
        assert_eq!(app.handle_event(&press(Key::Left)), EventResult::Consumed);
        assert_eq!(app.policy.max_length, Some(8));
        assert_eq!(app.handle_event(&press(Key::Left)), EventResult::Ignored);
        // ...nor the shortest above the longest.
        app.handle_event(&press(Key::Up));
        assert_eq!(app.handle_event(&press(Key::Right)), EventResult::Ignored);
        assert_eq!(app.policy.min_length, 8);
    }

    #[test]
    fn the_rules_tab_draws_every_rule_and_what_they_say() {
        let mut app = on_rules_tab();
        app.current_password = "password".to_owned();
        let drawn = card_text(&app);
        for row in RuleRow::ALL {
            assert!(drawn.contains(row.label()), "{row:?} is not drawn: {drawn}");
            assert!(
                drawn.contains(&row.value(&app.policy)),
                "{row:?}'s setting is not drawn: {drawn}"
            );
        }
        assert!(
            drawn.contains(RULES_HINT),
            "the tab does not say its keys: {drawn}"
        );
        assert!(
            drawn.contains("Is one of the most commonly used passwords"),
            "{drawn}"
        );
        app.current_password = "Str0ng!Password".to_owned();
        assert!(card_text(&app).contains("Meets every rule"));
    }

    #[test]
    fn a_rule_changed_is_kept_and_the_next_window_starts_with_it() {
        settingsfile::testing::with_scratch_config("passwordgen-rules", |dir| {
            let mut app = seeded_app().with_settings();
            app.handle_event(&press(Key::P));
            app.handle_event(&press(Key::Num4));
            app.handle_event(&press(Key::Right));
            let next = seeded_app().with_settings();
            assert_eq!(
                next.policy.min_length, 9,
                "the rule did not outlive the window"
            );
            assert_eq!(next.policy, app.policy);
            let text = std::fs::read_to_string(dir.join("slateos").join("passwordgen.yaml"))
                .unwrap_or_default();
            assert!(text.contains("shortest: 9"), "{text:?}");
        });
    }

    /// **A rule changed in one window reaches the others**, when the desktop
    /// says the file changed (§1434) -- while the export picker is up too,
    /// which takes every other event. Another program's announcement is not
    /// this one's; a window's own save announced back changes nothing; a
    /// hand edit that cannot be used is said; a window that keeps no
    /// settings reads none.
    #[test]
    fn a_rule_changed_in_one_window_reaches_the_others() {
        settingsfile::testing::with_scratch_config("passwordgen-reread", |dir| {
            let announce = |name: &[u8]| Event::SettingsChanged {
                group: guitk::event::SettingsGroup::Program(
                    guitk::event::SettingsName::new(name).expect("a settings name"),
                ),
            };
            let file = dir.join("slateos").join("passwordgen.yaml");
            let mut first = seeded_app().with_settings();
            let mut second = seeded_app().with_settings();
            first.handle_event(&press(Key::Num4));
            first.handle_event(&press(Key::Right));
            assert_eq!(first.policy.min_length, 9);
            assert_eq!(
                second.policy.min_length, 8,
                "the second window changed untold"
            );

            assert_eq!(
                second.handle_event(&announce(b"notes")),
                EventResult::Ignored
            );
            assert_eq!(
                second.policy.min_length, 8,
                "another program's file was read"
            );
            assert_eq!(
                second.handle_event(&announce(b"passwordgen")),
                EventResult::Consumed
            );
            assert_eq!(second.policy.min_length, 9, "the rule did not reach it");
            assert_eq!(
                first.handle_event(&announce(b"passwordgen")),
                EventResult::Ignored,
                "a window's own save, announced back, changed what it shows"
            );

            std::fs::write(&file, "rules:\n  shortest: banana\n").expect("a hand edit");
            second.handle_event(&announce(b"passwordgen"));
            assert!(
                !second.settings_problems.is_empty(),
                "a value that cannot be used was not said"
            );

            // Under the export picker, which takes every other event.
            second.handle_event(&press(Key::P));
            second.handle_event(&ctrl(Key::E));
            assert!(second.dialog.is_some(), "the picker did not come up");
            std::fs::write(&file, "rules:\n  shortest: 12\n").expect("a hand edit");
            second.handle_event(&announce(b"passwordgen"));
            assert_eq!(
                second.policy.min_length, 12,
                "the picker swallowed the news"
            );

            let mut quiet = seeded_app();
            assert_eq!(
                quiet.handle_event(&announce(b"passwordgen")),
                EventResult::Ignored
            );
            assert_eq!(
                quiet.policy.min_length, 8,
                "a window that keeps no rules read them"
            );
        });
    }

    #[test]
    fn a_window_a_test_builds_keeps_no_rules() {
        settingsfile::testing::with_scratch_config("passwordgen-quiet", |dir| {
            let mut app = on_rules_tab();
            app.handle_event(&press(Key::Right));
            assert_eq!(app.policy.min_length, 9);
            assert!(
                !dir.join("slateos").join("passwordgen.yaml").exists(),
                "a window that keeps nothing wrote the rules"
            );
        });
    }

    #[test]
    fn a_kept_rule_that_cannot_be_used_is_said_and_the_default_used() {
        let doc = yamldoc::Document::parse(
            "rules:\n  shortest: 500\n  digit: maybe\n  kinds: 2\n  longest: 4\n",
        );
        let (rules, problems) = PasswordPolicy::from_settings(&doc);
        assert_eq!(rules.min_length, 8, "an impossible length was used");
        assert!(rules.require_digit, "an unreadable switch was used");
        assert_eq!(
            rules.min_classes, 2,
            "a good value beside bad ones was dropped"
        );
        assert_eq!(
            rules.max_length, None,
            "a longest below the shortest was used"
        );
        assert_eq!(problems.len(), 3, "{problems:?}");
        for key in ["rules.shortest", "rules.digit", "rules.longest"] {
            assert!(
                problems.iter().any(|p| p.contains(key)),
                "{key}: {problems:?}"
            );
        }
    }

    #[test]
    fn the_rules_read_back_as_they_were_written() {
        let rules = PasswordPolicy {
            min_length: 12,
            max_length: Some(64),
            require_lowercase: false,
            require_uppercase: true,
            require_digit: false,
            require_symbol: true,
            min_classes: 2,
            min_bits: 60,
            disallow_common: false,
        };
        let mut doc = yamldoc::Document::parse("# my rules\nother: kept\n");
        rules.store_into(&mut doc);
        let (back, problems) =
            PasswordPolicy::from_settings(&yamldoc::Document::parse(&doc.to_text()));
        assert_eq!(back, rules);
        assert!(problems.is_empty(), "{problems:?}");
        let text = doc.to_text();
        assert!(
            text.contains("# my rules") && text.contains("other: kept"),
            "{text}"
        );
        // No limit is no key.
        let unlimited = PasswordPolicy {
            max_length: None,
            ..rules
        };
        unlimited.store_into(&mut doc);
        assert!(!doc.contains(&["rules", "longest"]), "{}", doc.to_text());
        assert_eq!(PasswordPolicy::from_settings(&doc).0.max_length, None);
    }

    #[test]
    fn a_settings_problem_is_said_when_the_window_opens_and_cleared_by_a_change() {
        settingsfile::testing::with_scratch_config("passwordgen-problem", |dir| {
            let folder = dir.join("slateos");
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("passwordgen.yaml"), "rules:\n  kinds: 9\n").unwrap();
            let mut app = seeded_app().with_settings();
            assert!(
                app.status
                    .as_deref()
                    .is_some_and(|s| s.contains("rules.kinds")),
                "{:?}",
                app.status
            );
            app.handle_event(&press(Key::Num4));
            assert!(card_text(&app).contains("YOUR SETTINGS FILE"));
            app.handle_event(&press(Key::Right));
            assert!(
                app.settings_problems.is_empty(),
                "{:?}",
                app.settings_problems
            );
            let text = std::fs::read_to_string(folder.join("passwordgen.yaml")).unwrap();
            assert!(text.contains("kinds: 3"), "{text}");
        });
    }

    #[test]
    fn digits_are_part_of_the_password_in_the_analyser() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::Num2));
        for (k, t) in [
            (Key::Num1, "1"),
            (Key::Num2, "2"),
            (Key::Num3, "3"),
            (Key::Num4, "4"),
        ] {
            assert_eq!(
                app.handle_event(&key_with_text(k, t)),
                EventResult::Consumed
            );
        }
        assert_eq!(app.analyzer_input, "1234");
        assert_eq!(
            app.active_tab,
            ActiveTab::Analyzer,
            "a digit left the analyser"
        );
    }

    #[test]
    fn a_key_that_carries_a_control_character_is_not_typed() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::Num2));
        assert_eq!(
            app.handle_event(&key_with_text(Key::Enter, "\r")),
            EventResult::Ignored
        );
        assert_eq!(app.analyzer_input, "");
    }

    #[test]
    fn the_analyser_shows_what_is_typed_as_dots_until_asked() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::Num2));
        for c in "hunter2".chars() {
            app.handle_event(&typed(c));
        }
        let drawn = card_text(&app);
        assert!(!drawn.contains("hunter2"), "drawn in the clear: {drawn}");
        assert!(drawn.contains(&MASK.to_string().repeat(7)), "{drawn}");
        assert!(drawn.contains("7 characters"), "{drawn}");
        assert_eq!(app.handle_event(&ctrl(Key::R)), EventResult::Consumed);
        assert!(
            card_text(&app).contains("hunter2"),
            "Ctrl+R did not show it"
        );
        assert_eq!(app.analyzer_input, "hunter2", "Ctrl+R was typed");
    }

    /// **The password box is the toolkit's field**, with the keyboard in the
    /// theme's mark at the user's width while the Analyzer tab is up -- every
    /// printable key there is the password -- and not under the list of
    /// keys; the password drawn with a caret after it. It was a card holding
    /// the text, with no caret at all.
    #[test]
    fn the_password_box_is_the_toolkits_field() {
        let mut app = seeded_app();
        let mut p = app.palette;
        p.widget_style.field.focus = guitk::widget_style::FocusMark::Ring;
        app.theme_changed(&p);
        app.appearance_changed(&appearance::AppearanceSettings {
            focus_ring_scale: 2.5,
            ..appearance::AppearanceSettings::default()
        });
        assert!(
            app.focus_ring_width > guitk::style::FOCUS_RING_WIDTH,
            "the user's focus width did not arrive"
        );
        app.handle_event(&press(Key::Num2));
        for c in "hunter2".chars() {
            app.handle_event(&typed(c));
        }
        let (w, h) = (1100.0, 760.0);
        let cmds = app.render_commands(w, h);
        // The box is where the field is drawn: the first field the window
        // draws, found by its well -- then checked to be the toolkit's.
        let has = |cmds: &[RenderCommand], want: &[RenderCommand]| {
            cmds.windows(want.len()).any(|win| win == want)
        };
        let shown = MASK.to_string().repeat(7);
        let at = cmds
            .iter()
            .position(|c| matches!(c, RenderCommand::RichText { text, .. } if *text == shown))
            .expect("the password is not drawn in a field");
        let (tx, ty) = match cmds.get(at) {
            Some(RenderCommand::RichText { x, y, .. }) => (*x, *y),
            _ => unreachable!("just matched"),
        };
        // The box the text sits in, as `render_analyzer_input` places it.
        let line = text::line_height(PASSWORD_TEXT_SIZE, FontWeightHint::Bold);
        let rect = guitk::frame::Rect::new(
            tx - 8.0,
            ty - (32.0 - line) / 2.0,
            LEFT_PANEL_WIDTH - 24.0,
            32.0,
        );
        let seq = |s: field::State| {
            let mut v: Vec<RenderCommand> = Vec::new();
            field::draw(&mut v, &p, rect, s, app.focus_ring_width);
            v
        };
        let focused = field::State {
            focused: true,
            ..field::State::default()
        };
        assert!(has(&cmds, &seq(focused)), "the box has no keyboard mark");
        let caret = match cmds.get(at + 1) {
            Some(RenderCommand::Line { x1, x2, .. }) if (x1 - x2).abs() < 0.01 => *x1,
            other => panic!("no caret after the password: {other:?}"),
        };
        let end = tx + text::measure(&shown, PASSWORD_TEXT_SIZE, FontWeightHint::Bold);
        assert!(
            (caret - end).abs() < 0.5,
            "the caret is at {caret}, not after the password at {end}"
        );

        app.show_help = true;
        let cmds = app.render_commands(w, h);
        assert!(
            has(&cmds, &seq(field::State::default())) && !has(&cmds, &seq(focused)),
            "the box keeps its mark under the list of keys"
        );
    }

    #[test]
    fn each_tab_measures_the_password_it_shows() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::P));
        let generated = app.current_password.chars().count();
        app.handle_event(&press(Key::Num2));
        assert!(
            app.current_analysis.is_none(),
            "the analyser showed the generated password's strength before anything was typed"
        );
        app.handle_event(&typed('a'));
        assert_eq!(app.current_analysis.as_ref().map(|a| a.length), Some(1));
        app.handle_event(&press(Key::Tab));
        app.handle_event(&press(Key::Num1));
        assert_eq!(
            app.current_analysis.as_ref().map(|a| a.length),
            Some(generated),
            "the generator tab measured what was typed in the analyser"
        );
        app.handle_event(&press(Key::Num2));
        assert_eq!(app.current_analysis.as_ref().map(|a| a.length), Some(1));
    }

    #[test]
    fn a_password_is_as_long_as_its_characters_not_its_bytes() {
        assert_eq!(analyze_password("pässwörd").length, 8);
        let rules = PasswordPolicy {
            min_length: 9,
            ..PasswordPolicy::default()
        };
        assert!(
            rules.check("pässwörd").iter().any(|v| v.contains("short")),
            "ten bytes were counted as ten characters"
        );
    }

    #[test]
    fn the_status_bar_judges_the_password_on_show() {
        let mut app = seeded_app();
        app.current_password = "Str0ng!Password".to_owned();
        assert!(
            card_text(&app).contains("Meets your rules"),
            "{}",
            card_text(&app)
        );
        app.handle_event(&press(Key::Num2));
        // An empty field is judged by nobody -- and the empty analysis says
        // what to do on this tab, not on the generator's.
        let empty = card_text(&app);
        assert!(
            !empty.contains("Breaks") && !empty.contains("Meets your rules"),
            "{empty}"
        );
        assert!(empty.contains("Type a password on the left"), "{empty}");
        for c in "password".chars() {
            app.handle_event(&typed(c));
        }
        assert!(card_text(&app).contains("Breaks"), "{}", card_text(&app));
    }

    /// Every rule row and every line of what the rules say stays above the
    /// status bar, at any height -- and when the rows do not all fit, the one
    /// under the cursor is among those drawn.
    #[test]
    fn the_rules_tab_stays_inside_its_panel_and_keeps_the_cursor_in_view() {
        for (w, h) in [(1100.0, 700.0), (640.0, 480.0), (800.0, 300.0)] {
            let mut app = on_rules_tab();
            app.current_password = "aaa".to_owned();
            app.analyzer_input = "b".to_owned();
            app.settings_problems = vec!["one".to_owned(), "two".to_owned(), "three".to_owned()];
            app.rule_cursor = RuleRow::ALL.len() - 1;
            let cmds = app.render_commands(w, h);
            let bottom = h - STATUS_BAR_HEIGHT;
            for c in &cmds {
                if let RenderCommand::Text { x, y, text, .. } = c
                    && *x >= LEFT_PANEL_WIDTH
                    && *y >= TOOLBAR_HEIGHT
                {
                    assert!(
                        *y + RULE_LINE_HEIGHT <= bottom + 0.5,
                        "{text:?} at y={y} runs past {bottom} in a {w}x{h} window"
                    );
                }
            }
            let last = RuleRow::ALL[RuleRow::ALL.len() - 1].label();
            assert!(
                text_rows(&cmds).iter().any(|(_, t)| t == last),
                "the rule under the cursor is not drawn at {w}x{h}"
            );
        }
    }

    #[test]
    fn deleting_every_character_takes_the_strength_away_too() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::Num2));
        app.handle_event(&typed('a'));
        assert!(app.current_analysis.is_some());
        app.handle_event(&press(Key::Backspace));
        assert!(
            app.current_analysis.is_none(),
            "an empty field was given a strength"
        );
    }

    #[test]
    fn tab_visits_every_tab_in_the_toolbar_order() {
        let mut app = seeded_app();
        for want in ActiveTab::ALL
            .iter()
            .cycle()
            .skip(1)
            .take(ActiveTab::ALL.len())
        {
            assert_eq!(app.handle_event(&press(Key::Tab)), EventResult::Consumed);
            assert_eq!(app.active_tab, *want);
        }
    }

    // -- The analyser's box edits at a caret -----------------------------------------

    fn type_into(app: &mut PasswordApp, text: &str) {
        for c in text.chars() {
            app.handle_event(&typed(c));
        }
    }

    /// Where the analyser's text is drawn -- its run's x and its spans --
    /// and the x of the caret drawn on its line.
    fn drawn_box(app: &PasswordApp, shown: &str) -> (f32, Vec<guitk::render::TextSpan>, Vec<f32>) {
        let cmds = app.render_commands(1100.0, 760.0);
        let (x, y, spans) = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::RichText {
                    text, x, y, spans, ..
                } if text == shown => Some((*x, *y, spans.clone())),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{shown:?} is not drawn"));
        let carets = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Line {
                    x1, x2, y1, width, ..
                } if (x1 - x2).abs() < f32::EPSILON
                    && (width - textedit::CARET_WIDTH).abs() < f32::EPSILON
                    && (y1 - y).abs() < 1.0 =>
                {
                    Some(*x1)
                }
                _ => None,
            })
            .collect();
        (x, spans, carets)
    }

    /// **The password being measured edits at a caret**, hidden or shown:
    /// the arrows, Home and End move it, typing goes where it is, Delete
    /// deletes at it, Ctrl+A selects, and it and the selection are drawn on
    /// the dots where they stand on the characters. Hidden, nothing is
    /// copied or cut from it; shown, Ctrl+C and Ctrl+X take it. The box
    /// took typing at its end and Backspace from it, and nothing else.
    #[test]
    fn the_password_being_measured_edits_at_a_caret() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::Num2));
        type_into(&mut app, "pasword");
        for _ in 0..4 {
            app.handle_event(&press(Key::Left));
        }
        type_into(&mut app, "s");
        assert_eq!(app.analyzer_input, "password", "the caret did not move");
        assert!(app.current_analysis.is_some(), "the edit was not measured");
        let dots = MASK.to_string().repeat(8);
        let (x, _, carets) = drawn_box(&app, &dots);
        let at = x + text::caret_x(
            &dots,
            text::TextCursor::from(4 * MASK.len_utf8()),
            PASSWORD_TEXT_SIZE,
            FontWeightHint::Bold,
        );
        assert_eq!(carets.len(), 1, "one caret in the box");
        assert!(
            carets.iter().all(|c| (c - at).abs() < 0.5),
            "the caret is drawn at {carets:?}, not after the fourth dot at {at}"
        );

        app.handle_event(&press(Key::Home));
        app.handle_event(&press(Key::Delete));
        assert_eq!(app.analyzer_input, "assword", "Delete at the caret");
        type_into(&mut app, "p");
        app.handle_event(&ctrl(Key::A));
        let (_, spans, _) = drawn_box(&app, &dots);
        assert!(!spans.is_empty(), "the selection is not drawn on the dots");

        // Hidden, nothing leaves it for the clipboard.
        app.handle_event(&ctrl(Key::C));
        app.handle_event(&ctrl(Key::X));
        assert_eq!(
            app.analyzer_input, "password",
            "Ctrl+X cut a hidden password"
        );
        assert!(
            app.clipboard.is_empty(),
            "a hidden password reached the clipboard"
        );

        // Shown, it copies as a box does.
        app.handle_event(&ctrl(Key::R));
        app.handle_event(&ctrl(Key::A));
        app.handle_event(&ctrl(Key::X));
        assert_eq!(
            app.analyzer_input, "",
            "Ctrl+X did not cut a shown password"
        );
        app.handle_event(&ctrl(Key::V));
        assert_eq!(app.analyzer_input, "password", "Ctrl+V");
    }

    /// **The dots stand for characters, and the caret steps one at a
    /// time**: an `é` is one dot, and Left steps back over it in one press.
    #[test]
    fn a_dot_is_a_character_and_the_caret_steps_over_it() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::Num2));
        type_into(&mut app, "a\u{e9}b");
        app.handle_event(&press(Key::Left));
        app.handle_event(&press(Key::Left));
        type_into(&mut app, "!");
        assert_eq!(
            app.analyzer_input, "a!\u{e9}b",
            "Left did not step a character"
        );
        let dots = MASK.to_string().repeat(4);
        let (x, _, carets) = drawn_box(&app, &dots);
        let at = x + text::caret_x(
            &dots,
            text::TextCursor::from(2 * MASK.len_utf8()),
            PASSWORD_TEXT_SIZE,
            FontWeightHint::Bold,
        );
        assert!(
            carets.iter().all(|c| (c - at).abs() < 0.5) && !carets.is_empty(),
            "the caret is drawn at {carets:?}, not after the second dot at {at}"
        );
        assert_eq!(
            app.handle_event(&held(Key::Left, Modifiers::alt(), "")),
            EventResult::Ignored,
            "Alt+Left moved the caret"
        );
        app.handle_event(&press(Key::Home));
        assert_eq!(
            app.handle_event(&press(Key::Left)),
            EventResult::Ignored,
            "Left at the start is a redraw"
        );
    }

    /// **The box edits what it shows**, however it came to be what it is.
    #[test]
    fn the_analyser_edits_what_it_shows() {
        let mut app = seeded_app();
        app.handle_event(&press(Key::Num2));
        type_into(&mut app, "ab");
        app.handle_event(&press(Key::Home));
        app.set_analyzer_input("hunter");
        type_into(&mut app, "2");
        assert_eq!(
            app.analyzer_input, "hunter2",
            "the key edited the password the editor held"
        );
    }
}

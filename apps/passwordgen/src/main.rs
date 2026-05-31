//! `OurOS` Password Generator & Strength Analyzer
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
// Password generator (deterministic PRNG)
// ============================================================================

/// Simple xorshift64 PRNG for deterministic password generation.
#[derive(Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x12345678ABCDEF01 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Generate a random index in [0, bound).
    pub fn next_usize(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as usize
    }

    /// Pick a random element from a slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        items.get(self.next_usize(items.len()))
    }

    /// Pick a random char from a char slice.
    pub fn pick_char(&mut self, chars: &[char]) -> char {
        chars
            .get(self.next_usize(chars.len()))
            .copied()
            .unwrap_or('?')
    }
}

/// Generate a password using the given options and PRNG.
pub fn generate_password(opts: &PasswordOptions, rng: &mut Rng) -> String {
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
                password.push(rng.pick_char(&filtered));
            }
        }
    }

    // Fill remaining with random characters from the full pool
    while password.len() < opts.length {
        password.push(rng.pick_char(&pool));
    }

    // Shuffle the password (Fisher-Yates)
    let len = password.len();
    for i in (1..len).rev() {
        let j = rng.next_usize(i.saturating_add(1));
        password.swap(i, j);
    }

    password.into_iter().collect()
}

/// Generate a passphrase.
pub fn generate_passphrase(opts: &PassphraseOptions, rng: &mut Rng) -> String {
    let mut words: Vec<String> = Vec::with_capacity(opts.word_count);

    for _ in 0..opts.word_count {
        let word = rng.pick(WORD_LIST).copied().unwrap_or("unknown").to_owned();
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
        let digit = rng.next_usize(10);
        result.push_str(&digit.to_string());
    }
    if opts.add_symbol {
        let sym_chars: Vec<char> = SYMBOLS.chars().collect();
        result.push(rng.pick_char(&sym_chars));
    }

    result
}

/// Generate a PIN.
pub fn generate_pin(length: usize, rng: &mut Rng) -> String {
    let digits: Vec<char> = DIGITS.chars().collect();
    (0..length).map(|_| rng.pick_char(&digits)).collect()
}

/// Generate a pronounceable password (alternating consonant-vowel).
pub fn generate_pronounceable(length: usize, rng: &mut Rng) -> String {
    let consonants: Vec<char> = CONSONANTS.chars().collect();
    let vowels: Vec<char> = VOWELS.chars().collect();
    let mut result = String::with_capacity(length);
    for i in 0..length {
        if i % 2 == 0 {
            result.push(rng.pick_char(&consonants));
        } else {
            result.push(rng.pick_char(&vowels));
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
        if curr as u32 == prev as u32 + 1 {
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
pub struct PasswordApp {
    pub password_opts: PasswordOptions,
    pub passphrase_opts: PassphraseOptions,
    pub current_password: String,
    pub current_analysis: Option<PasswordAnalysis>,
    pub analyzer_input: String,
    pub history: Vec<HistoryEntry>,
    pub policy: PasswordPolicy,
    pub active_tab: ActiveTab,
    pub pin_length: usize,
    pub bulk_count: usize,
    pub bulk_results: Vec<String>,
    pub window_width: f32,
    pub window_height: f32,
    rng: Rng,
    timestamp: u64,
}

impl PasswordApp {
    pub fn new(seed: u64) -> Self {
        Self {
            password_opts: PasswordOptions::default(),
            passphrase_opts: PassphraseOptions::default(),
            current_password: String::new(),
            current_analysis: None,
            analyzer_input: String::new(),
            history: Vec::new(),
            policy: PasswordPolicy::default(),
            active_tab: ActiveTab::Generator,
            pin_length: 6,
            bulk_count: 10,
            bulk_results: Vec::new(),
            window_width: 1100.0,
            window_height: 700.0,
            rng: Rng::new(seed),
            timestamp: 1000,
        }
    }

    fn tick(&mut self) -> u64 {
        self.timestamp = self.timestamp.saturating_add(1);
        self.timestamp
    }

    /// Generate a new password.
    pub fn gen_password(&mut self) {
        let pw = generate_password(&self.password_opts, &mut self.rng);
        let analysis = analyze_password(&pw);
        let ts = self.tick();
        self.history.push(HistoryEntry {
            password: pw.clone(),
            strength: analysis.rating,
            entropy: analysis.entropy_bits,
            gen_type: "Password".to_owned(),
            timestamp: ts,
        });
        self.current_analysis = Some(analysis);
        self.current_password = pw;
    }

    /// Generate a new passphrase.
    pub fn gen_passphrase(&mut self) {
        let pp = generate_passphrase(&self.passphrase_opts, &mut self.rng);
        let analysis = analyze_password(&pp);
        let ts = self.tick();
        self.history.push(HistoryEntry {
            password: pp.clone(),
            strength: analysis.rating,
            entropy: analysis.entropy_bits,
            gen_type: "Passphrase".to_owned(),
            timestamp: ts,
        });
        self.current_analysis = Some(analysis);
        self.current_password = pp;
    }

    /// Generate a PIN.
    pub fn gen_pin(&mut self) {
        let pin = generate_pin(self.pin_length, &mut self.rng);
        let analysis = analyze_password(&pin);
        let ts = self.tick();
        self.history.push(HistoryEntry {
            password: pin.clone(),
            strength: analysis.rating,
            entropy: analysis.entropy_bits,
            gen_type: "PIN".to_owned(),
            timestamp: ts,
        });
        self.current_analysis = Some(analysis);
        self.current_password = pin;
    }

    /// Generate a pronounceable password.
    pub fn gen_pronounceable(&mut self) {
        let pw = generate_pronounceable(self.password_opts.length, &mut self.rng);
        let analysis = analyze_password(&pw);
        let ts = self.tick();
        self.history.push(HistoryEntry {
            password: pw.clone(),
            strength: analysis.rating,
            entropy: analysis.entropy_bits,
            gen_type: "Pronounceable".to_owned(),
            timestamp: ts,
        });
        self.current_analysis = Some(analysis);
        self.current_password = pw;
    }

    /// Bulk generate passwords.
    pub fn gen_bulk(&mut self) {
        self.bulk_results.clear();
        for _ in 0..self.bulk_count {
            let pw = generate_password(&self.password_opts, &mut self.rng);
            self.bulk_results.push(pw);
        }
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

    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
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
            let btn_w = tab.label().len() as f32 * 8.0 + 20.0;
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
        let pw_display = if self.current_password.is_empty() {
            "Click Generate to create a password".to_owned()
        } else {
            self.current_password.clone()
        };
        let pw_color = if self.current_password.is_empty() {
            OVERLAY0
        } else {
            TEXT
        };
        cmds.push(RenderCommand::Text {
            x: lx + 8.0,
            y: cy + 9.0,
            text: pw_display,
            color: pw_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w - 16.0),
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
            let btn_w = label.len() as f32 * 7.5 + 24.0;
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
                });
                cy += 22.0;

                if let Some(ref analysis) = self.current_analysis {
                    // Rating badge
                    let badge_w = analysis.rating.label().len() as f32 * 9.0 + 20.0;
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
                        });
                        cmds.push(RenderCommand::Text {
                            x: lx + 160.0,
                            y: cy,
                            text: (*value).clone(),
                            color: TEXT,
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(max_w - 170.0),
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
                        });
                        cy += 18.0;

                        for pattern in &analysis.patterns_found {
                            cmds.push(RenderCommand::Text {
                                x: lx + 4.0,
                                y: cy,
                                text: format!("[{}] {}", pattern.kind.label(), pattern.description),
                                color: PEACH,
                                font_size: 10.0,
                                font_weight: FontWeightHint::Regular,
                                max_width: Some(max_w - 8.0),
                            });
                            cy += 15.0;
                        }
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
                });
                cy += 22.0;

                for entry in self.history.iter().rev().take(20) {
                    if cy > y + height {
                        break;
                    }

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

                    cmds.push(RenderCommand::Text {
                        x: lx + 22.0,
                        y: cy + 8.0,
                        text: entry.password.clone(),
                        color: TEXT,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_w - 140.0),
                    });

                    cmds.push(RenderCommand::Text {
                        x: lx + max_w - 110.0,
                        y: cy + 8.0,
                        text: format!("[{}] {:.0}b", entry.gen_type, entry.entropy),
                        color: SUBTEXT1,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(100.0),
                    });

                    cy += ITEM_HEIGHT + 4.0;
                }
            }
        }
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let mut app = PasswordApp::new(42);
    app.gen_password();
    app.gen_passphrase();
    app.gen_pin();

    let cmds = app.render(1100.0, 700.0);
    let _ = cmds.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- RNG tests ---

    #[test]
    fn test_rng_deterministic() {
        let mut r1 = Rng::new(42);
        let mut r2 = Rng::new(42);
        assert_eq!(r1.next_u64(), r2.next_u64());
        assert_eq!(r1.next_u64(), r2.next_u64());
    }

    #[test]
    fn test_rng_bounded() {
        let mut rng = Rng::new(123);
        for _ in 0..100 {
            let v = rng.next_usize(10);
            assert!(v < 10);
        }
    }

    #[test]
    fn test_rng_zero_bound() {
        let mut rng = Rng::new(1);
        assert_eq!(rng.next_usize(0), 0);
    }

    // --- Password generation ---

    #[test]
    fn test_generate_password_length() {
        let mut rng = Rng::new(1);
        let opts = PasswordOptions {
            length: 20,
            ..PasswordOptions::default()
        };
        let pw = generate_password(&opts, &mut rng);
        assert_eq!(pw.len(), 20);
    }

    #[test]
    fn test_generate_password_includes_classes() {
        let mut rng = Rng::new(42);
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
        let mut rng = Rng::new(1);
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
        let mut rng = Rng::new(1);
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
        let mut rng = Rng::new(1);
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
        let mut rng = Rng::new(42);
        let opts = PassphraseOptions::default();
        let pp = generate_passphrase(&opts, &mut rng);
        assert!(!pp.is_empty());
        // Should contain separator
        assert!(pp.contains('-'));
    }

    #[test]
    fn test_passphrase_word_count() {
        let mut rng = Rng::new(42);
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
        let mut rng = Rng::new(1);
        let pin = generate_pin(6, &mut rng);
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
    }

    // --- Pronounceable ---

    #[test]
    fn test_generate_pronounceable() {
        let mut rng = Rng::new(1);
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
        assert_eq!(analysis.entropy_bits, 0.0);
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
        let mut app = PasswordApp::new(42);
        app.gen_password();
        assert!(!app.current_password.is_empty());
        assert!(app.current_analysis.is_some());
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn test_app_gen_passphrase() {
        let mut app = PasswordApp::new(42);
        app.gen_passphrase();
        assert!(!app.current_password.is_empty());
        assert!(app.current_password.contains('-'));
    }

    #[test]
    fn test_app_gen_pin() {
        let mut app = PasswordApp::new(42);
        app.pin_length = 4;
        app.gen_pin();
        assert_eq!(app.current_password.len(), 4);
    }

    #[test]
    fn test_app_gen_pronounceable() {
        let mut app = PasswordApp::new(42);
        app.gen_pronounceable();
        assert!(!app.current_password.is_empty());
    }

    #[test]
    fn test_app_bulk_generate() {
        let mut app = PasswordApp::new(42);
        app.bulk_count = 5;
        app.gen_bulk();
        assert_eq!(app.bulk_results.len(), 5);
    }

    #[test]
    fn test_app_clear_history() {
        let mut app = PasswordApp::new(42);
        app.gen_password();
        app.gen_passphrase();
        assert_eq!(app.history.len(), 2);
        app.clear_history();
        assert!(app.history.is_empty());
    }

    #[test]
    fn test_app_export_history() {
        let mut app = PasswordApp::new(42);
        app.gen_password();
        let export = app.export_history();
        assert!(export.contains("Password Generation History"));
    }

    #[test]
    fn test_app_render() {
        let mut app = PasswordApp::new(42);
        app.gen_password();
        let cmds = app.render(1100.0, 700.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_strength_rating_ordering() {
        assert!(StrengthRating::VeryWeak < StrengthRating::Weak);
        assert!(StrengthRating::Weak < StrengthRating::Fair);
        assert!(StrengthRating::Fair < StrengthRating::Strong);
        assert!(StrengthRating::Strong < StrengthRating::VeryStrong);
    }

    #[test]
    fn test_is_common_password() {
        assert!(is_common_password("password"));
        assert!(is_common_password("123456"));
        assert!(is_common_password("Password")); // Case-insensitive
        assert!(!is_common_password("xK9mQ2pL7nR4"));
    }
}

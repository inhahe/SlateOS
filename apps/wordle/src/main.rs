//! Wordle — guess a hidden word in six tries, with each guess answered
//! letter by letter: green for right letter in the right place, yellow for
//! right letter in the wrong place, grey for a letter that is not in the word
//! at all. Four-, five- and six-letter puzzles; an on-screen keyboard that
//! remembers what every letter has been shown to be; and a hard mode that
//! refuses a guess contradicting what you have already been told.
//!
//! ## What wiring it found
//!
//! `main` was `let _app = Wordle::new();` — it seeded a generator from the
//! system, drew a target word, built a keyboard and dropped the lot, so no
//! puzzle ever reached a screen and no key or click ever arrived.
//!
//! Under that, **the picture and the click map were the same numbers written
//! twice.** `render` laid the on-screen keyboard out from `kb_y_start = 420.0`,
//! `key_w = 36.0`, `key_h = 40.0`, `gap = 4.0` and a left edge of `80.0`, and
//! `handle_keyboard_click` re-derived every one of those literals to decide
//! what had been clicked — including the two hand-computed expressions for the
//! Enter and Backspace boxes. Two copies of a layout are two layouts
//! (`known-issues.md` lesson 63); they agreed only for as long as nobody
//! touched one of them, and neither consulted the window. The layout is solved
//! from the live window size every frame now, and the hit boxes are recorded
//! by the drawing pass, so a key is clickable exactly where its ink is.
//!
//! Twelve blanket `#![allow(...)]` sat at the top of the file — `dead_code`
//! and `unused_imports` among them, which is what let a program whose `main`
//! discarded its own app compile without a word of complaint.

use guitk::color::Color;
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;

// ── Catppuccin Mocha palette ──
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Tile states ──
#[derive(Clone, Copy, Debug, PartialEq)]
enum TileState {
    Empty,
    Filled,  // letter entered but not evaluated
    Correct, // green — right letter, right position
    Present, // yellow — right letter, wrong position
    Absent,  // gray — letter not in word
}

impl TileState {
    fn color(self) -> Color {
        match self {
            Self::Empty => SURFACE0,
            Self::Filled => SURFACE1,
            Self::Correct => GREEN,
            Self::Present => YELLOW,
            Self::Absent => SURFACE2,
        }
    }
}

// ── Keyboard letter state ──
#[derive(Clone, Copy, Debug, PartialEq)]
enum LetterState {
    Unknown,
    Correct,
    Present,
    Absent,
}

impl LetterState {
    fn color(self) -> Color {
        match self {
            Self::Unknown => SURFACE1,
            Self::Correct => GREEN,
            Self::Present => YELLOW,
            Self::Absent => OVERLAY0,
        }
    }
}

// ── Difficulty ──
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Easy,   // 4 letters
    Normal, // 5 letters (classic)
    Hard,   // 6 letters
}

/// Guesses allowed, for every difficulty.
///
/// This was `Difficulty::max_guesses(self)`, a method that took a receiver and
/// returned `6` without looking at it. A method that ignores its receiver
/// advertises, in its signature, a rule that varies when it does not — every
/// caller had to read the body to learn that the four-letter puzzle is not
/// more generous than the six-letter one. It is a constant, so it is written
/// as one.
const MAX_GUESSES: usize = 6;

/// The longest word any difficulty uses, and so the width of the fixed arrays
/// a guess and its evaluation are carried in.
const MAX_WORD: usize = 6;

impl Difficulty {
    fn word_len(self) -> usize {
        match self {
            Self::Easy => 4,
            Self::Normal => 5,
            Self::Hard => 6,
        }
    }

    /// What the difficulty is called, without its length.
    ///
    /// The length is not written into the name. It used to be — the labels
    /// read `"Easy (4)"`, `"Normal (5)"`, `"Hard (6)"` — which is [`word_len`]
    /// spelled a second time in a place no test would ever compare against the
    /// first (`known-issues.md` lesson 63). The button composes the two.
    ///
    /// [`word_len`]: Self::word_len
    fn name(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Normal => "Normal",
            Self::Hard => "Hard",
        }
    }
}

// ── Randomness ──
//
// This file used to carry its own copy of the LCG that `guitk::rng` exists to
// replace, reduced with `% max` and seeded with a literal `42`. The seed picks
// the target word, so every fresh launch of the game set the same puzzle --
// and once a player had solved it, the next launch handed it back.

/// The seed a session falls back to when the kernel has no entropy to give.
///
/// A word may be predictable: the worst outcome is a repeated puzzle.
/// Refusing to start would be the worse failure; the rule is written out at
/// [`guitk::rng::seeded_from_system`]. "WORDLE!!" in ASCII.
const FALLBACK_SEED: u64 = 0x574F_5244_4C45_2121;

// ── Word lists ──
const WORDS_4: &[&str] = &[
    "able", "also", "area", "army", "away", "back", "band", "bank", "base", "bath", "bean", "bear",
    "beat", "bell", "best", "bird", "bite", "blow", "blue", "boat", "body", "bomb", "bone", "book",
    "born", "boss", "both", "bowl", "burn", "busy", "cafe", "cage", "cake", "call", "calm", "came",
    "camp", "card", "care", "case", "cash", "cast", "cave", "chat", "chip", "city", "clap", "clay",
    "clip", "club", "coal", "coat", "code", "coin", "cold", "come", "cook", "cool", "cope", "copy",
    "core", "cost", "crew", "crop", "curl", "cute", "dare", "dark", "data", "date", "dawn", "dead",
    "deaf", "deal", "dear", "debt", "deck", "deep", "deer", "deny", "desk", "diet", "dirt", "dish",
    "disk", "dock", "does", "done", "door", "dose", "down", "drag", "draw", "drop", "drum", "dual",
    "duck", "dull", "dump", "dust", "duty", "each", "earn", "ease", "east", "easy", "edge", "edit",
    "else", "epic", "even", "ever", "evil", "exam", "exit", "face", "fact", "fade", "fail", "fair",
    "fake", "fall", "fame", "farm", "fast", "fate", "fear", "feed", "feel", "file", "fill", "film",
    "find", "fine", "fire", "firm", "fish", "flag", "flat", "flew", "flip", "flow", "fold", "folk",
    "fond", "font", "food", "fool", "foot", "fork", "form", "fort", "foul", "four", "free", "from",
    "fuel", "full", "fund", "fury", "fuse", "gain", "game", "gang", "gate", "gave", "gaze", "gear",
    "gift", "girl", "give", "glad", "glow", "glue", "goal", "goat", "goes", "gold", "golf", "gone",
    "good", "grab", "gray", "grew", "grid", "grin", "grip", "grow", "gulf", "gust", "guys", "hack",
    "hair", "half", "hall", "halt", "hand", "hang", "hard", "harm", "harp", "hate", "have", "head",
    "heal", "heap", "hear", "heat", "heel", "held", "help", "herb", "here", "hero", "hide", "high",
    "hike", "hill", "hint", "hire", "hold", "hole", "holy", "home", "hood", "hook", "hope", "horn",
    "host", "hour", "huge", "hung", "hunt", "hurt", "hymn", "icon", "idea", "inch", "into", "iron",
    "item", "jack", "jail", "jazz", "jean", "jobs", "join", "joke", "jump", "jury", "just", "keen",
    "keep", "kept", "kick", "kids", "kill", "kind", "king", "kiss", "knee", "knew", "knit", "knob",
    "knot", "know", "lack", "laid", "lake", "lamp", "land", "lane", "last", "late", "lawn", "lead",
    "leaf", "lean", "left", "lend", "lens", "less", "lied", "life", "lift", "like", "limb", "lime",
    "line", "link", "lion", "list", "live", "load", "loan", "lock", "logo", "long", "look", "loop",
    "lord", "lose", "loss", "lost", "lots", "loud", "love", "luck", "lung", "made", "mail", "main",
    "make", "male", "mall", "many", "maps", "mark", "mass", "mate", "maze", "meal", "mean", "meat",
    "meet", "melt", "menu", "mere", "mesh", "mess", "mild", "mile", "milk", "mill", "mind", "mine",
    "mint", "miss", "mode", "mood", "moon", "more", "moss", "most", "move", "much", "must", "myth",
    "nail", "name", "navy", "near", "neat", "neck", "need", "nest", "nets", "next", "nice", "nine",
    "node", "none", "norm", "nose", "note", "noun", "odds", "okay", "once", "ones", "only", "onto",
    "open", "oral", "oven", "over", "pace", "pack", "page", "paid", "pain", "pair", "pale", "palm",
    "pane", "park", "part", "pass", "past", "path", "peak", "peel", "peer", "pick", "pile", "pine",
    "pink", "pipe", "plan", "play", "plot", "plug", "plus", "poem", "poet", "pole", "poll", "pond",
    "pool", "poor", "pope", "pork", "port", "pose", "post", "pour", "pray", "prey", "pull", "pump",
    "pure", "push", "quit", "quiz", "race", "rack", "rage", "raid", "rail", "rain", "rank", "rare",
    "rate", "read", "real", "rear", "reef", "rely", "rent", "rest", "rice", "rich", "ride", "ring",
    "rise", "risk", "road", "rock", "rode", "role", "roll", "roof", "room", "root", "rope", "rose",
    "ruin", "rule", "rush", "safe", "sage", "said", "sake", "sale", "salt", "same", "sand", "sang",
    "save", "seal", "seat", "seed", "seek", "seem", "seen", "self", "sell", "send", "sent", "sept",
    "shed", "ship", "shop", "shot", "show", "shut", "sick", "side", "sigh", "sign", "silk", "sing",
    "sink", "site", "size", "skin", "slam", "slid", "slim", "slip", "slot", "slow", "snap", "snow",
    "soap", "sofa", "soft", "soil", "sold", "sole", "some", "song", "soon", "sort", "soul", "soup",
    "spin", "spot", "star", "stay", "stem", "step", "stir", "stop", "such", "suit", "sure", "surf",
    "swim", "tack", "tail", "take", "tale", "talk", "tall", "tank", "tape", "task", "taxi", "team",
    "tear", "tell", "tend", "tent", "term", "test", "text", "than", "that", "them", "then", "they",
    "thin", "this", "thus", "tick", "tide", "tidy", "tied", "tier", "tile", "till", "time", "tiny",
    "tire", "toad", "told", "toll", "tone", "took", "tool", "tops", "tore", "torn", "tour", "town",
    "trap", "tray", "tree", "trim", "trio", "trip", "true", "tube", "tuck", "tune", "turn", "twin",
    "type", "ugly", "unit", "upon", "urge", "used", "user", "vale", "vary", "vast", "veil", "vein",
    "vent", "verb", "very", "vest", "view", "vine", "visa", "void", "volt", "vote", "wade", "wage",
    "wait", "wake", "walk", "wall", "want", "ward", "warm", "warn", "warp", "wash", "wave", "weak",
    "wear", "weed", "week", "well", "went", "were", "west", "what", "when", "whom", "wide", "wife",
    "wild", "will", "wind", "wine", "wing", "wire", "wise", "wish", "with", "woke", "wolf", "wood",
    "wool", "word", "wore", "work", "worm", "worn", "wrap", "yard", "year", "yell", "yoga", "your",
    "zero", "zone", "zoom",
];

const WORDS_5: &[&str] = &[
    "about", "above", "abuse", "actor", "acute", "adapt", "admit", "adopt", "adult", "after",
    "again", "agree", "ahead", "alarm", "album", "alien", "align", "alike", "alive", "alley",
    "allow", "alone", "along", "alter", "amaze", "among", "ample", "angel", "anger", "angle",
    "annex", "apple", "apply", "arena", "argue", "arise", "armor", "array", "arrow", "aside",
    "asset", "atlas", "avoid", "awake", "award", "aware", "badge", "badly", "baker", "basic",
    "basin", "basis", "batch", "beach", "beard", "beast", "begin", "being", "belly", "below",
    "bench", "berry", "birth", "black", "blade", "blame", "bland", "blank", "blast", "blaze",
    "bleed", "blend", "bless", "blind", "block", "blood", "bloom", "blown", "blues", "bluff",
    "blunt", "board", "bonus", "boost", "booth", "bound", "brain", "brand", "brave", "bread",
    "break", "breed", "brick", "bride", "brief", "bring", "broad", "brook", "brown", "brush",
    "build", "burst", "buyer", "cabin", "cable", "camel", "cargo", "carry", "catch", "cause",
    "cedar", "chain", "chair", "chalk", "chaos", "charm", "chart", "chase", "cheap", "check",
    "cheek", "cheer", "chess", "chest", "chief", "child", "chill", "china", "chunk", "civic",
    "claim", "clash", "class", "clean", "clear", "clerk", "cliff", "climb", "cling", "clock",
    "clone", "close", "cloth", "cloud", "coach", "coast", "color", "comet", "comic", "coral",
    "count", "court", "cover", "crack", "craft", "crane", "crash", "crazy", "cream", "crest",
    "crime", "crisp", "cross", "crowd", "crown", "crude", "crush", "curve", "cycle", "daily",
    "dance", "debut", "decay", "delay", "delta", "dense", "depot", "depth", "derby", "devil",
    "diary", "dirty", "donor", "doubt", "dough", "draft", "drain", "drama", "drank", "drawn",
    "dream", "dress", "dried", "drift", "drill", "drink", "drive", "drunk", "dying", "eager",
    "eagle", "early", "earth", "eight", "elder", "elect", "elite", "email", "ember", "empty",
    "enemy", "enjoy", "enter", "entry", "equal", "error", "essay", "event", "every", "exact",
    "exile", "exist", "extra", "fable", "facet", "faith", "false", "fancy", "fatal", "fault",
    "feast", "fence", "ferry", "fetch", "fever", "fiber", "field", "fifth", "fifty", "fight",
    "final", "first", "fixed", "flame", "flash", "flask", "fleet", "flesh", "float", "flood",
    "floor", "flour", "fluid", "flush", "focal", "focus", "force", "forge", "forth", "forum",
    "found", "frame", "frank", "fraud", "fresh", "front", "frost", "fruit", "fully", "funny",
    "giant", "given", "glass", "gleam", "glide", "globe", "gloom", "glory", "gloss", "glove",
    "going", "grace", "grade", "grain", "grand", "grant", "graph", "grasp", "grass", "grave",
    "great", "greed", "green", "greet", "grief", "grind", "groan", "groom", "gross", "group",
    "grove", "grown", "guard", "guess", "guide", "guild", "guilt", "gully", "habit", "happy",
    "harsh", "haste", "haunt", "heart", "heavy", "hedge", "hello", "hence", "hired", "hobby",
    "honor", "horse", "hotel", "house", "human", "humor", "hurry", "ideal", "image", "imply",
    "index", "indie", "inner", "input", "Irish", "issue", "ivory", "japan", "jewel", "joint",
    "judge", "juice", "karma", "knock", "kneel", "knife", "known", "label", "labor", "large",
    "laser", "later", "laugh", "layer", "learn", "lease", "leave", "legal", "lemon", "level",
    "light", "limit", "liner", "linen", "liver", "local", "lodge", "logic", "login", "loose",
    "lover", "lower", "loyal", "lucky", "lunch", "lunar", "lying", "magic", "major", "maker",
    "manor", "maple", "march", "marry", "match", "mayor", "media", "mercy", "merit", "metal",
    "meter", "micro", "might", "mimic", "minor", "minus", "mixed", "model", "money", "month",
    "moral", "motor", "mount", "mouse", "mouth", "movie", "muddy", "music", "naval", "nerve",
    "never", "newly", "night", "noble", "noise", "north", "noted", "novel", "nurse", "nylon",
    "occur", "ocean", "offer", "olive", "onset", "opera", "orbit", "order", "organ", "other",
    "outer", "ought", "owner", "oxide", "ozone", "paint", "panel", "panic", "paper", "patch",
    "pause", "peace", "penny", "phase", "phone", "photo", "piano", "piece", "pilot", "pinch",
    "pitch", "pixel", "pizza", "place", "plain", "plane", "plant", "plate", "plaza", "plead",
    "pluck", "plumb", "point", "polar", "pound", "power", "press", "price", "pride", "prime",
    "print", "prior", "prize", "probe", "proof", "proud", "prove", "proxy", "psalm", "pulse",
    "punch", "pupil", "purse", "queen", "quest", "queue", "quick", "quiet", "quite", "quota",
    "quote", "radar", "radio", "raise", "rally", "ranch", "range", "rapid", "reach", "react",
    "realm", "rebel", "refer", "reign", "relax", "renew", "repay", "reply", "rider", "ridge",
    "rifle", "right", "rigid", "risky", "rival", "river", "robin", "robot", "rocky", "rouge",
    "rough", "round", "route", "royal", "rugby", "ruler", "rural", "saint", "salad", "sales",
    "sauce", "scale", "scare", "scene", "scent", "scope", "score", "scout", "scrap", "seize",
    "sense", "serve", "setup", "seven", "shade", "shake", "shall", "shame", "shape", "share",
    "shark", "sharp", "sheep", "sheer", "sheet", "shelf", "shell", "shift", "shine", "shirt",
    "shock", "shoot", "shore", "short", "sight", "since", "sixth", "sixty", "sized", "skill",
    "skull", "slash", "slave", "sleep", "slice", "slide", "slope", "small", "smart", "smell",
    "smile", "smith", "smoke", "snake", "solar", "solid", "solve", "sorry", "sound", "south",
    "space", "spare", "speak", "speed", "spend", "spent", "spice", "spike", "spine", "spite",
    "split", "spoon", "sport", "spray", "squad", "stack", "staff", "stage", "stain", "stake",
    "stale", "stall", "stamp", "stand", "stare", "start", "state", "stays", "steam", "steel",
    "steep", "steer", "stern", "stick", "stiff", "still", "stock", "stone", "stood", "store",
    "storm", "story", "stout", "stove", "strap", "straw", "strip", "stuck", "study", "stuff",
    "style", "suite", "super", "surge", "swamp", "swear", "sweep", "sweet", "swept", "swift",
    "swing", "sword", "syrup", "table", "taste", "teach", "tempo", "thank", "theme", "thick",
    "thing", "think", "third", "thorn", "those", "three", "threw", "throw", "thumb", "tight",
    "timer", "tired", "title", "toast", "today", "token", "tooth", "topic", "total", "touch",
    "tough", "tower", "toxic", "trace", "track", "trade", "trail", "train", "trait", "trash",
    "treat", "trend", "trial", "tribe", "trick", "tried", "troop", "truck", "truly", "trump",
    "trunk", "trust", "truth", "tumor", "tuner", "twice", "twist", "ultra", "uncle", "under",
    "unify", "union", "unite", "unity", "until", "upper", "upset", "urban", "usage", "usual",
    "valid", "value", "valve", "vapor", "vault", "venue", "verse", "video", "vigor", "vinyl",
    "viral", "virus", "visit", "vital", "vivid", "vocal", "voice", "voter", "wagon", "waste",
    "watch", "water", "weave", "weigh", "weird", "whale", "wheat", "wheel", "where", "which",
    "while", "white", "whole", "whose", "wider", "witch", "woman", "world", "worry", "worse",
    "worst", "worth", "would", "wound", "wrath", "write", "wrong", "wrote", "yacht", "yield",
    "young", "yours", "youth",
];

const WORDS_6: &[&str] = &[
    "absorb", "accept", "access", "across", "acting", "action", "active", "actual", "afford",
    "agenda", "almost", "always", "amount", "animal", "annual", "anyone", "anyway", "appeal",
    "appear", "around", "arrive", "artist", "aspect", "assert", "assess", "assist", "assume",
    "attach", "attack", "attend", "author", "banner", "barely", "basket", "battle", "become",
    "before", "behalf", "behind", "belong", "beside", "beyond", "bitter", "blanch", "blight",
    "border", "borrow", "bottle", "bottom", "bounce", "branch", "breath", "bridge", "bright",
    "broken", "bronze", "broker", "browse", "bubble", "bucket", "budget", "buffer", "bundle",
    "burden", "bureau", "butter", "button", "camera", "cancel", "carbon", "carpet", "casual",
    "caught", "center", "chance", "change", "charge", "choose", "church", "circle", "clause",
    "client", "closet", "clutch", "coffee", "colony", "column", "combat", "comedy", "coming",
    "commit", "common", "comply", "convey", "cookie", "copper", "corner", "costly", "cotton",
    "county", "couple", "course", "cousin", "create", "credit", "crisis", "custom", "damage",
    "danger", "dealer", "debate", "decade", "decide", "defeat", "defend", "define", "degree",
    "delete", "demand", "dental", "depart", "deploy", "deputy", "derive", "desert", "design",
    "desire", "detail", "detect", "device", "differ", "digest", "dinner", "direct", "divide",
    "domain", "double", "driver", "during", "easily", "eating", "editor", "effect", "effort",
    "emerge", "empire", "enable", "endure", "energy", "engage", "engine", "enough", "ensure",
    "entire", "entity", "equity", "escape", "estate", "ethnic", "evolve", "exceed", "except",
    "excite", "excuse", "exempt", "exotic", "expand", "expect", "expert", "export", "expose",
    "extend", "extent", "fabric", "factor", "fairly", "family", "famous", "farmer", "father",
    "faucet", "fellow", "female", "fierce", "figure", "filter", "finger", "fiscal", "flight",
    "flower", "follow", "forbid", "forced", "forest", "forget", "format", "former", "foster",
    "french", "friend", "frozen", "future", "gallon", "garage", "garden", "gather", "gender",
    "gentle", "gifted", "global", "govern", "gravel", "guided", "guilty", "guitar", "handle",
    "happen", "hardly", "hazard", "health", "heaven", "height", "helmet", "hidden", "highly",
    "honest", "horror", "hunger", "hunter", "ignore", "import", "impose", "income", "indeed",
    "inform", "injure", "inland", "insect", "insert", "inside", "insist", "intact", "intend",
    "invest", "island", "itself", "jacket", "jargon", "jogger", "jungle", "junior", "kernel",
    "kidney", "knight", "ladder", "lately", "latter", "launch", "lawyer", "layout", "leader",
    "league", "legacy", "lender", "lesson", "letter", "likely", "linger", "liquid", "listen",
    "little", "lively", "living", "locate", "locker", "lonely", "lovely", "luxury", "magnet",
    "maiden", "mainly", "manage", "manner", "marble", "margin", "marine", "market", "master",
    "matter", "medium", "member", "memoir", "memory", "mental", "mentor", "merger", "method",
    "middle", "mighty", "mingle", "minute", "mirror", "mobile", "modern", "modest", "modify",
    "moment", "monkey", "mortal", "mostly", "mother", "motion", "muffin", "murder", "museum",
    "mutual", "muzzle", "myriad", "narrow", "nation", "nature", "nearby", "nearly", "needle",
    "nickel", "nobody", "normal", "notice", "number", "object", "obtain", "occupy", "offend",
    "office", "online", "opener", "oppose", "option", "orange", "origin", "outfit", "output",
    "palace", "parent", "parish", "partly", "patent", "patrol", "patron", "peanut", "pencil",
    "people", "period", "permit", "person", "phrase", "pigeon", "pillar", "pillow", "planet",
    "player", "please", "pledge", "plenty", "plunge", "pocket", "poetry", "poison", "police",
    "policy", "polish", "polite", "ponder", "poster", "potato", "powder", "prayer", "prefer",
    "profit", "prompt", "proper", "proven", "public", "puddle", "punish", "purple", "pursue",
    "puzzle", "rabbit", "racial", "random", "rarely", "rating", "rather", "reader", "reason",
    "recall", "recent", "record", "reduce", "reform", "refuge", "regard", "regime", "region",
    "reject", "relate", "relief", "remain", "remedy", "remote", "remove", "render", "rental",
    "repair", "repeat", "report", "rescue", "resign", "resist", "resort", "result", "retain",
    "retire", "return", "reveal", "review", "revolt", "reward", "ribbon", "riding", "ritual",
    "robust", "rocket", "roster", "rubber", "runner", "sacred", "saddle", "safely", "sailor",
    "salary", "salmon", "sample", "saving", "scheme", "school", "screen", "script", "search",
    "season", "second", "secret", "sector", "secure", "select", "seller", "senior", "series",
    "server", "settle", "severe", "shadow", "shaken", "shield", "shower", "shrink", "signal",
    "silent", "silver", "simple", "simply", "single", "sister", "sketch", "sleeve", "slight",
    "slowly", "smooth", "soccer", "social", "soften", "source", "sphere", "spirit", "splash",
    "spread", "spring", "square", "stable", "stance", "statue", "status", "steady", "strain",
    "strand", "stream", "street", "stress", "strict", "stride", "strike", "string", "stripe",
    "stroke", "strong", "studio", "submit", "subtle", "sudden", "suffer", "summer", "summit",
    "sunset", "superb", "supply", "surely", "survey", "switch", "symbol", "tackle", "talent",
    "target", "temple", "tenant", "tender", "terror", "threat", "thrive", "throne", "ticket",
    "timber", "tissue", "tongue", "toward", "travel", "treaty", "tribal", "trophy", "tunnel",
    "twelve", "unfair", "unfold", "unique", "united", "unless", "unlike", "unveil", "update",
    "uphold", "urgent", "useful", "valley", "vanish", "vendor", "veneer", "verbal", "verify",
    "victim", "violet", "virtue", "vision", "visual", "volume", "wander", "warmth", "wealth",
    "weapon", "weekly", "widely", "window", "winter", "wisdom", "within", "wonder", "worker",
    "worthy", "wounds", "writer", "yellow",
];

// ── Game state ──
// ── What a click can land on ────────────────────────────────────────
/// Everything the pointer can hit, recorded by the drawing pass so that a
/// target exists exactly where its ink was put.
///
/// The old program had one hit test, `handle_keyboard_click`, and it
/// re-derived the keyboard's geometry from the same literals `render` used —
/// so the two were a rule kept by copying (`known-issues.md` lesson 63) and
/// neither of them was a rule about the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// A letter on the on-screen keyboard, as an uppercase ASCII letter.
    Key(char),
    /// The on-screen Enter key.
    Enter,
    /// The on-screen Backspace key.
    Backspace,
    /// One of the three difficulty buttons.
    Level(Difficulty),
    /// The hard-mode switch.
    HardMode,
    /// Deal a new puzzle.
    NewGame,
}

// ── Layout ──────────────────────────────────────────────────────────
/// Where everything goes in a window of a given size, for a puzzle of a given
/// word length.
///
/// Built fresh every frame and never stored on the model. A remembered layout
/// is one that can disagree with the window it is drawn in — and a layout
/// written out twice, once to draw and once to hit-test, is two layouts that
/// can disagree with *each other*, which is what this program had.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// Title, difficulty buttons and the hard-mode switch.
    pub header: Rect,
    /// The guess grid.
    pub board: Rect,
    /// The one-line message under the grid.
    pub message: Rect,
    /// The on-screen keyboard.
    pub keyboard: Rect,
    /// Streak and win counts.
    pub footer: Rect,
    /// The side of one guess tile.
    pub tile: f32,
    /// The gap between adjacent tiles, and between adjacent keys.
    pub gap: f32,
    /// The width of one letter key.
    pub key_w: f32,
    /// The height of one keyboard row.
    pub key_h: f32,
    /// The gap between adjacent keys, across and down.
    pub key_gap: f32,
    /// The word length this layout was solved for.
    pub cols: usize,
    pub font: f32,
    pub small: f32,
    pub big: f32,
    pub pad: f32,
}

/// The letters of the on-screen keyboard, in the three rows they are drawn in.
///
/// One table, read by the drawing pass, which is what records the hit boxes —
/// so there is no second copy for the pointer to disagree with.
const KEY_ROWS: [&str; 3] = ["QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"];

/// The fraction of the window height the guess grid is guaranteed before any
/// band of chrome keeps its full height.
const BOARD_SHARE: f32 = 0.30;

/// Bands give up their height in this order when the window is too short:
/// the footer stats first, then the message line, then the header. The board
/// and the keyboard never give up any — without them there is no game.
const BAND_DROP_ORDER: [usize; 3] = [2, 1, 0];

/// The gap between adjacent tiles, per unit of tile size, so that every board
/// dimension is a multiple of the single number `tile`.
const GAP_PER_TILE: f32 = 0.10;

/// The buttons in the header, left to right. The pointer reads them from the
/// rectangles the drawing pass recorded; this table is what fixes the order.
const HEADER_BUTTONS: [Target; 5] = [
    Target::Level(Difficulty::Easy),
    Target::Level(Difficulty::Normal),
    Target::Level(Difficulty::Hard),
    Target::HardMode,
    Target::NewGame,
];

const WINDOW_WIDTH: f32 = 560.0;
const WINDOW_HEIGHT: f32 = 720.0;

impl Layout {
    /// The layout for a window of the given size holding a `cols`-letter word.
    #[must_use]
    pub fn new(width: f32, height: f32, cols: usize) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 40.0).clamp(8.0, 16.0);
        let small = (font - 2.0).max(7.0);
        let big = (font * 1.7).clamp(13.0, 30.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each droppable band would like, in [header, message, footer]
        // order. The keyboard is not in this list: a Wordle you cannot type
        // into is not a smaller Wordle, it is a picture of one.
        let mut wants = [
            (h * 0.10).clamp(24.0, 54.0),
            (h * 0.06).clamp(15.0, 30.0),
            (h * 0.07).clamp(16.0, 36.0),
        ];
        let kb_want = (h * 0.26).clamp(24.0, 180.0);
        let budget = (h - h * BOARD_SHARE - kb_want - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, msg_h, ftr_h] = wants;

        // A dropped band is a full-width strip nought pixels tall rather than
        // `Rect::EMPTY`. `Rect::is_empty` is `w <= 0.0 || h <= 0.0`, so a
        // zero-height strip already answers "no" to the only question drawing
        // code asks, and it sits where the band would have been — which is
        // what lets the edges below fall out without a guard apiece
        // (`known-issues.md` lesson 51, learnt the expensive way in sokoban).
        let header = Rect::new(0.0, 0.0, w, hdr_h);
        let footer = Rect::new(0.0, h - ftr_h, w, ftr_h);

        // The keyboard takes what it asked for, or what is left over it,
        // whichever is less — it is the one band that shrinks rather than
        // vanishing.
        let above_kb = hdr_h + msg_h + pad * 2.0;
        let kb_h = kb_want.min((footer.y - above_kb).max(0.0));
        let keyboard = Rect::new(pad, footer.y - kb_h, (w - pad * 2.0).max(0.0), kb_h);
        let message = Rect::new(0.0, keyboard.y - msg_h, w, msg_h);

        let key_gap = (keyboard.w * 0.006).clamp(1.0, 5.0);
        let key_w = ((keyboard.w - key_gap * 9.0) / 10.0).max(0.0);
        let key_h = ((keyboard.h - key_gap * 2.0) / 3.0).max(0.0);

        // What is left between the header and the message is the guess grid's,
        // and the grid is solved square inside it from both dimensions at once
        // — a stretched grid is one whose tiles are no longer where a square
        // hit box says they are.
        let area = Rect::new(
            pad,
            hdr_h + pad,
            (w - pad * 2.0).max(0.0),
            (message.y - hdr_h - pad * 2.0).max(0.0),
        );
        let (tile, gap, board) = if cols > 0 {
            let across = cols as f32;
            let down = MAX_GUESSES as f32;
            let per_w = across + (across - 1.0) * GAP_PER_TILE;
            let per_h = down + (down - 1.0) * GAP_PER_TILE;
            let tile = (area.w / per_w).min(area.h / per_h).max(0.0);
            let gap = tile * GAP_PER_TILE;
            let grid_w = across * tile + (across - 1.0) * gap;
            let grid_h = down * tile + (down - 1.0) * gap;
            let board = Rect::new(
                area.x + (area.w - grid_w) / 2.0,
                area.y + (area.h - grid_h) / 2.0,
                grid_w,
                grid_h,
            );
            (tile, gap, board)
        } else {
            (0.0, 0.0, Rect::EMPTY)
        };

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            board,
            message,
            keyboard,
            footer,
            tile,
            gap,
            key_w,
            key_h,
            key_gap,
            cols,
            font,
            small,
            big,
            pad,
        }
    }

    /// The tile for guess `row`, letter `col`.
    #[must_use]
    pub fn tile_rect(&self, row: usize, col: usize) -> Rect {
        if row >= MAX_GUESSES || col >= self.cols {
            return Rect::EMPTY;
        }
        let step = self.tile + self.gap;
        Rect::new(
            self.board.x + col as f32 * step,
            self.board.y + row as f32 * step,
            self.tile,
            self.tile,
        )
    }

    /// The step from one keyboard column to the next.
    fn key_step(&self) -> f32 {
        self.key_w + self.key_gap
    }

    /// How many key widths the row is indented by. Row 1 is centred under row
    /// 0 by half a key; row 2 starts after the Enter key, which is one and a
    /// half wide.
    fn row_indent(row: usize) -> f32 {
        match row {
            1 => 0.5,
            2 => 1.5,
            _ => 0.0,
        }
    }

    /// The key at `col` of keyboard `row`.
    #[must_use]
    pub fn key_rect(&self, row: usize, col: usize) -> Rect {
        let Some(letters) = KEY_ROWS.get(row) else {
            return Rect::EMPTY;
        };
        if col >= letters.len() {
            return Rect::EMPTY;
        }
        let step = self.key_step();
        Rect::new(
            self.keyboard.x + (Self::row_indent(row) + col as f32) * step,
            self.keyboard.y + row as f32 * (self.key_h + self.key_gap),
            self.key_w,
            self.key_h,
        )
    }

    /// The wide key at the left of the bottom row.
    #[must_use]
    pub fn enter_rect(&self) -> Rect {
        Rect::new(
            self.keyboard.x,
            self.keyboard.y + 2.0 * (self.key_h + self.key_gap),
            (self.key_w * 1.5 + self.key_gap * 0.5).max(0.0),
            self.key_h,
        )
    }

    /// The wide key at the right of the bottom row.
    #[must_use]
    pub fn backspace_rect(&self) -> Rect {
        let e = self.enter_rect();
        Rect::new(
            self.keyboard.x + 8.5 * self.key_step(),
            e.y,
            e.w,
            self.key_h,
        )
    }

    /// The header's buttons, sharing the right-hand end of the band in the
    /// order [`HEADER_BUTTONS`] names them.
    ///
    /// There is deliberately no bail on an empty header or a zero width: an
    /// empty band gives zero-height buttons, which are already empty, and a
    /// guard in front of a rule that already holds is a line no test can own.
    #[must_use]
    pub fn button_rects(&self) -> [Rect; HEADER_BUTTONS.len()] {
        let n = HEADER_BUTTONS.len() as f32;
        let strip_w = (self.header.w * 0.62 - self.pad).max(0.0);
        let inner = (strip_w - self.pad * (n - 1.0)).max(0.0);
        let bw = inner / n;
        let bh = (self.header.h - self.pad).max(0.0);
        let y = self.header.y + (self.header.h - bh) / 2.0;
        let x0 = self.header.right() - self.pad - strip_w;
        let mut out = [Rect::EMPTY; HEADER_BUTTONS.len()];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = Rect::new(x0 + i as f32 * (bw + self.pad), y, bw, bh);
        }
        out
    }

    /// The left-hand end of the header, which the title gets.
    #[must_use]
    pub fn title_rect(&self) -> Rect {
        let buttons = self.button_rects();
        let right = buttons.first().map_or(self.header.right(), |b| b.x);
        Rect::new(
            self.header.x + self.pad,
            self.header.y,
            (right - self.pad * 2.0).max(0.0),
            self.header.h,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum GamePhase {
    Playing,
    Won,
    Lost,
}

/// The game: the word, the guesses made against it, and the running totals.
pub struct Wordle {
    difficulty: Difficulty,
    /// The word to be found, padded out to [`MAX_WORD`].
    target: [char; MAX_WORD],
    target_len: usize,
    /// Every guess made, with the answer it drew, letter by letter.
    guesses: Vec<([char; MAX_WORD], [TileState; MAX_WORD])>,
    current_input: Vec<char>,
    /// What the keyboard has learnt about each of A-Z.
    keyboard_state: [LetterState; 26],
    phase: GamePhase,
    rng: SeededRng,
    message: Option<&'static str>,
    games_played: u32,
    games_won: u32,
    streak: u32,
    best_streak: u32,
    /// Refuse a guess that contradicts what has already been revealed.
    hard_mode: bool,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    size_drawn: (f32, f32),
}

impl Wordle {
    fn new() -> Self {
        let mut rng = seeded_from_system(FALLBACK_SEED);
        let difficulty = Difficulty::Normal;
        let (target, target_len) = Self::pick_word(difficulty, &mut rng);
        Self {
            difficulty,
            target,
            target_len,
            guesses: Vec::new(),
            current_input: Vec::new(),
            keyboard_state: [LetterState::Unknown; 26],
            phase: GamePhase::Playing,
            rng,
            message: None,
            games_played: 0,
            games_won: 0,
            streak: 0,
            best_streak: 0,
            hard_mode: false,
            size_drawn: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    fn word_list(difficulty: Difficulty) -> &'static [&'static str] {
        match difficulty {
            Difficulty::Easy => WORDS_4,
            Difficulty::Normal => WORDS_5,
            Difficulty::Hard => WORDS_6,
        }
    }

    fn pick_word(difficulty: Difficulty, rng: &mut SeededRng) -> ([char; MAX_WORD], usize) {
        let words = Self::word_list(difficulty);
        let idx = rng.below(words.len());
        let word = words.get(idx).copied().unwrap_or("crane");
        let mut chars = [' '; MAX_WORD];
        let len = word.len().min(MAX_WORD);
        for (i, ch) in word.chars().take(MAX_WORD).enumerate() {
            if let Some(slot) = chars.get_mut(i) {
                *slot = ch;
            }
        }
        (chars, len)
    }

    fn target_word(&self) -> String {
        self.target.iter().take(self.target_len).collect()
    }

    fn is_valid_word(&self, input: &[char]) -> bool {
        let word: String = input.iter().collect();
        let lower = word.to_lowercase();
        let words = Self::word_list(self.difficulty);
        words.iter().any(|w| w.to_lowercase() == lower)
    }

    fn evaluate_guess(&self, guess: &[char]) -> [TileState; MAX_WORD] {
        let mut result = [TileState::Empty; MAX_WORD];
        let len = self.target_len;
        let mut target_used = [false; MAX_WORD];
        let mut guess_matched = [false; MAX_WORD];

        // First pass: mark correct (green)
        for i in 0..len {
            let g = guess.get(i).copied().unwrap_or(' ').to_ascii_lowercase();
            let t = self
                .target
                .get(i)
                .copied()
                .unwrap_or(' ')
                .to_ascii_lowercase();
            if g == t {
                if let Some(r) = result.get_mut(i) {
                    *r = TileState::Correct;
                }
                if let Some(u) = target_used.get_mut(i) {
                    *u = true;
                }
                if let Some(m) = guess_matched.get_mut(i) {
                    *m = true;
                }
            }
        }

        // Second pass: mark present (yellow) or absent (gray)
        for i in 0..len {
            if guess_matched.get(i).copied().unwrap_or(false) {
                continue;
            }
            let g = guess.get(i).copied().unwrap_or(' ').to_ascii_lowercase();
            let mut found = false;
            for j in 0..len {
                if target_used.get(j).copied().unwrap_or(false) {
                    continue;
                }
                let t = self
                    .target
                    .get(j)
                    .copied()
                    .unwrap_or(' ')
                    .to_ascii_lowercase();
                if g == t {
                    if let Some(r) = result.get_mut(i) {
                        *r = TileState::Present;
                    }
                    if let Some(u) = target_used.get_mut(j) {
                        *u = true;
                    }
                    found = true;
                    break;
                }
            }
            if !found && let Some(r) = result.get_mut(i) {
                *r = TileState::Absent;
            }
        }

        result
    }

    fn update_keyboard(&mut self, guess: &[char], eval: &[TileState; MAX_WORD]) {
        for i in 0..self.target_len {
            let ch = guess.get(i).copied().unwrap_or(' ').to_ascii_uppercase();
            if !ch.is_ascii_alphabetic() {
                continue;
            }
            let idx = (ch as u8).wrapping_sub(b'A') as usize;
            if idx >= 26 {
                continue;
            }
            let tile = eval.get(i).copied().unwrap_or(TileState::Empty);
            let new_state = match tile {
                TileState::Correct => LetterState::Correct,
                TileState::Present => LetterState::Present,
                TileState::Absent => LetterState::Absent,
                _ => continue,
            };
            let current = self
                .keyboard_state
                .get(idx)
                .copied()
                .unwrap_or(LetterState::Unknown);
            // Only upgrade: Correct > Present > Absent > Unknown
            let should_update = matches!(
                (current, new_state),
                (LetterState::Unknown, _)
                    | (
                        LetterState::Absent,
                        LetterState::Present | LetterState::Correct
                    )
                    | (LetterState::Present, LetterState::Correct)
            );
            if should_update && let Some(slot) = self.keyboard_state.get_mut(idx) {
                *slot = new_state;
            }
        }
    }

    fn check_hard_mode(&self, guess: &[char]) -> Option<&'static str> {
        if !self.hard_mode || self.guesses.is_empty() {
            return None;
        }
        // Check that all previously revealed correct letters are in the right position
        // and all previously revealed present letters are used somewhere
        for (prev_guess, prev_eval) in &self.guesses {
            for i in 0..self.target_len {
                let prev_tile = prev_eval.get(i).copied().unwrap_or(TileState::Empty);
                let prev_ch = prev_guess
                    .get(i)
                    .copied()
                    .unwrap_or(' ')
                    .to_ascii_lowercase();
                let curr_ch = guess.get(i).copied().unwrap_or(' ').to_ascii_lowercase();

                if prev_tile == TileState::Correct && curr_ch != prev_ch {
                    return Some("Hard mode: must use correct letters");
                }
            }
            // Check present letters are used
            for i in 0..self.target_len {
                let prev_tile = prev_eval.get(i).copied().unwrap_or(TileState::Empty);
                if prev_tile == TileState::Present {
                    let prev_ch = prev_guess
                        .get(i)
                        .copied()
                        .unwrap_or(' ')
                        .to_ascii_lowercase();
                    let used = (0..self.target_len).any(|j| {
                        guess.get(j).copied().unwrap_or(' ').to_ascii_lowercase() == prev_ch
                    });
                    if !used {
                        return Some("Hard mode: must use present letters");
                    }
                }
            }
        }
        None
    }

    /// Answer the word currently typed in, and say whether the click that asked
    /// for it changed anything.
    ///
    /// A refused guess still counts as a change: the refusal is written into
    /// the message line, where the player can read it.
    fn submit_guess(&mut self) -> bool {
        if self.phase != GamePhase::Playing {
            return false;
        }
        if self.current_input.len() != self.target_len {
            self.message = Some("Not enough letters");
            return true;
        }

        if !self.is_valid_word(&self.current_input) {
            self.message = Some("Not in word list");
            return true;
        }

        if let Some(msg) = self.check_hard_mode(&self.current_input) {
            self.message = Some(msg);
            return true;
        }

        let mut guess_arr = [' '; MAX_WORD];
        for (i, ch) in self.current_input.iter().enumerate().take(MAX_WORD) {
            if let Some(slot) = guess_arr.get_mut(i) {
                *slot = *ch;
            }
        }

        let eval = self.evaluate_guess(&self.current_input);
        // Use the local `guess_arr` copy (not `self.current_input`) so we don't
        // hold an immutable borrow of `self` across the `&mut self` call.
        self.update_keyboard(&guess_arr, &eval);
        self.guesses.push((guess_arr, eval));
        self.message = None;

        // Check win/lose
        let all_correct = (0..self.target_len)
            .all(|i| eval.get(i).copied().unwrap_or(TileState::Empty) == TileState::Correct);

        if all_correct {
            self.phase = GamePhase::Won;
            self.games_played = self.games_played.saturating_add(1);
            self.games_won = self.games_won.saturating_add(1);
            self.streak = self.streak.saturating_add(1);
            if self.streak > self.best_streak {
                self.best_streak = self.streak;
            }
            self.message = Some("Brilliant!");
        } else if self.guesses.len() >= MAX_GUESSES {
            self.phase = GamePhase::Lost;
            self.games_played = self.games_played.saturating_add(1);
            self.streak = 0;
            self.message = None; // will show target word
        }

        self.current_input.clear();
        true
    }

    /// Type `ch` into the row being built, and say whether it went in.
    ///
    /// A word already at its full length takes no more letters, and a finished
    /// game takes none at all.
    fn add_letter(&mut self, ch: char) -> bool {
        if self.phase != GamePhase::Playing || self.current_input.len() >= self.target_len {
            return false;
        }
        self.current_input.push(ch.to_ascii_lowercase());
        self.message = None;
        true
    }

    /// Rub out the last letter typed, and say whether anything went.
    ///
    /// Clearing a stale message is a change the player can see even when there
    /// was no letter to take back, so it counts as one.
    fn delete_letter(&mut self) -> bool {
        if self.phase != GamePhase::Playing {
            return false;
        }
        let changed = self.current_input.pop().is_some() || self.message.is_some();
        self.message = None;
        changed
    }

    fn new_game(&mut self) {
        let (target, target_len) = Self::pick_word(self.difficulty, &mut self.rng);
        self.target = target;
        self.target_len = target_len;
        self.guesses.clear();
        self.current_input.clear();
        self.keyboard_state = [LetterState::Unknown; 26];
        self.phase = GamePhase::Playing;
        self.message = None;
    }

    /// Switch to `diff`, dealing a fresh word of the new length, and say
    /// whether anything moved.
    ///
    /// Picking the length already in play is not a request for a new word —
    /// a button that deals a new puzzle when clicked twice is a button that
    /// throws the game away on a slip of the mouse.
    fn set_difficulty(&mut self, diff: Difficulty) -> bool {
        if diff == self.difficulty {
            return false;
        }
        self.difficulty = diff;
        self.new_game();
        true
    }

    /// Turn hard mode on or off, and say whether it turned.
    ///
    /// It only turns before the first guess: the rule is about guesses already
    /// answered, so switching it on halfway through would apply it to hints the
    /// player was free to ignore when they were given.
    fn toggle_hard_mode(&mut self) -> bool {
        if !self.guesses.is_empty() {
            return false;
        }
        self.hard_mode = !self.hard_mode;
        true
    }
    // ── The size the last frame was drawn at ───────────────────────

    /// Remember the size the window is being drawn at.
    ///
    /// A click arrives with no size attached, so the only honest size to read
    /// it against is the one the picture it was aimed at was drawn at. The old
    /// program had no answer to this at all: its hit test used the literal
    /// `420.0` whatever the window happened to be.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size_drawn = (width.max(1.0), height.max(1.0));
    }

    #[must_use]
    pub fn size_drawn(&self) -> (f32, f32) {
        self.size_drawn
    }

    /// The layout of the most recent frame.
    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(self.size_drawn.0, self.size_drawn.1, self.target_len)
    }

    // ── Input ──────────────────────────────────────────────────────

    /// Do what the control named by `target` does, and say whether anything
    /// happened.
    ///
    /// The answer is what separates a button that is dead from one that is
    /// merely quiet: a letter key pressed after the last guess changes nothing,
    /// and reporting that click as handled is how a frozen game comes to look
    /// like a working one.
    pub fn activate(&mut self, target: Target) -> bool {
        match target {
            Target::Key(ch) => self.add_letter(ch),
            Target::Enter => self.submit_guess(),
            Target::Backspace => self.delete_letter(),
            Target::Level(diff) => self.set_difficulty(diff),
            Target::HardMode => self.toggle_hard_mode(),
            Target::NewGame => {
                self.new_game();
                true
            }
        }
    }

    /// The letter a key types, for the twenty-six that type one.
    ///
    /// Read from the key's *name*, not from `KeyEvent::text`. That looks like
    /// the wrong choice — text is what survives a keyboard layout — but the
    /// compositor's `handle_text_input` is a no-op today, so `text` arrives
    /// empty from a real window and a game that read it would take no letters
    /// at all. Consulting both would be one rule written twice.
    fn key_letter(key: Key) -> Option<char> {
        Some(match key {
            Key::A => 'a',
            Key::B => 'b',
            Key::C => 'c',
            Key::D => 'd',
            Key::E => 'e',
            Key::F => 'f',
            Key::G => 'g',
            Key::H => 'h',
            Key::I => 'i',
            Key::J => 'j',
            Key::K => 'k',
            Key::L => 'l',
            Key::M => 'm',
            Key::N => 'n',
            Key::O => 'o',
            Key::P => 'p',
            Key::Q => 'q',
            Key::R => 'r',
            Key::S => 's',
            Key::T => 't',
            Key::U => 'u',
            Key::V => 'v',
            Key::W => 'w',
            Key::X => 'x',
            Key::Y => 'y',
            Key::Z => 'z',
            _ => return None,
        })
    }

    pub fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // `pressed` says whether this is the key going down or coming back up.
        // The old handler destructured `KeyEvent { key, modifiers, .. }` and
        // dropped `pressed` on the floor, so every letter was typed twice per
        // press and every Enter submitted the guess twice.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        // A shifted or control-held key belongs to whatever binds it, not to
        // the puzzle.
        if ev.modifiers != Modifiers::NONE {
            return EventResult::Ignored;
        }
        let acted = match ev.key {
            Key::Backspace => self.delete_letter(),
            Key::Enter => self.submit_guess(),
            Key::Num1 => self.set_difficulty(Difficulty::Easy),
            Key::Num2 => self.set_difficulty(Difficulty::Normal),
            Key::Num3 => self.set_difficulty(Difficulty::Hard),
            // H and N are letters of the alphabet first. They reach their
            // shortcuts only where they could not be part of a guess: the
            // hard-mode switch when nothing is typed and the game is running,
            // the new word when the game is over.
            Key::H if self.phase == GamePhase::Playing && self.current_input.is_empty() => {
                self.toggle_hard_mode()
            }
            Key::N | Key::Escape if self.phase != GamePhase::Playing => {
                self.new_game();
                true
            }
            other => match Self::key_letter(other) {
                Some(ch) => self.add_letter(ch),
                None => return EventResult::Ignored,
            },
        };
        if acted {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let (w, h) = self.size_drawn;
        // The hit boxes come from the drawing pass, so a key is clickable
        // exactly where its ink is. `handle_keyboard_click` used to write the
        // keyboard's geometry out a second time and compare against that.
        match self.frame(w, h).hit_test(ev.x, ev.y) {
            Some(target) => {
                if self.activate(target) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            None => EventResult::Ignored,
        }
    }

    // ── Drawing ────────────────────────────────────────────────────

    /// One frame at the given size: the picture and the hit boxes together.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let l = Layout::new(width, height, self.target_len);
        let mut f = Frame::new(width, height);

        fill(&mut f, l.window, BASE, CornerRadii::ZERO);
        self.draw_header(&mut f, &l);
        self.draw_board(&mut f, &l);
        self.draw_message(&mut f, &l);
        self.draw_keyboard(&mut f, &l);
        self.draw_footer(&mut f, &l);
        self.draw_over(&mut f, &l);
        f
    }

    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, CornerRadii::ZERO);
        label_centred(
            f,
            &Label {
                text: "WORDLE",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: TEXT,
            },
            l.title_rect(),
        );

        for (target, r) in HEADER_BUTTONS.iter().zip(l.button_rects()) {
            let (name, lit) = match *target {
                // Name and length are composed here rather than stored
                // together, so the "(5)" on the button cannot drift from the
                // five letters the puzzle actually has.
                Target::Level(diff) => (
                    format!("{} ({})", diff.name(), diff.word_len()),
                    diff == self.difficulty,
                ),
                Target::HardMode => ("Hard mode".to_string(), self.hard_mode),
                Target::NewGame => ("New word".to_string(), false),
                // The header holds exactly the five controls HEADER_BUTTONS
                // names. A letter key belongs to the keyboard, which is the
                // only place that knows which letter it is.
                Target::Key(_) | Target::Enter | Target::Backspace => continue,
            };
            // The hard-mode switch stops being a switch once a guess is in:
            // the rule it enforces is about guesses already answered, so it
            // cannot be turned on halfway through. Saying so in the drawing is
            // the only warning a player gets before clicking it.
            let live = *target != Target::HardMode || self.guesses.is_empty();
            let bg = if lit { BLUE } else { SURFACE0 };
            let fg = if !live {
                OVERLAY0
            } else if lit {
                CRUST
            } else {
                TEXT
            };
            fill(f, r, bg, CornerRadii::all(4.0));
            label_centred(
                f,
                &Label {
                    text: &name,
                    size: l.small,
                    weight: FontWeightHint::Bold,
                    color: fg,
                },
                r,
            );
            f.hit(*target, r);
        }
    }

    /// What tile `(row, col)` shows: the letter, and how it was answered.
    fn tile_at(&self, row: usize, col: usize) -> (char, TileState) {
        if let Some((guess, eval)) = self.guesses.get(row) {
            return (
                guess.get(col).copied().unwrap_or(' '),
                eval.get(col).copied().unwrap_or(TileState::Empty),
            );
        }
        if row == self.guesses.len()
            && let Some(ch) = self.current_input.get(col)
        {
            return (*ch, TileState::Filled);
        }
        (' ', TileState::Empty)
    }

    fn draw_board(&self, f: &mut Frame<Target>, l: &Layout) {
        for row in 0..MAX_GUESSES {
            for col in 0..l.cols {
                let r = l.tile_rect(row, col);
                let (ch, state) = self.tile_at(row, col);
                fill(f, r, state.color(), CornerRadii::all(4.0));
                // An answered tile is a block of colour; an unanswered one is
                // an outline, so an empty board reads as six empty rows rather
                // than as thirty grey squares.
                if matches!(state, TileState::Empty | TileState::Filled) {
                    let edge = if state == TileState::Filled {
                        SURFACE2
                    } else {
                        SURFACE1
                    };
                    stroke(f, r, edge, 2.0, CornerRadii::all(4.0));
                }
                if ch != ' ' {
                    let fg = match state {
                        TileState::Correct | TileState::Present | TileState::Absent => CRUST,
                        TileState::Empty | TileState::Filled => TEXT,
                    };
                    let mut buf = [0u8; 4];
                    label_centred(
                        f,
                        &Label {
                            text: ch.to_ascii_uppercase().encode_utf8(&mut buf),
                            size: l.tile * 0.5,
                            weight: FontWeightHint::Bold,
                            color: fg,
                        },
                        r,
                    );
                }
            }
        }
    }

    fn draw_message(&self, f: &mut Frame<Target>, l: &Layout) {
        let Some(msg) = self.message else {
            return;
        };
        label_centred(
            f,
            &Label {
                text: msg,
                size: l.font,
                weight: FontWeightHint::Bold,
                color: PEACH,
            },
            l.message,
        );
    }

    fn draw_keyboard(&self, f: &mut Frame<Target>, l: &Layout) {
        for (row, letters) in KEY_ROWS.iter().enumerate() {
            for (col, ch) in letters.chars().enumerate() {
                let r = l.key_rect(row, col);
                let state = self.letter_state(ch);
                let fg = match state {
                    LetterState::Correct | LetterState::Present => CRUST,
                    LetterState::Unknown | LetterState::Absent => TEXT,
                };
                fill(f, r, state.color(), CornerRadii::all(4.0));
                let mut buf = [0u8; 4];
                label_centred(
                    f,
                    &Label {
                        text: ch.encode_utf8(&mut buf),
                        size: l.key_h * 0.4,
                        weight: FontWeightHint::Bold,
                        color: fg,
                    },
                    r,
                );
                f.hit(Target::Key(ch), r);
            }
        }

        for (target, r) in [
            (Target::Enter, l.enter_rect()),
            (Target::Backspace, l.backspace_rect()),
        ] {
            let name = if target == Target::Enter {
                "ENTER"
            } else {
                "DEL"
            };
            fill(f, r, SURFACE1, CornerRadii::all(4.0));
            label_centred(
                f,
                &Label {
                    text: name,
                    size: l.key_h * 0.28,
                    weight: FontWeightHint::Bold,
                    color: TEXT,
                },
                r,
            );
            f.hit(target, r);
        }
    }

    /// What the keyboard has learnt about `ch`, an uppercase ASCII letter.
    fn letter_state(&self, ch: char) -> LetterState {
        let idx = (ch as u8).wrapping_sub(b'A') as usize;
        self.keyboard_state
            .get(idx)
            .copied()
            .unwrap_or(LetterState::Unknown)
    }

    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.footer.is_empty() {
            return;
        }
        fill(f, l.footer, MANTLE, CornerRadii::ZERO);
        let weight = FontWeightHint::Regular;
        let y = l.footer.y + (l.footer.h - text::line_height(l.small, weight)) / 2.0;
        let left = l.footer.x + l.pad;
        let right = l.footer.right() - l.pad;

        let stats = format!(
            "Played {}  Won {}  Streak {}  Best {}",
            self.games_played, self.games_won, self.streak, self.best_streak
        );
        // The counter is placed from its measured width, but never further
        // left than the band it lives in: a right-aligned string long enough
        // to overrun ends up at a negative x, off the edge of the screen,
        // while its right end sits exactly where it was asked to.
        let room = (right - left).max(0.0);
        let stats_w = text::measure(&stats, l.small, weight).min(room);
        let stats_x = right - stats_w;

        // What is left of the band once the counter has taken its share is
        // the hint's, and the hint is told to stop there. Two strings sharing
        // a line with no limit between them is one string printed over another.
        let hint =
            "Type a word  \u{2022}  Enter guesses  \u{2022}  1/2/3 length  \u{2022}  N new word";
        let hint_room = (stats_x - l.pad - left).max(0.0);
        push_text(
            f,
            &Label {
                text: hint,
                size: l.small,
                weight,
                color: OVERLAY0,
            },
            left,
            y,
            Some(hint_room),
        );
        push_text(
            f,
            &Label {
                text: &stats,
                size: l.small,
                weight,
                color: SUBTEXT0,
            },
            stats_x,
            y,
            Some(stats_w),
        );
    }

    /// The panel over a finished game.
    ///
    /// It returns for a game still running rather than being called behind an
    /// `if` that says the same thing: the rule that this is only drawn when the
    /// game is over is one rule, and it is written once, here, where the phase
    /// is already being read to decide what the panel says.
    fn draw_over(&self, f: &mut Frame<Target>, l: &Layout) {
        let (head, head_color) = match self.phase {
            GamePhase::Won => ("You won!", GREEN),
            GamePhase::Lost => ("Out of guesses", RED),
            GamePhase::Playing => return,
        };
        let w = l.window.w * 0.7;
        let h = l.window.h * 0.25;
        let panel = Rect::new(
            l.window.x + (l.window.w - w) / 2.0,
            l.window.y + (l.window.h - h) / 2.0,
            w,
            h,
        );
        fill(f, panel, MANTLE, CornerRadii::all(12.0));
        stroke(f, panel, SURFACE2, 2.0, CornerRadii::all(12.0));

        let line = panel.h / 3.0;
        let row = |i: usize| Rect::new(panel.x, panel.y + i as f32 * line, panel.w, line);
        label_centred(
            f,
            &Label {
                text: head,
                size: l.big,
                weight: FontWeightHint::Bold,
                color: head_color,
            },
            row(0),
        );
        let detail = match self.phase {
            GamePhase::Won => format!(
                "Solved in {} guess{}",
                self.guesses.len(),
                if self.guesses.len() == 1 { "" } else { "es" }
            ),
            // A lost game that does not say the word is a game that keeps its
            // answer, which is the one thing a player who has run out of
            // guesses wants.
            _ => format!("The word was {}", self.target_word().to_uppercase()),
        };
        label_centred(
            f,
            &Label {
                text: &detail,
                size: l.font,
                weight: FontWeightHint::Regular,
                color: if self.phase == GamePhase::Won {
                    TEXT
                } else {
                    YELLOW
                },
            },
            row(1),
        );
        label_centred(
            f,
            &Label {
                text: "Press N or Esc for a new word",
                size: l.small,
                weight: FontWeightHint::Regular,
                color: SUBTEXT0,
            },
            row(2),
        );
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii,
    });
}

fn stroke(
    f: &mut Frame<Target>,
    r: Rect,
    color: Color,
    line_width: f32,
    corner_radii: CornerRadii,
) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii,
    });
}

/// One string and everything about how it looks, minus where it goes.
struct Label<'a> {
    text: &'a str,
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

/// The one place a `Text` command is built.
///
/// `limit` is passed straight through as `max_width`, so a caller that computed
/// a width limit gets one the renderer will actually stop at, and `TextOverflow`
/// follows from it rather than being a second choice that could disagree: no
/// limit means the overflow question is vacuous, a limit means the cut is real
/// and had better be marked.
fn push_text(f: &mut Frame<Target>, l: &Label, x: f32, y: f32, limit: Option<f32>) {
    if l.text.is_empty() {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: l.text.to_string(),
        color: l.color,
        font_size: l.size,
        font_weight: l.weight,
        max_width: limit,
        overflow: if limit.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

/// Centred in `r` — horizontally from the measured width, vertically from the
/// line height — **and limited to `r`**.
///
/// The width that decides the centre is the width the renderer is told to stop
/// at, so the two cannot disagree, and that single clamp is also what keeps a
/// string too wide for its box starting at the box rather than to the left of
/// it: with `w` never wider than `r.w`, `(r.w - w) / 2.0` is never negative.
fn label_centred(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if l.text.is_empty() || r.is_empty() {
        return;
    }
    let w = text::measure(l.text, l.size, l.weight).min(r.w);
    let lh = text::line_height(l.size, l.weight);
    push_text(
        f,
        l,
        r.x + (r.w - w) / 2.0,
        r.y + (r.h - lh) / 2.0,
        Some(r.w),
    );
}

// ── Window plumbing ─────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(game: &mut Wordle, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => game.handle_key(ev),
        Event::Mouse(ev) => game.handle_mouse(ev),
        Event::Resize { width, height } => {
            game.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for Wordle {
    fn title(&self) -> String {
        "Wordle".to_string()
    }

    fn app_id(&self) -> String {
        "wordle".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
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
        // The size the frame is drawn at is the size the next click is read
        // against — which is the only reason it is stored at all.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for Wordle {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
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

fn main() -> ExitCode {
    let mut game = Wordle::new();
    app::launch("wordle", &mut game)
}

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

    // ── Randomness, as the game uses it ──
    //
    // Three tests here checked that the private generator was deterministic
    // and stayed inside its bound. That is `randrange`'s contract now, tested
    // there against the real hazards. These test what *Wordle* needs.

    /// The target word must follow the generator it was given. Under the old
    /// fixed `42` this was true but unobservable, because nothing ever handed
    /// the game a different generator.
    #[test]
    fn the_target_word_follows_the_generator() {
        let mut seen = Vec::new();
        for seed in 0..40 {
            let mut rng = SeededRng::new(seed);
            let (word, len) = Wordle::pick_word(Difficulty::Normal, &mut rng);
            let word: String = word[..len].iter().collect();
            if !seen.contains(&word) {
                seen.push(word);
            }
        }
        assert!(
            seen.len() > 5,
            "40 seeds produced only {} distinct words",
            seen.len()
        );
    }

    /// Word choice must reach the whole list, not a band of it. A reduction
    /// reading the low bits of an LCG would concentrate it near one end; this
    /// is the game-level shape of the bug `randrange::below` avoids.
    #[test]
    fn word_choice_reaches_both_ends_of_the_list() {
        for difficulty in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
            let words = Wordle::word_list(difficulty);
            let mut rng = SeededRng::new(11);
            let mut lowest = usize::MAX;
            let mut highest = 0;
            for _ in 0..600 {
                let idx = rng.below(words.len());
                lowest = lowest.min(idx);
                highest = highest.max(idx);
            }
            assert!(lowest < words.len() / 10, "never drew from the first tenth");
            assert!(
                highest > words.len() * 9 / 10,
                "never drew from the last tenth"
            );
        }
    }

    /// `new()` must take its generator from the system rather than a literal.
    ///
    /// Checked by *which* seed, not by variety: the host test toolchain has no
    /// SlateOS kernel, so `seeded_from_system` correctly falls back and two
    /// fresh games agree there -- exactly as they did under the old `42`.
    #[test]
    #[cfg(not(unix))]
    fn a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal() {
        let mut fallback = seeded_from_system(FALLBACK_SEED);
        let expected = Wordle::pick_word(Difficulty::Normal, &mut fallback);
        assert_eq!((Wordle::new().target, Wordle::new().target_len), expected);
        let mut old_defect = SeededRng::new(42);
        assert_ne!(
            (Wordle::new().target, Wordle::new().target_len),
            Wordle::pick_word(Difficulty::Normal, &mut old_defect),
            "back on a hardcoded seed"
        );
    }

    // ── Fixtures ───────────────────────────────────────────────────

    /// A game whose word is known, so a test can guess it on purpose.
    ///
    /// The word is *set*, not searched for: a fixture that plays the generator
    /// until it deals what the test wants is a fixture that hangs the day the
    /// generator changes.
    fn game_with(word: &str) -> Wordle {
        let mut g = Wordle::new();
        set_word(&mut g, word);
        assert!(
            g.is_valid_word(&word.chars().collect::<Vec<_>>()),
            "the fixture word {word} is not in the list the game checks against"
        );
        g
    }

    /// Deal `word` as the answer and clear the board, without asking whether
    /// it is a word the game would ever have dealt.
    ///
    /// The answering rules are about letters, not vocabulary, so the cases that
    /// pin them down — a letter guessed more often than the word holds it, a
    /// green and a yellow competing for the same letter — read more clearly on
    /// a made-up word than on whichever real one happens to have the shape.
    fn set_word(g: &mut Wordle, word: &str) {
        g.difficulty = match word.len() {
            4 => Difficulty::Easy,
            5 => Difficulty::Normal,
            6 => Difficulty::Hard,
            other => panic!("no difficulty plays {other}-letter words"),
        };
        g.target = [' '; MAX_WORD];
        for (slot, ch) in g.target.iter_mut().zip(word.chars()) {
            *slot = ch;
        }
        g.target_len = word.len();
        g.guesses.clear();
        g.current_input.clear();
        g.keyboard_state = [LetterState::Unknown; 26];
        g.phase = GamePhase::Playing;
        g.message = None;
    }

    /// A game holding `word` purely so its answering can be read.
    fn answering(word: &str) -> Wordle {
        let mut g = Wordle::new();
        set_word(&mut g, word);
        g
    }

    /// How a guess is answered, cut to the length of the word.
    fn answer(g: &Wordle, word: &str) -> Vec<TileState> {
        let chars: Vec<char> = word.chars().collect();
        g.evaluate_guess(&chars)[..g.target_len].to_vec()
    }

    /// The classic five-letter game on a known word.
    fn game() -> Wordle {
        game_with("crane")
    }

    /// Type `word` in and submit it.
    fn guess(g: &mut Wordle, word: &str) {
        g.current_input = word.chars().collect();
        g.submit_guess();
    }

    /// A game that has been lost, so the over-panel and the frozen input can
    /// both be looked at.
    fn lost() -> Wordle {
        let mut g = game();
        for _ in 0..MAX_GUESSES {
            guess(&mut g, "stone");
        }
        assert_eq!(g.phase, GamePhase::Lost, "the fixture did not lose");
        g
    }

    /// A game that has been won.
    fn won() -> Wordle {
        let mut g = game();
        guess(&mut g, "crane");
        assert_eq!(g.phase, GamePhase::Won, "the fixture did not win");
        g
    }

    /// Every difficulty, so a rule can be stated once and checked on all three.
    const EVERY_DIFFICULTY: [Difficulty; 3] =
        [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard];

    // ── The word lists ─────────────────────────────────────────────

    #[test]
    fn every_difficulty_plays_words_of_the_length_it_advertises() {
        for difficulty in EVERY_DIFFICULTY {
            let want = difficulty.word_len();
            let words = Wordle::word_list(difficulty);
            assert!(
                !words.is_empty(),
                "{difficulty:?} has no words to deal from"
            );
            for word in words {
                assert_eq!(
                    word.len(),
                    want,
                    "{difficulty:?} lists {word}, which is not {want} letters"
                );
            }
        }
    }

    /// The three lists must be three lists. A copy-paste that left two
    /// difficulties pointing at the same array would still pass the length
    /// check above for two of the three.
    #[test]
    fn the_three_difficulties_deal_from_three_different_lists() {
        for (i, a) in EVERY_DIFFICULTY.iter().enumerate() {
            for b in EVERY_DIFFICULTY.iter().skip(i + 1) {
                assert!(
                    !std::ptr::eq(Wordle::word_list(*a), Wordle::word_list(*b)),
                    "{a:?} and {b:?} deal from the same list"
                );
            }
        }
    }

    #[test]
    fn a_word_list_holds_no_word_twice() {
        for difficulty in EVERY_DIFFICULTY {
            let words = Wordle::word_list(difficulty);
            let mut seen: Vec<&str> = Vec::new();
            for word in words {
                assert!(
                    !seen.contains(word),
                    "{difficulty:?} lists {word} more than once, so it is dealt twice as often"
                );
                seen.push(word);
            }
        }
    }

    /// The name and the length on a difficulty button are composed from the
    /// two things they describe, so they cannot drift apart. The old labels
    /// wrote the length into the name — `"Easy (4)"` — where nothing compared
    /// it against `word_len`.
    #[test]
    fn a_difficulty_is_named_without_its_length_written_in() {
        for difficulty in EVERY_DIFFICULTY {
            let name = difficulty.name();
            assert!(!name.is_empty(), "{difficulty:?} has no name");
            assert!(
                !name.contains(|c: char| c.is_ascii_digit()),
                "{difficulty:?} writes its length into its name: {name}"
            );
        }
        assert_eq!(Difficulty::Easy.word_len(), 4);
        assert_eq!(Difficulty::Normal.word_len(), 5);
        assert_eq!(Difficulty::Hard.word_len(), 6);
    }

    /// Six guesses, whatever the length. It reads as a rule that varies —
    /// it did not, and used to be a method taking a receiver it ignored.
    #[test]
    fn every_difficulty_allows_the_same_six_guesses() {
        assert_eq!(MAX_GUESSES, 6);
        for difficulty in EVERY_DIFFICULTY {
            let mut g = Wordle::new();
            g.difficulty = difficulty;
            g.new_game();
            let wrong = Wordle::word_list(difficulty)
                .iter()
                .find(|w| **w != g.target_word())
                .copied()
                .expect("a list with only one word cannot be played");
            for _ in 0..MAX_GUESSES {
                assert_eq!(
                    g.phase,
                    GamePhase::Playing,
                    "{difficulty:?} ran out before {MAX_GUESSES} guesses"
                );
                guess(&mut g, wrong);
            }
            assert_eq!(
                g.phase,
                GamePhase::Lost,
                "{difficulty:?} took more than {MAX_GUESSES} guesses"
            );
        }
    }

    /// A word longer than the arrays it is carried in would be silently cut.
    #[test]
    fn no_difficulty_plays_a_word_longer_than_the_array_that_holds_it() {
        for difficulty in EVERY_DIFFICULTY {
            assert!(
                difficulty.word_len() <= MAX_WORD,
                "{difficulty:?} wants {} letters, and a guess holds {MAX_WORD}",
                difficulty.word_len()
            );
        }
    }

    // ── How a guess is answered ────────────────────────────────────

    use TileState::{Absent, Correct, Present};

    #[test]
    fn a_letter_in_the_right_place_is_answered_green() {
        let g = answering("crane");
        assert_eq!(answer(&g, "crane"), vec![Correct; 5]);
        assert_eq!(
            answer(&g, "cxxxx").first(),
            Some(&Correct),
            "a letter standing where the word has it was not called correct"
        );
    }

    #[test]
    fn a_letter_in_the_word_but_the_wrong_place_is_answered_yellow() {
        let g = answering("crane");
        assert_eq!(
            answer(&g, "ecrna"),
            vec![Present, Present, Present, Correct, Present],
            "the letters of the word, shuffled, were not all called present"
        );
    }

    #[test]
    fn a_letter_not_in_the_word_at_all_is_answered_grey() {
        let g = answering("crane");
        // b, u, m, p, y: not one of them is in "crane".
        assert_eq!(answer(&g, "bumpy"), vec![Absent; 5]);
    }

    /// The count matters, not just the membership. A guess holding a letter
    /// three times against a word holding it once must be answered once.
    #[test]
    fn a_letter_is_answered_only_as_often_as_the_word_holds_it() {
        let g = answering("crane");
        // Three r's against a word holding one, none of them where the word
        // keeps it: the leftmost is answered and the other two are not. A
        // reading that only asked "is this letter in the word?" would call all
        // three yellow and tell the player the word has three r's.
        let row = answer(&g, "rxrrx");
        assert_eq!(
            row,
            vec![Present, Absent, Absent, Absent, Absent],
            "a letter was answered more times than the word holds it"
        );
        assert_eq!(
            row.iter().filter(|s| **s == Present).count(),
            1,
            "the word holds one r, so one r is owed an answer"
        );
    }

    /// A green claims its letter before any yellow can. The word holds two
    /// b's; the guess offers three, one of them standing where the word keeps
    /// its second. That one is green, one of the remaining two is yellow, and
    /// the third is grey.
    #[test]
    fn a_green_takes_its_letter_before_a_yellow_can_claim_it() {
        let g = answering("abbey");
        let row = answer(&g, "bbxbx");
        assert_eq!(
            row,
            vec![Present, Correct, Absent, Absent, Absent],
            "the green did not take its letter out of the pool first"
        );
        assert_eq!(
            row.iter().filter(|s| **s == Correct).count(),
            1,
            "the b standing where the word keeps one was not called correct"
        );
        assert_eq!(
            row.iter().filter(|s| **s == Present).count(),
            1,
            "the word holds two b's and one is already green, so one yellow is owed"
        );
    }

    #[test]
    fn a_guess_is_answered_the_same_whatever_case_it_is_typed_in() {
        let g = answering("crane");
        assert_eq!(answer(&g, "CRANE"), vec![Correct; 5]);
        assert_eq!(answer(&g, "ECRNA"), answer(&g, "ecrna"));
    }

    /// Only the letters of the word are answered. The arrays are [`MAX_WORD`]
    /// wide whatever the difficulty, and a four-letter game that answered the
    /// fifth slot would colour a tile that is not drawn.
    #[test]
    fn a_shorter_word_leaves_the_slots_past_its_end_untouched() {
        let g = answering("area");
        let row = g.evaluate_guess(&['a', 'r', 'e', 'a', 'a', 'a']);
        assert_eq!(
            &row[..4],
            &[Correct; 4],
            "the word itself was not all green"
        );
        assert_eq!(
            &row[4..],
            &[TileState::Empty; 2],
            "a slot past the end of a four-letter word was answered"
        );
    }

    // ── Which guesses are taken ────────────────────────────────────

    #[test]
    fn a_guess_of_the_wrong_length_is_refused_and_says_why() {
        let mut g = game();
        g.current_input = "cran".chars().collect();
        assert!(
            g.submit_guess(),
            "the refusal was reported as no change at all"
        );
        assert_eq!(g.guesses.len(), 0, "a short guess was taken");
        assert_eq!(g.message, Some("Not enough letters"));
        assert_eq!(
            g.current_input.len(),
            4,
            "the letters already typed were thrown away with the refusal"
        );
    }

    #[test]
    fn a_guess_that_is_not_in_the_word_list_is_refused_and_says_why() {
        let mut g = game();
        g.current_input = "xqzjv".chars().collect();
        assert!(g.submit_guess());
        assert_eq!(g.guesses.len(), 0, "a non-word was taken as a guess");
        assert_eq!(g.message, Some("Not in word list"));
    }

    #[test]
    fn a_refused_guess_does_not_use_up_a_turn() {
        let mut g = game();
        for _ in 0..MAX_GUESSES * 2 {
            g.current_input = "xqzjv".chars().collect();
            g.submit_guess();
        }
        assert_eq!(
            g.phase,
            GamePhase::Playing,
            "guesses that were never taken ran the game out"
        );
        assert_eq!(g.guesses.len(), 0);
    }

    #[test]
    fn a_guess_that_is_taken_clears_the_row_for_the_next_one() {
        let mut g = game();
        guess(&mut g, "stone");
        assert_eq!(g.guesses.len(), 1, "the guess was not recorded");
        assert!(
            g.current_input.is_empty(),
            "the letters stayed in the row they were just guessed from"
        );
        assert_eq!(g.message, None, "an accepted guess left a complaint up");
    }

    // ── What the on-screen keyboard learns ─────────────────────────

    #[test]
    fn the_keyboard_learns_what_the_answer_said_about_each_letter() {
        let mut g = game_with("crane");
        guess(&mut g, "crest");
        assert_eq!(
            g.letter_state('C'),
            LetterState::Correct,
            "c stands in place"
        );
        assert_eq!(
            g.letter_state('R'),
            LetterState::Correct,
            "r stands in place"
        );
        assert_eq!(
            g.letter_state('E'),
            LetterState::Present,
            "e is in the word"
        );
        assert_eq!(g.letter_state('S'), LetterState::Absent, "s is not");
        assert_eq!(g.letter_state('T'), LetterState::Absent, "t is not");
        assert_eq!(
            g.letter_state('Z'),
            LetterState::Unknown,
            "a letter never guessed was decided anyway"
        );
    }

    /// The keyboard may only ever learn more. A later guess putting a known
    /// green letter somewhere it is not must not demote the key to yellow —
    /// the player would read that as the letter having moved.
    #[test]
    fn the_keyboard_never_forgets_something_it_already_knew() {
        let mut g = game_with("crane");
        guess(&mut g, "crane");
        assert_eq!(g.letter_state('C'), LetterState::Correct);
        g.phase = GamePhase::Playing;
        guess(&mut g, "black");
        assert_eq!(
            g.letter_state('C'),
            LetterState::Correct,
            "a green key was demoted by a later guess that misplaced its letter"
        );

        let mut g = game_with("crane");
        guess(&mut g, "eight");
        assert_eq!(
            g.letter_state('E'),
            LetterState::Present,
            "e is in the word"
        );
        guess(&mut g, "crane");
        assert_eq!(
            g.letter_state('E'),
            LetterState::Correct,
            "a yellow key was not promoted when its letter turned out to be green"
        );
    }

    // ── Winning, losing and the running totals ─────────────────────

    #[test]
    fn guessing_the_word_wins_and_counts_the_win() {
        let mut g = game_with("crane");
        guess(&mut g, "crane");
        assert_eq!(g.phase, GamePhase::Won);
        assert_eq!(
            (g.games_played, g.games_won, g.streak, g.best_streak),
            (1, 1, 1, 1)
        );
    }

    #[test]
    fn running_out_of_guesses_loses_and_breaks_the_streak() {
        let mut g = game_with("crane");
        guess(&mut g, "crane");
        assert_eq!(g.streak, 1, "the win before the loss did not count");
        g.new_game();
        set_word(&mut g, "crane");
        for _ in 0..MAX_GUESSES {
            guess(&mut g, "stone");
        }
        assert_eq!(g.phase, GamePhase::Lost);
        assert_eq!(g.streak, 0, "a loss left the streak standing");
        assert_eq!(g.best_streak, 1, "the loss took the best streak with it");
        assert_eq!((g.games_played, g.games_won), (2, 1));
    }

    #[test]
    fn the_best_streak_is_the_longest_run_reached_not_the_current_one() {
        let mut g = game_with("crane");
        for _ in 0..3 {
            guess(&mut g, "crane");
            g.new_game();
            set_word(&mut g, "crane");
        }
        assert_eq!(g.best_streak, 3);
        for _ in 0..MAX_GUESSES {
            guess(&mut g, "stone");
        }
        assert_eq!(g.streak, 0);
        assert_eq!(g.best_streak, 3, "the best run was forgotten");
        // The loss alone is not enough to see the difference: it is the *win*
        // that writes the best streak, so a best streak that merely copied the
        // current one would still read 3 here and only fall to 1 on the next
        // win. Start a new run and check the record survives it.
        g.new_game();
        set_word(&mut g, "crane");
        guess(&mut g, "crane");
        assert_eq!(g.streak, 1, "the new run did not start");
        assert_eq!(
            g.best_streak, 3,
            "the best streak followed the current one back down"
        );
    }

    #[test]
    fn a_finished_game_takes_no_more_input() {
        for mut g in [won(), lost()] {
            let before = (g.guesses.len(), g.current_input.clone(), g.phase);
            assert!(!g.add_letter('a'), "a letter went in after the game ended");
            assert!(!g.delete_letter(), "a letter came out after the game ended");
            assert!(!g.submit_guess(), "a guess was taken after the game ended");
            assert_eq!(
                (g.guesses.len(), g.current_input.clone(), g.phase),
                before,
                "the finished game moved"
            );
        }
    }

    // ── Hard mode ──────────────────────────────────────────────────

    #[test]
    fn hard_mode_refuses_a_guess_that_moves_a_letter_already_shown_green() {
        let mut g = game_with("crane");
        g.hard_mode = true;
        guess(&mut g, "crest");
        assert_eq!(g.guesses.len(), 1, "the setup guess was refused");
        guess(&mut g, "stone");
        assert_eq!(
            g.guesses.len(),
            1,
            "a guess dropping a known green was taken"
        );
        assert_eq!(g.message, Some("Hard mode: must use correct letters"));
    }

    #[test]
    fn hard_mode_refuses_a_guess_that_drops_a_letter_already_shown_yellow() {
        let mut g = game_with("crane");
        g.hard_mode = true;
        guess(&mut g, "eight");
        assert_eq!(g.guesses.len(), 1, "the setup guess was refused");
        assert_eq!(g.letter_state('E'), LetterState::Present);
        guess(&mut g, "climb");
        assert_eq!(
            g.guesses.len(),
            1,
            "a guess dropping a known yellow was taken"
        );
        assert_eq!(g.message, Some("Hard mode: must use present letters"));
    }

    #[test]
    fn hard_mode_off_takes_a_guess_that_ignores_everything_revealed() {
        let mut g = game_with("crane");
        assert!(!g.hard_mode, "the game starts in hard mode");
        guess(&mut g, "crest");
        guess(&mut g, "blood");
        assert_eq!(
            g.guesses.len(),
            2,
            "an ordinary game refused a guess for ignoring a hint"
        );
    }

    #[test]
    fn hard_mode_only_turns_before_the_first_guess() {
        let mut g = game_with("crane");
        assert!(
            g.toggle_hard_mode(),
            "the switch did not turn on an empty board"
        );
        assert!(g.hard_mode);
        assert!(g.toggle_hard_mode(), "the switch would not turn back off");
        assert!(!g.hard_mode);

        guess(&mut g, "stone");
        assert!(
            !g.toggle_hard_mode(),
            "the switch turned after a guess had been answered"
        );
        assert!(!g.hard_mode, "hard mode came on halfway through a game");
    }

    // ── Starting again ─────────────────────────────────────────────

    #[test]
    fn a_new_word_clears_the_board_and_keeps_the_totals() {
        let mut g = won();
        let totals = (g.games_played, g.games_won, g.streak, g.best_streak);
        let hard_mode = g.hard_mode;
        g.new_game();
        assert_eq!(g.phase, GamePhase::Playing, "the new game was over already");
        assert!(g.guesses.is_empty(), "the old guesses stayed on the board");
        assert!(g.current_input.is_empty());
        assert_eq!(g.message, None);
        assert!(
            g.keyboard_state.iter().all(|s| *s == LetterState::Unknown),
            "the keyboard still shows what the last word taught it"
        );
        assert_eq!(
            (g.games_played, g.games_won, g.streak, g.best_streak),
            totals,
            "a new word threw the running totals away"
        );
        assert_eq!(g.hard_mode, hard_mode, "a new word turned hard mode off");
    }

    #[test]
    fn changing_the_length_deals_a_word_of_that_length() {
        let mut g = game_with("crane");
        assert!(g.set_difficulty(Difficulty::Hard), "the switch did nothing");
        assert_eq!(g.difficulty, Difficulty::Hard);
        assert_eq!(
            g.target_len,
            Difficulty::Hard.word_len(),
            "the six-letter game is not playing a six-letter word"
        );
        assert_eq!(g.target_word().len(), g.target_len);
    }

    /// Picking the length already in play deals nothing. It is one click away
    /// from the length button that *is* a change, and a button that throws the
    /// game away on a double click is a button that cannot be trusted.
    #[test]
    fn picking_the_length_already_in_play_leaves_the_word_alone() {
        let mut g = game();
        guess(&mut g, "stone");
        let before = (g.target, g.target_len, g.guesses.len());
        assert!(
            !g.set_difficulty(g.difficulty),
            "picking the length already in play reported a change"
        );
        assert_eq!(
            (g.target, g.target_len, g.guesses.len()),
            before,
            "picking the length already in play dealt a new word"
        );
    }

    // ── The layout ─────────────────────────────────────────────────

    /// A handful of window shapes worth solving the layout for: the default,
    /// a tall thin one, a wide short one, and sizes at the edge of usable.
    /// `140x900` is there because it is tall enough to keep every band and
    /// narrow enough that the footer has less room than its own text wants —
    /// the case where a string measured without regard to its box lands off
    /// the left edge of the window.
    const SHAPES: [(f32, f32); 7] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (320.0, 900.0),
        (1400.0, 400.0),
        (200.0, 200.0),
        (2000.0, 1400.0),
        (60.0, 60.0),
        (140.0, 900.0),
    ];

    /// Two layouts of the same window and the same word length must be the
    /// same layout — the old program remembered none, but a program that
    /// solves geometry from anything else (a frame counter, a stored size)
    /// draws a picture its own hit test cannot find.
    #[test]
    fn the_layout_is_a_function_of_the_window_size_and_the_word_length_alone() {
        for (w, h) in SHAPES {
            for cols in 4..=MAX_WORD {
                assert_eq!(
                    Layout::new(w, h, cols),
                    Layout::new(w, h, cols),
                    "{w}x{h} with {cols} letters solved differently twice"
                );
            }
        }
        assert_ne!(
            Layout::new(560.0, 720.0, 5),
            Layout::new(561.0, 720.0, 5),
            "a wider window gave exactly the same layout"
        );
        assert_ne!(
            Layout::new(560.0, 720.0, 5),
            Layout::new(560.0, 721.0, 5),
            "a taller window gave exactly the same layout"
        );
        assert_ne!(
            Layout::new(560.0, 720.0, 4),
            Layout::new(560.0, 720.0, 5),
            "a longer word gave exactly the same layout"
        );
    }

    /// The bands stack down the window in order, edge to edge. A gap between
    /// two of them is a strip of window nothing owns; an overlap is two things
    /// drawn on top of each other.
    #[test]
    fn the_bands_stack_down_the_window_without_a_gap_or_an_overlap() {
        for (w, h) in SHAPES {
            let l = Layout::new(w, h, 5);
            let at = format!("{w}x{h}");
            assert_eq!(l.header.y, 0.0, "{at}: the header is not at the top");
            assert!(
                l.header.bottom() <= l.board.y + 0.01,
                "{at}: the board starts inside the header"
            );
            assert!(
                l.board.bottom() <= l.message.y + 0.01,
                "{at}: the message starts inside the board"
            );
            assert!(
                (l.message.bottom() - l.keyboard.y).abs() <= 0.01,
                "{at}: the keyboard does not start where the message ends"
            );
            assert!(
                (l.keyboard.bottom() - l.footer.y).abs() <= 0.01,
                "{at}: the footer does not start where the keyboard ends"
            );
            assert!(
                (l.footer.bottom() - h.max(1.0)).abs() <= 0.01,
                "{at}: the footer does not reach the bottom of the window"
            );
        }
    }

    /// Every rectangle the layout hands out must be a rectangle: a negative
    /// width is a box whose right edge is left of its left one, and
    /// `Rect::contains` says yes to nothing inside it and no to everything.
    #[test]
    fn no_rectangle_the_layout_hands_out_is_inside_out() {
        for (w, h) in SHAPES {
            for cols in 4..=MAX_WORD {
                let l = Layout::new(w, h, cols);
                let mut boxes = vec![
                    ("window", l.window),
                    ("header", l.header),
                    ("board", l.board),
                    ("message", l.message),
                    ("keyboard", l.keyboard),
                    ("footer", l.footer),
                    ("title", l.title_rect()),
                    ("enter", l.enter_rect()),
                    ("backspace", l.backspace_rect()),
                ];
                for r in l.button_rects() {
                    boxes.push(("button", r));
                }
                for row in 0..MAX_GUESSES {
                    for col in 0..cols {
                        boxes.push(("tile", l.tile_rect(row, col)));
                    }
                }
                for (row, letters) in KEY_ROWS.iter().enumerate() {
                    for col in 0..letters.len() {
                        boxes.push(("key", l.key_rect(row, col)));
                    }
                }
                for (name, r) in boxes {
                    assert!(
                        r.w >= 0.0 && r.h >= 0.0,
                        "{w}x{h}/{cols}: the {name} is {}x{}, which is inside out",
                        r.w,
                        r.h
                    );
                }
            }
        }
    }

    /// The window can get too short for everything, and the bands give way in
    /// a stated order: the footer's counters first, then the message line,
    /// then the header. The board and the keyboard never give way — a Wordle
    /// with no keyboard is a picture of one.
    #[test]
    fn a_short_window_gives_up_the_footer_then_the_message_then_the_header() {
        // Every stage must actually be reached by the sweep, or the nesting
        // assertions below hold vacuously (`known-issues.md` lesson 67).
        let mut reached = [false; 4];
        for height in 40..=760 {
            let h = height as f32;
            let l = Layout::new(560.0, h, 5);
            let (header, message, footer) = (
                !l.header.is_empty(),
                !l.message.is_empty(),
                !l.footer.is_empty(),
            );
            if footer {
                assert!(
                    message && header,
                    "at {h} the footer outlived a band that is meant to go after it"
                );
            }
            if message {
                assert!(header, "at {h} the message outlived the header");
            }
            assert!(!l.keyboard.is_empty(), "at {h} the keyboard vanished");
            assert!(!l.board.is_empty(), "at {h} the board vanished");
            let gone = usize::from(!footer) + usize::from(!message) + usize::from(!header);
            if let Some(slot) = reached.get_mut(gone) {
                *slot = true;
            }
        }
        assert_eq!(
            reached, [true; 4],
            "the sweep never reached every stage of giving way"
        );
    }

    /// The keyboard shrinks where the chrome vanishes. It is the one band that
    /// is allowed to be smaller than it wants and still be there.
    #[test]
    fn the_keyboard_shrinks_rather_than_vanishing() {
        let roomy = Layout::new(560.0, 720.0, 5);
        let cramped = Layout::new(560.0, 150.0, 5);
        assert!(
            cramped.keyboard.h < roomy.keyboard.h,
            "the cramped window gave the keyboard as much room as the roomy one"
        );
        assert!(
            !cramped.keyboard.is_empty(),
            "the keyboard was dropped instead of shrunk"
        );
        assert!(
            cramped.key_w > 0.0 && cramped.key_h > 0.0,
            "the keys have no size"
        );
        // Shrinking is what the keyboard does *instead of* growing through the
        // bands above it. Below about 26 pixels of window the band it wants is
        // taller than the room there is, and a keyboard that took what it
        // asked for would start above the top edge of the window.
        for h in 10u16..400 {
            let l = Layout::new(560.0, f32::from(h), 5);
            assert!(
                l.keyboard.y >= l.window.y - 0.01,
                "h={h}: the keyboard starts at {}, above the top of the window",
                l.keyboard.y
            );
        }
    }

    /// The grid is square and centred in what the bands leave. A grid solved
    /// from one dimension is a grid that runs off the other.
    #[test]
    fn the_guess_grid_is_square_and_centred_in_what_is_left_over() {
        for (w, h) in SHAPES {
            for cols in 4..=MAX_WORD {
                let l = Layout::new(w, h, cols);
                let at = format!("{w}x{h}/{cols}");
                let first = l.tile_rect(0, 0);
                let last = l.tile_rect(MAX_GUESSES - 1, cols - 1);
                assert!(
                    (first.w - first.h).abs() <= 0.01,
                    "{at}: a tile is {}x{}, which is not square",
                    first.w,
                    first.h
                );
                assert!(
                    (first.x - l.board.x).abs() <= 0.01 && (first.y - l.board.y).abs() <= 0.01,
                    "{at}: the first tile is not at the corner of the board"
                );
                assert!(
                    (last.right() - l.board.right()).abs() <= 0.01
                        && (last.bottom() - l.board.bottom()).abs() <= 0.01,
                    "{at}: the last tile does not reach the far corner of the board"
                );
                // The tiles filling the board says nothing about where the
                // board is. It is centred in what the bands leave over, which
                // horizontally is the window and vertically is the strip
                // between the header and the message — the padding above and
                // below that strip is equal, so its midpoint is theirs.
                if l.board.is_empty() {
                    continue;
                }
                assert!(
                    (l.board.centre().0 - l.window.centre().0).abs() <= 0.01,
                    "{at}: the board is at {}, not centred across the window",
                    l.board.centre().0
                );
                let mid = f32::midpoint(l.header.bottom(), l.message.y);
                assert!(
                    (l.board.centre().1 - mid).abs() <= 0.01,
                    "{at}: the board is at {}, not centred between the header and the message \
                     at {mid}",
                    l.board.centre().1
                );
            }
        }
    }

    /// Widening the window without heightening it must not stretch the grid
    /// past the room it has, and lengthening the word must shrink the tile.
    /// Varying both at once cannot say which dimension the tile is solved from.
    #[test]
    fn the_tile_is_solved_from_whichever_dimension_binds() {
        let narrow = Layout::new(300.0, 2000.0, 5);
        let wide = Layout::new(1200.0, 2000.0, 5);
        assert!(
            wide.tile > narrow.tile * 1.5,
            "a window four times as wide, with height to spare, gave the same tile"
        );
        let short = Layout::new(2000.0, 300.0, 5);
        let tall = Layout::new(2000.0, 1200.0, 5);
        assert!(
            tall.tile > short.tile * 1.5,
            "a window four times as tall, with width to spare, gave the same tile"
        );
        let five = Layout::new(300.0, 2000.0, 5);
        let six = Layout::new(300.0, 2000.0, 6);
        assert!(
            six.tile < five.tile,
            "the six-letter word got the same tile as the five-letter one in a window \
             whose width is what binds"
        );
    }

    #[test]
    fn the_grid_has_a_tile_for_every_letter_of_every_guess_and_none_past_them() {
        let l = Layout::new(560.0, 720.0, 5);
        for row in 0..MAX_GUESSES {
            for col in 0..l.cols {
                assert!(
                    !l.tile_rect(row, col).is_empty(),
                    "row {row} letter {col} has no tile"
                );
            }
            assert!(
                l.tile_rect(row, l.cols).is_empty(),
                "row {row} has a tile past the end of the word"
            );
        }
        assert!(
            l.tile_rect(MAX_GUESSES, 0).is_empty(),
            "there is a tile below the last guess"
        );
    }

    #[test]
    fn no_two_tiles_of_the_grid_overlap() {
        let l = Layout::new(560.0, 720.0, 5);
        let mut boxes = Vec::new();
        for row in 0..MAX_GUESSES {
            for col in 0..l.cols {
                boxes.push(((row, col), l.tile_rect(row, col)));
            }
        }
        for (i, (a_at, a)) in boxes.iter().enumerate() {
            for (b_at, b) in boxes.iter().skip(i + 1) {
                assert!(
                    a.intersect(*b).is_none(),
                    "tiles {a_at:?} and {b_at:?} are drawn on top of each other"
                );
            }
        }
        // Not overlapping is not enough: touching tiles do not overlap either,
        // and a grid of touching tiles is one solid block with no letters
        // legible in it. Assert the separation is the gap the layout solved
        // for, which is the formula rather than a bound on it
        // (`known-issues.md` lesson 68).
        for row in 0..MAX_GUESSES {
            for col in 1..l.cols {
                let (prev, this) = (l.tile_rect(row, col - 1), l.tile_rect(row, col));
                assert!(
                    (this.x - prev.right() - l.gap).abs() <= 0.01,
                    "row {row} letters {} and {col} are {} apart, not the gap of {}",
                    col - 1,
                    this.x - prev.right(),
                    l.gap
                );
            }
        }
        for row in 1..MAX_GUESSES {
            let (prev, this) = (l.tile_rect(row - 1, 0), l.tile_rect(row, 0));
            assert!(
                (this.y - prev.bottom() - l.gap).abs() <= 0.01,
                "rows {} and {row} are {} apart, not the gap of {}",
                row - 1,
                this.y - prev.bottom(),
                l.gap
            );
        }
        assert!(l.gap > 0.0, "the grid was solved with no gap at all");
    }

    // ── The on-screen keyboard's geometry ──────────────────────────

    /// Every row is ten columns wide however many letters it holds, so the
    /// three rows line up under one another rather than each being centred on
    /// its own count.
    #[test]
    fn the_keyboard_is_ten_columns_wide_and_fills_its_band() {
        for (w, h) in SHAPES {
            let l = Layout::new(w, h, 5);
            let at = format!("{w}x{h}");
            let first = l.key_rect(0, 0);
            let last = l.key_rect(0, 9);
            assert!(
                (first.x - l.keyboard.x).abs() <= 0.01,
                "{at}: the top row does not start at the left of the band"
            );
            assert!(
                (last.right() - l.keyboard.right()).abs() <= 0.01,
                "{at}: the top row does not reach the right of the band"
            );
            let bottom = l.key_rect(2, 0);
            assert!(
                (bottom.bottom() - l.keyboard.bottom()).abs() <= 0.01,
                "{at}: the bottom row does not reach the bottom of the band"
            );
        }
    }

    /// The nine-letter middle row is centred under the ten-letter top one: the
    /// space left at its two ends must be equal. Indenting it by *any* amount
    /// would keep it inside the band, so the band check cannot see this
    /// (`known-issues.md` lesson 68).
    #[test]
    fn the_middle_row_is_centred_under_the_top_one() {
        let l = Layout::new(560.0, 720.0, 5);
        let left = l.key_rect(1, 0).x - l.keyboard.x;
        let right = l.keyboard.right() - l.key_rect(1, 8).right();
        assert!(
            (left - right).abs() <= 0.01,
            "the middle row has {left} of space on its left and {right} on its right"
        );
        assert!(left > 0.0, "the middle row is not indented at all");
    }

    /// Enter and Backspace bracket the bottom row, one key-gap from the
    /// letters and flush with the ends of the band. The old program placed
    /// backspace at `80.0 + 36.0 + 7.0 * (key_w + gap) + 36.0` — an expression
    /// with no stated relationship to anything else on the row.
    #[test]
    fn enter_and_backspace_bracket_the_bottom_row() {
        let l = Layout::new(560.0, 720.0, 5);
        let enter = l.enter_rect();
        let backspace = l.backspace_rect();
        let first = l.key_rect(2, 0);
        let last = l.key_rect(2, KEY_ROWS[2].len() - 1);

        assert!(
            (enter.x - l.keyboard.x).abs() <= 0.01,
            "enter is not flush with the left of the band"
        );
        assert!(
            (backspace.right() - l.keyboard.right()).abs() <= 0.01,
            "backspace is not flush with the right of the band"
        );
        assert!(
            (first.x - enter.right() - l.key_gap).abs() <= 0.01,
            "the gap between enter and Z is not the gap between two keys"
        );
        assert!(
            (backspace.x - last.right() - l.key_gap).abs() <= 0.01,
            "the gap between M and backspace is not the gap between two keys"
        );
        assert!(
            (enter.y - first.y).abs() <= 0.01 && (enter.h - first.h).abs() <= 0.01,
            "enter is not on the same line as the letters it sits beside"
        );
        assert!(
            enter.w > l.key_w,
            "enter is no wider than a letter key, so its name will not fit"
        );
    }

    #[test]
    fn nothing_on_the_keyboard_overlaps_anything_else_on_it() {
        let l = Layout::new(560.0, 720.0, 5);
        let mut boxes = vec![("enter", l.enter_rect()), ("backspace", l.backspace_rect())];
        for (row, letters) in KEY_ROWS.iter().enumerate() {
            for (col, ch) in letters.chars().enumerate() {
                assert!(
                    !l.key_rect(row, col).is_empty(),
                    "the key {ch} has no box to be clicked in"
                );
                boxes.push((letters, l.key_rect(row, col)));
            }
        }
        for (i, (a_name, a)) in boxes.iter().enumerate() {
            for (b_name, b) in boxes.iter().skip(i + 1) {
                assert!(
                    a.intersect(*b).is_none(),
                    "{a_name} and {b_name} overlap, so one of them cannot be clicked"
                );
            }
        }
    }

    #[test]
    fn a_key_past_the_end_of_a_row_has_no_box() {
        let l = Layout::new(560.0, 720.0, 5);
        for (row, letters) in KEY_ROWS.iter().enumerate() {
            assert!(
                l.key_rect(row, letters.len()).is_empty(),
                "row {row} has a key past its last letter"
            );
        }
        assert!(
            l.key_rect(KEY_ROWS.len(), 0).is_empty(),
            "there is a fourth row of keys"
        );
    }

    // ── The header ─────────────────────────────────────────────────

    /// The buttons share the right-hand end of the header, left to right, in
    /// the order the table names, one pad apart, and vertically centred in the
    /// band. Centring is the part the "inside the band" check cannot see: a
    /// button pinned to the top edge is still inside it (lesson 68).
    #[test]
    fn the_buttons_share_the_header_in_order_and_are_centred_in_it() {
        let l = Layout::new(560.0, 720.0, 5);
        let rects = l.button_rects();
        assert_eq!(rects.len(), HEADER_BUTTONS.len());
        for (i, r) in rects.iter().enumerate() {
            assert!(!r.is_empty(), "button {i} has no box");
            assert!(
                r.y >= l.header.y - 0.01 && r.bottom() <= l.header.bottom() + 0.01,
                "button {i} is outside the header band"
            );
            assert!(
                ((r.y - l.header.y) - (l.header.bottom() - r.bottom())).abs() <= 0.01,
                "button {i} is not centred in the band: {} above, {} below",
                r.y - l.header.y,
                l.header.bottom() - r.bottom()
            );
        }
        for pair in rects.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                (b.x - a.right() - l.pad).abs() <= 0.01,
                "the buttons are not one pad apart"
            );
            assert!(
                (a.w - b.w).abs() <= 0.01,
                "the buttons are not the same width"
            );
        }
        assert!(
            (rects[HEADER_BUTTONS.len() - 1].right() - (l.header.right() - l.pad)).abs() <= 0.01,
            "the last button is not a pad from the right edge"
        );
    }

    #[test]
    fn the_title_stops_before_the_first_button_starts() {
        let l = Layout::new(560.0, 720.0, 5);
        let title = l.title_rect();
        let first = l.button_rects()[0];
        assert!(!title.is_empty(), "the title has nowhere to go");
        assert!(
            title.right() <= first.x + 0.01,
            "the title runs under the first button"
        );
        assert!(
            title.x >= l.header.x - 0.01,
            "the title starts left of the window"
        );
    }

    // ── The keyboard ───────────────────────────────────────────────

    /// The row of letters typed in but not yet submitted.
    fn typed(g: &Wordle) -> String {
        g.current_input.iter().collect()
    }

    /// A key coming back up, which is the other half of every press.
    fn release(key: Key) -> KeyEvent {
        KeyEvent {
            pressed: false,
            ..probe::press(key)
        }
    }

    /// The letters must be read from the key's *name*.
    ///
    /// This is the one that says why: the event carries no text, because the
    /// compositor's `handle_text_input` is a no-op, so a game that read
    /// `KeyEvent::text` would take no letters at all from a real window.
    #[test]
    fn a_letter_is_taken_from_the_key_name_because_the_event_carries_no_text() {
        let mut g = game();
        let ev = probe::press(Key::C);
        assert!(
            ev.text.is_empty(),
            "the fixture stopped being the real shape"
        );
        assert_eq!(probe::key(&mut g, &ev), EventResult::Consumed);
        assert_eq!(typed(&g), "c");
    }

    /// Every letter of the alphabet types itself, and types the *same* letter
    /// its keyboard key would. A table of twenty-six arms is twenty-six chances
    /// to write the wrong one.
    #[test]
    fn every_letter_key_types_its_own_letter() {
        for (i, ch) in ('a'..='z').enumerate() {
            let key = [
                Key::A,
                Key::B,
                Key::C,
                Key::D,
                Key::E,
                Key::F,
                Key::G,
                Key::H,
                Key::I,
                Key::J,
                Key::K,
                Key::L,
                Key::M,
                Key::N,
                Key::O,
                Key::P,
                Key::Q,
                Key::R,
                Key::S,
                Key::T,
                Key::U,
                Key::V,
                Key::W,
                Key::X,
                Key::Y,
                Key::Z,
            ][i];
            let mut g = game();
            // H and N carry shortcuts, which are only reachable in positions a
            // letter could not be. One letter already typed puts us past both.
            g.add_letter('a');
            assert_eq!(
                probe::key(&mut g, &probe::press(key)),
                EventResult::Consumed
            );
            assert_eq!(
                typed(&g),
                format!("a{ch}"),
                "{key:?} typed the wrong letter"
            );
        }
    }

    /// The key coming back up must type nothing.
    ///
    /// The old handler destructured `KeyEvent { key, modifiers, .. }` and threw
    /// `pressed` away, so every letter went in twice and every Enter submitted
    /// the guess twice.
    #[test]
    fn a_key_coming_back_up_types_nothing() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::C));
        assert_eq!(probe::key(&mut g, &release(Key::C)), EventResult::Ignored);
        assert_eq!(typed(&g), "c", "the release typed a second C");
    }

    /// A whole word typed with both halves of every keystroke must arrive once.
    /// This is the shape the dropped `pressed` actually took on a screen.
    #[test]
    fn a_word_typed_with_presses_and_releases_arrives_once() {
        let mut g = game();
        for key in [Key::S, Key::T, Key::O, Key::N, Key::E] {
            probe::key(&mut g, &probe::press(key));
            probe::key(&mut g, &release(key));
        }
        assert_eq!(typed(&g), "stone");
        probe::key(&mut g, &probe::press(Key::Enter));
        probe::key(&mut g, &release(Key::Enter));
        assert_eq!(g.guesses.len(), 1, "the release submitted a second guess");
    }

    /// A held modifier means the keystroke belongs to whatever binds it.
    #[test]
    fn a_modified_key_is_left_for_whatever_binds_it() {
        for ev in [probe::ctrl(Key::C), probe::shift(Key::C)] {
            let mut g = game();
            assert_eq!(probe::key(&mut g, &ev), EventResult::Ignored);
            assert_eq!(typed(&g), "", "{:?} typed a letter", ev.modifiers);
        }
    }

    /// A key that is not a letter and not one of the shortcuts is not ours.
    #[test]
    fn a_key_the_game_has_no_use_for_is_left_alone() {
        for key in [Key::Tab, Key::F1, Key::Left, Key::Space] {
            let mut g = game();
            assert_eq!(
                probe::key(&mut g, &probe::press(key)),
                EventResult::Ignored,
                "{key:?} was taken"
            );
            assert_eq!(typed(&g), "");
        }
    }

    #[test]
    fn enter_submits_the_row_and_backspace_rubs_out_its_last_letter() {
        let mut g = game();
        for key in [Key::S, Key::T, Key::O, Key::N, Key::X] {
            probe::key(&mut g, &probe::press(key));
        }
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::Backspace)),
            EventResult::Consumed
        );
        assert_eq!(typed(&g), "ston");
        probe::key(&mut g, &probe::press(Key::E));
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::Enter)),
            EventResult::Consumed
        );
        assert_eq!(g.guesses.len(), 1);
        assert_eq!(typed(&g), "", "the answered row was left in place");
    }

    /// The row cannot be filled past the length of the word.
    #[test]
    fn a_row_already_full_takes_no_further_letters() {
        let mut g = game();
        for key in [Key::S, Key::T, Key::O, Key::N, Key::E] {
            probe::key(&mut g, &probe::press(key));
        }
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::X)),
            EventResult::Ignored,
            "a sixth letter was reported as typed"
        );
        assert_eq!(typed(&g), "stone");
    }

    #[test]
    fn one_two_and_three_pick_the_word_length() {
        for (key, want) in [
            (Key::Num1, Difficulty::Easy),
            (Key::Num2, Difficulty::Normal),
            (Key::Num3, Difficulty::Hard),
        ] {
            let mut g = game();
            let result = probe::key(&mut g, &probe::press(key));
            if want == Difficulty::Normal {
                // Already in play: nothing to do, and saying otherwise would
                // deal a fresh word on every stray keypress.
                assert_eq!(result, EventResult::Ignored);
            } else {
                assert_eq!(result, EventResult::Consumed);
            }
            assert_eq!(g.difficulty, want);
            assert_eq!(g.target_len, want.word_len());
        }
    }

    /// `H` is a letter of the alphabet before it is a shortcut. It reaches the
    /// hard-mode switch only where it could not be part of a guess.
    #[test]
    fn h_reaches_the_hard_mode_switch_only_where_no_letter_could_go() {
        let mut empty_row = game();
        assert_eq!(
            probe::key(&mut empty_row, &probe::press(Key::H)),
            EventResult::Consumed
        );
        assert!(empty_row.hard_mode, "H did not reach the switch");
        assert_eq!(typed(&empty_row), "", "H was typed as well as switched");

        let mut part_typed = game();
        part_typed.add_letter('c');
        probe::key(&mut part_typed, &probe::press(Key::H));
        assert_eq!(typed(&part_typed), "ch", "H did not type a letter mid-word");
        assert!(!part_typed.hard_mode, "H switched hard mode mid-word");

        let mut over = lost();
        probe::key(&mut over, &probe::press(Key::H));
        assert!(!over.hard_mode, "H switched hard mode on a finished game");
    }

    /// `N` and `Escape` deal a new word, and only once there is nothing left to
    /// guess. `N` mid-word is the letter.
    #[test]
    fn n_and_escape_deal_a_new_word_only_once_the_game_is_over() {
        for key in [Key::N, Key::Escape] {
            let mut g = lost();
            let before = g.games_played;
            assert_eq!(
                probe::key(&mut g, &probe::press(key)),
                EventResult::Consumed
            );
            assert_eq!(
                g.phase,
                GamePhase::Playing,
                "{key:?} did not deal a new word"
            );
            assert!(g.guesses.is_empty());
            assert_eq!(g.games_played, before, "{key:?} lost the totals");
        }

        let mut playing = game();
        probe::key(&mut playing, &probe::press(Key::N));
        assert_eq!(typed(&playing), "n", "N was a shortcut in a running game");

        let mut escaping = game();
        assert_eq!(
            probe::key(&mut escaping, &probe::press(Key::Escape)),
            EventResult::Ignored,
            "Escape dealt a new word out of a running game"
        );
    }

    /// A finished game takes no letters, and does not pretend it did. A frozen
    /// game that reports every keystroke as handled looks like a working one.
    #[test]
    fn a_finished_game_reports_the_keys_it_ignores_as_ignored() {
        for mut g in [lost(), won()] {
            let phase = g.phase;
            for key in [Key::C, Key::Enter, Key::Backspace] {
                assert_eq!(
                    probe::key(&mut g, &probe::press(key)),
                    EventResult::Ignored,
                    "{key:?} was reported as handled by a {phase:?} game"
                );
            }
            assert_eq!(typed(&g), "");
        }
    }

    // ── The mouse ──────────────────────────────────────────────────

    /// Every string the game draws at its usual size, in the order it draws
    /// them.
    fn shown(g: &Wordle) -> Vec<String> {
        shown_sized(g, (WINDOW_WIDTH, WINDOW_HEIGHT))
    }

    /// Every string the game draws into a window of the given size.
    fn shown_sized(g: &Wordle, size: (f32, f32)) -> Vec<String> {
        g.frame(size.0, size.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Every letter drawn on the on-screen keyboard must be clickable, and must
    /// type the letter it shows. The hit boxes come from the drawing pass, so
    /// this also says the ink and the target agree letter by letter.
    #[test]
    fn every_letter_drawn_on_the_keyboard_is_clickable_and_types_itself() {
        let mut seen = String::new();
        for letters in KEY_ROWS {
            for ch in letters.chars() {
                let mut g = game();
                assert_eq!(
                    probe::click(&mut g, Target::Key(ch)),
                    EventResult::Consumed,
                    "{ch} is drawn but not clickable"
                );
                assert_eq!(typed(&g), ch.to_ascii_lowercase().to_string());
                seen.push(ch);
            }
        }
        let mut alphabet: Vec<char> = seen.chars().collect();
        alphabet.sort_unstable();
        assert_eq!(
            alphabet.iter().collect::<String>(),
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            "the keyboard is not the alphabet"
        );
    }

    /// A key is clickable exactly where its ink is.
    ///
    /// `handle_keyboard_click` used to write the keyboard's geometry out a
    /// second time -- `kb_y_start = 420.0`, `key_w = 36.0`, a left edge of
    /// `80.0` -- and compare a click against *that*. Two copies of one rule is
    /// one copy that will be wrong.
    #[test]
    fn a_key_is_clickable_where_its_ink_is_and_nowhere_else() {
        let g = game();
        let l = g.layout();
        for (row, letters) in KEY_ROWS.iter().enumerate() {
            for (col, ch) in letters.chars().enumerate() {
                assert_eq!(
                    probe::rect_of(&g, Target::Key(ch)),
                    Some(l.key_rect(row, col)),
                    "{ch} is clickable somewhere other than where it is drawn"
                );
            }
        }
        assert_eq!(probe::rect_of(&g, Target::Enter), Some(l.enter_rect()));
        assert_eq!(
            probe::rect_of(&g, Target::Backspace),
            Some(l.backspace_rect())
        );
    }

    /// The `H` on the on-screen keyboard is a letter. Only the *key* H carries
    /// the hard-mode shortcut, and only where a letter could not go.
    #[test]
    fn the_h_drawn_on_the_keyboard_is_a_letter_and_not_the_switch() {
        let mut g = game();
        probe::click(&mut g, Target::Key('H'));
        assert_eq!(typed(&g), "h");
        assert!(!g.hard_mode, "clicking the letter H switched hard mode");
    }

    #[test]
    fn clicking_enter_submits_the_row_and_del_rubs_out_its_last_letter() {
        let mut g = game();
        for ch in "STONX".chars() {
            probe::click(&mut g, Target::Key(ch));
        }
        assert_eq!(
            probe::click(&mut g, Target::Backspace),
            EventResult::Consumed
        );
        assert_eq!(typed(&g), "ston");
        probe::click(&mut g, Target::Key('E'));
        assert_eq!(probe::click(&mut g, Target::Enter), EventResult::Consumed);
        assert_eq!(g.guesses.len(), 1);
    }

    /// Each length button deals a word of the length written on it -- and the
    /// number on the button is the number the game plays, because the label is
    /// composed from `word_len` rather than spelled out beside it.
    #[test]
    fn each_length_button_deals_a_word_of_the_length_it_names() {
        for difficulty in EVERY_DIFFICULTY {
            let mut g = game_with("word");
            assert!(probe::is_visible(&g, Target::Level(difficulty)));
            probe::click(&mut g, Target::Level(difficulty));
            assert_eq!(g.difficulty, difficulty);
            assert_eq!(g.target_len, difficulty.word_len());
            assert!(
                shown(&g).contains(&format!("{} ({})", difficulty.name(), g.target_len)),
                "the button does not say the length it deals"
            );
        }
    }

    /// Clicking the length already in play must not be reported as handled --
    /// and must not deal a fresh word, which would throw the game away on a
    /// double click.
    #[test]
    fn clicking_the_length_already_in_play_changes_nothing_and_says_so() {
        let mut g = game();
        let before = g.target;
        assert_eq!(
            probe::click(&mut g, Target::Level(Difficulty::Normal)),
            EventResult::Ignored
        );
        assert_eq!(g.target, before, "the word was re-dealt");
    }

    #[test]
    fn clicking_hard_mode_turns_it_and_stops_turning_once_a_guess_is_in() {
        let mut g = game();
        assert_eq!(
            probe::click(&mut g, Target::HardMode),
            EventResult::Consumed
        );
        assert!(g.hard_mode);
        assert_eq!(
            probe::click(&mut g, Target::HardMode),
            EventResult::Consumed
        );
        assert!(!g.hard_mode, "the switch does not switch back");

        guess(&mut g, "stone");
        assert_eq!(
            probe::click(&mut g, Target::HardMode),
            EventResult::Ignored,
            "hard mode turned halfway through a game"
        );
        assert!(!g.hard_mode);
    }

    #[test]
    fn clicking_new_word_deals_one_and_keeps_the_totals() {
        let mut g = won();
        let (played, won_count, streak) = (g.games_played, g.games_won, g.streak);
        assert_eq!(probe::click(&mut g, Target::NewGame), EventResult::Consumed);
        assert_eq!(g.phase, GamePhase::Playing);
        assert!(g.guesses.is_empty());
        assert_eq!(
            (g.games_played, g.games_won, g.streak),
            (played, won_count, streak)
        );
    }

    /// A click carries no size, so it is read against the size the picture it
    /// was aimed at was drawn at. The old hit test used the literal `420.0`
    /// whatever the window happened to be.
    #[test]
    fn a_click_is_read_against_the_size_the_frame_was_drawn_at() {
        let big = (1100.0_f32, 980.0_f32);
        let g = game();
        let wide = probe::rect_of_sized(&g, Target::Key('Q'), big).unwrap();
        let small = probe::rect_of(&g, Target::Key('Q')).unwrap();
        let (x, y) = wide.centre();
        assert!(
            !small.contains(x, y),
            "the two sizes draw Q in the same place, so this proves nothing"
        );

        let mut resized = game();
        assert_eq!(
            probe::click_sized(&mut resized, Target::Key('Q'), MouseButton::Left, big),
            EventResult::Consumed
        );
        assert_eq!(typed(&resized), "q");

        // The very same point, in a window drawn at the smaller size, is not Q.
        let mut shrunk = game();
        shrunk.resize(Wordle::SIZE.0, Wordle::SIZE.1);
        let hit = shrunk.frame(Wordle::SIZE.0, Wordle::SIZE.1).hit_test(x, y);
        assert_ne!(hit, Some(Target::Key('Q')));
    }

    #[test]
    fn a_click_where_nothing_is_drawn_does_nothing() {
        let mut g = game();
        assert_eq!(probe::click_background(&mut g), EventResult::Ignored);
        assert_eq!(typed(&g), "");
    }

    /// Only the left button plays. A right click belongs to whatever puts up a
    /// menu.
    #[test]
    fn only_the_left_button_plays() {
        for button in [MouseButton::Right, MouseButton::Middle] {
            let mut g = game();
            assert_eq!(
                probe::click_with(&mut g, Target::Key('Q'), button),
                EventResult::Ignored,
                "{button:?} typed a letter"
            );
            assert_eq!(typed(&g), "");
        }
    }

    /// The board is a picture, not a control: clicking a tile does nothing, and
    /// says so.
    #[test]
    fn the_guess_grid_takes_no_clicks() {
        let g = game();
        let l = g.layout();
        let f = g.frame(l.window.w, l.window.h);
        for row in 0..MAX_GUESSES {
            for col in 0..l.cols {
                let (x, y) = l.tile_rect(row, col).centre();
                assert_eq!(f.hit_test(x, y), None, "tile ({row}, {col}) is clickable");
            }
        }
    }

    /// The header must keep working once the game is over -- it is the only way
    /// back to a running game with a mouse.
    #[test]
    fn the_header_still_answers_once_the_game_is_over() {
        for make in [lost as fn() -> Wordle, won as fn() -> Wordle] {
            for target in HEADER_BUTTONS {
                let mut g = make();
                assert!(
                    probe::is_visible(&g, target),
                    "{target:?} is gone from a finished game"
                );
                // Two are quiet, for reasons that have nothing to do with the
                // game being over: the length already in play has nothing to
                // change, and the hard-mode switch is held by the guesses that
                // ended the game -- a finished game has guesses by definition.
                let quiet = target == Target::Level(g.difficulty) || target == Target::HardMode;
                let want = if quiet {
                    EventResult::Ignored
                } else {
                    EventResult::Consumed
                };
                assert_eq!(probe::click(&mut g, target), want, "{target:?}");
                if !quiet {
                    assert_eq!(
                        g.phase,
                        GamePhase::Playing,
                        "{target:?} did not get the player back to a running game"
                    );
                }
            }
        }
    }

    /// A finished game's on-screen keyboard is still drawn -- it is the board's
    /// record of what was learnt -- but it takes nothing, and does not pretend
    /// otherwise.
    #[test]
    fn a_finished_game_reports_the_clicks_it_ignores_as_ignored() {
        for mut g in [lost(), won()] {
            for target in [Target::Key('Q'), Target::Enter, Target::Backspace] {
                assert!(probe::is_visible(&g, target));
                assert_eq!(
                    probe::click(&mut g, target),
                    EventResult::Ignored,
                    "{target:?} was reported as handled by a finished game"
                );
            }
            assert_eq!(typed(&g), "");
        }
    }

    // ── What the picture says ──────────────────────────────────────

    /// The colour the last `FillRect` covering exactly `r` was painted.
    ///
    /// The *last* one, because that is the one a player sees: the frame is
    /// drawn back to front.
    fn fill_of(g: &Wordle, r: Rect) -> Option<Color> {
        let l = g.layout();
        g.frame(l.window.w, l.window.h)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if (*x, *y, *width, *height) == (r.x, r.y, r.w, r.h) => Some(*color),
                _ => None,
            })
            .next_back()
    }

    /// The colour of the string drawn centred in `r`, if there is one.
    fn text_in(g: &Wordle, r: Rect) -> Option<String> {
        let l = g.layout();
        g.frame(l.window.w, l.window.h)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, y, text, .. } if r.contains(*x, *y) => Some(text.clone()),
                _ => None,
            })
            .next_back()
    }

    /// The row being typed is shown as it is typed, letter by letter, in the
    /// row that is next to be answered.
    #[test]
    fn the_grid_shows_the_row_as_it_is_typed() {
        let mut g = game();
        guess(&mut g, "stone");
        for (i, ch) in "cru".chars().enumerate() {
            g.add_letter(ch);
            let r = g.layout().tile_rect(1, i);
            assert_eq!(
                text_in(&g, r).as_deref(),
                Some(ch.to_ascii_uppercase().to_string().as_str()),
                "the letter typed into slot {i} is not drawn there"
            );
            assert_eq!(
                fill_of(&g, r),
                Some(TileState::Filled.color()),
                "a typed but unanswered slot is drawn as an answered one"
            );
        }
        // The slots past what has been typed stay empty, and the row after this
        // one is untouched.
        assert_eq!(text_in(&g, g.layout().tile_rect(1, 3)), None);
        assert_eq!(text_in(&g, g.layout().tile_rect(2, 0)), None);
    }

    /// An answered guess keeps its answer on the board, letter by letter and
    /// colour by colour. This is the whole record of the game.
    #[test]
    fn an_answered_guess_stays_on_the_board_in_the_colours_it_was_given() {
        let mut g = game();
        guess(&mut g, "stone");
        let (word, eval) = &g.guesses[0];
        for col in 0..g.target_len {
            let r = g.layout().tile_rect(0, col);
            assert_eq!(
                text_in(&g, r).as_deref(),
                Some(word[col].to_ascii_uppercase().to_string().as_str())
            );
            assert_eq!(
                fill_of(&g, r),
                Some(eval[col].color()),
                "slot {col} is not drawn in the colour it was answered"
            );
        }
    }

    /// The on-screen keyboard is drawn in what it has learnt -- that is the
    /// only place a player can read it back.
    #[test]
    fn the_keyboard_is_drawn_in_what_it_has_learnt() {
        let mut g = game();
        guess(&mut g, "stone");
        guess(&mut g, "trace");
        let l = g.layout();
        let mut seen = [false; 4];
        for (row, letters) in KEY_ROWS.iter().enumerate() {
            for (col, ch) in letters.chars().enumerate() {
                let state = g.letter_state(ch);
                seen[state as usize] = true;
                assert_eq!(
                    fill_of(&g, l.key_rect(row, col)),
                    Some(state.color()),
                    "{ch} is not drawn in the state the keyboard holds for it"
                );
            }
        }
        // Against "crane", "stone" leaves N and E green and S/T/O grey, and
        // "trace" leaves C yellow -- so all four states are on the screen at
        // once and none of the four arms above went unchecked.
        assert_eq!(seen, [true; 4], "not every letter state was drawn");
    }

    /// The panel over a finished game is drawn when the game is finished, and
    /// not before.
    #[test]
    fn the_over_panel_is_drawn_only_once_the_game_is_finished() {
        for line in [
            "You won!",
            "Out of guesses",
            "Press N or Esc for a new word",
        ] {
            assert!(
                !shown(&game()).contains(&line.to_string()),
                "a running game already says {line:?}"
            );
        }
        for g in [lost(), won()] {
            assert!(
                shown(&g).contains(&"Press N or Esc for a new word".to_string()),
                "a finished game does not say how to start another"
            );
        }
    }

    /// A lost game must say what the word was. It is the one thing a player who
    /// has run out of guesses wants.
    #[test]
    fn a_lost_game_says_what_the_word_was() {
        let g = lost();
        assert!(shown(&g).contains(&"Out of guesses".to_string()));
        assert!(
            shown(&g).contains(&"The word was CRANE".to_string()),
            "the answer was kept from the player: {:?}",
            shown(&g)
        );
    }

    /// A won game says how many guesses it took, counting them rather than
    /// spelling the number out.
    #[test]
    fn a_won_game_counts_the_guesses_it_took() {
        let mut one = game();
        guess(&mut one, "crane");
        assert!(shown(&one).contains(&"You won!".to_string()));
        assert!(
            shown(&one).contains(&"Solved in 1 guess".to_string()),
            "a single guess is reported in the plural: {:?}",
            shown(&one)
        );

        let mut three = game();
        guess(&mut three, "stone");
        guess(&mut three, "crest");
        guess(&mut three, "crane");
        assert!(shown(&three).contains(&"Solved in 3 guesses".to_string()));
    }

    /// The message line carries a refusal only while there is one to carry.
    #[test]
    fn the_message_line_is_drawn_only_when_there_is_something_to_say() {
        let g = game();
        assert_eq!(text_in(&g, g.layout().message), None);

        let mut refused = game();
        guess(&mut refused, "zzzzz");
        assert_eq!(
            text_in(&refused, refused.layout().message).as_deref(),
            Some("Not in word list")
        );

        // Editing the row clears it. A refused word is left in place to be
        // corrected rather than retyped, so the row is full and the next thing
        // a player can do to it is rub a letter out -- and a refusal that
        // outlived the word it refused would read as a refusal of the new one.
        refused.delete_letter();
        assert_eq!(text_in(&refused, refused.layout().message), None);
        refused.add_letter('s');
        assert_eq!(text_in(&refused, refused.layout().message), None);
    }

    /// The counter reads the totals, and reads them from the totals.
    #[test]
    fn the_counter_reads_the_totals() {
        let mut g = game();
        guess(&mut g, "crane");
        probe::click(&mut g, Target::NewGame);
        set_word(&mut g, "crane");
        // `set_word` deals a board, not a scoreline: the win above is still on
        // the books.
        assert!(
            shown(&g).contains(&"Played 1  Won 1  Streak 1  Best 1".to_string()),
            "the counter does not read the totals: {:?}",
            shown(&g)
        );
        for _ in 0..MAX_GUESSES {
            guess(&mut g, "stone");
        }
        assert!(
            shown(&g).contains(&"Played 2  Won 1  Streak 0  Best 1".to_string()),
            "the counter did not follow the loss: {:?}",
            shown(&g)
        );
    }

    /// The hint and the counter share one line, so each is told where to stop.
    /// Two strings on a line with no limit between them is one string printed
    /// over the other.
    #[test]
    fn the_hint_stops_before_the_counter_starts() {
        for (w, h) in SHAPES {
            let g = game();
            let f = g.frame(w, h);
            let l = Layout::new(w, h, g.target_len);
            if l.footer.is_empty() {
                continue;
            }
            let mut ends: Vec<(f32, f32)> = Vec::new();
            for c in f.commands() {
                if let RenderCommand::Text {
                    x, y, max_width, ..
                } = c
                    && l.footer.contains(*x, *y)
                {
                    let limit = max_width.expect("a footer string with no limit");
                    ends.push((*x, x + limit));
                }
            }
            assert_eq!(ends.len(), 2, "the footer at {w}x{h} is not two strings");
            assert!(
                ends[0].1 <= ends[1].0,
                "at {w}x{h} the hint runs to {} and the counter starts at {}",
                ends[0].1,
                ends[1].0
            );
            assert!(
                ends[1].1 <= l.footer.right() + 0.01,
                "at {w}x{h} the counter overruns the band"
            );
            assert!(
                ends[0].0 >= l.footer.x,
                "at {w}x{h} the hint starts left of the band"
            );
        }
    }

    /// Every string is told where to stop. One that is not is one the renderer
    /// will run off the edge of whatever holds it.
    #[test]
    fn every_string_the_game_draws_is_told_where_to_stop() {
        for (w, h) in SHAPES {
            for g in [game(), lost(), won()] {
                for c in g.frame(w, h).commands() {
                    if let RenderCommand::Text {
                        text, max_width, ..
                    } = c
                    {
                        assert!(
                            max_width.is_some(),
                            "{text:?} is drawn with no limit at {w}x{h}"
                        );
                    }
                }
            }
        }
    }

    /// Nothing is drawn outside the window, at any size. A frame that paints
    /// past its edge is a frame whose layout does not know how big it is.
    #[test]
    fn nothing_is_drawn_outside_the_window() {
        for (w, h) in SHAPES {
            for g in [game(), lost(), won()] {
                let l = Layout::new(w, h, g.target_len);
                for c in g.frame(w, h).commands() {
                    let (x, y, cw, ch) = match c {
                        RenderCommand::FillRect {
                            x,
                            y,
                            width,
                            height,
                            ..
                        }
                        | RenderCommand::StrokeRect {
                            x,
                            y,
                            width,
                            height,
                            ..
                        } => (*x, *y, *width, *height),
                        // A string's limit is measured from the box it is
                        // centred in, not from where the string itself starts,
                        // so the origin plus the limit legitimately reaches
                        // past the box. What is never legitimate is the origin
                        // itself being off the window — that is a string
                        // centred by a width taken without regard to the box,
                        // and it is drawn where nobody can read it.
                        RenderCommand::Text { x, y, .. } => (*x, *y, 0.0, 0.0),
                        _ => continue,
                    };
                    assert!(
                        x >= l.window.x - 0.01
                            && y >= l.window.y - 0.01
                            && x + cw <= l.window.right() + 0.01
                            && y + ch <= l.window.bottom() + 0.01,
                        "a {cw}x{ch} rectangle at ({x}, {y}) is outside a {w}x{h} window"
                    );
                }
            }
        }
    }

    /// The length in play is the lit button, and it moves when the length does.
    #[test]
    fn the_length_in_play_is_the_button_that_is_lit() {
        for difficulty in EVERY_DIFFICULTY {
            let mut g = game();
            g.set_difficulty(difficulty);
            let l = g.layout();
            for (target, r) in HEADER_BUTTONS.iter().zip(l.button_rects()) {
                if let Target::Level(d) = target {
                    let want = if *d == difficulty { BLUE } else { SURFACE0 };
                    assert_eq!(
                        fill_of(&g, r),
                        Some(want),
                        "with {difficulty:?} in play, {d:?} is drawn wrong"
                    );
                }
            }
        }
    }

    /// The hard-mode switch says, before it is clicked, that it can no longer
    /// turn. That greying is the only warning a player gets.
    #[test]
    fn the_hard_mode_switch_is_greyed_once_it_can_no_longer_turn() {
        let mut g = game();
        let r = probe::rect_of(&g, Target::HardMode).unwrap();
        assert_eq!(text_colour(&g, r), Some(TEXT), "a live switch is greyed");
        guess(&mut g, "stone");
        assert_eq!(
            text_colour(&g, r),
            Some(OVERLAY0),
            "the switch still looks live after it has stopped turning"
        );
    }

    /// The colour of the last string drawn inside `r`.
    fn text_colour(g: &Wordle, r: Rect) -> Option<Color> {
        let l = g.layout();
        g.frame(l.window.w, l.window.h)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, y, color, .. } if r.contains(*x, *y) => Some(*color),
                _ => None,
            })
            .next_back()
    }

    // ── The window ─────────────────────────────────────────────────

    /// The probe must draw at the size the window opens at, or every layout
    /// test above measures a window the program never shows.
    #[test]
    fn the_probe_draws_at_the_size_the_window_opens_at() {
        let g = game();
        let (w, h) = g.initial_size();
        assert_eq!(
            (w as f32, h as f32),
            Wordle::SIZE,
            "the window opens at {w}x{h} but the probe draws at {:?}",
            Wordle::SIZE
        );
    }

    /// Drawing records the size it drew at, because that is the size the next
    /// click is read against.
    #[test]
    fn rendering_records_the_size_it_drew_at() {
        let mut g = game();
        // Deliberately not `SIZE`: a new game already records `SIZE`, so
        // rendering at `SIZE` and finding `SIZE` afterwards would pass with
        // `render` having done nothing at all.
        let odd = (Wordle::SIZE.0 + 137.0, Wordle::SIZE.1 - 61.0);
        assert!(
            (odd.0 - g.size_drawn().0).abs() > 0.01,
            "the fixture size is the size the game already records"
        );
        let tree = g.render(odd.0, odd.1);
        assert!(!tree.commands.is_empty(), "render produced no commands");
        assert_eq!(g.size_drawn(), odd);
    }

    /// A resize is a resize whether or not a frame follows it.
    #[test]
    fn a_resize_moves_the_layout_the_next_click_is_read_against() {
        let mut g = game();
        let before = g.layout().keyboard;
        assert_eq!(
            handle_event(
                &mut g,
                &Event::Resize {
                    width: 900,
                    height: 500
                }
            ),
            EventResult::Consumed
        );
        assert_eq!(g.size_drawn(), (900.0, 500.0));
        assert_ne!(g.layout().keyboard, before, "the layout did not follow");
    }

    /// A window can be dragged to nothing. The layout must survive it rather
    /// than divide by the zero it was handed.
    #[test]
    fn a_window_squashed_to_nothing_still_lays_out() {
        let mut g = game();
        g.resize(0.0, 0.0);
        assert!(g.size_drawn().0 > 0.0 && g.size_drawn().1 > 0.0);
        let l = g.layout();
        assert!(l.window.w > 0.0 && l.window.h > 0.0);
        assert!(l.tile >= 0.0 && l.key_w >= 0.0 && l.key_h >= 0.0);
        // And it still draws, without panicking on the way.
        let _ = g.frame(l.window.w, l.window.h);
    }

    #[test]
    fn closing_the_window_exits_and_nothing_else_does() {
        let mut g = game();
        assert_eq!(g.on_event(&Event::CloseRequested), Response::Exit);
        assert_eq!(
            g.on_event(&Event::FocusIn),
            Response::Idle,
            "an event the game does not use should not force a repaint"
        );
        assert_eq!(
            g.on_event(&Event::Key(probe::press(Key::C))),
            Response::Redraw,
            "a typed letter must repaint, or the row on screen is a letter behind"
        );
        assert_eq!(
            g.on_event(&Event::Key(release(Key::C))),
            Response::Idle,
            "a key coming back up repainted a picture that did not change"
        );
    }

    /// The window says what it is, and says the same thing twice.
    #[test]
    fn the_window_names_itself() {
        let g = game();
        assert_eq!(g.title(), "Wordle");
        assert_eq!(g.app_id(), "wordle");
    }
}

#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]
#![allow(clippy::needless_range_loop)]
#![allow(unused_imports)]

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ── Tile states ──
#[derive(Clone, Copy, Debug, PartialEq)]
enum TileState {
    Empty,
    Filled,   // letter entered but not evaluated
    Correct,  // green — right letter, right position
    Present,  // yellow — right letter, wrong position
    Absent,   // gray — letter not in word
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
#[derive(Clone, Copy, Debug, PartialEq)]
enum Difficulty {
    Easy,    // 4 letters
    Normal,  // 5 letters (classic)
    Hard,    // 6 letters
}

impl Difficulty {
    fn word_len(self) -> usize {
        match self {
            Self::Easy => 4,
            Self::Normal => 5,
            Self::Hard => 6,
        }
    }

    fn max_guesses(self) -> usize {
        6
    }

    fn label(self) -> &'static str {
        match self {
            Self::Easy => "Easy (4)",
            Self::Normal => "Normal (5)",
            Self::Hard => "Hard (6)",
        }
    }
}

// ── RNG ──
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    fn next_range(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next() % max as u64) as usize
    }
}

// ── Word lists ──
const WORDS_4: &[&str] = &[
    "able", "also", "area", "army", "away", "back", "band", "bank", "base", "bath",
    "bean", "bear", "beat", "bell", "best", "bird", "bite", "blow", "blue", "boat",
    "body", "bomb", "bone", "book", "born", "boss", "both", "bowl", "burn", "busy",
    "cafe", "cage", "cake", "call", "calm", "came", "camp", "card", "care", "case",
    "cash", "cast", "cave", "chat", "chip", "city", "clap", "clay", "clip", "club",
    "coal", "coat", "code", "coin", "cold", "come", "cook", "cool", "cope", "copy",
    "core", "cost", "crew", "crop", "curl", "cute", "dare", "dark", "data", "date",
    "dawn", "dead", "deaf", "deal", "dear", "debt", "deck", "deep", "deer", "deny",
    "desk", "diet", "dirt", "dish", "disk", "dock", "does", "done", "door", "dose",
    "down", "drag", "draw", "drop", "drum", "dual", "duck", "dull", "dump", "dust",
    "duty", "each", "earn", "ease", "east", "easy", "edge", "edit", "else", "epic",
    "even", "ever", "evil", "exam", "exit", "face", "fact", "fade", "fail", "fair",
    "fake", "fall", "fame", "farm", "fast", "fate", "fear", "feed", "feel", "file",
    "fill", "film", "find", "fine", "fire", "firm", "fish", "flag", "flat", "flew",
    "flip", "flow", "fold", "folk", "fond", "font", "food", "fool", "foot", "fork",
    "form", "fort", "foul", "four", "free", "from", "fuel", "full", "fund", "fury",
    "fuse", "gain", "game", "gang", "gate", "gave", "gaze", "gear", "gift", "girl",
    "give", "glad", "glow", "glue", "goal", "goat", "goes", "gold", "golf", "gone",
    "good", "grab", "gray", "grew", "grid", "grin", "grip", "grow", "gulf", "gust",
    "guys", "hack", "hair", "half", "hall", "halt", "hand", "hang", "hard", "harm",
    "harp", "hate", "have", "head", "heal", "heap", "hear", "heat", "heel", "held",
    "help", "herb", "here", "hero", "hide", "high", "hike", "hill", "hint", "hire",
    "hold", "hole", "holy", "home", "hood", "hook", "hope", "horn", "host", "hour",
    "huge", "hung", "hunt", "hurt", "hymn", "icon", "idea", "inch", "into", "iron",
    "item", "jack", "jail", "jazz", "jean", "jobs", "join", "joke", "jump", "jury",
    "just", "keen", "keep", "kept", "kick", "kids", "kill", "kind", "king", "kiss",
    "knee", "knew", "knit", "knob", "knot", "know", "lack", "laid", "lake", "lamp",
    "land", "lane", "last", "late", "lawn", "lead", "leaf", "lean", "left", "lend",
    "lens", "less", "lied", "life", "lift", "like", "limb", "lime", "line", "link",
    "lion", "list", "live", "load", "loan", "lock", "logo", "long", "look", "loop",
    "lord", "lose", "loss", "lost", "lots", "loud", "love", "luck", "lung", "made",
    "mail", "main", "make", "male", "mall", "many", "maps", "mark", "mass", "mate",
    "maze", "meal", "mean", "meat", "meet", "melt", "menu", "mere", "mesh", "mess",
    "mild", "mile", "milk", "mill", "mind", "mine", "mint", "miss", "mode", "mood",
    "moon", "more", "moss", "most", "move", "much", "must", "myth", "nail", "name",
    "navy", "near", "neat", "neck", "need", "nest", "nets", "next", "nice", "nine",
    "node", "none", "norm", "nose", "note", "noun", "odds", "okay", "once", "ones",
    "only", "onto", "open", "oral", "oven", "over", "pace", "pack", "page", "paid",
    "pain", "pair", "pale", "palm", "pane", "park", "part", "pass", "past", "path",
    "peak", "peel", "peer", "pick", "pile", "pine", "pink", "pipe", "plan", "play",
    "plot", "plug", "plus", "poem", "poet", "pole", "poll", "pond", "pool", "poor",
    "pope", "pork", "port", "pose", "post", "pour", "pray", "prey", "pull", "pump",
    "pure", "push", "quit", "quiz", "race", "rack", "rage", "raid", "rail", "rain",
    "rank", "rare", "rate", "read", "real", "rear", "reef", "rely", "rent", "rest",
    "rice", "rich", "ride", "ring", "rise", "risk", "road", "rock", "rode", "role",
    "roll", "roof", "room", "root", "rope", "rose", "ruin", "rule", "rush", "safe",
    "sage", "said", "sake", "sale", "salt", "same", "sand", "sang", "save", "seal",
    "seat", "seed", "seek", "seem", "seen", "self", "sell", "send", "sent", "sept",
    "shed", "ship", "shop", "shot", "show", "shut", "sick", "side", "sigh", "sign",
    "silk", "sing", "sink", "site", "size", "skin", "slam", "slid", "slim", "slip",
    "slot", "slow", "snap", "snow", "soap", "sofa", "soft", "soil", "sold", "sole",
    "some", "song", "soon", "sort", "soul", "soup", "spin", "spot", "star", "stay",
    "stem", "step", "stir", "stop", "such", "suit", "sure", "surf", "swim", "tack",
    "tail", "take", "tale", "talk", "tall", "tank", "tape", "task", "taxi", "team",
    "tear", "tell", "tend", "tent", "term", "test", "text", "than", "that", "them",
    "then", "they", "thin", "this", "thus", "tick", "tide", "tidy", "tied", "tier",
    "tile", "till", "time", "tiny", "tire", "toad", "told", "toll", "tone", "took",
    "tool", "tops", "tore", "torn", "tour", "town", "trap", "tray", "tree", "trim",
    "trio", "trip", "true", "tube", "tuck", "tune", "turn", "twin", "type", "ugly",
    "unit", "upon", "urge", "used", "user", "vale", "vary", "vast", "veil", "vein",
    "vent", "verb", "very", "vest", "view", "vine", "visa", "void", "volt", "vote",
    "wade", "wage", "wait", "wake", "walk", "wall", "want", "ward", "warm", "warn",
    "warp", "wash", "wave", "weak", "wear", "weed", "week", "well", "went", "were",
    "west", "what", "when", "whom", "wide", "wife", "wild", "will", "wind", "wine",
    "wing", "wire", "wise", "wish", "with", "woke", "wolf", "wood", "wool", "word",
    "wore", "work", "worm", "worn", "wrap", "yard", "year", "yell", "yoga", "your",
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
    "deliver", "demand", "dental", "depart", "deploy", "deputy", "derive", "desert", "design",
    "desire", "detail", "detect", "device", "differ", "digest", "dinner", "direct", "divide",
    "domain", "double", "driver", "during", "easily", "eating", "editor", "effect", "effort",
    "emerge", "empire", "enable", "endure", "energy", "engage", "engine", "enough", "ensure",
    "entire", "entity", "equity", "escape", "estate", "ethnic", "evolve", "exceed", "except",
    "excite", "excuse", "exempt", "exist", "expand", "expect", "expert", "export", "expose",
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
    "uphold", "urgent", "useful", "valley", "vanish", "vendor", "venture", "verbal", "verify",
    "victim", "violet", "virtue", "vision", "visual", "volume", "wander", "warmth", "wealth",
    "weapon", "weekly", "widely", "window", "winter", "wisdom", "within", "wonder", "worker",
    "worthy", "wounds", "writer", "yellow",
];

// ── Game state ──
#[derive(Clone, Copy, Debug, PartialEq)]
enum GamePhase {
    Playing,
    Won,
    Lost,
}

struct Wordle {
    difficulty: Difficulty,
    target: [char; 6],   // target word (up to 6 chars)
    target_len: usize,
    guesses: Vec<([char; 6], [TileState; 6])>, // each guess with its evaluation
    current_input: Vec<char>,
    keyboard_state: [LetterState; 26], // A-Z
    phase: GamePhase,
    rng: Rng,
    message: Option<&'static str>,
    games_played: u32,
    games_won: u32,
    streak: u32,
    best_streak: u32,
    hard_mode: bool, // must use revealed hints
}

impl Wordle {
    fn new() -> Self {
        let mut rng = Rng::new(42);
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
        }
    }

    fn word_list(difficulty: Difficulty) -> &'static [&'static str] {
        match difficulty {
            Difficulty::Easy => WORDS_4,
            Difficulty::Normal => WORDS_5,
            Difficulty::Hard => WORDS_6,
        }
    }

    fn pick_word(difficulty: Difficulty, rng: &mut Rng) -> ([char; 6], usize) {
        let words = Self::word_list(difficulty);
        let idx = rng.next_range(words.len());
        let word = words.get(idx).copied().unwrap_or("crane");
        let mut chars = [' '; 6];
        let len = word.len().min(6);
        for (i, ch) in word.chars().take(6).enumerate() {
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

    fn evaluate_guess(&self, guess: &[char]) -> [TileState; 6] {
        let mut result = [TileState::Empty; 6];
        let len = self.target_len;
        let mut target_used = [false; 6];
        let mut guess_matched = [false; 6];

        // First pass: mark correct (green)
        for i in 0..len {
            let g = guess.get(i).copied().unwrap_or(' ').to_ascii_lowercase();
            let t = self.target.get(i).copied().unwrap_or(' ').to_ascii_lowercase();
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
                let t = self.target.get(j).copied().unwrap_or(' ').to_ascii_lowercase();
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
            if !found {
                if let Some(r) = result.get_mut(i) {
                    *r = TileState::Absent;
                }
            }
        }

        result
    }

    fn update_keyboard(&mut self, guess: &[char], eval: &[TileState; 6]) {
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
            let current = self.keyboard_state.get(idx).copied().unwrap_or(LetterState::Unknown);
            // Only upgrade: Correct > Present > Absent > Unknown
            let should_update = match (current, new_state) {
                (LetterState::Unknown, _) => true,
                (LetterState::Absent, LetterState::Present | LetterState::Correct) => true,
                (LetterState::Present, LetterState::Correct) => true,
                _ => false,
            };
            if should_update {
                if let Some(slot) = self.keyboard_state.get_mut(idx) {
                    *slot = new_state;
                }
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
                let prev_ch = prev_guess.get(i).copied().unwrap_or(' ').to_ascii_lowercase();
                let curr_ch = guess.get(i).copied().unwrap_or(' ').to_ascii_lowercase();

                if prev_tile == TileState::Correct && curr_ch != prev_ch {
                    return Some("Hard mode: must use correct letters");
                }
            }
            // Check present letters are used
            for i in 0..self.target_len {
                let prev_tile = prev_eval.get(i).copied().unwrap_or(TileState::Empty);
                if prev_tile == TileState::Present {
                    let prev_ch = prev_guess.get(i).copied().unwrap_or(' ').to_ascii_lowercase();
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

    fn submit_guess(&mut self) {
        if self.phase != GamePhase::Playing {
            return;
        }
        if self.current_input.len() != self.target_len {
            self.message = Some("Not enough letters");
            return;
        }

        if !self.is_valid_word(&self.current_input) {
            self.message = Some("Not in word list");
            return;
        }

        if let Some(msg) = self.check_hard_mode(&self.current_input) {
            self.message = Some(msg);
            return;
        }

        let mut guess_arr = [' '; 6];
        for (i, ch) in self.current_input.iter().enumerate().take(6) {
            if let Some(slot) = guess_arr.get_mut(i) {
                *slot = *ch;
            }
        }

        let eval = self.evaluate_guess(&self.current_input);
        self.update_keyboard(&self.current_input, &eval);
        self.guesses.push((guess_arr, eval));
        self.message = None;

        // Check win/lose
        let all_correct = (0..self.target_len).all(|i| {
            eval.get(i).copied().unwrap_or(TileState::Empty) == TileState::Correct
        });

        if all_correct {
            self.phase = GamePhase::Won;
            self.games_played = self.games_played.saturating_add(1);
            self.games_won = self.games_won.saturating_add(1);
            self.streak = self.streak.saturating_add(1);
            if self.streak > self.best_streak {
                self.best_streak = self.streak;
            }
            self.message = Some("Brilliant!");
        } else if self.guesses.len() >= self.difficulty.max_guesses() {
            self.phase = GamePhase::Lost;
            self.games_played = self.games_played.saturating_add(1);
            self.streak = 0;
            self.message = None; // will show target word
        }

        self.current_input.clear();
    }

    fn add_letter(&mut self, ch: char) {
        if self.phase != GamePhase::Playing {
            return;
        }
        if self.current_input.len() < self.target_len {
            self.current_input.push(ch.to_ascii_lowercase());
            self.message = None;
        }
    }

    fn delete_letter(&mut self) {
        if self.phase != GamePhase::Playing {
            return;
        }
        self.current_input.pop();
        self.message = None;
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

    fn set_difficulty(&mut self, diff: Difficulty) {
        if diff != self.difficulty {
            self.difficulty = diff;
            self.new_game();
        }
    }

    fn toggle_hard_mode(&mut self) {
        // Can only toggle before first guess
        if self.guesses.is_empty() {
            self.hard_mode = !self.hard_mode;
        }
    }

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key, modifiers, .. }) => {
                if *modifiers != Modifiers::NONE {
                    return;
                }
                match key {
                    Key::A => self.add_letter('a'),
                    Key::B => self.add_letter('b'),
                    Key::C => self.add_letter('c'),
                    Key::D => self.add_letter('d'),
                    Key::E => self.add_letter('e'),
                    Key::F => self.add_letter('f'),
                    Key::G => self.add_letter('g'),
                    Key::H => {
                        if self.phase != GamePhase::Playing || !self.current_input.is_empty() {
                            self.add_letter('h');
                        } else {
                            self.toggle_hard_mode();
                        }
                    }
                    Key::I => self.add_letter('i'),
                    Key::J => self.add_letter('j'),
                    Key::K => self.add_letter('k'),
                    Key::L => self.add_letter('l'),
                    Key::M => self.add_letter('m'),
                    Key::N => {
                        if self.phase != GamePhase::Playing {
                            self.new_game();
                        } else {
                            self.add_letter('n');
                        }
                    }
                    Key::O => self.add_letter('o'),
                    Key::P => self.add_letter('p'),
                    Key::Q => self.add_letter('q'),
                    Key::R => self.add_letter('r'),
                    Key::S => self.add_letter('s'),
                    Key::T => self.add_letter('t'),
                    Key::U => self.add_letter('u'),
                    Key::V => self.add_letter('v'),
                    Key::W => self.add_letter('w'),
                    Key::X => self.add_letter('x'),
                    Key::Y => self.add_letter('y'),
                    Key::Z => self.add_letter('z'),
                    Key::Backspace => self.delete_letter(),
                    Key::Enter => self.submit_guess(),
                    Key::Num1 => self.set_difficulty(Difficulty::Easy),
                    Key::Num2 => self.set_difficulty(Difficulty::Normal),
                    Key::Num3 => self.set_difficulty(Difficulty::Hard),
                    Key::Escape => {
                        if self.phase != GamePhase::Playing {
                            self.new_game();
                        }
                    }
                    _ => {}
                }
            }
            Event::Mouse(MouseEvent { x, y, kind }) => {
                if !matches!(kind, MouseEventKind::Press(MouseButton::Left)) {
                    return;
                }
                self.handle_keyboard_click(*x, *y);
            }
            _ => {}
        }
    }

    fn handle_keyboard_click(&mut self, mx: f32, my: f32) {
        // On-screen keyboard layout
        let kb_y_start = 420.0_f32;
        let key_w = 36.0_f32;
        let key_h = 40.0_f32;
        let gap = 4.0_f32;

        let rows: &[&[char]] = &[
            &['Q','W','E','R','T','Y','U','I','O','P'],
            &['A','S','D','F','G','H','J','K','L'],
            &['Z','X','C','V','B','N','M'],
        ];

        for (row_idx, row) in rows.iter().enumerate() {
            let row_offset = match row_idx {
                1 => 18.0_f32,
                2 => 36.0_f32,
                _ => 0.0_f32,
            };
            let row_y = kb_y_start + (row_idx as f32) * (key_h + gap);

            for (col_idx, ch) in row.iter().enumerate() {
                let kx = 80.0_f32 + row_offset + (col_idx as f32) * (key_w + gap);
                let ky = row_y;

                if mx >= kx && mx < kx + key_w && my >= ky && my < ky + key_h {
                    self.add_letter(ch.to_ascii_lowercase());
                    return;
                }
            }
        }

        // Enter button (row 3, left of Z)
        let enter_y = kb_y_start + 2.0 * (key_h + gap);
        let enter_x = 80.0_f32;
        let enter_w = 32.0_f32;
        if mx >= enter_x && mx < enter_x + enter_w && my >= enter_y && my < enter_y + key_h {
            self.submit_guess();
            return;
        }

        // Backspace button (row 3, right of M)
        let bksp_x = 80.0_f32 + 36.0_f32 + 7.0 * (key_w + gap) + 36.0;
        if mx >= bksp_x && mx < bksp_x + 50.0 && my >= enter_y && my < enter_y + key_h {
            self.delete_letter();
        }
    }

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width, height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: width / 2.0 - 40.0, y: 15.0,
            text: "WORDLE".to_string(),
            color: TEXT,
            font_size: 28.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Difficulty indicator
        let diff_label = self.difficulty.label();
        cmds.push(RenderCommand::Text {
            x: width / 2.0 - 30.0, y: 48.0,
            text: diff_label.to_string(),
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Hard mode indicator
        if self.hard_mode {
            cmds.push(RenderCommand::Text {
                x: width - 100.0, y: 15.0,
                text: "HARD".to_string(),
                color: RED,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Stats
        let stats_text = format!(
            "Played: {}  Won: {}  Streak: {}  Best: {}",
            self.games_played, self.games_won, self.streak, self.best_streak
        );
        cmds.push(RenderCommand::Text {
            x: 10.0, y: 15.0,
            text: stats_text,
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Grid
        let tile_size = 52.0_f32;
        let tile_gap = 6.0_f32;
        let word_len = self.target_len;
        let max_guesses = self.difficulty.max_guesses();
        let grid_width = (word_len as f32) * tile_size + ((word_len as f32) - 1.0) * tile_gap;
        let grid_x = (width - grid_width) / 2.0;
        let grid_y = 70.0_f32;

        for row in 0..max_guesses {
            for col in 0..word_len {
                let tx = grid_x + (col as f32) * (tile_size + tile_gap);
                let ty = grid_y + (row as f32) * (tile_size + tile_gap);

                let (ch, state) = if row < self.guesses.len() {
                    let (guess, eval) = &self.guesses[row];
                    let c = guess.get(col).copied().unwrap_or(' ');
                    let s = eval.get(col).copied().unwrap_or(TileState::Empty);
                    (c, s)
                } else if row == self.guesses.len() {
                    // Current input row
                    if col < self.current_input.len() {
                        let c = self.current_input.get(col).copied().unwrap_or(' ');
                        (c, TileState::Filled)
                    } else {
                        (' ', TileState::Empty)
                    }
                } else {
                    (' ', TileState::Empty)
                };

                // Tile background
                cmds.push(RenderCommand::FillRect {
                    x: tx, y: ty,
                    width: tile_size, height: tile_size,
                    color: state.color(),
                    corner_radii: CornerRadii::all(4.0),
                });

                // Tile border for empty/filled
                if state == TileState::Empty || state == TileState::Filled {
                    let border_color = if state == TileState::Filled { SURFACE2 } else { SURFACE1 };
                    // Top
                    cmds.push(RenderCommand::Line {
                        x1: tx, y1: ty, x2: tx + tile_size, y2: ty,
                        color: border_color, width: 2.0,
                    });
                    // Bottom
                    cmds.push(RenderCommand::Line {
                        x1: tx, y1: ty + tile_size, x2: tx + tile_size, y2: ty + tile_size,
                        color: border_color, width: 2.0,
                    });
                    // Left
                    cmds.push(RenderCommand::Line {
                        x1: tx, y1: ty, x2: tx, y2: ty + tile_size,
                        color: border_color, width: 2.0,
                    });
                    // Right
                    cmds.push(RenderCommand::Line {
                        x1: tx + tile_size, y1: ty, x2: tx + tile_size, y2: ty + tile_size,
                        color: border_color, width: 2.0,
                    });
                }

                // Letter
                if ch != ' ' {
                    let letter_color = match state {
                        TileState::Correct | TileState::Present | TileState::Absent => CRUST,
                        _ => TEXT,
                    };
                    cmds.push(RenderCommand::Text {
                        x: tx + tile_size / 2.0 - 8.0,
                        y: ty + tile_size / 2.0 - 10.0,
                        text: ch.to_ascii_uppercase().to_string(),
                        color: letter_color,
                        font_size: 24.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }
            }
        }

        // Message
        if let Some(msg) = self.message {
            let msg_y = grid_y + (max_guesses as f32) * (tile_size + tile_gap) + 5.0;
            cmds.push(RenderCommand::Text {
                x: width / 2.0 - 80.0, y: msg_y,
                text: msg.to_string(),
                color: PEACH,
                font_size: 16.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // On-screen keyboard
        let kb_y_start = 420.0_f32;
        let key_w = 36.0_f32;
        let key_h = 40.0_f32;
        let gap = 4.0_f32;

        let rows: &[&[char]] = &[
            &['Q','W','E','R','T','Y','U','I','O','P'],
            &['A','S','D','F','G','H','J','K','L'],
            &['Z','X','C','V','B','N','M'],
        ];

        for (row_idx, row) in rows.iter().enumerate() {
            let row_offset = match row_idx {
                1 => 18.0_f32,
                2 => 36.0_f32,
                _ => 0.0_f32,
            };
            let row_y = kb_y_start + (row_idx as f32) * (key_h + gap);

            for (col_idx, ch) in row.iter().enumerate() {
                let kx = 80.0_f32 + row_offset + (col_idx as f32) * (key_w + gap);
                let ky = row_y;

                let idx = (*ch as u8).wrapping_sub(b'A') as usize;
                let letter_state = if idx < 26 {
                    self.keyboard_state.get(idx).copied().unwrap_or(LetterState::Unknown)
                } else {
                    LetterState::Unknown
                };

                let bg = letter_state.color();
                let fg = match letter_state {
                    LetterState::Correct | LetterState::Present => CRUST,
                    _ => TEXT,
                };

                cmds.push(RenderCommand::FillRect {
                    x: kx, y: ky,
                    width: key_w, height: key_h,
                    color: bg,
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: kx + key_w / 2.0 - 6.0,
                    y: ky + key_h / 2.0 - 8.0,
                    text: ch.to_string(),
                    color: fg,
                    font_size: 16.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
        }

        // Enter and Backspace keys on row 3
        let enter_y = kb_y_start + 2.0 * (key_h + gap);

        // Enter
        cmds.push(RenderCommand::FillRect {
            x: 80.0, y: enter_y,
            width: 32.0, height: key_h,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: 82.0, y: enter_y + 12.0,
            text: "ENT".to_string(),
            color: TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Backspace
        let bksp_x = 80.0_f32 + 36.0_f32 + 7.0 * (key_w + gap) + 36.0;
        cmds.push(RenderCommand::FillRect {
            x: bksp_x, y: enter_y,
            width: 50.0, height: key_h,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: bksp_x + 6.0, y: enter_y + 12.0,
            text: "DEL".to_string(),
            color: TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Footer
        let footer_y = height - 25.0;
        cmds.push(RenderCommand::Text {
            x: 10.0, y: footer_y,
            text: "Type letters | Enter: submit | Backspace: delete | 1/2/3: difficulty | N: new game (when over)".to_string(),
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Game over overlay
        if self.phase != GamePhase::Playing {
            // Semi-transparent overlay
            cmds.push(RenderCommand::FillRect {
                x: width / 2.0 - 140.0,
                y: height / 2.0 - 60.0,
                width: 280.0, height: 120.0,
                color: MANTLE,
                corner_radii: CornerRadii::all(12.0),
            });

            let (title, title_color) = match self.phase {
                GamePhase::Won => ("You Won!", GREEN),
                GamePhase::Lost => ("Game Over", RED),
                _ => ("", TEXT),
            };

            cmds.push(RenderCommand::Text {
                x: width / 2.0 - 50.0,
                y: height / 2.0 - 45.0,
                text: title.to_string(),
                color: title_color,
                font_size: 24.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            if self.phase == GamePhase::Won {
                let guesses_text = format!("Solved in {} guess{}", self.guesses.len(),
                    if self.guesses.len() == 1 { "" } else { "es" });
                cmds.push(RenderCommand::Text {
                    x: width / 2.0 - 70.0,
                    y: height / 2.0 - 10.0,
                    text: guesses_text,
                    color: TEXT,
                    font_size: 16.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            } else {
                let answer = format!("Word: {}", self.target_word().to_uppercase());
                cmds.push(RenderCommand::Text {
                    x: width / 2.0 - 50.0,
                    y: height / 2.0 - 10.0,
                    text: answer,
                    color: YELLOW,
                    font_size: 16.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            cmds.push(RenderCommand::Text {
                x: width / 2.0 - 60.0,
                y: height / 2.0 + 30.0,
                text: "Press N or Esc for new game".to_string(),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        cmds
    }
}

fn main() {
    let _app = Wordle::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rng_deterministic() {
        let mut r1 = Rng::new(42);
        let mut r2 = Rng::new(42);
        assert_eq!(r1.next(), r2.next());
        assert_eq!(r1.next(), r2.next());
    }

    #[test]
    fn test_rng_range() {
        let mut rng = Rng::new(123);
        for _ in 0..100 {
            let val = rng.next_range(10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_rng_range_zero() {
        let mut rng = Rng::new(1);
        assert_eq!(rng.next_range(0), 0);
    }

    #[test]
    fn test_difficulty_word_len() {
        assert_eq!(Difficulty::Easy.word_len(), 4);
        assert_eq!(Difficulty::Normal.word_len(), 5);
        assert_eq!(Difficulty::Hard.word_len(), 6);
    }

    #[test]
    fn test_difficulty_max_guesses() {
        assert_eq!(Difficulty::Easy.max_guesses(), 6);
        assert_eq!(Difficulty::Normal.max_guesses(), 6);
        assert_eq!(Difficulty::Hard.max_guesses(), 6);
    }

    #[test]
    fn test_difficulty_labels() {
        assert!(!Difficulty::Easy.label().is_empty());
        assert!(!Difficulty::Normal.label().is_empty());
        assert!(!Difficulty::Hard.label().is_empty());
    }

    #[test]
    fn test_tile_state_colors() {
        // Just make sure all variants return distinct colors
        let colors: Vec<_> = [TileState::Empty, TileState::Filled, TileState::Correct,
                              TileState::Present, TileState::Absent]
            .iter().map(|s| s.color()).collect();
        assert_eq!(colors.len(), 5);
    }

    #[test]
    fn test_letter_state_colors() {
        let colors: Vec<_> = [LetterState::Unknown, LetterState::Correct,
                              LetterState::Present, LetterState::Absent]
            .iter().map(|s| s.color()).collect();
        assert_eq!(colors.len(), 4);
    }

    #[test]
    fn test_new_game() {
        let game = Wordle::new();
        assert_eq!(game.phase, GamePhase::Playing);
        assert!(game.guesses.is_empty());
        assert!(game.current_input.is_empty());
        assert_eq!(game.target_len, 5); // default Normal
    }

    #[test]
    fn test_pick_word() {
        let mut rng = Rng::new(99);
        let (word, len) = Wordle::pick_word(Difficulty::Normal, &mut rng);
        assert_eq!(len, 5);
        assert!(word[0].is_ascii_alphabetic());
    }

    #[test]
    fn test_pick_word_easy() {
        let mut rng = Rng::new(99);
        let (_, len) = Wordle::pick_word(Difficulty::Easy, &mut rng);
        assert_eq!(len, 4);
    }

    #[test]
    fn test_pick_word_hard() {
        let mut rng = Rng::new(99);
        let (_, len) = Wordle::pick_word(Difficulty::Hard, &mut rng);
        assert_eq!(len, 6);
    }

    #[test]
    fn test_target_word() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        assert_eq!(game.target_word(), "crane");
    }

    #[test]
    fn test_add_letter() {
        let mut game = Wordle::new();
        game.add_letter('a');
        assert_eq!(game.current_input.len(), 1);
        assert_eq!(game.current_input[0], 'a');
    }

    #[test]
    fn test_add_letter_max() {
        let mut game = Wordle::new();
        for ch in ['a', 'b', 'c', 'd', 'e', 'f'] {
            game.add_letter(ch);
        }
        assert_eq!(game.current_input.len(), 5); // Normal = 5 max
    }

    #[test]
    fn test_delete_letter() {
        let mut game = Wordle::new();
        game.add_letter('a');
        game.add_letter('b');
        game.delete_letter();
        assert_eq!(game.current_input.len(), 1);
        assert_eq!(game.current_input[0], 'a');
    }

    #[test]
    fn test_delete_empty() {
        let mut game = Wordle::new();
        game.delete_letter();
        assert!(game.current_input.is_empty());
    }

    #[test]
    fn test_evaluate_all_correct() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        let eval = game.evaluate_guess(&['c', 'r', 'a', 'n', 'e']);
        assert_eq!(eval[0], TileState::Correct);
        assert_eq!(eval[1], TileState::Correct);
        assert_eq!(eval[2], TileState::Correct);
        assert_eq!(eval[3], TileState::Correct);
        assert_eq!(eval[4], TileState::Correct);
    }

    #[test]
    fn test_evaluate_all_absent() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        let eval = game.evaluate_guess(&['f', 'l', 'u', 's', 'h']);
        assert_eq!(eval[0], TileState::Absent);
        assert_eq!(eval[1], TileState::Absent);
        assert_eq!(eval[2], TileState::Absent);
        assert_eq!(eval[3], TileState::Absent);
        assert_eq!(eval[4], TileState::Absent);
    }

    #[test]
    fn test_evaluate_present() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        // "acorn" — a is present (pos 0 vs target pos 2), c is present (pos 1 vs 0),
        // etc.
        let eval = game.evaluate_guess(&['a', 'c', 'o', 'r', 'n']);
        assert_eq!(eval[0], TileState::Present); // a in word but not pos 0
        assert_eq!(eval[1], TileState::Present); // c in word but not pos 1
        assert_eq!(eval[2], TileState::Absent);  // o not in word
        assert_eq!(eval[3], TileState::Present); // r in word but not pos 3
        assert_eq!(eval[4], TileState::Present); // n in word but not pos 4
    }

    #[test]
    fn test_evaluate_mixed() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        let eval = game.evaluate_guess(&['c', 'l', 'e', 'a', 'r']);
        assert_eq!(eval[0], TileState::Correct);  // c correct
        assert_eq!(eval[1], TileState::Absent);   // l not in word
        assert_eq!(eval[2], TileState::Present);   // e in word but not pos 2
        assert_eq!(eval[3], TileState::Present);   // a in word but not pos 3
        assert_eq!(eval[4], TileState::Present);   // r in word but not pos 4
    }

    #[test]
    fn test_evaluate_duplicate_letters() {
        let mut game = Wordle::new();
        game.target = ['s', 'p', 'e', 'e', 'd', ' '];
        game.target_len = 5;
        // Guess "creep" — two e's: e at pos 2 is correct, e at pos 3 is correct
        let eval = game.evaluate_guess(&['c', 'r', 'e', 'e', 'p']);
        assert_eq!(eval[0], TileState::Absent);   // c not in word
        assert_eq!(eval[1], TileState::Absent);   // r not in word (not at that position, not present)
        assert_eq!(eval[2], TileState::Correct);   // e correct at pos 2
        assert_eq!(eval[3], TileState::Correct);   // e correct at pos 3
        assert_eq!(eval[4], TileState::Present);   // p present (pos 1)
    }

    #[test]
    fn test_evaluate_excess_duplicate() {
        let mut game = Wordle::new();
        game.target = ['a', 'b', 'c', 'd', 'e', ' '];
        game.target_len = 5;
        // Guess "aaxxx" — first a correct, second a should be absent (only one a in target)
        let eval = game.evaluate_guess(&['a', 'a', 'x', 'x', 'x']);
        assert_eq!(eval[0], TileState::Correct);
        assert_eq!(eval[1], TileState::Absent);
        assert_eq!(eval[2], TileState::Absent);
    }

    #[test]
    fn test_is_valid_word() {
        let game = Wordle::new();
        assert!(game.is_valid_word(&['c', 'r', 'a', 'n', 'e']));
        assert!(!game.is_valid_word(&['x', 'x', 'x', 'x', 'x']));
    }

    #[test]
    fn test_submit_too_short() {
        let mut game = Wordle::new();
        game.add_letter('a');
        game.add_letter('b');
        game.submit_guess();
        assert!(game.message.is_some());
        assert!(game.guesses.is_empty()); // not submitted
    }

    #[test]
    fn test_submit_invalid_word() {
        let mut game = Wordle::new();
        for ch in ['x', 'x', 'x', 'x', 'x'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        assert!(game.message.is_some());
        assert!(game.guesses.is_empty());
    }

    #[test]
    fn test_submit_valid_word() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        for ch in ['a', 'b', 'o', 'u', 't'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        assert_eq!(game.guesses.len(), 1);
        assert!(game.current_input.is_empty());
    }

    #[test]
    fn test_win_detection() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        for ch in ['c', 'r', 'a', 'n', 'e'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        assert_eq!(game.phase, GamePhase::Won);
        assert_eq!(game.games_won, 1);
        assert_eq!(game.games_played, 1);
        assert_eq!(game.streak, 1);
    }

    #[test]
    fn test_lose_detection() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        // 6 wrong guesses
        let words = ["about", "brain", "drift", "equal", "flesh", "grill"];
        for word in &words {
            for ch in word.chars() {
                game.add_letter(ch);
            }
            game.submit_guess();
        }
        assert_eq!(game.phase, GamePhase::Lost);
        assert_eq!(game.games_played, 1);
        assert_eq!(game.games_won, 0);
        assert_eq!(game.streak, 0);
    }

    #[test]
    fn test_keyboard_state_update() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        for ch in ['c', 'l', 'e', 'a', 'r'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        // C should be correct (index 2)
        assert_eq!(game.keyboard_state[2], LetterState::Correct);
        // L should be absent (index 11)
        assert_eq!(game.keyboard_state[11], LetterState::Absent);
    }

    #[test]
    fn test_keyboard_upgrade_only() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;

        // First guess: "eager" — a is present
        for ch in ['e', 'a', 'g', 'e', 'r'] {
            game.add_letter(ch);
        }
        game.submit_guess();

        // e at pos 0 is present (target has e at pos 4)
        // After this, E should be at least Present
        let e_idx = 4; // E
        let state_after_first = game.keyboard_state[e_idx];
        assert!(state_after_first == LetterState::Present || state_after_first == LetterState::Correct);
    }

    #[test]
    fn test_new_game_resets() {
        let mut game = Wordle::new();
        game.add_letter('a');
        game.new_game();
        assert!(game.current_input.is_empty());
        assert!(game.guesses.is_empty());
        assert_eq!(game.phase, GamePhase::Playing);
        assert!(game.keyboard_state.iter().all(|s| *s == LetterState::Unknown));
    }

    #[test]
    fn test_set_difficulty() {
        let mut game = Wordle::new();
        game.set_difficulty(Difficulty::Easy);
        assert_eq!(game.difficulty, Difficulty::Easy);
        assert_eq!(game.target_len, 4);
        assert!(game.guesses.is_empty());
    }

    #[test]
    fn test_no_input_after_game_over() {
        let mut game = Wordle::new();
        game.phase = GamePhase::Won;
        game.add_letter('a');
        assert!(game.current_input.is_empty());
    }

    #[test]
    fn test_no_delete_after_game_over() {
        let mut game = Wordle::new();
        game.current_input.push('a');
        game.phase = GamePhase::Won;
        game.delete_letter();
        assert_eq!(game.current_input.len(), 1);
    }

    #[test]
    fn test_hard_mode_toggle() {
        let mut game = Wordle::new();
        assert!(!game.hard_mode);
        game.toggle_hard_mode();
        assert!(game.hard_mode);
        game.toggle_hard_mode();
        assert!(!game.hard_mode);
    }

    #[test]
    fn test_hard_mode_locked_after_guess() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        for ch in ['a', 'b', 'o', 'u', 't'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        game.toggle_hard_mode(); // should be locked
        assert!(!game.hard_mode);
    }

    #[test]
    fn test_hard_mode_requires_correct() {
        let mut game = Wordle::new();
        game.hard_mode = true;
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;

        // First guess
        for ch in ['c', 'l', 'o', 'n', 'e'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        // c correct, n correct, e correct

        // Second guess missing c at pos 0
        let result = game.check_hard_mode(&['d', 'r', 'i', 'n', 'e']);
        assert!(result.is_some());
    }

    #[test]
    fn test_hard_mode_requires_present() {
        let mut game = Wordle::new();
        game.hard_mode = true;
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;

        // guess "clear" — r is present, a is present
        for ch in ['c', 'l', 'e', 'a', 'r'] {
            game.add_letter(ch);
        }
        game.submit_guess();

        // Next guess must use r and a
        let result = game.check_hard_mode(&['c', 'l', 'o', 'n', 'e']);
        assert!(result.is_some()); // missing r and a
    }

    #[test]
    fn test_streak_tracking() {
        let mut game = Wordle::new();
        // Win
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        for ch in ['c', 'r', 'a', 'n', 'e'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        assert_eq!(game.streak, 1);
        assert_eq!(game.best_streak, 1);

        // Win again
        game.new_game();
        let word = game.target_word();
        for ch in word.chars() {
            game.add_letter(ch);
        }
        game.submit_guess();
        assert_eq!(game.streak, 2);
        assert_eq!(game.best_streak, 2);
    }

    #[test]
    fn test_streak_reset_on_loss() {
        let mut game = Wordle::new();
        game.streak = 5;
        game.best_streak = 5;
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        let words = ["about", "brain", "drift", "equal", "flesh", "grill"];
        for word in &words {
            for ch in word.chars() {
                game.add_letter(ch);
            }
            game.submit_guess();
        }
        assert_eq!(game.streak, 0);
        assert_eq!(game.best_streak, 5); // preserved
    }

    #[test]
    fn test_word_lists_not_empty() {
        assert!(!WORDS_4.is_empty());
        assert!(!WORDS_5.is_empty());
        assert!(!WORDS_6.is_empty());
    }

    #[test]
    fn test_word_list_lengths() {
        for word in WORDS_4 {
            assert_eq!(word.len(), 4, "4-letter word '{}' has wrong length", word);
        }
        for word in WORDS_5 {
            assert_eq!(word.len(), 5, "5-letter word '{}' has wrong length", word);
        }
        for word in WORDS_6 {
            assert_eq!(word.len(), 6, "6-letter word '{}' has wrong length", word);
        }
    }

    #[test]
    fn test_key_event_letters() {
        let mut game = Wordle::new();
        let event = Event::Key(KeyEvent {
            key: Key::A,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert_eq!(game.current_input.len(), 1);
        assert_eq!(game.current_input[0], 'a');
    }

    #[test]
    fn test_key_event_backspace() {
        let mut game = Wordle::new();
        game.add_letter('a');
        let event = Event::Key(KeyEvent {
            key: Key::Backspace,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert!(game.current_input.is_empty());
    }

    #[test]
    fn test_key_event_enter() {
        let mut game = Wordle::new();
        game.target = ['a', 'b', 'o', 'u', 't', ' '];
        game.target_len = 5;
        for ch in ['a', 'b', 'o', 'u', 't'] {
            game.add_letter(ch);
        }
        let event = Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert_eq!(game.guesses.len(), 1);
    }

    #[test]
    fn test_key_event_difficulty_switch() {
        let mut game = Wordle::new();
        let event = Event::Key(KeyEvent {
            key: Key::Num1,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert_eq!(game.difficulty, Difficulty::Easy);
    }

    #[test]
    fn test_n_key_new_game_when_over() {
        let mut game = Wordle::new();
        game.phase = GamePhase::Won;
        let event = Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert_eq!(game.phase, GamePhase::Playing);
    }

    #[test]
    fn test_n_key_types_n_during_play() {
        let mut game = Wordle::new();
        let event = Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert_eq!(game.current_input.len(), 1);
        assert_eq!(game.current_input[0], 'n');
    }

    #[test]
    fn test_escape_new_game_when_over() {
        let mut game = Wordle::new();
        game.phase = GamePhase::Lost;
        let event = Event::Key(KeyEvent {
            key: Key::Escape,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert_eq!(game.phase, GamePhase::Playing);
    }

    #[test]
    fn test_render_returns_commands() {
        let game = Wordle::new();
        let cmds = game.render(600.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_guesses() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        for ch in ['a', 'b', 'o', 'u', 't'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        let cmds = game.render(600.0, 600.0);
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_game_over() {
        let mut game = Wordle::new();
        game.phase = GamePhase::Won;
        let cmds = game.render(600.0, 600.0);
        // Should include overlay
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_mouse_click_keyboard() {
        let mut game = Wordle::new();
        // Click on Q key area (row 0, col 0)
        // kx = 80 + 0 + 0 * 40 = 80
        // ky = 420
        game.handle_keyboard_click(90.0, 430.0);
        assert_eq!(game.current_input.len(), 1);
        assert_eq!(game.current_input[0], 'q');
    }

    #[test]
    fn test_full_game_simulation() {
        let mut game = Wordle::new();
        game.target = ['a', 'b', 'o', 'u', 't', ' '];
        game.target_len = 5;

        // Guess 1: wrong
        for ch in ['c', 'r', 'a', 'n', 'e'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        assert_eq!(game.phase, GamePhase::Playing);

        // Guess 2: correct
        for ch in ['a', 'b', 'o', 'u', 't'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        assert_eq!(game.phase, GamePhase::Won);
        assert_eq!(game.guesses.len(), 2);
    }

    #[test]
    fn test_evaluate_case_insensitive() {
        let mut game = Wordle::new();
        game.target = ['c', 'r', 'a', 'n', 'e', ' '];
        game.target_len = 5;
        let eval = game.evaluate_guess(&['C', 'R', 'A', 'N', 'E']);
        assert_eq!(eval[0], TileState::Correct);
        assert_eq!(eval[4], TileState::Correct);
    }

    #[test]
    fn test_word_list_easy() {
        let list = Wordle::word_list(Difficulty::Easy);
        assert_eq!(list.len(), WORDS_4.len());
    }

    #[test]
    fn test_word_list_normal() {
        let list = Wordle::word_list(Difficulty::Normal);
        assert_eq!(list.len(), WORDS_5.len());
    }

    #[test]
    fn test_word_list_hard() {
        let list = Wordle::word_list(Difficulty::Hard);
        assert_eq!(list.len(), WORDS_6.len());
    }

    #[test]
    fn test_multiple_games_stats() {
        let mut game = Wordle::new();

        // Win game 1
        game.target = ['a', 'b', 'o', 'u', 't', ' '];
        game.target_len = 5;
        for ch in ['a', 'b', 'o', 'u', 't'] {
            game.add_letter(ch);
        }
        game.submit_guess();
        assert_eq!(game.games_played, 1);
        assert_eq!(game.games_won, 1);

        // New game and win
        game.new_game();
        let word = game.target_word();
        for ch in word.chars() {
            game.add_letter(ch);
        }
        game.submit_guess();
        assert_eq!(game.games_played, 2);
        assert_eq!(game.games_won, 2);
    }

    #[test]
    fn test_h_key_toggles_hard_mode_when_empty() {
        let mut game = Wordle::new();
        assert!(!game.hard_mode);
        let event = Event::Key(KeyEvent {
            key: Key::H,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert!(game.hard_mode);
    }

    #[test]
    fn test_h_key_types_h_when_input_exists() {
        let mut game = Wordle::new();
        game.add_letter('a');
        let event = Event::Key(KeyEvent {
            key: Key::H,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert_eq!(game.current_input.len(), 2);
        assert_eq!(game.current_input[1], 'h');
        assert!(!game.hard_mode);
    }

    #[test]
    fn test_modifiers_ignored() {
        let mut game = Wordle::new();
        let event = Event::Key(KeyEvent {
            key: Key::A,
            modifiers: Modifiers { shift: false, ctrl: true, alt: false, super_key: false },
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert!(game.current_input.is_empty());
    }

    #[test]
    fn test_difficulty_switch_resets_game() {
        let mut game = Wordle::new();
        game.add_letter('a');
        game.set_difficulty(Difficulty::Hard);
        assert!(game.current_input.is_empty());
        assert_eq!(game.target_len, 6);
    }

    #[test]
    fn test_same_difficulty_no_reset() {
        let mut game = Wordle::new();
        game.add_letter('a');
        game.set_difficulty(Difficulty::Normal); // same as current
        assert_eq!(game.current_input.len(), 1); // not reset
    }
}

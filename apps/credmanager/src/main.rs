//! OurOS Credential Manager
//!
//! A secure password and credential management application for OurOS.
//! Stores and organizes passwords, secure notes, credit cards, identities,
//! and SSH keys in an encrypted vault. Features include:
//!
//! - Multiple entry types (login, secure note, credit card, identity, SSH key)
//! - Password generator with configurable length, character sets, and modes
//! - Password strength meter with entropy calculation
//! - Folder and tag organization with favorites
//! - Search and filtering across all entries
//! - Auto-lock after configurable timeout
//! - Clipboard auto-clear after 30 seconds
//! - Password audit (weak, reused, old, missing TOTP)
//! - CSV export and serialized backup for migration
//!
//! Uses the guitk library for UI rendering with Catppuccin Mocha theme.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use guitk::color::Color;
use guitk::event::{Event, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::CornerRadii;

// =============================================================================
// Catppuccin Mocha palette
// =============================================================================
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// =============================================================================
// Constants
// =============================================================================
const SIDEBAR_WIDTH: f32 = 220.0;
const ENTRY_LIST_WIDTH: f32 = 320.0;
const TOOLBAR_HEIGHT: f32 = 48.0;
const ROW_HEIGHT: f32 = 52.0;
const ICON_SIZE: f32 = 20.0;
const DEFAULT_FONT_SIZE: f32 = 14.0;
const HEADING_FONT_SIZE: f32 = 18.0;
const SMALL_FONT_SIZE: f32 = 12.0;
const CORNER_RADIUS: f32 = 6.0;
const DEFAULT_AUTO_LOCK_MINUTES: u32 = 15;
const CLIPBOARD_CLEAR_SECONDS: u32 = 30;
const PASSWORD_OLD_DAYS: u64 = 90;
const WEAK_PASSWORD_LEN: usize = 8;

// =============================================================================
// Unique ID generation
// =============================================================================

/// Monotonically increasing ID generator for entries and folders.
struct IdGen {
    next: u64,
}

impl IdGen {
    fn new() -> Self {
        Self { next: 1 }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
}

// =============================================================================
// Entry types
// =============================================================================

/// The type of credential entry stored in the vault.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EntryType {
    Login,
    SecureNote,
    CreditCard,
    Identity,
    SshKey,
}

impl EntryType {
    fn label(self) -> &'static str {
        match self {
            Self::Login => "Login",
            Self::SecureNote => "Secure Note",
            Self::CreditCard => "Credit Card",
            Self::Identity => "Identity",
            Self::SshKey => "SSH Key",
        }
    }

    fn icon_char(self) -> &'static str {
        match self {
            Self::Login => "@",
            Self::SecureNote => "#",
            Self::CreditCard => "$",
            Self::Identity => "&",
            Self::SshKey => ">",
        }
    }

    fn badge_color(self) -> Color {
        match self {
            Self::Login => BLUE,
            Self::SecureNote => YELLOW,
            Self::CreditCard => PEACH,
            Self::Identity => GREEN,
            Self::SshKey => LAVENDER,
        }
    }

    fn all() -> &'static [EntryType] {
        &[
            Self::Login,
            Self::SecureNote,
            Self::CreditCard,
            Self::Identity,
            Self::SshKey,
        ]
    }
}

// =============================================================================
// Login fields
// =============================================================================

/// Login credential with site, username, password, URL, notes, TOTP.
#[derive(Clone, Debug)]
struct LoginData {
    site: String,
    username: String,
    password: String,
    url: String,
    notes: String,
    totp_secret: Option<String>,
}

impl LoginData {
    fn new(site: &str, username: &str, password: &str) -> Self {
        Self {
            site: site.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            url: String::new(),
            notes: String::new(),
            totp_secret: None,
        }
    }
}

// =============================================================================
// Secure note fields
// =============================================================================

/// Encrypted secure note with title and free-form content.
#[derive(Clone, Debug)]
struct SecureNoteData {
    title: String,
    content: String,
}

impl SecureNoteData {
    fn new(title: &str, content: &str) -> Self {
        Self {
            title: title.to_string(),
            content: content.to_string(),
        }
    }
}

// =============================================================================
// Credit card fields
// =============================================================================

/// Credit card entry with masked number, expiry, and cardholder name.
#[derive(Clone, Debug)]
struct CreditCardData {
    name: String,
    number_masked: String,
    expiry: String,
    cardholder: String,
    notes: String,
}

impl CreditCardData {
    fn new(name: &str, number_masked: &str, expiry: &str, cardholder: &str) -> Self {
        Self {
            name: name.to_string(),
            number_masked: number_masked.to_string(),
            expiry: expiry.to_string(),
            cardholder: cardholder.to_string(),
            notes: String::new(),
        }
    }

    /// Mask a card number, showing only last 4 digits.
    fn mask_number(full_number: &str) -> String {
        let digits: String = full_number.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() < 4 {
            return "*".repeat(digits.len());
        }
        let visible = digits.len().saturating_sub(4);
        let mut masked = "*".repeat(visible);
        if let Some(tail) = digits.get(visible..) {
            masked.push_str(tail);
        }
        masked
    }
}

// =============================================================================
// Identity fields
// =============================================================================

/// Personal identity entry with contact details.
#[derive(Clone, Debug)]
struct IdentityData {
    name: String,
    email: String,
    phone: String,
    address: String,
}

impl IdentityData {
    fn new(name: &str, email: &str) -> Self {
        Self {
            name: name.to_string(),
            email: email.to_string(),
            phone: String::new(),
            address: String::new(),
        }
    }
}

// =============================================================================
// SSH key fields
// =============================================================================

/// SSH key entry with fingerprint and public key.
#[derive(Clone, Debug)]
struct SshKeyData {
    name: String,
    fingerprint: String,
    public_key: String,
}

impl SshKeyData {
    fn new(name: &str, fingerprint: &str, public_key: &str) -> Self {
        Self {
            name: name.to_string(),
            fingerprint: fingerprint.to_string(),
            public_key: public_key.to_string(),
        }
    }
}

// =============================================================================
// Credential entry
// =============================================================================

/// The payload of an entry, varying by type.
#[derive(Clone, Debug)]
enum EntryData {
    Login(LoginData),
    SecureNote(SecureNoteData),
    CreditCard(CreditCardData),
    Identity(IdentityData),
    SshKey(SshKeyData),
}

impl EntryData {
    fn entry_type(&self) -> EntryType {
        match self {
            Self::Login(_) => EntryType::Login,
            Self::SecureNote(_) => EntryType::SecureNote,
            Self::CreditCard(_) => EntryType::CreditCard,
            Self::Identity(_) => EntryType::Identity,
            Self::SshKey(_) => EntryType::SshKey,
        }
    }

    /// Display name for the entry.
    fn display_name(&self) -> &str {
        match self {
            Self::Login(d) => &d.site,
            Self::SecureNote(d) => &d.title,
            Self::CreditCard(d) => &d.name,
            Self::Identity(d) => &d.name,
            Self::SshKey(d) => &d.name,
        }
    }

    /// Subtitle line (username, masked number, email, fingerprint).
    fn subtitle(&self) -> &str {
        match self {
            Self::Login(d) => &d.username,
            Self::SecureNote(_) => "",
            Self::CreditCard(d) => &d.number_masked,
            Self::Identity(d) => &d.email,
            Self::SshKey(d) => &d.fingerprint,
        }
    }

    /// Check if text matches a search query (case-insensitive).
    fn matches_search(&self, query: &str) -> bool {
        let q = query.to_ascii_lowercase();
        let name_match = self.display_name().to_ascii_lowercase().contains(&q);
        let sub_match = self.subtitle().to_ascii_lowercase().contains(&q);
        let extra = match self {
            Self::Login(d) => d.url.to_ascii_lowercase().contains(&q)
                || d.notes.to_ascii_lowercase().contains(&q),
            Self::SecureNote(d) => d.content.to_ascii_lowercase().contains(&q),
            Self::CreditCard(d) => d.cardholder.to_ascii_lowercase().contains(&q)
                || d.notes.to_ascii_lowercase().contains(&q),
            Self::Identity(d) => d.phone.to_ascii_lowercase().contains(&q)
                || d.address.to_ascii_lowercase().contains(&q),
            Self::SshKey(d) => d.public_key.to_ascii_lowercase().contains(&q),
        };
        name_match || sub_match || extra
    }

    /// Extract password if this is a login entry.
    fn password(&self) -> Option<&str> {
        match self {
            Self::Login(d) => Some(&d.password),
            _ => None,
        }
    }
}

/// A single credential entry in the vault.
#[derive(Clone, Debug)]
struct Entry {
    id: u64,
    data: EntryData,
    folder_id: Option<u64>,
    tags: Vec<String>,
    starred: bool,
    created_at: u64,
    modified_at: u64,
    /// Whether this password was flagged as compromised.
    compromised: bool,
}

impl Entry {
    fn new(id: u64, data: EntryData, timestamp: u64) -> Self {
        Self {
            id,
            data,
            folder_id: None,
            tags: Vec::new(),
            starred: false,
            created_at: timestamp,
            modified_at: timestamp,
            compromised: false,
        }
    }

    fn entry_type(&self) -> EntryType {
        self.data.entry_type()
    }

    fn display_name(&self) -> &str {
        self.data.display_name()
    }

    fn subtitle(&self) -> &str {
        self.data.subtitle()
    }

    /// Age of the password in days (from `now` timestamp).
    fn password_age_days(&self, now: u64) -> u64 {
        now.saturating_sub(self.modified_at) / 86400
    }
}

// =============================================================================
// Folder
// =============================================================================

/// A folder for organizing entries.
#[derive(Clone, Debug)]
struct Folder {
    id: u64,
    name: String,
    parent_id: Option<u64>,
}

impl Folder {
    fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            parent_id: None,
        }
    }
}

// =============================================================================
// Vault
// =============================================================================

/// Lock state of the vault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VaultState {
    Locked,
    Unlocked,
}

/// The vault holds all entries, folders, and metadata.
#[derive(Clone, Debug)]
struct Vault {
    name: String,
    state: VaultState,
    master_password_hash: u64,
    entries: Vec<Entry>,
    folders: Vec<Folder>,
    last_access: u64,
    auto_lock_minutes: u32,
    id_gen: u64,
}

impl Vault {
    fn new(name: &str, master_password: &str) -> Self {
        Self {
            name: name.to_string(),
            state: VaultState::Locked,
            master_password_hash: simple_hash(master_password),
            entries: Vec::new(),
            folders: Vec::new(),
            last_access: 0,
            auto_lock_minutes: DEFAULT_AUTO_LOCK_MINUTES,
            id_gen: 1,
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.id_gen;
        self.id_gen = self.id_gen.saturating_add(1);
        id
    }

    fn unlock(&mut self, password: &str, now: u64) -> bool {
        if simple_hash(password) == self.master_password_hash {
            self.state = VaultState::Unlocked;
            self.last_access = now;
            true
        } else {
            false
        }
    }

    fn lock(&mut self) {
        self.state = VaultState::Locked;
    }

    fn is_unlocked(&self) -> bool {
        self.state == VaultState::Unlocked
    }

    /// Check if auto-lock timeout has been exceeded.
    fn should_auto_lock(&self, now: u64) -> bool {
        if self.state == VaultState::Locked {
            return false;
        }
        let elapsed_seconds = now.saturating_sub(self.last_access);
        let timeout_seconds = u64::from(self.auto_lock_minutes) * 60;
        elapsed_seconds >= timeout_seconds
    }

    fn touch(&mut self, now: u64) {
        self.last_access = now;
    }

    // -- Entry CRUD ---------------------------------------------------------

    fn add_entry(&mut self, data: EntryData, now: u64) -> u64 {
        let id = self.next_id();
        self.entries.push(Entry::new(id, data, now));
        self.touch(now);
        id
    }

    fn remove_entry(&mut self, entry_id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != entry_id);
        self.entries.len() < before
    }

    fn get_entry(&self, entry_id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == entry_id)
    }

    fn get_entry_mut(&mut self, entry_id: u64) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.id == entry_id)
    }

    fn update_entry(&mut self, entry_id: u64, data: EntryData, now: u64) -> bool {
        if let Some(entry) = self.get_entry_mut(entry_id) {
            entry.data = data;
            entry.modified_at = now;
            true
        } else {
            false
        }
    }

    fn toggle_star(&mut self, entry_id: u64) -> bool {
        if let Some(entry) = self.get_entry_mut(entry_id) {
            entry.starred = !entry.starred;
            true
        } else {
            false
        }
    }

    fn set_compromised(&mut self, entry_id: u64, compromised: bool) -> bool {
        if let Some(entry) = self.get_entry_mut(entry_id) {
            entry.compromised = compromised;
            true
        } else {
            false
        }
    }

    fn add_tag(&mut self, entry_id: u64, tag: &str) -> bool {
        if let Some(entry) = self.get_entry_mut(entry_id) {
            let tag_str = tag.to_string();
            if !entry.tags.contains(&tag_str) {
                entry.tags.push(tag_str);
            }
            true
        } else {
            false
        }
    }

    fn remove_tag(&mut self, entry_id: u64, tag: &str) -> bool {
        if let Some(entry) = self.get_entry_mut(entry_id) {
            let before = entry.tags.len();
            entry.tags.retain(|t| t != tag);
            entry.tags.len() < before
        } else {
            false
        }
    }

    fn set_folder(&mut self, entry_id: u64, folder_id: Option<u64>) -> bool {
        if let Some(entry) = self.get_entry_mut(entry_id) {
            entry.folder_id = folder_id;
            true
        } else {
            false
        }
    }

    // -- Folder CRUD --------------------------------------------------------

    fn add_folder(&mut self, name: &str) -> u64 {
        let id = self.next_id();
        self.folders.push(Folder::new(id, name));
        id
    }

    fn remove_folder(&mut self, folder_id: u64) -> bool {
        let before = self.folders.len();
        self.folders.retain(|f| f.id != folder_id);
        // Unset folder_id on entries in this folder
        for entry in &mut self.entries {
            if entry.folder_id == Some(folder_id) {
                entry.folder_id = None;
            }
        }
        self.folders.len() < before
    }

    fn get_folder(&self, folder_id: u64) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == folder_id)
    }

    fn rename_folder(&mut self, folder_id: u64, new_name: &str) -> bool {
        if let Some(folder) = self.folders.iter_mut().find(|f| f.id == folder_id) {
            folder.name = new_name.to_string();
            true
        } else {
            false
        }
    }

    // -- Query helpers -------------------------------------------------------

    fn entries_in_folder(&self, folder_id: Option<u64>) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.folder_id == folder_id).collect()
    }

    fn starred_entries(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.starred).collect()
    }

    fn entries_with_tag(&self, tag: &str) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.tags.iter().any(|t| t == tag)).collect()
    }

    fn entries_of_type(&self, entry_type: EntryType) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.entry_type() == entry_type).collect()
    }

    fn search_entries(&self, query: &str) -> Vec<&Entry> {
        if query.is_empty() {
            return self.entries.iter().collect();
        }
        self.entries.iter().filter(|e| e.data.matches_search(query)).collect()
    }

    /// All unique tags across all entries.
    fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self.entries.iter()
            .flat_map(|e| e.tags.iter().cloned())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        tags.sort();
        tags
    }

    fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// Simple hash for demonstration (not cryptographically secure).
fn simple_hash(input: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in input.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(byte));
    }
    hash
}

// =============================================================================
// Password generator
// =============================================================================

/// Character set options for the password generator.
#[derive(Clone, Debug)]
struct CharsetOptions {
    uppercase: bool,
    lowercase: bool,
    digits: bool,
    symbols: bool,
}

impl Default for CharsetOptions {
    fn default() -> Self {
        Self {
            uppercase: true,
            lowercase: true,
            digits: true,
            symbols: true,
        }
    }
}

impl CharsetOptions {
    fn build_charset(&self) -> Vec<char> {
        let mut chars = Vec::new();
        if self.uppercase {
            chars.extend('A'..='Z');
        }
        if self.lowercase {
            chars.extend('a'..='z');
        }
        if self.digits {
            chars.extend('0'..='9');
        }
        if self.symbols {
            chars.extend("!@#$%^&*()-_=+[]{}|;:',.<>?/~`".chars());
        }
        chars
    }

    /// Count of distinct characters in the pool.
    fn pool_size(&self) -> usize {
        let mut count = 0;
        if self.uppercase { count += 26; }
        if self.lowercase { count += 26; }
        if self.digits { count += 10; }
        if self.symbols { count += 30; }
        count
    }
}

/// Password generation mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GeneratorMode {
    Random,
    Pronounceable,
    Passphrase,
}

/// Passphrase-mode settings.
#[derive(Clone, Debug)]
struct PassphraseOptions {
    word_count: usize,
    separator: String,
}

impl Default for PassphraseOptions {
    fn default() -> Self {
        Self {
            word_count: 4,
            separator: "-".to_string(),
        }
    }
}

/// The password generator with all settings.
#[derive(Clone, Debug)]
struct PasswordGenerator {
    length: usize,
    mode: GeneratorMode,
    charset: CharsetOptions,
    passphrase: PassphraseOptions,
    /// Seed for deterministic generation (incremented each use).
    seed: u64,
}

impl PasswordGenerator {
    fn new() -> Self {
        Self {
            length: 20,
            mode: GeneratorMode::Random,
            charset: CharsetOptions::default(),
            passphrase: PassphraseOptions::default(),
            seed: 12345,
        }
    }

    fn set_length(&mut self, len: usize) {
        self.length = len.clamp(8, 128);
    }

    /// Generate a password based on current settings.
    fn generate(&mut self) -> String {
        match self.mode {
            GeneratorMode::Random => self.generate_random(),
            GeneratorMode::Pronounceable => self.generate_pronounceable(),
            GeneratorMode::Passphrase => self.generate_passphrase(),
        }
    }

    fn generate_random(&mut self) -> String {
        let charset = self.charset.build_charset();
        if charset.is_empty() {
            return String::new();
        }
        let mut result = String::with_capacity(self.length);
        for i in 0..self.length {
            let idx = self.pseudo_random(charset.len(), i as u64);
            if let Some(&ch) = charset.get(idx) {
                result.push(ch);
            }
        }
        self.seed = self.seed.wrapping_add(1);
        result
    }

    fn generate_pronounceable(&mut self) -> String {
        let consonants = b"bcdfghjklmnpqrstvwxyz";
        let vowels = b"aeiou";
        let mut result = String::with_capacity(self.length);
        let mut use_consonant = true;
        for i in 0..self.length {
            let ch = if use_consonant {
                let idx = self.pseudo_random(consonants.len(), i as u64);
                consonants.get(idx).copied().unwrap_or(b'b')
            } else {
                let idx = self.pseudo_random(vowels.len(), i as u64);
                vowels.get(idx).copied().unwrap_or(b'a')
            };
            result.push(ch as char);
            use_consonant = !use_consonant;
        }
        self.seed = self.seed.wrapping_add(1);
        result
    }

    fn generate_passphrase(&mut self) -> String {
        let words = WORDLIST;
        let count = self.passphrase.word_count.max(2);
        let mut parts = Vec::with_capacity(count);
        for i in 0..count {
            let idx = self.pseudo_random(words.len(), i as u64);
            if let Some(&word) = words.get(idx) {
                parts.push(word.to_string());
            }
        }
        self.seed = self.seed.wrapping_add(1);
        parts.join(&self.passphrase.separator)
    }

    /// Simple deterministic pseudo-random for index selection.
    fn pseudo_random(&self, bound: usize, offset: u64) -> usize {
        if bound == 0 {
            return 0;
        }
        let mut x = self.seed.wrapping_add(offset).wrapping_mul(6364136223846793005);
        x = x.wrapping_add(1442695040888963407);
        x ^= x >> 16;
        x = x.wrapping_mul(0x45d9f3b);
        x ^= x >> 16;
        (x as usize) % bound
    }

    /// Calculate entropy in bits for the current settings.
    fn entropy_bits(&self) -> f64 {
        match self.mode {
            GeneratorMode::Random => {
                let pool = self.charset.pool_size();
                if pool == 0 {
                    return 0.0;
                }
                self.length as f64 * (pool as f64).log2()
            }
            GeneratorMode::Pronounceable => {
                // Alternating consonant/vowel: 21 * 5 per pair
                let pairs = self.length / 2;
                let remainder = self.length % 2;
                let bits_per_pair = (21.0_f64 * 5.0).log2();
                pairs as f64 * bits_per_pair + remainder as f64 * 21.0_f64.log2()
            }
            GeneratorMode::Passphrase => {
                let dict_size = WORDLIST.len();
                if dict_size == 0 {
                    return 0.0;
                }
                self.passphrase.word_count as f64 * (dict_size as f64).log2()
            }
        }
    }
}

/// Small word list for passphrase generation.
const WORDLIST: &[&str] = &[
    "abandon", "ability", "abstract", "account", "across", "action",
    "adapt", "address", "adjust", "advance", "afford", "agree",
    "airport", "alarm", "album", "alert", "alien", "allow",
    "almost", "alpha", "already", "alter", "amazing", "amount",
    "anchor", "angle", "animal", "annual", "answer", "antenna",
    "apart", "apple", "approve", "arena", "armor", "army",
    "arrange", "arrest", "arrive", "arrow", "artist", "aspect",
    "assist", "attack", "attract", "auction", "author", "avoid",
    "awake", "balance", "bamboo", "banner", "barely", "barrel",
    "basket", "battle", "beach", "beauty", "become", "before",
    "behind", "believe", "below", "bench", "benefit", "beyond",
    "bicycle", "binder", "blanket", "blast", "bless", "blind",
    "block", "blossom", "board", "border", "bottom", "bounce",
    "branch", "brave", "breeze", "bridge", "bright", "broken",
    "bronze", "brother", "brush", "bubble", "budget", "buffalo",
    "burden", "burst", "butter", "cabin", "cable", "camera",
    "cancel", "candle", "canvas", "capture", "carbon", "carpet",
    "castle", "casual", "catalog", "caution", "ceiling", "cement",
    "census", "center", "cereal", "certain", "chair", "change",
    "chapter", "cherry", "chimney", "choice", "chronic", "circle",
    "citizen", "civil", "claim", "clap", "clarify", "claw",
    "clever", "clinic", "clock", "cluster", "coach", "coconut",
    "coffee", "collect", "column", "comfort", "common", "company",
    "concert", "conduct", "confirm", "connect", "consider", "control",
    "convert", "copper", "coral", "correct", "costume", "cotton",
    "couch", "country", "couple", "cousin", "cover", "cradle",
    "craft", "crater", "crazy", "credit", "cricket", "crisis",
    "crisp", "cross", "crouch", "crowd", "crucial", "cruel",
    "cruise", "crystal", "culture", "curtain", "custom", "cycle",
    "damage", "dance", "danger", "daring", "daughter", "dawn",
    "debris", "decade", "decline", "decorate", "defense", "degree",
    "deliver", "demand", "denial", "dentist", "depart", "deposit",
    "depth", "derive", "desert", "design", "desktop", "destroy",
    "detail", "detect", "device", "devote", "diagram", "diamond",
    "diesel", "differ", "digital", "dinner", "direct", "discover",
    "display", "distance", "divert", "doctor", "dolphin", "domain",
    "donate", "double", "dragon", "drama", "dream", "dress",
    "drift", "drink", "driver", "drop", "durable", "during",
    "eagle", "early", "earth", "eclipse", "ecology", "economy",
    "educate", "effort", "eighth", "either", "elbow", "elder",
    "elegant", "element", "elephant", "elevator", "elite", "embark",
    "embrace", "emerge", "emotion", "emperor", "enable", "endless",
    "energy", "enforce", "engine", "enhance", "enjoy", "enough",
    "entire", "episode", "equal", "erosion", "escape", "essence",
    "estate", "eternal", "evening", "evidence", "evolve", "exact",
    "example", "excess", "exclude", "execute", "exhaust", "exhibit",
    "exotic", "expand", "expect", "explain", "expose", "extend",
    "extra", "fabric", "faculty", "fading", "failure", "falcon",
    "family", "fantasy", "fashion", "father", "feature", "federal",
    "fiction", "figure", "filter", "final", "finger", "finish",
    "fiscal", "fitness", "flavor", "flight", "float", "floor",
    "flower", "fluid", "flutter", "focus", "follow", "forest",
    "forget", "formal", "fortune", "fossil", "foster", "found",
    "fragile", "frame", "freedom", "freeze", "fresh", "friend",
    "frozen", "fruit", "future", "galaxy", "gallery", "garage",
    "garden", "garlic", "gather", "general", "genius", "gentle",
    "genuine", "gesture", "giant", "glacier", "glance", "glimpse",
    "global", "gloom", "glory", "glove", "goddess", "golden",
    "gossip", "govern", "grace", "grain", "grammar", "grant",
    "gravity", "great", "grocery", "ground", "group", "growing",
    "guard", "guitar", "hammer", "hamster", "harbor", "harvest",
    "hazard", "health", "heaven", "helmet", "hidden", "holiday",
    "hollow", "honey", "horror", "hospital", "hotel", "human",
    "humor", "hunter", "hybrid", "kingdom", "kitchen", "kiwi",
    "ladder", "language", "large", "later", "launch", "lava",
    "leader", "lecture", "legend", "leisure", "lemon", "length",
    "letter", "level", "liberty", "library", "license", "light",
    "limit", "linear", "liquid", "little", "lively", "lobby",
    "local", "logic", "lonely", "lottery", "luggage", "lumber",
    "lunar", "luxury", "machine", "magnet", "maiden", "major",
    "manage", "mandate", "manual", "maple", "marble", "margin",
    "marine", "market", "master", "matter", "meadow", "measure",
    "medium", "melody", "member", "memory", "mention", "mentor",
    "mercy", "method", "middle", "migrate", "million", "minimum",
    "mirror", "misery", "mission", "mixture", "mobile", "model",
    "modify", "moment", "monitor", "monkey", "monster", "moral",
    "morning", "motion", "mountain", "mouse", "muscle", "museum",
    "mushroom", "mutual", "mystery", "narrow", "nation", "nature",
    "nearby", "needle", "neither", "nephew", "nerve", "network",
    "neutral", "noble", "normal", "notable", "nothing", "notice",
    "novel", "number", "obvious", "ocean", "office", "olive",
    "opinion", "option", "orange", "orbit", "origin", "orphan",
    "outdoor", "output", "outside", "oxygen", "paddle", "palace",
    "panda", "panel", "panic", "parcel", "parent", "partner",
    "pattern", "pebble", "penalty", "people", "perfect", "permit",
    "person", "phrase", "picture", "pilot", "pioneer", "pirate",
    "planet", "plastic", "player", "please", "pledge", "plunge",
    "pocket", "poetry", "pointer", "polar", "policy", "popular",
    "portion", "poverty", "powder", "praise", "predict", "prepare",
    "present", "pretty", "prevent", "primary", "print", "prison",
    "private", "problem", "process", "produce", "profile", "program",
    "project", "promote", "prosper", "protect", "proud", "provide",
    "public", "purpose", "puzzle", "pyramid", "quality", "quantum",
    "quarter", "question", "quickly", "rabbit", "raccoon", "radar",
    "random", "rapid", "rather", "raven", "reason", "rebel",
    "recall", "receive", "record", "reform", "region", "regret",
    "reject", "release", "relief", "remain", "remind", "remove",
    "render", "repair", "repeat", "replace", "report", "require",
    "rescue", "resist", "resolve", "result", "retire", "retreat",
    "return", "reveal", "review", "reward", "rhythm", "ribbon",
    "right", "ritual", "river", "robust", "rocket", "romance",
    "roster", "rotate", "royal", "rubber", "runway", "saddle",
    "safari", "salmon", "salute", "sample", "satisfy", "scatter",
    "scene", "scheme", "school", "science", "scissors", "search",
    "season", "secret", "section", "security", "select", "seller",
    "senior", "series", "service", "session", "settle", "shadow",
    "shallow", "shelter", "sheriff", "shield", "shimmer", "shiver",
    "shock", "shoulder", "shuffle", "sibling", "signal", "silent",
    "silver", "similar", "simple", "sister", "situation", "sketch",
    "skull", "slender", "slight", "slogan", "smart", "smooth",
    "snack", "soccer", "social", "soldier", "solution", "someone",
    "source", "spatial", "special", "sphere", "spirit", "sponsor",
    "spring", "squeeze", "stable", "stadium", "staff", "stage",
    "stamp", "stand", "start", "state", "station", "steady",
    "stereo", "stick", "stomach", "story", "strategy", "street",
    "strong", "student", "studio", "subject", "submit", "sudden",
    "suffer", "suggest", "summer", "sunrise", "super", "supply",
    "surface", "surplus", "surprise", "surround", "survey", "suspect",
    "sustain", "symbol", "system", "table", "tackle", "talent",
    "target", "tattoo", "teacher", "tenant", "tennis", "terminal",
    "texture", "theory", "therapy", "thrive", "thunder", "ticket",
    "timber", "tissue", "title", "toast", "tobacco", "today",
    "together", "tomato", "tomorrow", "tongue", "topic", "tornado",
    "tortoise", "tourist", "toward", "tower", "traffic", "tragedy",
    "train", "transfer", "travel", "treasure", "trend", "trial",
    "trigger", "triple", "trophy", "trouble", "truck", "truly",
    "trumpet", "trust", "tunnel", "turtle", "twelve", "twenty",
    "typical", "umbrella", "unable", "uncle", "under", "unfold",
    "unique", "universe", "unknown", "unlock", "unusual", "upgrade",
    "uphold", "upper", "urban", "useful", "usual", "utility",
    "vacant", "vacuum", "valley", "valve", "vanish", "vapor",
    "various", "vendor", "venture", "verify", "version", "vessel",
    "veteran", "victory", "video", "village", "vintage", "violin",
    "virtual", "virus", "vision", "visual", "vivid", "vocal",
    "volcano", "voltage", "volume", "voyage", "wagon", "warrior",
    "wealth", "weapon", "weather", "welcome", "western", "whisper",
    "widen", "wildlife", "window", "winter", "wisdom", "witness",
    "wonder", "world", "wreath", "wrestle", "wrist", "yellow",
    "yield", "young", "zebra", "zero", "zigzag", "zombie",
    "zone",
];

// =============================================================================
// Password strength
// =============================================================================

/// Password strength level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PasswordStrength {
    VeryWeak,
    Weak,
    Fair,
    Strong,
    VeryStrong,
}

impl PasswordStrength {
    fn label(self) -> &'static str {
        match self {
            Self::VeryWeak => "Very Weak",
            Self::Weak => "Weak",
            Self::Fair => "Fair",
            Self::Strong => "Strong",
            Self::VeryStrong => "Very Strong",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::VeryWeak => RED,
            Self::Weak => PEACH,
            Self::Fair => YELLOW,
            Self::Strong => GREEN,
            Self::VeryStrong => LAVENDER,
        }
    }

    fn fraction(self) -> f32 {
        match self {
            Self::VeryWeak => 0.15,
            Self::Weak => 0.35,
            Self::Fair => 0.55,
            Self::Strong => 0.75,
            Self::VeryStrong => 1.0,
        }
    }
}

/// Evaluate password strength based on entropy estimation.
fn evaluate_password_strength(password: &str) -> (PasswordStrength, f64) {
    if password.is_empty() {
        return (PasswordStrength::VeryWeak, 0.0);
    }

    let len = password.len();
    let mut pool_size: usize = 0;
    let mut has_lower = false;
    let mut has_upper = false;
    let mut has_digit = false;
    let mut has_symbol = false;

    for ch in password.chars() {
        if ch.is_ascii_lowercase() {
            has_lower = true;
        } else if ch.is_ascii_uppercase() {
            has_upper = true;
        } else if ch.is_ascii_digit() {
            has_digit = true;
        } else {
            has_symbol = true;
        }
    }

    if has_lower { pool_size += 26; }
    if has_upper { pool_size += 26; }
    if has_digit { pool_size += 10; }
    if has_symbol { pool_size += 30; }

    let entropy = if pool_size > 0 {
        len as f64 * (pool_size as f64).log2()
    } else {
        0.0
    };

    let strength = if entropy < 28.0 {
        PasswordStrength::VeryWeak
    } else if entropy < 36.0 {
        PasswordStrength::Weak
    } else if entropy < 60.0 {
        PasswordStrength::Fair
    } else if entropy < 80.0 {
        PasswordStrength::Strong
    } else {
        PasswordStrength::VeryStrong
    };

    (strength, entropy)
}

/// Common password patterns that are always considered weak.
fn is_common_pattern(password: &str) -> bool {
    let lower = password.to_ascii_lowercase();
    let common = [
        "password", "123456", "qwerty", "letmein", "admin",
        "welcome", "monkey", "master", "dragon", "login",
        "abc123", "111111", "iloveyou", "sunshine", "princess",
        "football", "shadow", "trustno1", "baseball", "access",
    ];
    common.iter().any(|c| lower.contains(c))
}

// =============================================================================
// Password audit
// =============================================================================

/// Result of auditing a single entry.
#[derive(Clone, Debug)]
struct AuditIssue {
    entry_id: u64,
    entry_name: String,
    issue: AuditIssueKind,
}

/// The kind of audit issue found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuditIssueKind {
    WeakPassword,
    ReusedPassword,
    OldPassword,
    NoTotp,
    Compromised,
    CommonPattern,
}

impl AuditIssueKind {
    fn label(self) -> &'static str {
        match self {
            Self::WeakPassword => "Weak password",
            Self::ReusedPassword => "Reused password",
            Self::OldPassword => "Old password",
            Self::NoTotp => "No TOTP configured",
            Self::Compromised => "Compromised",
            Self::CommonPattern => "Common pattern",
        }
    }

    fn severity_color(self) -> Color {
        match self {
            Self::WeakPassword | Self::Compromised | Self::CommonPattern => RED,
            Self::ReusedPassword | Self::OldPassword => YELLOW,
            Self::NoTotp => SUBTEXT0,
        }
    }
}

/// Run a full audit on the vault, returning all found issues.
fn audit_vault(vault: &Vault, now: u64) -> Vec<AuditIssue> {
    let mut issues = Vec::new();

    // Collect passwords for reuse detection
    let mut password_counts: HashMap<String, Vec<u64>> = HashMap::new();
    for entry in &vault.entries {
        if let Some(pw) = entry.data.password() {
            password_counts
                .entry(pw.to_string())
                .or_default()
                .push(entry.id);
        }
    }

    for entry in &vault.entries {
        let name = entry.display_name().to_string();

        // Compromised check
        if entry.compromised {
            issues.push(AuditIssue {
                entry_id: entry.id,
                entry_name: name.clone(),
                issue: AuditIssueKind::Compromised,
            });
        }

        if let Some(pw) = entry.data.password() {
            // Weak password check
            if pw.len() < WEAK_PASSWORD_LEN {
                issues.push(AuditIssue {
                    entry_id: entry.id,
                    entry_name: name.clone(),
                    issue: AuditIssueKind::WeakPassword,
                });
            }

            // Common pattern check
            if is_common_pattern(pw) {
                issues.push(AuditIssue {
                    entry_id: entry.id,
                    entry_name: name.clone(),
                    issue: AuditIssueKind::CommonPattern,
                });
            }

            // Reuse check
            if let Some(ids) = password_counts.get(pw)
                && ids.len() > 1 {
                    issues.push(AuditIssue {
                        entry_id: entry.id,
                        entry_name: name.clone(),
                        issue: AuditIssueKind::ReusedPassword,
                    });
                }

            // Old password check
            if entry.password_age_days(now) > PASSWORD_OLD_DAYS {
                issues.push(AuditIssue {
                    entry_id: entry.id,
                    entry_name: name.clone(),
                    issue: AuditIssueKind::OldPassword,
                });
            }

            // Missing TOTP
            if let EntryData::Login(ref login) = entry.data
                && login.totp_secret.is_none() {
                    issues.push(AuditIssue {
                        entry_id: entry.id,
                        entry_name: name.clone(),
                        issue: AuditIssueKind::NoTotp,
                    });
                }
        }
    }

    issues
}

// =============================================================================
// Import / Export
// =============================================================================

/// Export vault entries to CSV format.
fn export_csv(vault: &Vault) -> String {
    let mut csv = String::from("type,name,username,password,url,notes,tags,folder,starred\n");
    for entry in &vault.entries {
        let etype = entry.entry_type().label();
        let name = escape_csv(entry.display_name());
        let subtitle = escape_csv(entry.subtitle());
        let password = match &entry.data {
            EntryData::Login(d) => escape_csv(&d.password),
            _ => String::new(),
        };
        let url = match &entry.data {
            EntryData::Login(d) => escape_csv(&d.url),
            _ => String::new(),
        };
        let notes = match &entry.data {
            EntryData::Login(d) => escape_csv(&d.notes),
            EntryData::CreditCard(d) => escape_csv(&d.notes),
            _ => String::new(),
        };
        let tags = entry.tags.join(";");
        let folder = entry.folder_id
            .and_then(|fid| vault.get_folder(fid))
            .map_or(String::new(), |f| f.name.clone());
        let starred = if entry.starred { "true" } else { "false" };

        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            etype, name, subtitle, password, url, notes, tags, folder, starred,
        ));
    }
    csv
}

/// Escape a value for CSV output.
fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

/// Serialize vault to a backup string (simplified JSON-like format).
fn serialize_backup(vault: &Vault) -> String {
    let mut out = String::from("{\n  \"vault_name\": ");
    out.push_str(&format!("\"{}\",\n", vault.name));
    out.push_str(&format!("  \"entry_count\": {},\n", vault.entries.len()));
    out.push_str("  \"entries\": [\n");
    for (i, entry) in vault.entries.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"id\": {},\n", entry.id));
        out.push_str(&format!("      \"type\": \"{}\",\n", entry.entry_type().label()));
        out.push_str(&format!("      \"name\": \"{}\",\n", entry.display_name()));
        out.push_str(&format!("      \"starred\": {},\n", entry.starred));
        out.push_str(&format!("      \"compromised\": {},\n", entry.compromised));
        out.push_str(&format!("      \"created_at\": {},\n", entry.created_at));
        out.push_str(&format!("      \"modified_at\": {},\n", entry.modified_at));
        let tags_str: Vec<String> = entry.tags.iter().map(|t| format!("\"{}\"", t)).collect();
        out.push_str(&format!("      \"tags\": [{}]\n", tags_str.join(", ")));
        if i + 1 < vault.entries.len() {
            out.push_str("    },\n");
        } else {
            out.push_str("    }\n");
        }
    }
    out.push_str("  ],\n");
    out.push_str("  \"folders\": [\n");
    for (i, folder) in vault.folders.iter().enumerate() {
        out.push_str(&format!("    {{ \"id\": {}, \"name\": \"{}\" }}", folder.id, folder.name));
        if i + 1 < vault.folders.len() {
            out.push_str(",\n");
        } else {
            out.push('\n');
        }
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

// =============================================================================
// Sort order
// =============================================================================

/// Sort order for the entry list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortOrder {
    NameAsc,
    NameDesc,
    DateNewest,
    DateOldest,
    TypeAsc,
}

impl SortOrder {
    fn label(self) -> &'static str {
        match self {
            Self::NameAsc => "Name A-Z",
            Self::NameDesc => "Name Z-A",
            Self::DateNewest => "Newest",
            Self::DateOldest => "Oldest",
            Self::TypeAsc => "Type",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::NameAsc => Self::NameDesc,
            Self::NameDesc => Self::DateNewest,
            Self::DateNewest => Self::DateOldest,
            Self::DateOldest => Self::TypeAsc,
            Self::TypeAsc => Self::NameAsc,
        }
    }
}

fn sort_entries(entries: &mut [&Entry], order: SortOrder) {
    match order {
        SortOrder::NameAsc => entries.sort_by(|a, b| {
            a.display_name().to_ascii_lowercase().cmp(&b.display_name().to_ascii_lowercase())
        }),
        SortOrder::NameDesc => entries.sort_by(|a, b| {
            b.display_name().to_ascii_lowercase().cmp(&a.display_name().to_ascii_lowercase())
        }),
        SortOrder::DateNewest => entries.sort_by_key(|e| std::cmp::Reverse(e.modified_at)),
        SortOrder::DateOldest => entries.sort_by_key(|a| a.modified_at),
        SortOrder::TypeAsc => entries.sort_by(|a, b| {
            a.entry_type().label().cmp(b.entry_type().label())
        }),
    }
}

// =============================================================================
// Sidebar category
// =============================================================================

/// What the sidebar is currently showing / filtering by.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SidebarSelection {
    AllItems,
    Favorites,
    Folder(u64),
    Tag(String),
    TypeFilter(EntryType),
    Audit,
}

// =============================================================================
// View mode
// =============================================================================

/// Which panel is shown in the detail area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetailView {
    EntryDetail,
    PasswordGenerator,
    Settings,
    AuditReport,
}

// =============================================================================
// Clipboard state (simulated)
// =============================================================================

/// Tracks clipboard content and auto-clear timing.
#[derive(Clone, Debug)]
struct ClipboardState {
    content: Option<String>,
    copied_at: u64,
    auto_clear_seconds: u32,
}

impl ClipboardState {
    fn new() -> Self {
        Self {
            content: None,
            copied_at: 0,
            auto_clear_seconds: CLIPBOARD_CLEAR_SECONDS,
        }
    }

    fn copy(&mut self, text: &str, now: u64) {
        self.content = Some(text.to_string());
        self.copied_at = now;
    }

    fn should_clear(&self, now: u64) -> bool {
        if self.content.is_none() {
            return false;
        }
        now.saturating_sub(self.copied_at) >= u64::from(self.auto_clear_seconds)
    }

    fn clear(&mut self) {
        self.content = None;
    }

    fn tick(&mut self, now: u64) {
        if self.should_clear(now) {
            self.clear();
        }
    }
}

// =============================================================================
// Application state
// =============================================================================

/// Top-level application state.
struct AppState {
    vault: Vault,
    sidebar_selection: SidebarSelection,
    selected_entry_id: Option<u64>,
    detail_view: DetailView,
    search_query: String,
    sort_order: SortOrder,
    password_generator: PasswordGenerator,
    generated_password: String,
    clipboard: ClipboardState,
    show_password: bool,
    now: u64,
    /// Filtered and sorted entry IDs for the list.
    filtered_ids: Vec<u64>,
    /// Cached audit results.
    audit_issues: Vec<AuditIssue>,
    /// Master password input buffer (for unlock screen).
    master_input: String,
    /// Whether the unlock attempt failed.
    unlock_failed: bool,
    /// Scroll offset for the entry list.
    list_scroll: f32,
    /// Scroll offset for the detail panel.
    detail_scroll: f32,
    /// Settings: auto-lock minutes.
    settings_auto_lock: u32,
}

impl AppState {
    fn new() -> Self {
        let vault = Vault::new("My Vault", "master123");
        let mut state = Self {
            vault,
            sidebar_selection: SidebarSelection::AllItems,
            selected_entry_id: None,
            detail_view: DetailView::EntryDetail,
            search_query: String::new(),
            sort_order: SortOrder::NameAsc,
            password_generator: PasswordGenerator::new(),
            generated_password: String::new(),
            clipboard: ClipboardState::new(),
            show_password: false,
            now: 1000000,
            filtered_ids: Vec::new(),
            audit_issues: Vec::new(),
            master_input: String::new(),
            unlock_failed: false,
            list_scroll: 0.0,
            detail_scroll: 0.0,
            settings_auto_lock: DEFAULT_AUTO_LOCK_MINUTES,
        };
        state.refresh_filter();
        state
    }

    /// Rebuild the filtered entry list from current sidebar + search + sort.
    fn refresh_filter(&mut self) {
        let mut entries: Vec<&Entry> = match &self.sidebar_selection {
            SidebarSelection::AllItems => self.vault.entries.iter().collect(),
            SidebarSelection::Favorites => self.vault.starred_entries(),
            SidebarSelection::Folder(fid) => self.vault.entries_in_folder(Some(*fid)),
            SidebarSelection::Tag(tag) => self.vault.entries_with_tag(tag),
            SidebarSelection::TypeFilter(et) => self.vault.entries_of_type(*et),
            SidebarSelection::Audit => self.vault.entries.iter().collect(),
        };

        // Apply search filter
        if !self.search_query.is_empty() {
            entries.retain(|e| e.data.matches_search(&self.search_query));
        }

        sort_entries(&mut entries, self.sort_order);
        self.filtered_ids = entries.iter().map(|e| e.id).collect();
    }

    fn run_audit(&mut self) {
        self.audit_issues = audit_vault(&self.vault, self.now);
    }

    fn tick(&mut self, elapsed_ms: u64) {
        self.now = self.now.saturating_add(elapsed_ms / 1000);
        self.clipboard.tick(self.now);

        if self.vault.should_auto_lock(self.now) {
            self.vault.lock();
        }
    }
}

// =============================================================================
// Render helpers
// =============================================================================

/// Render a filled rounded rectangle.
fn draw_rect(
    rt: &mut RenderTree,
    x: f32, y: f32, w: f32, h: f32,
    color: Color, radius: f32,
) {
    rt.push(RenderCommand::FillRect {
        x, y, width: w, height: h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

/// Render a stroked rounded rectangle.
// 8 args: rect (x,y,w,h) + color + line_width + radius; introducing a wrapper
// struct would only add noise at every call site.
#[allow(clippy::too_many_arguments)]
fn draw_stroke_rect(
    rt: &mut RenderTree,
    x: f32, y: f32, w: f32, h: f32,
    color: Color, line_width: f32, radius: f32,
) {
    rt.push(RenderCommand::StrokeRect {
        x, y, width: w, height: h,
        color, line_width,
        corner_radii: CornerRadii::all(radius),
    });
}

/// Render text at a position.
// 8 args mirror the underlying Text render command; same shape on purpose.
#[allow(clippy::too_many_arguments)]
fn draw_text(
    rt: &mut RenderTree,
    x: f32, y: f32,
    text: &str, color: Color, size: f32,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    rt.push(RenderCommand::Text {
        x, y,
        text: text.to_string(),
        color, font_size: size,
        font_weight: weight,
        max_width,
    });
}

/// Render a horizontal separator line.
fn draw_separator(rt: &mut RenderTree, x: f32, y: f32, width: f32) {
    rt.push(RenderCommand::Line {
        x1: x, y1: y,
        x2: x + width, y2: y,
        color: SURFACE1,
        width: 1.0,
    });
}

/// Render a small colored badge with text.
fn draw_badge(
    rt: &mut RenderTree,
    x: f32, y: f32,
    text: &str, bg: Color, fg: Color,
) {
    let text_width = text.len() as f32 * 7.0;
    let badge_w = text_width + 12.0;
    let badge_h = 20.0;
    draw_rect(rt, x, y, badge_w, badge_h, bg, 4.0);
    draw_text(rt, x + 6.0, y + 3.0, text, fg, SMALL_FONT_SIZE, FontWeightHint::Bold, None);
}

/// Render a toolbar-style button.
// 9 args: rect (x,y,w,h) + label + bg/fg colors + hovered flag; grouping these
// would not improve clarity at the call sites.
#[allow(clippy::too_many_arguments)]
fn draw_button(
    rt: &mut RenderTree,
    x: f32, y: f32, w: f32, h: f32,
    text: &str, bg: Color, fg: Color,
    hovered: bool,
) {
    let actual_bg = if hovered {
        Color::rgba(bg.r.saturating_add(20), bg.g.saturating_add(20), bg.b.saturating_add(20), bg.a)
    } else {
        bg
    };
    draw_rect(rt, x, y, w, h, actual_bg, CORNER_RADIUS);
    let text_x = x + (w - text.len() as f32 * 7.5) / 2.0;
    let text_y = y + (h - DEFAULT_FONT_SIZE) / 2.0;
    draw_text(rt, text_x, text_y, text, fg, DEFAULT_FONT_SIZE, FontWeightHint::Regular, None);
}

/// Render a progress/strength bar.
fn draw_strength_bar(
    rt: &mut RenderTree,
    x: f32, y: f32, width: f32, height: f32,
    fraction: f32, color: Color,
) {
    draw_rect(rt, x, y, width, height, SURFACE0, 3.0);
    let fill_width = (width * fraction.clamp(0.0, 1.0)).max(0.0);
    if fill_width > 0.0 {
        draw_rect(rt, x, y, fill_width, height, color, 3.0);
    }
}

// =============================================================================
// Render: toolbar
// =============================================================================

fn render_toolbar(rt: &mut RenderTree, state: &AppState, width: f32) {
    // Toolbar background
    draw_rect(rt, 0.0, 0.0, width, TOOLBAR_HEIGHT, MANTLE, 0.0);

    let btn_y = 8.0;
    let btn_h = 32.0;
    let mut x = SIDEBAR_WIDTH + 12.0;

    // Add button
    draw_button(rt, x, btn_y, 60.0, btn_h, "+ Add", BLUE, BASE, false);
    x += 72.0;

    // Search box
    draw_rect(rt, x, btn_y, 200.0, btn_h, SURFACE0, CORNER_RADIUS);
    let search_text = if state.search_query.is_empty() {
        "Search..."
    } else {
        &state.search_query
    };
    let search_color = if state.search_query.is_empty() { OVERLAY0 } else { TEXT_COLOR };
    draw_text(rt, x + 10.0, btn_y + 8.0, search_text, search_color, DEFAULT_FONT_SIZE,
              FontWeightHint::Regular, Some(180.0));
    x += 212.0;

    // Sort button
    draw_button(rt, x, btn_y, 80.0, btn_h, state.sort_order.label(), SURFACE1, TEXT_COLOR, false);
    x += 92.0;

    // Generate password button
    draw_button(rt, x, btn_y, 100.0, btn_h, "Generator", SURFACE1, LAVENDER, false);
    x += 112.0;

    // Lock button
    let lock_text = if state.vault.is_unlocked() { "Lock" } else { "Unlock" };
    let lock_color = if state.vault.is_unlocked() { GREEN } else { RED };
    draw_button(rt, x, btn_y, 70.0, btn_h, lock_text, SURFACE1, lock_color, false);
    x += 82.0;

    // Settings button
    draw_button(rt, x, btn_y, 80.0, btn_h, "Settings", SURFACE1, SUBTEXT0, false);

    // Bottom border
    draw_separator(rt, 0.0, TOOLBAR_HEIGHT - 1.0, width);
}

// =============================================================================
// Render: sidebar
// =============================================================================

fn render_sidebar(rt: &mut RenderTree, state: &AppState, height: f32) {
    let y_start = TOOLBAR_HEIGHT;
    let h = height - y_start;

    // Sidebar background
    draw_rect(rt, 0.0, y_start, SIDEBAR_WIDTH, h, MANTLE, 0.0);

    let mut y = y_start + 12.0;
    let item_h = 32.0;
    let text_x = 16.0;

    // Vault name header
    draw_text(rt, text_x, y, &state.vault.name, TEXT_COLOR, HEADING_FONT_SIZE,
              FontWeightHint::Bold, Some(SIDEBAR_WIDTH - 24.0));
    y += 30.0;

    let entry_count_text = format!("{} items", state.vault.entry_count());
    draw_text(rt, text_x, y, &entry_count_text, SUBTEXT0, SMALL_FONT_SIZE,
              FontWeightHint::Regular, None);
    y += 24.0;

    draw_separator(rt, 8.0, y, SIDEBAR_WIDTH - 16.0);
    y += 12.0;

    // Categories section
    draw_text(rt, text_x, y, "CATEGORIES", OVERLAY0, SMALL_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 20.0;

    // All Items
    let all_selected = state.sidebar_selection == SidebarSelection::AllItems;
    if all_selected {
        draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
    }
    draw_text(rt, text_x + 4.0, y + 8.0, "All Items", if all_selected { BLUE } else { TEXT_COLOR },
              DEFAULT_FONT_SIZE, FontWeightHint::Regular, None);
    y += item_h + 2.0;

    // Favorites
    let fav_selected = state.sidebar_selection == SidebarSelection::Favorites;
    if fav_selected {
        draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
    }
    draw_text(rt, text_x + 4.0, y + 8.0, "* Favorites",
              if fav_selected { YELLOW } else { TEXT_COLOR },
              DEFAULT_FONT_SIZE, FontWeightHint::Regular, None);
    y += item_h + 2.0;

    // Audit
    let audit_selected = state.sidebar_selection == SidebarSelection::Audit;
    if audit_selected {
        draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
    }
    draw_text(rt, text_x + 4.0, y + 8.0, "! Audit",
              if audit_selected { RED } else { TEXT_COLOR },
              DEFAULT_FONT_SIZE, FontWeightHint::Regular, None);
    y += item_h + 8.0;

    draw_separator(rt, 8.0, y, SIDEBAR_WIDTH - 16.0);
    y += 12.0;

    // Types section
    draw_text(rt, text_x, y, "TYPES", OVERLAY0, SMALL_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 20.0;

    for etype in EntryType::all() {
        let type_selected = state.sidebar_selection == SidebarSelection::TypeFilter(*etype);
        if type_selected {
            draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
        }
        let label = format!("{} {}", etype.icon_char(), etype.label());
        let color = if type_selected { etype.badge_color() } else { TEXT_COLOR };
        draw_text(rt, text_x + 4.0, y + 8.0, &label, color,
                  DEFAULT_FONT_SIZE, FontWeightHint::Regular, None);
        y += item_h + 2.0;
    }

    y += 6.0;
    draw_separator(rt, 8.0, y, SIDEBAR_WIDTH - 16.0);
    y += 12.0;

    // Folders section
    draw_text(rt, text_x, y, "FOLDERS", OVERLAY0, SMALL_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 20.0;

    for folder in &state.vault.folders {
        let folder_sel = state.sidebar_selection == SidebarSelection::Folder(folder.id);
        if folder_sel {
            draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
        }
        let color = if folder_sel { BLUE } else { TEXT_COLOR };
        draw_text(rt, text_x + 4.0, y + 8.0, &folder.name, color,
                  DEFAULT_FONT_SIZE, FontWeightHint::Regular, None);
        y += item_h + 2.0;
    }

    y += 6.0;
    draw_separator(rt, 8.0, y, SIDEBAR_WIDTH - 16.0);
    y += 12.0;

    // Tags section
    draw_text(rt, text_x, y, "TAGS", OVERLAY0, SMALL_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 20.0;

    let all_tags = state.vault.all_tags();
    for tag in &all_tags {
        let tag_sel = state.sidebar_selection == SidebarSelection::Tag(tag.clone());
        if tag_sel {
            draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
        }
        let color = if tag_sel { LAVENDER } else { TEXT_COLOR };
        draw_text(rt, text_x + 4.0, y + 8.0, tag, color,
                  DEFAULT_FONT_SIZE, FontWeightHint::Regular, None);
        y += item_h + 2.0;
    }

    // Right border
    rt.push(RenderCommand::Line {
        x1: SIDEBAR_WIDTH, y1: y_start,
        x2: SIDEBAR_WIDTH, y2: height,
        color: SURFACE1,
        width: 1.0,
    });
}

// =============================================================================
// Render: entry list
// =============================================================================

fn render_entry_list(rt: &mut RenderTree, state: &AppState, height: f32) {
    let x_start = SIDEBAR_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let h = height - y_start;

    // List background
    draw_rect(rt, x_start, y_start, ENTRY_LIST_WIDTH, h, BASE, 0.0);

    // List header
    let count_text = format!("{} entries", state.filtered_ids.len());
    draw_text(rt, x_start + 12.0, y_start + 10.0, &count_text, SUBTEXT0,
              SMALL_FONT_SIZE, FontWeightHint::Regular, None);

    let mut y = y_start + 32.0;

    rt.push(RenderCommand::PushClip {
        x: x_start, y: y_start,
        width: ENTRY_LIST_WIDTH, height: h,
    });

    let effective_y = y - state.list_scroll;

    for (i, &entry_id) in state.filtered_ids.iter().enumerate() {
        let row_y = effective_y + i as f32 * ROW_HEIGHT;

        // Skip rows outside visible area
        if row_y + ROW_HEIGHT < y_start || row_y > y_start + h {
            continue;
        }

        if let Some(entry) = state.vault.get_entry(entry_id) {
            let is_selected = state.selected_entry_id == Some(entry_id);

            // Row background
            if is_selected {
                draw_rect(rt, x_start + 4.0, row_y, ENTRY_LIST_WIDTH - 8.0, ROW_HEIGHT - 2.0,
                          SURFACE0, 4.0);
            }

            let text_x = x_start + 16.0;

            // Type icon badge
            let badge_color = entry.entry_type().badge_color();
            draw_rect(rt, text_x, row_y + 8.0, ICON_SIZE, ICON_SIZE, badge_color, 4.0);
            draw_text(rt, text_x + 4.0, row_y + 10.0, entry.entry_type().icon_char(),
                      BASE, SMALL_FONT_SIZE, FontWeightHint::Bold, None);

            // Entry name
            let name_color = if is_selected { BLUE } else { TEXT_COLOR };
            draw_text(rt, text_x + 28.0, row_y + 8.0, entry.display_name(), name_color,
                      DEFAULT_FONT_SIZE, FontWeightHint::Regular,
                      Some(ENTRY_LIST_WIDTH - 60.0));

            // Subtitle
            let sub = entry.subtitle();
            if !sub.is_empty() {
                draw_text(rt, text_x + 28.0, row_y + 28.0, sub, SUBTEXT0,
                          SMALL_FONT_SIZE, FontWeightHint::Regular,
                          Some(ENTRY_LIST_WIDTH - 80.0));
            }

            // Star indicator
            if entry.starred {
                draw_text(rt, x_start + ENTRY_LIST_WIDTH - 30.0, row_y + 8.0, "*", YELLOW,
                          DEFAULT_FONT_SIZE, FontWeightHint::Bold, None);
            }

            // Compromised indicator
            if entry.compromised {
                draw_text(rt, x_start + ENTRY_LIST_WIDTH - 48.0, row_y + 8.0, "!", RED,
                          DEFAULT_FONT_SIZE, FontWeightHint::Bold, None);
            }

            // Bottom separator
            draw_separator(rt, x_start + 12.0, row_y + ROW_HEIGHT - 2.0,
                           ENTRY_LIST_WIDTH - 24.0);
        }
    }

    // keep y used so it doesn't get an unused warning
    let _ = y;
    y = effective_y + state.filtered_ids.len() as f32 * ROW_HEIGHT;
    let _ = y;

    rt.push(RenderCommand::PopClip);

    // Right border
    let list_right = x_start + ENTRY_LIST_WIDTH;
    rt.push(RenderCommand::Line {
        x1: list_right, y1: y_start,
        x2: list_right, y2: height,
        color: SURFACE1,
        width: 1.0,
    });
}

// =============================================================================
// Render: entry detail panel
// =============================================================================

fn render_entry_detail(rt: &mut RenderTree, state: &AppState, width: f32, height: f32) {
    let x_start = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let panel_width = width - x_start;
    let panel_height = height - y_start;

    // Background
    draw_rect(rt, x_start, y_start, panel_width, panel_height, BASE, 0.0);

    let entry = match state.selected_entry_id
        .and_then(|id| state.vault.get_entry(id))
    {
        Some(e) => e,
        None => {
            // Empty state
            draw_text(rt, x_start + panel_width / 2.0 - 80.0, y_start + panel_height / 2.0,
                      "Select an entry", OVERLAY0, HEADING_FONT_SIZE,
                      FontWeightHint::Light, None);
            return;
        }
    };

    rt.push(RenderCommand::PushClip {
        x: x_start, y: y_start,
        width: panel_width, height: panel_height,
    });

    let pad = 24.0;
    let mut y = y_start + pad - state.detail_scroll;

    // Entry type badge + name
    let badge_color = entry.entry_type().badge_color();
    draw_badge(rt, x_start + pad, y, entry.entry_type().label(), badge_color, BASE);

    if entry.starred {
        draw_text(rt, x_start + pad + entry.entry_type().label().len() as f32 * 7.0 + 24.0,
                  y + 2.0, "* Starred", YELLOW, SMALL_FONT_SIZE,
                  FontWeightHint::Regular, None);
    }
    y += 30.0;

    draw_text(rt, x_start + pad, y, entry.display_name(), TEXT_COLOR, HEADING_FONT_SIZE,
              FontWeightHint::Bold, Some(panel_width - pad * 2.0));
    y += 28.0;

    if entry.compromised {
        draw_rect(rt, x_start + pad, y, panel_width - pad * 2.0, 28.0,
                  Color::rgba(RED.r, RED.g, RED.b, 40), 4.0);
        draw_text(rt, x_start + pad + 8.0, y + 6.0, "! This password may be compromised",
                  RED, DEFAULT_FONT_SIZE, FontWeightHint::Bold, None);
        y += 36.0;
    }

    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 16.0;

    // Render field rows based on entry type
    let field_label_x = x_start + pad;
    let field_value_x = x_start + pad + 120.0;
    let copy_btn_x = x_start + panel_width - pad - 50.0;
    let row_spacing = 36.0;

    match &entry.data {
        EntryData::Login(login) => {
            // Site
            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Site", &login.site, false);
            y += row_spacing;

            // Username
            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Username", &login.username, false);
            y += row_spacing;

            // Password
            let pw_display = if state.show_password {
                login.password.clone()
            } else {
                "*".repeat(login.password.len().min(20))
            };
            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Password", &pw_display, true);

            // Password strength
            let (strength, entropy) = evaluate_password_strength(&login.password);
            y += 8.0;
            draw_strength_bar(rt, field_value_x, y, 160.0, 6.0,
                              strength.fraction(), strength.color());
            let strength_text = format!("{} ({:.0} bits)", strength.label(), entropy);
            draw_text(rt, field_value_x + 170.0, y - 2.0, &strength_text,
                      strength.color(), SMALL_FONT_SIZE, FontWeightHint::Regular, None);
            y += row_spacing;

            // Show/hide toggle
            let toggle_text = if state.show_password { "Hide" } else { "Show" };
            draw_button(rt, field_value_x, y, 60.0, 24.0, toggle_text,
                        SURFACE1, TEXT_COLOR, false);
            y += row_spacing;

            // URL
            if !login.url.is_empty() {
                y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                        panel_width - pad * 2.0,
                                        "URL", &login.url, false);
                y += row_spacing;
            }

            // TOTP
            if let Some(ref totp) = login.totp_secret {
                y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                        panel_width - pad * 2.0,
                                        "TOTP", totp, false);
                y += row_spacing;
            } else {
                draw_text(rt, field_label_x, y, "TOTP", SUBTEXT0, DEFAULT_FONT_SIZE,
                          FontWeightHint::Regular, None);
                draw_text(rt, field_value_x, y, "Not configured", OVERLAY0, DEFAULT_FONT_SIZE,
                          FontWeightHint::Light, None);
                y += row_spacing;
            }

            // Notes
            if !login.notes.is_empty() {
                draw_separator(rt, field_label_x, y, panel_width - pad * 2.0);
                y += 12.0;
                draw_text(rt, field_label_x, y, "Notes", SUBTEXT0, DEFAULT_FONT_SIZE,
                          FontWeightHint::Bold, None);
                y += 20.0;
                draw_text(rt, field_label_x, y, &login.notes, TEXT_COLOR, DEFAULT_FONT_SIZE,
                          FontWeightHint::Regular, Some(panel_width - pad * 2.0));
                y += 24.0;
            }
        }
        EntryData::SecureNote(note) => {
            draw_text(rt, field_label_x, y, "Title", SUBTEXT0, DEFAULT_FONT_SIZE,
                      FontWeightHint::Regular, None);
            draw_text(rt, field_value_x, y, &note.title, TEXT_COLOR, DEFAULT_FONT_SIZE,
                      FontWeightHint::Regular, Some(panel_width - pad * 2.0 - 120.0));
            y += row_spacing;

            draw_separator(rt, field_label_x, y, panel_width - pad * 2.0);
            y += 12.0;

            draw_text(rt, field_label_x, y, &note.content, TEXT_COLOR, DEFAULT_FONT_SIZE,
                      FontWeightHint::Regular, Some(panel_width - pad * 2.0));
            y += 24.0;
        }
        EntryData::CreditCard(card) => {
            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Card Name", &card.name, false);
            y += row_spacing;

            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Number", &card.number_masked, false);
            y += row_spacing;

            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Expiry", &card.expiry, false);
            y += row_spacing;

            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Cardholder", &card.cardholder, false);
            y += row_spacing;

            if !card.notes.is_empty() {
                draw_separator(rt, field_label_x, y, panel_width - pad * 2.0);
                y += 12.0;
                draw_text(rt, field_label_x, y, "Notes", SUBTEXT0, DEFAULT_FONT_SIZE,
                          FontWeightHint::Bold, None);
                y += 20.0;
                draw_text(rt, field_label_x, y, &card.notes, TEXT_COLOR, DEFAULT_FONT_SIZE,
                          FontWeightHint::Regular, Some(panel_width - pad * 2.0));
                y += 24.0;
            }
        }
        EntryData::Identity(ident) => {
            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Name", &ident.name, false);
            y += row_spacing;

            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Email", &ident.email, false);
            y += row_spacing;

            if !ident.phone.is_empty() {
                y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                        panel_width - pad * 2.0,
                                        "Phone", &ident.phone, false);
                y += row_spacing;
            }

            if !ident.address.is_empty() {
                y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                        panel_width - pad * 2.0,
                                        "Address", &ident.address, false);
                y += row_spacing;
            }
        }
        EntryData::SshKey(key) => {
            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Key Name", &key.name, false);
            y += row_spacing;

            y = render_detail_field(rt, y, field_label_x, field_value_x, copy_btn_x,
                                    panel_width - pad * 2.0,
                                    "Fingerprint", &key.fingerprint, false);
            y += row_spacing;

            draw_text(rt, field_label_x, y, "Public Key", SUBTEXT0, DEFAULT_FONT_SIZE,
                      FontWeightHint::Regular, None);
            y += 20.0;

            draw_rect(rt, field_label_x, y, panel_width - pad * 2.0, 60.0,
                      SURFACE0, 4.0);
            draw_text(rt, field_label_x + 8.0, y + 8.0, &key.public_key, TEXT_COLOR,
                      SMALL_FONT_SIZE, FontWeightHint::Regular,
                      Some(panel_width - pad * 2.0 - 16.0));
            y += 68.0;
        }
    }

    // Tags section
    if !entry.tags.is_empty() {
        y += 8.0;
        draw_separator(rt, field_label_x, y, panel_width - pad * 2.0);
        y += 12.0;
        draw_text(rt, field_label_x, y, "Tags", SUBTEXT0, DEFAULT_FONT_SIZE,
                  FontWeightHint::Bold, None);
        y += 22.0;

        let mut tag_x = field_label_x;
        for tag in &entry.tags {
            let tag_w = tag.len() as f32 * 7.5 + 16.0;
            if tag_x + tag_w > x_start + panel_width - pad {
                tag_x = field_label_x;
                y += 26.0;
            }
            draw_badge(rt, tag_x, y, tag, SURFACE1, LAVENDER);
            tag_x += tag_w + 6.0;
        }
        y += 28.0;
    }

    // Metadata
    y += 8.0;
    draw_separator(rt, field_label_x, y, panel_width - pad * 2.0);
    y += 12.0;

    let created_text = format!("Created: {} seconds ago", state.now.saturating_sub(entry.created_at));
    draw_text(rt, field_label_x, y, &created_text, OVERLAY0, SMALL_FONT_SIZE,
              FontWeightHint::Regular, None);
    y += 18.0;

    let modified_text = format!("Modified: {} seconds ago", state.now.saturating_sub(entry.modified_at));
    draw_text(rt, field_label_x, y, &modified_text, OVERLAY0, SMALL_FONT_SIZE,
              FontWeightHint::Regular, None);

    if entry.entry_type() == EntryType::Login {
        y += 18.0;
        let age_days = entry.password_age_days(state.now);
        let age_color = if age_days > PASSWORD_OLD_DAYS { YELLOW } else { OVERLAY0 };
        let age_text = format!("Password age: {} days", age_days);
        draw_text(rt, field_label_x, y, &age_text, age_color, SMALL_FONT_SIZE,
                  FontWeightHint::Regular, None);
    }

    // Suppress unused y warning
    let _ = y;

    rt.push(RenderCommand::PopClip);
}

/// Render a single labeled field row with optional copy button.
// 9 args: layout positions (y, label_x, value_x, copy_x, width) + 2 strings +
// flag + render tree. All needed at the call site; no useful grouping.
#[allow(clippy::too_many_arguments)]
fn render_detail_field(
    rt: &mut RenderTree,
    y: f32,
    label_x: f32, value_x: f32, copy_x: f32,
    _width: f32,
    label: &str, value: &str,
    is_password: bool,
) -> f32 {
    draw_text(rt, label_x, y, label, SUBTEXT0, DEFAULT_FONT_SIZE,
              FontWeightHint::Regular, None);

    let value_color = if is_password { PEACH } else { TEXT_COLOR };
    draw_text(rt, value_x, y, value, value_color, DEFAULT_FONT_SIZE,
              FontWeightHint::Regular, Some(copy_x - value_x - 8.0));

    // Copy button
    draw_button(rt, copy_x, y - 4.0, 44.0, 24.0, "Copy", SURFACE1, SUBTEXT0, false);

    y
}

// =============================================================================
// Render: password generator panel
// =============================================================================

fn render_generator_panel(rt: &mut RenderTree, state: &AppState, width: f32, height: f32) {
    let x_start = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let panel_width = width - x_start;
    let panel_height = height - y_start;

    draw_rect(rt, x_start, y_start, panel_width, panel_height, BASE, 0.0);

    let pad = 24.0;
    let mut y = y_start + pad;

    draw_text(rt, x_start + pad, y, "Password Generator", TEXT_COLOR, HEADING_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 36.0;

    // Generated password display
    draw_rect(rt, x_start + pad, y, panel_width - pad * 2.0, 48.0, SURFACE0, CORNER_RADIUS);
    let display_pw = if state.generated_password.is_empty() {
        "Click Generate to create a password"
    } else {
        &state.generated_password
    };
    let pw_color = if state.generated_password.is_empty() { OVERLAY0 } else { GREEN };
    draw_text(rt, x_start + pad + 12.0, y + 14.0, display_pw, pw_color,
              DEFAULT_FONT_SIZE, FontWeightHint::Regular,
              Some(panel_width - pad * 2.0 - 24.0));
    y += 56.0;

    // Strength bar for generated password
    if !state.generated_password.is_empty() {
        let (strength, entropy) = evaluate_password_strength(&state.generated_password);
        draw_strength_bar(rt, x_start + pad, y, panel_width - pad * 2.0, 8.0,
                          strength.fraction(), strength.color());
        y += 16.0;
        let label = format!("{} - {:.0} bits entropy", strength.label(), entropy);
        draw_text(rt, x_start + pad, y, &label, strength.color(), SMALL_FONT_SIZE,
                  FontWeightHint::Regular, None);
        y += 24.0;
    }

    // Buttons row
    draw_button(rt, x_start + pad, y, 100.0, 32.0, "Generate", BLUE, BASE, false);
    draw_button(rt, x_start + pad + 112.0, y, 80.0, 32.0, "Copy", SURFACE1, TEXT_COLOR, false);
    y += 48.0;

    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 16.0;

    // Mode selection
    draw_text(rt, x_start + pad, y, "Mode", TEXT_COLOR, DEFAULT_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 24.0;

    let modes = [
        (GeneratorMode::Random, "Random"),
        (GeneratorMode::Pronounceable, "Pronounceable"),
        (GeneratorMode::Passphrase, "Passphrase"),
    ];
    let mut mode_x = x_start + pad;
    for (mode, label) in &modes {
        let is_active = state.password_generator.mode == *mode;
        let bg = if is_active { BLUE } else { SURFACE1 };
        let fg = if is_active { BASE } else { TEXT_COLOR };
        let btn_w = label.len() as f32 * 8.5 + 20.0;
        draw_button(rt, mode_x, y, btn_w, 28.0, label, bg, fg, false);
        mode_x += btn_w + 8.0;
    }
    y += 40.0;

    // Length setting
    draw_text(rt, x_start + pad, y, "Length", TEXT_COLOR, DEFAULT_FONT_SIZE,
              FontWeightHint::Regular, None);
    let len_text = format!("{}", state.password_generator.length);
    draw_text(rt, x_start + pad + 100.0, y, &len_text, BLUE, DEFAULT_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 8.0;

    // Length slider track
    let slider_x = x_start + pad;
    let slider_w = panel_width - pad * 2.0;
    let slider_y = y + 12.0;
    draw_rect(rt, slider_x, slider_y, slider_w, 4.0, SURFACE1, 2.0);

    let frac = (state.password_generator.length as f32 - 8.0) / 120.0;
    let knob_x = slider_x + slider_w * frac.clamp(0.0, 1.0);
    draw_rect(rt, knob_x - 6.0, slider_y - 4.0, 12.0, 12.0, BLUE, 6.0);
    y += 32.0;

    // Character set toggles (for random mode)
    if state.password_generator.mode == GeneratorMode::Random {
        draw_text(rt, x_start + pad, y, "Character Sets", TEXT_COLOR, DEFAULT_FONT_SIZE,
                  FontWeightHint::Bold, None);
        y += 24.0;

        let options = [
            ("Uppercase A-Z", state.password_generator.charset.uppercase),
            ("Lowercase a-z", state.password_generator.charset.lowercase),
            ("Digits 0-9", state.password_generator.charset.digits),
            ("Symbols !@#$", state.password_generator.charset.symbols),
        ];

        for (label, enabled) in &options {
            let check_color = if *enabled { GREEN } else { SURFACE2 };
            let check_char = if *enabled { "[x]" } else { "[ ]" };
            draw_text(rt, x_start + pad, y, check_char, check_color, DEFAULT_FONT_SIZE,
                      FontWeightHint::Regular, None);
            draw_text(rt, x_start + pad + 32.0, y, label, TEXT_COLOR, DEFAULT_FONT_SIZE,
                      FontWeightHint::Regular, None);
            y += 26.0;
        }
    }

    // Passphrase options
    if state.password_generator.mode == GeneratorMode::Passphrase {
        draw_text(rt, x_start + pad, y, "Word Count", TEXT_COLOR, DEFAULT_FONT_SIZE,
                  FontWeightHint::Regular, None);
        let wc_text = format!("{}", state.password_generator.passphrase.word_count);
        draw_text(rt, x_start + pad + 120.0, y, &wc_text, BLUE, DEFAULT_FONT_SIZE,
                  FontWeightHint::Bold, None);
        y += 26.0;

        draw_text(rt, x_start + pad, y, "Separator", TEXT_COLOR, DEFAULT_FONT_SIZE,
                  FontWeightHint::Regular, None);
        draw_text(rt, x_start + pad + 120.0, y, &state.password_generator.passphrase.separator,
                  BLUE, DEFAULT_FONT_SIZE, FontWeightHint::Bold, None);
        y += 26.0;
    }

    // Entropy info
    y += 8.0;
    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 12.0;

    let entropy = state.password_generator.entropy_bits();
    let entropy_text = format!("Estimated entropy: {:.1} bits", entropy);
    draw_text(rt, x_start + pad, y, &entropy_text, SUBTEXT0, SMALL_FONT_SIZE,
              FontWeightHint::Regular, None);
    y += 18.0;

    let pool_text = match state.password_generator.mode {
        GeneratorMode::Random => {
            format!("Pool size: {} characters", state.password_generator.charset.pool_size())
        }
        GeneratorMode::Pronounceable => "Pool: alternating consonant/vowel".to_string(),
        GeneratorMode::Passphrase => {
            format!("Dictionary: {} words", WORDLIST.len())
        }
    };
    draw_text(rt, x_start + pad, y, &pool_text, SUBTEXT0, SMALL_FONT_SIZE,
              FontWeightHint::Regular, None);

    let _ = y;
}

// =============================================================================
// Render: settings panel
// =============================================================================

fn render_settings_panel(rt: &mut RenderTree, state: &AppState, width: f32, height: f32) {
    let x_start = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let panel_width = width - x_start;
    let panel_height = height - y_start;

    draw_rect(rt, x_start, y_start, panel_width, panel_height, BASE, 0.0);

    let pad = 24.0;
    let mut y = y_start + pad;

    draw_text(rt, x_start + pad, y, "Settings", TEXT_COLOR, HEADING_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 36.0;

    // Security section
    draw_text(rt, x_start + pad, y, "SECURITY", OVERLAY0, SMALL_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 24.0;

    draw_text(rt, x_start + pad, y, "Auto-lock timeout", TEXT_COLOR, DEFAULT_FONT_SIZE,
              FontWeightHint::Regular, None);
    let timeout_text = format!("{} minutes", state.settings_auto_lock);
    draw_text(rt, x_start + pad + 200.0, y, &timeout_text, BLUE, DEFAULT_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 32.0;

    // Timeout slider
    let slider_x = x_start + pad;
    let slider_w = panel_width - pad * 2.0;
    draw_rect(rt, slider_x, y, slider_w, 4.0, SURFACE1, 2.0);
    let frac = (state.settings_auto_lock as f32 - 1.0) / 59.0;
    let knob_x = slider_x + slider_w * frac.clamp(0.0, 1.0);
    draw_rect(rt, knob_x - 6.0, y - 4.0, 12.0, 12.0, BLUE, 6.0);
    y += 24.0;

    draw_text(rt, x_start + pad, y, "Clipboard auto-clear", TEXT_COLOR, DEFAULT_FONT_SIZE,
              FontWeightHint::Regular, None);
    let clear_text = format!("{} seconds", state.clipboard.auto_clear_seconds);
    draw_text(rt, x_start + pad + 200.0, y, &clear_text, BLUE, DEFAULT_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 36.0;

    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 16.0;

    // Vault info section
    draw_text(rt, x_start + pad, y, "VAULT INFO", OVERLAY0, SMALL_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 24.0;

    let info_items = [
        ("Vault name", state.vault.name.as_str()),
        ("Status", if state.vault.is_unlocked() { "Unlocked" } else { "Locked" }),
    ];
    for (label, value) in &info_items {
        draw_text(rt, x_start + pad, y, label, SUBTEXT0, DEFAULT_FONT_SIZE,
                  FontWeightHint::Regular, None);
        draw_text(rt, x_start + pad + 160.0, y, value, TEXT_COLOR, DEFAULT_FONT_SIZE,
                  FontWeightHint::Regular, None);
        y += 26.0;
    }

    let count_text = format!("{}", state.vault.entries.len());
    draw_text(rt, x_start + pad, y, "Total entries", SUBTEXT0, DEFAULT_FONT_SIZE,
              FontWeightHint::Regular, None);
    draw_text(rt, x_start + pad + 160.0, y, &count_text, TEXT_COLOR, DEFAULT_FONT_SIZE,
              FontWeightHint::Regular, None);
    y += 26.0;

    let folder_count_text = format!("{}", state.vault.folders.len());
    draw_text(rt, x_start + pad, y, "Folders", SUBTEXT0, DEFAULT_FONT_SIZE,
              FontWeightHint::Regular, None);
    draw_text(rt, x_start + pad + 160.0, y, &folder_count_text, TEXT_COLOR, DEFAULT_FONT_SIZE,
              FontWeightHint::Regular, None);
    y += 36.0;

    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 16.0;

    // Export section
    draw_text(rt, x_start + pad, y, "DATA", OVERLAY0, SMALL_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 24.0;

    draw_button(rt, x_start + pad, y, 120.0, 32.0, "Export CSV", SURFACE1, TEXT_COLOR, false);
    draw_button(rt, x_start + pad + 132.0, y, 120.0, 32.0, "Backup", SURFACE1, TEXT_COLOR, false);

    let _ = y;
}

// =============================================================================
// Render: audit report panel
// =============================================================================

fn render_audit_panel(rt: &mut RenderTree, state: &AppState, width: f32, height: f32) {
    let x_start = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let panel_width = width - x_start;
    let panel_height = height - y_start;

    draw_rect(rt, x_start, y_start, panel_width, panel_height, BASE, 0.0);

    let pad = 24.0;
    let mut y = y_start + pad;

    draw_text(rt, x_start + pad, y, "Password Audit", TEXT_COLOR, HEADING_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 28.0;

    if state.audit_issues.is_empty() {
        draw_text(rt, x_start + pad, y, "No issues found. All passwords look good!",
                  GREEN, DEFAULT_FONT_SIZE, FontWeightHint::Regular, None);
        return;
    }

    let summary = format!("{} issues found", state.audit_issues.len());
    draw_text(rt, x_start + pad, y, &summary, YELLOW, DEFAULT_FONT_SIZE,
              FontWeightHint::Bold, None);
    y += 28.0;

    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 12.0;

    rt.push(RenderCommand::PushClip {
        x: x_start, y,
        width: panel_width, height: panel_height - (y - y_start),
    });

    for issue in &state.audit_issues {
        if y > y_start + panel_height {
            break;
        }

        let issue_color = issue.issue.severity_color();

        draw_rect(rt, x_start + pad, y, panel_width - pad * 2.0, 36.0,
                  SURFACE0, 4.0);

        // Issue severity badge
        draw_badge(rt, x_start + pad + 8.0, y + 8.0, issue.issue.label(),
                   issue_color, BASE);

        // Entry name
        let badge_width = issue.issue.label().len() as f32 * 7.0 + 20.0;
        draw_text(rt, x_start + pad + badge_width + 16.0, y + 10.0,
                  &issue.entry_name, TEXT_COLOR, DEFAULT_FONT_SIZE,
                  FontWeightHint::Regular,
                  Some(panel_width - pad * 2.0 - badge_width - 24.0));

        y += 42.0;
    }

    rt.push(RenderCommand::PopClip);
}

// =============================================================================
// Render: lock screen
// =============================================================================

fn render_lock_screen(rt: &mut RenderTree, state: &AppState, width: f32, height: f32) {
    // Full-screen overlay
    draw_rect(rt, 0.0, 0.0, width, height, MANTLE, 0.0);

    let center_x = width / 2.0;
    let center_y = height / 2.0;
    let panel_w = 360.0;
    let panel_h = 280.0;

    let px = center_x - panel_w / 2.0;
    let py = center_y - panel_h / 2.0;

    // Lock panel with shadow
    rt.push(RenderCommand::BoxShadow {
        x: px, y: py,
        width: panel_w, height: panel_h,
        offset_x: 0.0, offset_y: 4.0,
        blur: 24.0, spread: 0.0,
        color: Color::rgba(0, 0, 0, 100),
        corner_radii: CornerRadii::all(12.0),
    });
    draw_rect(rt, px, py, panel_w, panel_h, SURFACE0, 12.0);

    // Lock icon
    draw_text(rt, center_x - 10.0, py + 30.0, "[=]", BLUE, 24.0,
              FontWeightHint::Bold, None);

    // Vault name
    draw_text(rt, center_x - state.vault.name.len() as f32 * 5.0, py + 70.0,
              &state.vault.name, TEXT_COLOR, HEADING_FONT_SIZE,
              FontWeightHint::Bold, None);

    // Instruction
    draw_text(rt, center_x - 80.0, py + 100.0, "Enter master password",
              SUBTEXT0, DEFAULT_FONT_SIZE, FontWeightHint::Regular, None);

    // Password input field
    let input_x = px + 30.0;
    let input_y = py + 130.0;
    let input_w = panel_w - 60.0;
    let input_h = 40.0;

    let border_color = if state.unlock_failed { RED } else { SURFACE2 };
    draw_rect(rt, input_x, input_y, input_w, input_h, BASE, CORNER_RADIUS);
    draw_stroke_rect(rt, input_x, input_y, input_w, input_h, border_color, 1.0, CORNER_RADIUS);

    // Masked input display
    let masked: String = "*".repeat(state.master_input.len());
    let display = if masked.is_empty() { "Password..." } else { &masked };
    let display_color = if masked.is_empty() { OVERLAY0 } else { TEXT_COLOR };
    draw_text(rt, input_x + 12.0, input_y + 12.0, display, display_color,
              DEFAULT_FONT_SIZE, FontWeightHint::Regular, Some(input_w - 24.0));

    // Error message
    if state.unlock_failed {
        draw_text(rt, center_x - 60.0, input_y + input_h + 8.0,
                  "Incorrect password", RED, SMALL_FONT_SIZE,
                  FontWeightHint::Regular, None);
    }

    // Unlock button
    let btn_y = py + 200.0;
    draw_button(rt, center_x - 50.0, btn_y, 100.0, 36.0, "Unlock", BLUE, BASE, false);
}

// =============================================================================
// Build complete render tree
// =============================================================================

fn build_render_tree(state: &AppState, width: f32, height: f32) -> RenderTree {
    let mut rt = RenderTree::new();

    if !state.vault.is_unlocked() {
        render_lock_screen(&mut rt, state, width, height);
        return rt;
    }

    // Background
    draw_rect(&mut rt, 0.0, 0.0, width, height, BASE, 0.0);

    // Toolbar
    render_toolbar(&mut rt, state, width);

    // Sidebar
    render_sidebar(&mut rt, state, height);

    // Entry list
    render_entry_list(&mut rt, state, height);

    // Detail panel (depends on view)
    match state.detail_view {
        DetailView::EntryDetail => render_entry_detail(&mut rt, state, width, height),
        DetailView::PasswordGenerator => render_generator_panel(&mut rt, state, width, height),
        DetailView::Settings => render_settings_panel(&mut rt, state, width, height),
        DetailView::AuditReport => render_audit_panel(&mut rt, state, width, height),
    }

    rt
}

// =============================================================================
// Event handling
// =============================================================================

fn handle_event(state: &mut AppState, event: &Event) {
    match event {
        Event::Tick { elapsed_ms } => {
            state.tick(*elapsed_ms);
        }
        Event::Key(key_event) if key_event.pressed => {
            handle_key(state, key_event);
        }
        Event::Mouse(mouse_event) => {
            handle_mouse(state, mouse_event);
        }
        _ => {}
    }
}

fn handle_key(state: &mut AppState, key: &KeyEvent) {
    use guitk::event::Key;

    // Lock screen input
    if !state.vault.is_unlocked() {
        match key.key {
            Key::Enter => {
                let password = state.master_input.clone();
                if state.vault.unlock(&password, state.now) {
                    state.unlock_failed = false;
                    state.master_input.clear();
                    state.refresh_filter();
                } else {
                    state.unlock_failed = true;
                }
            }
            Key::Backspace => {
                state.master_input.pop();
                state.unlock_failed = false;
            }
            Key::Escape => {
                state.master_input.clear();
                state.unlock_failed = false;
            }
            _ => {
                if let Some(ch) = key.text
                    && !ch.is_control() {
                        state.master_input.push(ch);
                        state.unlock_failed = false;
                    }
            }
        }
        return;
    }

    // Main app key handling
    match key.key {
        Key::L if key.modifiers.ctrl => {
            state.vault.lock();
        }
        Key::F if key.modifiers.ctrl => {
            // Focus search (toggle)
            state.search_query.clear();
            state.refresh_filter();
        }
        Key::G if key.modifiers.ctrl => {
            state.detail_view = DetailView::PasswordGenerator;
            state.password_generator.seed = state.password_generator.seed.wrapping_add(1);
            state.generated_password = state.password_generator.generate();
        }
        Key::Escape => {
            state.search_query.clear();
            state.detail_view = DetailView::EntryDetail;
            state.refresh_filter();
        }
        Key::Up => {
            navigate_entry_list(state, -1);
        }
        Key::Down => {
            navigate_entry_list(state, 1);
        }
        Key::Enter => {
            if state.detail_view == DetailView::PasswordGenerator {
                state.generated_password = state.password_generator.generate();
            }
        }
        _ => {
            // Text input for search
            if let Some(ch) = key.text
                && !ch.is_control() {
                    state.search_query.push(ch);
                    state.refresh_filter();
                }
            if key.key == Key::Backspace && !state.search_query.is_empty() {
                state.search_query.pop();
                state.refresh_filter();
            }
        }
    }

    state.vault.touch(state.now);
}

fn navigate_entry_list(state: &mut AppState, direction: i32) {
    if state.filtered_ids.is_empty() {
        return;
    }

    let current_idx = state.selected_entry_id
        .and_then(|id| state.filtered_ids.iter().position(|&fid| fid == id));

    let new_idx = match current_idx {
        Some(idx) => {
            let new = idx as i32 + direction;
            new.clamp(0, state.filtered_ids.len() as i32 - 1) as usize
        }
        None => 0,
    };

    state.selected_entry_id = state.filtered_ids.get(new_idx).copied();
    state.detail_view = DetailView::EntryDetail;
    state.show_password = false;
}

fn handle_mouse(state: &mut AppState, mouse: &MouseEvent) {
    if !state.vault.is_unlocked() {
        return;
    }

    if let MouseEventKind::Press(MouseButton::Left) = mouse.kind {
        let mx = mouse.x;
        let my = mouse.y;

        // Check toolbar clicks
        if my < TOOLBAR_HEIGHT {
            handle_toolbar_click(state, mx);
            return;
        }

        // Check sidebar clicks
        if mx < SIDEBAR_WIDTH {
            handle_sidebar_click(state, my);
            return;
        }

        // Check entry list clicks
        if mx < SIDEBAR_WIDTH + ENTRY_LIST_WIDTH {
            handle_list_click(state, my);
            return;
        }

        // Detail panel clicks
        handle_detail_click(state, mx, my);
    }

    if let MouseEventKind::Scroll { dy, .. } = mouse.kind {
        if mouse.x >= SIDEBAR_WIDTH && mouse.x < SIDEBAR_WIDTH + ENTRY_LIST_WIDTH {
            state.list_scroll = (state.list_scroll - dy * 20.0).max(0.0);
        } else if mouse.x >= SIDEBAR_WIDTH + ENTRY_LIST_WIDTH {
            state.detail_scroll = (state.detail_scroll - dy * 20.0).max(0.0);
        }
    }

    state.vault.touch(state.now);
}

fn handle_toolbar_click(state: &mut AppState, mx: f32) {
    let base_x = SIDEBAR_WIDTH + 12.0;

    // Sort button region
    if mx >= base_x + 284.0 && mx < base_x + 364.0 {
        state.sort_order = state.sort_order.next();
        state.refresh_filter();
        return;
    }

    // Generator button region
    if mx >= base_x + 376.0 && mx < base_x + 476.0 {
        state.detail_view = DetailView::PasswordGenerator;
        if state.generated_password.is_empty() {
            state.generated_password = state.password_generator.generate();
        }
        return;
    }

    // Lock button region
    if mx >= base_x + 488.0 && mx < base_x + 558.0 {
        state.vault.lock();
        return;
    }

    // Settings button region
    if mx >= base_x + 570.0 && mx < base_x + 650.0 {
        state.detail_view = DetailView::Settings;
    }
}

fn handle_sidebar_click(state: &mut AppState, my: f32) {
    let y_start = TOOLBAR_HEIGHT;
    let item_h = 32.0;
    let mut y = y_start + 12.0 + 30.0 + 24.0 + 12.0 + 20.0;

    // All Items
    if my >= y && my < y + item_h {
        state.sidebar_selection = SidebarSelection::AllItems;
        state.refresh_filter();
        return;
    }
    y += item_h + 2.0;

    // Favorites
    if my >= y && my < y + item_h {
        state.sidebar_selection = SidebarSelection::Favorites;
        state.refresh_filter();
        return;
    }
    y += item_h + 2.0;

    // Audit
    if my >= y && my < y + item_h {
        state.sidebar_selection = SidebarSelection::Audit;
        state.detail_view = DetailView::AuditReport;
        state.run_audit();
        state.refresh_filter();
        return;
    }
    y += item_h + 8.0 + 12.0 + 20.0;

    // Types
    for etype in EntryType::all() {
        if my >= y && my < y + item_h {
            state.sidebar_selection = SidebarSelection::TypeFilter(*etype);
            state.refresh_filter();
            return;
        }
        y += item_h + 2.0;
    }
}

fn handle_list_click(state: &mut AppState, my: f32) {
    let y_start = TOOLBAR_HEIGHT + 32.0;
    let row_idx = ((my - y_start + state.list_scroll) / ROW_HEIGHT) as usize;

    if let Some(&entry_id) = state.filtered_ids.get(row_idx) {
        state.selected_entry_id = Some(entry_id);
        state.detail_view = DetailView::EntryDetail;
        state.show_password = false;
    }
}

fn handle_detail_click(state: &mut AppState, _mx: f32, _my: f32) {
    // Toggle show password when clicking in the detail area password field region
    let _ = state;
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // == IdGen tests ===========================================================

    #[test]
    fn test_id_gen_sequential() {
        let mut id_gen = IdGen::new();
        assert_eq!(id_gen.next_id(), 1);
        assert_eq!(id_gen.next_id(), 2);
        assert_eq!(id_gen.next_id(), 3);
    }

    #[test]
    fn test_id_gen_no_overflow() {
        let mut id_gen = IdGen { next: u64::MAX };
        let id = id_gen.next_id();
        assert_eq!(id, u64::MAX);
        // saturating_add prevents overflow
        let id2 = id_gen.next_id();
        assert_eq!(id2, u64::MAX);
    }

    // == EntryType tests =======================================================

    #[test]
    fn test_entry_type_label() {
        assert_eq!(EntryType::Login.label(), "Login");
        assert_eq!(EntryType::SecureNote.label(), "Secure Note");
        assert_eq!(EntryType::CreditCard.label(), "Credit Card");
        assert_eq!(EntryType::Identity.label(), "Identity");
        assert_eq!(EntryType::SshKey.label(), "SSH Key");
    }

    #[test]
    fn test_entry_type_icon() {
        assert_eq!(EntryType::Login.icon_char(), "@");
        assert_eq!(EntryType::SshKey.icon_char(), ">");
    }

    #[test]
    fn test_entry_type_all() {
        let all = EntryType::all();
        assert_eq!(all.len(), 5);
        assert!(all.contains(&EntryType::Login));
        assert!(all.contains(&EntryType::SshKey));
    }

    #[test]
    fn test_entry_type_badge_colors_distinct() {
        let colors: Vec<Color> = EntryType::all().iter().map(|t| t.badge_color()).collect();
        for i in 0..colors.len() {
            for j in i + 1..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }

    // == LoginData tests =======================================================

    #[test]
    fn test_login_data_new() {
        let d = LoginData::new("github.com", "user", "pass123");
        assert_eq!(d.site, "github.com");
        assert_eq!(d.username, "user");
        assert_eq!(d.password, "pass123");
        assert!(d.url.is_empty());
        assert!(d.notes.is_empty());
        assert!(d.totp_secret.is_none());
    }

    // == SecureNoteData tests ==================================================

    #[test]
    fn test_secure_note_new() {
        let n = SecureNoteData::new("My Note", "Secret content");
        assert_eq!(n.title, "My Note");
        assert_eq!(n.content, "Secret content");
    }

    // == CreditCardData tests ==================================================

    #[test]
    fn test_credit_card_new() {
        let c = CreditCardData::new("Visa", "****1234", "12/25", "John Doe");
        assert_eq!(c.name, "Visa");
        assert_eq!(c.number_masked, "****1234");
        assert_eq!(c.expiry, "12/25");
        assert_eq!(c.cardholder, "John Doe");
    }

    #[test]
    fn test_mask_number_normal() {
        assert_eq!(CreditCardData::mask_number("4111111111111111"), "************1111");
    }

    #[test]
    fn test_mask_number_short() {
        assert_eq!(CreditCardData::mask_number("123"), "***");
    }

    #[test]
    fn test_mask_number_with_spaces() {
        assert_eq!(CreditCardData::mask_number("4111 1111 1111 1111"), "************1111");
    }

    #[test]
    fn test_mask_number_exactly_four() {
        assert_eq!(CreditCardData::mask_number("1234"), "1234");
    }

    // == IdentityData tests ====================================================

    #[test]
    fn test_identity_new() {
        let d = IdentityData::new("Alice", "alice@example.com");
        assert_eq!(d.name, "Alice");
        assert_eq!(d.email, "alice@example.com");
        assert!(d.phone.is_empty());
        assert!(d.address.is_empty());
    }

    // == SshKeyData tests ======================================================

    #[test]
    fn test_ssh_key_new() {
        let k = SshKeyData::new("my-key", "SHA256:abc123", "ssh-ed25519 AAAA...");
        assert_eq!(k.name, "my-key");
        assert_eq!(k.fingerprint, "SHA256:abc123");
        assert!(k.public_key.starts_with("ssh-ed25519"));
    }

    // == EntryData tests =======================================================

    #[test]
    fn test_entry_data_type() {
        let login = EntryData::Login(LoginData::new("site", "user", "pass"));
        assert_eq!(login.entry_type(), EntryType::Login);

        let note = EntryData::SecureNote(SecureNoteData::new("title", "content"));
        assert_eq!(note.entry_type(), EntryType::SecureNote);
    }

    #[test]
    fn test_entry_data_display_name() {
        let login = EntryData::Login(LoginData::new("github.com", "user", "pass"));
        assert_eq!(login.display_name(), "github.com");

        let note = EntryData::SecureNote(SecureNoteData::new("My Note", ""));
        assert_eq!(note.display_name(), "My Note");
    }

    #[test]
    fn test_entry_data_subtitle() {
        let login = EntryData::Login(LoginData::new("site", "alice", "pass"));
        assert_eq!(login.subtitle(), "alice");

        let note = EntryData::SecureNote(SecureNoteData::new("title", "body"));
        assert_eq!(note.subtitle(), "");
    }

    #[test]
    fn test_entry_data_search_login() {
        let d = EntryData::Login(LoginData::new("GitHub", "alice", "pass"));
        assert!(d.matches_search("git"));
        assert!(d.matches_search("alice"));
        assert!(!d.matches_search("zzz"));
    }

    #[test]
    fn test_entry_data_search_case_insensitive() {
        let d = EntryData::Login(LoginData::new("GitHub", "Alice", "pass"));
        assert!(d.matches_search("GITHUB"));
        assert!(d.matches_search("aLiCe"));
    }

    #[test]
    fn test_entry_data_search_note() {
        let d = EntryData::SecureNote(SecureNoteData::new("Keys", "super secret 123"));
        assert!(d.matches_search("secret"));
        assert!(d.matches_search("keys"));
    }

    #[test]
    fn test_entry_data_search_credit_card() {
        let d = EntryData::CreditCard(CreditCardData::new("My Visa", "****1234", "12/25", "John"));
        assert!(d.matches_search("visa"));
        assert!(d.matches_search("john"));
    }

    #[test]
    fn test_entry_data_search_identity() {
        let mut id = IdentityData::new("Bob", "bob@test.com");
        id.phone = "555-1234".to_string();
        let d = EntryData::Identity(id);
        assert!(d.matches_search("bob"));
        assert!(d.matches_search("555"));
    }

    #[test]
    fn test_entry_data_search_ssh() {
        let d = EntryData::SshKey(SshKeyData::new("deploy", "SHA256:xyz", "ssh-rsa AAAA..."));
        assert!(d.matches_search("deploy"));
        assert!(d.matches_search("sha256"));
    }

    #[test]
    fn test_entry_data_password() {
        let login = EntryData::Login(LoginData::new("site", "user", "secret"));
        assert_eq!(login.password(), Some("secret"));

        let note = EntryData::SecureNote(SecureNoteData::new("title", "content"));
        assert_eq!(note.password(), None);
    }

    // == Entry tests ===========================================================

    #[test]
    fn test_entry_new() {
        let data = EntryData::Login(LoginData::new("site", "user", "pass"));
        let entry = Entry::new(1, data, 1000);
        assert_eq!(entry.id, 1);
        assert_eq!(entry.created_at, 1000);
        assert_eq!(entry.modified_at, 1000);
        assert!(!entry.starred);
        assert!(!entry.compromised);
        assert!(entry.tags.is_empty());
        assert!(entry.folder_id.is_none());
    }

    #[test]
    fn test_entry_password_age() {
        let data = EntryData::Login(LoginData::new("site", "user", "pass"));
        let entry = Entry::new(1, data, 1000);
        // 1 day = 86400 seconds
        assert_eq!(entry.password_age_days(1000 + 86400), 1);
        assert_eq!(entry.password_age_days(1000 + 86400 * 100), 100);
        assert_eq!(entry.password_age_days(1000), 0);
    }

    // == Folder tests ==========================================================

    #[test]
    fn test_folder_new() {
        let f = Folder::new(1, "Work");
        assert_eq!(f.id, 1);
        assert_eq!(f.name, "Work");
        assert!(f.parent_id.is_none());
    }

    // == simple_hash tests =====================================================

    #[test]
    fn test_simple_hash_deterministic() {
        assert_eq!(simple_hash("test"), simple_hash("test"));
    }

    #[test]
    fn test_simple_hash_different_inputs() {
        assert_ne!(simple_hash("abc"), simple_hash("def"));
    }

    #[test]
    fn test_simple_hash_empty() {
        let h = simple_hash("");
        assert_eq!(h, 5381);
    }

    // == Vault tests ===========================================================

    #[test]
    fn test_vault_new() {
        let v = Vault::new("Test", "password");
        assert_eq!(v.name, "Test");
        assert_eq!(v.state, VaultState::Locked);
        assert_eq!(v.auto_lock_minutes, DEFAULT_AUTO_LOCK_MINUTES);
    }

    #[test]
    fn test_vault_unlock_correct() {
        let mut v = Vault::new("Test", "secret");
        assert!(v.unlock("secret", 100));
        assert!(v.is_unlocked());
    }

    #[test]
    fn test_vault_unlock_incorrect() {
        let mut v = Vault::new("Test", "secret");
        assert!(!v.unlock("wrong", 100));
        assert!(!v.is_unlocked());
    }

    #[test]
    fn test_vault_lock() {
        let mut v = Vault::new("Test", "pw");
        v.unlock("pw", 100);
        assert!(v.is_unlocked());
        v.lock();
        assert!(!v.is_unlocked());
    }

    #[test]
    fn test_vault_auto_lock() {
        let mut v = Vault::new("Test", "pw");
        v.auto_lock_minutes = 5;
        v.unlock("pw", 100);
        assert!(!v.should_auto_lock(100));
        assert!(!v.should_auto_lock(399)); // 299s < 300s
        assert!(v.should_auto_lock(400)); // 300s >= 300s
    }

    #[test]
    fn test_vault_auto_lock_when_locked() {
        let v = Vault::new("Test", "pw");
        assert!(!v.should_auto_lock(99999));
    }

    #[test]
    fn test_vault_add_entry() {
        let mut v = Vault::new("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        assert!(id > 0);
        assert_eq!(v.entries.len(), 1);
        assert!(v.get_entry(id).is_some());
    }

    #[test]
    fn test_vault_remove_entry() {
        let mut v = Vault::new("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        assert!(v.remove_entry(id));
        assert_eq!(v.entries.len(), 0);
        assert!(!v.remove_entry(999));
    }

    #[test]
    fn test_vault_update_entry() {
        let mut v = Vault::new("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("old", "u", "p")), 100);
        let new_data = EntryData::Login(LoginData::new("new", "u2", "p2"));
        assert!(v.update_entry(id, new_data, 200));
        assert_eq!(v.get_entry(id).map(|e| e.display_name()), Some("new"));
        assert_eq!(v.get_entry(id).map(|e| e.modified_at), Some(200));
    }

    #[test]
    fn test_vault_toggle_star() {
        let mut v = Vault::new("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        assert!(!v.get_entry(id).is_some_and(|e| e.starred));
        v.toggle_star(id);
        assert!(v.get_entry(id).is_some_and(|e| e.starred));
        v.toggle_star(id);
        assert!(!v.get_entry(id).is_some_and(|e| e.starred));
    }

    #[test]
    fn test_vault_compromised() {
        let mut v = Vault::new("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        v.set_compromised(id, true);
        assert!(v.get_entry(id).is_some_and(|e| e.compromised));
        v.set_compromised(id, false);
        assert!(!v.get_entry(id).is_some_and(|e| e.compromised));
    }

    #[test]
    fn test_vault_tags() {
        let mut v = Vault::new("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        v.add_tag(id, "work");
        v.add_tag(id, "important");
        v.add_tag(id, "work"); // duplicate
        let entry = v.get_entry(id).expect("entry");
        assert_eq!(entry.tags.len(), 2);
        assert!(entry.tags.contains(&"work".to_string()));

        v.remove_tag(id, "work");
        let entry = v.get_entry(id).expect("entry");
        assert_eq!(entry.tags.len(), 1);
    }

    #[test]
    fn test_vault_set_folder() {
        let mut v = Vault::new("V", "pw");
        let fid = v.add_folder("Work");
        let eid = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        v.set_folder(eid, Some(fid));
        assert_eq!(v.get_entry(eid).map(|e| e.folder_id), Some(Some(fid)));
    }

    #[test]
    fn test_vault_add_folder() {
        let mut v = Vault::new("V", "pw");
        let id = v.add_folder("Personal");
        assert!(v.get_folder(id).is_some());
        assert_eq!(v.get_folder(id).map(|f| f.name.as_str()), Some("Personal"));
    }

    #[test]
    fn test_vault_remove_folder_clears_entries() {
        let mut v = Vault::new("V", "pw");
        let fid = v.add_folder("Work");
        let eid = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        v.set_folder(eid, Some(fid));
        v.remove_folder(fid);
        assert_eq!(v.get_entry(eid).map(|e| e.folder_id), Some(None));
    }

    #[test]
    fn test_vault_rename_folder() {
        let mut v = Vault::new("V", "pw");
        let fid = v.add_folder("Old");
        assert!(v.rename_folder(fid, "New"));
        assert_eq!(v.get_folder(fid).map(|f| f.name.as_str()), Some("New"));
    }

    #[test]
    fn test_vault_entries_in_folder() {
        let mut v = Vault::new("V", "pw");
        let fid = v.add_folder("Work");
        let id1 = v.add_entry(EntryData::Login(LoginData::new("s1", "u", "p")), 100);
        let _id2 = v.add_entry(EntryData::Login(LoginData::new("s2", "u", "p")), 100);
        v.set_folder(id1, Some(fid));
        assert_eq!(v.entries_in_folder(Some(fid)).len(), 1);
        assert_eq!(v.entries_in_folder(None).len(), 1);
    }

    #[test]
    fn test_vault_starred_entries() {
        let mut v = Vault::new("V", "pw");
        let id1 = v.add_entry(EntryData::Login(LoginData::new("s1", "u", "p")), 100);
        let _id2 = v.add_entry(EntryData::Login(LoginData::new("s2", "u", "p")), 100);
        v.toggle_star(id1);
        assert_eq!(v.starred_entries().len(), 1);
    }

    #[test]
    fn test_vault_entries_with_tag() {
        let mut v = Vault::new("V", "pw");
        let id1 = v.add_entry(EntryData::Login(LoginData::new("s1", "u", "p")), 100);
        v.add_tag(id1, "tag1");
        assert_eq!(v.entries_with_tag("tag1").len(), 1);
        assert_eq!(v.entries_with_tag("tag2").len(), 0);
    }

    #[test]
    fn test_vault_entries_of_type() {
        let mut v = Vault::new("V", "pw");
        v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        v.add_entry(EntryData::SecureNote(SecureNoteData::new("n", "c")), 100);
        assert_eq!(v.entries_of_type(EntryType::Login).len(), 1);
        assert_eq!(v.entries_of_type(EntryType::SecureNote).len(), 1);
        assert_eq!(v.entries_of_type(EntryType::CreditCard).len(), 0);
    }

    #[test]
    fn test_vault_search() {
        let mut v = Vault::new("V", "pw");
        v.add_entry(EntryData::Login(LoginData::new("github.com", "alice", "pass")), 100);
        v.add_entry(EntryData::Login(LoginData::new("gitlab.com", "bob", "pass")), 100);
        assert_eq!(v.search_entries("git").len(), 2);
        assert_eq!(v.search_entries("alice").len(), 1);
        assert_eq!(v.search_entries("").len(), 2);
        assert_eq!(v.search_entries("zzz").len(), 0);
    }

    #[test]
    fn test_vault_all_tags() {
        let mut v = Vault::new("V", "pw");
        let id1 = v.add_entry(EntryData::Login(LoginData::new("s1", "u", "p")), 100);
        let id2 = v.add_entry(EntryData::Login(LoginData::new("s2", "u", "p")), 100);
        v.add_tag(id1, "beta");
        v.add_tag(id1, "alpha");
        v.add_tag(id2, "alpha");
        let tags = v.all_tags();
        assert_eq!(tags, vec!["alpha", "beta"]);
    }

    // == CharsetOptions tests ==================================================

    #[test]
    fn test_charset_default() {
        let cs = CharsetOptions::default();
        assert!(cs.uppercase);
        assert!(cs.lowercase);
        assert!(cs.digits);
        assert!(cs.symbols);
    }

    #[test]
    fn test_charset_pool_size() {
        let cs = CharsetOptions::default();
        assert_eq!(cs.pool_size(), 26 + 26 + 10 + 30);
    }

    #[test]
    fn test_charset_pool_size_partial() {
        let cs = CharsetOptions {
            uppercase: true,
            lowercase: false,
            digits: true,
            symbols: false,
        };
        assert_eq!(cs.pool_size(), 36);
    }

    #[test]
    fn test_charset_build_empty() {
        let cs = CharsetOptions {
            uppercase: false,
            lowercase: false,
            digits: false,
            symbols: false,
        };
        assert!(cs.build_charset().is_empty());
        assert_eq!(cs.pool_size(), 0);
    }

    #[test]
    fn test_charset_build_has_expected_chars() {
        let cs = CharsetOptions {
            uppercase: true,
            lowercase: false,
            digits: false,
            symbols: false,
        };
        let chars = cs.build_charset();
        assert_eq!(chars.len(), 26);
        assert!(chars.contains(&'A'));
        assert!(chars.contains(&'Z'));
    }

    // == PasswordGenerator tests ===============================================

    #[test]
    fn test_generator_new() {
        let pg = PasswordGenerator::new();
        assert_eq!(pg.length, 20);
        assert_eq!(pg.mode, GeneratorMode::Random);
    }

    #[test]
    fn test_generator_set_length_clamp() {
        let mut pg = PasswordGenerator::new();
        pg.set_length(5);
        assert_eq!(pg.length, 8);
        pg.set_length(200);
        assert_eq!(pg.length, 128);
        pg.set_length(64);
        assert_eq!(pg.length, 64);
    }

    #[test]
    fn test_generator_random_length() {
        let mut pg = PasswordGenerator::new();
        pg.set_length(16);
        let pw = pg.generate();
        assert_eq!(pw.len(), 16);
    }

    #[test]
    fn test_generator_random_deterministic() {
        let mut gen1 = PasswordGenerator::new();
        gen1.seed = 42;
        gen1.set_length(20);
        let pw1 = gen1.generate();

        let mut gen2 = PasswordGenerator::new();
        gen2.seed = 42;
        gen2.set_length(20);
        let pw2 = gen2.generate();

        assert_eq!(pw1, pw2);
    }

    #[test]
    fn test_generator_random_empty_charset() {
        let mut pg = PasswordGenerator::new();
        pg.charset = CharsetOptions {
            uppercase: false,
            lowercase: false,
            digits: false,
            symbols: false,
        };
        let pw = pg.generate();
        assert!(pw.is_empty());
    }

    #[test]
    fn test_generator_pronounceable() {
        let mut pg = PasswordGenerator::new();
        pg.mode = GeneratorMode::Pronounceable;
        pg.set_length(10);
        let pw = pg.generate();
        assert_eq!(pw.len(), 10);
        // Should alternate consonant/vowel
        for (i, ch) in pw.chars().enumerate() {
            if i % 2 == 0 {
                assert!(!"aeiou".contains(ch), "Even idx should be consonant: {}", ch);
            } else {
                assert!("aeiou".contains(ch), "Odd idx should be vowel: {}", ch);
            }
        }
    }

    #[test]
    fn test_generator_passphrase() {
        let mut pg = PasswordGenerator::new();
        pg.mode = GeneratorMode::Passphrase;
        pg.passphrase.word_count = 4;
        pg.passphrase.separator = "-".to_string();
        let pw = pg.generate();
        let words: Vec<&str> = pw.split('-').collect();
        assert_eq!(words.len(), 4);
        for word in &words {
            assert!(WORDLIST.contains(word));
        }
    }

    #[test]
    fn test_generator_passphrase_custom_separator() {
        let mut pg = PasswordGenerator::new();
        pg.mode = GeneratorMode::Passphrase;
        pg.passphrase.word_count = 3;
        pg.passphrase.separator = ".".to_string();
        let pw = pg.generate();
        assert_eq!(pw.split('.').count(), 3);
    }

    #[test]
    fn test_generator_entropy_random() {
        let pg = PasswordGenerator::new();
        let entropy = pg.entropy_bits();
        // 20 chars from 92 pool: ~130 bits
        assert!(entropy > 100.0);
    }

    #[test]
    fn test_generator_entropy_passphrase() {
        let mut pg = PasswordGenerator::new();
        pg.mode = GeneratorMode::Passphrase;
        pg.passphrase.word_count = 4;
        let entropy = pg.entropy_bits();
        // 4 words from ~700 word list
        assert!(entropy > 30.0);
    }

    #[test]
    fn test_generator_entropy_empty_charset() {
        let mut pg = PasswordGenerator::new();
        pg.charset = CharsetOptions {
            uppercase: false, lowercase: false,
            digits: false, symbols: false,
        };
        assert_eq!(pg.entropy_bits(), 0.0);
    }

    #[test]
    fn test_generator_seed_advances() {
        let mut pg = PasswordGenerator::new();
        let s1 = pg.seed;
        pg.generate();
        let s2 = pg.seed;
        assert_ne!(s1, s2);
    }

    // == Password strength tests ===============================================

    #[test]
    fn test_strength_empty() {
        let (s, e) = evaluate_password_strength("");
        assert_eq!(s, PasswordStrength::VeryWeak);
        assert_eq!(e, 0.0);
    }

    #[test]
    fn test_strength_short() {
        let (s, _) = evaluate_password_strength("abc");
        assert_eq!(s, PasswordStrength::VeryWeak);
    }

    #[test]
    fn test_strength_medium() {
        let (s, _) = evaluate_password_strength("Abcde12");
        // 7 chars, 62 pool -> ~41 bits -> Fair
        assert!(s >= PasswordStrength::Weak);
    }

    #[test]
    fn test_strength_strong() {
        let (s, _) = evaluate_password_strength("Th1s!sAStr0ngP@ss");
        assert!(s >= PasswordStrength::Strong);
    }

    #[test]
    fn test_strength_very_strong() {
        let (s, _) = evaluate_password_strength("X@9kL#mN2!pQr$tU8vW%yZ1a&bC3dE*f");
        assert_eq!(s, PasswordStrength::VeryStrong);
    }

    #[test]
    fn test_strength_ordering() {
        assert!(PasswordStrength::VeryWeak < PasswordStrength::Weak);
        assert!(PasswordStrength::Weak < PasswordStrength::Fair);
        assert!(PasswordStrength::Fair < PasswordStrength::Strong);
        assert!(PasswordStrength::Strong < PasswordStrength::VeryStrong);
    }

    #[test]
    fn test_strength_labels() {
        assert_eq!(PasswordStrength::VeryWeak.label(), "Very Weak");
        assert_eq!(PasswordStrength::VeryStrong.label(), "Very Strong");
    }

    #[test]
    fn test_strength_fractions() {
        assert!(PasswordStrength::VeryWeak.fraction() < PasswordStrength::VeryStrong.fraction());
    }

    // == Common pattern tests ==================================================

    #[test]
    fn test_common_pattern_match() {
        assert!(is_common_pattern("password123"));
        assert!(is_common_pattern("QWERTY"));
        assert!(is_common_pattern("letmein!"));
    }

    #[test]
    fn test_common_pattern_no_match() {
        assert!(!is_common_pattern("xK9#mL2$pQ"));
        assert!(!is_common_pattern("random-string"));
    }

    // == Audit tests ===========================================================

    #[test]
    fn test_audit_weak_password() {
        let mut v = Vault::new("V", "pw");
        v.add_entry(EntryData::Login(LoginData::new("site", "user", "abc")), 100);
        let issues = audit_vault(&v, 200);
        assert!(issues.iter().any(|i| i.issue == AuditIssueKind::WeakPassword));
    }

    #[test]
    fn test_audit_reused_password() {
        let mut v = Vault::new("V", "pw");
        v.add_entry(EntryData::Login(LoginData::new("site1", "u1", "same_pass")), 100);
        v.add_entry(EntryData::Login(LoginData::new("site2", "u2", "same_pass")), 100);
        let issues = audit_vault(&v, 200);
        let reused: Vec<_> = issues.iter()
            .filter(|i| i.issue == AuditIssueKind::ReusedPassword)
            .collect();
        assert_eq!(reused.len(), 2);
    }

    #[test]
    fn test_audit_old_password() {
        let mut v = Vault::new("V", "pw");
        // Entry created 91 days ago
        v.add_entry(
            EntryData::Login(LoginData::new("site", "user", "securepassword123")),
            100,
        );
        let now = 100 + 91 * 86400;
        let issues = audit_vault(&v, now);
        assert!(issues.iter().any(|i| i.issue == AuditIssueKind::OldPassword));
    }

    #[test]
    fn test_audit_no_totp() {
        let mut v = Vault::new("V", "pw");
        v.add_entry(EntryData::Login(LoginData::new("site", "user", "longpassword99")), 100);
        let issues = audit_vault(&v, 200);
        assert!(issues.iter().any(|i| i.issue == AuditIssueKind::NoTotp));
    }

    #[test]
    fn test_audit_compromised() {
        let mut v = Vault::new("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("site", "u", "longpass123!")), 100);
        v.set_compromised(id, true);
        let issues = audit_vault(&v, 200);
        assert!(issues.iter().any(|i| i.issue == AuditIssueKind::Compromised));
    }

    #[test]
    fn test_audit_common_pattern() {
        let mut v = Vault::new("V", "pw");
        v.add_entry(EntryData::Login(LoginData::new("site", "user", "password123")), 100);
        let issues = audit_vault(&v, 200);
        assert!(issues.iter().any(|i| i.issue == AuditIssueKind::CommonPattern));
    }

    #[test]
    fn test_audit_clean() {
        let mut v = Vault::new("V", "pw");
        let mut login = LoginData::new("site", "user", "Xk9!mLn2#pQr$tUv");
        login.totp_secret = Some("JBSWY3DPEHPK3PXP".to_string());
        v.add_entry(EntryData::Login(login), 100);
        let issues = audit_vault(&v, 200);
        // Should have no weak/common/reused/old issues, only possibly no-totp is cleared
        let critical: Vec<_> = issues.iter()
            .filter(|i| matches!(i.issue,
                AuditIssueKind::WeakPassword
                | AuditIssueKind::ReusedPassword
                | AuditIssueKind::CommonPattern
                | AuditIssueKind::Compromised
            ))
            .collect();
        assert!(critical.is_empty());
    }

    #[test]
    fn test_audit_issue_labels() {
        assert_eq!(AuditIssueKind::WeakPassword.label(), "Weak password");
        assert_eq!(AuditIssueKind::Compromised.label(), "Compromised");
    }

    // == Export tests ===========================================================

    #[test]
    fn test_export_csv_header() {
        let v = Vault::new("V", "pw");
        let csv = export_csv(&v);
        assert!(csv.starts_with("type,name,username,password,url,notes,tags,folder,starred\n"));
    }

    #[test]
    fn test_export_csv_entry() {
        let mut v = Vault::new("V", "pw");
        v.add_entry(EntryData::Login(LoginData::new("site", "user", "pass")), 100);
        let csv = export_csv(&v);
        assert!(csv.contains("Login"));
        assert!(csv.contains("site"));
        assert!(csv.contains("user"));
    }

    #[test]
    fn test_escape_csv_no_special() {
        assert_eq!(escape_csv("hello"), "hello");
    }

    #[test]
    fn test_escape_csv_with_comma() {
        assert_eq!(escape_csv("a,b"), "\"a,b\"");
    }

    #[test]
    fn test_escape_csv_with_quotes() {
        assert_eq!(escape_csv("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn test_serialize_backup() {
        let mut v = Vault::new("Test", "pw");
        v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        v.add_folder("Work");
        let backup = serialize_backup(&v);
        assert!(backup.contains("\"vault_name\": \"Test\""));
        assert!(backup.contains("\"entry_count\": 1"));
        assert!(backup.contains("\"name\": \"Work\""));
    }

    // == SortOrder tests =======================================================

    #[test]
    fn test_sort_order_labels() {
        assert_eq!(SortOrder::NameAsc.label(), "Name A-Z");
        assert_eq!(SortOrder::DateNewest.label(), "Newest");
    }

    #[test]
    fn test_sort_order_cycle() {
        let mut order = SortOrder::NameAsc;
        for _ in 0..5 {
            order = order.next();
        }
        assert_eq!(order, SortOrder::NameAsc); // Full cycle
    }

    #[test]
    fn test_sort_entries_name_asc() {
        let e1 = Entry::new(1, EntryData::Login(LoginData::new("Banana", "u", "p")), 100);
        let e2 = Entry::new(2, EntryData::Login(LoginData::new("Apple", "u", "p")), 200);
        let mut refs: Vec<&Entry> = vec![&e1, &e2];
        sort_entries(&mut refs, SortOrder::NameAsc);
        assert_eq!(refs[0].display_name(), "Apple");
        assert_eq!(refs[1].display_name(), "Banana");
    }

    #[test]
    fn test_sort_entries_date_newest() {
        let e1 = Entry::new(1, EntryData::Login(LoginData::new("A", "u", "p")), 100);
        let e2 = Entry::new(2, EntryData::Login(LoginData::new("B", "u", "p")), 200);
        let mut refs: Vec<&Entry> = vec![&e1, &e2];
        sort_entries(&mut refs, SortOrder::DateNewest);
        assert_eq!(refs[0].display_name(), "B");
    }

    // == ClipboardState tests ==================================================

    #[test]
    fn test_clipboard_new() {
        let c = ClipboardState::new();
        assert!(c.content.is_none());
        assert_eq!(c.auto_clear_seconds, CLIPBOARD_CLEAR_SECONDS);
    }

    #[test]
    fn test_clipboard_copy() {
        let mut c = ClipboardState::new();
        c.copy("secret", 100);
        assert_eq!(c.content, Some("secret".to_string()));
        assert_eq!(c.copied_at, 100);
    }

    #[test]
    fn test_clipboard_auto_clear() {
        let mut c = ClipboardState::new();
        c.copy("secret", 100);
        assert!(!c.should_clear(100));
        assert!(!c.should_clear(129));
        assert!(c.should_clear(130));
    }

    #[test]
    fn test_clipboard_tick_clears() {
        let mut c = ClipboardState::new();
        c.copy("secret", 100);
        c.tick(131);
        assert!(c.content.is_none());
    }

    #[test]
    fn test_clipboard_clear_explicit() {
        let mut c = ClipboardState::new();
        c.copy("data", 100);
        c.clear();
        assert!(c.content.is_none());
    }

    // == AppState tests ========================================================

    #[test]
    fn test_app_state_new() {
        let state = AppState::new();
        assert_eq!(state.sidebar_selection, SidebarSelection::AllItems);
        assert_eq!(state.detail_view, DetailView::EntryDetail);
        assert!(state.search_query.is_empty());
        assert_eq!(state.sort_order, SortOrder::NameAsc);
        assert!(!state.vault.is_unlocked());
    }

    #[test]
    fn test_app_state_refresh_filter() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);
        state.vault.add_entry(EntryData::Login(LoginData::new("GitHub", "user", "pass")), state.now);
        state.vault.add_entry(EntryData::Login(LoginData::new("GitLab", "user", "pass")), state.now);
        state.refresh_filter();
        assert_eq!(state.filtered_ids.len(), 2);

        state.search_query = "hub".to_string();
        state.refresh_filter();
        assert_eq!(state.filtered_ids.len(), 1);
    }

    #[test]
    fn test_app_state_tick() {
        let mut state = AppState::new();
        let old_now = state.now;
        state.tick(5000);
        assert!(state.now > old_now);
    }

    #[test]
    fn test_app_state_run_audit() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);
        state.vault.add_entry(EntryData::Login(LoginData::new("s", "u", "123")), state.now);
        state.run_audit();
        assert!(!state.audit_issues.is_empty());
    }

    // == Render tests ==========================================================

    #[test]
    fn test_render_lock_screen() {
        let state = AppState::new();
        let rt = build_render_tree(&state, 1024.0, 768.0);
        assert!(!rt.commands.is_empty());
        // Lock screen should have FillRect for background
        let has_fill = rt.commands.iter().any(|c| matches!(c, RenderCommand::FillRect { .. }));
        assert!(has_fill);
    }

    #[test]
    fn test_render_unlocked_main_ui() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);
        state.vault.add_entry(EntryData::Login(LoginData::new("GitHub", "alice", "pass123")), state.now);
        state.refresh_filter();
        state.selected_entry_id = state.filtered_ids.first().copied();
        let rt = build_render_tree(&state, 1280.0, 800.0);
        assert!(rt.commands.len() > 30);
    }

    #[test]
    fn test_render_generator_panel() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);
        state.detail_view = DetailView::PasswordGenerator;
        state.generated_password = "test-password-123".to_string();
        let rt = build_render_tree(&state, 1280.0, 800.0);
        assert!(rt.commands.len() > 20);
    }

    #[test]
    fn test_render_settings_panel() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);
        state.detail_view = DetailView::Settings;
        let rt = build_render_tree(&state, 1280.0, 800.0);
        assert!(rt.commands.len() > 20);
    }

    #[test]
    fn test_render_audit_panel_empty() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);
        state.detail_view = DetailView::AuditReport;
        let rt = build_render_tree(&state, 1280.0, 800.0);
        assert!(!rt.commands.is_empty());
    }

    #[test]
    fn test_render_audit_panel_with_issues() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);
        state.vault.add_entry(EntryData::Login(LoginData::new("s", "u", "123")), state.now);
        state.run_audit();
        state.detail_view = DetailView::AuditReport;
        let rt = build_render_tree(&state, 1280.0, 800.0);
        assert!(rt.commands.len() > 20);
    }

    #[test]
    fn test_render_entry_detail_all_types() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);

        let types: Vec<EntryData> = vec![
            EntryData::Login(LoginData::new("site", "user", "pass123!")),
            EntryData::SecureNote(SecureNoteData::new("Note", "Content")),
            EntryData::CreditCard(CreditCardData::new("Visa", "****1234", "12/25", "John")),
            EntryData::Identity(IdentityData::new("Alice", "alice@test.com")),
            EntryData::SshKey(SshKeyData::new("key", "SHA256:abc", "ssh-rsa AAAA")),
        ];

        for data in types {
            let id = state.vault.add_entry(data, state.now);
            state.selected_entry_id = Some(id);
            state.detail_view = DetailView::EntryDetail;
            let rt = build_render_tree(&state, 1280.0, 800.0);
            assert!(rt.commands.len() > 20, "Render failed for entry type");
        }
    }

    #[test]
    fn test_render_no_selected_entry() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);
        state.selected_entry_id = None;
        let rt = build_render_tree(&state, 1280.0, 800.0);
        assert!(!rt.commands.is_empty());
    }

    // == Event handling tests ==================================================

    #[test]
    fn test_handle_tick_event() {
        let mut state = AppState::new();
        let old = state.now;
        handle_event(&mut state, &Event::Tick { elapsed_ms: 2000 });
        assert!(state.now > old);
    }

    #[test]
    fn test_navigate_entry_list_down() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);
        state.vault.add_entry(EntryData::Login(LoginData::new("A", "u", "p")), state.now);
        state.vault.add_entry(EntryData::Login(LoginData::new("B", "u", "p")), state.now);
        state.refresh_filter();
        navigate_entry_list(&mut state, 1);
        assert!(state.selected_entry_id.is_some());
    }

    #[test]
    fn test_navigate_entry_list_empty() {
        let mut state = AppState::new();
        navigate_entry_list(&mut state, 1);
        assert!(state.selected_entry_id.is_none());
    }

    #[test]
    fn test_navigate_entry_list_clamp() {
        let mut state = AppState::new();
        state.vault.unlock("master123", state.now);
        let id = state.vault.add_entry(EntryData::Login(LoginData::new("A", "u", "p")), state.now);
        state.refresh_filter();
        state.selected_entry_id = Some(id);
        // Navigate up past beginning
        navigate_entry_list(&mut state, -10);
        assert_eq!(state.selected_entry_id, state.filtered_ids.first().copied());
    }

    // == Wordlist test =========================================================

    #[test]
    fn test_wordlist_not_empty() {
        assert!(WORDLIST.len() > 100);
    }

    #[test]
    fn test_wordlist_no_duplicates() {
        let set: HashSet<&str> = WORDLIST.iter().copied().collect();
        assert_eq!(set.len(), WORDLIST.len());
    }

    #[test]
    fn test_wordlist_all_lowercase() {
        for word in WORDLIST {
            assert_eq!(*word, word.to_ascii_lowercase(), "Word not lowercase: {}", word);
        }
    }
}

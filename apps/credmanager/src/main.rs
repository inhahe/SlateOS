//! Slate OS Credential Manager
//!
//! A secure password and credential management application for SlateOS.
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
//!
//! # Master password
//!
//! The vault's master password is checked against a [`pwkdf::PasswordVerifier`]
//! — a salted, stretched derivation shared with `apps/lockscreen` and
//! `gui/credentials`, so that the three cannot drift into three incompatible
//! formats (`design-decisions.md` §466). Until 2026-08-18 it was checked
//! against a 64-bit djb2 hash, which is not a password derivation: no salt, no
//! cost, and a width at which collisions are constructible rather than
//! theoretical. See [`Vault::create`].
//!
//! The vault *contents* are not yet encrypted at rest — this crate has no
//! persistence layer at all, and `main` is empty. When one is written, the key
//! comes from `pwkdf::derive_key` under the same params, and the verifier
//! stored beside it must be written with its salt and round count or the vault
//! is unopenable.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use guitk::color::Color;
use guitk::event::{Event, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[cfg(test)]
use guitk::rng::SeededRng;
use guitk::rng::{RandomSource, SecretSource, SystemRandom};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use pwkdf::{KdfError, KdfParams, PasswordVerifier};

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
/// Height of the entry list's own header strip -- the "N entries" line, which
/// stays put while the rows below it scroll.
///
/// It was written as a bare `32.0` in `render_entry_list` and again in
/// `handle_list_click`, which is two chances to disagree about where row zero
/// starts; the scroll bound needs it as well, which would have made three.
const LIST_HEADER_HEIGHT: f32 = 32.0;
/// The vertical step the detail panel's fields are laid out on, used as the
/// "row" a wheel notch is measured in there. The panel has no rows of its own
/// -- it is a column of labelled fields at varying spacings -- so this is the
/// nearest honest answer to "how far is one line".
const DETAIL_LINE_HEIGHT: f32 = 24.0;
/// Window size before the first `Event::Resize` arrives.
const DEFAULT_WINDOW_WIDTH: f32 = 1280.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 800.0;
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
            Self::Login(d) => {
                d.url.to_ascii_lowercase().contains(&q) || d.notes.to_ascii_lowercase().contains(&q)
            }
            Self::SecureNote(d) => d.content.to_ascii_lowercase().contains(&q),
            Self::CreditCard(d) => {
                d.cardholder.to_ascii_lowercase().contains(&q)
                    || d.notes.to_ascii_lowercase().contains(&q)
            }
            Self::Identity(d) => {
                d.phone.to_ascii_lowercase().contains(&q)
                    || d.address.to_ascii_lowercase().contains(&q)
            }
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

/// Domain label mixed into this crate's stored verifier.
///
/// It is what stops a verifier from meaning anything anywhere else: the lock
/// screen (`slateos-lockscreen-verifier`) and the credential service
/// (`slateos-credential-verifier`) derive theirs from the same key material
/// under different labels, so a value lifted from one store cannot be replayed
/// against another. Changing this string invalidates every existing vault, and
/// nothing local would fail — hence the format-pinning test.
const VERIFIER_DOMAIN: &[u8] = b"slateos-credmanager-vault";

/// Iteration count for vaults built by tests.
///
/// The properties under test — that the right password is accepted, that a
/// wrong one is not, that the salt is honoured — do not depend on the number
/// of rounds, and [`pwkdf::DEFAULT_ROUNDS`] is chosen to take ~130 ms, which
/// a suite that builds a vault per test would turn into several minutes.
#[cfg(test)]
const TEST_ROUNDS: u32 = 16;

/// The master password of the vault built by [`AppState::for_test`].
#[cfg(test)]
const TEST_MASTER_PASSWORD: &str = "master123";

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
    /// What the master password is checked against.
    ///
    /// A [`PasswordVerifier`], not a hash: it carries the salt and the
    /// iteration count *with* the stored value, because all three have to
    /// agree and a persistence layer that writes down only the last one has
    /// destroyed the vault. This field used to be a `u64` from a djb2 hash —
    /// see [`Vault::create`] for why that was worse than it looks.
    master: PasswordVerifier,
    entries: Vec<Entry>,
    folders: Vec<Folder>,
    last_access: u64,
    auto_lock_minutes: u32,
    id_gen: u64,
}

impl Vault {
    /// Create a vault, enrolling `master_password` against a fresh salt.
    ///
    /// # Errors
    ///
    /// [`KdfError::EntropyUnavailable`] if the kernel CSPRNG cannot be
    /// reached. Propagated rather than papered over with a fixed salt: this is
    /// the *secret* tier of `design-decisions.md` §465, and a predictable salt
    /// chosen once outlives every later chance to notice it. Refusing to
    /// create the vault is recoverable; creating it against a guessable salt
    /// is not.
    ///
    /// # What this replaced
    ///
    /// Until 2026-08-18 the master password was stored as
    /// `simple_hash(password): u64` — djb2, one multiply-add per byte, no
    /// salt, no iteration. Three separate failures, of which only the first is
    /// the obvious one:
    ///
    /// - **No cost.** Testing a guess took two arithmetic operations per
    ///   character, so an attacker with the stored value ran through the
    ///   entire plausible-password space at memory speed.
    /// - **No salt.** The same password produced the same 64 bits in every
    ///   vault on every machine, so one precomputed table opened all of them.
    /// - **64 bits, non-cryptographic.** Collisions are not a theoretical
    ///   concern for djb2 — they are constructible — and [`Vault::unlock`]
    ///   accepted *any* colliding string, not just the owner's password.
    fn create(name: &str, master_password: &str) -> Result<Self, KdfError> {
        let params = KdfParams::fresh(pwkdf::DEFAULT_ROUNDS)?;
        Ok(Self::with_verifier(
            name,
            PasswordVerifier::create(master_password.as_bytes(), params, VERIFIER_DOMAIN),
        ))
    }

    /// Reopen a vault whose verifier was read back from storage.
    ///
    /// `params` must be the salt and cost the verifier was created under; a
    /// store that keeps the verifier and loses the salt has locked the owner
    /// out permanently, and the symptom ("correct password refused") does not
    /// point at the cause.
    fn from_stored(name: &str, params: KdfParams, verifier: [u8; 32]) -> Self {
        Self::with_verifier(
            name,
            PasswordVerifier::from_parts(params, VERIFIER_DOMAIN, verifier),
        )
    }

    /// The empty vault around an already-built verifier — the one place the
    /// non-password fields are initialised, so the three constructors cannot
    /// drift apart in what a fresh vault contains.
    fn with_verifier(name: &str, master: PasswordVerifier) -> Self {
        Self {
            name: name.to_string(),
            state: VaultState::Locked,
            master,
            entries: Vec::new(),
            folders: Vec::new(),
            last_access: 0,
            auto_lock_minutes: DEFAULT_AUTO_LOCK_MINUTES,
            id_gen: 1,
        }
    }

    /// A vault with a known master password, a named salt and a cheap cost.
    ///
    /// `#[cfg(test)]` so that neither shortcut can reach production. Both are
    /// deliberate and neither is safe outside a test: the fixed salt makes
    /// assertions reproducible, and [`TEST_ROUNDS`] keeps a suite that builds
    /// a vault in almost every test from spending ~130 ms on each one.
    #[cfg(test)]
    fn for_test(name: &str, master_password: &str) -> Self {
        let params = KdfParams::new([0x5Au8; pwkdf::SALT_LEN], TEST_ROUNDS);
        Self::with_verifier(
            name,
            PasswordVerifier::create(master_password.as_bytes(), params, VERIFIER_DOMAIN),
        )
    }

    fn next_id(&mut self) -> u64 {
        let id = self.id_gen;
        self.id_gen = self.id_gen.saturating_add(1);
        id
    }

    /// Try to unlock the vault with `password`.
    ///
    /// Costs a full derivation — deliberately ~130 ms at
    /// [`pwkdf::DEFAULT_ROUNDS`]. That is the point, and it is why this is
    /// called on submit rather than per keystroke.
    fn unlock(&mut self, password: &str, now: u64) -> bool {
        if self.master.check(password.as_bytes()) {
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
        self.entries
            .iter()
            .filter(|e| e.folder_id == folder_id)
            .collect()
    }

    fn starred_entries(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.starred).collect()
    }

    fn entries_with_tag(&self, tag: &str) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.tags.iter().any(|t| t == tag))
            .collect()
    }

    fn entries_of_type(&self, entry_type: EntryType) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.entry_type() == entry_type)
            .collect()
    }

    fn search_entries(&self, query: &str) -> Vec<&Entry> {
        if query.is_empty() {
            return self.entries.iter().collect();
        }
        self.entries
            .iter()
            .filter(|e| e.data.matches_search(query))
            .collect()
    }

    /// All unique tags across all entries.
    fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .entries
            .iter()
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
            chars.extend(SYMBOL_ALPHABET.chars());
        }
        chars
    }

    /// Count of distinct characters in the pool.
    ///
    /// Derived from [`Self::build_charset`] rather than re-summed from the
    /// class sizes. The two used to be written out separately — a hand-added
    /// `26 + 26 + 10 + 30` here, against the ranges above — which agreed only
    /// by coincidence and would have disagreed silently the moment a character
    /// was added to [`SYMBOL_ALPHABET`]. The symptom of a disagreement is an
    /// entropy figure wrong in the reassuring direction, which nothing catches.
    fn pool_size(&self) -> usize {
        self.build_charset().len()
    }
}

/// The punctuation the generator draws from, and the size the strength meter
/// scores an unrecognised character against.
///
/// One definition because two would drift: [`estimate_symbol_pool`] has to
/// agree with what the generator can actually produce, or a generated password
/// is scored against the wrong alphabet.
const SYMBOL_ALPHABET: &str = "!@#$%^&*()-_=+[]{}|;:',.<>?/~`";

/// Number of distinct characters in [`SYMBOL_ALPHABET`].
fn estimate_symbol_pool() -> usize {
    SYMBOL_ALPHABET.chars().count()
}

/// Letters in one ASCII case.
const ASCII_LETTER_COUNT: usize = 26;

/// ASCII decimal digits.
const ASCII_DIGIT_COUNT: usize = 10;

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

/// Where a generated credential's randomness comes from.
///
/// Two live variants and no fallback between them, deliberately. See
/// [`PasswordGenerator`] for what this replaced and why a fallback would have
/// preserved the defect rather than fixed it.
#[derive(Debug)]
enum CredRandom {
    /// The kernel CSPRNG — the only source a stored credential may come from.
    /// Boxed because its refill buffer dwarfs the other variants, and this is
    /// constructed once per app rather than anywhere in a loop.
    System(Box<SystemRandom>),
    /// A named sequence, so a test can name the password it asserts on. Never
    /// reachable from the running app.
    #[cfg(test)]
    Seeded(SeededRng),
    /// The kernel had no entropy to give. Generating is refused outright.
    Unavailable,
}

impl CredRandom {
    fn from_system() -> Self {
        match SystemRandom::open() {
            Ok(source) => Self::System(Box::new(source)),
            Err(_) => Self::Unavailable,
        }
    }
}

/// The both-sides-of-the-draw rule lives in [`SecretSource::secret`]. This
/// crate, `apps/passwordgen` and `gui/credentials` each carried their own copy
/// of it before that; a rule about secrets restated once per crate is one that
/// will eventually be restated slightly wrong.
impl SecretSource for CredRandom {
    /// Whether a secret drawn from this source may be handed to the user.
    fn is_trustworthy(&self) -> bool {
        match self {
            Self::System(source) => source.is_healthy(),
            #[cfg(test)]
            Self::Seeded(_) => true,
            Self::Unavailable => false,
        }
    }
}

impl RandomSource for CredRandom {
    fn next_u64(&mut self) -> u64 {
        match self {
            Self::System(source) => source.next_u64(),
            #[cfg(test)]
            Self::Seeded(source) => source.next_u64(),
            // Visibly not random. `secret` refuses before this is ever read,
            // so it is the belt to that braces rather than a fallback.
            Self::Unavailable => 0,
        }
    }
}

/// What the generator panel shows when it cannot generate.
const NO_ENTROPY_MESSAGE: &str =
    "Cannot generate: the system random number generator is unavailable";

/// The password generator with all settings.
///
/// The randomness here used to be a `seed: u64` initialised to the literal
/// `12345` and bumped by one per generation, fed through a stateless integer
/// hash. Every install therefore produced the same passwords in the same
/// order: the first password this manager ever generated for you was the
/// first it generated for everyone, and the *n*th was a published function of
/// `12345 + n`. The strength meter beside it reported the entropy of the
/// character pool — a true statement about a password drawn at random, and a
/// false one about this password, whose real entropy was zero.
///
/// It now draws from the kernel CSPRNG and refuses to generate at all when it
/// cannot reach one. There is deliberately no weaker source to fall back to:
/// a fallback is how the original defect would survive the fix, and nobody can
/// tell a predictable password from an unpredictable one by looking at it.
/// See `design-decisions.md` §462.
#[derive(Debug)]
struct PasswordGenerator {
    length: usize,
    mode: GeneratorMode,
    charset: CharsetOptions,
    passphrase: PassphraseOptions,
    rng: CredRandom,
}

impl PasswordGenerator {
    fn new() -> Self {
        Self::with_rng(CredRandom::from_system())
    }

    /// A generator drawing from a named sequence, for tests only.
    #[cfg(test)]
    fn with_seed(seed: u64) -> Self {
        Self::with_rng(CredRandom::Seeded(SeededRng::new(seed)))
    }

    /// A generator whose source has already failed, for tests only.
    #[cfg(test)]
    fn without_entropy() -> Self {
        Self::with_rng(CredRandom::Unavailable)
    }

    fn with_rng(rng: CredRandom) -> Self {
        Self {
            length: 20,
            mode: GeneratorMode::Random,
            charset: CharsetOptions::default(),
            passphrase: PassphraseOptions::default(),
            rng,
        }
    }

    fn set_length(&mut self, len: usize) {
        self.length = len.clamp(8, 128);
    }

    /// Generate a password from the current settings, or `None` if the system
    /// has no randomness to draw it from.
    fn generate(&mut self) -> Option<String> {
        let mode = self.mode;
        let length = self.length;
        let charset = self.charset.build_charset();
        let words = self.passphrase.word_count.max(2);
        let separator = self.passphrase.separator.clone();
        self.rng.secret(|rng| match mode {
            GeneratorMode::Random => Self::draw_random(rng, &charset, length),
            GeneratorMode::Pronounceable => Self::draw_pronounceable(rng, length),
            GeneratorMode::Passphrase => Self::draw_passphrase(rng, words, &separator),
        })
    }

    /// `length` characters drawn from `charset`, or the empty string if the
    /// user has switched every character class off.
    fn draw_random(rng: &mut CredRandom, charset: &[char], length: usize) -> String {
        (0..length)
            .filter_map(|_| rng.choose(charset).copied())
            .collect()
    }

    /// `length` characters alternating consonant and vowel.
    fn draw_pronounceable(rng: &mut CredRandom, length: usize) -> String {
        const CONSONANTS: &[u8] = b"bcdfghjklmnpqrstvwxyz";
        const VOWELS: &[u8] = b"aeiou";
        (0..length)
            .map(|i| {
                let pool = if i % 2 == 0 { CONSONANTS } else { VOWELS };
                // Both pools are non-empty constants, so `pick` always answers.
                char::from(rng.choose(pool).copied().unwrap_or(b'?'))
            })
            .collect()
    }

    /// `words` words from the list, joined by `separator`.
    fn draw_passphrase(rng: &mut CredRandom, words: usize, separator: &str) -> String {
        (0..words)
            .filter_map(|_| rng.choose(WORDLIST).copied())
            .collect::<Vec<_>>()
            .join(separator)
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

/// Draw a new password into `state`, or record the refusal.
///
/// Every path that generates goes through here so that the refusal is recorded
/// in exactly one place; three call sites each assigning the result themselves
/// is three chances for one of them to show a stale password beside a message
/// saying the generator is unavailable.
fn regenerate_password(state: &mut AppState) {
    match state.password_generator.generate() {
        Some(password) => {
            state.generated_password = password;
            state.generator_error = None;
        }
        None => {
            // Clear the old password too: leaving the previous one on screen
            // beside the refusal invites the user to go on using it.
            state.generated_password.clear();
            state.generator_error = Some(NO_ENTROPY_MESSAGE.to_owned());
        }
    }
}

/// Small word list for passphrase generation.
const WORDLIST: &[&str] = &[
    "abandon",
    "ability",
    "abstract",
    "account",
    "across",
    "action",
    "adapt",
    "address",
    "adjust",
    "advance",
    "afford",
    "agree",
    "airport",
    "alarm",
    "album",
    "alert",
    "alien",
    "allow",
    "almost",
    "alpha",
    "already",
    "alter",
    "amazing",
    "amount",
    "anchor",
    "angle",
    "animal",
    "annual",
    "answer",
    "antenna",
    "apart",
    "apple",
    "approve",
    "arena",
    "armor",
    "army",
    "arrange",
    "arrest",
    "arrive",
    "arrow",
    "artist",
    "aspect",
    "assist",
    "attack",
    "attract",
    "auction",
    "author",
    "avoid",
    "awake",
    "balance",
    "bamboo",
    "banner",
    "barely",
    "barrel",
    "basket",
    "battle",
    "beach",
    "beauty",
    "become",
    "before",
    "behind",
    "believe",
    "below",
    "bench",
    "benefit",
    "beyond",
    "bicycle",
    "binder",
    "blanket",
    "blast",
    "bless",
    "blind",
    "block",
    "blossom",
    "board",
    "border",
    "bottom",
    "bounce",
    "branch",
    "brave",
    "breeze",
    "bridge",
    "bright",
    "broken",
    "bronze",
    "brother",
    "brush",
    "bubble",
    "budget",
    "buffalo",
    "burden",
    "burst",
    "butter",
    "cabin",
    "cable",
    "camera",
    "cancel",
    "candle",
    "canvas",
    "capture",
    "carbon",
    "carpet",
    "castle",
    "casual",
    "catalog",
    "caution",
    "ceiling",
    "cement",
    "census",
    "center",
    "cereal",
    "certain",
    "chair",
    "change",
    "chapter",
    "cherry",
    "chimney",
    "choice",
    "chronic",
    "circle",
    "citizen",
    "civil",
    "claim",
    "clap",
    "clarify",
    "claw",
    "clever",
    "clinic",
    "clock",
    "cluster",
    "coach",
    "coconut",
    "coffee",
    "collect",
    "column",
    "comfort",
    "common",
    "company",
    "concert",
    "conduct",
    "confirm",
    "connect",
    "consider",
    "control",
    "convert",
    "copper",
    "coral",
    "correct",
    "costume",
    "cotton",
    "couch",
    "country",
    "couple",
    "cousin",
    "cover",
    "cradle",
    "craft",
    "crater",
    "crazy",
    "credit",
    "cricket",
    "crisis",
    "crisp",
    "cross",
    "crouch",
    "crowd",
    "crucial",
    "cruel",
    "cruise",
    "crystal",
    "culture",
    "curtain",
    "custom",
    "cycle",
    "damage",
    "dance",
    "danger",
    "daring",
    "daughter",
    "dawn",
    "debris",
    "decade",
    "decline",
    "decorate",
    "defense",
    "degree",
    "deliver",
    "demand",
    "denial",
    "dentist",
    "depart",
    "deposit",
    "depth",
    "derive",
    "desert",
    "design",
    "desktop",
    "destroy",
    "detail",
    "detect",
    "device",
    "devote",
    "diagram",
    "diamond",
    "diesel",
    "differ",
    "digital",
    "dinner",
    "direct",
    "discover",
    "display",
    "distance",
    "divert",
    "doctor",
    "dolphin",
    "domain",
    "donate",
    "double",
    "dragon",
    "drama",
    "dream",
    "dress",
    "drift",
    "drink",
    "driver",
    "drop",
    "durable",
    "during",
    "eagle",
    "early",
    "earth",
    "eclipse",
    "ecology",
    "economy",
    "educate",
    "effort",
    "eighth",
    "either",
    "elbow",
    "elder",
    "elegant",
    "element",
    "elephant",
    "elevator",
    "elite",
    "embark",
    "embrace",
    "emerge",
    "emotion",
    "emperor",
    "enable",
    "endless",
    "energy",
    "enforce",
    "engine",
    "enhance",
    "enjoy",
    "enough",
    "entire",
    "episode",
    "equal",
    "erosion",
    "escape",
    "essence",
    "estate",
    "eternal",
    "evening",
    "evidence",
    "evolve",
    "exact",
    "example",
    "excess",
    "exclude",
    "execute",
    "exhaust",
    "exhibit",
    "exotic",
    "expand",
    "expect",
    "explain",
    "expose",
    "extend",
    "extra",
    "fabric",
    "faculty",
    "fading",
    "failure",
    "falcon",
    "family",
    "fantasy",
    "fashion",
    "father",
    "feature",
    "federal",
    "fiction",
    "figure",
    "filter",
    "final",
    "finger",
    "finish",
    "fiscal",
    "fitness",
    "flavor",
    "flight",
    "float",
    "floor",
    "flower",
    "fluid",
    "flutter",
    "focus",
    "follow",
    "forest",
    "forget",
    "formal",
    "fortune",
    "fossil",
    "foster",
    "found",
    "fragile",
    "frame",
    "freedom",
    "freeze",
    "fresh",
    "friend",
    "frozen",
    "fruit",
    "future",
    "galaxy",
    "gallery",
    "garage",
    "garden",
    "garlic",
    "gather",
    "general",
    "genius",
    "gentle",
    "genuine",
    "gesture",
    "giant",
    "glacier",
    "glance",
    "glimpse",
    "global",
    "gloom",
    "glory",
    "glove",
    "goddess",
    "golden",
    "gossip",
    "govern",
    "grace",
    "grain",
    "grammar",
    "grant",
    "gravity",
    "great",
    "grocery",
    "ground",
    "group",
    "growing",
    "guard",
    "guitar",
    "hammer",
    "hamster",
    "harbor",
    "harvest",
    "hazard",
    "health",
    "heaven",
    "helmet",
    "hidden",
    "holiday",
    "hollow",
    "honey",
    "horror",
    "hospital",
    "hotel",
    "human",
    "humor",
    "hunter",
    "hybrid",
    "kingdom",
    "kitchen",
    "kiwi",
    "ladder",
    "language",
    "large",
    "later",
    "launch",
    "lava",
    "leader",
    "lecture",
    "legend",
    "leisure",
    "lemon",
    "length",
    "letter",
    "level",
    "liberty",
    "library",
    "license",
    "light",
    "limit",
    "linear",
    "liquid",
    "little",
    "lively",
    "lobby",
    "local",
    "logic",
    "lonely",
    "lottery",
    "luggage",
    "lumber",
    "lunar",
    "luxury",
    "machine",
    "magnet",
    "maiden",
    "major",
    "manage",
    "mandate",
    "manual",
    "maple",
    "marble",
    "margin",
    "marine",
    "market",
    "master",
    "matter",
    "meadow",
    "measure",
    "medium",
    "melody",
    "member",
    "memory",
    "mention",
    "mentor",
    "mercy",
    "method",
    "middle",
    "migrate",
    "million",
    "minimum",
    "mirror",
    "misery",
    "mission",
    "mixture",
    "mobile",
    "model",
    "modify",
    "moment",
    "monitor",
    "monkey",
    "monster",
    "moral",
    "morning",
    "motion",
    "mountain",
    "mouse",
    "muscle",
    "museum",
    "mushroom",
    "mutual",
    "mystery",
    "narrow",
    "nation",
    "nature",
    "nearby",
    "needle",
    "neither",
    "nephew",
    "nerve",
    "network",
    "neutral",
    "noble",
    "normal",
    "notable",
    "nothing",
    "notice",
    "novel",
    "number",
    "obvious",
    "ocean",
    "office",
    "olive",
    "opinion",
    "option",
    "orange",
    "orbit",
    "origin",
    "orphan",
    "outdoor",
    "output",
    "outside",
    "oxygen",
    "paddle",
    "palace",
    "panda",
    "panel",
    "panic",
    "parcel",
    "parent",
    "partner",
    "pattern",
    "pebble",
    "penalty",
    "people",
    "perfect",
    "permit",
    "person",
    "phrase",
    "picture",
    "pilot",
    "pioneer",
    "pirate",
    "planet",
    "plastic",
    "player",
    "please",
    "pledge",
    "plunge",
    "pocket",
    "poetry",
    "pointer",
    "polar",
    "policy",
    "popular",
    "portion",
    "poverty",
    "powder",
    "praise",
    "predict",
    "prepare",
    "present",
    "pretty",
    "prevent",
    "primary",
    "print",
    "prison",
    "private",
    "problem",
    "process",
    "produce",
    "profile",
    "program",
    "project",
    "promote",
    "prosper",
    "protect",
    "proud",
    "provide",
    "public",
    "purpose",
    "puzzle",
    "pyramid",
    "quality",
    "quantum",
    "quarter",
    "question",
    "quickly",
    "rabbit",
    "raccoon",
    "radar",
    "random",
    "rapid",
    "rather",
    "raven",
    "reason",
    "rebel",
    "recall",
    "receive",
    "record",
    "reform",
    "region",
    "regret",
    "reject",
    "release",
    "relief",
    "remain",
    "remind",
    "remove",
    "render",
    "repair",
    "repeat",
    "replace",
    "report",
    "require",
    "rescue",
    "resist",
    "resolve",
    "result",
    "retire",
    "retreat",
    "return",
    "reveal",
    "review",
    "reward",
    "rhythm",
    "ribbon",
    "right",
    "ritual",
    "river",
    "robust",
    "rocket",
    "romance",
    "roster",
    "rotate",
    "royal",
    "rubber",
    "runway",
    "saddle",
    "safari",
    "salmon",
    "salute",
    "sample",
    "satisfy",
    "scatter",
    "scene",
    "scheme",
    "school",
    "science",
    "scissors",
    "search",
    "season",
    "secret",
    "section",
    "security",
    "select",
    "seller",
    "senior",
    "series",
    "service",
    "session",
    "settle",
    "shadow",
    "shallow",
    "shelter",
    "sheriff",
    "shield",
    "shimmer",
    "shiver",
    "shock",
    "shoulder",
    "shuffle",
    "sibling",
    "signal",
    "silent",
    "silver",
    "similar",
    "simple",
    "sister",
    "situation",
    "sketch",
    "skull",
    "slender",
    "slight",
    "slogan",
    "smart",
    "smooth",
    "snack",
    "soccer",
    "social",
    "soldier",
    "solution",
    "someone",
    "source",
    "spatial",
    "special",
    "sphere",
    "spirit",
    "sponsor",
    "spring",
    "squeeze",
    "stable",
    "stadium",
    "staff",
    "stage",
    "stamp",
    "stand",
    "start",
    "state",
    "station",
    "steady",
    "stereo",
    "stick",
    "stomach",
    "story",
    "strategy",
    "street",
    "strong",
    "student",
    "studio",
    "subject",
    "submit",
    "sudden",
    "suffer",
    "suggest",
    "summer",
    "sunrise",
    "super",
    "supply",
    "surface",
    "surplus",
    "surprise",
    "surround",
    "survey",
    "suspect",
    "sustain",
    "symbol",
    "system",
    "table",
    "tackle",
    "talent",
    "target",
    "tattoo",
    "teacher",
    "tenant",
    "tennis",
    "terminal",
    "texture",
    "theory",
    "therapy",
    "thrive",
    "thunder",
    "ticket",
    "timber",
    "tissue",
    "title",
    "toast",
    "tobacco",
    "today",
    "together",
    "tomato",
    "tomorrow",
    "tongue",
    "topic",
    "tornado",
    "tortoise",
    "tourist",
    "toward",
    "tower",
    "traffic",
    "tragedy",
    "train",
    "transfer",
    "travel",
    "treasure",
    "trend",
    "trial",
    "trigger",
    "triple",
    "trophy",
    "trouble",
    "truck",
    "truly",
    "trumpet",
    "trust",
    "tunnel",
    "turtle",
    "twelve",
    "twenty",
    "typical",
    "umbrella",
    "unable",
    "uncle",
    "under",
    "unfold",
    "unique",
    "universe",
    "unknown",
    "unlock",
    "unusual",
    "upgrade",
    "uphold",
    "upper",
    "urban",
    "useful",
    "usual",
    "utility",
    "vacant",
    "vacuum",
    "valley",
    "valve",
    "vanish",
    "vapor",
    "various",
    "vendor",
    "venture",
    "verify",
    "version",
    "vessel",
    "veteran",
    "victory",
    "video",
    "village",
    "vintage",
    "violin",
    "virtual",
    "virus",
    "vision",
    "visual",
    "vivid",
    "vocal",
    "volcano",
    "voltage",
    "volume",
    "voyage",
    "wagon",
    "warrior",
    "wealth",
    "weapon",
    "weather",
    "welcome",
    "western",
    "whisper",
    "widen",
    "wildlife",
    "window",
    "winter",
    "wisdom",
    "witness",
    "wonder",
    "world",
    "wreath",
    "wrestle",
    "wrist",
    "yellow",
    "yield",
    "young",
    "zebra",
    "zero",
    "zigzag",
    "zombie",
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

    // Characters, not bytes. `password.len()` counted a three-byte character
    // as three, so a four-character password of non-ASCII text scored as if it
    // were twelve — an overstatement, which is the direction a strength meter
    // must never err in.
    let len = password.chars().count();
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

    let pool_size = [
        (has_lower, ASCII_LETTER_COUNT),
        (has_upper, ASCII_LETTER_COUNT),
        (has_digit, ASCII_DIGIT_COUNT),
        (has_symbol, estimate_symbol_pool()),
    ]
    .into_iter()
    .filter(|&(present, _)| present)
    .fold(0usize, |acc, (_, size)| acc.saturating_add(size));

    let entropy = if pool_size > 0 {
        // `log2` of a pool of 10..=92 is 3.3..=6.5, and the length is bounded
        // by what fits in a password field, so the product cannot approach
        // `f64`'s range. The lint cannot see either bound.
        #[allow(
            clippy::arithmetic_side_effects,
            reason = "bounded above by a small pool size and a field-length password; \
                      floats have no checked multiply to use instead"
        )]
        let bits = len as f64 * (pool_size as f64).log2();
        bits
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
        "password", "123456", "qwerty", "letmein", "admin", "welcome", "monkey", "master",
        "dragon", "login", "abc123", "111111", "iloveyou", "sunshine", "princess", "football",
        "shadow", "trustno1", "baseball", "access",
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
                && ids.len() > 1
            {
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
                && login.totp_secret.is_none()
            {
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
        // Tag and folder names are free-form user text just like the other
        // columns, so they need the same quoting; before this they were the
        // only two fields interpolated raw.
        let tags = escape_csv(&entry.tags.join(";"));
        let folder = escape_csv(
            &entry
                .folder_id
                .and_then(|fid| vault.get_folder(fid))
                .map_or(String::new(), |f| f.name.clone()),
        );
        let starred = if entry.starred { "true" } else { "false" };

        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            etype, name, subtitle, password, url, notes, tags, folder, starred,
        ));
    }
    csv
}

/// Escape a value for CSV output.
/// Quote a CSV field per RFC 4180.
///
/// Delegates to the shared escaper so this app cannot drift from the other
/// CSV writers again. The local version this replaced omitted `\r` from its
/// trigger set; since RFC 4180 records are CRLF-terminated, a bare CR in an
/// unquoted field splits the record for most readers.
fn escape_csv(s: &str) -> String {
    guitk::csv::field(s)
}

/// Serialize vault to a backup string (simplified JSON-like format).
fn serialize_backup(vault: &Vault) -> String {
    let mut out = String::from("{\n  \"vault_name\": ");
    // Every string below is user-chosen (vault/entry/folder/tag names). None
    // of them was escaped before, so a `"` in any one of them produced a
    // backup file that no JSON reader could load -- i.e. a silently
    // unrestorable backup, which is the worst possible failure for a
    // credential vault.
    out.push_str(&format!(
        "\"{}\",\n",
        guitk::escape::json_string(&vault.name)
    ));
    out.push_str(&format!("  \"entry_count\": {},\n", vault.entries.len()));
    out.push_str("  \"entries\": [\n");
    // Index of the last element, so the "is this the final one?" test inside
    // the loop is a comparison rather than an `i + 1` that has to be argued
    // safe. `saturating_sub` covers the empty case, where the loop body never
    // runs and the value is unused.
    let last_entry = vault.entries.len().saturating_sub(1);
    for (i, entry) in vault.entries.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"id\": {},\n", entry.id));
        out.push_str(&format!(
            "      \"type\": \"{}\",\n",
            entry.entry_type().label()
        ));
        out.push_str(&format!(
            "      \"name\": \"{}\",\n",
            guitk::escape::json_string(entry.display_name())
        ));
        out.push_str(&format!("      \"starred\": {},\n", entry.starred));
        out.push_str(&format!("      \"compromised\": {},\n", entry.compromised));
        out.push_str(&format!("      \"created_at\": {},\n", entry.created_at));
        out.push_str(&format!("      \"modified_at\": {},\n", entry.modified_at));
        let tags_str: Vec<String> = entry
            .tags
            .iter()
            .map(|t| format!("\"{}\"", guitk::escape::json_string(t)))
            .collect();
        out.push_str(&format!("      \"tags\": [{}]\n", tags_str.join(", ")));
        if i < last_entry {
            out.push_str("    },\n");
        } else {
            out.push_str("    }\n");
        }
    }
    out.push_str("  ],\n");
    out.push_str("  \"folders\": [\n");
    let last_folder = vault.folders.len().saturating_sub(1);
    for (i, folder) in vault.folders.iter().enumerate() {
        out.push_str(&format!(
            "    {{ \"id\": {}, \"name\": \"{}\" }}",
            folder.id,
            guitk::escape::json_string(&folder.name)
        ));
        if i < last_folder {
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
            a.display_name()
                .to_ascii_lowercase()
                .cmp(&b.display_name().to_ascii_lowercase())
        }),
        SortOrder::NameDesc => entries.sort_by(|a, b| {
            b.display_name()
                .to_ascii_lowercase()
                .cmp(&a.display_name().to_ascii_lowercase())
        }),
        SortOrder::DateNewest => entries.sort_by_key(|e| std::cmp::Reverse(e.modified_at)),
        SortOrder::DateOldest => entries.sort_by_key(|a| a.modified_at),
        SortOrder::TypeAsc => {
            entries.sort_by(|a, b| a.entry_type().label().cmp(b.entry_type().label()));
        }
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
    /// Set when the generator refused to generate, cleared when it succeeds.
    /// Shown in place of the password so the refusal cannot be mistaken for
    /// a generator the user simply has not pressed yet.
    generator_error: Option<String>,
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
    /// Window size, kept current by `Event::Resize`.
    ///
    /// It used to be passed to `build_render_tree` and to exist nowhere else,
    /// which is why the two offsets above had no upper bound: at the moment
    /// the wheel turned there was no size in scope to compute one from, so
    /// both were clamped with `.max(0.0)` and nothing else and either pane
    /// could be scrolled into blank space indefinitely.
    width: f32,
    height: f32,
    /// Height of the detail panel's content as the renderer last laid it out.
    ///
    /// The panel's length depends on which fields the selected entry has, so
    /// unlike the entry list it cannot be derived without doing the layout.
    /// Measuring it during the render is what keeps the bound and the drawing
    /// in agreement; a second derivation here would drift the first time a
    /// field is added to one and not the other.
    detail_content_height: f32,
    /// Settings: auto-lock minutes.
    settings_auto_lock: u32,
}

impl AppState {
    /// Build the app around an already-opened vault.
    ///
    /// The vault is a parameter rather than something this constructor makes,
    /// because making one means choosing a master password, and this function
    /// used to choose `"master123"` — in non-test code, in a credential
    /// manager. Nothing called it outside tests, so nothing shipped; but the
    /// only reason it was harmless is that `main` is still empty, which is not
    /// a property to rely on. A real caller loads the verifier from storage
    /// ([`Vault::from_stored`]) or asks the user for a new one
    /// ([`Vault::create`]).
    fn new(vault: Vault) -> Self {
        let mut state = Self {
            vault,
            sidebar_selection: SidebarSelection::AllItems,
            selected_entry_id: None,
            detail_view: DetailView::EntryDetail,
            search_query: String::new(),
            sort_order: SortOrder::NameAsc,
            password_generator: PasswordGenerator::new(),
            generated_password: String::new(),
            generator_error: None,
            clipboard: ClipboardState::new(),
            show_password: false,
            now: 1000000,
            filtered_ids: Vec::new(),
            audit_issues: Vec::new(),
            master_input: String::new(),
            unlock_failed: false,
            list_scroll: 0.0,
            detail_scroll: 0.0,
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            detail_content_height: 0.0,
            settings_auto_lock: DEFAULT_AUTO_LOCK_MINUTES,
        };
        state.refresh_filter();
        state
    }

    /// App state around a locked test vault whose master password is
    /// [`TEST_MASTER_PASSWORD`].
    #[cfg(test)]
    fn for_test() -> Self {
        Self::new(Vault::for_test("My Vault", TEST_MASTER_PASSWORD))
    }

    /// Height of the area below the toolbar, which both panes are drawn into.
    fn pane_height(&self) -> f32 {
        (self.height - TOOLBAR_HEIGHT).max(0.0)
    }

    /// The y of the entry list's first row -- the header strip's bottom edge.
    ///
    /// `TOOLBAR_HEIGHT + LIST_HEADER_HEIGHT` was written once in the renderer
    /// and once in `handle_list_click`, which is the arrangement that let the
    /// two disagree about which pixels are rows in the first place.
    const fn rows_top() -> f32 {
        TOOLBAR_HEIGHT + LIST_HEADER_HEIGHT
    }

    /// The height of the entry list's scrolling row area.
    ///
    /// The header strip does *not* scroll, so it is not part of this. It used
    /// to be inside the renderer's clip, which meant a scrolled row was
    /// painted over the "N entries" caption rather than stopping under it.
    fn rows_height(&self) -> f32 {
        (self.height - Self::rows_top()).max(0.0)
    }

    /// The index into `filtered_ids` under `my`, or `None` if the pointer is
    /// not over a row.
    ///
    /// The bound the click path never had. Without it, a click in the 32px
    /// header strip produced a *negative* offset, and a negative `f32` cast to
    /// `usize` saturates to zero in Rust rather than wrapping -- so clicking
    /// the caption selected, decrypted and displayed the first entry in the
    /// vault. Scrolled, it selected some other entry instead, because the
    /// scroll offset was added before the cast could saturate.
    fn row_at(&self, my: f32) -> Option<usize> {
        let offset = my - Self::rows_top();
        if !offset.is_finite() || offset < 0.0 || offset >= self.rows_height() {
            return None;
        }
        let from_top = offset + self.list_scroll;
        if !from_top.is_finite() || from_top < 0.0 {
            return None;
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let idx = (from_top / ROW_HEIGHT) as usize;
        if idx < self.filtered_ids.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// How far the entry list may be scrolled before its last row sits on the
    /// bottom edge of the pane.
    ///
    /// Derived rather than measured because the list's content really is
    /// uniform -- one `ROW_HEIGHT` per filtered entry, which is exactly what
    /// `render_entry_list` draws into `rows_height`.
    fn max_list_scroll(&self) -> f32 {
        let content = self.filtered_ids.len() as f32 * ROW_HEIGHT;
        (content - self.rows_height()).max(0.0)
    }

    /// How far the detail panel may be scrolled, from the height the renderer
    /// last measured for it.
    ///
    /// Zero until something has been rendered, and zero for the generator,
    /// settings and audit views, none of which scroll.
    fn max_detail_scroll(&self) -> f32 {
        (self.detail_content_height - self.pane_height()).max(0.0)
    }

    /// Pull both offsets back inside their bounds.
    ///
    /// Needed after anything that can shorten the content under a pane that is
    /// already scrolled: a resize, a filter that drops entries, or moving to an
    /// entry with fewer fields than the one before it.
    fn clamp_scroll(&mut self) {
        self.list_scroll = self.list_scroll.clamp(0.0, self.max_list_scroll());
        self.detail_scroll = self.detail_scroll.clamp(0.0, self.max_detail_scroll());
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
fn draw_rect(rt: &mut RenderTree, x: f32, y: f32, w: f32, h: f32, color: Color, radius: f32) {
    rt.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
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
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
    line_width: f32,
    radius: f32,
) {
    rt.push(RenderCommand::StrokeRect {
        x,
        y,
        width: w,
        height: h,
        color,
        line_width,
        corner_radii: CornerRadii::all(radius),
    });
}

/// Render text at a position, marking the cut if `max_width` truncates it.
///
/// The overflow policy is derived rather than taken as a ninth argument: every
/// caller in this file draws a credential's own text — a service name, a user
/// name, a URL — where a bound exists precisely because the value is
/// variable-length and might not fit, and a fragment of a URL read as a whole
/// URL is the failure this app can least afford. A caller wanting a silent cut
/// should build the command directly and say so.
// 8 args mirror the underlying Text render command; same shape on purpose.
#[allow(clippy::too_many_arguments)]
fn draw_text(
    rt: &mut RenderTree,
    x: f32,
    y: f32,
    text: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    rt.push(RenderCommand::Text {
        x,
        y,
        text: text.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width,
        overflow: if max_width.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

/// Render a horizontal separator line.
fn draw_separator(rt: &mut RenderTree, x: f32, y: f32, width: f32) {
    rt.push(RenderCommand::Line {
        x1: x,
        y1: y,
        x2: x + width,
        y2: y,
        color: SURFACE1,
        width: 1.0,
    });
}

/// Width of the badge `draw_badge` draws for `label`.
///
/// Callers that lay something out beside a badge — the "* Starred" marker, the
/// entry name in the audit list, the tag strip's wrap test — need the badge's
/// width *before* it exists on screen. Each of them used to re-derive it from
/// `label.len()`, and they had already drifted apart: the tag strip advanced by
/// `len * 7.5 + 16` while the badge it was pacing was drawn `len * 7.0 + 12`
/// wide, and the audit list allowed 20 px of padding for a badge drawn with 12.
/// One function, measured in the weight the label is actually drawn in, is the
/// only arrangement in which the two can't disagree.
fn badge_width(label: &str) -> f32 {
    text::measure(label, SMALL_FONT_SIZE, FontWeightHint::Bold) + 12.0
}

/// Render a small colored badge with text. Returns the width it drew, so a
/// caller can lay out whatever follows without guessing at it.
fn draw_badge(rt: &mut RenderTree, x: f32, y: f32, label: &str, bg: Color, fg: Color) -> f32 {
    let badge_w = badge_width(label);
    let badge_h = 20.0;
    draw_rect(rt, x, y, badge_w, badge_h, bg, 4.0);
    draw_text(
        rt,
        x + 6.0,
        y + 3.0,
        label,
        fg,
        SMALL_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    badge_w
}

/// Render a toolbar-style button.
// 9 args: rect (x,y,w,h) + label + bg/fg colors + hovered flag; grouping these
// would not improve clarity at the call sites.
#[allow(clippy::too_many_arguments)]
fn draw_button(
    rt: &mut RenderTree,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
    bg: Color,
    fg: Color,
    hovered: bool,
) {
    let actual_bg = if hovered {
        Color::rgba(
            bg.r.saturating_add(20),
            bg.g.saturating_add(20),
            bg.b.saturating_add(20),
            bg.a,
        )
    } else {
        bg
    };
    draw_rect(rt, x, y, w, h, actual_bg, CORNER_RADIUS);
    // Centring is where a guessed width shows up worst: half the error goes
    // into the offset, and it grows with the label, so the longest label on a
    // toolbar is the one that visibly sits off-centre.
    let text_x = text::center_x(
        label,
        x + w / 2.0,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
    );
    let text_y = y + (h - DEFAULT_FONT_SIZE) / 2.0;
    draw_text(
        rt,
        text_x,
        text_y,
        label,
        fg,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
}

/// Width of a `draw_button` sized to fit `label` with `pad` px each side.
fn button_width(label: &str, pad: f32) -> f32 {
    text::measure(label, DEFAULT_FONT_SIZE, FontWeightHint::Regular) + pad * 2.0
}

/// Render a progress/strength bar.
fn draw_strength_bar(
    rt: &mut RenderTree,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fraction: f32,
    color: Color,
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
    let search_color = if state.search_query.is_empty() {
        OVERLAY0
    } else {
        TEXT_COLOR
    };
    draw_text(
        rt,
        x + 10.0,
        btn_y + 8.0,
        search_text,
        search_color,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        Some(180.0),
    );
    x += 212.0;

    // Sort button
    draw_button(
        rt,
        x,
        btn_y,
        80.0,
        btn_h,
        state.sort_order.label(),
        SURFACE1,
        TEXT_COLOR,
        false,
    );
    x += 92.0;

    // Generate password button
    draw_button(
        rt,
        x,
        btn_y,
        100.0,
        btn_h,
        "Generator",
        SURFACE1,
        LAVENDER,
        false,
    );
    x += 112.0;

    // Lock button
    let lock_text = if state.vault.is_unlocked() {
        "Lock"
    } else {
        "Unlock"
    };
    let lock_color = if state.vault.is_unlocked() {
        GREEN
    } else {
        RED
    };
    draw_button(
        rt, x, btn_y, 70.0, btn_h, lock_text, SURFACE1, lock_color, false,
    );
    x += 82.0;

    // Settings button
    draw_button(
        rt, x, btn_y, 80.0, btn_h, "Settings", SURFACE1, SUBTEXT0, false,
    );

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
    draw_text(
        rt,
        text_x,
        y,
        &state.vault.name,
        TEXT_COLOR,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        Some(SIDEBAR_WIDTH - 24.0),
    );
    y += 30.0;

    let entry_count_text = format!("{} items", state.vault.entry_count());
    draw_text(
        rt,
        text_x,
        y,
        &entry_count_text,
        SUBTEXT0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += 24.0;

    draw_separator(rt, 8.0, y, SIDEBAR_WIDTH - 16.0);
    y += 12.0;

    // Categories section
    draw_text(
        rt,
        text_x,
        y,
        "CATEGORIES",
        OVERLAY0,
        SMALL_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 20.0;

    // All Items
    let all_selected = state.sidebar_selection == SidebarSelection::AllItems;
    if all_selected {
        draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
    }
    draw_text(
        rt,
        text_x + 4.0,
        y + 8.0,
        "All Items",
        if all_selected { BLUE } else { TEXT_COLOR },
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += item_h + 2.0;

    // Favorites
    let fav_selected = state.sidebar_selection == SidebarSelection::Favorites;
    if fav_selected {
        draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
    }
    draw_text(
        rt,
        text_x + 4.0,
        y + 8.0,
        "* Favorites",
        if fav_selected { YELLOW } else { TEXT_COLOR },
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += item_h + 2.0;

    // Audit
    let audit_selected = state.sidebar_selection == SidebarSelection::Audit;
    if audit_selected {
        draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
    }
    draw_text(
        rt,
        text_x + 4.0,
        y + 8.0,
        "! Audit",
        if audit_selected { RED } else { TEXT_COLOR },
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += item_h + 8.0;

    draw_separator(rt, 8.0, y, SIDEBAR_WIDTH - 16.0);
    y += 12.0;

    // Types section
    draw_text(
        rt,
        text_x,
        y,
        "TYPES",
        OVERLAY0,
        SMALL_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 20.0;

    for etype in EntryType::all() {
        let type_selected = state.sidebar_selection == SidebarSelection::TypeFilter(*etype);
        if type_selected {
            draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
        }
        let label = format!("{} {}", etype.icon_char(), etype.label());
        let color = if type_selected {
            etype.badge_color()
        } else {
            TEXT_COLOR
        };
        draw_text(
            rt,
            text_x + 4.0,
            y + 8.0,
            &label,
            color,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        y += item_h + 2.0;
    }

    y += 6.0;
    draw_separator(rt, 8.0, y, SIDEBAR_WIDTH - 16.0);
    y += 12.0;

    // Folders section
    draw_text(
        rt,
        text_x,
        y,
        "FOLDERS",
        OVERLAY0,
        SMALL_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 20.0;

    for folder in &state.vault.folders {
        let folder_sel = state.sidebar_selection == SidebarSelection::Folder(folder.id);
        if folder_sel {
            draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
        }
        let color = if folder_sel { BLUE } else { TEXT_COLOR };
        draw_text(
            rt,
            text_x + 4.0,
            y + 8.0,
            &folder.name,
            color,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        y += item_h + 2.0;
    }

    y += 6.0;
    draw_separator(rt, 8.0, y, SIDEBAR_WIDTH - 16.0);
    y += 12.0;

    // Tags section
    draw_text(
        rt,
        text_x,
        y,
        "TAGS",
        OVERLAY0,
        SMALL_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 20.0;

    let all_tags = state.vault.all_tags();
    for tag in &all_tags {
        let tag_sel = state.sidebar_selection == SidebarSelection::Tag(tag.clone());
        if tag_sel {
            draw_rect(rt, 4.0, y, SIDEBAR_WIDTH - 8.0, item_h, SURFACE0, 4.0);
        }
        let color = if tag_sel { LAVENDER } else { TEXT_COLOR };
        draw_text(
            rt,
            text_x + 4.0,
            y + 8.0,
            tag,
            color,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        y += item_h + 2.0;
    }

    // Right border
    rt.push(RenderCommand::Line {
        x1: SIDEBAR_WIDTH,
        y1: y_start,
        x2: SIDEBAR_WIDTH,
        y2: height,
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
    draw_text(
        rt,
        x_start + 12.0,
        y_start + 10.0,
        &count_text,
        SUBTEXT0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );

    // Clip to the row area, not to the whole pane. Read from the same two
    // helpers the hit test uses, so the clip below *is* the region a click is
    // accepted in rather than a second opinion about it. The old clip started
    // at the toolbar, which included the non-scrolling header strip -- so a
    // scrolled row was painted straight over the "N entries" caption instead
    // of disappearing under it.
    let rows_y = AppState::rows_top();
    let rows_h = state.rows_height();
    let mut y = rows_y;

    rt.push(RenderCommand::PushClip {
        x: x_start,
        y: rows_y,
        width: ENTRY_LIST_WIDTH,
        height: rows_h,
    });

    let effective_y = y - state.list_scroll;

    for (i, &entry_id) in state.filtered_ids.iter().enumerate() {
        let row_y = effective_y + i as f32 * ROW_HEIGHT;

        // Skip rows outside visible area
        if row_y + ROW_HEIGHT < rows_y || row_y > rows_y + rows_h {
            continue;
        }

        if let Some(entry) = state.vault.get_entry(entry_id) {
            let is_selected = state.selected_entry_id == Some(entry_id);

            // Row background
            if is_selected {
                draw_rect(
                    rt,
                    x_start + 4.0,
                    row_y,
                    ENTRY_LIST_WIDTH - 8.0,
                    ROW_HEIGHT - 2.0,
                    SURFACE0,
                    4.0,
                );
            }

            let text_x = x_start + 16.0;

            // Type icon badge
            let badge_color = entry.entry_type().badge_color();
            draw_rect(
                rt,
                text_x,
                row_y + 8.0,
                ICON_SIZE,
                ICON_SIZE,
                badge_color,
                4.0,
            );
            draw_text(
                rt,
                text_x + 4.0,
                row_y + 10.0,
                entry.entry_type().icon_char(),
                BASE,
                SMALL_FONT_SIZE,
                FontWeightHint::Bold,
                None,
            );

            // Entry name
            let name_color = if is_selected { BLUE } else { TEXT_COLOR };
            draw_text(
                rt,
                text_x + 28.0,
                row_y + 8.0,
                entry.display_name(),
                name_color,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                Some(ENTRY_LIST_WIDTH - 60.0),
            );

            // Subtitle
            let sub = entry.subtitle();
            if !sub.is_empty() {
                draw_text(
                    rt,
                    text_x + 28.0,
                    row_y + 28.0,
                    sub,
                    SUBTEXT0,
                    SMALL_FONT_SIZE,
                    FontWeightHint::Regular,
                    Some(ENTRY_LIST_WIDTH - 80.0),
                );
            }

            // Star indicator
            if entry.starred {
                draw_text(
                    rt,
                    x_start + ENTRY_LIST_WIDTH - 30.0,
                    row_y + 8.0,
                    "*",
                    YELLOW,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Bold,
                    None,
                );
            }

            // Compromised indicator
            if entry.compromised {
                draw_text(
                    rt,
                    x_start + ENTRY_LIST_WIDTH - 48.0,
                    row_y + 8.0,
                    "!",
                    RED,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Bold,
                    None,
                );
            }

            // Bottom separator
            draw_separator(
                rt,
                x_start + 12.0,
                row_y + ROW_HEIGHT - 2.0,
                ENTRY_LIST_WIDTH - 24.0,
            );
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
        x1: list_right,
        y1: y_start,
        x2: list_right,
        y2: height,
        color: SURFACE1,
        width: 1.0,
    });
}

// =============================================================================
// Render: entry detail panel
// =============================================================================

/// Draw the detail panel, and return the height of the content it laid out.
///
/// The return value is the whole reason the panel can be scrolled to an end:
/// its length depends on which fields the entry carries, so the bound cannot
/// be computed without walking the same layout. Returning the walked height
/// means the bound and the drawing are one derivation rather than two.
fn render_entry_detail(rt: &mut RenderTree, state: &AppState, width: f32, height: f32) -> f32 {
    let x_start = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let panel_width = width - x_start;
    let panel_height = height - y_start;

    // Background
    draw_rect(rt, x_start, y_start, panel_width, panel_height, BASE, 0.0);

    let entry = match state
        .selected_entry_id
        .and_then(|id| state.vault.get_entry(id))
    {
        Some(e) => e,
        None => {
            // Empty state
            let empty = "Select an entry";
            let empty_x = text::center_x(
                empty,
                x_start + panel_width / 2.0,
                HEADING_FONT_SIZE,
                FontWeightHint::Light,
            );
            draw_text(
                rt,
                empty_x,
                y_start + panel_height / 2.0,
                empty,
                OVERLAY0,
                HEADING_FONT_SIZE,
                FontWeightHint::Light,
                None,
            );
            // Nothing selected: no content, so nothing to scroll.
            return 0.0;
        }
    };

    rt.push(RenderCommand::PushClip {
        x: x_start,
        y: y_start,
        width: panel_width,
        height: panel_height,
    });

    let pad = 24.0;
    let mut y = y_start + pad - state.detail_scroll;

    // Entry type badge + name
    let badge_color = entry.entry_type().badge_color();
    let type_badge_w = draw_badge(
        rt,
        x_start + pad,
        y,
        entry.entry_type().label(),
        badge_color,
        BASE,
    );

    if entry.starred {
        draw_text(
            rt,
            x_start + pad + type_badge_w + 12.0,
            y + 2.0,
            "* Starred",
            YELLOW,
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
    }
    y += 30.0;

    draw_text(
        rt,
        x_start + pad,
        y,
        entry.display_name(),
        TEXT_COLOR,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        Some(panel_width - pad * 2.0),
    );
    y += 28.0;

    if entry.compromised {
        draw_rect(
            rt,
            x_start + pad,
            y,
            panel_width - pad * 2.0,
            28.0,
            Color::rgba(RED.r, RED.g, RED.b, 40),
            4.0,
        );
        draw_text(
            rt,
            x_start + pad + 8.0,
            y + 6.0,
            "! This password may be compromised",
            RED,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Bold,
            None,
        );
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
            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Site",
                &login.site,
                false,
            );
            y += row_spacing;

            // Username
            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Username",
                &login.username,
                false,
            );
            y += row_spacing;

            // Password
            let pw_display = if state.show_password {
                login.password.clone()
            } else {
                "*".repeat(login.password.len().min(20))
            };
            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Password",
                &pw_display,
                true,
            );

            // Password strength
            let (strength, entropy) = evaluate_password_strength(&login.password);
            y += 8.0;
            draw_strength_bar(
                rt,
                field_value_x,
                y,
                160.0,
                6.0,
                strength.fraction(),
                strength.color(),
            );
            let strength_text = format!("{} ({:.0} bits)", strength.label(), entropy);
            draw_text(
                rt,
                field_value_x + 170.0,
                y - 2.0,
                &strength_text,
                strength.color(),
                SMALL_FONT_SIZE,
                FontWeightHint::Regular,
                None,
            );
            y += row_spacing;

            // Show/hide toggle
            let toggle_text = if state.show_password { "Hide" } else { "Show" };
            draw_button(
                rt,
                field_value_x,
                y,
                60.0,
                24.0,
                toggle_text,
                SURFACE1,
                TEXT_COLOR,
                false,
            );
            y += row_spacing;

            // URL
            if !login.url.is_empty() {
                y = render_detail_field(
                    rt,
                    y,
                    field_label_x,
                    field_value_x,
                    copy_btn_x,
                    panel_width - pad * 2.0,
                    "URL",
                    &login.url,
                    false,
                );
                y += row_spacing;
            }

            // TOTP
            if let Some(ref totp) = login.totp_secret {
                y = render_detail_field(
                    rt,
                    y,
                    field_label_x,
                    field_value_x,
                    copy_btn_x,
                    panel_width - pad * 2.0,
                    "TOTP",
                    totp,
                    false,
                );
                y += row_spacing;
            } else {
                draw_text(
                    rt,
                    field_label_x,
                    y,
                    "TOTP",
                    SUBTEXT0,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Regular,
                    None,
                );
                draw_text(
                    rt,
                    field_value_x,
                    y,
                    "Not configured",
                    OVERLAY0,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Light,
                    None,
                );
                y += row_spacing;
            }

            // Notes
            if !login.notes.is_empty() {
                draw_separator(rt, field_label_x, y, panel_width - pad * 2.0);
                y += 12.0;
                draw_text(
                    rt,
                    field_label_x,
                    y,
                    "Notes",
                    SUBTEXT0,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Bold,
                    None,
                );
                y += 20.0;
                draw_text(
                    rt,
                    field_label_x,
                    y,
                    &login.notes,
                    TEXT_COLOR,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Regular,
                    Some(panel_width - pad * 2.0),
                );
                y += 24.0;
            }
        }
        EntryData::SecureNote(note) => {
            draw_text(
                rt,
                field_label_x,
                y,
                "Title",
                SUBTEXT0,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                None,
            );
            draw_text(
                rt,
                field_value_x,
                y,
                &note.title,
                TEXT_COLOR,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                Some(panel_width - pad * 2.0 - 120.0),
            );
            y += row_spacing;

            draw_separator(rt, field_label_x, y, panel_width - pad * 2.0);
            y += 12.0;

            draw_text(
                rt,
                field_label_x,
                y,
                &note.content,
                TEXT_COLOR,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                Some(panel_width - pad * 2.0),
            );
            y += 24.0;
        }
        EntryData::CreditCard(card) => {
            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Card Name",
                &card.name,
                false,
            );
            y += row_spacing;

            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Number",
                &card.number_masked,
                false,
            );
            y += row_spacing;

            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Expiry",
                &card.expiry,
                false,
            );
            y += row_spacing;

            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Cardholder",
                &card.cardholder,
                false,
            );
            y += row_spacing;

            if !card.notes.is_empty() {
                draw_separator(rt, field_label_x, y, panel_width - pad * 2.0);
                y += 12.0;
                draw_text(
                    rt,
                    field_label_x,
                    y,
                    "Notes",
                    SUBTEXT0,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Bold,
                    None,
                );
                y += 20.0;
                draw_text(
                    rt,
                    field_label_x,
                    y,
                    &card.notes,
                    TEXT_COLOR,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Regular,
                    Some(panel_width - pad * 2.0),
                );
                y += 24.0;
            }
        }
        EntryData::Identity(ident) => {
            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Name",
                &ident.name,
                false,
            );
            y += row_spacing;

            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Email",
                &ident.email,
                false,
            );
            y += row_spacing;

            if !ident.phone.is_empty() {
                y = render_detail_field(
                    rt,
                    y,
                    field_label_x,
                    field_value_x,
                    copy_btn_x,
                    panel_width - pad * 2.0,
                    "Phone",
                    &ident.phone,
                    false,
                );
                y += row_spacing;
            }

            if !ident.address.is_empty() {
                y = render_detail_field(
                    rt,
                    y,
                    field_label_x,
                    field_value_x,
                    copy_btn_x,
                    panel_width - pad * 2.0,
                    "Address",
                    &ident.address,
                    false,
                );
                y += row_spacing;
            }
        }
        EntryData::SshKey(key) => {
            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Key Name",
                &key.name,
                false,
            );
            y += row_spacing;

            y = render_detail_field(
                rt,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Fingerprint",
                &key.fingerprint,
                false,
            );
            y += row_spacing;

            draw_text(
                rt,
                field_label_x,
                y,
                "Public Key",
                SUBTEXT0,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                None,
            );
            y += 20.0;

            draw_rect(
                rt,
                field_label_x,
                y,
                panel_width - pad * 2.0,
                60.0,
                SURFACE0,
                4.0,
            );
            draw_text(
                rt,
                field_label_x + 8.0,
                y + 8.0,
                &key.public_key,
                TEXT_COLOR,
                SMALL_FONT_SIZE,
                FontWeightHint::Regular,
                Some(panel_width - pad * 2.0 - 16.0),
            );
            y += 68.0;
        }
    }

    // Tags section
    if !entry.tags.is_empty() {
        y += 8.0;
        draw_separator(rt, field_label_x, y, panel_width - pad * 2.0);
        y += 12.0;
        draw_text(
            rt,
            field_label_x,
            y,
            "Tags",
            SUBTEXT0,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Bold,
            None,
        );
        y += 22.0;

        let mut tag_x = field_label_x;
        for tag in &entry.tags {
            let tag_w = badge_width(tag);
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

    let created_text = format!(
        "Created: {} seconds ago",
        state.now.saturating_sub(entry.created_at)
    );
    draw_text(
        rt,
        field_label_x,
        y,
        &created_text,
        OVERLAY0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += 18.0;

    let modified_text = format!(
        "Modified: {} seconds ago",
        state.now.saturating_sub(entry.modified_at)
    );
    draw_text(
        rt,
        field_label_x,
        y,
        &modified_text,
        OVERLAY0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );

    if entry.entry_type() == EntryType::Login {
        y += 18.0;
        let age_days = entry.password_age_days(state.now);
        let age_color = if age_days > PASSWORD_OLD_DAYS {
            YELLOW
        } else {
            OVERLAY0
        };
        let age_text = format!("Password age: {} days", age_days);
        draw_text(
            rt,
            field_label_x,
            y,
            &age_text,
            age_color,
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
    }

    rt.push(RenderCommand::PopClip);

    // `y` is the baseline of the last thing drawn, in screen coordinates with
    // the scroll already subtracted; adding it back gives the unscrolled
    // bottom of the content, and a trailing `pad` keeps the final line off the
    // panel edge at full scroll. This used to be discarded with `let _ = y`.
    y + state.detail_scroll - y_start + pad
}

/// Render a single labeled field row with optional copy button.
// 9 args: layout positions (y, label_x, value_x, copy_x, width) + 2 strings +
// flag + render tree. All needed at the call site; no useful grouping.
#[allow(clippy::too_many_arguments)]
fn render_detail_field(
    rt: &mut RenderTree,
    y: f32,
    label_x: f32,
    value_x: f32,
    copy_x: f32,
    _width: f32,
    label: &str,
    value: &str,
    is_password: bool,
) -> f32 {
    draw_text(
        rt,
        label_x,
        y,
        label,
        SUBTEXT0,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );

    let value_color = if is_password { PEACH } else { TEXT_COLOR };
    draw_text(
        rt,
        value_x,
        y,
        value,
        value_color,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        Some(copy_x - value_x - 8.0),
    );

    // Copy button
    draw_button(
        rt,
        copy_x,
        y - 4.0,
        44.0,
        24.0,
        "Copy",
        SURFACE1,
        SUBTEXT0,
        false,
    );

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

    draw_text(
        rt,
        x_start + pad,
        y,
        "Password Generator",
        TEXT_COLOR,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 36.0;

    // Generated password display
    draw_rect(
        rt,
        x_start + pad,
        y,
        panel_width - pad * 2.0,
        48.0,
        SURFACE0,
        CORNER_RADIUS,
    );
    // A refusal takes the password's own place, in red. Left in the prompt
    // state it would read as "you have not pressed the button yet", which is
    // exactly the wrong thing to tell someone whose generator cannot generate.
    let (display_pw, pw_color) = match (&state.generator_error, state.generated_password.is_empty())
    {
        (Some(message), _) => (message.as_str(), RED),
        (None, true) => ("Click Generate to create a password", OVERLAY0),
        (None, false) => (state.generated_password.as_str(), GREEN),
    };
    draw_text(
        rt,
        x_start + pad + 12.0,
        y + 14.0,
        display_pw,
        pw_color,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        Some(panel_width - pad * 2.0 - 24.0),
    );
    y += 56.0;

    // Strength bar for generated password
    if !state.generated_password.is_empty() {
        let (strength, entropy) = evaluate_password_strength(&state.generated_password);
        draw_strength_bar(
            rt,
            x_start + pad,
            y,
            panel_width - pad * 2.0,
            8.0,
            strength.fraction(),
            strength.color(),
        );
        y += 16.0;
        let label = format!("{} - {:.0} bits entropy", strength.label(), entropy);
        draw_text(
            rt,
            x_start + pad,
            y,
            &label,
            strength.color(),
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        y += 24.0;
    }

    // Buttons row
    draw_button(
        rt,
        x_start + pad,
        y,
        100.0,
        32.0,
        "Generate",
        BLUE,
        BASE,
        false,
    );
    draw_button(
        rt,
        x_start + pad + 112.0,
        y,
        80.0,
        32.0,
        "Copy",
        SURFACE1,
        TEXT_COLOR,
        false,
    );
    y += 48.0;

    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 16.0;

    // Mode selection
    draw_text(
        rt,
        x_start + pad,
        y,
        "Mode",
        TEXT_COLOR,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
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
        let btn_w = button_width(label, 10.0);
        draw_button(rt, mode_x, y, btn_w, 28.0, label, bg, fg, false);
        mode_x += btn_w + 8.0;
    }
    y += 40.0;

    // Length setting
    draw_text(
        rt,
        x_start + pad,
        y,
        "Length",
        TEXT_COLOR,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    let len_text = format!("{}", state.password_generator.length);
    draw_text(
        rt,
        x_start + pad + 100.0,
        y,
        &len_text,
        BLUE,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
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
        draw_text(
            rt,
            x_start + pad,
            y,
            "Character Sets",
            TEXT_COLOR,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Bold,
            None,
        );
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
            draw_text(
                rt,
                x_start + pad,
                y,
                check_char,
                check_color,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                None,
            );
            draw_text(
                rt,
                x_start + pad + 32.0,
                y,
                label,
                TEXT_COLOR,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                None,
            );
            y += 26.0;
        }
    }

    // Passphrase options
    if state.password_generator.mode == GeneratorMode::Passphrase {
        draw_text(
            rt,
            x_start + pad,
            y,
            "Word Count",
            TEXT_COLOR,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        let wc_text = format!("{}", state.password_generator.passphrase.word_count);
        draw_text(
            rt,
            x_start + pad + 120.0,
            y,
            &wc_text,
            BLUE,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Bold,
            None,
        );
        y += 26.0;

        draw_text(
            rt,
            x_start + pad,
            y,
            "Separator",
            TEXT_COLOR,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        draw_text(
            rt,
            x_start + pad + 120.0,
            y,
            &state.password_generator.passphrase.separator,
            BLUE,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Bold,
            None,
        );
        y += 26.0;
    }

    // Entropy info
    y += 8.0;
    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 12.0;

    let entropy = state.password_generator.entropy_bits();
    let entropy_text = format!("Estimated entropy: {:.1} bits", entropy);
    draw_text(
        rt,
        x_start + pad,
        y,
        &entropy_text,
        SUBTEXT0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += 18.0;

    let pool_text = match state.password_generator.mode {
        GeneratorMode::Random => {
            format!(
                "Pool size: {} characters",
                state.password_generator.charset.pool_size()
            )
        }
        GeneratorMode::Pronounceable => "Pool: alternating consonant/vowel".to_string(),
        GeneratorMode::Passphrase => {
            format!("Dictionary: {} words", WORDLIST.len())
        }
    };
    draw_text(
        rt,
        x_start + pad,
        y,
        &pool_text,
        SUBTEXT0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );

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

    draw_text(
        rt,
        x_start + pad,
        y,
        "Settings",
        TEXT_COLOR,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 36.0;

    // Security section
    draw_text(
        rt,
        x_start + pad,
        y,
        "SECURITY",
        OVERLAY0,
        SMALL_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 24.0;

    draw_text(
        rt,
        x_start + pad,
        y,
        "Auto-lock timeout",
        TEXT_COLOR,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    let timeout_text = format!("{} minutes", state.settings_auto_lock);
    draw_text(
        rt,
        x_start + pad + 200.0,
        y,
        &timeout_text,
        BLUE,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 32.0;

    // Timeout slider
    let slider_x = x_start + pad;
    let slider_w = panel_width - pad * 2.0;
    draw_rect(rt, slider_x, y, slider_w, 4.0, SURFACE1, 2.0);
    let frac = (state.settings_auto_lock as f32 - 1.0) / 59.0;
    let knob_x = slider_x + slider_w * frac.clamp(0.0, 1.0);
    draw_rect(rt, knob_x - 6.0, y - 4.0, 12.0, 12.0, BLUE, 6.0);
    y += 24.0;

    draw_text(
        rt,
        x_start + pad,
        y,
        "Clipboard auto-clear",
        TEXT_COLOR,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    let clear_text = format!("{} seconds", state.clipboard.auto_clear_seconds);
    draw_text(
        rt,
        x_start + pad + 200.0,
        y,
        &clear_text,
        BLUE,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 36.0;

    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 16.0;

    // Vault info section
    draw_text(
        rt,
        x_start + pad,
        y,
        "VAULT INFO",
        OVERLAY0,
        SMALL_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 24.0;

    let info_items = [
        ("Vault name", state.vault.name.as_str()),
        (
            "Status",
            if state.vault.is_unlocked() {
                "Unlocked"
            } else {
                "Locked"
            },
        ),
    ];
    for (label, value) in &info_items {
        draw_text(
            rt,
            x_start + pad,
            y,
            label,
            SUBTEXT0,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        draw_text(
            rt,
            x_start + pad + 160.0,
            y,
            value,
            TEXT_COLOR,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        y += 26.0;
    }

    let count_text = format!("{}", state.vault.entries.len());
    draw_text(
        rt,
        x_start + pad,
        y,
        "Total entries",
        SUBTEXT0,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    draw_text(
        rt,
        x_start + pad + 160.0,
        y,
        &count_text,
        TEXT_COLOR,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += 26.0;

    let folder_count_text = format!("{}", state.vault.folders.len());
    draw_text(
        rt,
        x_start + pad,
        y,
        "Folders",
        SUBTEXT0,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    draw_text(
        rt,
        x_start + pad + 160.0,
        y,
        &folder_count_text,
        TEXT_COLOR,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += 36.0;

    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 16.0;

    // Export section
    draw_text(
        rt,
        x_start + pad,
        y,
        "DATA",
        OVERLAY0,
        SMALL_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 24.0;

    draw_button(
        rt,
        x_start + pad,
        y,
        120.0,
        32.0,
        "Export CSV",
        SURFACE1,
        TEXT_COLOR,
        false,
    );
    draw_button(
        rt,
        x_start + pad + 132.0,
        y,
        120.0,
        32.0,
        "Backup",
        SURFACE1,
        TEXT_COLOR,
        false,
    );

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

    draw_text(
        rt,
        x_start + pad,
        y,
        "Password Audit",
        TEXT_COLOR,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 28.0;

    if state.audit_issues.is_empty() {
        draw_text(
            rt,
            x_start + pad,
            y,
            "No issues found. All passwords look good!",
            GREEN,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        return;
    }

    let summary = format!("{} issues found", state.audit_issues.len());
    draw_text(
        rt,
        x_start + pad,
        y,
        &summary,
        YELLOW,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 28.0;

    draw_separator(rt, x_start + pad, y, panel_width - pad * 2.0);
    y += 12.0;

    rt.push(RenderCommand::PushClip {
        x: x_start,
        y,
        width: panel_width,
        height: panel_height - (y - y_start),
    });

    for issue in &state.audit_issues {
        if y > y_start + panel_height {
            break;
        }

        let issue_color = issue.issue.severity_color();

        draw_rect(
            rt,
            x_start + pad,
            y,
            panel_width - pad * 2.0,
            36.0,
            SURFACE0,
            4.0,
        );

        // Issue severity badge
        let severity_w = draw_badge(
            rt,
            x_start + pad + 8.0,
            y + 8.0,
            issue.issue.label(),
            issue_color,
            BASE,
        );

        // Entry name, laid out from the width the badge actually drew.
        draw_text(
            rt,
            x_start + pad + 8.0 + severity_w + 16.0,
            y + 10.0,
            &issue.entry_name,
            TEXT_COLOR,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            Some(panel_width - pad * 2.0 - severity_w - 32.0),
        );

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
        x: px,
        y: py,
        width: panel_w,
        height: panel_h,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 24.0,
        spread: 0.0,
        color: Color::rgba(0, 0, 0, 100),
        corner_radii: CornerRadii::all(12.0),
    });
    draw_rect(rt, px, py, panel_w, panel_h, SURFACE0, 12.0);

    // Lock icon
    draw_text(
        rt,
        text::center_x("[=]", center_x, 24.0, FontWeightHint::Bold),
        py + 30.0,
        "[=]",
        BLUE,
        24.0,
        FontWeightHint::Bold,
        None,
    );

    // Vault name. A vault named in any non-ASCII script used to drift left of
    // centre by half its excess byte count, since the offset was `len * 5.0`.
    let name_x = text::center_x(
        &state.vault.name,
        center_x,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
    );
    draw_text(
        rt,
        name_x,
        py + 70.0,
        &state.vault.name,
        TEXT_COLOR,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );

    // Instruction
    let instruction = "Enter master password";
    let instruction_x = text::center_x(
        instruction,
        center_x,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
    );
    draw_text(
        rt,
        instruction_x,
        py + 100.0,
        instruction,
        SUBTEXT0,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );

    // Password input field
    let input_x = px + 30.0;
    let input_y = py + 130.0;
    let input_w = panel_w - 60.0;
    let input_h = 40.0;

    let border_color = if state.unlock_failed { RED } else { SURFACE2 };
    draw_rect(rt, input_x, input_y, input_w, input_h, BASE, CORNER_RADIUS);
    draw_stroke_rect(
        rt,
        input_x,
        input_y,
        input_w,
        input_h,
        border_color,
        1.0,
        CORNER_RADIUS,
    );

    // Masked input display
    let masked: String = "*".repeat(state.master_input.len());
    let display = if masked.is_empty() {
        "Password..."
    } else {
        &masked
    };
    let display_color = if masked.is_empty() {
        OVERLAY0
    } else {
        TEXT_COLOR
    };
    draw_text(
        rt,
        input_x + 12.0,
        input_y + 12.0,
        display,
        display_color,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        Some(input_w - 24.0),
    );

    // Error message
    if state.unlock_failed {
        let error = "Incorrect password";
        let error_x = text::center_x(error, center_x, SMALL_FONT_SIZE, FontWeightHint::Regular);
        draw_text(
            rt,
            error_x,
            input_y + input_h + 8.0,
            error,
            RED,
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
    }

    // Unlock button
    let btn_y = py + 200.0;
    draw_button(
        rt,
        center_x - 50.0,
        btn_y,
        100.0,
        36.0,
        "Unlock",
        BLUE,
        BASE,
        false,
    );
}

// =============================================================================
// Build complete render tree
// =============================================================================

/// Draw the whole window, and record what the detail panel measured.
///
/// Takes the state by `&mut` so the measurement has somewhere to go: the
/// detail panel's height is only known once it has been laid out, and the
/// wheel handler needs it to know where the panel ends. The window size comes
/// from the state rather than from parameters, so that there is one answer to
/// how big the window is instead of two that can disagree -- the parameters
/// were the reason nothing but the renderer knew the size.
fn build_render_tree(state: &mut AppState) -> RenderTree {
    let mut rt = RenderTree::new();
    let (width, height) = (state.width, state.height);

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

    // Detail panel (depends on view). Only the entry detail scrolls; the other
    // three are fixed-height panels, so they measure as zero content and their
    // bound comes out zero.
    let detail_content = match state.detail_view {
        DetailView::EntryDetail => render_entry_detail(&mut rt, state, width, height),
        DetailView::PasswordGenerator => {
            render_generator_panel(&mut rt, state, width, height);
            0.0
        }
        DetailView::Settings => {
            render_settings_panel(&mut rt, state, width, height);
            0.0
        }
        DetailView::AuditReport => {
            render_audit_panel(&mut rt, state, width, height);
            0.0
        }
    };
    state.detail_content_height = detail_content;
    // A shorter entry than the last one can leave the offset past the end.
    state.clamp_scroll();

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
        Event::Resize { width, height } => {
            // Until this arm existed the size lived only in the renderer's
            // parameters, so the scroll bounds had nothing to be computed
            // from. Growing the window can leave a pane scrolled past its own
            // end, hence the re-clamp.
            state.width = *width as f32;
            state.height = *height as f32;
            state.clamp_scroll();
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
                    && !ch.is_control()
                {
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
            regenerate_password(state);
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
                regenerate_password(state);
            }
        }
        _ => {
            // Text input for search
            if let Some(ch) = key.text
                && !ch.is_control()
            {
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

    let current_idx = state
        .selected_entry_id
        .and_then(|id| state.filtered_ids.iter().position(|&fid| fid == id));

    // Walked in `usize` rather than through `i32`: the list is indexed by
    // `usize`, and the round trip out to a signed type and back is where the
    // clamp's `len() as i32 - 1` used to sit — correct only because the empty
    // case returns above, and silently wrong on a list longer than `i32::MAX`.
    let last = state.filtered_ids.len().saturating_sub(1);
    let step = direction.unsigned_abs() as usize;
    let new_idx = match current_idx {
        Some(idx) if direction < 0 => idx.saturating_sub(step),
        Some(idx) => idx.saturating_add(step).min(last),
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
        // `wheel::pixels` and not an `Accumulator`: both offsets are already
        // continuous, so a trackpad's fifth of a notch can be shown as a fifth
        // of a row straight away rather than banked until it rounds. The
        // `20.0` per notch it replaces was one of a dozen private guesses in
        // this tree at what a notch is worth -- these rows are 52 px, so it
        // was not half of one.
        //
        // Both are now clamped at the far end as well as at zero. They were
        // clamped with `.max(0.0)` alone, which let either pane be wound off
        // the end of its content into blank space and kept going.
        if mouse.x >= SIDEBAR_WIDTH && mouse.x < SIDEBAR_WIDTH + ENTRY_LIST_WIDTH {
            state.list_scroll = (state.list_scroll + wheel::pixels(dy, ROW_HEIGHT))
                .clamp(0.0, state.max_list_scroll());
        } else if mouse.x >= SIDEBAR_WIDTH + ENTRY_LIST_WIDTH {
            state.detail_scroll = (state.detail_scroll + wheel::pixels(dy, DETAIL_LINE_HEIGHT))
                .clamp(0.0, state.max_detail_scroll());
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
            regenerate_password(state);
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
    let Some(row_idx) = state.row_at(my) else {
        return;
    };

    if let Some(&entry_id) = state.filtered_ids.get(row_idx) {
        state.selected_entry_id = Some(entry_id);
        state.detail_view = DetailView::EntryDetail;
        state.show_password = false;
        // A new entry's fields start at the top, not wherever the last one had
        // been scrolled to.
        state.detail_scroll = 0.0;
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
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

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
        assert_eq!(
            CreditCardData::mask_number("4111111111111111"),
            "************1111"
        );
    }

    #[test]
    fn test_mask_number_short() {
        assert_eq!(CreditCardData::mask_number("123"), "***");
    }

    #[test]
    fn test_mask_number_with_spaces() {
        assert_eq!(
            CreditCardData::mask_number("4111 1111 1111 1111"),
            "************1111"
        );
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

    // == Master password ======================================================
    //
    // These replace three tests of the djb2 `simple_hash` this crate used to
    // check the master password with. All three passed — the function *was*
    // deterministic, *did* map two distinct short inputs apart, and *did*
    // start from 5381 — and not one of them could have failed for the reason
    // the code was wrong. That is the shape to watch for: a green suite
    // asserting the properties an implementation happens to have, rather than
    // the ones its caller depends on.

    /// The property djb2 could not have had: a salt. Without one the same
    /// password yields the same stored value in every vault on every machine,
    /// so a single precomputed table opens all of them — and equal stored
    /// values advertise which vaults to try it against.
    ///
    /// Asserted against `Vault::create`, the real path, so it fails if the
    /// salt ever silently becomes a constant again. On a host build there is
    /// no kernel entropy source, so `create` refuses — and *that* is asserted
    /// instead, because what must never happen is a salt appearing anyway.
    #[test]
    fn two_vaults_with_the_same_password_do_not_store_the_same_verifier() {
        match (
            Vault::create("A", "correct horse"),
            Vault::create("B", "correct horse"),
        ) {
            (Ok(a), Ok(b)) => {
                assert_ne!(
                    a.master.params().salt(),
                    b.master.params().salt(),
                    "two vaults drew the same salt -- it is a constant again"
                );
                assert_ne!(
                    a.master.verifier(),
                    b.master.verifier(),
                    "the salt did not reach the stored value"
                );
                // And both still open with the password they were made from.
                assert!(a.master.check(b"correct horse"));
                assert!(b.master.check(b"correct horse"));
            }
            (Err(a), Err(b)) => {
                assert_eq!(a, KdfError::EntropyUnavailable);
                assert_eq!(b, KdfError::EntropyUnavailable);
            }
            _ => panic!("an entropy source that works only sometimes is the worst case of all"),
        }
    }

    /// The salt has to reach the stored value for any of this to mean
    /// anything. Guards against it being kept, round-tripped and then ignored,
    /// which would leave every vault behaving exactly as it did with djb2.
    #[test]
    fn the_salt_changes_the_stored_verifier() {
        let one = KdfParams::new(*b"salt number one!", TEST_ROUNDS);
        let two = KdfParams::new(*b"salt number two!", TEST_ROUNDS);
        assert_ne!(
            PasswordVerifier::create(b"identical password", one, VERIFIER_DOMAIN).verifier(),
            PasswordVerifier::create(b"identical password", two, VERIFIER_DOMAIN).verifier()
        );
    }

    #[test]
    fn a_verifier_is_worthless_without_the_salt_it_was_made_under() {
        // Why `master` holds a `PasswordVerifier` rather than a bare hash: a
        // persistence layer that writes the stored value and drops the salt
        // has locked the owner out, and the symptom — "correct password
        // refused" — does not point at the cause.
        let v = Vault::for_test("V", "correct horse");
        let wrong_salt = KdfParams::new([0xA5u8; pwkdf::SALT_LEN], TEST_ROUNDS);
        let reopened = Vault::from_stored("V", wrong_salt, v.master.verifier());
        assert!(v.master.check(b"correct horse"));
        assert!(!reopened.master.check(b"correct horse"));
    }

    #[test]
    fn a_vault_verifier_cannot_be_replayed_against_the_lock_screen() {
        // The domain label is the whole of this property, and changing it is a
        // one-word edit with no local symptom, so it is pinned here.
        let params = KdfParams::new([0x5Au8; pwkdf::SALT_LEN], TEST_ROUNDS);
        let vault = PasswordVerifier::create(b"correct horse", params, VERIFIER_DOMAIN);
        let lockscreen =
            PasswordVerifier::create(b"correct horse", params, b"slateos-lockscreen-verifier");
        assert_ne!(vault.verifier(), lockscreen.verifier());
    }

    #[test]
    fn a_stored_vault_reopens_with_the_password_it_was_created_from() {
        let original = Vault::for_test("V", "correct horse");
        let mut reopened =
            Vault::from_stored("V", original.master.params(), original.master.verifier());
        assert!(!reopened.unlock("wrong", 100));
        assert!(reopened.unlock("correct horse", 100));
    }

    // == Vault tests ===========================================================

    #[test]
    fn test_vault_new() {
        let v = Vault::for_test("Test", "password");
        assert_eq!(v.name, "Test");
        assert_eq!(v.state, VaultState::Locked);
        assert_eq!(v.auto_lock_minutes, DEFAULT_AUTO_LOCK_MINUTES);
    }

    #[test]
    fn test_vault_unlock_correct() {
        let mut v = Vault::for_test("Test", "secret");
        assert!(v.unlock("secret", 100));
        assert!(v.is_unlocked());
    }

    #[test]
    fn test_vault_unlock_incorrect() {
        let mut v = Vault::for_test("Test", "secret");
        assert!(!v.unlock("wrong", 100));
        assert!(!v.is_unlocked());
    }

    #[test]
    fn test_vault_lock() {
        let mut v = Vault::for_test("Test", "pw");
        v.unlock("pw", 100);
        assert!(v.is_unlocked());
        v.lock();
        assert!(!v.is_unlocked());
    }

    #[test]
    fn test_vault_auto_lock() {
        let mut v = Vault::for_test("Test", "pw");
        v.auto_lock_minutes = 5;
        v.unlock("pw", 100);
        assert!(!v.should_auto_lock(100));
        assert!(!v.should_auto_lock(399)); // 299s < 300s
        assert!(v.should_auto_lock(400)); // 300s >= 300s
    }

    #[test]
    fn test_vault_auto_lock_when_locked() {
        let v = Vault::for_test("Test", "pw");
        assert!(!v.should_auto_lock(99999));
    }

    #[test]
    fn test_vault_add_entry() {
        let mut v = Vault::for_test("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        assert!(id > 0);
        assert_eq!(v.entries.len(), 1);
        assert!(v.get_entry(id).is_some());
    }

    #[test]
    fn test_vault_remove_entry() {
        let mut v = Vault::for_test("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        assert!(v.remove_entry(id));
        assert_eq!(v.entries.len(), 0);
        assert!(!v.remove_entry(999));
    }

    #[test]
    fn test_vault_update_entry() {
        let mut v = Vault::for_test("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("old", "u", "p")), 100);
        let new_data = EntryData::Login(LoginData::new("new", "u2", "p2"));
        assert!(v.update_entry(id, new_data, 200));
        assert_eq!(v.get_entry(id).map(|e| e.display_name()), Some("new"));
        assert_eq!(v.get_entry(id).map(|e| e.modified_at), Some(200));
    }

    #[test]
    fn test_vault_toggle_star() {
        let mut v = Vault::for_test("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        assert!(!v.get_entry(id).is_some_and(|e| e.starred));
        v.toggle_star(id);
        assert!(v.get_entry(id).is_some_and(|e| e.starred));
        v.toggle_star(id);
        assert!(!v.get_entry(id).is_some_and(|e| e.starred));
    }

    #[test]
    fn test_vault_compromised() {
        let mut v = Vault::for_test("V", "pw");
        let id = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        v.set_compromised(id, true);
        assert!(v.get_entry(id).is_some_and(|e| e.compromised));
        v.set_compromised(id, false);
        assert!(!v.get_entry(id).is_some_and(|e| e.compromised));
    }

    #[test]
    fn test_vault_tags() {
        let mut v = Vault::for_test("V", "pw");
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
        let mut v = Vault::for_test("V", "pw");
        let fid = v.add_folder("Work");
        let eid = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        v.set_folder(eid, Some(fid));
        assert_eq!(v.get_entry(eid).map(|e| e.folder_id), Some(Some(fid)));
    }

    #[test]
    fn test_vault_add_folder() {
        let mut v = Vault::for_test("V", "pw");
        let id = v.add_folder("Personal");
        assert!(v.get_folder(id).is_some());
        assert_eq!(v.get_folder(id).map(|f| f.name.as_str()), Some("Personal"));
    }

    #[test]
    fn test_vault_remove_folder_clears_entries() {
        let mut v = Vault::for_test("V", "pw");
        let fid = v.add_folder("Work");
        let eid = v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        v.set_folder(eid, Some(fid));
        v.remove_folder(fid);
        assert_eq!(v.get_entry(eid).map(|e| e.folder_id), Some(None));
    }

    #[test]
    fn test_vault_rename_folder() {
        let mut v = Vault::for_test("V", "pw");
        let fid = v.add_folder("Old");
        assert!(v.rename_folder(fid, "New"));
        assert_eq!(v.get_folder(fid).map(|f| f.name.as_str()), Some("New"));
    }

    #[test]
    fn test_vault_entries_in_folder() {
        let mut v = Vault::for_test("V", "pw");
        let fid = v.add_folder("Work");
        let id1 = v.add_entry(EntryData::Login(LoginData::new("s1", "u", "p")), 100);
        let _id2 = v.add_entry(EntryData::Login(LoginData::new("s2", "u", "p")), 100);
        v.set_folder(id1, Some(fid));
        assert_eq!(v.entries_in_folder(Some(fid)).len(), 1);
        assert_eq!(v.entries_in_folder(None).len(), 1);
    }

    #[test]
    fn test_vault_starred_entries() {
        let mut v = Vault::for_test("V", "pw");
        let id1 = v.add_entry(EntryData::Login(LoginData::new("s1", "u", "p")), 100);
        let _id2 = v.add_entry(EntryData::Login(LoginData::new("s2", "u", "p")), 100);
        v.toggle_star(id1);
        assert_eq!(v.starred_entries().len(), 1);
    }

    #[test]
    fn test_vault_entries_with_tag() {
        let mut v = Vault::for_test("V", "pw");
        let id1 = v.add_entry(EntryData::Login(LoginData::new("s1", "u", "p")), 100);
        v.add_tag(id1, "tag1");
        assert_eq!(v.entries_with_tag("tag1").len(), 1);
        assert_eq!(v.entries_with_tag("tag2").len(), 0);
    }

    #[test]
    fn test_vault_entries_of_type() {
        let mut v = Vault::for_test("V", "pw");
        v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        v.add_entry(EntryData::SecureNote(SecureNoteData::new("n", "c")), 100);
        assert_eq!(v.entries_of_type(EntryType::Login).len(), 1);
        assert_eq!(v.entries_of_type(EntryType::SecureNote).len(), 1);
        assert_eq!(v.entries_of_type(EntryType::CreditCard).len(), 0);
    }

    #[test]
    fn test_vault_search() {
        let mut v = Vault::for_test("V", "pw");
        v.add_entry(
            EntryData::Login(LoginData::new("github.com", "alice", "pass")),
            100,
        );
        v.add_entry(
            EntryData::Login(LoginData::new("gitlab.com", "bob", "pass")),
            100,
        );
        assert_eq!(v.search_entries("git").len(), 2);
        assert_eq!(v.search_entries("alice").len(), 1);
        assert_eq!(v.search_entries("").len(), 2);
        assert_eq!(v.search_entries("zzz").len(), 0);
    }

    #[test]
    fn test_vault_all_tags() {
        let mut v = Vault::for_test("V", "pw");
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

    /// A password from a named sequence. Every generator test uses a seeded
    /// source: `PasswordGenerator::new()` reaches for the kernel, which the
    /// host test toolchain does not have, so it would refuse on this machine
    /// and every assertion below would be about a refusal.
    fn seeded(seed: u64) -> PasswordGenerator {
        PasswordGenerator::with_seed(seed)
    }

    #[test]
    fn test_generator_random_length() {
        let mut pg = seeded(1);
        pg.set_length(16);
        let pw = pg.generate().expect("a seeded source always generates");
        assert_eq!(pw.chars().count(), 16);
    }

    #[test]
    fn test_generator_random_deterministic() {
        let mut gen1 = seeded(42);
        gen1.set_length(20);
        let pw1 = gen1.generate();

        let mut gen2 = seeded(42);
        gen2.set_length(20);
        let pw2 = gen2.generate();

        assert_eq!(pw1, pw2);
    }

    #[test]
    fn test_generator_random_empty_charset() {
        let mut pg = seeded(2);
        pg.charset = CharsetOptions {
            uppercase: false,
            lowercase: false,
            digits: false,
            symbols: false,
        };
        // Every character class switched off is an empty password, not a
        // refusal: the source is fine, there is simply nothing to draw from.
        assert_eq!(pg.generate(), Some(String::new()));
    }

    #[test]
    fn test_generator_pronounceable() {
        let mut pg = seeded(3);
        pg.mode = GeneratorMode::Pronounceable;
        pg.set_length(10);
        let pw = pg.generate().expect("a seeded source always generates");
        assert_eq!(pw.chars().count(), 10);
        // Should alternate consonant/vowel
        for (i, ch) in pw.chars().enumerate() {
            if i % 2 == 0 {
                assert!(!"aeiou".contains(ch), "Even idx should be consonant: {ch}");
            } else {
                assert!("aeiou".contains(ch), "Odd idx should be vowel: {ch}");
            }
        }
    }

    #[test]
    fn test_generator_passphrase() {
        let mut pg = seeded(4);
        pg.mode = GeneratorMode::Passphrase;
        pg.passphrase.word_count = 4;
        pg.passphrase.separator = "-".to_string();
        let pw = pg.generate().expect("a seeded source always generates");
        let words: Vec<&str> = pw.split('-').collect();
        assert_eq!(words.len(), 4);
        for word in &words {
            assert!(WORDLIST.contains(word));
        }
    }

    #[test]
    fn test_generator_passphrase_custom_separator() {
        let mut pg = seeded(5);
        pg.mode = GeneratorMode::Passphrase;
        pg.passphrase.word_count = 3;
        pg.passphrase.separator = ".".to_string();
        let pw = pg.generate().expect("a seeded source always generates");
        assert_eq!(pw.split('.').count(), 3);
    }

    // ---- the entropy the generator actually has ----------------------

    #[test]
    fn two_generators_do_not_produce_the_same_password() {
        // The defect this replaced: the seed was the literal 12345 on every
        // install, so the first password this manager ever generated for you
        // was the first it generated for everyone. Two independently-built
        // generators must not agree.
        let mut first = seeded(1);
        let mut second = seeded(2);
        assert_ne!(first.generate(), second.generate());
    }

    #[test]
    fn every_mode_refuses_when_there_is_no_entropy() {
        for mode in [
            GeneratorMode::Random,
            GeneratorMode::Pronounceable,
            GeneratorMode::Passphrase,
        ] {
            let mut pg = PasswordGenerator::without_entropy();
            pg.mode = mode;
            assert_eq!(
                pg.generate(),
                None,
                "{mode:?} handed out a password with no randomness behind it"
            );
        }
    }

    #[test]
    fn a_source_with_no_entropy_is_never_trustworthy() {
        assert!(!CredRandom::Unavailable.is_trustworthy());
        assert!(CredRandom::Seeded(SeededRng::new(1)).is_trustworthy());
    }

    #[test]
    fn a_refusal_clears_the_password_that_was_showing() {
        // The dangerous shape is a stale password left on screen beside a
        // message saying the generator is unavailable — it reads as an offer.
        let mut state = AppState::for_test();
        state.generated_password = "hunter2".to_string();
        state.password_generator = PasswordGenerator::without_entropy();
        regenerate_password(&mut state);
        assert!(state.generated_password.is_empty());
        assert_eq!(state.generator_error.as_deref(), Some(NO_ENTROPY_MESSAGE));
    }

    #[test]
    fn a_successful_generation_clears_an_earlier_refusal() {
        let mut state = AppState::for_test();
        state.generator_error = Some(NO_ENTROPY_MESSAGE.to_string());
        state.password_generator = seeded(9);
        regenerate_password(&mut state);
        assert!(!state.generated_password.is_empty());
        assert_eq!(state.generator_error, None);
    }

    #[test]
    fn the_refusal_is_shown_where_the_password_would_be() {
        let mut state = AppState::for_test();
        state.detail_view = DetailView::PasswordGenerator;
        state.password_generator = PasswordGenerator::without_entropy();
        regenerate_password(&mut state);

        let mut rt = RenderTree::new();
        render_generator_panel(&mut rt, &state, 1200.0, 800.0);
        let shown = rt.commands.iter().any(
            |cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == NO_ENTROPY_MESSAGE),
        );
        assert!(shown, "the refusal never reached the screen");
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
            uppercase: false,
            lowercase: false,
            digits: false,
            symbols: false,
        };
        assert_eq!(pg.entropy_bits(), 0.0);
    }

    #[test]
    fn pressing_generate_twice_gives_two_different_passwords() {
        // This was `test_generator_seed_advances`, which watched the counter
        // rather than the passwords. The counter is gone; what it was really
        // asking is this, and this is the part a user would notice.
        let mut pg = seeded(77);
        pg.set_length(24);
        assert_ne!(pg.generate(), pg.generate());
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
    fn strength_counts_characters_rather_than_bytes() {
        // `password.len()` counted a three-byte character as three, so this
        // four-character password scored as a twelve-character one — an
        // overstatement, which is the one direction a strength meter must not
        // err in. Both passwords here have four characters and land in the
        // same character class, so their entropy must be equal.
        let (_, wide) = evaluate_password_strength("тест");
        let (_, narrow) = evaluate_password_strength("!@#$");
        assert!(
            (wide - narrow).abs() < 1e-9,
            "four characters scored as {wide} bits against {narrow} for four ASCII ones"
        );
    }

    #[test]
    fn the_generators_symbols_are_the_ones_the_meter_scores_against() {
        // The two used to be `"!@#$…"` in one place and a bare `30` in the
        // other. Adding a character to the alphabet would then have made every
        // generated password score against a pool it was not drawn from.
        let all = CharsetOptions::default();
        assert_eq!(
            all.pool_size(),
            ASCII_LETTER_COUNT * 2 + ASCII_DIGIT_COUNT + estimate_symbol_pool()
        );
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
        let mut v = Vault::for_test("V", "pw");
        v.add_entry(EntryData::Login(LoginData::new("site", "user", "abc")), 100);
        let issues = audit_vault(&v, 200);
        assert!(
            issues
                .iter()
                .any(|i| i.issue == AuditIssueKind::WeakPassword)
        );
    }

    #[test]
    fn test_audit_reused_password() {
        let mut v = Vault::for_test("V", "pw");
        v.add_entry(
            EntryData::Login(LoginData::new("site1", "u1", "same_pass")),
            100,
        );
        v.add_entry(
            EntryData::Login(LoginData::new("site2", "u2", "same_pass")),
            100,
        );
        let issues = audit_vault(&v, 200);
        let reused: Vec<_> = issues
            .iter()
            .filter(|i| i.issue == AuditIssueKind::ReusedPassword)
            .collect();
        assert_eq!(reused.len(), 2);
    }

    #[test]
    fn test_audit_old_password() {
        let mut v = Vault::for_test("V", "pw");
        // Entry created 91 days ago
        v.add_entry(
            EntryData::Login(LoginData::new("site", "user", "securepassword123")),
            100,
        );
        let now = 100 + 91 * 86400;
        let issues = audit_vault(&v, now);
        assert!(
            issues
                .iter()
                .any(|i| i.issue == AuditIssueKind::OldPassword)
        );
    }

    #[test]
    fn test_audit_no_totp() {
        let mut v = Vault::for_test("V", "pw");
        v.add_entry(
            EntryData::Login(LoginData::new("site", "user", "longpassword99")),
            100,
        );
        let issues = audit_vault(&v, 200);
        assert!(issues.iter().any(|i| i.issue == AuditIssueKind::NoTotp));
    }

    #[test]
    fn test_audit_compromised() {
        let mut v = Vault::for_test("V", "pw");
        let id = v.add_entry(
            EntryData::Login(LoginData::new("site", "u", "longpass123!")),
            100,
        );
        v.set_compromised(id, true);
        let issues = audit_vault(&v, 200);
        assert!(
            issues
                .iter()
                .any(|i| i.issue == AuditIssueKind::Compromised)
        );
    }

    #[test]
    fn test_audit_common_pattern() {
        let mut v = Vault::for_test("V", "pw");
        v.add_entry(
            EntryData::Login(LoginData::new("site", "user", "password123")),
            100,
        );
        let issues = audit_vault(&v, 200);
        assert!(
            issues
                .iter()
                .any(|i| i.issue == AuditIssueKind::CommonPattern)
        );
    }

    #[test]
    fn test_audit_clean() {
        let mut v = Vault::for_test("V", "pw");
        let mut login = LoginData::new("site", "user", "Xk9!mLn2#pQr$tUv");
        login.totp_secret = Some("JBSWY3DPEHPK3PXP".to_string());
        v.add_entry(EntryData::Login(login), 100);
        let issues = audit_vault(&v, 200);
        // Should have no weak/common/reused/old issues, only possibly no-totp is cleared
        let critical: Vec<_> = issues
            .iter()
            .filter(|i| {
                matches!(
                    i.issue,
                    AuditIssueKind::WeakPassword
                        | AuditIssueKind::ReusedPassword
                        | AuditIssueKind::CommonPattern
                        | AuditIssueKind::Compromised
                )
            })
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
        let v = Vault::for_test("V", "pw");
        let csv = export_csv(&v);
        assert!(csv.starts_with("type,name,username,password,url,notes,tags,folder,starred\n"));
    }

    #[test]
    fn test_export_csv_entry() {
        let mut v = Vault::for_test("V", "pw");
        v.add_entry(
            EntryData::Login(LoginData::new("site", "user", "pass")),
            100,
        );
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
    fn a_carriage_return_forces_a_quoted_csv_field() {
        // RFC 4180 records are CRLF-terminated, so a bare CR in an unquoted
        // field splits the record for most readers.
        assert_eq!(escape_csv("a\rb"), "\"a\rb\"");
    }

    #[test]
    fn a_hostile_entry_name_cannot_forge_a_backup_field() {
        let mut v = Vault::for_test("My \"Vault\"", "pw");
        v.add_entry(
            EntryData::Login(LoginData::new(
                "svc\",\n      \"starred\": true,\n      \"x\": \"",
                "u",
                "p",
            )),
            100,
        );
        v.add_folder("Home\\Work");
        let backup = serialize_backup(&v);
        // The forged key must not appear as a second `starred` field.
        assert_eq!(
            backup.matches("\"starred\":").count(),
            1,
            "entry name forged a key: {backup}"
        );
        // Quotes in the vault name and backslashes in a folder name survive
        // as data rather than terminating their strings.
        assert!(
            backup.contains("\"vault_name\": \"My \\\"Vault\\\"\""),
            "vault name not escaped: {backup}"
        );
        assert!(
            backup.contains("\"name\": \"Home\\\\Work\""),
            "folder name not escaped: {backup}"
        );
    }

    #[test]
    fn a_hostile_tag_cannot_forge_a_backup_tag() {
        let mut v = Vault::for_test("V", "pw");
        v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 100);
        if let Some(e) = v.entries.first_mut() {
            e.tags.push("a\", \"injected".to_string());
        }
        let backup = serialize_backup(&v);
        let tags_line = backup
            .lines()
            .find(|l| l.contains("\"tags\":"))
            .expect("tags line");
        // One tag in, one tag out: the comma inside it must stay inert.
        assert_eq!(
            tags_line.matches("\", \"").count(),
            0,
            "tag forged a second array element: {tags_line}"
        );
    }

    #[test]
    fn test_serialize_backup() {
        let mut v = Vault::for_test("Test", "pw");
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
        let state = AppState::for_test();
        assert_eq!(state.sidebar_selection, SidebarSelection::AllItems);
        assert_eq!(state.detail_view, DetailView::EntryDetail);
        assert!(state.search_query.is_empty());
        assert_eq!(state.sort_order, SortOrder::NameAsc);
        assert!(!state.vault.is_unlocked());
    }

    #[test]
    fn test_app_state_refresh_filter() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state.vault.add_entry(
            EntryData::Login(LoginData::new("GitHub", "user", "pass")),
            state.now,
        );
        state.vault.add_entry(
            EntryData::Login(LoginData::new("GitLab", "user", "pass")),
            state.now,
        );
        state.refresh_filter();
        assert_eq!(state.filtered_ids.len(), 2);

        state.search_query = "hub".to_string();
        state.refresh_filter();
        assert_eq!(state.filtered_ids.len(), 1);
    }

    #[test]
    fn test_app_state_tick() {
        let mut state = AppState::for_test();
        let old_now = state.now;
        state.tick(5000);
        assert!(state.now > old_now);
    }

    #[test]
    fn test_app_state_run_audit() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state
            .vault
            .add_entry(EntryData::Login(LoginData::new("s", "u", "123")), state.now);
        state.run_audit();
        assert!(!state.audit_issues.is_empty());
    }

    // == Wheel scrolling ======================================================

    /// An unlocked vault holding `n` logins, with the list rebuilt.
    fn unlocked_with_entries(n: usize) -> AppState {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        for i in 0..n {
            state.vault.add_entry(
                EntryData::Login(LoginData::new(
                    &format!("site{i}"),
                    &format!("user{i}"),
                    "Correct-Horse-Battery-9!",
                )),
                state.now,
            );
        }
        state.refresh_filter();
        state
    }

    /// A point inside the entry list column.
    const LIST_X: f32 = SIDEBAR_WIDTH + 10.0;
    /// A point inside the detail panel.
    const DETAIL_X: f32 = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH + 10.0;

    fn wheel_at(state: &mut AppState, x: f32, dy: f32) {
        handle_event(
            state,
            &Event::Mouse(MouseEvent {
                x,
                y: TOOLBAR_HEIGHT + 100.0,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            }),
        );
    }

    #[test]
    fn one_wheel_notch_crosses_three_rows_of_the_entry_list() {
        // Not the flat `20.0` px this handler used to add, which on a 52 px
        // row was under half of one -- three detents did not clear two rows.
        let mut state = unlocked_with_entries(60);
        wheel_at(&mut state, LIST_X, -1.0);
        assert_eq!(state.list_scroll, 3.0 * ROW_HEIGHT);
        wheel_at(&mut state, LIST_X, 1.0);
        assert_eq!(state.list_scroll, 0.0);
    }

    #[test]
    fn a_fraction_of_a_notch_moves_now_rather_than_being_banked() {
        // The offset is in pixels, so there is nothing an accumulator could
        // buy: it would only sit on movement the list can already show.
        let mut state = unlocked_with_entries(60);
        wheel_at(&mut state, LIST_X, -0.1);
        assert_eq!(state.list_scroll, 0.3 * ROW_HEIGHT);
    }

    #[test]
    fn the_entry_list_stops_with_its_last_row_on_the_bottom_edge() {
        // The bug this pins: the bound was `.max(0.0)` and nothing else, so
        // the list wound off the end of its content into blank space and kept
        // going for as long as the wheel was turned.
        let mut state = unlocked_with_entries(60);
        for _ in 0..200 {
            wheel_at(&mut state, LIST_X, -1.0);
        }
        let content = 60.0 * ROW_HEIGHT;
        assert_eq!(state.list_scroll, content - state.rows_height());
        assert!(state.list_scroll > 0.0, "the fixture must be scrollable");
        // The last row's bottom edge lands on the pane's bottom edge, which is
        // what "scrolled to the end" is supposed to mean.
        let last_row_bottom = AppState::rows_top() + content - state.list_scroll;
        assert_eq!(last_row_bottom, AppState::rows_top() + state.rows_height());
    }

    #[test]
    fn a_list_shorter_than_the_pane_does_not_scroll_at_all() {
        let mut state = unlocked_with_entries(2);
        wheel_at(&mut state, LIST_X, -10.0);
        assert_eq!(state.list_scroll, 0.0);
    }

    #[test]
    fn a_nonfinite_delta_does_not_freeze_either_pane() {
        // An infinity added to an offset clamps to the far end and never comes
        // back; `wheel::pixels` turns one into no movement at all.
        let mut state = unlocked_with_entries(60);
        wheel_at(&mut state, LIST_X, f32::NAN);
        wheel_at(&mut state, DETAIL_X, f32::INFINITY);
        assert_eq!(state.list_scroll, 0.0);
        assert_eq!(state.detail_scroll, 0.0);
        wheel_at(&mut state, LIST_X, -1.0);
        assert_eq!(state.list_scroll, 3.0 * ROW_HEIGHT);
    }

    #[test]
    fn the_detail_panel_cannot_be_scrolled_before_it_has_been_measured() {
        // Its length depends on the entry's fields, so until a render has
        // walked the layout there is no bound and the honest answer is zero.
        let mut state = unlocked_with_entries(60);
        state.selected_entry_id = state.filtered_ids.first().copied();
        wheel_at(&mut state, DETAIL_X, -5.0);
        assert_eq!(state.detail_scroll, 0.0);
    }

    #[test]
    fn a_detail_panel_taller_than_its_pane_scrolls_to_the_measured_end() {
        let mut state = unlocked_with_entries(60);
        state.selected_entry_id = state.filtered_ids.first().copied();
        // A short window, so the login panel is certainly longer than it.
        handle_event(
            &mut state,
            &Event::Resize {
                width: 1280,
                height: 200,
            },
        );
        let _ = build_render_tree(&mut state);
        let max = state.max_detail_scroll();
        assert!(max > 0.0, "the panel must overflow a 200 px window");
        for _ in 0..200 {
            wheel_at(&mut state, DETAIL_X, -1.0);
        }
        assert_eq!(state.detail_scroll, max);
        for _ in 0..200 {
            wheel_at(&mut state, DETAIL_X, 1.0);
        }
        assert_eq!(state.detail_scroll, 0.0);
    }

    #[test]
    fn the_generator_settings_and_audit_panels_do_not_scroll() {
        // They are fixed-height panels; measuring them as zero content is what
        // keeps the wheel from winding them off the screen.
        for view in [
            DetailView::PasswordGenerator,
            DetailView::Settings,
            DetailView::AuditReport,
        ] {
            let mut state = unlocked_with_entries(60);
            state.detail_view = view;
            let _ = build_render_tree(&mut state);
            wheel_at(&mut state, DETAIL_X, -5.0);
            assert_eq!(state.detail_scroll, 0.0, "{view:?} should not scroll");
        }
    }

    #[test]
    fn shrinking_the_window_pulls_a_scrolled_list_back_inside_it() {
        // Before `Event::Resize` was handled the state did not know the window
        // had changed at all, so an offset taken at one size stayed valid at
        // every other.
        let mut state = unlocked_with_entries(60);
        for _ in 0..200 {
            wheel_at(&mut state, LIST_X, -1.0);
        }
        let tall = state.list_scroll;
        handle_event(
            &mut state,
            &Event::Resize {
                width: 1280,
                height: 2000,
            },
        );
        assert!(
            state.list_scroll < tall,
            "a taller window shows more, so the offset must come back"
        );
        assert_eq!(state.list_scroll, state.max_list_scroll());
    }

    #[test]
    fn selecting_a_different_entry_starts_its_panel_at_the_top() {
        let mut state = unlocked_with_entries(60);
        state.selected_entry_id = state.filtered_ids.first().copied();
        handle_event(
            &mut state,
            &Event::Resize {
                width: 1280,
                height: 200,
            },
        );
        let _ = build_render_tree(&mut state);
        wheel_at(&mut state, DETAIL_X, -1.0);
        assert!(state.detail_scroll > 0.0);
        handle_list_click(&mut state, TOOLBAR_HEIGHT + LIST_HEADER_HEIGHT + 1.0);
        assert_eq!(state.detail_scroll, 0.0);
    }

    #[test]
    fn the_hit_test_and_the_renderer_agree_on_where_row_zero_starts() {
        // Both used a bare `32.0`; naming it is what stops them drifting.
        let mut state = unlocked_with_entries(60);
        let first = state.filtered_ids.first().copied();
        handle_list_click(&mut state, TOOLBAR_HEIGHT + LIST_HEADER_HEIGHT + 1.0);
        assert_eq!(state.selected_entry_id, first);
        // One row further down, after scrolling by exactly one row, is row 1.
        state.list_scroll = ROW_HEIGHT;
        handle_list_click(&mut state, TOOLBAR_HEIGHT + LIST_HEADER_HEIGHT + 1.0);
        assert_eq!(state.selected_entry_id, state.filtered_ids.get(1).copied());
    }

    // == The entry list's edges ================================================

    /// The rectangle the renderer actually clipped the entry rows to.
    ///
    /// Read out of the emitted commands rather than recomputed from the
    /// constants. Recomputing is what makes a layout test worthless: it
    /// re-derives the renderer's arithmetic and then checks the hit test
    /// against *that*, so the two can drift together and the test still
    /// passes. This asks the renderer what it drew.
    fn rows_clip(state: &mut AppState) -> (f32, f32) {
        build_render_tree(state)
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if *x == SIDEBAR_WIDTH && *width == ENTRY_LIST_WIDTH => Some((*y, *height)),
                _ => None,
            })
            .expect("the entry rows are drawn under a clip")
    }

    /// A left click in the entry-list column at `my`, through the same
    /// `handle_event` a real pointer would arrive by.
    fn click_list(state: &mut AppState, my: f32) {
        state.selected_entry_id = None;
        handle_event(
            state,
            &Event::Mouse(MouseEvent {
                x: LIST_X,
                y: my,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
    }

    #[test]
    // Exact equality is the assertion, not an approximation of it: the
    // renderer passes these two helpers' return values straight into the
    // clip, so anything short of bit-for-bit identity means a third copy of
    // the arithmetic has appeared -- which is the bug being pinned.
    #[allow(clippy::float_cmp)]
    fn the_lists_clip_is_the_region_the_hit_test_accepts() {
        let mut state = unlocked_with_entries(60);
        let (clip_y, clip_h) = rows_clip(&mut state);
        assert_eq!(clip_y, AppState::rows_top());
        assert_eq!(clip_h, state.rows_height());

        click_list(&mut state, clip_y);
        assert!(
            state.selected_entry_id.is_some(),
            "the clip's top edge is dead"
        );
        click_list(&mut state, clip_y + clip_h - 0.5);
        assert!(
            state.selected_entry_id.is_some(),
            "the clip's bottom edge is dead"
        );
        click_list(&mut state, clip_y + clip_h);
        assert_eq!(
            state.selected_entry_id, None,
            "the hit test runs past the clip the rows are painted in"
        );
    }

    #[test]
    fn the_entry_count_caption_does_not_open_the_first_credential() {
        // The bug: `handle_list_click` had no guard, so a click in the 32px
        // header strip produced a negative offset -- and a negative `f32` cast
        // to `usize` saturates to zero rather than wrapping. Clicking the
        // caption therefore selected, decrypted and displayed entry zero.
        let mut state = unlocked_with_entries(60);
        for my in [
            TOOLBAR_HEIGHT,
            TOOLBAR_HEIGHT + LIST_HEADER_HEIGHT / 2.0,
            AppState::rows_top() - 0.5,
        ] {
            click_list(&mut state, my);
            assert_eq!(
                state.selected_entry_id, None,
                "the caption selected at {my}"
            );
        }
    }

    #[test]
    fn a_scrolled_caption_click_does_not_open_some_other_credential() {
        // Worse than selecting entry zero: with the list scrolled, the offset
        // was added *before* the cast could saturate, so a caption click
        // resolved to a real -- and arbitrary -- entry.
        let mut state = unlocked_with_entries(60);
        state.list_scroll = 10.0 * ROW_HEIGHT;
        click_list(&mut state, TOOLBAR_HEIGHT + LIST_HEADER_HEIGHT / 2.0);
        assert_eq!(state.selected_entry_id, None);
    }

    #[test]
    fn every_row_edge_selects_the_row_the_renderer_drew_there() {
        let mut state = unlocked_with_entries(60);
        let (clip_y, clip_h) = rows_clip(&mut state);
        let mut slot = 0usize;
        loop {
            let top = clip_y + slot as f32 * ROW_HEIGHT;
            if top >= clip_y + clip_h {
                break;
            }
            let bottom = (top + ROW_HEIGHT - 0.5).min(clip_y + clip_h - 0.5);
            for probe in [top, bottom] {
                click_list(&mut state, probe);
                assert_eq!(
                    state.selected_entry_id,
                    state.filtered_ids.get(slot).copied(),
                    "slot {slot} at y={probe}"
                );
            }
            slot += 1;
        }
        assert!(slot > 1, "the pane must fit more than one row");
    }

    #[test]
    fn empty_space_below_a_short_list_selects_nothing() {
        // Inside the row area but past the end of the list -- a different
        // rejection from the caption one.
        let mut state = unlocked_with_entries(2);
        let (clip_y, clip_h) = rows_clip(&mut state);
        let below_last = clip_y + 2.0 * ROW_HEIGHT + 1.0;
        assert!(below_last < clip_y + clip_h, "the pane must fit >2 rows");
        click_list(&mut state, below_last);
        assert_eq!(state.selected_entry_id, None);
    }

    #[test]
    // `max(0.0)` returns a literal zero, so the comparison is exact.
    #[allow(clippy::float_cmp)]
    fn a_window_shorter_than_its_own_chrome_has_no_rows() {
        let mut state = unlocked_with_entries(60);
        state.height = 10.0;
        assert_eq!(state.rows_height(), 0.0);
        click_list(&mut state, 5.0);
        assert_eq!(state.selected_entry_id, None);
    }

    #[test]
    fn a_nonfinite_coordinate_selects_nothing() {
        let mut state = unlocked_with_entries(60);
        for y in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            click_list(&mut state, y);
            assert_eq!(state.selected_entry_id, None, "selected on {y}");
        }
    }

    // == Render tests ==========================================================

    #[test]
    fn test_render_lock_screen() {
        let mut state = AppState::for_test();
        state.width = 1024.0;
        state.height = 768.0;
        let rt = build_render_tree(&mut state);
        assert!(!rt.commands.is_empty());
        // Lock screen should have FillRect for background
        let has_fill = rt
            .commands
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { .. }));
        assert!(has_fill);
    }

    #[test]
    fn test_render_unlocked_main_ui() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state.vault.add_entry(
            EntryData::Login(LoginData::new("GitHub", "alice", "pass123")),
            state.now,
        );
        state.refresh_filter();
        state.selected_entry_id = state.filtered_ids.first().copied();
        let rt = build_render_tree(&mut state);
        assert!(rt.commands.len() > 30);
    }

    #[test]
    fn test_render_generator_panel() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state.detail_view = DetailView::PasswordGenerator;
        state.generated_password = "test-password-123".to_string();
        let rt = build_render_tree(&mut state);
        assert!(rt.commands.len() > 20);
    }

    #[test]
    fn test_render_settings_panel() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state.detail_view = DetailView::Settings;
        let rt = build_render_tree(&mut state);
        assert!(rt.commands.len() > 20);
    }

    #[test]
    fn test_render_audit_panel_empty() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state.detail_view = DetailView::AuditReport;
        let rt = build_render_tree(&mut state);
        assert!(!rt.commands.is_empty());
    }

    #[test]
    fn test_render_audit_panel_with_issues() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state
            .vault
            .add_entry(EntryData::Login(LoginData::new("s", "u", "123")), state.now);
        state.run_audit();
        state.detail_view = DetailView::AuditReport;
        let rt = build_render_tree(&mut state);
        assert!(rt.commands.len() > 20);
    }

    #[test]
    fn test_render_entry_detail_all_types() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);

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
            let rt = build_render_tree(&mut state);
            assert!(rt.commands.len() > 20, "Render failed for entry type");
        }
    }

    #[test]
    fn test_render_no_selected_entry() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state.selected_entry_id = None;
        let rt = build_render_tree(&mut state);
        assert!(!rt.commands.is_empty());
    }

    // == Event handling tests ==================================================

    #[test]
    fn test_handle_tick_event() {
        let mut state = AppState::for_test();
        let old = state.now;
        handle_event(&mut state, &Event::Tick { elapsed_ms: 2000 });
        assert!(state.now > old);
    }

    #[test]
    fn test_navigate_entry_list_down() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state
            .vault
            .add_entry(EntryData::Login(LoginData::new("A", "u", "p")), state.now);
        state
            .vault
            .add_entry(EntryData::Login(LoginData::new("B", "u", "p")), state.now);
        state.refresh_filter();
        navigate_entry_list(&mut state, 1);
        assert!(state.selected_entry_id.is_some());
    }

    #[test]
    fn test_navigate_entry_list_empty() {
        let mut state = AppState::for_test();
        navigate_entry_list(&mut state, 1);
        assert!(state.selected_entry_id.is_none());
    }

    #[test]
    fn test_navigate_entry_list_clamp() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        let id = state
            .vault
            .add_entry(EntryData::Login(LoginData::new("A", "u", "p")), state.now);
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
            assert_eq!(
                *word,
                word.to_ascii_lowercase(),
                "Word not lowercase: {}",
                word
            );
        }
    }

    // == Text measurement ======================================================

    /// A badge's label has to fit inside it with its 6 px of padding on each
    /// side, in the bold weight the label is actually drawn in.
    #[test]
    fn badge_labels_fit_their_badges() {
        for label in [
            "Login", "Note", "Card", "Identity", "SSH Key", "Weak", "Reused",
        ] {
            let w = badge_width(label);
            let drawn = text::measure(label, SMALL_FONT_SIZE, FontWeightHint::Bold);
            assert!(drawn + 12.0 <= w + 0.01, "{label:?} overflows its badge");
            assert!(w > 12.0, "{label:?} produced an empty badge");
        }
    }

    /// Everything laid out beside a badge sizes it with the same function the
    /// badge itself uses. Two separate estimates had already drifted apart:
    /// the tag strip paced its wrap at `len * 7.5 + 16` while the badge it was
    /// pacing was drawn `len * 7.0 + 12` wide, so a row of tags wrapped one
    /// tag early and left a gap on the right.
    #[test]
    fn a_badge_is_measured_the_same_way_wherever_it_is_measured() {
        let mut rt = RenderTree::new();
        for label in ["Login", "Identity", "Compromised"] {
            let drawn = draw_badge(&mut rt, 0.0, 0.0, label, BLUE, BASE);
            assert!(
                (drawn - badge_width(label)).abs() < f32::EPSILON,
                "{label:?}: drawn {drawn} but laid out {}",
                badge_width(label)
            );
        }
    }

    /// A badge is measured in characters, not UTF-8 bytes.
    #[test]
    fn badge_width_is_not_driven_by_byte_length() {
        // Six characters, nine bytes. A byte-count estimate would make this
        // half again as wide as the six-character ASCII label beside it.
        let accented = badge_width("Résumé");
        let ascii = badge_width("Resume");
        assert!(
            (accented - ascii).abs() < ascii * 0.25,
            "an accented label ({accented}) should measure close to its \
             unaccented twin ({ascii}), not to its byte count"
        );
    }

    /// A button label is centred by measuring it, so the offset does not carry
    /// half of a guessed width's error — which is what made the longest label
    /// on a toolbar the one that visibly sat off-centre.
    #[test]
    fn button_labels_are_centred_on_their_buttons() {
        for label in ["Random", "Pronounceable", "Passphrase", "Unlock"] {
            let (x, w) = (100.0, button_width(label, 10.0));
            let tx = text::center_x(
                label,
                x + w / 2.0,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
            );
            let left = tx - x;
            let right =
                (x + w) - (tx + text::measure(label, DEFAULT_FONT_SIZE, FontWeightHint::Regular));
            assert!(
                (left - right).abs() < 0.01,
                "{label:?} sits off-centre by {}",
                left - right
            );
            assert!(left >= 0.0, "{label:?} overflows its button");
        }
    }
}

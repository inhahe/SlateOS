//! A SlateOS password vault: its entries, and the sealed file they are kept
//! in -- the vault the password manager (`apps/credmanager`) keeps, as a
//! library both it and the credential service (`gui/credentials`, lane C)
//! build on (`requests/c-e-share-the-password-vault-with-the-credential-service.md`).
//!
//! One file, `<config>/credmanager/vault`, sealed under a key made from the
//! master password with the vetted primitives `rustcrypto/seal` wraps
//! (design-decisions §539, §1218): Argon2id makes the key and
//! XChaCha20-Poly1305 seals the entries. The layout is [`vaultfile`]'s.
//!
//! **Read with the code that writes it.** The service reads the vault the
//! password manager saves; with one copy of the format, a change to it is
//! one change, and neither can read a file the other wrote differently.
//!
//! **Written whole.** The password manager replaces the file whole -- a new
//! file renamed over the old (`safeio::write_atomically`) -- so a reader in
//! another process never sees half a save.

use std::collections::HashSet;

pub mod vaultfile;

/// How long an unlocked vault may sit unused before it locks itself, unless
/// it says otherwise.
pub const DEFAULT_AUTO_LOCK_MINUTES: u32 = 15;

// =============================================================================
// Entry types
// =============================================================================

/// The type of credential entry stored in the vault.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryType {
    Login,
    SecureNote,
    CreditCard,
    Identity,
    SshKey,
}

impl EntryType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Login => "Login",
            Self::SecureNote => "Secure Note",
            Self::CreditCard => "Credit Card",
            Self::Identity => "Identity",
            Self::SshKey => "SSH Key",
        }
    }

    pub fn all() -> &'static [EntryType] {
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
#[derive(Clone)]
pub struct LoginData {
    pub site: String,
    pub username: String,
    pub password: String,
    pub url: String,
    pub notes: String,
    pub totp_secret: Option<String>,
}

impl LoginData {
    pub fn new(site: &str, username: &str, password: &str) -> Self {
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
#[derive(Clone)]
pub struct SecureNoteData {
    pub title: String,
    pub content: String,
}

impl SecureNoteData {
    pub fn new(title: &str, content: &str) -> Self {
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
#[derive(Clone)]
pub struct CreditCardData {
    pub name: String,
    pub number_masked: String,
    pub expiry: String,
    pub cardholder: String,
    pub notes: String,
}

impl CreditCardData {
    pub fn new(name: &str, number_masked: &str, expiry: &str, cardholder: &str) -> Self {
        Self {
            name: name.to_string(),
            number_masked: number_masked.to_string(),
            expiry: expiry.to_string(),
            cardholder: cardholder.to_string(),
            notes: String::new(),
        }
    }

    /// Mask a card number, showing only last 4 digits.
    pub fn mask_number(full_number: &str) -> String {
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
#[derive(Clone)]
pub struct IdentityData {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub address: String,
}

impl IdentityData {
    pub fn new(name: &str, email: &str) -> Self {
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
#[derive(Clone)]
pub struct SshKeyData {
    pub name: String,
    pub fingerprint: String,
    pub public_key: String,
}

impl SshKeyData {
    pub fn new(name: &str, fingerprint: &str, public_key: &str) -> Self {
        Self {
            name: name.to_string(),
            fingerprint: fingerprint.to_string(),
            public_key: public_key.to_string(),
        }
    }
}

// =============================================================================
// What `{:?}` shows
// =============================================================================
//
// An entry's name, and nothing it guards: `{:?}` is how a value reaches a log,
// and a password, a note, a card's details or a person's address must not.

impl std::fmt::Debug for LoginData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginData")
            .field("site", &self.site)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for SecureNoteData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecureNoteData")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for CreditCardData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreditCardData")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for IdentityData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityData")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for SshKeyData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshKeyData")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Overwrite `bytes` with zeros, in a way the optimiser may not remove.
///
/// `write_volatile` in a loop rather than `fill`: the optimiser is entitled
/// to delete a store to memory that is never read again, and memory about to
/// be let go is exactly that.
fn wipe(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        // SAFETY: `byte` is a valid, aligned, exclusively borrowed `u8` for
        // the duration of the iteration; a volatile write of a `u8` to it is
        // in bounds and initialises it.
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

// =============================================================================
// Credential entry
// =============================================================================

/// The payload of an entry, varying by type.
#[derive(Clone, Debug)]
pub enum EntryData {
    Login(LoginData),
    SecureNote(SecureNoteData),
    CreditCard(CreditCardData),
    Identity(IdentityData),
    SshKey(SshKeyData),
}

impl EntryData {
    pub fn entry_type(&self) -> EntryType {
        match self {
            Self::Login(_) => EntryType::Login,
            Self::SecureNote(_) => EntryType::SecureNote,
            Self::CreditCard(_) => EntryType::CreditCard,
            Self::Identity(_) => EntryType::Identity,
            Self::SshKey(_) => EntryType::SshKey,
        }
    }

    /// Display name for the entry.
    pub fn display_name(&self) -> &str {
        match self {
            Self::Login(d) => &d.site,
            Self::SecureNote(d) => &d.title,
            Self::CreditCard(d) => &d.name,
            Self::Identity(d) => &d.name,
            Self::SshKey(d) => &d.name,
        }
    }

    /// Subtitle line (username, masked number, email, fingerprint).
    pub fn subtitle(&self) -> &str {
        match self {
            Self::Login(d) => &d.username,
            Self::SecureNote(_) => "",
            Self::CreditCard(d) => &d.number_masked,
            Self::Identity(d) => &d.email,
            Self::SshKey(d) => &d.fingerprint,
        }
    }

    /// Check if text matches a search query (case-insensitive).
    pub fn matches_search(&self, query: &str) -> bool {
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
    pub fn password(&self) -> Option<&str> {
        match self {
            Self::Login(d) => Some(&d.password),
            _ => None,
        }
    }
}

/// A single credential entry in the vault.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub data: EntryData,
    pub folder_id: Option<u64>,
    pub tags: Vec<String>,
    pub starred: bool,
    pub created_at: u64,
    pub modified_at: u64,
    /// Whether this password was flagged as compromised.
    pub compromised: bool,
}

impl Entry {
    pub fn new(id: u64, data: EntryData, timestamp: u64) -> Self {
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

    pub fn entry_type(&self) -> EntryType {
        self.data.entry_type()
    }

    pub fn display_name(&self) -> &str {
        self.data.display_name()
    }

    pub fn subtitle(&self) -> &str {
        self.data.subtitle()
    }

    /// Age of the password in days (from `now` timestamp).
    pub fn password_age_days(&self, now: u64) -> u64 {
        now.saturating_sub(self.modified_at) / 86400
    }
}

// =============================================================================
// Folder
// =============================================================================

/// A folder for organizing entries.
#[derive(Clone, Debug)]
/// A folder in the sidebar.
///
/// `SidebarSelection::Folder` can filter by one and nothing can create one, so
/// no folder is ever constructed. See `todo.txt`.
#[allow(dead_code, reason = "no control creates a folder yet -- see todo.txt")]
pub struct Folder {
    pub id: u64,
    pub name: String,
    pub parent_id: Option<u64>,
}

impl Folder {
    pub fn new(id: u64, name: &str) -> Self {
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

/// How much work makes the key of a new vault: RFC 9106's second
/// recommendation -- 64 MiB, three passes, four lanes -- through `seal`.
///
/// Every vault records its own parameters in its file's header, so raising
/// this later makes new vaults costlier without making an old one unopenable.
pub const NEW_VAULT_KDF: seal::KdfParams = seal::KdfParams::RECOMMENDED;

/// The shortest master password a new vault accepts.
///
/// Length is not strength, and the strength meter says more; but a master
/// password shorter than this is guessed quickly whatever the key derivation
/// costs, and it is the one password that guards all the others.
pub const MIN_MASTER_PASSWORD_CHARS: usize = 10;

/// The key derivation of the vaults tests build: the cheapest Argon2id allows.
///
/// The properties under test -- the right password opens, a wrong one does
/// not, the salt is honoured -- do not depend on the cost, and the real one
/// takes seconds per unlock in a debug build.
#[cfg(any(test, feature = "testing"))]
pub const TEST_KDF: seal::KdfParams = seal::KdfParams {
    memory_kib: 8,
    iterations: 1,
    lanes: 1,
};

/// The master password of the vault built by [`Vault::for_test`] -- and by
/// the password manager's test window, which is built on it.
#[cfg(any(test, feature = "testing"))]
pub const TEST_MASTER_PASSWORD: &str = "master123";

/// Lock state of the vault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultState {
    Locked,
    Unlocked,
}

/// The vault: its entries while it is unlocked, and the sealed file they are
/// kept in.
///
/// **Locked**, it holds [`sealed`](Self::sealed) -- the vault as last
/// written, encrypted -- and what making the key needs: the Argon2id
/// parameters and the salt. No entry and no key is in memory. **Unlocking**
/// makes the key from the master password and opens `sealed` with it; a key
/// that does not open it is the wrong password (or a changed file), and there
/// is no other check, so none that is cheaper to run. **Locking** forgets the
/// key and every entry.
pub struct Vault {
    pub name: String,
    pub state: VaultState,
    /// How the key is made from the master password.
    pub kdf: seal::KdfParams,
    /// What it is made with besides -- drawn at random when the vault is made.
    pub salt: [u8; seal::SALT_LEN],
    /// The key, while unlocked.
    pub key: Option<seal::Key>,
    /// The vault as last sealed: what its file holds, and what unlocking
    /// opens. Empty for a vault that has never been sealed.
    pub sealed: Vec<u8>,
    pub entries: Vec<Entry>,
    pub folders: Vec<Folder>,
    pub last_access: u64,
    pub auto_lock_minutes: u32,
    pub id_gen: u64,
}

/// What a vault is, never what it holds: no key, and no entry's contents.
impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("name", &self.name)
            .field("state", &self.state)
            .field("entries", &self.entries.len())
            .field("folders", &self.folders.len())
            .finish_non_exhaustive()
    }
}

/// The key is overwritten before its memory is let go, however the vault
/// goes: locked, replaced, or dropped while open.
impl Drop for Vault {
    fn drop(&mut self) {
        if let Some(key) = self.key.as_mut() {
            wipe(key);
        }
    }
}

impl Vault {
    /// A new vault, unlocked and empty, whose key `master_password` makes
    /// under `kdf` and `salt`.
    ///
    /// The salt is the caller's to draw, from the system's secure random
    /// source: this runs under test too, where that source is not reachable.
    ///
    /// # Errors
    ///
    /// The key cannot be made: not enough memory, or parameters Argon2 refuses.
    pub fn create(
        name: &str,
        master_password: &str,
        kdf: seal::KdfParams,
        salt: [u8; seal::SALT_LEN],
    ) -> Result<Self, vaultfile::OpenError> {
        let key = vaultfile::key_for(master_password, kdf, &salt)?;
        let mut vault = Self::locked(kdf, salt, Vec::new());
        vault.name = name.to_string();
        vault.key = Some(key);
        vault.state = VaultState::Unlocked;
        Ok(vault)
    }

    /// The vault a file's bytes hold, locked.
    ///
    /// # Errors
    ///
    /// The header does not read: not a vault, a newer format, cut short, or
    /// asking for more work than any vault this program writes.
    pub fn from_file(bytes: Vec<u8>) -> Result<Self, vaultfile::OpenError> {
        let header = vaultfile::Header::parse(&bytes)?;
        Ok(Self::locked(header.kdf, header.salt, bytes))
    }

    /// No vault yet: what the window holds while the first one is being made,
    /// or while the file there cannot be read.
    pub fn none_yet() -> Self {
        Self::locked(NEW_VAULT_KDF, [0; seal::SALT_LEN], Vec::new())
    }

    /// A locked vault around `sealed` -- the one place the fields are set, so
    /// the constructors cannot drift apart in what a vault starts with.
    pub fn locked(kdf: seal::KdfParams, salt: [u8; seal::SALT_LEN], sealed: Vec<u8>) -> Self {
        Self {
            name: String::new(),
            state: VaultState::Locked,
            kdf,
            salt,
            key: None,
            sealed,
            entries: Vec::new(),
            folders: Vec::new(),
            last_access: 0,
            auto_lock_minutes: DEFAULT_AUTO_LOCK_MINUTES,
            id_gen: 1,
        }
    }

    /// A sealed, locked vault with a known master password and the cheapest
    /// key derivation.
    #[cfg(any(test, feature = "testing"))]
    #[allow(
        clippy::expect_used,
        reason = "a test fixture: the cheapest key derivation, sealed once"
    )]
    pub fn for_test(name: &str, master_password: &str) -> Self {
        let mut vault = Self::create(name, master_password, TEST_KDF, [0x5A; seal::SALT_LEN])
            .expect("the test vault's key");
        vault
            .reseal([0x11; seal::NONCE_LEN])
            .expect("the test vault's seal");
        vault.lock();
        vault
    }

    pub fn next_id(&mut self) -> u64 {
        let id = self.id_gen;
        self.id_gen = self.id_gen.saturating_add(1);
        id
    }

    /// Open the vault with `password`.
    ///
    /// Costs a full key derivation -- a large part of a second at
    /// [`NEW_VAULT_KDF`] -- which is the point: it is what every guess costs
    /// an attacker holding the file. Called on submit, never per keystroke.
    ///
    /// # Errors
    ///
    /// The key does not open the vault (the wrong password, or the file has
    /// been changed), or what it holds is not a vault this program reads.
    /// Nothing about the vault changes then.
    pub fn open(&mut self, password: &str, now: u64) -> Result<(), vaultfile::OpenError> {
        let mut key = vaultfile::key_for(password, self.kdf, &self.salt)?;
        let opened = vaultfile::open_file(&self.sealed, &key).and_then(|text| {
            let contents = vaultfile::parse_contents(&text).map_err(vaultfile::OpenError::Contents);
            // The whole vault in the clear, overwritten once it is read.
            let mut bytes = text.into_bytes();
            wipe(&mut bytes);
            contents
        });
        let contents = match opened {
            Ok(contents) => contents,
            Err(why) => {
                wipe(&mut key);
                return Err(why);
            }
        };
        if let Some(old) = self.key.as_mut() {
            wipe(old);
        }
        self.name = contents.name;
        self.auto_lock_minutes = contents.auto_lock_minutes;
        self.id_gen = contents.next_id;
        self.folders = contents.folders;
        self.entries = contents.entries;
        self.key = Some(key);
        // `seal::Key` is an array: storing it copied it, and the copy goes too.
        wipe(&mut key);
        self.state = VaultState::Unlocked;
        self.last_access = now;
        Ok(())
    }

    /// [`open`](Self::open), saying only whether it did -- for tests.
    #[cfg(any(test, feature = "testing"))]
    pub fn unlock(&mut self, password: &str, now: u64) -> bool {
        self.open(password, now).is_ok()
    }

    /// Lock: forget the key and every entry. What they were is in
    /// [`sealed`](Self::sealed), and unlocking brings it back.
    pub fn lock(&mut self) {
        if let Some(key) = self.key.as_mut() {
            // Overwritten before it is let go, so the key is not left for
            // whatever reuses the memory. The entries' strings are freed
            // without being overwritten: `String` does not promise that, and
            // a crate that does is not vendored.
            wipe(key);
        }
        self.key = None;
        self.entries.clear();
        self.folders.clear();
        self.state = VaultState::Locked;
    }

    /// Seal what the vault holds, under `nonce`, into
    /// [`sealed`](Self::sealed).
    ///
    /// `nonce` must be new: sealing twice under one key and nonce gives both
    /// away. The caller draws it from the system's secure random source.
    ///
    /// # Errors
    ///
    /// The vault is locked -- there is no key -- or holds more than one nonce
    /// can seal (256 GiB).
    pub fn reseal(&mut self, nonce: seal::Nonce) -> Result<(), &'static str> {
        let Some(key) = self.key.as_ref() else {
            return Err("the vault is locked");
        };
        let header = vaultfile::Header {
            kdf: self.kdf,
            salt: self.salt,
            nonce,
        };
        let sealed = vaultfile::seal_file(key, &header, &vaultfile::contents_text(self))
            .map_err(|_| "the vault is too large to seal")?;
        self.sealed = sealed;
        Ok(())
    }

    pub fn is_unlocked(&self) -> bool {
        self.state == VaultState::Unlocked
    }

    /// Seconds from `now` until the auto-lock falls due, or `None` while the
    /// vault is locked and there is nothing to lock.
    pub fn auto_lock_in(&self, now: u64) -> Option<u64> {
        if self.state == VaultState::Locked {
            return None;
        }
        let timeout_seconds = u64::from(self.auto_lock_minutes).saturating_mul(60);
        let due = self.last_access.saturating_add(timeout_seconds);
        Some(due.saturating_sub(now))
    }

    /// Check if auto-lock timeout has been exceeded.
    pub fn should_auto_lock(&self, now: u64) -> bool {
        if self.state == VaultState::Locked {
            return false;
        }
        let elapsed_seconds = now.saturating_sub(self.last_access);
        let timeout_seconds = u64::from(self.auto_lock_minutes) * 60;
        elapsed_seconds >= timeout_seconds
    }

    pub fn touch(&mut self, now: u64) {
        self.last_access = now;
    }

    // -- Entry CRUD ---------------------------------------------------------

    pub fn add_entry(&mut self, data: EntryData, now: u64) -> u64 {
        let id = self.next_id();
        self.entries.push(Entry::new(id, data, now));
        self.touch(now);
        id
    }

    /// Delete an entry.
    ///
    /// Reached from the entry's Delete button and the Delete key, after a
    /// question -- see `ask_delete`. It had no caller until 2026-09-27.
    pub fn remove_entry(&mut self, entry_id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != entry_id);
        self.entries.len() < before
    }

    pub fn get_entry(&self, entry_id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == entry_id)
    }

    /// An entry that can be changed. No caller yet, for the same reason as
    /// `remove_entry`: the detail view shows a credential and cannot edit one.
    pub fn get_entry_mut(&mut self, entry_id: u64) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.id == entry_id)
    }

    /// Replace an entry's payload, from the form's Edit. It had no caller
    /// until 2026-09-27.
    pub fn update_entry(&mut self, entry_id: u64, data: EntryData, now: u64) -> bool {
        if let Some(entry) = self.get_entry_mut(entry_id) {
            entry.data = data;
            entry.modified_at = now;
            true
        } else {
            false
        }
    }

    /// Mark an entry a favourite. The sidebar can filter by favourites and
    /// nothing can set one, so the filter is always empty. See `todo.txt`.
    #[allow(dead_code, reason = "no favourite control yet -- see todo.txt")]
    pub fn toggle_star(&mut self, entry_id: u64) -> bool {
        if let Some(entry) = self.get_entry_mut(entry_id) {
            entry.starred = !entry.starred;
            true
        } else {
            false
        }
    }

    /// Flag a credential as known-breached. Nothing calls it: there is no
    /// breach feed to learn it from and no control to set it by hand.
    #[allow(dead_code, reason = "no breach feed and no control -- see todo.txt")]
    pub fn set_compromised(&mut self, entry_id: u64, compromised: bool) -> bool {
        if let Some(entry) = self.get_entry_mut(entry_id) {
            entry.compromised = compromised;
            true
        } else {
            false
        }
    }

    /// Tag an entry. The sidebar lists every tag in the vault and filters by
    /// them, and nothing can put one on an entry, so the list is always empty.
    /// See `todo.txt`.
    #[allow(dead_code, reason = "no tag control yet -- see todo.txt")]
    pub fn add_tag(&mut self, entry_id: u64, tag: &str) -> bool {
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

    #[allow(dead_code, reason = "no tag control yet -- see todo.txt")]
    pub fn remove_tag(&mut self, entry_id: u64, tag: &str) -> bool {
        if let Some(entry) = self.get_entry_mut(entry_id) {
            let before = entry.tags.len();
            entry.tags.retain(|t| t != tag);
            entry.tags.len() < before
        } else {
            false
        }
    }

    /// Move entry `entry_id` into folder `folder_id` (`None`: into no
    /// folder). Whether it did: a folder the vault does not have is refused,
    /// since a vault holding an entry in one could never be opened again.
    #[allow(dead_code, reason = "no folder control yet -- see todo.txt")]
    pub fn set_folder(&mut self, entry_id: u64, folder_id: Option<u64>) -> bool {
        if folder_id.is_some_and(|f| self.get_folder(f).is_none()) {
            return false;
        }
        if let Some(entry) = self.get_entry_mut(entry_id) {
            entry.folder_id = folder_id;
            true
        } else {
            false
        }
    }

    // -- Folder CRUD --------------------------------------------------------

    /// Make a folder. Nothing calls it, which is why the sidebar's folder
    /// section is always empty. See `Folder` and `todo.txt`.
    #[allow(dead_code, reason = "no folder control yet -- see todo.txt")]
    pub fn add_folder(&mut self, name: &str) -> u64 {
        let id = self.next_id();
        self.folders.push(Folder::new(id, name));
        id
    }

    /// Remove folder `folder_id`. What was in it -- its entries and the
    /// folders inside it -- moves up into the folder it was in, so nothing is
    /// left in a folder the vault no longer has: a vault holding anything
    /// there could never be opened again. Whether there was one to remove.
    #[allow(dead_code, reason = "no folder control yet -- see todo.txt")]
    pub fn remove_folder(&mut self, folder_id: u64) -> bool {
        let Some(at) = self.folders.iter().position(|f| f.id == folder_id) else {
            return false;
        };
        let removed = self.folders.remove(at);
        for entry in &mut self.entries {
            if entry.folder_id == Some(folder_id) {
                entry.folder_id = removed.parent_id;
            }
        }
        for folder in &mut self.folders {
            if folder.parent_id == Some(folder_id) {
                folder.parent_id = removed.parent_id;
            }
        }
        true
    }

    pub fn get_folder(&self, folder_id: u64) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == folder_id)
    }

    #[allow(dead_code, reason = "no folder control yet -- see todo.txt")]
    pub fn rename_folder(&mut self, folder_id: u64, new_name: &str) -> bool {
        if let Some(folder) = self.folders.iter_mut().find(|f| f.id == folder_id) {
            folder.name = new_name.to_string();
            true
        } else {
            false
        }
    }

    // -- Query helpers -------------------------------------------------------

    pub fn entries_in_folder(&self, folder_id: Option<u64>) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.folder_id == folder_id)
            .collect()
    }

    pub fn starred_entries(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.starred).collect()
    }

    pub fn entries_with_tag(&self, tag: &str) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.tags.iter().any(|t| t == tag))
            .collect()
    }

    pub fn entries_of_type(&self, entry_type: EntryType) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.entry_type() == entry_type)
            .collect()
    }

    /// Full-text search across a vault. The toolbar's search box filters
    /// through `refresh_filter` instead, so this second search has no caller;
    /// one of the two should go once it is clear which the UI wants.
    #[allow(
        dead_code,
        reason = "the toolbar filters through refresh_filter -- see todo.txt"
    )]
    pub fn search_entries(&self, query: &str) -> Vec<&Entry> {
        if query.is_empty() {
            return self.entries.iter().collect();
        }
        self.entries
            .iter()
            .filter(|e| e.data.matches_search(query))
            .collect()
    }

    /// All unique tags across all entries.
    pub fn all_tags(&self) -> Vec<String> {
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

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    // A test's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production
    // code are off here, as CLAUDE.md prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// A vault the cheapest key derivation opens, unlocked.
    fn unlocked_vault() -> Vault {
        let mut vault = Vault::for_test("Test Vault", TEST_MASTER_PASSWORD);
        assert!(vault.unlock(TEST_MASTER_PASSWORD, 0));
        vault
    }

    #[test]
    fn the_contents_keep_every_character_of_every_field() {
        let mut vault = unlocked_vault();
        let odd = "tab\there\nline\rreturn\\slash \u{e9}\u{1F512} ,\"quoted\"; = + - nul\0";
        let folder = vault.add_folder(odd);
        let mut login = LoginData::new(odd, odd, odd);
        login.url = odd.to_string();
        login.notes = odd.to_string();
        login.totp_secret = Some(odd.to_string());
        let id = vault.add_entry(EntryData::Login(login), 5);
        vault.set_folder(id, Some(folder));
        vault.add_tag(id, odd);
        vault.add_tag(id, "second");
        vault.toggle_star(id);
        vault.add_entry(EntryData::SecureNote(SecureNoteData::new("", "")), 6);
        vault.add_entry(
            EntryData::CreditCard(CreditCardData::new(odd, "****1234", "12/30", odd)),
            7,
        );
        let text = vaultfile::contents_text(&vault);
        let back = vaultfile::parse_contents(&text).unwrap();
        let mut rebuilt = unlocked_vault();
        rebuilt.name = back.name;
        rebuilt.auto_lock_minutes = back.auto_lock_minutes;
        rebuilt.id_gen = back.next_id;
        rebuilt.folders = back.folders;
        rebuilt.entries = back.entries;
        assert_eq!(vaultfile::contents_text(&rebuilt), text);
        assert_eq!(rebuilt.entries.len(), 3);
    }

    #[test]
    fn contents_that_are_not_understood_are_refused_whole() {
        // A vault whose next id is past the entry below, so each bad case is
        // wrong in exactly one way and is refused for that reason alone.
        let mut base = unlocked_vault();
        base.id_gen = 100;
        let good = vaultfile::contents_text(&base);
        assert!(vaultfile::parse_contents(&good).is_ok());
        let entry = "login\t9\t\t0\t0\t1\t1\ts\tu\tp\turl\tnotes";
        assert!(
            vaultfile::parse_contents(&format!("{good}{entry}\n")).is_ok(),
            "control: the entry the bad cases are made from is itself good"
        );
        for bad in [
            String::new(),
            "not a vault".to_string(),
            format!("{good}mystery\tfield\n"),
            format!("{good}{entry}\textra\n"),
            format!("{good}{}\n", entry.replace("\t0\t0\t", "\t2\t0\t")),
            format!("{good}{}\n", entry.replace("\tu\t", "\t\\q\t")),
            format!("{good}{entry}\n{entry}\n"),
            format!("{good}tag\t77\tno such entry\n"),
            format!("{good}{}\n", entry.replace("login\t9\t\t", "login\t9\t4\t")),
            format!("{good}note\t9\t\t0\t0\t1\t1\ttitle\tbody\ntotp\t9\tsecret\n"),
            good.replacen("vault\t", "vault\tx\t", 1),
        ] {
            assert!(
                vaultfile::parse_contents(&bad).is_err(),
                "read as a vault: {bad:?}"
            );
        }
        // An id at or past the next one to hand out would be handed out twice.
        let mut vault = unlocked_vault();
        vault.add_entry(EntryData::SecureNote(SecureNoteData::new("n", "")), 0);
        let text =
            vaultfile::contents_text(&vault).replacen(&format!("\t{}\n", vault.id_gen), "\t1\n", 1);
        assert!(vaultfile::parse_contents(&text).is_err(), "{text}");
    }

    // == Entry id allocation ===================================================
    //
    // These used to drive a free-standing `IdGen`, which was the only thing
    // keeping it alive: the vault allocates from its own `id_gen` field and
    // never touched the other one. Pointed at the allocator that is actually
    // used, the same two properties still hold and now say something about
    // the program.

    #[test]
    fn entry_ids_are_handed_out_in_order() {
        let mut vault = unlocked_vault();
        let a = vault.add_entry(EntryData::SecureNote(SecureNoteData::new("a", "")), 0);
        let b = vault.add_entry(EntryData::SecureNote(SecureNoteData::new("b", "")), 0);
        let c = vault.add_entry(EntryData::SecureNote(SecureNoteData::new("c", "")), 0);
        assert!(a < b && b < c, "ids must increase: {a}, {b}, {c}");
    }

    #[test]
    fn entry_ids_saturate_rather_than_wrap() {
        // Wrapping would hand a new entry the id of an existing one, and the
        // vault looks entries up by id.
        let mut vault = unlocked_vault();
        vault.id_gen = u64::MAX;
        let first = vault.add_entry(EntryData::SecureNote(SecureNoteData::new("a", "")), 0);
        let second = vault.add_entry(EntryData::SecureNote(SecureNoteData::new("b", "")), 0);
        assert_eq!(first, u64::MAX);
        assert_eq!(second, u64::MAX, "saturating, not wrapping to zero");
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
    fn test_entry_type_all() {
        let all = EntryType::all();
        assert_eq!(all.len(), 5);
        assert!(all.contains(&EntryType::Login));
        assert!(all.contains(&EntryType::SshKey));
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
    /// password makes the same key in every vault on every machine, so one
    /// precomputed table opens all of them.
    #[test]
    fn two_vaults_with_one_password_do_not_share_a_key() {
        let a = Vault::create("A", "correct horse", TEST_KDF, [1; seal::SALT_LEN]).unwrap();
        let b = Vault::create("B", "correct horse", TEST_KDF, [2; seal::SALT_LEN]).unwrap();
        assert_ne!(a.key, b.key, "the salt did not reach the key");
    }

    /// The salt is in the file's header, so it cannot be lost apart from the
    /// vault -- and a header whose salt was changed opens nothing.
    #[test]
    fn a_vault_is_worthless_without_the_salt_it_was_made_under() {
        let vault = Vault::for_test("V", "correct horse");
        let mut bytes = vault.sealed.clone();
        let salt_at = 20;
        bytes[salt_at] ^= 1;
        let mut changed = Vault::from_file(bytes).unwrap();
        assert!(!changed.unlock("correct horse", 100));
    }

    #[test]
    fn a_stored_vault_reopens_with_the_password_it_was_created_from() {
        let original = Vault::for_test("V", "correct horse");
        let mut reopened = Vault::from_file(original.sealed.clone()).unwrap();
        assert!(!reopened.unlock("wrong", 100));
        assert!(!reopened.is_unlocked());
        assert!(reopened.unlock("correct horse", 100));
        assert_eq!(
            reopened.name, "V",
            "the name comes out of the sealed contents"
        );
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

    /// **A folder edit leaves a vault that opens**: an entry is not put in a
    /// folder the vault does not have, and removing a folder moves what was
    /// in it -- entries and folders -- up into the folder it was in, where
    /// the file's reader finds them.
    #[test]
    fn a_folder_edit_leaves_a_vault_that_opens() {
        let mut v = unlocked_vault();
        let id = v.add_entry(EntryData::SecureNote(SecureNoteData::new("n", "")), 0);
        assert!(
            !v.set_folder(id, Some(999)),
            "an entry was put in a folder that is not there"
        );
        assert_eq!(v.get_entry(id).unwrap().folder_id, None);

        let outer = v.add_folder("outer");
        let inner = v.add_folder("inner");
        let innermost = v.add_folder("innermost");
        let nest = |v: &mut Vault, child: u64, parent: u64| {
            v.folders
                .iter_mut()
                .find(|f| f.id == child)
                .unwrap()
                .parent_id = Some(parent);
        };
        nest(&mut v, inner, outer);
        nest(&mut v, innermost, inner);
        assert!(v.set_folder(id, Some(inner)));

        assert!(v.remove_folder(inner));
        assert!(!v.remove_folder(inner), "a folder was removed twice");
        assert_eq!(
            v.get_entry(id).unwrap().folder_id,
            Some(outer),
            "the entry was not moved up"
        );
        assert_eq!(
            v.get_folder(innermost).unwrap().parent_id,
            Some(outer),
            "the folder inside was not moved up"
        );

        v.reseal([0x33; seal::NONCE_LEN]).unwrap();
        let mut reopened = Vault::from_file(v.sealed.clone()).unwrap();
        assert!(
            reopened.unlock(TEST_MASTER_PASSWORD, 0),
            "the vault no longer opens"
        );
        assert_eq!(reopened.entries.len(), 1);
        assert_eq!(reopened.folders.len(), 2);
    }

    /// **`{:?}` shows no secret** -- not a vault's key, and nothing an entry
    /// guards -- since `{:?}` is how a value reaches a log. It names each
    /// entry, so a print still says which one.
    #[test]
    fn a_debug_print_shows_no_secret() {
        let mut v = unlocked_vault();
        let mut login = LoginData::new("example.org", "ann", "pw-SECRET-1");
        login.notes = "notes-SECRET-2".to_string();
        login.totp_secret = Some("totp-SECRET-3".to_string());
        v.add_entry(EntryData::Login(login), 0);
        v.add_entry(
            EntryData::SecureNote(SecureNoteData::new("plan", "note-SECRET-4")),
            0,
        );
        let mut card = CreditCardData::new("visa", "****1234", "12/30", "holder-SECRET-5");
        card.notes = "card-SECRET-6".to_string();
        v.add_entry(EntryData::CreditCard(card), 0);
        let mut person = IdentityData::new("me", "mail-SECRET-7@example.org");
        person.address = "addr-SECRET-8".to_string();
        v.add_entry(EntryData::Identity(person), 0);
        v.add_entry(
            EntryData::SshKey(SshKeyData::new("box", "fp", "pub-SECRET-9")),
            0,
        );
        let contents = vaultfile::parse_contents(&vaultfile::contents_text(&v));
        let printed = format!("{v:?} {:?} {contents:?}", v.entries);
        assert!(
            printed.contains("example.org"),
            "no entry is named: {printed}"
        );
        assert!(
            !printed.contains("SECRET"),
            "a secret was printed: {printed}"
        );
        let key = v.key.unwrap();
        assert!(
            !printed.contains(&format!("{key:?}")),
            "the key was printed"
        );
    }

    /// `wipe` overwrites every byte it is given.
    #[test]
    fn wipe_overwrites_every_byte() {
        let mut bytes = [0xA5_u8; 40];
        wipe(&mut bytes);
        assert_eq!(bytes, [0; 40]);
    }

    /// **Locking forgets the key and every entry and folder**, and unlocking
    /// brings back what was locked.
    #[test]
    fn locking_forgets_the_key_and_every_entry() {
        let mut v = unlocked_vault();
        v.add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 0);
        v.add_folder("f");
        v.reseal([0x22; seal::NONCE_LEN]).unwrap();
        v.lock();
        assert!(!v.is_unlocked());
        assert!(v.key.is_none(), "the key outlived the lock");
        assert!(v.entries.is_empty(), "the entries outlived the lock");
        assert!(v.folders.is_empty(), "the folders outlived the lock");
        assert!(v.unlock(TEST_MASTER_PASSWORD, 0));
        assert_eq!(v.entries.len(), 1, "what was locked did not come back");
    }

    #[test]
    fn test_vault_auto_lock() {
        let mut v = Vault::for_test("Test", "pw");
        // Set while open: the timeout is kept inside the sealed vault, so
        // unlocking reads it back from there.
        v.unlock("pw", 100);
        v.auto_lock_minutes = 5;
        assert!(!v.should_auto_lock(100));
        assert!(!v.should_auto_lock(399)); // 299s < 300s
        assert!(v.should_auto_lock(400)); // 300s >= 300s
        assert_eq!(
            v.auto_lock_in(100),
            Some(300),
            "due a timeout after the last use"
        );
        assert_eq!(v.auto_lock_in(399), Some(1));
        assert_eq!(v.auto_lock_in(400), Some(0));
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
}

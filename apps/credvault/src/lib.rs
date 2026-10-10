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
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
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
        let key = vaultfile::key_for(password, self.kdf, &self.salt)?;
        let text = vaultfile::open_file(&self.sealed, &key)?;
        let contents = vaultfile::parse_contents(&text).map_err(vaultfile::OpenError::Contents)?;
        self.name = contents.name;
        self.auto_lock_minutes = contents.auto_lock_minutes;
        self.id_gen = contents.next_id;
        self.folders = contents.folders;
        self.entries = contents.entries;
        self.key = Some(key);
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
            key.fill(0);
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

    /// Move an entry into a folder. Nothing creates a folder, so there is
    /// nowhere to move one to. See `Folder` and `todo.txt`.
    #[allow(dead_code, reason = "no folder control yet -- see todo.txt")]
    pub fn set_folder(&mut self, entry_id: u64, folder_id: Option<u64>) -> bool {
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

    #[allow(dead_code, reason = "no folder control yet -- see todo.txt")]
    pub fn remove_folder(&mut self, folder_id: u64) -> bool {
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
    #![allow(clippy::unwrap_used, clippy::expect_used)]

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
}

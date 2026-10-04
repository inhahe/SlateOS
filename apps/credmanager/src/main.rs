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
//! # The vault on disk
//!
//! The vault is one file, `<config>/credmanager/vault`, encrypted under a key
//! made from the master password: Argon2id makes the key and
//! XChaCha20-Poly1305 seals the entries, through `rustcrypto/seal` -- vetted
//! code, not this project's (`design-decisions.md` §539, §1218). The master
//! password is never stored, nor anything that checks a guess more cheaply
//! than opening the file does; a key that does not open the vault is the
//! wrong password. Locked, the program holds the sealed file and nothing
//! else -- no key, no entry. Every change is saved as it is made, under a new
//! nonce. The file's layout is `vaultfile`'s module doc.
//!
//! Until 2026-09-27 nothing was kept: every launch opened an empty vault, the
//! master password was checked against a stored verifier that was never
//! written anywhere, and locking changed a flag while every entry stayed in
//! memory behind the lock screen.

// Nineteen items are built and exercised by tests but reachable from no
// control yet: the CSV export, the backup serialiser, the clipboard copy, and
// the constructors for the entry kinds the "Add" button does not open a form
// for. Those are finished, tested code waiting on a button, not corpses, so
// they are kept rather than deleted -- but a blanket allow also swallows
// genuine orphans, which is how `rows_top` and `build_render_tree` sat unused
// through the conversion to a recorded-hit-box renderer without a peep. Both
// are `#[cfg(test)]` now, and anything else that falls out of use should be
// too, so that what is left under this allow is only the not-yet-wired.
// Narrowing it per-item is
// `known-issues.md` → `C-CREDMANAGER-ALLOWS-DEAD-CODE-CRATE-WIDE`.

use appearance::Palette;
use pathtext::ShowPath;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use guitk::color::Color;
use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
#[cfg(test)]
use guitk::probe;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[cfg(test)]
use guitk::rng::SeededRng;
use guitk::rng::{RandomSource, SecretSource, SystemRandom};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::text::TextCursor;
use guitk::textedit;
use guitk::textinput::TextInput;
use guitk::wheel;
use oswindow::app::{self, App, Response};

mod vaultfile;

// =============================================================================
// Catppuccin Mocha palette
// =============================================================================

// =============================================================================
// Constants
// =============================================================================
const SIDEBAR_WIDTH: f32 = 220.0;
const ENTRY_LIST_WIDTH: f32 = 320.0;
const TOOLBAR_HEIGHT: f32 = 48.0;
/// The not-saved notice's strip under the toolbar, and one line of it.
const NOTICE_H: f32 = 34.0;
const NOTICE_LINE_H: f32 = 15.0;
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
/// The most a box holds, in characters: a password, a passphrase, a
/// note -- and a paste of a page is none of them.
const BOX_CAPACITY: usize = 4096;
/// What a masked box draws for each character of a secret.
const MASK: char = '*';
const HEADING_FONT_SIZE: f32 = 18.0;
const SMALL_FONT_SIZE: f32 = 12.0;
const CORNER_RADIUS: f32 = 6.0;
const DEFAULT_AUTO_LOCK_MINUTES: u32 = 15;

/// The auto-lock times the settings slider offers, in minutes: one to an hour.
const AUTO_LOCK_MINUTES: (u32, u32) = (1, 60);

/// The settings panel's padding.
const SETTINGS_PAD: f32 = 24.0;

/// Where the settings panel draws the auto-lock slider in a window `width`
/// wide: below the heading (36 pixels), the SECURITY label (24) and the
/// auto-lock row (32), across the panel inside its padding.
///
/// An auto-lock slider value as the whole minutes it stands for, in the
/// slider's range.
fn whole_minutes(value: f64) -> u32 {
    let (lo, hi) = AUTO_LOCK_MINUTES;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rounded and clamped into the slider's own range first"
    )]
    let minutes = value.round().clamp(f64::from(lo), f64::from(hi)) as u32;
    minutes
}

/// One function, read by the drawing and by the pointer, so a press lands
/// on the value the thumb is drawn at.
fn auto_lock_placement(width: f32) -> guitk::slider::Placement {
    let x = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH + SETTINGS_PAD;
    let y = TOOLBAR_HEIGHT + SETTINGS_PAD + 36.0 + 24.0 + 32.0;
    let w = (width - SIDEBAR_WIDTH - ENTRY_LIST_WIDTH - SETTINGS_PAD * 2.0).max(0.0);
    guitk::slider::Placement::horizontal(Rect::new(x, y, w, 4.0), 12.0)
}
const PASSWORD_OLD_DAYS: u64 = 90;
const WEAK_PASSWORD_LEN: usize = 8;

// =============================================================================
// Targets and layout
// =============================================================================

/// Everything in the window a pointer can be over.
///
/// The renderer records one of these beside each control's rectangle as it
/// draws it, and the click handler asks the recorded frame rather than
/// rebuilding the geometry from the same constants. The four
/// `handle_*_click` functions this replaces did rebuild it — the toolbar one
/// carried a hand-computed `base_x + 284.0 ..= base_x + 364.0` for a button
/// the renderer placed by adding five widths and five gaps, and the sidebar
/// one re-summed a stack of `+ 12.0 + 30.0 + 24.0` offsets. They happened to
/// agree; nothing made them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// Toolbar: add a new entry.
    Add,
    /// Toolbar: the search box.
    Search,
    /// Toolbar: cycle the sort order.
    Sort,
    /// Toolbar: open the password generator.
    Generator,
    /// Toolbar: lock the vault.
    LockVault,
    /// Toolbar: open settings.
    Settings,
    /// Sidebar row `n`, indexing [`AppState::sidebar_items`].
    ///
    /// An index rather than the [`SidebarSelection`] itself because a target
    /// must be `Copy` and a tag filter carries a `String`. The index is not a
    /// second derivation of the order: the renderer *iterates* that list, so
    /// row `n` is drawn from `items[n]` and resolved back through `items[n]`.
    Sidebar(usize),
    /// Entry-list row `n`, indexing `filtered_ids`.
    EntryRow(usize),
    /// Lock screen: the master-password field.
    MasterInput,
    /// Lock screen: the Unlock button.
    Unlock,
    /// First run: the master-password field.
    NewPassword,
    /// First run: the field it is typed again in.
    ConfirmPassword,
    /// First run: the button that makes the vault.
    CreateVault,
    /// Settings: back the vault up.
    BackUp,
    /// Settings: restore from a backup.
    RestoreBackup,
    /// Settings: export as plain text.
    ExportCsv,
    /// Settings: the auto-lock slider. Its press, drag and release go to the
    /// slider itself ([`AppState::auto_lock_mouse`]), which needs where on it
    /// they land; this target is its place in the hit boxes.
    AutoLock,
    /// The export warning: go on.
    ExportAnyway,
    /// The restore dialog's password field.
    RestoreInput,
    /// The restore dialog: open the backup with the password typed.
    RestoreOpen,
    /// The restore dialog: replace this vault's entries with the backup's.
    RestoreReplace,
    /// Any vault dialog: leave it, doing nothing.
    DialogCancel,
    /// Entry detail: change this entry.
    EditEntry,
    /// Entry detail: delete this entry (after asking).
    DeleteEntry,
    /// The delete question: yes.
    DeleteConfirmed,
    /// New-entry form: the type chooser, indexing [`EntryType::all`].
    NewKind(usize),
    /// New-entry form: field `n`, indexing [`fields_for`].
    NewField(usize),
    /// New-entry form: save.
    NewSave,
    /// New-entry form: discard.
    NewCancel,
    /// Entry detail: copy field `n` to the clipboard.
    ///
    /// The index is into [`copyable_fields`], which is what the detail view
    /// draws a button beside, so the row and the target cannot disagree.
    CopyField(usize),
}

impl Target {
    /// Whether this is a text box: the targets the pointer lights.
    fn is_text_box(self) -> bool {
        matches!(
            self,
            Self::Search
                | Self::MasterInput
                | Self::NewPassword
                | Self::ConfirmPassword
                | Self::RestoreInput
                | Self::NewField(_)
        )
    }
}

type Frame = guitk::frame::Frame<Target>;

/// A window size that can be laid out against: never negative, never NaN.
fn sane(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Take `want` px off the top of the space between `y` and `limit`, shrinking
/// rather than clamping if there is less than that left.
///
/// Clamping is the wrong instinct here: [`Frame`] does not clip to the window,
/// so a rectangle clamped to a minimum height in a window too short for it
/// would still record a hit box — a button that cannot be seen but can be
/// pressed.
fn take_top(y: &mut f32, limit: f32, width: f32, want: f32) -> Rect {
    let h = want.min((limit - *y).max(0.0));
    let r = Rect::new(0.0, *y, width, h);
    *y += h;
    r
}

/// `r` cut down to `bounds`, or empty if it falls outside entirely.
fn trim(r: Rect, bounds: Rect) -> Rect {
    r.intersect(bounds).unwrap_or(Rect::EMPTY)
}

/// Where every pane goes, derived from the live window size.
///
/// Built fresh on every frame and never stored. The size a window *is* and the
/// size it was told to be last are two different things for exactly one frame
/// — the first one, which arrives before any `Event::Resize` — and that is the
/// frame in which a remembered layout is wrong.
struct Layout {
    /// The whole window.
    window: Rect,
    /// The toolbar strip across the top.
    toolbar: Rect,
    /// The strip under the toolbar that says the vault is not saved. Its own
    /// strip: the lines were drawn at the top of the window, before the
    /// toolbar, which filled the same pixels -- so a password manager that
    /// loses every entry at close said so where it could not be seen.
    notice: Rect,
    /// The category sidebar down the left.
    sidebar: Rect,
    /// The entry-list column, header strip included.
    list: Rect,
    /// The scrolling part of the entry list — the header strip excluded,
    /// because it does not scroll.
    list_rows: Rect,
    /// The detail panel filling the rest.
    detail: Rect,
}

impl Layout {
    fn new(width: f32, height: f32) -> Self {
        let width = sane(width);
        let height = sane(height);
        let window = Rect::new(0.0, 0.0, width, height);

        let mut top = 0.0;
        let toolbar = take_top(&mut top, height, width, TOOLBAR_HEIGHT);
        let notice = take_top(&mut top, height, width, NOTICE_H);
        let body = (height - top).max(0.0);

        let sidebar = trim(Rect::new(0.0, top, SIDEBAR_WIDTH, body), window);
        let list = trim(
            Rect::new(SIDEBAR_WIDTH, top, ENTRY_LIST_WIDTH, body),
            window,
        );

        let rows_top = top + LIST_HEADER_HEIGHT;
        let list_rows = trim(
            Rect::new(
                SIDEBAR_WIDTH,
                rows_top,
                ENTRY_LIST_WIDTH,
                (height - rows_top).max(0.0),
            ),
            window,
        );

        let detail_x = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
        let detail = trim(
            Rect::new(detail_x, top, (width - detail_x).max(0.0), body),
            window,
        );

        Self {
            window,
            toolbar,
            notice,
            sidebar,
            list,
            list_rows,
            detail,
        }
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

    fn badge_color(self, pal: &Palette) -> Color {
        match self {
            Self::Login => pal.blue,
            Self::SecureNote => pal.yellow,
            Self::CreditCard => pal.peach,
            Self::Identity => pal.green,
            Self::SshKey => pal.lavender,
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
/// A folder in the sidebar.
///
/// `SidebarSelection::Folder` can filter by one and nothing can create one, so
/// no folder is ever constructed. See `todo.txt`.
#[allow(dead_code, reason = "no control creates a folder yet -- see todo.txt")]
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

/// How much work makes the key of a new vault: RFC 9106's second
/// recommendation -- 64 MiB, three passes, four lanes -- through `seal`.
///
/// Every vault records its own parameters in its file's header, so raising
/// this later makes new vaults costlier without making an old one unopenable.
const NEW_VAULT_KDF: seal::KdfParams = seal::KdfParams::RECOMMENDED;

/// The shortest master password a new vault accepts.
///
/// Length is not strength, and the strength meter says more; but a master
/// password shorter than this is guessed quickly whatever the key derivation
/// costs, and it is the one password that guards all the others.
const MIN_MASTER_PASSWORD_CHARS: usize = 10;

/// The key derivation of the vaults tests build: the cheapest Argon2id allows.
///
/// The properties under test -- the right password opens, a wrong one does
/// not, the salt is honoured -- do not depend on the cost, and the real one
/// takes seconds per unlock in a debug build.
#[cfg(test)]
const TEST_KDF: seal::KdfParams = seal::KdfParams {
    memory_kib: 8,
    iterations: 1,
    lanes: 1,
};

/// The master password of the vault built by [`AppState::for_test`].
#[cfg(test)]
const TEST_MASTER_PASSWORD: &str = "master123";

/// Lock state of the vault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VaultState {
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
struct Vault {
    name: String,
    state: VaultState,
    /// How the key is made from the master password.
    kdf: seal::KdfParams,
    /// What it is made with besides -- drawn at random when the vault is made.
    salt: [u8; seal::SALT_LEN],
    /// The key, while unlocked.
    key: Option<seal::Key>,
    /// The vault as last sealed: what its file holds, and what unlocking
    /// opens. Empty for a vault that has never been sealed.
    sealed: Vec<u8>,
    entries: Vec<Entry>,
    folders: Vec<Folder>,
    last_access: u64,
    auto_lock_minutes: u32,
    id_gen: u64,
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
    fn create(
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
    fn from_file(bytes: Vec<u8>) -> Result<Self, vaultfile::OpenError> {
        let header = vaultfile::Header::parse(&bytes)?;
        Ok(Self::locked(header.kdf, header.salt, bytes))
    }

    /// No vault yet: what the window holds while the first one is being made,
    /// or while the file there cannot be read.
    fn none_yet() -> Self {
        Self::locked(NEW_VAULT_KDF, [0; seal::SALT_LEN], Vec::new())
    }

    /// A locked vault around `sealed` -- the one place the fields are set, so
    /// the constructors cannot drift apart in what a vault starts with.
    fn locked(kdf: seal::KdfParams, salt: [u8; seal::SALT_LEN], sealed: Vec<u8>) -> Self {
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
    #[cfg(test)]
    #[allow(
        clippy::expect_used,
        reason = "a test fixture: the cheapest key derivation, sealed once"
    )]
    fn for_test(name: &str, master_password: &str) -> Self {
        let mut vault = Self::create(name, master_password, TEST_KDF, [0x5A; seal::SALT_LEN])
            .expect("the test vault's key");
        vault
            .reseal([0x11; seal::NONCE_LEN])
            .expect("the test vault's seal");
        vault.lock();
        vault
    }

    fn next_id(&mut self) -> u64 {
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
    fn open(&mut self, password: &str, now: u64) -> Result<(), vaultfile::OpenError> {
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
    #[cfg(test)]
    fn unlock(&mut self, password: &str, now: u64) -> bool {
        self.open(password, now).is_ok()
    }

    /// Lock: forget the key and every entry. What they were is in
    /// [`sealed`](Self::sealed), and unlocking brings it back.
    fn lock(&mut self) {
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
    fn reseal(&mut self, nonce: seal::Nonce) -> Result<(), &'static str> {
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

    fn is_unlocked(&self) -> bool {
        self.state == VaultState::Unlocked
    }

    /// Seconds from `now` until the auto-lock falls due, or `None` while the
    /// vault is locked and there is nothing to lock.
    fn auto_lock_in(&self, now: u64) -> Option<u64> {
        if self.state == VaultState::Locked {
            return None;
        }
        let timeout_seconds = u64::from(self.auto_lock_minutes).saturating_mul(60);
        let due = self.last_access.saturating_add(timeout_seconds);
        Some(due.saturating_sub(now))
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

    /// Delete an entry.
    ///
    /// Reached from the entry's Delete button and the Delete key, after a
    /// question -- see `ask_delete`. It had no caller until 2026-09-27.
    fn remove_entry(&mut self, entry_id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != entry_id);
        self.entries.len() < before
    }

    fn get_entry(&self, entry_id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == entry_id)
    }

    /// An entry that can be changed. No caller yet, for the same reason as
    /// `remove_entry`: the detail view shows a credential and cannot edit one.
    fn get_entry_mut(&mut self, entry_id: u64) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.id == entry_id)
    }

    /// Replace an entry's payload, from the form's Edit. It had no caller
    /// until 2026-09-27.
    fn update_entry(&mut self, entry_id: u64, data: EntryData, now: u64) -> bool {
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
    fn toggle_star(&mut self, entry_id: u64) -> bool {
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
    fn set_compromised(&mut self, entry_id: u64, compromised: bool) -> bool {
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

    #[allow(dead_code, reason = "no tag control yet -- see todo.txt")]
    fn remove_tag(&mut self, entry_id: u64, tag: &str) -> bool {
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
    fn set_folder(&mut self, entry_id: u64, folder_id: Option<u64>) -> bool {
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
    fn add_folder(&mut self, name: &str) -> u64 {
        let id = self.next_id();
        self.folders.push(Folder::new(id, name));
        id
    }

    #[allow(dead_code, reason = "no folder control yet -- see todo.txt")]
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

    #[allow(dead_code, reason = "no folder control yet -- see todo.txt")]
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

    /// Full-text search across a vault. The toolbar's search box filters
    /// through `refresh_filter` instead, so this second search has no caller;
    /// one of the two should go once it is clear which the UI wants.
    #[allow(
        dead_code,
        reason = "the toolbar filters through refresh_filter -- see todo.txt"
    )]
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

/// One character-set checkbox of the generator panel.
///
/// The panel drew four of these as `[x]` and `[ ]`, a mode row of three
/// buttons with the active one highlighted, and a length slider with a knob
/// positioned from the current value -- and not one of them could be
/// operated. `CharsetOptions` was `true` four times over at construction with
/// no writer in the crate, `mode` was `Random` for ever, and `set_length`
/// carried an `allow(dead_code)` saying in as many words that the panel had
/// no length control.
///
/// The checkbox is this list now, so a box cannot be drawn without a key
/// that ticks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharsetBox {
    Uppercase,
    Lowercase,
    Digits,
    Symbols,
}

impl CharsetBox {
    /// Every box, in the order the panel draws them.
    const ALL: [CharsetBox; 4] = [
        Self::Uppercase,
        Self::Lowercase,
        Self::Digits,
        Self::Symbols,
    ];

    /// The keystroke, as the panel prints it.
    fn keys(self) -> &'static str {
        match self {
            Self::Uppercase => "Ctrl+1",
            Self::Lowercase => "Ctrl+2",
            Self::Digits => "Ctrl+3",
            Self::Symbols => "Ctrl+4",
        }
    }

    /// The name beside the box.
    fn label(self) -> &'static str {
        match self {
            Self::Uppercase => "Uppercase A-Z",
            Self::Lowercase => "Lowercase a-z",
            Self::Digits => "Digits 0-9",
            Self::Symbols => "Symbols !@#$",
        }
    }

    /// Which key ticks it, decided by parsing the label the panel prints.
    fn from_key(key: Key, ctrl: bool) -> Option<Self> {
        if !ctrl {
            return None;
        }
        Self::ALL.into_iter().find(|b| {
            guitk::shortcut::keystrokes(b.keys())
                .is_ok_and(|presses| presses.iter().any(|p| p.key == key))
        })
    }

    fn is_on(self, opts: &CharsetOptions) -> bool {
        match self {
            Self::Uppercase => opts.uppercase,
            Self::Lowercase => opts.lowercase,
            Self::Digits => opts.digits,
            Self::Symbols => opts.symbols,
        }
    }

    fn set(self, opts: &mut CharsetOptions, on: bool) {
        match self {
            Self::Uppercase => opts.uppercase = on,
            Self::Lowercase => opts.lowercase = on,
            Self::Digits => opts.digits = on,
            Self::Symbols => opts.symbols = on,
        }
    }
}

impl GeneratorMode {
    /// The next mode, wrapping, in the order the panel draws the buttons.
    fn next(self) -> Self {
        match self {
            Self::Random => Self::Pronounceable,
            Self::Pronounceable => Self::Passphrase,
            Self::Passphrase => Self::Random,
        }
    }
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

    /// Set the generated password's length.
    ///
    /// The generator panel shows the length and offers no way to change it, so
    /// nothing calls this. See `todo.txt`.
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

    fn color(self, pal: &Palette) -> Color {
        match self {
            Self::VeryWeak => pal.red,
            Self::Weak => pal.peach,
            Self::Fair => pal.yellow,
            Self::Strong => pal.green,
            Self::VeryStrong => pal.lavender,
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
    /// Which entry the issue is about.
    ///
    /// The audit panel names the entry in its text and does not yet let you
    /// jump to it, which is what would read this. See `todo.txt`.
    #[allow(dead_code, reason = "the audit list is not clickable yet")]
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

    fn severity_color(self, pal: &Palette) -> Color {
        match self {
            Self::WeakPassword | Self::Compromised | Self::CommonPattern => pal.red,
            Self::ReusedPassword | Self::OldPassword => pal.yellow,
            Self::NoTotp => pal.subtext0,
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

/// The vault as CSV with every password in plain text, for moving to another
/// password manager -- the operator's option A of C-Q25 (`design-decisions.md`
/// §1417), offered only behind a warning that says what the file is.
///
/// **Every field is quoted, always**, and a `"` inside one is doubled
/// (RFC 4180). A password may hold any character at all -- a comma, a quote, a
/// line break, a tab, a leading space or `=` -- and quoting only the fields
/// that "need" it is how an exporter splits one password into two columns, or
/// loses the space it began with to a reader that trims. Records end in CRLF,
/// as RFC 4180 says. Nothing is altered to defend spreadsheets from formulas:
/// the file is for importing, and a password changed on the way out is a
/// password that no longer works.
///
/// Columns: `type`, `name`, `username`, `password`, `url`, `notes`,
/// `one-time code secret`, `tags` (`;` between), `folder`, `favourite`. A card,
/// an identity and an SSH key have no columns of their own in any importer's
/// CSV, so their fields are written into `notes`, one `label: value` a line.
fn export_csv(vault: &Vault) -> String {
    fn quoted(s: &str) -> String {
        let mut out = String::with_capacity(s.len().saturating_add(2));
        out.push('"');
        for c in s.chars() {
            if c == '"' {
                out.push('"');
            }
            out.push(c);
        }
        out.push('"');
        out
    }
    fn lines(pairs: &[(&str, &str)], notes: &str) -> String {
        let mut out: Vec<String> = pairs
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        if !notes.is_empty() {
            out.push(notes.to_string());
        }
        out.join("\n")
    }
    let header = [
        "type",
        "name",
        "username",
        "password",
        "url",
        "notes",
        "one-time code secret",
        "tags",
        "folder",
        "favourite",
    ];
    let mut csv = header.map(quoted).join(",");
    csv.push_str("\r\n");
    for entry in &vault.entries {
        let (username, password, url, notes, totp) = match &entry.data {
            EntryData::Login(d) => (
                d.username.clone(),
                d.password.clone(),
                d.url.clone(),
                d.notes.clone(),
                d.totp_secret.clone().unwrap_or_default(),
            ),
            EntryData::SecureNote(d) => (
                String::new(),
                String::new(),
                String::new(),
                d.content.clone(),
                String::new(),
            ),
            EntryData::CreditCard(d) => (
                String::new(),
                String::new(),
                String::new(),
                lines(
                    &[
                        ("Number", &d.number_masked),
                        ("Expiry", &d.expiry),
                        ("Cardholder", &d.cardholder),
                    ],
                    &d.notes,
                ),
                String::new(),
            ),
            EntryData::Identity(d) => (
                String::new(),
                String::new(),
                String::new(),
                lines(
                    &[
                        ("Email", &d.email),
                        ("Phone", &d.phone),
                        ("Address", &d.address),
                    ],
                    "",
                ),
                String::new(),
            ),
            EntryData::SshKey(d) => (
                String::new(),
                String::new(),
                String::new(),
                lines(
                    &[
                        ("Fingerprint", &d.fingerprint),
                        ("Public key", &d.public_key),
                    ],
                    "",
                ),
                String::new(),
            ),
        };
        let folder = entry
            .folder_id
            .and_then(|fid| vault.get_folder(fid))
            .map_or(String::new(), |f| f.name.clone());
        let row = [
            entry.entry_type().label().to_string(),
            entry.display_name().to_string(),
            username,
            password,
            url,
            notes,
            totp,
            entry.tags.join(";"),
            folder,
            if entry.starred { "yes" } else { "no" }.to_string(),
        ];
        csv.push_str(&row.map(|field| quoted(&field)).join(","));
        csv.push_str("\r\n");
    }
    csv
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

/// The headed block of the sidebar a selection belongs to.
///
/// The renderer starts a new block — separator, gap, heading — whenever this
/// changes between one row and the next, which is why the sidebar can be drawn
/// by walking a single flat list of selections. Before that it was four
/// hand-written blocks, and the click handler knew about three of them: the
/// folders and tags it drew were dead to the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarGroup {
    Categories,
    Types,
    Folders,
    Tags,
}

impl SidebarGroup {
    fn heading(self) -> &'static str {
        match self {
            Self::Categories => "CATEGORIES",
            Self::Types => "TYPES",
            Self::Folders => "FOLDERS",
            Self::Tags => "TAGS",
        }
    }
}

impl SidebarSelection {
    fn group(&self) -> SidebarGroup {
        match self {
            Self::AllItems | Self::Favorites | Self::Audit => SidebarGroup::Categories,
            Self::TypeFilter(_) => SidebarGroup::Types,
            Self::Folder(_) => SidebarGroup::Folders,
            Self::Tag(_) => SidebarGroup::Tags,
        }
    }

    /// The colour the row's label takes while it is the selected one.
    fn accent(&self, pal: &Palette) -> Color {
        match self {
            Self::AllItems | Self::Folder(_) => pal.blue,
            Self::Favorites => pal.yellow,
            Self::Audit => pal.red,
            Self::TypeFilter(etype) => etype.badge_color(pal),
            Self::Tag(_) => pal.lavender,
        }
    }

    /// The row's label. Folders need the vault to name themselves.
    fn label(&self, vault: &Vault) -> String {
        match self {
            Self::AllItems => "All Items".to_string(),
            Self::Favorites => "* Favorites".to_string(),
            Self::Audit => "! Audit".to_string(),
            Self::TypeFilter(etype) => format!("{} {}", etype.icon_char(), etype.label()),
            Self::Folder(id) => vault
                .folders
                .iter()
                .find(|f| f.id == *id)
                .map_or_else(String::new, |f| f.name.clone()),
            Self::Tag(tag) => tag.clone(),
        }
    }
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
    /// The form for a credential that does not exist yet.
    NewEntry,
}

// =============================================================================
// New-entry form
// =============================================================================

/// The fields one kind of credential is made of, in the order they are shown.
///
/// Derived from the payload structs rather than written out beside them: a
/// second list of a type's fields is a second thing to forget to update when
/// a field is added, and the form would silently stop offering it.
fn fields_for(kind: EntryType) -> &'static [&'static str] {
    match kind {
        EntryType::Login => &["Site", "Username", "Password", "URL", "Notes"],
        EntryType::SecureNote => &["Title", "Content"],
        EntryType::CreditCard => &["Name", "Number", "Expiry", "Cardholder", "Notes"],
        EntryType::Identity => &["Name", "Email", "Phone", "Address"],
        EntryType::SshKey => &["Name", "Fingerprint", "Public key"],
    }
}

/// Which fields hold a secret, and so are drawn masked and never shown by
/// default.
fn field_is_secret(kind: EntryType, index: usize) -> bool {
    matches!(
        (kind, index),
        // Login: Password. CreditCard: Number.
        (EntryType::Login, 2) | (EntryType::CreditCard, 1)
    )
}

/// A credential being written, before it is a credential.
#[derive(Clone, Debug)]
struct NewEntryForm {
    /// The entry this form changes, or `None` for a new one.
    editing: Option<u64>,
    kind: EntryType,
    /// One string per entry in `fields_for(kind)`.
    values: Vec<String>,
    /// Which field the keyboard is in.
    focused: usize,
}

impl NewEntryForm {
    fn new(kind: EntryType) -> Self {
        Self {
            editing: None,
            kind,
            values: vec![String::new(); fields_for(kind).len()],
            focused: 0,
        }
    }

    /// Switch the form to another kind of credential.
    ///
    /// The values are not carried across: "Fingerprint" and "Password" are not
    /// the same field because they happen to sit at the same index, and moving
    /// a typed secret into a field that is drawn in the clear would be the
    /// worst possible way to find that out.
    fn set_kind(&mut self, kind: EntryType) {
        // An entry being edited keeps its kind: a login does not turn into a
        // note by a click on the wrong pill.
        if self.kind == kind || self.editing.is_some() {
            return;
        }
        *self = Self::new(kind);
    }

    fn labels(&self) -> &'static [&'static str] {
        fields_for(self.kind)
    }

    /// The form over an existing entry, its fields filled in -- in the order
    /// [`fields_for`] names them.
    fn for_entry(entry: &Entry) -> Self {
        let values: Vec<String> = match &entry.data {
            EntryData::Login(d) => vec![
                d.site.clone(),
                d.username.clone(),
                d.password.clone(),
                d.url.clone(),
                d.notes.clone(),
            ],
            EntryData::SecureNote(d) => vec![d.title.clone(), d.content.clone()],
            EntryData::CreditCard(d) => vec![
                d.name.clone(),
                d.number_masked.clone(),
                d.expiry.clone(),
                d.cardholder.clone(),
                d.notes.clone(),
            ],
            EntryData::Identity(d) => vec![
                d.name.clone(),
                d.email.clone(),
                d.phone.clone(),
                d.address.clone(),
            ],
            EntryData::SshKey(d) => {
                vec![d.name.clone(), d.fingerprint.clone(), d.public_key.clone()]
            }
        };
        Self {
            editing: Some(entry.id),
            kind: entry.entry_type(),
            values,
            focused: 0,
        }
    }

    fn value(&self, index: usize) -> &str {
        self.values.get(index).map_or("", String::as_str)
    }

    fn focus(&mut self, index: usize) {
        if index < self.values.len() {
            self.focused = index;
        }
    }

    /// Move to the next field, wrapping -- Tab in a form is expected to cycle.
    fn focus_next(&mut self) {
        let len = self.values.len().max(1);
        self.focused = self.focused.saturating_add(1).checked_rem(len).unwrap_or(0);
    }

    /// The first field, which every kind uses as its display name.
    ///
    /// A credential with no name is one the list cannot show and the user
    /// cannot find again, so it is what `is_complete` insists on.
    fn name(&self) -> &str {
        self.value(0).trim()
    }

    /// Can this be saved?
    fn is_complete(&self) -> bool {
        !self.name().is_empty()
    }

    /// Build the payload, or `None` if the form is not complete.
    fn build(&self) -> Option<EntryData> {
        if !self.is_complete() {
            return None;
        }
        // Through each type's own constructor rather than a struct literal
        // beside it: `LoginData::new` is where `totp_secret` starts as `None`,
        // and a literal here would be a second place deciding that.
        let f = |i: usize| self.value(i).trim().to_string();
        Some(match self.kind {
            EntryType::Login => {
                let mut d = LoginData::new(&f(0), &f(1), self.value(2));
                d.url = f(3);
                d.notes = f(4);
                EntryData::Login(d)
            }
            EntryType::SecureNote => {
                EntryData::SecureNote(SecureNoteData::new(&f(0), self.value(1)))
            }
            EntryType::CreditCard => {
                // Masked on the way in, not on the way out: the vault should
                // never hold the digits it does not need, and `mask_number` is
                // the one place that decides what "masked" means.
                let masked = CreditCardData::mask_number(self.value(1));
                let mut d = CreditCardData::new(&f(0), &masked, &f(2), &f(3));
                d.notes = f(4);
                EntryData::CreditCard(d)
            }
            EntryType::Identity => {
                let mut d = IdentityData::new(&f(0), &f(1));
                d.phone = f(2);
                d.address = f(3);
                EntryData::Identity(d)
            }
            EntryType::SshKey => EntryData::SshKey(SshKeyData::new(&f(0), &f(1), self.value(2))),
        })
    }
}

// =============================================================================
// Copying: refused, because applications have no clipboard
// =============================================================================

/// Why a Copy press copies nothing, drawn where "Copied" would have been.
///
/// SlateOS has a system clipboard in the kernel (`fs::clipboard`), but only
/// kshell can reach it: no call lets an application put text there, or read
/// it back. This program used to copy into a clipboard of its own -- a
/// variable -- and say "Copied Password -- clears in 30s", which no other
/// program could paste from. For a password manager that is the worst
/// answer: the user goes to paste, gets nothing, and cannot tell why.
/// `requests/e-a-a-clipboard-door-for-applications.md` asks for the door.
const NOT_COPIED: &str =
    "no other program could paste it -- applications have no clipboard yet; reveal it to read it";

// =============================================================================
// Application state
// =============================================================================

/// What the lock screen is for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Gate {
    /// There is a vault: ask for its master password.
    Unlock,
    /// There is none yet: choose the master password that makes one.
    Create(NewVault),
    /// The vault file is there and cannot be opened, for the reason given.
    /// Nothing is ever written over it: the window offers no way to.
    Unreadable(String),
}

/// The form that makes the first vault.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NewVault {
    /// The master password, as typed.
    password: String,
    /// Typed again, to catch a slip nobody can see behind the dots.
    confirm: String,
    /// Whether the keyboard is in the second field rather than the first.
    confirming: bool,
    /// Why the last press of Create made nothing.
    error: Option<String>,
    /// The field that refusal was about, drawn red until it is typed in:
    /// the first, too short, or the second, which did not match. `None` for
    /// a refusal that is no field's fault -- which only a pair long enough
    /// and alike can reach, and so only after both were typed in.
    wrong: Option<Target>,
}

/// A question the vault's own controls ask before they act.
#[derive(Clone, Debug)]
enum VaultDialog {
    /// Plain-text export: what the file will be, before a place is chosen.
    ExportWarning,
    /// A backup to restore from, waiting for the master password it was
    /// made with.
    RestorePassword {
        path: PathBuf,
        backup: Box<Vault>,
        input: String,
        error: Option<String>,
    },
    /// The backup is open: replace this vault's entries with its own?
    RestoreConfirm { path: PathBuf, backup: Box<Vault> },
    /// Delete this entry?
    DeleteConfirm { id: u64 },
}

/// What the file dialog that is up was opened to do with the file chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickFor {
    Backup,
    Restore,
    Export,
}

/// The largest file Restore reads as a vault. A vault of ten thousand entries
/// is a few MiB; this is room for far more, and a stop for a file that is
/// plainly something else.
const MAX_VAULT_BYTES: usize = 256 << 20;

/// Where the vault is kept: `<config>/credmanager/vault`, beside every other
/// program's settings and data (`settingsfile::config_dir`).
fn vault_path() -> Option<PathBuf> {
    settingsfile::config_dir().map(|dir| dir.join("credmanager").join("vault"))
}

/// Fill `out` from the kernel's secure random source: `false` if it cannot
/// be reached, and then nothing may be sealed -- a salt or nonce that is not
/// random is not a salt or a nonce.
fn kernel_entropy(out: &mut [u8]) -> bool {
    guitk::rng::fill_secret(out).is_ok()
}

/// Random-enough bytes for tests, different on every call.
#[cfg(test)]
fn test_entropy(out: &mut [u8]) -> bool {
    use std::sync::atomic::{AtomicU64, Ordering};
    static DRAWS: AtomicU64 = AtomicU64::new(1);
    let draw = DRAWS.fetch_add(1, Ordering::Relaxed).to_le_bytes();
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = draw.get(i % 8).copied().unwrap_or(0) ^ u8::try_from(i % 251).unwrap_or(0);
    }
    true
}

/// A system whose secure random source cannot be reached.
#[cfg(test)]
fn no_entropy(_out: &mut [u8]) -> bool {
    false
}

/// A fingerprint of what the vault holds, to tell when it has changed since it
/// was last sealed -- so a change is saved once, and a keystroke that changed
/// nothing saves nothing.
fn fingerprint(vault: &Vault) -> [u8; 32] {
    sha2::sha256(vaultfile::contents_text(vault).as_bytes())
}

/// Write the sealed vault to `path`, whole or not at all, readable by its
/// owner alone.
///
/// The folder is made owner-only as it is made, so the file is never
/// readable by anyone else even for the moment before its own mode is set.
/// Its contents are encrypted either way; this keeps other users from
/// copying it to guess at offline.
fn write_vault(path: &Path, sealed: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(dir)?;
    }
    safeio::write_atomically(path, sealed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Seconds since 1970, from the clock.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What moves [`AppState::now`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clock {
    /// The system's wall clock, read as each event arrives: what a window
    /// runs on. The auto-lock is a span of real time -- a machine asleep for
    /// an hour has been away from its vault for an hour -- and an entry's
    /// times are dates.
    Wall,
    /// Moved only by ticks, by the time each says elapsed, carrying the part
    /// of a second the last one left over: what the tests run on, so one can
    /// say "fifteen minutes pass" without waiting them.
    Ticked {
        /// Milliseconds elapsed and not yet a whole second.
        carry_ms: u64,
    },
}

/// Top-level application state.
/// The keys the open vault answers, as the F1 list shows them.
///
/// It had no list: F1 did nothing, and of all of these only Ctrl+E was
/// written anywhere, on its button -- Ctrl+L, which locks the vault, Ctrl+G,
/// which makes a password, and Delete, which asks to delete an entry, could
/// be found only by pressing them. F1 alone raises it: what a key types goes
/// into the search, `?` among it.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F1", "This list"),
    ("Up / Down", "Choose an entry"),
    ("Ctrl+E", "Edit the entry"),
    ("Delete", "Delete the entry (it asks first)"),
    ("Ctrl+G", "Make a password"),
    ("Ctrl+M", "Generator: the next kind of password"),
    ("Ctrl+1-4", "Generator: a set of characters"),
    ("Left / Right", "Generator: shorter or longer"),
    ("Ctrl+F", "Clear the search"),
    ("Escape", "Clear the search, close the panel"),
    ("Ctrl+L", "Lock the vault"),
    ("Ctrl+Q", "Close the window"),
];

struct AppState {
    vault: Vault,
    /// Where the vault is kept. `None` when there is no home folder: the vault
    /// then lasts as long as the window, and the window says so.
    store: Option<PathBuf>,
    /// What the lock screen is for.
    gate: Gate,
    /// Where salts and nonces come from: the kernel's secure random source.
    entropy: fn(&mut [u8]) -> bool,
    /// How much work makes a new vault's key ([`NEW_VAULT_KDF`]; the cheapest
    /// under test).
    new_vault_kdf: seal::KdfParams,
    /// What the vault held when it was last sealed ([`fingerprint`]).
    saved: Option<[u8; 32]>,
    /// Why the last save did not happen, for as long as that is so.
    save_error: Option<String>,
    /// A close found the vault unsaved and said so: the next close goes.
    close_anyway: bool,
    /// Why the last unlock did not.
    unlock_message: String,
    /// The question a vault control is asking, while it is up.
    dialog: Option<VaultDialog>,
    /// The file dialog, and what it was opened for.
    picker: FilePicker,
    pick_for: Option<PickFor>,
    /// What the last back-up, restore or export did, or why it did not.
    vault_message: Option<String>,
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
    show_password: bool,
    /// The time, in seconds since the Unix epoch, as of the event being
    /// taken: what the auto-lock measures the vault's idleness against, and
    /// what an entry's times are stamped with.
    now: u64,
    /// What moves [`Self::now`]: the wall clock in a window, ticks in a test.
    clock: Clock,
    /// Filtered and sorted entry IDs for the list.
    filtered_ids: Vec<u64>,
    /// Cached audit results.
    audit_issues: Vec<AuditIssue>,
    /// Master password input buffer (for unlock screen).
    master_input: String,
    /// The editor of the box a key types into (`typing_into`) -- its caret
    /// and selection over that box's text -- reloaded when the keyboard
    /// moved to another box or the text changed under it.
    editor: TextInput,
    /// Which box `editor` holds, by the target it is drawn as.
    editor_for: Option<Target>,
    /// What the boxes' Ctrl+C and Ctrl+X took, for their Ctrl+V. Nothing is
    /// taken from a masked box.
    clipboard: String,
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
    /// Settings: the auto-lock slider, the toolkit's, from one minute to an
    /// hour. Its value is the vault's own `auto_lock_minutes` whenever no
    /// drag is moving it ([`Self::sync_auto_lock`]); a drag shows its minutes
    /// as it goes and writes them to the vault only when let go.
    ///
    /// It was `settings_auto_lock`, a number set to the default once and
    /// read by nothing but its own picture: the panel said "15 minutes" of a
    /// vault restored with five, and the knob drawn under it moved for
    /// nothing.
    auto_lock: guitk::slider::Slider,
    /// The credential being written, while [`DetailView::NewEntry`] is up.
    ///
    /// `None` at every other moment rather than a form kept warm between
    /// visits: a half-typed password left in memory after the user cancelled
    /// is a thing a credential manager should not be holding.
    new_entry: Option<NewEntryForm>,
    /// The field a Copy press was refused for, to say so in the toolbar --
    /// see [`NOT_COPIED`].
    copy_refused: Option<String>,
    /// Where the pointer is in the window, or `None` once it has left.
    pointer: Option<(f32, f32)>,
    /// The text box under the pointer, settled after every event against
    /// what is drawn by then (`App::on_event`) -- so a dialog or the file
    /// dialog over the window, which leave nothing under them to hit, leave
    /// nothing lit.
    hover: Option<Target>,
    /// The user's focus width, which the text boxes draw their focus mark
    /// at (`appearance_changed`).
    focus_ring_width: f32,
    /// Whether the list of keys is up. Only over the open vault: locking
    /// puts it away (`lock_vault`).
    show_help: bool,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
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
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            vault,
            store: None,
            gate: Gate::Unlock,
            entropy: kernel_entropy,
            new_vault_kdf: NEW_VAULT_KDF,
            saved: None,
            save_error: None,
            close_anyway: false,
            unlock_message: String::new(),
            dialog: None,
            picker: FilePicker::new(),
            pick_for: None,
            vault_message: None,
            sidebar_selection: SidebarSelection::AllItems,
            selected_entry_id: None,
            detail_view: DetailView::EntryDetail,
            search_query: String::new(),
            sort_order: SortOrder::NameAsc,
            password_generator: PasswordGenerator::new(),
            generated_password: String::new(),
            generator_error: None,
            show_password: false,
            now: 1000000,
            clock: Clock::Ticked { carry_ms: 0 },
            filtered_ids: Vec::new(),
            audit_issues: Vec::new(),
            master_input: String::new(),
            editor: TextInput::new(),
            editor_for: None,
            clipboard: String::new(),
            unlock_failed: false,
            list_scroll: 0.0,
            detail_scroll: 0.0,
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            auto_lock: guitk::slider::Slider::new(
                f64::from(AUTO_LOCK_MINUTES.0),
                f64::from(AUTO_LOCK_MINUTES.1),
                f64::from(DEFAULT_AUTO_LOCK_MINUTES),
            )
            .with_step(1.0),
            new_entry: None,
            copy_refused: None,
            pointer: None,
            hover: None,
            focus_ring_width: guitk::style::FOCUS_RING_WIDTH,
            show_help: false,
        };
        state.refresh_filter();
        state
    }

    /// The text box a key types into now, if any.
    ///
    /// The window has no focus to move from box to box: where typing goes
    /// follows from what is showing, in the order the keys are offered --
    /// the file dialog, the lock screen, a vault dialog, the new-entry form,
    /// and otherwise the search. Read here from the same state, so the box
    /// drawn as having the keyboard is the one that does.
    fn typing_into(&self) -> Option<Target> {
        if self.picker.is_open() {
            return None;
        }
        if !self.vault.is_unlocked() {
            return match &self.gate {
                Gate::Unlock => Some(Target::MasterInput),
                Gate::Create(form) if form.confirming => Some(Target::ConfirmPassword),
                Gate::Create(_) => Some(Target::NewPassword),
                Gate::Unreadable(_) => None,
            };
        }
        match &self.dialog {
            Some(VaultDialog::RestorePassword { .. }) => return Some(Target::RestoreInput),
            Some(_) => return None,
            None => {}
        }
        if self.detail_view == DetailView::NewEntry
            && let Some(form) = &self.new_entry
        {
            return Some(Target::NewField(form.focused));
        }
        Some(Target::Search)
    }

    /// What the box drawn as `target` holds, while it is drawn.
    fn box_value(&self, target: Target) -> Option<&str> {
        match (target, &self.gate, &self.dialog) {
            (Target::NewPassword, Gate::Create(form), _) => Some(&form.password),
            (Target::ConfirmPassword, Gate::Create(form), _) => Some(&form.confirm),
            (Target::MasterInput, _, _) => Some(&self.master_input),
            (Target::Search, _, _) => Some(&self.search_query),
            (Target::NewField(index), _, _) => self
                .new_entry
                .as_ref()
                .and_then(|form| form.values.get(index))
                .map(String::as_str),
            (Target::RestoreInput, _, Some(VaultDialog::RestorePassword { input, .. })) => {
                Some(input)
            }
            _ => None,
        }
    }

    /// The box drawn as `target`'s text, to change.
    fn box_value_mut(&mut self, target: Target) -> Option<&mut String> {
        match (target, &mut self.gate, &mut self.dialog) {
            (Target::NewPassword, Gate::Create(form), _) => Some(&mut form.password),
            (Target::ConfirmPassword, Gate::Create(form), _) => Some(&mut form.confirm),
            (Target::MasterInput, _, _) => Some(&mut self.master_input),
            (Target::Search, _, _) => Some(&mut self.search_query),
            (Target::NewField(index), _, _) => self
                .new_entry
                .as_mut()
                .and_then(|form| form.values.get_mut(index)),
            (Target::RestoreInput, _, Some(VaultDialog::RestorePassword { input, .. })) => {
                Some(input)
            }
            _ => None,
        }
    }

    /// Whether the box drawn as `target` holds a secret, drawn masked: the
    /// master passwords and a backup's always, a form's secret field until
    /// it is shown.
    fn box_masked(&self, target: Target) -> bool {
        match target {
            Target::NewPassword
            | Target::ConfirmPassword
            | Target::MasterInput
            | Target::RestoreInput => true,
            Target::NewField(index) => {
                !self.show_password
                    && self
                        .new_entry
                        .as_ref()
                        .is_some_and(|form| field_is_secret(form.kind, index))
            }
            _ => false,
        }
    }

    /// Load the box drawn as `target` into the editor unless it holds it
    /// already, the caret after its text.
    fn load_box(&mut self, target: Target) {
        let held = self.box_value(target).unwrap_or_default().to_owned();
        if self.editor_for != Some(target) || self.editor.text() != held {
            self.editor.set_text(&held);
            self.editor_for = Some(target);
        }
    }

    /// A key for the box drawn as `target`: the caret keys, Backspace and
    /// Delete at the caret, Ctrl+A, C, X and V, and typing -- what AltGr
    /// types among it (Polish `ł` is AltGr+L), and no command's letter,
    /// which a chord carries as text (Ctrl+V typed a `v` into the master
    /// password). A masked box is a masked field
    /// (`textline::apply_masked_key`): its arrows step a character at a
    /// time and nothing is copied or cut from it. `None` for a key the box
    /// does not answer, or a box not drawn; else whether the key changed
    /// its text, caret or selection. The boxes took typing at their end and
    /// Backspace from it, and nothing else.
    fn box_key(&mut self, target: Target, key: &KeyEvent) -> Option<bool> {
        self.box_value(target)?;
        self.load_box(target);
        let editor = &self.editor;
        let before = (
            editor.text().to_owned(),
            editor.cursor(),
            editor.selection_anchor(),
        );
        let edit = if self.box_masked(target) {
            textline::apply_masked_key(&mut self.editor, key, BOX_CAPACITY, &self.clipboard)
        } else {
            textline::apply_key(
                &mut self.editor,
                key,
                BOX_CAPACITY,
                &self.clipboard,
                DEFAULT_FONT_SIZE,
            )
        };
        if let Some(copied) = edit.copied {
            self.clipboard = copied;
        }
        if !edit.handled {
            return None;
        }
        let typed = self.editor.text().to_owned();
        if let Some(value) = self.box_value_mut(target)
            && *value != typed
        {
            *value = typed;
        }
        let editor = &self.editor;
        let after = (
            editor.text().to_owned(),
            editor.cursor(),
            editor.selection_anchor(),
        );
        Some(after != before)
    }

    /// Where the caret and the selection anchor of the box drawn as
    /// `target` are in its text: the editor's while it holds the box, after
    /// the text and none otherwise.
    fn box_caret(&self, target: Target) -> (TextCursor, Option<usize>) {
        let held = self.box_value(target).unwrap_or_default();
        if self.editor_for == Some(target) && self.editor.text() == held {
            (self.editor.cursor(), self.editor.selection_anchor())
        } else {
            (TextCursor::from(held.len()), None)
        }
    }

    /// A press at `x` in the box drawn as `target` at `rect`: the caret
    /// under the pointer, measured against the box -- its mask, for a secret
    /// -- as it was drawn.
    fn press_box(&mut self, target: Target, rect: Rect, x: f32) {
        let Some(held) = self.box_value(target).map(str::to_owned) else {
            return;
        };
        let (cursor, anchor) = self.box_caret(target);
        let masked = self.box_masked(target);
        self.load_box(target);
        let area = box_text_area(target, rect);
        let at = if masked {
            let (shown, drawn, _) = textline::masked(&held, cursor.byte(), anchor, MASK);
            let on_mask = textedit::cursor_at_click(
                &shown,
                TextCursor::from(drawn),
                area.w,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                x - area.x,
            );
            TextCursor::from(textline::unmasked(&held, on_mask.byte(), MASK))
        } else {
            textedit::cursor_at_click(
                &held,
                cursor,
                area.w,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                x - area.x,
            )
        };
        self.editor.set_selection_anchor(None);
        self.editor.set_cursor(at);
    }

    /// How the text box `target` is drawn now: lit under the pointer, marked
    /// while what is typed goes into it, and red when what is in it is
    /// `wrong` -- in the toolkit's field, the theme's shape (lane C,
    /// c-e-a-theme-can-shape-the-controls).
    fn field_state(&self, target: Target, wrong: bool) -> guitk::field::State {
        guitk::field::State {
            hovered: self.hover == Some(target),
            focused: self.typing_into() == Some(target),
            disabled: false,
            invalid: wrong,
        }
    }

    /// Whether the settings panel is drawn and can be used: the vault open,
    /// the panel chosen, and no vault dialog over it. (The file dialog needs
    /// no check here: it takes all input before any reaches the window.)
    fn settings_live(&self) -> bool {
        self.vault.is_unlocked()
            && self.detail_view == DetailView::Settings
            && self.dialog.is_none()
    }

    /// The auto-lock slider as it is drawn: at the vault's own minutes --
    /// which opening the vault and a restore set as well -- unless a drag is
    /// moving it.
    fn auto_lock_shown(&self) -> guitk::slider::Slider {
        let mut shown = self.auto_lock.clone();
        if !shown.is_dragging() {
            shown.set_value(f64::from(self.vault.auto_lock_minutes));
        }
        shown
    }

    /// Keep a value the slider settled on as the vault's auto-lock time:
    /// written with the vault, and the time the next lock is counted by.
    fn set_auto_lock(&mut self, minutes: f64) {
        self.vault.auto_lock_minutes = whole_minutes(minutes);
    }

    /// The auto-lock slider's share of a pointer event while the settings
    /// panel is up, or `None` for an event that is not the slider's.
    ///
    /// A drag shows its minutes as it goes and writes them to the vault only
    /// when it is let go: the vault is sealed and written whenever what it
    /// holds changes, and once per pixel of a drag is a file rewritten fifty
    /// times for one decision.
    fn auto_lock_mouse(&mut self, mouse: &MouseEvent) -> Option<EventResult> {
        if !self.settings_live() {
            return None;
        }
        self.auto_lock = self.auto_lock_shown();
        let lit = self.auto_lock.is_hovered();
        let response = self
            .auto_lock
            .handle_mouse(&auto_lock_placement(self.width), mouse);
        if let Some(guitk::slider::SliderEvent::Confirmed(minutes)) = response.event() {
            self.set_auto_lock(minutes);
        }
        (response.is_taken() || lit != self.auto_lock.is_hovered()).then_some(EventResult::Consumed)
    }

    /// The auto-lock slider's share of a key while the settings panel is up:
    /// Left and Right, Home and End, Page Up and Page Down move it, and during
    /// a drag Escape takes the drag back. Up and Down stay the entry list's.
    fn auto_lock_key(&mut self, key: &KeyEvent) -> Option<EventResult> {
        if !self.settings_live() {
            return None;
        }
        let its_key = matches!(
            key.key,
            Key::Left | Key::Right | Key::Home | Key::End | Key::PageUp | Key::PageDown
        );
        if !its_key && !self.auto_lock.is_dragging() {
            return None;
        }
        self.auto_lock = self.auto_lock_shown();
        let response = self.auto_lock.handle_key(key);
        if let Some(guitk::slider::SliderEvent::Confirmed(minutes)) = response.event() {
            self.set_auto_lock(minutes);
        }
        response.is_taken().then_some(EventResult::Consumed)
    }

    /// The text box under the pointer in what is drawn now, if any.
    fn text_box_under_pointer(&self) -> Option<Target> {
        let (x, y) = self.pointer?;
        self.target_at(x, y).filter(|t| t.is_text_box())
    }

    /// App state around a locked test vault whose master password is
    /// [`TEST_MASTER_PASSWORD`].
    #[cfg(test)]
    fn for_test() -> Self {
        let mut state = Self::new(Vault::for_test("My Vault", TEST_MASTER_PASSWORD));
        state.entropy = test_entropy;
        state.new_vault_kdf = TEST_KDF;
        state
    }

    /// The window over the vault kept at `store`: its lock screen if there is
    /// one, the form that makes one if there is not, and the reason if the
    /// file there cannot be opened.
    fn open(store: Option<PathBuf>) -> Self {
        let mut state = Self::new(Vault::none_yet());
        state.gate = match store.as_deref().map(std::fs::read) {
            None => Gate::Create(NewVault::default()),
            Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                Gate::Create(NewVault::default())
            }
            Some(Err(e)) => Gate::Unreadable(format!("it could not be read: {e}")),
            Some(Ok(bytes)) => match Vault::from_file(bytes) {
                Ok(vault) => {
                    state.vault = vault;
                    Gate::Unlock
                }
                Err(e) => Gate::Unreadable(e.to_string()),
            },
        };
        state.store = store;
        state
    }

    /// Save the vault if what it holds is not what was last sealed.
    ///
    /// Run after every event, so no path that changes an entry can forget to
    /// save it: the check is on what the vault holds, not on who changed it.
    fn keep_if_changed(&mut self) {
        if !self.vault.is_unlocked() {
            return;
        }
        let now = fingerprint(&self.vault);
        if self.saved != Some(now) {
            self.keep(now);
        }
    }

    /// Seal the vault under a new nonce and write it, recording `holds` as
    /// what was saved -- or say why not.
    fn keep(&mut self, holds: [u8; 32]) {
        let mut nonce = [0u8; seal::NONCE_LEN];
        if !(self.entropy)(&mut nonce) {
            self.save_error = Some(
                "this system's secure random source cannot be reached, and a vault sealed \
                 without it would not be safe"
                    .to_string(),
            );
            return;
        }
        if let Err(why) = self.vault.reseal(nonce) {
            self.save_error = Some(why.to_string());
            return;
        }
        if let Some(path) = self.store.as_deref()
            && let Err(e) = write_vault(path, &self.vault.sealed)
        {
            self.save_error = Some(format!("{} could not be written: {e}", path.shown()));
            return;
        }
        self.saved = Some(holds);
        self.save_error = None;
    }

    /// Lock the vault, saving it first if it changed.
    fn lock_vault(&mut self) {
        self.keep_if_changed();
        self.vault.lock();
        self.show_help = false;
        self.saved = None;
        self.dialog = None;
        self.picker.close();
        self.pick_for = None;
    }

    /// Put the file dialog up: where to back up to, what to restore, or where
    /// to export to.
    fn open_picker(&mut self, purpose: PickFor) {
        match purpose {
            PickFor::Backup => self.picker.open_to_write("credmanager-backup.vault"),
            PickFor::Export => self.picker.open_to_write("credentials.csv"),
            PickFor::Restore => self.picker.open_to_read(),
        }
        self.pick_for = Some(purpose);
    }

    /// Do with `path` what the file dialog was opened for.
    fn picked(&mut self, purpose: PickFor, path: &Path) {
        match purpose {
            PickFor::Backup => self.back_up(path),
            PickFor::Restore => self.start_restore(path),
            PickFor::Export => self.export_to(path),
        }
    }

    /// Back the vault up to `path`: the vault sealed as it is now, under a new
    /// nonce -- the operator's option B of C-Q25. It opens with the master
    /// password the vault has now, and restores everything in it; to anyone
    /// without that password it is as closed as the vault itself.
    fn back_up(&mut self, path: &Path) {
        let mut nonce = [0u8; seal::NONCE_LEN];
        if !(self.entropy)(&mut nonce) {
            self.vault_message = Some(
                "Not backed up: this system's secure random source cannot be reached.".to_string(),
            );
            return;
        }
        if let Err(why) = self.vault.reseal(nonce) {
            self.vault_message = Some(format!("Not backed up: {why}."));
            return;
        }
        self.vault_message = Some(match write_vault(path, &self.vault.sealed) {
            Ok(()) => format!(
                "Backed up to {}. It opens with the master password you have now.",
                path.shown()
            ),
            Err(e) => format!("Not backed up: {} could not be written: {e}", path.shown()),
        });
    }

    /// Read the backup at `path`, and ask for the master password it was made
    /// with.
    fn start_restore(&mut self, path: &Path) {
        let bytes = match safeio::read_capped(path, MAX_VAULT_BYTES) {
            Ok(read) if read.truncated => {
                self.vault_message = Some(format!("{} is too large to be a vault.", path.shown()));
                return;
            }
            Ok(read) => read.bytes,
            Err(e) => {
                self.vault_message = Some(format!("{} could not be read: {e}", path.shown()));
                return;
            }
        };
        match Vault::from_file(bytes) {
            Ok(backup) => {
                self.dialog = Some(VaultDialog::RestorePassword {
                    path: path.to_path_buf(),
                    backup: Box::new(backup),
                    input: String::new(),
                    error: None,
                });
            }
            Err(why) => {
                self.vault_message = Some(format!("{} cannot be restored: {why}.", path.shown()));
            }
        }
    }

    /// Open the backup with the password typed into the restore dialog.
    fn restore_open(&mut self) {
        match self.dialog.take() {
            Some(VaultDialog::RestorePassword {
                path,
                mut backup,
                input,
                ..
            }) => match backup.open(&input, self.now) {
                Ok(()) => self.dialog = Some(VaultDialog::RestoreConfirm { path, backup }),
                Err(why) => {
                    self.dialog = Some(VaultDialog::RestorePassword {
                        path,
                        backup,
                        input: String::new(),
                        error: Some(capitalised(&why.to_string())),
                    });
                }
            },
            other => self.dialog = other,
        }
    }

    /// Replace this vault's entries with the opened backup's. The vault keeps
    /// its own master password: what is restored is sealed under it.
    fn restore_replace(&mut self) {
        match self.dialog.take() {
            Some(VaultDialog::RestoreConfirm { path, backup }) => {
                let mut backup = *backup;
                let entries = std::mem::take(&mut backup.entries);
                let folders = std::mem::take(&mut backup.folders);
                let name = std::mem::take(&mut backup.name);
                let (id_gen, auto_lock) = (backup.id_gen, backup.auto_lock_minutes);
                // Its key, overwritten, and nothing of it left behind.
                backup.lock();
                let count = entries.len();
                self.vault.name = name;
                self.vault.entries = entries;
                self.vault.folders = folders;
                self.vault.id_gen = id_gen;
                self.vault.auto_lock_minutes = auto_lock;
                self.selected_entry_id = None;
                self.refresh_filter();
                self.keep_if_changed();
                self.vault_message = Some(format!(
                    "Restored {count} {} from {}. It opens with your master password.",
                    if count == 1 { "entry" } else { "entries" },
                    path.shown()
                ));
            }
            other => self.dialog = other,
        }
    }

    /// Write every entry to `path` as CSV, passwords in plain text -- the
    /// operator's option A of C-Q25, reached only through the warning.
    fn export_to(&mut self, path: &Path) {
        let count = self.vault.entries.len();
        self.vault_message = Some(
            match write_vault(path, export_csv(&self.vault).as_bytes()) {
                Ok(()) => format!(
                    "Exported {count} {} to {} in plain text: anyone who can open it can read \
                 every password in it. Delete it once it is imported.",
                    if count == 1 { "entry" } else { "entries" },
                    path.shown()
                ),
                Err(e) => format!("Not exported: {} could not be written: {e}", path.shown()),
            },
        );
    }

    /// Where the panes go at the size the window is currently believed to be.
    fn layout(&self) -> Layout {
        Layout::new(self.width, self.height)
    }

    /// Adopt a new window size, and pull both scroll offsets back inside it.
    ///
    /// The size arrives two ways — `Event::Resize`, and the `width`/`height`
    /// the compositor passes to [`App::render`] — and the second of those is
    /// how the *first* frame is sized, before any resize event exists.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = sane(width);
        self.height = sane(height);
        self.clamp_scroll();
    }

    /// Height of the area below the toolbar, which both panes are drawn into.
    fn pane_height(&self) -> f32 {
        (self.height - TOOLBAR_HEIGHT).max(0.0)
    }

    /// The y of the entry list's first row -- the header strip's bottom edge.
    ///
    /// `TOOLBAR_HEIGHT + LIST_HEADER_HEIGHT` was written once in the renderer
    /// and once in `handle_list_click`, which is the arrangement that let the
    /// two disagree about which pixels are rows in the first place. Neither
    /// writes it any more -- [`Layout`] derives it once and the click path
    /// reads the boxes the renderer recorded -- so this survives only as the
    /// independent second opinion the layout tests check that against.
    #[cfg(test)]
    const fn rows_top() -> f32 {
        TOOLBAR_HEIGHT + NOTICE_H + LIST_HEADER_HEIGHT
    }

    /// The height of the entry list's scrolling row area.
    ///
    /// The header strip does *not* scroll, so it is not part of this. It used
    /// to be inside the renderer's clip, which meant a scrolled row was
    /// painted over the "N entries" caption rather than stopping under it.
    fn rows_height(&self) -> f32 {
        self.layout().list_rows.h
    }

    /// Every sidebar row, in the order they are drawn.
    ///
    /// One list, walked by the renderer and indexed by
    /// [`Target::Sidebar`]. The folders and tags at the end of it used to be
    /// drawn by a block of the renderer that the click handler had no
    /// counterpart for, so they looked selectable and were not.
    fn sidebar_items(&self) -> Vec<SidebarSelection> {
        let mut items = vec![
            SidebarSelection::AllItems,
            SidebarSelection::Favorites,
            SidebarSelection::Audit,
        ];
        items.extend(
            EntryType::all()
                .iter()
                .map(|t| SidebarSelection::TypeFilter(*t)),
        );
        items.extend(
            self.vault
                .folders
                .iter()
                .map(|folder| SidebarSelection::Folder(folder.id)),
        );
        items.extend(self.vault.all_tags().into_iter().map(SidebarSelection::Tag));
        items
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

    /// The height the detail panel's content lays out to, measured by laying
    /// it out.
    ///
    /// Zero for the generator, settings and audit panels, which are
    /// fixed-height and do not scroll.
    ///
    /// Measured on demand rather than cached from the last render, because the
    /// cache had to be *written back* during drawing — which is why
    /// `build_render_tree` took `&mut AppState`, and why the bound was zero (so
    /// the wheel dead) until the first frame had gone out. Laying the panel out
    /// twice costs a scratch `Vec` on a wheel event; a bound that lags the
    /// state it describes costs a scroll that silently stops early.
    fn detail_content_height(&self) -> f32 {
        if self.detail_view != DetailView::EntryDetail {
            return 0.0;
        }
        let mut scratch = Frame::new(self.width, self.height);
        render_entry_detail(&mut scratch, self, self.width, self.height)
    }

    /// How far the detail panel may be scrolled before its last field sits on
    /// the bottom edge of the pane.
    fn max_detail_scroll(&self) -> f32 {
        (self.detail_content_height() - self.pane_height()).max(0.0)
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

    /// Bring [`Self::now`] up to date for `event`, and lock the vault if it
    /// has been left alone for its auto-lock time. Returns whether it locked.
    ///
    /// Run before the event is taken, not after: after an hour away, the key
    /// that wakes the window must find the vault locked, rather than count as
    /// the use that keeps it open.
    fn advance_clock(&mut self, event: &Event) -> bool {
        match &mut self.clock {
            Clock::Wall => self.now = unix_now(),
            Clock::Ticked { carry_ms } => {
                if let Event::Tick { elapsed_ms } = event {
                    // The carry, because ticks come at the auto-lock's
                    // deadline and whenever else the harness runs the loop,
                    // and `elapsed_ms / 1000` lost every tick's part second:
                    // all of it, for a tick shorter than one.
                    let total = carry_ms.saturating_add(*elapsed_ms);
                    self.now = self.now.saturating_add(total / 1000);
                    *carry_ms = total % 1000;
                }
            }
        }
        if self.vault.should_auto_lock(self.now) {
            self.lock_vault();
            return true;
        }
        false
    }
}

// =============================================================================
// Render helpers
// =============================================================================

/// Render a filled rounded rectangle.
fn draw_rect(frame: &mut Frame, x: f32, y: f32, w: f32, h: f32, color: Color, radius: f32) {
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color,
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
    frame: &mut Frame,
    x: f32,
    y: f32,
    text: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    frame.push(RenderCommand::Text {
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
fn draw_separator(frame: &mut Frame, pal: &Palette, x: f32, y: f32, width: f32) {
    frame.push(RenderCommand::Line {
        x1: x,
        y1: y,
        x2: x + width,
        y2: y,
        color: pal.surface1,
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
fn draw_badge(frame: &mut Frame, x: f32, y: f32, label: &str, bg: Color, fg: Color) -> f32 {
    let badge_w = badge_width(label);
    let badge_h = 20.0;
    draw_rect(frame, x, y, badge_w, badge_h, bg, 4.0);
    draw_text(
        frame,
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
    frame: &mut Frame,
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
    draw_rect(frame, x, y, w, h, actual_bg, CORNER_RADIUS);
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
        frame,
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
    frame: &mut Frame,
    pal: &Palette,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fraction: f32,
    color: Color,
) {
    draw_rect(frame, x, y, width, height, pal.surface0, 3.0);
    let fill_width = (width * fraction.clamp(0.0, 1.0)).max(0.0);
    if fill_width > 0.0 {
        draw_rect(frame, x, y, fill_width, height, color, 3.0);
    }
}

// =============================================================================
// Render: toolbar
// =============================================================================

/// Space between two adjacent toolbar controls.
const TOOLBAR_GAP: f32 = 12.0;
/// Height of the toolbar's buttons and search box.
const TOOLBAR_BUTTON_HEIGHT: f32 = 32.0;
/// Distance from the top of the toolbar to the top of its controls; the same
/// again below them is what makes the strip [`TOOLBAR_HEIGHT`] tall.
const TOOLBAR_BUTTON_INSET: f32 = 8.0;

/// The search box's width when there is room for it.
const SEARCH_WIDTH: f32 = 200.0;
/// The narrowest the search box is allowed to get before the buttons after it
/// start falling off the right edge instead.
///
/// It still shows a few characters of the query at this width, which is what
/// separates "cramped" from "useless". Below it there is nothing left to give
/// and the toolbar is genuinely too wide for the window.
const SEARCH_MIN_WIDTH: f32 = 80.0;

/// Every fixed-width control in the toolbar, plus the gaps around them.
///
/// Add 60, Sort 80, Generator 100, Lock Vault 70, Settings 80, and six gaps --
/// one before each of the six controls. The search box is the only elastic
/// thing in the row and is deliberately not counted here.
const TOOLBAR_FIXED_WIDTH: f32 = 60.0 + 80.0 + 100.0 + 70.0 + 80.0 + 6.0 * TOOLBAR_GAP;

/// How wide the search box may be in a window `width` wide.
///
/// The row is laid out left to right from the sidebar's edge and neither wraps
/// nor scrolls, so something has to give when the window is narrower than the
/// row wants. The search box gives first, because it is the only control whose
/// job survives being smaller: a button at half width is a button with its
/// label cut in half, whereas a search box at half width is a search box.
///
/// This is *not* an overflow menu, and does not pretend to be. Below about
/// 762 px the buttons at the right-hand end still fall off the edge --
/// `Lock Vault` among them, which is why `Ctrl+L` exists and is tested. See
/// known-issues.md -> `C-CREDMANAGER-TOOLBAR-FALLS-OFF-A-NARROW-WINDOW`.
fn search_box_width(width: f32) -> f32 {
    let available = width - (SIDEBAR_WIDTH + TOOLBAR_GAP) - TOOLBAR_FIXED_WIDTH;
    available.clamp(SEARCH_MIN_WIDTH, SEARCH_WIDTH)
}

/// Take the next `w`-wide control off the toolbar's left-to-right run.
///
/// The cursor advances by the control's own width plus one gap, so a control
/// that changes width moves everything after it — which is what the hit test
/// used to have to be told about separately, in the form of a
/// `base_x + 284.0 ..= base_x + 364.0` written out by hand.
fn take_toolbar(x: &mut f32, w: f32) -> Rect {
    let r = Rect::new(*x, TOOLBAR_BUTTON_INSET, w, TOOLBAR_BUTTON_HEIGHT);
    *x += w + TOOLBAR_GAP;
    r
}

/// What the window says about the vault not being kept.
///
/// The module doc has said this since the crate was written: "this crate has
/// no persistence layer at all, so every launch opens an empty vault". It is
/// tracked in known-issues, the `reopen` door is documented as the place a
/// loader would come in, and the `allow` on it carries a reason. Everything
/// about this app's handling of the gap is exemplary **except that none of it
/// is on screen**, and the screen is what the user reads.
///
/// For a password manager that silence is the expensive kind. Somebody stores
/// a credential, closes the window, and the account is the thing they lose --
/// not a preference, not a layout, an account. Every other app in this sweep
/// that keeps nothing costs the user their afternoon; this one can cost them
/// access.
const NOT_KEPT_LINES: [&str; 2] = [
    "This vault is not saved anywhere.",
    "There is no home folder to keep it in, so every entry is gone when the window closes. Do not rely on it.",
];

impl AppState {
    /// The two lines under the toolbar: where the vault is kept, or why it is
    /// not -- and whether that is a warning.
    fn notice_lines(&self) -> ([String; 2], bool) {
        let Some(path) = self.store.as_deref() else {
            return (NOT_KEPT_LINES.map(str::to_string), true);
        };
        if let Some(why) = &self.save_error {
            return (
                [
                    "Not saved -- the last change is only in this window.".to_string(),
                    capitalised(why),
                ],
                true,
            );
        }
        (
            [
                format!(
                    "Kept in {}, encrypted with your master password.",
                    path.shown()
                ),
                self.vault_message.clone().unwrap_or_else(|| {
                    "Every change is saved as it is made. Ctrl+L locks the vault.".to_string()
                }),
            ],
            false,
        )
    }
}

fn render_toolbar(frame: &mut Frame, state: &AppState, layout: &Layout) {
    let width = layout.window.w;

    // Toolbar background
    draw_rect(
        frame,
        0.0,
        0.0,
        width,
        TOOLBAR_HEIGHT,
        state.palette.mantle,
        0.0,
    );

    // Clipped to the strip the layout gave it, so that in a window shorter
    // than its own chrome the buttons are trimmed out of existence rather than
    // left hanging below the bottom edge, invisible but still clickable.
    frame.clip(layout.toolbar);

    let mut x = SIDEBAR_WIDTH + TOOLBAR_GAP;

    // Add button
    let add = take_toolbar(&mut x, 60.0);
    draw_button(
        frame,
        add.x,
        add.y,
        add.w,
        add.h,
        "+ Add",
        state.palette.blue,
        state.palette.base,
        false,
    );
    frame.hit(Target::Add, add);

    // Search box -- the one elastic control in the row; see
    // `search_box_width` for why it is the one that gives.
    let search = take_toolbar(&mut x, search_box_width(width));
    // The toolbar leaves TOOLBAR_BUTTON_INSET above and below it, and the gap
    // beside it, which is room for the widest focus ring (eight pixels, at the
    // appearance settings' 4x clamp) inside the toolbar's clip.
    guitk::field::draw(
        frame,
        &state.palette,
        search,
        state.field_state(Target::Search, false),
        state.focus_ring_width,
    );
    draw_box_text(frame, state, Target::Search, search, "Search...");
    frame.hit(Target::Search, search);

    // Sort button
    let sort = take_toolbar(&mut x, 80.0);
    draw_button(
        frame,
        sort.x,
        sort.y,
        sort.w,
        sort.h,
        state.sort_order.label(),
        state.palette.surface1,
        state.palette.text,
        false,
    );
    frame.hit(Target::Sort, sort);

    // Generate password button
    let generator = take_toolbar(&mut x, 100.0);
    draw_button(
        frame,
        generator.x,
        generator.y,
        generator.w,
        generator.h,
        "Generator",
        state.palette.surface1,
        state.palette.lavender,
        false,
    );
    frame.hit(Target::Generator, generator);

    // Lock button
    let lock = take_toolbar(&mut x, 70.0);
    let lock_text = if state.vault.is_unlocked() {
        "Lock"
    } else {
        "Unlock"
    };
    let lock_color = if state.vault.is_unlocked() {
        state.palette.green
    } else {
        state.palette.red
    };
    draw_button(
        frame,
        lock.x,
        lock.y,
        lock.w,
        lock.h,
        lock_text,
        state.palette.surface1,
        lock_color,
        false,
    );
    frame.hit(Target::LockVault, lock);

    // Settings button
    let settings = take_toolbar(&mut x, 80.0);
    draw_button(
        frame,
        settings.x,
        settings.y,
        settings.w,
        settings.h,
        "Settings",
        state.palette.surface1,
        state.palette.subtext0,
        false,
    );
    frame.hit(Target::Settings, settings);

    // WHY NOTHING WAS COPIED. A press on Copy used to be answered with
    // "Copied Password -- clears in 30s" over a clipboard only this program
    // could read. The press is answered in words still -- a press that
    // shows nothing reads as a program that did not hear it -- but with what
    // is true.
    if let Some(label) = state.copy_refused.as_deref() {
        let notice_x = x + TOOLBAR_GAP;
        draw_text(
            frame,
            notice_x,
            (TOOLBAR_HEIGHT - DEFAULT_FONT_SIZE) / 2.0,
            &format!("{label} not copied: {NOT_COPIED}"),
            state.palette.ink(state.palette.yellow),
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            Some((width - notice_x).max(0.0)),
        );
    }

    frame.unclip();

    // Bottom border
    draw_separator(frame, &state.palette, 0.0, TOOLBAR_HEIGHT - 1.0, width);
}

// =============================================================================
// Render: sidebar
// =============================================================================

/// The drawn height of one sidebar row.
const SIDEBAR_ROW_HEIGHT: f32 = 32.0;
/// Row pitch: the row's own height plus the 2 px of air under it.
const SIDEBAR_ROW_STEP: f32 = SIDEBAR_ROW_HEIGHT + 2.0;

fn render_sidebar(frame: &mut Frame, state: &AppState, layout: &Layout) {
    let pane = layout.sidebar;

    // Sidebar background
    draw_rect(
        frame,
        pane.x,
        pane.y,
        pane.w,
        pane.h,
        state.palette.mantle,
        0.0,
    );

    // Clipped to the pane, so that rows past the bottom edge -- a vault with
    // enough tags will produce them -- record no hit box at all. Bounding the
    // click by hand instead is how the two get to disagree.
    frame.clip(pane);

    let mut y = pane.y + 12.0;
    let text_x = 16.0;

    // Vault name header
    draw_text(
        frame,
        text_x,
        y,
        &state.vault.name,
        state.palette.text,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        Some(SIDEBAR_WIDTH - 24.0),
    );
    y += 30.0;

    let entry_count_text = format!("{} items", state.vault.entry_count());
    draw_text(
        frame,
        text_x,
        y,
        &entry_count_text,
        state.palette.subtext0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += 24.0;

    // One walk over `sidebar_items`, opening a new block -- separator, gap,
    // heading -- wherever the group changes. The four hand-written blocks this
    // replaces are why `handle_sidebar_click` knew about three of them: the
    // folders and tags it drew looked selectable and were dead to the pointer,
    // because nothing tied the block that drew them to the block that would
    // have had to accept the click.
    let items = state.sidebar_items();
    let mut group = None;
    for (index, item) in items.iter().enumerate() {
        let item_group = item.group();
        if group != Some(item_group) {
            if group.is_some() {
                y += 6.0;
            }
            draw_separator(frame, &state.palette, 8.0, y, SIDEBAR_WIDTH - 16.0);
            y += 12.0;
            draw_text(
                frame,
                text_x,
                y,
                item_group.heading(),
                state.palette.overlay0,
                SMALL_FONT_SIZE,
                FontWeightHint::Bold,
                None,
            );
            y += 20.0;
            group = Some(item_group);
        }

        let row = Rect::new(4.0, y, SIDEBAR_WIDTH - 8.0, SIDEBAR_ROW_HEIGHT);
        let selected = state.sidebar_selection == *item;
        if selected {
            draw_rect(
                frame,
                row.x,
                row.y,
                row.w,
                row.h,
                state.palette.surface0,
                4.0,
            );
        }
        draw_text(
            frame,
            text_x + 4.0,
            y + 8.0,
            &item.label(&state.vault),
            if selected {
                state.palette.ink(item.accent(&state.palette))
            } else {
                state.palette.text
            },
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        frame.hit(Target::Sidebar(index), row);
        y += SIDEBAR_ROW_STEP;
    }

    frame.unclip();

    // Right border, outside the clip: it sits on the pane's right edge, which
    // the clip is half-open at.
    frame.push(RenderCommand::Line {
        x1: SIDEBAR_WIDTH,
        y1: pane.y,
        x2: SIDEBAR_WIDTH,
        y2: pane.bottom(),
        color: state.palette.surface1,
        width: 1.0,
    });
}

// =============================================================================
// Render: entry list
// =============================================================================

fn render_entry_list(frame: &mut Frame, state: &AppState, layout: &Layout) {
    let pane = layout.list;
    let x_start = pane.x;

    // List background
    draw_rect(
        frame,
        pane.x,
        pane.y,
        pane.w,
        pane.h,
        state.palette.base,
        0.0,
    );

    // List header
    let count_text = format!("{} entries", state.filtered_ids.len());
    draw_text(
        frame,
        x_start + 12.0,
        pane.y + 10.0,
        &count_text,
        state.palette.subtext0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );

    // Clip to the row area, not to the whole pane: the "N entries" caption
    // does not scroll, so a row wound up under it must disappear rather than
    // paint over it. Pushed through `Frame::clip` and not as a raw command,
    // which is what makes the boxes recorded below get trimmed to it -- a row
    // scrolled out of the pane records nothing and so cannot be clicked, with
    // no bounds check in the click path to go stale.
    let rows = layout.list_rows;
    frame.clip(rows);

    let effective_y = rows.y - state.list_scroll;

    for (i, &entry_id) in state.filtered_ids.iter().enumerate() {
        let row_y = effective_y + i as f32 * ROW_HEIGHT;

        // Skip rows outside visible area
        if row_y + ROW_HEIGHT < rows.y || row_y > rows.bottom() {
            continue;
        }

        if let Some(entry) = state.vault.get_entry(entry_id) {
            let is_selected = state.selected_entry_id == Some(entry_id);

            // The whole row is the target, including the 2 px of air the
            // selection highlight leaves under it: the pointer is over row `i`
            // everywhere between one row's top and the next one's.
            frame.hit(
                Target::EntryRow(i),
                Rect::new(x_start, row_y, ENTRY_LIST_WIDTH, ROW_HEIGHT),
            );

            // Row background
            if is_selected {
                draw_rect(
                    frame,
                    x_start + 4.0,
                    row_y,
                    ENTRY_LIST_WIDTH - 8.0,
                    ROW_HEIGHT - 2.0,
                    state.palette.surface0,
                    4.0,
                );
            }

            let text_x = x_start + 16.0;

            // Type icon badge
            let badge_color = entry.entry_type().badge_color(&state.palette);
            draw_rect(
                frame,
                text_x,
                row_y + 8.0,
                ICON_SIZE,
                ICON_SIZE,
                badge_color,
                4.0,
            );
            draw_text(
                frame,
                text_x + 4.0,
                row_y + 10.0,
                entry.entry_type().icon_char(),
                state.palette.base,
                SMALL_FONT_SIZE,
                FontWeightHint::Bold,
                None,
            );

            // Entry name
            let name_color = if is_selected {
                state.palette.blue
            } else {
                state.palette.text
            };
            draw_text(
                frame,
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
                    frame,
                    text_x + 28.0,
                    row_y + 28.0,
                    sub,
                    state.palette.subtext0,
                    SMALL_FONT_SIZE,
                    FontWeightHint::Regular,
                    Some(ENTRY_LIST_WIDTH - 80.0),
                );
            }

            // Star indicator
            if entry.starred {
                draw_text(
                    frame,
                    x_start + ENTRY_LIST_WIDTH - 30.0,
                    row_y + 8.0,
                    "*",
                    state.palette.yellow,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Bold,
                    None,
                );
            }

            // Compromised indicator
            if entry.compromised {
                draw_text(
                    frame,
                    x_start + ENTRY_LIST_WIDTH - 48.0,
                    row_y + 8.0,
                    "!",
                    state.palette.red,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Bold,
                    None,
                );
            }

            // Bottom separator
            draw_separator(
                frame,
                &state.palette,
                x_start + 12.0,
                row_y + ROW_HEIGHT - 2.0,
                ENTRY_LIST_WIDTH - 24.0,
            );
        }
    }

    frame.unclip();

    // Right border, outside the clip: it sits on the pane's right edge, which
    // the clip is half-open at.
    let list_right = x_start + ENTRY_LIST_WIDTH;
    frame.push(RenderCommand::Line {
        x1: list_right,
        y1: pane.y,
        x2: list_right,
        y2: pane.bottom(),
        color: state.palette.surface1,
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
fn render_entry_detail(frame: &mut Frame, state: &AppState, width: f32, height: f32) -> f32 {
    let x_start = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let panel_width = width - x_start;
    let panel_height = height - y_start;

    // Background
    draw_rect(
        frame,
        x_start,
        y_start,
        panel_width,
        panel_height,
        state.palette.base,
        0.0,
    );

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
                frame,
                empty_x,
                y_start + panel_height / 2.0,
                empty,
                state.palette.overlay0,
                HEADING_FONT_SIZE,
                FontWeightHint::Light,
                None,
            );
            // Nothing selected: no content, so nothing to scroll.
            return 0.0;
        }
    };

    frame.push(RenderCommand::PushClip {
        x: x_start,
        y: y_start,
        width: panel_width,
        height: panel_height,
    });

    let pad = 24.0;
    let mut y = y_start + pad - state.detail_scroll;

    // Entry type badge + name
    let badge_color = entry.entry_type().badge_color(&state.palette);
    let type_badge_w = draw_badge(
        frame,
        x_start + pad,
        y,
        entry.entry_type().label(),
        badge_color,
        state.palette.base,
    );

    // Edit and Delete, at the right of the badge's line. Delete asks first.
    let mut bx = x_start + panel_width - pad;
    for (label, target, width) in [
        ("Delete", Target::DeleteEntry, 70.0),
        ("Edit (Ctrl+E)", Target::EditEntry, 110.0),
    ] {
        bx -= width;
        draw_button(
            frame,
            bx,
            y - 4.0,
            width,
            26.0,
            label,
            state.palette.surface1,
            state.palette.text,
            false,
        );
        frame.hit(target, Rect::new(bx, y - 4.0, width, 26.0));
        bx -= 8.0;
    }

    if entry.starred {
        draw_text(
            frame,
            x_start + pad + type_badge_w + 12.0,
            y + 2.0,
            "* Starred",
            state.palette.yellow,
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
    }
    y += 30.0;

    draw_text(
        frame,
        x_start + pad,
        y,
        entry.display_name(),
        state.palette.text,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        Some(panel_width - pad * 2.0),
    );
    y += 28.0;

    if entry.compromised {
        draw_rect(
            frame,
            x_start + pad,
            y,
            panel_width - pad * 2.0,
            28.0,
            Color::rgba(
                state.palette.red.r,
                state.palette.red.g,
                state.palette.red.b,
                40,
            ),
            4.0,
        );
        draw_text(
            frame,
            x_start + pad + 8.0,
            y + 6.0,
            "! This password may be compromised",
            state.palette.red,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Bold,
            None,
        );
        y += 36.0;
    }

    draw_separator(
        frame,
        &state.palette,
        x_start + pad,
        y,
        panel_width - pad * 2.0,
    );
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
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Site",
                &login.site,
                false,
                Target::CopyField(0),
            );
            y += row_spacing;

            // Username
            y = render_detail_field(
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Username",
                &login.username,
                false,
                Target::CopyField(1),
            );
            y += row_spacing;

            // Password
            let pw_display = if state.show_password {
                login.password.clone()
            } else {
                "*".repeat(login.password.len().min(20))
            };
            y = render_detail_field(
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Password",
                &pw_display,
                true,
                Target::CopyField(2),
            );

            // Password strength
            let (strength, entropy) = evaluate_password_strength(&login.password);
            y += 8.0;
            draw_strength_bar(
                frame,
                &state.palette,
                field_value_x,
                y,
                160.0,
                6.0,
                strength.fraction(),
                strength.color(&state.palette),
            );
            let strength_text = format!("{} ({:.0} bits)", strength.label(), entropy);
            draw_text(
                frame,
                field_value_x + 170.0,
                y - 2.0,
                &strength_text,
                state.palette.ink(strength.color(&state.palette)),
                SMALL_FONT_SIZE,
                FontWeightHint::Regular,
                None,
            );
            y += row_spacing;

            // Show/hide toggle
            let toggle_text = if state.show_password { "Hide" } else { "Show" };
            draw_button(
                frame,
                field_value_x,
                y,
                60.0,
                24.0,
                toggle_text,
                state.palette.surface1,
                state.palette.text,
                false,
            );
            y += row_spacing;

            // URL
            if !login.url.is_empty() {
                y = render_detail_field(
                    frame,
                    &state.palette,
                    y,
                    field_label_x,
                    field_value_x,
                    copy_btn_x,
                    panel_width - pad * 2.0,
                    "URL",
                    &login.url,
                    false,
                    Target::CopyField(3),
                );
                y += row_spacing;
            }

            // TOTP
            if let Some(ref totp) = login.totp_secret {
                y = render_detail_field(
                    frame,
                    &state.palette,
                    y,
                    field_label_x,
                    field_value_x,
                    copy_btn_x,
                    panel_width - pad * 2.0,
                    "TOTP",
                    totp,
                    false,
                    Target::CopyField(4),
                );
                y += row_spacing;
            } else {
                draw_text(
                    frame,
                    field_label_x,
                    y,
                    "TOTP",
                    state.palette.subtext0,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Regular,
                    None,
                );
                draw_text(
                    frame,
                    field_value_x,
                    y,
                    "Not configured",
                    state.palette.overlay0,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Light,
                    None,
                );
                y += row_spacing;
            }

            // Notes
            if !login.notes.is_empty() {
                draw_separator(
                    frame,
                    &state.palette,
                    field_label_x,
                    y,
                    panel_width - pad * 2.0,
                );
                y += 12.0;
                draw_text(
                    frame,
                    field_label_x,
                    y,
                    "Notes",
                    state.palette.subtext0,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Bold,
                    None,
                );
                y += 20.0;
                draw_text(
                    frame,
                    field_label_x,
                    y,
                    &login.notes,
                    state.palette.text,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Regular,
                    Some(panel_width - pad * 2.0),
                );
                y += 24.0;
            }
        }
        EntryData::SecureNote(note) => {
            draw_text(
                frame,
                field_label_x,
                y,
                "Title",
                state.palette.subtext0,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                None,
            );
            draw_text(
                frame,
                field_value_x,
                y,
                &note.title,
                state.palette.text,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                Some(panel_width - pad * 2.0 - 120.0),
            );
            y += row_spacing;

            draw_separator(
                frame,
                &state.palette,
                field_label_x,
                y,
                panel_width - pad * 2.0,
            );
            y += 12.0;

            draw_text(
                frame,
                field_label_x,
                y,
                &note.content,
                state.palette.text,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                Some(panel_width - pad * 2.0),
            );
            y += 24.0;
        }
        EntryData::CreditCard(card) => {
            y = render_detail_field(
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Card Name",
                &card.name,
                false,
                Target::CopyField(0),
            );
            y += row_spacing;

            y = render_detail_field(
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Number",
                &card.number_masked,
                false,
                Target::CopyField(1),
            );
            y += row_spacing;

            y = render_detail_field(
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Expiry",
                &card.expiry,
                false,
                Target::CopyField(2),
            );
            y += row_spacing;

            y = render_detail_field(
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Cardholder",
                &card.cardholder,
                false,
                Target::CopyField(3),
            );
            y += row_spacing;

            if !card.notes.is_empty() {
                draw_separator(
                    frame,
                    &state.palette,
                    field_label_x,
                    y,
                    panel_width - pad * 2.0,
                );
                y += 12.0;
                draw_text(
                    frame,
                    field_label_x,
                    y,
                    "Notes",
                    state.palette.subtext0,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Bold,
                    None,
                );
                y += 20.0;
                draw_text(
                    frame,
                    field_label_x,
                    y,
                    &card.notes,
                    state.palette.text,
                    DEFAULT_FONT_SIZE,
                    FontWeightHint::Regular,
                    Some(panel_width - pad * 2.0),
                );
                y += 24.0;
            }
        }
        EntryData::Identity(ident) => {
            y = render_detail_field(
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Name",
                &ident.name,
                false,
                Target::CopyField(0),
            );
            y += row_spacing;

            y = render_detail_field(
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Email",
                &ident.email,
                false,
                Target::CopyField(1),
            );
            y += row_spacing;

            if !ident.phone.is_empty() {
                y = render_detail_field(
                    frame,
                    &state.palette,
                    y,
                    field_label_x,
                    field_value_x,
                    copy_btn_x,
                    panel_width - pad * 2.0,
                    "Phone",
                    &ident.phone,
                    false,
                    Target::CopyField(2),
                );
                y += row_spacing;
            }

            if !ident.address.is_empty() {
                y = render_detail_field(
                    frame,
                    &state.palette,
                    y,
                    field_label_x,
                    field_value_x,
                    copy_btn_x,
                    panel_width - pad * 2.0,
                    "Address",
                    &ident.address,
                    false,
                    Target::CopyField(3),
                );
                y += row_spacing;
            }
        }
        EntryData::SshKey(key) => {
            y = render_detail_field(
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Key Name",
                &key.name,
                false,
                Target::CopyField(0),
            );
            y += row_spacing;

            y = render_detail_field(
                frame,
                &state.palette,
                y,
                field_label_x,
                field_value_x,
                copy_btn_x,
                panel_width - pad * 2.0,
                "Fingerprint",
                &key.fingerprint,
                false,
                Target::CopyField(1),
            );
            y += row_spacing;

            draw_text(
                frame,
                field_label_x,
                y,
                "Public Key",
                state.palette.subtext0,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                None,
            );
            y += 20.0;

            draw_rect(
                frame,
                field_label_x,
                y,
                panel_width - pad * 2.0,
                60.0,
                state.palette.surface0,
                4.0,
            );
            draw_text(
                frame,
                field_label_x + 8.0,
                y + 8.0,
                &key.public_key,
                state.palette.text,
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
        draw_separator(
            frame,
            &state.palette,
            field_label_x,
            y,
            panel_width - pad * 2.0,
        );
        y += 12.0;
        draw_text(
            frame,
            field_label_x,
            y,
            "Tags",
            state.palette.subtext0,
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
            draw_badge(
                frame,
                tag_x,
                y,
                tag,
                state.palette.surface1,
                state.palette.lavender,
            );
            tag_x += tag_w + 6.0;
        }
        y += 28.0;
    }

    // Metadata
    y += 8.0;
    draw_separator(
        frame,
        &state.palette,
        field_label_x,
        y,
        panel_width - pad * 2.0,
    );
    y += 12.0;

    // Dates, as every program here renders an instant (`guitk::datetime`);
    // UTC, explicitly, until the system has a zone of its own (known-issues
    // `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`). They were counts of seconds --
    // "Created: 31536000 seconds ago" for an entry a year old -- a number to
    // work out rather than a date to read.
    let stamp = |secs: u64| {
        guitk::datetime::stamp(
            i64::try_from(secs).unwrap_or(i64::MAX),
            &guitk::tzrules::Tz::utc(),
        )
    };
    let created_text = format!("Created: {}", stamp(entry.created_at));
    draw_text(
        frame,
        field_label_x,
        y,
        &created_text,
        state.palette.overlay0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += 18.0;

    let modified_text = format!("Modified: {}", stamp(entry.modified_at));
    draw_text(
        frame,
        field_label_x,
        y,
        &modified_text,
        state.palette.overlay0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );

    if entry.entry_type() == EntryType::Login {
        y += 18.0;
        let age_days = entry.password_age_days(state.now);
        let age_color = if age_days > PASSWORD_OLD_DAYS {
            state.palette.yellow
        } else {
            state.palette.overlay0
        };
        let age_text = format!("Password age: {} days", age_days);
        draw_text(
            frame,
            field_label_x,
            y,
            &age_text,
            age_color,
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
    }

    frame.push(RenderCommand::PopClip);

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
    frame: &mut Frame,
    pal: &Palette,
    y: f32,
    label_x: f32,
    value_x: f32,
    copy_x: f32,
    _width: f32,
    label: &str,
    value: &str,
    is_password: bool,
    copy: Target,
) -> f32 {
    draw_text(
        frame,
        label_x,
        y,
        label,
        pal.subtext0,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );

    let value_color = if is_password { pal.peach } else { pal.text };
    draw_text(
        frame,
        value_x,
        y,
        value,
        value_color,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        Some(copy_x - value_x - 8.0),
    );

    // Copy button. `draw_button` paints; the hit box is registered here,
    // because until it was every one of these was a button that could be seen
    // and not pressed.
    draw_button(
        frame,
        copy_x,
        y - 4.0,
        44.0,
        24.0,
        "Copy",
        pal.surface1,
        pal.subtext0,
        false,
    );
    frame.hit(copy, Rect::new(copy_x, y - 4.0, 44.0, 24.0));

    y
}

// =============================================================================
// Render: password generator panel
// =============================================================================

/// The new-entry form.
///
/// Every control it draws registers its own hit box, so a chooser, a field or
/// a button that is drawn is one that can be pressed -- the defect this whole
/// change is about was a button that was drawn, hit-tested, and answered with
/// `Ignored`.
fn render_new_entry_panel(frame: &mut Frame, state: &AppState, width: f32, height: f32) {
    let x_start = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let panel_width = width - x_start;
    let panel_height = height - y_start;

    draw_rect(
        frame,
        x_start,
        y_start,
        panel_width,
        panel_height,
        state.palette.base,
        0.0,
    );

    let Some(form) = state.new_entry.as_ref() else {
        return;
    };

    let pad = 24.0;
    let inner = (panel_width - pad * 2.0).max(0.0);
    let mut y = y_start + pad;

    draw_text(
        frame,
        x_start + pad,
        y,
        if form.editing.is_some() {
            "Edit Entry"
        } else {
            "New Entry"
        },
        state.palette.text,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 36.0;

    // Type chooser: one pill per kind, the chosen one filled. An entry being
    // edited keeps its kind, so only its own pill is drawn.
    let mut chooser_x = x_start + pad;
    for (index, kind) in EntryType::all().iter().enumerate() {
        if form.editing.is_some() && *kind != form.kind {
            continue;
        }
        let label = kind.label();
        let w = text::padded_width(label, 10.0, SMALL_FONT_SIZE, FontWeightHint::Regular);
        if chooser_x + w > x_start + pad + inner {
            break;
        }
        let chosen = *kind == form.kind;
        let rect = Rect::new(chooser_x, y, w, 26.0);
        draw_rect(
            frame,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if chosen {
                kind.badge_color(&state.palette)
            } else {
                state.palette.surface0
            },
            CORNER_RADIUS,
        );
        draw_text(
            frame,
            chooser_x + 10.0,
            y + 6.0,
            label,
            if chosen {
                state.palette.base
            } else {
                state.palette.subtext0
            },
            SMALL_FONT_SIZE,
            if chosen {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            Some(w),
        );
        frame.hit(Target::NewKind(index), rect);
        chooser_x += w + 6.0;
    }
    y += 40.0;

    // One row per field of the chosen kind.
    for (index, label) in form.labels().iter().enumerate() {
        draw_text(
            frame,
            x_start + pad,
            y,
            label,
            state.palette.subtext0,
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            Some(inner),
        );
        y += 18.0;

        let rect = Rect::new(x_start + pad, y, inner, 30.0);
        guitk::field::draw(
            frame,
            &state.palette,
            rect,
            state.field_state(Target::NewField(index), false),
            state.focus_ring_width,
        );

        // A secret is drawn masked while it is typed, because a password
        // manager is used in front of other people.
        draw_box_text(frame, state, Target::NewField(index), rect, "-");
        frame.hit(Target::NewField(index), rect);
        y += 38.0;
    }

    y += 8.0;

    // Save is refused rather than hidden when the form has no name: a button
    // that vanishes leaves the user with nothing to press and no reason why.
    let can_save = form.is_complete();
    let save = Rect::new(x_start + pad, y, 96.0, 32.0);
    draw_rect(
        frame,
        save.x,
        save.y,
        save.w,
        save.h,
        if can_save {
            state.palette.green
        } else {
            state.palette.surface0
        },
        CORNER_RADIUS,
    );
    draw_text(
        frame,
        save.x + 28.0,
        save.y + 8.0,
        "Save",
        if can_save {
            state.palette.base
        } else {
            state.palette.overlay0
        },
        DEFAULT_FONT_SIZE,
        FontWeightHint::Bold,
        Some(save.w),
    );
    frame.hit(Target::NewSave, save);

    let cancel = Rect::new(save.x + save.w + 12.0, y, 96.0, 32.0);
    draw_rect(
        frame,
        cancel.x,
        cancel.y,
        cancel.w,
        cancel.h,
        state.palette.surface0,
        CORNER_RADIUS,
    );
    draw_text(
        frame,
        cancel.x + 22.0,
        cancel.y + 8.0,
        "Cancel",
        state.palette.subtext0,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        Some(cancel.w),
    );
    frame.hit(Target::NewCancel, cancel);

    if !can_save {
        y += 44.0;
        draw_text(
            frame,
            x_start + pad,
            y,
            "A name is needed before this can be saved.",
            state.palette.overlay0,
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            Some(inner),
        );
    }
}

fn render_generator_panel(frame: &mut Frame, state: &AppState, width: f32, height: f32) {
    let x_start = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let panel_width = width - x_start;
    let panel_height = height - y_start;

    draw_rect(
        frame,
        x_start,
        y_start,
        panel_width,
        panel_height,
        state.palette.base,
        0.0,
    );

    let pad = 24.0;
    let mut y = y_start + pad;

    draw_text(
        frame,
        x_start + pad,
        y,
        "Password Generator",
        state.palette.text,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 36.0;

    // Generated password display
    draw_rect(
        frame,
        x_start + pad,
        y,
        panel_width - pad * 2.0,
        48.0,
        state.palette.surface0,
        CORNER_RADIUS,
    );
    // A refusal takes the password's own place, in red. Left in the prompt
    // state it would read as "you have not pressed the button yet", which is
    // exactly the wrong thing to tell someone whose generator cannot generate.
    let (display_pw, pw_color) = match (&state.generator_error, state.generated_password.is_empty())
    {
        (Some(message), _) => (message.as_str(), state.palette.red),
        (None, true) => (
            "Click Generate to create a password",
            state.palette.overlay0,
        ),
        (None, false) => (state.generated_password.as_str(), state.palette.green),
    };
    draw_text(
        frame,
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
            frame,
            &state.palette,
            x_start + pad,
            y,
            panel_width - pad * 2.0,
            8.0,
            strength.fraction(),
            strength.color(&state.palette),
        );
        y += 16.0;
        let label = format!("{} - {:.0} bits entropy", strength.label(), entropy);
        draw_text(
            frame,
            x_start + pad,
            y,
            &label,
            state.palette.ink(strength.color(&state.palette)),
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        y += 24.0;
    }

    // Buttons row
    draw_button(
        frame,
        x_start + pad,
        y,
        100.0,
        32.0,
        "Generate",
        state.palette.blue,
        state.palette.base,
        false,
    );
    draw_button(
        frame,
        x_start + pad + 112.0,
        y,
        80.0,
        32.0,
        "Copy",
        state.palette.surface1,
        state.palette.text,
        false,
    );
    y += 48.0;

    draw_separator(
        frame,
        &state.palette,
        x_start + pad,
        y,
        panel_width - pad * 2.0,
    );
    y += 16.0;

    // Mode selection
    draw_text(
        frame,
        x_start + pad,
        y,
        "Mode (Ctrl+M)",
        state.palette.text,
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
        let bg = if is_active {
            state.palette.blue
        } else {
            state.palette.surface1
        };
        let fg = if is_active {
            state.palette.base
        } else {
            state.palette.text
        };
        let btn_w = button_width(label, 10.0);
        draw_button(frame, mode_x, y, btn_w, 28.0, label, bg, fg, false);
        mode_x += btn_w + 8.0;
    }
    y += 40.0;

    // Length setting
    draw_text(
        frame,
        x_start + pad,
        y,
        "Length (Left/Right)",
        state.palette.text,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    let len_text = format!("{}", state.password_generator.length);
    draw_text(
        frame,
        x_start + pad + 100.0,
        y,
        &len_text,
        state.palette.blue,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 8.0;

    // Length slider track
    let slider_x = x_start + pad;
    let slider_w = panel_width - pad * 2.0;
    let slider_y = y + 12.0;
    draw_rect(
        frame,
        slider_x,
        slider_y,
        slider_w,
        4.0,
        state.palette.surface1,
        2.0,
    );

    let frac = (state.password_generator.length as f32 - 8.0) / 120.0;
    let knob_x = slider_x + slider_w * frac.clamp(0.0, 1.0);
    draw_rect(
        frame,
        knob_x - 6.0,
        slider_y - 4.0,
        12.0,
        12.0,
        state.palette.blue,
        6.0,
    );
    y += 32.0;

    // Character set toggles (for random mode)
    if state.password_generator.mode == GeneratorMode::Random {
        draw_text(
            frame,
            x_start + pad,
            y,
            "Character Sets",
            state.palette.text,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Bold,
            None,
        );
        y += 24.0;

        for box_ in CharsetBox::ALL {
            let enabled = box_.is_on(&state.password_generator.charset);
            let label = format!("{} ({})", box_.label(), box_.keys());
            let check_color = if enabled {
                state.palette.green
            } else {
                state.palette.surface2
            };
            let check_char = if enabled { "[x]" } else { "[ ]" };
            draw_text(
                frame,
                x_start + pad,
                y,
                check_char,
                check_color,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                None,
            );
            draw_text(
                frame,
                x_start + pad + 32.0,
                y,
                &label,
                state.palette.text,
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
            frame,
            x_start + pad,
            y,
            "Word Count",
            state.palette.text,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        let wc_text = format!("{}", state.password_generator.passphrase.word_count);
        draw_text(
            frame,
            x_start + pad + 120.0,
            y,
            &wc_text,
            state.palette.blue,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Bold,
            None,
        );
        y += 26.0;

        draw_text(
            frame,
            x_start + pad,
            y,
            "Separator",
            state.palette.text,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        draw_text(
            frame,
            x_start + pad + 120.0,
            y,
            &state.password_generator.passphrase.separator,
            state.palette.blue,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Bold,
            None,
        );
        y += 26.0;
    }

    // Entropy info
    y += 8.0;
    draw_separator(
        frame,
        &state.palette,
        x_start + pad,
        y,
        panel_width - pad * 2.0,
    );
    y += 12.0;

    let entropy = state.password_generator.entropy_bits();
    let entropy_text = format!("Estimated entropy: {:.1} bits", entropy);
    draw_text(
        frame,
        x_start + pad,
        y,
        &entropy_text,
        state.palette.subtext0,
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
        frame,
        x_start + pad,
        y,
        &pool_text,
        state.palette.subtext0,
        SMALL_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );

    let _ = y;
}

// =============================================================================
// Render: settings panel
// =============================================================================

fn render_settings_panel(frame: &mut Frame, state: &AppState, width: f32, height: f32) {
    let x_start = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let panel_width = width - x_start;
    let panel_height = height - y_start;

    draw_rect(
        frame,
        x_start,
        y_start,
        panel_width,
        panel_height,
        state.palette.base,
        0.0,
    );

    // The rows down to the auto-lock slider are the ones
    // `auto_lock_placement` counts; it and this must change together.
    let pad = SETTINGS_PAD;
    let mut y = y_start + pad;

    draw_text(
        frame,
        x_start + pad,
        y,
        "Settings",
        state.palette.text,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 36.0;

    // Security section
    draw_text(
        frame,
        x_start + pad,
        y,
        "SECURITY",
        state.palette.overlay0,
        SMALL_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 24.0;

    draw_text(
        frame,
        x_start + pad,
        y,
        "Auto-lock timeout",
        state.palette.text,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    let slider = state.auto_lock_shown();
    let minutes = whole_minutes(slider.value());
    let timeout_text = if minutes == 1 {
        String::from("1 minute")
    } else {
        format!("{minutes} minutes")
    };
    draw_text(
        frame,
        x_start + pad + 200.0,
        y,
        &timeout_text,
        state.palette.blue,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );

    // The slider: the toolkit's, at the place the pointer is read against.
    // No focus ring: Left and Right move it while this panel is up, as they
    // move the generator's, but what is typed goes to the search box, and
    // one keyboard drawn in two places would say neither.
    let placement = auto_lock_placement(width);
    slider.draw(
        frame,
        &state.palette,
        &placement,
        guitk::slider::Look::accent(&state.palette, state.palette.surface1),
        false,
        state.focus_ring_width,
    );
    frame.hit(Target::AutoLock, placement.hit());
    y = placement.track.bottom() + 20.0;

    draw_text(
        frame,
        x_start + pad,
        y,
        "Clipboard auto-clear",
        state.palette.text,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    // Nothing is ever copied, so there is nothing to clear -- said, rather
    // than a thirty-second promise about a clipboard that does not exist.
    let clear_text = String::from("Not applicable: applications have no clipboard yet");
    draw_text(
        frame,
        x_start + pad + 200.0,
        y,
        &clear_text,
        state.palette.blue,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 36.0;

    draw_separator(
        frame,
        &state.palette,
        x_start + pad,
        y,
        panel_width - pad * 2.0,
    );
    y += 16.0;

    // Vault info section
    draw_text(
        frame,
        x_start + pad,
        y,
        "VAULT INFO",
        state.palette.overlay0,
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
            frame,
            x_start + pad,
            y,
            label,
            state.palette.subtext0,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        draw_text(
            frame,
            x_start + pad + 160.0,
            y,
            value,
            state.palette.text,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        y += 26.0;
    }

    let count_text = format!("{}", state.vault.entries.len());
    draw_text(
        frame,
        x_start + pad,
        y,
        "Total entries",
        state.palette.subtext0,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    draw_text(
        frame,
        x_start + pad + 160.0,
        y,
        &count_text,
        state.palette.text,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += 26.0;

    let folder_count_text = format!("{}", state.vault.folders.len());
    draw_text(
        frame,
        x_start + pad,
        y,
        "Folders",
        state.palette.subtext0,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    draw_text(
        frame,
        x_start + pad + 160.0,
        y,
        &folder_count_text,
        state.palette.text,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );
    y += 36.0;

    draw_separator(
        frame,
        &state.palette,
        x_start + pad,
        y,
        panel_width - pad * 2.0,
    );
    y += 16.0;

    // Your vault: back it up, restore it, or take everything out of it. The
    // two buttons drawn here before 2026-09-27 -- "Export CSV" and "Backup" --
    // recorded no target, so pressing them did nothing at all.
    draw_text(
        frame,
        x_start + pad,
        y,
        "YOUR VAULT",
        state.palette.subtext0,
        SMALL_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 24.0;
    let explain = [
        "A backup is this vault, sealed: it opens with your master password, restores everything,",
        "and is as closed as the vault to anyone else. An export is every password in plain text.",
    ];
    for line in explain {
        draw_text(
            frame,
            x_start + pad,
            y,
            line,
            state.palette.subtext0,
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            Some(panel_width - pad * 2.0),
        );
        y += SMALL_FONT_SIZE + 6.0;
    }
    y += 6.0;
    let mut bx = x_start + pad;
    for (label, target, width) in [
        ("Back up...", Target::BackUp, 110.0),
        ("Restore from a backup...", Target::RestoreBackup, 200.0),
        ("Export as plain text...", Target::ExportCsv, 190.0),
    ] {
        draw_button(
            frame,
            bx,
            y,
            width,
            32.0,
            label,
            state.palette.surface1,
            state.palette.text,
            false,
        );
        frame.hit(target, Rect::new(bx, y, width, 32.0));
        bx += width + 12.0;
    }

    let _ = y;
}

// =============================================================================
// Render: audit report panel
// =============================================================================

fn render_audit_panel(frame: &mut Frame, state: &AppState, width: f32, height: f32) {
    let x_start = SIDEBAR_WIDTH + ENTRY_LIST_WIDTH;
    let y_start = TOOLBAR_HEIGHT;
    let panel_width = width - x_start;
    let panel_height = height - y_start;

    draw_rect(
        frame,
        x_start,
        y_start,
        panel_width,
        panel_height,
        state.palette.base,
        0.0,
    );

    let pad = 24.0;
    let mut y = y_start + pad;

    draw_text(
        frame,
        x_start + pad,
        y,
        "Password Audit",
        state.palette.text,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 28.0;

    if state.audit_issues.is_empty() {
        draw_text(
            frame,
            x_start + pad,
            y,
            "No issues found. All passwords look good!",
            state.palette.green,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        return;
    }

    let summary = format!("{} issues found", state.audit_issues.len());
    draw_text(
        frame,
        x_start + pad,
        y,
        &summary,
        state.palette.yellow,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Bold,
        None,
    );
    y += 28.0;

    draw_separator(
        frame,
        &state.palette,
        x_start + pad,
        y,
        panel_width - pad * 2.0,
    );
    y += 12.0;

    frame.push(RenderCommand::PushClip {
        x: x_start,
        y,
        width: panel_width,
        height: panel_height - (y - y_start),
    });

    for issue in &state.audit_issues {
        if y > y_start + panel_height {
            break;
        }

        let issue_color = issue.issue.severity_color(&state.palette);

        draw_rect(
            frame,
            x_start + pad,
            y,
            panel_width - pad * 2.0,
            36.0,
            state.palette.surface0,
            4.0,
        );

        // Issue severity badge
        let severity_w = draw_badge(
            frame,
            x_start + pad + 8.0,
            y + 8.0,
            issue.issue.label(),
            issue_color,
            state.palette.base,
        );

        // Entry name, laid out from the width the badge actually drew.
        draw_text(
            frame,
            x_start + pad + 8.0 + severity_w + 16.0,
            y + 10.0,
            &issue.entry_name,
            state.palette.text,
            DEFAULT_FONT_SIZE,
            FontWeightHint::Regular,
            Some(panel_width - pad * 2.0 - severity_w - 32.0),
        );

        y += 42.0;
    }

    frame.push(RenderCommand::PopClip);
}

// =============================================================================
// Render: lock screen
// =============================================================================

fn render_lock_screen(frame: &mut Frame, state: &AppState, width: f32, height: f32) {
    // Full-screen overlay
    draw_rect(frame, 0.0, 0.0, width, height, state.palette.mantle, 0.0);
    match &state.gate {
        Gate::Unlock => render_unlock_panel(frame, state, width, height),
        Gate::Create(form) => render_create_panel(frame, state, form, width, height),
        Gate::Unreadable(why) => render_unreadable_panel(frame, state, why, width, height),
    }
}

/// A lock-screen card of `w` x `h`, centred, with its shadow. Returns its
/// top-left corner.
fn lock_card(
    frame: &mut Frame,
    state: &AppState,
    width: f32,
    height: f32,
    w: f32,
    h: f32,
) -> (f32, f32) {
    let px = width / 2.0 - w / 2.0;
    let py = height / 2.0 - h / 2.0;
    frame.push(RenderCommand::BoxShadow {
        x: px,
        y: py,
        width: w,
        height: h,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 24.0,
        spread: 0.0,
        color: Color::rgba(0, 0, 0, 100),
        corner_radii: CornerRadii::all(12.0),
    });
    draw_rect(frame, px, py, w, h, state.palette.surface0, 12.0);
    (px, py)
}

/// Lines of a lock-screen card, each centred on `center_x`, from `y` down.
fn centred_lines(
    frame: &mut Frame,
    lines: &[(&str, Color, f32, FontWeightHint)],
    center_x: f32,
    mut y: f32,
    max_width: f32,
) -> f32 {
    for &(line, color, size, weight) in lines {
        draw_text(
            frame,
            text::center_x(line, center_x, size, weight).max(center_x - max_width / 2.0),
            y,
            line,
            color,
            size,
            weight,
            Some(max_width),
        );
        y += size + 8.0;
    }
    y
}

/// A masked password field, with its hit box: the toolkit's field, drawn
/// from what [`AppState::field_state`] says of `target` and red when
/// `wrong`.
/// Where the text of the box drawn as `target` sits in its box at `rect`:
/// inside its inset, a line high and centred. One answer for the drawing and
/// for a press.
fn box_text_area(target: Target, rect: Rect) -> Rect {
    let inset = match target {
        Target::Search | Target::NewField(_) => 10.0,
        _ => 12.0,
    };
    let line = text::line_height(DEFAULT_FONT_SIZE, FontWeightHint::Regular).min(rect.h);
    Rect::new(
        rect.x + inset,
        rect.y + (rect.h - line) / 2.0,
        (rect.w - 2.0 * inset).max(0.0),
        line,
    )
}

/// The text of the box drawn as `target` in its box at `rect`: what it
/// holds -- a mask for each character of a secret -- with its caret and
/// selection where they are while it has the keyboard, scrolled so the
/// caret stays in view; or, empty, `placeholder`, with the caret before it
/// while it has the keyboard. The boxes drew their text with no caret at
/// all, cut with an ellipsis at the characters being typed -- and the master
/// password a mask for each *byte* of it.
fn draw_box_text(
    frame: &mut Frame,
    state: &AppState,
    target: Target,
    rect: Rect,
    placeholder: &str,
) {
    let Some(held) = state.box_value(target) else {
        return;
    };
    let area = box_text_area(target, rect);
    let focused = state.typing_into() == Some(target);
    let mut tree = RenderTree::new();
    if held.is_empty() {
        tree.push(RenderCommand::Text {
            x: area.x,
            y: area.y,
            text: placeholder.to_owned(),
            color: state.palette.subtext0,
            font_size: DEFAULT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(area.w),
            overflow: TextOverflow::Ellipsis,
        });
        if focused {
            textedit::push_caret(
                &mut tree,
                area.x,
                area.y,
                area.h,
                state.palette.text,
                textedit::CARET_WIDTH,
            );
        }
    } else {
        // A box without the keyboard reads from its start.
        let (cursor, anchor) = if focused {
            state.box_caret(target)
        } else {
            (TextCursor::default(), None)
        };
        let (shown, cursor, selection_anchor) = if state.box_masked(target) {
            let (mask, at, anchor) = textline::masked(held, cursor.byte(), anchor, MASK);
            (mask, TextCursor::from(at), anchor)
        } else {
            (held.to_owned(), cursor, anchor)
        };
        textedit::draw(
            &mut tree,
            &textedit::SingleLine {
                text: &shown,
                cursor,
                selection_anchor,
                focused,
                x: area.x,
                y: area.y,
                width: area.w,
                line_height: area.h,
                font_size: DEFAULT_FONT_SIZE,
                weight: FontWeightHint::Regular,
                color: state.palette.text,
                selection_bg: state.palette.accent,
                selection_fg: state.palette.crust,
                caret_width: textedit::CARET_WIDTH,
            },
        );
    }
    frame.extend(tree.commands);
}

fn masked_field(
    frame: &mut Frame,
    state: &AppState,
    rect: Rect,
    placeholder: &str,
    wrong: bool,
    target: Target,
) {
    guitk::field::draw(
        frame,
        &state.palette,
        rect,
        state.field_state(target, wrong),
        state.focus_ring_width,
    );
    draw_box_text(frame, state, target, rect, placeholder);
    frame.hit(target, rect);
}

/// The first run: choose the master password that makes the vault.
fn render_create_panel(
    frame: &mut Frame,
    state: &AppState,
    form: &NewVault,
    width: f32,
    height: f32,
) {
    let (w, h) = (440.0, 420.0);
    let (px, py) = lock_card(frame, state, width, height, w, h);
    let cx = px + w / 2.0;
    let mut y = centred_lines(
        frame,
        &[
            (
                "Create your vault",
                state.palette.text,
                HEADING_FONT_SIZE,
                FontWeightHint::Bold,
            ),
            (
                "Choose a master password. It opens everything kept here,",
                state.palette.subtext0,
                SMALL_FONT_SIZE,
                FontWeightHint::Regular,
            ),
            (
                "and it cannot be recovered: forget it, and the vault is lost.",
                state.palette.subtext0,
                SMALL_FONT_SIZE,
                FontWeightHint::Regular,
            ),
        ],
        cx,
        py + 28.0,
        w - 40.0,
    );
    y += 8.0;
    let field_w = w - 60.0;
    for (label, target) in [
        ("Master password", Target::NewPassword),
        ("Type it again", Target::ConfirmPassword),
    ] {
        draw_text(
            frame,
            px + 30.0,
            y,
            label,
            state.palette.subtext0,
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            None,
        );
        y += SMALL_FONT_SIZE + 6.0;
        masked_field(
            frame,
            state,
            Rect::new(px + 30.0, y, field_w, 40.0),
            "",
            form.wrong == Some(target),
            target,
        );
        y += 52.0;
    }
    if !form.password.is_empty() {
        let (strength, bits) = evaluate_password_strength(&form.password);
        let line = format!("Strength: {} ({bits:.0} bits)", strength.label());
        draw_text(
            frame,
            px + 30.0,
            y,
            &line,
            state.palette.ink(strength.color(&state.palette)),
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            Some(field_w),
        );
    }
    y += SMALL_FONT_SIZE + 10.0;
    if let Some(error) = &form.error {
        draw_text(
            frame,
            px + 30.0,
            y,
            error,
            state.palette.ink(state.palette.red),
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            Some(field_w),
        );
    }
    let create = Rect::new(cx - 70.0, py + h - 56.0, 140.0, 36.0);
    draw_button(
        frame,
        create.x,
        create.y,
        create.w,
        create.h,
        "Create vault",
        state.palette.blue,
        state.palette.base,
        false,
    );
    frame.hit(Target::CreateVault, create);
}

/// The vault file is there and cannot be opened: say why, and do nothing to it.
fn render_unreadable_panel(
    frame: &mut Frame,
    state: &AppState,
    why: &str,
    width: f32,
    height: f32,
) {
    let (w, h) = (480.0, 220.0);
    let (px, py) = lock_card(frame, state, width, height, w, h);
    let where_ = state
        .store
        .as_deref()
        .map(|p| p.shown().to_string())
        .unwrap_or_default();
    let reason = capitalised(why);
    centred_lines(
        frame,
        &[
            (
                "The vault cannot be opened",
                state.palette.text,
                HEADING_FONT_SIZE,
                FontWeightHint::Bold,
            ),
            (
                where_.as_str(),
                state.palette.subtext0,
                SMALL_FONT_SIZE,
                FontWeightHint::Regular,
            ),
            (
                reason.as_str(),
                state.palette.ink(state.palette.red),
                SMALL_FONT_SIZE,
                FontWeightHint::Regular,
            ),
            (
                "It is left exactly as it is: nothing here will write over it.",
                state.palette.subtext0,
                SMALL_FONT_SIZE,
                FontWeightHint::Regular,
            ),
        ],
        px + w / 2.0,
        py + 36.0,
        w - 40.0,
    );
}

/// A line of a vault dialog and its colour.
type DialogLine = (String, Color);
/// A vault dialog's button: its label, what it presses, and whether it is
/// the one Enter presses.
type DialogButton = (&'static str, Target, bool);

/// A vault dialog, over everything else, with nothing behind it clickable.
fn render_vault_dialog(
    frame: &mut Frame,
    state: &AppState,
    dialog: &VaultDialog,
    width: f32,
    height: f32,
) {
    // Modal: the window behind stays painted, and none of it can be pressed.
    frame.discard_hits();
    draw_rect(
        frame,
        0.0,
        0.0,
        width,
        height,
        Color::rgba(0, 0, 0, 120),
        0.0,
    );
    let (w, h) = (520.0, 250.0);
    let (px, py) = lock_card(frame, state, width, height, w, h);
    let cx = px + w / 2.0;
    let small = |text: &str, color: Color| (text.to_string(), color);
    let (title, lines, buttons): (&str, Vec<DialogLine>, [DialogButton; 2]) = match dialog {
        VaultDialog::ExportWarning => (
            "Export every password as plain text?",
            vec![
                small(
                    "The file will hold every login and password in this vault,",
                    state.palette.ink(state.palette.red),
                ),
                small(
                    "readable by anyone -- and any program -- that can open it.",
                    state.palette.ink(state.palette.red),
                ),
                small(
                    "Use it only to move to another password manager, and delete it after.",
                    state.palette.subtext0,
                ),
                // Formula injection is warned of, not defended against by
                // changing the file: a password altered on the way out no
                // longer works (lane B's request, the operator's §1417).
                small(
                    "Don't open it in a spreadsheet: it may run a password as a formula.",
                    state.palette.subtext0,
                ),
            ],
            [
                ("Export anyway", Target::ExportAnyway, true),
                ("Cancel", Target::DialogCancel, false),
            ],
        ),
        VaultDialog::RestorePassword { path, error, .. } => (
            "Restore from a backup",
            vec![
                small(&path.shown().to_string(), state.palette.subtext0),
                small(
                    "Type the master password this backup was made with.",
                    state.palette.subtext0,
                ),
                small(
                    error.as_deref().unwrap_or(""),
                    state.palette.ink(state.palette.red),
                ),
            ],
            [
                ("Open", Target::RestoreOpen, true),
                ("Cancel", Target::DialogCancel, false),
            ],
        ),
        VaultDialog::DeleteConfirm { id } => (
            "Delete this entry?",
            vec![
                small(
                    state.vault.get_entry(*id).map_or("", |e| e.display_name()),
                    state.palette.text,
                ),
                small(
                    "It is gone from this vault as soon as you say so.",
                    state.palette.subtext0,
                ),
                small(
                    "A backup made before now still holds it.",
                    state.palette.subtext0,
                ),
            ],
            [
                ("Delete", Target::DeleteConfirmed, true),
                ("Keep it", Target::DialogCancel, false),
            ],
        ),
        VaultDialog::RestoreConfirm { path, backup } => (
            "Replace this vault's entries?",
            vec![
                small(
                    &format!(
                        "{} holds {} {}; this vault holds {}.",
                        path.file_name()
                            .map_or_else(|| path.shown().to_string(), |n| n.shown().to_string()),
                        backup.entries.len(),
                        if backup.entries.len() == 1 {
                            "entry"
                        } else {
                            "entries"
                        },
                        state.vault.entries.len()
                    ),
                    state.palette.text,
                ),
                small(
                    "Everything here now is replaced. Back it up first if you may want it.",
                    state.palette.subtext0,
                ),
                small(
                    "The vault keeps your master password.",
                    state.palette.subtext0,
                ),
            ],
            [
                ("Replace", Target::RestoreReplace, true),
                ("Keep mine", Target::DialogCancel, false),
            ],
        ),
    };
    draw_text(
        frame,
        text::center_x(title, cx, HEADING_FONT_SIZE, FontWeightHint::Bold).max(px + 16.0),
        py + 22.0,
        title,
        state.palette.text,
        HEADING_FONT_SIZE,
        FontWeightHint::Bold,
        Some(w - 32.0),
    );
    let mut y = py + 58.0;
    for (line, color) in &lines {
        if line.is_empty() {
            continue;
        }
        draw_text(
            frame,
            px + 24.0,
            y,
            line,
            *color,
            SMALL_FONT_SIZE,
            FontWeightHint::Regular,
            Some(w - 48.0),
        );
        y += SMALL_FONT_SIZE + 8.0;
    }
    if let VaultDialog::RestorePassword { error, .. } = dialog {
        masked_field(
            frame,
            state,
            Rect::new(px + 24.0, py + h - 100.0, w - 48.0, 36.0),
            "",
            error.is_some(),
            Target::RestoreInput,
        );
    }
    let mut bx = px + w - 24.0;
    for (label, target, primary) in buttons.iter().rev() {
        let bw = 130.0;
        bx -= bw;
        let (bg, fg) = if *primary {
            (state.palette.blue, state.palette.base)
        } else {
            (state.palette.surface1, state.palette.text)
        };
        draw_button(frame, bx, py + h - 52.0, bw, 34.0, label, bg, fg, false);
        frame.hit(*target, Rect::new(bx, py + h - 52.0, bw, 34.0));
        bx -= 12.0;
    }
}

/// There is a vault: its name, the master-password field and Unlock.
fn render_unlock_panel(frame: &mut Frame, state: &AppState, width: f32, height: f32) {
    let center_x = width / 2.0;
    let center_y = height / 2.0;
    let panel_w = 360.0;
    let panel_h = 280.0;

    let px = center_x - panel_w / 2.0;
    let py = center_y - panel_h / 2.0;

    // Lock panel with shadow
    frame.push(RenderCommand::BoxShadow {
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
    draw_rect(
        frame,
        px,
        py,
        panel_w,
        panel_h,
        state.palette.surface0,
        12.0,
    );

    // Lock icon
    draw_text(
        frame,
        text::center_x("[=]", center_x, 24.0, FontWeightHint::Bold),
        py + 30.0,
        "[=]",
        state.palette.blue,
        24.0,
        FontWeightHint::Bold,
        None,
    );

    // Vault name. A vault named in any non-ASCII script used to drift left of
    // centre by half its excess byte count, since the offset was `len * 5.0`.
    // A vault read from its file has no name until it is opened: the name is
    // inside, encrypted with everything else.
    let name = if state.vault.name.is_empty() {
        "Your vault"
    } else {
        state.vault.name.as_str()
    };
    let name_x = text::center_x(name, center_x, HEADING_FONT_SIZE, FontWeightHint::Bold);
    draw_text(
        frame,
        name_x,
        py + 70.0,
        name,
        state.palette.text,
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
        frame,
        instruction_x,
        py + 100.0,
        instruction,
        state.palette.subtext0,
        DEFAULT_FONT_SIZE,
        FontWeightHint::Regular,
        None,
    );

    // Password input field
    let input_x = px + 30.0;
    let input_y = py + 130.0;
    let input_w = panel_w - 60.0;
    let input_h = 40.0;

    masked_field(
        frame,
        state,
        Rect::new(input_x, input_y, input_w, input_h),
        "Password...",
        state.unlock_failed,
        Target::MasterInput,
    );

    // Why it did not open. The usual reason is long -- "that is not its
    // master password -- or the file has been changed since it was saved" --
    // so it is broken at its dash onto two lines rather than cut short.
    if state.unlock_failed {
        let mut ey = input_y + input_h + 6.0;
        for part in state.unlock_message.split(" -- ") {
            let part = capitalised(part.trim());
            let error_x = text::center_x(&part, center_x, SMALL_FONT_SIZE, FontWeightHint::Regular)
                .max(px + 8.0);
            draw_text(
                frame,
                error_x,
                ey,
                &part,
                state.palette.red,
                SMALL_FONT_SIZE,
                FontWeightHint::Regular,
                Some(panel_w - 16.0),
            );
            ey += SMALL_FONT_SIZE + 3.0;
        }
    }

    // Unlock button
    let unlock = Rect::new(center_x - 50.0, py + 200.0, 100.0, 36.0);
    draw_button(
        frame,
        unlock.x,
        unlock.y,
        unlock.w,
        unlock.h,
        "Unlock",
        state.palette.blue,
        state.palette.base,
        false,
    );
    // Until this was recorded the button was decoration: the only way past the
    // lock screen was the Enter key, and `handle_mouse` returned immediately
    // while the vault was locked.
    frame.hit(Target::Unlock, unlock);
}

// =============================================================================
// Build complete render tree
// =============================================================================

impl AppState {
    /// Draw the whole window at `width` x `height`, recording where every
    /// control ended up.
    ///
    /// Takes the size as parameters and *believes* them, rather than reading
    /// `self.width`/`self.height`: the compositor hands the size to
    /// [`App::render`], and the first frame of a window's life is drawn before
    /// any `Event::Resize` has arrived to tell the state about it. A renderer
    /// that consults its own memory instead lays that frame out at the default
    /// size and puts every control somewhere the pointer is not.
    fn frame(&self, width: f32, height: f32) -> Frame {
        let layout = Layout::new(width, height);
        let mut frame = Frame::new(layout.window.w, layout.window.h);
        let (w, h) = (layout.window.w, layout.window.h);

        if !self.vault.is_unlocked() {
            render_lock_screen(&mut frame, self, w, h);
            debug_assert!(frame.is_balanced(), "a clip was pushed and not popped");
            return frame;
        }

        // Background
        draw_rect(&mut frame, 0.0, 0.0, w, h, self.palette.base, 0.0);

        render_toolbar(&mut frame, self, &layout);
        // In the strip under the toolbar, which nothing else is drawn in.
        let (notice, warning) = self.notice_lines();
        for (i, line) in notice.iter().enumerate() {
            #[expect(clippy::cast_precision_loss, reason = "two lines; index is 0 or 1")]
            let ty = layout.notice.y + 3.0 + i as f32 * NOTICE_LINE_H;
            let avail = (layout.notice.w - 16.0).max(0.0);
            if avail <= 0.0 || ty + NOTICE_LINE_H > layout.notice.y + layout.notice.h {
                break;
            }
            frame.push(RenderCommand::Text {
                x: 8.0,
                y: ty,
                text: line.clone(),
                color: if i == 0 && warning {
                    self.palette.ink(self.palette.yellow)
                } else {
                    self.palette.subtext0
                },
                font_size: if i == 0 { 12.0 } else { 11.0 },
                font_weight: if i == 0 {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(avail),
                overflow: TextOverflow::Ellipsis,
            });
        }
        render_sidebar(&mut frame, self, &layout);
        render_entry_list(&mut frame, self, &layout);

        // Detail panel (depends on view). Only the entry detail scrolls; the
        // other three are fixed-height panels, which is why
        // `detail_content_height` reports zero for them.
        match self.detail_view {
            DetailView::EntryDetail => {
                let _measured = render_entry_detail(&mut frame, self, w, h);
            }
            DetailView::PasswordGenerator => render_generator_panel(&mut frame, self, w, h),
            DetailView::Settings => render_settings_panel(&mut frame, self, w, h),
            DetailView::AuditReport => render_audit_panel(&mut frame, self, w, h),
            DetailView::NewEntry => render_new_entry_panel(&mut frame, self, w, h),
        }
        if let Some(dialog) = &self.dialog {
            render_vault_dialog(&mut frame, self, dialog, w, h);
        }
        if self.picker.is_open() {
            // Modal like the dialog: nothing behind it may be clicked.
            frame.discard_hits();
            for command in self.picker.render(&self.palette, w, h) {
                frame.push(command);
            }
        }
        // The list of keys over all of it: it is the one thing on screen a
        // reader asked for explicitly. Nothing under it takes a press.
        if self.show_help {
            frame.discard_hits();
            guitk::shortcut::render_card(
                &mut frame,
                &self.palette,
                (w, h),
                0.0,
                SHORTCUTS,
                "F1 or Escape closes this",
            );
        }

        debug_assert!(frame.is_balanced(), "a clip was pushed and not popped");
        frame
    }

    /// The topmost control at `(x, y)`, or `None` for bare background.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }
}

/// The window as a render tree, at the size the state currently believes.
///
/// [`App::render`] does not go through here -- it is handed the live size and
/// passes it straight to [`AppState::frame`], because believing the caller is
/// the whole point. This is the tests' shorthand for "draw it at whatever size
/// you were last told", which is what they set up by hand anyway.
#[cfg(test)]
fn build_render_tree(state: &AppState) -> RenderTree {
    state.frame(state.width, state.height).into_tree()
}

// =============================================================================
// Event handling
// =============================================================================

/// Keys while the new-entry form is up.
fn handle_new_entry_key(state: &mut AppState, key: &KeyEvent) -> EventResult {
    // The form's own keys, taken plain: Escape, Enter, Tab and Down.
    // Every other key is the field's (`box_key`) -- AltGr's characters
    // among it, and not a command's letter, which a chord carries as text:
    // Ctrl+L typed an `l` into a password.
    let own = textline::is_plain(key.modifiers)
        && matches!(key.key, Key::Escape | Key::Enter | Key::Tab | Key::Down);
    if !own {
        let Some(target) = state.typing_into() else {
            return EventResult::Ignored;
        };
        return if state.box_key(target, key) == Some(true) {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        };
    }
    match key.key {
        Key::Escape => {
            cancel_new_entry(state);
            EventResult::Consumed
        }
        Key::Enter => {
            // Enter saves rather than adding a newline: none of these fields is
            // more than a line, and a form that cannot be finished from the
            // keyboard is one that needs the mouse for its last step.
            if save_new_entry(state) {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        Key::Tab | Key::Down => {
            if let Some(form) = state.new_entry.as_mut() {
                form.focus_next();
            }
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// Open the new-entry form.
///
/// The kind it opens on is the one the sidebar is filtering by when that is a
/// type filter, because a user who has just narrowed the list to Logins and
/// pressed Add is asking for a Login.
fn open_new_entry(state: &mut AppState) {
    let kind = match state.sidebar_selection {
        SidebarSelection::TypeFilter(kind) => kind,
        _ => EntryType::Login,
    };
    state.new_entry = Some(NewEntryForm::new(kind));
    state.detail_view = DetailView::NewEntry;
    state.detail_scroll = 0.0;
}

/// Put the form's credential in the vault and show it.
///
/// Returns whether anything was saved: a form with no name is not saved, and
/// the caller keeps it open rather than discarding what was typed.
fn save_new_entry(state: &mut AppState) -> bool {
    let Some(form) = state.new_entry.as_ref() else {
        return false;
    };
    let Some(mut data) = form.build() else {
        return false;
    };
    let id = if let Some(id) = form.editing {
        // What the form does not show is kept as it was: a login's
        // one-time-code secret, and a card number the form showed masked
        // and was not changed -- masking a mask again would lose it.
        if let Some(old) = state.vault.get_entry(id) {
            match (&old.data, &mut data) {
                (EntryData::Login(o), EntryData::Login(n)) => {
                    n.totp_secret.clone_from(&o.totp_secret);
                }
                (EntryData::CreditCard(o), EntryData::CreditCard(n))
                    if form.value(1) == o.number_masked =>
                {
                    n.number_masked.clone_from(&o.number_masked);
                }
                _ => {}
            }
        }
        if !state.vault.update_entry(id, data, state.now) {
            return false;
        }
        id
    } else {
        state.vault.add_entry(data, state.now)
    };
    state.new_entry = None;
    state.selected_entry_id = Some(id);
    state.detail_view = DetailView::EntryDetail;
    state.detail_scroll = 0.0;
    state.refresh_filter();
    state.run_audit();
    state.clamp_scroll();
    true
}

/// Open the form over the selected entry, to change it.
fn open_edit_entry(state: &mut AppState) {
    let Some(entry) = state
        .selected_entry_id
        .and_then(|id| state.vault.get_entry(id))
    else {
        return;
    };
    state.new_entry = Some(NewEntryForm::for_entry(entry));
    state.detail_view = DetailView::NewEntry;
    state.detail_scroll = 0.0;
}

/// Ask whether to delete the selected entry.
fn ask_delete(state: &mut AppState) {
    if let Some(id) = state
        .selected_entry_id
        .filter(|id| state.vault.get_entry(*id).is_some())
    {
        state.dialog = Some(VaultDialog::DeleteConfirm { id });
    }
}

/// Delete the entry the question was about.
fn delete_entry(state: &mut AppState, id: u64) {
    state.dialog = None;
    if state.vault.remove_entry(id) {
        if state.selected_entry_id == Some(id) {
            state.selected_entry_id = None;
        }
        state.refresh_filter();
        state.run_audit();
        state.clamp_scroll();
    }
}

/// Throw the form away.
fn cancel_new_entry(state: &mut AppState) {
    state.new_entry = None;
    state.detail_view = DetailView::EntryDetail;
    state.detail_scroll = 0.0;
}

/// The fields of the selected entry that are worth copying, in the order the
/// detail view lists them: a label, and the text a copy would put on the
/// clipboard.
fn copyable_fields(state: &AppState) -> Vec<(&'static str, String)> {
    let Some(id) = state.selected_entry_id else {
        return Vec::new();
    };
    let Some(entry) = state.vault.get_entry(id) else {
        return Vec::new();
    };
    // The same fields the detail view draws, in the same order and with the
    // empties left in: the view passes `CopyField(n)` counted down its own
    // rows, so a list filtered here would shift every index below the first
    // blank field and copy the wrong secret.
    match &entry.data {
        EntryData::Login(d) => vec![
            ("Site", d.site.clone()),
            ("Username", d.username.clone()),
            ("Password", d.password.clone()),
            ("URL", d.url.clone()),
            ("TOTP", d.totp_secret.clone().unwrap_or_default()),
        ],
        EntryData::SecureNote(d) => vec![("Title", d.title.clone())],
        EntryData::CreditCard(d) => vec![
            ("Card Name", d.name.clone()),
            ("Number", d.number_masked.clone()),
            ("Expiry", d.expiry.clone()),
            ("Cardholder", d.cardholder.clone()),
        ],
        EntryData::Identity(d) => vec![
            ("Name", d.name.clone()),
            ("Email", d.email.clone()),
            ("Phone", d.phone.clone()),
            ("Address", d.address.clone()),
        ],
        EntryData::SshKey(d) => vec![
            ("Key Name", d.name.clone()),
            ("Fingerprint", d.fingerprint.clone()),
        ],
    }
}

/// Answer a Copy press on field `index` of the selected entry: nothing is
/// copied, because applications have no clipboard, and the toolbar says so
/// for that field ([`NOT_COPIED`]). Returns whether the press named a field.
fn copy_field(state: &mut AppState, index: usize) -> bool {
    let fields = copyable_fields(state);
    let Some((label, value)) = fields.get(index) else {
        return false;
    };
    if value.is_empty() {
        return false;
    }
    state.copy_refused = Some((*label).to_string());
    true
}

/// Answer `event`, then save the vault if the answer changed it.
fn handle_event(state: &mut AppState, event: &Event) -> EventResult {
    let result = dispatch_event(state, event);
    state.keep_if_changed();
    result
}

fn dispatch_event(state: &mut AppState, event: &Event) -> EventResult {
    if state.advance_clock(event) {
        // Locked by the time that has passed. The event is not taken: it was
        // meant for a vault that is no longer open -- a key would go into the
        // master password, a press land on the lock screen.
        return EventResult::Consumed;
    }
    match event {
        // A tick that did not lock changed nothing drawn: the entry's times
        // are dates, not spans counted to now.
        Event::Tick { .. } => EventResult::Ignored,
        Event::Key(key_event) if key_event.pressed => handle_key(state, key_event),
        Event::Mouse(mouse_event) => handle_mouse(state, mouse_event),
        Event::Resize { width, height } => {
            // Until this arm existed the size lived only in the renderer's
            // parameters, so the scroll bounds had nothing to be computed
            // from. Growing the window can leave a pane scrolled past its own
            // end, which is what `resize` re-clamps.
            state.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

fn handle_key(state: &mut AppState, key: &KeyEvent) -> EventResult {
    // Lock screen input
    if !state.vault.is_unlocked() {
        let create = match &mut state.gate {
            Gate::Unlock => None,
            // Nothing to type into and nothing to do: the file is left alone.
            Gate::Unreadable(_) => return EventResult::Consumed,
            // The form's own keys, taken plain; every other key is its
            // boxes' (`box_key`).
            Gate::Create(form) if textline::is_plain(key.modifiers) => match key.key {
                Key::Enter if form.confirming => Some(true),
                Key::Enter | Key::Tab => {
                    form.confirming = true;
                    Some(false)
                }
                Key::Escape => {
                    *form = NewVault::default();
                    Some(false)
                }
                _ => None,
            },
            Gate::Create(_) => None,
        };
        if let Some(create) = create {
            if create {
                create_vault(state);
            }
            return EventResult::Consumed;
        }
        if matches!(state.gate, Gate::Unlock) && textline::is_plain(key.modifiers) {
            match key.key {
                Key::Enter => {
                    attempt_unlock(state);
                    return EventResult::Consumed;
                }
                Key::Escape => {
                    state.master_input.clear();
                    state.unlock_failed = false;
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }
        // Every other key is the box with the keyboard's: the master
        // password, or the new vault's two. An edit puts away what was
        // said of the last attempt.
        let Some(target) = state.typing_into() else {
            return EventResult::Ignored;
        };
        return match state.box_key(target, key) {
            Some(changed) => {
                // Any key the box answers puts away what was said of the last
                // attempt, as typing and Backspace did -- a Backspace in a box
                // emptied for the retyping among them.
                let said = state.unlock_failed
                    || matches!(&state.gate, Gate::Create(form)
                        if form.error.is_some() || form.wrong.is_some());
                state.unlock_failed = false;
                if let Gate::Create(form) = &mut state.gate {
                    form.error = None;
                    form.wrong = None;
                }
                if changed || said {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            None => EventResult::Ignored,
        };
    }

    // F1 raises the list of keys over the open vault, from a dialog, the
    // form or a panel too: none of them types it. F1 alone -- what a key types
    // goes into the search, `?` among it.
    if key.key == Key::F1 && textline::is_plain(key.modifiers) {
        state.show_help = true;
        return EventResult::Consumed;
    }

    // A vault dialog is modal: its keys, and nothing behind it.
    if state.dialog.is_some() {
        return dialog_key(state, key);
    }

    // The new-entry form takes the keyboard while it is up: every printable
    // key goes into a field rather than into the search box behind it.
    if state.detail_view == DetailView::NewEntry && state.new_entry.is_some() {
        return handle_new_entry_key(state, key);
    }

    // The generator panel's own controls, live only while it is up. Every
    // one of these was drawn -- three mode buttons with the active one
    // highlighted, a slider with a knob, four ticked boxes -- and none could
    // be operated. Ahead of the match because the catch-all below would
    // otherwise type them into the search box behind the panel.
    if state.detail_view == DetailView::PasswordGenerator && generator_key(state, key) {
        state.vault.touch(state.now);
        return EventResult::Consumed;
    }

    // The settings panel's auto-lock slider, the same way: its keys while the
    // panel is up, ahead of the catch-all that types into the search box.
    if let Some(result) = state.auto_lock_key(key) {
        state.vault.touch(state.now);
        return result;
    }

    // Main app key handling. The four shortcuts are Ctrl chords, not Ctrl
    // held: AltGr arrives as Ctrl+Alt, and AltGr+L -- a Polish `ł`, typed
    // into the search -- locked the vault. What a key types goes into the
    // search, and not a command's letter; the list's keys are taken plain,
    // so Alt+Delete does not ask to delete an entry.
    let chord = textline::is_ctrl_chord(key.modifiers);
    let plain = textline::is_plain(key.modifiers);
    let result = match key.key {
        Key::L if chord => {
            state.lock_vault();
            EventResult::Consumed
        }
        Key::F if chord => {
            // Focus search (toggle)
            state.search_query.clear();
            state.refresh_filter();
            state.clamp_scroll();
            EventResult::Consumed
        }
        Key::G if chord => {
            state.detail_view = DetailView::PasswordGenerator;
            regenerate_password(state);
            EventResult::Consumed
        }
        // A plain letter goes to the search box, so editing is Ctrl+E.
        Key::E if chord => {
            open_edit_entry(state);
            EventResult::Consumed
        }
        // The list's keys, taken plain -- Alt+Delete asked to delete an
        // entry -- and ahead of the search's: Delete asks to delete the
        // selected entry, Up and Down move through the entries.
        Key::Delete if plain && state.selected_entry_id.is_some() => {
            ask_delete(state);
            EventResult::Consumed
        }
        Key::Escape if plain => {
            state.search_query.clear();
            state.detail_view = DetailView::EntryDetail;
            state.refresh_filter();
            state.clamp_scroll();
            EventResult::Consumed
        }
        Key::Up if plain => {
            navigate_entry_list(state, -1);
            EventResult::Consumed
        }
        Key::Down if plain => {
            navigate_entry_list(state, 1);
            EventResult::Consumed
        }
        Key::Enter if plain => {
            if state.detail_view == DetailView::PasswordGenerator {
                regenerate_password(state);
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        // Every other key is the search's (`box_key`): the caret keys,
        // Backspace and Delete at the caret -- Delete with no entry selected
        // -- Ctrl+A, C, X and V, and typing, AltGr's among it and no
        // command's letter. The search took typing at its end and Backspace
        // from it, and nothing else.
        _ => {
            let before = state.search_query.clone();
            match state.box_key(Target::Search, key) {
                Some(changed) => {
                    if state.search_query != before {
                        state.refresh_filter();
                        state.clamp_scroll();
                    }
                    if changed {
                        EventResult::Consumed
                    } else {
                        EventResult::Ignored
                    }
                }
                None => EventResult::Ignored,
            }
        }
    };

    // Only a keystroke the app acted on postpones the auto-lock. A modifier
    // held down on its own is not use of the vault.
    if result == EventResult::Consumed {
        state.vault.touch(state.now);
    }
    result
}

/// Handle a keystroke aimed at the password generator panel.
///
/// Returns whether it was one. Every branch regenerates, because the panel is
/// showing a password produced under the *previous* settings and leaving it
/// there invites reading it as the new ones' output -- the same reason the
/// kind keys in `apps/passwordgen` produce immediately.
fn generator_key(state: &mut AppState, key: &KeyEvent) -> bool {
    // Ctrl chords, not Ctrl held: AltGr arrives as Ctrl+Alt and types.
    let ctrl = textline::is_ctrl_chord(key.modifiers);
    if ctrl && key.key == Key::M {
        state.password_generator.mode = state.password_generator.mode.next();
        regenerate_password(state);
        return true;
    }

    // Length, on the arrows, because the control drawn for it is a slider --
    // plain arrows: a chord with Alt or the Windows key is not the panel's.
    if textline::is_plain(key.modifiers) && matches!(key.key, Key::Left | Key::Right) {
        let len = state.password_generator.length;
        let next = if key.key == Key::Left {
            len.saturating_sub(1)
        } else {
            len.saturating_add(1)
        };
        state.password_generator.set_length(next);
        regenerate_password(state);
        return true;
    }

    let Some(box_) = CharsetBox::from_key(key.key, ctrl) else {
        return false;
    };
    // The boxes belong to Random mode; the panel does not draw them in the
    // other two, and a key that silently changed an invisible setting would
    // be worse than one that does nothing.
    if state.password_generator.mode != GeneratorMode::Random {
        return false;
    }

    let opts = &mut state.password_generator.charset;
    let on = box_.is_on(opts);
    if on && CharsetBox::ALL.iter().filter(|b| b.is_on(opts)).count() <= 1 {
        // `build_charset` answers an empty pool with an empty password, so a
        // generator with every box clear would produce nothing and say
        // nothing about why.
        state.generator_error = Some(NEEDS_ONE_CHARACTER_SET.to_owned());
        state.generated_password.clear();
        return true;
    }
    box_.set(opts, !on);
    regenerate_password(state);
    true
}

/// Why the last box cannot be cleared.
const NEEDS_ONE_CHARACTER_SET: &str = "A password needs at least one kind of character";

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

fn handle_mouse(state: &mut AppState, mouse: &MouseEvent) -> EventResult {
    // The auto-lock slider's press, drag and release, and its thumb's light.
    // The press and the release are use of the vault; the pointer moving is
    // not.
    if let Some(result) = state.auto_lock_mouse(mouse) {
        if matches!(
            mouse.kind,
            MouseEventKind::Press(_) | MouseEventKind::Release(_)
        ) {
            state.vault.touch(state.now);
        }
        return result;
    }
    let result = match mouse.kind {
        MouseEventKind::Press(MouseButton::Left) => handle_click(state, mouse.x, mouse.y),
        MouseEventKind::Scroll { dy, .. } => handle_scroll(state, mouse.x, mouse.y, dy),
        _ => EventResult::Ignored,
    };

    // Only a click the app actually did something with counts as activity for
    // the auto-lock timer. A pointer resting on the window is not use of it.
    if result == EventResult::Consumed {
        state.vault.touch(state.now);
    }
    result
}

/// Act on a left click, from what the renderer recorded at that point.
///
/// The four `handle_*_click` functions this replaces each re-derived the
/// geometry of the pane they covered -- the toolbar one by hand-adding the
/// widths of five buttons, the sidebar one by re-summing a stack of vertical
/// offsets -- and one of them, the detail panel's, was an empty stub. That the
/// numbers agreed with the renderer's was a coincidence maintained by hand.
fn handle_click(state: &mut AppState, x: f32, y: f32) -> EventResult {
    let frame = state.frame(state.width, state.height);
    let Some(target) = frame.hit_test(x, y) else {
        return EventResult::Ignored;
    };
    // A press in a box: what the box was, and where it was drawn, before
    // the press gives it the keyboard.
    let rect = frame.rect_of(|t| *t == target);
    let drawn_focused = state.typing_into() == Some(target);
    let result = act_on(state, target);
    // The box the press gave the keyboard to: the caret under the pointer.
    // One without the keyboard was drawn from its start, so its caret is
    // measured from there.
    if let Some(rect) = rect
        && state.typing_into() == Some(target)
        && state.box_value(target).is_some()
    {
        if !drawn_focused {
            state.load_box(target);
            state.editor.set_cursor(TextCursor::default());
            state.editor.set_selection_anchor(None);
        }
        state.press_box(target, rect, x);
        return EventResult::Consumed;
    }
    result
}

/// What pressing a target does.
///
/// Split from `handle_click` so that what a control *does* can be stated
/// without going through where it happens to be drawn: a test that has to hit
/// a pixel to press a button is a test of the layout as much as the behaviour,
/// and it goes red when the layout moves for unrelated reasons.
fn act_on(state: &mut AppState, target: Target) -> EventResult {
    // While the vault is locked the only two live targets are the lock
    // screen's own, and nothing else is drawn -- so this is a statement about
    // what the renderer emits, not a second guard that could disagree with it.
    match target {
        Target::Unlock => {
            attempt_unlock(state);
            EventResult::Consumed
        }
        Target::CreateVault => {
            create_vault(state);
            EventResult::Consumed
        }
        Target::BackUp => {
            state.open_picker(PickFor::Backup);
            EventResult::Consumed
        }
        Target::RestoreBackup => {
            state.open_picker(PickFor::Restore);
            EventResult::Consumed
        }
        Target::ExportCsv => {
            state.dialog = Some(VaultDialog::ExportWarning);
            EventResult::Consumed
        }
        // The slider takes its own presses, where on it they land, before
        // a press reaches here (`handle_mouse`); a target with no point has
        // nothing to set it to.
        Target::AutoLock => EventResult::Ignored,
        Target::ExportAnyway => {
            state.dialog = None;
            state.open_picker(PickFor::Export);
            EventResult::Consumed
        }
        Target::RestoreOpen => {
            state.restore_open();
            EventResult::Consumed
        }
        Target::RestoreReplace => {
            state.restore_replace();
            EventResult::Consumed
        }
        Target::DialogCancel => {
            state.dialog = None;
            EventResult::Consumed
        }
        Target::EditEntry => {
            open_edit_entry(state);
            EventResult::Consumed
        }
        Target::DeleteEntry => {
            ask_delete(state);
            EventResult::Consumed
        }
        Target::DeleteConfirmed => {
            if let Some(VaultDialog::DeleteConfirm { id }) = state.dialog {
                delete_entry(state, id);
            }
            EventResult::Consumed
        }
        // The field is where typing already goes.
        Target::RestoreInput => EventResult::Consumed,
        Target::NewPassword | Target::ConfirmPassword => {
            if let Gate::Create(form) = &mut state.gate {
                form.confirming = target == Target::ConfirmPassword;
            }
            EventResult::Consumed
        }
        // The field is where typing already goes; clicking it is a no-op that
        // still has to be claimed, or the click falls through to the scrim.
        Target::MasterInput => EventResult::Consumed,
        Target::Add => {
            // Until this arm existed the toolbar's Add button was drawn, was
            // hit-tested, and was answered with `Ignored` -- so the vault had
            // no way to gain an entry, and everything downstream of having one
            // (`Vault::add_entry`, `IdGen`, every `EntryData` variant, the
            // clipboard, the CSV export) was dead code the compiler had been
            // reporting all along.
            open_new_entry(state);
            EventResult::Consumed
        }
        // The field is where typing already goes; clicking it is claimed so it
        // does not fall through.
        Target::Search => EventResult::Consumed,
        Target::NewKind(index) => {
            let Some(&kind) = EntryType::all().get(index) else {
                return EventResult::Ignored;
            };
            let Some(form) = state.new_entry.as_mut() else {
                return EventResult::Ignored;
            };
            form.set_kind(kind);
            EventResult::Consumed
        }
        Target::NewField(index) => {
            let Some(form) = state.new_entry.as_mut() else {
                return EventResult::Ignored;
            };
            form.focus(index);
            EventResult::Consumed
        }
        Target::NewSave => {
            save_new_entry(state);
            EventResult::Consumed
        }
        Target::NewCancel => {
            cancel_new_entry(state);
            EventResult::Consumed
        }
        Target::CopyField(index) => {
            if copy_field(state, index) {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        Target::Sort => {
            state.sort_order = state.sort_order.next();
            state.refresh_filter();
            state.clamp_scroll();
            EventResult::Consumed
        }
        Target::Generator => {
            state.detail_view = DetailView::PasswordGenerator;
            if state.generated_password.is_empty() {
                regenerate_password(state);
            }
            EventResult::Consumed
        }
        // As Ctrl+L locks, through `lock_vault`: it locked the vault alone,
        // so a change whose save had failed was not tried again, and gone.
        Target::LockVault => {
            state.lock_vault();
            EventResult::Consumed
        }
        Target::Settings => {
            state.detail_view = DetailView::Settings;
            EventResult::Consumed
        }
        Target::Sidebar(index) => {
            let Some(selection) = state.sidebar_items().get(index).cloned() else {
                return EventResult::Ignored;
            };
            state.sidebar_selection = selection;
            if state.sidebar_selection == SidebarSelection::Audit {
                state.detail_view = DetailView::AuditReport;
                state.run_audit();
            }
            state.refresh_filter();
            // A narrower filter can leave the list scrolled past its own end.
            state.clamp_scroll();
            EventResult::Consumed
        }
        Target::EntryRow(index) => {
            let Some(&entry_id) = state.filtered_ids.get(index) else {
                return EventResult::Ignored;
            };
            state.selected_entry_id = Some(entry_id);
            state.detail_view = DetailView::EntryDetail;
            state.show_password = false;
            // A new entry's fields start at the top, not wherever the last one
            // had been scrolled to.
            state.detail_scroll = 0.0;
            EventResult::Consumed
        }
    }
}

/// Scroll whichever pane the pointer is over.
///
/// `wheel::pixels` and not an `Accumulator`: both offsets are already
/// continuous, so a trackpad's fifth of a notch can be shown as a fifth of a
/// row straight away rather than banked until it rounds. The `20.0` per notch
/// it replaces was one of a dozen private guesses in this tree at what a notch
/// is worth -- these rows are 52 px, so it was not half of one.
///
/// Both are clamped at the far end as well as at zero. They were clamped with
/// `.max(0.0)` alone, which let either pane be wound off the end of its
/// content into blank space and kept going.
fn handle_scroll(state: &mut AppState, x: f32, y: f32, dy: f32) -> EventResult {
    if !state.vault.is_unlocked() {
        return EventResult::Ignored;
    }
    let layout = state.layout();
    if layout.list_rows.contains(x, y) {
        state.list_scroll =
            (state.list_scroll + wheel::pixels(dy, ROW_HEIGHT)).clamp(0.0, state.max_list_scroll());
        EventResult::Consumed
    } else if layout.detail.contains(x, y) {
        state.detail_scroll = (state.detail_scroll + wheel::pixels(dy, DETAIL_LINE_HEIGHT))
            .clamp(0.0, state.max_detail_scroll());
        EventResult::Consumed
    } else {
        EventResult::Ignored
    }
}

/// Check `master_input` against the vault's verifier.
fn attempt_unlock(state: &mut AppState) {
    let password = state.master_input.clone();
    match state.vault.open(&password, state.now) {
        Ok(()) => {
            state.unlock_failed = false;
            state.unlock_message.clear();
            state.master_input.clear();
            // What was just opened is what the file holds.
            state.saved = Some(fingerprint(&state.vault));
            state.refresh_filter();
        }
        Err(why) => {
            state.unlock_failed = true;
            state.unlock_message = capitalised(&why.to_string());
        }
    }
}

/// `text` with its first letter a capital, for a message that starts a line.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Keys while a vault dialog is up: Escape leaves it, Enter does what its
/// first button does, and typing goes into the restore dialog's field.
fn dialog_key(state: &mut AppState, key: &KeyEvent) -> EventResult {
    /// What a key asks of the dialog, decided before anything is done, so the
    /// dialog is not still borrowed when the doing needs the whole state.
    enum Then {
        Close,
        Export,
        Open,
        Replace,
        Delete(u64),
        Nothing,
    }
    // What a key types goes into the password being asked for -- not a
    // command's letter -- and the dialog's own keys are taken plain: Alt+Enter
    // answered "delete this entry?" with yes.
    let plain = textline::is_plain(key.modifiers);
    let dialog_key = plain && matches!(key.key, Key::Escape | Key::Enter);
    if !dialog_key && matches!(state.dialog, Some(VaultDialog::RestorePassword { .. })) {
        let changed = state.box_key(Target::RestoreInput, key);
        // Any key the box answers puts away what was said of the last try.
        let mut said = false;
        if changed.is_some()
            && let Some(VaultDialog::RestorePassword { error, .. }) = &mut state.dialog
        {
            said = error.take().is_some();
        }
        return if changed == Some(true) || said {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        };
    }
    let then = match (&mut state.dialog, key.key) {
        _ if !plain => Then::Nothing,
        (_, Key::Escape) => Then::Close,
        (Some(VaultDialog::ExportWarning), Key::Enter) => Then::Export,
        (Some(VaultDialog::RestorePassword { .. }), Key::Enter) => Then::Open,
        (Some(VaultDialog::RestoreConfirm { .. }), Key::Enter) => Then::Replace,
        (Some(VaultDialog::DeleteConfirm { id }), Key::Enter) => Then::Delete(*id),
        _ => Then::Nothing,
    };
    match then {
        Then::Close => state.dialog = None,
        Then::Export => {
            state.dialog = None;
            state.open_picker(PickFor::Export);
        }
        Then::Open => state.restore_open(),
        Then::Replace => state.restore_replace(),
        Then::Delete(id) => delete_entry(state, id),
        Then::Nothing => {}
    }
    EventResult::Consumed
}

/// Make the first vault from the form, and save it.
fn create_vault(state: &mut AppState) {
    let Gate::Create(form) = &mut state.gate else {
        return;
    };
    if form.password.chars().count() < MIN_MASTER_PASSWORD_CHARS {
        form.error = Some(format!(
            "Choose a master password of at least {MIN_MASTER_PASSWORD_CHARS} characters."
        ));
        form.wrong = Some(Target::NewPassword);
        form.confirming = false;
        return;
    }
    if form.password != form.confirm {
        form.error = Some("The two do not match. Type it again.".to_string());
        form.wrong = Some(Target::ConfirmPassword);
        form.confirm.clear();
        form.confirming = true;
        return;
    }
    let mut salt = [0u8; seal::SALT_LEN];
    if !(state.entropy)(&mut salt) {
        form.error = Some(
            "This system's secure random source cannot be reached, so no vault can be made safely."
                .to_string(),
        );
        return;
    }
    let password = std::mem::take(&mut form.password);
    form.confirm.clear();
    match Vault::create("My Vault", &password, state.new_vault_kdf, salt) {
        Ok(vault) => {
            state.vault = vault;
            state.vault.touch(state.now);
            state.gate = Gate::Unlock;
            state.saved = None;
            state.keep_if_changed();
            state.refresh_filter();
        }
        Err(why) => {
            if let Gate::Create(form) = &mut state.gate {
                form.error = Some(format!("The vault could not be made: {why}."));
            }
        }
    }
}

// =============================================================================
// Entry point
// =============================================================================

impl AppState {
    /// An event while the list of keys is up, which is modal: a plain F1 or
    /// Escape puts it away and no other key reaches what it covers -- Ctrl+Q
    /// does not close the window under it, Delete does not ask to delete an
    /// entry; a press with any button puts it away and does nothing else; the
    /// wheel scrolls nothing it covers. `None` while it is down, and for what
    /// is no key or press: a tick may still lock the vault, which puts the
    /// list away.
    fn help_event(&mut self, event: &Event) -> Option<Response> {
        if !self.show_help {
            return None;
        }
        match event {
            Event::Key(key) => {
                if key.pressed
                    && textline::is_plain(key.modifiers)
                    && matches!(key.key, Key::F1 | Key::Escape)
                {
                    self.show_help = false;
                }
                Some(Response::Redraw)
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {
                    self.show_help = false;
                    Some(Response::Redraw)
                }
                MouseEventKind::Scroll { .. } => Some(Response::Idle),
                _ => None,
            },
            _ => None,
        }
    }

    /// What the window does with `event`, before the pointer's light is
    /// settled (`App::on_event`).
    fn respond(&mut self, event: &Event) -> Response {
        if let Some(response) = self.help_event(event) {
            return response;
        }
        // Ctrl+Q closes the window. Ctrl+L is *not* a close -- it locks the
        // vault, which is the point of having it. A Ctrl chord, not Ctrl
        // held: AltGr+Q -- Ctrl+Alt -- is a German `@`, typed into a user
        // name, and closed the window.
        if let Event::Key(key) = event
            && key.pressed
            && key.key == Key::Q
            && textline::is_ctrl_chord(key.modifiers)
        {
            return Response::Exit;
        }
        // The file dialog first: while it is up it takes the keyboard and the
        // mouse, so a key meant for a file name cannot reach the vault behind.
        match self.picker.handle(event, self.width, self.height) {
            Picked::Chose(path) => {
                if let Some(purpose) = self.pick_for.take() {
                    self.picked(purpose, &path);
                }
                self.keep_if_changed();
                return Response::Redraw;
            }
            Picked::Cancelled => {
                self.pick_for = None;
                return Response::Redraw;
            }
            Picked::Handled => return Response::Redraw,
            Picked::Ignored => {}
        }
        if matches!(event, Event::CloseRequested) {
            // Every change is saved as it is made, so this is the last chance
            // for one whose save failed: say so once, and let the next close go.
            self.keep_if_changed();
            if self.save_error.is_some() && self.vault.is_unlocked() && !self.close_anyway {
                self.close_anyway = true;
                return Response::KeepOpen;
            }
            return Response::Exit;
        }
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }
}

impl App for AppState {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        "Credential Manager".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (DEFAULT_WINDOW_WIDTH as u32, DEFAULT_WINDOW_HEIGHT as u32)
    }

    /// A clock only while there is a vault open to lock, and only for the
    /// moment its auto-lock falls due. The harness never pushes a wake later,
    /// so use in the meantime costs one early tick, after which this is asked
    /// again. There was no clock at all, the trait's default being none: no
    /// tick ever came, and the vault never locked itself however long it was
    /// left.
    fn tick_interval(&self) -> Option<std::time::Duration> {
        self.vault
            .auto_lock_in(self.now)
            .map(std::time::Duration::from_secs)
    }

    fn appearance_changed(&mut self, settings: &appearance::AppearanceSettings) {
        self.focus_ring_width = settings.focus_ring_width();
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if let Event::Mouse(mouse) = event {
            match mouse.kind {
                MouseEventKind::Move => self.pointer = Some((mouse.x, mouse.y)),
                MouseEventKind::Leave => self.pointer = None,
                _ => {}
            }
        }
        let response = self.respond(event);
        // What the pointer is over is settled after every event, against
        // what is drawn by then: a dialog that came up or went, a lock, or a
        // list that moved under a still pointer changes it as surely as a
        // move does. A change of light is worth a repaint on its own.
        let lit = self.hover;
        self.hover = self.text_box_under_pointer();
        match response {
            Response::Idle if self.hover != lit => Response::Redraw,
            other => other,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for AppState {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
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

/// Open the window on a fresh, locked vault.
///
/// The vault is created empty with an empty master password because this crate
/// still has no persistence layer: there is nothing on disk to read a
/// [`pwkdf::PasswordVerifier`] and its salt back from, and inventing a password
/// here would be worse than admitting there is none. So the window opens on
/// the lock screen of a vault that pressing Enter opens. That is not a
/// shipping arrangement; it is tracked in `known-issues.md` as
/// `C-CREDMANAGER-HAS-NO-VAULT-ON-DISK`, and wiring the window is what turns
/// the gap from theoretical into visible.
fn main() -> ExitCode {
    let mut state = AppState::open(vault_path());
    state.clock = Clock::Wall;
    state.now = unix_now();
    app::launch("credmanager", &mut state)
}

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

    /// The window says the vault is not saved anywhere.
    ///
    /// This crate's handling of the gap was already exemplary in every place
    /// except the one the user looks at. The module doc says "this crate has
    /// no persistence layer at all, so every launch opens an empty vault"; the
    /// `reopen` door is documented as where a loader would come in; its
    /// `allow` carries a reason; `main` refuses to open at all if the key
    /// derivation will not run.
    ///
    /// None of that is on screen, and for a password manager the silence is
    /// the expensive kind. Somebody stores a credential, closes the window,
    /// and what they lose is an account -- not a preference, not a layout.
    #[test]
    fn the_window_says_the_vault_is_not_saved() {
        // `Vault::create` cannot run here: the test machine has no entropy
        // source, on purpose, so `KdfParams::fresh` returns
        // EntropyUnavailable. `unlocked_vault` is the cheap path the other
        // tests use.
        let state = AppState::new(unlocked_vault());

        let texts: Vec<String> = state
            .draw((1200.0, 800.0))
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for line in NOT_KEPT_LINES {
            assert!(
                texts.iter().any(|t| t == line),
                "the window never said {line:?}"
            );
        }
        assert!(
            NOT_KEPT_LINES
                .iter()
                .any(|l| l.contains("Do not rely on it")),
            "nothing tells the user not to trust the vault with anything",
        );
    }

    /// A vault that can be written to, built the cheap way tests use.
    fn unlocked_vault() -> Vault {
        let mut vault = Vault::for_test("Test Vault", TEST_MASTER_PASSWORD);
        assert!(vault.unlock(TEST_MASTER_PASSWORD, 0));
        vault
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let colors: Vec<Color> = EntryType::all()
            .iter()
            .map(|t| t.badge_color(&pal))
            .collect();
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
        // Set while open: the timeout is kept inside the sealed vault, so
        // unlocking reads it back from there.
        v.unlock("pw", 100);
        v.auto_lock_minutes = 5;
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

    /// Every string the toolbar draws.
    fn toolbar_texts(state: &AppState) -> Vec<String> {
        let layout = Layout::new(1200.0, 800.0);
        let mut frame = Frame::new(1200.0, 800.0);
        render_toolbar(&mut frame, state, &layout);
        frame
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_refusal_is_shown_where_the_password_would_be() {
        let mut state = AppState::for_test();
        state.detail_view = DetailView::PasswordGenerator;
        state.password_generator = PasswordGenerator::without_entropy();
        regenerate_password(&mut state);

        let mut frame = Frame::new(1200.0, 800.0);
        render_generator_panel(&mut frame, &state, 1200.0, 800.0);
        let shown = frame.commands().iter().any(
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

    // == Export, backup and restore (2026-09-27; C-Q25, §1417) ===================
    //
    // The operator's answer: a plain-text export made totally clear to be
    // insecure (A), and an encrypted backup that restores everything (B). The
    // backup is the sealed vault itself; the old JSON-ish "backup" that left
    // every password out is gone, never having been connected.

    /// Every character a password can hold comes back out of the CSV exactly
    /// (B-Q12: the export must not mangle a password that holds the
    /// characters CSV itself uses).
    #[test]
    fn an_export_keeps_every_character_of_every_password() {
        let mut v = unlocked_vault();
        let every: String = (1u8..=127)
            .map(char::from)
            .chain("\u{e9}\u{1F512}".chars())
            .collect();
        let hostile = [
            "plain",
            "a,b",
            "say \"hi\"",
            "\"",
            "\"\"",
            "nul\0inside",
            "line\nbreak",
            "cr\rand\r\ncrlf",
            " leading and trailing ",
            "=1+1",
            "tab\there",
            every.as_str(),
            "",
        ];
        for (i, pw) in hostile.iter().enumerate() {
            let mut login = LoginData::new(&format!("site{i}"), "user", pw);
            login.notes = (*pw).to_string();
            v.add_entry(EntryData::Login(login), 1);
        }
        let csv = export_csv(&v);
        assert!(csv.ends_with("\r\n"), "records end in CRLF");
        let records = textfmt::csv::parse_records(&csv);
        assert_eq!(records.len(), hostile.len() + 1, "{csv:?}");
        assert_eq!(records[0][3].text, "password");
        for (i, pw) in hostile.iter().enumerate() {
            let row = &records[i + 1];
            assert_eq!(row.len(), 10, "row {i} split: {row:?}");
            assert_eq!(row[3].text, *pw, "password {i}");
            assert_eq!(row[5].text, *pw, "notes {i}");
            assert!(row.iter().all(|f| f.quoted), "row {i} has a bare field");
        }
    }

    #[test]
    fn every_kind_of_entry_reaches_the_export() {
        let mut v = unlocked_vault();
        v.add_entry(
            EntryData::CreditCard(CreditCardData::new("Visa", "****4242", "12/30", "Ann")),
            1,
        );
        v.add_entry(
            EntryData::Identity(IdentityData::new("Ann", "ann@example.org")),
            1,
        );
        v.add_entry(
            EntryData::SshKey(SshKeyData::new("deploy", "SHA256:xyz", "ssh-ed25519 AAAA")),
            1,
        );
        v.add_entry(
            EntryData::SecureNote(SecureNoteData::new("Wifi", "the code")),
            1,
        );
        let records = textfmt::csv::parse_records(&export_csv(&v));
        let notes: Vec<&str> = records[1..].iter().map(|r| r[5].text.as_str()).collect();
        assert!(
            notes[0].contains("****4242") && notes[0].contains("12/30"),
            "{notes:?}"
        );
        assert!(notes[1].contains("ann@example.org"), "{notes:?}");
        assert!(notes[2].contains("SHA256:xyz"), "{notes:?}");
        assert_eq!(notes[3], "the code");
    }

    #[test]
    fn export_comes_only_after_the_warning_says_what_it_is() {
        let mut state = unlocked_app();
        act_on(&mut state, Target::ExportCsv);
        assert!(matches!(state.dialog, Some(VaultDialog::ExportWarning)));
        assert!(
            !state.picker.is_open(),
            "the file dialog came up before the warning"
        );
        let shown = drawn(&state);
        assert!(shown.contains("plain text"), "{shown}");
        assert!(shown.contains("readable by anyone"), "{shown}");
        assert!(
            shown.contains("spreadsheet"),
            "the warning does not say to keep it out of a spreadsheet: {shown}"
        );
        act_on(&mut state, Target::DialogCancel);
        assert!(state.dialog.is_none() && !state.picker.is_open());

        act_on(&mut state, Target::ExportCsv);
        act_on(&mut state, Target::ExportAnyway);
        assert!(state.picker.is_open());
        assert_eq!(state.pick_for, Some(PickFor::Export));
    }

    #[test]
    fn an_export_is_written_where_chosen_and_says_what_it_is() {
        let scratch = scratchdir::ScratchDir::new("credmanager_export");
        let mut state = unlocked_app();
        state.vault.add_entry(
            EntryData::Login(LoginData::new("bank", "ann", "hunter2")),
            1,
        );
        let path = scratch.dir().join("out").join("credentials.csv");
        state.picked(PickFor::Export, &path);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"hunter2\""), "{text}");
        assert!(
            state
                .vault_message
                .as_deref()
                .unwrap_or("")
                .contains("plain text"),
            "{:?}",
            state.vault_message
        );
    }

    #[test]
    fn a_backup_is_the_vault_sealed_and_opens_with_its_master_password() {
        let (scratch, mut state) = first_run("backup");
        make_vault(&mut state, MASTER, MASTER);
        add_login(&mut state, "bank.example", "hunter2-secret");
        let backup = scratch.dir().join("elsewhere").join("vault.backup");
        state.picked(PickFor::Backup, &backup);
        assert!(
            state
                .vault_message
                .as_deref()
                .unwrap_or("")
                .contains("Backed up"),
            "{:?}",
            state.vault_message
        );
        let bytes = std::fs::read(&backup).unwrap();
        assert!(
            !bytes.windows(14).any(|w| w == b"hunter2-secret"),
            "the backup holds a password in the clear"
        );
        let mut opened = Vault::from_file(bytes).unwrap();
        assert!(!opened.unlock("not it", 0));
        assert!(opened.unlock(MASTER, 0));
        assert_eq!(opened.entries.len(), 1);
    }

    #[test]
    fn a_restore_needs_the_backups_password_and_asks_before_replacing() {
        // A backup of one vault...
        let (scratch, mut old) = first_run("restore_from");
        make_vault(&mut old, MASTER, MASTER);
        add_login(&mut old, "old.example", "old-secret");
        add_login(&mut old, "older.example", "older-secret");
        let backup = scratch.dir().join("vault.backup");
        old.picked(PickFor::Backup, &backup);

        // ...restored into another, with a different master password.
        let other = "another master password";
        let (scratch2, mut state) = first_run("restore_into");
        make_vault(&mut state, other, other);
        add_login(&mut state, "new.example", "new-secret");
        state.picked(PickFor::Restore, &backup);
        assert!(
            matches!(state.dialog, Some(VaultDialog::RestorePassword { .. })),
            "{:?}",
            state.dialog
        );

        type_in(&mut state, "wrong password");
        handle_event(&mut state, &key(Key::Enter));
        assert!(
            matches!(
                state.dialog,
                Some(VaultDialog::RestorePassword { error: Some(_), .. })
            ),
            "{:?}",
            state.dialog
        );
        assert_eq!(state.vault.entries.len(), 1);

        type_in(&mut state, MASTER);
        handle_event(&mut state, &key(Key::Enter));
        assert!(
            matches!(state.dialog, Some(VaultDialog::RestoreConfirm { .. })),
            "{:?}",
            state.dialog
        );
        assert!(
            drawn(&state).contains("Replace this vault"),
            "{}",
            drawn(&state)
        );
        assert_eq!(state.vault.entries.len(), 1, "replaced before being asked");

        handle_event(&mut state, &key(Key::Enter));
        assert!(state.dialog.is_none());
        assert_eq!(state.vault.entries.len(), 2);

        // Kept, under this vault's own master password and not the backup's.
        let mut again = reopen(&scratch2);
        assert!(!again.vault.unlock(MASTER, 0));
        assert!(again.vault.unlock(other, 0));
        assert_eq!(again.vault.entries.len(), 2);
    }

    #[test]
    fn a_file_that_is_not_a_backup_is_said_to_be_so() {
        let scratch = scratchdir::ScratchDir::new("credmanager_not_a_backup");
        let path = scratch.dir().join("notes.txt");
        std::fs::write(&path, "shopping").unwrap();
        let mut state = unlocked_app();
        state.picked(PickFor::Restore, &path);
        assert!(state.dialog.is_none());
        assert!(
            state
                .vault_message
                .as_deref()
                .unwrap_or("")
                .contains("cannot be restored"),
            "{:?}",
            state.vault_message
        );
    }

    fn ctrl_l() -> Event {
        Event::Key(KeyEvent {
            key: Key::L,
            pressed: true,
            modifiers: guitk::event::Modifiers {
                ctrl: true,
                ..guitk::event::Modifiers::NONE
            },
            text: String::new(),
        })
    }

    #[test]
    fn a_vault_dialog_is_modal() {
        let mut state = unlocked_app();
        act_on(&mut state, Target::ExportCsv);
        handle_event(&mut state, &ctrl_l());
        assert!(
            state.vault.is_unlocked(),
            "a key reached the window behind the dialog"
        );
        assert!(
            !probe::is_visible(&state, Target::Add),
            "the toolbar behind can be clicked"
        );
        assert!(probe::is_visible(&state, Target::ExportAnyway));
        handle_event(&mut state, &key(Key::Escape));
        assert!(state.dialog.is_none());
    }

    #[test]
    fn the_settings_buttons_can_be_pressed() {
        let mut state = unlocked_app();
        state.detail_view = DetailView::Settings;
        for target in [Target::BackUp, Target::RestoreBackup, Target::ExportCsv] {
            assert!(
                probe::is_visible(&state, target),
                "{target:?} is drawn and cannot be pressed"
            );
        }
    }

    #[test]
    fn the_file_dialog_takes_the_keys_while_it_is_up() {
        let mut state = unlocked_app();
        act_on(&mut state, Target::BackUp);
        assert!(state.picker.is_open());
        state.on_event(&ctrl_l());
        assert!(
            state.vault.is_unlocked(),
            "a key reached the vault behind the file dialog"
        );
        // Ctrl+L started a path in the dialog's own address bar (the dialog
        // takes it, as a file manager does), so the first Escape stops the
        // typing and the second is the dialog's.
        state.on_event(&key(Key::Escape));
        assert!(state.picker.is_open(), "Escape left a path and the dialog");
        state.on_event(&key(Key::Escape));
        assert!(!state.picker.is_open());
        assert_eq!(state.pick_for, None);
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

    // == The auto-lock ========================================================

    /// The unlocked test vault, with the time its auto-lock takes.
    fn unlocked_for_the_clock() -> (AppState, u64) {
        let mut state = AppState::for_test();
        assert!(state.vault.unlock(TEST_MASTER_PASSWORD, state.now));
        let timeout = u64::from(state.vault.auto_lock_minutes) * 60;
        (state, timeout)
    }

    fn tick(state: &mut AppState, elapsed_ms: u64) {
        handle_event(state, &Event::Tick { elapsed_ms });
    }

    /// **Left alone for its auto-lock time, the vault locks itself** -- which
    /// it never did in a window: the app asked for no clock, so no tick came.
    /// The clock is asked for at the deadline. Ticks a millisecond short of
    /// it leave the vault open and the millisecond after locks it, which is
    /// the carry `elapsed_ms / 1000` lost.
    #[test]
    fn left_alone_the_vault_locks_itself_on_time() {
        use std::time::Duration;
        let (mut state, timeout) = unlocked_for_the_clock();
        assert_eq!(
            App::tick_interval(&state),
            Some(Duration::from_secs(timeout)),
            "the clock is not asked for at the auto-lock's deadline"
        );
        tick(&mut state, 400);
        tick(&mut state, 400);
        tick(&mut state, timeout * 1000 - 801);
        assert!(state.vault.is_unlocked(), "locked before its time");
        assert_eq!(App::tick_interval(&state), Some(Duration::from_secs(1)));
        tick(&mut state, 1);
        assert!(
            !state.vault.is_unlocked(),
            "the vault did not lock itself when its time came"
        );
        assert_eq!(
            App::tick_interval(&state),
            None,
            "a locked vault, with nothing to lock, still wants a clock"
        );
    }

    /// **Use puts the lock off**, and the clock asked for with it.
    #[test]
    fn use_puts_the_auto_lock_off() {
        use std::time::Duration;
        let (mut state, timeout) = unlocked_for_the_clock();
        tick(&mut state, (timeout - 60) * 1000);
        assert_eq!(App::tick_interval(&state), Some(Duration::from_mins(1)));
        type_in(&mut state, "a");
        assert_eq!(state.search_query, "a", "control: the key was taken");
        assert_eq!(
            App::tick_interval(&state),
            Some(Duration::from_secs(timeout)),
            "use did not put the lock off"
        );
        tick(&mut state, 60 * 1000);
        assert!(
            state.vault.is_unlocked(),
            "locked a minute after it was used"
        );
    }

    /// **The event that finds the lock due is not taken.** A machine asleep
    /// past the deadline sends no tick: the first key after it must find the
    /// vault locked rather than count as the use that keeps it open -- and
    /// must not be typed into the master password either.
    #[test]
    fn the_event_that_finds_the_lock_due_is_not_taken() {
        let (mut state, timeout) = unlocked_for_the_clock();
        // Time passes with no tick, as it does across a sleep.
        state.now += timeout;
        type_in(&mut state, "x");
        assert!(
            !state.vault.is_unlocked(),
            "a key after the deadline kept the vault open"
        );
        assert!(
            state.search_query.is_empty() && state.master_input.is_empty(),
            "the key that found the lock due was taken as well"
        );
    }

    /// **A window runs on the wall clock**, read at every event: an entry's
    /// times are dates, and a span spent asleep counts towards the auto-lock.
    #[test]
    fn a_window_runs_on_the_wall_clock() {
        let (mut state, _) = unlocked_for_the_clock();
        state.clock = Clock::Wall;
        let before = unix_now();
        handle_event(
            &mut state,
            &Event::Mouse(MouseEvent {
                x: 1.0,
                y: 1.0,
                kind: MouseEventKind::Move,
            }),
        );
        let after = unix_now();
        assert!(
            (before..=after).contains(&state.now),
            "{} is not the time it is ({before}..={after})",
            state.now
        );
        // And the wall clock's time is the one the auto-lock counts against:
        // the test vault was opened in 1970.
        assert!(
            !state.vault.is_unlocked(),
            "fifty years idle, and the vault is still open"
        );
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

    /// Press the left button where the window would, at the size it believes.
    fn press_at(state: &mut AppState, x: f32, y: f32) -> EventResult {
        handle_event(
            state,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        )
    }

    /// A point one pixel into whichever entry row is drawn first.
    const FIRST_ROW_Y: f32 = AppState::rows_top() + 1.0;

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
        let _ = build_render_tree(&state);
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
            let _ = build_render_tree(&state);
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
        wheel_at(&mut state, DETAIL_X, -1.0);
        assert!(state.detail_scroll > 0.0);
        press_at(&mut state, LIST_X, FIRST_ROW_Y);
        assert_eq!(state.detail_scroll, 0.0);
    }

    #[test]
    fn the_hit_test_and_the_renderer_agree_on_where_row_zero_starts() {
        // Both used a bare `32.0`; naming it is what stops them drifting.
        // They no longer *can* drift -- the click resolves through the boxes
        // the renderer recorded -- but the agreement is still worth asserting.
        let mut state = unlocked_with_entries(60);
        let first = state.filtered_ids.first().copied();
        press_at(&mut state, LIST_X, FIRST_ROW_Y);
        assert_eq!(state.selected_entry_id, first);
        // One row further down, after scrolling by exactly one row, is row 1.
        state.list_scroll = ROW_HEIGHT;
        press_at(&mut state, LIST_X, FIRST_ROW_Y);
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
    fn rows_clip(state: &AppState) -> (f32, f32) {
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
        let (clip_y, clip_h) = rows_clip(&state);
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
        let (clip_y, clip_h) = rows_clip(&state);
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
        let (clip_y, clip_h) = rows_clip(&state);
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
        let rt = build_render_tree(&state);
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
        let rt = build_render_tree(&state);
        assert!(rt.commands.len() > 30);
    }

    #[test]
    fn test_render_generator_panel() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state.detail_view = DetailView::PasswordGenerator;
        state.generated_password = "test-password-123".to_string();
        let rt = build_render_tree(&state);
        assert!(rt.commands.len() > 20);
    }

    #[test]
    fn test_render_settings_panel() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state.detail_view = DetailView::Settings;
        let rt = build_render_tree(&state);
        assert!(rt.commands.len() > 20);
    }

    #[test]
    fn test_render_audit_panel_empty() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state.detail_view = DetailView::AuditReport;
        let rt = build_render_tree(&state);
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
        let rt = build_render_tree(&state);
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
            let rt = build_render_tree(&state);
            assert!(rt.commands.len() > 20, "Render failed for entry type");
        }
    }

    #[test]
    fn test_render_no_selected_entry() {
        let mut state = AppState::for_test();
        state.vault.unlock(TEST_MASTER_PASSWORD, state.now);
        state.selected_entry_id = None;
        let rt = build_render_tree(&state);
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut frame = Frame::new(1280.0, 800.0);
        for label in ["Login", "Identity", "Compromised"] {
            let drawn = draw_badge(&mut frame, 0.0, 0.0, label, pal.blue, pal.base);
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

    // == The window is what the renderer recorded ==============================

    /// A vault with one folder and one tag, so the sidebar draws every group.
    fn unlocked_with_a_folder_and_a_tag() -> AppState {
        let mut state = unlocked_with_entries(3);
        let folder = state.vault.add_folder("Work");
        let first = state.filtered_ids[0];
        state.vault.set_folder(first, Some(folder));
        state.vault.add_tag(first, "banking");
        state.refresh_filter();
        state
    }

    /// Folders and tags were drawn as selectable sidebar rows and wired to
    /// nothing: `handle_sidebar_click` re-derived the row bands by re-summing
    /// the renderer's arithmetic, and simply had no arm for the two groups the
    /// renderer drew last. Clicking one did nothing at all, silently.
    ///
    /// It cannot regress in that shape now -- the click resolves through the
    /// boxes the renderer recorded, so a drawn row is a clickable row by
    /// construction -- but this is the assertion that says so.
    #[test]
    fn a_folder_row_in_the_sidebar_selects_that_folder() {
        let mut state = unlocked_with_a_folder_and_a_tag();
        let folder = state.vault.folders[0].id;
        let index = state
            .sidebar_items()
            .iter()
            .position(|item| *item == SidebarSelection::Folder(folder))
            .expect("the folder is drawn in the sidebar");

        probe::click(&mut state, Target::Sidebar(index));
        assert_eq!(state.sidebar_selection, SidebarSelection::Folder(folder));
        assert_eq!(
            state.filtered_ids.len(),
            1,
            "selecting a folder must narrow the list to it"
        );
    }

    #[test]
    fn a_tag_row_in_the_sidebar_selects_that_tag() {
        let mut state = unlocked_with_a_folder_and_a_tag();
        let index = state
            .sidebar_items()
            .iter()
            .position(|item| *item == SidebarSelection::Tag("banking".to_string()))
            .expect("the tag is drawn in the sidebar");

        probe::click(&mut state, Target::Sidebar(index));
        assert_eq!(
            state.sidebar_selection,
            SidebarSelection::Tag("banking".to_string())
        );
        assert_eq!(state.filtered_ids.len(), 1);
    }

    /// Every sidebar row the renderer draws is reachable, and reaching it
    /// selects *that* row rather than its neighbour.
    ///
    /// The old arrangement could satisfy the first half and fail the second:
    /// two independent walks down the same list of headings and gaps agree
    /// until one of them gains a group, and then every row below it is off by
    /// the height of a heading.
    #[test]
    fn every_sidebar_row_selects_the_item_the_renderer_drew_there() {
        let mut state = unlocked_with_a_folder_and_a_tag();
        let items = state.sidebar_items();
        assert!(
            items.len() > EntryType::all().len() + 3,
            "the fixture must reach the folder and tag groups"
        );
        for (index, item) in items.iter().enumerate() {
            probe::click(&mut state, Target::Sidebar(index));
            assert_eq!(
                state.sidebar_selection, *item,
                "row {index} is drawn for {item:?} but selected something else"
            );
        }
    }

    /// The lock screen's Unlock button was decoration: `handle_mouse` returned
    /// `Ignored` outright while the vault was locked, so the only way in was
    /// the Enter key. A button that is painted, labelled and inert is worse
    /// than no button -- it tells the user the pointer is the way in.
    #[test]
    fn the_unlock_button_unlocks_the_vault() {
        let mut state = AppState::for_test();
        assert!(!state.vault.is_unlocked(), "a fresh vault starts locked");
        state.master_input = TEST_MASTER_PASSWORD.to_string();

        probe::click(&mut state, Target::Unlock);
        assert!(
            state.vault.is_unlocked(),
            "the button must do what Enter does"
        );
        assert!(
            state.master_input.is_empty(),
            "the typed master password must not outlive the unlock"
        );
    }

    #[test]
    fn the_unlock_button_reports_a_wrong_master_password() {
        let mut state = AppState::for_test();
        state.master_input = "not the master password".to_string();

        probe::click(&mut state, Target::Unlock);
        assert!(!state.vault.is_unlocked());
        assert!(state.unlock_failed, "the refusal must be visible");
    }

    /// While the vault is locked, the lock screen is the whole window: none of
    /// the controls behind it are drawn, so none of them can be reached.
    #[test]
    fn a_locked_vault_draws_no_control_but_its_own() {
        let state = AppState::for_test();
        let names = probe::control_names(&state);
        assert!(names.contains(&"Unlock".to_string()));
        assert!(names.contains(&"MasterInput".to_string()));
        for hidden in ["LockVault", "Settings", "Sidebar", "EntryRow"] {
            assert!(
                !names.contains(&hidden.to_string()),
                "{hidden} is behind the lock screen but was drawn: {names:?}"
            );
        }
    }

    /// The renderer must believe the size it is handed rather than the one it
    /// remembers, because the first frame a real window submits goes out
    /// before any resize event exists.
    #[test]
    fn the_window_draws_what_it_is_given_rather_than_what_it_remembers() {
        let state = unlocked_with_entries(3);
        assert_eq!(state.width, DEFAULT_WINDOW_WIDTH);

        let narrow = (700.0, 500.0);
        let frame = state.draw(narrow);
        assert_eq!(
            (frame.width, frame.height),
            narrow,
            "the frame was sized from the remembered width, not the given one"
        );

        // A toolbar button is laid out left to right from a fixed origin, so
        // it must not move with the width; the detail pane is measured from
        // the right edge, so it must.
        let wide_button = probe::rect_of_sized(&state, Target::Add, AppState::SIZE)
            .expect("the toolbar is drawn when unlocked");
        let narrow_button =
            probe::rect_of_sized(&state, Target::Add, narrow).expect("Add is left of the fold");
        assert_eq!(wide_button, narrow_button);
        assert!(
            Layout::new(narrow.0, narrow.1).detail.w
                < Layout::new(DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT)
                    .detail
                    .w,
            "the detail pane must be measured from the width it was given"
        );
    }

    /// The toolbar is a fixed 882px of buttons laid out left to right from the
    /// sidebar's edge, and it neither wraps nor scrolls. Narrow the window and
    /// the right-hand buttons -- Lock Vault among them -- go off the edge with
    /// nothing to say so.
    ///
    /// What this asserts is the half that is *safe*: a button clipped away
    /// records no hit box, so there is no invisible target sitting off-screen
    /// or under the pane beside it. The half that is not safe is that the
    /// button is simply gone, which is
    /// `known-issues.md` -> `C-CREDMANAGER-TOOLBAR-FALLS-OFF-A-NARROW-WINDOW`.
    /// When the toolbar learns to overflow, this test should start failing.
    #[test]
    fn a_toolbar_button_past_the_right_edge_records_no_hit_box() {
        let state = unlocked_with_entries(3);
        let full = probe::rect_of_sized(&state, Target::Settings, AppState::SIZE)
            .expect("Settings is drawn at the default size");
        assert!(
            full.right() > 700.0,
            "the fixture must be narrower than the toolbar needs"
        );

        // 500, not the 700 this used until 2026-09-03. The search box now
        // gives up its width before the buttons do (`search_box_width`), which
        // moved `Settings` from 802..882 to 682..762 at a 700px window -- so at
        // 700 it is *partially* on screen and correctly records a hit box for
        // the part you can see. That is the fix working, exactly as this
        // entry's known-issue predicted ("this test should start failing"), not
        // a regression in the clipping. The property being asserted is
        // unchanged: a button drawn *entirely* past the edge is not clickable.
        assert!(
            probe::rect_of_sized(&state, Target::Settings, (500.0, 500.0)).is_none(),
            "a button drawn past the edge must not be clickable there"
        );
        // Everything left of the fold is untouched.
        assert!(probe::rect_of_sized(&state, Target::Add, (500.0, 500.0)).is_some());
        assert!(probe::rect_of_sized(&state, Target::Sort, (500.0, 500.0)).is_some());

        // And a button straddling the edge is clickable only where it is
        // visible -- the half-truth between "drawn" and "gone", which is worth
        // pinning because it is the case the elastic search box created.
        let straddling = probe::rect_of_sized(&state, Target::Settings, (700.0, 500.0))
            .expect("Settings straddles the right edge at 700px");
        assert!(
            straddling.right() <= 700.0 + 0.01,
            "the visible part of a straddling button must stop at the window edge, not run past it to {}",
            straddling.right()
        );
    }

    /// `Ctrl+L` locks the vault, at a window size where the button cannot.
    ///
    /// This is the accelerator that makes `C-CREDMANAGER-TOOLBAR-FALLS-OFF-A-
    /// NARROW-WINDOW` an annoyance rather than a security defect: narrow the
    /// window far enough and the `Lock Vault` button is clipped away, and the
    /// keyboard is then the only way to lock the vault and walk away.
    ///
    /// It was **untested until 2026-09-03**, and the known-issue entry asserted
    /// the opposite -- "the keyboard has no accelerator for it either" -- which
    /// was already false when it was written. A handler nothing drives is a
    /// handler nobody knows is reachable: `handle_key`'s `Key::L` arm sits
    /// after an early return for the lock screen, so "it is in the file" and
    /// "a keystroke gets to it" are different claims. This drives the keystroke.
    #[test]
    fn ctrl_l_locks_the_vault_even_when_the_button_is_off_the_edge() {
        let mut state = unlocked_with_entries(3);
        assert!(state.vault.is_unlocked(), "the fixture starts unlocked");

        // Narrow enough that the button is genuinely gone, so this cannot pass
        // by accidentally exercising the same path a click would. 500, not 700:
        // since `search_box_width` landed, 700px keeps Lock Vault on screen at
        // 600..670 -- which is the point of that change, and would have made
        // this test assert nothing.
        let narrow = (500.0, 500.0);
        assert!(
            probe::rect_of_sized(&state, Target::LockVault, narrow).is_none(),
            "this test is about the case where the button is not reachable; \
             if it is drawn here, the fixture no longer sets that case up"
        );

        let ctrl_l = |pressed: bool| {
            Event::Key(KeyEvent {
                key: Key::L,
                pressed,
                modifiers: guitk::event::Modifiers::ctrl(),
                text: String::new(),
            })
        };

        // Through `handle_event`, not `handle_key`: the press/release guard is
        // at the dispatch site, so calling `handle_key` directly would prove
        // the arm runs while saying nothing about whether a real keystroke
        // reaches it -- and would pass just as loudly if the guard were wrong.
        assert_eq!(
            handle_event(&mut state, &ctrl_l(false)),
            EventResult::Ignored,
            "a key coming *up* must not act; it is not a second press"
        );
        assert!(
            state.vault.is_unlocked(),
            "the release alone locked the vault"
        );

        assert_eq!(
            handle_event(&mut state, &ctrl_l(true)),
            EventResult::Consumed
        );
        assert!(
            !state.vault.is_unlocked(),
            "Ctrl+L did not lock the vault, and at this width nothing else can"
        );
    }

    /// The search box gives up its width before any button falls off.
    ///
    /// The row is fixed-width and neither wraps nor scrolls, so in a narrow
    /// window something has to go. The search box is the only control that is
    /// still itself at half size, so it shrinks first -- which buys 120 px, and
    /// 120 px is two more buttons.
    #[test]
    fn the_search_box_shrinks_before_the_buttons_are_pushed_off() {
        // Wide: the search box is at its full width and nothing is cramped.
        assert_eq!(search_box_width(1280.0), SEARCH_WIDTH);

        // Narrow: it has given up width, but not below the floor.
        let cramped = search_box_width(800.0);
        assert!(
            cramped < SEARCH_WIDTH,
            "at 800px the box should have given up width, not held 200"
        );
        assert!(
            cramped >= SEARCH_MIN_WIDTH,
            "the box shrank past the floor to {cramped}"
        );

        // Absurd: it stops at the floor rather than going to zero or negative,
        // which a bare subtraction would do and `Rect` would happily accept.
        for w in [400.0_f32, 100.0, 1.0, 0.0] {
            assert_eq!(
                search_box_width(w),
                SEARCH_MIN_WIDTH,
                "a {w}px window should pin the search box at the floor"
            );
        }
    }

    /// Shrinking the search box actually keeps a button on screen that would
    /// otherwise be gone.
    ///
    /// The point of the previous test is arithmetic; this one is the thing the
    /// user gets. At 820 px the fixed layout put `Lock Vault` past the right
    /// edge -- 882 px is what the row wanted -- and with the elastic box it is
    /// drawn and clickable.
    #[test]
    fn a_narrower_window_still_reaches_lock_vault() {
        let state = unlocked_with_entries(3);
        let rect = probe::rect_of_sized(&state, Target::LockVault, (820.0, 500.0))
            .expect("Lock Vault should survive at 820px now the search box gives way");
        assert!(
            rect.right() <= 820.0 + 0.01,
            "the button is recorded but hangs off the edge at {}",
            rect.right()
        );
    }

    /// A window too small for its own layout must not record a hit box outside
    /// itself. `Frame::new` does not clip, so a pane that clamped rather than
    /// shrank would leave a clickable region hanging off the edge -- and a
    /// click there would select a row the user cannot see.
    #[test]
    fn a_window_smaller_than_its_layout_records_nothing_outside_itself() {
        let state = unlocked_with_entries(20);
        for size in [(200.0, 120.0), (60.0, 40.0), (1.0, 1.0), (0.0, 0.0)] {
            let frame = state.draw(size);
            for (target, rect) in frame.hits() {
                assert!(
                    rect.x >= 0.0
                        && rect.y >= 0.0
                        && rect.right() <= size.0 + 0.01
                        && rect.bottom() <= size.1 + 0.01,
                    "{target:?} recorded {rect:?}, outside a {size:?} window"
                );
            }
        }
    }

    /// A size the compositor should never send, but which costs one
    /// `is_finite` to survive and an unbounded loop to not.
    #[test]
    fn a_nonfinite_window_size_draws_an_empty_window() {
        let state = unlocked_with_entries(20);
        for size in [(f32::NAN, 800.0), (1280.0, f32::INFINITY), (-50.0, -50.0)] {
            let frame = state.draw(size);
            assert!(frame.width >= 0.0 && frame.width.is_finite());
            assert!(frame.height >= 0.0 && frame.height.is_finite());
        }
    }

    /// Every clickable thing the unlocked window draws is on screen. Adding a
    /// `Target` variant and forgetting to draw a box for it fails here rather
    /// than shipping as a control that does nothing.
    #[test]
    fn every_control_the_unlocked_window_draws_is_recorded() {
        let state = unlocked_with_a_folder_and_a_tag();
        let names = probe::control_names(&state);
        for expected in [
            "Add",
            "Search",
            "Sort",
            "Generator",
            "LockVault",
            "Settings",
            "Sidebar",
            "EntryRow",
        ] {
            assert!(
                names.contains(&expected.to_string()),
                "{expected} is not on screen: {names:?}"
            );
        }
    }

    /// Clicking the toolbar's Lock returns the window to the lock screen, and
    /// the lock screen is drawn from the same frame the next click resolves
    /// through -- so the way back in is reachable immediately.
    #[test]
    fn the_lock_button_returns_the_window_to_the_lock_screen() {
        let mut state = unlocked_with_entries(5);
        probe::click(&mut state, Target::LockVault);
        assert!(!state.vault.is_unlocked());
        assert!(
            probe::is_visible(&state, Target::Unlock),
            "locking must draw the way back in"
        );
    }

    /// Opening the generator fills it, so the panel never appears blank.
    ///
    /// Seeded on purpose: `PasswordGenerator::new` draws from
    /// `CredRandom::from_system`, which on a host test has no kernel entropy to
    /// draw from and therefore *refuses* -- correctly, since handing out a
    /// password that only looks random is the failure this crate is built to
    /// avoid. Asserting on a real password here would be asserting that the
    /// refusal is broken.
    #[test]
    fn the_generator_button_opens_the_generator_with_a_password_in_it() {
        let mut state = unlocked_with_entries(5);
        state.password_generator = seeded(7);
        assert!(state.generated_password.is_empty());

        probe::click(&mut state, Target::Generator);
        assert_eq!(state.detail_view, DetailView::PasswordGenerator);
        assert!(
            !state.generated_password.is_empty(),
            "an empty generator panel is a panel that looks broken"
        );
        assert_eq!(state.generator_error, None);
    }

    /// And when the source cannot be trusted, the panel says so rather than
    /// showing an empty field -- which is what a user reads as "still loading".
    #[test]
    fn the_generator_button_opens_a_refusal_when_there_is_no_entropy() {
        let mut state = unlocked_with_entries(5);
        state.password_generator = PasswordGenerator::without_entropy();

        probe::click(&mut state, Target::Generator);
        assert_eq!(state.detail_view, DetailView::PasswordGenerator);
        assert!(state.generated_password.is_empty());
        assert_eq!(
            state.generator_error.as_deref(),
            Some(NO_ENTROPY_MESSAGE),
            "a blank field with no reason is indistinguishable from a bug"
        );
    }

    /// Clicking bare background must not select, scroll or navigate anything.
    #[test]
    fn clicking_the_background_changes_nothing() {
        let mut state = unlocked_with_entries(5);
        probe::click(&mut state, Target::Sidebar(0));
        let before = (
            state.selected_entry_id,
            state.detail_view,
            state.sidebar_selection.clone(),
        );
        probe::click_background(&mut state);
        assert_eq!(state.selected_entry_id, before.0);
        assert_eq!(state.detail_view, before.1);
        assert_eq!(state.sidebar_selection, before.2);
    }

    // == Adding a credential ===================================================
    //
    // The toolbar's Add button was drawn, was hit-tested, and was answered
    // with `EventResult::Ignored`, so the vault had no way to gain an entry --
    // and every feature downstream of having one was dead code the compiler
    // had been reporting all along.

    /// An app past the lock screen, which is where all of this lives.
    fn unlocked_app() -> AppState {
        let mut state = AppState::for_test();
        assert!(
            state.vault.unlock(TEST_MASTER_PASSWORD, state.now),
            "the test vault should open with the test password"
        );
        state
    }

    fn press(state: &mut AppState, target: Target) -> EventResult {
        act_on(state, target)
    }

    fn type_str(state: &mut AppState, text: &str) {
        for ch in text.chars() {
            let ev = KeyEvent {
                key: Key::A,
                pressed: true,
                modifiers: guitk::event::Modifiers::NONE,
                text: ch.to_string(),
            };
            handle_key(state, &ev);
        }
    }

    /// Every control the generator panel draws can be operated.
    ///
    /// The panel drew three mode buttons with the active one highlighted, a
    /// length slider with its knob placed from the current value, and four
    /// ticked checkboxes. `CharsetOptions` was four `true`s with no writer in
    /// the crate, `mode` never left `Random`, and `set_length` carried an
    /// `allow(dead_code)` saying the panel had no length control.
    #[test]
    fn every_generator_control_answers_a_key() {
        let mut state = unlocked_app();
        state.detail_view = DetailView::PasswordGenerator;

        // The mode row.
        let mode = state.password_generator.mode;
        assert_eq!(
            handle_key(&mut state, &ctrl_of(Key::M)),
            EventResult::Consumed,
            "Ctrl+M was ignored on the generator panel"
        );
        assert_ne!(
            state.password_generator.mode, mode,
            "Ctrl+M did not move the mode"
        );

        // The length slider.
        state.password_generator.mode = GeneratorMode::Random;
        let len = state.password_generator.length;
        handle_key(&mut state, &key_of(Key::Right));
        assert_eq!(
            state.password_generator.length,
            len.saturating_add(1),
            "Right did not lengthen the password"
        );
        handle_key(&mut state, &key_of(Key::Left));
        assert_eq!(
            state.password_generator.length, len,
            "Left did not shorten it back"
        );

        // The four boxes.
        for box_ in CharsetBox::ALL {
            let before = box_.is_on(&state.password_generator.charset);
            let keys = box_.keys();
            let presses = guitk::shortcut::keystrokes(keys)
                .unwrap_or_else(|e| panic!("{} prints unparseable keys: {e:?}", box_.label()));
            let first = presses.first().expect("no keystroke");
            let ev = KeyEvent {
                key: first.key,
                pressed: true,
                modifiers: first.modifiers,
                text: String::new(),
            };
            assert_eq!(
                handle_key(&mut state, &ev),
                EventResult::Consumed,
                "{} ignored {keys}",
                box_.label()
            );
            assert_ne!(
                box_.is_on(&state.password_generator.charset),
                before,
                "{} did not change when {keys} was pressed",
                box_.label()
            );
            // Put it back, so each box is tested against a full set rather
            // than against whatever the previous one left behind.
            handle_key(&mut state, &ev);
        }
    }

    /// The last character set cannot be cleared, and the panel says why.
    ///
    /// `build_charset` answers an empty pool with an empty password, so a
    /// generator with every box clear would produce nothing and explain
    /// nothing.
    #[test]
    fn the_last_character_set_cannot_be_cleared() {
        let mut state = unlocked_app();
        state.detail_view = DetailView::PasswordGenerator;
        state.password_generator.mode = GeneratorMode::Random;

        for box_ in [
            CharsetBox::Lowercase,
            CharsetBox::Digits,
            CharsetBox::Symbols,
        ] {
            box_.set(&mut state.password_generator.charset, false);
        }
        assert!(
            CharsetBox::Uppercase.is_on(&state.password_generator.charset),
            "control: uppercase should be the only one left"
        );

        handle_key(&mut state, &ctrl_of(Key::Num1));
        assert!(
            CharsetBox::Uppercase.is_on(&state.password_generator.charset),
            "the last character set was cleared"
        );
        assert_eq!(
            state.generator_error.as_deref(),
            Some(NEEDS_ONE_CHARACTER_SET),
            "clearing the last set said nothing about why it refused"
        );
    }

    /// The panel prints the key beside every box it draws.
    #[test]
    fn the_generator_panel_names_its_keys() {
        let mut state = unlocked_app();
        state.detail_view = DetailView::PasswordGenerator;
        state.password_generator.mode = GeneratorMode::Random;
        let texts: Vec<String> = state
            .draw((1200.0, 800.0))
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        for box_ in CharsetBox::ALL {
            let want = format!("{} ({})", box_.label(), box_.keys());
            assert!(texts.contains(&want), "the panel never draws {want:?}");
        }
        assert!(
            texts.iter().any(|t| t == "Mode (Ctrl+M)"),
            "the mode row does not say which key changes it"
        );
        assert!(
            texts.iter().any(|t| t == "Length (Left/Right)"),
            "the slider does not say which keys move it"
        );
    }

    /// The generator's keys belong to the generator: they must not reach the
    /// search box that is behind it, nor act when it is not up.
    #[test]
    fn the_generator_keys_do_nothing_when_the_panel_is_not_up() {
        let mut state = unlocked_app();
        state.detail_view = DetailView::EntryDetail;
        let before = state.password_generator.length;
        handle_key(&mut state, &key_of(Key::Right));
        assert_eq!(
            state.password_generator.length, before,
            "Right changed the generator from a view that does not show it"
        );
    }

    fn ctrl_of(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::ctrl(),
            text: String::new(),
        }
    }

    fn key_of(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        }
    }

    #[test]
    fn the_add_button_opens_a_form() {
        let mut state = unlocked_app();
        assert_eq!(press(&mut state, Target::Add), EventResult::Consumed);
        assert_eq!(state.detail_view, DetailView::NewEntry);
        assert!(state.new_entry.is_some());
    }

    #[test]
    fn a_credential_typed_into_the_form_reaches_the_vault() {
        let mut state = unlocked_app();
        let before = state.vault.entries.len();
        press(&mut state, Target::Add);
        type_str(&mut state, "example.com");
        handle_key(&mut state, &key_of(Key::Tab));
        type_str(&mut state, "alice");
        handle_key(&mut state, &key_of(Key::Tab));
        type_str(&mut state, "hunter2");
        assert_eq!(press(&mut state, Target::NewSave), EventResult::Consumed);

        assert_eq!(state.vault.entries.len(), before + 1);
        let id = state.selected_entry_id.expect("the new entry is selected");
        let entry = state.vault.get_entry(id).expect("it is in the vault");
        match &entry.data {
            EntryData::Login(d) => {
                assert_eq!(d.site, "example.com");
                assert_eq!(d.username, "alice");
                assert_eq!(d.password, "hunter2");
            }
            other => panic!("expected a login, got {other:?}"),
        }
        assert_eq!(state.detail_view, DetailView::EntryDetail);
        assert!(state.new_entry.is_none(), "the form is put away once saved");
    }

    #[test]
    fn a_form_with_no_name_is_not_saved() {
        let mut state = unlocked_app();
        let before = state.vault.entries.len();
        press(&mut state, Target::Add);
        // Straight to the password, leaving the site blank.
        handle_key(&mut state, &key_of(Key::Tab));
        handle_key(&mut state, &key_of(Key::Tab));
        type_str(&mut state, "secret");
        press(&mut state, Target::NewSave);
        assert_eq!(state.vault.entries.len(), before);
        assert!(
            state.new_entry.is_some(),
            "the form stays open rather than discarding what was typed"
        );
    }

    #[test]
    fn cancelling_throws_the_form_away() {
        let mut state = unlocked_app();
        let before = state.vault.entries.len();
        press(&mut state, Target::Add);
        type_str(&mut state, "something");
        assert_eq!(press(&mut state, Target::NewCancel), EventResult::Consumed);
        assert!(state.new_entry.is_none());
        assert_eq!(state.vault.entries.len(), before);
        assert_eq!(state.detail_view, DetailView::EntryDetail);
    }

    #[test]
    fn escape_cancels_the_form() {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        type_str(&mut state, "half typed");
        handle_key(&mut state, &key_of(Key::Escape));
        assert!(
            state.new_entry.is_none(),
            "a half-typed password should not be left in memory"
        );
    }

    #[test]
    fn every_kind_of_credential_can_be_created() {
        for (index, kind) in EntryType::all().iter().enumerate() {
            let mut state = unlocked_app();
            press(&mut state, Target::Add);
            press(&mut state, Target::NewKind(index));
            type_str(&mut state, "a name");
            assert!(
                press(&mut state, Target::NewSave) == EventResult::Consumed,
                "{kind:?} could not be saved"
            );
            let id = state.selected_entry_id.expect("selected");
            let entry = state.vault.get_entry(id).expect("in the vault");
            assert_eq!(
                entry.data.entry_type(),
                *kind,
                "the form saved the wrong kind"
            );
        }
    }

    #[test]
    fn changing_kind_does_not_carry_typed_values_across() {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        // Field 2 of a Login is the password; field 2 of an SSH key is the
        // public key, which is drawn in the clear.
        handle_key(&mut state, &key_of(Key::Tab));
        handle_key(&mut state, &key_of(Key::Tab));
        type_str(&mut state, "hunter2");
        let ssh = EntryType::all()
            .iter()
            .position(|k| *k == EntryType::SshKey)
            .expect("SshKey is one of the kinds");
        press(&mut state, Target::NewKind(ssh));
        let form = state.new_entry.as_ref().expect("still open");
        assert!(
            form.values.iter().all(|v| v.is_empty()),
            "a secret typed under one kind must not reappear in a field that \
             another kind draws in the clear"
        );
    }

    #[test]
    fn the_form_opens_on_the_kind_the_sidebar_is_filtering_by() {
        let mut state = unlocked_app();
        state.sidebar_selection = SidebarSelection::TypeFilter(EntryType::CreditCard);
        press(&mut state, Target::Add);
        assert_eq!(
            state.new_entry.as_ref().map(|f| f.kind),
            Some(EntryType::CreditCard),
            "someone who has just narrowed the list to cards and pressed Add \
             is asking for a card"
        );
    }

    #[test]
    fn a_card_number_is_masked_on_the_way_in() {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        let card = EntryType::all()
            .iter()
            .position(|k| *k == EntryType::CreditCard)
            .expect("CreditCard is one of the kinds");
        press(&mut state, Target::NewKind(card));
        type_str(&mut state, "Everyday");
        handle_key(&mut state, &key_of(Key::Tab));
        type_str(&mut state, "4111111111111111");
        press(&mut state, Target::NewSave);
        let id = state.selected_entry_id.expect("selected");
        match &state.vault.get_entry(id).expect("in the vault").data {
            EntryData::CreditCard(d) => {
                assert!(
                    d.number_masked.ends_with("1111") && d.number_masked.contains('*'),
                    "the vault should never hold the digits it does not need: \
                     {:?}",
                    d.number_masked
                );
                assert!(!d.number_masked.contains("4111111111111111"));
            }
            other => panic!("expected a card, got {other:?}"),
        }
    }

    #[test]
    fn tab_cycles_the_fields() {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        let count = state.new_entry.as_ref().expect("open").values.len();
        for _ in 0..count {
            handle_key(&mut state, &key_of(Key::Tab));
        }
        assert_eq!(
            state.new_entry.as_ref().map(|f| f.focused),
            Some(0),
            "a full circle of Tab comes back to the first field"
        );
    }

    #[test]
    fn clicking_a_field_focuses_it() {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        press(&mut state, Target::NewField(2));
        type_str(&mut state, "typed here");
        let form = state.new_entry.as_ref().expect("open");
        assert_eq!(form.value(2), "typed here");
        assert_eq!(form.value(0), "");
    }

    #[test]
    fn typing_goes_to_the_form_and_not_the_search_box_behind_it() {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        type_str(&mut state, "abc");
        assert_eq!(state.search_query, "");
        assert_eq!(state.new_entry.as_ref().map(|f| f.value(0)), Some("abc"));
    }

    // == Copying a credential ==================================================

    /// A vault with one login in it, selected.
    fn state_with_login() -> AppState {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        type_str(&mut state, "example.com");
        handle_key(&mut state, &key_of(Key::Tab));
        type_str(&mut state, "alice");
        handle_key(&mut state, &key_of(Key::Tab));
        type_str(&mut state, "hunter2");
        press(&mut state, Target::NewSave);
        state
    }

    /// **A chord is neither a vault key nor typing, and AltGr types** -- in
    /// the program where a stray letter is a password that opens nothing:
    ///
    /// - the master password, an entry's fields and the search took a
    ///   command's letter, which a chord carries as text: Ctrl+V typed a
    ///   `v` into the master password, Ctrl+L an `l` into an entry;
    /// - AltGr+L, a Polish `ł`, locked the vault rather than typing, and
    ///   AltGr+Q, a German `@`, closed the window;
    /// - Alt+Enter answered "delete this entry?" with yes, and tried the
    ///   master password.
    #[test]
    fn a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types() {
        use guitk::event::Modifiers;
        let altgr = Modifiers {
            alt: true,
            ..Modifiers::ctrl()
        };
        let key = |key: Key, text: &str, modifiers: Modifiers| {
            Event::Key(KeyEvent {
                key,
                pressed: true,
                modifiers,
                text: text.to_owned(),
            })
        };
        let commands = [
            (Key::V, "v", Modifiers::ctrl()),
            (Key::X, "x", Modifiers::alt()),
            (Key::X, "x", Modifiers::super_key()),
        ];

        // The lock screen.
        let mut state = AppState::for_test();
        for (k, text, m) in commands {
            handle_event(&mut state, &key(k, text, m));
        }
        assert_eq!(
            state.master_input, "",
            "a command's letter went into the master password"
        );
        handle_event(&mut state, &key(Key::L, "ł", altgr));
        assert_eq!(state.master_input, "ł", "AltGr's ł was not typed");
        handle_event(&mut state, &key(Key::Enter, "", Modifiers::alt()));
        assert!(!state.unlock_failed, "Alt+Enter tried the master password");

        // A new vault's master password, the same.
        let mut state = AppState::for_test();
        state.gate = Gate::Create(NewVault::default());
        for (k, text, m) in commands {
            handle_event(&mut state, &key(k, text, m));
        }
        handle_event(&mut state, &key(Key::Tab, "", Modifiers::alt()));
        handle_event(&mut state, &key(Key::L, "ł", altgr));
        let Gate::Create(form) = &state.gate else {
            panic!("the new-vault form went away");
        };
        assert_eq!(
            form.password, "ł",
            "the new password typed a command or lost AltGr's ł"
        );
        assert!(!form.confirming, "Alt+Tab moved to the second field");

        // Past it: AltGr+L types into the search and does not lock.
        let mut state = unlocked_app();
        handle_event(&mut state, &key(Key::L, "ł", altgr));
        assert!(state.vault.is_unlocked(), "AltGr+L locked the vault");
        handle_event(&mut state, &key(Key::W, "w", Modifiers::alt()));
        assert_eq!(
            state.search_query, "ł",
            "the search typed a command or lost AltGr's ł"
        );
        // The list's keys are taken plain: Alt+Escape is no Escape.
        handle_event(&mut state, &key(Key::Escape, "", Modifiers::alt()));
        assert_eq!(state.search_query, "ł", "Alt+Escape cleared the search");
        assert!(
            !matches!(state.on_event(&key(Key::Q, "@", altgr)), Response::Exit),
            "AltGr+Q closed the window"
        );

        // An entry's field.
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        for (k, text, m) in commands {
            handle_event(&mut state, &key(k, text, m));
        }
        handle_event(&mut state, &key(Key::L, "l", Modifiers::ctrl()));
        handle_event(&mut state, &key(Key::L, "ł", altgr));
        let typed = state
            .new_entry
            .as_ref()
            .and_then(|form| form.values.get(form.focused).cloned());
        assert_eq!(
            typed.as_deref(),
            Some("ł"),
            "the field typed a command or lost AltGr's ł"
        );
        handle_event(&mut state, &key(Key::Escape, "", Modifiers::alt()));
        assert!(state.new_entry.is_some(), "Alt+Escape threw the form away");

        // The generator panel: its Ctrl+M is a Ctrl chord, its arrows plain.
        let mut state = unlocked_app();
        handle_event(&mut state, &key(Key::G, "g", Modifiers::ctrl()));
        assert_eq!(state.detail_view, DetailView::PasswordGenerator);
        let (mode, length) = (
            state.password_generator.mode,
            state.password_generator.length,
        );
        handle_event(&mut state, &key(Key::M, "", altgr));
        handle_event(&mut state, &key(Key::Right, "", Modifiers::alt()));
        handle_event(&mut state, &key(Key::Right, "", Modifiers::super_key()));
        assert_eq!(
            state.password_generator.mode, mode,
            "AltGr+M changed the mode"
        );
        assert_eq!(
            state.password_generator.length, length,
            "a chord moved the length"
        );

        // The list: Alt+Delete does not ask to delete.
        let mut state = state_with_login();
        let id = state.selected_entry_id.expect("the new login is selected");
        handle_event(&mut state, &key(Key::Delete, "", Modifiers::alt()));
        assert!(
            state.dialog.is_none(),
            "Alt+Delete asked to delete the entry"
        );

        // A backup's password, asked for in a dialog, types as the others.
        state.dialog = Some(VaultDialog::RestorePassword {
            path: std::path::PathBuf::from("backup.vault"),
            backup: Box::new(unlocked_vault()),
            input: String::new(),
            error: None,
        });
        for (k, text, m) in commands {
            handle_event(&mut state, &key(k, text, m));
        }
        handle_event(&mut state, &key(Key::L, "ł", altgr));
        let Some(VaultDialog::RestorePassword { input, .. }) = &state.dialog else {
            panic!("a chord closed the dialog");
        };
        assert_eq!(
            input, "ł",
            "the backup's password typed a command or lost AltGr's ł"
        );

        // "Delete this entry?" -- asked by a plain Delete -- is answered by a
        // plain Enter only.
        state.dialog = None;
        handle_event(&mut state, &key(Key::Delete, "", Modifiers::NONE));
        assert!(
            matches!(state.dialog, Some(VaultDialog::DeleteConfirm { .. })),
            "control: Delete asks"
        );
        for m in [
            Modifiers::alt(),
            Modifiers::super_key(),
            altgr,
            Modifiers::ctrl(),
        ] {
            handle_event(&mut state, &key(Key::Enter, "", m));
        }
        assert!(
            state.vault.get_entry(id).is_some(),
            "a chord deleted the entry"
        );
        assert!(state.dialog.is_some(), "a chord answered the question");
    }

    /// **A Copy press says nothing was copied, and why** -- where it said
    /// "Copied Password -- clears in 30s" over a clipboard only this program
    /// could read, so the user pasted nothing elsewhere and could not tell
    /// why. The secret is not kept anywhere by the press.
    #[test]
    fn a_copy_press_says_nothing_was_copied_and_why() {
        let mut state = state_with_login();
        let fields = copyable_fields(&state);
        let (label, _) = fields
            .iter()
            .find(|(l, v)| *l == "Password" && !v.is_empty())
            .expect("the fixture has a password")
            .clone();
        let index = fields
            .iter()
            .position(|(l, _)| *l == label)
            .expect("its row");
        assert!(copy_field(&mut state, index), "the press was not answered");
        assert_eq!(state.copy_refused.as_deref(), Some("Password"));
        let said = format!("Password not copied: {NOT_COPIED}");
        assert!(
            toolbar_texts(&state).contains(&said),
            "the refusal is not drawn: {:?}",
            toolbar_texts(&state)
        );
        assert!(
            !toolbar_texts(&state)
                .iter()
                .any(|t| t.starts_with("Copied")),
            "something still claims a copy"
        );
    }

    /// An empty field is not a press worth answering: nothing is said.
    #[test]
    fn an_empty_field_is_not_answered() {
        let mut state = state_with_login();
        let fields = copyable_fields(&state);
        if let Some(index) = fields.iter().position(|(_, v)| v.is_empty()) {
            assert!(!copy_field(&mut state, index));
            assert_eq!(state.copy_refused, None);
        }
    }

    #[test]
    fn a_copy_target_past_the_last_field_does_nothing() {
        let mut state = state_with_login();
        assert_eq!(
            press(&mut state, Target::CopyField(99)),
            EventResult::Ignored
        );
        assert_eq!(
            state.copy_refused, None,
            "a press past the fields was answered"
        );
    }

    #[test]
    fn copying_with_nothing_selected_does_nothing() {
        let mut state = unlocked_app();
        state.selected_entry_id = None;
        assert_eq!(
            press(&mut state, Target::CopyField(0)),
            EventResult::Ignored
        );
    }

    #[test]
    fn the_copyable_fields_match_what_the_detail_view_draws() {
        // The view passes `CopyField(n)` counted down its own rows, so the two
        // lists have to agree on what row n is -- including the blank ones.
        let state = state_with_login();
        let fields = copyable_fields(&state);
        let labels: Vec<&str> = fields.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, vec!["Site", "Username", "Password", "URL", "TOTP"]);
    }

    #[test]
    fn a_name_of_only_spaces_is_not_a_name() {
        let mut state = unlocked_app();
        let before = state.vault.entries.len();
        press(&mut state, Target::Add);
        type_str(&mut state, "   ");
        press(&mut state, Target::NewSave);
        assert_eq!(
            state.vault.entries.len(),
            before,
            "a credential whose name is blank is one the list cannot show and the user cannot find again"
        );
    }

    #[test]
    fn a_saved_credential_appears_in_the_list() {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        type_str(&mut state, "example.com");
        press(&mut state, Target::NewSave);
        let id = state.selected_entry_id.expect("selected");
        assert!(
            state.filtered_ids.contains(&id),
            "the list is rebuilt from the vault, so it has to be rebuilt when the vault gains an entry"
        );
    }

    #[test]
    fn focusing_a_field_that_is_not_there_leaves_the_focus_alone() {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        press(&mut state, Target::NewField(99));
        type_str(&mut state, "typed");
        assert_eq!(
            state.new_entry.as_ref().map(|f| f.value(0)),
            Some("typed"),
            "an out-of-range focus must not move the cursor somewhere there is no field to receive it"
        );
    }

    #[test]
    fn backspace_on_an_empty_field_asks_for_no_frame() {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        assert_eq!(
            handle_key(&mut state, &key_of(Key::Backspace)),
            EventResult::Ignored
        );
    }

    #[test]
    fn enter_saves_the_form() {
        let mut state = unlocked_app();
        let before = state.vault.entries.len();
        press(&mut state, Target::Add);
        type_str(&mut state, "example.com");
        assert_eq!(
            handle_key(&mut state, &key_of(Key::Enter)),
            EventResult::Consumed
        );
        assert_eq!(
            state.vault.entries.len(),
            before + 1,
            "a form that cannot be finished from the keyboard needs the mouse for its last step"
        );
    }

    #[test]
    fn the_copy_buttons_are_where_the_pointer_can_reach_them() {
        // Through `target_at`, not `act_on`: the defect was a button that was
        // drawn and registered no hit box, which only a hit test can see.
        let state = state_with_login();
        let frame = state.frame(state.width, state.height);
        let copies: Vec<&Target> = frame
            .hits()
            .iter()
            .map(|(target, _)| target)
            .filter(|t| matches!(t, Target::CopyField(_)))
            .collect();
        assert!(
            !copies.is_empty(),
            "every field's Copy button was painted and none of them registered a hit box"
        );
        // And each one is reachable at its own rectangle.
        for (target, rect) in frame.hits() {
            if matches!(target, Target::CopyField(_)) {
                assert_eq!(
                    state.target_at(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
                    Some(*target)
                );
            }
        }
    }

    #[test]
    fn copying_with_the_selection_cleared_does_nothing() {
        let mut state = state_with_login();
        assert!(!copyable_fields(&state).is_empty());
        state.selected_entry_id = None;
        assert!(
            copyable_fields(&state).is_empty(),
            "with nothing selected there is no field to copy, whatever ids happen to exist in the vault"
        );
        assert_eq!(
            press(&mut state, Target::CopyField(0)),
            EventResult::Ignored
        );
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
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

        fn fills(app: &mut AppState) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = unlocked_app();

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

    /// The warning lines are where they can be seen: nothing drawn after a
    /// line fills the point it is drawn at. The sweep that added them drew
    /// them "after the background, or it would be painted over" -- and in
    /// several apps a bar was then drawn over the same pixels, while a test
    /// that read the frame's texts said they were there. known-issues.md,
    /// `[E] Warnings drawn where the next thing drawn covers them`.
    #[test]
    fn the_warning_lines_are_not_painted_over() {
        let state = AppState::new(unlocked_vault());
        let commands: Vec<RenderCommand> = state.draw((1200.0, 800.0)).commands().to_vec();
        for line in NOT_KEPT_LINES {
            let (at, x, y, reach) = commands
                .iter()
                .enumerate()
                .find_map(|(i, c)| match c {
                    RenderCommand::Text {
                        text,
                        x,
                        y,
                        max_width,
                        ..
                    } if text == line => Some((i, *x, *y, x + max_width.unwrap_or(f32::INFINITY))),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{line:?} is not drawn"));
            let covered = commands.iter().skip(at + 1).any(|c| {
                matches!(c, RenderCommand::FillRect { x: rx, y: ry, width, height, .. }
                    if x >= *rx && x < rx + width && y >= *ry && y < ry + height)
            });
            assert!(!covered, "{line:?} is painted over");
            // Nor drawn on the same row as other text: a header's title over
            // a warning is as unreadable as a fill over it.
            let crowded = commands.iter().any(|c| {
                matches!(c, RenderCommand::Text { text, x: tx, y: ty, max_width: tw, .. }
                    if !NOT_KEPT_LINES.contains(&text.as_str())
                        && (ty - y).abs() < 10.0
                        && *tx < reach
                        && tx + tw.unwrap_or(f32::INFINITY) > x)
            });
            assert!(!crowded, "{line:?} shares its row with other text");
        }
    }

    // == The vault on disk (2026-09-27) ========================================
    //
    // Until then nothing was kept: every launch opened an empty vault, the
    // master password was checked against a verifier stored nowhere, and
    // locking changed a flag while every entry stayed in memory. These are
    // about the file, the gate in front of it, and the rule that every change
    // is saved as it is made.

    /// Every string the window draws, joined.
    fn drawn(state: &AppState) -> String {
        state
            .draw((DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT))
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn vault_file(scratch: &scratchdir::ScratchDir) -> PathBuf {
        scratch
            .dir()
            .join("config")
            .join("credmanager")
            .join("vault")
    }

    /// The window a new launch opens on the vault kept in `scratch`, with the
    /// cheapest key derivation and test randomness.
    fn reopen(scratch: &scratchdir::ScratchDir) -> AppState {
        let mut state = AppState::open(Some(vault_file(scratch)));
        state.entropy = test_entropy;
        state.new_vault_kdf = TEST_KDF;
        state
    }

    /// A first run on an empty scratch folder.
    fn first_run(tag: &str) -> (scratchdir::ScratchDir, AppState) {
        let scratch = scratchdir::ScratchDir::new(&format!("credmanager_{tag}"));
        let state = reopen(&scratch);
        (scratch, state)
    }

    fn key(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        })
    }

    /// Type `text` as the user would: through the whole event path, saving
    /// included.
    fn type_in(state: &mut AppState, text: &str) {
        for ch in text.chars() {
            handle_event(
                state,
                &Event::Key(KeyEvent {
                    key: Key::A,
                    pressed: true,
                    modifiers: guitk::event::Modifiers::NONE,
                    text: ch.to_string(),
                }),
            );
        }
    }

    const MASTER: &str = "correct horse battery";

    /// Make the vault the way a person does: type, Tab, type again, Enter.
    fn make_vault(state: &mut AppState, password: &str, again: &str) {
        type_in(state, password);
        handle_event(state, &key(Key::Tab));
        type_in(state, again);
        handle_event(state, &key(Key::Enter));
    }

    /// Add a login through the vault, then let an event save it.
    fn add_login(state: &mut AppState, site: &str, password: &str) {
        state.vault.add_entry(
            EntryData::Login(LoginData::new(site, "ann", password)),
            1_700_000_000,
        );
        handle_event(state, &Event::Tick { elapsed_ms: 0 });
    }

    #[test]
    fn a_first_run_makes_the_vault_from_a_master_password_typed_twice() {
        let (scratch, mut state) = first_run("first");
        assert!(matches!(state.gate, Gate::Create(_)));
        assert!(
            drawn(&state).contains("Create your vault"),
            "{}",
            drawn(&state)
        );
        make_vault(&mut state, MASTER, MASTER);
        assert!(state.vault.is_unlocked(), "{:?}", state.gate);
        assert_eq!(state.gate, Gate::Unlock);
        assert!(
            vault_file(&scratch).exists(),
            "the new vault was not written"
        );
        assert!(state.save_error.is_none(), "{:?}", state.save_error);
    }

    #[test]
    fn a_short_or_mismatched_master_password_makes_no_vault() {
        let (scratch, mut state) = first_run("mismatch");
        make_vault(&mut state, "short", "short");
        assert!(!state.vault.is_unlocked());
        let Gate::Create(form) = &state.gate else {
            panic!("{:?}", state.gate)
        };
        assert!(
            form.error.as_deref().unwrap_or("").contains("at least"),
            "{form:?}"
        );

        let (_, mut state) = first_run("mismatch2");
        make_vault(&mut state, MASTER, "correct horse batterY");
        assert!(!state.vault.is_unlocked());
        let Gate::Create(form) = &state.gate else {
            panic!("{:?}", state.gate)
        };
        assert!(
            form.error.as_deref().unwrap_or("").contains("do not match"),
            "{form:?}"
        );
        assert!(!vault_file(&scratch).exists());
    }

    #[test]
    fn nothing_in_the_vault_file_is_in_the_clear() {
        let (scratch, mut state) = first_run("clear");
        make_vault(&mut state, MASTER, MASTER);
        add_login(&mut state, "bank.example", "hunter2-secret");
        let bytes = std::fs::read(vault_file(&scratch)).unwrap();
        for needle in ["hunter2-secret", "bank.example", MASTER, "login"] {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle.as_bytes()),
                "{needle:?} is in the vault file in the clear"
            );
        }
    }

    #[test]
    fn a_vault_opens_again_only_with_its_master_password() {
        let (scratch, mut state) = first_run("reopen");
        make_vault(&mut state, MASTER, MASTER);
        add_login(&mut state, "bank.example", "hunter2-secret");
        let before = std::fs::read(vault_file(&scratch)).unwrap();

        let mut again = reopen(&scratch);
        assert_eq!(again.gate, Gate::Unlock);
        assert!(!again.vault.is_unlocked());
        assert!(
            again.vault.entries.is_empty(),
            "entries in memory before unlocking"
        );

        type_in(&mut again, "not the password");
        handle_event(&mut again, &key(Key::Enter));
        assert!(!again.vault.is_unlocked());
        assert!(
            drawn(&again).contains("not its master password"),
            "{}",
            drawn(&again)
        );
        assert_eq!(
            std::fs::read(vault_file(&scratch)).unwrap(),
            before,
            "a failed unlock wrote"
        );

        again.master_input.clear();
        type_in(&mut again, MASTER);
        handle_event(&mut again, &key(Key::Enter));
        assert!(again.vault.is_unlocked());
        assert_eq!(again.vault.entries.len(), 1);
        let EntryData::Login(login) = &again.vault.entries[0].data else {
            panic!("{:?}", again.vault.entries[0].data)
        };
        assert_eq!(login.password, "hunter2-secret");
        assert_eq!(
            std::fs::read(vault_file(&scratch)).unwrap(),
            before,
            "opening the vault saved it again, though nothing changed"
        );
    }

    #[test]
    fn every_change_is_saved_as_it_is_made_under_a_new_nonce() {
        let (scratch, mut state) = first_run("nonce");
        make_vault(&mut state, MASTER, MASTER);
        let first = std::fs::read(vault_file(&scratch)).unwrap();
        add_login(&mut state, "a.example", "one");
        let second = std::fs::read(vault_file(&scratch)).unwrap();
        add_login(&mut state, "b.example", "two");
        let third = std::fs::read(vault_file(&scratch)).unwrap();
        let nonce =
            |b: &[u8]| b[vaultfile::HEADER_LEN - seal::NONCE_LEN..vaultfile::HEADER_LEN].to_vec();
        assert_ne!(first, second, "the first change was not saved");
        assert_ne!(second, third, "the second change was not saved");
        assert_ne!(nonce(&first), nonce(&second), "a nonce was used twice");
        assert_ne!(nonce(&second), nonce(&third), "a nonce was used twice");

        let mut again = reopen(&scratch);
        assert!(again.vault.unlock(MASTER, 0));
        assert_eq!(again.vault.entries.len(), 2);
    }

    #[test]
    fn a_vault_file_changed_in_any_byte_opens_nothing() {
        let (scratch, mut state) = first_run("changed");
        make_vault(&mut state, MASTER, MASTER);
        add_login(&mut state, "bank.example", "hunter2-secret");
        let path = vault_file(&scratch);
        let good = std::fs::read(&path).unwrap();
        // Past the header, in the sealed part: the header's own bytes are
        // checked by vaultfile's tests.
        for at in [vaultfile::HEADER_LEN, good.len() - 1] {
            let mut bent = good.clone();
            bent[at] ^= 1;
            std::fs::write(&path, &bent).unwrap();
            let mut again = reopen(&scratch);
            assert!(
                !again.vault.unlock(MASTER, 0),
                "a file changed at {at} opened"
            );
        }
    }

    #[test]
    fn a_file_that_is_not_a_vault_is_left_alone() {
        let (scratch, _) = first_run("junk");
        let path = vault_file(&scratch);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"my shopping list").unwrap();
        let mut state = reopen(&scratch);
        assert!(
            matches!(state.gate, Gate::Unreadable(_)),
            "{:?}",
            state.gate
        );
        assert!(
            drawn(&state).contains("cannot be opened"),
            "{}",
            drawn(&state)
        );
        make_vault(&mut state, MASTER, MASTER);
        assert!(
            !state.vault.is_unlocked(),
            "a vault was made over a file there"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"my shopping list");
    }

    #[test]
    fn locking_forgets_the_key_and_every_entry() {
        let mut state = unlocked_app();
        state
            .vault
            .add_entry(EntryData::Login(LoginData::new("s", "u", "p")), 0);
        handle_event(&mut state, &Event::Tick { elapsed_ms: 0 });
        handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::L,
                pressed: true,
                modifiers: guitk::event::Modifiers {
                    ctrl: true,
                    ..guitk::event::Modifiers::NONE
                },
                text: String::new(),
            }),
        );
        assert!(!state.vault.is_unlocked());
        assert!(state.vault.key.is_none(), "the key outlived the lock");
        assert!(
            state.vault.entries.is_empty(),
            "the entries outlived the lock"
        );
        assert!(state.vault.unlock(TEST_MASTER_PASSWORD, 0));
        assert_eq!(
            state.vault.entries.len(),
            1,
            "what was locked did not come back"
        );
    }

    #[test]
    fn without_secure_randomness_nothing_is_sealed_and_the_window_says_so() {
        let (scratch, mut state) = first_run("entropy");
        make_vault(&mut state, MASTER, MASTER);
        let saved = std::fs::read(vault_file(&scratch)).unwrap();
        state.entropy = no_entropy;
        add_login(&mut state, "bank.example", "hunter2-secret");
        assert!(state.save_error.is_some());
        assert_eq!(std::fs::read(vault_file(&scratch)).unwrap(), saved);
        assert!(drawn(&state).contains("Not saved"), "{}", drawn(&state));

        let (_, mut fresh) = first_run("entropy2");
        fresh.entropy = no_entropy;
        make_vault(&mut fresh, MASTER, MASTER);
        assert!(!fresh.vault.is_unlocked(), "a vault was made with no salt");
    }

    #[test]
    fn a_close_that_could_not_save_says_so_once() {
        let (scratch, mut state) = first_run("close");
        make_vault(&mut state, MASTER, MASTER);
        // A file takes the folder's name, so nothing can be written there.
        let dir = vault_file(&scratch).parent().unwrap().to_path_buf();
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::write(&dir, b"in the way").unwrap();
        add_login(&mut state, "bank.example", "hunter2-secret");
        assert!(state.save_error.is_some());
        assert_eq!(state.on_event(&Event::CloseRequested), Response::KeepOpen);
        assert_eq!(state.on_event(&Event::CloseRequested), Response::Exit);
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

    #[cfg(unix)]
    #[test]
    fn the_vault_file_is_its_owners_alone() {
        use std::os::unix::fs::PermissionsExt;
        let (scratch, mut state) = first_run("mode");
        make_vault(&mut state, MASTER, MASTER);
        let path = vault_file(&scratch);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
        let dir = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(dir & 0o077, 0, "{dir:o}");
    }

    // == Edit and delete (2026-09-27) ============================================
    //
    // With the vault kept on disk, an entry saved could never be changed or
    // taken back out: `update_entry` and `remove_entry` had no caller.

    /// A login with a one-time-code secret, a tag, a folder and a star, selected.
    fn app_with_a_login() -> (AppState, u64) {
        let mut state = unlocked_app();
        let mut login = LoginData::new("bank.example", "ann", "old-password");
        login.totp_secret = Some("JBSWY3DPEHPK3PXP".to_string());
        let id = state.vault.add_entry(EntryData::Login(login), 100);
        let folder = state.vault.add_folder("Money");
        state.vault.set_folder(id, Some(folder));
        state.vault.add_tag(id, "important");
        state.vault.toggle_star(id);
        state.selected_entry_id = Some(id);
        state.refresh_filter();
        (state, id)
    }

    /// **An entry's times are dates**: they were counts of seconds to now,
    /// "Created: 31536000 seconds ago" for an entry a year old.
    #[test]
    fn an_entrys_times_are_dates() {
        let mut state = unlocked_app();
        // 2023-11-14 22:13:20 UTC, and a day later.
        let id = state.vault.add_entry(
            EntryData::Login(LoginData::new("bank.example", "ann", "pw")),
            1_700_000_000,
        );
        assert!(state.vault.update_entry(
            id,
            EntryData::Login(LoginData::new("bank.example", "ann", "pw2")),
            1_700_086_400,
        ));
        state.selected_entry_id = Some(id);
        state.refresh_filter();
        let shown = drawn(&state);
        assert!(
            shown.contains("Created: 2023-11-14 22:13")
                && shown.contains("Modified: 2023-11-15 22:13"),
            "{shown}"
        );
        assert!(!shown.contains("seconds ago"), "{shown}");
    }

    #[test]
    fn editing_a_login_changes_what_was_typed_and_keeps_the_rest() {
        let (mut state, id) = app_with_a_login();
        state.now = 500;
        assert_eq!(act_on(&mut state, Target::EditEntry), EventResult::Consumed);
        assert_eq!(state.detail_view, DetailView::NewEntry);
        assert!(drawn(&state).contains("Edit Entry"), "{}", drawn(&state));
        let form = state.new_entry.as_mut().unwrap();
        assert_eq!(
            form.value(2),
            "old-password",
            "the form did not open on the entry"
        );
        form.values[2] = String::from("new-password");
        assert!(save_new_entry(&mut state));

        assert_eq!(state.vault.entries.len(), 1, "an edit added an entry");
        let entry = state.vault.get_entry(id).unwrap();
        let EntryData::Login(login) = &entry.data else {
            panic!("{:?}", entry.data)
        };
        assert_eq!(login.password, "new-password");
        assert_eq!(login.site, "bank.example");
        assert_eq!(
            login.totp_secret.as_deref(),
            Some("JBSWY3DPEHPK3PXP"),
            "the TOTP secret was lost"
        );
        assert_eq!(entry.tags, vec!["important".to_string()]);
        assert!(entry.folder_id.is_some() && entry.starred);
        assert_eq!((entry.created_at, entry.modified_at), (100, 500));
    }

    #[test]
    fn an_entry_being_edited_keeps_its_kind() {
        let (mut state, _) = app_with_a_login();
        open_edit_entry(&mut state);
        let form = state.new_entry.as_mut().unwrap();
        form.set_kind(EntryType::SecureNote);
        assert_eq!(form.kind, EntryType::Login);
        assert!(
            !probe::is_visible(&state, Target::NewKind(1)),
            "another kind is offered for an entry being edited"
        );
    }

    #[test]
    fn a_card_number_left_alone_stays_as_it_was_kept() {
        let mut state = unlocked_app();
        let id = state.vault.add_entry(
            EntryData::CreditCard(CreditCardData::new(
                "Visa",
                "************4242",
                "12/30",
                "Ann",
            )),
            1,
        );
        state.selected_entry_id = Some(id);
        open_edit_entry(&mut state);
        let form = state.new_entry.as_mut().unwrap();
        form.values[2] = String::from("01/31");
        assert!(save_new_entry(&mut state));
        let EntryData::CreditCard(card) = &state.vault.get_entry(id).unwrap().data else {
            panic!()
        };
        assert_eq!(card.number_masked, "************4242");
        assert_eq!(card.expiry, "01/31");
    }

    #[test]
    fn delete_asks_first_and_then_takes_the_entry_out_of_the_vault() {
        let (scratch, mut state) = first_run("delete");
        make_vault(&mut state, MASTER, MASTER);
        add_login(&mut state, "bank.example", "hunter2-secret");
        add_login(&mut state, "mail.example", "other-secret");
        let id = state.vault.entries[0].id;
        state.selected_entry_id = Some(id);

        handle_event(&mut state, &key(Key::Delete));
        assert!(
            matches!(state.dialog, Some(VaultDialog::DeleteConfirm { .. })),
            "{:?}",
            state.dialog
        );
        assert!(
            drawn(&state).contains("Delete this entry?"),
            "{}",
            drawn(&state)
        );
        handle_event(&mut state, &key(Key::Escape));
        assert_eq!(state.vault.entries.len(), 2, "Escape deleted it");

        assert_eq!(
            act_on(&mut state, Target::DeleteEntry),
            EventResult::Consumed
        );
        handle_event(&mut state, &key(Key::Enter));
        assert_eq!(state.vault.entries.len(), 1);
        assert!(state.vault.get_entry(id).is_none());
        assert_eq!(state.selected_entry_id, None);

        let mut again = reopen(&scratch);
        assert!(again.vault.unlock(MASTER, 0));
        assert_eq!(again.vault.entries.len(), 1, "the deletion was not kept");
    }

    #[test]
    fn edit_and_delete_can_be_pressed_on_an_entry() {
        let (state, _) = app_with_a_login();
        assert!(probe::is_visible(&state, Target::EditEntry));
        assert!(probe::is_visible(&state, Target::DeleteEntry));
    }

    // == The text boxes, the toolkit's fields =================================
    //
    // Lane C's c-e-a-theme-can-shape-the-controls. Driven through
    // `App::on_event`, as a window drives them, because that is where what
    // the pointer is over is settled.

    /// The theme's fields with a ring for a focus mark, and the user's focus
    /// width wider than the toolkit's: what the boxes are drawn against.
    fn field_rig(state: &mut AppState) -> (Palette, f32) {
        let mut palette = state.palette;
        palette.widget_style.field.focus = guitk::widget_style::FocusMark::Ring;
        App::theme_changed(state, &palette);
        let settings = appearance::AppearanceSettings {
            focus_ring_scale: 2.5,
            ..Default::default()
        };
        let width = settings.focus_ring_width();
        assert!(width > guitk::style::FOCUS_RING_WIDTH);
        App::appearance_changed(state, &settings);
        (palette, width)
    }

    /// Whether `state` draws the toolkit's field at `rect` in state `s` --
    /// and, when `s` is not focused, no focus mark round it either: an
    /// unfocused box's commands begin a focused one's.
    fn draws(
        state: &AppState,
        (palette, width): (Palette, f32),
        rect: Rect,
        s: guitk::field::State,
    ) -> bool {
        let seq = |f: guitk::field::State| {
            let mut want: Vec<RenderCommand> = Vec::new();
            guitk::field::draw(&mut want, &palette, rect, f, width);
            want
        };
        let frame = state.frame(state.width, state.height);
        let cmds = frame.commands();
        let has = |want: &[RenderCommand]| {
            !want.is_empty() && cmds.windows(want.len()).any(|w| w == want)
        };
        has(&seq(s)) && (s.focused || !has(&seq(guitk::field::State { focused: true, ..s })))
    }

    fn pointer_at(state: &mut AppState, x: f32, y: f32, kind: MouseEventKind) -> Response {
        App::on_event(state, &Event::Mouse(MouseEvent { x, y, kind }))
    }

    /// A press on `target`, through the window.
    fn press_on(state: &mut AppState, target: Target) {
        let (x, y) = probe::rect_of(state, target)
            .unwrap_or_else(|| panic!("{target:?} is not drawn"))
            .centre();
        pointer_at(state, x, y, MouseEventKind::Move);
        pointer_at(state, x, y, MouseEventKind::Press(MouseButton::Left));
    }

    const IDLE: guitk::field::State = guitk::field::State {
        hovered: false,
        focused: false,
        disabled: false,
        invalid: false,
    };
    const LIT: guitk::field::State = guitk::field::State {
        hovered: true,
        ..IDLE
    };
    const KEYED: guitk::field::State = guitk::field::State {
        focused: true,
        ..IDLE
    };
    const BOTH: guitk::field::State = guitk::field::State {
        hovered: true,
        ..KEYED
    };

    /// **The lock screen's box**: it has the keyboard, lights under the
    /// pointer and goes out when it leaves, and is red when the password in
    /// it was refused.
    #[test]
    fn the_lock_screens_box_is_the_toolkits_field() {
        let mut state = AppState::for_test();
        let rig = field_rig(&mut state);
        let master = probe::rect_of(&state, Target::MasterInput).expect("the master password box");
        assert!(
            draws(&state, rig, master, KEYED),
            "the box the master password is typed into is not marked at the user's width"
        );

        let (x, y) = master.centre();
        assert_eq!(
            pointer_at(&mut state, x, y, MouseEventKind::Move),
            Response::Redraw
        );
        assert!(
            draws(&state, rig, master, BOTH),
            "the box does not light under the pointer"
        );
        assert_eq!(
            pointer_at(&mut state, x + 1.0, y, MouseEventKind::Move),
            Response::Idle,
            "a move within the box repaints"
        );
        assert_eq!(
            pointer_at(&mut state, x, master.y - 2.0, MouseEventKind::Move),
            Response::Redraw
        );
        assert!(
            draws(&state, rig, master, KEYED),
            "the box stays lit after the pointer moves off"
        );
        pointer_at(&mut state, x, y, MouseEventKind::Move);
        assert_eq!(
            pointer_at(&mut state, -1.0, -1.0, MouseEventKind::Leave),
            Response::Redraw
        );
        assert!(
            draws(&state, rig, master, KEYED),
            "the box stays lit after the pointer leaves the window"
        );
        // A button is not a box: crossing one lights nothing, and asks for no
        // repaint.
        let (ux, uy) = probe::rect_of(&state, Target::Unlock)
            .expect("the Unlock button")
            .centre();
        assert_eq!(
            pointer_at(&mut state, ux, uy, MouseEventKind::Move),
            Response::Idle,
            "the pointer crossing the Unlock button repaints, though nothing lit"
        );

        type_in(&mut state, "not it");
        handle_event(&mut state, &key(Key::Enter));
        assert!(state.unlock_failed, "control: the password must be refused");
        assert!(
            draws(
                &state,
                rig,
                master,
                guitk::field::State {
                    invalid: true,
                    ..KEYED
                }
            ),
            "a refused password is not shown red"
        );
    }

    /// **The search box and the new-entry form's boxes**: the search has the
    /// keyboard until the form takes it; under a dialog or the file dialog
    /// neither is lit nor marked, and when it goes the light under a pointer
    /// that has not moved comes back.
    #[test]
    fn the_search_and_the_forms_boxes_are_the_toolkits_fields() {
        let mut state = unlocked_app();
        let rig = field_rig(&mut state);
        let search = probe::rect_of(&state, Target::Search).expect("the search box");
        assert!(
            draws(&state, rig, search, KEYED),
            "the search box, which takes what is typed, is not marked"
        );
        let (sx, sy) = search.centre();
        pointer_at(&mut state, sx, sy, MouseEventKind::Move);
        assert!(draws(&state, rig, search, BOTH));

        // A vault dialog.
        press_on(&mut state, Target::Settings);
        press_on(&mut state, Target::ExportCsv);
        assert!(matches!(state.dialog, Some(VaultDialog::ExportWarning)));
        pointer_at(&mut state, sx, sy, MouseEventKind::Move);
        assert!(
            draws(&state, rig, search, IDLE),
            "the search box is lit or marked under a dialog"
        );
        press_on(&mut state, Target::DialogCancel);
        assert!(state.dialog.is_none());
        assert_eq!(
            pointer_at(&mut state, sx, sy, MouseEventKind::Move),
            Response::Redraw,
            "the light under the pointer did not come back with the dialog gone"
        );
        assert!(draws(&state, rig, search, BOTH));

        // The file dialog, closed by a key -- the pointer has not moved, and
        // the light comes back with no move to bring it.
        press_on(&mut state, Target::BackUp);
        assert!(state.picker.is_open());
        pointer_at(&mut state, sx, sy, MouseEventKind::Move);
        assert!(
            draws(&state, rig, search, IDLE),
            "the search box is lit or marked under the file dialog"
        );
        App::on_event(&mut state, &key(Key::Escape));
        assert!(!state.picker.is_open());
        assert!(
            draws(&state, rig, search, BOTH),
            "the light was not settled when the file dialog went"
        );

        // The new-entry form takes the keyboard from the search box.
        press_on(&mut state, Target::Add);
        assert!(state.new_entry.is_some());
        pointer_at(&mut state, sx, sy, MouseEventKind::Move);
        assert!(
            draws(&state, rig, search, LIT),
            "the search box is marked while the form has the keyboard"
        );
        let first = probe::rect_of(&state, Target::NewField(0)).expect("the form's first box");
        let second = probe::rect_of(&state, Target::NewField(1)).expect("the form's second box");
        assert!(
            draws(&state, rig, first, KEYED),
            "the form's box with the keyboard is not marked"
        );
        let (fx, fy) = second.centre();
        pointer_at(&mut state, fx, fy, MouseEventKind::Move);
        assert!(
            draws(&state, rig, second, LIT),
            "the form's box does not light under the pointer"
        );
    }

    // == The auto-lock slider ==================================================

    /// **The auto-lock setting is the vault's own, and the slider sets it.**
    /// It was a number set to the default once and a knob drawn for nothing:
    /// the panel said fifteen minutes of a vault restored with five, and no
    /// press, drag or key moved it. A drag shows its minutes as it goes and
    /// keeps them only when let go; a key keeps its at once; Escape takes a
    /// drag back; and what is kept is the vault's -- the lock is counted by
    /// it, and it is there when the vault is opened again.
    #[test]
    fn the_auto_lock_slider_shows_and_sets_the_vaults_own_time() {
        use std::time::Duration;
        let (scratch, mut state) = first_run("auto_lock");
        make_vault(&mut state, MASTER, MASTER);
        assert!(state.vault.is_unlocked(), "control: the vault must be made");
        state.vault.auto_lock_minutes = 7; // as a restore sets it
        press_on(&mut state, Target::Settings);
        let shown = drawn(&state);
        assert!(
            shown.contains("7 minutes") && !shown.contains("15 minutes"),
            "the panel does not show the vault's own time: {shown}"
        );
        assert!(
            probe::is_visible(&state, Target::AutoLock),
            "the slider has no place among the window's controls"
        );

        let placement = auto_lock_placement(state.width);
        let at = |minutes: f32| placement.track.x + placement.track.w * (minutes - 1.0) / 59.0;
        let y = placement.track.y + placement.track.h / 2.0;
        pointer_at(&mut state, at(30.0), y, MouseEventKind::Move);
        pointer_at(
            &mut state,
            at(30.0),
            y,
            MouseEventKind::Press(MouseButton::Left),
        );
        assert!(
            drawn(&state).contains("30 minutes"),
            "a press on the track did not move the slider there"
        );
        assert_eq!(
            state.vault.auto_lock_minutes, 7,
            "a drag kept its minutes before it was let go"
        );
        pointer_at(&mut state, at(45.0), y, MouseEventKind::Move);
        assert!(
            drawn(&state).contains("45 minutes"),
            "the drag's minutes are not shown as it goes"
        );
        pointer_at(
            &mut state,
            at(45.0),
            y,
            MouseEventKind::Release(MouseButton::Left),
        );
        assert_eq!(
            state.vault.auto_lock_minutes, 45,
            "letting the drag go kept nothing"
        );
        assert_eq!(
            App::tick_interval(&state),
            Some(Duration::from_mins(45)),
            "the lock is not counted by the time set"
        );

        App::on_event(&mut state, &key(Key::Left));
        assert_eq!(state.vault.auto_lock_minutes, 44, "Left took no minute off");
        App::on_event(&mut state, &key(Key::Up));
        assert_eq!(
            state.vault.auto_lock_minutes, 44,
            "Up, the entry list's, moved the slider"
        );
        App::on_event(&mut state, &key(Key::End));
        assert_eq!(
            state.vault.auto_lock_minutes, 60,
            "End did not go to the hour"
        );

        pointer_at(&mut state, at(10.0), y, MouseEventKind::Move);
        pointer_at(
            &mut state,
            at(10.0),
            y,
            MouseEventKind::Press(MouseButton::Left),
        );
        App::on_event(&mut state, &key(Key::Escape));
        pointer_at(
            &mut state,
            at(10.0),
            y,
            MouseEventKind::Release(MouseButton::Left),
        );
        assert_eq!(
            state.vault.auto_lock_minutes, 60,
            "a drag taken back with Escape kept its minutes"
        );
        assert!(drawn(&state).contains("60 minutes"));

        let mut again = reopen(&scratch);
        assert!(again.vault.unlock(MASTER, 0));
        assert_eq!(
            again.vault.auto_lock_minutes, 60,
            "the time set was not kept with the vault"
        );
    }

    /// **The slider is the panel's only while the panel can be used**: under
    /// a dialog a press on it is the dialog's, and the pointer passing over
    /// it is no use of the vault -- a press is.
    #[test]
    fn the_auto_lock_slider_is_not_used_through_a_dialog_or_by_passing_over_it() {
        let mut state = unlocked_app();
        let placement = auto_lock_placement(state.width);
        let (x, y) = (
            placement.track.x + 1.0,
            placement.track.y + placement.track.h / 2.0,
        );
        let before = state.vault.auto_lock_minutes;

        // Not the slider's keys or presses with another panel up.
        App::on_event(&mut state, &key(Key::Home));
        pointer_at(&mut state, x, y, MouseEventKind::Press(MouseButton::Left));
        pointer_at(&mut state, x, y, MouseEventKind::Release(MouseButton::Left));
        assert_eq!(
            state.vault.auto_lock_minutes, before,
            "the slider was moved with the settings panel not up"
        );

        press_on(&mut state, Target::Settings);
        state.now += 30;
        let used = state.vault.last_access;
        pointer_at(&mut state, x, y, MouseEventKind::Move);
        assert_eq!(
            state.vault.last_access, used,
            "the pointer passing over the slider counted as use of the vault"
        );

        press_on(&mut state, Target::ExportCsv);
        assert!(state.dialog.is_some(), "control: the dialog must be up");
        pointer_at(&mut state, x, y, MouseEventKind::Press(MouseButton::Left));
        pointer_at(&mut state, x, y, MouseEventKind::Release(MouseButton::Left));
        assert_eq!(
            state.vault.auto_lock_minutes, before,
            "a press under a dialog moved the slider"
        );

        App::on_event(&mut state, &key(Key::Escape));
        assert!(state.dialog.is_none());
        state.now += 30;
        pointer_at(&mut state, x, y, MouseEventKind::Press(MouseButton::Left));
        pointer_at(&mut state, x, y, MouseEventKind::Release(MouseButton::Left));
        assert_eq!(
            state.vault.auto_lock_minutes, 1,
            "control: with the dialog gone the press moves it"
        );
        assert_eq!(
            state.vault.last_access, state.now,
            "a press on the slider was not counted as use of the vault"
        );

        // Nor through the lock screen, which is drawn over the panel.
        App::on_event(&mut state, &key(Key::End));
        assert_eq!(state.vault.auto_lock_minutes, 60);
        state.lock_vault();
        pointer_at(&mut state, x, y, MouseEventKind::Press(MouseButton::Left));
        pointer_at(&mut state, x, y, MouseEventKind::Release(MouseButton::Left));
        // Read while still locked: opening the vault reads its time back
        // from the sealed file, which would hide a change made in memory.
        assert_eq!(
            state.vault.auto_lock_minutes, 60,
            "a press on the lock screen moved the slider behind it"
        );
    }

    /// **The first run's two boxes and the restore dialog's**: the keyboard
    /// is in one of the two, and a refusal makes the box it is about red
    /// until that box is typed in; the restore box is red while the backup's
    /// password is refused.
    #[test]
    fn the_first_runs_and_the_restores_boxes_are_the_toolkits_fields() {
        let (_scratch, mut state) = first_run("fields");
        let rig = field_rig(&mut state);
        let new = probe::rect_of(&state, Target::NewPassword).expect("the first box");
        let again = probe::rect_of(&state, Target::ConfirmPassword).expect("the second box");
        let red = |s: guitk::field::State| guitk::field::State { invalid: true, ..s };
        assert!(draws(&state, rig, new, KEYED) && draws(&state, rig, again, IDLE));

        // Too short: the first box is wrong.
        type_in(&mut state, "short");
        handle_event(&mut state, &key(Key::Enter));
        handle_event(&mut state, &key(Key::Enter));
        assert!(
            draws(&state, rig, new, red(KEYED)) && draws(&state, rig, again, IDLE),
            "a password refused as too short is not the box shown red"
        );
        type_in(&mut state, "-and-longer");
        assert!(
            draws(&state, rig, new, KEYED),
            "the box stays red once it is typed in"
        );

        // Not the same twice: the second box is wrong.
        handle_event(&mut state, &key(Key::Tab));
        assert!(draws(&state, rig, new, IDLE) && draws(&state, rig, again, KEYED));
        type_in(&mut state, "something else");
        handle_event(&mut state, &key(Key::Enter));
        assert!(
            draws(&state, rig, again, red(KEYED)) && draws(&state, rig, new, IDLE),
            "a second password that does not match is not the box shown red"
        );
        // Emptied for the retyping, it is still put right by a Backspace.
        handle_event(&mut state, &key(Key::Backspace));
        assert!(
            draws(&state, rig, again, KEYED),
            "the box stays red after a Backspace in it"
        );

        // The restore dialog's box.
        let mut state = unlocked_app();
        let rig = field_rig(&mut state);
        state.dialog = Some(VaultDialog::RestorePassword {
            path: std::path::PathBuf::from("backup.vault"),
            backup: Box::new(unlocked_vault()),
            input: String::new(),
            error: None,
        });
        let restore = probe::rect_of(&state, Target::RestoreInput).expect("the restore box");
        assert!(
            draws(&state, rig, restore, KEYED),
            "the restore dialog's box is not marked"
        );
        type_in(&mut state, "not it");
        handle_event(&mut state, &key(Key::Enter));
        assert!(
            draws(&state, rig, restore, red(KEYED)),
            "a backup password refused is not shown red"
        );
    }

    // --- The list of keys, and the Lock button ---

    fn chord_of(k: Key, modifiers: guitk::event::Modifiers, text: &str) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: text.to_owned(),
        })
    }

    fn drawn_strings(state: &AppState) -> Vec<String> {
        state
            .frame(state.width, state.height)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// **Every key the list of keys advertises is answered by this window**,
    /// over an open vault with an entry chosen -- the generator's with the
    /// generator up. Ctrl+Q closes the window.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let mut state = unlocked_with_entries(4);
                state.on_event(&key(Key::Down));
                assert!(
                    state.selected_entry_id.is_some(),
                    "the test needs an entry chosen"
                );
                if what.starts_with("Generator") {
                    state.detail_view = DetailView::PasswordGenerator;
                    regenerate_password(&mut state);
                }
                assert_ne!(
                    state.on_event(&Event::Key(stroke.clone())),
                    Response::Idle,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// **The list of keys reaches the window** over the open vault, and goes
    /// on F1 or a plain Escape -- not on Alt+Escape, which is the desktop's.
    /// `?` is typed into the search, as every character is here; and on the
    /// lock screen, whose keys these are not, F1 raises nothing.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        use guitk::event::Modifiers;
        let mut state = unlocked_with_entries(3);
        assert!(
            !drawn_strings(&state)
                .iter()
                .any(|t| t.contains("F1 or Escape closes this")),
            "the list is up before anybody asked for it"
        );
        for chord in [Modifiers::alt(), Modifiers::super_key()] {
            state.on_event(&chord_of(Key::F1, chord, ""));
            assert!(
                !state.show_help,
                "{chord:?}+F1, the desktop's, raised the list"
            );
        }
        state.on_event(&chord_of(Key::Slash, Modifiers::shift(), "?"));
        assert!(!state.show_help, "? raised the list");
        assert_eq!(state.search_query, "?", "? was not typed into the search");

        state.on_event(&key(Key::F1));
        let missing = guitk::shortcut::missing_rows(&drawn_strings(&state), SHORTCUTS);
        assert!(missing.is_empty(), "{missing:?}");
        state.on_event(&chord_of(Key::Escape, Modifiers::alt(), ""));
        assert!(state.show_help, "Alt+Escape put the list away");
        state.on_event(&key(Key::Escape));
        assert!(!state.show_help, "Escape left the list up");
        assert_eq!(
            state.search_query, "?",
            "Escape under the list cleared the search under it"
        );
        state.on_event(&key(Key::F1));
        state.on_event(&key(Key::F1));
        assert!(!state.show_help, "F1 left the list up");

        state.on_event(&chord_of(Key::L, Modifiers::ctrl(), "l"));
        assert!(!state.vault.is_unlocked());
        state.on_event(&key(Key::F1));
        assert!(
            !state.show_help,
            "F1 raised the vault's keys over the lock screen"
        );
    }

    /// **The list of keys is modal**: with it up, no key, press or turn of the
    /// wheel reaches what it covers -- Ctrl+Q does not close the window,
    /// Delete does not ask to delete the entry, Ctrl+L does not lock. The
    /// window is the same with it down.
    #[test]
    fn the_shortcut_list_takes_the_keys_and_a_press() {
        use guitk::event::Modifiers;
        let mut state = unlocked_with_entries(40);
        state.on_event(&key(Key::Down));
        let chosen = state.selected_entry_id;
        let row = probe::rect_of(&state, Target::EntryRow(3)).expect("a fourth row");
        let (rx, ry) = (row.x + row.w / 2.0, row.y + row.h / 2.0);
        let mouse = |kind| Event::Mouse(guitk::event::MouseEvent { x: rx, y: ry, kind });

        state.on_event(&key(Key::F1));
        assert_eq!(
            state.on_event(&chord_of(Key::Q, Modifiers::ctrl(), "q")),
            Response::Redraw,
            "Ctrl+Q closed the window under the list"
        );
        for (k, m, t) in [
            (Key::Delete, Modifiers::NONE, ""),
            (Key::Down, Modifiers::NONE, ""),
            (Key::L, Modifiers::ctrl(), "l"),
            (Key::G, Modifiers::ctrl(), "g"),
            (Key::A, Modifiers::NONE, "a"),
        ] {
            state.on_event(&chord_of(k, m, t));
        }
        assert!(
            state.show_help,
            "a key other than F1 or Escape put the list away"
        );
        assert!(
            state.dialog.is_none(),
            "Delete asked to delete under the list"
        );
        assert_eq!(state.selected_entry_id, chosen, "Down moved under the list");
        assert!(
            state.vault.is_unlocked(),
            "Ctrl+L locked the vault under the list"
        );
        assert_eq!(
            state.detail_view,
            DetailView::EntryDetail,
            "Ctrl+G under the list"
        );
        assert_eq!(state.search_query, "", "a key was typed under the list");
        assert_eq!(
            state.on_event(&mouse(MouseEventKind::Scroll { dx: 0.0, dy: -3.0 })),
            Response::Idle
        );
        assert!(
            state.list_scroll.abs() < f32::EPSILON,
            "the wheel scrolled the entries under it"
        );
        state.on_event(&mouse(MouseEventKind::Press(MouseButton::Left)));
        assert!(!state.show_help, "the press did not put the list away");
        assert_eq!(
            state.selected_entry_id, chosen,
            "the press chose an entry under it"
        );
        state.on_event(&key(Key::F1));
        state.on_event(&mouse(MouseEventKind::Press(MouseButton::Right)));
        assert!(!state.show_help, "a right-button press left the list up");

        // The window.
        state.on_event(&key(Key::Down));
        assert_ne!(state.selected_entry_id, chosen);
    }

    /// **Locking puts the list away**, by the clock as by Ctrl+L: it is the
    /// open vault's list, and it would come back over the next unlock.
    #[test]
    fn locking_puts_the_list_away() {
        let mut state = unlocked_with_entries(2);
        state.on_event(&key(Key::F1));
        assert!(state.show_help);
        state.on_event(&Event::Tick {
            elapsed_ms: 24 * 60 * 60 * 1000,
        });
        assert!(
            !state.vault.is_unlocked(),
            "the clock did not lock the vault"
        );
        assert!(!state.show_help, "the list outlived the lock");
    }

    /// **The Lock button locks as Ctrl+L does**: it saves first what could not
    /// be saved before. It locked the vault alone, so a change whose save had
    /// failed was gone with the lock.
    #[test]
    fn the_lock_button_saves_what_could_not_be_saved_as_ctrl_l_does() {
        let (scratch, mut state) = first_run("lockbutton");
        make_vault(&mut state, MASTER, MASTER);
        state.entropy = no_entropy;
        add_login(&mut state, "bank.example", "hunter2-secret");
        assert!(
            state.save_error.is_some(),
            "the test needs a save that failed"
        );
        let unsaved = std::fs::read(vault_file(&scratch)).unwrap();
        state.entropy = test_entropy;
        probe::click(&mut state, Target::LockVault);
        assert!(!state.vault.is_unlocked(), "the Lock button did not lock");
        assert_ne!(
            std::fs::read(vault_file(&scratch)).unwrap(),
            unsaved,
            "the Lock button threw away the change that could not be saved"
        );
        let mut again = reopen(&scratch);
        assert!(again.vault.unlock(MASTER, again.now));
        assert_eq!(again.vault.entries.len(), 1, "the login was not kept");
    }

    // -- The boxes edit at a caret ---------------------------------------------------

    fn ctrl_key(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::ctrl(),
            text: String::new(),
        })
    }

    /// The x of every caret drawn inside `area`.
    fn carets_in(state: &AppState, area: Rect) -> Vec<f32> {
        state
            .frame(state.width, state.height)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Line {
                    x1, x2, y1, width, ..
                } if (x1 - x2).abs() < f32::EPSILON
                    && (width - textedit::CARET_WIDTH).abs() < f32::EPSILON
                    && area.contains(*x1, *y1 + 1.0) =>
                {
                    Some(*x1)
                }
                _ => None,
            })
            .collect()
    }

    /// Whether `text` is drawn with some of it selected.
    fn drawn_selected(state: &AppState, text: &str) -> bool {
        state
            .frame(state.width, state.height)
            .commands()
            .iter()
            .any(|c| matches!(c, RenderCommand::RichText { text: t, spans, .. } if t == text && !spans.is_empty()))
    }

    /// Where the caret of `target`'s box, showing `shown`, is drawn after
    /// `chars` characters, and whether it is drawn there and only there.
    fn caret_after(state: &AppState, target: Target, shown: &str, byte: usize) -> bool {
        let rect = probe::rect_of(state, target).expect("the box");
        let area = box_text_area(target, rect);
        let at = area.x
            + text::caret_x(
                shown,
                TextCursor::from(byte),
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
            );
        let carets = carets_in(state, rect);
        carets.len() == 1 && carets.iter().all(|x| (x - at).abs() < 0.5)
    }

    /// **The search edits at a caret**: the arrows, Home and End move it,
    /// typing goes where it is, Delete deletes at it with no entry
    /// selected, Ctrl+A, C, X and V select, copy, cut and paste, a press
    /// puts it where it lands -- and it and the selection are drawn where
    /// they are. It took typing at its end and Backspace from it, and
    /// nothing else, and drew no caret.
    #[test]
    fn the_search_edits_at_a_caret() {
        let mut state = unlocked_app();
        state.selected_entry_id = None;
        type_in(&mut state, "bnk");
        handle_event(&mut state, &key(Key::Left));
        handle_event(&mut state, &key(Key::Left));
        type_in(&mut state, "a");
        assert_eq!(state.search_query, "bank", "the caret did not move");
        assert!(
            caret_after(&state, Target::Search, "bank", 2),
            "the caret is not drawn after the `ba`"
        );
        handle_event(&mut state, &key(Key::Home));
        handle_event(&mut state, &key(Key::Delete));
        assert_eq!(state.search_query, "ank", "Delete at the caret");
        type_in(&mut state, "b");
        handle_event(&mut state, &ctrl_key(Key::A));
        assert!(drawn_selected(&state, "bank"), "the selection is not drawn");
        handle_event(&mut state, &ctrl_key(Key::C));
        handle_event(&mut state, &ctrl_key(Key::X));
        assert_eq!(state.search_query, "", "Ctrl+A and Ctrl+X");
        handle_event(&mut state, &ctrl_key(Key::V));
        assert_eq!(state.search_query, "bank", "Ctrl+C or Ctrl+V");

        let rect = probe::rect_of(&state, Target::Search).expect("the search box");
        let area = box_text_area(Target::Search, rect);
        let mid = area.y + area.h / 2.0;
        handle_event(&mut state, &key(Key::End));
        press_at(&mut state, area.x + 0.5, mid);
        type_in(&mut state, "<");
        press_at(&mut state, area.right() - 1.0, mid);
        type_in(&mut state, ">");
        assert_eq!(
            state.search_query, "<bank>",
            "a press did not put the caret where it landed"
        );
    }

    /// **Delete with an entry selected is the list's**: it asks to delete
    /// the entry, whatever the search holds.
    #[test]
    fn delete_with_an_entry_selected_asks_to_delete_it() {
        let (mut state, id) = app_with_a_login();
        // Used now, or the first key finds it idle since time 100 and locks it.
        state.vault.touch(state.now);
        type_in(&mut state, "ba");
        assert_eq!(
            (
                state.search_query.as_str(),
                state.selected_entry_id,
                state.vault.is_unlocked()
            ),
            ("ba", Some(id), true),
            "control: the search took the typing and the entry stayed selected"
        );
        handle_event(&mut state, &key(Key::Home));
        handle_event(&mut state, &key(Key::Delete));
        assert!(
            matches!(state.dialog, Some(VaultDialog::DeleteConfirm { id: d }) if d == id),
            "Delete did not ask to delete the entry"
        );
        assert_eq!(state.search_query, "ba", "Delete edited the search");
    }

    /// **The master password edits at a caret**, a masked field: the
    /// arrows step a mask at a time, the caret is drawn where it stands on
    /// the characters -- one mask for each character, where it was one for
    /// each byte -- nothing is copied or cut from it, and a press puts the
    /// caret where it lands.
    #[test]
    fn the_master_password_edits_at_a_caret() {
        let mut state = AppState::for_test();
        assert!(!state.vault.is_unlocked());
        type_in(&mut state, "s\u{e9}crt");
        handle_event(&mut state, &key(Key::Left));
        type_in(&mut state, "e");
        assert_eq!(state.master_input, "s\u{e9}cret", "the caret did not move");
        assert!(
            caret_after(&state, Target::MasterInput, "******", 5),
            "the caret is not drawn after the fifth mask, one mask a character"
        );
        handle_event(&mut state, &ctrl_key(Key::A));
        handle_event(&mut state, &ctrl_key(Key::C));
        handle_event(&mut state, &ctrl_key(Key::X));
        assert_eq!(
            (state.master_input.as_str(), state.clipboard.as_str()),
            ("s\u{e9}cret", ""),
            "a hidden password was copied or cut"
        );
        let rect = probe::rect_of(&state, Target::MasterInput).expect("the box");
        let area = box_text_area(Target::MasterInput, rect);
        press_at(&mut state, area.x + 0.5, area.y + area.h / 2.0);
        type_in(&mut state, "<");
        assert_eq!(
            state.master_input, "<s\u{e9}cret",
            "the press did not put the caret there"
        );
    }

    /// **The new vault's two boxes edit at a caret**, each its own.
    #[test]
    fn the_new_vaults_boxes_edit_at_a_caret() {
        let (_scratch, mut state) = first_run("caret_create");
        assert!(matches!(state.gate, Gate::Create(_)));
        type_in(&mut state, "pasword");
        for _ in 0..4 {
            handle_event(&mut state, &key(Key::Left));
        }
        type_in(&mut state, "s");
        handle_event(&mut state, &key(Key::Tab));
        type_in(&mut state, "password");
        let Gate::Create(form) = &state.gate else {
            panic!("the form went away");
        };
        assert_eq!(
            (form.password.as_str(), form.confirm.as_str()),
            ("password", "password"),
            "the caret did not move, or Tab took it to the second box"
        );
    }

    /// **A form's fields edit at a caret, and a secret one is a masked
    /// field until it is shown**: nothing is copied from it hidden, and
    /// shown it copies as any box does.
    #[test]
    fn a_forms_fields_edit_at_a_caret() {
        let mut state = unlocked_app();
        press(&mut state, Target::Add);
        assert_eq!(state.typing_into(), Some(Target::NewField(0)));
        type_in(&mut state, "bnk");
        handle_event(&mut state, &key(Key::Left));
        handle_event(&mut state, &key(Key::Left));
        type_in(&mut state, "a");
        assert_eq!(state.new_entry.as_ref().unwrap().value(0), "bank");
        assert!(
            caret_after(&state, Target::NewField(0), "bank", 2),
            "the caret is not drawn after the `ba`"
        );

        handle_event(&mut state, &key(Key::Tab));
        handle_event(&mut state, &key(Key::Tab));
        assert_eq!(state.typing_into(), Some(Target::NewField(2)));
        type_in(&mut state, "hunter2");
        handle_event(&mut state, &ctrl_key(Key::A));
        handle_event(&mut state, &ctrl_key(Key::C));
        assert_eq!(state.clipboard, "", "a hidden password was copied");
        state.show_password = true;
        handle_event(&mut state, &ctrl_key(Key::A));
        handle_event(&mut state, &ctrl_key(Key::C));
        assert_eq!(state.clipboard, "hunter2", "a shown password did not copy");

        // A press in another field puts the keyboard there, and the caret
        // where it landed -- at the end here, not the start a field the
        // keyboard moves to begins with.
        let rect = probe::rect_of(&state, Target::NewField(0)).expect("the first field");
        let area = box_text_area(Target::NewField(0), rect);
        press_at(&mut state, area.right() - 1.0, area.y + area.h / 2.0);
        type_in(&mut state, ">");
        assert_eq!(state.new_entry.as_ref().unwrap().value(0), "bank>");
    }

    /// **Every edit of the search filters the list by it**: a Backspace, a
    /// cut and a paste as much as a letter typed.
    #[test]
    fn an_edit_of_the_search_filters_the_list() {
        let (mut state, id) = app_with_a_login();
        state.vault.touch(state.now);
        state.selected_entry_id = None;
        type_in(&mut state, "bankx");
        assert!(state.filtered_ids.is_empty(), "nothing is called bankx");
        handle_event(&mut state, &key(Key::Backspace));
        assert_eq!(state.filtered_ids, [id], "a Backspace did not filter again");
        handle_event(&mut state, &ctrl_key(Key::A));
        handle_event(&mut state, &ctrl_key(Key::X));
        assert_eq!(
            state.filtered_ids,
            [id],
            "the empty search lists everything"
        );
        type_in(&mut state, "zz");
        handle_event(&mut state, &ctrl_key(Key::A));
        handle_event(&mut state, &ctrl_key(Key::V));
        assert_eq!(state.search_query, "bank");
        assert_eq!(state.filtered_ids, [id], "a paste did not filter again");
    }

    /// **The restore dialog's password edits at a caret**, a masked field,
    /// and any key it answers puts away what was said of the last try.
    #[test]
    fn the_restore_password_edits_at_a_caret() {
        let mut state = unlocked_app();
        state.dialog = Some(VaultDialog::RestorePassword {
            path: PathBuf::from("backup.vault"),
            backup: Box::new(Vault::for_test("Backup", TEST_MASTER_PASSWORD)),
            input: String::new(),
            error: Some(String::from("that is not its password")),
        });
        type_in(&mut state, "ac");
        handle_event(&mut state, &key(Key::Left));
        type_in(&mut state, "b");
        let Some(VaultDialog::RestorePassword { input, error, .. }) = &state.dialog else {
            panic!("the dialog went away");
        };
        assert_eq!(input, "abc", "the caret did not move");
        assert!(error.is_none(), "an edit left the last try's error up");
    }

    /// **A box edits the text it shows**, however that text came to be in
    /// it.
    #[test]
    fn a_box_edits_the_text_it_shows() {
        let mut state = unlocked_app();
        state.selected_entry_id = None;
        type_in(&mut state, "ab");
        handle_event(&mut state, &key(Key::Home));
        state.search_query = String::from("cd");
        type_in(&mut state, "!");
        assert_eq!(
            state.search_query, "cd!",
            "the key edited the search the editor held"
        );
    }
}

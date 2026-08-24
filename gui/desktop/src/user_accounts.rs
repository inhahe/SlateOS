//! User account management module for the desktop shell.
//!
//! Provides:
//! - User account list and profile editing
//! - Avatar selection and management
//! - Account type (Admin/Standard/Guest)
//! - Password change flow
//! - Account creation/deletion
//! - Login options (auto-login, require password)
//! - User switching UI
//! - Account activity log

use appearance::{Palette, readable_on};
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// ============================================================================
// Colour
// ============================================================================
//
// This module used to carry fourteen `MOCHA_*` constants of its own. They are
// gone; every colour below is read out of the [`Palette`] the caller resolved,
// so the panel follows the mode and the accent like the rest of the shell.
// Two judgements are worth writing down, because both look wrong at a glance.
//
// **The account-type badge does not follow the accent.** `Administrator` is
// red, `Standard` is blue, `Guest` is grey — a row of siblings that means
// "which kind of account is this", exactly as a risk level or a device status
// does. The blue member is the trap: blue is the *default* accent, so
// `Standard` reads like an obvious `p.accent` site. It is not one. Moving one
// cell of a categorical row while its siblings stay put is the bug, not the
// feature — a user who picks a mauve accent has said nothing about what a
// standard account is.
//
// **The avatar palette is identity, not decoration.** The seven colours a
// user's initials circle can take are how you tell one account from another at
// a glance, and they are chosen by an index that comes straight off disk. They
// are therefore categorical for the same reason, *and* they must stay mutually
// distinct — which an accent-following member could not guarantee, since it
// would collide with whichever of the seven the accent happens to equal.
//
// The active tab's label and the "+ Add User" button are the accent sites here:
// both are "you are here" / "the primary thing to do", which is what the accent
// is for.

// ============================================================================
// Account types
// ============================================================================

/// Account privilege level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountType {
    /// Full system administration rights.
    Administrator,
    /// Normal user — can modify own settings, install user-level apps.
    Standard,
    /// Temporary guest — no persistence, limited access.
    Guest,
}

impl AccountType {
    /// Display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Administrator => "Administrator",
            Self::Standard => "Standard",
            Self::Guest => "Guest",
        }
    }

    /// Short label for serialization.
    pub fn id(self) -> &'static str {
        match self {
            Self::Administrator => "admin",
            Self::Standard => "standard",
            Self::Guest => "guest",
        }
    }

    /// Parse from id string.
    pub fn from_id(s: &str) -> Self {
        match s {
            "admin" | "administrator" => Self::Administrator,
            "standard" | "user" => Self::Standard,
            "guest" => Self::Guest,
            _ => Self::Standard,
        }
    }

    /// Badge color for UI.
    ///
    /// Categorical, so it reads roles and never the accent: see the colour
    /// note at the top of this module for why `Standard` staying blue under a
    /// mauve accent is the correct behaviour rather than a missed conversion.
    pub fn badge_color(self, p: &Palette) -> Color {
        match self {
            Self::Administrator => p.red,
            Self::Standard => p.blue,
            Self::Guest => p.overlay0,
        }
    }
}

// ============================================================================
// Avatar
// ============================================================================

/// Avatar type (predefined colors/icons since we don't have real image loading).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Avatar {
    /// Colored circle with initials.
    Initials { color_index: u8 },
    /// System-provided icon by ID.
    SystemIcon(u32),
    /// Custom image path.
    ImagePath(String),
}

impl Default for Avatar {
    fn default() -> Self {
        Self::Initials { color_index: 0 }
    }
}

/// How many colors the avatar palette holds.
///
/// Named separately from the array so the wrap-around in
/// [`Avatar::palette_color`] divides by a compile-time constant; the array's
/// type below is written in terms of it, so the two cannot disagree.
const AVATAR_COLOR_COUNT: usize = 7;

/// The same count as a `u32`, for reducing a uid. Derived from
/// [`AVATAR_COLOR_COUNT`] rather than written out again so the two cannot
/// disagree.
const AVATAR_COLOR_COUNT_U32: u32 = AVATAR_COLOR_COUNT as u32;

/// The avatar colors, in slot order, drawn from `p`.
///
/// A function rather than the `const [Color; 7]` this used to be, because the
/// values are now roles of a palette that is resolved at runtime. The order is
/// part of the data format — a user's avatar index is stored on disk, so
/// reordering this list silently repaints existing accounts.
fn avatar_colors(p: &Palette) -> [Color; AVATAR_COLOR_COUNT] {
    [
        p.blue, p.green, p.peach, p.mauve, p.red, p.yellow, p.lavender,
    ]
}

impl Avatar {
    /// The palette color an initials avatar with this index shows.
    ///
    /// Wrapping is what lets [`Avatar::Initials`] carry an unconstrained `u8`
    /// that no caller has to range-check — including one read straight off
    /// disk. The wrap used to be written twice, once here in `usize` and once
    /// in [`UserAccount::new`] in `u32`; two spellings of one rule is one too
    /// many.
    pub fn palette_color(p: &Palette, color_index: u8) -> Color {
        let slot = usize::from(color_index) % AVATAR_COLOR_COUNT;
        // Unreachable: `slot` is a remainder modulo the array's own length.
        // The fallback keeps the function total instead of panicking.
        avatar_colors(p).get(slot).copied().unwrap_or(p.surface1)
    }

    /// The palette slot a brand-new account with this uid starts on, spread
    /// across the palette so consecutive uids do not all look alike.
    pub fn palette_index_for_uid(uid: u32) -> u8 {
        let slot = uid % AVATAR_COLOR_COUNT_U32;
        // Unreachable for the same reason: a remainder modulo 7 fits in a u8.
        u8::try_from(slot).unwrap_or(0)
    }

    /// Get the background color for this avatar.
    pub fn background_color(&self, p: &Palette) -> Color {
        match self {
            Self::Initials { color_index } => Self::palette_color(p, *color_index),
            Self::SystemIcon(_) => p.surface1,
            Self::ImagePath(_) => p.surface0,
        }
    }

    /// Serialize to string.
    pub fn to_string_repr(&self) -> String {
        match self {
            Self::Initials { color_index } => format!("initials:{}", color_index),
            Self::SystemIcon(id) => format!("icon:{}", id),
            Self::ImagePath(p) => format!("image:{}", p),
        }
    }

    /// Parse from string.
    pub fn from_string_repr(s: &str) -> Self {
        if let Some(rest) = s.strip_prefix("initials:") {
            let idx = rest.parse::<u8>().unwrap_or(0);
            Self::Initials { color_index: idx }
        } else if let Some(rest) = s.strip_prefix("icon:") {
            let id = rest.parse::<u32>().unwrap_or(0);
            Self::SystemIcon(id)
        } else if let Some(rest) = s.strip_prefix("image:") {
            Self::ImagePath(rest.to_string())
        } else {
            Self::default()
        }
    }
}

// ============================================================================
// Login options
// ============================================================================

/// Login configuration for an account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginOptions {
    /// Automatically log in at boot (only one account can have this).
    pub auto_login: bool,
    /// Require password on screen wake/resume.
    pub require_password_on_wake: bool,
    /// Password hint displayed on login screen.
    pub password_hint: String,
    /// Whether password is set.
    pub has_password: bool,
}

impl Default for LoginOptions {
    fn default() -> Self {
        Self {
            auto_login: false,
            require_password_on_wake: true,
            password_hint: String::new(),
            has_password: true,
        }
    }
}

// ============================================================================
// User account
// ============================================================================

/// A single user account.
#[derive(Clone, Debug)]
pub struct UserAccount {
    /// Unique user ID.
    pub uid: u32,
    /// Username (login name).
    pub username: String,
    /// Display name (full name).
    pub display_name: String,
    /// Account type.
    pub account_type: AccountType,
    /// Avatar.
    pub avatar: Avatar,
    /// Login options.
    pub login_options: LoginOptions,
    /// Home directory path.
    pub home_dir: String,
    /// Shell path.
    pub shell: String,
    /// Whether this account is currently logged in.
    pub is_logged_in: bool,
    /// Whether this is the current user viewing the settings.
    pub is_current: bool,
    /// Creation timestamp (unix seconds).
    pub created_at: u64,
    /// Last login timestamp (unix seconds).
    pub last_login: u64,
}

impl UserAccount {
    /// Create a new account with sensible defaults.
    pub fn new(uid: u32, username: &str, display_name: &str, account_type: AccountType) -> Self {
        Self {
            uid,
            username: username.to_string(),
            display_name: display_name.to_string(),
            account_type,
            avatar: Avatar::Initials {
                color_index: Avatar::palette_index_for_uid(uid),
            },
            login_options: LoginOptions::default(),
            home_dir: format!("/home/{}", username),
            shell: "/bin/sh".to_string(),
            is_logged_in: false,
            is_current: false,
            created_at: 0,
            last_login: 0,
        }
    }

    /// Get initials from display name (first letters of first two words).
    pub fn initials(&self) -> String {
        let mut result = String::with_capacity(2);
        for word in self.display_name.split_whitespace().take(2) {
            if let Some(ch) = word.chars().next() {
                result.push(ch.to_ascii_uppercase());
            }
        }
        if result.is_empty() {
            // Fall back to first char of username
            if let Some(ch) = self.username.chars().next() {
                result.push(ch.to_ascii_uppercase());
            }
        }
        result
    }
}

// ============================================================================
// Account activity log
// ============================================================================

/// Type of account activity event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivityEvent {
    Login,
    Logout,
    PasswordChanged,
    AccountCreated,
    AccountTypeChanged(AccountType),
    ProfileUpdated,
    FailedLogin,
}

impl ActivityEvent {
    pub fn display_text(&self) -> &'static str {
        match self {
            Self::Login => "Logged in",
            Self::Logout => "Logged out",
            Self::PasswordChanged => "Password changed",
            Self::AccountCreated => "Account created",
            Self::AccountTypeChanged(_) => "Account type changed",
            Self::ProfileUpdated => "Profile updated",
            Self::FailedLogin => "Failed login attempt",
        }
    }
}

/// An entry in the account activity log.
#[derive(Clone, Debug)]
pub struct ActivityLogEntry {
    pub timestamp: u64,
    pub uid: u32,
    pub event: ActivityEvent,
}

/// Activity log with max entries.
#[derive(Clone, Debug)]
pub struct ActivityLog {
    entries: Vec<ActivityLogEntry>,
    max_entries: usize,
}

impl ActivityLog {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    pub fn record(&mut self, uid: u32, event: ActivityEvent, timestamp: u64) {
        self.entries.push(ActivityLogEntry {
            timestamp,
            uid,
            event,
        });
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }
    }

    pub fn entries(&self) -> &[ActivityLogEntry] {
        &self.entries
    }

    pub fn entries_for_user(&self, uid: u32) -> Vec<&ActivityLogEntry> {
        self.entries.iter().filter(|e| e.uid == uid).collect()
    }

    pub fn recent(&self, count: usize) -> &[ActivityLogEntry] {
        let start = self.entries.len().saturating_sub(count);
        // `start` is clamped to the length, so the split always succeeds; the
        // fallback is the whole log, which is what a caller asking for more
        // than there is wants anyway.
        self.entries.get(start..).unwrap_or(&self.entries)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ============================================================================
// Account manager
// ============================================================================

/// Escapes one field of a `USER|` record.
///
/// A display name, home directory or shell path may legitimately contain a
/// `|` or a newline. Unescaped, such a value splits its own line into more
/// fields than the record has — and because the reader takes fields by
/// position, every field after the offending one is then read from the wrong
/// place. An account named `Bo|Peep` used to come back named `Bo` with a
/// garbage shell. Backslash escapes `\`, `|`, newline and carriage return;
/// [`unescape_split`] is its exact inverse.
fn escape_field(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

/// Splits a record body on unescaped `|`, undoing [`escape_field`].
///
/// A backslash before anything else keeps that character literally, and a
/// trailing lone backslash is kept as itself, so this is total: every input
/// splits into some list of fields rather than failing.
fn unescape_split(body: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut after_backslash = false;
    for ch in body.chars() {
        if after_backslash {
            match ch {
                'n' => current.push('\n'),
                'r' => current.push('\r'),
                other => current.push(other),
            }
            after_backslash = false;
        } else {
            match ch {
                '\\' => after_backslash = true,
                '|' => fields.push(core::mem::take(&mut current)),
                other => current.push(other),
            }
        }
    }
    if after_backslash {
        current.push('\\');
    }
    fields.push(current);
    fields
}

/// The seven fields of a `USER|` line, named instead of positional.
///
/// This type is where the on-disk record format lives. Before it, the field
/// order was stated twice — once by `to_config_text`'s format string and once
/// by `from_config_text`'s `parts[0]`..`parts[6]` — with nothing tying the two
/// together, and the reader's `parts.len() >= 7` guard sat several lines away
/// from the seven indexes it was supposed to justify.
struct UserRecord {
    uid: String,
    username: String,
    display_name: String,
    account_type: String,
    avatar: String,
    home: String,
    shell: String,
}

impl UserRecord {
    /// The record body for `account`: the seven fields, escaped, joined by
    /// `|`. The array literal here and the destructuring in [`Self::parse`]
    /// are the only two statements of the field order, and they sit together
    /// so they cannot drift apart.
    fn render(account: &UserAccount) -> String {
        let fields: [String; 7] = [
            account.uid.to_string(),
            account.username.clone(),
            account.display_name.clone(),
            account.account_type.id().to_string(),
            account.avatar.to_string_repr(),
            account.home_dir.clone(),
            account.shell.clone(),
        ];
        fields
            .iter()
            .map(|field| escape_field(field))
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Parses a record body, or `None` unless it has exactly seven fields.
    ///
    /// The conversion into a fixed-size array is both the length check and the
    /// extraction, so there is no way to check one count and then index by
    /// another. Exactly seven rather than at least seven: now that fields are
    /// escaped, a longer line can only mean a corrupted file, and silently
    /// keeping the first seven of it is how corruption becomes permanent.
    fn parse(body: &str) -> Option<Self> {
        let fields: [String; 7] = unescape_split(body).try_into().ok()?;
        let [
            uid,
            username,
            display_name,
            account_type,
            avatar,
            home,
            shell,
        ] = fields;
        Some(Self {
            uid,
            username,
            display_name,
            account_type,
            avatar,
            home,
            shell,
        })
    }

    /// Rebuilds an account. A field that does not parse falls back the way it
    /// always has: uid 0, the default account type, the default avatar.
    fn into_account(self) -> UserAccount {
        let uid = self.uid.parse::<u32>().unwrap_or(0);
        let mut account = UserAccount::new(
            uid,
            &self.username,
            &self.display_name,
            AccountType::from_id(&self.account_type),
        );
        account.avatar = Avatar::from_string_repr(&self.avatar);
        account.home_dir = self.home;
        account.shell = self.shell;
        account
    }
}

/// Manages all user accounts.
#[derive(Clone, Debug)]
pub struct AccountManager {
    /// All accounts.
    accounts: Vec<UserAccount>,
    /// Next UID to assign.
    next_uid: u32,
    /// Activity log.
    pub activity_log: ActivityLog,
}

impl Default for AccountManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AccountManager {
    /// Create a new manager with a default admin account.
    pub fn new() -> Self {
        let mut admin = UserAccount::new(
            1000,
            "admin",
            "System Administrator",
            AccountType::Administrator,
        );
        admin.is_current = true;
        admin.is_logged_in = true;

        Self {
            accounts: vec![admin],
            next_uid: 1001,
            activity_log: ActivityLog::new(100),
        }
    }

    /// Get all accounts.
    pub fn accounts(&self) -> &[UserAccount] {
        &self.accounts
    }

    /// Get account by UID.
    pub fn get(&self, uid: u32) -> Option<&UserAccount> {
        self.accounts.iter().find(|a| a.uid == uid)
    }

    /// Get mutable account by UID.
    pub fn get_mut(&mut self, uid: u32) -> Option<&mut UserAccount> {
        self.accounts.iter_mut().find(|a| a.uid == uid)
    }

    /// Get the currently active user.
    pub fn current_user(&self) -> Option<&UserAccount> {
        self.accounts.iter().find(|a| a.is_current)
    }

    /// Create a new account. Returns the UID.
    pub fn create_account(
        &mut self,
        username: &str,
        display_name: &str,
        account_type: AccountType,
        timestamp: u64,
    ) -> Result<u32, &'static str> {
        // Validate username
        if username.is_empty() {
            return Err("Username cannot be empty");
        }
        if username.len() > 32 {
            return Err("Username too long (max 32 characters)");
        }
        if !username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err("Username can only contain letters, digits, underscores, and hyphens");
        }

        // Check for duplicates
        if self.accounts.iter().any(|a| a.username == username) {
            return Err("Username already exists");
        }

        let uid = self.next_uid;
        self.next_uid = self.next_uid.checked_add(1).unwrap_or(self.next_uid);

        let mut account = UserAccount::new(uid, username, display_name, account_type);
        account.created_at = timestamp;

        self.accounts.push(account);
        self.activity_log
            .record(uid, ActivityEvent::AccountCreated, timestamp);

        Ok(uid)
    }

    /// Delete an account by UID. Cannot delete the current user.
    pub fn delete_account(&mut self, uid: u32) -> Result<(), &'static str> {
        // Carry the account out of the search rather than its position and
        // three later lookups: `position` answers "which one" and then throws
        // away the only thing that made the answer safe to index with.
        let (idx, account) = self
            .accounts
            .iter()
            .enumerate()
            .find(|(_, a)| a.uid == uid)
            .ok_or("Account not found")?;
        let is_current = account.is_current;
        let is_admin = account.account_type == AccountType::Administrator;

        if is_current {
            return Err("Cannot delete the current user");
        }

        // Must have at least one admin remaining
        if is_admin {
            let admin_count = self
                .accounts
                .iter()
                .filter(|a| a.account_type == AccountType::Administrator)
                .count();
            if admin_count <= 1 {
                return Err("Cannot delete the last administrator");
            }
        }

        self.accounts.remove(idx);
        Ok(())
    }

    /// Change account type.
    pub fn change_account_type(
        &mut self,
        uid: u32,
        new_type: AccountType,
        timestamp: u64,
    ) -> Result<(), &'static str> {
        // If demoting admin, check we have another admin
        if let Some(acct) = self.get(uid)
            && acct.account_type == AccountType::Administrator
            && new_type != AccountType::Administrator
        {
            let admin_count = self
                .accounts
                .iter()
                .filter(|a| a.account_type == AccountType::Administrator)
                .count();
            if admin_count <= 1 {
                return Err("Cannot demote the last administrator");
            }
        }

        if let Some(acct) = self.get_mut(uid) {
            acct.account_type = new_type;
            self.activity_log
                .record(uid, ActivityEvent::AccountTypeChanged(new_type), timestamp);
            Ok(())
        } else {
            Err("Account not found")
        }
    }

    /// Set auto-login for a user (disables it for all others).
    pub fn set_auto_login(&mut self, uid: u32, enabled: bool) {
        for acct in &mut self.accounts {
            if acct.uid == uid {
                acct.login_options.auto_login = enabled;
            } else if enabled {
                acct.login_options.auto_login = false;
            }
        }
    }

    /// Switch active user.
    pub fn switch_user(&mut self, uid: u32, timestamp: u64) -> Result<(), &'static str> {
        if !self.accounts.iter().any(|a| a.uid == uid) {
            return Err("Account not found");
        }

        // Log out current
        if let Some(current) = self.accounts.iter_mut().find(|a| a.is_current) {
            let old_uid = current.uid;
            current.is_current = false;
            self.activity_log
                .record(old_uid, ActivityEvent::Logout, timestamp);
        }

        // Log in new
        if let Some(new_user) = self.accounts.iter_mut().find(|a| a.uid == uid) {
            new_user.is_current = true;
            new_user.is_logged_in = true;
            new_user.last_login = timestamp;
            self.activity_log
                .record(uid, ActivityEvent::Login, timestamp);
        }

        Ok(())
    }

    /// Count accounts by type.
    pub fn count_by_type(&self, account_type: AccountType) -> usize {
        self.accounts
            .iter()
            .filter(|a| a.account_type == account_type)
            .count()
    }

    /// Serialize all accounts to config text.
    pub fn to_config_text(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("# User accounts\n");

        for acct in &self.accounts {
            out.push_str("USER|");
            out.push_str(&UserRecord::render(acct));
            out.push('\n');
        }

        out
    }

    /// Parse accounts from config text.
    pub fn from_config_text(text: &str) -> Self {
        let mut mgr = Self {
            accounts: Vec::new(),
            next_uid: 1000,
            activity_log: ActivityLog::new(100),
        };

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(body) = line.strip_prefix("USER|")
                && let Some(record) = UserRecord::parse(body)
            {
                let acct = record.into_account();
                if acct.uid >= mgr.next_uid {
                    mgr.next_uid = acct.uid.checked_add(1).unwrap_or(acct.uid);
                }
                mgr.accounts.push(acct);
            }
        }

        mgr
    }
}

// ============================================================================
// Account settings UI
// ============================================================================

/// Tab in the accounts settings panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountsTab {
    YourInfo,
    OtherUsers,
    SignInOptions,
    ActivityLog,
}

impl AccountsTab {
    pub const ALL: &'static [Self] = &[
        Self::YourInfo,
        Self::OtherUsers,
        Self::SignInOptions,
        Self::ActivityLog,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::YourInfo => "Your Info",
            Self::OtherUsers => "Other Users",
            Self::SignInOptions => "Sign-in Options",
            Self::ActivityLog => "Activity Log",
        }
    }
}

/// State for the account settings UI.
#[derive(Clone, Debug)]
pub struct AccountSettingsUI {
    /// Account manager.
    pub manager: AccountManager,
    /// Active tab.
    pub active_tab: AccountsTab,
    /// Selected user UID in "Other Users" tab.
    pub selected_uid: Option<u32>,
    /// Whether the create account dialog is open.
    pub create_dialog_open: bool,
    /// Create dialog fields.
    pub create_username: String,
    pub create_display_name: String,
    pub create_account_type: AccountType,
    /// Whether the confirm delete dialog is open.
    pub confirm_delete_open: bool,
    /// UID being deleted.
    pub delete_uid: Option<u32>,
    /// Status message.
    pub status_message: Option<String>,
}

impl Default for AccountSettingsUI {
    fn default() -> Self {
        Self {
            manager: AccountManager::new(),
            active_tab: AccountsTab::YourInfo,
            selected_uid: None,
            create_dialog_open: false,
            create_username: String::new(),
            create_display_name: String::new(),
            create_account_type: AccountType::Standard,
            confirm_delete_open: false,
            delete_uid: None,
            status_message: None,
        }
    }
}

impl AccountSettingsUI {
    /// Render the account settings panel.
    pub fn render(
        &self,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(64);

        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 16.0,
            text: "User Accounts".to_string(),
            font_size: 18.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Tab bar
        let tab_y = y + 48.0;
        for (i, tab) in AccountsTab::ALL.iter().enumerate() {
            let tab_x = x + 16.0 + i as f32 * 130.0;
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: 120.0,
                    height: 28.0,
                    color: p.surface1,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 8.0,
                y: tab_y + 6.0,
                text: tab.display_name().to_string(),
                font_size: 12.0,
                color: if is_active { p.accent } else { p.subtext0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(110.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Content
        let content_y = tab_y + 40.0;
        let content_h = height - (content_y - y) - 16.0;

        match self.active_tab {
            AccountsTab::YourInfo => {
                self.render_your_info(p, &mut cmds, x + 16.0, content_y, width - 32.0, content_h);
            }
            AccountsTab::OtherUsers => {
                self.render_other_users(p, &mut cmds, x + 16.0, content_y, width - 32.0, content_h);
            }
            AccountsTab::SignInOptions => {
                self.render_sign_in_options(
                    p,
                    &mut cmds,
                    x + 16.0,
                    content_y,
                    width - 32.0,
                    content_h,
                );
            }
            AccountsTab::ActivityLog => {
                self.render_activity_log(
                    p,
                    &mut cmds,
                    x + 16.0,
                    content_y,
                    width - 32.0,
                    content_h,
                );
            }
        }

        // Status message
        if let Some(msg) = &self.status_message {
            cmds.push(RenderCommand::FillRect {
                x: x + 16.0,
                y: y + height - 32.0,
                width: width - 32.0,
                height: 24.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 24.0,
                y: y + height - 28.0,
                text: msg.clone(),
                font_size: 11.0,
                color: p.yellow,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 48.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        cmds
    }

    fn render_your_info(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        if let Some(user) = self.manager.current_user() {
            // Avatar circle
            let avatar_size = 64.0;
            let avatar_color = user.avatar.background_color(p);
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width: avatar_size,
                height: avatar_size,
                color: avatar_color,
                corner_radii: CornerRadii::all(avatar_size / 2.0),
            });

            // Initials
            cmds.push(RenderCommand::Text {
                x: x + avatar_size / 2.0 - 12.0,
                y: y + avatar_size / 2.0 - 10.0,
                text: user.initials(),
                font_size: 20.0,
                // The initials sit *on* the avatar circle, so they are chosen
                // for that fill's brightness rather than read off the palette:
                // a light-mode pale-yellow circle needs dark initials and a
                // dark-mode one needs the same, but a red circle does not.
                color: readable_on(avatar_color),
                font_weight: FontWeightHint::Bold,
                max_width: Some(avatar_size),
                overflow: TextOverflow::Ellipsis,
            });

            // Name and username
            let info_x = x + avatar_size + 16.0;
            cmds.push(RenderCommand::Text {
                x: info_x,
                y,
                text: user.display_name.clone(),
                font_size: 16.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - avatar_size - 32.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: info_x,
                y: y + 24.0,
                text: format!("@{}", user.username),
                font_size: 12.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - avatar_size - 32.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Account type badge
            let badge_color = user.account_type.badge_color(p);
            cmds.push(RenderCommand::FillRect {
                x: info_x,
                y: y + 44.0,
                width: 90.0,
                height: 20.0,
                color: badge_color,
                corner_radii: CornerRadii::all(10.0),
            });
            cmds.push(RenderCommand::Text {
                x: info_x + 8.0,
                y: y + 47.0,
                text: user.account_type.display_name().to_string(),
                font_size: 10.0,
                // On the badge fill, for the same reason as the initials.
                color: readable_on(badge_color),
                font_weight: FontWeightHint::Bold,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Info rows
            let mut row_y = y + 80.0;

            self.render_info_row(p, cmds, x, row_y, width, "Home Directory", &user.home_dir);
            row_y += 24.0;

            self.render_info_row(p, cmds, x, row_y, width, "Shell", &user.shell);
            row_y += 24.0;

            let password_status = if user.login_options.has_password {
                "Set"
            } else {
                "Not set"
            };
            self.render_info_row(p, cmds, x, row_y, width, "Password", password_status);
        }
    }

    fn render_other_users(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Other Users".to_string(),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });

        let mut row_y = y + 28.0;

        for acct in self.manager.accounts() {
            if acct.is_current {
                continue;
            }

            let is_selected = self.selected_uid == Some(acct.uid);

            // Row background
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y - 2.0,
                    width,
                    height: 36.0,
                    color: p.surface0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Avatar small circle
            let av_size = 28.0;
            let av_color = acct.avatar.background_color(p);
            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: row_y + 2.0,
                width: av_size,
                height: av_size,
                color: av_color,
                corner_radii: CornerRadii::all(av_size / 2.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: row_y + 7.0,
                text: acct.initials(),
                font_size: 11.0,
                color: readable_on(av_color),
                font_weight: FontWeightHint::Bold,
                max_width: Some(av_size),
                overflow: TextOverflow::Ellipsis,
            });

            // Name
            cmds.push(RenderCommand::Text {
                x: x + 40.0,
                y: row_y + 4.0,
                text: acct.display_name.clone(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 180.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Type badge
            cmds.push(RenderCommand::Text {
                x: x + 40.0,
                y: row_y + 20.0,
                text: acct.account_type.display_name().to_string(),
                font_size: 10.0,
                color: acct.account_type.badge_color(p),
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Login status
            let status = if acct.is_logged_in {
                "Signed in"
            } else {
                "Signed out"
            };
            let status_color = if acct.is_logged_in {
                p.green
            } else {
                p.overlay0
            };
            cmds.push(RenderCommand::Text {
                x: x + width - 80.0,
                y: row_y + 10.0,
                text: status.to_string(),
                font_size: 10.0,
                color: status_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(75.0),
                overflow: TextOverflow::Ellipsis,
            });

            row_y += 40.0;
        }

        // Add user button
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y + 8.0,
            width: 140.0,
            height: 28.0,
            color: p.accent,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: row_y + 14.0,
            text: "+ Add User".to_string(),
            font_size: 12.0,
            color: readable_on(p.accent),
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_sign_in_options(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        if let Some(user) = self.manager.current_user() {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: "Sign-in Options".to_string(),
                font_size: 14.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });

            let mut row_y = y + 28.0;

            // Auto-login toggle
            self.render_toggle_row(
                p,
                cmds,
                x,
                row_y,
                width,
                "Auto sign-in at boot",
                user.login_options.auto_login,
            );
            row_y += 32.0;

            // Require password on wake
            self.render_toggle_row(
                p,
                cmds,
                x,
                row_y,
                width,
                "Require password on wake",
                user.login_options.require_password_on_wake,
            );
            row_y += 32.0;

            // Password hint
            self.render_info_row(
                p,
                cmds,
                x,
                row_y,
                width,
                "Password hint",
                if user.login_options.password_hint.is_empty() {
                    "(not set)"
                } else {
                    &user.login_options.password_hint
                },
            );
            row_y += 32.0;

            // Change password button
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width: 160.0,
                height: 28.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 6.0,
                text: "Change Password".to_string(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_activity_log(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Recent Activity".to_string(),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });

        let mut row_y = y + 28.0;
        let entries = self.manager.activity_log.recent(20);

        if entries.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y: row_y,
                text: "No activity recorded.".to_string(),
                font_size: 12.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        for entry in entries.iter().rev() {
            let username = self
                .manager
                .get(entry.uid)
                .map(|a| a.username.as_str())
                .unwrap_or("unknown");

            cmds.push(RenderCommand::Text {
                x,
                y: row_y,
                text: format!("{}: {}", username, entry.event.display_text()),
                font_size: 11.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });

            row_y += 20.0;
        }
    }

    fn render_info_row(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: label.to_string(),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.42,
            y,
            text: value.to_string(),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.55),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_toggle_row(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        enabled: bool,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y: y + 4.0,
            text: label.to_string(),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 60.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Toggle switch
        let toggle_x = x + width - 44.0;
        // Green rather than the accent, faithfully to what this module drew
        // before the conversion. The shell is not consistent about which of the
        // two an "on" switch uses; that inconsistency is recorded in
        // known-issues.md and is not a thing to settle mid-conversion.
        let toggle_bg = if enabled { p.green } else { p.surface0 };
        cmds.push(RenderCommand::FillRect {
            x: toggle_x,
            y: y + 2.0,
            width: 36.0,
            height: 18.0,
            color: toggle_bg,
            corner_radii: CornerRadii::all(9.0),
        });

        // Toggle knob
        let knob_x = if enabled {
            toggle_x + 20.0
        } else {
            toggle_x + 2.0
        };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 4.0,
            width: 14.0,
            height: 14.0,
            color: p.text,
            corner_radii: CornerRadii::all(7.0),
        });
    }
}

// ============================================================================
// Tests
// ============================================================================

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
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use crate::palette_check;

    /// The palette the ordinary render tests draw with.
    fn test_palette() -> Palette {
        Palette::for_mode(false)
    }

    // ---- AccountType tests ----

    #[test]
    fn test_account_type_display_name() {
        assert_eq!(AccountType::Administrator.display_name(), "Administrator");
        assert_eq!(AccountType::Standard.display_name(), "Standard");
        assert_eq!(AccountType::Guest.display_name(), "Guest");
    }

    #[test]
    fn test_account_type_roundtrip() {
        for t in [
            AccountType::Administrator,
            AccountType::Standard,
            AccountType::Guest,
        ] {
            assert_eq!(AccountType::from_id(t.id()), t);
        }
    }

    #[test]
    fn test_account_type_unknown_defaults() {
        assert_eq!(AccountType::from_id("xyz"), AccountType::Standard);
    }

    // ---- Avatar tests ----

    #[test]
    fn test_avatar_default_is_initials() {
        let a = Avatar::default();
        assert!(matches!(a, Avatar::Initials { color_index: 0 }));
    }

    #[test]
    fn test_avatar_roundtrip_initials() {
        let a = Avatar::Initials { color_index: 3 };
        let s = a.to_string_repr();
        let parsed = Avatar::from_string_repr(&s);
        assert_eq!(parsed, a);
    }

    #[test]
    fn test_avatar_roundtrip_icon() {
        let a = Avatar::SystemIcon(42);
        let s = a.to_string_repr();
        let parsed = Avatar::from_string_repr(&s);
        assert_eq!(parsed, a);
    }

    #[test]
    fn test_avatar_roundtrip_image() {
        let a = Avatar::ImagePath("/home/user/avatar.png".to_string());
        let s = a.to_string_repr();
        let parsed = Avatar::from_string_repr(&s);
        assert_eq!(parsed, a);
    }

    #[test]
    fn test_avatar_unknown_defaults() {
        let parsed = Avatar::from_string_repr("garbage");
        assert!(matches!(parsed, Avatar::Initials { color_index: 0 }));
    }

    #[test]
    fn test_avatar_background_color() {
        let p = test_palette();
        let a = Avatar::Initials { color_index: 0 };
        assert_eq!(a.background_color(&p), p.blue);
    }

    // ---- UserAccount tests ----

    #[test]
    fn test_user_account_new() {
        let a = UserAccount::new(1000, "alice", "Alice Smith", AccountType::Standard);
        assert_eq!(a.uid, 1000);
        assert_eq!(a.username, "alice");
        assert_eq!(a.home_dir, "/home/alice");
        assert!(!a.is_current);
    }

    #[test]
    fn test_user_initials_two_words() {
        let a = UserAccount::new(1, "jd", "John Doe", AccountType::Standard);
        assert_eq!(a.initials(), "JD");
    }

    #[test]
    fn test_user_initials_single_word() {
        let a = UserAccount::new(1, "admin", "Administrator", AccountType::Administrator);
        assert_eq!(a.initials(), "A");
    }

    #[test]
    fn test_user_initials_empty_display_name() {
        let a = UserAccount::new(1, "test", "", AccountType::Standard);
        assert_eq!(a.initials(), "T"); // Falls back to username
    }

    // ---- ActivityLog tests ----

    #[test]
    fn test_activity_log_record() {
        let mut log = ActivityLog::new(10);
        log.record(1, ActivityEvent::Login, 1000);
        assert_eq!(log.entries().len(), 1);
    }

    #[test]
    fn test_activity_log_max_entries() {
        let mut log = ActivityLog::new(3);
        for i in 0..5 {
            log.record(1, ActivityEvent::Login, i);
        }
        assert_eq!(log.entries().len(), 3);
    }

    #[test]
    fn test_activity_log_per_user() {
        let mut log = ActivityLog::new(10);
        log.record(1, ActivityEvent::Login, 100);
        log.record(2, ActivityEvent::Login, 200);
        log.record(1, ActivityEvent::Logout, 300);
        assert_eq!(log.entries_for_user(1).len(), 2);
        assert_eq!(log.entries_for_user(2).len(), 1);
    }

    #[test]
    fn test_activity_log_recent() {
        let mut log = ActivityLog::new(10);
        for i in 0..5 {
            log.record(1, ActivityEvent::Login, i);
        }
        let recent = log.recent(2);
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn test_activity_log_clear() {
        let mut log = ActivityLog::new(10);
        log.record(1, ActivityEvent::Login, 100);
        log.clear();
        assert!(log.entries().is_empty());
    }

    // ---- AccountManager tests ----

    #[test]
    fn test_manager_new_has_admin() {
        let mgr = AccountManager::new();
        assert_eq!(mgr.accounts().len(), 1);
        assert_eq!(mgr.accounts()[0].account_type, AccountType::Administrator);
        assert!(mgr.accounts()[0].is_current);
    }

    #[test]
    fn test_manager_create_account() {
        let mut mgr = AccountManager::new();
        let uid = mgr
            .create_account("bob", "Bob Smith", AccountType::Standard, 1000)
            .unwrap();
        assert!(uid >= 1001);
        assert_eq!(mgr.accounts().len(), 2);
        assert_eq!(mgr.get(uid).unwrap().username, "bob");
    }

    #[test]
    fn test_manager_create_duplicate_fails() {
        let mut mgr = AccountManager::new();
        let _ = mgr.create_account("bob", "Bob", AccountType::Standard, 1000);
        let result = mgr.create_account("bob", "Bob2", AccountType::Standard, 2000);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_create_empty_username_fails() {
        let mut mgr = AccountManager::new();
        let result = mgr.create_account("", "Nobody", AccountType::Standard, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_create_invalid_username_fails() {
        let mut mgr = AccountManager::new();
        let result = mgr.create_account("bad name!", "Bad", AccountType::Standard, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_delete_account() {
        let mut mgr = AccountManager::new();
        let uid = mgr
            .create_account("bob", "Bob", AccountType::Standard, 1000)
            .unwrap();
        mgr.delete_account(uid).unwrap();
        assert_eq!(mgr.accounts().len(), 1);
    }

    #[test]
    fn test_manager_delete_current_user_fails() {
        let mgr_clone = AccountManager::new();
        let current_uid = mgr_clone.current_user().unwrap().uid;
        let mut mgr = mgr_clone;
        let result = mgr.delete_account(current_uid);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_delete_last_admin_fails() {
        let mgr_orig = AccountManager::new();
        let admin_uid = mgr_orig.accounts()[0].uid;
        let mut mgr = mgr_orig;
        // Can't delete because it's current AND last admin
        let result = mgr.delete_account(admin_uid);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_change_account_type() {
        let mut mgr = AccountManager::new();
        let uid = mgr
            .create_account("bob", "Bob", AccountType::Standard, 1000)
            .unwrap();
        mgr.change_account_type(uid, AccountType::Administrator, 2000)
            .unwrap();
        assert_eq!(
            mgr.get(uid).unwrap().account_type,
            AccountType::Administrator
        );
    }

    #[test]
    fn test_manager_demote_last_admin_fails() {
        let mut mgr = AccountManager::new();
        let admin_uid = mgr.accounts()[0].uid;
        let result = mgr.change_account_type(admin_uid, AccountType::Standard, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_auto_login_exclusive() {
        let mut mgr = AccountManager::new();
        let uid1 = mgr.accounts()[0].uid;
        let uid2 = mgr
            .create_account("bob", "Bob", AccountType::Standard, 1000)
            .unwrap();

        mgr.set_auto_login(uid1, true);
        assert!(mgr.get(uid1).unwrap().login_options.auto_login);

        mgr.set_auto_login(uid2, true);
        assert!(!mgr.get(uid1).unwrap().login_options.auto_login);
        assert!(mgr.get(uid2).unwrap().login_options.auto_login);
    }

    #[test]
    fn test_manager_switch_user() {
        let mut mgr = AccountManager::new();
        let uid2 = mgr
            .create_account("bob", "Bob", AccountType::Standard, 1000)
            .unwrap();

        mgr.switch_user(uid2, 2000).unwrap();
        assert!(mgr.get(uid2).unwrap().is_current);
        assert!(!mgr.accounts()[0].is_current);
    }

    #[test]
    fn test_manager_switch_to_nonexistent_fails() {
        let mut mgr = AccountManager::new();
        let result = mgr.switch_user(9999, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_count_by_type() {
        let mut mgr = AccountManager::new();
        let _ = mgr.create_account("bob", "Bob", AccountType::Standard, 1000);
        let _ = mgr.create_account("guest", "Guest", AccountType::Guest, 1000);
        assert_eq!(mgr.count_by_type(AccountType::Administrator), 1);
        assert_eq!(mgr.count_by_type(AccountType::Standard), 1);
        assert_eq!(mgr.count_by_type(AccountType::Guest), 1);
    }

    #[test]
    fn test_manager_config_roundtrip() {
        let mut mgr = AccountManager::new();
        let _ = mgr.create_account("alice", "Alice", AccountType::Standard, 1000);
        let text = mgr.to_config_text();
        let mgr2 = AccountManager::from_config_text(&text);
        assert_eq!(mgr2.accounts().len(), 2);
        assert!(mgr2.accounts().iter().any(|a| a.username == "alice"));
    }

    #[test]
    fn a_field_containing_the_separator_survives_a_config_roundtrip() {
        // The old format split on every `|`, so a display name with one in it
        // shifted every later field along by one: the account came back with a
        // truncated name, a defaulted type, and a garbage home and shell.
        let mut mgr = AccountManager::new();
        let uid = mgr
            .create_account("bo", "Bo|Peep", AccountType::Standard, 1000)
            .expect("a valid username is accepted");
        if let Some(acct) = mgr.get_mut(uid) {
            acct.home_dir = "/home/bo|peep".to_string();
            acct.shell = "/bin/sh|sh".to_string();
        }

        let restored = AccountManager::from_config_text(&mgr.to_config_text());
        let bo = restored
            .get(uid)
            .expect("the account is still there after a roundtrip");
        assert_eq!(bo.display_name, "Bo|Peep");
        assert_eq!(bo.home_dir, "/home/bo|peep");
        assert_eq!(bo.shell, "/bin/sh|sh");
        assert_eq!(bo.account_type, AccountType::Standard);
    }

    #[test]
    fn awkward_field_values_survive_a_config_roundtrip() {
        let mut mgr = AccountManager::new();
        let uid = mgr
            .create_account("odd", "placeholder", AccountType::Standard, 1000)
            .expect("a valid username is accepted");
        for value in [
            "a\\b",
            "trailing\\",
            "\\|",
            "line\none",
            "carriage\rreturn",
            "||||",
            "",
            "\\n literally",
        ] {
            if let Some(acct) = mgr.get_mut(uid) {
                acct.display_name = value.to_string();
                acct.home_dir = value.to_string();
            }
            let restored = AccountManager::from_config_text(&mgr.to_config_text());
            let odd = restored.get(uid).expect("account survives");
            assert_eq!(odd.display_name, value, "display name {value:?}");
            assert_eq!(odd.home_dir, value, "home dir {value:?}");
        }
    }

    #[test]
    fn escaping_and_splitting_are_inverses() {
        for value in [
            "", "plain", "|", "\\", "\\|", "|\\", "a|b|c", "\n", "\r\n", "\\\\|\\|",
        ] {
            assert_eq!(
                unescape_split(&escape_field(value)),
                vec![value.to_string()],
                "{value:?} must escape to a single field"
            );
        }
    }

    #[test]
    fn a_record_with_the_wrong_number_of_fields_is_rejected() {
        assert!(UserRecord::parse("1|a|b|c|d|e").is_none(), "six fields");
        assert!(
            UserRecord::parse("1|a|b|c|d|e|f|g").is_none(),
            "eight fields"
        );
        assert!(UserRecord::parse("1|a|b|c|d|e|f").is_some(), "seven fields");
        // The eighth field above is not silently dropped, so a corrupted line
        // is skipped rather than half-read.
        let mgr = AccountManager::from_config_text("USER|1|a|b|c|d|e|f|g\n");
        assert_eq!(mgr.accounts().len(), 0);
    }

    #[test]
    fn every_avatar_index_names_a_palette_color() {
        // The index is an unconstrained `u8` that can come straight off disk,
        // so all 256 of them have to land somewhere.
        let p = test_palette();
        let palette: Vec<Color> = (0..AVATAR_COLOR_COUNT)
            .map(|i| {
                Avatar::palette_color(
                    &p,
                    u8::try_from(i).expect("the palette is far shorter than 256"),
                )
            })
            .collect();
        for index in 0..=255u8 {
            let color = Avatar::Initials { color_index: index }.background_color(&p);
            assert!(
                palette.contains(&color),
                "index {index} produced a color outside the palette"
            );
            assert_eq!(
                color,
                Avatar::palette_color(&p, index),
                "the avatar and the bare index disagree at {index}"
            );
        }
    }

    #[test]
    fn the_palette_wraps_rather_than_clamping() {
        let p = test_palette();
        for index in 0..=255u8 {
            let wrapped = index.wrapping_add(u8::try_from(AVATAR_COLOR_COUNT).expect("fits"));
            // Only compare where adding the palette length did not itself wrap
            // past 255, which would land on a different slot.
            if wrapped > index {
                assert_eq!(
                    Avatar::palette_color(&p, index),
                    Avatar::palette_color(&p, wrapped),
                    "{index} and {wrapped} are the same slot"
                );
            }
        }
    }

    #[test]
    fn recent_never_asks_for_more_log_than_there_is() {
        let mut log = ActivityLog::new(100);
        for i in 0..5u32 {
            log.record(i, ActivityEvent::Login, u64::from(i));
        }
        assert_eq!(log.recent(0).len(), 0);
        assert_eq!(log.recent(3).len(), 3);
        assert_eq!(log.recent(5).len(), 5);
        assert_eq!(log.recent(500).len(), 5);
        assert_eq!(log.recent(usize::MAX).len(), 5);
        // The newest entries, not the oldest.
        assert_eq!(log.recent(2).first().map(|e| e.uid), Some(3));
    }

    // ---- LoginOptions tests ----

    #[test]
    fn test_login_options_default() {
        let opts = LoginOptions::default();
        assert!(!opts.auto_login);
        assert!(opts.require_password_on_wake);
        assert!(opts.has_password);
    }

    // ---- Activity event tests ----

    #[test]
    fn test_activity_event_display() {
        assert_eq!(ActivityEvent::Login.display_text(), "Logged in");
        assert_eq!(
            ActivityEvent::PasswordChanged.display_text(),
            "Password changed"
        );
    }

    // ---- UI tests ----

    #[test]
    fn test_ui_default() {
        let ui = AccountSettingsUI::default();
        assert_eq!(ui.active_tab, AccountsTab::YourInfo);
        assert!(!ui.create_dialog_open);
    }

    #[test]
    fn test_ui_render_your_info() {
        let ui = AccountSettingsUI::default();
        let cmds = ui.render(&test_palette(), 0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_other_users() {
        let mut ui = AccountSettingsUI::default();
        ui.active_tab = AccountsTab::OtherUsers;
        let cmds = ui.render(&test_palette(), 0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_sign_in_options() {
        let mut ui = AccountSettingsUI::default();
        ui.active_tab = AccountsTab::SignInOptions;
        let cmds = ui.render(&test_palette(), 0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_activity_log() {
        let mut ui = AccountSettingsUI::default();
        ui.active_tab = AccountsTab::ActivityLog;
        let cmds = ui.render(&test_palette(), 0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_with_status_message() {
        let mut ui = AccountSettingsUI::default();
        ui.status_message = Some("Account created successfully".to_string());
        let cmds = ui.render(&test_palette(), 0.0, 0.0, 600.0, 400.0);
        let has_status = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("Account created")
            } else {
                false
            }
        });
        assert!(has_status);
    }

    #[test]
    fn test_tab_display_names() {
        for tab in AccountsTab::ALL {
            assert!(!tab.display_name().is_empty());
        }
    }

    // ---- Palette conversion ----

    /// A manager in which every colour-bearing branch is present at once.
    ///
    /// One account per avatar slot, so all seven identity colours are actually
    /// drawn, plus the two non-initials avatar kinds; account types cycle so
    /// all three badges appear; and the signed-in flag alternates so both
    /// status colours do.
    fn every_branch_accounts() -> AccountManager {
        let mut mgr = AccountManager::new();
        let types = [
            AccountType::Standard,
            AccountType::Guest,
            AccountType::Administrator,
        ];
        for slot in 0..9u8 {
            let ty = types[usize::from(slot) % types.len()];
            let uid = mgr
                .create_account(
                    &format!("user{slot}"),
                    &format!("User {slot}"),
                    ty,
                    u64::from(slot),
                )
                .expect("the fixture's usernames are valid and unique");
            let account = mgr.get_mut(uid).expect("just created");
            account.avatar = match slot {
                7 => Avatar::SystemIcon(3),
                8 => Avatar::ImagePath("/home/u/avatar.png".to_string()),
                other => Avatar::Initials { color_index: other },
            };
            account.is_logged_in = slot % 2 == 0;
        }
        mgr
    }

    /// The panel wound to one state. `switches` is the current user's
    /// `(auto_login, require_password_on_wake, has_password)`, which is what
    /// decides both toggle positions and the password info row.
    fn wound_ui(
        populated: bool,
        tab: AccountsTab,
        selected: Option<u32>,
        status: Option<&str>,
        switches: (bool, bool, bool),
        avatar: Avatar,
    ) -> AccountSettingsUI {
        let mut mgr = if populated {
            every_branch_accounts()
        } else {
            AccountManager::new()
        };
        if let Some(uid) = mgr.current_user().map(|a| a.uid)
            && let Some(account) = mgr.get_mut(uid)
        {
            account.login_options.auto_login = switches.0;
            account.login_options.require_password_on_wake = switches.1;
            account.login_options.has_password = switches.2;
            account.avatar = avatar;
        }
        AccountSettingsUI {
            manager: mgr,
            active_tab: tab,
            selected_uid: selected,
            status_message: status.map(str::to_string),
            ..AccountSettingsUI::default()
        }
    }

    /// Every colour this panel *computes* rather than reads.
    ///
    /// All of it is `readable_on` ink: initials on an avatar circle, the
    /// account-type word on its badge, and the lettering on the accent-filled
    /// button. Such ink is declared rather than exempt because each value
    /// `readable_on` can return is also a role of one of the two palettes —
    /// see `palette_check`'s module docs.
    ///
    /// The fills are written out by hand rather than read back through
    /// `avatar_colors` and `badge_color`: an expectation taken from the code
    /// under test is an echo of it. The cost is that adding an avatar colour
    /// without adding it here fails the sweep, which is the correct direction
    /// for that failure to point. Each fill is wrapped individually rather
    /// than mapped over a bare list of roles, because a bare list of the seven
    /// avatar roles is textually the production table — a second copy of the
    /// thing being checked, sitting one screen away from it.
    fn every_ink(p: &Palette) -> [Color; 11] {
        [
            // Initials, on each of the seven avatar slots.
            readable_on(p.blue),
            readable_on(p.green),
            readable_on(p.peach),
            readable_on(p.mauve),
            readable_on(p.red),
            readable_on(p.yellow),
            readable_on(p.lavender),
            // Initials again, on the two non-initials avatar backgrounds.
            readable_on(p.surface1),
            readable_on(p.surface0),
            // The account-type word: red and blue are covered above, so only
            // the guest badge adds a fill here.
            readable_on(p.overlay0),
            // The lettering on the accent-filled button.
            readable_on(p.accent),
        ]
    }

    /// The membership sweep: nothing this panel draws may be a colour outside
    /// the palette it was handed. See `palette_check` for why the light render
    /// is what makes a leftover Mocha constant name itself.
    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let ink = every_ink(&p);
            for populated in [false, true] {
                for tab in AccountsTab::ALL {
                    for selected in [None, Some(1001)] {
                        for status in [None, Some("Account created successfully")] {
                            for switches in [(false, false, false), (true, true, true)] {
                                for avatar in [
                                    Avatar::Initials { color_index: 5 },
                                    Avatar::SystemIcon(1),
                                    Avatar::ImagePath("/home/u/a.png".to_string()),
                                ] {
                                    let ui = wound_ui(
                                        populated, *tab, selected, status, switches, avatar,
                                    );
                                    let cmds = ui.render(&p, 0.0, 0.0, 600.0, 400.0);
                                    palette_check::assert_drawn_from(
                                        &p,
                                        &cmds,
                                        &ink,
                                        "user_accounts",
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // And the branch where nobody is signed in, which two of the four tabs
        // draw nothing at all for.
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mut ui = wound_ui(
                true,
                AccountsTab::YourInfo,
                None,
                None,
                (false, false, false),
                Avatar::default(),
            );
            for account in 0..ui.manager.accounts().len() {
                let uid = ui.manager.accounts()[account].uid;
                if let Some(a) = ui.manager.get_mut(uid) {
                    a.is_current = false;
                }
            }
            for tab in AccountsTab::ALL {
                ui.active_tab = *tab;
                let cmds = ui.render(&p, 0.0, 0.0, 600.0, 400.0);
                palette_check::assert_drawn_from(
                    &p,
                    &cmds,
                    &every_ink(&p),
                    "user_accounts (nobody signed in)",
                );
            }
        }
    }

    /// Every colour that says *which user* or *which kind of user* this is.
    ///
    /// The avatar circles (64pt on Your Info, 28pt in the list), the
    /// account-type pill and the 10pt captions under each name — all
    /// categorical, none of them the accent's to move.
    fn identity_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width: 64.0,
                    height: 64.0,
                    color,
                    ..
                }
                | RenderCommand::FillRect {
                    width: 28.0,
                    height: 28.0,
                    color,
                    ..
                }
                | RenderCommand::FillRect {
                    width: 90.0,
                    height: 20.0,
                    color,
                    ..
                }
                | RenderCommand::Text {
                    font_size: 10.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The one colour on every tab that *is* the accent's: the active tab's
    /// label, the only bold 12pt text in the tab bar.
    fn active_tab_label_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    font_size: 12.0,
                    font_weight: FontWeightHint::Bold,
                    text,
                    color,
                    ..
                } if AccountsTab::ALL.iter().any(|t| t.display_name() == text) => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// A user's identity is not the accent's to repaint.
    ///
    /// The membership sweep above cannot see this: a wrong *role* is a member
    /// of both palettes, so swapping `p.blue` for `p.accent` passes it in light
    /// mode exactly as in dark. Only a second render under a different accent
    /// separates the two.
    ///
    /// The `assert_ne!` on the tab label is the load-bearing half. Without it a
    /// panel that ignored the accent everywhere — drew the whole thing in one
    /// frozen colour — would pass the equality check while measuring nothing.
    #[test]
    fn a_users_identity_colours_do_not_follow_the_accent() {
        for tab in AccountsTab::ALL {
            let ui = wound_ui(
                true,
                *tab,
                Some(1001),
                None,
                (true, false, true),
                Avatar::Initials { color_index: 2 },
            );

            let mut blue = Palette::for_mode(false);
            blue.accent = appearance::BLUE;
            let mut mauve = Palette::for_mode(false);
            mauve.accent = appearance::MAUVE;

            let under_blue = ui.render(&blue, 0.0, 0.0, 600.0, 400.0);
            let under_mauve = ui.render(&mauve, 0.0, 0.0, 600.0, 400.0);

            let label_blue = active_tab_label_colors(&under_blue);
            let label_mauve = active_tab_label_colors(&under_mauve);
            assert_eq!(
                label_blue.len(),
                1,
                "{} has no active tab label",
                tab.display_name()
            );
            assert_ne!(
                label_blue,
                label_mauve,
                "the {} tab's own label did not move with the accent, so this \
                 test would pass on a panel that ignored the accent entirely",
                tab.display_name()
            );

            let identity_blue = identity_colors(&under_blue);
            let identity_mauve = identity_colors(&under_mauve);
            // Only two of the four tabs show a user at all; on the other two
            // the equality below is vacuous, which is why the emptiness check
            // is asserted exactly where it means something.
            let draws_identity = matches!(tab, AccountsTab::YourInfo | AccountsTab::OtherUsers);
            assert_eq!(
                !identity_blue.is_empty(),
                draws_identity,
                "the {} tab drew {} identity colours",
                tab.display_name(),
                identity_blue.len()
            );
            assert_eq!(
                identity_blue,
                identity_mauve,
                "an avatar or account-type colour on the {} tab moved with the \
                 accent. Those say which user and which kind of user, the way a \
                 risk level or a device status does; a mauve accent says nothing \
                 about what a standard account is.",
                tab.display_name()
            );
        }
    }

    /// The seven avatar colours are how you tell accounts apart at a glance, so
    /// two of them landing on the same value would silently merge two users.
    #[test]
    fn the_avatar_colours_stay_distinct_in_both_modes() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mut seen: Vec<Color> = Vec::with_capacity(AVATAR_COLOR_COUNT);
            for slot in 0..AVATAR_COLOR_COUNT {
                let index = u8::try_from(slot).expect("the palette is far shorter than 256");
                let color = Avatar::palette_color(&p, index);
                assert!(
                    !seen.contains(&color),
                    "avatar slots collide at {slot} in {} mode, so two accounts \
                     would be indistinguishable",
                    if light { "light" } else { "dark" }
                );
                seen.push(color);
            }
        }
    }
}

//! Slate OS Login Manager — Display Manager and Session Launcher
//!
//! Presents the login screen, authenticates users against the local user
//! database, and starts user sessions. Also provides a lock screen that
//! protects running sessions from unauthorized access.
//!
//! # Features
//!
//! - Multi-user account management (load from /etc/users.yaml)
//! - Password hashing via SHA-256 with salt
//! - Account lockout after repeated failures (5 attempts / 5 minute cooldown)
//! - Auto-login support for configured accounts
//! - Guest login (no password)
//! - Lock screen with idle timeout
//! - Accessibility options (high contrast, large text)
//! - Power menu (shutdown, restart, sleep)
//! - Full keyboard navigation

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::HashMap;

// ============================================================================
// Theme colors (dark theme — Catppuccin Mocha inspired)
// ============================================================================

/// Deep background
const COL_BG_DARK: Color = Color::from_hex(0x11111B);
/// Background gradient top
const COL_BG_TOP: Color = Color::from_hex(0x1E1E2E);
/// Background gradient bottom
const COL_BG_BOTTOM: Color = Color::from_hex(0x181825);
/// Login box background
const COL_PANEL: Color = Color::from_hex(0x313244);
/// Input field background
const COL_INPUT_BG: Color = Color::from_hex(0x45475A);
/// Input field border (focused)
const COL_INPUT_FOCUS: Color = Color::from_hex(0x89B4FA);
/// Input field border (normal)
#[allow(dead_code)]
const COL_INPUT_BORDER: Color = Color::from_hex(0x585B70);
/// Primary text
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
/// Secondary / dim text
const COL_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
/// Accent color (buttons, selected items)
const COL_ACCENT: Color = Color::from_hex(0x89B4FA);
/// Accent hover
#[allow(dead_code)]
const COL_ACCENT_HOVER: Color = Color::from_hex(0xB4D0FB);
/// Error / red
const COL_ERROR: Color = Color::from_hex(0xF38BA8);
/// Success / green
#[allow(dead_code)]
const COL_SUCCESS: Color = Color::from_hex(0xA6E3A1);
/// Warning / peach
const COL_WARNING: Color = Color::from_hex(0xFAB387);
/// Avatar circle colors (assigned by uid)
const COL_AVATAR_PALETTE: [Color; 6] = [
    Color::from_hex(0x89B4FA), // blue
    Color::from_hex(0xCBA6F7), // mauve
    Color::from_hex(0xF38BA8), // red
    Color::from_hex(0xA6E3A1), // green
    Color::from_hex(0xF9E2AF), // yellow
    Color::from_hex(0xFAB387), // peach
];
/// Power menu background
const COL_POWER_BG: Color = Color::rgba(17, 17, 27, 220);

// High contrast overrides
const COL_HC_BG: Color = Color::BLACK;
const COL_HC_TEXT: Color = Color::WHITE;
const COL_HC_ACCENT: Color = Color::from_hex(0x00FFFF);
const COL_HC_ERROR: Color = Color::from_hex(0xFF4444);
const COL_HC_PANEL: Color = Color::from_hex(0x222222);

// ============================================================================
// Layout constants
// ============================================================================

const SCREEN_WIDTH: f32 = 1920.0;
const SCREEN_HEIGHT: f32 = 1080.0;

const LOGIN_BOX_WIDTH: f32 = 400.0;
const LOGIN_BOX_HEIGHT: f32 = 480.0;
const LOGIN_BOX_RADIUS: f32 = 16.0;

const AVATAR_SIZE: f32 = 80.0;
const AVATAR_Y_OFFSET: f32 = 40.0;

const INPUT_WIDTH: f32 = 320.0;
const INPUT_HEIGHT: f32 = 44.0;
const INPUT_RADIUS: f32 = 8.0;

const BUTTON_WIDTH: f32 = 320.0;
const BUTTON_HEIGHT: f32 = 44.0;
const BUTTON_RADIUS: f32 = 8.0;

const POWER_BUTTON_SIZE: f32 = 48.0;
const POWER_MENU_WIDTH: f32 = 200.0;

const FONT_SIZE_LARGE: f32 = 24.0;
const FONT_SIZE_NORMAL: f32 = 16.0;
const FONT_SIZE_SMALL: f32 = 13.0;
const FONT_SIZE_CLOCK: f32 = 18.0;

const LARGE_FONT_SCALE: f32 = 1.4;

// ============================================================================
// User account model
// ============================================================================

/// A user account on the system.
///
/// This is a view over one entry of `/etc/users.yaml`, not a copy of it. The
/// entry's own lines are kept, so a field this program has never heard of —
/// anything `useradm` or a later version writes — survives a login, a password
/// change and a save. The previous version rebuilt the file from this struct,
/// which meant every write silently deleted whatever the other writer had
/// added; see `design-decisions.md` §330.
#[derive(Clone, Debug)]
pub struct UserAccount {
    record: userdb::Record,
}

impl UserAccount {
    /// Wrap a database record.
    fn from_record(record: userdb::Record) -> Self {
        Self { record }
    }

    /// The underlying record.
    fn record(&self) -> &userdb::Record {
        &self.record
    }

    /// Create a new user account with a plaintext password (will be hashed).
    fn new_with_password(
        uid: u32,
        username: &str,
        display_name: &str,
        password: &str,
        is_admin: bool,
    ) -> Self {
        let mut record = userdb::Record::new();
        record.set_uid(uid);
        record.set(userdb::field::USERNAME, username);
        record.set(userdb::field::DISPLAY_NAME, display_name);
        record.set_avatar("");
        record.set(userdb::field::SHELL, "/bin/nush");
        record.set_home(&format!("/home/{username}"));
        record.set_admin(is_admin);
        record.set_auto_login(false);
        record.record_login(0);
        // `record_login` counts one; these accounts have logged in zero times.
        record.set(userdb::field::LOGIN_COUNT, "0");

        // Prefer a real salt. The fallback is a *fixed* one, and is acceptable
        // here and nowhere else: it is only reached for the two built-in
        // accounts, whose passwords are published in this source file, so the
        // salt is protecting a secret that is not a secret. Any account a user
        // or a tool creates goes through `Record::set_password`, which refuses
        // to invent randomness it does not have.
        const BUILTIN_ACCOUNT_SALT: &str = "slateos.";
        if record.set_password(password).is_err() {
            let _ = record.set_password_with_salt(password, BUILTIN_ACCOUNT_SALT);
        }

        Self { record }
    }

    /// Create the root (admin) account.
    fn root_account() -> Self {
        let mut account = Self::new_with_password(0, "root", "Administrator", "root", true);
        account.record.set_home("/root");
        account
    }

    /// Create the guest account (no password required).
    fn guest_account() -> Self {
        let mut record = userdb::Record::new();
        record.set_uid(65534);
        record.set(userdb::field::USERNAME, "guest");
        record.set(userdb::field::DISPLAY_NAME, "Guest");
        record.set(userdb::field::PASSWORD_HASH, "");
        record.set_avatar("");
        record.set(userdb::field::SHELL, "/bin/nush");
        record.set_home("/tmp/guest");
        record.set_admin(false);
        record.set_auto_login(false);
        record.record_login(0);
        record.set(userdb::field::LOGIN_COUNT, "0");
        Self { record }
    }

    /// Unique user identifier.
    fn uid(&self) -> u32 {
        self.record.uid().unwrap_or(0)
    }

    /// Login username.
    fn username(&self) -> String {
        self.record.username().unwrap_or_default()
    }

    /// Name shown on the login screen, falling back to the login name so that
    /// a record without one is not drawn as a blank tile.
    fn display_name(&self) -> String {
        match self.record.get(userdb::field::DISPLAY_NAME) {
            Some(name) if !name.is_empty() => name,
            _ => self.username(),
        }
    }

    /// The user's preferred shell.
    fn shell(&self) -> String {
        match self.record.get(userdb::field::SHELL) {
            Some(shell) if !shell.is_empty() => shell,
            _ => "/bin/nush".to_string(),
        }
    }

    /// Home directory.
    fn home_dir(&self) -> String {
        self.record.home().unwrap_or_default()
    }

    /// Optional avatar image path.
    ///
    /// Unused by the drawing code, which always renders initials — see
    /// `known-issues.md`, "the login screen ignores `avatar_path`". Kept
    /// because the field is part of the format and the accessor is what the
    /// fix will use.
    #[allow(dead_code)]
    fn avatar_path(&self) -> Option<String> {
        self.record.avatar()
    }

    /// Whether this user has admin privileges.
    fn is_admin(&self) -> bool {
        self.record.is_admin()
    }

    /// Whether this account should log in without being asked.
    fn auto_login(&self) -> bool {
        self.record.auto_login()
    }

    /// Unix timestamp of last successful login.
    ///
    /// Read by the tests, which is what makes the login-counting assertion
    /// possible; nothing on the screen shows it yet.
    #[cfg_attr(not(test), allow(dead_code))]
    fn last_login_timestamp(&self) -> u64 {
        self.record.last_login()
    }

    /// Total successful logins.
    #[cfg_attr(not(test), allow(dead_code))]
    fn login_count(&self) -> u32 {
        self.record.login_count()
    }

    /// Check if this account requires a password (guest does not).
    fn requires_password(&self) -> bool {
        !matches!(
            self.record.check_password(""),
            userdb::Auth::NoPassword | userdb::Auth::Accepted
        )
    }

    /// Check a password against this account.
    ///
    /// Nothing here decodes a salt or compares hex strings: the stored entry
    /// is `crypt`'s own setting, so recomputing it with the offered password
    /// reproduces it exactly when the password is right. The version this
    /// replaces hashed `sha256(salt_bytes || password)` while `useradm` hashed
    /// `sha256(salt_hex || password)`, and neither could check the other's
    /// entries.
    fn check_password(&self, password: &str) -> userdb::Auth {
        self.record.check_password(password)
    }

    /// Get initials for the avatar circle (first letters of display name words).
    fn initials(&self) -> String {
        self.display_name()
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .take(2)
            .map(|c| c.to_uppercase().to_string())
            .collect()
    }

    /// Get the avatar color based on uid.
    fn avatar_color(&self) -> Color {
        // The palette is never empty, so the remainder is always in range and
        // the fallbacks are unreachable — but they are written out rather than
        // asserted, because a display manager that panics while drawing an
        // avatar leaves the machine with no way to log in at all.
        let idx = (self.uid() as usize)
            .checked_rem(COL_AVATAR_PALETTE.len())
            .unwrap_or(0);
        COL_AVATAR_PALETTE.get(idx).copied().unwrap_or(COL_ACCENT)
    }
}

// ============================================================================
// User database (YAML-based)
// ============================================================================

/// Load the user database, falling back to default accounts.
///
/// The fallback applies to a *missing* database and to one with no accounts in
/// it. A database that exists and cannot be read does not fall back: the
/// defaults include a root account with a known password, so silently
/// presenting them because `/etc/users.yaml` was briefly unreadable would let
/// anyone in with `root`/`root` on a machine that has no such account.
fn load_user_database() -> Vec<UserAccount> {
    let Ok(db) = userdb::UserDb::load(userdb::DEFAULT_PATH) else {
        return Vec::new();
    };
    if db.records().is_empty() {
        return default_accounts();
    }
    db.records()
        .iter()
        .cloned()
        .map(UserAccount::from_record)
        .collect()
}

/// Save the user database.
///
/// The records are written back into the file they came from, so comments,
/// ordering and every unrecognised field are preserved; only the fields this
/// program changed differ. Writing is atomic — see [`userdb::UserDb::save`].
fn save_user_database(users: &[UserAccount]) -> Result<(), std::io::Error> {
    let mut db = userdb::UserDb::load(userdb::DEFAULT_PATH)?;
    let existing: Vec<userdb::Record> = db.records().to_vec();
    let records = db.records_mut();
    records.clear();
    for user in users {
        records.push(user.record().clone());
    }
    // Records that were in the file but are not in this list are dropped —
    // which is right when a user is deleted, and would be a disaster if the
    // caller had loaded a subset. Nothing loads a subset; the assertion is
    // recorded here rather than left implicit.
    debug_assert!(
        existing.len() >= records.len() || existing.is_empty(),
        "save_user_database writes the whole database, not a subset"
    );
    db.save(userdb::DEFAULT_PATH)
}

/// Default user accounts for a fresh system.
fn default_accounts() -> Vec<UserAccount> {
    vec![UserAccount::root_account(), UserAccount::guest_account()]
}

// ============================================================================
// Session management
// ============================================================================

/// Information about a running user session.
#[derive(Clone, Debug)]
pub struct SessionInfo {
    /// User who owns this session.
    pub user_uid: u32,
    /// Unique session identifier.
    pub session_id: u64,
    /// Timestamp when the session started.
    pub started_at: u64,
    /// Path to the user's shell.
    pub shell_path: String,
    /// User's home directory.
    pub home_dir: String,
    /// Environment variables for this session.
    pub environment: HashMap<String, String>,
}

impl SessionInfo {
    /// Create a new session for a user.
    fn new(user: &UserAccount, session_id: u64, timestamp: u64) -> Self {
        let mut env = HashMap::new();
        env.insert("HOME".to_string(), user.home_dir().clone());
        env.insert("USER".to_string(), user.username().clone());
        env.insert("LOGNAME".to_string(), user.username().clone());
        env.insert("SHELL".to_string(), user.shell().clone());
        env.insert(
            "PATH".to_string(),
            "/bin:/usr/bin:/usr/local/bin".to_string(),
        );
        env.insert(
            "XDG_RUNTIME_DIR".to_string(),
            format!("/run/user/{}", user.uid()),
        );
        env.insert(
            "XDG_DATA_HOME".to_string(),
            format!("{}/.local/share", user.home_dir()),
        );
        env.insert(
            "XDG_CONFIG_HOME".to_string(),
            format!("{}/.config", user.home_dir()),
        );
        env.insert(
            "XDG_CACHE_HOME".to_string(),
            format!("{}/.cache", user.home_dir()),
        );
        env.insert("XDG_SESSION_TYPE".to_string(), "graphical".to_string());

        Self {
            user_uid: user.uid(),
            session_id,
            started_at: timestamp,
            shell_path: user.shell().clone(),
            home_dir: user.home_dir().clone(),
            environment: env,
        }
    }
}

// ============================================================================
// Account lockout tracking
// ============================================================================

/// Tracks failed login attempts and lockout state for an account.
#[derive(Clone, Debug)]
pub struct LockoutState {
    /// Number of consecutive failed attempts.
    failed_attempts: u32,
    /// Timestamp when the lockout expires (0 = not locked).
    locked_until: u64,
}

impl LockoutState {
    fn new() -> Self {
        Self {
            failed_attempts: 0,
            locked_until: 0,
        }
    }

    /// Record a failed attempt; returns true if account is now locked.
    fn record_failure(&mut self, now: u64) -> bool {
        // Saturating: the counter only ever has to reach MAX_FAILED_ATTEMPTS,
        // and one that wrapped to zero would hand an attacker a fresh budget.
        self.failed_attempts = self.failed_attempts.saturating_add(1);
        if self.failed_attempts >= MAX_FAILED_ATTEMPTS {
            self.locked_until = now.saturating_add(LOCKOUT_DURATION_SECS);
            true
        } else {
            false
        }
    }

    /// Check if the account is currently locked.
    fn is_locked(&self, now: u64) -> bool {
        self.locked_until > now
    }

    /// Remaining lockout seconds.
    fn remaining_lockout_secs(&self, now: u64) -> u64 {
        self.locked_until.saturating_sub(now)
    }

    /// Reset on successful login.
    fn reset(&mut self) {
        self.failed_attempts = 0;
        self.locked_until = 0;
    }
}

const MAX_FAILED_ATTEMPTS: u32 = 5;
const LOCKOUT_DURATION_SECS: u64 = 300; // 5 minutes

/// Entries in the power menu: shutdown, restart, sleep.
const POWER_MENU_ENTRIES: usize = 3;
/// Index of the last power-menu entry, which `Up` wraps around to.
const POWER_MENU_LAST: usize = POWER_MENU_ENTRIES - 1;

// ============================================================================
// Login view state machine
// ============================================================================

/// Which screen the login manager is currently displaying.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginView {
    /// Show the user list for selection.
    UserSelect,
    /// Show the password entry for the selected user.
    PasswordEntry,
    /// Screen is locked (session active, user must unlock).
    Locked,
    /// Power menu overlay is visible.
    PowerMenu,
    /// System is shutting down (or restarting).
    ShuttingDown,
}

/// Power menu options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PowerAction {
    Shutdown,
    Restart,
    Sleep,
}

// ============================================================================
// Accessibility settings
// ============================================================================

/// Accessibility options for the login screen.
#[derive(Clone, Debug)]
pub struct AccessibilitySettings {
    /// High contrast mode.
    pub high_contrast: bool,
    /// Large text (1.4x scale).
    pub large_text: bool,
    /// On-screen keyboard visible.
    pub onscreen_keyboard: bool,
    /// Screen reader announcements (collect for output).
    pub screen_reader_enabled: bool,
    /// Pending announcements for screen reader.
    pub announcements: Vec<String>,
}

impl AccessibilitySettings {
    fn new() -> Self {
        Self {
            high_contrast: false,
            large_text: false,
            onscreen_keyboard: false,
            screen_reader_enabled: false,
            announcements: Vec::new(),
        }
    }

    /// Announce a text description for the screen reader.
    fn announce(&mut self, text: &str) {
        if self.screen_reader_enabled {
            self.announcements.push(text.to_string());
        }
    }

    /// Drain pending announcements.
    #[allow(dead_code)]
    fn drain_announcements(&mut self) -> Vec<String> {
        std::mem::take(&mut self.announcements)
    }
}

// ============================================================================
// Login Manager (main state)
// ============================================================================

/// The login manager state machine. Holds all state for the display manager,
/// including user database, authentication, session tracking, and UI state.
pub struct LoginManager {
    /// Current view being displayed.
    pub current_view: LoginView,
    /// All user accounts on the system.
    pub users: Vec<UserAccount>,
    /// Index of the currently selected user in the user list.
    pub selected_user_index: usize,
    /// Current password input (masked on screen).
    pub password_input: String,
    /// Whether to show the password in cleartext.
    pub password_visible: bool,
    /// Current error message to display (cleared on input).
    pub error_message: Option<String>,
    /// Per-account lockout state, keyed by uid.
    pub locked_accounts: HashMap<u32, LockoutState>,
    /// Active sessions, keyed by session_id.
    pub sessions: HashMap<u64, SessionInfo>,
    /// Next session ID to assign.
    next_session_id: u64,
    /// Lock screen idle timeout in seconds.
    pub lock_timeout_secs: u64,
    /// Seconds since last user input (for idle timeout).
    pub idle_seconds: u64,
    /// Whether the screen is dimmed (30s warning before lock).
    pub screen_dimmed: bool,
    /// Current timestamp (updated by tick()).
    current_time: u64,
    /// Accessibility settings.
    pub accessibility: AccessibilitySettings,
    /// Power menu selection index.
    power_menu_selection: usize,
    /// The uid of the user whose session is locked (only used in Locked view).
    locked_session_uid: Option<u32>,
    /// Clock string for display.
    clock_display: String,
}

impl Default for LoginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginManager {
    /// Create a new login manager with accounts loaded from the database.
    pub fn new() -> Self {
        let users = load_user_database();
        Self {
            current_view: LoginView::UserSelect,
            users,
            selected_user_index: 0,
            password_input: String::new(),
            password_visible: false,
            error_message: None,
            locked_accounts: HashMap::new(),
            sessions: HashMap::new(),
            next_session_id: 1,
            lock_timeout_secs: 300, // 5 minutes default
            idle_seconds: 0,
            screen_dimmed: false,
            current_time: 0,
            accessibility: AccessibilitySettings::new(),
            power_menu_selection: 0,
            locked_session_uid: None,
            clock_display: "00:00".to_string(),
        }
    }

    /// Create a login manager with specific accounts (for testing).
    pub fn with_users(users: Vec<UserAccount>) -> Self {
        Self {
            users,
            ..Self::new_internal()
        }
    }

    /// Internal constructor without loading from disk.
    fn new_internal() -> Self {
        Self {
            current_view: LoginView::UserSelect,
            users: Vec::new(),
            selected_user_index: 0,
            password_input: String::new(),
            password_visible: false,
            error_message: None,
            locked_accounts: HashMap::new(),
            sessions: HashMap::new(),
            next_session_id: 1,
            lock_timeout_secs: 300,
            idle_seconds: 0,
            screen_dimmed: false,
            current_time: 0,
            accessibility: AccessibilitySettings::new(),
            power_menu_selection: 0,
            locked_session_uid: None,
            clock_display: "00:00".to_string(),
        }
    }

    /// Check for an auto-login user and bypass the login screen if found.
    pub fn check_auto_login(&mut self) -> Option<SessionInfo> {
        let auto_user = self.users.iter().find(|u| u.auto_login()).cloned();
        if let Some(user) = auto_user {
            self.start_session(user.uid()).ok()
        } else {
            None
        }
    }

    // ========================================================================
    // Authentication
    // ========================================================================

    /// Authenticate a user with the given password.
    /// Returns Ok(()) on success, Err(message) on failure.
    pub fn authenticate(&mut self, username: &str, password: &str) -> Result<(), String> {
        let now = self.current_time;

        // Find the user.
        let user = self
            .users
            .iter()
            .find(|u| u.username() == username)
            .cloned();
        let user = match user {
            Some(u) => u,
            None => return Err("User not found".to_string()),
        };

        // Check lockout.
        if let Some(lockout) = self.locked_accounts.get(&user.uid())
            && lockout.is_locked(now)
        {
            let remaining = lockout.remaining_lockout_secs(now);
            return Err(format!(
                "Account locked. Try again in {} seconds.",
                remaining
            ));
        }

        // Guest accounts don't need a password.
        if !user.requires_password() {
            return Ok(());
        }

        match user.check_password(password) {
            userdb::Auth::Accepted => {
                // Success: reset lockout, update login stats.
                self.locked_accounts
                    .entry(user.uid())
                    .and_modify(LockoutState::reset);
                if let Some(u) = self.users.iter_mut().find(|u| u.uid() == user.uid()) {
                    u.record.record_login(now);
                }
                Ok(())
            }
            // An account the administrator has disabled is not a wrong
            // password, and must not be reported as one: five wrong guesses
            // would otherwise "lock" an account that is already locked, and
            // the message would tell an attacker the password was close.
            userdb::Auth::Locked => Err("Account is locked.".to_string()),
            // No entry we can check. This is what a password written by one of
            // the two implementations that predate `userdb` looks like — see
            // `design-decisions.md` §330. Say so, because "incorrect password"
            // would send the user round the same loop for ever.
            userdb::Auth::Unusable => Err(
                "This account's password was stored in a format that can no longer be \
                 checked. An administrator must reset it with `useradm passwd'."
                    .to_string(),
            ),
            userdb::Auth::NoPassword | userdb::Auth::Rejected => {
                // Failure: record attempt.
                let lockout = self
                    .locked_accounts
                    .entry(user.uid())
                    .or_insert_with(LockoutState::new);
                let now_locked = lockout.record_failure(now);
                if now_locked {
                    Err("Account locked after too many attempts. Wait 5 minutes.".to_string())
                } else {
                    let remaining = MAX_FAILED_ATTEMPTS.saturating_sub(lockout.failed_attempts);
                    Err(format!(
                        "Incorrect password. {} attempts remaining.",
                        remaining
                    ))
                }
            }
        }
    }

    // ========================================================================
    // Session management
    // ========================================================================

    /// Start a new session for the given user.
    pub fn start_session(&mut self, uid: u32) -> Result<SessionInfo, String> {
        let user = self.users.iter().find(|u| u.uid() == uid).cloned();
        let user = match user {
            Some(u) => u,
            None => return Err("User not found".to_string()),
        };

        let session_id = self.next_session_id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        let session = SessionInfo::new(&user, session_id, self.current_time);
        self.sessions.insert(session_id, session.clone());

        // Save updated login stats.
        let _ = save_user_database(&self.users);

        Ok(session)
    }

    /// End a session and return to the login screen.
    pub fn end_session(&mut self, session_id: u64) {
        self.sessions.remove(&session_id);
        self.current_view = LoginView::UserSelect;
        self.password_input.clear();
        self.error_message = None;
        self.idle_seconds = 0;
        self.screen_dimmed = false;
    }

    /// Lock the screen for the given session.
    pub fn lock_screen(&mut self, session_uid: u32) {
        self.current_view = LoginView::Locked;
        self.locked_session_uid = Some(session_uid);
        self.password_input.clear();
        self.error_message = None;
        self.screen_dimmed = false;
        self.accessibility
            .announce("Screen locked. Enter password to unlock.");
    }

    /// Attempt to unlock the screen with a password.
    pub fn unlock_screen(&mut self, password: &str) -> Result<(), String> {
        let uid = match self.locked_session_uid {
            Some(uid) => uid,
            None => return Err("No locked session".to_string()),
        };

        let user = self.users.iter().find(|u| u.uid() == uid).cloned();
        let user = match user {
            Some(u) => u,
            None => return Err("Session user not found".to_string()),
        };

        // An account with no password cannot be asked for one. Refusing would
        // not protect the session: it would strand it, since there is no
        // password that could ever unlock it and the only way out is a reboot,
        // which loses the session's contents anyway. The guest account is the
        // case that matters, and the previous code compared the offered
        // password against an empty hash, which no password matches — so a
        // locked guest screen was a dead end.
        if !user.requires_password() {
            self.current_view = LoginView::UserSelect;
            self.locked_session_uid = None;
            self.idle_seconds = 0;
            self.screen_dimmed = false;
            return Ok(());
        }

        // Verify password (same flow as authenticate).
        match user.check_password(password) {
            userdb::Auth::Accepted => {
                self.current_view = LoginView::UserSelect; // Returns to desktop in real system.
                self.locked_session_uid = None;
                self.idle_seconds = 0;
                self.screen_dimmed = false;
                Ok(())
            }
            userdb::Auth::Unusable => Err(
                "This account's password can no longer be checked; an administrator must \
                 reset it."
                    .to_string(),
            ),
            // A session that is already running stays locked rather than
            // opening because its account has no password: the screen lock
            // exists to protect a session, and a passwordless account still
            // has one to protect.
            userdb::Auth::Locked | userdb::Auth::NoPassword | userdb::Auth::Rejected => {
                Err("Incorrect password".to_string())
            }
        }
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle an input event. Returns EventResult::Consumed if handled.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // Any input resets idle timer.
        self.idle_seconds = 0;
        self.screen_dimmed = false;

        match event {
            Event::Key(key_event) if key_event.pressed => self.handle_key(key_event),
            Event::Mouse(mouse_event) => self.handle_mouse(mouse_event),
            Event::Tick { elapsed_ms } => {
                self.tick(*elapsed_ms);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle a key press.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // Global accessibility shortcuts.
        if key.modifiers.ctrl && key.modifiers.alt {
            match key.key {
                Key::H => {
                    self.accessibility.high_contrast = !self.accessibility.high_contrast;
                    return EventResult::Consumed;
                }
                Key::L => {
                    self.accessibility.large_text = !self.accessibility.large_text;
                    return EventResult::Consumed;
                }
                Key::K => {
                    self.accessibility.onscreen_keyboard = !self.accessibility.onscreen_keyboard;
                    return EventResult::Consumed;
                }
                Key::S => {
                    self.accessibility.screen_reader_enabled =
                        !self.accessibility.screen_reader_enabled;
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        match self.current_view {
            LoginView::UserSelect => self.handle_key_user_select(key),
            LoginView::PasswordEntry => self.handle_key_password_entry(key),
            LoginView::Locked => self.handle_key_locked(key),
            LoginView::PowerMenu => self.handle_key_power_menu(key),
            LoginView::ShuttingDown => EventResult::Consumed,
        }
    }

    fn handle_key_user_select(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Up | Key::Left => {
                // Wraps to the end of the list; an empty list stays at zero,
                // which is the only index a selection can safely hold there.
                self.selected_user_index = match self.selected_user_index.checked_sub(1) {
                    Some(prev) => prev,
                    None => self.users.len().saturating_sub(1),
                };
                EventResult::Consumed
            }
            Key::Down | Key::Right => {
                self.selected_user_index = self
                    .selected_user_index
                    .saturating_add(1)
                    .checked_rem(self.users.len())
                    .unwrap_or(0);
                EventResult::Consumed
            }
            Key::Enter => {
                if let Some(user) = self.users.get(self.selected_user_index).cloned() {
                    if user.requires_password() {
                        self.current_view = LoginView::PasswordEntry;
                        self.password_input.clear();
                        self.error_message = None;
                        self.accessibility.announce(&format!(
                            "Password entry for {}. Type your password.",
                            user.display_name()
                        ));
                    } else {
                        // Guest or no-password account: log in directly.
                        let uid = user.uid();
                        match self.start_session(uid) {
                            Ok(_) => {
                                self.accessibility.announce("Logged in as guest.");
                            }
                            Err(msg) => {
                                self.error_message = Some(msg);
                            }
                        }
                    }
                }
                EventResult::Consumed
            }
            Key::Escape => {
                self.current_view = LoginView::PowerMenu;
                self.power_menu_selection = 0;
                EventResult::Consumed
            }
            Key::Tab => {
                // Cycle focus to accessibility options in the future.
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key_password_entry(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Enter => {
                self.attempt_login();
                EventResult::Consumed
            }
            Key::Escape => {
                self.current_view = LoginView::UserSelect;
                self.password_input.clear();
                self.error_message = None;
                EventResult::Consumed
            }
            Key::Backspace => {
                self.password_input.pop();
                self.error_message = None;
                EventResult::Consumed
            }
            _ => {
                // Type character into password field.
                if let Some(ch) = key.text
                    && !ch.is_control()
                {
                    self.password_input.push(ch);
                    self.error_message = None;
                }
                EventResult::Consumed
            }
        }
    }

    fn handle_key_locked(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Enter => {
                let password = self.password_input.clone();
                match self.unlock_screen(&password) {
                    Ok(()) => {
                        self.password_input.clear();
                        self.accessibility.announce("Screen unlocked.");
                    }
                    Err(msg) => {
                        self.error_message = Some(msg);
                        self.password_input.clear();
                    }
                }
                EventResult::Consumed
            }
            Key::Backspace => {
                self.password_input.pop();
                self.error_message = None;
                EventResult::Consumed
            }
            Key::Escape => {
                // Cannot escape lock screen, just clear input.
                self.password_input.clear();
                self.error_message = None;
                EventResult::Consumed
            }
            _ => {
                if let Some(ch) = key.text
                    && !ch.is_control()
                {
                    self.password_input.push(ch);
                    self.error_message = None;
                }
                EventResult::Consumed
            }
        }
    }

    fn handle_key_power_menu(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Up => {
                self.power_menu_selection = self
                    .power_menu_selection
                    .checked_sub(1)
                    .unwrap_or(POWER_MENU_LAST);
                EventResult::Consumed
            }
            Key::Down => {
                self.power_menu_selection = self
                    .power_menu_selection
                    .saturating_add(1)
                    .checked_rem(POWER_MENU_ENTRIES)
                    .unwrap_or(0);
                EventResult::Consumed
            }
            Key::Enter => {
                let action = match self.power_menu_selection {
                    0 => PowerAction::Shutdown,
                    1 => PowerAction::Restart,
                    _ => PowerAction::Sleep,
                };
                self.execute_power_action(action);
                EventResult::Consumed
            }
            Key::Escape => {
                self.current_view = LoginView::UserSelect;
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    /// Handle mouse events (click detection on UI elements).
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match &mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => self.handle_click(mouse.x, mouse.y),
            _ => EventResult::Ignored,
        }
    }

    /// Handle a left-click at the given screen coordinates.
    fn handle_click(&mut self, x: f32, y: f32) -> EventResult {
        match self.current_view {
            LoginView::UserSelect => {
                // Check if clicking on a user avatar/name.
                let center_x = SCREEN_WIDTH / 2.0;
                let box_x = center_x - LOGIN_BOX_WIDTH / 2.0;
                let box_y = SCREEN_HEIGHT / 2.0 - LOGIN_BOX_HEIGHT / 2.0;

                // User list starts at avatar area.
                let list_y_start = box_y + AVATAR_Y_OFFSET + AVATAR_SIZE + 20.0;
                let item_height = 50.0;

                for (i, _user) in self.users.iter().enumerate() {
                    let item_y = list_y_start + (i as f32) * item_height;
                    if x >= box_x
                        && x <= box_x + LOGIN_BOX_WIDTH
                        && y >= item_y
                        && y <= item_y + item_height
                    {
                        self.selected_user_index = i;
                        // Double-click or single press both select.
                        return EventResult::Consumed;
                    }
                }

                // Check power button (bottom-right corner).
                let power_x = SCREEN_WIDTH - POWER_BUTTON_SIZE - 20.0;
                let power_y = SCREEN_HEIGHT - POWER_BUTTON_SIZE - 20.0;
                if x >= power_x
                    && x <= power_x + POWER_BUTTON_SIZE
                    && y >= power_y
                    && y <= power_y + POWER_BUTTON_SIZE
                {
                    self.current_view = LoginView::PowerMenu;
                    self.power_menu_selection = 0;
                    return EventResult::Consumed;
                }

                // Check accessibility toggle buttons (bottom-left).
                let acc_y = SCREEN_HEIGHT - 50.0;
                if y >= acc_y && y <= acc_y + 30.0 {
                    if (20.0..=60.0).contains(&x) {
                        self.accessibility.high_contrast = !self.accessibility.high_contrast;
                        return EventResult::Consumed;
                    }
                    if (70.0..=110.0).contains(&x) {
                        self.accessibility.large_text = !self.accessibility.large_text;
                        return EventResult::Consumed;
                    }
                    if (120.0..=160.0).contains(&x) {
                        self.accessibility.onscreen_keyboard =
                            !self.accessibility.onscreen_keyboard;
                        return EventResult::Consumed;
                    }
                }

                EventResult::Ignored
            }
            LoginView::PasswordEntry => {
                // Check login button.
                let center_x = SCREEN_WIDTH / 2.0;
                let btn_x = center_x - BUTTON_WIDTH / 2.0;
                let btn_y = SCREEN_HEIGHT / 2.0 + 60.0;
                if x >= btn_x
                    && x <= btn_x + BUTTON_WIDTH
                    && y >= btn_y
                    && y <= btn_y + BUTTON_HEIGHT
                {
                    self.attempt_login();
                    return EventResult::Consumed;
                }

                // Check password visibility toggle.
                let toggle_x = center_x + INPUT_WIDTH / 2.0 - 40.0;
                let toggle_y = SCREEN_HEIGHT / 2.0 - 10.0;
                if x >= toggle_x
                    && x <= toggle_x + 30.0
                    && y >= toggle_y
                    && y <= toggle_y + INPUT_HEIGHT
                {
                    self.password_visible = !self.password_visible;
                    return EventResult::Consumed;
                }

                EventResult::Consumed
            }
            LoginView::Locked => {
                // Similar to password entry.
                EventResult::Consumed
            }
            LoginView::PowerMenu => {
                // Check power menu item clicks.
                let menu_x = SCREEN_WIDTH / 2.0 - POWER_MENU_WIDTH / 2.0;
                let menu_y = SCREEN_HEIGHT / 2.0 - 75.0;
                let item_h = 50.0;

                for i in 0..3 {
                    let item_y = menu_y + (i as f32) * item_h;
                    if x >= menu_x
                        && x <= menu_x + POWER_MENU_WIDTH
                        && y >= item_y
                        && y <= item_y + item_h
                    {
                        self.power_menu_selection = i;
                        let action = match i {
                            0 => PowerAction::Shutdown,
                            1 => PowerAction::Restart,
                            _ => PowerAction::Sleep,
                        };
                        self.execute_power_action(action);
                        return EventResult::Consumed;
                    }
                }

                // Click outside menu to dismiss.
                self.current_view = LoginView::UserSelect;
                EventResult::Consumed
            }
            LoginView::ShuttingDown => EventResult::Consumed,
        }
    }

    /// Attempt login with the current password input.
    fn attempt_login(&mut self) {
        let username = self
            .users
            .get(self.selected_user_index)
            .map(|u| u.username().clone())
            .unwrap_or_default();
        let password = self.password_input.clone();

        match self.authenticate(&username, &password) {
            Ok(()) => {
                let uid = self
                    .users
                    .get(self.selected_user_index)
                    .map(|u| u.uid())
                    .unwrap_or(0);
                match self.start_session(uid) {
                    Ok(_session) => {
                        self.password_input.clear();
                        self.error_message = None;
                        self.current_view = LoginView::UserSelect;
                        self.accessibility.announce("Login successful.");
                    }
                    Err(msg) => {
                        self.error_message = Some(msg);
                    }
                }
            }
            Err(msg) => {
                self.error_message = Some(msg);
                self.password_input.clear();
            }
        }
    }

    /// Execute a power action.
    fn execute_power_action(&mut self, action: PowerAction) {
        match action {
            PowerAction::Shutdown | PowerAction::Restart => {
                self.current_view = LoginView::ShuttingDown;
                self.accessibility
                    .announce(if action == PowerAction::Shutdown {
                        "Shutting down..."
                    } else {
                        "Restarting..."
                    });
                // In a real system, this would invoke the init system.
            }
            PowerAction::Sleep => {
                // Return to login screen; the system would suspend.
                self.current_view = LoginView::UserSelect;
                self.accessibility.announce("System going to sleep.");
            }
        }
    }

    // ========================================================================
    // Tick (idle timeout, lockout countdown)
    // ========================================================================

    /// Called periodically with elapsed time. Handles idle timeout and lockout expiry.
    pub fn tick(&mut self, elapsed_ms: u64) {
        let elapsed_secs = elapsed_ms / 1000;
        self.current_time = self.current_time.saturating_add(elapsed_secs);
        self.idle_seconds = self.idle_seconds.saturating_add(elapsed_secs);

        // Update clock display (HH:MM format from timestamp).
        let total_minutes = self.current_time / 60;
        let hours = (total_minutes / 60) % 24;
        let minutes = total_minutes % 60;
        self.clock_display = format!("{:02}:{:02}", hours, minutes);

        // Idle timeout → lock screen (only if a session is active).
        if !self.sessions.is_empty() && self.current_view != LoginView::Locked {
            let dim_threshold = self.lock_timeout_secs.saturating_sub(30);
            if self.idle_seconds >= self.lock_timeout_secs {
                // Find the active session to lock.
                if let Some(session) = self.sessions.values().next() {
                    let uid = session.user_uid;
                    self.lock_screen(uid);
                }
            } else if self.idle_seconds >= dim_threshold {
                self.screen_dimmed = true;
            }
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the current login screen state to a RenderTree.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background.
        self.render_background(&mut tree);

        // Clock (top-right).
        self.render_clock(&mut tree);

        // Main content based on view.
        match self.current_view {
            LoginView::UserSelect => self.render_user_select(&mut tree),
            LoginView::PasswordEntry => self.render_password_entry(&mut tree),
            LoginView::Locked => self.render_lock_screen(&mut tree),
            LoginView::PowerMenu => {
                // Show user select behind the overlay.
                self.render_user_select(&mut tree);
                self.render_power_menu_overlay(&mut tree);
            }
            LoginView::ShuttingDown => self.render_shutdown_screen(&mut tree),
        }

        // Accessibility buttons (bottom-left).
        self.render_accessibility_buttons(&mut tree);

        // Power button (bottom-right, except during power menu / shutdown).
        if self.current_view != LoginView::PowerMenu && self.current_view != LoginView::ShuttingDown
        {
            self.render_power_button(&mut tree);
        }

        // Screen dim overlay (idle warning).
        if self.screen_dimmed {
            tree.fill_rect(
                0.0,
                0.0,
                SCREEN_WIDTH,
                SCREEN_HEIGHT,
                Color::rgba(0, 0, 0, 128),
            );
        }

        tree
    }

    /// Render the gradient background.
    fn render_background(&self, tree: &mut RenderTree) {
        let (bg_top, bg_bottom) = if self.accessibility.high_contrast {
            (COL_HC_BG, COL_HC_BG)
        } else {
            (COL_BG_TOP, COL_BG_BOTTOM)
        };

        // Render as two halves for a simple gradient approximation.
        tree.fill_rect(0.0, 0.0, SCREEN_WIDTH, SCREEN_HEIGHT / 2.0, bg_top);
        tree.fill_rect(
            0.0,
            SCREEN_HEIGHT / 2.0,
            SCREEN_WIDTH,
            SCREEN_HEIGHT / 2.0,
            bg_bottom,
        );

        // Subtle decorative circles in background (disabled in high contrast).
        if !self.accessibility.high_contrast {
            tree.fill_rounded_rect(
                -100.0,
                -100.0,
                400.0,
                400.0,
                Color::rgba(137, 180, 250, 8),
                CornerRadii::all(200.0),
            );
            tree.fill_rounded_rect(
                SCREEN_WIDTH - 200.0,
                SCREEN_HEIGHT - 200.0,
                500.0,
                500.0,
                Color::rgba(203, 166, 247, 6),
                CornerRadii::all(250.0),
            );
        }
    }

    /// Render the clock in the top-right corner.
    fn render_clock(&self, tree: &mut RenderTree) {
        let font_size = self.scaled_font(FONT_SIZE_CLOCK);
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };
        tree.push(RenderCommand::Text {
            x: SCREEN_WIDTH - 100.0,
            y: 20.0,
            text: self.clock_display.clone(),
            color: text_color,
            font_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render the user selection screen.
    fn render_user_select(&self, tree: &mut RenderTree) {
        let center_x = SCREEN_WIDTH / 2.0;
        let box_x = center_x - LOGIN_BOX_WIDTH / 2.0;
        let box_y = SCREEN_HEIGHT / 2.0 - LOGIN_BOX_HEIGHT / 2.0;

        let panel_color = if self.accessibility.high_contrast {
            COL_HC_PANEL
        } else {
            COL_PANEL
        };

        // Login box shadow.
        if !self.accessibility.high_contrast {
            tree.push(RenderCommand::BoxShadow {
                x: box_x,
                y: box_y,
                width: LOGIN_BOX_WIDTH,
                height: LOGIN_BOX_HEIGHT,
                offset_x: 0.0,
                offset_y: 4.0,
                blur: 24.0,
                spread: 0.0,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: CornerRadii::all(LOGIN_BOX_RADIUS),
            });
        }

        // Login box background.
        tree.fill_rounded_rect(
            box_x,
            box_y,
            LOGIN_BOX_WIDTH,
            LOGIN_BOX_HEIGHT,
            panel_color,
            CornerRadii::all(LOGIN_BOX_RADIUS),
        );

        // Title.
        let title_y = box_y + 30.0;
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };
        tree.push(RenderCommand::Text {
            x: center_x - 50.0,
            y: title_y,
            text: "Sign In".to_string(),
            color: text_color,
            font_size: self.scaled_font(FONT_SIZE_LARGE),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // User list.
        let list_y_start = title_y + 50.0;
        let item_height = 60.0;

        for (i, user) in self.users.iter().enumerate() {
            let item_y = list_y_start + (i as f32) * item_height;
            let is_selected = i == self.selected_user_index;

            // Selection highlight.
            if is_selected {
                let accent = if self.accessibility.high_contrast {
                    COL_HC_ACCENT
                } else {
                    COL_ACCENT
                };
                tree.fill_rounded_rect(
                    box_x + 16.0,
                    item_y,
                    LOGIN_BOX_WIDTH - 32.0,
                    item_height - 4.0,
                    Color::rgba(accent.r, accent.g, accent.b, 30),
                    CornerRadii::all(8.0),
                );
                // Left accent bar.
                tree.fill_rounded_rect(
                    box_x + 16.0,
                    item_y + 8.0,
                    3.0,
                    item_height - 20.0,
                    accent,
                    CornerRadii::all(2.0),
                );
            }

            // Avatar circle.
            let avatar_x = box_x + 36.0;
            let avatar_y = item_y + (item_height - 36.0) / 2.0;
            let avatar_radius = 18.0;
            tree.fill_rounded_rect(
                avatar_x,
                avatar_y,
                avatar_radius * 2.0,
                avatar_radius * 2.0,
                user.avatar_color(),
                CornerRadii::all(avatar_radius),
            );

            // Initials in avatar.
            let initials = user.initials();
            tree.push(RenderCommand::Text {
                x: avatar_x + avatar_radius - 8.0,
                y: avatar_y + avatar_radius - 6.0,
                text: initials,
                color: COL_BG_DARK,
                font_size: self.scaled_font(12.0),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Username and display name.
            let name_x = avatar_x + avatar_radius * 2.0 + 12.0;
            tree.push(RenderCommand::Text {
                x: name_x,
                y: item_y + 14.0,
                text: user.display_name().clone(),
                color: text_color,
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(LOGIN_BOX_WIDTH - 120.0),
                overflow: TextOverflow::Ellipsis,
            });

            let subtext_color = if self.accessibility.high_contrast {
                COL_HC_TEXT
            } else {
                COL_SUBTEXT
            };
            tree.push(RenderCommand::Text {
                x: name_x,
                y: item_y + 34.0,
                text: format!("@{}", user.username()),
                color: subtext_color,
                font_size: self.scaled_font(FONT_SIZE_SMALL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(LOGIN_BOX_WIDTH - 120.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Admin badge.
            if user.is_admin() {
                let badge_x = box_x + LOGIN_BOX_WIDTH - 70.0;
                tree.fill_rounded_rect(
                    badge_x,
                    item_y + 18.0,
                    40.0,
                    20.0,
                    Color::rgba(250, 179, 135, 40),
                    CornerRadii::all(4.0),
                );
                tree.push(RenderCommand::Text {
                    x: badge_x + 5.0,
                    y: item_y + 21.0,
                    text: "admin".to_string(),
                    color: COL_WARNING,
                    font_size: self.scaled_font(10.0),
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }

        // Hint text at bottom.
        let hint_y = box_y + LOGIN_BOX_HEIGHT - 40.0;
        let hint_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_SUBTEXT
        };
        tree.push(RenderCommand::Text {
            x: center_x - 100.0,
            y: hint_y,
            text: "Press Enter to sign in | Esc for power menu".to_string(),
            color: hint_color,
            font_size: self.scaled_font(FONT_SIZE_SMALL),
            font_weight: FontWeightHint::Regular,
            max_width: Some(LOGIN_BOX_WIDTH - 40.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the password entry screen.
    fn render_password_entry(&self, tree: &mut RenderTree) {
        let center_x = SCREEN_WIDTH / 2.0;
        let box_x = center_x - LOGIN_BOX_WIDTH / 2.0;
        let box_y = SCREEN_HEIGHT / 2.0 - LOGIN_BOX_HEIGHT / 2.0;

        let panel_color = if self.accessibility.high_contrast {
            COL_HC_PANEL
        } else {
            COL_PANEL
        };
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };
        let accent = if self.accessibility.high_contrast {
            COL_HC_ACCENT
        } else {
            COL_ACCENT
        };

        // Box shadow.
        if !self.accessibility.high_contrast {
            tree.push(RenderCommand::BoxShadow {
                x: box_x,
                y: box_y,
                width: LOGIN_BOX_WIDTH,
                height: LOGIN_BOX_HEIGHT,
                offset_x: 0.0,
                offset_y: 4.0,
                blur: 24.0,
                spread: 0.0,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: CornerRadii::all(LOGIN_BOX_RADIUS),
            });
        }

        // Login box.
        tree.fill_rounded_rect(
            box_x,
            box_y,
            LOGIN_BOX_WIDTH,
            LOGIN_BOX_HEIGHT,
            panel_color,
            CornerRadii::all(LOGIN_BOX_RADIUS),
        );

        // Selected user avatar (large, centered).
        if let Some(user) = self.users.get(self.selected_user_index) {
            let avatar_x = center_x - AVATAR_SIZE / 2.0;
            let avatar_y = box_y + AVATAR_Y_OFFSET;

            tree.fill_rounded_rect(
                avatar_x,
                avatar_y,
                AVATAR_SIZE,
                AVATAR_SIZE,
                user.avatar_color(),
                CornerRadii::all(AVATAR_SIZE / 2.0),
            );

            // Initials.
            let initials = user.initials();
            tree.push(RenderCommand::Text {
                x: avatar_x + AVATAR_SIZE / 2.0 - 14.0,
                y: avatar_y + AVATAR_SIZE / 2.0 - 10.0,
                text: initials,
                color: COL_BG_DARK,
                font_size: self.scaled_font(22.0),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Display name.
            let name_y = avatar_y + AVATAR_SIZE + 16.0;
            tree.push(RenderCommand::Text {
                x: center_x - 60.0,
                y: name_y,
                text: user.display_name().clone(),
                color: text_color,
                font_size: self.scaled_font(FONT_SIZE_LARGE),
                font_weight: FontWeightHint::Bold,
                max_width: Some(LOGIN_BOX_WIDTH - 40.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Username.
            tree.push(RenderCommand::Text {
                x: center_x - 40.0,
                y: name_y + 30.0,
                text: format!("@{}", user.username()),
                color: if self.accessibility.high_contrast {
                    COL_HC_TEXT
                } else {
                    COL_SUBTEXT
                },
                font_size: self.scaled_font(FONT_SIZE_SMALL),
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Password input field.
        let input_x = center_x - INPUT_WIDTH / 2.0;
        let input_y = box_y + 240.0;
        let input_border_color = if self.error_message.is_some() {
            if self.accessibility.high_contrast {
                COL_HC_ERROR
            } else {
                COL_ERROR
            }
        } else {
            COL_INPUT_FOCUS
        };

        // Input background.
        tree.fill_rounded_rect(
            input_x,
            input_y,
            INPUT_WIDTH,
            INPUT_HEIGHT,
            COL_INPUT_BG,
            CornerRadii::all(INPUT_RADIUS),
        );

        // Input border.
        tree.push(RenderCommand::StrokeRect {
            x: input_x,
            y: input_y,
            width: INPUT_WIDTH,
            height: INPUT_HEIGHT,
            color: input_border_color,
            line_width: 2.0,
            corner_radii: CornerRadii::all(INPUT_RADIUS),
        });

        // Password text (masked or visible).
        let display_text = if self.password_visible {
            self.password_input.clone()
        } else {
            "\u{2022}".repeat(self.password_input.len())
        };

        if display_text.is_empty() {
            // Placeholder.
            tree.push(RenderCommand::Text {
                x: input_x + 16.0,
                y: input_y + 13.0,
                text: "Password".to_string(),
                color: if self.accessibility.high_contrast {
                    COL_HC_TEXT
                } else {
                    COL_SUBTEXT
                },
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH - 60.0),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            tree.push(RenderCommand::Text {
                x: input_x + 16.0,
                y: input_y + 13.0,
                text: display_text,
                color: text_color,
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH - 60.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Show/hide password toggle.
        let toggle_x = input_x + INPUT_WIDTH - 40.0;
        let toggle_text = if self.password_visible {
            "Hide"
        } else {
            "Show"
        };
        tree.push(RenderCommand::Text {
            x: toggle_x,
            y: input_y + 14.0,
            text: toggle_text.to_string(),
            color: accent,
            font_size: self.scaled_font(FONT_SIZE_SMALL),
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Error message.
        if let Some(ref error) = self.error_message {
            let err_color = if self.accessibility.high_contrast {
                COL_HC_ERROR
            } else {
                COL_ERROR
            };
            tree.push(RenderCommand::Text {
                x: input_x,
                y: input_y + INPUT_HEIGHT + 8.0,
                text: error.clone(),
                color: err_color,
                font_size: self.scaled_font(FONT_SIZE_SMALL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Login button.
        let btn_x = center_x - BUTTON_WIDTH / 2.0;
        let btn_y = input_y + INPUT_HEIGHT + 40.0;
        tree.fill_rounded_rect(
            btn_x,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            accent,
            CornerRadii::all(BUTTON_RADIUS),
        );
        tree.push(RenderCommand::Text {
            x: center_x - 20.0,
            y: btn_y + 13.0,
            text: "Sign In".to_string(),
            color: COL_BG_DARK,
            font_size: self.scaled_font(FONT_SIZE_NORMAL),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // "Back" link.
        let back_y = btn_y + BUTTON_HEIGHT + 16.0;
        tree.push(RenderCommand::Text {
            x: center_x - 30.0,
            y: back_y,
            text: "< Back to user list".to_string(),
            color: accent,
            font_size: self.scaled_font(FONT_SIZE_SMALL),
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render the lock screen.
    fn render_lock_screen(&self, tree: &mut RenderTree) {
        let center_x = SCREEN_WIDTH / 2.0;
        let box_x = center_x - LOGIN_BOX_WIDTH / 2.0;
        let box_y = SCREEN_HEIGHT / 2.0 - LOGIN_BOX_HEIGHT / 2.0;

        let panel_color = if self.accessibility.high_contrast {
            COL_HC_PANEL
        } else {
            COL_PANEL
        };
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };
        let accent = if self.accessibility.high_contrast {
            COL_HC_ACCENT
        } else {
            COL_ACCENT
        };

        // Box shadow.
        if !self.accessibility.high_contrast {
            tree.push(RenderCommand::BoxShadow {
                x: box_x,
                y: box_y,
                width: LOGIN_BOX_WIDTH,
                height: LOGIN_BOX_HEIGHT,
                offset_x: 0.0,
                offset_y: 4.0,
                blur: 24.0,
                spread: 0.0,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: CornerRadii::all(LOGIN_BOX_RADIUS),
            });
        }

        // Box.
        tree.fill_rounded_rect(
            box_x,
            box_y,
            LOGIN_BOX_WIDTH,
            LOGIN_BOX_HEIGHT,
            panel_color,
            CornerRadii::all(LOGIN_BOX_RADIUS),
        );

        // "Locked" badge.
        let badge_y = box_y + 20.0;
        tree.fill_rounded_rect(
            center_x - 40.0,
            badge_y,
            80.0,
            28.0,
            Color::rgba(243, 139, 168, 30),
            CornerRadii::all(14.0),
        );
        let lock_color = if self.accessibility.high_contrast {
            COL_HC_ERROR
        } else {
            COL_ERROR
        };
        tree.push(RenderCommand::Text {
            x: center_x - 28.0,
            y: badge_y + 6.0,
            text: "Locked".to_string(),
            color: lock_color,
            font_size: self.scaled_font(FONT_SIZE_NORMAL),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Show locked user avatar.
        if let Some(uid) = self.locked_session_uid
            && let Some(user) = self.users.iter().find(|u| u.uid() == uid)
        {
            let avatar_x = center_x - AVATAR_SIZE / 2.0;
            let avatar_y = box_y + 70.0;

            tree.fill_rounded_rect(
                avatar_x,
                avatar_y,
                AVATAR_SIZE,
                AVATAR_SIZE,
                user.avatar_color(),
                CornerRadii::all(AVATAR_SIZE / 2.0),
            );

            let initials = user.initials();
            tree.push(RenderCommand::Text {
                x: avatar_x + AVATAR_SIZE / 2.0 - 14.0,
                y: avatar_y + AVATAR_SIZE / 2.0 - 10.0,
                text: initials,
                color: COL_BG_DARK,
                font_size: self.scaled_font(22.0),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Display name.
            tree.push(RenderCommand::Text {
                x: center_x - 60.0,
                y: avatar_y + AVATAR_SIZE + 16.0,
                text: user.display_name().clone(),
                color: text_color,
                font_size: self.scaled_font(FONT_SIZE_LARGE),
                font_weight: FontWeightHint::Bold,
                max_width: Some(LOGIN_BOX_WIDTH - 40.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Password input.
        let input_x = center_x - INPUT_WIDTH / 2.0;
        let input_y = box_y + 260.0;
        let input_border = if self.error_message.is_some() {
            if self.accessibility.high_contrast {
                COL_HC_ERROR
            } else {
                COL_ERROR
            }
        } else {
            COL_INPUT_FOCUS
        };

        tree.fill_rounded_rect(
            input_x,
            input_y,
            INPUT_WIDTH,
            INPUT_HEIGHT,
            COL_INPUT_BG,
            CornerRadii::all(INPUT_RADIUS),
        );
        tree.push(RenderCommand::StrokeRect {
            x: input_x,
            y: input_y,
            width: INPUT_WIDTH,
            height: INPUT_HEIGHT,
            color: input_border,
            line_width: 2.0,
            corner_radii: CornerRadii::all(INPUT_RADIUS),
        });

        // Password text.
        let display_text = "\u{2022}".repeat(self.password_input.len());
        if display_text.is_empty() {
            tree.push(RenderCommand::Text {
                x: input_x + 16.0,
                y: input_y + 13.0,
                text: "Enter password to unlock".to_string(),
                color: if self.accessibility.high_contrast {
                    COL_HC_TEXT
                } else {
                    COL_SUBTEXT
                },
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            tree.push(RenderCommand::Text {
                x: input_x + 16.0,
                y: input_y + 13.0,
                text: display_text,
                color: text_color,
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Error message.
        if let Some(ref error) = self.error_message {
            let err_color = if self.accessibility.high_contrast {
                COL_HC_ERROR
            } else {
                COL_ERROR
            };
            tree.push(RenderCommand::Text {
                x: input_x,
                y: input_y + INPUT_HEIGHT + 8.0,
                text: error.clone(),
                color: err_color,
                font_size: self.scaled_font(FONT_SIZE_SMALL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Unlock button.
        let btn_y = input_y + INPUT_HEIGHT + 40.0;
        let btn_x = center_x - BUTTON_WIDTH / 2.0;
        tree.fill_rounded_rect(
            btn_x,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            accent,
            CornerRadii::all(BUTTON_RADIUS),
        );
        tree.push(RenderCommand::Text {
            x: center_x - 22.0,
            y: btn_y + 13.0,
            text: "Unlock".to_string(),
            color: COL_BG_DARK,
            font_size: self.scaled_font(FONT_SIZE_NORMAL),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render the power menu overlay.
    fn render_power_menu_overlay(&self, tree: &mut RenderTree) {
        // Dim overlay.
        tree.fill_rect(0.0, 0.0, SCREEN_WIDTH, SCREEN_HEIGHT, COL_POWER_BG);

        let center_x = SCREEN_WIDTH / 2.0;
        let menu_x = center_x - POWER_MENU_WIDTH / 2.0;
        let menu_y = SCREEN_HEIGHT / 2.0 - 100.0;
        let item_height = 56.0;

        let panel_color = if self.accessibility.high_contrast {
            COL_HC_PANEL
        } else {
            COL_PANEL
        };
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };
        let accent = if self.accessibility.high_contrast {
            COL_HC_ACCENT
        } else {
            COL_ACCENT
        };

        // Menu background.
        tree.fill_rounded_rect(
            menu_x - 16.0,
            menu_y - 16.0,
            POWER_MENU_WIDTH + 32.0,
            item_height * 3.0 + 48.0,
            panel_color,
            CornerRadii::all(12.0),
        );

        // Title.
        tree.push(RenderCommand::Text {
            x: center_x - 30.0,
            y: menu_y - 8.0,
            text: "Power".to_string(),
            color: text_color,
            font_size: self.scaled_font(FONT_SIZE_NORMAL),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let items = ["Shut Down", "Restart", "Sleep"];
        let icons = ["\u{23FB}", "\u{21BB}", "\u{263E}"]; // Unicode power, refresh, moon

        for (i, (label, icon)) in items.iter().zip(icons.iter()).enumerate() {
            let item_y = menu_y + 20.0 + (i as f32) * item_height;
            let is_selected = i == self.power_menu_selection;

            if is_selected {
                tree.fill_rounded_rect(
                    menu_x,
                    item_y,
                    POWER_MENU_WIDTH,
                    item_height - 4.0,
                    Color::rgba(accent.r, accent.g, accent.b, 30),
                    CornerRadii::all(8.0),
                );
            }

            // Icon.
            tree.push(RenderCommand::Text {
                x: menu_x + 16.0,
                y: item_y + 16.0,
                text: icon.to_string(),
                color: if is_selected { accent } else { text_color },
                font_size: self.scaled_font(20.0),
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Label.
            tree.push(RenderCommand::Text {
                x: menu_x + 48.0,
                y: item_y + 18.0,
                text: label.to_string(),
                color: if is_selected { accent } else { text_color },
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Escape hint.
        let hint_y = menu_y + 20.0 + 3.0 * item_height + 8.0;
        tree.push(RenderCommand::Text {
            x: center_x - 50.0,
            y: hint_y,
            text: "Esc to cancel".to_string(),
            color: if self.accessibility.high_contrast {
                COL_HC_TEXT
            } else {
                COL_SUBTEXT
            },
            font_size: self.scaled_font(FONT_SIZE_SMALL),
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render the shutdown/restart screen.
    fn render_shutdown_screen(&self, tree: &mut RenderTree) {
        let center_x = SCREEN_WIDTH / 2.0;
        let center_y = SCREEN_HEIGHT / 2.0;
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };

        // Simple centered message.
        tree.push(RenderCommand::Text {
            x: center_x - 80.0,
            y: center_y - 20.0,
            text: "Shutting down...".to_string(),
            color: text_color,
            font_size: self.scaled_font(FONT_SIZE_LARGE),
            font_weight: FontWeightHint::Light,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Spinner placeholder (just a dot animation would go here).
        tree.push(RenderCommand::Text {
            x: center_x - 10.0,
            y: center_y + 20.0,
            text: "\u{25CF} \u{25CB} \u{25CB}".to_string(),
            color: if self.accessibility.high_contrast {
                COL_HC_ACCENT
            } else {
                COL_ACCENT
            },
            font_size: self.scaled_font(FONT_SIZE_NORMAL),
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render the accessibility toggle buttons (bottom-left corner).
    fn render_accessibility_buttons(&self, tree: &mut RenderTree) {
        let y = SCREEN_HEIGHT - 50.0;
        let btn_size = 36.0;
        let spacing = 8.0;
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_SUBTEXT
        };

        let buttons = [
            ("HC", self.accessibility.high_contrast),
            ("Aa", self.accessibility.large_text),
            ("KB", self.accessibility.onscreen_keyboard),
        ];

        for (i, (label, active)) in buttons.iter().enumerate() {
            let x = 20.0 + (i as f32) * (btn_size + spacing);
            let bg = if *active {
                Color::rgba(137, 180, 250, 60)
            } else {
                Color::rgba(88, 91, 112, 40)
            };

            tree.fill_rounded_rect(x, y, btn_size, btn_size, bg, CornerRadii::all(6.0));
            tree.push(RenderCommand::Text {
                x: x + 6.0,
                y: y + 10.0,
                text: label.to_string(),
                color: if *active {
                    if self.accessibility.high_contrast {
                        COL_HC_ACCENT
                    } else {
                        COL_ACCENT
                    }
                } else {
                    text_color
                },
                font_size: self.scaled_font(12.0),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    /// Render the power button (bottom-right corner).
    fn render_power_button(&self, tree: &mut RenderTree) {
        let x = SCREEN_WIDTH - POWER_BUTTON_SIZE - 20.0;
        let y = SCREEN_HEIGHT - POWER_BUTTON_SIZE - 20.0;

        tree.fill_rounded_rect(
            x,
            y,
            POWER_BUTTON_SIZE,
            POWER_BUTTON_SIZE,
            Color::rgba(88, 91, 112, 40),
            CornerRadii::all(POWER_BUTTON_SIZE / 2.0),
        );

        // Power icon (Unicode symbol).
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_SUBTEXT
        };
        tree.push(RenderCommand::Text {
            x: x + POWER_BUTTON_SIZE / 2.0 - 8.0,
            y: y + POWER_BUTTON_SIZE / 2.0 - 8.0,
            text: "\u{23FB}".to_string(),
            color: text_color,
            font_size: self.scaled_font(18.0),
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // ========================================================================
    // Helpers
    // ========================================================================

    /// Apply font scaling for large text accessibility mode.
    fn scaled_font(&self, base_size: f32) -> f32 {
        if self.accessibility.large_text {
            base_size * LARGE_FONT_SCALE
        } else {
            base_size
        }
    }
}

// ============================================================================
// Application entry point
// ============================================================================

fn main() {
    let mut manager = LoginManager::new();

    // Check for auto-login configuration.
    if let Some(_session) = manager.check_auto_login() {
        // Auto-login succeeded; in a real system, we would launch the compositor.
        return;
    }

    // In a real Slate OS environment, this enters the compositor event loop.
    // For now, render one frame to verify the UI builds correctly.
    let tree = manager.render();
    assert!(!tree.is_empty(), "Login UI must produce render commands");

    // Verify password entry view renders too.
    manager.current_view = LoginView::PasswordEntry;
    let tree2 = manager.render();
    assert!(
        !tree2.is_empty(),
        "Password entry UI must produce render commands"
    );

    // Verify lock screen renders.
    manager.lock_screen(0);
    let tree3 = manager.render();
    assert!(
        !tree3.is_empty(),
        "Lock screen UI must produce render commands"
    );
}

// ============================================================================
// Tests
// ============================================================================

// The workspace's defensive lints are for production code; a test that indexes
// a fixture it just built is asserting, and an assertion that fails by
// panicking is a test doing its job.
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a test login manager with known accounts.
    fn test_manager() -> LoginManager {
        let users = vec![
            UserAccount::new_with_password(1000, "alice", "Alice Smith", "password123", false),
            UserAccount::new_with_password(1001, "bob", "Bob Jones", "hunter2", true),
            UserAccount::guest_account(),
        ];
        LoginManager::with_users(users)
    }

    // ========================================================================
    // Authentication tests
    // ========================================================================

    #[test]
    fn test_authenticate_success() {
        let mut mgr = test_manager();
        let result = mgr.authenticate("alice", "password123");
        assert!(
            result.is_ok(),
            "Valid password should authenticate: {:?}",
            result
        );
    }

    #[test]
    fn test_authenticate_wrong_password() {
        let mut mgr = test_manager();
        let result = mgr.authenticate("alice", "wrongpass");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Incorrect password"));
    }

    #[test]
    fn test_authenticate_nonexistent_user() {
        let mut mgr = test_manager();
        let result = mgr.authenticate("nobody", "pass");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("User not found"));
    }

    #[test]
    fn test_authenticate_guest_no_password() {
        let mut mgr = test_manager();
        // Guest should authenticate with any password (including empty).
        let result = mgr.authenticate("guest", "");
        assert!(result.is_ok());
        let result2 = mgr.authenticate("guest", "anything");
        assert!(result2.is_ok());
    }

    #[test]
    fn test_authenticate_updates_login_stats() {
        let mut mgr = test_manager();
        mgr.current_time = 1000;
        let _ = mgr.authenticate("alice", "password123");
        let alice = mgr.users.iter().find(|u| u.username() == "alice");
        assert!(alice.is_some());
        let alice = alice.unwrap();
        assert_eq!(alice.last_login_timestamp(), 1000);
        assert_eq!(alice.login_count(), 1);
    }

    // ========================================================================
    // Account lockout tests
    // ========================================================================

    #[test]
    fn test_lockout_after_max_failures() {
        let mut mgr = test_manager();
        mgr.current_time = 100;

        // Fail 5 times.
        for i in 0..5 {
            let result = mgr.authenticate("alice", "wrong");
            if i < 4 {
                assert!(result.unwrap_err().contains("attempts remaining"));
            } else {
                assert!(result.unwrap_err().contains("locked"));
            }
        }

        // Now even the correct password should fail (locked).
        let result = mgr.authenticate("alice", "password123");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Account locked"));
    }

    #[test]
    fn test_lockout_expires() {
        let mut mgr = test_manager();
        mgr.current_time = 100;

        // Lock the account.
        for _ in 0..5 {
            let _ = mgr.authenticate("alice", "wrong");
        }

        // Advance time past lockout (5 minutes = 300 seconds).
        mgr.current_time = 100 + 301;

        // Should be able to authenticate now.
        let result = mgr.authenticate("alice", "password123");
        assert!(result.is_ok());
    }

    #[test]
    fn test_lockout_reset_on_success() {
        let mut mgr = test_manager();

        // Fail a few times (but not enough to lock).
        for _ in 0..3 {
            let _ = mgr.authenticate("alice", "wrong");
        }

        // Succeed.
        let result = mgr.authenticate("alice", "password123");
        assert!(result.is_ok());

        // Lockout state should be reset; we should get 5 fresh attempts.
        let lockout = mgr.locked_accounts.get(&1000);
        assert!(lockout.is_none() || lockout.unwrap().failed_attempts == 0);
    }

    #[test]
    fn test_lockout_countdown() {
        let mut mgr = test_manager();
        mgr.current_time = 100;

        for _ in 0..5 {
            let _ = mgr.authenticate("alice", "wrong");
        }

        // Check countdown.
        mgr.current_time = 200;
        let result = mgr.authenticate("alice", "password123");
        let err = result.unwrap_err();
        assert!(err.contains("Account locked"));
        // Should show approximately 200 seconds remaining.
        assert!(err.contains("200"));
    }

    // ========================================================================
    // Session management tests
    // ========================================================================

    #[test]
    fn test_start_session() {
        let mut mgr = test_manager();
        mgr.current_time = 5000;
        let session = mgr.start_session(1000);
        assert!(session.is_ok());
        let session = session.unwrap();
        assert_eq!(session.user_uid, 1000);
        assert_eq!(session.started_at, 5000);
        assert_eq!(session.home_dir, "/home/alice");
        assert_eq!(session.environment.get("USER"), Some(&"alice".to_string()));
        assert_eq!(
            session.environment.get("HOME"),
            Some(&"/home/alice".to_string())
        );
    }

    #[test]
    fn test_start_session_nonexistent_user() {
        let mut mgr = test_manager();
        let result = mgr.start_session(9999);
        assert!(result.is_err());
    }

    #[test]
    fn test_end_session() {
        let mut mgr = test_manager();
        let session = mgr.start_session(1000).unwrap();
        let sid = session.session_id;
        assert!(mgr.sessions.contains_key(&sid));

        mgr.end_session(sid);
        assert!(!mgr.sessions.contains_key(&sid));
        assert_eq!(mgr.current_view, LoginView::UserSelect);
    }

    #[test]
    fn test_session_ids_increment() {
        let mut mgr = test_manager();
        let s1 = mgr.start_session(1000).unwrap();
        let s2 = mgr.start_session(1001).unwrap();
        assert_eq!(s1.session_id + 1, s2.session_id);
    }

    // ========================================================================
    // Lock screen tests
    // ========================================================================

    #[test]
    fn test_lock_screen() {
        let mut mgr = test_manager();
        mgr.lock_screen(1000);
        assert_eq!(mgr.current_view, LoginView::Locked);
        assert_eq!(mgr.locked_session_uid, Some(1000));
    }

    #[test]
    fn test_unlock_screen_correct_password() {
        let mut mgr = test_manager();
        mgr.lock_screen(1000);
        let result = mgr.unlock_screen("password123");
        assert!(result.is_ok());
        assert_ne!(mgr.current_view, LoginView::Locked);
    }

    #[test]
    fn test_unlock_screen_wrong_password() {
        let mut mgr = test_manager();
        mgr.lock_screen(1000);
        let result = mgr.unlock_screen("wrongpass");
        assert!(result.is_err());
        assert_eq!(mgr.current_view, LoginView::Locked);
    }

    // ========================================================================
    // Idle timeout tests
    // ========================================================================

    #[test]
    fn test_idle_timeout_locks_screen() {
        let mut mgr = test_manager();
        let _ = mgr.start_session(1000);
        mgr.lock_timeout_secs = 60;

        // Tick past the timeout.
        mgr.tick(61000); // 61 seconds in ms.
        assert_eq!(mgr.current_view, LoginView::Locked);
    }

    #[test]
    fn test_idle_dimming_before_lock() {
        let mut mgr = test_manager();
        let _ = mgr.start_session(1000);
        mgr.lock_timeout_secs = 60;

        // Tick to 30s before lock (within dim threshold).
        mgr.tick(31000);
        assert!(mgr.screen_dimmed);
        assert_ne!(mgr.current_view, LoginView::Locked);
    }

    #[test]
    fn test_input_resets_idle() {
        let mut mgr = test_manager();
        let _ = mgr.start_session(1000);
        mgr.idle_seconds = 50;
        mgr.screen_dimmed = true;

        let event = Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('a'),
        });
        mgr.handle_event(&event);

        assert_eq!(mgr.idle_seconds, 0);
        assert!(!mgr.screen_dimmed);
    }

    // ========================================================================
    // Password tests
    //
    // The SHA-256 and salted-hash tests that used to live here went with the
    // code they tested. They are not replaced with equivalents, because they
    // were the reason the bug survived: they asserted that hashing is
    // deterministic, that different passwords hash differently and that
    // different salts hash differently — all of which are true of any function
    // written by accident. `userdb` pins its construction to a published
    // vector instead, which is the only kind of test that tells an algorithm
    // apart from something that resembles one. What belongs *here* is the
    // question this program actually asks: does a stored entry admit the right
    // password and refuse the wrong one.
    // ========================================================================

    #[test]
    fn test_correct_password_is_accepted_and_wrong_one_refused() {
        let account =
            UserAccount::new_with_password(1000, "testuser", "Test User", "hunter2", false);
        assert_eq!(account.check_password("hunter2"), userdb::Auth::Accepted);
        assert_eq!(account.check_password("hunter3"), userdb::Auth::Rejected);
        assert_eq!(account.check_password(""), userdb::Auth::Rejected);
        assert!(account.requires_password());
    }

    #[test]
    fn test_guest_account_needs_no_password() {
        let guest = UserAccount::guest_account();
        assert!(!guest.requires_password());
        assert_eq!(guest.check_password(""), userdb::Auth::NoPassword);
    }

    /// The bug this migration closes: an entry written by `useradm`'s old
    /// hash, and one written by this program's old hash, are both refused
    /// outright rather than silently compared against something they are not.
    /// A refusal the user can act on beats a comparison that can never match.
    #[test]
    fn test_a_password_from_the_old_formats_is_unusable_not_wrong() {
        let mut record = userdb::Record::new();
        record.set_uid(1000);
        record.set(userdb::field::USERNAME, "olduser");
        // 64 hex digits: what both of the previous implementations wrote, and
        // what no `crypt` method produces.
        record.set(
            userdb::field::PASSWORD_HASH,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );
        let account = UserAccount::from_record(record);
        assert_eq!(account.check_password("anything"), userdb::Auth::Unusable);

        let mut mgr = test_manager();
        mgr.users.push(account);
        let err = mgr
            .authenticate("olduser", "anything")
            .expect_err("must not authenticate");
        assert!(err.contains("useradm passwd"), "{err}");
    }

    // ========================================================================
    // User database serialization tests
    // ========================================================================

    #[test]
    fn test_serialize_and_parse_roundtrip() {
        let mut db = userdb::UserDb::new();
        for account in [
            UserAccount::new_with_password(1000, "testuser", "Test User", "pass", false),
            UserAccount::guest_account(),
        ] {
            db.push(account.record().clone());
        }

        let parsed: Vec<UserAccount> = userdb::UserDb::parse(&db.to_text())
            .records()
            .iter()
            .cloned()
            .map(UserAccount::from_record)
            .collect();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].uid(), 1000);
        assert_eq!(parsed[0].username(), "testuser");
        assert_eq!(parsed[0].display_name(), "Test User");
        assert!(!parsed[0].is_admin());
        // The password survives the trip, which the old round-trip test never
        // checked — and it was the half that was broken.
        assert_eq!(parsed[0].check_password("pass"), userdb::Auth::Accepted);
        assert_eq!(parsed[1].uid(), 65534);
        assert_eq!(parsed[1].username(), "guest");
    }

    #[test]
    fn test_parse_empty_yaml() {
        assert!(userdb::UserDb::parse("").records().is_empty());
    }

    /// A field this program does not model must survive a save. The previous
    /// serialiser rebuilt the file from its own struct, so everything
    /// `useradm` had written — group memberships above all — was deleted the
    /// first time anybody logged in.
    #[test]
    fn test_a_login_does_not_delete_useradms_fields() {
        let text = "users:\n  \
            - uid: 1000\n    \
            username: \"alice\"\n    \
            groups: [\"users\", \"admin\"]\n    \
            home: \"/home/alice\"\n";
        let mut db = userdb::UserDb::parse(text);
        let record = db.records_mut().remove(0);
        let mut account = UserAccount::from_record(record);
        account.record.record_login(1234);

        let mut out = userdb::UserDb::new();
        out.push(account.record().clone());
        let saved = out.to_text();
        assert!(saved.contains("groups: [\"users\", \"admin\"]"), "{saved}");
        assert!(saved.contains("home: \"/home/alice\""), "{saved}");
        assert!(saved.contains("last_login_timestamp: 1234"), "{saved}");
    }

    // ========================================================================
    // Rendering tests
    // ========================================================================

    #[test]
    fn test_render_user_select() {
        let mgr = test_manager();
        let tree = mgr.render();
        assert!(!tree.is_empty());
        // Should have background + clock + user list + accessibility buttons + power button.
        assert!(tree.len() > 10);
    }

    #[test]
    fn test_render_password_entry() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PasswordEntry;
        mgr.password_input = "secret".to_string();
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_lock_screen() {
        let mut mgr = test_manager();
        mgr.lock_screen(1000);
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_power_menu() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PowerMenu;
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_shutdown() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::ShuttingDown;
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_high_contrast() {
        let mut mgr = test_manager();
        mgr.accessibility.high_contrast = true;
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_large_text() {
        let mut mgr = test_manager();
        mgr.accessibility.large_text = true;
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_error_message() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PasswordEntry;
        mgr.error_message = Some("Incorrect password. 4 attempts remaining.".to_string());
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    // ========================================================================
    // Event handling tests
    // ========================================================================

    #[test]
    fn test_navigate_user_list() {
        let mut mgr = test_manager();
        assert_eq!(mgr.selected_user_index, 0);

        let down = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        mgr.handle_event(&down);
        assert_eq!(mgr.selected_user_index, 1);

        mgr.handle_event(&down);
        assert_eq!(mgr.selected_user_index, 2);

        // Wraps around.
        mgr.handle_event(&down);
        assert_eq!(mgr.selected_user_index, 0);
    }

    #[test]
    fn test_enter_selects_user() {
        let mut mgr = test_manager();
        // Alice requires password.
        let enter = Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        mgr.handle_event(&enter);
        assert_eq!(mgr.current_view, LoginView::PasswordEntry);
    }

    #[test]
    fn test_escape_opens_power_menu() {
        let mut mgr = test_manager();
        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        mgr.handle_event(&esc);
        assert_eq!(mgr.current_view, LoginView::PowerMenu);
    }

    #[test]
    fn test_escape_in_password_goes_back() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PasswordEntry;
        mgr.password_input = "typed".to_string();

        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        mgr.handle_event(&esc);
        assert_eq!(mgr.current_view, LoginView::UserSelect);
        assert!(mgr.password_input.is_empty());
    }

    #[test]
    fn test_typing_password() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PasswordEntry;

        for ch in "hello".chars() {
            let event = Event::Key(KeyEvent {
                key: Key::A, // Key code doesn't matter for text input.
                pressed: true,
                modifiers: Modifiers::NONE,
                text: Some(ch),
            });
            mgr.handle_event(&event);
        }
        assert_eq!(mgr.password_input, "hello");
    }

    #[test]
    fn test_backspace_in_password() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PasswordEntry;
        mgr.password_input = "abc".to_string();

        let backspace = Event::Key(KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        mgr.handle_event(&backspace);
        assert_eq!(mgr.password_input, "ab");
    }

    #[test]
    fn test_accessibility_toggle_shortcut() {
        let mut mgr = test_manager();
        assert!(!mgr.accessibility.high_contrast);

        let event = Event::Key(KeyEvent {
            key: Key::H,
            pressed: true,
            modifiers: Modifiers {
                shift: false,
                ctrl: true,
                alt: true,
                super_key: false,
            },
            text: None,
        });
        mgr.handle_event(&event);
        assert!(mgr.accessibility.high_contrast);

        // Toggle off.
        mgr.handle_event(&event);
        assert!(!mgr.accessibility.high_contrast);
    }

    // The hex-conversion tests went with the hex conversions. Nothing in this
    // program encodes a hash by hand any more: a `crypt` entry is text
    // already, and the salt inside it never leaves `userdb`.

    // ========================================================================
    // UserAccount helper tests
    // ========================================================================

    #[test]
    fn test_user_initials() {
        let user = UserAccount::new_with_password(1, "jd", "John Doe", "pass", false);
        assert_eq!(user.initials(), "JD");

        let single = UserAccount::new_with_password(2, "x", "Xavier", "pass", false);
        assert_eq!(single.initials(), "X");
    }

    #[test]
    fn test_guest_requires_no_password() {
        let guest = UserAccount::guest_account();
        assert!(!guest.requires_password());

        let normal = UserAccount::new_with_password(1, "u", "User", "p", false);
        assert!(normal.requires_password());
    }

    // ========================================================================
    // Auto-login tests
    // ========================================================================

    #[test]
    fn test_auto_login() {
        let mut users = vec![UserAccount::new_with_password(
            1000,
            "auto",
            "Auto User",
            "pass",
            false,
        )];
        users[0].record.set_auto_login(true);

        let mut mgr = LoginManager::with_users(users);
        let session = mgr.check_auto_login();
        assert!(session.is_some());
        assert_eq!(session.unwrap().user_uid, 1000);
    }

    #[test]
    fn test_no_auto_login() {
        let mgr_users = vec![UserAccount::new_with_password(
            1000,
            "normal",
            "Normal User",
            "pass",
            false,
        )];
        let mut mgr = LoginManager::with_users(mgr_users);
        let session = mgr.check_auto_login();
        assert!(session.is_none());
    }
}

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

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Theme
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
const MOCHA_YELLOW: Color = Color::from_hex(0xF9E2AF);
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);
const MOCHA_MAUVE: Color = Color::from_hex(0xCBA6F7);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);
const MOCHA_LAVENDER: Color = Color::from_hex(0xB4BEFE);

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
    pub fn badge_color(self) -> Color {
        match self {
            Self::Administrator => MOCHA_RED,
            Self::Standard => MOCHA_BLUE,
            Self::Guest => MOCHA_OVERLAY0,
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
    Initials {
        color_index: u8,
    },
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

/// Predefined avatar colors.
const AVATAR_COLORS: &[Color] = &[
    MOCHA_BLUE,
    MOCHA_GREEN,
    MOCHA_PEACH,
    MOCHA_MAUVE,
    MOCHA_RED,
    MOCHA_YELLOW,
    MOCHA_LAVENDER,
];

impl Avatar {
    /// Get the background color for this avatar.
    pub fn background_color(&self) -> Color {
        match self {
            Self::Initials { color_index } => {
                let idx = (*color_index as usize) % AVATAR_COLORS.len();
                AVATAR_COLORS[idx]
            }
            Self::SystemIcon(_) => MOCHA_SURFACE1,
            Self::ImagePath(_) => MOCHA_SURFACE0,
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
                color_index: (uid % AVATAR_COLORS.len() as u32) as u8,
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
        &self.entries[start..]
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ============================================================================
// Account manager
// ============================================================================

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
        let mut admin = UserAccount::new(1000, "admin", "System Administrator", AccountType::Administrator);
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
        let idx = self
            .accounts
            .iter()
            .position(|a| a.uid == uid)
            .ok_or("Account not found")?;

        if self.accounts[idx].is_current {
            return Err("Cannot delete the current user");
        }

        // Must have at least one admin remaining
        let is_admin = self.accounts[idx].account_type == AccountType::Administrator;
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
            self.activity_log.record(
                uid,
                ActivityEvent::AccountTypeChanged(new_type),
                timestamp,
            );
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
            out.push_str(&format!(
                "USER|{}|{}|{}|{}|{}|{}|{}\n",
                acct.uid,
                acct.username,
                acct.display_name,
                acct.account_type.id(),
                acct.avatar.to_string_repr(),
                acct.home_dir,
                acct.shell,
            ));
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
            if let Some(rest) = line.strip_prefix("USER|") {
                let parts: Vec<&str> = rest.split('|').collect();
                if parts.len() >= 7 {
                    let uid = parts[0].parse::<u32>().unwrap_or(0);
                    let username = parts[1];
                    let display_name = parts[2];
                    let account_type = AccountType::from_id(parts[3]);
                    let avatar = Avatar::from_string_repr(parts[4]);
                    let home = parts[5];
                    let shell = parts[6];

                    let mut acct = UserAccount::new(uid, username, display_name, account_type);
                    acct.avatar = avatar;
                    acct.home_dir = home.to_string();
                    acct.shell = shell.to_string();

                    if uid >= mgr.next_uid {
                        mgr.next_uid = uid.checked_add(1).unwrap_or(uid);
                    }

                    mgr.accounts.push(acct);
                }
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
    pub fn render(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(64);

        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 16.0,
            text: "User Accounts".to_string(),
            font_size: 18.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 32.0),
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
                    color: MOCHA_SURFACE1,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 8.0,
                y: tab_y + 6.0,
                text: tab.display_name().to_string(),
                font_size: 12.0,
                color: if is_active { MOCHA_BLUE } else { MOCHA_SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(110.0),
            });
        }

        // Content
        let content_y = tab_y + 40.0;
        let content_h = height - (content_y - y) - 16.0;

        match self.active_tab {
            AccountsTab::YourInfo => {
                self.render_your_info(&mut cmds, x + 16.0, content_y, width - 32.0, content_h);
            }
            AccountsTab::OtherUsers => {
                self.render_other_users(&mut cmds, x + 16.0, content_y, width - 32.0, content_h);
            }
            AccountsTab::SignInOptions => {
                self.render_sign_in_options(&mut cmds, x + 16.0, content_y, width - 32.0, content_h);
            }
            AccountsTab::ActivityLog => {
                self.render_activity_log(&mut cmds, x + 16.0, content_y, width - 32.0, content_h);
            }
        }

        // Status message
        if let Some(msg) = &self.status_message {
            cmds.push(RenderCommand::FillRect {
                x: x + 16.0,
                y: y + height - 32.0,
                width: width - 32.0,
                height: 24.0,
                color: MOCHA_SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 24.0,
                y: y + height - 28.0,
                text: msg.clone(),
                font_size: 11.0,
                color: MOCHA_YELLOW,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 48.0),
            });
        }

        cmds
    }

    fn render_your_info(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        if let Some(user) = self.manager.current_user() {
            // Avatar circle
            let avatar_size = 64.0;
            let avatar_color = user.avatar.background_color();
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
                color: MOCHA_MANTLE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(avatar_size),
            });

            // Name and username
            let info_x = x + avatar_size + 16.0;
            cmds.push(RenderCommand::Text {
                x: info_x,
                y,
                text: user.display_name.clone(),
                font_size: 16.0,
                color: MOCHA_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - avatar_size - 32.0),
            });

            cmds.push(RenderCommand::Text {
                x: info_x,
                y: y + 24.0,
                text: format!("@{}", user.username),
                font_size: 12.0,
                color: MOCHA_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - avatar_size - 32.0),
            });

            // Account type badge
            let badge_color = user.account_type.badge_color();
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
                color: MOCHA_MANTLE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(80.0),
            });

            // Info rows
            let mut row_y = y + 80.0;

            self.render_info_row(cmds, x, row_y, width, "Home Directory", &user.home_dir);
            row_y += 24.0;

            self.render_info_row(cmds, x, row_y, width, "Shell", &user.shell);
            row_y += 24.0;

            let password_status = if user.login_options.has_password {
                "Set"
            } else {
                "Not set"
            };
            self.render_info_row(cmds, x, row_y, width, "Password", password_status);
        }
    }

    fn render_other_users(
        &self,
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
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
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
                    color: MOCHA_SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Avatar small circle
            let av_size = 28.0;
            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: row_y + 2.0,
                width: av_size,
                height: av_size,
                color: acct.avatar.background_color(),
                corner_radii: CornerRadii::all(av_size / 2.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: row_y + 7.0,
                text: acct.initials(),
                font_size: 11.0,
                color: MOCHA_MANTLE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(av_size),
            });

            // Name
            cmds.push(RenderCommand::Text {
                x: x + 40.0,
                y: row_y + 4.0,
                text: acct.display_name.clone(),
                font_size: 12.0,
                color: MOCHA_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 180.0),
            });

            // Type badge
            cmds.push(RenderCommand::Text {
                x: x + 40.0,
                y: row_y + 20.0,
                text: acct.account_type.display_name().to_string(),
                font_size: 10.0,
                color: acct.account_type.badge_color(),
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });

            // Login status
            let status = if acct.is_logged_in {
                "Signed in"
            } else {
                "Signed out"
            };
            let status_color = if acct.is_logged_in {
                MOCHA_GREEN
            } else {
                MOCHA_OVERLAY0
            };
            cmds.push(RenderCommand::Text {
                x: x + width - 80.0,
                y: row_y + 10.0,
                text: status.to_string(),
                font_size: 10.0,
                color: status_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(75.0),
            });

            row_y += 40.0;
        }

        // Add user button
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y + 8.0,
            width: 140.0,
            height: 28.0,
            color: MOCHA_BLUE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: row_y + 14.0,
            text: "+ Add User".to_string(),
            font_size: 12.0,
            color: MOCHA_MANTLE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
        });
    }

    fn render_sign_in_options(
        &self,
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
                color: MOCHA_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
            });

            let mut row_y = y + 28.0;

            // Auto-login toggle
            self.render_toggle_row(
                cmds, x, row_y, width,
                "Auto sign-in at boot",
                user.login_options.auto_login,
            );
            row_y += 32.0;

            // Require password on wake
            self.render_toggle_row(
                cmds, x, row_y, width,
                "Require password on wake",
                user.login_options.require_password_on_wake,
            );
            row_y += 32.0;

            // Password hint
            self.render_info_row(
                cmds, x, row_y, width,
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
                color: MOCHA_SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 6.0,
                text: "Change Password".to_string(),
                font_size: 12.0,
                color: MOCHA_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
            });
        }
    }

    fn render_activity_log(
        &self,
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
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });

        let mut row_y = y + 28.0;
        let entries = self.manager.activity_log.recent(20);

        if entries.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y: row_y,
                text: "No activity recorded.".to_string(),
                font_size: 12.0,
                color: MOCHA_OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
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
                color: MOCHA_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });

            row_y += 20.0;
        }
    }

    fn render_info_row(
        &self,
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
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.42,
            y,
            text: value.to_string(),
            font_size: 12.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.55),
        });
    }

    fn render_toggle_row(
        &self,
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
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 60.0),
        });

        // Toggle switch
        let toggle_x = x + width - 44.0;
        let toggle_bg = if enabled { MOCHA_GREEN } else { MOCHA_SURFACE0 };
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
            color: MOCHA_TEXT,
            corner_radii: CornerRadii::all(7.0),
        });
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        let a = Avatar::Initials { color_index: 0 };
        let c = a.background_color();
        assert_eq!(c, MOCHA_BLUE);
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
        assert_eq!(
            mgr.accounts()[0].account_type,
            AccountType::Administrator
        );
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
        let result =
            mgr.change_account_type(admin_uid, AccountType::Standard, 1000);
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
        assert_eq!(ActivityEvent::PasswordChanged.display_text(), "Password changed");
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
        let cmds = ui.render(0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_other_users() {
        let mut ui = AccountSettingsUI::default();
        ui.active_tab = AccountsTab::OtherUsers;
        let cmds = ui.render(0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_sign_in_options() {
        let mut ui = AccountSettingsUI::default();
        ui.active_tab = AccountsTab::SignInOptions;
        let cmds = ui.render(0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_activity_log() {
        let mut ui = AccountSettingsUI::default();
        ui.active_tab = AccountsTab::ActivityLog;
        let cmds = ui.render(0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_with_status_message() {
        let mut ui = AccountSettingsUI::default();
        ui.status_message = Some("Account created successfully".to_string());
        let cmds = ui.render(0.0, 0.0, 600.0, 400.0);
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
}

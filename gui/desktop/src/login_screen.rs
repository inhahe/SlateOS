//! Login screen (greeter) for the desktop shell.
//!
//! Renders a full-screen login UI before the desktop session starts.
//! Features: user avatar list, password entry, autologin, login background
//! image (can match desktop wallpaper), keyboard layout indicator,
//! accessibility options, power options (shutdown/reboot/sleep), and
//! on-screen keyboard toggle.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Types
// ============================================================================

/// Login screen state machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginPhase {
    /// Showing the user selection list.
    UserSelect,
    /// Password entry for a selected user.
    PasswordEntry,
    /// Authentication in progress.
    Authenticating,
    /// Login succeeded, transitioning to desktop.
    LoggingIn,
    /// Login failed, showing error.
    Failed,
    /// Locked (screen lock, not initial login).
    Locked,
}

/// A user account entry for the login screen.
#[derive(Clone, Debug)]
pub struct LoginUser {
    /// User ID.
    pub uid: u32,
    /// Display name.
    pub display_name: String,
    /// Username (for authentication).
    pub username: String,
    /// Avatar icon character (placeholder for real avatar images).
    pub avatar: String,
    /// Whether autologin is enabled for this user.
    pub autologin: bool,
    /// Whether this user has a password set.
    pub has_password: bool,
    /// Whether this is the last logged-in user (shown first).
    pub last_login: bool,
    /// Account type label (e.g., "Administrator", "Standard").
    pub account_type: String,
}

impl LoginUser {
    pub fn new(uid: u32, username: &str, display_name: &str) -> Self {
        Self {
            uid,
            display_name: display_name.to_string(),
            username: username.to_string(),
            avatar: "\u{1F464}".to_string(),
            autologin: false,
            has_password: true,
            last_login: false,
            account_type: "Standard".to_string(),
        }
    }

    pub fn with_avatar(mut self, avatar: &str) -> Self {
        self.avatar = avatar.to_string();
        self
    }

    pub fn with_autologin(mut self) -> Self {
        self.autologin = true;
        self
    }

    pub fn with_admin(mut self) -> Self {
        self.account_type = "Administrator".to_string();
        self
    }

    pub fn with_last_login(mut self) -> Self {
        self.last_login = true;
        self
    }

    pub fn with_no_password(mut self) -> Self {
        self.has_password = false;
        self
    }
}

/// Background style for the login screen.
#[derive(Clone, Debug, PartialEq)]
pub enum LoginBackground {
    /// Solid color.
    SolidColor(Color),
    /// Same as desktop wallpaper (path to image).
    SameAsDesktop(String),
    /// Custom image path.
    CustomImage(String),
    /// Gradient between two colors.
    Gradient { top: Color, bottom: Color },
}

impl Default for LoginBackground {
    fn default() -> Self {
        Self::SolidColor(CRUST)
    }
}

/// Power action from the login screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginPowerAction {
    Shutdown,
    Reboot,
    Sleep,
    Hibernate,
}

/// Login configuration.
#[derive(Clone, Debug)]
pub struct LoginConfig {
    /// Background style.
    pub background: LoginBackground,
    /// Show clock on login screen.
    pub show_clock: bool,
    /// Show date.
    pub show_date: bool,
    /// Show keyboard layout indicator.
    pub show_keyboard_layout: bool,
    /// Show accessibility button.
    pub show_accessibility: bool,
    /// Show power button.
    pub show_power: bool,
    /// Show on-screen keyboard button.
    pub show_osk_button: bool,
    /// Maximum login attempts before lockout.
    pub max_attempts: u32,
    /// Lockout duration in seconds after max_attempts.
    pub lockout_seconds: u32,
    /// Whether password characters are shown briefly before being masked.
    pub show_password_hint: bool,
    /// Clock format (true = 24h, false = 12h).
    pub clock_24h: bool,
}

impl Default for LoginConfig {
    fn default() -> Self {
        Self {
            background: LoginBackground::default(),
            show_clock: true,
            show_date: true,
            show_keyboard_layout: true,
            show_accessibility: true,
            show_power: true,
            show_osk_button: true,
            max_attempts: 5,
            lockout_seconds: 30,
            show_password_hint: false,
            clock_24h: true,
        }
    }
}

// ============================================================================
// Login screen state
// ============================================================================

/// Full login screen state.
pub struct LoginScreen {
    /// Available users.
    pub users: Vec<LoginUser>,
    /// Currently selected user index.
    pub selected_user: usize,
    /// Current phase.
    pub phase: LoginPhase,
    /// Password input buffer.
    password_input: String,
    /// Whether to show the password text.
    pub show_password: bool,
    /// Error message after failed login.
    pub error_message: Option<String>,
    /// Failed attempt count for current user.
    pub failed_attempts: u32,
    /// Whether account is locked out.
    pub locked_out: bool,
    /// Lockout expiry timestamp (ms).
    pub lockout_until: u64,
    /// Power menu open.
    pub power_menu_open: bool,
    /// Accessibility menu open.
    pub a11y_menu_open: bool,
    /// Current keyboard layout name.
    pub keyboard_layout: String,
    /// Screen dimensions.
    pub screen_width: f32,
    pub screen_height: f32,
    /// Configuration.
    pub config: LoginConfig,
    /// Current time string (updated externally).
    pub time_string: String,
    /// Current date string.
    pub date_string: String,
    /// Shake animation offset (for failed login).
    pub shake_offset: f32,
    /// Shake animation timer.
    pub shake_timer: f32,
}

impl LoginScreen {
    pub fn new(screen_width: f32, screen_height: f32, users: Vec<LoginUser>) -> Self {
        // Default select last-login user or first.
        let selected = users
            .iter()
            .position(|u| u.last_login)
            .unwrap_or(0);

        Self {
            users,
            selected_user: selected,
            phase: LoginPhase::UserSelect,
            password_input: String::new(),
            show_password: false,
            error_message: None,
            failed_attempts: 0,
            locked_out: false,
            lockout_until: 0,
            power_menu_open: false,
            a11y_menu_open: false,
            keyboard_layout: "US".to_string(),
            screen_width,
            screen_height,
            config: LoginConfig::default(),
            time_string: "12:00".to_string(),
            date_string: "Sunday, May 18, 2026".to_string(),
            shake_offset: 0.0,
            shake_timer: 0.0,
        }
    }

    /// Select a user by index.
    pub fn select_user(&mut self, index: usize) {
        if index < self.users.len() {
            self.selected_user = index;
            self.password_input.clear();
            self.error_message = None;
            self.failed_attempts = 0;
            self.locked_out = false;

            // If user has no password, go to PasswordEntry phase (auto-submit).
            if !self.users[index].has_password {
                self.phase = LoginPhase::PasswordEntry;
            } else {
                self.phase = LoginPhase::PasswordEntry;
            }
        }
    }

    /// Go back to user select.
    pub fn back_to_user_select(&mut self) {
        self.phase = LoginPhase::UserSelect;
        self.password_input.clear();
        self.error_message = None;
        self.power_menu_open = false;
        self.a11y_menu_open = false;
    }

    /// Type a character into the password field.
    pub fn type_char(&mut self, c: char) {
        if self.phase != LoginPhase::PasswordEntry || self.locked_out {
            return;
        }
        self.password_input.push(c);
        self.error_message = None;
    }

    /// Backspace in password field.
    pub fn backspace(&mut self) {
        if self.phase != LoginPhase::PasswordEntry {
            return;
        }
        self.password_input.pop();
    }

    /// Clear password field.
    pub fn clear_password(&mut self) {
        self.password_input.clear();
    }

    /// Get the current password input (for authentication).
    pub fn password(&self) -> &str {
        &self.password_input
    }

    /// Password display (masked).
    pub fn password_display(&self) -> String {
        if self.show_password {
            self.password_input.clone()
        } else {
            "\u{2022}".repeat(self.password_input.len())
        }
    }

    /// Submit the password for authentication.
    /// Returns the username if the phase transitions to Authenticating.
    pub fn submit_password(&mut self) -> Option<String> {
        if self.phase != LoginPhase::PasswordEntry || self.locked_out {
            return None;
        }
        if self.selected_user >= self.users.len() {
            return None;
        }

        self.phase = LoginPhase::Authenticating;
        Some(self.users[self.selected_user].username.clone())
    }

    /// Called when authentication succeeds.
    pub fn auth_success(&mut self) {
        self.phase = LoginPhase::LoggingIn;
        self.error_message = None;
        self.failed_attempts = 0;
    }

    /// Called when authentication fails.
    pub fn auth_failure(&mut self, message: &str) {
        self.phase = LoginPhase::Failed;
        self.error_message = Some(message.to_string());
        self.failed_attempts += 1;
        self.shake_timer = 1.0; // start shake animation

        // Check lockout.
        if self.failed_attempts >= self.config.max_attempts {
            self.locked_out = true;
        }
    }

    /// Return to password entry from failed state.
    pub fn retry(&mut self) {
        if self.phase == LoginPhase::Failed {
            self.password_input.clear();
            self.phase = LoginPhase::PasswordEntry;
        }
    }

    /// Check lockout (call each tick with current time).
    pub fn check_lockout(&mut self, now_ms: u64) {
        if self.locked_out && now_ms >= self.lockout_until {
            self.locked_out = false;
            self.failed_attempts = 0;
            self.phase = LoginPhase::PasswordEntry;
        }
    }

    /// Set lockout expiry (called after auth_failure triggers lockout).
    pub fn set_lockout_expiry(&mut self, now_ms: u64) {
        if self.locked_out {
            self.lockout_until = now_ms + self.config.lockout_seconds as u64 * 1000;
        }
    }

    /// Toggle power menu.
    pub fn toggle_power_menu(&mut self) {
        self.power_menu_open = !self.power_menu_open;
        self.a11y_menu_open = false;
    }

    /// Toggle accessibility menu.
    pub fn toggle_a11y_menu(&mut self) {
        self.a11y_menu_open = !self.a11y_menu_open;
        self.power_menu_open = false;
    }

    /// Currently selected user (if any).
    pub fn current_user(&self) -> Option<&LoginUser> {
        self.users.get(self.selected_user)
    }

    /// Check if any user has autologin enabled.
    pub fn autologin_user(&self) -> Option<&LoginUser> {
        self.users.iter().find(|u| u.autologin)
    }

    /// Tick animation (shake effect).
    pub fn tick_animation(&mut self, dt: f32) {
        if self.shake_timer > 0.0 {
            self.shake_timer = (self.shake_timer - dt).max(0.0);
            // Sine-wave shake.
            self.shake_offset = (self.shake_timer * 30.0).sin() * 8.0 * self.shake_timer;
        } else {
            self.shake_offset = 0.0;
        }
    }

    /// Render the full login screen.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut commands = Vec::new();

        // Background.
        self.render_background(&mut commands);

        // Clock and date (top center).
        if self.config.show_clock {
            self.render_clock(&mut commands);
        }

        // Main content (user list or password entry).
        match self.phase {
            LoginPhase::UserSelect => self.render_user_select(&mut commands),
            LoginPhase::PasswordEntry | LoginPhase::Failed => {
                self.render_password_entry(&mut commands);
            }
            LoginPhase::Authenticating => self.render_authenticating(&mut commands),
            LoginPhase::LoggingIn => self.render_logging_in(&mut commands),
            LoginPhase::Locked => self.render_password_entry(&mut commands),
        }

        // Bottom bar (power, accessibility, keyboard layout).
        self.render_bottom_bar(&mut commands);

        // Power menu overlay.
        if self.power_menu_open {
            self.render_power_menu(&mut commands);
        }

        commands
    }

    // ========================================================================
    // Render helpers
    // ========================================================================

    fn render_background(&self, commands: &mut Vec<RenderCommand>) {
        match &self.config.background {
            LoginBackground::SolidColor(color) => {
                commands.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: 0.0,
                    width: self.screen_width,
                    height: self.screen_height,
                    color: *color,
                    corner_radii: CornerRadii::ZERO,
                });
            }
            LoginBackground::Gradient { top, bottom } => {
                // Approximate gradient with horizontal bands.
                let bands = 20;
                let band_h = self.screen_height / bands as f32;
                for i in 0..bands {
                    let t = i as f32 / (bands - 1) as f32;
                    let r = (top.r as f32 * (1.0 - t) + bottom.r as f32 * t) as u8;
                    let g = (top.g as f32 * (1.0 - t) + bottom.g as f32 * t) as u8;
                    let b = (top.b as f32 * (1.0 - t) + bottom.b as f32 * t) as u8;
                    commands.push(RenderCommand::FillRect {
                        x: 0.0,
                        y: i as f32 * band_h,
                        width: self.screen_width,
                        height: band_h + 1.0,
                        color: Color::rgb(r, g, b),
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
            _ => {
                // For image backgrounds, render placeholder dark.
                commands.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: 0.0,
                    width: self.screen_width,
                    height: self.screen_height,
                    color: CRUST,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
    }

    fn render_clock(&self, commands: &mut Vec<RenderCommand>) {
        let cx = self.screen_width / 2.0;
        commands.push(RenderCommand::Text {
            x: cx - 80.0,
            y: 80.0,
            text: self.time_string.clone(),
            font_size: 64.0,
            color: TEXT,
            font_weight: FontWeightHint::Light,
            max_width: None,
        });
        if self.config.show_date {
            commands.push(RenderCommand::Text {
                x: cx - 100.0,
                y: 155.0,
                text: self.date_string.clone(),
                font_size: 16.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_user_select(&self, commands: &mut Vec<RenderCommand>) {
        let cx = self.screen_width / 2.0;
        let start_y = self.screen_height / 2.0 - (self.users.len() as f32 * 40.0);

        for (i, user) in self.users.iter().enumerate() {
            let uy = start_y + i as f32 * 80.0;
            let selected = i == self.selected_user;

            // User row.
            let row_w = 280.0;
            let row_x = cx - row_w / 2.0;
            commands.push(RenderCommand::FillRect {
                x: row_x,
                y: uy,
                width: row_w,
                height: 64.0,
                color: if selected { SURFACE0 } else { Color::rgba(BASE.r, BASE.g, BASE.b, 180) },
                corner_radii: CornerRadii::all(12.0),
            });

            // Avatar.
            commands.push(RenderCommand::Text {
                x: row_x + 12.0,
                y: uy + 14.0,
                text: user.avatar.clone(),
                font_size: 28.0,
                color: if selected { BLUE } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Name.
            commands.push(RenderCommand::Text {
                x: row_x + 52.0,
                y: uy + 12.0,
                text: user.display_name.clone(),
                font_size: 16.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Account type.
            commands.push(RenderCommand::Text {
                x: row_x + 52.0,
                y: uy + 36.0,
                text: user.account_type.clone(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_password_entry(&self, commands: &mut Vec<RenderCommand>) {
        let cx = self.screen_width / 2.0 + self.shake_offset;
        let cy = self.screen_height / 2.0;

        if let Some(user) = self.current_user() {
            // Avatar (large).
            commands.push(RenderCommand::Text {
                x: cx - 24.0,
                y: cy - 80.0,
                text: user.avatar.clone(),
                font_size: 48.0,
                color: BLUE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Name.
            commands.push(RenderCommand::Text {
                x: cx - 60.0,
                y: cy - 20.0,
                text: user.display_name.clone(),
                font_size: 18.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Password field.
            let field_w = 260.0;
            let field_h = 36.0;
            let field_x = cx - field_w / 2.0;
            let field_y = cy + 20.0;

            let border_color = if self.error_message.is_some() { RED } else { SURFACE1 };
            commands.push(RenderCommand::FillRect {
                x: field_x,
                y: field_y,
                width: field_w,
                height: field_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            commands.push(RenderCommand::StrokeRect {
                x: field_x,
                y: field_y,
                width: field_w,
                height: field_h,
                color: border_color,
                line_width: 1.5,
                corner_radii: CornerRadii::all(8.0),
            });

            // Password text or placeholder.
            let display = if self.password_input.is_empty() {
                "Password".to_string()
            } else {
                self.password_display()
            };
            commands.push(RenderCommand::Text {
                x: field_x + 12.0,
                y: field_y + 9.0,
                text: display,
                font_size: 14.0,
                color: if self.password_input.is_empty() { OVERLAY0 } else { TEXT },
                font_weight: FontWeightHint::Regular,
                max_width: Some(field_w - 24.0),
            });

            // Show/hide toggle.
            commands.push(RenderCommand::Text {
                x: field_x + field_w - 28.0,
                y: field_y + 8.0,
                text: if self.show_password { "\u{1F441}" } else { "\u{1F576}" }.to_string(),
                font_size: 14.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Submit button.
            let btn_y = field_y + field_h + 12.0;
            commands.push(RenderCommand::FillRect {
                x: cx - 50.0,
                y: btn_y,
                width: 100.0,
                height: 32.0,
                color: BLUE,
                corner_radii: CornerRadii::all(8.0),
            });
            commands.push(RenderCommand::Text {
                x: cx - 24.0,
                y: btn_y + 8.0,
                text: "Sign In".to_string(),
                font_size: 13.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Error message.
            if let Some(msg) = &self.error_message {
                commands.push(RenderCommand::Text {
                    x: cx - 100.0,
                    y: btn_y + 48.0,
                    text: msg.clone(),
                    font_size: 12.0,
                    color: RED,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                });
            }

            // Lockout message.
            if self.locked_out {
                commands.push(RenderCommand::Text {
                    x: cx - 120.0,
                    y: btn_y + 70.0,
                    text: format!("Too many attempts. Try again in {}s.", self.config.lockout_seconds),
                    font_size: 12.0,
                    color: YELLOW,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(240.0),
                });
            }

            // Back button.
            if self.users.len() > 1 {
                commands.push(RenderCommand::Text {
                    x: cx - 120.0,
                    y: cy - 80.0,
                    text: "\u{2190}".to_string(),
                    font_size: 20.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }
    }

    fn render_authenticating(&self, commands: &mut Vec<RenderCommand>) {
        let cx = self.screen_width / 2.0;
        let cy = self.screen_height / 2.0;

        commands.push(RenderCommand::Text {
            x: cx - 60.0,
            y: cy,
            text: "Signing in...".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_logging_in(&self, commands: &mut Vec<RenderCommand>) {
        let cx = self.screen_width / 2.0;
        let cy = self.screen_height / 2.0;

        if let Some(user) = self.current_user() {
            commands.push(RenderCommand::Text {
                x: cx - 80.0,
                y: cy,
                text: format!("Welcome, {}!", user.display_name),
                font_size: 20.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_bottom_bar(&self, commands: &mut Vec<RenderCommand>) {
        let bar_y = self.screen_height - 40.0;
        let bar_h = 40.0;

        // Semi-transparent bar.
        commands.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: self.screen_width,
            height: bar_h,
            color: Color::rgba(CRUST.r, CRUST.g, CRUST.b, 180),
            corner_radii: CornerRadii::ZERO,
        });

        // Keyboard layout (left).
        if self.config.show_keyboard_layout {
            commands.push(RenderCommand::Text {
                x: 16.0,
                y: bar_y + 12.0,
                text: self.keyboard_layout.clone(),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Power button (right).
        if self.config.show_power {
            commands.push(RenderCommand::Text {
                x: self.screen_width - 40.0,
                y: bar_y + 10.0,
                text: "\u{23FB}".to_string(),
                font_size: 16.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Accessibility (before power).
        if self.config.show_accessibility {
            commands.push(RenderCommand::Text {
                x: self.screen_width - 80.0,
                y: bar_y + 10.0,
                text: "\u{267F}".to_string(),
                font_size: 16.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // On-screen keyboard.
        if self.config.show_osk_button {
            commands.push(RenderCommand::Text {
                x: self.screen_width - 120.0,
                y: bar_y + 10.0,
                text: "\u{2328}".to_string(),
                font_size: 16.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_power_menu(&self, commands: &mut Vec<RenderCommand>) {
        let menu_w = 160.0;
        let menu_h = 140.0;
        let mx = self.screen_width - menu_w - 16.0;
        let my = self.screen_height - 40.0 - menu_h - 8.0;

        commands.push(RenderCommand::FillRect {
            x: mx,
            y: my,
            width: menu_w,
            height: menu_h,
            color: MANTLE,
            corner_radii: CornerRadii::all(8.0),
        });
        commands.push(RenderCommand::StrokeRect {
            x: mx,
            y: my,
            width: menu_w,
            height: menu_h,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        let options = [
            ("\u{23FB}", "Shut Down"),
            ("\u{1F504}", "Restart"),
            ("\u{1F4A4}", "Sleep"),
            ("\u{1F4BE}", "Hibernate"),
        ];

        for (i, (icon, label)) in options.iter().enumerate() {
            let iy = my + 8.0 + i as f32 * 32.0;
            commands.push(RenderCommand::Text {
                x: mx + 12.0,
                y: iy + 6.0,
                text: icon.to_string(),
                font_size: 14.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            commands.push(RenderCommand::Text {
                x: mx + 36.0,
                y: iy + 7.0,
                text: label.to_string(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_users() -> Vec<LoginUser> {
        vec![
            LoginUser::new(1000, "alice", "Alice")
                .with_admin()
                .with_last_login(),
            LoginUser::new(1001, "bob", "Bob"),
        ]
    }

    fn make_screen() -> LoginScreen {
        LoginScreen::new(1920.0, 1080.0, make_users())
    }

    #[test]
    fn initial_state() {
        let s = make_screen();
        assert_eq!(s.phase, LoginPhase::UserSelect);
        assert_eq!(s.selected_user, 0); // alice has last_login
        assert_eq!(s.users.len(), 2);
    }

    #[test]
    fn select_user() {
        let mut s = make_screen();
        s.select_user(1);
        assert_eq!(s.selected_user, 1);
        assert_eq!(s.phase, LoginPhase::PasswordEntry);
    }

    #[test]
    fn type_password() {
        let mut s = make_screen();
        s.select_user(0);
        s.type_char('h');
        s.type_char('i');
        assert_eq!(s.password(), "hi");
    }

    #[test]
    fn backspace() {
        let mut s = make_screen();
        s.select_user(0);
        s.type_char('a');
        s.type_char('b');
        s.backspace();
        assert_eq!(s.password(), "a");
    }

    #[test]
    fn clear_password() {
        let mut s = make_screen();
        s.select_user(0);
        s.type_char('x');
        s.clear_password();
        assert_eq!(s.password(), "");
    }

    #[test]
    fn password_display_masked() {
        let mut s = make_screen();
        s.select_user(0);
        s.type_char('a');
        s.type_char('b');
        s.type_char('c');
        assert_eq!(s.password_display(), "\u{2022}\u{2022}\u{2022}");
    }

    #[test]
    fn password_display_visible() {
        let mut s = make_screen();
        s.select_user(0);
        s.show_password = true;
        s.type_char('a');
        s.type_char('b');
        assert_eq!(s.password_display(), "ab");
    }

    #[test]
    fn submit_password() {
        let mut s = make_screen();
        s.select_user(0);
        s.type_char('p');
        let username = s.submit_password();
        assert_eq!(username, Some("alice".to_string()));
        assert_eq!(s.phase, LoginPhase::Authenticating);
    }

    #[test]
    fn auth_success() {
        let mut s = make_screen();
        s.select_user(0);
        s.submit_password();
        s.auth_success();
        assert_eq!(s.phase, LoginPhase::LoggingIn);
    }

    #[test]
    fn auth_failure() {
        let mut s = make_screen();
        s.select_user(0);
        s.submit_password();
        s.auth_failure("Wrong password");
        assert_eq!(s.phase, LoginPhase::Failed);
        assert_eq!(s.error_message.as_deref(), Some("Wrong password"));
        assert_eq!(s.failed_attempts, 1);
    }

    #[test]
    fn lockout_after_max_attempts() {
        let mut s = make_screen();
        s.config.max_attempts = 3;
        s.select_user(0);
        for _ in 0..3 {
            s.phase = LoginPhase::PasswordEntry;
            s.submit_password();
            s.auth_failure("Wrong");
        }
        assert!(s.locked_out);
    }

    #[test]
    fn lockout_expires() {
        let mut s = make_screen();
        s.config.max_attempts = 1;
        s.select_user(0);
        s.submit_password();
        s.auth_failure("Wrong");
        s.set_lockout_expiry(1000);
        assert!(s.locked_out);
        s.check_lockout(1000 + 31000); // 30s lockout
        assert!(!s.locked_out);
    }

    #[test]
    fn retry_after_failure() {
        let mut s = make_screen();
        s.select_user(0);
        s.submit_password();
        s.auth_failure("Wrong");
        s.retry();
        assert_eq!(s.phase, LoginPhase::PasswordEntry);
        assert!(s.password().is_empty());
    }

    #[test]
    fn back_to_user_select() {
        let mut s = make_screen();
        s.select_user(0);
        s.back_to_user_select();
        assert_eq!(s.phase, LoginPhase::UserSelect);
    }

    #[test]
    fn toggle_power_menu() {
        let mut s = make_screen();
        s.toggle_power_menu();
        assert!(s.power_menu_open);
        s.toggle_power_menu();
        assert!(!s.power_menu_open);
    }

    #[test]
    fn toggle_a11y_menu_closes_power() {
        let mut s = make_screen();
        s.power_menu_open = true;
        s.toggle_a11y_menu();
        assert!(s.a11y_menu_open);
        assert!(!s.power_menu_open);
    }

    #[test]
    fn current_user() {
        let s = make_screen();
        let u = s.current_user().unwrap();
        assert_eq!(u.username, "alice");
    }

    #[test]
    fn autologin_user() {
        let users = vec![
            LoginUser::new(1, "a", "A"),
            LoginUser::new(2, "b", "B").with_autologin(),
        ];
        let s = LoginScreen::new(1920.0, 1080.0, users);
        let auto = s.autologin_user().unwrap();
        assert_eq!(auto.username, "b");
    }

    #[test]
    fn no_autologin() {
        let s = make_screen();
        assert!(s.autologin_user().is_none());
    }

    #[test]
    fn shake_animation() {
        let mut s = make_screen();
        s.shake_timer = 1.0;
        s.tick_animation(0.5);
        assert!(s.shake_timer > 0.0);
        assert!(s.shake_offset.abs() > 0.0);
        s.tick_animation(0.6);
        assert!((s.shake_timer - 0.0).abs() < 0.01);
    }

    #[test]
    fn typing_while_locked_ignored() {
        let mut s = make_screen();
        s.select_user(0);
        s.locked_out = true;
        s.type_char('x');
        assert!(s.password().is_empty());
    }

    #[test]
    fn submit_while_locked_ignored() {
        let mut s = make_screen();
        s.select_user(0);
        s.locked_out = true;
        assert!(s.submit_password().is_none());
    }

    // ---- LoginUser ----

    #[test]
    fn login_user_builder() {
        let u = LoginUser::new(1, "test", "Test User")
            .with_avatar("\u{1F468}")
            .with_admin()
            .with_autologin()
            .with_no_password();
        assert_eq!(u.avatar, "\u{1F468}");
        assert_eq!(u.account_type, "Administrator");
        assert!(u.autologin);
        assert!(!u.has_password);
    }

    // ---- LoginBackground ----

    #[test]
    fn default_background() {
        let bg = LoginBackground::default();
        assert!(matches!(bg, LoginBackground::SolidColor(_)));
    }

    // ---- Rendering ----

    #[test]
    fn render_user_select() {
        let s = make_screen();
        let cmds = s.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_password_entry() {
        let mut s = make_screen();
        s.select_user(0);
        let cmds = s.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_with_error() {
        let mut s = make_screen();
        s.select_user(0);
        s.phase = LoginPhase::Failed;
        s.error_message = Some("Bad password".to_string());
        let cmds = s.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_authenticating() {
        let mut s = make_screen();
        s.phase = LoginPhase::Authenticating;
        let cmds = s.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_logging_in() {
        let mut s = make_screen();
        s.phase = LoginPhase::LoggingIn;
        let cmds = s.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_power_menu() {
        let mut s = make_screen();
        s.power_menu_open = true;
        let cmds = s.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_gradient_background() {
        let mut s = make_screen();
        s.config.background = LoginBackground::Gradient {
            top: Color::from_hex(0x1E1E2E),
            bottom: Color::from_hex(0x11111B),
        };
        let cmds = s.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_lockout() {
        let mut s = make_screen();
        s.select_user(0);
        s.locked_out = true;
        let cmds = s.render();
        assert!(!cmds.is_empty());
    }
}

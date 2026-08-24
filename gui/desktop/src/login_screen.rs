//! Login screen (greeter) for the desktop shell.
//!
//! Renders a full-screen login UI before the desktop session starts.
//! Features: user avatar list, password entry, autologin, login background
//! image (can match desktop wallpaper), keyboard layout indicator,
//! accessibility options, power options (shutdown/reboot/sleep), and
//! on-screen keyboard toggle.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// ============================================================================
// Colour
// ============================================================================
//
// This module used to keep eleven `const … : Color` of its own, hard-coded to
// Catppuccin Mocha, so the greeter stayed dark for a user who had chosen the
// light theme — and stayed blue for a user who had chosen any other accent.
// Every colour now comes from the `Palette` the caller resolved. Four
// judgements are worth stating, because most of them make this file *look*
// like it is ignoring the palette when it is doing the opposite:
//
// 1. *Most of what this screen draws sits on a background the shell did not
//    choose.* The login background is a solid colour, a gradient or a
//    photograph, picked by the user or copied from their wallpaper — so the
//    clock, the date, the greeting and the failure notices land on a surface
//    of unknown brightness. Text there takes [`Palette::on_wallpaper`] with a
//    [`Palette::text_shadow`] pass under it, exactly as a desktop icon's label
//    does (see `icons.rs`), and deliberately *not* `p.text`: Latte's `text` is
//    nearly black and would vanish on a dark photograph. Text this module
//    mounts on a panel it drew itself — a user row, the password field, the
//    bottom bar, the power menu — keeps ordinary roles, because there the
//    module does know what is behind it. [`push_on_background`] is the
//    boundary between the two cases.
//
// 2. *The Sign In button is the screen's default action, so it carries the
//    accent* — it is what Enter does, which is an invitation, and an
//    invitation is what the accent is for. Its label is
//    [`Palette::on_accent`], derived from whatever accent the user set, never
//    a named role. The selected user's avatar carries the accent for the
//    neighbouring reason: it marks *which* row you are on, and position is the
//    accent's other job.
//
// 3. *A refusal is not decoration.* The error message stays `p.red` and the
//    lockout notice `p.yellow` on every machine, whatever the accent is — a
//    user who picked a red accent must still be able to tell "wrong password"
//    from "press me". Both are drawn through `push_on_background`, so freezing
//    the hue costs nothing in legibility.
//
// 4. *`LoginBackground::default()` cannot name a colour, so it stops trying.*
//    `Default::default` takes no arguments and therefore no palette, which is
//    exactly how `CRUST` came to be hard-coded here. The default is now
//    [`LoginBackground::Theme`], a variant that names no colour at all and is
//    resolved at render time to `p.crust`. A user-picked `SolidColor` or
//    `Gradient` is still drawn verbatim: that is the user's colour, not the
//    theme's, and re-theming it would be overriding a choice they made.
//
// One thing deliberately *not* changed: the two translucent surfaces here (an
// unselected user row, the bottom bar) keep their literal alpha of 180 rather
// than reading `p.panel_alpha`. Transparency is applied centrally in
// `DesktopTheme::from_settings`, not per module, and making this one file the
// exception would leave the bar honouring the setting while the menu on top of
// it did not.

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
#[derive(Clone, Debug, Default, PartialEq)]
pub enum LoginBackground {
    /// Whatever the theme's deepest surface is — resolved at render time.
    ///
    /// The default, and the only variant that names no colour. A variant was
    /// needed because [`Default::default`] takes no arguments and so cannot be
    /// handed a [`Palette`]: the previous default was `SolidColor(CRUST)` with
    /// `CRUST` a Mocha literal in this file, which is why the greeter stayed
    /// black for a user who had asked for the light theme. Deferring the
    /// question to `render` is what makes the answer follow the mode.
    #[default]
    Theme,
    /// Solid color, chosen by the user.
    ///
    /// Drawn exactly as given. This is not a theme role and is not re-themed:
    /// a user who picked a colour picked *that* colour.
    SolidColor(Color),
    /// Same as desktop wallpaper (path to image).
    SameAsDesktop(String),
    /// Custom image path.
    CustomImage(String),
    /// Gradient between two colors, chosen by the user. Drawn as given.
    Gradient { top: Color, bottom: Color },
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
// Background gradient
// ============================================================================

/// How many horizontal bands approximate a gradient background.
const GRADIENT_BANDS: usize = 20;

/// One channel of a gradient band, `t` of the way from `a` to `b`.
///
/// Rounds rather than truncates. Truncating looks like a rounding nicety and is
/// not: `a as f32 * (1 - t) + b as f32 * t` is only *exactly* `a` when `a == b`
/// in real arithmetic, and in `f32` it lands a hair below on most values of
/// `t`. Truncation then turns that hair into a whole step, so a "gradient"
/// between one colour and itself came out banded — twenty stripes alternating
/// between the colour the user chose and one darker. The same error biases
/// every real gradient a step dark along its whole length.
fn lerp_channel(a: u8, b: u8, t: f32) -> u8 {
    let v = f32::from(a).mul_add(1.0 - t, f32::from(b) * t);
    // Clamped before the cast because a saturating float-to-int cast is a
    // silent behaviour, and `t` outside 0..=1 would be a caller bug worth
    // pinning to the endpoints rather than wrapping around.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 on the line above, so the cast is exact"
    )]
    {
        v.round().clamp(0.0, 255.0) as u8
    }
}

// ============================================================================
// Text that lands on the login background
// ============================================================================

/// Push `text` twice: once as a hard shadow, once as itself.
///
/// The boundary judgement 1 describes, made mechanical. Anything drawn on the
/// login *background* goes through here; anything drawn on a panel this module
/// filled itself is pushed directly. The background is a colour, a gradient or
/// a photograph the shell did not choose, so no single ink is legible on all of
/// them — the shadow supplies the contrast the palette cannot, which is
/// precisely what [`Palette::text_shadow`] exists for. `icons.rs` solves the
/// identical problem for desktop icon labels the identical way.
///
/// Taking a whole [`RenderCommand`] rather than nine arguments keeps the call
/// sites ordinary struct literals, so a reader can see the text's real colour
/// where it is written instead of at the end of a long argument list. A
/// non-`Text` command is passed through untouched: only text needs a shadow,
/// and only text is ever handed to this function.
fn push_on_background(commands: &mut Vec<RenderCommand>, p: &Palette, text: RenderCommand) {
    let RenderCommand::Text {
        x,
        y,
        text: body,
        font_size,
        font_weight,
        max_width,
        overflow,
        ..
    } = &text
    else {
        commands.push(text);
        return;
    };
    commands.push(RenderCommand::Text {
        x: x + 1.0,
        y: y + 1.0,
        text: body.clone(),
        font_size: *font_size,
        color: p.text_shadow(),
        font_weight: *font_weight,
        max_width: *max_width,
        overflow: *overflow,
    });
    commands.push(text);
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
        let selected = users.iter().position(|u| u.last_login).unwrap_or(0);

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

            // Always advance to PasswordEntry: users without passwords still
            // pass through this phase so they can auto-submit an empty entry.
            self.phase = LoginPhase::PasswordEntry;
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
    ///
    /// One bullet per *character*, not per byte. `type_char` pushes a `char`
    /// and `backspace` pops one, so counting bytes made a password containing
    /// any non-ASCII character draw more bullets than the user had pressed
    /// keys — two for `é`, three for `€`, four for an emoji — and made one
    /// backspace erase several of them. That is worse than untidy: the bullets
    /// are the only feedback that a keystroke registered at all, so a user
    /// whose password is not pure ASCII could not tell a doubled character
    /// from a correct one. It also published the password's UTF-8 byte length
    /// to anyone watching the screen, which distinguishes `é` from `e`.
    pub fn password_display(&self) -> String {
        if self.show_password {
            self.password_input.clone()
        } else {
            "\u{2022}".repeat(self.password_input.chars().count())
        }
    }

    /// Submit the password for authentication.
    /// Returns the username if the phase transitions to Authenticating.
    pub fn submit_password(&mut self) -> Option<String> {
        if self.phase != LoginPhase::PasswordEntry || self.locked_out {
            return None;
        }
        // Take the name through the accessor that already resolves it, and
        // take it *before* advancing the phase. The bounds proof used to sit
        // three statements above the indexing it justified, and the phase
        // change sat between them — so any later edit that reordered those two
        // lines would have left the login screen in `Authenticating` with
        // nobody to authenticate, or panicked outright. A panic here locks the
        // user out of the machine, which is the worst place in the shell to
        // leave a proof that can drift from its use.
        let username = self.current_user()?.username.clone();
        self.phase = LoginPhase::Authenticating;
        Some(username)
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
        self.failed_attempts = self.failed_attempts.saturating_add(1);
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
    ///
    /// Saturating, and saturating in the safe direction: an expiry that
    /// cannot be represented becomes `u64::MAX`, which is a lockout that
    /// does not end. A wrapping one would land *behind* `now_ms` and clear
    /// the lockout on the next tick, which is the failure that matters here.
    pub fn set_lockout_expiry(&mut self, now_ms: u64) {
        if self.locked_out {
            self.lockout_until =
                now_ms.saturating_add(u64::from(self.config.lockout_seconds).saturating_mul(1000));
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

    /// Render the full login screen with the palette the caller resolved.
    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {
        let mut commands = Vec::new();

        // Background.
        self.render_background(p, &mut commands);

        // Clock and date (top center).
        if self.config.show_clock {
            self.render_clock(p, &mut commands);
        }

        // Main content (user list or password entry).
        match self.phase {
            LoginPhase::UserSelect => self.render_user_select(p, &mut commands),
            LoginPhase::PasswordEntry | LoginPhase::Failed => {
                self.render_password_entry(p, &mut commands);
            }
            LoginPhase::Authenticating => self.render_authenticating(p, &mut commands),
            LoginPhase::LoggingIn => self.render_logging_in(p, &mut commands),
            LoginPhase::Locked => self.render_password_entry(p, &mut commands),
        }

        // Bottom bar (power, accessibility, keyboard layout).
        self.render_bottom_bar(p, &mut commands);

        // Power menu overlay.
        if self.power_menu_open {
            self.render_power_menu(p, &mut commands);
        }

        commands
    }

    // ========================================================================
    // Render helpers
    // ========================================================================

    fn render_background(&self, p: &Palette, commands: &mut Vec<RenderCommand>) {
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
                let bands = GRADIENT_BANDS;
                let band_h = self.screen_height / bands as f32;
                for i in 0..bands {
                    let t = i as f32 / (bands - 1) as f32;
                    let r = lerp_channel(top.r, bottom.r, t);
                    let g = lerp_channel(top.g, bottom.g, t);
                    let b = lerp_channel(top.b, bottom.b, t);
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
            // Named rather than `_` so that adding a variant is a compile
            // error here instead of a silent dark rectangle. `Theme` *is* this
            // fill; the two image variants land on it as a placeholder until
            // the greeter can load an image, and the theme's deepest surface is
            // a better placeholder than a fixed near-black — that fixed value
            // is what made a light session flash dark before the image arrived.
            LoginBackground::Theme
            | LoginBackground::SameAsDesktop(_)
            | LoginBackground::CustomImage(_) => {
                commands.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: 0.0,
                    width: self.screen_width,
                    height: self.screen_height,
                    color: p.crust,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
    }

    fn render_clock(&self, p: &Palette, commands: &mut Vec<RenderCommand>) {
        let cx = self.screen_width / 2.0;
        push_on_background(
            commands,
            p,
            RenderCommand::Text {
                x: cx - 80.0,
                y: 80.0,
                text: self.time_string.clone(),
                font_size: 64.0,
                color: p.on_wallpaper(),
                font_weight: FontWeightHint::Light,
                max_width: None,
                overflow: TextOverflow::Clip,
            },
        );
        if self.config.show_date {
            push_on_background(
                commands,
                p,
                RenderCommand::Text {
                    x: cx - 100.0,
                    y: 155.0,
                    text: self.date_string.clone(),
                    font_size: 16.0,
                    // Dim rather than dark: the date is subordinate to the
                    // time, and on an unknown background the only safe way to
                    // say "subordinate" is alpha. See judgement 1.
                    color: p.on_wallpaper_dim(),
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                },
            );
        }
    }

    fn render_user_select(&self, p: &Palette, commands: &mut Vec<RenderCommand>) {
        let cx = self.screen_width / 2.0;
        let start_y = self.screen_height / 2.0 - (self.users.len() as f32 * 40.0);

        for (i, user) in self.users.iter().enumerate() {
            let uy = start_y + i as f32 * 80.0;
            let selected = i == self.selected_user;

            // User row. A panel this module draws itself, so everything mounted
            // on it below uses ordinary roles rather than the wallpaper pair.
            let row_w = 280.0;
            let row_x = cx - row_w / 2.0;
            commands.push(RenderCommand::FillRect {
                x: row_x,
                y: uy,
                width: row_w,
                height: 64.0,
                color: if selected {
                    p.surface0
                } else {
                    Color::rgba(p.base.r, p.base.g, p.base.b, 180)
                },
                corner_radii: CornerRadii::all(12.0),
            });

            // Avatar. The accent marks which row you are on — judgement 2.
            commands.push(RenderCommand::Text {
                x: row_x + 12.0,
                y: uy + 14.0,
                text: user.avatar.clone(),
                font_size: 28.0,
                color: if selected { p.accent } else { p.subtext0 },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Name.
            commands.push(RenderCommand::Text {
                x: row_x + 52.0,
                y: uy + 12.0,
                text: user.display_name.clone(),
                font_size: 16.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Account type.
            commands.push(RenderCommand::Text {
                x: row_x + 52.0,
                y: uy + 36.0,
                text: user.account_type.clone(),
                font_size: 11.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn render_password_entry(&self, p: &Palette, commands: &mut Vec<RenderCommand>) {
        let cx = self.screen_width / 2.0 + self.shake_offset;
        let cy = self.screen_height / 2.0;

        if let Some(user) = self.current_user() {
            // Avatar (large). This is the user you are signing in as — the
            // selection carried forward from the row list — so it keeps the
            // accent it had there. It floats on the background, so it also
            // gets the shadow pass.
            push_on_background(
                commands,
                p,
                RenderCommand::Text {
                    x: cx - 24.0,
                    y: cy - 80.0,
                    text: user.avatar.clone(),
                    font_size: 48.0,
                    color: p.accent,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                },
            );

            // Name.
            push_on_background(
                commands,
                p,
                RenderCommand::Text {
                    x: cx - 60.0,
                    y: cy - 20.0,
                    text: user.display_name.clone(),
                    font_size: 18.0,
                    color: p.on_wallpaper(),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                },
            );

            // Password field. A panel, so its contents take ordinary roles.
            let field_w = 260.0;
            let field_h = 36.0;
            let field_x = cx - field_w / 2.0;
            let field_y = cy + 20.0;

            // Judgement 3: a rejected password must read as a rejection under
            // every accent, so this is `p.red` and never `p.accent`.
            let border_color = if self.error_message.is_some() {
                p.red
            } else {
                p.surface1
            };
            commands.push(RenderCommand::FillRect {
                x: field_x,
                y: field_y,
                width: field_w,
                height: field_h,
                color: p.surface0,
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
                color: if self.password_input.is_empty() {
                    p.overlay0
                } else {
                    p.text
                },
                font_weight: FontWeightHint::Regular,
                max_width: Some(field_w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Show/hide toggle.
            commands.push(RenderCommand::Text {
                x: field_x + field_w - 28.0,
                y: field_y + 8.0,
                text: if self.show_password {
                    "\u{1F441}"
                } else {
                    "\u{1F576}"
                }
                .to_string(),
                font_size: 14.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Submit button — the screen's default action, so it takes the
            // accent, and its label is derived from that accent rather than
            // named. See judgement 2.
            let btn_y = field_y + field_h + 12.0;
            commands.push(RenderCommand::FillRect {
                x: cx - 50.0,
                y: btn_y,
                width: 100.0,
                height: 32.0,
                color: p.accent,
                corner_radii: CornerRadii::all(8.0),
            });
            commands.push(RenderCommand::Text {
                x: cx - 24.0,
                y: btn_y + 8.0,
                text: "Sign In".to_string(),
                font_size: 13.0,
                color: p.on_accent(),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Error message.
            if let Some(msg) = &self.error_message {
                push_on_background(
                    commands,
                    p,
                    RenderCommand::Text {
                        x: cx - 100.0,
                        y: btn_y + 48.0,
                        text: msg.clone(),
                        font_size: 12.0,
                        color: p.red,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(200.0),
                        overflow: TextOverflow::Ellipsis,
                    },
                );
            }

            // Lockout message.
            if self.locked_out {
                push_on_background(
                    commands,
                    p,
                    RenderCommand::Text {
                        x: cx - 120.0,
                        y: btn_y + 70.0,
                        text: format!(
                            "Too many attempts. Try again in {}s.",
                            self.config.lockout_seconds
                        ),
                        font_size: 12.0,
                        color: p.yellow,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(240.0),
                        overflow: TextOverflow::Ellipsis,
                    },
                );
            }

            // Back button.
            if self.users.len() > 1 {
                push_on_background(
                    commands,
                    p,
                    RenderCommand::Text {
                        x: cx - 120.0,
                        y: cy - 80.0,
                        text: "\u{2190}".to_string(),
                        font_size: 20.0,
                        color: p.on_wallpaper_dim(),
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    },
                );
            }
        }
    }

    fn render_authenticating(&self, p: &Palette, commands: &mut Vec<RenderCommand>) {
        let cx = self.screen_width / 2.0;
        let cy = self.screen_height / 2.0;

        push_on_background(
            commands,
            p,
            RenderCommand::Text {
                x: cx - 60.0,
                y: cy,
                text: "Signing in...".to_string(),
                font_size: 16.0,
                color: p.on_wallpaper(),
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            },
        );
    }

    fn render_logging_in(&self, p: &Palette, commands: &mut Vec<RenderCommand>) {
        let cx = self.screen_width / 2.0;
        let cy = self.screen_height / 2.0;

        if let Some(user) = self.current_user() {
            push_on_background(
                commands,
                p,
                RenderCommand::Text {
                    x: cx - 80.0,
                    y: cy,
                    text: format!("Welcome, {}!", user.display_name),
                    font_size: 20.0,
                    color: p.on_wallpaper(),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                },
            );
        }
    }

    fn render_bottom_bar(&self, p: &Palette, commands: &mut Vec<RenderCommand>) {
        let bar_y = self.screen_height - 40.0;
        let bar_h = 40.0;

        // Semi-transparent bar. A panel this module draws, so the four things
        // mounted on it below take ordinary roles rather than the wallpaper
        // pair — that is the whole distinction judgement 1 draws.
        commands.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: self.screen_width,
            height: bar_h,
            color: Color::rgba(p.crust.r, p.crust.g, p.crust.b, 180),
            corner_radii: CornerRadii::ZERO,
        });

        // Keyboard layout (left).
        if self.config.show_keyboard_layout {
            commands.push(RenderCommand::Text {
                x: 16.0,
                y: bar_y + 12.0,
                text: self.keyboard_layout.clone(),
                font_size: 12.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Power button (right).
        if self.config.show_power {
            commands.push(RenderCommand::Text {
                x: self.screen_width - 40.0,
                y: bar_y + 10.0,
                text: "\u{23FB}".to_string(),
                font_size: 16.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Accessibility (before power).
        if self.config.show_accessibility {
            commands.push(RenderCommand::Text {
                x: self.screen_width - 80.0,
                y: bar_y + 10.0,
                text: "\u{267F}".to_string(),
                font_size: 16.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // On-screen keyboard.
        if self.config.show_osk_button {
            commands.push(RenderCommand::Text {
                x: self.screen_width - 120.0,
                y: bar_y + 10.0,
                text: "\u{2328}".to_string(),
                font_size: 16.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn render_power_menu(&self, p: &Palette, commands: &mut Vec<RenderCommand>) {
        let menu_w = 160.0;
        let menu_h = 140.0;
        let mx = self.screen_width - menu_w - 16.0;
        let my = self.screen_height - 40.0 - menu_h - 8.0;

        commands.push(RenderCommand::FillRect {
            x: mx,
            y: my,
            width: menu_w,
            height: menu_h,
            color: p.mantle,
            corner_radii: CornerRadii::all(8.0),
        });
        commands.push(RenderCommand::StrokeRect {
            x: mx,
            y: my,
            width: menu_w,
            height: menu_h,
            color: p.surface1,
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
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            commands.push(RenderCommand::Text {
                x: mx + 36.0,
                y: iy + 7.0,
                text: label.to_string(),
                font_size: 13.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
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
    // Font sizes and rectangle dimensions are the *identifiers* the colour
    // tables below look a site up by — 16pt is written as `16.0` in the
    // renderer and as `16.0` here, and nothing computes either. An approximate
    // comparison would let one helper match two different sites, which is the
    // failure those helpers exist to prevent.
    #![allow(clippy::float_cmp)]

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
    fn the_mask_draws_one_bullet_per_keystroke_not_per_byte() {
        // `password_display_masked` above is pure ASCII, where a byte count and
        // a character count are the same number — so it passed throughout the
        // life of the bug. Three keys pressed, three bullets, whatever those
        // keys were worth in UTF-8: 'e' is 1 byte, 'é' is 2, '€' is 3, an emoji
        // is 4.
        for (label, ch) in [
            ("ascii", 'e'),
            ("two-byte", '\u{e9}'),
            ("three-byte", '\u{20ac}'),
            ("four-byte", '\u{1f512}'),
        ] {
            let mut s = make_screen();
            s.select_user(0);
            s.type_char(ch);
            s.type_char(ch);
            s.type_char(ch);
            assert_eq!(
                s.password_display(),
                "\u{2022}\u{2022}\u{2022}",
                "three {label} keystrokes must draw three bullets"
            );
        }
    }

    #[test]
    fn one_backspace_erases_one_bullet_whatever_the_character_cost() {
        // The other half of the same defect: `backspace` pops a char, so under
        // a byte count one press removed a character and several bullets.
        let mut s = make_screen();
        s.select_user(0);
        s.type_char('\u{1f512}');
        s.type_char('\u{e9}');
        assert_eq!(s.password_display(), "\u{2022}\u{2022}");
        s.backspace();
        assert_eq!(s.password_display(), "\u{2022}");
        s.backspace();
        assert_eq!(s.password_display(), "");
    }

    #[test]
    fn submitting_with_no_selectable_user_does_not_strand_the_phase() {
        // The guard used to sit above the phase change, so this path returned
        // None correctly — but the indexing it protected sat below. Pin the
        // property that actually matters: a refused submit leaves the screen
        // where the user can try again, rather than in `Authenticating` with
        // nothing in flight.
        let mut s = make_screen();
        s.select_user(0);
        s.type_char('x');
        s.users.clear();
        assert_eq!(s.submit_password(), None);
        assert_eq!(s.phase, LoginPhase::PasswordEntry);
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

    /// The default names no colour at all.
    ///
    /// It used to be `SolidColor(CRUST)`, where `CRUST` was a Mocha literal in
    /// this file — a default that had already made the choice the palette
    /// exists to make. `matches!` on `Theme` is not a restatement of the code:
    /// it pins that the default carries *no* payload, so a future edit cannot
    /// quietly reintroduce a hard-coded colour by changing the argument.
    #[test]
    fn the_default_background_defers_its_colour_to_the_palette() {
        assert_eq!(LoginBackground::default(), LoginBackground::Theme);
    }

    // ---- Rendering ----

    #[test]
    fn render_user_select() {
        let s = make_screen();
        let cmds = s.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_password_entry() {
        let mut s = make_screen();
        s.select_user(0);
        let cmds = s.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_with_error() {
        let mut s = make_screen();
        s.select_user(0);
        s.phase = LoginPhase::Failed;
        s.error_message = Some("Bad password".to_string());
        let cmds = s.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_authenticating() {
        let mut s = make_screen();
        s.phase = LoginPhase::Authenticating;
        let cmds = s.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_logging_in() {
        let mut s = make_screen();
        s.phase = LoginPhase::LoggingIn;
        let cmds = s.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_power_menu() {
        let mut s = make_screen();
        s.power_menu_open = true;
        let cmds = s.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_gradient_background() {
        let mut s = make_screen();
        s.config.background = LoginBackground::Gradient {
            top: Color::from_hex(0x1E1E2E),
            bottom: Color::from_hex(0x11111B),
        };
        let cmds = s.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_lockout() {
        let mut s = make_screen();
        s.select_user(0);
        s.locked_out = true;
        let cmds = s.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    // ========================================================================
    // Colour — the palette conversion (module 28 of 49)
    // ========================================================================
    //
    // This file used to hold eleven Mocha constants of its own. What follows
    // proves they are gone and that what replaced them is in the role it
    // claims. Seven lessons from earlier modules are applied here up front, and
    // this module adds an eighth:
    //
    // 1. The sweep is only as wide as the render it is given: enumerate the
    //    renderer's `if`s, not its colours.
    // 2. A representative sample is not a per-site check: n *source* sites give
    //    n assertions, not n rendered instances and not n kinds.
    // 3. A site that only *selects* a colour is a source site too, even though
    //    it draws nothing itself.
    // 4. Render under an accent that is in neither palette. Testing a
    //    conversion in the palette it was converted *from* hides exactly the
    //    failures that conversion causes.
    // 5. An expectation written in terms of the code under test cannot fail.
    //    State the role literal.
    // 6. You cannot recognise the legitimate instances of a property by testing
    //    for that property — the bug has it too. Count them instead.
    // 7. Pinning a surface to one mode disarms the membership sweep for that
    //    surface; the per-site tables are then the whole proof.
    //
    // 8. NEW HERE: *a declared derivation is a claim about a value, not about
    //    where that value may appear — and this module draws one such value
    //    all over the screen.* `on_wallpaper()` is `#EFF1F5`, which the sweep
    //    has to be told about in a dark render, and which is also Latte's
    //    `base`, so in a light render it is an ordinary role and needs no
    //    telling. Either way it is an accounted-for colour everywhere, so a
    //    site that took `p.on_wallpaper()` where it owed `p.text`, or the
    //    reverse, is something only the tables below can see. That
    //    matters more here than the arithmetic suggests, because the mistake is
    //    a legibility bug rather than a colour bug: `p.text` on a photograph is
    //    unreadable, and looks perfectly fine in every screenshot taken against
    //    the default background. [`floating_text`] and [`panel_text`] exist to
    //    turn the boundary into an assertion that cannot be satisfied by
    //    accident — a text is either mounted on a panel (one command) or on the
    //    background (two, the first a shadow), and a site that changes sides
    //    fails whichever of the two helpers names it.

    use crate::palette_check::assert_drawn_from;
    use appearance::readable_on;

    /// An accent that is in neither palette, so a site reaching for the accent
    /// where it must not is visible rather than merely plausible. The stock
    /// accent *is* `blue`, and this screen used to draw `BLUE` at four sites;
    /// under the stock accent a missed one is indistinguishable from a
    /// converted one.
    const OFF_PALETTE: Color = Color::from_hex(0x00FF_8C1A);

    /// The avatar glyph `LoginUser::new` gives everyone.
    const AVATAR: &str = "\u{1F464}";

    /// Both modes, each with an accent no palette contains.
    fn table_palettes() -> Vec<(String, Palette)> {
        [false, true]
            .into_iter()
            .map(|light| {
                let mut p = Palette::for_mode(light);
                p.accent = OFF_PALETTE;
                (format!("light={light}"), p)
            })
            .collect()
    }

    fn base() -> LoginScreen {
        LoginScreen::new(1920.0, 1080.0, make_users())
    }

    /// A password-entry render with every optional element present at once.
    ///
    /// Two users (so the back arrow is drawn), a typed password, an error, a
    /// lockout, the clock and the date. Used by the counting tests, which need
    /// one render that contains every site they are counting.
    fn everything() -> LoginScreen {
        let mut s = base();
        s.select_user(0);
        s.type_char('h');
        s.error_message = Some("Bad password".to_string());
        s.locked_out = true;
        s
    }

    /// Every distinct render this screen has: one fixture per branch of every
    /// `if` and `match` in the seven renderers (lesson 1).
    ///
    /// Takes the palette because two of the background variants carry a colour
    /// the *user* chose. Those are given a role value here so the membership
    /// sweep can account for them; that they are drawn verbatim rather than
    /// re-themed is a separate claim, proved by
    /// [`a_user_chosen_background_is_never_re_themed`].
    fn screens(p: &Palette) -> Vec<(String, LoginScreen)> {
        let mut v: Vec<(String, LoginScreen)> = Vec::new();

        for (name, bg) in [
            ("bg theme", LoginBackground::Theme),
            ("bg solid", LoginBackground::SolidColor(p.mauve)),
            (
                "bg same as desktop",
                LoginBackground::SameAsDesktop("/w.png".to_string()),
            ),
            (
                "bg custom image",
                LoginBackground::CustomImage("/w.png".to_string()),
            ),
            (
                "bg gradient",
                LoginBackground::Gradient {
                    top: p.mauve,
                    bottom: p.mauve,
                },
            ),
        ] {
            let mut s = base();
            s.config.background = bg;
            v.push((name.to_string(), s));
        }

        // Clock and date, both ways.
        let mut s = base();
        s.config.show_clock = false;
        v.push(("clock hidden".to_string(), s));
        let mut s = base();
        s.config.show_date = false;
        v.push(("date hidden".to_string(), s));

        // The six phases. `UserSelect` with two users covers the selected and
        // the unselected row in one render.
        v.push(("user select".to_string(), base()));

        let mut s = base();
        s.select_user(0);
        v.push(("password entry, empty".to_string(), s));

        let mut s = base();
        s.select_user(0);
        s.type_char('h');
        s.type_char('i');
        v.push(("password entry, typed".to_string(), s));

        let mut s = base();
        s.select_user(0);
        s.type_char('h');
        s.show_password = true;
        v.push(("password entry, revealed".to_string(), s));

        let mut s = base();
        s.select_user(0);
        s.phase = LoginPhase::Failed;
        s.error_message = Some("Bad password".to_string());
        v.push(("failed".to_string(), s));

        let mut s = base();
        s.select_user(0);
        s.locked_out = true;
        v.push(("locked out".to_string(), s));

        let mut s = base();
        s.phase = LoginPhase::Authenticating;
        v.push(("authenticating".to_string(), s));

        let mut s = base();
        s.phase = LoginPhase::LoggingIn;
        v.push(("logging in".to_string(), s));

        let mut s = base();
        s.select_user(0);
        s.phase = LoginPhase::Locked;
        v.push(("locked".to_string(), s));

        // One user: no back arrow.
        let mut s = LoginScreen::new(1920.0, 1080.0, vec![LoginUser::new(1, "solo", "Solo")]);
        s.select_user(0);
        v.push(("single user".to_string(), s));

        // No user at all: the `if let` in the password renderer takes its None
        // arm, which draws nothing.
        let mut s = LoginScreen::new(1920.0, 1080.0, Vec::new());
        s.phase = LoginPhase::PasswordEntry;
        v.push(("no user".to_string(), s));

        // The overlay.
        let mut s = base();
        s.power_menu_open = true;
        v.push(("power menu".to_string(), s));

        // Every bottom-bar element off, which is four `if`s at once.
        let mut s = base();
        s.config.show_keyboard_layout = false;
        s.config.show_power = false;
        s.config.show_accessibility = false;
        s.config.show_osk_button = false;
        v.push(("bare bottom bar".to_string(), s));

        v.push(("everything".to_string(), everything()));

        v
    }

    /// Every `(x, y, colour)` of the texts with this exact string *and* size.
    ///
    /// Both discriminators are needed: this screen draws the same avatar glyph
    /// at 28pt in a row and at 48pt in the password panel, and draws the power
    /// glyph at 16pt in the bar and 14pt in the menu.
    fn texts(cmds: &[RenderCommand], want: &str, size: f32) -> Vec<(f32, f32, Color)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    color,
                    font_size,
                    ..
                } if text == want && *font_size == size => Some((*x, *y, *color)),
                _ => None,
            })
            .collect()
    }

    /// The colour of a text that must be mounted on a panel: one command, with
    /// no shadow under it.
    ///
    /// Half of the boundary in lesson 8. A site that moved onto the background
    /// would draw two commands and fail here.
    fn panel_text(cmds: &[RenderCommand], want: &str, size: f32) -> Color {
        let hits = texts(cmds, want, size);
        assert_eq!(
            hits.len(),
            1,
            "{want:?} at {size}pt: expected exactly one command, because it is \
             mounted on a panel this module filled itself; found {}",
            hits.len()
        );
        hits[0].2
    }

    /// The `(shadow, ink)` pair of a text that must sit on the login
    /// background.
    ///
    /// The other half of the boundary. Two commands, the shadow first and one
    /// pixel down-right of the ink — a site that moved onto a panel would draw
    /// one command and fail here.
    fn floating_text(cmds: &[RenderCommand], want: &str, size: f32) -> (Color, Color) {
        let hits = texts(cmds, want, size);
        assert_eq!(
            hits.len(),
            2,
            "{want:?} at {size}pt: expected a shadow and an ink, because it \
             lands on a background this module did not choose; found {}",
            hits.len()
        );
        let (sx, sy, shadow) = hits[0];
        let (ix, iy, ink) = hits[1];
        assert!(
            (sx - ix - 1.0).abs() < 0.001 && (sy - iy - 1.0).abs() < 0.001,
            "{want:?}: the shadow must sit one pixel down-right of the ink, \
             not at ({sx}, {sy}) against ({ix}, {iy})"
        );
        (shadow, ink)
    }

    fn fills_of_size(cmds: &[RenderCommand], w: f32, h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if *width == w && *height == h => Some(*color),
                _ => None,
            })
            .collect()
    }

    fn fill_of_size(cmds: &[RenderCommand], w: f32, h: f32) -> Color {
        let hits = fills_of_size(cmds, w, h);
        assert_eq!(hits.len(), 1, "fill {w}x{h}: {} matches", hits.len());
        hits[0]
    }

    fn stroke_of_size(cmds: &[RenderCommand], w: f32, h: f32) -> Color {
        let hits: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::StrokeRect {
                    width,
                    height,
                    color,
                    ..
                } if *width == w && *height == h => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "stroke {w}x{h}: {} matches", hits.len());
        hits[0]
    }

    fn every_color(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    fn count_of(cmds: &[RenderCommand], c: Color) -> usize {
        every_color(cmds).into_iter().filter(|x| *x == c).count()
    }

    /// Lesson 1: the sweep walks every fixture in both modes, so it sees every
    /// branch rather than the one the default configuration happens to take.
    #[test]
    fn every_colour_the_login_screen_draws_comes_from_its_palette() {
        for (mode, p) in table_palettes() {
            for (what, s) in screens(&p) {
                let cmds = s.render(&p);
                // The three computed inks: the pale extreme the wallpaper
                // labels take in both modes (see note 8), and the lettering
                // on the accent-filled sign-in button.
                let derived = [p.on_wallpaper(), p.on_wallpaper_dim(), p.on_accent()];
                assert_drawn_from(&p, &cmds, &derived, &format!("login {what} ({mode})"));
            }
        }
    }

    /// Lesson 6: the fixture list is only worth what it covers, and "it looks
    /// thorough" is not a measurement. Every branch is named and counted here,
    /// so deleting a fixture fails a test instead of quietly narrowing the
    /// sweep above.
    #[test]
    fn the_fixtures_take_every_branch_this_module_has() {
        let p = Palette::for_mode(false);
        let all = screens(&p);

        // The five background arms.
        for want in [
            "bg theme",
            "bg solid",
            "bg same as desktop",
            "bg custom image",
            "bg gradient",
        ] {
            assert!(
                all.iter().any(|(n, _)| n == want),
                "no fixture covers {want}"
            );
        }

        // Every phase the `match` in `render` dispatches on.
        for phase in [
            LoginPhase::UserSelect,
            LoginPhase::PasswordEntry,
            LoginPhase::Authenticating,
            LoginPhase::LoggingIn,
            LoginPhase::Failed,
            LoginPhase::Locked,
        ] {
            assert!(
                all.iter().any(|(_, s)| s.phase == phase),
                "no fixture renders {phase:?}"
            );
        }

        // Every remaining two-way branch, both ways.
        /// A named two-way branch and the predicate that reads it off a
        /// fixture.
        type Branch = (&'static str, fn(&LoginScreen) -> bool);

        let both_ways: [Branch; 11] = [
            ("show_clock", |s| s.config.show_clock),
            ("show_date", |s| s.config.show_date),
            ("show_keyboard_layout", |s| s.config.show_keyboard_layout),
            ("show_power", |s| s.config.show_power),
            ("show_accessibility", |s| s.config.show_accessibility),
            ("show_osk_button", |s| s.config.show_osk_button),
            ("power_menu_open", |s| s.power_menu_open),
            ("locked_out", |s| s.locked_out),
            ("error_message", |s| s.error_message.is_some()),
            ("password typed", |s| !s.password().is_empty()),
            ("show_password", |s| s.show_password),
        ];
        for (name, pred) in both_ways {
            assert!(all.iter().any(|(_, s)| pred(s)), "no fixture has {name}");
            assert!(all.iter().any(|(_, s)| !pred(s)), "no fixture lacks {name}");
        }

        // The back arrow is drawn only with more than one user, and the
        // password renderer's `if let` needs a fixture with none at all.
        for (label, want) in [("no users", 0), ("one user", 1), ("two users", 2)] {
            assert!(
                all.iter().any(|(_, s)| s.users.len() == want),
                "no fixture has {label}"
            );
        }
    }

    /// The six sites in the user list, one assertion each (lesson 2).
    #[test]
    fn every_colour_in_the_user_list_is_in_the_role_it_claims() {
        for (mode, p) in table_palettes() {
            let cmds = base().render(&p);

            // Two rows, in index order: alice is selected, bob is not.
            let rows = fills_of_size(&cmds, 280.0, 64.0);
            assert_eq!(rows.len(), 2, "{mode}: two users, two rows");
            assert_eq!(rows[0], p.surface0, "{mode}: the selected row");
            assert_eq!(
                rows[1],
                Color::rgba(p.base.r, p.base.g, p.base.b, 180),
                "{mode}: an unselected row is `base` at the module's panel alpha"
            );

            // The avatars, same glyph and size, distinguished by row order.
            let avatars = texts(&cmds, AVATAR, 28.0);
            assert_eq!(avatars.len(), 2, "{mode}: two avatars");
            assert_eq!(
                avatars[0].2, p.accent,
                "{mode}: the accent marks which row you are on"
            );
            assert_eq!(avatars[1].2, p.subtext0, "{mode}: an unselected avatar");

            assert_eq!(panel_text(&cmds, "Alice", 16.0), p.text, "{mode}: a name");
            assert_eq!(panel_text(&cmds, "Bob", 16.0), p.text, "{mode}: a name");
            assert_eq!(
                panel_text(&cmds, "Administrator", 11.0),
                p.overlay0,
                "{mode}: an account type"
            );
            assert_eq!(
                panel_text(&cmds, "Standard", 11.0),
                p.overlay0,
                "{mode}: an account type"
            );
        }
    }

    /// The thirteen sites in the password panel, one assertion each.
    #[test]
    fn every_colour_in_the_password_entry_is_in_the_role_it_claims() {
        for (mode, p) in table_palettes() {
            // The failed render: error border, error message, lockout notice,
            // a typed password, and two users so the back arrow is drawn.
            let cmds = everything().render(&p);

            assert_eq!(
                floating_text(&cmds, AVATAR, 48.0).1,
                p.accent,
                "{mode}: the avatar of the user you are signing in as"
            );
            assert_eq!(
                floating_text(&cmds, "Alice", 18.0).1,
                p.on_wallpaper(),
                "{mode}: the name sits on the background, not on a panel"
            );
            assert_eq!(
                fill_of_size(&cmds, 260.0, 36.0),
                p.surface0,
                "{mode}: the password field"
            );
            assert_eq!(
                stroke_of_size(&cmds, 260.0, 36.0),
                p.red,
                "{mode}: a rejected password borders red, never the accent"
            );
            assert_eq!(
                panel_text(&cmds, "\u{2022}", 14.0),
                p.text,
                "{mode}: a typed password"
            );
            assert_eq!(
                panel_text(&cmds, "\u{1F576}", 14.0),
                p.subtext0,
                "{mode}: the reveal toggle"
            );
            assert_eq!(
                fill_of_size(&cmds, 100.0, 32.0),
                p.accent,
                "{mode}: Sign In is the default action"
            );
            assert_eq!(
                panel_text(&cmds, "Sign In", 13.0),
                readable_on(p.accent),
                "{mode}: its label is derived from the accent, not named"
            );
            assert_eq!(
                floating_text(&cmds, "Bad password", 12.0).1,
                p.red,
                "{mode}: the error message"
            );
            assert_eq!(
                floating_text(&cmds, "Too many attempts. Try again in 30s.", 12.0).1,
                p.yellow,
                "{mode}: the lockout notice"
            );
            assert_eq!(
                floating_text(&cmds, "\u{2190}", 20.0).1,
                p.on_wallpaper_dim(),
                "{mode}: the back arrow"
            );

            // The two remaining branches need a render without an error and
            // without a typed password.
            let mut s = base();
            s.select_user(0);
            let cmds = s.render(&p);
            assert_eq!(
                stroke_of_size(&cmds, 260.0, 36.0),
                p.surface1,
                "{mode}: a field at rest"
            );
            assert_eq!(
                panel_text(&cmds, "Password", 14.0),
                p.overlay0,
                "{mode}: the placeholder"
            );

            // And the revealed toggle is a third glyph.
            let mut s = base();
            s.select_user(0);
            s.show_password = true;
            let cmds = s.render(&p);
            assert_eq!(
                panel_text(&cmds, "\u{1F441}", 14.0),
                p.subtext0,
                "{mode}: the reveal toggle, showing"
            );
        }
    }

    /// The nine sites in the bottom bar and the power menu.
    #[test]
    fn every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims() {
        for (mode, p) in table_palettes() {
            let mut s = base();
            s.power_menu_open = true;
            let cmds = s.render(&p);

            assert_eq!(
                fill_of_size(&cmds, 1920.0, 40.0),
                Color::rgba(p.crust.r, p.crust.g, p.crust.b, 180),
                "{mode}: the bottom bar"
            );
            assert_eq!(panel_text(&cmds, "US", 12.0), p.subtext0, "{mode}: layout");
            assert_eq!(
                panel_text(&cmds, "\u{23FB}", 16.0),
                p.subtext0,
                "{mode}: the bar's power button"
            );
            assert_eq!(
                panel_text(&cmds, "\u{267F}", 16.0),
                p.subtext0,
                "{mode}: accessibility"
            );
            assert_eq!(
                panel_text(&cmds, "\u{2328}", 16.0),
                p.subtext0,
                "{mode}: the on-screen keyboard button"
            );

            assert_eq!(
                fill_of_size(&cmds, 160.0, 140.0),
                p.mantle,
                "{mode}: the power menu"
            );
            assert_eq!(
                stroke_of_size(&cmds, 160.0, 140.0),
                p.surface1,
                "{mode}: its border"
            );
            assert_eq!(
                panel_text(&cmds, "\u{23FB}", 14.0),
                p.subtext0,
                "{mode}: a menu icon"
            );
            assert_eq!(
                panel_text(&cmds, "Shut Down", 13.0),
                p.text,
                "{mode}: a menu label"
            );
        }
    }

    /// The four texts that land on the background rather than on a panel.
    ///
    /// Two of them live in phases of their own, so they need their own
    /// fixtures — lesson 1 again, at the level of the file rather than the
    /// function.
    #[test]
    fn the_clock_and_the_status_lines_are_wallpaper_ink() {
        for (mode, p) in table_palettes() {
            let cmds = base().render(&p);
            assert_eq!(
                floating_text(&cmds, "12:00", 64.0).1,
                p.on_wallpaper(),
                "{mode}: the clock"
            );
            assert_eq!(
                floating_text(&cmds, "Sunday, May 18, 2026", 16.0).1,
                p.on_wallpaper_dim(),
                "{mode}: the date is subordinate to the time"
            );

            let mut s = base();
            s.phase = LoginPhase::Authenticating;
            assert_eq!(
                floating_text(&s.render(&p), "Signing in...", 16.0).1,
                p.on_wallpaper(),
                "{mode}: the authenticating notice"
            );

            let mut s = base();
            s.phase = LoginPhase::LoggingIn;
            assert_eq!(
                floating_text(&s.render(&p), "Welcome, Alice!", 20.0).1,
                p.on_wallpaper(),
                "{mode}: the greeting"
            );
        }
    }

    /// The background fill when the user named no colour.
    #[test]
    fn a_background_with_no_colour_of_its_own_takes_the_theme() {
        for (mode, p) in table_palettes() {
            for bg in [
                LoginBackground::Theme,
                LoginBackground::SameAsDesktop("/w.png".to_string()),
                LoginBackground::CustomImage("/w.png".to_string()),
            ] {
                let mut s = base();
                s.config.background = bg.clone();
                assert_eq!(
                    fill_of_size(&s.render(&p), 1920.0, 1080.0),
                    p.crust,
                    "{mode}: {bg:?} must resolve to the theme's deepest surface"
                );
            }
        }
    }

    /// A colour the user picked is drawn as the user picked it.
    ///
    /// The mirror of the test above, and the reason `Theme` had to be a
    /// separate variant rather than `SolidColor(p.crust)` resolved somewhere:
    /// re-theming every solid background would silently discard a setting.
    #[test]
    fn a_user_chosen_background_is_never_re_themed() {
        const USER_SOLID: Color = Color::from_hex(0x0034_2C1F);
        const USER_TOP: Color = Color::from_hex(0x0051_0A3C);
        const USER_BOTTOM: Color = Color::from_hex(0x000B_4F45);

        for (mode, p) in table_palettes() {
            let mut s = base();
            s.config.background = LoginBackground::SolidColor(USER_SOLID);
            assert_eq!(
                fill_of_size(&s.render(&p), 1920.0, 1080.0),
                USER_SOLID,
                "{mode}: the user's own background colour"
            );

            let mut s = base();
            s.config.background = LoginBackground::Gradient {
                top: USER_TOP,
                bottom: USER_BOTTOM,
            };
            let bands = fills_of_size(&s.render(&p), 1920.0, 55.0);
            assert_eq!(bands.len(), 20, "{mode}: twenty gradient bands");
            assert_eq!(bands[0], USER_TOP, "{mode}: the top of the gradient");
            assert_eq!(bands[19], USER_BOTTOM, "{mode}: the bottom of the gradient");
        }
    }

    /// A gradient between a colour and itself is that colour, every band.
    ///
    /// Found by the membership sweep above, which rejected `#CBA5F7` — one
    /// step off `mauve` — in a gradient whose two endpoints were both `mauve`.
    /// The band colour was truncated rather than rounded, and
    /// `a*(1-t) + a*t` lands a hair under `a` in `f32` for most `t`, so a flat
    /// "gradient" came out as twenty stripes alternating between the user's
    /// colour and one darker. The same bias darkened every real gradient along
    /// its whole length; it was invisible there because a gradient is expected
    /// to change.
    #[test]
    fn a_gradient_between_a_colour_and_itself_is_flat() {
        let p = Palette::for_mode(false);
        // A value that exercises the failure: every channel of Mocha `mauve`
        // truncated a step at some of the twenty band positions.
        for flat in [p.mauve, p.red, p.text, Color::from_hex(0x0001_0203)] {
            let mut s = base();
            s.config.background = LoginBackground::Gradient {
                top: flat,
                bottom: flat,
            };
            let bands = fills_of_size(&s.render(&p), 1920.0, 55.0);
            assert_eq!(bands.len(), 20);
            for (i, band) in bands.iter().enumerate() {
                assert_eq!(
                    *band, flat,
                    "band {i} of a flat gradient drifted off the colour the \
                     user chose"
                );
            }
        }
    }

    /// And a real gradient rounds to the nearest step at each band.
    ///
    /// The companion claim: the fix must not be "special-case equal
    /// endpoints". Half way between 0 and 255 is 127.5, which rounds to 128 and
    /// truncates to 127 — so this fails under the old arithmetic too.
    #[test]
    fn a_gradient_band_rounds_to_the_nearest_step() {
        let p = Palette::for_mode(false);
        let mut s = base();
        s.config.background = LoginBackground::Gradient {
            top: Color::from_hex(0x0000_0000),
            bottom: Color::from_hex(0x00FF_FFFF),
        };
        let bands = fills_of_size(&s.render(&p), 1920.0, 55.0);
        assert_eq!(bands.len(), 20);
        // Bands 9 and 10 straddle the midpoint: t = 9/19 and 10/19, so the
        // exact values are 120.789… and 134.210…, which round to 121 and 134.
        assert_eq!(bands[9].r, 121, "the band just above the midpoint");
        assert_eq!(bands[10].r, 134, "the band just below it");
        assert_eq!(bands[0].r, 0, "the top endpoint is exact");
        assert_eq!(bands[19].r, 255, "and so is the bottom");
    }

    /// Lesson 6, applied to the accent: count the sites that carry it.
    ///
    /// Every role table above runs under an accent no palette contains, which
    /// catches a site that reaches for the accent *instead of* a role. It
    /// cannot catch a site that reaches for the accent *as well* — a new
    /// button, or a border someone decided to brighten. The count can.
    #[test]
    fn exactly_two_things_in_the_password_panel_carry_the_accent() {
        for (mode, p) in table_palettes() {
            let cmds = everything().render(&p);
            assert_eq!(
                count_of(&cmds, OFF_PALETTE),
                2,
                "{mode}: the avatar of the chosen user and the Sign In fill, \
                 and nothing else. A third would mean the accent had leaked \
                 onto something that is a category or a measurement — see \
                 judgements 2 and 3 at the top of the file."
            );

            let mut s = base();
            s.power_menu_open = true;
            assert_eq!(
                count_of(&s.render(&p), OFF_PALETTE),
                1,
                "{mode}: in the user list only the selected avatar is accented"
            );
        }
    }

    /// Lesson 6 again, applied to the panel/background boundary.
    ///
    /// The tables above name each floating site individually; this counts them,
    /// so a *new* site that lands on the background without a shadow — or a
    /// panel site that grows one — is caught even though no table names it.
    #[test]
    fn exactly_seven_things_in_the_full_render_sit_on_the_background() {
        // Stated as the literal rather than as `p.text_shadow()`: this is a
        // legibility floor, not a role, and a palette that started returning
        // something else for it would take the expectation with it.
        const SHADOW: Color = Color::rgba(0, 0, 0, 180);

        for (mode, p) in table_palettes() {
            let cmds = everything().render(&p);
            assert_eq!(
                count_of(&cmds, SHADOW),
                7,
                "{mode}: the clock, the date, the avatar, the name, the error, \
                 the lockout notice and the back arrow — every text this render \
                 puts on a surface the shell did not choose, and no others"
            );
            assert_eq!(
                count_of(&cmds, p.on_wallpaper()),
                2,
                "{mode}: the clock and the name"
            );
            assert_eq!(
                count_of(&cmds, p.on_wallpaper_dim()),
                2,
                "{mode}: the date and the back arrow"
            );
        }
    }

    /// The Sign In label is computed from the accent, not chosen from a role.
    ///
    /// Proving a colour is *derived* needs the thing it derives from to move,
    /// so this sweeps the accent from near-black to near-white and requires the
    /// label to take both answers. A label frozen to either endpoint — which is
    /// what `CRUST` was — passes half of this and fails the other half.
    #[test]
    fn the_sign_in_label_follows_the_accent_it_sits_on() {
        let mut seen_dark_ink = false;
        let mut seen_light_ink = false;
        for light in [false, true] {
            for v in [0x00_u8, 0x30, 0x60, 0x90, 0xC0, 0xF0] {
                let mut p = Palette::for_mode(light);
                p.accent = Color::rgb(v, v, v);
                let cmds = everything().render(&p);
                let ink = panel_text(&cmds, "Sign In", 13.0);
                assert_eq!(
                    ink,
                    readable_on(p.accent),
                    "light={light} accent={v:#04X}: the label must be readable \
                     on the fill it is drawn on"
                );
                if ink == Color::from_hex(0x0011_111B) {
                    seen_dark_ink = true;
                }
                if ink == Color::from_hex(0x00EF_F1F5) {
                    seen_light_ink = true;
                }
            }
        }
        assert!(
            seen_dark_ink && seen_light_ink,
            "the sweep never made the label change, so it proved nothing about \
             where the label comes from"
        );
    }

    /// The whole screen actually moves when the mode does.
    ///
    /// A canary for the failure this conversion exists to prevent: a module
    /// that takes a `&Palette`, ignores it, and draws the same pixels either
    /// way would pass every membership sweep in the file, because a Mocha value
    /// is a member of the Mocha palette.
    #[test]
    fn the_render_is_not_the_same_in_both_modes() {
        let dark = Palette::for_mode(false);
        let light = Palette::for_mode(true);
        for (what, s) in screens(&dark) {
            let a = every_color(&s.render(&dark));
            let b = every_color(&s.render(&light));
            assert_eq!(
                a.len(),
                b.len(),
                "{what}: the two modes must draw the \
                same commands, only in different colours"
            );
            assert_ne!(
                a, b,
                "{what}: this fixture draws identically in both modes, so \
                 nothing in it is reading the palette"
            );
        }
    }

    /// No Mocha literal survives anywhere in the file.
    ///
    /// The eleven deleted constants, checked by value against a light render.
    /// This is the one assertion that would catch a constant left behind in a
    /// branch none of the fixtures above happens to reach — and unlike the
    /// membership sweep it names the offender.
    #[test]
    fn none_of_the_eleven_deleted_constants_is_still_drawn() {
        let deleted: [(&str, u32); 11] = [
            ("BASE", 0x001E_1E2E),
            ("MANTLE", 0x0018_1825),
            ("CRUST", 0x0011_111B),
            ("SURFACE0", 0x0031_3244),
            ("SURFACE1", 0x0045_475A),
            ("TEXT", 0x00CD_D6F4),
            ("SUBTEXT0", 0x00A6_ADC8),
            ("BLUE", 0x0089_B4FA),
            ("RED", 0x00F3_8BA8),
            ("YELLOW", 0x00F9_E2AF),
            ("OVERLAY0", 0x006C_7086),
        ];
        let mut p = Palette::for_mode(true);
        p.accent = OFF_PALETTE;
        for (what, s) in screens(&p) {
            let drawn = every_color(&s.render(&p));
            for (name, hex) in deleted {
                // `CRUST` is exempt: `#11111B` is also what `readable_on`
                // answers for a pale fill, so the Sign In label legitimately
                // draws it whenever the accent is light. That hole is named
                // rather than papered over — see `palette_check`'s module docs.
                if name == "CRUST" {
                    continue;
                }
                let rgb = (
                    ((hex >> 16) & 0xFF) as u8,
                    ((hex >> 8) & 0xFF) as u8,
                    (hex & 0xFF) as u8,
                );
                assert!(
                    !drawn.iter().any(|c| (c.r, c.g, c.b) == rgb),
                    "{what}: the light render still draws {name} (#{hex:06X}), \
                     so that constant was not converted"
                );
            }
        }
    }
}

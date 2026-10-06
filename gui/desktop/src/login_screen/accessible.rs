//! The login screen as tools see it ([`Accessible`]): the screen a screen
//! reader meets first, before anyone has signed in.
//!
//! # What is shown
//!
//! The accounts, while the screen asks who you are: a list, each account
//! named as the screen names it, described by its kind, the one the
//! keyboard is on chosen. While it asks for a password: the field -- named
//! for whose password it is, its text never shown, whatever a tool's
//! rights, as no password is -- the eye that shows it or hides it, Sign In,
//! the arrow back to the accounts where there are others, and what the
//! screen says of a refusal or a lockout. Along the bottom bar: the power
//! button and the accessibility button, and each one's menu while it is up
//! -- the power choices, and each accessibility setting as a switch -- and
//! the keyboard layout.
//!
//! # As its user would
//!
//! Each part is pressed with the screen's own press at its middle, and
//! answers what the screen answers a click ([`LoginAction`]) for the
//! session to act on: a sign in, a power choice, a setting switched. A
//! menu up over the screen takes every press -- one beside it only closes
//! it -- so a part under it is refused, and nothing changes. The password
//! set is typed: the field emptied, then each character, as the keyboard
//! types them -- after a refusal, starting again first, as any key does.

use super::{
    ACCESS_MENU, Hit, LoginAccess, LoginAction, LoginPhase, LoginScreen, POWER_MENU_LABELS,
};
use guitk::event::{MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::widget::CheckState;
use guitk::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of the login screen, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginPart {
    /// The screen.
    Screen,
    /// The accounts, while the screen asks who you are.
    Accounts,
    /// An account, by its place among them.
    Account(usize),
    /// The password field, while the screen asks for one.
    Password,
    /// The eye that shows the password or hides it.
    Reveal,
    /// Sign In.
    SignIn,
    /// The arrow back to the accounts, where there are others.
    Back,
    /// What the screen says: a refusal, a lockout, signing in.
    Message,
    /// The power button in the bottom bar.
    Power,
    /// The power menu, while it is up.
    PowerMenu,
    /// A power choice, by its place in the menu.
    PowerChoice(usize),
    /// The accessibility button in the bottom bar.
    Accessibility,
    /// The accessibility menu, while it is up.
    AccessMenu,
    /// An accessibility setting's switch, in its menu.
    Access(LoginAccess),
    /// The keyboard layout, in the bottom bar.
    KeyboardLayout,
}

impl From<Hit> for Rect {
    fn from(hit: Hit) -> Self {
        Self::new(hit.x, hit.y, hit.w, hit.h)
    }
}

impl LoginScreen {
    /// Whether the screen asks for a password: the field and its controls
    /// are drawn.
    fn asks_for_password(&self) -> bool {
        matches!(self.phase, LoginPhase::PasswordEntry | LoginPhase::Failed)
            && self.current_user().is_some()
    }

    /// What the screen says now, if anything: why a password was refused,
    /// that there were too many tries, or that it is signing in.
    fn said(&self) -> Option<String> {
        if self.locked_out {
            return Some(format!(
                "Too many attempts. Try again in {}s.",
                self.config.lockout_seconds
            ));
        }
        if let Some(message) = &self.error_message {
            return Some(message.clone());
        }
        match self.phase {
            LoginPhase::Authenticating => Some("Signing in...".to_owned()),
            LoginPhase::LoggingIn => self
                .current_user()
                .map(|user| format!("Welcome, {}!", user.shown_name())),
            LoginPhase::UserSelect | LoginPhase::PasswordEntry | LoginPhase::Failed => None,
        }
    }

    /// The bottom bar's parts, and each one's menu while it is up.
    fn bar_nodes(&self) -> Vec<Node<LoginPart>> {
        let mut nodes = Vec::new();
        if self.config.show_keyboard_layout {
            nodes.push(Node::new(
                LoginPart::KeyboardLayout,
                Role::Label,
                format!("Keyboard layout: {}", self.keyboard_layout),
                self.keyboard_layout_rect().into(),
            ));
        }
        if self.config.show_accessibility {
            let mut button = Node::new(
                LoginPart::Accessibility,
                Role::Button,
                "Accessibility",
                self.a11y_button_rect().into(),
            );
            button.description = Some("Super+U".to_owned());
            nodes.push(button);
        }
        if self.config.show_power {
            nodes.push(Node::new(
                LoginPart::Power,
                Role::Button,
                "Power",
                self.power_button_rect().into(),
            ));
        }
        if self.a11y_menu_open {
            let mut menu = Node::new(
                LoginPart::AccessMenu,
                Role::Menu,
                "Accessibility",
                self.a11y_menu_rect().into(),
            );
            for (i, (setting, label)) in ACCESS_MENU.iter().enumerate() {
                let mut switch = Node::new(
                    LoginPart::Access(*setting),
                    Role::CheckBox,
                    *label,
                    self.a11y_menu_row_rect(i).into(),
                );
                switch.value = Some(Value::Check(if self.access(*setting) {
                    CheckState::Checked
                } else {
                    CheckState::Unchecked
                }));
                switch.focused = i == self.access_row;
                switch.focusable = true;
                menu.children.push(switch);
            }
            nodes.push(menu);
        }
        if self.power_menu_open {
            let mut menu = Node::new(
                LoginPart::PowerMenu,
                Role::Menu,
                "Power",
                self.power_menu_rect().into(),
            );
            for (i, (_, label)) in POWER_MENU_LABELS.iter().enumerate() {
                menu.children.push(Node::new(
                    LoginPart::PowerChoice(i),
                    Role::MenuItem,
                    *label,
                    self.power_menu_row_rect(i).into(),
                ));
            }
            nodes.push(menu);
        }
        nodes
    }

    /// Where `part` is pressed, or why it is not there to press.
    fn press_box(&self, part: LoginPart) -> Result<Hit, Refusal> {
        let shown = |there: bool, hit: Hit| {
            if there {
                Ok(hit)
            } else {
                Err(Refusal::NoSuchWidget)
            }
        };
        match part {
            LoginPart::Account(i) => shown(
                self.phase == LoginPhase::UserSelect && i < self.users.len(),
                self.user_row_rect(i),
            ),
            LoginPart::Reveal => shown(self.asks_for_password(), self.reveal_rect()),
            LoginPart::SignIn => shown(self.asks_for_password(), self.sign_in_rect()),
            LoginPart::Back => shown(
                self.asks_for_password() && self.users.len() > 1,
                self.back_rect(),
            ),
            LoginPart::Power => shown(self.config.show_power, self.power_button_rect()),
            LoginPart::Accessibility => {
                shown(self.config.show_accessibility, self.a11y_button_rect())
            }
            LoginPart::PowerChoice(i) => shown(
                self.power_menu_open && i < POWER_MENU_LABELS.len(),
                self.power_menu_row_rect(i),
            ),
            LoginPart::Access(setting) => {
                let row = ACCESS_MENU.iter().position(|(s, _)| *s == setting);
                match row {
                    Some(i) if self.a11y_menu_open => Ok(self.a11y_menu_row_rect(i)),
                    _ => Err(Refusal::NoSuchWidget),
                }
            }
            LoginPart::Screen
            | LoginPart::Accounts
            | LoginPart::Password
            | LoginPart::Message
            | LoginPart::PowerMenu
            | LoginPart::AccessMenu
            | LoginPart::KeyboardLayout => Err(Refusal::NoSuchWidget),
        }
    }

    /// Press `part` at its middle, as the user's press -- refused where a
    /// menu up over the screen would take the press instead.
    fn press_part(&mut self, part: LoginPart) -> Result<Option<LoginAction>, Refusal> {
        let hit = self.press_box(part)?;
        let in_menu = matches!(part, LoginPart::PowerChoice(_) | LoginPart::Access(_));
        if (self.power_menu_open || self.a11y_menu_open) && !in_menu {
            return Err(Refusal::Hidden);
        }
        let action = self.handle_mouse(&MouseEvent {
            x: hit.x + hit.w / 2.0,
            y: hit.y + hit.h / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        Ok((action != LoginAction::Ignored).then_some(action))
    }

    /// Type `text` into the password field, as the keyboard does: emptied
    /// first, then each character -- after a refusal, starting again first.
    fn type_password(&mut self, text: &str) -> Result<Option<LoginAction>, Refusal> {
        if !self.asks_for_password() {
            return Err(Refusal::NoSuchWidget);
        }
        if self.power_menu_open || self.a11y_menu_open {
            return Err(Refusal::Hidden);
        }
        if self.locked_out {
            return Err(Refusal::Disabled);
        }
        if self.phase == LoginPhase::Failed {
            self.retry();
        }
        self.clear_password();
        for c in text.chars().filter(|c| !c.is_control()) {
            self.type_char(c);
        }
        Ok(Some(LoginAction::Redraw))
    }
}

impl Accessible for LoginScreen {
    type Part = LoginPart;
    type Event = LoginAction;

    fn automation(&self, _width: f32, _height: f32) -> Node<LoginPart> {
        let mut screen = Node::new(
            LoginPart::Screen,
            Role::Dialog,
            "Sign in",
            Rect::new(0.0, 0.0, self.screen_width, self.screen_height),
        );
        let menu_up = self.power_menu_open || self.a11y_menu_open;
        if self.phase == LoginPhase::UserSelect {
            let rows: Vec<Hit> = (0..self.users.len())
                .map(|i| self.user_row_rect(i))
                .collect();
            let bounds = match (rows.first(), rows.last()) {
                (Some(first), Some(last)) => {
                    Rect::new(first.x, first.y, first.w, last.y + last.h - first.y)
                }
                _ => Rect::default(),
            };
            let mut list = Node::new(LoginPart::Accounts, Role::List, "Accounts", bounds);
            for (i, user) in self.users.iter().enumerate() {
                let mut row = Node::new(
                    LoginPart::Account(i),
                    Role::ListItem,
                    user.shown_name(),
                    self.user_row_rect(i).into(),
                );
                row.description = Some(user.account_type.clone());
                row.value = Some(Value::Chosen(i == self.selected_user));
                row.focused = !menu_up && i == self.selected_user;
                row.focusable = true;
                list.children.push(row);
            }
            screen.children.push(list);
        }
        if self.asks_for_password() {
            let name = self.current_user().map_or_else(String::new, |user| {
                format!("Password for {}", user.shown_name())
            });
            // Its text is never a node's, as no secret is.
            let mut field = Node::new(
                LoginPart::Password,
                Role::TextField,
                name,
                self.password_field_rect().into(),
            );
            field.focused = !menu_up;
            field.focusable = true;
            field.enabled = !self.locked_out;
            screen.children.push(field);
            let mut reveal = Node::new(
                LoginPart::Reveal,
                Role::CheckBox,
                "Show password",
                self.reveal_rect().into(),
            );
            reveal.value = Some(Value::Check(if self.show_password {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            }));
            screen.children.push(reveal);
            let mut sign_in = Node::new(
                LoginPart::SignIn,
                Role::Button,
                "Sign In",
                self.sign_in_rect().into(),
            );
            sign_in.enabled = !self.locked_out;
            screen.children.push(sign_in);
            if self.users.len() > 1 {
                screen.children.push(Node::new(
                    LoginPart::Back,
                    Role::Button,
                    "Back to the accounts",
                    self.back_rect().into(),
                ));
            }
        }
        if let Some(said) = self.said() {
            screen.children.push(Node::new(
                LoginPart::Message,
                Role::Label,
                said,
                self.message_rect().into(),
            ));
        }
        screen.children.extend(self.bar_nodes());
        screen
    }

    fn invoke(
        &mut self,
        part: &LoginPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<LoginAction>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        match (*part, &action) {
            (LoginPart::Password, Action::SetText(text)) => self.type_password(text),
            (LoginPart::Password, Action::Focus) => {
                if !self.asks_for_password() {
                    return Err(Refusal::NoSuchWidget);
                }
                // The field has the keyboard whenever it is shown and no menu
                // is up over it.
                if self.power_menu_open || self.a11y_menu_open {
                    return Err(Refusal::Hidden);
                }
                Ok(None)
            }
            (LoginPart::Password, _) => Err(not_for(Role::TextField)),
            (LoginPart::Account(_), Action::Press | Action::Choose | Action::Focus)
            | (LoginPart::Reveal | LoginPart::Access(_), Action::Press | Action::Toggle)
            | (
                LoginPart::SignIn
                | LoginPart::Back
                | LoginPart::Power
                | LoginPart::Accessibility
                | LoginPart::PowerChoice(_),
                Action::Press,
            ) => {
                if matches!(part, LoginPart::SignIn) && self.locked_out {
                    self.press_box(*part)?;
                    return Err(Refusal::Disabled);
                }
                self.press_part(*part)
            }
            (LoginPart::Account(_), _) => Err(not_for(Role::ListItem)),
            (LoginPart::Reveal | LoginPart::Access(_), _) => Err(not_for(Role::CheckBox)),
            (LoginPart::PowerChoice(_), _) => Err(not_for(Role::MenuItem)),
            (
                LoginPart::SignIn | LoginPart::Back | LoginPart::Power | LoginPart::Accessibility,
                _,
            ) => Err(not_for(Role::Button)),
            (LoginPart::Message | LoginPart::KeyboardLayout, _) => Err(not_for(Role::Label)),
            (LoginPart::PowerMenu | LoginPart::AccessMenu, _) => Err(not_for(Role::Menu)),
            (LoginPart::Accounts, _) => Err(not_for(Role::List)),
            (LoginPart::Screen, _) => Err(not_for(Role::Dialog)),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;

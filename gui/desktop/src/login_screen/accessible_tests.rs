//! Tests for the login screen as tools see it: its accounts, its password
//! field -- typed into, its text never shown -- Sign In and the rest, what it
//! says, and its bar's buttons and menus, each pressed as clicked, refused
//! under a menu.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;
use crate::login_screen::{LoginPowerAction, LoginUser};

/// Alice, an administrator who signed in last, and Bob, who gives no
/// display name.
fn screen() -> LoginScreen {
    LoginScreen::new(
        1920.0,
        1080.0,
        vec![
            LoginUser::new(Some(1000), "alice", "Alice")
                .with_admin()
                .with_last_login(),
            LoginUser::new(Some(1001), "bob", ""),
        ],
    )
}

fn act(
    s: &mut LoginScreen,
    part: LoginPart,
    action: Action,
) -> Result<Option<LoginAction>, Refusal> {
    s.invoke(&part, action, 0.0, 0.0)
}

fn node(s: &LoginScreen, part: LoginPart) -> Option<Node<LoginPart>> {
    s.automation(0.0, 0.0)
        .walk()
        .find(|n| n.id == part)
        .cloned()
}

/// **The accounts are a list, each named as the screen names it -- by its
/// login name where it gives no other -- described by its kind, the one the
/// keyboard is on chosen**; one chosen is clicked, and the screen asks for
/// its password.
#[test]
fn the_accounts_are_a_list_chosen_as_clicked() {
    let mut s = screen();
    let root = s.automation(0.0, 0.0);
    assert_eq!((root.role, root.name.as_str()), (Role::Dialog, "Sign in"));
    let accounts = node(&s, LoginPart::Accounts).expect("the list");
    let rows: Vec<(&str, Option<&str>, Option<Value>)> = accounts
        .children
        .iter()
        .map(|n| (n.name.as_str(), n.description.as_deref(), n.value.clone()))
        .collect();
    assert_eq!(
        rows,
        [
            ("Alice", Some("Administrator"), Some(Value::Chosen(true))),
            ("bob", Some("Standard"), Some(Value::Chosen(false))),
        ]
    );
    assert_eq!(
        Rect::from(s.user_row_rect(1)),
        accounts.children[1].bounds,
        "where it is drawn"
    );
    assert!(node(&s, LoginPart::Password).is_none(), "no field yet");

    assert_eq!(
        act(&mut s, LoginPart::Account(1), Action::Choose),
        Ok(Some(LoginAction::Redraw))
    );
    assert_eq!(s.phase, LoginPhase::PasswordEntry);
    assert_eq!(s.current_user().unwrap().username, "bob");
    assert!(node(&s, LoginPart::Accounts).is_none(), "the list gone");
    assert_eq!(
        act(&mut s, LoginPart::Account(0), Action::Press),
        Err(Refusal::NoSuchWidget)
    );
}

/// **The password is typed as the keyboard types it, its text never in a
/// node; Sign In asks to sign in with it**, as a click on it does. The eye
/// shows it or hides it, and says which.
#[test]
fn the_password_is_typed_and_signed_in_with() {
    let mut s = screen();
    act(&mut s, LoginPart::Account(0), Action::Choose).unwrap();
    let field = node(&s, LoginPart::Password).expect("the field");
    assert_eq!(
        (field.role, field.name.as_str()),
        (Role::TextField, "Password for Alice")
    );
    assert!(field.focused && field.enabled);
    assert_eq!(
        act(
            &mut s,
            LoginPart::Password,
            Action::SetText("s3cret".to_owned())
        ),
        Ok(Some(LoginAction::Redraw))
    );
    assert_eq!(s.password(), "s3cret");
    assert!(
        s.automation(0.0, 0.0)
            .walk()
            .all(|n| !n.name.contains("s3cret")
                && !matches!(&n.value, Some(Value::Text(t)) if t.contains("s3cret"))),
        "never shown"
    );
    assert_eq!(
        act(
            &mut s,
            LoginPart::Password,
            Action::SetText("pw".to_owned())
        ),
        Ok(Some(LoginAction::Redraw))
    );
    assert_eq!(s.password(), "pw", "emptied first, as a new password");
    s.power_menu_open = true;
    assert!(
        !node(&s, LoginPart::Password).unwrap().focused,
        "the keyboard is the menu's while it is up"
    );
    s.power_menu_open = false;

    assert_eq!(
        act(&mut s, LoginPart::Reveal, Action::Toggle),
        Ok(Some(LoginAction::Redraw))
    );
    assert_eq!(
        node(&s, LoginPart::Reveal).unwrap().value,
        Some(Value::Check(CheckState::Checked))
    );
    assert_eq!(
        act(&mut s, LoginPart::SignIn, Action::Press),
        Ok(Some(LoginAction::Authenticate {
            username: "alice".to_owned(),
            password: "pw".to_owned(),
        }))
    );
    assert_eq!(s.phase, LoginPhase::Authenticating);
    assert_eq!(
        node(&s, LoginPart::Message).map(|n| n.name),
        Some("Signing in...".to_owned())
    );
}

/// **A refusal is said, and the password typed after it starts again** --
/// as any key does after one; locked out, the field and Sign In are
/// disabled, and say so.
#[test]
fn a_refusal_is_said_and_a_lockout_disables_the_field() {
    let mut s = screen();
    act(&mut s, LoginPart::Account(0), Action::Choose).unwrap();
    act(
        &mut s,
        LoginPart::Password,
        Action::SetText("wrong".to_owned()),
    )
    .unwrap();
    act(&mut s, LoginPart::SignIn, Action::Press).unwrap();
    s.auth_failure("Incorrect password");
    assert_eq!(
        node(&s, LoginPart::Message).map(|n| n.name),
        Some("Incorrect password".to_owned())
    );
    assert_eq!(
        act(
            &mut s,
            LoginPart::Password,
            Action::SetText("again".to_owned())
        ),
        Ok(Some(LoginAction::Redraw))
    );
    assert_eq!(s.phase, LoginPhase::PasswordEntry);
    assert_eq!(s.password(), "again");

    s.locked_out = true;
    assert!(!node(&s, LoginPart::Password).unwrap().enabled);
    assert!(!node(&s, LoginPart::SignIn).unwrap().enabled);
    assert!(
        node(&s, LoginPart::Message)
            .unwrap()
            .name
            .starts_with("Too many attempts")
    );
    assert_eq!(
        act(
            &mut s,
            LoginPart::Password,
            Action::SetText("more".to_owned())
        ),
        Err(Refusal::Disabled)
    );
    assert_eq!(
        act(&mut s, LoginPart::SignIn, Action::Press),
        Err(Refusal::Disabled)
    );
    assert_eq!(s.password(), "again", "nothing changed");
}

/// **The back arrow goes back to the accounts, as clicked** -- and is no
/// part where there is only one account.
#[test]
fn the_back_arrow_goes_back_to_the_accounts() {
    let mut s = screen();
    act(&mut s, LoginPart::Account(1), Action::Choose).unwrap();
    assert_eq!(
        act(&mut s, LoginPart::Back, Action::Press),
        Ok(Some(LoginAction::Redraw))
    );
    assert_eq!(s.phase, LoginPhase::UserSelect);

    let mut solo = LoginScreen::new(
        1920.0,
        1080.0,
        vec![LoginUser::new(Some(1), "solo", "Solo")],
    );
    assert!(node(&solo, LoginPart::Back).is_none());
    assert_eq!(
        act(&mut solo, LoginPart::Back, Action::Press),
        Err(Refusal::NoSuchWidget)
    );
}

/// **The bar's buttons open their menus, as clicked: the power choices, each
/// chosen as clicked, and each accessibility setting a switch, switched as
/// clicked, the menu staying up**. While a menu is up, a part under it is
/// refused -- a click there only closes the menu -- and nothing changes.
#[test]
fn the_bars_menus_are_used_as_clicked() {
    let mut s = screen();
    let access = node(&s, LoginPart::Accessibility).expect("the button");
    assert_eq!(access.description.as_deref(), Some("Super+U"));
    assert_eq!(
        node(&s, LoginPart::KeyboardLayout).map(|n| n.name),
        Some("Keyboard layout: US".to_owned())
    );

    act(&mut s, LoginPart::Power, Action::Press).unwrap();
    let menu = node(&s, LoginPart::PowerMenu).expect("up");
    let choices: Vec<&str> = menu.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(choices, ["Shut Down", "Restart", "Sleep", "Hibernate"]);
    assert_eq!(
        act(&mut s, LoginPart::Account(1), Action::Choose),
        Err(Refusal::Hidden),
        "under the menu"
    );
    assert!(
        s.power_menu_open && s.phase == LoginPhase::UserSelect,
        "nothing changed"
    );
    assert_eq!(
        act(&mut s, LoginPart::PowerChoice(1), Action::Press),
        Ok(Some(LoginAction::Power(LoginPowerAction::Reboot)))
    );
    assert!(!s.power_menu_open, "a choice closes it");

    act(&mut s, LoginPart::Accessibility, Action::Press).unwrap();
    let high = LoginPart::Access(LoginAccess::HighContrast);
    let switch = node(&s, high).expect("up");
    assert_eq!(
        (switch.role, switch.name.as_str(), switch.value),
        (
            Role::CheckBox,
            "High contrast",
            Some(Value::Check(CheckState::Unchecked))
        )
    );
    assert_eq!(
        act(&mut s, high, Action::Toggle),
        Ok(Some(LoginAction::Access(LoginAccess::HighContrast)))
    );
    assert!(s.a11y_menu_open, "a switch: the menu stays");
    assert_eq!(
        act(&mut s, LoginPart::Power, Action::Press),
        Err(Refusal::Hidden),
        "the other button, under the menu"
    );
    assert_eq!(
        act(&mut s, LoginPart::PowerChoice(0), Action::Press),
        Err(Refusal::NoSuchWidget),
        "its menu is not up"
    );
}

/// **What is not a part's to do is refused.**
#[test]
fn what_a_part_cannot_do_is_refused() {
    let mut s = screen();
    let refused = |role, action| Err(Refusal::NotApplicable { role, action });
    assert_eq!(
        act(&mut s, LoginPart::Account(0), Action::Toggle),
        refused(Role::ListItem, "toggle")
    );
    assert_eq!(
        act(&mut s, LoginPart::Power, Action::Toggle),
        refused(Role::Button, "toggle")
    );
    assert_eq!(
        act(&mut s, LoginPart::KeyboardLayout, Action::Press),
        refused(Role::Label, "press")
    );
    assert_eq!(
        act(&mut s, LoginPart::Screen, Action::Press),
        refused(Role::Dialog, "press")
    );
    assert_eq!(
        act(&mut s, LoginPart::Account(5), Action::Press),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        act(&mut s, LoginPart::Password, Action::SetText("x".to_owned())),
        Err(Refusal::NoSuchWidget),
        "no field while it asks who you are"
    );
}

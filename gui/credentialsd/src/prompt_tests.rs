//! Tests for the prompt's windows, driven as the window loop drives them:
//! events in, frames out, with no display.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::path::PathBuf;

use credentials::service::{Asking, Choice, Picked, Program, Said, Scope};
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::render::{RenderCommand, RenderTree};
use oswindow::app::{App, Response};

use super::{ARMING, AskApp, AskButton, AskFocus, ChooseApp, ChooseButton, is_hidden, shown};

fn mail() -> Program {
    Program {
        pid: 77,
        exe: PathBuf::from("/usr/bin/mail"),
    }
}

fn asking<'a>(program: &'a Program, target: &'a str, locked: bool) -> Asking<'a> {
    Asking {
        program,
        target,
        username: Some("ann"),
        locked,
        wrong: false,
    }
}

fn key(k: Key, text: &str) -> Event {
    Event::Key(KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: text.to_string(),
    })
}

fn shift_tab() -> Event {
    Event::Key(KeyEvent {
        key: Key::Tab,
        pressed: true,
        modifiers: Modifiers::shift(),
        text: "\t".to_string(),
    })
}

fn mouse(x: f32, y: f32, kind: MouseEventKind) -> Event {
    Event::Mouse(MouseEvent { x, y, kind })
}

/// Let the window arm itself.
fn arm(app: &mut impl App) {
    let ms = u64::try_from(ARMING.as_millis()).unwrap();
    assert_eq!(
        app.on_event(&Event::Tick { elapsed_ms: ms }),
        Response::Redraw
    );
    assert_eq!(app.tick_interval(), None);
}

/// Every string a frame draws.
fn texts(tree: &RenderTree) -> Vec<String> {
    tree.commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn drawn(app: &mut impl App) -> Vec<String> {
    let (w, h) = app.initial_size();
    texts(&app.render(w as f32, h as f32))
}

fn centre(rect: Rect) -> (f32, f32) {
    (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

/// Click the middle of `rect`: a press and a release there.
fn click(app: &mut impl App, rect: Rect) -> Response {
    let (x, y) = centre(rect);
    app.on_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Left)));
    app.on_event(&mouse(x, y, MouseEventKind::Release(MouseButton::Left)))
}

fn ask_button(app: &AskApp, which: AskButton) -> Rect {
    app.drawn
        .iter()
        .find(|(b, _)| *b == which)
        .map(|(_, rect)| *rect)
        .expect("drawn")
}

fn master(said: Option<Said>) -> (Scope, Option<String>) {
    match said {
        Some(Said::Allow { scope, master }) => (
            scope,
            master.map(|m| String::from_utf8(m.as_bytes().to_vec()).unwrap()),
        ),
        other => panic!("not allowed: {other:?}"),
    }
}

/// **The window names the program, where it lives, and what it asks for**:
/// the domain whole, the address beside it, the user name.
#[test]
fn the_window_names_the_program_and_what_it_asks_for() {
    let program = mail();
    let mut app = AskApp::new(&asking(&program, "https://bank.example/login", false));
    let words = drawn(&mut app);
    for want in [
        "mail asks for a password",
        "/usr/bin/mail \u{b7} process 77",
        "bank.example",
        "https://bank.example/login",
        "as ann",
        "Refuse",
        "Allow once",
        "Allow until locked",
    ] {
        assert!(words.iter().any(|w| w == want), "{want:?} in {words:?}");
    }
    assert!(!words.join(" ").contains("master password"));
    // The domain is drawn whole -- never with room to cut it -- and the
    // address beside it may be cut.
    let (w, h) = app.initial_size();
    let tree = app.render(w as f32, h as f32);
    let room_of = |want: &str| {
        tree.commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    text, max_width, ..
                } if text == want => Some(*max_width),
                _ => None,
            })
            .expect("drawn")
    };
    assert_eq!(room_of("bank.example"), None);
    assert!(room_of("https://bank.example/login").is_some());
    assert_eq!(app.title(), "Password request");
    assert_eq!(app.app_id(), "credentials");
    assert!(!app.resizable());
    // A target that is only a domain is not shown twice.
    let mut bare = AskApp::new(&asking(&program, "work-vpn", false));
    let words = drawn(&mut bare);
    assert_eq!(words.iter().filter(|w| *w == "work-vpn").count(), 1);
}

/// **Nothing a program sends is hidden from the user**: what cannot be seen,
/// or would rearrange what can, is spelled out; a domain in letters beyond
/// ASCII is called out.
#[test]
fn nothing_a_program_sends_is_hidden() {
    assert_eq!(shown("bank.example"), "bank.example");
    assert_eq!(
        shown("evil\u{202E}elpmaxe.knab"),
        "evil<U+202E>elpmaxe.knab"
    );
    assert_eq!(shown("a\u{200B}b\u{7}c"), "a<U+200B>b<U+0007>c");
    for c in ['\u{061C}', '\u{2066}', '\u{FEFF}', '\u{E0041}', '\u{00AD}'] {
        assert!(is_hidden(c), "U+{:04X}", u32::from(c));
    }
    for c in ['a', 'é', 'а', ' ', '\u{4E00}'] {
        assert!(!is_hidden(c), "U+{:04X}", u32::from(c));
    }
    let program = mail();
    let mut spoof = AskApp::new(&asking(&program, "https://b\u{0430}nk.example/", false));
    let words = drawn(&mut spoof);
    assert!(words.iter().any(|w| w == "b\u{0430}nk.example"));
    assert!(words.iter().any(|w| w.starts_with("Careful:")));
    let mut plain = AskApp::new(&asking(&program, "https://bank.example/", false));
    assert!(!drawn(&mut plain).iter().any(|w| w.starts_with("Careful:")));
    let mut odd = AskApp::new(&Asking {
        username: Some("ann\u{202E}"),
        ..asking(&program, "bank.example", false)
    });
    assert!(drawn(&mut odd).iter().any(|w| w == "as ann<U+202E>"));
}

/// **Nothing is taken before the window is armed** -- no key, no click --
/// but Escape, which refuses at once.
#[test]
fn nothing_is_taken_before_the_window_is_armed() {
    let program = mail();
    let mut app = AskApp::new(&asking(&program, "bank.example", false));
    drawn(&mut app);
    assert_eq!(app.tick_interval(), Some(super::ARMING_TICK));
    assert_eq!(app.on_event(&key(Key::Enter, "\r")), Response::Idle);
    assert_eq!(app.on_event(&key(Key::Tab, "\t")), Response::Idle);
    let once = ask_button(&app, AskButton::Once);
    assert_eq!(click(&mut app, once), Response::Idle);
    assert!(app.take_said().is_none());
    // Partly armed is not armed.
    assert_eq!(app.on_event(&Event::Tick { elapsed_ms: 1 }), Response::Idle);
    assert_eq!(app.on_event(&key(Key::Escape, "\u{1b}")), Response::Exit);
    assert!(matches!(app.take_said(), Some(Said::Refuse)));
}

/// **With the vault locked, allowing takes the master password typed** --
/// and an allow with nothing typed sends the keyboard to the field instead.
#[test]
fn allowing_a_locked_vault_takes_the_password_typed() {
    let program = mail();
    let mut app = AskApp::new(&asking(&program, "bank.example", true));
    let words = drawn(&mut app);
    assert!(words.join(" ").contains("Type its master password"));
    arm(&mut app);
    assert_eq!(app.focus, AskFocus::Field);
    // Enter with nothing typed does nothing.
    assert_eq!(app.on_event(&key(Key::Enter, "\r")), Response::Idle);
    // Allow once, pressed from the keyboard with nothing typed.
    app.on_event(&key(Key::Tab, "\t"));
    app.on_event(&key(Key::Tab, "\t"));
    assert_eq!(app.focus, AskFocus::On(AskButton::Once));
    assert_eq!(app.on_event(&key(Key::Space, " ")), Response::Redraw);
    assert_eq!(app.focus, AskFocus::Field);
    for c in "hunter2".chars() {
        assert_eq!(
            app.on_event(&key(Key::Unknown(0), &c.to_string())),
            Response::Redraw
        );
    }
    let words = drawn(&mut app);
    assert!(!words.iter().any(|w| w.contains("hunter2")));
    assert!(words.iter().any(|w| *w == "\u{2022}".repeat(7)));
    assert_eq!(app.on_event(&key(Key::Enter, "\r")), Response::Exit);
    assert_eq!(
        master(app.take_said()),
        (Scope::Once, Some("hunter2".into()))
    );
    // Until locked, by a click, with the password typed.
    let mut app = AskApp::new(&asking(&program, "bank.example", true));
    drawn(&mut app);
    arm(&mut app);
    for c in "pw".chars() {
        app.on_event(&key(Key::Unknown(0), &c.to_string()));
    }
    let until = ask_button(&app, AskButton::UntilLocked);
    assert_eq!(click(&mut app, until), Response::Exit);
    assert_eq!(
        master(app.take_said()),
        (Scope::UntilLocked, Some("pw".into()))
    );
}

/// **With the vault open, the keyboard starts on Refuse**, so a stray Enter
/// refuses; the arrows and Tab go round the buttons.
#[test]
fn unlocked_the_keyboard_starts_on_refuse() {
    let program = mail();
    let mut app = AskApp::new(&asking(&program, "bank.example", false));
    drawn(&mut app);
    arm(&mut app);
    assert_eq!(app.focus, AskFocus::On(AskButton::Refuse));
    assert_eq!(app.on_event(&key(Key::Enter, "\r")), Response::Exit);
    assert!(matches!(app.take_said(), Some(Said::Refuse)));

    let mut app = AskApp::new(&asking(&program, "bank.example", false));
    drawn(&mut app);
    arm(&mut app);
    app.on_event(&key(Key::Right, ""));
    assert_eq!(app.focus, AskFocus::On(AskButton::Once));
    app.on_event(&shift_tab());
    assert_eq!(app.focus, AskFocus::On(AskButton::Refuse));
    app.on_event(&shift_tab());
    assert_eq!(app.focus, AskFocus::On(AskButton::UntilLocked));
    app.on_event(&key(Key::Tab, "\t"));
    app.on_event(&key(Key::Tab, "\t"));
    assert_eq!(app.focus, AskFocus::On(AskButton::Once));
    // Typing with no field to type in does nothing.
    assert_eq!(app.on_event(&key(Key::A, "a")), Response::Idle);
    assert_eq!(app.on_event(&key(Key::Space, " ")), Response::Exit);
    assert_eq!(master(app.take_said()), (Scope::Once, None));
}

/// **A click is a press and a release on the same button**: let go
/// elsewhere, and nothing is pressed.
#[test]
fn a_click_is_a_press_and_a_release_on_one_button() {
    let program = mail();
    let mut app = AskApp::new(&asking(&program, "bank.example", false));
    drawn(&mut app);
    arm(&mut app);
    let once = ask_button(&app, AskButton::Once);
    let refuse = ask_button(&app, AskButton::Refuse);
    let (x, y) = centre(once);
    assert_eq!(
        app.on_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Left))),
        Response::Redraw
    );
    let (rx, ry) = centre(refuse);
    assert_eq!(
        app.on_event(&mouse(rx, ry, MouseEventKind::Release(MouseButton::Left))),
        Response::Redraw
    );
    assert!(app.take_said().is_none());
    // Hovering is drawn; leaving forgets it.
    assert_eq!(
        app.on_event(&mouse(x, y, MouseEventKind::Move)),
        Response::Redraw
    );
    assert_eq!(
        app.on_event(&mouse(x, y, MouseEventKind::Move)),
        Response::Idle
    );
    assert_eq!(
        app.on_event(&mouse(x, y, MouseEventKind::Leave)),
        Response::Redraw
    );
    assert_eq!(click(&mut app, refuse), Response::Exit);
    assert!(matches!(app.take_said(), Some(Said::Refuse)));
}

/// **Closing the window refuses**, and a wrong master password says so.
#[test]
fn closing_refuses_and_a_wrong_password_says_so() {
    let program = mail();
    let mut app = AskApp::new(&asking(&program, "bank.example", false));
    assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    assert!(matches!(app.take_said(), Some(Said::Refuse)));
    let mut wrong = AskApp::new(&Asking {
        wrong: true,
        ..asking(&program, "bank.example", true)
    });
    assert!(
        drawn(&mut wrong)
            .iter()
            .any(|w| w == "That is not the master password.")
    );
    // Asking for the password makes the window taller.
    let open = AskApp::new(&asking(&program, "bank.example", false));
    let locked = AskApp::new(&asking(&program, "bank.example", true));
    assert_eq!(open.initial_size().0, 480);
    assert!(locked.initial_size().1 > open.initial_size().1);
}

fn choices() -> Vec<(String, String)> {
    (0..10)
        .map(|n| (format!("https://example.com/{n}"), format!("user{n}")))
        .collect()
}

fn choose_app(program: &Program, rows: &[(String, String)]) -> ChooseApp {
    let offered: Vec<Choice<'_>> = rows
        .iter()
        .map(|(target, username)| Choice { target, username })
        .collect();
    ChooseApp::new(&asking(program, "https://example.com", false), &offered)
}

fn choose_button(app: &ChooseApp, which: ChooseButton) -> Rect {
    app.drawn
        .iter()
        .find(|(b, _)| *b == which)
        .map(|(_, rect)| *rect)
        .expect("drawn")
}

/// **Choosing lists the logins by user name and address**, gives nothing
/// until one is picked, and gives the one picked.
#[test]
fn choosing_lists_the_logins_and_gives_the_one_picked() {
    let program = mail();
    let rows = choices()[..3].to_vec();
    let mut app = choose_app(&program, &rows);
    let words = drawn(&mut app);
    for want in [
        "Which password may mail have?",
        "user0",
        "https://example.com/2",
    ] {
        assert!(words.iter().any(|w| w == want), "{want:?} in {words:?}");
    }
    arm(&mut app);
    let give = choose_button(&app, ChooseButton::Give);
    assert_eq!(click(&mut app, give), Response::Idle);
    assert_eq!(app.on_event(&key(Key::Enter, "\r")), Response::Idle);
    assert_eq!(app.on_event(&key(Key::Down, "")), Response::Redraw);
    assert_eq!(app.on_event(&key(Key::Down, "")), Response::Redraw);
    assert_eq!(app.on_event(&key(Key::Up, "")), Response::Redraw);
    assert_eq!(app.on_event(&key(Key::Enter, "\r")), Response::Exit);
    assert_eq!(app.take_picked(), Some(Picked::Login(0)));
    // A click on a row picks it.
    let mut app = choose_app(&program, &rows);
    drawn(&mut app);
    arm(&mut app);
    let list = app.list_at.unwrap();
    let row = Rect::new(
        list.x,
        list.y + super::ROW_HEIGHT * 2.0,
        list.w,
        super::ROW_HEIGHT,
    );
    let (x, y) = centre(row);
    assert_eq!(
        app.on_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Left))),
        Response::Redraw
    );
    assert_eq!(app.selected, Some(2));
    let give = choose_button(&app, ChooseButton::Give);
    assert_eq!(click(&mut app, give), Response::Exit);
    assert_eq!(app.take_picked(), Some(Picked::Login(2)));
}

/// **Choosing none -- by its button, Escape, or closing -- gives nothing**,
/// and nothing is taken before the window is armed.
#[test]
fn choosing_none_gives_nothing() {
    let program = mail();
    let rows = choices()[..2].to_vec();
    let mut app = choose_app(&program, &rows);
    drawn(&mut app);
    assert_eq!(app.on_event(&key(Key::Down, "")), Response::Idle);
    let none = choose_button(&app, ChooseButton::Nothing);
    assert_eq!(click(&mut app, none), Response::Idle);
    arm(&mut app);
    assert_eq!(click(&mut app, none), Response::Exit);
    assert_eq!(app.take_picked(), Some(Picked::Nothing));
    let mut app = choose_app(&program, &rows);
    assert_eq!(app.on_event(&key(Key::Escape, "\u{1b}")), Response::Exit);
    assert_eq!(app.take_picked(), Some(Picked::Nothing));
    let mut app = choose_app(&program, &rows);
    assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    assert_eq!(app.take_picked(), Some(Picked::Nothing));
}

/// **A long list scrolls**, by the keyboard and the wheel, keeping the one
/// picked in view and never past its end.
#[test]
fn a_long_list_scrolls() {
    let program = mail();
    let rows = choices();
    let mut app = choose_app(&program, &rows);
    drawn(&mut app);
    arm(&mut app);
    assert_eq!(app.on_event(&key(Key::End, "")), Response::Redraw);
    assert_eq!((app.selected, app.scrolled), (Some(9), 4));
    let words = drawn(&mut app);
    assert!(words.iter().any(|w| w == "user9"));
    assert!(!words.iter().any(|w| w == "user3"));
    let list = app.list_at.unwrap();
    let (x, y) = centre(list);
    let wheel = |dy: f32| mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy });
    assert_eq!(app.on_event(&wheel(-1.0)), Response::Idle);
    assert_eq!(app.on_event(&wheel(1.0)), Response::Redraw);
    assert_eq!(app.scrolled, 3);
    assert_eq!(app.on_event(&key(Key::Home, "")), Response::Redraw);
    assert_eq!((app.selected, app.scrolled), (Some(0), 0));
    assert_eq!(app.on_event(&wheel(1.0)), Response::Idle);
    // A taller list for more logins, up to a point.
    let few = choose_app(&program, &rows[..2]);
    let many = choose_app(&program, &rows);
    let most = choose_app(&program, &[rows.clone(), rows.clone()].concat());
    assert!(many.initial_size().1 > few.initial_size().1);
    assert_eq!(many.initial_size().1, most.initial_size().1);
}

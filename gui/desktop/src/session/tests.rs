//! What a live shell does, against a compositor that is not there.
//!
//! `oswindow::testing::desktop()` gives an [`EventLoop`] wired to a
//! [`TestDesktop`] over a loopback pipe: requests get real answers, input can
//! be pushed at the client, and every frame the client draws is recorded. That
//! is enough to assert the whole path — a press arrives on a surface, the shell
//! decides what it means, and a request goes back out — with no compositor, no
//! display and no window system.

// A test that indexes past the end, or unwraps a `None`, is a test that has
// caught the thing it was watching for; panicking there is the report, not a
// defect. The same allow-list the crate's other test modules carry.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::cell::RefCell;
use std::rc::Rc;

use appearance::AccentColor;
use guiremote::control::{RequestBody, ShellControlAction};
use guiremote::window_list::WindowInfo;
use guitk::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEventKind, SettingsGroup};
use guitk::render::RenderCommand;
use oswindow::InputEvent;
use oswindow::testing::{TestConnection, TestDesktop, desktop as wired};

use super::{ShellSession, Surface};
use crate::{DesktopShell, Hit, Rect, click};

type Session = ShellSession<TestConnection>;
type Desktop = Rc<RefCell<TestDesktop>>;

/// A started session and the compositor behind it.
///
/// `TestDesktop` hands out ids from 100, so the four surfaces are 100
/// (background), 101 (panel), 102 (popups) and 103 (osd), in the order `start`
/// creates them.
fn session() -> (Session, Desktop) {
    let (events, desktop) = wired();
    let session = ShellSession::start(events).expect("the harness refused a surface");
    (session, desktop)
}

/// Deliver a press at a point in *screen* coordinates, through the surface that
/// would really have received it.
///
/// The translation under test is deliberately not done here — the point is
/// converted the way the *compositor* would convert it, by subtracting the
/// surface's origin, so that a session which failed to add it back would see
/// the wrong point.
fn press_at(desktop: &Desktop, surface: Surface, x: f32, y: f32) {
    let (ox, oy) = surface.origin();
    desktop.borrow_mut().send_input(&[InputEvent::new(
        surface.window(),
        guitk::event::Event::Mouse(click(x - ox, y - oy)),
    )]);
}

fn key(k: Key) -> guitk::event::Event {
    guitk::event::Event::Key(KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers::default(),
        text: String::new(),
    })
}

/// Super+N — the chord that opens and closes the notification pane.
fn super_n() -> guitk::event::Event {
    chord(
        Key::N,
        Modifiers {
            super_key: true,
            ..Modifiers::NONE
        },
    )
}

/// A press with modifiers held — a desktop shortcut rather than a bare key.
fn chord(k: Key, modifiers: Modifiers) -> guitk::event::Event {
    guitk::event::Event::Key(KeyEvent {
        key: k,
        pressed: true,
        modifiers,
        text: String::new(),
    })
}

/// Every `ShellControl` the session sent, in order.
fn controls(desktop: &Desktop) -> Vec<(u64, ShellControlAction)> {
    desktop
        .borrow()
        .seen
        .iter()
        .filter_map(|r| match r.body {
            RequestBody::ShellControl { window, action } => Some((window, action)),
            _ => None,
        })
        .collect()
}

/// An ordinary application window, as the compositor would list it.
fn app(id: u64, title: &str) -> WindowInfo {
    WindowInfo::new(id, 2000 + id, title)
}

/// Every `CreateWindow` spec the session sent, in order.
fn created(desktop: &Desktop) -> Vec<guiremote::control::WindowSpec> {
    desktop
        .borrow()
        .seen
        .iter()
        .filter_map(|r| match &r.body {
            RequestBody::CreateWindow(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// The centre of a rectangle.
fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

// ---- the surfaces ----

#[test]
fn the_shell_opens_a_background_a_panel_a_menu_and_an_overlay_surface() {
    let (session, desktop) = session();
    let specs = created(&desktop);
    assert_eq!(specs.len(), 4, "a shell is four surfaces, not one");

    // The band each one is in is the load-bearing part: a taskbar in
    // `Layer::Normal` vanishes behind the first window the user opens.
    assert_eq!(specs[0].layer, oswindow::Layer::Background);
    assert_eq!(specs[1].layer, oswindow::Layer::Overlay);
    assert_eq!(specs[2].layer, oswindow::Layer::Overlay);
    assert_eq!(specs[3].layer, oswindow::Layer::Overlay);

    // None of them is a window in the ordinary sense.
    for spec in &specs {
        assert!(!spec.decorations, "{} has a title bar", spec.title);
        assert!(
            !spec.resizable,
            "{} can be dragged to a new size",
            spec.title
        );
    }

    // The panel is exactly the taskbar; the other three are the whole display.
    let bar = session.shell().taskbar_rect();
    assert_eq!(specs[1].position, Some((bar.x as i32, bar.y as i32)));
    assert_eq!(specs[1].width, bar.w.round() as u32);
    assert_eq!(specs[1].height, bar.h.round() as u32);
    assert_eq!((specs[0].width, specs[0].height), (2560, 1440));
    assert_eq!((specs[2].width, specs[2].height), (2560, 1440));
    assert_eq!((specs[3].width, specs[3].height), (2560, 1440));

    // And the origins the session will translate by say the same thing.
    assert_eq!(session.background().origin(), (0.0, 0.0));
    assert_eq!(session.panel().origin(), (bar.x, bar.y));
    assert_eq!(session.popups().origin(), (0.0, 0.0));
    assert_eq!(session.osd().origin(), (0.0, 0.0));
}

/// Exactly one of the shell's surfaces declines the mouse, and it is the one
/// whose job is to be looked at rather than clicked.
///
/// Both halves matter. A clickable OSD is a full-screen sheet that eats the
/// press aimed at the document under a volume indicator; a click-through
/// *taskbar* is a taskbar whose buttons do nothing, and the taskbar is already
/// `transparent`, so this is the assertion that keeps the two flags apart. See
/// `design-decisions.md` 566.
#[test]
fn only_the_overlay_surface_refuses_the_mouse() {
    let (_session, desktop) = session();
    let specs = created(&desktop);
    let click_through: Vec<&str> = specs
        .iter()
        .filter(|s| s.input_transparent)
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(click_through, ["Shell overlays"]);
    // And it is the last one created, so it is above the menus: an overlay is a
    // report, and a report under the start menu is a report nobody reads.
    assert!(specs[3].input_transparent);
}

#[test]
fn the_shell_asks_to_be_told_about_windows_it_does_not_own() {
    let (_session, desktop) = session();
    assert!(
        desktop
            .borrow()
            .seen
            .iter()
            .any(|r| r.body == RequestBody::SubscribeWindowList { subscribe: true }),
        "a taskbar that never subscribes is a taskbar that is always empty"
    );
}

/// Without this the shell hears a shortcut only while it is itself focused,
/// which for a taskbar is almost never — and Alt+Tab, which exists to be pressed
/// from inside another window, never at all. The grab list is checked against
/// the binding table elsewhere; what is checked here is that the session asks
/// for it, on a surface that stays mapped.
#[test]
fn the_shell_claims_its_shortcuts_before_it_starts_listening() {
    let (session, desktop) = session();
    let panel = session.panel().window();
    let seen = &desktop.borrow().seen;
    for (key, modifiers) in session.shell().global_chords() {
        assert!(
            seen.iter().any(|r| r.body
                == RequestBody::GrabKey {
                    window: panel,
                    key,
                    modifiers
                }),
            "{key:?} with {modifiers:?} is bound but never claimed, so it is dead \
             in every window but the shell's own"
        );
    }
}

/// Escape is the one chord held conditionally: a permanent grab takes the key
/// from every dialog on the desktop, and no grab means a menu opened with the
/// mouse cannot be closed with the keyboard.
#[test]
fn escape_is_claimed_while_a_menu_is_open_and_given_back_after() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    let chords = session.shell().conditional_chords();
    assert!(
        !chords.is_empty(),
        "no chord dismisses a popup, so a menu opened with the mouse cannot be \
         closed with the keyboard at all"
    );
    let grabs: Vec<_> = chords
        .iter()
        .map(|&(key, modifiers)| RequestBody::GrabKey {
            window: panel,
            key,
            modifiers,
        })
        .collect();
    let ungrabs: Vec<_> = chords
        .iter()
        .map(|&(key, modifiers)| RequestBody::UngrabKey {
            window: panel,
            key,
            modifiers,
        })
        .collect();
    for grab in &grabs {
        assert!(
            !desktop.borrow().seen.iter().any(|r| &r.body == grab),
            "Escape was taken from the whole desktop before anything was open"
        );
    }

    let start = centre(session.shell().start_button_rect());
    press_at(&desktop, session.panel(), start.0, start.1);
    session.pump().expect("pump");
    assert!(session.shell().start_menu_open);
    for grab in &grabs {
        assert!(
            desktop.borrow().seen.iter().any(|r| &r.body == grab),
            "the start menu is open and Escape still goes to whoever has the keyboard"
        );
    }

    session.shell_mut().dismiss_popups();
    session.pump().expect("pump");
    for ungrab in &ungrabs {
        assert!(
            desktop.borrow().seen.iter().any(|r| &r.body == ungrab),
            "the menu closed and the shell kept Escape"
        );
    }
}

#[test]
fn the_menu_surface_is_unmapped_while_no_menu_is_open() {
    let (mut session, desktop) = session();
    let popups = session.popups().window();

    // Mapped at creation, so the session's first paint has to take it away
    // again — otherwise an invisible full-screen sheet swallows every click on
    // an idle desktop.
    assert!(
        desktop.borrow().seen.iter().any(|r| r.body
            == RequestBody::SetVisible {
                window: popups,
                visible: false
            }),
        "the menu surface was left covering the desktop"
    );

    let start = centre(session.shell().start_button_rect());
    press_at(&desktop, session.panel(), start.0, start.1);
    session.pump().expect("pump");
    assert!(session.shell().start_menu_open);
    assert!(
        desktop.borrow().seen.iter().any(|r| r.body
            == RequestBody::SetVisible {
                window: popups,
                visible: true
            }),
        "the start menu opened with nothing to draw it on"
    );
}

#[test]
fn closing_the_last_menu_takes_the_surface_away_again() {
    let (mut session, desktop) = session();
    let popups = session.popups().window();
    let start = centre(session.shell().start_button_rect());

    press_at(&desktop, session.panel(), start.0, start.1);
    session.pump().expect("pump");
    let before = desktop.borrow().seen.len();

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(popups, key(Key::Escape))]);
    session.pump().expect("pump");

    assert!(!session.shell().start_menu_open, "Escape did not close it");
    assert!(
        desktop.borrow().seen[before..].iter().any(|r| r.body
            == RequestBody::SetVisible {
                window: popups,
                visible: false
            }),
        "the menu closed but its surface stayed up"
    );
}

// ---- the two coordinate spaces ----

#[test]
fn a_point_that_hits_an_element_is_a_point_that_element_was_drawn_at() {
    // The whole reason `Surface` exists. Input runs one way — window-local to
    // screen, because that is the space the shell hit-tests in — and rendering
    // runs the other. Either alone looks right in a unit test; only together do
    // they mean that the thing under the pointer is the thing the user is
    // looking at. So this asserts the round trip, on a surface whose origin is
    // not the screen's.
    let shell = DesktopShell::new(2560, 1440);
    let bar = shell.taskbar_rect();
    let panel = Surface {
        window: 101,
        origin: (bar.x, bar.y),
    };
    assert!(
        bar.y > 0.0,
        "a taskbar at the top of the screen would make this test vacuous"
    );

    let (sx, sy) = centre(shell.start_button_rect());
    // What the compositor would deliver for a press at that screen point.
    let local = click(sx - bar.x, sy - bar.y);

    // Direction one: the shell understands it as the start button.
    let screen = panel.to_screen(&local);
    assert_eq!(shell.hit_test(screen.x, screen.y), Hit::StartButton);

    // Direction two: whatever the shell drew over that screen point is drawn,
    // in the surface's own coordinates, over the point the compositor sent.
    let tree = shell.render_taskbar();
    let covering = tree
        .commands
        .iter()
        .find_map(|cmd| match *cmd {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } if Rect::new(x, y, width, height).contains(sx, sy) => {
                Some(Rect::new(x, y, width, height))
            }
            _ => None,
        })
        .expect("the taskbar drew nothing under its own start button");

    let localized = panel.localize(&tree);
    let RenderCommand::PushTranslate { dx, dy } = localized.commands[0] else {
        panic!("a localized tree must open with the translation that localizes it");
    };
    assert!(
        Rect::new(covering.x + dx, covering.y + dy, covering.w, covering.h)
            .contains(local.x, local.y),
        "drawn at ({}, {}) but clicked at ({}, {}) — the two translations disagree",
        covering.x + dx,
        covering.y + dy,
        local.x,
        local.y
    );
}

#[test]
fn localizing_preserves_every_command_and_wraps_them_in_one_translation() {
    let surface = Surface {
        window: 7,
        origin: (12.0, 34.0),
    };
    let mut tree = guitk::render::RenderTree::new();
    tree.fill_rect(0.0, 0.0, 5.0, 5.0, guitk::color::Color::WHITE);
    tree.fill_rect(1.0, 2.0, 3.0, 4.0, guitk::color::Color::BLACK);

    let out = surface.localize(&tree);
    assert_eq!(out.commands.len(), tree.commands.len() + 2);
    assert!(matches!(
        out.commands[0],
        RenderCommand::PushTranslate {
            dx: -12.0,
            dy: -34.0
        }
    ));
    assert!(matches!(
        out.commands[out.commands.len() - 1],
        RenderCommand::PopTranslate
    ));
    // The commands themselves are untouched — the translation is a transform
    // the renderer applies, not a rewrite of what was drawn. (`RenderCommand`
    // is not `PartialEq`, so this compares the fields that would move.)
    let corners: Vec<(f32, f32)> = out.commands[1..=2]
        .iter()
        .filter_map(|c| match *c {
            RenderCommand::FillRect { x, y, .. } => Some((x, y)),
            _ => None,
        })
        .collect();
    assert_eq!(corners, [(0.0, 0.0), (1.0, 2.0)]);
}

#[test]
fn a_surface_at_the_screens_origin_is_translated_the_same_way_as_any_other() {
    // Not "skipped because it is a no-op". The identity case going down a
    // different path is precisely how the two directions get to disagree
    // without any test noticing.
    let surface = Surface {
        window: 100,
        origin: (0.0, 0.0),
    };
    let mut tree = guitk::render::RenderTree::new();
    tree.fill_rect(3.0, 4.0, 5.0, 6.0, guitk::color::Color::WHITE);
    let out = surface.localize(&tree);
    assert!(matches!(
        out.commands[0],
        RenderCommand::PushTranslate { dx: 0.0, dy: 0.0 }
    ));
    assert_eq!(out.commands.len(), 3);
}

#[test]
fn a_press_on_the_panel_is_understood_where_the_user_pressed() {
    // The end-to-end form of the round trip above: the point goes over the wire
    // in the compositor's space and has to come out in the shell's.
    let (mut session, desktop) = session();
    let clock = centre(session.shell().clock_rect());
    press_at(&desktop, session.panel(), clock.0, clock.1);
    session.pump().expect("pump");
    assert!(
        session.shell().calendar.visible,
        "the tray clock did not open the calendar — the press landed elsewhere"
    );
}

// ---- the window list, and acting on it ----

#[test]
fn the_compositors_window_list_is_what_the_taskbar_is_drawn_from() {
    let (mut session, desktop) = session();
    assert!(session.shell().taskbar_windows().is_empty());

    desktop
        .borrow_mut()
        .send_window_list(&[app(1, "Terminal"), app(2, "notes.txt")]);
    session.pump().expect("pump");

    let titles: Vec<&str> = session
        .shell()
        .taskbar_windows()
        .iter()
        .map(|w| w.title.as_str())
        .collect();
    assert_eq!(titles, ["Terminal", "notes.txt"]);
}

#[test]
fn a_taskbar_button_asks_the_compositor_rather_than_changing_anything() {
    let (mut session, desktop) = session();
    desktop
        .borrow_mut()
        .send_window_list(&[app(1, "Terminal"), app(2, "notes.txt")]);
    session.pump().expect("pump");

    let button = centre(session.shell().taskbar_button_rect(1));
    press_at(&desktop, session.panel(), button.0, button.1);
    session.pump().expect("pump");

    assert!(
        desktop.borrow().seen.iter().any(|r| r.body
            == RequestBody::ShellControl {
                window: 2,
                action: ShellControlAction::Activate
            }),
        "the click never reached the compositor"
    );
    // And the shell did *not* act on its own: the window is still exactly what
    // the last list said it was.
    assert_eq!(session.shell().focused_window, None);
}

#[test]
fn a_second_press_on_the_focused_windows_button_asks_for_it_to_be_minimised() {
    let (mut session, desktop) = session();
    let mut focused = app(2, "notes.txt");
    focused.focused = true;
    desktop
        .borrow_mut()
        .send_window_list(&[app(1, "Terminal"), focused]);
    session.pump().expect("pump");

    let button = centre(session.shell().taskbar_button_rect(1));
    press_at(&desktop, session.panel(), button.0, button.1);
    session.pump().expect("pump");

    assert!(
        desktop.borrow().seen.iter().any(|r| r.body
            == RequestBody::ShellControl {
                window: 2,
                action: ShellControlAction::Minimize
            }),
        "a click on the focused window's button should put it away"
    );
}

/// A window rule reaches the compositor, which is the only way a rule can do
/// anything at all.
///
/// `apply_window_list` decides what a rule asks for but holds no connection, so
/// it hands the requests back and the session sends them. Between those two
/// halves is the seam this covers: the rules engine sat in the tree for 2,600
/// lines with nothing calling it, and a shell that evaluated rules and then
/// dropped the answer on the floor would look exactly the same from either end.
#[test]
fn a_window_rule_about_an_arriving_window_reaches_the_compositor() {
    let (mut session, desktop) = session();
    let mut rule = crate::window_rules::WindowRule::new(
        0,
        "editors start maximised",
        crate::window_rules::MatchCriteria::AppId("slateos-editor".to_string()),
    );
    rule.actions.initial_state = Some(crate::window_rules::InitialState::Maximized);
    assert!(session.shell_mut().rules.add_rule(rule).is_some());

    let mut editor = app(1, "notes.md");
    editor.app_id = "slateos-editor".to_string();
    desktop.borrow_mut().send_window_list(&[editor.clone()]);
    session.pump().expect("pump");

    assert_eq!(
        controls(&desktop),
        vec![(1, ShellControlAction::Maximize)],
        "the rule never left the shell"
    );

    // And it is not asked again on the next frame: "initial state" is the
    // state it starts in, so a user who un-maximises it must be able to.
    desktop.borrow_mut().send_window_list(&[editor]);
    session.pump().expect("pump");
    assert_eq!(
        controls(&desktop).len(),
        1,
        "the rule fired a second time on an unchanged window"
    );
}

/// A window nothing matches costs nothing.
///
/// The rules engine is consulted for every window that arrives, so the
/// no-rules case is the common one and must be silent — a shell that sent
/// something per window would recomposite the desktop on every launch.
#[test]
fn an_arriving_window_no_rule_matches_asks_for_nothing() {
    let (mut session, desktop) = session();
    desktop
        .borrow_mut()
        .send_window_list(&[app(1, "Terminal"), app(2, "notes.txt")]);
    session.pump().expect("pump");
    assert!(controls(&desktop).is_empty());
}

/// The keyboard half of the same rule the taskbar obeys.
///
/// Alt+F4 used to call the shell's own `remove_window`, which dropped the
/// taskbar button and left the program running: the compositor was never told,
/// and the next window list put the button straight back. The shortcut has to
/// leave the session as a request like any other.
#[test]
fn alt_f4_asks_the_compositor_to_close_the_focused_window() {
    let (mut session, desktop) = session();
    let mut focused = app(2, "notes.txt");
    focused.focused = true;
    desktop
        .borrow_mut()
        .send_window_list(&[app(1, "Terminal"), focused]);
    session.pump().expect("pump");

    desktop.borrow_mut().send_input(&[InputEvent::new(
        session.background().window(),
        chord(Key::F4, Modifiers::alt()),
    )]);
    session.pump().expect("pump");

    assert_eq!(
        controls(&desktop),
        [(2, ShellControlAction::Close)],
        "Alt+F4 never reached the compositor"
    );
    // And the shell has not pretended: the window it was told about is still
    // there, because the only thing that removes it is the next list.
    assert_eq!(session.shell().taskbar_windows().len(), 2);
}

/// The shortcut that names every window at once — the reason a shortcut's
/// outcome carries a list rather than one request.
#[test]
fn super_d_asks_for_every_window_to_be_minimised() {
    let (mut session, desktop) = session();
    desktop.borrow_mut().send_window_list(&[
        app(1, "Terminal"),
        app(2, "notes.txt"),
        app(3, "mail"),
    ]);
    session.pump().expect("pump");

    desktop.borrow_mut().send_input(&[InputEvent::new(
        session.background().window(),
        chord(
            Key::D,
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ),
    )]);
    session.pump().expect("pump");

    assert_eq!(
        controls(&desktop),
        [
            (1, ShellControlAction::Minimize),
            (2, ShellControlAction::Minimize),
            (3, ShellControlAction::Minimize),
        ],
        "one press has to reach every window, not just the focused one"
    );
}

/// A shortcut aimed at a window that has just closed is the same race a taskbar
/// click is, and — for Super+D — must not stop the rest of the batch.
#[test]
fn a_refused_shortcut_does_not_swallow_the_rest_of_the_batch() {
    let (mut session, desktop) = session();
    desktop
        .borrow_mut()
        .send_window_list(&[app(1, "Terminal"), app(2, "notes.txt")]);
    session.pump().expect("pump");

    desktop.borrow_mut().refuse = Some("no such window".to_owned());
    desktop.borrow_mut().send_input(&[InputEvent::new(
        session.background().window(),
        chord(
            Key::D,
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ),
    )]);
    session
        .pump()
        .expect("a refused shell control must not bring the desktop down");

    assert_eq!(
        controls(&desktop).len(),
        2,
        "the second window was skipped because the first one was refused"
    );
}

/// Super+Right tiles the focused window by *naming the edge*. The shell must not
/// compute the rectangle — the compositor knows the work area, and a shell that
/// guessed would disagree with it the moment a monitor changed.
#[test]
fn super_right_asks_for_a_tile_and_computes_no_geometry() {
    let (mut session, desktop) = session();
    let mut focused = app(1, "Terminal");
    focused.focused = true;
    desktop.borrow_mut().send_window_list(&[focused]);
    session.pump().expect("pump");

    desktop.borrow_mut().send_input(&[InputEvent::new(
        session.background().window(),
        chord(
            Key::Right,
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ),
    )]);
    session.pump().expect("pump");

    // One request, naming an edge and no pixels. There is no protocol verb at
    // all that would let the shell send a rectangle for a window it does not
    // own — see `ShellControlAction` — so this asserts what *is* sent and the
    // absence of the alternative is structural.
    assert_eq!(controls(&desktop), [(1, ShellControlAction::SnapRight)]);
}

#[test]
fn a_refused_control_request_is_a_race_and_not_a_failure() {
    // The window closed between the list the button was drawn from and the
    // click. A shell that treated that as fatal would exit the desktop.
    let (mut session, desktop) = session();
    desktop.borrow_mut().send_window_list(&[app(1, "Terminal")]);
    session.pump().expect("pump");

    desktop.borrow_mut().refuse = Some("no such window".to_owned());
    let button = centre(session.shell().taskbar_button_rect(0));
    press_at(&desktop, session.panel(), button.0, button.1);
    session
        .pump()
        .expect("a refused shell control must not bring the desktop down");
}

#[test]
fn a_window_list_arriving_with_a_click_is_folded_in_after_it() {
    // `poll` is also what reads the window list off the wire, so a list can be
    // sitting in the connection by the time the click is handled. The click was
    // aimed at the picture the *old* list produced, and answering it against
    // the new one would minimise whichever window had inherited the slot.
    let (mut session, desktop) = session();
    desktop
        .borrow_mut()
        .send_window_list(&[app(1, "Terminal"), app(2, "notes.txt")]);
    session.pump().expect("pump");

    let button = centre(session.shell().taskbar_button_rect(1));
    {
        let mut d = desktop.borrow_mut();
        // Both in flight at once, the list first — the worst ordering for a
        // shell that folds it in too early.
        d.send_window_list(&[app(7, "something else")]);
        let (ox, oy) = session.panel().origin();
        d.send_input(&[InputEvent::new(
            session.panel().window(),
            guitk::event::Event::Mouse(click(button.0 - ox, button.1 - oy)),
        )]);
    }
    session.pump().expect("pump");

    assert!(
        desktop.borrow().seen.iter().any(|r| r.body
            == RequestBody::ShellControl {
                window: 2,
                action: ShellControlAction::Activate
            }),
        "the click acted on the desktop it was not aimed at"
    );
    // The new list still arrives — it is applied, just second.
    let titles: Vec<&str> = session
        .shell()
        .taskbar_windows()
        .iter()
        .map(|w| w.title.as_str())
        .collect();
    assert_eq!(titles, ["something else"]);
}

// ---- the intents this loop cannot carry out ----

#[test]
fn a_start_menu_row_comes_out_as_a_program_to_start() {
    let (mut session, desktop) = session();
    let start = centre(session.shell().start_button_rect());
    press_at(&desktop, session.panel(), start.0, start.1);
    session.pump().expect("pump");

    let row = centre(session.shell().start_menu_row_rect(0));
    press_at(&desktop, session.popups(), row.0, row.1);
    session.pump().expect("pump");

    let launched = session.take_launches();
    assert_eq!(launched.len(), 1, "expected one program, got {launched:?}");
    assert!(
        launched[0].starts_with("/"),
        "a launch should be a path, not {:?}",
        launched[0]
    );
    assert!(
        session.take_launches().is_empty(),
        "taking the launches twice must not hand out the same program twice"
    );
}

#[test]
fn an_ordinary_press_produces_no_launch() {
    let (mut session, desktop) = session();
    press_at(&desktop, session.background(), 40.0, 40.0);
    session.pump().expect("pump");
    assert!(session.take_launches().is_empty());
}

// ---- running ----

#[test]
fn run_returns_when_the_compositor_hangs_up() {
    // `TestDesktop::turn` closes the pipe once neither side has anything left,
    // which is how a test that would otherwise block forever ends.
    let (mut session, desktop) = session();
    desktop.borrow_mut().send_window_list(&[app(1, "Terminal")]);
    session.run().expect("run");
    assert!(!session.is_running());
    assert_eq!(session.shell().taskbar_windows().len(), 1);
}

#[test]
fn the_background_is_painted_once_and_the_chrome_on_every_change() {
    let (mut session, desktop) = session();
    let (background, panel) = (session.background().window(), session.panel().window());
    let after_start = desktop.borrow_mut().drawn();
    assert_eq!(
        after_start.iter().filter(|(w, _)| *w == background).count(),
        1,
        "the wallpaper should have been drawn exactly once"
    );

    let start = centre(session.shell().start_button_rect());
    press_at(&desktop, session.panel(), start.0, start.1);
    session.pump().expect("pump");

    let after_click = desktop.borrow_mut().drawn();
    assert_eq!(
        after_click.iter().filter(|(w, _)| *w == background).count(),
        1,
        "a click on the taskbar re-sent the whole wallpaper"
    );
    assert!(
        after_click.iter().filter(|(w, _)| *w == panel).count() > 1,
        "the taskbar was not repainted after the menu opened"
    );
}

#[test]
fn an_open_menu_is_actually_drawn_on_the_surface_that_was_mapped_for_it() {
    // Mapping the surface and drawing on it are two separate steps, and a shell
    // that did only the first would show the user a blank rectangle over the
    // desktop with no way to tell what had gone wrong.
    let (mut session, desktop) = session();
    let popups = session.popups().window();
    assert!(
        !desktop
            .borrow_mut()
            .drawn()
            .iter()
            .any(|(w, _)| *w == popups),
        "something was drawn on the menu surface before a menu opened"
    );

    let start = centre(session.shell().start_button_rect());
    press_at(&desktop, session.panel(), start.0, start.1);
    session.pump().expect("pump");

    let frame = desktop
        .borrow_mut()
        .drawn()
        .into_iter()
        .find(|(w, _)| *w == popups)
        .expect("the start menu opened and nothing was drawn on its surface");
    assert!(
        frame.1 > 2,
        "the menu surface got only its translation wrapper — an empty menu"
    );
}

#[test]
fn a_press_the_shell_does_not_want_repaints_nothing() {
    let (mut session, desktop) = session();
    let before = desktop.borrow_mut().drawn().len();
    // Bare desktop, with no menu open: nothing the shell drew has changed.
    press_at(&desktop, session.background(), 400.0, 400.0);
    session.pump().expect("pump");
    assert_eq!(desktop.borrow_mut().drawn().len(), before);
}

// ---- a settings change, announced rather than polled ----

/// Announce a settings change to one of the shell's surfaces, the way the
/// compositor does after handling a `ReloadAppearance` request.
fn announce(desktop: &Desktop, surface: Surface, group: SettingsGroup) {
    desktop.borrow_mut().send_input(&[InputEvent::new(
        surface.window(),
        guitk::event::Event::SettingsChanged { group },
    )]);
}

#[test]
fn an_announced_appearance_change_repaints_the_chrome() {
    // The end of the chain this feature is: the Settings app writes
    // `appearance.yaml` and sends `ReloadAppearance`; the compositor re-reads
    // and announces; the shell re-reads and repaints. `apps/settings` proves
    // the first link and `wire.rs` the second. This is the third.
    settingsfile::testing::with_scratch_config("session-announce", |_root| {
        let (mut session, desktop) = session();
        session.shell_mut().load_appearance();
        let before_accent = session.shell().appearance.accent_color;
        let before_drawn = desktop.borrow_mut().drawn().len();

        // Somebody else rewrites the file.
        let mut file = appearance::AppearanceFile::load();
        file.settings.accent_color = if before_accent == AccentColor::Teal {
            AccentColor::Mauve
        } else {
            AccentColor::Teal
        };
        file.save().expect("save");

        announce(&desktop, session.panel(), SettingsGroup::Appearance);
        session.pump().expect("pump");

        assert_ne!(
            session.shell().appearance.accent_color,
            before_accent,
            "the shell should have re-read the file it was told about"
        );
        assert!(
            desktop.borrow_mut().drawn().len() > before_drawn,
            "and repainted, since the chrome is drawn in the accent"
        );
    });
}

#[test]
fn an_announcement_with_nothing_behind_it_repaints_nothing() {
    // The compositor announces once per window, so the shell hears the same
    // change four times -- one per surface. Only the first can find anything
    // changed; the rest must be free. They are, because `poll_appearance`
    // compares the settings it read against the ones it holds rather than
    // trusting that an announcement means a difference.
    settingsfile::testing::with_scratch_config("session-announce-twice", |_root| {
        let (mut session, desktop) = session();
        session.shell_mut().load_appearance();

        let mut file = appearance::AppearanceFile::load();
        file.settings.accent_color = AccentColor::Peach;
        file.save().expect("save");

        announce(&desktop, session.panel(), SettingsGroup::Appearance);
        session.pump().expect("pump");
        let after_first = desktop.borrow_mut().drawn().len();

        announce(&desktop, session.panel(), SettingsGroup::Appearance);
        session.pump().expect("pump");
        assert_eq!(
            desktop.borrow_mut().drawn().len(),
            after_first,
            "the second announcement of the same change should cost nothing"
        );
    });
}

#[test]
fn an_input_settings_announcement_does_not_touch_the_shell() {
    // The shell reads `appearance.yaml` and not `input.yaml`, so an
    // announcement about the other group is not its business.
    //
    // The appearance file is changed *and left changed* on purpose. A first
    // version of this test announced `Input` against an unchanged file and
    // proved nothing: with nothing to find, a handler that ignored the group
    // entirely also repainted nothing, and the mutation that deletes the group
    // check survived. What separates the two is an announcement naming one
    // group while the *other* group's file is dirty -- then the over-eager
    // handler adopts a change it was never told about, and the correct one
    // waits to be told.
    settingsfile::testing::with_scratch_config("session-announce-input", |_root| {
        let (mut session, desktop) = session();
        session.shell_mut().load_appearance();
        let before_accent = session.shell().appearance.accent_color;
        let before_drawn = desktop.borrow_mut().drawn().len();

        let mut file = appearance::AppearanceFile::load();
        file.settings.accent_color = if before_accent == AccentColor::Teal {
            AccentColor::Mauve
        } else {
            AccentColor::Teal
        };
        file.save().expect("save");

        announce(&desktop, session.panel(), SettingsGroup::Input);
        session.pump().expect("pump");

        assert_eq!(
            session.shell().appearance.accent_color,
            before_accent,
            "an input announcement must not make the shell read appearance"
        );
        assert_eq!(desktop.borrow_mut().drawn().len(), before_drawn);
    });
}

// ---- following the display ----

#[test]
fn a_display_that_changes_size_moves_the_panel_with_it() {
    let (mut session, desktop) = session();
    let background = session.background().window();
    let panel = session.panel().window();

    desktop.borrow_mut().send_input(&[InputEvent::new(
        background,
        guitk::event::Event::Resize {
            width: 1280,
            height: 720,
        },
    )]);
    session.pump().expect("pump");

    assert_eq!(session.shell().screen_height, 720);
    let bar = session.shell().taskbar_rect();
    assert_eq!(session.panel().origin(), (bar.x, bar.y));
    assert!(
        desktop.borrow().seen.iter().any(|r| r.body
            == RequestBody::Move {
                window: panel,
                x: bar.x as i32,
                y: bar.y as i32
            }),
        "the taskbar stayed where the old screen's bottom edge used to be"
    );
}

// ---- the shape of an event that is not ours ----

#[test]
fn a_release_over_the_taskbar_is_not_mistaken_for_a_press() {
    let (mut session, desktop) = session();
    let (ox, oy) = session.panel().origin();
    let start = centre(session.shell().start_button_rect());
    desktop.borrow_mut().send_input(&[InputEvent::new(
        session.panel().window(),
        guitk::event::Event::Mouse(guitk::event::MouseEvent {
            x: start.0 - ox,
            y: start.1 - oy,
            kind: MouseEventKind::Release(MouseButton::Left),
        }),
    )]);
    session.pump().expect("pump");
    assert!(
        !session.shell().start_menu_open,
        "a release opened the start menu"
    );
}

// ---- the frame clock ----

#[test]
fn the_shell_s_park_is_bounded_by_a_wake_up_it_registered() {
    // The shell drives the loop by hand — `pump`, then park — so it has its own
    // chance to get this wrong, and would get it wrong in the quietest possible
    // way: `Connection::wait` knows nothing about wake-ups, so a shell parking
    // there sleeps straight through every deadline it set, and the only symptom
    // is an animation that stops. The recorded bound is what says the park went
    // through `EventLoop::wait` instead.
    let (mut session, _desktop) = session();
    let panel = session.panel().window();
    session
        .events_mut()
        .wake_after(panel, std::time::Duration::from_millis(50));

    // The harness hangs up once neither side has anything left to say, so this
    // returns after one park rather than looping.
    session.run().expect("the shell's own loop");

    let asked = &session.events_mut().connection().transport().asked;
    assert!(
        asked.iter().flatten().next().is_some(),
        "the shell parked with no bound at all: {asked:?}"
    );
}

// ---- what the frame clock actually drives ----

/// Super+Tab — the chord that opens the overview.
fn super_tab() -> guitk::event::Event {
    chord(
        Key::Tab,
        Modifiers {
            super_key: true,
            ..Modifiers::NONE
        },
    )
}

/// Push one frame at the shell, as the loop's own clock would.
///
/// The pending wake-up is cancelled first so that what is asserted afterwards
/// is whether *this frame* armed the next one, rather than the leftover of the
/// frame before. A real tick consumes its wake-up on the way out — wake-ups are
/// one-shot — and delivering one by hand does not, so without this every
/// assertion about re-arming would pass whatever the shell did.
fn frame(session: &mut Session, desktop: &Desktop, elapsed_ms: u64) {
    let panel = session.panel().window();
    session.events_mut().cancel_wake(panel);
    desktop.borrow_mut().send_input(&[InputEvent::new(
        panel,
        guitk::event::Event::Tick { elapsed_ms },
    )]);
    session.pump().expect("pump");
}

#[test]
fn an_idle_desktop_asks_for_no_frames() {
    // The property that makes a frame clock affordable. A shell that armed a
    // wake-up unconditionally would work exactly as well and cost a wake-up
    // every 16 ms for ever, on a desktop where nothing is moving — invisible in
    // every test that only checks what is drawn.
    let (mut session, _desktop) = session();
    let panel = session.panel().window();
    assert!(!session.events_mut().is_waking(panel));
    assert_eq!(session.events_mut().next_wakeup(), None);
}

#[test]
fn opening_the_overview_asks_for_a_frame() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_tab())]);
    session.pump().expect("pump");

    assert!(session.shell().overview.visible, "Super+Tab did nothing");
    assert!(
        session.shell().overview.is_fading(),
        "the overview opened without starting its fade"
    );
    assert!(
        session.events_mut().is_waking(panel),
        "the fade was started and no frame was asked for — it would sit at its \
         dimmest for ever, which is the defect design-decisions.md §520 is about"
    );
}

#[test]
fn an_overview_whose_fade_never_runs_is_still_drawn_and_still_clickable() {
    // The §520 regression, at the level it actually bit: the first fade gated
    // every draw path on progress, so an overlay whose clock never ran was
    // blank *and* took every click. Nothing here may depend on a frame having
    // arrived — the fade is begun below and deliberately never advanced.
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    let popups = session.popups().window();
    desktop
        .borrow_mut()
        .send_window_list(&[app(1, "Terminal"), app(2, "Editor")]);
    session.pump().expect("pump");
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_tab())]);
    session.pump().expect("pump");
    assert!(
        session.shell().overview.is_fading(),
        "the test's premise is wrong: no fade was started, so nothing is being \
         held at zero progress"
    );

    // Drawn: the overlay reached the surface with more than its translation
    // wrapper on it.
    let frame = desktop
        .borrow_mut()
        .drawn()
        .into_iter()
        .rfind(|(w, _)| *w == popups)
        .expect("the overview opened and nothing was drawn on its surface");
    assert!(
        frame.1 > 2,
        "the overview surface got only its translation wrapper — a blank \
         fullscreen overlay, which is exactly what §520 shipped"
    );

    // Clickable: a press inside a card is answered, rather than swallowed by an
    // overlay that has not faded in yet.
    let card = session
        .shell()
        .overview_layout()
        .into_iter()
        .next()
        .expect("no cards laid out");
    let (x, y) = (
        card.render_x + card.render_width / 2.0,
        card.render_y + card.render_height / 2.0,
    );
    press_at(&desktop, session.popups(), x, y);
    session.pump().expect("pump");
    assert!(
        !controls(&desktop).is_empty(),
        "a click on a card in an un-ticked overview did nothing"
    );
}

#[test]
fn a_frame_advances_the_fade_and_the_last_one_stops_asking_for_more() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    let fade_ms = session.shell().overview_config.fade_ms;
    assert!(fade_ms > 0, "the default overview has no fade to advance");

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_tab())]);
    session.pump().expect("pump");
    let opening = session.shell().overview.fade_opacity();
    assert!(opening < 1.0, "the fade began already finished: {opening}");

    // Half way: further on than it was, and still asking for frames.
    frame(&mut session, &desktop, u64::from(fade_ms) / 2);
    let midway = session.shell().overview.fade_opacity();
    assert!(midway > opening, "a frame did not advance the fade");
    assert!(midway < 1.0, "half a fade's worth of time finished it");
    assert!(
        session.events_mut().is_waking(panel),
        "the shell stopped asking for frames with the fade half done"
    );

    // Past the end: fully open, and — the point of the whole design — no
    // wake-up left registered, so the loop parks unbounded again.
    frame(&mut session, &desktop, u64::from(fade_ms));
    assert!(!session.shell().overview.is_fading());
    assert!(
        (session.shell().overview.fade_opacity() - 1.0).abs() < f32::EPSILON,
        "a finished fade did not land on fully open"
    );
    assert!(
        !session.events_mut().is_waking(panel),
        "the fade finished and the shell kept the clock running — an idle \
         desktop waking 60 times a second for nothing"
    );
}

#[test]
fn the_frame_that_finishes_the_fade_is_still_painted() {
    // Off-by-one bait. `has_active` is false *after* the step that finishes the
    // last animation, so a shell that decided whether to repaint by asking
    // afterwards would drop precisely the frame that puts the overlay at its
    // final opacity, and the fade would visibly stop one frame short.
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    let fade_ms = session.shell().overview_config.fade_ms;
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_tab())]);
    session.pump().expect("pump");

    let popups = session.popups().window();
    let before = desktop
        .borrow_mut()
        .drawn()
        .iter()
        .filter(|(w, _)| *w == popups)
        .count();
    // One frame, long enough to run the fade past its end in a single step.
    frame(&mut session, &desktop, u64::from(fade_ms) * 2);
    let after = desktop
        .borrow_mut()
        .drawn()
        .iter()
        .filter(|(w, _)| *w == popups)
        .count();
    assert!(
        after > before,
        "the frame that finished the fade drew nothing"
    );
}

#[test]
fn a_frame_with_nothing_moving_asks_for_no_more_frames() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    // A tick can arrive with nothing to advance — the last frame of one
    // animation and a stray wake-up can race. It must not re-arm.
    frame(&mut session, &desktop, 16);
    assert!(!session.events_mut().is_waking(panel));
}

#[test]
fn reduced_motion_opens_the_overview_without_a_fade_and_without_a_clock() {
    // Reduced motion is not "the same animation, faster". An animation that
    // still runs but is invisible costs the same wake-ups and is the same
    // motion sickness; the setting has to reach the clock, not just the paint.
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    session.set_reduced_motion(true);
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_tab())]);
    session.pump().expect("pump");

    assert!(session.shell().overview.visible, "Super+Tab did nothing");
    assert!(!session.shell().overview.is_fading());
    assert!(
        (session.shell().overview.fade_opacity() - 1.0).abs() < f32::EPSILON,
        "reduced motion left the overview part-way through a fade it will \
         never finish"
    );
    assert!(
        !session.events_mut().is_waking(panel),
        "reduced motion still armed the frame clock"
    );
}

#[test]
fn turning_reduced_motion_on_mid_fade_lands_on_fully_open() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_tab())]);
    session.pump().expect("pump");
    assert!(session.shell().overview.is_fading());

    session.set_reduced_motion(true);
    assert!(!session.shell().overview.is_fading());
    assert!((session.shell().overview.fade_opacity() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn closing_the_overview_takes_its_fade_with_it() {
    // Otherwise the next `show` inherits a part-finished fade, and — worse —
    // the shell keeps asking for frames to advance an overlay that is not on
    // screen.
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_tab())]);
    session.pump().expect("pump");
    assert!(session.shell().overview.is_fading());

    session.events_mut().cancel_wake(panel);
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_tab())]);
    session.pump().expect("pump");
    assert!(
        !session.shell().overview.visible,
        "Super+Tab did not close it"
    );
    assert!(!session.shell().overview.is_fading());
    assert!(
        !session.events_mut().is_waking(panel),
        "the shell is still clocking a fade for an overlay that is gone"
    );
}

#[test]
fn a_window_animation_runs_off_the_same_clock() {
    // The manager's animations and the overview's fade are separate things —
    // the fade lives on the overview so the overview can be drawn without a
    // manager — and a shell that armed the clock for one but not the other
    // would work until they were used apart.
    use crate::animations::WindowAnimation;
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    assert!(!session.events_mut().is_waking(panel));

    session.animate_window(WindowAnimation::open(7, 0.0, 0.0, 800.0, 600.0, 100));
    assert_eq!(session.animations().active_count(), 1);
    assert!(
        session.events_mut().is_waking(panel),
        "an animation was started and no frame was asked for"
    );

    frame(&mut session, &desktop, 200);
    assert_eq!(
        session.animations().active_count(),
        0,
        "a frame twice the animation's length did not finish it"
    );
    assert!(
        !session.events_mut().is_waking(panel),
        "the animation finished and the clock kept running"
    );
}

// ---- the wallpaper's pixels ----
//
// `WallpaperManager` allocates an image id and emits a `RenderCommand::Image`
// naming it; the compositor draws whatever bytes are stored under that id, and
// silently nothing if none are. Until `refresh_wallpaper_image` existed nothing
// in the tree ever stored any, so every wallpaper set to a file painted the
// colour underlay and stopped. These tests are about the half that was missing:
// that the bytes go up, that they go up *once*, and that a file which cannot be
// read costs a wallpaper rather than a desktop.

/// A real PNG, written by libpng rather than by anything in this tree.
///
/// Reached across to the sibling crate's conformance fixtures rather than
/// copied here, because the property that makes the file worth using is that
/// *nothing in this repository chose its bytes* — and a copy inherits the bytes
/// without inheriting the property, then goes stale the first time the fixture
/// is regenerated. The pictures are 9x7; see `gui/imagecodec/tests/data/`.
fn fixture(name: &str) -> String {
    format!(
        "{}/../imagecodec/tests/data/{name}.png",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// The directory this process's scratch files live in, created once.
///
/// Per *process*, not per test, and that is the whole point. The temp
/// directory is shared by every `cargo test` running against every worktree on
/// the machine, so a fixed name is a name two processes will write at the same
/// moment. On Windows that does not race quietly: the second writer gets
/// "Access is denied. (os error 5)" and the test fails outright. That is an
/// observed failure, not a hypothesis — `userspace/firejail`'s
/// `test_remove_sandbox_file` (fixed path `%TEMP%/firejail_test_rm`) failed
/// exactly that way on 2026-08-26 when two workspace runs overlapped, and it
/// passes on its own every time.
///
/// The name that was here before was per *test*, on the reasoning that a
/// leftover from a crashed run would be overwritten by the next one rather
/// than accumulating. That trades a guaranteed small leak for an intermittent
/// failure, which is the wrong way round: a stray file costs bytes, a flaky
/// test costs a fourteen-minute workspace run and the credibility of the next
/// real failure. The PID keeps the leak to one directory per crashed run.
fn scratch_dir() -> &'static std::path::Path {
    use std::sync::OnceLock;
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("slateos-wallpaper-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the temp directory is not writable");
        dir
    })
}

/// A file with contents of our choosing, for the cases no valid fixture covers.
fn scratch(name: &str, bytes: &[u8]) -> String {
    let path = scratch_dir().join(name);
    std::fs::write(&path, bytes).expect("the temp directory is not writable");
    path.to_string_lossy().into_owned()
}

/// Every image upload the session sent, as `(window, image_id, width, height,
/// stride, byte count)`.
fn uploads(desktop: &Desktop) -> Vec<(u64, u64, u32, u32, u32, usize)> {
    desktop
        .borrow()
        .seen
        .iter()
        .filter_map(|r| match &r.body {
            RequestBody::UploadImage {
                window,
                image_id,
                width,
                height,
                stride,
                bytes,
                ..
            } => Some((*window, *image_id, *width, *height, *stride, bytes.len())),
            _ => None,
        })
        .collect()
}

/// Every image release the session sent, as `(window, image_id)`.
fn drops(desktop: &Desktop) -> Vec<(u64, u64)> {
    desktop
        .borrow()
        .seen
        .iter()
        .filter_map(|r| match r.body {
            RequestBody::DropImage { window, image_id } => Some((window, image_id)),
            _ => None,
        })
        .collect()
}

/// The names of every request the session has sent, in order.
///
/// Separate from the typed helpers above because two of these tests are about
/// *sequence* rather than payload — a drop that arrives after the upload it was
/// meant to make room for is a correct-looking pair in the wrong order.
fn order(desktop: &Desktop) -> Vec<&'static str> {
    desktop.borrow_mut().asked()
}

#[test]
fn a_wallpaper_file_is_uploaded_under_the_id_the_render_tree_names() {
    let (mut session, desktop) = session();
    let background = session.background().window();
    session
        .wallpaper_mut()
        .set_image(&fixture("rgb8"), crate::wallpaper::ImageFit::Fill);
    let id = session.wallpaper_mut().current_image_id();
    assert_ne!(id, 0, "setting an image did not allocate an id");

    session.paint_background().expect("the harness refused");

    assert_eq!(session.wallpaper_error(), None);
    assert_eq!(
        uploads(&desktop),
        vec![(background, id, 9, 7, 9 * 4, 9 * 7 * 4)],
        "the wallpaper did not reach the compositor, or reached it padded"
    );
}

#[test]
fn the_picture_goes_up_before_the_frame_that_draws_it() {
    // Order, not just presence. The compositor draws nothing — silently — for
    // an id it has no bytes for, so a frame that overtook its upload would be
    // one blank repaint with no error anywhere to explain it.
    let (mut session, desktop) = session();
    session
        .wallpaper_mut()
        .set_image(&fixture("rgb8"), crate::wallpaper::ImageFit::Fill);
    // Flush whatever `start` drew, so the count below moves only for this paint.
    let before = desktop.borrow_mut().drawn().len();

    session.paint_background().expect("the harness refused");

    // The upload is a round trip and the frame is not, so at this instant the
    // compositor has necessarily read and answered the upload, while the frame
    // is still unread in the pipe. That asymmetry is what makes the order
    // observable from here at all — hence `seen` and `submitted` read directly,
    // rather than through the helpers, which absorb the pipe and destroy it.
    assert!(
        desktop
            .borrow()
            .seen
            .iter()
            .any(|r| matches!(r.body, RequestBody::UploadImage { .. })),
        "the frame was built before the upload reached the compositor"
    );
    assert_eq!(
        desktop.borrow().submitted.len(),
        before,
        "the frame overtook its own upload"
    );
    assert_eq!(
        desktop.borrow_mut().drawn().len(),
        before + 1,
        "the background surface drew nothing, or drew twice"
    );
}

#[test]
fn painting_the_background_twice_uploads_the_picture_once() {
    // `paint_background` runs on every repaint, and a repaint happens on every
    // click that changes anything. Re-reading and re-inflating a 4K wallpaper
    // per click would be a stutter with no visible cause.
    let (mut session, desktop) = session();
    session
        .wallpaper_mut()
        .set_image(&fixture("rgba8"), crate::wallpaper::ImageFit::Fill);

    session.paint_background().expect("the harness refused");
    session.paint_background().expect("the harness refused");
    session.paint_background().expect("the harness refused");

    assert_eq!(uploads(&desktop).len(), 1, "one picture, one upload");
    assert!(
        drops(&desktop).is_empty(),
        "nothing changed and a drop went out"
    );
}

#[test]
fn a_slideshow_step_releases_the_old_picture_before_uploading_the_new_one() {
    // The order is the point, not just the pair. A shell that uploaded first
    // and dropped afterwards would charge the compositor's per-link image
    // budget for two full-screen pictures at once, so a budget that fits one
    // wallpaper would refuse every slide after the first.
    let (mut session, desktop) = session();
    let background = session.background().window();
    session.wallpaper_mut().set_slideshow("/pics", 60, false);
    session
        .wallpaper_mut()
        .populate_slideshow_paths(vec![fixture("rgb8"), fixture("gray8")]);
    let first = session.wallpaper_mut().current_image_id();
    session.paint_background().expect("the harness refused");

    session.wallpaper_mut().next_wallpaper();
    let second = session.wallpaper_mut().current_image_id();
    assert_ne!(first, second, "a slideshow step reused the image id");
    session.paint_background().expect("the harness refused");

    assert_eq!(drops(&desktop), vec![(background, first)]);
    let sent = uploads(&desktop);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].1, first);
    assert_eq!(sent[1].1, second);

    let names = order(&desktop);
    let dropped = names
        .iter()
        .position(|n| *n == "DropImage")
        .expect("no drop was sent");
    let second_upload = names
        .iter()
        .enumerate()
        .filter(|(_, n)| **n == "UploadImage")
        .map(|(i, _)| i)
        .nth(1)
        .expect("no second upload was sent");
    assert!(
        dropped < second_upload,
        "the new slide went up while the old one was still charged: {names:?}"
    );
}

#[test]
fn a_wallpaper_that_is_not_there_costs_a_picture_and_not_a_desktop() {
    // The failure a user actually hits: a config carried over from another
    // machine, naming a path that does not exist here. The taskbar still has to
    // draw.
    let missing = scratch_dir()
        .join("nowhere-at-all.png")
        .to_string_lossy()
        .into_owned();
    // Not merely assumed absent: a leftover from an earlier run under this name
    // would make the test pass for the wrong reason. The scratch directory is
    // this process's own, so removing a file in it cannot disturb a concurrent
    // run of the same test in another process.
    let _ = std::fs::remove_file(&missing);
    let (mut session, desktop) = session();
    session
        .wallpaper_mut()
        .set_image(&missing, crate::wallpaper::ImageFit::Fill);
    let before = desktop.borrow_mut().drawn().len();

    session
        .paint_background()
        .expect("a missing wallpaper failed the whole repaint");

    let why = session.wallpaper_error().expect("no error was recorded");
    assert!(
        why.contains("nowhere-at-all.png"),
        "the error does not say which file: {why}"
    );
    assert!(uploads(&desktop).is_empty());
    assert_eq!(
        desktop.borrow_mut().drawn().len(),
        before + 1,
        "the background surface drew nothing at all"
    );
}

#[test]
fn a_corrupt_wallpaper_is_attempted_once_and_not_on_every_repaint() {
    let (mut session, desktop) = session();
    let path = scratch("corrupt.png", b"\x89PNG\r\n\x1a\nand then nonsense");
    session
        .wallpaper_mut()
        .set_image(&path, crate::wallpaper::ImageFit::Fill);

    session
        .paint_background()
        .expect("a corrupt wallpaper failed the repaint");
    let first = session
        .wallpaper_error()
        .expect("no error was recorded")
        .to_owned();
    session
        .paint_background()
        .expect("a corrupt wallpaper failed the repaint");

    assert_eq!(session.wallpaper_error(), Some(first.as_str()));
    assert!(uploads(&desktop).is_empty());
    assert!(
        first.contains("corrupt.png"),
        "the error does not say which file: {first}"
    );
    // A second attempt is not directly observable — a failed read sends no
    // request — so the proxy is that removing the file entirely between the two
    // paints changes nothing: a session that went back to disk would report a
    // *different* reason the second time.
    std::fs::remove_file(&path).expect("the scratch file vanished");
    session
        .paint_background()
        .expect("a corrupt wallpaper failed the repaint");
    assert_eq!(
        session.wallpaper_error(),
        Some(first.as_str()),
        "the file was read again on a repaint that changed nothing"
    );
}

#[test]
fn going_back_to_a_solid_colour_gives_the_picture_back() {
    // An id of zero means the render tree emits no `Image` command at all, so
    // anything still uploaded is unreachable — and unreachable bytes still
    // count against the link's budget.
    let (mut session, desktop) = session();
    let background = session.background().window();
    session
        .wallpaper_mut()
        .set_image(&fixture("palette8_trns"), crate::wallpaper::ImageFit::Fill);
    let id = session.wallpaper_mut().current_image_id();
    session.paint_background().expect("the harness refused");

    session
        .wallpaper_mut()
        .set_solid_color(guitk::color::Color::rgb(20, 20, 30));
    session.paint_background().expect("the harness refused");

    assert_eq!(drops(&desktop), vec![(background, id)]);
    assert_eq!(uploads(&desktop).len(), 1);
    assert_eq!(session.wallpaper_error(), None);
}

#[test]
fn a_compositor_that_refuses_the_picture_still_gets_a_painted_desktop() {
    // The refusal `RequestBody::UploadImage` is documented to give: a picture
    // over this link's image budget. Survivable on exactly the same terms as a
    // corrupt file — the colour underlay paints either way — and distinctly
    // *not* on the same terms as a dead connection, which propagates.
    let (mut session, desktop) = session();
    session
        .wallpaper_mut()
        .set_image(&fixture("rgb8"), crate::wallpaper::ImageFit::Fill);
    desktop.borrow_mut().refuse = Some("image budget exhausted".to_string());

    session
        .paint_background()
        .expect("a refused upload failed the whole repaint");

    let why = session
        .wallpaper_error()
        .expect("a refusal recorded no error");
    assert!(
        why.contains("image budget exhausted"),
        "the compositor's reason was thrown away: {why}"
    );
}

// ============================================================================
// The wallpaper failure the user can actually read
//
// `wallpaper_error` had four writers and one reader — a getter nothing called.
// A wallpaper that failed to decode therefore left the user looking at a plain
// colour with no way at all to find out why. These are the tests that would
// have failed while that was true.
// ============================================================================

/// Every notification the pane is holding, newest first, as `(title, body)`.
fn posted(session: &Session) -> Vec<(String, String)> {
    session
        .shell()
        .notifications
        .notifications()
        .iter()
        .map(|n| (n.title.clone(), n.body.clone()))
        .collect()
}

#[test]
fn a_wallpaper_that_will_not_decode_says_so_where_the_user_can_read_it() {
    let (mut session, _desktop) = session();
    let path = scratch("says-so.png", b"\x89PNG\r\n\x1a\nand then nonsense");
    session
        .wallpaper_mut()
        .set_image(&path, crate::wallpaper::ImageFit::Fill);

    session.paint_background().expect("the harness refused");

    let notes = posted(&session);
    assert_eq!(
        notes.len(),
        1,
        "a failure the shell noticed is a failure it must say: {notes:?}"
    );
    assert!(
        notes[0].1.contains("says-so.png"),
        "the notification does not name the file: {:?}",
        notes[0]
    );
    // The same reason, not a second wording of it. Two strings for one failure
    // is two strings that can drift apart.
    assert_eq!(
        Some(notes[0].1.as_str()),
        session.wallpaper_error(),
        "the notification and the recorded error disagree"
    );
}

#[test]
fn a_failure_is_reported_once_and_not_once_per_repaint() {
    // `paint_background` runs on every repaint, so an unconditional post would
    // put one notification per mouse click into the history for as long as a
    // corrupt file stayed selected.
    let (mut session, _desktop) = session();
    let path = scratch("once-only.png", b"\x89PNG\r\n\x1a\nand then nonsense");
    session
        .wallpaper_mut()
        .set_image(&path, crate::wallpaper::ImageFit::Fill);

    for _ in 0..5 {
        session.paint_background().expect("the harness refused");
    }

    assert_eq!(
        posted(&session).len(),
        1,
        "one failure produced one notification per repaint"
    );
}

#[test]
fn reporting_a_failure_does_not_shove_the_pane_over_the_screen() {
    // A wallpaper that did not load is a thing to explain, not an emergency to
    // interrupt with. The desktop is fully usable; a panel that opened itself
    // at login over a missing file would be worse than the missing file.
    let (mut session, _desktop) = session();
    session.wallpaper_mut().set_image(
        &scratch("quiet.png", b"\x89PNG\r\n\x1a\nnonsense"),
        crate::wallpaper::ImageFit::Fill,
    );

    session.paint_background().expect("the harness refused");

    assert!(
        !session.shell().notifications.pane_state().is_visible(),
        "a failed wallpaper opened the notification pane"
    );
    assert_eq!(session.shell().notifications.unread_count(), 1);
}

#[test]
fn a_wallpaper_that_loads_reports_nothing() {
    let (mut session, _desktop) = session();
    session
        .wallpaper_mut()
        .set_image(&fixture("rgb8"), crate::wallpaper::ImageFit::Fill);

    session.paint_background().expect("the harness refused");

    assert_eq!(session.wallpaper_error(), None);
    assert!(
        posted(&session).is_empty(),
        "a wallpaper that worked still filed a complaint"
    );
}

#[test]
fn a_second_broken_wallpaper_gets_its_own_notification() {
    // Deduplicating on "is there already an error" rather than on "is it this
    // error" would silence every failure after the first, so a user who fixed
    // one path and mistyped the next would get no word about the second.
    let (mut session, _desktop) = session();
    session.wallpaper_mut().set_image(
        &scratch("first-bad.png", b"\x89PNG\r\n\x1a\nnonsense"),
        crate::wallpaper::ImageFit::Fill,
    );
    session.paint_background().expect("the harness refused");

    session.wallpaper_mut().set_image(
        &scratch("second-bad.png", b"\x89PNG\r\n\x1a\nnonsense"),
        crate::wallpaper::ImageFit::Fill,
    );
    session.paint_background().expect("the harness refused");

    let notes = posted(&session);
    assert_eq!(
        notes.len(),
        2,
        "the second failure was swallowed: {notes:?}"
    );
    assert!(notes.iter().any(|(_, body)| body.contains("first-bad.png")));
    assert!(
        notes
            .iter()
            .any(|(_, body)| body.contains("second-bad.png"))
    );
}

// ============================================================================
// The notification pane's slide
// ============================================================================

#[test]
fn opening_the_pane_from_a_key_rewinds_it_into_a_slide() {
    // The session is the caller that owns a clock, so it puts the pane back
    // where it started and lets the frame clock carry it. Every other caller
    // gets the pane fully open. See design-decisions.md 520 and 562.
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_n())]);

    session.pump().expect("the harness refused");

    assert!(
        session.shell().notifications.is_sliding(),
        "the pane snapped open instead of sliding"
    );
    assert!(
        session.shell().notifications.pane_state().is_visible(),
        "a sliding pane is still a pane that is on screen"
    );
}

#[test]
fn the_slide_finishes_and_then_the_desktop_goes_quiet() {
    // The condition that keeps an idle desktop idle: once nothing is moving,
    // no wake-up is registered and the loop parks with no bound at all. A pane
    // missing from `anything_moving` is a pane that stops mid-slide.
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_n())]);
    session.pump().expect("the harness refused");

    // Well past the pane's own animation length at any plausible frame rate.
    for _ in 0..200 {
        session.step_frame(16);
    }

    assert!(!session.anything_moving(), "the desktop never went quiet");
    assert_eq!(
        session.shell().notifications.pane_state(),
        crate::notif_pane::PaneState::Visible,
        "the slide stopped somewhere other than fully open"
    );
}

#[test]
fn closing_the_pane_slides_it_out_and_it_stays_out() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_n())]);
    session.pump().expect("the harness refused");
    for _ in 0..200 {
        session.step_frame(16);
    }

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_n())]);
    session.pump().expect("the harness refused");
    assert!(
        session.shell().notifications.pane_state().is_visible(),
        "the close snapped instead of sliding"
    );

    for _ in 0..200 {
        session.step_frame(16);
    }

    assert_eq!(
        session.shell().notifications.pane_state(),
        crate::notif_pane::PaneState::Hidden
    );
    assert!(!session.anything_moving());
}

#[test]
fn a_frame_tick_does_not_restart_the_slide_it_just_finished() {
    // The slide *ends* by changing the same open flag the session watches to
    // decide a gesture happened. Watching a tick as well would read the end of
    // the slide as a fresh gesture and start it over, for ever.
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_n())]);
    session.pump().expect("the harness refused");
    for _ in 0..200 {
        session.step_frame(16);
    }
    // Close it, let the slide-out run to completion *through the event path*,
    // which is where the sampling lives.
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_n())]);
    session.pump().expect("the harness refused");

    for _ in 0..200 {
        session
            .dispatch(panel, guitk::event::Event::Tick { elapsed_ms: 16 })
            .expect("a tick failed");
    }

    assert_eq!(
        session.shell().notifications.pane_state(),
        crate::notif_pane::PaneState::Hidden,
        "the pane bounced back open when its slide finished"
    );
}

// ---- the heads-up overlays ----

/// A bare media key, with no modifiers. The three are bound
/// modifier-agnostically — a key with one meaning and no other job is not a
/// chord — so this is only the most ordinary of the sixteen ways to press one.
fn media(k: Key) -> guitk::event::Event {
    key(k)
}

#[test]
fn the_overlay_surface_is_unmapped_until_something_asks_to_be_shown() {
    let (mut session, desktop) = session();
    let osd = session.osd().window();

    // Mapped at creation, like the popup surface, so the first paint has to
    // take it away again.
    assert!(
        desktop.borrow().seen.iter().any(|r| r.body
            == RequestBody::SetVisible {
                window: osd,
                visible: false
            }),
        "the overlay surface was left covering the desktop"
    );

    let panel = session.panel().window();
    let before = desktop.borrow().seen.len();
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, media(Key::VolumeUp))]);
    session.pump().expect("pump");

    assert!(
        desktop.borrow().seen[before..].iter().any(|r| r.body
            == RequestBody::SetVisible {
                window: osd,
                visible: true
            }),
        "the volume changed and the overlay had nothing to be drawn on"
    );
    let drawn = desktop
        .borrow_mut()
        .drawn()
        .into_iter()
        .rfind(|(w, _)| *w == osd)
        .expect("the overlay surface was mapped and never painted");
    assert!(
        drawn.1 > 2,
        "the overlay surface got only its translation wrapper — a mapped, \
         blank, full-screen sheet"
    );
}

#[test]
fn the_volume_keys_move_the_volume_and_say_so() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    let start = session.shell().notifications.volume();

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, media(Key::VolumeUp))]);
    session.pump().expect("pump");
    let up = session.shell().notifications.volume();
    assert!(
        up > start,
        "VolumeUp did not turn the volume up: {start} then {up}"
    );
    assert!(
        session.shell().osd.has_visible(),
        "the volume moved with nothing on screen to say it had"
    );

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, media(Key::VolumeDown))]);
    session.pump().expect("pump");
    assert_eq!(
        session.shell().notifications.volume(),
        start,
        "down did not undo up"
    );
}

/// The overlay's whole life is a timeout, and a timeout with no frame behind it
/// never comes due: a shell that showed one without arming a frame would leave
/// the volume indicator on screen until the user happened to move something
/// else. The §520 failure in its second form.
#[test]
fn showing_an_overlay_asks_for_a_frame_and_the_last_one_stops_asking() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    assert!(
        !session.events_mut().is_waking(panel),
        "the test's premise is wrong: the desktop was not idle to begin with"
    );

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, media(Key::VolumeUp))]);
    session.pump().expect("pump");
    assert!(
        session.events_mut().is_waking(panel),
        "an overlay was put on screen and no frame was asked for, so its \
         timeout can never come due"
    );

    // Well short of the whole fade-in plus timeout plus fade-out.
    frame(&mut session, &desktop, 500);
    assert!(
        session.shell().osd.has_visible(),
        "half a second retired it"
    );
    assert!(
        session.events_mut().is_waking(panel),
        "the shell stopped asking for frames with an overlay still up"
    );

    // Past the end: gone, the surface taken away again, and — the point of the
    // whole design — no wake-up left, so the loop parks unbounded.
    let osd = session.osd().window();
    let before = desktop.borrow().seen.len();
    frame(&mut session, &desktop, 10_000);
    assert!(
        !session.shell().osd.has_visible(),
        "ten seconds did not retire a two-second overlay"
    );
    assert!(
        desktop.borrow().seen[before..].iter().any(|r| r.body
            == RequestBody::SetVisible {
                window: osd,
                visible: false
            }),
        "the overlay expired but its surface stayed up"
    );
    assert!(
        !session.events_mut().is_waking(panel),
        "the last overlay went away and the desktop kept waking up for ever"
    );
}

/// The overlay is not a menu, and the two must not be confused for each other:
/// a volume indicator that dismissed the start menu, or a start menu whose
/// surface was taken away when the volume faded, would both be this failing.
#[test]
fn an_overlay_and_a_menu_do_not_disturb_each_other() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    let popups = session.popups().window();

    let start = centre(session.shell().start_button_rect());
    press_at(&desktop, session.panel(), start.0, start.1);
    session.pump().expect("pump");
    assert!(session.shell().start_menu_open);

    let before = desktop.borrow().seen.len();
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, media(Key::VolumeUp))]);
    session.pump().expect("pump");
    assert!(
        session.shell().start_menu_open,
        "turning the volume up closed the start menu"
    );
    assert!(
        session.shell().osd.has_visible(),
        "the overlay did not appear over the open menu"
    );

    // Now let the overlay expire, with the menu still open: the menu's surface
    // must not go with it.
    frame(&mut session, &desktop, 10_000);
    assert!(!session.shell().osd.has_visible());
    assert!(
        !desktop.borrow().seen[before..].iter().any(|r| r.body
            == RequestBody::SetVisible {
                window: popups,
                visible: false
            }),
        "the overlay faded and took the start menu's surface with it"
    );
}

#[test]
fn the_mute_key_silences_without_forgetting_the_level() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    let level = session.shell().notifications.volume();
    assert!(
        level > 0,
        "the test needs a level there is something to keep"
    );

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, media(Key::VolumeMute))]);
    session.pump().expect("pump");
    assert!(session.shell().notifications.is_muted(), "mute did nothing");
    assert_eq!(
        session.shell().notifications.volume(),
        level,
        "muting threw the level away, so unmuting has nothing to restore"
    );

    // And turning it up is a way back: a key that only moves an inaudible
    // number looks broken.
    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, media(Key::VolumeUp))]);
    session.pump().expect("pump");
    assert!(
        !session.shell().notifications.is_muted(),
        "turning the volume up left it muted"
    );
}

/// A display that changed size while an overlay was up would otherwise centre
/// the drawing on the new screen inside a surface still the old size, which
/// clips it away entirely.
#[test]
fn the_overlay_surface_follows_the_display() {
    let (mut session, desktop) = session();
    let background = session.background().window();
    let osd = session.osd().window();
    let before = desktop.borrow().seen.len();

    desktop.borrow_mut().send_input(&[InputEvent::new(
        background,
        guitk::event::Event::Resize {
            width: 1920,
            height: 1080,
        },
    )]);
    session.pump().expect("pump");

    assert!(
        desktop.borrow().seen[before..].iter().any(|r| r.body
            == RequestBody::Resize {
                window: osd,
                width: 1920,
                height: 1080
            }),
        "the display shrank and the overlay surface stayed at the old size"
    );
}

// ---- the Run box ----

/// Super+R — the chord that opens and closes the Run box.
fn super_r() -> guitk::event::Event {
    chord(
        Key::R,
        Modifiers {
            super_key: true,
            ..Modifiers::NONE
        },
    )
}

/// One printable keystroke, carried the way a real keyboard sends it.
fn typed(k: Key, ch: char) -> guitk::event::Event {
    guitk::event::Event::Key(KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: ch.to_string(),
    })
}

/// The Run box is drawn on the popup surface, not the overlay one, and the
/// popup surface is unmapped whenever nothing is on it: a box that did not
/// count as "something on it" would be a box the compositor was never asked to
/// show, so Super+R would appear to do nothing at all.
#[test]
fn opening_the_run_box_maps_the_surface_it_is_drawn_on() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();
    let popups = session.popups().window();
    let before = desktop.borrow().seen.len();

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_r())]);
    session.pump().expect("pump");
    assert!(session.shell().run_dialog.is_visible());

    assert!(
        desktop.borrow().seen[before..].iter().any(|r| r.body
            == RequestBody::SetVisible {
                window: popups,
                visible: true
            }),
        "the box was opened and had nothing to be drawn on"
    );
    let drawn = desktop
        .borrow_mut()
        .drawn()
        .into_iter()
        .rfind(|(w, _)| *w == popups)
        .expect("the popup surface was mapped and never painted");
    assert!(
        drawn.1 > 2,
        "the popup surface got only its translation wrapper — a mapped, blank \
         sheet where the box should be"
    );
}

/// The keyboard's half of `ShellAction::Launch`. Everything up to the last step
/// worked without this: the box opened, took the text, resolved it and reported
/// an `Execute` — which the shell then had no channel to hand back, so the
/// command was resolved and dropped.
#[test]
fn a_command_confirmed_with_enter_reaches_the_launcher() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_r())]);
    session.pump().expect("pump");

    for (k, ch) in [
        (Key::T, 't'),
        (Key::E, 'e'),
        (Key::R, 'r'),
        (Key::M, 'm'),
        (Key::I, 'i'),
        (Key::N, 'n'),
        (Key::A, 'a'),
        (Key::L, 'l'),
    ] {
        desktop
            .borrow_mut()
            .send_input(&[InputEvent::new(panel, typed(k, ch))]);
    }
    session.pump().expect("pump");
    assert!(
        session.take_launches().is_empty(),
        "typing a command started something before it was confirmed"
    );

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, key(Key::Enter))]);
    session.pump().expect("pump");

    assert_eq!(
        session.take_launches(),
        [std::path::PathBuf::from("terminal")]
    );
    assert!(
        !session.shell().run_dialog.is_visible(),
        "the box stayed up after starting the command"
    );
    assert!(
        session.take_launches().is_empty(),
        "taking the launches twice handed out the same command twice"
    );
}

/// A press outside the box is the ordinary way to give up on it, and it arrives
/// on the popup surface — the same surface the box itself is on — so the
/// session has to translate it and hand it to the shell before anything else
/// gets a look at it.
#[test]
fn a_press_beside_the_box_closes_it_without_starting_anything() {
    let (mut session, desktop) = session();
    let panel = session.panel().window();

    desktop
        .borrow_mut()
        .send_input(&[InputEvent::new(panel, super_r())]);
    session.pump().expect("pump");

    press_at(&desktop, session.popups(), 8.0, 8.0);
    session.pump().expect("pump");

    assert!(
        !session.shell().run_dialog.is_visible(),
        "a click on the desktop left the box up"
    );
    assert!(session.take_launches().is_empty());
}

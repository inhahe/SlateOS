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

use guiremote::control::{RequestBody, ShellControlAction};
use guiremote::window_list::WindowInfo;
use guitk::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
use guitk::render::RenderCommand;
use oswindow::InputEvent;
use oswindow::testing::{TestConnection, TestDesktop, desktop as wired};

use super::{ShellSession, Surface};
use crate::{DesktopShell, Hit, Rect, click};

type Session = ShellSession<TestConnection>;
type Desktop = Rc<RefCell<TestDesktop>>;

/// A started session and the compositor behind it.
///
/// `TestDesktop` hands out ids from 100, so the three surfaces are 100
/// (background), 101 (panel) and 102 (popups), in the order `start` creates
/// them.
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
        text: None,
    })
}

/// A press with modifiers held — a desktop shortcut rather than a bare key.
fn chord(k: Key, modifiers: Modifiers) -> guitk::event::Event {
    guitk::event::Event::Key(KeyEvent {
        key: k,
        pressed: true,
        modifiers,
        text: None,
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
fn the_shell_opens_a_background_a_panel_and_a_menu_surface() {
    let (session, desktop) = session();
    let specs = created(&desktop);
    assert_eq!(specs.len(), 3, "a shell is three surfaces, not one");

    // The band each one is in is the load-bearing part: a taskbar in
    // `Layer::Normal` vanishes behind the first window the user opens.
    assert_eq!(specs[0].layer, oswindow::Layer::Background);
    assert_eq!(specs[1].layer, oswindow::Layer::Overlay);
    assert_eq!(specs[2].layer, oswindow::Layer::Overlay);

    // None of them is a window in the ordinary sense.
    for spec in &specs {
        assert!(!spec.decorations, "{} has a title bar", spec.title);
        assert!(
            !spec.resizable,
            "{} can be dragged to a new size",
            spec.title
        );
    }

    // The panel is exactly the taskbar; the other two are the whole display.
    let bar = session.shell().taskbar_rect();
    assert_eq!(specs[1].position, Some((bar.x as i32, bar.y as i32)));
    assert_eq!(specs[1].width, bar.w.round() as u32);
    assert_eq!(specs[1].height, bar.h.round() as u32);
    assert_eq!((specs[0].width, specs[0].height), (2560, 1440));
    assert_eq!((specs[2].width, specs[2].height), (2560, 1440));

    // And the origins the session will translate by say the same thing.
    assert_eq!(session.background().origin(), (0.0, 0.0));
    assert_eq!(session.panel().origin(), (bar.x, bar.y));
    assert_eq!(session.popups().origin(), (0.0, 0.0));
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
    assert!(session.shell().visible_windows().is_empty());

    desktop
        .borrow_mut()
        .send_window_list(&[app(1, "Terminal"), app(2, "notes.txt")]);
    session.pump().expect("pump");

    let titles: Vec<&str> = session
        .shell()
        .visible_windows()
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
    assert_eq!(session.shell().visible_windows().len(), 2);
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
        .visible_windows()
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
        launched[0].starts_with('/'),
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
    assert_eq!(session.shell().visible_windows().len(), 1);
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

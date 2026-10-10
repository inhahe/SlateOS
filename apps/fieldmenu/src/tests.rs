use super::*;
use guitk::editmenu::EditCommand;
use guitk::event::{Key, Modifiers};
use guitk::textinput::TextInput;

const VIEWPORT: (f32, f32) = (800.0, 600.0);

/// A field holding `text`, all of it selected if `selected`.
fn field(text: &str, selected: bool) -> TextInput {
    let mut input = TextInput::new();
    input.set_text(text);
    if selected {
        input.select_all();
    }
    input
}

/// A mouse event of `kind` at `(x, y)`.
fn mouse(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
    MouseEvent { x, y, kind }
}

/// `key`, pressed, nothing held.
fn key(key: Key) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    }
}

/// The index of the row doing `command` in the menu that is up.
fn row(menu: &FieldMenu<u8>, command: EditCommand) -> usize {
    menu.row_of(command)
        .unwrap_or_else(|| panic!("no {command:?} row"))
}

/// The middle of the row doing `command` in the menu that is up.
fn middle(menu: &FieldMenu<u8>, command: EditCommand) -> (f32, f32) {
    menu.row_middle(command).expect("the row is drawn")
}

/// **A row chosen with the left button is handed back with its field**, the
/// menu put away -- and the field, given it, does it.
#[test]
fn a_row_chosen_is_handed_back_with_its_field() {
    let mut input = field("hello", true);
    let mut menu = FieldMenu::new();
    assert!(!menu.is_up());
    menu.open(7_u8, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert_eq!(menu.field(), Some(7));
    let (x, y) = middle(&menu, EditCommand::Delete);
    let Outcome::Chosen(field, id) =
        menu.mouse(&mouse(x, y, MouseEventKind::Press(MouseButton::Left)))
    else {
        panic!("no row was chosen");
    };
    assert_eq!(field, 7);
    assert!(!menu.is_up(), "the menu stayed up");
    let _edit = input.edit_command(id);
    assert_eq!(input.text(), "", "the field did not do the row");
}

/// **A press on a dimmed row leaves the menu up**; a press outside it puts it
/// away and reaches nothing; a right press outside asks for the menu of
/// whatever field is there.
#[test]
fn presses_on_a_dimmed_row_outside_and_with_the_right_button() {
    let input = field("hello", false);
    let mut menu = FieldMenu::new();
    menu.open(1_u8, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    // Nothing selected: Copy is dimmed, Select all lit.
    assert_eq!(menu.row_lit(EditCommand::Copy), Some(false));
    assert_eq!(menu.row_lit(EditCommand::SelectAll), Some(true));
    assert_eq!(
        menu.row_lit(EditCommand::Undo),
        None,
        "a one-line field offered Undo"
    );
    let (x, y) = middle(&menu, EditCommand::Copy);
    assert_eq!(
        menu.mouse(&mouse(x, y, MouseEventKind::Press(MouseButton::Left))),
        Outcome::Taken
    );
    assert!(menu.is_up(), "a press on a dimmed row put the menu away");

    assert_eq!(
        menu.mouse(&mouse(
            700.0,
            500.0,
            MouseEventKind::Press(MouseButton::Left)
        )),
        Outcome::Taken
    );
    assert!(!menu.is_up(), "a press outside left the menu up");

    menu.open(1, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert_eq!(
        menu.mouse(&mouse(
            700.0,
            500.0,
            MouseEventKind::Press(MouseButton::Right)
        )),
        Outcome::RightPressElsewhere(700.0, 500.0)
    );
    assert!(!menu.is_up());

    // A right press on the menu itself is the menu's.
    menu.open(1, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    let (x, y) = middle(&menu, EditCommand::SelectAll);
    assert_eq!(
        menu.mouse(&mouse(x, y, MouseEventKind::Press(MouseButton::Right))),
        Outcome::Taken
    );
    assert!(menu.is_up());
}

/// **Every key is the menu's while it is up**: Escape puts it away, the
/// arrows and Enter choose a row -- and with no menu up, every event is the
/// window's.
#[test]
fn the_keys_are_the_menus_while_it_is_up() {
    let input = field("hello", true);
    let mut menu: FieldMenu<u8> = FieldMenu::new();
    assert_eq!(menu.key(&key(Key::Escape)), Outcome::Window);
    assert_eq!(
        menu.mouse(&mouse(1.0, 1.0, MouseEventKind::Press(MouseButton::Left))),
        Outcome::Window
    );

    menu.open(2, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert_eq!(
        menu.key(&key(Key::A)),
        Outcome::Taken,
        "a letter reached the field"
    );
    assert!(menu.is_up());
    assert_eq!(menu.key(&key(Key::Escape)), Outcome::Taken);
    assert!(!menu.is_up(), "Escape left the menu up");

    menu.open(2, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    let wanted = row(&menu, EditCommand::Copy);
    let mut chosen = None;
    for _ in 0..12 {
        if menu.menu().and_then(ContextMenu::highlighted) == Some(wanted) {
            chosen = Some(menu.key(&key(Key::Enter)));
            break;
        }
        assert_eq!(menu.key(&key(Key::Down)), Outcome::Taken);
    }
    assert_eq!(chosen, Some(Outcome::Chosen(2, EditCommand::Copy.id())));
    assert!(!menu.is_up());
}

/// **It is drawn while it is up and not after**, and the pointer and the
/// wheel over it are its own.
#[test]
fn it_is_drawn_while_up_and_takes_the_pointer() {
    let input = field("hello", true);
    let palette = Palette::for_mode(false);
    let mut menu = FieldMenu::new();
    assert!(menu.render(&palette).is_empty());
    menu.open(3_u8, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert!(
        menu.render(&palette).iter().any(|c| matches!(
            c,
            RenderCommand::Text { text, .. } if text == "Select all"
        )),
        "the menu is not drawn"
    );
    let (x, y) = middle(&menu, EditCommand::Cut);
    assert_eq!(
        menu.mouse(&mouse(x, y, MouseEventKind::Move)),
        Outcome::Taken
    );
    assert_eq!(
        menu.menu().and_then(ContextMenu::highlighted),
        Some(row(&menu, EditCommand::Cut)),
        "the pointer did not light the row under it"
    );
    assert_eq!(
        menu.mouse(&mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy: -1.0 })),
        Outcome::Taken
    );
    menu.close();
    assert!(menu.render(&palette).is_empty());
}

/// **A dimmed row the pointer rests on says why once its wait is over**: the
/// menu asks for a tick while it waits -- never one of nought -- and none
/// once it is away.
#[test]
fn a_dimmed_row_says_why_on_the_clock() {
    let input = field("hello", false);
    let mut menu = FieldMenu::new();
    assert_eq!(menu.due_in(), None);
    // A clock already far along: the menu counts its waits from when they
    // start, not from nought.
    assert!(!menu.tick(90_000));
    menu.open(4_u8, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    let (x, y) = middle(&menu, EditCommand::Copy);
    assert_eq!(
        menu.mouse(&mouse(x, y, MouseEventKind::Move)),
        Outcome::Taken
    );
    let mut waited = 0;
    let mut said = false;
    for _ in 0..40 {
        let Some(wait) = menu.due_in() else {
            break;
        };
        assert!(wait >= 1, "a tick of nought");
        waited += wait;
        if menu.tick(wait) && menu.menu().and_then(ContextMenu::showing_reason).is_some() {
            said = true;
            break;
        }
    }
    assert!(said, "the dimmed row never said why");
    assert!(
        waited >= 400,
        "the reason came after {waited} ms, before the toolkit's delay"
    );
    menu.close();
    assert_eq!(menu.due_in(), None);
    assert!(!menu.tick(1));
}

/// **What is let go, and the pointer leaving, are the window's while the
/// menu is up**: a drag the window had going when the menu came up ends
/// where the button comes up, and its hover goes out -- the menu stays.
#[test]
fn what_is_let_go_and_the_pointer_leaving_are_the_windows() {
    let input = field("hello", true);
    let mut menu = FieldMenu::new();
    menu.open(5_u8, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert_eq!(
        menu.mouse(&mouse(
            300.0,
            300.0,
            MouseEventKind::Release(MouseButton::Left)
        )),
        Outcome::Window,
        "the menu took the release that ends the window's drag"
    );
    assert_eq!(
        menu.mouse(&mouse(-1.0, -1.0, MouseEventKind::Leave)),
        Outcome::Window,
        "the menu took the pointer leaving"
    );
    let mut let_go = key(Key::A);
    let_go.pressed = false;
    assert_eq!(
        menu.key(&let_go),
        Outcome::Window,
        "the menu took a key let go"
    );
    assert!(menu.is_up(), "what passed to the window put the menu away");
}

/// **One call routes an event**: a key or the pointer to the menu, as
/// `key` and `mouse` take them, and anything else to the window -- a tick
/// among them, which `tick` counts.
#[test]
fn one_call_routes_an_event() {
    let input = field("hello", true);
    let mut menu = FieldMenu::new();
    assert_eq!(
        menu.event(&Event::Key(key(Key::Escape))),
        Outcome::Window,
        "with no menu up, a key is the window's"
    );
    menu.open(6_u8, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert_eq!(menu.event(&Event::Key(key(Key::A))), Outcome::Taken);
    assert_eq!(menu.event(&Event::Tick { elapsed_ms: 16 }), Outcome::Window);
    // A move leaves the menu where it was: its place is in the window's own
    // coordinates.
    assert_eq!(menu.event(&Event::Moved { x: 40, y: 30 }), Outcome::Window);
    assert_eq!(
        menu.event(&Event::Mouse(mouse(
            700.0,
            500.0,
            MouseEventKind::Press(MouseButton::Right)
        ))),
        Outcome::RightPressElsewhere(700.0, 500.0)
    );
    assert!(!menu.is_up());
}

/// **The double click after a second press is the menu's and does nothing
/// more**: the press acted already -- here it put up the menu of the field
/// under it -- and a double click taken for another press put that menu
/// away again.
#[test]
fn a_double_click_is_the_menus_and_does_nothing_more() {
    let input = field("hello", true);
    let mut menu = FieldMenu::new();
    menu.open(8_u8, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert_eq!(
        menu.mouse(&mouse(
            700.0,
            500.0,
            MouseEventKind::DoubleClick(MouseButton::Right)
        )),
        Outcome::Taken
    );
    assert!(menu.is_up(), "a double click put the menu away");
    let (x, y) = middle(&menu, EditCommand::Delete);
    assert_eq!(
        menu.mouse(&mouse(x, y, MouseEventKind::DoubleClick(MouseButton::Left))),
        Outcome::Taken,
        "a double click chose a row its press had not"
    );
    assert!(menu.is_up());
}

/// **Left does not put the menu away**: only Escape does. Left answers a
/// submenu's parent, and a field's menu has none.
#[test]
fn left_does_not_put_the_menu_away() {
    let input = field("hello", true);
    let mut menu = FieldMenu::new();
    menu.open(9_u8, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert_eq!(menu.key(&key(Key::Left)), Outcome::Taken);
    assert!(menu.is_up(), "Left put the menu away");
}

/// **The pointer leaving takes the menu's waiting reason with it**: no row
/// waits to say why with the pointer gone from the window.
#[test]
fn the_pointer_leaving_ends_a_rows_wait() {
    let input = field("hello", false);
    let mut menu = FieldMenu::new();
    menu.open(10_u8, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    let (x, y) = middle(&menu, EditCommand::Copy);
    menu.mouse(&mouse(x, y, MouseEventKind::Move));
    assert!(menu.due_in().is_some(), "resting on a dimmed row waits");
    assert_eq!(
        menu.mouse(&mouse(-1.0, -1.0, MouseEventKind::Leave)),
        Outcome::Window
    );
    assert_eq!(
        menu.due_in(),
        None,
        "a row still waits with the pointer gone"
    );
}

/// **The menu goes when the window loses the keyboard, or is resized or
/// rescaled**: left up it took the first keys and press after, at a place
/// that was no longer its. A focus loss that put it away asks for a frame.
#[test]
fn the_menu_goes_with_the_keyboard_or_the_windows_size() {
    let input = field("hello", true);
    let mut menu = FieldMenu::new();
    assert_eq!(menu.event(&Event::FocusOut), Outcome::Window);
    menu.open(11_u8, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert_eq!(menu.event(&Event::FocusOut), Outcome::Taken);
    assert!(!menu.is_up(), "the menu outlasted the keyboard");
    menu.open(11, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert_eq!(
        menu.event(&Event::Resize {
            width: 400,
            height: 300
        }),
        Outcome::Window
    );
    assert!(!menu.is_up(), "the menu outlasted the window's size");
    menu.open(11, input.edit_menu(), 100.0, 100.0, VIEWPORT);
    assert_eq!(
        menu.event(&Event::ScaleChanged { scale: 2.0 }),
        Outcome::Window
    );
    assert!(!menu.is_up(), "the menu outlasted the window's scale");
}

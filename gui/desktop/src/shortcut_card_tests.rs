//! Tests for the card of keyboard shortcuts as a pointer meets it: a row
//! chosen by a click and given new keys by a double click; buttons along its
//! foot that are its keys; the action list chosen from and scrolled; a
//! greyed button that says why -- and a press off the card still reaching
//! what is under it, as before the card answered any.
//!
//! `shortcut_editor`'s own tests hold what each key does; these hold the
//! shell's part: the card laid out once and pressed where it is drawn, a
//! press handed on as the key it stands for, and a change saved.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
// A row's highlight is found by the very numbers its row was laid out with:
// the same arithmetic on the same inputs, so equality is what is meant.
#![allow(clippy::float_cmp)]

use crate::hotkeys::{Hotkey, HotkeyRegistry};
use crate::shortcut_editor::{CardButton, CardLayout, CardSpot, Mode};
use crate::{DesktopShell, MouseButton, MouseEvent, MouseEventKind, Rect, ShellAction};
use guitk::event::{Key, KeyEvent, Modifiers};
use guitk::render::RenderCommand;

/// A shell with the card open, as its chord or the start menu opens it.
fn card_shell() -> DesktopShell {
    let mut shell = DesktopShell::new(1920, 1080);
    shell.toggle_shortcut_card();
    assert!(shell.shortcut_card_open);
    shell
}

/// Where everything on the open card is.
fn layout(shell: &DesktopShell) -> CardLayout {
    shell.shortcut_card_layout().expect("the card is open")
}

fn at(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
    MouseEvent { x, y, kind }
}

/// A click at the middle of `rect`: the press, answered, and its release.
fn click_on(shell: &mut DesktopShell, rect: Rect) -> ShellAction {
    let (x, y) = rect.centre();
    let pressed = shell.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Left)));
    let _released = shell.handle_mouse(&at(x, y, MouseEventKind::Release(MouseButton::Left)));
    pressed
}

/// A double click at the middle of `rect`: a click, then the second press
/// and its release.
fn double_click_on(shell: &mut DesktopShell, rect: Rect) {
    let _first = click_on(shell, rect);
    let (x, y) = rect.centre();
    let _second = shell.handle_mouse(&at(x, y, MouseEventKind::DoubleClick(MouseButton::Left)));
    let _released = shell.handle_mouse(&at(x, y, MouseEventKind::Release(MouseButton::Left)));
}

/// The button `which` along the card's foot: whether it can be pressed, and
/// where it is.
fn button(shell: &DesktopShell, which: CardButton) -> (bool, Rect) {
    layout(shell)
        .buttons
        .iter()
        .find(|(b, _, _)| *b == which)
        .map(|&(_, enabled, at)| (enabled, at))
        .unwrap_or_else(|| panic!("no {which:?} on the card"))
}

fn press(shell: &mut DesktopShell, which: CardButton) {
    let (enabled, at) = button(shell, which);
    assert!(enabled, "{which:?} is greyed");
    assert_eq!(click_on(shell, at), ShellAction::Consumed);
}

/// The buttons along the foot now, by name.
fn buttons(shell: &DesktopShell) -> Vec<CardButton> {
    layout(shell).buttons.iter().map(|&(b, _, _)| b).collect()
}

/// Every piece of text the open card draws.
fn drawn_text(shell: &DesktopShell) -> Vec<String> {
    shell
        .render_shortcut_card()
        .expect("the card is open")
        .commands
        .into_iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// **A click chooses a row of the card, as Up and Down do, and a double
/// click asks for new keys for it, as Enter does.**
#[test]
fn a_click_chooses_a_row_and_a_double_click_asks_for_its_new_keys() {
    let mut shell = card_shell();
    let rows = layout(&shell).rows;
    assert!(rows.len() > 3, "the defaults fill several rows");

    assert_eq!(click_on(&mut shell, rows[2].1), ShellAction::Consumed);
    assert_eq!(shell.shortcut_editor.selected(), 2);
    assert!(!shell.shortcut_editor.is_recording());

    double_click_on(&mut shell, rows[3].1);
    assert_eq!(shell.shortcut_editor.selected(), 3);
    assert!(
        shell.shortcut_editor.is_recording(),
        "a double click on a row records new keys for it"
    );
    // While keys are recorded a row is not what the card waits for.
    let _ = click_on(&mut shell, rows[1].1);
    assert_eq!(shell.shortcut_editor.selected(), 3);
    assert!(shell.shortcut_editor.is_recording());
}

/// **The buttons along the foot are the card's keys**: Remove takes the
/// chosen row off and the change is saved; Add and Change action open the
/// action list, Cancel leaves it unchanged; New keys records, Cancel stops;
/// Close closes the card.
#[test]
fn the_cards_buttons_are_its_keys() {
    settingsfile::testing::with_scratch_config("sc-the-cards-buttons-are-its-keys", |_root| {
        let mut shell = card_shell();
        assert_eq!(
            buttons(&shell),
            [
                CardButton::NewKeys,
                CardButton::ChangeAction,
                CardButton::Remove,
                CardButton::Add,
                CardButton::Close
            ]
        );
        let (keys, at) = layout(&shell).rows[1];
        let _ = click_on(&mut shell, at);
        press(&mut shell, CardButton::Remove);
        assert!(
            shell.hotkeys.conflicts_with(&keys).is_none(),
            "Remove left {} bound",
            keys.display_name()
        );
        let mut fresh = DesktopShell::new(1920, 1080);
        fresh.load_shortcuts();
        assert!(
            fresh.hotkeys.conflicts_with(&keys).is_none(),
            "the change was not saved: the next login has {} back",
            keys.display_name()
        );

        press(&mut shell, CardButton::Add);
        assert!(
            shell.shortcut_editor.is_picking(),
            "Add opens the action list"
        );
        assert_eq!(buttons(&shell), [CardButton::Choose, CardButton::Cancel]);
        press(&mut shell, CardButton::Cancel);
        assert!(!shell.shortcut_editor.is_picking());
        assert_eq!(shell.shortcut_editor.message(), Some("Unchanged"));

        press(&mut shell, CardButton::ChangeAction);
        assert!(
            matches!(shell.shortcut_editor.mode(), Mode::Pick { .. }),
            "Change action opens the action list"
        );
        press(&mut shell, CardButton::Cancel);

        press(&mut shell, CardButton::NewKeys);
        assert!(shell.shortcut_editor.is_recording());
        assert_eq!(buttons(&shell), [CardButton::Cancel]);
        press(&mut shell, CardButton::Cancel);
        assert!(!shell.shortcut_editor.is_recording());

        press(&mut shell, CardButton::Close);
        assert!(!shell.shortcut_card_open, "Close closes the card");
    });
}

/// **Keys that already do something else are moved or kept by a button,
/// as by Enter or Escape.**
#[test]
fn the_question_over_taken_keys_is_answered_by_its_buttons() {
    settingsfile::testing::with_scratch_config("sc-the-question-over-taken-keys", |_root| {
        let mut shell = card_shell();
        let mut rows = shell.hotkeys.all_bindings().map(|(h, a)| (*h, a.clone()));
        let (first, first_action) = rows.next().expect("a first binding");
        // The next on a real key: a recording waits through a bare modifier
        // -- the user still assembling a chord -- and the defaults bind
        // Super alone, second in the list.
        let (second, second_action) = rows
            .find(|(keys, _)| {
                !matches!(
                    keys.key,
                    Key::LeftCtrl
                        | Key::RightCtrl
                        | Key::LeftAlt
                        | Key::RightAlt
                        | Key::LeftShift
                        | Key::RightShift
                        | Key::LeftSuper
                        | Key::RightSuper
                )
            })
            .expect("a second binding on a real key");
        drop(rows);
        let taken = |shell: &mut DesktopShell| {
            let at = layout(shell)
                .rows
                .iter()
                .find(|(keys, _)| *keys == first)
                .map(|&(_, at)| at)
                .expect("the first row");
            double_click_on(shell, at);
            drop(shell.handle_hotkey(&KeyEvent {
                key: second.key,
                pressed: true,
                modifiers: second.modifiers(),
                text: String::new(),
            }));
        };

        taken(&mut shell);
        assert!(matches!(shell.shortcut_editor.mode(), Mode::Steal { .. }));
        assert_eq!(buttons(&shell), [CardButton::Move, CardButton::Keep]);
        press(&mut shell, CardButton::Keep);
        assert_eq!(shell.hotkeys.conflicts_with(&second), Some(&second_action));
        assert_eq!(shell.hotkeys.conflicts_with(&first), Some(&first_action));

        taken(&mut shell);
        press(&mut shell, CardButton::Move);
        assert_eq!(
            shell.hotkeys.conflicts_with(&second),
            Some(&first_action),
            "Move gives the keys to the row's action"
        );
    });
}

/// **The action list is chosen from with the pointer and scrolled by the
/// wheel**: a click chooses an entry, a double click takes it; the wheel
/// moves the list and leaves the choice.
#[test]
fn the_action_list_is_chosen_from_and_scrolled_with_the_pointer() {
    let mut shell = card_shell();
    press(&mut shell, CardButton::Add);
    let chosen = |shell: &DesktopShell| match shell.shortcut_editor.mode() {
        Mode::Pick { view, .. } => view.selected(),
        _ => panic!("not choosing an action"),
    };
    let entries = layout(&shell).entries;
    assert!(entries.len() >= 3, "{entries:?}");
    let _ = click_on(&mut shell, entries[2].1);
    assert_eq!(chosen(&shell), Some(entries[2].0));

    // The wheel, towards the user: further down the list.
    let list = layout(&shell).list.expect("the action list");
    let (x, y) = list.centre();
    for _ in 0..3 {
        assert_eq!(
            shell.handle_mouse(&at(x, y, MouseEventKind::Scroll { dx: 0.0, dy: -1.0 })),
            ShellAction::Consumed
        );
    }
    let scrolled = layout(&shell).entries;
    assert!(
        scrolled[0].0 > entries[0].0,
        "the wheel did not scroll the list: {scrolled:?}"
    );
    assert_eq!(
        chosen(&shell),
        Some(entries[2].0),
        "the wheel moved the choice"
    );

    // "Run a program..." is first: back to the top, and take it.
    for _ in 0..30 {
        let _ = shell.handle_mouse(&at(x, y, MouseEventKind::Scroll { dx: 0.0, dy: 1.0 }));
    }
    let first = layout(&shell).entries[0];
    assert_eq!(first.0, 0);
    double_click_on(&mut shell, first.1);
    assert!(
        matches!(shell.shortcut_editor.mode(), Mode::Command { .. }),
        "a double click on \"Run a program\" asks for the program"
    );
    assert_eq!(buttons(&shell), [CardButton::Ok, CardButton::Cancel]);
}

/// **A greyed button does nothing, and says why while the pointer rests
/// on it** (`design-decisions.md` §1473): with no shortcuts there is no row
/// for New keys to act on.
#[test]
fn a_greyed_button_does_nothing_and_says_why() {
    let mut shell = DesktopShell::new(1920, 1080);
    shell.hotkeys = HotkeyRegistry::new();
    shell.toggle_shortcut_card();
    let (enabled, at) = button(&shell, CardButton::NewKeys);
    assert!(!enabled, "New keys with no row to act on");
    let _ = click_on(&mut shell, at);
    assert!(!shell.shortcut_editor.is_recording());

    let why = CardButton::NewKeys.why_disabled();
    let (x, y) = at.centre();
    let _ = shell.handle_mouse(&at_point(x, y));
    assert!(
        !drawn_text(&shell).iter().any(|t| t == why),
        "the reason came before the tooltip's delay"
    );
    assert!(shell.tooltip_due_in().is_some(), "nothing to wake for");
    shell.advance_osd(1_000);
    assert!(
        drawn_text(&shell).iter().any(|t| t == why),
        "{:?}",
        drawn_text(&shell)
    );
    // Off it, the reason goes.
    let _ = shell.handle_mouse(&at_point(at.x - 40.0, at.y - 60.0));
    assert!(!drawn_text(&shell).iter().any(|t| t == why));
}

fn at_point(x: f32, y: f32) -> MouseEvent {
    at(x, y, MouseEventKind::Move)
}

/// **The card is pressed where it is drawn**: each button's face, the
/// chosen row's highlight and the foot's line are where the layout says.
#[test]
fn the_card_is_pressed_where_it_is_drawn() {
    let mut shell = card_shell();
    let second = layout(&shell).rows[1].1;
    let _ = click_on(&mut shell, second);
    let laid = layout(&shell);
    let fills: Vec<Rect> = shell
        .render_shortcut_card()
        .expect("open")
        .commands
        .into_iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } => Some(Rect::new(x, y, width, height)),
            _ => None,
        })
        .collect();
    for (which, _, at) in &laid.buttons {
        assert!(
            fills.contains(at),
            "{which:?}'s face is not drawn where it is pressed: {at:?}"
        );
        assert_eq!(
            laid.at(at.centre().0, at.centre().1),
            Some(CardSpot::Button(*which, true))
        );
    }
    let row = laid.rows[1].1;
    assert!(
        fills
            .iter()
            .any(|f| f.x == row.x && f.w == row.w && f.y > row.y && f.bottom() < row.bottom()),
        "the chosen row's highlight is not in its row {row:?}"
    );
    // Nothing on the foot over the rows, nor a button over the line.
    let last_row = laid
        .rows
        .iter()
        .map(|(_, r)| r.bottom())
        .fold(0.0, f32::max);
    assert!(
        laid.message.y >= last_row - 0.01,
        "the line is over the rows"
    );
    for (which, _, at) in &laid.buttons {
        assert!(
            at.y >= laid.message.bottom() - 0.01,
            "{which:?} is over the line"
        );
        assert!(
            at.bottom() <= laid.card.bottom(),
            "{which:?} is off the card"
        );
    }
}

/// **A press off the card still reaches what is under it** -- an icon is
/// chosen with the card up -- and one on the card is the card's alone.
#[test]
fn a_press_off_the_card_still_reaches_the_desktop() {
    let mut shell = card_shell();
    shell.icons.populate_defaults();
    let icon = shell.icons.icon_ids()[0];
    let rect = shell.icons.icon_rect(icon).expect("on the desktop");
    assert!(
        layout(&shell)
            .at(rect.centre().0, rect.centre().1)
            .is_none(),
        "the icon is under the card"
    );
    let _ = click_on(&mut shell, rect);
    assert_eq!(shell.icons.selected_ids(), vec![icon]);
    assert!(shell.shortcut_card_open, "the card stays up");

    let foot = layout(&shell).message;
    assert_eq!(click_on(&mut shell, foot), ShellAction::Consumed);
    assert_eq!(
        shell.icons.selected_ids(),
        vec![icon],
        "the press went past the card"
    );
}

/// **Everything the card draws is the user's theme's**, light and dark,
/// under an accent of the user's, in each thing the card can be doing: the
/// list with a button lit under the pointer, a recording, the action list,
/// one with nothing to choose and the reason its greyed Choose gives. The
/// buttons' faces are blended from the palette by the toolkit's button,
/// against the card's ground: declared as what they are.
#[test]
fn everything_the_card_draws_is_the_users_theme() {
    use appearance::palette_check::assert_drawn_from;
    use guitk::button::{Kind, State, paint};
    for mode in [appearance::ThemeMode::Light, appearance::ThemeMode::Dark] {
        let mut shell = DesktopShell::new(1920, 1080);
        shell.appearance.theme_mode = mode;
        shell.appearance.accent_color = appearance::AccentColor::Green;
        shell.toggle_shortcut_card();
        let p = appearance::Palette::from_settings(&shell.appearance);
        let mut derived = Vec::new();
        for kind in [Kind::Plain, Kind::Primary, Kind::Destructive] {
            for (hovered, disabled) in [(false, false), (true, false), (false, true)] {
                let c = paint(
                    &p,
                    kind,
                    State {
                        hovered,
                        disabled,
                        ..State::default()
                    },
                    p.panel_bg(),
                );
                derived.extend([c.upper, c.lower, c.edge, c.ink]);
            }
        }
        let check = |shell: &DesktopShell, what: &str| {
            let cmds = shell.render_shortcut_card().expect("open").commands;
            assert!(cmds.len() > 20, "{what}: the card drew almost nothing");
            assert_drawn_from(&p, &cmds, &derived, &format!("{mode:?}: {what}"));
        };
        let (_, close) = button(&shell, CardButton::Close);
        let _ = shell.handle_mouse(&at_point(close.centre().0, close.centre().1));
        check(&shell, "the list, Close lit");
        press(&mut shell, CardButton::NewKeys);
        check(&shell, "a recording");
        press(&mut shell, CardButton::Cancel);
        press(&mut shell, CardButton::Add);
        check(&shell, "the action list");
        for k in "zzzz".chars() {
            drop(shell.handle_hotkey(&KeyEvent {
                key: Key::Unknown(0),
                pressed: true,
                modifiers: Modifiers::NONE,
                text: k.to_string(),
            }));
        }
        let (enabled, choose) = button(&shell, CardButton::Choose);
        assert!(!enabled, "nothing matches zzzz");
        let _ = shell.handle_mouse(&at_point(choose.centre().0, choose.centre().1));
        shell.advance_osd(1_000);
        assert!(
            drawn_text(&shell)
                .iter()
                .any(|t| t == CardButton::Choose.why_disabled()),
            "the reason is not up"
        );
        check(&shell, "nothing to choose, and why");
    }
}

/// **The buttons are drawn as what they do**: New keys -- what Enter does
/// on the list -- as what the card is for, Remove in the red of what is
/// taken away, the rest plain; the one under the pointer lit, and no
/// longer once the pointer leaves it.
#[test]
fn the_buttons_are_drawn_as_what_they_do() {
    use guitk::button::{Kind, State, paint};
    let mut shell = card_shell();
    let p = appearance::Palette::from_settings(&shell.appearance);
    let face = |shell: &DesktopShell, which: CardButton| {
        let (_, at) = button(shell, which);
        shell
            .render_shortcut_card()
            .expect("open")
            .commands
            .into_iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if Rect::new(x, y, width, height) == at => Some(color),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{which:?} has no face"))
    };
    let at_rest = |kind| paint(&p, kind, State::default(), p.panel_bg()).lower;
    for (which, kind) in [
        (CardButton::NewKeys, Kind::Primary),
        (CardButton::ChangeAction, Kind::Plain),
        (CardButton::Remove, Kind::Destructive),
        (CardButton::Add, Kind::Plain),
        (CardButton::Close, Kind::Plain),
    ] {
        assert_eq!(face(&shell, which), at_rest(kind), "{which:?}");
    }
    let lit = paint(
        &p,
        Kind::Plain,
        State {
            hovered: true,
            ..State::default()
        },
        p.panel_bg(),
    )
    .lower;
    assert_ne!(
        lit,
        at_rest(Kind::Plain),
        "a lit face looks like one at rest"
    );
    let (_, add) = button(&shell, CardButton::Add);
    let _ = shell.handle_mouse(&at_point(add.centre().0, add.centre().1));
    assert_eq!(face(&shell, CardButton::Add), lit, "Add under the pointer");
    assert_eq!(face(&shell, CardButton::Close), at_rest(Kind::Plain));
    let _ = shell.handle_mouse(&at_point(add.x - 300.0, add.y - 300.0));
    assert_eq!(
        face(&shell, CardButton::Add),
        at_rest(Kind::Plain),
        "the pointer left"
    );
}

/// **An icon carried onto the card is put down there**: the card takes the
/// presses and the wheel that land on it, and the motion and the release of
/// a drag begun off it go on to the icon -- taken by the card, the icon was
/// never let go.
#[test]
fn an_icon_carried_onto_the_card_is_put_down() {
    settingsfile::testing::with_scratch_config("sc-an-icon-carried-onto-the-card", |_root| {
        let mut shell = card_shell();
        shell.icons.populate_defaults();
        let icon = shell.icons.icon_ids()[0];
        let before = shell.icons.icon_rect(icon).expect("on the desktop");
        let (x, y) = before.centre();
        assert!(
            layout(&shell).at(x, y).is_none(),
            "the icon is under the card"
        );
        let onto = layout(&shell).message.centre();

        let _ = shell.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Left)));
        let _ = shell.handle_mouse(&at_point(x + 40.0, y + 40.0));
        let _ = shell.handle_mouse(&at_point(onto.0, onto.1));
        let _ = shell.handle_mouse(&at(
            onto.0,
            onto.1,
            MouseEventKind::Release(MouseButton::Left),
        ));
        assert!(
            !shell.icons.is_interacting(),
            "the release over the card did not end the icon's drag"
        );
        assert_ne!(
            shell.icons.icon_rect(icon),
            Some(before),
            "the icon was not carried"
        );
        assert!(shell.shortcut_card_open, "and the card stays up");
    });
}

/// **On a short display the card still fits, its foot on it**: the rows
/// fold into more columns above the foot rather than under it, and the
/// card keeps the margin the screen leaves it.
#[test]
fn on_a_short_display_the_rows_fold_above_the_foot() {
    for tall in [480_u32, 600, 800] {
        let mut shell = DesktopShell::new(1920, tall);
        shell.toggle_shortcut_card();
        let laid = layout(&shell);
        #[allow(clippy::cast_precision_loss, reason = "a display's height")]
        let room = tall as f32 - crate::SHORTCUT_CARD_MARGIN * 2.0;
        assert!(
            laid.card.h <= room + 0.01,
            "{tall}: the card is {} tall in {room}",
            laid.card.h
        );
        for (keys, r) in &laid.rows {
            assert!(
                r.bottom() <= laid.message.y + 0.01,
                "{tall}: {} is under the foot",
                keys.display_name()
            );
        }
        for (which, _, r) in &laid.buttons {
            assert!(r.bottom() <= laid.card.bottom() + 0.01, "{tall}: {which:?}");
        }
    }
}

/// **A shortcut is named as it is written** -- what a tool gives the card
/// while it records keys -- and every row's keys read back as they parse.
#[test]
fn every_rows_keys_are_named_as_they_parse() {
    let shell = card_shell();
    for (keys, _) in shell.hotkeys.all_bindings() {
        assert_eq!(Hotkey::parse(&keys.display_name()).ok(), Some(*keys));
    }
    assert!(
        Hotkey::parse("Ctrl+Alt").is_err(),
        "modifiers alone are no keys"
    );
    assert!(Hotkey::parse("Ctrl+Nonsense").is_err());
    assert_eq!(
        Hotkey::parse("Ctrl+Alt+T").ok(),
        Some(Hotkey::new(
            Key::T,
            Modifiers {
                ctrl: true,
                alt: true,
                ..Modifiers::NONE
            }
        ))
    );
}

//! Tests of the Font Manager's window, over a machine's fonts built in a
//! scratch folder (`library::tests::Machine`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use crate::library::tests::{Machine, font};
use guitk::event::Modifiers;

fn manager(machine: &Machine) -> FontManager {
    FontManager::new(machine.places())
}

/// Every text the window draws.
fn texts(state: &FontManager) -> Vec<String> {
    state
        .render_tree()
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn names(state: &FontManager) -> Vec<String> {
    state.visible().iter().map(|f| f.name.clone()).collect()
}

fn key_with(key: Key, modifiers: Modifiers) -> Event {
    Event::Key(KeyEvent {
        key,
        pressed: true,
        modifiers,
        text: String::new(),
    })
}

fn key(key: Key) -> Event {
    key_with(key, Modifiers::NONE)
}

fn ctrl(key: Key) -> Event {
    key_with(
        key,
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    )
}

fn type_text(state: &mut FontManager, text: &str) {
    for ch in text.chars() {
        state.handle_event(&Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        }));
    }
}

fn press(state: &mut FontManager, rect: Rect) -> EventResult {
    state.handle_event(&Event::Mouse(MouseEvent {
        x: rect.x + rect.w / 2.0,
        y: rect.y + rect.h / 2.0,
        kind: MouseEventKind::Press(MouseButton::Left),
    }))
}

/// **The list is the machine's fonts**, and nothing else -- this program
/// listed nineteen it made up -- with the first chosen and a count in the
/// status line.
#[test]
fn the_list_is_the_machines_fonts() {
    let machine = Machine::new("fontmanager-window-list");
    let state = manager(&machine);
    assert_eq!(names(&state), ["Alpha", "Beta", "Gamma"]);
    assert_eq!(state.chosen().map(|f| f.name.as_str()), Some("Alpha"));
    let drawn = texts(&state);
    for name in ["Alpha", "Beta", "Gamma", "3 font families installed."] {
        assert!(
            drawn.iter().any(|t| t == name),
            "{name:?} is not drawn: {drawn:?}"
        );
    }
    assert!(
        !drawn.iter().any(|t| t == "DejaVu Sans" || t == "Noto Sans"),
        "an invented font is listed"
    );
}

/// **The sidebar shows all fonts, the user's own, or the fixed-pitch ones**,
/// each with how many, and the choice follows what is shown.
#[test]
fn the_sidebar_filters_the_list() {
    let machine = Machine::new("fontmanager-window-filters");
    let mut state = manager(&machine);
    let filters = state.layout().filters;
    press(&mut state, filters[1].0);
    assert_eq!(state.filter, Filter::Yours);
    assert_eq!(names(&state), ["Gamma"]);
    assert_eq!(
        state.chosen().map(|f| f.name.as_str()),
        Some("Gamma"),
        "the choice was left on a font not shown"
    );
    press(&mut state, filters[2].0);
    assert_eq!(names(&state), ["Beta"]);
    press(&mut state, filters[0].0);
    assert_eq!(names(&state).len(), 3);
    let drawn = texts(&state);
    for (label, count) in [("All fonts", "3"), ("Yours", "1"), ("Fixed-pitch", "1")] {
        let at = drawn.iter().position(|t| t == label).unwrap();
        assert_eq!(drawn[at + 1], count, "{label} does not say how many");
    }
}

/// **The search box is typed into, and the list follows it** -- Ctrl+F or a
/// press takes the keyboard there, Escape leaves it, and Escape again empties
/// it.
#[test]
fn the_search_box_finds_fonts_by_name() {
    let machine = Machine::new("fontmanager-window-search");
    let mut state = manager(&machine);
    state.handle_event(&ctrl(Key::F));
    assert!(state.search_focused);
    type_text(&mut state, "AM");
    assert_eq!(names(&state), ["Gamma"], "the search is not in any case");
    assert_eq!(state.chosen().map(|f| f.name.as_str()), Some("Gamma"));
    type_text(&mut state, "x");
    assert!(names(&state).is_empty());
    assert!(
        texts(&state)
            .iter()
            .any(|t| t == "No font's name has that in it.")
    );
    state.handle_event(&key(Key::Escape));
    assert!(!state.search_focused, "Escape did not leave the search");
    state.handle_event(&key(Key::Escape));
    assert_eq!(state.search.text(), "", "Escape again did not empty it");
    assert_eq!(names(&state).len(), 3);

    let field = state.layout().search;
    press(&mut state, field);
    assert!(
        state.search_focused,
        "a press on the box did not give it the keyboard"
    );
    let row = state.layout().rows[0].0;
    press(&mut state, row);
    assert!(
        !state.search_focused,
        "a press elsewhere left the box the keyboard"
    );
}

/// **The arrows, Home and End move through the fonts**, and a press on a
/// row chooses it.
#[test]
fn the_keys_and_the_pointer_choose_a_font() {
    let machine = Machine::new("fontmanager-window-keys");
    let mut state = manager(&machine);
    let chosen = |state: &FontManager| state.chosen().map(|f| f.name.clone());
    state.handle_event(&key(Key::Down));
    assert_eq!(chosen(&state).as_deref(), Some("Beta"));
    state.handle_event(&key(Key::End));
    assert_eq!(chosen(&state).as_deref(), Some("Gamma"));
    state.handle_event(&key(Key::Down));
    assert_eq!(
        chosen(&state).as_deref(),
        Some("Gamma"),
        "Down past the last moved"
    );
    state.handle_event(&key(Key::Home));
    assert_eq!(chosen(&state).as_deref(), Some("Alpha"));
    state.handle_event(&key(Key::Up));
    assert_eq!(
        chosen(&state).as_deref(),
        Some("Alpha"),
        "Up past the first moved"
    );
    let rows = state.layout().rows;
    press(&mut state, rows[1].0);
    assert_eq!(chosen(&state).as_deref(), Some("Beta"));
}

/// **The panel says what the font chosen is**: its styles, whether it is
/// fixed-pitch, whose, and its files -- and offers Remove only for the
/// user's own, saying why not for the system's.
#[test]
fn the_panel_says_what_the_font_is() {
    let machine = Machine::new("fontmanager-window-panel");
    let mut state = manager(&machine);
    let drawn = texts(&state);
    for said in ["Regular, Bold", "No", "The system's"] {
        assert!(
            drawn.iter().any(|t| t == said),
            "{said:?} is not said: {drawn:?}"
        );
    }
    assert!(
        state.layout().remove.is_none(),
        "Remove is offered for the system's fonts"
    );
    assert!(
        drawn
            .iter()
            .any(|t| t == "The system's fonts are shared by every account.")
    );
    let alpha_file = machine.system.join("Alpha-Bold.ttf");
    assert!(
        drawn
            .iter()
            .any(|t| *t == alpha_file.as_path().shown().to_string()),
        "the files are not listed"
    );

    state.handle_event(&key(Key::End));
    let drawn = texts(&state);
    for said in ["Italic", "Yours"] {
        assert!(drawn.iter().any(|t| t == said), "{said:?} is not said");
    }
    assert!(
        state.layout().remove.is_some(),
        "Remove is not offered for the user's own"
    );
}

/// **The font chosen is shown in its own letters**: pictures drawn with its
/// face, uploaded before the frame that draws them, given back when another
/// font is chosen or the colours change, and not made again for nothing.
#[test]
fn the_font_chosen_is_shown_in_its_own_letters() {
    let machine = Machine::new("fontmanager-window-preview");
    let mut state = manager(&machine);
    let tree = App::render(&mut state, DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT);
    let uploads: Vec<u64> = App::take_images(&mut state)
        .into_iter()
        .filter_map(|change| match change {
            ImageChange::Upload { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(uploads.len(), PREVIEW_LINES.len(), "a line was not drawn");
    let drawn: Vec<u64> = tree
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Image { image_id, .. } => Some(*image_id),
            _ => None,
        })
        .collect();
    assert_eq!(drawn, uploads, "the frame draws pictures it did not upload");

    App::render(&mut state, DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT);
    assert!(
        App::take_images(&mut state).is_empty(),
        "the same pictures were made again"
    );

    state.handle_event(&key(Key::Down));
    App::render(&mut state, DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT);
    let changes = App::take_images(&mut state);
    let dropped: Vec<u64> = changes
        .iter()
        .filter_map(|c| match c {
            ImageChange::Drop(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(dropped, uploads, "the last font's pictures were kept");
    let first_upload = changes
        .iter()
        .position(|c| matches!(c, ImageChange::Upload { .. }))
        .expect("the new font's pictures");
    let last_drop = changes
        .iter()
        .rposition(|c| matches!(c, ImageChange::Drop(_)))
        .unwrap();
    assert!(
        last_drop < first_upload,
        "a picture went up before the old ones came down"
    );

    let mut darker = state.palette;
    darker.mantle = Color::rgba(1, 2, 3, 255);
    App::theme_changed(&mut state, &darker);
    App::render(&mut state, DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT);
    assert!(
        App::take_images(&mut state)
            .iter()
            .any(|c| matches!(c, ImageChange::Upload { .. })),
        "the pictures keep the old panel's colour"
    );
}

/// **A font file chosen in the picker is installed** -- into the user's
/// fonts folder, chosen in the list, and said -- and one that is not a font
/// is refused with why, and nothing is written.
#[test]
fn a_font_file_is_installed_from_the_picker() {
    let machine = Machine::new("fontmanager-window-install");
    let mut state = manager(&machine);
    state.handle_event(&ctrl(Key::O));
    assert!(state.dialog.is_some(), "Ctrl+O put no picker up");
    let from = machine.download("Delta.ttf", &font("Delta", false, false, false));
    state.apply_dialog_answer(DialogAction::Selected(from));
    assert!(state.dialog.is_none());
    assert!(
        machine.own.join("Delta.ttf").exists(),
        "nothing was written"
    );
    assert_eq!(state.chosen().map(|f| f.name.as_str()), Some("Delta"));
    let message = state.message.clone().expect("said");
    assert!(!message.failed);
    assert_eq!(
        message.text,
        "Installed Delta Regular. Programs started from now on can use it."
    );

    let junk = machine.download("junk.ttf", b"not a font");
    state.apply_dialog_answer(DialogAction::Selected(junk));
    let message = state.message.clone().expect("said");
    assert!(message.failed);
    assert_eq!(
        message.text,
        "junk.ttf was not installed: it is not a font this system can draw."
    );
    assert!(!machine.own.join("junk.ttf").exists());

    let install = state.layout().install;
    press(&mut state, install);
    assert!(state.dialog.is_some(), "the button put no picker up");
    state.apply_dialog_answer(DialogAction::Cancelled);
    assert!(state.dialog.is_none());
}

/// **The user's own font is removed after asking**: Delete or the button
/// asks, Keep it and Escape keep it, Remove and Enter delete its files -- and
/// the system's is not offered.
#[test]
fn your_font_is_removed_after_asking() {
    let machine = Machine::new("fontmanager-window-remove");
    let gamma = machine.own.join("Gamma.ttf");
    let mut state = manager(&machine);

    state.handle_event(&key(Key::Delete));
    assert!(
        state.confirm.is_none(),
        "removing a system font was asked about"
    );

    state.handle_event(&key(Key::End));
    state.handle_event(&key(Key::Delete));
    let removal = state.confirm.clone().expect("asked");
    assert_eq!(removal.files, std::slice::from_ref(&gamma));
    assert!(texts(&state).iter().any(|t| t == "Remove Gamma?"));
    state.handle_event(&key(Key::Escape));
    assert!(
        state.confirm.is_none() && gamma.exists(),
        "Escape removed it"
    );

    let remove = state.layout().remove.expect("offered");
    press(&mut state, remove);
    let (_, keep) = state.layout().confirm.expect("asked");
    press(&mut state, keep);
    assert!(
        state.confirm.is_none() && gamma.exists(),
        "Keep it removed it"
    );

    press(&mut state, remove);
    let (yes, _) = state.layout().confirm.expect("asked");
    press(&mut state, yes);
    assert!(!gamma.exists(), "the file is still there");
    assert_eq!(names(&state), ["Alpha", "Beta"]);
    let message = state.message.clone().expect("said");
    assert_eq!(
        message.text,
        "Removed Gamma: 1 file deleted. Programs started from now on will not have it."
    );

    // Enter answers yes.
    let again = machine.download("Gamma.ttf", &font("Gamma", false, true, false));
    state.apply_dialog_answer(DialogAction::Selected(again));
    state.handle_event(&key(Key::Delete));
    assert!(state.confirm.is_some());
    state.handle_event(&key(Key::Enter));
    assert!(!machine.own.join("Gamma.ttf").exists());
}

/// **A long list scrolls under the wheel**, and no further than its rows.
#[test]
fn a_long_list_scrolls() {
    let machine = Machine::new("fontmanager-window-scroll");
    for n in 0..40 {
        std::fs::write(
            machine.system.join(format!("Many{n:02}.ttf")),
            font(&format!("Many {n:02}"), false, false, false),
        )
        .unwrap();
    }
    let mut state = manager(&machine);
    let list = state.layout().list;
    let wheel = |state: &mut FontManager, dy| {
        state.handle_event(&Event::Mouse(MouseEvent {
            x: list.x + 20.0,
            y: list.y + 20.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        }))
    };
    assert_eq!(
        wheel(&mut state, 1.0),
        EventResult::Ignored,
        "the top scrolled up"
    );
    assert_eq!(wheel(&mut state, -1.0), EventResult::Consumed);
    assert!(state.list_scroll > 0.0);
    for _ in 0..200 {
        wheel(&mut state, -1.0);
    }
    let rows = state.visible().len() as f32;
    assert!((state.list_scroll - (rows * ROW_HEIGHT - list.h)).abs() < 0.01);
    let last = state.layout().rows.last().map(|(_, index)| *index);
    assert_eq!(
        last,
        Some(state.visible().len() - 1),
        "the last row is not reached"
    );

    state.handle_event(&key(Key::Home));
    assert_eq!(
        state.list_scroll, 0.0,
        "the first font was chosen off screen"
    );
}

/// **Every key the card advertises is answered by this window.**
#[test]
fn every_advertised_key_does_something() {
    for (label, what) in SHORTCUTS {
        for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
            let answered = (0..3).any(|setup| {
                let machine = Machine::new("fontmanager-window-card");
                let mut state = manager(&machine);
                match setup {
                    // The user's own font chosen, for Delete.
                    1 => {
                        state.handle_event(&key(Key::End));
                    }
                    // Something searched, for Escape.
                    2 => {
                        state.search.set_text("a");
                    }
                    _ => {}
                }
                state.handle_key(&stroke) == EventResult::Consumed
            });
            assert!(
                answered,
                "the card advertises {label:?} for {what:?}, and nothing answers {:?}",
                stroke.key
            );
        }
    }
}

/// **The card is modal**: F1 puts it up, and a press or Escape puts it away
/// without reaching what is under it.
#[test]
fn the_card_is_modal() {
    let machine = Machine::new("fontmanager-window-card-modal");
    let mut state = manager(&machine);
    state.handle_event(&key(Key::F1));
    assert!(state.show_help);
    state.handle_event(&key(Key::Down));
    assert_eq!(
        state.chosen().map(|f| f.name.as_str()),
        Some("Alpha"),
        "a key went under the card"
    );
    let rows = state.layout().rows;
    assert_eq!(
        press(&mut state, rows[2].0),
        EventResult::Consumed,
        "putting the card away asked for no frame, so it stays drawn"
    );
    assert!(!state.show_help);
    assert_eq!(
        state.chosen().map(|f| f.name.as_str()),
        Some("Alpha"),
        "the press went under the card"
    );
}

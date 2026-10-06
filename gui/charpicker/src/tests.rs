#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;
use guitk::event::Modifiers;

const W: f32 = 560.0;
const H: f32 = 460.0;

fn press(key: Key) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    }
}

fn shifted(key: Key) -> KeyEvent {
    KeyEvent {
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        ..press(key)
    }
}

fn typed(text: &str) -> KeyEvent {
    KeyEvent {
        key: Key::A,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: text.to_string(),
    }
}

/// Type `text` into the picker, a character at a time.
fn type_text(picker: &mut CharPicker, text: &str) {
    for c in text.chars() {
        assert_eq!(picker.handle_key(&typed(&c.to_string()), W, H), None);
    }
}

fn key(picker: &mut CharPicker, k: Key) -> Option<CharPickerEvent> {
    picker.handle_key(&press(k), W, H)
}

fn mouse(picker: &mut CharPicker, x: f32, y: f32, kind: MouseEventKind) -> Option<CharPickerEvent> {
    picker.handle_mouse(&MouseEvent { x, y, kind }, W, H)
}

/// The centre of what `target` was drawn as.
fn centre_of(picker: &CharPicker, target: Target) -> (f32, f32) {
    let frame = picker.frame(&Palette::for_mode(false), W, H);
    let r = frame
        .rect_of(|t| *t == target)
        .unwrap_or_else(|| panic!("{target:?} is not drawn"));
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn click(picker: &mut CharPicker, target: Target) -> Option<CharPickerEvent> {
    let (x, y) = centre_of(picker, target);
    mouse(picker, x, y, MouseEventKind::Press(MouseButton::Left))
}

fn picked(text: &str) -> Option<CharPickerEvent> {
    Some(CharPickerEvent::Picked(text.to_string()))
}

/// The sidebar's row for `category`.
fn row_of(picker: &CharPicker, category: Category) -> usize {
    picker
        .side
        .iter()
        .position(|row| *row == SideRow::Category(category))
        .unwrap()
}

/// **A picker opens on the first emoji group with nothing picked yet, the
/// search field taking the keyboard.**
#[test]
fn a_picker_opens_on_the_smileys() {
    let picker = CharPicker::new();
    assert_eq!(picker.category(), Category::Emoji(0));
    assert_eq!(picker.category().name(), "Smileys & Emotion");
    assert_eq!(picker.focus(), Part::Search);
    assert_eq!(picker.shown().next().as_deref(), Some("\u{1F600}"));
    assert_eq!(picker.cursor(), None);
    assert!(picker.recent().is_empty());
    assert_eq!(picker.tone(), None);
}

/// **The sidebar holds the recent picks, the emoji groups and the other
/// characters, under headings -- and not the emoji "Component" group, whose
/// swatches are parts of emoji rather than emoji.**
#[test]
fn the_sidebar_lists_the_categories() {
    let picker = CharPicker::new();
    let names: Vec<&str> = picker
        .side
        .iter()
        .map(|row| match row {
            SideRow::Heading(name) => *name,
            SideRow::Category(category) => category.name(),
        })
        .collect();
    assert_eq!(names[..3], ["Recent", "Emoji", "Smileys & Emotion"]);
    assert!(!names.contains(&"Component"));
    let characters = names.iter().position(|n| *n == "Characters").unwrap();
    assert_eq!(
        names[characters + 1..],
        [
            "Symbols", "Math", "Arrows", "Currency", "Latin", "Greek", "Cyrillic"
        ]
    );
    // It opens at its top, Recent in sight, though the category chosen is
    // the third row.
    let frame = picker.frame(&Palette::for_mode(false), W, H);
    assert!(
        frame.rect_of(|t| *t == Target::Category(0)).is_some(),
        "Recent is out of sight"
    );
    let mut picker = picker;
    key(&mut picker, Key::Tab);
    assert_eq!(picker.side_view.first_visible(), 0);
}

/// **The keyboard moves through the categories, choosing as it goes and
/// stepping over the headings.**
#[test]
fn the_keyboard_chooses_categories() {
    let mut picker = CharPicker::new();
    key(&mut picker, Key::Tab);
    assert_eq!(picker.focus(), Part::Categories);
    key(&mut picker, Key::Up);
    // Over the "Emoji" heading to Recent.
    assert_eq!(picker.category(), Category::Recent);
    key(&mut picker, Key::Down);
    assert_eq!(picker.category(), Category::Emoji(0));
    key(&mut picker, Key::End);
    assert_eq!(picker.category(), Category::Characters("Cyrillic"));
    assert!(picker.shown().any(|t| t == "\u{416}"), "ZHE");
    // Up from Symbols steps over the "Characters" heading to Flags.
    let symbols = row_of(&picker, Category::Characters("Symbols"));
    picker.choose(symbols);
    key(&mut picker, Key::Up);
    assert_eq!(picker.category().name(), "Flags");
}

/// **Typing searches, and the grid shows what it finds best first with the
/// cursor on the best; Enter picks it.**
#[test]
fn typing_searches_and_enter_picks_the_best_match() {
    let mut picker = CharPicker::new();
    type_text(&mut picker, "cat");
    assert_eq!(picker.search_text(), "cat");
    assert_eq!(picker.shown().next().as_deref(), Some("\u{1F408}"));
    assert_eq!(picker.cursor(), Some(0));
    assert_eq!(key(&mut picker, Key::Enter), picked("\u{1F408}"));
    assert_eq!(picker.recent(), ["\u{1F408}"]);
    // Emptying the search shows the category again.
    for _ in 0..3 {
        key(&mut picker, Key::Backspace);
    }
    assert_eq!(picker.search_text(), "");
    assert_eq!(picker.shown().next().as_deref(), Some("\u{1F600}"));
    assert_eq!(picker.cursor(), None);
    assert_eq!(
        key(&mut picker, Key::Enter),
        None,
        "nothing under the cursor"
    );
}

/// **A code point finds its character -- one with no name here too, which
/// the search still offers.**
#[test]
fn a_code_point_finds_its_character() {
    let mut picker = CharPicker::new();
    type_text(&mut picker, "U+00E9");
    assert_eq!(picker.shown().collect::<Vec<_>>(), ["\u{E9}"]);
    let mut picker = CharPicker::new();
    type_text(&mut picker, "U+4E00");
    assert_eq!(picker.shown().collect::<Vec<_>>(), ["\u{4E00}"]);
    assert_eq!(key(&mut picker, Key::Enter), picked("\u{4E00}"));
    // And it is remembered, though it has no name.
    assert_eq!(picker.recent(), ["\u{4E00}"]);
    let mut picker = CharPicker::new();
    type_text(&mut picker, "zzzzqqq");
    assert_eq!(picker.shown().count(), 0);
    assert_eq!(key(&mut picker, Key::Enter), None);
}

/// **An emoji is drawn and picked in the skin tone chosen, and remembered
/// untoned -- so the recent ones follow the tone too.**
#[test]
fn emoji_come_in_the_tone_chosen() {
    let mut picker = CharPicker::new();
    type_text(&mut picker, "waving hand");
    assert_eq!(picker.shown().next().as_deref(), Some("\u{1F44B}"));
    // Tab round to the tones: Categories, Grid, Tones.
    for _ in 0..3 {
        key(&mut picker, Key::Tab);
    }
    assert_eq!(picker.focus(), Part::Tones);
    for _ in 0..3 {
        key(&mut picker, Key::Right);
    }
    assert_eq!(picker.tone(), Some(SkinTone::Medium));
    assert_eq!(picker.shown().next().as_deref(), Some("\u{1F44B}\u{1F3FD}"));
    key(&mut picker, Key::Tab);
    assert_eq!(picker.focus(), Part::Search);
    assert_eq!(key(&mut picker, Key::Enter), picked("\u{1F44B}\u{1F3FD}"));
    assert_eq!(picker.recent(), ["\u{1F44B}"]);
    // An emoji with no tones is itself in any.
    let mut picker = CharPicker::new().with_tone(Some(SkinTone::Dark));
    assert_eq!(picker.shown().next().as_deref(), Some("\u{1F600}"));
    // And the tones run out at the ends.
    picker.focus = Part::Tones;
    key(&mut picker, Key::End);
    key(&mut picker, Key::Right);
    assert_eq!(picker.tone(), Some(SkinTone::Dark));
    key(&mut picker, Key::Home);
    key(&mut picker, Key::Left);
    assert_eq!(picker.tone(), None);
}

/// **The arrows move through the grid; its edges lead to the parts beside
/// it -- the search field above, the categories to the left.**
#[test]
fn the_arrows_move_through_the_grid() {
    let mut picker = CharPicker::new();
    let cells = Layout::new(W, H).cells();
    let columns = cells.columns;
    assert!(columns > 2, "{columns}");
    key(&mut picker, Key::Down);
    assert_eq!(picker.focus(), Part::Grid);
    assert_eq!(picker.cursor(), Some(0), "onto the first cell");
    key(&mut picker, Key::Right);
    key(&mut picker, Key::Right);
    assert_eq!(picker.cursor(), Some(2));
    key(&mut picker, Key::Down);
    assert_eq!(picker.cursor(), Some(2 + columns));
    key(&mut picker, Key::Up);
    assert_eq!(picker.cursor(), Some(2));
    key(&mut picker, Key::Up);
    assert_eq!(picker.focus(), Part::Search, "up from the top row");
    key(&mut picker, Key::Down);
    assert_eq!(picker.cursor(), Some(2), "back where it was");
    key(&mut picker, Key::Left);
    key(&mut picker, Key::Left);
    assert_eq!(picker.cursor(), Some(0));
    key(&mut picker, Key::Left);
    assert_eq!(
        picker.focus(),
        Part::Categories,
        "left from a row's first cell"
    );
    key(&mut picker, Key::Right);
    assert_eq!(picker.focus(), Part::Grid);
    let len = picker.shown().count();
    key(&mut picker, Key::End);
    assert_eq!(picker.cursor(), Some(len - 1));
    key(&mut picker, Key::Down);
    assert_eq!(picker.cursor(), Some(len - 1), "nowhere below the last row");
    key(&mut picker, Key::Right);
    assert_eq!(picker.cursor(), Some(len - 1));
    // From a cell of the last row that is not its last, Down stays put too:
    // there is no row below it to land in.
    if cells.column_of(len - 1) > 0 {
        key(&mut picker, Key::Left);
        assert_eq!(picker.cursor(), Some(len - 2));
        key(&mut picker, Key::Down);
        assert_eq!(
            picker.cursor(),
            Some(len - 2),
            "down from the last row moved"
        );
    }
    // From the row above a shorter last row, Down lands on its last cell.
    let above = (cells.row_of(len - 1) - 1) * columns + (columns - 1);
    if cells.column_of(len - 1) < columns - 1 {
        picker.cursor = Some(above);
        key(&mut picker, Key::Down);
        assert_eq!(picker.cursor(), Some(len - 1), "into the short last row");
    }
    key(&mut picker, Key::Home);
    assert_eq!(picker.cursor(), Some(0));
    key(&mut picker, Key::PageDown);
    assert_eq!(picker.cursor(), Some(cells.rows_shown * columns));
    let text = picker.shown().nth(cells.rows_shown * columns).unwrap();
    assert_eq!(key(&mut picker, Key::Space), picked(&text));
}

/// **The grid scrolls to keep the keyboard's cell on screen.**
#[test]
fn the_grid_follows_the_cursor() {
    let mut picker = CharPicker::new();
    let cells = Layout::new(W, H).cells();
    key(&mut picker, Key::Down);
    key(&mut picker, Key::End);
    let len = picker.shown().count();
    let rows = cells.rows(len);
    assert_eq!(picker.first_row, rows - cells.rows_shown);
    // The last cell is drawn, and can be clicked.
    let last = picker.shown().last().unwrap();
    assert_eq!(click(&mut picker, Target::Cell(len - 1)), picked(&last));
    key(&mut picker, Key::Home);
    assert_eq!(picker.first_row, 0);
}

/// **Typing in the categories or the grid goes on with the search.**
#[test]
fn typing_elsewhere_goes_to_the_search() {
    let mut picker = CharPicker::new();
    type_text(&mut picker, "hea");
    key(&mut picker, Key::Down);
    assert_eq!(picker.focus(), Part::Grid);
    type_text(&mut picker, "rt");
    assert_eq!(picker.focus(), Part::Search);
    assert_eq!(picker.search_text(), "heart");
    key(&mut picker, Key::Down);
    key(&mut picker, Key::Backspace);
    assert_eq!(picker.search_text(), "hear");
    assert_eq!(picker.focus(), Part::Search);
    // With no search, Backspace in the grid does nothing.
    let mut picker = CharPicker::new();
    key(&mut picker, Key::Down);
    key(&mut picker, Key::Backspace);
    assert_eq!(picker.focus(), Part::Grid);
}

/// **A click on a cell picks it without taking the keyboard from the search
/// field; one on a category shows it, emptying the search.**
#[test]
fn clicks_pick_and_choose() {
    let mut picker = CharPicker::new();
    type_text(&mut picker, "arrow right");
    let third = picker.shown().nth(2).unwrap();
    assert_eq!(click(&mut picker, Target::Cell(2)), picked(&third));
    assert_eq!(picker.focus(), Part::Search);
    assert_eq!(picker.cursor(), Some(2));
    let flags = picker
        .side
        .iter()
        .position(|row| matches!(row, SideRow::Category(c) if c.name() == "Flags"))
        .unwrap();
    assert_eq!(click(&mut picker, Target::Category(flags)), None);
    assert_eq!(picker.category().name(), "Flags");
    assert_eq!(picker.search_text(), "");
    assert_eq!(picker.focus(), Part::Categories);
    assert_eq!(
        picker.shown().next().as_deref(),
        Some("\u{1F3C1}"),
        "the chequered flag"
    );
    // A category scrolled into view is chosen as well.
    let (x, y) = centre_of(&picker, Target::Sidebar);
    mouse(
        &mut picker,
        x,
        y,
        MouseEventKind::Scroll { dx: 0.0, dy: -30.0 },
    );
    let arrows = row_of(&picker, Category::Characters("Arrows"));
    assert_eq!(click(&mut picker, Target::Category(arrows)), None);
    assert_eq!(picker.category(), Category::Characters("Arrows"));
    assert_eq!(picker.shown().next().as_deref(), Some("\u{2190}"));
    // A tone's swatch chooses it.
    assert_eq!(
        click(&mut picker, Target::Tone(Some(SkinTone::Light))),
        None
    );
    assert_eq!(picker.tone(), Some(SkinTone::Light));
    assert_eq!(click(&mut picker, Target::Tone(None)), None);
    assert_eq!(picker.tone(), None);
}

/// **The status line names the character under the pointer, else the
/// keyboard's, with its code points.**
#[test]
fn the_status_line_names_the_character() {
    let mut picker = CharPicker::new();
    let texts = |picker: &CharPicker| -> Vec<String> {
        picker
            .render(&Palette::for_mode(false), W, H)
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    assert!(texts(&picker).contains(&"Click a character to insert it".to_string()));
    type_text(&mut picker, "U+00E9");
    let shown = texts(&picker);
    assert!(shown.contains(&"LATIN SMALL LETTER E WITH ACUTE".to_string()));
    assert!(shown.contains(&"U+00E9".to_string()));
    // The pointer's cell wins over the keyboard's.
    let mut picker = CharPicker::new();
    let (x, y) = centre_of(&picker, Target::Cell(1));
    mouse(&mut picker, x, y, MouseEventKind::Move);
    let shown = texts(&picker);
    assert!(
        shown.contains(&"grinning face with big eyes".to_string()),
        "{shown:?}"
    );
    assert!(shown.contains(&"U+1F603".to_string()));
    // In a skin tone, the toned name and code points.
    let mut picker = CharPicker::new().with_tone(Some(SkinTone::Dark));
    type_text(&mut picker, "waving hand");
    let shown = texts(&picker);
    assert!(
        shown.contains(&"waving hand: dark skin tone".to_string()),
        "{shown:?}"
    );
    assert!(shown.contains(&"U+1F44B U+1F3FF".to_string()));
    // A character with no name says so.
    let mut picker = CharPicker::new();
    type_text(&mut picker, "U+4E00");
    assert!(texts(&picker).contains(&"No name known here".to_string()));
}

/// **The recent picks come first, most recent first, each once, at most
/// [`MAX_RECENT`] -- and a host's saved list is taken back, less what cannot
/// be shown.**
#[test]
fn recent_picks_are_remembered() {
    let mut picker = CharPicker::new();
    for text in ["U+00E9", "U+00E8", "U+00E9"] {
        type_text(&mut picker, text);
        assert!(key(&mut picker, Key::Enter).is_some());
        picker.search.clear();
    }
    assert_eq!(picker.recent(), ["\u{E9}", "\u{E8}"]);
    let reopened = CharPicker::new().with_recent(picker.recent().to_vec());
    assert_eq!(reopened.category(), Category::Recent);
    assert_eq!(reopened.shown().collect::<Vec<_>>(), ["\u{E9}", "\u{E8}"]);
    // What cannot be shown is dropped, and so is a repeat.
    let reopened = CharPicker::new().with_recent(vec![
        "ab".to_string(),
        "\u{1F600}".to_string(),
        "\u{7}".to_string(),
        "\u{1F600}".to_string(),
        " ".to_string(),
    ]);
    assert_eq!(reopened.recent(), ["\u{1F600}"]);
    // No more than MAX_RECENT.
    let many: Vec<String> = charnames::emoji()
        .take(MAX_RECENT + 5)
        .map(|e| e.text.to_string())
        .collect();
    let mut picker = CharPicker::new().with_recent(many.clone());
    assert_eq!(picker.recent().len(), MAX_RECENT);
    assert_eq!(picker.recent(), &many[..MAX_RECENT]);
    // A pick goes to the front and the oldest falls off.
    type_text(&mut picker, "U+00E9");
    key(&mut picker, Key::Enter);
    assert_eq!(picker.recent().len(), MAX_RECENT);
    assert_eq!(picker.recent()[0], "\u{E9}");
    assert_eq!(picker.recent()[MAX_RECENT - 1], many[MAX_RECENT - 2]);
    // With none, the picker opens on the emoji, and Recent says so.
    let mut picker = CharPicker::new();
    picker.choose(0);
    assert_eq!(picker.category(), Category::Recent);
    assert_eq!(picker.shown().count(), 0);
}

/// **Picking in the Recent category does not shuffle the cells under the
/// pointer.**
#[test]
fn a_pick_in_recent_does_not_move_the_cells() {
    let recent = vec!["\u{E9}".to_string(), "\u{E8}".to_string()];
    let mut picker = CharPicker::new().with_recent(recent);
    assert_eq!(click(&mut picker, Target::Cell(1)), picked("\u{E8}"));
    assert_eq!(picker.recent(), ["\u{E8}", "\u{E9}"]);
    assert_eq!(
        picker.shown().collect::<Vec<_>>(),
        ["\u{E9}", "\u{E8}"],
        "until shown again"
    );
}

/// **The wheel scrolls the list under the pointer, and the cursor stays.**
#[test]
fn the_wheel_scrolls_the_list_under_it() {
    let mut picker = CharPicker::new();
    let (x, y) = centre_of(&picker, Target::Cell(0));
    mouse(
        &mut picker,
        x,
        y,
        MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
    );
    assert!(picker.first_row > 0);
    let scrolled = picker.first_row;
    // A key that does not move the cursor does not undo the scroll.
    key(&mut picker, Key::Tab);
    assert_eq!(picker.first_row, scrolled);
    mouse(
        &mut picker,
        x,
        y,
        MouseEventKind::Scroll { dx: 0.0, dy: 30.0 },
    );
    assert_eq!(picker.first_row, 0);
    // The sidebar, under the pointer, scrolls itself and not the grid.
    let (sx, sy) = centre_of(&picker, Target::Category(row_of(&picker, Category::Recent)));
    let before = picker.side_view.first_visible();
    mouse(
        &mut picker,
        sx,
        sy,
        MouseEventKind::Scroll { dx: 0.0, dy: -30.0 },
    );
    assert!(
        picker.side_view.first_visible() > before,
        "the sidebar is longer than its well"
    );
    assert_eq!(picker.first_row, 0);
}

/// **Dragging the grid's thumb scrolls it; a press beside the thumb pages.**
#[test]
fn the_grids_scrollbar_scrolls_it() {
    let mut picker = CharPicker::new();
    let (x, y) = centre_of(&picker, Target::Thumb(List::Grid));
    mouse(&mut picker, x, y, MouseEventKind::Press(MouseButton::Left));
    mouse(&mut picker, x, y + 100.0, MouseEventKind::Move);
    assert!(picker.first_row > 0);
    mouse(
        &mut picker,
        x,
        y + 100.0,
        MouseEventKind::Release(MouseButton::Left),
    );
    let dragged = picker.first_row;
    mouse(&mut picker, x, y + 200.0, MouseEventKind::Move);
    assert_eq!(picker.first_row, dragged, "released");
    // A press on the track below the thumb pages down.
    let frame = picker.frame(&Palette::for_mode(false), W, H);
    let track = frame.rect_of(|t| *t == Target::Track(List::Grid)).unwrap();
    let thumb = frame.rect_of(|t| *t == Target::Thumb(List::Grid)).unwrap();
    let below = (thumb.bottom() + track.bottom()) / 2.0;
    mouse(
        &mut picker,
        x,
        below,
        MouseEventKind::Press(MouseButton::Left),
    );
    assert!(picker.first_row > dragged);
}

/// **Escape turns the picker down from anywhere; Tab and Shift+Tab go round
/// the four parts.**
#[test]
fn escape_cancels_and_tab_goes_round() {
    let mut picker = CharPicker::new();
    for part in [Part::Categories, Part::Grid, Part::Tones, Part::Search] {
        key(&mut picker, Key::Tab);
        assert_eq!(picker.focus(), part);
    }
    picker.handle_key(&shifted(Key::Tab), W, H);
    assert_eq!(picker.focus(), Part::Tones);
    for part in [Part::Tones, Part::Grid, Part::Categories, Part::Search] {
        picker.focus = part;
        assert_eq!(
            key(&mut picker, Key::Escape),
            Some(CharPickerEvent::Cancelled)
        );
    }
    // A release is nothing.
    let release = KeyEvent {
        pressed: false,
        ..press(Key::Escape)
    };
    assert_eq!(picker.handle_key(&release, W, H), None);
}

/// **Every cell drawn is where a click reaches it, and nothing is drawn past
/// the grid's well.**
#[test]
fn every_cell_drawn_is_hit_where_it_is_drawn() {
    let mut picker = CharPicker::new();
    type_text(&mut picker, "face");
    let frame = picker.frame(&Palette::for_mode(false), W, H);
    let grid = Layout::new(W, H).grid;
    let mut drawn = 0;
    for i in 0..picker.shown.len() {
        let Some(r) = frame.rect_of(|t| *t == Target::Cell(i)) else {
            continue;
        };
        drawn += 1;
        assert_eq!(
            frame.hit_test(r.x + r.w / 2.0, r.y + r.h / 2.0),
            Some(Target::Cell(i))
        );
        assert!(
            r.x >= grid.x && r.right() <= grid.right() + 0.01,
            "{i}: {r:?}"
        );
        assert!(
            r.y >= grid.y && r.bottom() <= grid.bottom() + 0.01,
            "{i}: {r:?}"
        );
    }
    let cells = Layout::new(W, H).cells();
    assert_eq!(drawn, cells.rows_shown * cells.columns);
}

/// **A picker drawn too small for its parts, or at no size at all, draws
/// what fits and answers events without a panic.**
#[test]
fn a_picker_too_small_draws_what_fits() {
    for (w, h) in [
        (0.0, 0.0),
        (40.0, 30.0),
        (200.0, 90.0),
        (-5.0, -5.0),
        (f32::NAN, 100.0),
    ] {
        let mut picker = CharPicker::new();
        let _ = picker.render(&Palette::for_mode(false), w, h);
        for k in [
            Key::Down,
            Key::End,
            Key::PageDown,
            Key::Up,
            Key::Left,
            Key::Tab,
            Key::Right,
        ] {
            let _ = picker.handle_key(&press(k), w, h);
        }
        let _ = picker.handle_mouse(
            &MouseEvent {
                x: 10.0,
                y: 10.0,
                kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
            },
            w,
            h,
        );
        let _ = picker.render(&Palette::for_mode(true), w, h);
    }
}

/// **A code point's status reads as Unicode writes it.**
#[test]
fn code_points_are_written_as_unicode_writes_them() {
    assert_eq!(code_points("\u{E9}"), "U+00E9");
    assert_eq!(code_points("\u{1F44B}\u{1F3FD}"), "U+1F44B U+1F3FD");
    assert_eq!(code_points(""), "");
}

/// **Ctrl+. is the chord, and nothing like it is**: not a full stop typed,
/// not a release, not with Shift, and not with Alt -- AltGr, which types a
/// character on several layouts, arrives as Ctrl+Alt.
#[test]
fn ctrl_period_is_the_chord() {
    let chord = |modifiers: Modifiers, pressed: bool| KeyEvent {
        key: Key::Period,
        pressed,
        modifiers,
        text: String::new(),
    };
    assert!(is_shortcut(&chord(Modifiers::ctrl(), true)));
    assert!(
        !is_shortcut(&chord(Modifiers::NONE, true)),
        "a full stop typed"
    );
    assert!(!is_shortcut(&chord(Modifiers::ctrl(), false)), "a release");
    let ctrl_shift = Modifiers {
        shift: true,
        ..Modifiers::ctrl()
    };
    assert!(!is_shortcut(&chord(ctrl_shift, true)));
    let altgr = Modifiers {
        alt: true,
        ..Modifiers::ctrl()
    };
    assert!(!is_shortcut(&chord(altgr, true)), "AltGr types a character");
    assert!(!is_shortcut(&KeyEvent {
        key: Key::A,
        ..chord(Modifiers::ctrl(), true)
    }));
}

/// How many boxes inside `area` are filled in the selection's colour.
fn chosen_in(picker: &CharPicker, area: Rect) -> usize {
    let palette = Palette::for_mode(false);
    let fill = palette.selection_fill();
    picker
        .render(&palette, W, H)
        .iter()
        .filter(|c| {
            matches!(c, RenderCommand::FillRect { x, y, color, .. }
                if *color == fill && area.contains(*x + 1.0, *y + 1.0))
        })
        .count()
}

/// **Only what the keyboard acts on is drawn chosen**: the category while no
/// search has replaced its cells, and the grid's cursor while the keyboard
/// is in the grid or the search field, whose Enter picks it -- not while it
/// is among the categories, where Enter picks nothing.
#[test]
fn only_what_the_keyboard_acts_on_is_drawn_chosen() {
    let layout = Layout::new(W, H);
    let mut picker = CharPicker::new();
    assert_eq!(chosen_in(&picker, layout.sidebar), 1, "the category chosen");
    type_text(&mut picker, "cat");
    assert_eq!(
        chosen_in(&picker, layout.sidebar),
        0,
        "a search's results drawn as a category's"
    );
    assert_eq!(chosen_in(&picker, layout.grid), 1, "the best match");
    key(&mut picker, Key::Tab);
    assert_eq!(picker.focus(), Part::Categories);
    assert_eq!(
        chosen_in(&picker, layout.grid),
        0,
        "the cursor drawn where Enter does not reach it"
    );
}

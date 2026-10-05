// A test module's job is to fail loudly the instant the code under test is
// wrong, so the defensive lints that forbid exactly that in production code
// are off here -- as `CLAUDE.md` prescribes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use crate::event::Modifiers;

/// The families most tests pick from, in the order a font index sorts them.
const FAMILIES: [&str; 6] = [
    "Arial",
    "Arial Black",
    "Bahnschrift",
    "Candara",
    "Open Sans",
    "Symbol Pi",
];

/// Whether a test family can be drawn bold: all but the symbol face.
fn bold_unless_symbol(name: &str) -> bool {
    !name.starts_with("Symbol")
}

fn picker_over(families: &[&str], initial: FontChoice) -> FontPickerDialog {
    FontPickerDialog::over(
        initial,
        families.iter().map(|f| (*f).to_string()).collect(),
        bold_unless_symbol,
    )
}

fn picker(initial: FontChoice) -> FontPickerDialog {
    picker_over(&FAMILIES, initial)
}

fn open_sans() -> FontChoice {
    FontChoice::new("Open Sans", FontWeightHint::Regular, 13)
}

fn size() -> (f32, f32) {
    FontPickerDialog::preferred_size()
}

fn key_with(key: Key, modifiers: Modifiers, text: &str) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        modifiers,
        text: text.to_string(),
    }
}

fn key(key: Key) -> KeyEvent {
    key_with(key, Modifiers::NONE, "")
}

fn press(p: &mut FontPickerDialog, k: Key) -> Option<FontPickerEvent> {
    let (w, h) = size();
    p.handle_key(&key(k), w, h)
}

/// Type `text` a character at a time, returning the last event.
fn type_text(p: &mut FontPickerDialog, text: &str) -> Option<FontPickerEvent> {
    let (w, h) = size();
    let mut last = None;
    for c in text.chars() {
        // As the other tests type: the text is what counts, whatever key.
        last = p.handle_key(&key_with(Key::A, Modifiers::NONE, &c.to_string()), w, h);
    }
    last
}

fn focus_on(p: &mut FontPickerDialog, part: Part) {
    for _ in 0..Part::ORDER.len() {
        if p.focus() == part {
            return;
        }
        press(p, Key::Tab);
    }
    assert_eq!(p.focus(), part);
}

fn rect_of(p: &FontPickerDialog, target: Target) -> Option<Rect> {
    let (w, h) = size();
    p.frame(&Palette::for_mode(false), w, h)
        .rect_of(|t| *t == target)
}

fn mouse(
    p: &mut FontPickerDialog,
    kind: MouseEventKind,
    (x, y): (f32, f32),
) -> Option<FontPickerEvent> {
    let (w, h) = size();
    p.handle_mouse(&MouseEvent { x, y, kind }, w, h)
}

fn click(p: &mut FontPickerDialog, target: Target) -> Option<FontPickerEvent> {
    let at = rect_of(p, target)
        .unwrap_or_else(|| panic!("{target:?} is not drawn"))
        .centre();
    mouse(p, MouseEventKind::Press(MouseButton::Left), at)
}

/// The event a tentative change to this choice returns, as the handlers
/// return it -- in an `Option`, to compare with them directly.
#[allow(
    clippy::unnecessary_wraps,
    reason = "compared with the handlers' Option results"
)]
fn changed_to(family: &str, weight: FontWeightHint, size: u16) -> Option<FontPickerEvent> {
    Some(FontPickerEvent::Changed(FontChoice::new(
        family, weight, size,
    )))
}

/// **A choice names the family a render tree draws it in** -- by its name,
/// or the UI face for a name no family can have -- and its size is held to
/// whole pixels the font cache builds.
#[test]
fn a_choice_names_its_family_for_the_render_tree() {
    let choice = FontChoice::new("Noto Serif", FontWeightHint::Bold, 18);
    assert_eq!(
        choice.font_family(),
        FontFamily::named("Noto Serif").unwrap()
    );
    assert_eq!(choice.font_size(), 18.0);
    assert_eq!(
        FontChoice::new("", FontWeightHint::Regular, 12).font_family(),
        FontFamily::Ui
    );
    assert_eq!(FontChoice::new("x", FontWeightHint::Regular, 0).size, 1);
    assert_eq!(
        FontChoice::new("x", FontWeightHint::Regular, u16::MAX).size,
        MAX_SIZE
    );
}

/// **A size is a whole number of pixels from 1 to the largest the cache
/// builds**, spaces round it allowed.
#[test]
fn a_size_is_a_whole_number_of_pixels_in_range() {
    assert_eq!(parse_size("14"), Some(14));
    assert_eq!(parse_size(" 9 "), Some(9));
    assert_eq!(parse_size("1"), Some(1));
    assert_eq!(parse_size(&MAX_SIZE.to_string()), Some(MAX_SIZE));
    for bad in ["0", "", "10.5", "-3", "abc", &(MAX_SIZE + 1).to_string()] {
        assert_eq!(parse_size(bad), None, "{bad:?}");
    }
}

/// **The dialog opens on the family it was given, whatever its capitals**,
/// with that family's row drawn and selected, and the size's row too.
#[test]
fn it_opens_on_the_family_and_size_it_was_given() {
    let p = picker(FontChoice::new("open sans", FontWeightHint::Regular, 16));
    assert_eq!(p.family_view.selected(), Some(4));
    assert!(rect_of(&p, Target::Family(4)).is_some());
    assert_eq!(p.size_view.selected(), SIZES.iter().position(|&s| s == 16));
    assert_eq!(p.focus(), Part::Search);
    assert!(p.size_is_valid());
    // Scrolled no further than the choice needs: the smaller sizes above 16
    // are on show, not hidden by a list opened with 16 at its top.
    assert!(rect_of(&p, Target::Size(0)).is_some(), "8 is hidden");
    assert!(rect_of(&p, Target::Family(0)).is_some(), "Arial is hidden");
}

/// **A family it does not list is kept** -- a document's font from another
/// machine -- **with nothing selected**, until the user picks one.
#[test]
fn a_family_it_does_not_list_is_kept_with_nothing_selected() {
    let p = picker(FontChoice::new("Lost Family", FontWeightHint::Regular, 12));
    assert_eq!(p.choice().family, "Lost Family");
    assert_eq!(p.family_view.selected(), None);
}

/// **Each step through the families is a tentative choice**: the arrows in
/// the list change the family, and an event says so each time.
#[test]
fn each_step_through_the_families_is_a_tentative_choice() {
    let mut p = picker(open_sans());
    focus_on(&mut p, Part::Families);
    assert_eq!(
        press(&mut p, Key::Down),
        changed_to("Symbol Pi", FontWeightHint::Regular, 13)
    );
    assert_eq!(press(&mut p, Key::Down), None, "past the last family");
    assert_eq!(
        press(&mut p, Key::Up),
        changed_to("Open Sans", FontWeightHint::Regular, 13)
    );
    assert_eq!(
        press(&mut p, Key::Home),
        changed_to("Arial", FontWeightHint::Regular, 13)
    );
    assert_eq!(
        press(&mut p, Key::End),
        changed_to("Symbol Pi", FontWeightHint::Regular, 13)
    );
    assert_eq!(
        p.original(),
        &open_sans(),
        "the original is kept for Cancel"
    );
}

/// **Typing narrows the families and lands on the first** -- those whose
/// name starts with what was typed before those that only contain it -- and
/// says so.
#[test]
fn typing_narrows_the_families_and_lands_on_the_first() {
    let mut p = picker(open_sans());
    assert_eq!(
        type_text(&mut p, "ar"),
        changed_to("Arial", FontWeightHint::Regular, 13)
    );
    assert_eq!(
        p.shown().collect::<Vec<_>>(),
        ["Arial", "Arial Black", "Candara"]
    );
    assert_eq!(p.search_text(), "ar");
    assert_eq!(p.family_view.selected(), Some(0));
}

/// **Names that start with the text come before names that only contain
/// it**, whatever order the families are listed in -- typing "a" reaches
/// Arial before Bahnschrift.
#[test]
fn names_that_start_with_the_text_come_first() {
    // "Corbel" has no "a", so the search does not list the choice and lands
    // on the first family it does list.
    let mut p = picker_over(
        &["Bahnschrift", "Candara", "Noto Sans", "Arial", "Corbel"],
        FontChoice::new("Corbel", FontWeightHint::Regular, 13),
    );
    assert_eq!(
        type_text(&mut p, "a"),
        changed_to("Arial", FontWeightHint::Regular, 13)
    );
    assert_eq!(
        p.shown().collect::<Vec<_>>(),
        ["Arial", "Bahnschrift", "Candara", "Noto Sans"]
    );
}

/// **The family chosen stays while the search still lists it**, even when
/// it is not the first listed.
#[test]
fn the_family_chosen_stays_while_the_search_lists_it() {
    let mut p = picker(FontChoice::new("Arial Black", FontWeightHint::Regular, 13));
    assert_eq!(type_text(&mut p, "ar"), None);
    assert_eq!(p.choice().family, "Arial Black");
    assert_eq!(p.family_view.selected(), Some(1));
}

/// **A search that lists nothing keeps the choice** and says the list is
/// empty; emptying the search lists everything again with the choice on it.
#[test]
fn a_search_that_lists_nothing_keeps_the_choice() {
    let mut p = picker(open_sans());
    assert_eq!(type_text(&mut p, "zz"), None);
    assert_eq!(p.shown().count(), 0);
    assert_eq!(p.choice(), &open_sans());
    let drawn = p.render(&Palette::for_mode(false), size().0, size().1);
    assert!(
        drawn
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "No family matches"))
    );
    press(&mut p, Key::Backspace);
    press(&mut p, Key::Backspace);
    assert_eq!(p.shown().count(), FAMILIES.len());
    assert_eq!(p.family_view.selected(), Some(4));
}

/// **The arrows move through the families from the search field** without
/// taking the keyboard from it; Home and End stay the field's.
#[test]
fn the_arrows_move_through_the_families_from_the_search_field() {
    let mut p = picker(open_sans());
    // "ar", not "a": "Open Sans" contains an "a", so the choice would stay.
    type_text(&mut p, "ar");
    assert_eq!(p.choice().family, "Arial");
    assert_eq!(
        press(&mut p, Key::Down),
        changed_to("Arial Black", FontWeightHint::Regular, 13)
    );
    assert_eq!(p.focus(), Part::Search);
    assert_eq!(press(&mut p, Key::End), None, "End moves the field's caret");
    assert_eq!(p.choice().family, "Arial Black");
}

/// **Typing in the family list types into the search**, and the keyboard
/// goes with it.
#[test]
fn typing_in_the_family_list_types_into_the_search() {
    let mut p = picker(open_sans());
    focus_on(&mut p, Part::Families);
    assert_eq!(
        type_text(&mut p, "c"),
        changed_to("Candara", FontWeightHint::Regular, 13)
    );
    assert_eq!(p.focus(), Part::Search);
    assert_eq!(p.search_text(), "c");
}

/// **A family that cannot be drawn bold offers regular alone**, and moving
/// to it from a bold choice takes the choice to regular in the same event.
#[test]
fn a_family_without_bold_offers_regular_alone() {
    let mut p = picker(FontChoice::new("Open Sans", FontWeightHint::Bold, 13));
    assert_eq!(p.weights(), [FontWeightHint::Regular, FontWeightHint::Bold]);
    focus_on(&mut p, Part::Families);
    assert_eq!(
        press(&mut p, Key::Down),
        changed_to("Symbol Pi", FontWeightHint::Regular, 13)
    );
    assert_eq!(p.weights(), [FontWeightHint::Regular]);
    assert!(rect_of(&p, Target::Weight(FontWeightHint::Bold)).is_none());
    focus_on(&mut p, Part::Weights);
    assert_eq!(press(&mut p, Key::Down), None, "no bold to step to");
}

/// **A weight is chosen by the arrows or the pointer**, once: choosing the
/// weight already chosen says nothing.
#[test]
fn a_weight_is_chosen_by_the_arrows_or_the_pointer() {
    let mut p = picker(open_sans());
    focus_on(&mut p, Part::Weights);
    assert_eq!(
        press(&mut p, Key::Down),
        changed_to("Open Sans", FontWeightHint::Bold, 13)
    );
    assert_eq!(
        press(&mut p, Key::Up),
        changed_to("Open Sans", FontWeightHint::Regular, 13)
    );
    assert_eq!(
        click(&mut p, Target::Weight(FontWeightHint::Bold)),
        changed_to("Open Sans", FontWeightHint::Bold, 13)
    );
    assert_eq!(click(&mut p, Target::Weight(FontWeightHint::Bold)), None);
}

/// **A size typed is the size once it is one**: half typed or out of range,
/// the field is marked wrong and the size stays the last that was one.
#[test]
fn a_size_typed_is_the_size_once_it_is_one() {
    let mut p = picker(open_sans());
    focus_on(&mut p, Part::SizeField);
    press(&mut p, Key::Backspace);
    press(&mut p, Key::Backspace);
    assert!(!p.size_is_valid(), "an empty field holds no size");
    assert_eq!(p.choice().size, 13, "the last size it held");
    assert_eq!(
        type_text(&mut p, "2"),
        changed_to("Open Sans", FontWeightHint::Regular, 2)
    );
    assert_eq!(
        type_text(&mut p, "0"),
        changed_to("Open Sans", FontWeightHint::Regular, 20)
    );
    assert_eq!(p.size_view.selected(), SIZES.iter().position(|&s| s == 20));
    // On to 200, which is a size, then 2000, which is past the largest.
    assert_eq!(
        type_text(&mut p, "0"),
        changed_to("Open Sans", FontWeightHint::Regular, 200)
    );
    assert_eq!(p.size_view.selected(), None, "200 is not listed");
    assert_eq!(type_text(&mut p, "0"), None, "2000 is past the largest");
    assert!(!p.size_is_valid());
    assert_eq!(p.choice().size, 200);
}

/// **A field the keyboard comes to holds its text selected**, so typing
/// replaces it -- by Tab, or a press on the size field.
#[test]
fn typing_in_a_field_just_come_to_replaces_it() {
    let mut p = picker(open_sans());
    focus_on(&mut p, Part::SizeField);
    type_text(&mut p, "1");
    assert_eq!(
        type_text(&mut p, "4"),
        changed_to("Open Sans", FontWeightHint::Regular, 14)
    );
    assert_eq!(p.size_text.text(), "14");

    let mut p = picker(open_sans());
    assert_eq!(click(&mut p, Target::SizeField), None);
    assert_eq!(
        type_text(&mut p, "9"),
        changed_to("Open Sans", FontWeightHint::Regular, 9)
    );

    let mut p = picker(open_sans());
    type_text(&mut p, "zz");
    focus_on(&mut p, Part::Families);
    focus_on(&mut p, Part::Search);
    assert_eq!(
        type_text(&mut p, "c"),
        changed_to("Candara", FontWeightHint::Regular, 13)
    );
    assert_eq!(p.search_text(), "c", "the old search was replaced");
}

/// **The arrows in the size field step through the listed sizes**, down the
/// list for larger, from a typed size the list does not have as well, and
/// write the size into the field.
#[test]
fn the_arrows_step_through_the_listed_sizes() {
    let mut p = picker(FontChoice::new("Open Sans", FontWeightHint::Regular, 15));
    assert_eq!(p.size_view.selected(), None);
    focus_on(&mut p, Part::SizeField);
    assert_eq!(
        press(&mut p, Key::Down),
        changed_to("Open Sans", FontWeightHint::Regular, 16)
    );
    assert_eq!(p.size_text.text(), "16");
    assert_eq!(
        press(&mut p, Key::Up),
        changed_to("Open Sans", FontWeightHint::Regular, 14)
    );
    focus_on(&mut p, Part::Sizes);
    assert_eq!(
        press(&mut p, Key::Down),
        changed_to("Open Sans", FontWeightHint::Regular, 16)
    );
    assert_eq!(
        press(&mut p, Key::End),
        changed_to("Open Sans", FontWeightHint::Regular, 72)
    );
    assert_eq!(press(&mut p, Key::Down), None, "past the largest listed");
    assert_eq!(
        press(&mut p, Key::Home),
        changed_to("Open Sans", FontWeightHint::Regular, 8)
    );
    assert_eq!(p.size_text.text(), "8");
}

/// **A size picked from the list by the pointer is written into the field**.
#[test]
fn a_size_picked_by_the_pointer_is_written_into_the_field() {
    let mut p = picker(open_sans());
    let at = SIZES.iter().position(|&s| s == 9).unwrap();
    assert_eq!(
        click(&mut p, Target::Size(at)),
        changed_to("Open Sans", FontWeightHint::Regular, 9)
    );
    assert_eq!(p.size_text.text(), "9");
    assert_eq!(p.focus(), Part::Sizes);
}

/// **Enter keeps the choice; Escape puts back the original** -- and after
/// either, the dialog takes nothing more.
#[test]
fn enter_keeps_the_choice_and_escape_puts_back_the_original() {
    let mut p = picker(open_sans());
    type_text(&mut p, "ar");
    let kept = p.choice().clone();
    assert_eq!(
        press(&mut p, Key::Enter),
        Some(FontPickerEvent::Confirmed(kept.clone()))
    );
    assert!(p.is_confirmed() && !p.is_cancelled());
    assert_eq!(press(&mut p, Key::Escape), None, "done with");
    assert_eq!(p.choice(), &kept);

    let mut p = picker(open_sans());
    type_text(&mut p, "ar");
    assert_eq!(press(&mut p, Key::Escape), Some(FontPickerEvent::Cancelled));
    assert!(p.is_cancelled());
    assert_eq!(p.choice(), &open_sans(), "the original put back");
    assert_eq!(type_text(&mut p, "b"), None, "done with");
    assert_eq!(click(&mut p, Target::Ok), None, "done with");
}

/// **Enter on Cancel cancels, and Space presses the button with the
/// keyboard**.
#[test]
fn the_buttons_are_pressed_from_the_keyboard() {
    let mut p = picker(open_sans());
    focus_on(&mut p, Part::Cancel);
    assert_eq!(press(&mut p, Key::Enter), Some(FontPickerEvent::Cancelled));

    let mut p = picker(open_sans());
    focus_on(&mut p, Part::Ok);
    assert_eq!(
        press(&mut p, Key::Space),
        Some(FontPickerEvent::Confirmed(open_sans()))
    );

    let mut p = picker(open_sans());
    focus_on(&mut p, Part::Cancel);
    assert_eq!(press(&mut p, Key::Space), Some(FontPickerEvent::Cancelled));
}

/// **Tab and Shift+Tab go round every part, in order, both ways.**
#[test]
fn tab_goes_round_the_parts_both_ways() {
    let mut p = picker(open_sans());
    let mut seen = vec![p.focus()];
    for _ in 0..Part::ORDER.len() {
        press(&mut p, Key::Tab);
        seen.push(p.focus());
    }
    let mut expected = Part::ORDER.to_vec();
    expected.push(Part::Search);
    assert_eq!(seen, expected);
    let (w, h) = size();
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    p.handle_key(&key_with(Key::Tab, shift, ""), w, h);
    assert_eq!(p.focus(), Part::Cancel);
    p.handle_key(&key_with(Key::Tab, shift, ""), w, h);
    assert_eq!(p.focus(), Part::Ok);
}

/// **A click on a family chooses it, and a double-click keeps it.**
#[test]
fn a_click_chooses_a_family_and_a_double_click_keeps_it() {
    let mut p = picker(open_sans());
    assert_eq!(
        click(&mut p, Target::Family(3)),
        changed_to("Candara", FontWeightHint::Regular, 13)
    );
    assert_eq!(p.focus(), Part::Families);
    let at = rect_of(&p, Target::Family(3)).unwrap().centre();
    assert_eq!(
        mouse(&mut p, MouseEventKind::DoubleClick(MouseButton::Left), at),
        Some(FontPickerEvent::Confirmed(FontChoice::new(
            "Candara",
            FontWeightHint::Regular,
            13
        )))
    );
}

/// **OK and Cancel are pressed by the pointer**, and the fields take the
/// keyboard when pressed.
#[test]
fn the_pointer_presses_the_buttons_and_focuses_the_fields() {
    let mut p = picker(open_sans());
    assert_eq!(click(&mut p, Target::SizeField), None);
    assert_eq!(p.focus(), Part::SizeField);
    assert_eq!(click(&mut p, Target::Search), None);
    assert_eq!(p.focus(), Part::Search);
    assert_eq!(
        click(&mut p, Target::Ok),
        Some(FontPickerEvent::Confirmed(open_sans()))
    );

    let mut p = picker(open_sans());
    assert_eq!(
        click(&mut p, Target::Cancel),
        Some(FontPickerEvent::Cancelled)
    );
}

/// A list of `n` families, "Family 000" on.
fn many(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("Family {i:03}")).collect()
}

fn picker_of_many(n: usize) -> FontPickerDialog {
    FontPickerDialog::over(
        FontChoice::new("Family 000", FontWeightHint::Regular, 13),
        many(n),
        bold_unless_symbol,
    )
}

/// **The wheel scrolls the list it is over without moving the choice** --
/// and the scroll survives the pointer moving afterwards, which tells the
/// list its height again.
#[test]
fn the_wheel_scrolls_the_list_without_moving_the_choice() {
    let mut p = picker_of_many(100);
    let row = rect_of(&p, Target::Family(0)).unwrap().centre();
    mouse(&mut p, MouseEventKind::Scroll { dx: 0.0, dy: -3.0 }, row);
    let first = p.family_view.first_visible();
    assert!(first > 0, "the list did not scroll");
    assert_eq!(p.choice().family, "Family 000");
    mouse(&mut p, MouseEventKind::Move, row);
    assert_eq!(
        p.family_view.first_visible(),
        first,
        "a move undid the scroll"
    );
    assert!(
        rect_of(&p, Target::Family(0)).is_none(),
        "the first row scrolled away"
    );
    assert!(rect_of(&p, Target::Family(first)).is_some());
}

/// **The scrollbar's thumb drags the family list**, and a press on its
/// track pages towards the press.
#[test]
fn the_scrollbar_drags_and_pages_the_list() {
    let mut p = picker_of_many(100);
    let thumb = rect_of(&p, Target::Thumb(List::Families)).unwrap();
    let track = rect_of(&p, Target::Track(List::Families)).unwrap();
    let (x, y) = thumb.centre();
    mouse(&mut p, MouseEventKind::Press(MouseButton::Left), (x, y));
    mouse(&mut p, MouseEventKind::Move, (x, track.bottom()));
    let len = 100;
    let last_page = len - p.family_view.height();
    assert_eq!(
        p.family_view.first_visible(),
        last_page,
        "dragged to the end"
    );
    mouse(
        &mut p,
        MouseEventKind::Release(MouseButton::Left),
        (x, track.bottom()),
    );
    mouse(&mut p, MouseEventKind::Move, (x, track.y));
    assert_eq!(
        p.family_view.first_visible(),
        last_page,
        "released: no longer dragged"
    );
    // The thumb is at the bottom now: a press at the track's top pages up.
    mouse(
        &mut p,
        MouseEventKind::Press(MouseButton::Left),
        (x, track.y + 1.0),
    );
    assert_eq!(
        p.family_view.first_visible(),
        last_page - p.family_view.height()
    );
    assert_eq!(p.choice().family, "Family 000", "scrolling chose nothing");
}

/// **Page Down moves a windowful of families**, the rows on screen at the
/// size the dialog is drawn.
#[test]
fn page_down_moves_a_windowful() {
    let mut p = picker_of_many(100);
    focus_on(&mut p, Part::Families);
    let rows = Layout::new(size().0, size().1).family_rows();
    assert!(rows > 1);
    assert_eq!(
        press(&mut p, Key::PageDown),
        changed_to(&format!("Family {rows:03}"), FontWeightHint::Regular, 13)
    );
}

/// **The first frame shows the family chosen**, even far down the list,
/// before any event has told the list how many rows it has.
#[test]
fn the_first_frame_shows_the_family_chosen() {
    let p = FontPickerDialog::over(
        FontChoice::new("Family 080", FontWeightHint::Regular, 13),
        many(100),
        bold_unless_symbol,
    );
    assert!(rect_of(&p, Target::Family(80)).is_some());
}

/// The family pushed when each `Text` command was drawn, paired with it.
fn texts_with_families(
    commands: &[RenderCommand],
) -> Vec<(String, FontFamily, f32, FontWeightHint)> {
    let mut fonts = vec![FontFamily::Ui];
    let mut out = Vec::new();
    for command in commands {
        match command {
            RenderCommand::PushFont { family } => fonts.push(*family),
            RenderCommand::PopFont => {
                fonts.pop();
            }
            RenderCommand::Text {
                text,
                font_size,
                font_weight,
                ..
            } => out.push((
                text.clone(),
                *fonts.last().unwrap(),
                *font_size,
                *font_weight,
            )),
            _ => {}
        }
    }
    out
}

/// **Each family's name is drawn in its own face; each weight in the family
/// chosen at that weight; the preview in the choice at its own size.**
#[test]
fn each_family_and_the_preview_are_drawn_in_their_own_faces() {
    let p =
        picker(FontChoice::new("Candara", FontWeightHint::Bold, 20)).with_sample("Grumpy wizards");
    let drawn = texts_with_families(&p.render(&Palette::for_mode(false), size().0, size().1));
    for family in FAMILIES {
        assert!(
            drawn
                .iter()
                .any(|(text, face, _, _)| text == family
                    && *face == FontFamily::named(family).unwrap()),
            "{family} not drawn in its own face"
        );
    }
    let candara = FontFamily::named("Candara").unwrap();
    assert!(drawn.contains(&(
        "Bold".to_string(),
        candara,
        scaled(ROW_SIZE),
        FontWeightHint::Bold
    )));
    assert!(drawn.contains(&(
        "Regular".to_string(),
        candara,
        scaled(ROW_SIZE),
        FontWeightHint::Regular
    )));
    assert!(drawn.contains(&(
        "Grumpy wizards".to_string(),
        candara,
        20.0,
        FontWeightHint::Bold
    )));
}

/// **The dialog follows the user's text size, and the preview does not**:
/// the font being chosen is drawn at its own size, whatever size the
/// interface is.
#[test]
fn the_dialog_follows_the_text_size_and_the_preview_does_not() {
    let (w, h) = FontPickerDialog::preferred_size();
    text::set_base_size(26.0);
    assert_eq!(text::base_size(), 26.0);
    let (w2, h2) = FontPickerDialog::preferred_size();
    assert_eq!((w2, h2), (w * 2.0, h * 2.0));
    let p = picker(open_sans()).with_sample("Grumpy wizards");
    let drawn = texts_with_families(&p.render(&Palette::for_mode(false), w2, h2));
    let open = FontFamily::named("Open Sans").unwrap();
    assert!(drawn.contains(&(
        "Grumpy wizards".to_string(),
        open,
        13.0,
        FontWeightHint::Regular
    )));
    assert!(drawn.contains(&(
        "Open Sans".to_string(),
        open,
        scaled(FAMILY_SIZE),
        FontWeightHint::Regular
    )));
    text::set_base_size(text::DEFAULT_SIZE);
}

/// **A dialog too small for its lists draws and takes events** without
/// rows, and without a box whose bottom is above its top.
#[test]
fn a_dialog_too_small_for_its_lists_still_works() {
    let mut p = picker(open_sans());
    for (w, h) in [(0.0, 0.0), (90.0, 60.0), (300.0, 150.0)] {
        let frame = p.frame(&Palette::for_mode(false), w, h);
        assert!(frame.is_balanced());
        assert!(
            !frame
                .hits()
                .iter()
                .any(|(t, _)| matches!(t, Target::Family(_)))
        );
        for command in frame.commands() {
            if let RenderCommand::PushClip { width, height, .. } = command {
                assert!(*width >= 0.0 && *height >= 0.0, "{command:?}");
            }
        }
        // The keys still work with no row on show -- Page Down moves the
        // choice a row, as a window one row tall would -- and the choice
        // stays a family the dialog lists.
        let _ = p.handle_key(&key(Key::PageDown), w, h);
        assert!(FAMILIES.contains(&p.choice().family.as_str()));
        assert_eq!(
            p.handle_mouse(
                &MouseEvent {
                    x: 5.0,
                    y: 5.0,
                    kind: MouseEventKind::Move
                },
                w,
                h
            ),
            None
        );
    }
}

/// **With no families at all the dialog says so**, and still keeps or turns
/// down the choice it was given.
#[test]
fn with_no_families_it_says_so() {
    let mut p = picker_over(&[], open_sans());
    let drawn = p.render(&Palette::for_mode(false), size().0, size().1);
    assert!(drawn.iter().any(
        |c| matches!(c, RenderCommand::Text { text, .. } if text == "No fonts are installed")
    ));
    assert_eq!(type_text(&mut p, "a"), None);
    assert_eq!(
        press(&mut p, Key::Enter),
        Some(FontPickerEvent::Confirmed(open_sans()))
    );
}

/// **The frame is balanced and every hit box is inside the dialog.**
#[test]
fn the_frame_is_balanced_and_its_boxes_are_inside_the_dialog() {
    let p = picker_of_many(40);
    let (w, h) = size();
    let frame = p.frame(&Palette::for_mode(false), w, h);
    assert!(frame.is_balanced());
    for (target, rect) in frame.hits() {
        assert!(
            rect.x >= 0.0 && rect.y >= 0.0 && rect.right() <= w + 0.01 && rect.bottom() <= h + 0.01,
            "{target:?} at {rect:?}"
        );
    }
}

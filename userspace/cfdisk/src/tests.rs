#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

//! The parts of cfdisk that need no terminal: the line editor (in the C
//! locale the tests run in, where every byte is a character of one cell),
//! C's conversions between `size_t` and `int`, the main menu's ignored keys,
//! and where the menus put their items.

use super::*;

/// An editor on `text`, in a buffer of `size` bytes, the cursor at its end.
fn editor(text: &[u8], size: usize) -> Editor {
    let mut e = Editor::new(text, size, false);
    assert_eq!(e.goto(MBS_EDIT_END), 0);
    e
}

#[test]
fn the_editor_starts_on_the_text_up_to_its_nul() {
    let e = Editor::new(b"abc\0def", 128, false);
    assert_eq!(e.text(), b"abc");
    assert_eq!((e.cur_bytes, e.cur_cells), (3, 3));
    assert_eq!((e.cursor, e.cursor_cells), (0, 0));
}

#[test]
fn backspace_and_delete_take_out_the_character_before_and_at_the_cursor() {
    let mut e = editor(b"abc", 128);
    assert_eq!(e.backspace(), 0);
    assert_eq!(e.text(), b"ab");
    assert_eq!((e.cursor, e.cur_bytes), (2, 2));
    // At the end, delete takes the character before the cursor.
    assert_eq!(e.delete(), 0);
    assert_eq!(e.text(), b"a");
    assert_eq!(e.goto(MBS_EDIT_HOME), 0);
    assert_eq!(e.delete(), 0);
    assert_eq!(e.text(), b"");
    // Nothing left to take.
    assert_eq!(e.delete(), 1);
    assert_eq!(e.backspace(), 1);
}

#[test]
fn insert_puts_the_character_at_the_cursor_and_moves_past_it() {
    let mut e = editor(b"ac", 128);
    assert_eq!(e.goto(MBS_EDIT_LEFT), 0);
    assert_eq!(e.insert(u32::from(b'b')), 0);
    assert_eq!(e.text(), b"abc");
    assert_eq!(
        (e.cursor, e.cursor_cells, e.cur_bytes, e.cur_cells),
        (2, 2, 3, 3)
    );
}

#[test]
fn the_cursor_stops_at_either_end() {
    let mut e = editor(b"ab", 128);
    assert_eq!(e.goto(MBS_EDIT_RIGHT), 1);
    assert_eq!(e.goto(MBS_EDIT_HOME), 0);
    assert_eq!(e.goto(MBS_EDIT_LEFT), 1);
    assert_eq!(e.goto(MBS_EDIT_RIGHT), 0);
    assert_eq!(e.cursor, 1);
    assert_eq!(e.goto(99), -libcall::EINVAL);
}

#[test]
fn insert_refuses_once_a_character_of_the_most_bytes_might_not_fit() {
    // MB_CUR_MAX is 1 in the C locale: a buffer of 4 takes 3 bytes and the
    // NUL, so a fourth is refused.
    let mut e = editor(b"abc", 4);
    assert_eq!(e.insert(u32::from(b'd')), 0);
    assert_eq!(e.insert(u32::from(b'e')), 1);
    assert_eq!(e.text(), b"abcd");
}

#[test]
fn c_conversions_wrap_as_c_does() {
    assert_eq!(c_int(usize::MAX), -1);
    assert_eq!(c_int(0x1_0000_0005), 5);
    assert_eq!(c_size(-1), usize::MAX);
    assert_eq!(c_size(7), 7);
    assert_eq!(neg(22), -22);
}

#[test]
fn the_main_menu_ignores_what_does_nothing_for_the_current_partition() {
    let mut m = Menu::default();
    let mut keys = MainKeys {
        freespace: Some(true),
        dos_or_sgi: true,
        wrong_order: false,
        readonly: false,
    };
    assert_eq!(main_menu_ignore_keys(&keys, &mut m), b"dtbrs");
    assert_eq!(m.prefkey, i32::from(b'n'));
    keys.freespace = Some(false);
    keys.wrong_order = true;
    assert_eq!(main_menu_ignore_keys(&keys, &mut m), b"n");
    assert_eq!(m.prefkey, i32::from(b'q'));
    keys.dos_or_sgi = false;
    keys.readonly = true;
    assert_eq!(main_menu_ignore_keys(&keys, &mut m), b"nbW");
    // No current partition: nothing, and the preferred key left as it was.
    keys.freespace = None;
    assert_eq!(main_menu_ignore_keys(&keys, &mut m), b"");
    assert_eq!(m.prefkey, i32::from(b'q'));
}

/// A screen of `lines` by `cols` with the main menu on it.
fn ui_with_main_menu(lines: usize, cols: usize) -> Ui {
    let mut ui = Ui {
        enabled: false,
        lines,
        cols,
        menus: Vec::new(),
        colors: false,
        utf8: false,
        term: Rc::new(Term::new(b"cfdisk".to_vec())),
    };
    ui.menu_push(main_menuitems());
    ui
}

#[test]
fn the_main_menu_lays_its_items_out_in_rows_centred_on_the_screen() {
    let ui = ui_with_main_menu(24, 80);
    let m = ui.menu().unwrap();
    // "Bootable" is the widest: "[Bootable]" and two spaces, twelve columns,
    // six to an 80-column row, the eight left over shared out.
    assert_eq!((m.width, m.nitems, m.page_sz), (8, 10, 0));
    assert_eq!(
        (ui.menuitem_get_line(0), ui.menuitem_get_column(0)),
        (20, 5)
    );
    assert_eq!(
        (ui.menuitem_get_line(5), ui.menuitem_get_column(5)),
        (20, 65)
    );
    assert_eq!(
        (ui.menuitem_get_line(7), ui.menuitem_get_column(7)),
        (21, 17)
    );
}

#[test]
fn a_screen_too_narrow_for_one_item_puts_every_item_at_the_origin() {
    let ui = ui_with_main_menu(24, 10);
    assert_eq!((ui.menuitem_get_line(3), ui.menuitem_get_column(3)), (0, 0));
}

#[test]
fn ignored_items_are_skipped_and_a_nul_key_finds_the_strings_end() {
    let mut ui = ui_with_main_menu(24, 80);
    ui.menu_mut().unwrap().ignore = Some(b"dtbr".to_vec());
    // Bootable, Delete and Resize are left out, so the first shown is New,
    // the fourth of the list; and Quit is the second shown.
    assert_eq!(ui.menu_get_menuitem(0), Some(3));
    assert_eq!(ui.menu_get_menuitem_by_key(i32::from(b'q')), Some((4, 1)));
    assert!(Ui::ignored(ui.menu().unwrap(), 0));
    assert!(!Ui::ignored(ui.menu().unwrap(), i32::from(b'q')));
}

#[test]
fn iszero_is_true_only_for_zeros() {
    assert!(iszero(b"000"));
    assert!(iszero(b""));
    assert!(!iszero(b"0010"));
}

"""Mutation test for the photo manager's photograph loader.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26: the selected photograph was
decoded inside `render`, freezing the window for as long as that took; it is
decoded on `offloop`'s worker now, and put up when the worker wakes the
window.  `offloop`'s own rule -- only the newest request is answered -- is
swept by `apps/offloop/mutate.py`.  The first rows, added 2026-10-04, cover
the list of keys' hold on the pointer -- a press with it up puts it away and
reaches nothing under it, and the wheel scrolls nothing it covers -- and the
grid's wheel, which it did not have before that day.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

OFF = "the_selected_photograph_is_decoded_off_the_window"
GONE = "a_photograph_deselected_while_decoding_is_not_put_up"
FAILS = "a_photograph_that_fails_off_the_window_says_why"
NOT_A_PICTURE = "a_file_that_is_not_a_picture_says_why_instead_of_staying_blank"
THUMBS = "the_grids_thumbnails_are_made_off_the_window"
CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"
WHEEL = "the_wheel_scrolls_the_grid_a_row_a_notch"
NOTHING_ELSE = "the_wheel_scrolls_the_grid_and_nothing_else"
PAST_THE_END = "turning_the_wheel_on_past_the_end_banks_nothing"
FRACTION = "a_fraction_of_a_notch_does_not_outlive_the_grid"

MUTATIONS = [
    # The list of keys takes a press rather than letting it reach the
    # photograph under it (known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        "a press goes through the list of keys",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n"
        "                    return true;\n"
        "                }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the list of keys away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        [CARD],
    ),
    (
        "a press under the list of keys leaves it up",
        "                    self.show_help = false;\n"
        "                    return true;\n",
        "                    return true;\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the list of keys covers",
        "                MouseEventKind::Scroll { .. } => return false,\n",
        "",
        [CARD],
    ),
    # The grid's wheel, which it did not have.
    (
        "the grid has no wheel",
        "        if let MouseEventKind::Scroll { dy, .. } = event.kind {\n"
        "            return self.scroll_grid(event.x, event.y, dy);\n"
        "        }\n",
        "",
        [WHEEL, NOTHING_ELSE, PAST_THE_END, FRACTION, CARD],
    ),
    (
        "a notch is three rows of thumbnails",
        "        let rows = self.grid_wheel.rows_at(dy, 1.0);\n",
        "        let rows = self.grid_wheel.rows(dy);\n",
        [WHEEL],
    ),
    (
        "the wheel scrolls a grid that is not shown",
        "        if self.view_mode != ViewMode::Grid\n"
        "            || self.photo_menu.is_some()\n",
        "        if self.photo_menu.is_some()\n",
        [NOTHING_ELSE],
    ),
    (
        "the wheel scrolls the grid under an open menu",
        "            || self.photo_menu.is_some()\n",
        "",
        [NOTHING_ELSE],
    ),
    (
        "the wheel over the sidebar scrolls the grid",
        "            || !self.content_rect().contains(x, y)\n",
        "",
        [NOTHING_ELSE],
    ),
    (
        "a wheel turned on past the end banks rows",
        "        self.grid_scroll = self.grid_window().start;\n"
        "        self.grid_scroll != before\n",
        "        self.grid_scroll != before\n",
        [PAST_THE_END],
    ),
    (
        "a fraction of a notch outlives the grid",
        "        self.grid_wheel.reset();\n",
        "",
        [FRACTION],
    ),
    (
        "the photograph is decoded on the window even with a loader",
        "            Some(loader) => loader.ask((pid, path)),",
        "            Some(_) => Err::<offloop::Ticket, _>((pid, path)),",
        [OFF],
    ),
    (
        "no waker is asked for",
        "    fn wants_waker(&self) -> bool {\n        true",
        "    fn wants_waker(&self) -> bool {\n        false",
        [OFF],
    ),
    (
        "no loader is started",
        "        self.picture_loader = offloop::Latest::start(",
        "        self.picture_loader = None;\n        let _unused = offloop::Latest::start(",
        [OFF],
    ),
    (
        "a decoded photograph is not put up",
        "        self.show_picture(pid, decoded);\n        true",
        "        let _ = (pid, decoded);\n        true",
        [OFF],
    ),
    (
        "a decoded photograph is not drawn",
        "        self.show_picture(pid, decoded);\n        true",
        "        self.show_picture(pid, decoded);\n        false",
        [OFF],
    ),
    (
        "a photograph no longer selected is put up",
        "        if self.picture_for != Some(pid) {\n            return false;\n        }",
        "",
        [GONE],
    ),
    (
        "a photograph that will not decode says nothing",
        "            Err(why) => {\n                self.picture_error = Some(why);\n                return;",
        "            Err(why) => {\n                let _ = why;\n                return;",
        [FAILS, NOT_A_PICTURE],
    ),
    # -- the grid's thumbnails ------------------------------------------------
    (
        "thumbnails are left to the frame even with a worker",
        "            Some(worker) => match worker.replace(wanted) {",
        "            Some(_) => match Err::<(), _>(wanted) {",
        [THUMBS],
    ),
    (
        "no thumbnail worker is started",
        "        self.thumb_worker = offloop::Queue::start(",
        "        self.thumb_worker = None;\n        let _unused = offloop::Queue::start(",
        [THUMBS],
    ),
    (
        "a made thumbnail is not filed",
        "        for (req, thumb) in made {\n            self.file_thumbnail(req, thumb);\n        }\n        any",
        "        let _ = made;\n        any",
        [THUMBS],
    ),
    (
        "a wake with thumbnails asks for no frame",
        "        if self.take_decoded_picture() || thumbnails {",
        "        if self.take_decoded_picture() {",
        [THUMBS],
    ),
]

# The keys are taken plain, a command's letter is no binding, and the search
# box, a new album's row and the tag are the toolkit's fields (2026-10-04;
# lane C, c-e-a-theme-can-shape-the-controls).
CHORDS = "a_chord_is_no_photo_manager_key_and_types_nothing"
SEARCH = "the_search_box_is_the_toolkits_field"
EDITS = "the_search_box_edits_like_a_field"
ALBUM = "a_new_albums_name_is_typed_into_a_field_in_its_row"
TAG_MENU = "a_tag_typed_from_the_menu_reaches_the_photograph"
TAG_SHOWN = "the_tag_being_typed_is_shown"
TAG_DIGIT = "a_digit_typed_into_a_tag_does_not_rate_a_photograph"
TAG_ALL = "a_tag_goes_on_the_whole_selection"

MUTATIONS += [
    (
        "a chord raises the list of keys",
        "        if event.key == Key::F1 && plain {\n",
        "        if event.key == Key::F1 {\n",
        [CHORDS],
    ),
    (
        "a chorded Escape puts the list of keys away",
        "            if plain && matches!(event.key, Key::Escape | Key::Enter) {\n",
        "            if matches!(event.key, Key::Escape | Key::Enter) {\n",
        [CHORDS],
    ),
    (
        "Windows+Right moves the selection",
        "            Key::Right if plain => self.step_selection(1, extend),\n",
        "            Key::Right => self.step_selection(1, extend),\n",
        [CHORDS],
    ),
    (
        "Ctrl+Down moves the selection",
        "            Key::Down if plain => {\n",
        "            Key::Down => {\n",
        [CHORDS],
    ),
    (
        "Alt+End moves the selection",
        "            Key::End if plain => {\n",
        "            Key::End => {\n",
        [CHORDS],
    ),
    (
        "Alt+Enter opens the picture",
        "            Key::Enter if plain => {\n"
        "                if self.selected_photo.is_some()",
        "            Key::Enter => {\n"
        "                if self.selected_photo.is_some()",
        [CHORDS],
    ),
    (
        "Ctrl+Delete puts the selection in the trash",
        "            Key::Delete if plain => self.trash_selection(),\n",
        "            Key::Delete => self.trash_selection(),\n",
        [CHORDS],
    ),
    (
        "Windows+Space starts the slideshow",
        "            Key::Space if plain => {\n",
        "            Key::Space => {\n",
        [CHORDS],
    ),
    (
        "a command's letter is a binding",
        "        if textline::is_command(event.modifiers) {\n"
        "            return false;\n"
        "        }\n"
        "        let Some(ch) = event.typed().next() else {\n",
        "        let Some(ch) = event.typed().next() else {\n",
        [CHORDS],
    ),
    (
        "a character typed with AltGr is no binding",
        "        if textline::is_command(event.modifiers) {\n"
        "            return false;\n"
        "        }\n"
        "        let Some(ch) = event.typed().next() else {\n",
        "        if !textline::is_plain(event.modifiers) {\n"
        "            return false;\n"
        "        }\n"
        "        let Some(ch) = event.typed().next() else {\n",
        [CHORDS],
    ),
    (
        "a chord works the slideshow",
        "        if !textline::is_plain(event.modifiers) {\n"
        "            return false;\n"
        "        }\n"
        "        match event.key {\n"
        "            Key::Escape => {\n"
        "                self.stop_slideshow();\n",
        "        match event.key {\n"
        "            Key::Escape => {\n"
        "                self.stop_slideshow();\n",
        [CHORDS],
    ),
    (
        "a chorded Escape leaves a box",
        "            Key::Escape if plain => {\n                self.text_entry = None;\n",
        "            Key::Escape => {\n                self.text_entry = None;\n",
        [CHORDS],
    ),
    (
        "a chorded Enter makes the album",
        "            Key::Enter if plain => {\n                self.text_entry = None;\n",
        "            Key::Enter => {\n                self.text_entry = None;\n",
        [CHORDS],
    ),
    (
        # What the boxes did before the editor: type whatever text a key
        # carried, at the end.
        "a box types a command's letter, at its end",
        "        if self.entry_editor.text() != text {\n"
        "            self.entry_editor.set_text(&text);\n"
        "        }\n"
        "        let edit = textline::apply_key(\n",
        "        if event.types_text() {\n"
        "            let typed: String = event.typed().collect();\n"
        "            self.edit_entry(|t| t.push_str(&typed));\n"
        "            return;\n"
        "        }\n"
        "        if self.entry_editor.text() != text {\n"
        "            self.entry_editor.set_text(&text);\n"
        "        }\n"
        "        let edit = textline::apply_key(\n",
        [CHORDS, EDITS],
    ),
    (
        "the tag dialog types a command's letter",
        "        if let Event::Key(key) = event\n"
        "            && textline::is_command(key.modifiers)\n"
        "        {\n"
        "            return true;\n"
        "        }\n",
        "",
        [CHORDS],
    ),
    (
        "a box keeps the keyboard under the tag dialog",
        "        self.text_entry = None;\n        self.asking_tag = Some(dialog);\n",
        "        self.asking_tag = Some(dialog);\n",
        [CHORDS],
    ),
    (
        "the search box never has the keyboard's mark",
        "            focused: matches!(self.text_entry, Some(TextEntry::Search)) && !self.show_help,\n",
        "            focused: false,\n",
        [SEARCH],
    ),
    (
        "the search box keeps its mark under the list of keys",
        "            focused: matches!(self.text_entry, Some(TextEntry::Search)) && !self.show_help,\n",
        "            focused: matches!(self.text_entry, Some(TextEntry::Search)),\n",
        [SEARCH],
    ),
    (
        "a search that finds nothing is not red",
        "            invalid: !self.search_query.is_empty() && self.visible_photos().is_empty(),\n",
        "            invalid: false,\n",
        [SEARCH],
    ),
    (
        "the album's row never has the keyboard's mark",
        "                            focused: !self.show_help,\n",
        "                            focused: false,\n",
        [ALBUM],
    ),
    (
        "the album's row keeps its mark under the list of keys",
        "                            focused: !self.show_help,\n",
        "                            focused: true,\n",
        [ALBUM],
    ),
    (
        "the album's row is a label, not a field",
        "                        (self.album_name_box(), &self.text_entry)\n",
        "                        (None::<Rect>, &self.text_entry)\n",
        [ALBUM],
    ),
    (
        "an empty album name says nothing of what goes in it",
        '                            "Album name",\n',
        '                            "",\n',
        [ALBUM],
    ),
    (
        "an empty box with the keyboard has no caret",
        "                textedit::push_caret(\n"
        "                    &mut tree,\n"
        "                    x,\n"
        "                    y,\n"
        "                    line,\n"
        "                    self.palette.text,\n"
        "                    textedit::CARET_WIDTH,\n"
        "                );\n",
        "                let _ = (x, y, line);\n",
        [SEARCH, ALBUM],
    ),
    (
        "a box's caret is drawn at its start",
        "                    cursor: if has_keys {\n",
        "                    cursor: if false {\n",
        [SEARCH],
    ),
    (
        "a press in a box does nothing",
        "            self.press_entry(target, rect, event.x);\n",
        "            let _ = (target, rect);\n",
        [EDITS, ALBUM],
    ),
    (
        "a press leaves the caret where it was",
        "        self.entry_editor.set_cursor(cursor);\n",
        "        let _ = cursor;\n",
        [EDITS, ALBUM],
    ),
    (
        "a press in the album's row starts a new name",
        "        let row = self.album_name_row()?;\n",
        "        let row = self.album_name_row().filter(|_| false)?;\n",
        [ALBUM],
    ),
    (
        "a press beside the album's box starts a new name",
        "        row.contains(x, y).then_some((EntryBox::AlbumName, drawn))\n",
        "        drawn.contains(x, y).then_some((EntryBox::AlbumName, drawn))\n",
        [ALBUM],
    ),
    (
        "a box whose text changed is edited as it used to read",
        "        if self.entry_editor.text() != text {\n"
        "            self.entry_editor.set_text(&text);\n"
        "        }\n"
        "        let edit = textline::apply_key(\n",
        "        let edit = textline::apply_key(\n",
        [EDITS],
    ),
    (
        "a box holds any length",
        "            ENTRY_CAPACITY,\n            &self.entry_clipboard,\n",
        "            usize::MAX,\n            &self.entry_clipboard,\n",
        [EDITS],
    ),
    (
        "a copy or a cut takes nothing to the clipboard",
        "            self.entry_clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "an edit is not written back to its box",
        "            self.edit_entry(|text| *text = edited);\n",
        "            let _ = edited;\n",
        [EDITS, ALBUM],
    ),
    (
        "a box keeps the keyboard under the picker",
        "        self.text_entry = None;\n        self.picker.open_to_read();\n",
        "        self.picker.open_to_read();\n",
        [EDITS],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [SEARCH, ALBUM],
    ),
    (
        "Add tag asks for nothing",
        "            self.ask_tag();\n            return;\n",
        "            return;\n",
        [TAG_MENU],
    ),
    (
        "the tag dialog's answer is not acted on",
        "            self.commit_tag(&typed);\n",
        "            let _ = typed;\n",
        [TAG_MENU, TAG_ALL],
    ),
    (
        "the tag dialog stays up once answered",
        "        self.asking_tag = None;\n        // Cancel",
        "        // Cancel",
        [TAG_MENU],
    ),
    (
        "the tag dialog does not take the keys",
        "        if self.asking_tag.is_some() && matches!(event, Event::Key(_) | Event::Mouse(_)) {\n"
        "            return self.answer_tag(event);\n"
        "        }\n",
        "",
        [TAG_DIGIT],
    ),
    (
        "the tag dialog is not drawn",
        "            dialog.render(&palette, width, height, &mut tree);\n",
        "            let _ = (&palette, &dialog);\n",
        [TAG_SHOWN],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "photomanager", timeout=600, only=only))

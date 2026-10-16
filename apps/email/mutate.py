"""Mutation test for the mail client: its window over mail kept in files, the
codings it reads and writes mail in, and the store of folders, drafts and
marks.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

It drew a compose form it never showed, typed every key into an invisible
draft, showed a one-line preview as the message, drew no reading pane at all
"below", and had no mail it could ever show (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"
DECODE_SRC = Path(__file__).parent / "src" / "decode.rs"
STORE_SRC = Path(__file__).parent / "src" / "store.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a message opened is not marked read",
        "        self.mark_read(id);\n"
        "        self.remember(id);\n"
        "        self.active_panel = Panel::Reading;",
        "        self.remember(id);\n"
        "        self.active_panel = Panel::Reading;",
        ["a_mail_folder_is_read_and_its_marks_are_kept"],
    ),
    (
        "a mark is not kept",
        "        if let Some(path) = &self.flags_path {",
        "        if let Some(path) = None::<&PathBuf> {",
        ["a_mail_folder_is_read_and_its_marks_are_kept"],
    ),
    (
        "a Status header is not a read mark",
        "            seen: marks.seen || seen_in_file || own,",
        "            seen: marks.seen || own,",
        ["a_mail_folder_is_read_and_its_marks_are_kept"],
    ),
    (
        "mail read from files is deleted",
        "            Some(origin) if summary.flags.draft => {",
        "            Some(origin) if true => {",
        ["mail_read_from_files_is_never_deleted"],
    ),
    (
        "one Delete deletes a draft",
        "                if self.doomed != Some(id) {",
        "                if false {",
        ["a_draft_is_saved_edited_and_deleted"],
    ),
    (
        "saving a draft again makes a second file",
        "        let result = match &compose.saved_as {",
        "        let result = match &None::<PathBuf> {",
        ["a_draft_is_saved_edited_and_deleted"],
    ),
    (
        "a draft opens to be read, not written",
        "        if own && let Some(stored) = self.stored.get(&id) {",
        "        if false && let Some(stored) = self.stored.get(&id) {",
        ["a_draft_is_saved_edited_and_deleted"],
    ),
    (
        "closing unsaved work discards it at once",
        "        if compose.unsaved() && !compose.confirm_close {",
        "        if false {",
        ["closing_unsaved_work_asks_first"],
    ),
    (
        "a reply quotes the preview",
        "        if let Some(stored) = self.stored.get(&id) {\n"
        "            return Some(stored.message.readable_body().text);\n"
        "        }",
        "",
        ["a_message_is_read_whole_and_its_attachment_saved"],
    ),
    (
        "a forward leaves its attachments behind",
        "            if let Some(stored) = self.stored.get(&message_id) {\n"
        "                draft.attachments = stored",
        "            if let Some(stored) = None::<&store::Stored> {\n"
        "                draft.attachments = stored",
        ["every_control_answers_the_pointer"],
    ),
    (
        "the pane below draws nothing",
        "                    Some(Rect::new(x, top + list_h, w, (h - list_h).max(0.0))),",
        "                    None,",
        ["the_reading_pane_is_drawn_wherever_it_is"],
    ),
    (
        "Save as writes nothing",
        "                    self.status_message = match safeio::write_str_atomically(path, &built.text) {",
        "                    self.status_message = match Ok::<(), std::io::Error>(()) {",
        ["save_as_writes_a_message_that_reads_back"],
    ),
    (
        "an attachment is not saved",
        "                    Some(bytes) => match safeio::write_atomically(path, &bytes) {",
        "                    Some(bytes) => match Ok::<(), std::io::Error>(drop(bytes)) {",
        ["a_message_is_read_whole_and_its_attachment_saved"],
    ),
    (
        "a file opened is not listed",
        "                if !self.opened.iter().any(|p| p == path) {\n"
        "                    self.opened.push(path.to_path_buf());\n"
        "                }",
        "",
        ["a_file_is_opened_from_anywhere"],
    ),
    (
        "the list does not follow the keys",
        "                if self.selected_message != before {\n"
        "                    self.follow_selection();\n"
        "                }",
        "",
        ["the_message_list_scrolls_and_follows_the_keys"],
    ),
    (
        "the wheel does not scroll the message",
        "                self.read_scroll = next;",
        "                let _ = next;",
        ["a_message_is_read_whole_and_its_attachment_saved"],
    ),
    (
        "the search box cannot be pressed",
        "        f.hit(Target::SearchBox, search);",
        "",
        ["every_control_answers_the_pointer"],
    ),
    (
        "the notice does not say where mail comes from",
        "    \"An empty folder here is not \\u{201C}no new mail\\u{201D}. It reads mail kept in files",
        "    \"An empty folder here is empty. It reads mail kept in files",
        ["the_window_says_it_cannot_fetch_mail"],
    ),
    # The body is the toolkit's multi-line field
    # (requests/c-e-a-multi-line-text-field-for-the-apps-that-edit-text.md).
    (
        "the body grows past its capacity",
        "        <= BODY_CAPACITY\n}",
        "        <= usize::MAX\n}",
        ["the_body_stops_at_its_capacity"],
    ),
    (
        'a paste is not capped',
        '        return Some(guitk::clipboard::text());\n',
        '        return None;\n',
        ["the_body_stops_at_its_capacity"],
    ),
    (
        "a drag does not select",
        "                        compose.body.drag_to(event.x - left, event.y - top, &m);",
        "                        let _ = (left, top, &m);",
        ["a_drag_and_a_double_click_select_in_the_body"],
    ),
    (
        "a release goes on dragging",
        "                Some(compose) if compose.body_drag => {\n                    compose.body_drag = false;",
        "                Some(compose) if compose.body_drag => {\n                    compose.body_drag = true;",
        ["a_drag_and_a_double_click_select_in_the_body"],
    ),
    (
        "a double click is one click",
        "                    .press(event.x - left, event.y - top, 2, false, &m);",
        "                    .press(event.x - left, event.y - top, 1, false, &m);",
        ["a_drag_and_a_double_click_select_in_the_body"],
    ),
    (
        "the wheel does not scroll the body",
        "            compose\n                .body\n                .scroll_by(wheel::pixels(dy, m.line_height()), &m);",
        "            let _ = (dy, &m);",
        ["the_wheel_scrolls_the_body"],
    ),
    (
        "the caret's width setting is ignored",
        "        self.caret_width = settings.caret_width();",
        "        let _ = settings;",
        ["the_caret_is_as_wide_as_the_setting_says"],
    ),
    (
        "the compose form stops naming the clipboard keys",
        "  \\u{00B7}  Ctrl+A: select all  \\u{00B7}  Ctrl+C / Ctrl+X / Ctrl+V: copy, cut, paste",
        "",
        ["the_compose_forms_keys_are_all_on_its_line"],
    ),
    (
        'a chord raises the keys',
        '        if key.key == Key::F1 && plain {',
        '        if key.key == Key::F1 {',
        ['a_chord_is_neither_a_mail_key_nor_typing'],
    ),
    (
        'AltGr is taken for Ctrl in the mailbox',
        '        if textline::is_ctrl_chord(key.modifiers) {',
        '        if key.modifiers.ctrl {',
        ['a_chord_is_neither_a_mail_key_nor_typing'],
    ),
    (
        'a chord works the mailbox',
        '        if !plain {\n            return EventResult::Ignored;\n        }\n',
        '',
        ['a_chord_is_neither_a_mail_key_nor_typing'],
    ),
    (
        "a command's letter is typed into the search",
        "        let edit = textline::apply_key_on_clipboard(&mut self.search_editor, key, SEARCH_TEXT_SIZE);\n",
        "        self.search_editor.insert_text(&key.text);\n        let edit = textline::apply_key_on_clipboard(&mut self.search_editor, key, SEARCH_TEXT_SIZE);\n",
        ['a_chord_is_neither_a_mail_key_nor_typing'],
    ),
    (
        'a chord works the search box',
        "            Key::Escape if plain => {\n                self.searching = false;\n                self.set_search(\"\");\n",
        "            Key::Escape => {\n                self.searching = false;\n                self.set_search(\"\");\n",
        ['a_chord_is_neither_a_mail_key_nor_typing'],
    ),
    (
        "Alt's and the Windows key's chords reach a message",
        '        if textline::is_alt_or_windows_chord(key.modifiers) {',
        '        if false {',
        ['a_chord_is_neither_a_mail_key_nor_typing'],
    ),
    (
        'AltGr is taken for Ctrl in a message',
        '        let ctrl = textline::is_ctrl_chord(key.modifiers);',
        '        let ctrl = key.modifiers.ctrl;',
        ['a_chord_is_neither_a_mail_key_nor_typing'],
    ),
    (
        'Ctrl+Escape closes a message',
        '            Key::Escape if plain => self.close_compose(),',
        '            Key::Escape => self.close_compose(),',
        ['a_chord_is_neither_a_mail_key_nor_typing'],
    ),
    (
        "Ctrl+Tab walks a message's fields",
        '            Key::Tab if plain => {',
        '            Key::Tab => {',
        ['a_chord_is_neither_a_mail_key_nor_typing'],
    ),
    # -- the text boxes, the toolkit's fields (c-e-a-theme-can-shape-the-controls)
    (
        'the search box is drawn the same wherever the pointer is',
        '                hovered: self.hover == Some(Target::SearchBox),',
        '                hovered: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        'the search box is not marked while it has the keyboard',
        '                focused: self.searching,',
        '                focused: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        'a compose field is drawn the same wherever the pointer is',
        '                hovered: self.hover == Some(Target::Field(field)),',
        '                hovered: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        'the body is drawn the same wherever the pointer is',
        '                hovered: self.hover == Some(Target::Field(ComposeField::Body)),',
        '                hovered: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        "the text boxes take the toolkit's focus width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();',
        '        let _ = &settings;',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
]

DECODE_MUTATIONS = [
    (
        "Windows-1252's high table is ignored",
        "    if (0x80..=0x9F).contains(&b) {",
        "    if false {",
        ["character_sets_are_decoded_and_a_wrong_label_is_said"],
    ),
    (
        "the space between encoded words is kept",
        "            if !(after_word && before.chars().all(char::is_whitespace)) {",
        "            if true {",
        ["encoded_words_are_decoded_and_joined"],
    ),
    (
        "Q's underscore is not a space",
        "            b'_' => out.push(b' '),",
        "            b'_' => out.push(b'_'),",
        ["encoded_words_are_decoded_and_joined"],
    ),
    (
        "a semicolon in quotes splits a parameter",
        "        } else if c == ';' && !quoted {",
        "        } else if c == ';' {",
        ["parameters_are_unquoted_and_rfc_2231_is_put_together"],
    ),
    (
        "RFC 2231 pieces are taken in the order they come",
        "        parts.sort_by_key(|(index, _, _)| *index);",
        "",
        ["parameters_are_unquoted_and_rfc_2231_is_put_together"],
    ),
    (
        "a script is read as text",
        "                if !closing && (name == \"script\" || name == \"style\") {",
        "                if false {",
        ["html_reads_as_text"],
    ),
    (
        "an envelope line needs no date",
        "    has_sender && dated",
        "    has_sender",
        ["an_mbox_splits_into_its_messages"],
    ),
    (
        "a quoted From is not unquoted",
        "            if quoted > 0 && line.get(quoted..).is_some_and(|l| l.starts_with(b\"From \")) {",
        "            if false {",
        ["an_mbox_splits_into_its_messages"],
    ),
    (
        "a date's zone is ignored",
        "            .saturating_sub(offset_minutes.saturating_mul(60)),",
        "            .saturating_sub(0),",
        ["dates_are_read_to_seconds"],
    ),
    (
        "quoted-printable leaves = as it is",
        "                ((b == b' ' || b == b'\\t') && !last) || ((33..=126).contains(&b) && b != b'=');",
        "                ((b == b' ' || b == b'\\t') && !last) || (33..=126).contains(&b);",
        ["quoted_printable_round_trips"],
    ),
    (
        "quoted-printable lines run long",
        "            if width.saturating_add(piece_len) > limit {",
        "            if false {",
        ["quoted_printable_round_trips"],
    ),
    (
        "an encoded word runs long",
        "        if chunk.len().saturating_add(c.len_utf8()) > 45 {",
        "        if false {",
        ["header_words_round_trip"],
    ),
]

STORE_MUTATIONS = [
    (
        "a folder with no extension is not an mbox",
        "        decode::is_envelope(first)",
        "        false",
        ["the_mail_directory_lists_its_folders"],
    ),
    (
        "a mark is not kept under the Message-ID",
        "            |id| format!(\"id:{id}\"),",
        "            |_| String::from(\"id:\"),",
        ["a_folder_is_read_message_by_message"],
    ),
    (
        "any file is taken for a marks file",
        "        if lines.next() != Some(FLAGS_HEADER) {",
        "        if false {",
        ["flags_are_kept_in_a_file_of_their_own"],
    ),
    (
        "a tab in a key breaks the marks file",
        "            '\\t' => out.push_str(\"\\\\t\"),",
        "            '\\t' => out.push('\\t'),",
        ["flags_are_kept_in_a_file_of_their_own"],
    ),
    (
        "a draft's name keeps a slash",
        "            if c.is_control() || matches!(c, '/' | '\\\\' | ':' | '*' | '?' | '\"' | '<' | '>' | '|') {",
        "            if c.is_control() {",
        ["a_draft_is_named_for_its_subject"],
    ),
]

# The compose fields' right-click menu (2026-10-10, requests/c-e-text-fields-
# have-a-right-click-menu.md): Cut, Copy, Paste, Delete and Select all (Undo
# and Redo in the body) through apps/fieldmenu; the lines, the body and the
# menu share the program's clipboard; each line holds its capacity and the
# body refuses a paste past what a message holds, as by key.
E_COPY = "a_fields_right_click_menu_copies_and_pastes_between_fields"
E_ESC = "escape_puts_a_fields_menu_away_and_leaves_the_message_open"
E_CAP = "a_paste_from_the_menu_stops_at_each_fields_capacity"
E_CLOCK = "the_menu_is_drawn_and_a_dimmed_row_says_why_on_its_own_clock"

MUTATIONS += [
    (
        "a right press on a field opens no menu",
        "            MouseEventKind::Press(MouseButton::Right) => self.open_field_menu(event.x, event.y),\n",
        "",
        [E_COPY, E_ESC],
    ),
    (
        "the right-click leaves the keyboard where it was",
        "        compose.field = field;\n        self.field_menu\n            .open(MenuField::Compose(field), menu, x, y, viewport);\n",
        "        self.field_menu\n            .open(MenuField::Compose(field), menu, x, y, viewport);\n",
        [E_COPY],
    ),
    (
        "a row chosen for a line does nothing",
        "                Some(input) => input.edit_command(id),\n",
        "                Some(_input) => KeyEdit::Unhandled,\n",
        [E_COPY],
    ),
    (
        "a row chosen for the body does nothing",
        "            compose.body.edit_command(id, &m)\n",
        "            KeyEdit::Unhandled\n",
        [E_COPY],
    ),
    (
        "a paste from the menu goes past the body's cap",
        "            if pasting && !body_fits(&compose.body, &guitk::clipboard::text()) {\n"
        "                return;\n"
        "            }\n",
        "",
        [E_CAP],
    ),
    (
        'a line holds no capacity',
        '    textline::field_holding(text, FIELD_CAPACITY)\n',
        '    textline::field_holding(text, usize::MAX)\n',
        [E_CAP],
    ),
    (
        "the lines' keys keep a clipboard the menu does not share",
        "let _edit = textline::apply_key_on_clipboard(input, key, 13.0);",
        'let _edit = textline::apply_key(input, key, "", 13.0);',
        [E_COPY],
    ),
    (
        "Escape reaches the form under a field's menu",
        '            fieldmenu::Outcome::Taken => return EventResult::Consumed,\n',
        '            fieldmenu::Outcome::Taken => {}\n',
        [E_ESC],
    ),
    (
        "a field's menu is never drawn",
        "        tree.commands.extend(self.field_menu.render(&self.palette));\n",
        "",
        [E_CLOCK],
    ),
    (
        "the window's ticks do not move the menu's clock",
        '            return if self.field_menu.tick(*elapsed_ms) {\n',
        '            return if self.field_menu.tick(0) {\n',
        [E_CLOCK],
    ),
    (
        "the window asks no tick for the menu's wait",
        '        self.field_menu.due_in().map(Duration::from_millis)\n',
        '        self.field_menu.due_in().map(Duration::from_millis).filter(|_| false)\n',
        [E_CLOCK],
    ),
]

MUTATIONS += [
    (
        'a release the menu passes on is kept from the window',
        '        match self.field_menu.event(event) {\n            fieldmenu::Outcome::Window => {}\n',
        '        match self.field_menu.event(event) {\n            fieldmenu::Outcome::Window\n                if self.field_menu.is_up()\n                    && matches!(\n                        event,\n                        Event::Mouse(MouseEvent {\n                            kind: MouseEventKind::Release(_),\n                            ..\n                        })\n                    ) =>\n            {\n                return EventResult::Consumed;\n            }\n            fieldmenu::Outcome::Window => {}\n',
        ["a_body_drag_ends_at_its_release_though_a_menu_came_up_in_it"],
    ),
]

# The search box edits at a caret, on the program's clipboard, with a
# right-click menu (2026-10-16).
S_CARET = "the_search_box_edits_at_a_caret"
S_PRESS = "a_press_in_the_search_box_puts_the_caret_there"
S_SYNC = "a_search_emptied_by_the_list_is_empty_in_the_box"
S_MENU = "the_search_boxs_right_click_menu_edits_the_search"

MUTATIONS += [
    (
        "the search types only at its end",
        "        let edit = textline::apply_key_on_clipboard(&mut self.search_editor, key, SEARCH_TEXT_SIZE);\n",
        "        let end = self.search_editor.text().len();\n        self.search_editor.set_cursor(text::TextCursor::from(end));\n        let edit = textline::apply_key_on_clipboard(&mut self.search_editor, key, SEARCH_TEXT_SIZE);\n",
        [S_CARET],
    ),
    (
        "the search box's editor is not reloaded",
        "        self.sync_search_editor();\n        let edit = textline::apply_key_on_clipboard(&mut self.search_editor, key, SEARCH_TEXT_SIZE);\n",
        "        let edit = textline::apply_key_on_clipboard(&mut self.search_editor, key, SEARCH_TEXT_SIZE);\n",
        [S_SYNC],
    ),
    (
        "what is typed is not searched for",
        "        let typed = self.search_editor.text().to_owned();\n        self.set_search(&typed);\n        if edit.handled {\n",
        "        if edit.handled {\n",
        [S_CARET],
    ),
    (
        "a key typed into the search is reported unanswered",
        "        self.set_search(&typed);\n        if edit.handled {\n",
        "        self.set_search(&typed);\n        if false {\n",
        [S_CARET],
    ),
    (
        "the search box answers the window's chords",
        "        self.set_search(&typed);\n        if edit.handled {\n",
        "        self.set_search(&typed);\n        if true {\n",
        [S_CARET],
    ),
    (
        "the box with the keyboard is drawn as any other",
        "        if self.searching {\n            // The box with the keyboard:",
        "        if false {\n            // The box with the keyboard:",
        [S_CARET],
    ),
    (
        "a press in the search box puts the caret nowhere",
        "            Target::SearchBox => self.place_search_caret(search_box_rect(), x),\n",
        "            Target::SearchBox => self.searching = true,\n",
        [S_PRESS],
    ),
    (
        "a right press on the search box opens no menu",
        "            Some(Target::SearchBox) => {\n                self.sync_search_editor();\n",
        "            Some(Target::SearchBox) if false => {\n                self.sync_search_editor();\n",
        [S_MENU],
    ),
    (
        "the search's right-click leaves the keyboard where it was",
        "                self.searching = true;\n                let menu = self.search_editor.edit_menu();\n",
        "                let menu = self.search_editor.edit_menu();\n",
        [S_MENU],
    ),
    (
        "a row chosen does nothing to the search",
        "                if self.search_editor.edit_command(id) == KeyEdit::Changed {\n",
        "                if false {\n",
        [S_MENU],
    ),
    (
        "a paste from the menu is not searched for",
        "                    let typed = self.search_editor.text().to_owned();\n                    self.set_search(&typed);\n                }\n                return EventResult::Consumed;\n",
        "                }\n                return EventResult::Consumed;\n",
        [S_MENU],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing, which is right for one
    # table and made every filtered run of this three-table file fail.
    only = sys.argv[1:]
    tables = [
        (SRC, MUTATIONS),
        (DECODE_SRC, DECODE_MUTATIONS),
        (STORE_SRC, STORE_MUTATIONS),
    ]
    names = [name for _, rows in tables for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    results = [0]
    for src, rows in tables:
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        results.append(sweep(src, rows, "email", timeout=900, only=mine or None))
    raise SystemExit(max(results))

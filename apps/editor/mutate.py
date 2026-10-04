"""Mutation test for the text editor's close: asking before unsaved work is
lost, with a tab's close or the window's.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Closing a modified tab was refused with a status message offering
Ctrl+Shift+W to discard -- a key nothing bound -- and closing the window went
at once whatever it held.  Both ask now, and the window stays open while they
do (`Response::KeepOpen`, which lane F added for this).

The table covers the close, and -- since 2026-09-28 (C-Q24, §1416) -- the
history kept as a tree: an edit made after undoing starts a branch and the
undone one is kept, reachable with Alt+Z and Alt+Shift+Z; Ctrl+F4 closes the
tab; and AltGr, which arrives as Ctrl+Alt, types its letter. The rest of the
editor's suite predates it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

MAIN_SRC = Path(__file__).parent / "src" / "main.rs"
INPUT_SRC = Path(__file__).parent / "src" / "input.rs"

# (name, old, new, [tests that must fail])
MAIN_MUTATIONS = [
    (
        "the window closes over unsaved work",
        "        if self.tabs.iter().any(|d| d.modified) {",
        "        if false {",
        ["closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept"],
    ),
    (
        "Save on close saves nothing",
        "            (CloseScope::Window, Choice::Save) => self.continue_quitting(),",
        "            (CloseScope::Window, Choice::Save) => self.quit = true,",
        ["saving_on_close_writes_what_has_a_file_and_asks_where_for_the_rest"],
    ),
    (
        "an untitled document is not asked about",
        "                && doc.path.is_some()\n",
        "",
        ["saving_on_close_writes_what_has_a_file_and_asks_where_for_the_rest"],
    ),
    # -- the history as a tree (C-Q24, §1416) --------------------------------
    (
        "a journey takes its steps the wrong way",
        "                Travel::Undo(action) => self.revert(&action),",
        "                Travel::Undo(action) => self.reapply(&action),",
        ["a_new_edit_after_an_undo_starts_a_branch_and_keeps_the_undone_one"],
    ),
    (
        "Alt+Z goes forward in time",
        "        let steps = self.history.earlier();",
        "        let steps = self.history.later();",
        ["a_new_edit_after_an_undo_starts_a_branch_and_keeps_the_undone_one"],
    ),
    (
        "a journey that goes nowhere is said to have moved",
        "        let moved = !steps.is_empty();",
        "        let moved = true;",
        ["there_is_nothing_before_the_first_version_or_after_the_newest"],
    ),
    (
        "a step made again leaves the caret where it was",
        "        (self.cursor_line, self.cursor_col) = action.cursor_after;",
        "        let _ = action.cursor_after;",
        ["a_new_edit_after_an_undo_starts_a_branch_and_keeps_the_undone_one"],
    ),
    (
        "a reload keeps the old buffer's history",
        "        self.history.clear();",
        "",
        ["a_reload_starts_the_history_again"],
    ),
]

INPUT_MUTATIONS = [
    (
        "the question is drawn into a window the loop has closed",
        "                    Response::KeepOpen",
        "                    Response::Redraw",
        ["closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept"],
    ),
    (
        "keys go to the document under the question",
        "            return self.question_event(&Event::Key(key.clone()));",
        "            let _ = self.question_event(&Event::Key(key.clone()));",
        ["closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept"],
    ),
    (
        "the question's buttons take no click",
        "            return self.question_event(&Event::Mouse(mouse.clone()));",
        "            let _ = mouse;",
        ["the_questions_buttons_answer_the_pointer"],
    ),
    (
        "Ctrl+W closes a modified tab without asking",
        "    fn close_active_tab(&mut self) {\n        let idx = self.tabs.active_index();\n        self.request_close_tab(idx);",
        "    fn close_active_tab(&mut self) {\n        self.tabs.close_active();",
        ["closing_a_modified_tab_asks_first"],
    ),
    (
        "a modified tab's close box closes it",
        "                self.request_close_tab(index);",
        "                self.tabs.set_active(index);\n                self.tabs.close_active();",
        ["a_modified_tabs_close_box_asks_first"],
    ),
    (
        "the last answer from the dialog does not close the window",
        "            return if self.quit { Response::Exit } else { response };",
        "            return response;",
        ["saving_on_close_writes_what_has_a_file_and_asks_where_for_the_rest"],
    ),
    (
        "saving the untitled document does not carry on closing",
        "                        self.continue_quitting();",
        "",
        ["saving_on_close_writes_what_has_a_file_and_asks_where_for_the_rest"],
    ),
    # -- the keys C-Q24 asks for (§1416) --------------------------------------
    (
        "Alt+Z is not a key",
        "        if key.key == Key::Z && key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key",
        "        if false && key.key == Key::Z && key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key",
        ["every_shortcut_a_menu_advertises_is_really_bound"],
    ),
    (
        "Alt+Shift+Z goes back as Alt+Z does",
        "            return self.run(if key.modifiers.shift {\n                Command::Later",
        "            return self.run(if key.modifiers.shift {\n                Command::Earlier",
        ["every_shortcut_a_menu_advertises_is_really_bound"],
    ),
    (
        "AltGr is taken for Ctrl",
        "        if key.modifiers.ctrl && !key.modifiers.alt {",
        "        if key.modifiers.ctrl {",
        ["an_altgr_letter_is_typed_not_taken_for_a_chord"],
    ),
    (
        "Ctrl+F4 closes nothing",
        "            Key::W | Key::F4 => self.run(Command::CloseTab),",
        "            Key::W => self.run(Command::CloseTab),",
        ["ctrl_f4_closes_the_current_document"],
    ),
    (
        "Earlier Version is offered at the first version",
        "            Command::Undo | Command::Earlier => doc.history.can_undo(),",
        "            Command::Undo => doc.history.can_undo(),\n            Command::Earlier => true,",
        ["the_ends_of_the_history_are_said"],
    ),
    (
        "the newest version is not said",
        "                    self.status = Some(String::from(\"This is the newest version\"));",
        "",
        ["the_ends_of_the_history_are_said"],
    ),
    (
        "the menu does not name Ctrl+F4",
        "            Self::CloseTab => \"Ctrl+W / Ctrl+F4\",",
        "            Self::CloseTab => \"Ctrl+W\",",
        ["ctrl_f4_closes_the_current_document"],
    ),
    (
        "Ctrl+F4 does not close the document",
        "            Key::W | Key::F4 => self.run(Command::CloseTab),",
        "            Key::W => self.run(Command::CloseTab),",
        [
            "ctrl_f4_closes_the_current_document",
            "every_shortcut_a_menu_advertises_is_really_bound",
        ],
    ),
]

FIND_BAR = "the_find_bar_takes_the_pointer_and_its_boxes_are_the_toolkits_fields"

# The find bar takes the pointer over itself, and its boxes are the toolkit's
# fields (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
MAIN_MUTATIONS += [
    (
        "a find box never lights",
        "            hovered: open && self.find_box_under_pointer() == Some(field),\n",
        "            hovered: false,\n",
        [FIND_BAR],
    ),
    (
        "both find boxes are marked",
        "            focused: open && self.find_field == field,\n",
        "            focused: open,\n",
        [FIND_BAR],
    ),
    (
        "a find box shows the keys under an open menu",
        "        self.external_prompt.is_some() || self.question.is_some() || self.menu_bar.is_open()\n",
        "        self.external_prompt.is_some() || self.question.is_some()\n",
        [FIND_BAR],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIND_BAR],
    ),
]

INPUT_MUTATIONS += [
    (
        "a change in the find box's light asks for no redraw",
        "                (self.find_box_under_pointer() != before).then_some(Response::Redraw)\n",
        "                None\n",
        [FIND_BAR],
    ),
    (
        "a press on the find bar goes through to the text",
        "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_)\n"
        "                if self.find_visible && self.find_panel_rect().contains(mouse.x, mouse.y) =>\n",
        "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_)\n"
        "                if false && self.find_visible =>\n",
        [FIND_BAR],
    ),
    (
        "a press on a find box gives it no keys",
        "                    Some(field) if field != self.find_field => {\n",
        "                    Some(field) if false && field != self.find_field => {\n",
        [FIND_BAR],
    ),
]

# The list of keys (2026-10-04): F1 did nothing, and the menus printed only
# their own rows' keys -- not Ctrl+T, Alt+Z, the find bar's or the moves.
ADDS = "every_key_the_list_adds_does_something"
REACHES = "the_shortcut_list_reaches_the_window"
MODAL = "the_shortcut_list_takes_the_keys_and_a_press"
MENU = "every_shortcut_a_menu_advertises_is_really_bound"

F1_ANCHOR = "        if plain && key.key == Key::F1 {\n"
CLOSE_ANCHOR = "            if plain && matches!(key.key, Key::F1 | Key::Escape) {\n"
PRESS_ANCHOR = (
    "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
    "                    self.show_help = false;\n"
    "                    return Response::Redraw;\n"
)

INPUT_MUTATIONS += [
    (
        "the list of keys never comes up",
        F1_ANCHOR + "            self.show_help = true;\n",
        F1_ANCHOR,
        [REACHES],
    ),
    (
        "Alt+F1 raises the list",
        F1_ANCHOR,
        "        if key.key == Key::F1 {\n",
        [REACHES],
    ),
    (
        "the list is not modal for the keys",
        "            return Response::Redraw;\n        }\n        if self.external_prompt.is_some() {\n",
        "        }\n        if self.external_prompt.is_some() {\n",
        [MODAL],
    ),
    (
        "Escape leaves the list up",
        CLOSE_ANCHOR,
        "            if plain && matches!(key.key, Key::F1) {\n",
        [REACHES],
    ),
    (
        "Alt+Escape puts the list away",
        CLOSE_ANCHOR,
        "            if matches!(key.key, Key::F1 | Key::Escape) {\n",
        [REACHES],
    ),
    (
        "a press leaves the list up",
        PRESS_ANCHOR,
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    return Response::Redraw;\n",
        [MODAL],
    ),
    (
        "a press reaches what the list covers",
        PRESS_ANCHOR,
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n",
        [MODAL],
    ),
    (
        "the wheel scrolls what the list covers",
        "                MouseEventKind::Release(_) => {}\n                _ => return Response::Idle,\n",
        "                _ => {}\n",
        [MODAL],
    ),
    (
        "Ctrl+Shift+Z, advertised beside Ctrl+Y, does not redo",
        "            Key::Z => self.run(if shift { Command::Redo } else { Command::Undo }),\n",
        "            Key::Z => self.run(Command::Undo),\n",
        [MENU],
    ),
]

MAIN_MUTATIONS += [
    (
        "the list of keys is not drawn",
        "        if self.show_help {\n            guitk::shortcut::render_card(\n",
        "        if false {\n            guitk::shortcut::render_card(\n",
        [REACHES],
    ),
]

if __name__ == "__main__":
    # Each table is given only the filters that name one of its own rows: the
    # harness refuses a filter that selects nothing in its table, so handing
    # both tables every filter refused any run that named rows in one.
    only = sys.argv[1:]
    tables = [(MAIN_SRC, MAIN_MUTATIONS), (INPUT_SRC, INPUT_MUTATIONS)]
    names = [name for _, rows in tables for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    results = []
    for src, rows in tables:
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        results.append(sweep(src, rows, "editor", timeout=900, only=mine or None))
    raise SystemExit(max(results, default=0))

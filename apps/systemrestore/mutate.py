"""Mutation test for System Restore's real restore points.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-27, when the window stopped showing
five invented restore points and began keeping real ones in `apps/snapstore`:
what a restore point holds and how the list of them is kept, what a restore
does (keep the folder first, then make it match), what deleting does to the
tree, the schedule and its retention policy, and the window's refusals -- a
damaged list is never written over, running work is never abandoned, a
close waits for it, a locked point is not deleted.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

RESTORE = "a_restore_puts_the_files_back_and_keeps_the_ones_it_replaced"
REFUSED = "a_list_that_is_not_one_is_refused"
KEPT = "every_component_says_whether_it_can_be_kept"
TAKEN = "a_restore_point_is_taken_kept_and_listed"
DELETE = "deleting_a_restore_point_keeps_the_branches"
RETENTION = "retention_removes_only_scheduled_restore_points"
FIRST = "a_first_run_opens_on_nothing"
DAMAGED = "a_damaged_list_is_never_written_over"
CLOSE = "closing_waits_for_the_work"
OVERLAY = "the_overlay_holds_until_the_work_has_ended"
LOCKED = "a_locked_restore_point_is_not_deleted"
SCHEDULE = "the_schedule_is_changed_from_its_view"

POINTS = [
    (
        "a restore leaves what was added since",
        "                        mirror: true,",
        "                        mirror: false,",
        [RESTORE],
    ),
    (
        "a list is read in part",
        '        points.push(point_from_json(v).ok_or_else(|| bad("a point is incomplete"))?);',
        "        if let Some(p) = point_from_json(v) {\n            points.push(p);\n        }",
        [REFUSED],
    ),
    (
        "a current point not in the list is accepted",
        "    if current.is_some_and(|id| tree.get_snapshot(id).is_none()) {",
        "    if false {",
        [REFUSED],
    ),
    (
        "the desktop is captured as a component of its own",
        "            Self::DesktopConfig => Err(KEPT_WITH_PROGRAMS),",
        "            Self::DesktopConfig => loc.settings.clone().ok_or(NO_HOME),",
        [KEPT],
    ),
]

MAIN = [
    (
        "a new restore point hangs from nothing",
        "            .filter(|id| self.manager.tree.get_snapshot(*id).is_some());\n"
        "        let components = captured.stored.keys().copied().collect();",
        "            .filter(|_| false);\n"
        "        let components = captured.stored.keys().copied().collect();",
        [TAKEN],
    ),
    (
        "deleting keeps no branches",
        "        for child in tree.children_of(id).to_vec() {",
        "        for child in Vec::<u64>::new() {",
        [DELETE],
    ),
    (
        "retention removes restore points somebody took",
        "            .filter(|s| s.snapshot_type == SnapshotType::Scheduled)\n",
        "",
        [RETENTION],
    ),
    (
        "retention removes the current restore point",
        ".filter(|id| Some(*id) != self.current && self.tree.children_of(*id).is_empty())",
        ".filter(|id| self.tree.children_of(*id).is_empty())",
        [RETENTION],
    ),
    (
        "a schedule that is off still runs",
        "        if !self.enabled {\n            return false;\n        }\n",
        "",
        [FIRST],
    ),
    (
        "the list is not saved",
        "        match points::save(&store, &self.manager) {",
        "        match Ok::<(), std::io::Error>(()) {",
        [TAKEN],
    ),
    (
        "a list that did not read is written over",
        "        if let Some(why) = &self.load_error {\n            return Some(format!(",
        "        if let Some(why) = None::<&String> {\n            return Some(format!(",
        [DAMAGED],
    ),
    (
        "a close does not wait for the work",
        "            if self.work.is_some() {\n                self.close_when_done = true;",
        "            if false {\n                self.close_when_done = true;",
        [CLOSE],
    ),
    (
        "running work can be abandoned",
        "            if plain && progress.complete && matches!(key.key, Key::Escape | Key::Enter) {",
        "            if plain && matches!(key.key, Key::Escape | Key::Enter) {",
        [OVERLAY],
    ),
    (
        "a locked restore point is deleted",
        "        } else if point.locked {",
        "        } else if false {",
        [LOCKED],
    ),
    (
        "Space does nothing in the Schedule view",
        "                Key::Space => return self.schedule_control(ScheduleControl::Toggle),\n",
        "",
        [SCHEDULE],
    ),
    (
        'a chord raises the list of keys',
        '        if plain && (key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift)) {',
        '        if key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift) {',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
    (
        'AltGr is taken for Ctrl in the list',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return match key.key {',
        '        if key.modifiers.ctrl {\n            return match key.key {',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
    (
        'Alt+Tab changes the view',
        '            Key::Tab if plain => {\n                self.cycle_view(',
        '            Key::Tab => {\n                self.cycle_view(',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
    (
        'Alt+Enter asks to restore',
        '            Key::Enter if plain => {\n                if let Some(id) = self.selected_id {\n                    self.dialog = DialogKind::ConfirmRestore(id);',
        '            Key::Enter => {\n                if let Some(id) = self.selected_id {\n                    self.dialog = DialogKind::ConfirmRestore(id);',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
    (
        'Alt+Delete asks to delete',
        '            Key::Delete if plain => {\n                if let Some(id) = self.selected_id {\n                    self.dialog = DialogKind::ConfirmDelete(id);',
        '            Key::Delete => {\n                if let Some(id) = self.selected_id {\n                    self.dialog = DialogKind::ConfirmDelete(id);',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
    (
        'Alt+Backspace deletes from the search',
        '            Key::Backspace if !textline::is_alt_or_windows_chord(key.modifiers) => {\n                self.search_query.pop();',
        '            Key::Backspace => {\n                self.search_query.pop();',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
    (
        "the search types a command's letter",
        '            _ if !textline::types_into_field(key) => EventResult::Ignored,\n',
        '',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chorded Escape closes the dialog',
        '            Key::Escape if plain => {\n                self.dialog = DialogKind::None;',
        '            Key::Escape => {\n                self.dialog = DialogKind::None;',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chorded Enter answers the dialog',
        '            Key::Enter if plain => {\n                self.confirm_dialog();',
        '            Key::Enter => {\n                self.confirm_dialog();',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
    (
        "Alt+Tab walks the form's fields",
        '            Key::Tab if plain && self.dialog == DialogKind::CreateSnapshot => {',
        '            Key::Tab if self.dialog == DialogKind::CreateSnapshot => {',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
    # The form's keys are textline's editor's since 2026-10-04, which refuses
    # Alt's and the Windows key's chords and types only what a key typed: the
    # rule is textline's, and its own table covers it. What is this program's
    # is handing the editor the key as it came -- chord and all.
    (
        "the form's editor is given the key without its chord",
        '            &mut self.form_editor,\n            key,\n',
        '            &mut self.form_editor,\n'
        '            &KeyEvent {\n'
        '                modifiers: guitk::event::Modifiers::NONE,\n'
        '                ..key.clone()\n'
        '            },\n',
        ['a_chord_is_neither_a_restore_key_nor_typing_and_altgr_types'],
    ),
]

CARD = 'the_shortcut_card_takes_a_press_rather_than_passing_it_on'

MAIN += [
    # The list of keys takes the pointer (2026-10-04; known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        'a press goes through the list of keys',
        '        if self.show_help {\n'
        '            return match mouse.kind {\n'
        '                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                    self.show_help = false;\n'
        '                    EventResult::Consumed\n'
        '                }\n'
        '                _ => EventResult::Ignored,\n'
        '            };\n'
        '        }\n',
        '',
        [CARD],
    ),
    (
        'only the left button puts the list of keys away',
        '                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                    self.show_help = false;\n'
        '                    EventResult::Consumed\n',
        '                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n'
        '                    self.show_help = false;\n'
        '                    EventResult::Consumed\n',
        [CARD],
    ),
    (
        'a press under the list of keys leaves it up',
        '                    self.show_help = false;\n'
        '                    EventResult::Consumed\n'
        '                }\n'
        '                _ => EventResult::Ignored,\n'
        '            };\n',
        '                    EventResult::Consumed\n'
        '                }\n'
        '                _ => EventResult::Ignored,\n'
        '            };\n',
        [CARD],
    ),
    (
        'the wheel scrolls what the list of keys covers',
        '        if self.show_help {\n'
        '            return match mouse.kind {\n',
        '        if self.show_help && !matches!(mouse.kind, MouseEventKind::Scroll { .. }) {\n'
        '            return match mouse.kind {\n',
        [CARD],
    ),
]

# The Create Snapshot form's two boxes and the search box are the toolkit's
# field; the form's are edited by textline's editor, and the description's
# text is drawn at last (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
FORM = "the_form_boxes_are_the_toolkits_field"
EDITS = "the_form_boxes_edit_like_a_field"
SEARCH = "the_search_box_is_the_toolkits_field"
FOLDS = "the_search_finds_a_name_whatever_the_case_of_its_letters"
SEARCH_FOCUS = (
    "            focused: self.dialog == DialogKind::None && self.progress.is_none() && !self.show_help,\n"
)

MAIN += [
    (
        "the form box with the keyboard has no mark",
        "            focused: field == self.form_field && !self.show_help,\n",
        "            focused: false,\n",
        [FORM],
    ),
    (
        "the form keeps its mark under the list of keys",
        "            focused: field == self.form_field && !self.show_help,\n",
        "            focused: field == self.form_field,\n",
        [FORM],
    ),
    (
        "the empty name says nothing of what it will be called",
        "            self.render_form_box(rt, field, rect, &self.default_point_name());\n",
        '            self.render_form_box(rt, field, rect, "Enter snapshot name...");\n',
        [FORM],
    ),
    (
        "the description is never drawn",
        '            self.render_form_box(rt, field, rect, "Optional");\n',
        "            let _ = (field, rect);\n",
        [FORM],
    ),
    (
        "an empty box with the keyboard has no caret",
        "                textedit::push_caret(rt, x, y, line, self.palette.text, textedit::CARET_WIDTH);\n",
        "                let _ = (x, y, line);\n",
        [FORM, SEARCH],
    ),
    (
        "Tab leaves the keyboard where it was",
        "                self.focus_form_field(match self.form_field {\n",
        "                let _ = (match self.form_field {\n",
        [FORM, EDITS],
    ),
    (
        "the form types only at its end",
        "        if self.form_editor.text() != text {\n"
        "            self.form_editor.set_text(&text);\n"
        "        }\n"
        "        let edit = textline::apply_key(\n",
        "        if textline::types_into_field(key) {\n"
        "            self.form_text_mut().push_str(&key.text);\n"
        "            return EventResult::Consumed;\n"
        "        }\n"
        "        if self.form_editor.text() != text {\n"
        "            self.form_editor.set_text(&text);\n"
        "        }\n"
        "        let edit = textline::apply_key(\n",
        [EDITS],
    ),
    (
        "a box whose text changed is edited as it used to read",
        "        if self.form_editor.text() != text {\n"
        "            self.form_editor.set_text(&text);\n"
        "        }\n"
        "        let edit = textline::apply_key(\n",
        "        let edit = textline::apply_key(\n",
        [EDITS],
    ),
    (
        "a box holds any length",
        "            FORM_CAPACITY,\n            &self.form_clipboard,\n",
        "            usize::MAX,\n            &self.form_clipboard,\n",
        [EDITS],
    ),
    (
        "a copy or a cut takes nothing to the clipboard",
        "            self.form_clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "a key the box does not answer is taken",
        "        if !edit.handled {\n            return EventResult::Ignored;\n        }\n",
        "",
        [EDITS],
    ),
    (
        "an edit is not written back to the form",
        "            *self.form_text_mut() = edited;\n",
        "            let _ = edited;\n",
        [EDITS],
    ),
    (
        "a press in a box does nothing",
        "                    self.press_form_field(field, rect, x);\n",
        "                    let _ = (field, rect);\n",
        [EDITS],
    ),
    (
        "a press leaves the caret where it was",
        "        self.form_editor.set_selection_anchor(None);\n"
        "        self.form_editor.set_cursor(cursor);\n",
        "        let _ = cursor;\n",
        [EDITS],
    ),
    (
        "a press moves the keyboard but not the box's text",
        "        } else {\n            self.focus_form_field(field);\n        }\n",
        "        } else {\n            self.form_field = field;\n        }\n",
        [EDITS],
    ),
    (
        "the search box never has the keyboard",
        SEARCH_FOCUS,
        "            focused: false,\n",
        [SEARCH],
    ),
    (
        "the search box keeps its mark under a dialog",
        SEARCH_FOCUS,
        "            focused: self.progress.is_none() && !self.show_help,\n",
        [SEARCH],
    ),
    (
        "the search box keeps its mark under the work",
        SEARCH_FOCUS,
        "            focused: self.dialog == DialogKind::None && !self.show_help,\n",
        [SEARCH],
    ),
    (
        "the search box keeps its mark under the list of keys",
        SEARCH_FOCUS,
        "            focused: self.dialog == DialogKind::None && self.progress.is_none(),\n",
        [SEARCH],
    ),
    (
        "a search that finds nothing is not red",
        "            invalid: !self.search_query.is_empty() && self.visible_rows().is_empty(),\n",
        "            invalid: false,\n",
        [SEARCH],
    ),
    (
        "the search's caret is at its start",
        "                TextCursor::from(self.search_query.len()),\n",
        "                TextCursor::default(),\n",
        [SEARCH],
    ),
    (
        "the search folds only ASCII's letters",
        "        let q = self.search_query.to_lowercase();\n",
        "        let q = self.search_query.to_ascii_lowercase();\n",
        [FOLDS],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FORM, SEARCH],
    ),
]

TABLES = {
    "points.rs": POINTS,
    "main.rs": MAIN,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "systemrestore", timeout=900, only=mine or None))
    raise SystemExit(worst)

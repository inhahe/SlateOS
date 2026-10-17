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
    # The form's keys are textline's since 2026-10-04 -- each box a LineBox
    # since 2026-10-17 -- which refuses Alt's and the Windows key's chords and
    # types only what a key typed: the rule is textline's, and its own table
    # covers it. What is this program's is handing the box the key as it
    # came -- chord and all.
    (
        "the form's box is given the key without its chord",
        "let edit = self.form_box_mut(field).key(key);",
        "let edit = self.form_box_mut(field).key(&KeyEvent {\n"
        "            modifiers: guitk::event::Modifiers::NONE,\n"
        "            ..key.clone()\n"
        "        });",
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
# field, each a textline LineBox, and the description's text is drawn at
# last (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
FORM = "the_form_boxes_are_the_toolkits_field"
EDITS = "the_form_boxes_edit_like_a_field"
SEARCH = "the_search_box_is_the_toolkits_field"
FOLDS = "the_search_finds_a_name_whatever_the_case_of_its_letters"
CHECKS = "the_components_are_checkboxes_that_change"
SEARCH_FOCUS = (
    "            focused: self.dialog == DialogKind::None\n"
    "                && self.progress.is_none()\n"
    "                && !self.show_help\n"
    "                && self.view_mode != ViewMode::Schedule,\n"
)

MAIN += [
    (
        "the form box with the keyboard has no mark",
        "            focused: self.form_focus == FormFocus::Text(field) && !self.show_help,\n",
        "            focused: false,\n",
        [FORM],
    ),
    (
        "the form keeps its mark under the list of keys",
        "            focused: self.form_focus == FormFocus::Text(field) && !self.show_help,\n",
        "            focused: self.form_focus == FormFocus::Text(field),\n",
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
        "                self.step_form_focus(key.modifiers.shift);\n",
        "                let _ = key.modifiers.shift;\n",
        [FORM, EDITS, CHECKS],
    ),
    (
        "the form types only at its end",
        "        let edit = self.form_box_mut(field).key(key);\n",
        "        if textline::types_into_field(key) {\n"
        "            let typed = format!(\"{}{}\", self.form_box(field).text(), key.text);\n"
        "            self.form_box_mut(field).set_text(&typed);\n"
        "            return EventResult::Consumed;\n"
        "        }\n"
        "        let edit = self.form_box_mut(field).key(key);\n",
        [EDITS],
    ),
    (
        "a box holds any length",
        'form_name: LineBox::new(FORM_CAPACITY, FONT_SIZE),',
        'form_name: LineBox::new(usize::MAX, FONT_SIZE),',
        [EDITS],
    ),
    (
        "a key the box does not answer is taken",
        "        if !edit.handled {\n            return EventResult::Ignored;\n        }\n",
        "",
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
        "        self.form_box_mut(field).press(dx, width, had_the_keyboard);\n",
        "        let _ = (dx, width, had_the_keyboard);\n",
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
        "            focused: self.progress.is_none()\n"
        "                && !self.show_help\n"
        "                && self.view_mode != ViewMode::Schedule,\n",
        [SEARCH],
    ),
    (
        "the search box keeps its mark under the work",
        SEARCH_FOCUS,
        "            focused: self.dialog == DialogKind::None\n"
        "                && !self.show_help\n"
        "                && self.view_mode != ViewMode::Schedule,\n",
        [SEARCH],
    ),
    (
        "the search box keeps its mark under the list of keys",
        SEARCH_FOCUS,
        "            focused: self.dialog == DialogKind::None\n"
        "                && self.progress.is_none()\n"
        "                && self.view_mode != ViewMode::Schedule,\n",
        [SEARCH],
    ),
    (
        "a search that finds nothing is not red",
        "            invalid: !self.search.is_empty() && self.visible_rows().is_empty(),\n",
        "            invalid: false,\n",
        [SEARCH],
    ),
    (
        "the search's caret is drawn at its start",
        "                self.search.input().cursor(),\n",
        "                TextCursor::default(),\n",
        [SEARCH],
    ),
    (
        "the search folds only ASCII's letters",
        "        let q = self.search.text().to_lowercase();\n",
        "        let q = self.search.text().to_ascii_lowercase();\n",
        [FOLDS],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FORM, SEARCH],
    ),
]

# The form's components are the toolkit's checkboxes, and the ones this
# system can keep can be changed (2026-10-04): they were drawn as checkboxes
# that nothing changed.
MAIN += [
    (
        "a component cannot be ticked",
        "            *chosen = !*chosen;\n",
        "            let _ = chosen;\n",
        [CHECKS],
    ),
    (
        "a press on a disabled checkbox takes the keyboard",
        "    fn press_component(&mut self, i: usize) {\n"
        "        if !self.keepable(i) {\n"
        "            return;\n"
        "        }\n",
        "    fn press_component(&mut self, i: usize) {\n",
        [CHECKS],
    ),
    (
        "a disabled checkbox is drawn enabled",
        "                    disabled: !keepable,\n",
        "                    disabled: false,\n",
        [CHECKS],
    ),
    (
        "a checkbox with the keyboard has no ring",
        "                    focused: self.form_focus == FormFocus::Component(i) && !self.show_help,\n",
        "                    focused: false,\n",
        [CHECKS],
    ),
    (
        "a checkbox keeps its ring under the list of keys",
        "                    focused: self.form_focus == FormFocus::Component(i) && !self.show_help,\n",
        "                    focused: self.form_focus == FormFocus::Component(i),\n",
        [CHECKS],
    ),
    (
        "Tab passes the checkboxes over",
        "                .filter(|i| self.keepable(*i))\n",
        "                .filter(|_| false)\n",
        [CHECKS],
    ),
    (
        "Tab stops at a disabled checkbox",
        "                .filter(|i| self.keepable(*i))\n",
        "                .filter(|_| true)\n",
        [CHECKS],
    ),
    (
        "Shift+Tab goes forward",
        "        let next = if back {\n",
        "        let next = if false {\n",
        [CHECKS, EDITS],
    ),
    (
        "Tab stops at the last stop",
        "            at.saturating_add(1).checked_rem(stops.len()).unwrap_or(0)\n",
        "            at.saturating_add(1).min(stops.len().saturating_sub(1))\n",
        [CHECKS],
    ),
    (
        "Space does nothing to a checkbox",
        "                self.toggle_component(i);\n"
        "                return EventResult::Consumed;\n",
        "                return EventResult::Consumed;\n",
        [CHECKS],
    ),
    (
        "every key flips a checkbox",
        "                if !checkbox::toggles(key) {\n"
        "                    return EventResult::Ignored;\n"
        "                }\n",
        "",
        [CHECKS],
    ),
    (
        "a press on a checkbox does nothing",
        "                    self.press_component(i);\n",
        "                    let _ = i;\n",
        [CHECKS],
    ),
    (
        "a press on a checkbox's label misses it",
        "                checkbox::hit(*cx, *cy, COMPONENT_ROW_HEIGHT, label).contains(x, y)\n",
        '                checkbox::hit(*cx, *cy, COMPONENT_ROW_HEIGHT, &label[..0]).contains(x, y)\n',
        [CHECKS],
    ),
    (
        "the form opens on the last one's choice",
        "        self.form_components = (0..SnapshotComponent::all().len())\n"
        "            .map(|i| self.keepable(i))\n"
        "            .collect();\n",
        "",
        [CHECKS],
    ),
    (
        "a point of nothing is not refused for what it is",
        "        if chosen.is_empty() {\n"
        '            return self.refuse(title, "Choose at least one component to keep");\n'
        "        }\n",
        "",
        [CHECKS],
    ),
    (
        "the form says it keeps something when nothing is chosen",
        "        let holds = if self.form_selected_components().is_empty() {\n",
        "        let holds = if false {\n",
        [CHECKS],
    ),
]


# Why a component checkbox is switched off, said while the pointer rests
# on it (2026-10-10; requests/c-e-say-why-a-control-is-disabled.md).
MAIN += [
    (
        'a switched-off checkbox gives no reason',
        '                let why = component.source(&self.locations).err()?;',
        '                let why = component.source(&self.locations).err().filter(|_| false)?;',
        ['a_switched_off_component_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        "a checkbox's reason is not a sentence",
        '                Some(((r.x, r.y, r.w, r.h), format!("{why}.")))',
        '                Some(((r.x, r.y, r.w, r.h), why.to_string()))',
        ['a_switched_off_component_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        "a checkbox's reason is given under the shortcut card",
        '        if self.show_help || self.field_menu.is_up() {\n',
        '        if self.field_menu.is_up() {\n',
        ['a_switched_off_component_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        "the pointer is not followed for a checkbox's reason",
        '                MouseEventKind::Leave => None,\n                _ => Some((mouse.x, mouse.y)),',
        '                MouseEventKind::Leave => None,\n                _ => self.pointer,',
        ['a_switched_off_component_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        "a pointer that left keeps a checkbox's reason",
        '                MouseEventKind::Leave => None,\n                _ => Some((mouse.x, mouse.y)),',
        '                MouseEventKind::Leave => self.pointer,\n                _ => Some((mouse.x, mouse.y)),',
        ['a_switched_off_component_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        "the clock for a checkbox's reason does not move",
        '            self.clock_ms = self.clock_ms.saturating_add(*elapsed_ms);',
        '            let _ = elapsed_ms;',
        ['a_switched_off_component_says_why_while_the_pointer_rests_on_it'],
    ),
    # Not a row: the tick that shows a checkbox's reason asks for a frame of
    # its own, which matters on the machine -- the window's own tick draws
    # only when the wall clock's second has changed -- but no test can see
    # it, because handle_tick reads the real clock and every test tick
    # moves the window's time and asks for a frame anyway.
    (
        "a checkbox's reason going asks for no frame",
        '        if self.explain_disabled() {\n            result = EventResult::Consumed;\n        }',
        '        self.explain_disabled();',
        ['a_switched_off_component_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        "a checkbox's reason is not drawn",
        '        rt.commands.extend(self.why_disabled.render(&self.palette));',
        '        let _ = self.why_disabled.render(&self.palette);',
        ['a_switched_off_component_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        "the window does not wake for a checkbox's reason",
        "        Some(soonest.map_or(own, |ms| own.min(Duration::from_millis(ms))))\n",
        "        Some(soonest.map_or(own, |_| own))\n",
        ['a_switched_off_component_says_why_while_the_pointer_rests_on_it'],
    ),
]

# The search box edits at a caret, the form's boxes share the program's
# clipboard, and both have right-click menus (2026-10-16).
S_CARET = "the_search_box_edits_at_a_caret"
S_PRESS = "a_press_in_the_search_box_puts_the_caret_there"
S_SYNC = "a_search_emptied_by_escape_is_empty_in_the_box"
S_MENU = "the_search_boxs_right_click_menu_edits_the_search"
F_MENU = "the_forms_right_click_menu_edits_its_boxes"

MAIN += [
    (
        "a key typed into the search is reported unanswered",
        "            self.reanchor_selection();\n        }\n        if edit.handled {\n",
        "            self.reanchor_selection();\n        }\n        if false {\n",
        [S_CARET],
    ),
    (
        "the search's clipboard chords are the window's",
        "                Key::A | Key::C | Key::X | Key::V => self.search_key(key),\n",
        "",
        [S_CARET],
    ),
    (
        "the arrows do not move the search's caret",
        "            Key::Left | Key::Right | Key::Backspace => self.search_key(key),\n",
        "            Key::Backspace => self.search_key(key),\n",
        [S_CARET],
    ),
    (
        "the list does not follow the search",
        "        if edit.changed {\n            self.reanchor_selection();\n        }\n",
        "",
        ["typing_filters_the_list_and_the_selection_stays_on_something_visible"],
    ),
    (
        "a press in the search box puts the caret nowhere",
        "            }\n            self.press_search_box(x);\n",
        "            }\n",
        [S_PRESS],
    ),
    (
        "a right press opens no menu",
        "            MouseEventKind::Press(MouseButton::Right) => self.open_field_menu(mouse.x, mouse.y),\n",
        "",
        [S_MENU, F_MENU],
    ),
    (
        "the keys reach a box through its menu",
        "            fieldmenu::Outcome::Taken => EventResult::Consumed,\n",
        "            fieldmenu::Outcome::Taken => self.dispatch(event),\n",
        [S_MENU],
    ),
    (
        "a row chosen does nothing to the search",
        "                if self.search.menu_row(id) {\n",
        "                if false {\n",
        [S_MENU],
    ),
    (
        "a row chosen does nothing to a form box",
        "                self.form_box_mut(form_field).menu_row(id);\n",
        "                let _ = (form_field, id);\n",
        [F_MENU],
    ),
    (
        "the form's right-click leaves the keyboard where it was",
        "            if self.form_focus != FormFocus::Text(field) {\n                self.focus_form_field(field);\n            }\n",
        "",
        [F_MENU],
    ),
    (
        "the menu is not drawn",
        "        rt.commands.extend(self.field_menu.render(&self.palette));\n",
        "",
        [S_MENU],
    ),
]

TABLES = {
    "points.rs": POINTS,
    "main.rs": MAIN,
}

# The review of 007d0552c..752d46d80: a switched-off checkbox's reason under
# a box's menu, and the search box in the Schedule view.
WHY = "a_switched_off_component_says_why_while_the_pointer_rests_on_it"
SCHEDULE_BOX = "the_search_box_has_not_the_keyboard_in_the_schedule_view"

# Extends MAIN in place, the list TABLES already holds for main.rs.
MAIN += [
    (
        "a switched-off reason is drawn over a box's menu",
        "        if self.show_help || self.field_menu.is_up() {\n",
        "        if self.show_help {\n",
        [WHY],
    ),
    (
        "the search box takes keys in the Schedule view",
        "        if self.view_mode == ViewMode::Schedule {\n            return EventResult::Ignored;\n        }\n        let edit = self.search.key(key);\n",
        "        let edit = self.search.key(key);\n",
        [SCHEDULE_BOX],
    ),
    (
        "the search box is marked with the keyboard in the Schedule view",
        "                && self.view_mode != ViewMode::Schedule,\n",
        "                && true,\n",
        [SCHEDULE_BOX],
    ),
    (
        "a press on the search stays in the Schedule view",
        "                self.view_mode = ViewMode::Tree;\n",
        "",
        [SCHEDULE_BOX],
    ),
]

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

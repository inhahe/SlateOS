"""Mutation test for the process explorer's New Task box and Open file location.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

This table covers the two controls that were stubs until 2026-09-26: the
toolbar's New Task button ("New Task dialog not yet implemented") and the
context menu's Open file location ("(NYI)").  New Task puts up a box that
takes a command line, splits it as a shell would -- quotes and escapes, no
expansion -- and starts it; Open file location follows `/proc/<pid>/exe` and
hands the program's path to the file manager.

And, from 2026-09-27, the machine's totals: `read_system`'s doc said a figure
`/proc/meminfo` does not carry keeps the value last read, and the code zeroed
it.  And, from 2026-10-04, the list of keys' hold on the pointer: a press with
it up puts it away and reaches nothing under it, and the wheel scrolls nothing
it covers.

What is deliberately not here: where the box and its buttons are drawn.  The
pointer tests click the middle of the rectangles `run_box_layout` returns, so
moving a rectangle moves the click with it; only swapping or overlapping the
two buttons is observable, and that is the Run/Cancel rows below.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

SPLIT = "a_command_line_splits_as_a_shell_splits_it"
CTRL_N = "ctrl_n_runs_a_task_and_names_it"
POINTER = "the_new_task_button_and_the_box_answer_the_pointer"
WHY = "a_task_that_will_not_start_says_why"
KEYS = "the_run_box_takes_the_keys_while_it_is_up"
MODAL = "the_run_box_is_modal_to_the_pointer"
PASTE = "a_line_copied_in_the_run_box_pastes_back"
UNANSWERED = "a_key_the_box_does_not_answer_keeps_the_complaint"
PLAIN_N = "a_plain_n_does_not_put_the_run_box_up"
OPEN_QUOTE = "a_line_with_an_open_quote_is_not_run"
LOCATION = "open_file_location_opens_the_programs_folder"
MENU = "the_context_menus_open_file_location_looks_for_the_program"
HAND_OFF = "show_in_folder_hands_the_path_to_the_file_manager"
READ_SYSTEM = "the_system_figures_are_read_and_not_invented"
KEEP = "a_figure_the_file_stops_carrying_keeps_its_last_value"
CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"

MUTATIONS = [
    # --- The list of keys takes the pointer (known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it) ---
    (
        "a press goes through the list of keys",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n"
        "                    return EventResult::Consumed;\n"
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
        "                    return EventResult::Consumed;\n",
        "                    return EventResult::Consumed;\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the list of keys covers",
        "                MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "",
        [CARD],
    ),
    # --- Splitting a command line ---
    (
        "white space does not end a word",
        "            c if c.is_whitespace() => {\n                if in_word {",
        "            c if c.is_whitespace() => {\n                if false {",
        [SPLIT, CTRL_N],
    ),
    (
        "white space is part of the word",
        "                    words.push(std::mem::take(&mut word));\n"
        "                    in_word = false;",
        "                    word.push(c);",
        [SPLIT, CTRL_N],
    ),
    (
        "a single quote never closes",
        "                        Some('\\'') => break,",
        "                        Some('\\'') => {}",
        [SPLIT],
    ),
    (
        "an empty quoted word is no word",
        "            '\"' => {\n                in_word = true;",
        "            '\"' => {\n                in_word |= false;",
        [SPLIT],
    ),
    (
        "an escaped quote keeps its backslash",
        "                            Some(c @ ('\"' | '\\\\')) => word.push(c),",
        "                            Some(c @ ('\"' | '\\\\')) => {\n"
        "                                word.push('\\\\');\n"
        "                                word.push(c);\n"
        "                            }",
        [SPLIT],
    ),
    (
        "a backslash outside quotes is kept",
        "                match chars.next() {\n"
        "                    Some(c) => word.push(c),\n"
        "                    None => return Err(String::from(\"the line ends in a \\\\\")),",
        "                match chars.next() {\n"
        "                    Some(c) => {\n"
        "                        word.push('\\\\');\n"
        "                        word.push(c);\n"
        "                    }\n"
        "                    None => return Err(String::from(\"the line ends in a \\\\\")),",
        [SPLIT],
    ),
    (
        "a trailing backslash is dropped without a word",
        "                    None => return Err(String::from(\"the line ends in a \\\\\")),",
        "                    None => {}",
        [SPLIT],
    ),
    (
        "an open double quote is closed by the end of the line",
        "                        Some(c) => word.push(c),\n"
        "                        None => return Err(String::from(\"a \\\" is not closed\")),",
        "                        Some(c) => word.push(c),\n"
        "                        None => break,",
        [SPLIT, OPEN_QUOTE],
    ),
    (
        "the last word is lost",
        "    if in_word {\n        words.push(word);",
        "    if false {\n        words.push(word);",
        [SPLIT, CTRL_N],
    ),
    # --- The box and the keys ---
    (
        "N alone is New Task",
        "        if key.key == Key::N && ctrl {",
        "        if key.key == Key::N {",
        [PLAIN_N],
    ),
    (
        "the box does not take the keys",
        "        if self.run_box.is_some() {\n            return self.run_box_key(key);",
        "        if false {\n            return self.run_box_key(key);",
        [KEYS, CTRL_N],
    ),
    (
        "the box does not take the pointer",
        "        if self.run_box.is_some() {\n            return match mouse.kind {",
        "        if false {\n            return match mouse.kind {",
        [MODAL],
    ),
    (
        "the toolbar's New Task does nothing",
        "                ToolbarAction::NewTask => self.open_run_box(),\n",
        "                ToolbarAction::NewTask => self.status_message.clear(),\n",
        [POINTER],
    ),
    (
        "Escape leaves the box up",
        "            Key::Escape => self.run_box = None,",
        "            Key::Escape => {}",
        [KEYS],
    ),
    (
        "Enter does not run",
        "            Key::Enter => self.run_task(),",
        "            Key::Enter => {}",
        [CTRL_N, WHY],
    ),
    (
        "every key clears the complaint",
        "                    if edit.handled {\n                        run_box.error = None;\n                    }",
        "                    run_box.error = None;",
        [UNANSWERED],
    ),
    (
        "an edit leaves the complaint up",
        "                    if edit.handled {\n                        run_box.error = None;",
        "                    if false {\n                        run_box.error = None;",
        [WHY],
    ),
    # --- Running ---
    (
        "a line that does not split is run as plain words",
        "            Err(why) => {\n                run_box.error = Some(why);\n                return;\n            }",
        "            Err(_) => run_box\n"
        "                .input\n"
        "                .text()\n"
        "                .split_whitespace()\n"
        "                .map(str::to_owned)\n"
        "                .collect(),",
        [OPEN_QUOTE],
    ),
    (
        "an empty line says nothing",
        "            run_box.error = Some(String::from(\"Type the program to run\"));",
        "            run_box.error = None;",
        [WHY, UNANSWERED],
    ),
    (
        "the first argument is dropped",
        "        let args: Vec<OsString> = args.iter().map(OsString::from).collect();",
        "        let args: Vec<OsString> = args.iter().skip(1).map(OsString::from).collect();",
        [CTRL_N],
    ),
    (
        "a start leaves the box up",
        "            Ok(pid) => {\n                self.run_box = None;",
        "            Ok(pid) => {\n                let _ = &self.run_box;",
        [CTRL_N],
    ),
    (
        "the refresh's summary is left on the status line",
        "                self.refresh();\n"
        "                self.status_message = format!(\"Started {program} (PID {pid})\");",
        "                self.status_message = format!(\"Started {program} (PID {pid})\");\n"
        "                self.refresh();",
        [CTRL_N],
    ),
    (
        "a start that failed says nothing",
        "            Err(e) => run_box.error = Some(format!(\"Cannot run {program}: {e}\")),",
        "            Err(_) => run_box.error = None,",
        [WHY],
    ),
    (
        "a start that failed closes the box",
        "            Err(e) => run_box.error = Some(format!(\"Cannot run {program}: {e}\")),",
        "            Err(_) => self.run_box = None,",
        [WHY],
    ),
    # --- The box's buttons ---
    (
        "Run does not run",
        "        if inside(l.run) {\n            self.run_task();",
        "        if inside(l.run) {\n            self.run_box = None;",
        [POINTER],
    ),
    (
        "Cancel does nothing",
        "        } else if inside(l.cancel) {\n            self.run_box = None;",
        "        } else if inside(l.cancel) {\n            let _ = &self.run_box;",
        [POINTER, MODAL],
    ),
    # --- Open file location ---
    (
        "the wrong link is read",
        "        let link = fs.root().join(pid.to_string()).join(\"exe\");",
        "        let link = fs.root().join(pid.to_string()).join(\"cwd\");",
        [LOCATION],
    ),
    (
        "a program that cannot be found says nothing",
        "            Err(e) => self.status_message = format!(\"Cannot find PID {pid}'s program: {e}\"),",
        "            Err(_) => {}",
        [LOCATION, MENU],
    ),
    (
        "the context menu's Open file location does nothing",
        "                self.open_file_location(&procinfo::ProcFs::new(), target_pid);",
        "                let _ = target_pid;",
        [MENU],
    ),
    (
        "the folder is handed over, not the program",
        "        self.status_message = match (self.spawner)(OsStr::new(FILE_MANAGER), &[file.into()]) {",
        "        let folder = file.parent().map(PathBuf::from).unwrap_or_default();\n"
        "        self.status_message = match (self.spawner)(OsStr::new(FILE_MANAGER), &[folder.into()]) {",
        [HAND_OFF, LOCATION],
    ),
    (
        "a file manager that will not start is reported as opened",
        "            Err(e) => format!(\"Cannot open the file manager: {e}\"),",
        "            Err(_) => format!(\"Opened the folder of {shown}\"),",
        [HAND_OFF],
    ),
    # --- The machine's totals (2026-09-27) ---
    (
        "a figure the file does not carry is zeroed",
        "    if let Some(kib) = kib {\n        *field = kib.saturating_mul(1024);\n    }",
        "    *field = kib.unwrap_or(0).saturating_mul(1024);",
        [KEEP],
    ),
    (
        "a figure in KiB is taken for bytes",
        "        *field = kib.saturating_mul(1024);",
        "        *field = kib;",
        [READ_SYSTEM, KEEP],
    ),
    (
        "the cache is read from the free figure",
        "            set_kib(&mut info.cached_memory, mem.cached_kib);",
        "            set_kib(&mut info.cached_memory, mem.free_kib);",
        [READ_SYSTEM, KEEP],
    ),
    (
        "swap in use is not read",
        "            set_kib(&mut info.swap_used, mem.swap_used_kib());\n",
        "",
        [READ_SYSTEM, KEEP],
    ),
]

ASK = "the_identify_button_and_ctrl_i_ask_for_a_window_pick"
NAMED = "a_picked_window_names_and_selects_its_program"
BUTTONS = "every_toolbar_button_is_pressed_where_it_is_drawn"

MUTATIONS += [
    # The window pick, through lane F's `App::take_pick` (2026-10-04), and
    # the toolbar pressed where it is drawn.
    (
        "the Identify button asks for nothing",
        "                ToolbarAction::IdentifyWindow => self.toggle_window_pick(),\n",
        "                ToolbarAction::IdentifyWindow => {}\n",
        [ASK],
    ),
    (
        "pressing Identify again does not give the pick up",
        "        if self.window_picker.active {\n            self.cancel_window_pick();\n        } else {\n",
        "        if false {\n            self.cancel_window_pick();\n        } else {\n",
        [ASK],
    ),
    (
        "one press asks for a pick at every turn",
        "        self.pick_request.take()\n",
        "        self.pick_request\n",
        [ASK],
    ),
    (
        "Escape does not give the pick up",
        "        if key.key == Key::Escape && plain && self.window_picker.active {\n            self.cancel_window_pick();\n            return EventResult::Consumed;\n        }\n",
        "",
        [ASK],
    ),
    (
        "AltGr+I picks",
        "        if key.key == Key::I && ctrl {\n",
        "        if key.key == Key::I && key.modifiers.ctrl && !key.modifiers.super_key {\n",
        [ASK],
    ),
    (
        "nothing says the next click is a pick",
        "        if self.window_picker.active {\n            tree.commands.extend(self.window_picker.render(\n",
        "        if false {\n            tree.commands.extend(self.window_picker.render(\n",
        [ASK],
    ),
    (
        "the picked window's process is not selected",
        "                        self.select_row(row);\n",
        "",
        [NAMED],
    ),
    (
        "the list the process is selected in is not shown",
        "                            self.set_tab(Tab::Processes);\n",
        "",
        [NAMED],
    ),
    (
        "a pick that came to nothing leaves the banner up",
        "        // Whatever the answer, the pick is over.\n        self.window_picker.cancel();\n",
        "",
        [NAMED],
    ),
    (
        "a press beside a button presses it",
        "        .find(|(r, _)| r.contains(x, y))\n",
        "        .find(|(r, _)| x < r.right() + 6.0 && y >= r.y && y < r.bottom())\n",
        [BUTTONS],
    ),
]

# The filter box is the toolkit's field, edited by textline's editor; the
# keys bound to themselves are plain, and a Ctrl chord is not AltGr
# (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
FILTER = "the_filter_box_is_the_toolkits_field"
EDITS = "the_filter_box_edits_like_a_field_and_types_no_chord"
FILTER_PRESS = "a_press_in_the_filter_puts_the_caret_under_the_pointer"
PLAIN = "a_chord_is_none_of_the_process_explorers_keys"
CARET = "the_filter_caret_follows_the_glyphs"

MUTATIONS += [
    (
        "a chord raises the list of keys",
        "        if key.key == Key::F1 && plain {\n",
        "        if key.key == Key::F1 {\n",
        [PLAIN],
    ),
    (
        "a chord works the explorer's keys",
        "        if !plain {\n            return EventResult::Ignored;\n        }\n        match key.key {\n",
        "        match key.key {\n",
        [PLAIN],
    ),
    (
        "AltGr+N opens New Task",
        "        if key.key == Key::N && ctrl {",
        "        if key.key == Key::N && key.modifiers.ctrl {",
        [EDITS],
    ),
    (
        "AltGr+F focuses the filter",
        "        if key.key == Key::F && ctrl {\n",
        "        if key.key == Key::F && key.modifiers.ctrl {\n",
        [PLAIN],
    ),
    (
        "the filter types a chord's letter",
        '            _ => {\n                if self.filter.key(key).changed {\n',
        '            _ => {\n                if !key.text.is_empty() && !textline::types_into_field(key) {\n                    let typed = format!("{}{}", self.filter.text(), key.text);\n                    self.filter.set_text(&typed);\n                    self.rebuild_visible_list();\n                    return EventResult::Consumed;\n                }\n                if self.filter.key(key).changed {\n',
        [EDITS],
    ),
    (
        "the filter box is never lit",
        "            hovered: self.filter_hovered && !covered,\n",
        "            hovered: false,\n",
        [FILTER],
    ),
    (
        "the pointer leaving leaves the filter lit",
        "                self.hovered_index = None;\n                self.filter_hovered = false;\n",
        "                self.hovered_index = None;\n",
        [FILTER],
    ),
    (
        "the filter keeps its mark under the list of keys",
        "            focused: self.filter_focused && !covered,\n",
        "            focused: self.filter_focused,\n",
        [FILTER],
    ),
    (
        "the filter keeps its mark under New Task",
        "        let covered = self.show_help || self.run_box.is_some();\n",
        "        let covered = self.show_help;\n",
        [FILTER],
    ),
    (
        "a filter that matches nothing is not red",
        "            invalid: !self.filter.is_empty() && self.visible_indices.is_empty(),\n",
        "            invalid: false,\n",
        [FILTER],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FILTER],
    ),
    (
        "a press puts the caret at the start",
        "            x - rect.x - FILTER_TEXT_INSET,\n",
        "            0.0,\n",
        [FILTER_PRESS],
    ),
    (
        "the filter is drawn with no caret",
        "                focused: state.focused,\n                placeholder: \"Filter (Ctrl+F)\",\n",
        "                focused: false,\n                placeholder: \"Filter (Ctrl+F)\",\n",
        [CARET],
    ),
]

# The boxes' right-click menu (2026-10-10, requests/c-e-text-fields-have-a-
# right-click-menu.md): Cut, Copy, Paste, Delete and Select all over the filter
# box and the New Task box's line, through apps/fieldmenu; the keys and the menu
# share the program's clipboard; each box holds its capacity.
FM_FILTER = "the_filter_boxs_right_click_menu_pastes_and_filters"
FM_RUN = "the_run_boxs_menu_pastes_to_its_capacity_and_puts_the_complaint_away"
FM_MODAL = "a_boxs_menu_is_modal_and_a_process_keeps_its_own"
FM_CLOCK = "a_boxs_menu_is_drawn_and_a_dimmed_row_says_why_on_the_windows_ticks"

MUTATIONS += [
    (
        "a right press on the filter box opens no menu",
        "                if self.open_field_menu(mx, my) == EventResult::Consumed {\n"
        "                    return EventResult::Consumed;\n"
        "                }\n",
        "",
        [FM_FILTER, FM_MODAL],
    ),
    (
        "a right press in the New Task box opens no menu",
        "                MouseEventKind::Press(MouseButton::Right) => {\n"
        "                    let _menu = self.open_field_menu(mx, my);\n"
        "                    EventResult::Consumed\n"
        "                }\n",
        "",
        [FM_RUN],
    ),
    (
        "the right-click leaves the filter without the keyboard",
        "            if !self.filter_focused {\n"
        "                self.focus_filter();\n"
        "            }\n"
        "            (MenuField::Filter, self.filter.edit_menu())\n",
        "            (MenuField::Filter, self.filter.edit_menu())\n",
        [FM_FILTER],
    ),
    (
        "opening a box's menu leaves the process's menu up",
        "        self.context_menu = None;\n        self.field_menu.open(field, menu, x, y, viewport);\n",
        "        self.field_menu.open(field, menu, x, y, viewport);\n",
        [FM_MODAL],
    ),
    (
        'a row chosen for the filter does nothing',
        '                if self.filter.menu_row(id) {\n',
        '                if false {\n',
        [FM_FILTER],
    ),
    (
        "an edit from the menu leaves the list unfiltered",
        "                    self.rebuild_visible_list();\n"
        "                }\n"
        "            }\n"
        "            MenuField::RunBox => {\n",
        "                }\n"
        "            }\n"
        "            MenuField::RunBox => {\n",
        [FM_FILTER],
    ),
    (
        "an edit from the menu leaves the New Task box's complaint up",
        '                    && run_box.input.edit_command(id) == KeyEdit::Changed\n                {\n                    run_box.error = None;\n                }\n',
        '                    && run_box.input.edit_command(id) == KeyEdit::Changed\n                {\n                }\n',
        [FM_RUN],
    ),
    (
        'the New Task box holds no capacity',
        '            input: textline::field_holding("", RUN_BOX_CAPACITY),\n',
        '            input: textline::field_holding("", usize::MAX),\n',
        [FM_RUN],
    ),
    (
        'the filter box holds no capacity',
        '            filter: LineBox::new(FILTER_CAPACITY, FILTER_TEXT_SIZE),\n',
        '            filter: LineBox::new(usize::MAX, FILTER_TEXT_SIZE),\n',
        [FM_FILTER],
    ),
    (
        "the New Task box's keys keep a clipboard the menu does not share",
        "let edit = textline::apply_key_on_clipboard(\n"
        "                        &mut run_box.input,\n"
        "                        key,\n"
        "                        RUN_BOX_TEXT_SIZE,\n"
        "                    );\n",
        "let edit = textline::apply_key(\n"
        "                        &mut run_box.input,\n"
        "                        key,\n"
        '                        "",\n'
        "                        RUN_BOX_TEXT_SIZE,\n"
        "                    );\n",
        [FM_FILTER],
    ),
    (
        'a key the menu took reaches the box under it',
        '            fieldmenu::Outcome::Taken => return EventResult::Consumed,\n',
        '            fieldmenu::Outcome::Taken => {}\n',
        [FM_MODAL],
    ),
    (
        "a right press on a process under a box's menu opens nothing",
        '                    return self.handle_mouse(mouse);\n',
        '                    return EventResult::Consumed;\n',
        [FM_MODAL],
    ),
    (
        "a box's menu is never drawn",
        "        tree.commands.extend(self.field_menu.render(&self.palette));\n",
        "",
        [FM_CLOCK],
    ),
    (
        "the window's ticks do not move the menu's clock",
        '                let _menu_changed = self.field_menu.tick(*elapsed_ms);\n',
        '',
        [FM_CLOCK],
    ),
    (
        "the window asks no tick for the menu's wait",
        "        Some(menu.map_or(refresh, |menu| menu.min(refresh)))\n",
        "        Some(menu.map_or(refresh, |_menu| refresh))\n",
        [FM_CLOCK],
    ),
]

# Three ways to end a program -- close, end without asking, force -- through
# apps/processctl, as lane B's request asks (2026-10-16).
ASKED = "a_program_is_closed_ended_or_forced_as_asked"
CANNOT = "a_program_that_cannot_be_asked_says_how_to_end_it"

MUTATIONS += [
    (
        "Delete forces rather than asks",
        "                self.end_selected(Ending::Close);\n                EventResult::Consumed\n            }\n            Key::Delete if key.modifiers == Modifiers::shift() => {\n",
        "                self.end_selected(Ending::Force);\n                EventResult::Consumed\n            }\n            Key::Delete if key.modifiers == Modifiers::shift() => {\n",
        [ASKED],
    ),
    (
        "Shift+Delete ends nothing",
        "            Key::Delete if key.modifiers == Modifiers::shift() => {\n                self.end_selected(Ending::End);\n                EventResult::Consumed\n            }\n",
        "",
        [ASKED],
    ),
    (
        "every ending in the menu forces",
        "            ContextAction::End(ending) => self.end_pid(target_pid, ending),\n",
        "            ContextAction::End(_) => self.end_pid(target_pid, Ending::Force),\n",
        [ASKED],
    ),
    (
        "a process is ended through the real system, whatever was given",
        "        self.status_message = processctl::end_named(self.stopper, &name, pid, ending);\n",
        "        self.status_message = processctl::end_named(&SYSTEM, &name, pid, ending);\n",
        [ASKED, CANNOT],
    ),
    (
        "an ending says nothing of what was done",
        "        self.status_message = processctl::end_named(self.stopper, &name, pid, ending);\n",
        "        let _ = processctl::end_named(self.stopper, &name, pid, ending);\n",
        [CANNOT],
    ),
    (
        "a signal goes through the real system, whatever was given",
        "        self.status_message = processctl::signal_named(self.stopper, &name, pid, sig, verb);\n",
        "        self.status_message = processctl::signal_named(&SYSTEM, &name, pid, sig, verb);\n",
        [ASKED],
    ),
    (
        "End Task forces",
        "                ToolbarAction::EndProcess => self.end_selected(Ending::Close),\n",
        "                ToolbarAction::EndProcess => self.end_selected(Ending::Force),\n",
        [ASKED],
    ),
    (
        "the menu's pause resumes",
        "            ContextAction::Pause => self.signal_pid(target_pid, processctl::SIGSTOP, \"pause\"),\n",
        "            ContextAction::Pause => self.signal_pid(target_pid, processctl::SIGCONT, \"pause\"),\n",
        [ASKED],
    ),
]

# The process menu kept on screen (the Opus review of 007d0552c..752d46d80:
# eight rows opened at the press ran off the bottom).
ON_SCREEN = "the_process_menu_stays_on_screen"

MUTATIONS += [
    (
        "the process menu runs off the bottom",
        "            y: y.min(window.1 - h).max(0.0),\n",
        "            y,\n",
        [ON_SCREEN],
    ),
    (
        "the process menu runs off the right",
        "            x: x.min(window.0 - CONTEXT_MENU_W).max(0.0),\n",
        "            x,\n",
        [ON_SCREEN],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "procexplorer", timeout=600, only=only))

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
it.

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

MUTATIONS = [
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
        "        if key.key == Key::N && key.modifiers.ctrl {",
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
        "        } else if mx < 170.0 {\n            self.open_run_box();",
        "        } else if mx < 170.0 {\n            self.status_message.clear();",
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
        "a copy is not kept",
        "                    if let Some(copied) = edit.copied {\n"
        "                        self.clipboard = copied;\n"
        "                    }",
        "                    let _ = &edit.copied;",
        [PASTE],
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

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "procexplorer", timeout=600, only=only))

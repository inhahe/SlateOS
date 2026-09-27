"""Mutation test for the file manager's thumbnail worker.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26: thumbnails were made a few per
tick on the thread that draws, and are made on `offloop::Queue`'s worker now,
filed as each arrives.  `offloop`'s own rules are swept by
`apps/offloop/mutate.py`.

And what changed on 2026-09-27 (`FILEOPS`, `COLUMNPREFS` and the prompt rows
in `MAIN`): the copy engine waits at a taken name instead of skipping it and
the window asks; a file is never replaced by itself, nor a folder put inside
itself; a link is copied, moved and deleted as the link and never followed;
a tree too deep to be real is refused.

Rows the Windows host cannot decide are left out on purpose: it cannot make a
symbolic link without a privilege, so a *copy* of a link always fails there,
and "copied as a link" cannot be told from "failed" -- `copy_link` and the
move's `remove_link` are covered by tests that run on a unix host.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

OFF = "thumbnails_are_made_off_the_window"
ASKS = "a_paste_onto_a_taken_name_asks_and_waits"
ANSWERS = "each_answer_to_the_prompt_does_what_it_says"
REST_WINDOW = "the_same_for_the_rest_is_asked_once"
CLICK = "the_prompt_answers_a_click_on_its_buttons"
DEFAULT_ASK = "ask_is_offered_first_and_is_what_a_paste_does_until_told_otherwise"
REMEMBERED = "a_taken_name_is_asked_about_until_the_user_chooses_otherwise_and_the_choice_is_remembered"
SELF_PASTE = "a_cut_pasted_back_into_its_own_folder_keeps_the_file_even_under_replace"
WAITS = "ask_stops_at_the_taken_name_and_waits"
EACH = "each_answer_does_what_it_says_to_the_file_it_was_asked_about"
STOP = "stop_ends_the_operation_where_it_is"
REST = "the_same_for_the_rest_asks_no_more"
ONE_FILE = "an_answer_for_one_file_is_not_an_answer_for_the_next"
CANCEL_WAITING = "cancelling_an_operation_that_is_asking_ends_it"
SELF_MOVE = "a_file_moved_into_its_own_folder_stays_whatever_the_policy"
SELF_DIR = "a_folder_copied_into_its_own_folder_is_duplicated_whole"
INSIDE = "a_folder_cannot_go_inside_itself"
SELF_LINK = "a_link_made_in_its_own_folder_never_replaces_the_file"
BY_HAND = "the_executor_never_replaces_a_file_with_itself_whatever_the_plan_says"
SAME_ENTRY = "the_same_entry_is_the_same_name_in_the_same_folder"
DELETE_LINK = "deleting_a_folder_never_deletes_what_a_link_inside_it_reaches"
MOVE_LINK = "moving_a_folder_never_takes_what_a_link_inside_it_reaches"
CYCLE = "a_link_to_a_folder_above_does_not_make_the_scan_endless"
NOT_A_LINK = "a_link_that_is_no_longer_one_is_not_removed"
NO_FOLDER = "a_folder_in_the_way_is_never_removed_to_make_room"
TOO_DEEP = "a_tree_deeper_than_anyone_makes_is_refused_not_walked"
EXIF = "a_photographs_exif_is_three_columns"
LATE = "exif_past_the_head_of_a_webp_is_found"

MAIN = [
    (
        "no waker is asked for",
        "    fn wants_waker(&self) -> bool {\n        true",
        "    fn wants_waker(&self) -> bool {\n        false",
        [OFF],
    ),
    (
        "no worker is started",
        "        self.thumb_worker = offloop::Queue::start(",
        "        self.thumb_worker = None;\n        let _unused = offloop::Queue::start(",
        [OFF],
    ),
    (
        "thumbnails are left to the ticks even with a worker",
        "            Some(worker) => match worker.replace(wanted) {",
        "            Some(_) => match Err::<(), _>(wanted) {",
        [OFF],
    ),
    (
        "a made thumbnail is not filed",
        "        for (req, thumb) in made {\n            self.file_thumbnail(req, thumb);\n        }\n        count",
        "        let _ = made;\n        count",
        [OFF],
    ),
    (
        "a wake with thumbnails asks for no frame",
        "        if self.collect_thumbnails() > 0 {\n            oswindow::app::Response::Redraw",
        "        if self.collect_thumbnails() > 0 {\n            oswindow::app::Response::Idle",
        [OFF],
    ),
    # -- 2026-09-27: asking about a taken name --------------------------------
    (
        "the prompt never opens",
        "            self.modal = Some(Modal::Conflict { prompt });",
        "            let _unused = prompt;",
        [ASKS],
    ),
    (
        "an operation waiting on an answer wants the clock",
        "            .any(|op| op.executor.waiting_on().is_none())",
        "            .any(|_| true)",
        [ASKS],
    ),
    (
        "the answer never reaches the operation",
        "            running.executor.answer(answer, for_the_rest);",
        "            let _unused = (&running, answer, for_the_rest);",
        [ANSWERS],
    ),
    (
        "Enter replaces",
        "                    return (true, Some(ConflictAnswer::KeepBoth));",
        "                    return (true, Some(ConflictAnswer::Replace));",
        [ANSWERS],
    ),
    (
        "the same for the rest is dropped on the way",
        "        let (plan, for_the_rest) = (prompt.plan, prompt.for_the_rest);",
        "        let (plan, for_the_rest) = (prompt.plan, false);",
        [REST_WINDOW],
    ),
    (
        "A does not flip the same for the rest",
        "                if key.key == Key::A {\n                    self.for_the_rest = !self.for_the_rest;",
        "                if false {\n                    self.for_the_rest = !self.for_the_rest;",
        [REST_WINDOW],
    ),
    (
        "a click on an answer is not an answer",
        "                    Some((PromptControl::Answer(answer), _)) => (true, Some(*answer)),",
        "                    Some((PromptControl::Answer(_), _)) => (true, None),",
        [CLICK],
    ),
    (
        "a cut pasted back where it came from is spent",
        "        if plan.actions.is_empty() && operation == FileOperation::Move {",
        "        if false && operation == FileOperation::Move {",
        [SELF_PASTE],
    ),
]

COLUMNS = [
    (
        "the camera column reads nothing",
        "            ColumnId::CAMERA => text(exif().and_then(|e| e.camera())),",
        "            ColumnId::CAMERA => ColumnValue::Empty,",
        [EXIF],
    ),
    (
        "the date column keeps the file's colons",
        "            ColumnId::DATE_TAKEN => text(exif().and_then(|e| e.date_taken).map(|d| exif_date(&d))),",
        "            ColumnId::DATE_TAKEN => text(exif().and_then(|e| e.date_taken)),",
        [EXIF],
    ),
    (
        "a turn right is called a turn left",
        "        6 => \"Turned right\",",
        "        6 => \"Turned left\",",
        [EXIF],
    ),
    (
        "EXIF past the head of the file is never looked for",
        "    if !found.is_empty() || !anywhere || head.len() < IMAGE_HEAD_BYTES {",
        "    if true {",
        [LATE],
    ),
    # The Orientation column says the turn `imagecodec` applies -- the
    # thumbnail's -- not the one the EXIF asks for.
    (
        "a picture imagecodec does not turn is said to be turned",
        "        } else {\n            Orientation::TopLeft\n        }\n    };",
        "        } else {\n            Orientation::RightTop\n        }\n    };",
        [LATE],
    ),
    (
        "a JPEG's turn is not read",
        "            imagecodec::jpeg::orientation(bytes)",
        "            Orientation::TopLeft",
        [EXIF],
    ),
    (
        "a TIFF is read as a JPEG",
        "            imagecodec::tiff::orientation(bytes)",
        "            imagecodec::jpeg::orientation(bytes)",
        [EXIF],
    ),
]

COLUMNPREFS = [
    (
        "keep both is still the default",
        "        .unwrap_or(ConflictPolicy::Ask)",
        "        .unwrap_or(ConflictPolicy::Rename)",
        [DEFAULT_ASK, REMEMBERED],
    ),
    (
        "Ask is not offered",
        '    (ConflictPolicy::Ask, "ask", "Ask each time"),\n',
        # The array's length is its type, so the row is replaced, not removed.
        '    (ConflictPolicy::Rename, "ask", "Ask each time"),\n',
        [DEFAULT_ASK],
    ),
]

FILEOPS = [
    # -- the engine waits at a taken name -------------------------------------
    (
        "asking leaves no question",
        "        self.question = Some(ConflictQuestion {",
        "        let _unused = Some(ConflictQuestion {",
        [WAITS],
    ),
    (
        "a waiting operation is stepped anyway",
        "    pub fn step(&mut self) {\n        if self.question.is_some() {",
        "    pub fn step(&mut self) {\n        if false {",
        [WAITS],
    ),
    (
        "a waiting action is passed over",
        "            Ok(ActionOutcome::Waiting) => {\n                self.next = self.next.saturating_sub(1);",
        "            Ok(ActionOutcome::Waiting) => {\n                let _unused = self.next;",
        [EACH],
    ),
    (
        "one file's answer is every file's",
        "            Some((index, policy)) if index == action.index => policy,",
        "            Some((_, policy)) => policy,",
        [ONE_FILE],
    ),
    (
        "the same for the rest is forgotten",
        "                if for_the_rest {\n                    self.policy_for_the_rest = Some(policy);",
        "                if false {\n                    self.policy_for_the_rest = Some(policy);",
        [REST],
    ),
    (
        "Stop does not stop",
        "            None => self.cancel(),",
        "            None => {}",
        [STOP],
    ),
    (
        "a cancel leaves the question up",
        "    pub fn cancel(&mut self) {\n        self.question = None;",
        "    pub fn cancel(&mut self) {\n        let _unused = &self.question;",
        [CANCEL_WAITING],
    ),
    # -- a file is never its own destination ----------------------------------
    (
        "a move onto itself is planned",
        "                if operation == FileOperation::Move {\n                    continue;\n                }",
        "                if false {\n                    continue;\n                }",
        [SELF_MOVE],
    ),
    (
        "a copy onto itself is not numbered",
        "                    continue;\n                }\n                dest = resolve_rename(&dest);",
        "                    continue;\n                }",
        [SELF_DIR],
    ),
    (
        "the executor replaces a file with itself",
        "            if same_entry(&action.src, dest) {\n                if self.plan.operation == FileOperation::Move {",
        "            if false {\n                if self.plan.operation == FileOperation::Move {",
        [BY_HAND],
    ),
    (
        "a folder may go inside itself",
        "            if is_real_dir(src) && inside(dest_dir, src) {",
        "            if false && inside(dest_dir, src) {",
        [INSIDE],
    ),
    (
        "inside is a prefix of the text",
        "        (Ok(p), Ok(d)) => p.starts_with(&d),",
        "        (Ok(p), Ok(d)) => p.to_string_lossy().starts_with(&*d.to_string_lossy()),",
        [INSIDE],
    ),
    (
        "a link in its own folder replaces the file",
        "            if same_entry(src, &dest) {\n                dest = resolve_rename(&dest);\n            }\n            actions.push(PlannedAction {",
        "            if false {\n                dest = resolve_rename(&dest);\n            }\n            actions.push(PlannedAction {",
        [SELF_LINK],
    ),
    (
        "the same entry ignores the folder",
        "            (Ok(x), Ok(y)) if x == y",
        "            (Ok(_), Ok(_))",
        [SAME_ENTRY],
    ),
    # -- a link is never followed ----------------------------------------------
    (
        "a delete follows links",
        "            // files of whatever folder the link reached.\n            let meta = fs::symlink_metadata(&src)?;",
        "            // files of whatever folder the link reached.\n            let meta = fs::metadata(&src)?;",
        [DELETE_LINK, CYCLE],
    ),
    (
        "a copy follows links",
        "            // copied as the link. See `PlannedAction::is_link`.\n            let meta = fs::symlink_metadata(&src)?;",
        "            // copied as the link. See `PlannedAction::is_link`.\n            let meta = fs::metadata(&src)?;",
        [MOVE_LINK, CYCLE],
    ),
    (
        "a link is deleted as a file",
        "        } else if action.is_link {\n            remove_link(&action.src)?;",
        "        } else if false {\n            remove_link(&action.src)?;",
        [DELETE_LINK],
    ),
    (
        "whatever is there is removed as a link",
        "    if !is_link(path) {\n        return Err(io::Error::other(\"it is no longer a link\"));",
        "    if false {\n        return Err(io::Error::other(\"it is no longer a link\"));",
        [NOT_A_LINK],
    ),
    (
        "a folder in the way is removed whole",
        "    if meta.is_dir() {\n        return Err(io::Error::other(",
        "    if meta.is_dir() {\n        return fs::remove_dir_all(path);\n    }\n    if false {\n        return Err(io::Error::other(",
        [NO_FOLDER],
    ),
    (
        "a tree of any depth is walked",
        "    if depth > MAX_TREE_DEPTH {",
        "    if false {",
        [TOO_DEEP],
    ),
]

TABLES = {
    "main.rs": MAIN,
    "columns.rs": COLUMNS,
    "columnprefs.rs": COLUMNPREFS,
    "fileops.rs": FILEOPS,
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
        worst = max(worst, sweep(SRC / file, rows, "explorer", timeout=900, only=mine))
    raise SystemExit(worst)

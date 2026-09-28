"""Mutation test for the file manager's thumbnail worker.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The scroll rows (`MAIN`, 2026-09-27): the listing's viewport was never told
how many rows fit, and measured the whole pane rather than the listing's part
of it -- so the first arrow press scrolled its row off the top, the wheel ran
past the last page, a grid's short last row could not be reached, and with
the preview open the scrollbar sat over the preview.

The table covers what changed on 2026-09-26: thumbnails were made a few per
tick on the thread that draws, and are made on `offloop::Queue`'s worker now,
filed as each arrives.  `offloop`'s own rules are swept by
`apps/offloop/mutate.py`.

And what changed on 2026-09-27 (`FILEOPS`, `COLUMNPREFS` and the prompt rows
in `MAIN`): the copy engine waits at a taken name instead of skipping it and
the window asks; a file is never replaced by itself, nor a folder put inside
itself; a link is copied, moved and deleted as the link and never followed;
a tree too deep to be real is refused.

And the recycle bin's view (`BINVIEW`, and the bin rows in `MAIN`): the pane
shows the bin in place of the folder, and nothing done there -- a click, a
key, the wheel, the preview's divider -- reaches the folder behind it.

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
BIN_SIDEBAR = "the_sidebar_opens_the_bin_and_each_item_is_listed_where_it_came_from"
BIN_CLI = "the_command_line_can_open_on_the_recycle_bin"
BIN_RESTORE = "restore_puts_the_chosen_item_back_and_says_where"
BIN_TAKEN = "restoring_over_a_taken_name_lands_beside_it_and_says_so"
BIN_ERASE = "delete_permanently_asks_and_erases_only_what_was_chosen"
BIN_EMPTY = "empty_erases_what_the_bin_showed_when_it_asked_and_no_more"
BIN_DAMAGED = "a_damaged_entry_can_be_erased_but_not_put_back"
BIN_LEAVE = "back_escape_and_backspace_leave_the_bin_for_the_folder"
BIN_NAVIGATE = "going_to_a_folder_from_the_bin_leaves_it"
BIN_KEYS = "the_folders_keys_do_nothing_while_the_bin_is_showing"
BIN_SCROLL = "the_scrollbar_and_the_wheel_move_the_bin_while_it_is_showing"
BIN_PREVIEW = "the_bin_has_the_whole_pane_even_with_the_preview_open"
BIN_MENU = "the_bins_menu_acts_on_the_row_under_the_pointer"
BIN_CHOOSE = "choosing_a_row_chooses_it_alone"
BIN_SHIFT = "the_arrows_move_the_choice_and_shift_extends_it"
BIN_HIT = "a_click_finds_the_row_drawn_under_it_after_a_scroll"
BIN_FIT = "fitting_the_same_height_again_leaves_the_scroll_alone"
BIN_BUTTONS = "the_buttons_say_why_they_are_off"
BIN_ROW = "a_row_says_where_it_was_deleted_from_and_when"
BIN_KINDS = "a_folder_and_a_link_say_what_they_are_instead_of_a_size"
BIN_SUMMARY = "the_summary_counts_the_bin_and_the_choice"
WHEEL_ENDS = "the_wheel_does_not_scroll_past_either_end"
ICON_END = "end_in_the_icon_grid_shows_the_last_icon"
ICON_WALK = "arrowing_through_the_icon_grid_keeps_the_chosen_icon_drawn"
GRID_FIRST = "a_grid_rounds_its_offset_to_a_row_and_reaches_a_short_last_row"
BAR_BESIDE = "with_the_preview_open_the_scrollbar_is_the_listings"
BELOW = "with_the_preview_below_the_arrows_keep_the_selection_above_it"
HEADER = "the_column_header_is_the_listings_not_the_panes"
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
        "the listing is never told how many rows fit",
        "        let folder = self.folder_capacity();\n        if self.viewport.height() != folder {\n"
        "            self.viewport.set_height(folder, self.entries.len());\n        }",
        "",
        [WHEEL_ENDS],
    ),
    (
        "the selection is revealed by entries",
        "        self.reveal_entry(index);\n        true",
        "        self.viewport.select(Some(index), self.entries.len());\n        true",
        [ICON_WALK],
    ),
    (
        "the grid always rounds down",
        "    if at_the_end && down.saturating_add(cells) < len {",
        "    if at_the_end && false {",
        [GRID_FIRST, ICON_END],
    ),
    (
        "the scrollbar is at the pane's edge",
        "        // edge bare.\n        let pane = self.list_rect();",
        "        // edge bare.\n        let pane = self.pane_rect();",
        [BAR_BESIDE],
    ),
    (
        "the rows that fit are counted in the whole pane",
        "    fn folder_capacity(&self) -> usize {\n        let list = self.list_rect();",
        "    fn folder_capacity(&self) -> usize {\n        let list = self.pane_rect();",
        [BELOW],
    ),
    (
        "the column header is the pane's",
        "        // over a column.\n        let pane = self.list_rect();",
        "        // over a column.\n        let pane = self.pane_rect();",
        [HEADER],
    ),
    (
        "a click in the bin's pane is not the bin's",
        "        if self.bin.is_some() && self.pane_rect().contains(x, y) {\n            return self.click_bin(x, y);\n        }",
        "",
        [BIN_RESTORE],
    ),
    (
        "the sidebar's Recycle Bin opens nothing",
        "            if self.bin.is_none() {\n                self.open_recycle_bin();\n            }",
        "            if self.bin.is_none() {}",
        [BIN_SIDEBAR],
    ),
    (
        "--recycle-bin is taken for a folder's name",
        '    if first == "--recycle-bin" {',
        '    if first == "--recycle-bin-never" {',
        [BIN_CLI],
    ),
    (
        "the folder's keys work under the bin",
        "        if self.bin.is_some() {\n            return self.handle_bin_key(k);\n        }",
        "",
        [BIN_KEYS],
    ),
    (
        "Back from the bin goes through history",
        "        if self.bin.is_some() {\n            self.leave_recycle_bin();\n            return;\n        }\n        if let Some(prev)",
        "        if let Some(prev)",
        [BIN_LEAVE],
    ),
    (
        "going to the folder the bin was opened over leaves it up",
        "        if self.bin.take().is_some() && path == self.current_path {",
        "        if self.bin.is_some() && path == self.current_path {",
        [BIN_NAVIGATE],
    ),
    (
        "the preview shares the pane with the bin",
        "        if !self.preview_open || self.bin.is_some() {",
        "        if !self.preview_open {",
        [BIN_PREVIEW],
    ),
    (
        "the scrollbar measures the folder under the bin",
        "            .map_or(self.entries.len(), |bin| bin.entries().len())",
        "            .map_or(self.entries.len(), |_| self.entries.len())",
        [BIN_SCROLL],
    ),
    (
        "the wheel scrolls the folder under the bin",
        "                if self.bin.is_some() {\n                    self.scroll_rows_by(rows);\n                    return true;\n                }",
        "",
        [BIN_SCROLL],
    ),
    (
        "a right-click in the bin opens the folder's menu",
        "            MouseEventKind::Press(MouseButton::Right) if self.bin.is_some() => {",
        "            MouseEventKind::Press(MouseButton::Right) if self.bin.is_some() && false => {",
        [BIN_MENU],
    ),
    (
        "a press on an off button is acted on",
        "        if bin.button_state(button).is_disabled() {\n            return true;\n        }\n        match button {",
        "        match button {",
        [BIN_DAMAGED],
    ),
    (
        "Delete permanently erases without asking",
        "        self.modal = Some(Modal::ConfirmBin {\n            dialog,\n            action: BinAction::DeleteChosen(ids),\n        });",
        "        drop(dialog);\n        self.erase_from_bin(&ids, false);",
        [BIN_ERASE],
    ),
    (
        "Empty erases what arrived after it asked",
        "                        BinAction::Empty(ids) => self.erase_from_bin(&ids, true),",
        "                        BinAction::Empty(_) => {\n"
        "                            let ids: Vec<String> = self.recycle.list().map(|l| l.into_iter().map(|e| e.id).collect()).unwrap_or_default();\n"
        "                            self.erase_from_bin(&ids, true);\n"
        "                        }",
        [BIN_EMPTY],
    ),
    (
        "a restore under a new name is not said to be one",
        "                    let renamed = landed != *original;",
        "                    let renamed = false;",
        [BIN_TAKEN],
    ),
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

BINVIEW = [
    (
        "choosing a row keeps the old choice",
        "        self.chosen.clear();\n        self.chosen.insert(entry.id.clone());",
        "        self.chosen.insert(entry.id.clone());",
        [BIN_CHOOSE],
    ),
    (
        "Shift chooses one row",
        "        if !extend {\n            self.choose_only(index);",
        "        if true {\n            self.choose_only(index);",
        [BIN_SHIFT],
    ),
    (
        "a run starts wherever the keyboard is",
        "        let anchor = *self.anchor.get_or_insert(index);",
        "        let anchor = index;",
        [BIN_SHIFT],
    ),
    (
        "the choice keeps entries that are gone",
        "        self.chosen.retain(|id| present.contains(id.as_str()));\n",
        "        let _ = &present;\n",
        [BIN_RESTORE],
    ),
    (
        "the hit test ignores the scroll",
        "        Some(match rows.start.checked_add(row) {",
        "        Some(match Some(row) {",
        [BIN_HIT],
    ),
    (
        "below the last row is the last row",
        "            Some(index) if index < rows.end() => BinHit::Row(index),",
        "            Some(index) => BinHit::Row(index.min(rows.end().saturating_sub(1))),",
        [BIN_HIT],
    ),
    (
        "fitting undoes the wheel",
        "        if self.viewport.height() != capacity {",
        "        if self.viewport.height() == capacity || true {",
        [BIN_FIT],
    ),
    (
        "a damaged entry may be put back",
        "            BinButton::Restore if !self.chosen().iter().any(|e| e.is_readable()) => {",
        "            BinButton::Restore if false => {",
        [BIN_BUTTONS],
    ),
    (
        "the folder column shows the whole path",
        "        Some(path) => path\n            .parent()",
        "        Some(path) => Some(path.as_path())",
        [BIN_ROW],
    ),
    (
        "a folder shows its byte count",
        "    } else if entry.is_dir {\n        \"Folder\".to_string()",
        "    } else if false {\n        \"Folder\".to_string()",
        [BIN_KINDS],
    ),
    (
        "one item is counted as items",
        "    if count == 1 {\n        \"1 item\".to_string()",
        "    if count == 0 {\n        \"1 item\".to_string()",
        [BIN_SUMMARY],
    ),
]

TABLES = {
    "main.rs": MAIN,
    "binview.rs": BINVIEW,
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

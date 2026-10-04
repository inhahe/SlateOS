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

And the failure prompt (2026-09-28, `FILEOPS` and the failure rows in
`MAIN`): a file an operation cannot carry out is asked about -- try again,
skip, skip all, stop -- as a taken name is, where it used to be skipped and
said at the end; a file that failed part-way is tried again from its first
byte; a failure the user answered is not reported again at the end; and
both prompts stand on a panel, where under borders they were an outline.

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
FAIL_ASKED = "a_failed_file_is_asked_about_and_nothing_moves_until_it_is_answered"
FAIL_AGAIN = "try_again_carries_the_file_out_again"
FAIL_SKIP = "skip_leaves_the_file_failed_and_goes_on"
FAIL_SKIP_ALL = "skip_all_skips_every_later_failure_without_asking"
FAIL_STOP = "stop_ends_the_operation_and_what_is_done_stays_done"
FAIL_COUNTS = "a_failure_the_plan_skips_is_counted_as_failed_alone"
FAIL_PART_WAY = "a_file_that_failed_part_way_is_tried_again_from_its_first_byte"
FAIL_CANCEL = "cancelling_an_operation_stopped_at_a_failure_ends_it"
W_FAIL_ASKED = "a_file_a_paste_cannot_copy_is_asked_about"
W_FAIL_ANSWERS = "each_answer_to_a_failure_does_what_it_says"
W_FAIL_STOP = "stop_at_a_failure_stops_the_paste"
W_FAIL_CLICK = "the_failure_prompt_answers_a_click_on_its_buttons"
W_FAIL_CANCEL = "cancelling_a_paste_stopped_at_a_failure_takes_its_prompt_down"
W_FAIL_UNTOLD = "a_failure_after_skip_all_is_reported_at_the_end"
W_FAIL_NARROW = "the_failure_prompt_keeps_its_answers_inside_a_narrow_window"
W_WHY_WRAP = "a_long_reason_is_wrapped_above_the_answers"
W_WHY_CUT = "a_reason_past_three_lines_is_cut_on_the_third"
W_FILLED = "the_prompts_are_filled_in_every_look"
FAILURE_DEFAULT = "ask_is_what_a_failure_does_until_told_otherwise"
FAILURE_REMEMBERED = (
    "a_failed_file_is_asked_about_until_the_user_chooses_otherwise_and_the_choice_is_remembered"
)
FAILURE_MENU = "the_failure_choice_is_offered_on_the_folder_menu"
FAILURE_SKIPS = "a_paste_told_to_skip_skips_a_file_it_cannot_copy_and_says_so_at_the_end"
LINK_DRAG = "an_alt_drag_makes_a_link_or_reports_that_it_could_not"
EXIF = "a_photographs_exif_is_three_columns"
LATE = "exif_past_the_head_of_a_webp_is_found"
CARD_MODAL = "the_shortcut_card_takes_every_key_and_press_while_it_is_up"

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
        "            if let Some(question) = op.executor.waiting_on() {\n                Some(Modal::Conflict {",
        "            if let Some(question) = op.executor.waiting_on().filter(|_| false) {\n                Some(Modal::Conflict {",
        [ASKS],
    ),
    (
        "an operation waiting on an answer wants the clock",
        "        self.operations.iter().any(|op| !op.executor.asking())",
        "        self.operations.iter().any(|_| true)",
        [ASKS, W_FAIL_ASKED],
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
    # requests/c-e-the-explorers-address-completions-are-drawn-under-its-listing.md
    (
        "the address bar is drawn before the panes it hangs over",
        "        self.render_sidebar(&mut tree, &mut zones);\n\n"
        "        // File list\n        self.render_file_list(&mut tree, &mut zones);\n\n"
        "        // The Transfers view over the bottom of the listing, before the\n"
        "        // status bar it sits above.\n        self.render_transfers(&mut tree);\n\n"
        "        // The address bar after the panes, because the completions it offers\n"
        "        // hang below it, over the sidebar and the listing: drawn before them,\n"
        "        // as it was, the list was there -- Tab and the arrows worked on it --\n"
        "        // and painted over, so nobody could see it. The bar's own rectangle\n"
        "        // overlaps nothing, so nothing else moves.\n"
        "        self.render_address_bar(&mut tree);\n",
        "        self.render_address_bar(&mut tree);\n"
        "        self.render_sidebar(&mut tree, &mut zones);\n"
        "        self.render_file_list(&mut tree, &mut zones);\n"
        "        self.render_transfers(&mut tree);\n",
        ["the_address_completions_are_drawn_over_the_listing_and_take_a_press"],
    ),
    (
        "a press on a completion goes to what is under it",
        "        address.contains(x, y)\n            || self\n",
        "        address.contains(x, y)\n            || false && self\n",
        ["the_address_completions_are_drawn_over_the_listing_and_take_a_press"],
    ),
    # -- a failed file is asked about (2026-09-28) ------------------------------
    (
        "a failure is never asked about",
        "                op.executor.failed_on().map(|question| Modal::Failed {",
        "                op.executor.failed_on().filter(|_| false).map(|question| Modal::Failed {",
        [W_FAIL_ASKED],
    ),
    (
        "a failure prompt outlives its question",
        "                if !asking(&self.operations, prompt.plan, true) {",
        "                if false && !asking(&self.operations, prompt.plan, true) {",
        [W_FAIL_CANCEL],
    ),
    (
        "the failure prompt's answer is not given",
        "        self.modal = None;\n        self.answer_failure(plan, answer);",
        "        self.modal = None;\n        let _unused = (plan, answer);",
        [W_FAIL_ANSWERS],
    ),
    (
        "a failure's answer goes to another operation",
        "            .find(|op| op.executor.plan_id() == plan)\n        {\n            running.executor.answer_error(answer);",
        "            .find(|op| op.executor.plan_id() != plan)\n        {\n            running.executor.answer_error(answer);",
        [W_FAIL_ANSWERS],
    ),
    (
        "Enter does not try again",
        "                if key.key == Key::Enter {\n                    return (true, Some(ErrorAnswer::TryAgain));",
        "                if key.key == Key::Tab {\n                    return (true, Some(ErrorAnswer::TryAgain));",
        [W_FAIL_ANSWERS],
    ),
    (
        "a key answers what another key does",
        "                let answer = ERROR_BUTTONS\n                    .iter()\n                    .find(|(_, _, k)| *k == key.key)",
        "                let answer = ERROR_BUTTONS\n                    .iter()\n                    .find(|(_, _, k)| *k != key.key)",
        [W_FAIL_ANSWERS],
    ),
    (
        "a click anywhere answers the failure prompt",
        "                    .find(|(_, r)| r.contains(m.x, m.y))\n                    .map(|(answer, _)| *answer);",
        "                    .find(|(_, _)| true)\n                    .map(|(answer, _)| *answer);",
        [W_FAIL_CLICK],
    ),
    (
        "passing over a failure's answer gives it",
        "                if m.kind != MouseEventKind::Press(MouseButton::Left) {\n                    return (true, None);\n                }\n                let answer = self\n                    .hits\n                    .iter()\n                    .find(|(_, r)| r.contains(m.x, m.y))\n                    .map(|(answer, _)| *answer);",
        "                let answer = self\n                    .hits\n                    .iter()\n                    .find(|(_, r)| r.contains(m.x, m.y))\n                    .map(|(answer, _)| *answer);",
        [W_FAIL_CLICK],
    ),
    # Not a row: the prompt answering "mine" for an event it ignores (a
    # tick, a resize). A tick reaches the work behind a modal whatever the
    # modal says, and the window loop redraws after a resize whatever the
    # window says, so the only difference is one redundant repaint --
    # swept 2026-09-28 and survived, as an equivalent mutant must.
    (
        "a failed copy is called something else",
        "        FileOperation::Copy => \"copy\",",
        "        FileOperation::Copy => \"move\",",
        [W_FAIL_ASKED],
    ),
    (
        "a failed link is called something else",
        "        FileOperation::Link => \"make a link to\",",
        "        FileOperation::Link => \"link\",",
        [LINK_DRAG],
    ),
    (
        "the reason is one line cut at the edge",
        "        let why = why_lines(&self.why, inner);",
        "        let why = vec![self.why.clone()];",
        [W_WHY_WRAP],
    ),
    (
        "the reason is given every line it wraps to",
        "    if lines.len() > WHY_LINES {",
        "    if false {",
        [W_WHY_CUT],
    ),
    (
        "the reason's last line is not cut with an ellipsis",
        "        lines.push(guitk::text::elide(&rest, inner, \"\\u{2026}\", 12.0, weight));",
        "        lines.push(rest);",
        [W_WHY_CUT],
    ),
    (
        "the answers do not move down for the reason",
        "        let first_row = y + ERROR_PROMPT_H - 50.0 + more_why;",
        "        let first_row = y + ERROR_PROMPT_H - 50.0;",
        [W_WHY_WRAP],
    ),
    (
        "the card does not grow for the reason",
        "        let card_h = ERROR_PROMPT_H + more_why + PROMPT_ROW",
        "        let card_h = ERROR_PROMPT_H + PROMPT_ROW",
        [W_WHY_WRAP],
    ),
    (
        "the failure prompt's answers do not wrap in a narrow window",
        "        let card_h = ERROR_PROMPT_H + more_why + PROMPT_ROW * f32::from(rows.saturating_sub(1));",
        "        let card_h = ERROR_PROMPT_H + more_why;",
        [W_FAIL_NARROW],
    ),
    (
        "the failure prompt is an outline under borders",
        "        let card_h = ERROR_PROMPT_H + more_why + PROMPT_ROW * f32::from(rows.saturating_sub(1));\n"
        "        let x = ((w - card_w) / 2.0).max(0.0);\n        let y = ((h - card_h) / 2.0).max(0.0);\n"
        "        pal.push_surface(\n            &mut tree.commands,\n            x,\n            y,\n            card_w,\n            card_h,\n            8.0,\n            // A panel, as a dialog is: filled in every look. A card is an\n            // outline alone under borders, and the prompt's words would sit\n            // on the dimmed listing with its rows showing through.\n            appearance::Surface::Panel,",
        "        let card_h = ERROR_PROMPT_H + more_why + PROMPT_ROW * f32::from(rows.saturating_sub(1));\n"
        "        let x = ((w - card_w) / 2.0).max(0.0);\n        let y = ((h - card_h) / 2.0).max(0.0);\n"
        "        pal.push_surface(\n            &mut tree.commands,\n            x,\n            y,\n            card_w,\n            card_h,\n            8.0,\n            // A panel, as a dialog is: filled in every look. A card is an\n            // outline alone under borders, and the prompt's words would sit\n            // on the dimmed listing with its rows showing through.\n            appearance::Surface::Card,",
        [W_FILLED],
    ),
    (
        "the taken-name prompt is an outline under borders",
        "        let card_h = PROMPT_H + PROMPT_ROW * f32::from(rows.saturating_sub(1));\n"
        "        let x = ((w - card_w) / 2.0).max(0.0);\n        let y = ((h - card_h) / 2.0).max(0.0);\n"
        "        pal.push_surface(\n            &mut tree.commands,\n            x,\n            y,\n            card_w,\n            card_h,\n            8.0,\n            // A panel, as a dialog is: filled in every look. A card is an\n            // outline alone under borders, and the prompt's words would sit\n            // on the dimmed listing with its rows showing through.\n            appearance::Surface::Panel,",
        "        let card_h = PROMPT_H + PROMPT_ROW * f32::from(rows.saturating_sub(1));\n"
        "        let x = ((w - card_w) / 2.0).max(0.0);\n        let y = ((h - card_h) / 2.0).max(0.0);\n"
        "        pal.push_surface(\n            &mut tree.commands,\n            x,\n            y,\n            card_w,\n            card_h,\n            8.0,\n            // A panel, as a dialog is: filled in every look. A card is an\n            // outline alone under borders, and the prompt's words would sit\n            // on the dimmed listing with its rows showing through.\n            appearance::Surface::Card,",
        [W_FILLED],
    ),
    (
        "an answered failure is reported again at the end",
        "        let Some(untold) = errors.iter().find(|e| !e.answered) else {",
        "        let Some(untold) = errors.iter().find(|_| true) else {",
        [W_FAIL_ANSWERS, W_FAIL_STOP],
    ),
    (
        "the end names a failure the user saw",
        "            untold.path.shown(),\n            untold.message",
        "            first.path.shown(),\n            first.message",
        [W_FAIL_UNTOLD],
    ),
    (
        "the Transfers view does not say which file failed",
        "                    .or_else(|| op.executor.failed_on().map(|q| q.path.as_path()))",
        "                    .or_else(|| None)",
        [W_FAIL_ASKED],
    ),
    # -- the folder menu's "When a file cannot be done" (2026-09-28, §1228) ------
    (
        "the failure choice is not offered",
        "            self.conflict_menu(),\n            self.failure_menu(),\n",
        "            self.conflict_menu(),\n",
        [FAILURE_MENU],
    ),
    (
        "the failure menu ticks the wrong choice",
        "                        *policy == self.failure_policy,",
        "                        *policy != self.failure_policy,",
        [FAILURE_MENU],
    ),
    (
        "choosing what a failure does changes nothing",
        "        self.failure_policy = policy;\n        columnprefs::set_failure_policy",
        "        let _ = policy;\n        columnprefs::set_failure_policy",
        [FAILURE_REMEMBERED],
    ),
    (
        "the failure choice is not read, when a window opens or again",
        "        self.failure_policy = columnprefs::failure_policy(prefs);\n",
        "",
        [FAILURE_REMEMBERED, "a_choice_made_in_another_window_reaches_this_one"],
    ),
    (
        "a paste asks whatever was chosen",
        "            _ => OperationPlan::plan_copy(\n                &paths,\n                &self.current_path,\n"
        "                self.conflict_policy,\n                self.failure_policy,",
        "            _ => OperationPlan::plan_copy(\n                &paths,\n                &self.current_path,\n"
        "                self.conflict_policy,\n                ErrorPolicy::Ask,",
        [FAILURE_SKIPS],
    ),
    (
        'an announced explorer.yaml is not read again',
        '            return group.file_name() == columnprefs::CONFIG_NAME && self.reread_prefs();',
        '            return false && group.file_name() == columnprefs::CONFIG_NAME && self.reread_prefs();',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        "every program's announcement is read as the explorer's",
        '            return group.file_name() == columnprefs::CONFIG_NAME && self.reread_prefs();',
        '            return !group.file_name().is_empty() && self.reread_prefs();',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        'an announcement waits behind a dialog',
        '        if let Event::SettingsChanged { group } = event {',
        '        if let (Event::SettingsChanged { group }, None) = (event, &self.modal) {',
        ['a_choice_made_elsewhere_is_taken_up_while_a_dialog_is_open'],
    ),
    (
        "the preview's being open is not taken up",
        '        self.preview_open = columnprefs::preview_open(prefs);\n',
        '',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        "the preview's split is not taken up",
        '        self.preview_split = columnprefs::preview_split(prefs);\n',
        '',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        "the preview's side is not taken up",
        '        self.preview_side = columnprefs::preview_side(prefs);\n',
        '',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        "the icons' labels are not taken up",
        '        self.icon_labels = columnprefs::icon_labels(prefs);\n',
        '',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        'what a paste does with a taken name is not taken up',
        '        self.conflict_policy = columnprefs::conflict_policy(prefs);\n',
        '',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        "the thumbnails' size is not taken up",
        '        if size != self.thumb_config.size {\n            self.thumb_config.size = size;',
        '        if size != self.thumb_config.size {\n            self.thumb_config.size = self.thumb_config.size;',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        'thumbnails made at the old size are kept when the file changes it',
        '            self.thumbs.clear();\n            self.queue_thumbnails();\n        }\n    }',
        '            self.queue_thumbnails();\n        }\n    }',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        'thumbnails made at the old size are kept when the menu changes it',
        '        self.thumbs.clear();\n        self.queue_thumbnails();\n',
        '        self.queue_thumbnails();\n',
        ['a_chosen_thumbnail_size_applies_and_is_remembered'],
    ),
    (
        'the window opens on the out-of-the-box choices',
        '        state.take_up_prefs();\n',
        '',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        'a file unchanged is read as a change',
        '        if prefs.to_text() == self.column_prefs.to_text() {\n            return false;\n        }\n',
        '',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        'a re-read keeps the document it had',
        '        let before = std::mem::replace(&mut self.column_prefs, prefs);',
        '        let before = self.column_prefs.clone();\n        drop(prefs);',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        'a column set saved elsewhere is not taken up',
        '        if saved_columns(&before) != saved_columns(&self.column_prefs) {',
        '        if false {',
        ['a_choice_made_in_another_window_reaches_this_one', 'unsaved_columns_outlast_another_windows_choice'],
    ),
    (
        'columns shown here and not saved are undone by any change elsewhere',
        '        if saved_columns(&before) != saved_columns(&self.column_prefs) {',
        '        if true {',
        ['unsaved_columns_outlast_another_windows_choice'],
    ),
    (
        'an arrangement made elsewhere is not taken up',
        '        if manualorder::for_folder(&before, &folder)\n            != manualorder::for_folder(&self.column_prefs, &folder)\n        {',
        '        if false {',
        ['an_arrangement_made_elsewhere_keeps_the_selection_on_its_files'],
    ),
    (
        'a drag goes on across a new arrangement',
        '            self.row_drag = None;\n            self.load_manual_order();',
        '            self.load_manual_order();',
        ['an_arrangement_made_elsewhere_keeps_the_selection_on_its_files'],
    ),
    (
        'a new arrangement is not loaded',
        '            self.load_manual_order();\n            self.sync_sort_indicator();\n            self.resort();',
        '            self.sync_sort_indicator();\n            self.resort();',
        ['an_arrangement_made_elsewhere_keeps_the_selection_on_its_files'],
    ),
    (
        'a new arrangement is sorted without keeping the selection',
        '            self.load_manual_order();\n            self.sync_sort_indicator();\n            self.resort();\n        }\n        true',
        '            self.load_manual_order();\n            self.sync_sort_indicator();\n            self.sort_entries();\n        }\n        true',
        ['an_arrangement_made_elsewhere_keeps_the_selection_on_its_files'],
    ),
    (
        'sorting leaves the selection in its places',
        '        self.selected_indices = chosen\n',
        '        let _kept: Vec<usize> = chosen\n',
        ['an_arrangement_made_elsewhere_keeps_the_selection_on_its_files', 'sorting_keeps_the_selection_on_its_files'],
    ),
    (
        'choosing a sort does not keep the selection',
        '        self.sync_sort_indicator();\n        self.resort();\n    }\n\n    /// Switch view modes',
        '        self.sync_sort_indicator();\n        self.sort_entries();\n    }\n\n    /// Switch view modes',
        ['sorting_keeps_the_selection_on_its_files'],
    ),
    (
        'a re-read says it changed nothing',
        '            self.load_manual_order();\n            self.sync_sort_indicator();\n            self.resort();\n        }\n        true\n    }',
        '            self.load_manual_order();\n            self.sync_sort_indicator();\n            self.resort();\n        }\n        false\n    }',
        ['a_choice_made_in_another_window_reaches_this_one'],
    ),
    (
        'a click on a heading does nothing',
        '        if self.over_column_header(x, y) {\n            return self.sort_by_heading(x);\n        }\n',
        '',
        ['a_click_on_a_heading_sorts_by_it_and_again_the_other_way', 'a_heading_that_cannot_sort_says_so'],
    ),
    (
        'a heading that can sort does not',
        '            Some(by) => self.set_sort(by),',
        '            Some(_) => {}',
        ['a_click_on_a_heading_sorts_by_it_and_again_the_other_way'],
    ),
    (
        'a heading that cannot sort says nothing',
        '                self.status_message = format!(\n                    "The list cannot be sorted by {label} -- by Name, Size, Date modified or Type"\n                );\n',
        '                let _ = label;\n',
        ['a_heading_that_cannot_sort_says_so'],
    ),
    (
        'a heading asks for no sort',
        '            .find(|by| by.column() == Some(column))',
        '            .find(|_| false)',
        ['a_click_on_a_heading_sorts_by_it_and_again_the_other_way'],
    ),
    (
        "the Size heading's sort is another column's",
        '            Self::Size => Some(ColumnId::SIZE),',
        '            Self::Size => Some(ColumnId::NAME),',
        ['a_click_on_a_heading_sorts_by_it_and_again_the_other_way'],
    ),
    (
        'the folder menu has no Sort by',
        '            self.sort_menu(),\n            self.conflict_menu(),',
        '            self.conflict_menu(),',
        ['sort_by_is_on_the_folder_menu_and_the_headings_menu'],
    ),
    (
        "the headings' menu has no Sort by",
        '        let mut items = vec![self.sort_menu(), MenuItem::Separator];',
        '        let mut items = vec![MenuItem::Separator];',
        ['sort_by_is_on_the_folder_menu_and_the_headings_menu'],
    ),
    (
        'your own order is offered where there is none',
        '                    enabled: by != SortBy::Custom || !self.manual_order.is_empty(),',
        '                    enabled: true,',
        ['your_own_order_is_offered_only_where_there_is_one'],
    ),
    (
        'asking for your own order where there is none takes it anyway',
        '        if by == SortBy::Custom && self.manual_order.is_empty() {',
        '        if false {',
        ['your_own_order_is_offered_only_where_there_is_one'],
    ),
    (
        "the menu's sort is not applied",
        '        if self.sort_by != by {\n            self.sort_by = by;\n',
        '        if self.sort_by != by {\n',
        ['the_folders_own_order_comes_back_from_the_menu'],
    ),
    (
        'choosing the sort in force turns it round',
        '        if self.sort_by != by {\n            self.sort_by = by;\n            self.sort_dir = SortDir::Ascending;\n            self.sync_sort_indicator();\n            self.resort();\n        }\n        true\n    }',
        '        self.set_sort(by);\n        true\n    }',
        ['the_folders_own_order_comes_back_from_the_menu'],
    ),
    (
        "the menu's rows are counted from the submenu's own id",
        '            .checked_sub(MENU_SORT_BASE.saturating_add(1))',
        '            .checked_sub(MENU_SORT_BASE)',
        ['the_folders_own_order_comes_back_from_the_menu'],
    ),
    (
        'the menu does not reach the sort',
        '            || self.sort_action(id)\n',
        '',
        ['the_folders_own_order_comes_back_from_the_menu', 'your_own_order_is_offered_only_where_there_is_one'],
    ),
    (
        "a folder with nothing saved keeps the last folder's columns",
        '            if !self.apply_saved_columns() {\n                self.columns.show_built_in();\n            }',
        '            self.apply_saved_columns();',
        ['a_folder_with_nothing_saved_shows_the_built_in_columns'],
    ),
    (
        'a refresh re-applies the saved columns',
        '        if self.columns_folder.as_deref() != Some(self.current_path.as_path()) {',
        '        if true {',
        ['columns_shown_and_not_saved_outlast_a_refresh'],
    ),
    (
        "entering a folder keeps the last folder's columns",
        '        if self.columns_folder.as_deref() != Some(self.current_path.as_path()) {',
        '        if self.columns_folder.is_none() {',
        ['a_folder_with_nothing_saved_shows_the_built_in_columns'],
    ),
    (
        'the folder whose columns are shown is not recorded',
        '            self.columns_folder = Some(self.current_path.clone());\n',
        '',
        ['columns_shown_and_not_saved_outlast_a_refresh'],
    ),
    (
        'a file nobody chose a program for opens with nothing',
        '        let id = programs::default_for(guitk::filetypes::mime_for_extension(ext))?;',
        '        let id = programs::default_for("")?;',
        ['a_file_nobody_chose_a_program_for_opens_with_slateos_default'],
    ),
    (
        "the person's choice is not asked first",
        '        if let Some(program) = Self::opener_for(path) {',
        '        if let Some(program) = Self::opener_for(path).filter(|_| false) {',
        ['the_persons_choice_goes_before_the_default', 'opening_a_file_starts_what_the_user_chose_for_it'],
    ),
    (
        'the default is looked for among no programs',
        '        let app = known_programs(&self.app_dirs)\n            .into_iter()\n            .find(|app| app.id == id)?;',
        '        let app = Vec::<desktopentry::App>::new()\n            .into_iter()\n            .find(|app| app.id == id)?;',
        ['a_file_nobody_chose_a_program_for_opens_with_slateos_default'],
    ),
    (
        "an entry's command line is not given the file",
        '                &[desktopentry::Target::File(path.to_path_buf())],',
        '                &[],',
        ['a_file_nobody_chose_a_program_for_opens_with_slateos_default', 'open_with_offers_the_programs_that_open_the_type'],
    ),
    (
        'a program that runs in a terminal is started outside one',
        '        let (program, args) = if app.terminal {',
        '        let (program, args) = if false {',
        ['open_with_offers_the_programs_that_open_the_type'],
    ),
    (
        'Open With offers nothing',
        '            .map(|entry| self.programs_opening(&entry.path))',
        '            .map(|_| Vec::new())',
        ['open_with_offers_the_programs_that_open_the_type', 'an_unknown_kind_is_offered_the_hex_editor_and_a_folder_nothing'],
    ),
    (
        "a folder is offered what opens its name's kind",
        '            .filter(|entry| !entry.is_dir)\n            .map(|entry| self.programs_opening(&entry.path))',
        '            .map(|entry| self.programs_opening(&entry.path))',
        ['an_unknown_kind_is_offered_the_hex_editor_and_a_folder_nothing'],
    ),
    (
        'Open With offers programs that do not open the kind',
        '            .filter(|app| app.mime_types.iter().any(|m| m.eq_ignore_ascii_case(mime)))',
        '            .filter(|_| true)',
        ['an_unknown_kind_is_offered_the_hex_editor_and_a_folder_nothing'],
    ),
    (
        'Open With does not put the default first',
        '            list.insert(0, default);',
        '            list.push(default);',
        ['open_with_offers_the_programs_that_open_the_type'],
    ),
    (
        'the Open With row chosen starts the first program',
        '            .and_then(|n| self.open_with.get(n))',
        '            .and_then(|_| self.open_with.first())',
        ['open_with_offers_the_programs_that_open_the_type'],
    ),
    (
        "Open With's rows are counted from the submenu's own id",
        '            .checked_sub(MENU_OPEN_WITH_BASE.saturating_add(1))',
        '            .checked_sub(MENU_OPEN_WITH_BASE)',
        ['open_with_offers_the_programs_that_open_the_type'],
    ),
    (
        'Open With is not reached',
        '            || self.open_with_action(id)\n',
        '',
        ['open_with_offers_the_programs_that_open_the_type'],
    ),
    (
        'Open With is greyed where it has programs',
        '            enabled: !self.open_with.is_empty(),',
        '            enabled: false,',
        ['open_with_offers_the_programs_that_open_the_type'],
    ),
    (
        'AltGr is taken for Ctrl over a folder',
        '        let chord = textline::is_ctrl_chord(k.modifiers);',
        '        let chord = k.modifiers.ctrl;',
        ['a_chord_is_not_the_file_lists_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord works the file list',
        '        let plain = textline::is_plain(k.modifiers);',
        '        let plain = true;',
        ['a_chord_is_not_the_file_lists_key_and_altgr_is_not_ctrl'],
    ),
    (
        "a chord's letter is typed into the path",
        '            if textline::is_alt_or_windows_chord(k.modifiers) {',
        '            if false {',
        ['a_chord_is_not_the_file_lists_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord raises the keys',
        '        if self.modal.is_none() && plain {',
        '        if self.modal.is_none() {',
        ['a_chord_is_not_the_file_lists_key_and_altgr_is_not_ctrl'],
    ),
    (
        "a chord reaches the list's plain keys",
        '            _ if !plain => false,\n',
        '',
        ['a_chord_is_not_the_file_lists_key_and_altgr_is_not_ctrl'],
    ),
    (
        'AltGr+A chooses all in the bin',
        '        if k.key == Key::A && textline::is_ctrl_chord(k.modifiers) {',
        '        if k.key == Key::A && k.modifiers.ctrl {',
        ['a_chord_is_not_the_file_lists_key_and_altgr_is_not_ctrl'],
    ),
    (
        'back is taken with AltGr or the Windows key',
        '    k.modifiers.alt && !k.modifiers.ctrl && !k.modifiers.super_key',
        '    k.modifiers.alt || k.modifiers.super_key',
        ['a_chord_is_not_the_file_lists_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord works the bin',
        '        if !textline::is_plain(k.modifiers) {\n            return false;\n        }\n',
        '',
        ['a_chord_is_not_the_file_lists_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord answers the taken-name prompt',
        '            // modal.\n            Event::Key(key) if key.pressed && !textline::is_plain(key.modifiers) => (true, None),\n',
        '            // modal.\n',
        ['a_chord_is_not_the_file_lists_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord answers the failure prompt',
        '            // Answered by a plain key, as the taken-name prompt is.\n            Event::Key(key) if key.pressed && !textline::is_plain(key.modifiers) => (true, None),\n',
        '            // Answered by a plain key, as the taken-name prompt is.\n',
        ['a_chord_is_not_the_file_lists_key_and_altgr_is_not_ctrl'],
    ),
    # -- the theme's widget style (c-e-a-theme-can-shape-the-controls) ------
    (
        'the scrollbar is drawn the same wherever the pointer is',
        '            hovered: self.scrollbar_hovered,',
        '            hovered: false,',
        ['the_scrollbar_follows_the_themes_style_and_lights_under_the_pointer'],
    ),
    (
        'the pointer coming to the scrollbar is not noticed',
        '            MouseEventKind::Move => self.hover_scrollbar(m.x, m.y),',
        '            MouseEventKind::Move => false,',
        ['the_scrollbar_follows_the_themes_style_and_lights_under_the_pointer'],
    ),
    (
        'the scrollbar stays lit after the pointer leaves the window',
        '            MouseEventKind::Leave => std::mem::take(&mut self.scrollbar_hovered),',
        '            MouseEventKind::Leave => false,',
        ['the_scrollbar_follows_the_themes_style_and_lights_under_the_pointer'],
    ),
    (
        "the address bar and the dialogs take the toolkit's focus width",
        '        self.focus_ring_width = settings.focus_ring_width();',
        '        let _ = settings;',
        ['the_address_bar_and_the_dialogs_take_the_users_focus_width'],
    ),
    (
        "the New folder dialog takes the toolkit's focus width",
        '        let mut dialog = InputDialog::prompt("New folder", "Name:", "")\n            .with_focus_ring_width(self.focus_ring_width);',
        '        let mut dialog = InputDialog::prompt("New folder", "Name:", "");',
        ['the_address_bar_and_the_dialogs_take_the_users_focus_width'],
    ),
    # -- the shortcut card is modal, for the keys and the pointer
    (
        "the card is modal for nothing",
        '        if self.show_help {\n            match event {\n',
        '        if false && self.show_help {\n            match event {\n',
        [CARD_MODAL],
    ),
    (
        "a key that is not the card's acts behind it",
        '                    if closes {\n'
        '                        self.show_help = false;\n'
        '                    }\n'
        '                    return closes;\n',
        '                    if closes {\n'
        '                        self.show_help = false;\n'
        '                        return true;\n'
        '                    }\n',
        [CARD_MODAL],
    ),
    (
        "? does not put the card away",
        '                            || k.key == Key::Slash && k.modifiers.shift);\n',
        '                            || false);\n',
        [CARD_MODAL],
    ),
    (
        "Escape does not put the card away",
        '                        && (matches!(k.key, Key::F1 | Key::Escape)\n',
        '                        && (matches!(k.key, Key::F1)\n',
        [CARD_MODAL, 'the_shortcut_list_reaches_the_window'],
    ),
    (
        "a press goes through the card",
        '                    MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                        self.show_help = false;\n'
        '                        return true;\n'
        '                    }\n',
        '',
        [CARD_MODAL],
    ),
    (
        "only the left button puts the card away",
        '                    MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n',
        '                    MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n',
        [CARD_MODAL],
    ),
    (
        "the wheel scrolls what the card covers",
        '                    MouseEventKind::Scroll { .. } => return false,\n',
        '',
        [CARD_MODAL],
    ),
    (
        # 2026-10-04: the toolkit's input dialog types the text of every
        # key, a command's letter among it.
        "a command is typed into a name box",
        "        let command = matches!(event, Event::Key(key) if textline::is_command(key.modifiers));\n",
        "        let command = false;\n",
        ["a_command_is_not_typed_into_a_name_box"],
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
    (
        "the picture columns leave AVIF out",
        "            \"png\", \"jpg\", \"jpeg\", \"gif\", \"bmp\", \"webp\", \"avif\", \"ico\", \"cur\", \"tif\", \"tiff\",",
        "            \"png\", \"jpg\", \"jpeg\", \"gif\", \"bmp\", \"webp\", \"ico\", \"cur\", \"tif\", \"tiff\",",
        ["an_avif_is_measured"],
    ),
    (
        'a click names a heading to the left of the one under it',
        '        .find(|&(_, left, width)| x >= left && x < left + width)',
        '        .find(|&(_, left, width)| x >= left + width)',
        ['a_click_on_a_heading_sorts_by_it_and_again_the_other_way'],
    ),
    (
        "every heading starts at the header's left edge",
        '            left += width;\n',
        '',
        ['a_click_on_a_heading_sorts_by_it_and_again_the_other_way'],
    ),
    (
        'the built-in set is not name, size, date modified',
        '    pub const BUILT_IN: [ColumnId; 3] = [ColumnId::NAME, ColumnId::SIZE, ColumnId::DATE_MODIFIED];',
        '    pub const BUILT_IN: [ColumnId; 3] = [ColumnId::NAME, ColumnId::DATE_MODIFIED, ColumnId::SIZE];',
        ['a_folder_with_nothing_saved_shows_the_built_in_columns'],
    ),
    (
        'showing the built-in set shows nothing new',
        '        self.active_columns = Self::BUILT_IN.to_vec();\n',
        '        let _ = Self::BUILT_IN;\n',
        ['a_folder_with_nothing_saved_shows_the_built_in_columns'],
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
    # -- what an operation does with a file it cannot do (2026-09-28) -----------
    (
        "a failed file is skipped unasked by default",
        "        .unwrap_or(ErrorPolicy::Ask)",
        "        .unwrap_or(ErrorPolicy::SkipAndContinue)",
        [FAILURE_DEFAULT],
    ),
    (
        "the failure choice is not kept",
        "        doc.set_str(&ON_FAILURE, spelled);",
        "        let _ = spelled;",
        [FAILURE_REMEMBERED],
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
        "    pub fn step(&mut self) {\n        if self.asking() {",
        "    pub fn step(&mut self) {\n        if false {",
        [WAITS, FAIL_ASKED],
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
    # -- a failed file is asked about (2026-09-28) ------------------------------
    (
        "asking forgets a failed file",
        "        self.question.is_some() || self.failed.is_some()",
        "        self.question.is_some()",
        [W_FAIL_ASKED],
    ),
    (
        "a failure leaves no question",
        "                        self.failed = Some(ErrorQuestion {",
        "                        let _unused = Some(ErrorQuestion {",
        [FAIL_ASKED],
    ),
    (
        "a failed file is passed over while asking",
        "                        // question is answered, as for a taken name.\n                        self.next = self.next.saturating_sub(1);",
        "                        // question is answered, as for a taken name.\n                        let _unused = self.next;",
        [FAIL_AGAIN],
    ),
    (
        "a failure's answer is never taken",
        "        let Some(failed) = self.failed.take() else {",
        "        let Some(failed) = self.failed.clone() else {",
        [FAIL_AGAIN],
    ),
    (
        "Try again skips the file",
        "            ErrorAnswer::TryAgain => {}\n            ErrorAnswer::Skip | ErrorAnswer::SkipAll => {",
        "            ErrorAnswer::TryAgain | ErrorAnswer::Skip | ErrorAnswer::SkipAll => {",
        [FAIL_AGAIN],
    ),
    (
        "Skip tries the file again",
        "                self.next = self.next.saturating_add(1);\n                if answer == ErrorAnswer::SkipAll {",
        "                let _unused = self.next;\n                if answer == ErrorAnswer::SkipAll {",
        [FAIL_SKIP],
    ),
    (
        "Skip all asks again",
        "                    self.error_policy_for_the_rest = Some(ErrorPolicy::SkipAndContinue);",
        "                    let _unused = ErrorPolicy::SkipAndContinue;",
        [FAIL_SKIP_ALL],
    ),
    (
        "the policy for the rest is not read",
        "        let error_policy = self\n            .error_policy_for_the_rest\n            .unwrap_or(self.plan.error_policy);",
        "        let error_policy = self.plan.error_policy;",
        [FAIL_SKIP_ALL],
    ),
    (
        "Stop at a failure does not stop",
        "                self.record_failure(&failed.path, &failed.error, true);\n                self.cancel();",
        "                self.record_failure(&failed.path, &failed.error, true);\n                self.progress.state = OperationState::Running;",
        [FAIL_STOP],
    ),
    (
        "a skipped failure counts as one nobody saw",
        "                self.record_failure(&failed.path, &failed.error, true);\n                self.next = self.next.saturating_add(1);",
        "                self.record_failure(&failed.path, &failed.error, false);\n                self.next = self.next.saturating_add(1);",
        [W_FAIL_ANSWERS],
    ),
    (
        "a stop counts as a failure nobody saw",
        "                self.record_failure(&failed.path, &failed.error, true);\n                self.cancel();",
        "                self.record_failure(&failed.path, &failed.error, false);\n                self.cancel();",
        [W_FAIL_STOP],
    ),
    (
        "a failure skipped unasked counts as answered",
        "                        self.record_failure(&action.src, &e.to_string(), false);",
        "                        self.record_failure(&action.src, &e.to_string(), true);",
        [W_FAIL_UNTOLD],
    ),
    (
        "a failure is counted as skipped as well",
        "    fn record_failure(&mut self, path: &Path, error: &str, answered: bool) {\n",
        "    fn record_failure(&mut self, path: &Path, error: &str, answered: bool) {\n        self.skipped = self.skipped.saturating_add(1);\n",
        [FAIL_COUNTS, FAIL_SKIP],
    ),
    (
        "a file that failed part-way goes on from where it stood",
        "                self.discard_cursor();\n                match error_policy {",
        "                match error_policy {",
        [FAIL_PART_WAY],
    ),
    (
        "a cancel leaves the failure's question",
        "        self.question = None;\n        self.failed = None;",
        "        self.question = None;",
        [FAIL_CANCEL],
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

"""Mutation test for the kanban boards file: what is kept, how it is read back,
the ids an import brings, and the close.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The app kept nothing: every card was gone when the window closed, and a
board's JSON export was the only way to keep one.  An import also kept the ids
its file carried without moving the id counter past them, so the next card
made could replace an imported one.  The table covers the file it keeps now
(design-decisions §1205's kind), the import, the status line and the close;
the rest of the suite predates them.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "boards_written_and_read_again_are_the_same_boards"
REFUSED = "boards_that_cannot_be_read_whole_are_refused_and_say_where"
KEPT = "what_is_put_on_a_board_is_there_next_time"
FAILING = "closing_while_a_save_fails_asks_first"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a save writes nothing",
        "            .and_then(|()| safeio::write_str_atomically(&path, &text));",
        "            .and_then(|()| Ok::<(), std::io::Error>(()));",
        [KEPT],
    ),
    (
        "the file loses the comments",
        "            for comment in &card.comments {",
        "            for comment in card.comments.iter().take(0) {",
        [ROUND],
    ),
    (
        "the file loses a column's cards",
        "            push_ids(&mut out, &column.card_ids);\n",
        "",
        [ROUND],
    ),
    (
        "the file loses the archive",
        "        push_ids(&mut out, &board.archived_card_ids);\n",
        "",
        [ROUND],
    ),
    (
        "the file loses a colour's transparency",
        "    if c.a == 255 {",
        "    if true {",
        [ROUND],
    ),
    (
        "the file forgets which board was open",
        r'    let mut out = format!("{BOARDS_MAGIC}\t{BOARDS_FORMAT}\nactive\t{active}\n");',
        r'    let mut out = format!("{BOARDS_MAGIC}\t{BOARDS_FORMAT}\nactive\t0\n");',
        [ROUND],
    ),
    (
        "a later format is half-read",
        "    if version > BOARDS_FORMAT {",
        "    if version > BOARDS_FORMAT + 100 {",
        [REFUSED],
    ),
    (
        "two cards with one number are both taken",
        "                if board.cards.contains_key(&id) {",
        "                if false {",
        [REFUSED],
    ),
    (
        "a column naming a card the board does not have is read",
        "        if !board.cards.contains_key(&id) {",
        "        if false {",
        [REFUSED],
    ),
    (
        "a card in two places is read",
        "        if !placed.insert(id) {",
        "        if !placed.insert(id) && false {",
        [REFUSED],
    ),
    (
        "an open board past the end is taken",
        "        Some((at, index)) if index >= boards.len() => {",
        "        Some((at, index)) if false => {",
        [REFUSED],
    ),
    (
        "a file too big to read whole is read in part",
        "        if read.truncated {",
        "        if false {",
        ["a_file_too_big_to_read_whole_is_refused"],
    ),
    (
        "a refused file is saved over",
        "            Err(why) => {\n                self.persist = false;\n",
        "            Err(why) => {\n",
        ["a_file_that_cannot_be_read_is_left_as_it_is"],
    ),
    (
        "a first run writes the starting board",
        "        app.kept_text = boards_text(&app.boards, app.active_board_idx);\n",
        "",
        [KEPT],
    ),
    (
        "an event's change is not kept",
        "        if matches!(event, Event::Key(_) | Event::Mouse(_)) {\n            self.keep();\n        }\n",
        "",
        [KEPT, FAILING],
    ),
    (
        "a failed save is not said",
        '                self.store_error = Some(format!("Not saved to {}: {err}", path.display()));',
        "                drop(err);",
        [FAILING],
    ),
    (
        "the window closes over a failing save",
        "        if !self.unkept() {",
        "        if true {",
        [FAILING],
    ),
    (
        "keys reach the board under the question",
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "            && false",
        [FAILING],
    ),
    (
        "Save leaves while the save still fails",
        "                self.quit = !self.unkept();",
        "                self.quit = true;",
        [FAILING],
    ),
    (
        "an id read is not moved past",
        "        NEXT_ID.fetch_max(raw.saturating_add(1), Ordering::Relaxed);\n",
        "",
        ["a_card_made_after_an_import_replaces_nothing"],
    ),
    (
        "an import keeps a card in two columns",
        "                .retain(|id| board.cards.contains_key(id) && placed.insert(*id));",
        "                .retain(|id| board.cards.contains_key(id) && (placed.insert(*id) || true));",
        ["an_import_leaves_a_board_that_can_be_kept"],
    ),
    (
        "a stamp after a restart is earlier than the kept ones",
        "                self.last_stamp = self.last_stamp.max(latest);\n",
        "",
        ["stamps_are_the_clocks_and_always_later"],
    ),
    (
        "the status line is drawn nowhere",
        "    if app.view != View::CardDetail {\n        render_status(&mut tree, app, width, height);\n    }\n",
        "",
        ["the_empty_board_says_how_to_start_where_it_can_be_seen"],
    ),
    (
        "what an import did is drawn nowhere",
        "    } else if let Some(action) = &app.last_file_action {",
        "    } else if let Some(action) = None::<&String> {",
        ["what_an_import_did_is_on_the_status_line"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "kanban", timeout=900, only=only))

"""Mutation test for crossword's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Crossword is the forty-first application in this campaign, and it had no tests
at all.  `main` was `let _app = CrosswordApp::new();`: the program built three
puzzles, numbered them, dropped the whole thing and exited.  Nothing was ever
displayed and nothing could be played.

What that hid, in rough order of how badly it would have shown:

  * The three grids were not crosswords.  They were laid out from their across
    answers and nobody looked down the columns, which spelled `SAPETRE`,
    `HALLILA` and `FSIVRNA`.
  * The clue tables carried hand-written numbers, which the program matched
    against numbering it derived from the grid.  In all three puzzles the two
    disagreed, so a clue whose number named a *down* start was hung on whatever
    cell happened to carry that number -- and the eight words per puzzle the
    tables never mentioned got no clue at all.
  * The click handler re-derived the grid geometry from its own copies of
    `cell_size`, `grid_x` and `grid_y`, and its `grid_y` was 60.0 while the
    drawing pass used 72.0.  Every click was resolved against a grid twelve
    pixels above the one on the screen.
  * `elapsed_secs` was a field set to zero on load that nothing ever added to,
    under a readout that displayed it.
  * The end test was `all_filled && all_correct`, whose first half no board can
    fail while the second holds.
  * `flagged_wrong` was a field on every cell, set by the check and cleared at
    four separate call sites, so the mark could disagree with the letter.
  * The panel drew `.take(8)` of each direction from a scroll offset no event
    ever changed.
  * `move_to_next_clue(_reverse: bool)` and `word_length(.., _dir)` took
    arguments and ignored them.
  * `key_to_letter` answered `'\\0'` for "not a letter" and the caller asked
    `is_ascii_alphabetic` of the answer.
  * A clue was cut at `(w / 7.0) - 3` *bytes* -- a guessed advance and a byte
    offset into a `&str`, which aborts the process on the first accent.
  * Every heading was centred by subtracting half of one particular string at
    one particular size: `width / 2.0 - 100.0`, `cx - 60.0`, `bx + bw / 2.0 -
    30.0`.
  * The help card was a fixed 360x280 box, drawn outside any window narrower
    than 360.
  * The footer was one line of text naming eight keystrokes, in the one strip
    of the window that exists to be clicked.

Writing the suite turned up one more, which no amount of reading would have:
the clue panel's row budget counted clues while the drawing put direction
headings among them, so the last clue of every puzzle was on no screen the
panel could scroll to.  `every_clue_can_be_scrolled_onto_the_panel` is the
test that caught it and `PanelRow` is the fix.

Usage:  python -u apps/crossword/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The layout follows the window ─────────────────────────────────────
    (
        "the layout ignores the window it was given",
        "    fn solve(w: f32, h: f32, cols: usize, rows: usize) -> Self {\n"
        "        let w = w.max(0.0);\n        let h = h.max(0.0);",
        "    fn solve(w: f32, h: f32, cols: usize, rows: usize) -> Self {\n"
        "        let _ = (w, h);\n        let w = WINDOW_WIDTH;\n"
        "        let h = WINDOW_HEIGHT;",
        ["the_layout_follows_the_window_rather_than_a_constant"],
    ),
    (
        "the type size is a constant rather than a share of the height",
        "        let font = (h / 38.0).clamp(9.0, 17.0);",
        "        let font: f32 = 15.0;",
        ["the_layout_follows_the_window_rather_than_a_constant"],
    ),
    (
        "the margin is a constant rather than a share of the window",
        "        let pad = (w.min(h) * 0.03).clamp(4.0, 18.0);",
        "        let pad: f32 = 12.0;",
        ["the_layout_follows_the_window_rather_than_a_constant"],
    ),
    (
        "the header is a fixed height",
        "        let header = Rect::new(0.0, 0.0, w, (title + pad * 1.6).min(h));",
        "        let header = Rect::new(0.0, 0.0, w, 48.0f32.min(h));",
        ["the_layout_follows_the_window_rather_than_a_constant"],
    ),
    (
        "the cell is sized by the width alone",
        "            (avail / c).min(body.h / r).clamp(0.0, MAX_CELL)",
        "            (avail / c).clamp(0.0, MAX_CELL)",
        ["nothing_is_painted_outside_the_window"],
    ),
    (
        "a cell may grow without limit",
        "            (avail / c).min(body.h / r).clamp(0.0, MAX_CELL)",
        "            (avail / c).min(body.h / r).max(0.0)",
        ["a_cell_stops_growing_before_the_grid_swallows_the_monitor"],
    ),
    (
        "the panel is drawn however narrow the window",
        "        let panel = if panel_w >= MIN_PANEL_WIDTH {\n"
        "            Rect::new(panel_x, body.y, panel_w, body.h)\n"
        "        } else {\n            Rect::EMPTY\n        };",
        "        let panel = Rect::new(panel_x, body.y, panel_w, body.h);",
        ["a_panel_too_narrow_to_read_is_left_out_rather_than_drawn_narrow"],
    ),
    (
        "the panel's share is not given back to the grid when it is dropped",
        "        let cell = if body.w - grid_w - pad >= MIN_PANEL_WIDTH {\n"
        "            shared\n        } else {\n            fit(body.w)\n        };",
        "        let cell = shared;",
        ["the_grid_fills_the_body_when_the_window_has_no_room_for_a_panel"],
    ),
    (
        "the grid is not as wide as the cells it holds",
        "        let grid = Rect::new(\n            body.x,\n            body.y,\n"
        "            cell * cols.max(1) as f32,\n            cell * rows.max(1) as f32,\n        );",
        "        let grid = Rect::new(body.x, body.y, body.w * GRID_WIDTH_SHARE, body.h);",
        ["the_grid_is_square_and_its_cells_tile_it_exactly"],
    ),
    (
        "a cell's far edge is its own width rather than the next cell's edge",
        "        Rect::new(\n            x,\n            y,\n"
        "            self.edge_x(col.saturating_add(1)) - x,\n"
        "            self.edge_y(row.saturating_add(1)) - y,\n        )",
        "        Rect::new(x, y, self.cell, self.cell)",
        ["the_grid_is_square_and_its_cells_tile_it_exactly"],
    ),
    # ── The grid is a crossword ───────────────────────────────────────────
    (
        "a black square is a playable cell",
        "                Some(&ch) if ch != '#' => Some(Cell::new(ch)),",
        "                Some(&ch) => Some(Cell::new(ch)),",
        ["every_grid_spells_the_words_it_is_supposed_to"],
    ),
    (
        "a single cell counts as a word",
        "        !self.offset_playable(row, col, dir.back()) "
        "&& self.offset_playable(row, col, dir.step())",
        "        !self.offset_playable(row, col, dir.back())",
        ["every_run_of_two_or_more_cells_is_a_word_with_a_clue"],
    ),
    (
        "a word start does not look backwards",
        "        !self.offset_playable(row, col, dir.back()) "
        "&& self.offset_playable(row, col, dir.step())",
        "        self.offset_playable(row, col, dir.step())",
        ["every_grid_spells_the_words_it_is_supposed_to"],
    ),
    (
        "a cell that starts a word both ways is numbered twice",
        "                number = number.saturating_add(1);",
        "                number = number.saturating_add("
        "if across && down { 2 } else { 1 });",
        ["the_numbers_run_from_one_in_reading_order"],
    ),
    (
        "the clue lists are handed to the wrong directions",
        "            let texts = match dir {\n"
        "                Direction::Across => def.across,\n"
        "                Direction::Down => def.down,\n            };",
        "            let texts = match dir {\n"
        "                Direction::Across => def.down,\n"
        "                Direction::Down => def.across,\n            };",
        ["a_clue_carries_the_text_written_for_the_word_it_is_on"],
    ),
    (
        "a clue is hung on the cell one place along the list",
        "                    row,\n                    col,\n"
        "                    len: self.word_cells(row, col, dir).len(),",
        "                    row,\n                    col,\n"
        "                    len: self.word_cells(row, col, dir).len().saturating_add(1),",
        ["every_run_of_two_or_more_cells_is_a_word_with_a_clue"],
    ),
    (
        "a clue's number is the position it has in the list",
        "                self.clues.push(Clue {\n                    number,",
        "                self.clues.push(Clue {\n"
        "                    number: (i as u16).saturating_add(1),",
        ["every_clue_number_is_the_number_on_the_cell_it_starts_at"],
    ),
    (
        "a word the tables never mentioned is given the first clue instead of none",
        "                    text: texts.get(i).copied().unwrap_or(MISSING_CLUE),",
        "                    text: texts.get(i).copied().unwrap_or(\"\"),",
        ["a_word_with_no_clue_written_for_it_says_so_in_the_panel"],
    ),
    # ── Reading the grid ──────────────────────────────────────────────────
    (
        "a coordinate off the bottom of the board wraps to a row that exists",
        "        if row >= self.height || col >= self.width {\n            return None;\n        }",
        "        if col >= self.width {\n            return None;\n        }",
        ["a_coordinate_off_the_board_is_off_the_board"],
    ),
    (
        "a word ends where the row does rather than where the black square is",
        "        while self.playable(r, c) {\n            out.push((r, c));",
        "        while self.cell(r, c).is_some() || self.index(r, c).is_some() {\n"
        "            out.push((r, c));",
        ["every_grid_spells_the_words_it_is_supposed_to"],
    ),
    # ── The clock ─────────────────────────────────────────────────────────
    (
        "the clock does not advance",
        "        self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        "        self.elapsed_ms = self.elapsed_ms;",
        ["the_clock_advances_while_the_puzzle_is_open"],
    ),
    (
        "the clock runs on the menu and the end card too",
        "        if self.view != View::Playing {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["the_clock_is_stopped_in_the_menu_and_on_the_end_card"],
    ),
    (
        "the readout is a constant",
        '        format!("{m:02}:{s:02}")',
        '        format!("00:00")',
        ["the_readout_shows_the_clock_rather_than_a_constant"],
    ),
    (
        "the app never asks for a tick",
        "        Some(std::time::Duration::from_millis(TICK_MS))\n    }",
        "        None\n    }",
        ["the_app_asks_for_a_tick"],
    ),
    # ── Typing ────────────────────────────────────────────────────────────
    (
        "a typed letter does not move the cursor along the word",
        "        if let Some(next) = self.step_in_word(true) {\n            self.cursor = next;\n        }",
        "        {}",
        ["typing_a_letter_fills_the_cell_and_moves_along_the_word"],
    ),
    (
        "typing walks past the black square into the next word",
        "        .filter(|&(r, c)| self.playable(r, c))",
        "        .filter(|&(r, c)| self.index(r, c).is_some())",
        ["typing_stops_at_the_end_of_a_word_rather_than_jumping_the_black_square"],
    ),
    (
        "a letter typed over a revealed one stays the program's",
        "        cell.revealed = false;\n        if let Some(next) = self.step_in_word(true) {",
        "        if let Some(next) = self.step_in_word(true) {",
        ["typing_over_a_letter_that_was_given_away_makes_it_the_players"],
    ),
    (
        # The case of a typed letter is decided once, in `letter_of`. There used
        # to be a second `to_ascii_uppercase` at the store, which made this
        # mutation unobservable in either place.
        "a letter is stored in the case the key is named in",
        "        (Key::S, 'S'),",
        "        (Key::S, 's'),",
        ["a_letter_is_stored_upper_case_however_it_is_typed"],
    ),
    (
        "backspace steps back before it clears",
        "        if self\n            .cell(self.cursor.0, self.cursor.1)\n"
        "            .is_some_and(|c| c.entry.is_some())\n        {\n"
        "            self.clear_cell();\n            return;\n        }",
        "        {}",
        ["backspace_clears_this_cell_then_steps_back"],
    ),
    (
        "a key that types nothing types the letter A",
        "    letters\n        .into_iter()\n        .find_map(|(k, ch)| (k == key).then_some(ch))",
        "    Some(\n        letters\n            .into_iter()\n"
        "            .find_map(|(k, ch)| (k == key).then_some(ch))\n"
        "            .unwrap_or('A'),\n    )",
        ["a_key_that_types_nothing_is_not_treated_as_a_letter"],
    ),
    # ── The cursor ────────────────────────────────────────────────────────
    (
        "an arrow stops at the first black square instead of crossing it",
        "        while let Some((nr, nc)) = self.offset(r, c, step) {\n"
        "            (r, c) = (nr, nc);\n            if self.playable(r, c) {",
        "        if let Some((nr, nc)) = self.offset(r, c, step) {\n"
        "            (r, c) = (nr, nc);\n            if self.playable(r, c) {",
        ["an_arrow_crosses_a_black_square_where_typing_would_not"],
    ),
    (
        "an arrow that cannot move still turns the cursor",
        "            if self.playable(r, c) {\n                self.cursor = (r, c);\n"
        "                self.direction = face;\n                return true;\n            }\n"
        "        }\n        false",
        "            if self.playable(r, c) {\n                self.cursor = (r, c);\n"
        "                return true;\n            }\n        }\n"
        "        self.direction = face;\n        false",
        ["an_arrow_at_the_edge_stays_put_rather_than_wrapping"],
    ),
    (
        "the up arrow reads across",
        "            Key::Up => self.arrow((-1, 0), Direction::Down),",
        "            Key::Up => self.arrow((-1, 0), Direction::Across),",
        ["an_arrow_turns_the_cursor_the_way_it_moved"],
    ),
    (
        "the left arrow steps a row rather than a column",
        "            Key::Left => self.arrow((0, -1), Direction::Across),",
        "            Key::Left => self.arrow((-1, 0), Direction::Across),",
        ["the_four_arrows_are_the_same_code_and_each_undoes_the_other"],
    ),
    (
        "a step off the board wraps to the far side",
        "        (r < self.height && c < self.width).then_some((r, c))",
        "        Some((r % self.height.max(1), c % self.width.max(1)))",
        ["an_arrow_at_the_edge_stays_put_rather_than_wrapping"],
    ),
    (
        "space does not turn the cursor",
        "        self.direction = self.direction.other();",
        "        self.direction = self.direction;",
        ["space_turns_the_word_under_the_cursor"],
    ),
    # ── The clue panel ────────────────────────────────────────────────────
    (
        "the scroll limit counts clues rather than the rows they are drawn on",
        "        self.panel_rows().len().saturating_sub(visible)",
        "        self.clues.len().saturating_sub(visible)",
        ["every_clue_can_be_scrolled_onto_the_panel"],
    ),
    (
        "a heading is not a row of the list it heads",
        "                rows.push(PanelRow::Heading(clue.direction));",
        "                rows.push(PanelRow::Clue(i));",
        ["a_heading_is_a_row_of_the_list_it_heads"],
    ),
    (
        "the panel draws one row fewer than it has room for",
        "        for row in rows.iter().skip(first).take(visible) {",
        "        for row in rows.iter().skip(first).take(visible.saturating_sub(1)) {",
        ["the_panel_draws_every_row_it_has_room_for_and_no_more"],
    ),
    (
        "the panel starts at the top of the list whatever the scroll says",
        "        let first = self.clue_scroll.min(self.max_scroll());",
        "        let first = 0;",
        ["every_clue_can_be_scrolled_onto_the_panel"],
    ),
    (
        "a clue row does not say which way its word goes",
        "            Self::Across => 'A',",
        "            Self::Across => 'D',",
        ["a_clue_is_handed_to_the_renderer_whole_and_bounded_by_width"],
    ),
    (
        "the wheel does not move the list",
        "                if self.scroll_clues(rows) {",
        "                if self.scroll_clues(0) {",
        ["the_wheel_scrolls_the_clue_list_and_stops_at_both_ends"],
    ),
    (
        "the list may be scrolled past its last screen",
        "        self.clue_scroll = moved.min(self.max_scroll());",
        "        self.clue_scroll = moved;",
        ["the_wheel_scrolls_the_clue_list_and_stops_at_both_ends"],
    ),
    (
        "a fraction of a notch is thrown away rather than kept",
        "                let rows = self.scroll.rows(dy);",
        "                let rows = (dy * -3.0) as isize;",
        ["a_fraction_of_a_notch_is_kept_rather_than_rounded_away"],
    ),
    (
        "moving to a clue does not bring it onto the panel",
        "        self.direction = clue.direction;\n        self.show_clue(index);",
        "        self.direction = clue.direction;",
        ["moving_the_cursor_scrolls_the_clue_it_lands_on_into_view"],
    ),
    (
        "Shift+Tab walks the list the same way Tab does",
        "                self.cycle_clue(!key.modifiers.shift);",
        "                self.cycle_clue(true);",
        ["tab_and_shift_tab_walk_the_clue_list_in_opposite_directions"],
    ),
    # ── Clicking ──────────────────────────────────────────────────────────
    (
        "a click is resolved against a grid of its own",
        "    fn cell_rect(&self, row: usize, col: usize) -> Rect {\n"
        "        let x = self.edge_x(col);\n        let y = self.edge_y(row);",
        "    fn cell_rect(&self, row: usize, col: usize) -> Rect {\n"
        "        let x = self.edge_x(col);\n        let y = self.edge_y(row) - 12.0;",
        ["a_click_selects_the_cell_it_was_painted_in"],
    ),
    (
        "a cell's hit box is wider than the cell",
        "                f.hit(Target::Cell(row, col), r);",
        "                f.hit(\n                    Target::Cell(row, col),\n"
        "                    Rect::new(r.x, r.y, r.w * 2.0, r.h),\n                );",
        ["no_two_hit_boxes_claim_the_same_pixel"],
    ),
    (
        "clicking the cell the cursor is on does not turn the word",
        "                if self.cursor == (row, col) {\n"
        "                    self.toggle_direction();\n                } else {\n"
        "                    self.cursor = (row, col);\n                }",
        "                self.cursor = (row, col);",
        ["clicking_the_cell_the_cursor_is_already_on_turns_the_word"],
    ),
    (
        "clicking a clue moves the cursor without facing its direction",
        "        self.cursor = (clue.row, clue.col);\n        self.direction = clue.direction;",
        "        self.cursor = (clue.row, clue.col);",
        ["clicking_a_clue_moves_the_cursor_to_the_word_it_names"],
    ),
    (
        "clicking a menu row selects it without opening it",
        "                self.select(index);\n                self.load_puzzle(index);",
        "                self.select(index);",
        ["clicking_a_menu_row_opens_that_puzzle"],
    ),
    (
        "a click on nothing is a click on the grid",
        "            if self.show_help {\n                self.show_help = false;\n"
        "                return EventResult::Consumed;\n            }\n"
        "            return EventResult::Ignored;",
        "            return EventResult::Ignored;",
        ["a_click_on_nothing_puts_the_help_away_and_otherwise_does_nothing"],
    ),
    # ── The buttons ───────────────────────────────────────────────────────
    (
        "the Clear button clears the letters rather than the marks",
        "            Button::Clear => self.clear_checks(),",
        "            Button::Clear => self.clear_cell(),",
        # Not `the_buttons_and_the_keys_do_the_same_thing`: the key route sends
        # Ctrl+U through `press(Button::Clear)`, so both routes run this arm and
        # no comparison of the two can see what it does. That the button and the
        # key are one code path is the point of the button work; it just means
        # this arm is pinned by what it does, not by who reaches it.
        ["clearing_the_marks_leaves_the_letters_alone"],
    ),
    (
        "a button is drawn outside the footer",
        "            let r = Rect::new(x, y, w, h);\n            let on = button == Button::Check",
        "            let r = Rect::new(x, y - h, w, h);\n            let on = button == Button::Check",
        ["every_button_is_drawn_where_a_click_can_reach_it"],
    ),
    (
        "a button that will not fit is drawn off the edge anyway",
        "            if x + w > l.footer.right() - l.pad {\n                break;\n            }",
        "            {}",
        ["a_button_that_will_not_fit_is_left_out_rather_than_drawn_off_the_edge"],
    ),
    # ── Checking, revealing, finishing ────────────────────────────────────
    (
        "an empty cell is marked wrong",
        "                .is_some_and(|cell| cell.entry.is_some() && !cell.is_correct())",
        "                .is_some_and(|cell| !cell.is_correct())",
        ["check_marks_the_wrong_letters_and_nothing_else"],
    ),
    (
        "the marks are shown whether or not the puzzle was checked",
        "        self.check_mode\n            && self",
        "        true\n            && self",
        ["check_marks_the_wrong_letters_and_nothing_else"],
    ),
    (
        "clearing the marks clears the letters too",
        "    fn clear_checks(&mut self) {\n        self.check_mode = false;\n    }",
        "    fn clear_checks(&mut self) {\n        self.check_mode = false;\n"
        "        for cell in self.cells.iter_mut().flatten() {\n"
        "            cell.entry = None;\n        }\n    }",
        ["clearing_the_marks_leaves_the_letters_alone"],
    ),
    (
        "revealing a word fills the row rather than the word",
        "        for (r, c) in self.current_word() {",
        "        for (r, c) in (0..self.width)\n"
        "            .map(|c| (self.cursor.0, c))\n"
        "            .collect::<Vec<_>>()\n        {",
        ["revealing_a_word_fills_all_of_it_and_only_it"],
    ),
    (
        "a revealed letter is not marked as given away",
        "                cell.entry = Some(cell.solution);\n"
        "                cell.revealed = true;\n            }\n        }\n        self.settle();",
        "                cell.entry = Some(cell.solution);\n            }\n        }\n        self.settle();",
        ["revealing_a_word_fills_all_of_it_and_only_it"],
    ),
    (
        "a filled grid is a finished one",
        "        !self.cells.is_empty() && self.cells.iter().flatten().all(Cell::is_correct)",
        "        !self.cells.is_empty()\n            && self\n                .cells\n"
        "                .iter()\n                .flatten()\n"
        "                .all(|c| c.entry.is_some())",
        ["a_full_grid_of_wrong_letters_is_not_a_finished_puzzle"],
    ),
    (
        "an empty board has solved everything it holds",
        "        !self.cells.is_empty() && self.cells.iter().flatten().all(Cell::is_correct)",
        "        self.cells.iter().flatten().all(Cell::is_correct)",
        ["an_empty_board_is_not_a_solved_one"],
    ),
    (
        "the puzzle never ends",
        "        if self.is_solved() {\n            self.view = View::Completed;\n        }",
        "        {}",
        ["the_puzzle_ends_when_the_last_letter_goes_in_and_not_before"],
    ),
    (
        "Escape leaves the puzzle with the help card still up",
        "                if self.show_help {\n                    self.show_help = false;\n"
        "                } else {\n                    self.go_to_menu();\n                }",
        "                self.go_to_menu();",
        ["escape_puts_the_help_away_before_it_leaves_the_puzzle"],
    ),
    (
        "the end card is not a way back to the menu",
        "                if matches!(key.key, Key::Enter | Key::Escape) {",
        "                if matches!(key.key, Key::F1) {",
        ["the_end_card_is_a_way_back_to_the_menu_by_click_and_by_key"],
    ),
    (
        "the end card records no click",
        "        f.hit(Target::Button(Button::Menu), card);",
        "        let _ = card;",
        ["the_end_card_is_a_way_back_to_the_menu_by_click_and_by_key"],
    ),
    # ── The text ──────────────────────────────────────────────────────────
    (
        "a clue is cut by the panel rather than by the renderer",
        "                max_width: Some((r.w - l.pad * 0.6).max(0.0)),\n"
        "                overflow: TextOverflow::Ellipsis,",
        "                max_width: None,\n                overflow: TextOverflow::Clip,",
        ["a_clue_is_handed_to_the_renderer_whole_and_bounded_by_width"],
    ),
    (
        "a clue is cut at a byte offset before it reaches the renderer",
        '                text: format!("{}{}. {}", clue.number, '
        "clue.direction.initial(), clue.text),",
        '                text: format!(\n                    "{}{}. {}",\n'
        "                    clue.number,\n                    clue.direction.initial(),\n"
        "                    &clue.text[..clue.text.len().min(24)]\n                ),",
        ["a_clue_with_an_accent_in_it_is_drawn_rather_than_aborting"],
    ),
    (
        "the banner names the word without its length",
        '        let s = format!(\n            "{} {} ({}): {}",\n            clue.number,',
        '        let s = format!(\n            "{} {} {}: {}",\n            clue.number,',
        ["the_banner_names_the_word_the_cursor_is_in"],
    ),
    (
        "the banner is unbounded",
        "            max_width: Some((l.banner.w - l.pad * 2.0).max(0.0)),",
        "            max_width: None,",
        ["the_banner_names_the_word_the_cursor_is_in"],
    ),
    (
        "a heading is centred by a literal rather than by measuring it",
        "    let measured = text::measure(s, size, weight);\n"
        "    text_at(f, x + (w - measured) / 2.0, y, s, color, size, weight);",
        "    let _ = (w, weight);\n    text_at(f, x + 100.0, y, s, color, size, "
        "FontWeightHint::Bold);",
        ["a_heading_is_centred_by_measuring_it_rather_than_by_a_literal"],
    ),
    # ── The menu, and the frame ───────────────────────────────────────────
    (
        "the menu selection runs off the end of the list",
        "        self.selected_puzzle = index.min(PUZZLES.len().saturating_sub(1));",
        "        self.selected_puzzle = index;",
        ["the_menu_arrows_stay_inside_the_list"],
    ),
    (
        "opening a puzzle keeps the clock of the last one",
        "        self.elapsed_ms = 0;\n        self.check_mode = false;",
        "        self.check_mode = false;",
        ["opening_a_puzzle_starts_it_from_the_beginning"],
    ),
    (
        "opening a puzzle keeps the scroll of the last one",
        "        self.clue_scroll = 0;\n        self.view = View::Playing;",
        "        self.view = View::Playing;",
        ["opening_a_puzzle_starts_it_from_the_beginning"],
    ),
    (
        "a puzzle that does not exist opens an empty board",
        "        let Some(def) = PUZZLES.get(index) else {\n            return;\n        };",
        "        let Some(def) = PUZZLES.get(index % PUZZLES.len()) else {\n"
        "            return;\n        };",
        ["a_puzzle_that_does_not_exist_leaves_the_app_where_it_was"],
    ),
    (
        "an empty rectangle is still painted",
        "fn fill(f: &mut Frame<Target>, r: Rect, color: Color, radius: f32) {\n"
        "    if r.is_empty() {\n        return;\n    }",
        "fn fill(f: &mut Frame<Target>, r: Rect, color: Color, radius: f32) {",
        ["nothing_with_no_area_is_painted"],
    ),
    (
        "the help card is a fixed size",
        "        let card_w = (inner + l.pad * 2.0).min(l.window.w);\n"
        "        let card_h = (rows_h + l.title * 2.2 + l.pad).min(l.window.h);",
        "        let card_w = 360.0;\n        let card_h = 280.0;",
        ["the_help_card_is_drawn_inside_the_window_that_needs_it"],
    ),
    (
        "a close request does not close the window",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        {}",
        ["a_close_request_closes_the_window"],
    ),
    (
        "a move does not ask for a redraw",
        "            EventResult::Consumed => Response::Redraw,",
        "            EventResult::Consumed => Response::Idle,",
        ["a_move_asks_for_a_redraw_and_a_dead_key_does_not"],
    ),
    (
        "render does not record the window it was given",
        "        self.size = (width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["render_records_the_window_it_was_given"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "crossword", timeout=300, only=only))

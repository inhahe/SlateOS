"""Mutation test for the Game of Life suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

This app is the sharpest case in the tree for doing it, because the program it
replaces was green, warning-free and *could not run a single generation*: both
key handlers destructured `KeyEvent { key, .. }` and so ran every key on the
press and again on the release, which meant Space started and stopped the
simulation between one frame and the next.  Nothing caught that, because there
was no window to deliver a key and no test that sent one.  The suite this
script measures sends whole keystrokes -- down, then up -- for exactly that
reason, and the `swallowed pressed` mutations below put the fault back to prove
the suite would now notice.

Usage:  python apps/life/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The rules of Life ─────────────────────────────────────────────────
    (
        "a live cell with two neighbours dies",
        "                let alive = if self.get(row, col) {\n                    n == 2 || n == 3",
        "                let alive = if self.get(row, col) {\n                    n == 3",
        ["birth_and_survival_follow_b3_s23_for_every_neighbour_count"],
    ),
    (
        "a dead cell is born on two neighbours as well as three",
        "                } else {\n                    n == 3\n                };\n                next.set(row, col, alive);",
        "                } else {\n                    n == 2 || n == 3\n                };\n                next.set(row, col, alive);",
        ["birth_and_survival_follow_b3_s23_for_every_neighbour_count"],
    ),
    (
        "a cell counts itself among its neighbours",
        "                if dr == 0 && dc == 0 {\n                    continue;\n                }",
        "                if false {\n                    continue;\n                }",
        ["a_cell_never_counts_itself_as_its_own_neighbour"],
    ),
    (
        # `rem_euclid` is what wraps the board.  `%` agrees with it everywhere
        # except at the left and top edges, where -1 % 80 is -1 and not 79 --
        # so the wrap is lost on two sides only, which is precisely the kind of
        # half-fault a test at the middle of the board never sees.
        "the neighbour count does not wrap at the left edge",
        "                let nc = c.saturating_add(dc).rem_euclid(cols);",
        "                let nc = (c.saturating_add(dc) + cols) % (cols + 1);",
        ["the_board_wraps_at_every_edge"],
    ),
    (
        "the neighbour count does not wrap at the top edge",
        "                let nr = r.saturating_add(dr).rem_euclid(rows);",
        "                let nr = (r.saturating_add(dr) + rows) % (rows + 1);",
        ["the_board_wraps_at_every_edge"],
    ),
    (
        "a generation is stepped but not counted",
        "        self.generation = self.generation.saturating_add(1);",
        "        self.generation = self.generation;",
        ["the_header_says_what_the_board_actually_is"],
    ),
    # ── Index and coordinate ──────────────────────────────────────────────
    (
        "an index is built from the wrong dimension",
        "        row.checked_mul(self.cols)?.checked_add(col)",
        "        row.checked_mul(self.rows)?.checked_add(col)",
        ["an_index_and_a_coordinate_pair_say_the_same_thing"],
    ),
    (
        "row and column are swapped coming back out of an index",
        "        Some((index.checked_div(self.cols)?, index.checked_rem(self.cols)?))",
        "        Some((index.checked_rem(self.cols)?, index.checked_div(self.cols)?))",
        ["an_index_and_a_coordinate_pair_say_the_same_thing"],
    ),
    (
        "a cell off the right of the board is an index into the next row",
        "        if row >= self.rows || col >= self.cols {\n            return None;\n        }",
        "        if row >= self.rows {\n            return None;\n        }",
        ["an_index_and_a_coordinate_pair_say_the_same_thing"],
    ),
    # ── The keyboard: press and release ───────────────────────────────────
    (
        # Fault one, restored: the exact line whose absence made the program
        # unable to advance one generation.
        "the key handler swallows `pressed` and runs everything twice",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        [
            "space_runs_and_pauses_once_per_whole_keystroke",
            "a_key_release_on_its_own_changes_nothing",
        ],
    ),
    (
        # Caught by the release-alone test, and NOT by the whole-keystroke one,
        # which is worth writing down: `stroke()` sends a press and a release,
        # so a program that acts on the release instead of the press produces
        # the identical net result and no test built on `stroke` can tell the
        # two apart.  Only a test that sends half a keystroke can.  That is the
        # entire reason `a_key_release_on_its_own_changes_nothing` exists, and
        # this mutant is the proof that it is not redundant with the others.
        "the key handler answers releases and ignores presses",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }",
        "        if ev.pressed {\n            return EventResult::Ignored;\n        }",
        ["a_key_release_on_its_own_changes_nothing"],
    ),
    (
        "a held Ctrl no longer refuses the key",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {",
        "        if ev.modifiers.alt || ev.modifiers.super_key {",
        ["a_modifier_held_down_refuses_the_key_in_both_views"],
    ),
    (
        "a held Shift refuses the key, so `+` cannot be typed",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key || ev.modifiers.shift {",
        ["shift_is_not_a_modifier_that_refuses_a_key"],
    ),
    # ── What the keys mean ────────────────────────────────────────────────
    (
        # Survived the first sweep.  Every test that checked the help sheet
        # blocked the board went through the *pointer*, and the clock test could
        # not see it either, because `clock_running()` carries its own separate
        # `!show_help` guard — so the board kept still for a reason that had
        # nothing to do with the key handler.  Nothing had ever pressed a board
        # key with the sheet up.  A guard that is duplicated is a guard whose
        # copies must each be tested; the second one masks the first.
        "the help sheet lets the board's keys through underneath it",
        "        if self.show_help {\n            return match key {",
        "        if false {\n            return match key {",
        ["while_the_help_sheet_is_up_the_board_keys_do_nothing"],
    ),
    (
        "S steps but does not pause first",
        "                self.running = false;\n                self.step();\n                self.tick_accum = 0;",
        "                self.step();\n                self.tick_accum = 0;",
        ["a_step_while_running_pauses_first_and_then_takes_one"],
    ),
    (
        "the arrow keys move the cursor two cells",
        "                Key::Up => Some(Action::MoveCursor(-1, 0)),",
        "                Key::Up => Some(Action::MoveCursor(-2, 0)),",
        ["each_arrow_key_moves_the_cursor_exactly_one_cell"],
    ),
    (
        "left and right are swapped",
        "                Key::Left => Some(Action::MoveCursor(0, -1)),\n                Key::Right => Some(Action::MoveCursor(0, 1)),",
        "                Key::Left => Some(Action::MoveCursor(0, 1)),\n                Key::Right => Some(Action::MoveCursor(0, -1)),",
        ["each_arrow_key_moves_the_cursor_exactly_one_cell"],
    ),
    (
        "the cursor stops at the edge rather than wrapping as the board does",
        "        let nr = r.saturating_add(dr).rem_euclid(rows);\n        let nc = c.saturating_add(dc).rem_euclid(cols);",
        "        let nr = r.saturating_add(dr).clamp(0, rows - 1);\n        let nc = c.saturating_add(dc).clamp(0, cols - 1);",
        ["the_cursor_wraps_at_every_edge_as_the_board_does"],
    ),
    (
        "a number key sets a speed one off the one it names",
        "                Key::Num3 => Some(Action::SetSpeed(3)),",
        "                Key::Num3 => Some(Action::SetSpeed(4)),",
        ["every_number_key_sets_its_own_speed"],
    ),
    (
        "minus and plus are swapped",
        "                Key::Minus => Some(Action::NudgeSpeed(-1)),\n                Key::Equals => Some(Action::NudgeSpeed(1)),",
        "                Key::Minus => Some(Action::NudgeSpeed(1)),\n                Key::Equals => Some(Action::NudgeSpeed(-1)),",
        ["minus_and_plus_move_the_speed_by_one_and_stop_at_the_ends"],
    ),
    (
        "the speed runs off the bottom of its range",
        "                let next = here.saturating_add(delta).clamp(1, 9);",
        "                let next = here.saturating_add(delta).clamp(0, 9);",
        ["minus_and_plus_move_the_speed_by_one_and_stop_at_the_ends"],
    ),
    (
        # Survived the first sweep, because no keystroke can reach it: only
        # `Num1`..`Num9` produce a `SetSpeed`, and those nine are already in
        # range, so a test driven through the keyboard can never tell whether
        # the clamp is there.  It is not an equivalent mutant, though —
        # `Action` and `apply` are public, and `speed_ms()`'s catch-all arm
        # returns 15ms, so an unclamped speed reads silently as the *fastest*
        # rather than as an error.  The test drives `apply` directly.
        "a speed set out of range is taken at its word",
        "            Action::SetSpeed(s) => self.speed = s.clamp(1, 9),",
        "            Action::SetSpeed(s) => self.speed = s,",
        ["a_speed_is_never_set_outside_the_range_the_speeds_run_in"],
    ),
    (
        "a faster speed is a longer interval",
        "            7 => 50,\n            8 => 30,",
        "            7 => 30,\n            8 => 50,",
        ["a_higher_speed_is_a_shorter_interval"],
    ),
    (
        "G is not answered, so the help sheet promises a key that does nothing",
        "                Key::G => Some(Action::ToggleGridLines),",
        "                Key::Q => Some(Action::ToggleGridLines),",
        ["the_help_sheet_names_every_key_the_program_answers"],
    ),
    (
        "a key the program does not answer is consumed anyway",
        "            None => EventResult::Ignored,",
        "            None => EventResult::Consumed,",
        ["a_key_the_program_does_not_answer_is_left_alone"],
    ),
    # ── The clock ─────────────────────────────────────────────────────────
    (
        # Fault two, restored in the only way that still compiles: the app keeps
        # perfect time and is never asked for an interval, so no tick arrives.
        "the window is never told to run a clock",
        "        self.clock_running()\n            .then(|| Duration::from_millis(self.speed_ms()))",
        "        None",
        ["the_clock_is_asked_for_exactly_when_it_is_wanted"],
    ),
    (
        # NOT caught by `opening_the_sheet_stops_the_board_running`, which is
        # the sort of near-miss worth recording: opening the pattern sheet also
        # sets `running = false`, so the mutant's `self.running` is false anyway
        # and the test passes for the wrong reason.  What catches it is the help
        # sheet, which is opened *without* stopping the run.
        "the clock keeps running with a modal sheet over the board",
        "        self.running && matches!(self.view, View::Board) && !self.show_help",
        "        self.running",
        [
            "the_clock_is_asked_for_exactly_when_it_is_wanted",
            "a_tick_while_the_board_is_not_running_does_nothing",
        ],
    ),
    (
        "the interval asked for is not the interval the speed names",
        "            .then(|| Duration::from_millis(self.speed_ms()))",
        "            .then(|| Duration::from_millis(self.speed_ms() / 2))",
        ["the_interval_asked_for_is_the_one_the_speed_names"],
    ),
    (
        "a tick advances by one interval however long really passed",
        "        self.tick_accum = self.tick_accum.saturating_add(elapsed_ms);",
        "        self.tick_accum = self.tick_accum.saturating_add(self.speed_ms());",
        ["a_tick_advances_by_the_time_that_passed_not_by_the_interval"],
    ),
    (
        "time shorter than an interval is thrown away rather than banked",
        "        self.tick_accum = self.tick_accum.saturating_add(elapsed_ms);\n        let interval",
        "        self.tick_accum = elapsed_ms;\n        let interval",
        ["a_tick_shorter_than_the_interval_banks_the_time_rather_than_losing_it"],
    ),
    (
        # Fault three, restored.  Unbounded catch-up: a window that stalls for
        # ten seconds at speed 9 asks for 666 sweeps of 4800 cells in the frame
        # it comes back, which stalls it again.
        "the catch-up loop is unbounded",
        "            if taken >= MAX_CATCHUP {",
        "            if false {",
        ["catching_up_is_capped_and_the_backlog_is_dropped_not_banked"],
    ),
    (
        "the dropped backlog is banked and paid out on the next tick",
        "                self.tick_accum = self.tick_accum.checked_rem(interval).unwrap_or(0);",
        "                self.tick_accum = self.tick_accum;",
        ["catching_up_is_capped_and_the_backlog_is_dropped_not_banked"],
    ),
    (
        "a tick runs generations on a paused board",
        "        if !self.clock_running() {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_tick_while_the_board_is_not_running_does_nothing"],
    ),
    (
        "pausing keeps the part-generation that had elapsed",
        "                self.running = !self.running;\n                // The accumulator belongs to the run that is starting, not to\n                // the one that stopped: keeping it would make the first\n                // generation after a pause arrive early by however long the\n                // pause interrupted.\n                self.tick_accum = 0;",
        "                self.running = !self.running;",
        ["pausing_forgets_the_part_of_a_generation_that_had_elapsed"],
    ),
    (
        # NOT caught by `a_tick_while_the_board_is_not_running_does_nothing`:
        # that test's tick returns `Ignored` at the `clock_running()` guard and
        # never reaches the `taken == 0` line at all.  The tick that does reach
        # it is the running one that was too short to buy a generation.
        "a tick that ran nothing claims the frame changed",
        "        if taken == 0 {",
        "        if false {",
        ["a_tick_shorter_than_the_interval_banks_the_time_rather_than_losing_it"],
    ),
    # ── The layout ────────────────────────────────────────────────────────
    (
        # NOT caught by `cells_are_square_…`, which reads oddly until you look
        # at `cell_rect`: it uses the single `cell` for both the width and the
        # height, so a cell sized off the width alone is still perfectly square.
        # It merely no longer fits the band.  The squareness test is blind to
        # this by construction; what catches it is the containment tests.
        "cells are stretched to the band instead of being square",
        "            (band.w / cols as f32).min(band.h / rows as f32).max(0.0)",
        "            (band.w / cols as f32).max(0.0)",
        [
            "the_whole_board_is_on_screen_at_every_window_size",
            "nothing_is_drawn_outside_the_window",
            "the_board_never_overlaps_the_bands_above_and_below_it",
        ],
    ),
    (
        "the board is placed without regard to the bands above it",
        "        let top = hdr_h;",
        "        let top = 0.0;",
        ["the_board_never_overlaps_the_bands_above_and_below_it"],
    ),
    (
        "the board runs under the controls",
        "        let bottom = if ctl_h > 0.0 { h - ctl_h } else { h };",
        "        let bottom = h;",
        ["the_board_never_overlaps_the_bands_above_and_below_it"],
    ),
    (
        # Fault four, restored: the mapping from a cell to its pixels, made a
        # constant again.  The drawing and the hit boxes come from this one
        # function, so a wrong one is wrong in both -- which is why the test
        # that catches it clicks the pixel the frame actually filled rather
        # than asking the layout where the cell is.
        # These next two are the campaign's sharpest lesson about agreement
        # tests, so both carry the same note.  `the_pixels_a_cell_is_drawn_on_
        # are_the_pixels_that_click_it` does not catch either of them, and it
        # cannot: the drawing pass and the hit boxes are both computed from the
        # one `cell_rect`, so breaking `cell_rect` moves both sides together and
        # they go on agreeing perfectly.  An agreement test can only catch a
        # disagreement between two mappings; it is structurally blind to a fault
        # in a single mapping that both sides share.  What catches these is the
        # geometry: a swapped or displaced grid leaves the window.
        "a cell's pixels are found with the columns and rows swapped",
        "            self.board.x + col as f32 * self.cell,\n            self.board.y + row as f32 * self.cell,",
        "            self.board.x + row as f32 * self.cell,\n            self.board.y + col as f32 * self.cell,",
        [
            "nothing_is_drawn_outside_the_window",
            "the_whole_board_is_on_screen_at_every_window_size",
        ],
    ),
    (
        # See the note above: an agreement test is blind to a fault in the one
        # mapping both of its sides are built from.
        "the board's origin is a constant rather than the window's",
        "            self.board.x + col as f32 * self.cell,",
        "            44.0 + col as f32 * self.cell,",
        [
            "nothing_is_drawn_outside_the_window",
            "the_whole_board_is_on_screen_at_every_window_size",
        ],
    ),
    (
        "the header is dropped before the controls",
        "const BAND_DROP_ORDER: [usize; 2] = [1, 0];",
        "const BAND_DROP_ORDER: [usize; 2] = [0, 1];",
        ["a_window_too_short_for_everything_drops_the_controls_before_the_header"],
    ),
    (
        "a dropped band is a full-width strip of no height",
        "        let controls = if ctl_h > 0.0 {\n            Rect::new(0.0, h - ctl_h, w, ctl_h)\n        } else {\n            Rect::EMPTY\n        };",
        "        let controls = Rect::new(0.0, h - ctl_h, w, ctl_h);",
        ["a_dropped_band_is_empty_rather_than_a_strip_of_no_height"],
    ),
    (
        # Fault two of the layout family: the margin floor that put the board's
        # own origin outside a tiny window.
        "the margin may be larger than the window it is taken from",
        "        let pad = (w.min(h) * 0.015).clamp(2.0, 10.0).min(w.min(h) / 4.0);",
        "        let pad = (w.min(h) * 0.015).clamp(2.0, 10.0);",
        ["the_whole_board_is_on_screen_at_every_window_size"],
    ),
    (
        "the layout is taken from the window it was built at, not the one asked for",
        "        let l = Layout::new(width, height, self.grid.cols(), self.grid.rows());",
        "        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT, self.grid.cols(), self.grid.rows());",
        [
            "the_layout_is_read_from_the_window_and_not_remembered",
            "the_first_frame_is_drawn_at_the_size_it_is_given",
        ],
    ),
    # ── Drawing ───────────────────────────────────────────────────────────
    (
        "a centred string is given the whole box to run into from where it starts",
        "        Some((r.right() - x).max(0.0)),",
        "        Some(r.w),",
        ["every_string_is_bounded_by_the_space_it_has"],
    ),
    (
        # Survived the first sweep against a test that looked like it covered
        # it and did not.  `every_string_is_bounded_by_the_space_it_has` bounds
        # each string by the *window*, and the max_width is `r.right() - x`:
        # as `x` shrinks the width grows by exactly as much, so `x + max_width`
        # never moves and the window-level check cannot see the box-level
        # overflow at all.  Catching it needs a check against the control's own
        # box — at 320x240 the "Patterns" button is ~33px wide and its label
        # wants ~38px, so the unclamped start puts the text outside its button.
        "a string too long to centre starts left of its box",
        "    let x = (r.x + (r.w - w) / 2.0).max(r.x);",
        "    let x = r.x + (r.w - w) / 2.0;",
        ["a_label_stays_inside_the_control_it_names"],
    ),
    (
        "a string is drawn with no width to fit in",
        "        max_width,\n        overflow: TextOverflow::Ellipsis,",
        "        max_width: None,\n        overflow: TextOverflow::Ellipsis,",
        ["every_string_is_bounded_by_the_space_it_has"],
    ),
    (
        "a dead cell is drawn as a live one",
        "                if alive {\n                    let color = if is_cursor { LAVENDER } else { GREEN };",
        "                if true {\n                    let color = if is_cursor { LAVENDER } else { GREEN };",
        ["a_live_cell_is_drawn_and_a_dead_one_is_not"],
    ),
    (
        "a live cell is not drawn at all",
        "                if alive {\n                    let color",
        "                if false {\n                    let color",
        ["a_live_cell_is_drawn_and_a_dead_one_is_not"],
    ),
    (
        "the cursor is not shown on a cell that is already alive",
        "                } else if is_cursor {",
        "                } else if is_cursor && false {",
        ["the_cursor_is_drawn_whether_its_cell_is_alive_or_not"],
    ),
    (
        "the grid lines are drawn whether they are switched on or not",
        "                if self.show_grid && l.cell >= 4.0 {",
        "                if l.cell >= 4.0 {",
        ["the_grid_lines_go_away_when_they_are_switched_off"],
    ),
    (
        "grid lines are drawn on cells too small to have one",
        "                if self.show_grid && l.cell >= 4.0 {",
        "                if self.show_grid {",
        ["grid_lines_are_not_drawn_when_a_cell_is_too_small_to_have_one"],
    ),
    (
        "the header says a generation count of its own",
        '        self.generation = self.generation.saturating_add(1);\n    }',
        '        self.generation = self.generation.saturating_add(2);\n    }',
        ["the_header_says_what_the_board_actually_is"],
    ),
    # ── The pointer ───────────────────────────────────────────────────────
    (
        # Fault six, restored: nothing is recorded for a cell, so the board
        # cannot be clicked at all.
        "no hit box is recorded for a cell",
        "                if let Some(i) = self.grid.index(row, col) {\n                    f.hit(Target::Cell(i), r);\n                }",
        "                let _ = row;",
        [
            "every_cell_is_clickable_where_it_is_drawn",
            "a_click_flips_the_cell_it_lands_on_and_takes_the_cursor_there",
        ],
    ),
    (
        "a cell is only clickable while it is alive",
        "                if let Some(i) = self.grid.index(row, col) {",
        "                if let Some(i) = self.grid.index(row, col).filter(|_| alive) {",
        ["every_cell_is_clickable_where_it_is_drawn"],
    ),
    (
        "a click flips a cell but leaves the cursor where it was",
        "                    self.grid.toggle(row, col);\n                    self.cursor_row = row;\n                    self.cursor_col = col;",
        "                    self.grid.toggle(row, col);",
        ["a_click_flips_the_cell_it_lands_on_and_takes_the_cursor_there"],
    ),
    (
        "a right press does what a left one does",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if !matches!(ev.kind, MouseEventKind::Press(_)) {",
        ["only_a_left_press_does_anything"],
    ),
    (
        "a release does what a press does",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["only_a_left_press_does_anything"],
    ),
    (
        "a click on nothing is consumed anyway",
        "        let Some(target) = self.target_at(ev.x, ev.y) else {\n            return EventResult::Ignored;\n        };",
        "        let Some(target) = self.target_at(ev.x, ev.y) else {\n            return EventResult::Consumed;\n        };",
        ["a_click_outside_everything_is_ignored"],
    ),
    (
        "the Step button plays instead of stepping",
        "            Target::StepOnce => Action::StepOnce,",
        "            Target::StepOnce => Action::TogglePlay,",
        ["every_button_does_what_its_key_does"],
    ),
    (
        "the Clear button randomises",
        "            Target::Clear => Action::Clear,",
        "            Target::Clear => Action::Randomize,",
        ["every_button_does_what_its_key_does"],
    ),
    (
        "the slower and faster buttons are swapped",
        "            Target::Slower => Action::NudgeSpeed(-1),\n            Target::Faster => Action::NudgeSpeed(1),",
        "            Target::Slower => Action::NudgeSpeed(1),\n            Target::Faster => Action::NudgeSpeed(-1),",
        ["every_button_does_what_its_key_does"],
    ),
    (
        "the controls overlap one another",
        "                l.controls.x + l.pad + (each + gap) * i as f32,",
        "                l.controls.x + l.pad + (each * 0.6 + gap) * i as f32,",
        ["the_buttons_are_side_by_side_and_do_not_overlap"],
    ),
    # ── The modal sheets ──────────────────────────────────────────────────
    (
        # Fault seven, restored: the backdrop is recorded *after* the sheet, so
        # it wins and the sheet's own rows become unclickable.
        "the modal backdrop is recorded over the sheet it is behind",
        "        fill(f, l.window, Color::rgba(0, 0, 0, 180), 0.0);\n        // First, so that every box recorded below it wins. `hit_test` takes the\n        // last box at a point, which is what makes a modal backdrop and the\n        // things on top of it both work with no special case in the handler.\n        f.hit(Target::ClosePatterns, l.window);",
        "        fill(f, l.window, Color::rgba(0, 0, 0, 180), 0.0);",
        [
            "a_click_anywhere_off_the_sheet_cancels_it",
            "while_the_sheet_is_up_no_click_reaches_the_board_beneath_it",
        ],
    ),
    (
        "the help sheet does not block the board underneath it",
        '        fill(f, l.window, Color::rgba(0, 0, 0, 190), 0.0);\n        // The whole window closes it: there is nothing on the sheet to press.\n        f.hit(Target::CloseHelp, l.window);',
        '        fill(f, l.window, Color::rgba(0, 0, 0, 190), 0.0);',
        ["while_the_help_sheet_is_up_a_click_anywhere_only_closes_it"],
    ),
    (
        "the sheet is drawn under the help sheet rather than over it",
        "        if self.show_help {\n            self.draw_help(&mut f, &l);\n        }",
        "        if self.show_help && false {\n            self.draw_help(&mut f, &l);\n        }",
        ["the_help_sheet_covers_the_pattern_sheet_and_not_the_other_way_round"],
    ),
    (
        "a pattern row selects the row after it",
        "            Target::PatternRow(i) => Action::SelectPattern(i),",
        "            Target::PatternRow(i) => Action::SelectPattern(i + 1),",
        ["every_pattern_the_menu_lists_can_be_picked_with_the_pointer"],
    ),
    (
        "picking a pattern closes the sheet without placing it",
        "            Target::PatternRow(i) => Action::SelectPattern(i),",
        "            Target::PatternRow(_) => Action::ClosePatterns,",
        ["every_pattern_the_menu_lists_can_be_picked_with_the_pointer"],
    ),
    (
        "the sheet offers one fewer pattern than there are",
        "        for (i, pattern) in Pattern::ALL.iter().enumerate() {",
        "        for (i, pattern) in Pattern::ALL.iter().enumerate().take(Pattern::ALL.len() - 1) {",
        ["every_pattern_the_menu_lists_can_be_picked_with_the_pointer"],
    ),
    (
        "Cancel places the pattern instead of dismissing it",
        "        f.hit(Target::ClosePatterns, cancel);",
        "        f.hit(Target::PlacePattern, cancel);",
        ["cancelling_the_sheet_leaves_the_board_exactly_as_it_was"],
    ),
    (
        "Place is not clickable",
        "        f.hit(Target::PlacePattern, place);",
        "        let _ = place;",
        ["the_place_button_stamps_the_pattern_the_sheet_has_selected"],
    ),
    (
        "opening the sheet leaves the board running underneath it",
        "                self.view = View::PatternMenu;\n                self.running = false;",
        "                self.view = View::PatternMenu;",
        ["opening_the_sheet_stops_the_board_running"],
    ),
    (
        "placing a pattern leaves the sheet up",
        "                self.grid\n                    .place(self.pattern(), self.cursor_row, self.cursor_col);\n                self.view = View::Board;",
        "                self.grid\n                    .place(self.pattern(), self.cursor_row, self.cursor_col);",
        ["placing_a_pattern_does_not_flip_one_of_its_own_cells_back_off"],
    ),
    (
        "the selection runs off the end of the list",
        "                let next = here.saturating_add(delta).clamp(0, last_i);",
        "                let next = here.saturating_add(delta);",
        ["the_pattern_menu_selection_moves_one_row_and_stops_at_the_ends"],
    ),
    # ── The patterns themselves ───────────────────────────────────────────
    (
        # The fault this suite found while being written: the Pulsar had six of
        # its twelve quarter-cells on the mirror lines, so it drew 36 cells
        # under the name of a 48-cell figure and was not an oscillator at all.
        "the pulsar's quarter sits on the mirror lines",
        "                    (0, 2),\n                    (0, 3),\n                    (0, 4),",
        "                    (0, 2),\n                    (0, 3),\n                    (6, 4),",
        [
            "the_pulsar_is_symmetric_about_its_middle",
            "every_pattern_named_for_an_oscillator_oscillates_at_its_own_period",
        ],
    ),
    (
        "a pattern is stamped one cell off from the cursor",
        "                    .place(self.pattern(), self.cursor_row, self.cursor_col);",
        "                    .place(self.pattern(), self.cursor_row + 1, self.cursor_col);",
        ["placing_a_pattern_does_not_flip_one_of_its_own_cells_back_off"],
    ),
    # ── Clearing and the soup ─────────────────────────────────────────────
    (
        "Clear leaves the generation count where it was",
        "            Action::Clear => {\n                self.grid.clear();\n                self.generation = 0;",
        "            Action::Clear => {\n                self.grid.clear();",
        ["clearing_empties_the_board_and_starts_the_count_again"],
    ),
    (
        "the soup is twice as dense as it says",
        "                self.grid.randomize(&mut self.rng, 25);",
        "                self.grid.randomize(&mut self.rng, 50);",
        ["a_random_soup_fills_about_the_quarter_of_the_board_it_promises"],
    ),
    (
        "the soup only ever turns cells on",
        "            if let Some(cell) = self.cells.get_mut(i) {\n                *cell = alive;\n            }",
        "            if let Some(cell) = self.cells.get_mut(i) {\n                *cell = *cell || alive;\n            }",
        ["a_soup_is_a_new_board_and_not_a_layer_over_the_old_one"],
    ),
    (
        "the soup does not start the generation count again",
        "                self.grid.randomize(&mut self.rng, 25);\n                self.generation = 0;\n                self.tick_accum = 0;",
        "                self.grid.randomize(&mut self.rng, 25);",
        ["a_soup_starts_the_generation_count_again_and_forgets_the_part_generation"],
    ),
    (
        "every seed draws the same soup",
        "    pub fn with_seed(seed: u64) -> Self {",
        "    pub fn with_seed(_ignored: u64) -> Self {\n        let seed = 1u64;",
        ["two_seeds_give_two_different_soups_and_one_seed_gives_the_same_one"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "life", timeout=240))

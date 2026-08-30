"""Mutation test for the maze suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program -- which is how this app shipped a key
handler that ran every key twice, a clock that never ticked and a solution
overlay with no reachable state in which it showed anything, all under a green
build.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The maze itself ───────────────────────────────────────────────────
    (
        # Fault six restored in the only place that can still hold it.  The old
        # BFS did `(r as i32 + dr) as usize` with no bounds test, so a north
        # step out of row 0 gave `usize::MAX` and `row * cols + col` turned that
        # into a *plausible* index into another cell rather than a panic.
        # Clamping the sum is the same shape of error with the same result: a
        # neighbour that is not there is reported as one that is.
        "a step off the top of the grid lands somewhere",
        "        let nr = i64::try_from(r).ok()?.checked_add(i64::from(dr))?;\n"
        "        let nc = i64::try_from(c).ok()?.checked_add(i64::from(dc))?;",
        "        let nr = i64::try_from(r).ok()?.checked_add(i64::from(dr))?.max(0);\n"
        "        let nc = i64::try_from(c).ok()?.checked_add(i64::from(dc))?.max(0);",
        ["a_neighbour_off_the_grid_is_no_neighbour"],
    ),
    (
        # The other edge.  Without the far check a step off the bottom row is a
        # coordinate one past the end, which every caller then has to catch.
        "a step off the bottom of the grid lands somewhere",
        "        if nr >= self.rows || nc >= self.cols {\n            return None;\n        }\n        Some((nr, nc))",
        "        Some((nr, nc))",
        ["a_neighbour_off_the_grid_is_no_neighbour"],
    ),
    (
        # A cell off the grid must read as walled on every side; that is what
        # makes the outer border solid without a special case anywhere else.
        "a cell off the grid is open on every side",
        "        let Some(i) = self.index(r, c) else {\n            return true;\n        };",
        "        let Some(i) = self.index(r, c) else {\n            return false;\n        };",
        ["a_neighbour_off_the_grid_is_no_neighbour"],
    ),
    (
        # The guard that fault six actually needed: a border wall bit that is
        # down must still not let the search walk off the grid, because being
        # in bounds is a property of the coordinates and not of the wall.
        "a wall that is down at the border is a way out of the grid",
        "        self.step(r, c, dir).is_some() && !self.has_wall(r, c, dir)",
        "        !self.has_wall(r, c, dir)",
        ["a_wall_bit_cleared_at_the_border_does_not_walk_the_search_off_the_grid"],
    ),
    (
        # A passage is one wall, recorded on both of the cells it joins.
        # Writing only the near side leaves a wall you can walk through one way
        # and not the other.
        "carving writes only the near side of the wall",
        "        if let Some(cell) = self.cells.get_mut(there) {\n            *cell &= !dir.opposite().bit();\n        }\n        true",
        "        true",
        [
            "a_wall_reads_the_same_from_both_of_the_cells_it_stands_between",
            "every_dealt_maze_is_perfect",
        ],
    ),
    (
        # Without the shuffle the backtracker takes the directions in a fixed
        # order, so every maze of a given size is the same maze and the seed
        # buys nothing.  The generated maze is still perfect, so only a test
        # that compares two seeds can see it.
        "the generator ignores the random source",
        "            rng.shuffle(&mut dirs);",
        "",
        ["the_same_seed_deals_the_same_maze_and_two_seeds_do_not"],
    ),
    (
        # The search must not number a cell twice.  Re-queuing a cell that
        # already has a distance bounces between neighbours forever: the queue
        # grows without bound and the binary dies on a two-gigabyte allocation
        # before any test can report a failure.  Caught by the harness dying,
        # which is why `run_tests` has to tell a crash from a clean pass.
        "the search renumbers cells it has already reached",
        "                if dist.get(i).copied().flatten().is_some() {\n                    continue;\n                }",
        "",
        [],
    ),
    (
        # `distances` must be breadth-first to be shortest-path.  Reading the
        # queue from the back turns it into a depth-first walk, whose numbers
        # are the length of the first route found and not the shortest.
        #
        # No maze the *generator* deals can tell the two apart: a perfect maze
        # is a tree, so there is exactly one route between any two cells and
        # every traversal order agrees.  It took a hand-carved ring to catch
        # this, and the first sweep let it survive for want of one.
        "the search reads its queue from the back",
        "        while let Some(&current) = queue.get(head) {\n            head = head.saturating_add(1);",
        "        while let Some(current) = queue.pop() {",
        ["the_distance_round_a_ring_is_the_shorter_way_round"],
    ),
    (
        # The route is walked back from the far end by stepping to whichever
        # neighbour is *one closer*.  Any other neighbour is a step sideways or
        # backwards and the walk never terminates at the start.
        "the route steps to any neighbour rather than a closer one",
        "                if dist.get(i).copied().flatten() == d.checked_sub(1) {",
        "                if dist.get(i).copied().flatten().is_some() {",
        ["the_route_out_is_a_run_of_open_steps_and_is_as_long_as_the_distance"],
    ),
    (
        # Every passage is counted from both of the cells it joins, so the bit
        # count is twice the number of passages.
        "passages are counted once per cell rather than once per wall",
        "        bits / 2",
        "        bits",
        ["a_perfect_maze_is_one_reachable_run_with_no_ring_in_it"],
    ),
    (
        # A tree is *both* connected and acyclic; either half alone passes a
        # maze that is wrong in the other direction.
        "a maze is perfect as soon as every cell can be reached",
        "        reached == n && self.passages() == n.saturating_sub(1)",
        "        reached == n",
        ["a_perfect_maze_is_one_reachable_run_with_no_ring_in_it"],
    ),
    (
        "a maze is perfect as soon as it has the right number of passages",
        "        reached == n && self.passages() == n.saturating_sub(1)",
        "        self.passages() == n.saturating_sub(1)",
        ["a_perfect_maze_is_one_reachable_run_with_no_ring_in_it"],
    ),
    # ── Play ──────────────────────────────────────────────────────────────
    (
        "a step through a wall is allowed",
        '        if !self.maze.open(r, c, dir) {\n            self.status = "A wall".to_string();\n            return;\n        }',
        "",
        ["walking_into_a_wall_moves_nothing_and_counts_nothing"],
    ),
    (
        "a step is not counted",
        "        self.moves = self.moves.saturating_add(1);",
        "",
        [
            "an_arrow_key_moves_one_cell_and_counts_one_move",
            "a_click_walks_to_the_cell_it_lands_on_by_the_shortest_route",
        ],
    ),
    (
        "a solved maze goes on being walked",
        '        if self.state == GameState::Won {\n            self.status = "You are already out — N for another".to_string();\n            return;\n        }\n        let (r, c) = self.player;',
        "        let (r, c) = self.player;",
        ["a_won_maze_ignores_further_steps"],
    ),
    (
        # A click whose route passes the exit walks on past it, so the win the
        # player just earned flashes by in a state no frame is ever drawn in.
        "a walk that passes the way out carries on",
        "            if self.state == GameState::Won {\n                return;\n            }",
        "",
        ["a_walk_that_passes_the_way_out_stops_there"],
    ),
    (
        "clicking where you stand counts a move",
        '        if dest == self.player {\n            self.status = "You are standing there".to_string();\n            return;\n        }',
        "",
        ["clicking_where_you_stand_says_so_rather_than_counting_a_move"],
    ),
    (
        # Fault eight.  One best shared by all three sizes made a 10x10's 18
        # moves a target a 30x30 could never meet, and the win box presented
        # the two as comparable.
        "one best is shared by all three sizes",
        "        if let Some(slot) = self.best.get_mut(self.level) {",
        "        if let Some(slot) = self.best.get_mut(0) {",
        ["the_best_is_kept_for_each_size_on_its_own"],
    ),
    (
        "a worse run overwrites the best",
        "            if slot.is_none_or(|prev| moves < prev) {\n                *slot = Some(moves);\n            }",
        "            *slot = Some(moves);",
        ["a_better_run_lowers_the_best_and_a_worse_one_leaves_it"],
    ),
    (
        # Fault seven.  The old program computed the optimum once from (0, 0)
        # and went on quoting it after twenty moves down a journey you were no
        # longer on.
        "the steps left are measured from the corner, not from the player",
        "        self.maze.steps_between(self.player, self.goal)",
        "        self.maze.steps_between((0, 0), self.goal)",
        ["the_steps_left_is_the_distance_to_the_goal_from_where_the_player_stands"],
    ),
    (
        "the way out is drawn from the corner, not from the player",
        "        self.maze.path(self.player, self.goal)",
        "        self.maze.path((0, 0), self.goal)",
        ["the_way_out_shown_starts_from_where_the_player_stands"],
    ),
    (
        "a fresh maze keeps the clock running from the last one",
        "        self.elapsed_ms = 0;",
        "",
        ["a_fresh_maze_starts_the_clock_and_the_count_again"],
    ),
    (
        "choosing the size you are already on deals another maze",
        '        if index == self.level {\n            self.status = format!("Already {}", self.level().name());\n            return;\n        }',
        "",
        ["choosing_the_size_you_are_already_on_says_so_rather_than_redealing"],
    ),
    # ── The clock ─────────────────────────────────────────────────────────
    (
        # Fault two, in its subtler form: a clock driven by the interval it
        # asked for rather than the time that passed runs slow by however much
        # the loop was busy, and runs slow silently.
        "the clock advances by the interval it asked for",
        "        self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        "        self.elapsed_ms = self.elapsed_ms.saturating_add(250);",
        ["the_clock_counts_the_time_that_passed_not_the_interval_it_asked_for"],
    ),
    (
        "the clock runs on after the maze is solved",
        "        if !self.clock_running() {\n            return EventResult::Ignored;\n        }",
        "",
        ["the_clock_runs_while_playing_and_stops_once_you_are_out"],
    ),
    (
        # A tick that always asks for a repaint wakes the compositor four times
        # a second to redraw a number that changes once.
        "every tick asks for a repaint",
        "        if self.elapsed_ms / 1000 == before {\n            EventResult::Ignored\n        } else {\n            EventResult::Consumed\n        }",
        "        EventResult::Consumed",
        ["a_tick_asks_for_a_frame_only_when_the_digits_change"],
    ),
    (
        # An app that leaves `tick_interval` at the default gets no ticks at
        # all: the clock reads zero for the life of the process, which is
        # exactly what this one did.  `known-issues.md` lesson 47.
        "the window is never asked for a tick",
        "        self.clock_running().then_some(TICK)",
        "        None",
        ["the_clock_runs_while_playing_and_stops_once_you_are_out"],
    ),
    # ── Keys ──────────────────────────────────────────────────────────────
    (
        # Fault one, restored.  It is one line, and it is the reason the
        # solution overlay had no reachable state in which it showed anything.
        "a key release is a second press again",
        "        if !ev.pressed {\n            return None;\n        }",
        "",
        [
            "a_key_that_comes_back_up_is_not_a_second_press",
            "the_solution_can_be_seen_after_a_whole_keystroke",
            "the_release_of_every_key_the_program_answers_does_nothing",
        ],
    ),
    (
        # Fault five.  The old program had two handlers and put this test in
        # only one of them, so Ctrl-N did nothing during play and dealt a fresh
        # maze on the win screen.
        "a modifier the program does not use is answered",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {\n            return None;\n        }",
        "",
        ["a_modifier_the_program_does_not_use_is_ignored"],
    ),
    (
        # A sheet you can play behind is a sheet in the way.
        "the open sheet does not swallow the keys behind it",
        "        if self.show_help {\n"
        "            return match ev.key {\n"
        "                Key::H | Key::Escape => Some(Action::CloseHelp),\n"
        "                _ => None,\n"
        "            };\n"
        "        }",
        "",
        ["the_open_sheet_swallows_the_keys_that_are_not_about_it"],
    ),
    (
        # The size cycle steps by two, which on three sizes is the order
        # reversed -- which is precisely what the double-fired `D` key did:
        # Small, Large, Medium.
        "the size key walks the sizes backwards",
        "                self.level\n                    .saturating_add(1)",
        "                self.level\n                    .saturating_add(2)",
        ["the_size_key_walks_the_sizes_in_the_order_the_buttons_name_them"],
    ),
    # ── Pointer ───────────────────────────────────────────────────────────
    (
        # The middle button is not a control.  Answering it swallows a click
        # the window manager may want.
        "every mouse button walks the player",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if !matches!(ev.kind, MouseEventKind::Press(_)) {",
        ["a_button_the_program_does_not_use_walks_nowhere"],
    ),
    (
        # Fault three.  A click read against a size the window is not is a
        # click that lands on whatever used to be there.
        "a click is read against a fixed window size",
        "        self.frame(self.size_drawn.0, self.size_drawn.1)\n            .hit_test(x, y)",
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).hit_test(x, y)",
        [
            "a_click_is_read_against_the_size_last_drawn",
            "rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against",
        ],
    ),
    (
        "rendering does not record the size it drew at",
        "        self.size_drawn = (width.max(1.0), height.max(1.0));",
        "",
        [
            "a_click_is_read_against_the_size_last_drawn",
            "rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against",
        ],
    ),
    # ── Layout ────────────────────────────────────────────────────────────
    (
        # Not `side = band.h`: that still assigns one number to both the width
        # and the height, so the maze comes out square anyway and the mutant
        # survives for a reason that has nothing to do with the test.  The
        # board has to actually take the band's shape.
        "the maze is not squared",
        "        let side = band.w.min(band.h).max(0.0);\n"
        "        let board = Rect::new(\n"
        "            band.x + (band.w - side) / 2.0,\n"
        "            band.y + (band.h - side) / 2.0,\n"
        "            side,\n"
        "            side,\n"
        "        );",
        "        let board = band;",
        ["the_maze_is_square_in_every_window"],
    ),
    (
        "band drop order reversed",
        "const BAND_DROP_ORDER: [usize; 3] = [0, 2, 1];",
        "const BAND_DROP_ORDER: [usize; 3] = [1, 2, 0];",
        ["the_bands_go_in_the_stated_order"],
    ),
    (
        # Reserving *less* than the share does not break this: the test asserts
        # a lower bound, and a smaller reservation cannot violate a lower bound
        # in a window roomy enough to satisfy it anyway.  Removing the
        # reservation is what starves the maze, because then the chrome only
        # drops a band once it has eaten the whole window.
        "the maze's share of the window is not reserved",
        "        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);",
        "        let budget = h;",
        [
            "the_maze_keeps_its_share_of_every_window",
            "the_maze_is_still_playable_in_a_window_too_small_for_the_chrome",
        ],
    ),
    # NOT a mutation: `let top = hdr_h + inf_h;` -> `let top = info.bottom();`.
    # It looks like one -- a dropped band is `Rect::EMPTY`, whose bottom is
    # zero, which would put the maze over the header -- but `BAND_DROP_ORDER`
    # drops the header before the info line, so the info line is only ever
    # dropped once the header already is, and both forms then give zero.  The
    # mutant is equivalent and no test can catch it.  Written the safe way in
    # the source regardless, with the reasoning at the line, because the
    # equivalence is a property of a constant somebody will reorder.
    # ── Drawing and hit boxes ─────────────────────────────────────────────
    (
        "cells record no hit box",
        "                    f.hit(Target::Cell(i), rect);",
        "",
        [
            "a_cell_is_clickable_exactly_where_it_is_drawn",
            "a_click_walks_to_the_cell_it_lands_on_by_the_shortest_route",
            "the_maze_is_still_playable_in_a_window_too_small_for_the_chrome",
        ],
    ),
    (
        "size buttons record no hit box",
        "            f.hit(Target::Level(slot), r);",
        "",
        ["a_size_button_switches_to_the_size_it_names"],
    ),
    (
        # The version this replaced.  It reads sensibly -- cover the sheet you
        # drew -- and it is wrong: the sheet is opaque but smaller than the
        # window, so the controls it does not physically cover go on answering
        # clicks whose targets the player cannot see.
        "the open sheet covers only its own pixels, not the window",
        "        f.hit(Target::ToggleHelp, l.window);",
        "        f.hit(Target::ToggleHelp, sheet);",
        [
            "while_the_sheet_is_up_the_frame_says_every_point_belongs_to_the_sheet",
            "a_click_while_the_sheet_is_open_closes_it_and_reaches_nothing_behind_it",
        ],
    ),
    (
        # The win notice is a notice, not a sheet.  Giving it a hit box puts a
        # control the player never asked for over the middle of the maze.
        "the win notice swallows the clicks under it",
        "        fill(f, plate, CRUST, (h * 0.12).min(12.0));",
        "        fill(f, plate, CRUST, (h * 0.12).min(12.0));\n        f.hit(Target::NewMaze, plate);",
        ["the_win_notice_covers_nothing_a_click_needs"],
    ),
    (
        "the info line does not say the clock",
        '            "{}  \\u{2022}  {} moves  \\u{2022}  {}  \\u{2022}  {best}  \\u{2022}  out {}",\n            self.clock(),',
        '            "{}  \\u{2022}  {} moves  \\u{2022}  {}  \\u{2022}  {best}  \\u{2022}  out {}",\n            "",',
        ["the_info_line_says_the_clock_the_moves_and_the_steps_left"],
    ),
    # ── Window ────────────────────────────────────────────────────────────
    (
        "a close request is ignored",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "",
        ["a_close_request_ends_the_program"],
    ),
    (
        "the window has no name",
        '        "Maze".to_string()',
        '        String::new()',
        ["the_window_names_itself"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "maze", timeout=180))

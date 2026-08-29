"""Mutation test for the pipes suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program -- which is exactly how this app shipped a
generator that had never once dealt a solvable board while its old suite stayed
green.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        # The original bug, restored.  `dir_between(r, c, pr, pc)` points from
        # this cell back to the previous one, which is the opening this cell
        # needs.  Reversing the arguments gives the direction the walk arrived
        # *by*, which points away -- so a straight run asks for the same side
        # twice and is fitted a corner.  Every board it laid was unsolvable.
        # Note which test is NOT in this list: `every_puzzle_dealt_can_be_solved`
        # passes against the restored bug.  A board whose laid solution is
        # nonsense is still usually *solvable*, because the random fill leaves
        # enough pipe lying about that some rotation of it joins the two ends
        # by accident.  That is precisely how this bug survived 80-odd tests --
        # solvability is the property everyone thinks to check, and it is the
        # one the bug does not break.  Only asking whether the laid solution
        # itself connects catches it.
        "the laid path opens away from the cell before it",
        "&& let Some(back) = Self::dir_between(r, c, pr, pc)",
        "&& let Some(back) = Self::dir_between(pr, pc, r, c)",
        [
            "the_solution_the_generator_lays_actually_joins_the_source_to_the_drain",
            "a_straight_run_of_the_walk_is_laid_as_straight_pipe",
            "a_turn_in_the_walk_is_laid_as_a_corner_facing_both_neighbours",
        ],
    ),
    (
        # The other half of the same fault: a cell with no opening toward the
        # cell the walk goes on to is a dead end in the middle of the solution.
        "the laid path does not open toward the cell after it",
        "                openings.push(on);",
        "",
        [
            "the_solution_the_generator_lays_actually_joins_the_source_to_the_drain",
            "every_puzzle_dealt_can_be_solved",
        ],
    ),
    (
        # `pipe_for_openings` must match the opening *set*, not merely the
        # count.  Dropping the containment check restores the old behaviour,
        # which fitted the first shape with the right number of ends whichever
        # way round it faced.
        "a pipe is fitted by how many ends it has, not which",
        "if have.len() == openings.len() && openings.iter().all(|d| have.contains(d))",
        "if have.len() == openings.len()",
        ["a_pipe_is_fitted_to_exactly_the_sides_it_was_asked_for"],
    ),
    (
        # A scramble that happens to land solved deals a puzzle already over.
        # Watched through `generate` alone this is invisible: a 6x6 scramble
        # lands solved about never, so the suite passes with the guard gone and
        # never runs it.  `deal` is split out so a two-cell board, which
        # reassembles roughly one time in sixteen, can.
        #
        # The mutation has to be `break;`, taking the first attempt whatever it
        # is.  *Deleting* the condition leaves the loop running all
        # SCRAMBLE_ATTEMPTS rounds and returning the last, which is a fresh
        # scramble and so almost never solved -- an almost-equivalent mutant
        # that survives on its own merits rather than through a weak test.
        "the first scramble is dealt however it lands",
        "            if !attempt.is_solved() {\n                break;\n            }",
        "            break;",
        ["a_scramble_that_lands_solved_is_dealt_again"],
    ),
    (
        # The retry must be bounded.  A board that cannot be unsolved -- a 1x1,
        # where the source is the drain -- otherwise spins forever.  Caught by
        # the suite hanging, not by a failure.
        "the scramble retries without a bound",
        "        for _ in 0..SCRAMBLE_ATTEMPTS {",
        "        loop {",
        ["dealing_a_board_that_cannot_be_unsolved_gives_up_rather_than_spinning"],
    ),
    (
        # Fault eight.  A cross turns onto itself and a straight repeats every
        # second turn; charging the player for either makes the win message
        # report a number of turns that did not all do anything.
        "a turn that changes nothing is counted anyway",
        "        if !pipe.turning_changes_anything() {\n            return false;\n        }",
        "",
        [
            "turning_a_shape_that_looks_the_same_is_refused",
            "a_turn_that_changes_nothing_is_not_counted",
        ],
    ),
    (
        # Not `Straight => 4`: `distinct_rotations` is only ever read through
        # `> 1`, so raising a 2 to a 4 changes nothing and the mutant survives
        # for a reason that has nothing to do with the test.  A cross claiming
        # four faces is the change that actually reaches behaviour.
        "a cross is treated as turnable",
        "            Self::Cross | Self::Empty => 1,",
        "            Self::Cross => 4,\n            Self::Empty => 1,",
        [
            "turning_a_shape_that_looks_the_same_is_refused",
            "a_turn_that_changes_nothing_is_not_counted",
        ],
    ),
    (
        # The floor is a *lower* bound.  Charging the shorter way round in one
        # direction only makes it overshoot on some boards, and a floor that
        # exceeds the real cost is not a floor.
        "the floor charges clockwise turns only",
        "    d.min(4_u32.saturating_sub(d))",
        "    d",
        ["the_floor_is_never_more_than_the_turns_it_really_takes"],
    ),
    (
        # A cell of the run must be open on both the side the water enters by
        # and the side it leaves by.  Charging only the exit is free passage
        # through cells that would have to be turned.
        "the floor does not charge the side the water came in by",
        "turns_to_open(pipe, &[entered, out])",
        "turns_to_open(pipe, &[out])",
        [
            "a_board_that_cannot_be_joined_says_so",
            "the_floor_is_zero_exactly_when_the_board_is_solved",
        ],
    ),
    (
        "a key release is a second press again",
        "        if !ev.pressed {\n            return None;\n        }",
        "",
        ["a_key_that_comes_back_up_is_not_a_second_press"],
    ),
    (
        "a modifier the program does not use is answered",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {\n"
        "            return None;\n"
        "        }",
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
        "cells record no hit box",
        "                f.hit(Target::Cell(index), square);",
        "",
        [
            "a_cell_is_clickable_exactly_where_it_is_drawn",
            "a_left_click_turns_the_pipe_it_lands_on_clockwise",
            "the_board_is_still_playable_in_a_window_too_small_for_the_chrome",
        ],
    ),
    (
        "level buttons record no hit box",
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
        "        f.hit(Target::ToggleHelp, l.help);",
        [
            "a_click_while_the_sheet_is_open_closes_it_and_reaches_nothing_behind_it",
            "while_the_sheet_is_up_the_frame_says_every_point_belongs_to_the_sheet",
        ],
    ),
    (
        # A right-click that turns the same way as a left one leaves no way to
        # undo an over-turn but three more turns.
        "both mouse buttons turn the same way",
        "        let clockwise = button == MouseButton::Left;",
        "        let clockwise = true;",
        ["a_right_click_turns_it_the_other_way"],
    ),
    (
        # The middle button is not a control.  Answering it swallows a click
        # the window manager may want.
        "every mouse button is a turn",
        "            MouseEventKind::Press(b @ (MouseButton::Left | MouseButton::Right)) => b,",
        "            MouseEventKind::Press(_) => MouseButton::Left,",
        ["a_button_the_program_does_not_use_turns_nothing"],
    ),
    (
        # A click read against a size the window is not is a click that lands
        # on whatever used to be there.
        "a click is read against a fixed window size",
        "        self.frame(self.size_drawn.0, self.size_drawn.1)\n            .hit_test(x, y)",
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).hit_test(x, y)",
        [
            "a_click_is_read_against_the_size_last_drawn",
            "rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against",
        ],
    ),
    (
        # Not `side = band.h`: that still assigns one number to both the width
        # and the height, so the board comes out square anyway and the mutant
        # survives for a reason that has nothing to do with the test.  The
        # board has to actually take the band's shape.
        "the board is not squared",
        "        let side = band.w.min(band.h).max(0.0);\n"
        "        let board = Rect::new(\n"
        "            band.x + (band.w - side) / 2.0,\n"
        "            band.y + (band.h - side) / 2.0,\n"
        "            side,\n"
        "            side,\n"
        "        );",
        "        let board = band;",
        ["the_board_is_square_in_every_window"],
    ),
    (
        "band drop order reversed",
        "const BAND_DROP_ORDER: [usize; 3] = [0, 2, 1];",
        "const BAND_DROP_ORDER: [usize; 3] = [1, 2, 0];",
        ["the_bands_go_in_the_stated_order"],
    ),
    (
        # Reserving *less* than the share is not what breaks this: the test
        # asserts a lower bound, and a smaller reservation cannot violate a
        # lower bound in a window roomy enough to satisfy it anyway.  Removing
        # the reservation is what starves the board, because then the chrome
        # only drops a band once it has eaten the whole window.
        "the board's share of the window is not reserved",
        "        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);",
        "        let budget = h;",
        ["the_board_keeps_its_share_of_every_window"],
    ),
    # NOT a mutation: `let top = hdr_h + inf_h;` -> `let top = info.bottom();`.
    # It looks like one -- a dropped band is `Rect::EMPTY`, whose bottom is
    # zero, which would put the board over the header -- but `BAND_DROP_ORDER`
    # drops the header before the info line, so the info line is only ever
    # dropped once the header already is and both forms give zero.  The mutant
    # is equivalent and no test can catch it.  Left in the source as the safe
    # form anyway, with the reasoning at the line, because the equivalence is
    # a property of a constant that someone will reorder.
    (
        "a won board goes on playing",
        '        if self.state == GameState::Won {\n'
        '            self.status = "Solved — press N for a new puzzle".to_string();\n'
        "            return;\n"
        "        }",
        "",
        ["a_won_board_ignores_further_turns"],
    ),
    (
        # Water must travel by a run of *mutual* joins.  Accepting a one-sided
        # opening floods through pipes that do not meet.
        "water flows through a pipe that does not meet the next one",
        "            (Some(here), Some(there)) => here.has_opening(dir) && there.has_opening(dir.opposite()),",
        "            (Some(here), Some(there)) => here.has_opening(dir) || there.has_opening(dir.opposite()),",
        ["water_reaches_a_cell_only_by_a_run_of_joins_from_the_source"],
    ),
    (
        # `Cross` in the filler is what makes the shape reachable at all; it
        # was unreachable in the shipped app (fault five).
        "the cross never turns up on a board again",
        "        const FILL: [PipeKind; 5] = [\n"
        "            PipeKind::Straight,\n"
        "            PipeKind::Corner,\n"
        "            PipeKind::Tee,\n"
        "            PipeKind::Cross,\n"
        "            PipeKind::End,\n"
        "        ];",
        "        const FILL: [PipeKind; 4] = [\n"
        "            PipeKind::Straight,\n"
        "            PipeKind::Corner,\n"
        "            PipeKind::Tee,\n"
        "            PipeKind::End,\n"
        "        ];",
        ["every_shape_turns_up_on_a_dealt_board"],
    ),
    (
        # The size the window renders at is what the next click is read
        # against; not recording it strands the hit test at the last size.
        "rendering does not record the size it drew at",
        "        self.size_drawn = (width.max(1.0), height.max(1.0));",
        "",
        [
            "a_click_is_read_against_the_size_last_drawn",
            "rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against",
        ],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "pipes", timeout=180))

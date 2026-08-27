"""Mutation test for the sliding-puzzle suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.
"""

import re
import subprocess
import sys
from pathlib import Path

SRC = Path(__file__).parent / "src" / "main.rs"
BAK = Path(__file__).parent / "src" / "main.rs.bak"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a key release is a second press again",
        "        if !ev.pressed {\n            return None;\n        }",
        "",
        [
            "a_key_that_comes_back_up_is_not_a_second_press",
            "one_tap_of_an_arrow_slides_exactly_one_tile",
            "one_tap_of_h_leaves_the_help_sheet_open_to_be_read",
            "one_tap_of_t_turns_the_numbers_off_and_they_stay_off",
        ],
    ),
    (
        "band drop order reversed",
        "const BAND_DROP_ORDER: [usize; 4] = [3, 0, 2, 1];",
        "const BAND_DROP_ORDER: [usize; 4] = [1, 2, 0, 3];",
        ["the_bands_go_in_the_stated_order"],
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
        "tiles record no hit box",
        "            f.hit(Target::Tile(index), square);",
        "",
        [
            "a_tile_is_clickable_exactly_where_it_is_drawn",
            "the_board_is_still_playable_in_a_window_too_small_for_the_chrome",
        ],
    ),
    (
        "size buttons record no hit box",
        "            f.hit(Target::Size(slot), r);",
        "",
        ["a_size_button_switches_to_the_size_it_names"],
    ),
    (
        "the open sheet records no hit box",
        "        f.hit(Target::ToggleHelp, l.window);",
        "",
        [
            "a_click_while_the_sheet_is_open_closes_it_and_reaches_nothing_behind_it",
            "while_the_sheet_is_up_the_frame_says_every_point_belongs_to_the_sheet",
        ],
    ),
    (
        # The version this replaced.  It reads sensibly -- cover the sheet you
        # drew -- and it is wrong: the sheet is opaque but smaller than the
        # window, so the controls it does not physically cover go on answering
        # clicks that the player cannot see the targets of.
        "the open sheet covers only its own pixels, not the window",
        "        f.hit(Target::ToggleHelp, l.window);",
        "        f.hit(Target::ToggleHelp, sheet);",
        ["while_the_sheet_is_up_the_frame_says_every_point_belongs_to_the_sheet"],
    ),
    (
        "a push moves one tile instead of the run",
        "        let mut moved = 0_usize;\n        for _ in 0..count {",
        "        let mut moved = 0_usize;\n        for _ in 0..count.min(1) {",
        [
            "clicking_the_far_end_of_the_gaps_row_pushes_the_whole_run",
            "a_pushed_run_counts_every_tile_it_moved",
            "a_push_moves_only_tiles_in_line_with_the_gap",
        ],
    ),
    (
        "a push counts one move however many tiles moved",
        "                        self.after_move(u32::try_from(moved).unwrap_or(u32::MAX));",
        "                        self.after_move(1);",
        ["a_pushed_run_counts_every_tile_it_moved"],
    ),
    (
        "a tile out of line with the gap is pushed anyway",
        "            // Not in line with the gap: no run of legal moves reaches it, and\n"
        "            // guessing which way the player meant would move tiles they did\n"
        "            // not click on.\n"
        "            return 0;",
        "            (Direction::Left, col.abs_diff(gcol))",
        ["a_push_moves_only_tiles_in_line_with_the_gap"],
    ),
    (
        "the scramble does not walk again when it comes home",
        "    for _ in 0..SCRAMBLE_ATTEMPTS {\n"
        "        board.shuffle(rng, walk);\n"
        "        if !board.is_solved() {\n"
        "            break;\n"
        "        }\n"
        "    }",
        "    board.shuffle(rng, walk);",
        ["a_scramble_that_comes_home_is_walked_again"],
    ),
    (
        "the scramble walks until it is unsolved, without a bound",
        "    for _ in 0..SCRAMBLE_ATTEMPTS {",
        "    loop {",
        ["a_board_that_cannot_be_scrambled_gives_up_instead_of_hanging"],
    ),
    (
        "the shuffle undoes itself half the time",
        "                if last == Some(dir.opposite()) {\n                    continue;\n                }",
        "",
        ["the_walk_never_undoes_the_move_it_just_made"],
    ),
    (
        "a won board goes on playing",
        "                if self.state == GameState::Playing && self.board.slide(dir) {",
        "                if self.board.slide(dir) {",
        ["a_won_board_ignores_further_moves"],
    ),
    (
        "the score is overwritten by any later game",
        "                if slot.is_none_or(|b| self.moves < b) {",
        "                if true {",
        ["a_worse_second_solve_does_not_replace_the_score"],
    ),
    (
        "the open sheet does not swallow the keys behind it",
        "        if self.show_help {\n"
        "            return match ev.key {\n"
        "                Key::H | Key::Escape | Key::Enter | Key::Space => Some(Action::CloseHelp),\n"
        "                _ => None,\n"
        "            };\n"
        "        }",
        "",
        ["the_open_sheet_swallows_the_keys_that_are_not_about_it"],
    ),
    (
        "a click is read against a fixed window size",
        "        self.frame(self.size_drawn.0, self.size_drawn.1)\n            .hit_test(x, y)",
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).hit_test(x, y)",
        ["a_click_is_read_against_the_size_last_drawn"],
    ),
    (
        "the distance floor counts the gap too",
        "            if tile == 0 {\n                continue;\n            }",
        "",
        ["one_move_changes_the_floor_by_exactly_one"],
    ),
    (
        # Not `BOARD_SHARE = 0.0`: the board's share is asserted as a *lower*
        # bound, and a smaller reservation cannot violate a lower bound in a
        # window roomy enough to satisfy it anyway.  Removing the reservation
        # from the budget is what actually starves the board, because then the
        # chrome only drops a band once it has eaten the whole window.
        "the board's share of the window is not reserved",
        "        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);",
        "        let budget = h;",
        ["the_board_keeps_its_share_of_every_window"],
    ),
    (
        "a modifier the program does not use is answered",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {\n"
        "            return None;\n"
        "        }",
        "",
        ["a_modifier_the_program_does_not_use_is_ignored"],
    ),
]


def run_tests():
    out = subprocess.run(
        [
            "python",
            "scripts/run-timeout.py",
            "120",
            "cargo",
            "test",
            "-p",
            "sliding",
            "--target",
            "x86_64-pc-windows-gnu",
        ],
        capture_output=True,
        text=True,
        cwd=Path(__file__).parent.parent.parent,
    )
    failed = set(re.findall(r"^    tests::(\S+)$", out.stdout, re.M))
    compiled = "could not compile" not in out.stdout + out.stderr
    timed_out = out.returncode == 124
    return compiled, failed, timed_out, out


def main():
    original = BAK.read_text(encoding="utf-8", newline="")
    SRC.write_text(original, encoding="utf-8", newline="")
    verdicts = []
    only = sys.argv[1:]
    for name, old, new, expect in MUTATIONS:
        if only and not any(o in name for o in only):
            continue
        if original.count(old) != 1:
            verdicts.append((name, f"SKIP anchor appears {original.count(old)}x"))
            print(f"[skip] {name}: anchor appears {original.count(old)} times")
            continue
        SRC.write_text(original.replace(old, new), encoding="utf-8", newline="")
        compiled, failed, timed_out, out = run_tests()
        if timed_out:
            # A mutant that hangs is caught: the test it hangs in is the one
            # whose whole job is to prove the loop is bounded.
            verdicts.append((name, "caught by a hang (bound removed)"))
            print(f"[ok]   {name}: caught \u2014 the suite hung, as the bound is gone")
        elif not compiled:
            verdicts.append((name, "SKIP did not compile"))
            print(f"[skip] {name}: mutant did not compile")
            print(out.stdout[-2000:])
        elif set(expect) <= failed:
            verdicts.append((name, f"caught by {len(failed)} test(s)"))
            print(f"[ok]   {name}: caught ({', '.join(sorted(failed))})")
        elif failed:
            verdicts.append((name, f"WRONG TESTS: {sorted(failed)}"))
            print(f"[??]   {name}: expected {expect}, got {sorted(failed)}")
        else:
            verdicts.append((name, "SURVIVED"))
            print(f"[BAD]  {name}: SURVIVED \u2014 no test failed")
        SRC.write_text(original, encoding="utf-8", newline="")
    print("\n=== summary ===")
    for name, v in verdicts:
        print(f"{v:<34} {name}")


if __name__ == "__main__":
    main()

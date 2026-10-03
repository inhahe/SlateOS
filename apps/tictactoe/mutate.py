"""Mutation test for tictactoe's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Tic-tac-toe was the first game given its window and its theme, and had no table
of its own until the legibility test reached it; this is its first.  The rows
break back the five faults its header records -- every key firing twice, a click
beside the board playing inside it, a constant layout, a status line that could
not be caught saying "thinking", a frame that needed `&mut self` -- and the
rules, the search, the pause before the computer's reply, the theme (C-Q16,
§1422) and the inks the legibility test moved.

The legibility test's first pass here found the score digits in the players'
colours at 4.1:1 on the score well, and the banner's and the help sheet's words
at 4.2:1 on the chrome's translucent veil, in the light theme.  The banner and
the sheet are the toolkit's panel now, and a colour-keyed word is moved only as
far as it must be to read on the ground it is drawn on (`gamechrome::Ink`).

Writing the table found three claims no test checked -- the keys that close
the sheet (only Escape was tried), a key the game does not know being left for
someone else, and a click that is not a left press -- and two tests narrower
than their names: the modifier test tried only Ctrl, and the sheet test only
N.

Usage:  python -u apps/tictactoe/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

T = ["the_window_is_drawn_in_the_users_colours"]
L = ["every_text_reads_on_what_is_under_it_in_either_theme"]

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── Fault 1: a release is not a second press ──────────────────────────
    (
        "a key's release runs it a second time",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }\n        let m = ev.modifiers;",
        "        let m = ev.modifiers;",
        [
            "arrow_key_release_does_not_move_the_cursor_again",
            "a_release_plays_no_move",
            "help_toggles_once_per_press",
        ],
    ),
    (
        "a key held with Ctrl is taken as the game's",
        "        if m.ctrl || m.alt || m.super_key {",
        "        if m.ctrl && m.alt && m.super_key {",
        ["keys_with_a_modifier_belong_to_the_window_manager"],
    ),
    (
        "keys reach the board behind the help sheet",
        "        if self.show_help {\n            // The sheet is modal",
        "        if false {\n            // The sheet is modal",
        ["the_sheet_swallows_the_keys_it_does_not_answer"],
    ),
    (
        "Enter and Space do not close the sheet",
        "            if matches!(ev.key, Key::H | Key::Escape | Key::Enter | Key::Space) {",
        "            if matches!(ev.key, Key::H | Key::Escape) {",
        ["the_keys_that_close_the_sheet_close_it"],
    ),
    (
        "a key the game does not know is claimed",
        "            None => EventResult::Ignored,\n        }\n    }\n\n    fn handle_mouse",
        "            None => EventResult::Consumed,\n        }\n    }\n\n    fn handle_mouse",
        ["a_key_the_game_does_not_know_is_left_for_someone_else"],
    ),
    (
        "Enter on a finished board plays instead of starting the next game",
        "            Key::Enter | Key::Space => Some(if self.playing() {\n                Action::Play(self.cursor)\n            } else {\n                Action::NewGame\n            }),",
        "            Key::Enter | Key::Space => Some(Action::Play(self.cursor)),",
        ["enter_on_a_finished_game_starts_an_empty_one"],
    ),
    (
        "Left moves the cursor right",
        "            Key::Left => Some(Action::Select(self.neighbour(-1, 0))),",
        "            Key::Left => Some(Action::Select(self.neighbour(1, 0))),",
        ["every_square_is_reachable_by_arrow_keys"],
    ),
    (
        "the cursor may walk off the bottom of the board",
        "        let row = here.div_euclid(side).saturating_add(dy).clamp(0, last);",
        "        let row = here.div_euclid(side).saturating_add(dy);",
        ["an_arrow_at_the_edge_is_a_wall"],
    ),
    # ── Fault 2: a click lands where it is drawn ──────────────────────────
    (
        "a click that is not a left press is taken",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if false {",
        ["a_click_that_is_not_a_left_press_is_ignored"],
    ),
    (
        "a square's hit box is its ink, not its whole square",
        "            f.hit(Target::Cell(i), outer);",
        "            f.hit(Target::Cell(i), r);",
        ["the_hit_box_of_a_square_is_the_square"],
    ),
    (
        "the header's buttons record no hit box",
        "        ground,\n    );\n    f.hit(target, r);\n}",
        "        ground,\n    );\n}",
        ["the_header_buttons_do_what_they_say"],
    ),
    (
        "the help sheet covers only its panel",
        "    f.hit(Target::HelpSheet, l.window);",
        "    f.hit(Target::HelpSheet, l.help);",
        ["the_help_sheet_takes_every_click_behind_it"],
    ),
    (
        "a click on a finished board does nothing",
        "                    // nothing, which is what every player tries first.\n                    self.apply(Action::NewGame);",
        "                    // nothing, which is what every player tries first.\n                    let _ = Action::NewGame;",
        ["a_click_on_a_finished_board_starts_the_next_game"],
    ),
    # ── Fault 3: the layout follows the window ────────────────────────────
    (
        "the layout is the window it was written on",
        "        let w = width.max(1.0);\n        let h = height.max(1.0);\n        let font = (h / 40.0).clamp(8.0, 17.0);",
        "        let _ = (width, height);\n        let w = WINDOW_WIDTH;\n        let h = WINDOW_HEIGHT;\n        let font = (h / 40.0).clamp(8.0, 17.0);",
        [
            "the_board_is_square_and_inside_the_window",
            "the_grid_follows_the_window_when_it_is_resized",
        ],
    ),
    (
        "the chrome takes the board's share of a short window",
        "        let budget = (h - h * BOARD_SHARE).max(0.0);",
        "        let budget = h;",
        ["the_board_keeps_its_share_of_the_window"],
    ),
    (
        "a band is squashed rather than dropped",
        "            if let Some(band) = wants.get_mut(i) {\n                *band = 0.0;\n            }",
        "            if let Some(band) = wants.get_mut(i) {\n                *band *= 0.5;\n            }",
        ["chrome_is_dropped_whole_rather_than_squashed"],
    ),
    (
        "the board is as wide as the window, not square",
        "        let side = (free.w.min(free.h) / SIDE as f32).floor().max(0.0) * SIDE as f32;",
        "        let side = (free.w / SIDE as f32).floor().max(0.0) * SIDE as f32;",
        ["the_board_is_square_and_inside_the_window"],
    ),
    (
        "the mark is one size whatever the square",
        "                let size = (r.h * 0.62).max(6.0);",
        "                let size = 60.0;",
        ["the_mark_shrinks_with_the_square"],
    ),
    # ── Fault 4: the computer's reply is a state a frame can show ─────────
    (
        "the computer replies in the same breath",
        "        self.turn = self.turn.other();\n        if self.turn != self.human {\n            self.begin_reply();\n        }",
        "        self.turn = self.turn.other();\n        if self.turn != self.human\n            && let Some(i) = best_move(&self.cells, self.turn)\n        {\n            self.play(i);\n        }",
        [
            "the_computer_does_not_reply_in_the_same_breath",
            "thinking_is_a_sentence_some_frame_actually_shows",
        ],
    ),
    (
        "the pause is counted in ticks, not in time",
        "        self.think_ms = self.think_ms.saturating_sub(elapsed_ms.max(1));",
        "        self.think_ms = self.think_ms.saturating_sub(TICK_MS);",
        ["the_pause_is_the_same_length_whatever_the_tick_rate"],
    ),
    (
        "ticks are asked for all the time",
        "    fn tick_interval(&self) -> Option<Duration> {\n        if self.thinking() {",
        "    fn tick_interval(&self) -> Option<Duration> {\n        if true {",
        ["ticks_are_asked_for_only_while_the_reply_is_owed"],
    ),
    (
        "a tick with nothing owed asks for a repaint",
        "            if app.advance(*elapsed_ms) {\n                EventResult::Consumed\n            } else {\n                EventResult::Ignored\n            }",
        "            app.advance(*elapsed_ms);\n            EventResult::Consumed",
        ["a_tick_with_nothing_owed_is_ignored"],
    ),
    (
        "a new game keeps the old game's pause",
        "        self.state = GameState::Playing;\n        self.think_ms = 0;",
        "        self.state = GameState::Playing;",
        ["a_new_game_started_mid_pause_is_never_played_into"],
    ),
    (
        "the computer does not open when it has X",
        "        self.think_ms = 0;\n        if self.turn != self.human {\n            self.begin_reply();\n        }",
        "        self.think_ms = 0;",
        ["the_computer_opens_when_the_human_takes_o"],
    ),
    # ── The rules ─────────────────────────────────────────────────────────
    (
        "two of a kind with any third square wins",
        "        if b == Some(a) && c == Some(a) {",
        "        if b == Some(a) || c == Some(a) {",
        ["a_full_board_with_no_line_is_a_draw"],
    ),
    (
        "one diagonal is not a line",
        "    [2, 4, 6],\n];",
        "    [2, 4, 7],\n];",
        ["every_line_of_three_wins"],
    ),
    (
        "a taken square can be played again",
        "        if !self.playing() || self.cell(index).is_some() {",
        "        if !self.playing() {",
        ["a_taken_square_cannot_be_played_again"],
    ),
    (
        "a finished board takes more moves",
        "        if !self.playing() || self.cell(index).is_some() {",
        "        if self.cell(index).is_some() {",
        ["play_itself_refuses_a_move_on_a_finished_board"],
    ),
    (
        "the score is kept by mark, not by player",
        "            let slot = usize::from(mark != self.human);",
        "            let slot = usize::from(mark == Mark::O);",
        ["the_score_follows_the_human_not_the_mark"],
    ),
    (
        "a draw is scored as a win for the human",
        "            if let Some(count) = self.scores.get_mut(2) {",
        "            if let Some(count) = self.scores.get_mut(0) {",
        ["a_full_board_with_no_line_is_a_draw"],
    ),
    (
        "a new game wipes the score",
        "    pub fn new_game(&mut self) {\n        self.cells = [None; CELLS];",
        "    pub fn new_game(&mut self) {\n        self.scores = [0; 3];\n        self.cells = [None; CELLS];",
        ["the_score_survives_a_new_game"],
    ),
    (
        "the human always moves first",
        "        self.cells = [None; CELLS];\n        self.turn = Mark::X;",
        "        self.cells = [None; CELLS];\n        self.turn = self.human;",
        ["x_always_moves_first"],
    ),
    (
        "swapping sides keeps the board",
        "        self.human = self.human.other();\n        self.new_game();",
        "        self.human = self.human.other();",
        ["swapping_sides_starts_a_fresh_board"],
    ),
    (
        "New game is offered on an empty board",
        "            Action::NewGame => !self.cells.iter().all(Option::is_none) || !self.playing(),",
        "            Action::NewGame => true,",
        ["new_game_is_offered_only_when_it_would_do_something"],
    ),
    (
        "a click on a taken square leaves the cursor where it was",
        "                let moved = self.cursor != i && i < CELLS;",
        "                let moved = false;",
        ["a_click_on_a_taken_square_moves_the_cursor_and_nothing_else"],
    ),
    (
        "a square past the board can be selected",
        "                if i >= CELLS || i == self.cursor {",
        "                if i == self.cursor {",
        ["a_square_out_of_range_is_refused"],
    ),
    (
        "the side button names the mark you already hold",
        '        let swap = format!("Play {}", self.computer().symbol());',
        '        let swap = format!("Play {}", self.human.symbol());',
        ["the_side_button_names_the_mark_you_would_move_to"],
    ),
    # ── The search ────────────────────────────────────────────────────────
    (
        "the computer picks its worst move",
        "        if best.is_none_or(|(top, _)| score > top) {",
        "        if best.is_none_or(|(top, _)| score < top) {",
        [
            "the_human_can_never_win_however_they_play",
            "the_search_never_gives_away_a_position_it_could_hold",
        ],
    ),
    (
        "the search expects its opponent to play for it",
        "            best = best.min(score);",
        "            best = best.max(score);",
        ["the_search_never_gives_away_a_position_it_could_hold"],
    ),
    # ── The theme (C-Q16, §1422) and its legibility ───────────────────────
    (
        "the theme is never taken up",
        "    fn theme_changed(&mut self, palette: &Palette) {\n        self.palette = *palette;",
        "    fn theme_changed(&mut self, palette: &Palette) {\n        let _ = palette;",
        T,
    ),
    (
        "O is drawn in the accent, like X",
        "            o: gamechrome::apart_from_accent(p),",
        "            o: p.ink(p.accent),",
        ["the_two_marks_are_never_alike"],
    ),
    (
        "the scores are their players' colours unmoved, 4.1:1 on the well",
        "                Ink::on(colour, &[c.chrome.well]).at(l.small, true),",
        "                colour,",
        L,
    ),
    (
        "the banner has no ground of its own",
        "(r.h * 0.3).min(12.0), Surface::Panel);",
        "(r.h * 0.3).min(12.0), Surface::Card);",
        ["the_banner_and_the_help_sheet_are_grounded"],
    ),
    (
        "the help sheet has no ground of its own",
        "        palette.push_surface(f, p.x, p.y, p.w, p.h, 10.0, Surface::Panel);",
        "        palette.push_surface(f, p.x, p.y, p.w, p.h, 10.0, Surface::Card);",
        ["the_banner_and_the_help_sheet_are_grounded"],
    ),
    (
        "the cursor may walk off the right of the board",
        "        let col = here.rem_euclid(side).saturating_add(dx).clamp(0, last);",
        "        let col = here.rem_euclid(side).saturating_add(dx);",
        ["an_arrow_at_the_edge_is_a_wall"],
    ),
    (
        "the winning line's marks are the page's shade on their lit cells",
        "                        Ink::on(c.chrome.even, &[c.chrome.lit]).at(size, true)",
        "                        c.chrome.even",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "tictactoe", timeout=300, only=only))

"""Mutation test for flood's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Flood-It was given its window (5873991b9) without a table of its own, so this
is its first.  The window's rewrite is recorded in `src/main.rs`'s header: a
`main` that built a game and dropped it, every key firing twice because the
handler never read `pressed`, swatches clickable ten pixels below where they
were drawn, a board nothing could click, a layout that was a constant, a size
kept twice and a size (10) no key could reach, and a random-number generator
whose parity put half the palette in every other column.  The rows below
break each of those back, and the theme (C-Q16, §1422: the chrome follows the
user's palette, the six colours keep their own).

Writing and sweeping the table found five tests that did not check what they
said:

* `a_board_the_size_of_a_postage_stamp_still_draws_every_cell` said "the gap
  between cells goes before the colour does" and counted hit boxes, which are
  recorded whether or not a cell is drawn.  A cell that was all gap passed.
  It now counts the cells drawn in their colour.
* `a_flood_takes_the_whole_connected_region_and_nothing_else` had nothing else
  of the flooded colour to take: every 0 on its board touched the corner.  A
  flood of every cell of the old colour, reachable or not, passed.  Its board
  now has a 0 the corner cannot reach.
* The corner the flood grows from is marked, and nothing checked the mark was
  there or could be seen on the cell -- now
  `the_corner_the_flood_grows_from_is_marked`.
* Nothing checked that a new board after a win or a loss is one to play: a
  new game that left the state `Lost` passed every test.  Now
  `a_new_board_after_the_end_is_playable`.
* `enabled_and_apply_agree` probed every action from `mid_game`'s 4x4 board,
  and 4 is not a size the game offers -- so no size button was ever the size
  in play, and a game that dealt a new board when asked for the size it was
  already on passed.  The first sweep said so (`[??]` on "the size in play
  deals a board when asked for again"); the test now probes a board of an
  offered size as well.

Usage:  python -u apps/flood/mutate.py [substring ...]
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
    # ── Keys ───────────────────────────────────────────────────────────────
    (
        "a key's release runs it a second time",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }\n",
        "        let _ = ev.pressed;\n",
        ["a_release_is_not_a_second_press", "a_key_makes_one_board_not_two"],
    ),
    (
        "a key held with Ctrl is taken as the game's",
        "        if m.ctrl || m.alt || m.super_key {",
        "        if m.ctrl && m.alt && m.super_key {",
        ["a_modified_key_is_left_for_someone_else"],
    ),
    (
        "keys reach the board behind the help sheet",
        "        if self.show_help {\n            // The sheet has the keyboard",
        "        if false {\n            // The sheet has the keyboard",
        [
            "keys_do_nothing_behind_the_help_sheet",
            "the_keys_that_dismiss_the_sheet_dismiss_it",
        ],
    ),
    (
        "Escape does not put the sheet down",
        "                Key::H | Key::Escape | Key::Enter | Key::Space => {",
        "                Key::H | Key::Enter | Key::Space => {",
        ["the_keys_that_dismiss_the_sheet_dismiss_it"],
    ),
    (
        "a key the game does not know is claimed",
        "            None => EventResult::Ignored,",
        "            None => EventResult::Consumed,",
        ["a_key_the_game_does_not_know_is_left_for_someone_else"],
    ),
    (
        "a colour key chooses the next colour",
        "                .map(Action::Choose)",
        "                .map(|i| Action::Choose((i + 1) % NUM_COLORS))",
        ["every_colour_key_is_the_swatch_it_names"],
    ),
    # ── The pointer ────────────────────────────────────────────────────────
    (
        "a click that is not a left press is taken",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if false {",
        ["a_click_that_is_not_a_left_press_is_ignored"],
    ),
    (
        "the help sheet covers only its panel",
        "        f.hit(Target::HelpSheet, l.window);",
        "        f.hit(Target::HelpSheet, l.help);",
        ["the_help_sheet_is_in_front_of_everything_it_covers"],
    ),
    (
        "a click on a cell does nothing",
        "            Target::Cell(row, col) => {\n                if let Some(value) = self.at(row, col) {\n                    self.apply(Action::Choose(value as usize));\n                }\n            }",
        "            Target::Cell(_, _) => {}",
        ["clicking_a_cell_chooses_its_colour"],
    ),
    (
        "a click the game answered is passed on",
        "        // footer is the board.\n        EventResult::Consumed",
        "        // footer is the board.\n        EventResult::Ignored",
        [
            "a_swatch_you_already_are_is_dimmed_and_still_takes_its_click",
            "clicking_a_cell_of_your_own_colour_is_not_a_move",
        ],
    ),
    (
        "a swatch chooses the colour after its own",
        "            Target::Swatch(i) => {\n                self.apply(Action::Choose(i));",
        "            Target::Swatch(i) => {\n                self.apply(Action::Choose((i + 1) % NUM_COLORS));",
        ["every_swatch_is_the_colour_it_shows", "every_colour_key_is_the_swatch_it_names"],
    ),
    (
        "a size button sets the size after its own",
        "                if let Some(&side) = SIZES.get(i) {",
        "                if let Some(&side) = SIZES.get(i.saturating_add(1)) {",
        ["every_board_size_is_reachable"],
    ),
    (
        "a swatch is clickable ten pixels below where it is drawn",
        "            f.hit(Target::Swatch(i), r);",
        "            f.hit(Target::Swatch(i), Rect::new(r.x, r.y + 4.0, r.w, r.h + 6.0));",
        [
            "what_is_drawn_at_a_swatch_is_what_a_click_there_selects",
            "the_strip_below_a_swatch_is_not_the_swatch",
        ],
    ),
    (
        "the board cannot be clicked",
        "                f.hit(Target::Cell(row, col), cell);",
        "                let _ = (row, col);",
        [
            "the_board_is_where_a_click_on_it_lands",
            "a_board_the_size_of_a_postage_stamp_still_draws_every_cell",
        ],
    ),
    (
        "the banner that says to press N is not N",
        "        f.hit(Target::NewGame, r);",
        "        let _ = Target::NewGame;",
        [
            "a_finished_game_says_so_and_offers_a_way_out",
            "a_new_board_after_the_end_is_playable",
        ],
    ),
    # ── The layout ─────────────────────────────────────────────────────────
    (
        "the layout is the window it was written on",
        "        let w = width.max(1.0);\n        let h = height.max(1.0);",
        "        let _ = (width, height);\n        let w = WINDOW_WIDTH;\n        let h = WINDOW_HEIGHT;",
        [
            "the_board_is_square_and_inside_its_window",
            "no_band_is_laid_past_the_bottom_of_the_window",
        ],
    ),
    (
        "the chrome takes the board's share of a short window",
        "        let budget = (h - h * BOARD_SHARE).max(0.0);",
        "        let budget = h;",
        ["a_window_too_short_for_the_footer_drops_it_rather_than_the_board"],
    ),
    (
        "the bands are dropped palette first",
        "const BAND_DROP_ORDER: [usize; 4] = [3, 2, 0, 1];",
        "const BAND_DROP_ORDER: [usize; 4] = [1, 0, 2, 3];",
        ["a_window_too_short_for_the_footer_drops_it_rather_than_the_board"],
    ),
    (
        "the footer is shown with no room for it",
        "    pub fn shows_footer(&self) -> bool {\n        self.footer.h >= 10.0 && self.footer.w >= 160.0\n    }",
        "    pub fn shows_footer(&self) -> bool {\n        true\n    }",
        ["a_window_too_short_for_the_footer_drops_it_rather_than_the_board"],
    ),
    (
        "the board is stretched to the width",
        "            side,\n            side,\n        );",
        "            avail_w,\n            side,\n        );",
        ["the_board_is_square_and_inside_its_window"],
    ),
    (
        "a tiny cell keeps its gap and loses its colour",
        "        let gap = if cs >= MIN_GAPPED_CELL { 1.0 } else { 0.0 };",
        "        let gap = 1.0;",
        ["a_board_the_size_of_a_postage_stamp_still_draws_every_cell"],
    ),
    (
        "only the board is painted",
        "        fill(&mut f, l.window, self.chrome.page, 0.0);",
        "        fill(&mut f, l.board, self.chrome.page, 0.0);",
        ["the_whole_window_is_painted"],
    ),
    (
        "the help sheet's rows run past its panel",
        "            if y + line > p.bottom() - pad {\n                break;\n            }",
        "            if y > f32::MAX {\n                break;\n            }",
        ["the_help_sheet_never_writes_past_its_own_panel"],
    ),
    (
        "the readout counts a row of the board, not the board",
        "            self.filled_count(),\n            self.cell_count()\n",
        "            self.filled_count(),\n            self.size()\n",
        ["the_readout_counts_the_board_it_is_drawn_beside"],
    ),
    (
        "the corner the flood grows from is not marked",
        "        if corner.w >= 6.0 {",
        "        if corner.w >= f32::MAX {",
        ["the_corner_the_flood_grows_from_is_marked"],
    ),
    # ── The rules ──────────────────────────────────────────────────────────
    (
        "the size in play deals a board when asked for again",
        "        if !SIZES.contains(&size) || size == self.size() {",
        "        if !SIZES.contains(&size) {",
        [
            "changing_size_resets_the_game_and_staying_put_does_not",
            "enabled_and_apply_agree",
        ],
    ),
    (
        "a size the game does not offer is dealt",
        "        if !SIZES.contains(&size) || size == self.size() {",
        "        if size == self.size() {",
        ["a_size_the_game_does_not_offer_is_refused"],
    ),
    (
        "10 is not offered",
        "const SIZES: [usize; 4] = [8, 10, 14, 18];",
        "const SIZES: [usize; 4] = [8, 12, 14, 18];",
        ["every_board_size_is_reachable"],
    ),
    (
        "a new size keeps the old size's move budget",
        "        self.max_moves = max_moves_for_size(size);\n        self.moves = 0;\n        self.state = GameState::Playing;\n        true",
        "        self.max_moves = max_moves_for_size(DEFAULT_SIZE);\n        self.moves = 0;\n        self.state = GameState::Playing;\n        true",
        ["each_size_has_its_own_move_budget"],
    ),
    (
        "a new game keeps the moves spent on the last",
        "        self.moves = 0;\n        self.state = GameState::Playing;\n    }\n\n    fn set_size",
        "        self.state = GameState::Playing;\n    }\n\n    fn set_size",
        ["the_new_game_button_deals_a_new_board", "a_new_board_after_the_end_is_playable"],
    ),
    (
        "a new game after the end is still over",
        "        self.moves = 0;\n        self.state = GameState::Playing;\n    }\n\n    fn set_size",
        "        self.moves = 0;\n    }\n\n    fn set_size",
        ["a_new_board_after_the_end_is_playable"],
    ),
    (
        "a flood spreads into any colour",
        "                if on_board && unseen && self.at(nr, nc) == Some(target) {",
        "                if on_board && unseen {",
        [
            "a_flood_takes_the_whole_connected_region_and_nothing_else",
            "the_filled_count_is_the_region_touching_the_corner",
        ],
    ),
    (
        "a flood takes every cell of the old colour, reachable or not",
        "        for (r, c) in self.region_of(old_color) {\n            self.set_cell(r, c, new_color);\n        }",
        "        let size = self.size();\n        for r in 0..size {\n            for c in 0..size {\n                if self.at(r, c) == Some(old_color) {\n                    self.set_cell(r, c, new_color);\n                }\n            }\n        }",
        ["a_flood_takes_the_whole_connected_region_and_nothing_else"],
    ),
    (
        "a flood reaches diagonally instead of to the right",
        "            (row, col.saturating_add(1)),",
        "            (row.saturating_add(1), col.saturating_add(1)),",
        [
            "a_flood_takes_the_whole_connected_region_and_nothing_else",
            "the_filled_count_is_the_region_touching_the_corner",
        ],
    ),
    (
        "the move limit is checked before the win",
        "        if all_same {\n            self.state = GameState::Won;\n        } else if self.moves >= self.max_moves {\n            self.state = GameState::Lost;\n        }",
        "        if self.moves >= self.max_moves {\n            self.state = GameState::Lost;\n        } else if all_same {\n            self.state = GameState::Won;\n        }",
        ["the_last_move_can_still_win"],
    ),
    (
        "the last move is not the last",
        "        } else if self.moves >= self.max_moves {",
        "        } else if self.moves > self.max_moves {",
        ["running_out_of_moves_loses_it"],
    ),
    (
        "choosing your own colour costs a move",
        "        if old_color == new_color {\n            return false;\n        }",
        "        let _ = old_color == new_color;",
        ["flooding_your_own_colour_is_free", "clicking_a_cell_of_your_own_colour_is_not_a_move"],
    ),
    (
        "a finished game takes moves",
        "        if self.state != GameState::Playing {\n            return false;\n        }\n        let old_color = self.head();",
        "        let old_color = self.head();",
        ["a_finished_game_takes_no_more_moves", "enabled_and_apply_agree"],
    ),
    (
        "a colour the palette does not have is flooded",
        "            Action::Choose(c) => c < NUM_COLORS && self.flood_fill(c as u8),",
        "            Action::Choose(c) => self.flood_fill(c as u8),",
        ["a_colour_the_palette_does_not_have_is_refused"],
    ),
    (
        "your own colour's swatch is offered as a move",
        "        self.state == GameState::Playing && color < NUM_COLORS && self.head() != color as u8",
        "        self.state == GameState::Playing && color < NUM_COLORS",
        [
            "enabled_and_apply_agree",
            "a_swatch_you_already_are_is_dimmed_and_still_takes_its_click",
        ],
    ),
    (
        "consecutive cells alternate parity, as the old generator's did",
        "        for row in &mut grid {\n            for cell in row.iter_mut() {\n                *cell = rng.below(NUM_COLORS) as u8;\n            }\n        }",
        "        let mut odd = false;\n        for row in &mut grid {\n            for cell in row.iter_mut() {\n                *cell = (rng.below(NUM_COLORS / 2) * 2 + usize::from(odd)) as u8;\n                odd = !odd;\n            }\n        }",
        ["horizontally_adjacent_cells_can_match", "a_column_is_not_confined_to_half_the_palette"],
    ),
    # ── The theme (C-Q16, §1422) ──────────────────────────────────────────
    (
        "the theme is never taken up",
        "        self.palette = *palette;\n        self.chrome = Chrome::of(palette);",
        "        let _ = palette;",
        T + ["a_new_game_keeps_the_users_colours"],
    ),
    (
        "the chrome is not rebuilt with the palette",
        "        self.chrome = Chrome::of(palette);",
        "        let _ = Chrome::of(palette);",
        T,
    ),
    (
        "the six colours follow the theme",
        "                let color = PALETTE\n",
        "                let color = [\n                    self.palette.red,\n                    self.palette.peach,\n                    self.palette.yellow,\n                    self.palette.green,\n                    self.palette.teal,\n                    self.palette.mauve,\n                ]\n",
        ["the_six_colours_are_the_same_in_every_theme"],
    ),
    (
        "a swatch's name is written dark whatever is under it",
        "                gamechrome::legible_on(INKS, under),\n                FontWeightHint::Bold,",
        "                INKS.1,\n                FontWeightHint::Bold,",
        L,
    ),
    (
        "a dimmed swatch's ink is chosen for it undimmed",
        "            let under = if dimmed {\n                self.chrome.scrim.over(*color)\n            } else {\n                *color\n            };",
        "            let under = *color;",
        L,
    ),
    (
        "the corner's mark is lost on its cell",
        "                gamechrome::legible_on(INKS, under),\n                d / 2.0,",
        "                INKS.0,\n                d / 2.0,",
        ["the_corner_the_flood_grows_from_is_marked"],
    ),
    (
        "your own colour is not outlined",
        "            if self.head() == i as u8 {\n                stroke(",
        "            if self.head() != i as u8 {\n                stroke(",
        ["your_own_colour_is_outlined_among_the_swatches"],
    ),
    (
        "the banner has no ground of its own",
        "            .push_surface(f, r.x, r.y, r.w, r.h, 4.0, Surface::Panel);",
        "            .push_surface(f, r.x, r.y, r.w, r.h, 4.0, Surface::Card);",
        ["the_banner_and_the_help_sheet_are_grounded"],
    ),
    (
        "the help sheet has no ground of its own",
        "                .push_surface(f, p.x, p.y, p.w, p.h, 8.0, Surface::Panel);",
        "                .push_surface(f, p.x, p.y, p.w, p.h, 8.0, Surface::Card);",
        ["the_banner_and_the_help_sheet_are_grounded"],
    ),
    (
        "the size in play looks like the others",
        "                if *side == self.size() {\n                    Kind::Primary\n                } else {\n                    Kind::Plain\n                },",
        "                Kind::Plain,",
        ["the_size_in_play_looks_chosen"],
    ),
    (
        "a win is told in the colour for a loss",
        "self.moves), self.chrome.good),",
        "self.moves), self.chrome.bad),",
        ["a_win_and_a_loss_are_told_in_their_colours"],
    ),
    (
        "the swatch scrim is Mocha's whatever the theme",
        "                fill(f, r, self.chrome.scrim, 4.0);",
        "                fill(f, r, Color::rgba(0x1E, 0x1E, 0x2E, 158), 4.0);",
        T,
    ),
    (
        "the help sheet's veil is Mocha's whatever the theme",
        "        fill(f, l.window, self.chrome.veil, 0.0);",
        "        fill(f, l.window, Color::rgba(0x11, 0x11, 0x1B, 214), 0.0);",
        T,
    ),
    (
        "the header band is Mocha's whatever the theme",
        "        fill(f, l.header, self.chrome.band, 0.0);",
        "        fill(f, l.header, Color::from_hex(0x181825), 0.0);",
        T,
    ),
    (
        "the readout is the faintest grey",
        "            &body,\n            l.small,\n            self.chrome.dim,\n            FontWeightHint::Regular,\n        );\n    }\n\n    fn draw_board",
        "            &body,\n            l.small,\n            self.chrome.off,\n            FontWeightHint::Regular,\n        );\n    }\n\n    fn draw_board",
        L,
    ),
    (
        "the help sheet's descriptions are the faintest grey",
        "                desc,\n                l.small,\n                self.chrome.dim,",
        "                desc,\n                l.small,\n                self.chrome.off,",
        L,
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "flood", timeout=300, only=only))

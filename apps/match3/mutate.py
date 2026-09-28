"""Mutation test for match3's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table starts with the theme (the operator's answer to C-Q16, §1422): the
board and the chrome follow the user's palette, the seven gems keep their
colours, and a new game -- which rebuilds the game from scratch -- keeps the
window's colours.

Usage:  python -u apps/match3/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The theme ─────────────────────────────────────────────────────────
    (
        "the theme is never taken up",
        "    fn theme_changed(&mut self, palette: &Palette) {\n        self.palette = *palette;",
        "    fn theme_changed(&mut self, palette: &Palette) {\n        let _ = palette;",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "the game-over veil is Mocha's whatever the theme",
        "        fill(f, l.board, c.chrome.veil, (l.radius() + l.frame).min(10.0));",
        "        fill(f, l.board, Color::rgba(17, 17, 27, 200), (l.radius() + l.frame).min(10.0));",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "a gem's symbol is written in the dark theme's text colour",
        "        (size, FontWeightHint::Bold),\n        GEM_INK,\n    );",
        "        (size, FontWeightHint::Bold),\n        Color::from_hex(0xCDD6F4),\n    );",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "two gems share a colour",
        "            Self::Ruby => RUBY,",
        "            Self::Ruby => SAPPHIRE,",
        ["test_gem_type_color_distinct"],
    ),
    (
        "a new game drops the user's colours",
        "        self.palette = palette;",
        "        let _ = palette;",
        ["a_new_game_keeps_the_users_colours"],
    ),
    (
        "the header's words are the page's inks, 3.6:1 on its band",
        "        let on = c.chrome.on(c.chrome.raised);",
        "        let on = c.chrome;",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "the game-over words have no panel of their own",
        "                l.pad * 0.6,\n                Surface::Panel,",
        "                l.pad * 0.6,\n                Surface::Card,",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "the bands give way in another order",
        "        for i in [2, 1, 0] {",
        "        for i in [0, 1, 2] {",
        ["in_a_short_window_the_bands_give_way_in_order"],
    ),
    (
        "the cells are a fraction of a pixel",
        "        let cell = ((side - frame * 2.0 - gap * (n - 1.0)) / n)\n            .floor()\n            .max(0.0);",
        "        let cell = ((side - frame * 2.0 - gap * (n - 1.0)) / n).max(0.0);",
        ["the_board_fits_every_window"],
    ),
    (
        "the board sits at the left of the window",
        "            free.x + (free.w - board_side) / 2.0,",
        "            free.x,",
        ["a_larger_window_gets_a_larger_board"],
    ),
    (
        "the board is a fixed size",
        "        let side = free.w.min(free.h);",
        "        let side = free.w.min(free.h).min(400.0);",
        ["a_larger_window_gets_a_larger_board"],
    ),
    (
        "the cells run past the board's frame",
        "            self.board.x + self.frame + pos.col as f32 * step,",
        "            self.board.x + self.frame * 3.0 + pos.col as f32 * step,",
        ["the_board_fits_every_window"],
    ),
    (
        "a gem's symbol stands at the cell's left",
        "    let x = r.x + (r.w - w).max(0.0) / 2.0;",
        "    let x = r.x;",
        ["a_gems_symbol_is_centred_on_it"],
    ),
    (
        "a click on a cell is not taken",
        "            Some(Target::Cell(pos)) if self.state != GameState::GameOver => {\n                self.select_or_swap(pos);\n            }",
        "            Some(Target::Cell(_)) => {}",
        ["test_mouse_click_selects", "every_cell_is_clicked_where_it_is_drawn"],
    ),
    (
        "a mode's button does not switch",
        "            Some(Target::Mode(mode)) => self.switch_mode(mode),",
        "            Some(Target::Mode(_)) => {}",
        ["the_controls_do_what_they_say"],
    ),
    (
        "the Hint button shows nothing",
        "            Some(Target::Hint) => self.show_hint(),",
        "            Some(Target::Hint) => {}",
        ["the_controls_do_what_they_say"],
    ),
    (
        "New game and the panel do nothing",
        "            Some(Target::NewGame | Target::GameOver) => self.new_game(),",
        "            Some(Target::NewGame | Target::GameOver) => {}",
        ["the_controls_do_what_they_say", "a_click_on_the_game_over_panel_starts_the_next_game"],
    ),
    (
        "the board takes clicks under the game-over panel",
        "                if !over {\n                    f.hit(Target::Cell(pos), r);\n                }",
        "                f.hit(Target::Cell(pos), r);",
        ["a_click_on_the_game_over_panel_starts_the_next_game"],
    ),
    (
        "Hint is live on a finished game",
        "                Target::Hint => (Kind::Plain, over),",
        "                Target::Hint => (Kind::Plain, false),",
        ["a_click_on_the_game_over_panel_starts_the_next_game"],
    ),
    (
        "the mode being played looks like the others",
        "                Target::Mode(mode) if mode == self.mode => (Kind::Primary, false),",
        "                Target::Mode(mode) if mode == self.mode => (Kind::Plain, false),",
        ["the_mode_being_played_is_the_primary_button"],
    ),
    (
        "the game-over panel takes no click",
        "        f.hit(Target::GameOver, panel);",
        "        let _ = panel;",
        ["a_click_on_the_game_over_panel_starts_the_next_game"],
    ),
    (
        "a click is read against the window it asks for, not the one it has",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["a_click_is_read_against_the_size_the_window_was_drawn_at"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "match3", timeout=240))

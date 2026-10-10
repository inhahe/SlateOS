"""Mutation test for breakout's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table starts with the theme (the operator's answer to C-Q16, §1422): the
field, the paddle and the chrome follow the user's palette; the brick rows and
the power-ups keep their hues, each in the shade that stands off the field; and
a new game keeps the window's colours.

Usage:  python -u apps/breakout/mutate.py [substring ...]
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
        "the overlay's veil is Mocha's whatever the theme",
        "        fill(f, l.window, c.chrome.veil, 0.0);",
        "        fill(f, l.window, Color::rgba(0x11, 0x11, 0x1B, 180), 0.0);",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "a brick row is always its pale shade",
        "        BRICK_ROWS_HUES.get(row).map_or(self.chrome.high, |&pair| {\n            gamechrome::legible_on(pair, self.chrome.page)\n        })",
        "        BRICK_ROWS_HUES.get(row).map_or(self.chrome.high, |&(pale, _)| pale)",
        ["every_brick_and_power_up_stands_off_the_field_in_either_theme"],
    ),
    (
        "a power-up's letter is always near-black",
        "                gamechrome::legible_on(POWERUP_INKS, face),",
        "                POWERUP_INKS.1,",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "a new game drops the user's colours",
        "        self.palette = palette;",
        "        let _ = palette;",
        ["a_new_game_keeps_the_users_colours"],
    ),
    (
        "the header's words are the page's inks, 4.1:1 in the well",
        "        let on = c.chrome.on(c.chrome.well);",
        "        let on = c.chrome;",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "the pause and game-over card's words are the page's inks, 3.6:1 on it",
        "        let on = c.chrome.on(c.chrome.raised);",
        "        let on = c.chrome;",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
]


# Why a greyed button is greyed, said while the pointer rests on it
# (2026-10-10; lane C, requests/c-e-say-why-a-control-is-disabled.md).
MUTATIONS += [
    (
        "the pointer is not followed for a greyed button's reason",
        '    let why = app.reasons.event(event);',
        '    let why = false;',
        ['a_greyed_pause_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        "the frame's greyed buttons are not handed over",
        '        self.reasons.drawn(greyed, (width, height));',
        '        let _ = greyed;',
        ['a_greyed_pause_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        "a greyed button's reason is not drawn",
        '        tree.commands.extend(self.reasons.render(&self.palette));',
        '        let _ = self.reasons.render(&self.palette);',
        ['a_greyed_pause_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        "the window asks for no tick for a greyed button's reason",
        '        self.reasons.sooner(own)',
        '        own',
        ['a_greyed_pause_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        'a greyed button says nothing: No game is under way yet: start one to ',
        '            GameState::Menu => Some("No game is under way yet: start one to pause it."),',
        '            GameState::Menu => Some(""),',
        ['a_greyed_pause_says_why_while_the_pointer_rests_on_it'],
    ),
    (
        'a greyed button says nothing: The game is over: start a new one to pl',
        '            GameState::GameOver => Some("The game is over: start a new one to play again."),',
        '            GameState::GameOver => Some(""),',
        ['a_greyed_pause_says_why_while_the_pointer_rests_on_it'],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "breakout", timeout=240, only=sys.argv[1:] or None))

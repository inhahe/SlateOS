"""Mutation test for hangman's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Hangman is the thirty-ninth application in this campaign.  Its old suite was
respectable -- 108 tests -- and it knew the rules of the game very well:
guessing, the six-strike limit, free reveals, the hint, the streak counters and
the word corpus all had tests and all of them passed.  None of it could see the
two things actually wrong with the program.

First, `main` was `let _app = HangmanApp::new();` -- it picked a word, revealed
whatever the difficulty owed, dropped the whole thing and exited.  Nothing was
ever displayed.

Second, and worse, there was no mouse handler at all.  Not a broken one: none.
`handle_event` matched `Event::Key` and nothing else, so the three rows of
letters the program drew across the bottom of its window were a *picture* of a
keyboard.  A player could look at the A key and could not press it.  The hint
was in the same state -- a line of text reading "Hint: H key", a label
describing a keystroke where a button would do -- and so were the category rows
and the difficulty chips.

Around that sat the usual fixed-window damage: `render` took no width and no
height and painted the same 740x560 picture into every window it was given,
from ten constants (`PADDING`, `HEADER_HEIGHT`, `GALLOWS_SIZE`,
`WORD_AREA_HEIGHT`, `KEYBOARD_HEIGHT`, `STATS_PANEL_WIDTH` and four font
sizes).  The menu title was centred by subtracting a literal 80.0.  The win
rate was drawn at `x: header_w - 60.0`, where `header_w` is a *width* being
used as a coordinate.  The header items sat at `PADDING + 100.0` and
`PADDING + 220.0`, offsets right for one set of words.  The chip row started
`5.0 * (btn_h + btn_gap)` down the menu -- the number of categories written a
second time, forty lines from `Category::ALL`.  The figure was six
`if wrong_count >= N` blocks, one per part, each repeating a number `MAX_WRONG`
already held.  Fifteen crate-level `#![allow]`s hid all of the arithmetic and
indexing findings that would have named most of it.

Usage:  python -u apps/hangman/mutate.py [substring ...]
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
        "    fn solve(w: f32, h: f32) -> Self {\n        let w = w.max(0.0);\n        let h = h.max(0.0);",
        "    fn solve(w: f32, h: f32) -> Self {\n        let _ = (w, h);\n        let w = WINDOW_WIDTH;\n        let h = WINDOW_HEIGHT;",
        ["the_layout_follows_the_window_rather_than_a_constant"],
    ),
    (
        "the type size is a constant rather than a share of the height",
        "        let font = (h / 38.0).clamp(9.0, 16.0);",
        "        let font: f32 = 14.0;",
        ["the_layout_follows_the_window_rather_than_a_constant"],
    ),
    (
        "the keyboard is sized by the width alone",
        "        let key = by_width.min(by_height).clamp(0.0, 34.0);",
        "        let key = by_width.clamp(0.0, 34.0);",
        ["the_keyboard_never_takes_more_than_its_share_of_the_height"],
    ),
    (
        "the keys are allowed to grow without limit",
        "        let key = by_width.min(by_height).clamp(0.0, 34.0);",
        "        let key = by_width.min(by_height).max(0.0);",
        ["the_keys_stop_growing_before_they_become_billboards"],
    ),
    (
        "the keyboard is left-aligned instead of centred",
        "        let keyboard = Rect::new(\n            ((w - kb_w) / 2.0).max(0.0),",
        "        let keyboard = Rect::new(\n            0.0,",
        ["the_keys_stop_growing_before_they_become_billboards"],
    ),
    (
        "the keyboard is placed below the bottom of the window",
        "            (h - pad - kb_h).max(header.bottom()),",
        "            (h - kb_h).max(header.bottom()) + pad,",
        ["every_part_of_the_layout_stays_inside_the_window"],
    ),
    (
        "the header is given the whole window width, padding and all",
        "        let header = Rect::new(pad, pad, (w - pad * 2.0).max(0.0), (font * 3.4).min(h));",
        "        let header = Rect::new(pad, pad, w, (font * 3.4).min(h));",
        ["every_part_of_the_layout_stays_inside_the_window"],
    ),
    (
        "the word row is allowed to run into the keyboard",
        "        let word_h = (word_font_for(font) * 2.0).min(free_h);",
        "        let word_h = word_font_for(font) * 2.0;",
        ["the_word_row_never_pushes_past_the_keyboard"],
    ),
    (
        "the free band is allowed to start below where it ends",
        "        let free_y = (header.bottom() + pad).min(keyboard.y);",
        "        let free_y = header.bottom() + pad;",
        ["the_word_row_never_pushes_past_the_keyboard"],
    ),
    (
        "the gallows takes the width the statistics column is using",
        "        let main_w = (if stats.w > 0.0 {\n            stats.x - pad - pad\n        } else {\n            w - pad * 2.0\n        })",
        "        let main_w = (w - pad * 2.0)",
        ["the_parts_of_the_layout_do_not_sit_on_top_of_each_other"],
    ),
    # ── The statistics column ─────────────────────────────────────────────
    (
        "the statistics column is sized by its heading alone",
        "        let stats_w_min = STATS_LINES\n            .iter()\n            .fold(0.0f32, |acc, s| {\n                acc.max(text::measure(s, small, FontWeightHint::Regular))\n            })\n            .max(text::measure(STATS_HEADING, font, FontWeightHint::Bold))\n            + pad * 2.0;",
        "        let stats_w_min = text::measure(STATS_HEADING, font, FontWeightHint::Bold) + pad * 2.0;",
        ["the_statistics_column_is_dropped_rather_than_squeezed"],
    ),
    (
        "the statistics column is kept in a window of any width",
        "        let stats_w = if w - stats_w_min - pad * 3.0 >= free_h.max(140.0) {\n            stats_w_min\n        } else {\n            0.0\n        };",
        "        let stats_w = stats_w_min;",
        ["the_statistics_column_is_dropped_rather_than_squeezed"],
    ),
    (
        "the statistics column is narrowed to whatever is left",
        "        let stats = if stats_w > 0.0 {\n            Rect::new(w - pad - stats_w, free_y, stats_w, free_h)",
        "        let stats = if stats_w > 0.0 {\n            Rect::new(w - pad - stats_w, free_y, stats_w * 0.6, free_h)",
        ["the_statistics_column_is_dropped_rather_than_squeezed"],
    ),
    # ── The keys ──────────────────────────────────────────────────────────
    (
        "the second and third key rows are not indented",
        "            let indent = KEY_ROWS\n                .first()\n                .map_or(0, |r| r.len())\n                .saturating_sub(row.len()) as f32\n                / 2.0;",
        "            let indent = 0.0;",
        ["each_row_of_keys_is_centred_in_the_keyboard"],
    ),
    (
        "the key rows are stacked without the gap between them",
        "            let step = self.key + self.key_gap;\n            return Some(Rect::new(\n                self.keyboard.x + (colf + indent) * step,\n                self.keyboard.y + rowf * step,",
        "            let step = self.key + self.key_gap;\n            return Some(Rect::new(\n                self.keyboard.x + (colf + indent) * step,\n                self.keyboard.y + rowf * self.key * 0.8,",
        ["no_two_keys_overlap"],
    ),
    (
        "the key grid is transposed: rows are read as columns",
        "                self.keyboard.x + (colf + indent) * step,\n                self.keyboard.y + rowf * step,",
        "                self.keyboard.x + (rowf + indent) * step,\n                self.keyboard.y + colf * step,",
        [
            "each_row_of_keys_is_centred_in_the_keyboard",
            "every_letter_has_a_key_and_the_keys_stay_in_the_keyboard",
            "nothing_is_drawn_outside_the_window",
        ],
    ),
    (
        "a key is placed one column to the right of where it is drawn",
        "                self.keyboard.x + (colf + indent) * step,",
        "                self.keyboard.x + (colf + indent + 1.0) * step,",
        ["every_letter_has_a_key_and_the_keys_stay_in_the_keyboard"],
    ),
    (
        "the keyboard reports a key even when it has been squeezed away",
        "        if self.key <= 0.0 {\n            return None;\n        }\n        let upper = letter.to_ascii_uppercase();",
        "        let upper = letter.to_ascii_uppercase();",
        ["a_window_too_small_for_a_keyboard_draws_no_keys_and_offers_none"],
    ),
    (
        "the keyboard drops a letter",
        'const KEY_ROWS: [&[u8]; 3] = [b"QWERTYUIOP", b"ASDFGHJKL", b"ZXCVBNM"];',
        'const KEY_ROWS: [&[u8]; 3] = [b"QWERTYUIOP", b"ASDFGHJKL", b"ZXCVBN"];',
        ["every_letter_the_keyboard_draws_can_also_be_typed"],
    ),
    (
        "the keyboard draws one letter twice",
        'const KEY_ROWS: [&[u8]; 3] = [b"QWERTYUIOP", b"ASDFGHJKL", b"ZXCVBNM"];',
        'const KEY_ROWS: [&[u8]; 3] = [b"QWERTYUIOP", b"ASDFGHJKL", b"ZXCVBNMM"];',
        ["every_letter_the_keyboard_draws_can_also_be_typed"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "hangman", timeout=300, only=only))

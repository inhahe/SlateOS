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

# The test that holds lesson 109.  The harness requires *every* named test to
# fail, so this names only the tighter of the two: a pass that overruns its own
# band often overruns the window as well, but "often" is not "always" -- a band
# in the middle of the window has siblings on both sides and the window test
# sees nothing at all, which is the entire blind spot this campaign is about.
CONTAINMENT = ["no_pass_paints_outside_the_region_it_owns"]

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
        "        let header = Rect::new(\n            pad,\n            pad,\n            (w - pad * 2.0).max(0.0),",
        "        let header = Rect::new(\n            pad,\n            pad,\n            w,",
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
    # ── Lesson 109: centring is not a bound ───────────────────────────────
    #
    # Everything below this line was added by the second pass over hangman.
    # The rows above it were all satisfied by the *old* size grid, which
    # stepped 240, 320, 480, 640 -- sizes at which every band is comfortably
    # taller than the type centred in it, so the arithmetic that only
    # misbehaves in a squeezed band never ran.  Six slivers now join the grid
    # and these rows are what proves they bite.
    (
        "a line is centred in a band too short to hold it",
        "    (!band.is_empty() && band.h >= size).then(|| band.y + (band.h - size) / 2.0)",
        "    Some(band.y + (band.h - size) / 2.0)",
        # The fault the lesson is named for.  The subtraction goes negative and
        # centring obligingly halves it, so the run is above the band by the
        # same distance it hangs below it.
        CONTAINMENT,
    ),
    (
        "a band with no room still hands out a place for a line",
        "    (!band.is_empty() && band.h >= size).then(|| band.y + (band.h - size) / 2.0)",
        "    Some(band.y)",
        # Not the same mutation.  This one is inside the band's *top* edge and
        # fails only on the bottom -- the half a test that checked `y >= band.y`
        # and stopped there would have missed entirely.
        CONTAINMENT,
    ),
    (
        "a run wider than its band starts to the left of it",
        "        None => (band.x + (band.w - text::measure(s, size, weight)) / 2.0).max(band.x),",
        "        None => band.x + (band.w - text::measure(s, size, weight)) / 2.0,",
        # Centring is not a bound in this direction either.
        CONTAINMENT,
    ),
    (
        "a run is given its band's full width as a max_width from an inset x",
        "    let w = band.right() - x;",
        "    let w = band.w;",
        # A run inset by a pad and told it may use the whole band's width may
        # elide nothing and still finish a pad past the right-hand edge.
        CONTAINMENT,
    ),
    (
        "a run is drawn into a band with no width left",
        "    (w > 0.0).then_some((x, w))",
        "    Some((x, w))",
        # Named for `span`'s own test rather than the containment sweep: the
        # sweep sees it too, but the test that states what `span` promises is
        # the one whose failure explains the fault.
        ["span_never_answers_a_box_that_starts_or_finishes_outside_its_band"],
    ),
    (
        "a column line is drawn wherever the column has got to",
        "    if y < band.y || y + size > band.bottom() {\n        return false;\n    }",
        "    if false {\n        return false;\n    }",
        CONTAINMENT,
    ),
    (
        "a column line is bounded at the top but not at the bottom",
        "    if y < band.y || y + size > band.bottom() {",
        "    if y < band.y {",
        # The guard written as half a guard, which is how it read in five of the
        # six places it used to be spelled out by hand.
        CONTAINMENT,
    ),
    # ── The two faults the slivers found in the layout ────────────────────
    (
        "the header's height is clamped to the window rather than to what is below it",
        "            (font * 3.4).min((h - pad).max(0.0)),",
        "            (font * 3.4).min(h),",
        # A height clamped to the whole window is not a bound on a band that
        # starts a pad down: 740x12 gave a 12 px bar at y = 4.
        ["every_part_of_the_layout_stays_inside_the_window"],
    ),
    (
        "an empty keyboard still claims the gap between the keys it does not have",
        "        let key_gap = if key > 0.0 {\n            (key * 0.14).clamp(1.0, 5.0)\n        } else {\n            0.0\n        };",
        "        let key_gap = (key * 0.14).clamp(1.0, 5.0);",
        ["the_keyboard_never_takes_more_than_its_share_of_the_height"],
    ),
    # ── The gallows ───────────────────────────────────────────────────────
    (
        "the gallows is sized by the width alone",
        "        let outer = (l.gallows.w - l.pad * 2.0).min(l.gallows.h).max(0.0);",
        "        let outer = (l.gallows.w - l.pad * 2.0).max(0.0);",
        # `min` has two operands and each of them is a separate bound, so each
        # gets a row.  Drop the height and any band wider than it is tall --
        # the common case, the band being what is left beside the statistics
        # column -- draws the beam above the band and the base below it.
        CONTAINMENT,
    ),
    (
        "the gallows is sized by the height alone",
        "        let outer = (l.gallows.w - l.pad * 2.0).min(l.gallows.h).max(0.0);",
        "        let outer = l.gallows.h.max(0.0);",
        # And drop the width: a band taller than it is wide puts the uprights
        # out through the side of it.
        CONTAINMENT,
    ),
    (
        "the stroke's own thickness is left out of the square it is drawn in",
        "        let side = (outer - stroke).max(0.0);",
        "        let side = outer;",
        # A stroke straddles the line it is drawn on, and its thickness has a
        # floor of one point that does not scale with the band, so in a band a
        # few points tall the half-stroke is wider than the picture's own top
        # margin of ten units in two hundred and twenty.
        CONTAINMENT,
    ),
    (
        "the gallows is pinned to the top of its band rather than centred in it",
        "        let oy = l.gallows.y + (l.gallows.h - side) / 2.0;",
        "        let oy = l.gallows.y + l.pad;",
        CONTAINMENT,
    ),
    (
        "the gallows is drawn in a band that has no room for it",
        "        if side <= 0.0 {\n            return;\n        }",
        "        if false {\n            return;\n        }",
        CONTAINMENT,
    ),
    # ── The word row ──────────────────────────────────────────────────────
    (
        "the word's baseline is centred whether or not the row can hold it",
        "        let Some(baseline) = centre_line(l.word, l.word_font) else {\n            return;\n        };",
        "        let baseline = l.word.y + (l.word.h - l.word_font) / 2.0;",
        CONTAINMENT,
    ),
    (
        "the rule under each blank is drawn at a fixed offset below the glyph",
        "        let rule = rule_y + RULE_WIDTH / 2.0 <= l.word.bottom();",
        "        let rule = true;",
        # The row is squeezed exactly when `word_h` clamps against `free_h`,
        # which any short window does.
        CONTAINMENT,
    ),
    (
        "the rule's own thickness is left out of the check that it fits",
        "        let rule = rule_y + RULE_WIDTH / 2.0 <= l.word.bottom();",
        "        let rule = rule_y <= l.word.bottom();",
        # A stroke straddles the line it is drawn on; half of it is below `y`.
        CONTAINMENT,
    ),
    (
        "each letter is bounded by the row rather than by its own cell",
        "            let cell = Rect::new(x, l.word.y, step, l.word.h);",
        "            let cell = l.word;",
        # Containment cannot see this either: a cell that is wrongly the whole
        # row is still inside the row.  Every letter then centres at the same
        # x and the word is drawn on top of itself, which only a test that
        # reads the positions can say.
        ["every_letter_of_the_word_gets_its_own_column"],
    ),
    # ── The header ────────────────────────────────────────────────────────
    (
        "the hint button is drawn at its nominal height in a squeezed header",
        "        let bh = (l.font * 1.8).min(l.header.h);",
        "        let bh = l.font * 1.8;",
        # A fill is drawn at exactly the size it is given, so this one paints
        # over the band above and answers clicks there too.
        CONTAINMENT,
    ),
    # ── The difficulty chips ──────────────────────────────────────────────
    (
        "a chip is drawn without being cut to the strip it is in",
        "            let Some(r) = Rect::new(cx, band.y, chip_w, chip_h).intersect(band) else {\n                continue;\n            };",
        "            let r = Rect::new(cx, band.y, chip_w, chip_h);",
        ["the_difficulty_chips_stay_inside_any_strip_they_are_given"],
    ),
    # ── The result card ───────────────────────────────────────────────────
    (
        "the result card is drawn at its nominal height in a band that cannot hold it",
        "        let box_h = box_h.min(over.h);",
        "        let box_h = box_h;",
        CONTAINMENT,
    ),
    (
        "the result card's buttons are placed below the card's own bottom edge",
        "            if r.bottom() > card.bottom() || r.is_empty() {\n                break;\n            }",
        "            if r.is_empty() {\n                break;\n            }",
        CONTAINMENT,
    ),
    (
        "the result card is painted over a band that no longer exists",
        "        let over = l.overlay;\n        if over.is_empty() {\n            return;\n        }",
        "        let over = l.overlay;",
        # Containment cannot see this one.  The dimming fill *is* the band, so
        # a band of no area gives a fill of no area, and an empty rectangle is
        # inside everything.  Only the converse -- no area, no commands -- can
        # tell refusing apart from drawing nothing.
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
    (
        "the header is filled before its band is looked at",
        "        if l.header.is_empty() {\n            return;\n        }\n        fill(f, l.header, MANTLE, CornerRadii::all(4.0));",
        "        fill(f, l.header, MANTLE, CornerRadii::all(4.0));",
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
    (
        "the statistics panel is filled before its band is looked at",
        "        if l.stats.is_empty() {\n            return;\n        }\n        fill(f, l.stats, MANTLE, CornerRadii::all(6.0));",
        "        fill(f, l.stats, MANTLE, CornerRadii::all(6.0));",
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
    (
        "the divider is drawn in a column too narrow to hold one",
        "        if w > 0.0 {\n            f.push(RenderCommand::Line {",
        "        if true {\n            f.push(RenderCommand::Line {",
        # A stroke of no length is still a mark one line width across, and it
        # is drawn a padding in from a left edge the column may not have.
        CONTAINMENT,
    ),
    # ── Containment is satisfied by drawing nothing ───────────────────────
    (
        "every centred run quietly refuses, and the window goes blank",
        "    let Some(y) = centre_line(band, size) else {\n        return;\n    };",
        "    if size >= f32::NEG_INFINITY {\n        return;\n    }\n    let Some(y) = centre_line(band, size) else {\n        return;\n    };",
        # The converse of every row above.  Each bound added for lesson 109 is
        # an intersection or a refusal, and both are perfectly happy to answer
        # nothing at all -- so without this row the whole table is satisfied by
        # a `return;` at the top of each pass.
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
    (
        "the statistics column stops after its heading",
        "            if !column_line(\n                f,\n                l.stats,\n                y,\n                &line,\n                l.small,\n                FontWeightHint::Regular,\n                color,\n                Some(l.pad),\n            ) {\n                return;\n            }",
        "            return;",
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "hangman", timeout=300, only=only))

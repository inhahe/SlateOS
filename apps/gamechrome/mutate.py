"""Mutation test for gamechrome's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Two tables, one per source file: the chrome and its buttons (`lib.rs`), and
the legibility reader a game's tests hold every text to (`legibility.rs`).

Usage:  python -u apps/gamechrome/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

LIB = Path(__file__).parent / "src" / "lib.rs"
LEGIBILITY = Path(__file__).parent / "src" / "legibility.rs"

# (name, old, new, [tests that must fail])
LIB_MUTATIONS = [
    (
        "a button's width leaves out the room on one side",
        "    text::measure(label, font_size, FontWeightHint::Bold) + label_pad(h) * 2.0",
        "    text::measure(label, font_size, FontWeightHint::Bold) + label_pad(h)",
        ["a_button_as_wide_as_it_asks_shows_its_label_whole"],
    ),
    (
        "a label taller than its button is drawn over its edges",
        "    if line > h || room <= 0.0 {",
        "    if room <= 0.0 {",
        ["a_label_taller_than_its_button_is_left_out"],
    ),
    (
        "a button with no room still draws its face",
        "    if w <= 0.0 || h <= 0.0 {\n        return;\n    }",
        "    if false {\n        return;\n    }",
        ["a_button_with_no_room_draws_nothing"],
    ),
    (
        "a label is centred on its font size rather than its line",
        "        y: y + (h - line) / 2.0,",
        "        y: y + (h - font_size) / 2.0,",
        ["a_buttons_label_is_centred_on_its_line"],
    ),
    (
        "a label's room is measured from the button's edge, not its start",
        "        max_width: Some(x + w - label_pad(h) - text_x),",
        "        max_width: Some(room),",
        ["a_labels_room_ends_at_the_buttons_padding"],
    ),
    (
        "large text is held to the small floor",
        "            large: to(legibility::LARGE_TEXT_FLOOR),",
        "            large: to(legibility::TEXT_FLOOR),",
        ["an_ink_is_moved_only_as_far_as_it_must_be_to_read"],
    ),
    (
        "an ink is moved toward the pole that does not read",
        "    let toward = if contrast_ratio(ground, black) >= contrast_ratio(ground, white) {",
        "    let toward = if contrast_ratio(ground, black) < contrast_ratio(ground, white) {",
        ["an_ink_is_moved_only_as_far_as_it_must_be_to_read"],
    ),
    (
        "an ink is moved past its floor",
        "    for _ in 0..24 {",
        "    for _ in 0..2 {",
        ["an_ink_is_moved_only_as_far_as_it_must_be_to_read"],
    ),
    (
        "every text takes the large strength",
        "        if legibility::is_large(size, bold) {\n            self.large",
        "        if true {\n            self.large",
        ["an_ink_is_picked_by_the_size_it_is_drawn_at"],
    ),
    (
        "on leaves the secondary grey as the page's",
        "            dim: read(self.dim),",
        "            dim: self.dim,",
        ["the_chrome_on_a_ground_reads_there"],
    ),
    (
        "on moves the inks only as far as large text needs",
        "        let read = |ink: Color| Ink::on(ink, &[ground]).small;",
        "        let read = |ink: Color| Ink::on(ink, &[ground]).large;",
        ["the_chrome_on_a_ground_reads_there"],
    ),
]

LEGIBILITY_MUTATIONS = [
    (
        "a translation is ignored",
        "            RenderCommand::PushTranslate { dx: tx, dy: ty } => offsets.push((dx + tx, dy + ty)),",
        "            RenderCommand::PushTranslate { dx: tx, dy: ty } => offsets.push((dx + 0.0 * tx, dy + 0.0 * ty)),",
        ["a_translation_moves_fills_and_texts_alike"],
    ),
    (
        "a fill is painted outside the clip in force",
        "                .intersect(clip);\n                painted.push((area, Paint::Fill(*color)));",
        "                ;\n                painted.push((area, Paint::Fill(*color)));",
        ["a_fill_clipped_away_is_not_under_anything"],
    ),
    (
        "a see-through fill hides what it covers",
        "                        Paint::Fill(c) => ground.map(|g| c.over(g)),",
        "                        Paint::Fill(c) => ground.map(|_| *c),",
        ["a_see_through_fill_is_composited_over_what_it_covers"],
    ),
    (
        "an opaque fill over a picture is still unknown",
        "                        Paint::Fill(c) if c.a == u8::MAX => Some(*c),",
        "                        Paint::Fill(c) if false => Some(*c),",
        ["a_text_over_a_picture_is_not_read"],
    ),
    (
        "a picture is read as whatever was under it",
        "                        Paint::Picture => None,",
        "                        Paint::Picture => ground,",
        ["a_text_over_a_picture_is_not_read"],
    ),
    (
        "a text is read at its corner",
        "                let mid_x = x + dx + wide / 2.0;",
        "                let mid_x = x + dx + wide * 0.0;",
        ["a_text_is_read_at_the_middle_of_its_line_not_its_corner"],
    ),
    (
        "a large text read is held to the small text's floor",
        "        if self.is_large() {\n            LARGE_TEXT_FLOOR\n",
        "        if false {\n            LARGE_TEXT_FLOOR\n",
        ["a_faint_text_is_illegible_unless_exempt_and_large_text_has_the_lower_floor"],
    ),
    (
        "an exempt text is held to the floor anyway",
        "        .filter(|r| !exempt(r) && r.ratio() < r.floor())",
        "        .filter(|r| r.ratio() < r.floor())",
        ["a_faint_text_is_illegible_unless_exempt_and_large_text_has_the_lower_floor"],
    ),
    (
        "bold text is large only at the plain size",
        "    size >= 24.0 || (bold && size >= 18.66)",
        "    size >= 24.0 || (bold && size >= 24.0)",
        ["an_ink_is_picked_by_the_size_it_is_drawn_at"],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing.
    only = sys.argv[1:]
    tables = [(LIB, LIB_MUTATIONS), (LEGIBILITY, LEGIBILITY_MUTATIONS)]
    names = [name for _, rows in tables for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    results = [0]
    for src, rows in tables:
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        results.append(sweep(src, rows, "gamechrome", timeout=600, only=mine or None))
    raise SystemExit(max(results))

"""Mutation test for the dictionary suite.

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
        "band drop order reversed",
        "const BAND_DROP_ORDER: [usize; 2] = [1, 0];",
        "const BAND_DROP_ORDER: [usize; 2] = [0, 1];",
        ["the_bands_go_in_the_stated_order"],
    ),
    (
        "list rows record no hit box",
        "            f.hit(Target::Row(top.saturating_add(slot)), r);",
        "",
        ["clicking_a_result_row_opens_the_word_that_row_shows"],
    ),
    (
        "list pane not clipped",
        "        f.clip(pane);\n        for slot in 0..visible.saturating_add(peek) {",
        "        for slot in 0..visible.saturating_add(peek) {",
        ["a_half_scrolled_row_is_not_clickable_where_it_was_never_drawn"],
    ),
    (
        "scroll_into_view does nothing",
        "pub fn scroll_into_view(sel: usize, top: &mut usize, visible: usize) {\n    if visible == 0 {",
        "pub fn scroll_into_view(sel: usize, top: &mut usize, visible: usize) {\n    if true {\n        let _ = (sel, visible);\n        return;\n    }\n    if visible == 0 {",
        ["the_selection_never_leaves_the_rows_on_screen"],
    ),
    (
        "wheel truncates instead of accumulating",
        "        let rows = self.wheel.rows(dy);",
        "        let rows = -(dy as isize);",
        ["the_wheel_scrolls_a_list_in_notches_not_pixels"],
    ),
    (
        "opening an entry keeps the old scroll",
        "        self.entry_scroll = 0.0;",
        "",
        ["opening_a_second_entry_starts_it_at_the_top"],
    ),
    (
        "chips never resolve to an entry",
        "            row.push((word.clone(), self.find_word(word)));",
        "            row.push((word.clone(), None));",
        ["every_entry_leads_somewhere_else_in_the_dictionary"],
    ),
    (
        "search box records no hit box",
        "            f.hit(Target::SearchBox, field);",
        "",
        ["the_search_field_answers_a_click_on_its_own_pixels"],
    ),
    (
        "the slash key is a shortcut again",
        "        if ev.types_text() {",
        "        if ev.single_char() == Some('/') {\n"
        "            return EventResult::Ignored;\n"
        "        }\n"
        "        if ev.types_text() {",
        ["the_slash_key_types_rather_than_being_a_shortcut_that_cannot_fire"],
    ),
    (
        "entry scroll is not clamped to the last line",
        "        self.entry_scroll = next.clamp(0.0, max);",
        "        self.entry_scroll = next.max(0.0);",
        ["an_entry_cannot_be_scrolled_past_its_own_last_line"],
    ),
    (
        "the featured word never steps",
        "            Action::StepFeatured(n) => self.step_featured(n),",
        "            Action::StepFeatured(n) => {\n                let _ = n;\n            }",
        ["the_featured_word_can_be_stepped_through_the_whole_dictionary"],
    ),
    (
        "history keeps duplicates",
        "        self.history.retain(|w| w != &word);",
        "",
        ["looking_the_same_word_up_twice_leaves_one_entry"],
    ),
]


def run_tests():
    out = subprocess.run(
        [
            "cargo",
            "test",
            "-p",
            "dictionary",
            "--target",
            "x86_64-pc-windows-gnu",
        ],
        capture_output=True,
        text=True,
        cwd=Path(__file__).parent.parent.parent,
    )
    failed = set(re.findall(r"^    tests::(\S+)$", out.stdout, re.M))
    compiled = "could not compile" not in out.stderr
    return compiled, failed, out


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
        compiled, failed, out = run_tests()
        if not compiled:
            verdicts.append((name, "SKIP did not compile"))
            print(f"[skip] {name}: mutant did not compile")
            print(out.stderr[-2000:])
        elif set(expect) <= failed:
            verdicts.append((name, f"caught by {len(failed)} test(s)"))
            print(f"[ok]   {name}: caught ({', '.join(sorted(failed))})")
        elif failed:
            verdicts.append((name, f"WRONG TESTS: {sorted(failed)}"))
            print(f"[??]   {name}: expected {expect}, got {sorted(failed)}")
        else:
            verdicts.append((name, "SURVIVED"))
            print(f"[BAD]  {name}: SURVIVED — no test failed")
        SRC.write_text(original, encoding="utf-8", newline="")
    print("\n=== summary ===")
    for name, v in verdicts:
        print(f"{v:<28} {name}")


if __name__ == "__main__":
    main()

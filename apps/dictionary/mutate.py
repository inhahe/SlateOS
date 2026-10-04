"""Mutation test for the dictionary suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Two files: `main.rs`, the window, and `online.rs`, the lookup over DICT that
the window uses for a word its built-in list lacks (2026-09-26).

Deliberately absent from `online.rs`: the whitespace collapsing inside a
braced word (`braced`). The lines of a sense are joined with single spaces
before the lists are read, so no reply WordNet sends reaches it with more
than one; it stays as a guard, read by eye.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

# main.rs, the lookup
LACKS = "a_word_the_list_lacks_is_looked_up_and_opened"
NEAR = "a_word_nobody_has_is_answered_with_what_is_near_it"
FAILS = "a_lookup_that_fails_says_why_and_changes_nothing"
REMEMBERED = "a_remembered_word_the_list_lacks_is_kept_and_looked_up"
MOVED_ON = "an_answer_for_a_reader_who_moved_on_does_not_take_the_screen"
ONCE = "asking_again_while_a_lookup_is_out_asks_once"
WAKES = "the_answer_wakes_the_window"
CHIP = "a_chip_naming_a_word_the_dictionary_lacks_looks_it_up"
NUMBERED = "every_chip_numbers_its_own_word"
# online.rs
READ = "a_word_is_looked_up_and_read"
LONG = "a_long_entry_keeps_its_parts_and_its_wrapped_lists"
MISSPELT = "a_misspelling_is_answered_with_suggestions"
UNSENDABLE = "a_word_that_would_break_the_command_is_refused_before_connecting"
BOUNDED = "a_reply_past_its_bounds_is_cut_off"
TURNED_AWAY = "a_server_that_turns_the_client_away_is_reported"
DOTS = "a_line_beginning_with_a_dot_is_read_as_sent"
LATIN1 = "a_line_that_is_not_utf8_loses_no_byte"
NOT_WORDNET = "text_that_is_not_wordnets_is_not_guessed_at"
CONTINUATION = "a_continuation_that_looks_like_a_sense_is_not_one"

# (name, old, new, [tests that must fail])
MAIN = [
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
        "        f.clip(strip);\n        for slot in 0..visible.saturating_add(peek) {",
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
        "                entry: self.find_word(word),",
        "                entry: None,",
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
    # -- the lookup, 2026-09-26 --
    (
        "a word the list has is sent to be looked up",
        "        if let Some(index) = self.find_word(word) {\n            self.open(index);\n            return;\n        }\n        if word.is_empty() || self.looking_up(word) {",
        "        if word.is_empty() || self.looking_up(word) {",
        [NEAR],
    ),
    (
        "the same word is asked twice",
        "        if word.is_empty() || self.looking_up(word) {",
        "        if word.is_empty() {",
        [ONCE],
    ),
    (
        "the answer does not wake the window",
        "                if let Some(waker) = waker {\n                    waker.wake();\n                }",
        "                let _ = waker;",
        [WAKES],
    ),
    (
        "a found entry is not kept",
        "                    self.entries.push(entry);",
        "                    drop(entry);",
        [LACKS, MOVED_ON],
    ),
    (
        "an answer takes the screen from a reader who moved on",
        "                let still_there = self.screen == screen && self.query == query;",
        "                let still_there = true;",
        [MOVED_ON],
    ),
    (
        "suggestions are dropped",
        "                self.miss = Some(Miss { word, suggestions });",
        "                let _ = (word, suggestions);\n                self.miss = None;",
        [NEAR],
    ),
    (
        "the selection stays off the suggestions",
        "                if offered && self.screen == Screen::Search {",
        "                if offered && false {",
        [NEAR],
    ),
    (
        "a remembered word the list lacks is dropped",
        "                .map(|w| {\n                    self.find_word(w)\n                        .map_or_else(|| Row::LookUp(w.clone()), Row::Entry)\n                })",
        "                .filter_map(|w| self.find_word(w).map(Row::Entry))",
        [REMEMBERED],
    ),
    (
        "the lookup is offered for a word the list has",
        "                if !query.is_empty() && self.find_word(query).is_none() {",
        "                if !query.is_empty() {",
        [LACKS],
    ),
    (
        "a chip for a word the list lacks is dead",
        "                f.hit(\n                    chip_word\n                        .entry\n                        .map_or(Target::Fetch(chip_word.ordinal), Target::Link),\n                    chip,\n                );",
        "                if let Some(i) = chip_word.entry {\n                    f.hit(Target::Link(i), chip);\n                }",
        [CHIP],
    ),
    (
        "cross-references skip the antonyms",
        "            .chain(&entry.antonyms)\n            .chain(&entry.related)\n            .nth(n)",
        "            .chain(&entry.related)\n            .nth(n)",
        [NUMBERED],
    ),
    (
        "no clock while a lookup is out",
        "        self.pending\n            .is_some()\n            .then_some(std::time::Duration::from_millis(500))",
        "        None",
        [LACKS],
    ),
    # -- the scrollbars, in the theme's style (c-e-a-theme-can-shape-the-controls)
    (
        "a long list's rows run under its bar",
        "        let strip = if bar.is_some() {\n            beside_bar(pane)\n        } else {\n            pane\n        };",
        "        let strip = pane;",
        ["a_long_lists_rows_stop_short_of_its_bar"],
    ),
    (
        "a tall entry's lines run under its bar",
        "        let text = beside_bar(pane);",
        "        let text = pane;",
        ["an_entry_too_tall_for_its_pane_gives_its_bar_a_column_and_drags"],
    ),
    (
        "the bar's column takes no press",
        "        f.hit(Target::ScrollTrack, bar.track);\n",
        "",
        [
            "a_long_lists_thumb_drags_the_rows_and_lets_go_on_release",
            "a_press_on_a_lists_track_pages_towards_it",
        ],
    ),
    (
        "the thumb takes no press",
        "        f.hit(Target::ScrollThumb, bar.thumb);\n",
        "",
        ["a_long_lists_thumb_drags_the_rows_and_lets_go_on_release"],
    ),
    (
        "a list's thumb is drawn at its top wherever the list is",
        "        thumb: scrollbar::thumb(track, total, visible, top),",
        "        thumb: scrollbar::thumb(track, total, visible, 0),",
        ["a_press_on_a_lists_track_pages_towards_it"],
    ),
    (
        "an entry's thumb is drawn at its top wherever the entry is",
        "        let position = if max > 0.0 { offset / max } else { 0.0 };",
        "        let position = 0.0;",
        ["an_entry_too_tall_for_its_pane_gives_its_bar_a_column_and_drags"],
    ),
    (
        "a press on the thumb takes no hold of it",
        "                    self.thumb_grab = Some(ev.y - bar.thumb.y);",
        "                    let _ = bar;",
        [
            "a_long_lists_thumb_drags_the_rows_and_lets_go_on_release",
            "an_entry_too_tall_for_its_pane_gives_its_bar_a_column_and_drags",
        ],
    ),
    (
        "a list's drag forgets where the thumb was taken hold of",
        "scrollbar::first_from_drag(bar.track, bar.thumb.h, grab, y, total, visible)",
        "scrollbar::first_from_drag(bar.track, bar.thumb.h, 0.0, y, total, visible)",
        ["a_long_lists_thumb_drags_the_rows_and_lets_go_on_release"],
    ),
    (
        "an entry's drag moves nothing",
        "                self.entry_scroll = next;\n",
        "",
        ["an_entry_too_tall_for_its_pane_gives_its_bar_a_column_and_drags"],
    ),
    (
        "a release does not let go of the thumb",
        "                return if self.thumb_grab.take().is_some() {",
        "                return if self.thumb_grab.is_some() {",
        ["a_long_lists_thumb_drags_the_rows_and_lets_go_on_release"],
    ),
    (
        "a press keeps hold of a thumb whose release never came",
        "        self.thumb_grab = None;\n        let Some(target) = self.target_at(ev.x, ev.y) else {",
        "        let Some(target) = self.target_at(ev.x, ev.y) else {",
        ["a_press_lets_go_of_a_thumb_whose_release_never_came"],
    ),
    (
        "a press on a list's track always pages down",
        "            self.scroll_rows(if up {\n                page.checked_neg().unwrap_or(0)\n            } else {\n                page\n            });",
        "            self.scroll_rows(page);",
        ["a_press_on_a_lists_track_pages_towards_it"],
    ),
    (
        "a press on an entry's track always pages down",
        "            self.scroll_entry(if up { -page } else { page });",
        "            self.scroll_entry(page);",
        ["an_entry_too_tall_for_its_pane_gives_its_bar_a_column_and_drags"],
    ),
    (
        "the bar never lights under the pointer",
        "            self.bar_hovered = over;\n",
        "",
        ["the_bar_follows_the_themes_style_and_lights_under_the_pointer"],
    ),
    (
        "the bar stays lit after the pointer leaves the window",
        "                return if std::mem::take(&mut self.bar_hovered) {",
        "                return if self.bar_hovered {",
        ["the_bar_follows_the_themes_style_and_lights_under_the_pointer"],
    ),
    (
        "a held thumb is drawn as if let go",
        "            dragging: self.thumb_grab.is_some(),",
        "            dragging: false,",
        ["the_bar_follows_the_themes_style_and_lights_under_the_pointer"],
    ),
    (
        "the shortcut card lets a press through to what is under it",
        "        if self.show_help {\n            // The card is modal for the pointer",
        "        if false {\n            // The card is modal for the pointer",
        ["the_shortcut_card_takes_a_press_rather_than_passing_it_on"],
    ),
    (
        "a file's lists keep the selections of the lists they replaced",
        "            self.set_sel(screen, 0);\n",
        "",
        ["lists_read_from_a_file_start_at_their_top"],
    ),
]

ONLINE = [
    (
        "a control character is sent",
        "    if word.chars().any(char::is_control) {",
        "    if false {",
        [UNSENDABLE],
    ),
    (
        "a quote is not escaped",
        "        if matches!(c, '\"' | '\\\\') {",
        "        if c == '\\\\' {",
        [UNSENDABLE],
    ),
    (
        "dot-stuffing is not undone",
        "            lines.push(match line.strip_prefix(\"..\") {\n                Some(rest) => format!(\".{rest}\"),\n                None => line,\n            });",
        "            lines.push(line);",
        [DOTS],
    ),
    (
        "a line is not bounded",
        "        let cap = MAX_LINE.min(self.budget).saturating_add(1);",
        "        let cap = usize::MAX;",
        [BOUNDED],
    ),
    (
        "a line that is not UTF-8 is emptied",
        "            .unwrap_or_else(|e| e.into_bytes().iter().map(|&b| char::from(b)).collect()))",
        "            .unwrap_or_default())",
        [LATIN1],
    ),
    (
        "a refusal at the door is taken for a greeting",
        "    if code != 220 {",
        "    if code == 0 {",
        [TURNED_AWAY],
    ),
    (
        "no suggestions are asked for",
        "            send(&mut writer, &format!(\"MATCH {DATABASE} lev {quoted}\"))?;\n",
        "",
        [MISSPELT],
    ),
    (
        "a sense's part of speech is not carried to the next",
        "            senses.push((current?, text.to_owned()));",
        "            senses.push((current.take()?, text.to_owned()));",
        [READ, LONG],
    ),
    (
        "any indented line can start a sense",
        "    if indent == 0 || indent > 5 {",
        "    if indent == 0 {",
        [CONTINUATION],
    ),
    (
        "an unknown part of speech is filed as a noun",
        "                current = Some(part_of_speech(tag)?);",
        "                current = Some(part_of_speech(tag).unwrap_or(PartOfSpeech::Noun));",
        [NOT_WORDNET],
    ),
    (
        "the examples stay in the gloss",
        "    } else if let Some(at) = text.find(\"; \\\"\") {",
        "    } else if let Some(at) = None::<usize> {",
        [READ],
    ),
    (
        "the headword is listed as its own synonym",
        "                if !w.eq_ignore_ascii_case(&entry.word) && !into.contains(&w) {",
        "                if !into.contains(&w) {",
        [READ, NOT_WORDNET],
    ),
    (
        "antonyms are filed as synonyms",
        "                \"ant\" => &mut entry.antonyms,",
        "                \"ant\" => &mut entry.synonyms,",
        [READ, LONG],
    ),
]

TABLES = {
    "main.rs": MAIN,
    "online.rs": ONLINE,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "dictionary", timeout=900, only=mine))
    raise SystemExit(worst)

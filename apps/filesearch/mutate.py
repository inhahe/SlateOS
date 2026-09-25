"""Mutation test for the file search's pointer layer and what it made reachable.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The search drew a query box, three filter strips, four switches, a sortable
table and a pane of action buttons, and handled no pointer event
(`known-issues.md`, `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
Asking of each thing it offered "can anything reach this?" found more:

  * Enter, listed on the F1 card as "Open what is selected", re-ran the search,
    and the preview's Open / Open Location / Copy Path / Properties buttons were
    wired to nothing -- the tool could find a file and do nothing with it;
  * the results were cut off at the panel's edge with no scrolling, and the
    keyboard could select rows that were never drawn;
  * the filters panel ran under the status bar;
  * every search was recorded on every keystroke into a history nothing
    showed, and saving a search had no caller;
  * and "now" was the constant 1_779_000_000, so "Today" was one day in May
    2026 for ever.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a filter chip is drawn and records no hit box",
        "                    f.hit(target, Rect::new(rect.x + 4.0, rect.y, rect.w - 8.0, 22.0));\n",
        "",
        ["every_filter_answers_the_pointer"],
    ),
    (
        "the lit file-type chip does not turn itself off",
        "                self.criteria.category_filter = if self.criteria.category_filter == Some(cat) {",
        "                self.criteria.category_filter = if false && self.criteria.category_filter == Some(cat) {",
        ["every_filter_answers_the_pointer"],
    ),
    (
        "the mode chip is a label again",
        "        f.hit(Target::SearchMode, mode);\n",
        "",
        ["every_filter_answers_the_pointer"],
    ),
    (
        "the filters panel does not scroll",
        "            Some(target) if self.show_filters && target.is_in_filters() => {",
        "            Some(target) if false && self.show_filters && target.is_in_filters() => {",
        [
            "the_filters_panel_scrolls_to_its_last_row",
            "every_filter_answers_the_pointer",
            "a_saved_search_outlives_the_window",
            "a_recent_search_can_be_run_again",
        ],
    ),
    (
        "a column heading does not sort",
        "            f.hit(Target::Header(sort), rect);\n",
        "",
        ["a_column_heading_sorts"],
    ),
    (
        "a result row records no hit box",
        "            });\n            f.hit(target, row);\n\n            ry += RESULT_ROW_H;",
        "            });\n\n            ry += RESULT_ROW_H;",
        ["a_press_selects_the_result_under_it"],
    ),
    (
        "the selection walks off the bottom of the results",
        "            self.results_scroll = selected.saturating_sub(rows.saturating_sub(1));",
        "            let _ = rows;",
        ["results_past_the_panel_can_be_reached"],
    ),
    (
        "the wheel over the results scrolls nothing",
        "                    .saturating_add_signed(rows)",
        "                    .saturating_add_signed(rows * 0)",
        ["results_past_the_panel_can_be_reached"],
    ),
    (
        "a page step past the end refuses to move",
        "                let moved = pos.saturating_add(delta).max(0);",
        "                let moved = pos.saturating_add(delta).max(0);\n"
        "                if usize::try_from(moved).map_or(true, |m| m >= self.results.len()) {\n"
        "                    return EventResult::Ignored;\n"
        "                }",
        ["results_past_the_panel_can_be_reached"],
    ),
    (
        "a new search keeps the old scroll",
        "        self.results_scroll = 0;\n    }",
        "    }",
        ["results_past_the_panel_can_be_reached"],
    ),
    (
        "Enter re-runs the search again",
        "                if self.selected_result.is_some() {",
        "                if false && self.selected_result.is_some() {",
        [
            "enter_and_the_actions_open_what_is_selected",
            "a_search_is_remembered_when_it_is_used_not_as_it_is_typed",
        ],
    ),
    (
        "Open Location opens nothing",
        "            Target::OpenLocation => self.open_location(),",
        "            Target::OpenLocation => EventResult::Consumed,",
        ["enter_and_the_actions_open_what_is_selected"],
    ),
    (
        "a folder result is opened with a file's program",
        "            Some(FILE_MANAGER.to_string())\n        } else {\n            opener_for(&path)\n        };",
        "            opener_for(&path)\n        } else {\n            opener_for(&path)\n        };",
        ["a_folder_result_opens_in_the_file_manager"],
    ),
    (
        "the same search is listed twice",
        "        if let Some(pos) = self\n            .search_history\n            .iter()\n"
        "            .position(|s| s.query == query && s.mode == mode)\n        {\n"
        "            let mut again = self.search_history.remove(pos);",
        "        if let Some(pos) = self\n            .search_history\n            .iter()\n"
        "            .position(|s| false && s.query == query && s.mode == mode)\n        {\n"
        "            let mut again = self.search_history.remove(pos);",
        ["a_search_is_remembered_when_it_is_used_not_as_it_is_typed"],
    ),
    (
        "an unreadable saved search is dropped on the next write",
        "            .chain(self.unreadable_saved.iter().cloned())\n",
        "",
        ["an_unreadable_saved_search_is_written_back_as_it_was"],
    ),
    (
        "now is frozen again",
        "            current_time: unix_now(),",
        "            current_time: 1_779_000_000,",
        ["now_is_read_from_the_clock"],
    ),
    (
        "indexing keeps a stale clock",
        "        self.criteria.current_time = unix_now();",
        "        let _ = unix_now();",
        ["now_is_read_from_the_clock"],
    ),
    (
        "the folder button is drawn and records no hit box",
        "        self.draw_button(f, folder, FOLDER_BUTTON_LABEL, Target::ChooseFolder);",
        "        self.draw_button(f, folder, FOLDER_BUTTON_LABEL, Target::SearchBox);",
        ["the_folder_button_asks_for_a_folder"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "filesearch", timeout=600, only=only))

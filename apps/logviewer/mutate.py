"""Mutation test for the log viewer's reader, its tail, its export and its
pointer layer.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The viewer opened on twenty invented entries under the name of a file it never
read, and its "tailing" and "export" were a switch and a sentence in the module
doc (known-issues, TD-C-LOGVIEWER-TAILS-A-STRING-COMPILED-INTO-ITSELF).  It
drew tabs, view buttons, level pills, a search box and a list, and handled no
pointer event (TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED).
Reading real files exposed what a reader has to get right: a line still being
written, a character cut by a read, a log rotated underneath, a log too big to
read whole, an export that must write the file's bytes rather than their
rendering.

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
    # -- reading ---------------------------------------------------------------
    (
        "a last line with no newline is hidden",
        "        if !open.is_empty() {\n            lines.push(open);\n        }\n",
        "",
        [
            "a_line_caught_half_written_becomes_one_entry_when_it_is_finished",
            "test_log_file_parse_plain",
        ],
    ),
    (
        "a line finished later becomes a second entry",
        "        if !joined.is_empty() {\n            // The unfinished line's entry",
        "        if false {\n            // The unfinished line's entry",
        ["a_line_caught_half_written_becomes_one_entry_when_it_is_finished"],
    ),
    (
        "a line finished later loses its bookmark",
        "            if n == 0 {\n                entry.bookmarked = bookmarked;",
        "            if n == usize::MAX {\n                entry.bookmarked = bookmarked;",
        ["finishing_a_line_keeps_the_bookmark_it_was_given_half_written"],
    ),
    (
        "a character cut by a read stays cut",
        "        self.unfinished = open.to_vec();",
        "        self.unfinished = String::from_utf8_lossy(open).into_owned().into_bytes();",
        ["a_character_cut_in_half_by_a_read_is_whole_once_the_rest_arrives"],
    ),
    (
        "every entry is kept, however many",
        "        if old > 0 {\n            self.entries.drain(..old);\n        }\n",
        "",
        ["the_newest_entries_are_kept_when_there_are_too_many"],
    ),
    (
        "a cut at the start of a line drops that line",
        "        let mut start = cut_at.saturating_sub(1);",
        "        let mut start = cut_at;",
        ["a_log_too_large_to_read_whole_is_read_from_its_end_at_a_whole_line"],
    ),
    (
        "a cut inside a line keeps the half line",
        "            bytes.drain(..cut);",
        "            let _ = cut;",
        ["a_log_too_large_to_read_whole_is_read_from_its_end_at_a_whole_line"],
    ),
    (
        "journal times are read as milliseconds",
        "    if t < 100_000_000_000 {",
        "    if t < 1 {",
        ["a_journal_record_is_read_with_its_time_in_seconds"],
    ),
    # -- the tail -----------------------------------------------------------------
    (
        "a rotated log is not read again",
        "        if len < self.read_len {\n            let fresh = Self::open(&path).ok()?;",
        "        if len < self.read_len && false {\n            let fresh = Self::open(&path).ok()?;",
        ["a_log_rotated_under_the_viewer_is_read_again_from_its_start"],
    ),
    (
        "the clock reads nothing",
        "            Event::Tick { .. } => {\n                if self.refresh_tails() {",
        "            Event::Tick { .. } if false => {\n                if self.refresh_tails() {",
        [
            "the_tail_reads_what_is_written_and_following_goes_to_it",
            "a_reader_who_selects_an_earlier_entry_is_not_dragged_away_from_it",
        ],
    ),
    (
        "the clock stops when not following",
        "            .any(|f| f.source.is_some())\n            .then_some(TAIL_POLL)",
        "            .any(|f| f.source.is_some() && self.auto_scroll)\n            .then_some(TAIL_POLL)",
        ["the_clock_runs_while_a_file_is_open_following_or_not"],
    ),
    (
        "following drags a reader away from the entry they chose",
        "            && self.selected_entry != self.last_shown()\n        {",
        "            && self.selected_entry != self.last_shown()\n            && false\n        {",
        ["a_reader_who_selects_an_earlier_entry_is_not_dragged_away_from_it"],
    ),
    (
        "a filter change while following goes to the top",
        "        if self.auto_scroll {\n            // Following: the end of whatever is shown now.",
        "        if false {\n            // Following: the end of whatever is shown now.",
        ["a_filter_changed_while_following_stays_at_the_end"],
    ),
    (
        "the rows move under a reader when old entries go",
        "        if let Some(top) = top_moved {",
        "        if let Some(top) = top_moved.filter(|_| false) {",
        ["the_rows_under_a_reader_stay_put_when_old_entries_are_let_go"],
    ),
    (
        "a file already open is opened again",
        "            .position(|f| f.source.as_deref().is_some_and(|src| same_file(src, path)))",
        "            .position(|f| f.source.as_deref().is_some_and(|src| same_file(src, path) && false))",
        ["a_file_already_open_is_brought_forward_rather_than_opened_twice"],
    ),
    # -- what is worked out once --------------------------------------------------
    (
        "the filtered list outlives the entries it was worked out from",
        "        let key = VisibleKey {\n            log: log.id,\n            revision: log.revision,",
        "        let key = VisibleKey {\n            log: log.id,\n            revision: 0,",
        [
            "the_tail_reads_what_is_written_and_following_goes_to_it",
            "the_star_bookmarks_its_entry_and_the_switch_shows_only_those",
        ],
    ),
    (
        "the detail layout outlives a change of entry",
        "            revision: log.revision,\n            entry: index,",
        "            revision: log.revision,\n            entry: 0,",
        ["the_detail_buttons_step_bookmark_and_toggle_the_source"],
    ),
    (
        "the detail layout outlives a change of theme",
        "            .is_some_and(|l| l.key == key);",
        "            .is_some_and(|l| l.key.entry == key.entry && l.key.log == key.log);",
        ["the_detail_is_drawn_again_in_a_new_theme"],
    ),
    # -- exporting ------------------------------------------------------------------
    (
        "an export writes the decoded text",
        "        let Some(path) = &self.source else {",
        "        let Some(path) = None::<&std::path::PathBuf> else {",
        ["an_export_writes_the_exact_bytes_of_the_entries_shown"],
    ),
    (
        "an export of a log changed on disk is written anyway",
        "            || fnv1a(FNV_OFFSET, &bytes) != self.read_hash",
        "            || false",
        ["an_export_refuses_a_log_that_changed_on_disk_since_it_was_read"],
    ),
    (
        "an export writes over the log itself",
        "            .is_some_and(|src| same_file(src, path))\n        {",
        "            .is_some_and(|src| same_file(src, path) && false)\n        {",
        ["an_export_will_not_write_over_the_log_it_is_exporting"],
    ),
    (
        "every export is called .jsonl",
        '    name.push(log_name.extension().unwrap_or(std::ffi::OsStr::new("log")));',
        '    name.push("jsonl");',
        ["an_export_is_offered_under_the_logs_name_in_its_own_format"],
    ),
    # -- the detail view ---------------------------------------------------------------
    (
        "the detail body does not scroll",
        "                if over == Some(Target::DetailBody) && self.view_mode == ViewMode::Detail {",
        "                if over == Some(Target::DetailBody) && false {",
        ["a_long_entry_scrolls_in_the_detail_and_its_buttons_stay_where_they_are"],
    ),
    (
        "the detail's buttons scroll away with a long entry",
        "        let by = body.bottom() + 8.0;",
        "        let by = body.bottom() + 8.0 - offset;",
        ["a_long_entry_scrolls_in_the_detail_and_its_buttons_stay_where_they_are"],
    ),
    (
        "the detail cuts a long line",
        "    let lines = text::wrap_hard(text_in, width, size, FontWeightHint::Regular);",
        '    let lines = vec![text::elide(text_in, width, "...", size, FontWeightHint::Regular)];',
        ["the_detail_shows_all_of_a_raw_line_with_no_spaces_in_it"],
    ),
    (
        "the detail leaves a word with no spaces on one line",
        "    let lines = text::wrap_hard(text_in, width, size, FontWeightHint::Regular);",
        "    let lines = text::wrap(text_in, width, size, FontWeightHint::Regular);",
        ["the_detail_shows_all_of_a_raw_line_with_no_spaces_in_it"],
    ),
    (
        "the list's wrapping leaves a word with no spaces on one line",
        "            text::wrap_hard(message, width, NORMAL_TEXT, FontWeightHint::Regular)",
        "            text::wrap(message, width, NORMAL_TEXT, FontWeightHint::Regular)",
        ["wrapping_breaks_a_word_too_long_for_the_row"],
    ),
    (
        "the detail's source button is not a toggle",
        "                if own.is_some() && self.filter.source_filter == own {",
        "                if false {",
        ["the_detail_buttons_step_bookmark_and_toggle_the_source"],
    ),
    # -- the pointer ------------------------------------------------------------------
    (
        "a tab is drawn and records no hit box",
        "            cmds.hit(Target::Tab(fi), tab);",
        "",
        [
            "a_tab_brings_its_log_forward_and_its_box_closes_it",
            "every_control_drawn_is_the_one_a_press_on_it_reaches",
        ],
    ),
    (
        "a tab's close box closes nothing",
        "            Target::CloseTab(i) => self.close_tab(i),",
        "            Target::CloseTab(_) => EventResult::Ignored,",
        ["a_tab_brings_its_log_forward_and_its_box_closes_it"],
    ),
    (
        "Open opens nothing",
        "            Target::OpenLog => {\n                self.ask_to_open();",
        "            Target::OpenLog => {\n                let _ = ();",
        ["open_and_export_put_up_the_picker_and_escape_takes_it_down"],
    ),
    (
        "a level pill records no hit box",
        "            cmds.hit(Target::Level(*level), pill);",
        "",
        ["a_level_pill_sets_the_floor"],
    ),
    (
        "the search box keeps the keyboard after a press elsewhere",
        "                let unfocused =\n"
        "                    target != Some(Target::SearchBox) && std::mem::take(&mut self.search_focused);",
        "                let unfocused = false;",
        ["the_search_box_takes_the_keyboard_and_a_press_elsewhere_takes_it_back"],
    ),
    (
        "the pattern switch does nothing",
        "            Target::RegexSwitch => {\n                self.filter.regex = !self.filter.regex;",
        "            Target::RegexSwitch => {\n                let _ = self.filter.regex;",
        ["the_pattern_switch_makes_the_search_a_regular_expression"],
    ),
    (
        "a pattern minds case where the text search does not",
        "        Some(ere::Regex::new_flags(self.search_query.as_bytes(), true)",
        "        Some(ere::Regex::new_flags(self.search_query.as_bytes(), false)",
        ["the_pattern_switch_makes_the_search_a_regular_expression"],
    ),
    (
        "a pattern that does not compile does not say why",
        '            Some(Err(why)) => Some(format!("The pattern does not compile: {why}")),',
        "            Some(Err(_)) => None,",
        ["a_pattern_that_does_not_compile_matches_nothing_and_says_why"],
    ),
    (
        "the bookmarked switch does nothing",
        "            Target::BookmarkedOnly => {\n"
        "                self.filter.show_bookmarked_only = !self.filter.show_bookmarked_only;",
        "            Target::BookmarkedOnly => {\n"
        "                let _ = self.filter.show_bookmarked_only;",
        ["the_star_bookmarks_its_entry_and_the_switch_shows_only_those"],
    ),
    (
        "the source chip clears nothing",
        "            Target::SourceChip => {\n                self.filter.source_filter = None;",
        "            Target::SourceChip => {\n                let _ = &self.filter.source_filter;",
        ["a_press_on_a_source_shows_only_that_source_and_its_chip_clears_it"],
    ),
    (
        "the time chip clears only one end",
        "                self.filter.time_start = None;\n                self.filter.time_end = None;",
        "                self.filter.time_start = None;",
        ["the_detail_buttons_set_the_time_range_and_its_chip_clears_it"],
    ),
    (
        "a row is named by where it is drawn rather than by its entry",
        "                Target::Entry(*original_idx),",
        "                Target::Entry(vi),",
        ["a_press_on_a_row_selects_that_entry_whatever_row_it_is_drawn_on"],
    ),
    (
        "the star bookmarks the next entry",
        "            Target::Star(i) => {\n                self.toggle_bookmark(i);",
        "            Target::Star(i) => {\n                self.toggle_bookmark(i.saturating_add(1));",
        ["the_star_bookmarks_its_entry_and_the_switch_shows_only_those"],
    ),
    (
        "a press on a source does not filter by it",
        "            Target::Source(i) => {\n                self.selected_entry = Some(i);\n"
        "                self.filter.source_filter = None;\n                self.toggle_source_filter()",
        "            Target::Source(i) => {\n                self.selected_entry = Some(i);\n"
        "                EventResult::Consumed",
        ["a_press_on_a_source_shows_only_that_source_and_its_chip_clears_it"],
    ),
    (
        "a level's row in the statistics stays in the statistics",
        "                self.set_min_level(level);\n                self.view_mode = ViewMode::List;",
        "                self.set_min_level(level);",
        ["the_statistics_lead_to_the_list"],
    ),
    (
        "a level's row runs under the summary",
        "                        .min(stats_x - 20.0 - (PADDING + 4.0))\n",
        "",
        ["a_level_row_in_the_statistics_stops_short_of_the_summary"],
    ),
    (
        "the wheel does not move the list",
        "                self.list_scroll = self.list_scroll.saturating_add_signed(rows).min(last);",
        "                self.list_scroll = self.list_scroll.min(last);",
        ["the_wheel_scrolls_the_list_and_scrolling_back_stops_following"],
    ),
    (
        "scrolling back keeps following",
        "                    if rows < 0 {\n                        self.auto_scroll = false;",
        "                    if rows < isize::MIN {\n                        self.auto_scroll = false;",
        ["the_wheel_scrolls_the_list_and_scrolling_back_stops_following"],
    ),
    (
        "the selection walks off the screen",
        "        self.list_scroll = first;\n    }",
        "        let _ = first;\n    }",
        ["walking_past_the_bottom_of_the_screen_scrolls_the_list_to_the_selection"],
    ),
    (
        "a page up near the top goes nowhere",
        "                let moved = pos.saturating_add(delta).max(0);",
        "                let moved = pos.saturating_add(delta);\n"
        "                if moved < 0 {\n                    return EventResult::Ignored;\n                }",
        ["walking_past_the_bottom_of_the_screen_scrolls_the_list_to_the_selection"],
    ),
    (
        "a double press opens nothing",
        "            MouseEventKind::DoubleClick(MouseButton::Left) => {",
        "            MouseEventKind::DoubleClick(MouseButton::Left) if false => {",
        ["a_double_press_on_an_entry_opens_it"],
    ),
    (
        # The card's hit box is the whole of its modality: an earlier row broke
        # a second rule in `handle_event` that said the same thing, survived,
        # and the rule was deleted as dead.
        "a press goes through the shortcut card",
        "            cmds.hit(\n                Target::HelpCard,\n"
        "                Rect::new(0.0, 0.0, self.win_w, self.win_h),\n            );",
        "",
        ["a_press_puts_the_shortcut_card_away_and_reaches_nothing_under_it"],
    ),
    (
        "a press on the shortcut card leaves it up",
        "            Target::HelpCard => {\n                self.show_help = false;",
        "            Target::HelpCard => {\n                let _ = self.show_help;",
        ["a_press_puts_the_shortcut_card_away_and_reaches_nothing_under_it"],
    ),
    (
        "the pointer lights nothing",
        "                self.hover = over;\n                EventResult::Consumed",
        "                let _ = over;\n                EventResult::Consumed",
        ["the_pointer_lights_what_it_is_over_and_the_status_bar_says_what_it_does"],
    ),
    # -- the window ---------------------------------------------------------------------
    (
        "the background stops at the size the window opened at",
        "            width: self.win_w,\n            height: self.win_h,\n            color: self.palette.base,",
        "            width: WINDOW_WIDTH,\n            height: WINDOW_HEIGHT,\n            color: self.palette.base,",
        ["the_window_is_laid_out_at_the_size_it_is_given"],
    ),
    (
        "a resize is not believed",
        "                    self.win_w = *width as f32;",
        "                    let _ = width;",
        ["the_window_is_laid_out_at_the_size_it_is_given"],
    ),
    (
        "the empty window's message is drawn over entries",
        "        if self.files.is_empty() {\n            // Where the entries would be",
        "        if self.files.len() < usize::MAX {\n            // Where the entries would be",
        ["with_no_log_open_the_window_says_how_to_open_one"],
    ),
    # -- keys -----------------------------------------------------------------------------
    (
        "a chord nobody bound runs the bare key",
        "        if key.modifiers.ctrl {\n            return self.handle_chord(key);\n        }\n",
        "        if key.modifiers.ctrl && self.handle_chord(key) == EventResult::Consumed {\n"
        "            return EventResult::Consumed;\n        }\n",
        ["a_chord_nobody_bound_does_not_run_the_key_under_it"],
    ),
    (
        "the chords stop working while a search is typed",
        "        // Chords first, and apart.",
        "        if self.search_focused {\n            return self.handle_key_search(key);\n        }\n"
        "        // Chords first, and apart.",
        ["the_chords_and_f1_work_while_a_search_is_being_typed"],
    ),
    (
        "O does not filter by source",
        "            Key::O => self.toggle_source_filter(),",
        "            Key::O => EventResult::Ignored,",
        ["o_and_the_brackets_filter_by_the_selected_entry", "every_advertised_key_does_something"],
    ),
    (
        "N and P move only the counter",
        "            self.selected_entry = Some(hit);\n            self.keep_selection_visible();",
        "            let _ = hit;",
        ["n_and_p_go_to_the_match_as_well_as_counting_it"],
    ),
    (
        "Ctrl+Tab goes nowhere",
        "            Key::Tab => self.cycle_tab(!key.modifiers.shift),",
        "            Key::Tab => EventResult::Ignored,",
        ["ctrl_tab_goes_round_the_logs_and_ctrl_w_closes_them", "every_advertised_key_does_something"],
    ),
    (
        "starting to follow does not go to the end",
        "        self.auto_scroll = !self.auto_scroll;\n        if self.auto_scroll {\n"
        "            self.select_edge(false);\n        }\n        EventResult::Consumed",
        "        self.auto_scroll = !self.auto_scroll;\n        EventResult::Consumed",
        [
            "the_follow_button_starts_following_from_the_end_and_stops_it",
            "a_reader_who_selects_an_earlier_entry_is_not_dragged_away_from_it",
        ],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "logviewer", timeout=900, only=only))

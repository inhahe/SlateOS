"""Mutation test for the snippets suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- Languages -----------------------------------------------------
    (
        "an unknown extension is guessed at rather than admitted",
        "            _ => Self::PlainText,\n        }\n    }\n\n    fn detect_from_content",
        "            _ => Self::Rust,\n        }\n    }\n\n    fn detect_from_content",
        # Only the one: `every_language_is_reachable_from_its_own_extension`
        # walks the explicit arms and skips PlainText by name, so the fallback
        # this breaks is the one arm it never reaches.
        ["an_unknown_extension_is_plain_text"],
    ),
    (
        "an extension is matched with its case",
        "        match ext.to_ascii_lowercase().as_str() {",
        "        match ext {",
        ["an_extension_is_recognised_whatever_its_case"],
    ),
    (
        "one Rust token is enough to call it Rust",
        '        if content.contains("fn ")',
        '        if content.contains("fn ") || true || false',
        ["rust_is_detected_from_a_function_and_one_other_token"],
    ),
    (
        "a name with an extension is ignored in favour of the content",
        "        let by_extension = Language::from_extension(ext);",
        "        let by_extension = Language::PlainText;",
        ["a_name_beats_the_content_it_disagrees_with"],
    ),
    (
        "a name with no useful extension stops there",
        "    Language::detect_from_content(content)\n}",
        "    let _ = content;\n    Language::PlainText\n}",
        ["a_name_with_no_useful_extension_falls_through_to_the_content"],
    ),
    # -- Searching -----------------------------------------------------
    (
        "an empty query matches nothing instead of everything",
        "    if query.is_empty() {\n        return snippets.iter().collect();",
        "    if query.is_empty() {\n        return Vec::new();",
        ["an_empty_query_matches_everything"],
    ),
    (
        "a title search looks at the content too",
        "            SearchScope::Title => s.title.to_ascii_lowercase().contains(&lower_query),",
        "            SearchScope::Title => s.content.to_ascii_lowercase().contains(&lower_query),",
        ["a_scope_looks_only_where_it_says"],
    ),
    (
        "a content search looks at the title too",
        "            SearchScope::Content => s.content.to_ascii_lowercase().contains(&lower_query),",
        "            SearchScope::Content => s.title.to_ascii_lowercase().contains(&lower_query),",
        ["a_scope_looks_only_where_it_says"],
    ),
    (
        "the all scope only looks at the title",
        "                s.title.to_ascii_lowercase().contains(&lower_query)\n                    || s.content.to_ascii_lowercase().contains(&lower_query)",
        "                s.title.to_ascii_lowercase().contains(&lower_query)\n                    || false",
        ["the_all_scope_looks_everywhere"],
    ),
    (
        "a query is matched with its case",
        "    let lower_query = query.to_ascii_lowercase();",
        "    let lower_query = query.to_string();",
        ["a_query_ignores_case"],
    ),
    # -- Templates -----------------------------------------------------
    (
        "a placeholder is spelt without its braces",
        '        let placeholder = format!("${{{name}}}");',
        '        let placeholder = format!("${name}");',
        ["applying_a_template_replaces_every_copy_of_a_variable"],
    ),
    (
        "only the first copy of a variable is filled",
        "        result = result.replace(&placeholder, value);",
        "        result = result.replacen(&placeholder, value, 1);",
        ["applying_a_template_replaces_every_copy_of_a_variable"],
    ),
    (
        "the same variable is listed once per appearance",
        "            if !var.is_empty() && !vars.contains(&var) {",
        "            if !var.is_empty() {",
        ["a_template_variable_is_found_once_however_often_it_appears"],
    ),
    (
        "an unclosed placeholder is dropped",
        "            if !var.is_empty() && !vars.contains(&var) {\n                vars.push(var);",
        "            if !var.is_empty() && !vars.contains(&var) && chars.get(i) == Some(&'}') {\n                vars.push(var);",
        ["an_unclosed_placeholder_is_still_a_variable"],
    ),
    # -- Exporting -----------------------------------------------------
    (
        "the export does not escape what it writes",
        '    format!("\\"{}\\"", guitk::escape::json_string(s))',
        '    format!("\\"{s}\\"")',
        ["an_export_escapes_what_would_break_it"],
    ),
    (
        "a failed write reports a success",
        "            Err(err) => Err(format!(\"Could not write {}: {err}\", show_path(&path))),",
        "            Err(_) => Ok(String::new()),",
        ["an_export_that_failed_says_so_in_red"],
    ),
    (
        "a note that worked is coloured like one that did not",
        "                Ok(message) => (message, GREEN),",
        "                Ok(message) => (message, RED),",
        ["an_export_that_worked_says_so_in_green"],
    ),
    (
        "a note that failed is coloured like one that worked",
        "                Err(message) => (message, RED),",
        "                Err(message) => (message, GREEN),",
        ["an_export_that_failed_says_so_in_red"],
    ),
    (
        "the note lets the tags share its line",
        "            return;\n        }\n\n        let Some(s) = self.selected_snippet() else {",
        "        }\n\n        let Some(s) = self.selected_snippet() else {",
        ["the_export_note_takes_the_line_the_tags_were_on"],
    ),
    # -- The layout ----------------------------------------------------
    (
        "the columns leave a gap between them",
        "            list: Rect::new(sidebar_w, body_y, list_w, body_h),",
        "            list: Rect::new(sidebar_w + 4.0, body_y, list_w, body_h),",
        ["the_columns_lie_side_by_side_and_leave_no_gap"],
    ),
    (
        "the editor starts where the list started",
        "                sidebar_w + list_w,\n                body_y,",
        "                sidebar_w,\n                body_y,",
        ["the_columns_lie_side_by_side_and_leave_no_gap"],
    ),
    (
        "the body starts at the top of the window rather than under the toolbar",
        "        let body_y = toolbar.bottom();",
        "        let body_y = 0.0;",
        ["the_toolbar_is_above_the_columns_and_they_reach_the_bottom"],
    ),
    (
        "the columns stop short of the bottom",
        "        let body_h = (h - toolbar_h).max(0.0);",
        "        let body_h = (h - toolbar_h - 20.0).max(0.0);",
        ["the_toolbar_is_above_the_columns_and_they_reach_the_bottom"],
    ),
    (
        "the sidebar takes a share of the window with no cap",
        "        let want_side = (w * SIDEBAR_SHARE).min(SIDEBAR_MAX);",
        "        let want_side = w * SIDEBAR_SHARE;",
        ["a_wider_window_widens_the_editor_not_the_chrome"],
    ),
    (
        "the list takes a share of the window with no cap",
        "        let want_list = (w * LIST_SHARE).min(LIST_MAX);",
        "        let want_list = w * LIST_SHARE;",
        ["a_wider_window_widens_the_editor_not_the_chrome"],
    ),
    (
        "the editor is given no floor to keep",
        "        let least_editor = font * 24.0;",
        "        let least_editor = 0.0;",
        [
            "the_editor_is_never_squeezed_out_by_the_columns",
            "a_narrow_window_drops_the_sidebar_before_the_list",
        ],
    ),
    (
        "the list gives way before the sidebar",
        "        let sidebar_w = if w - want_side - want_list >= least_editor {\n            want_side\n        } else {\n            0.0\n        };",
        "        let sidebar_w = want_side;",
        [
            "a_narrow_window_drops_the_sidebar_before_the_list",
            "a_window_too_narrow_for_either_column_is_all_editor",
        ],
    ),
    (
        "the text grows without limit in a tall window",
        "        let font = (h / LINES_PER_WINDOW).clamp(8.0, 16.0);",
        "        let font = h / LINES_PER_WINDOW;",
        ["a_taller_window_does_not_make_the_text_bigger_for_ever"],
    ),
    (
        "the small size is the same as the body size",
        "        let small = (font - 2.0).max(7.0);",
        "        let small = font;",
        ["the_text_sizes_keep_their_order"],
    ),
    (
        "a row is a fixed height whatever the text in it",
        "        let row = text::line_height(font, FontWeightHint::Regular) + pad;",
        "        let row = 8.0;",
        ["a_row_is_tall_enough_for_the_text_that_goes_in_it"],
    ),
    (
        "the list body starts at the top of the column, under the header",
        "            head.bottom(),\n            l.list.w,",
        "            l.list.y,\n            l.list.w,",
        ["the_list_header_sits_on_top_of_the_list_body"],
    ),
    (
        "the status bar is taller than the editor it is in",
        "            (text::line_height(l.tiny, FontWeightHint::Regular) + l.pad * 2.0).min(e.h - header_h);",
        "            text::line_height(l.tiny, FontWeightHint::Regular) + l.pad * 2.0;",
        ["the_editor_parts_survive_an_editor_with_no_room_in_it"],
    ),
    (
        "the code panel is measured from the top of the editor",
        "            (status.y - header.bottom()).max(0.0),",
        "            (status.y - e.y).max(0.0),",
        [
            "the_editor_is_a_header_a_code_panel_and_a_status_bar_in_that_order",
            "the_editor_parts_survive_an_editor_with_no_room_in_it",
        ],
    ),
    (
        "the code panel's capacity ignores the padding it is drawn inside",
        "        scroll_window::capacity(l.line, code.h - l.pad * 2.0)",
        "        scroll_window::capacity(l.line, code.h)",
        ["the_code_panel_holds_as_many_lines_as_it_draws"],
    ),
    # -- Clicking ------------------------------------------------------
    (
        "a row's hit box is the card rather than the row",
        "        f.hit(Target::Row(s.id), r);",
        "        f.hit(Target::Row(s.id), shrink(r, l.pad * 4.0));",
        ["a_row_is_where_its_own_title_is_drawn"],
    ),
    (
        "the rows all stack at the top of the list",
        "                    body.y + f32_from_usize(offset) * l.list_row,",
        "                    body.y,",
        ["a_row_is_where_its_own_title_is_drawn"],
    ),
    (
        "the panel swallows the rows drawn on it",
        "        f.hit(Target::List, body);\n",
        "",
        # Not `the_wheel_over_the_list_*`: that one goes in over a row, and a
        # row routes to the list too, so it passed with this box deleted.  The
        # strip below the last row is the only place the box is what answers.
        [
            "the_wheel_below_the_last_row_still_scrolls_the_list",
            "every_kind_of_control_is_on_the_first_screen",
        ],
    ),
    (
        "the star is not recorded, so the row takes its clicks",
        "        f.hit(Target::Star(s.id), star);\n",
        "",
        ["clicking_a_star_favourites_only_that_snippet"],
    ),
    (
        "a star click selects the row as well",
        "            Target::Star(id) => self.toggle_favorite(id),",
        "            Target::Star(id) => {\n                self.toggle_favorite(id);\n                self.select(id);\n            }",
        ["a_star_click_does_not_also_select_the_row"],
    ),
    (
        "clicking a row does not select it",
        "            Target::Row(id) => self.select(id),",
        "            Target::Row(_) => {}",
        ["clicking_a_row_selects_that_snippet"],
    ),
    (
        "a new snippet is not shown once it is made",
        "        // Picked so it is the one on screen: a create that leaves the old\n        // snippet showing looks like nothing happened.\n        self.select(id);\n        EventResult::Consumed\n    }",
        "        EventResult::Consumed\n    }",
        ["clicking_new_makes_a_snippet_and_shows_it"],
    ),
    (
        "a new snippet is always called Untitled",
        '        let title = if title.is_empty() { "Untitled" } else { title }.to_string();',
        '        let title = "Untitled".to_string();',
        ["a_new_snippet_takes_its_name_from_the_search_box"],
    ),
    (
        "a new snippet's language is not guessed from its name",
        '        let language = guess_language(&title, "");',
        "        let language = Language::PlainText;",
        ["a_new_snippet_takes_its_language_from_its_name"],
    ),
    (
        "a full library grows anyway",
        "        if self.snippets.len() >= MAX_SNIPPETS || content.len() > MAX_CONTENT_LEN {",
        "        if content.len() > MAX_CONTENT_LEN {",
        ["a_full_library_refuses_a_new_snippet_rather_than_pretending"],
    ),
    (
        "a full shelf of folders takes another",
        "        if self.folders.len() >= MAX_FOLDERS || name.is_empty() {\n            return None;\n        }",
        "        if name.is_empty() {\n            return None;\n        }",
        ["a_full_shelf_of_folders_refuses_another"],
    ),
    (
        "a new folder is always at the root",
        "            parent_id: self.selected_folder_id,",
        "            parent_id: None,",
        ["clicking_new_folder_makes_one_under_the_selected_folder"],
    ),
    (
        "an empty search box makes a folder with no name",
        '        let name = if name.is_empty() { "Folder" } else { name }.to_string();',
        "        let name = name.to_string();",
        ["a_new_folder_with_nothing_typed_still_gets_a_name"],
    ),
    (
        "a delete cross is drawn on every folder",
        "            if selected {\n                let cross = take_right(",
        "            if true {\n                let cross = take_right(",
        ["only_the_selected_folder_offers_to_be_deleted"],
    ),
    (
        "deleting a folder deletes what was in it",
        "            if snippet.folder_id == Some(id) {\n                snippet.folder_id = None;\n            }",
        "            let _ = id;",
        ["deleting_a_folder_keeps_its_snippets_and_moves_them_to_the_root"],
    ),
    (
        "a use is recorded against an id that is not a snippet's",
        "        let Some(snippet) = self.snippets.iter_mut().find(|s| s.id == id) else {\n            return EventResult::Ignored;\n        };\n        snippet.use_count = snippet.use_count.saturating_add(1);",
        "        if let Some(snippet) = self.snippets.iter_mut().find(|s| s.id == id) {\n            snippet.use_count = snippet.use_count.saturating_add(1);\n        }",
        ["an_id_that_is_not_a_snippet_takes_no_place_in_the_recent_list"],
    ),
    (
        "a second use leaves a second entry in the recent list",
        "        self.recently_used.retain(|&rid| rid != id);\n        self.recently_used.insert(0, id);",
        "        self.recently_used.insert(0, id);",
        ["using_a_snippet_twice_leaves_one_entry_in_the_recent_list"],
    ),
    (
        "the most recent goes on the end of the recent list",
        "        self.recently_used.insert(0, id);",
        "        self.recently_used.push(id);",
        ["the_recent_list_holds_the_most_recent_first"],
    ),
    (
        "the recent list grows without limit",
        "        self.recently_used.truncate(MAX_RECENT);",
        "        // truncate removed",
        ["the_recent_list_never_grows_past_its_limit"],
    ),
    (
        "a use does not count",
        "        snippet.use_count = snippet.use_count.saturating_add(1);",
        "        snippet.use_count = snippet.use_count;",
        ["clicking_use_counts_a_use_and_remembers_it"],
    ),
    (
        "using a template leaves no copy",
        "        let filled = self.filled_from_template(id);",
        "        let filled: Option<(String, String, Language)> = None;",
        ["using_a_template_leaves_a_filled_copy_behind"],
    ),
    (
        "an ordinary snippet is copied like a template",
        "        if !snippet.is_template {\n            return None;\n        }",
        "        if false {\n            return None;\n        }",
        ["using_an_ordinary_snippet_leaves_no_copy"],
    ),
    (
        "a delete takes the whole library",
        "        self.snippets.retain(|s| s.id != id);",
        "        self.snippets.clear();",
        ["clicking_delete_removes_the_selected_snippet_and_only_that_one"],
    ),
    (
        "a deleted snippet stays in the recent list",
        "        self.recently_used.retain(|&rid| rid != id);\n    }\n\n    /// Add a folder under the selected one",
        "    }\n\n    /// Add a folder under the selected one",
        ["a_deleted_snippet_leaves_the_recent_list_too"],
    ),
    (
        "clicking the picked folder again keeps it picked",
        "                self.selected_folder_id = (self.selected_folder_id != Some(id)).then_some(id);",
        "                self.selected_folder_id = Some(id);",
        ["clicking_a_folder_selects_it_and_clicking_it_again_lets_go"],
    ),
    (
        "a twisty opens every folder",
        "                if let Some(folder) = self.folders.iter_mut().find(|f| f.id == id) {\n                    folder.expanded = !folder.expanded;\n                }",
        "                let _ = id;\n                for folder in &mut self.folders {\n                    folder.expanded = !folder.expanded;\n                }",
        ["a_twisty_opens_and_shuts_its_own_folder_and_no_other"],
    ),
    (
        "a shut folder still shows its children",
        "            if folder.expanded {\n                self.walk_folders(Some(folder.id), depth.saturating_add(1), out);\n            }",
        "            self.walk_folders(Some(folder.id), depth.saturating_add(1), out);",
        ["shutting_a_folder_hides_its_children_from_the_tree"],
    ),
    (
        # There used to be a row here for a `depth >= MAX_FOLDER_DEPTH`
        # bail-out.  It went, with the guard: a folder has one parent and the
        # walk enters from `None`, so a cycle is never reached and the guard
        # only ever truncated real nesting.  What is left to mutate is the
        # depth the walk carries, which is the tree's indent.
        "the tree is flat",
        "                self.walk_folders(Some(folder.id), depth.saturating_add(1), out);",
        "                self.walk_folders(Some(folder.id), depth, out);",
        ["a_deeply_nested_folder_is_still_in_the_tree"],
    ),
    (
        "a twisty is drawn where there is nothing to open",
        "        if self.has_children(id) {",
        "        if true {",
        ["a_twisty_is_drawn_only_where_there_is_something_to_open"],
    ),
    (
        "the cross is offered when the search box is empty",
        "        if !self.search_query.is_empty() {",
        "        if true {",
        ["there_is_no_cross_when_there_is_nothing_to_clear"],
    ),
    (
        "the cross does not clear the box",
        "            Target::ClearSearch => {\n                self.search_query.clear();",
        "            Target::ClearSearch => {\n                self.list_scroll = 0;",
        ["clicking_the_cross_empties_the_search_box"],
    ),
    (
        "the overlay's backdrop is not a control",
        "        f.hit(Target::CloseStats, l.window);\n",
        "",
        ["clicking_stats_opens_the_overlay_and_clicking_outside_shuts_it", "the_overlay_covers_everything_behind_it"],
    ),
    (
        "a press on a panel body is a press on the panel",
        "            Target::List | Target::Code => return EventResult::Ignored,",
        "            Target::List | Target::Code => {}",
        ["a_click_on_a_panel_body_is_not_a_click_on_a_thing"],
    ),
    # -- The keyboard --------------------------------------------------
    (
        "down walks two rows at a time",
        "            Key::Down => self.move_selection(1),\n            Key::PageUp",
        "            Key::Down => self.move_selection(2),\n            Key::PageUp",
        ["down_walks_the_list_and_stops_at_the_end"],
    ),
    (
        "up walks forwards",
        "            Key::Up => self.move_selection(-1),\n            Key::Down => self.move_selection(1),\n            Key::PageUp",
        "            Key::Up => self.move_selection(1),\n            Key::Down => self.move_selection(1),\n            Key::PageUp",
        ["up_walks_back_and_stops_at_the_top"],
    ),
    (
        "the walk runs off the end of the list",
        "            Some(row) => scroll_window::shift(row, delta).min(last),",
        "            Some(row) => scroll_window::shift(row, delta),",
        # Not `down_walks_the_list_and_stops_at_the_end`: without the clamp the
        # walk names a row past the end, `ids.get` refuses it, and the
        # selection stays where it was -- which is what that test asks for.
        # The clamp is only visible where re-picking the row *does* something,
        # i.e. where the row it re-picks is off screen (lesson 70).
        ["walking_past_the_end_brings_the_end_back_on_screen"],
    ),
    (
        "up before anything is picked starts at the top",
        "            None if delta < 0 => last,",
        "            None if delta < 0 => 0,",
        ["up_with_nothing_selected_starts_at_the_bottom"],
    ),
    (
        "End goes to the top",
        "        let row = if last { bottom } else { 0 };",
        "        let row = 0;",
        ["home_and_end_reach_the_ends"],
    ),
    (
        "Home goes to the bottom",
        "            Key::Home => self.select_end(false),",
        "            Key::Home => self.select_end(true),",
        ["home_and_end_reach_the_ends"],
    ),
    (
        "a page is one row",
        "        isize::try_from(capacity.saturating_sub(1).max(1)).unwrap_or(1)",
        "        1",
        ["a_page_is_more_than_a_row_and_no_more_than_the_list_holds"],
    ),
    (
        "a page is more than the panel holds",
        "        isize::try_from(capacity.saturating_sub(1).max(1)).unwrap_or(1)\n    }",
        "        isize::try_from(capacity.saturating_add(4).max(1)).unwrap_or(1)\n    }",
        ["a_page_is_more_than_a_row_and_no_more_than_the_list_holds"],
    ),
    (
        "page up goes down",
        "            Key::PageUp => self.move_selection(self.page().saturating_neg()),",
        "            Key::PageUp => self.move_selection(self.page()),",
        ["page_down_moves_a_page_and_page_up_brings_it_back"],
    ),
    (
        "an empty list still answers the arrows",
        # The empty list reaches `ids.get(0)` of an empty vector and is refused
        # there; there is no separate emptiness guard to delete (there was one,
        # and it was dead -- lesson 51).  So mutate the refusal itself.
        "        let Some(&id) = ids.get(next) else {\n            return EventResult::Ignored;\n        };",
        "        let Some(&id) = ids.get(next) else {\n            return EventResult::Consumed;\n        };",
        ["moving_the_selection_on_an_empty_list_is_ignored"],
    ),
    (
        "the walk does not bring the row on screen",
        "        self.select(id);\n        self.scroll_row_into_view(next);",
        "        self.select(id);",
        ["walking_the_list_scrolls_the_row_into_view"],
    ),
    (
        "a row below the panel is brought to the top instead of the bottom",
        "            self.list_scroll = row.saturating_sub(capacity.saturating_sub(1));",
        "            self.list_scroll = row;",
        ["walking_the_list_scrolls_the_row_into_view"],
    ),
    (
        "Enter does nothing",
        "            Key::Enter => self.press(Target::Use),",
        "            Key::Enter => EventResult::Ignored,",
        ["enter_uses_the_selected_snippet"],
    ),
    (
        "Delete does nothing",
        "            Key::Delete => self.press(Target::Delete),",
        "            Key::Delete => EventResult::Ignored,",
        ["delete_deletes_the_selected_snippet"],
    ),
    (
        "F favourites nothing",
        "                self.toggle_favorite(id);\n                EventResult::Consumed",
        "                let _ = id;\n                EventResult::Consumed",
        ["f_favourites_the_selected_snippet"],
    ),
    (
        "S does not open the statistics",
        "            Key::S => self.press(Target::Stats),",
        "            Key::S => EventResult::Ignored,",
        ["s_opens_the_statistics_and_closes_them_again"],
    ),
    (
        "N makes nothing",
        "            Key::N => self.press(Target::New),",
        "            Key::N => EventResult::Ignored,",
        ["n_makes_a_snippet", "typing_with_the_search_box_shut_still_works_the_shortcuts"],
    ),
    (
        "O does not step the sort order",
        "            Key::O => self.press(Target::Sort),",
        "            Key::O => EventResult::Ignored,",
        ["o_steps_the_sort_order"],
    ),
    (
        "Escape does not shut the statistics",
        "                Key::Escape | Key::Enter | Key::S => {\n                    self.show_stats = false;",
        "                Key::Enter | Key::S => {\n                    self.show_stats = false;",
        ["escape_shuts_the_statistics"],
    ),
    (
        "keys reach the library behind the overlay",
        "        if self.show_stats {\n            return match ev.key {",
        "        if false {\n            return match ev.key {",
        ["the_statistics_overlay_swallows_the_keys_behind_it"],
    ),
    (
        "Shift-Tab steps forwards like Tab",
        "                self.cycle_view(ev.modifiers.shift);",
        "                self.cycle_view(false);",
        ["tab_and_shift_tab_step_the_sidebar_the_two_ways"],
    ),
    (
        "the view cycle skips a view",
        "    fn next(self) -> Self {\n        match self {\n            Self::Folders => Self::Tags,",
        "    fn next(self) -> Self {\n        match self {\n            Self::Folders => Self::Languages,",
        ["tab_reaches_every_view_and_comes_back_round"],
    ),
    (
        "changing view keeps the old view's scroll",
        "            self.sidebar_view.next()\n        };\n        self.list_scroll = 0;",
        "            self.sidebar_view.next()\n        };",
        ["changing_view_puts_the_list_back_at_the_top"],
    ),
    (
        "a key nothing is bound to is acted on",
        "            _ => EventResult::Ignored,\n        }\n    }\n\n    /// A keystroke while the search box has the keyboard.",
        "            _ => EventResult::Consumed,\n        }\n    }\n\n    /// A keystroke while the search box has the keyboard.",
        ["a_key_nothing_is_bound_to_is_ignored"],
    ),
    (
        "a key that is not pressed is acted on",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        # Not `a_key_nothing_is_bound_to_is_ignored`: that one presses a key
        # with nothing bound to it, and a press is the half this guard lets
        # through either way.  Only a release tells the two apart.
        ["a_key_coming_back_up_is_not_a_second_keystroke"],
    ),
    # -- The search box ------------------------------------------------
    (
        "slash does not reach the search box",
        "            Key::Slash => {\n                self.search_focus = true;",
        "            Key::Slash => {\n                self.search_focus = false;",
        ["slash_and_ctrl_f_both_reach_the_search_box"],
    ),
    (
        "ctrl-F does not reach the search box",
        "            Key::F if ev.modifiers.ctrl => {\n                self.search_focus = true;",
        "            Key::F if ev.modifiers.ctrl => {\n                self.search_focus = false;",
        ["slash_and_ctrl_f_both_reach_the_search_box"],
    ),
    (
        "the box takes whatever text arrives, control characters and all",
        "                self.search_query.extend(ev.typed());",
        "                self.search_query.push_str(&ev.text);",
        # Not `the_key_that_opens_the_search_box_is_not_also_typed_into_it`:
        # that one presses `/` with the box shut, so nothing is appended by
        # either spelling.  The two differ only on a keystroke whose text is
        # a control character.
        ["the_search_box_takes_the_text_a_key_types_and_not_the_rest"],
    ),
    (
        "a keystroke that types nothing still types",
        "                if !ev.types_text() {\n                    return EventResult::Ignored;\n                }",
        "                if false {\n                    return EventResult::Ignored;\n                }",
        # Not `enter_and_escape_both_leave_the_search_box`: Enter and Escape
        # are matched by an arm above this one and never reach it.
        ["the_search_box_takes_the_text_a_key_types_and_not_the_rest"],
    ),
    (
        "backspace takes a byte rather than a character",
        "                if self.search_query.pop().is_none() {\n                    return EventResult::Ignored;\n                }",
        "                if self.search_query.is_empty() {\n                    return EventResult::Ignored;\n                }\n                let cut = self.search_query.len().saturating_sub(1);\n                self.search_query = self.search_query.chars().collect::<String>()[..0].to_string()\n                    + &self.search_query.chars().take(cut).collect::<String>();",
        ["backspace_takes_back_a_character_not_a_byte"],
    ),
    (
        "escape does not leave the search box",
        "            Key::Escape | Key::Enter => {\n                self.search_focus = false;",
        "            Key::Enter => {\n                self.search_focus = false;",
        ["enter_and_escape_both_leave_the_search_box"],
    ),
    (
        "the arrows stop working while the box has the keyboard",
        "            Key::Up => self.move_selection(-1),\n            Key::Down => self.move_selection(1),\n            _ => {",
        "            _ => {",
        ["the_arrows_still_walk_the_list_while_the_search_box_has_the_keyboard"],
    ),
    (
        "escape with an empty query is consumed anyway",
        "                if self.search_query.is_empty() {\n                    return EventResult::Ignored;\n                }\n                self.press(Target::ClearSearch)",
        "                self.press(Target::ClearSearch)",
        ["escape_with_nothing_to_clear_is_ignored"],
    ),
    (
        "typing leaves the list where the old query had scrolled it",
        "                self.search_query.extend(ev.typed());\n                self.list_scroll = 0;",
        "                self.search_query.extend(ev.typed());",
        ["a_narrowed_query_puts_the_list_back_at_the_top"],
    ),
    # -- The wheel -----------------------------------------------------
    (
        "the wheel over the list scrolls the code",
        "            Target::List | Target::Row(_) | Target::Star(_) => {",
        "            Target::Code | Target::Row(_) | Target::Star(_) => {",
        [
            "the_wheel_over_the_list_scrolls_the_list_and_not_the_code",
            "the_wheel_over_the_code_scrolls_the_code_and_not_the_list",
        ],
    ),
    (
        "the wheel runs past the last row",
        "    scroll_window::shift(offset, rows).min(total.saturating_sub(capacity))",
        "    scroll_window::shift(offset, rows)",
        ["the_wheel_stops_where_the_last_row_is_at_the_bottom", "a_list_that_fits_does_not_scroll_at_all"],
    ),
    (
        "a fraction of a notch is dropped rather than banked",
        "        let rows = self.wheel.rows(dy);",
        "        let rows = if dy.abs() < 1.0 { 0 } else { self.wheel.rows(dy) };",
        ["the_wheel_banks_the_fractions_a_trackpad_sends"],
    ),
    (
        "the wheel over nothing scrolls the list",
        "            _ => return EventResult::Ignored,\n        }\n        EventResult::Consumed\n    }\n\n    /// Act on a keystroke.",
        "            _ => self.list_scroll = self.list_scroll.saturating_add(1),\n        }\n        EventResult::Consumed\n    }\n\n    /// Act on a keystroke.",
        ["the_wheel_over_nothing_scrolls_nothing"],
    ),
    (
        "picking a snippet keeps the last one's scroll",
        "        self.selected_snippet_id = Some(id);\n        self.code_scroll = 0;",
        "        self.selected_snippet_id = Some(id);",
        ["picking_a_snippet_starts_it_at_its_first_line"],
    ),
    # -- What is drawn -------------------------------------------------
    (
        "a label is drawn with no width to stop at",
        "        max_width: Some(limit),",
        "        max_width: None,",
        ["every_string_is_drawn_with_a_width_to_stop_at"],
    ),
    (
        "a centred label may run past its box",
        "    push_text(f, l, r.x + (r.w - w) / 2.0, r.y + (r.h - lh) / 2.0, w);",
        "    push_text(f, l, r.x + (r.w - w) / 2.0, r.y + (r.h - lh) / 2.0, r.w);",
        ["nothing_is_drawn_off_the_right_edge", "the_overlay_fits_in_the_window_it_covers"],
    ),
    (
        "the status line counts something with nothing selected",
        '            let lines = format!("{} lines", s.content.lines().count());',
        '            let lines = format!("{} lines", s.content.lines().count().saturating_add(1));',
        ["the_status_line_says_how_long_the_selected_snippet_is"],
    ),
    (
        "a row draws every tag it has",
        "                .take(TAGS_ON_A_ROW)",
        "                .take(usize::MAX)",
        ["a_list_row_shows_no_more_tags_than_fit_on_it"],
    ),
    (
        "the overlay drops a statistic",
        '            ("Total Lines", stats.total_lines.to_string()),\n',
        "",
        ["the_overlay_lists_every_statistic"],
    ),
    (
        "the overlay lists every language there is",
        "            .take(LANGUAGES_ON_OVERLAY)",
        "            .take(usize::MAX)",
        ["the_overlay_lists_no_more_languages_than_it_has_room_for"],
    ),
    (
        "the overlay is a fixed size whatever window it is in",
        "        let w = wanted_w.min(l.window.w * OVERLAY_SHARE);\n        let h = wanted_h.min(l.window.h * OVERLAY_SHARE);",
        "        let w = 400.0;\n        let h = 300.0;",
        ["the_overlay_fits_in_the_window_it_covers"],
    ),
    (
        "an empty list shows nothing rather than saying so",
        "        if filtered.is_empty() {\n            label_centred(",
        "        if false {\n            label_centred(",
        ["an_empty_list_says_so_rather_than_showing_nothing"],
    ),
    (
        "the empty editor says nothing",
        "                    text: EMPTY_SUBLINE,",
        '                    text: "",',
        ["an_empty_editor_says_what_to_do_next"],
    ),
    (
        "the template badge is on everything",
        "        if s.is_template {\n            let pill = inset_y(",
        "        if true {\n            let pill = inset_y(",
        ["the_template_badge_is_only_on_a_template"],
    ),
    (
        "the editor header does not name the extension",
        '        let lang = format!("{} .{}", s.language.name(), s.language.extension());',
        "        let lang = s.language.name().to_string();",
        ["the_editor_header_names_the_language_and_the_extension_it_is_guessed_from"],
    ),
    (
        "the gutter numbers the lines from zero",
        "                .saturating_add(1)\n                .to_string();",
        "                .to_string();",
        ["the_gutter_numbers_the_lines_from_one"],
    ),
    # -- The window ----------------------------------------------------
    (
        "a resize is not remembered",
        "            app.resize(f32_from_u32(*width), f32_from_u32(*height));",
        "            let _ = (width, height);",
        ["a_resize_is_what_the_next_click_is_read_against", "a_click_lands_where_the_window_it_was_resized_to_put_the_control"],
    ),
    (
        "keys do not reach the app",
        "        Event::Key(ev) => app.handle_key(ev),",
        "        Event::Key(_) => EventResult::Ignored,",
        ["the_window_forwards_the_events_it_has_a_use_for"],
    ),
    (
        "a tick is treated as a change",
        "        _ => EventResult::Ignored,\n    }\n}\n\nimpl WindowApp for App",
        "        _ => EventResult::Consumed,\n    }\n}\n\nimpl WindowApp for App",
        ["the_window_ignores_the_events_it_has_no_use_for"],
    ),
    (
        "the close button does not close",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        if false {\n            return Response::Exit;\n        }",
        ["the_close_button_ends_the_program"],
    ),
    (
        "an event that changed something does not ask for a redraw",
        "            EventResult::Consumed => Response::Redraw,",
        "            EventResult::Consumed => Response::Idle,",
        ["a_keystroke_that_changes_nothing_does_not_ask_for_a_redraw"],
    ),
    (
        "an event that changed nothing asks for a redraw anyway",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["a_keystroke_that_changes_nothing_does_not_ask_for_a_redraw"],
    ),
    (
        "the size a frame is drawn at is thrown away",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["rendering_lays_the_frame_out_at_the_size_it_is_given"],
    ),
    (
        "the window has no name",
        "        TOOLBAR_TITLE.to_string()",
        "        String::new()",
        ["the_window_is_named_and_identified"],
    ),
    (
        "the window opens at a size nothing was designed for",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (640, 480)",
        ["the_window_is_named_and_identified"],
    ),
    (
        "the window asks to be woken for an animation it does not have",
        "    fn on_event(&mut self, event: &Event) -> Response {",
        "    fn tick_interval(&self) -> Option<std::time::Duration> {\n"
        "        Some(std::time::Duration::from_millis(16))\n"
        "    }\n\n"
        "    fn on_event(&mut self, event: &Event) -> Response {",
        ["nothing_here_animates_so_nothing_here_ticks"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "snippets", timeout=240, only=sys.argv[1:] or None))

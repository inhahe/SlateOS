"""Mutation test for the feed reader's pointer layer, its scrolling and the
fixes they exposed.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The reader drew a sidebar, an article list, an article, a filter button, a
sort button, a search box and a "Refresh All" button, and handled no pointer
event (known-issues, TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-
CLICKED).  None of its three panes scrolled; its notice that it cannot fetch
was painted over by its own title bar; "Refresh All" could refresh nothing;
and feed discovery was written and called by nothing.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

EACH = "every_change_to_the_subscriptions_is_kept"
MARKS = "every_mark_change_is_kept"
REMOVED = "a_removed_feeds_marks_go_with_it"
BACK = "subscriptions_folders_and_marks_come_back_next_time"
TWICE = "an_opml_imported_twice_adds_nothing_and_an_empty_folder_survives"
BROKEN = "a_kept_file_that_cannot_be_read_is_left_as_it_is"
FAILING = "closing_while_a_save_fails_asks_first"
DISCARD = "discard_leaves_and_cancel_stays"
ROUND = "marks_read_back_as_they_were_written_whatever_the_text"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "Open opens nothing",
        "            Target::OpenFile => self.open_file_dialog(false),",
        "            Target::OpenFile => {}",
        ["refresh_is_not_offered_and_open_and_export_are"],
    ),
    (
        "the filter button does nothing",
        "            Target::FilterButton => {\n                self.filter_mode = self.filter_mode.next();",
        "            Target::FilterButton => {\n                let _ = self.filter_mode;",
        ["the_filter_and_sort_buttons_step_through_their_choices"],
    ),
    (
        "the search box does not take the keyboard",
        "            Target::SearchBox => self.search_active = true,",
        "            Target::SearchBox => {}",
        ["the_search_box_takes_the_keyboard_and_a_press_elsewhere_gives_it_back"],
    ),
    (
        "the search box keeps the keyboard after a press elsewhere",
        "        let unfocused =\n"
        "            target != Some(Target::SearchBox) && std::mem::take(&mut self.search_active);",
        "        let unfocused = false;",
        ["the_search_box_takes_the_keyboard_and_a_press_elsewhere_gives_it_back"],
    ),
    (
        "a folder's mark does not open or close it",
        "                let Some(folder) = self.folders.iter_mut().find(|f| f.id == id) else {\n"
        "                    return EventResult::Ignored;\n                };\n"
        "                folder.is_expanded = !folder.is_expanded;",
        "                let _ = id;",
        ["a_sidebar_row_selects_and_a_folder_mark_opens_and_closes_it"],
    ),
    (
        "a star in the list stars nothing",
        "                self.toggle_star();\n            }\n            Target::ContentRead => self.toggle_read(),",
        "            }\n            Target::ContentRead => self.toggle_read(),",
        ["the_dot_and_the_star_mark_their_article"],
    ),
    (
        "the article's read badge does nothing",
        "            Target::ContentRead => self.toggle_read(),",
        "            Target::ContentRead => {}",
        ["the_badges_on_the_article_mark_it_too"],
    ),
    (
        "the help hint raises nothing",
        "            Target::HelpHint => self.show_help = true,",
        "            Target::HelpHint => {}",
        ["the_help_hint_raises_the_list_and_a_press_puts_either_overlay_away"],
    ),
    (
        "a removal cannot be confirmed by its button",
        "                Some(Target::PromptYes) => self.answer_yes(),",
        "                Some(Target::PromptYes) => EventResult::Consumed,",
        ["a_removal_is_answered_by_its_buttons_and_nothing_else"],
    ),
    (
        "a press behind the question acts",
        "        if self.prompt.is_some() || self.text_entry.is_some() {\n            return match target {",
        "        if false {\n            return match target {",
        ["a_removal_is_answered_by_its_buttons_and_nothing_else"],
    ),
    (
        "a folder chip files nothing",
        "                    if let Some(Prompt::MoveFeed(id)) = self.prompt.take() {\n"
        "                        self.finish_move(id, folder);",
        "                    if let Some(Prompt::MoveFeed(id)) = self.prompt.take() {\n"
        "                        let _ = (id, folder);",
        ["a_feed_is_filed_by_pressing_a_folder"],
    ),
    (
        "OK does not accept the typed name",
        "                Some(Target::EntryAccept) => {\n                    self.commit_text_entry();",
        "                Some(Target::EntryAccept) => {\n                    self.text_entry = None;",
        ["a_typed_name_is_accepted_or_cancelled_by_its_buttons"],
    ),
    (
        "the search box records no hit box",
        "        cmds.hit(Target::SearchBox, Rect::new(search_x, y + 6.0, 240.0, 24.0));",
        "",
        ["the_search_box_takes_the_keyboard_and_a_press_elsewhere_gives_it_back"],
    ),
    (
        "the selection walks off the bottom of the list",
        "            self.selected_article_index = next;\n            self.content_scroll_offset = 0.0;\n"
        "            self.keep_article_visible();",
        "            self.selected_article_index = next;\n            self.content_scroll_offset = 0.0;",
        ["the_article_list_follows_the_selection_and_scrolls_under_the_wheel"],
    ),
    (
        "the sidebar does not follow its selection",
        "        self.article_scroll_offset = 0.0;\n        self.keep_sidebar_visible();\n    }\n\n"
        "    /// Move the sidebar selection to the next or previous feed",
        "        self.article_scroll_offset = 0.0;\n    }\n\n"
        "    /// Move the sidebar selection to the next or previous feed",
        ["the_sidebar_scrolls_and_follows_its_selection"],
    ),
    (
        "nothing scrolls under the wheel",
        "        *offset = (*offset + px).clamp(0.0, limit);",
        "        *offset = offset.clamp(0.0, limit);",
        [
            "the_article_list_follows_the_selection_and_scrolls_under_the_wheel",
            "a_long_article_scrolls_by_the_wheel_and_by_page_down",
            "the_sidebar_scrolls_and_follows_its_selection",
        ],
    ),
    (
        "Page Down does not read on",
        "            Key::PageDown | Key::PageUp if self.active_pane == ActivePane::ContentView => {",
        "            Key::PageDown | Key::PageUp if false => {",
        ["a_long_article_scrolls_by_the_wheel_and_by_page_down"],
    ),
    (
        "the notice is not in the list",
        "        if self.articles.is_empty() {\n            // Nothing at all to read",
        "        if false {\n            // Nothing at all to read",
        ["the_window_says_it_cannot_fetch"],
    ),
    (
        "a saved web page subscribes to nothing",
        '        if head.contains("<html") {',
        '        if head.contains("<html") && false {',
        ["a_saved_web_page_subscribes_to_the_feeds_it_links_to"],
    ),
    (
        "the pointer lights nothing",
        "                self.hover = over;\n                EventResult::Consumed",
        "                let _ = over;\n                EventResult::Consumed",
        ["the_pointer_lights_the_button_it_is_over"],
    ),
    # ---- what is kept between sessions ----
    (
        "a new folder is not counted",
        "        self.folders.push(Folder::new(id, name));\n        self.subscriptions_changed();",
        "        self.folders.push(Folder::new(id, name));",
        [EACH],
    ),
    (
        "a new feed is not counted",
        "        self.feeds.push(feed);\n        self.subscriptions_changed();",
        "        self.feeds.push(feed);",
        [EACH],
    ),
    (
        "a removed feed is not counted",
        "        self.articles.retain(|a| a.feed_id != feed_id);\n        self.subscriptions_changed();",
        "        self.articles.retain(|a| a.feed_id != feed_id);",
        [EACH],
    ),
    (
        "a removed feed's marks stay",
        "        self.subscriptions_changed();\n        self.marks_revision = self.marks_revision.wrapping_add(1);\n    }",
        "        self.subscriptions_changed();\n    }",
        [REMOVED],
    ),
    (
        "a rename is not counted",
        "            feed.title = new_name.to_string();\n            self.subscriptions_changed();",
        "            feed.title = new_name.to_string();",
        [EACH],
    ),
    (
        "a removed folder is not counted",
        "        self.folders.retain(|f| f.id != folder_id);\n        self.subscriptions_changed();",
        "        self.folders.retain(|f| f.id != folder_id);",
        [EACH],
    ),
    (
        "a move is not counted",
        "            feed.folder_id = folder_id;\n            self.subscriptions_changed();",
        "            feed.folder_id = folder_id;",
        [EACH],
    ),
    (
        "a feed's own title is not counted",
        "        if renamed {\n            self.subscriptions_changed();\n        }",
        "",
        [EACH],
    ),
    (
        "a read toggle is not kept",
        "            article.is_read = !article.is_read;\n            self.remember_marks(idx);",
        "            article.is_read = !article.is_read;",
        [MARKS],
    ),
    (
        "a star toggle is not kept",
        "            article.is_starred = !article.is_starred;\n            self.remember_marks(idx);",
        "            article.is_starred = !article.is_starred;",
        [MARKS],
    ),
    (
        "marking all read is not kept",
        "                article.is_read = true;\n                self.remember_marks(idx);",
        "                article.is_read = true;",
        [MARKS],
    ),
    (
        "marking all read counts what was read already",
        "            if let Some(article) = self.articles.get_mut(idx)\n                && !article.is_read\n            {",
        "            if let Some(article) = self.articles.get_mut(idx) {",
        [MARKS],
    ),
    (
        "a feed read again does not get its marks back",
        "                    article.is_read = kept.read;\n                    article.is_starred = kept.starred;",
        "",
        [BACK],
    ),
    (
        "an OPML list imported twice doubles its feeds",
        "            if self.feeds.iter().any(|f| &f.url == url) {\n                return 0;\n            }",
        "",
        [TWICE],
    ),
    (
        "an empty folder is not a folder",
        "        } else if !outline.children.is_empty() || !outline.text.is_empty() {",
        "        } else if !outline.children.is_empty() {",
        [TWICE, BACK],
    ),
    (
        "a feed read from a file is not read again at start",
        "        for file in &files {\n            self.read_any_file(Path::new(file));\n        }",
        "",
        [BACK],
    ),
    (
        "what was just read is written again at once",
        "        self.kept_subs = self.subs_revision;\n        self.kept_marks = self.marks_revision;\n    }",
        "    }",
        [BACK],
    ),
    (
        "a kept file that cannot be read is saved over",
        "            Err(why) => {\n                self.persist = false;\n                self.store_error = Some(why);\n                return;\n            }",
        "            Err(why) => {\n                self.store_error = Some(why);\n                return;\n            }",
        [BROKEN],
    ),
    (
        "unreadable marks are saved over",
        "                Err(why) => {\n                    self.persist = false;\n                    self.store_error = Some(refused(marks, why));",
        "                Err(why) => {\n                    self.store_error = Some(refused(marks, why));",
        [BROKEN],
    ),
    (
        "a failed save is not said",
        "        self.store_error = failed;\n    }",
        "        self.store_error = None;\n        let _ = failed;\n    }",
        [FAILING],
    ),
    (
        "why nothing is kept is not drawn",
        "        } else if let Some(error) = &self.store_error {",
        "        } else if let Some(error) = None::<&String> {",
        [BROKEN, FAILING],
    ),
    (
        "closing does not ask",
        "        self.keep();\n        if !self.unkept() {\n            return true;\n        }",
        "        self.keep();\n        if true {\n            return true;\n        }",
        [FAILING],
    ),
    (
        "a key under the question reaches the reader",
        "            if let Some(choice) = question.handle(event) {\n                self.question = None;\n                self.answer(choice);\n            }\n            return EventResult::Consumed;",
        "            if let Some(choice) = question.handle(event) {\n                self.question = None;\n                self.answer(choice);\n                return EventResult::Consumed;\n            }",
        [FAILING],
    ),
    (
        "Save leaves while the save still fails",
        "                self.running = self.unkept();",
        "                self.running = false;",
        [FAILING],
    ),
    (
        "Discard stays",
        "            Choice::Discard => self.running = false,",
        "            Choice::Discard => {}",
        [DISCARD],
    ),
    (
        "an unsubscribed feed's marks are written",
        "        if !subscribed.iter().any(|f| &f.url == feed) {\n            continue;\n        }",
        "",
        [ROUND],
    ),
    (
        "a marks file in a later format is read",
        "        Some(first) if first.starts_with(\"slateos-feed-marks\\t\") => {",
        "        Some(first) if first.starts_with(\"slateos-feed-marks-never\\t\") => {",
        [ROUND],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "rssreader", timeout=900, only=only))

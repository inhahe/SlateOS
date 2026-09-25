"""Mutation test for the torrent client's pointer layer, its magnet dialog,
its search, its labels and its file priorities.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Nothing answered the pointer (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED); the notice
saying this client cannot transfer was painted over; the list did not scroll;
labels, the search and magnet links could not be reached; a skipped file was
downloaded all the same; and a transfer set going with no network ticked seven
times a second for good.

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
    # -- the file priorities ---------------------------------------------------------------------------------
    (
        "a file's priority reaches no piece",
        "            self.pieces.set_priority(piece, wanted);",
        "            let _ = (piece, wanted);",
        ["a_skipped_file_is_not_downloaded"],
    ),
    (
        "a shared piece takes the lower priority",
        "                .max()\n"
        "                .unwrap_or(5);",
        "                .min()\n"
        "                .unwrap_or(5);",
        ["a_skipped_file_is_not_downloaded"],
    ),
    (
        "a press on a priority changes nothing",
        "                t.set_file_priority(index, next);",
        "                let _ = (index, next);",
        ["a_skipped_file_is_not_downloaded"],
    ),
    # -- the clock ---------------------------------------------------------------------------------------------
    (
        "a stalled download keeps the machine awake",
        "            .any(|t| t.state == TorrentState::Downloading && !t.peers.is_empty())",
        "            .any(|t| t.state == TorrentState::Downloading)",
        [
            "a_stalled_download_says_there_is_no_network",
            "a_downloading_torrent_takes_pieces_until_it_is_done",
        ],
    ),
    (
        "a stalled download says Downloading",
        "            TorrentState::Downloading if torrent.peers.is_empty() => {",
        "            TorrentState::Downloading if false => {",
        ["a_stalled_download_says_there_is_no_network"],
    ),
    # -- the notice ---------------------------------------------------------------------------------------------
    (
        "the notice is painted over again",
        "                y: y + 6.0 + i as f32 * 14.0,",
        "                y: 4.0 + i as f32 * 14.0,",
        ["the_notice_is_drawn_where_it_can_be_read"],
    ),
    # -- the keys ------------------------------------------------------------------------------------------------
    (
        "the magnet dialog lets keys through",
        "        if self.show_add_dialog {\n"
        "            return self.handle_dialog_key(key);",
        "        if false {\n"
        "            return self.handle_dialog_key(key);",
        ["a_press_behind_the_magnet_dialog_reaches_nothing", "a_magnet_link_is_added_from_the_dialog"],
    ),
    (
        "the search box lets keys through",
        "        if self.search_active {\n"
        "            return self.handle_search_key(key);",
        "        if false {\n"
        "            return self.handle_search_key(key);",
        ["the_search_box_filters_and_escape_clears_it"],
    ),
    (
        "Ctrl+U opens nothing",
        "                Key::U => {\n"
        "                    self.open_magnet_dialog();",
        "                Key::U => {\n"
        "                    let _ = 0;",
        ["a_magnet_link_is_added_from_the_dialog"],
    ),
    (
        "L labels nothing",
        "            Key::L => self.cycle_label(),",
        "            Key::L => EventResult::Ignored,",
        ["a_label_can_be_given_and_chosen_by", "every_advertised_key_does_something"],
    ),
    (
        "a search finds nothing by name",
        "                    t.name.to_lowercase().contains(&q) || t.label.to_lowercase().contains(&q)",
        "                    t.label.to_lowercase().contains(&q)",
        ["the_search_box_filters_and_escape_clears_it"],
    ),
    (
        "Down walks the order things were added in",
        "        let ids: Vec<u32> = self.filtered_torrents().iter().map(|t| t.id).collect();\n"
        "        if ids.is_empty() {",
        "        let ids: Vec<u32> = self.torrents.iter().map(|t| t.id).collect();\n"
        "        if ids.is_empty() {",
        ["the_transfer_list_scrolls_and_follows_the_selection"],
    ),
    # -- the pointer ----------------------------------------------------------------------------------------------
    (
        "Pause pauses nothing",
        "            Target::Pause => {\n"
        "                let Some(id) = self.selected_torrent else {\n"
        "                    return EventResult::Ignored;\n"
        "                };\n"
        "                self.pause_torrent(id);",
        "            Target::Pause => {\n"
        "                let Some(id) = self.selected_torrent else {\n"
        "                    return EventResult::Ignored;\n"
        "                };\n"
        "                let _ = id;",
        ["the_toolbar_answers_the_pointer"],
    ),
    (
        "a filter press does nothing",
        "            Target::Filter(filter) => return self.set_filter(filter),",
        "            Target::Filter(_) => return EventResult::Ignored,",
        ["the_sidebar_filters_and_labels_answer_the_pointer"],
    ),
    (
        "a second label press keeps the label",
        "                self.selected_label = if self.selected_label.as_ref() == Some(&label) {",
        "                self.selected_label = if false {",
        ["the_sidebar_filters_and_labels_answer_the_pointer"],
    ),
    (
        "a second press on a column head does not reverse it",
        "                if column == self.sort_column {\n"
        "                    self.sort_ascending = !self.sort_ascending;",
        "                if false {\n"
        "                    self.sort_ascending = !self.sort_ascending;",
        ["a_column_head_sorts_and_a_second_press_reverses"],
    ),
    (
        "a second press on a row does not show its details",
        "                if self.selected_torrent == Some(id) {\n"
        "                    self.active_tab = Tab::Details;",
        "                if false {\n"
        "                    self.active_tab = Tab::Details;",
        ["a_row_press_chooses_and_a_second_shows_its_details"],
    ),
    (
        "the details' label does nothing",
        "            Target::CycleLabel => return self.cycle_label(),",
        "            Target::CycleLabel => return EventResult::Ignored,",
        ["a_label_can_be_given_and_chosen_by"],
    ),
    (
        "Add does not add the typed magnet",
        "            Target::MagnetAdd => self.add_typed_magnet(),",
        "            Target::MagnetAdd => self.close_magnet_dialog(),",
        ["a_magnet_link_is_added_from_the_dialog"],
    ),
    (
        "a press behind the magnet dialog reaches the row",
        "        f.hit(Target::DialogBackdrop, Rect::new(0.0, 0.0, width, height));",
        "",
        ["a_press_behind_the_magnet_dialog_reaches_nothing"],
    ),
    (
        "a press goes through the list of keys",
        "            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));",
        "",
        ["the_list_of_keys_is_modal_to_the_pointer"],
    ),
    (
        "a press elsewhere leaves the keys in the search box",
        "        if target != Target::Search {\n"
        "            self.search_active = false;",
        "        if false {\n"
        "            self.search_active = false;",
        ["the_search_box_filters_and_escape_clears_it"],
    ),
    # -- scrolling ------------------------------------------------------------------------------------------------
    (
        "the list does not scroll",
        "        self.transfer_scroll = next;\n"
        "        EventResult::Consumed",
        "        let _ = next;\n"
        "        EventResult::Consumed",
        ["the_transfer_list_scrolls_and_follows_the_selection"],
    ),
    (
        "the list scrolls past its end",
        "            now.saturating_add(rows.unsigned_abs())\n"
        "        }\n"
        "        .min(last);",
        "            now.saturating_add(rows.unsigned_abs())\n"
        "        };\n"
        "        let _ = last;",
        ["the_transfer_list_scrolls_and_follows_the_selection"],
    ),
    (
        "the chosen transfer scrolls off the bottom",
        "            } else if at >= self.transfer_scroll.saturating_add(visible) {",
        "            } else if false {",
        ["the_transfer_list_scrolls_and_follows_the_selection"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "torrent", timeout=900, only=only))

"""Mutation test for the freecell suite.

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
    # -- Layout: the bands ---------------------------------------------
    (
        "the padding is allowed to be wider than the window it pads",
        "        let pad = (w.min(h) * 0.014).clamp(2.0, 14.0).min(w.min(h) / 2.0);",
        "        let pad = (w.min(h) * 0.014).clamp(2.0, 14.0);",
        ["the_bands_stay_inside_the_window_at_every_size"],
    ),
    (
        "the header is given a share of the width instead of the height",
        "        let header_h = (h * 0.06).clamp(20.0, 54.0).min(h);",
        "        let header_h = (w * 0.06).clamp(20.0, 54.0).min(h);",
        ["the_bands_are_shares_of_the_height_not_the_width"],
    ),
    (
        "the footer is given a share of the width instead of the height",
        "        let footer_h = (h * 0.045).clamp(14.0, 36.0).min(h);",
        "        let footer_h = (w * 0.045).clamp(14.0, 36.0).min(h);",
        ["the_bands_are_shares_of_the_height_not_the_width"],
    ),
    (
        "the header is allowed to be taller than the window",
        "        let header_h = (h * 0.06).clamp(20.0, 54.0).min(h);",
        "        let header_h = (h * 0.06).clamp(20.0, 54.0);",
        ["the_bands_stay_inside_the_window_at_every_size"],
    ),
    (
        "the body starts at the top of the window instead of under the header",
        "        let body_y = (header.bottom() + pad).min(h);",
        "        let body_y = pad.min(h);",
        ["the_bands_run_down_the_window_in_order_and_do_not_overlap"],
    ),
    (
        "the footer is allowed to start above the body",
        "        let footer_y = (h - footer_h).max(body_y);",
        "        let footer_y = h - footer_h;",
        # A window short enough for the footer to be pushed above the body is
        # also short enough that every band is empty, so the "inside the
        # window" test cannot see it -- the ordering test can.
        ["the_bands_run_down_the_window_in_order_and_do_not_overlap"],
    ),
    (
        "the body is allowed a negative height",
        "                (footer_y - body_y - pad).max(0.0),",
        "                footer_y - body_y - pad,",
        ["the_bands_stay_inside_the_window_at_every_size"],
    ),
    (
        "the title type is a fixed size instead of a share of the window",
        "        let title = (h / 22.0).clamp(14.0, 40.0);",
        "        let title = 14.0;",
        ["the_type_sizes_come_from_the_window_and_stay_legible"],
    ),
    (
        "the small type is allowed to be the largest reading",
        "        let small = (h / 64.0).clamp(7.0, 13.0);",
        "        let small = (h / 4.0).clamp(7.0, 130.0);",
        ["the_type_sizes_come_from_the_window_and_stay_legible"],
    ),
    # -- Table: fitting the board into the body ------------------------
    (
        "the table is fitted to width alone, ignoring the height it has",
        "        let card_w = by_width.min(by_height).max(0.0);",
        "        let card_w = by_width.max(0.0);",
        ["the_whole_table_fits_the_space_it_was_given"],
    ),
    (
        "the table is fitted across eight slots instead of nine",
        "        let by_width = aw / 9.0;",
        "        let by_width = aw / 8.0;",
        ["the_whole_table_fits_the_space_it_was_given"],
    ),
    (
        "the fan is not tightened when a column runs deep",
        "        let cascade = if steps > 0.0 {\n            (spare / steps).min(natural)\n        } else {\n            natural\n        };",
        "        let cascade = natural;",
        ["the_whole_table_fits_the_space_it_was_given"],
    ),
    (
        "the fan is allowed to open wider than its natural step",
        "            (spare / steps).min(natural)",
        "            spare / steps",
        ["a_deeper_column_is_never_drawn_looser_than_a_shallow_one"],
    ),
    (
        "the deepest column is measured as if it held one card more",
        "        let steps = f32_from_usize(deepest.saturating_sub(1));",
        "        let steps = f32_from_usize(deepest);",
        ["a_column_of_one_card_is_fitted_as_if_nothing_hung_below_it"],
    ),
    (
        "the table is pinned to the left rather than centred in its area",
        "        let left = area.x + (aw - card_w * 9.0).max(0.0) / 2.0;",
        "        let left = area.x;",
        ["the_table_is_centred_in_whatever_it_was_given"],
    ),
    (
        "the columns are laid out with no gap between them",
        "        let step = card_w + gap;",
        "        let step = card_w;",
        ["the_columns_are_evenly_spaced_at_every_size"],
    ),
    (
        "the tableau is drawn over the top row instead of below it",
        "        let tableau_y = top_row_y + card_h * (1.0 + PILE_COUNT_SHARE + ROW_GAP_SHARE);",
        "        let tableau_y = top_row_y;",
        ["the_top_row_and_the_tableau_stand_on_one_grid"],
    ),
    (
        "a column's reachable box stops at its cards instead of the table floor",
        "            (self.area.bottom() - self.tableau_y).max(self.card_h),",
        "            self.card_h,",
        ["a_column_can_be_clicked_below_its_last_card"],
    ),
    # -- The cursor ring -----------------------------------------------
    (
        "stepping back from the start of a ring stays where it is",
        "    i.checked_sub(1).unwrap_or_else(|| len.saturating_sub(1))",
        "    i.saturating_sub(1)",
        [
            "stepping_back_from_the_start_of_a_ring_lands_on_its_end",
            "the_cursor_walks_all_the_way_round_every_zone_and_back",
        ],
    ),
    (
        "stepping forward off the end of a ring runs past it",
        "    if next >= len { 0 } else { next }",
        "    next",
        [
            "stepping_forward_off_the_end_of_a_ring_lands_on_its_start",
            "the_cursor_walks_all_the_way_round_every_zone_and_back",
        ],
    ),
    (
        "the top row is split at a literal four rather than where the cells end",
        "                if i < FREE_CELL_COUNT {\n                    FocusArea::FreeCell(i)\n                } else {\n                    FocusArea::Foundation(i.saturating_sub(FREE_CELL_COUNT))\n                }",
        "                if i < 3 {\n                    FocusArea::FreeCell(i)\n                } else {\n                    FocusArea::Foundation(i.saturating_sub(3))\n                }",
        ["the_top_row_the_cursor_rises_into_is_split_where_the_cells_end"],
    ),
    (
        "a foundation drops to the column under the free cell of the same index",
        "                FocusArea::Tableau(i.saturating_add(FREE_CELL_COUNT).min(LAST_TABLEAU_COL))",
        "                FocusArea::Tableau(i.min(LAST_TABLEAU_COL))",
        ["a_foundation_the_cursor_drops_from_lands_on_the_column_under_it"],
    ),
    # -- The rules of the game -----------------------------------------
    (
        # Not the `wrapping_add` form, which is an equivalent mutant: a `u8` top
        # of 255 wraps to 0, and no rank has value 0, so nothing on any board
        # can tell the two apart. What can be told apart is a comparison that
        # accepts any card at or above the top rather than the one card above.
        "a card any distance above the foundation's top is sent home",
        "        foundation_top_value.checked_add(1) == Some(self.rank.value())",
        "        foundation_top_value < self.rank.value()",
        ["test_card_cannot_place_on_foundation_wrong"],
    ),
    (
        "a card may stack on one of its own colour",
        "        self.suit.is_red() != below.suit.is_red()\n            && self.rank.value().checked_add(1) == Some(below.rank.value())",
        "        self.rank.value().checked_add(1) == Some(below.rank.value())",
        ["test_card_cannot_stack_same_color"],
    ),
    (
        "a card may stack on one of any rank",
        "        self.suit.is_red() != below.suit.is_red()\n            && self.rank.value().checked_add(1) == Some(below.rank.value())",
        "        self.suit.is_red() != below.suit.is_red()",
        ["test_card_cannot_stack_wrong_rank"],
    ),
    # -- Sending a card home -------------------------------------------
    (
        "a card leaves its pile before the foundation is known to exist",
        "        if !card.can_place_on_foundation(self.foundation_top_value(fidx))\n            || !self.is_safe_to_auto_move(card)\n            || self.foundations.get(fidx).is_none()\n            || self.take_card_from(from).is_none()\n        {\n            return false;\n        }",
        "        if self.take_card_from(from).is_none()\n            || !card.can_place_on_foundation(self.foundation_top_value(fidx))\n            || !self.is_safe_to_auto_move(card)\n            || self.foundations.get(fidx).is_none()\n        {\n            return false;\n        }",
        ["a_card_that_cannot_go_home_stays_where_it_is"],
    ),
    (
        "a card is pushed home without being taken off the board",
        "            || self.take_card_from(from).is_none()\n        {",
        "            || false\n        {",
        ["a_card_sent_home_is_in_exactly_one_place_afterwards"],
    ),
    (
        "sending a card home records no step to undo",
        "        self.undo_stack.push(UndoStep { from, to, player });\n        true",
        "        true",
        ["a_won_game_that_is_undone_is_no_longer_won"],
    ),
    # -- Counting a move -----------------------------------------------
    (
        "a move is not counted",
        "        self.move_count = self.move_count.saturating_add(1);",
        "        self.move_count = self.move_count;",
        ["f_parks_the_focused_column_in_a_free_cell"],
    ),
    (
        "asking for the auto-move is not itself a move",
        "            if run == AutoRun::Asked {\n                self.count_move();\n            }",
        "            if false {\n                self.count_move();\n            }",
        ["asking_for_the_auto_move_is_itself_one_move"],
    ),
    (
        "a run that merely followed a placement is counted as a move of its own",
        "            if run == AutoRun::Asked {\n                self.count_move();\n            }",
        "            {\n                self.count_move();\n            }",
        ["the_move_count_returns_to_where_it_started"],
    ),
    # -- Undo ----------------------------------------------------------
    (
        "every card of an auto-run is treated as a press of its own",
        "                if self.send_home(\n                    MoveLocation::Tableau(col),\n                    card,\n                    run == AutoRun::Asked && total == 0,\n                ) {",
        "                if self.send_home(MoveLocation::Tableau(col), card, true) {",
        ["one_press_of_undo_takes_back_one_press_of_play"],
    ),
    (
        "no step of an auto-run is ever the player's",
        "                if self.send_home(\n                    MoveLocation::FreeCell(fc_idx),\n                    card,\n                    run == AutoRun::Asked && total == 0,\n                ) {",
        "                if self.send_home(MoveLocation::FreeCell(fc_idx), card, false) {",
        ["a_run_that_empties_a_free_cell_is_the_player_s_move_too"],
    ),
    # -- The Free move -------------------------------------------------
    (
        "F parks a card from whatever the cursor is on",
        "        if let FocusArea::Tableau(col) = self.focus {",
        "        let col = match self.focus {\n            FocusArea::Tableau(c) | FocusArea::FreeCell(c) | FocusArea::Foundation(c) => c,\n        };\n        {",
        ["f_does_nothing_when_the_cursor_is_not_on_a_column"],
    ),
    (
        "F does not run the follow-on auto-move every other placement runs",
        "            if self.try_tableau_to_freecell(col) {\n                self.selection = None;\n                self.auto_move_to_foundations(AutoRun::Followed);\n            }",
        "            if self.try_tableau_to_freecell(col) {\n                self.selection = None;\n            }",
        ["parking_an_ace_sends_it_straight_home"],
    ),
    (
        "F is not wired to the game at all",
        "            Key::F => self.park_focused_column(),",
        "            Key::F => {}",
        [
            "f_parks_the_focused_column_in_a_free_cell",
            "the_free_button_parks_a_card_just_as_the_key_does",
        ],
    ),
    (
        "the Free button is captioned for a key it does not run",
        "            Self::Free => Key::F,",
        "            Self::Free => Key::N,",
        ["the_free_button_parks_a_card_just_as_the_key_does"],
    ),
    (
        "a card is parked in a cell that already holds one",
        "        let Some(fc_idx) = self.first_empty_free_cell() else {\n            return false;\n        };",
        "        let fc_idx = self.first_empty_free_cell().unwrap_or(0);",
        ["f_does_nothing_when_every_cell_is_full"],
    ),
    # -- The board is only reachable through the sheet -----------------
    (
        "a won board still takes every key",
        "        if self.won {\n            if key == Key::N {\n                self.new_game();\n            }\n            return EventResult::Consumed;\n        }",
        "        if self.won && key == Key::N {\n            self.new_game();\n            return EventResult::Consumed;\n        }",
        ["f_is_refused_on_a_won_board"],
    ),
    # The sheet is guarded twice over -- a hit box across the whole window, and
    # the `won` branch in `click` -- so neither break alone changes what a click
    # on the board *does*. Each is therefore named and owned for what it alone
    # is responsible for: the covering box for what a click FINDS, the branch
    # for the one target it routes differently.
    (
        "the win sheet leaves the board underneath it reachable",
        "        f.hit(Target::Overlay, l.window);",
        "        let _ = &l.window;",
        ["a_click_on_the_win_sheet_does_not_reach_the_board_behind_it"],
    ),
    (
        "the new-game line on the win sheet is answered as inert chrome",
        "        if self.state.won {\n            return match target {",
        "        if false {\n            return match target {",
        ["a_click_on_the_new_game_line_deals_a_new_board"],
    ),
    # -- The footer ----------------------------------------------------
    (
        "the room reading counts cells that are full",
        "            self.empty_free_cell_count(),",
        "            FREE_CELL_COUNT - self.empty_free_cell_count(),",
        [
            "the_footer_reports_how_much_room_is_left",
            "the_room_reading_follows_the_board",
        ],
    ),
    (
        "the room reading counts columns that hold cards",
        "            self.empty_tableau_count()",
        "            TABLEAU_COLS - self.empty_tableau_count()",
        ["the_footer_reports_how_much_room_is_left"],
    ),
    (
        "the room reading is squeezed in whether or not there is width for it",
        "        if room_x < x {\n            f.unclip();\n            return;\n        }",
        "        if false {\n            f.unclip();\n            return;\n        }",
        ["a_footer_with_no_room_for_either_caption_still_draws_its_buttons"],
    ),
    (
        "the keyboard reminder is drawn over the room reading",
        "        if hint_x >= x {\n            label(f, hint_x, text_y, hint, OVERLAY0, l.font, regular);\n        }",
        "        label(f, hint_x, text_y, hint, OVERLAY0, l.font, regular);",
        ["the_keyboard_hint_goes_before_the_room_reading_does"],
    ),
    (
        "a control's box is recorded before it is drawn wide enough to hit",
        "            let w = text::measure(control.label(), l.font, regular) + inner * 2.0;",
        "            let w = 0.0;",
        ["every_control_on_the_strip_does_what_its_caption_says"],
    ),
    # -- The window ----------------------------------------------------
    (
        "the frame is drawn at the size it was launched with, not the live one",
        "        let (w, h) = self.size();",
        "        let (w, h) = (WINDOW_WIDTH, WINDOW_HEIGHT);",
        ["a_resize_is_what_the_next_click_is_read_against"],
    ),
    (
        "a resize is remembered with the window's width in both places",
        "        self.size = (width.max(0.0), height.max(0.0));",
        "        self.size = (width.max(0.0), width.max(0.0));",
        ["a_resize_is_what_the_next_click_is_read_against"],
    ),
    (
        "a key release is played as if it were a press",
        "            pressed: true,\n            ..\n        }) => app.state.handle_key(*key, *modifiers),",
        "            ..\n        }) => app.state.handle_key(*key, *modifiers),",
        ["a_key_press_reaches_the_game_and_a_release_does_not"],
    ),
    (
        "a right-click is played as if it were a left one",
        "        if button != MouseButton::Left {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_click_on_a_button_that_is_not_the_left_one_does_nothing"],
    ),
    (
        "the launch deals the one board that used to be hardcoded",
        "        Self::with_seed(seed_from_system(0x4652_4545_4345_4C4C))",
        "        Self::with_seed(42)",
        ["a_launch_does_not_deal_the_one_board_that_was_hardcoded"],
    ),
    (
        "the free cells are drawn a slot to the right of where they are laid out",
        "        for idx in 0..FREE_CELL_COUNT {\n            let rect = t.top_slot(idx);",
        "        for idx in 0..FREE_CELL_COUNT {\n            let rect = t.top_slot(idx.saturating_add(1));",
        ["at_a_size_that_fits_the_clip_crops_nothing"],
    ),
    (
        "the whole window is left unclipped",
        "        f.clip(l.window);",
        "        f.clip(Rect::new(0.0, 0.0, f32::MAX, f32::MAX));",
        ["the_whole_frame_is_clipped_to_the_window"],
    ),
    (
        "the win sheet is drawn over every board, won or not",
        "        if self.won {\n            self.draw_win_sheet(&mut f, &l);\n        }",
        "        {\n            self.draw_win_sheet(&mut f, &l);\n        }",
        ["the_win_sheet_is_only_there_once_the_game_is_won"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "freecell", timeout=300, only=sys.argv[1:] or None))

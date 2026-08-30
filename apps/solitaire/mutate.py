"""Mutation test for the solitaire suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The old suite would have survived nearly all of these.  It tested
`top_row_x(1) - top_row_x(0) == CARD_WIDTH + CARD_GAP_X` -- the definition of
`top_row_x` restated -- and it agreed with any layout at all, including one
that painted the whole deal off the bottom of the window.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- The card size -------------------------------------------------
    #
    # One card size serves both bands, and it is the largest that fits all
    # three constraints.  Dropping any one of them is what the old fifteen
    # fixed constants amounted to: a size that was right at 900x720 and wrong
    # everywhere else.
    (
        "the card is fitted across alone, so it runs off a short window",
        "        let card_w = by_width.min(by_top).min(by_tableau);",
        "        let card_w = by_width;",
        [
            "the_deepest_fan_a_deal_can_reach_still_fits_the_window",
            "the_top_row_sits_above_the_columns_and_never_overlaps_them",
        ],
    ),
    (
        "the card is fitted to the tableau alone, so it runs off a narrow window",
        "        let card_w = by_width.min(by_top).min(by_tableau);",
        "        let card_w = by_tableau;",
        ["no_pile_is_cut_off_by_the_edge_of_the_window"],
    ),
    (
        "the card is fitted to the top row alone, ignoring the fan below it",
        "        let card_w = by_width.min(by_top).min(by_tableau);",
        "        let card_w = by_width.min(by_top);",
        ["the_deepest_fan_a_deal_can_reach_still_fits_the_window"],
    ),
    (
        "the deal is pinned to the left rather than centred",
        "        let left = (l.window.w - used) / 2.0;",
        "        let left = l.pad;",
        ["widening_the_window_moves_the_deal_rather_than_the_gap_beside_it"],
    ),
    # -- The two rows --------------------------------------------------
    #
    # The top row and the tableau are laid out by two separate functions, and
    # nothing but this arithmetic makes column 3 sit under foundation 0.
    (
        "the columns are spaced by the card width alone, so they overlap",
        "    fn col_x(self, col: usize) -> f32 {\n"
        "        self.left + (self.card_w + self.gap_x) * col as f32\n"
        "    }",
        "    fn col_x(self, col: usize) -> f32 {\n"
        "        self.left + self.card_w * col as f32\n"
        "    }",
        [
            "the_columns_line_up_under_the_top_row",
            "the_seven_columns_are_evenly_spaced_the_same_width_and_never_overlap",
        ],
    ),
    (
        "the top row is spaced by the card width alone, so it drifts off the columns",
        "            self.left + (self.card_w + self.gap_x) * index as f32,",
        "            self.left + self.card_w * index as f32,",
        ["the_columns_line_up_under_the_top_row"],
    ),
    (
        "the tableau starts at the top of the window, under the header",
        "            tableau_y: l.tableau.y + l.pad,",
        "            tableau_y: l.window.y + l.pad,",
        ["the_top_row_sits_above_the_columns_and_never_overlaps_them"],
    ),
    (
        "the stock is drawn a size of its own rather than the deck's",
        "    fn slot(self, index: usize) -> Rect {\n"
        "        Rect::new(\n"
        "            self.left + (self.card_w + self.gap_x) * index as f32,\n"
        "            self.top_y,\n"
        "            self.card_w,\n"
        "            self.card_h,\n"
        "        )\n"
        "    }",
        "    fn slot(self, index: usize) -> Rect {\n"
        "        Rect::new(\n"
        "            self.left + (self.card_w + self.gap_x) * index as f32,\n"
        "            self.top_y,\n"
        "            self.card_w * if index == 0 { 0.7 } else { 1.0 },\n"
        "            self.card_h,\n"
        "        )\n"
        "    }",
        ["the_stock_the_waste_and_the_foundations_are_all_one_card_size"],
    ),
    # -- The fan -------------------------------------------------------
    (
        "the face-up cards are fanned as tightly as the backs, hiding their ranks",
        "            face_step: card_h * 0.22,",
        "            face_step: card_h * 0.08,",
        ["a_covered_face_up_card_shows_more_of_itself_than_a_covered_face_down_one"],
    ),
    (
        "the face fan ignores the face-down cards under it, so the run overlaps them",
        "            self.tableau_y + self.back_step * backs as f32 + self.face_step * nth as f32,",
        "            self.tableau_y + self.face_step * nth as f32,",
        ["a_covered_card_is_clickable_only_where_it_can_be_seen"],
    ),
    (
        "every face-down card is drawn on top of the first, so a column shows one back",
        "            self.tableau_y + self.back_step * nth as f32,",
        "            self.tableau_y,",
        ["a_covered_face_up_card_shows_more_of_itself_than_a_covered_face_down_one"],
    ),
    # -- The header ----------------------------------------------------
    #
    # The move counter used to be drawn at x = 400 and the help at x = 500,
    # which put them on top of the title in any window narrow enough and
    # adrift from it in any window wide enough.
    (
        "the move counter goes back to a fixed x",
        "        let x = title.right() + gap;",
        "        let x = 400.0;",
        ["the_header_follows_the_title_rather_than_a_fixed_offset"],
    ),
    (
        "the stock's count is the waste's",
        '            self.draw_pile_count(f, l, stock, &format!("{}", self.stock.len()));',
        '            self.draw_pile_count(f, l, stock, &format!("{}", self.waste.len()));',
        ["the_stock_says_how_many_cards_are_left_and_the_count_follows_it"],
    ),
    # -- The hit boxes -------------------------------------------------
    #
    # A hit test reads the boxes in reverse paint order, so the column's whole
    # strip has to go down before the cards that sit on it.  Recorded after
    # them, it swallows every click on a card.
    (
        "the column strip is recorded over the cards, so it swallows their clicks",
        "            f.hit(Target::TableauColumn(col), self.column_reach(t, col, l));\n"
        "\n"
        "            if pile.is_empty() {",
        "            if pile.is_empty() {",
        ["a_click_below_the_last_card_of_a_column_still_reaches_the_column"],
    ),
    (
        "the column strip is recorded after the cards, so it swallows their clicks",
        "                    f.hit(Target::TableauBack(col, i), rect);\n"
        "                }\n"
        "            }\n"
        "        }\n"
        "    }",
        "                    f.hit(Target::TableauBack(col, i), rect);\n"
        "                }\n"
        "            }\n"
        "            f.hit(Target::TableauColumn(col), self.column_reach(t, col, l));\n"
        "        }\n"
        "    }",
        [
            "clicking_a_face_up_card_picks_up_from_that_card",
            "a_covered_card_is_clickable_only_where_it_can_be_seen",
        ],
    ),
    (
        "the column's strip stops at its cards, so the bare table below is dead",
        "        let bottom = l.tableau.bottom().max(top.bottom());",
        "        let bottom = top.bottom();",
        ["a_click_below_the_last_card_of_a_column_still_reaches_the_column"],
    ),
    (
        "a covered card's box is the whole card, so it steals the clicks above it",
        "                    let rect = t.back_rect(col, i);\n"
        "                    self.draw_card_back(f, rect, t, false);\n"
        "                    f.hit(Target::TableauBack(col, i), rect);",
        "                    let rect = t.back_rect(col, i);\n"
        "                    self.draw_card_back(f, rect, t, false);\n"
        "                    f.hit(\n"
        "                        Target::TableauBack(col, i),\n"
        "                        Rect::new(rect.x, rect.y, rect.w, rect.h * 4.0),\n"
        "                    );",
        ["a_covered_card_is_clickable_only_where_it_can_be_seen"],
    ),
    # -- The mouse -----------------------------------------------------
    #
    # The program had none of this: `handle_event` matched `Event::Key` and
    # nothing else, so every one of these is a behaviour that did not exist.
    (
        "every mouse button plays the game",
        "        if button != MouseButton::Left {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["only_the_left_button_plays_a_card"],
    ),
    (
        "a click on the stock moves the cursor but does not deal",
        "            Target::Stock => {\n"
        "                self.state.focus = FocusArea::Stock;\n"
        "                self.state.activate();",
        "            Target::Stock => {\n                self.state.focus = FocusArea::Stock;",
        ["clicking_the_stock_turns_a_card_over"],
    ),
    (
        "a click on a face-up card always picks up the deepest of the run",
        "            Target::TableauCard(col, nth) => {\n"
        "                self.state.focus = FocusArea::Tableau(col, nth);",
        "            Target::TableauCard(col, _nth) => {\n"
        "                self.state.focus = FocusArea::Tableau(col, 0);",
        ["clicking_a_face_up_card_picks_up_from_that_card"],
    ),
    (
        "a click on a covered card picks it up anyway",
        "            Target::TableauBack(col, _) => {\n"
        "                // A covered card cannot be picked up. Moving the cursor there\n"
        "                // is still worth doing -- it is how the keyboard would reach\n"
        "                // the column -- but nothing is activated.\n"
        "                let nth = self.state.tableau_face_up_count(col).saturating_sub(1);\n"
        "                self.state.focus = FocusArea::Tableau(col, nth);",
        "            Target::TableauBack(col, _) => {\n"
        "                let nth = self.state.tableau_face_up_count(col).saturating_sub(1);\n"
        "                self.state.focus = FocusArea::Tableau(col, nth);\n"
        "                self.state.activate();",
        ["a_covered_card_is_clickable_only_where_it_can_be_seen"],
    ),
    (
        "a click on the title is taken for a move",
        "            Target::Title | Target::Moves | Target::Help | Target::WinBanner => {\n"
        "                EventResult::Ignored\n"
        "            }",
        "            Target::Title | Target::Moves | Target::Help | Target::WinBanner => {\n"
        "                self.state.activate();\n"
        "                EventResult::Consumed\n"
        "            }",
        ["a_click_on_the_header_is_not_a_move"],
    ),
    (
        "a click anywhere at all is a move, even outside the window",
        "        let Some(target) = self.frame(w, h).hit_test(x, y) else {\n"
        "            return EventResult::Ignored;\n"
        "        };",
        "        let target = self\n"
        "            .frame(w, h)\n"
        "            .hit_test(x, y)\n"
        "            .unwrap_or(Target::Stock);",
        ["a_click_on_no_pile_at_all_changes_nothing"],
    ),
    # -- The window ----------------------------------------------------
    (
        "the render pass ignores the size the window hands it",
        "    fn render(&mut self, width: f32, height: f32) -> RenderTree {\n"
        "        self.resize(width, height);\n"
        "        self.frame(width, height).into_tree()\n"
        "    }",
        "    fn render(&mut self, width: f32, height: f32) -> RenderTree {\n"
        "        self.resize(width, height);\n"
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()\n"
        "    }",
        ["the_render_pass_draws_at_the_size_the_window_hands_it"],
    ),
    (
        "an event that changed nothing still asks for a redraw",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["the_window_is_asked_to_redraw_only_when_something_changed"],
    ),
    (
        "the window opens at a size the layout was not written against",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (640, 480)",
        ["the_window_opens_at_the_size_the_tests_draw_at"],
    ),
    (
        "the close button does not close the window",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Idle;\n        }",
        ["the_window_is_asked_to_redraw_only_when_something_changed"],
    ),
    # -- The deal ------------------------------------------------------
    (
        "the deal turns every card face up",
        "                pile.push(PileCard::new(card, row == col));",
        "                pile.push(PileCard::new(card, true));",
        [
            "test_initial_deal_tableau_face_up",
            "test_tableau_face_down_count",
            "test_tableau_face_up_count",
        ],
    ),
    (
        "every column is dealt one card",
        "            for row in 0..=col {",
        "            for row in 0..1 {",
        ["test_initial_deal_tableau_sizes", "test_initial_deal_stock_size"],
    ),
    (
        "the cards left after the deal are dropped instead of becoming the stock",
        "        self.stock.extend(cards);",
        "        drop(cards);",
        ["test_initial_deal_stock_size", "test_initial_deal_all_cards_present"],
    ),
    (
        "the deck is dealt in order, so the hand is the same every game",
        "        self.rng.shuffle(&mut deck);",
        "        let _ = &mut deck;",
        ["test_new_game_different_layout", "the_opening_tableau_shows_a_fair_sample_of_the_deck"],
    ),
    # -- The rules -----------------------------------------------------
    (
        "cards stack on their own colour",
        "        self.suit.is_red() != below.suit.is_red()",
        "        self.suit.is_red() == below.suit.is_red()",
        ["test_card_can_stack_on_tableau"],
    ),
    (
        "cards stack in ascending rank",
        "            && self.rank.value().saturating_add(1) == below.rank.value()",
        "            && self.rank.value() == below.rank.value().saturating_add(1)",
        ["test_card_can_stack_on_tableau"],
    ),
    (
        "a foundation accepts any rank",
        "        self.rank.value() == foundation_top_value.saturating_add(1)",
        "        true",
        ["test_card_can_place_on_foundation", "test_foundation_rejects_wrong_order"],
    ),
    (
        "any card can start an empty column, not only a king",
        "            None => card.rank == Rank::King,",
        "            None => true,",
        ["test_cannot_place_non_king_on_empty_tableau"],
    ),
    (
        "the game is won a card short of a full suit",
        "        self.won = self.foundations.iter().all(|f| f.len() == 13);",
        "        self.won = self.foundations.iter().all(|f| f.len() == 12);",
        ["test_win_detection"],
    ),
    (
        "the newly exposed card is not turned over",
        "        if let Some(pile) = self.col_mut(col)\n"
        "            && let Some(top) = pile.last_mut()\n"
        "            && !top.face_up\n"
        "        {\n"
        "            top.face_up = true;\n"
        "            return true;\n"
        "        }\n"
        "        false",
        "        false",
        ["test_tableau_to_foundation_flips", "test_undo_tableau_to_tableau_with_flip"],
    ),
    (
        "a move is not counted",
        "    fn bump_moves(&mut self) {\n"
        "        self.move_count = self.move_count.saturating_add(1);\n"
        "    }",
        "    fn bump_moves(&mut self) {}",
        ["test_draw_increments_move_count"],
    ),
    (
        "the empty stock is not recycled from the waste",
        "        } else if !self.waste.is_empty() {",
        "        } else if false {",
        ["test_recycle_when_stock_empty"],
    ),
    # -- The cursor ----------------------------------------------------
    (
        "tab runs off the last foundation instead of entering the tableau",
        "                let next = i.saturating_add(1);\n"
        "                if next < FOUNDATION_COUNT {",
        "                let next = i.saturating_add(1);\n                if true {",
        ["test_tab_forward_cycle"],
    ),
    (
        "tab runs off the last column instead of wrapping to the stock",
        "                let next = col.saturating_add(1);\n"
        "                if next < TABLEAU_COLS {",
        "                let next = col.saturating_add(1);\n                if true {",
        ["test_tab_forward_cycle"],
    ),
    (
        "the cursor walks off the right of the foundations",
        "                Some(new_i) if new_i < FOUNDATION_COUNT => {",
        "                Some(new_i) if new_i < usize::MAX => {",
        ["test_horizontal_clamp_right"],
    ),
    (
        "the cursor walks off the right of the tableau",
        "                if let Some(new_c) = step_index(col, delta)\n"
        "                    && new_c < TABLEAU_COLS\n"
        "                {",
        "                if let Some(new_c) = step_index(col, delta)\n"
        "                    && new_c < usize::MAX\n"
        "                {",
        ["test_horizontal_clamp_right"],
    ),
    (
        "the cursor walks past the top card of a column",
        "                offset.saturating_add(step).min(max_idx)",
        "                offset.saturating_add(step)",
        ["test_move_within_tableau_up_down"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "solitaire", timeout=420, only=sys.argv[1:] or None))

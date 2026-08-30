"""Mutation test for the yahtzee suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The old suite had 168 tests and would have survived every one of the mutations
below except the handful in "The rules".  It tested the scoring functions --
which were already right -- and nothing at all about the picture, because
before this rewrite there was no picture: `main` built a `Yahtzee` and dropped
it.  The three tests it had for `render` asserted that it returned a non-empty
`Vec`, which is true of a renderer that paints one rectangle in the wrong
place at the wrong size.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- The bands -----------------------------------------------------
    #
    # `Layout::solve` is the whole of the geometry.  Sixteen constants used to
    # stand where each of these expressions does, so every one of these
    # mutations is a faithful restoration of what the file said before.
    (
        "the scorecard is a fixed width again, so it swallows a narrow window",
        "        let card_w = (w * 0.44).clamp(needed, needed * 1.6).min(w / 2.0);",
        "        let card_w = 320.0;",
        ["the_scorecard_never_takes_more_than_half_the_window"],
    ),
    (
        "the scorecard is never capped, so it crowds the dice out",
        "        let card_w = (w * 0.44).clamp(needed, needed * 1.6).min(w / 2.0);",
        "        let card_w = (w * 0.44).clamp(needed, needed * 1.6);",
        ["the_scorecard_never_takes_more_than_half_the_window"],
    ),
    (
        "the card is never floored, so a narrow window gets a sliver of a card",
        "        let card_w = (w * 0.44).clamp(needed, needed * 1.6).min(w / 2.0);",
        "        let card_w = (w * 0.44).min(needed * 1.6).min(w / 2.0);",
        ["no_category_name_is_painted_into_a_box_too_narrow_to_show_it"],
    ),
    (
        "the card's floor is the old magic constant rather than a measurement",
        "        let card_w = (w * 0.44).clamp(needed, needed * 1.6).min(w / 2.0);",
        "        let card_w = (w * 0.44).clamp(170.0, 380.0).min(w / 2.0);",
        ["no_category_name_is_painted_into_a_box_too_narrow_to_show_it"],
    ),
    (
        "the floor forgets the row's own inset, so the longest name is clipped",
        "        let needed = (longest + pad * 1.2) / NAME_SHARE + pad * 2.0;",
        "        let needed = longest / NAME_SHARE + pad * 2.0;",
        ["no_category_name_is_painted_into_a_box_too_narrow_to_show_it"],
    ),
    (
        "the floor forgets the card's padding, so the longest name is clipped",
        "        let needed = (longest + pad * 1.2) / NAME_SHARE + pad * 2.0;",
        "        let needed = (longest + pad * 1.2) / NAME_SHARE;",
        ["no_category_name_is_painted_into_a_box_too_narrow_to_show_it"],
    ),
    (
        "the floor is measured from the shortest name rather than the longest",
        "            .fold(0.0_f32, f32::max);",
        "            .fold(f32::INFINITY, f32::min);",
        ["no_category_name_is_painted_into_a_box_too_narrow_to_show_it"],
    ),
    (
        "the two columns are laid out independently, so they overlap",
        "        let card = Rect::new(left.right(), body.y, card_w.min(body.w), body.h);",
        "        let card = Rect::new(body.x + body.w * 0.5, body.y, card_w.min(body.w), body.h);",
        ["the_two_columns_share_the_width_without_a_gap_or_an_overlap"],
    ),
    (
        "the body starts at the top of the window, under the header",
        "        let body = Rect::new(0.0, header.bottom(), w, (h - header_h).max(0.0));",
        "        let body = Rect::new(0.0, 0.0, w, h);",
        ["the_header_sits_above_both_columns_and_never_overlaps_them"],
    ),
    (
        "the padding is a share of the width alone, so a short window loses its header to it",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 16.0).min(w.min(h) / 2.0);",
        "        let pad = w * 0.02;",
        # Named for the header, not the left column, because that is what the
        # break actually costs.  At 1200x260 the width-only padding is 24 and
        # the header is 23.4 tall, so insetting it leaves nothing and the
        # title, the turn and the high score are not painted at all.  The left
        # column survives the same padding: its bands are stacked through
        # `.max()` guards that collapse to zero height rather than overlapping,
        # which is the behaviour that band-stacking test asserts and it keeps
        # on asserting it.
        ["the_header_boxes_stay_within_the_header_band"],
    ),
    # -- The left column -----------------------------------------------
    #
    # The dice, the button and the help used to be stacked downwards from a
    # fixed top with no regard for the bottom of the window.
    (
        "the three bands are stacked from the top, so the help slides off the floor",
        "        let hints = Rect::new(\n"
        "            inner.x,\n"
        "            (inner.bottom() - hints_h).max(inner.y),\n"
        "            inner.w,\n"
        "            hints_h,\n"
        "        );",
        "        let hints = Rect::new(inner.x, inner.y + 220.0, inner.w, hints_h);",
        ["the_help_is_pinned_to_the_floor_of_the_column"],
    ),
    (
        "the button is placed without regard for the help below it",
        "        let button = Rect::new(\n"
        "            inner.x,\n"
        "            (hints.y - self.pad - button_h).max(inner.y),\n"
        "            inner.w,\n"
        "            button_h,\n"
        "        );",
        "        let button = Rect::new(inner.x, hints.y, inner.w, button_h);",
        ["the_dice_the_button_and_the_help_stack_without_overlapping"],
    ),
    (
        "the dice take the whole column, so the button lands on top of them",
        "            (button.y - self.pad - inner.y).max(0.0),",
        "            inner.h,",
        ["the_dice_the_button_and_the_help_stack_without_overlapping"],
    ),
    # -- The dice ------------------------------------------------------
    #
    # A die was 64 pixels with a 12-pixel gap and its pips 6 across at a fixed
    # 16-pixel offset, so nothing about a die scaled with anything.
    (
        "a die is a fixed size again",
        "        let side = by_w.min(by_h).max(0.0);",
        "        let side = 64.0;",
        [
            "widening_the_window_grows_the_dice_rather_than_the_gaps_beside_them",
            "the_dice_stay_inside_the_band_the_layout_gave_them",
        ],
    ),
    (
        "the dice are fitted across alone, so a short window runs them off the bottom",
        "        let side = by_w.min(by_h).max(0.0);",
        "        let side = by_w;",
        ["the_dice_stay_inside_the_band_the_layout_gave_them"],
    ),
    (
        "the dice are fitted down alone, so a narrow window runs them off the side",
        "        let side = by_w.min(by_h).max(0.0);",
        "        let side = by_h;",
        ["the_dice_stay_inside_the_band_the_layout_gave_them"],
    ),
    (
        "the row leaves no room for the labels above and below a die",
        "        let by_h = (area.h - label * 3.4).max(0.0);",
        "        let by_h = area.h;",
        ["the_held_label_under_a_die_stays_inside_the_dice_band"],
    ),
    (
        "the gap is a constant, so five dice in a narrow window are swallowed by it",
        "        let gap = side * gap_share;",
        "        let gap = 12.0;",
        ["the_dice_stay_inside_the_band_the_layout_gave_them"],
    ),
    (
        "the dice are spaced by the die alone, so they touch",
        "            self.origin.0 + (self.side + self.gap) * i as f32,",
        "            self.origin.0 + self.side * i as f32,",
        ["the_dice_are_evenly_spaced_and_run_left_to_right"],
    ),
    (
        "the row is pinned to the left of its band rather than centred",
        "                area.x + (area.w - row_w).max(0.0) / 2.0,",
        "                area.x,",
        # Not the button test, though that was the first guess: the button
        # centres itself on the dice *row*, so pinning the row left carries
        # the button along with it and the two stay lined up.  Only a test
        # that measures the row against its band can see this.
        ["the_dice_are_centred_in_their_band"],
    ),
    (
        "a die is drawn as wide as its band, so the five overlap",
        "            self.side,\n            self.side,\n        )\n    }",
        "            self.side * 2.0,\n            self.side,\n        )\n    }",
        ["a_die_is_square", "the_dice_are_evenly_spaced_and_run_left_to_right"],
    ),
    (
        "every die is drawn on top of the first, so the row shows one die",
        "    fn die(self, i: usize) -> Rect {",
        "    fn die(self, _unused: usize) -> Rect {\n        let i = 0usize;",
        ["the_dice_are_evenly_spaced_and_run_left_to_right"],
    ),
    (
        "the die's box is recorded but nothing is painted in it",
        "        let ring = (d.side * 0.05).max(1.0);",
        "        let ring = (d.side * 0.05).max(1.0);\n        if true {\n            f.hit(Target::Die(i), die);\n            return;\n        }",
        ["something_is_painted_inside_every_die_box"],
    ),
    # -- The header ----------------------------------------------------
    #
    # The counter was drawn at `PADDING + 130.0` and the high score at
    # `PADDING + 400.0`.
    (
        "the counter goes back to a fixed x",
        "        let turn_x = title.right() + l.pad;",
        "        let turn_x = band.x + 130.0;",
        ["the_title_the_turn_and_the_high_score_never_overlap"],
    ),
    (
        "the high score goes back to a fixed x",
        "        let high = Rect::new(\n"
        "            (band.right() - high_w).max(title.right() + l.pad),",
        "        let high = Rect::new(\n"
        "            band.x + 400.0,",
        ["the_high_score_is_anchored_to_the_right_edge"],
    ),
    (
        "the header ignores the size of the text it is placing",
        "        let title = Rect::new(band.x, band.y, title_w.min(band.w), band.h);",
        "        let title = Rect::new(band.x, band.y, band.w, band.h);",
        ["the_title_the_turn_and_the_high_score_never_overlap"],
    ),
    (
        "the header boxes are as tall as the window rather than the band",
        "        let band = inset(l.header, l.pad);",
        "        let band = Rect::new(l.header.x, l.header.y, l.header.w, l.window.h);",
        ["the_header_boxes_stay_within_the_header_band"],
    ),
    (
        "the header never says the game is over",
        "        let turn_text = if self.phase() == GamePhase::GameOver {",
        "        let turn_text = if false {",
        ["the_header_reads_game_over_once_every_box_is_spent"],
    ),
    # -- The button ----------------------------------------------------
    (
        "the button is a constant width again, cut for the shortest legend",
        "        let width = (widest + l.pad * 3.0).min(band.w);",
        "        let width = 140.0f32.min(band.w);",
        ["the_button_is_wide_enough_for_its_widest_legend"],
    ),
    (
        "the button is sized to the legend it happens to be showing",
        "        let widest = [\"New Game (N)\", \"No Rolls Left\", \"Roll (R)\"]\n"
        "            .into_iter()\n"
        "            .map(|s| text::measure(s, l.font, FontWeightHint::Bold))\n"
        "            .fold(0.0f32, f32::max);",
        "        let widest = text::measure(label, l.font, FontWeightHint::Bold);",
        ["the_button_is_the_same_box_whatever_it_says"],
    ),
    (
        "the button is pinned to the left of its band rather than under the dice",
        "        let x = (row.centre().0 - width / 2.0).clamp(band.x, (band.right() - width).max(band.x));",
        "        let x = band.x;",
        ["the_button_sits_under_the_dice_it_rolls"],
    ),
    (
        "the button says the same thing whatever a click on it would do",
        "            GamePhase::MustScore => (OVERLAY0, \"No Rolls Left\"),",
        "            GamePhase::MustScore => (OVERLAY0, \"Roll (R)\"),",
        ["the_button_says_what_the_click_will_do"],
    ),
    # -- The scorecard -------------------------------------------------
    #
    # The rows were described twice: a running `row_y` in the painting and
    # `cat_index + 3` in the click.  The Yahtzee-bonus row appears only after a
    # second Yahtzee, which is exactly the row two descriptions cannot agree on.
    (
        "the upper tally is inserted at a counted index rather than at the section boundary",
        "            if in_upper && !cat.is_upper() {",
        "            if i == 5 {",
        ["the_upper_tally_and_its_bonus_close_the_upper_section"],
    ),
    (
        "the bonus row is listed whether or not a bonus was earned",
        "        if self.yahtzee_bonus_count > 0 {",
        "        if true {",
        ["the_yahtzee_bonus_row_appears_only_once_a_bonus_is_earned"],
    ),
    (
        "the bonus row is never listed, so an earned bonus is invisible",
        "        if self.yahtzee_bonus_count > 0 {\n            rows.push(Row::YahtzeeBonus);\n        }",
        "",
        ["the_yahtzee_bonus_row_appears_only_once_a_bonus_is_earned"],
    ),
    (
        "the rows are placed by a second description of their order",
        "        for (n, row) in rows.iter().enumerate() {\n"
        "            let band = Rect::new(area.x, area.y + row_h * n as f32, area.w, row_h);",
        "        for (n, row) in rows.iter().enumerate() {\n"
        "            let n = n.saturating_sub(3);\n"
        "            let band = Rect::new(area.x, area.y + row_h * n as f32, area.w, row_h);",
        ["the_rows_are_the_same_height_and_do_not_overlap"],
    ),
    (
        "the rows are a fixed height, so a short card drops the last categories",
        "        let row_h = (area.h / count).clamp(l.small, l.font * 2.2);",
        "        let row_h = 28.0;",
        # Not "draws past its bottom", which was the first guess: the card's
        # own guard stops drawing once a row would fall off the end, so the
        # damage is silent rather than visible.  At 400x300 exactly nine of the
        # eighteen rows fit and the player simply cannot reach Yahtzee.
        ["every_category_has_a_box_of_its_own"],
    ),
    (
        "the row height has no floor, so a short window squeezes them flat",
        "        let row_h = (area.h / count).clamp(l.small, l.font * 2.2);",
        "        let row_h = (area.h / count).min(l.font * 2.2);",
        ["a_window_too_short_for_the_card_drops_rows_rather_than_squashing_them"],
    ),
    (
        "a row that falls past the bottom is drawn anyway",
        "            if band.bottom() > area.bottom() + 0.01 {\n"
        "                // The card ran out of window. A row drawn past the bottom edge\n"
        "                // is a row painted over whatever the compositor puts there.\n"
        "                break;\n"
        "            }",
        "",
        ["a_window_too_short_for_the_card_drops_rows_rather_than_squashing_them"],
    ),
    (
        "the score column is a fixed 80 pixels, so a narrow card overlaps the name",
        "        let score_w = (band.w * SCORE_SHARE).min(band.w);",
        "        let score_w = 80.0;",
        ["the_rows_are_wide_enough_for_a_name_and_a_score"],
    ),
    (
        "every row is drawn at the top of the card, so the card shows one row",
        "            let band = Rect::new(area.x, area.y + row_h * n as f32, area.w, row_h);",
        "            let band = Rect::new(area.x, area.y, area.w, row_h);",
        ["the_rows_are_the_same_height_and_do_not_overlap"],
    ),
    (
        "a row is labelled with the category after it",
        "        Category::ALL.get(index).copied()",
        "        Category::ALL.get(index.saturating_add(1)).copied()",
        ["every_category_box_carries_that_category_s_name"],
    ),
    (
        "a scored box shows the box's number rather than its score",
        "                let filled = self.score_at(i);",
        "                let filled = self.score_at(i).map(|_| i as u16);",
        ["a_scored_box_shows_its_score"],
    ),
    # -- The mouse -----------------------------------------------------
    #
    # There was no mouse handling in the toolkit sense: the old click
    # recomputed the geometry from the constants the drawing used.
    (
        "every mouse button plays the game",
        "        if button != MouseButton::Left {\n            return EventResult::Ignored;\n        }",
        "",
        ["only_the_left_button_plays_the_game"],
    ),
    (
        "a click on a die moves the cursor but does not hold it",
        "                let held = self.toggle_hold(i);",
        "                let held = false;",
        ["clicking_a_die_holds_it_and_clicking_it_again_lets_it_go"],
    ),
    (
        "a click on a die always holds the first one",
        "            Target::Die(i) => {",
        "            Target::Die(_ignored) => {\n                let i = 0usize;",
        ["a_click_lands_on_the_die_it_is_over_and_no_other"],
    ),
    (
        "a click on the button rolls even when the game is over",
        "                if self.phase() == GamePhase::GameOver {\n                    self.new_game();",
        "                if false {\n                    self.new_game();",
        ["clicking_the_button_after_the_last_box_starts_a_new_game"],
    ),
    (
        # Splitting the variant out of the ignore group rather than deleting it
        # from the group: deleting a variant from a `match` arm leaves the
        # `match` non-exhaustive, and a mutation that will not compile tests
        # nothing.  The arm has to still be there and do the wrong thing.
        "a click on a tally row spends the category nearest it",
        "            Target::Title\n            | Target::Turn",
        "            Target::Tally(_) => {\n"
        "                let _spent = self.score_category(self.selected_category);\n"
        "                EventResult::Consumed\n"
        "            }\n"
        "            Target::Title\n"
        "            | Target::Turn",
        ["every_tally_row_is_read_only"],
    ),
    (
        "a click on the title is taken for a move",
        "            Target::Title\n            | Target::Turn",
        "            Target::Title => {\n"
        "                let _rolled = self.roll();\n"
        "                EventResult::Consumed\n"
        "            }\n"
        "            Target::Turn",
        ["clicking_the_furniture_is_not_a_move"],
    ),
    (
        "a click anywhere at all is a move, even outside the window",
        "        let Some(target) = self.frame(self.width, self.height).hit_test(x, y) else {\n"
        "            return EventResult::Ignored;\n"
        "        };",
        "        let target = self\n"
        "            .frame(self.width, self.height)\n"
        "            .hit_test(x, y)\n"
        "            .unwrap_or(Target::RollButton);",
        ["a_click_outside_the_window_is_not_a_move"],
    ),
    # -- The keyboard --------------------------------------------------
    #
    # `handle_key` returned `()`, so every release looked exactly like a move
    # and repainted the whole window.
    (
        "a key release is handled like a press",
        "        if !key.pressed {\n            return EventResult::Ignored;\n        }",
        "",
        ["a_key_release_is_ignored"],
    ),
    (
        "the arrows move whichever cursor, regardless of the focus",
        "            Key::Right if self.focus == FocusRegion::Dice => {",
        "            Key::Right => {",
        ["the_arrows_only_move_the_cursor_that_has_the_focus"],
    ),
    (
        "the cursor walks off the end of the dice",
        "                let last = NUM_DICE.saturating_sub(1);\n"
        "                if self.selected_die >= last {\n"
        "                    return EventResult::Ignored;\n"
        "                }\n"
        "                self.selected_die = self.selected_die.saturating_add(1).min(last);",
        "                self.selected_die = self.selected_die.saturating_add(1);",
        ["the_cursor_does_not_walk_off_either_end_of_the_dice"],
    ),
    (
        "the cursor walks off the end of the card",
        "                let last = NUM_CATEGORIES.saturating_sub(1);\n"
        "                if self.selected_category >= last {\n"
        "                    return EventResult::Ignored;\n"
        "                }\n"
        "                self.selected_category = self.selected_category.saturating_add(1).min(last);",
        "                self.selected_category = self.selected_category.saturating_add(1);",
        ["the_cursor_does_not_walk_off_either_end_of_the_card"],
    ),
    (
        "the number keys are off by one",
        "                    Key::Num1 => 0,",
        "                    Key::Num1 => 1,",
        ["the_number_keys_hold_the_die_they_name"],
    ),
    (
        "tab only ever moves the focus one way",
        "                    FocusRegion::Scorecard => FocusRegion::Dice,",
        "                    FocusRegion::Scorecard => FocusRegion::Scorecard,",
        ["tab_moves_the_cursor_between_the_dice_and_the_card"],
    ),
    (
        "space always holds a die, whichever half has the focus",
        "                    FocusRegion::Scorecard => self.score_category(self.selected_category),",
        "                    FocusRegion::Scorecard => self.toggle_hold(self.selected_die),",
        ["space_holds_a_die_or_spends_a_box_depending_on_the_focus"],
    ),
    (
        "a key that does nothing still asks for a repaint",
        "            _ => return EventResult::Ignored,\n        }\n        EventResult::Consumed",
        "            _ => {}\n        }\n        EventResult::Consumed",
        ["a_key_the_game_does_not_use_is_ignored"],
    ),
    # -- The window ----------------------------------------------------
    (
        "the render pass ignores the size the window hands it",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.resize(width, height);\n"
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()",
        ["the_render_pass_uses_the_size_the_window_hands_it"],
    ),
    (
        "the window opens at a size the layout was not written against",
        "    fn initial_size(&self) -> (u32, u32) {",
        "    fn initial_size(&self) -> (u32, u32) {\n        return (700, 700);",
        ["the_window_opens_at_the_size_the_layout_was_written_against"],
    ),
    (
        "the close button does not close the window",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "",
        ["the_close_button_closes_the_window"],
    ),
    (
        "an event that changed nothing still asks for a redraw",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["an_event_that_changed_nothing_does_not_ask_for_a_redraw"],
    ),
    (
        "a resize is not remembered, so the next click lands where the window used to be",
        "        Event::Resize { width, height } => {\n"
        "            game.resize(f32_from_u32(*width), f32_from_u32(*height));",
        "        Event::Resize { width, height } => {\n"
        "            let _ = (width, height);",
        ["a_resize_is_remembered_so_the_next_click_lands"],
    ),
    # -- The rules -----------------------------------------------------
    #
    # These the old suite did cover; they are here so a rewrite of the
    # geometry cannot quietly break the game underneath it.
    (
        "a die can be held before it has ever been rolled",
        "        if self.roll_number == 0 || self.roll_number >= MAX_ROLLS {\n            return false;\n        }",
        "",
        ["a_die_cannot_be_held_before_it_has_been_rolled"],
    ),
    (
        "a fourth roll is allowed",
        "        } else if self.roll_number >= MAX_ROLLS {",
        "        } else if false {",
        ["clicking_the_button_rolls_and_stops_after_three", "r_rolls_until_there_are_no_rolls_left"],
    ),
    (
        "a box can be spent twice",
        "        if self.score_at(cat_index).is_some() {\n            return false;\n        }",
        "",
        ["clicking_a_spent_category_a_second_time_changes_nothing"],
    ),
    (
        "a new game forgets the high score",
        "    fn new_game(&mut self) {",
        "    fn new_game(&mut self) {\n        self.high_score = 0;",
        ["n_starts_a_new_game_and_keeps_the_high_score"],
    ),
    (
        "the game never ends, so the last box can be spent again",
        "        if self.turn_number >= NUM_TURNS {",
        "        if false {",
        ["a_box_that_is_already_filled_cannot_be_spent_twice"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "yahtzee", timeout=300, only=sys.argv[1:] or None))

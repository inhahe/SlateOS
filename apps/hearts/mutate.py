"""Mutation test for hearts' suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Hearts is the forty-second application in this campaign.  It had 84 tests, all
of them about the rules -- what a trick is worth, who takes it, which cards may
be played -- and not one about the window, because there was no window: `main`
was `let _app = Hearts::new();`.  The game shuffled a deck, dealt four hands,
built a status line and dropped the whole thing on the next line.  Nothing was
ever displayed, nothing could be clicked, and no clock ever ran.

What that hid, in rough order of how badly it would have shown:

  * **The picture was drawn at one size in a window of any other.**  `render`
    took a width and a height and passed neither to `render_trick_area`,
    `render_hand` or `render_opponent_labels`; `render_scores` took a width
    and named it `_width`.  Only `render_status` used what it was given.
  * **The hit test and the picture were two facts that could disagree, and did.**
    The click handler re-derived the hand from its own copies of `HAND_Y`,
    `HAND_X_START`, `CARD_OVERLAP` and `CARD_WIDTH` -- while the drawing pass
    lifted a card chosen for the pass sixteen pixels (`HAND_Y - 16.0`).  The
    top sixteen pixels of every chosen card were therefore unclickable, and the
    strip the click searched was the row the card had left.
  * **The hand was 516 pixels wide in every window there has ever been**:
    thirteen cards at a fixed 38-pixel step from a fixed `HAND_X_START = 60.0`.
  * **The scoreboard was pinned at `SCORE_X = 700.0`** by the method that took
    the window width and ignored it, so it left the right-hand edge of any
    window narrower than 860 and floated mid-felt in a wider one.
  * **The status line was drawn at `height - 18.0` and a controls hint at
    `height - 14.0`** -- four pixels apart, in the same font, one painted
    through the other -- and both were `max_width: None`, so a status naming a
    seat and a card ran off the edge of a narrow window.
  * **A finished trick vanished in the event that finished it.**  `finish_trick`
    did `mem::replace(&mut self.current_trick, Trick::new())` the instant the
    fourth card landed, and what the table then showed in its place was four
    `FillRect`s of `rgba(49, 50, 68, 180)`: blank grey rectangles with no rank
    and no suit on them.  The player never saw the trick they had played into.
  * **There was no clock.**  `handle_event` matched `Key` and `Mouse` and
    nothing else, and the machine players were moved along inside the human's
    click handler by `play_ai_turns()`.  Three seats answered instantly and
    simultaneously, in the middle of the human's own click.
  * **`completed_tricks: Vec<Trick>`** was pushed to on every trick, cleared on
    every round, and read by nothing.
  * **The seat labels were three literal offsets from a literal centre**
    (`TRICK_CENTER_X - 200.0`, `- 30.0`, `+ 140.0`), and said how many cards a
    seat held but not what it had taken -- which is the number that decides the
    round.
  * **The three machine hands were not drawn at all**, face down or otherwise.
  * **The window had no buttons.**  The only clickable thing in it was the
    hand; every other verb was a keystroke, listed once in the four-pixel hint
    that was painted through the status line.  There was no help.
  * **`Key::Left` carried its bound in the match arm's guard and `Key::Right`
    carried its own in the arm's body**, so the two directions were not the
    same code in any sense a reader could check.
  * **No modifier was ever examined**, and `Key::N` dealt a new game: Ctrl+N,
    which is the compositor's new window, threw the player's game away.
  * **`Trick::lead_suit` was a field written by `play`** -- the same fact in two
    places.  A trick built any other way carried cards and no lead suit, and
    `winner` answered `None` for it.
  * **Every launch dealt the same game.**  `SeededRng::new(42)`, a fixed seed
    shuffling a fixed deck: the identical thirteen cards, every time the
    program was opened.

One thing that reads like a fault and is not, recorded here so that nobody
"fixes" it twice: the no-points-on-the-first-trick rule used to be written only
in the cannot-follow branch of `valid_plays`.  That is the wrong shape -- it is
a rule about the first trick, not about one path through it -- and it is now
written once, after the branch.  But it is not a behaviour change: the first
trick is always led with the two of clubs, so the follow branch on trick 0 can
only ever offer clubs, which hold no points, and the leading branch is
pre-empted by the forced two of clubs.  The void branch was the only one that
could ever offer a point card.  The restructuring is for the reader.

Usage:  python -u apps/hearts/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The window ────────────────────────────────────────────────────────
    (
        "a close request does not close the window",
        "        if matches!(event, Event::CloseRequested) {\n"
        "            return Response::Exit;\n        }",
        "        {}",
        ["closing_the_window_exits"],
    ),
    (
        "the program asks for no clock",
        "        // advanced when the human touched it.\n"
        "        Some(std::time::Duration::from_millis(TICK_MS))",
        "        // advanced when the human touched it.\n        None",
        ["the_program_asks_for_a_window_and_a_clock"],
    ),
    (
        "render draws at a constant size rather than the window's",
        "        self.size = (width, height);\n"
        "        self.frame(width, height).into_tree()",
        "        self.size = (width, height);\n"
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()",
        ["render_draws_at_the_window_it_was_given_and_records_it"],
    ),
    (
        "render does not record the window it was given",
        "        self.size = (width, height);\n"
        "        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["render_draws_at_the_window_it_was_given_and_records_it"],
    ),
    (
        "a move does not ask for a redraw",
        "            EventResult::Consumed => Response::Redraw,",
        "            EventResult::Consumed => Response::Idle,",
        ["a_move_asks_for_a_redraw_and_a_dead_key_does_not"],
    ),
    # ── The layout follows the window ─────────────────────────────────────
    (
        "the layout ignores the window it was given",
        "    fn solve(w: f32, h: f32) -> Self {\n"
        "        let w = w.max(0.0);\n        let h = h.max(0.0);",
        "    fn solve(w: f32, h: f32) -> Self {\n"
        "        let _ = (w, h);\n        let w = WINDOW_WIDTH;\n"
        "        let h = WINDOW_HEIGHT;",
        ["a_frame_is_drawn_at_the_size_it_is_given_not_the_size_it_remembers"],
    ),
    (
        "the card is sized by the window's width alone",
        "        let card_w = (w / 11.0).min(free_h / 5.6).clamp(0.0, MAX_CARD_W);",
        "        let card_w = (w / 11.0).clamp(0.0, MAX_CARD_W);",
        # Not the seat labels: a label that cannot sit clear of an oversized
        # card is left out now, so it is no longer a witness against one.
        ["every_card_played_is_drawn_inside_the_table"],
    ),
    (
        "a card may grow without limit",
        "        let card_w = (w / 11.0).min(free_h / 5.6).clamp(0.0, MAX_CARD_W);",
        "        let card_w = (w / 11.0).min(free_h / 5.6).max(0.0);",
        ["a_wide_window_lays_the_hand_out_without_hiding_any_card"],
    ),
    (
        "the hand strip is not kept clear of the buttons",
        "        let hand_h = card_h.min((free_h - footer_h).max(0.0));",
        "        let hand_h = card_h.min(free_h);",
        ["the_panes_are_stacked_and_do_not_overlap"],
    ),
    (
        "the scoreboard is drawn however narrow the window",
        "        let scores = if scores_w >= MIN_SCORES_WIDTH && table.h >= scores_h {\n"
        "            Rect::new(table.right() - scores_w, table.y + pad, scores_w, scores_h)\n"
        "        } else {\n            Rect::EMPTY\n        };",
        "        let scores = Rect::new(table.right() - scores_w, table.y + pad, scores_w, scores_h);",
        ["the_scoreboard_is_inside_the_table_or_is_left_out"],
    ),
    (
        "the hand is drawn at a fixed step whatever the window",
        "        ((self.hand.w - cw) / gaps).clamp(0.0, cw)",
        "        38.0f32.min(cw)",
        ["a_narrow_window_closes_the_hand_up_rather_than_running_off_the_edge"],
    ),
    (
        "the hand is not centred in its strip",
        "        let x0 = self.hand.x + (self.hand.w - span) / 2.0;",
        "        let x0 = self.hand.x;",
        ["the_hand_is_centred_in_its_strip"],
    ),
    (
        "every card of the hand is drawn in the same place",
        "        Rect::new(\n            step.mul_add(fi, x0),",
        "        Rect::new(\n            step.mul_add(0.0, x0),",
        ["the_cards_of_the_hand_run_left_to_right_in_order"],
    ),
    (
        "the four seats play their cards on top of one another",
        "        let (dx, dy) = match seat % SEATS {\n"
        "            0 => (0.0, ch * 0.55),\n"
        "            1 => (-cw * 1.05, 0.0),\n"
        "            2 => (0.0, -ch * 0.55),\n"
        "            _ => (cw * 1.05, 0.0),\n        };",
        "        let (dx, dy) = ((seat % SEATS) as f32 * 0.0, 0.0);",
        ["each_seat_plays_its_card_in_its_own_place"],
    ),
    (
        "a seat label is drawn on top of the card it names",
        "        if label.intersect(self.trick_card(seat)).is_some() {\n"
        "            return Rect::EMPTY;\n        }",
        "        {}",
        ["the_seat_labels_stay_on_the_felt_and_clear_of_the_trick"],
    ),
    (
        "the seat labels are placed without regard to the felt's edges",
        "            1 => Rect::new((cx - cw * 1.6 - w).max(self.table.x), cy - h / 2.0, w, h),",
        "            1 => Rect::new(cx - cw * 1.6 - w, cy - h / 2.0, w, h),",
        ["the_seat_labels_stay_on_the_felt_and_clear_of_the_trick"],
    ),
    # ── The rules of a trick ──────────────────────────────────────────────
    (
        "the first trick is not led with the two of clubs",
        "        if leading\n            && first_trick\n"
        "            && let Some(i) = hand.iter().position(|&c| c == Card::TWO_OF_CLUBS)\n"
        "        {\n            return vec![i];\n        }",
        "        let _ = leading;",
        ["the_first_trick_is_led_with_the_two_of_clubs"],
    ),
    (
        "the led suit need not be followed",
        "            Some(lead) if hand.iter().any(|c| c.suit == lead) => all(&|c| c.suit == lead),",
        "            Some(_) => everything(),",
        ["following_suit_is_forced_when_the_suit_is_held"],
    ),
    (
        "a heart may be led before one has been played",
        "                let others = all(&|c| !c.is_heart());",
        "                let others = all(&|_| true);",
        ["hearts_are_not_led_until_one_has_been_played"],
    ),
    (
        "a hand of nothing but hearts may lead nothing at all",
        "                if others.is_empty() {\n                    everything()\n"
        "                } else {\n                    others\n                }",
        "                others",
        ["a_hand_of_nothing_but_hearts_may_lead_one"],
    ),
    (
        "a point card may be discarded on the first trick",
        "        if first_trick {\n            let clean: Vec<usize> = candidates\n"
        "                .iter()\n                .copied()\n"
        "                .filter(|&i| hand.get(i).is_some_and(|c| c.point_value() == 0))\n"
        "                .collect();\n            if !clean.is_empty() {\n"
        "                return clean;\n            }\n        }",
        "        {}",
        ["no_point_card_is_discarded_on_the_first_trick"],
    ),
    (
        "the first-trick rule does not yield when points are all that is left",
        "            if !clean.is_empty() {\n                return clean;\n            }",
        "            return clean;",
        ["the_first_trick_rule_yields_when_points_are_all_that_is_left"],
    ),
    (
        "playing a heart does not break hearts",
        "        if card.is_heart() {\n            self.hearts_broken = true;\n        }",
        "        {}",
        ["playing_a_heart_breaks_hearts"],
    ),
    (
        "the lead suit is taken from the last card played, not the first",
        "        self.cards.first().map(|tc| tc.card.suit)",
        "        self.cards.last().map(|tc| tc.card.suit)",
        ["a_trick_names_its_lead_suit_from_the_card_that_was_led"],
    ),
    (
        "the trick goes to the highest card of any suit",
        "            .filter(|tc| tc.card.suit == lead)\n"
        "            .max_by_key(|tc| tc.card.rank.value())",
        "            .max_by_key(|tc| tc.card.rank.value())",
        ["the_highest_card_of_the_led_suit_takes_the_trick"],
    ),
    (
        "the queen of spades is worth what an ordinary spade is",
        "        if matches!(self.suit, Suit::Spades) && matches!(self.rank, Rank::Queen) {\n"
        "            13",
        "        if matches!(self.suit, Suit::Spades) && matches!(self.rank, Rank::Queen) {\n"
        "            0",
        ["a_trick_is_worth_a_point_a_heart_and_thirteen_for_the_queen"],
    ),
    (
        "the trick's points go to the seat that led rather than the seat that took",
        "        if let Some(p) = self.round_points.get_mut(taker) {",
        "        if let Some(p) = self.round_points.get_mut(0) {",
        ["the_taker_of_a_trick_takes_its_points_and_leads_the_next"],
    ),
    (
        "the taker does not lead the next trick",
        "        self.turn = taker;\n"
        "        self.trick_number = self.trick_number.saturating_add(1);",
        "        self.turn = 0;\n"
        "        self.trick_number = self.trick_number.saturating_add(1);",
        ["the_taker_of_a_trick_takes_its_points_and_leads_the_next"],
    ),
    (
        "play passes to the wrong seat",
        "            self.turn = player.saturating_add(1) % SEATS;",
        "            self.turn = player.saturating_add(2) % SEATS;",
        ["a_round_is_thirteen_tricks_and_then_it_is_scored"],
    ),
    (
        "the round never reaches its thirteenth trick",
        "        if self.trick_number >= HAND_SIZE {",
        "        if self.trick_number > HAND_SIZE {",
        ["a_round_is_thirteen_tricks_and_then_it_is_scored"],
    ),
    # ── Rounds and scores ─────────────────────────────────────────────────
    (
        "the moon scores the shooter and spares the table",
        "                if seat != shooter {",
        "                if seat == shooter {",
        ["shooting_the_moon_scores_everyone_else_instead"],
    ),
    (
        "a near miss counts as the moon",
        "        self.round_points.iter().position(|&p| p == MOON)",
        "        self.round_points.iter().position(|&p| p >= 25)",
        ["twenty_five_points_is_not_the_moon"],
    ),
    (
        "the game never ends",
        "        if self.scores.iter().copied().max().unwrap_or(0) >= GAME_OVER_SCORE {",
        "        if self.scores.iter().copied().max().unwrap_or(0) >= u32::MAX {",
        ["the_game_ends_when_a_seat_reaches_a_hundred"],
    ),
    (
        "the game ends on the first point scored",
        "        if self.scores.iter().copied().max().unwrap_or(0) >= GAME_OVER_SCORE {",
        "        if self.scores.iter().copied().max().unwrap_or(0) >= 1 {",
        ["a_game_short_of_a_hundred_carries_on"],
    ),
    (
        "the highest score wins the game",
        "        let low = self.scores.iter().copied().min()?;",
        "        let low = self.scores.iter().copied().max()?;",
        ["the_game_ends_when_a_seat_reaches_a_hundred"],
    ),
    (
        "the pass direction does not rotate",
        "        self.pass_direction = self.pass_direction.next();",
        "        self.pass_direction = self.pass_direction;",
        ["the_pass_direction_rotates_round_the_table"],
    ),
    # ── Dealing and passing ───────────────────────────────────────────────
    (
        "every seat is dealt the same thirteen cards",
        "                deck.iter()\n"
        "                    .skip(seat.saturating_mul(HAND_SIZE))\n"
        "                    .take(HAND_SIZE),",
        "                deck.iter().take(HAND_SIZE),",
        ["a_deal_gives_every_seat_thirteen_distinct_cards"],
    ),
    (
        "the deck is not shuffled",
        "        self.rng.shuffle(&mut deck);",
        "        let _ = &mut self.rng;",
        ["two_seeds_deal_two_different_games"],
    ),
    (
        "a launch deals from the seed the program used to carry",
        "        Self::with_seed(seed_from_system(0x4845_4152_5453))",
        "        Self::with_seed(42)",
        ["a_launch_does_not_deal_the_one_hand_that_was_hardcoded"],
    ),
    (
        "the hand is not held in order",
        "                    .take(HAND_SIZE),\n            );\n            hand.sort_unstable();",
        "                    .take(HAND_SIZE),\n            );",
        ["a_hand_is_held_in_suit_then_rank_order"],
    ),
    (
        "a fourth card can be chosen for the pass",
        "        } else if self.chosen.len() < PASS_SIZE {",
        "        } else if self.chosen.len() < PASS_SIZE + 1 {",
        ["three_cards_are_chosen_and_no_more"],
    ),
    (
        "the pass goes through with fewer than three cards",
        "        if self.phase != GamePhase::Passing || self.chosen.len() != PASS_SIZE {",
        "        if self.phase != GamePhase::Passing {",
        ["the_pass_is_refused_until_three_cards_are_chosen"],
    ),
    (
        "the pass always goes left",
        "            let to = self.pass_direction.target(from);",
        "            let to = PassDirection::Left.target(from);",
        ["passing_gives_the_cards_to_the_seat_the_direction_names"],
    ),
    (
        "the cards are removed lowest index first, so the wrong ones go",
        "            indices.sort_unstable();\n            indices.reverse();",
        "            indices.sort_unstable();",
        ["passing_gives_the_cards_to_the_seat_the_direction_names"],
    ),
    (
        "the machine players give nothing away",
        "                self.ai_pass_choice(seat)",
        "                Vec::new()",
        ["every_seat_still_holds_thirteen_after_the_pass"],
    ),
    (
        "a machine player passes its lowest cards rather than its most dangerous",
        "            want.saturating_neg()",
        "            want",
        ["a_machine_player_gives_away_the_queen_of_spades"],
    ),
    (
        "the no-pass round still stops to ask for three cards",
        "        if self.pass_direction == PassDirection::Keep {\n"
        "            self.begin_play();\n        } else {",
        "        if false {\n            self.begin_play();\n        } else {",
        ["the_round_with_no_pass_is_dealt_straight_into_play"],
    ),
    # ── The clock ─────────────────────────────────────────────────────────
    (
        "a machine player answers instantly",
        "        self.think_ms = if self.turn == 0 { 0 } else { THINK_MS };",
        "        self.think_ms = if self.turn == 0 { 0 } else { 1 };",
        ["a_machine_player_plays_on_the_clock_and_not_before"],
    ),
    (
        "a finished trick is swept in the event that finished it",
        "        self.sweep_ms = SWEEP_MS;",
        "        self.sweep_ms = 1;",
        ["a_finished_trick_stays_on_the_table_before_it_is_swept"],
    ),
    (
        "the table is ready for a card while a settled trick is still shown",
        "        self.sweep_ms == 0",
        "        true",
        ["nobody_plays_while_a_settled_trick_is_being_shown"],
    ),
    (
        "the clock is armed for the human as well as the machines",
        "        self.think_ms = if self.turn == 0 { 0 } else { THINK_MS };",
        "        self.think_ms = THINK_MS;",
        ["the_clock_does_nothing_while_it_is_the_humans_turn"],
    ),
    # ── The keyboard and the pointer ──────────────────────────────────────
    (
        "the selection runs off the right end of the hand",
        "            self.selected.saturating_add(step.unsigned_abs()).min(last)",
        "            self.selected.saturating_add(step.unsigned_abs())",
        ["the_arrows_move_the_selection_and_stop_at_both_ends"],
    ),
    (
        "a key release moves the selection as well as a press",
        "        if !event.pressed {\n            return EventResult::Ignored;\n        }",
        "        {}",
        ["a_key_release_does_nothing"],
    ),
    (
        "a key with a modifier is taken by the game",
        "        if event.modifiers.ctrl || event.modifiers.alt || event.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n        }",
        "        {}",
        ["a_key_with_a_modifier_is_left_to_the_window"],
    ),
    (
        "a click is resolved against a frame drawn at another size",
        "        let (w, h) = self.size;\n"
        "        let changed = match self.frame(w, h).hit_test(event.x, event.y) {",
        "        let (w, h) = (self.size.0, self.size.1 - 16.0);\n"
        "        let changed = match self.frame(w, h).hit_test(event.x, event.y) {",
        ["every_button_does_what_its_key_does"],
    ),
    (
        "a click reaches the card underneath rather than the one on top",
        "            f.hit(Target::Card(i), r);",
        "            f.hit(Target::Card(n.saturating_sub(1).saturating_sub(i)), r);",
        ["a_click_where_two_cards_overlap_reaches_the_one_on_top"],
    ),
    (
        "the human may play out of turn",
        "        if self.phase != GamePhase::Playing || self.turn != 0 || !self.ready() {",
        "        if self.phase != GamePhase::Playing || !self.ready() {",
        ["a_click_on_a_card_out_of_turn_plays_nothing"],
    ),
    (
        "an illegal card is played without complaint",
        "        if !self.valid_plays(0).contains(&self.selected) {\n"
        "            self.status = String::from(\"That card cannot be played on this trick\");\n"
        "            return true;\n        }",
        "        {}",
        ["a_click_on_a_card_the_rules_forbid_says_so_and_plays_nothing"],
    ),
    (
        "the keyboard does not follow the pointer",
        "        self.selected = index;\n        match self.phase {",
        "        match self.phase {",
        ["a_click_on_a_card_chooses_it_for_the_pass"],
    ),
    (
        "Escape clears the pass instead of closing the help card",
        "        if self.show_help {\n            self.show_help = false;\n"
        "            return true;\n        }",
        "        {}",
        ["escape_closes_the_help_card_before_it_clears_a_choice"],
    ),
    (
        "the help card lets a click through to the table underneath",
        "        f.hit(Target::Help, card);",
        "        let _ = card;",
        ["the_help_card_covers_the_table_and_a_click_dismisses_it"],
    ),
    (
        "the selection is left pointing past the end of a shortened hand",
        "        self.selected = self.selected.min(self.hands[0].len().saturating_sub(1));",
        "        {}",
        ["the_selection_stays_on_a_card_that_exists"],
    ),
    # ── What the window says ──────────────────────────────────────────────
    (
        "a card on the table is drawn as a blank rectangle",
        "            draw_card_face(f, r, tc.card, false);",
        "            fill(f, r, SURFACE0, r.w * 0.1);",
        ["a_card_on_the_table_is_drawn_with_its_rank_and_its_suit"],
    ),
    (
        "the table does not say who took the trick",
        "            if self.taker == Some(tc.player) {\n"
        "                outline(f, r, GREEN, (l.card.0 * 0.07).clamp(1.0, 4.0));\n            }",
        "        {}",
        ["the_taker_of_the_trick_on_the_table_is_ringed"],
    ),
    (
        "a card the rules forbid is drawn as though it could be played",
        "            let dim = legal.as_ref().is_some_and(|valid| !valid.contains(&i));",
        "            let dim = false;",
        ["a_card_the_rules_forbid_is_drawn_dimmed"],
    ),
    (
        "the whole hand is greyed between the human's turns",
        "        let legal = (self.phase == GamePhase::Playing && self.turn == 0 && self.ready())\n"
        "            .then(|| self.valid_plays(0));",
        "        let legal = Some(self.valid_plays(0));",
        ["no_card_is_dimmed_when_it_is_not_the_humans_turn"],
    ),
    (
        "the selection is not marked",
        "            if i == self.selected {\n                outline(f, r, YELLOW, ring);",
        "            if false {\n                outline(f, r, YELLOW, ring);",
        ["the_selected_card_is_ringed_and_only_that_one"],
    ),
    (
        "the confirm button carries one label in every phase",
        "            Button::Confirm => confirm_label(self.phase, self.chosen.len()),",
        "            Button::Confirm => \"Confirm\",",
        ["the_confirm_button_says_what_pressing_it_will_do"],
    ),
    (
        "the help card lists the keys and not what they do",
        "        rows.extend(BUTTONS.iter().map(|&b| {\n            (\n"
        "                String::from(b.key_name()),\n"
        "                String::from(self.button_label(b)),\n            )\n        }));",
        "        rows.extend(BUTTONS.iter().map(|&b| {\n"
        "            (String::from(b.key_name()), String::new())\n        }));",
        ["the_help_card_lists_every_button_by_key_and_by_label"],
    ),
    (
        "the scoreboard shows the game score and not the round's",
        "            let value = format!(\"{total} (+{taken})\");",
        "            let value = format!(\"{total}\");",
        ["the_scoreboard_names_every_seat_and_shows_what_it_has"],
    ),
    (
        "a seat label says who it is and nothing more",
        "            let line = format!(\"{held} left \\u{00b7} {taken} pt\");",
        "            let line = String::new();",
        ["a_seat_label_says_who_it_is_and_how_much_is_left"],
    ),
    (
        "no seat is marked as the seat to play",
        "            let live = self.phase == GamePhase::Playing && self.turn == seat && self.ready();",
        "            let live = false;",
        ["the_seat_whose_turn_it_is_is_marked"],
    ),
    (
        "the felt does not say which way the pass goes",
        "            GamePhase::Passing => Some(String::from(self.pass_direction.label())),",
        "            GamePhase::Passing => None,",
        ["the_table_says_which_way_the_pass_goes"],
    ),
    (
        "the header does not say which round is being played",
        "        let right = format!(\n            \"Round {} \\u{2014} {}\",\n"
        "            self.round_number.saturating_add(1),\n"
        "            self.pass_direction.label()\n        );",
        "        let right = String::new();",
        ["the_header_says_which_round_is_being_played"],
    ),
    (
        "the status line is drawn unbounded",
        "            max_width: Some(budget),\n            overflow: TextOverflow::Ellipsis,",
        "            max_width: None,\n            overflow: TextOverflow::Clip,",
        ["the_status_line_is_bounded_by_the_window_it_is_drawn_in"],
    ),
    (
        "the status does not name the seat that took the trick",
        "        self.status = format!(\n            \"{} takes the trick ({points} pt{})\",\n"
        "            name(taker),\n"
        "            if points == 1 { \"\" } else { \"s\" }\n        );",
        "        self.status = String::from(\"Trick taken\");",
        ["the_status_line_says_what_just_happened"],
    ),
    (
        "the buttons run off the right-hand edge of a narrow window",
        "            if x + w > l.footer.right() - l.pad {\n                break;\n            }",
        "        {}",
        ["the_buttons_are_inside_the_footer_and_clear_of_the_status"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "hearts", timeout=300, only=only))

"""Mutation test for spades' suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Spades is the forty-third application in this campaign.  It had 90 tests, every
one of them about the rules -- what a bid is worth, who takes a trick, when a
bag becomes a hundred-point penalty -- and not one about the window, because
there was no window.  `main` was:

    fn main() {
        let _app = SpadesGame::new();
    }

It shuffled a deck, dealt four hands of thirteen, ran the machine seats' bids,
wrote a status line asking the human for theirs, and dropped the whole thing on
the next line.  Nothing was ever drawn, no key or click ever arrived, and no
clock ever ran.

What that hid, in rough order of how badly it would have shown:

  * **The picture was drawn at one size in a window of any other.**  `render`
    was `fn render(&self) -> Vec<RenderCommand>` -- it took no window size at
    all and opened by painting a `FillRect` of a literal 900x700.  Every
    rectangle in the picture was a compile-time constant: `HAND_Y = 580.0`,
    `TRICK_CENTER_X = 390.0`, `SIDEBAR_X = 720.0`, `FOOTER_Y = 675.0`, and two
    header boxes at literal x 230 and 470.
  * **The hit test and the picture were two facts that could disagree, and
    did.**  The click handler re-derived the hand from its own copies of the
    geometry and searched the strip `HAND_Y ..= HAND_Y + CARD_HEIGHT`, while
    the drawing pass lifted the selected card ten pixels to `HAND_Y - 10.0`.
    The top ten pixels of the card the player had chosen were the one part of
    it that could not be clicked, and the strip the click searched was the row
    the card had left.
  * **The bid pad was painted in one place and clicked in another.**  It was
    drawn from `TRICK_CENTER_X - 140.0` / `overlay_y + 75.0` and hit-tested
    from `TRICK_CENTER_X - 120.0` / `overlay_y + 50.0`: twenty pixels across
    and twenty-five down between the button and the thing that answered for it.
  * **The hand was 540 pixels wide in every window there has ever been**,
    centred on a `TRICK_CENTER_X` of 390 in a window 900 wide -- so it was not
    even centred in the window it was designed for.
  * **Every text was `max_width: None`**, so a status naming a seat and a card
    ran straight off the right edge of a narrow window, and so did the title.
  * **There was no clock.**  `handle_event` matched `Key` and `Mouse` and
    nothing else; `run_ai_bids_before_human` and `run_ai_plays` moved all three
    machine seats along inside the human's own event handler, so three seats
    answered instantly and simultaneously in the middle of one click.
  * **A settled trick sat at `TrickDone` for ever waiting on Enter** -- and
    Enter was also the only thing that unblocked the machine seats, so a game
    left alone stopped dead.
  * **The window had no buttons and no help**, only a one-line hint listing the
    keystrokes, painted unbounded near the bottom edge.
  * **`Key::Left` carried its bound in the match arm's guard and `Key::Right`
    carried its own in the arm's body** (and the same for Up and Down), so the
    two directions were not the same code in any sense a reader could check.
  * **No modifier was ever examined, and `Key::N` dealt a new game**, so Ctrl+N
    -- the compositor's new window -- threw the player's round away.
  * **Thirteen crate-level `#![allow]`s**, including `dead_code` and
    `unused_imports`, were hiding sixty-two real indexing and overflow lints, a
    `Trick::leader` field written by every construction and read by nothing, and
    a `Suit::color()` returning palette greens and blues illegible on a
    near-white card face.

Run it:

    python apps/spades/mutate.py            # every row
    python apps/spades/mutate.py 12 13      # only rows 12 and 13
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

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
        "the app never asks for a clock, so no machine seat ever acts",
        "        // played for all three of them inside the human's own event handler.\n"
        "        Some(std::time::Duration::from_millis(TICK_MS))",
        "        None",
        ["the_app_asks_for_a_clock"],
    ),
    (
        "every event asks for a repaint, forty times a second for ever",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["a_tick_that_moves_the_game_redraws_and_one_that_does_not_idles"],
    ),
    (
        "the window opens at a size the layout was not designed for",
        "    fn initial_size(&self) -> (u32, u32) {\n"
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "    fn initial_size(&self) -> (u32, u32) {\n        (320, 240)",
        ["the_window_opens_at_the_size_the_layout_was_designed_for"],
    ),
    (
        "the renderer forgets the size it drew at, so clicks land elsewhere",
        "        self.size = (width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        [
            "render_draws_at_the_window_it_was_given_and_records_it",
            "a_click_lands_on_what_was_drawn_in_that_window_not_the_last_one",
        ],
    ),
    (
        "the picture is drawn at the size it remembers, not the size it is given",
        "        let mut f = Frame::new(width, height);\n"
        "        let l = Layout::solve(width, height);",
        "        let mut f = Frame::new(width, height);\n"
        "        let l = Layout::solve(self.size.0, self.size.1);",
        ["a_frame_is_drawn_at_the_size_it_is_given_not_the_size_it_remembers"],
    ),
    # ── The panes ─────────────────────────────────────────────────────────
    (
        "the hand strip is not kept clear of the buttons",
        "        let hand_h = (card_h * HAND_STRIP_SLACK).min(strip_h);",
        "        let hand_h = card_h * HAND_STRIP_SLACK;",
        ["the_panes_are_stacked_and_do_not_overlap"],
    ),
    (
        "a card is measured against the free height rather than its own strip",
        "            .min(strip_h / (CARD_ASPECT * HAND_STRIP_SLACK))",
        "            .min(f32::INFINITY)",
        ["the_hand_fits_the_window_it_is_drawn_in"],
    ),
    (
        "a card may grow without limit",
        "            .clamp(0.0, MAX_CARD_W);",
        "            .max(0.0);",
        ["a_wide_window_lays_the_hand_out_without_hiding_any_card"],
    ),
    (
        "the footer may be taller than the space there is for it",
        "        let footer_h = (small * 2.4).min(free_h);",
        "        let footer_h = small * 2.4;",
        ["the_panes_are_stacked_and_do_not_overlap"],
    ),
    (
        "the status bar may be taller than the window below the header",
        "        let status_h = (font + pad * 1.1).min((h - header.h).max(0.0));",
        "        let status_h = font + pad * 1.1;",
        ["the_panes_are_stacked_and_do_not_overlap"],
    ),
    (
        "the header may be taller than the window",
        "        let header = Rect::new(0.0, 0.0, w, (title + pad * 1.6).min(h));",
        "        let header = Rect::new(0.0, 0.0, w, title + pad * 1.6);",
        ["every_pane_stays_inside_the_window"],
    ),
    (
        "the felt runs off the right-hand edge",
        "        let table = Rect::new(\n            pad,\n"
        "            header.bottom(),\n            (w - pad * 2.0).max(0.0),",
        "        let table = Rect::new(\n            pad,\n"
        "            header.bottom(),\n            w,",
        ["every_pane_stays_inside_the_window"],
    ),
    # ── The hand: painted where it is clicked ─────────────────────────────
    (
        "the fan is stepped without regard to the card's own width",
        "        ((self.hand.w - cw) / gaps).clamp(0.0, cw)",
        "        ((self.hand.w - cw) / gaps).max(0.0)",
        ["a_wide_window_lays_the_hand_out_without_hiding_any_card"],
    ),
    (
        "the fan is laid out from the left edge rather than centred",
        "        let x0 = self.hand.x + (self.hand.w - span) / 2.0;",
        "        let x0 = self.hand.x - self.pad;",
        ["the_hand_fits_the_window_it_is_drawn_in"],
    ),
    (
        "the chosen card is painted lifted and hit-boxed where it used to be",
        "            f.hit(Target::Card(i), r);",
        "            f.hit(Target::Card(i), l.hand_card(i, n));",
        ["a_chosen_card_lifts_and_its_hit_box_lifts_with_it"],
    ),
    (
        "no card is lifted at all, so nothing shows which one is chosen",
        "                l.hand_card(i, n).translated(0.0, -l.hand_lift())",
        "                l.hand_card(i, n)",
        ["a_chosen_card_lifts_and_its_hit_box_lifts_with_it"],
    ),
    (
        "the strip carries no slack, so a lifted card leaves it",
        "const HAND_STRIP_SLACK: f32 = 1.14;",
        "const HAND_STRIP_SLACK: f32 = 1.0;",
        ["a_chosen_card_lifts_and_its_hit_box_lifts_with_it"],
    ),
    (
        "the hand is hit-boxed on a rectangle nobody painted",
        "            draw_card_face(f, r, card, choosing && !legal.contains(&i));",
        "            draw_card_face(f, r.translated(0.0, 3.0), card, choosing && !legal.contains(&i));",
        ["the_hand_is_hit_boxed_where_it_is_painted"],
    ),
    (
        "the cards are recorded in an order that answers the one underneath",
        "        for (i, &card) in hand.iter().enumerate() {",
        "        for (i, &card) in hand.iter().enumerate().rev() {",
        ["a_click_where_two_cards_overlap_reaches_the_one_on_top"],
    ),
    # ── The felt ──────────────────────────────────────────────────────────
    (
        "the four seats play their cards on top of one another",
        "        let (dx, dy) = match seat % SEATS {\n"
        "            0 => (0.0, ch * 0.55),\n"
        "            1 => (cw * 1.05, 0.0),\n"
        "            2 => (0.0, -ch * 0.55),\n"
        "            _ => (-cw * 1.05, 0.0),\n        };",
        "        let (dx, dy) = (0.0, 0.0);",
        ["each_seat_plays_its_card_in_its_own_place"],
    ),
    (
        "the seat labels are placed without regard to the felt's edges",
        "                (cx + cw * 1.7).min(self.table.right() - w),",
        "                cx + cw * 1.7,",
        ["the_seat_labels_stay_on_the_felt_and_clear_of_their_own_card"],
    ),
    (
        "a label clamped onto the felt is drawn over the card it names",
        "        if label.intersect(self.trick_card(seat)).is_some() || label.intersect(self.panel).is_some()\n"
        "        {\n            return Rect::EMPTY;\n        }",
        "        {}",
        ["the_seat_labels_stay_on_the_felt_and_clear_of_their_own_card"],
    ),
    (
        "a settled trick is cleared in the event that settles it",
        "        self.last_trick = Some(self.current_trick.clone());",
        "        self.last_trick = None;",
        ["a_settled_trick_stays_face_up_and_names_who_took_it"],
    ),
    (
        "the ring is round the seat that led rather than the one that took it",
        "        let taker = if settled { trick.winner() } else { None };",
        "        let taker = if settled { trick.leader() } else { None };",
        ["a_settled_trick_stays_face_up_and_names_who_took_it"],
    ),
    (
        "the status does not name the seat that took the trick",
        '        self.status_message = format!("{} wins the trick", winner.name());',
        '        self.status_message = String::from("Trick over");',
        ["a_settled_trick_stays_face_up_and_names_who_took_it"],
    ),
    # ── The bid pad ───────────────────────────────────────────────────────
    (
        "the bid pad is drawn in one place and clicked in another",
        "            (cell + gap).mul_add(col, pad.x + self.pad),\n"
        "            (cell + gap).mul_add(row, pad.y + self.pad + self.font * 2.6),",
        "            (cell + gap).mul_add(col, pad.x + self.pad + 20.0),\n"
        "            (cell + gap).mul_add(row, pad.y + self.pad + self.font * 2.6 - 25.0),",
        ["every_bid_button_is_inside_the_pad_that_holds_it"],
    ),
    (
        "a bid pad too big for the felt is squeezed on anyway",
        "        if cell < 12.0 || w > self.table.w || h > self.table.h {\n"
        "            return Rect::EMPTY;\n        }",
        "        {}",
        ["every_bid_button_is_inside_the_pad_that_holds_it"],
    ),
    (
        "the bid buttons are hit-boxed somewhere they are not drawn",
        "            f.hit(Target::Bid(value), r);",
        "            f.hit(Target::Bid(value), r.translated(4.0, 4.0));",
        ["the_bid_pad_is_drawn_where_it_is_clicked"],
    ),
    (
        "a click on a bid does not bid it",
        "        self.bid_selection = value;\n        self.submit_human_bid()",
        "        self.bid_selection = value;\n        true",
        ["a_click_on_a_bid_bids_it"],
    ),
    # ── The footer, the keys and the help ─────────────────────────────────
    (
        "the buttons run off the right-hand edge of a narrow window",
        "            if x + w > l.footer.right() - l.pad {\n                break;\n            }",
        "        {}",
        ["a_button_too_wide_for_the_footer_is_left_out"],
    ),
    (
        "the buttons are hit-boxed somewhere they are not drawn",
        "            f.hit(Target::Button(button), r);",
        "            f.hit(Target::Button(button), r.translated(0.0, -6.0));",
        ["a_button_too_wide_for_the_footer_is_left_out"],
    ),
    (
        "a button does not do what its key does",
        "            Button::Sort => self.resort(),",
        "            Button::Sort => false,",
        ["every_button_does_what_its_key_does"],
    ),
    (
        "the confirm button says one thing and Enter does another",
        "            Button::Confirm => confirm_label(self.phase),",
        '            Button::Confirm => "Enter",',
        ["the_confirm_button_says_what_enter_will_do"],
    ),
    (
        "the help card does not list the buttons",
        "        rows.extend(BUTTONS.iter().map(|&b| {",
        "        rows.extend([].iter().map(|&b: &Button| {",
        ["the_help_card_lists_every_button_and_is_dismissed_by_a_click"],
    ),
    (
        "the help card does not swallow the click that dismisses it",
        "        f.hit(Target::Help, card);",
        "        {}",
        ["the_help_card_lists_every_button_and_is_dismissed_by_a_click"],
    ),
    (
        "the help card is drawn bigger than the window that holds it",
        "        let card_w = (inner + l.pad * 2.0).min(l.window.w);\n"
        "        let card_h = (rows_h + l.title * 2.2 + l.pad).min(l.window.h);",
        "        let card_w = inner + l.pad * 2.0;\n"
        "        let card_h = rows_h + l.title * 2.2 + l.pad;",
        ["the_help_card_fits_the_window_it_is_drawn_in"],
    ),
    (
        "a key with a modifier on it is taken from the window",
        "        if event.modifiers.ctrl || event.modifiers.alt || event.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n        }",
        "        {}",
        ["a_key_with_a_modifier_is_left_to_the_window"],
    ),
    (
        "a key release is handled as though it were a press",
        "        if !event.pressed {\n            return EventResult::Ignored;\n        }",
        "        {}",
        ["a_key_release_does_nothing"],
    ),
    (
        "a click on nothing is answered by the last thing that was drawn",
        "        let changed = match self.frame(w, h).hit_test(event.x, event.y) {",
        "        let changed = match self.frame(w, h).hits().last().map(|&(t, _)| t) {",
        ["a_click_on_nothing_changes_nothing"],
    ),
    # ── The text ──────────────────────────────────────────────────────────
    (
        "the status line is drawn unbounded",
        "        max_width: Some(width),\n        overflow: TextOverflow::Ellipsis,",
        "        max_width: None,\n        overflow: TextOverflow::Clip,",
        [
            "no_text_runs_off_the_window_it_is_drawn_in",
            "the_status_line_is_elided_rather_than_cut_off",
        ],
    ),
    (
        "the status is bounded by the whole window rather than by its own bar",
        "        let mut budget = (l.status.w - l.pad * 2.0).max(0.0);",
        "        let mut budget = l.window.w * 2.0;",
        [
            "no_text_runs_off_the_window_it_is_drawn_in",
            "the_status_line_is_elided_rather_than_cut_off",
        ],
    ),
    (
        "the title is drawn unbounded",
        "            (l.header.w - l.pad * 2.0).max(0.0),\n            TITLE,",
        "            f32::INFINITY,\n            TITLE,",
        ["no_text_runs_off_the_window_it_is_drawn_in"],
    ),
    (
        "a centred run too wide to centre hangs off both sides",
        "        (x + ((w - measured) / 2.0).max(0.0), y),\n        w,",
        "        (x + (w - measured) / 2.0, y),\n        f32::INFINITY,",
        ["no_text_runs_off_the_window_it_is_drawn_in"],
    ),
    (
        "a card's corner is written onto the felt beside it",
        "    let corner_w = (r.right() - corner_x).max(0.0);",
        "    let corner_w = f32::INFINITY;",
        ["no_text_runs_off_the_window_it_is_drawn_in"],
    ),
    # ── The clock ─────────────────────────────────────────────────────────
    (
        "a machine seat answers the instant it is asked, inside the human's click",
        "        self.think_ms = if waiting { THINK_MS } else { 0 };",
        "        self.think_ms = if waiting { 1 } else { 0 };",
        ["the_machine_seats_answer_on_a_clock_not_inside_the_human_s_click"],
    ),
    (
        "the clock is owed to the human as well as to the machines",
        "        let waiting = matches!(self.phase, Phase::Bidding | Phase::Playing)\n"
        "            && !self.current_player.is_human();",
        "        let waiting = matches!(self.phase, Phase::Bidding | Phase::Playing);",
        ["a_game_left_alone_reaches_the_human_and_stops_there"],
    ),
    (
        "a settled trick is swept before it has been seen",
        "        self.sweep_ms = SWEEP_MS;",
        "        self.sweep_ms = 1;",
        ["nobody_plays_while_a_settled_trick_is_still_on_the_table"],
    ),
    (
        "a settled trick is never swept, so the game stops dead",
        "        if self.sweep_ms > 0 {\n"
        "            self.sweep_ms = self.sweep_ms.saturating_sub(ms);",
        "        if self.sweep_ms > 0 {\n            self.sweep_ms = self.sweep_ms;",
        ["a_trick_left_alone_is_swept_and_the_next_one_starts"],
    ),
    (
        "the machines play on while a settled trick is still on the table",
        "        if self.sweep_ms > 0 {\n"
        "            self.sweep_ms = self.sweep_ms.saturating_sub(ms);\n"
        "            if self.sweep_ms == 0 {\n"
        "                self.advance_after_trick();\n            }\n"
        "            return true;\n        }",
        "        if self.sweep_ms > 0 {\n"
        "            self.sweep_ms = self.sweep_ms.saturating_sub(ms);\n"
        "            if self.sweep_ms == 0 {\n"
        "                self.advance_after_trick();\n            }\n        }",
        ["nobody_plays_while_a_settled_trick_is_still_on_the_table"],
    ),
    # ── The pointer, the panel and the deal ───────────────────────────────
    (
        "the pointer may walk off the end of the hand",
        "            self.selected_card\n                .saturating_add(step.unsigned_abs())\n"
        "                .min(last)",
        "            self.selected_card.saturating_add(step.unsigned_abs())",
        ["the_selection_stays_on_a_card_that_exists"],
    ),
    (
        "the pointer is left past the end when the hand shrinks under it",
        "        self.selected_card = self.selected_card.min(hand_len.saturating_sub(1));",
        "        {}",
        ["the_selection_stays_on_a_card_that_exists"],
    ),
    (
        "re-sorting leaves the pointer on whatever index it held",
        "        if let Some(card) = held\n"
        "            && let Some(i) = self.hands[0].iter().position(|c| *c == card)\n"
        "        {\n            self.selected_card = i;\n        }",
        "        {}",
        ["re_sorting_keeps_the_pointer_on_the_same_card"],
    ),
    (
        "the panel is squeezed into a felt that cannot hold it",
        "        let panel = if panel_w >= MIN_PANEL_W && table.h >= panel_h + pad * 2.0 {",
        "        let panel = if true {",
        ["the_panel_is_left_out_rather_than_squeezed"],
    ),
    (
        "the panel does not say what a partnership bid",
        '            let contract = format!(\n                "  bid {}, won {}",\n'
        "                self.team_bid(index),\n                self.team_tricks(index)\n            );",
        '            let contract = String::from("  in play");',
        ["the_panel_says_what_each_team_bid_and_what_it_has_taken"],
    ),
    (
        "a new game keeps the old game's score",
        "        self.teams = [TeamState::new(), TeamState::new()];",
        "        {}",
        ["a_new_game_from_the_button_deals_a_fresh_round"],
    ),
    (
        "every game is dealt from the same fixed seed",
        "        Self::with_rng(Rng::new(seed))",
        "        Self::with_rng(Rng::new(42))",
        ["two_different_seeds_deal_two_different_games"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "spades", timeout=300, only=only))

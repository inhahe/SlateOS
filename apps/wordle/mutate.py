"""Mutation test for the wordle suite.

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
        # The footer is measured from the bottom edge.  Laid out from the top
        # it sits on the header in every window.
        "the footer is placed from the top of the window",
        "        let footer = Rect::new(0.0, h - ftr_h, w, ftr_h);",
        "        let footer = Rect::new(0.0, ftr_h, w, ftr_h);",
        ["the_bands_stack_down_the_window_without_a_gap_or_an_overlap"],
    ),
    (
        # The keyboard stacks on the footer, so its `y` is the footer's minus
        # its own height.  Anchoring it to the window bottom puts it over the
        # footer whenever there is one.
        "the keyboard is stacked on the window rather than on the footer",
        "        let keyboard = Rect::new(pad, footer.y - kb_h, (w - pad * 2.0).max(0.0), kb_h);",
        "        let keyboard = Rect::new(pad, h - kb_h, (w - pad * 2.0).max(0.0), kb_h);",
        ["the_bands_stack_down_the_window_without_a_gap_or_an_overlap"],
    ),
    (
        "the message line is stacked on the window rather than on the keyboard",
        "        let message = Rect::new(0.0, keyboard.y - msg_h, w, msg_h);",
        "        let message = Rect::new(0.0, footer.y - msg_h, w, msg_h);",
        ["the_bands_stack_down_the_window_without_a_gap_or_an_overlap"],
    ),
    (
        # The keyboard takes what it asked for or what is left, whichever is
        # less.  Dropping the clamp lets it grow through everything above it.
        "the keyboard takes what it asked for whether or not there is room",
        "        let kb_h = kb_want.min((footer.y - above_kb).max(0.0));",
        "        let kb_h = kb_want;",
        ["the_keyboard_shrinks_rather_than_vanishing"],
    ),
    (
        # The drop order is footer, then message, then header: the chrome goes
        # before the puzzle, and the least useful chrome goes first.
        "the bands are given up in the opposite order",
        "const BAND_DROP_ORDER: [usize; 3] = [2, 1, 0];",
        "const BAND_DROP_ORDER: [usize; 3] = [0, 1, 2];",
        ["a_short_window_gives_up_the_footer_then_the_message_then_the_header"],
    ),
    (
        # A Wordle you cannot type into is not a smaller Wordle.  Putting the
        # keyboard in the droppable list is how it would come to vanish.
        "the keyboard is given up like the chrome",
        "        let kb_want = (h * 0.26).clamp(24.0, 180.0);",
        "        let kb_want = 0.0;",
        ["the_keyboard_shrinks_rather_than_vanishing"],
    ),
    # -- Layout: the grid ----------------------------------------------
    (
        # One number decides the whole grid.  Solving for it from the width
        # alone leaves the tiles square -- they just stop fitting the band.
        "the tile is solved from the width alone",
        "            let tile = (area.w / per_w).min(area.h / per_h).max(0.0);",
        "            let tile = (area.w / per_w).max(0.0);",
        ["the_tile_is_solved_from_whichever_dimension_binds"],
    ),
    (
        # A stretched grid is one whose tiles are no longer where a square hit
        # box says they are.
        "the grid is stretched to fill the area rather than kept square",
        "            let grid_h = down * tile + (down - 1.0) * gap;",
        "            let grid_h = area.h;",
        ["the_guess_grid_is_square_and_centred_in_what_is_left_over"],
    ),
    (
        "the grid is pushed to the top left of what is left over",
        "                area.y + (area.h - grid_h) / 2.0,",
        "                area.y,",
        ["the_guess_grid_is_square_and_centred_in_what_is_left_over"],
    ),
    (
        # The step is a tile plus a gap.  Stepping by the tile alone butts
        # every tile against the next.
        "the grid steps by the tile without its gap",
        "        let step = self.tile + self.gap;",
        "        let step = self.tile;",
        [
            "the_guess_grid_is_square_and_centred_in_what_is_left_over",
            "no_two_tiles_of_the_grid_overlap",
        ],
    ),
    (
        # Past the end of the word there is no tile.  Handing one out draws a
        # sixth letter into a five-letter game.
        "a column past the end of the word still gets a tile",
        "        if row >= MAX_GUESSES || col >= self.cols {\n"
        "            return Rect::EMPTY;\n"
        "        }\n",
        "",
        ["the_grid_has_a_tile_for_every_letter_of_every_guess_and_none_past_them"],
    ),
    # -- Layout: the keyboard ------------------------------------------
    (
        # Ten keys and nine gaps fill the band.  Dividing the whole band by ten
        # makes each key a gap too wide, so the row runs off the right edge.
        "the key width does not allow for the gaps between the keys",
        "        let key_w = ((keyboard.w - key_gap * 9.0) / 10.0).max(0.0);",
        "        let key_w = (keyboard.w / 10.0).max(0.0);",
        ["the_keyboard_is_ten_columns_wide_and_fills_its_band"],
    ),
    (
        # Three rows and two gaps fill the band.  Dividing the whole band by
        # three makes each row a gap too tall, so the bottom row runs off the
        # foot of the band -- which is the rule the band test owns.  The rows
        # still do not *overlap* (each is drawn from its own step), so the
        # overlap test is not the one that sees this.
        "the key height does not allow for the gaps between the rows",
        "        let key_h = ((keyboard.h - key_gap * 2.0) / 3.0).max(0.0);",
        "        let key_h = (keyboard.h / 3.0).max(0.0);",
        ["the_keyboard_is_ten_columns_wide_and_fills_its_band"],
    ),
    (
        # Row 1 is centred under row 0 by half a key.  Flush left it is a
        # keyboard nobody recognises.
        "the middle row is not indented under the top one",
        "            1 => 0.5,",
        "            1 => 0.0,",
        ["the_middle_row_is_centred_under_the_top_one"],
    ),
    (
        # Row 2 starts after the Enter key, which is one and a half wide.
        "the bottom row starts under the enter key rather than after it",
        "            2 => 1.5,",
        "            2 => 0.0,",
        ["nothing_on_the_keyboard_overlaps_anything_else_on_it"],
    ),
    (
        # Backspace ends where the keyboard ends.  Any other x leaves a ragged
        # right edge or hangs off the band.
        "backspace is not flush with the right edge of the keyboard",
        "            self.keyboard.x + 8.5 * self.key_step(),",
        "            self.keyboard.x + 8.0 * self.key_step(),",
        ["enter_and_backspace_bracket_the_bottom_row"],
    ),
    (
        "enter is only as wide as a letter key",
        "            (self.key_w * 1.5 + self.key_gap * 0.5).max(0.0),",
        "            self.key_w.max(0.0),",
        ["enter_and_backspace_bracket_the_bottom_row"],
    ),
    (
        "a key past the end of its row is handed a box anyway",
        "        if col >= letters.len() {\n            return Rect::EMPTY;\n        }\n",
        "",
        ["a_key_past_the_end_of_a_row_has_no_box"],
    ),
    # -- Layout: the header --------------------------------------------
    (
        "the buttons are stacked from the left of the header",
        "        let x0 = self.header.right() - self.pad - strip_w;",
        "        let x0 = self.header.x;",
        ["the_buttons_share_the_header_in_order_and_are_centred_in_it"],
    ),
    (
        "the buttons step by their width without the gap between them",
        "            *slot = Rect::new(x0 + i as f32 * (bw + self.pad), y, bw, bh);",
        "            *slot = Rect::new(x0 + i as f32 * bw, y, bw, bh);",
        ["the_buttons_share_the_header_in_order_and_are_centred_in_it"],
    ),
    (
        # The title takes the left-hand end, and stops where the buttons start.
        # Given the whole band it prints under them.
        "the title runs on under the buttons",
        "        let right = buttons.first().map_or(self.header.right(), |b| b.x);",
        "        let right = self.header.right();",
        ["the_title_stops_before_the_first_button_starts"],
    ),
    # -- The answering rules -------------------------------------------
    (
        "a guess of the wrong length is taken",
        "        if self.current_input.len() != self.target_len {",
        "        if self.current_input.len() > self.target_len {",
        ["a_guess_of_the_wrong_length_is_refused_and_says_why"],
    ),
    (
        "a guess that is not a word is taken",
        "        if !self.is_valid_word(&self.current_input) {\n"
        '            self.message = Some("Not in word list");\n'
        "            return true;\n"
        "        }\n",
        "",
        ["a_guess_that_is_not_in_the_word_list_is_refused_and_says_why"],
    ),
    (
        # A green consumes the letter it matched, so a second copy of that
        # letter in the guess has nothing left to claim.
        "a letter already claimed by a green is claimed again by a yellow",
        "                if target_used.get(j).copied().unwrap_or(false) {\n"
        "                    continue;\n"
        "                }\n",
        "",
        [
            "a_letter_is_answered_only_as_often_as_the_word_holds_it",
            "a_green_takes_its_letter_before_a_yellow_can_claim_it",
        ],
    ),
    (
        "a yellow does not consume the letter it found",
        "                    if let Some(u) = target_used.get_mut(j) {\n"
        "                        *u = true;\n"
        "                    }\n",
        "",
        ["a_letter_is_answered_only_as_often_as_the_word_holds_it"],
    ),
    (
        "a guess is answered against the case it was typed in",
        "            let g = guess.get(i).copied().unwrap_or(' ').to_ascii_lowercase();\n"
        "            let mut found = false;",
        "            let g = guess.get(i).copied().unwrap_or(' ');\n"
        "            let mut found = false;",
        ["a_guess_is_answered_the_same_whatever_case_it_is_typed_in"],
    ),
    # -- The keyboard's memory -----------------------------------------
    (
        # Only upgrade: a letter shown green must not go back to grey because a
        # later guess put it in the wrong place.
        "the keyboard is overwritten by the latest answer rather than upgraded",
        "            if should_update && let Some(slot) = self.keyboard_state.get_mut(idx) {",
        "            if let Some(slot) = self.keyboard_state.get_mut(idx) {",
        ["the_keyboard_never_forgets_something_it_already_knew"],
    ),
    (
        "the keyboard learns nothing from an answered guess",
        "        self.update_keyboard(&guess_arr, &eval);\n",
        "",
        [
            "the_keyboard_learns_what_the_answer_said_about_each_letter",
            "the_keyboard_is_drawn_in_what_it_has_learnt",
        ],
    ),
    # -- Hard mode ------------------------------------------------------
    (
        "hard mode does not check the guess against what has been revealed",
        "        if let Some(msg) = self.check_hard_mode(&self.current_input) {\n"
        "            self.message = Some(msg);\n"
        "            return true;\n"
        "        }\n",
        "",
        [
            "hard_mode_refuses_a_guess_that_moves_a_letter_already_shown_green",
            "hard_mode_refuses_a_guess_that_drops_a_letter_already_shown_yellow",
        ],
    ),
    (
        "hard mode is enforced even when it is switched off",
        "        if !self.hard_mode || self.guesses.is_empty() {",
        "        if self.guesses.is_empty() {",
        ["hard_mode_off_takes_a_guess_that_ignores_everything_revealed"],
    ),
    (
        # The rule is about guesses already answered, so the switch cannot turn
        # halfway through a game.
        "hard mode turns halfway through a game",
        "        if !self.guesses.is_empty() {\n            return false;\n        }\n",
        "",
        [
            "hard_mode_only_turns_before_the_first_guess",
            "clicking_hard_mode_turns_it_and_stops_turning_once_a_guess_is_in",
        ],
    ),
    # -- Winning, losing and the totals --------------------------------
    (
        "a loss does not break the streak",
        "            self.streak = 0;",
        "            self.streak = self.streak;",
        ["running_out_of_guesses_loses_and_breaks_the_streak"],
    ),
    (
        "the best streak follows the current one down as well as up",
        "            if self.streak > self.best_streak {\n"
        "                self.best_streak = self.streak;\n"
        "            }\n",
        "            self.best_streak = self.streak;\n",
        ["the_best_streak_is_the_longest_run_reached_not_the_current_one"],
    ),
    (
        "the game is lost one guess early",
        "        } else if self.guesses.len() >= MAX_GUESSES {",
        "        } else if self.guesses.len() >= MAX_GUESSES - 1 {",
        ["every_difficulty_allows_the_same_six_guesses"],
    ),
    (
        "a new word throws the totals away",
        "    fn new_game(&mut self) {\n",
        "    fn new_game(&mut self) {\n"
        "        self.games_played = 0;\n"
        "        self.games_won = 0;\n"
        "        self.streak = 0;\n"
        "        self.best_streak = 0;\n",
        [
            "a_new_word_clears_the_board_and_keeps_the_totals",
            "clicking_new_word_deals_one_and_keeps_the_totals",
        ],
    ),
    (
        # A button that deals a new puzzle when clicked twice is a button that
        # throws the game away on a slip of the mouse.
        "picking the length already in play deals a fresh word",
        "        if diff == self.difficulty {\n            return false;\n        }\n",
        "",
        [
            "picking_the_length_already_in_play_leaves_the_word_alone",
            "clicking_the_length_already_in_play_changes_nothing_and_says_so",
        ],
    ),
    (
        "changing the length does not deal a word of the new length",
        "        self.difficulty = diff;\n        self.new_game();\n        true",
        "        self.difficulty = diff;\n        true",
        ["changing_the_length_deals_a_word_of_that_length"],
    ),
    # -- The keys -------------------------------------------------------
    (
        # The old handler dropped `pressed`, so every letter was typed twice
        # per press and every Enter submitted the guess twice.
        "the key coming back up is acted on as well as the key going down",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }\n",
        "",
        [
            "a_key_coming_back_up_types_nothing",
            "a_word_typed_with_presses_and_releases_arrives_once",
            "closing_the_window_exits_and_nothing_else_does",
        ],
    ),
    (
        "a shifted or control-held key is taken as a letter",
        "        if ev.modifiers != Modifiers::NONE {\n"
        "            return EventResult::Ignored;\n"
        "        }\n",
        "",
        ["a_modified_key_is_left_for_whatever_binds_it"],
    ),
    (
        "one letter key types the wrong letter",
        "            Key::C => 'c',",
        "            Key::C => 'x',",
        ["every_letter_key_types_its_own_letter"],
    ),
    (
        # H is a letter of the alphabet before it is a shortcut.
        "H is the hard-mode switch even in the middle of a word",
        "            Key::H if self.phase == GamePhase::Playing && self.current_input.is_empty() => {",
        "            Key::H => {",
        ["h_reaches_the_hard_mode_switch_only_where_no_letter_could_go"],
    ),
    (
        "N deals a new word even in the middle of a game",
        "            Key::N | Key::Escape if self.phase != GamePhase::Playing => {",
        "            Key::N | Key::Escape => {",
        ["n_and_escape_deal_a_new_word_only_once_the_game_is_over"],
    ),
    (
        # `None => false` would be no mutation at all: `acted == false` already
        # returns `Ignored` two lines below.  Swallowing the key means claiming
        # it, which is the fault worth breaking for.
        "a key the game has no use for is swallowed",
        "                None => return EventResult::Ignored,",
        "                None => true,",
        ["a_key_the_game_has_no_use_for_is_left_alone"],
    ),
    (
        # A frozen game that reports every keystroke as handled looks like a
        # working one.
        "a keystroke that changed nothing is reported as handled",
        "        if acted {\n"
        "            EventResult::Consumed\n"
        "        } else {\n"
        "            EventResult::Ignored\n"
        "        }\n"
        "    }\n\n"
        "    pub fn handle_mouse",
        "        let _ = acted;\n"
        "        EventResult::Consumed\n"
        "    }\n\n"
        "    pub fn handle_mouse",
        ["a_finished_game_reports_the_keys_it_ignores_as_ignored"],
    ),
    (
        "a row already full takes a further letter",
        "        if self.phase != GamePhase::Playing || self.current_input.len() >= self.target_len {",
        "        if self.phase != GamePhase::Playing {",
        ["a_row_already_full_takes_no_further_letters"],
    ),
    (
        "a letter is stored in whatever case it arrived in",
        "        self.current_input.push(ch.to_ascii_lowercase());",
        "        self.current_input.push(ch);",
        ["every_letter_drawn_on_the_keyboard_is_clickable_and_types_itself"],
    ),
    # -- The mouse ------------------------------------------------------
    (
        "any mouse button plays",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {\n"
        "            return EventResult::Ignored;\n"
        "        }\n",
        "",
        ["only_the_left_button_plays"],
    ),
    (
        # The old hit test used the literal `420.0` whatever the window was.
        "a click is read against the size the window opens at, not the size it was drawn at",
        "        let (w, h) = self.size_drawn;",
        "        let (w, h) = (WINDOW_WIDTH, WINDOW_HEIGHT);",
        ["a_click_is_read_against_the_size_the_frame_was_drawn_at"],
    ),
    (
        "a click that changed nothing is reported as handled",
        "                if self.activate(target) {\n"
        "                    EventResult::Consumed\n"
        "                } else {\n"
        "                    EventResult::Ignored\n"
        "                }\n",
        "                let _ = self.activate(target);\n"
        "                EventResult::Consumed\n",
        [
            "a_finished_game_reports_the_clicks_it_ignores_as_ignored",
            "clicking_the_length_already_in_play_changes_nothing_and_says_so",
        ],
    ),
    (
        "a click that landed on nothing is reported as handled",
        "            None => EventResult::Ignored,\n        }\n    }",
        "            None => EventResult::Consumed,\n        }\n    }",
        ["a_click_where_nothing_is_drawn_does_nothing"],
    ),
    # -- The hit boxes --------------------------------------------------
    (
        "the letters of the on-screen keyboard are not clickable",
        "                f.hit(Target::Key(ch), r);\n",
        "",
        [
            "every_letter_drawn_on_the_keyboard_is_clickable_and_types_itself",
            "a_key_is_clickable_where_its_ink_is_and_nowhere_else",
        ],
    ),
    (
        "enter and backspace are not clickable",
        "            f.hit(target, r);\n        }\n    }\n\n"
        "    /// What the keyboard has learnt about `ch`",
        "        }\n    }\n\n    /// What the keyboard has learnt about `ch`",
        ["a_key_is_clickable_where_its_ink_is_and_nowhere_else"],
    ),
    (
        "the header buttons are not clickable",
        "            f.hit(*target, r);\n",
        "",
        ["each_length_button_deals_a_word_of_the_length_it_names"],
    ),
    (
        # The board is a picture, not a control.
        "the tiles of the guess grid are clickable",
        "                fill(f, r, state.color(), CornerRadii::all(4.0));\n"
        "                // An answered tile is a block of colour;",
        "                fill(f, r, state.color(), CornerRadii::all(4.0));\n"
        "                f.hit(Target::Enter, r);\n"
        "                // An answered tile is a block of colour;",
        ["the_guess_grid_takes_no_clicks"],
    ),
    # -- What the picture says ------------------------------------------
    (
        "the tiles are all drawn in one colour",
        "                fill(f, r, state.color(), CornerRadii::all(4.0));\n"
        "                // An answered tile is a block of colour;",
        "                fill(f, r, SURFACE0, CornerRadii::all(4.0));\n"
        "                // An answered tile is a block of colour;",
        ["an_answered_guess_stays_on_the_board_in_the_colours_it_was_given"],
    ),
    (
        "the keys are all drawn in one colour",
        "                fill(f, r, state.color(), CornerRadii::all(4.0));\n"
        "                let mut buf = [0u8; 4];",
        "                fill(f, r, SURFACE1, CornerRadii::all(4.0));\n"
        "                let mut buf = [0u8; 4];",
        ["the_keyboard_is_drawn_in_what_it_has_learnt"],
    ),
    (
        "the row being typed is not drawn",
        "        if row == self.guesses.len()\n"
        "            && let Some(ch) = self.current_input.get(col)\n"
        "        {\n"
        "            return (*ch, TileState::Filled);\n"
        "        }\n",
        "",
        ["the_grid_shows_the_row_as_it_is_typed"],
    ),
    (
        "the panel over a finished game is drawn over a running one too",
        "            GamePhase::Playing => return,",
        "            GamePhase::Playing => (\"You won!\", GREEN),",
        ["the_over_panel_is_drawn_only_once_the_game_is_finished"],
    ),
    (
        # The one thing a player who has run out of guesses wants.
        "a lost game keeps the word to itself",
        '            _ => format!("The word was {}", self.target_word().to_uppercase()),',
        '            _ => "Better luck next time".to_string(),',
        ["a_lost_game_says_what_the_word_was"],
    ),
    (
        "a single guess is reported in the plural",
        '                if self.guesses.len() == 1 { "" } else { "es" }',
        '                if self.guesses.len() == 1 { "es" } else { "es" }',
        ["a_won_game_counts_the_guesses_it_took"],
    ),
    (
        "the message line is drawn whether or not there is a message",
        "        let Some(msg) = self.message else {\n            return;\n        };\n",
        '        let msg = self.message.unwrap_or("Not in word list");\n',
        ["the_message_line_is_drawn_only_when_there_is_something_to_say"],
    ),
    (
        "the counter reports numbers of its own rather than the totals",
        '            "Played {}  Won {}  Streak {}  Best {}",\n'
        "            self.games_played, self.games_won, self.streak, self.best_streak",
        '            "Played {}  Won {}  Streak {}  Best {}",\n'
        "            0, 0, 0, 0",
        ["the_counter_reads_the_totals"],
    ),
    (
        # Two strings on a line with no limit between them is one string
        # printed over the other.
        "the hint is drawn with no limit, so it runs under the counter",
        "            y,\n            Some(hint_room),",
        "            y,\n            None,",
        [
            "the_hint_stops_before_the_counter_starts",
            "every_string_the_game_draws_is_told_where_to_stop",
        ],
    ),
    (
        # A right-aligned string long enough to overrun ends up at a negative
        # x, off the edge of the screen.
        "the counter is placed from its measured width without regard to the band",
        "        let stats_w = text::measure(&stats, l.small, weight).min(room);",
        "        let stats_w = text::measure(&stats, l.small, weight);",
        ["the_hint_stops_before_the_counter_starts"],
    ),
    (
        "the difficulty button does not say the length it deals",
        '                    format!("{} ({})", diff.name(), diff.word_len()),',
        "                    diff.name().to_string(),",
        ["each_length_button_deals_a_word_of_the_length_it_names"],
    ),
    (
        "every difficulty button is lit, so none of them says which is in play",
        "                    diff == self.difficulty,",
        "                    true,",
        ["the_length_in_play_is_the_button_that_is_lit"],
    ),
    (
        # The greying is the only warning a player gets before clicking a
        # switch that will not turn.
        "the hard-mode switch looks live after it has stopped turning",
        "            let live = *target != Target::HardMode || self.guesses.is_empty();",
        "            let live = true;",
        ["the_hard_mode_switch_is_greyed_once_it_can_no_longer_turn"],
    ),
    (
        "a string centred in its box is not stopped at the edge of it",
        "        r.y + (r.h - lh) / 2.0,\n        Some(r.w),",
        "        r.y + (r.h - lh) / 2.0,\n        None,",
        ["every_string_the_game_draws_is_told_where_to_stop"],
    ),
    (
        "a string wider than its box is centred off the left edge of it",
        "    let w = text::measure(l.text, l.size, l.weight).min(r.w);",
        "    let w = text::measure(l.text, l.size, l.weight);",
        ["nothing_is_drawn_outside_the_window"],
    ),
    # -- The window -----------------------------------------------------
    (
        # If these drift apart, every layout test checks a window the program
        # never actually opens.
        "the window opens at a size the probe does not draw at",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (800, 600)",
        ["the_probe_draws_at_the_size_the_window_opens_at"],
    ),
    (
        "rendering does not record the size it drew at",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["rendering_records_the_size_it_drew_at"],
    ),
    (
        "a resize is ignored",
        "        Event::Resize { width, height } => {\n"
        "            game.resize(*width as f32, *height as f32);\n"
        "            EventResult::Consumed\n"
        "        }\n",
        "",
        ["a_resize_moves_the_layout_the_next_click_is_read_against"],
    ),
    (
        "a window squashed to nothing is taken at its word",
        "        self.size_drawn = (width.max(1.0), height.max(1.0));",
        "        self.size_drawn = (width, height);",
        ["a_window_squashed_to_nothing_still_lays_out"],
    ),
    (
        "the close request is not answered",
        "        if matches!(event, Event::CloseRequested) {\n"
        "            return Response::Exit;\n"
        "        }\n",
        "",
        ["closing_the_window_exits_and_nothing_else_does"],
    ),
    (
        "the window is named something other than what it is",
        '        "wordle".to_string()\n    }\n\n    fn initial_size',
        '        "sokoban".to_string()\n    }\n\n    fn initial_size',
        ["the_window_names_itself"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "wordle", timeout=240))

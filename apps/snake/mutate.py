"""Mutation test for the snake suite.

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
    # -- Which way is which --------------------------------------------
    (
        "up is down",
        "            Self::Up => (-1, 0),",
        "            Self::Up => (1, 0),",
        ["every_direction_moves_the_snake_its_own_way"],
    ),
    (
        "down is up",
        "            Self::Down => (1, 0),",
        "            Self::Down => (-1, 0),",
        ["every_direction_moves_the_snake_its_own_way"],
    ),
    (
        "left is right",
        "            Self::Left => (0, -1),",
        "            Self::Left => (0, 1),",
        ["every_direction_moves_the_snake_its_own_way"],
    ),
    (
        "right is left",
        "            Self::Right => (0, 1),",
        "            Self::Right => (0, -1),",
        ["every_direction_moves_the_snake_its_own_way"],
    ),
    (
        "a step along the rows is no step at all",
        "            Self::Up => (-1, 0),\n            Self::Down => (1, 0),",
        "            Self::Up => (0, 0),\n            Self::Down => (0, 0),",
        ["every_direction_moves_the_snake_its_own_way"],
    ),
    (
        "the reverse of a direction is the direction",
        "        dr == odr.saturating_neg() && dc == odc.saturating_neg()",
        "        dr == odr && dc == odc",
        ["the_snake_may_not_turn_back_on_itself"],
    ),
    (
        "nothing is the reverse of anything",
        "        dr == odr.saturating_neg() && dc == odc.saturating_neg()",
        "        false",
        ["the_snake_may_not_turn_back_on_itself"],
    ),
    (
        "the arrows are not bound",
        "            Key::Up | Key::W => Some(Self::Up),\n"
        "            Key::Down | Key::S => Some(Self::Down),\n"
        "            Key::Left | Key::A => Some(Self::Left),\n"
        "            Key::Right | Key::D => Some(Self::Right),",
        "            Key::W => Some(Self::Up),\n"
        "            Key::S => Some(Self::Down),\n"
        "            Key::A => Some(Self::Left),\n"
        "            Key::D => Some(Self::Right),",
        ["the_arrows_steer_the_snake"],
    ),
    (
        "WASD is not bound",
        "            Key::Up | Key::W => Some(Self::Up),\n"
        "            Key::Down | Key::S => Some(Self::Down),\n"
        "            Key::Left | Key::A => Some(Self::Left),\n"
        "            Key::Right | Key::D => Some(Self::Right),",
        "            Key::Up => Some(Self::Up),\n"
        "            Key::Down => Some(Self::Down),\n"
        "            Key::Left => Some(Self::Left),\n"
        "            Key::Right => Some(Self::Right),",
        ["wasd_steers_the_snake_as_the_arrows_do"],
    ),
    (
        "W asks for the wrong way",
        "            Key::Up | Key::W => Some(Self::Up),\n"
        "            Key::Down | Key::S => Some(Self::Down),",
        "            Key::Up => Some(Self::Up),\n"
        "            Key::Down | Key::S | Key::W => Some(Self::Down),",
        ["wasd_steers_the_snake_as_the_arrows_do"],
    ),
    (
        "a key that is not a direction is taken as one",
        "            _ => None,\n        }\n    }\n}\n\n// ── Grid position",
        "            _ => Some(Self::Up),\n        }\n    }\n}\n\n// ── Grid position",
        ["a_key_that_changes_nothing_is_left_for_whoever_wants_it"],
    ),
    # -- Where a square is ---------------------------------------------
    (
        "a move does not move the row",
        "            row: self.row.saturating_add(dr),",
        "            row: self.row,",
        ["every_direction_moves_the_snake_its_own_way"],
    ),
    (
        "a move does not move the column",
        "            col: self.col.saturating_add(dc),",
        "            col: self.col,",
        ["a_move_takes_the_head_one_square_and_brings_the_tail_along"],
    ),
    (
        "a move takes two squares",
        "            col: self.col.saturating_add(dc),",
        "            col: self.col.saturating_add(dc * 2),",
        ["a_move_takes_the_head_one_square_and_brings_the_tail_along"],
    ),
    (
        "a wrap off the top does not come out at the bottom",
        "            row: wrap_index(self.row, GRID_ROWS),",
        "            row: self.row,",
        ["wrapping_works_off_every_edge"],
    ),
    (
        "a wrap off the side does not come out the other side",
        "            col: wrap_index(self.col, GRID_COLS),",
        "            col: self.col,",
        ["a_wall_is_a_doorway_in_wrap_mode"],
    ),
    (
        "a negative index is left negative, as Rust's % leaves it",
        "    if m < 0 { m.saturating_add(len) } else { m }",
        "    m",
        ["wrapping_works_off_every_edge"],
    ),
    (
        "a wrap adds the length whether it was needed or not",
        "    if m < 0 { m.saturating_add(len) } else { m }",
        "    m.saturating_add(len)",
        ["wrapping_works_off_every_edge"],
    ),
    (
        "the rows are not walls",
        "        self.row >= 0 && self.row < rows && self.col >= 0 && self.col < cols",
        "        self.col >= 0 && self.col < cols",
        ["running_into_a_wall_ends_the_game"],
    ),
    (
        "the columns are not walls",
        "        self.row >= 0 && self.row < rows && self.col >= 0 && self.col < cols",
        "        self.row >= 0 && self.row < rows",
        ["running_into_a_wall_ends_the_game"],
    ),
    (
        "the far wall is one square further out than the board",
        "        self.row >= 0 && self.row < rows && self.col >= 0 && self.col < cols",
        "        self.row >= 0 && self.row <= rows && self.col >= 0 && self.col <= cols",
        ["running_into_a_wall_ends_the_game"],
    ),
    # -- The snake's move ----------------------------------------------
    (
        "the tail never moves, so the snake grows on every move",
        "            self.snake.pop();",
        "            let _ = &mut self.snake;",
        ["a_move_takes_the_head_one_square_and_brings_the_tail_along"],
    ),
    (
        "the head is added at the back rather than the front",
        "        self.snake.insert(0, new_head);",
        "        self.snake.push(new_head);",
        ["a_move_takes_the_head_one_square_and_brings_the_tail_along"],
    ),
    (
        "the snake may run through itself",
        "        if self.snake.contains(&new_head) {\n"
        "            self.game_over();\n"
        "            return;\n"
        "        }\n",
        "",
        ["running_into_itself_ends_the_game"],
    ),
    (
        "the tail is not part of the snake to run into",
        "        if self.snake.contains(&new_head) {",
        "        if self.snake.iter().rev().skip(1).rev().any(|p| *p == new_head) {",
        ["the_last_square_of_the_snake_is_as_solid_as_the_rest"],
    ),
    (
        "the walls are doorways whether the switch is on or not",
        "        if self.wrap_mode {\n            new_head = new_head.wrapped();\n"
        "        } else if !new_head.in_bounds() {",
        "        if true {\n            new_head = new_head.wrapped();\n"
        "        } else if !new_head.in_bounds() {",
        ["running_into_a_wall_ends_the_game"],
    ),
    (
        "the walls are walls whether the switch is on or not",
        "        if self.wrap_mode {",
        "        if false {",
        ["a_wall_is_a_doorway_in_wrap_mode", "wrapping_works_off_every_edge"],
    ),
    # -- Eating ---------------------------------------------------------
    (
        "landing on the food is not eating it",
        "        let ate_normal = new_head == self.food.pos;",
        "        let ate_normal = false;",
        ["eating_lengthens_the_snake_and_scores"],
    ),
    (
        "landing on the bonus is not eating it",
        "        let ate_bonus = self.bonus_food.is_some_and(|b| new_head == b.pos);",
        "        let ate_bonus = false;",
        ["a_bonus_is_worth_more_than_a_meal"],
    ),
    (
        "a bonus is eaten wherever the snake goes",
        "        let ate_bonus = self.bonus_food.is_some_and(|b| new_head == b.pos);",
        "        let ate_bonus = self.bonus_food.is_some();",
        ["a_bonus_food_disappears_when_its_time_is_up"],
    ),
    (
        "a meal scores nothing",
        "            .saturating_add(NORMAL_FOOD_POINTS.saturating_mul(self.multiplier()));",
        "            .saturating_add(0);",
        ["eating_lengthens_the_snake_and_scores"],
    ),
    (
        "a bonus scores what a meal scores",
        "            .saturating_add(BONUS_FOOD_POINTS.saturating_mul(self.multiplier()));",
        "            .saturating_add(NORMAL_FOOD_POINTS.saturating_mul(self.multiplier()));",
        ["a_bonus_is_worth_more_than_a_meal"],
    ),
    (
        "a meal is not counted",
        "        self.foods_eaten = self.foods_eaten.saturating_add(1);\n\n        self.spawn_food();",
        "        self.spawn_food();",
        ["eating_lengthens_the_snake_and_scores"],
    ),
    (
        "a bonus is not counted",
        "        self.bonus_eaten = self.bonus_eaten.saturating_add(1);\n",
        "",
        ["a_bonus_is_worth_more_than_a_meal"],
    ),
    (
        "a bonus is not counted as a meal as well",
        "        self.bonus_eaten = self.bonus_eaten.saturating_add(1);\n"
        "        self.foods_eaten = self.foods_eaten.saturating_add(1);\n",
        "        self.bonus_eaten = self.bonus_eaten.saturating_add(1);\n",
        ["a_bonus_is_worth_more_than_a_meal"],
    ),
    (
        "the bonus that was eaten stays on the board",
        "        self.bonus_food = None;\n    }\n\n    /// Count this meal towards the streak",
        "    }\n\n    /// Count this meal towards the streak",
        ["a_bonus_is_worth_more_than_a_meal"],
    ),
    (
        "the food that was eaten is not replaced",
        "        self.spawn_food();\n\n        if self.bonus_food.is_none()",
        "        if self.bonus_food.is_none()",
        ["eating_lengthens_the_snake_and_scores"],
    ),
    # -- Where the food goes -------------------------------------------
    (
        "the food may be put under the snake",
        "                if !self.snake.contains(&pos) {\n                    free.push(pos);\n                }",
        "                free.push(pos);",
        ["the_food_is_never_put_under_the_snake"],
    ),
    (
        "a full board is not a win",
        "        let Some(pos) = self.pick(&free) else {\n            self.win();\n            return;\n        };",
        "        let Some(pos) = self.pick(&free) else {\n            return;\n        };",
        ["filling_the_board_is_a_win_and_not_a_hang"],
    ),
    (
        "the bonus may be put on top of the meal",
        "            .filter(|p| *p != food_pos)\n",
        "",
        ["a_bonus_never_appears_on_top_of_the_food"],
    ),
    (
        "the bonus may be put under the snake",
        "        let free: Vec<Pos> = self\n"
        "            .free_cells()\n"
        "            .into_iter()\n"
        "            .filter(|p| *p != food_pos)\n"
        "            .collect();",
        "        let mut free: Vec<Pos> = Vec::new();\n"
        "        for row in 0..GRID_ROWS {\n"
        "            for col in 0..GRID_COLS {\n"
        "                let p = Pos::new(\n"
        "                    i32::try_from(row).unwrap_or(0),\n"
        "                    i32::try_from(col).unwrap_or(0),\n"
        "                );\n"
        "                if p != food_pos {\n"
        "                    free.push(p);\n"
        "                }\n"
        "            }\n"
        "        }",
        ["a_bonus_never_appears_under_the_snake"],
    ),
    (
        "a bonus has no lifetime, so it never expires",
        "                ticks_remaining: BONUS_FOOD_LIFETIME,",
        "                ticks_remaining: u32::MAX,",
        ["a_spawned_bonus_is_given_the_lifetime_it_is_meant_to_have"],
    ),
    (
        "the bonus clock does not run down",
        "            bonus.ticks_remaining = bonus.ticks_remaining.saturating_sub(1);\n",
        "",
        ["a_bonus_food_disappears_when_its_time_is_up"],
    ),
    (
        "a bonus is taken away a move early",
        "            if bonus.ticks_remaining == 0 {",
        "            if bonus.ticks_remaining <= 1 {",
        ["a_bonus_food_disappears_when_its_time_is_up"],
    ),
    (
        "the food is always put in the same square",
        "        cells.get(self.rng.below(cells.len())).copied()",
        "        cells.first().copied()",
        ["a_restart_deals_a_different_board"],
    ),
    (
        "the food is put in the last free square rather than a random one",
        "        cells.get(self.rng.below(cells.len())).copied()",
        "        cells.last().copied()",
        ["a_restart_deals_a_different_board"],
    ),
]

MUTATIONS += [
    # -- The clock ------------------------------------------------------
    (
        "the clock runs whatever state the game is in",
        "        if self.state != GameState::Playing {\n"
        "            return EventResult::Ignored;\n"
        "        }\n"
        "        let interval = u64::from(self.current_interval_ms()).max(1);",
        "        let interval = u64::from(self.current_interval_ms()).max(1);",
        ["the_clock_runs_only_while_the_game_is_being_played"],
    ),
    (
        "time short of a move is thrown away rather than banked",
        "        self.accumulated_ms = self.accumulated_ms.saturating_add(elapsed_ms);",
        "        self.accumulated_ms = elapsed_ms;",
        ["time_short_of_a_move_is_banked_rather_than_lost"],
    ),
    (
        "a stall is played out in one frame, as it used to be",
        "            && moves < MAX_CATCH_UP_MOVES\n",
        "",
        ["a_stall_moves_the_snake_at_most_twice"],
    ),
    (
        "the catch-up ceiling is one move higher than it says",
        "            && moves < MAX_CATCH_UP_MOVES",
        "            && moves <= MAX_CATCH_UP_MOVES",
        ["a_stall_moves_the_snake_at_most_twice"],
    ),
    (
        "a move costs no time, so one interval moves the snake for ever",
        "            self.accumulated_ms = self.accumulated_ms.saturating_sub(interval);",
        "            let _ = interval;",
        ["a_tick_from_the_window_moves_the_snake"],
    ),
    (
        "the moves the game could not catch up on are carried, not dropped",
        "        self.accumulated_ms = self.accumulated_ms.checked_rem(interval).unwrap_or(0);\n",
        "",
        ["what_could_not_be_caught_up_on_is_dropped_rather_than_carried"],
    ),
    (
        "a game over part way through a catch-up does not stop it",
        "            && moves < MAX_CATCH_UP_MOVES\n"
        "            && self.state == GameState::Playing\n",
        "            && moves < MAX_CATCH_UP_MOVES\n",
        ["a_game_that_ends_part_way_through_a_catch_up_stops_there"],
    ),
    (
        "a frame that moved nothing says it changed something",
        "        if moves == 0 {\n"
        "            EventResult::Ignored\n"
        "        } else {\n"
        "            EventResult::Consumed\n"
        "        }",
        "        EventResult::Consumed",
        ["time_short_of_a_move_is_banked_rather_than_lost"],
    ),
    (
        "a frame that moved the snake says it changed nothing",
        "        if moves == 0 {\n"
        "            EventResult::Ignored\n"
        "        } else {\n"
        "            EventResult::Consumed\n"
        "        }",
        "        EventResult::Ignored",
        ["time_short_of_a_move_is_banked_rather_than_lost"],
    ),
    (
        "the moves are counted by setting rather than adding",
        "        self.total_ticks = self.total_ticks.saturating_add(1);",
        "        self.total_ticks = 1;",
        ["the_clock_advances_the_count_rather_than_setting_it"],
    ),
    (
        "the pulse beats once per frame rather than once per move",
        "        self.pulse_counter = self.pulse_counter.wrapping_add(1);\n",
        "",
        ["the_bonus_pulse_follows_the_moves_and_not_the_frames"],
    ),
    (
        "a queued turn is never taken",
        "        if !self.dir_queue.is_empty() {\n"
        "            self.direction = self.dir_queue.remove(0);\n"
        "        }\n",
        "",
        ["a_turn_reaches_the_snake_on_its_next_move"],
    ),
    (
        "the turns are taken newest first",
        "            self.direction = self.dir_queue.remove(0);",
        "            self.direction = self.dir_queue.remove(self.dir_queue.len() - 1);",
        ["two_quick_turns_both_land"],
    ),
    (
        "a turn is taken and left in the queue",
        "            self.direction = self.dir_queue.remove(0);",
        "            self.direction = self.dir_queue[0];",
        ["a_turn_reaches_the_snake_on_its_next_move"],
    ),
    # -- The streak and the score --------------------------------------
    (
        "the moves since the last meal are not counted",
        "        self.ticks_since_food = self.ticks_since_food.saturating_add(1);\n",
        "",
        ["dawdling_between_meals_lets_the_streak_lapse"],
    ),
    (
        "a streak never lapses",
        "        self.ticks_since_food <= STREAK_WINDOW_TICKS",
        "        true",
        [
            "a_meal_eaten_late_starts_the_streak_again",
            "a_bonus_eaten_after_dawdling_does_not_revive_a_lapsed_streak",
        ],
    ),
    (
        "a streak lapses between one meal and the next",
        "        self.ticks_since_food <= STREAK_WINDOW_TICKS",
        "        false",
        ["a_meal_eaten_promptly_carries_the_streak_on"],
    ),
    (
        "the streak window is one move shorter than it says",
        "        self.ticks_since_food <= STREAK_WINDOW_TICKS",
        "        self.ticks_since_food < STREAK_WINDOW_TICKS",
        ["a_meal_on_the_last_move_of_the_window_carries_the_streak_on"],
    ),
    (
        "a lapsed streak starts again from nought rather than from this meal",
        "        self.streak = if self.streak_is_alive() {\n"
        "            self.streak.saturating_add(1)\n"
        "        } else {\n"
        "            1\n"
        "        };",
        "        self.streak = if self.streak_is_alive() {\n"
        "            self.streak.saturating_add(1)\n"
        "        } else {\n"
        "            0\n"
        "        };",
        ["a_meal_eaten_late_starts_the_streak_again"],
    ),
    (
        "the clock since the last meal is not reset by a meal",
        "        self.ticks_since_food = 0;\n    }\n\n    fn game_over",
        "    }\n\n    fn game_over",
        ["a_meal_eaten_promptly_carries_the_streak_on"],
    ),
    (
        "a bonus does not count towards the streak at all",
        "        self.extend_streak();\n"
        "        self.score = self\n"
        "            .score\n"
        "            .saturating_add(BONUS_FOOD_POINTS.saturating_mul(self.multiplier()));",
        "        self.score = self\n"
        "            .score\n"
        "            .saturating_add(BONUS_FOOD_POINTS.saturating_mul(self.multiplier()));",
        ["a_bonus_eaten_after_dawdling_does_not_revive_a_lapsed_streak"],
    ),
    (
        "a bonus revives a lapsed streak, as it used to",
        "    fn eat_bonus_food(&mut self) {\n",
        "    fn eat_bonus_food(&mut self) {\n"
        "        self.streak = self.streak.saturating_add(1);\n"
        "        self.ticks_since_food = 0;\n"
        "        if true {\n"
        "            self.score = self.score.saturating_add(BONUS_FOOD_POINTS);\n"
        "            self.bonus_eaten = self.bonus_eaten.saturating_add(1);\n"
        "            self.foods_eaten = self.foods_eaten.saturating_add(1);\n"
        "            self.bonus_food = None;\n"
        "            return;\n"
        "        }\n",
        ["a_bonus_eaten_after_dawdling_does_not_revive_a_lapsed_streak"],
    ),
    (
        "a streak is worth nothing",
        "        if self.streak >= STREAK_THRESHOLD {\n"
        "            STREAK_MULTIPLIER\n"
        "        } else {\n"
        "            1\n"
        "        }",
        "        1",
        ["a_streak_doubles_what_a_meal_is_worth"],
    ),
    (
        "every meal is worth double",
        "        if self.streak >= STREAK_THRESHOLD {\n"
        "            STREAK_MULTIPLIER\n"
        "        } else {\n"
        "            1\n"
        "        }",
        "        STREAK_MULTIPLIER",
        ["a_meal_short_of_the_threshold_is_worth_its_face_value"],
    ),
    (
        "the streak pays a meal early",
        "        if self.streak >= STREAK_THRESHOLD {",
        "        if self.streak >= STREAK_THRESHOLD - 1 {",
        ["a_meal_short_of_the_threshold_is_worth_its_face_value"],
    ),
    (
        "the streak pays a meal late",
        "        if self.streak >= STREAK_THRESHOLD {",
        "        if self.streak > STREAK_THRESHOLD {",
        ["a_streak_doubles_what_a_meal_is_worth"],
    ),
    # -- Speed ----------------------------------------------------------
    (
        "the score does not buy speed",
        "    (score / 50).saturating_add(1).min(MAX_SPEED_LEVEL)",
        "    1",
        ["speed_climbs_a_level_every_fifty_points_and_then_stops"],
    ),
    (
        "speed climbs without a ceiling",
        "    (score / 50).saturating_add(1).min(MAX_SPEED_LEVEL)",
        "    (score / 50).saturating_add(1)",
        ["speed_climbs_a_level_every_fifty_points_and_then_stops"],
    ),
    (
        "the first level is level nought",
        "    (score / 50).saturating_add(1).min(MAX_SPEED_LEVEL)",
        "    (score / 50).min(MAX_SPEED_LEVEL)",
        ["speed_climbs_a_level_every_fifty_points_and_then_stops"],
    ),
    (
        "a level costs a hundred points rather than fifty",
        "    (score / 50).saturating_add(1).min(MAX_SPEED_LEVEL)",
        "    (score / 100).saturating_add(1).min(MAX_SPEED_LEVEL)",
        ["speed_climbs_a_level_every_fifty_points_and_then_stops"],
    ),
    (
        "the first level is already a level's worth of speed faster",
        "    let reduction = level.saturating_sub(1).saturating_mul(base / 10);",
        "    let reduction = level.saturating_mul(base / 10);",
        ["level_one_moves_at_the_difficultys_own_pace"],
    ),
    (
        "levelling up does not speed the snake",
        "    let reduction = level.saturating_sub(1).saturating_mul(base / 10);",
        "    let reduction = 0;",
        ["a_higher_level_moves_the_snake_sooner_but_never_below_the_floor"],
    ),
    (
        "there is no floor under the interval",
        "    base.saturating_sub(reduction).max(MIN_INTERVAL_MS)",
        "    base.saturating_sub(reduction)",
        ["a_higher_level_moves_the_snake_sooner_but_never_below_the_floor"],
    ),
    (
        "every difficulty moves at the same pace",
        "            Self::Easy => 200,\n            Self::Medium => 150,\n            Self::Hard => 100,",
        "            Self::Easy => 150,\n            Self::Medium => 150,\n            Self::Hard => 150,",
        ["a_harder_difficulty_moves_the_snake_sooner"],
    ),
    (
        "Hard is the slowest and Easy the fastest",
        "            Self::Easy => 200,\n            Self::Medium => 150,\n            Self::Hard => 100,",
        "            Self::Easy => 100,\n            Self::Medium => 150,\n            Self::Hard => 200,",
        ["a_harder_difficulty_moves_the_snake_sooner"],
    ),
    (
        "the snake moves at whatever difficulty it started at",
        "        tick_interval_ms(self.difficulty, self.current_speed_level())",
        "        tick_interval_ms(Difficulty::Medium, self.current_speed_level())",
        ["the_chosen_difficulty_is_what_the_snake_moves_at"],
    ),
    (
        "the speed the score bought is not used",
        "        tick_interval_ms(self.difficulty, self.current_speed_level())",
        "        tick_interval_ms(self.difficulty, 1)",
        ["a_bigger_score_moves_the_snake_sooner"],
    ),
    # -- Ending and restarting -----------------------------------------
    (
        "a game that is over is not banked as the best one",
        "        self.state = GameState::GameOver;\n"
        "        self.high_score = self.high_score.max(self.score);",
        "        self.state = GameState::GameOver;",
        ["ending_the_game_banks_the_score_as_the_best_one"],
    ),
    (
        "the best score is whatever the last game scored",
        "        self.state = GameState::GameOver;\n"
        "        self.high_score = self.high_score.max(self.score);",
        "        self.state = GameState::GameOver;\n        self.high_score = self.score;",
        ["a_worse_game_does_not_lower_the_best_score"],
    ),
    (
        "a game that ended is still being played",
        "        self.state = GameState::GameOver;\n"
        "        self.high_score = self.high_score.max(self.score);",
        "        self.high_score = self.high_score.max(self.score);",
        ["running_into_a_wall_ends_the_game"],
    ),
    (
        "a win is not banked as the best score",
        "        self.state = GameState::Won;\n"
        "        self.high_score = self.high_score.max(self.score);",
        "        self.state = GameState::Won;",
        ["a_win_banks_the_score_as_the_best_one"],
    ),
    (
        "a restart forgets the best score",
        "        self.high_score = high;\n",
        "",
        ["the_high_score_survives_a_restart_and_the_score_does_not"],
    ),
    (
        "a restart forgets the difficulty",
        "        self.difficulty = difficulty;\n",
        "",
        ["a_restart_keeps_the_difficulty_and_the_wrap_switch"],
    ),
    (
        "a restart forgets the wrap switch",
        "        self.wrap_mode = wrap;\n",
        "",
        ["a_restart_keeps_the_difficulty_and_the_wrap_switch"],
    ),
    (
        "a restart forgets the size the window is",
        "        self.size = size;\n    }",
        "    }",
        ["a_resize_moves_the_switches_and_the_clicks_follow_them"],
    ),
    (
        "every restart deals the same board",
        "        let seed = self.rng.next_u64();",
        "        let seed = 7;",
        ["a_restart_deals_a_different_board"],
    ),
    (
        "the snake starts somewhere other than the middle",
        "                .push(Pos::new(rows / 2, (cols / 2).saturating_sub(i)));",
        "                .push(Pos::new(rows / 4, (cols / 2).saturating_sub(i)));",
        ["a_new_game_has_a_snake_of_three_in_the_middle_facing_right"],
    ),
    (
        "the snake starts laid out the way it is going",
        "                .push(Pos::new(rows / 2, (cols / 2).saturating_sub(i)));",
        "                .push(Pos::new(rows / 2, (cols / 2).saturating_add(i)));",
        ["a_new_game_has_a_snake_of_three_in_the_middle_facing_right"],
    ),
    (
        "the snake starts one square long",
        "        for i in 0..START_LENGTH {",
        "        for i in 0..1 {",
        ["a_new_game_has_a_snake_of_three_in_the_middle_facing_right"],
    ),
    (
        "a new game keeps whichever way the old snake was pointing",
        "            direction: Direction::Right,\n",
        "            direction: Direction::Up,\n",
        ["a_new_game_has_a_snake_of_three_in_the_middle_facing_right"],
    ),
]

MUTATIONS += [
    # -- The keyboard ---------------------------------------------------
    (
        "letting a key go counts as pressing it",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }\n",
        "",
        ["letting_a_key_go_is_not_pressing_it"],
    ),
    (
        "a shortcut is taken by the game",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n"
        "        }\n",
        "",
        ["a_shortcut_belongs_to_whoever_is_listening_for_shortcuts"],
    ),
    (
        "only Alt and Super belong to whoever is listening for shortcuts",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {",
        "        if ev.modifiers.alt || ev.modifiers.super_key {",
        ["a_shortcut_belongs_to_whoever_is_listening_for_shortcuts"],
    ),
    (
        "R does not start again",
        "            Key::R => {\n"
        "                self.restart();\n"
        "                return EventResult::Consumed;\n"
        "            }\n",
        "",
        ["r_starts_again_at_any_time"],
    ),
    (
        "B does not turn the walls into doorways",
        "            Key::B => {\n"
        "                self.wrap_mode = !self.wrap_mode;\n"
        "                return EventResult::Consumed;\n"
        "            }\n",
        "",
        ["b_turns_the_walls_into_doorways_and_back"],
    ),
    (
        "B turns the walls into doorways and leaves them that way",
        "                self.wrap_mode = !self.wrap_mode;\n"
        "                return EventResult::Consumed;\n"
        "            }\n"
        "            Key::Num1",
        "                self.wrap_mode = true;\n"
        "                return EventResult::Consumed;\n"
        "            }\n"
        "            Key::Num1",
        ["b_turns_the_walls_into_doorways_and_back"],
    ),
    (
        "the number keys are not bound",
        "            Key::Num1 | Key::Num2 | Key::Num3 => {",
        "            Key::F1 => {",
        ["a_difficulty_key_changes_the_speed_without_starting_a_new_game"],
    ),
    (
        "a number key picks the difficulty and starts a new game, as it used to",
        "                    self.difficulty = *d;\n                    return EventResult::Consumed;",
        "                    self.difficulty = *d;\n"
        "                    self.restart();\n"
        "                    return EventResult::Consumed;",
        ["a_difficulty_key_changes_the_speed_without_starting_a_new_game"],
    ),
    (
        "every number key picks the same difficulty",
        "                if let Some(d) = DIFFICULTIES.iter().find(|d| d.key() == ev.key) {",
        "                if let Some(d) = DIFFICULTIES.first() {",
        ["a_difficulty_key_changes_the_speed_without_starting_a_new_game"],
    ),
    (
        "the number keys are read in the wrong order",
        "            Self::Easy => Key::Num1,",
        "            Self::Easy => Key::Num3,",
        ["a_difficulty_key_changes_the_speed_without_starting_a_new_game"],
    ),
    (
        "R and B only work while the game is being played",
        "        match ev.key {\n            Key::R => {",
        "        if self.state != GameState::Playing {\n"
        "            return match self.state {\n"
        "                GameState::Paused => self.key_paused(ev.key),\n"
        "                _ => self.key_finished(ev.key),\n"
        "            };\n"
        "        }\n"
        "        match ev.key {\n            Key::R => {",
        ["a_difficulty_key_works_on_the_game_over_screen_too"],
    ),
    (
        "a paused game is still being steered",
        "            GameState::Paused => self.key_paused(ev.key),",
        "            GameState::Paused => self.key_playing(ev.key),",
        ["an_arrow_does_nothing_while_the_game_is_paused"],
    ),
    (
        "a finished game is still being steered",
        "            GameState::GameOver | GameState::Won => self.key_finished(ev.key),",
        "            GameState::GameOver | GameState::Won => self.key_playing(ev.key),",
        ["enter_starts_again_once_the_game_is_over"],
    ),
    (
        "P does not pause",
        "            Key::P | Key::Escape => {\n"
        "                self.state = GameState::Paused;\n"
        "                EventResult::Consumed\n"
        "            }",
        "            Key::Escape => {\n"
        "                self.state = GameState::Paused;\n"
        "                EventResult::Consumed\n"
        "            }",
        ["p_pauses_and_unpauses"],
    ),
    (
        "Escape does not pause",
        "            Key::P | Key::Escape => {\n"
        "                self.state = GameState::Paused;\n"
        "                EventResult::Consumed\n"
        "            }",
        "            Key::P => {\n"
        "                self.state = GameState::Paused;\n"
        "                EventResult::Consumed\n"
        "            }",
        ["escape_pauses_and_unpauses_too"],
    ),
    (
        "a pause cannot be left",
        "            Key::P | Key::Escape => {\n"
        "                self.state = GameState::Playing;\n"
        "                EventResult::Consumed\n"
        "            }",
        "            Key::P | Key::Escape => EventResult::Ignored,",
        ["p_pauses_and_unpauses", "escape_pauses_and_unpauses_too"],
    ),
    (
        "Enter does not start again",
        "            Key::Enter | Key::Space => {",
        "            Key::Space => {",
        ["enter_starts_again_once_the_game_is_over"],
    ),
    (
        "Space does not start again",
        "            Key::Enter | Key::Space => {",
        "            Key::Enter => {",
        ["space_starts_again_once_the_game_is_over"],
    ),
    (
        "Enter throws away a game that is still going",
        "            GameState::Playing => self.key_playing(ev.key),",
        "            GameState::Playing => self.key_finished(ev.key),",
        ["enter_does_nothing_while_the_game_is_still_being_played"],
    ),
    (
        "a key the game has no use for is swallowed",
        "            _ => EventResult::Ignored,\n"
        "        }\n"
        "    }\n"
        "\n"
        "    fn key_paused",
        "            _ => EventResult::Consumed,\n"
        "        }\n"
        "    }\n"
        "\n"
        "    fn key_paused",
        ["a_key_that_changes_nothing_is_left_for_whoever_wants_it"],
    ),
    (
        "the turn queue has no bottom",
        "        if self.dir_queue.len() >= MAX_DIR_QUEUE {\n"
        "            return EventResult::Ignored;\n"
        "        }\n",
        "",
        ["the_turn_queue_has_a_bottom"],
    ),
    (
        "the turn queue holds one turn",
        "        if self.dir_queue.len() >= MAX_DIR_QUEUE {",
        "        if self.dir_queue.len() >= 1 {",
        ["two_quick_turns_both_land"],
    ),
    (
        "a turn is measured against the direction rather than the last turn",
        "        let effective = self.dir_queue.last().copied().unwrap_or(self.direction);",
        "        let effective = self.direction;",
        ["two_quick_turns_both_land"],
    ),
    (
        "the way the snake is already going counts as a turn",
        "        if new_dir == effective || new_dir.is_opposite(effective) {",
        "        if new_dir.is_opposite(effective) {",
        ["the_way_the_snake_is_already_going_is_not_a_turn"],
    ),
    (
        "a reversal counts as a turn",
        "        if new_dir == effective || new_dir.is_opposite(effective) {",
        "        if new_dir == effective {",
        ["the_snake_may_not_turn_back_on_itself"],
    ),
    (
        "a turn is refused and reported as taken",
        "            return EventResult::Ignored;\n"
        "        }\n"
        "        self.dir_queue.push(new_dir);",
        "            return EventResult::Consumed;\n"
        "        }\n"
        "        self.dir_queue.push(new_dir);",
        ["the_snake_may_not_turn_back_on_itself"],
    ),
    # -- The pointer ----------------------------------------------------
    (
        "a right click is a click",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {\n"
        "            return EventResult::Ignored;\n"
        "        }\n",
        "",
        ["a_right_click_is_not_a_click"],
    ),
    (
        "a click on nothing is swallowed",
        "        let Some(target) = self.frame(w, h).hit_test(ev.x, ev.y) else {\n"
        "            return EventResult::Ignored;\n"
        "        };\n"
        "        self.activate(target)",
        "        match self.frame(w, h).hit_test(ev.x, ev.y) {\n"
        "            Some(target) => self.activate(target),\n"
        "            None => EventResult::Consumed,\n"
        "        }",
        ["a_click_on_nothing_is_left_for_whoever_wants_it"],
    ),
    (
        "a click is read against the size the window opened at",
        "        let (w, h) = self.size;\n        let Some(target) = self.frame(w, h)",
        "        let (w, h) = (WINDOW_WIDTH, WINDOW_HEIGHT);\n        let Some(target) = self.frame(w, h)",
        ["a_resize_moves_the_switches_and_the_clicks_follow_them"],
    ),
    (
        "the pause switch restarts",
        "            Target::Pause => self.toggle_pause(),",
        "            Target::Pause => {\n"
        "                self.restart();\n"
        "                EventResult::Consumed\n"
        "            }",
        ["the_pause_switch_pauses_and_then_offers_to_carry_on"],
    ),
    (
        "the restart switch does nothing",
        "            Target::Restart => {\n"
        "                self.restart();\n"
        "                EventResult::Consumed\n"
        "            }",
        "            Target::Restart => EventResult::Ignored,",
        ["the_restart_switch_starts_a_new_game"],
    ),
    (
        "the wrap switch does nothing",
        "            Target::Wrap => {\n"
        "                self.wrap_mode = !self.wrap_mode;\n"
        "                EventResult::Consumed\n"
        "            }",
        "            Target::Wrap => EventResult::Ignored,",
        ["the_wrap_switch_turns_the_walls_into_doorways"],
    ),
    (
        "every difficulty switch picks the same difficulty",
        "            Target::Level(level) => {\n                self.difficulty = level;",
        "            Target::Level(_) => {\n                self.difficulty = Difficulty::Easy;",
        ["each_difficulty_switch_picks_its_own_difficulty"],
    ),
    (
        "a square of the board does nothing when it is clicked",
        "            Target::Cell(row, col) => self.steer_towards(row, col),",
        "            Target::Cell(_, _) => EventResult::Ignored,",
        ["clicking_a_square_off_to_one_side_turns_the_snake_towards_it"],
    ),
    (
        "the pause switch works on a game that is over",
        "            GameState::GameOver | GameState::Won => EventResult::Ignored,\n"
        "        }\n"
        "    }",
        "            GameState::GameOver | GameState::Won => {\n"
        "                self.state = GameState::Paused;\n"
        "                EventResult::Consumed\n"
        "            }\n"
        "        }\n"
        "    }",
        ["the_pause_switch_is_refused_once_the_game_is_over"],
    ),
    (
        "a paused game cannot be carried on with by the switch",
        "            GameState::Paused => {\n"
        "                self.state = GameState::Playing;\n"
        "                EventResult::Consumed\n"
        "            }\n"
        "            GameState::GameOver | GameState::Won => EventResult::Ignored,",
        "            GameState::Paused | GameState::GameOver | GameState::Won => {\n"
        "                EventResult::Ignored\n"
        "            }",
        ["the_pause_switch_pauses_and_then_offers_to_carry_on"],
    ),
    (
        "a finished game can be steered",
        "        if self.state != GameState::Playing {\n"
        "            return EventResult::Ignored;\n"
        "        }\n"
        "        let Some(head) = self.snake.first().copied() else {",
        "        let Some(head) = self.snake.first().copied() else {",
        ["a_finished_game_cannot_be_steered"],
    ),
    (
        "a click above the head asks to go down",
        "            wanted.push(if dr < 0 {\n"
        "                Direction::Up\n"
        "            } else {\n"
        "                Direction::Down\n"
        "            });",
        "            wanted.push(if dr < 0 {\n"
        "                Direction::Down\n"
        "            } else {\n"
        "                Direction::Up\n"
        "            });",
        ["clicking_a_square_off_to_one_side_turns_the_snake_towards_it"],
    ),
    (
        "a click to the left of the head asks to go right",
        "            wanted.push(if dc < 0 {\n"
        "                Direction::Left\n"
        "            } else {\n"
        "                Direction::Right\n"
        "            });",
        "            wanted.push(if dc < 0 {\n"
        "                Direction::Right\n"
        "            } else {\n"
        "                Direction::Left\n"
        "            });",
        ["clicking_a_square_off_to_one_side_turns_the_snake_towards_it"],
    ),
    (
        "a square level with the head is still asked for a turn up or down",
        "        if dr != 0 {",
        "        if true {",
        ["clicking_the_square_the_head_is_on_asks_for_nothing"],
    ),
    (
        "a square in the head's own column is still asked for a turn sideways",
        "        if dc != 0 {",
        "        if true {",
        ["clicking_the_square_the_head_is_on_asks_for_nothing"],
    ),
    (
        "only the row a square is in is steered towards",
        "        if dc != 0 {\n"
        "            wanted.push(if dc < 0 {\n"
        "                Direction::Left\n"
        "            } else {\n"
        "                Direction::Right\n"
        "            });\n"
        "        }\n",
        "",
        ["clicking_a_square_off_to_one_side_turns_the_snake_towards_it"],
    ),
    (
        "only the column a square is in is steered towards",
        "        if dr != 0 {\n"
        "            wanted.push(if dr < 0 {\n"
        "                Direction::Up\n"
        "            } else {\n"
        "                Direction::Down\n"
        "            });\n"
        "        }\n",
        "",
        ["clicking_a_square_off_to_one_side_turns_the_snake_towards_it"],
    ),
    (
        "a turn the snake cannot take stops it looking at the other axis",
        "        for dir in wanted {\n"
        "            if self.queue_direction(dir) == EventResult::Consumed {\n"
        "                return EventResult::Consumed;\n"
        "            }\n"
        "        }\n"
        "        EventResult::Ignored",
        "        for dir in wanted {\n"
        "            return self.queue_direction(dir);\n"
        "        }\n"
        "        EventResult::Ignored",
        ["a_click_behind_the_snake_still_turns_it"],
    ),
    (
        "a click is measured from the tail rather than the head",
        "        let Some(head) = self.snake.first().copied() else {\n"
        "            return EventResult::Ignored;\n"
        "        };\n"
        "        let (Ok(row), Ok(col))",
        "        let Some(head) = self.snake.last().copied() else {\n"
        "            return EventResult::Ignored;\n"
        "        };\n"
        "        let (Ok(row), Ok(col))",
        ["clicking_the_square_the_head_is_on_asks_for_nothing"],
    ),
]
MUTATIONS += [
    # ── The bands ───────────────────────────────────────────────────
    (
        "the bands may spend the whole window on themselves",
        "        let spare = h * 0.45;",
        "        let spare = h * 1.0;",
        ["the_board_keeps_most_of_the_window"],
    ),
    (
        "the header is twice as deep as it should be",
        "        let mut hdr = (h * 0.09).clamp(22.0, 54.0);",
        "        let mut hdr = (h * 0.18).clamp(22.0, 54.0);",
        ["the_board_keeps_most_of_the_window"],
    ),
    (
        "the footer is half as deep as it should be",
        "        let mut ftr = (h * 0.08).clamp(20.0, 46.0);",
        "        let mut ftr = (h * 0.04).clamp(20.0, 46.0);",
        ["the_board_keeps_most_of_the_window"],
    ),
    (
        "the header gives up its height before the footer",
        "            if hdr > spare {\n"
        "                hdr = spare;\n"
        "                ftr = 0.0;\n"
        "            } else {\n"
        "                ftr = spare - hdr;\n"
        "            }",
        "            if ftr > spare {\n"
        "                ftr = spare;\n"
        "                hdr = 0.0;\n"
        "            } else {\n"
        "                hdr = spare - ftr;\n"
        "            }",
        ["a_window_squashed_from_below_loses_its_switches_before_its_score"],
    ),
    (
        "the footer keeps its whole height when the header has taken it all",
        "                ftr = 0.0;",
        "                ftr = spare;",
        ["a_window_squashed_from_below_loses_its_switches_before_its_score"],
    ),
    (
        "the footer takes the whole allowance rather than what is left of it",
        "                ftr = spare - hdr;",
        "                ftr = spare;",
        ["the_board_keeps_most_of_the_window"],
    ),
    (
        "a window squashed from below keeps no header at all",
        "        let header = Rect::new(0.0, 0.0, w, hdr);",
        "        let header = Rect::new(0.0, 0.0, w, 0.0);",
        ["a_window_squashed_from_below_loses_its_switches_before_its_score"],
    ),
    (
        "the footer is a band of no height",
        "        let footer = Rect::new(0.0, h - ftr, w, ftr);",
        "        let footer = Rect::new(0.0, h - ftr, w, 0.0);",
        ["a_window_squashed_from_below_loses_its_switches_before_its_score"],
    ),
    (
        "the body starts at the top of the window rather than under the header",
        "        let body = Rect::new(\n            pad,\n            hdr + pad,",
        "        let body = Rect::new(\n            pad,\n            pad,",
        ["the_body_sits_between_the_bands_and_inside_the_window"],
    ),
    (
        "the body is as wide as the window and hangs off the end of it",
        "            (w - pad * 2.0).max(0.0),",
        "            w,",
        ["the_body_sits_between_the_bands_and_inside_the_window"],
    ),
    (
        "the body reaches down into the footer",
        "            (footer.y - hdr - pad * 2.0).max(0.0),",
        "            (footer.y - hdr).max(0.0),",
        ["the_body_sits_between_the_bands_and_inside_the_window"],
    ),
    # ── Board and panel ─────────────────────────────────────────────
    (
        "the panel may take the whole body",
        "        let ceiling = (self.body.w * STATS_SHARE - self.pad).max(0.0);",
        "        let ceiling = self.body.w;",
        ["the_stats_panel_never_takes_more_than_its_share_of_the_body"],
    ),
    (
        "the panel always takes its ceiling rather than what it asked for",
        "        let stats_w = wanted.max(0.0).min(ceiling);",
        "        let stats_w = ceiling;",
        ["the_stats_panel_is_as_wide_as_what_is_written_in_it"],
    ),
    (
        "the board keeps the whole body and the panel is drawn over it",
        "        let board_w = (self.body.w - stats_w - self.pad).max(0.0);",
        "        let board_w = self.body.w;",
        ["the_panel_sits_beside_the_board_and_not_on_it"],
    ),
    (
        "the panel is put at the left of the body, on top of the board",
        "            self.body.right() - stats_w,",
        "            self.body.x,",
        ["the_panel_sits_beside_the_board_and_not_on_it"],
    ),
    (
        "the panel is as wide as its names and not as wide as its numbers",
        "                text::measure(name, l.small, FontWeightHint::Regular)\n"
        "                    + l.pad * 2.0\n"
        "                    + text::measure(value, l.small, FontWeightHint::Bold)",
        "                text::measure(name, l.small, FontWeightHint::Regular)\n"
        "                    + l.pad * 2.0\n"
        "                    + 0.0 * text::measure(value, l.small, FontWeightHint::Bold)",
        ["the_stats_panel_is_as_wide_as_what_is_written_in_it"],
    ),
    (
        "the panel is as wide as its narrowest row",
        "            .fold(heading, f32::max)",
        "            .fold(heading, f32::min)",
        ["the_stats_panel_is_as_wide_as_what_is_written_in_it"],
    ),
    (
        "the board is measured against the whole body and not what is left of it",
        "        let (area, _) = l.split(self.stats_width(l));",
        "        let (area, _) = l.split(0.0);",
        ["the_stats_panel_is_as_wide_as_what_is_written_in_it"],
    ),
    # ── Fitting the squares ─────────────────────────────────────────
    (
        "the squares are fitted by the looser of the two axes",
        "        let cell = (area.w / per_w).min(area.h / per_h).max(0.0);",
        "        let cell = (area.w / per_w).max(area.h / per_h).max(0.0);",
        ["the_board_fits_whatever_window_it_is_given"],
    ),
    (
        "the gaps are not counted when fitting across",
        "        let per_w = across + (across - 1.0) * GAP_PER_CELL;",
        "        let per_w = across;",
        ["the_board_fits_whatever_window_it_is_given"],
    ),
    (
        "the gaps are not counted when fitting down",
        "        let per_h = down + (down - 1.0) * GAP_PER_CELL;",
        "        let per_h = down;",
        ["the_board_fits_whatever_window_it_is_given"],
    ),
    (
        "the squares are drawn edge to edge with no gap between them",
        "        let gap = cell * GAP_PER_CELL;",
        "        let gap = 0.0;",
        ["the_squares_are_square_and_evenly_spaced"],
    ),
    (
        "the board's width does not count the gaps it contains",
        "        let span_w = across * cell + (across - 1.0) * gap;",
        "        let span_w = across * cell;",
        ["the_board_fits_whatever_window_it_is_given"],
    ),
    (
        "the board's height does not count the gaps it contains",
        "        let span_h = down * cell + (down - 1.0) * gap;",
        "        let span_h = down * cell;",
        ["the_board_fits_whatever_window_it_is_given"],
    ),
    (
        "the board is pushed against the left of the space it was given",
        "            area.x + (area.w - span_w) / 2.0,",
        "            area.x,",
        ["the_board_sits_in_the_middle_of_the_space_it_was_given"],
    ),
    (
        "the board is pushed against the top of the space it was given",
        "            area.y + (area.h - span_h) / 2.0,",
        "            area.y,",
        ["the_board_sits_in_the_middle_of_the_space_it_was_given"],
    ),
    # ── One square ──────────────────────────────────────────────────
    (
        "the columns are spaced by a square and not by a square and a gap",
        "            self.cells.x + usize_f32(col) * self.step(),",
        "            self.cells.x + usize_f32(col) * self.cell,",
        ["the_squares_are_square_and_evenly_spaced"],
    ),
    (
        "the rows are spaced by a square and not by a square and a gap",
        "            self.cells.y + usize_f32(row) * self.step(),",
        "            self.cells.y + usize_f32(row) * self.cell,",
        ["the_squares_are_square_and_evenly_spaced"],
    ),
    (
        "a square is half as tall as it is wide",
        "            self.cell,\n            self.cell,\n        )\n    }",
        "            self.cell,\n            self.cell * 0.5,\n        )\n    }",
        ["the_squares_are_square_and_evenly_spaced"],
    ),
    (
        "the gaps belong to nobody, so a click in one falls through",
        "        Rect::new(r.x - half, r.y - half, r.w + self.gap, r.h + self.gap)",
        "        Rect::new(r.x, r.y, r.w, r.h)",
        ["a_click_in_a_gap_lands_on_the_square_it_is_nearest"],
    ),
    (
        # Grown by the whole gap on one side and nothing on the other: the box
        # is the right size, in the wrong place, and every gap still belongs to
        # somebody -- so only a test that asks *which* square catches it.
        "a click box is grown away from its ink rather than around it",
        "        let half = self.gap / 2.0;",
        "        let half = 0.0;",
        ["a_click_in_a_gap_lands_on_the_square_it_is_nearest"],
    ),
    (
        "a square's hit box is moved off its ink rather than grown around it",
        "        Rect::new(r.x - half, r.y - half, r.w + self.gap, r.h + self.gap)",
        "        Rect::new(r.x - half, r.y - half, r.w, r.h)",
        ["a_square_is_clickable_where_its_ink_is"],
    ),
    # ── Drawing the board ───────────────────────────────────────────
    (
        "the squares are filed under their own transpose",
        "                f.hit(Target::Cell(row, col), b.cell_hit(row, col));",
        "                f.hit(Target::Cell(col, row), b.cell_hit(row, col));",
        ["a_square_is_clickable_where_its_ink_is"],
    ),
    (
        "the board's own squares are not drawn",
        "                fill(f, r, SURFACE0, radius);",
        "                fill(f, Rect::EMPTY, SURFACE0, radius);",
        ["every_square_of_the_board_is_drawn"],
    ),
    (
        "the first column of the board is not drawn",
        "            for col in 0..b.cols {",
        "            for col in 1..b.cols {",
        ["every_square_of_the_board_is_drawn"],
    ),
    (
        "the first row of the board is not drawn",
        "        for row in 0..b.rows {",
        "        for row in 1..b.rows {",
        ["every_square_of_the_board_is_drawn"],
    ),
    (
        "the second segment is drawn as the head",
        "            let head = i == 0;",
        "            let head = i == 1;",
        ["the_snake_is_drawn_where_the_snake_is"],
    ),
    (
        "the head is the same colour as the rest of the snake",
        "            fill(f, r, if head { GREEN } else { TEAL }, radius);",
        "            fill(f, r, TEAL, radius);",
        ["the_snake_is_drawn_where_the_snake_is"],
    ),
    (
        "the food is not drawn at all",
        "        self.draw_food(f, b);\n",
        "",
        ["the_food_is_drawn_where_the_food_is"],
    ),
    (
        "the food is drawn at its own transpose",
        "        if let (Ok(row), Ok(col)) = (usize::try_from(normal.row), usize::try_from(normal.col))",
        "        if let (Ok(row), Ok(col)) = (usize::try_from(normal.col), usize::try_from(normal.row))",
        ["the_food_is_drawn_where_the_food_is"],
    ),
    (
        "the food is shrunk out of existence",
        "            let r = shrink(b.cell_rect(row, col), b.cell * 0.15);",
        "            let r = shrink(b.cell_rect(row, col), b.cell * 0.6);",
        ["the_food_is_drawn_where_the_food_is"],
    ),
    (
        "the bonus is the same size all the way round its pulse",
        "        let scale = self.pulse_scale();",
        "        let scale = 1.0;",
        ["the_bonus_food_pulses_as_the_snake_moves"],
    ),
    (
        "the bonus pulse runs backwards",
        "        let r = shrink(cell, cell.w * (1.0 - scale) / 2.0);",
        "        let r = shrink(cell, cell.w * scale / 2.0);",
        ["the_bonus_food_pulses_as_the_snake_moves"],
    ),
    (
        "the pulse never moves off its first step",
        "        let i = usize::try_from(self.pulse_counter % 8).unwrap_or(0);",
        "        let i = usize::try_from(self.pulse_counter % 1).unwrap_or(0);",
        ["the_bonus_food_pulses_as_the_snake_moves"],
    ),
    # ── The header, the footer and the overlay ──────────────────────
    (
        "the header shows the score where the best score should be",
        '        let best = format!("Best {}", self.high_score);',
        '        let best = format!("Best {}", self.score);',
        ["the_header_shows_the_score_and_the_best_one"],
    ),
    (
        "the header shows the best score where the score should be",
        '        let score = format!("Score {}", self.score);',
        '        let score = format!("Score {}", self.high_score);',
        ["the_header_shows_the_score_and_the_best_one"],
    ),
    (
        "a game in play is called paused",
        '            GameState::Playing => "Playing",',
        '            GameState::Playing => "Paused",',
        ["the_header_says_what_state_the_game_is_in"],
    ),
    (
        "a paused game is called something else",
        '            GameState::Paused => "Paused",',
        '            GameState::Paused => "Waiting",',
        ["the_header_says_what_state_the_game_is_in"],
    ),
    (
        "a finished game is called something else",
        '            GameState::GameOver => "Game over",',
        '            GameState::GameOver => "Finished",',
        ["the_header_says_what_state_the_game_is_in"],
    ),
    (
        "a won game is called something else",
        '            GameState::Won => "You win",',
        '            GameState::Won => "Finished",',
        ["the_header_says_what_state_the_game_is_in"],
    ),
    (
        "the switches are not clickable",
        "            f.hit(target, box_rect);",
        "            f.hit(target, Rect::EMPTY);",
        ["the_screen_offers_every_control_the_program_has"],
    ),
    (
        "the switches are laid one on top of another",
        "    area.x += w + gap;",
        "    area.x += 0.0;",
        ["each_difficulty_switch_picks_its_own_difficulty"],
    ),
    (
        "the restart switch is always lit",
        '            (Target::Restart, "Restart", false),',
        '            (Target::Restart, "Restart", true),',
        ["the_difficulty_switch_that_is_chosen_is_the_one_lit_up"],
    ),
    (
        "the pause switch never offers to carry on",
        "                if self.state == GameState::Paused {\n"
        '                    "Resume"\n'
        "                } else {\n"
        '                    "Pause"\n'
        "                },",
        '                "Pause",',
        ["the_pause_switch_pauses_and_then_offers_to_carry_on"],
    ),
    (
        "the wrap switch is never lit",
        "            (Target::Wrap, \"Wrap\", self.wrap_mode),",
        "            (Target::Wrap, \"Wrap\", false),",
        ["the_wrap_switch_turns_the_walls_into_doorways"],
    ),
    (
        "no difficulty switch is ever lit",
        "                level == self.difficulty,",
        "                false,",
        ["the_difficulty_switch_that_is_chosen_is_the_one_lit_up"],
    ),
    (
        "the footer offers only the first two difficulties",
        "        for level in DIFFICULTIES {",
        "        for level in DIFFICULTIES.into_iter().take(2) {",
        ["the_footer_names_every_difficulty"],
    ),
    (
        "a game in play is written across as paused",
        "            GameState::Playing => return,",
        '            GameState::Playing => ("Paused", "P or the Pause switch to carry on", PEACH),',
        ["a_game_that_is_still_going_has_nothing_across_the_board"],
    ),
    # ── The frame ───────────────────────────────────────────────────
    (
        "the header is not drawn",
        "        self.draw_header(&mut f, &l);\n",
        "",
        ["the_header_shows_the_score_and_the_best_one"],
    ),
    (
        "the stats panel is not drawn",
        "        self.draw_stats(&mut f, &l, stats);\n",
        "",
        ["the_stats_panel_shows_what_the_game_has_counted"],
    ),
    (
        "the footer is not drawn",
        "        self.draw_footer(&mut f, &l);\n",
        "",
        ["the_screen_offers_every_control_the_program_has"],
    ),
    (
        "nothing is written across a game that has stopped",
        "        self.draw_overlay(&mut f, &l, &board);\n",
        "",
        ["a_game_that_has_stopped_says_how_to_carry_on"],
    ),
    (
        "the panel is drawn in a strip of no width",
        "        let (_, stats) = l.split(self.stats_width(&l));",
        "        let (_, stats) = l.split(0.0);",
        ["the_stats_panel_shows_what_the_game_has_counted"],
    ),
    (
        "no string is told where to stop",
        "        max_width: Some(limit),",
        "        max_width: None,",
        ["every_string_is_told_where_to_stop"],
    ),
    # ── The window ──────────────────────────────────────────────────
    (
        "a key never reaches the game",
        "        Event::Key(ev) => app.handle_key(ev),",
        "        Event::Key(_) => EventResult::Ignored,",
        ["the_arrows_steer_the_snake"],
    ),
    (
        "a click never reaches the game",
        "        Event::Mouse(ev) => app.handle_mouse(ev),",
        "        Event::Mouse(_) => EventResult::Ignored,",
        ["the_restart_switch_starts_a_new_game"],
    ),
    (
        "a tick never reaches the game",
        "        Event::Tick { elapsed_ms } => app.handle_tick(*elapsed_ms),",
        "        Event::Tick { .. } => EventResult::Ignored,",
        ["a_tick_from_the_window_moves_the_snake"],
    ),
    (
        "a resize is read the wrong way round",
        "            app.resize(f32_from_u32(*width), f32_from_u32(*height));",
        "            app.resize(f32_from_u32(*height), f32_from_u32(*width));",
        ["a_resize_moves_the_switches_and_the_clicks_follow_them"],
    ),
    (
        "an event the game has no use for is claimed anyway",
        "        _ => EventResult::Ignored,\n    }\n}",
        "        _ => EventResult::Consumed,\n    }\n}",
        ["an_event_the_game_has_no_use_for_is_left_alone"],
    ),
    (
        "the window is given the wrong name",
        '        "Snake".to_string()',
        '        "snake".to_string()',
        ["the_window_is_named_and_sized"],
    ),
    (
        "the window is given the wrong identifier",
        '        "snake".to_string()\n    }\n\n    fn initial_size',
        '        "Snake".to_string()\n    }\n\n    fn initial_size',
        ["the_window_is_named_and_sized"],
    ),
    (
        "the window opens on its side",
        "    fn initial_size(&self) -> (u32, u32) {\n"
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "    fn initial_size(&self) -> (u32, u32) {\n"
        "        (WINDOW_HEIGHT as u32, WINDOW_WIDTH as u32)",
        ["the_window_is_named_and_sized"],
    ),
    (
        "the window is asked for no ticks at all",
        "        Some(TICK)",
        "        None",
        ["the_window_asks_for_a_tick_often_enough_to_run_the_fastest_game"],
    ),
    (
        "closing the window does not end the program",
        "        if matches!(event, Event::CloseRequested) {\n"
        "            return Response::Exit;\n"
        "        }\n",
        "",
        ["closing_the_window_ends_the_program"],
    ),
    (
        "an event that changed something asks for no redraw",
        "            EventResult::Consumed => Response::Redraw,",
        "            EventResult::Consumed => Response::Idle,",
        ["an_event_that_changed_something_asks_for_a_redraw"],
    ),
    (
        "an event the game ignored asks for a redraw anyway",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["an_event_that_changed_something_asks_for_a_redraw"],
    ),
    (
        "the size a frame is drawn at is thrown away",
        "        self.resize(width, height);\n",
        "",
        ["the_size_a_frame_is_drawn_at_is_the_size_the_next_click_is_read_against"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "snake", timeout=240, only=sys.argv[1:] or None))

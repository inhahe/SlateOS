"""Mutation test for the sokoban suite.

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
    # -- Layout -------------------------------------------------------
    (
        # One number decides the whole board.  Solving for it from the width
        # alone still leaves the cells square -- they just stop fitting the
        # band, which is what the band test is watching for.
        "the cell size is solved from the width alone",
        "            let cell = (body.w / per_w).min(body.h / per_h).max(0.0);",
        "            let cell = (body.w / per_w).max(0.0);",
        ["the_board_is_drawn_with_square_cells_inside_the_body"],
    ),
    (
        # A stretched grid is one whose cells are no longer where a square hit
        # box says they are.
        "the board is stretched to fill the body rather than kept square",
        "            let grid_h = rows as f32 * cell + (rows as f32 - 1.0) * gap;",
        "            let grid_h = body.h;",
        ["the_board_is_drawn_with_square_cells_inside_the_body"],
    ),
    (
        # The footer is measured from the bottom edge, not the top: a footer
        # laid out from the top sits on the header in every window.
        "the footer is placed from the top of the window",
        "        let footer = Rect::new(0.0, h - ftr_h, w, ftr_h);",
        "        let footer = Rect::new(0.0, ftr_h, w, ftr_h);",
        ["the_chrome_bands_do_not_overlap_each_other_or_the_body"],
    ),
    (
        # The controls stack on top of the footer, so their `y` is the footer's
        # minus their own height.  Anchoring them to the window bottom instead
        # puts them over the footer whenever there is a footer.
        "the controls are stacked on the window rather than on the footer",
        "        let controls = Rect::new(0.0, footer.y - ctl_h, w, ctl_h);",
        "        let controls = Rect::new(0.0, h - ctl_h, w, ctl_h);",
        ["the_chrome_bands_do_not_overlap_each_other_or_the_body"],
    ),
    (
        # The body stops where the controls start.  Reading the footer's edge
        # instead gives the body the controls band as well.
        "the body runs on under the controls",
        "        let bottom = controls.y;",
        "        let bottom = footer.y;",
        ["the_chrome_bands_do_not_overlap_each_other_or_the_body"],
    ),
    (
        # Without a drop order the chrome keeps every band and the body is
        # squeezed to nothing in a short window.
        "no band is ever dropped when the window is too short for the chrome",
        "        for &i in &BAND_DROP_ORDER {\n"
        "            if wants.iter().sum::<f32>() <= budget {\n"
        "                break;\n"
        "            }\n"
        "            if let Some(band) = wants.get_mut(i) {\n"
        "                *band = 0.0;\n"
        "            }\n"
        "        }",
        "",
        ["the_chrome_bands_do_not_overlap_each_other_or_the_body"],
    ),
    (
        "the layout ignores the window it was given",
        "            window: Rect::new(0.0, 0.0, w, h),",
        "            window: Rect::new(0.0, 0.0, 800.0, 600.0),",
        ["every_band_stays_inside_the_window"],
    ),
    (
        # The shape of the warehouse is the only thing that makes one level's
        # board different from another's.
        "the board is sized from a fixed shape rather than the level's",
        "            let per_w = cols as f32 + (cols as f32 + 1.0) * GAP_PER_CELL;",
        "            let per_w = 8.0;",
        ["the_layout_is_a_function_of_the_window_size_and_the_level_shape_alone"],
    ),
    (
        "a cell off the grid still gets a rectangle",
        "        if self.cell <= 0.0 || row >= self.rows || col >= self.cols {\n"
        "            return Rect::EMPTY;\n"
        "        }",
        "",
        ["a_cell_off_the_grid_has_no_rectangle"],
    ),
    (
        "the cells are laid out without the gap between them",
        "            self.board.x + col as f32 * (self.cell + self.gap),\n"
        "            self.board.y + row as f32 * (self.cell + self.gap),",
        "            self.board.x + col as f32 * self.cell,\n"
        "            self.board.y + row as f32 * self.cell,",
        ["cells_tile_the_board_without_overlapping"],
    ),
    (
        "row and column are read the other way round",
        "            self.board.x + col as f32 * (self.cell + self.gap),\n"
        "            self.board.y + row as f32 * (self.cell + self.gap),",
        "            self.board.x + row as f32 * (self.cell + self.gap),\n"
        "            self.board.y + col as f32 * (self.cell + self.gap),",
        ["cells_tile_the_board_without_overlapping"],
    ),
    (
        "the menu fits one more row than the body has room for",
        "        (self.body.h / self.row) as usize",
        "        (self.body.h / self.row) as usize + 1",
        ["the_menu_shows_as_many_rows_as_the_body_has_room_for"],
    ),
    (
        "a menu slot past the bottom of the body still gets a rectangle",
        "        if slot >= self.list_rows() {\n"
        "            return Rect::EMPTY;\n"
        "        }",
        "",
        ["the_menu_shows_as_many_rows_as_the_body_has_room_for"],
    ),
    (
        "the buttons are laid out against a fixed count rather than their own",
        "        let bw = inner / count;",
        "        let bw = inner / 3.0;",
        ["the_buttons_share_the_controls_band_in_order"],
    ),
    (
        # The buttons are centred in the band vertically; anchoring them to its
        # top edge instead lets the bottom row of pixels fall out of the band.
        "the buttons sit on the top edge of their band rather than centred",
        "        let y = self.controls.y + (self.controls.h - bh) / 2.0;",
        "        let y = self.controls.y;",
        ["the_buttons_share_the_controls_band_in_order"],
    ),
    (
        # The panel this replaced was a fixed 320x150 box, so a window smaller
        # than that drew the celebration off the edge of the screen.
        "the victory panel is a fixed box again",
        "        let w = (self.window.w * 0.82).min(340.0);\n"
        "        let h = (self.window.h * 0.42).min(190.0);",
        "        let w = 320.0;\n        let h = 150.0;",
        ["the_victory_panel_stays_inside_the_window"],
    ),
    (
        "the menu never scrolls, so the cursor can leave the screen",
        "    let last = count.saturating_sub(rows);\n"
        "    // Keep the cursor off the very bottom edge where there is room to.\n"
        "    cursor.saturating_sub(rows.saturating_sub(1)).min(last)",
        "    let _ = cursor;\n    0",
        ["the_menu_scrolls_far_enough_to_reach_every_level_and_no_further"],
    ),
    (
        "the menu scrolls one row further than there is list to show",
        "    let last = count.saturating_sub(rows);",
        "    let last = count;",
        ["the_menu_scrolls_far_enough_to_reach_every_level_and_no_further"],
    ),
    # -- The parser ---------------------------------------------------
    (
        "an unknown character is swallowed as floor again",
        "                other => return Err(LevelError::UnknownTile(other)),",
        "                _ => row.push(Tile::Floor),",
        ["an_unknown_character_is_rejected_rather_than_swallowed"],
    ),
    (
        "a second player silently replaces the first",
        "                    if player.is_some() {\n"
        "                        return Err(LevelError::TwoPlayers);\n"
        "                    }",
        "",
        ["a_level_with_two_players_is_rejected"],
    ),
    (
        "a level with no player starts the player at the origin",
        "    let Some(player) = player else {\n"
        "        return Err(LevelError::NoPlayer);\n"
        "    };",
        "    let player = player.unwrap_or(Pos::new(0, 0));",
        ["a_level_with_no_player_is_rejected"],
    ),
    (
        "a level with no crates loads and is solved on arrival",
        "    if boxes.is_empty() {\n"
        "        return Err(LevelError::NoBoxes);\n"
        "    }",
        "",
        ["a_level_with_no_crates_is_rejected"],
    ),
    (
        "crates and targets are never compared, so a level can be unwinnable",
        "    if targets != boxes.len() {\n"
        "        return Err(LevelError::Unbalanced {\n"
        "            boxes: boxes.len(),\n"
        "            targets,\n"
        "        });\n"
        "    }",
        "",
        ["a_level_whose_crates_and_targets_do_not_balance_is_rejected"],
    ),
    (
        "the size bound is declared and not read",
        "    if width > MAX_LEVEL_WIDTH || height > MAX_LEVEL_HEIGHT {\n"
        "        return Err(LevelError::TooBig { width, height });\n"
        "    }",
        "",
        ["a_level_bigger_than_the_bound_is_rejected"],
    ),
    (
        "an empty level parses into a warehouse with nothing in it",
        "    if width == 0 || height == 0 {\n"
        "        return Err(LevelError::Empty);\n"
        "    }",
        "",
        ["an_empty_level_is_rejected"],
    ),
    (
        "a ragged row is padded with floor, so the player can leave the building",
        "            row.push(Tile::Empty);",
        "            row.push(Tile::Floor);",
        [
            "the_padding_around_a_ragged_level_is_not_floor",
            "the_notation_cannot_put_a_crate_or_the_player_outside_the_warehouse",
        ],
    ),
    (
        "a crate already on a target is not counted as standing on one",
        "                '*' => {\n"
        "                    row.push(Tile::Target);\n"
        "                    boxes.push(at);\n"
        "                }",
        "                '*' => {\n"
        "                    row.push(Tile::Floor);\n"
        "                    boxes.push(at);\n"
        "                }",
        ["the_notation_reads_every_character_it_claims_to"],
    ),
    (
        "the player on a target loses the target underneath",
        "                    row.push(if ch == '+' { Tile::Target } else { Tile::Floor });",
        "                    row.push(Tile::Floor);",
        ["the_notation_reads_every_character_it_claims_to"],
    ),
    (
        "a dash is not the floor a space is",
        "                ' ' | '-' => row.push(Tile::Floor),",
        "                ' ' => row.push(Tile::Floor),",
        ["every_built_in_level_parses"],
    ),
    # -- Walls, walking and pushing -----------------------------------
    (
        # `is_wall` used to ask only whether the tile was `Wall`, so the
        # padding outside a ragged warehouse was walkable.
        "outside the warehouse is walkable",
        "        !self.tile_at(pos).walkable()",
        "        self.tile_at(pos) == Tile::Wall",
        ["the_padding_beside_a_ragged_row_is_not_walkable"],
    ),
    (
        "a target is not somewhere the player can stand",
        "        matches!(self, Tile::Floor | Tile::Target)",
        "        matches!(self, Tile::Floor)",
        ["the_player_can_walk_onto_an_empty_target"],
    ),
    (
        "a cell off the grid reads as floor rather than as outside",
        "            .unwrap_or(Tile::Empty)",
        "            .unwrap_or(Tile::Floor)",
        ["a_step_off_the_grid_changes_nothing"],
    ),
    (
        "a step into a wall is taken anyway",
        "        if self.is_blocked(dest) {\n            return false;\n        }",
        "",
        ["a_step_into_a_wall_changes_nothing"],
    ),
    (
        "a crate is pushed into whatever is behind it",
        "            if self.is_blocked(beyond) || self.has_box(beyond) {\n"
        "                return false;\n"
        "            }",
        "",
        ["a_crate_with_a_wall_behind_it_does_not_move"],
    ),
    (
        "a crate is pushed into another crate",
        "            if self.is_blocked(beyond) || self.has_box(beyond) {",
        "            if self.is_blocked(beyond) {",
        ["a_crate_with_another_crate_behind_it_does_not_move"],
    ),
    (
        "the crate is left where it was and the player walks through it",
        "        if let Some((from, to)) = push {\n"
        "            self.move_box(from, to);",
        "        if let Some((from, to)) = push {\n"
        "            let _ = (from, to);",
        ["a_crate_with_floor_behind_it_is_pushed_and_counts_a_push"],
    ),
    (
        "every crate moves when one is pushed",
        "            if *b == from {\n                *b = to;\n            }",
        "            *b = to;",
        ["only_the_crate_that_was_pushed_moves"],
    ),
    (
        "a push is not counted as a push",
        "            self.pushes = self.pushes.saturating_add(1);",
        "",
        ["a_crate_with_floor_behind_it_is_pushed_and_counts_a_push"],
    ),
    (
        "a move is not counted as a move",
        "        self.moves = self.moves.saturating_add(1);",
        "",
        ["a_step_onto_floor_moves_the_player_and_counts_a_move"],
    ),
    (
        "the player does not actually move",
        "        self.player = dest;",
        "",
        ["a_step_onto_floor_moves_the_player_and_counts_a_move"],
    ),
    (
        "up and down are the other way round",
        "            Direction::Up => (-1, 0),\n            Direction::Down => (1, 0),",
        "            Direction::Up => (1, 0),\n            Direction::Down => (-1, 0),",
        ["each_direction_steps_the_way_it_is_named"],
    ),
    (
        "left and right are the other way round",
        "            Direction::Left => (0, -1),\n            Direction::Right => (0, 1),",
        "            Direction::Left => (0, 1),\n            Direction::Right => (0, -1),",
        ["each_direction_steps_the_way_it_is_named"],
    ),
    (
        "the menu accepts warehouse moves",
        "        if self.screen != Screen::Playing || self.is_solved() {\n"
        "            return false;\n"
        "        }",
        "",
        ["a_move_made_on_the_menu_is_refused"],
    ),
    (
        # This is what makes the victory overlay modal.
        "a solved level keeps accepting moves under the overlay",
        "        if self.screen != Screen::Playing || self.is_solved() {",
        "        if self.screen != Screen::Playing {",
        ["a_solved_level_refuses_every_further_move"],
    ),
    # -- Undo ---------------------------------------------------------
    (
        "undo on an empty stack claims to have taken a move back",
        "        let Some(entry) = self.undo_stack.pop_back() else {\n"
        "            return false;\n"
        "        };",
        "        let Some(entry) = self.undo_stack.pop_back() else {\n"
        "            return true;\n"
        "        };",
        ["undo_on_an_untouched_level_does_nothing_and_says_so"],
    ),
    (
        "undo does not put the player back",
        "        self.player = entry.player;",
        "",
        ["undo_takes_back_a_step"],
    ),
    (
        "undo does not put the crate back",
        "        if let Some((from, to)) = entry.push {\n            self.move_box(to, from);",
        "        if let Some((from, to)) = entry.push {\n            let _ = (from, to);",
        ["undo_takes_back_a_push_including_the_crate"],
    ),
    (
        "undo does not unwind the move count",
        "        self.moves = self.moves.saturating_sub(1);",
        "",
        ["undo_takes_back_a_step"],
    ),
    (
        "undo does not unwind the push count",
        "            self.pushes = self.pushes.saturating_sub(1);",
        "",
        ["undo_takes_back_a_push_including_the_crate"],
    ),
    (
        "the move is never recorded, so nothing can be undone",
        "        self.undo_stack.push_back(UndoEntry {\n            player: self.player,\n            push,\n        });",
        "",
        ["undo_takes_back_a_step"],
    ),
    (
        "the undo stack grows without a cap",
        "        if self.undo_stack.len() > MAX_UNDO {\n"
        "            self.undo_stack.pop_front();\n"
        "        }",
        "",
        ["the_undo_stack_stops_growing_at_its_cap"],
    ),
    # -- Winning ------------------------------------------------------
    (
        "any crate on a target counts as a win",
        "        self.boxes.iter().all(|b| self.is_target(*b))",
        "        self.boxes.iter().any(|b| self.is_target(*b))",
        ["a_level_with_one_crate_off_its_target_is_not_solved"],
    ),
    (
        "solving a level does not mark it completed",
        "        if self.is_solved()\n"
        "            && let Some(slot) = self.completed.get_mut(self.current)\n"
        "        {\n"
        "            *slot = true;\n"
        "        }",
        "",
        ["solving_a_level_marks_it_completed"],
    ),
    (
        "the crate tally counts crates rather than crates on targets",
        "        self.boxes.iter().filter(|b| self.is_target(**b)).count()",
        "        self.boxes.len()",
        ["a_level_is_solved_when_every_crate_stands_on_a_target"],
    ),
    # -- Moving between levels ----------------------------------------
    (
        "a level index past the end wraps back to the last",
        "        let Some(level) = self.levels.get(index) else {\n            return false;\n        };",
        "        let level = self.levels[index.min(self.levels.len() - 1)].clone();\n"
        "        let level = &level;",
        ["starting_a_level_that_is_not_there_changes_nothing"],
    ),
    (
        # `load_level` answers whether there was a level to load, and
        # `start_level` is the one caller that can be handed an index from
        # outside.  Ignoring the answer moves the cursor and leaves the menu
        # for a level that was never loaded.
        "starting a level does not check that the level loaded",
        "        if !self.load_level(index) {\n            return;\n        }",
        "        let _ = self.load_level(index);",
        ["starting_a_level_that_is_not_there_changes_nothing"],
    ),
    (
        "starting a level does not leave the menu",
        "        self.screen = Screen::Playing;\n    }\n\n    /// Put the current level back to how it started.",
        "    }\n\n    /// Put the current level back to how it started.",
        ["starting_a_level_loads_it_and_leaves_the_menu"],
    ),
    (
        "restart reloads a different level than the one being played",
        "        let _ = self.load_level(self.current);",
        "        let _ = self.load_level(0);",
        ["restart_puts_the_level_back_without_leaving_it"],
    ),
    (
        "the last level rolls over to the first instead of the menu",
        "        if next < self.levels.len() {\n"
        "            self.start_level(next);\n"
        "        } else {\n"
        "            self.to_menu();\n"
        "        }",
        "        self.start_level(next % self.levels.len());",
        ["the_next_level_after_the_last_is_the_menu"],
    ),
    (
        "the menu forgets which level was left",
        "        self.cursor = self.current;\n    }",
        "        self.cursor = 0;\n    }",
        ["leaving_for_the_menu_puts_the_cursor_on_the_level_left"],
    ),
    # -- The pointer --------------------------------------------------
    (
        "a click walks the whole way rather than one step",
        "                std::cmp::Ordering::Greater => Some(Direction::Right),\n"
        "                std::cmp::Ordering::Less => Some(Direction::Left),",
        "                std::cmp::Ordering::Greater => Some(Direction::Left),\n"
        "                std::cmp::Ordering::Less => Some(Direction::Right),",
        ["a_cell_in_the_players_row_or_column_gives_the_step_towards_it"],
    ),
    (
        "a diagonal click is given a direction of its own",
        "                Some(Direction::Up)\n            }\n"
        "        } else {\n            None\n        }",
        "                Some(Direction::Up)\n            }\n"
        "        } else {\n            Some(Direction::Right)\n        }",
        ["a_cell_off_the_players_row_and_column_gives_no_step"],
    ),
    (
        "clicking the player is a move",
        "                std::cmp::Ordering::Equal => None,",
        "                std::cmp::Ordering::Equal => Some(Direction::Right),",
        ["the_players_own_cell_gives_no_step"],
    ),
    (
        "a target that did nothing reports that something happened",
        "                if self.activate(target) {\n"
        "                    EventResult::Consumed\n"
        "                } else {",
        "                if true {\n"
        "                    EventResult::Consumed\n"
        "                } else {",
        ["the_undo_button_with_nothing_to_undo_reports_that_nothing_happened"],
    ),
    (
        "every mouse event is a click, press or release, left or right",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {\n"
        "            return EventResult::Ignored;\n"
        "        }",
        "",
        ["only_a_left_press_counts"],
    ),
    (
        "a click on nothing is treated as a click on something",
        "            None => EventResult::Ignored,",
        "            None => EventResult::Consumed,",
        ["a_click_on_nothing_is_ignored"],
    ),
    (
        "the click is read against a size the window was never at",
        "        let (w, h) = self.size_drawn;",
        "        let (w, h) = Self::SIZE;",
        ["a_resize_event_changes_the_size_the_next_click_is_read_against"],
    ),
    (
        "the Play button starts a level the cursor is not on",
        "            Target::Play => {\n                self.start_level(self.cursor);",
        "            Target::Play => {\n                self.start_level(0);",
        ["clicking_play_starts_the_level_the_cursor_is_on"],
    ),
    (
        "a click on a level row starts a different level",
        "            Target::Level(index) => {\n                self.start_level(index);",
        "            Target::Level(index) => {\n                self.start_level(index + 1);",
        ["clicking_a_level_row_starts_that_level"],
    ),
    # -- Drawing ------------------------------------------------------
    (
        "a square outside the warehouse is drawn and clickable",
        "                if tile == Tile::Empty {\n                    continue;\n                }",
        "",
        ["the_board_draws_a_square_for_every_tile_inside_the_warehouse"],
    ),
    (
        "the crate takes the click off its own square",
        "                if home { GREEN } else { PEACH },\n"
        "                CornerRadii::all((l.cell * 0.12).max(1.0)),\n"
        "            );\n"
        "        }",
        "                if home { GREEN } else { PEACH },\n"
        "                CornerRadii::all((l.cell * 0.12).max(1.0)),\n"
        "            );\n"
        "            f.hit(Target::Undo, r);\n"
        "        }",
        ["the_crate_and_the_player_do_not_take_the_click_off_their_square"],
    ),
    (
        "the menu draws every level rather than the ones it has room for",
        "            if index >= self.level_count() {\n                break;\n            }",
        "",
        ["the_menu_draws_a_row_for_every_level_it_has_room_for"],
    ),
    (
        "the menu never follows the cursor down the list",
        "        let first = first_visible(self.cursor, self.level_count(), rows);",
        "        let first = 0_usize;",
        ["the_menu_scrolls_the_cursor_into_view"],
    ),
    (
        "the header does not say which level is being played",
        '                    "Level {} of {}",\n'
        "                    self.current.saturating_add(1),",
        '                    "Level {} of {}",\n'
        "                    self.current.saturating_add(2),",
        ["the_header_says_which_level_and_how_it_is_going"],
    ),
    (
        "the menu header does not count the levels solved",
        '                format!("Solved: {}/{}", self.completed_count(), self.level_count()),',
        '                format!("Solved: {}/{}", 0, self.level_count()),',
        ["the_menu_header_counts_the_levels_solved"],
    ),
    (
        "both screens show the same keyboard reminder",
        "            Screen::Select => SELECT_FOOTER,",
        "            Screen::Select => PLAY_FOOTER,",
        ["each_screen_shows_its_own_keyboard_reminder"],
    ),
    (
        "both screens offer the same buttons",
        "            Screen::Select => SELECT_BUTTONS.to_vec(),",
        "            Screen::Select => PLAY_BUTTONS.to_vec(),",
        ["the_menu_offers_only_play_and_the_warehouse_only_its_three"],
    ),
    (
        "the footer line is drawn without a width limit",
        "                (l.footer.w - l.pad * 2.0).max(0.0),",
        "                f32::MAX,",
        ["every_string_the_footer_draws_is_bounded_and_clipped"],
    ),
    (
        # The mirror image of positioning text by guessing its width: right
        # alignment with no left bound puts a long string off the screen.
        "a right-aligned string is bounded on one side only",
        "    let w = text::measure(l.text, l.size, l.weight).min(room);",
        "    let w = text::measure(l.text, l.size, l.weight);",
        ["every_string_is_drawn_inside_the_window_it_was_given"],
    ),
    (
        "a right-aligned counter sits in a fixed column",
        "    let w = text::measure(l.text, l.size, l.weight).min(room);",
        "    let w = 60.0_f32.min(room);",
        ["a_right_aligned_counter_moves_with_its_own_width"],
    ),
    (
        "a centred string is centred on a width the renderer does not stop at",
        "    let w = text::measure(l.text, l.size, l.weight).min(r.w);",
        "    let w = text::measure(l.text, l.size, l.weight);",
        ["a_label_never_starts_left_of_the_box_it_is_centred_in"],
    ),
    # -- The victory overlay ------------------------------------------
    (
        "the overlay is drawn before the level is solved",
        "        if self.screen == Screen::Playing && self.is_solved() {",
        "        if self.screen == Screen::Playing {",
        ["the_overlay_appears_only_once_the_level_is_solved"],
    ),
    (
        # `discard_hits` is what makes it modal.
        "the finished warehouse is still clickable under the scrim",
        "        f.discard_hits();",
        "",
        ["the_overlay_swallows_a_click_on_what_is_behind_it"],
    ),
    (
        "the overlay's decoration is placed off the window it decorates",
        "                    dx.clamp(l.window.x, l.window.right() - d),\n"
        "                    dy.clamp(l.window.y, l.window.bottom() - d),",
        "                    dx,\n                    dy,",
        ["the_overlay_survives_every_window_in_the_list"],
    ),
    (
        "the overlay's Next button replays the level instead",
        "            Target::Next => {\n                self.next_level();",
        "            Target::Next => {\n                self.restart();",
        ["the_overlay_buttons_do_what_they_are_labelled"],
    ),
    # -- The keyboard -------------------------------------------------
    (
        "every binding runs twice, once down and once up",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }",
        "",
        ["a_key_coming_back_up_does_nothing"],
    ),
    (
        "both screens read the same key table",
        "            Screen::Select => self.key_select(ev, plain),",
        "            Screen::Select => self.key_playing(ev, plain),",
        ["the_two_screens_read_the_same_key_differently"],
    ),
    (
        "a modifier held with a number key is ignored",
        "            | Key::Num9\n                if plain =>",
        "            | Key::Num9 =>",
        ["a_number_key_with_a_modifier_held_is_handed_on"],
    ),
    (
        "a modifier held with undo is ignored",
        "            Key::Z if plain => {",
        "            Key::Z => {",
        ["a_warehouse_shortcut_with_a_modifier_held_is_handed_on"],
    ),
    (
        "the menu cursor runs off the bottom of the list",
        "            Key::Down | Key::S => self.cursor = self.cursor.saturating_add(1).min(last),",
        "            Key::Down | Key::S => self.cursor = self.cursor.saturating_add(1),",
        ["up_and_down_walk_the_menu_and_stop_at_the_ends"],
    ),
    (
        "End does not reach the last level",
        "            Key::End => self.cursor = last,",
        "            Key::End => self.cursor = 0,",
        ["home_and_end_jump_to_the_ends_of_the_list"],
    ),
    (
        "a number key jumps to the wrong level",
        "        Key::Num5 => 4,",
        "        Key::Num5 => 3,",
        ["a_number_key_jumps_to_that_level"],
    ),
    (
        "WASD does not walk",
        "            Key::Up | Key::W => {\n                self.try_move(Direction::Up);",
        "            Key::Up => {\n                self.try_move(Direction::Up);",
        ["the_arrows_and_wasd_both_walk"],
    ),
    (
        "Escape on the warehouse is an empty arm again",
        "            Key::Escape => self.to_menu(),",
        "            Key::Escape => {}",
        ["z_undoes_and_r_restarts_and_n_advances_and_escape_leaves"],
    ),
    (
        "Enter abandons an unsolved position",
        "                if !self.is_solved() {\n"
        "                    return EventResult::Ignored;\n"
        "                }",
        "",
        ["enter_only_moves_on_once_the_level_is_solved"],
    ),
    (
        "a key the program does not use is swallowed",
        "            _ => return EventResult::Ignored,\n"
        "        }\n"
        "        EventResult::Consumed\n"
        "    }\n"
        "}",
        "            _ => {}\n"
        "        }\n"
        "        EventResult::Consumed\n"
        "    }\n"
        "}",
        ["a_key_the_program_does_not_use_is_handed_on"],
    ),
    # -- The window's own plumbing ------------------------------------
    (
        "the resize event does not reach the size clicks are read against",
        "        Event::Resize { width, height } => {",
        "        Event::Resize { width, height } if false => {",
        ["a_resize_event_changes_the_size_the_next_click_is_read_against"],
    ),
    (
        # If these drift apart, every layout test checks a window the program
        # never actually opens.
        "the window opens at a size the probe does not draw at",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (800, 600)",
        ["the_probe_draws_at_the_size_the_window_opens_at"],
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
        "rendering does not record the size it drew at",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["rendering_records_the_size_it_drew_at"],
    ),
    # -- Lesson 109: every bound a squeeze can reach --------------------
    (
        # The four lines the whole campaign is about. A centring that does not
        # refuse a band it cannot fill is above the band by half the shortfall.
        "a run is centred in a band that cannot hold it",
        "    (!band.is_empty() && band.h >= height).then(|| band.y + (band.h - height) / 2.0)",
        "    Some(band.y + (band.h - height) / 2.0)",
        [
            "centre_line_refuses_a_band_it_cannot_fill_rather_than_going_negative",
            "no_pass_paints_outside_the_region_it_owns",
        ],
    ),
    (
        # A band with no width is no band, however tall it is.
        "a band with no width is still a band a run can be centred in",
        "    (!band.is_empty() && band.h >= height).then(|| band.y + (band.h - height) / 2.0)",
        "    (band.h >= height).then(|| band.y + (band.h - height) / 2.0)",
        ["centre_line_refuses_a_band_it_cannot_fill_rather_than_going_negative"],
    ),
    (
        # The mat is the grid grown by a gap on every side, so the solve has to
        # reserve `cols + 1` gaps and not `cols - 1`.
        "the board reserves the gaps between its cells but not the ones at its ends",
        "            let per_h = rows as f32 + (rows as f32 + 1.0) * GAP_PER_CELL;",
        "            let per_h = rows as f32 + (rows as f32 - 1.0) * GAP_PER_CELL;",
        ["the_board_is_drawn_with_square_cells_inside_the_body"],
    ),
    (
        # The mat is a named rectangle so that a test can hold the pass to it.
        # Growing it by a gap it did not reserve is the fault that naming found.
        "the mat is drawn a gap larger than the one the solve sized",
        "        fill(f, l.board_frame, CRUST, CornerRadii::all(l.gap.max(1.0)));",
        "        fill(\n"
        "            f,\n"
        "            Rect::new(\n"
        "                l.board_frame.x - l.gap,\n"
        "                l.board_frame.y - l.gap,\n"
        "                l.board_frame.w + l.gap * 2.0,\n"
        "                l.board_frame.h + l.gap * 2.0,\n"
        "            ),\n"
        "            CRUST,\n"
        "            CornerRadii::all(l.gap.max(1.0)),\n"
        "        );",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # The header's one-line fallback: falling back to one line does not make
        # one line fit.
        "the header falls back to one line without asking whether one line fits",
        "        let stack = if two_lines { title_h + sub_h } else { title_h };\n"
        "        let Some(top) = centre_line(l.header, stack) else {\n"
        "            return;\n"
        "        };",
        "        let top = if two_lines {\n"
        "            l.header.y + (l.header.h - title_h - sub_h) / 2.0\n"
        "        } else {\n"
        "            l.header.y + (l.header.h - title_h) / 2.0\n"
        "        };",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # The title takes what the counters leave. Given the whole band it runs
        # under them and off the right-hand edge in a narrow window.
        "the header's title takes the whole band rather than what the counters leave",
        "        let title_w = (right - left - counter_w - l.pad).max(0.0);",
        "        let title_w = (right - left).max(0.0);",
        ["the_headers_title_stops_where_its_counters_begin"],
    ),
    (
        # A run given no room is a run the renderer is asked to draw in
        # nothing. Containment cannot see it: `inked` takes the limit as
        # the run's width, and a box with no width is inside anything.
        "a run with no room is pushed anyway",
        "    if l.text.is_empty() || limit <= 0.0 {",
        "    if l.text.is_empty() {",
        ["no_run_is_pushed_into_a_box_with_no_room"],
    ),
    (
        # A centred run starts half the slack in, so the box's full width is
        # half the slack more than the run can have.
        "a centred run is given its box's width from where it starts",
        "    push_text(f, l, x, y, r.right() - x);",
        "    push_text(f, l, x, y, r.w);",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "a right-aligned run is given its box's width from where it starts",
        "    push_text(f, l, x, y, right - x);",
        "    push_text(f, l, x, y, (right - left).max(0.0));",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # A stroke straddles the line it is drawn on.
        "a border is drawn on the edge it is meant to sit inside",
        "    let inner = Rect::new(r.x + lw / 2.0, r.y + lw / 2.0, r.w - lw, r.h - lw);",
        "    let inner = r;",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # A clamp on the origin is not a bound on the dot.
        "a window too small for one dot gets a full set of them",
        "        if d <= l.window.w && d <= l.window.h {",
        "        if l.window.w > 0.0 {",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # The stripe's width has a one-point floor that does not scale.
        "the cursor stripe is as wide as its floor whatever the row is",
        "                if let Some(stripe) = Rect::new(r.x, r.y, (l.pad * 0.5).max(1.0), r.h).intersect(r)\n"
        "                {\n"
        "                    fill(f, stripe, PEACH, CornerRadii::all(1.0));\n"
        "                }",
        "                fill(\n"
        "                    f,\n"
        "                    Rect::new(r.x, r.y, (l.pad * 0.5).max(1.0), r.h),\n"
        "                    PEACH,\n"
        "                    CornerRadii::all(1.0),\n"
        "                );",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # The mark is bounded by the gutter it was measured for.
        "the done mark is drawn without the gutter it was measured for",
        "                (r.w - l.pad * 2.0).min(mark_w).max(0.0),",
        "                f32::MAX,",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # The menu's rows are centred with a bare offset, because the floor
        # on the row height dominates every `centre_line` that could stand
        # there. This is that floor: drop it and the first row's words go
        # above the body.
        "the menu rows have no floor to keep their type inside them",
        "            row: (font * 2.1).max(1.0),",
        "            row: (font * 0.5).max(1.0),",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the footer places its lines whether or not the band can hold them",
        "        let Some(top) = centre_line(l.footer, lh * shown as f32) else {\n"
        "            return;\n"
        "        };",
        "        let top = l.footer.y + (l.footer.h - lh * shown as f32) / 2.0;",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the victory card places its stack whether or not the panel can hold it",
        "        let Some(top) = centre_line(panel, stack) else {\n"
        "            return;\n"
        "        };",
        "        let top = panel.y + (panel.h - stack) / 2.0;",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # The converse: containment is satisfied by drawing nothing at all,
        # and a `centre_line` that refuses every band is how a pass draws
        # nothing while staying scrupulously inside its region.
        "no band is ever tall enough for a line",
        "    (!band.is_empty() && band.h >= height).then(|| band.y + (band.h - height) / 2.0)",
        "    None",
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
    (
        "the menu refuses every body rather than only the ones with no room",
        "        if l.body.is_empty() {",
        "        if true {",
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
    (
        # A band of no area gets no commands at all, not one degenerate fill.
        # This is the refusal every pass in the file leans on, which is why
        # none of them repeats it.
        "a rectangle with no area is still filled",
        "    if r.is_empty() {\n"
        "        return;\n"
        "    }\n"
        "    f.push(RenderCommand::FillRect {",
        "    f.push(RenderCommand::FillRect {",
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
    (
        # `f.clip` pushes a command whether or not the rectangle has area,
        # so a pass that clips before it has refused the band cannot be
        # silent about a band it was never given -- and leaves the clip
        # unbalanced on the way out besides.
        "the footer clips its band before it has refused it",
        "        let Some(top) = centre_line(l.footer, lh * shown as f32) else {\n"
        "            return;\n"
        "        };\n"
        "        f.clip(l.footer);",
        "        f.clip(l.footer);\n"
        "        let Some(top) = centre_line(l.footer, lh * shown as f32) else {\n"
        "            return;\n"
        "        };",
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "sokoban", timeout=120))

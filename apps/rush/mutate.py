"""Mutation test for the rush suite.

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
    # ── Layout ──────────────────────────────────────────────────────
    (
        # One number decides the whole yard.  Solving for it from the width
        # alone overflows the band a square grid was meant to fit inside.
        # Not `the_board_is_drawn_with_square_cells`: one number still sizes
        # both axes, so the cells stay square — they just stop fitting.
        "the cell size is solved from the width alone",
        "        let cell = (band.w / per_w).min(band.h / per_h).max(0.0);",
        "        let cell = (band.w / per_w).max(0.0);",
        ["the_board_never_overlaps_the_chrome"],
    ),
    (
        # The exit strip is meant to be a picture of the win rule.  Pinned to
        # the top of the board it is a picture of nothing.
        "the exit strip is not beside the row the win rule reads",
        "            let ey = by + EXIT_ROW as f32 * (cell + gap);",
        "            let ey = by;",
        ["the_exit_strip_sits_beside_the_row_the_win_rule_reads"],
    ),
    (
        # The width has to carry the strip and its gap as well as the grid.
        # Forgetting that draws a grid that ends where the window does and a
        # strip painted off the edge of it.
        "the width does not reserve room for the exit strip",
        "        let per_w = side + GAP_PER_CELL + EXIT_PER_CELL;",
        "        let per_w = side;",
        ["every_band_stays_inside_the_window"],
    ),
    (
        # `Rect::EMPTY` and a full-width strip nought pixels tall read alike to
        # anything asking "does this show?" and differently to anything asking
        # "how wide is it?".
        "a dropped band is a zero-height strip rather than empty",
        "        let footer = if ftr_h > 0.0 {\n"
        "            Rect::new(0.0, h - ftr_h, w, ftr_h)\n"
        "        } else {\n"
        "            Rect::EMPTY\n"
        "        };",
        "        let footer = Rect::new(0.0, h - ftr_h, w, ftr_h);",
        ["a_dropped_band_is_empty_rather_than_a_zero_height_strip"],
    ),
    (
        # Not `BOARD_SHARE = 0.0`: the yard's share is asserted as a *lower*
        # bound on the yard and an *upper* bound on the chrome, and this is the
        # line that makes both true.  Without it the chrome takes what it likes
        # and the yard gets the remainder.
        "the yard's share of the window is not reserved",
        "        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);",
        "        let budget = h;",
        ["the_chrome_never_takes_more_than_its_share_of_the_window"],
    ),
    (
        # The footer is the least useful band on a cramped window, so it goes
        # first and the controls — the only way to play by pointer — go last.
        "the bands are dropped in the wrong order",
        "const BAND_DROP_ORDER: [usize; 3] = [2, 0, 1];",
        "const BAND_DROP_ORDER: [usize; 3] = [1, 0, 2];",
        [
            "the_footer_is_the_first_chrome_to_go",
            "the_controls_are_the_last_chrome_to_go",
        ],
    ),
    (
        "cell_rect forgets the gaps between the columns",
        "            self.board.x + col as f32 * (self.cell + self.gap),",
        "            self.board.x + col as f32 * self.cell,",
        ["the_grid_fills_the_board_rect_exactly"],
    ),
    (
        "cell_rect forgets the gaps between the rows",
        "            self.board.y + row as f32 * (self.cell + self.gap),",
        "            self.board.y + row as f32 * self.cell,",
        ["the_grid_fills_the_board_rect_exactly"],
    ),
    (
        # Off the grid it would otherwise answer with a rectangle beyond the
        # board — a cell that is not there, drawn where nothing is.
        "cell_rect does not guard against a cell that is not on the grid",
        "        if self.cell <= 0.0 || row >= GRID_SIZE || col >= GRID_SIZE {",
        "        if self.cell <= 0.0 {",
        ["cell_rect_answers_empty_off_the_grid"],
    ),
    (
        # A truck drawn one cell long is a truck you can only click a third of.
        # Not `every_car_is_clickable_where_its_ink_is`: the hit box is the
        # same rect as the ink, so the two shrink together and go on agreeing.
        # Only a test that knows how many cells the car sits on can see it.
        "vehicle_rect ignores how long the vehicle is",
        "        let body = v.length as f32 * self.cell + (v.length as f32 - 1.0) * self.gap;",
        "        let body = self.cell;",
        ["a_vehicle_covers_exactly_the_cells_it_sits_on"],
    ),
    (
        "vehicle_rect draws a vehicle across its own axis",
        "            Orientation::Horizontal => Rect::new(head.x, head.y, body, self.cell),\n"
        "            Orientation::Vertical => Rect::new(head.x, head.y, self.cell, body),",
        "            Orientation::Horizontal => Rect::new(head.x, head.y, self.cell, body),\n"
        "            Orientation::Vertical => Rect::new(head.x, head.y, body, self.cell),",
        ["a_vehicle_covers_exactly_the_cells_it_sits_on"],
    ),
    (
        # `Rect::EMPTY` sits at the origin, so reading a dropped band's `y` puts
        # the yard's bottom edge at zero and leaves no yard at all.
        "the yard's bottom edge is read from a band that may not be there",
        "        let bottom = if ctl_h > 0.0 { controls.y } else { lower };",
        "        let bottom = controls.y;",
        ["the_board_survives_every_window_a_band_is_dropped_in"],
    ),
    (
        "the buttons ignore the padding around them",
        "        let inner = (self.controls.w - self.pad * (n + 1.0)).max(0.0);",
        "        let inner = self.controls.w;",
        ["the_buttons_stay_inside_the_controls_band"],
    ),
    # No mutation for a `controls.is_empty()` bail at the top of
    # `button_rects`: there used to be one, and it survived, because a band
    # that was dropped has a zero width and a zero height and the `bw <= 0.0`
    # test below catches it anyway.  The guard was deleted rather than covered.
    (
        "the sheet rows overflow the panel they are listed in",
        "        let listing_h = (panel.bottom() - self.pad - hint_h - listing_top).max(0.0);",
        "        let listing_h = panel.h;",
        ["the_sheet_rows_stay_inside_the_sheet_panel"],
    ),
    (
        "the victory panel is wider than the window it is centred in",
        "        let w = (self.window.w * 0.72).min(340.0);\n"
        "        let h = (self.window.h * 0.42).min(190.0);",
        "        let w = 340.0;\n        let h = 190.0;",
        ["the_victory_panel_stays_inside_the_window"],
    ),
    # ── Hit boxes ───────────────────────────────────────────────────
    (
        "an empty cell is drawn but not recorded as somewhere to click",
        "                f.hit(Target::Cell(row, col), r);",
        "",
        ["every_cell_is_clickable_where_it_is_drawn"],
    ),
    (
        # A truck recorded as one cell is a truck two thirds of which does
        # nothing when clicked.
        "a car's hit box is the cell under its nose rather than the car",
        "            f.hit(Target::Vehicle(v.id), r);",
        "            f.hit(Target::Vehicle(v.id), l.cell_rect(v.row, v.col));",
        ["the_hit_box_of_a_car_is_the_rectangle_it_was_painted_in"],
    ),
    (
        # A control that swallows a click and does nothing is worse than no
        # control: the click it ate would otherwise have reached the yard.
        "the exit strip eats clicks aimed past it",
        "            fill(f, l.exit, PLAYER_COLOR, CornerRadii::all(l.gap.max(1.0)));",
        "            fill(f, l.exit, PLAYER_COLOR, CornerRadii::all(l.gap.max(1.0)));\n"
        "            f.hit(Target::Cell(EXIT_ROW, 0), l.exit);",
        ["the_exit_strip_is_not_a_hit_target"],
    ),
    (
        # A modal that only *looks* in front is one whose buttons you can press
        # through.
        "the victory panel does not take the yard away from the pointer",
        "        // Nothing behind the panel is clickable any more — a modal that only\n"
        "        // *looks* in front is one whose buttons you can press through.\n"
        "        f.discard_hits();\n",
        "",
        ["the_victory_panel_hides_the_yard_from_the_pointer"],
    ),
    (
        "the puzzle sheet does not take the yard away from the pointer",
        "        fill(f, l.window, SCRIM, CornerRadii::ZERO);\n        f.discard_hits();",
        "        fill(f, l.window, SCRIM, CornerRadii::ZERO);",
        ["the_sheet_hides_the_yard_from_the_pointer"],
    ),
    (
        # This is the whole reason the sheet can be got out of by pointer at
        # all, which the version it replaced could not.
        "the sheet cannot be dismissed by clicking beside it",
        "        f.hit(Target::CloseSheet, l.window);",
        "",
        ["clicking_beside_the_sheet_dismisses_it"],
    ),
    (
        "a row of the sheet is drawn but not clickable",
        "            f.hit(Target::Puzzle(i), r);",
        "",
        ["clicking_a_row_of_the_sheet_opens_that_puzzle"],
    ),
    (
        # An opaque scrim paints out the very jam it is celebrating.
        "the scrim over a covered yard is opaque",
        "const SCRIM: Color = Color::rgba(0x11, 0x11, 0x1B, 0xB4);",
        "const SCRIM: Color = Color::rgba(0x11, 0x11, 0x1B, 0xFF);",
        ["the_scrim_over_a_covered_yard_is_translucent"],
    ),
    # ── Text ────────────────────────────────────────────────────────
    (
        "the undo button is drawn live whether or not it can do anything",
        "                Target::Undo => !self.undo_stack.is_empty(),",
        "                Target::Undo => true,",
        ["the_undo_button_is_drawn_dim_until_there_is_something_to_undo"],
    ),
    (
        # The counter used to be drawn at `total_width - 120.0`, a guess at how
        # wide "Moves: 1234" would turn out to be.
        "a right-aligned string does not measure itself",
        "    let w = text::measure(l.text, l.size, l.weight);\n    push_text(f, l, right - w, y, None);",
        "    push_text(f, l, right - 120.0, y, None);",
        ["the_move_counter_is_right_aligned_from_its_measured_width"],
    ),
    (
        # Every car's letter used to be drawn at `cx + vw / 2.0 - 5.0`, which is
        # a claim that the glyph is ten pixels wide.
        "a centred string is placed by guessing its width",
        "        r.x + (r.w - w) / 2.0,",
        "        r.x + r.w / 2.0 - 5.0,",
        ["a_cars_letter_is_centred_in_the_car_from_its_measured_width"],
    ),
    (
        "a centred string is not centred vertically",
        "        r.y + (r.h - lh) / 2.0,",
        "        r.y,",
        ["a_cars_letter_is_centred_in_the_car_from_its_measured_width"],
    ),
    (
        # The width that decides the centre has to be the width the renderer is
        # told to stop at, or the two disagree about where the string ends.
        "a centred string is centred in a box it is not limited to",
        "        r.y + (r.h - lh) / 2.0,\n        Some(r.w),",
        "        r.y + (r.h - lh) / 2.0,\n        None,",
        ["every_centred_string_is_limited_to_the_box_it_is_centred_in"],
    ),
    (
        # A cut with no mark is a string the reader cannot tell was cut.
        "a string cut to a width limit is cut without a mark",
        "        overflow: if limit.is_some() {\n"
        "            TextOverflow::Ellipsis\n"
        "        } else {\n"
        "            TextOverflow::Clip\n"
        "        },",
        "        overflow: TextOverflow::Clip,",
        ["every_centred_string_is_limited_to_the_box_it_is_centred_in"],
    ),
    (
        # The title used to be drawn at the left with no limit while the
        # counters were drawn against a flat 120-pixel reservation, so in a
        # narrow window the two were painted through each other.
        "the header's title is drawn without regard for the counters",
        "        let room = (right - counters_w - l.pad - left).max(0.0);",
        "        let room = (right - left).max(0.0);",
        ["the_title_gives_way_to_the_counters_rather_than_running_under_them"],
    ),
    (
        "a letter too big for its car is drawn anyway",
        "            if text::line_height(size, FontWeightHint::Bold) <= r.h\n"
        "                && text::measure(&glyph, size, FontWeightHint::Bold) <= r.w\n"
        "            {",
        "            if true {",
        ["a_letter_too_big_for_its_car_is_dropped_rather_than_spilled"],
    ),
    (
        # The footer's second line is wider than a narrow window, and the clip
        # is the only thing that stops it running off the edge.
        "the footer's lines are not clipped to the footer",
        "        f.clip(l.footer);",
        "",
        ["every_string_drawn_is_inside_the_window"],
    ),
    (
        "the footer keeps both lines in a band with room for one",
        "        let shown = if lh * 2.0 <= l.footer.h { 2 } else { 1 };",
        "        let shown = 2;",
        ["the_footer_drops_its_second_line_before_its_first"],
    ),
    # ── Rules ───────────────────────────────────────────────────────
    (
        # The win rule used to be column-only, and was correct solely because
        # every puzzle table happened to put the player on row 2.
        "the win rule reads the wrong row",
        "        self.player()\n            .is_some_and(|v| v.occupies(EXIT_ROW, EXIT_COL))",
        "        self.player().is_some_and(|v| v.occupies(0, EXIT_COL))",
        ["the_red_car_wins_by_covering_the_way_out"],
    ),
    (
        "the win rule reads the wrong column",
        "        self.player()\n            .is_some_and(|v| v.occupies(EXIT_ROW, EXIT_COL))",
        "        self.player().is_some_and(|v| v.occupies(EXIT_ROW, 0))",
        ["the_red_car_wins_by_covering_the_way_out"],
    ),
    (
        # `is_player()` used to be `color_index == 0`, so "which car wins" was
        # decided by the palette.
        "any car that reaches the way out wins",
        "        self.player()\n            .is_some_and(|v| v.occupies(EXIT_ROW, EXIT_COL))",
        "        self.vehicles.iter().any(|v| v.occupies(EXIT_ROW, EXIT_COL))",
        ["only_the_red_car_wins"],
    ),
    (
        # The win panel would otherwise stay up over a board that had moved on
        # underneath it.
        "a won yard takes further slides",
        "        if self.is_won() {\n            return false;\n        }\n        if !self.can_slide(id, delta) {",
        "        if !self.can_slide(id, delta) {",
        ["a_won_yard_takes_no_further_slides"],
    ),
    (
        # Aiming at the far wall past a blocker is how a player says "go as far
        # as you can that way"; the version this replaced did nothing at all.
        "a click past a blocker is not clamped to what the yard allows",
        "        let reach =\n"
        "            isize::try_from(self.max_slide(id, direction).min(wanted.unsigned_abs())).ok()?;",
        "        let reach = isize::try_from(wanted.unsigned_abs()).ok()?;",
        ["clicking_past_a_blocker_slides_as_far_as_the_yard_allows"],
    ),
    (
        # `break` rather than `continue` is the whole of "stops at the first
        # obstacle": with `continue` a car hops over the blocker and claims the
        # free run on its far side.  That only shows in a fixture that *has* a
        # free run on the far side, which is why one was added.
        "a car may slide onto a cell another car is standing on",
        "                Some(Some(_)) => break,",
        "                Some(Some(_)) => {}",
        [
            "a_car_cannot_slide_through_another",
            "max_slide_stops_at_the_first_obstacle",
        ],
    ),
    (
        "a car may slide off the edge of the yard",
        "                None => break,",
        "                None => {}",
        ["a_car_cannot_slide_off_the_yard"],
    ),
    (
        # Stopping the walk one short means a car can never reach the far wall,
        # and the red car can never get out from the left of the yard.  The
        # bound used to be the grid width, which no car could reach at all.
        "max_slide stops one cell short of the wall",
        "        for step in 1..=GRID_SIZE.saturating_sub(v.length) {",
        "        for step in 1..GRID_SIZE.saturating_sub(v.length) {",
        ["max_slide_stops_at_the_wall"],
    ),
    (
        # A car measures its run from the edge that faces the way it is going;
        # from the other edge the run is its own body plus the road ahead.
        "the walk starts at the wrong end of the car",
        "        let (lead_row, lead_col) = if backwards {\n"
        "            (v.row, v.col)\n"
        "        } else {\n"
        "            (v.tail_row(), v.tail_col())\n"
        "        };",
        "        let (lead_row, lead_col) = (v.row, v.col);",
        ["max_slide_stops_at_the_wall"],
    ),
    (
        # `can_slide` asks whether the free run is at least `delta` long.
        # Asking whether it is *exactly* `delta` refuses every short move down
        # a long clear road.
        "can_slide demands the exact distance rather than at least it",
        "        self.max_slide(id, delta.signum()) >= delta.unsigned_abs()",
        "        self.max_slide(id, delta.signum()) == delta.unsigned_abs()",
        ["a_car_can_slide_further_than_its_own_length"],
    ),
    (
        "a slide costs a move per cell travelled",
        "        self.moves = self.moves.saturating_add(1);",
        "        self.moves = self.moves.saturating_add(delta.unsigned_abs());",
        ["a_slide_costs_one_move_however_far_it_went"],
    ),
    (
        # The oldest move has to go, and the header shows the depth so the loss
        # is at least visible.
        "the undo stack grows past its cap",
        "        if self.undo_stack.len() > MAX_UNDO {\n            self.undo_stack.pop_front();\n        }",
        "",
        ["the_undo_stack_forgets_its_oldest_move_at_the_cap"],
    ),
    (
        # An undo entry is a car id and one signed delta: the number the move
        # added, which undo subtracts.
        "undo repeats the move rather than reversing it",
        "        let back = entry.delta.saturating_neg();",
        "        let back = entry.delta;",
        ["undo_puts_the_car_back_and_lowers_the_count"],
    ),
    (
        # `undo` used to open with `if self.status == Won { return; }`, so the
        # winning move was the one move you could not take back.
        "undo unwinds the moves in the order they were made",
        "        let Some(entry) = self.undo_stack.pop_back() else {",
        "        let Some(entry) = self.undo_stack.pop_front() else {",
        ["undo_unwinds_the_moves_in_the_order_they_were_made"],
    ),
    (
        # Wrapping here as well as in `next_puzzle` would be a second answer to
        # the same question, and the two would come to disagree.
        "loading a puzzle past the end wraps rather than doing nothing",
        "        let Some(def) = PUZZLES.get(index) else {\n            return;\n        };",
        "        let index = index % PUZZLE_COUNT;\n"
        "        let Some(def) = PUZZLES.get(index) else {\n            return;\n        };",
        ["loading_a_puzzle_past_the_end_does_nothing"],
    ),
    (
        # A slide is measured from whichever end faces the cell, so clicking
        # just past a car's nose moves it exactly one.
        "a slide is measured from the head whichever end faces the cell",
        "        let from = if along < head {\n"
        "            head\n"
        "        } else if along > tail {\n"
        "            tail\n"
        "        } else {",
        "        let from = if along < head {\n"
        "            head\n"
        "        } else if along > tail {\n"
        "            head\n"
        "        } else {",
        ["a_slide_is_measured_from_the_end_facing_the_cell"],
    ),
    (
        "the cell a car is already sitting on is somewhere to go",
        "            // A cell the car is already sitting on is not somewhere to go.\n"
        "            return None;",
        "            head",
        ["clicking_the_cell_a_car_is_already_on_is_not_somewhere_to_go"],
    ),
    (
        "ids are handed out as positions in the vector",
        "            next_id: 1,",
        "            next_id: 0,",
        ["ids_are_unique_and_never_a_position_in_the_vector"],
    ),
    # ── Events ──────────────────────────────────────────────────────
    (
        # Reading only `key` runs every binding twice per press: once on the
        # way down and once on the way back up.
        "a key coming back up is acted on too",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }",
        "",
        ["a_key_coming_back_up_does_nothing"],
    ),
    (
        # Whether the selected car can go that way is the move rule's business,
        # but which *axis* the key names is not.
        "an arrow slides a car across its own axis",
        "                if self.vehicle(id).is_none_or(|v| v.orientation != axis) {\n"
        "                    return EventResult::Ignored;\n"
        "                }",
        "",
        ["an_arrow_across_the_cars_axis_does_nothing"],
    ),
    (
        "the right mouse button is treated as the left",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {\n"
        "            return EventResult::Ignored;\n"
        "        }",
        "",
        ["a_right_click_is_left_for_something_else"],
    ),
    (
        "clicking a car does not put it down again",
        "                self.selected = if self.selected == Some(id) {\n"
        "                    None\n"
        "                } else {\n"
        "                    Some(id)\n"
        "                };",
        "                self.selected = Some(id);",
        ["clicking_a_car_picks_it_up_and_clicking_it_again_puts_it_down"],
    ),
    (
        "clicking an unreachable cell leaves the car selected",
        "                let had = self.selected.is_some();\n"
        "                self.selected = None;\n"
        "                had\n"
        "            }\n"
        "        }\n"
        "    }\n"
        "\n"
        "    pub fn handle_mouse",
        "                self.selected.is_some()\n"
        "            }\n"
        "        }\n"
        "    }\n"
        "\n"
        "    pub fn handle_mouse",
        ["clicking_a_cell_a_car_cannot_move_towards_puts_it_down"],
    ),
    (
        "clicking the bare background leaves the car selected",
        "                // Bare background deselects, wherever it is.\n"
        "                let had = self.selected.is_some();\n"
        "                self.selected = None;",
        "                let had = false;",
        ["clicking_the_background_puts_the_car_down"],
    ),
    (
        # The size the frame is drawn at is the size the next click is read
        # against — which is the only reason it is stored at all.
        "rendering does not record the size it drew at",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["drawing_at_a_size_is_what_decides_where_the_next_click_lands"],
    ),
    (
        "a resize is not remembered",
        "        Event::Resize { width, height } => {\n"
        "            game.resize(*width as f32, *height as f32);",
        "        Event::Resize { width, height } => {\n"
        "            let _ = (width, height);",
        ["a_resize_event_changes_the_size_clicks_are_read_against"],
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
        ["the_window_close_request_exits"],
    ),
    (
        "every event asks for a repaint, including the ignored ones",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["only_a_handled_event_asks_for_a_repaint"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "rush", timeout=120))

"""Mutation test for the klotski suite.

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
        # One number decides the whole board.  Solving for it from the width
        # alone stretches a 4x5 grid to fill a band that is not 4:5, and in a
        # wide short window the resulting stack is taller than the band it was
        # centred in, so the board starts above the chrome above it.
        "the cell size is solved from the width alone",
        "        let cell = (band.w / per_w).min(band.h / per_h).max(0.0);",
        "        let cell = (band.w / per_w).max(0.0);",
        [
            "every_band_stays_inside_the_window",
            "the_board_never_overlaps_the_chrome",
        ],
    ),
    (
        "the exit strip sits at the board's left edge",
        "            let exit_x = bx + WIN_COL as f32 * (cell + gap);",
        "            let exit_x = bx;",
        ["the_exit_strip_spans_the_columns_the_big_block_must_reach"],
    ),
    (
        "the exit strip is not below the board",
        "                Rect::new(exit_x, by + grid_h + gap, exit_w, exit_h),",
        "                Rect::new(exit_x, by, exit_w, exit_h),",
        ["the_exit_strip_spans_the_columns_the_big_block_must_reach"],
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
        # Not `BOARD_SHARE = 0.0`: the board's share is asserted as a *lower*
        # bound on the board and an *upper* bound on the chrome, and a smaller
        # reservation cannot violate a lower bound in a window roomy enough to
        # satisfy it anyway.  Removing the reservation from the budget is what
        # actually lets the chrome eat the board.
        "the board's share of the window is not reserved",
        "        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);",
        "        let budget = h;",
        ["the_chrome_never_takes_more_than_its_share_of_the_window"],
    ),
    (
        "the bands are dropped in a different order",
        "const BAND_DROP_ORDER: [usize; 3] = [2, 0, 1];",
        "const BAND_DROP_ORDER: [usize; 3] = [1, 0, 2];",
        ["the_footer_is_the_first_chrome_to_go"],
    ),
    (
        "cell_rect forgets the gaps between cells",
        "            self.board.x + col as f32 * (self.cell + self.gap),",
        "            self.board.x + col as f32 * self.cell,",
        ["the_grid_fills_the_board_rect_exactly"],
    ),
    (
        # The gaps *between* a block's own cells belong to the block once
        # something is sitting on them; a block drawn one cell square leaves
        # its far cells reporting the empty square underneath.
        "a block is drawn one cell square whatever its kind",
        "            kind.cols() as f32 * self.cell + (kind.cols() as f32 - 1.0) * self.gap,\n"
        "            kind.rows() as f32 * self.cell + (kind.rows() as f32 - 1.0) * self.gap,",
        "            self.cell,\n            self.cell,",
        ["a_block_covers_the_cells_it_sits_on"],
    ),
    (
        "the win panel is the whole window",
        "        let w = (self.window.w * 0.82).min(320.0);\n"
        "        let h = (self.window.h * 0.42).min(180.0);",
        "        let w = self.window.w;\n        let h = self.window.h;",
        ["the_win_panel_does_not_cover_the_whole_board"],
    ),
    # ── Hit boxes ───────────────────────────────────────────────────
    (
        # The hit test answers with the *last* target covering the point, so
        # recording the cells after the blocks hands every click on a block to
        # the empty square underneath it.
        "the cells are recorded after the blocks",
        "            f.hit(Target::Block(block.id), r);\n        }\n    }",
        "            f.hit(Target::Block(block.id), r);\n"
        "        }\n"
        "\n"
        "        for row in 0..GRID_ROWS {\n"
        "            for col in 0..GRID_COLS {\n"
        "                f.hit(Target::Cell(row, col), l.cell_rect(row, col));\n"
        "            }\n"
        "        }\n"
        "    }",
        [
            "a_block_covers_the_cells_it_sits_on",
            "every_block_is_clickable_where_it_is_drawn",
        ],
    ),
    (
        "blocks record no hit box",
        "            f.hit(Target::Block(block.id), r);\n",
        "",
        [
            "a_block_covers_the_cells_it_sits_on",
            "every_block_is_clickable_where_it_is_drawn",
            "clicking_a_block_selects_it_and_clicking_it_again_puts_it_down",
        ],
    ),
    (
        # The strip is a picture of where the big block must reach, not a
        # control.  A hit box on it swallows a click and does nothing, which is
        # worse than no hit box: the click it ate would have reached the cell.
        "the exit strip records a target",
        "        fill(f, l.exit, MAUVE, CornerRadii::all(l.gap.max(1.0)));",
        "        fill(f, l.exit, MAUVE, CornerRadii::all(l.gap.max(1.0)));\n"
        "        f.hit(Target::Cell(GRID_ROWS, WIN_COL), l.exit);",
        ["the_exit_strip_records_no_target"],
    ),
    (
        # The anchor has to carry the comment above it: `f.hit(target, r);`
        # also ends the win panel's button loop.
        "the control buttons record no hit box",
        "            // Recorded even when it is drawn dim: `undo` on an empty stack\n"
        "            // answers `false` and changes nothing, and a target that reports\n"
        "            // \"nothing happened\" is the thing the tests can hold on to.\n"
        "            f.hit(target, r);",
        "",
        [
            "every_control_is_reachable_by_pointer",
            "every_button_does_something",
        ],
    ),
    (
        "the win panel records no way on",
        # Anchored on the label call above it: `f.hit(target, r);` on its own
        # also ends `draw_controls`' loop, and an anchor that matches twice
        # patches two sites, so the row stops saying what its name says.
        "                r,\n"
        "            );\n"
        "            f.hit(target, r);",
        "                r,\n"
        "            );",
        ["the_win_overlay_offers_a_way_on"],
    ),
    (
        # A modal that only *looks* in front is one whose buttons you can press
        # through.
        "the win overlay leaves the board clickable behind it",
        "        f.discard_hits();\n",
        "",
        ["the_win_overlay_takes_the_board_out_of_reach"],
    ),
    (
        "the win scrim is opaque",
        "const SCRIM: Color = Color::rgba(0x11, 0x11, 0x1B, 0xB4);",
        "const SCRIM: Color = Color::rgba(0x11, 0x11, 0x1B, 0xFF);",
        ["the_win_scrim_is_translucent"],
    ),
    (
        "the footer clip is never lifted",
        "        f.unclip();\n",
        "",
        ["the_frame_is_balanced"],
    ),
    # ── Text placement ──────────────────────────────────────────────
    (
        # The version this replaced drew both header numbers at
        # `total_width - PADDING - 100.0` — a guess at how wide "Moves: 1234"
        # would turn out to be.
        # Not `the_move_counter_moves_left_as_the_number_grows`, though it
        # looks like the test for exactly this: `room` is `right - split` and
        # `split` is itself measured from the counters, so a counter given a
        # constant width is still clamped to a `room` that moved with the
        # number.  It slides left for the wrong reason and that test cannot
        # tell.  The right-hand edge can.
        "the right-aligned text guesses its own width",
        "    let w = text::measure(l.text, l.size, l.weight).min(room);",
        "    let w = 100.0_f32.min(room);",
        ["the_move_counter_is_right_aligned_from_its_own_width"],
    ),
    (
        "the header numbers are flush with the window edge",
        "        let right = l.header.right() - l.pad;",
        "        let right = l.header.right();",
        ["the_move_counter_is_right_aligned_from_its_own_width"],
    ),
    (
        # The width that decides the centre must be the width the renderer is
        # told to stop at.  The version this replaced took the limit as a
        # separate argument, used it to pick the centre, and then drew with
        # `max_width: None`.
        "a centred label is given no width limit",
        "    push_text(f, l, x, y, r.right() - x);",
        "    push_text(f, l, x, y, f32::MAX);",
        ["a_centred_label_is_stopped_at_the_right_hand_edge_of_its_box"],
    ),
    (
        # Not `the_block_label_stays_inside_the_block`: starting at the box's
        # right-hand edge leaves `r.right() - x == 0`, `push_text` refuses, and
        # a label that was never drawn is inside every box there is.  What dies
        # is the centring's own test and the pass-draws-something converse.
        "a centred label starts at the right-hand edge of its box",
        "    let x = r.x + (r.w - w) / 2.0;",
        "    let x = r.x + r.w;",
        [
            "a_centred_label_is_stopped_at_the_right_hand_edge_of_its_box",
            "a_pass_with_room_paints_and_a_pass_with_none_paints_nothing",
        ],
    ),
    (
        # Not the containment test.  A footer told to stack two lines in a
        # band with room for one does not spill: `centre_line` refuses the
        # taller stack and the footer draws nothing at all, which every
        # containment test in the file is happy with.
        "the footer draws both lines however short it is",
        "        let shown = if lh * 2.0 <= l.footer.h { 2 } else { 1 };",
        "        let shown = 2;",
        ["a_band_tall_enough_for_a_line_draws_one"],
    ),
    (
        "the header draws two lines however short it is",
        "        let two_lines = title_h + sub_h <= l.header.h;",
        "        let two_lines = true;",
        ["a_band_tall_enough_for_a_line_draws_one"],
    ),
    # ── Identity ────────────────────────────────────────────────────
    (
        # The bug this campaign found: an undo entry holding a `Vec` index in a
        # field called `block_id`, spent as `self.blocks[entry.block_id]`.
        "an undo entry holds the block's position instead of its id",
        "            block: id,",
        "            block: idx,",
        [
            "undo_moves_the_block_the_move_moved",
            "undo_unwinds_a_run_of_moves_in_order",
        ],
    ),
    (
        "undo spends the entry's id as a position",
        "        let Some(idx) = self.index_of(entry.block) else {\n"
        "            return false;\n"
        "        };",
        "        let idx = entry.block;",
        [
            "undo_moves_the_block_the_move_moved",
            "undo_unwinds_a_run_of_moves_in_order",
        ],
    ),
    (
        # Ids start at 1 precisely so that `id != index` for every block the
        # program ever makes, which is what turns "confuses the two" into a
        # first-move failure rather than a latent one.
        "ids start at zero, so the first id is also the first index",
        "            next_id: 1,",
        "            next_id: 0,",
        ["no_block_id_equals_its_position_in_the_vector"],
    ),
    (
        "the id counter restarts with every puzzle",
        "        self.blocks.clear();\n        if let Some(puzzle) = PUZZLES.get(idx) {",
        "        self.blocks.clear();\n"
        "        self.next_id = 1;\n"
        "        if let Some(puzzle) = PUZZLES.get(idx) {",
        ["ids_are_never_reused_across_puzzle_loads"],
    ),
    # ── Rules ───────────────────────────────────────────────────────
    (
        "the big block wins anywhere past the exit",
        "            .any(|b| b.kind == BlockKind::Big && b.row == WIN_ROW && b.col == WIN_COL)",
        "            .any(|b| b.kind == BlockKind::Big && b.row >= WIN_ROW && b.col >= WIN_COL)",
        ["the_big_block_wins_only_where_the_exit_is"],
    ),
    (
        "any block on the exit wins",
        "            .any(|b| b.kind == BlockKind::Big && b.row == WIN_ROW && b.col == WIN_COL)",
        "            .any(|b| b.row == WIN_ROW && b.col == WIN_COL)",
        ["only_the_big_block_wins"],
    ),
    (
        "a won board goes on playing",
        "        if self.is_won() || !self.can_move(id, dir) {",
        "        if !self.can_move(id, dir) {",
        ["a_won_board_refuses_further_moves"],
    ),
    (
        # The version this replaced opened `undo` with
        # `if self.status == Won { return; }`, so the winning move was the one
        # move you could not take back.
        "undo is refused once the puzzle is solved",
        "    pub fn undo(&mut self) -> bool {\n"
        "        let Some(entry) = self.undo_stack.pop_back() else {",
        "    pub fn undo(&mut self) -> bool {\n"
        "        if self.is_won() {\n"
        "            return false;\n"
        "        }\n"
        "        let Some(entry) = self.undo_stack.pop_back() else {",
        ["undoing_the_winning_move_un_wins"],
    ),
    (
        "the undo stack has no cap",
        "        if self.undo_stack.len() >= MAX_UNDO {\n"
        "            self.undo_stack.pop_front();\n"
        "        }\n",
        "",
        ["the_undo_stack_stops_at_its_cap"],
    ),
    (
        "the undo cap drops the newest move instead of the oldest",
        "            self.undo_stack.pop_front();",
        "            self.undo_stack.pop_back();",
        ["the_undo_cap_drops_the_oldest_move_not_the_newest"],
    ),
    (
        # `shifted` refuses Up and Left on its own (they fail in
        # `checked_add_signed`); Down and Right are refused only here.
        "a block may be moved off the far edge of the grid",
        "        if !block.can_fit_in_grid(dir) {\n            return false;\n        }\n",
        "",
        ["a_block_cannot_move_off_the_grid"],
    ),
    (
        "a block collides with the cells it is itself vacating",
        "                if let Some(Some(other)) = occupant\n"
        "                    && other != id\n"
        "                {",
        "                if let Some(Some(_other)) = occupant {",
        ["a_block_may_move_into_the_cell_it_is_vacating"],
    ),
    (
        # Not a mutation of a modulo: the version this replaced took `idx + 1`
        # modulo the length *and* fell back to the first block when `get`
        # answered `None`, which are two spellings of one wrap. Taking the
        # modulo out changed nothing, and the sweep said so by surviving.
        "the selection skips every other block",
        "                .get(idx.saturating_add(1))",
        "                .get(idx.saturating_add(2))",
        ["enter_cycles_the_selection_through_every_block"],
    ),
    (
        "the selection past the last block is a block that does not exist",
        "                .map_or(first, |b| b.id),",
        "                .map_or(0, |b| b.id),",
        ["enter_cycles_the_selection_through_every_block"],
    ),
    (
        "prev does not wrap round to the last puzzle",
        "        let prev = if self.current_puzzle == 0 {\n"
        "            PUZZLES.len().saturating_sub(1)\n"
        "        } else {\n"
        "            self.current_puzzle.saturating_sub(1)\n"
        "        };",
        "        let prev = self.current_puzzle.saturating_sub(1);",
        ["the_next_and_prev_buttons_walk_the_puzzles_and_wrap"],
    ),
    (
        "restart keeps the undo stack of the game it threw away",
        "        self.blocks.clone_from(&self.initial_blocks);\n"
        "        self.selected = None;\n"
        "        self.moves = 0;\n"
        "        self.undo_stack.clear();",
        "        self.blocks.clone_from(&self.initial_blocks);\n"
        "        self.selected = None;\n"
        "        self.moves = 0;",
        ["the_restart_button_returns_the_opening_position"],
    ),
    (
        # The anchor runs to the doc comment of the next function: `position`
        # ends with the same four lines, on purpose, and a mutation that hit
        # both would be testing the test helper too.
        "a new puzzle inherits the old one's move count",
        "        self.selected = None;\n"
        "        self.moves = 0;\n"
        "        self.undo_stack.clear();\n"
        "    }\n"
        "\n"
        "    /// Build an arbitrary position",
        "        self.selected = None;\n"
        "        self.undo_stack.clear();\n"
        "    }\n"
        "\n"
        "    /// Build an arbitrary position",
        ["changing_puzzle_clears_the_move_count_and_the_undo_stack"],
    ),
    # ── Pointer ─────────────────────────────────────────────────────
    (
        "clicking the selected block does not put it down",
        "                self.selected = if self.selected == Some(id) {\n"
        "                    None\n"
        "                } else {\n"
        "                    Some(id)\n"
        "                };",
        "                self.selected = Some(id);",
        ["clicking_a_block_selects_it_and_clicking_it_again_puts_it_down"],
    ),
    (
        # The version this replaced deselected on an empty *cell* but left the
        # selection alone on a click outside the grid — two answers to one
        # question.
        "bare background leaves the selection alone",
        "                let had = self.selected.is_some();\n"
        "                self.selected = None;\n"
        "                if had {\n"
        "                    EventResult::Consumed\n"
        "                } else {\n"
        "                    EventResult::Ignored\n"
        "                }",
        "                EventResult::Ignored",
        ["clicking_bare_background_puts_the_block_down"],
    ),
    (
        "a click is read against a fixed window size",
        "        let (w, h) = self.size_drawn;",
        "        let (w, h) = (WINDOW_WIDTH, WINDOW_HEIGHT);",
        [
            "a_click_reads_against_the_size_the_frame_was_drawn_at",
            "a_resize_is_remembered_and_read_by_the_next_click",
        ],
    ),
    (
        "a button release is a second press",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {\n"
        "            return EventResult::Ignored;\n"
        "        }\n",
        "",
        ["a_release_is_not_a_press"],
    ),
    (
        "the undo button is never dimmed",
        "                if live { SURFACE1 } else { SURFACE0 },",
        "                SURFACE1,",
        ["the_undo_button_is_dimmed_when_there_is_nothing_to_undo"],
    ),
    # ── Keyboard ────────────────────────────────────────────────────
    (
        "a key release is a second press",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }\n",
        "",
        ["a_key_release_does_nothing"],
    ),
    (
        "a modifier the program does not use is answered",
        "        let plain = ev.modifiers == guitk::event::Modifiers::NONE;",
        "        let plain = true;",
        ["a_modified_letter_is_not_a_shortcut"],
    ),
    (
        # A key the program has no use for must be handed back, not swallowed:
        # the window above it may have a use for it, and a swallowed key also
        # costs a redraw that draws the same picture again.
        "an unbound key is swallowed",
        "            _ => return EventResult::Ignored,",
        "            _ => {}",
        [
            "an_unbound_key_is_ignored_rather_than_swallowed",
            "a_handled_event_asks_for_a_redraw_and_an_ignored_one_does_not",
        ],
    ),
    (
        "an arrow with nothing selected is swallowed",
        "                    None => return EventResult::Ignored,",
        "                    None => {}",
        ["an_arrow_with_nothing_selected_is_ignored"],
    ),
    # ── Window plumbing ─────────────────────────────────────────────
    (
        "the frame does not record the size it drew at",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["rendering_records_the_size_it_drew_at"],
    ),
    (
        "a resize is not remembered",
        "        Event::Resize { width, height } => {\n"
        "            game.resize(*width as f32, *height as f32);\n"
        "            EventResult::Consumed\n"
        "        }\n",
        "",
        ["a_resize_is_remembered_and_read_by_the_next_click"],
    ),
    (
        # If these drift apart, every test in the file checks a window the
        # program never actually opens.
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
    # ── C-CENTRING-IS-NOT-A-BOUND (lesson 109) ──────────────────────
    (
        # The whole campaign in one row: centring without asking whether the
        # band has the room is an arrangement, not a bound.
        "centre_line never refuses a band",
        "    (!band.is_empty() && band.h >= height).then(|| band.y + (band.h - height) / 2.0)",
        "    Some(band.y + (band.h - height) / 2.0)",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "centre_line always refuses",
        "    (!band.is_empty() && band.h >= height).then(|| band.y + (band.h - height) / 2.0)",
        "    None",
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
    (
        "a band with no width is still a band",
        "    (!band.is_empty() && band.h >= height).then(|| band.y + (band.h - height) / 2.0)",
        "    (band.h >= height).then(|| band.y + (band.h - height) / 2.0)",
        ["centre_line_refuses_a_band_it_cannot_fill_rather_than_going_negative"],
    ),
    (
        "a band one point short is close enough",
        "    (!band.is_empty() && band.h >= height).then(|| band.y + (band.h - height) / 2.0)",
        "    (!band.is_empty() && band.h + 1.0 >= height).then(|| band.y + (band.h - height) / 2.0)",
        ["centre_line_refuses_a_band_it_cannot_fill_rather_than_going_negative"],
    ),
    (
        # A run told it may fill nought points is a run the renderer is asked to
        # ellipsise into nothing. Containment cannot see it -- an empty box is
        # inside everything -- so it needs a test of its own.
        "a run is pushed into a box with no room",
        "    if l.text.is_empty() || limit <= 0.0 {",
        "    if l.text.is_empty() {",
        ["no_run_is_pushed_into_a_box_with_no_room"],
    ),
    (
        "the renderer is never told where to stop",
        "        max_width: Some(limit),",
        "        max_width: None,",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # Lesson 8: a run's width limit is measured from where the run starts,
        # not from the width of the band the caller had in mind.
        # Anchored on the signature, because `label_right` ends in the same
        # statement and an anchor that matches twice patches two sites.
        "a labels limit is measured from the band, not from where it starts",
        "fn label(f: &mut Frame<Target>, l: &Label, x: f32, y: f32, right: f32) {\n"
        "    push_text(f, l, x, y, right - x);",
        "fn label(f: &mut Frame<Target>, l: &Label, x: f32, y: f32, right: f32) {\n"
        "    push_text(f, l, x, y, right);",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # A run right-aligned from an unclamped width starts at
        # `right - measured`, which on a band narrower than the string is left
        # of the band's own edge -- so this escapes the header outright rather
        # than merely crossing the split.
        "a right-aligned label has no left bound",
        "    let w = text::measure(l.text, l.size, l.weight).min(room);",
        "    let w = text::measure(l.text, l.size, l.weight);",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "a centred label centres without asking",
        "    let Some(y) = centre_line(r, lh) else {\n"
        "        return;\n"
        "    };",
        "    let y = r.y + (r.h - lh) / 2.0;",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # The fault this row reproduces was live: `label_centred` insets the run
        # by half the slack and then handed the renderer the *box's* width as the
        # limit, so a run may end half the slack past the box's right edge.  On a
        # 120-point window the "Next" label was allowed to reach 123.5 in a band
        # 120 wide.  A limit is a distance from where the run starts.
        "a centred labels limit is measured from the box, not from where it starts",
        "    push_text(f, l, x, y, r.right() - x);",
        "    push_text(f, l, x, y, r.w);",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # The header stacks one or two lines and then centres the stack. Asking
        # about nothing at all is the shape the guard had before: present,
        # unreachable, and worth no coverage.
        "the header asks whether its band can hold nothing",
        "        let Some(top) = centre_line(l.header, stack) else {",
        "        let Some(top) = centre_line(l.header, 0.0) else {",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the header centres one line when it drew two",
        "        let stack = if two_lines { title_h + sub_h } else { title_h };",
        "        let stack = title_h;",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # Two columns in one band: each staying inside the band is not the two
        # of them staying off each other.
        # The column test cannot catch this one, and the reason is the fix:
        # `split` is the title's right bound *and* the counters' left bound, so
        # a split at the band's far edge leaves the counters nought points of
        # room, `push_text` refuses them, and two columns of which one is not
        # drawn do not overlap.  What dies is every test that expects a counter.
        "the header title runs to the far edge of the band",
        "        let split = (right - counters - l.pad).max(left);",
        "        let split = right;",
        [
            "a_pass_with_room_paints_and_a_pass_with_none_paints_nothing",
            "the_move_counter_is_right_aligned_from_its_own_width",
            "the_move_counter_moves_left_as_the_number_grows",
        ],
    ),
    (
        "the header title is given no column at all",
        "        let split = (right - counters - l.pad).max(left);",
        "        let split = (right - counters - l.pad).min(left);",
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
    (
        # Splitting on the *narrower* counter puts the split too far right,
        # and the wider counter is then clamped to the room left over instead
        # of spilling across the title -- so it stops short of the margin, and
        # the right-edge test is the one that says so.
        "the split is measured from the narrower counter",
        "        let counters = text::measure(&moves, l.font, FontWeightHint::Regular).max(if two_lines {",
        "        let counters = text::measure(&moves, l.font, FontWeightHint::Regular).min(if two_lines {",
        ["the_move_counter_is_right_aligned_from_its_own_width"],
    ),
    (
        "the footer centres its stack as though it were one line",
        "        let Some(top) = centre_line(l.footer, lh * shown as f32) else {",
        "        let Some(top) = centre_line(l.footer, lh) else {",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the footer lines run one padding past their band",
        "                l.footer.right() - l.pad,",
        "                l.footer.right() + l.pad,",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the victory card centres its title and forgets the rest",
        "        let Some(top) = centre_line(panel, stack) else {",
        "        let Some(top) = centre_line(panel, title_h) else {",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the victory card does not ask at all",
        "        let Some(top) = centre_line(panel, stack) else {\n"
        "            return;\n"
        "        };",
        "        let top = panel.y + (panel.h - stack) / 2.0;",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # The solve sizes the mat, not the grid. Sizing the grid and painting a
        # mat around it is how the mat ends up outside the band -- and on any
        # window wider than it is tall the vertical shortfall is nought, so
        # there is nowhere for the extra gap to go.
        # Not `no_pass_paints_outside_the_region_it_owns`, and the reason is
        # worth the paragraph: `board_frame` is *derived from the solve*, so a
        # solve that oversizes the mat oversizes the region the pass is checked
        # against by exactly as much.  The pass stays inside its own region no
        # matter how wrong that region is.  What the mutation moves is the mat
        # relative to the *band*, and the tests that hold a band are the ones
        # that can see it.
        "the solve does not reserve the mats ring above and below",
        "            + GAP_PER_CELL * 2.0",
        "            + GAP_PER_CELL",
        ["the_board_never_overlaps_the_chrome"],
    ),
    (
        "the solve does not reserve the mats ring left and right",
        "        let per_w = GRID_COLS as f32 + (GRID_COLS as f32 + 1.0) * GAP_PER_CELL;",
        "        let per_w = GRID_COLS as f32 + (GRID_COLS as f32 - 1.0) * GAP_PER_CELL;",
        ["every_band_stays_inside_the_window"],
    ),
    (
        "the grid is centred and the mat is drawn around it",
        "            let fy = band.y + (band.h - frame_h) / 2.0;",
        "            let fy = band.y + (band.h - stack_h) / 2.0;",
        ["the_board_never_overlaps_the_chrome"],
    ),
    (
        # The floor that kept a selection visible on a tiny board kept it
        # visible by drawing it outside the board.
        "the selection halo has a floor that outgrows the ring",
        "                let grow = l.gap;",
        "                let grow = (l.gap * 0.8).max(1.0);",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the mat is painted over the whole window",
        "        fill(f, l.board_frame, CRUST, CornerRadii::all(l.gap.max(1.0)));",
        "        fill(f, l.window, CRUST, CornerRadii::all(l.gap.max(1.0)));",
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        # Lesson 11: `Frame::clip` pushes a command whether or not the rectangle
        # has area, so a pass that clips before it has refused its band cannot
        # be silent about a band it never had.
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
    (
        "a rectangle with no area is still filled",
        "    if r.is_empty() {\n"
        "        return;\n"
        "    }\n"
        "    f.push(RenderCommand::FillRect {",
        "    f.push(RenderCommand::FillRect {",
        ["a_pass_with_room_paints_and_a_pass_with_none_paints_nothing"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "klotski", timeout=120))

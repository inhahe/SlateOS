"""Mutation test for the mahjong suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The old suite had nothing to say about any of this.  `main` was
`let _app = Mahjong::new();` -- the game had never been on a screen -- so the
geometry, the paint order, the hit boxes and the whole keyboard were untested
by construction, and the four faults named in the commit that wired it up
(a 172-position "144-tile" turtle silently truncated to a capless blob, two
layer-1 tiles resting on empty squares, a legend fits-test whose width half
could not fail, and a bounding box that reserved four layer-offsets on a side
no tile reaches) had all survived it.
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
    # `Layout::solve` is the whole of the geometry.  Every fixed number the
    # old file used to carry stood where one of these expressions does, so
    # each of these mutations is a faithful restoration of what it said.
    (
        "the header is a fixed height again, so the counters spill out of it",
        "        let header_h = (title + status + pad * 3.0).min(h);",
        "        let header_h = 80.0_f32.min(h);",
        ["the_header_is_sized_from_the_two_fonts_it_stacks"],
    ),
    (
        "the header forgets one of the two lines it stacks",
        "        let header_h = (title + status + pad * 3.0).min(h);",
        "        let header_h = (title + pad * 3.0).min(h);",
        ["the_header_is_sized_from_the_two_fonts_it_stacks"],
    ),
    (
        "the header is sized from a share of the height rather than its fonts",
        "        let header_h = (title + status + pad * 3.0).min(h);",
        "        let header_h = (h * 0.11).min(h);",
        ["the_header_grows_with_the_font_it_holds"],
    ),
    (
        "the header may be taller than the window it is drawn in",
        "        let header_h = (title + status + pad * 3.0).min(h);",
        "        let header_h = title + status + pad * 3.0;",
        # Not the three-bands test: it checks that they stack, and a header
        # taller than the window still stacks. Only a window shorter than the
        # header's floor can show it, and until this sweep the fixture had none.
        ["a_window_smaller_than_its_own_furniture_still_solves_a_sane_layout"],
    ),
    (
        "the help bar is allowed to eat into the header",
        "        let help_h = (small + pad * 2.0).min((h - header_h).max(0.0));",
        "        let help_h = small + pad * 2.0;",
        # Only the squeezed window: at every size in `SIZES` the middle band is
        # roomy enough that the cap the mutation removes was never the binding
        # term, so the three-bands test agrees with the mutant.
        ["a_window_smaller_than_its_own_furniture_still_solves_a_sane_layout"],
    ),
    (
        # The fault the squeezed fixture found. `inset` clamped the size to
        # zero but moved the origin in by the full padding regardless, so on a
        # 200x20 window -- where the middle band is zero-tall -- the padded
        # board came back starting below the window's bottom edge, and the
        # whole turtle was solved from it and drawn off-screen.
        "an inset pushes its origin past the rect it came from",
        "    let dx = by.clamp(0.0, r.w / 2.0);\n    let dy = by.clamp(0.0, r.h / 2.0);\n"
        "    Rect::new(r.x + dx, r.y + dy, r.w - dx * 2.0, r.h - dy * 2.0)",
        "    Rect::new(\n"
        "        r.x + by,\n"
        "        r.y + by,\n"
        "        (r.w - by * 2.0).max(0.0),\n"
        "        (r.h - by * 2.0).max(0.0),\n"
        "    )",
        [
            "an_inset_never_leaves_the_rect_it_came_from",
            "a_window_smaller_than_its_own_furniture_still_solves_a_sane_layout",
        ],
    ),
    (
        # The same fault with the size left clamped, which is why it needs its
        # own row: reverting only the origin leaves a *negative* height, and
        # centring the turtle in a negative height moves it back up by exactly
        # the two pixels the origin moved it down. The layout cannot see this
        # one at all; only the helper's own test can.
        "an inset moves its origin in without checking there is room",
        "    let dx = by.clamp(0.0, r.w / 2.0);\n    let dy = by.clamp(0.0, r.h / 2.0);",
        "    let dx = by;\n    let dy = by;",
        ["an_inset_never_leaves_the_rect_it_came_from"],
    ),
    (
        "the help bar is a fixed height, so its text spills at a large font",
        "        let help_h = (small + pad * 2.0).min((h - header_h).max(0.0));",
        "        let help_h = 30.0_f32.min((h - header_h).max(0.0));",
        ["the_help_bar_is_painted_before_its_text_and_covers_the_bottom_strip"],
    ),
    (
        "the padding is taken from the width alone, so a short window loses its height to it",
        "        let pad = (w.min(h) * 0.015).clamp(2.0, 14.0).min(w.min(h) / 2.0);",
        "        let pad = (w * 0.015).clamp(2.0, 14.0).min(w.min(h) / 2.0);",
        ["the_padding_is_taken_from_the_shorter_side"],
    ),
    (
        "the padding has no ceiling, so a huge window is mostly margin",
        "        let pad = (w.min(h) * 0.015).clamp(2.0, 14.0).min(w.min(h) / 2.0);",
        "        let pad = (w.min(h) * 0.015).max(2.0).min(w.min(h) / 2.0);",
        # The fonts test is about the fonts; the padding's own ceiling belongs
        # with the padding's own test, which until this sweep only compared two
        # windows and so was satisfied by any padding that grew at all.
        ["the_padding_is_taken_from_the_shorter_side"],
    ),
    (
        "the title font has no ceiling",
        "        let title = (h * 0.031).clamp(10.0, 26.0);",
        "        let title = (h * 0.031).max(10.0);",
        ["the_fonts_grow_with_the_window_and_stop_at_both_ends"],
    ),
    (
        "the title font has no floor, so a short window draws it at nothing",
        "        let title = (h * 0.031).clamp(10.0, 26.0);",
        "        let title = (h * 0.031).min(26.0);",
        ["the_fonts_grow_with_the_window_and_stop_at_both_ends"],
    ),
    (
        "the status font has no ceiling",
        "        let status = (h * 0.023).clamp(8.0, 18.0);",
        "        let status = (h * 0.023).max(8.0);",
        ["the_fonts_grow_with_the_window_and_stop_at_both_ends"],
    ),
    (
        "the small font is capped at a constant rather than at the line above it",
        "        let small = (h * 0.017).clamp(7.0, status);",
        # Not `18.0`, which was the first try and is `status`'s own ceiling --
        # so the mutant computed the same number as the program at every
        # height and survived by being equivalent rather than by being missed.
        "        let small = (h * 0.017).clamp(7.0, 26.0);",
        ["the_small_font_never_outgrows_the_font_above_it"],
    ),
    (
        "the fonts are all fixed again",
        "        let title = (h * 0.031).clamp(10.0, 26.0);",
        "        let title = 24.0;",
        ["the_fonts_grow_with_the_window_and_stop_at_both_ends"],
    ),
    # -- The legend ----------------------------------------------------
    (
        "the legend is squeezed to a third of the window instead of being dropped",
        "        let legend_fits = needed <= w / 3.0 && middle.h >= small * LEGEND_ROWS as f32;",
        "        let legend_fits = middle.h >= small * LEGEND_ROWS as f32;",
        ["the_legend_never_takes_more_than_a_third_of_the_window"],
    ),
    (
        "the legend is drawn however short the window is",
        "        let legend_fits = needed <= w / 3.0 && middle.h >= small * LEGEND_ROWS as f32;",
        "        let legend_fits = needed <= w / 3.0;",
        ["a_window_too_short_for_the_legends_rows_drops_it_too"],
    ),
    (
        "the legend is never drawn at all",
        "        let legend_fits = needed <= w / 3.0 && middle.h >= small * LEGEND_ROWS as f32;",
        "        let legend_fits = false;",
        [
            "a_window_with_room_for_the_legend_gets_one",
            "the_legend_draws_one_row_for_each_group_and_a_note_for_the_asterisks",
        ],
    ),
    (
        "the legend is a fixed width rather than the width its rows measure",
        "        let needed = legend_width(small, pad);",
        "        let needed = 180.0;",
        ["the_legend_takes_the_width_its_widest_row_measures"],
    ),
    (
        "the legend's width forgets the font it is drawn at",
        "    let widest = LEGEND_ITEMS",
        "    let font = 12.0;\n    let widest = LEGEND_ITEMS",
        ["the_legend_column_widens_with_the_font_it_is_drawn_at"],
    ),
    (
        "the legend takes its width off the board even when it is dropped",
        "            Rect::new(middle.right(), middle.y, 0.0, middle.h)",
        "            Rect::new(middle.right() - needed, middle.y, needed, middle.h)",
        # Not `the_board_and_the_legend_divide_the_middle_between_them`, which
        # this sweep showed agrees with the mutant: a dropped legend that still
        # takes `needed` off the board still starts where the board ends and
        # still reaches the right edge, so the division it checks is intact.
        # Dividing the middle correctly and dropping the legend are two claims.
        [
            "a_window_too_narrow_for_the_legend_drops_it_rather_than_squeezing_it",
            "a_dropped_legend_draws_nothing_and_records_nothing",
        ],
    ),
    (
        "the board takes the whole middle, so the legend is painted over the tiles",
        "        let board = Rect::new(middle.x, middle.y, (legend.x - middle.x).max(0.0), middle.h);",
        "        let board = middle;",
        ["the_board_and_the_legend_divide_the_middle_between_them"],
    ),
    # -- The turtle ----------------------------------------------------
    (
        "the tile is a fixed size again, so it overflows a small window",
        "            (inner.w / span_w).min(inner.h / span_h).max(0.0)",
        "            42.0",
        ["every_tile_lands_inside_the_board_band"],
    ),
    (
        "the tile is sized by the width alone, so a short window overflows",
        "            (inner.w / span_w).min(inner.h / span_h).max(0.0)",
        "            (inner.w / span_w).max(0.0)",
        [
            "every_tile_lands_inside_the_board_band",
            "the_binding_side_is_whichever_runs_out_first",
        ],
    ),
    (
        "the tile is sized by the height alone, so a narrow window overflows",
        "            (inner.w / span_w).min(inner.h / span_h).max(0.0)",
        "            (inner.h / span_h).max(0.0)",
        [
            "every_tile_lands_inside_the_board_band",
            "the_binding_side_is_whichever_runs_out_first",
        ],
    ),
    (
        # Was "a window with no room left draws tiles of a negative size",
        # deleting a `.max(0.0)` on the ratio. It could not be made to bind:
        # `inset` returns no negative side, so neither ratio is ever negative.
        # The floor is gone from the program rather than left as a guard no
        # test can reach. What is worth mutating here is the choice of the
        # *smaller* ratio, which is what keeps the turtle inside the board.
        "the tile is solved from whichever side has more room, so the turtle overflows",
        "            (inner.w / span_w).min(inner.h / span_h)",
        "            (inner.w / span_w).max(inner.h / span_h)",
        ["the_turtle_fits_inside_the_board_at_every_size"],
    ),
    (
        "the tile is square, so its face is not a mahjong tile",
        "        let tile_h = tile_w * TILE_ASPECT;",
        "        let tile_h = tile_w;",
        ["a_tile_keeps_its_shape_at_every_size"],
    ),
    (
        "the turtle is pinned to the left with all the slack on one side",
        "            inner.x + (inner.w - tile_w * span_w) / 2.0,",
        "            inner.x,",
        ["the_turtle_is_centred_in_the_space_left_for_it"],
    ),
    (
        "the turtle is pinned to the top",
        "            inner.y + (inner.h - tile_w * span_h) / 2.0,",
        "            inner.y,",
        ["the_turtle_is_centred_in_the_space_left_for_it"],
    ),
    (
        "the turtle's box is the old formula, which reserves layers no tile reaches",
        "        let (_, _, span_w, span_h) = turtle_extent();",
        "        let (span_w, span_h) = (\n"
        "            16.0 * (1.0 + TILE_GAP_SHARE),\n"
        "            8.0 * (TILE_ASPECT + TILE_GAP_SHARE),\n"
        "        );",
        ["the_turtle_box_is_exactly_the_tiles_it_contains"],
    ),
    (
        "the tiles are not shifted back to the turtle's own left edge",
        "            self.turtle.x + (x - min_x) * self.tile_w,",
        "            self.turtle.x + x * self.tile_w,",
        ["the_turtle_box_is_exactly_the_tiles_it_contains"],
    ),
    (
        # Not `self.turtle.y + y * self.tile_w`, which was the first try: the
        # topmost tile in this deal is layer 0 row 0, whose offset is zero, so
        # `min_y` is zero and deleting the subtraction changes nothing. It
        # survived by being equivalent. Subtracting the *other* axis's minimum
        # is the same mistake with a number that is not zero.
        "the tiles are shifted back by the wrong axis's minimum",
        "            self.turtle.y + (y - min_y) * self.tile_w,",
        "            self.turtle.y + (y - min_x) * self.tile_w,",
        ["the_turtle_box_is_exactly_the_tiles_it_contains"],
    ),
    (
        "a row is measured in tile widths, so the rows overlap",
        "        pos.row as f32 * (TILE_ASPECT + TILE_GAP_SHARE) - off,",
        "        pos.row as f32 * (1.0 + TILE_GAP_SHARE) - off,",
        ["neighbouring_tiles_on_one_layer_are_separated_by_a_gap"],
    ),
    (
        "the columns are packed edge to edge with no gap between them",
        "        pos.col as f32 * (1.0 + TILE_GAP_SHARE) - off,",
        "        pos.col as f32 - off,",
        ["neighbouring_tiles_on_one_layer_are_separated_by_a_gap"],
    ),
    (
        "a higher layer sits exactly on the one below, so the stack is invisible",
        "    let off = pos.layer as f32 * LAYER_OFFSET_SHARE;",
        "    let off = 0.0;",
        ["a_higher_layer_sits_up_and_to_the_left_of_the_one_below"],
    ),
    (
        "a higher layer sits down and to the right instead of up and to the left",
        "    let off = pos.layer as f32 * LAYER_OFFSET_SHARE;",
        "    let off = -(pos.layer as f32) * LAYER_OFFSET_SHARE;",
        ["a_higher_layer_sits_up_and_to_the_left_of_the_one_below"],
    ),
    (
        "the turtle loses its cap, which is the fault the wiring found",
        "    // The cap.\n    push(4, 3, 6..=6);",
        "",
        ["the_turtle_holds_the_whole_deal_with_nothing_stacked_on_a_shared_square"],
    ),
    (
        "two tiles are dealt onto the same square",
        "    push(0, 4, 14..=15);",
        "    push(0, 4, 14..=14);\n    push(0, 4, 14..=14);",
        ["the_turtle_holds_the_whole_deal_with_nothing_stacked_on_a_shared_square"],
    ),
    (
        "layer 1 overhangs layer 0 again, so two tiles float on empty squares",
        "    for row in 1..=6 {\n        push(1, row, 4..=9);\n    }",
        "    for row in 1..=6 {\n        push(1, row, 3..=8);\n    }",
        ["every_stacked_tile_rests_on_one_that_is_actually_there"],
    ),
    (
        "layer 2 is as wide as layer 1, so the turtle is a tower rather than a pyramid",
        "    for row in 2..=5 {\n        push(2, row, 5..=8);\n    }",
        "    for row in 2..=5 {\n        push(2, row, 4..=9);\n    }",
        [
            "the_turtle_is_a_pyramid_narrowing_towards_the_top",
            "the_turtle_holds_the_whole_deal_with_nothing_stacked_on_a_shared_square",
        ],
    ),
    # -- The rules -----------------------------------------------------
    #
    # These were reachable before the rewrite -- `Board` did not need a window
    # to be tested -- but the old suite tested none of them either.
    (
        "a tile under another one is free anyway",
        "                if other.pos.row == pos.row && other.pos.col == pos.col {\n"
        "                    return false;\n"
        "                }",
        "",
        ["a_tile_with_another_on_top_of_it_is_not_free"],
    ),
    (
        "a tile is blocked by a neighbour on any layer, not only its own",
        "            if other.pos.layer == pos.layer && other.pos.row == pos.row {",
        "            if other.pos.row == pos.row {",
        ["a_tile_is_only_blocked_by_a_neighbour_on_its_own_layer_and_row"],
    ),
    (
        "a tile is blocked by a neighbour on any row",
        "            if other.pos.layer == pos.layer && other.pos.row == pos.row {",
        "            if other.pos.layer == pos.layer {",
        ["a_tile_is_only_blocked_by_a_neighbour_on_its_own_layer_and_row"],
    ),
    (
        "a tile needs both sides open, so the deal is unplayable",
        "        !blocked_left || !blocked_right",
        "        !blocked_left && !blocked_right",
        ["a_tile_with_open_air_on_one_side_is_free"],
    ),
    (
        "a removed tile still blocks its neighbours",
        "        for (i, other) in self.tiles.iter().enumerate() {\n"
        "            if i == idx || other.removed {\n"
        "                continue;\n"
        "            }\n"
        "            if other.pos.layer == pos.layer && other.pos.row == pos.row {",
        "        for (i, other) in self.tiles.iter().enumerate() {\n"
        "            if i == idx {\n"
        "                continue;\n"
        "            }\n"
        "            if other.pos.layer == pos.layer && other.pos.row == pos.row {",
        ["a_removed_tile_blocks_nothing_and_is_itself_not_free"],
    ),
    (
        "a removed tile is free, so it can be played twice",
        "        if tile.removed {\n            return false;\n        }",
        "",
        ["a_removed_tile_blocks_nothing_and_is_itself_not_free"],
    ),
    (
        "an index off the end of the board is free",
        "        let tile = match self.tiles.get(idx) {\n"
        "            Some(t) => t,\n"
        "            None => return false,\n"
        "        };",
        "        let Some(tile) = self.tiles.get(idx) else {\n"
        "            return true;\n"
        "        };",
        ["an_index_off_the_end_of_the_board_is_not_free"],
    ),
    (
        "a hint may name a tile that is not free",
        "        let free = self.free_tiles();\n"
        "        for (n, &a) in free.iter().enumerate() {",
        "        let free: Vec<usize> = (0..self.tiles.len()).collect();\n"
        "        for (n, &a) in free.iter().enumerate() {",
        ["a_matching_pair_that_is_buried_is_not_a_hint"],
    ),
    (
        "a hint may pair a tile with itself",
        "            for &b in free.iter().skip(n).skip(1) {",
        "            for &b in free.iter().skip(n) {",
        ["a_hint_names_two_free_tiles_that_actually_match"],
    ),
    (
        "a shuffle moves the tiles instead of only their faces",
        "    fn shuffle_remaining(&mut self, rng: &mut SeededRng) {",
        "    fn shuffle_remaining(&mut self, rng: &mut SeededRng) {\n"
        "        self.tiles.reverse();",
        ["a_shuffle_keeps_every_tile_where_it_is_and_deals_it_a_new_face"],
    ),
    (
        "a shuffle deals faces back onto the tiles already taken",
        "    fn shuffle_remaining(&mut self, rng: &mut SeededRng) {",
        "    fn shuffle_remaining(&mut self, rng: &mut SeededRng) {\n"
        "        for t in &mut self.tiles {\n            t.removed = false;\n        }",
        ["a_shuffle_leaves_the_removed_tiles_removed"],
    ),
    (
        "a season matches a flower",
        "    fn matches(self, other: Self) -> bool {",
        "    fn matches(self, other: Self) -> bool {\n"
        "        if self.wildcard() && other.wildcard() {\n            return true;\n        }",
        ["a_season_does_not_match_a_flower"],
    ),
    (
        "a wild tile matches anything at all",
        "    fn matches(self, other: Self) -> bool {",
        "    fn matches(self, other: Self) -> bool {\n"
        "        if self.wildcard() || other.wildcard() {\n            return true;\n        }",
        [
            "a_wild_tile_does_not_match_an_ordinary_one",
            "the_wildcard_flag_says_exactly_which_tiles_match_a_stranger",
        ],
    ),
    (
        "the deal is short of a full set",
        "const LAYOUT_SIZE: usize = 144;",
        "const LAYOUT_SIZE: usize = 140;",
        ["the_deal_is_a_full_mahjong_set_of_a_hundred_and_forty_four"],
    ),
    (
        "the board is only won when it is also unwinnable",
        "        self.remaining() == 0",
        "        self.remaining() == 0 && self.find_hint().is_none()",
        ["an_empty_board_is_won_and_not_lost"],
    ),
    (
        "an empty board is reported as lost",
        "        self.remaining() > 0 && self.find_hint().is_none()",
        "        self.find_hint().is_none()",
        ["an_empty_board_is_won_and_not_lost"],
    ),
    (
        "a board with no move left is left silent",
        "        } else if self.board.is_lost() {\n"
        "            self.status = GameStatus::Lost;\n"
        '            self.message = Some("No moves left! S=shuffle, N=new");',
        "        } else if false {\n"
        "            self.status = GameStatus::Lost;",
        ["a_board_with_no_move_left_is_announced_rather_than_left_silent"],
    ),
    (
        "an undone pair is put back but the move is still counted",
        "            self.moves = self.moves.saturating_sub(1);",
        "",
        ["z_undoes_the_last_pair_and_says_so_when_there_is_none"],
    ),
    (
        "undo with an empty stack says nothing",
        '            let changed = self.message != Some("Nothing to undo");\n'
        '            self.message = Some("Nothing to undo");\n'
        "            changed",
        "            false",
        ["z_undoes_the_last_pair_and_says_so_when_there_is_none"],
    ),
    (
        "a pair that does not match is taken off the board anyway",
        "                } else if match (self.board.tiles.get(prev), self.board.tiles.get(idx)) {\n"
        "                    (Some(a), Some(b)) => a.kind.matches(b.kind),\n"
        "                    _ => false,\n"
        "                } {",
        "                } else if true {",
        ["clicking_two_tiles_that_do_not_match_selects_the_second_and_says_so"],
    ),
    (
        "a matched pair is not counted as a move",
        "                    self.moves = self.moves.saturating_add(1);",
        "",
        ["clicking_a_matching_pair_takes_both_off_the_board"],
    ),
    (
        "clicking the selected tile a second time matches it with itself",
        "                if prev == idx {",
        "                if false {",
        ["clicking_the_selected_tile_again_puts_it_back"],
    ),
    (
        "a covered tile is selected rather than refused",
        "        if !self.board.is_free(idx) {\n"
        '            let changed = self.message != Some("Tile is not free");\n'
        '            self.message = Some("Tile is not free");\n'
        "            return changed;\n"
        "        }",
        "",
        ["clicking_a_hemmed_in_tile_says_why_nothing_happened"],
    ),
    (
        "a finished game goes on answering clicks on tiles",
        "        if self.status != GameStatus::Playing {\n            return false;\n        }\n\n"
        "        if !self.board.is_free(idx) {",
        "        if !self.board.is_free(idx) {",
        ["a_game_that_is_over_stops_answering_clicks_on_tiles"],
    ),
    (
        "a shuffle puts a won board back into play",
        "        if self.status == GameStatus::Won {\n            return false;\n        }",
        "",
        ["s_does_nothing_to_a_board_that_has_already_been_cleared"],
    ),
    (
        "a new game is dealt from the same seed as the last one",
        "        self.seed = self.seed.wrapping_add(1);",
        "",
        ["n_deals_a_new_game_from_a_new_seed"],
    ),
    (
        "the deal is not seeded, so the same seed gives two different games",
        "    fn with_seed(seed: u64) -> Self {",
        "    fn with_seed(seed: u64) -> Self {\n        let seed = seed.wrapping_add(1);",
        ["two_games_from_one_seed_are_the_same_and_from_two_seeds_are_not"],
    ),
    # -- The picture ---------------------------------------------------
    (
        "the tiles are painted top layer first, so the cap is buried",
        "        order.sort_by_key(|&i| self.tiles.get(i).map_or(0, |t| t.pos.layer));",
        "        order.sort_by_key(|&i| usize::MAX - self.tiles.get(i).map_or(0, |t| t.pos.layer));",
        [
            "the_paint_order_runs_bottom_layer_first_and_skips_what_is_gone",
            "a_click_on_a_stack_reaches_the_tile_on_top",
        ],
    ),
    (
        "a tile that has been taken is painted anyway",
        "        let mut order: Vec<usize> = self\n"
        "            .tiles\n"
        "            .iter()\n"
        "            .enumerate()\n"
        "            .filter(|(_, t)| !t.removed)",
        "        let mut order: Vec<usize> = self\n"
        "            .tiles\n"
        "            .iter()\n"
        "            .enumerate()\n"
        "            .filter(|(_, _t)| true)",
        [
            "the_paint_order_runs_bottom_layer_first_and_skips_what_is_gone",
            "a_removed_tile_is_neither_painted_nor_clickable",
        ],
    ),
    (
        "the hit box is a different rectangle from the one that was painted",
        "            f.hit(Target::Tile(idx), r);",
        "            f.hit(Target::Tile(idx), Rect::new(r.x, r.y, r.w * 0.5, r.h));",
        ["a_tiles_hit_box_is_the_rectangle_it_was_drawn_in"],
    ),
    (
        "the board records no box, so a click on bare felt hits nothing",
        "        f.hit(Target::Board, l.board);",
        "",
        ["clicking_the_boards_margin_lands_on_the_board_and_not_on_a_tile"],
    ),
    (
        "a window with no room draws zero-sized tiles rather than none",
        "        if l.tile_w <= 0.0 || l.tile_h <= 0.0 {\n            return;\n        }",
        "",
        ["a_window_with_no_room_for_a_tile_draws_no_tiles_rather_than_zero_sized_ones"],
    ),
    (
        "the selected tile is painted like every other one",
        "            let bg_color = if is_selected {\n                TILE_SELECTED",
        "            let bg_color = if false {\n                TILE_SELECTED",
        ["a_selected_tile_is_painted_differently_from_an_unselected_one"],
    ),
    (
        "a hinted pair is painted like every other tile",
        "            } else if is_hint {\n                TILE_HINT",
        "            } else if false {\n                TILE_HINT",
        ["a_hinted_pair_is_painted_differently_from_the_rest"],
    ),
    (
        "the hint highlights both tiles of the wrong pair",
        "            let is_hint = self.show_hint && self.hint.is_some_and(|(a, b)| idx == a || idx == b);",
        "            let is_hint = self.hint.is_some_and(|(a, b)| idx == a || idx == b);",
        ["a_hinted_pair_is_painted_differently_from_the_rest"],
    ),
    (
        "the cursor border is the old fixed two pixels",
        "                let bw = (l.tile_w * 0.05).clamp(1.0, 4.0);",
        "                let bw = 2.0;",
        ["the_cursor_is_drawn_as_a_border_thick_enough_to_see_at_any_size"],
    ),
    (
        "the label is drawn at a fixed size again, so it spills off a small tile",
        "        let tile_font = fit_tile_font(tile_w, tile_h);",
        "        let tile_font = 16.0;",
        [
            "a_tiles_label_is_drawn_inside_the_tile_at_every_size",
            "the_label_font_grows_with_the_tile_it_is_drawn_on",
        ],
    ),
    (
        "the label is not centred in the tile it names",
        "                x: text::center_x(label, r.x + r.w / 2.0, l.tile_font, FontWeightHint::Bold),",
        "                x: r.x,",
        ["a_tiles_hit_box_is_the_rectangle_it_was_drawn_in"],
    ),
    (
        "the counters are written once rather than read off the board",
        '                    "Tiles: {}  Moves: {}  Free: {}",\n'
        "                    self.board.remaining(),\n"
        "                    self.moves,\n"
        "                    self.board.free_tiles().len()",
        '                    "Tiles: {}  Moves: {}  Free: {}",\n                    144, 0, 38',
        ["the_counters_follow_the_board_rather_than_being_written_once"],
    ),
    (
        "the counters and the message are drawn over each other",
        "        let message_box = Rect::new(inner.x + half, line_y, inner.w - half, l.status);",
        "        let message_box = Rect::new(inner.x, line_y, inner.w, l.status);",
        ["the_counters_and_the_message_never_share_a_pixel"],
    ),
    (
        "a message is drawn even when there is none",
        "        if let Some(msg) = self.message {",
        '        if let Some(msg) = self.message.or(Some("")) {',
        ["a_message_is_drawn_only_when_there_is_one_to_draw"],
    ),
    (
        "the title is not told how wide its box is, so it runs off the window",
        "            max_width: Some(title_box.w),",
        "            max_width: None,",
        ["every_line_of_text_is_told_how_wide_its_box_is"],
    ),
    (
        "the header may be drawn outside the band it was given",
        "        let line_y = (title_box.bottom() + l.pad).min(inner.bottom() - l.status);",
        "        let line_y = title_box.bottom() + l.pad;",
        ["the_counters_and_the_message_never_share_a_pixel"],
    ),
    # -- The window ----------------------------------------------------
    (
        "the right button plays a tile too",
        "        if !matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if !matches!(event.kind, MouseEventKind::Press(_)) {",
        ["only_the_left_button_plays_a_tile"],
    ),
    (
        "a button release plays a tile as well as a press",
        "        if !matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if false {",
        ["only_the_left_button_plays_a_tile"],
    ),
    (
        "a click on the furniture falls through to a tile",
        "            | Target::Board => EventResult::Ignored,",
        "            | Target::Board => EventResult::Consumed,",
        [
            "clicking_the_furniture_is_not_a_move",
            "clicking_a_legend_row_is_not_a_move_either",
            "clicking_the_boards_margin_lands_on_the_board_and_not_on_a_tile",
        ],
    ),
    (
        "a click leaves the keyboard cursor where it was",
        "                let moved = self.cursor.tile_idx != Some(idx);\n"
        "                self.cursor.tile_idx = Some(idx);",
        "                let moved = false;",
        ["a_click_moves_the_keyboard_cursor_to_the_tile_it_landed_on"],
    ),
    (
        "a click is read against the size the app started at, not the one it was drawn at",
        "        let Some(target) = self\n"
        "            .frame(self.width, self.height)\n"
        "            .hit_test(event.x, event.y)",
        "        let Some(target) = self\n"
        "            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)\n"
        "            .hit_test(event.x, event.y)",
        ["a_click_is_read_against_the_size_the_frame_was_drawn_at"],
    ),
    (
        "a resize is not remembered, so the next click is read against the old size",
        "        Event::Resize { width, height } => {\n"
        "            game.resize(f32_from_u32(*width), f32_from_u32(*height));",
        "        Event::Resize { width, height } => {\n            let _ = (width, height);",
        ["a_resize_is_remembered_so_the_next_click_is_read_against_it"],
    ),
    (
        "a render does not record the size it was drawn at",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["a_render_records_the_size_it_was_drawn_at"],
    ),
    (
        "the close button is ignored, so the window cannot be shut",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "",
        ["the_close_button_ends_the_program"],
    ),
    (
        "every event asks for a repaint, whether or not anything changed",
        "        match handle_event(self, event) {\n"
        "            EventResult::Consumed => Response::Redraw,\n"
        "            EventResult::Ignored => Response::Idle,\n"
        "        }",
        "        let _ = handle_event(self, event);\n        Response::Redraw",
        ["an_event_that_changes_nothing_does_not_ask_for_a_repaint"],
    ),
    (
        "no event asks for a repaint, so the picture never updates",
        "        match handle_event(self, event) {\n"
        "            EventResult::Consumed => Response::Redraw,\n"
        "            EventResult::Ignored => Response::Idle,\n"
        "        }",
        "        let _ = handle_event(self, event);\n        Response::Idle",
        ["an_event_that_changes_something_asks_for_a_repaint"],
    ),
    (
        "a key release is handled as a press",
        "        if !event.pressed {\n            return EventResult::Ignored;\n        }",
        "",
        ["a_key_release_is_not_a_key_press"],
    ),
    (
        "a key this game does not use is swallowed anyway",
        "            _ => return EventResult::Ignored,",
        "            _ => false,",
        ["a_key_this_game_does_not_use_is_left_for_the_window"],
    ),
    (
        "enter plays a tile even with no cursor on one",
        "            Key::Enter | Key::Space => match self.cursor.tile_idx {\n"
        "                Some(ci) => self.try_select(ci),\n"
        "                None => false,\n"
        "            },",
        "            Key::Enter | Key::Space => match self.cursor.tile_idx.or(Some(0)) {\n"
        "                Some(ci) => self.try_select(ci),\n"
        "                None => false,\n"
        "            },",
        ["enter_with_no_cursor_does_nothing"],
    ),
    (
        "space does nothing, so only enter plays a tile",
        "            Key::Enter | Key::Space => match self.cursor.tile_idx {",
        "            Key::Enter => match self.cursor.tile_idx {",
        ["enter_and_space_play_the_tile_under_the_cursor"],
    ),
    (
        "escape clears the selection but leaves the hint and the message",
        "                self.selected = None;\n"
        "                self.show_hint = false;\n"
        "                self.message = None;\n"
        "                changed",
        "                self.selected = None;\n                changed",
        ["escape_clears_the_selection_the_hint_and_the_message_together"],
    ),
    (
        "the arrows are swapped, so left goes right",
        "            Key::Left => self.move_cursor(-1, 0),\n"
        "            Key::Right => self.move_cursor(1, 0),",
        "            Key::Left => self.move_cursor(1, 0),\n"
        "            Key::Right => self.move_cursor(-1, 0),",
        ["the_arrows_walk_the_cursor_between_free_tiles"],
    ),
    (
        "the cursor may land on a tile that cannot be played",
        "        let free = self.board.free_tiles();",
        "        let free: Vec<usize> = (0..self.board.tiles.len()).collect();",
        ["the_arrows_walk_the_cursor_between_free_tiles"],
    ),
    (
        "the arrows are measured in pixels, so the same press picks a different tile at each size",
        "        let unit = l.tile_w;",
        "        let unit = 1.0;",
        ["the_arrows_measure_in_tile_widths_so_the_same_press_picks_the_same_tile"],
    ),
    (
        "the cross-axis costs nothing, so an arrow wanders off its row",
        "                    let dist = main + cross * 2.0;",
        "                    let dist = main;",
        ["the_arrows_measure_in_tile_widths_so_the_same_press_picks_the_same_tile"],
    ),
    (
        "an arrow at the edge of the board reports a change it did not make",
        "        if let Some((bi, _)) = best {\n"
        "            let moved = self.cursor.tile_idx != Some(bi);",
        "        if let Some((bi, _)) = best {\n            let moved = true;",
        ["an_arrow_at_the_edge_of_the_board_reports_that_nothing_changed"],
    ),
    (
        "a window too small for a tile jumps the cursor to an arbitrary tile",
        "        if l.tile_w <= 0.0 {\n            return false;\n        }",
        "",
        ["a_window_too_small_to_draw_a_tile_leaves_the_cursor_where_it_is"],
    ),
    (
        "the cursor starts on nothing, so the keyboard cannot play the first tile",
        "        let first_free = board.free_tiles().first().copied();",
        "        let first_free = None;",
        ["a_fresh_deal_is_playable_and_the_cursor_starts_on_a_tile_that_can_be_played"],
    ),
    (
        "the game is never won, so the last pair leaves an empty board in play",
        "        if self.board.is_won() {\n            self.status = GameStatus::Won;",
        "        if false {\n            self.status = GameStatus::Won;",
        ["taking_the_last_pair_wins_the_game"],
    ),
    (
        "the app has no name for the taskbar",
        '        "Mahjong Solitaire".to_string()\n    }\n\n    fn app_id(&self) -> String {',
        '        String::new()\n    }\n\n    fn app_id(&self) -> String {',
        ["the_app_names_itself_for_the_taskbar_and_asks_for_a_size_it_can_use"],
    ),
    (
        "the app asks for a window it cannot draw anything in",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (1, 1)",
        ["the_app_names_itself_for_the_taskbar_and_asks_for_a_size_it_can_use"],
    ),
    (
        "the two spellings of the starting size drift apart again",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (900, 700)",
        ["the_app_names_itself_for_the_taskbar_and_asks_for_a_size_it_can_use"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "mahjong", timeout=300, only=sys.argv[1:] or None))

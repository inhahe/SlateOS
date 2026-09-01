"""Mutation test for the terminal's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The terminal is the forty-fifth application in this campaign, and the one where
the gap between what was written and what ran was widest.  There were 73 tests
and a working VT parser underneath them -- CSI, OSC, SGR, the alternate screen,
the scroll region, tab stops -- and beside it, in the same crate, a complete
two-thousand-line pseudo-terminal with a line discipline, cooked and raw modes,
a window size and a child process.  `main` was:

    fn main() {
        let mut term = TerminalState::new(TerminalConfig::default());
        term.feed(b"Welcome to SlateOS Terminal\\r\\n");
        term.feed(b"$ ");
        let _commands = term.render();
    }

It fed itself two strings, rendered one frame into a `Vec`, dropped it on the
next line and returned.  The two halves had never been introduced: `pty.rs` had
no caller at all outside its own tests.

What that hid, in rough order of how badly it would have shown:

  * **There was no child.**  Not a slow child or a mis-plumbed one -- none.
    Nothing opened a PTY, nothing wrote a keystroke to one, nothing read one
    back.  The terminal was a VT parser with a private conversation.
  * **The grid was eighty by twenty-four whatever the window was.**  `resize`
    existed and had no caller outside the tests, so a maximised terminal drew
    a small rectangle of text in the corner of a large empty window, and a
    narrow one drew its right-hand columns past the edge onto the desktop.
  * **`PtyMaster::resize` -- this tree's `TIOCSWINSZ` -- had no caller either.**
    A shell under this terminal wrapped its prompt at eighty columns in a
    window twice that wide.
  * **The selection copied the wrong line.**  It recorded a *screen* row and
    read `self.screen` with that number, while the drawing pass walked the
    scrollback: selecting a line of history highlighted the right text and
    copied whatever happened to be at that screen slot.
  * **The visual bell was a countdown of frames.**  It lasted a tenth of a
    second at sixty frames a second, twice that at thirty, and *for ever* on a
    terminal with nothing else to redraw for -- which is the usual state of a
    terminal waiting at a prompt.
  * **`config.cursor_blink` was a setting nothing read.**  The cursor was
    painted on every frame regardless.
  * **There was no scrollbar and no other sign that scrollback existed.**  A
    terminal scrolled a thousand lines back looked exactly like one sitting at
    a prompt with a quiet child.
  * **`parse_extended_color` range-checked one of three components** and then
    indexed all three, and narrowed with `as u8`, so `38;5;256` painted black
    while meaning white.
  * **Fifty-seven lints were hidden behind crate-level allows.**

Two more faults were found while the tests below were being written, and they
are the two the sweep leans on hardest:

  * `output_buffer` had two writers and one dead end.  Keystrokes went straight
    to the PTY, while the parser's own replies -- the cursor position report,
    the device attributes answer -- were queued in the buffer and sent nowhere,
    so a full-screen program that asked this terminal where its cursor was
    waited for an answer sitting in a `Vec`.  And `on_event` appended the bytes
    `handle_event` had just queued, so every character typed arrived doubled.
  * `on_event` answered a tick itself and returned before the dispatch that
    reads the child could run.  Under a real window -- the only place
    `on_event` is called -- the terminal was write-only.

A third round came from this sweep rather than from the wiring, and is
known-issues.md lesson 103: two features had tests that asked the *deciding
function* instead of reading the picture -- `is_selected` for the highlight and
the `blink_on` field for the cursor -- so the drawing pass was free to pass the
wrong row, or to consult nothing at all, with every assertion still true.  A
third whole-frame test supplied a bound the code no longer declared
(`max_width.unwrap_or_else(measure)`), and one character measured is one cell
wide, so an unbounded glyph looked bounded.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- the grid as a quotient of the window --------------------------------
    (
        "the bar is taken out of the window after the columns rather than before",
        "        let grid_w = (window.w - bar_w).max(0.0);",
        "        let grid_w = window.w;",
        ["the_grid_is_a_whole_number_of_cells_and_never_reaches_the_bar"],
    ),
    (
        "the bar takes its full width however narrow the window",
        "        let bar_w = BAR_W.min(window.w / 4.0).max(0.0);",
        "        let bar_w = BAR_W;",
        ["the_bar_never_takes_more_than_a_quarter_of_the_window"],
    ),
    (
        "the grid is the space it was given rather than a whole number of cells",
        "        let grid = Rect::new(0.0, 0.0, usize_f32(cols) * cell_w, usize_f32(rows) * cell_h);",
        "        let grid = Rect::new(0.0, 0.0, grid_w, window.h);",
        # Not `every_hit_box_lies_in_the_band_that_owns_it`: the grid's band
        # *is* `l.grid`, so it moves with the mutation and the hit box stays
        # inside it.
        ["the_grid_is_a_whole_number_of_cells_and_never_reaches_the_bar"],
    ),
    (
        "a nonsense cell size is a window with every column in it",
        "    if !cell.is_finite() || cell <= 0.0 || !span.is_finite() {\n        return 0;\n    }",
        "    if false {\n        return 0;\n    }",
        ["a_nonsense_cell_size_yields_no_grid_rather_than_a_full_one"],
    ),
    (
        "the loop counts one cell too many, so the last column is off the edge",
        "    while used <= span + 0.01 {",
        "    while used <= span + cell {",
        [
            "a_nonsense_cell_size_yields_no_grid_rather_than_a_full_one",
            "nothing_is_painted_outside_the_window",
        ],
    ),
    (
        "the cell count is exact, so a window of exactly n cells holds n-1",
        "    while used <= span + 0.01 {",
        "    while used <= span {",
        # The drift is a rounding error, so it does not show at one or two
        # cells -- `8.4f32 + 8.4f32` is exactly `16.8f32`. It shows at the
        # eighty columns the window opens at.
        ["the_window_opens_at_a_size_the_default_grid_fits_in"],
    ),
    # -- what is painted -----------------------------------------------------
    #
    # Two guards have no row here, and deliberately.  Removing the frame's
    # outer `f.clip(l.window)`, and the `l.grid.intersect(l.window)` the grid's
    # hit box is taken from, changes no picture at any size: every rectangle
    # the pass paints is derived from the window it was given, and `draw_cells`
    # already caps the rows and columns it walks at the layout's.  Both are
    # there for the change that has not been made yet, and the tests that would
    # catch that change -- `nothing_is_painted_outside_the_window` and
    # `every_hit_box_is_inside_the_window_and_has_area` -- are load-bearing
    # against the rows that follow.  A mutation that cannot alter the output is
    # not evidence about a test; inventing an owner for it would only have
    # taught the table to lie.
    (
        "the bar is hung past the right-hand edge of the window",
        "        let bar = Rect::new(window.w - bar_w, 0.0, bar_w, window.h);",
        "        let bar = Rect::new(window.w, 0.0, bar_w, window.h);",
        ["the_grid_and_the_bar_stay_inside_the_window"],
    ),
    (
        "the clip is opened and never closed",
        "        f.unclip();\n        f\n    }",
        "        f\n    }",
        ["the_frame_balances_its_clips_at_every_size"],
    ),
    (
        "only the grid is filled, leaving a strip of desktop showing through",
        "        fill(&mut f, l.window, scheme.background);",
        "        fill(&mut f, l.grid, scheme.background);",
        ["the_window_is_filled_edge_to_edge_before_anything_else"],
    ),
    (
        "a glyph is drawn unbounded, so a proportional face walks off the row",
        '        max_width: Some(text::measure("W", font_size, font_weight).max(font_size)),',
        "        max_width: None,",
        ["no_glyph_runs_off_the_window_it_is_drawn_in"],
    ),
    (
        "the thumb is hit-boxed over the whole track rather than over itself",
        "        f.hit(Target::ScrollThumb, thumb);",
        "        f.hit(Target::ScrollThumb, bar);",
        # Not `every_hit_box_has_ink_painted_at_exactly_that_rectangle`: the
        # track is a fill at exactly that rectangle, so the ink is there.  What
        # the mutation destroys is the thumb's *position*, which is the one
        # thing the thumb is for.
        ["the_thumb_says_where_in_the_scrollback_the_viewport_is"],
    ),
    (
        "the grid reaches under the bar, so the bar's clicks land on text",
        "        f.hit(Target::Grid, grid);",
        "        f.hit(Target::Grid, l.window);",
        ["every_hit_box_lies_in_the_band_that_owns_it"],
    ),
    (
        "the rows drawn are the buffer's rather than the window's",
        "        let rows = l.rows.min(self.rows());\n        let cols = l.cols.min(self.cols());",
        "        let rows = self.rows();\n        let cols = self.cols();",
        ["the_grids_hit_box_covers_every_cell_that_was_drawn"],
    ),
    # -- the scrollbar -------------------------------------------------------
    (
        "a thumb is drawn even when the whole buffer is on screen",
        "        if total == 0 || shown == 0 || total <= shown {",
        "        if total == 0 || shown == 0 {",
        ["there_is_no_thumb_when_the_whole_buffer_is_on_screen"],
    ),
    (
        "a long scrollback gets a sliver of a thumb rather than a floor",
        "        let thumb_h = (bar.h * span).max(4.0).min(bar.h);",
        "        let thumb_h = bar.h * span;",
        ["the_thumb_is_always_thick_enough_to_aim_at"],
    ),
    (
        "the thumb is pinned to the top of its track",
        "        let thumb = Rect::new(bar.x, bar.y + travel * progress, bar.w, thumb_h);",
        "        let thumb = Rect::new(bar.x, bar.y, bar.w, thumb_h);",
        ["the_thumb_says_where_in_the_scrollback_the_viewport_is"],
    ),
    (
        "a press in the bar does nothing",
        "                if l.bar.contains(event.x, event.y) {\n                    self.press_bar(&l, event.y);\n                    return;\n                }",
        "                if l.bar.contains(event.x, event.y) {\n                    return;\n                }",
        ["a_press_in_the_bar_pages_towards_where_it_landed"],
    ),
    (
        "a press in the bar starts a selection as well",
        "                if l.bar.contains(event.x, event.y) {\n                    self.press_bar(&l, event.y);\n                    return;\n                }",
        "                if l.bar.contains(event.x, event.y) {\n                    self.press_bar(&l, event.y);\n                }",
        ["a_press_in_the_bar_does_not_start_a_selection"],
    ),
    (
        "a press pages one way only",
        "        if aimed_top < current_top {\n            self.scroll_viewport_up(page);\n        } else if aimed_top > current_top {\n            self.scroll_viewport_down(page);\n        }",
        "        if aimed_top < current_top {\n            self.scroll_viewport_up(page);\n        }",
        ["a_press_in_the_bar_pages_towards_where_it_landed"],
    ),
    # -- the pointer on the grid ---------------------------------------------
    (
        "a click selects the cell one row down from the one it landed on",
        "                self.clear_selection();\n                self.selection_start(event.x, event.y);",
        "                self.clear_selection();\n                self.selection_start(event.x, event.y + self.config.cell_height);",
        ["a_click_in_the_grid_selects_the_character_it_landed_on"],
    ),
    (
        "the copy reads the screen at the selection's row, not the buffer",
        "            let Some(line) = self.line_at(row) else {\n                continue;\n            };",
        "            let Some(line) = self.screen.get(row) else {\n                continue;\n            };",
        [
            "a_selection_made_in_the_scrollback_copies_the_scrollback",
            "the_highlight_and_the_copy_agree_on_which_row_is_selected",
        ],
    ),
    (
        "the highlight is painted at the screen row rather than the buffer row",
        "                let selected = self.is_selected(buffer_row, col);",
        "                let selected = self.is_selected(screen_row, col);",
        ["the_highlight_is_painted_on_the_row_the_pointer_landed_on"],
    ),
    (
        "a line falling off the scrollback drags the selection down with it",
        "        if let Some(sel) = self.selection.as_mut() {\n            sel.start_row = sel.start_row.saturating_sub(1);\n            sel.end_row = sel.end_row.saturating_sub(1);\n        }",
        "        if let Some(sel) = self.selection.as_mut() {\n            let _ = &sel.start_row;\n        }",
        ["a_line_falling_off_the_scrollback_does_not_move_the_selection"],
    ),
    # -- the scroll offset ---------------------------------------------------
    (
        "the offset may pass the top of the scrollback",
        "        self.scroll_offset = self.scroll_offset.saturating_add(lines).min(max);",
        "        let _ = max;\n        self.scroll_offset = self.scroll_offset.saturating_add(lines);",
        ["the_offset_never_passes_the_top_of_the_scrollback"],
    ),
    (
        "scrolling forward underflows past the live end",
        "        self.scroll_offset = self.scroll_offset.saturating_sub(lines);",
        "        self.scroll_offset = self.scroll_offset.wrapping_sub(lines);",
        ["the_offset_never_passes_the_live_end"],
    ),
    (
        "a resize does not put the offset back in range",
        "        self.clamp_scroll();\n    }\n\n    /// Put the scrollback offset back in range.",
        "    }\n\n    /// Put the scrollback offset back in range.",
        ["a_taller_window_does_not_leave_the_offset_above_the_buffer"],
    ),
    (
        "the viewport top counts forward from the oldest line rather than back from the newest",
        "        self.scrollback.len().saturating_sub(self.scroll_offset)",
        "        self.scroll_offset",
        ["scrolling_back_and_returning_shows_the_same_rows_again"],
    ),
    (
        "typing leaves the user reading the scrollback",
        "                self.scroll_offset = 0;\n                self.wake_cursor();",
        "                self.wake_cursor();",
        ["typing_returns_to_the_live_end"],
    ),
    # -- the child on the other end ------------------------------------------
    (
        "nothing is ever sent to the child",
        "        if let Ok(n) = pair.master.write(&self.output_buffer) {",
        "        if let Ok(n) = Ok::<usize, ()>(self.output_buffer.len()) {",
        ["a_typed_line_reaches_the_child"],
    ),
    (
        "a keystroke is queued and never flushed",
        "        let out = self.dispatch_event(event);\n        self.flush_to_child();\n        out",
        "        self.dispatch_event(event)",
        ["a_typed_line_reaches_the_child"],
    ),
    (
        "the parser's answer to `where is the cursor` goes nowhere",
        "                self.to_child(report.as_bytes());",
        "                let _ = report;",
        ["a_reply_the_parser_produced_reaches_the_child_too"],
    ),
    (
        "every keystroke is sent twice",
        "        self.handle_event(event);\n        // A tick that changed nothing",
        "        let echoed = self.handle_event(event);\n        self.to_child(&echoed);\n        self.flush_to_child();\n        // A tick that changed nothing",
        ["a_keystroke_is_sent_once"],
    ),
    (
        "a short write loses the bytes the channel could not take",
        "            let taken = n.min(self.output_buffer.len());\n            self.output_buffer.drain(..taken);",
        "            let _ = n;\n            self.output_buffer.clear();",
        ["what_the_child_could_not_take_yet_is_kept_rather_than_dropped"],
    ),
    (
        "the child is never read",
        "        let mut got = Vec::new();",
        "        return false;\n        #[allow(unreachable_code)]\n        let mut got = Vec::new();",
        [
            "what_the_child_writes_appears_on_the_screen",
            "a_tick_is_what_reads_the_child",
            "a_tick_is_what_reads_the_child_through_the_window_too",
        ],
    ),
    (
        "the tick does not read the child, only the direct call does",
        "            Event::Tick { elapsed_ms, .. } => {\n                self.on_tick(*elapsed_ms);",
        "            Event::Tick { elapsed_ms, .. } => {\n                self.tick(*elapsed_ms);",
        [
            "a_tick_is_what_reads_the_child",
            "a_tick_is_what_reads_the_child_through_the_window_too",
        ],
    ),
    (
        "the window answers the tick itself, so the child is never read under it",
        "        self.handle_event(event);\n        // A tick that changed nothing",
        "        if let Event::Tick { elapsed_ms, .. } = event {\n            return if self.tick(*elapsed_ms) {\n                Response::Redraw\n            } else {\n                Response::Idle\n            };\n        }\n        self.handle_event(event);\n        // A tick that changed nothing",
        ["a_tick_is_what_reads_the_child_through_the_window_too"],
    ),
    (
        "the child is not told how big the window is",
        "            if let Some(pair) = self.pty.as_ref() {\n                pair.master.resize(u16_of(l.cols), u16_of(l.rows));\n            }",
        "            if let Some(pair) = self.pty.as_ref() {\n                let _ = pair;\n            }",
        ["the_child_is_told_how_big_the_window_is"],
    ),
    (
        "an exited child is indistinguishable from a quiet one",
        "            if pair.master.child_finished() && !self.child_finished {",
        "            if false && !self.child_finished {",
        ["a_child_that_has_exited_is_said_so_once"],
    ),
    (
        "the exit is announced on every tick for ever",
        "            if pair.master.child_finished() && !self.child_finished {\n                self.child_finished = true;",
        "            if pair.master.child_finished() {\n                self.child_finished = true;",
        ["a_child_that_has_exited_is_said_so_once"],
    ),
    # -- the clock -----------------------------------------------------------
    (
        "the bell is counted down by frames rather than by the clock",
        "            self.bell_flash_ms = self.bell_flash_ms.saturating_sub(elapsed_ms);",
        "            self.bell_flash_ms = self.bell_flash_ms.saturating_sub(1);",
        ["the_bell_flash_ages_on_the_clock_rather_than_on_redraws"],
    ),
    (
        "the bell is never painted",
        "        if self.bell_flash_ms > 0 {\n            fill(&mut f, l.window, Color::rgba(255, 255, 255, 30));\n        }",
        "        if false {\n            fill(&mut f, l.window, Color::rgba(255, 255, 255, 30));\n        }",
        ["the_bell_is_visible_while_it_is_lit"],
    ),
    (
        "the blink advances by one half per tick rather than by the time elapsed",
        "            self.blink_ms = self.blink_ms.saturating_add(elapsed_ms);\n            while self.blink_ms >= BLINK_MS {",
        "            self.blink_ms = self.blink_ms.saturating_add(BLINK_MS);\n            while self.blink_ms >= BLINK_MS {",
        ["the_cursor_blinks_once_per_interval_however_the_ticks_arrive"],
    ),
    (
        "a tick that fell behind consumes one blink and drops the rest",
        "            while self.blink_ms >= BLINK_MS {",
        "            if self.blink_ms >= BLINK_MS {",
        ["a_tick_longer_than_several_blinks_lands_on_the_right_half"],
    ),
    (
        "the cursor is drawn on every frame, so the blink setting reads nothing",
        "        if !self.cursor_visible || self.scroll_offset != 0 || !self.blink_on {",
        "        if !self.cursor_visible || self.scroll_offset != 0 {",
        ["the_cursor_leaves_the_picture_in_the_dark_half_of_the_blink"],
    ),
    (
        "a cursor whose blink was turned off is left in the dark half",
        "        } else if !self.blink_on {\n            // A cursor whose blink was turned off mid-blink must not be left in\n            // the dark half of it for the rest of the session.\n            self.blink_on = true;\n            changed = true;\n        }",
        "        }",
        ["a_hidden_cursor_needs_no_clock_and_is_not_left_dark"],
    ),
    (
        "every tick asks for a repaint, twenty-five times a second for ever",
        "        self.tick_changed = aged || read;",
        "        let _ = (aged, read);\n        self.tick_changed = true;",
        ["a_tick_that_changed_nothing_asks_for_no_frame"],
    ),
    (
        "a tick that changed something asks for no repaint",
        "        self.tick_changed = aged || read;",
        "        let _ = (aged, read);\n        self.tick_changed = false;",
        [
            "a_tick_that_changed_nothing_asks_for_no_frame",
            "a_tick_is_what_reads_the_child_through_the_window_too",
        ],
    ),
    (
        "the clock is asked for whether or not anything is moving",
        "        if blinking || self.bell_flash_ms > 0 {\n            Some(std::time::Duration::from_millis(BLINK_MS / 5))\n        } else {\n            None\n        }",
        "        let _ = blinking;\n        Some(std::time::Duration::from_millis(BLINK_MS / 5))",
        ["the_clock_is_asked_for_only_while_something_is_moving"],
    ),
    (
        "the clock is never asked for, so nothing ages",
        "        if blinking || self.bell_flash_ms > 0 {\n            Some(std::time::Duration::from_millis(BLINK_MS / 5))\n        } else {\n            None\n        }",
        "        let _ = blinking;\n        None",
        ["the_clock_is_asked_for_only_while_something_is_moving"],
    ),
    (
        "a hidden cursor goes on blinking, holding the desktop awake",
        "        let blinking = self.config.cursor_blink && self.cursor_visible;",
        "        let blinking = self.config.cursor_blink;",
        ["a_hidden_cursor_needs_no_clock_and_is_not_left_dark"],
    ),
    # -- the window the compositor drives ------------------------------------
    (
        "a close request does not close the window",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Idle;\n        }",
        ["closing_the_window_ends_the_program"],
    ),
    (
        "the resize event does not resize the grid",
        "            Event::Resize { width, height } => {\n                self.resize_to_window(u32_f32(*width), u32_f32(*height));",
        "            Event::Resize { width, height } => {\n                let _ = (width, height);",
        ["the_first_resize_is_what_sizes_the_grid"],
    ),
    (
        "drawing a frame does not re-derive the grid from the window",
        "        self.resize_to_window(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["drawing_a_frame_sizes_the_grid_as_well"],
    ),
    (
        "the window opens at a size its own default grid does not fit in",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (320, 240)",
        ["the_window_opens_at_a_size_the_default_grid_fits_in"],
    ),
    # -- the two arithmetic helpers the bar is built on ----------------------
    (
        "a fraction of nothing is a NaN",
        "    if whole == 0 {\n        return 0.0;\n    }\n    (usize_f32(part) / usize_f32(whole)).clamp(0.0, 1.0)",
        "    (usize_f32(part) / usize_f32(whole)).clamp(0.0, 1.0)",
        ["a_fraction_of_nothing_is_nothing_rather_than_a_nan"],
    ),
    (
        "a part larger than the whole is a fraction over one",
        "    (usize_f32(part) / usize_f32(whole)).clamp(0.0, 1.0)",
        "    usize_f32(part) / usize_f32(whole)",
        ["a_fraction_of_nothing_is_nothing_rather_than_a_nan"],
    ),
    (
        "a nonsense fraction scales to the whole rather than to none of it",
        "    if !fraction.is_finite() || fraction <= 0.0 {\n        return 0;\n    }",
        "    if false {\n        return 0;\n    }",
        ["scaling_by_a_nonsense_fraction_yields_none_of_the_whole"],
    ),
    (
        "the scale rounds up rather than down",
        "    while n < whole && usize_f32(n.saturating_add(1)) <= target {",
        "    while n < whole && usize_f32(n) < target {",
        ["scaling_by_a_nonsense_fraction_yields_none_of_the_whole"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "terminal", timeout=300, only=only))

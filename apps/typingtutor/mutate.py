"""Mutation test for the typingtutor suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The rows below are almost all about *wiring* -- the layout solved from the live
window size, the mouse arm, the wheel, the clock -- because that is what this
app had none of.  Its 65 inherited tests were written against a fixed 620x560
simulation with no event loop behind it, and every one of them still passed
while the program drew every control at a coordinate typed in by hand and threw
away every click.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- The layout ------------------------------------------------------
    (
        "a row is sized from the window rather than from the text in it",
        "        let row = (font + small + pad * 1.6).max(1.0);",
        "        let row = (h * 0.12).max(1.0);",
        # A body twice as tall then shows the same number of rows at twice the
        # size, which is the failure the test names in as many words.
        ["a_taller_window_lists_more_lessons"],
    ),
    (
        "a body too short for a row shows one anyway",
        "        if self.row <= 0.0 || self.body.h < self.row {\n"
        "            return 0;\n"
        "        }\n"
        "        (self.body.h / self.row).floor() as usize",
        "        if self.row <= 0.0 {\n"
        "            return 0;\n"
        "        }\n"
        "        ((self.body.h / self.row).floor() as usize).max(1)",
        ["a_body_too_short_for_a_row_shows_none"],
    ),
    (
        "list rows are stacked at a pitch chosen by eye",
        "            self.body.y + i as f32 * self.row,",
        "            self.body.y + i as f32 * 30.0,",
        ["rows_tile_the_body_without_overlapping"],
    ),
    (
        "a card is as wide as the old fixed pitch rather than as the window",
        "        let card_w = ((body.w - pad * (cards_across as f32 - 1.0)) "
        "/ cards_across as f32).max(0.0);",
        "        let card_w = 180.0;",
        ["cards_tile_their_row_without_overlapping"],
    ),
    (
        "cards are placed across on the old 180 px pitch",
        "            self.body.x + col as f32 * (cw + self.pad),",
        "            self.body.x + col as f32 * 180.0,",
        ["cards_tile_their_row_without_overlapping"],
    ),
    (
        "the card grid never starts a second row",
        "            self.body.y + rowi as f32 * (ch + self.pad),",
        "            self.body.y,",
        ["cards_tile_their_row_without_overlapping"],
    ),
    (
        "the body is measured from the top of the window, over its own title",
        "        let body = Rect::new(\n            pad,\n            subhead.bottom() + pad,",
        "        let body = Rect::new(\n            pad,\n            pad,",
        ["the_bands_stack_without_overlapping"],
    ),
    (
        "the chrome is kept whatever it costs the body",
        "        let budget = (h - h * 0.45 - pad * 2.0).max(0.0);",
        "        let budget = f32::INFINITY;",
        ["a_squeezed_window_gives_up_its_chrome_before_its_body"],
    ),
    (
        "the title is given up before the reminder lines are",
        "        for &i in &[1usize, 2, 0] {",
        "        for &i in &[0usize, 1, 2] {",
        ["a_squeezed_window_gives_up_its_chrome_before_its_body"],
    ),
    # -- The mouse -------------------------------------------------------
    (
        "the mouse is not wired at all",
        "            Event::Mouse(MouseEvent {\n"
        "                x,\n"
        "                y,\n"
        "                kind: MouseEventKind::Press(MouseButton::Left),\n"
        "            }) => {\n"
        "                let frame = self.frame(self.width, self.height);\n"
        "                match frame.hit_test(*x, *y) {\n"
        "                    Some(target) => self.activate(target),\n"
        "                    None => EventResult::Ignored,\n"
        "                }\n"
        "            }",
        "            Event::Mouse(MouseEvent {\n"
        "                kind: MouseEventKind::Press(MouseButton::Left),\n"
        "                ..\n"
        "            }) => EventResult::Ignored,",
        # This is the state the program shipped in, so most of the mouse suite
        # goes with it; these two are the narrowest that must.
        [
            "clicking_a_row_selects_the_lesson_drawn_in_it",
            "the_stats_chip_opens_the_statistics_view",
        ],
    ),
    (
        "a click is read against the size the program was written for",
        "                let frame = self.frame(self.width, self.height);",
        "                let frame = self.frame(WINDOW_WIDTH, WINDOW_HEIGHT);",
        ["the_controls_move_with_the_window_and_are_still_clickable"],
    ),
    (
        "a click on nothing is taken as a click on something",
        "                    None => EventResult::Ignored,\n                }\n            }",
        "                    None => EventResult::Consumed,\n                }\n            }",
        ["a_click_on_empty_space_is_ignored"],
    ),
    (
        "a row carries its place in the filtered list rather than its lesson",
        "            f.hit(Target::Lesson(lesson_idx), r);",
        "            f.hit(Target::Lesson(i), r);",
        # Invisible without a filter: unfiltered and unscrolled the two
        # numberings agree, which is why the plain click test cannot see this.
        ["a_row_clicked_under_a_filter_selects_the_lesson_it_shows"],
    ),
    (
        "the first click on a row starts it",
        "                if self.selected_lesson == idx && self.view == AppView::LessonSelect {",
        "                if self.view == AppView::LessonSelect {",
        ["a_second_click_on_the_selected_row_starts_the_lesson"],
    ),
    (
        "going back leaves the abandoned lesson in place",
        "            Target::Back => {\n"
        "                self.view = AppView::LessonSelect;\n"
        "                self.session = None;\n"
        "            }",
        "            Target::Back => {\n                self.view = AppView::LessonSelect;\n            }",
        ["every_view_away_from_the_list_has_a_labelled_way_back"],
    ),
    (
        "Retry restarts the first lesson rather than the one just finished",
        "            Target::Retry => self.start_lesson(self.selected_lesson),",
        "            Target::Retry => self.start_lesson(0),",
        ["retry_restarts_the_lesson_that_was_just_finished"],
    ),
    (
        "the filter chip does not say what it filters by",
        "        let filter_text = match self.category_filter {\n"
        "            None => String::from(\"All Categories\"),\n"
        "            Some(cat) => format!(\"Category: {}\", cat.name()),\n"
        "        };",
        "        let filter_text = String::from(\"All Categories\");",
        ["the_filter_chip_says_what_it_does_and_does_it"],
    ),
    (
        "filtering puts the cursor on lesson zero",
        "        self.selected_lesson = self.filtered_lessons().first().copied().unwrap_or(0);",
        "        self.selected_lesson = 0;",
        ["filtering_moves_the_cursor_to_a_lesson_the_filter_admits"],
    ),
    # -- Scrolling -------------------------------------------------------
    (
        "the wheel moves the list the wrong way",
        "        let next = if dy > 0.0 {\n"
        "            self.scroll_offset.saturating_sub(step)\n"
        "        } else if dy < 0.0 {\n"
        "            self.scroll_offset.saturating_add(step).min(max_offset)",
        "        let next = if dy < 0.0 {\n"
        "            self.scroll_offset.saturating_sub(step)\n"
        "        } else if dy > 0.0 {\n"
        "            self.scroll_offset.saturating_add(step).min(max_offset)",
        ["the_wheel_reveals_lessons_that_were_off_the_bottom"],
    ),
    (
        "a wheel notch that changes nothing asks for a repaint",
        "        if next == self.scroll_offset {\n"
        "            // A wheel notch at the end of the list changed nothing, and\n"
        "            // saying otherwise asks for a repaint of an identical picture.\n"
        "            return EventResult::Ignored;\n"
        "        }\n",
        "",
        ["a_wheel_notch_that_moves_nothing_is_ignored"],
    ),
    (
        "the cursor walks off the bottom without bringing the list with it",
        "        if here < self.scroll_offset {\n"
        "            self.scroll_offset = here;\n"
        "        } else if here >= self.scroll_offset.saturating_add(capacity) {\n"
        "            self.scroll_offset = here.saturating_sub(capacity.saturating_sub(1));\n"
        "        }",
        "        if here < self.scroll_offset {\n            self.scroll_offset = here;\n        }",
        ["the_cursor_key_scrolls_the_list_to_follow_the_cursor"],
    ),
    (
        "the cursor is anchored to the top of the list rather than its bottom",
        "            self.scroll_offset = here.saturating_sub(capacity.saturating_sub(1));",
        "            self.scroll_offset = here;",
        # Still keeps the cursor on screen, so a visibility-only test passes;
        # what it throws away is the page the typist has just read.
        ["the_cursor_key_scrolls_the_list_to_follow_the_cursor"],
    ),
    (
        "the offset is left where a longer list put it",
        "        let max_offset = self.filtered_lessons().len().saturating_sub(capacity);\n"
        "        self.scroll_offset = self.scroll_offset.min(max_offset);",
        "        let _ = self.filtered_lessons();",
        ["a_window_that_grows_shows_the_rows_the_scroll_had_hidden"],
    ),
    # -- The typing panel ------------------------------------------------
    (
        "the panel shows the top of the text rather than following the typist",
        "    let first_line = cursor_line.saturating_sub(lines_visible.saturating_sub(1));",
        "    let first_line = 0usize;",
        ["the_typing_panel_follows_the_cursor"],
    ),
    (
        "the pen advances by an approximate character width",
        "        let advance = text::measure_in(glyph, size, weight, FontFamily::Mono);",
        "        let advance = 13.2;",
        ["typed_characters_are_placed_on_measured_advances"],
    ),
    (
        "the text is drawn in the proportional face it is not measured in",
        "    f.push(RenderCommand::PushFont {\n        family: FontFamily::Mono,\n    });\n",
        "",
        ["the_lesson_text_is_drawn_in_the_family_it_is_measured_in"],
    ),
    (
        "a line breaks at a fixed width rather than at the edge of the panel",
        "        if x > 0.0 && x + advance > area.w {",
        "        if x > 0.0 && x + advance > 600.0 {",
        ["nothing_is_painted_outside_the_window"],
    ),
    (
        "a character is coloured by where it is rather than how it was typed",
        "            match session.statuses.get(i) {\n"
        "                Some(CharStatus::Correct) => hex(COL_GREEN),\n"
        "                Some(CharStatus::Incorrect) => hex(COL_RED),\n"
        "                _ => hex(COL_SURFACE2),\n"
        "            }",
        "            match session.statuses.get(i) {\n"
        "                Some(_) if i < session.cursor => hex(COL_GREEN),\n"
        "                _ => hex(COL_SURFACE2),\n"
        "            }",
        ["each_character_is_coloured_by_how_it_was_typed"],
    ),
    (
        "a key that types nothing asks for a repaint",
        "        if typed_any {\n"
        "            EventResult::Consumed\n"
        "        } else {\n"
        "            EventResult::Ignored\n"
        "        }",
        "        let _ = typed_any;\n        EventResult::Consumed",
        ["a_key_that_types_nothing_does_not_ask_for_a_repaint"],
    ),
    (
        "the progress bar is full from the first keystroke",
        "        Rect::new(track.x, track.y, track.w * progress, track.h),",
        "        Rect::new(track.x, track.y, track.w, track.h),",
        ["the_progress_bar_fills_by_as_much_as_was_typed"],
    ),
    # -- The clock, end to end -------------------------------------------
    (
        "the window loop is never asked for a clock",
        "    fn tick_interval(&self) -> Option<Duration> {\n        Some(Duration::from_millis(100))\n    }",
        "    fn tick_interval(&self) -> Option<Duration> {\n        None\n    }",
        ["the_app_asks_the_window_loop_for_a_clock"],
    ),
    (
        "the tick arrives and is thrown away",
        "                self.advance_time(*elapsed_ms);",
        "                let _ = elapsed_ms;",
        ["typing_then_ticking_produces_a_real_speed"],
    ),
    (
        "the lesson list asks to be redrawn on every tick",
        "                if self.view == AppView::Typing {\n"
        "                    EventResult::Consumed\n"
        "                } else {\n"
        "                    EventResult::Ignored\n"
        "                }",
        "                EventResult::Consumed",
        ["only_the_view_with_a_clock_repaints_on_a_tick"],
    ),
    # -- The window ------------------------------------------------------
    (
        "closing the window is handled as an ordinary event",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }\n",
        "",
        ["a_close_request_exits"],
    ),
    (
        "an ignored event asks for a repaint",
        "        match self.handle_event(event) {\n"
        "            EventResult::Consumed => Response::Redraw,\n"
        "            EventResult::Ignored => Response::Idle,\n"
        "        }",
        "        let _ = self.handle_event(event);\n        Response::Redraw",
        ["the_window_repaints_for_what_the_app_used_and_not_for_what_it_did_not"],
    ),
    (
        "the frame is drawn at the size given and the size forgotten",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["rendering_remembers_the_size_the_frame_was_drawn_at"],
    ),
    (
        "the window opens at a size nothing was designed for",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (200, 120)",
        ["the_window_is_named_and_identified"],
    ),
    (
        "the background is painted at the size the program was written for",
        "        fill(&mut f, l.window, hex(COL_BASE), CornerRadii::ZERO);",
        "        fill(\n"
        "            &mut f,\n"
        "            Rect::new(0.0, 0.0, WINDOW_WIDTH, WINDOW_HEIGHT),\n"
        "            hex(COL_BASE),\n"
        "            CornerRadii::ZERO,\n"
        "        );",
        ["the_background_covers_the_window_at_every_size"],
    ),
    # -- The list, the results and the history ---------------------------
    (
        "an unselected row is filled with the background",
        "                hex(if selected { COL_SURFACE0 } else { COL_MANTLE }),",
        "                hex(if selected { COL_SURFACE0 } else { COL_BASE }),",
        ["an_unselected_row_is_told_apart_from_the_background"],
    ),
    (
        "the lesson subtitle counts bytes",
        "                lesson.text.chars().count()",
        "                lesson.text.len()",
        ["a_lesson_subtitle_counts_characters_not_bytes"],
    ),
    (
        "the rating bands are off by one keystroke at every boundary",
        "    if wpm >= 80.0 {",
        "    if wpm > 80.0 {",
        ["the_rating_bands_are_where_they_say_they_are"],
    ),
    (
        "a results card rounds away the figure it names",
        '            ("Accuracy", format!("{:.1}%", session.accuracy()), COL_GREEN),',
        '            ("Accuracy", format!("{:.0}%", session.accuracy()), COL_GREEN),',
        ["every_results_card_shows_its_own_figure"],
    ),
    (
        "the results text starts at a constant rather than below the cards",
        "        let bottom = draw_cards(f, l, &cards);\n        let rating = format!",
        "        draw_cards(f, l, &cards);\n        let bottom = 240.0;\n        let rating = format!",
        # Not `every_results_card_shows_its_own_figure`: that reads the card
        # text, which a constant placed below the cards does not touch. The
        # only thing that noticed was the geometric catch-all, and only at the
        # sizes where 240 itself fell outside the window -- silence about the
        # actual damage, which is the rating printed over the cards, well
        # inside it.
        ["the_results_text_sits_below_the_cards_it_summarises"],
    ),
    (
        "the history is oldest first",
        "        for (n, result) in self.results.iter().rev().take(capacity.min(8)).enumerate() {",
        "        for (n, result) in self.results.iter().take(capacity.min(8)).enumerate() {",
        ["the_history_table_is_newest_first"],
    ),
    (
        "the history row is drawn in the text colour, so its category is lost",
        "                (result.lesson_title.clone(), result.category.color()),",
        "                (result.lesson_title.clone(), hex(COL_TEXT)),",
        ["a_history_row_carries_its_category_colour"],
    ),
    (
        "the history columns are at the old pixel positions",
        "        let col_x = |i: usize| area.x + area.w * SHARES.get(i).copied().unwrap_or(0.0);",
        "        let col_x = |i: usize| area.x + [30.0f32, 250.0, 340.0, 440.0]"
        ".get(i).copied().unwrap_or(0.0);",
        ["nothing_is_painted_outside_the_window"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "typingtutor", timeout=240, only=sys.argv[1:] or None))

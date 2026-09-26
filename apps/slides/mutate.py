"""Mutation test for the slide editor's pointer layer, its decks and its show.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The editor drew a toolbar, thumbnails, a slide with its elements, a property
panel and a notes panel, and handled no pointer event (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED).  Nothing a
layout put on a slide could be selected, undo and redo had no key, the notes
could not be written, five of six layouts were unreachable, decks could not be
saved, and the transitions every slide chose were never played.

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
    # -- keys ----------------------------------------------------------------------------------------
    (
        "undo has no key",
        "            Key::Z if ctrl => self.undo_if_any(),",
        "            Key::Z if ctrl => EventResult::Ignored,",
        ["undo_and_redo_have_keys"],
    ),
    (
        "redo has no key",
        "            Key::Y if ctrl => self.redo_if_any(),",
        "            Key::Y if ctrl => EventResult::Ignored,",
        ["undo_and_redo_have_keys", "every_advertised_key_does_something"],
    ),
    (
        "Tab walks nothing",
        "            Key::Tab if editing_view => self.select_next_element(!shift),",
        "            Key::Tab if editing_view => EventResult::Ignored,",
        ["tab_walks_the_elements_and_wraps", "a_layouts_title_can_be_selected_and_typed_into"],
    ),
    (
        "Shift+Tab walks forward",
        "            Key::Tab if editing_view => self.select_next_element(!shift),",
        "            Key::Tab if editing_view => self.select_next_element(true),",
        ["tab_walks_the_elements_and_wraps"],
    ),
    (
        "Ctrl+M opens no menu",
        "            Key::M if ctrl => {\n                self.layout_menu = Some(1);",
        "            Key::M if ctrl => {\n                self.layout_menu = None;",
        ["every_layout_can_be_added"],
    ),
    (
        "the menu's 3 adds the second layout",
        "            Key::Num3 => Some(2),",
        "            Key::Num3 => Some(1),",
        ["every_layout_can_be_added"],
    ),
    (
        "N does not type the notes",
        "            Key::N => self.begin_notes(),",
        "            Key::N => EventResult::Ignored,",
        ["the_speaker_notes_can_be_written"],
    ),
    (
        "the shortcut list lets keys through",
        "        if self.show_help {\n            let closes",
        "        if self.show_help && false {\n            let closes",
        ["the_shortcut_list_is_modal"],
    ),
    # -- selecting, typing, moving -------------------------------------------------------------
    (
        "a press selects nothing",
        "                let edit = self.selected_element == Some(id);\n                self.selected_element = Some(id);",
        "                let edit = self.selected_element == Some(id);",
        ["a_press_selects_and_a_second_types_into_the_element"],
    ),
    (
        "a second press never types",
        "                if !moved && edit {",
        "                if false && !moved && edit {",
        ["a_press_selects_and_a_second_types_into_the_element"],
    ),
    (
        "an element records no hit box",
        "            f.hit(Target::Element(element.id()), self.element_rect(element));",
        "",
        [
            "a_press_selects_and_a_second_types_into_the_element",
            "dragging_an_element_moves_it",
            "a_flat_line_can_be_pressed",
        ],
    ),
    (
        "a flat line has no height to press",
        "        if r.h < 6.0 {",
        "        if false {",
        ["a_flat_line_can_be_pressed"],
    ),
    (
        "a drag moves nothing",
        "                let (nx, ny) = keep_on_slide(origin.0 + dx, origin.1 + dy, w, h);\n"
        "                element.set_position(nx, ny);",
        "                let (nx, ny) = keep_on_slide(origin.0 + dx, origin.1 + dy, w, h);\n"
        "                let _ = (nx, ny);",
        ["dragging_an_element_moves_it"],
    ),
    (
        "a drag can lose an element off the slide",
        "                let (nx, ny) = keep_on_slide(origin.0 + dx, origin.1 + dy, w, h);",
        "                let (nx, ny) = (origin.0 + dx + 0.0 * w, origin.1 + dy + 0.0 * h);",
        ["a_drag_cannot_lose_an_element_off_the_slide"],
    ),
    (
        "a drag is not one undo step",
        "                // A press that wanders a pixel or two is still a press.\n"
        "                if !moved && (dx.abs() + dy.abs()) * scale < 3.0 {\n"
        "                    return EventResult::Ignored;\n                }\n"
        "                if !moved {\n                    self.checkpoint();\n                }",
        "                // A press that wanders a pixel or two is still a press.\n"
        "                if !moved && (dx.abs() + dy.abs()) * scale < 3.0 {\n"
        "                    return EventResult::Ignored;\n                }",
        ["dragging_an_element_moves_it"],
    ),
    (
        "the handles record no hit box",
        "                    f.hit(Target::Handle(corner), handle);",
        "",
        ["a_corner_handle_resizes_and_the_opposite_corner_stays"],
    ),
    (
        "a resize has no smallest size",
        "resized(bounds, corner, dx, dy, min_size(element))",
        "resized(bounds, corner, dx, dy, -1.0e9 + 0.0 * min_size(element))",
        ["a_corner_handle_resizes_and_the_opposite_corner_stays"],
    ),
    (
        "leaving the window keeps the drag",
        "                let dragged = self.drag.take().is_some();",
        "                let dragged = self.drag.is_some();",
        ["leaving_the_window_ends_a_drag"],
    ),
    (
        "the pointer lights nothing",
        "                self.hover = over;\n                EventResult::Consumed",
        "                let _ = over;\n                EventResult::Consumed",
        ["hovering_a_button_lights_it"],
    ),
    # -- the panels and the toolbar ---------------------------------------------------------------
    (
        "the toolbar records no hit boxes",
        "            self.button(f, rect, &label, Target::Tool(tool), enabled);",
        "            self.button(f, rect, &label, Target::Tool(tool), false && enabled);",
        ["every_toolbar_button_answers_the_pointer"],
    ),
    (
        "undo can be pressed with nothing to undo",
        "                String::from(\"Undo\"),\n                54.0,\n                self.undo_mgr.can_undo(),",
        "                String::from(\"Undo\"),\n                54.0,\n                true,",
        ["undo_cannot_be_pressed_with_nothing_to_undo"],
    ),
    (
        "bold does nothing",
        "            Prop::Bold => self.toggle_bold(),",
        "            Prop::Bold => EventResult::Ignored,",
        ["the_property_buttons_change_the_selected_text_box"],
    ),
    (
        "the outline does not step",
        "                let next = if up {\n                    *stroke_width + 1.0",
        "                let next = if up {\n                    *stroke_width + 0.0",
        ["the_panel_changes_the_slide_and_a_shapes_outline"],
    ),
    (
        "the background never changes",
        "            slide.background = next;",
        "            let _ = next;",
        ["the_panel_changes_the_slide_and_a_shapes_outline"],
    ),
    (
        "a bullet list is not written back",
        "                        *items = buf",
        "                        let _: Vec<String> = buf",
        ["a_bullet_list_is_typed_one_line_per_bullet"],
    ),
    (
        "a blank line is a bullet",
        "                            .filter(|line| !line.trim().is_empty())",
        "                            .filter(|_| true)",
        ["a_bullet_list_is_typed_one_line_per_bullet"],
    ),
    (
        "an image label is not kept",
        "                        *placeholder_label = buf.trim().to_owned();",
        "                        let _ = buf;",
        ["an_image_label_can_be_typed"],
    ),
    (
        "the notes are not kept",
        "                self.set_current_notes(buf.to_owned());",
        "",
        ["the_speaker_notes_can_be_written"],
    ),
    # -- the theme --------------------------------------------------------------------------------------
    (
        "a theme change restyles nothing",
        "                        *color = colour(*color);",
        "                        let _ = colour(*color);",
        ["a_theme_change_restyles_what_the_theme_styled"],
    ),
    (
        "undo forgets the theme",
        "        self.theme = snap.theme;",
        "        let _ = snap.theme;",
        ["a_theme_change_restyles_what_the_theme_styled"],
    ),
    # -- what is drawn --------------------------------------------------------------------------------
    (
        "a text box draws one line",
        "        for line in text.split('\\n') {",
        "        for line in [text] {",
        ["a_text_box_draws_each_of_its_lines"],
    ),
    (
        "centred text is nudged in again",
        "guitk::text::center_x(line, fx + fw / 2.0, fs, weight).max(fx)",
        "(fx + fw * 0.1) + 0.0 * guitk::text::center_x(line, fx + fw / 2.0, fs, weight)",
        ["centred_text_is_centred"],
    ),
    (
        "the arrowhead is lopsided",
        "                                y2: hy - head * (uy - side * 0.5 * ux),",
        "                                y2: hy - head * (uy - side.max(0.0) * 0.5 * ux),",
        ["an_arrowhead_is_symmetric_about_its_line"],
    ),
    # -- the thumbnails -------------------------------------------------------------------------------
    (
        "a thumbnail press goes nowhere",
        "                if !was_current {\n                    self.go_to_slide(i);\n                }",
        "",
        [
            "the_thumbnail_column_scrolls_and_follows_the_current_slide",
            "the_sorter_scrolls_and_a_second_press_opens_a_slide",
        ],
    ),
    (
        "a thumbnail drag moves nothing",
        "                        self.move_slide(from, to);",
        "",
        ["dragging_a_thumbnail_moves_its_slide"],
    ),
    (
        "the thumbnail column never scrolls",
        "            let ty = pane.y + THUMBNAIL_PAD + (i as f32) * pitch - self.sidebar_scroll;",
        "            let ty = pane.y + THUMBNAIL_PAD + (i as f32) * pitch;",
        ["the_thumbnail_column_scrolls_and_follows_the_current_slide"],
    ),
    (
        "the sorter never scrolls",
        "            let ty = g.pane.y + g.gap + (row as f32) * g.pitch - self.sorter_scroll;",
        "            let ty = g.pane.y + g.gap + (row as f32) * g.pitch;",
        ["the_sorter_scrolls_and_a_second_press_opens_a_slide"],
    ),
    (
        "the column does not follow the current slide",
        "        if top < self.sidebar_scroll {\n            self.sidebar_scroll = top;\n"
        "        } else if bottom > self.sidebar_scroll + pane.h {\n"
        "            self.sidebar_scroll = bottom - pane.h;\n        }",
        "",
        ["the_thumbnail_column_scrolls_and_follows_the_current_slide"],
    ),
    # -- decks on disk ------------------------------------------------------------------------------------
    (
        "a save asks every time",
        "        match self.deck_path.clone() {",
        "        match None::<std::path::PathBuf> {",
        ["ctrl_s_asks_the_first_time_and_saves_in_place_after"],
    ),
    (
        "a save leaves the deck marked unsaved",
        "                self.deck_path = Some(path.to_path_buf());\n                self.dirty = false;\n"
        "                format!(\"Saved",
        "                self.deck_path = Some(path.to_path_buf());\n                format!(\"Saved",
        [
            "a_deck_survives_being_saved_and_opened",
            "the_window_bar_marks_unsaved_changes",
            "ctrl_s_asks_the_first_time_and_saves_in_place_after",
        ],
    ),
    (
        "a change does not mark the deck",
        "        self.undo_mgr.save(now);\n        self.dirty = true;",
        "        self.undo_mgr.save(now);",
        ["the_window_bar_marks_unsaved_changes", "open_asks_before_losing_unsaved_changes"],
    ),
    (
        "open does not ask",
        "            self.ask(Pending::Open);",
        "            let _ = Pending::Open;",
        ["open_asks_before_losing_unsaved_changes"],
    ),
    (
        "closing over unsaved changes does not ask",
        "        if !self.dirty {\n            return true;\n        }",
        "        if true {\n            return true;\n        }",
        ["closing_over_unsaved_changes_asks_and_each_answer_is_kept"],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "            } else {\n                Response::KeepOpen\n            };",
        "            } else {\n                Response::Redraw\n            };",
        ["closing_over_unsaved_changes_asks_and_each_answer_is_kept"],
    ),
    (
        "saving before going on does not go on",
        "                    self.status_message = Some(said);\n"
        "                    if !self.dirty {\n"
        "                        self.go_on(pending);\n"
        "                    }",
        "                    self.status_message = Some(said);",
        [
            "closing_over_unsaved_changes_asks_and_each_answer_is_kept",
            "open_can_save_the_deck_first",
        ],
    ),
    (
        "a save that failed goes on anyway",
        "                    self.status_message = Some(said);\n"
        "                    if !self.dirty {",
        "                    self.status_message = Some(said);\n"
        "                    if true {",
        ["saving_on_close_asks_where_and_a_failure_keeps_the_window"],
    ),
    (
        "a deck saved where the picker said does not go on",
        "                let said = self.write_deck(path);\n"
        "                if !self.dirty {\n"
        "                    self.go_on(pending);\n"
        "                }",
        "                let said = self.write_deck(path);",
        ["saving_on_close_asks_where_and_a_failure_keeps_the_window"],
    ),
    (
        "keys and clicks reach the deck under the question",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && false",
        [
            "open_asks_before_losing_unsaved_changes",
            "closing_over_unsaved_changes_asks_and_each_answer_is_kept",
        ],
    ),
    (
        "words being typed are dropped when the window closes",
        "        // Words being typed into a box are part of the deck.\n"
        "        if let Some((edit, buf)) = self.editing.take() {\n"
        "            self.commit_editing(edit, &buf);\n"
        "        }",
        "        // Words being typed into a box are part of the deck.\n"
        "        self.editing = None;",
        ["words_being_typed_when_the_window_closes_are_asked_about"],
    ),
    (
        "an edit that changed nothing marks the deck",
        "                if self.element_words(eid).as_deref() == Some(buf) {\n"
        "                    return;\n"
        "                }\n",
        "",
        ["typing_nothing_is_no_change"],
    ),
    (
        "a show hides the question",
        "        self.show = None;\n        self.picker.close();",
        "        self.picker.close();",
        ["closing_during_a_show_ends_it_to_ask"],
    ),
    (
        "the picker's answer to Save exports",
        "            PickerFor::Save => self.write_deck(path),",
        "            PickerFor::Save => self.write_html(path),",
        ["ctrl_s_asks_the_first_time_and_saves_in_place_after"],
    ),
    (
        "a later format is taken for no deck",
        "        Some(later) if later > DECK_FORMAT => {",
        "        Some(later) if later > DECK_FORMAT + 100 => {",
        ["a_file_that_is_not_a_deck_is_refused"],
    ),
    (
        "an unknown element becomes a rectangle",
        "                    .find(|k| shape_name(*k) == other) else {",
        "                    .find(|k| shape_name(*k) == other || !other.is_empty()) else {",
        ["an_element_of_an_unknown_kind_is_left_out"],
    ),
    (
        "what a save or an export did is not shown",
        "            && let Some(message) = &self.status_message",
        "            && let Some(message) = None::<&String>",
        ["what_a_save_or_an_export_did_is_shown"],
    ),
    # -- the show --------------------------------------------------------------------------------------------
    (
        "F5 presents nothing",
        "            Key::F5 => self.present(0),",
        "            Key::F5 => EventResult::Ignored,",
        ["f5_presents_from_the_start_and_shift_f5_from_this_slide"],
    ),
    (
        "Shift+F5 presents from the start",
        "            Key::F5 if shift => self.present(self.current_index),",
        "            Key::F5 if shift => self.present(0),",
        ["f5_presents_from_the_start_and_shift_f5_from_this_slide"],
    ),
    (
        "the show's keys reach the editor",
        "        if self.show.is_some() {\n            return self.handle_show_key(key);\n        }",
        "",
        ["the_show_moves_by_key_and_press_and_ends_on_the_slide_shown"],
    ),
    (
        "a press does not go on",
        "                MouseEventKind::Press(MouseButton::Left) => self.show_next(),",
        "                MouseEventKind::Press(MouseButton::Left) => EventResult::Ignored,",
        ["the_show_moves_by_key_and_press_and_ends_on_the_slide_shown"],
    ),
    (
        "ending the show forgets the slide shown",
        "        self.go_to_slide(show.index);",
        "        let _ = show.index;",
        ["the_show_moves_by_key_and_press_and_ends_on_the_slide_shown"],
    ),
    (
        "a transition never plays",
        "        let leaving = (slide.transition != Transition::None).then_some((show.index, 0));",
        "        let leaving = None;",
        [
            "a_transition_plays_over_the_ticks_and_the_clock_stops",
            "each_transition_draws_both_slides_on_the_way",
        ],
    ),
    (
        "a transition never ends",
        "            leaving: (done < TRANSITION_MS).then_some((from, done)),",
        "            leaving: Some((from, done)),",
        [
            "a_transition_plays_over_the_ticks_and_the_clock_stops",
            "each_transition_draws_both_slides_on_the_way",
        ],
    ),
    (
        "the clock never runs",
        "            .map(|_| Duration::from_millis(16))",
        "            .and(None::<Duration>)",
        ["a_transition_plays_over_the_ticks_and_the_clock_stops"],
    ),
    (
        "the fade does not darken",
        "                let alpha = (dark * 255.0).round().clamp(0.0, 255.0) as u8;",
        "                let alpha = 0.0f32.max(dark * 0.0) as u8;",
        [
            "each_transition_draws_both_slides_on_the_way",
            "each_transition_is_a_quarter_done_a_quarter_of_the_way",
        ],
    ),
    (
        "Slide Left slides right",
        "                let dir = if transition == Transition::SlideLeft {\n                    -1.0",
        "                let dir = if transition == Transition::SlideLeft {\n                    1.0",
        ["each_transition_is_a_quarter_done_a_quarter_of_the_way"],
    ),
    (
        "a wipe shows everything at once",
        "                f.clip(Rect::new(x, y, sw * p, sh));",
        "                f.clip(Rect::new(x, y, sw, sh));",
        ["each_transition_is_a_quarter_done_a_quarter_of_the_way"],
    ),
    (
        "a dissolve reveals every cell at once",
        "                let shown = ((p * cells as f32).round() as usize).min(cells);",
        "                let shown = cells;",
        ["each_transition_is_a_quarter_done_a_quarter_of_the_way"],
    ),
    (
        "the export plays no transition",
        '                other => format!(" t-{}", transition_name(other)),',
        "                _other => String::new(),",
        ["the_export_plays_each_slides_transition"],
    ),
    (
        "the export draws lines flat",
        'x2=\\"{width}\\" y2=\\"{height}\\" \\',
        'x2=\\"{width}\\" y2=\\"0\\" \\',
        ["the_export_draws_lines_along_their_direction"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "slides", timeout=900, only=only))

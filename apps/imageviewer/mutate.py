"""Mutation test for the image viewer's pointer layer and its Open dialog.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26.  The toolbar and the thumbnail
strip were drawn -- the toolbar with a hover colour and tooltips naming keys
-- and the mouse handler never looked at either: a press over the toolbar was
"not mine", and a press on the strip began a pan.  The Open button and Ctrl+O
were a comment reading "in a real implementation, this would open a file
dialog", so a viewer started with no file could not be given one.  Now one
reading of the window's layout (`ViewerState::layout`) places each part for
the renderer and for the hit-tests alike.

Later the same day: pictures decoded off the window's thread (`offloop`), the
last one kept up until the next is ready; Rotate and Flip turning the picture
itself, as one of the eight views, each change applied to what is on screen;
a picture opening whole and following the window until the user zooms or
pans; zoom steps as factors; SVG drawings; the file's real date.

The loader's own rule -- a result for a superseded request is never handed
back -- is `offloop`'s, and is swept by `apps/offloop/mutate.py`.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

BUTTONS = "every_toolbar_button_answers_a_click"
HIDDEN = "a_hidden_toolbar_takes_no_click"
LIT = "the_button_under_the_pointer_is_lit"
THUMB = "a_thumbnail_opens_its_picture"
PAN = "only_a_press_on_the_picture_starts_a_pan"
CTRL_O = "ctrl_o_asks_for_a_picture_beside_the_last"
CHOSEN = "a_picture_chosen_in_the_dialog_opens"
TILES = "the_layout_tiles_the_window"
CENTRED = "the_strip_is_centred_on_the_current_picture"
DOUBLE = "a_double_click_on_a_button_does_not_zoom"
OFF_LOOP = "a_picture_is_decoded_off_the_window_and_drawn_when_it_wakes_it"
STAYS_UP = "the_last_picture_stays_up_until_the_next_is_ready"
TURN = "a_turn_applies_to_the_picture_on_screen"
TURNS = "rotating_and_flipping_turn_the_picture_itself"
LEFT_DOWN = "turning_left_and_flipping_down_turn_the_picture"
REFIT_TURN = "a_turn_refits_a_fitted_picture_and_keeps_a_chosen_zoom"
WHOLE = "a_picture_opens_whole_and_no_larger_than_itself"
FOLLOWS = "a_fitted_picture_follows_the_window_until_the_user_zooms"
DRAGGED = "a_dragged_picture_stays_where_it_was_put"
STEP = "a_zoom_step_is_a_factor_and_stops_at_actual_size"
NOTCH = "one_wheel_notch_is_one_zoom_step"
LARGE = "a_large_photograph_still_fits_the_window"
SVG = "an_svg_drawing_opens"
SVG_KNOWN = "a_drawing_is_known_by_its_first_element"
DATE = "the_info_panel_says_when_the_file_was_changed"
FAILS_OFF = "a_failure_off_the_window_takes_the_last_picture_down"

MUTATIONS = [
    # -- the layout ---------------------------------------------------------
    (
        "full screen keeps the bars",
        "        let bars = !self.fullscreen;",
        "        let bars = true;",
        [TILES, HIDDEN],
    ),
    (
        "the picture starts under nothing",
        "        let top = if toolbar.is_some() {\n            TOOLBAR_HEIGHT\n        } else {\n            0.0\n        };",
        "        let top = 0.0;",
        [TILES],
    ),
    (
        "the strip ignores the status bar",
        "            .then_some(bottom - THUMBNAIL_STRIP_HEIGHT);",
        "            .then_some(self.window_height - THUMBNAIL_STRIP_HEIGHT);",
        [TILES],
    ),
    (
        "the picture runs under the info panel",
        "        let image_width = if self.show_info_panel {\n            self.window_width - INFO_PANEL_WIDTH",
        "        let image_width = if self.show_info_panel {\n            self.window_width",
        # Not PAN: the panel moves out with the picture's edge, off the
        # window, so a press where it was is still outside the picture.
        [TILES],
    ),
    (
        "the picture is over only by its left and top",
        "        x >= left && x < left + width && y >= top && y < top + height",
        "        x >= left && y >= top",
        [PAN],
    ),
    # -- the toolbar --------------------------------------------------------
    (
        "a press on a toolbar button is not taken",
        "                if let Some(button) = self.toolbar_button_at(event.x, event.y) {",
        "                if let Some(button) = None::<usize> {",
        [BUTTONS],
    ),
    (
        "a toolbar press runs no action",
        "                        self.execute_action(action);\n",
        "                        let _ = action;\n",
        [BUTTONS],
    ),
    (
        "a button answers below itself",
        "        if y < top || y >= top + height {\n            return None;\n        }\n        toolbar_buttons()",
        "        let _ = (top, height, y);\n        toolbar_buttons()",
        [PAN],
    ),
    (
        "a button is found by its left edge alone",
        "            .position(|b| x >= b.x && x < b.x + b.width)",
        "            .position(|b| x >= b.x)",
        [BUTTONS],
    ),
    (
        "the hovered button is not recorded",
        "                self.hovered_button = hovered;\n",
        "",
        [LIT],
    ),
    (
        "every move redraws",
        "                self.hovered_button = hovered;\n                changed",
        "                self.hovered_button = hovered;\n                let _ = changed;\n                true",
        [LIT],
    ),
    (
        "a double click on a button changes the zoom",
        "                if !self.layout().in_image(event.x, event.y) =>",
        "                if false =>",
        [DOUBLE],
    ),
    # -- the thumbnail strip ----------------------------------------------------
    (
        "a press on a thumbnail is not taken",
        "                    self.go_to_entry(entry);\n                    return true;",
        "                    let _ = entry;\n                    return true;",
        [THUMB],
    ),
    (
        "going to a thumbnail's picture does not show it",
        "            self.current_index = index;\n            self.load_current_entry();",
        "            self.current_index = index;",
        [THUMB],
    ),
    (
        "a thumbnail answers above itself",
        "        if y < top || y >= top + THUMB_SIZE {\n            return None;\n        }",
        "        let _ = (top, y);",
        [THUMB],
    ),
    (
        "a thumbnail is found by its left edge alone",
        "            .find(|&(_, left)| x >= left && x < left + THUMB_SIZE)",
        "            .find(|&(_, left)| x >= left)",
        [THUMB],
    ),
    (
        "the strip starts at the first picture",
        "        let start = self.current_index.saturating_sub(room / 2);",
        "        let start: usize = 0;",
        [CENTRED],
    ),
    (
        "the strip lays thumbnails on one another",
        "            .map(|(entry, slot)| (entry, f32::from(slot) * THUMB_PITCH + THUMB_PAD))",
        "            .map(|(entry, _)| (entry, THUMB_PAD))",
        [CENTRED],
    ),
    # -- panning ------------------------------------------------------------
    (
        "a pan starts anywhere",
        "                if self.layout().in_image(event.x, event.y) {\n                    self.dragging = true;",
        "                if true {\n                    self.dragging = true;",
        [PAN],
    ),
    # -- the Open dialog ----------------------------------------------------
    (
        "Ctrl+O does nothing",
        "            Key::O if ctrl => {\n                self.execute_action(ViewerAction::Open);\n                true",
        "            Key::O if ctrl => {\n                true",
        [CTRL_O],
    ),
    (
        "Open does nothing",
        "            ViewerAction::Open => self.ask_for_a_picture(),",
        "            ViewerAction::Open => {}",
        [CTRL_O, CHOSEN],
    ),
    (
        "the dialog opens at home, not beside the picture",
        "            .filter(|dir| dir.is_dir())",
        "            .filter(|_| false)",
        [CTRL_O],
    ),
    (
        "the dialog lists every file",
        "                .with_initial_path(start)\n                .with_filter(\"Pictures\", &patterns),",
        "                .with_initial_path(start),",
        [CTRL_O],
    ),
    (
        "the viewer takes the keys meant for the dialog",
        "        if self.picker.is_open() && self.picker_took(event) {\n            return true;\n        }",
        "",
        [CTRL_O, CHOSEN],
    ),
    # -- the loader -----------------------------------------------------------
    (
        "a picture is decoded here even with a loader",
        "            Some(loader) => loader.ask((path, view)),",
        "            Some(_) => Err((path, view)),",
        [OFF_LOOP],
    ),
    (
        "the viewer asks for no waker",
        "    fn wants_waker(&self) -> bool {\n        true",
        "    fn wants_waker(&self) -> bool {\n        false",
        [OFF_LOOP],
    ),
    (
        "no loader is started",
        "        self.loader = offloop::Latest::start(",
        "        self.loader = None;\n        let _unused = offloop::Latest::start(",
        [OFF_LOOP],
    ),
    (
        "a picture ready is not redrawn",
        "                let _ = self.apply_loaded(loaded);\n                oswindow::app::Response::Redraw",
        "                let _ = self.apply_loaded(loaded);\n                oswindow::app::Response::Idle",
        [OFF_LOOP],
    ),
    (
        "nothing is ever loading",
        "        let busy = self.loader.as_ref().is_some_and(offloop::Latest::busy);",
        "        let busy = false;",
        [OFF_LOOP, STAYS_UP],
    ),
    (
        "the status bar does not say what is coming",
        "    let name = state.loading().map_or_else(",
        "    let name = None::<&Path>.map_or_else(",
        [STAYS_UP],
    ),
    (
        "the empty canvas does not say what is coming",
        "        let (headline, detail) = match (state.loading(), &state.load_error) {",
        "        let (headline, detail) = match (None::<&Path>, &state.load_error) {",
        [OFF_LOOP],
    ),
    (
        "a failure names no file",
        "        Err(e) => return failed(info, format!(\"{}: {e}\", shown_path(path))),",
        "        Err(e) => return failed(info, format!(\"{e}\")),",
        [FAILS_OFF],
    ),
    # -- turning ------------------------------------------------------------
    (
        "Rotate turns nothing",
        "            ViewerAction::RotateCw => self.turn(View::rotated_cw),",
        "            ViewerAction::RotateCw => {}",
        [TURNS, BUTTONS],
    ),
    (
        "Rotate left turns right",
        "            ViewerAction::RotateCcw => self.turn(View::rotated_ccw),",
        "            ViewerAction::RotateCcw => self.turn(View::rotated_cw),",
        [LEFT_DOWN],
    ),
    (
        "Flip down flips across",
        "            ViewerAction::FlipVertical => self.turn(View::flipped_down),",
        "            ViewerAction::FlipVertical => self.turn(View::flipped_across),",
        [LEFT_DOWN],
    ),
    (
        "a turn re-reads nothing",
        "            let _ = self.request(path, how(view));",
        "            let _ = (path, how(view));",
        [TURNS],
    ),
    (
        "a quarter turn is three",
        "            quarter_turns: self.quarter_turns.wrapping_add(1) & 3,",
        "            quarter_turns: self.quarter_turns.wrapping_add(3) & 3,",
        [TURN],
    ),
    (
        "a flip across after a turn flips along the other axis",
        "            quarter_turns: 4_u8.wrapping_sub(self.quarter_turns) & 3,",
        "            quarter_turns: self.quarter_turns,",
        [TURN],
    ),
    (
        "a flip down after a turn flips along the other axis",
        "            quarter_turns: 6_u8.wrapping_sub(self.quarter_turns) & 3,",
        "            quarter_turns: 2_u8.wrapping_add(self.quarter_turns) & 3,",
        [TURN],
    ),
    (
        "the two diagonals are swapped",
        "            (true, 1) => Orientation::RightBottom,",
        "            (true, 1) => Orientation::LeftTop,",
        [TURN],
    ),
    # -- fitting and zooming ----------------------------------------------------
    (
        "a turn drops the user's zoom",
        "        if new_file {\n            self.transform.reset();\n        }",
        "        let _ = new_file;\n        self.transform.reset();",
        [REFIT_TURN],
    ),
    (
        "a new file keeps the last one's zoom",
        "        if new_file {\n            self.transform.reset();\n        }",
        "        let _ = new_file;",
        [WHOLE],
    ),
    (
        "a picture opens enlarged",
        "            Fit::Shrink => self.fit_zoom().min(1.0),",
        "            Fit::Shrink => self.fit_zoom(),",
        [WHOLE, FOLLOWS],
    ),
    (
        "a loaded picture is not fitted",
        "        if new_file {\n            self.transform.reset();\n        }\n        self.refit();",
        "        if new_file {\n            self.transform.reset();\n        }",
        [WHOLE, REFIT_TURN],
    ),
    (
        "a fitted picture does not follow the window",
        "        self.refit();\n        render(self)",
        "        render(self)",
        [FOLLOWS],
    ),
    (
        "Fit does not fill",
        "        self.transform.fit = Fit::Fill;",
        "        self.transform.fit = Fit::Shrink;",
        [WHOLE],
    ),
    (
        "actual size is fitted away",
        "        self.transform.pan_y = 0.0;\n        self.transform.fit = Fit::Free;\n    }",
        "        self.transform.pan_y = 0.0;\n    }",
        [FOLLOWS],
    ),
    (
        "a drag is fitted away",
        "                // Where the user put it, it stays: a resize no longer refits.\n                self.transform.fit = Fit::Free;",
        "",
        [DRAGGED],
    ),
    (
        "a zoom in is not the user's",
        "            next.min(MAX_ZOOM)\n        };\n        self.fit = Fit::Free;",
        "            next.min(MAX_ZOOM)\n        };",
        [FOLLOWS, STEP],
    ),
    (
        "a zoom out is not the user's",
        "            next.max(MIN_ZOOM)\n        };\n        self.fit = Fit::Free;",
        "            next.max(MIN_ZOOM)\n        };",
        [STEP],
    ),
    (
        "a zoom step adds",
        "        let next = self.zoom * ZOOM_FACTOR;",
        "        let next = self.zoom + 0.25;",
        [NOTCH, STEP],
    ),
    (
        "a zoom step out takes away",
        "        let next = self.zoom / ZOOM_FACTOR;",
        "        let next = self.zoom - 0.25;",
        [STEP],
    ),
    (
        "a zoom in steps past actual size",
        "        self.zoom = if self.zoom < 1.0 && next > 1.0 {",
        "        self.zoom = if false {",
        [STEP],
    ),
    (
        "a zoom out steps past actual size",
        "        self.zoom = if self.zoom > 1.0 && next < 1.0 {",
        "        self.zoom = if false {",
        [STEP],
    ),
    (
        "a camera's photograph cannot be fitted",
        "const MIN_ZOOM: f32 = 0.01;",
        "const MIN_ZOOM: f32 = 0.25;",
        [LARGE],
    ),
    # -- drawings and dates -------------------------------------------------------
    (
        "an SVG is not known",
        "        if looks_like_svg(data) {",
        "        if false {",
        [SVG],
    ),
    (
        "any XML is a drawing",
        "    text.starts_with(b\"<?xml\")\n        && text",
        "    text.starts_with(b\"<?xml\")\n        || text",
        [SVG_KNOWN],
    ),
    (
        "a byte-order mark hides a drawing",
        "    let text = data.strip_prefix(b\"\\xEF\\xBB\\xBF\").unwrap_or(data);",
        "    let text = data;",
        [SVG_KNOWN],
    ),
    (
        "a drawing is drawn at its own size",
        "    let scale = SVG_DRAWN_AT / width.max(height);",
        "    let scale = 1.0_f32;",
        [SVG],
    ),
    (
        "a drawing's red and blue are swapped",
        "            [r, g, b, a] => u32::from_be_bytes([a, r, g, b]),",
        "            [r, g, b, a] => u32::from_be_bytes([a, b, g, r]),",
        [SVG],
    ),
    (
        "the date is a placeholder again",
        "        .map(|since| {\n            guitk::datetime::stamp(\n                i64::try_from(since.as_secs()).unwrap_or(i64::MAX),\n                &guitk::tzrules::Tz::utc(),\n            )\n        });",
        "        .map(|_| String::from(\"(available)\"));",
        [DATE],
    ),
    (
        "a chosen picture is not opened",
        "                let _ = self.open_file(&path);",
        "                let _ = &path;",
        [CHOSEN],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "imageviewer", timeout=600, only=only))

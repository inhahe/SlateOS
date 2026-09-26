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
KEYS = "every_advertised_key_does_something"

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
        [TILES, PAN],
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
        "        let start = 0;",
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

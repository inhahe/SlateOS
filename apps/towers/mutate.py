"""Mutation test for towers's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table starts with the theme (the operator's answer to C-Q16, §1422): the
pegs, the base and the chrome follow the user's palette, the disks keep their
colours by size, and the buttons are the toolkit's.

Then the history (the operator's answer to C-Q24, §1416): a tree of moves,
where a move made after an undo keeps the one undone as a branch, reached with
Alt+Z; Ctrl+Z and Ctrl+Y or Ctrl+Shift+Z beside the game's own `Z`.

Usage:  python -u apps/towers/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

TREE = "a_move_after_an_undo_keeps_the_undone_one_reachable_with_alt_z"
CTRL = "ctrl_z_undoes_and_ctrl_y_and_ctrl_shift_z_redo"
ALTGR = "altgr_z_and_windows_alt_z_move_nothing"
WAITS = "the_history_waits_for_a_held_disk_and_stops_the_solver"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The theme ─────────────────────────────────────────────────────────
    (
        "the theme is never taken up",
        "    fn theme_changed(&mut self, palette: &Palette) {\n        self.palette = *palette;",
        "    fn theme_changed(&mut self, palette: &Palette) {\n        let _ = palette;",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "the help sheet's scrim is Mocha's whatever the theme",
        "    fill(f, l.window, c.chrome.scrim, 0.0);",
        "    fill(f, l.window, Color::rgba(0x1E, 0x1E, 0x2E, 158), 0.0);",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "a button that would do nothing is drawn live",
        "            disabled: !live,",
        "            disabled: false,",
        ["a_button_that_would_do_nothing_is_switched_off"],
    ),
    (
        "a disk's number is the pale text colour it was",
        "const DISK_INK: Color = Color::from_hex(0x161616);",
        "const DISK_INK: Color = Color::from_hex(0xCDD6F4);",
        ["every_disks_number_reads_on_it"],
    ),
    (
        "the records are the page's inks, 4.2:1 in the well",
        "            let on = c.chrome.on(c.chrome.well);",
        "            let on = c.chrome;",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "the solved banner has no ground of its own",
        "(h * 0.3).min(12.0), Surface::Panel);",
        "(h * 0.3).min(12.0), Surface::Card);",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "the help sheet has no ground of its own",
        "        palette.push_surface(f, p.x, p.y, p.w, p.h, 10.0, Surface::Panel);",
        "        palette.push_surface(f, p.x, p.y, p.w, p.h, 10.0, Surface::Card);",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    # ── The history: a tree, walked with Alt+Z (C-Q24) ────────────────────
    (
        "Ctrl+Z is not an undo",
        "            HistoryKey::Undo => self.undo(),",
        "            HistoryKey::Undo => false,",
        [CTRL],
    ),
    (
        "Ctrl+Y is not a redo",
        "            HistoryKey::Redo => self.redo(),",
        "            HistoryKey::Redo => false,",
        [CTRL],
    ),
    (
        "Alt+Z goes nowhere",
        "            HistoryKey::Earlier => self.earlier(),",
        "            HistoryKey::Earlier => false,",
        [TREE, WAITS],
    ),
    (
        "Alt+Shift+Z goes back too",
        "            HistoryKey::Later => self.later(),",
        "            HistoryKey::Later => self.earlier(),",
        [TREE],
    ),
    (
        "redo undoes",
        "        let Some((from, to)) = self.history.redo() else {",
        "        let Some((from, to)) = self.history.undo() else {",
        [TREE, CTRL],
    ),
    (
        "Alt+Z only undoes",
        "        let steps = self.history.earlier();",
        "        let steps: Vec<Travel<(usize, usize)>> =\n"
        "            self.history.undo().map(Travel::Undo).into_iter().collect();",
        [TREE],
    ),
    (
        "Alt+Shift+Z only redoes",
        "        let steps = self.history.later();",
        "        let steps: Vec<Travel<(usize, usize)>> =\n"
        "            self.history.redo().map(Travel::Redo).into_iter().collect();",
        [TREE],
    ),
    (
        "a journey takes its steps back the wrong way",
        "                Travel::Undo((from, to)) => self.take_back(from, to),",
        "                Travel::Undo((from, to)) => self.make_again(from, to),",
        [TREE],
    ),
    (
        "a move made again is not counted",
        "        self.moves = self.moves.saturating_add(1);\n        self.cursor = to;\n    }",
        "        self.cursor = to;\n    }",
        [TREE, CTRL],
    ),
    (
        "a move taken back is still counted",
        "        self.moves = self.moves.saturating_sub(1);\n        self.cursor = from;",
        "        self.cursor = from;",
        [TREE, "undo_takes_back_one_move_per_press"],
    ),
    (
        "the history moves with a disk in the air",
        "        self.playing() && self.held.is_none()\n    }",
        "        self.playing()\n    }",
        [WAITS, "undo_is_refused_with_a_disk_in_the_air"],
    ),
    (
        "the history's keys leave the solver playing",
        "    fn history_key(&mut self, key: HistoryKey) -> bool {\n        let stopped = self.interrupt();",
        "    fn history_key(&mut self, key: HistoryKey) -> bool {\n        let stopped = false;",
        [WAITS],
    ),
    (
        "the history's keys reach the board behind the sheet",
        "            if !self.show_help {\n                self.history_key(key);",
        "            if true {\n                self.history_key(key);",
        ["the_help_sheet_keeps_the_history_keys_too"],
    ),
    (
        "a key held with Ctrl, Alt or the Windows key is a bare key",
        "        if m.ctrl || m.alt || m.super_key {\n            return EventResult::Ignored;\n        }",
        "",
        [ALTGR],
    ),
    (
        "the history keeps more than its limit",
        "match core::num::NonZeroUsize::new(10_000) {",
        "match core::num::NonZeroUsize::new(10_100) {",
        ["the_history_keeps_its_last_moves"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "towers", timeout=240, only=sys.argv[1:] or None))

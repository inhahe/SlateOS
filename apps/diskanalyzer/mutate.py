"""Mutation test for the disk analyzer's command line.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-28: `diskanalyzer <folder>` handed its folder
to `app::launch`, which refuses every argument it does not take itself, so the
file manager's "Analyze disk usage" printed an error and opened nothing.  The
folder is read by `scan_root_from` now, one at most.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROOT = "the_folder_named_on_the_command_line_is_the_root"
CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "the folder named is not the one scanned",
        "    let root = words.next().map(PathBuf::from);",
        '    let root = words.next().map(|_| PathBuf::from("/"));',
        [ROOT],
    ),
    (
        "a second folder is taken",
        "    if let Some(extra) = words.next() {",
        "    if let Some(extra) = None::<&std::ffi::OsString> {",
        [ROOT],
    ),
    # -- the shortcut card's hold on the pointer
    (
        "a press goes through the shortcut card",
        "                MouseEventKind::Press(_) if self.show_help => {\n"
        "                    self.show_help = false;\n"
        "                    Action::Redraw\n"
        "                }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the card away",
        "                MouseEventKind::Press(_) if self.show_help => {\n",
        "                MouseEventKind::Press(MouseButton::Left) if self.show_help => {\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the card covers",
        "                MouseEventKind::Scroll { .. } if self.show_help => Action::None,\n",
        "",
        [CARD],
    ),
    (
        "the probe goes round the window's own way in",
        "        self.handle_event(\n"
        "            &Event::Mouse(MouseEvent {\n"
        "                x,\n"
        "                y,\n"
        "                kind: MouseEventKind::Press(button),\n"
        "            }),\n"
        "            size,\n"
        "        )\n",
        "        self.handle_click(x, y, button, size)\n",
        [CARD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "diskanalyzer", timeout=900, only=only))

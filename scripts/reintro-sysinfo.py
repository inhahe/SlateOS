"""Reintroduction check for the sysinfo scroll fixes.

Reverts each fixed defect one at a time and asserts that at least one test
fails. A defect that can be put back with every test still green is a defect
the suite does not actually pin -- which is what "N tests pass" hides.

Run from the lane-c worktree root:
    python scripts/reintro-sysinfo.py
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

SRC = Path("apps/sysinfo/src/main.rs")

# (name, old, new) -- `old` must occur exactly once.
DEFECTS = [
    (
        "wheel notches rounded per event instead of accumulated",
        "let rows = self.tree_wheel.rows(*dy);",
        "#[allow(clippy::cast_possible_truncation)]\n"
        "                    let rows = (-dy * wheel::ROWS_PER_NOTCH) as isize;",
    ),
    (
        "sidebar wheel unbounded at the far end",
        "scroll_window::shift(self.tree_scroll, rows).min(self.max_tree_scroll());",
        "scroll_window::shift(self.tree_scroll, rows);",
    ),
    (
        "detail wheel unbounded at the far end",
        "self.detail_scroll = scroll_window::shift(self.detail_scroll, rows)\n"
        "                        .min(self.max_detail_scroll());",
        "self.detail_scroll = scroll_window::shift(self.detail_scroll, rows);",
    ),
    (
        "one accumulator shared by both panes",
        "let rows = self.detail_wheel.rows(*dy);",
        "let rows = self.tree_wheel.rows(*dy);",
    ),
    (
        "hit test forgets the scroll offset",
        "let row = self.tree_scroll.checked_add(slot)?;",
        "let row = slot;",
    ),
    (
        "hit test not ranged to the pane",
        "if !offset.is_finite() || offset < 0.0 || offset >= self.pane_height() {\n"
        "            return None;\n"
        "        }",
        "if !offset.is_finite() || offset < 0.0 {\n            return None;\n        }",
    ),
    (
        "PageDown unbounded",
        "self.detail_scroll = self\n"
        "                    .detail_scroll\n"
        "                    .saturating_add(page)\n"
        "                    .min(self.max_detail_scroll());",
        "self.detail_scroll = self.detail_scroll.saturating_add(page);",
    ),
    (
        "selection not scrolled into view (down)",
        "            self.detail_scroll = 0;\n"
        "        }\n"
        "        self.scroll_selection_into_view();\n"
        "    }\n"
        "\n"
        "    /// Select the previous visible tree row.",
        "            self.detail_scroll = 0;\n"
        "        }\n"
        "    }\n"
        "\n"
        "    /// Select the previous visible tree row.",
    ),
    (
        "row stripes keyed on the screen slot",
        "let row_bg = if idx % 2 == 0 {",
        "let row_bg = if slot % 2 == 0 {",
    ),
]


def run_tests() -> tuple[bool, str]:
    p = subprocess.run(
        [
            sys.executable,
            "scripts/run-timeout.py",
            "300",
            "cargo",
            "test",
            "-p",
            "sysinfo-app",
            "--target",
            "x86_64-pc-windows-gnu",
        ],
        capture_output=True,
        text=True,
        errors="replace",
    )
    out = p.stdout + p.stderr
    failed = [
        line.strip()
        for line in out.splitlines()
        if line.strip().startswith("test tests::") and "FAILED" in line
    ]
    if not failed:
        # A red run with no named test is not evidence the defect is pinned --
        # it may be that the reverted code simply did not compile, which proves
        # nothing about the suite. Surface the compiler's own words so the two
        # cases can be told apart instead of both printing "failed".
        failed = [line.strip() for line in out.splitlines() if line.startswith("error")][:3]
    return p.returncode == 0, "; ".join(failed)


def main() -> int:
    original = SRC.read_text(encoding="utf-8", newline="")
    bad = []
    try:
        for name, old, new in DEFECTS:
            count = original.count(old)
            if count != 1:
                print(f"SETUP ERROR: {name!r} anchor occurs {count} times")
                bad.append(name)
                continue
            SRC.write_text(original.replace(old, new), encoding="utf-8", newline="")
            passed, failures = run_tests()
            if passed:
                print(f"NOT PINNED: {name}")
                bad.append(name)
            else:
                print(f"pinned by: {failures or '(build/other failure)'}  <- {name}")
    finally:
        SRC.write_text(original, encoding="utf-8", newline="")

    if bad:
        print(f"\n{len(bad)} defect(s) not pinned by the suite")
        return 1
    print(f"\nall {len(DEFECTS)} defects pinned")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

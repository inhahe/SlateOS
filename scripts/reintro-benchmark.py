"""Reintroduction check for the benchmark scroll fixes.

Reverts each fixed defect one at a time and asserts that at least one test
fails. A defect that can be put back with every test still green is a defect
the suite does not actually pin -- which is what "N tests pass" hides.

Run from the lane-c worktree root:
    python scripts/reintro-benchmark.py
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

SRC = Path("apps/benchmark/src/main.rs")

# (name, old, new) -- `old` must occur exactly once.
DEFECTS = [
    (
        "wheel notches multiplied by a pixel constant",
        "self.scroll_by(wheel::pixels(dy, ROW_HEIGHT));",
        "self.scroll_by(dy * 20.0);",
    ),
    (
        "scroll offset unbounded at the far end",
        "self.scroll_y = (self.scroll_y + delta).clamp(0.0, self.max_scroll());",
        "self.scroll_y = (self.scroll_y + delta).max(0.0);",
    ),
    (
        "content measured from wherever it is already scrolled to",
        "self.render_active_tab(&mut scratch, 0.0);",
        "self.render_active_tab(&mut scratch, self.scroll_y);",
    ),
    (
        "no End key: the wheel is the only way past the fold",
        "            Key::End => {\n"
        "                self.scroll_y = self.max_scroll();\n"
        "                EventResult::Consumed\n"
        "            }\n",
        "",
    ),
    (
        "no Down key",
        "            Key::Down => {\n"
        "                self.scroll_by(ROW_HEIGHT);\n"
        "                EventResult::Consumed\n"
        "            }\n",
        "",
    ),
    (
        "PageDown unbounded",
        "                self.scroll_by(self.page_step());",
        "                self.scroll_y += self.page_step();",
    ),
    (
        "a page is a constant instead of the pane that is showing",
        "        (self.content_height() - ROW_HEIGHT).max(ROW_HEIGHT)",
        "        100.0",
    ),
    (
        "hit test skips only the title, not the summary and header",
        "        let offset = y - self.history_rows_top();",
        "        let offset = y\n"
        "            - (self.content_top() + CONTENT_PADDING - self.scroll_y\n"
        "                + HISTORY_TITLE_ROW);",
    ),
    (
        "hit test forgets the scroll offset",
        "        self.content_top() + CONTENT_PADDING - self.scroll_y\n"
        "            + HISTORY_TITLE_ROW",
        "        self.content_top() + CONTENT_PADDING + HISTORY_TITLE_ROW",
    ),
    (
        "hit test not ranged to the pane",
        "        if y < self.content_top() || y >= self.content_bottom_edge() {\n"
        "            return None;\n"
        "        }\n",
        "",
    ),
    (
        "switching tabs keeps the old tab's scroll offset",
        "                self.active_tab = tab;\n"
        "                self.scroll_y = 0.0;",
        "                self.active_tab = tab;",
    ),
]


def run_tests() -> tuple[bool, str]:
    p = subprocess.run(
        [
            sys.executable,
            "scripts/run-timeout.py",
            "600",
            "cargo",
            "test",
            "-p",
            "benchmark",
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

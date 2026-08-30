"""Reintroduction check for the spreadsheet layout/scroll/frozen-pane fixes.

Reverts each fixed defect one at a time and asserts that at least one test
fails. A defect that can be put back with every test still green is a defect
the suite does not actually pin -- which is what "N tests pass" hides.

Run from the lane-c worktree root:
    python scripts/reintro-spreadsheet.py
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

SRC = Path("apps/spreadsheet/src/main.rs")

# (name, old, new) -- `old` must occur exactly once.
DEFECTS = [
    (
        "columns positioned without regard to the frozen band",
        "        let unscrolled = ROW_HEADER_WIDTH + sheet.col_x_offset(col);\n"
        "        if col < sheet.frozen_cols {\n"
        "            unscrolled\n"
        "        } else {\n"
        "            unscrolled - self.scroll().x\n"
        "        }",
        "        ROW_HEADER_WIDTH + sheet.col_x_offset(col) - self.scroll().x",
    ),
    (
        "rows positioned without regard to the frozen band",
        "        let unscrolled = self.grid_top() + sheet.row_y_offset(row);\n"
        "        if row < sheet.frozen_rows {\n"
        "            unscrolled\n"
        "        } else {\n"
        "            unscrolled - self.scroll().y\n"
        "        }",
        "        self.grid_top() + sheet.row_y_offset(row) - self.scroll().y",
    ),
    (
        "hit test subtracts the offset from the frozen band too",
        "        let content_x = if x < self.frozen_right() {\n"
        "            x - ROW_HEADER_WIDTH\n"
        "        } else {\n"
        "            x - ROW_HEADER_WIDTH + self.scroll().x\n"
        "        };",
        "        let content_x = x - ROW_HEADER_WIDTH + self.scroll().x;",
    ),
    (
        "hit test saturates into the grid instead of missing",
        "        if x < ROW_HEADER_WIDTH || x >= self.grid_right() {\n"
        "            return None;\n"
        "        }\n"
        "        if y < grid_top || y >= self.grid_bottom() {\n"
        "            return None;\n"
        "        }",
        "        let x = x.max(ROW_HEADER_WIDTH);\n"
        "        let y = y.max(grid_top);",
    ),
    (
        "drag clamp bounds left inversion-prone",
        "        let right = (self.grid_right() - 1.0).max(ROW_HEADER_WIDTH);\n"
        "        let bottom = (self.grid_bottom() - 1.0).max(self.grid_top());",
        "        let right = self.grid_right() - 1.0;\n"
        "        let bottom = self.grid_bottom() - 1.0;",
    ),
    (
        "revealing a frozen cell scrolls the sheet back to the edge",
        "        if addr.col >= frozen_cols {",
        "        if true {",
    ),
    (
        "frozen band drawn under the band that scrolls beneath it",
        "        self.render_row_band(\n"
        "            cmds,\n"
        "            first_scrolling..last_scrolling.saturating_add(1),\n"
        "            frozen_bottom,\n"
        "            grid_bottom - frozen_bottom,\n"
        "        );\n"
        "\n"
        "        // The frozen rows, over the top.\n"
        "        self.render_row_band(\n"
        "            cmds,\n"
        "            0..frozen_rows.min(MAX_ROWS),\n"
        "            y_start,\n"
        "            frozen_bottom - y_start,\n"
        "        );",
        "        self.render_row_band(\n"
        "            cmds,\n"
        "            0..frozen_rows.min(MAX_ROWS),\n"
        "            y_start,\n"
        "            frozen_bottom - y_start,\n"
        "        );\n"
        "        self.render_row_band(\n"
        "            cmds,\n"
        "            first_scrolling..last_scrolling.saturating_add(1),\n"
        "            frozen_bottom,\n"
        "            grid_bottom - frozen_bottom,\n"
        "        );",
    ),
    (
        "frozen columns drawn under the ones that scroll beneath them",
        "        for row in rows.clone() {\n"
        "            for col in frozen_cols..MAX_COLS {\n"
        "                self.render_cell(cmds, col, row);\n"
        "            }\n"
        "        }",
        "        for row in rows.clone() {\n"
        "            for col in 0..MAX_COLS {\n"
        "                self.render_cell(cmds, col, row);\n"
        "            }\n"
        "        }",
    ),
    (
        "clip with a negative extent left to the backend",
        "            width: width.max(0.0),\n            height: height.max(0.0),",
        "            width,\n            height,",
    ),
    (
        "horizontal wheel converted with the vertical axis' helper",
        "wheel::pixels_x(*dx, DEFAULT_COL_WIDTH),",
        "wheel::pixels(*dx, DEFAULT_COL_WIDTH),",
    ),
    (
        "wheel accepts a non-finite delta",
        "        if dx.is_finite() {\n"
        "            self.scroll_mut().x += dx;\n"
        "        }\n"
        "        if dy.is_finite() {\n"
        "            self.scroll_mut().y += dy;\n"
        "        }",
        "        self.scroll_mut().x += dx;\n        self.scroll_mut().y += dy;",
    ),
    (
        "resize leaves the offset past the end of the sheet",
        "        self.window_height = height as f32;",
        "        self.window_height = height as f32;\n        return;\n        #[allow(unreachable_code)]",
    ),
    (
        "page step is a constant rather than the pane's own height",
        "    pub fn page_rows(&self) -> i32 {",
        "    pub fn page_rows(&self) -> i32 {\n        return 20;\n        #[allow(unreachable_code)]",
    ),
    (
        "page step counts the rows that never scroll",
        "let pane = (self.grid_height() - sheet.row_y_offset(sheet.frozen_rows)).max(0.0);",
        "let pane = self.grid_height().max(0.0);",
    ),
    (
        "scroll offset shared between sheets",
        "    pub fn scroll(&self) -> ScrollPosition {\n        self.active_sheet().scroll\n    }",
        "    pub fn scroll(&self) -> ScrollPosition {\n"
        "        self.sheets.get(0).map_or_else(ScrollPosition::new, |s| s.scroll)\n"
        "    }",
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
            "spreadsheet",
            "--target",
            "x86_64-pc-windows-gnu",
        ],
        capture_output=True,
        text=True,
        errors="replace",
        env={**os.environ, "CARGO_TARGET_DIR": "target-c"},
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

"""Reintroduction check for the credmanager scroll fixes.

Puts each fixed defect back one at a time and asserts that at least one test
fails. A defect that can be restored with every test still green is a defect
the suite does not actually pin -- which is exactly what "150 tests pass"
hides.

Run from the lane-c worktree root:
    python scripts/reintro-credmanager.py
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

SRC = Path("apps/credmanager/src/main.rs")

# (name, old, new) -- `old` must occur exactly once.
DEFECTS = [
    (
        "the entry list has no upper scroll bound",
        "            state.list_scroll = (state.list_scroll + wheel::pixels(dy, ROW_HEIGHT))\n"
        "                .clamp(0.0, state.max_list_scroll());",
        "            state.list_scroll =\n"
        "                (state.list_scroll + wheel::pixels(dy, ROW_HEIGHT)).max(0.0);",
    ),
    (
        "the detail panel has no upper scroll bound",
        "            state.detail_scroll = (state.detail_scroll + wheel::pixels(dy, DETAIL_LINE_HEIGHT))\n"
        "                .clamp(0.0, state.max_detail_scroll());",
        "            state.detail_scroll =\n"
        "                (state.detail_scroll + wheel::pixels(dy, DETAIL_LINE_HEIGHT)).max(0.0);",
    ),
    (
        "a notch is a flat 20 px again instead of three rows",
        "wheel::pixels(dy, ROW_HEIGHT)",
        "(-dy * 20.0)",
    ),
    (
        "the wheel reads only the sign of a trackpad's delta",
        "    let out = rows_f(dy) * row_h;",
        "    let out = -dy.signum() * ROWS_PER_NOTCH * row_h;",
    ),
    (
        "a non-finite delta reaches the offset",
        "pub fn pixels(dy: f32, row_h: f32) -> f32 {\n"
        "    if !dy.is_finite() || !row_h.is_finite() {\n"
        "        return 0.0;\n"
        "    }\n"
        "    let out = rows_f(dy) * row_h;\n"
        "    if out.is_finite() { out } else { 0.0 }\n"
        "}",
        "pub fn pixels(dy: f32, row_h: f32) -> f32 {\n"
        "    -dy * ROWS_PER_NOTCH * row_h\n"
        "}",
    ),
    (
        "the detail panel's measured height is discarded again",
        "    y + state.detail_scroll - y_start + pad\n}",
        "    let _ = (y, pad);\n    0.0\n}",
    ),
    (
        "Event::Resize is ignored, so the bounds never learn the new size",
        "            state.width = *width as f32;\n            state.height = *height as f32;\n            state.clamp_scroll();",
        "            let _ = (width, height);",
    ),
    (
        "the renderer and the hit test each carry their own row-zero constant",
        "    let y_start = TOOLBAR_HEIGHT + LIST_HEADER_HEIGHT;\n"
        "    let row_idx = ((my - y_start + state.list_scroll) / ROW_HEIGHT) as usize;",
        "    let y_start = TOOLBAR_HEIGHT + 44.0;\n"
        "    let row_idx = ((my - y_start + state.list_scroll) / ROW_HEIGHT) as usize;",
    ),
    (
        "selecting an entry keeps the previous one's scroll offset",
        "        state.show_password = false;\n"
        "        // A new entry's fields start at the top, not wherever the last one had\n"
        "        // been scrolled to.\n"
        "        state.detail_scroll = 0.0;",
        "        state.show_password = false;",
    ),
]

# `rows_f` and `pixels` live in the toolkit, not the app.
WHEEL = Path("gui/toolkit/src/wheel.rs")


def file_for(old: str) -> Path:
    return WHEEL if "row_h" in old else SRC


def run_tests() -> tuple[bool, str]:
    p = subprocess.run(
        [
            sys.executable,
            "scripts/run-timeout.py",
            "600",
            "cargo",
            "test",
            "-p",
            "credmanager",
            "--target",
            "x86_64-pc-windows-gnu",
            "-j",
            "2",
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
        # the reverted code may simply not have compiled, which proves nothing
        # about the suite. Surface the compiler's own words so the two cases
        # can be told apart instead of both printing "failed".
        failed = [line.strip() for line in out.splitlines() if line.startswith("error")][:3]
    return p.returncode == 0, "; ".join(failed)


def main() -> int:
    originals = {
        SRC: SRC.read_text(encoding="utf-8", newline=""),
        WHEEL: WHEEL.read_text(encoding="utf-8", newline=""),
    }
    bad = []
    try:
        for name, old, new in DEFECTS:
            path = file_for(old)
            original = originals[path]
            nl = "\r\n" if "\r\n" in original else "\n"
            body = original.replace("\r\n", "\n")
            count = body.count(old)
            if count != 1:
                print(f"SETUP ERROR: {name!r} anchor occurs {count} times")
                bad.append(name)
                continue
            patched = body.replace(old, new)
            path.write_text(
                patched.replace("\n", nl) if nl == "\r\n" else patched,
                encoding="utf-8",
                newline="",
            )
            passed, failures = run_tests()
            path.write_text(original, encoding="utf-8", newline="")
            if passed:
                print(f"NOT PINNED: {name}")
                bad.append(name)
            else:
                print(f"pinned by: {failures or '(build/other failure)'}  <- {name}")
    finally:
        for path, original in originals.items():
            path.write_text(original, encoding="utf-8", newline="")

    if bad:
        print(f"\n{len(bad)} defect(s) not pinned by the suite")
        return 1
    print(f"\nall {len(DEFECTS)} defects pinned")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

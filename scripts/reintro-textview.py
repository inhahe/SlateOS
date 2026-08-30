"""Reintroduction check for the toolkit/desktop wheel-unit fixes.

Puts each fixed defect back one at a time and asserts that at least one test
fails. A defect that can be restored with every test still green is a defect
the suite does not actually pin -- which is exactly what "N tests pass" hides.

These four defects all had the same root cause: `MouseEventKind::Scroll`
carries *notches*, and each handler treated it as a pixel measurement. Two of
them divided a notch by a line height, which truncates to zero -- so the wheel
scrolled those widgets by nothing at all, ever.

Run from the lane-c worktree root:
    python scripts/reintro-textview.py
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

TEXTVIEW = Path("gui/toolkit/src/textview.rs")
DESKTOP = Path("gui/desktop/src/main.rs")

# (crate, path, name, old, new) -- `old` must occur exactly once in `path`.
DEFECTS = [
    (
        "guitk",
        TEXTVIEW,
        "the selectable text view divides a notch by a line height again",
        "            MouseEventKind::Scroll { dy, .. } => {\n"
        "                self.scroll_by_wheel(*dy);\n"
        "                (EventResult::Consumed, None)\n"
        "            }\n"
        "            _ => (EventResult::Ignored, None),\n"
        "        }\n"
        "    }\n"
        "\n"
        "    /// Scroll by one wheel event's worth of `dy`, in notches.",
        "            MouseEventKind::Scroll { dy, .. } => {\n"
        "                let lines = (dy / self.config.line_height).round() as i32;\n"
        "                self.scroll_by(lines.saturating_neg());\n"
        "                (EventResult::Consumed, None)\n"
        "            }\n"
        "            _ => (EventResult::Ignored, None),\n"
        "        }\n"
        "    }\n"
        "\n"
        "    /// Scroll by one wheel event's worth of `dy`, in notches.",
    ),
    (
        "guitk",
        TEXTVIEW,
        "the non-selectable arm divides a notch by a line height again",
        "            if let MouseEventKind::Scroll { dy, .. } = event.kind {\n"
        "                self.scroll_by_wheel(dy);",
        "            if let MouseEventKind::Scroll { dy, .. } = event.kind {\n"
        "                let lines = (dy / self.config.line_height).round() as i32;\n"
        "                self.scroll_by(lines.saturating_neg());",
    ),
    (
        "guitk",
        TEXTVIEW,
        "the text view rounds each event instead of banking the fraction",
        "        let rows = self.wheel.rows(dy);",
        "        let rows = wheel::rows_f(dy).trunc() as isize;",
    ),
    (
        "guitk",
        TEXTVIEW,
        "rich text scrolls three pixels per notch instead of three lines",
        "                self.scroll_by_px(wheel::pixels(*dy, self.config.line_height));",
        "                self.scroll_by_px(-dy * 3.0);",
    ),
    (
        "guitk",
        TEXTVIEW,
        "a non-finite delta reaches the rich text offset",
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
        "desktop",
        DESKTOP,
        "the start menu moves one row per event regardless of the delta",
        "fn scroll_rows(acc: &mut wheel::Accumulator, dy: f32) -> i32 {\n"
        "    let rows = acc.rows(dy);\n"
        "    i32::try_from(rows).unwrap_or(if rows < 0 { i32::MIN } else { i32::MAX })\n"
        "}",
        "fn scroll_rows(acc: &mut wheel::Accumulator, dy: f32) -> i32 {\n"
        "    let _ = acc;\n"
        "    let whole = (dy / START_MENU_ROW_HEIGHT) as i32;\n"
        "    if whole != 0 {\n"
        "        -whole\n"
        "    } else if dy > 0.0 {\n"
        "        -1\n"
        "    } else if dy < 0.0 {\n"
        "        1\n"
        "    } else {\n"
        "        0\n"
        "    }\n"
        "}",
    ),
    (
        "desktop",
        DESKTOP,
        "reopening the start menu keeps the leftover fraction",
        "            self.start_menu_wheel.reset();",
        "",
    ),
    (
        "desktop",
        DESKTOP,
        "scroll_start_menu's sign is inverted again",
        "        let moved = if rows >= 0 {\n"
        "            self.start_menu_scroll\n"
        "                .saturating_add(rows.unsigned_abs() as usize)\n"
        "        } else {\n"
        "            self.start_menu_scroll\n"
        "                .saturating_sub(rows.unsigned_abs() as usize)\n"
        "        };",
        "        let moved = if rows >= 0 {\n"
        "            self.start_menu_scroll\n"
        "                .saturating_sub(rows.unsigned_abs() as usize)\n"
        "        } else {\n"
        "            self.start_menu_scroll\n"
        "                .saturating_add(rows.unsigned_abs() as usize)\n"
        "        };",
    ),
]

WHEEL = Path("gui/toolkit/src/wheel.rs")


def run_tests(crate: str) -> tuple[bool, str]:
    p = subprocess.run(
        [
            sys.executable,
            "scripts/run-timeout.py",
            "600",
            "cargo",
            "test",
            "-p",
            crate,
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
        if line.strip().startswith("test ") and "FAILED" in line
    ]
    if not failed:
        # A red run with no named test is not evidence the defect is pinned --
        # the reverted code may simply not have compiled, which proves nothing
        # about the suite. Surface the compiler's own words so the two cases
        # can be told apart instead of both printing "failed".
        failed = [line.strip() for line in out.splitlines() if line.startswith("error")][:3]
    return p.returncode == 0, "; ".join(failed[:3])


def main() -> int:
    paths = [TEXTVIEW, DESKTOP, WHEEL]
    originals = {p: p.read_text(encoding="utf-8", newline="") for p in paths}
    bad = []
    try:
        for crate, path, name, old, new in DEFECTS:
            # The `pixels` guard lives in the toolkit's wheel module, not in the
            # widget that calls it.
            target = WHEEL if old.startswith("pub fn pixels") else path
            original = originals[target]
            nl = "\r\n" if "\r\n" in original else "\n"
            body = original.replace("\r\n", "\n")
            count = body.count(old)
            if count != 1:
                print(f"SETUP ERROR: {name!r} anchor occurs {count} times")
                bad.append(name)
                continue
            patched = body.replace(old, new)
            target.write_text(
                patched.replace("\n", nl) if nl == "\r\n" else patched,
                encoding="utf-8",
                newline="",
            )
            passed, failures = run_tests(crate)
            target.write_text(original, encoding="utf-8", newline="")
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

"""Reintroduction check for the scrollable-pane fixes.

Puts each fixed defect back one at a time and asserts that at least one test
fails. A defect that can be restored with every test still green is a defect
the suite does not actually pin -- which is exactly what "N tests pass" hides.

Three families of defect are covered:

1. `MouseEventKind::Scroll` carries *notches*, and each of these handlers
   treated it as a pixel measurement -- so a full detent of a real mouse wheel
   moved the list by one pixel, or by three, out of a row height of fifty-odd.
2. A scroll offset clamped at one end only. `.max(0.0)` stops the list going
   above its start and says nothing about its end, so the wheel walked every
   one of these lists off into empty space indefinitely.
3. The same layout arithmetic written twice -- once by the renderer, once by
   the hit test -- which is how a list ends up drawn in one place and clicked
   in another.

Run from the lane-c worktree root:
    python scripts/reintro-scroll-panes.py
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

NOTIF_PANE = Path("gui/desktop/src/notif_pane.rs")
SETTINGS = Path("apps/settings/src/main.rs")
UNITCONV = Path("apps/unitconverter/src/main.rs")
NOTIFD = Path("gui/notifications/src/main.rs")

# (crate, path, name, old, new) -- `old` must occur exactly once in `path`.
DEFECTS = [
    # ---------------------------------------------------------------- shade
    (
        "desktop",
        NOTIF_PANE,
        "the notification shade hit-tests 76 px from where it paints",
        "const QUICK_SETTINGS_HEIGHT: f32 = QS_TITLE_HEIGHT\n"
        "    + QuickSetting::COUNT as f32 * QS_ROW_HEIGHT\n"
        "    + QS_SLIDER_GAP\n"
        "    + 2.0 * QS_ROW_HEIGHT;",
        "const QUICK_SETTINGS_HEIGHT: f32 = 200.0;",
    ),
    (
        "desktop",
        NOTIF_PANE,
        "the shade's card hit test sits 76 px from where the card is painted",
        "        let list_top = Self::list_start_y();",
        "        let list_top = Self::list_start_y() - 76.0;",
    ),
    (
        "desktop",
        NOTIF_PANE,
        "the shade's wheel is a private thirty pixels a notch",
        "self.scroll_offset += wheel::pixels(*dy, NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING);",
        "self.scroll_offset -= dy * 30.0;",
    ),
    (
        "desktop",
        NOTIF_PANE,
        "the shade's scroll offset is clamped at one end only",
        "    fn clamp_scroll(&mut self) {\n"
        "        self.scroll_offset = self.scroll_offset.clamp(0.0, self.max_scroll());\n"
        "    }",
        "    fn clamp_scroll(&mut self) {\n"
        "        self.scroll_offset = self.scroll_offset.max(0.0);\n"
        "    }",
    ),
    (
        "desktop",
        NOTIF_PANE,
        "a shade page is a constant rather than the pane's own height",
        "                    self.scroll_offset += self.list_height();",
        "                    self.scroll_offset += 200.0;",
    ),
    # ------------------------------------------------------------- settings
    (
        "settings",
        SETTINGS,
        "a settings dropdown moves three items per event regardless of the delta",
        "                let rows = self.dropdown_wheel.rows(*dy);\n"
        "                self.scroll_dropdown_by(rows);",
        "                let _ = &mut self.dropdown_wheel;\n"
        "                self.scroll_dropdown_by(if *dy > 0.0 { -3 } else { 3 });",
    ),
    (
        "settings",
        SETTINGS,
        "reopening a settings dropdown keeps the leftover fraction",
        "        self.dropdown_wheel.reset();",
        "",
    ),
    # -------------------------------------------------------- unitconverter
    (
        "unitconverter",
        UNITCONV,
        "the converter's history scrolls one pixel per notch",
        "self.scroll_history_by(wheel::pixels(*dy, HISTORY_ITEM_HEIGHT));",
        "self.scroll_history_by(-dy);",
    ),
    (
        "unitconverter",
        UNITCONV,
        "the converter's history has no upper scroll bound",
        "        self.history_scroll = (self.history_scroll + delta).clamp(0.0, self.max_history_scroll());",
        "        self.history_scroll = (self.history_scroll + delta).max(0.0);",
    ),
    # --------------------------------------------------- notification daemon
    (
        "notifications",
        NOTIFD,
        "the notification centre scrolls one pixel per notch",
        "self.scroll_center_by(wheel::pixels(*dy, CENTER_ITEM_HEIGHT));",
        "self.scroll_center_by(-dy);",
    ),
    (
        "notifications",
        NOTIFD,
        "the notification centre has no upper scroll bound",
        "        self.center.scroll_offset = self\n"
        "            .center\n"
        "            .scroll_offset\n"
        "            .clamp(0.0, self.max_center_scroll());",
        "        self.center.scroll_offset = self.center.scroll_offset.max(0.0);",
    ),
    (
        "notifications",
        NOTIFD,
        "the centre's hit test ignores the clip and lets a hidden row be clicked",
        "        if my >= list_top {",
        "        if true {",
    ),
    (
        "notifications",
        NOTIFD,
        "the centre's hit test walks the list itself instead of reading the layout",
        "        if my >= list_top {\n"
        "            for (top, row) in self.center_rows() {\n"
        "                let y = content_y + top;",
        "        if my >= list_top {\n"
        "            for (top, row) in self.center_rows() {\n"
        "                let y = content_y + top + CENTER_GROUP_HEADER_HEIGHT;",
    ),
]


def run_tests(crate: str) -> tuple[bool, str]:
    p = subprocess.run(
        [
            sys.executable,
            "scripts/run-timeout.py",
            "900",
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
    paths = sorted({path for _, path, _, _, _ in DEFECTS})
    originals = {p: p.read_text(encoding="utf-8", newline="") for p in paths}
    bad = []
    try:
        for crate, path, name, old, new in DEFECTS:
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
            passed, failures = run_tests(crate)
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
